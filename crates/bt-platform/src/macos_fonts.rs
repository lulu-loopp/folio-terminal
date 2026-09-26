//! **Every monospaced family this Mac has, for the settings page's picker** —
//! the CoreText twin of `windows_impl`'s DirectWrite enumeration (ticket M2-4).
//!
//! # What the picker needs, and what each platform can give it
//!
//! The Windows arm answers with a name **and the files the family's faces live
//! in**, and it goes through DirectWrite rather than GDI precisely to get the
//! second half: `bt-render`'s font database on Windows is a *fixed list of
//! seven files* under `%WINDIR%\Fonts`, loaded so that startup is bounded, so a
//! family the reader picks out of the list is a family the renderer has never
//! opened — and `set_terminal_font` loads it from the paths this enumeration
//! hands over.
//!
//! **On macOS that second half is already done.** `bt-render`'s
//! `terminal_font_system` on this platform calls `Database::load_system_fonts`,
//! which walks `/System/Library/Fonts`, `/Library/Fonts`, the `AssetsV2` font
//! assets and `~/Library/Fonts` and memory-maps every face it finds — that arm
//! exists because PingFang and Hiragino live under content-hashed directory
//! names that a fixed file list cannot keep up with. So every family this
//! module can name is a family the renderer already holds, and the honest value
//! for `files` here is **the empty vector**: `MonospaceFamily::files` documents
//! it as "this family needs no loading", `set_terminal_font` loops over an empty
//! slice and loads nothing, and a path invented to fill the field would be a
//! second answer to a question that is already settled.
//!
//! That is also why this module is small. The whole of the work is: ask CoreText
//! for the descriptors, keep the ones whose traits carry the monospace bit, take
//! their family names, and hand the list to [`crate::order_monospace_families`]
//! — which is the same sort, the same de-duplication and the same guarantee
//! about the default face that the Windows list goes through, because a picker
//! that ordered its rows differently on two machines would be a picker nobody
//! can learn twice.
//!
//! # The four entry points, and why they arrive as a package
//!
//! M2-1 declared FSEvents' seven entry points by hand, because CoreServices has
//! no binding in the `objc2` family and no crate in the lock file. CoreText
//! does: `objc2-core-text` 0.3.2, from the same repository and the same release
//! as the `objc2-app-kit`, `objc2-foundation` and `objc2-core-foundation` this
//! crate already carries, generated from the same headers. One package
//! (`docs/DESIGN.md` §8's bar is that a dependency is a line somebody reads;
//! this is that line), and what it buys is the retain and release of every
//! `CTFontCollection` and `CTFontDescriptor` below handled by `CFRetained`
//! rather than by hand — which is the failure a hand-declared CoreText would
//! have, since every one of these functions is a `Copy`/`Create` that hands
//! back an owned reference.
//!
//! # The thread
//!
//! CoreText is a C API over the font database, not AppKit: nothing here owns a
//! view and nothing here is the main thread's. Since ticket 50 two threads call
//! in at once: the walk runs on `bt-app`'s font lane, and the lookup of one
//! family by name ([`monospace_family_named`]) on the window thread at launch.
//! That is within CoreText's own contract — its overview states that every
//! individual Core Text function is thread-safe and that font objects may be
//! used by several threads simultaneously — and the two share no object: each
//! call makes its own collection or descriptor and drops it before returning.

use objc2_core_foundation::{CFArray, CFDictionary, CFNumber, CFString, CFType};
use objc2_core_text::{
    CTFont, CTFontCollection, CTFontDescriptor, CTFontSymbolicTraits, kCTFontFamilyNameAttribute,
    kCTFontSymbolicTrait, kCTFontTraitsAttribute,
};

use crate::MonospaceFamily;

/// **Every monospaced family installed on this machine, in the order the picker
/// draws them.**
///
/// A `Vec` and never a `Result`, for the Windows arm's reason: there is nothing
/// a caller could do with the error that this has not already done. A machine
/// whose CoreText answers nothing still gets [`crate::DEFAULT_MONOSPACE_FAMILY`]
/// in the list — inserted by [`crate::order_monospace_families`] — which is the
/// face the renderer is already drawing, so the picker degrades to one row
/// rather than to an empty list or a dialog.
#[must_use]
pub fn monospace_font_families() -> Vec<MonospaceFamily> {
    crate::order_monospace_families(collect_monospace_families())
}

