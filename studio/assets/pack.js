/* VSPack FullCanvas: ZIP validation and merge always executed by Rust.
 * Never inject source-controlled names, ZIP paths or labels as HTML. */
(() => {
"use strict";
const API="http://127.0.0.1:3061/api/v1/packs";
const mounted=new Map();
function size(n){return n>=1048576?(n/1048576).toFixed(1)+" MiB":n>=1024?(n/1024).toFixed(1)+" KiB":n+" B";}
function el(tag,cls,value){
  const node=document.createElement(tag);
  if(cls)node.className=cls;
  if(value!==undefined)node.textContent=String(value);
  return node;
}
async function request(path,opts={}){
  const response=await fetch(API+path,{cache:"no-store",credentials:"omit",...opts});
  if(!response.ok){
    let reason="HTTP "+response.status;
    try{reason=(await response.json()).error||reason;}catch(_){}
    throw Error(reason);
  }
  return response;
}
function install(root){
  if(root.dataset.packReady)return;
  root.dataset.packReady="true";
  root.innerHTML=
    '<div class="vspack-head"><div><small>VSMEDIA / RESOURCE PACK ENGINE 26.3</small>'+
    '<h2>Gestor de Resource Packs</h2><p>Combiná recursos, controlá conflictos y exportá ZIPs reproducibles.</p></div>'+
    '<span>Sin datapacks · Sesión local</span></div>'+
    '<div class="vspack-workspace">'+
      '<aside class="vspack-sidebar"><div class="vspack-heading"><strong>PAQUETES</strong><span data-field="count">0 / 6</span></div>'+
      '<label class="vspack-upload">＋ Importar ZIP <input data-field="upload" id="vspack-upload" type="file" accept=".zip,application/zip" multiple></label>'+
      '<div class="vspack-sources" data-field="packs"></div>'+
      '<div class="vspack-usage"><span data-field="usage">0 / 64 MiB</span><progress data-field="memory" max="67108864" value="0"></progress></div>'+
      '<p class="vspack-note">La prioridad más alta gana. Los eventos de sounds.json se fusionan por clave. Las colisiones con igual prioridad se rechazan.</p>'+
      '</aside>'+
      '<section class="vspack-inspector">'+
      '<div class="vspack-stats" data-field="stats"></div>'+
      '<div class="vspack-heading"><strong>CONFLICTOS DE RECURSOS</strong><span data-field="conflictcount">0</span></div>'+
      '<div data-field="conflicts" class="vspack-conflicts"></div>'+
      '<div class="vspack-heading"><strong>EXPLORADOR DEL PACK FINAL</strong><span data-field="filecount">0</span></div>'+
      '<div class="vspack-filters"><input type="search" data-field="search" placeholder="Buscar ruta o nombre de recurso">'+
      '<select data-field="category"><option value="">Todas las categorías</option>'+
      '<option value="textures">Texturas</option><option value="models">Modelos</option>'+
      '<option value="sounds">Sonidos</option><option value="fonts">Fuentes</option>'+
      '<option value="languages">Idiomas</option><option value="shaders">Shaders</option>'+
      '<option value="metadata">Manifiesto</option><option value="other">Otros</option></select></div>'+
      '<div class="vspack-files" data-field="files"></div>'+
      '<div class="vspack-texture-preview" data-field="texture"><small>Seleccioná una textura PNG, JPG o WebP para verla sin cargar todas las imágenes.</small></div>'+
      '<div class="vspack-pagination"><span data-field="pagination">Sin recursos</span>'+
      '<button data-action="prev" type="button">Anterior</button><button data-action="next" type="button">Siguiente</button></div>'+
      '</section>'+
      '<aside class="vspack-output"><strong>EXPORTAR</strong>'+
      '<p>ZIP reproducible, PNG/OGG/WebP sin recompresión innecesaria, JSON con DEFLATE.</p>'+
      '<dl><dt>Estado</dt><dd data-field="ready">Sin fuentes</dd>'+
      '<dt>Archivos</dt><dd data-field="totalfiles">0</dd>'+
      '<dt>Datos</dt><dd data-field="totalbytes">0 B</dd>'+
      '<dt>Namespaces</dt><dd data-field="namespaces">—</dd></dl>'+
      '<button data-action="export" id="vspack-export" type="button" class="vspack-export" disabled>↓ Exportar Resource Pack</button>'+
      '<section class="vspack-layer-builder"><strong>PACK BASE + OVERLAYS</strong>'+
      '<p>Exportá subpacks por rutas para conservar la caché del pack base cuando cambian recursos.</p>'+
      '<label>ID del pack base<input type="text" data-field="baseid" maxlength="64" value="virgoland"></label>'+
      '<label>Reglas de capas (id = prefijo de recursos)<textarea data-field="rules" rows="4" placeholder="ui=assets/virgoland/textures/ui/"></textarea></label>'+
      '<button data-action="layers" type="button" disabled>↓ Exportar capas separadas</button>'+
      '<small>Devuelve un ZIP de transporte con manifest.json y los ZIPs de Minecraft base/overlays. No es un pack único instalable directamente.</small></section>'+
      '<button data-action="reload" type="button" class="vspack-reload">↻ Analizar de nuevo</button>'+
      '<p class="vspack-note">La exportación descarga un ZIP local; todavía no lo publica en VSEngine ni en una CDN.</p>'+
      '</aside>'+
    '</div><div role="status" aria-live="polite" class="vspack-status" data-field="status">Importá ZIPs para empezar.</div>';
  const f=name=>root.querySelector('[data-field="'+name+'"]');
  const button=name=>root.querySelector('[data-action="'+name+'"]');
  const state={packs:[],preview:null,page:0,busy:false,disposed:false,
    textureURL:null,controller:new AbortController()};
  function status(message,failed=false){
    f("status").textContent=message;f("status").classList.toggle("error",failed);
  }
  async function guard(action){
    if(state.busy)return;
    state.busy=true;root.classList.add("busy");render();
    try{await action();}catch(e){status(e.message||String(e),true);}
    finally{state.busy=false;root.classList.remove("busy");if(!state.disposed)render();}
  }
  function clearTexture(){
    if(state.textureURL){URL.revokeObjectURL(state.textureURL);state.textureURL=null;}
    f("texture").replaceChildren(el("small","",
      "Seleccioná una textura PNG, JPG o WebP para verla sin cargar todas las imágenes."));
  }
  async function load(){
    clearTexture();
    const info=await (await request("",{signal:state.controller.signal})).json();
    state.packs=info.packs||[];
    state.preview=null;
    if(state.packs.length){
      try{
        state.preview=await(await request("/preview",{signal:state.controller.signal})).json();
        status("Pack analizado. Revisá los archivos y conflictos antes de exportar.");
      }catch(e){status("Fusión bloqueada: "+e.message+". Cambiá prioridades o eliminá capas.",true);}
    }
  }
  function sources(){
    f("packs").replaceChildren();
    f("count").textContent=state.packs.length+" / 6";
    const memory=state.packs.reduce((n,p)=>n+p.bytes,0);
    f("usage").textContent=size(memory)+" / 64 MiB";
    f("memory").value=memory;
    for(const p of [...state.packs].sort((a,b)=>b.priority-a.priority)){
      const card=el("div","vspack-source"),heading=el("div","vspack-source-head");
      heading.append(el("strong","",p.name),el("small","",p.files+" archivos · "+size(p.bytes)));
      const tools=el("div","vspack-source-tools");
      const label=el("label","","Prioridad ");
      const input=el("input");input.type="number";input.min="-1000";input.max="1000";input.step="10";input.value=p.priority;
      input.setAttribute("aria-label","Prioridad de "+p.name);
      input.onchange=()=>guard(async()=>{
        const value=Number(input.value);
        if(!Number.isInteger(value)||value< -1000||value>1000)throw Error("Prioridad fuera de rango (-1000..1000)");
        await request("/"+encodeURIComponent(p.id)+"/priority",{
          method:"POST",headers:{"Content-Type":"application/json"},
          body:JSON.stringify({priority:value}),signal:state.controller.signal});
        await load();
      });
      label.append(input);
      const remove=el("button","vspack-remove","Quitar");
      remove.type="button";remove.onclick=()=>guard(async()=>{
        await request("/"+encodeURIComponent(p.id),{method:"DELETE",signal:state.controller.signal});
        await load();status("El pack fue quitado de la sesión.");
      });
      tools.append(label,remove);card.append(heading,tools);f("packs").append(card);
    }
  }
  function preview(){
    const data=state.preview;
    f("stats").replaceChildren();f("files").replaceChildren();f("conflicts").replaceChildren();
    if(!data){
      f("ready").textContent="Sin composición";f("totalfiles").textContent="0";
      f("totalbytes").textContent="0 B";f("namespaces").textContent="—";
      f("conflictcount").textContent="0";f("filecount").textContent="0";
      f("pagination").textContent="Sin recursos";
      button("prev").disabled=true;button("next").disabled=true;
      button("export").disabled=true;button("layers").disabled=true;
      return;
    }
    for(const [label,value] of [["ARCHIVOS",data.file_count],["CAPAS",state.packs.length],
      ["CONFLICTOS",data.conflicts_count],["TAMAÑO",size(data.bytes)]]){
      const stat=el("div","vspack-stat");
      stat.append(el("small","",label),el("strong","",value));f("stats").append(stat);
    }
    f("ready").textContent=data.ready?"Listo para exportar":"Falta pack.mcmeta";
    f("totalfiles").textContent=data.file_count;
    f("totalbytes").textContent=size(data.bytes);
    f("namespaces").textContent=data.namespaces.join(", ")||"—";
    f("conflictcount").textContent=data.conflicts_count+" conflictos";
    if(!data.conflicts_count)f("conflicts").append(el("div","vspack-empty","Sin conflictos de prioridad."));
    for(const conflict of data.conflicts){
      const row=el("div","vspack-conflict");
      row.append(el("code","",conflict.path),el("small","",conflict.loser+" → "+conflict.winner));
      f("conflicts").append(row);
    }
    if(data.conflicts_count>data.conflicts.length){
      f("conflicts").append(el("small","","Mostrando "+data.conflicts.length+" de "+data.conflicts_count));
    }
    const phrase=f("search").value.toLowerCase().trim(),category=f("category").value;
    const all=data.files.filter(item=>(!category||item.category===category)&&
      (!phrase||item.path.toLowerCase().includes(phrase)));
    const limit=80,pages=Math.max(1,Math.ceil(all.length/limit));
    state.page=Math.max(0,Math.min(state.page,pages-1));
    for(const item of all.slice(state.page*limit,state.page*limit+limit)){
      const row=el("div","vspack-file");
      row.append(el("span","vspack-category",item.category),
        el("code","vspack-path",item.path),el("span","vspack-size",size(item.bytes)));
      if(item.category==="textures" && /.(png|jpe?g|webp)$/i.test(item.path)){
        const view=el("button","vspack-view","Ver");
        view.type="button";view.title="Vista previa de "+item.path;
        view.onclick=()=>guard(async()=>{
          const response=await request("/texture?path="+encodeURIComponent(item.path),
            {signal:state.controller.signal});
          const blob=await response.blob();
          if(blob.size>4*1024*1024)throw Error("Vista previa demasiado grande");
          clearTexture();
          state.textureURL=URL.createObjectURL(blob);
          const image=el("img");
          image.alt=item.path;
          image.loading="lazy";
          image.src=state.textureURL;
          const caption=el("span","",item.path+" · "+size(blob.size));
          f("texture").replaceChildren(image,caption);
          status("Textura de origen validado por Rust. Clic en otra textura para reemplazar.");
        });
        row.append(view);
      }
      f("files").append(row);
    }
    if(!all.length)f("files").append(el("div","vspack-empty","No hay recursos para ese filtro."));
    f("filecount").textContent=all.length+" / "+data.file_count;
    f("pagination").textContent="Página "+(state.page+1)+" / "+pages;
    button("prev").disabled=state.page===0;button("next").disabled=state.page===pages-1;
    button("export").disabled=!data.ready||state.busy;
    button("layers").disabled=!data.ready||state.busy;
  }
  function render(){
    sources();preview();
    f("upload").disabled=state.busy||state.packs.length>=6;
    button("reload").disabled=state.busy;
    if(state.busy){button("export").disabled=true;button("layers").disabled=true;}
  }
  f("upload").onchange=e=>guard(async()=>{
    const files=Array.from(e.target.files||[]);e.target.value="";
    if(files.length+state.packs.length>6)throw Error("Máximo 6 packs.");
    for(const file of files){
      if(!file.name.toLowerCase().endsWith(".zip"))throw Error("Sólo ZIP de Resource Packs.");
      if(file.size>12*1024*1024)throw Error(file.name+": límite 12 MiB por ZIP.");
    }
    for(const file of files){
      const id="pack_"+Array.from(crypto.getRandomValues(new Uint8Array(8)),
        b=>b.toString(16).padStart(2,"0")).join("");
      status("Validando "+file.name+"…");
      await request("/"+id,{
        method:"POST",headers:{"Content-Type":"application/zip",
          "X-VSPack-Label":encodeURIComponent(file.name.slice(0,80))},
        body:file,signal:state.controller.signal});
      await load();
    }
    status("ZIPs importados. Ajustá prioridades y exportá cuando estén listos.");
  });
  f("search").oninput=()=>{state.page=0;preview();};
  f("category").onchange=()=>{state.page=0;preview();};
  button("prev").onclick=()=>{state.page--;preview();};
  button("next").onclick=()=>{state.page++;preview();};
  button("reload").onclick=()=>guard(load);
  button("layers").onclick=()=>guard(async()=>{
    if(!state.preview?.ready)throw Error("Necesitás pack.mcmeta válido.");
    const base_id=f("baseid").value.trim();
    const lines=f("rules").value.split(/?
/).map(line=>line.trim()).filter(Boolean);
    if(!/^[a-z0-9_-]{1,64}$/.test(base_id))throw Error("ID de base inválido.");
    if(!lines.length||lines.length>5)throw Error("Ingresá de 1 a 5 overlays (id=assets/namespace/ruta/).");
    const rules=lines.map(line=>{
      const equals=line.indexOf("=");
      if(equals<1)throw Error("Cada regla debe tener id=prefijo.");
      const logical_id=line.slice(0,equals).trim(),prefix=line.slice(equals+1).trim();
      if(!/^[a-z0-9_-]{1,64}$/.test(logical_id))throw Error("ID overlay inválido: "+logical_id);
      if(!prefix.startsWith("assets/")||!prefix.endsWith("/")){
        throw Error("El prefijo debe comenzar con assets/ y terminar con /");
      }
      return {logical_id,prefixes:[prefix]};
    });
    status("Compilando base y overlays en Rust…");
    const response=await request("/layers/export",{
      method:"POST",headers:{"Content-Type":"application/json"},
      body:JSON.stringify({base_id,rules}),signal:state.controller.signal
    });
    const blob=await response.blob();
    const url=URL.createObjectURL(blob);
    const a=el("a");a.href=url;a.download="vsmedia-layer-bundle.zip";
    document.body.append(a);a.click();a.remove();
    setTimeout(()=>URL.revokeObjectURL(url),5000);
    status("Capas exportadas ("+size(blob.size)+"). Revisá manifest.json; falta publicación HTTPS.");
  });
  button("export").onclick=()=>guard(async()=>{
    if(!state.preview?.ready)throw Error("Falta pack.mcmeta.");
    status("Compilando ZIP determinista en Rust…");
    const response=await request("/export",{signal:state.controller.signal});
    const blob=await response.blob();
    const url=URL.createObjectURL(blob);
    const a=el("a");a.href=url;a.download="vsmedia-resourcepack.zip";
    document.body.append(a);a.click();a.remove();
    setTimeout(()=>URL.revokeObjectURL(url),5000);
    status("ZIP exportado ("+size(blob.size)+"). Todavía no publicado en Minecraft.");
  });
  state.dispose=()=>{
    state.disposed=true;state.controller.abort();
    if(state.textureURL)URL.revokeObjectURL(state.textureURL);
  };
  mounted.set(root,state);
  guard(load);
}
function observe(){
  for(const [root,state] of mounted){
    if(!root.isConnected){state.dispose();mounted.delete(root);}
  }
  document.querySelectorAll("[data-vspack]").forEach(install);
}
function start(){
  observe();
  new MutationObserver(observe).observe(document.body,{childList:true,subtree:true});
}
if(document.readyState==="loading")document.addEventListener("DOMContentLoaded",start);
else start();
})();
