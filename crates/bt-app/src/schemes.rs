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
//! # Enumerated once, and re-enumerated on a reason
//!
//! ~~[`catalogue`] is a `OnceLock`~~ — **superseded by §7.1.6c-4c**. The
//! original text said a file dropped into the folder would be found on the next
//! launch, "the same answer the font row gives to a font installed
//! mid-session". That answer stopped being tenable the day this product started
//! *writing* into the folder itself: a `Customise scheme…` that wrote a file the
//! dialog could not list until a restart would be a verb that appears not to
//! have worked.
//!
//! So the store is a revision-keyed [`std::sync::RwLock`] instead, read through
//! an [`Arc`] so a caller holds a whole consistent catalogue rather than a
//! borrow of a list that can be replaced under it. Re-enumeration happens on a
//! **reason** and never on a clock — the folder is customised, or the folder's
//! own watch says a file in it moved — which is the same rule R31 states about
//! repositories, applied to the one directory this window writes schemes into.
//!
//! `SettingsRow::option_label` still hands back `&'static str`, and that is
//! answered where the question is asked: `settings::scheme_labels` keeps one
//! leaked slice per catalogue revision and interns each distinct name once. See
//! its own doc comment for why the interning is bounded.
//!
//! # A file that will not parse
//!
//! It is skipped, and its name and reason are kept in [`Catalogue::rejects`] so
//! the window can say so once, in an Error toast, when it comes up. Skipped and
//! not fatal: one bad file in a folder of good ones must not be able to stop a
//! terminal from starting, and a silent skip would leave the user staring at a
//! list their scheme is not in with nothing to read.

