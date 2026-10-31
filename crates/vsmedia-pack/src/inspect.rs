//! Bounded, read-only diagnostics for merged Minecraft Resource Packs.
//! All missing references are REVIEW hints, never hard errors: a resource
//! may legitimately be provided by vanilla Minecraft, a mod, or another pack.
use crate::{split,MergeOutput};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

const MAX_JSON:usize=2*1024*1024;
const MAX_ISSUES:usize=300;
const MAX_REFERENCES:usize=40_000;

#[derive(Debug,Clone,PartialEq,Eq,Serialize)]
pub struct Issue {
    pub code:&'static str,
    pub severity:&'static str,
    pub path:String,
    pub reference:Option<String>,
    pub message:&'static str,
}
#[derive(Debug,Clone,PartialEq,Eq,Serialize)]
pub struct Dependency {
    pub source:String,
    pub target:String,
    pub source_category:&'static str,
    pub target_category:&'static str,
}
#[derive(Debug,Serialize)]
pub struct Inspection {
    pub schema:&'static str,
    pub files:usize,
    pub examined_json:usize,
    pub known_references:usize,
    pub missing_references:usize,
    pub cross_category_references:usize,
    pub orphan_animation_metadata:usize,
    pub truncated:bool,
    pub issues:Vec<Issue>,
    pub cross_category_examples:Vec<Dependency>,
}
fn asset_path(namespace:&str,location:&str,folder:&str,suffix:&str)->Option<String>{
    if location.starts_with('#')||location.starts_with("builtin/") {return None;}
    let (ns,resource)=location.split_once(':')
        .unwrap_or((namespace,location));
    if ns.is_empty()||resource.is_empty()||resource.starts_with('/')
        ||resource.contains("..")||resource.contains('\\')
        ||resource.chars().any(|ch|ch.is_control()) {return None;}
    Some(format!("assets/{ns}/{folder}/{resource}{suffix}"))
}
fn ns_of(path:&str)->Option<&str>{
    let rest=path.strip_prefix("assets/")?;
    rest.split_once('/').map(|(ns,_)|ns)
}
fn is_model(path:&str)->bool{
    path.starts_with("assets/")&&path.contains("/models/")&&path.ends_with(".json")
}
fn is_font(path:&str)->bool{
    path.starts_with("assets/")&&path.contains("/font/")&&path.ends_with(".json")
}
fn references(path:&str,data:&[u8])->Option<Vec<String>> {
    if data.len()>MAX_JSON{return None;}
    let ns=ns_of(path)?;
    if !is_model(path)&&!is_font(path){return None;}
    let json:Value=serde_json::from_slice(data).ok()?;
    let mut refs=Vec::new();
    if is_model(path){
        if let Some(parent)=json.get("parent").and_then(Value::as_str){
            if let Some(target)=asset_path(ns,parent,"models",".json"){
                refs.push(target);
            }
        }
        if let Some(textures)=json.get("textures").and_then(Value::as_object){
            for texture in textures.values().filter_map(Value::as_str){
                if let Some(target)=asset_path(ns,texture,"textures",".png"){
                    refs.push(target);
                }
            }
        }
    }
    if is_font(path){
        if let Some(providers)=json.get("providers").and_then(Value::as_array){
            for provider in providers.iter().take(2_048){
                if provider.get("type").and_then(Value::as_str)==Some("bitmap"){
                    if let Some(file)=provider.get("file").and_then(Value::as_str){
                        if let Some(target)=asset_path(ns,file,"textures",".png"){
                            refs.push(target);
                        }
                    }
                }
            }
        }
    }
    Some(refs)
}
/// Analyzes the assembled archive without assuming vanilla/mod assets are
/// present in the provided ZIP. Reports are bounded to avoid JSON bombs.
pub fn inspect(output:&MergeOutput)->Inspection{
    let mut result=Inspection{
        schema:"vspack-inspection-v1",
        files:output.files.len(),examined_json:0,known_references:0,
        missing_references:0,cross_category_references:0,
        orphan_animation_metadata:0,truncated:false,
        issues:Vec::new(),cross_category_examples:Vec::new()
    };
    let mut referenced=BTreeMap::<String,Vec<String>>::new();
    for (path,bytes) in &output.files{
        if path.ends_with(".png.mcmeta"){
            let texture=path.trim_end_matches(".mcmeta");
            if !output.files.contains_key(texture){
                result.orphan_animation_metadata+=1;
                push_issue(&mut result,Issue{
                    code:"orphan_animation_metadata",severity:"warning",
                    path:path.clone(),reference:Some(texture.into()),
                    message:"Animation metadata lacks its PNG in the assembled pack"
                });
            }
        }
        if (is_font(path)||is_model(path))&&bytes.len()<=MAX_JSON {
            result.examined_json+=1;
            if let Some(refs)=references(path,bytes){
                for target in refs{
                    if result.known_references>=MAX_REFERENCES{
                        result.truncated=true;break;
                    }
                    result.known_references+=1;
                    referenced.entry(target).or_default().push(path.clone());
                }
            }else{
                push_issue(&mut result,Issue{
                    code:"unreadable_reference_json",severity:"warning",
                    path:path.clone(),reference:None,
                    message:"Cannot inspect this model/font JSON; check its syntax"
                });
            }
        }
    }
    for (target,from) in referenced{
        for path in from {
            if output.files.contains_key(&target){
                let left=split::group_for(&path);
                let right=split::group_for(&target);
                if left!=right {
                    result.cross_category_references+=1;
                    if result.cross_category_examples.len()<MAX_ISSUES{
                        result.cross_category_examples.push(Dependency{
                            source:path,target:target.clone(),
                            source_category:left,target_category:right
                        });
                    }else{result.truncated=true;}
                }
            }else{
                result.missing_references+=1;
                push_issue(&mut result,Issue{
                    code:"reference_not_in_pack",severity:"info",
                    path,reference:Some(target.clone()),
                    message:"Referenced file is not in this pack; vanilla or a mod may provide it"
                });
            }
        }
    }
    result
}
fn push_issue(result:&mut Inspection,issue:Issue){
    if result.issues.len()<MAX_ISSUES {result.issues.push(issue);}
    else{result.truncated=true;}
}

