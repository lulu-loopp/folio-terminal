//! Font picker metadata and language projections.
pub use bt_unicode::font_coverage::CjkCoverage;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CjkFamily {
    /// Stable renderer identifier, including values saved before localized display names.
    pub name: String,
    pub files: Vec<std::path::PathBuf>,
    /// Font-owned name records, retained for app-language changes without enumeration.
    pub localized_names: Vec<(String, String)>,
    pub coverage: CjkCoverage,
}
impl CjkFamily {
    pub fn has_name(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
            || self
                .localized_names
                .iter()
                .any(|(_, n)| n.eq_ignore_ascii_case(name))
    }
    pub fn display_name(&self, locale: &str) -> &str {
        self.localized_names
            .iter()
            .find(|(l, _)| l.eq_ignore_ascii_case(locale))
            .or_else(|| {
                self.localized_names
                    .iter()
                    .find(|(l, _)| l.split('-').next() == locale.split('-').next())
            })
            .map_or(&self.name, |(_, n)| n.as_str())
    }
}
/// A reader's language first; YaHei UI remains the first Simplified picker
/// choice even though the terminal grid's Automatic policy prefers NSimSun.
pub fn order_for_language(mut families: Vec<CjkFamily>, locale: &str) -> Vec<CjkFamily> {
    let preferred = |f: &CjkFamily| match locale.split('-').next().unwrap_or("") {
        "zh" => f.coverage.simplified,
        "ja" => f.coverage.japanese,
        "ko" => f.coverage.korean,
        _ => false,
    };
    families.sort_by_key(|f| {
        (
            !preferred(f),
            !(preferred(f) && f.name == "Microsoft YaHei UI"),
            f.display_name(locale).to_lowercase(),
            f.name.clone(),
        )
    });
    let mut seen = std::collections::HashSet::new();
    families.retain(|f| seen.insert(f.name.to_lowercase()));
    families
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cjk_picker_language_order_display_and_saved_identifiers() {
        let jp = CjkFamily {
            name: "A Japanese".into(),
            coverage: CjkCoverage::from_code_pages(1 << 17),
            ..Default::default()
        };
        let sc = CjkFamily {
            name: "Z Chinese".into(),
            localized_names: vec![("zh-CN".into(), "Reader name".into())],
            coverage: CjkCoverage::from_code_pages(1 << 18),
            ..Default::default()
        };
        let ui = CjkFamily {
            name: "Microsoft YaHei UI".into(),
            coverage: sc.coverage,
            ..Default::default()
        };
        let ordered = order_for_language(vec![jp, sc, ui], "zh-CN");
        assert_eq!(
            ordered.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            ["Microsoft YaHei UI", "Z Chinese", "A Japanese"]
        );
        assert_eq!(ordered[1].display_name("zh-CN"), "Reader name");
        assert_eq!(ordered[1].display_name("en-US"), "Z Chinese");
        assert!(ordered[1].has_name("Z Chinese"));
        assert!(ordered[1].has_name("Reader name"));
    }
    #[test]
    fn cjk_undeclared_faces_need_block_coverage_not_one_sample() {
        let cmap = |start: u32, end: u32| {
            let mut bytes = vec![0, 0, 0, 1, 0, 3, 0, 10, 0, 0, 0, 12, 0, 12, 0, 0];
            for value in [28u32, 0, 1, start, end, 1] {
                bytes.extend_from_slice(&value.to_be_bytes());
            }
            bytes
        };
        assert!(!CjkCoverage::from_tables(None, Some(&cmap(0x4f60, 0x4f60))).han);
        let han = CjkCoverage::from_tables(None, Some(&cmap(0x4e00, 0x9fff)));
        assert!(han.han);
        assert!(!han.simplified && !han.japanese && !han.traditional);
        assert!(CjkCoverage::from_tables(None, Some(&cmap(0x3041, 0x3096))).hiragana);
        assert!(CjkCoverage::from_tables(None, Some(&cmap(0xac00, 0xd7a3))).hangul);
    }
    #[test]
    fn cjk_os2_language_bits_are_font_owned() {
        for (bit, expected) in [
            (17, (true, false, false, false)),
            (18, (false, true, false, false)),
            (19, (false, false, false, true)),
            (20, (false, false, true, false)),
            (21, (false, false, false, true)),
        ] {
            let c = CjkCoverage::from_code_pages(1 << bit);
            assert_eq!(
                (c.japanese, c.simplified, c.traditional, c.korean),
                expected
            );
        }
        assert!(!CjkCoverage::from_tables(None, None).any());
    }
}
