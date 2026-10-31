//! Persist local VSPack project recipes without persisting third-party ZIPs.
//! Files are bounded, atomically replaced and restricted to safe project IDs.
//! This is single-user storage for the loopback-only Studio bridge, NOT an
//! authenticated workspace service or public file repository.
use std::{
    fs::{self,OpenOptions},
    io::Write,
    path::{Path as FsPath,PathBuf},
    sync::{Arc,Mutex},
    time::{SystemTime,UNIX_EPOCH},
};
use axum::{
    body::Bytes,extract::{Path,Query},Extension,Json,
    http::{HeaderMap,StatusCode},
};
use serde::{Serialize,Deserialize};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use crate::{
    bad,authorize_origin,ApiError,
    pack_manager::{PackRecipe,validate_recipe_structure},
    packs::PackHub,
};

const MAX_PROJECTS:usize=32;
const MAX_RECIPE_BYTES:usize=1024*1024;
const MAX_FILE_BYTES:usize=MAX_RECIPE_BYTES+4096;
const SCHEMA:&str="vspack-saved-project-v1";

#[derive(Clone)]
pub struct ProjectHub {
    folder:PathBuf,
    mutex:Arc<Mutex<()>>,
}
#[derive(Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedProject {
    schema:String,
    id:String,
    revision:String,
    updated_unix_seconds:u64,
    recipe:PackRecipe,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveRequest {
    recipe:PackRecipe,
    #[serde(default)]
    expected_revision:Option<String>,
}
#[derive(Deserialize)]
struct DeleteQuery {revision:String}

