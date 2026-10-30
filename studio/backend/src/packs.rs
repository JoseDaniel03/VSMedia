//! Session-local Resource Pack compositor for FullCanvas Studio.
//! Reuses the production Rust import/structured merge/deterministic export.
//! Strict in-memory quotas; no arbitrary filesystem writes or external URLs.
//! All operations are local-only. Public CDN / Folia delivery comes later.
use std::{collections::{BTreeMap,BTreeSet},sync::Arc};
use axum::{
    body::Bytes,extract::{Path,Query},Extension,http::{header,HeaderMap,StatusCode},
    response::{IntoResponse,Response},Json,
};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use tokio::sync::Mutex;
use vsmedia_pack::{archive::{self,ArchiveLimits},structured,PackAsset,MergeOutput};

use crate::{ApiError,authorize_origin,bad};

const MAX_ZIP_BYTES:usize=12*1024*1024;
const MAX_RAM_BYTES:usize=64*1024*1024;
const MAX_PACKS:usize=6;
const MAX_PREVIEW_CONFLICTS:usize=200;
fn limits()->ArchiveLimits {
    ArchiveLimits {max_files:2048,max_total_bytes:32*1024*1024,
        max_single_file_bytes:8*1024*1024}
}
#[derive(Clone,Default)]
pub struct PackHub(Arc<Mutex<Vec<LoadedPack>>>);
#[derive(Clone)]
struct LoadedPack {
    id:String,
    name:String,
    priority:i32,
    files:Arc<Vec<PackAsset>>,
    bytes:usize,
}
#[derive(Serialize,Debug)]
pub struct PackInfo {
    id:String,name:String,priority:i32,files:usize,bytes:usize
}
impl From<&LoadedPack> for PackInfo {
    fn from(pack:&LoadedPack)->Self{
        Self {id:pack.id.clone(),name:pack.name.clone(),
            priority:pack.priority,files:pack.files.len(),bytes:pack.bytes}
    }
}
fn valid_id(id:&str)->bool{
    (1..=48).contains(&id.len())&&id.bytes().all(|b|
        b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'_'||b==b'-')
}
fn friendly_error(err:impl std::fmt::Display)->ApiError{
    bad(format!("Resource Pack inválido: {err}"))
}
pub async fn list(Extension(hub):Extension<PackHub>)->Json<Value>{
    let packs=hub.0.lock().await;
    Json(json!({"packs":packs.iter().map(PackInfo::from).collect::<Vec<_>>(),
        "total_bytes":packs.iter().map(|pack|pack.bytes).sum::<usize>(),
        "max_packs":MAX_PACKS,"max_ram_bytes":MAX_RAM_BYTES,
        "max_zip_bytes":MAX_ZIP_BYTES,"session_only":true,
        "minecraft_delivery":false}))
}
pub async fn import(
    Extension(hub):Extension<PackHub>,Path(id):Path<String>,
    headers:HeaderMap,bytes:Bytes
)->Result<Json<PackInfo>,ApiError>{
    authorize_origin(&headers)?;
    if !valid_id(&id){return Err(bad("ID de pack inválido"));}
    if bytes.is_empty()||bytes.len()>MAX_ZIP_BYTES {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,
            "ZIP debe ocupar entre 1 byte y 12 MiB".into()));
    }
    let label=headers.get("x-vspack-label").and_then(|h|h.to_str().ok())
        .map(|raw|percent_encoding::percent_decode_str(raw).decode_utf8()
            .map(|value|value.trim().to_owned()))
        .transpose().map_err(|_|bad("Nombre UTF-8 inválido"))?
        .unwrap_or_else(||id.clone());
    if label.is_empty()||label.chars().count()>80||label.chars().any(char::is_control) {
        return Err(bad("Nombre de pack inválido (máximo 80 caracteres)"));
    }
    // Reject duplicate IDs and excessive packs BEFORE decompression.
    {
        let saved=hub.0.lock().await;
        if saved.iter().any(|p|p.id==id) {
            return Err(ApiError(StatusCode::CONFLICT,
                "Ese pack ya se encuentra en la sesión".into()));
        }
        if saved.len()>=MAX_PACKS {return Err(bad("Máximo 6 packs por sesión"));}
    }
    let source_id=id.clone();
    let imported=tokio::task::spawn_blocking(move||
        archive::import_zip(&bytes,&source_id,0,limits())
    ).await.map_err(|e|bad(format!("Importación interrumpida: {e}")))?
        .map_err(friendly_error)?;
    if imported.is_empty(){return Err(bad("ZIP sin archivos de Resource Pack"));}
    let used=imported.iter().map(|p|p.data.len()).sum::<usize>();
    let mut saved=hub.0.lock().await;
    if saved.iter().any(|p|p.id==id) {
        return Err(ApiError(StatusCode::CONFLICT,
            "Ese pack ya se encuentra en la sesión".into()));
    }
    if saved.len()>=MAX_PACKS ||
        saved.iter().map(|p|p.bytes).sum::<usize>().saturating_add(used)>MAX_RAM_BYTES {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,
            "El límite de 6 packs / 64 MiB de datos descomprimidos fue alcanzado".into()));
    }
    let next_priority=(saved.len() as i32)*10;
    let pack=LoadedPack{id,name:label,priority:next_priority,files:Arc::new(imported),bytes:used};
    let out=PackInfo::from(&pack);
    saved.push(pack);
    Ok(Json(out))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriorityInput {priority:i32}
