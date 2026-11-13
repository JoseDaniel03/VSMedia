//! Persistent VSRadio stations with one UTC-based playlist clock for ALL listeners.
//! Audio is preconverted by the editor to 48kHz mono 16-bit PCM WAV. Folia/Plasmo
//! only sends frames; neither Minecraft regions nor Axum request threads decode audio.
use axum::{body::Bytes,extract::{Path,State},http::{header,HeaderMap,StatusCode},
    response::{IntoResponse,Response},Json};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::time::{SystemTime,UNIX_EPOCH};
use vsmedia_api::{AssetKey,AssetKind,AssetRepository};
use crate::{Store,ApiError,authorize_origin,bad,not_found};

const MAX_TRACK_BYTES:usize=32*1024*1024;
const MAX_TRACKS:usize=128;
const MAX_STATIONS:usize=32;
const MAX_ITEMS:usize=64;
const TRACK_PREFIX:&str="radio/tracks/";
const STATION_PREFIX:&str="radio/stations/";

fn valid_id(s:&str)->bool {
    (1..=48).contains(&s.len()) && s.bytes().all(|b|
        b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'-'||b==b'_')
}
fn key(prefix:&str,id:&str,suffix:&str)->Result<AssetKey,ApiError> {
    if !valid_id(id){return Err(bad("Identificador: minúsculas, números, - y _, máximo 48"));}
    AssetKey::new("studio",format!("{prefix}{id}{suffix}")).map_err(bad)
}
fn track_key(id:&str)->Result<AssetKey,ApiError>{key(TRACK_PREFIX,id,".wav")}
fn station_key(id:&str)->Result<AssetKey,ApiError>{key(STATION_PREFIX,id,".json")}
fn millis()->u64 {SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64}

/// Require exactly the WAV layout produced by the browser's Radio publisher.
/// Avoid trusting filenames or duration values supplied by an editor.
fn pcm<'a>(wav:&'a [u8])->Result<&'a [u8],ApiError> {
    if wav.len()<44||wav.len()>MAX_TRACK_BYTES
        || &wav[0..4]!=b"RIFF" || &wav[8..12]!=b"WAVE"
        || &wav[12..16]!=b"fmt " || u32::from_le_bytes(wav[16..20].try_into().unwrap())!=16
        || u16::from_le_bytes(wav[20..22].try_into().unwrap())!=1
        || u16::from_le_bytes(wav[22..24].try_into().unwrap())!=1
        || u32::from_le_bytes(wav[24..28].try_into().unwrap())!=48000
        || u32::from_le_bytes(wav[28..32].try_into().unwrap())!=96000
        || u16::from_le_bytes(wav[32..34].try_into().unwrap())!=2
        || u16::from_le_bytes(wav[34..36].try_into().unwrap())!=16
        || &wav[36..40]!=b"data" {
        return Err(bad("VSRadio necesita WAV PCM16 mono 48 kHz (header de 44 bytes)"));
    }
    let riff=u32::from_le_bytes(wav[4..8].try_into().unwrap()) as usize;
    let len=u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
    if riff!=wav.len()-8 || len!=wav.len()-44 || len%2!=0 || len<1920 {
        return Err(bad("WAV inconsistente o audio inferior a 20 ms"));
    }
    Ok(&wav[44..])
}
fn duration(wav:&[u8])->Result<u64,ApiError>{
    let data=pcm(wav)?;
    // Exact audio samples divided by 48 samples per millisecond.
    Ok((data.len() as u64/2)*1000/48000)
}

