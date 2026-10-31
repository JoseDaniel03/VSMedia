//! Session-local Resource Pack compositor for FullCanvas Studio.
//! Reuses the production Rust import/structured merge/deterministic export.
//! Strict in-memory quotas; no arbitrary filesystem writes or external URLs.
//! All operations are local-only. Public CDN / Folia delivery comes later.
use std::{collections::{BTreeMap,BTreeSet},sync::Arc};
use sha2::{Digest,Sha256};
use axum::{
    body::Bytes,extract::{Path,Query},Extension,http::{header,HeaderMap,StatusCode},
    response::{IntoResponse,Response},Json,
};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use tokio::sync::{Mutex,Semaphore};
use vsmedia_pack::{archive::{self,ArchiveLimits},structured,PackAsset,MergeOutput};

use crate::{ApiError,authorize_origin,bad};

// Account for multiple large GUI/audio packs without removing ZIP-bomb protections.
const MAX_ZIP_BYTES:usize=96*1024*1024;
const MAX_RAM_BYTES:usize=384*1024*1024;
const MAX_PACKS:usize=24;
const MAX_PREVIEW_CONFLICTS:usize=200;
pub(crate) fn limits()->ArchiveLimits {
    ArchiveLimits {max_files:12000,max_total_bytes:192*1024*1024,
        max_single_file_bytes:32*1024*1024}
}
/// Source packs plus explicit per-resource choices. Both are ephemeral.
#[derive(Clone)]
pub struct PackHub(
    pub(crate) Arc<Mutex<Vec<LoadedPack>>>,
    pub(crate) Arc<Mutex<BTreeMap<String,String>>>,
    // Shared CPU budget across imports, merges and manager previews.
    pub(crate) Arc<Semaphore>
);
impl Default for PackHub {
    fn default()->Self{
        Self(Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(BTreeMap::new())),
            Arc::new(Semaphore::new(2)))
    }
}
#[derive(Clone)]
pub(crate) struct LoadedPack {
    pub(crate) id:String,
    name:String,
    pub(crate) priority:i32,
    pub(crate) files:Arc<Vec<PackAsset>>,
    bytes:usize,
    pub(crate) fingerprint:String,
}
#[derive(Serialize,Debug)]
pub struct PackInfo {
    id:String,name:String,priority:i32,files:usize,bytes:usize,
    fingerprint:String
}
impl From<&LoadedPack> for PackInfo {
    fn from(pack:&LoadedPack)->Self{
        Self {id:pack.id.clone(),name:pack.name.clone(),
            priority:pack.priority,files:pack.files.len(),bytes:pack.bytes,
            fingerprint:pack.fingerprint.clone()}
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
    Json(json!({"schema":"vspack-catalog-v1",
        "resource_index":"vspack-resources-v1",
        "supports_texture_overrides":true,
        "supports_interface_catalog":true,
        "packs":packs.iter().map(PackInfo::from).collect::<Vec<_>>(),
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
            "ZIP debe ocupar entre 1 byte y 96 MiB".into()));
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
        if saved.len()>=MAX_PACKS {return Err(bad("Máximo 24 packs por sesión"));}
    }
    // The original ZIP hash remains stable when a saved recipe is reloaded
    // with the exact same archive, even when the UI generates another source ID.
    let fingerprint=format!("{:x}",Sha256::digest(&bytes));
    let source_id=id.clone();
    let permit=hub.2.clone().acquire_owned().await
        .map_err(|_|bad("Motor ZIP cerrado"))?;
    let imported=tokio::task::spawn_blocking(move||{
        let _permit=permit;
        archive::import_zip(&bytes,&source_id,0,limits())
    }).await.map_err(|e|bad(format!("Importación interrumpida: {e}")))?
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
            "El límite de 24 packs / 384 MiB de datos descomprimidos fue alcanzado".into()));
    }
    let next_priority=(saved.len() as i32)*10;
    let pack=LoadedPack{id,name:label,priority:next_priority,files:Arc::new(imported),
        bytes:used,fingerprint};
    let out=PackInfo::from(&pack);
    saved.push(pack);
    Ok(Json(out))
}
/// Download one imported source pack as a canonical, validated ZIP.
/// Since sources are held decompressed in RAM this exports a deterministic
/// equivalent, not necessarily the byte-identical ZIP that was uploaded.
pub async fn download_source(
    Extension(hub):Extension<PackHub>,Path(id):Path<String>
)->Result<Response,ApiError>{
    let pack=hub.0.lock().await.iter().find(|p|p.id==id).cloned()
        .ok_or_else(||ApiError(StatusCode::NOT_FOUND,"Pack no encontrado".into()))?;
    let zipped=tokio::task::spawn_blocking(move||{
        let files=pack.files.iter().map(|file|
            (file.path.clone(),file.data.clone())
        ).collect::<BTreeMap<String,Vec<u8>>>();
        archive::export_zip(&MergeOutput{files,conflicts:vec![]},limits())
    }).await.map_err(|e|bad(format!("Exportación de fuente interrumpida: {e}")))?
        .map_err(friendly_error)?;
    Ok(([
        (header::CONTENT_TYPE,"application/zip"),
        (header::CONTENT_DISPOSITION,"attachment; filename=\"vsmedia-source-pack.zip\""),
        (header::CACHE_CONTROL,"no-store"),
        (header::X_CONTENT_TYPE_OPTIONS,"nosniff"),
    ],zipped).into_response())
}
pub async fn source_info(
    Extension(hub):Extension<PackHub>,Path(id):Path<String>
)->Result<Json<PackInfo>,ApiError>{
    let packs=hub.0.lock().await;
    let pack=packs.iter().find(|p|p.id==id)
        .ok_or_else(||ApiError(StatusCode::NOT_FOUND,"Pack no encontrado".into()))?;
    Ok(Json(PackInfo::from(pack)))
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
    drop(packs);
    hub.1.lock().await.retain(|_,source|source!=&id);
    Ok(Json(json!({"deleted":true,"id":id})))
}
/// Clone only small Arc handles under lock. CPU-heavy structured merge/ZIP
/// compression runs on a bounded blocking thread, not a Folia/async worker.
pub(crate) async fn composed(hub:&PackHub)->Result<MergeOutput,ApiError>{
    let current=hub.0.lock().await.clone();
    let overrides=hub.1.lock().await.clone();
    if current.is_empty(){return Err(bad("Primero importá un Resource Pack ZIP"));}
    let permit=hub.2.clone().acquire_owned().await
        .map_err(|_|bad("Motor de fusión cerrado"))?;
    tokio::task::spawn_blocking(move || {
        let _permit=permit;
        let files=current.iter().flat_map(|p|
            p.files.iter().cloned().map(|mut file|{
                file.priority=if overrides.get(&file.path)==Some(&p.id){
                    1001 // higher than any user-assigned pack priority
                }else{p.priority};
                file
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


const MAX_RESOURCES_PAGE:usize=200;
const MAX_SOURCE_CHOICES:usize=256;
fn interface_kind(path:&str)->Option<&'static str>{
    let tail=path.strip_prefix("assets/")?.split_once('/')?.1;
    if tail.starts_with("textures/gui/")||tail.starts_with("textures/ui/")
        ||tail.starts_with("textures/hud/"){Some("gui_textures")}
    else if tail.starts_with("font/")||tail.starts_with("textures/font/"){Some("fonts")}
    else if tail.starts_with("atlases/"){Some("atlases")}
    else if tail.starts_with("models/item/"){Some("item_models")}
    else if tail.starts_with("lang/"){Some("translations")}
    else if tail.starts_with("sounds/ui/"){Some("ui_sounds")}
    else{None}
}
fn namespace(path:&str)->Option<&str>{
    path.strip_prefix("assets/")?.split_once('/').map(|v|v.0)
}
#[derive(Deserialize,Default)]
#[serde(deny_unknown_fields)]
pub struct ResourceQuery {
    #[serde(default)] limit:Option<usize>,
    #[serde(default)] offset:Option<usize>,
    #[serde(default)] scope:Option<String>,
    #[serde(default)] namespace:Option<String>,
    #[serde(default)] search:Option<String>,
    #[serde(default)] conflicts_only:Option<bool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverrideInput {path:String,source_id:String}
fn check_resource_query(q:&ResourceQuery)->Result<(),ApiError>{
    if q.limit.is_some_and(|n|n==0||n>MAX_RESOURCES_PAGE)||
        q.offset.is_some_and(|n|n>16_384) {
        return Err(bad("Paginación inválida: limit 1..200, offset <= 16384"));
    }
    if q.search.as_ref().is_some_and(|s|s.len()>128)||
        q.namespace.as_ref().is_some_and(|s|
            s.is_empty()||s.len()>64||
            !s.bytes().all(|c|c.is_ascii_lowercase()||c.is_ascii_digit()||c==b'_')) {
        return Err(bad("Búsqueda o namespace inválido"));
    }
    if q.scope.as_deref().is_some_and(|s|s!="all"&&s!="textures"&&s!="interfaces"){
        return Err(bad("scope debe ser all, textures o interfaces"));
    }
    Ok(())
}
async fn resource_plan(hub:&PackHub,filter:ResourceQuery)->Result<Json<Value>,ApiError>{
    check_resource_query(&filter)?;
    let packs=hub.0.lock().await.clone();
    let overrides=hub.1.lock().await.clone();
    let page=tokio::task::spawn_blocking(move||{
        let mut grouped=BTreeMap::<String,Vec<(String,String,i32,usize,String)>>::new();
        for pack in &packs{
            for asset in pack.files.iter(){
                if let Some(ns)=filter.namespace.as_deref(){
                    if namespace(&asset.path)!=Some(ns){continue;}
                }
                if let Some(term)=filter.search.as_deref(){
                    if !asset.path.to_ascii_lowercase().contains(&term.to_ascii_lowercase()){
                        continue;
                    }
                }
                let kind=interface_kind(&asset.path);
                if filter.scope.as_deref()==Some("textures") && category(&asset.path)!="textures"{
                    continue;
                }
                if filter.scope.as_deref()==Some("interfaces")&&kind.is_none(){continue;}
                grouped.entry(asset.path.clone()).or_default().push((
                    pack.id.clone(),pack.name.clone(),pack.priority,
                    asset.data.len(),format!("{:x}",Sha256::digest(&asset.data))
                ));
            }
        }
        let mut results=Vec::new();
        let mut conflicts=0usize;
        let mut blocked=0usize;
        for (path,mut providers) in grouped{
            providers.sort_by(|a,b|b.2.cmp(&a.2).then_with(||a.0.cmp(&b.0)));
            let override_id=overrides.get(&path);
            let chosen=override_id
                .and_then(|id|providers.iter().find(|c|&c.0==id))
                .unwrap_or(&providers[0]);
            let hashes=providers.iter().map(|c|&c.4).collect::<BTreeSet<_>>();
            let different=hashes.len()>1;
            let tie=different&&override_id.is_none() &&
                providers.iter().skip(1).any(|p|
                    p.2==providers[0].2 && p.4!=providers[0].4);
            if different {conflicts+=1;}
            if tie{blocked+=1;}
            if filter.conflicts_only==Some(true)&&!different{continue;}
            results.push(json!({
                "path":path,"namespace":namespace(&path),
                "category":category(&path),"interface_kind":interface_kind(&path),
                "chosen_source":if tie{Value::Null}else{json!(chosen.0)},
                "chosen_priority":chosen.2,"chosen_sha256":chosen.4,
                "bytes":chosen.3,"conflict":different,"blocked":tie,
                "override_source":override_id,
                "candidates":providers.iter().map(|p|
                    json!({"source_id":p.0,"source_name":p.1,
                        "priority":p.2,"bytes":p.3,"sha256":p.4})
                ).collect::<Vec<_>>()
            }));
        }
        let total=results.len();
        let offset=filter.offset.unwrap_or(0);
        let limit=filter.limit.unwrap_or(75);
        let items=results.into_iter().skip(offset).take(limit).collect::<Vec<_>>();
        json!({"schema":"vspack-resources-v1","scope":filter.scope.unwrap_or("all".into()),
            "total":total,"offset":offset,"limit":limit,"items":items,
            "conflicts":conflicts,"blocked":blocked,
            "overrides":overrides.len(),"session_only":true})
    }).await.map_err(|e|bad(format!("Análisis de recursos interrumpido: {e}")))?;
    Ok(Json(page))
}
/// Stable, paginated API for the future Interface Manager. Does not depend on
/// the current FullCanvas UI markup, so a separate frontend can reuse it.
pub async fn resources(
    Extension(hub):Extension<PackHub>,Query(q):Query<ResourceQuery>
)->Result<Json<Value>,ApiError>{resource_plan(&hub,q).await}
pub async fn interfaces(
    Extension(hub):Extension<PackHub>,Query(mut q):Query<ResourceQuery>
)->Result<Json<Value>,ApiError>{
    q.scope=Some("interfaces".into());
    resource_plan(&hub,q).await
}
pub async fn choose_resource(
    Extension(hub):Extension<PackHub>,
    headers:HeaderMap,
    Json(choice):Json<OverrideInput>
)->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;
    vsmedia_pack::validate_path(&choice.path).map_err(friendly_error)?;
    if !choice.path.starts_with("assets/"){
        return Err(bad("La selección manual sólo se aplica a assets/"));
    }
    let exists=hub.0.lock().await.iter().any(|pack|pack.id==choice.source_id &&
        pack.files.iter().any(|file|file.path==choice.path));
    if !exists{
        return Err(ApiError(StatusCode::NOT_FOUND,
            "La fuente elegida no proporciona ese recurso".into()));
    }
    let mut choices=hub.1.lock().await;
    if choices.len()>=MAX_SOURCE_CHOICES && !choices.contains_key(&choice.path){
        return Err(bad("Máximo 256 selecciones manuales por sesión"));
    }
    choices.insert(choice.path.clone(),choice.source_id.clone());
    Ok(Json(json!({"path":choice.path,"chosen_source":choice.source_id,
        "override":true})))
}
pub async fn clear_resource_choice(
    Extension(hub):Extension<PackHub>,
    headers:HeaderMap,Query(q):Query<TextureQuery>
)->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;
    vsmedia_pack::validate_path(&q.path).map_err(friendly_error)?;
    let removed=hub.1.lock().await.remove(&q.path).is_some();
    Ok(Json(json!({"path":q.path,"removed":removed})))
}
pub async fn resource_choices(
    Extension(hub):Extension<PackHub>
)->Json<Value>{
    let items=hub.1.lock().await.clone();
    Json(json!({"schema":"vspack-resources-v1","overrides":items}))
}

/// Single-resource download for future interface editors. Return bytes from
/// the resolved, merged pack. Force attachment + nosniff, never inline HTML.
pub async fn resource_file(
    Extension(hub):Extension<PackHub>,Query(q):Query<TextureQuery>
)->Result<Response,ApiError>{
    vsmedia_pack::validate_path(&q.path).map_err(friendly_error)?;
    let merged=composed(&hub).await?;
    let data=merged.files.get(&q.path)
        .ok_or_else(||ApiError(StatusCode::NOT_FOUND,"Recurso no encontrado".into()))?;
    if data.len()>8*1024*1024{
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,
            "Recurso supera 8 MiB".into()));
    }
    Ok(([
        (header::CONTENT_TYPE,"application/octet-stream"),
        (header::CONTENT_DISPOSITION,"attachment; filename=\"resource.bin\""),
        (header::CACHE_CONTROL,"no-store"),
        (header::X_CONTENT_TYPE_OPTIONS,"nosniff"),
    ],data.clone()).into_response())
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

/// Read-only, bounded diagnostics of the active Studio pack composition.
pub async fn diagnostics(Extension(hub):Extension<PackHub>)
    ->Result<Json<vsmedia_pack::inspect::Inspection>,ApiError>{
    let merged=composed(&hub).await?;
    let inspection=tokio::task::spawn_blocking(move||vsmedia_pack::inspect::inspect(&merged))
        .await.map_err(|e|bad(format!("Diagnóstico interrumpido: {e}")))?;
    Ok(Json(inspection))
}

pub async fn export(Extension(hub):Extension<PackHub>)->Result<Response,ApiError>{
    let merged=composed(&hub).await?;
    let permit=hub.2.clone().acquire_owned().await
        .map_err(|_|bad("Motor ZIP cerrado"))?;
    let bytes=tokio::task::spawn_blocking(move||{
        let _permit=permit;
        archive::export_zip(&merged,limits())
    }).await.map_err(|e|bad(format!("Exportación interrumpida: {e}")))?
        .map_err(friendly_error)?;
    Ok(([
        (header::CONTENT_TYPE,"application/zip"),
        (header::CACHE_CONTROL,"no-store"),
        (header::CONTENT_DISPOSITION,"attachment; filename=\"vsmedia-resourcepack.zip\""),
        (header::X_CONTENT_TYPE_OPTIONS,"nosniff")
    ],bytes).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerBundleInput {
    base_id:String,
    rules:Vec<LayerRuleInput>
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerRuleInput {
    logical_id:String,
    prefixes:Vec<String>
}
/// Generate a portable *ZIP of ordinary Resource Pack ZIPs* plus immutable
/// SHA-1/SHA-256/UUID metadata. This is not a public Minecraft download URL.
pub async fn export_layers(
    Extension(hub):Extension<PackHub>,
    headers:HeaderMap,
    Json(input):Json<LayerBundleInput>
)->Result<Response,ApiError>{
    authorize_origin(&headers)?;
    if input.rules.is_empty()||input.rules.len()>5{
        return Err(bad("Usá entre 1 y 5 reglas overlay"));
    }
    if input.rules.iter().any(|r|r.prefixes.is_empty()||r.prefixes.len()>8){
        return Err(bad("Cada overlay admite entre 1 y 8 prefijos"));
    }
    let merged=composed(&hub).await?;
    let result=tokio::task::spawn_blocking(move||{
        use std::io::{Cursor,Write};
        use sha1::Digest;
        use zip::{ZipWriter,write::SimpleFileOptions,CompressionMethod};
        use uuid::Uuid;
        let rules=input.rules.into_iter().map(|r|
            vsmedia_pack::layers::OverlayRule{
                logical_id:r.logical_id,prefixes:r.prefixes
            }
        ).collect::<Vec<_>>();
        let layers=vsmedia_pack::layers::split_into_layers(
            &merged,&input.base_id,&rules
        )?;
        let mut collected=Vec::with_capacity(layers.len());
        let mut inventory=Vec::new();
        for layer in layers {
            let bytes=archive::export_zip(&layer.pack,limits())?;
            let filename=format!("packs/{}.zip",layer.logical_id);
            let uuid=Uuid::new_v5(&Uuid::NAMESPACE_URL,
                format!("vsmedia:resource-pack:{}",layer.logical_id).as_bytes());
            inventory.push(json!({
                "logical_id":layer.logical_id,
                "uuid":uuid.to_string(),
                "sha1":format!("{:x}",sha1::Sha1::digest(&bytes)),
                "sha256":format!("{:x}",sha2::Sha256::digest(&bytes)),
                "bytes":bytes.len(),"filename":filename,
                "files":layer.pack.files.len()
            }));
            collected.push((filename,bytes));
        }
        let mut zip=ZipWriter::new(Cursor::new(Vec::new()));
        let stored=SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
            .last_modified_time(zip::DateTime::default());
        let deflate=SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        let manifest=json!({
            "format":"vsmedia-resource-pack-layer-bundle-v1",
            "delivery":"not_published",
            "ordered_base_first":true,
            "layers":inventory
        });
        zip.start_file("manifest.json",deflate)
            .map_err(|e|vsmedia_api::MediaError::InvalidInput(e.to_string()))?;
        zip.write_all(&serde_json::to_vec(&manifest)
            .map_err(|e|vsmedia_api::MediaError::InvalidInput(e.to_string()))?)
            .map_err(|e|vsmedia_api::MediaError::Io(e.to_string()))?;
        for (filename,bytes) in collected{
            zip.start_file(filename,stored)
                .map_err(|e|vsmedia_api::MediaError::InvalidInput(e.to_string()))?;
            zip.write_all(&bytes)
                .map_err(|e|vsmedia_api::MediaError::Io(e.to_string()))?;
        }
        let bytes=zip.finish()
            .map_err(|e|vsmedia_api::MediaError::InvalidInput(e.to_string()))?
            .into_inner();
        if bytes.len()>MAX_RAM_BYTES{
            return Err(vsmedia_api::MediaError::InvalidInput(
                "Bundle de capas supera el límite de 64 MiB".into()));
        }
        Ok::<_,vsmedia_api::MediaError>(bytes)
    }).await.map_err(|e|bad(format!("Compilación de capas interrumpida: {e}")))?
        .map_err(friendly_error)?;
    Ok(([
        (header::CONTENT_TYPE,"application/zip"),
        (header::CACHE_CONTROL,"no-store"),
        (header::CONTENT_DISPOSITION,"attachment; filename=\"vsmedia-layer-bundle.zip\""),
        (header::X_CONTENT_TYPE_OPTIONS,"nosniff")
    ],result).into_response())
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
    #[tokio::test]
    async fn preview_only_serves_image_signatures_and_bounded_paths(){
        let hub=PackHub::default();
        let image=b"\x89PNG\r\n\x1a\nfake.png.bytes";
        let zip=fixture(&[
            ("assets/virgo/textures/gui/icon.png",image),
            ("assets/virgo/models/item/test.json",br#"{"parent":"item/generated"}"#),
        ]);
        import(Extension(hub.clone()),Path("base".into()),headers(),
            Bytes::from(zip)).await.unwrap();
        let path="assets/virgo/textures/gui/icon.png".to_owned();
        let response=texture(Extension(hub.clone()),Query(TextureQuery{path}))
            .await.unwrap();
        assert_eq!(response.status(),StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE],"image/png");
        let bytes=to_bytes(response.into_body(),2*1024*1024).await.unwrap();
        assert_eq!(bytes.as_ref(),image);
        assert!(texture(Extension(hub.clone()),Query(TextureQuery{
            path:"assets/virgo/models/item/test.json".into(),
        })).await.is_err());
        assert!(texture(Extension(hub.clone()),Query(TextureQuery{
            path:"assets/virgo/../private.png".into(),
        })).await.is_err());
        assert!(texture(Extension(hub),Query(TextureQuery{
            path:"assets/virgo/textures/gui/missing.webp".into(),
        })).await.is_err());
    }
    #[tokio::test]
    async fn layer_bundle_contains_independent_minecraft_packs_and_hash_manifest(){
        use std::io::{Cursor,Read};
        let hub=PackHub::default();
        let zip=fixture(&[
            ("assets/virgo/textures/base.png",b"base_image"),
            ("assets/virgo/textures/ui/button.png",b"ui_image"),
        ]);
        import(Extension(hub.clone()),Path("base".into()),headers(),
            Bytes::from(zip)).await.unwrap();
        let input=LayerBundleInput{
            base_id:"virgoland".into(),
            rules:vec![LayerRuleInput{
                logical_id:"ui".into(),
                prefixes:vec!["assets/virgo/textures/ui/".into()],
            }]
        };
        let response=export_layers(Extension(hub.clone()),headers(),Json(input))
            .await.unwrap();
        assert_eq!(response.status(),StatusCode::OK);
        let bundle=to_bytes(response.into_body(),64*1024*1024).await.unwrap();
        let mut outer=zip::ZipArchive::new(Cursor::new(bundle)).unwrap();
        assert_eq!(outer.len(),3);
        let mut manifest_text=String::new();
        outer.by_name("manifest.json").unwrap()
            .read_to_string(&mut manifest_text).unwrap();
        let manifest:Value=serde_json::from_str(&manifest_text).unwrap();
        assert_eq!(manifest["layers"].as_array().unwrap().len(),2);
        assert_eq!(manifest["layers"][0]["logical_id"],"virgoland");
        assert_eq!(manifest["layers"][1]["logical_id"],"ui");
        assert_eq!(manifest["layers"][0]["sha1"].as_str().unwrap().len(),40);
        assert_eq!(manifest["layers"][1]["sha256"].as_str().unwrap().len(),64);
        let mut base_bytes=Vec::new();
        outer.by_name("packs/virgoland.zip").unwrap()
            .read_to_end(&mut base_bytes).unwrap();
        let mut ui_bytes=Vec::new();
        outer.by_name("packs/ui.zip").unwrap()
            .read_to_end(&mut ui_bytes).unwrap();
        let base=archive::import_zip(&base_bytes,"base",0,limits()).unwrap();
        let ui=archive::import_zip(&ui_bytes,"ui",0,limits()).unwrap();
        assert!(base.iter().any(|r|r.path=="assets/virgo/textures/base.png"));
        assert!(!base.iter().any(|r|r.path=="assets/virgo/textures/ui/button.png"));
        assert!(ui.iter().any(|r|r.path=="assets/virgo/textures/ui/button.png"));
        assert!(base.iter().any(|r|r.path=="pack.mcmeta"));
        assert!(ui.iter().any(|r|r.path=="pack.mcmeta"));
        let invalid=LayerBundleInput{
            base_id:"virgoland".into(),
            rules:vec![LayerRuleInput{logical_id:"ui".into(),
                prefixes:vec!["../unsafe/".into()]}],
        };
        assert!(export_layers(Extension(hub),headers(),Json(invalid)).await.is_err());
    }

    #[tokio::test]
    async fn manually_selects_texture_winner_and_resolves_equal_priority_conflict(){
        let hub=PackHub::default();
        let path="assets/virgo/textures/gui/hotbar.png";
        let first=fixture(&[(path,b"VIRGO_A")]);
        let second=fixture(&[(path,b"VIRGO_B")]);
        import(Extension(hub.clone()),Path("first".into()),headers(),
            Bytes::from(first)).await.unwrap();
        import(Extension(hub.clone()),Path("second".into()),headers(),
            Bytes::from(second)).await.unwrap();
        for id in ["first","second"]{
            priority(Extension(hub.clone()),Path(id.into()),headers(),
                Json(PriorityInput{priority:10})).await.unwrap();
        }
        // A tie remains inspectable even when exporting the pack is blocked.
        let initial=resource_plan(&hub,ResourceQuery{
            scope:Some("textures".into()),conflicts_only:Some(true),
            ..Default::default()
        }).await.unwrap().0;
        assert_eq!(initial["schema"],"vspack-resources-v1");
        assert_eq!(initial["total"],1);
        assert_eq!(initial["blocked"],1);
        assert_eq!(initial["items"][0]["chosen_source"],Value::Null);
        assert_eq!(initial["items"][0]["candidates"].as_array().unwrap().len(),2);
        assert!(composed(&hub).await.is_err());

        choose_resource(Extension(hub.clone()),headers(),Json(OverrideInput{
            path:path.into(),source_id:"first".into()
        })).await.unwrap();
        let selected=resource_plan(&hub,ResourceQuery::default()).await.unwrap().0;
        let texture=selected["items"].as_array().unwrap().iter().find(|item|
            item["path"]==path).unwrap();
        assert_eq!(texture["chosen_source"],"first");
        assert_eq!(texture["override_source"],"first");
        assert_eq!(texture["blocked"],false);
        assert_eq!(composed(&hub).await.unwrap().files[path],b"VIRGO_A");
        let response=resource_file(Extension(hub.clone()),Query(TextureQuery{
            path:path.into()
        })).await.unwrap();
        assert_eq!(response.headers()[header::CONTENT_TYPE],"application/octet-stream");
        assert_eq!(response.headers()[header::X_CONTENT_TYPE_OPTIONS],"nosniff");
        let content=to_bytes(response.into_body(),4096).await.unwrap();
        assert_eq!(content.as_ref(),b"VIRGO_A");

        clear_resource_choice(Extension(hub.clone()),headers(),
            Query(TextureQuery{path:path.into()})).await.unwrap();
        assert!(composed(&hub).await.is_err());
        assert!(choose_resource(Extension(hub.clone()),headers(),Json(OverrideInput{
            path:path.into(),source_id:"third_missing".into()
        })).await.is_err());
        choose_resource(Extension(hub.clone()),headers(),Json(OverrideInput{
            path:path.into(),source_id:"first".into()
        })).await.unwrap();
        remove(Extension(hub.clone()),Path("first".into()),headers()).await.unwrap();
        assert!(hub.1.lock().await.is_empty());
        assert_eq!(composed(&hub).await.unwrap().files[path],b"VIRGO_B");
    }

    #[tokio::test]
    async fn interface_catalog_filters_groups_and_paginates_without_audio_assets(){
        let hub=PackHub::default();
        let archive=fixture(&[
            ("assets/virgo/textures/gui/menu.png",b"gui"),
            ("assets/virgo/textures/hud/hotbar.png",b"hud"),
            ("assets/virgo/font/default.json",br#"{"providers":[]}"#),
            ("assets/virgo/atlases/gui.json",br#"{"sources":[]}"#),
            ("assets/virgo/models/item/icon.json",br#"{"parent":"item/generated"}"#),
            ("assets/virgo/lang/es_ar.json",br#"{"item.example":"Hola"}"#),
            ("assets/virgo/textures/block/rock.png",b"block"),
            ("assets/other/textures/gui/foreign.png",b"other")
        ]);
        import(Extension(hub.clone()),Path("ui".into()),headers(),
            Bytes::from(archive)).await.unwrap();
        let result=interfaces(Extension(hub.clone()),Query(ResourceQuery{
            namespace:Some("virgo".into()),limit:Some(3),
            ..Default::default()
        })).await.unwrap().0;
        assert_eq!(result["scope"],"interfaces");
        assert_eq!(result["total"],6);
        assert_eq!(result["items"].as_array().unwrap().len(),3);
        let result=interfaces(Extension(hub.clone()),Query(ResourceQuery{
            namespace:Some("virgo".into()),
            search:Some("hotbar".into()),..Default::default()
        })).await.unwrap().0;
        assert_eq!(result["total"],1);
        assert_eq!(result["items"][0]["interface_kind"],"gui_textures");
        let invalid=resource_plan(&hub,ResourceQuery{
            scope:Some("datapacks".into()),..Default::default()
        }).await;
        assert!(invalid.is_err());
        let rejected=resource_file(Extension(hub),Query(TextureQuery{
            path:"assets/virgo/../../secret.txt".into()
        })).await;
        assert!(rejected.is_err());
    }

    #[tokio::test]
    async fn interface_manager_can_retrieve_source_metadata_and_canonical_zip(){
        let hub=PackHub::default();
        let path="assets/virgo/textures/gui/icon.png";
        let original=fixture(&[(path,b"\x89PNG\r\n\x1a\nimage")]);
        import(Extension(hub.clone()),Path("gui".into()),headers(),
            Bytes::from(original)).await.unwrap();
        let info=source_info(Extension(hub.clone()),Path("gui".into())).await.unwrap().0;
        assert_eq!(info.id,"gui");
        assert_eq!(info.files,2);
        let resp=download_source(Extension(hub.clone()),Path("gui".into()))
            .await.unwrap();
        assert_eq!(resp.status(),StatusCode::OK);
        assert_eq!(resp.headers()[header::CONTENT_TYPE],"application/zip");
        let body=to_bytes(resp.into_body(),16*1024*1024).await.unwrap();
        let decoded=archive::import_zip(&body,"export",0,limits()).unwrap();
        assert!(decoded.iter().any(|item|item.path==path &&
            item.data.starts_with(b"\x89PNG\r\n\x1a\n")));
        assert!(source_info(Extension(hub.clone()),Path("missing".into()))
            .await.is_err());
        assert!(download_source(Extension(hub),Path("missing".into()))
            .await.is_err());
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