#[cfg(test)]
mod tests{
    use super::*;
    use std::collections::BTreeMap;
    fn pack(files:&[(&str,&[u8])])->MergeOutput{
        MergeOutput{
            files:files.iter().map(|(p,d)|(p.to_string(),d.to_vec())).collect::<BTreeMap<_,_>>(),
            conflicts:vec![]
        }
    }
    #[test]
    fn finds_merged_font_bitmap_dependency_and_cross_category_model_reference(){
        let model=br#"{"parent":"builtin/generated","textures":{"layer0":"demo:item/sword"}}"#;
        let font=br#"{"providers":[{"type":"bitmap","file":"demo:font/tooltip.png","chars":["\uE000"]}]}"#;
        let output=pack(&[
            ("pack.mcmeta",br#"{"pack":{"description":"demo"}}"#),
            ("assets/demo/font/default.json",font),
            ("assets/demo/textures/font/tooltip.png",b"FONT"),
            ("assets/demo/models/item/sword.json",model),
            ("assets/demo/textures/item/sword.png",b"SPRITE"),
            ("assets/demo/textures/gui/tooltip.png.mcmeta",b"{}"),
        ]);
        let report=inspect(&output);
        assert_eq!(report.known_references,2);
        assert_eq!(report.missing_references,0);
        assert_eq!(report.orphan_animation_metadata,1);
        assert_eq!(report.issues[0].code,"orphan_animation_metadata");
        assert_eq!(report.cross_category_references,0);
    }
    #[test]
    fn reports_absent_files_without_mistaking_minecraft_fallback_for_failure(){
        let output=pack(&[(
            "assets/demo/models/item/sword.json",
            br#"{"parent":"minecraft:item/generated","textures":{"layer0":"demo:item/sword"}}"#
        )]);
        let report=inspect(&output);
        assert_eq!(report.known_references,2);
        assert_eq!(report.missing_references,2);
        assert!(report.issues.iter().all(|x|x.severity=="info"));
    }
    #[test]
    fn ignores_strings_that_could_be_unsafe_file_references(){
        assert!(asset_path("minecraft","../secret","textures",".png").is_none());
        assert!(asset_path("minecraft","#layer0","textures",".png").is_none());
        assert!(asset_path("minecraft","builtin/generated","models",".json").is_none());
    }
}