pub async fn priority(
    Extension(hub):Extension<PackHub>,Path(id):Path<String>,
    headers:HeaderMap,Json(change):Json<PriorityInput>
)->Result<Json<PackInfo>,ApiError>{
    authorize_origin(&headers)?;
    if !(-1000..=1000).contains(&change.priority){
        return Err(bad("La prioridad debe estar entre -1000 y 1000"));
    }
    let mut packs=hub.0.lock().await;
    let item=packs.iter_mut().find(|p|p.id==id)
        .ok_or_else(||ApiError(StatusCode::NOT_FOUND,"Pack no encontrado".into()))?;
    item.priority=change.priority;
    Ok(Json(PackInfo::from(&*item)))
}
pub async fn remove(
    Extension(hub):Extension<PackHub>,Path(id):Path<String>,headers:HeaderMap
)->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;
    let mut packs=hub.0.lock().await;
    let len=packs.len();
    packs.retain(|p|p.id!=id);
    if len==packs.len(){
        return Err(ApiError(StatusCode::NOT_FOUND,"Pack no encontrado".into()));
    }
    Ok(Json(json!({"deleted":true,"id":id})))
}
/// Clone only small Arc handles under lock. CPU-heavy structured merge/ZIP
/// compression runs on a bounded blocking thread, not a Folia/async worker.
async fn composed(hub:&PackHub)->Result<MergeOutput,ApiError>{
    let current=hub.0.lock().await.clone();
    if current.is_empty(){return Err(bad("Primero importá un Resource Pack ZIP"));}
    tokio::task::spawn_blocking(move || {
        let files=current.iter().flat_map(|p|
            p.files.iter().cloned().map(|mut file|{
                file.priority=p.priority;file
            })
        ).collect::<Vec<PackAsset>>();
        structured::merge_structured(&files)
    }).await.map_err(|e|bad(format!("Fusión interrumpida: {e}")))?
        .map_err(friendly_error)
}
fn category(path:&str)->&'static str{
    if path=="pack.mcmeta"||path=="pack.png"{"metadata"}
    else if path.contains("/textures/"){"textures"}
    else if path.contains("/sounds/")||path.ends_with("/sounds.json"){"sounds"}
    else if path.contains("/models/"){"models"}
    else if path.contains("/font/"){"fonts"}
    else if path.contains("/lang/"){"languages"}
    else if path.contains("/shaders/"){"shaders"}
    else{"other"}
}
pub async fn preview(Extension(hub):Extension<PackHub>)->Result<Json<Value>,ApiError>{
    let merged=composed(&hub).await?;
    let counts=merged.files.keys().fold(BTreeMap::<&str,usize>::new(),|mut acc,path|{
        *acc.entry(category(path)).or_default()+=1;acc
    });
    let namespaces=merged.files.keys().filter_map(|path|
        path.strip_prefix("assets/").and_then(|rest|rest.split('/').next()))
        .collect::<BTreeSet<_>>();
    let total_bytes:usize=merged.files.values().map(Vec::len).sum();
    let has_manifest=merged.files.contains_key("pack.mcmeta");
    let valid_manifest=merged.files.get("pack.mcmeta")
        .and_then(|bytes|serde_json::from_slice::<Value>(bytes).ok())
        .is_some_and(|root|root.get("pack").is_some_and(Value::is_object));
    let conflicts_count=merged.conflicts.len();
    Ok(Json(json!({
        "ready":valid_manifest,"has_manifest":has_manifest,
        "file_count":merged.files.len(),"bytes":total_bytes,
        "categories":counts,"namespaces":namespaces,
        "conflicts_count":conflicts_count,
        "conflicts":merged.conflicts.into_iter().take(MAX_PREVIEW_CONFLICTS)
            .map(|c|json!({"path":c.path,"winner":c.winner,"loser":c.loser}))
            .collect::<Vec<_>>(),
        "files":merged.files.iter().map(|(path,data)|
            json!({"path":path,"bytes":data.len(),"category":category(path)})
        ).collect::<Vec<_>>(),
        "warning":if valid_manifest{""}
            else{"Falta un pack.mcmeta válido con el objeto pack. Añadí un pack base antes de exportar"}
    })))
}