use std::{
    path::PathBuf,
    sync::{Arc, RwLock, atomic::AtomicU64},
};

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
    /// The file this entry was read out of — a bare file name, the same string
    /// [`SchemeReject`] carries.
    ///
    /// Kept because a name and a file are two different identities and the
    /// watcher needs both: the settings file stores a *name*, while a rescan
    /// that fails reports a *file*. Without this field there is no way to say
    /// "the scheme you are wearing is the one that just stopped parsing", and
    /// the window would have to choose between reporting every bad file in the
    /// folder or none.
    pub file: String,
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
                        file,
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

    /// Whether any row of either canvas is listed under this name.
    ///
    /// Canvas-blind on purpose, unlike every other lookup here: this is the
    /// question [`unique_custom`] asks, and a copy that took a name already used
    /// by the *other* canvas would be a folder holding two files claiming one
    /// name — legal for the picker, which filters by canvas, and a trap for the
    /// person reading the folder.
    #[must_use]
    pub fn holds_name(&self, name: &str) -> bool {
        self.entries.iter().any(|entry| entry.name == name)
    }

    /// The file the scheme listed under `name` on this canvas was read out of.
    #[must_use]
    pub fn file_of(&self, name: &str, light: bool) -> Option<&str> {
        self.entries
            .iter()
            .find(|entry| entry.light == light && entry.name == name)
            .map(|entry| entry.file.as_str())
    }

    /// The **user's own** file behind a scheme on this canvas, when there is
    /// one (§7.1.6c-4d).
    ///
    /// [`Self::file_of`] with the one extra clause `Delete scheme` turns on: a
    /// bundled scheme travels inside the executable and has no file in the
    /// folder to send anywhere, so it answers `None` and the verb is dark. Asked
    /// as a question rather than by handing out the entry list, which is the
    /// rule stated on [`Self::entries`] — every reader of this catalogue names
    /// what it wants to know.
    #[must_use]
    pub fn user_file_of(&self, name: &str, light: bool) -> Option<&str> {
        self.entries
            .iter()
            .find(|entry| {
                entry.light == light && entry.name == name && entry.origin == SchemeOrigin::User
            })
            .map(|entry| entry.file.as_str())
    }

    /// The name a **file** is currently listed under, on the canvas it paints.
    ///
    /// The inverse of [`Self::file_of`], and it exists for the one question that
    /// cannot be asked the other way round: a user who renames their scheme
    /// edits the `name` inside the file, and `settings.json` stores the *name* —
    /// so the row's stored string stops resolving while the file it came from is
    /// still sitting in the folder, listed under something else. Asked of the
    /// tracked file, this says what that something else is, and the rename can be
    /// followed instead of reported as a disappearance (user report 2026-08-18).
    ///
    /// Canvas-checked, because a file whose background was edited across the
    /// light/dark line is not a rename — it is the same file painting the other
    /// canvas, and the row that was wearing it is genuinely without a scheme.
    #[must_use]
    pub fn name_of_file(&self, file: &str, light: bool) -> Option<&str> {
        self.entries
            .iter()
            .find(|entry| entry.light == light && entry.file == file)
            .map(|entry| entry.name.as_str())
    }

    /// The user's own scheme files, as the `Delete scheme…` menu lists them.
    ///
    /// **Files and not schemes**: what the verb does is send a file to the
    /// Recycle Bin, so what it offers is the folder's contents. Bundled entries
    /// travel inside the executable and are never listed — there is nothing of
    /// them on disk to delete — which is the same clause [`Self::user_file_of`]
    /// carries, asked over the whole catalogue rather than about one name.
    ///
    /// Both canvases in one list, deliberately. A reader who made a light custom
    /// must be able to delete it while a dark canvas is in force, and having to
    /// select a scheme before its file can be removed is exactly the dance this
    /// menu exists to end (user report 2026-08-18).
    #[must_use]
    pub fn user_files(&self) -> Vec<(&str, &str)> {
        self.entries
            .iter()
            .filter(|entry| entry.origin == SchemeOrigin::User)
            .map(|entry| (entry.name.as_str(), entry.file.as_str()))
            .collect()
    }

    /// The reason a named file was skipped, if it was.
    #[must_use]
    pub fn reject_reason(&self, file: &str) -> Option<&str> {
        self.rejects
            .iter()
            .find(|reject| reject.file == file)
            .map(|reject| reject.reason.as_str())
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

/// The store behind [`catalogue`] and [`rescan`], and the revision that names
/// what is in it.
///
/// The revision is a separate atomic rather than a field of [`Catalogue`],
/// because the readers who need it — `settings::scheme_labels`, which keeps a
/// leaked slice per revision — need to ask "is what I cached still current"
/// without taking the lock or cloning the `Arc`.
static CATALOGUE: RwLock<Option<Arc<Catalogue>>> = RwLock::new(None);
static REVISION: AtomicU64 = AtomicU64::new(0);

/// Which enumeration [`catalogue`] would answer with.
///
/// Starts at 1 and advances on every [`rescan`] that changed the list, so `0`
/// is "nothing has been enumerated yet" and no cache keyed on it can collide
/// with a real answer.
#[must_use]
pub fn revision() -> u64 {
    if REVISION.load(std::sync::atomic::Ordering::Acquire) == 0 {
        drop(catalogue());
    }
    REVISION.load(std::sync::atomic::Ordering::Acquire)
}

/// Every scheme this process can offer.
///
/// Enumerated on the first ask — at startup, because the stored pair has to be
/// in force before the first grid is measured — and re-enumerated only by
/// [`rescan`]. See the module header for why this stopped being a `OnceLock`.
#[must_use]
pub fn catalogue() -> Arc<Catalogue> {
    if let Some(held) = CATALOGUE
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
    {
        return held;
    }
    rescan()
}

/// Read the folder again and put the result in force.
///
/// Always re-reads: the caller only ever asks because something happened —
/// this window wrote a file, or the folder's watch fired — and a rescan that
/// second-guessed its caller would be a cache with an opinion. The revision
/// advances only when the answer actually differs, so a save that changed a
/// comment costs no relabelling downstream.
pub fn rescan() -> Arc<Catalogue> {
    let built = Arc::new(Catalogue::build(bundled_sources().chain(user_sources())));
    let mut held = CATALOGUE
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let changed = held
        .as_ref()
        .is_none_or(|previous| previous.entries != built.entries);
    if changed {
        REVISION.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }
    *held = Some(Arc::clone(&built));
    built
}

/// The folder a user's own schemes live in, made if it is not there.
///
/// Made rather than merely named, because the one caller that asks for it by
/// this door is about to write into it — the enumeration reads through
/// [`user_sources`], which deliberately does *not* create anything so that a
/// user who never customises a scheme never gets an empty directory in their
/// `%APPDATA%` advertising a feature they did not ask for.
pub fn user_dir() -> std::io::Result<PathBuf> {
    let directory = crate::persist::storage_dir().join(USER_SCHEME_DIR);
    std::fs::create_dir_all(&directory)?;
    Ok(directory)
}

/// What the copy of `base` is called, at `attempt`.
///
/// `attempt` 1 is `Nord (custom)`, and every later one numbers itself —
/// `Nord (custom 2)`. The first has no number because the overwhelmingly common
/// case is one copy, and `Nord (custom 1)` beside no `Nord (custom 2)` reads as
/// a program that expects you to make more.
///
/// **A copy of a copy numbers itself from the original's name**: customising
/// `Nord (custom)` again yields `Nord (custom 2)`, not `Nord (custom) (custom)`.
/// The suffix is this function's own mark, so it is stripped before it is
/// re-applied — a name that grew a `(custom)` per press would be a program
/// counting the reader's presses out loud.
#[must_use]
pub fn custom_name(base: &str, attempt: u32) -> String {
    let base = custom_stem(base);
    if attempt <= 1 {
        format!("{base} (custom)")
    } else {
        format!("{base} (custom {attempt})")
    }
}

/// `base` without a trailing ` (custom)` / ` (custom N)` this module put there.
fn custom_stem(base: &str) -> &str {
    let Some(open) = base.rfind(" (custom") else {
        return base;
    };
    let tail = &base[open + " (custom".len()..];
    let numbered = tail
        .strip_prefix(' ')
        .and_then(|rest| rest.strip_suffix(')'))
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()));
    if tail == ")" || numbered {
        &base[..open]
    } else {
        base
    }
}

