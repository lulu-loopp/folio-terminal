//! Installed Linux font families and their file-backed faces.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use fontconfig_parser::{Alias, FontConfig};
use fontdb::{Database, FaceInfo, Language, Source};

use crate::{CjkCoverage, CjkFamily, MonospaceFamily};

struct FamilyFaces {
    canonical_name: String,
    names: Vec<(String, Language)>,
    files: Vec<PathBuf>,
    monospace_files: Vec<PathBuf>,
    coverage: CjkCoverage,
}

/// Load system fonts for standalone SVG text and map CSS generic families to
/// the first configured family that is present in the loaded database.
pub fn load_svg_fonts(database: &mut Database) {
    database.load_system_fonts();
    use_installed_fontconfig_aliases(database, &fontconfig_config().aliases);
}

fn fontconfig_config() -> FontConfig {
    let mut config = FontConfig::default();
    let home = std::env::var("HOME");

    if let Ok(config_file) = std::env::var("FONTCONFIG_FILE") {
        let _ = config.merge_config(Path::new(&config_file));
        return config;
    }

    let xdg_config_home = if let Ok(path) = std::env::var("XDG_CONFIG_HOME") {
        Some(path.into())
    } else if let Ok(home) = &home {
        Some(Path::new(home).join(".config"))
    } else {
        None
    };
    let read_global = match xdg_config_home {
        Some(path) => config
            .merge_config(&path.join("fontconfig/fonts.conf"))
            .is_err(),
        None => true,
    };
    if read_global {
        let _ = config.merge_config(Path::new("/etc/fonts/local.conf"));
    }
    let _ = config.merge_config(Path::new("/etc/fonts/fonts.conf"));
    config
}

