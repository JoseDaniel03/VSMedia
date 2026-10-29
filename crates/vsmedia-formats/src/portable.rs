//! Portable offline project snapshots. Source documents stay editable JSON;
//! this deterministic ZIP is a transport envelope, not a runtime resource pack.
//! No filesystem extraction or network access occurs during parsing.
use crate::{decode, AssetRef, DocumentLink, NativeDocument, NativeKind, NativePayload};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Write},
};
use vsmedia_api::{AssetKey, AssetKind, AssetRecord, AssetRepository, MediaError, MediaResult};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

pub const BUNDLE_VERSION: u32 = 1;
pub const MAX_OBJECTS: usize = 512;
pub const MAX_TOTAL_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_BUNDLE_INPUT_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_OBJECT_BYTES: usize = 64 * 1024 * 1024;
const MAX_DEPTH: usize = 16;
const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleEntry {
    pub key: AssetKey,
    pub kind: AssetKind,
    pub sha256: String,
    pub size: u64,
    pub document_kind: Option<NativeKind>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleManifest {
    pub format: String,
    pub version: u32,
    pub root: AssetKey,
    pub entries: Vec<BundleEntry>,
}

#[derive(Debug)]
pub struct PreparedBundle {
    pub manifest: BundleManifest,
    /// Includes every logical asset only once; identical blobs are deduplicated on disk.
    pub assets: Vec<(BundleEntry, Vec<u8>)>,
}

/// Traverse all nested document links, then include every pinned source media asset.
/// Any stale key, mismatched SHA, cross-document cycle or excessive expansion fails.
pub fn collect<R: AssetRepository>(
    repo: &R,
    root: &AssetKey,
) -> MediaResult<BTreeMap<AssetKey, BundleEntry>> {
    let record = repo.metadata(root)?;
    let link = DocumentLink {
        key: root.clone(),
        kind: NativeKind::Project,
        sha256: record.sha256,
    };
    let mut state = Traversal {
        repo, entries: BTreeMap::new(), active: BTreeSet::new(), bytes: 0,
    };
    state.document(&link, 0)?;
    Ok(state.entries)
}

struct Traversal<'a, R: AssetRepository> {
    repo: &'a R,
    entries: BTreeMap<AssetKey, BundleEntry>,
    active: BTreeSet<AssetKey>,
    bytes: usize,
}
impl<R: AssetRepository> Traversal<'_, R> {
    fn add(&mut self, key: &AssetKey, pinned: &str, doc: Option<NativeKind>) -> MediaResult<bool> {
        key.validated()?;
        if pinned.len() != 64 || !pinned.bytes().all(|b|
            b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
        ) {
            return Err(invalid("invalid asset checksum"));
        }
        let record = self.repo.metadata(key)?;
        if record.sha256 != pinned {
            return Err(MediaError::Conflict(format!("stale asset reference: {}", key.qualified())));
        }
        if (doc.is_some() && record.kind != AssetKind::Other) ||
            (doc.is_none() && record.kind == AssetKind::Other)
        {
            return Err(invalid("asset kind does not match document/media reference"));
        }
        if let Some(old) = self.entries.get(key) {
            if old.sha256 != pinned || old.document_kind != doc || old.kind != record.kind {
                return Err(MediaError::Conflict(format!(
                    "conflicting asset references: {}", key.qualified()
                )));
            }
            return Ok(false);
        }
        let size = usize::try_from(record.size)
            .map_err(|_| invalid("asset size exceeds architecture limits"))?;
        if size > MAX_OBJECT_BYTES || self.entries.len() >= MAX_OBJECTS ||
            self.bytes.checked_add(size).is_none_or(|v| v > MAX_TOTAL_BYTES)
        {
            return Err(invalid("portable bundle resource budget exceeded"));
        }
        self.bytes += size;
        self.entries.insert(key.clone(), BundleEntry {
            key: key.clone(), kind: record.kind, sha256: pinned.into(),
            size: record.size, document_kind: doc,
        });
        Ok(true)
    }
    fn media(&mut self, reference: &AssetRef) -> MediaResult<()> {
        self.add(&reference.key, &reference.sha256, None)?;
        Ok(())
    }
    fn document(&mut self, link: &DocumentLink, depth: usize) -> MediaResult<()> {
        if depth >= MAX_DEPTH {
            return Err(invalid("nested project limit exceeded"));
        }
        if self.active.contains(&link.key) {
            return Err(MediaError::Conflict(format!(
                "cycle between project documents: {}", link.key.qualified()
            )));
        }
        if !self.add(&link.key, &link.sha256, Some(link.kind))? {
            return Ok(());
        }
        self.active.insert(link.key.clone());
        let bytes = self.repo.read(&link.key)?;
        check_bytes(&bytes, &link.sha256)?;
        let doc = decode(&bytes, link.kind.extension())?;
        match doc.payload {
            NativePayload::Audio(audio) => {
                for source in &audio.sources { self.media(source)?; }
            }
            NativePayload::Image(image) => {
                for layer in &image.layers { self.media(&layer.source)?; }
            }
            NativePayload::Stream(stream) => {
                for source in &stream.playlist { self.media(source)?; }
            }
            NativePayload::Pack(pack) => {
                for entry in &pack.entries { self.media(&entry.source)?; }
            }
            NativePayload::Project(project) => {
                for node in &project.nodes {
                    if let Some(subdocument) = &node.document {
                        self.document(subdocument, depth + 1)?;
                    }
                }
            }
        }
        self.active.remove(&link.key);
        Ok(())
    }
}

