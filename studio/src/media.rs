use leptos::prelude::*;
use serde::{Serialize, Deserialize};
use web_sys::{Blob, Url};
use std::{cell::{RefCell,Cell},collections::HashMap};
thread_local! {static BINARIES: RefCell<HashMap<String,Blob>> = RefCell::new(HashMap::new());}
pub fn get_blob(id: &str) -> Option<Blob> { BINARIES.with(|m|m.borrow().get(id).cloned()) }

#[derive(Clone)]
pub struct AudioAsset {
    pub id: String,
    pub name: String,
    pub url: String,
    pub folder: String,
    pub genre: String,
    pub media_type: String,
    pub bytes: f64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Folder { pub id: String, pub parent: String, pub name: String }
#[derive(Clone)]
pub struct FolderRow { pub id: String, pub name: String, pub depth: usize }
pub fn folder_rows(folders: Vec<Folder>) -> Vec<FolderRow> {
    fn visit(parent:&str,depth:usize,all:&[Folder],seen:&mut std::collections::HashSet<String>,out:&mut Vec<FolderRow>) {
        for f in all.iter().filter(|folder|folder.parent==parent) {
            if seen.insert(f.id.clone()) {
                out.push(FolderRow{id:f.id.clone(),name:f.name.clone(),depth});
                visit(&f.id,depth+1,all,seen,out);
            }
        }
    }
    let mut out=Vec::new();
    let mut seen=std::collections::HashSet::new();
    visit("",0,&folders,&mut seen,&mut out);
    for f in &folders {if !seen.contains(&f.id){visit(&f.parent,0,&folders,&mut seen,&mut out);}}
    out
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum MixFadeCurve {
    #[default] Linear,
    EqualPower,
}
impl MixFadeCurve {
    pub fn as_str(self)->&'static str { match self {Self::Linear=>"linear",Self::EqualPower=>"equal_power"} }
    pub fn from_str(value:&str)->Option<Self> {
        match value {"linear"=>Some(Self::Linear),"equal_power"=>Some(Self::EqualPower),_=>None}
    }
    pub fn amplitude(self, progress:f64)->f64 {
        let fraction=progress.clamp(0.0,1.0);
        match self {Self::Linear=>fraction,Self::EqualPower=>(fraction*std::f64::consts::FRAC_PI_2).sin()}
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct MixRegion {
    pub id: String,
    pub asset_id: String,
    /// Position of this region on the output timeline (seconds).
    pub start: f64,
    /// Duration to play from the source file (seconds).
    pub duration: f64,
    pub source_in: f64,
    pub volume: f64,
    pub pan: f64,
    pub fade_in: f64,
    pub fade_out: f64,
    /// Missing in earlier local sessions and manifests: use legacy linear fade.
    #[serde(default)]
    pub fade_curve: MixFadeCurve,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: String, pub name: String, pub asset_id: String,
    pub volume: f64, pub muted: bool, pub solo: bool,
    pub fade_in: f64, pub fade_out: f64,
    /// Empty keeps the old one-file-per-track VSMix session compatible.
    #[serde(default)]
    pub regions: Vec<MixRegion>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Version {
    pub id: String, pub asset_id: String, pub label: String,
    pub start: f64, pub end: f64, pub gain: f64,
    pub fade_in: f64, pub fade_out: f64, pub reversed: bool,
    /// Extra per-clip controls stored in VSAudio and rendered by VSCore.
    #[serde(default)]
    pub pan: f64,
    #[serde(default)]
    pub low_pass_hz: Option<f64>,
    #[serde(default)]
    pub high_pass_hz: Option<f64>,
    /// Optional gain at the end of the clip, interpolated from `gain`.
    #[serde(default)]
    pub end_gain: Option<f64>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Playlist { pub id: String, pub name: String, pub items: Vec<String> }

pub fn uid(prefix: &str) -> String {
    thread_local! {static NEXT: Cell<u64> = const { Cell::new(0) };}
    let counter=NEXT.with(|n|{let current=n.get();n.set(current+1);current});
    format!("{}-{}-{}", prefix, js_sys::Date::now() as u64, counter)
}

pub fn add_blob(blob: Blob, name: String, assets: RwSignal<Vec<AudioAsset>>, status: RwSignal<String>) -> Option<String> {
    match Url::create_object_url_with_blob(&blob) {
        Ok(url) => {
            let id = uid("asset");
            let size = blob.size();
            BINARIES.with(|m|m.borrow_mut().insert(id.clone(),blob));
            assets.update(|list| list.push(AudioAsset {
                id: id.clone(), name, url, folder: String::new(),
                genre: String::new(), media_type: "Audio".to_owned(), bytes: size,
            }));
            status.set("Archivo incorporado localmente; no se subió al servidor.".to_owned());
            Some(id)
        }
        Err(_) => {status.set("No fue posible abrir el archivo en este navegador.".to_owned()); None}
    }
}

pub fn asset_by_id(assets: RwSignal<Vec<AudioAsset>>, id: &str) -> Option<AudioAsset> {
    assets.get_untracked().into_iter().find(|a|a.id==id)
}

pub fn remove_asset(assets: RwSignal<Vec<AudioAsset>>, id: &str) {
    let url = assets.get_untracked().into_iter().find(|a|a.id==id).map(|a|a.url);
    assets.update(|a|a.retain(|item| item.id!=id));
    BINARIES.with(|m|m.borrow_mut().remove(id));
    if let Some(url)=url { let _=Url::revoke_object_url(&url); }
}

pub fn storage_get(key: &str) -> String {
    web_sys::window().and_then(|w|w.local_storage().ok().flatten())
        .and_then(|s|s.get_item(key).ok().flatten()).unwrap_or_default()
}
pub fn storage_set(key: &str, value: &str) {
    if let Some(s)=web_sys::window().and_then(|w|w.local_storage().ok().flatten()) {let _=s.set_item(key,value);}
}
pub fn load_folders() -> Vec<Folder> { serde_json::from_str(&storage_get("vsmedia:folders:v1")).unwrap_or_default() }
pub fn save_folders(list: &[Folder]) { if let Ok(json)=serde_json::to_string(list) {storage_set("vsmedia:folders:v1",&json);} }

pub fn download_blob(blob: &Blob, file_name: &str) -> Result<(), String> {
    let url = Url::create_object_url_with_blob(blob).map_err(|_| "No se pudo crear una URL de descarga")?;
    let document = web_sys::window().and_then(|w|w.document()).ok_or("Sin documento activo")?;
    let element = document.create_element("a").map_err(|_| "No se pudo preparar la descarga")?;
    let _=element.set_attribute("href",&url);
    let _=element.set_attribute("download",file_name);
    if let Some(body)=document.body() {
        let _=body.append_child(&element);
        if let Ok(anchor) = element.clone().dyn_into::<web_sys::HtmlElement>() {anchor.click();}
        let _=body.remove_child(&element);
    }
    // La URL se libera posteriormente al cerrar la pestaña; revocarla aquí puede cancelar la descarga.
    Ok(())
}

use wasm_bindgen::JsCast;

pub fn player(id: &str) -> Option<web_sys::HtmlAudioElement> {
    web_sys::window()?.document()?.get_element_by_id(id)?
        .dyn_into::<web_sys::HtmlAudioElement>().ok()
}