fn use_installed_fontconfig_aliases(database: &mut Database, aliases: &[Alias]) {
    let mut assigned = BTreeSet::new();
    for alias in aliases {
        let Some(generic) = generic_family(&alias.alias) else {
            continue;
        };
        if assigned.contains(&generic) {
            continue;
        }
        let family = alias
            .prefer
            .iter()
            .chain(&alias.accept)
            .chain(&alias.default)
            .find_map(|candidate| installed_family(database, candidate));
        let Some(family) = family else {
            continue;
        };
        match generic {
            GenericFamily::Serif => database.set_serif_family(family),
            GenericFamily::SansSerif => database.set_sans_serif_family(family),
            GenericFamily::Monospace => database.set_monospace_family(family),
            GenericFamily::Cursive => database.set_cursive_family(family),
            GenericFamily::Fantasy => database.set_fantasy_family(family),
        }
        assigned.insert(generic);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum GenericFamily {
    Serif,
    SansSerif,
    Monospace,
    Cursive,
    Fantasy,
}

fn generic_family(alias: &str) -> Option<GenericFamily> {
    match alias.to_ascii_lowercase().as_str() {
        "serif" => Some(GenericFamily::Serif),
        "sans-serif" | "sans serif" => Some(GenericFamily::SansSerif),
        "monospace" => Some(GenericFamily::Monospace),
        "cursive" => Some(GenericFamily::Cursive),
        "fantasy" => Some(GenericFamily::Fantasy),
        _ => None,
    }
}

fn installed_family(database: &Database, wanted: &str) -> Option<String> {
    database.faces().find_map(|face| {
        face.families
            .iter()
            .find(|(family, _)| family.eq_ignore_ascii_case(wanted))
            .map(|(family, _)| family.clone())
    })
}

impl FamilyFaces {
    fn new(canonical_name: &str) -> Self {
        Self {
            canonical_name: canonical_name.to_owned(),
            names: Vec::new(),
            files: Vec::new(),
            monospace_files: Vec::new(),
            coverage: CjkCoverage::default(),
        }
    }

    fn add_face(&mut self, face: &FaceInfo, path: PathBuf, coverage: CjkCoverage) {
        for (name, language) in &face.families {
            let exists = self
                .names
                .iter()
                .any(|(saved, saved_language)| saved == name && saved_language == language);
            if !exists {
                self.names.push((name.clone(), *language));
            }
        }
        if !self.files.contains(&path) {
            self.files.push(path.clone());
        }
        if face.monospaced && !self.monospace_files.contains(&path) {
            self.monospace_files.push(path);
        }
        self.coverage = merge_coverage(self.coverage, coverage);
    }

    fn matches_name(&self, name: &str) -> bool {
        self.names
            .iter()
            .any(|(family_name, _)| family_name.eq_ignore_ascii_case(name))
    }

    fn visible_name(&self, locale: &str) -> String {
        self.names
            .iter()
            .find(|(_, language)| {
                language_tag(*language).is_some_and(|tag| tag.eq_ignore_ascii_case(locale))
            })
            .or_else(|| {
                self.names.iter().find(|(_, language)| {
                    language_tag(*language).is_some_and(|tag| locale_matches(tag, locale))
                })
            })
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| self.canonical_name.clone())
    }

    fn localized_names(&self) -> Vec<(String, String)> {
        let mut names: Vec<_> = self
            .names
            .iter()
            .filter_map(|(name, language)| {
                Some((language_tag(*language)?.to_owned(), name.clone()))
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

fn monospace_family_entry(family: &FamilyFaces, locale: &str) -> Option<MonospaceFamily> {
    if family.monospace_files.is_empty() {
        return None;
    }
    Some(MonospaceFamily {
        name: family.visible_name(locale),
        files: family.monospace_files.clone(),
    })
}

/// The installed collection is read for each request so fonts added after a
/// previous Settings scan can appear on the next one.
fn system_fonts() -> Database {
    let mut database = Database::new();
    database.load_system_fonts();
    database
}

pub(super) fn monospace_font_families() -> Vec<MonospaceFamily> {
    let database = system_fonts();
    let locale = crate::os_ui_language();
    crate::order_monospace_families(
        collect_families(&database, false)
            .into_values()
            .filter_map(|family| monospace_family_entry(&family, &locale))
            .collect(),
    )
}

pub(super) fn monospace_family_named(
    token: crate::admission::WaitToken<'_, crate::admission::doors::FontFamilyLookup>,
    name: &str,
) -> Option<MonospaceFamily> {
    let _ = token;
    let database = system_fonts();
    let locale = crate::os_ui_language();
    collect_families(&database, false)
        .into_values()
        .find(|family| family.matches_name(name))
        .and_then(|family| monospace_family_entry(&family, &locale))
}

pub(super) fn cjk_font_families() -> Vec<CjkFamily> {
    let database = system_fonts();
    let locale = crate::os_ui_language();
    crate::order_cjk_families(
        collect_families(&database, true)
            .into_values()
            .filter(|family| family.coverage.any() && !family.files.is_empty())
            .map(|family| CjkFamily {
                name: family.visible_name(&locale),
                localized_names: family.localized_names(),
                files: family.files,
                coverage: family.coverage,
            })
            .collect(),
    )
}

fn collect_families(database: &Database, read_coverage: bool) -> BTreeMap<String, FamilyFaces> {
    let mut families = BTreeMap::new();
    for face in database.faces() {
        let Some(path) = source_path(&face.source) else {
            continue;
        };
        let Some(canonical_name) = canonical_name(face) else {
            continue;
        };
        let key = canonical_name.to_lowercase();
        let coverage = if read_coverage {
            face_coverage(database, face.id)
        } else {
            CjkCoverage::default()
        };
        families
            .entry(key)
            .or_insert_with(|| FamilyFaces::new(canonical_name))
            .add_face(face, path, coverage);
    }
    families
}

fn canonical_name(face: &FaceInfo) -> Option<&str> {
    face.families
        .iter()
        .find(|(_, language)| *language == Language::English_UnitedStates)
        .or_else(|| face.families.first())
        .map(|(name, _)| name.as_str())
        .filter(|name| !name.trim().is_empty())
}

fn source_path(source: &Source) -> Option<PathBuf> {
    match source {
        Source::File(path) | Source::SharedFile(path, _) => Some(path.clone()),
        Source::Binary(_) => None,
    }
}

fn face_coverage(database: &Database, id: fontdb::ID) -> CjkCoverage {
    database
        .with_face_data(id, |bytes, index| {
            let face = ttf_parser::Face::parse(bytes, index).ok()?;
            let raw = face.raw_face();
            Some(CjkCoverage::from_tables(
                raw.table(ttf_parser::Tag::from_bytes(b"OS/2")),
                raw.table(ttf_parser::Tag::from_bytes(b"cmap")),
            ))
        })
        .flatten()
        .unwrap_or_default()
}

fn merge_coverage(left: CjkCoverage, right: CjkCoverage) -> CjkCoverage {
    CjkCoverage {
        japanese: left.japanese || right.japanese,
        simplified: left.simplified || right.simplified,
        traditional: left.traditional || right.traditional,
        korean: left.korean || right.korean,
        han: left.han || right.han,
        hiragana: left.hiragana || right.hiragana,
        katakana: left.katakana || right.katakana,
        hangul: left.hangul || right.hangul,
    }
}

fn language_tag(language: Language) -> Option<&'static str> {
    Some(match language {
        Language::English_UnitedStates => "en-US",
        Language::English_Australia => "en-AU",
        Language::English_Canada => "en-CA",
        Language::English_India => "en-IN",
        Language::English_Ireland => "en-IE",
        Language::English_Malaysia => "en-MY",
        Language::English_NewZealand => "en-NZ",
        Language::English_Singapore => "en-SG",
        Language::English_SouthAfrica => "en-ZA",
        Language::English_UnitedKingdom => "en-GB",
        Language::Chinese_HongKongSAR => "zh-HK",
        Language::Chinese_MacaoSAR => "zh-MO",
        Language::Chinese_PeoplesRepublicOfChina => "zh-CN",
        Language::Chinese_Singapore => "zh-SG",
        Language::Chinese_Taiwan => "zh-TW",
        Language::Japanese_Japan => "ja-JP",
        Language::Korean_Korea => "ko-KR",
        _ => return None,
    })
}

fn locale_matches(candidate: &str, requested: &str) -> bool {
    candidate.eq_ignore_ascii_case(requested)
        || candidate
            .split('-')
            .next()
            .zip(requested.split('-').next())
            .is_some_and(|(candidate, requested)| candidate.eq_ignore_ascii_case(requested))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font_family_lookup(name: &str) -> Option<MonospaceFamily> {
        use crate::admission::{Role, admitted, doors, enter_window_thread, loop_running, role};

        if role() != Role::Window {
            assert!(enter_window_thread());
            assert!(loop_running());
        }
        admitted::<doors::FontFamilyLookup, _>(|token| crate::monospace_family_named(token, name))
            .expect("font lookup is admitted on the window thread")
    }

    fn load_family_files(family: &MonospaceFamily) -> Database {
        let mut database = Database::new();
        for path in &family.files {
            database
                .load_font_file(path)
                .unwrap_or_else(|error| panic!("could not load {:?}: {error}", path));
        }
        database
    }

    #[test]
    fn generic_family_keeps_the_first_installed_alias_after_later_aliases() {
        let mut database = Database::new();
        database.load_system_fonts();
        let installed = database
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
            .find(|name| !name.trim().is_empty())
            .expect("the Linux font database contains an installed family");
        let later_installed = database
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
            .find(|name| !name.trim().is_empty() && !name.eq_ignore_ascii_case(&installed))
            .expect("the Linux font database contains another installed family");
        let mut missing = "folio-missing-family-0".to_owned();
        let mut suffix = 0_u64;
        while installed_family(&database, &missing).is_some() {
            suffix += 1;
            missing = format!("folio-missing-family-{suffix}");
        }
        let aliases = [
            Alias {
                alias: "sans-serif".to_owned(),
                prefer: vec![missing.clone(), installed.clone()],
                ..Alias::default()
            },
            Alias {
                alias: "sans-serif".to_owned(),
                prefer: vec![later_installed],
                ..Alias::default()
            },
            Alias {
                alias: "sans-serif".to_owned(),
                prefer: vec![missing],
                ..Alias::default()
            },
        ];

        use_installed_fontconfig_aliases(&mut database, &aliases);

        assert_eq!(database.family_name(&fontdb::Family::SansSerif), installed);
        assert!(
            database
                .query(&fontdb::Query {
                    families: &[fontdb::Family::SansSerif],
                    ..fontdb::Query::default()
                })
                .is_some()
        );
    }

    #[cfg(unix)]
    #[test]
    fn font_paths_preserve_non_utf8_bytes() {
        use std::os::unix::ffi::OsStrExt;

        let path = PathBuf::from(std::ffi::OsStr::from_bytes(b"/fonts/mono-\xff.ttf"));
        assert_eq!(source_path(&Source::File(path.clone())), Some(path));
    }

    #[test]
    fn installed_linux_monospace_families_have_loadable_files_and_name_lookup_agrees() {
        let families = crate::monospace_font_families();
        let family = families
            .iter()
            .find(|family| !family.files.is_empty())
            .expect("the Linux font collection has an installed monospaced face");
        assert!(!family.name.trim().is_empty());
        assert!(family.files.iter().all(|path| path.is_absolute()));
        let loaded = load_family_files(family);
        assert!(
            loaded.faces().any(|face| {
                face.monospaced
                    && face
                        .families
                        .iter()
                        .any(|(name, _)| name.eq_ignore_ascii_case(&family.name))
            }),
            "the returned files load a monospaced face named {}",
            family.name
        );
        let looked_up = font_family_lookup(&family.name)
            .expect("the installed family can be looked up by the name shown in the picker");
        assert_eq!(&looked_up, family);
        assert_eq!(
            font_family_lookup("No Installed Family Has This Name"),
            None
        );
        eprintln!(
            "BT_FONT_TRACE linux_monospace family={:?} files={:?}",
            family.name, family.files
        );
    }

    #[test]
    fn installed_linux_cjk_families_report_coverage_and_load_from_returned_paths() {
        let families = crate::cjk_font_families();
        let family = families
            .iter()
            .find(|family| !family.files.is_empty())
            .expect("the Linux font collection has an installed CJK face");
        assert!(!family.name.trim().is_empty());
        assert!(family.coverage.any());
        assert!(family.files.iter().all(|path| path.is_absolute()));

        let mut loaded = Database::new();
        for path in &family.files {
            loaded
                .load_font_file(path)
                .unwrap_or_else(|error| panic!("could not load {:?}: {error}", path));
        }
        assert!(
            loaded.faces().any(|face| {
                face.families
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case(&family.name))
                    && face_coverage(&loaded, face.id).any()
            }),
            "the returned files load a face with CJK coverage for {}",
            family.name
        );
        eprintln!(
            "BT_FONT_TRACE linux_cjk family={:?} coverage={:?} files={:?}",
            family.name, family.coverage, family.files
        );
    }
}