fn invalid(message: &str) -> MediaError {
    MediaError::InvalidInput(message.into())
}
fn io_error<E: std::fmt::Display>(error: E) -> MediaError {
    MediaError::Io(error.to_string())
}
fn check_bytes(data: &[u8], hash: &str) -> MediaResult<()> {
    if format!("{:x}", Sha256::digest(data)) != hash {
        return Err(MediaError::Conflict("asset content SHA-256 mismatch".into()));
    }
    Ok(())
}
fn check_manifest(manifest: &BundleManifest) -> MediaResult<()> {
    if manifest.format != "VSMEDIA_BUNDLE" || manifest.version != BUNDLE_VERSION ||
        manifest.entries.is_empty() || manifest.entries.len() > MAX_OBJECTS
    {
        return Err(invalid("unsupported bundle format/version or number of entries"));
    }
    manifest.root.validated()?;
    let mut unique = BTreeSet::new();
    let mut total = 0usize;
    for entry in &manifest.entries {
        entry.key.validated()?;
        if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|b|
            b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) ||
            !unique.insert(&entry.key) ||
            (entry.document_kind.is_some() != (entry.kind == AssetKind::Other))
        {
            return Err(invalid("invalid or duplicate bundled asset"));
        }
        let size = usize::try_from(entry.size).map_err(|_| invalid("invalid asset size"))?;
        total = total.checked_add(size).ok_or_else(|| invalid("bundle size overflow"))?;
        if size > MAX_OBJECT_BYTES || total > MAX_TOTAL_BYTES {
            return Err(invalid("bundle resource size limit exceeded"));
        }
    }
    let root = manifest.entries.iter().find(|entry| entry.key == manifest.root)
        .ok_or_else(|| invalid("bundle missing root project"))?;
    if root.document_kind != Some(NativeKind::Project) {
        return Err(invalid("bundle root must be a native project"));
    }
    Ok(())
}

/// Build a deterministic, size-bounded transport archive from the current
/// immutable project dependency closure. No output is written to the filesystem.
pub fn export<R: AssetRepository>(repo: &R, root: &AssetKey) -> MediaResult<Vec<u8>> {
    let entries: Vec<_> = collect(repo, root)?.into_values().collect();
    let manifest = BundleManifest {
        format: "VSMEDIA_BUNDLE".into(), version: BUNDLE_VERSION,
        root: root.clone(), entries,
    };
    check_manifest(&manifest)?;
    let manifest_bytes = serde_json::to_vec(&manifest).map_err(io_error)?;
    if manifest_bytes.len() > MAX_MANIFEST_BYTES {
        return Err(invalid("bundle manifest too large"));
    }

    let mut unique_objects = BTreeMap::<String, Vec<u8>>::new();
    for entry in &manifest.entries {
        let content = repo.read(&entry.key)?;
        if content.len() as u64 != entry.size {
            return Err(MediaError::Conflict("asset size changed during export".into()));
        }
        check_bytes(&content, &entry.sha256)?;
        unique_objects.entry(entry.sha256.clone()).or_insert(content);
    }
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default());
    zip.start_file("bundle.json", options).map_err(io_error)?;
    zip.write_all(&manifest_bytes).map_err(io_error)?;
    for (hash, bytes) in unique_objects {
        zip.start_file(format!("objects/{hash}"), options).map_err(io_error)?;
        zip.write_all(&bytes).map_err(io_error)?;
    }
    let result = zip.finish().map_err(io_error)?.into_inner();
    if result.len() > MAX_BUNDLE_INPUT_BYTES {
        return Err(invalid("portable archive exceeds size limit"));
    }
    Ok(result)
}

