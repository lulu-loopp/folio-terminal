//! **Whether this build may update itself** — decided once, by `build.rs`, from
//! the one environment variable the release pipeline sets.
//!
//! **This is build-script code.** `build.rs` reaches it by `#[path]`; the binary
//! compiles it only under `cfg(test)`, which is how a build script's decision gets
//! tests at all, and ships none of it (the pattern of `bt-pty`'s
//! `conpty_sidecar`). What ships is the answer: `build.rs` emits the cfg
//! [`CFG`] when, and only when, [`decide`] says yes, and
//! `crate::update::eligible` reads that cfg.
//!
//! # Why a compile-time flag, and why only one value
//!
//! `docs/plans/design/self-update-2026-09-16.md` revision (b), F-13, and the
//! owner's ruling of 2026-09-25: a copy may update itself only when the running
//! build is signed **and** was built with the updater flag. A signature cannot
//! tell a release from a candidate — `sign.ps1 -OutDir` signs builds that are
//! published nowhere — so the flag is set by the release build invocation and by
//! nothing else: `build-release.yml` on a `v*` tag or an explicit dispatch input,
//! and the macOS release build. It is a *capability the bytes carry*, not a
//! provenance claim: copied bytes carry it too.
//!
//! **Exactly one value turns it on, and every other non-empty value stops the
//! build.** A flag that took `1`, `true` and `yes` would take a typo too, and a
//! typo in the release invocation that silently built a copy unable to update
//! itself is found out one release later, on everybody's machine at once. Unset
//! and empty are the same answer — off — which is the environment's own rule for
//! every variable this workspace reads (`docs/ARCHITECTURE.md` §9: set-but-empty
//! is off).

/// The environment variable `build.rs` reads.
pub const VARIABLE: &str = "FOLIO_UPDATER";

/// The one value that makes a build eligible.
pub const VALUE: &str = "on";

/// The cfg `build.rs` emits for an eligible build, and declares to `check-cfg`
/// for every build.
#[cfg_attr(
    test,
    expect(
        dead_code,
        reason = "build.rs's; the crate reads the cfg by name, as `cfg!(folio_updater)` in `update::eligible`"
    )
)]
pub const CFG: &str = "folio_updater";

/// **The decision**: `Ok(true)` for exactly [`VALUE`], `Ok(false)` for unset or
/// empty, and a refusal naming [`VARIABLE`] for anything else.
///
/// `value` is what the environment held, already decoded; a value that is not
/// Unicode is `Some` of something that is not [`VALUE`], which `build.rs` passes
/// as its lossy form so that the refusal can quote it.
pub fn decide(value: Option<&str>) -> Result<bool, String> {
    match value {
        None | Some("") => Ok(false),
        Some(VALUE) => Ok(true),
        Some(other) => Err(format!(
            "{VARIABLE} is `{other}`; the one value it takes is `{VALUE}` \
             (unset or empty builds a copy that never updates itself)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{VALUE, VARIABLE, decide};

    /// RED (U-8) — **the flag accepts only its exact value: unset and empty are
    /// off, `on` is on, and anything else is refused with the variable's name.**
    ///
    /// The release invocation is typed by a person or a workflow file, and the
    /// two failures are asymmetric: a refused build is found out in the minute it
    /// runs, while a lenient reading that took `On`, `1` or ` on` as off would
    /// ship a release unable to update itself, and one that took them as on would
    /// make the flag mean "anything at all". So every spelling that is not the
    /// value is a refusal, and the refusal names the variable so the person
    /// reading a failed build log knows which line of the invocation to fix.
    ///
    /// MUTATION: make `decide` answer `Ok(false)` for the catch-all arm and the
    /// refusal assertions go red; match `VALUE` case-insensitively and the `On`
    /// row goes red.
    #[test]
    fn the_flag_accepts_only_its_exact_value() {
        assert_eq!(
            decide(None),
            Ok(false),
            "unset is a build that never updates itself"
        );
        assert_eq!(
            decide(Some("")),
            Ok(false),
            "set-but-empty is off, as everywhere"
        );
        assert_eq!(
            decide(Some(VALUE)),
            Ok(true),
            "`{VALUE}` is the one value that is on"
        );

        for other in [
            "On", "ON", "1", "true", "yes", " on", "on ", "off", "0", "no",
        ] {
            let refusal = decide(Some(other)).expect_err(&format!(
                "`{other}` is neither unset nor `{VALUE}`, so it is refused"
            ));
            assert!(
                refusal.contains(VARIABLE),
                "a refusal names the variable it is about: {refusal}"
            );
            assert!(
                refusal.contains(&format!("`{other}`")),
                "and quotes what it was given: {refusal}"
            );
        }
    }
}
