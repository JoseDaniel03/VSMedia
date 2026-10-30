//! Browser-to-Rust Studio bridge. Explicit user action is required for upload.
//! No game or radio operation is emulated: only CAS assets and VSAudio projects.
use crate::media::{self,AudioAsset,Version,Track};
use js_sys::{ArrayBuffer,Uint8Array};
use std::{cell::RefCell,collections::HashMap};
use web_sys::{Blob,Url};
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
async fn compose_audio(asset:AudioAsset,version:Version,duration:f64)->Result<Value,String>{
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
    if !version.pan.is_finite() || !(-1.0..=1.0).contains(&version.pan) {
        return Err("El paneo debe estar entre -100 y +100".into());
    }
    let mut filters=Vec::<Value>::new();
    for (frequency,kind) in [
        (version.high_pass_hz,"high_pass"),
        (version.low_pass_hz,"low_pass"),
    ] {
        if let Some(hz)=frequency {
            if !hz.is_finite() || !(20.0..=18_000.0).contains(&hz) {
                return Err("La frecuencia EQ debe estar entre 20 y 18000 Hz".into());
            }
            filters.push(json!({"type":kind,"frequency_hz":hz,"q":0.707}));
        }
    }
    if version.high_pass_hz.zip(version.low_pass_hz)
        .is_some_and(|(high,low)|high>=low) {
        return Err("El pasa-altos debe ser menor que el pasa-bajos".into());
    }
    let mut gain_points=vec![json!({"at_ms":0,"gain_db":gain_db})];
    if let Some(end_gain)=version.end_gain {
        if !end_gain.is_finite() || !(0.0..=2.0).contains(&end_gain) {
            return Err("Ganancia final inválida".into());
        }
        let end_db=if end_gain==0. {-120.0} else {20.*end_gain.log10()};
        gain_points.push(json!({"at_ms":length,"gain_db":end_db}));
    }
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
                "pan":version.pan,"fade_in_ms":fade_in,"fade_out_ms":fade_out,
                "reversed":version.reversed,"muted":false,
                "gain_points":gain_points,"filters":filters
            }]
        }}
    });
    Ok(project)
}
async fn save(asset:AudioAsset,version:Version,duration:f64)->Result<String,String> {
    let project_id=asset.id.clone();
    let document=compose_audio(asset,version,duration).await?;
    let saved=request(&format!("/audio/projects/{project_id}"),"POST",
        JsValue::from_str(&document.to_string()),"application/json").await?;
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
    let filters=edit.and_then(|v|v.get("filters")).and_then(Value::as_array);
    let frequency=|kind:&str| filters.and_then(|filters|filters.iter().find(|filter|
        filter.get("type").and_then(Value::as_str)==Some(kind)))
        .and_then(|filter|filter.get("frequency_hz")).and_then(Value::as_f64);
    let end_gain=edit.and_then(|v|v.get("gain_points"))
        .and_then(Value::as_array).and_then(|list| {
            if list.len()>1 {list.last()} else {None}
        }).and_then(|point|point.get("gain_db")).and_then(Value::as_f64)
        .map(|db|10_f64.powf(db/20.).clamp(0.,2.));
    let version=Version {
        id:media::uid("version"),asset_id:id.clone(),label:name_hint,
        start:source_in,end:source_in+length,
        gain:10_f64.powf(gain_db/20.).clamp(0.,2.),
        fade_in,fade_out,reversed,
        pan:edit.and_then(|v|v.get("pan")).and_then(Value::as_f64)
            .unwrap_or(0.),
        low_pass_hz:frequency("low_pass"),
        high_pass_hz:frequency("high_pass"),
        end_gain,
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

/// Publish a VSMix session as a genuine multipista VSAudio document.
/// Uploads occur only on explicit user action and never on local import.
pub fn save_mix_project(
    assets:Vec<AudioAsset>,tracks:Vec<Track>,status:RwSignal<String>
) {
    status.set("Publicando pistas y proyecto multipista en VSCore…".into());
    spawn_local(async move {
        let result=save_mix(&assets,&tracks).await;
        status.set(match result {
            Ok(message)=>message,
            Err(error)=>format!("No se guardó VSMix: {error}")
        });
    });
}
/// Compose a native timeline. This uploads only explicitly referenced audio
/// to the Studio CAS; it does not create or replace any project revision.
async fn compose_mix(assets:&[AudioAsset],tracks:&[Track])->Result<Value,String> {
    if tracks.is_empty() || tracks.len()>256 {
        return Err("La mezcla debe tener entre 1 y 256 pistas".into());
    }
    let mut known=std::collections::HashMap::<String,String>::new();
    let mut sources=Vec::new();
    let mut timeline_tracks=Vec::new();
    let mut clip_edits=Vec::new();
    let mut track_edits=Vec::new();
    let mut total_clips=0usize;
    for (index,track) in tracks.iter().enumerate() {
        if track.name.trim().is_empty() {return Err("Todas las pistas necesitan nombre".into());}
        if !track.volume.is_finite() || !(0.0..=1.0).contains(&track.volume) {
            return Err(format!("Volumen inválido en {}",track.name));
        }
        let track_db=if track.volume==0. {-120.0} else{20.*track.volume.log10()};
        track_edits.push(json!({"track_index":index,"gain_db":track_db,
            "pan":0.0,"muted":track.muted,"solo":track.solo}));
        // Older sessions retain their one-file track. Once regions exist,
        // region editing is authoritative; never silently append the legacy file.
        let regions=if track.regions.is_empty() && !track.asset_id.is_empty() {
            let duration=media::player(&format!("track-{}",track.id))
                .map(|player|player.duration()).unwrap_or(0.);
            vec![media::MixRegion{id:format!("legacy-{}",track.id),
                asset_id:track.asset_id.clone(),start:0.,duration,source_in:0.,
                volume:1.,pan:0.,fade_in:track.fade_in,fade_out:track.fade_out}]
        }else{track.regions.clone()};
        total_clips=total_clips.saturating_add(regions.len());
        if total_clips>256 {return Err("La mezcla supera el límite de 256 regiones".into());}
        let mut clips=Vec::with_capacity(regions.len());
        for (clip_index,region) in regions.iter().enumerate() {
            let asset=assets.iter().find(|item|item.id==region.asset_id)
                .ok_or_else(||format!("No se encuentra el audio de una región de {}",track.name))?;
            let kind=codec(&asset.name)
                .ok_or("VSMix admite WAV, MP3, FLAC, OGG, Opus y WebM")?;
            let start_ms=millis(region.start)?;
            let source_in_ms=millis(region.source_in)?;
            let length=millis(region.duration)?;
            let fade_in=millis(region.fade_in)?;
            let fade_out=millis(region.fade_out)?;
            if length==0 || fade_in.saturating_add(fade_out)>length {
                return Err(format!("Una región de {} tiene duración o fades inválidos",track.name));
            }
            if !region.volume.is_finite() || !(0.0..=2.0).contains(&region.volume)
                || !region.pan.is_finite() || !(-1.0..=1.0).contains(&region.pan) {
                return Err(format!("Una región de {} tiene volumen o paneo inválido",track.name));
            }
            if !known.contains_key(&asset.id) {
                let blob=media::get_blob(&asset.id)
                    .ok_or("El audio original no está disponible en memoria")?;
                if blob.size()<=0. || blob.size()>16.*1024.*1024. {
                    return Err("Cada audio debe ocupar entre 1 B y 16 MiB".into());
                }
                let raw=JsFuture::from(blob.array_buffer()).await
                    .map_err(|_|"No se pudo leer un audio de VSMix")?;
                let array:ArrayBuffer=raw.dyn_into().map_err(|_|"Audio inválido")?;
                let uploaded=request(&format!("/assets/{}",asset.id),"POST",
                    Uint8Array::new(&array).into(),"application/octet-stream").await?;
                let sha=uploaded.get("sha256").and_then(Value::as_str)
                    .ok_or("VSCore no devolvió el SHA-256 del audio")?.to_owned();
                known.insert(asset.id.clone(),sha.clone());
                sources.push(json!({
                    "key":{"namespace":"studio","name":format!("audio/{}",asset.id)},
                    "sha256":sha,"codec":kind
                }));
            }
            let gain_db=if region.volume==0. {-120.0} else{20.*region.volume.log10()};
            clips.push(json!({
                "asset":{"namespace":"studio","name":format!("audio/{}",asset.id)},
                "start_ms":start_ms,"duration_ms":length,"gain_db":gain_db
            }));
            clip_edits.push(json!({
                "track_index":index,"clip_index":clip_index,
                "source_in_ms":source_in_ms,"pan":region.pan,
                "fade_in_ms":fade_in,"fade_out_ms":fade_out,
                "reversed":false,"muted":false,"gain_points":[],"filters":[]
            }));
        }
        timeline_tracks.push(json!({"name":track.name,"muted":false,"clips":clips}));
    }
    Ok(json!({"magic":"VSMEDIA","version":1,
        "payload":{"kind":"audio","data":{
            "timeline":{"name":"VSMix FullCanvas","tracks":timeline_tracks},
            "sources":sources,"clip_edits":clip_edits,"track_edits":track_edits
        }}}))
}
async fn save_mix(assets:&[AudioAsset],tracks:&[Track])->Result<String,String> {
    let document=compose_mix(assets,tracks).await?;
    let project_id=media::uid("mix");
    let result=request(&format!("/audio/projects/{project_id}"),"POST",
        JsValue::from_str(&document.to_string()),"application/json").await?;
    let revision=result.get("revision").and_then(Value::as_u64).unwrap_or(0);
    Ok(format!("VSMix guardado como {project_id}.vsaudio (revisión {revision})."))
}

/// Rehydrate a native multipista document into the local VSMix session.
/// Reject unsupported multi-clip tracks instead of losing timing data.
async fn load_mix(name:&str,assets:RwSignal<Vec<AudioAsset>>,
    tracks:RwSignal<Vec<Track>>,status:RwSignal<String>
)->Result<String,String> {
    if name.is_empty() || !name.bytes().all(|b|
        b.is_ascii_alphanumeric() || matches!(b,b'-'|b'_'|b'.')) {
        return Err("Identificador de proyecto inválido".into());
    }
    let doc=request(&format!("/audio/projects/{name}"),"GET",JsValue::NULL,"").await?;
    if doc.pointer("/payload/kind").and_then(Value::as_str)!=Some("audio") {
        return Err("El proyecto no contiene audio".into());
    }
    let body=doc.pointer("/payload/data").ok_or("Documento de audio inválido")?;
    let timeline=body.get("timeline").ok_or("Proyecto sin línea de tiempo")?;
    let entries=timeline.get("tracks").and_then(Value::as_array)
        .ok_or("Estructura de pistas inválida")?;
    if entries.len()>256 {return Err("Demasiadas pistas para VSMix".into());}
    let sources=body.get("sources").and_then(Value::as_array)
        .ok_or("Proyecto sin referencias de origen")?;
    let mut pending=Vec::<(String,String,web_sys::Blob)>::new();
    // Stage downloads and validate every source before adding any to the UI.
    for track in entries {
        let clips=track.get("clips").and_then(Value::as_array)
            .ok_or("Pista con lista de clips inválida")?;
        for clip in clips {
        let key=clip.get("asset").ok_or("Clip sin referencia a audio")?;
        if key.get("namespace").and_then(Value::as_str)!=Some("studio") {
            return Err("Una pista apunta fuera del namespace de Studio".into());
        }
        let short=key.get("name").and_then(Value::as_str)
            .and_then(|v|v.strip_prefix("audio/"))
            .ok_or("Ruta de audio inválida")?;
        if short.is_empty() || !short.bytes().all(|b|
            b.is_ascii_alphanumeric() || matches!(b,b'_'|b'-'|b'.')) {
            return Err("Identificador de archivo inseguro".into());
        }
        if pending.iter().any(|(id,_,_)|id==short){continue;}
        let source=sources.iter().find(|source|
            source.pointer("/key/namespace").and_then(Value::as_str)==Some("studio")
            && source.pointer("/key/name").and_then(Value::as_str)
                .and_then(|full|full.strip_prefix("audio/"))==Some(short)
        ).ok_or("Clip sin archivo pinneado")?;
        let expected=source.get("sha256").and_then(Value::as_str)
            .ok_or("Fuente sin SHA-256")?;
        let metadata=request(&format!("/assets/{short}/metadata"),"GET",JsValue::NULL,"").await?;
        if metadata.get("sha256").and_then(Value::as_str)!=Some(expected) {
            return Err("Uno de los audios fue modificado desde que se guardó la mezcla.".into());
        }
        let codec=source.get("codec").and_then(Value::as_str).unwrap_or("wav");
        let suffix=match codec {
            "wav"=>"wav","mp3"=>"mp3","flac"=>"flac",
            "ogg_vorbis"=>"ogg","opus"=>"opus","webm"=>"webm",
            _=>return Err("Códec no compatible con VSMix".into())
        };
        let blob=fetch_source_blob(short).await?;
        if blob.size()<=0. || blob.size()>16.*1024.*1024. {
            return Err("Un audio supera el límite de descarga local".into());
        }
        pending.push((short.to_owned(),suffix.to_owned(),blob));
        }
    }
    let mut local_map=std::collections::HashMap::<String,String>::new();
    for (original,extension,blob) in pending {
        let filename=format!("{original}.{extension}");
        let new_id=media::add_blob(blob,filename,assets,status)
            .ok_or("No se pudo incorporar el archivo descargado")?;
        local_map.insert(original,new_id);
    }
    let mut recovered=Vec::with_capacity(entries.len());
    for (idx,t) in entries.iter().enumerate() {
        let name=t.get("name").and_then(Value::as_str)
            .unwrap_or("Pista").to_owned();
        let clips=t.get("clips").and_then(Value::as_array)
            .ok_or("Pista sin lista de clips")?;
        let edit=body.get("track_edits").and_then(Value::as_array)
            .and_then(|list|list.iter().find(|value|
                value.get("track_index").and_then(Value::as_u64)==Some(idx as u64)));
        let gain_db=edit.and_then(|v|v.get("gain_db"))
            .and_then(Value::as_f64).unwrap_or(0.);
        let mut regions=Vec::with_capacity(clips.len());
        for (clip_idx,clip) in clips.iter().enumerate() {
            let short=clip.pointer("/asset/name").and_then(Value::as_str)
                .and_then(|value|value.strip_prefix("audio/"))
                .ok_or("Audio de región inválido")?;
            let asset_id=local_map.get(short).cloned()
                .ok_or("No se encontró una fuente descargada para una región")?;
            let edit=body.get("clip_edits").and_then(Value::as_array)
                .and_then(|list|list.iter().find(|value|
                    value.get("track_index").and_then(Value::as_u64)==Some(idx as u64)
                    && value.get("clip_index").and_then(Value::as_u64)==Some(clip_idx as u64)));
            let db=clip.get("gain_db").and_then(Value::as_f64).unwrap_or(0.);
            regions.push(media::MixRegion{
                id:media::uid("region"),asset_id,
                start:clip.get("start_ms").and_then(Value::as_u64)
                    .ok_or("Posición de región inválida")? as f64/1000.,
                duration:clip.get("duration_ms").and_then(Value::as_u64)
                    .ok_or("Duración de región inválida")? as f64/1000.,
                source_in:edit.and_then(|value|value.get("source_in_ms"))
                    .and_then(Value::as_u64).unwrap_or(0) as f64/1000.,
                volume:10_f64.powf(db/20.).clamp(0.,2.),
                pan:edit.and_then(|value|value.get("pan"))
                    .and_then(Value::as_f64).unwrap_or(0.),
                fade_in:edit.and_then(|value|value.get("fade_in_ms"))
                    .and_then(Value::as_u64).unwrap_or(0) as f64/1000.,
                fade_out:edit.and_then(|value|value.get("fade_out_ms"))
                    .and_then(Value::as_u64).unwrap_or(0) as f64/1000.,
            });
        }
        let legacy_id=regions.first().map(|region|region.asset_id.clone())
            .unwrap_or_default();
        recovered.push(Track {
            id:media::uid("track"),name,asset_id:legacy_id,
            volume:10_f64.powf(gain_db/20.).clamp(0.,1.),
            muted:t.get("muted").and_then(Value::as_bool).unwrap_or(false)
                ||edit.and_then(|v|v.get("muted")).and_then(Value::as_bool).unwrap_or(false),
            solo:edit.and_then(|v|v.get("solo")).and_then(Value::as_bool).unwrap_or(false),
            fade_in:regions.first().map_or(0.,|r|r.fade_in),
            fade_out:regions.first().map_or(0.,|r|r.fade_out),
            regions,
        });
    }
    tracks.set(recovered);
    Ok(format!("Mezcla {name} recuperada desde VSCore con sus pistas y controles. Guardar crea una nueva copia."))
}
pub fn open_saved_mix(name:String,assets:RwSignal<Vec<AudioAsset>>,
    tracks:RwSignal<Vec<Track>>,status:RwSignal<String>
) {
    status.set(format!("Recuperando mezcla {name} desde VSCore…"));
    spawn_local(async move {
        let result=load_mix(&name,assets,tracks,status).await;
        status.set(match result {
            Ok(message)=>message,
            Err(error)=>format!("No se pudo recuperar VSMix: {error}")
        });
    });
}

/// One-shot connection probe; local editing remains available if VSCore
/// is not running. Refresh is initiated explicitly by a user click.
pub fn probe_engine(available:RwSignal<bool>) {
    spawn_local(async move {
        let result=request("/health","GET",JsValue::NULL,"").await;
        available.set(result.ok().as_ref().is_some_and(|data|
            data.get("status").and_then(Value::as_str)==Some("ready")
            && data.get("storage_connected").and_then(Value::as_bool)==Some(true)
        ));
    });
}

/// A renderer preview is always generated from a stored .vsaudio revision.
/// The browser never pretends that its local HTMLAudioElement player applies
/// Rust's EQ, automation, reverse or multitrack mixing to the source file.
async fn request_engine_wav(name:&str)->Result<Blob,String> {
    if name.is_empty() || !name.bytes().all(|b|
        b.is_ascii_alphanumeric() || matches!(b,b'-'|b'_'|b'.')) {
        return Err("Nombre de proyecto no válido".into());
    }
    let req=Request::new_with_str_and_init(
        &format!("{BASE}/audio/projects/{name}/render"),&RequestInit::new()
    ).map_err(|_|"No se pudo crear la solicitud de render")?;
    let window=web_sys::window().ok_or("No hay ventana")?;
    let response=JsFuture::from(window.fetch_with_request(&req)).await
        .map_err(|_|"No se pudo conectar con el Audio Engine de VSCore")?;
    let response:Response=response.dyn_into()
        .map_err(|_|"Respuesta del Audio Engine no válida")?;
    if !response.ok() {
        let body=JsFuture::from(response.text()
            .map_err(|_|"Error leyendo mensaje del Audio Engine")?).await
            .map_err(|_|"Error leyendo mensaje del Audio Engine")?;
        let message=body.as_string().and_then(|raw|
            serde_json::from_str::<Value>(&raw).ok()
        ).and_then(|value|value.get("error").and_then(Value::as_str)
            .map(ToOwned::to_owned))
        .unwrap_or_else(||format!("El Audio Engine respondió con HTTP {}",response.status()));
        return Err(message);
    }
    let response_body=JsFuture::from(response.blob()
        .map_err(|_|"El WAV no pudo prepararse")?).await
        .map_err(|_|"Error recibiendo el WAV procesado")?;
    let blob:Blob=response_body.dyn_into()
        .map_err(|_|"VSCore devolvió un archivo de audio inválido")?;
    if blob.size()<44. || blob.size()>64.*1024.*1024. {
        return Err("El render tiene un tamaño inválido o supera los límites".into());
    }
    Ok(blob)
}
thread_local! {
    static ENGINE_PREVIEWS: RefCell<HashMap<&'static str,String>> =
        RefCell::new(HashMap::new());
}
/// Listen to actual Rust-engine rendered WAV, not a browser approximation.
pub fn preview_engine(name:String,player_id:&'static str,status:RwSignal<String>) {
    status.set(format!("Renderizando {name} con el Audio Engine de Rust…"));
    spawn_local(async move {
        let result=async {
            let wav=request_engine_wav(&name).await?;
            let url=Url::create_object_url_with_blob(&wav)
                .map_err(|_|"No se pudo reproducir el WAV generado")?;
            let player=media::player(player_id)
                .ok_or("No se encontró el reproductor de previsualización")?;
            player.pause().ok();
            player.set_src(&url);
            player.set_current_time(0.);
            ENGINE_PREVIEWS.with(|previews|{
                if let Some(old)=previews.borrow_mut().insert(player_id,url) {
                    Url::revoke_object_url(&old).ok();
                }
            });
            player.play().map_err(|_|"El navegador bloqueó la reproducción")?;
            Ok::<(),String>(())
        }.await;
        status.set(match result {
            Ok(())=>format!("Escuchando {name}: WAV renderizado con el motor Rust."),
            Err(error)=>format!("No se pudo renderizar {name}: {error}"),
        });
    });
}
pub fn export_engine_wav(name:String,status:RwSignal<String>) {
    status.set(format!("Preparando WAV definitivo de {name} con VSCore…"));
    spawn_local(async move {
        let result=async {
            let wav=request_engine_wav(&name).await?;
            media::download_blob(&wav,&format!("{name}-vsmedia-render.wav"))
        }.await;
        status.set(match result {
            Ok(())=>format!("WAV renderizado por VSCore preparado para descargar: {name}."),
            Err(error)=>format!("No se pudo exportar {name}: {error}"),
        });
    });
}

/// Render the current unsaved region layout. Audio sources are uploaded to
/// Studio CAS on this explicit action, but NO .vsaudio project is persisted.
async fn request_draft_wav(document:&Value)->Result<Blob,String> {
    let settings=RequestInit::new();
    settings.set_method("POST");
    settings.set_mode(RequestMode::Cors);
    settings.set_body(&JsValue::from_str(&document.to_string()));
    let req=Request::new_with_str_and_init(&format!("{BASE}/audio/render"),&settings)
        .map_err(|_|"No se pudo preparar el render del borrador")?;
    req.headers().set("Content-Type","application/json")
        .map_err(|_|"No se pudo enviar el manifiesto de la mezcla")?;
    let window=web_sys::window().ok_or("No hay navegador")?;
    let response=JsFuture::from(window.fetch_with_request(&req)).await
        .map_err(|_|"El Audio Engine local no está disponible")?;
    let response:Response=response.dyn_into()
        .map_err(|_|"El Audio Engine devolvió una respuesta inválida")?;
    if !response.ok() {
        let text=JsFuture::from(response.text()
            .map_err(|_|"No se pudo leer el error del motor")?).await
            .map_err(|_|"No se pudo leer el error del motor")?;
        let detail=text.as_string().and_then(|raw|
            serde_json::from_str::<Value>(&raw).ok()
        ).and_then(|value|value.get("error").and_then(Value::as_str)
            .map(ToOwned::to_owned))
        .unwrap_or_else(||format!("Render rechazado (HTTP {})",response.status()));
        return Err(detail);
    }
    let blob=JsFuture::from(response.blob()
        .map_err(|_|"No se pudo abrir el audio renderizado")?).await
        .map_err(|_|"Error al recibir el audio renderizado")?;
    let wav:Blob=blob.dyn_into()
        .map_err(|_|"El motor no devolvió un WAV válido")?;
    if wav.size()<44. || wav.size()>64.*1024.*1024. {
        return Err("El WAV excede el tamaño de previsualización permitido".into());
    }
    Ok(wav)
}
pub fn preview_draft_mix(
    assets:Vec<AudioAsset>,tracks:Vec<Track>,status:RwSignal<String>
) {
    status.set("Preparando regiones y fuentes para render Rust temporal…".into());
    spawn_local(async move {
        let result=async {
            let document=compose_mix(&assets,&tracks).await?;
            let wav=request_draft_wav(&document).await?;
            let url=Url::create_object_url_with_blob(&wav)
                .map_err(|_|"No se pudo reproducir el render temporal")?;
            let player=media::player("mix-draft-engine-audio")
                .ok_or("No se encuentra el reproductor del borrador")?;
            player.pause().ok();
            player.set_src(&url);
            player.set_current_time(0.);
            ENGINE_PREVIEWS.with(|previews| {
                if let Some(previous)=previews.borrow_mut().insert("mix-draft-engine-audio",url) {
                    Url::revoke_object_url(&previous).ok();
                }
            });
            player.play().map_err(|_|"El navegador bloqueó la reproducción")?;
            Ok::<(),String>(())
        }.await;
        status.set(match result {
            Ok(())=>"VSMix: previsualización Rust actual, sin guardar proyecto.".into(),
            Err(error)=>format!("No se pudo previsualizar VSMix: {error}"),
        });
    });
}

/// Preview the active VSAudio clip through Rust without saving a .vsaudio
/// revision. A deliberate click may upload its source to the local CAS.
pub fn preview_draft_audio(
    asset:AudioAsset,version:Version,duration:f64,status:RwSignal<String>
) {
    status.set("Renderizando versión actual con VSCore, sin guardar proyecto…".into());
    spawn_local(async move {
        let result=async {
            let doc=compose_audio(asset,version,duration).await?;
            let wav=request_draft_wav(&doc).await?;
            let url=Url::create_object_url_with_blob(&wav)
                .map_err(|_|"No se pudo preparar el WAV del clip")?;
            let player=media::player("simple-draft-engine-audio")
                .ok_or("Reproductor del Audio Engine no disponible")?;
            player.pause().ok();
            player.set_src(&url);
            player.set_current_time(0.);
            ENGINE_PREVIEWS.with(|previews| {
                if let Some(previous)=previews.borrow_mut().insert("simple-draft-engine-audio",url) {
                    Url::revoke_object_url(&previous).ok();
                }
            });
            player.play().map_err(|_|"El navegador bloqueó el audio")?;
            Ok::<(),String>(())
        }.await;
        status.set(match result {
            Ok(())=>"Clip procesado con Rust, sin guardar proyecto .vsaudio.".into(),
            Err(error)=>format!("No se pudo previsualizar el clip: {error}"),
        });
    });
}
