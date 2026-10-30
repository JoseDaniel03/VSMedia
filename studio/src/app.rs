use leptos::prelude::*;
use crate::media::{self,AudioAsset,Folder,Version,Track,MixRegion,MixGainPoint,MixFadeCurve,Playlist};
use crate::{export,recording,studio_api};
use crate::mix_history::MixHistory;
use wasm_bindgen::JsCast;
use std::collections::{HashSet,HashMap};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {Home, Audio, Mix, Radio, Streams, Pack}
impl Page {
    fn title(self)->&'static str {match self {Self::Home=>"Home",Self::Audio=>"VSAudio",Self::Mix=>"VSMix",Self::Radio=>"VSRadio",Self::Streams=>"VSStreams",Self::Pack=>"VSPack"}}
}

#[component]
pub fn App() -> impl IntoView {
    let page = RwSignal::new(Page::Home);
    let assets = RwSignal::new(Vec::<AudioAsset>::new());
    let folders = RwSignal::new(media::load_folders());
    let selected = RwSignal::new(String::new());
    let status = RwSignal::new(String::new());
    let versions = RwSignal::new(Vec::<Version>::new());
    let current_version = RwSignal::new(String::new());
    let tracks = RwSignal::new(Vec::<Track>::new());
    let history=RwSignal::new(MixHistory::default());
    history.update(|item|item.observe(&[]));
    // Each completed local state change is one reversible checkpoint. Pointer
    // operations commit only on release, so no per-frame history allocation.
    Effect::new(move |_| {
        tracks.with(|current|history.update(|item|item.observe(current)));
    });
    let playlists = RwSignal::new(media::load_playlists());
    Effect::new(move |_| {
        playlists.with(|items|media::save_playlists(items));
    });
    let recording = RwSignal::new(false);
    let core_connected=RwSignal::new(false);
    studio_api::probe_engine(core_connected);
    view! {
        <div class="app-shell">
            <header class="app-topbar">
                <nav class="app-tabs" aria-label="Espacios de trabajo">
                    { [Page::Home,Page::Audio,Page::Mix,Page::Radio,Page::Streams,Page::Pack].into_iter().map(|target| view! {
                        <button type="button" class:active=move || page.get()==target
                            on:click=move |_|{crate::teleprompter::stop();page.set(target);}>{target.title()}</button>
                    }).collect_view() }
                </nav>
                <div class="app-header-right">
                    <button type="button" class="core-connection"
                        title="Consultar conexión local al motor y almacenamiento de VSMedia"
                        on:click=move |_|studio_api::probe_engine(core_connected)>
                        <span class="privacy-dot" class:offline=move ||!core_connected.get()></span>
                        {move ||if core_connected.get() {
                            "VSCore conectado · archivos locales".to_owned()
                        }else{
                            "VSCore sin conexión · modo local".to_owned()
                        }}
                        " ↻"
                    </button>
                </div>
            </header>
            <main class="workspace-main">
                <Show when=move ||page.get()==Page::Home>
                    <Home assets folders selected page status />
                </Show>
                <Show when=move ||page.get()==Page::Audio>
                    <SimpleAudio assets folders selected status versions current_version page />
                </Show>
                <Show when=move ||page.get()==Page::Mix>
                    <MixStudio assets folders selected status tracks history recording page />
                </Show>
                <Show when=move ||page.get()==Page::Radio>
                    <RadioStudio assets folders selected status playlists recording page />
                </Show>
                <Show when=move ||page.get()==Page::Streams>
                    <StreamsStudio page status />
                </Show>
                <Show when=move ||page.get()==Page::Pack>
                    <PackStudio page status />
                </Show>
            </main>
            <Show when=move ||!status.get().is_empty()>
                <div class="toast" role="status">{move ||status.get()}</div>
            </Show>
        </div>
    }
}

#[component]
fn Home(assets: RwSignal<Vec<AudioAsset>>,folders: RwSignal<Vec<Folder>>,selected: RwSignal<String>,
        page: RwSignal<Page>,status: RwSignal<String>) -> impl IntoView {
    view! {<div class="home-page">
        <div class="home-lead">
            <div><span class="eyebrow">"WORKSPACE / INICIO"</span>
                <h1>"Tu estudio. A tu manera."</h1>
                <p>"Organizá tus recursos y abrí exactamente la herramienta que necesitás. Todo empieza en un espacio limpio."</p>
            </div>
            <div class="home-visual" aria-hidden="true">
                <div class="visual-orbit orbit-a"></div><div class="visual-orbit orbit-b"></div>
                <div class="visual-lines"></div><div class="visual-core"></div>
            </div>
        </div>
        <div class="home-module-grid">
            <button type="button" on:click=move |_|page.set(Page::Audio)>
                <span class="tile-symbol">"∿"</span><strong>"VSAudio"</strong><small>"Un clip · versiones · edición · WAV"</small><span class="tile-open">"Abrir editor ↗"</span>
            </button>
            <button type="button" on:click=move |_|page.set(Page::Mix)>
                <span class="tile-symbol">"≋"</span><strong>"VSMix"</strong><small>"Multipista · mezcla · voces · doblaje"</small><span class="tile-open">"Abrir estudio ↗"</span>
            </button>
            <button type="button" on:click=move |_|page.set(Page::Radio)>
                <span class="tile-symbol">"◉"</span><strong>"VSRadio"</strong><small>"Playlists · programación · emisión P2P"</small><span class="tile-open">"Abrir consola ↗"</span>
            </button>
            <button type="button" on:click=move |_|page.set(Page::Streams)>
                <span class="tile-symbol">"◎"</span><strong>"VSStreams"</strong><small>"Plasmo Voice · proximidad · integración"</small><span class="tile-open">"Abrir salas ↗"</span>
            </button>
            <button type="button" on:click=move |_|page.set(Page::Pack)>
                <span class="tile-symbol">"▦"</span><strong>"VSPack"</strong><small>"ZIPs · capas · conflictos · exportación"</small><span class="tile-open">"Abrir gestor ↗"</span>
            </button>
        </div>
        <div class="home-library-head"><h2>"Biblioteca"</h2><span>"Carpetas, subcarpetas y clasificación de archivos"</span></div>
        <Library assets folders selected status />
    </div>}
}

#[component]
fn Library(assets: RwSignal<Vec<AudioAsset>>,folders: RwSignal<Vec<Folder>>,
           selected: RwSignal<String>,status: RwSignal<String>) -> impl IntoView {
    let active_folder=RwSignal::new("~all".to_owned());
    let folder_name=RwSignal::new(String::new());
    let add_folder_open=RwSignal::new(false);
    let selected_in_folder = move || {
        let which=active_folder.get();
        assets.get().into_iter().filter(|a| which=="~all" || a.folder==which).collect::<Vec<_>>()
    };
    view! {<div id="media-library" class="library-shell">
        <aside class="library-tree">
            <div class="pane-title">"CARPETAS"<button title="Crear carpeta o subcarpeta" type="button" on:click=move |_|add_folder_open.update(|v|*v=!*v)>"＋"</button></div>
            <button class="folder-row" class:active=move ||active_folder.get()=="~all"
                on:click=move |_| active_folder.set("~all".to_owned())>"▦  Todos los archivos"</button>
            <button class="folder-row" class:active=move ||active_folder.get().is_empty()
                on:click=move |_| active_folder.set(String::new())>"⌑  Sin carpeta"</button>
            <For each=move ||media::folder_rows(folders.get()) key=|f|f.id.clone() children=move |folder| {
                let id=folder.id.clone();let action_id=id.clone();let active_id=id.clone();
                let nested = folder.depth>0;
                view!{<button class="folder-row" class:nested=nested style=format!("padding-left: {}px",11+folder.depth*15)
                    class:active=move || active_folder.get()==active_id
                    on:click=move |_|active_folder.set(action_id.clone())>
                        "▸ "{folder.name}
                    </button>}
            }/>
            <Show when=move ||add_folder_open.get()><div class="folder-new">
                <input aria-label="Nombre de carpeta" placeholder="Nueva carpeta" prop:value=move || folder_name.get()
                    on:input=move |ev|folder_name.set(event_target_value(&ev)) />
                <button type="button" on:click=move |_|{
                    let name=folder_name.get_untracked().trim().to_owned();
                    if name.is_empty(){return;}
                    let parent=active_folder.get_untracked();
                    let parent=if parent=="~all"{String::new()}else{parent};
                    let id=media::uid("folder");
                    folders.update(|f|f.push(Folder{id:id.clone(),parent,name}));
                    media::save_folders(&folders.get_untracked());
                    active_folder.set(id);folder_name.set(String::new());add_folder_open.set(false);
                }>"Crear"</button>
            </div></Show>
            <small class="tree-help">"Para crear una subcarpeta, seleccioná primero la carpeta principal."</small>
        </aside>
        <section class="library-content">
            <div class="pane-title library-title">
                <span>"ARCHIVOS"<span class="subtle-count">{move ||format!(" · {}",selected_in_folder().len())}</span></span>
                <label class="action-primary file-action">"＋ Importar audio"
                    <input id="library-import" type="file" multiple accept="audio/*,.wav,.mp3,.ogg,.flac,.m4a,.webm"
                        on:change=move |ev| {
                            if let Some(input)=event_target::<web_sys::HtmlInputElement>(&ev).files() {
                                for i in 0..input.length(){if let Some(file)=input.get(i){
                                    let name=file.name();let blob: web_sys::Blob = file.into();
                                    if let Some(id)=media::add_blob(blob,name,assets,status) {
                                        selected.set(id.clone());
                                        let folder=active_folder.get_untracked();
                                        if folder!="~all" {assets.update(|a|if let Some(item)=a.iter_mut().find(|a|a.id==id){item.folder=folder.clone()});}
                                    }
                                }}
                            }
                        } />
                </label>
            </div>
            <Show when=move ||assets.get().is_empty()>
                <div class="empty-canvas"><span>"♪"</span><strong>"Todavía no hay archivos"</strong><p>"Importá audio desde tu equipo para empezar. Ningún archivo se envía a un servidor."</p></div>
            </Show>
            <Show when=move ||!assets.get().is_empty() && selected_in_folder().is_empty()>
                <div class="empty-canvas"><strong>"Esta carpeta está vacía"</strong><p>"Importá archivos o cambiales la carpeta desde el inspector."</p></div>
            </Show>
            <div class="asset-list">
                <For each=selected_in_folder key=|a|a.id.clone() children=move |asset|{
                    let aid=asset.id.clone();let remove_id=aid.clone();let select_id=aid.clone();let active_id=aid.clone();
                    view!{<div class="asset-row" class:active=move ||selected.get()==active_id>
                        <button class="asset-select" type="button" on:click=move |_|selected.set(select_id.clone())>
                            <span class="asset-icon">"♫"</span><span><strong>{asset.name}</strong><small>{format!("{:.1} MB",asset.bytes/1_048_576.)}</small></span>
                        </button>
                        <span class="asset-tag">{if asset.genre.is_empty(){"Sin género".to_owned()}else{asset.genre}}</span>
                        <button class="text-icon" type="button" title="Quitar de la sesión" on:click=move |_|{
                            if selected.get_untracked()==remove_id{selected.set(String::new());}
                            media::remove_asset(assets,&remove_id);
                        }>"×"</button>
                    </div>}
                }/>
            </div>
            <Show when=move ||!selected.get().is_empty() && assets.get().iter().any(|a|a.id==selected.get())>
                <div class="asset-inspector"><div class="pane-title">"CLASIFICACIÓN"</div>
                    <div class="inspector-grid">
                        <label>"Género"
                            <input placeholder="Ej. Rock, voz, ambiente" prop:value=move ||media::asset_by_id(assets,&selected.get()).map(|a|a.genre).unwrap_or_default()
                                on:input=move |ev| {let value=event_target_value(&ev);let id=selected.get_untracked();assets.update(|list|if let Some(a)=list.iter_mut().find(|a|a.id==id){a.genre=value.clone();});} />
                        </label>
                        <label>"Tipo"
                            <select prop:value=move ||media::asset_by_id(assets,&selected.get()).map(|a|a.media_type).unwrap_or_default()
                                on:change=move |ev| {let value=event_target_value(&ev);let id=selected.get_untracked();assets.update(|list|if let Some(a)=list.iter_mut().find(|a|a.id==id){a.media_type=value.clone();});}>
                                <option value="Audio">"Audio"</option><option value="Voz">"Voz"</option>
                                <option value="Música">"Música"</option><option value="Efecto">"Efecto"</option>
                                <option value="Ambiente">"Ambiente"</option><option value="Cuña">"Cuña"</option>
                            </select>
                        </label>
                        <label>"Carpeta"
                            <select prop:value=move ||media::asset_by_id(assets,&selected.get()).map(|a|a.folder).unwrap_or_default()
                                on:change=move |ev| {let value=event_target_value(&ev);let id=selected.get_untracked();assets.update(|list|if let Some(a)=list.iter_mut().find(|a|a.id==id){a.folder=value.clone();});}>
                                <option value="">"Sin carpeta"</option>
                                <For each=move ||folders.get() key=|f|f.id.clone() children=move |folder|{
                                    view!{<option value=folder.id>{folder.name}</option>}
                                }/>
                            </select>
                        </label>
                    </div>
                </div>
            </Show>
        </section>
    </div>}
}

