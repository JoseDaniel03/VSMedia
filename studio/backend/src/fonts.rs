//! Persistent font assets in the existing SQLite/CAS. The browser never writes pack paths.
use axum::{extract::{State,Query},Extension,http::{header,HeaderMap,StatusCode},response::{IntoResponse,Response},Json};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use base64::{Engine,engine::general_purpose::STANDARD};
use sha2::{Digest,Sha256};
use vsmedia_api::{AssetKey,AssetKind,AssetRepository};
use vsmedia_pack::MergeOutput;
use crate::{Store,ApiError,bad,authorize_origin,packs::{self,PackHub}};

const MAX_FONT:usize=8*1024*1024;
#[derive(Deserialize,Serialize,Clone)]
#[serde(deny_unknown_fields)]
pub struct FontImport { pub family:String,pub source:String,pub data:String,#[serde(default)]pub license:Option<String> }
fn checked(input:&FontImport)->Result<Vec<u8>,ApiError>{
    if input.family.is_empty()||input.family.len()>120||input.family.chars().any(char::is_control)||input.data.len()>12*1024*1024
        ||input.license.as_ref().is_some_and(|text|text.len()>65536)||input.source.len()>2048 {return Err(bad("Datos de fuente inválidos"));}
    let bytes=STANDARD.decode(&input.data).map_err(|_|bad("Fuente base64 inválida"))?;
    if bytes.len()>MAX_FONT||bytes.len()<12 {return Err(bad("Fuente fuera de cuota (8 MiB)"));}
    ttf_parser::Face::parse(&bytes,0).map_err(|_|bad("Se necesita una fuente TrueType/OpenType válida; WOFF2 no es un resource pack de Minecraft"))?;
    if !bytes.starts_with(&[0,1,0,0])&&!bytes.starts_with(b"OTTO"){return Err(bad("Contenedor de fuente no soportado"));}
    Ok(bytes)
}
fn key(id:&str)->Result<AssetKey,ApiError>{AssetKey::new("studio",format!("fonts/{id}.json")).map_err(bad)}
fn id(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
fn saved(store:&Store)->Result<Vec<(String,FontImport)>,ApiError>{
    store.list_assets("studio","fonts/",256).map_err(bad)?.into_iter().map(|record|{
        let doc:FontImport=serde_json::from_slice(&store.read(&record.key).map_err(bad)?).map_err(bad)?;
        let id=record.key.name.trim_start_matches("fonts/").trim_end_matches(".json").to_string();Ok((id,doc))
    }).collect()
}
pub async fn import(State(store):State<Store>,headers:HeaderMap,Json(input):Json<FontImport>)->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;let bytes=checked(&input)?;let id=id(&bytes);
    if store.list_assets("studio","fonts/",256).map_err(bad)?.len()>=256&&store.metadata(&key(&id)?).is_err(){return Err(bad("Máximo 256 fuentes persistentes"));}
    let document=serde_json::to_vec(&input).map_err(bad)?;
    let old=store.metadata(&key(&id)?).map(|a|a.size).unwrap_or(0);
    let used:u64=store.list_assets("studio","fonts/",256).map_err(bad)?.iter().map(|a|a.size).sum();
    if used.saturating_sub(old)+document.len() as u64>96*1024*1024{return Err(bad("Cuota de fuentes persistentes: 96 MiB"));}
    store.publish(key(&id)?,AssetKind::Font,&document).map_err(bad)?;
    Ok(Json(json!({"id":format!("vsmedia:imported/{id}"),"family":input.family,"sha256":id,"persistent":true,"previewAvailable":true})))
}
pub(crate) fn add_to_pack(store:&Store,pack:&mut MergeOutput)->Result<(),ApiError>{
    for (id,doc) in saved(store)? {
        let bytes=checked(&doc)?;
        pack.files.insert(format!("assets/vsmedia/font/imported/{id}.ttf"),bytes);
        pack.files.insert(format!("assets/vsmedia/font/imported/{id}.json"),serde_json::to_vec(&json!({"providers":[
            {"type":"ttf","file":format!("vsmedia:imported/{id}.ttf"),"shift":[0,0],"size":11,"oversample":2},
            {"type":"reference","id":"minecraft:default"}
        ]})).map_err(bad)?);
        if let Some(license)=&doc.license {pack.files.insert(format!("assets/vsmedia/licenses/{id}.txt"),license.as_bytes().to_vec());}
        pack.files.insert(format!("assets/vsmedia/font_sources/{id}.json"),serde_json::to_vec(&json!({"family":doc.family,"source":doc.source,"sha256":id})).map_err(bad)?);
    }Ok(())
}
fn resource_id(path:&str)->Option<String>{let path=path.strip_prefix("assets/")?;let(ns,rest)=path.split_once("/font/")?;let name=rest.strip_suffix(".json")?;if name.ends_with(".source"){return None;}Some(format!("{ns}:{name}"))}
pub async fn catalog(State(store):State<Store>,Extension(hub):Extension<PackHub>)->Result<Json<Value>,ApiError>{
    let pack=composed(&store,&hub).await?;
    let mut out=std::collections::BTreeMap::new();
    for name in ["default","uniform","alt"] {let id=format!("minecraft:{name}");out.insert(id.clone(),json!({"id":id,"family":format!("Minecraft {name}"),"source":"minecraft","previewAvailable":false}));}
    for (path,bytes) in &pack.files {
        if let Some(id)=resource_id(path){
            let preview=font_file(&pack,&id).is_some();let doc:Value=serde_json::from_slice(bytes).map_err(bad)?;
            if doc.get("providers").is_some(){out.insert(id.clone(),json!({"id":id,"family":id,"source":"pack","previewAvailable":preview}));}
        }
    }
    for (id,doc) in saved(&store)?{let key=format!("vsmedia:imported/{id}");out.insert(key.clone(),json!({"id":key,"family":doc.family,"source":doc.source,"previewAvailable":true,"persistent":true}));}
    Ok(Json(json!({"schema":"vsmedia-fonts-v1","fonts":out.into_values().collect::<Vec<_>>()})))
}
fn font_file<'a>(pack:&'a MergeOutput,id:&str)->Option<&'a Vec<u8>> {
    let(ns,name)=id.split_once(':')?;if id.contains("..")||!id.bytes().all(|b|b.is_ascii_alphanumeric()||b"_./:-".contains(&b)){return None;}
    let doc:Value=serde_json::from_slice(pack.files.get(&format!("assets/{ns}/font/{name}.json"))?).ok()?;
    for provider in doc.get("providers")?.as_array()? {
        if provider.get("type")?.as_str()?=="ttf"{
            let file=provider.get("file")?.as_str()?;let(fns,path)=file.split_once(':')?;
            if let Some(bytes)=pack.files.get(&format!("assets/{fns}/font/{path}")){return Some(bytes);}
        }
    }None
}
#[derive(Deserialize)]pub struct FontQuery{font:String}
pub async fn file(State(store):State<Store>,Extension(hub):Extension<PackHub>,Query(q):Query<FontQuery>)->Result<Response,ApiError>{
    let pack=composed(&store,&hub).await?;
    let bytes=font_file(&pack,&q.font).ok_or_else(||ApiError(StatusCode::NOT_FOUND,"La fuente no tiene preview TTF".into()))?;
    Ok(([(header::CONTENT_TYPE,"font/ttf"),(header::X_CONTENT_TYPE_OPTIONS,"nosniff")],bytes.clone()).into_response())
}
#[cfg(test)]mod tests{use super::*;
    #[test]fn rejects_fake_font_and_woff2(){for bytes in [b"not a font".to_vec(),b"wOF2abcdefgh".to_vec()]{assert!(checked(&FontImport{family:"Test".into(),source:"upload".into(),data:STANDARD.encode(bytes),license:None}).is_err());}}
    #[test]fn preserves_namespaces_and_ignores_metadata(){assert_eq!(resource_id("assets/demo/font/title.json"),Some("demo:title".into()));assert_eq!(resource_id("assets/demo/font/title.source.json"),None);}
}

async fn composed(store:&Store,hub:&PackHub)->Result<MergeOutput,ApiError>{
    let mut pack=if hub.0.lock().await.is_empty(){
        let mut files=std::collections::BTreeMap::new();
        files.insert("pack.mcmeta".into(),serde_json::to_vec(&json!({"pack":{"description":"VSMedia cinematic fonts","min_format":[97,1],"max_format":[97,1]}})).map_err(bad)?);
        MergeOutput{files,conflicts:vec![]}
    }else{packs::composed(hub).await?};
    add_to_pack(store,&mut pack)?;
    crate::published_sounds::add_to_pack(store,&mut pack)?;
    Ok(pack)
}
pub async fn export(State(store):State<Store>,Extension(hub):Extension<PackHub>)->Result<Response,ApiError>{
    let pack=composed(&store,&hub).await?;
    let bytes=tokio::task::spawn_blocking(move||vsmedia_pack::archive::export_zip(&pack,packs::limits()))
        .await.map_err(bad)?.map_err(bad)?;
    Ok(([(header::CONTENT_TYPE,"application/zip"),(header::CONTENT_DISPOSITION,"attachment; filename=vsmedia-resourcepack.zip")],bytes).into_response())
}
