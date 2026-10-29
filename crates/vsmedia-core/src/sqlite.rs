//! Transactional SQLite index with an immutable content-addressed object store.
//! All metadata updates are transactions; assets are never served without checksum validation.
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{atomic::{AtomicU64, Ordering}, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use vsmedia_api::{AccessControlledRepository, AssetAccess, AssetImport, AssetKey, AssetKind, AssetRecord, AssetRepository, AtomicAssetRepository, AuthorizedAtomicAssetRepository, MediaError, MediaResult};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
pub const DEFAULT_MAX_ASSET_BYTES: usize = 64 * 1024 * 1024;

pub struct SqliteAssetStore {
    root: PathBuf,
    database: Mutex<Connection>,
    max_asset_bytes: usize,
}
impl SqliteAssetStore {
    pub fn open(root: impl AsRef<Path>) -> MediaResult<Self> {
        Self::with_max_asset_bytes(root, DEFAULT_MAX_ASSET_BYTES)
    }
    pub fn with_max_asset_bytes(root: impl AsRef<Path>, max_asset_bytes: usize) -> MediaResult<Self> {
        if max_asset_bytes == 0 || max_asset_bytes > 1024 * 1024 * 1024 {
            return Err(MediaError::InvalidInput("invalid per-asset size quota".into()));
        }
        fs::create_dir_all(root.as_ref().join("objects")).map_err(io_error)?;
        let root = fs::canonicalize(root).map_err(io_error)?;
        let db = Connection::open(root.join("catalog.sqlite")).map_err(db_error)?;
        db.busy_timeout(std::time::Duration::from_secs(5)).map_err(db_error)?;
        db.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .map_err(db_error)?;
        const SCHEMA_VERSION: i64 = 1;
        let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(db_error)?;
        if !(0..=SCHEMA_VERSION).contains(&version) {
            return Err(MediaError::Conflict(format!(
                "unsupported SQLite catalog schema version: {version}"
            )));
        }
        // Version 0 includes existing VSMedia catalogs created before tracking
        // schema versions. All CREATE statements are safe to replay.
        db.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS assets (
               namespace TEXT NOT NULL,
               name TEXT NOT NULL,
               kind TEXT NOT NULL,
               sha256 TEXT NOT NULL,
               size INTEGER NOT NULL,
               revision INTEGER NOT NULL,
               PRIMARY KEY(namespace, name)
             );
             CREATE TABLE IF NOT EXISTS asset_versions (
               namespace TEXT NOT NULL,
               name TEXT NOT NULL,
               revision INTEGER NOT NULL,
               kind TEXT NOT NULL,
               sha256 TEXT NOT NULL,
               size INTEGER NOT NULL,
               PRIMARY KEY(namespace, name, revision)
             );
             CREATE TABLE IF NOT EXISTS producers (
               producer TEXT NOT NULL,
               namespace TEXT NOT NULL,
               can_publish INTEGER NOT NULL,
               can_read INTEGER NOT NULL,
               PRIMARY KEY(producer, namespace)
             );
             PRAGMA user_version = 1;
             COMMIT;"
        ).map_err(db_error)?;
        Ok(Self { root, database: Mutex::new(db), max_asset_bytes })
    }

    /// Only privileged administration code may grant producer permissions.
    pub fn grant(&self, producer: &str, namespace: &str, publish: bool, read: bool) -> MediaResult<()> {
        validate_producer(producer)?;
        AssetKey::new(namespace, "permission")?;
        let conn = self.database.lock().map_err(lock_error)?;
        conn.execute(
            "INSERT INTO producers(producer, namespace, can_publish, can_read)
             VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(producer, namespace) DO UPDATE SET
             can_publish=excluded.can_publish, can_read=excluded.can_read",
            params![producer, namespace, publish, read],
        ).map_err(db_error)?;
        Ok(())
    }

    pub fn authorize(&self, producer: &str, namespace: &str, write: bool) -> MediaResult<()> {
        validate_producer(producer)?;
        AssetKey::new(namespace, "permission")?;
        let conn = self.database.lock().map_err(lock_error)?;
        let permissions: Option<(bool, bool)> = conn.query_row(
            "SELECT can_publish,can_read FROM producers WHERE producer=?1 AND namespace=?2",
            params![producer, namespace],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional().map_err(db_error)?;
        match permissions {
            Some((can_publish, can_read)) if if write { can_publish } else { can_read } => Ok(()),
            _ => Err(MediaError::InvalidInput("producer lacks permission for namespace".into())),
        }
    }

    pub fn history(&self, key: &AssetKey) -> MediaResult<Vec<AssetRecord>> {
        key.validated()?;
        let conn = self.database.lock().map_err(lock_error)?;
        let mut stmt = conn.prepare(
            "SELECT kind,sha256,size,revision FROM asset_versions
             WHERE namespace=?1 AND name=?2 ORDER BY revision DESC LIMIT 100"
        ).map_err(db_error)?;
        let rows = stmt.query_map(params![key.namespace, key.name], |row| {
            let kind: String = row.get(0)?;
            Ok((kind, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?))
        }).map_err(db_error)?;
        let mut out = Vec::new();
        for row in rows {
            let (kind, sha256, size, revision) = row.map_err(db_error)?;
            out.push(AssetRecord {
                key: key.clone(),
                kind: serde_json::from_str(&kind).map_err(|e| MediaError::Io(e.to_string()))?,
                sha256,
                size: size as u64,
                revision: revision as u64,
            });
        }
        Ok(out)
    }

    fn object_path(&self, hash: &str) -> PathBuf {
        self.root.join("objects").join(hash)
    }

    fn persist_blob(&self, hash: &str, bytes: &[u8]) -> MediaResult<()> {
        let final_path = self.object_path(hash);
        if final_path.exists() { return Ok(()); }
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)
            .map_err(|e| MediaError::Io(e.to_string()))?.as_nanos();
        let tmp = self.root.join("objects").join(format!(
            ".incoming-{}-{}-{stamp}", std::process::id(), TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> MediaResult<()> {
            let mut file = OpenOptions::new().write(true).create_new(true)
                .open(&tmp).map_err(io_error)?;
            file.write_all(bytes).map_err(io_error)?;
            file.sync_all().map_err(io_error)?;
            // Atomic replacement is safe: identical hash means identical contents.
            fs::rename(&tmp, &final_path).map_err(io_error)?;
            Ok(())
        })();
        if result.is_err() { let _ = fs::remove_file(&tmp); }
        result
    }

    fn lookup(&self, key: &AssetKey) -> MediaResult<AssetRecord> {
        key.validated()?;
        let conn = self.database.lock().map_err(lock_error)?;
        let row: Option<(String, String, i64, i64)> = conn.query_row(
            "SELECT kind,sha256,size,revision FROM assets WHERE namespace=?1 AND name=?2",
            params![key.namespace, key.name],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).optional().map_err(db_error)?;
        let (kind, sha256, size, revision) = row.ok_or_else(|| MediaError::NotFound(key.qualified()))?;
        Ok(AssetRecord {
            key: key.clone(),
            kind: serde_json::from_str(&kind).map_err(|e| MediaError::Io(e.to_string()))?,
            sha256,
            size: size as u64,
            revision: revision as u64,
        })
    }
}
impl AssetRepository for SqliteAssetStore {
    fn publish(&self, key: AssetKey, kind: AssetKind, bytes: &[u8]) -> MediaResult<AssetRecord> {
        key.validated()?;
        if bytes.len() > self.max_asset_bytes {
            return Err(MediaError::InvalidInput("asset exceeds size quota".into()));
        }
        let hash = format!("{:x}", Sha256::digest(bytes));
        let kind_text = serde_json::to_string(&kind)
            .map_err(|e| MediaError::Io(e.to_string()))?;
        self.persist_blob(&hash, bytes)?;
        let mut conn = self.database.lock().map_err(lock_error)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
        let old: Option<i64> = tx.query_row(
            "SELECT revision FROM assets WHERE namespace=?1 AND name=?2",
            params![key.namespace, key.name], |r| r.get(0),
        ).optional().map_err(db_error)?;
        let revision = old.unwrap_or(0).checked_add(1)
            .ok_or_else(|| MediaError::Io("asset revision overflow".into()))?;
        tx.execute(
            "INSERT INTO asset_versions(namespace,name,revision,kind,sha256,size)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![key.namespace, key.name, revision, kind_text, hash, bytes.len() as i64],
        ).map_err(db_error)?;
        tx.execute(
            "INSERT INTO assets(namespace,name,kind,sha256,size,revision)
             VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(namespace,name) DO UPDATE SET
             kind=excluded.kind, sha256=excluded.sha256,
             size=excluded.size, revision=excluded.revision",
            params![key.namespace, key.name, kind_text, hash, bytes.len() as i64, revision],
        ).map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        Ok(AssetRecord { key, kind, sha256: hash, size: bytes.len() as u64, revision: revision as u64 })
    }
    fn metadata(&self, key: &AssetKey) -> MediaResult<AssetRecord> {
        self.lookup(key)
    }
    fn read(&self, key: &AssetKey) -> MediaResult<Vec<u8>> {
        let record = self.lookup(key)?;
        if record.size > self.max_asset_bytes as u64 {
            return Err(MediaError::InvalidInput("asset exceeds size quota".into()));
        }
        let bytes = fs::read(self.object_path(&record.sha256)).map_err(io_error)?;
        if bytes.len() as u64 != record.size || format!("{:x}", Sha256::digest(&bytes)) != record.sha256 {
            return Err(MediaError::Io("asset checksum or size mismatch".into()));
        }
        Ok(bytes)
    }
}
fn validate_producer(p: &str) -> MediaResult<()> {
    if p.is_empty() || p.len() > 64 ||
        !p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-')) {
        return Err(MediaError::InvalidInput("invalid producer ID".into()));
    }
    Ok(())
}
fn lock_error<E: std::fmt::Display>(e: E) -> MediaError { MediaError::Io(e.to_string()) }
fn db_error(e: rusqlite::Error) -> MediaError { MediaError::Io(e.to_string()) }
fn io_error(e: io::Error) -> MediaError { MediaError::Io(e.to_string()) }

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "vsmedia-db-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)
        ))
    }
    #[test]
    fn stores_versions_and_survives_restart() {
        let root = root();
        let key = AssetKey::new("demo", "sounds/voice.ogg").unwrap();
        {
            let db = SqliteAssetStore::open(&root).unwrap();
            assert_eq!(db.publish(key.clone(), AssetKind::Sound, b"one").unwrap().revision, 1);
            assert_eq!(db.publish(key.clone(), AssetKind::Sound, b"two").unwrap().revision, 2);
            assert_eq!(db.history(&key).unwrap().len(), 2);
        }
        let db = SqliteAssetStore::open(&root).unwrap();
        assert_eq!(db.read(&key).unwrap(), b"two");
        assert_eq!(db.history(&key).unwrap()[1].revision, 1);
        drop(db);
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn catalog_upgrades_legacy_version_zero_and_rejects_unknown_future_versions() {
        let root=root();
        let db=SqliteAssetStore::open(&root).unwrap();
        let key=AssetKey::new("demo","asset").unwrap();
        db.publish(key.clone(),AssetKind::Sound,b"preserved").unwrap();
        drop(db);
        {
            let raw=Connection::open(root.join("catalog.sqlite")).unwrap();
            raw.pragma_update(None,"user_version",0).unwrap();
        }
        let reopened=SqliteAssetStore::open(&root).unwrap();
        assert_eq!(reopened.read(&key).unwrap(),b"preserved");
        drop(reopened);
        {
            let raw=Connection::open(root.join("catalog.sqlite")).unwrap();
            let version:i64=raw.query_row("PRAGMA user_version",[],|row|row.get(0)).unwrap();
            assert_eq!(version,1);
            raw.pragma_update(None,"user_version",99).unwrap();
        }
        assert!(SqliteAssetStore::open(&root).is_err());
        let _=fs::remove_dir_all(root);
    }
    #[test]
    fn permissions_fail_closed_and_quota_is_enforced() {
        let root = root();
        let db = SqliteAssetStore::with_max_asset_bytes(&root, 3).unwrap();
        assert!(db.authorize("plugin", "mine", true).is_err());
        db.grant("plugin", "mine", true, false).unwrap();
        assert!(db.authorize("plugin", "mine", true).is_ok());
        assert!(db.authorize("plugin", "mine", false).is_err());
        assert!(db.publish(AssetKey::new("mine","x").unwrap(), AssetKind::Sound, b"oversized").is_err());
        drop(db);
        let _ = fs::remove_dir_all(root);
    }
}


