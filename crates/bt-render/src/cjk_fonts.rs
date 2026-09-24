//! Per-font-database metadata, rebuilt only when font settings change.
use super::*;
use std::ops::{Deref, DerefMut};

pub struct FontSystem {
    inner: glyphon::FontSystem,
    cjk: OnceLock<Arc<CjkCatalog>>,
    /// What the grid's primary family (`Family::Monospace`) offers for each
    /// asked weight and style — [`family_face_matches`] of that one family.
    /// Cleared with the catalogue by [`FontSystem::db_mut`], which is also the
    /// only road to `set_monospace_family`.
    primary: OnceLock<Arc<FamilyFaces>>,
}
impl Deref for FontSystem {
    type Target = glyphon::FontSystem;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl DerefMut for FontSystem {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
impl Default for FontSystem {
    fn default() -> Self {
        Self::new()
    }
}
impl FontSystem {
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    pub fn new() -> Self {
        Self {
            inner: glyphon::FontSystem::new(),
            cjk: OnceLock::new(),
            primary: OnceLock::new(),
        }
    }
    pub fn new_with_locale_and_db_and_fallback(
        locale: String,
        db: glyphon::fontdb::Database,
        fallback: impl Fallback + 'static,
    ) -> Self {
        let fonts = Self {
            inner: glyphon::FontSystem::new_with_locale_and_db_and_fallback(locale, db, fallback),
            cjk: OnceLock::new(),
            primary: OnceLock::new(),
        };
        // Pay metadata resolution during font setup, before a settings frame.
        let _ = fonts.cjk_catalog();
        fonts
    }
    pub fn db_mut(&mut self) -> &mut glyphon::fontdb::Database {
        self.cjk.take();
        self.primary.take();
        self.inner.db_mut()
    }
    pub(super) fn cjk_catalog(&self) -> Arc<CjkCatalog> {
        Arc::clone(
            self.cjk
                .get_or_init(|| Arc::new(CjkCatalog::read(self.inner.db()))),
        )
    }
    pub(super) fn primary_faces(&self) -> Arc<FamilyFaces> {
        Arc::clone(self.primary.get_or_init(|| {
            let db = self.inner.db();
            Arc::new(FamilyFaces {
                matches: family_face_matches(db, db.family_name(&Family::Monospace)),
            })
        }))
    }
}
/// One family's answer to "which face do you offer for this (weight, style)",
/// as `(asked weight, asked style, face weight, face style)` rows.
///
/// **The one derivation** (CONVENTIONS §十 rule 9) behind both the CJK
/// catalogue ([`CjkFace`]) and the grid's primary family ([`FamilyFaces`]):
/// fontdb CSS-matches inside the named family, and a face whose `wght` axis
/// spans the asked weight answers with the asked weight itself, because the
/// rasterizer moves the axis there.
fn family_face_matches(
    db: &glyphon::fontdb::Database,
    name: &str,
) -> Vec<(Weight, Style, Weight, Style)> {
    let mut matches = Vec::new();
    for weight in [
        Weight::NORMAL,
        Weight::MEDIUM,
        Weight::SEMIBOLD,
        Weight::BOLD,
    ] {
        for style in [Style::Normal, Style::Italic, Style::Oblique] {
            let found = db
                .query(&glyphon::fontdb::Query {
                    families: &[Family::Name(name)],
                    weight,
                    style,
                    stretch: Stretch::Normal,
                })
                .and_then(|id| db.face(id));
            if let Some(face) = found {
                matches.push((
                    weight,
                    style,
                    if wght_axis_reaches(db, face.id, weight) {
                        weight
                    } else {
                        face.weight
                    },
                    face.style,
                ));
            }
        }
    }
    matches
}
/// Whether a face's `wght` axis spans `weight` — the case in which the
/// rasterizer draws that weight from the one face, by moving the axis.
pub(super) fn wght_axis_reaches(
    db: &glyphon::fontdb::Database,
    id: glyphon::fontdb::ID,
    weight: Weight,
) -> bool {
    db.with_face_data(id, |bytes, index| {
        ttf_parser::Face::parse(bytes, index).ok().is_some_and(|f| {
            f.variation_axes().into_iter().any(|axis| {
                axis.tag == ttf_parser::Tag::from_bytes(b"wght")
                    && f32::from(weight.0) >= axis.min_value
                    && f32::from(weight.0) <= axis.max_value
            })
        })
    })
    .unwrap_or(false)
}
/// The asked attributes with the weight replaced by the face's own, when the
/// family's table has a row for the asked (weight, style). The STYLE stays what
/// was asked: a family with no italic cut CSS-matches its upright face, and the
/// shaper synthesises the slant only while the request still says italic
/// (closure review F1, 2026-09-20).
fn offered_attrs<'a>(
    matches: &[(Weight, Style, Weight, Style)],
    mut attrs: Attrs<'a>,
) -> Attrs<'a> {
    if let Some((_, _, weight, _)) = matches
        .iter()
        .find(|(w, s, _, _)| *w == attrs.weight && *s == attrs.style)
    {
        attrs.weight = *weight;
    }
    attrs
}
/// The grid's primary family's rows of [`family_face_matches`].
#[derive(Debug)]
pub(super) struct FamilyFaces {
    matches: Vec<(Weight, Style, Weight, Style)>,
}
#[derive(Debug)]
pub(super) struct CjkFace {
    pub name: String,
    pub coverage: bt_unicode::font_coverage::CjkCoverage,
    points: Vec<u32>,
    matches: Vec<(Weight, Style, Weight, Style)>,
}
impl CjkFace {
    pub fn covers(&self, text: &str) -> bool {
        text.chars()
            .filter(|c| !matches!(*c as u32, 0xfe00..=0xfe0f | 0xe0100..=0xe01ef | 0x200d))
            .all(|c| self.points.binary_search(&(c as u32)).is_ok())
    }
    /// The weight is the face's own; the style stays what was asked — see
    /// [`offered_attrs`]. In the terminal grid a bold request on a face with no
    /// bold cut is then emboldened downstream from that face's own outline, and
    /// the slant by the shaper.
    fn attrs<'a>(&self, attrs: Attrs<'a>) -> Attrs<'a> {
        offered_attrs(&self.matches, attrs)
    }
}
#[derive(Debug)]
pub(super) struct CjkCatalog {
    pub faces: Vec<Arc<CjkFace>>,
    pub proportional: Vec<Arc<CjkFace>>,
}
impl CjkCatalog {
    fn read(db: &glyphon::fontdb::Database) -> Self {
        if std::env::var_os("BT_PERF_TRACE").is_some_and(|v| !v.is_empty()) {
            for face in db.faces() {
                let file = match &face.source {
                    glyphon::fontdb::Source::File(p)
                    | glyphon::fontdb::Source::SharedFile(p, _) => p.display().to_string(),
                    _ => "<memory>".into(),
                };
                bt_viewport::trace::line(format!(
                    "BT_PERF_TRACE chrome_font_db font_id={:?} families={:?} file={file:?} face_weight={} style={:?}",
                    face.id, face.families, face.weight.0, face.style
                ));
            }
        }
        let mut faces = Vec::new();
        let names: std::collections::BTreeSet<_> = db
            .faces()
            .flat_map(|f| f.families.iter().map(|(n, _)| n.clone()))
            .collect();
        for name in names {
            let query = |weight, style| {
                db.query(&glyphon::fontdb::Query {
                    families: &[Family::Name(&name)],
                    weight,
                    style,
                    stretch: Stretch::Normal,
                })
            };
            let Some(id) = query(Weight::NORMAL, Style::Normal) else {
                continue;
            };
            let Some((coverage, points)) = db
                .with_face_data(id, |bytes, index| {
                    let face = ttf_parser::Face::parse(bytes, index).ok()?;
                    let raw = face.raw_face();
                    let coverage = bt_unicode::font_coverage::CjkCoverage::from_tables(
                        raw.table(ttf_parser::Tag::from_bytes(b"OS/2")),
                        raw.table(ttf_parser::Tag::from_bytes(b"cmap")),
                    );
                    if !coverage.any() {
                        return None;
                    }
                    let mut points = std::collections::BTreeSet::new();
                    for sub in face
                        .tables()
                        .cmap?
                        .subtables
                        .into_iter()
                        .filter(|s| s.is_unicode())
                    {
                        sub.codepoints(|c| {
                            if sub.glyph_index(c).is_some_and(|g| g.0 != 0) {
                                points.insert(c);
                            }
                        });
                    }
                    Some((coverage, points.into_iter().collect()))
                })
                .flatten()
            else {
                continue;
            };
            let matches = family_face_matches(db, &name);
            faces.push(Arc::new(CjkFace {
                name,
                coverage,
                points,
                matches,
            }));
        }
        let proportional = platform_cjk_fallback_families()
            .iter()
            .filter_map(|name| {
                faces
                    .iter()
                    .find(|face| face.name.eq_ignore_ascii_case(name))
                    .cloned()
            })
            .collect();
        Self {
            faces,
            proportional,
        }
    }
}
/// Family ownership is decided before matching weight/style. fontdb applies
/// CSS matching inside that family; cosmic-text must receive the resulting
/// face attributes because its named-family iterator rejects weight misses.
/// A missing bold/italic cut uses the family's available face. In the terminal
/// grid a bold request on such a face is then drawn heavier from that face's own
/// outline (`synthetic_bold`, ticket 38, 2026-09-24 — superseding the sentence
/// of 2026-09-20 that said no emboldening would be added); chrome labels and
/// previews draw the available face as it is.
pub(super) fn match_cjk_attrs<'a>(fs: &FontSystem, attrs: Attrs<'a>) -> Attrs<'a> {
    let Family::Name(name) = attrs.family else {
        return attrs;
    };
    let catalogue = fs.cjk_catalog();
    catalogue
        .faces
        .iter()
        .find(|f| f.name.eq_ignore_ascii_case(name))
        .map_or_else(|| attrs.clone(), |f| f.attrs(attrs.clone()))
}
/// **The terminal grid's weight match: the family that draws a cluster never
/// depends on the weight asked** (2026-09-20 ruling, `1ddd516c`, for the
/// resolved family — CJK and primary alike).
///
/// A named family goes through [`match_cjk_attrs`]. `Family::Monospace`, the
/// primary, gets the same swap from its own rows of [`family_face_matches`]:
/// without it cosmic-text is asked for a weight the family does not have, its
/// default-monospace shortcut needs an exact weight, and its fallback ranking
/// then prefers any monospace face with a nearer weight — so on Windows every
/// regular-only primary drew bold cells in Consolas Bold (ticket 38, step 0).
/// Grid only: chrome labels and previews keep [`match_cjk_attrs`].
pub(super) fn match_grid_attrs<'a>(fs: &FontSystem, attrs: Attrs<'a>) -> Attrs<'a> {
    if matches!(attrs.family, Family::Monospace) {
        return offered_attrs(&fs.primary_faces().matches, attrs);
    }
    match_cjk_attrs(fs, attrs)
}
/// The proportional owner, shared by chrome labels and every preview run.
/// Coordinator ruling, 2026-09-20: the proportional chain remains YaHei UI
/// first; the grid's historic NSimSun preference is a different surface.
pub(super) fn proportional_cjk_family<'a>(
    text: &str,
    catalogue: &'a CjkCatalog,
) -> Option<Family<'a>> {
    // Preserve the proportional fallback's coverage order, including kana or
    // Hangul drawn by a Chinese-declared face. Grid language ownership must not
    // change a single regular proportional glyph (coordinator, 2026-09-20).
    if !text.chars().any(|c| {
        matches!(
            c.script(),
            unicode_script::Script::Han
                | unicode_script::Script::Hiragana
                | unicode_script::Script::Katakana
                | unicode_script::Script::Hangul
        )
    }) {
        return None;
    }
    catalogue
        .proportional
        .iter()
        .find(|face| face.covers(text))
        .map(|face| Family::Name(face.name.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_italic_request_on_an_upright_only_family_stays_italic() {
        let face = CjkFace {
            name: "Upright Only".into(),
            coverage: bt_unicode::font_coverage::CjkCoverage::default(),
            points: vec![0x4f60],
            matches: vec![
                (Weight::NORMAL, Style::Italic, Weight::NORMAL, Style::Normal),
                (Weight::BOLD, Style::Italic, Weight::NORMAL, Style::Normal),
            ],
        };
        let asked = Attrs::new().weight(Weight::BOLD).style(Style::Italic);
        let given = face.attrs(asked);
        assert_eq!(given.weight, Weight::NORMAL, "the face's own weight");
        assert_eq!(given.style, Style::Italic, "the slant is still owed");
    }
    fn declared(name: &str, bits: u32, points: Vec<u32>) -> Arc<CjkFace> {
        Arc::new(CjkFace {
            name: name.into(),
            coverage: bt_unicode::font_coverage::CjkCoverage::from_code_pages(bits),
            points,
            matches: Vec::new(),
        })
    }
    #[test]
    fn cjk_automatic_simplified_owner_precedes_japanese_only_declaration() {
        let faces = vec![
            declared("Japanese", 1 << 17, vec![0x4f60]),
            declared("Simplified", 1 << 18, vec![0x4f60, 0x8fd9]),
        ];
        let auto = resolve_cjk_chain("", ["Japanese", "Simplified"].into_iter(), &faces, true);
        assert_eq!(auto.han, "Simplified");
        let chosen = resolve_cjk_chain(
            "Japanese",
            ["Japanese", "Simplified"].into_iter(),
            &faces,
            true,
        );
        assert_eq!(
            terminal_grid_family("你", &chosen),
            Family::Name("Japanese")
        );
        assert_eq!(
            terminal_grid_family("这", &chosen),
            Family::Name("Simplified")
        );
        assert_eq!(chosen.chosen, "Japanese");
    }
    #[test]
    fn cjk_proportional_kana_keeps_the_existing_first_covering_face() {
        let faces = vec![
            declared("SC", 1 << 18, vec![0x3042, 0xd55c]),
            declared("JP", 1 << 17, vec![0x3042]),
            declared("KR", 1 << 19, vec![0xd55c]),
        ];
        let proportional = faces.clone();
        let catalogue = CjkCatalog {
            faces,
            proportional,
        };
        assert_eq!(
            proportional_cjk_family("あ", &catalogue),
            Some(Family::Name("SC"))
        );
        assert_eq!(
            proportional_cjk_family("한", &catalogue),
            Some(Family::Name("SC"))
        );
    }
    #[test]
    fn cjk_grid_and_proportional_chains_have_separate_first_installed_owners() {
        let grid = grid_cjk_fallback_families();
        let chrome = platform_cjk_fallback_families();
        let names = grid
            .iter()
            .chain(chrome)
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let faces = names
            .into_iter()
            .map(|name| declared(name, 1 << 18, vec![0x4f60]))
            .collect::<Vec<_>>();
        let grid_owner = resolve_cjk_chain("", grid.iter().copied(), &faces, true);
        let chrome_owner = CjkCatalog {
            faces: faces.clone(),
            proportional: chrome
                .iter()
                .filter_map(|name| faces.iter().find(|face| face.name == *name).cloned())
                .collect(),
        };
        assert_eq!(
            terminal_grid_family("你", &grid_owner),
            Family::Name(grid[0])
        );
        assert_eq!(
            proportional_cjk_family("你", &chrome_owner),
            Some(Family::Name(chrome[0]))
        );
        #[cfg(target_os = "windows")]
        {
            assert_eq!(grid_owner.han, "NSimSun");
            assert_eq!(
                proportional_cjk_family("你", &chrome_owner),
                Some(Family::Name("Microsoft YaHei UI"))
            );
        }
    }
}
