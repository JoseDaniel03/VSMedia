//! Deterministic, non-destructive pack asset merging. ZIP compilation comes later.
pub mod archive;
pub mod delivery;
pub mod structured;
use std::collections::BTreeMap;
use vsmedia_api::{MediaError, MediaResult};

#[derive(Clone, Debug)]
pub struct PackAsset {
    pub source: String,
    pub path: String,
    pub priority: i32,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackConflict {
    pub path: String,
    pub winner: String,
    pub loser: String,
}

#[derive(Debug)]
pub struct MergeOutput {
    pub files: BTreeMap<String, Vec<u8>>,
    pub conflicts: Vec<PackConflict>,
}

pub fn validate_path(path: &str) -> MediaResult<()> {
    if path.is_empty() || path.len() > 512
        || path.starts_with('/') || path.contains('\\')
        || path.split('/').any(|part| part.is_empty() || part == "." || part == "..")
        || !path.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'.'))
        || !(path.starts_with("assets/") || path == "pack.mcmeta" || path == "pack.png")
    {
        return Err(MediaError::InvalidInput(format!("unsafe pack path: {path}")));
    }
    Ok(())
}

/// Higher priority wins. Equal priority with different bytes is a hard conflict.
/// Input ordering never changes the effective outcome.
pub fn merge(inputs: &[PackAsset]) -> MediaResult<MergeOutput> {
    let mut selected: BTreeMap<String, PackAsset> = BTreeMap::new();
    let mut conflicts = Vec::new();
    for input in inputs {
        validate_path(&input.path)?;
        if input.source.is_empty() {
            return Err(MediaError::InvalidInput("pack source required".into()));
        }
        if let Some(existing) = selected.get(&input.path) {
            if existing.data == input.data { continue; }
            if existing.priority == input.priority {
                return Err(MediaError::Conflict(format!(
                    "{} from {} and {} have equal priority",
                    input.path, existing.source, input.source
                )));
            }
            let (winner, loser) = if input.priority > existing.priority {
                (input.source.clone(), existing.source.clone())
            } else {
                (existing.source.clone(), input.source.clone())
            };
            conflicts.push(PackConflict { path: input.path.clone(), winner, loser });
            if input.priority > existing.priority {
                selected.insert(input.path.clone(), input.clone());
            }
        } else {
            selected.insert(input.path.clone(), input.clone());
        }
    }
    Ok(MergeOutput {
        files: selected.into_iter().map(|(key, value)| (key, value.data)).collect(),
        conflicts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn file(source: &str, priority: i32, data: &[u8]) -> PackAsset {
        PackAsset { source: source.into(), path: "assets/demo/sounds/a.ogg".into(),
            priority, data: data.to_vec() }
    }
    #[test]
    fn higher_priority_wins_and_reports_conflict() {
        let result = merge(&[file("plugin-a", 1, b"a"), file("plugin-b", 3, b"b")]).unwrap();
        assert_eq!(result.files["assets/demo/sounds/a.ogg"], b"b");
        assert_eq!(result.conflicts[0].winner, "plugin-b");
    }
    #[test]
    fn equal_priority_is_error() {
        assert!(merge(&[file("a", 1, b"a"), file("b", 1, b"b")]).is_err());
    }
    #[test]
    fn rejects_traversal() {
        let mut f = file("a", 1, b"a");
        f.path = "assets/demo/../escape.txt".into();
        assert!(merge(&[f]).is_err());
    }
}
