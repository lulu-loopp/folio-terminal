//! The colour schemes this build can offer, bundled and user-written
//! (§7.1.6c-4a).
//!
//! # One parser, two sources
//!
//! Ten schemes travel inside the executable as `include_str!`ed JSON, and every
//! `*.json` under `%APPDATA%\Folio\schemes\` joins them. **Both go through
//! `bt_persist::parse_scheme`**, which is the whole reason the bundled ones are
//! files rather than Rust literals: a format the product's own schemes did not
//! have to use is a format nobody tests. A bundled scheme that stopped parsing
//! would fail
//! `every_bundled_scheme_parses_and_keeps_the_name_its_row_is_listed_under`
//! before it ever reached a user.
//!
//! # Enumerated once
//!
//! [`catalogue`] is a `OnceLock`, for the reason `settings::monospace_families`
//! gives about this machine's fonts: `SettingsRow::option_label` hands back
//! `&'static str`, and a list that can change under it cannot supply one
//! without a leak per call. The first ask is at startup — the stored pair has
//! to be in force before the first grid is measured — so a file dropped into
//! the folder afterwards is found on the next launch. That is the same answer
//! the font row gives to a font installed mid-session, and it is stated here
//! rather than left to be discovered.
//!
//! # A file that will not parse
//!
//! It is skipped, and its name and reason are kept in [`Catalogue::rejects`] so
//! the window can say so once, in an Error toast, when it comes up. Skipped and
//! not fatal: one bad file in a folder of good ones must not be able to stop a
//! terminal from starting, and a silent skip would leave the user staring at a
//! list their scheme is not in with nothing to read.

use bt_render::ColourScheme;

/// Where a scheme came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemeOrigin {
    /// Shipped inside the executable.
    Bundled,
    /// Read out of `%APPDATA%\Folio\schemes\`.
    User,
}

/// One scheme, under the name its row is listed by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemeEntry {
    /// The `name` its file declares — the string `settings.json` stores, and
    /// the word the picker draws.
    pub name: String,
    /// Whether its canvas is a light one, which is the row it belongs to. See
    /// [`Catalogue::names_for`].
    pub light: bool,
    pub origin: SchemeOrigin,
    pub scheme: ColourScheme,
}

/// A file under `%APPDATA%\Folio\schemes\` that could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemeReject {
    /// The file's own name, not its full path: the toast names something the
    /// user can find in the folder they just opened.
    pub file: String,
    /// `SchemeParseError`'s sentence, which names the offending key.
    pub reason: String,
}

/// Every scheme this process can put in force, and every file it could not read.
#[derive(Clone, Debug, Default)]
pub struct Catalogue {
    entries: Vec<SchemeEntry>,
    rejects: Vec<SchemeReject>,
}

/// The ten that travel inside the executable, in the order their rows are drawn.
///
/// Folio's own two first because they are the defaults and a picker whose first
/// item is somebody else's scheme reads as a product that borrowed its own look;
/// the other eight alphabetically, which is how a reader hunting for a name
/// scans a list. Each pair's two halves are separate files rather than one file
/// with two canvases, because that is what Windows Terminal's format is and the
/// format is the point.
const BUNDLED: [(&str, &str); 10] = [
    (
        "folio-dark.json",
        include_str!("../../../assets/schemes/folio-dark.json"),
    ),
    (
        "folio-light.json",
        include_str!("../../../assets/schemes/folio-light.json"),
    ),
    (
        "dracula.json",
        include_str!("../../../assets/schemes/dracula.json"),
    ),
    (
        "gruvbox-dark.json",
        include_str!("../../../assets/schemes/gruvbox-dark.json"),
    ),
    (
        "gruvbox-light.json",
        include_str!("../../../assets/schemes/gruvbox-light.json"),
    ),
    (
        "nord.json",
        include_str!("../../../assets/schemes/nord.json"),
    ),
    (
        "one-half-dark.json",
        include_str!("../../../assets/schemes/one-half-dark.json"),
    ),
    (
        "one-half-light.json",
        include_str!("../../../assets/schemes/one-half-light.json"),
    ),
    (
        "solarized-dark.json",
        include_str!("../../../assets/schemes/solarized-dark.json"),
    ),
    (
        "solarized-light.json",
        include_str!("../../../assets/schemes/solarized-light.json"),
    ),
];