#[derive(Deserialize)]
pub struct TextureQuery {path:String}
/// Only one merged texture at a time; no HTML, SVG or arbitrary file reads.
pub async fn texture(
    Extension(hub):Extension<PackHub>,Query(query):Query<TextureQuery>
)->Result<Response,ApiError>{
    vsmedia_pack::validate_path(&query.path).map_err(friendly_error)?;
    let path=query.path.to_ascii_lowercase();
    let mime=if path.ends_with(".png"){"image/png"}
        else if path.ends_with(".jpg")||path.ends_with(".jpeg"){"image/jpeg"}
        else if path.ends_with(".webp"){"image/webp"}
        else{return Err(bad("Vista previa limitada a PNG, JPG o WebP"));};
    let merged=composed(&hub).await?;
    let data=merged.files.get(&query.path)
        .ok_or_else(||ApiError(StatusCode::NOT_FOUND,"Textura no encontrada".into()))?;
    if data.is_empty()||data.len()>4*1024*1024 {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,
            "La textura supera 4 MiB para la vista previa".into()));
    }
    let valid=match mime{
        "image/png"=>data.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg"=>data.starts_with(&[0xff,0xd8,0xff]),
        "image/webp"=>data.len()>=12 && &data[..4]==b"RIFF" && &data[8..12]==b"WEBP",
        _=>false,
    };
    if !valid{return Err(bad("Cabecera de imagen no válida"));}
    Ok(([
        (header::CONTENT_TYPE,mime),
        (header::CACHE_CONTROL,"no-store"),
        (header::X_CONTENT_TYPE_OPTIONS,"nosniff")
    ],data.clone()).into_response())
}

pub async fn export(Extension(hub):Extension<PackHub>)->Result<Response,ApiError>{
    let merged=composed(&hub).await?;
    let bytes=tokio::task::spawn_blocking(move||archive::export_zip(&merged,limits()))
        .await.map_err(|e|bad(format!("Exportación interrumpida: {e}")))?
        .map_err(friendly_error)?;
    Ok(([
        (header::CONTENT_TYPE,"application/zip"),
        (header::CACHE_CONTROL,"no-store"),
        (header::CONTENT_DISPOSITION,"attachment; filename=\"vsmedia-resourcepack.zip\""),
        (header::X_CONTENT_TYPE_OPTIONS,"nosniff")
    ],bytes).into_response())
}
#[cfg(test)]
mod tests{
    use super::*;
    use std::collections::BTreeMap;
    use vsmedia_pack::MergeOutput;
    use axum::body::to_bytes;