fn mutate_version(versions: RwSignal<Vec<Version>>,current_version: RwSignal<String>,f: impl Fn(&mut Version)) {
    let id=current_version.get_untracked();
    versions.update(|list| {if let Some(v)=list.iter_mut().find(|v|v.id==id){f(v);}});
}

#[component]
fn SimpleAudio(assets: RwSignal<Vec<AudioAsset>>,folders: RwSignal<Vec<Folder>>,selected: RwSignal<String>,
               status: RwSignal<String>,versions: RwSignal<Vec<Version>>,current_version: RwSignal<String>,page: RwSignal<Page>) -> impl IntoView {
    let saved_projects=RwSignal::new(Vec::<studio_api::SavedAudioProject>::new());
    let project_picker=RwSignal::new(false);
    let selected_version=move ||versions.get().into_iter().find(|v|v.id==current_version.get());
    let clip_versions=move ||versions.get().into_iter().filter(|v|v.asset_id==selected.get()).collect::<Vec<Version>>();
    let selected_asset=move || assets.get().into_iter().find(|a|a.id==selected.get());
    view! {<div class="studio-screen">
        <div class="studio-toolbar">
            <EditorMenu page status />
            <strong>"VSAudio"</strong><span class="toolbar-separator"></span><span class="toolbar-caption">"EDICIÓN DE CLIP"</span>
            <button type="button" class="toolbar-button" on:click=move |_| {
                if let Some(asset)=selected_asset(){if let Some(blob)=media::get_blob(&asset.id){let _=media::download_blob(&blob,&asset.name);}}
                else {status.set("Seleccioná primero un archivo.".to_owned());}
            }>"↓ Original"</button>
            <button id="audio-export" type="button" class="toolbar-button accent" on:click=move |_|{
                if let (Some(a),Some(v))=(selected_asset(),selected_version()) {export::export_wav(a,v,status);}
                else {status.set("Elegí un clip y creá una versión antes de exportar.".to_owned());}
            }>"↓ Exportar WAV"</button>
            <button id="audio-open-projects" type="button" class="toolbar-button"
                on:click=move |_| {
                    let opened=!project_picker.get_untracked();
                    project_picker.set(opened);
                    if opened { studio_api::refresh_projects(saved_projects,status); }
                }>"☁ Proyectos guardados"</button>
            <button id="audio-preview-draft" type="button" class="toolbar-button"
                title="Escuchar el clip procesado por el motor Rust sin guardar el proyecto"
                on:click=move |_|{
                    let asset=assets.get_untracked().into_iter()
                        .find(|a|a.id==selected.get_untracked());
                    let version=versions.get_untracked().into_iter()
                        .find(|v|v.id==current_version.get_untracked());
                    if let (Some(asset),Some(version))=(asset,version) {
                        let duration=media::player("simple-player")
                            .map(|p|p.duration()).unwrap_or(0.);
                        studio_api::preview_draft_audio(asset,version,duration,status);
                    } else {
                        status.set("Seleccioná un clip y una versión para previsualizar.".into());
                    }
                }>"▶ Motor (borrador)"</button>
            <button id="audio-save-project" type="button" class="toolbar-button"
                title="Sube el audio seleccionado y guarda esta versión como proyecto nativo .vsaudio"
                on:click=move |_| {
                    let asset=assets.get_untracked().into_iter()
                        .find(|a|a.id==selected.get_untracked());
                    let version=versions.get_untracked().into_iter()
                        .find(|v|v.id==current_version.get_untracked());
                    if let (Some(asset),Some(version))=(asset,version) {
                        let duration=media::player("simple-player")
                            .map(|p|p.duration()).unwrap_or(0.);
                        studio_api::save_audio_project(asset,version,duration,status);
                    } else {
                        status.set("Seleccioná un archivo y una versión para guardar.".to_owned());
                    }
                }>"☁ Guardar proyecto"</button>
        </div>
        <Show when=move ||project_picker.get()>
            <section class="saved-projects-panel" aria-label="Proyectos de audio guardados">
                <div class="pane-title"><span>"PROYECTOS DEL MOTOR"</span>
                    <button type="button" class="toolbar-button"
                        on:click=move |_|studio_api::refresh_projects(saved_projects,status)>"↻ Actualizar"</button>
                </div>
                <div class="saved-projects-list">
                    <Show when=move ||saved_projects.get().is_empty()>
                        <p class="field-note">"Sin proyectos disponibles. Guardá una versión o comprobá que VSCore esté iniciado."</p>
                    </Show>
                    <For each=move ||saved_projects.get() key=|entry|entry.name.clone() children=move |entry|{
                        let name=entry.name.clone();
                        let preview_name=entry.name.clone();
                        let download_name=entry.name.clone();
                        view!{<div class="version-row">
                            <strong>{entry.name.clone()}</strong>
                            <small>{format!("Revisión {}",entry.revision)}</small>
                            <button type="button" class="action-secondary" on:click=move |_|{
                                studio_api::open_saved_project(name.clone(),assets,selected,versions,current_version,status);
                                project_picker.set(false);
                            }>"Abrir"</button>
                            <button type="button" class="action-secondary"
                                title="Render real del proyecto WAV: EQ, fades, paneo y automatización"
                                on:click=move |_|studio_api::preview_engine(
                                    preview_name.clone(),"studio-engine-audio",status
                                )>"▶ Motor"</button>
                            <button type="button" class="action-secondary"
                                on:click=move |_|studio_api::export_engine_wav(
                                    download_name.clone(),status
                                )>"↓ WAV"</button>
                        </div>}
                    }/>
                </div>
                <div class="engine-preview">
                    <audio id="studio-engine-audio" controls preload="none"></audio>
                    <small>"Render procesado por VSCore · fuentes WAV · no modifica los originales"</small>
                </div>
            </section>
        </Show>
        <div class="studio-grid single-grid">
            <div class="studio-library"><Library assets folders selected status /></div>
            <div class="single-stage">
                <div class="stage-topline"><span>"CLIP ACTIVO"</span><span>"Edición no destructiva"</span></div>
                <Show when=move ||selected_asset().is_none()>
                    <div class="empty-canvas large-empty"><span>"∿"</span><strong>"Seleccioná un clip de la biblioteca"</strong>
                        <p>"La edición simple trabaja sobre un archivo a la vez, sin modificar el original."</p></div>
                </Show>
                <Show when=move ||selected_asset().is_some()>
                    <h2 class="clip-title">{move ||selected_asset().map(|a|a.name).unwrap_or_default()}</h2>
                    <div class="audio-preview">
                        <audio id="simple-player" controls preload="metadata"
                            src=move ||selected_asset().map(|a|a.url).unwrap_or_default()
                            on:timeupdate=move |_| {
                                if let (Some(v),Some(player))=(selected_version(),media::player("simple-player")) {
                                    if v.end>0. && player.current_time()>=v.end {player.pause().ok();}
                                }
                            }>
                            "Tu navegador no admite audio HTML5."
                        </audio>
                    </div>
                    <div class="mix-draft-preview">
                        <audio id="simple-draft-engine-audio" controls preload="none"></audio>
                        <small>"WAV procesado por Rust · sin crear proyecto · fuentes utilizadas pueden subirse al CAS local"</small>
                    </div>
                    <div class="version-heading"><strong>"Versiones de este clip"</strong>
                        <button id="audio-new-version" class="action-primary" type="button" on:click=move |_| {
                            let asset_id=selected.get_untracked();
                            if asset_id.is_empty(){return;}
                            let count=versions.get_untracked().iter().filter(|v|v.asset_id==asset_id).count()+1;
                            let id=media::uid("version");
                            versions.update(|list|list.push(Version {id:id.clone(),asset_id,
                                label:format!("Versión {}",count),start:0.,end:0.,
                                gain:1.,fade_in:0.,fade_out:0.,reversed:false,
                                pan:0.,low_pass_hz:None,high_pass_hz:None,end_gain:None}));
                            current_version.set(id);
                        }>"＋ Nueva versión"</button>
                    </div>
                    <div class="version-list">
                        <For each=clip_versions
                            key=|v|v.id.clone() children=move |version| {
                            let id=version.id.clone();let click_id=id.clone();let active_id=id.clone();let del_id=id.clone();
                            view!{<div class="version-row" class:active=move ||current_version.get()==active_id>
                                <button type="button" on:click=move |_|current_version.set(click_id.clone())>{version.label}</button>
                                <button type="button" class="text-icon" title="Eliminar versión" on:click=move |_|{
                                    versions.update(|list|list.retain(|v|v.id!=del_id));
                                    if current_version.get_untracked()==del_id{current_version.set(String::new());}
                                }>"×"</button>
                            </div>}
                        }/>
                    </div>
                    <Show when=move ||selected_version().is_some()>
                        <div id="audio-effect-target" class="edit-controls">
                            <label class="wide-field">"Nombre de versión"
                                <input prop:value=move ||selected_version().map(|v|v.label).unwrap_or_default()
                                    on:input=move |ev|{let val=event_target_value(&ev);mutate_version(versions,current_version,|v|v.label=val.clone());}/>
                            </label>
                            <div class="controls-quad">
                                <label>"Inicio (segundos)"<input type="number" min="0" step="0.1"
                                    prop:value=move ||selected_version().map(|v|v.start.to_string()).unwrap_or_default()
                                    on:change=move |ev| {if let Ok(val)=event_target_value(&ev).parse::<f64>(){mutate_version(versions,current_version,|v|v.start=val.max(0.));}}/></label>
                                <label>"Final (0 = completo)"<input type="number" min="0" step="0.1"
                                    prop:value=move ||selected_version().map(|v|v.end.to_string()).unwrap_or_default()
                                    on:change=move |ev| {if let Ok(val)=event_target_value(&ev).parse::<f64>(){mutate_version(versions,current_version,|v|v.end=val.max(0.));}}/></label>
                                <label>"Fade in (s)"<input type="number" min="0" max="300" step="0.1"
                                    prop:value=move ||selected_version().map(|v|v.fade_in.to_string()).unwrap_or_default()
                                    on:change=move |ev| {if let Ok(val)=event_target_value(&ev).parse::<f64>(){mutate_version(versions,current_version,|v|v.fade_in=val.max(0.));}}/></label>
                                <label>"Fade out (s)"<input type="number" min="0" max="300" step="0.1"
                                    prop:value=move ||selected_version().map(|v|v.fade_out.to_string()).unwrap_or_default()
                                    on:change=move |ev| {if let Ok(val)=event_target_value(&ev).parse::<f64>(){mutate_version(versions,current_version,|v|v.fade_out=val.max(0.));}}/></label>
                            </div>
                            <label class="range-field">"Ganancia para exportación"<span>{move ||format!("{:.0}%",selected_version().map(|v|v.gain*100.).unwrap_or(100.))}</span>
                                <input type="range" min="0" max="200" step="5"
                                    prop:value=move || selected_version().map(|v|(v.gain*100.).to_string()).unwrap_or_default()
                                    on:input=move |ev| {if let Ok(val)=event_target_value(&ev).parse::<f64>() {
                                        mutate_version(versions,current_version,|v|v.gain=val/100.);
                                        if let Some(p)=media::player("simple-player"){p.set_volume((val/100.).min(1.));}
                                    }} />
                            </label>
                            <div class="fx-controls">
                                <div class="pane-title">"EFECTOS Y AUTOMATIZACIÓN · MOTOR RUST"</div>
                                <label class="range-field">"Paneo (izquierda ↔ derecha)"
                                    <span>{move ||format!("{:+.0}%",selected_version()
                                        .map(|v|v.pan*100.).unwrap_or(0.))}</span>
                                    <input type="range" min="-100" max="100" step="5"
                                        prop:value=move ||selected_version()
                                            .map(|v|(v.pan*100.).round().to_string()).unwrap_or_default()
                                        on:input=move |ev| {
                                            if let Ok(v)=event_target_value(&ev).parse::<f64>() {
                                                mutate_version(versions,current_version,
                                                    |clip|clip.pan=(v/100.).clamp(-1.,1.));
                                            }
                                        }/>
                                </label>
                                <div class="controls-quad">
                                    <label>"Pasa-altos (Hz; vacío = desactivado)"
                                        <input type="number" min="20" max="18000" step="10"
                                            placeholder="Desactivado"
                                            prop:value=move ||selected_version()
                                                .and_then(|v|v.high_pass_hz)
                                                .map(|hz|hz.to_string()).unwrap_or_default()
                                            on:change=move |ev|{
                                                let raw=event_target_value(&ev);
                                                let hz=if raw.trim().is_empty(){None}
                                                    else{raw.parse::<f64>().ok()
                                                        .filter(|value|value.is_finite() && (20.0..=18_000.0).contains(value))};
                                                mutate_version(versions,current_version,|clip|clip.high_pass_hz=hz);
                                            }/>
                                    </label>
                                    <label>"Pasa-bajos (Hz; vacío = desactivado)"
                                        <input type="number" min="20" max="18000" step="10"
                                            placeholder="Desactivado"
                                            prop:value=move ||selected_version()
                                                .and_then(|v|v.low_pass_hz)
                                                .map(|hz|hz.to_string()).unwrap_or_default()
                                            on:change=move |ev|{
                                                let raw=event_target_value(&ev);
                                                let hz=if raw.trim().is_empty(){None}
                                                    else{raw.parse::<f64>().ok()
                                                        .filter(|value|value.is_finite() && (20.0..=18_000.0).contains(value))};
                                                mutate_version(versions,current_version,|clip|clip.low_pass_hz=hz);
                                            }/>
                                    </label>
                                    <label>"Ganancia al final del clip (0–200%; vacío = constante)"
                                        <input type="number" min="0" max="200" step="5"
                                            placeholder="Sin automatización"
                                            prop:value=move ||selected_version()
                                                .and_then(|v|v.end_gain)
                                                .map(|g|(g*100.).round().to_string()).unwrap_or_default()
                                            on:change=move |ev|{
                                                let raw=event_target_value(&ev);
                                                let final_gain=if raw.trim().is_empty(){None}
                                                    else{raw.parse::<f64>().ok()
                                                        .filter(|value|value.is_finite() && (0.0..=200.0).contains(value))
                                                        .map(|value|value/100.)};
                                                mutate_version(versions,current_version,|clip|clip.end_gain=final_gain);
                                            }/>
                                    </label>
                                </div>
                                <small class="field-note">"Estos controles se guardan en .vsaudio y se oyen con ▶ Motor en Proyectos guardados. La reproducción local y el WAV rápido no aplican todavía EQ ni paneo."</small>
                            </div>
                            <label class="checkbox-field"><input type="checkbox"
                                prop:checked=move ||selected_version().map(|v|v.reversed).unwrap_or(false)
                                on:change=move |ev|{let val=event_target_checked(&ev);mutate_version(versions,current_version,|v|v.reversed=val);}/>
                                "Invertir audio al exportar"</label>
                            <button id="audio-play" type="button" class="action-secondary" on:click=move |_| {
                                if let (Some(v),Some(p))=(selected_version(),media::player("simple-player")){
                                    p.set_current_time(v.start);
                                    p.set_volume(v.gain.min(1.));
                                    let _=p.play();
                                }
                            }>"▶ Escuchar desde el recorte"</button>
                            <small class="field-note">"Los recortes, fades y reversa se aplican realmente al WAV exportado. El reproductor usa el original, con inicio y volumen ajustados."</small>
                        </div>
                    </Show>
                </Show>
            </div>
        </div>
    </div>}
}