/// The file name a scheme called `name` is written under.
///
/// Windows forbids nine characters in a file name and forbids a trailing dot or
/// space; a scheme's `name` is a free string out of somebody's JSON and may hold
/// any of them. Each forbidden character becomes `-` — a replacement and not a
/// deletion, so two names that differ only in punctuation do not collide into
/// one file — and the trailing run is trimmed. The name inside the file is
/// **not** touched by any of this: the file name is a place to keep it and the
/// `name` key is its identity.
#[must_use]
pub fn file_name_for(name: &str) -> String {
    let mut cleaned: String = name
        .chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) || character.is_control()
            {
                '-'
            } else {
                character
            }
        })
        .collect();
    while cleaned.ends_with(['.', ' ']) {
        cleaned.pop();
    }
    format!("{cleaned}.json")
}

/// The first `(name, file)` pair for a copy of `base` that neither the catalogue
/// nor the folder already holds.
///
/// **Both are checked, at the same attempt**, and that is the whole of this
/// function. A name that is free while its file is taken would write over
/// somebody's file; a file that is free while its name is taken would produce a
/// second row the picker cannot tell from the first — `Catalogue::build` folds
/// two entries of one name into one, so the copy would silently replace what it
/// was copied from.
///
/// `taken` is handed in rather than read here so the rule can be tested without
/// a filesystem: it answers "is this name or this file already spoken for".
pub fn unique_custom(base: &str, taken: impl Fn(&str, &str) -> bool) -> (String, String) {
    for attempt in 1..=u32::MAX {
        let name = custom_name(base, attempt);
        let file = file_name_for(&name);
        if !taken(&name, &file) {
            return (name, file);
        }
    }
    unreachable!("a u32's worth of copies of one scheme cannot all be taken")
}

/// Write a copy of `scheme` into the user's folder, under a name and a file
/// nothing else holds, and put the enlarged catalogue in force.
///
/// Returns the path written and the name the copy is listed under — the two
/// things the caller needs and the two it must not compute a second time: the
/// path is what a preview pane is opened on, and the name is what goes into
/// `settings.json`, and a caller that re-derived either would be re-running
/// [`unique_custom`] against a folder that now contains the answer.
///
/// **The copy is written, not linked.** What is put on disk is every one of the
/// twenty-two keys resolved — see `bt_persist::write_scheme` — so the file the
/// user opens holds the colours they are looking at, including the ones the
/// scheme it was copied from left to a fallback.
pub fn write_custom_copy(
    base: &str,
    scheme: &ColourScheme,
) -> std::io::Result<(PathBuf, String, Arc<Catalogue>)> {
    let directory = user_dir()?;
    let held = catalogue();
    let (name, file) = unique_custom(base, |name, file| {
        held.holds_name(name) || directory.join(file).exists()
    });
    let text = bt_persist::write_scheme(&bt_persist::SchemeFileV1 {
        name: name.clone(),
        background: scheme.background,
        foreground: scheme.foreground,
        cursor: scheme.cursor,
        selection: scheme.selection,
        ansi: scheme.ansi,
        accent: scheme.accent,
    });
    let path = directory.join(&file);
    std::fs::write(&path, text)?;
    Ok((path, name, rescan()))
}

