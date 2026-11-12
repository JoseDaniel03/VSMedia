/* VSMedia Minecraft controls: additive UI on top of the original Studio.
 * Browser performs file reading; VSCore persists OGG/fonts and exports ZIP.
 * Folia only sends already published sound/title packets to players.
 */
(() => {
  "use strict";
  const API = "/api/v1";
  const FALLBACK = "http://127.0.0.1:3061/api/v1";
  let open = false;
  const status = (message, failed = false) => {
    const node = document.querySelector("#vs-minecraft-panel [data-mc-status]");
    if (node) { node.textContent = message; node.dataset.error = String(failed); }
  };
  function elm(tag, attrs = {}, text = "") {
    const e = document.createElement(tag);
    for (const [name, value] of Object.entries(attrs)) {
      if (name === "class") e.className = value;
      else e.setAttribute(name, String(value));
    }
    if (text) e.textContent = text;
    return e;
  }
  async function api(path, options = {}) {
    let response;
    try { response = await fetch(API + path, options); } catch (_) { /* Trunk dev path */ }
    if (!response || response.status === 404 || response.status === 501) {
      response = await fetch(FALLBACK + path, options);
    }
    if (!response.ok) {
      const text = await response.text();
      let message = text;
      try { message = JSON.parse(text).error || message; } catch (_) {}
      throw Error(message || "HTTP " + response.status);
    }
    return response;
  }
  function selected(selector) {
    return document.querySelector("#vs-minecraft-panel " + selector);
  }
  function value(name) { return selected('[name="' + name + '"]').value.trim(); }
  function loading(button, task) {
    if (button.disabled) return;
    button.disabled = true;
    status("Procesando…");
    Promise.resolve().then(task).then(message => status(message)).catch(e => status(e.message, true))
      .finally(() => { button.disabled = false; });
  }
  function action(data) {
    const encoded = new URLSearchParams(data);
    return fetch(API + "/minecraft/action", {
      method: "POST",
      headers: {"Content-Type":"application/x-www-form-urlencoded;charset=UTF-8",
                "X-VSMedia-Control":"1"},
      body: encoded
    }).then(async response => {
      if (!response.ok) throw Error(await response.text());
      return response.json();
    });
  }
  async function listSounds() {
    const data = await (await api("/sounds")).json();
    const select = selected('[name="sound"]');
    const old = select.value;
    select.replaceChildren();
    for (const sound of data.sounds || []) {
      select.append(elm("option", {value:sound.id}, sound.id + " · " + Math.ceil(sound.bytes/1024) + " KiB"));
    }
    if ([...select.options].some(o => o.value === old)) select.value = old;
    selected("[data-mc-sound-count]").textContent =
      (data.sounds || []).length + " sonidos añadidos al pack";
  }
  async function listFonts() {
    const data = await (await api("/fonts")).json();
    const select = selected('[name="font"]');
    const old = select.value;
    select.replaceChildren();
    for (const font of data.fonts || []) {
      const option = elm("option", {value:font.id}, (font.family || font.id) + " — " + font.id);
      option.dataset.preview = String(Boolean(font.previewAvailable));
      select.append(option);
    }
    if ([...select.options].some(o => o.value === old)) select.value = old;
    await previewFont();
  }
  async function previewFont() {
    const el = selected("[data-mc-font-preview]");
    const fontId = value("font");
    el.textContent = value("text") || "VSMedia · Vista previa de tipografía";
    el.style.fontFamily = "";
    if (!fontId) return;
    const option = selected('[name="font"]').selectedOptions[0];
    if (!option || option.dataset.preview !== "true") {
      el.title = "Sin TTF disponible para previsualización en el navegador. Usá Vista previa en Minecraft.";
      return;
    }
    try {
      const blob = await (await api("/fonts/file?font=" + encodeURIComponent(fontId))).blob();
      const family = "vsmedia-" + fontId.replace(/[^a-z0-9]/gi,"-");
      const face = new FontFace(family, await blob.arrayBuffer());
      await face.load();
      document.fonts.add(face);
      el.style.fontFamily = '"' + family + '", sans-serif';
      el.title = "Vista previa TTF cargada; confirmá la apariencia real dentro de Minecraft.";
    } catch (error) {
      el.title = "No se pudo previsualizar en el navegador: " + error.message;
    }
  }
  async function uploadSound() {
    const file = selected('[name="sound-file"]').files[0];
    const name = value("sound-id").toLowerCase();
    if (!file) throw Error("Elegí un OGG Vorbis para publicar.");
    if (!/^[a-z0-9_-]{1,48}$/.test(name)) throw Error("ID: 1–48 caracteres, minúsculas, números, guión o _.");
    if (file.size > 96*1024*1024) throw Error("Archivo de origen demasiado grande (96 MiB).");
    let ogg = file;
    if (!file.name.toLowerCase().endsWith(".ogg")) {
      if (!window.VSOggVorbis?.encodeBuffer) {
        throw Error("No se empaquetó el codificador OGG del navegador. Ejecutá scripts/build-browser-vorbis.sh al compilar.");
      }
      status("Convirtiendo a OGG Vorbis en tu navegador; no se procesa PCM en Folia…");
      const AudioCtx = window.AudioContext || window.webkitAudioContext;
      if (!AudioCtx) throw Error("Tu navegador no permite convertir este audio.");
      const context = new AudioCtx();
      try {
        const decoded = await context.decodeAudioData(await file.arrayBuffer());
        ogg = await window.VSOggVorbis.encodeBuffer(decoded,{quality:5});
      } finally {await context.close().catch(() => {});}
    }
    if (ogg.size > 16*1024*1024) throw Error("OGG comprimido supera 16 MiB. Acortá o dividí el audio.");
    const result = await (await api("/sounds/" + name, {
      method:"POST",headers:{"Content-Type":"audio/ogg"},body:ogg
    })).json();
    await listSounds();
    selected('[name="sound"]').value = result.id;
    return result.id + " incluido en el próximo ZIP. Falta distribuir el pack actualizado a los jugadores.";
  }
  function base64(bytes) {
    let binary = "";
    for (let p=0;p<bytes.length;p+=32768) {
      binary += String.fromCharCode(...bytes.subarray(p,p+32768));
    }
    return btoa(binary);
  }
  async function importFont() {
    const file = selected('[name="font-file"]').files[0];
    if (!file) throw Error("Elegí una fuente TTF/OTF.");
    if (file.size > 8*1024*1024) throw Error("La fuente supera 8 MiB.");
    const family = value("font-family") || file.name.replace(/\.(ttf|otf)$/i,"");
    const body = {family,source:"studio-upload",data:base64(new Uint8Array(await file.arrayBuffer()))};
    const result = await (await api("/fonts/import", {
      method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify(body)
    })).json();
    await listFonts();
    selected('[name="font"]').value = result.id;
    await previewFont();
    return "Fuente " + result.id + " importada. Se incluirá en el próximo ZIP; probala en Minecraft.";
  }
  async function exportPack() {
    const response = await api("/packs/export");
    const blob = await response.blob();
    if (!blob.size) throw Error("ZIP vacío");
    const url = URL.createObjectURL(blob);
    const link = elm("a", {href:url,download:"vsmedia-resourcepack.zip"});
    document.body.append(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url),10000);
    return "ZIP exportado (" + Math.ceil(blob.size/1024) +
      " KiB). Para que se aplique, publicalo en HTTPS y actualizá resource-pack en el plugin.";
  }
  function mount() {
    if (document.getElementById("vs-minecraft-panel")) return;
    const shell = elm("section",{id:"vs-minecraft-panel",class:"vs-minecraft-panel","aria-label":"Publicación Minecraft"});
    shell.innerHTML = `
      <button type="button" class="vs-minecraft-toggle" data-mc-toggle>✦ Minecraft · Publicar</button>
      <div class="vs-minecraft-body" hidden>
        <div class="vs-minecraft-heading"><strong>Publicación y prueba en Minecraft</strong>
          <button type="button" data-mc-close aria-label="Cerrar">×</button></div>
        <div class="vs-minecraft-tabs">
          <button type="button" data-mc-tab="audio" aria-pressed="true">Sonidos</button>
          <button type="button" data-mc-tab="fonts" aria-pressed="false">Tipografías</button>
          <button type="button" data-mc-tab="player" aria-pressed="false">Jugador</button>
        </div>
        <div data-mc-page="audio">
          <p>Subí OGG Vorbis o exportá WAV desde VSAudio/VSMix y elegí ese archivo. El navegador convierte a Vorbis y lo agrega al ZIP unificado.</p>
          <label>Audio exportado o OGG <input type="file" name="sound-file" accept=".ogg,.wav,.mp3,.flac,.webm,.opus,audio/*"></label>
          <label>ID del sonido <input name="sound-id" placeholder="intro, mezcla_01, radio_jingle" maxlength="48"></label>
          <button type="button" data-mc-upload-sound>＋ Agregar sonido al pack</button>
          <small data-mc-sound-count>Consultando sonidos…</small>
          <button type="button" data-mc-export>↓ Exportar Resource Pack ZIP</button>
          <small>Exportar el ZIP no lo instala a los jugadores. Publicalo en una URL HTTPS y actualizá el pack del plugin.</small>
        </div>
        <div data-mc-page="fonts" hidden>
          <label>Archivo TTF u OTF <input type="file" name="font-file" accept=".ttf,.otf,font/ttf,font/otf"></label>
          <label>Nombre de familia <input name="font-family" placeholder="Ejemplo: Cinématica"></label>
          <button type="button" data-mc-import-font>＋ Importar tipografía al pack</button>
          <small>TTF/OTF se convierte en provider del Resource Pack. La previsualización web no garantiza que el jugador ya lo tenga.</small>
        </div>
        <div data-mc-page="player" hidden>
          <label>Jugador <input name="player" placeholder="Nick de Minecraft o *" maxlength="16"></label>
          <label>Sonido publicado <select name="sound"></select></label>
          <div class="vs-minecraft-inline"><label>Volumen <input name="volume" value="1" type="number" min="0" max="4" step="0.1"></label>
            <label>Tono <input name="pitch" value="1" type="number" min="0.5" max="2" step="0.1"></label></div>
          <div class="vs-minecraft-inline"><button type="button" data-mc-play>▶ Reproducir al jugador</button>
            <button type="button" data-mc-stop>■ Detener</button></div>
          <label>Tipografía <select name="font"></select></label>
          <div data-mc-font-preview class="vs-minecraft-font-preview">VSMedia · Vista previa</div>
          <label>Color <input name="color" type="text" value="#F4D7A1" maxlength="16"></label>
          <label>Texto <input name="text" value="Bienvenidos a Virgoland" maxlength="512"></label>
          <div class="vs-minecraft-inline">
            <button type="button" data-mc-action="title">Title</button>
            <button type="button" data-mc-action="subtitle">Subtitle</button>
            <button type="button" data-mc-action="preview">Vista previa</button>
          </div>
          <button type="button" data-mc-action="chatfont">Aplicar fuente al chat de este jugador</button>
          <small>Se formatea el texto que ese jugador envía; no el cuadro de escritura ni la interfaz vanilla.</small>
        </div>
        <p role="status" data-mc-status>Los sonidos y fuentes se ven en Minecraft únicamente tras cargar el pack actualizado.</p>
      </div>
    `;
    document.body.append(shell);
    const show = yes => {
      open = yes;
      shell.querySelector(".vs-minecraft-body").hidden = !yes;
      shell.querySelector("[data-mc-toggle]").setAttribute("aria-expanded",String(yes));
    };
    const page = kind => {
      show(true);
      shell.querySelectorAll("[data-mc-page]").forEach(el => {el.hidden = el.dataset.mcPage !== kind;});
      shell.querySelectorAll("[data-mc-tab]").forEach(el => {el.setAttribute("aria-pressed",String(el.dataset.mcTab === kind));});
    };
    window.VSMinecraft = {open:page};
    shell.querySelector("[data-mc-toggle]").onclick = () => { show(!open); if(open) page("audio"); };
    shell.querySelector("[data-mc-close]").onclick = () => show(false);
    shell.querySelectorAll("[data-mc-tab]").forEach(el => el.onclick = () => page(el.dataset.mcTab));
    shell.querySelector("[data-mc-upload-sound]").onclick = e => loading(e.currentTarget,uploadSound);
    shell.querySelector("[data-mc-import-font]").onclick = e => loading(e.currentTarget,importFont);
    shell.querySelector("[data-mc-export]").onclick = e => loading(e.currentTarget,exportPack);
    shell.querySelector("[name=font]").onchange = () => previewFont();
    shell.querySelector("[name=text]").oninput = () => {
      const el = selected("[data-mc-font-preview]");
      if (el) el.textContent = value("text");
    };
    shell.querySelector("[data-mc-play]").onclick = e => loading(e.currentTarget,async () => {
      await action({action:"play",player:value("player"),sound:value("sound"),volume:value("volume"),pitch:value("pitch")});
      return "Orden de reproducción enviada. Confirmá que el jugador tiene el pack actualizado.";
    });
    shell.querySelector("[data-mc-stop]").onclick = e => loading(e.currentTarget,async () => {
      await action({action:"stopsound",player:value("player"),sound:value("sound")});
      return "Orden de detención programada.";
    });
    shell.querySelectorAll("[data-mc-action]").forEach(el => el.onclick = e =>
      loading(e.currentTarget,async () => {
        const kind = el.dataset.mcAction;
        await action({action:kind,player:value("player"),
          font:value("font"),color:value("color"),text:value("text")});
        return "Acción " + kind + " programada. Verificá el resultado dentro de Minecraft.";
      })
    );
    // The original Studio changes pages with Leptos without full reloads.
    // Insert only ONE contextual shortcut in each visible editor toolbar.
    function addShortcuts() {
      document.querySelectorAll(".studio-toolbar").forEach(toolbar => {
        if (toolbar.querySelector("[data-mc-shortcut]")) return;
        const title = toolbar.querySelector("strong")?.textContent || "";
        if (!/VSAudio|VSMix|VSRadio/i.test(title)) return;
        const button = elm("button",{type:"button",class:"toolbar-button","data-mc-shortcut":"1"},"✦ Publicar a Minecraft");
        button.onclick = () => page("audio");
        toolbar.append(button);
      });
    }
    const observer = new MutationObserver(addShortcuts);
    observer.observe(document.body,{subtree:true,childList:true});
    addShortcuts();
    Promise.allSettled([listSounds(),listFonts()]).then(results => {
      const failures = results.filter(r => r.status === "rejected");
      if (failures.length) status("VSCore no está disponible; comprobá el servicio 3061.",true);
    });
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", mount,{once:true});
  else mount();
})();