/// The content objects may leave harmless orphans after a crash, but the
/// transaction guarantees the catalog is either fully updated or unchanged.
/// Concurrent imports are serialized by the SQLite IMMEDIATE transaction.
impl SqliteAssetStore {
    fn import_batch(&self, inputs: &[AssetImport], producer: Option<&str>) -> MediaResult<Vec<AssetRecord>> {
        const MAX_IMPORT_OBJECTS: usize = 512;
        const MAX_IMPORT_BYTES: usize = 128 * 1024 * 1024;
        if inputs.is_empty() || inputs.len() > MAX_IMPORT_OBJECTS {
            return Err(MediaError::InvalidInput("invalid import batch length".into()));
        }
        let mut identifiers = std::collections::BTreeSet::new();
        let mut total = 0usize;
        let mut hashes = Vec::with_capacity(inputs.len());
        for input in inputs {
            input.key.validated()?;
            if !identifiers.insert(&input.key) {
                return Err(MediaError::Conflict("duplicate asset in import batch".into()));
            }
            total = total.checked_add(input.bytes.len())
                .ok_or_else(|| MediaError::InvalidInput("import byte count overflow".into()))?;
            if total > MAX_IMPORT_BYTES || input.bytes.len() > self.max_asset_bytes {
                return Err(MediaError::InvalidInput("import exceeds configured quota".into()));
            }
            hashes.push(format!("{:x}", Sha256::digest(&input.bytes)));
        }

        let mut connection = self.database.lock().map_err(lock_error)?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        if let Some(producer) = producer {
            validate_producer(producer)?;
            let mut checked = std::collections::BTreeSet::new();
            for item in inputs {
                if checked.insert(&item.key.namespace) {
                    let permission: Option<bool> = tx.query_row(
                        "SELECT can_publish FROM producers WHERE producer=?1 AND namespace=?2",
                        params![producer, item.key.namespace],
                        |row| row.get(0),
                    ).optional().map_err(db_error)?;
                    if permission != Some(true) {
                        return Err(MediaError::InvalidInput(
                            "producer lacks permission for import namespace".into()
                        ));
                    }
                }
            }
        }
        let mut records = Vec::with_capacity(inputs.len());
        // All conflicts are detected before a single new catalog entry is committed.
        for (input, hash) in inputs.iter().zip(&hashes) {
            let existing: Option<(String, String, i64, i64)> = tx.query_row(
                "SELECT kind,sha256,size,revision FROM assets WHERE namespace=?1 AND name=?2",
                params![input.key.namespace, input.key.name],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
            ).optional().map_err(db_error)?;
            if let Some((kind, old_hash, size, revision)) = existing {
                let expected_kind = serde_json::to_string(&input.kind)
                    .map_err(|e| MediaError::Io(e.to_string()))?;
                if kind != expected_kind || old_hash != *hash || size != input.bytes.len() as i64 {
                    return Err(MediaError::Conflict(format!(
                        "import would replace an existing asset: {}", input.key.qualified()
                    )));
                }
                records.push(AssetRecord {
                    key: input.key.clone(), kind: input.kind,
                    sha256: old_hash, size: size as u64, revision: revision as u64,
                });
            } else {
                records.push(AssetRecord {
                    key: input.key.clone(), kind: input.kind,
                    sha256: hash.clone(), size: input.bytes.len() as u64, revision: 1,
                });
            }
        }
        for (input, record) in inputs.iter().zip(&records) {
            if record.revision != 1 || tx.query_row(
                "SELECT 1 FROM assets WHERE namespace=?1 AND name=?2",
                params![input.key.namespace,input.key.name], |row| row.get::<_, i64>(0)
            ).optional().map_err(db_error)?.is_some() {
                continue;
            }
            self.persist_blob(&record.sha256, &input.bytes)?;
            let serialized_kind = serde_json::to_string(&input.kind)
                .map_err(|e| MediaError::Io(e.to_string()))?;
            tx.execute(
                "INSERT INTO assets(namespace,name,kind,sha256,size,revision) VALUES(?1,?2,?3,?4,?5,1)",
                params![input.key.namespace,input.key.name,serialized_kind,record.sha256,record.size as i64]
            ).map_err(db_error)?;
            tx.execute(
                "INSERT INTO asset_versions(namespace,name,revision,kind,sha256,size) VALUES(?1,?2,1,?3,?4,?5)",
                params![input.key.namespace,input.key.name,serialized_kind,record.sha256,record.size as i64]
            ).map_err(db_error)?;
        }
        tx.commit().map_err(db_error)?;
        Ok(records)
    }
}


