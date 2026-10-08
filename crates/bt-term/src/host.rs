//! **What the program around this crate answers for it** — the facts about the machine a terminal
//! session needs and does not ask the platform for itself (`docs/ARCHITECTURE.md` §3.2).
//!
//! `bt-term` builds without the platform layer: a browser build references it, and there is no
//! operating system behind it there to name a host or set a thread's priority. So the host
//! program installs these answers once, at its start, before its first session:
//!
//! * [`install_host_names`] — the names this machine answers to, which a `file://<host>/` report
//!   is compared against ([`local_host_names`]);
//! * [`install_pool_thread_start`] — what every thread of the image resample pool runs first
//!   (Folio's desktop build puts the thread in the band below normal).
//!
//! **Each is one answer per process.** Installing the same host names again is a no-op; installing
//! different ones is a panic, and so is a second thread-start hook. A read of the host names before
//! anything was installed panics in every build profile: a session that quietly read "no names"
//! would take every `file://<host>/` report from this machine for a remote share, which is the
//! defect the installed fact exists to close (B-AUDIT-046 TRM-3). The thread-start hook is
//! optional: a host that installs none (a browser) gets a pool with no hook.
//!
//! The third answer a host gives this crate — the finished name a hand-off door would open — is
//! not installed: it is handed to [`crate::verify_path`] by the worker that calls it.

use std::sync::OnceLock;

/// The panic a read of the host names makes when no host has installed them.
pub const HOST_NAMES_READ_BEFORE_INSTALL: &str = "host names read before the host installed them";

/// The names a test process installs: one name no machine is given (`.invalid` is reserved for
/// exactly that, RFC 2606), so a test that reads it can only have been handed it.
pub const TEST_HOST_NAMES: &[&str] = &["folio-test-host.invalid"];

static HOST_NAMES: OnceLock<Vec<String>> = OnceLock::new();

/// `None` once read with nothing installed: the pool was built without a hook, and a hook
/// installed after that would never run.
static POOL_THREAD_START: OnceLock<Option<fn()>> = OnceLock::new();

/// **Install this machine's names** — every spelling a shell on it may put in the authority of a
/// `file://<host>/path` report. Called by the host before its first session.
///
/// # Panics
///
/// When different names were installed before: the process has one answer.
pub fn install_host_names(names: Vec<String>) {
    let Err(offered) = HOST_NAMES.set(names) else {
        return;
    };
    assert!(
        HOST_NAMES.get() == Some(&offered),
        "host names installed twice with different values: {:?}, then {offered:?}",
        HOST_NAMES.get()
    );
}

/// [`TEST_HOST_NAMES`], installed: what a test process calls in place of the host's own answer.
/// Equal values install idempotently, so every test of a process may call it.
pub fn install_test_host_names() {
    install_host_names(
        TEST_HOST_NAMES
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
    );
}

/// This machine's names, as the host installed them — the authorities a `file://` URI may carry
/// besides none and `localhost`.
///
/// **Installed by the host, never read from an environment variable** (B-AUDIT-046 TRM-3). The
/// reader used to read `COMPUTERNAME`, which exists only on Windows: on a Mac it answered nothing,
/// so every OSC 7 of the form `file://<host>/path` — fish's own report, Apple's
/// `zshrc_Apple_Terminal`, `vte.sh` — was taken for a remote share and the pane forgot its
/// directory.
///
/// This crate's own unit tests install [`TEST_HOST_NAMES`] here, in the one place every read
/// passes, so no test of the crate can read before an installation.
///
/// # Panics
///
/// With [`HOST_NAMES_READ_BEFORE_INSTALL`] when nothing was installed, in every build profile.
pub fn local_host_names() -> &'static [String] {
    #[cfg(test)]
    install_test_host_names();
    HOST_NAMES.get().expect(HOST_NAMES_READ_BEFORE_INSTALL)
}

/// **Install what every thread of the image resample pool runs first.** Called by the host before
/// its first session; a host that installs nothing gets a pool whose threads run no hook.
///
/// # Panics
///
/// When a hook was installed before, or when the pool was already built without one.
pub fn install_pool_thread_start(hook: fn()) {
    assert!(
        POOL_THREAD_START.set(Some(hook)).is_ok(),
        "the resample pool's thread-start hook was installed twice, or after the pool was built \
         without one"
    );
}

/// The installed thread-start hook, if any. Reading it settles the answer for the process.
pub(crate) fn pool_thread_start() -> Option<fn()> {
    *POOL_THREAD_START.get_or_init(|| None)
}