fn sync_mix(tracks: RwSignal<Vec<Track>>) {
    let current=tracks.get_untracked();
    let solo=current.iter().any(|t|t.solo);
    for t in current {
        if let Some(player)=media::player(&format!("track-{}",t.id)) {
            let position=player.current_time();
            let duration=player.duration();
            let in_factor=if t.fade_in>0. {(position/t.fade_in).clamp(0.,1.)} else {1.};
            let out_factor=if t.fade_out>0. && duration.is_finite() && duration>0. {
                ((duration-position)/t.fade_out).clamp(0.,1.)
            } else {1.};
            let gain=if t.muted || (solo && !t.solo){0.}else{t.volume.clamp(0.,1.)*in_factor.min(out_factor)};
            player.set_volume(gain);
        }
    }
}

fn edit_mix_region(
    tracks:RwSignal<Vec<Track>>,track_id:&str,region_id:&str,
    update:impl FnOnce(&mut MixRegion)
) {
    tracks.update(|list|{
        if let Some(region)=list.iter_mut().find(|track|track.id==track_id)
            .and_then(|track|track.regions.iter_mut()
                .find(|region|region.id==region_id)) {
            update(region);
            crate::gain_editor::trim_end(region);
        }
    });
}
/// Reactive field reads stay bound to the persistent region identity, not
/// the original keyed-list snapshot. Pointer edits and automatic crossfades
/// therefore update the numeric controls immediately.
fn mix_region_value(
    tracks:RwSignal<Vec<Track>>,track_id:&str,region_id:&str,
    read:impl FnOnce(&MixRegion)->String
)->String {
    tracks.with(|list|list.iter().find(|track|track.id==track_id)
        .and_then(|track|track.regions.iter().find(|region|region.id==region_id))
        .map(read).unwrap_or_default())
}
#[component]
fn RegionEditor(
    track_id:String,region:MixRegion,
    tracks:RwSignal<Vec<Track>>,assets:RwSignal<Vec<AudioAsset>>
)->impl IntoView {
    let rid=region.id.clone();
    let t_asset=track_id.clone();let r_asset=rid.clone();
    let t_start=track_id.clone();let r_start=rid.clone();
    let t_length=track_id.clone();let r_length=rid.clone();
    let t_source=track_id.clone();let r_source=rid.clone();
    let t_gain=track_id.clone();let r_gain=rid.clone();
    let t_pan=track_id.clone();let r_pan=rid.clone();
    let t_in=track_id.clone();let r_in=rid.clone();
    let t_out=track_id.clone();let r_out=rid.clone();
    let t_remove=track_id.clone();let r_remove=rid.clone();
    let t_curve=track_id.clone();let r_curve=rid.clone();
    let show_asset={let tid=track_id.clone();let rid=rid.clone();
        move || mix_region_value(tracks,&tid,&rid,|r|r.asset_id.clone())};
    let show_start={let tid=track_id.clone();let rid=rid.clone();
        move || mix_region_value(tracks,&tid,&rid,|r|r.start.to_string())};
    let show_duration={let tid=track_id.clone();let rid=rid.clone();
        move || mix_region_value(tracks,&tid,&rid,|r|r.duration.to_string())};
    let show_source={let tid=track_id.clone();let rid=rid.clone();
        move || mix_region_value(tracks,&tid,&rid,|r|r.source_in.to_string())};
    let show_gain={let tid=track_id.clone();let rid=rid.clone();
        move || mix_region_value(tracks,&tid,&rid,|r|(r.volume*100.).round().to_string())};
    let show_pan={let tid=track_id.clone();let rid=rid.clone();
        move || mix_region_value(tracks,&tid,&rid,|r|(r.pan*100.).round().to_string())};
    let show_in={let tid=track_id.clone();let rid=rid.clone();
        move || mix_region_value(tracks,&tid,&rid,|r|r.fade_in.to_string())};
    let show_out={let tid=track_id.clone();let rid=rid.clone();
        move || mix_region_value(tracks,&tid,&rid,|r|r.fade_out.to_string())};
    let show_curve={let tid=track_id.clone();let rid=rid.clone();
        move || mix_region_value(tracks,&tid,&rid,|r|r.fade_curve.as_str().to_owned())};
    view! {
        <div class="mix-region">
            <div class="mix-region-heading">
                <strong>"REGIÓN"</strong>
                <small>{format!("#{}",rid.rsplit('-').next().unwrap_or("1"))}</small>
                <button type="button" class="text-icon" title="Eliminar región"
                    on:click=move |_|tracks.update(|list| {
                        if let Some(track)=list.iter_mut().find(|t|t.id==t_remove) {
                            track.regions.retain(|r|r.id!=r_remove);
                            // Do not resurrect the original legacy track after
                            // deleting the last region in the region editor.
                            if track.regions.is_empty(){track.asset_id.clear();}
                        }
                    })>"×"</button>
            </div>
            <select aria-label="Audio de región"
                prop:value=show_asset
                on:change=move |ev|{
                    let id=event_target_value(&ev);
                    edit_mix_region(tracks,&t_asset,&r_asset,|r|r.asset_id=id);
                }>
                <option value="">"Elegí un audio para esta región"</option>
                <For each=move||assets.get() key=|a|a.id.clone() children=move |asset|{
                    view!{<option value=asset.id>{asset.name}</option>}
                }/>
            </select>
            <div class="region-fields">
                <label>"Inicio en pista (s)"<input type="number" min="0" max="3600" step="0.1"
                    prop:value=show_start
                    on:change=move |ev| { if let Ok(value)=event_target_value(&ev).parse::<f64>() {
                        if value.is_finite(){edit_mix_region(tracks,&t_start,&r_start,|r|r.start=value.max(0.));}
                    } }/></label>
                <label>"Duración (s)"<input type="number" min="0.1" max="3600" step="0.1"
                    prop:value=show_duration
                    on:change=move |ev| { if let Ok(value)=event_target_value(&ev).parse::<f64>() {
                        if value.is_finite(){edit_mix_region(tracks,&t_length,&r_length,|r|r.duration=value.max(0.1));}
                    } }/></label>
                <label>"Desde origen (s)"<input type="number" min="0" max="3600" step="0.1"
                    prop:value=show_source
                    on:change=move |ev| { if let Ok(value)=event_target_value(&ev).parse::<f64>() {
                        if value.is_finite(){edit_mix_region(tracks,&t_source,&r_source,|r|r.source_in=value.max(0.));}
                    } }/></label>
                <label>"Volumen (%)"<input type="number" min="0" max="200" step="5"
                    prop:value=show_gain
                    on:change=move |ev| { if let Ok(value)=event_target_value(&ev).parse::<f64>() {
                        if value.is_finite(){edit_mix_region(tracks,&t_gain,&r_gain,|r|r.volume=(value/100.).clamp(0.,2.));}
                    } }/></label>
                <label>"Paneo (%)"<input type="number" min="-100" max="100" step="5"
                    prop:value=show_pan
                    on:change=move |ev| { if let Ok(value)=event_target_value(&ev).parse::<f64>() {
                        if value.is_finite(){edit_mix_region(tracks,&t_pan,&r_pan,|r|r.pan=(value/100.).clamp(-1.,1.));}
                    } }/></label>
                <label>"Fade in (s)"<input type="number" min="0" max="3600" step="0.1"
                    prop:value=show_in
                    on:change=move |ev| { if let Ok(value)=event_target_value(&ev).parse::<f64>() {
                        if value.is_finite(){edit_mix_region(tracks,&t_in,&r_in,|r|r.fade_in=value.max(0.));}
                    } }/></label>
                <label>"Curva de fades"
                    <select aria-label="Curva de fades" prop:value=show_curve
                        on:change=move |ev| {
                            let value=event_target_value(&ev);
                            if let Some(curve)=MixFadeCurve::from_str(&value) {
                                edit_mix_region(tracks,&t_curve,&r_curve,|r|r.fade_curve=curve);
                            }
                        }>
                        <option value="linear">"Lineal"</option>
                        <option value="equal_power">"Potencia constante"</option>
                    </select>
                </label>
                <label>"Fade out (s)"<input type="number" min="0" max="3600" step="0.1"
                    prop:value=show_out
                    on:change=move |ev| { if let Ok(value)=event_target_value(&ev).parse::<f64>() {
                        if value.is_finite(){edit_mix_region(tracks,&t_out,&r_out,|r|r.fade_out=value.max(0.));}
                    } }/></label>
            </div>
            <crate::gain_editor::GainEditor tracks=tracks track_id=track_id.clone()
                region_id=rid.clone()/>
        </div>
    }
}