impl AtomicAssetRepository for SqliteAssetStore {
    fn import_if_absent_or_identical(&self, inputs: &[AssetImport]) -> MediaResult<Vec<AssetRecord>> {
        self.import_batch(inputs, None)
    }
}

impl AccessControlledRepository for SqliteAssetStore {
    fn check_access(&self, producer: &str, key: &AssetKey, mode: AssetAccess) -> MediaResult<()> {
        key.validated()?;
        self.authorize(producer, &key.namespace, matches!(mode, AssetAccess::Publish))
    }
}

impl AuthorizedAtomicAssetRepository for SqliteAssetStore {
    fn import_as(&self, producer: &str, inputs: &[AssetImport]) -> MediaResult<Vec<AssetRecord>> {
        self.import_batch(inputs, Some(producer))
    }
}


#[cfg(test)]
mod batch_import_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64,Ordering};
    static COUNTER:AtomicU64=AtomicU64::new(0);
    fn temporary()->PathBuf {
        std::env::temp_dir().join(format!("vsmedia-atomic-batch-{}-{}",
            std::process::id(),COUNTER.fetch_add(1,Ordering::Relaxed)))
    }
    fn asset(name:&str, bytes:&[u8])->AssetImport {
        AssetImport {
            key:AssetKey::new("test",name).unwrap(),kind:AssetKind::Sound,bytes:bytes.to_vec()
        }
    }
    #[test]
    fn batch_is_atomic_on_conflicts_and_identical_imports_are_idempotent() {
        let path=temporary();
        let db=SqliteAssetStore::open(&path).unwrap();
        db.publish(AssetKey::new("test","existing").unwrap(),AssetKind::Sound,b"original").unwrap();
        let denied=db.import_if_absent_or_identical(&[
            asset("new",b"new bytes"),asset("existing",b"replacement")
        ]);
        assert!(denied.is_err());
        assert!(db.metadata(&AssetKey::new("test","new").unwrap()).is_err());
        assert_eq!(db.read(&AssetKey::new("test","existing").unwrap()).unwrap(),b"original");
        let imported=db.import_if_absent_or_identical(&[
            asset("new",b"new bytes"),asset("existing",b"original")
        ]).unwrap();
        assert_eq!(imported.len(),2);
        assert_eq!(imported[0].revision,1);
        assert_eq!(db.import_if_absent_or_identical(&[
            asset("new",b"new bytes"),asset("existing",b"original")
        ]).unwrap()[0].revision,1);
        drop(db);
        let reopened=SqliteAssetStore::open(&path).unwrap();
        assert_eq!(reopened.read(&AssetKey::new("test","new").unwrap()).unwrap(),b"new bytes");
        drop(reopened);
        let _=fs::remove_dir_all(path);
    }
    #[test]
    fn scoped_import_checks_every_namespace_inside_transaction() {
        let path=temporary();
        let db=SqliteAssetStore::open(&path).unwrap();
        db.grant("plugin_a","alpha",true,true).unwrap();
        let entries=[
            AssetImport { key:AssetKey::new("alpha","sound").unwrap(),
                kind:AssetKind::Sound, bytes:b"sound".to_vec() },
            AssetImport { key:AssetKey::new("beta","texture").unwrap(),
                kind:AssetKind::Texture, bytes:b"texture".to_vec() },
        ];
        assert!(db.import_as("plugin_a",&entries).is_err());
        assert!(db.metadata(&entries[0].key).is_err());
        assert!(db.metadata(&entries[1].key).is_err());
        assert!(db.check_access("plugin_a",&entries[0].key,AssetAccess::Read).is_ok());
        assert!(db.check_access("plugin_a",&entries[1].key,AssetAccess::Read).is_err());
        db.grant("plugin_a","beta",true,false).unwrap();
        assert_eq!(db.import_as("plugin_a",&entries).unwrap().len(),2);
        assert_eq!(db.import_as("plugin_a",&entries).unwrap()[0].revision,1);
        drop(db);
        let _=fs::remove_dir_all(path);
    }
    #[test]
    fn duplicate_keys_are_rejected_without_any_writes() {
        let path=temporary();
        let db=SqliteAssetStore::open(&path).unwrap();
        assert!(db.import_if_absent_or_identical(&[
            asset("same",b"a"), asset("same",b"a")
        ]).is_err());
        assert!(db.metadata(&AssetKey::new("test","same").unwrap()).is_err());
        drop(db);
        let _=fs::remove_dir_all(path);
    }
}
