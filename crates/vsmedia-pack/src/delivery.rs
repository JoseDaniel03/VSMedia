//! Immutable static resource pack publishing. Minecraft downloads over HTTPS
//! from an external static server/CDN; the private IPC is never exposed.
use crate::archive::{import_zip, ArchiveLimits};
use serde::{Deserialize, Serialize};
use sha1::{Digest as _, Sha1};
use sha2::Sha256;
use std::{fs::{self, File, OpenOptions}, io::{self, Write}, path::Path};
use url::Url;
use uuid::Uuid;
use vsmedia_api::{MediaError, MediaResult};

pub const MAX_DELIVERY_ZIP: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryDescriptor {
    pub logical_id: String,
    pub uuid: String,
    pub sha1: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub filename: String,
    pub url: String,
}
fn invalid(message:&str) -> MediaError { MediaError::InvalidInput(message.into()) }
fn io_error(e:impl std::fmt::Display) -> MediaError { MediaError::Io(e.to_string()) }

/// Publish an already assembled ZIP into an admin-controlled static directory.
/// Each filename is its SHA-256 so previous URLs can remain cached indefinitely.
/// SHA-1 is separately calculated for the Minecraft client-side cache.
pub fn stage(zip_bytes:&[u8], logical_id:&str, static_dir:&Path, base_url:&str)
    -> MediaResult<DeliveryDescriptor>
{
    if logical_id.is_empty() || logical_id.len()>64 ||
        !logical_id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() ||
            b==b'_' || b==b'-')
    {
        return Err(invalid("invalid logical resource pack ID"));
    }
    if zip_bytes.is_empty() || zip_bytes.len()>MAX_DELIVERY_ZIP {
        return Err(invalid("resource-pack ZIP exceeds 64 MiB quota"));
    }
    let base=Url::parse(base_url).map_err(io_error)?;
    if base.scheme()!="https" || base.host_str().is_none() ||
        !base.username().is_empty() || base.password().is_some() ||
        base.fragment().is_some() || base.query().is_some() ||
        !base.path().ends_with('/')
    {
        return Err(invalid("public base URL must be an HTTPS directory URL"));
    }
    // Strict ZIP validation happens before the file becomes public.
    let files=import_zip(zip_bytes,"delivery",0,ArchiveLimits::default())?;
    let meta=files.iter().find(|x|x.path=="pack.mcmeta")
        .ok_or_else(||invalid("pack.mcmeta missing"))?;
    let parsed:serde_json::Value=serde_json::from_slice(&meta.data)
        .map_err(|_|invalid("pack.mcmeta is invalid JSON"))?;
    if !parsed.get("pack").is_some_and(serde_json::Value::is_object) {
        return Err(invalid("pack.mcmeta lacks pack object"));
    }
    let sha256=format!("{:x}",Sha256::digest(zip_bytes));
    let sha1=format!("{:x}",Sha1::digest(zip_bytes));
    let filename=format!("{sha256}.zip");
    let url=base.join(&filename).map_err(io_error)?.to_string();
    // A stable slot UUID identifies this pack; its SHA-1 changes on updates.
    let uuid=Uuid::new_v5(&Uuid::NAMESPACE_URL,
        format!("vsmedia:resource-pack:{logical_id}").as_bytes()).to_string();
    fs::create_dir_all(static_dir).map_err(io_error)?;
    let root=fs::canonicalize(static_dir).map_err(io_error)?;
    let target=root.join(&filename);
    if target.exists() {
        verify_existing(&target,&sha256,zip_bytes.len())?;
    } else {
        // Creating the hash-named final file with a hard link is atomic:
        // public HTTP servers cannot observe a partially written ZIP.
        let tmp=root.join(format!(".{}.tmp",Uuid::new_v4()));
        let outcome=(|| -> MediaResult<()> {
            let mut file=OpenOptions::new().create_new(true).write(true)
                .open(&tmp).map_err(io_error)?;
            file.write_all(zip_bytes).map_err(io_error)?;
            file.sync_all().map_err(io_error)?;
            drop(file);
            match fs::hard_link(&tmp,&target) {
                Ok(()) => Ok(()),
                Err(e) if e.kind()==io::ErrorKind::AlreadyExists =>
                    verify_existing(&target,&sha256,zip_bytes.len()),
                Err(e) => Err(io_error(e)),
            }
        })();
        let _=fs::remove_file(&tmp);
        outcome?;
    }
    Ok(DeliveryDescriptor {
        logical_id:logical_id.into(),uuid,sha1,sha256,
        size_bytes:zip_bytes.len() as u64,filename,url,
    })
}
fn verify_existing(path:&Path, expected:&str, len:usize) -> MediaResult<()> {
    let meta=fs::symlink_metadata(path).map_err(io_error)?;
    if !meta.file_type().is_file() || meta.len()!=len as u64 {
        return Err(MediaError::Conflict("invalid published resource pack".into()));
    }
    let mut file=File::open(path).map_err(io_error)?;
    let mut hash=Sha256::new();
    io::copy(&mut file,&mut hash).map_err(io_error)?;
    if format!("{:x}",hash.finalize())!=expected {
        return Err(MediaError::Conflict("published resource pack checksum mismatch".into()));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{archive::export_zip,MergeOutput};
    use std::{collections::BTreeMap,sync::atomic::{AtomicUsize,Ordering},path::PathBuf};
    fn temp()->PathBuf {
        static NEXT:AtomicUsize=AtomicUsize::new(0);
        std::env::temp_dir().join(format!("vsmedia-delivery-{}-{}",
            std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)))
    }
    fn pack(name:&str)->Vec<u8> {
        let output=MergeOutput { files:BTreeMap::from([
            ("pack.mcmeta".into(),
                format!(r#"{{"pack":{{"description":"{name}"}}}}"#).into_bytes()),
            ("assets/demo/models/test.json".into(),b"{}".to_vec()),
        ]),conflicts:vec![] };
        export_zip(&output,ArchiveLimits::default()).unwrap()
    }
    #[test]
    fn unchanged_pack_keeps_url_sha1_and_uuid_and_update_only_changes_hash() {
        let dir=temp();
        let first_bytes=pack("First");
        let a=stage(&first_bytes,"virgoland",&dir,"https://packs.example.test/").unwrap();
        let b=stage(&first_bytes,"virgoland",&dir,"https://packs.example.test/").unwrap();
        assert_eq!(a,b);
        assert_eq!(a.sha1.len(),40);
        assert_eq!(a.sha256.len(),64);
        assert_eq!(fs::read(dir.join(&a.filename)).unwrap(),first_bytes);
        assert_eq!(fs::read_dir(&dir).unwrap().count(),1);
        let c=stage(&pack("Updated"),"virgoland",&dir,
            "https://packs.example.test/").unwrap();
        assert_ne!(a.sha1,c.sha1);
        assert_ne!(a.url,c.url);
        assert_eq!(a.uuid,c.uuid);
        let _=fs::remove_dir_all(dir);
    }
    #[test]
    fn rejects_unsafe_urls_malformed_packs_and_changed_existing_files() {
        let dir=temp();
        let data=pack("Test");
        assert!(stage(&data,"demo",&dir,"http://example.test/").is_err());
        assert!(stage(&data,"demo",&dir,"https://example.test/?secret=abc").is_err());
        assert!(stage(&data,"demo",&dir,"https://user:secret@example.test/").is_err());
        assert!(stage(&data,"Bad/ID",&dir,"https://example.test/").is_err());
        assert!(stage(b"broken ZIP","demo",&dir,"https://example.test/").is_err());
        assert!(!dir.exists());
        let first=stage(&data,"demo",&dir,"https://example.test/").unwrap();
        fs::write(dir.join(&first.filename),b"corrupt").unwrap();
        assert!(stage(&data,"demo",&dir,"https://example.test/").is_err());
        let _=fs::remove_dir_all(dir);
    }
}
