/* VSRadio → VSCore: one real UTC station clock for Minecraft and web previews.
 * Enhances the existing Leptos playlist; does not replace the original editor.
 * Browser performs audio decode/resample/PCM conversion, never Folia.
 */
(() => {
  "use strict";
  const FALLBACK = "http://127.0.0.1:3061";
  const PREFIX = "/api/v1/radio";
  const MAX_TRACK_BYTES = 32 * 1024 * 1024;
  let active = null, timer = null, busy = false;

  async function request(path, options = {}) {
    let response;
    try { response = await fetch(PREFIX + path, options); } catch (_) {}
    if (!response || response.status === 404 || response.status === 501) {
      response = await fetch(FALLBACK + PREFIX + path, options);
    }
    if (!response.ok) {
      const body = await response.text();
      try { throw Error(JSON.parse(body).error || body); }
      catch (error) { if (error instanceof SyntaxError) throw Error(body || "VSCore " + response.status); throw error; }
    }
    return response;
  }
  const slug = name => name.normalize("NFD").replace(/[\u0300-\u036f]/g, "")
    .toLowerCase().replace(/[^a-z0-9_-]/g, "").slice(0, 48);
  const currentRows = () => [...document.querySelectorAll(".radio-stage .playlist-rows .playlist-row")];
  const currentName = () => document.querySelector(".radio-stage .playlist-tab.active > button")?.textContent?.trim() || "";
  function status(message, error = false) {
    const node = document.querySelector("[data-vsradio-publisher-status]");
    if (node) {
      node.textContent = message;
      node.style.color = error ? "#b94a52" : "";
    }
  }
  function wavMono48(buffer) {
    const samples = buffer.getChannelData(0);
    const byteLength = samples.length * 2;
    if (byteLength + 44 > MAX_TRACK_BYTES) throw Error("Esta pista supera 32 MiB en WAV mono. Acortala o dividila.");
    const result = new ArrayBuffer(44 + byteLength);
    const view = new DataView(result);
    const ascii = (pos, value) => [...value].forEach((char, i) => view.setUint8(pos + i, char.charCodeAt(0)));
    ascii(0, "RIFF"); view.setUint32(4, 36 + byteLength, true);
    ascii(8, "WAVE"); ascii(12, "fmt "); view.setUint32(16, 16, true);
    view.setUint16(20, 1, true); view.setUint16(22, 1, true);
    view.setUint32(24, 48000, true); view.setUint32(28, 96000, true);
    view.setUint16(32, 2, true); view.setUint16(34, 16, true);
    ascii(36, "data"); view.setUint32(40, byteLength, true);
    for (let i = 0; i < samples.length; ++i) {
      const clamped = Math.max(-1, Math.min(1, samples[i]));
      view.setInt16(44 + i * 2, clamped < 0 ? Math.round(clamped * 32768) : Math.round(clamped * 32767), true);
    }
    return new Blob([result], {type:"audio/wav"});
  }
  async function encodeUrl(url, decoder) {
    if (!url.startsWith("blob:")) {
      throw Error("Se requiere un archivo de la biblioteca local de VSMedia, no una URL remota.");
    }
    const payload = await (await fetch(url)).arrayBuffer();
    const decoded = await decoder.decodeAudioData(payload);
    if (decoded.duration > 340 || decoded.duration < 0.02) {
      throw Error("Cada pista debe durar entre 20 ms y 340 segundos para emitir por Plasmo Voice.");
    }
    const samples = Math.ceil(decoded.duration * 48000);
    const offline = new OfflineAudioContext(1, samples, 48000);
    const source = offline.createBufferSource();
    source.buffer = decoded;
    source.connect(offline.destination);
    source.start();
    const rendered = await offline.startRendering();
    return wavMono48(rendered);
  }
  async function publish() {
    if (busy) return;
    const name = currentName();
    const id = slug(name);
    const rows = currentRows();
    if (!name || !id || !rows.length) {
      status("Elegí una playlist con archivos antes de emitir en Minecraft.", true);
      return;
    }
    if (name.length > 80 || rows.length > 64) {
      status("La emisora admite como máximo 64 pistas y 80 caracteres de nombre.", true);
      return;
    }
    if (rows.some(row => !row.dataset.radioFileUrl)) {
      status("Falta algún archivo de la playlist. Reimportalo en la biblioteca.", true);
      return;
    }
    busy = true;
    const button = document.querySelector("[data-vsradio-publish]");
    if (button) button.disabled = true;
    const decoder = new (window.AudioContext || window.webkitAudioContext)();
    try {
      const tracks = [], durations = [];
      const fileUrls = rows.map(row => row.dataset.radioFileUrl);
      const local = document.getElementById("radio-player");
      const oldPlayback = local && !local.paused && fileUrls.includes(local.currentSrc || local.src);
      const currentIndex = oldPlayback ? fileUrls.indexOf(local.currentSrc || local.src) : -1;
      for (let index = 0; index < rows.length; ++index) {
        const number = String(index + 1).padStart(3, "0");
        const track = (id.slice(0, 34) + "_" + number).slice(0, 48);
        status("Preparando " + name + " · pista " + (index + 1) + "/" + rows.length + "…");
        const blob = await encodeUrl(fileUrls[index], decoder);
        const result = await (await request("/tracks/" + track, {
          method: "POST", headers: {"Content-Type": "audio/wav"}, body: blob
        })).json();
        tracks.push(result.id);
        durations.push(result.duration_ms);
      }
      let elapsed_ms;
      if (currentIndex >= 0) {
        elapsed_ms = durations.slice(0, currentIndex).reduce((a, b) => a + b, 0)
          + Math.round(Math.max(0, local.currentTime) * 1000);
      }
      const body = {display_name:name, playlist:tracks};
      if (typeof elapsed_ms === "number") {
        body.elapsed_ms = Math.min(Math.max(0, elapsed_ms), durations.reduce((a, b) => a + b, 0));
      }
      const now = await (await request("/stations/" + id, {
        method: "POST", headers: {"Content-Type":"application/json"}, body:JSON.stringify(body)
      })).json();
      active = {id, name, urls:fileUrls, tracks};
      window.dispatchEvent(new CustomEvent("vsradio:published",{detail:{id,name}}));
      status("● " + name + " emitiendo · /vsradio emisora " + name + " · "
        + Math.floor(now.position_ms / 1000) + " s");
      if (timer !== null) clearInterval(timer);
      timer = setInterval(() => syncPreview().catch(error => status(error.message, true)), 3000);
      await syncPreview();
    } catch (error) {
      status("No se pudo emitir: " + error.message, true);
    } finally {
      busy = false;
      await decoder.close().catch(() => {});
      if (button) button.disabled = false;
    }
  }
  async function syncPreview() {
    if (!active || currentName() !== active.name) return;
    const response = await (await request("/stations/" + active.id + "/now")).json();
    const audio = document.getElementById("radio-player");
    if (!audio) return;
    const rowIndex = active.tracks.indexOf(response.track);
    if (rowIndex < 0) return;
    const rows = currentRows();
    if (rows.length !== active.tracks.length || rows.some((row, i) => row.dataset.radioFileUrl !== active.urls[i])) {
      status("Playlist editada: volvé a emitir para actualizar la programación.", true);
      return;
    }
    const desired = Math.max(0, (response.position_ms + (response.playing ? Math.max(0,Date.now() - response.server_unix_ms) : 0)) / 1000);
    const source = active.urls[rowIndex];
    if (audio.currentSrc !== source && audio.src !== source) {
      // Click the existing song button to update the Leptos play_index as well.
      rows[rowIndex].querySelector(".playlist-song")?.click();
      audio.addEventListener("loadedmetadata", () => {
        try {audio.currentTime = Math.min(desired, audio.duration || desired);} catch (_) {}
      }, {once:true});
    } else if (audio.readyState >= 1 && Math.abs(audio.currentTime - desired) > 0.7) {
      audio.currentTime = Math.min(desired, audio.duration || desired);
    }
    if (!response.playing && !audio.paused) audio.pause();
    if (response.playing && audio.paused) audio.play().catch(() => {});
    status("● " + active.name + (response.playing?" en emisión":" en pausa") + " · " + response.track
      + " · " + Math.floor(response.position_ms / 1000) + " s · /vsradio emisora " + active.name);
  }
  async function control(action, extra={}) {
    if (!active) throw Error("Primero publicá una playlist como emisora.");
    const response=await (await request("/stations/"+active.id+"/control",{
      method:"POST",headers:{"Content-Type":"application/json"},
      body:JSON.stringify({action,...extra})
    })).json();
    await syncPreview();
    window.dispatchEvent(new CustomEvent("vsradio:control",{detail:{action,...extra,now:response}}));
    return response;
  }
  window.VSMediaRadio={get active(){return active;},control, syncPreview};
  function mount() {
    for (const toolbar of document.querySelectorAll(".studio-toolbar")) {
      if (toolbar.querySelector("[data-vsradio-publish]")) continue;
      if (!/^VSRadio$/i.test(toolbar.querySelector("strong")?.textContent?.trim() || "")) continue;
      const button = document.createElement("button");
      button.type = "button"; button.className = "toolbar-button";
      button.dataset.vsradioPublish = "1";
      button.textContent = "◉ Emitir en Minecraft";
      button.title = "Publica la playlist elegida en VSCore/Plasmo Voice sincronizada con su reproducción";
      button.addEventListener("click", publish);
      const label = document.createElement("span");
      label.dataset.vsradioPublisherStatus = "1";
      label.className = "toolbar-caption";
      label.textContent = "Sin emisora Minecraft";
      const toolbarControls=[["pause","Ⅱ Pausar emisión"],["resume","▶ Reanudar"],
        ["previous","⏮ Anterior"],["next","⏭ Siguiente"]];
      for(const [action,title] of toolbarControls){
        const key=document.createElement("button");
        key.type="button";key.className="toolbar-button";key.textContent=title;
        key.dataset.vsradioAction=action;
        key.addEventListener("click",()=>control(action).catch(e=>status(e.message,true)));
        toolbar.append(key);
      }
      const seek=document.createElement("button");
      seek.type="button";seek.className="toolbar-button";seek.textContent="⏱ Aplicar posición";
      seek.title="Mueve la emisión de TODOS los oyentes al tema y segundo elegidos en el reproductor";
      seek.addEventListener("click",()=>{
        if(!active){status("Publicá una playlist antes de cambiar su posición.",true);return;}
        const player=document.getElementById("radio-player");
        const index=active.urls.indexOf(player?.currentSrc||player?.src||"");
        if(index<0){status("Reproducí una canción válida para elegir el punto.",true);return;}
        control("seek",{index,position_ms:Math.max(0,Math.round(player.currentTime*1000))})
          .catch(e=>status(e.message,true));
      });
      toolbar.append(seek,button,label);
    }
  }
  function start() {
    mount();
    new MutationObserver(mount).observe(document.body, {childList:true, subtree:true});
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start, {once:true});
  else start();
})();
