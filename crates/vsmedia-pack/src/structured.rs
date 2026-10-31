//! Schema-aware resource-pack merging. Only known JSON document shapes are combined.
//! Unknown assets, textures, arbitrary JSON and pack.mcmeta use explicit file priorities.
use std::collections::BTreeMap;
use serde_json::{Map, Value};
use vsmedia_api::{MediaError, MediaResult};
use super::{merge, validate_path, MergeOutput, PackAsset, PackConflict};

const MAX_JSON_BYTES:usize=2*1024*1024;
const MAX_SOUND_ENTRIES:usize=16_384;
// Bounded by Studio's 384-MiB decoded resource quota; enough for many GUI packs.
const MAX_INPUTS:usize=65_536;

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
        } else if is_language(path) {
            merge_languages(path,&group)?
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
/// Localization files (assets/<namespace>/lang/<locale>.json) are merged by
/// translation KEY, just as Minecraft combines keys from stacked packs.
/// Unlike arbitrary JSON, this format has a flat, well-known schema.
fn is_language(path:&str)->bool{
    let parts=path.split('/').collect::<Vec<_>>();
    parts.len()==4 && parts[0]=="assets" && !parts[1].is_empty()
        && parts[2]=="lang" && parts[3].ends_with(".json")
}
/// Normalize JSONC line/block comments without ever stripping URL characters
/// or // sequences inside quoted strings. Used only for Minecraft lang JSON
/// files; arbitrary JSON and manifests remain strict JSON.
fn strip_json_comments(data:&[u8])->MediaResult<Vec<u8>>{
    let mut out=Vec::with_capacity(data.len());
    let mut pos=0usize;
    let mut string=false;
    let mut escaped=false;
    while pos<data.len(){
        let ch=data[pos];
        if string {
            out.push(ch);
            if escaped{escaped=false;}
            else if ch==b'\\'{escaped=true;}
            else if ch==b'"'{string=false;}
            pos+=1;continue;
        }
        if ch==b'"'{string=true;out.push(ch);pos+=1;continue;}
        if ch==b'/'&&pos+1<data.len()&&data[pos+1]==b'/'{
            pos+=2;
            while pos<data.len()&&data[pos]!=b'\n'&&data[pos]!=b'\r'{pos+=1;}
            continue;
        }
        if ch==b'/'&&pos+1<data.len()&&data[pos+1]==b'*'{
            pos+=2;let mut closed=false;
            while pos+1<data.len(){
                if data[pos]==b'*'&&data[pos+1]==b'/'{
                    pos+=2;closed=true;break;
                }
                if data[pos]==b'\n'||data[pos]==b'\r'{out.push(data[pos]);}
                pos+=1;
            }
            if !closed{
                return Err(MediaError::InvalidInput("unterminated lang JSON comment".into()));
            }
            out.push(b' ');
            continue;
        }
        out.push(ch);pos+=1;
    }
    Ok(out)
}
fn merge_languages(path:&str,group:&[PackAsset])->MediaResult<MergeOutput>{
    let mut entries:BTreeMap<String,Vec<(i32,String,String)>>=BTreeMap::new();
    for source in group {
        if source.data.len()>MAX_JSON_BYTES{
            return Err(MediaError::InvalidInput(format!("language JSON exceeds 2 MiB: {path}")));
        }
        let sanitized=strip_json_comments(&source.data)?;
        let value:Value=serde_json::from_slice(&sanitized)
            .map_err(|e|MediaError::InvalidInput(format!("invalid lang JSON at {path}: {e}")))?;
        let root=value.as_object().ok_or_else(||
            MediaError::InvalidInput(format!("language must be JSON object: {path}")))?;
        if root.len()>65_536{
            return Err(MediaError::InvalidInput("too many translation keys".into()));
        }
        for (key,value) in root {
            let translated=value.as_str().ok_or_else(||
                MediaError::InvalidInput("translations must contain strings".into()))?;
            if key.is_empty()||key.len()>512{
                return Err(MediaError::InvalidInput("invalid translation key".into()));
            }
            entries.entry(key.clone()).or_default()
                .push((source.priority,source.source.clone(),translated.into()));
        }
        if entries.len()>65_536{
            return Err(MediaError::InvalidInput("too many merged translation keys".into()));
        }
    }
    let mut document=Map::new();
    let mut conflicts=Vec::new();
    for (key,mut candidates) in entries {
        candidates.sort_by(|a,b|b.0.cmp(&a.0).then_with(||a.1.cmp(&b.1)));
        let winner=&candidates[0];
        for other in candidates.iter().skip(1) {
            if winner.2==other.2{continue;}
            if winner.0==other.0{
                return Err(MediaError::Conflict(format!(
                    "equal-priority translation collision in {path}: {key} ({} vs {})",
                    winner.1,other.1
                )));
            }
            conflicts.push(PackConflict{
                path:format!("{path}#{key}"),
                winner:winner.1.clone(),loser:other.1.clone()
            });
        }
        document.insert(key,Value::String(winner.2.clone()));
    }
    let bytes=serde_json::to_vec(&Value::Object(document))
        .map_err(|e|MediaError::Io(e.to_string()))?;
    if bytes.len()>MAX_JSON_BYTES{
        return Err(MediaError::InvalidInput("merged language exceeds size quota".into()));
    }
    Ok(MergeOutput {files:BTreeMap::from([(path.into(),bytes)]),conflicts})
}

fn merge_sounds(path:&str,group:&[PackAsset])->MediaResult<MergeOutput> {
    // Select the best contribution for each event before considering
    // conflicts. Shadowed, lower-priority ties must not break the pack.
    let mut contributions:BTreeMap<String,Vec<(i32,String,Value)>>=BTreeMap::new();
    for asset in group {
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
            contributions.entry(event.clone()).or_default()
                .push((asset.priority,asset.source.clone(),value.clone()));
        }
        if contributions.len()>MAX_SOUND_ENTRIES {
            return Err(MediaError::InvalidInput("merged sounds.json exceeds event quota".into()));
        }
    }
    let mut document=Map::new();
    let mut conflicts=Vec::new();
    for (event, mut choices) in contributions {
        choices.sort_by(|a,b|b.0.cmp(&a.0).then_with(||a.1.cmp(&b.1)));
        let winner=&choices[0];
        for other in choices.iter().skip(1) {
            if winner.2==other.2 {continue;}
            if winner.0==other.0 {
                return Err(MediaError::Conflict(format!(
                    "equal-priority sound event collision in {path}: {event} ({} vs {})",
                    winner.1,other.1
                )));
            }
            conflicts.push(PackConflict {
                path:format!("{path}#{event}"),winner:winner.1.clone(),
                loser:other.1.clone()
            });
        }
        document.insert(event,winner.2.clone());
    }
    let bytes=serde_json::to_vec(&Value::Object(document))
        .map_err(|e|MediaError::Io(e.to_string()))?;
    if bytes.len()>MAX_JSON_BYTES {
        return Err(MediaError::InvalidInput("merged sounds.json exceeds size quota".into()));
    }
    Ok(MergeOutput {
        files:BTreeMap::from([(path.into(),bytes)]),conflicts,
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
    fn effective_event_winner_ignores_shadowed_ties_regardless_of_import_order(){
        let first=sound("one",1,r#"{"click":{"sounds":["one"]}}"#);
        let second=sound("two",1,r#"{"click":{"sounds":["two"]}}"#);
        let winning=sound("latest",5,r#"{"click":{"sounds":["latest"]}}"#);
        let one=merge_structured(&[first.clone(),second.clone(),winning.clone()]).unwrap();
        let two=merge_structured(&[winning,second,first]).unwrap();
        assert_eq!(one.files,two.files);
        assert_eq!(one.conflicts,two.conflicts);
        assert_eq!(one.conflicts.len(),2);
        assert!(one.conflicts.iter().all(|c|c.winner=="latest"));
    }
    #[test]
    fn merges_language_keys_and_sanitizes_comments_without_damaging_strings(){
        let path="assets/demo/lang/en_us.json";
        let base=PackAsset{source:"cozy".into(),path:path.into(),priority:40,
            data:br#"{
              // tooltip and icon text
              "container.chest":"Chest",
              "website":"https://example.org/docs//safe",
              "tooltip":"Click"
            }"#.to_vec()};
        let icons=PackAsset{source:"enchant".into(),path:path.into(),priority:50,
            data:br#"{"enchantment.level.1":"I","container.chest":"Fancy Chest"}"#.to_vec()};
        let one=merge_structured(&[base.clone(),icons.clone()]).unwrap();
        let two=merge_structured(&[icons,base]).unwrap();
        assert_eq!(one.files,two.files);
        assert_eq!(one.conflicts,two.conflicts);
        let doc:Value=serde_json::from_slice(&one.files[path]).unwrap();
        assert_eq!(doc["container.chest"],"Fancy Chest");
        assert_eq!(doc["website"],"https://example.org/docs//safe");
        assert_eq!(doc["enchantment.level.1"],"I");
        assert_eq!(doc["tooltip"],"Click");
        assert_eq!(one.conflicts.len(),1);
        assert_eq!(one.conflicts[0].path,format!("{path}#container.chest"));
    }
    #[test]
    fn language_comment_and_priority_safety(){
        let path="assets/demo/lang/en_us.json";
        let source=|name:&str,priority:i32,json:&str|PackAsset{
            source:name.into(),path:path.into(),priority,data:json.as_bytes().to_vec()
        };
        let old=source("old",1,r#"{"title":"first"}"#);
        let middle=source("middle",1,r#"{"title":"other"}"#);
        let newer=source("new",10,r#"{"title":"latest"}"#);
        let final_pack=merge_structured(&[old,middle,newer]).unwrap();
        assert_eq!(final_pack.conflicts.len(),2);
        assert!(merge_structured(&[
            source("a",2,r#"{"title":"a"}"#),
            source("b",2,r#"{"title":"b"}"#)
        ]).is_err());
        assert!(merge_structured(&[source("bad",1,"{/*not closed")]).is_err());
        assert!(merge_structured(&[source("bad",1,r#"{"key":42}"#)]).is_err());
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