/// Validate all archive entries, checksums, nested project links and completeness.
/// Does not mutate the asset repository. No extraction to arbitrary filesystem paths.
pub fn inspect(bytes: &[u8]) -> MediaResult<PreparedBundle> {
    if bytes.len() > MAX_BUNDLE_INPUT_BYTES || bytes.len() < 22 {
        return Err(invalid("invalid archive size"));
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(io_error)?;
    if archive.len() < 2 || archive.len() > MAX_OBJECTS + 1 {
        return Err(invalid("invalid number of archive entries"));
    }
    let manifest_bytes = {
        let mut file = archive.by_name("bundle.json").map_err(io_error)?;
        if file.size() as usize > MAX_MANIFEST_BYTES { return Err(invalid("manifest too large")); }
        let mut data = Vec::new();
        file.by_ref().take((MAX_MANIFEST_BYTES + 1) as u64)
            .read_to_end(&mut data).map_err(io_error)?;
        if data.len() > MAX_MANIFEST_BYTES { return Err(invalid("manifest too large")); }
        data
    };
    let manifest: BundleManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| invalid(&format!("invalid bundle manifest: {e}")))?;
    check_manifest(&manifest)?;
    let by_hash: BTreeMap<_, _> = manifest.entries.iter()
        .map(|entry| (entry.sha256.as_str(), entry.size)).collect();
    let mut objects = BTreeMap::<String, Vec<u8>>::new();
    let mut seen = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(io_error)?;
        let path = entry.name().to_owned();
        if entry.is_dir() || !seen.insert(path.clone()) ||
            entry.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(invalid("invalid, duplicate or symlink archive entry"));
        }
        if path == "bundle.json" { continue; }
        let hash = path.strip_prefix("objects/")
            .ok_or_else(|| invalid("unexpected archive entry path"))?;
        let size = *by_hash.get(hash).ok_or_else(|| invalid("unreferenced object in archive"))?;
        if entry.size() != size {
            return Err(invalid("stored object length mismatch"));
        }
        let mut raw = Vec::new();
        entry.by_ref().take(size + 1).read_to_end(&mut raw).map_err(io_error)?;
        if raw.len() as u64 != size {
            return Err(invalid("corrupt or truncated bundled object"));
        }
        check_bytes(&raw, hash)?;
        objects.insert(hash.into(), raw);
    }
    if objects.len() != by_hash.len() {
        return Err(invalid("bundle is missing referenced objects"));
    }
    let assets: Vec<_> = manifest.entries.iter().map(|entry| {
        let bytes = objects.get(&entry.sha256).ok_or_else(|| invalid("missing object"))?.clone();
        Ok((entry.clone(), bytes))
    }).collect::<MediaResult<_>>()?;
    let view = ArchiveView { assets: &assets };
    let resolved = collect(&view, &manifest.root)?;
    let expected: BTreeMap<_, _> = manifest.entries.iter()
        .map(|entry| (entry.key.clone(), entry.clone())).collect();
    if resolved != expected {
        return Err(invalid("bundle is incomplete or contains unrelated assets"));
    }
    Ok(PreparedBundle { manifest, assets })
}

