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
//! A substitution of one name, and two refusals. `packaging/macos/Info.plist.in`
//! is XML that is already a valid plist apart from `@VERSION@` standing where a
//! version belongs, twice; [`render`] puts the workspace version in both places
//! and hands back the text. It is deliberately not a plist *writer*: the
//! template is the file a person reads, argues with and comments, and a
//! generator that emitted the whole document from Rust would move all of that
//! into code nobody opens when they want to know what the bundle claims.

use crate::ResourceError;

/// The one name [`render`] fills in. Any other `@…@` is refused.
const VERSION_PLACEHOLDER: &str = "VERSION";

/// The template, with `@VERSION@` replaced by `version` everywhere it stands.
///
/// # Errors
///
/// [`ResourceError`] when `version` is not something `CFBundleVersion` can
/// carry (see [`check_version`]), or when the template names a placeholder
/// other than `@VERSION@` — which would otherwise be shipped verbatim inside a
/// signed bundle, where the first reader of it is a user.
pub fn render(template: &str, version: &str) -> Result<String, ResourceError> {
    check_version(version)?;

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
        if name != VERSION_PLACEHOLDER {
            return Err(ResourceError(format!(
                "the template asks for @{name}@, which nothing fills; \
                 @{VERSION_PLACEHOLDER}@ is the only substitution"
            )));
        }
        out.push_str(&rest[..at]);
        out.push_str(version);
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

    /// PIN — **the template may ask for one substitution, and this is it.**
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
