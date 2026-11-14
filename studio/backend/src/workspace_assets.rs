//! Project-scoped editable source assets. NOT part of a Minecraft resource pack.
//! The project owns physical assets/, exports/ and project.json. Nothing here
//! is published to players; only explicit VSPack publication can do that.
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path as FsPath, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use axum::{
    body::{Body, Bytes}, extract::{Path, Query}, http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response}, Extension, Json,
};
use futures_util::stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use crate::{authorize_origin, bad, ApiError};

const SCHEMA: &str = "vsmedia-workspace-v1";
const MAX_PROJECTS: usize = 32;
const MAX_FILES: usize = 2000;
const MAX_FILE_BYTES: usize = 96 * 1024 * 1024;
const MAX_PROJECT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_FOLDERS: &[&str] = &[
    "models", "textures", "audio", "video", "fonts", "scripts", "documents", "other",
];

#[derive(Clone)]
pub struct WorkspaceHub {
    root: PathBuf,
    lock: Arc<Mutex<()>>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileEntry {
    path: String,
    bytes: u64,
    sha256: String,
    updated_unix: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Project {
    schema: String,
    id: String,
    name: String,
    created_unix: u64,
    files: Vec<FileEntry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {name: String}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FolderRequest {path: String}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveRequest {from: String, to: String}
#[derive(Deserialize)]
pub struct FileQuery {
    path: String,
    #[serde(default)]
    replace: bool,
}

fn timestamp() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}
fn failed(verb: &str, e: std::io::Error) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("Workspace: {verb}: {e}"))
}
fn missing(message: &str) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, message.to_owned())
}
fn forbidden(message: &str) -> ApiError {
    ApiError(StatusCode::FORBIDDEN, message.to_owned())
}
fn conflict(message: &str) -> ApiError {
    ApiError(StatusCode::CONFLICT, message.to_owned())
}
fn check_id(value: &str) -> Result<(), ApiError> {
    if (1..=48).contains(&value.len())
        && value.bytes().all(|b| b.is_ascii_lowercase()
            || b.is_ascii_digit() || b == b'-' || b == b'_')
    {Ok(())} else {Err(bad("ID de proyecto inválido: usar a-z, 0-9, _ o -"))}
}
fn check_name(value: &str) -> Result<(), ApiError> {
    if !value.trim().is_empty() && value.chars().count() <= 80
        && !value.chars().any(char::is_control)
    {Ok(())} else {Err(bad("Nombre de proyecto inválido (máximo 80 caracteres)"))}
}
fn segments(path: &str, directory: bool) -> Result<Vec<&str>, ApiError> {
    if path.len() > 320 || path.contains('\\') || path.contains('\0')
        || path.starts_with('/') || path.ends_with('/')
    {return Err(bad("Ruta de assets inválida"));}
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 2 || parts.len() > 16 || parts[0] != "assets" {
        return Err(bad("Los archivos y carpetas deben estar dentro de assets/"));
    }
    for part in &parts {
        if part.is_empty() || *part == "." || *part == ".."
            || part.as_bytes().len() > 100
            || part.ends_with('.') || part.ends_with(' ')
            || part.chars().any(|c| c.is_control() || matches!(c, ':' | '<' | '>' | '|' | '"' | '*' | '?'))
        {return Err(bad("La ruta contiene un segmento peligroso"));}
        let upper=part.split('.').next().unwrap_or_default().to_ascii_uppercase();
        if matches!(upper.as_str(), "CON"|"PRN"|"AUX"|"NUL"
            |"COM1"|"COM2"|"COM3"|"COM4"|"COM5"|"COM6"|"COM7"|"COM8"|"COM9"
            |"LPT1"|"LPT2"|"LPT3"|"LPT4"|"LPT5"|"LPT6"|"LPT7"|"LPT8"|"LPT9") {
            return Err(bad("Nombre reservado del sistema operativo"));
        }
    }
    if !directory && parts.last().is_some_and(|part| part.starts_with('.')) {
        return Err(bad("No se admiten archivos ocultos ejecutables en el proyecto"));
    }
    Ok(parts)
}
fn ensure_regular(path: &FsPath) -> Result<(), ApiError> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Err(forbidden("Enlace simbólico no permitido")),
        Ok(m) if !m.is_file() && !m.is_dir() => Err(forbidden("Archivo especial no permitido")),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(failed("inspeccionar ruta", e)),
    }
}
fn ensure_directory(path: &FsPath) -> Result<(), ApiError> {
    ensure_regular(path)?;
    fs::create_dir_all(path).map_err(|e| failed("crear carpeta",e))?;
    ensure_regular(path)?;
    if !fs::metadata(path).map_err(|e| failed("examinar carpeta",e))?.is_dir() {
        return Err(forbidden("La ruta no corresponde a una carpeta"));
    }
    Ok(())
}
fn secure_file(root: &FsPath, relative: &str, create_parents: bool) -> Result<PathBuf, ApiError> {
    let parts=segments(relative, false)?;
    let mut current=root.to_path_buf();
    ensure_regular(&current)?;
    for part in &parts[..parts.len()-1] {
        current.push(part);
        if create_parents {ensure_directory(&current)?;}
        else {
            ensure_regular(&current)?;
            if !current.is_dir() {return Err(missing("Carpeta de origen no encontrada"));}
        }
    }
    let path=current.join(parts.last().expect("validated path"));
    ensure_regular(&path)?;
    if path.exists() && !path.is_file() {return Err(forbidden("La ruta no es un archivo"));}
    Ok(path)
}
fn atomic(folder: &FsPath, target: &FsPath, data: &[u8]) -> Result<(), ApiError> {
    ensure_regular(target)?;
    let tmp=folder.join(format!(".workspace-{}.tmp",Uuid::new_v4()));
    let result=(||{
        let mut f=OpenOptions::new().create_new(true).write(true).open(&tmp)
            .map_err(|e|failed("crear archivo temporal",e))?;
        f.write_all(data).map_err(|e|failed("escribir archivo",e))?;
        f.sync_all().map_err(|e|failed("sincronizar archivo",e))?;
        fs::rename(&tmp,target).map_err(|e|failed("guardar archivo",e))
    })();
    if result.is_err(){let _=fs::remove_file(tmp);}
    result
}
fn fingerprint(data: &[u8]) -> String {format!("{:x}",Sha256::digest(data))}
fn manifest_path(root: &FsPath) -> PathBuf {root.join("project.json")}
fn total(project:&Project) -> u64 {
    project.files.iter().map(|f|f.bytes).sum()
}
fn source_type(path:&str) -> &'static str {
    let lower=path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match lower.as_str() {
        "png"|"jpg"|"jpeg"|"webp"|"gif"|"svg"|"psd"|"ase" => "texture",
        "bbmodel"|"obj"|"fbx"|"gltf"|"glb"|"blend"|"json5" => "model",
        "wav"|"ogg"|"mp3"|"flac"|"opus"|"m4a"|"aiff" => "audio",
        "mp4"|"webm"|"mov"|"mkv" => "video",
        "ttf"|"otf"|"woff"|"woff2" => "font",
        "js"|"ts"|"py"|"rs"|"java"|"lua"|"sh"|"mcfunction" => "script",
        "txt"|"md"|"pdf"|"docx"|"odt" => "document",
        _ => "other",
    }
}
fn file_summary(entry:&FileEntry) -> Value {
    json!({"path":entry.path,"bytes":entry.bytes,"sha256":entry.sha256,
        "category":source_type(&entry.path),"modified_unix":entry.updated_unix})
}
impl WorkspaceHub {
    pub fn new(root:PathBuf)->Self {
        Self{root,lock:Arc::new(Mutex::new(()))}
    }
    fn prepare(&self)->Result<(),ApiError> {
        ensure_directory(&self.root)
    }
    fn root(&self,id:&str)->Result<PathBuf,ApiError> {
        check_id(id)?;
        let path=self.root.join(id);
        ensure_regular(&path)?;
        Ok(path)
    }
    fn load(&self,id:&str)->Result<Project,ApiError>{
        let root=self.root(id)?;
        let path=manifest_path(&root);
        ensure_regular(&path)?;
        let bytes=fs::read(path).map_err(|e|if e.kind()==std::io::ErrorKind::NotFound {
            missing("El proyecto no existe")
        }else{failed("abrir proyecto",e)})?;
        if bytes.len()>MAX_MANIFEST_BYTES{return Err(bad("Metadatos demasiado grandes"));}
        let data:Project=serde_json::from_slice(&bytes).map_err(bad)?;
        if data.schema!=SCHEMA || data.id!=id {return Err(bad("Proyecto corrupto o ID inconsistente"));}
        check_name(&data.name)?;
        if data.files.len()>MAX_FILES || total(&data)>MAX_PROJECT_BYTES {
            return Err(bad("El índice de assets supera las cuotas"));
        }
        let mut found=BTreeSet::new();
        for entry in &data.files {
            segments(&entry.path,false)?;
            if !found.insert(entry.path.as_str()) || entry.bytes>MAX_FILE_BYTES as u64
                || entry.sha256.len()!=64 || !entry.sha256.bytes().all(|c|c.is_ascii_hexdigit()) {
                return Err(bad("Índice de assets corrupto"));
            }
        }
        Ok(data)
    }
    fn save(&self,project:&Project)->Result<(),ApiError>{
        let root=self.root(&project.id)?;
        let body=serde_json::to_vec_pretty(project).map_err(bad)?;
        if body.len()>MAX_MANIFEST_BYTES{return Err(bad("Índice de assets supera 2 MiB"));}
        atomic(&root,&manifest_path(&root),&body)
    }
    fn list_projects(&self)->Result<Vec<Value>,ApiError>{
        self.prepare()?;
        let mut out=Vec::new();
        for entry in fs::read_dir(&self.root).map_err(|e|failed("listar proyectos",e))? {
            let entry=entry.map_err(|e|failed("leer entrada",e))?;
            let id=entry.file_name().to_string_lossy().to_string();
            if check_id(&id).is_err(){continue;}
            ensure_regular(&entry.path())?;
            let p=self.load(&id)?;
            out.push(json!({"id":p.id,"name":p.name,"files":p.files.len(),
                "bytes":total(&p),"created_unix":p.created_unix}));
        }
        out.sort_by(|a,b|a["name"].as_str().cmp(&b["name"].as_str()));
        Ok(out)
    }
    fn create(&self,id:&str,name:&str)->Result<Value,ApiError>{
        self.prepare()?;
        check_name(name)?;
        let root=self.root(id)?;
        if root.exists(){return Err(conflict("Ya existe ese proyecto"));}
        if self.list_projects()?.len()>=MAX_PROJECTS{return Err(bad("Máximo 32 proyectos"));}
        fs::create_dir(&root).map_err(|e|failed("crear proyecto",e))?;
        for dir in DEFAULT_FOLDERS {
            ensure_directory(&root.join("assets").join(dir))?;
        }
        ensure_directory(&root.join("exports"))?;
        let project=Project{schema:SCHEMA.into(),id:id.to_owned(),name:name.trim().into(),
            created_unix:timestamp(),files:Vec::new()};
        self.save(&project)?;
        Ok(json!({"id":id,"name":project.name,"created":true,
            "root":"assets/","resourcepack_published":false}))
    }
    fn inventory(&self,id:&str)->Result<Value,ApiError>{
        let project=self.load(id)?;
        let root=self.root(id)?;
        let mut folders=Vec::new();
        fn visit(root:&FsPath,relative:&str,list:&mut Vec<String>,depth:usize)->Result<(),ApiError>{
            if depth>16||list.len()>4096{return Err(bad("Demasiadas carpetas"));}
            let folder=root.join(relative);
            ensure_regular(&folder)?;
            for entry in fs::read_dir(&folder).map_err(|e|failed("leer carpetas",e))? {
                let entry=entry.map_err(|e|failed("leer entrada",e))?;
                ensure_regular(&entry.path())?;
                if entry.file_type().map_err(|e|failed("leer tipo",e))?.is_dir(){
                    let next=format!("{relative}/{}",entry.file_name().to_string_lossy());
                    list.push(next.clone());
                    visit(root,&next,list,depth+1)?;
                }
            }
            Ok(())
        }
        folders.push("assets".to_string());
        visit(&root,"assets",&mut folders,0)?;
        folders.sort();
        let files:Vec<Value>=project.files.iter().map(file_summary).collect();
        Ok(json!({"id":id,"name":project.name,"schema":SCHEMA,"files":files,
            "folders":folders,"total_bytes":total(&project),
            "max_bytes":MAX_PROJECT_BYTES,"max_file_bytes":MAX_FILE_BYTES,
            "resourcepack_published":false,"resourcepack_mode":"explicit_only",
            "separate_from_vspack":true}))
    }
    fn folder(&self,id:&str,relative:&str)->Result<Value,ApiError>{
        let _=self.load(id)?;
        let parts=segments(relative,true)?;
        let root=self.root(id)?;
        let mut dir=root;
        for part in parts {
            dir.push(part);
            ensure_directory(&dir)?;
        }
        Ok(json!({"created":true,"path":relative}))
    }
    fn upload(&self,id:&str,relative:&str,bytes:&[u8],replace:bool)->Result<Value,ApiError>{
        if bytes.is_empty()||bytes.len()>MAX_FILE_BYTES {
            return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,"Archivo vacío o superior a 96 MiB".into()));
        }
        let mut project=self.load(id)?;
        let existing=project.files.iter().position(|e|e.path==relative);
        if existing.is_some()&&!replace {
            return Err(conflict("Ya existe el archivo. Elegí reemplazarlo explícitamente"));
        }
        if existing.is_none()&&project.files.len()>=MAX_FILES {
            return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,"Máximo 2000 archivos por proyecto".into()));
        }
        let old=existing.map(|i|project.files[i].bytes).unwrap_or(0);
        if total(&project).saturating_sub(old).saturating_add(bytes.len() as u64)>MAX_PROJECT_BYTES {
            return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,"El proyecto supera 512 MiB de assets".into()));
        }
        let root=self.root(id)?;
        let path=secure_file(&root,relative,true)?;
        if path.exists()&&existing.is_none(){
            return Err(conflict("Existe un archivo físico sin registrar; revisá el proyecto antes de sobrescribir"));
        }
        let hash=fingerprint(bytes);
        if let Some(i)=existing {
            if project.files[i].sha256==hash&&project.files[i].bytes==bytes.len() as u64 {
                return Ok(json!({"path":relative,"bytes":bytes.len(),"sha256":hash,
                    "unchanged":true,"resourcepack_published":false}));
            }
        }
        atomic(path.parent().expect("file has parent"),&path,bytes)?;
        let entry=FileEntry{path:relative.into(),bytes:bytes.len() as u64,
            sha256:hash.clone(),updated_unix:timestamp()};
        if let Some(i)=existing {project.files[i]=entry;} else {project.files.push(entry);}
        project.files.sort_by(|a,b|a.path.cmp(&b.path));
        self.save(&project)?;
        Ok(json!({"path":relative,"bytes":bytes.len(),"sha256":hash,
            "resourcepack_published":false,"unchanged":false}))
    }
    fn verify_one(&self,id:&str,entry:&FileEntry)->Result<Vec<u8>,ApiError>{
        let root=self.root(id)?;
        let path=secure_file(&root,&entry.path,false)?;
        let len=fs::metadata(&path).map_err(|_|missing("Archivo del proyecto no encontrado"))?.len();
        if len!=entry.bytes {return Err(conflict("Archivo modificado fuera de VSMedia: tamaño diferente"));}
        let content=fs::read(path).map_err(|e|failed("leer asset",e))?;
        if fingerprint(&content)!=entry.sha256 {
            return Err(conflict("Integridad SHA-256 incorrecta; asset modificado o dañado"));
        }
        Ok(content)
    }
    fn download(&self,id:&str,path:&str)->Result<Vec<u8>,ApiError>{
        let project=self.load(id)?;
        segments(path,false)?;
        let entry=project.files.iter().find(|e|e.path==path).ok_or_else(||
            missing("Archivo no encontrado en el índice de proyecto"))?;
        self.verify_one(id,entry)
    }
    fn remove(&self,id:&str,path:&str)->Result<Value,ApiError>{
        let mut project=self.load(id)?;
        segments(path,false)?;
        let index=project.files.iter().position(|e|e.path==path)
            .ok_or_else(||missing("Archivo no encontrado"))?;
        let file=secure_file(&self.root(id)?,path,false)?;
        fs::remove_file(file).map_err(|e|failed("borrar asset",e))?;
        project.files.remove(index);
        self.save(&project)?;
        Ok(json!({"deleted":true,"path":path,"resourcepack_unchanged":true}))
    }
    fn move_file(&self,id:&str,source:&str,target:&str)->Result<Value,ApiError>{
        segments(source,false)?;segments(target,false)?;
        if source==target {return Err(bad("Elegí una ruta de destino distinta"));}
        let mut project=self.load(id)?;
        let index=project.files.iter().position(|f|f.path==source)
            .ok_or_else(||missing("Archivo de origen no encontrado"))?;
        if project.files.iter().any(|f|f.path==target){
            return Err(conflict("Ya existe un archivo con ese nombre"));
        }
        let root=self.root(id)?;
        let from=secure_file(&root,source,false)?;
        let to=secure_file(&root,target,true)?;
        if to.exists(){return Err(conflict("El destino ya existe en disco"));}
        // Verify source before moving: corruption never becomes an accepted revision.
        self.verify_one(id,&project.files[index])?;
        fs::rename(from,to).map_err(|e|failed("mover archivo",e))?;
        project.files[index].path=target.to_owned();
        project.files[index].updated_unix=timestamp();
        project.files.sort_by(|a,b|a.path.cmp(&b.path));
        self.save(&project)?;
        Ok(json!({"moved":true,"from":source,"to":target}))
    }
    fn verify(&self,id:&str)->Result<Value,ApiError>{
        let project=self.load(id)?;
        let mut problems=Vec::new();
        for entry in &project.files {
            if let Err(problem)=self.verify_one(id,entry){
                problems.push(json!({"path":entry.path,"error":problem.1}));
            }
        }
        Ok(json!({"verified":project.files.len()-problems.len(),
            "total":project.files.len(),"problems":problems,"ok":problems.is_empty()}))
    }
    fn prepare_export(&self,id:&str)->Result<PathBuf,ApiError>{
        let project=self.load(id)?;
        let root=self.root(id)?;
        let exports=root.join("exports");
        ensure_directory(&exports)?;
        let name=format!(".workspace-export-{}.tmp",Uuid::new_v4());
        let tmp=exports.join(name);
        let result=(||{
            let file=OpenOptions::new().create_new(true).write(true).open(&tmp)
                .map_err(|e|failed("crear exportación ZIP",e))?;
            let mut zip=zip::ZipWriter::new(file);
            let options=zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            let metadata=serde_json::to_vec_pretty(&project).map_err(bad)?;
            zip.start_file("project.json",options)
                .map_err(bad)?;
            zip.write_all(&metadata).map_err(|e|failed("exportar manifest",e))?;
            for item in &project.files {
                let data=self.verify_one(id,item)?;
                zip.start_file(&item.path,options).map_err(bad)?;
                zip.write_all(&data).map_err(|e|failed("exportar asset",e))?;
            }
            zip.finish().map_err(bad)?;
            Ok(())
        })();
        if result.is_err(){let _=fs::remove_file(&tmp);}
        result?;
        Ok(tmp)
    }
}
async fn run<T,F>(hub:WorkspaceHub,f:F)->Result<T,ApiError>
where T:Send+'static,F:FnOnce(&WorkspaceHub)->Result<T,ApiError>+Send+'static {
    tokio::task::spawn_blocking(move||{
        let lock=hub.lock.clone();
        let _guard=lock.lock().map_err(|_|bad("Bloqueo de workspace dañado"))?;
        f(&hub)
    }).await.map_err(|e|failed("ejecutar operación",std::io::Error::other(e.to_string())))?
}
pub async fn list(Extension(hub):Extension<WorkspaceHub>)->Result<Json<Value>,ApiError>{
    Ok(Json(json!({"projects":run(hub,|h|h.list_projects()).await?,
        "schema":SCHEMA,"pack_separate":true})))
}
pub async fn create(Extension(hub):Extension<WorkspaceHub>,Path(id):Path<String>,
    headers:HeaderMap,Json(request):Json<CreateRequest>)->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;
    Ok(Json(run(hub,move|h|h.create(&id,&request.name)).await?))
}
pub async fn inventory(Extension(hub):Extension<WorkspaceHub>,Path(id):Path<String>)
    ->Result<Json<Value>,ApiError>{
    Ok(Json(run(hub,move|h|h.inventory(&id)).await?))
}
pub async fn folder(Extension(hub):Extension<WorkspaceHub>,Path(id):Path<String>,
    headers:HeaderMap,Json(request):Json<FolderRequest>)->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;
    Ok(Json(run(hub,move|h|h.folder(&id,&request.path)).await?))
}
pub async fn upload(Extension(hub):Extension<WorkspaceHub>,Path(id):Path<String>,
    Query(q):Query<FileQuery>,headers:HeaderMap,body:Bytes)->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;
    Ok(Json(run(hub,move|h|h.upload(&id,&q.path,&body,q.replace)).await?))
}
pub async fn download(Extension(hub):Extension<WorkspaceHub>,Path(id):Path<String>,
    Query(q):Query<FileQuery>)->Result<Response,ApiError>{
    let content=run(hub,move|h|h.download(&id,&q.path)).await?;
    Ok(([(header::CONTENT_TYPE,"application/octet-stream"),
         (header::CONTENT_DISPOSITION,"attachment; filename=\"workspace-asset\""),
         (header::X_CONTENT_TYPE_OPTIONS,"nosniff"),
         (header::CACHE_CONTROL,"no-store")],content).into_response())
}
pub async fn delete(Extension(hub):Extension<WorkspaceHub>,Path(id):Path<String>,
    Query(q):Query<FileQuery>,headers:HeaderMap)->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;
    Ok(Json(run(hub,move|h|h.remove(&id,&q.path)).await?))
}
pub async fn rename(Extension(hub):Extension<WorkspaceHub>,Path(id):Path<String>,
    headers:HeaderMap,Json(req):Json<MoveRequest>)->Result<Json<Value>,ApiError>{
    authorize_origin(&headers)?;
    Ok(Json(run(hub,move|h|h.move_file(&id,&req.from,&req.to)).await?))
}
pub async fn verify(Extension(hub):Extension<WorkspaceHub>,Path(id):Path<String>)
    ->Result<Json<Value>,ApiError>{
    Ok(Json(run(hub,move|h|h.verify(&id)).await?))
}
pub async fn export(Extension(hub):Extension<WorkspaceHub>,Path(id):Path<String>)
    ->Result<Response,ApiError>{
    let tmp=run(hub,move|h|h.prepare_export(&id)).await?;
    let (sender,receiver)=tokio::sync::mpsc::channel::<Result<Bytes,std::io::Error>>(4);
    tokio::task::spawn_blocking(move||{
        let mut buffer=[0u8;64*1024];
        let opened=File::open(&tmp);
        match opened {
            Ok(mut file)=>{
                loop{
                    match file.read(&mut buffer){
                        Ok(0)=>break,
                        Ok(n)=>{
                            if sender.blocking_send(Ok(Bytes::copy_from_slice(&buffer[..n]))).is_err(){
                                break;
                            }
                        },
                        Err(error)=>{
                            let _=sender.blocking_send(Err(error));break;
                        }
                    }
                }
            }
            Err(error)=>{let _=sender.blocking_send(Err(error));}
        }
        let _=fs::remove_file(tmp);
    });
    let stream=stream::unfold(receiver,|mut rx|async move {
        rx.recv().await.map(|item|(item,rx))
    });
    let body=Body::from_stream(stream);
    let mut response=body.into_response();
    response.headers_mut().insert(header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"));
    response.headers_mut().insert(header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"vsmedia-project.zip\""));
    response.headers_mut().insert(header::CACHE_CONTROL,HeaderValue::from_static("no-store"));
    response.headers_mut().insert(header::X_CONTENT_TYPE_OPTIONS,HeaderValue::from_static("nosniff"));
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize,Ordering};
    static NEXT:AtomicUsize=AtomicUsize::new(0);
    fn hub()->WorkspaceHub {
        WorkspaceHub::new(std::env::temp_dir().join(format!("vsmedia-workspace-{}-{}",
            std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed))))
    }
    #[test]
    fn project_assets_are_persistent_and_never_enter_resourcepack(){
        let h=hub();
        h.create("virgoland","Virgoland").unwrap();
        let path="assets/models/faro-12.bbmodel";
        let data=br#"{"meta":{"format_version":"4.10"}}"#;
        let uploaded=h.upload("virgoland",path,data,false).unwrap();
        assert_eq!(uploaded["resourcepack_published"],false);
        assert_eq!(h.inventory("virgoland").unwrap()["files"].as_array().unwrap().len(),1);
        assert_eq!(h.download("virgoland",path).unwrap(),data);
        let restarted=WorkspaceHub::new(h.root.clone());
        assert_eq!(restarted.download("virgoland",path).unwrap(),data);
        assert!(h.upload("virgoland",path,data,false).is_err());
        h.move_file("virgoland",path,"assets/models/escopeta.bbmodel").unwrap();
        assert!(h.download("virgoland",path).is_err());
        assert_eq!(h.verify("virgoland").unwrap()["ok"],true);
        h.remove("virgoland","assets/models/escopeta.bbmodel").unwrap();
        assert!(h.inventory("virgoland").unwrap()["files"].as_array().unwrap().is_empty());
        let _=fs::remove_dir_all(h.root);
    }
    #[test]
    fn rejects_traversal_reserved_names_symlinks_and_corruption(){
        for bad in ["assets/../secrets.txt","assets/C:/foo","assets/CON.txt",
            "other/data.zip","assets/.env","assets//double","assets/./file","assets/a\\b"] {
            assert!(segments(bad,false).is_err(),"{bad}");
        }
        let h=hub();
        h.create("demo","Demo").unwrap();
        h.upload("demo","assets/textures/banner.png",b"original",false).unwrap();
        let file=h.root.join("demo/assets/textures/banner.png");
        fs::write(file,b"altered").unwrap();
        assert!(h.download("demo","assets/textures/banner.png").is_err());
        assert_eq!(h.verify("demo").unwrap()["ok"],false);
        let _=fs::remove_dir_all(h.root);
    }
    #[test]
    fn folder_creation_and_project_quota_checks(){
        let h=hub();
        h.create("demo","Demo").unwrap();
        h.folder("demo","assets/models/weapons").unwrap();
        let listed=h.inventory("demo").unwrap();
        assert!(listed["folders"].as_array().unwrap().iter()
            .any(|p|p=="assets/models/weapons"));
        assert!(h.create("demo","Different").is_err());
        assert!(h.upload("demo","assets/audio/empty.wav",b"",false).is_err());
        let _=fs::remove_dir_all(h.root);
    }
}
