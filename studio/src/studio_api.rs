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
