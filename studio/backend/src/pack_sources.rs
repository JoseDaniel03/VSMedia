//! Persistent, single-user VSPack ZIP originals, loaded only from a local directory.
//! Files are integrity-checked on every boot. No network, URLs, or file paths
//! are accepted from request bodies; names are derived from validated pack IDs.
//! Source ZIPs are stored separately from project recipes so they survive restarts.
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use vsmedia_pack::archive;
use crate::{ApiError, bad, packs::{LoadedPack, limits}};
use axum::http::StatusCode;
use std::sync::Arc;

const SCHEMA:&str="vspack-source-v1";
const MAX_ZIP:usize=96*1024*1024;
const MAX_DISK:usize=512*1024*1024;
const MAX_MEMORY:usize=384*1024*1024;
const MAX_PACKS:usize=24;

#[derive(Clone)]
pub(crate) struct SourceStore {folder:PathBuf}

#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceMeta{
    schema:String,
    id:String,
    name:String,
    fingerprint:String,
    priority:i32,
    zip_bytes:usize,
}
fn valid_id(id:&str)->bool{
    (1..=48).contains(&id.len())&&id.bytes().all(|b|
        b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'_'||b==b'-')
}
fn err(action:&str,e:std::io::Error)->ApiError{
    ApiError(StatusCode::INTERNAL_SERVER_ERROR,format!("VSPack: no se pudo {action}: {e}"))
}
fn ensure_regular(path:&Path)->Result<(),ApiError>{
    match fs::symlink_metadata(path){
        Ok(meta) if meta.file_type().is_symlink()=>
            Err(ApiError(StatusCode::FORBIDDEN,"Se rechazó un enlace simbólico en VSPack".into())),
        Ok(meta) if !meta.is_file() && !meta.is_dir()=>
            Err(bad("Archivo o carpeta VSPack no regular")),
        Ok(_)=>Ok(()),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(()),
        Err(e)=>Err(err("inspeccionar archivo",e)),
    }
}
fn write_atomic(folder:&Path,dest:&Path,data:&[u8])->Result<(),ApiError>{
    ensure_regular(dest)?;
    let temporary=folder.join(format!(".vspack-{}.tmp",Uuid::new_v4()));
    let result=(||{
        let mut file=OpenOptions::new().write(true).create_new(true).open(&temporary)
            .map_err(|e|err("crear archivo temporal",e))?;
        #[cfg(unix)]{
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|e|err("proteger archivo",e))?;
        }
        file.write_all(data).map_err(|e|err("escribir archivo",e))?;
        file.sync_all().map_err(|e|err("sincronizar archivo",e))?;
        fs::rename(&temporary,dest).map_err(|e|err("publicar archivo",e))
    })();
    if result.is_err(){let _=fs::remove_file(temporary);}
    result
}
impl SourceStore {
    pub(crate) fn new(folder:PathBuf)->Self{Self{folder}}
    fn prepare(&self)->Result<(),ApiError>{
        ensure_regular(&self.folder)?;
        fs::create_dir_all(&self.folder).map_err(|e|err("crear carpeta",e))?;
        ensure_regular(&self.folder)?;
        #[cfg(unix)]{
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.folder,fs::Permissions::from_mode(0o700))
                .map_err(|e|err("proteger carpeta",e))?;
        }
        Ok(())
    }
    fn paths(&self,id:&str)->Result<(PathBuf,PathBuf),ApiError>{
        if !valid_id(id){return Err(bad("ID de fuente inseguro"));}
        Ok((self.folder.join(format!("{id}.zip")),self.folder.join(format!("{id}.json"))))
    }
    fn meta(&self,id:&str)->Result<SourceMeta,ApiError>{
        let (_,path)=self.paths(id)?;
        ensure_regular(&path)?;
        let data=fs::read(&path).map_err(|e|err("leer datos de fuente",e))?;
        if data.len()>4096{return Err(bad("Metadatos de pack demasiado grandes"));}
        let meta:SourceMeta=serde_json::from_slice(&data)
            .map_err(|e|bad(format!("Metadatos dañados: {e}")))?;
        if meta.schema!=SCHEMA||meta.id!=id||meta.zip_bytes==0||meta.zip_bytes>MAX_ZIP
            || meta.name.trim().is_empty()||meta.name.chars().count()>80
            || meta.name.chars().any(char::is_control)
            || !(-1000..=1000).contains(&meta.priority)
            || meta.fingerprint.len()!=64
            || !meta.fingerprint.bytes().all(|c|c.is_ascii_hexdigit()){
            return Err(bad("Metadatos de fuente inválidos"));
        }
        Ok(meta)
    }
    fn list_ids(&self)->Result<Vec<String>,ApiError>{
        let mut ids=Vec::new();
        let iter=match fs::read_dir(&self.folder){
            Ok(v)=>v,
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(ids),
            Err(e)=>return Err(err("listar fuentes",e)),
        };
        for entry in iter{
            let entry=entry.map_err(|e|err("listar archivos",e))?;
            let file=entry.file_name();
            if let Some(id)=file.to_str().and_then(|s|s.strip_suffix(".json")){
                if valid_id(id){ids.push(id.to_owned());}
            }
        }
        ids.sort();
        if ids.len()>MAX_PACKS{return Err(bad("Máximo 24 fuentes persistentes"));}
        Ok(ids)
    }
    pub(crate) fn save(&self,pack:&LoadedPack,zip:&[u8])->Result<(),ApiError>{
        self.prepare()?;
        if zip.is_empty()||zip.len()>MAX_ZIP{return Err(bad("ZIP supera 96 MiB"));}
        if format!("{:x}",Sha256::digest(zip))!=pack.fingerprint {
            return Err(bad("El ZIP no coincide con SHA-256 de la fuente"));
        }
        let (zip_path,meta_path)=self.paths(&pack.id)?;
        if meta_path.exists()||zip_path.exists() {
            return Err(ApiError(StatusCode::CONFLICT,"Ya existe la fuente persistida".into()));
        }
        let mut disk_total=0usize;
        for id in self.list_ids()? {
            let (path,_)=self.paths(&id)?;
            ensure_regular(&path)?;
            disk_total=disk_total.saturating_add(
                fs::metadata(&path).map_err(|e|err("medir archivos",e))?.len() as usize);
        }
        if disk_total.saturating_add(zip.len())>MAX_DISK {
            return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE,
                "VSPack supera el límite de 512 MiB de ZIPs persistidos".into()));
        }
        let meta=SourceMeta{schema:SCHEMA.into(),id:pack.id.clone(),
            name:pack.name.clone(),fingerprint:pack.fingerprint.clone(),
            priority:pack.priority,zip_bytes:zip.len()};
        // Atomic descriptor last: it is the commit marker for a complete ZIP.
        write_atomic(&self.folder,&zip_path,zip)?;
        let info=serde_json::to_vec(&meta).map_err(bad)?;
        if let Err(e)=write_atomic(&self.folder,&meta_path,&info){
            let _=fs::remove_file(zip_path);
            return Err(e);
        }
        Ok(())
    }
    pub(crate) fn priority(&self,id:&str,value:i32)->Result<(),ApiError>{
        let (_,path)=self.paths(id)?;
        let mut meta=self.meta(id)?;
        meta.priority=value;
        let bytes=serde_json::to_vec(&meta).map_err(bad)?;
        write_atomic(&self.folder,&path,&bytes)
    }
    pub(crate) fn remove(&self,id:&str)->Result<(),ApiError>{
        let (zip,meta)=self.paths(id)?;
        ensure_regular(&meta)?;ensure_regular(&zip)?;
        // Commit removal by deleting descriptor first; orphan ZIPs are never loaded.
        if meta.exists(){fs::remove_file(meta).map_err(|e|err("eliminar metadatos",e))?;}
        if zip.exists(){fs::remove_file(zip).map_err(|e|err("eliminar ZIP",e))?;}
        Ok(())
    }
    /// Boot recovery. A damaged source is SKIPPED, never trusted or published.
    pub(crate) fn restore(&self)->Result<(Vec<LoadedPack>,Vec<String>),ApiError>{
        self.prepare()?;
        let mut sources=Vec::new();
        let mut warnings=Vec::new();
        let mut fingerprints=BTreeSet::new();
        let mut total=0usize;
        for id in self.list_ids()?{
            let loaded=(|| -> Result<LoadedPack,ApiError> {
                let meta=self.meta(&id)?;
                let (path,_)=self.paths(&id)?;
                ensure_regular(&path)?;
                let length=fs::metadata(&path).map_err(|e|err("medir ZIP",e))?.len() as usize;
                if length!=meta.zip_bytes{return Err(bad("El ZIP persistido cambió de tamaño"));}
                let bytes=fs::read(&path).map_err(|e|err("leer ZIP",e))?;
                if format!("{:x}",Sha256::digest(&bytes))!=meta.fingerprint {
                    return Err(bad("Hash SHA-256 de ZIP persistido no coincide"));
                }
                let files=archive::import_zip(&bytes,&id,0,limits()).map_err(bad)?;
                let expanded=files.iter().map(|f|f.data.len()).sum::<usize>();
                if files.is_empty()||total.saturating_add(expanded)>MAX_MEMORY {
                    return Err(bad("El conjunto persistido excede 384 MiB en memoria"));
                }
                Ok(LoadedPack{id:id.clone(),name:meta.name,
                    priority:meta.priority,files:Arc::new(files),bytes:expanded,
                    fingerprint:meta.fingerprint})
            })();
            match loaded{
                Ok(pack)=>{
                    if fingerprints.insert(pack.fingerprint.clone()){
                        total+=pack.bytes;sources.push(pack);
                    }else{
                        warnings.push(format!("Fuente {id} duplicada: ignorada"));
                    }
                }
                Err(e)=>warnings.push(format!("Fuente {id} ignorada: {}",e.1)),
            }
        }
        Ok((sources,warnings))
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use vsmedia_pack::MergeOutput;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize,Ordering};
    static COUNTER:AtomicUsize=AtomicUsize::new(0);
    fn fixture()->Vec<u8>{
        let files=BTreeMap::from([
            ("pack.mcmeta".to_string(),
                br#"{"pack":{"pack_format":1,"description":"demo"}}"#.to_vec()),
            ("assets/vs/lang/es_ar.json".to_string(),b"{}".to_vec()),
        ]);
        archive::export_zip(&MergeOutput{files,conflicts:vec![]},limits()).unwrap()
    }
    fn folder()->PathBuf{
        std::env::temp_dir().join(format!("vspack-zip-restart-{}-{}",
            std::process::id(),COUNTER.fetch_add(1,Ordering::Relaxed)))
    }
    #[test]
    fn restart_restores_original_zip_and_priority(){
        let dir=folder();let store=SourceStore::new(dir.clone());
        let zip=fixture();
        let files=archive::import_zip(&zip,"gui",0,limits()).unwrap();
        let pack=LoadedPack{id:"gui".into(),name:"GUI".into(),
            priority:15,bytes:files.iter().map(|f|f.data.len()).sum(),
            files:Arc::new(files),fingerprint:format!("{:x}",Sha256::digest(&zip))};
        store.save(&pack,&zip).unwrap();
        let recovered=SourceStore::new(dir.clone());
        let (restored,warnings)=recovered.restore().unwrap();
        assert!(warnings.is_empty());
        assert_eq!(restored.len(),1);
        assert_eq!(restored[0].fingerprint,pack.fingerprint);
        assert_eq!(restored[0].priority,15);
        recovered.priority("gui",33).unwrap();
        assert_eq!(recovered.restore().unwrap().0[0].priority,33);
        recovered.remove("gui").unwrap();
        assert!(recovered.restore().unwrap().0.is_empty());
        let _=fs::remove_dir_all(dir);
    }
    #[test]
    fn tampered_source_is_not_restored(){
        let dir=folder();let store=SourceStore::new(dir.clone());
        let zip=fixture();
        let files=archive::import_zip(&zip,"gui",0,limits()).unwrap();
        let pack=LoadedPack{id:"gui".into(),name:"GUI".into(),
            priority:0,bytes:files.iter().map(|f|f.data.len()).sum(),
            files:Arc::new(files),fingerprint:format!("{:x}",Sha256::digest(&zip))};
        store.save(&pack,&zip).unwrap();
        fs::write(dir.join("gui.zip"),b"tampered").unwrap();
        let (restored,warnings)=store.restore().unwrap();
        assert!(restored.is_empty());assert_eq!(warnings.len(),1);
        let _=fs::remove_dir_all(dir);
    }
}
