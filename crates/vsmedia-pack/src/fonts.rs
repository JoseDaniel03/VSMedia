//! Non-destructive font provider diagnostics for merged resource packs.
//! Do not concatenate unknown provider schemas or claim to fully determine
//! Minecraft's actual glyph resolution (TTF/unihex/reference are opaque).
use crate::PackAsset;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap,BTreeSet};

const MAX_FONT_JSON:usize=2*1024*1024;
const MAX_GLYPHS:usize=65_536;
const MAX_COLLISIONS:usize=300;
const MAX_PROVIDERS:usize=2_048;

#[derive(Debug,Serialize)]
pub struct GlyphCollision {
    pub font:String,
    pub codepoint:String,
    pub sources:Vec<String>,
}
#[derive(Debug,Serialize)]
pub struct FontReview {
    pub schema:&'static str,
    pub font_files:usize,
    pub total_providers:usize,
    pub opaque_providers:usize,
    pub shared_codepoints:usize,
    pub sample:Vec<GlyphCollision>,
    pub warnings:Vec<String>,
    pub truncated:bool,
}
fn font_path(path:&str)->bool{
    path.starts_with("assets/")&&path.contains("/font/")&&path.ends_with(".json")
}
fn characters(provider:&Value)->Option<BTreeSet<u32>>{
    let mut chars=BTreeSet::new();
    match provider.get("type")?.as_str()? {
        "bitmap"=>{
            for row in provider.get("chars")?.as_array()? {
                for ch in row.as_str()?.chars() {
                    if ch!='\0'{chars.insert(ch as u32);}
                    if chars.len()>MAX_GLYPHS{return None;}
                }
            }
        },
        "space"=>{
            for key in provider.get("advances")?.as_object()?.keys(){
                for ch in key.chars(){
                    if ch!='\0'{chars.insert(ch as u32);}
                    if chars.len()>MAX_GLYPHS{return None;}
                }
            }
        },
        _=>return None
    }
    Some(chars)
}
pub fn review(inputs:&[PackAsset])->FontReview {
    let mut result=FontReview{
        schema:"vspack-font-review-v1",
        font_files:0,total_providers:0,opaque_providers:0,
        shared_codepoints:0,sample:vec![],warnings:vec![],truncated:false
    };
    let mut all=BTreeMap::<(String,u32),BTreeSet<String>>::new();
    for asset in inputs.iter().filter(|p|font_path(&p.path)){
        result.font_files+=1;
        if asset.data.len()>MAX_FONT_JSON {
            result.warnings.push(format!("{}: font JSON exceeds 2 MiB",asset.path));
            continue;
        }
        let root:Value=match serde_json::from_slice(&asset.data){
            Ok(v)=>v,
            Err(_)=>{
                result.warnings.push(format!("{}: font JSON is not valid",asset.path));
                continue
            }
        };
        let Some(providers)=root.get("providers").and_then(Value::as_array) else {
            result.warnings.push(format!("{}: no providers list",asset.path));
            continue;
        };
        if providers.len()>MAX_PROVIDERS{
            result.warnings.push(format!("{}: providers exceed review limit",asset.path));
            result.truncated=true;continue;
        }
        result.total_providers+=providers.len();
        for provider in providers {
            let Some(chars)=characters(provider) else {
                result.opaque_providers+=1;
                continue
            };
            for codepoint in chars {
                let key=(asset.path.clone(),codepoint);
                if all.len()>=MAX_GLYPHS && !all.contains_key(&key){
                    result.truncated=true;continue;
                }
                all.entry(key).or_default().insert(asset.source.clone());
            }
        }
    }
    for ((font,ch),sources) in all {
        if sources.len()<2{continue;}
        result.shared_codepoints+=1;
        if result.sample.len()<MAX_COLLISIONS {
            result.sample.push(GlyphCollision{
                font,codepoint:format!("U+{ch:04X}"),
                sources:sources.into_iter().collect()
            });
        }else{result.truncated=true;}
    }
    if result.opaque_providers>0{
        result.warnings.push(
            "Some providers are opaque (e.g. TTF/unihex/reference); glyph overlap analysis is partial".into()
        );
    }
    if result.warnings.len()>MAX_COLLISIONS {
        result.warnings.truncate(MAX_COLLISIONS);
        result.truncated=true;
    }
    result
}
#[cfg(test)]
mod tests{
    use super::*;
    fn font(source:&str,json:&str)->PackAsset{
        PackAsset{source:source.into(),
            path:"assets/demo/font/default.json".into(),
            priority:0,data:json.as_bytes().to_vec()}
    }
    #[test]
    fn detects_private_unicode_bitmap_and_space_collisions(){
        let one=font("cozy",r#"{"providers":[{"type":"bitmap","file":"demo:font/icons.png","chars":["\uE000\uE001"]}]}"#);
        let two=font("icons",r#"{"providers":[{"type":"space","advances":{"\uE000":-3,"\uE002":4}}]}"#);
        let report=review(&[one,two]);
        assert_eq!(report.font_files,2);
        assert_eq!(report.shared_codepoints,1);
        assert_eq!(report.sample[0].codepoint,"U+E000");
        assert_eq!(report.sample[0].sources,vec!["cozy","icons"]);
        assert_eq!(report.opaque_providers,0);
    }
    #[test]
    fn does_not_flag_glyphs_twice_within_a_single_pack(){
        let a=font("cozy",r#"{"providers":[
            {"type":"bitmap","chars":["\uE000\uE001"]},
            {"type":"space","advances":{"\uE000":2}}]}"#);
        let b=font("alt",r#"{"providers":[{"type":"ttf","file":"demo:font/a.ttf"}]}"#);
        let report=review(&[a,b]);
        assert_eq!(report.shared_codepoints,0);
        assert_eq!(report.opaque_providers,1);
        assert!(!report.warnings.is_empty());
    }
}
