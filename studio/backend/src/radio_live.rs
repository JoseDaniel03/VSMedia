//! Bounded DJ PCM ingress from browser and low-latency local Plasmo pull.
//! Browser -> Axum is loopback-only; Plasmo addon is the single local consumer.
//! Never archives microphone audio or exposes a remotely reachable stream.
use axum::{body::Bytes,extract::{Extension,Path,State},
    http::{header,HeaderMap,StatusCode},response::{IntoResponse,Response},Json};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{collections::{HashMap,VecDeque},sync::{Arc,Mutex},
    time::{Duration,Instant,SystemTime,UNIX_EPOCH}};
use crate::{Store,ApiError,authorize_origin,bad};
use crate::radio;

const MAX_STATIONS:usize=8;
const MAX_CHUNKS:usize=24;
const MAX_CHUNK_BYTES:usize=19200; // 200ms PCM16 mono 48k
const STALE:Duration=Duration::from_secs(3);

#[derive(Clone,Default)]
pub struct DjHub(Arc<Mutex<HashMap<String,DjStream>>>);

struct DjStream{
    token:String,
    mode:String,
    chunks:VecDeque<(u64,Vec<u8>)>,
    seq:u64,
    last_packet:Instant,
    last_upload:Option<Instant>,
}
fn current_ms()->u64{
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
fn valid_station(id:&str)->bool{
    !id.is_empty() && id.len()<=48 &&
        id.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'-'||b==b'_')
}
fn lock(hub:&DjHub)->Result<std::sync::MutexGuard<'_,HashMap<String,DjStream>>,ApiError>{
    hub.0.lock().map_err(|_|ApiError(StatusCode::INTERNAL_SERVER_ERROR,"DJ hub unavailable".into()))
}
fn authorized(headers:&HeaderMap,token:&str)->bool{
    let supplied=headers.get("x-vsradio-session").and_then(|x|x.to_str().ok()).unwrap_or("");
    supplied.len()==token.len() && supplied.as_bytes().iter().zip(token.as_bytes())
        .fold(0u8,|acc,(a,b)|acc|(a^b))==0
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRequest {mode:String}
pub async fn start(State(store):State<Store>,Extension(hub):Extension<DjHub>,
    Path(id):Path<String>,headers:HeaderMap,Json(req):Json<StartRequest>)
    ->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;
    if !valid_station(&id)|| !matches!(req.mode.as_str(),"mic"|"mix"){
        return Err(bad("Emisora o modo inválido. Elegí mic o mix."));
    }
    radio::load(&store,&id)?;
    let mut map=lock(&hub)?;
    let tick=Instant::now();
    map.retain(|_,v|tick.duration_since(v.last_packet)<STALE);
    if map.contains_key(&id){
        return Err(ApiError(StatusCode::CONFLICT,
            "Ya hay un DJ emitiendo. Detené su sesión antes de tomar control.".into()));
    }
    if map.len()>=MAX_STATIONS{return Err(bad("Máximo 8 DJs simultáneos"));}
    let token=uuid::Uuid::new_v4().to_string();
    map.insert(id.clone(),DjStream{token:token.clone(),mode:req.mode,
        chunks:VecDeque::new(),seq:0,last_packet:tick,last_upload:None});
    Ok(Json(json!({"schema":"vsradio-dj-v1","station":id,
        "token":token,"sample_rate":48000,"channels":1,"chunk_ms_max":200})))
}
pub async fn push(Extension(hub):Extension<DjHub>,Path(id):Path<String>,
    headers:HeaderMap,body:Bytes)->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;
    if !valid_station(&id)||body.len()<1920||body.len()>MAX_CHUNK_BYTES
        ||body.len()%1920!=0{
        return Err(bad("Se aceptan lotes de 20-200ms PCM16 mono 48k (múltiplos de 1920 bytes)"));
    }
    let mut map=lock(&hub)?;
    let dj=map.get_mut(&id).ok_or_else(||ApiError(StatusCode::NOT_FOUND,
        "Emisión no iniciada o vencida".into()))?;
    if !authorized(&headers,&dj.token){
        return Err(ApiError(StatusCode::FORBIDDEN,"Token de emisión inválido".into()));
    }
    let now=Instant::now();
    if dj.last_upload.is_some_and(|prev|now.duration_since(prev)<Duration::from_millis(35)){
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS,"Demasiados lotes por segundo".into()));
    }
    dj.last_upload=Some(now);
    dj.last_packet=now;
    dj.seq+=1;
    dj.chunks.push_back((dj.seq,body.to_vec()));
    while dj.chunks.len()>MAX_CHUNKS {dj.chunks.pop_front();}
    Ok(Json(json!({"ok":true,"seq":dj.seq,"queued_chunks":dj.chunks.len()})))
}
pub async fn stop(Extension(hub):Extension<DjHub>,Path(id):Path<String>,
    headers:HeaderMap)->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;
    let mut map=lock(&hub)?;
    let dj=map.get(&id).ok_or_else(||ApiError(StatusCode::NOT_FOUND,
        "No hay transmisión activa".into()))?;
    if !authorized(&headers,&dj.token){return Err(ApiError(StatusCode::FORBIDDEN,
        "Token de emisión inválido".into()));}
    map.remove(&id);
    Ok(Json(json!({"ok":true,"station":id,"stopped":true})))
}
pub async fn status(Extension(hub):Extension<DjHub>,Path(id):Path<String>)
    ->Result<Json<Value>,ApiError>{
    if !valid_station(&id){return Err(bad("ID inválido"));}
    let map=lock(&hub)?;
    let dj=map.get(&id).filter(|dj|dj.last_packet.elapsed()<STALE
        &&dj.seq>0);
    Ok(Json(json!({"schema":"vsradio-live-v1","station":id,"active":dj.is_some(),
        "mode":dj.map(|x|x.mode.as_str()).unwrap_or("playlist"),
        "seq":dj.map(|x|x.seq).unwrap_or(0),"server_unix_ms":current_ms(),
        "buffered_chunks":dj.map(|x|x.chunks.len()).unwrap_or(0)})))
}
/// Binary protocol: 8-byte LE u64 sequence + ordered PCM bytes after last seen seq.
/// If the consumer falls behind, expired packets are dropped, never replayed.
pub async fn drain(Extension(hub):Extension<DjHub>,
    Path((id,after)):Path<(String,u64)>)->Result<Response,ApiError>{
    if !valid_station(&id){return Err(bad("ID inválido"));}
    let map=lock(&hub)?;
    let Some(dj)=map.get(&id).filter(|x|x.last_packet.elapsed()<STALE) else{
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    // Select the newest 600ms rather than replaying old buffered voice.
    // A listener that was disconnected should rejoin near LIVE, not hear stale audio.
    let mut chunks=dj.chunks.iter().rev().filter(|(seq,_)|*seq>after)
        .take(3).collect::<Vec<_>>();
    chunks.reverse();
    if chunks.is_empty(){return Ok(StatusCode::NO_CONTENT.into_response());}
    let seq=chunks.last().unwrap().0;
    let mut out=Vec::with_capacity(8+chunks.iter().map(|x|x.1.len()).sum::<usize>());
    out.extend_from_slice(&seq.to_le_bytes());
    for (_,data) in chunks {out.extend_from_slice(data);}
    Ok(([(header::CONTENT_TYPE,"application/octet-stream"),
        (header::CACHE_CONTROL,"no-store")],out).into_response())
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn valid_ids_and_audio_frame_size(){
        assert!(valid_station("latinsabroso"));
        assert!(!valid_station("../secret"));
        assert!(!valid_station("LatinSabroso"));
        assert_eq!(MAX_CHUNK_BYTES%1920,0);
    }
    #[test]
    fn dj_hub_is_bounded(){
        let hub=DjHub::default();
        let mut map=lock(&hub).unwrap();
        let tick=Instant::now();
        map.insert("radio".into(),DjStream{token:"abc".into(),mode:"mic".into(),
            chunks:VecDeque::new(),seq:0,last_packet:tick,last_upload:None});
        assert_eq!(map.len(),1);
    }
}