/// The folder a user's own schemes go in, beside `settings.json`.
pub const USER_SCHEME_DIR: &str = "schemes";

impl Catalogue {
    /// Bundled first in [`BUNDLED`]'s order, then whatever the folder held, in
    /// the order the filesystem listed it sorted by name.
    ///
    /// A user file whose `name` is one a bundled scheme already uses **replaces**
    /// it rather than appearing twice. Two rows reading `Nord` would be a picker
    /// that cannot say which one is ticked, and the file the user wrote is the
    /// one they meant — overriding a bundled scheme by name is the only way to
    /// retune one without forking the build.
    fn build(sources: impl IntoIterator<Item = SchemeSource>) -> Self {
        let mut entries: Vec<SchemeEntry> = Vec::new();
        let mut rejects = Vec::new();
        for SchemeSource { file, origin, text } in sources {
            let text = match text {
                Ok(text) => text,
                Err(reason) => {
                    rejects.push(SchemeReject { file, reason });
                    continue;
                }
            };
            match bt_persist::parse_scheme(&text) {
                Ok(parsed) => {
                    let entry = SchemeEntry {
                        light: bt_render::background_is_light(parsed.background),
                        name: parsed.name,
                        origin,
                        scheme: ColourScheme {
                            background: parsed.background,
                            foreground: parsed.foreground,
                            cursor: parsed.cursor,
                            selection: parsed.selection,
                            ansi: parsed.ansi,
                            accent: parsed.accent,
                        },
                    };
                    match entries.iter_mut().find(|held| held.name == entry.name) {
                        Some(held) => *held = entry,
                        None => entries.push(entry),
                    }
                }
                Err(reason) => rejects.push(SchemeReject {
                    file,
                    reason: reason.to_string(),
                }),
            }
        }
        Self { entries, rejects }
    }

    /// Every scheme, in picker order.
    ///
    /// Test-only: the product reaches a scheme through [`Self::resolve`] or
    /// [`Self::names_for`], both of which answer a question, and a whole-list
    /// accessor the window never calls would be a fourth way to read the
    /// catalogue for the tests' convenience alone.
    #[cfg(test)]
    #[must_use]
    pub fn entries(&self) -> &[SchemeEntry] {
        &self.entries
    }

    /// The files that would not parse, oldest listing order.
    #[must_use]
    pub fn rejects(&self) -> &[SchemeReject] {
        &self.rejects
    }

