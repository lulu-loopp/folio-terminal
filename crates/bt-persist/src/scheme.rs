//! Terminal colour schemes, in **Windows Terminal's own file format**.
//!
//! The format is not this product's invention and deliberately so. Every scheme
//! anyone has ever wanted already exists as a Windows Terminal `schemes[]` entry
//! — `mbadolato/iTerm2-Color-Schemes` alone converts several hundred of them,
//! and windowsterminalthemes.dev hands one out as a JSON object per copy button.
//! A format of our own would mean every one of those had to be re-typed by hand,
//! so this module reads theirs: a scheme downloaded from either place pastes in
//! unchanged, with no conversion step and nothing for the user to rename.
//!
//! Folio adds exactly one key, `accent`, and adds it as an *optional* one, so
//! that adding it is never required to make a foreign file work. Everything
//! unrecognised is ignored, because real files carry keys this product has no
//! use for — `cursorTextColor`, `selectionForeground`, and, when someone copies
//! a whole profile instead of a scheme, a good deal of Windows Terminal besides.
//!
//! What is deliberately **not** here is anything that touches the filesystem.
//! This module turns one string into one scheme; where schemes live, which of
//! them exist, and what happens when two claim the same name are `bt-app`'s
//! questions, and keeping them out of here is what lets this be tested with
//! nothing but a `&str`.

use serde_json::{Map, Value};

/// One colour scheme as it is written on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemeFileV1 {
    pub name: String,
    pub background: [u8; 3],
    pub foreground: [u8; 3],
    pub cursor: [u8; 3],
    pub selection: [u8; 3],
    /// ANSI 0..=15, normal then bright: black red green yellow blue purple cyan
    /// white, then the eight bright ones.
    pub ansi: [[u8; 3]; 16],
    /// Folio's one addition to the format.
    pub accent: [u8; 3],
}

