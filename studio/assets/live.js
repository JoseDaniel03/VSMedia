/* VSMedia: ephemeral peer-to-peer radio and voice rooms. No server audio. */
(() => {
"use strict";
const URL = "ws://127.0.0.1:3061/api/v1/live/ws";
const mounted = new Map();
const radioSources = new WeakMap();
function roomCode(prefix) {
  const bytes = new Uint8Array(12);
  crypto.getRandomValues(bytes);
  return prefix + "_" + Array.from(bytes, x => x.toString(16).padStart(2,"0")).join("");
}
function getRadioSource(player, microphone) {
  if (!player || !player.src) throw Error("Elegí un archivo de VSRadio y reproducilo.");
  const Ctx = window.AudioContext || window.webkitAudioContext;
  if (!Ctx) throw Error("AudioContext no está disponible");
  let graph = radioSources.get(player);
  if (!graph) {
    const ctx = new Ctx();
    const source = ctx.createMediaElementSource(player);
    source.connect(ctx.destination);
    graph = {ctx,source};
    radioSources.set(player,graph);
  }
  const dest = graph.ctx.createMediaStreamDestination();
  graph.source.connect(dest);
  const micNode = microphone ? graph.ctx.createMediaStreamSource(microphone) : null;
  if (micNode) micNode.connect(dest);
  graph.ctx.resume();
  return {stream:dest.stream,dispose:()=>{
    try {graph.source.disconnect(dest);}catch(_){}
    if (micNode) try {micNode.disconnect();}catch(_){}
    dest.stream.getTracks().forEach(track=>track.stop());
  }};
}
function mount(root) {
  if (root.dataset.liveReady) return;
  root.dataset.liveReady="1";
  const radio=root.dataset.vslive==="radio";
  root.innerHTML =
    '<header class="vs-live-header"><div><strong>'+(radio?"VSRadio · Emisión en vivo":"VSStreams · Chat de voz")+
    '</strong><small>'+(radio?"1 emisor + 15 oyentes máximo":"8 participantes máximo")+
    '</small></div><span class="vs-live-status" data-field="status" role="status">Sin conectar</span></header>'+
    '<div class="vs-live-fields"><label>Código de sala <input data-field="room" maxlength="64" autocomplete="off" spellcheck="false"></label>'+
    '<button type="button" data-action="generate">Crear código</button>'+
    (radio?'<label>Mi función <select data-field="role"><option value="listener">Oyente</option><option value="host">Emisor</option></select></label>'+
    '<label>Fuente <select data-field="source"><option value="mic">Micrófono</option><option value="player">Playlist</option><option value="both">Playlist + micrófono</option></select></label>':"")+
    '</div><div class="vs-live-actions">'+
    '<button class="action-primary" type="button" data-action="join">Conectar</button>'+
    '<button type="button" data-action="leave" disabled>Salir</button>'+
    '<button type="button" data-action="mute" disabled>Silenciar micrófono</button></div>'+
    '<div class="vs-live-peers"><strong data-field="count">0 participantes</strong><div data-field="peers"></div></div>'+
    '<div class="vs-live-remote" data-field="remote"></div>'+
    '<small class="vs-live-note">WebRTC P2P: el audio no pasa por Axum ni se graba. Micrófono sólo con permiso al pulsar Conectar. '+
    'Esta versión funciona en el entorno local; Internet requiere HTTPS/WSS, autenticación y TURN. No es radio pública masiva.</small>';
  const field = name=>root.querySelector('[data-field="'+name+'"]');
  const button = name=>root.querySelector('[data-action="'+name+'"]');
  const state={ws:null,id:null,role:"",stream:null,mic:null,cleanup:null,
    peers:new Map(),closed:false,muted:false};
  function status(value,error=false){
    field("status").textContent=value;
    field("status").classList.toggle("error",error);
  }
  function peersDisplay(){
    field("count").textContent=String(state.peers.size+(state.id==null?0:1))+" participantes";
    field("peers").textContent=Array.from(state.peers,([id,p])=>"#"+id+" · "+p.role).join(" | ");
  }
  function send(payload){
    if(state.ws?.readyState===WebSocket.OPEN){
      const json=JSON.stringify(payload);
      if(json.length<=48000)state.ws.send(json);
    }
  }
  function sendSignal(to,kind,data){send({type:"signal",to,kind,data});}
  function peer(id,role){
    if(state.peers.has(id))return state.peers.get(id);
    const config={iceServers:Array.isArray(window.VSMEDIA_ICE_SERVERS)?window.VSMEDIA_ICE_SERVERS:[]};
    const pc=new RTCPeerConnection(config);
    const record={pc,role,pending:[]};
    state.peers.set(id,record);
    if(state.stream)state.stream.getAudioTracks().forEach(t=>pc.addTrack(t,state.stream));
    else pc.addTransceiver("audio",{direction:"recvonly"});
    pc.onicecandidate=e=>{if(e.candidate)sendSignal(id,"candidate",e.candidate.toJSON());};
    pc.ontrack=e=>{
      if(!root.isConnected)return;
      let audio=root.querySelector('[data-peer-audio="'+id+'"]');
      if(!audio){
        audio=document.createElement("audio");
        audio.dataset.peerAudio=String(id);
        audio.controls=true;audio.autoplay=true;audio.playsInline=true;
        field("remote").append(audio);
      }
      audio.srcObject=e.streams[0]||new MediaStream([e.track]);
      audio.play().catch(()=>status("Activá reproducir para escuchar al otro participante"));
    };
    pc.onconnectionstatechange=()=>{
      if(pc.connectionState==="failed")status("No se pudo conectar el audio. Podría requerir TURN.",true);
    };
    peersDisplay();
    return record;
  }
  async function makeOffer(id,role){
    const conn=peer(id,role).pc;
    await conn.setLocalDescription(await conn.createOffer());
    sendSignal(id,"offer",conn.localDescription.toJSON());
  }
  async function signal(msg){
    const {from,kind,data}=msg;
    if(!Number.isSafeInteger(from)||!["offer","answer","candidate"].includes(kind))return;
    const record=peer(from,radio?(state.role==="host"?"listener":"host"):"speaker");
    const conn=record.pc;
    if(kind==="offer"){
      await conn.setRemoteDescription(new RTCSessionDescription(data));
      while(record.pending.length)await conn.addIceCandidate(record.pending.shift());
      await conn.setLocalDescription(await conn.createAnswer());
      sendSignal(from,"answer",conn.localDescription.toJSON());
    }else if(kind==="answer"){
      await conn.setRemoteDescription(new RTCSessionDescription(data));
      while(record.pending.length)await conn.addIceCandidate(record.pending.shift());
    }else{
      const candidate=new RTCIceCandidate(data);
      if(conn.remoteDescription)await conn.addIceCandidate(candidate);
      else if(record.pending.length<128)record.pending.push(candidate);
    }
  }
  function offerNeeded(role,isNew){
    return radio?(state.role==="host"&&role==="listener"):isNew;
  }
  async function stop(){
    state.closed=true;
    for(const value of state.peers.values())value.pc.close();
    state.peers.clear();
    field("remote").replaceChildren();
    if(state.ws){
      if(state.ws.readyState===WebSocket.OPEN)send({type:"leave"});
      if(state.ws.readyState<=WebSocket.OPEN)state.ws.close();
      state.ws=null;
    }
    if(state.cleanup){state.cleanup();state.cleanup=null;}
    if(state.mic){state.mic.getTracks().forEach(t=>t.stop());state.mic=null;}
    state.stream=null;state.id=null;state.muted=false;
    button("join").disabled=false;button("leave").disabled=true;
    button("mute").disabled=true;button("mute").textContent="Silenciar micrófono";
    peersDisplay();status("Sin conectar");
  }
  async function join(){
    if(state.ws)return;
    const code=field("room").value.trim();
    if(!/^[a-zA-Z0-9_-]{8,64}$/.test(code)){
      status("Código inválido: usá 8–64 letras, números, - o _",true);return;
    }
    if(!window.RTCPeerConnection){status("WebRTC no disponible",true);return;}
    state.closed=false;
    state.role=radio?field("role").value:"speaker";
    const output=!radio || state.role==="host";
    const source=radio?field("source").value:"mic";
    button("join").disabled=true;
    try{
      if(output && (!radio || source==="mic" || source==="both")){
        if(!navigator.mediaDevices?.getUserMedia)
          throw Error("Se requiere localhost o HTTPS para el micrófono");
        state.mic=await navigator.mediaDevices.getUserMedia({
          video:false,audio:{echoCancellation:true,noiseSuppression:true,autoGainControl:true}
        });
      }
      if(!radio)state.stream=state.mic;
      else if(output&&(source==="player"||source==="both")){
        const result=getRadioSource(document.getElementById("radio-player"),
          source==="both"?state.mic:null);
        state.stream=result.stream;state.cleanup=result.dispose;
      }else if(output)state.stream=state.mic;
      if(state.closed)throw Error("Conexión cancelada");
      const ws=new WebSocket(URL);
      state.ws=ws;status("Conectando…");
      ws.onopen=()=>send({type:"join",room:code,mode:radio?"radio":"voice",role:state.role});
      ws.onmessage=async event=>{
        if(!root.isConnected || typeof event.data!=="string" || event.data.length>48000)return;
        try{
          const msg=JSON.parse(event.data);
          if(msg.type==="error"){
            status(msg.message||"Error de señalización",true);
            if(state.id===null)await stop();
          }else if(msg.type==="joined"){
            state.id=msg.id;
            button("leave").disabled=false;button("mute").disabled=!state.mic;
            status(radio?(state.role==="host"?"Transmitiendo":"Escuchando"):"Voz conectada");
            for(const p of msg.peers||[]){
              if(offerNeeded(p.role,true))await makeOffer(p.id,p.role);
              else peer(p.id,p.role);
            }
            peersDisplay();
          }else if(msg.type==="peer_joined"){
            if(offerNeeded(msg.role,false))await makeOffer(msg.id,msg.role);
            else peer(msg.id,msg.role);
          }else if(msg.type==="peer_left"){
            const p=state.peers.get(msg.id);
            if(p){p.pc.close();state.peers.delete(msg.id);}
            root.querySelector('[data-peer-audio="'+msg.id+'"]')?.remove();
            peersDisplay();
          }else if(msg.type==="signal")await signal(msg);
        }catch(error){status("Problema con audio remoto: "+error.message,true);}
      };
      ws.onerror=()=>status("No se pudo conectar con Axum local (puerto 3061)",true);
      ws.onclose=()=>{if(!state.closed)stop().catch(()=>{});};
    }catch(error){
      await stop();status(error.message||"No se pudo conectar",true);
    }
  }
  button("generate").onclick=()=>field("room").value=roomCode(radio?"radio":"voice");
  button("join").onclick=()=>join();
  button("leave").onclick=()=>stop();
  button("mute").onclick=()=>{
    if(!state.mic)return;
    state.muted=!state.muted;
    state.mic.getAudioTracks().forEach(track=>track.enabled=!state.muted);
    button("mute").textContent=state.muted?"Activar micrófono":"Silenciar micrófono";
  };
  state.stop=stop;
  mounted.set(root,state);
}
function refresh(){
  for(const [root,state] of mounted){
    if(!root.isConnected){state.stop().catch(()=>{});mounted.delete(root);}
  }
  document.querySelectorAll("[data-vslive]").forEach(mount);
}
function boot(){refresh();new MutationObserver(refresh).observe(document.body,{childList:true,subtree:true});}
if(document.readyState==="loading")document.addEventListener("DOMContentLoaded",boot);
else boot();
})();
