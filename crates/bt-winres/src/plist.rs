//! **The macOS bundle's `Info.plist`, rendered from the template beside it** —
//! the other container the product's one version line has to reach.
//!
//! # Why this is in a crate named for a Windows resource
//!
//! Because it is the same job, and the job is not "Windows". What this crate
//! does is take the workspace's one version string and produce the bytes a
//! platform's *container* wants it in, refusing a version that container cannot
//! carry — [`FileVersion::parse_semver`](crate::FileVersion::parse_semver) is
//! that refusal for `VS_FIXEDFILEINFO`'s four numbers, and [`check_version`]
//! below is it for `CFBundleVersion`'s dotted integers. Both are pure string
//! work over a documented layout, with no dependencies and no platform API, and
//! both are read by the same gate in `bt_app::version`.
//!
//! The two places it could have gone instead are worse for reasons worth
//! writing down. `bt-app` is a binary crate with no library, so nothing there
//! can be called by a second binary without compiling the whole window —
//! several hundred packages — to print forty lines of XML on the machine that
//! is assembling the bundle. And a build script cannot be it either: the
//! refusals above are the interesting half, and a refusal that only happens
//! during a build is a refusal no test can ask for.
//!
//! # What the renderer is, exactly
//!
//! A substitution of three names, and two refusals.
//! `packaging/macos/Info.plist.in` is XML that is already a valid plist apart
//! from `@VERSION@` standing where a version belongs, twice, and `@PROTOCOL@`
//! and `@MIN_UPDATER@` standing where the release's update protocol and the
//! oldest updater that can install it belong. [`render`] puts the workspace
//! version in the first two places and
//! [`crate::release_manifest::PROTOCOL`] and
//! [`crate::release_manifest::MIN_UPDATER`] in the other two — the same two
//! constants the Windows build writes into the manifest `folio.exe` carries
//! (0.4.6 ticket U-9; on macOS the bundle's seal already covers every file, so
//! these two keys are all of that manifest a bundle needs) — and hands back the
//! text. It is deliberately not a plist *writer*: the
//! template is the file a person reads, argues with and comments, and a
//! generator that emitted the whole document from Rust would move all of that
//! into code nobody opens when they want to know what the bundle claims.

use crate::ResourceError;
use crate::release_manifest::{MIN_UPDATER, PROTOCOL};

/// The names [`render`] fills in. Any other `@…@` is refused.
const PLACEHOLDERS: [&str; 3] = ["VERSION", "PROTOCOL", "MIN_UPDATER"];