/// **One family, looked up by its name** — the CoreText twin of the Windows
/// arm's `FindFamilyName` door (ticket 50).
///
/// A descriptor carrying only `kCTFontFamilyNameAttribute`, matched against the
/// font database: CoreText answers with the one descriptor it prefers for that
/// family, or — when the family is not installed — with whatever it falls back
/// to, which is why the answer's own family name is compared with the one asked
/// for. The row is built by [`monospace_family_entry`], the same derivation the
/// walk makes for every face it keeps, so a family is monospaced here exactly
/// when it is a row of [`monospace_font_families`], and its `files` are empty
/// for the reason this module's header gives.
///
/// **A door** (`doors::FontFamilyLookup`), the same signature as the Windows arm's.
#[must_use]
pub fn monospace_family_named(
    token: crate::admission::WaitToken<'_, crate::admission::doors::FontFamilyLookup>,
    name: &str,
) -> Option<MonospaceFamily> {
    let _ = token;
    let wanted = CFString::from_str(name);
    // SAFETY: a CoreText constant string, read for the length of the call that
    // copies it into the dictionary.
    let key: &CFString = unsafe { kCTFontFamilyNameAttribute };
    let attributes = CFDictionary::<CFString, CFString>::from_slices(&[key], &[&*wanted]);
    // SAFETY: the dictionary is keyed by a CoreText attribute name and holds a
    // string under it, which is the type that attribute is documented to take.
    let descriptor = unsafe { CTFontDescriptor::with_attributes(attributes.as_opaque()) };
    // SAFETY: the descriptor above is live and no mandatory attributes are
    // passed; the match comes back owned or not at all.
    let matched = unsafe { descriptor.matching_font_descriptor(None) }?;
    monospace_family_entry(&matched).filter(|family| family.name.eq_ignore_ascii_case(name))
}

/// Every visible CJK family, read on the font worker.
///
/// **Asked of CoreText on every call** (ticket 65), for the Windows arm's
/// reason: it was read once per process, so a family installed after the
/// first open of Settings never reached the list. `from_available_fonts` is
/// the live font database; no flag asks it to look again.
#[must_use]
pub fn cjk_font_families() -> Vec<crate::CjkFamily> {
    crate::order_cjk_families(collect_cjk_families())
}

fn collect_cjk_families() -> Vec<crate::CjkFamily> {
    use objc2_core_text::{CTFontTableOptions, kCTFontFamilyNameKey};
    let collection = unsafe { CTFontCollection::from_available_fonts(None) };
    let Some(descriptors) = (unsafe { collection.matching_font_descriptors() }) else {
        return Vec::new();
    };
    let descriptors: &CFArray<CTFontDescriptor> = unsafe { descriptors.cast_unchecked() };
    let mut families = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for descriptor in descriptors.iter() {
        let Some(name) = family_name(&descriptor) else {
            continue;
        };
        if name.starts_with('.') || name.trim().is_empty() || !seen.insert(name.clone()) {
            continue;
        }
        let font = unsafe { CTFont::with_font_descriptor(&descriptor, 0.0, std::ptr::null()) };
        let table = |tag: &[u8; 4]| {
            unsafe { font.table(u32::from_be_bytes(*tag), CTFontTableOptions::NoOptions) }
                .map(|data| unsafe { data.as_bytes_unchecked() }.to_vec())
        };
        let os2 = table(b"OS/2");
        let cmap = table(b"cmap");
        let coverage = crate::CjkCoverage::from_tables(os2.as_deref(), cmap.as_deref());
        if !coverage.any() {
            continue;
        }
        let mut localized_names = Vec::new();
        // CoreText's own localized name API supplies its native UI-language
        // answer. Explicit name-table records handle an app language different
        // from macOS, without changing the process-global language preferences.
        if let Some(localized) =
            unsafe { font.localized_name(kCTFontFamilyNameKey, std::ptr::null_mut()) }
        {
            localized_names.push((crate::os_ui_language(), localized.to_string()));
        }
        if let Some(bytes) = table(b"name")
            && let Some(names) = ttf_parser::name::Table::parse(&bytes)
        {
            for record in names.names {
                if record.name_id != ttf_parser::name_id::FAMILY {
                    continue;
                }
                let locale = match record.language_id {
                    0x0804 => "zh-CN",
                    0x0409 => "en-US",
                    _ => continue,
                };
                if let Some(text) = record.to_string() {
                    localized_names.retain(|(lang, _)| lang != locale);
                    localized_names.push((locale.into(), text));
                }
            }
        }
        families.push(crate::CjkFamily {
            name,
            files: Vec::new(),
            localized_names,
            coverage,
        });
    }
    families
}

