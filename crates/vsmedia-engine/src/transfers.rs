//! Bounded download sessions for the local IPC transport.
//! One validated read per session, then sequential chunks from in-memory PCM/assets.
//! This avoids re-reading and re-hashing an entire 64 MiB object for each chunk.
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use uuid::Uuid;
use vsmedia_api::{AssetKey, AssetRepository, MediaError, MediaResult};
use vsmedia_core::sqlite::SqliteAssetStore;

pub const CHUNK_BYTES: usize = 256 * 1024;
pub const MAX_TRANSFER_BYTES: usize = 64 * 1024 * 1024;
const MAX_STAGED_BYTES: usize = 128 * 1024 * 1024;
const MAX_SESSIONS: usize = 4;
const SESSION_TTL: Duration = Duration::from_secs(120);

struct Download {
    owner: String,
    key: AssetKey,
    data: Vec<u8>,
    next_offset: u64,
    touched: Instant,
}
#[derive(Default)]
pub struct DownloadRegistry {
    sessions: Mutex<HashMap<String, Download>>,
}
impl DownloadRegistry {
    fn prune(sessions: &mut HashMap<String, Download>) {
        sessions.retain(|_, entry| entry.touched.elapsed() < SESSION_TTL);
    }
    pub fn begin(&self, store: &SqliteAssetStore, owner: &str, key: &AssetKey)
        -> MediaResult<(String, u64, String)>
    {
        // Existing SqliteStore::read validates SHA-256 once, before serving
        // any bytes to the caller. ACL checks are performed by the dispatcher.
        let record = store.metadata(key)?;
        if record.size > MAX_TRANSFER_BYTES as u64 {
            return Err(MediaError::InvalidInput("asset exceeds IPC download limit".into()));
        }
        let data = store.read(key)?;
        if data.len() as u64 != record.size {
            return Err(MediaError::Conflict("asset changed during download setup".into()));
        }
        let mut sessions = self.sessions.lock()
            .map_err(|_| MediaError::Io("download registry poisoned".into()))?;
        Self::prune(&mut sessions);
        let occupied: usize = sessions.values().map(|entry|entry.data.len()).sum();
        if sessions.len() >= MAX_SESSIONS || occupied + data.len() > MAX_STAGED_BYTES {
            return Err(MediaError::Conflict("download session memory quota exceeded".into()));
        }
        let transfer_id = Uuid::new_v4().to_string();
        let size = data.len() as u64;
        sessions.insert(transfer_id.clone(), Download {
            owner: owner.into(), key: key.clone(), data,
            next_offset: 0, touched: Instant::now(),
        });
        Ok((transfer_id, size, record.sha256))
    }
    pub fn key(&self, owner: &str, id: &str) -> MediaResult<AssetKey> {
        let mut sessions = self.sessions.lock()
            .map_err(|_| MediaError::Io("download registry poisoned".into()))?;
        Self::prune(&mut sessions);
        let session = sessions.get(id)
            .filter(|v|v.owner == owner)
            .ok_or_else(|| MediaError::NotFound("download session".into()))?;
        Ok(session.key.clone())
    }
    pub fn chunk(&self, owner: &str, id: &str, offset: u64)
        -> MediaResult<(Vec<u8>, bool)>
    {
        let mut sessions = self.sessions.lock()
            .map_err(|_| MediaError::Io("download registry poisoned".into()))?;
        Self::prune(&mut sessions);
        let session = sessions.get_mut(id)
            .filter(|v|v.owner == owner)
            .ok_or_else(|| MediaError::NotFound("download session".into()))?;
        if offset != session.next_offset {
            return Err(MediaError::Conflict("unexpected download offset".into()));
        }
        let position = usize::try_from(offset)
            .map_err(|_|MediaError::InvalidInput("offset overflow".into()))?;
        let end = position.saturating_add(CHUNK_BYTES).min(session.data.len());
        let bytes = session.data[position..end].to_vec();
        session.next_offset = end as u64;
        session.touched = Instant::now();
        Ok((bytes, end == session.data.len()))
    }
    pub fn end(&self, owner: &str, id: &str) -> MediaResult<()> {
        let mut sessions = self.sessions.lock()
            .map_err(|_| MediaError::Io("download registry poisoned".into()))?;
        Self::prune(&mut sessions);
        if sessions.get(id).is_none_or(|v|v.owner != owner) {
            return Err(MediaError::NotFound("download session".into()));
        }
        sessions.remove(id);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::atomic::{AtomicU64, Ordering}};
    use vsmedia_api::{AssetKind, AssetRepository};
    fn path()->std::path::PathBuf {
        static N:AtomicU64=AtomicU64::new(0);
        std::env::temp_dir().join(format!("vsmedia-download-{}-{}",
            std::process::id(),N.fetch_add(1,Ordering::Relaxed)))
    }
    #[test]
    fn large_asset_is_read_in_chunks_with_exact_bytes() {
        let root=path();
        let db=SqliteAssetStore::open(&root).unwrap();
        let key=AssetKey::new("audio","recording").unwrap();
        let original=vec![53u8; CHUNK_BYTES*3+11];
        db.publish(key.clone(),AssetKind::Sound,&original).unwrap();
        let sessions=DownloadRegistry::default();
        let (id,size,hash)=sessions.begin(&db,"audio_player",&key).unwrap();
        assert_eq!(size,original.len() as u64);
        assert_eq!(hash,db.metadata(&key).unwrap().sha256);
        assert!(sessions.chunk("other_producer",&id,0).is_err());
        let mut received=Vec::new();
        let mut offset=0;
        while offset<size {
            let (bytes,done)=sessions.chunk("audio_player",&id,offset).unwrap();
            assert!(!bytes.is_empty());
            offset+=bytes.len() as u64;
            received.extend(bytes);
            if done { break; }
        }
        assert_eq!(received,original);
        assert!(sessions.chunk("audio_player",&id,0).is_err());
        sessions.end("audio_player",&id).unwrap();
        assert!(sessions.key("audio_player",&id).is_err());
        drop(db);
        let _=fs::remove_dir_all(root);
    }
    #[test]
    fn staged_memory_and_session_count_are_bounded() {
        let root=path();
        let db=SqliteAssetStore::open(&root).unwrap();
        let sessions=DownloadRegistry::default();
        for n in 0..4 {
            let key=AssetKey::new("test",format!("asset-{n}")).unwrap();
            db.publish(key.clone(),AssetKind::Sound,&[7;32]).unwrap();
            sessions.begin(&db,"producer",&key).unwrap();
        }
        let last=AssetKey::new("test","extra").unwrap();
        db.publish(last.clone(),AssetKind::Sound,b"extra").unwrap();
        assert!(sessions.begin(&db,"producer",&last).is_err());
        drop(db);
        let _=fs::remove_dir_all(root);
    }
}
