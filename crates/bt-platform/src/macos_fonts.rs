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
//! view and nothing here is the main thread's. Apple's *Thread Safety Summary*
//! rule for what it does not list applies — "in most cases, you can use these
//! classes from any thread as long as you use them from only one thread at a
//! time" — and this is called from one place, `settings::monospace_families`,
//! behind a lock of its own. See `handoff::macos_handoff`'s header for the same
//! statement made about `NSWorkspace`.

use objc2_core_foundation::{CFArray, CFDictionary, CFNumber, CFString, CFType};
use objc2_core_text::{
    CTFontCollection, CTFontDescriptor, CTFontSymbolicTraits, kCTFontFamilyNameAttribute,
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

    let mut families = Vec::new();
    for descriptor in descriptors.iter() {
        if !is_monospaced(&descriptor) {
            continue;
        }
        let Some(name) = family_name(&descriptor) else {
            continue;
        };
        if name.starts_with('.') || name.trim().is_empty() {
            continue;
        }
        families.push(MonospaceFamily {
            name,
            // Nothing to load: `bt-render`'s macOS font system already holds
            // every installed face. See this module's own header.
            files: Vec::new(),
        });
    }
    families
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
