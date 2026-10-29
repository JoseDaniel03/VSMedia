use leptos::prelude::*;
use crate::media::{self,AudioAsset,Folder,Version,Track,Playlist};
use crate::{export,recording,studio_api};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {Home, Audio, Mix, Radio}
impl Page {
    fn title(self)->&'static str {match self {Self::Home=>"Home",Self::Audio=>"VSAudio",Self::Mix=>"VSMix",Self::Radio=>"VSRadio"}}
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
    let playlists = RwSignal::new(Vec::<Playlist>::new());
    let recording = RwSignal::new(false);
    let core_connected=RwSignal::new(false);
    studio_api::probe_engine(core_connected);
    view! {
        <div class="app-shell">
            <header class="app-topbar">
                <nav class="app-tabs" aria-label="Espacios de trabajo">
                    { [Page::Home,Page::Audio,Page::Mix,Page::Radio].into_iter().map(|target| view! {
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
                    <MixStudio assets folders selected status tracks recording page />
                </Show>
                <Show when=move ||page.get()==Page::Radio>
                    <RadioStudio assets folders selected status playlists recording page />
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
                <span class="tile-symbol">"◉"</span><strong>"VSRadio"</strong><small>"Playlists · programación · grabación"</small><span class="tile-open">"Abrir consola ↗"</span>
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

#[component]
fn MixStudio(assets: RwSignal<Vec<AudioAsset>>,folders: RwSignal<Vec<Folder>>,selected: RwSignal<String>,
             status: RwSignal<String>,tracks: RwSignal<Vec<Track>>,recording: RwSignal<bool>,page: RwSignal<Page>) -> impl IntoView {
    let selected_track=RwSignal::new(String::new());
    let saved_projects=RwSignal::new(Vec::<studio_api::SavedAudioProject>::new());
    let show_saved=RwSignal::new(false);
    let script=RwSignal::new(media::storage_get("vsmedia:dubbing:script"));
    let scroll_speed=RwSignal::new(50.0f64);
    let script_running=RwSignal::new(false);
    view! {<div class="studio-screen">
        <div class="studio-toolbar">
            <EditorMenu page status />
            <strong>"VSMix"</strong><span class="toolbar-separator"></span><span class="toolbar-caption">"ESTUDIO MULTIPISTA"</span>
            <button id="mix-add-track" type="button" class="toolbar-button" on:click=move |_|{
                let id=media::uid("track");
                let count=tracks.get_untracked().len()+1;
                tracks.update(|list|list.push(Track{id:id.clone(),name:format!("Pista {}",count),asset_id:String::new(),volume:1.,muted:false,solo:false,fade_in:0.,fade_out:0.}));
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
            <button id="mix-play" type="button" class="toolbar-button" on:click=move |_|{
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
                <Show when=move ||tracks.get().is_empty()>
                    <div class="empty-canvas large-empty"><span>"≋"</span><strong>"Tu sesión no tiene pistas"</strong>
                    <p>"Añadí pistas, vinculá archivos de la biblioteca o grabá directamente en un canal."</p></div>
                </Show>
                <div class="track-list">
                    <For each=move ||tracks.get() key=|t|t.id.clone() children=move |track| {
                        let tid=track.id.clone();
                        let id_name=tid.clone();let id_select=tid.clone();let id_file=tid.clone();
                        let id_mute=tid.clone();let id_mute_click=tid.clone();let id_solo=tid.clone();let id_solo_click=tid.clone();let id_vol=tid.clone();let id_vol_input=tid.clone();let id_fade_in=tid.clone();let id_fade_in_change=tid.clone();let id_fade_out=tid.clone();let id_fade_out_change=tid.clone();let id_remove=tid.clone();
                        let id_active=tid.clone();let id_click=tid.clone();let id_player=tid.clone();
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

fn play_radio(assets: RwSignal<Vec<AudioAsset>>,playlists: RwSignal<Vec<Playlist>>,
              active_playlist: RwSignal<String>,index: RwSignal<usize>,status: RwSignal<String>) {
    let pid=active_playlist.get_untracked();
    let Some(playlist)=playlists.get_untracked().into_iter().find(|p|p.id==pid) else {
        status.set("Creá o seleccioná una playlist.".to_owned());return;
    };
    let Some(file_id)=playlist.items.get(index.get_untracked()) else {
        status.set("La playlist terminó o está vacía.".to_owned());return;
    };
    let Some(asset)=media::asset_by_id(assets,file_id) else {
        status.set("El archivo no está disponible en la sesión.".to_owned());return;
    };
    if let Some(player)=media::player("radio-player") {
        player.set_src(&asset.url);
        let _=player.play();
        status.set(format!("Reproducción local: {}",asset.name));
    }
}

#[component]
fn RadioStudio(assets: RwSignal<Vec<AudioAsset>>,folders: RwSignal<Vec<Folder>>,selected: RwSignal<String>,
               status: RwSignal<String>,playlists: RwSignal<Vec<Playlist>>,recording: RwSignal<bool>,page: RwSignal<Page>) -> impl IntoView {
    let new_playlist=RwSignal::new(String::new());
    let active_playlist=RwSignal::new(String::new());
    let play_index=RwSignal::new(0usize);
    let schedule=RwSignal::new(media::storage_get("vsmedia:radio:schedule"));
    let active_items=move ||playlists.get().into_iter().find(|p|p.id==active_playlist.get()).map(|p|p.items).unwrap_or_default();
    let indexed_items=move ||active_items().into_iter().enumerate().collect::<Vec<(usize,String)>>();
    view!{<div class="studio-screen">
        <div class="studio-toolbar">
            <EditorMenu page status />
            <strong>"VSRadio"</strong><span class="toolbar-separator"></span><span class="toolbar-caption">"CONSOLA LOCAL"</span>
            <span class="connection-badge">"● Sin emisión conectada"</span>
            <button id="radio-play" type="button" class="toolbar-button" on:click=move |_|{
                play_index.set(0);
                play_radio(assets,playlists,active_playlist,play_index,status);
            }>"▶ Reproducir playlist"</button>
            <button type="button" class="toolbar-button" on:click=move |_|{
                if let Some(player)=media::player("radio-player"){player.pause().ok();}
            }>"Ⅱ Pausar"</button>
            <Show when=move ||!recording.get()>
                <button id="radio-record" type="button" class="toolbar-button record-button" on:click=move |_|{
                    recording::start(assets,status,recording,move |file_id|{
                        let pid=active_playlist.get_untracked();
                        if !pid.is_empty(){
                            playlists.update(|list|if let Some(p)=list.iter_mut().find(|p|p.id==pid){p.items.push(file_id.clone());});
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
                        let name=new_playlist.get_untracked().trim().to_owned();if name.is_empty(){return;}
                        let id=media::uid("playlist");
                        playlists.update(|list|list.push(Playlist{id:id.clone(),name,items:Vec::new()}));
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
                            playlists.update(|all|if let Some(p)=all.iter_mut().find(|p|p.id==pid){p.items.push(file.clone());});
                        }>"＋ Agregar archivo seleccionado"</button>
                        <span>{move ||format!("{} archivos",active_items().len())}</span>
                    </div>
                    <div class="playlist-rows">
                        <For each=indexed_items
                            key=|(i,id)|format!("{}-{}",i,id) children=move |(i,file_id)|{
                            let file_for_play=file_id.clone();
                            let file_name=media::asset_by_id(assets,&file_id).map(|a|a.name).unwrap_or("Archivo no disponible".to_owned());
                            view!{<div class="playlist-row">
                                <span class="playlist-order">{format!("{:02}",i+1)}</span>
                                <button type="button" class="playlist-song" on:click=move |_|{
                                    if let Some(a)=media::asset_by_id(assets,&file_for_play){
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
                                    playlists.update(|list|if let Some(p)=list.iter_mut().find(|p|p.id==pid){if i<p.items.len(){p.items.remove(i);}});
                                }>"×"</button>
                            </div>}
                        }/>
                    </div>
                </Show>
                <div class="radio-player-block"><div class="pane-title">"REPRODUCTOR LOCAL"</div>
                    <audio id="radio-player" controls preload="metadata"
                        on:ended=move |_|{
                            let next=play_index.get_untracked()+1;
                            play_index.set(next);
                            play_radio(assets,playlists,active_playlist,play_index,status);
                        }>"Audio HTML5 no disponible"</audio>
                    <small>"La salida se escucha en este navegador; no equivale a transmitir por Internet."</small>
                </div>
            </section>
            <aside class="schedule-panel"><div class="stage-topline"><span>"PROGRAMACIÓN"</span></div>
                <label>"Plan de emisiones, horarios y notas"
                    <textarea placeholder="Ejemplo: 18:00 — Inicio del programa…"
                        prop:value=move ||schedule.get()
                        on:input=move |ev|{
                            let value=event_target_value(&ev);schedule.set(value.clone());media::storage_set("vsmedia:radio:schedule",&value);
                        }></textarea>
                </label>
                <small>"Las notas se guardan localmente. La conexión de streaming se añadirá mediante el backend del plugin."</small>
            </aside>
        </div>
    </div>}
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
        MenuCommand::Import => click_control("library-import"),
        MenuCommand::New => match page.get_untracked() {
            Page::Audio=>click_control("audio-new-version"),
            Page::Mix=>click_control("mix-add-track"),
            Page::Radio=>focus_control("radio-new-playlist"),
            Page::Home=>{},
        },
        MenuCommand::Export => match page.get_untracked() {
            Page::Audio=>click_control("audio-export"),
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
            Page::Home=>{},
        },
        MenuCommand::Playback => match page.get_untracked(){
            Page::Audio=>{
                if web_sys::window().and_then(|w|w.document()).and_then(|d|d.get_element_by_id("audio-play")).is_some(){click_control("audio-play");}
                else{status.set("Seleccioná un clip y una versión para reproducir.".to_owned());}
            },
            Page::Mix=>click_control("mix-play"),
            Page::Radio=>click_control("radio-play"),
            Page::Home=>{},
        },
        MenuCommand::View=>scroll_to("media-library"),
        MenuCommand::Help=>status.set("Archivos locales · VSAudio exporta WAV · VSMix graba pistas · VSRadio reproduce listas. No hay emisión remota conectada.".to_owned()),
    }
}

#[component]
fn EditorMenu(page: RwSignal<Page>,status: RwSignal<String>) -> impl IntoView {
    view! {<nav class="editor-menus" aria-label="Menú del editor">
        <details class="editor-menu"><summary>"Archivo"</summary><div class="editor-menu-popover">
            <button on:click=move |_|handle_menu(MenuCommand::Import,page,status)>"Importar audio…"</button>
            <button on:click=move |_|handle_menu(MenuCommand::Export,page,status)>"Exportar clip WAV…"</button>
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