fn history_step(
    history:RwSignal<MixHistory>,tracks:RwSignal<Vec<Track>>,
    status:RwSignal<String>,undo:bool
){
    let mut restore=None;
    history.update(|h|restore=if undo{h.undo()}else{h.redo()});
    if let Some(snapshot)=restore {
        tracks.set(snapshot);
        status.set(if undo{"Cambio deshecho."}else{"Cambio rehecho."}.into());
    }
}
#[component]
fn MixStudio(assets: RwSignal<Vec<AudioAsset>>,folders: RwSignal<Vec<Folder>>,selected: RwSignal<String>,
             status: RwSignal<String>,tracks: RwSignal<Vec<Track>>,
             history:RwSignal<MixHistory>,recording: RwSignal<bool>,page: RwSignal<Page>) -> impl IntoView {
    let selected_track=RwSignal::new(String::new());
    let saved_projects=RwSignal::new(Vec::<studio_api::SavedAudioProject>::new());
    let show_saved=RwSignal::new(false);
    let script=RwSignal::new(media::storage_get("vsmedia:dubbing:script"));
    let scroll_speed=RwSignal::new(50.0f64);
    let script_running=RwSignal::new(false);
    view! {<div class="studio-screen"
        on:keydown=move |event:web_sys::KeyboardEvent|{
            // Preserve native text editing shortcuts in focused form inputs.
            let editing=event.target()
                .and_then(|target|target.dyn_into::<web_sys::Element>().ok())
                .is_some_and(|element|{
                    let tag=element.tag_name();
                    tag=="INPUT" || tag=="TEXTAREA" || tag=="SELECT"
                        || element.closest("[contenteditable=true]").ok().flatten().is_some()
                });
            if editing {return;}
            let ctrl=event.ctrl_key() || event.meta_key();
            let key=event.key().to_lowercase();
            let undo=ctrl && key=="z" && !event.shift_key();
            let redo=ctrl && (key=="y" || (key=="z" && event.shift_key()));
            if undo || redo {
                event.prevent_default();
                history_step(history,tracks,status,undo);
            }
        }>
        <div class="studio-toolbar">
            <EditorMenu page status />
            <strong>"VSMix"</strong><span class="toolbar-separator"></span><span class="toolbar-caption">"ESTUDIO MULTIPISTA"</span>
            <button type="button" class="toolbar-button" title="Deshacer (Ctrl+Z)"
                disabled=move ||!history.with(|h|h.can_undo())
                on:click=move |_|history_step(history,tracks,status,true)>"↶ Deshacer"</button>
            <button type="button" class="toolbar-button" title="Rehacer (Ctrl+Y o Ctrl+Mayús+Z)"
                disabled=move ||!history.with(|h|h.can_redo())
                on:click=move |_|history_step(history,tracks,status,false)>"↷ Rehacer"</button>
            <small class="mix-history-count">{move ||history.with(|h|
                if h.oversized{"Historial pausado".into()}else{format!("{} estados",h.size())})}</small>
            <button id="mix-add-track" type="button" class="toolbar-button" on:click=move |_|{
                let id=media::uid("track");
                let count=tracks.get_untracked().len()+1;
                tracks.update(|list|list.push(Track{id:id.clone(),name:format!("Pista {}",count),asset_id:String::new(),volume:1.,muted:false,solo:false,fade_in:0.,fade_out:0.,regions:Vec::new()}));
                selected_track.set(id);
            }>"＋ Pista"</button>
            <button id="mix-open-projects" type="button" class="toolbar-button"
                on:click=move |_| {
                    let active=!show_saved.get_untracked();
                    show_saved.set(active);
                    if active {studio_api::refresh_projects(saved_projects,status);}
                }>"☁ Abrir mezcla"</button>
            <button id="mix-save-project" type="button" class="toolbar-button accent"
                title="Publica los audios asignados a cada pista y guarda una sesión multipista .vsaudio"
                on:click=move |_|studio_api::save_mix_project(
                    assets.get_untracked(),tracks.get_untracked(),status
                )>"☁ Guardar mezcla"</button>
            <button id="mix-preview-draft" type="button" class="toolbar-button"
                title="Procesa la mezcla actual en Rust sin crear una revisión de proyecto"
                on:click=move |_|studio_api::preview_draft_mix(
                    assets.get_untracked(),tracks.get_untracked(),status
                )>"▶ Motor (borrador)"</button>
            <button id="mix-play" type="button" class="toolbar-button" on:click=move |_|{
                if tracks.get_untracked().iter().any(|t|!t.regions.is_empty()) {
                    status.set("Esta mezcla tiene regiones: utilizá ▶ Motor (borrador) para escuchar el resultado real.".into());
                    return;
                }
                for track in tracks.get_untracked(){
                    if let Some(p)=media::player(&format!("track-{}",track.id)) {
                        p.pause().ok();p.set_current_time(0.);
                    }
                }
                sync_mix(tracks);
                for track in tracks.get_untracked(){
                    if !track.asset_id.is_empty(){if let Some(p)=media::player(&format!("track-{}",track.id)){let _=p.play();}}
                }
            }>"▶ Reproducir"</button>
            <button type="button" class="toolbar-button" on:click=move |_|{
                for track in tracks.get_untracked(){if let Some(p)=media::player(&format!("track-{}",track.id)){p.pause().ok();}}
            }>"Ⅱ Pausar"</button>
            <Show when=move ||!recording.get()>
                <button id="mix-record" type="button" class="toolbar-button record-button" on:click=move |_|{
                    let active_id=selected_track.get_untracked();
                    if active_id.is_empty(){status.set("Seleccioná una pista para grabar.".to_owned());return;}
                    recording::start(assets,status,recording,move |file_id|{
                        tracks.update(|list|if let Some(t)=list.iter_mut().find(|t|t.id==active_id){t.asset_id=file_id.clone();});
                    });
                }>"● Grabar pista"</button>
            </Show>
            <Show when=move ||recording.get()>
                <button type="button" class="toolbar-button record-button active" on:click=move |_|recording::stop(recording,status)>"■ Detener grabación"</button>
            </Show>
        </div>
        <Show when=move ||show_saved.get()>
            <section class="saved-projects-panel" aria-label="Mezclas guardadas">
                <div class="pane-title"><span>"PROYECTOS DE AUDIO EN VSCORE"</span>
                    <button type="button" class="toolbar-button"
                        on:click=move |_|studio_api::refresh_projects(saved_projects,status)>"↻ Actualizar"</button>
                </div>
                <div class="saved-projects-list">
                    <Show when=move ||saved_projects.get().is_empty()>
                        <p class="field-note">"Sin proyectos guardados. Guardá una mezcla o comprobá el backend."</p>
                    </Show>
                    <For each=move ||saved_projects.get() key=|entry|entry.name.clone() children=move |entry|{
                        let name=entry.name.clone();
                        let preview_name=entry.name.clone();
                        let download_name=entry.name.clone();
                        view!{<div class="version-row">
                            <strong>{entry.name.clone()}</strong>
                            <small>{format!("Revisión {}",entry.revision)}</small>
                            <button type="button" class="action-secondary" on:click=move |_|{
                                studio_api::open_saved_mix(name.clone(),assets,tracks,status);
                                selected_track.set(String::new());
                                show_saved.set(false);
                            }>"Abrir en VSMix"</button>
                            <button type="button" class="action-secondary"
                                title="Reproduce una mezcla renderizada por el Audio Engine de Rust"
                                on:click=move |_|studio_api::preview_engine(
                                    preview_name.clone(),"mix-engine-audio",status
                                )>"▶ Motor"</button>
                            <button type="button" class="action-secondary"
                                on:click=move |_|studio_api::export_engine_wav(
                                    download_name.clone(),status
                                )>"↓ WAV"</button>
                        </div>}
                    }/>
                </div>
                <div class="engine-preview">
                    <audio id="mix-engine-audio" controls preload="none"></audio>
                    <small>"Previsualización Rust de la mezcla guardada · WAV únicamente"</small>
                </div>
            </section>
        </Show>
        <div class="studio-grid mix-grid">
            <div class="studio-library"><Library assets folders selected status /></div>
            <section class="mix-stage">
                <div class="stage-topline"><span>"PISTAS"</span><span>{move ||format!("{} canales",tracks.get().len())}</span></div>
                <div class="mix-draft-preview">
                    <audio id="mix-draft-engine-audio" controls preload="none"></audio>
                    <small>"Previsualiza todos los clips con Rust · no guarda proyecto · puede subir al CAS las fuentes utilizadas"</small>
                </div>
                <Show when=move ||tracks.get().is_empty()>
                    <div class="empty-canvas large-empty"><span>"≋"</span><strong>"Tu sesión no tiene pistas"</strong>
                    <p>"Añadí pistas, vinculá archivos de la biblioteca o grabá directamente en un canal."</p></div>
                </Show>
                <crate::mix_timeline::MixTimeline tracks=tracks assets=assets status=status />
                <div class="track-list">
                    <For each=move ||tracks.get() key=|t|t.id.clone() children=move |track| {
                        let tid=track.id.clone();
                        let id_name=tid.clone();let id_select=tid.clone();let id_file=tid.clone();
                        let id_mute=tid.clone();let id_mute_click=tid.clone();let id_solo=tid.clone();let id_solo_click=tid.clone();let id_vol=tid.clone();let id_vol_input=tid.clone();let id_fade_in=tid.clone();let id_fade_in_change=tid.clone();let id_fade_out=tid.clone();let id_fade_out_change=tid.clone();let id_remove=tid.clone();
                        let id_active=tid.clone();let id_click=tid.clone();let id_player=tid.clone();let id_region_add=tid.clone();let id_region_count=tid.clone();
                        let id_region_list=tid.clone();let id_region_track=tid.clone();
                        view!{<div class="track-channel" class:selected=move ||selected_track.get()==id_active
                            on:click=move |_|selected_track.set(id_click.clone())>
                            <div class="track-heading">
                                <span class="track-led"></span>
                                <input aria-label="Nombre de pista" prop:value=move ||tracks.get().iter().find(|t|t.id==id_name).map(|t|t.name.clone()).unwrap_or_default()
                                    on:input=move |ev| {let value=event_target_value(&ev);let id=tid.clone();tracks.update(|list|if let Some(t)=list.iter_mut().find(|t|t.id==id){t.name=value.clone()});}/>
                                <button class="text-icon" type="button" title="Eliminar pista" on:click=move |_|{
                                    tracks.update(|list|list.retain(|t|t.id!=id_remove));
                                    if selected_track.get_untracked()==id_remove{selected_track.set(String::new());}
                                }>"×"</button>
                            </div>
                            <div class="track-controls">
                                <button type="button" class:lit=move ||tracks.get().iter().find(|t|t.id==id_mute).map(|t|t.muted).unwrap_or(false)
                                    on:click=move |_|{tracks.update(|list|if let Some(t)=list.iter_mut().find(|t|t.id==id_mute_click){t.muted=!t.muted;});sync_mix(tracks);}>"M"</button>
                                <button type="button" class:lit=move ||tracks.get().iter().find(|t|t.id==id_solo).map(|t|t.solo).unwrap_or(false)
                                    on:click=move |_|{tracks.update(|list|if let Some(t)=list.iter_mut().find(|t|t.id==id_solo_click){t.solo=!t.solo;});sync_mix(tracks);}>"S"</button>
                                <label>"Vol"<input type="range" min="0" max="100" step="1"
                                    prop:value=move ||tracks.get().iter().find(|t|t.id==id_vol).map(|t|(t.volume*100.).round().to_string()).unwrap_or_default()
                                    on:input=move |ev|{
                                        if let Ok(value)=event_target_value(&ev).parse::<f64>(){tracks.update(|list|if let Some(t)=list.iter_mut().find(|t|t.id==id_vol_input){t.volume=value/100.;});sync_mix(tracks);}
                                    }/></label>
                            </div>
                            <div class="track-fades">
                                <label>"Entrada (s)"<input type="number" min="0" step="0.5"
                                    prop:value=move ||tracks.get().iter().find(|t|t.id==id_fade_in).map(|t|t.fade_in.to_string()).unwrap_or_default()
                                    on:change=move |ev| {if let Ok(x)=event_target_value(&ev).parse::<f64>() {
                                        tracks.update(|list|if let Some(t)=list.iter_mut().find(|t|t.id==id_fade_in_change){t.fade_in=x.max(0.);});sync_mix(tracks);
                                    }}/></label>
                                <label>"Salida (s)"<input type="number" min="0" step="0.5"
                                    prop:value=move ||tracks.get().iter().find(|t|t.id==id_fade_out).map(|t|t.fade_out.to_string()).unwrap_or_default()
                                    on:change=move |ev| {if let Ok(x)=event_target_value(&ev).parse::<f64>() {
                                        tracks.update(|list|if let Some(t)=list.iter_mut().find(|t|t.id==id_fade_out_change){t.fade_out=x.max(0.);});sync_mix(tracks);
                                    }}/></label>
                            </div>
                            <select aria-label="Archivo asociado a la pista" class="track-media-select"
                                prop:value=move ||tracks.get().iter().find(|t|t.id==id_file).map(|t|t.asset_id.clone()).unwrap_or_default()
                                on:change=move |ev|{
                                    let value=event_target_value(&ev);
                                    tracks.update(|list|if let Some(t)=list.iter_mut().find(|t|t.id==id_select){t.asset_id=value.clone()});
                                }>
                                <option value="">"Elegir audio de biblioteca…"</option>
                                <For each=move ||assets.get() key=|a|a.id.clone() children=move |asset|{
                                    view!{<option value=asset.id>{asset.name}</option>}
                                }/>
                            </select>
                            <div class="mix-regions-toolbar">
                                <strong>{move||format!("{} regiones",tracks.get().iter()
                                    .find(|t|t.id==id_region_count).map_or(0,|t|t.regions.len()))}</strong>
                                <button type="button" class="action-secondary"
                                    title="Agregar una región editable en la línea de tiempo"
                                    on:click=move |_|{
                                        let id=id_region_add.clone();
                                        let mut missing=false;
                                        tracks.update(|list|{
                                            let Some(track)=list.iter_mut().find(|t|t.id==id) else{return;};
                                            if track.asset_id.is_empty(){missing=true;return;}
                                            if track.regions.len()>=32 {missing=true;return;}
                                            let duration=media::player(&format!("track-{}",track.id))
                                                .map(|p|p.duration()).filter(|n|n.is_finite() && *n>0.)
                                                .unwrap_or(5.).min(3600.);
                                            let start=track.regions.last()
                                                .map_or(0.,|r|r.start+r.duration);
                                            track.regions.push(MixRegion{
                                                id:media::uid("region"),asset_id:track.asset_id.clone(),
                                                start,duration,source_in:0.,volume:1.,pan:0.,
                                                fade_in:0.,fade_out:0.,fade_curve:MixFadeCurve::Linear,
                                                gain_points:Vec::new()
                                            });
                                        });
                                        if missing {
                                            status.set("Seleccioná un archivo base y usá un máximo de 32 regiones por pista.".to_owned());
                                        }
                                    }>"＋ Región"</button>
                            </div>
                            <div class="mix-regions-list">
                                <For each=move||tracks.get().iter().find(|t|t.id==id_region_list)
                                    .map(|t|t.regions.clone()).unwrap_or_default()
                                    key=|r|r.id.clone()
                                    children=move |region|{
                                        view!{<RegionEditor track_id=id_region_track.clone()
                                            region=region tracks=tracks assets=assets/>}
                                    }/>
                            </div>
                            <div class="track-wave-slot">
                                <span>{move ||tracks.get().iter().find(|t|t.id==id_player).and_then(|t|media::asset_by_id(assets,&t.asset_id)).map(|a|a.name).unwrap_or_else(||"Pista vacía".to_owned())}</span>
                            </div>
                            <audio id=format!("track-{}",track.id) preload="metadata" class="hidden-audio"
                                on:timeupdate=move |_|sync_mix(tracks)
                                src=move ||tracks.get().iter().find(|t|t.id==track.id).and_then(|t|media::asset_by_id(assets,&t.asset_id)).map(|a|a.url).unwrap_or_default()></audio>
                        </div>}
                    }/>
                </div>
                <div class="mix-guidance">"M = silenciar · S = escuchar solo · los canales reproducen archivos independientes. Grabá micrófono en la pista seleccionada."</div>
            </section>
            <aside class="script-panel">
                <div class="stage-topline"><span>"LECTOR DE GUION / DOBLAJE"</span></div>
                <label>"Texto para lectura o interpretación"
                    <textarea class="script-input" placeholder="Pegá aquí un diálogo, guion de doblaje o locución…"
                        prop:value=move ||script.get() on:input=move |ev|{
                            let value=event_target_value(&ev);script.set(value.clone());media::storage_set("vsmedia:dubbing:script",&value);
                        }></textarea>
                </label>
                <div class="script-controls">
                    <label>"Desplazamiento"<input type="range" min="10" max="200" step="10"
                        prop:value=move ||scroll_speed.get().to_string()
                        on:input=move |ev| {if let Ok(v)=event_target_value(&ev).parse::<f64>(){scroll_speed.set(v);}}/></label>
                    <span>{move ||format!("{:.0} px/s",scroll_speed.get())}</span>
                </div>
                <div class="script-actions">
                    <Show when=move ||!script_running.get()>
                        <button type="button" class="action-primary" on:click=move |_|{
                            crate::teleprompter::start(scroll_speed);script_running.set(true);
                        }>"▶ Desplazar"</button>
                    </Show>
                    <Show when=move ||script_running.get()>
                        <button type="button" class="action-secondary" on:click=move |_|{
                            crate::teleprompter::stop();script_running.set(false);
                        }>"Ⅱ Pausar"</button>
                    </Show>
                    <button type="button" class="action-secondary" on:click=move |_|{
                        crate::teleprompter::stop();script_running.set(false);
                        if let Some(el)=web_sys::window().and_then(|w|w.document()).and_then(|d|d.get_element_by_id("script-reader")){
                            if let Ok(elem)=wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(el){elem.set_scroll_top(0);}
                        }
                    }>"↺ Inicio"</button>
                </div>
                <div id="script-reader" class="script-reader">{move ||script.get()}</div>
                <small>"El texto se guarda localmente. El desplazamiento no modifica la velocidad de grabación."</small>
            </aside>
        </div>
    </div>}
}