pub async fn upload_track(State(store):State<Store>,Path(id):Path<String>,
    headers:HeaderMap,body:Bytes)->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;
    let key=track_key(&id)?;
    let duration_ms=duration(&body)?;
    let records=store.list_assets("studio",TRACK_PREFIX,MAX_TRACKS).map_err(bad)?;
    if records.len()>=MAX_TRACKS && store.metadata(&key).is_err(){
        return Err(bad("Máximo 128 pistas de radio"));
    }
    let previous=store.metadata(&key).map(|r|r.size).unwrap_or(0);
    let total:u64=records.iter().map(|r|r.size).sum();
    if total.saturating_sub(previous).saturating_add(body.len() as u64)>384*1024*1024 {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,"Radio supera 384 MiB".into()));
    }
    let sha256=format!("{:x}",Sha256::digest(&body));
    if let Ok(existing)=store.metadata(&key) {
        if existing.sha256==sha256 && existing.kind==AssetKind::Sound {
            return Ok(Json(json!({"id":id,"sha256":existing.sha256,
                "duration_ms":duration_ms,"bytes":existing.size,"stored":true,
                "unchanged":true,"transmitting":false})));
        }
        // Never mutate an in-use immutable radio track; interrupted updates
        // must leave the old station and all its listener streams intact.
        return Err(ApiError(StatusCode::CONFLICT,
            "Pista existente con contenido distinto: usá un ID por hash".into()));
    }
    let record=store.publish(key,AssetKind::Sound,&body).map_err(not_found)?;
    Ok(Json(json!({"id":id,"sha256":record.sha256,"duration_ms":duration_ms,
        "bytes":record.size,"stored":true,"unchanged":false,"transmitting":false})))
}

pub async fn track_metadata(State(store):State<Store>,Path(id):Path<String>)
    ->Result<Json<Value>,ApiError>{
    let record=store.metadata(&track_key(&id)?).map_err(not_found)?;
    if record.kind!=AssetKind::Sound {return Err(bad("La pista no es audio"));}
    Ok(Json(json!({"id":id,"sha256":record.sha256,
        "bytes":record.size,"stored":true})))
}

pub async fn track_pcm(State(store):State<Store>,Path(id):Path<String>)->Result<Response,ApiError>{
    let key=track_key(&id)?;
    let wav=store.read(&key).map_err(not_found)?;
    let raw=pcm(&wav)?;
    Ok(([(header::CONTENT_TYPE,"application/octet-stream"),(header::CACHE_CONTROL,"no-store"),
        (header::X_CONTENT_TYPE_OPTIONS,"nosniff")],raw.to_vec()).into_response())
}

