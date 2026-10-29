//! VSMedia Core: content-addressed storage and bounded background processing.
//! The initial FsAssetStore is retained only for test migration; new services use SQLite.
pub mod jobs;
pub mod sqlite;

use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io, path::{Path, PathBuf}, sync::Mutex};
use vsmedia_api::{AssetKey, AssetKind, AssetRecord, AssetRepository, MediaError, MediaResult};

/// Legacy development backend, not safe for multiple processes.
#[deprecated(note = "use sqlite::SqliteAssetStore for new code")]
pub struct FsAssetStore {
    root: PathBuf,
    index: Mutex<BTreeMap<String, AssetRecord>>,
}
#[allow(deprecated)]
impl FsAssetStore {
    pub fn open(root: impl AsRef<Path>) -> MediaResult<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("objects")).map_err(io_error)?;
        let index_file = root.join("index.json");
        let index = if index_file.exists() {
            serde_json::from_slice(&fs::read(index_file).map_err(io_error)?)
                .map_err(|e| MediaError::Io(e.to_string()))?
        } else {
            BTreeMap::new()
        };
        Ok(Self { root, index: Mutex::new(index) })
    }
    fn object_path(&self, hash: &str) -> PathBuf { self.root.join("objects").join(hash) }
    fn record(&self, key: &AssetKey) -> MediaResult<AssetRecord> {
        key.validated()?;
        self.index.lock().map_err(|e| MediaError::Io(e.to_string()))?
            .get(&key.qualified()).cloned()
            .ok_or_else(|| MediaError::NotFound(key.qualified()))
    }
}
#[allow(deprecated)]
impl AssetRepository for FsAssetStore {
    fn publish(&self, key: AssetKey, kind: AssetKind, bytes: &[u8]) -> MediaResult<AssetRecord> {
        key.validated()?;
        let hash = format!("{:x}", Sha256::digest(bytes));
        let object_path = self.object_path(&hash);
        if !object_path.exists() {
            match fs::OpenOptions::new().write(true).create_new(true).open(&object_path) {
                Ok(mut file) => {
                    use io::Write;
                    if let Err(err) = file.write_all(bytes) {
                        let _ = fs::remove_file(&object_path);
                        return Err(io_error(err));
                    }
                    file.sync_all().map_err(io_error)?;
                }
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
                Err(err) => return Err(io_error(err)),
            }
        }
        let mut index = self.index.lock().map_err(|e| MediaError::Io(e.to_string()))?;
        let revision = index.get(&key.qualified()).map_or(1, |r| r.revision + 1);
        let record = AssetRecord {
            key: key.clone(), kind, sha256: hash, size: bytes.len() as u64, revision
        };
        let mut next = index.clone();
        next.insert(key.qualified(), record.clone());
        let payload = serde_json::to_vec_pretty(&next)
            .map_err(|e| MediaError::Io(e.to_string()))?;
        let staged = self.root.join("index.json.next");
        fs::write(&staged, payload).map_err(io_error)?;
        fs::rename(staged, self.root.join("index.json")).map_err(io_error)?;
        *index = next;
        Ok(record)
    }
    fn metadata(&self, key: &AssetKey) -> MediaResult<AssetRecord> { self.record(key) }
    fn read(&self, key: &AssetKey) -> MediaResult<Vec<u8>> {
        let record = self.record(key)?;
        let bytes = fs::read(self.object_path(&record.sha256)).map_err(io_error)?;
        if format!("{:x}", Sha256::digest(&bytes)) != record.sha256 {
            return Err(MediaError::Io("asset checksum mismatch".into()));
        }
        Ok(bytes)
    }
}
fn io_error(err: io::Error) -> MediaError { MediaError::Io(err.to_string()) }
#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    #[test]
    fn publish_survives_reopen_with_checksum() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("vsmedia-test-{}-{stamp}", std::process::id()));
        let key = AssetKey::new("demo", "sounds/intro.ogg").unwrap();
        {
            let db = FsAssetStore::open(&root).unwrap();
            let saved = db.publish(key.clone(), AssetKind::Sound, b"sample").unwrap();
            assert_eq!(saved.revision, 1);
            assert_eq!(db.read(&key).unwrap(), b"sample");
        }
        {
            let db = FsAssetStore::open(&root).unwrap();
            assert_eq!(db.metadata(&key).unwrap().revision, 1);
            assert_eq!(db.publish(key.clone(), AssetKind::Sound, b"sample2").unwrap().revision, 2);
            assert_eq!(db.read(&key).unwrap(), b"sample2");
        }
        let _ = fs::remove_dir_all(root);
    }
}