/// The template, with `@VERSION@` replaced by `version`, `@PROTOCOL@` by
/// [`PROTOCOL`] and `@MIN_UPDATER@` by [`MIN_UPDATER`], everywhere they stand.
///
/// # Errors
///
/// [`ResourceError`] when `version` is not something `CFBundleVersion` can
/// carry (see [`check_version`]), or when the template names a placeholder
/// other than those three — which would otherwise be shipped verbatim inside a
/// signed bundle, where the first reader of it is a user.
pub fn render(template: &str, version: &str) -> Result<String, ResourceError> {
    check_version(version)?;
    let protocol = PROTOCOL.to_string();
    let value_of = |name: &str| match name {
        "VERSION" => Some(version),
        "PROTOCOL" => Some(protocol.as_str()),
        "MIN_UPDATER" => Some(MIN_UPDATER),
        _ => None,
    };

    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find('@') {
        // `@` is one byte and never part of a multi-byte character, so slicing
        // around it cannot split one.
        let after = &rest[at + 1..];
        let name_ends =
            after.find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'));
        let Some(end) = name_ends.filter(|&end| end > 0 && after.as_bytes()[end] == b'@') else {
            // A lone `@` — in an address, in prose, in a comment. Not every `@`
            // in an XML file is a placeholder, and refusing one would make this
            // a rule about the character rather than about substitution.
            out.push_str(&rest[..=at]);
            rest = after;
            continue;
        };
        let name = &after[..end];
        let Some(value) = value_of(name) else {
            return Err(ResourceError(format!(
                "the template asks for @{name}@, which nothing fills; \
                 the substitutions are {}",
                PLACEHOLDERS.map(|known| format!("@{known}@")).join(", ")
            )));
        };
        out.push_str(&rest[..at]);
        out.push_str(value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Whether `CFBundleVersion` can carry this version string.
///
/// **One to three dotted integers and nothing else** — that is the whole of
/// what the key accepts, and the system compares builds with it, so a value it
/// cannot read is not a cosmetic fault: `0.3.0-preview` installs as a build the
/// machine cannot order against `0.3.0`.
///
/// Cargo is looser: `0.3.0-preview` and `0.3.0+deadbeef` are versions it is
/// happy to carry, and the Windows resource is looser still in one direction —
/// it drops the suffix into the four numbers and keeps the whole string beside
/// them in `FileVersion`. A plist has nowhere to keep it, which is why this
/// refuses rather than truncates. It never has to fire in a release:
/// `docs/RELEASING.md` puts the channel suffix on the *tag* and never in the
/// manifest. It fires the day somebody changes that, which is the point.
///
/// # Errors
///
/// [`ResourceError`] naming what the string has in it that a bundle cannot.
fn check_version(version: &str) -> Result<(), ResourceError> {
    let fields: Vec<&str> = version.split('.').collect();
    if fields.len() > 3 {
        return Err(ResourceError(format!(
            "CFBundleVersion takes at most three dotted integers; {version} has {}",
            fields.len()
        )));
    }
    for field in fields {
        if field.is_empty() || !field.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ResourceError(format!(
                "CFBundleVersion takes dotted integers only; {version} is not one"
            )));
        }
        if field.parse::<u32>().is_err() {
            return Err(ResourceError(format!(
                "CFBundleVersion cannot carry {version}: a field does not fit in an integer"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::render;

    /// A template with the shape of the real one and none of its length.
    fn template() -> String {
        "<key>CFBundleShortVersionString</key>\n<string>@VERSION@</string>\n\
         <key>CFBundleVersion</key>\n<string>@VERSION@</string>\n"
            .to_owned()
    }

    /// PIN — **a version cargo carries happily and a bundle cannot is refused,
    /// not truncated.**
    ///
    /// `CFBundleVersion` is what macOS orders two builds of the same app with.
    /// The tempting alternative — drop the suffix, as the Windows resource's
    /// four numbers do — would put `0.3.0` in a bundle built from `0.3.0-rc.2`
    /// and leave nothing anywhere in it saying which of the two it is.
    ///
    /// MUTATION: make `check_version` accept a pre-release suffix and this
    /// fails on the first case.
    #[test]
    fn a_version_cargo_can_carry_but_a_bundle_cannot_is_refused() {
        for refused in [
            "0.3.0-preview",
            "0.3.0-rc.2",
            "0.3.0+2026091201",
            "0.3.0.1",
            "0.3.x",
            "0.3.",
            "",
            "v0.3.0",
            "4294967296.0.0",
        ] {
            assert!(
                render(&template(), refused).is_err(),
                "a bundle cannot carry {refused:?}"
            );
        }

        for taken in ["0.3.0", "0", "1.0", "0.2.5", "10.11.12"] {
            assert!(
                render(&template(), taken).is_ok(),
                "a bundle can carry {taken:?}"
            );
        }
    }

    /// PIN — **the template may ask for three substitutions, and these are
    /// they.**
    ///
    /// An unfilled `@…@` does not fail a build, does not fail `codesign`, and
    /// does not fail notarization: it ships, inside a signed bundle, and is read
    /// by Launch Services as the literal text it is.
    ///
    /// MUTATION: fill any `@NAME@` with an empty string instead of refusing it
    /// and the first half fails.
    #[test]
    fn an_unknown_placeholder_is_refused() {
        let unknown = "<string>@VERSION@</string>\n<string>@BUILD@</string>\n";
        let error = render(unknown, "0.3.0").expect_err("@BUILD@ is nobody's to fill");
        assert!(error.to_string().contains("@BUILD@"), "{error}");

        // And a `@` that is not a placeholder is left where it is: this is a
        // rule about substitution, not about the character.
        let address = "<string>folio@example.invalid</string>\n<string>@VERSION@</string>\n";
        let rendered = render(address, "0.3.0").expect("a lone @ is not a placeholder");
        assert_eq!(
            rendered,
            "<string>folio@example.invalid</string>\n<string>0.3.0</string>\n"
        );
    }

    /// RED (U-9) — **the bundle carries the release's update protocol and the
    /// oldest updater that can install it, from the constants the Windows
    /// manifest carries.**
    ///
    /// `docs/plans/design/self-update-2026-09-16.md` revision (b), F-4: on
    /// macOS the bundle's seal covers every file, so the manifest `folio.exe`
    /// carries reduces to two facts, and they travel as `Info.plist` keys —
    /// sealed by `codesign` like the rest of the file. A running build reads its
    /// successor's before it quits (F-8), so a key spelled differently from the
    /// constant, or a literal that stayed behind when the constant moved, is an
    /// updater that refuses or accepts the wrong release.
    ///
    /// MUTATION: write `1` into the template in place of `@PROTOCOL@` and move
    /// `PROTOCOL` to 2; or drop the two keys from the template.
    #[test]
    fn the_rendered_bundle_carries_the_update_protocol_and_min_updater() {
        use crate::release_manifest::{MIN_UPDATER, PROTOCOL};

        let template = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packaging/macos/Info.plist.in"
        ))
        .expect("the bundle template is at packaging/macos/Info.plist.in");
        assert_eq!(template.matches("@PROTOCOL@").count(), 1, "one key, filled");
        assert_eq!(
            template.matches("@MIN_UPDATER@").count(),
            1,
            "one key, filled"
        );

        let plist = render(&template, "0.3.0").expect("the shipped template renders");
        let value = |key: &str| {
            let after = plist
                .split_once(&format!("<key>{key}</key>"))
                .unwrap_or_else(|| panic!("the bundle declares {key}"))
                .1
                .trim_start();
            let (tag, rest) = after
                .strip_prefix('<')
                .and_then(|rest| rest.split_once('>'))
                .unwrap_or_else(|| panic!("{key} is followed by a value"));
            let (value, _) = rest
                .split_once(&format!("</{tag}>"))
                .unwrap_or_else(|| panic!("{key}'s value is closed"));
            (tag.to_owned(), value.to_owned())
        };
        assert_eq!(
            value("FolioUpdateProtocol"),
            ("integer".to_owned(), PROTOCOL.to_string())
        );
        assert_eq!(
            value("FolioMinUpdater"),
            ("string".to_owned(), MIN_UPDATER.to_owned())
        );
    }

    /// PIN — **the shipped template's Services declaration survives the
    /// render, whole** (M4-9).
    ///
    /// The three tests above are about the substitution; this one is about the
    /// **file**, and it is here rather than in a fixture because what a reader
    /// right-clicks a folder into is the rendered text and not the template.
    /// `NSServices` is the one key in this document whose value is a nested
    /// structure — a dictionary inside an array inside a dictionary — and it is
    /// also the only one a renderer could plausibly damage, because it is the
    /// only place where a `<dict>` is closed before the document's own is.
    ///
    /// The pairing between `NSMessage` and the selector the provider object
    /// answers is `bt_platform::app_delegate`'s pin
    /// (`the_service_this_bundle_declares_is_the_one_the_provider_answers`):
    /// that claim is about two files, and this one is about one.
    ///
    /// MUTATION: make `render` return the text up to the last placeholder and
    /// this fails, where the version pins do not — every `@VERSION@` is above
    /// this array.
    #[test]
    fn the_rendered_bundle_declares_the_finder_service() {
        let template = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packaging/macos/Info.plist.in"
        ))
        .expect("the bundle template is at packaging/macos/Info.plist.in");
        let plist = render(&template, "0.3.0").expect("the shipped template renders");

        for expected in [
            "<key>NSServices</key>",
            "<key>NSMenuItem</key>",
            "<string>Open in Folio</string>",
            "<key>NSMessage</key>",
            "<string>openInFolio</string>",
            "<key>NSPortName</key>",
            "<key>NSSendTypes</key>",
            "<string>public.file-url</string>",
        ] {
            assert!(
                plist.contains(expected),
                "the rendered bundle is missing {expected}"
            );
        }
        assert!(
            plist.trim_end().ends_with("</plist>"),
            "the document is closed"
        );
    }

    /// PIN — **the bundle asks the reader's permission for nothing, and that is
    /// what stands between a previewed page and CoreLocation** (M5-1, and
    /// `docs/DESIGN.md` §13.38).
    ///
    /// M4-3 measured that WebKit on this system offers **no** delegate method
    /// for geolocation — not under the public spelling and not under the private
    /// one — so the web host cannot refuse that capability at all. What refuses
    /// it is the *bundle*: an application carrying no `NSLocation…
    /// UsageDescription` cannot be authorised for location, so the request
    /// cannot be granted and no prompt can be raised. The same is true of the
    /// camera and the microphone, which the delegate *does* refuse, with this
    /// file as the second lock.
    ///
    /// That makes a purpose string added here for some unrelated feature a
    /// change to what a previewed page can reach, several sections away from the
    /// paragraph that says so. This test is the noise that change makes.
    ///
    /// It is written as a rule about **every key**, not as a list of three: the
    /// key a future feature would add is one nobody has typed yet, and a list
    /// would not have it. Prose is not searched — the template's own comment
    /// explains at length why these keys are absent, and a test that refused the
    /// explanation as well as the key would be a test against writing it down.
    ///
    /// MUTATION: add `<key>NSCameraUsageDescription</key><string>…</string>` to
    /// `Info.plist.in` and this fails naming it.
    #[test]
    fn the_rendered_bundle_asks_the_reader_for_nothing() {
        let template = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packaging/macos/Info.plist.in"
        ))
        .expect("the bundle template is at packaging/macos/Info.plist.in");
        let plist = render(&template, "0.3.0").expect("the shipped template renders");

        let declared: Vec<&str> = plist
            .match_indices("<key>")
            .filter_map(|(at, opening)| {
                let after = &plist[at + opening.len()..];
                after.find("</key>").map(|end| &after[..end])
            })
            .collect();
        assert!(
            declared.contains(&"CFBundleIdentifier"),
            "the keys were not found at all, so finding none of the wrong ones proves nothing"
        );

        for key in &declared {
            assert!(
                !key.ends_with("UsageDescription"),
                "the bundle declares {key}, which is a permission this product does not request"
            );
        }

        // Named as well as ruled out, because these three are the ones the
        // sections above argue about and a reader looking for them should find
        // them written here.
        for never in [
            "NSLocationUsageDescription",
            "NSLocationWhenInUseUsageDescription",
            "NSLocationAlwaysAndWhenInUseUsageDescription",
            "NSCameraUsageDescription",
            "NSMicrophoneUsageDescription",
        ] {
            assert!(!declared.contains(&never), "the bundle declares {never}");
        }
    }

    /// Every `@VERSION@` is filled, and the rest of the file is handed back
    /// byte for byte — including the parts that look like versions and are not.
    #[test]
    fn every_version_placeholder_is_filled_and_nothing_else_moves() {
        let source = "<plist version=\"1.0\">@VERSION@ @VERSION@\n<string>14.0</string>\n";
        let rendered = render(source, "0.3.0").expect("the template renders");
        assert_eq!(
            rendered,
            "<plist version=\"1.0\">0.3.0 0.3.0\n<string>14.0</string>\n"
        );
    }
}
