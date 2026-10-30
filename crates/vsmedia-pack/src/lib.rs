//! Deterministic, non-destructive pack asset merging. ZIP compilation comes later.
pub mod archive;
pub mod delivery;
pub mod layers;
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
    let mut grouped:BTreeMap<&str,Vec<&PackAsset>>=BTreeMap::new();
    for input in inputs {
        validate_path(&input.path)?;
        if input.source.is_empty() {
            return Err(MediaError::InvalidInput("pack source required".into()));
        }
        grouped.entry(&input.path).or_default().push(input);
    }
    let mut files=BTreeMap::new();
    let mut conflicts=Vec::new();
    for (path, mut group) in grouped {
        // Resolve from the highest priority. Shadowed, lower-priority packs
        // cannot make a valid higher-priority choice fail due to their ties.
        // The source ID is a stable tie-break for IDENTICAL bytes only.
        group.sort_by(|a,b|b.priority.cmp(&a.priority)
            .then_with(||a.source.cmp(&b.source)));
        let winner=group[0];
        for candidate in group.iter().skip(1) {
            if candidate.data==winner.data {continue;}
            if candidate.priority==winner.priority {
                return Err(MediaError::Conflict(format!(
                    "{path} from {} and {} have equal priority",
                    winner.source,candidate.source
                )));
            }
            conflicts.push(PackConflict {
                path:path.into(),winner:winner.source.clone(),
                loser:candidate.source.clone(),
            });
        }
        files.insert(path.to_owned(),winner.data.clone());
    }
    Ok(MergeOutput {files,conflicts})
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
    fn lower_priority_ties_do_not_block_highest_priority_texture(){
        let mut a=file("low_one",1,b"one");
        a.path="assets/virgoland/textures/gui/hud.png".into();
        let mut b=file("low_two",1,b"two");
        b.path=a.path.clone();
        let mut top=file("override",10,b"top");
        top.path=a.path.clone();
        let one=merge(&[a.clone(),b.clone(),top.clone()]).unwrap();
        let two=merge(&[top,b,a]).unwrap();
        assert_eq!(one.files,two.files);
        assert_eq!(one.conflicts,two.conflicts);
        assert_eq!(one.files["assets/virgoland/textures/gui/hud.png"],b"top");
        assert_eq!(one.conflicts.len(),2);
        assert!(one.conflicts.iter().all(|c|c.winner=="override"));
    }
    #[test]
    fn three_layers_report_effective_winner_not_intermediate_owner(){
        let source=vec![
            file("base",0,b"base"),
            file("middle",10,b"middle"),
            file("top",20,b"top"),
        ];
        let result=merge(&source).unwrap();
        assert_eq!(result.files["assets/demo/sounds/a.ogg"],b"top");
        assert_eq!(result.conflicts.len(),2);
        assert!(result.conflicts.iter().all(|c|c.winner=="top"));
    }
    #[test]
    fn rejects_traversal() {
        let mut f = file("a", 1, b"a");
        f.path = "assets/demo/../escape.txt".into();
        assert!(merge(&[f]).is_err());
    }
}