struct ArchiveView<'a> {
    assets: &'a [(BundleEntry, Vec<u8>)],
}
impl AssetRepository for ArchiveView<'_> {
    fn metadata(&self, key: &AssetKey) -> MediaResult<AssetRecord> {
        let item = self.assets.iter().find(|(entry, _)| &entry.key == key)
            .ok_or_else(|| MediaError::NotFound(key.qualified()))?;
        Ok(AssetRecord {
            key: key.clone(), kind: item.0.kind, sha256: item.0.sha256.clone(),
            size: item.0.size, revision: 1,
        })
    }
    fn read(&self, key: &AssetKey) -> MediaResult<Vec<u8>> {
        self.assets.iter().find(|(entry, _)| &entry.key == key)
            .map(|(_, bytes)| bytes.clone())
            .ok_or_else(|| MediaError::NotFound(key.qualified()))
    }
    fn publish(&self, _: AssetKey, _: AssetKind, _: &[u8]) -> MediaResult<AssetRecord> {
        Err(invalid("archive view is read-only"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{publish, NativeDocument, NativePayload, VSProject, ProjectNode, VSAudio, AssetRef, MediaCodec};
    use std::{fs, sync::atomic::{AtomicU64, Ordering}};
    use vsmedia_audio::{AudioProject, AudioTrack, AudioClip};
    use vsmedia_core::sqlite::SqliteAssetStore;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn store() -> (std::path::PathBuf, SqliteAssetStore) {
        let path = std::env::temp_dir().join(format!(
            "vsmedia-portable-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let db = SqliteAssetStore::open(&path).unwrap();
        (path, db)
    }
    fn project_with_shared_sources(db: &SqliteAssetStore) -> AssetKey {
        let sample_key = AssetKey::new("demo", "sounds/intro.wav").unwrap();
        let sound = db.publish(sample_key.clone(), AssetKind::Sound, b"audio bytes").unwrap();
        let audio_key = AssetKey::new("demo", "studio/dialogue").unwrap();
        let audio = NativeDocument::new(NativePayload::Audio(VSAudio {
            timeline: AudioProject { name: "Intro".into(), tracks: vec![
                AudioTrack { name: "NPC".into(), muted: false, clips: vec![
                    AudioClip { asset: sample_key.clone(), start_ms: 0, duration_ms: 1000, gain_db: 0.0 }
                ]}
            ]},
            sources: vec![AssetRef { key: sample_key, sha256: sound.sha256, codec: MediaCodec::Wav }],
        }));
        let saved_audio = publish(db, audio_key.clone(), &audio).unwrap();
        let project_key = AssetKey::new("demo", "projects/intro").unwrap();
        let project = NativeDocument::new(NativePayload::Project(VSProject {
            name: "Intro scene".into(),
            nodes: vec![
                ProjectNode { id: "dialogue".into(), depends_on: vec![], document: Some(DocumentLink {
                    key: audio_key.clone(), kind: NativeKind::Audio, sha256: saved_audio.sha256.clone()
                }) },
                ProjectNode { id: "voice".into(), depends_on: vec!["dialogue".into()],
                    document: Some(DocumentLink {
                        key: audio_key, kind: NativeKind::Audio, sha256: saved_audio.sha256
                    })
                },
            ], entry_points: vec!["voice".into()],
        }));
        publish(db, project_key.clone(), &project).unwrap();
        project_key
    }
    #[test]
    fn portable_round_trip_is_reproducible_and_deduplicated() {
        let (path, db) = store();
        let key = project_with_shared_sources(&db);
        let first = export(&db, &key).unwrap();
        let again = export(&db, &key).unwrap();
        assert_eq!(first, again);
        let prepared = inspect(&first).unwrap();
        assert_eq!(prepared.manifest.root, key);
        assert_eq!(prepared.assets.len(), 3); // project + audio + unique sound
        let _ = fs::remove_dir_all(path);
    }
    #[test]
    fn stale_pinned_blob_rejects_export() {
        let (path, db) = store();
        let key = project_with_shared_sources(&db);
        db.publish(AssetKey::new("demo", "sounds/intro.wav").unwrap(), AssetKind::Sound, b"changed").unwrap();
        assert!(export(&db, &key).is_err());
        let _ = fs::remove_dir_all(path);
    }
    #[test]
    fn malformed_archive_is_rejected() {
        assert!(inspect(b"not an archive").is_err());
    }
}