/// Match the active in-memory Blob first; otherwise reconcile a reimported
/// local file by its persisted filename. No file bytes enter localStorage.
fn radio_resolve_asset(assets:RwSignal<Vec<AudioAsset>>,playlist:&Playlist,
    source_id:&str)->Option<AudioAsset>{
    media::asset_by_id(assets,source_id).or_else(||{
        let filename=playlist.source_names.get(source_id)?;
        assets.with_untracked(|all|all.iter().find(|asset|asset.name==*filename).cloned())
    })
}
fn radio_available(assets:RwSignal<Vec<AudioAsset>>,playlist:&Playlist)->HashSet<String>{
    playlist.items.iter().filter(|source_id|
        radio_resolve_asset(assets,playlist,source_id).is_some()
    ).cloned().collect()
}
fn play_radio(assets: RwSignal<Vec<AudioAsset>>,playlists: RwSignal<Vec<Playlist>>,
              active_playlist: RwSignal<String>,index: RwSignal<usize>,status: RwSignal<String>) {
    let pid=active_playlist.get_untracked();
    let Some(playlist)=playlists.with_untracked(|all|all.iter().find(|p|p.id==pid).cloned()) else {
        status.set("Creá o seleccioná una playlist.".into());return;
    };
    let ids=radio_available(assets,&playlist);
    // Playlist source IDs persist even though reimported Blob IDs change.
    let Some(pos)=(index.get_untracked()..playlist.items.len())
        .find(|&i|ids.contains(&playlist.items[i])) else {
        status.set("No quedan archivos disponibles. Reimportá los audios si recargaste el editor.".into());
        if let Some(player)=media::player("radio-player"){let _=player.pause();}
        return;
    };
    index.set(pos);
    let Some(asset)=radio_resolve_asset(assets,&playlist,&playlist.items[pos]) else{return;};
    if let Some(player)=media::player("radio-player") {
        player.set_src(&asset.url);
        let _=player.play();
        status.set(format!("VSRadio · {} ({}/{})",asset.name,pos+1,playlist.items.len()));
    }
}
fn radio_advance(assets: RwSignal<Vec<AudioAsset>>,playlists: RwSignal<Vec<Playlist>>,
    active_playlist:RwSignal<String>,index:RwSignal<usize>,status:RwSignal<String>,
    repeat:bool,backward:bool) {
    let pid=active_playlist.get_untracked();
    let Some(playlist)=playlists.with_untracked(|all|all.iter().find(|p|p.id==pid).cloned()) else{
        status.set("Seleccioná una playlist.".into());return;
    };
    let ids=radio_available(assets,&playlist);
    let next=if backward {
        crate::radio_queue::previous_playable(&playlist.items,&ids,index.get_untracked())
    }else {
        crate::radio_queue::next_playable(&playlist.items,&ids,index.get_untracked(),repeat)
    };
    if let Some(next)=next{
        index.set(next);
        play_radio(assets,playlists,active_playlist,index,status);
    }else{
        if let Some(player)=media::player("radio-player"){let _=player.pause();}
        status.set(if backward{"No hay pistas anteriores disponibles."}else{"La playlist terminó. Podés activar Repetir."}.into());
    }
}

