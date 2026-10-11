//! Linux Fontconfig aliases used when rendering standalone SVG text.

use std::collections::BTreeSet;
use std::path::Path;

use fontconfig_parser::{Alias, FontConfig};
#[cfg(test)]
use resvg::usvg::fontdb;
use resvg::usvg::fontdb::Database;

/// Load system fonts for standalone SVG text and map CSS generic families to
/// the first configured family that is present in the loaded database.
pub(super) fn load_svg_fonts(database: &mut Database) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