/// What a rescan of the folder decided about the two schemes in force.
///
/// **Pure, and separate from everything that raises a card or repaints a
/// window**, for [`crate::watch_clock::WatchClock`]'s reason: every interesting
/// claim about this mechanism is a claim about *which* of three things happened
/// to a named scheme, and a version of it living inside the runtime would only
/// be checkable by writing files and watching a window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RescanVerdict {
    /// What to put in force, `(light, dark)`.
    pub schemes: (ColourScheme, ColourScheme),
    /// The file behind a scheme in force that will not parse, if there is one.
    /// The caller holds it so the next rescan can tell *still broken* from
    /// *broken again*.
    pub fault: Option<String>,
    /// `(file, reason)` when that fault has not been said yet.
    pub report: Option<(String, String)>,
    /// `(name, fallback)` for each canvas whose file has left the folder.
    pub gone: Vec<(String, String)>,
    /// `(index, new name)` for each canvas whose tracked file is still in the
    /// folder under a **different** name — the reader renamed their scheme
    /// (user report 2026-08-18).
    ///
    /// The index is into the caller's `[light, dark]` pair rather than a
    /// canvas flag, because the caller's job with this is to write the new name
    /// into the settings row of that index — and both rows can be pointing at
    /// one file, in which case both are here and both follow.
    ///
    /// A rename is neither of the other two outcomes and must not be reported as
    /// either. `gone` would fall the row to the default and say the scheme had
    /// vanished, over a file the reader is looking at; keeping the old name would
    /// leave the row unresolvable until they picked it again by hand, which is
    /// the bug reported.
    pub renamed: Vec<(usize, String)>,
}

