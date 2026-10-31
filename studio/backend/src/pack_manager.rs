//! Stateless VSPack Studio manager API for a future React/Vue/Leptos UI.
//! Recipes use original ZIP SHA-256 fingerprints rather than ephemeral DOM IDs.
//! None of the operations change imported archives or the active session.
//! Project persistence is deliberately portable JSON until an authenticated
//! CAS-backed workspace service is installed.
use std::{
    collections::{BTreeMap,BTreeSet},
    io::{Cursor,Write},
};
use axum::{
    body::Bytes, Extension,Json,
    http::{header,HeaderMap,StatusCode},
    response::{IntoResponse,Response},
};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use sha1::Digest as _;
use sha2::Sha256;
use uuid::Uuid;
use vsmedia_pack::{
    archive,structured,split,
    PackAsset,MergeOutput,validate_path,
};
use zip::{ZipWriter,CompressionMethod,write::SimpleFileOptions};
use crate::{ApiError,bad,authorize_origin,packs::{PackHub,LoadedPack,limits}};

const SCHEMA:&str="vspack-recipe-v1";
const MAX_RECIPE_BYTES:usize=64*1024;
const MAX_SOURCES:usize=24;
const MAX_CHOICES:usize=512;
const MAX_EXCLUSIONS:usize=8192;
const MAX_BUNDLE_BYTES:usize=384*1024*1024;

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeSource {
    /// SHA-256 of the original uploaded ZIP, retained across session reloads.
    pub fingerprint:String,
    pub enabled:bool,
    pub priority:i32,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackRecipe {
    pub schema:String,
    pub name:String,
    pub sources:Vec<RecipeSource>,
    /// Resource path -> original source ZIP fingerprint.
    #[serde(default)]
    pub choices:BTreeMap<String,String>,
    /// Exact asset paths to omit from the result.
    #[serde(default)]
    pub exclude:Vec<String>,
}
fn is_hash(s:&str)->bool{
    s.len()==64 && s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))
}
fn validate(recipe:&PackRecipe,loaded:&[LoadedPack])->Result<(),ApiError>{
    if recipe.schema!=SCHEMA{return Err(bad("Schema de receta no compatible"));}
    if recipe.name.trim().is_empty()||recipe.name.chars().count()>80
        ||recipe.name.chars().any(char::is_control){
        return Err(bad("Nombre de proyecto inválido"));
    }
    if recipe.sources.is_empty()||recipe.sources.len()>MAX_SOURCES {
        return Err(bad("La receta necesita de 1 a 24 fuentes"));
    }
    let mut unique=BTreeSet::new();
    for source in &recipe.sources {
        if !is_hash(&source.fingerprint)||!unique.insert(&source.fingerprint){
            return Err(bad("Huella de ZIP duplicada o inválida"));
        }
        if !(-1000..=1000).contains(&source.priority){
            return Err(bad("Prioridad fuera de -1000..1000"));
        }
        if !loaded.iter().any(|p|p.fingerprint==source.fingerprint){
            return Err(bad(format!(
                "Falta volver a importar el pack SHA-256 {}",
                &source.fingerprint[..12])));
        }
    }
    if !recipe.sources.iter().any(|p|p.enabled){
        return Err(bad("Activá al menos una fuente"));
    }
    if recipe.choices.len()>MAX_CHOICES||recipe.exclude.len()>MAX_EXCLUSIONS{
        return Err(bad("La receta excede el límite de decisiones o exclusiones"));
    }
    let enabled=recipe.sources.iter().filter(|source|source.enabled)
        .map(|source|source.fingerprint.as_str()).collect::<BTreeSet<_>>();
    let mut exclude=BTreeSet::new();
    for path in &recipe.exclude {
        validate_path(path).map_err(bad)?;
        if path=="pack.mcmeta"||!exclude.insert(path){
            return Err(bad("No se puede excluir pack.mcmeta ni repetir exclusiones"));
        }
    }
    for (path,fingerprint) in &recipe.choices {
        validate_path(path).map_err(bad)?;
        if !path.starts_with("assets/")||!is_hash(fingerprint) {
            return Err(bad("Elección manual de recurso inválida"));
        }
        if !enabled.contains(fingerprint.as_str()){
            return Err(bad("La fuente de la elección está desactivada"));
        }
        if exclude.contains(path) {
            return Err(bad("No se puede elegir un recurso excluido"));
        }
        if !loaded.iter().any(|p|p.fingerprint==*fingerprint
            && p.files.iter().any(|file|&file.path==path)){
            return Err(bad("La elección no pertenece a la fuente seleccionada"));
        }
    }
    Ok(())
}
fn merge_recipe(recipe:PackRecipe,loaded:Vec<LoadedPack>)
    ->Result<MergeOutput,ApiError>
{
    validate(&recipe,&loaded)?;
    let rules=recipe.sources.iter()
        .filter(|rule|rule.enabled)
        .map(|rule|(rule.fingerprint.as_str(),rule.priority))
        .collect::<BTreeMap<_,_>>();
    let excluded=recipe.exclude.into_iter().collect::<BTreeSet<_>>();
    let mut assets=Vec::<PackAsset>::new();
    let mut total=0usize;
    for pack in loaded {
        let Some(&priority)=rules.get(pack.fingerprint.as_str()) else{continue;};
        for file in pack.files.iter() {
            if excluded.contains(&file.path){continue;}
            total=total.saturating_add(file.data.len());
            if total>384*1024*1024 {
                return Err(bad("La receta supera el límite de datos del gestor"));
            }
            let mut asset=file.clone();
            asset.priority=if recipe.choices.get(&asset.path)==Some(&pack.fingerprint){
                1001
            }else{priority};
            assets.push(asset);
        }
    }
    structured::merge_structured(&assets).map_err(bad)
}
fn stats(output:&MergeOutput)->Value{
    let mut categories=BTreeMap::<String,Value>::new();
    for group in split::GROUPS {
        let count=output.files.keys().filter(|path|path.as_str()!="pack.mcmeta"
            && split::group_for(path)==*group).count();
        if count!=0{categories.insert((*group).into(),json!(count));}
    }
    let warning=output.files.keys().any(|p|p.ends_with(".png.mcmeta") &&
        !output.files.contains_key(p.trim_end_matches(".mcmeta")));
    json!({
        "schema":"vspack-manager-plan-v1",
        "files":output.files.len(),
        "bytes":output.files.values().map(Vec::len).sum::<usize>(),
        "conflicts":output.conflicts.len(),
        "groups":categories,
        "needs_dependency_review":true,
        "orphan_texture_metadata":warning,
        "preview_limit":"source_zip_not_published"
    })
}
async fn from_recipe(hub:PackHub,recipe:PackRecipe)
    ->Result<MergeOutput,ApiError>
{
    let loaded=hub.0.lock().await.clone();
    tokio::task::spawn_blocking(move ||merge_recipe(recipe,loaded))
        .await.map_err(|e|bad(format!("No se completó la fusión: {e}")))?
}

