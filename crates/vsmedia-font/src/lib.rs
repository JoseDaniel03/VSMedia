//! Build namespaced JSON bitmap-font definitions from registered glyphs.
pub mod registry;
use std::collections::{BTreeMap, BTreeSet};
use vsmedia_api::{AssetKey, MediaError, MediaResult};

#[derive(Clone, Debug)]
pub struct Glyph {
    pub namespace: String,
    /// Path relative to assets/<namespace>/textures/, e.g. font/icons/alert.png.
    pub texture: String,
    /// Use Unicode Private Use Area to avoid clashing with regular text.
    pub codepoint: char,
    pub height: u8,
    pub ascent: i16,
}

pub fn build_font_documents(glyphs: &[Glyph]) -> MediaResult<BTreeMap<String, String>> {
    let mut groups: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    let mut reserved = BTreeSet::new();
    for glyph in glyphs {
        AssetKey::new(&glyph.namespace, &glyph.texture)?;
        if !glyph.texture.starts_with("font/") || !glyph.texture.ends_with(".png")
            || !(('\u{E000}'..='\u{F8FF}').contains(&glyph.codepoint))
            || glyph.height == 0 || glyph.height > 64
            || glyph.ascent < -64 || glyph.ascent > i16::from(glyph.height)
        {
            return Err(MediaError::InvalidInput("invalid glyph definition".into()));
        }
        if !reserved.insert((glyph.namespace.clone(), glyph.codepoint)) {
            return Err(MediaError::Conflict("duplicate glyph codepoint".into()));
        }
        groups.entry(glyph.namespace.clone()).or_default().push(serde_json::json!({
            "type": "bitmap",
            "file": format!("{}:{}", glyph.namespace, glyph.texture),
            "ascent": glyph.ascent,
            "height": glyph.height,
            "chars": [glyph.codepoint.to_string()]
        }));
    }
    groups.into_iter().map(|(namespace, providers)| {
        let text = serde_json::to_string_pretty(&serde_json::json!({ "providers": providers }))
            .map_err(|e| MediaError::Io(e.to_string()))?;
        Ok((namespace, text))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn glyph(codepoint: char) -> Glyph {
        Glyph { namespace: "demo".into(), texture: "font/icons/alert.png".into(),
            codepoint, height: 8, ascent: 7 }
    }
    #[test]
    fn emits_namespaced_bitmap_provider() {
        let output = build_font_documents(&[glyph('\u{E001}')]).unwrap();
        assert!(output["demo"].contains("demo:font/icons/alert.png"));
    }
    #[test]
    fn detects_duplicate_codepoints() {
        assert!(build_font_documents(&[glyph('\u{E001}'), glyph('\u{E001}')]).is_err());
    }
}
