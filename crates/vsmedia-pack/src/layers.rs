//! Split an already merged Resource Pack into a stable base and configurable
//! small overlays. Paper 26.3 can stack these with addResourcePack. A changed
//! overlay should not alter the base archive/hash or force its re-download.
use crate::{validate_path, MergeOutput};
use std::collections::{BTreeMap, BTreeSet};
use vsmedia_api::{MediaError, MediaResult};

pub const MAX_PACK_LAYERS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayRule {
    /// Stable pack identifier (used for the Minecraft pack UUID).
    pub logical_id: String,
    /// Asset path prefixes, e.g. "assets/demo/textures/ui/".
    pub prefixes: Vec<String>,
}

#[derive(Debug)]
pub struct NamedLayer {
    pub logical_id: String,
    pub pack: MergeOutput,
}

pub fn split_into_layers(
    merged: &MergeOutput,
    base_id: &str,
    overlays: &[OverlayRule],
) -> MediaResult<Vec<NamedLayer>> {
    if overlays.len() + 1 > MAX_PACK_LAYERS {
        return Err(MediaError::InvalidInput("too many resource pack layers".into()));
    }
    let mut names = BTreeSet::new();
    for id in std::iter::once(base_id).chain(overlays.iter().map(|r|r.logical_id.as_str())) {
        if id.is_empty() || id.len()>64 || !id.bytes().all(|b|
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b,b'_' | b'-')
        ) || !names.insert(id) {
            return Err(MediaError::InvalidInput("invalid or duplicate layer ID".into()));
        }
    }
    let meta=merged.files.get("pack.mcmeta")
        .ok_or_else(||MediaError::InvalidInput("pack.mcmeta is required".into()))?;
    let mut all_prefixes = BTreeSet::new();
    for rule in overlays {
        if rule.prefixes.is_empty() {
            return Err(MediaError::InvalidInput("overlay must select asset paths".into()));
        }
        for prefix in &rule.prefixes {
            if !prefix.starts_with("assets/") || !prefix.ends_with('/') ||
                prefix.len()>256 || !all_prefixes.insert(prefix) ||
                validate_path(&format!("{prefix}example")).is_err() {
                return Err(MediaError::InvalidInput("invalid or duplicate overlay path prefix".into()));
            }
        }
    }

    let mut results=Vec::with_capacity(overlays.len()+1);
    let mut base=BTreeMap::new();
    base.insert("pack.mcmeta".into(),meta.clone());
    results.push(NamedLayer {
        logical_id:base_id.into(),
        pack:MergeOutput {files:base, conflicts:Vec::new()},
    });
    for rule in overlays {
        let mut files=BTreeMap::new();
        files.insert("pack.mcmeta".into(),meta.clone());
        results.push(NamedLayer {
            logical_id:rule.logical_id.clone(),
            pack:MergeOutput {files, conflicts:Vec::new()},
        });
    }
    let mut active=vec![false;results.len()];
    for (path, data) in &merged.files {
        validate_path(path)?;
        if path=="pack.mcmeta" { continue; }
        // Most-specific path rule wins. Equal-length ties are impossible after
        // duplicate-prefix rejection. Other files always remain in the base.
        let mut index=0;
        let mut best=0;
        if path.starts_with("assets/") {
            for (i,rule) in overlays.iter().enumerate() {
                for prefix in &rule.prefixes {
                    if path.starts_with(prefix) && prefix.len()>best {
                        best=prefix.len();
                        index=i+1;
                    }
                }
            }
        }
        results[index].pack.files.insert(path.clone(),data.clone());
        active[index]=true;
    }
    // Avoid pushing metadata-only overlays with no assets.
    let mut selected=Vec::with_capacity(results.len());
    for (idx,layer) in results.into_iter().enumerate() {
        if idx==0 || active[idx] { selected.push(layer); }
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{export_zip,ArchiveLimits};
    use sha1::{Digest, Sha1};

    fn pack(ui:&[u8])->MergeOutput {
        MergeOutput {
            files:BTreeMap::from([
                ("pack.mcmeta".into(),br#"{"pack":{"description":"Server"}}"#.to_vec()),
                ("assets/demo/textures/base.png".into(),b"base texture".to_vec()),
                ("assets/demo/sounds/voice.ogg".into(),b"voice".to_vec()),
                ("assets/demo/textures/ui/button.png".into(),ui.to_vec()),
            ]),
            conflicts:Vec::new(),
        }
    }
    fn rules()->Vec<OverlayRule> {
        vec![OverlayRule {
            logical_id:"ui".into(),
            prefixes:vec!["assets/demo/textures/ui/".into()],
        }]
    }
    #[test]
    fn only_changed_overlay_changes_its_zip_sha() {
        let one=split_into_layers(&pack(b"old"),"base",&rules()).unwrap();
        let two=split_into_layers(&pack(b"new"),"base",&rules()).unwrap();
        assert_eq!(one.len(),2);
        assert_eq!(two.len(),2);
        let zipped=|layers:&Vec<NamedLayer>, idx:usize| {
            export_zip(&layers[idx].pack,ArchiveLimits::default()).unwrap()
        };
        let base_1=zipped(&one,0);
        let base_2=zipped(&two,0);
        assert_eq!(base_1,base_2);
        assert_eq!(format!("{:x}",Sha1::digest(&base_1)),
            format!("{:x}",Sha1::digest(&base_2)));
        let overlay_1=zipped(&one,1);
        let overlay_2=zipped(&two,1);
        assert_ne!(overlay_1,overlay_2);
        assert!(one[0].pack.files.contains_key("assets/demo/textures/base.png"));
        assert!(!one[0].pack.files.contains_key("assets/demo/textures/ui/button.png"));
        assert!(one[1].pack.files.contains_key("assets/demo/textures/ui/button.png"));
    }
    #[test]
    fn skips_empty_overlays_and_rejects_invalid_rule_configuration() {
        let empty=vec![OverlayRule {
            logical_id:"unused".into(),prefixes:vec!["assets/demo/not_present/".into()]
        }];
        let result=split_into_layers(&pack(b"ui"),"base",&empty).unwrap();
        assert_eq!(result.len(),1);
        assert!(split_into_layers(&pack(b"ui"),"base",&[
            OverlayRule {logical_id:"ui".into(),prefixes:vec!["../danger/".into()]}
        ]).is_err());
        assert!(split_into_layers(&pack(b"ui"),"base",&[
            OverlayRule {logical_id:"base".into(),
                prefixes:vec!["assets/demo/textures/ui/".into()]}
        ]).is_err());
    }
    #[test]
    fn longest_asset_path_prefix_is_deterministic() {
        let rules=vec![
            OverlayRule {
                logical_id:"textures".into(),prefixes:vec!["assets/demo/textures/".into()]
            },
            OverlayRule {
                logical_id:"buttons".into(),prefixes:vec!["assets/demo/textures/ui/".into()]
            },
        ];
        let layers=split_into_layers(&pack(b"new"),"base",&rules).unwrap();
        assert_eq!(layers.len(),3);
        assert!(layers[1].pack.files.contains_key("assets/demo/textures/base.png"));
        assert!(layers[2].pack.files.contains_key("assets/demo/textures/ui/button.png"));
    }
}