#[derive(Clone,Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
pub struct Station {
    pub id:String,
    pub display_name:String,
    pub playlist:Vec<String>,
    pub anchor_unix_ms:u64,
    pub cycle_ms:u64,
    pub revision:String,
    #[serde(default)]pub durations_ms:Vec<u64>,
    #[serde(default)]pub paused:bool,
    #[serde(default)]pub paused_phase_ms:u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StationRequest {
    pub display_name:String,
    pub playlist:Vec<String>,
    /// When Studio is currently playing a playlist, preserve its position.
    #[serde(default)]pub elapsed_ms:Option<u64>,
}
pub(crate) fn load(store:&Store,id:&str)->Result<Station,ApiError>{
    let data=store.read(&station_key(id)?).map_err(not_found)?;
    serde_json::from_slice(&data).map_err(bad)
}
fn playlist_durations(store:&Store,playlist:&[String])->Result<Vec<u64>,ApiError>{
    if playlist.is_empty() || playlist.len()>MAX_ITEMS{return Err(bad("La emisora necesita entre 1 y 64 canciones"));}
    let mut durations=Vec::with_capacity(playlist.len());
    for id in playlist {
        let wav=store.read(&track_key(id)?).map_err(not_found)?;
        durations.push(duration(&wav)?);
    }
    Ok(durations)
}
fn snapshot(station:&Station,durations:&[u64],now:u64)->Value {
    let cycle=durations.iter().sum::<u64>();
    let phase=if station.paused {station.paused_phase_ms%cycle}
        else {now.saturating_sub(station.anchor_unix_ms)%cycle};
    let mut start=0;
    for (index,len) in durations.iter().enumerate() {
        if phase<start+len {
            let position=phase-start;
            return json!({
                "schema":"vsradio-now-v1","station":station.id,"display_name":station.display_name,
                "revision":station.revision,"playing":!station.paused,"paused":station.paused,
                "track":station.playlist[index],
                "index":index,"position_ms":position,"remaining_ms":len-position,
                "track_started_unix_ms":now-position,"server_unix_ms":now,"cycle_ms":cycle,
                "anchor_unix_ms":station.anchor_unix_ms,"playlist":station.playlist
            });
        }
        start+=len;
    }
    unreachable!("sum of durations > 0")
}
pub async fn publish_station(State(store):State<Store>,Path(id):Path<String>,
    headers:HeaderMap,Json(req):Json<StationRequest>)->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;
    let key=station_key(&id)?;
    if req.display_name.trim().is_empty()||req.display_name.len()>80
        || req.display_name.chars().any(char::is_control) {
        return Err(bad("Nombre de emisora inválido"));
    }
    let durations=playlist_durations(&store,&req.playlist)?;
    let cycle=durations.iter().sum::<u64>();
    if cycle==0||cycle>24*60*60*1000{return Err(bad("La playlist supera 24 horas o no tiene audio"));}
    let records=store.list_assets("studio",STATION_PREFIX,MAX_STATIONS).map_err(bad)?;
    if records.len()>=MAX_STATIONS && store.metadata(&key).is_err() {
        return Err(bad("Máximo 32 emisoras"));
    }
    let mut version=Sha256::new();
    for (id,len) in req.playlist.iter().zip(&durations){
        let rec=store.metadata(&track_key(id)?).map_err(not_found)?;
        version.update(id.as_bytes());version.update(rec.sha256.as_bytes());version.update(len.to_le_bytes());
    }
    let revision=format!("{:x}",version.finalize());
    let now=millis();
    let previous=load(&store,&id).ok();
    let anchor=if let Some(elapsed)=req.elapsed_ms {
        if elapsed>cycle{return Err(bad("Posición inicial fuera de la playlist"));}
        now.saturating_sub(elapsed)
    } else if let Some(old)=&previous {
        if old.revision==revision {old.anchor_unix_ms}else{now}
    } else {now};
    let station=Station{id:id.clone(),display_name:req.display_name,
        playlist:req.playlist,anchor_unix_ms:anchor,cycle_ms:cycle,revision:revision.clone(),
        durations_ms:durations.clone(),
        paused:req.elapsed_ms.is_none() && previous.as_ref().is_some_and(|p|p.revision==revision && p.paused),
        paused_phase_ms:previous.as_ref().filter(|p|p.revision==revision)
            .map(|p|p.paused_phase_ms).unwrap_or(0)};
    store.publish(key,AssetKind::Other,&serde_json::to_vec(&station).map_err(bad)?)
        .map_err(not_found)?;
    Ok(Json(snapshot(&station,&durations,now)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    action:String,
    #[serde(default)]index:Option<usize>,
    #[serde(default)]position_ms:Option<u64>,
}
pub async fn control_station(State(store):State<Store>,Path(id):Path<String>,
    headers:HeaderMap,Json(req):Json<ControlRequest>)
    ->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;
    let key=station_key(&id)?;
    let mut station=load(&store,&id)?;
    let times=if station.durations_ms.len()==station.playlist.len()
        && station.durations_ms.iter().all(|v|*v>0) {
        station.durations_ms.clone()
    } else {playlist_durations(&store,&station.playlist)?};
    let cycle=times.iter().sum::<u64>();
    if cycle==0 {return Err(bad("La emisora no tiene audio"));}
    let now=millis();
    let phase=if station.paused {station.paused_phase_ms%cycle}
        else {now.saturating_sub(station.anchor_unix_ms)%cycle};
    let mut elapsed=0u64;
    let current_index=times.iter().position(|length|{
        let hit=phase<elapsed+length;
        if !hit {elapsed+=length;}
        hit
    }).unwrap_or(0);
    match req.action.as_str(){
        "pause"=>{
            station.paused_phase_ms=phase;
            station.paused=true;
        }
        "resume"=>{
            station.anchor_unix_ms=now.saturating_sub(phase);
            station.paused=false;
        }
        "next"|"previous"|"seek"=>{
            let index=match req.action.as_str(){
                "next"=>(current_index+1)%times.len(),
                "previous"=>(current_index+times.len()-1)%times.len(),
                _=>req.index.ok_or_else(||bad("seek requiere index"))?,
            };
            if index>=times.len() {return Err(bad("Índice de canción fuera de rango"));}
            let pos=if req.action=="seek" {req.position_ms.unwrap_or(0)}else{0};
            if pos>=times[index] {return Err(bad("Posición fuera de duración"));}
            let next=times[..index].iter().sum::<u64>()+pos;
            station.anchor_unix_ms=now.saturating_sub(next);
            station.paused_phase_ms=next;
        }
        _=>return Err(bad("Acción permitida: pause, resume, previous, next, seek"))
    }
    store.publish(key,AssetKind::Other,&serde_json::to_vec(&station).map_err(bad)?)
        .map_err(not_found)?;
    Ok(Json(snapshot(&station,&times,now)))
}

