//! Schema-aware resource-pack merging. Only known JSON document shapes are combined.
//! Unknown assets, textures, arbitrary JSON and pack.mcmeta use explicit file priorities.
use std::collections::BTreeMap;
use serde_json::{Map, Value};
use vsmedia_api::{MediaError, MediaResult};
use super::{merge, validate_path, MergeOutput, PackAsset, PackConflict};

const MAX_JSON_BYTES:usize=2*1024*1024;
const MAX_SOUND_ENTRIES:usize=16_384;
const MAX_INPUTS:usize=16_384;

/// Merge all producer contributions deterministically.
/// sounds.json is merged by individual sound event name; binary files follow
/// the default file priority policy. Unknown JSON is never deeply merged.
pub fn merge_structured(inputs:&[PackAsset]) -> MediaResult<MergeOutput> {
    if inputs.len()>MAX_INPUTS {
        return Err(MediaError::InvalidInput("too many pack contributions".into()));
    }
    let mut groups:BTreeMap<&str,Vec<PackAsset>>=BTreeMap::new();
    for input in inputs {
        validate_path(&input.path)?;
        groups.entry(&input.path).or_default().push(input.clone());
    }
    let mut result=MergeOutput {files:BTreeMap::new(),conflicts:Vec::new()};
    for (path, group) in groups {
        let output=if is_sounds(path) && group.len()>1 {
            merge_sounds(path,&group)?
        } else {
            merge(&group)?
        };
        result.files.extend(output.files);
        result.conflicts.extend(output.conflicts);
    }
    // Predictable diagnostics independent of producer registration order.
    result.conflicts.sort_by(|a,b| {
        (&a.path,&a.winner,&a.loser).cmp(&(&b.path,&b.winner,&b.loser))
    });
    Ok(result)
}
fn is_sounds(path:&str)->bool {
    let mut components=path.split('/');
    components.next()==Some("assets")
        && components.next().is_some_and(|s| !s.is_empty())
        && components.next()==Some("sounds.json")
        && components.next().is_none()
}
fn merge_sounds(path:&str,group:&[PackAsset])->MediaResult<MergeOutput> {
    let mut sorted=group.to_vec();
    // Low priority first; do not depend on plugin startup order.
    sorted.sort_by(|a,b| (a.priority,&a.source).cmp(&(b.priority,&b.source)));
    let mut entries:BTreeMap<String,(i32,String,Value)>=BTreeMap::new();
    let mut conflicts=Vec::new();
    for asset in sorted {
        if asset.data.len()>MAX_JSON_BYTES {
            return Err(MediaError::InvalidInput(format!("sounds.json exceeds 2 MiB: {path}")));
        }
        let root:Value=serde_json::from_slice(&asset.data)
            .map_err(|e|MediaError::InvalidInput(format!("invalid sounds.json at {path}: {e}")))?;
        let map=root.as_object().ok_or_else(||
            MediaError::InvalidInput(format!("sounds.json must be an object: {path}")))?;
        if map.len()>MAX_SOUND_ENTRIES {
            return Err(MediaError::InvalidInput("too many sound events in JSON".into()));
        }
        for (event,value) in map {
            if event.is_empty() || event.len()>256 {
                return Err(MediaError::InvalidInput("invalid sound event key".into()));
            }
            if let Some((priority,owner,old_value))=entries.get(event) {
                if old_value!=value {
                    if *priority==asset.priority {
                        return Err(MediaError::Conflict(format!(
                            "equal-priority sound event collision in {path}: {event} ({owner} vs {})",
                            asset.source
                        )));
                    }
                    conflicts.push(PackConflict {
                        path:format!("{path}#{event}"),
                        winner:asset.source.clone(),
                        loser:owner.clone(),
                    });
                }
            }
            entries.insert(event.clone(),(asset.priority,asset.source.clone(),value.clone()));
        }
        if entries.len()>MAX_SOUND_ENTRIES {
            return Err(MediaError::InvalidInput("merged sounds.json exceeds event quota".into()));
        }
    }
    let document:Map<String,Value>=entries.into_iter()
        .map(|(key,(_,_,value))|(key,value)).collect();
    let bytes=serde_json::to_vec(&Value::Object(document))
        .map_err(|e|MediaError::Io(e.to_string()))?;
    if bytes.len()>MAX_JSON_BYTES {
        return Err(MediaError::InvalidInput("merged sounds.json exceeds size quota".into()));
    }
    Ok(MergeOutput {
        files:BTreeMap::from([(path.into(),bytes)]),
        conflicts,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sound(source:&str,priority:i32,content:&str)->PackAsset {
        PackAsset{source:source.into(),path:"assets/demo/sounds.json".into(),
            priority,data:content.as_bytes().to_vec()}
    }
    #[test]
    fn merges_distinct_sound_events_instead_of_overwriting_file() {
        let a=sound("typewriter",1,r#"{"intro":{"sounds":["intro"]}}"#);
        let b=sound("vsrender",1,r#"{"click":{"sounds":["click"]}}"#);
        let result=merge_structured(&[a,b]).unwrap();
        let json:Value=serde_json::from_slice(&result.files["assets/demo/sounds.json"]).unwrap();
        assert!(json.get("intro").is_some());
        assert!(json.get("click").is_some());
        assert!(result.conflicts.is_empty());
    }
    #[test]
    fn colliding_event_is_resolved_by_priority() {
        let a=sound("a",1,r#"{"intro":{"sounds":["old"]}}"#);
        let b=sound("b",5,r#"{"intro":{"sounds":["new"]}}"#);
        let result=merge_structured(&[b.clone(),a.clone()]).unwrap();
        let reversed=merge_structured(&[a,b]).unwrap();
        assert_eq!(result.files,reversed.files);
        assert_eq!(result.conflicts.len(),1);
        assert_eq!(result.conflicts[0].path,"assets/demo/sounds.json#intro");
        let json:Value=serde_json::from_slice(&result.files["assets/demo/sounds.json"]).unwrap();
        assert_eq!(json["intro"]["sounds"][0],"new");
    }
    #[test]
    fn rejects_equal_priority_and_broken_json() {
        assert!(merge_structured(&[
            sound("a",1,r#"{"intro":{"sounds":["a"]}}"#),
            sound("b",1,r#"{"intro":{"sounds":["b"]}}"#)
        ]).is_err());
        assert!(merge_structured(&[
            sound("a",1,r#"{"intro":{}}"#),
            sound("b",2,"[]")
        ]).is_err());
    }
    #[test]
    fn never_deep_merges_unknown_json_paths() {
        let mut a=sound("a",1,r#"{"first":1}"#);
        let mut b=sound("b",2,r#"{"second":2}"#);
        a.path="assets/demo/other.json".into();
        b.path=a.path.clone();
        let result=merge_structured(&[a,b]).unwrap();
        assert_eq!(result.files["assets/demo/other.json"],br#"{"second":2}"#);
    }
}