fn valid_id(id:&str)->bool{
    (1..=48).contains(&id.len()) && id.bytes().all(|c|
        c.is_ascii_lowercase()||c.is_ascii_digit()||c==b'-'||c==b'_')
}
fn valid_revision(value:&str)->bool{
    value.len()==64 && value.bytes().all(|c|c.is_ascii_hexdigit())
}
fn digest(recipe:&PackRecipe)->Result<String,ApiError>{
    let bytes=serde_json::to_vec(recipe)
        .map_err(|e|bad(format!("Receta no serializable: {e}")))?;
    Ok(format!("{:x}",Sha256::digest(&bytes)))
}
fn fs_error(action:&str,e:std::io::Error)->ApiError{
    ApiError(StatusCode::INTERNAL_SERVER_ERROR,
        format!("No se pudo {action} el proyecto local: {e}"))
}
fn internal(reason:&str)->ApiError{
    ApiError(StatusCode::INTERNAL_SERVER_ERROR,reason.into())
}
fn conflict()->ApiError{
    ApiError(StatusCode::CONFLICT,
        "La revisión cambió: actualizá el proyecto antes de sobrescribir".into())
}
fn rejected_link(path:&FsPath)->Result<(),ApiError>{
    match fs::symlink_metadata(path){
        Ok(meta) if meta.file_type().is_symlink()=>
            Err(ApiError(StatusCode::FORBIDDEN,
                "Los enlaces simbólicos no se aceptan en proyectos".into())),
        Ok(_)=>Ok(()),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(()),
        Err(e)=>Err(fs_error("inspeccionar",e)),
    }
}
fn project_path(folder:&FsPath,id:&str)->Result<PathBuf,ApiError>{
    if !valid_id(id){return Err(bad("ID del proyecto inválido (a-z, 0-9, _ o -)"));}
    Ok(folder.join(format!("{id}.json")))
}
impl ProjectHub {
    pub fn new(folder:PathBuf)->Self{
        Self{folder,mutex:Arc::new(Mutex::new(()))}
    }
    fn with_lock<T>(&self,operation:impl FnOnce()->Result<T,ApiError>)
        ->Result<T,ApiError>{
        let _lock=self.mutex.lock()
            .map_err(|_|internal("El gestor de proyectos se encuentra bloqueado"))?;
        operation()
    }
    fn prepare(&self)->Result<(),ApiError>{
        rejected_link(&self.folder)?;
        fs::create_dir_all(&self.folder).map_err(|e|fs_error("crear",e))?;
        rejected_link(&self.folder)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.folder,fs::Permissions::from_mode(0o700))
                .map_err(|e|fs_error("proteger",e))?;
        }
        Ok(())
    }
    fn load_inner(&self,id:&str)->Result<Option<SavedProject>,ApiError>{
        let path=project_path(&self.folder,id)?;
        rejected_link(&path)?;
        let metadata=match fs::metadata(&path){
            Ok(m)=>m,
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),
            Err(e)=>return Err(fs_error("leer",e)),
        };
        if !metadata.is_file()||metadata.len()>MAX_FILE_BYTES as u64 {
            return Err(internal("Proyecto guardado inválido o excesivamente grande"));
        }
        let bytes=fs::read(path).map_err(|e|fs_error("leer",e))?;
        if bytes.len()>MAX_FILE_BYTES {
            return Err(internal("Proyecto guardado excede el máximo"));
        }
        let stored:SavedProject=serde_json::from_slice(&bytes)
            .map_err(|_|internal("El archivo del proyecto está corrupto"))?;
        if stored.schema!=SCHEMA||stored.id!=id||
            !valid_revision(&stored.revision)||
            digest(&stored.recipe)?!=stored.revision||
            validate_recipe_structure(&stored.recipe).is_err(){
            return Err(internal("El proyecto guardado no supera la validación"));
        }
        Ok(Some(stored))
    }
    fn count_inner(&self)->Result<usize,ApiError>{
        let iter=match fs::read_dir(&self.folder){
            Ok(d)=>d,
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(0),
            Err(e)=>return Err(fs_error("listar",e)),
        };
        let mut count=0usize;
        for item in iter {
            let item=item.map_err(|e|fs_error("listar",e))?;
            if let Some(id)=item.file_name().to_str()
                .and_then(|name|name.strip_suffix(".json")){
                if valid_id(id){
                    count+=1;
                    if count>MAX_PROJECTS {
                        return Err(internal("El directorio supera el máximo de proyectos"));
                    }
                }
            }
        }
        Ok(count)
    }
    fn list_inner(&self)->Result<Value,ApiError>{
        let iter=match fs::read_dir(&self.folder){
            Ok(d)=>d,
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>
                return Ok(json!({"schema":"vspack-project-list-v1",
                    "projects":[],"max_projects":MAX_PROJECTS})),
            Err(e)=>return Err(fs_error("listar",e)),
        };
        let mut ids=Vec::new();
        for item in iter {
            let item=item.map_err(|e|fs_error("listar",e))?;
            if let Some(id)=item.file_name().to_str()
                .and_then(|name|name.strip_suffix(".json")){
                if valid_id(id){ids.push(id.to_owned());}
            }
            if ids.len()>MAX_PROJECTS {
                return Err(internal("Demasiados proyectos para listar"));
            }
        }
        ids.sort();
        let mut projects=Vec::new();
        for id in ids {
            if let Some(stored)=self.load_inner(&id)? {
                projects.push(json!({
                    "id":stored.id,
                    "name":stored.recipe.name,
                    "revision":stored.revision,
                    "updated_unix_seconds":stored.updated_unix_seconds,
                    "source_count":stored.recipe.sources.len()
                }));
            }
        }
        Ok(json!({"schema":"vspack-project-list-v1",
            "projects":projects,"max_projects":MAX_PROJECTS}))
    }
    fn save_inner(&self,id:String,input:SaveRequest)
        ->Result<SavedProject,ApiError>{
        validate_recipe_structure(&input.recipe)?;
        if let Some(expected)=input.expected_revision.as_deref(){
            if !valid_revision(expected){return Err(bad("Revisión esperada inválida"));}
        }
        self.prepare()?;
        let path=project_path(&self.folder,&id)?;
        let before=self.load_inner(&id)?;
        match (before.as_ref(),input.expected_revision.as_deref()) {
            (Some(old),Some(expected)) if old.revision==expected=>{},
            (None,None)=>{},
            _=>return Err(conflict()),
        }
        if before.is_none()&&self.count_inner()? >= MAX_PROJECTS{
            return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,
                "Máximo de 32 proyectos guardados".into()));
        }
        let revision=digest(&input.recipe)?;
        let updated_unix_seconds=SystemTime::now().duration_since(UNIX_EPOCH)
            .map_err(|_|internal("Reloj del sistema inválido"))?.as_secs();
        let record=SavedProject {
            schema:SCHEMA.into(),id:id.clone(),revision,
            updated_unix_seconds,recipe:input.recipe
        };
        let bytes=serde_json::to_vec(&record)
            .map_err(|_|internal("No se pudo serializar el proyecto"))?;
        if bytes.len()>MAX_FILE_BYTES{
            return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,
                "Proyecto guardado excede el máximo".into()));
        }
        // Never partially rewrite an existing recipe. A temp file is fsynced
        // before rename, and no untrusted path is used to create filenames.
        let temp_name=format!(".{id}-{}-{}.tmp",std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)
                .map_err(|_|internal("Reloj inválido"))?.as_nanos());
        let temp=self.folder.join(temp_name);
        let result=(||->Result<(),ApiError>{
            let mut opts=OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {use std::os::unix::fs::OpenOptionsExt;opts.mode(0o600);}
            let mut file=opts.open(&temp).map_err(|e|fs_error("crear",e))?;
            file.write_all(&bytes).map_err(|e|fs_error("escribir",e))?;
            file.sync_all().map_err(|e|fs_error("sincronizar",e))?;
            drop(file);
            rejected_link(&path)?;
            fs::rename(&temp,&path).map_err(|e|fs_error("reemplazar",e))?;
            Ok(())
        })();
        if result.is_err(){let _=fs::remove_file(temp);}
        result?;
        Ok(record)
    }
    fn delete_inner(&self,id:&str,revision:&str)->Result<(),ApiError>{
        if !valid_revision(revision){return Err(bad("Revisión inválida"));}
        let stored=self.load_inner(id)?.ok_or_else(||ApiError(
            StatusCode::NOT_FOUND,"Proyecto no encontrado".into()))?;
        if stored.revision!=revision{return Err(conflict());}
        let path=project_path(&self.folder,id)?;
        fs::remove_file(path).map_err(|e|fs_error("eliminar",e))
    }
}
async fn execute<T:Send+'static>(
    hub:ProjectHub,operation:impl FnOnce(ProjectHub)->Result<T,ApiError>
        +Send+'static
)->Result<T,ApiError>{
    tokio::task::spawn_blocking(move||hub.with_lock(||operation(hub.clone())))
        .await.map_err(|e|internal(&format!("Operación de proyecto interrumpida: {e}")))?
}
pub async fn list(Extension(hub):Extension<ProjectHub>)
    ->Result<Json<Value>,ApiError>{
    Ok(Json(execute(hub,|hub|hub.list_inner()).await?))
}
pub async fn get(Extension(hub):Extension<ProjectHub>,
    Extension(packs):Extension<PackHub>,
    Path(id):Path<String>)->Result<Json<Value>,ApiError>{
    let saved=execute(hub,move|hub|hub.load_inner(&id)).await?
        .ok_or_else(||ApiError(StatusCode::NOT_FOUND,
            "Proyecto no encontrado".into()))?;
    let loaded=packs.0.lock().await;
    let missing=saved.recipe.sources.iter()
        .filter(|source|!loaded.iter().any(|p|p.fingerprint==source.fingerprint))
        .map(|source|source.fingerprint.clone()).collect::<Vec<_>>();
    Ok(Json(json!({
        "schema":SCHEMA,
        "id":saved.id,"revision":saved.revision,
        "updated_unix_seconds":saved.updated_unix_seconds,
        "recipe":saved.recipe,"missing_sources":missing,
        "source_zips_persisted":false
    })))
}
pub async fn save(Extension(hub):Extension<ProjectHub>,
    Path(id):Path<String>,headers:HeaderMap,body:Bytes)
    ->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;
    if body.len()>MAX_RECIPE_BYTES{
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,
            "Receta supera el máximo de 1 MiB".into()));
    }
    if !valid_id(&id){return Err(bad("ID de proyecto inválido"));}
    let input:SaveRequest=serde_json::from_slice(&body)
        .map_err(|e|bad(format!("Proyecto inválido: {e}")))?;
    let saved=execute(hub,move|hub|hub.save_inner(id,input)).await?;
    Ok(Json(json!({"schema":SCHEMA,"id":saved.id,
        "revision":saved.revision,
        "updated_unix_seconds":saved.updated_unix_seconds,
        "source_zips_persisted":false})))
}
pub async fn delete(Extension(hub):Extension<ProjectHub>,
    Path(id):Path<String>,Query(query):Query<DeleteQuery>,headers:HeaderMap)
    ->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;
    execute(hub,move|hub|hub.delete_inner(&id,&query.revision)).await?;
    Ok(Json(json!({"deleted":true})))
}
#[cfg(test)]
mod tests{
    use super::*;
    use std::sync::atomic::{AtomicUsize,Ordering};
    static COUNTER:AtomicUsize=AtomicUsize::new(0);
    fn temporary()->(ProjectHub,PathBuf){
        let path=std::env::temp_dir().join(format!(
            "vsmedia-project-store-{}-{}",
            std::process::id(),COUNTER.fetch_add(1,Ordering::Relaxed)));
        let hub=ProjectHub::new(path.clone());
        (hub,path)
    }
    fn recipe()->PackRecipe{
        PackRecipe {
            schema:"vspack-recipe-v1".into(),
            name:"Virgoland - Mandala tooltip".into(),
            sources:vec![crate::pack_manager::RecipeSource{
                fingerprint:"a".repeat(64),enabled:true,priority:20}],
            choices:Default::default(),exclude:Vec::new(),
        }
    }
    #[test]
    fn survives_restart_and_protects_against_lost_updates(){
        let (hub,path)=temporary();
        let first=hub.with_lock(||hub.save_inner("virgoland".into(),
            SaveRequest{recipe:recipe(),expected_revision:None})).unwrap();
        let reloaded=ProjectHub::new(path.clone());
        assert_eq!(reloaded.load_inner("virgoland").unwrap().unwrap().recipe.name,
            "Virgoland - Mandala tooltip");
        assert_eq!(reloaded.list_inner().unwrap()["projects"].as_array().unwrap().len(),1);
        let duplicate=hub.save_inner("virgoland".into(),
            SaveRequest{recipe:recipe(),expected_revision:None});
        assert_eq!(duplicate.unwrap_err().0,StatusCode::CONFLICT);
        let mut revised=recipe();
        revised.name="Virgoland - Cozy + Mandala".into();
        let saved=hub.save_inner("virgoland".into(),SaveRequest{
            recipe:revised,expected_revision:Some(first.revision.clone())}).unwrap();
        assert_ne!(saved.revision,first.revision);
        assert_eq!(hub.delete_inner("virgoland",&first.revision)
            .unwrap_err().0,StatusCode::CONFLICT);
        hub.delete_inner("virgoland",&saved.revision).unwrap();
        assert!(hub.load_inner("virgoland").unwrap().is_none());
        let _=fs::remove_dir_all(path);
    }
    #[test]
    fn rejects_unsafe_ids_corrupted_files_and_invalid_recipe(){
        let (hub,path)=temporary();
        assert!(!valid_id("../../secret"));
        assert!(project_path(&path,"../escape").is_err());
        let mut invalid=recipe();
        invalid.sources[0].fingerprint="not-a-hash".into();
        assert!(hub.save_inner("safe-id".into(),SaveRequest{
            recipe:invalid,expected_revision:None}).is_err());
        let _=hub.save_inner("safe-id".into(),SaveRequest{
            recipe:recipe(),expected_revision:None}).unwrap();
        fs::write(path.join("safe-id.json"),b"{}").unwrap();
        assert_eq!(hub.load_inner("safe-id").unwrap_err().0,
            StatusCode::INTERNAL_SERVER_ERROR);
        let _=fs::remove_dir_all(path);
    }
    #[cfg(unix)]
    #[test]
    fn refuses_symlink_projects(){
        use std::os::unix::fs::symlink;
        let (hub,path)=temporary();
        fs::create_dir_all(&path).unwrap();
        symlink(path.join("outside"),path.join("alias.json")).unwrap();
        assert_eq!(hub.load_inner("alias").unwrap_err().0,StatusCode::FORBIDDEN);
        let _=fs::remove_dir_all(path);
    }
}