/// The enumeration itself: one collection, one pass, one predicate.
///
/// `CTFontCollectionCreateFromAvailableFonts` is the whole font database as the
/// system sees it, and `CTFontCollectionCreateMatchingFontDescriptors` is its
/// contents as **descriptors** — one per *face*, so a family with a regular, a
/// bold and an italic arrives three times. The repeats are not filtered here:
/// `order_monospace_families` de-duplicates case-insensitively after sorting,
/// which is the same shape the Windows arm relies on for a family whose
/// localized names coincide, and doing it twice would be two rules to keep in
/// step.
///
/// **The trait is read from the traits dictionary rather than from the family
/// name.** `kCTFontSymbolicTrait` is a `CFNumber` of bit flags, and
/// `kCTFontTraitMonoSpace` (bit 10) is the font designer's own statement that
/// every glyph in the face has the same advance. Guessing from a name would put
/// `Menlo Sans` in a terminal's font picker and leave out a fixed-width face
/// whose name says nothing.
///
/// **Families whose name begins with a dot are skipped**, and that is Apple's
/// own convention rather than a filter invented here: `.SF NS Mono`,
/// `.LastResort` and their neighbours are installed dot-prefixed *so that a
/// font picker will not offer them* — the same fact `bt-render`'s
/// `MACOS_CHROME_SANS_FAMILIES` records about `.AppleSystemUIFont`. A row for
/// one is a row that can be chosen and then behaves unlike anything the reader
/// can see in Font Book.
fn collect_monospace_families() -> Vec<MonospaceFamily> {
    // SAFETY: no options are passed, which the function documents as "all
    // available fonts", and the collection is owned by the `CFRetained` that
    // comes back.
    let collection = unsafe { CTFontCollection::from_available_fonts(None) };
    // SAFETY: the collection above was made by `from_available_fonts`, which is
    // the one constructor this call is defined for.
    let Some(descriptors) = (unsafe { collection.matching_font_descriptors() }) else {
        return Vec::new();
    };
    // SAFETY: CoreText documents this array's elements as `CTFontDescriptorRef`
    // — it is the return of `CTFontCollectionCreateMatchingFontDescriptors`,
    // whose whole subject is descriptors — so reinterpreting the untyped array
    // as an array of them is reading it as what it holds.
    let descriptors: &CFArray<CTFontDescriptor> = unsafe { descriptors.cast_unchecked() };

    descriptors
        .iter()
        .filter_map(|descriptor| monospace_family_entry(&descriptor))
        .collect()
}

/// **One face as a picker row**, or `None` when it is not one: the one
/// derivation the walk and the lookup by name share (`CONVENTIONS` §十 rule 9).
fn monospace_family_entry(descriptor: &CTFontDescriptor) -> Option<MonospaceFamily> {
    if !is_monospaced(descriptor) {
        return None;
    }
    let name = family_name(descriptor)?;
    if name.starts_with('.') || name.trim().is_empty() {
        return None;
    }
    Some(MonospaceFamily {
        name,
        // Nothing to load: `bt-render`'s macOS font system already holds
        // every installed face. See this module's own header.
        files: Vec::new(),
    })
}

