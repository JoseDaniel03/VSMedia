/* VSAssets — source files per project, independent of VSPack and Minecraft.
 * Plain DOM layer added to the existing FullCanvas Studio; no framework swap.
 */
(() => {
  "use strict";
  const BASE="/api/v1/workspace/projects";
  const FALLBACK="http://127.0.0.1:3061/api/v1/workspace/projects";
  const MAX_FILE=96*1024*1024;
  const $=(q)=>document.querySelector("#vsassets-screen "+q);
  const state={project:"",folder:"assets",data:null,projects:[],filter:"",busy:false,previewUrl:"",open:false};
  const elt=(tag,txt,cls)=>{
    const e=document.createElement(tag);
    if(txt!==undefined&&txt!==null)e.textContent=String(txt);
    if(cls)e.className=cls;
    return e;
  };
  const encode=(text)=>encodeURIComponent(text);
  const slug=(text)=>text.normalize("NFD").replace(/[\u0300-\u036f]/g,"")
    .toLowerCase().replace(/[^a-z0-9_-]+/g,"-").replace(/^-+|-+$/g,"").slice(0,44);
  const qpath=(path)=>"?path="+encode(path);
  const fmt=n=>n>=1048576?(n/1048576).toFixed(1)+" MiB":
    n>=1024?(n/1024).toFixed(1)+" KiB":n+" B";
  function message(text,error=false){
    const e=$("[data-assets-status]");
    if(e){e.textContent=text;e.dataset.error=String(error);}
  }
  async function api(url,options={}){
    let response;
    try {response=await fetch(BASE+url,{cache:"no-store",...options});}catch(_){}
    if(!response || [404,501].includes(response.status))
      response=await fetch(FALLBACK+url,{cache:"no-store",...options});
    if(!response.ok){
      const raw=await response.text();
      let error=raw;
      try {error=JSON.parse(raw).error||raw;}catch(_){}
      throw Error(error||"Error HTTP "+response.status);
    }
    return response;
  }
  const json=async(url,options={})=>(await api(url,options)).json();
  async function task(callback){
    if(state.busy)return;
    state.busy=true;
    const root=document.querySelector("#vsassets-screen");
    for(const button of root?.querySelectorAll("button")||[])button.disabled=true;
    try{await callback();}
    catch(error){message(error.message,true);}
    finally{
      state.busy=false;
      for(const button of root?.querySelectorAll("button")||[])button.disabled=false;
    }
  }
  function releasePreview(){
    if(state.previewUrl)URL.revokeObjectURL(state.previewUrl);
    state.previewUrl="";
  }
  function open(){
    mount();
    const screen=document.getElementById("vsassets-screen");
    const main=document.querySelector(".workspace-main");
    if(!screen||!main)return;
    state.open=true;
    main.hidden=true;
    screen.hidden=false;
    const tabs=document.querySelectorAll(".app-tabs button");
    for(const tab of tabs){
      const active=tab.dataset.vsassetsTab==="true";
      tab.classList.toggle("active",active);
      if(active)tab.setAttribute("aria-current","page");
      else tab.removeAttribute("aria-current");
    }
    loadProjects().catch(e=>message(e.message,true));
  }
  function close(){
    if(!state.open)return;
    state.open=false;releasePreview();
    const screen=document.getElementById("vsassets-screen");
    if(screen)screen.hidden=true;
    const main=document.querySelector(".workspace-main");
    if(main)main.hidden=false;
    document.querySelector('[data-vsassets-tab]')?.classList.remove("active");
    document.querySelector('[data-vsassets-tab]')?.removeAttribute("aria-current");
  }
  function fillProjects(){
    const select=$("[data-assets-project]");
    if(!select)return;
    select.replaceChildren(elt("option","Seleccioná un proyecto"));
    select.options[0].value="";
    for(const project of state.projects){
      const option=elt("option",project.name+" · "+fmt(project.bytes));
      option.value=project.id;
      select.append(option);
    }
    select.value=state.project;
  }
  async function loadProjects(){
    const data=await json("");
    state.projects=data.projects||[];
    if(state.project&&!state.projects.some(p=>p.id===state.project)){
      state.project="";state.data=null;
    }
    fillProjects();
    if(state.project)await refresh();
    else render();
  }
  async function refresh(){
    if(!state.project){state.data=null;render();return;}
    state.data=await json("/"+encode(state.project));
    if(!state.data.folders.includes(state.folder))state.folder="assets";
    render();
  }
  function currentProject(){return state.projects.find(p=>p.id===state.project);}
  function render(){
    const heading=$("[data-assets-name]");
    heading.textContent=state.data?.name||"Biblioteca del proyecto";
    const sub=$("[data-assets-summary]");
    sub.textContent=state.data?
      state.data.files.length+" archivos · "+fmt(state.data.total_bytes)+
        " / "+fmt(state.data.max_bytes)+" · originales privados":"Elegí o creá un proyecto para comenzar.";
    const folders=$("[data-assets-folders]");
    const files=$("[data-assets-files]");
    folders.replaceChildren();files.replaceChildren();
    if(!state.data){
      files.append(elt("p","Los archivos originales viven en assets/. Nada se agrega automáticamente al Resource Pack.","assets-empty"));
      $("[data-assets-preview]").replaceChildren(elt("p","Elegí un archivo para ver su contenido seguro.","assets-empty"));
      return;
    }
    for(const path of state.data.folders){
      const label=path==="assets"?"▣ Assets":("  ".repeat(Math.min(path.split("/").length-2,4))+
        "▸ "+path.split("/").pop());
      const button=elt("button",label,"assets-folder"+(path===state.folder?" selected":""));
      button.type="button";button.title=path;button.setAttribute("aria-pressed",String(path===state.folder));
      button.onclick=()=>{state.folder=path;render();};
      folders.append(button);
    }
    const filtered=(state.data.files||[]).filter(f=>
      f.path.startsWith(state.folder+"/")
      &&(!state.filter||f.path.toLowerCase().includes(state.filter.toLowerCase())));
    if(!filtered.length){
      files.append(elt("p","No hay archivos en esta carpeta. Arrastrá archivos o usá Importar.","assets-empty"));
    }
    for(const file of filtered){
      const row=elt("div",null,"assets-file-row");
      const icon=elt("span",{
        model:"◈",texture:"▧",audio:"♫",video:"▤",font:"Aa",
        script:"⌘",document:"▣",other:"◇"
      }[file.category]||"◇","assets-file-icon");
      const detail=elt("div",null,"assets-file-detail");
      const title=elt("strong",file.path.split("/").pop());
      title.title=file.path;
      const meta=elt("small",file.category+" · "+fmt(file.bytes)+" · SHA-256 "+file.sha256.slice(0,12));
      detail.append(title,meta);
      const actions=elt("div",null,"assets-row-actions");
      function action(caption,callback){
        const b=elt("button",caption);b.type="button";b.onclick=()=>task(callback);actions.append(b);
      }
      action("Ver",()=>preview(file));
      action("↓",()=>download(file));
      action("Mover",async()=>{
        const next=prompt("Nueva ruta dentro de assets/",file.path);
        if(!next||next===file.path)return;
        await json("/"+encode(state.project)+"/move",{
          method:"POST",headers:{"Content-Type":"application/json"},
          body:JSON.stringify({from:file.path,to:next})
        });
        message("Archivo movido a "+next);await refresh();
      });
      action("Eliminar",async()=>{
        if(!confirm("¿Eliminar permanentemente "+file.path+" del proyecto?"))return;
        await api("/"+encode(state.project)+"/file"+qpath(file.path),{method:"DELETE"});
        releasePreview();
        message("Archivo eliminado. El Resource Pack publicado no cambió.");
        await refresh();
      });
      row.append(icon,detail,actions);files.append(row);
    }
  }
  function mime(path){
    const ext=path.split(".").pop().toLowerCase();
    return {
      png:"image/png",webp:"image/webp",jpg:"image/jpeg",jpeg:"image/jpeg",gif:"image/gif",
      wav:"audio/wav",ogg:"audio/ogg",mp3:"audio/mpeg",flac:"audio/flac",
      webm:"video/webm",mp4:"video/mp4",mov:"video/quicktime",
      json:"application/json",bbmodel:"application/json",
      txt:"text/plain",md:"text/plain",js:"text/plain",ts:"text/plain",
      rs:"text/plain",java:"text/plain",py:"text/plain",
      mcfunction:"text/plain",html:"text/plain",css:"text/plain",svg:"text/plain"
    }[ext]||"application/octet-stream";
  }
  async function download(file){
    const response=await api("/"+encode(state.project)+"/file"+qpath(file.path));
    const blob=await response.blob();
    const url=URL.createObjectURL(blob);
    const link=elt("a");link.href=url;link.download=file.path.split("/").pop();
    document.body.append(link);link.click();link.remove();
    setTimeout(()=>URL.revokeObjectURL(url),30000);
    message("Descargado "+file.path.split("/").pop());
  }
  async function preview(file){
    releasePreview();
    const box=$("[data-assets-preview]");
    box.replaceChildren();
    const title=elt("strong",file.path);
    const info=elt("small",fmt(file.bytes)+" · "+file.sha256);
    box.append(title,info);
    const type=mime(file.path);
    if(!type.startsWith("image/")&&!type.startsWith("audio/")&&!type.startsWith("video/")
      &&!["application/json","text/plain"].includes(type)){
      box.append(elt("p","Archivo original conservado. Este formato se descarga sin ejecutarse.","assets-empty"));
      return;
    }
    if(file.bytes>12*1024*1024){
      box.append(elt("p","Vista previa omitida para evitar cargar más de 12 MiB en RAM. Usá Descargar.","assets-empty"));
      return;
    }
    const response=await api("/"+encode(state.project)+"/file"+qpath(file.path));
    const blob=await response.blob();
    if(type==="text/plain"||type==="application/json"){
      const text=await blob.text();
      const pre=elt("pre",text.slice(0,40000),"assets-text-preview");
      box.append(pre);
      if(text.length>40000)box.append(elt("small","Vista previa limitada a 40.000 caracteres."));
    }else{
      state.previewUrl=URL.createObjectURL(new Blob([blob],{type}));
      const view=elt(type.startsWith("image/")?"img":
        type.startsWith("audio/")?"audio":"video");
      view.src=state.previewUrl;view.controls=!type.startsWith("image/");
      if(view.tagName==="IMG")view.alt="Vista previa: "+file.path;
      view.preload="metadata";
      box.append(view);
    }
    message("Vista previa local del archivo original. No se publicó en VSPack.");
  }
  async function uploadFiles(files,folderImport=false){
    if(!state.project)throw Error("Elegí un proyecto.");
    if(!files.length)throw Error("Seleccioná uno o más archivos.");
    if(files.length>2000)throw Error("Máximo 2000 archivos por proyecto.");
    let count=0;
    for(const file of files){
      if(file.size===0||file.size>MAX_FILE)throw Error(
        file.name+" está vacío o supera 96 MiB.");
      const name=folderImport?(file.webkitRelativePath||file.name):file.name;
      const path=state.folder+"/"+name.replace(/\\/g,"/");
      message("Importando "+(++count)+"/"+files.length+" · "+file.name+"…");
      try{
        await api("/"+encode(state.project)+"/files"+qpath(path),{
          method:"POST",headers:{"Content-Type":"application/octet-stream"},body:file
        });
      }catch(error){
        if(!/Ya existe el archivo|HTTP 409/i.test(error.message))throw error;
        if(!confirm("Ya existe "+path+". ¿Reemplazar el original?"))continue;
        await api("/"+encode(state.project)+"/files"+qpath(path)+"&replace=true",{
          method:"POST",headers:{"Content-Type":"application/octet-stream"},body:file
        });
      }
    }
    await refresh();
    message("Importación completada. "+count+" archivos procesados. Resource Pack sin cambios.");
  }
  async function createProject(){
    const input=$("[data-assets-new-name]");
    const name=input.value.trim();
    if(!name)throw Error("Escribí un nombre de proyecto.");
    const id=slug(name)+"-"+Math.random().toString(36).slice(2,6);
    if(!slug(name))throw Error("Elegí un nombre con letras o números.");
    await json("/"+encode(id),{method:"POST",
      headers:{"Content-Type":"application/json"},
      body:JSON.stringify({name})});
    state.project=id;state.folder="assets";input.value="";
    await loadProjects();
    message("Proyecto "+name+" creado. Carpeta assets/ lista.");
  }
  async function createFolder(){
    if(!state.project)throw Error("Elegí un proyecto.");
    const name=prompt("Nueva carpeta dentro de "+state.folder+"/","nueva-carpeta");
    if(!name)return;
    const path=state.folder+"/"+name;
    await json("/"+encode(state.project)+"/folders",{
      method:"POST",headers:{"Content-Type":"application/json"},
      body:JSON.stringify({path})
    });
    await refresh();state.folder=path;render();
    message("Carpeta creada: "+path);
  }
  async function verify(){
    if(!state.project)throw Error("Elegí un proyecto.");
    message("Verificando SHA-256 de todos los archivos del proyecto…");
    const result=await json("/"+encode(state.project)+"/verify");
    if(result.ok)message("Integridad correcta: "+result.verified+" / "+result.total+" archivos.");
    else message("Se encontraron "+result.problems.length+" archivos modificados o dañados: "+
      result.problems.slice(0,3).map(i=>i.path).join(", "),true);
  }
  async function rescan(){
    if(!state.project)throw Error("Elegí un proyecto.");
    if(!confirm("¿Sincronizar los archivos actuales en disco y aceptar sus nuevas versiones SHA-256? No se publican a Minecraft."))return;
    message("Leyendo archivos locales, tamaños y SHA-256…");
    const result=await json("/"+encode(state.project)+"/rescan",{method:"POST"});
    await refresh();
    message("Sincronizado desde disco: "+result.files+" archivos. Resource Pack sin cambios.");
  }
  async function exportProject(){
    if(!state.project)throw Error("Elegí un proyecto.");
    message("Empaquetando originales y project.json, sin generar un Resource Pack…");
    const response=await api("/"+encode(state.project)+"/export");
    const file=await response.blob();
    const url=URL.createObjectURL(file);
    const link=elt("a");link.href=url;link.download=state.project+"-assets.zip";
    document.body.append(link);link.click();link.remove();
    setTimeout(()=>URL.revokeObjectURL(url),30000);
    message("Proyecto exportado: "+fmt(file.size)+". No se envió al cliente Minecraft.");
  }
  function mount(){
    const shell=document.querySelector(".app-shell");
    const main=document.querySelector(".workspace-main");
    const nav=document.querySelector(".app-tabs");
    if(!shell||!main||!nav)return;
    if(!nav.querySelector("[data-vsassets-tab]")){
      const tab=elt("button","VSAssets");
      tab.dataset.vsassetsTab="true";tab.type="button";
      tab.title="Biblioteca privada de archivos originales por proyecto";
      tab.onclick=open;nav.append(tab);
    }
    const grid=document.querySelector(".home-module-grid");
    if(grid&&!grid.querySelector("[data-vsassets-home]")){
      const card=elt("button");card.type="button";card.dataset.vsassetsHome="1";
      card.innerHTML='<span class="tile-symbol">▧</span><strong>VSAssets</strong>'+
        '<small>Proyectos · modelos · texturas · archivos originales</small>'+
        '<span class="tile-open">Abrir biblioteca ↗</span>';
      card.addEventListener("click",open);grid.append(card);
    }
    if(document.getElementById("vsassets-screen"))return;
    const page=elt("section",null,"workspace-assets-screen");
    page.id="vsassets-screen";page.hidden=true;page.setAttribute("aria-label","VSAssets — archivos del proyecto");
    page.innerHTML=[
      '<header class="assets-topline"><div><span class="eyebrow">VSMEDIA / PROYECTOS</span>',
      '<h1 data-assets-name>Biblioteca del proyecto</h1>',
      '<p data-assets-summary>Los archivos originales se conservan fuera del Resource Pack.</p></div>',
      '<span class="assets-private-mark">◈ Recursos privados · publicación manual</span></header>',
      '<div class="assets-project-toolbar">',
      '<label>Proyecto <select data-assets-project></select></label>',
      '<label>Nuevo proyecto <input data-assets-new-name maxlength="80" placeholder="Ej. Virgoland"></label>',
      '<button data-assets-new type="button" class="action-primary">＋ Crear proyecto</button>',
      '<button data-assets-refresh type="button" class="action-secondary">Actualizar</button>',
      '</div>',
      '<div class="assets-workspace-grid">',
      '<aside class="assets-sidebar"><div class="assets-panel-title">CARPETAS</div>',
      '<div data-assets-folders class="assets-folders"></div>',
      '<button type="button" data-assets-add-folder class="action-secondary">＋ Carpeta</button>',
      '<p class="assets-notice">Todo se guarda en assets/. El Resource Pack se construye por separado en VSPack.</p>',
      '</aside>',
      '<section class="assets-browser"><div class="assets-browser-top">',
      '<div class="assets-panel-title">ARCHIVOS ORIGINALES</div>',
      '<input data-assets-search placeholder="Buscar archivos o rutas…" aria-label="Buscar archivos">',
      '</div>',
      '<div class="assets-upload-tools">',
      '<label class="assets-file-upload action-primary">＋ Importar archivos',
      '<input data-assets-file-input type="file" multiple></label>',
      '<label class="assets-file-upload action-secondary">＋ Importar carpeta',
      '<input data-assets-folder-input type="file" webkitdirectory multiple></label>',
      '<button data-assets-export type="button" class="action-secondary">↓ Exportar proyecto ZIP</button>',
      '<button data-assets-verify type="button" class="action-secondary">Verificar integridad</button>',
      '<button data-assets-rescan type="button" class="action-secondary">Sincronizar carpeta del disco</button>',
      '</div>',
      '<div class="assets-drop-zone" data-assets-drop tabindex="0">',
      'Soltá archivos acá · se guardan en la carpeta seleccionada, sin publicar a Minecraft</div>',
      '<div data-assets-files class="assets-file-list"></div>',
      '</section>',
      '<aside class="assets-inspector"><div class="assets-panel-title">VISTA PREVIA</div>',
      '<div data-assets-preview class="assets-preview"><p class="assets-empty">',
      'Seleccioná un archivo para previsualizarlo.</p></div></aside></div>',
      '<p class="assets-status" data-assets-status role="status">',
      'La biblioteca guarda originales, incluidos .bbmodel, PSD, audio, vídeo y scripts. No forma parte del texture pack.</p>'
    ].join("");
    main.after(page);
    $("[data-assets-project]").onchange=e=>{
      state.project=e.target.value;state.folder="assets";releasePreview();
      task(refresh);
    };
    $("[data-assets-new]").onclick=()=>task(createProject);
    $("[data-assets-refresh]").onclick=()=>task(loadProjects);
    $("[data-assets-add-folder]").onclick=()=>task(createFolder);
    $("[data-assets-verify]").onclick=()=>task(verify);
    $("[data-assets-rescan]").onclick=()=>task(rescan);
    $("[data-assets-export]").onclick=()=>task(exportProject);
    $("[data-assets-search]").oninput=e=>{state.filter=e.target.value;render();};
    $("[data-assets-file-input]").onchange=e=>task(async()=>{
      await uploadFiles([...e.target.files]);e.target.value="";
    });
    $("[data-assets-folder-input]").onchange=e=>task(async()=>{
      await uploadFiles([...e.target.files],true);e.target.value="";
    });
    const drop=$("[data-assets-drop]");
    drop.ondragover=e=>{e.preventDefault();drop.classList.add("drag-over");};
    drop.ondragleave=()=>drop.classList.remove("drag-over");
    drop.ondrop=e=>{
      e.preventDefault();drop.classList.remove("drag-over");
      const files=[...e.dataTransfer.files];
      task(()=>uploadFiles(files));
    };
    document.addEventListener("click",event=>{
      if(!state.open)return;
      if(event.target?.closest?.(".app-tabs button:not([data-vsassets-tab])"))close();
    },true);
    window.addEventListener("pagehide",releasePreview);
    render();
  }
  const start=()=>{
    mount();
    new MutationObserver(mount).observe(document.body,{childList:true,subtree:true});
  };
  if(document.readyState==="loading")
    document.addEventListener("DOMContentLoaded",start,{once:true});
  else start();
})();
