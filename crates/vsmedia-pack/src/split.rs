//! Deterministic categorization and splitting of an already merged Minecraft
//! Resource Pack. A category ZIP always has its own pack.mcmeta, and every
//! non-manifest resource occurs in exactly one output. Source ZIPs are untouched.
use crate::{validate_path, MergeOutput};
use std::collections::BTreeMap;
use vsmedia_api::{MediaError, MediaResult};

pub const GROUPS:&[&str]=&[
    "interface","items","blocks","audio","particles","entities","languages","other"
];

/// Categories are based on path semantics, not guesses about pixel content.
/// They also keep .png.mcmeta companions with their texture by matching prefixes.
pub fn group_for(path:&str)->&'static str {
    if path=="pack.mcmeta"||path=="pack.png" {return "other";}
    let Some(tail)=path.strip_prefix("assets/").and_then(|s|s.split_once('/').map(|(_,rest)|rest))
        else{return "other";};
    if tail.starts_with("textures/gui/")||tail.starts_with("textures/ui/")
        ||tail.starts_with("textures/hud/")||tail.starts_with("textures/font/")
        ||tail.starts_with("font/")||tail.starts_with("atlases/")
        ||tail.starts_with("textures/mob_effect/") { "interface" }
    else if tail.starts_with("textures/item/")||tail.starts_with("models/item/")
        ||tail.starts_with("items/")||tail.starts_with("equipment/") { "items" }
    else if tail.starts_with("textures/block/")||tail.starts_with("models/block/")
        ||tail.starts_with("blockstates/") { "blocks" }
    else if tail.starts_with("sounds/")||tail=="sounds.json" { "audio" }
    else if tail.starts_with("particles/")||tail.starts_with("textures/particle/") { "particles" }
    else if tail.starts_with("textures/entity/")||tail.starts_with("models/entity/")
        ||tail.starts_with("entity/") { "entities" }
    else if tail.starts_with("lang/") { "languages" }
    else { "other" }
}

/// Split into category-specific installable packs. Rules do not rewrite
/// cross-file JSON references; the returned reports should warn that categories
/// may need to be enabled as a stack to preserve their dependencies.
pub fn split_categories(output:&MergeOutput)->MediaResult<BTreeMap<String,MergeOutput>> {
    let manifest=output.files.get("pack.mcmeta")
        .ok_or_else(||MediaError::InvalidInput("pack.mcmeta required".into()))?;
    let root:serde_json::Value=serde_json::from_slice(manifest)
        .map_err(|e|MediaError::InvalidInput(format!("invalid pack.mcmeta: {e}")))?;
    if !root.get("pack").is_some_and(serde_json::Value::is_object) {
        return Err(MediaError::InvalidInput("pack.mcmeta missing pack object".into()));
    }
    let mut groups:BTreeMap<String,BTreeMap<String,Vec<u8>>>=BTreeMap::new();
    for (path,data) in &output.files {
        validate_path(path)?;
        if path=="pack.mcmeta" {continue;}
        groups.entry(group_for(path).into()).or_default()
            .insert(path.clone(),data.clone());
    }
    if groups.is_empty(){groups.insert("other".into(),BTreeMap::new());}
    Ok(groups.into_iter().map(|(name,mut files)|{
        files.insert("pack.mcmeta".into(),manifest.clone());
        (name,MergeOutput{files,conflicts:Vec::new()})
    }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn divides_categories_and_keeps_animation_sidecars_together(){
        let files=BTreeMap::from([
            ("pack.mcmeta".into(),br#"{"pack":{"description":"Virgoland"}}"#.to_vec()),
            ("assets/demo/textures/gui/tooltip.png".into(),b"png".to_vec()),
            ("assets/demo/textures/gui/tooltip.png.mcmeta".into(),b"animation".to_vec()),
            ("assets/demo/font/default.json".into(),b"fonts".to_vec()),
            ("assets/demo/textures/item/sword.png".into(),b"item".to_vec()),
            ("assets/demo/sounds/click.ogg".into(),b"audio".to_vec()),
            ("assets/demo/particles/smoke.json".into(),b"particle".to_vec()),
        ]);
        let split=split_categories(&MergeOutput{files:files.clone(),conflicts:vec![]}).unwrap();
        assert_eq!(split.len(),4);
        let ui=&split["interface"].files;
        assert!(ui.contains_key("assets/demo/textures/gui/tooltip.png"));
        assert!(ui.contains_key("assets/demo/textures/gui/tooltip.png.mcmeta"));
        assert!(ui.contains_key("assets/demo/font/default.json"));
        for (group,pack) in &split {
            assert!(pack.files.contains_key("pack.mcmeta"),"{group}");
        }
        let total=split.values().map(|pack|pack.files.len()-1).sum::<usize>();
        assert_eq!(total,files.len()-1);
        assert_eq!(group_for("assets/demo/textures/item/diamond.png.mcmeta"),"items");
        assert_eq!(group_for("assets/demo/lang/es_ar.json"),"languages");
    }
    #[test]
    fn rejects_invalid_manifest_and_missing_manifest(){
        assert!(split_categories(&MergeOutput{
            files:BTreeMap::new(),conflicts:vec![]
        }).is_err());
        assert!(split_categories(&MergeOutput{
            files:BTreeMap::from([("pack.mcmeta".into(),b"{}".to_vec())]),
            conflicts:vec![]
        }).is_err());
    }
}