pub async fn capabilities()->Json<Value>{
    Json(json!({
        "schema":"vspack-manager-capabilities-v1",
        "recipe_schema":SCHEMA,
        "source_matching":"original_zip_sha256",
        "profiles":"portable_json",
        "profiles_persisted_on_server":false,
        "split_groups":split::GROUPS,
        "resource_priority":true,
        "resource_choices":true,
        "excluded_paths":true,
        "split_preserves_png_mcmeta":true,
        "split_may_have_cross_category_references":true,
        "minecraft_delivery":false,
        "client_installer_connected":false,
        "max_sources":MAX_SOURCES,
        "max_choices":MAX_CHOICES,
        "max_exclusions":MAX_EXCLUSIONS,
        "max_recipe_bytes":MAX_RECIPE_BYTES
    }))
}

/// A portable recipe for the *current* imported sources; save this JSON to
/// disk from the new UI, then re-import the exact ZIPs to restore a workspace.
pub async fn draft(Extension(hub):Extension<PackHub>)->Json<Value>{
    let loaded=hub.0.lock().await.clone();
    let current=hub.1.lock().await.clone();
    let sources=loaded.iter().map(|p|RecipeSource{
        fingerprint:p.fingerprint.clone(),enabled:true,priority:p.priority,
    }).collect::<Vec<_>>();
    let choices=current.into_iter().filter_map(|(path,id)|loaded.iter()
        .find(|pack|pack.id==id)
        .map(|pack|(path,pack.fingerprint.clone()))
    ).collect::<BTreeMap<_,_>>();
    Json(json!(PackRecipe{
        schema:SCHEMA.into(),name:"Virgoland Studio".into(),
        sources,choices,exclude:vec![]
    }))
}