#[component]
fn RadioStudio(assets: RwSignal<Vec<AudioAsset>>,folders: RwSignal<Vec<Folder>>,selected: RwSignal<String>,
               status: RwSignal<String>,playlists: RwSignal<Vec<Playlist>>,recording: RwSignal<bool>,page: RwSignal<Page>) -> impl IntoView {
    let new_playlist=RwSignal::new(String::new());
    let active_playlist=RwSignal::new(media::storage_get("vsmedia:radio:active_playlist"));
    Effect::new(move |_|media::storage_set("vsmedia:radio:active_playlist",&active_playlist.get()));
    let play_index=RwSignal::new(0usize);
    let repeat_radio=RwSignal::new(false);
    let schedule=RwSignal::new(media::storage_get("vsmedia:radio:schedule"));
    let active_items=move ||playlists.get().into_iter().find(|p|p.id==active_playlist.get()).map(|p|p.items).unwrap_or_default();
    let indexed_items=move ||active_items().into_iter().enumerate().collect::<Vec<(usize,String)>>();
    view!{<div class="studio-screen">
        <div class="studio-toolbar">
            <EditorMenu page status />
            <strong>"VSRadio"</strong><span class="toolbar-separator"></span><span class="toolbar-caption">"CONSOLA LOCAL"</span>
            <span class="connection-badge">"● Emisión P2P · conectar para activar"</span>
            <button id="radio-play" type="button" class="toolbar-button" on:click=move |_|{
                play_index.set(0);
                play_radio(assets,playlists,active_playlist,play_index,status);
            }>"▶ Reproducir playlist"</button>
            <button type="button" class="toolbar-button" on:click=move |_|{
                if let Some(player)=media::player("radio-player"){player.pause().ok();}
            }>"Ⅱ Pausar"</button>
            <button type="button" class="toolbar-button" title="Pista anterior"
                on:click=move |_|radio_advance(assets,playlists,active_playlist,play_index,status,false,true)>"◀ Anterior"</button>
            <button type="button" class="toolbar-button" title="Siguiente pista disponible"
                on:click=move |_|radio_advance(assets,playlists,active_playlist,play_index,status,repeat_radio.get_untracked(),false)>"Siguiente ▶"</button>
            <button type="button" class="toolbar-button"
                class:lit=move ||repeat_radio.get()
                title="Al terminar la lista, volver a empezar"
                on:click=move |_|repeat_radio.update(|repeat|*repeat=!*repeat)>
                {move ||if repeat_radio.get(){"↻ Repetir: sí"}else{"↻ Repetir: no"}}
            </button>
            <Show when=move ||!recording.get()>
                <button id="radio-record" type="button" class="toolbar-button record-button" on:click=move |_|{
                    recording::start(assets,status,recording,move |file_id|{
                        let pid=active_playlist.get_untracked();
                        if !pid.is_empty(){
                            playlists.update(|list|if let Some(p)=list.iter_mut().find(|p|p.id==pid){
                                if p.items.len()<crate::radio_queue::MAX_ITEMS{
                                    if let Some(a)=media::asset_by_id(assets,&file_id){
                                        p.source_names.insert(file_id.clone(),a.name.chars().take(160).collect());
                                    }
                                    p.items.push(file_id.clone());
                                }
                            });
                        }
                    });
                }>"● Grabar micrófono"</button>
            </Show>
            <Show when=move ||recording.get()>
                <button type="button" class="toolbar-button record-button active" on:click=move |_|recording::stop(recording,status)>"■ Detener"</button>
            </Show>
        </div>
        <div class="studio-grid radio-grid">
            <div class="studio-library"><Library assets folders selected status /></div>
            <section class="radio-stage">
                <div class="stage-topline"><span>"PLAYLISTS"</span><span>{move ||format!("{} listas",playlists.get().len())}</span></div>
                <div class="playlist-create">
                    <input id="radio-new-playlist" aria-label="Nombre de playlist" placeholder="Nombre de nueva playlist" prop:value=move ||new_playlist.get()
                        on:input=move |ev|new_playlist.set(event_target_value(&ev)) />
                    <button class="action-primary" type="button" on:click=move |_|{
                        let name=new_playlist.get_untracked().trim().to_owned();
                        if !crate::radio_queue::valid_name(&name){
                            status.set("Nombre de lista inválido (máximo 80 caracteres).".into());return;
                        }
                        if playlists.with_untracked(|all|all.len()>=crate::radio_queue::MAX_PLAYLISTS){
                            status.set("Máximo 32 playlists por sesión.".into());return;
                        }
                        let id=media::uid("playlist");
                        playlists.update(|list|list.push(Playlist{id:id.clone(),name,items:Vec::new(),source_names:HashMap::new()}));
                        active_playlist.set(id);new_playlist.set(String::new());
                    }>"＋ Crear"</button>
                </div>
                <div class="playlist-tabs">
                    <For each=move ||playlists.get() key=|p|p.id.clone() children=move |list|{
                        let id=list.id.clone();let active_id=id.clone();let choose_id=id.clone();let remove_id=id.clone();
                        view!{<div class="playlist-tab" class:active=move ||active_playlist.get()==active_id>
                            <button type="button" on:click=move |_|{active_playlist.set(choose_id.clone());play_index.set(0);}>{list.name}</button>
                            <button type="button" class="text-icon" title="Eliminar playlist" on:click=move |_|{
                                playlists.update(|all|all.retain(|p|p.id!=remove_id));
                                if active_playlist.get_untracked()==remove_id{active_playlist.set(String::new());}
                            }>"×"</button>
                        </div>}
                    }/>
                </div>
                <Show when=move ||active_playlist.get().is_empty()>
                    <div class="empty-canvas"><span>"◉"</span><strong>"Creá tu primera playlist"</strong><p>"Elegí un nombre y agregá música o grabaciones de tu biblioteca."</p></div>
                </Show>
                <Show when=move ||!active_playlist.get().is_empty()>
                    <div class="playlist-actions">
                        <button class="action-secondary" type="button" on:click=move |_|{
                            let file=selected.get_untracked();let pid=active_playlist.get_untracked();
                            if file.is_empty(){status.set("Seleccioná un archivo de la biblioteca.".to_owned());return;}
                            let mut added=false;
                            playlists.update(|all|if let Some(p)=all.iter_mut().find(|p|p.id==pid){
                                if p.items.len()<crate::radio_queue::MAX_ITEMS {
                                    if let Some(a)=media::asset_by_id(assets,&file){
                                        p.source_names.insert(file.clone(),a.name.chars().take(160).collect());
                                    }
                                    p.items.push(file.clone());added=true;
                                }
                            });
                            if !added{status.set("La playlist está llena o no fue encontrada (máximo 512 pistas).".into());}
                        }>"＋ Agregar archivo seleccionado"</button>
                        <span>{move ||format!("{} archivos",active_items().len())}</span>
                    </div>
                    <div class="playlist-rows">
                        <For each=indexed_items
                            key=|(i,id)|format!("{}-{}",i,id) children=move |(i,file_id)|{
                            let file_for_play=file_id.clone();
                            let file_name=playlists.with_untracked(|all|all.iter()
                                .find(|p|p.id==active_playlist.get_untracked())
                                .and_then(|p|radio_resolve_asset(assets,p,&file_id).map(|a|a.name)
                                    .or_else(||p.source_names.get(&file_id).cloned())))
                                .unwrap_or("Archivo no disponible".into());
                            view!{<div class="playlist-row">
                                <span class="playlist-order">{format!("{:02}",i+1)}</span>
                                <button type="button" class="playlist-song" on:click=move |_|{
                                    let matched=playlists.with_untracked(|all|all.iter()
                                        .find(|p|p.id==active_playlist.get_untracked())
                                        .and_then(|p|radio_resolve_asset(assets,p,&file_for_play)));
                                    if let Some(a)=matched{
                                        play_index.set(i);
                                        if let Some(p)=media::player("radio-player"){p.set_src(&a.url);let _=p.play();}
                                    }
                                }>{file_name}</button>
                                <button type="button" title="Subir" on:click=move |_|{
                                    let pid=active_playlist.get_untracked();
                                    playlists.update(|list|if let Some(p)=list.iter_mut().find(|p|p.id==pid){if i>0 && i<p.items.len(){p.items.swap(i,i-1);}});
                                }>"↑"</button>
                                <button type="button" title="Bajar" on:click=move |_|{
                                    let pid=active_playlist.get_untracked();
                                    playlists.update(|list|if let Some(p)=list.iter_mut().find(|p|p.id==pid){if i+1<p.items.len(){p.items.swap(i,i+1);}});
                                }>"↓"</button>
                                <button type="button" title="Quitar de playlist" on:click=move |_|{
                                    let pid=active_playlist.get_untracked();
                                    playlists.update(|list|if let Some(p)=list.iter_mut().find(|p|p.id==pid){if i<p.items.len(){
                                        let removed=p.items.remove(i);
                                        if !p.items.contains(&removed){p.source_names.remove(&removed);}
                                    }});
                                }>"×"</button>
                            </div>}
                        }/>
                    </div>
                </Show>
                <div class="vs-live-panel" data-vslive="radio"></div>
                <div class="radio-player-block"><div class="pane-title">"REPRODUCTOR LOCAL"</div>
                    <audio id="radio-player" controls preload="metadata"
                        on:ended=move |_|{
                            radio_advance(assets,playlists,active_playlist,play_index,status,
                                repeat_radio.get_untracked(),false);
                        }>"Audio HTML5 no disponible"</audio>
                    <small>"Se guardan listas y nombres, no archivos binarios. Si recargás, reimportá audios con el mismo nombre para recuperarlos. P2P inicia sólo cuando conectás la emisión."</small>
                </div>
            </section>
            <aside class="schedule-panel"><div class="stage-topline"><span>"NOTAS DE PROGRAMACIÓN"</span></div>
                <label>"Guion, horarios previstos y notas (sin emisión programada automática)"
                    <textarea placeholder="Ejemplo: 18:00 — Inicio del programa…"
                        prop:value=move ||schedule.get()
                        on:input=move |ev|{
                            let value=event_target_value(&ev);schedule.set(value.clone());media::storage_set("vsmedia:radio:schedule",&value);
                        }></textarea>
                </label>
                <small>"Las notas se guardan localmente. La transmisión P2P utiliza señalización Axum en localhost; no hay servidor de distribución masiva."</small>
            </aside>
        </div>
    </div>}
}