/// Read the three outcomes out of a fresh catalogue.
///
/// * **Still there** — resolve it, which is how a scheme edited in place gets
///   its new colours.
/// * **Its file will not parse** — keep what is in force. Falling back to the
///   default here would be the worst of both: the window would change colour
///   *because* of a typo, and the error would be read against a palette that is
///   not the one being edited.
/// * **Its file is gone** — fall to the default, which is [`Catalogue::resolve`]'s
///   standing answer, and say so. The stored name is untouched, so putting the
///   file back puts the scheme back.
///
/// A scheme nobody selected does all three and moves nothing, and that falls out
/// of the arithmetic rather than being checked: what comes back is derived from
/// the two stored names alone.
///
/// `source` is what each row resolved to **before** this rescan — the name it
/// held and the file that name came from. It is the caller's memory because the
/// catalogue cannot be it: once a file stops parsing, the entry that connected
/// the name to the file is exactly what is missing.
#[must_use]
pub fn rescan_verdict(
    after: &Catalogue,
    stored: [&str; 2],
    source: [Option<(String, String)>; 2],
    in_force: (ColourScheme, ColourScheme),
    held_fault: Option<&str>,
) -> RescanVerdict {
    let mut schemes = (
        after.resolve(stored[0], true),
        after.resolve(stored[1], false),
    );
    let mut fault: Option<(String, String)> = None;
    let mut gone = Vec::new();
    let mut renamed = Vec::new();
    for (index, light) in [true, false].into_iter().enumerate() {
        let name = stored[index];
        if after.file_of(name, light).is_some() {
            continue;
        }
        // It resolved before and does not now, under the same name: this row's
        // own file is what moved.
        let Some((_, file)) = source[index].as_ref().filter(|(held, _)| held == name) else {
            continue;
        };
        match after.reject_reason(file) {
            Some(reason) => {
                if light {
                    schemes.0 = in_force.0;
                } else {
                    schemes.1 = in_force.1;
                }
                fault.get_or_insert((file.clone(), reason.to_owned()));
            }
            // **The file is still there, under another name.** The reader edited
            // the `name` field, which is where a scheme's name lives, and the
            // settings row stores the name — so the row and the file have come
            // apart while both are perfectly intact. Followed rather than
            // reported: the file is the thing that was tracked, this is what it
            // is called now, and the caller writes that into the row.
            //
            // Resolved through the *new* name, so the colours in force are the
            // file's own and not a default the row is about to stop naming.
            None if after.name_of_file(file, light).is_some() => {
                let now = after
                    .name_of_file(file, light)
                    .expect("just matched Some")
                    .to_owned();
                if light {
                    schemes.0 = after.resolve(&now, true);
                } else {
                    schemes.1 = after.resolve(&now, false);
                }
                renamed.push((index, now));
            }
            None => gone.push((name.to_owned(), after.default_name(light).to_owned())),
        }
    }
    let report = fault
        .clone()
        .filter(|(file, _)| held_fault != Some(file.as_str()));
    RescanVerdict {
        schemes,
        fault: fault.map(|(file, _)| file),
        report,
        gone,
        renamed,
    }
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

    /// PIN (§7.1.6c-4c) — **what this window writes is what it ships**: a copy
    /// of Folio's own dark scheme is `folio-dark.json`, byte for byte, but for
    /// the name.
    ///
    /// This is the canonical-key-order pin and it is stated as an equality with
    /// a real file rather than as a list of keys, because the list of keys is
    /// what a reader would compare a written file against by eye. A writer that
    /// sorted its keys, dropped a fallback it could have omitted, spelled its
    /// hex in capitals or indented four spaces fails here.
    #[test]
    fn a_written_scheme_is_the_bundled_file_it_was_copied_from() {
        let catalogue = Catalogue::build(bundled_sources());
        let dark = catalogue.resolve(FOLIO_DARK_NAME, false);
        let written = bt_persist::write_scheme(&bt_persist::SchemeFileV1 {
            name: FOLIO_DARK_NAME.to_owned(),
            background: dark.background,
            foreground: dark.foreground,
            cursor: dark.cursor,
            selection: dark.selection,
            ansi: dark.ansi,
            accent: dark.accent,
        });
        assert_eq!(
            written,
            include_str!("../../../assets/schemes/folio-dark.json")
        );
    }

    /// PIN — a copy takes the first name **and** file nothing holds, and both are
    /// checked at the same attempt.
    ///
    /// The two halves matter separately: a free name over a taken file writes
    /// over somebody's file, and a free file under a taken name produces a
    /// second entry the picker folds into the first — so the copy would silently
    /// replace what it was copied from.
    #[test]
    fn a_copy_takes_the_first_name_and_file_that_are_both_free() {
        assert_eq!(
            unique_custom("Nord", |_, _| false),
            ("Nord (custom)".to_owned(), "Nord (custom).json".to_owned())
        );
        // The name is taken; the file is not.
        assert_eq!(
            unique_custom("Nord", |name, _| name == "Nord (custom)"),
            (
                "Nord (custom 2)".to_owned(),
                "Nord (custom 2).json".to_owned()
            )
        );
        // The file is taken; the name is not.
        assert_eq!(
            unique_custom("Nord", |_, file| file == "Nord (custom).json"),
            (
                "Nord (custom 2)".to_owned(),
                "Nord (custom 2).json".to_owned()
            )
        );
        // A folder somebody has been busy in.
        let taken = ["Nord (custom)", "Nord (custom 2)", "Nord (custom 3)"];
        assert_eq!(
            unique_custom("Nord", |name, _| taken.contains(&name)).0,
            "Nord (custom 4)"
        );
        // A copy of a copy is numbered from the original's name: the suffix is
        // this module's own mark and is not stacked.
        assert_eq!(
            unique_custom("Nord (custom)", |_, _| false).0,
            "Nord (custom)"
        );
        assert_eq!(
            unique_custom("Nord (custom 3)", |name, _| name == "Nord (custom)").0,
            "Nord (custom 2)"
        );
        // A name that merely resembles the mark is left whole.
        assert_eq!(
            unique_custom("Nord (customary)", |_, _| false).0,
            "Nord (customary) (custom)"
        );
        assert_eq!(
            unique_custom("Nord (custom x)", |_, _| false).0,
            "Nord (custom x) (custom)"
        );
    }

    /// A scheme whose name is not a legal file name still gets a file, and the
    /// name inside the file is untouched.
    #[test]
    fn a_name_the_filesystem_refuses_is_spelled_into_one_it_accepts() {
        assert_eq!(file_name_for("Nord"), "Nord.json");
        assert_eq!(file_name_for("a/b:c*d?e"), "a-b-c-d-e.json");
        assert_eq!(file_name_for("trailing dots..."), "trailing dots.json");
        assert_eq!(file_name_for("trailing space   "), "trailing space.json");
        // A replacement rather than a deletion, so two names that differ only in
        // punctuation do not collide into one file.
        assert_ne!(file_name_for("a:b"), file_name_for("ab"));
    }

    /// PIN — **the selected scheme changed, so it is applied; an unselected one
    /// changed, so nothing moves.**
    #[test]
    fn a_rescan_moves_the_selected_scheme_and_only_the_selected_one() {
        let retuned =
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
        let in_force = (bt_render::FOLIO_LIGHT, bt_render::FOLIO_DARK);

        let chosen = rescan_verdict(
            &retuned,
            ["", "Nord"],
            [None, Some(("Nord".to_owned(), "nord.json".to_owned()))],
            in_force,
            None,
        );
        assert_eq!(chosen.schemes.1.background, [0x11, 0x18, 0x27]);
        assert_eq!(chosen.report, None);
        assert!(chosen.gone.is_empty());

        // The same folder, with nobody wearing Nord.
        let ignored = rescan_verdict(
            &retuned,
            ["Folio Light", "Folio Dark"],
            [
                Some(("Folio Light".to_owned(), "folio-light.json".to_owned())),
                Some(("Folio Dark".to_owned(), "folio-dark.json".to_owned())),
            ],
            in_force,
            None,
        );
        assert_eq!(ignored.schemes, in_force);
        assert_eq!(ignored.report, None);
    }

    /// PIN — **the file behind the scheme in force stopped parsing: the colours
    /// stay, and it is said once.**
    #[test]
    fn a_broken_file_keeps_the_palette_and_is_reported_once_until_it_is_fixed() {
        let broken = Catalogue::build(bundled_sources().chain([SchemeSource {
            file: "mine.json".to_owned(),
            origin: SchemeOrigin::User,
            text: Ok(r##"{"name":"Mine","background":"#nope"}"##.to_owned()),
        }]));
        // Whatever is on screen right now, which is deliberately not either of
        // the constants: this is the value that has to come back out.
        let worn = ColourScheme {
            background: [0x11, 0x22, 0x33],
            ..bt_render::FOLIO_DARK
        };
        let in_force = (bt_render::FOLIO_LIGHT, worn);
        let source = [None, Some(("Mine".to_owned(), "mine.json".to_owned()))];

        let first = rescan_verdict(&broken, ["", "Mine"], source.clone(), in_force, None);
        assert_eq!(first.schemes.1, worn, "the palette in force does not move");
        assert_eq!(first.fault.as_deref(), Some("mine.json"));
        let (file, reason) = first.report.clone().expect("a new fault is said");
        assert_eq!(file, "mine.json");
        assert!(
            reason.contains("background"),
            "and it names the key: {reason}"
        );
        assert!(first.gone.is_empty(), "a broken file has not gone anywhere");

        // Saved broken again, with the same fault held: nothing more is said.
        let again = rescan_verdict(
            &broken,
            ["", "Mine"],
            source.clone(),
            in_force,
            first.fault.as_deref(),
        );
        assert_eq!(again.fault.as_deref(), Some("mine.json"));
        assert_eq!(again.report, None, "one card, not one per save");

        // Fixed. The fault clears, and the new colours go in.
        let fixed = Catalogue::build(bundled_sources().chain([SchemeSource {
            file: "mine.json".to_owned(),
            origin: SchemeOrigin::User,
            text: Ok(include_str!("../../../assets/schemes/nord.json").replace("Nord", "Mine")),
        }]));
        let good = rescan_verdict(
            &fixed,
            ["", "Mine"],
            source.clone(),
            in_force,
            again.fault.as_deref(),
        );
        assert_eq!(good.schemes.1.background, [0x2e, 0x34, 0x40]);
        assert_eq!(good.fault, None);
        assert_eq!(good.report, None);

        // And broken a third time, with nothing held: it is said again.
        let relapse = rescan_verdict(&broken, ["", "Mine"], source, in_force, None);
        assert!(
            relapse.report.is_some(),
            "a fault that was good in between is news again"
        );
    }

    /// PIN (user report 2026-08-18) — **a scheme renamed inside its own file is
    /// followed, not lost.**
    ///
    /// A scheme's name lives in the file's `name` field and `settings.json`
    /// stores that name, so editing it parts the row from the file it came from:
    /// the stored name stops resolving while the file sits in the folder under a
    /// new one. The standing answer was "gone" — fall to the default, raise a
    /// card — over a change the reader had just made on purpose, and the only
    /// way back was to open the picker and choose their own scheme again.
    ///
    /// The watcher already tracks each row's *file*, which is the fact a rename
    /// leaves alone, so the new name is read off it and handed back for the
    /// caller to write into the row. Both rows follow when both were on one
    /// file, and a row that was on something else is untouched.
    ///
    /// MUTATIONS:
    /// (1) drop the `name_of_file` arm — `renamed` is empty and `gone` says the
    ///     reader's scheme disappeared;
    /// (2) resolve through the *stored* name instead of the new one — the
    ///     colours fall to the default while the row's name follows, which is
    ///     the worst of the two.
    #[test]
    fn a_scheme_renamed_in_its_own_file_is_followed_by_the_row_that_wore_it() {
        // A scheme of the reader's own — a name no bundled entry holds, which
        // is the ordinary case and the one this is about. (A rename *onto* a
        // bundled name is its own pin below, because the format has a rule for
        // that and the rule is what decides.)
        let renamed = Catalogue::build(
            bundled_sources().chain([SchemeSource {
                file: "mine.json".to_owned(),
                origin: SchemeOrigin::User,
                text: Ok(include_str!("../../../assets/schemes/nord.json")
                    .replace("\"name\": \"Nord\"", "\"name\": \"Midnight\"")
                    .replace("\"background\": \"#2e3440\"", "\"background\": \"#111827\"")),
            }]),
        );
        let in_force = (bt_render::FOLIO_LIGHT, bt_render::FOLIO_DARK);

        let verdict = rescan_verdict(
            &renamed,
            ["", "Mine"],
            [None, Some(("Mine".to_owned(), "mine.json".to_owned()))],
            in_force,
            None,
        );
        assert_eq!(
            verdict.renamed,
            [(1, "Midnight".to_owned())],
            "the dark row follows its own file to the name it now carries"
        );
        assert!(
            verdict.gone.is_empty(),
            "and nothing disappeared: the file is right there"
        );
        assert_eq!(verdict.report, None);
        assert_eq!(
            verdict.schemes.1.background,
            [0x11, 0x18, 0x27],
            "the colours in force are the file's own and not the default the \
             row is about to stop naming"
        );

        // Both rows on one file, both following. (One file can only paint one
        // canvas, so this is the case where a reader has pointed the light row
        // at a dark scheme's name — legal, and `resolve` gives the light row the
        // light default while the name still follows.)
        let both = rescan_verdict(
            &renamed,
            ["Mine", "Mine"],
            [
                Some(("Mine".to_owned(), "mine.json".to_owned())),
                Some(("Mine".to_owned(), "mine.json".to_owned())),
            ],
            in_force,
            None,
        );
        assert_eq!(
            both.renamed,
            [(1, "Midnight".to_owned())],
            "and only the canvas the file actually paints follows it: the other \
             row was never resolving through this file"
        );

        // A row wearing something else is not touched by somebody else's rename.
        let elsewhere = rescan_verdict(
            &renamed,
            ["", "Folio Dark"],
            [
                None,
                Some(("Folio Dark".to_owned(), "folio-dark.json".to_owned())),
            ],
            in_force,
            None,
        );
        assert!(elsewhere.renamed.is_empty());
        assert!(elsewhere.gone.is_empty());
    }

    /// PIN (user report 2026-08-18) — **a rename onto a name the catalogue
    /// already holds is a replacement, and the row follows it there.**
    ///
    /// The honest outcome rather than a refusal, and it is the file format's own
    /// rule showing through: `Catalogue::build` lets a user file whose `name` is
    /// a bundled scheme's **replace** it, because overriding a bundled scheme by
    /// name is the only way to retune one without forking the build. So renaming
    /// a scheme to `Nord` does exactly what putting a file called `Nord` in the
    /// folder has always done — the reader's file is what `Nord` means now — and
    /// the row that was wearing the old name is pointed at `Nord`.
    ///
    /// Nothing is lost either way: the bundled scheme is still in the executable
    /// and comes back the moment the name is freed.
    #[test]
    fn a_rename_onto_a_name_the_catalogue_holds_replaces_it_and_the_row_follows() {
        // The reader's file, saved with `"name": "Nord"` — a name the
        // executable already carries.
        let collided =
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
        assert_eq!(
            collided.file_of("Nord", false),
            Some("mine.json"),
            "the reader's file is what the name means now — the format's own rule"
        );

        let verdict = rescan_verdict(
            &collided,
            ["", "Mine"],
            [None, Some(("Mine".to_owned(), "mine.json".to_owned()))],
            (bt_render::FOLIO_LIGHT, bt_render::FOLIO_DARK),
            None,
        );
        assert_eq!(
            verdict.renamed,
            [(1, "Nord".to_owned())],
            "the row follows the file to the name it took over"
        );
        assert_eq!(
            verdict.schemes.1.background,
            [0x11, 0x18, 0x27],
            "and wears the file's colours, which is what the reader edited"
        );
        assert!(verdict.gone.is_empty());
    }

    /// PIN (user report 2026-08-18) — **the menu lists the reader's own files
    /// and never a bundled one.**
    #[test]
    fn the_user_files_list_is_the_folder_and_only_the_folder() {
        let folder = Catalogue::build(
            bundled_sources().chain([
                SchemeSource {
                    file: "mine.json".to_owned(),
                    origin: SchemeOrigin::User,
                    text: Ok(include_str!("../../../assets/schemes/nord.json")
                        .replace("\"name\": \"Nord\"", "\"name\": \"Midnight\"")),
                },
                SchemeSource {
                    file: "day.json".to_owned(),
                    origin: SchemeOrigin::User,
                    text: Ok(include_str!("../../../assets/schemes/folio-light.json")
                        .replace("\"name\": \"Folio Light\"", "\"name\": \"Day\"")),
                },
            ]),
        );
        assert_eq!(
            folder.user_files(),
            [("Midnight", "mine.json"), ("Day", "day.json")],
            "both canvases in one list: a light scheme must be deletable while \
             a dark one is in force, which is the dance the menu exists to end"
        );
        assert!(
            Catalogue::build(bundled_sources()).user_files().is_empty(),
            "a folder with nothing in it offers nothing, whatever the \
             executable carries"
        );
    }

    /// PIN — **the file left the folder: the row falls to the default and says
    /// so, once.**
    ///
    /// Once falls out of the state rather than being counted: the second rescan
    /// has no source to compare against, because the first one is what cleared
    /// it.
    #[test]
    fn a_file_that_left_the_folder_falls_to_the_default_and_says_so_once() {
        let without = Catalogue::build(bundled_sources());
        let in_force = (bt_render::FOLIO_LIGHT, bt_render::FOLIO_DARK);
        let first = rescan_verdict(
            &without,
            ["", "Mine"],
            [None, Some(("Mine".to_owned(), "mine.json".to_owned()))],
            in_force,
            None,
        );
        assert_eq!(first.schemes.1, bt_render::FOLIO_DARK);
        assert_eq!(first.gone, [("Mine".to_owned(), "Folio Dark".to_owned())]);
        assert_eq!(first.report, None, "a missing file is not a broken one");

        let second = rescan_verdict(&without, ["", "Mine"], [None, None], in_force, None);
        assert!(second.gone.is_empty(), "said once");
        assert_eq!(second.schemes.1, bt_render::FOLIO_DARK);
    }

    /// PIN (user ruling 2026-08-18) — **a deletion this window made raises one
    /// card, not two.**
    ///
    /// `Delete scheme` recycles the file, falls the row back to the default and
    /// raises an `Ok` card naming the file and where it went. The folder's
    /// watcher then sees the same deletion, and its standing answer to "the
    /// selected scheme's file is gone" is a fall to the default **and an Info
    /// card** — right for a file that left behind this window's back, wrong for
    /// one this window just deleted, because the press already said so.
    ///
    /// The suppression is arithmetic and not a flag: the verdict is derived from
    /// the two **stored names**, the press has already made this one the empty
    /// default, and a default resolves to a bundled scheme that cannot go
    /// missing. There is nothing to silence because there is nothing to report.
    ///
    /// Red gate: leave the stored name alone in `delete_scheme_in_force` — the
    /// "one card" half of it — and the first assertion here goes red with the
    /// second card the reader would have seen.
    #[test]
    fn a_deletion_this_window_made_raises_one_card_not_two() {
        let after = Catalogue::build(bundled_sources());
        let in_force = (bt_render::FOLIO_LIGHT, bt_render::FOLIO_DARK);

        // What the watcher reaches after the press: the file is gone from the
        // folder *and* the row has already fallen back to the default.
        let watched = rescan_verdict(&after, ["", ""], [None, None], in_force, None);
        assert!(
            watched.gone.is_empty(),
            "the press said it; the watcher must not say it again"
        );
        assert_eq!(watched.report, None);
        assert_eq!(watched.schemes.0, bt_render::FOLIO_LIGHT);
        assert_eq!(watched.schemes.1, bt_render::FOLIO_DARK);

        // And the rule it is not allowed to break: the *other* row's file
        // leaving the folder behind this window's back is still one card.
        let elsewhere = rescan_verdict(
            &after,
            ["Mine", ""],
            [Some(("Mine".to_owned(), "mine.json".to_owned())), None],
            in_force,
            None,
        );
        assert_eq!(
            elsewhere.gone,
            [("Mine".to_owned(), "Folio Light".to_owned())],
            "a file that left on its own is still news"
        );
    }

    /// A name nobody ever held is not news: a `settings.json` edited by hand can
    /// name a scheme that has never existed, and every rescan must not announce
    /// its absence.
    #[test]
    fn a_name_that_never_resolved_is_never_reported_as_gone() {
        let catalogue = Catalogue::build(bundled_sources());
        let verdict = rescan_verdict(
            &catalogue,
            ["", "A Scheme Nobody Wrote"],
            [None, None],
            (bt_render::FOLIO_LIGHT, bt_render::FOLIO_DARK),
            None,
        );
        assert!(verdict.gone.is_empty());
        assert_eq!(verdict.report, None);
        assert_eq!(verdict.schemes.1, bt_render::FOLIO_DARK);
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
