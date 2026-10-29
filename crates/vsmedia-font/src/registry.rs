//! Central glyph allocator to prevent colliding private-use codepoints across producer plugins.
use super::{build_font_documents, Glyph};
use std::collections::BTreeSet;
use vsmedia_api::{AssetKey, MediaError, MediaResult};

#[derive(Default)]
pub struct GlyphRegistry {
    glyphs: Vec<Glyph>,
    used: BTreeSet<char>,
}
impl GlyphRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn reserve(&mut self, glyph: Glyph) -> MediaResult<()> {
        AssetKey::new(&glyph.namespace, &glyph.texture)?;
        build_font_documents(std::slice::from_ref(&glyph))?;
        if !self.used.insert(glyph.codepoint) {
            return Err(MediaError::Conflict(format!("glyph codepoint already allocated: U+{:04X}", glyph.codepoint as u32)));
        }
        self.glyphs.push(glyph);
        Ok(())
    }

    pub fn allocate(
        &mut self, namespace: &str, texture: &str, height: u8, ascent: i16,
    ) -> MediaResult<Glyph> {
        for cp in 0xE000..=0xF8FF {
            let codepoint = char::from_u32(cp).expect("PUA is valid Unicode");
            if self.used.contains(&codepoint) { continue; }
            let glyph = Glyph {
                namespace: namespace.into(), texture: texture.into(),
                codepoint, height, ascent,
            };
            self.reserve(glyph.clone())?;
            return Ok(glyph);
        }
        Err(MediaError::Conflict("Unicode private-use glyph space exhausted".into()))
    }

    pub fn glyphs(&self) -> &[Glyph] { &self.glyphs }
    pub fn render(&self) -> MediaResult<std::collections::BTreeMap<String, String>> {
        build_font_documents(&self.glyphs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allocates_unique_codepoints_across_plugins() {
        let mut fonts = GlyphRegistry::new();
        let a = fonts.allocate("vsrender", "font/a.png", 8, 7).unwrap();
        let b = fonts.allocate("typewriter", "font/b.png", 8, 7).unwrap();
        assert_ne!(a.codepoint, b.codepoint);
        assert!(fonts.reserve(a).is_err());
        assert_eq!(fonts.glyphs().len(), 2);
        assert_eq!(fonts.render().unwrap().len(), 2);
    }
    #[test]
    fn refuses_invalid_glyph_before_registration() {
        let mut fonts = GlyphRegistry::new();
        assert!(fonts.allocate("demo", "../bad.png", 8, 7).is_err());
        assert!(fonts.glyphs().is_empty());
    }
}