#[component]
fn StreamsStudio(page:RwSignal<Page>,status:RwSignal<String>)->impl IntoView {
    let integration=RwSignal::new("Estado no consultado".to_owned());
    studio_api::probe_streams(integration);
    view! {
        <div class="studio-screen">
            <div class="studio-toolbar">
                <EditorMenu page status />
                <strong>"VSStreams"</strong>
                <span class="toolbar-separator"></span>
                <span class="toolbar-caption">"INTEGRACIÓN DE VOZ POR PROXIMIDAD"</span>
                <span class="connection-badge">"Plasmo Voice · no sustituimos su motor"</span>
            </div>
            <main class="vs-streams-workspace">
                <div class="stage-topline">
                    <span>"VSSTREAMS / VIRGOLAND 26.3"</span>
                    <span>"Estado de integración"</span>
                </div>
                <section class="vs-streams-intro">
                    <h2>"Voz de Minecraft, administrada desde VSMedia."</h2>
                    <p>"Plasmo Voice proporciona el audio espacial, el micrófono, el alcance por distancia y el transporte entre jugadores. VSStreams no abre otra sala de voz WebRTC ni solicita acceso a tu micrófono."</p>
                </section>
                <section class="vs-streams-status" aria-label="Estado de Plasmo Voice">
                    <div class="vs-streams-status-line">
                        <span class="privacy-dot offline"></span>
                        <strong>{move ||integration.get()}</strong>
                        <button type="button" class="action-secondary"
                            on:click=move |_|studio_api::probe_streams(integration)>"↻ Comprobar conexión"</button>
                    </div>
                    <p>"Esta comprobación consulta el backend local. Todavía falta conectar la telemetría de Plasmo Voice con VSEngine; no se muestran datos inventados del juego."</p>
                </section>
                <div class="vs-streams-checklist">
                    <section>
                        <h3>"1 · Servidor"</h3>
                        <p>"Instalar Plasmo Voice para Folia/Paper compatible con Minecraft 26.3 en VSEngine."</p>
                        <a href="https://modrinth.com/plugin/plasmo-voice" target="_blank" rel="noopener noreferrer">"Ver Plasmo Voice para servidor ↗"</a>
                    </section>
                    <section>
                        <h3>"2 · Cliente"</h3>
                        <p>"Instalar el mod Fabric correspondiente en VSClient para la voz de proximidad."</p>
                        <a href="https://modrinth.com/plugin/plasmo-voice/versions?l=fabric" target="_blank" rel="noopener noreferrer">"Versiones Fabric ↗"</a>
                    </section>
                    <section>
                        <h3>"3 · Integración"</h3>
                        <p>"La futura conexión con VSEngine expondrá presencia, permisos y moderación sin introducir otro protocolo de audio."</p>
                        <a href="https://plasmovoice.com/docs/api/" target="_blank" rel="noopener noreferrer">"Documentación de la API ↗"</a>
                    </section>
                </div>
                <section class="vs-streams-info">
                    <strong>"Funciones nativas de Plasmo Voice"</strong>
                    <p>"Proximidad 3D, Push-to-Talk, distancia configurable y silenciamiento desde el servidor. Por ahora los controles se utilizan en Minecraft y en la consola del servidor, no desde esta página."</p>
                    <strong>"Administración avanzada: etapa posterior"</strong>
                    <p>"Panel de jugadores, muteos temporales, auditoría, reglas por mundos y regiones. No se habilitan botones sin un adaptador VSEngine autenticado."</p>
                </section>
            </main>
        </div>
    }
}