    fn fixture(files:&[(&str,&[u8])])->Vec<u8>{
        let mut map=BTreeMap::from([
            ("pack.mcmeta".to_owned(),
                br#"{"pack":{"pack_format":1,"description":"Studio fixture"}}"#.to_vec())
        ]);
        for (name,content) in files {map.insert((*name).to_owned(),content.to_vec());}
        archive::export_zip(&MergeOutput{files:map,conflicts:vec![]},limits()).unwrap()
    }
    fn headers()->HeaderMap{
        let mut h=HeaderMap::new();
        h.insert(header::ORIGIN,header::HeaderValue::from_static("http://127.0.0.1:3060"));
        h
    }
    #[tokio::test]
    async fn zip_import_priority_preview_export_round_trip(){
        let hub=PackHub::default();
        let source_a=fixture(&[
            ("assets/virgo/textures/gui/menu.png",b"old"),
            ("assets/virgo/sounds.json",br#"{"a":{"sounds":["a"]}}"#)
        ]);
        let source_b=fixture(&[
            ("assets/virgo/textures/gui/menu.png",b"new"),
            ("assets/virgo/sounds.json",br#"{"b":{"sounds":["b"]}}"#)
        ]);
        import(Extension(hub.clone()),Path("base".into()),headers(),Bytes::from(source_a))
            .await.unwrap();
        import(Extension(hub.clone()),Path("overlay".into()),headers(),Bytes::from(source_b))
            .await.unwrap();
        let summary=preview(Extension(hub.clone())).await.unwrap().0;
        assert_eq!(summary["ready"],true);
        assert_eq!(summary["file_count"],3);
        assert_eq!(summary["conflicts_count"],1);
        assert_eq!(summary["conflicts"][0]["winner"],"overlay");
        assert_eq!(summary["categories"]["textures"],1);
        assert!(summary["namespaces"].as_array().unwrap().iter().any(|x|x=="virgo"));
        let response=export(Extension(hub.clone())).await.unwrap();
        assert_eq!(response.status(),StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE],"application/zip");
        let zipped=to_bytes(response.into_body(),16*1024*1024).await.unwrap();
        let files=archive::import_zip(&zipped,"result",0,limits()).unwrap();
        let icon=files.iter().find(|file|file.path.ends_with("menu.png")).unwrap();
        assert_eq!(icon.data,b"new");
        let sounds=files.iter().find(|file|file.path.ends_with("sounds.json")).unwrap();
        let json:Value=serde_json::from_slice(&sounds.data).unwrap();
        assert!(json.get("a").is_some()&&json.get("b").is_some());
        let again=export(Extension(hub.clone())).await.unwrap();
        let again=to_bytes(again.into_body(),16*1024*1024).await.unwrap();
        assert_eq!(zipped,again);
    }
    #[tokio::test]
    async fn invalid_zip_external_origin_and_conflicting_priority_are_rejected(){
        let hub=PackHub::default();
        let response=import(Extension(hub.clone()),Path("bad".into()),
            headers(),Bytes::from_static(b"not a zip")).await;
        assert!(response.is_err());
        assert_eq!(list(Extension(hub.clone())).await.0["packs"].as_array().unwrap().len(),0);
        let mut blocked=headers();
        blocked.insert(header::ORIGIN,header::HeaderValue::from_static("https://attacker.example"));
        let source=fixture(&[("assets/virgo/textures/gui/a.png",b"a")]);
        assert_eq!(import(Extension(hub.clone()),Path("base".into()),
            blocked,Bytes::from(source.clone())).await.unwrap_err().0,StatusCode::FORBIDDEN);
        import(Extension(hub.clone()),Path("base".into()),
            headers(),Bytes::from(source)).await.unwrap();
        let modified=fixture(&[("assets/virgo/textures/gui/a.png",b"b")]);
        import(Extension(hub.clone()),Path("other".into()),headers(),
            Bytes::from(modified)).await.unwrap();
        priority(Extension(hub.clone()),Path("base".into()),headers(),
            Json(PriorityInput{priority:5})).await.unwrap();
        priority(Extension(hub.clone()),Path("other".into()),headers(),
            Json(PriorityInput{priority:5})).await.unwrap();
        assert!(preview(Extension(hub.clone())).await.is_err());
        assert!(export(Extension(hub)).await.is_err());
    }
    #[tokio::test]
    async fn removing_pack_frees_session_resources(){
        let hub=PackHub::default();
        let bytes=fixture(&[]);
        import(Extension(hub.clone()),Path("a".into()),headers(),
            Bytes::from(bytes)).await.unwrap();
        assert_eq!(list(Extension(hub.clone())).await.0["packs"].as_array().unwrap().len(),1);
        remove(Extension(hub.clone()),Path("a".into()),headers()).await.unwrap();
        assert_eq!(list(Extension(hub)).await.0["total_bytes"],0);
    }
    #[test]
    fn pack_id_and_category_boundaries(){
        assert!(valid_id("pack_base"));
        assert!(!valid_id("../bad"));
        assert!(!valid_id("A"));
        assert_eq!(category("assets/virgoland/textures/ui/a.png"),"textures");
        assert_eq!(category("assets/virgoland/sounds.json"),"sounds");
        assert_eq!(category("assets/virgoland/models/item/a.json"),"models");
    }
}