    /// The schemes one of the two rows may offer.
    ///
    /// **Each row lists only the schemes whose canvas is its own**, and that is
    /// a correctness rule rather than a tidiness one. The chrome picks its
    /// palette by the luma of the background actually painted
    /// (`bt_render`'s `scheme_for_background`), so a dark scheme sitting in the
    /// Light row would paint a dark canvas and then be handed the *dark*
    /// scheme's chrome to wear — a window whose terminal came from one file and
    /// whose tab strip came from another. Filtering the list is what makes that
    /// state unreachable instead of merely unlikely.
    pub fn names_for(&self, light: bool) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter(move |entry| entry.light == light)
            .map(|entry| entry.name.as_str())
    }

    /// The colours a stored name resolves to on this build, and the default's
    /// when it resolves to nothing.
    ///
    /// A name the catalogue does not hold — a file deleted since it was chosen,
    /// or one written by a build that bundled more — falls to the default, and
    /// the row shows the default: [`Self::default_name`] is what
    /// `settings::scheme_index` ticks, so the picker's mark and the window's
    /// colours are one resolution rather than two that agree most of the time.
    /// The stored string is left alone by that fall, so moving a file out of the
    /// folder and back does not consume the choice.
    #[must_use]
    pub fn resolve(&self, name: &str, light: bool) -> ColourScheme {
        self.entries
            .iter()
            .find(|entry| entry.light == light && entry.name == name)
            .or_else(|| self.default_entry(light))
            .map_or_else(|| Self::last_resort(light), |entry| entry.scheme)
    }

    /// The name an unnamed setting shows in its picker.
    #[must_use]
    pub fn default_name(&self, light: bool) -> &str {
        self.default_entry(light)
            .map_or(Self::last_resort_name(light), |entry| entry.name.as_str())
    }

    /// Folio's own scheme for a canvas, or — if a user file has taken its name
    /// for the other canvas — whatever else that canvas has.
    ///
    /// **Both halves of the search are canvas-first**, because a user file may
    /// legitimately be called `Folio Dark` and hold a light scheme, and
    /// answering the dark row with it would put a light canvas under the dark
    /// row's chrome — the exact state [`Self::names_for`] exists to make
    /// unreachable.
    fn default_entry(&self, light: bool) -> Option<&SchemeEntry> {
        let wanted = Self::last_resort_name(light);
        self.entries
            .iter()
            .find(|entry| entry.light == light && entry.name == wanted)
            .or_else(|| self.entries.iter().find(|entry| entry.light == light))
    }

    /// What a canvas with no scheme at all wears.
    ///
    /// Unreachable through the bundled ten, and reachable only if a user renamed
    /// every scheme of one canvas out of existence — but reachable is reachable,
    /// and a terminal that panics because of what is in a folder is not a
    /// terminal. The constants are the very ones the derivation pin runs
    /// against, so the floor is the product's own look and not a stand-in.
    const fn last_resort(light: bool) -> ColourScheme {
        if light {
            bt_render::FOLIO_LIGHT
        } else {
            bt_render::FOLIO_DARK
        }
    }

    const fn last_resort_name(light: bool) -> &'static str {
        if light {
            FOLIO_LIGHT_NAME
        } else {
            FOLIO_DARK_NAME
        }
    }
}

/// The name Folio's own light scheme is listed under.
pub const FOLIO_LIGHT_NAME: &str = "Folio Light";
/// And its dark half.
pub const FOLIO_DARK_NAME: &str = "Folio Dark";

/// Every scheme this process can offer, read once — see the module header.
pub fn catalogue() -> &'static Catalogue {
    static CATALOGUE: std::sync::OnceLock<Catalogue> = std::sync::OnceLock::new();
    CATALOGUE.get_or_init(|| Catalogue::build(bundled_sources().chain(user_sources())))
}

/// One candidate file: its name, where it came from, and either its text or the
/// reason there is none.
///
/// The unreadable case travels beside the unparseable one rather than being
/// dropped, because to the reader they are one thing — the scheme they put in
/// the folder is not in the list — and a folder whose permissions are wrong is
/// exactly the case a silent skip would leave unexplainable.
struct SchemeSource {
    file: String,
    origin: SchemeOrigin,
    text: Result<String, String>,
}

fn bundled_sources() -> impl Iterator<Item = SchemeSource> {
    BUNDLED.into_iter().map(|(file, text)| SchemeSource {
        file: file.to_owned(),
        origin: SchemeOrigin::Bundled,
        text: Ok(text.to_owned()),
    })
}