#[component]
fn PackStudio(page:RwSignal<Page>,status:RwSignal<String>)->impl IntoView {
    view! {
        <div class="studio-screen">
            <div class="studio-toolbar">
                <EditorMenu page status />
                <strong>"VSPack"</strong>
                <span class="toolbar-separator"></span>
                <span class="toolbar-caption">"GESTOR DE RESOURCE PACKS"</span>
                <span class="connection-badge">"Rust Pack Engine · sesión local"</span>
                <button type="button" class="toolbar-button"
                    on:click=move |_|click_control("vspack-upload")>"＋ Importar ZIP"</button>
                <button type="button" class="toolbar-button accent"
                    on:click=move |_|click_control("vspack-export")>"↓ Compilar y exportar"</button>
            </div>
            <main data-vspack class="vspack-root" aria-label="Editor de Resource Packs"></main>
        </div>
    }
}
fn click_control(id: &str) {
    if let Some(elem)=web_sys::window().and_then(|w|w.document()).and_then(|d|d.get_element_by_id(id)) {
        if let Ok(html)=wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(elem){html.click();}
    }
}
fn focus_control(id: &str) {
    if let Some(elem)=web_sys::window().and_then(|w|w.document()).and_then(|d|d.get_element_by_id(id)) {
        if let Ok(html)=wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(elem){let _=html.focus();}
    }
}
fn scroll_to(id: &str) {
    if let Some(elem)=web_sys::window().and_then(|w|w.document()).and_then(|d|d.get_element_by_id(id)) {
        elem.scroll_into_view();
    }
}
#[derive(Clone,Copy)]
enum MenuCommand {Import,New,Export,Effects,Record,Playback,View,Help}
fn handle_menu(command: MenuCommand,page: RwSignal<Page>,status: RwSignal<String>) {
    match command {
        MenuCommand::Import => {
            if page.get_untracked()==Page::Pack{click_control("vspack-upload");}
            else {click_control("library-import");}
        },
        MenuCommand::New => match page.get_untracked() {
            Page::Audio=>click_control("audio-new-version"),
            Page::Mix=>click_control("mix-add-track"),
            Page::Radio=>focus_control("radio-new-playlist"),
            Page::Streams=>{},
            Page::Pack=>click_control("vspack-upload"),
            Page::Home=>{},
        },
        MenuCommand::Export => match page.get_untracked() {
            Page::Audio=>click_control("audio-export"),
            Page::Pack=>click_control("vspack-export"),
            _=>{page.set(Page::Audio);status.set("Seleccioná un clip y exportalo desde VSAudio.".to_owned());}
        },
        MenuCommand::Effects => match page.get_untracked(){
            Page::Audio=>{
                if web_sys::window().and_then(|w|w.document()).and_then(|d|d.get_element_by_id("audio-effect-target")).is_some(){scroll_to("audio-effect-target");}
                else{status.set("Importá un archivo y creá una versión para ajustar efectos.".to_owned());}
            },
            _=>{page.set(Page::Audio);status.set("Los efectos de clip están en VSAudio.".to_owned());}
        },
        MenuCommand::Record => match page.get_untracked(){
            Page::Audio=>{page.set(Page::Mix);status.set("Elegí una pista para grabar el micrófono.".to_owned());},
            Page::Mix=>click_control("mix-record"),
            Page::Radio=>click_control("radio-record"),
            Page::Streams=>{},
            Page::Pack=>{},
            Page::Home=>{},
        },
        MenuCommand::Playback => match page.get_untracked(){
            Page::Audio=>{
                if web_sys::window().and_then(|w|w.document()).and_then(|d|d.get_element_by_id("audio-play")).is_some(){click_control("audio-play");}
                else{status.set("Seleccioná un clip y una versión para reproducir.".to_owned());}
            },
            Page::Mix=>click_control("mix-play"),
            Page::Radio=>click_control("radio-play"),
            Page::Streams=>{},
            Page::Pack=>{},
            Page::Home=>{},
        },
        MenuCommand::View=>scroll_to("media-library"),
        MenuCommand::Help=>status.set("VSAudio edita · VSMix mezcla · VSRadio emite · VSStreams integra Plasmo Voice · VSPack combina Resource Packs y exporta ZIP.".to_owned()),
    }
}

#[component]
fn EditorMenu(page: RwSignal<Page>,status: RwSignal<String>) -> impl IntoView {
    view! {<nav class="editor-menus" aria-label="Menú del editor">
        <details class="editor-menu"><summary>"Archivo"</summary><div class="editor-menu-popover">
            <button on:click=move |_|handle_menu(MenuCommand::Import,page,status)>"Importar archivos…"</button>
            <button on:click=move |_|handle_menu(MenuCommand::Export,page,status)>"Exportar…"</button>
        </div></details>
        <details class="editor-menu"><summary>"Editar"</summary><div class="editor-menu-popover">
            <button on:click=move |_|handle_menu(MenuCommand::New,page,status)>"Nuevo elemento"</button>
        </div></details>
        <details class="editor-menu"><summary>"Efectos"</summary><div class="editor-menu-popover">
            <button on:click=move |_|handle_menu(MenuCommand::Effects,page,status)>"Editar efectos del clip"</button>
        </div></details>
        <details class="editor-menu"><summary>"Grabar"</summary><div class="editor-menu-popover">
            <button on:click=move |_|handle_menu(MenuCommand::Record,page,status)>"Grabación de micrófono"</button>
        </div></details>
        <details class="editor-menu"><summary>"Reproducción"</summary><div class="editor-menu-popover">
            <button on:click=move |_|handle_menu(MenuCommand::Playback,page,status)>"Reproducir"</button>
        </div></details>
        <details class="editor-menu"><summary>"Ver"</summary><div class="editor-menu-popover">
            <button on:click=move |_|handle_menu(MenuCommand::View,page,status)>"Ir a la biblioteca"</button>
        </div></details>
        <details class="editor-menu"><summary>"Ayuda"</summary><div class="editor-menu-popover">
            <button on:click=move |_|handle_menu(MenuCommand::Help,page,status)>"Acerca de las herramientas"</button>
        </div></details>
    </nav>}
}
