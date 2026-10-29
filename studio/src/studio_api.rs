//! Browser-to-Rust Studio bridge. Explicit user action is required for upload.
//! No game or radio operation is emulated: only CAS assets and VSAudio projects.
use crate::media::{self,AudioAsset,Version};
use js_sys::{ArrayBuffer,Uint8Array};
use leptos::prelude::*;
use serde_json::{json,Value};
use wasm_bindgen::{JsCast,JsValue};
use wasm_bindgen_futures::{spawn_local,JsFuture};
use web_sys::{Request,RequestInit,RequestMode,Response};

const BASE: &str = "http://127.0.0.1:3061/api/v1";

async fn request(path:&str,method:&str,body:JsValue,content_type:&str)
    ->Result<Value,String>
{
    let opts=RequestInit::new();
    opts.set_method(method);
    opts.set_mode(RequestMode::Cors);
    if method=="POST" {opts.set_body(&body);}
    let req=Request::new_with_str_and_init(&format!("{BASE}{path}"),&opts)
        .map_err(|_|"No se pudo crear la solicitud HTTP")?;
    if method=="POST" {
        req.headers().set("Content-Type",content_type)
            .map_err(|_|"No se pudo asignar Content-Type")?;
    }
    let window=web_sys::window().ok_or("No hay ventana")?;
    let res=JsFuture::from(window.fetch_with_request(&req)).await
        .map_err(|_|"No se pudo conectar con VSMedia en 127.0.0.1:3061")?;
    let res:Response=res.dyn_into().map_err(|_|"Respuesta HTTP inválida")?;
    let text=JsFuture::from(res.text().map_err(|_|"No se pudo leer la respuesta")?).await
        .map_err(|_|"La respuesta del motor no pudo leerse")?;
    let data:Value=serde_json::from_str(&text.as_string().unwrap_or_default())
        .map_err(|_|"El motor devolvió JSON inválido")?;
    if !res.ok() {
        return Err(data.get("error").and_then(Value::as_str).unwrap_or("Error del motor").to_owned());
    }
    Ok(data)
}
fn codec(name:&str)->Option<&'static str> {
    match name.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "wav" =>Some("wav"),"mp3"=>Some("mp3"),"flac"=>Some("flac"),
        "ogg"=>Some("ogg_vorbis"),"opus"=>Some("opus"),
        "webm"=>Some("webm"),_=>None
    }
}
fn millis(seconds:f64)->Result<u64,String> {
    if !seconds.is_finite() || seconds<0. || seconds>3_600. {
        return Err("Tiempo inválido: máximo una hora por clip".into());
    }
    Ok((seconds*1000.).round() as u64)
}
async fn save(asset:AudioAsset,version:Version,duration:f64)->Result<String,String>{
    let content=media::get_blob(&asset.id)
        .ok_or("El audio original ya no está disponible en memoria")?;
    if content.size()<=0. || content.size()>16.*1024.*1024. {
        return Err("El audio debe ocupar entre 1 B y 16 MiB".into());
    }
    let codec=codec(&asset.name)
        .ok_or("Para guardar proyectos, utilizá WAV, MP3, FLAC, OGG, Opus o WebM")?;
    let total=if version.end>0. {version.end} else {duration};
    let start=millis(version.start)?;
    let end=millis(total)?;
    if end<=start {return Err("Indicá un tiempo final válido antes de guardar".into());}
    let length=end-start;
    let fade_in=millis(version.fade_in)?;
    let fade_out=millis(version.fade_out)?;
    if fade_in.saturating_add(fade_out)>length {
        return Err("Los fades superan la duración del clip".into());
    }
    if !version.gain.is_finite() || !(0.0..=2.0).contains(&version.gain) {
        return Err("La ganancia debe estar entre 0% y 200%".into());
    }
    let audio=JsFuture::from(content.array_buffer()).await
        .map_err(|_|"No se pudo leer el audio local")?;
    let array:ArrayBuffer=audio.dyn_into().map_err(|_|"Archivo no válido")?;
    let data=Uint8Array::new(&array);
    let info=request(&format!("/assets/{}",asset.id),"POST",data.into(),
        "application/octet-stream").await?;
    let sha=info.get("sha256").and_then(Value::as_str)
        .ok_or("El motor no devolvió SHA-256")?;
    // The native document references the original immutable asset hash.
    let gain_db=if version.gain==0. {-120.0} else {
        20.*version.gain.log10()
    };
    let name=if version.label.trim().is_empty() {
        "Proyecto de audio".to_owned()
    } else {version.label.clone()};
    let project=json!({
        "magic":"VSMEDIA","version":1,
        "payload":{"kind":"audio","data":{
            "timeline":{"name":name,"tracks":[{
                "name":"Audio principal","muted":false,
                "clips":[{"asset":{"namespace":"studio","name":format!("audio/{}",asset.id)},
                    "start_ms":0,"duration_ms":length,"gain_db":0.0}]
            }]},
            "sources":[{"key":{"namespace":"studio","name":format!("audio/{}",asset.id)},
                "sha256":sha,"codec":codec}],
            "clip_edits":[{
                "track_index":0,"clip_index":0,"source_in_ms":start,
                "pan":0.0,"fade_in_ms":fade_in,"fade_out_ms":fade_out,
                "reversed":version.reversed,"muted":false,
                "gain_points":[{"at_ms":0,"gain_db":gain_db}],"filters":[]
            }]
        }}
    });
    let saved=request(&format!("/audio/projects/{}",asset.id),"POST",
        JsValue::from_str(&project.to_string()),"application/json").await?;
    let revision=saved.get("revision").and_then(Value::as_u64).unwrap_or(0);
    Ok(format!("Proyecto .vsaudio guardado en VSCore (revisión {revision})."))
}
/// Explicit opt-in. Local files do not leave the browser otherwise.
pub fn save_audio_project(
    asset:AudioAsset,version:Version,duration:f64,status:RwSignal<String>
) {
    status.set("Conectando con VSCore para guardar audio y proyecto…".into());
    spawn_local(async move {
        let message=match save(asset,version,duration).await {
            Ok(msg)=>msg,Err(error)=>format!("No se guardó el proyecto: {error}")
        };
        status.set(message);
    });
}

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct SavedAudioProject {
    pub name:String,
    pub revision:u64,
    pub sha256:String,
}
pub fn refresh_projects(projects:RwSignal<Vec<SavedAudioProject>>,status:RwSignal<String>) {
    status.set("Consultando los proyectos de VSCore…".into());
    spawn_local(async move {
        match request("/audio/projects","GET",JsValue::NULL,"").await {
            Ok(data)=>{
                let Some(rows)=data.as_array() else {
                    status.set("VSCore devolvió una lista inválida.".into());
                    return;
                };
                let entries=rows.iter().filter_map(|record|{
                    let name=record.pointer("/key/name")?.as_str()?.strip_prefix("projects/")?;
                    let revision=record.get("revision")?.as_u64()?;
                    let hash=record.get("sha256")?.as_str()?;
                    Some(SavedAudioProject {
                        name:name.to_owned(),revision,sha256:hash.to_owned()
                    })
                }).collect::<Vec<_>>();
                status.set(format!("{} proyectos de audio disponibles en VSCore.",entries.len()));
                projects.set(entries);
            }
            Err(error)=>status.set(format!("No se pudo consultar VSCore: {error}")),
        }
    });
}
async fn fetch_source_blob(name:&str)->Result<web_sys::Blob,String> {
    let req=Request::new_with_str_and_init(
        &format!("{BASE}/assets/{name}"),&RequestInit::new()
    ).map_err(|_|"No se pudo crear la solicitud de audio")?;
    let window=web_sys::window().ok_or("No hay ventana")?;
    let value=JsFuture::from(window.fetch_with_request(&req)).await
        .map_err(|_|"No se pudo descargar el archivo desde VSCore")?;
    let response:Response=value.dyn_into().map_err(|_|"Respuesta de audio inválida")?;
    if !response.ok(){return Err("VSCore no encontró el audio del proyecto".into());}
    let object=JsFuture::from(response.blob()
        .map_err(|_|"No se pudo preparar el archivo de audio")?).await
        .map_err(|_|"Falló la descarga de audio")?;
    object.dyn_into().map_err(|_|"El servidor no devolvió un archivo válido".into())
}
async fn load_saved(name:&str,assets:RwSignal<Vec<AudioAsset>>,
    selected:RwSignal<String>,versions:RwSignal<Vec<Version>>,
    current_version:RwSignal<String>,status:RwSignal<String>
)->Result<String,String> {
    if name.is_empty() || !name.bytes().all(|b|
        b.is_ascii_alphanumeric() || matches!(b,b'-'|b'_'|b'.')) {
        return Err("Identificador de proyecto inválido".into());
    }
    let doc=request(&format!("/audio/projects/{name}"),"GET",JsValue::NULL,"").await?;
    if doc.pointer("/payload/kind").and_then(Value::as_str)!=Some("audio") {
        return Err("El proyecto guardado no es de tipo VSAudio".into());
    }
    let timeline=doc.pointer("/payload/data/timeline")
        .ok_or("Proyecto sin línea de tiempo")?;
    let tracks=timeline.get("tracks").and_then(Value::as_array)
        .ok_or("Estructura de pistas inválida")?;
    if tracks.len()!=1 {
        return Err("Este proyecto tiene varias pistas: se abrirá desde VSMix cuando esté conectado.".into());
    }
    let clips=tracks[0].get("clips").and_then(Value::as_array)
        .ok_or("Proyecto sin clips")?;
    if clips.len()!=1 {return Err("La edición de varios clips corresponde a VSMix.".into());}
    let clip=&clips[0];
    let asset_key=clip.get("asset").ok_or("Clip sin audio de origen")?;
    if asset_key.get("namespace").and_then(Value::as_str)!=Some("studio") {
        return Err("El proyecto referencia un namespace ajeno a Studio".into());
    }
    let asset_name=asset_key.get("name").and_then(Value::as_str)
        .and_then(|s|s.strip_prefix("audio/"))
        .ok_or("El clip referencia un audio inválido")?;
    if !asset_name.bytes().all(|b|
        b.is_ascii_alphanumeric() || matches!(b,b'-'|b'_'|b'.')) {
        return Err("El identificador del audio es inválido".into());
    }
    let expected_hash=doc.pointer("/payload/data/sources")
        .and_then(Value::as_array)
        .and_then(|sources|sources.iter().find(|source|
            source.pointer("/key/name").and_then(Value::as_str)
                .and_then(|full|full.strip_prefix("audio/"))==Some(asset_name)))
        .and_then(|source|source.get("sha256")).and_then(Value::as_str)
        .ok_or("El proyecto no contiene la referencia SHA-256 del audio")?;
    let metadata=request(&format!("/assets/{asset_name}/metadata"),"GET",
        JsValue::NULL,"").await?;
    if metadata.get("sha256").and_then(Value::as_str)!=Some(expected_hash) {
        return Err("El archivo fuente cambió desde que se guardó el proyecto.".into());
    }
    let blob=fetch_source_blob(asset_name).await?;
    if blob.size()<=0. || blob.size()>16.*1024.*1024. {
        return Err("El audio supera el límite de importación local (16 MiB).".into());
    }
    let name_hint=timeline.get("name").and_then(Value::as_str)
        .unwrap_or("Proyecto recuperado").to_owned();
    let source_in=doc.pointer("/payload/data/clip_edits/0/source_in_ms")
        .and_then(Value::as_u64).unwrap_or(0) as f64/1000.;
    let length=clip.get("duration_ms").and_then(Value::as_u64)
        .ok_or("Duración del clip inválida")? as f64/1000.;
    let edit=doc.pointer("/payload/data/clip_edits/0");
    let gain_db=edit.and_then(|v|v.pointer("/gain_points/0/gain_db"))
        .and_then(Value::as_f64).unwrap_or(0.);
    let fade_in=edit.and_then(|v|v.get("fade_in_ms"))
        .and_then(Value::as_u64).unwrap_or(0) as f64/1000.;
    let fade_out=edit.and_then(|v|v.get("fade_out_ms"))
        .and_then(Value::as_u64).unwrap_or(0) as f64/1000.;
    let reversed=edit.and_then(|v|v.get("reversed"))
        .and_then(Value::as_bool).unwrap_or(false);
    let source_codec=doc.pointer("/payload/data/sources/0/codec")
        .and_then(Value::as_str).unwrap_or("wav");
    let extension=match source_codec {
        "wav"=>"wav","flac"=>"flac","mp3"=>"mp3",
        "ogg_vorbis"=>"ogg","opus"=>"opus","webm"=>"webm",_=>"wav"
    };
    let id=media::add_blob(blob,format!("{name_hint}.{extension}"),assets,status)
        .ok_or("No fue posible abrir el audio recuperado")?;
    let version=Version {
        id:media::uid("version"),asset_id:id.clone(),label:name_hint,
        start:source_in,end:source_in+length,
        gain:10_f64.powf(gain_db/20.).clamp(0.,2.),
        fade_in,fade_out,reversed,
    };
    selected.set(id);
    versions.update(|all|all.push(version.clone()));
    current_version.set(version.id);
    Ok(format!("Proyecto {name} recuperado. El editor abrió una copia local; Guardar crea una nueva revisión de proyecto independiente."))
}
pub fn open_saved_project(name:String,assets:RwSignal<Vec<AudioAsset>>,
    selected:RwSignal<String>,versions:RwSignal<Vec<Version>>,
    current_version:RwSignal<String>,status:RwSignal<String>
) {
    status.set(format!("Abriendo {name} desde VSCore…"));
    spawn_local(async move {
        let msg=match load_saved(&name,assets,selected,versions,current_version,status).await {
            Ok(ok)=>ok,Err(error)=>format!("No se pudo abrir {name}: {error}")
        };
        status.set(msg);
    });
}