/// Why a scheme file could not be read.
///
/// Every variant names the offending key, and the malformed-value one echoes
/// the value back. That is a requirement rather than a nicety: the caller
/// surfaces this in a toast beside a file the user just dropped into a folder,
/// and "this scheme is invalid" tells them to open all nineteen colours and
/// compare them by eye, while "`brightBlue` is `#12345`, which is not a
/// `#rrggbb` colour" tells them where to put the cursor.
#[derive(Debug, thiserror::Error)]
pub enum SchemeParseError {
    #[error("the file is not JSON: {source}")]
    NotJson {
        #[source]
        source: serde_json::Error,
    },
    #[error("a scheme must be a JSON object, and this file's top level is {found}")]
    NotAnObject { found: &'static str },
    #[error("the required key `{key}` is missing")]
    MissingKey { key: &'static str },
    #[error("`{key}` must be a string")]
    NotAString { key: &'static str },
    #[error("`{key}` is `{value}`, which is not a `#rrggbb` colour")]
    BadHex { key: &'static str, value: String },
    #[error("`name` is empty, and a scheme is chosen by name")]
    EmptyName,
}

/// The sixteen ANSI keys, in the order the palette is indexed by.
///
/// The names are Windows Terminal's, including `purple`/`brightPurple` where
/// the ANSI standard says magenta. Spelling them the standard's way would mean
/// every downloaded file failed on two keys, which is the whole reason this
/// module speaks somebody else's format rather than a tidier one.
const ANSI_KEYS: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "purple",
    "cyan",
    "white",
    "brightBlack",
    "brightRed",
    "brightGreen",
    "brightYellow",
    "brightBlue",
    "brightPurple",
    "brightCyan",
    "brightWhite",
];

/// Index of `blue` in [`ANSI_KEYS`] — the accent a scheme that does not name one
/// falls back to.
const ANSI_BLUE: usize = 4;

/// How heavily a selection is laid over the surface beneath it, in per mille.
///
/// 300 because that is the weight this product's own chrome already marks a
/// selection at; a scheme that omits `selectionBackground` gets the house rule
/// rather than a second one invented here.
const SELECTION_ALPHA_PER_MILLE: i32 = 300;

/// Parses one Windows Terminal scheme object.
///
/// The input is a single JSON object, not the `{"schemes": [...]}` array a
/// `settings.json` wraps them in: splitting a downloaded settings file into its
/// schemes is a caller's job, and one object in is what both of the places
/// people actually copy from hand out.
pub fn parse_scheme(json: &str) -> Result<SchemeFileV1, SchemeParseError> {
    let value: Value =
        serde_json::from_str(json).map_err(|source| SchemeParseError::NotJson { source })?;
    let object = match value {
        Value::Object(object) => object,
        other => {
            return Err(SchemeParseError::NotAnObject {
                found: json_shape(&other),
            });
        }
    };

    let name = required_string(&object, "name")?.to_owned();
    if name.is_empty() {
        return Err(SchemeParseError::EmptyName);
    }

    let background = required_colour(&object, "background")?;
    let foreground = required_colour(&object, "foreground")?;

    let mut ansi = [[0u8; 3]; 16];
    for (slot, key) in ansi.iter_mut().zip(ANSI_KEYS) {
        *slot = required_colour(&object, key)?;
    }

    let accent = optional_colour(&object, "accent")?.unwrap_or(ansi[ANSI_BLUE]);
    let cursor = optional_colour(&object, "cursorColor")?.unwrap_or(foreground);
    let selection = optional_colour(&object, "selectionBackground")?
        .unwrap_or_else(|| ink_over(background, accent, SELECTION_ALPHA_PER_MILLE));

    Ok(SchemeFileV1 {
        name,
        background,
        foreground,
        cursor,
        selection,
        ansi,
        accent,
    })
}

/// Every key a scheme Folio writes carries, in the order it writes them.
///
/// The order is the one the ten bundled files are written in, and it is the
/// reading order of the format rather than an alphabetical one: the scheme's
/// identity, then the surface, then the ink, then the two marks drawn on that
/// surface, then Folio's own key, then the ANSI sixteen in palette order. A
/// file this product hands a user to edit has to look like the ten they can
/// compare it against, and `serde_json`'s map order is alphabetical — which
/// would put `accent` above `background` and scatter the sixteen from
/// `brightBlack` to `yellow`, a file nobody could diff against a bundled one.
///
/// It is spelled out here rather than derived from [`SchemeFileV1`]'s field
/// order because the two differ on purpose: the struct holds `ansi` as one
/// array, and the file spells all sixteen of its members.
const WRITE_KEYS: [&str; 6] = [
    "name",
    "background",
    "foreground",
    "cursorColor",
    "selectionBackground",
    "accent",
];

/// One scheme as a file: the same JSON [`parse_scheme`] reads, pretty-printed
/// two spaces deep, keys in [`WRITE_KEYS`] then [`ANSI_KEYS`] order.
///
/// **Every optional key is written out.** `cursorColor`, `selectionBackground`
/// and `accent` all have documented fallbacks, so a writer could leave out the
/// ones that happen to equal theirs and produce a shorter file — and hand the
/// reader a file whose colours change when they edit an unrelated line, because
/// the fallback they were silently riding on moved. What is written is what is
/// in force.
///
/// The name goes through `serde_json` rather than being quoted here, so that a
/// scheme called `He said "hi"` writes a file that parses; every other value is
/// six hex digits and has nothing to escape.
#[must_use]
pub fn write_scheme(scheme: &SchemeFileV1) -> String {
    let values: [String; 6] = [
        Value::from(scheme.name.as_str()).to_string(),
        quoted_hex(scheme.background),
        quoted_hex(scheme.foreground),
        quoted_hex(scheme.cursor),
        quoted_hex(scheme.selection),
        quoted_hex(scheme.accent),
    ];
    let mut out = String::from("{\n");
    let lines = WRITE_KEYS
        .into_iter()
        .zip(values)
        .chain(ANSI_KEYS.into_iter().zip(scheme.ansi.map(quoted_hex)));
    let mut first = true;
    for (key, value) in lines {
        if !first {
            out.push_str(",\n");
        }
        first = false;
        out.push_str("  \"");
        out.push_str(key);
        out.push_str("\": ");
        out.push_str(&value);
    }
    out.push_str("\n}\n");
    out
}

/// `#rrggbb`, lower case, in quotes — the spelling [`parse_hex`] reads and the
/// ten bundled files use.
fn quoted_hex(colour: [u8; 3]) -> String {
    format!("\"#{:02x}{:02x}{:02x}\"", colour[0], colour[1], colour[2])
}

/// `ink` at `alpha_per_mille` over `canvas`, rounding half away from zero.
///
/// **This is `bt_render::theme::ink_over`'s arithmetic, transcribed** — the same
/// `(scaled ± 500) / 1000` in `i32`, with the sign of the difference choosing
/// which way the bias goes. It is transcribed rather than called because this
/// crate does not depend on the renderer and must not start to for one
/// composite; what it must not become is a *second* rounding rule, because a
/// selection mark composed one way here and another way there would differ by a
/// channel on some schemes and by nothing on others, which is the hardest kind
/// of disagreement to ever notice.
fn ink_over(canvas: [u8; 3], ink: [u8; 3], alpha_per_mille: i32) -> [u8; 3] {
    let mut faded = [0u8; 3];
    for (channel, out) in faded.iter_mut().enumerate() {
        let base = i32::from(canvas[channel]);
        let scaled = (i32::from(ink[channel]) - base) * alpha_per_mille;
        let step = if scaled >= 0 {
            (scaled + 500) / 1000
        } else {
            (scaled - 500) / 1000
        };
        *out = (base + step) as u8;
    }
    faded
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    key: &'static str,
) -> Result<&'a str, SchemeParseError> {
    object
        .get(key)
        .ok_or(SchemeParseError::MissingKey { key })?
        .as_str()
        .ok_or(SchemeParseError::NotAString { key })
}

fn required_colour(
    object: &Map<String, Value>,
    key: &'static str,
) -> Result<[u8; 3], SchemeParseError> {
    parse_hex(key, required_string(object, key)?)
}

/// A key that may be absent, but that may not be present and wrong.
///
/// The distinction is the whole of this function. Absent means "the format does
/// not require it and this file did not offer it", which has a documented
/// answer; present-but-malformed means the author wrote something and got it
/// wrong, and swallowing that as a fallback would hand them a scheme that
/// silently ignores a line they can see in their own file.
fn optional_colour(
    object: &Map<String, Value>,
    key: &'static str,
) -> Result<Option<[u8; 3]>, SchemeParseError> {
    match object.get(key) {
        None => Ok(None),
        Some(value) => {
            let text = value.as_str().ok_or(SchemeParseError::NotAString { key })?;
            parse_hex(key, text).map(Some)
        }
    }
}

/// `#rrggbb`, in either case, and nothing else.
///
/// The two forms deliberately rejected are `#rgb` and `#rrggbbaa`. Windows
/// Terminal writes neither, so accepting them would be inventing dialect; and
/// the eight-digit one is the one worth refusing loudly, because a scheme colour
/// has nowhere to put an alpha channel and the only ways to "accept" it are to
/// drop the alpha silently or to composite it against a guess. Both hand back a
/// palette that is not the one in the file, without saying so.
fn parse_hex(key: &'static str, value: &str) -> Result<[u8; 3], SchemeParseError> {
    let malformed = || SchemeParseError::BadHex {
        key,
        value: value.to_owned(),
    };
    let digits = value.strip_prefix('#').ok_or_else(malformed)?.as_bytes();
    if digits.len() != 6 {
        return Err(malformed());
    }
    let mut channels = [0u8; 3];
    for (channel, pair) in channels.iter_mut().zip(digits.chunks_exact(2)) {
        let high = hex_nibble(pair[0]).ok_or_else(malformed)?;
        let low = hex_nibble(pair[1]).ok_or_else(malformed)?;
        *channel = high * 16 + low;
    }
    Ok(channels)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// What the top level turned out to be, for an error a user can act on: "an
/// array" is the message that tells someone they copied the whole `schemes`
/// list instead of one entry out of it.
fn json_shape(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-exact from `mbadolato/iTerm2-Color-Schemes`' Windows Terminal
    /// export. It is checked in verbatim on purpose: the point of this module is
    /// that a file from there works untouched, and a sample tidied up on the way
    /// in would stop testing that.
    const NORD: &str = r##"{"name":"Nord","black":"#3b4252","red":"#bf616a","green":"#a3be8c","yellow":"#ebcb8b","blue":"#81a1c1","purple":"#b48ead","cyan":"#88c0d0","white":"#e5e9f0","brightBlack":"#596377","brightRed":"#bf616a","brightGreen":"#a3be8c","brightYellow":"#ebcb8b","brightBlue":"#81a1c1","brightPurple":"#b48ead","brightCyan":"#8fbcbb","brightWhite":"#eceff4","background":"#2e3440","foreground":"#d8dee9","cursorColor":"#eceff4","selectionBackground":"#eceff4"}"##;

    /// The sample with an edit applied, so that each test below differs from a
    /// real file in exactly the one way it is about.
    fn nord_with(edit: impl FnOnce(&mut Map<String, Value>)) -> String {
        let Value::Object(mut object) = serde_json::from_str::<Value>(NORD).unwrap() else {
            unreachable!("the sample is an object")
        };
        edit(&mut object);
        Value::Object(object).to_string()
    }

    /// PIN — a real Windows Terminal scheme, copied out of the export and not
    /// adapted, parses.
    #[test]
    fn a_real_windows_terminal_scheme_parses_unchanged() {
        let scheme = parse_scheme(NORD).unwrap();
        assert_eq!(scheme.name, "Nord");
        assert_eq!(scheme.background, [0x2e, 0x34, 0x40]);
        assert_eq!(scheme.foreground, [0xd8, 0xde, 0xe9]);
        assert_eq!(scheme.cursor, [0xec, 0xef, 0xf4]);
        assert_eq!(scheme.selection, [0xec, 0xef, 0xf4]);
        assert_eq!(scheme.ansi[0], [0x3b, 0x42, 0x52], "ansi 0 is `black`");
        assert_eq!(
            scheme.ansi[8],
            [0x59, 0x63, 0x77],
            "ansi 8 is `brightBlack`"
        );
        assert_eq!(
            scheme.ansi[15],
            [0xec, 0xef, 0xf4],
            "ansi 15 is `brightWhite`"
        );
        assert_eq!(
            scheme.accent,
            [0x81, 0xa1, 0xc1],
            "`accent` is Folio's own key and Nord does not carry it, so it falls \
             back to `blue` — the entry a foreign file is most likely to have \
             chosen as its highlight"
        );
    }

    /// PIN — the sixteen land in ANSI order, not in the order the file wrote
    /// them.
    ///
    /// Worth its own test because the failure is invisible in the one above: a
    /// palette indexed by the file's key order rather than by the standard's
    /// would still round-trip every colour, and would simply draw the wrong one
    /// for every `\x1b[3Nm` a program emitted.
    #[test]
    fn the_sixteen_are_indexed_by_ansi_number_and_not_by_file_order() {
        let scheme = parse_scheme(NORD).unwrap();
        let expected = [
            "#3b4252", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
            "#596377", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#8fbcbb", "#eceff4",
        ];
        for (index, hex) in expected.into_iter().enumerate() {
            assert_eq!(
                scheme.ansi[index],
                parse_hex("test", hex).unwrap(),
                "ansi {index} must be {hex}"
            );
        }
    }

    /// PIN — a file that names an accent gets the one it named, not `blue`.
    #[test]
    fn an_explicit_accent_wins_over_the_blue_fallback() {
        let json = nord_with(|object| {
            object.insert("accent".to_owned(), Value::from("#b48ead"));
        });
        let scheme = parse_scheme(&json).unwrap();
        assert_eq!(scheme.accent, [0xb4, 0x8e, 0xad]);
        assert_ne!(
            scheme.accent, scheme.ansi[4],
            "the fallback must not survive an explicit choice"
        );
    }

    /// PIN — no `cursorColor` means the cursor is the foreground.
    ///
    /// The foreground and not black, and not a colour of this module's choosing:
    /// a block cursor drawn in a colour the scheme never mentions is the one way
    /// to make a downloaded palette look broken on arrival.
    #[test]
    fn a_scheme_without_a_cursor_colour_uses_its_foreground() {
        let json = nord_with(|object| {
            object.remove("cursorColor");
        });
        let scheme = parse_scheme(&json).unwrap();
        assert_eq!(scheme.cursor, [0xd8, 0xde, 0xe9]);
        assert_eq!(scheme.cursor, scheme.foreground);
    }

    /// PIN — no `selectionBackground` means the accent at 30% over the
    /// background, with the answer written out rather than recomputed.
    ///
    /// The literal is the point. Nord's accent falls back to `blue` `#81a1c1`
    /// over background `#2e3440`, so per channel, at 300 per mille, rounding
    /// half away from zero:
    ///
    /// - R: 46 + (129-46)·300 = 46 + 24900 → (24900+500)/1000 = 25 → 71 = `0x47`
    /// - G: 52 + (161-52)·300 = 52 + 32700 → (32700+500)/1000 = 33 → 85 = `0x55`
    /// - B: 64 + (193-64)·300 = 64 + 38700 → (38700+500)/1000 = 39 → 103 = `0x67`
    ///
    /// Asserting `ink_over(...)` here instead would restate the implementation
    /// and pin nothing; `#475567` is a number that has to keep coming out.
    #[test]
    fn a_scheme_without_a_selection_colour_marks_at_thirty_percent_accent() {
        let json = nord_with(|object| {
            object.remove("selectionBackground");
        });
        let scheme = parse_scheme(&json).unwrap();
        assert_eq!(scheme.selection, [0x47, 0x55, 0x67]);
    }

    /// PIN — the composite rounds half away from zero in both directions, which
    /// is `bt_render::theme::ink_over`'s rule and the reason this helper exists
    /// as a transcription rather than as a fresh piece of arithmetic.
    #[test]
    fn the_composite_rounds_half_away_from_zero_going_both_ways() {
        // Up: 0 + (5-0)·300 = 1500 → (1500+500)/1000 = 2, where truncation would
        // give 1.
        assert_eq!(ink_over([0, 0, 0], [5, 5, 5], 300), [2, 2, 2]);
        // Down: 5 + (0-5)·300 = -1500 → (-1500-500)/1000 = -2, where truncation
        // would give -1 and the two directions would disagree.
        assert_eq!(ink_over([5, 5, 5], [0, 0, 0], 300), [3, 3, 3]);
    }

    /// PIN — every malformed hex the format does not use is refused, by name and
    /// with the value echoed.
    ///
    /// `#12345678` is the one that matters most. It is valid CSS and a person may
    /// well paste it in; accepting it would mean silently dropping an alpha the
    /// author wrote down, and handing back a palette that is not the file's.
    #[test]
    fn a_malformed_colour_names_its_key_and_echoes_what_was_written() {
        for bad in ["#zzz", "1234", "#12345", "#12345678", "", "#", "#abc"] {
            let json = nord_with(|object| {
                object.insert("blue".to_owned(), Value::from(bad));
            });
            let error = parse_scheme(&json).unwrap_err();
            let message = error.to_string();
            assert!(
                matches!(error, SchemeParseError::BadHex { key: "blue", ref value } if value == bad),
                "`{bad}` must be refused as a malformed `blue`, got: {message}"
            );
            assert!(
                message.contains("blue") && message.contains(bad),
                "the message must name the key and echo the value: {message}"
            );
        }
    }

    /// PIN — upper-case hex parses, because half the exports in circulation are
    /// written that way and a scheme that failed on `#FFFFFF` would look like a
    /// bug in the file.
    #[test]
    fn upper_case_hex_parses_to_the_same_bytes_as_lower_case() {
        let json = nord_with(|object| {
            object.insert("blue".to_owned(), Value::from("#81A1C1"));
            object.insert("brightWhite".to_owned(), Value::from("#ECEFF4"));
        });
        let scheme = parse_scheme(&json).unwrap();
        assert_eq!(scheme.ansi[4], [0x81, 0xa1, 0xc1]);
        assert_eq!(scheme.ansi[15], [0xec, 0xef, 0xf4]);
    }

    /// PIN — every required key is required, and the error says which one is
    /// gone.
    ///
    /// The loop covers all nineteen rather than a representative one, because
    /// the failure this catches is a key left out of the required list entirely
    /// — and a spot check would find that only if it happened to spot-check the
    /// key that was forgotten.
    #[test]
    fn a_missing_required_key_is_refused_by_name() {
        let required = ["name", "background", "foreground"]
            .into_iter()
            .chain(ANSI_KEYS);
        for key in required {
            let json = nord_with(|object| {
                object.remove(key);
            });
            let error = parse_scheme(&json).unwrap_err();
            assert!(
                matches!(error, SchemeParseError::MissingKey { key: missing } if missing == key),
                "removing `{key}` must be refused by name, got: {error}"
            );
            assert!(error.to_string().contains(key));
        }
    }

    /// PIN — a value of the wrong type is refused by key, including on the
    /// optional keys, where "not a string" must not be quietly treated as
    /// "absent".
    #[test]
    fn a_value_that_is_not_a_string_is_refused_by_name() {
        for key in ["name", "background", "blue", "cursorColor", "accent"] {
            let json = nord_with(|object| {
                object.insert(key.to_owned(), Value::from(16));
            });
            let error = parse_scheme(&json).unwrap_err();
            assert!(
                matches!(error, SchemeParseError::NotAString { key: named } if named == key),
                "a numeric `{key}` must be refused by name, got: {error}"
            );
            assert!(error.to_string().contains(key));
        }
    }

    /// PIN — an empty name is refused.
    ///
    /// A scheme is chosen by name and stored in `settings.json` by name, so a
    /// nameless one is the same value the empty string already means there:
    /// "this build's default". It could never be selected, and if it somehow
    /// were, it would be indistinguishable from having selected nothing.
    #[test]
    fn a_scheme_with_no_name_is_refused() {
        let json = nord_with(|object| {
            object.insert("name".to_owned(), Value::from(""));
        });
        assert!(matches!(
            parse_scheme(&json).unwrap_err(),
            SchemeParseError::EmptyName
        ));
    }

    /// PIN — the keys this product has no use for are ignored rather than
    /// refused.
    ///
    /// All four appear in files people actually download: the first two are
    /// standard Windows Terminal scheme keys Folio does not read, and the last
    /// two are what arrives when somebody copies a whole profile instead of the
    /// scheme inside it.
    #[test]
    fn unknown_keys_are_ignored() {
        let json = nord_with(|object| {
            object.insert("cursorTextColor".to_owned(), Value::from("#2e3440"));
            object.insert("selectionForeground".to_owned(), Value::from("#2e3440"));
            object.insert("useAcrylic".to_owned(), Value::from(true));
            object.insert("historySize".to_owned(), Value::from(9001));
        });
        assert_eq!(parse_scheme(&json).unwrap(), parse_scheme(NORD).unwrap());
    }

    /// PIN — a file that is not JSON at all says so, rather than surfacing as a
    /// missing key.
    #[test]
    fn something_that_is_not_json_is_refused_as_such() {
        for text in ["", "not json", "{", "{\"name\": }"] {
            assert!(
                matches!(
                    parse_scheme(text).unwrap_err(),
                    SchemeParseError::NotJson { .. }
                ),
                "`{text}` is not JSON"
            );
        }
    }

    /// PIN — JSON that is not an object says what it is instead.
    ///
    /// The array case is the one with a user behind it: it is what you get by
    /// copying the whole `schemes` list out of a `settings.json`, and the
    /// message has to be the thing that tells them to take one entry out of it.
    #[test]
    fn json_that_is_not_an_object_is_refused_with_its_shape() {
        for (text, shape) in [
            ("[]", "an array"),
            ("\"Nord\"", "a string"),
            ("7", "a number"),
            ("null", "null"),
            ("true", "a boolean"),
        ] {
            let error = parse_scheme(text).unwrap_err();
            assert!(
                matches!(error, SchemeParseError::NotAnObject { found } if found == shape),
                "`{text}` must be refused as {shape}, got: {error}"
            );
            assert!(error.to_string().contains(shape));
        }
    }

    /// PIN — what [`write_scheme`] writes, [`parse_scheme`] reads back as the
    /// very same scheme.
    ///
    /// The sample is a foreign file that names neither `accent` nor a cursor of
    /// its own, so the round trip has to carry the *resolved* colours out —
    /// which is what makes a written file independent of the fallbacks that
    /// filled it in.
    #[test]
    fn a_written_scheme_parses_back_to_the_scheme_it_was_written_from() {
        let scheme = parse_scheme(NORD).unwrap();
        let written = write_scheme(&scheme);
        assert_eq!(parse_scheme(&written).unwrap(), scheme);
        assert!(
            written.contains(r##""accent": "#81a1c1""##),
            "a fallback that filled a key in is written out as a value: {written}"
        );
    }

    /// PIN — the keys come out in the order the bundled ten are written in, and
    /// all twenty-two of them come out.
    #[test]
    fn a_written_scheme_spells_every_key_in_the_bundled_order() {
        let written = write_scheme(&parse_scheme(NORD).unwrap());
        let keys: Vec<&str> = written
            .lines()
            .filter_map(|line| line.trim().strip_prefix('"'))
            .filter_map(|rest| rest.split('"').next())
            .collect();
        assert_eq!(
            keys,
            [
                "name",
                "background",
                "foreground",
                "cursorColor",
                "selectionBackground",
                "accent",
                "black",
                "red",
                "green",
                "yellow",
                "blue",
                "purple",
                "cyan",
                "white",
                "brightBlack",
                "brightRed",
                "brightGreen",
                "brightYellow",
                "brightBlue",
                "brightPurple",
                "brightCyan",
                "brightWhite",
            ]
        );
    }

    /// PIN — the shape of the file, not just its content: two-space indent, one
    /// key per line, lower-case hex, a closing newline.
    ///
    /// A user is going to edit this file by hand, so the layout is part of what
    /// is being written; `serde_json::to_string` would satisfy the two tests
    /// above with one very long line.
    #[test]
    fn a_written_scheme_is_laid_out_the_way_a_bundled_file_is() {
        let written = write_scheme(&parse_scheme(NORD).unwrap());
        assert!(written.starts_with("{\n  \"name\": \"Nord\",\n"));
        assert!(written.ends_with("\n  \"brightWhite\": \"#eceff4\"\n}\n"));
        assert!(
            !written.contains("#ECEFF4"),
            "hex is written lower case whatever the source file used"
        );
        assert_eq!(written.lines().count(), 24, "a brace, 22 keys, a brace");
    }

    /// A name with something to escape in it writes a file that parses.
    #[test]
    fn a_name_that_needs_escaping_survives_the_round_trip() {
        let mut scheme = parse_scheme(NORD).unwrap();
        scheme.name = r#"He said "hi"\"#.to_owned();
        assert_eq!(
            parse_scheme(&write_scheme(&scheme)).unwrap().name,
            scheme.name
        );
    }
}
