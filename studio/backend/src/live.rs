//! Ephemeral local-only WebRTC signaling for small VSRadio and VSStreams rooms.
//! This server NEVER receives audio, SDP is not persisted, and only peers in
//! the same validated room can exchange offer/answer/ICE messages.
//! Production public deployment requires TLS, an authenticated proxy and TURN.
use std::{collections::HashMap,sync::Arc};
use axum::{extract::{ws::{WebSocket,WebSocketUpgrade,Message},Extension},http::{HeaderMap,StatusCode},response::IntoResponse,Json};
use futures_util::{SinkExt,StreamExt};
use serde_json::{Value,json};
use tokio::sync::{Mutex,mpsc};
use crate::{authorize_origin,ApiError};

const MAX_MESSAGE:usize=48*1024;
const MAX_TOTAL_PEERS:usize=128;
const MAX_VOICE_PEERS:usize=8;
const MAX_RADIO_PEERS:usize=16;
const MAX_QUEUE:usize=64;

#[derive(Clone,Default)]
pub struct LiveHub(Arc<Mutex<Rooms>>);
#[derive(Default)]
struct Rooms {
    next:u64,
    members:HashMap<u64,Member>,
}
struct Member {
    room:String,
    mode:String,
    role:String,
    tx:mpsc::Sender<String>,
}
fn validate_room(room:&str)->Result<(),String>{
    if !(8..=64).contains(&room.len())
        || !room.bytes().all(|b|b.is_ascii_alphanumeric() || b=='-' as u8 || b=='_' as u8) {
        return Err("Código de sala inválido: 8–64 caracteres alfanuméricos, - o _".into());
    }
    Ok(())
}
fn validate_join(mode:&str,role:&str)->Result<(),String> {
    match (mode,role){
        ("radio","host")|("radio","listener")|("voice","speaker")=>Ok(()),
        _=>Err("Modo o rol no admitido".into())
    }
}
fn emit(tx:&mpsc::Sender<String>,message:Value){
    let _=tx.try_send(message.to_string());
}
impl LiveHub {
    async fn join(&self,room:&str,mode:&str,role:&str,tx:mpsc::Sender<String>)->Result<u64,String>{
        validate_room(room)?;
        validate_join(mode,role)?;
        let mut state=self.0.lock().await;
        if state.members.len()>=MAX_TOTAL_PEERS {
            return Err("Servidor local completo".into());
        }
        let existing=state.members.iter()
            .filter(|(_,member)|member.room==room)
            .collect::<Vec<_>>();
        if existing.iter().any(|(_,member)|member.mode!=mode){
            return Err("Esa sala ya usa otro tipo de comunicación".into());
        }
        if mode=="radio" && role=="host" && existing.iter().any(|(_,m)|m.role=="host"){
            return Err("Esta radio ya tiene un emisor".into());
        }
        let limit=if mode=="radio"{MAX_RADIO_PEERS}else{MAX_VOICE_PEERS};
        if existing.len()>=limit{return Err("La sala alcanzó su límite".into());}
        let peers=existing.iter().map(|(id,m)|json!({"id":id,"role":m.role}))
            .collect::<Vec<_>>();
        state.next=state.next.checked_add(1).ok_or("No hay más identificadores")?;
        let id=state.next;
        emit(&tx,json!({"type":"joined","id":id,"room":room,
            "mode":mode,"role":role,"peers":peers,"limit":limit}));
        for (_,member) in existing{
            emit(&member.tx,json!({"type":"peer_joined","id":id,"role":role}));
        }
        state.members.insert(id,Member{
            room:room.to_owned(),mode:mode.to_owned(),role:role.to_owned(),tx
        });
        Ok(id)
    }
    async fn leave(&self,id:u64){
        let mut state=self.0.lock().await;
        let Some(member)=state.members.remove(&id) else{return;};
        for other in state.members.values(){
            if other.room==member.room{
                emit(&other.tx,json!({"type":"peer_left","id":id}));
            }
        }
    }
    async fn signal(&self,from:u64,to:u64,kind:&str,data:&Value)->Result<(),String>{
        if !matches!(kind,"offer"|"answer"|"candidate"){
            return Err("Señal WebRTC desconocida".into());
        }
        if !data.is_object(){return Err("Señal sin datos válidos".into());}
        let wire=json!({"type":"signal","from":from,"kind":kind,"data":data});
        let payload=wire.to_string();
        if payload.len()>MAX_MESSAGE{return Err("Señal demasiado grande".into());}
        let state=self.0.lock().await;
        let sender=state.members.get(&from).ok_or("Emisor desconectado")?;
        let receiver=state.members.get(&to).ok_or("Destinatario desconectado")?;
        if from==to || sender.room!=receiver.room || sender.mode!=receiver.mode {
            return Err("No se puede enviar a otra sala".into());
        }
        if sender.mode=="radio" {
            if (kind=="offer" && sender.role!="host")
                ||(kind=="answer" && sender.role!="listener")
                ||sender.role==receiver.role {
                return Err("La radio sólo permite oferta del emisor y respuesta del oyente".into());
            }
        }
        receiver.tx.try_send(payload).map_err(|_|"El destinatario está saturado".to_owned())
    }
    async fn stats(&self)->Value {
        let state=self.0.lock().await;
        json!({"transport":"webrtc_p2p","rooms":state.members.values()
            .map(|m|m.room.as_str()).collect::<std::collections::HashSet<_>>().len(),
            "peers":state.members.len(),"max_voice":MAX_VOICE_PEERS,
            "max_radio":MAX_RADIO_PEERS,"audio_recorded":false})
    }
}
pub async fn status(
    Extension(hub):Extension<LiveHub>
)->Json<Value> {Json(hub.stats().await)}
pub async fn socket(
    headers:HeaderMap,upgrade:WebSocketUpgrade,Extension(hub):Extension<LiveHub>
)->Result<impl IntoResponse,ApiError>{
    authorize_origin(&headers)?;
    Ok(upgrade.max_message_size(MAX_MESSAGE)
        .on_upgrade(move |socket|serve(socket,hub)))
}
async fn serve(socket:WebSocket,hub:LiveHub){
    let (mut sink,mut source)=socket.split();
    let (tx,mut rx)=mpsc::channel::<String>(MAX_QUEUE);
    let writer=tokio::spawn(async move {
        while let Some(msg)=rx.recv().await {
            if sink.send(Message::Text(msg.into())).await.is_err(){break;}
        }
    });
    let mut id=None;
    while let Some(Ok(message))=source.next().await {
        let Message::Text(text)=message else{
            if matches!(message,Message::Close(_)){break;}
            continue;
        };
        if text.len()>MAX_MESSAGE {
            emit(&tx,json!({"type":"error","message":"Mensaje demasiado grande"}));
            break;
        }
        let Ok(value)=serde_json::from_str::<Value>(&text) else{
            emit(&tx,json!({"type":"error","message":"JSON inválido"}));
            continue;
        };
        match value.get("type").and_then(Value::as_str){
            Some("join") if id.is_none()=>{
                let room=value.get("room").and_then(Value::as_str).unwrap_or("");
                let mode=value.get("mode").and_then(Value::as_str).unwrap_or("");
                let role=value.get("role").and_then(Value::as_str).unwrap_or("");
                match hub.join(room,mode,role,tx.clone()).await {
                    Ok(new_id)=>id=Some(new_id),
                    Err(problem)=>emit(&tx,json!({"type":"error","message":problem})),
                }
            }
            Some("signal") if id.is_some()=>{
                let to=value.get("to").and_then(Value::as_u64).unwrap_or(0);
                let kind=value.get("kind").and_then(Value::as_str).unwrap_or("");
                let data=value.get("data").unwrap_or(&Value::Null);
                if let Err(problem)=hub.signal(id.unwrap_or_default(),to,kind,data).await{
                    emit(&tx,json!({"type":"error","message":problem}));
                }
            }
            Some("leave")=>break,
            _=>emit(&tx,json!({"type":"error","message":"Primero ingresá a una sala"})),
        }
    }
    if let Some(id)=id{hub.leave(id).await;}
    writer.abort();
}
#[cfg(test)]
mod tests{
    use super::*;
    fn channel()->(mpsc::Sender<String>,mpsc::Receiver<String>){mpsc::channel(64)}
    #[tokio::test]
    async fn unique_host_and_mode_limits_are_enforced(){
        let hub=LiveHub::default();
        let (a,mut receive)=channel();
        let first=hub.join("radio_12345","radio","host",a).await.unwrap();
        assert!(first>0);
        assert!(receive.recv().await.unwrap().contains("\"joined\""));
        assert!(hub.join("radio_12345","radio","host",channel().0).await.is_err());
        assert!(hub.join("radio_12345","voice","speaker",channel().0).await.is_err());
        assert!(hub.join("short","voice","speaker",channel().0).await.is_err());
        hub.leave(first).await;
        assert_eq!(hub.stats().await["peers"],0);
    }
    #[tokio::test]
    async fn signaling_is_room_isolated_and_radio_directional(){
        let hub=LiveHub::default();
        let (a,mut host)=channel();
        let host_id=hub.join("radio_12345","radio","host",a).await.unwrap();
        let _=host.recv().await;
        let (b,mut listener)=channel();
        let listener_id=hub.join("radio_12345","radio","listener",b).await.unwrap();
        let _=listener.recv().await;
        let _=host.recv().await;
        let (c,mut outsider)=channel();
        let outsider_id=hub.join("other_12345","voice","speaker",c).await.unwrap();
        let _=outsider.recv().await;
        assert!(hub.signal(listener_id,host_id,"offer",&json!({"sdp":"bad"})).await.is_err());
        assert!(hub.signal(host_id,outsider_id,"offer",&json!({"sdp":"bad"})).await.is_err());
        hub.signal(host_id,listener_id,"offer",&json!({"sdp":"test"})).await.unwrap();
        assert!(listener.recv().await.unwrap().contains("\"from\":1"));
        hub.signal(listener_id,host_id,"answer",&json!({"sdp":"ok"})).await.unwrap();
        assert!(host.recv().await.unwrap().contains("\"answer\""));
        hub.leave(host_id).await;
        assert!(listener.recv().await.unwrap().contains("peer_left"));
    }
    #[tokio::test]
    async fn voice_rooms_limit_members_and_dont_persist_history(){
        let hub=LiveHub::default();
        for _ in 0..8{
            hub.join("voice_12345","voice","speaker",channel().0).await.unwrap();
        }
        assert!(hub.join("voice_12345","voice","speaker",channel().0).await.is_err());
        assert_eq!(hub.stats().await["rooms"],1);
        assert_eq!(hub.stats().await["peers"],8);
    }
}
