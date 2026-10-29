//! Import external resource-pack ZIPs safely and create reproducible merged ZIPs.
use super::{validate_path, MergeOutput, PackAsset};
use std::{
    collections::{BTreeMap, HashSet},
    io::{Cursor, Read, Write},
};
use vsmedia_api::{MediaError, MediaResult};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

#[derive(Clone, Copy, Debug)]
pub struct ArchiveLimits {
    pub max_files: usize,
    pub max_total_bytes: usize,
    pub max_single_file_bytes: usize,
}
impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_files: 4096,
            max_total_bytes: 64 * 1024 * 1024,
            max_single_file_bytes: 16 * 1024 * 1024,
        }
    }
}
impl ArchiveLimits {
    fn check(self) -> MediaResult<()> {
        if self.max_files == 0 || self.max_files > 65536
            || self.max_total_bytes == 0 || self.max_total_bytes > 1024 * 1024 * 1024
            || self.max_single_file_bytes == 0 || self.max_single_file_bytes > self.max_total_bytes
        {
            return Err(MediaError::InvalidInput("invalid ZIP limits".into()));
        }
        Ok(())
    }
}

pub fn import_zip(bytes: &[u8], source: &str, priority: i32, limits: ArchiveLimits) -> MediaResult<Vec<PackAsset>> {
    limits.check()?;
    if source.trim().is_empty() || source.len() > 128 {
        return Err(MediaError::InvalidInput("invalid pack source".into()));
    }
    if bytes.len() > limits.max_total_bytes {
        return Err(MediaError::InvalidInput("ZIP input size too large".into()));
    }
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(zip_error)?;
    if zip.len() > limits.max_files {
        return Err(MediaError::InvalidInput("too many ZIP entries".into()));
    }
    let mut paths = HashSet::new();
    let mut used = 0usize;
    let mut assets = Vec::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(zip_error)?;
        if entry.is_dir() { continue; }
        let path = entry.name().to_string();
        validate_path(&path)?;
        // ZIP files must not contain symlinks or other special file types.
        if entry.unix_mode().is_some_and(|mode| (mode & 0o170000) == 0o120000) {
            return Err(MediaError::InvalidInput("symlinks forbidden in resource packs".into()));
        }
        if !paths.insert(path.clone()) {
            return Err(MediaError::Conflict(format!("duplicate ZIP entry: {path}")));
        }
        let size = usize::try_from(entry.size())
            .map_err(|_| MediaError::InvalidInput("ZIP file too large".into()))?;
        used = used.checked_add(size)
            .ok_or_else(|| MediaError::InvalidInput("ZIP uncompressed size overflow".into()))?;
        if size > limits.max_single_file_bytes || used > limits.max_total_bytes {
            return Err(MediaError::InvalidInput("ZIP uncompressed size quota exceeded".into()));
        }
        let mut data = Vec::with_capacity(size);
        // Bound decompression even when ZIP headers claim an incorrect size.
        entry.by_ref().take((size as u64) + 1)
            .read_to_end(&mut data).map_err(io_error)?;
        if data.len() != size {
            return Err(MediaError::InvalidInput("ZIP entry size mismatch".into()));
        }
        assets.push(PackAsset { source: source.into(), path, priority, data });
    }
    Ok(assets)
}

pub fn export_zip(output: &MergeOutput, limits: ArchiveLimits) -> MediaResult<Vec<u8>> {
    limits.check()?;
    if output.files.len() > limits.max_files {
        return Err(MediaError::InvalidInput("too many resource pack files".into()));
    }
    let mut used = 0usize;
    for (path, bytes) in &output.files {
        validate_path(path)?;
        used = used.checked_add(bytes.len())
            .ok_or_else(|| MediaError::InvalidInput("pack total size overflow".into()))?;
        if bytes.len() > limits.max_single_file_bytes || used > limits.max_total_bytes {
            return Err(MediaError::InvalidInput("pack size quota exceeded".into()));
        }
    }
    if let Some(manifest) = output.files.get("pack.mcmeta") {
        let doc: serde_json::Value = serde_json::from_slice(manifest)
            .map_err(|_| MediaError::InvalidInput("pack.mcmeta is not valid JSON".into()))?;
        if !doc.is_object() || !doc.get("pack").is_some_and(serde_json::Value::is_object) {
            return Err(MediaError::InvalidInput("pack.mcmeta missing pack object".into()));
        }
    } else {
        return Err(MediaError::InvalidInput("pack.mcmeta is required".into()));
    }
    let writer = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(writer);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    // BTreeMap ensures deterministic ordering and stable ZIP output.
    for (path, data) in &output.files {
        zip.start_file(path, options).map_err(zip_error)?;
        zip.write_all(data).map_err(io_error)?;
    }
    let archive = zip.finish().map_err(zip_error)?.into_inner();
    if archive.len() > limits.max_total_bytes {
        return Err(MediaError::InvalidInput("compressed pack exceeds quota".into()));
    }
    Ok(archive)
}
fn io_error(e: std::io::Error) -> MediaError { MediaError::Io(e.to_string()) }
fn zip_error(e: zip::result::ZipError) -> MediaError { MediaError::InvalidInput(e.to_string()) }

/// Convenient import for multiple archives. Results are still subject to merge priorities.
pub fn merge_archives(
    archives: &[(String, i32, Vec<u8>)],
    limits: ArchiveLimits,
) -> MediaResult<MergeOutput> {
    let mut files = Vec::new();
    for (source, priority, data) in archives {
        files.extend(import_zip(data, source, *priority, limits)?);
    }
    super::structured::merge_structured(&files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge;
    #[test]
    fn zip_round_trip_preserves_files_and_is_deterministic() {
        let mut map = BTreeMap::new();
        map.insert("pack.mcmeta".into(), br#"{"pack":{"pack_format":1,"description":"Test"}}"#.to_vec());
        map.insert("assets/demo/sounds/test.ogg".into(), b"audio bytes".to_vec());
        let output = MergeOutput { files: map, conflicts: Vec::new() };
        let one = export_zip(&output, ArchiveLimits::default()).unwrap();
        let two = export_zip(&output, ArchiveLimits::default()).unwrap();
        assert_eq!(one, two);
        let imported = import_zip(&one, "demo", 1, ArchiveLimits::default()).unwrap();
        assert_eq!(merge(&imported).unwrap().files, output.files);
    }
    #[test]
    fn rejects_unsafe_pack_paths() {
        let mut map = BTreeMap::new();
        map.insert("../escape".into(), b"no".to_vec());
        assert!(export_zip(&MergeOutput { files: map, conflicts: Vec::new() }, ArchiveLimits::default()).is_err());
    }
}
