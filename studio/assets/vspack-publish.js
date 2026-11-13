/* VSPack module: additive controls in the original VSMedia Minecraft panel.
   Browser assembles a folder into an uncompressed ZIP. VSCore alone validates,
   persists and merges models, textures, animations, fonts and other pack assets.
*/
(() => {
"use strict";
const MAX=96*1024*1024;
const $=selector=>document.querySelector("#vs-minecraft-panel "+selector);
let busy=false;
const node=(type,text="",attrs={})=>{
  const e=document.createElement(type);e.textContent=text;
  for(const [key,value] of Object.entries(attrs))e.setAttribute(key,String(value));
  return e;
};
function status(text,error=false){
  const out=$("[data-vspack-status]");
  if(out){out.textContent=text;out.dataset.error=String(error);}
}
async function api(path,options={}){
  let r;try{r=await fetch("/api/v1"+path,options);}catch(_){}
  if(!r||r.status===404||r.status===501)r=await fetch("http://127.0.0.1:3061/api/v1"+path,options);
  if(!r.ok){
    let message=await r.text();
    try{message=JSON.parse(message).error||message;}catch(_){}
    throw Error(message||"HTTP "+r.status);
  }
  return r;
}
async function guarded(work){
  if(busy)return;busy=true;
  for(const btn of document.querySelectorAll('[data-mc-page="pack"] button'))btn.disabled=true;
  try{await work();}catch(e){status(e.message,true);}
  finally{busy=false;for(const btn of document.querySelectorAll('[data-mc-page="pack"] button'))btn.disabled=false;}
}
const idFrom=name=>name.normalize("NFD").replace(/[\u0300-\u036f]/g,"").toLowerCase()
  .replace(/[^a-z0-9_-]/g,"-").replace(/^-+|-+$/g,"").slice(0,40);
function pathFrom(file){
  const relative=(file.webkitRelativePath||file.name).replace(/\\/g,"/");
  const marker=relative.indexOf("/assets/");
  if(marker>=0)return relative.slice(marker+1);
  if(relative.startsWith("assets/"))return relative;
  const last=relative.split("/").pop();
  if(last==="pack.mcmeta"||last==="pack.png")return last;
  return null;
}
function safePath(p){
  return p && p.length<=220 && !p.startsWith("/") && !p.includes("\\") &&
    !p.includes(":") && p.split("/").every(s=>s&&s!=="."&&s!==".."&&/^[a-zA-Z0-9_. -]+$/.test(s));
}
const ENCODER=new TextEncoder();
const CRC=new Uint32Array(256);
for(let i=0;i<256;i++){
  let n=i;for(let j=0;j<8;j++)n=(n&1)?0xedb88320^(n>>>1):n>>>1;
  CRC[i]=n>>>0;
}
function crc32(data){
  let n=0xffffffff;for(const b of data)n=CRC[(n^b)&255]^(n>>>8);
  return (n^0xffffffff)>>>0;
}
function record(size){const bytes=new Uint8Array(size);return {bytes,view:new DataView(bytes.buffer)};}
async function folderZip(files){
  const names=new Set(),fileset=[];
  for(const file of files){
    const name=pathFrom(file);
    if(!name)continue;
    if(!safePath(name)||names.has(name))throw Error("Ruta de pack insegura o repetida: "+name);
    names.add(name);fileset.push({name,file});
  }
  if(!names.has("pack.mcmeta"))throw Error("Falta pack.mcmeta en la raíz de la carpeta.");
  if(fileset.length>12000)throw Error("Exceso de archivos (máximo 12.000).");
  // Byte-order stable across machines and language locales: same folder => same ZIP.
  fileset.sort((a,b)=>a.name<b.name?-1:(a.name>b.name?1:0));
  const chunks=[],central=[];let position=0,centralBytes=0;
  for(const {name,file} of fileset){
    if(file.size>32*1024*1024)throw Error("Archivo supera 32 MiB: "+name);
    const data=new Uint8Array(await file.arrayBuffer());
    const nameBytes=ENCODER.encode(name), checksum=crc32(data);
    const local=record(30+nameBytes.length),h=local.view;
    h.setUint32(0,0x04034b50,true);h.setUint16(4,20,true);
    h.setUint16(6,0x800,true);h.setUint16(8,0,true);
    h.setUint16(12,33,true);h.setUint32(14,checksum,true);
    h.setUint32(18,data.length,true);h.setUint32(22,data.length,true);
    h.setUint16(26,nameBytes.length,true);local.bytes.set(nameBytes,30);
    chunks.push(local.bytes,data);
    const c=record(46+nameBytes.length),v=c.view;
    v.setUint32(0,0x02014b50,true);v.setUint16(4,20,true);
    v.setUint16(6,20,true);v.setUint16(8,0x800,true);
    v.setUint16(14,33,true);v.setUint32(16,checksum,true);
    v.setUint32(20,data.length,true);v.setUint32(24,data.length,true);
    v.setUint16(28,nameBytes.length,true);v.setUint32(42,position,true);
    c.bytes.set(nameBytes,46);central.push(c.bytes);centralBytes+=c.bytes.length;
    position+=local.bytes.length+data.length;
    if(position+centralBytes+22>MAX)throw Error("Carpeta supera 96 MiB. Comprimí como ZIP antes de importarla.");
  }
  const centralStart=position;
  chunks.push(...central);
  const end=record(22),v=end.view;
  v.setUint32(0,0x06054b50,true);
  v.setUint16(8,fileset.length,true);v.setUint16(10,fileset.length,true);
  v.setUint32(12,centralBytes,true);v.setUint32(16,centralStart,true);
  chunks.push(end.bytes);
  return new Blob(chunks,{type:"application/zip"});
}
// Reusable by VSRender/Typewriter browser integrations; no server roundtrip.
window.VSPackBrowser = Object.freeze({zipFolder:folderZip});
async function refresh(){
  const value=await (await api("/packs")).json();
  const list=$("[data-vspack-list]");list.replaceChildren();
  for(const pack of value.packs||[]){
    const row=node("div","",{class:"vs-minecraft-inline"});
    const title=node("span",pack.name+" · "+pack.files+" archivos");
    title.title=pack.fingerprint;
    const priority=node("input","",{type:"number",min:"-1000",max:"1000",value:String(pack.priority)});
    priority.style.width="65px";
    const save=node("button","Orden",{type:"button"});
    save.onclick=()=>guarded(async()=>{
      const n=Number(priority.value);
      if(!Number.isInteger(n)||n < -1000||n>1000)throw Error("Prioridad entre -1000 y 1000");
      await api("/packs/"+encodeURIComponent(pack.id)+"/priority",{method:"POST",
        headers:{"Content-Type":"application/json"},body:JSON.stringify({priority:n})});
      await refresh();status("Prioridad guardada.");
    });
    const remove=node("button","Quitar",{type:"button"});
    remove.onclick=()=>guarded(async()=>{
      if(!confirm("¿Eliminar del pack la fuente "+pack.name+"?"))return;
      await api("/packs/"+encodeURIComponent(pack.id),{method:"DELETE"});
      await refresh();status("Fuente quitada.");
    });
    row.append(title,priority,save,remove);list.append(row);
  }
  if(!(value.packs||[]).length)list.append(node("p","Sin packs importados."));
  $("[data-vspack-total]").textContent=(value.packs||[]).length+" ZIP · "+
    ((value.total_bytes||0)/1048576).toFixed(2)+" MiB RAM · "+
    (value.zip_sources_persisted?"Originales guardados":"Solo sesión");
}
async function upload(zip,name){
  if(!zip||!zip.size)throw Error("Elegí un ZIP o carpeta válida");
  if(zip.size>MAX)throw Error("El ZIP supera 96 MiB.");
  const id=(idFrom(name.replace(/\.zip$/i,""))||"pack")+"-"+Math.random().toString(36).slice(2,7);
  status("Importando texturas/modelos y verificando el ZIP…");
  const item=await (await api("/packs/"+id,{method:"POST",
    headers:{"Content-Type":"application/zip","X-VSPack-Label":encodeURIComponent(name)},body:zip})).json();
  await refresh();status(item.name+" agregado · "+item.files+
    " recursos. Se incluirá en el Resource Pack final.");
}
async function diagnose(){
  status("Revisando conflictos y referencias…");
  const catalog=await (await api("/packs")).json();
  const soundCatalog=await (await api("/sounds")).json();
  const fontCatalog=await (await api("/fonts")).json();
  const importedFonts=(fontCatalog.fonts||[]).filter(f=>f.persistent).length;
  const publishedSounds=(soundCatalog.sounds||[]).length;
  if(!(catalog.packs||[]).length){
    $("[data-vspack-diagnostics]").textContent=
      "Fuentes ZIP: 0\nSonidos persistentes: "+publishedSounds+
      "\nTipografías importadas: "+importedFonts+
      "\nPack final: los sonidos y fuentes también se exportan, aun sin ZIPs adicionales.";
    status("Biblioteca persistente revisada. Podés exportar directamente.",false);
    return;
  }
  const p=await (await api("/packs/preview")).json();
  const d=await (await api("/packs/diagnostics")).json();
  $("[data-vspack-diagnostics]").textContent=
    "Recursos de ZIPs: "+p.file_count+" | "+(p.bytes/1048576).toFixed(2)+" MiB\n"+
    "Sonidos persistentes adicionales: "+publishedSounds+
    "\nTipografías adicionales: "+importedFonts+
    "\nCategorías: "+JSON.stringify(p.categories)+"\n"+
    "Namespaces: "+JSON.stringify(p.namespaces)+"\n"+
    "Conflictos: "+p.conflicts_count+"\n"+
    "Dependencias: "+JSON.stringify(d,null,2).slice(0,3000);
  status(p.ready?"Pack analizado. Comprobá referencias antes de distribuir.":
    "El pack no supera las comprobaciones previas.",!p.ready);
}
async function exportPack(){
  const response=await api("/packs/export");
  const blob=await response.blob();
  if(!blob.size)throw Error("Pack ZIP vacío");
  const url=URL.createObjectURL(blob);
  const a=node("a","",{href:url,download:"vsmedia-resourcepack.zip"});
  document.body.append(a);a.click();a.remove();
  setTimeout(()=>URL.revokeObjectURL(url),15000);
  status("Pack unificado exportado ("+(blob.size/1048576).toFixed(2)+
    " MiB). Para entregarlo a Minecraft falta publicar una revisión CDN HTTPS.");
}
function mount(){
  const panel=document.querySelector("#vs-minecraft-panel");
  if(!panel||panel.querySelector('[data-mc-page="pack"]'))return;
  const button=node("button","Recursos / VSPack",{
    type:"button","data-mc-tab":"pack","aria-pressed":"false"});
  panel.querySelector(".vs-minecraft-tabs").append(button);
  const page=node("div","",{"data-mc-page":"pack"});
  page.hidden=true;
  page.innerHTML=[
    '<p>Subí Resource Packs completos: texturas, modelos, entidades,',
    ' fuentes, animaciones, GUI, HUD, partículas, sonidos, shaders y más.</p>',
    '<label>Archivo ZIP <input type="file" name="vspack-zip" accept=".zip,application/zip"></label>',
    '<button type="button" data-vspack-upload>＋ Importar ZIP</button>',
    '<label>O carpeta con pack.mcmeta y assets/ ',
    '<input type="file" name="vspack-folder" webkitdirectory directory multiple></label>',
    '<button type="button" data-vspack-folder>＋ Importar carpeta</button>',
    '<small>La carpeta se empaqueta en el navegador. Límite: 96 MiB, 12.000 archivos.</small>',
    '<h4>Fuentes del pack</h4><small data-vspack-total>Consultando…</small>',
    '<div data-vspack-list></div>',
    '<div class="vs-minecraft-inline">',
    '<button type="button" data-vspack-refresh>Actualizar</button>',
    '<button type="button" data-vspack-check>Revisar dependencias</button>',
    '<button type="button" data-vspack-export>↓ Exportar pack</button></div>',
    '<pre data-vspack-diagnostics style="white-space:pre-wrap;max-height:190px;overflow:auto"></pre>',
    '<p data-vspack-status role="status">Importá modelos y texturas con sus rutas originales.</p>',
    '<small>El ZIP exportado aún necesita alojamiento HTTPS para llegar al jugador.</small>'
  ].join("");
  panel.querySelector('[data-mc-page="audio"]').before(page);
  button.onclick=()=>{window.VSMinecraft?.open("pack");refresh().catch(e=>status(e.message,true));};
  $("[data-vspack-upload]").onclick=()=>guarded(async()=>{
    const file=$('[name="vspack-zip"]').files[0];
    if(!file)throw Error("Elegí un ZIP");
    await upload(file,file.name);
  });
  $("[data-vspack-folder]").onclick=()=>guarded(async()=>{
    const files=[...($('[name="vspack-folder"]').files||[])];
    if(!files.length)throw Error("Elegí una carpeta");
    status("Empaquetando "+files.length+" archivos en el cliente…");
    const archive=await folderZip(files);
    await upload(archive,files[0].webkitRelativePath.split("/")[0]||"Resource Pack");
  });
  $("[data-vspack-refresh]").onclick=()=>guarded(refresh);
  $("[data-vspack-check]").onclick=()=>guarded(diagnose);
  $("[data-vspack-export]").onclick=()=>guarded(exportPack);
  refresh().catch(()=>{});
}
if(document.readyState==="loading")
  document.addEventListener("DOMContentLoaded",mount,{once:true});
else mount();
})();