/// Every `*.json` in `%APPDATA%\Folio\schemes\`, by name.
///
/// Sorted, because `read_dir` promises no order and a picker whose rows move
/// between launches is a picker nobody can build muscle memory in. A folder
/// that does not exist is not an error: it is the ordinary case, and creating it
/// here would put an empty directory in every user's `%APPDATA%` to advertise a
/// feature they have not asked for.
fn user_sources() -> impl Iterator<Item = SchemeSource> {
    let directory = crate::persist::storage_dir().join(USER_SCHEME_DIR);
    let Ok(listing) = std::fs::read_dir(&directory) else {
        return Vec::new().into_iter();
    };
    let mut found: Vec<SchemeSource> = listing
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        })
        .map(|entry| SchemeSource {
            file: entry.file_name().to_string_lossy().into_owned(),
            origin: SchemeOrigin::User,
            text: std::fs::read_to_string(entry.path())
                .map_err(|error| format!("the file could not be read: {error}")),
        })
        .collect();
    found.sort_by(|left, right| left.file.cmp(&right.file));
    found.into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — every scheme this build ships parses through the very reader a
    /// user's file goes through, and is listed under the name in its file.
    #[test]
    fn every_bundled_scheme_parses_and_keeps_the_name_its_row_is_listed_under() {
        let catalogue = Catalogue::build(bundled_sources());
        assert!(
            catalogue.rejects().is_empty(),
            "a bundled scheme did not parse: {:?}",
            catalogue.rejects()
        );
        let names: Vec<&str> = catalogue
            .entries()
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Folio Dark",
                "Folio Light",
                "Dracula",
                "Gruvbox Dark",
                "Gruvbox Light",
                "Nord",
                "One Half Dark",
                "One Half Light",
                "Solarized Dark",
                "Solarized Light",
            ]
        );
    }

    /// PIN — the two files a user can copy are the two constants the derivation
    /// pin runs against, to the byte.
    ///
    /// They exist twice on purpose: `bt_render`'s consts are what
    /// `ChromePalette::derive` is held to, and cannot be JSON because that crate
    /// parses none; the files are what a user opens to make their own. This is
    /// the test that stops the two from drifting.
    #[test]
    fn folios_own_two_schemes_are_the_constants_the_derivation_is_pinned_to() {
        let catalogue = Catalogue::build(bundled_sources());
        assert_eq!(
            catalogue.resolve(FOLIO_DARK_NAME, false),
            bt_render::FOLIO_DARK
        );
        assert_eq!(
            catalogue.resolve(FOLIO_LIGHT_NAME, true),
            bt_render::FOLIO_LIGHT
        );
    }

    /// Each row offers only its own canvas — see [`Catalogue::names_for`].
    #[test]
    fn a_row_offers_only_the_schemes_its_canvas_can_wear() {
        let catalogue = Catalogue::build(bundled_sources());
        let light: Vec<&str> = catalogue.names_for(true).collect();
        let dark: Vec<&str> = catalogue.names_for(false).collect();
        assert_eq!(
            light,
            [
                "Folio Light",
                "Gruvbox Light",
                "One Half Light",
                "Solarized Light"
            ]
        );
        assert_eq!(
            dark,
            [
                "Folio Dark",
                "Dracula",
                "Gruvbox Dark",
                "Nord",
                "One Half Dark",
                "Solarized Dark"
            ]
        );
    }

    /// A name nobody holds falls to the default, and the fall does not depend on
    /// which row asked.
    #[test]
    fn a_scheme_that_is_gone_falls_to_folios_own() {
        let catalogue = Catalogue::build(bundled_sources());
        assert_eq!(catalogue.default_name(true), FOLIO_LIGHT_NAME);
        assert_eq!(catalogue.default_name(false), FOLIO_DARK_NAME);
        assert_eq!(catalogue.resolve("", true), bt_render::FOLIO_LIGHT);
        assert_eq!(catalogue.resolve("", false), bt_render::FOLIO_DARK);
        assert_eq!(
            catalogue.resolve("A Scheme Nobody Wrote", false),
            bt_render::FOLIO_DARK
        );
        // A light name asked for by the dark row is as gone as one that does not
        // exist: the rows do not share a namespace at the point of use.
        assert_eq!(
            catalogue.resolve("Solarized Light", false),
            bt_render::FOLIO_DARK
        );
    }

    /// A folder that renamed every scheme of one canvas away still starts a
    /// terminal — the floor is Folio's own constants, not a panic.
    #[test]
    fn a_canvas_with_nothing_left_falls_all_the_way_to_the_constants() {
        let empty = Catalogue::default();
        assert_eq!(empty.resolve("Nord", false), bt_render::FOLIO_DARK);
        assert_eq!(empty.resolve("", true), bt_render::FOLIO_LIGHT);
        assert_eq!(empty.default_name(false), FOLIO_DARK_NAME);
        assert_eq!(empty.names_for(true).count(), 0);
    }

    /// A user file may be called `Folio Dark` and hold a light scheme; the dark
    /// row must not answer with it.
    #[test]
    fn the_default_is_looked_up_canvas_first_and_not_name_first() {
        let catalogue = Catalogue::build([SchemeSource {
            file: "mine.json".to_owned(),
            origin: SchemeOrigin::User,
            text: Ok(include_str!("../../../assets/schemes/folio-light.json")
                .replace("\"Folio Light\"", "\"Folio Dark\"")),
        }]);
        assert_eq!(catalogue.default_name(true), "Folio Dark");
        assert_eq!(
            catalogue.resolve("Folio Dark", true).background,
            [0xff, 0xff, 0xff]
        );
        // Nothing dark is left, so the dark row falls to the constant rather
        // than to a light scheme wearing the dark row's name.
        assert_eq!(
            catalogue.resolve("Folio Dark", false),
            bt_render::FOLIO_DARK
        );
    }

    /// A file that will not parse is skipped, keeps the good ones, and says
    /// which file and why.
    #[test]
    fn a_malformed_file_is_skipped_and_named() {
        let catalogue = Catalogue::build([
            SchemeSource {
                file: "folio-dark.json".to_owned(),
                origin: SchemeOrigin::Bundled,
                text: Ok(include_str!("../../../assets/schemes/folio-dark.json").to_owned()),
            },
            SchemeSource {
                file: "broken.json".to_owned(),
                origin: SchemeOrigin::User,
                text: Ok(r##"{"name":"Broken","background":"#not-a-colour"}"##.to_owned()),
            },
            SchemeSource {
                file: "half.json".to_owned(),
                origin: SchemeOrigin::User,
                text: Ok(r##"{"name":"Half","background":"#101010"}"##.to_owned()),
            },
            SchemeSource {
                file: "gone.json".to_owned(),
                origin: SchemeOrigin::User,
                text: Err("the file could not be read: access is denied".to_owned()),
            },
        ]);
        assert_eq!(catalogue.entries().len(), 1);
        assert_eq!(catalogue.entries()[0].name, "Folio Dark");
        let rejects = catalogue.rejects();
        assert_eq!(rejects.len(), 3);
        assert_eq!(rejects[0].file, "broken.json");
        assert!(
            rejects[0].reason.contains("background"),
            "the reason has to name the key: {}",
            rejects[0].reason
        );
        assert_eq!(rejects[1].file, "half.json");
        assert!(
            rejects[1].reason.contains("foreground"),
            "a missing key is named too: {}",
            rejects[1].reason
        );
        assert_eq!(rejects[2].file, "gone.json");
        assert!(rejects[2].reason.contains("could not be read"));
    }

    /// A user file that takes a bundled scheme's name replaces it, in place.
    #[test]
    fn a_user_file_may_retune_a_bundled_scheme_without_doubling_its_row() {
        let catalogue =
            Catalogue::build(bundled_sources().chain([
                SchemeSource {
                    file: "mine.json".to_owned(),
                    origin: SchemeOrigin::User,
                    text:
                        Ok(
                            include_str!("../../../assets/schemes/nord.json").replace(
                                "\"background\": \"#2e3440\"",
                                "\"background\": \"#111827\"",
                            ),
                        ),
                },
            ]));
        let nord: Vec<&SchemeEntry> = catalogue
            .entries()
            .iter()
            .filter(|entry| entry.name == "Nord")
            .collect();
        assert_eq!(nord.len(), 1, "one row, not two");
        assert_eq!(nord[0].origin, SchemeOrigin::User);
        assert_eq!(nord[0].scheme.background, [0x11, 0x18, 0x27]);
    }

    /// The chrome a bundled scheme derives is that scheme's, all the way to the
    /// tab strip — the property the whole slice exists for.
    #[test]
    fn a_bundled_scheme_paints_the_chrome_as_well_as_the_grid() {
        let catalogue = Catalogue::build(bundled_sources());
        let solarized = catalogue.resolve("Solarized Dark", false);
        let palette = bt_render::ChromePalette::derive(&solarized);
        assert_eq!(palette.seat_body, [0x00, 0x2b, 0x36]);
        assert_eq!(palette.accent, solarized.accent);
        assert_ne!(palette.title_bar, bt_render::DARK_CHROME.title_bar);
    }
}