/// Whether this face's own traits carry the monospace bit.
fn is_monospaced(descriptor: &CTFontDescriptor) -> bool {
    // SAFETY: `kCTFontTraitsAttribute` is a CoreText constant string and the
    // descriptor is live; the call copies the value and hands back an owned
    // reference, or nothing when the attribute is absent.
    let Some(traits) = (unsafe { descriptor.attribute(kCTFontTraitsAttribute) }) else {
        return false;
    };
    let Ok(traits) = traits.downcast::<CFDictionary>() else {
        return false;
    };
    // SAFETY: CoreText documents the traits value as a dictionary keyed by its
    // own constant strings, of which `kCTFontSymbolicTrait` is one.
    let traits: &CFDictionary<CFString, CFType> = unsafe { traits.cast_unchecked() };
    // SAFETY: a CoreText constant string, read for the length of the lookup.
    let Some(symbolic) = traits.get(unsafe { kCTFontSymbolicTrait }) else {
        return false;
    };
    let Ok(symbolic) = symbolic.downcast::<CFNumber>() else {
        return false;
    };
    let Some(bits) = symbolic.as_i64() else {
        return false;
    };
    // The mask is CoreText's own constant rather than `1 << 10` written out:
    // the number is a fact about their header, not about this file.
    bits & i64::from(CTFontSymbolicTraits::TraitMonoSpace.bits()) != 0
}

/// The family name this face belongs to, as the system spells it.
fn family_name(descriptor: &CTFontDescriptor) -> Option<String> {
    // SAFETY: `kCTFontFamilyNameAttribute` is a CoreText constant string and
    // the descriptor is live; the value comes back owned or not at all.
    let name = unsafe { descriptor.attribute(kCTFontFamilyNameAttribute) }?;
    let name = name.downcast::<CFString>().ok()?;
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RED — **the families this Mac answers with are real, sorted, and carry
    /// the face the renderer draws when nothing has been chosen.**
    ///
    /// Deliberately not an assertion about *which* families beyond one: the
    /// machine running this decides that, and pinning "SF Mono is present"
    /// would fail on an account that has removed it. What is pinned is the
    /// contract every caller depends on — a name that is not empty, no name
    /// that Font Book hides, the picker's order, no repeats, and
    /// [`crate::DEFAULT_MONOSPACE_FAMILY`] among them, because the picker shows
    /// that row as selected for a `settings.json` that names no family and a
    /// selected row missing from its own list draws as a blank.
    ///
    /// MUTATIONS: ① drop the trait filter and the list is every family on the
    /// machine, which is a monospace picker offering Helvetica; ② drop the sort
    /// and the rows move between launches.
    #[test]
    fn monospace_families_are_real_and_sorted() {
        let families = monospace_font_families();
        // **The list itself, for a reader running `-- --nocapture`.** Everything
        // below is a property rather than a name, on purpose — but the one
        // question somebody porting this actually asks is *what did the machine
        // say*, and an assertion message only answers it when the test fails.
        println!(
            "{} monospaced families; the first ten: {:?}",
            families.len(),
            families
                .iter()
                .take(10)
                .map(|family| family.name.as_str())
                .collect::<Vec<_>>()
        );
        assert!(
            families.iter().any(|family| family
                .name
                .eq_ignore_ascii_case(crate::DEFAULT_MONOSPACE_FAMILY)),
            "the default face is promised to be a row on every machine; got {:?}",
            families.iter().map(|f| &f.name).collect::<Vec<_>>()
        );
        assert!(
            families.len() > 1,
            "a Mac has more than one fixed-width family, so a list of one is an \
             enumeration that answered nothing: {families:?}"
        );
        for family in &families {
            assert!(!family.name.trim().is_empty(), "a row must have a name");
            assert!(
                !family.name.starts_with('.'),
                "{} is hidden from font pickers by Apple's own convention",
                family.name
            );
        }
        let mut sorted: Vec<String> = families
            .iter()
            .map(|family| family.name.to_lowercase())
            .collect();
        let listed = sorted.clone();
        sorted.sort();
        assert_eq!(listed, sorted, "the picker's rows are alphabetical");
        let mut seen = sorted.clone();
        seen.dedup();
        assert_eq!(
            seen, sorted,
            "a family is one row, however many faces it has"
        );
    }
}