pub async fn plan(Extension(hub):Extension<PackHub>,
    headers:HeaderMap,body:Bytes)->Result<Json<Value>,ApiError>
{
    authorize_origin(&headers)?;
    let recipe=decode(&body)?;
    let merged=from_recipe(hub,recipe).await?;
    Ok(Json(stats(&merged)))
}
fn decode(bytes:&Bytes)->Result<PackRecipe,ApiError>{
    if bytes.len()>MAX_RECIPE_BYTES {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,"Receta JSON supera 64 KiB".into()));
    }
    serde_json::from_slice(bytes).map_err(|e|bad(format!("Receta inválida: {e}")))
}
pub async fn export(Extension(hub):Extension<PackHub>,
    headers:HeaderMap,body:Bytes)->Result<Response,ApiError>
{
    authorize_origin(&headers)?;
    let recipe=decode(&body)?;
    let merged=from_recipe(hub,recipe).await?;
    let bytes=tokio::task::spawn_blocking(move||archive::export_zip(&merged,limits()))
        .await.map_err(|e|bad(format!("Exportación interrumpida: {e}")))?
        .map_err(bad)?;
    Ok(zip_response(bytes,"vsmedia-recipe-merged.zip"))
}
fn zip_response(bytes:Vec<u8>,name:&str)->Response {
    ([
        (header::CONTENT_TYPE,"application/zip".to_owned()),
        (header::CONTENT_DISPOSITION,format!("attachment; filename=\"{name}\"")),
        (header::CACHE_CONTROL,"no-store".to_owned()),
        (header::X_CONTENT_TYPE_OPTIONS,"nosniff".to_owned())
    ],bytes).into_response()
}
pub async fn split_plan(Extension(hub):Extension<PackHub>,
    headers:HeaderMap,body:Bytes)->Result<Json<Value>,ApiError>
{
    authorize_origin(&headers)?;
    let recipe=decode(&body)?;
    let merged=from_recipe(hub,recipe).await?;
    let category=split::split_categories(&merged).map_err(bad)?;
    let groups=category.iter().map(|(name,group)|json!({
        "id":name,"files":group.files.len(),"bytes":group.files.values()
            .map(Vec::len).sum::<usize>()
    })).collect::<Vec<_>>();
    Ok(Json(json!({
        "schema":"vspack-split-plan-v1","groups":groups,
        "original_files":merged.files.len(),
        "order":"dependency_review_required",
        "warning":"Los JSON pueden referenciar recursos de otras categorías; probá los subpacks juntos antes de distribuirlos.",
        "source_archives_untouched":true
    })))
}
pub async fn split_export(Extension(hub):Extension<PackHub>,
    headers:HeaderMap,body:Bytes)->Result<Response,ApiError>
{
    authorize_origin(&headers)?;
    let recipe=decode(&body)?;
    let merged=from_recipe(hub,recipe).await?;
    let bytes=tokio::task::spawn_blocking(move||{
        let groups=split::split_categories(&merged).map_err(bad)?;
        let mut zipped=Vec::new();
        let mut manifest=Vec::<Value>::new();
        for (name,pack) in groups {
            let bytes=archive::export_zip(&pack,limits()).map_err(bad)?;
            let fingerprint=format!("{:x}",Sha256::digest(&bytes));
            manifest.push(json!({
                "name":name,
                "filename":format!("packs/{name}.zip"),
                "sha1":format!("{:x}",sha1::Sha1::digest(&bytes)),
                "sha256":fingerprint,
                "uuid":Uuid::new_v5(&Uuid::NAMESPACE_URL,
                    format!("vsmedia:split-category:{name}").as_bytes()).to_string(),
                "files":pack.files.len(),
                "bytes":bytes.len()
            }));
            zipped.push((format!("packs/{name}.zip"),bytes));
        }
        let cursor=Cursor::new(Vec::new());
        let mut output=ZipWriter::new(cursor);
        let stored=SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .last_modified_time(zip::DateTime::default());
        let deflated=SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        output.start_file("manifest.json",deflated)
            .map_err(|e|bad(e.to_string()))?;
        let description=json!({
            "schema":"vspack-category-bundle-v1",
            "distributable_as_single_minecraft_zip":false,
            "review_cross_category_dependencies":true,
            "packs":manifest
        });
        output.write_all(&serde_json::to_vec(&description).map_err(bad)?).map_err(bad)?;
        for (name,bytes) in zipped {
            output.start_file(name,stored).map_err(bad)?;
            output.write_all(&bytes).map_err(bad)?;
        }
        let result=output.finish().map_err(bad)?.into_inner();
        if result.len()>MAX_BUNDLE_BYTES {
            return Err(bad("El bundle de categorías supera 384 MiB"));
        }
        Ok::<_,ApiError>(result)
    }).await.map_err(|e|bad(format!("Separación interrumpida: {e}")))??;
    Ok(zip_response(bytes,"vsmedia-categories.zip"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs;
    #[test]
    fn recipe_schema_and_hash_validation(){
        assert!(is_hash(&"a".repeat(64)));
        assert!(!is_hash(&"g".repeat(64)));
        let empty=PackRecipe{schema:SCHEMA.into(),name:"demo".into(),
            sources:vec![],choices:BTreeMap::new(),exclude:vec![]};
        assert!(validate(&empty,&[]).is_err());
    }
    #[test]
    fn splits_compatibly_with_source_zip_limits(){
        assert!(packs::limits().max_files>=12000);
        assert_eq!(split::group_for("assets/minecraft/textures/gui/container.png.mcmeta"),"interface");
    }
}
