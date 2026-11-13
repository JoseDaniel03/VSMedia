/* VSRadio FullCanvas live DJ → localhost VSCore → Plasmo Voice.
 * Never records or uploads permanent mic audio; holds no long-running secrets.
 * AudioWorklet sends 20ms chunks, fetch batches are bounded and sequential.
 */
(() => {
"use strict";
const BASE="/api/v1/radio";
const FALLBACK="http://127.0.0.1:3061";
let dj=null, installing=false;
const status=(msg,error=false)=>{
  const node=document.querySelector("[data-vsradio-dj-status]");
  if(node){node.textContent=msg;node.style.color=error?"#c34c58":"";}
};
async function api(path,options={}){
  let response;
  try{response=await fetch(BASE+path,options);}catch(_){}
  if(!response||[404,501].includes(response.status)){
    response=await fetch(FALLBACK+BASE+path,options);
  }
  if(!response.ok){
    const text=await response.text();
    try{const json=JSON.parse(text);throw Error(json.error||json.message||text);}
    catch(e){if(e instanceof SyntaxError)throw Error(text||"HTTP "+response.status);throw e;}
  }
  return response;
}
const getStation=()=>window.VSMediaRadio?.active?.id;
const field=name=>document.querySelector("[data-vsradio-dj-"+name+"]");
const clamp=(x,min,max)=>Math.max(min,Math.min(max,Number(x)||0));
function release(dj){
  if(!dj)return;
  if(dj.levelTimer)clearInterval(dj.levelTimer);
  if(dj.node) {try{dj.node.disconnect();}catch(_){}}
  if(dj.script){dj.script.onaudioprocess=null;try{dj.script.disconnect();}catch(_){}}
  for(const node of [dj.micNode,dj.micGain,dj.musicNode,dj.musicGain,dj.outGain]){
    try{node?.disconnect();}catch(_){}
  }
  dj.mic?.getTracks().forEach(track=>track.stop());
  if(dj.player && dj.reattachMusic) {
    dj.player.removeEventListener("playing",dj.reattachMusic);
    dj.player.removeEventListener("loadedmetadata",dj.reattachMusic);
  }
  if(dj.context)dj.context.close().catch(()=>{});
}
async function stop(){
  const old=dj;
  if(!old)return;
  dj=null;
  release(old);
  field("start")?.removeAttribute("disabled");
  field("stop")?.setAttribute("disabled","disabled");
  field("mute")?.setAttribute("disabled","disabled");
  if(old.token){
    await api("/stations/"+old.station+"/live/stop",{
      method:"POST",headers:{"x-vsradio-session":old.token}
    }).catch(()=>{});
  }
  status("DJ desconectado · la emisora vuelve a su playlist");
}
function onPcm(block,stream){
  if(dj!==stream||!stream.token||stream.inFlight>=2)return;
  stream.chunks.push(block);
  if(stream.chunks.length<10)return;
  const buffer=new Int16Array(9600);
  for(let i=0;i<10;i++)buffer.set(stream.chunks.shift(),i*960);
  stream.inFlight++;
  const url="/stations/"+stream.station+"/live/push";
  stream.chain=stream.chain.catch(()=>{}).then(async()=>{
    if(dj!==stream)return;
    const response=await api(url,{method:"POST",headers:{
      "Content-Type":"application/octet-stream","x-vsradio-session":stream.token
    },body:buffer.buffer});
    const json=await response.json();
    stream.lastSeq=json.seq;
    stream.lastPacket=Date.now();
  }).catch(e=>{
    stream.errors=(stream.errors||0)+1;
    if(stream.errors>4)stop().then(()=>status("DJ desconectado: "+e.message,true));
    else status("Reconectando transporte de voz: "+e.message,true);
  }).finally(()=>{stream.inFlight--;});
}
function startFallback(stream,source){
  const node=stream.context.createScriptProcessor(4096,1,1);
  let queue=new Int16Array(960),at=0;
  node.onaudioprocess=e=>{
    if(dj!==stream)return;
    const input=e.inputBuffer.getChannelData(0);
    for(const sample of input){
      const x=Math.max(-1,Math.min(1,sample));
      queue[at++]=x<0?Math.round(x*32768):Math.round(x*32767);
      if(at===960){onPcm(queue,stream);queue=new Int16Array(960);at=0;}
    }
    e.outputBuffer.getChannelData(0).fill(0);
  };
  source.connect(node);node.connect(stream.context.destination);
  stream.script=node;
}
async function start(){
  if(dj||installing)return;
  const station=getStation();
  if(!station){status("Primero publicá la playlist con «Emitir en Minecraft».",true);return;}
  const player=document.getElementById("radio-player");
  const mode=field("mode")?.value||"mic";
  if(mode==="mix"&&(!player||!player.src)){
    status("Elegí y reproducí un tema para mezclarlo con el micrófono.",true);return;
  }
  if(!navigator.mediaDevices?.getUserMedia){status("El micrófono requiere localhost o HTTPS.",true);return;}
  installing=true;
  field("start")?.setAttribute("disabled","disabled");
  let stream;
  try{
    const mic=await navigator.mediaDevices.getUserMedia({audio:{
      echoCancellation:true,noiseSuppression:true,autoGainControl:true
    },video:false});
    stream={station,mode,mic,context:null,node:null,script:null,micNode:null,
      musicNode:null,micGain:null,musicGain:null,outGain:null,player:null,reattachMusic:null,
      chunks:[],inFlight:0,chain:Promise.resolve(),token:null,
      errors:0,muted:false,levelTimer:null,lastSeq:0};
    const Ctx=window.AudioContext||window.webkitAudioContext;
    if(!Ctx)throw Error("Este navegador no permite mezclar audio");
    const context=new Ctx({sampleRate:48000});
    stream.context=context;
    if(context.sampleRate!==48000)throw Error("El dispositivo no ofrece audio a 48 kHz");
    const outGain=context.createGain();
    outGain.gain.value=1;
    stream.outGain=outGain;
    stream.micNode=context.createMediaStreamSource(mic);
    stream.micGain=context.createGain();
    stream.micGain.gain.value=clamp(field("mic")?.value||100,0,150)/100;
    stream.micNode.connect(stream.micGain);
    stream.micGain.connect(outGain);
    if(mode==="mix"){
      // captureStream avoids creating a second MediaElementSource on an element
      // that is also used by the optional WebRTC P2P radio mixer.
      const capture=player.captureStream?.bind(player)||player.mozCaptureStream?.bind(player);
      if(!capture)throw Error("El navegador no soporta captura del reproductor.");
      stream.musicGain=context.createGain();
      stream.musicGain.gain.value=clamp(field("music")?.value||90,0,150)/100;
      stream.musicGain.connect(outGain);
      stream.player=player;
      stream.reattachMusic=()=>{
        if(dj!==stream && stream.token) return;
        const captured=capture();
        const track=captured.getAudioTracks()[0];
        if(!track) return;
        if(stream.musicTrackId===track.id && stream.musicNode)return;
        try{stream.musicNode?.disconnect();}catch(_){}
        stream.musicNode=context.createMediaStreamSource(captured);
        stream.musicNode.connect(stream.musicGain);
        stream.musicTrackId=track.id;
      };
      stream.reattachMusic();
      if(!stream.musicNode)throw Error("Reproducí primero la playlist para capturar su pista.");
      player.addEventListener("playing",stream.reattachMusic);
      player.addEventListener("loadedmetadata",stream.reattachMusic);
      const analyser=context.createAnalyser();
      analyser.fftSize=512;
      stream.micNode.connect(analyser);
      const values=new Float32Array(analyser.fftSize);
      stream.levelTimer=setInterval(()=>{
        if(dj!==stream||!stream.musicGain)return;
        analyser.getFloatTimeDomainData(values);
        let square=0;
        for(const value of values)square+=value*value;
        const rms=Math.sqrt(square/values.length);
        const duck=clamp(field("duck")?.value||65,0,100)/100;
        const base=clamp(field("music")?.value||90,0,150)/100;
        const target=rms>.022&&!stream.muted?base*(1-duck):base;
        stream.musicGain.gain.setTargetAtTime(target,context.currentTime,rms>.022?.055:.22);
      },100);
    }
    if(context.audioWorklet?.addModule){
      await context.audioWorklet.addModule("./radio-dj-worklet.js");
      const node=new AudioWorkletNode(context,"vsradio-pcm20");
      node.port.onmessage=e=>{if(e.data instanceof Int16Array)onPcm(e.data,stream);};
      outGain.connect(node);node.connect(context.destination);
      stream.node=node;
    }else{
      startFallback(stream,outGain);
      status("Captura compatible activada; tu navegador no tiene AudioWorklet.");
    }
    // Register only AFTER media device and processing nodes are ready.
    const response=await api("/stations/"+station+"/live/start",{
      method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({mode})
    });
    const info=await response.json();
    stream.token=info.token;
    dj=stream;
    await context.resume();
    field("stop")?.removeAttribute("disabled");
    field("mute")?.removeAttribute("disabled");
    status("◉ DJ en directo · "+(mode==="mix"?"música + micrófono":"micrófono")+
      " · Plasmo Voice");
  }catch(e){
    release(stream);
    status("No se pudo iniciar la emisión: "+e.message,true);
    field("start")?.removeAttribute("disabled");
  }finally{installing=false;}
}
function mount(){
  const radio=[...document.querySelectorAll(".studio-toolbar")].find(
    el=>/^VSRadio$/i.test(el.querySelector("strong")?.textContent?.trim()||""));
  if(!radio||radio.querySelector("[data-vsradio-dj-start]"))return;
  const wrap=document.createElement("span");
  wrap.className="vsradio-live-controls";
  wrap.style.cssText="display:inline-flex;gap:6px;align-items:center;flex-wrap:wrap";
  const button=(name,label,callback,disabled=false)=>{
    const elem=document.createElement("button");elem.type="button";elem.className="toolbar-button";
    elem.textContent=label;elem.dataset["vsradioDj"+name[0].toUpperCase()+name.slice(1)]="1";
    if(disabled)elem.disabled=true;
    elem.addEventListener("click",callback);wrap.append(elem);
  };
  const select=document.createElement("select");select.dataset.vsradioDjMode="1";
  [["mic","Micrófono DJ"],["mix","Playlist + DJ"]].forEach(([value,label])=>{
    const item=document.createElement("option");item.value=value;item.textContent=label;select.add(item);
  });
  select.title="Qué transmite la emisora por Plasmo Voice";
  wrap.append(select);
  const slider=(name,label,value)=>{
    const text=document.createElement("label");text.textContent=label+" ";
    const input=document.createElement("input");input.type="range";
    input.min="0";input.max=name==="duck"?"100":"150";input.step="5";
    input.value=String(value);input.dataset["vsradioDj"+name[0].toUpperCase()+name.slice(1)]="1";
    input.style.width="70px";
    input.addEventListener("input",()=>{
      if(name==="mic"&&dj?.micGain)dj.micGain.gain.setTargetAtTime(
        dj.muted?0:Number(input.value)/100,dj.context.currentTime,.03);
    });
    text.append(input);wrap.append(text);
  };
  slider("music","Música",90);slider("mic","DJ",100);slider("duck","Ducking",65);
  button("start","◉ Abrir DJ",()=>start());
  button("stop","■ Cortar DJ",()=>stop(),true);
  button("mute","Silenciar",()=>{
    if(!dj)return;dj.muted=!dj.muted;
    dj.micGain.gain.setTargetAtTime(dj.muted?0:clamp(field("mic")?.value||100,0,150)/100,
      dj.context.currentTime,.03);
    const elem=field("mute");if(elem)elem.textContent=dj.muted?"Activar DJ":"Silenciar";
  },true);
  const note=document.createElement("span");
  note.dataset.vsradioDjStatus="1";note.className="toolbar-caption";
  note.textContent="DJ no conectado";
  wrap.append(note);
  radio.append(wrap);
}
function ready(){
  mount();
  new MutationObserver(mount).observe(document.body,{childList:true,subtree:true});
  window.addEventListener("pagehide",()=>{if(dj)stop();});
  window.addEventListener("vsradio:published",e=>{
    if(dj&&e.detail?.id!==dj.station)stop();
  });
}
if(document.readyState==="loading")document.addEventListener("DOMContentLoaded",ready,{once:true});
else ready();
})();
