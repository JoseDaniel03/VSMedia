//! Persistent Minecraft OGG/Vorbis sound entries in the existing Studio CAS.
//! Only lightweight validation and byte persistence happen here; conversion
//! must happen in the browser/offline encoder, never on a Folia region thread.
use axum::{
    body::Bytes,
    extract::{Path,State},
    http::{header,HeaderMap,StatusCode},
    Json,
};
use serde_json::{json,Map,Value};
use vsmedia_api::{AssetKey,AssetKind,AssetRepository};
use vsmedia_pack::MergeOutput;
use crate::{Store,ApiError,authorize_origin,bad,not_found};

const MAX_SOUND_BYTES:usize=16*1024*1024;
const MAX_COUNT:usize=256;
const MAX_TOTAL_BYTES:u64=192*1024*1024;
const PREFIX:&str="published-sounds/";

fn valid_id(id:&str)->bool {
    (1..=48).contains(&id.len()) &&
        id.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'_'||b==b'-')
}
fn key(id:&str)->Result<AssetKey,ApiError> {
    if !valid_id(id) {return Err(bad("ID inválido (a-z, 0-9, guión o _; 1..48 caracteres)"));}
    AssetKey::new("studio",format!("{PREFIX}{id}.ogg")).map_err(bad)
}
fn name(record:&vsmedia_api::AssetRecord)->Option<String> {
    record.key.name.strip_prefix(PREFIX)?
        .strip_suffix(".ogg").filter(|id|valid_id(id)).map(str::to_owned)
}
fn vorbis(bytes:&[u8])->bool {
    // Minimum signature check: an Ogg stream with a Vorbis identification
    // packet. This does not claim to fully decode/verify all audio frames.
    bytes.len()>=64 && bytes.starts_with(b"OggS") &&
        bytes[..bytes.len().min(512)].windows(7).any(|s|s==b"\x01vorbis")
}

pub async fn publish(
    State(store):State<Store>,
    Path(id):Path<String>,
    headers:HeaderMap,
    body:Bytes
)->Result<Json<Value>,ApiError> {
    authorize_origin(&headers)?;
    let key=key(&id)?;
    if body.len()>MAX_SOUND_BYTES {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,
            "Archivo superior a 16 MiB; comprimí en el navegador".into()));
    }
    if !vorbis(&body) {
        return Err(bad("Se necesita OGG Vorbis real; WAV, Opus, MP3 y OGG vacíos no se registran como sonidos de Minecraft"));
    }
    let entries=store.list_assets("studio",PREFIX,MAX_COUNT).map_err(bad)?;
    let old=store.metadata(&key).ok().map(|r|r.size).unwrap_or(0);
    if old==0 && entries.len()>=MAX_COUNT {
        return Err(bad("Máximo de 256 sonidos publicados"));
    }
    let used:u64=entries.iter().map(|r|r.size).sum();
    if used.saturating_sub(old).saturating_add(body.len() as u64)>MAX_TOTAL_BYTES {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,
            "La biblioteca de sonidos publicados superaría 192 MiB".into()));
    }
    let record=store.publish_for("studio",key,AssetKind::Sound,&body).map_err(not_found)?;
    Ok(Json(json!({
        "ok":true,"id":format!("vsmedia:published.{id}"),
        "name":id,"sha256":record.sha256,"bytes":record.size,
        "addedToPack":true,"deliveredToMinecraft":false,
        "next":"Exportá /api/v1/packs/export y publicá la nueva versión HTTPS del ZIP"
    })))
}

pub async fn list(State(store):State<Store>)->Result<Json<Value>,ApiError> {
    let records=store.list_assets("studio",PREFIX,MAX_COUNT).map_err(bad)?;
    let sounds=records.iter().filter_map(|record|{
        let id=name(record)?;
        Some(json!({"name":id,"id":format!("vsmedia:published.{id}"),
            "sha256":record.sha256,"bytes":record.size}))
    }).collect::<Vec<_>>();
    Ok(Json(json!({"schema":"vsmedia-sounds-v1","sounds":sounds})))
}

/// Adds the persistent sound files and event IDs to the SAME final pack as
/// the imported fonts. A pre-existing sounds.json retains unrelated entries.
pub(crate) fn add_to_pack(store:&Store,pack:&mut MergeOutput)->Result<(),ApiError> {
    let records=store.list_assets("studio",PREFIX,MAX_COUNT).map_err(bad)?;
    if records.is_empty(){return Ok(());}
    let manifest_path="assets/vsmedia/sounds.json";
    let mut manifest:Map<String,Value>=match pack.files.get(manifest_path){
        None=>Map::new(),
        Some(bytes)=>{
            let value:Value=serde_json::from_slice(bytes).map_err(bad)?;
            value.as_object().cloned().ok_or_else(||bad("assets/vsmedia/sounds.json no es un objeto JSON"))?
        }
    };
    for record in records {
        let Some(id)=name(&record) else {continue};
        let sound=store.read(&record.key).map_err(not_found)?;
        if !vorbis(&sound){
            return Err(bad(format!("OGG Vorbis corrupto en sonido publicado: {id}")));
        }
        pack.files.insert(format!("assets/vsmedia/sounds/published/{id}.ogg"),sound);
        manifest.insert(format!("published.{id}"),json!({
            "sounds":[{"name":format!("vsmedia:published/{id}"),"stream":true}]
        }));
    }
    pack.files.insert(manifest_path.into(),serde_json::to_vec(&Value::Object(manifest)).map_err(bad)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_ids_and_rejects_false_ogg(){
        assert!(valid_id("radio_01"));
        assert!(!valid_id("../traversal"));
        assert!(!valid_id("CapitalLetters"));
        assert!(!vorbis(b"OggSrandom-not-real-vorbis"));
        assert!(!vorbis(b"RIFF----WAVE"));
        let mut header=vec![0;64];
        header[..4].copy_from_slice(b"OggS");
        header[10..17].copy_from_slice(b"\x01vorbis");
        assert!(vorbis(&header));
    }
}