pub async fn list_stations(State(store):State<Store>)->Result<Json<Value>,ApiError>{
    let records=store.list_assets("studio",STATION_PREFIX,MAX_STATIONS).map_err(bad)?;
    let mut stations=Vec::new();
    for record in records {
        let doc:Station=serde_json::from_slice(&store.read(&record.key).map_err(not_found)?)
            .map_err(bad)?;
        stations.push(json!({"id":doc.id,"display_name":doc.display_name,
            "tracks":doc.playlist.len(),"revision":doc.revision}));
    }
    Ok(Json(json!({"schema":"vsradio-stations-v1","stations":stations})))
}
pub async fn now(State(store):State<Store>,Path(id):Path<String>)->Result<Json<Value>,ApiError>{
    let station=load(&store,&id)?;
    // Stable cached durations make frequent live clock polls O(number of tracks)
    // without repeatedly loading potentially hundreds of MiB of PCM from SQLite.
    let cached_valid=station.durations_ms.len()==station.playlist.len()
        && station.durations_ms.iter().all(|t|*t>0)
        && station.durations_ms.iter().sum::<u64>()==station.cycle_ms;
    let times=if cached_valid {station.durations_ms.clone()}
        else {playlist_durations(&store,&station.playlist)?};
    Ok(Json(snapshot(&station,&times,millis())))
}
#[cfg(test)]
mod tests{
    use super::*;
    fn wave(seconds:u32)->Vec<u8>{
        let count=seconds*48000;
        let mut b=Vec::new();
        b.extend_from_slice(b"RIFF");b.extend_from_slice(&(count*2+36).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&48000u32.to_le_bytes());b.extend_from_slice(&96000u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");b.extend_from_slice(&(count*2).to_le_bytes());
        b.resize(44+count as usize*2,0);b
    }
    #[test]fn validates_wav_and_duration(){
        let w=wave(2);assert_eq!(duration(&w).unwrap(),2000);
        let mut invalid=w.clone();invalid[22]=2;assert!(pcm(&invalid).is_err());
        let mut invalid=w;invalid[40]=1;assert!(pcm(&invalid).is_err());
    }
    #[test]fn timeline_joins_at_current_position_and_wraps(){
        let station=Station{id:"latinsabroso".into(),display_name:"LatinSabroso".into(),
            playlist:vec!["tema1".into(),"tema2".into()],anchor_unix_ms:1000,cycle_ms:6000,
            revision:"a".into(),durations_ms:vec![2000,4000],paused:false,paused_phase_ms:0};
        let at=snapshot(&station,&[2000,4000],4500);
        assert_eq!(at["track"],"tema2");assert_eq!(at["position_ms"],1500);
        assert_eq!(at["remaining_ms"],2500);
        let looped=snapshot(&station,&[2000,4000],8500);
        assert_eq!(looped["track"],"tema1");assert_eq!(looped["position_ms"],1500);
    }
    #[test]fn rejects_invalid_station_ids(){assert!(!valid_id("../path"));assert!(valid_id("latinsabroso"));}
}
