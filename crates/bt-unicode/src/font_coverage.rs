//! Font-owned language declarations shared by the picker and renderer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CjkCoverage {
    pub japanese: bool,
    pub simplified: bool,
    pub traditional: bool,
    pub korean: bool,
    pub han: bool,
    pub hiragana: bool,
    pub katakana: bool,
    pub hangul: bool,
}
impl CjkCoverage {
    pub fn from_code_pages(bits: u32) -> Self {
        let japanese = bits & (1 << 17) != 0;
        let simplified = bits & (1 << 18) != 0;
        let traditional = bits & (1 << 20) != 0;
        let korean = bits & ((1 << 19) | (1 << 21)) != 0;
        Self {
            japanese,
            simplified,
            traditional,
            korean,
            han: japanese || simplified || traditional,
            hiragana: japanese,
            katakana: japanese,
            hangul: korean,
        }
    }
    pub fn any(self) -> bool {
        self.han || self.hiragana || self.katakana || self.hangul
    }
    /// OS/2 is authoritative when it declares a CJK language. Old/odd faces
    /// without that declaration qualify by a majority of an entire Unicode
    /// script block in cmap, never by a representative character. This fallback
    /// establishes script coverage, not a regional-language claim.
    pub fn from_tables(os2: Option<&[u8]>, cmap: Option<&[u8]>) -> Self {
        let bits = os2
            .and_then(|t| t.get(78..82))
            .map(|v| u32::from_be_bytes(v.try_into().unwrap()))
            .unwrap_or(0);
        let declared = Self::from_code_pages(bits);
        if declared.any() {
            return declared;
        }
        let mut points = std::collections::BTreeSet::new();
        if let Some(table) = cmap.and_then(ttf_parser::cmap::Table::parse) {
            for sub in table.subtables.into_iter().filter(|s| s.is_unicode()) {
                sub.codepoints(|c| {
                    let in_cjk_block = matches!(c,
                        0x3041..=0x3096 | 0x30a1..=0x30fa | 0x4e00..=0x9fff | 0xac00..=0xd7a3);
                    if in_cjk_block && sub.glyph_index(c).is_some_and(|g| g.0 != 0) {
                        points.insert(c);
                    }
                });
            }
        }
        let block =
            |start, end| points.range(start..=end).count() * 2 >= (end - start + 1) as usize;
        Self {
            han: block(0x4e00, 0x9fff),
            hiragana: block(0x3041, 0x3096),
            katakana: block(0x30a1, 0x30fa),
            hangul: block(0xac00, 0xd7a3),
            ..Self::default()
        }
    }
}
