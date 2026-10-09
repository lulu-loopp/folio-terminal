//! **The one owner of a test's scratch name.**
//!
//! A test that needs a file or a directory of its own names it here, and
//! nowhere else: `{tag}-{process id}-{ordinal}`, where the ordinal is one
//! process-wide counter that every call advances.
//!
//! Two things have to be true of such a name, and each part of it carries one:
//!
//! * **No other process's test has it.** Two `cargo test` runs at once — two
//!   worktrees, CI's parallel jobs, a lane running beside a developer — share
//!   the system's temporary directory. The process id is what keeps them apart.
//! * **No other call in this process has it.** A test binary runs its tests on
//!   many threads, and a helper is called by several of them at once. The
//!   ordinal is what keeps those apart: a counter is advanced once per call,
//!   so two calls cannot read the same value however close together they run.
//!
//! The wall clock is in neither part, and that is the point of this crate.
//! The shape it replaces named a path by the process id and the nanoseconds
//! since the epoch, which reads as unique and is not: two threads that sample
//! the clock inside one tick of its resolution get the same number, and the
//! second test's `remove_file` then finds the first test already removed it
//! (`shell_integration_script.rs`, 2026-10-05). `bt-source`'s
//! `temp_paths` guard refuses that shape anywhere in test code.
//!
//! The tag is the test's own word for what the path is for. It only has to be
//! readable in a listing of the temporary directory; uniqueness never rests on
//! it.

use std::path::{Component, PathBuf, Prefix};
use std::sync::atomic::{AtomicU64, Ordering};

/// The process-wide ordinal. Every name this crate hands out takes the next
/// value.
static NEXT: AtomicU64 = AtomicU64::new(0);

/// A name no other call in any running process has been given:
/// `{tag}-{process id}-{ordinal}`.
///
/// For a scratch path under a root of the test's own choosing (a link-free
/// temporary directory, a directory another fixture already made, a registry
/// key) — `root.join(bt_testpath::unique_name("probe"))`.
#[must_use]
pub fn unique_name(tag: &str) -> String {
    let ordinal = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("{tag}-{}-{ordinal}", std::process::id())
}

/// [`unique_name`] under the system's temporary directory.
///
/// Nothing is created: the caller makes the file or the directory it wants
/// there, which is also how a test that needs the path to be *absent* uses it.
#[must_use]
pub fn temp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(unique_name(tag))
}

/// **The system's temporary directory with every link above it resolved, in its ordinary
/// spelling.**
///
/// For a test whose subject refuses a path with a link among its ancestors — the uninstall
/// door's removal roots, the shell profile writer — and so must stand in a directory whose own
/// spelling carries none, or every fixture reads as a planted link. On macOS the system's own
/// spelling does: `$TMPDIR` is `/var/folders/…`, and `/var` is the system's link to
/// `/private/var`. `canonicalize` is the resolution; on Windows it answers the verbatim `\\?\`
/// form, where `/` is not a separator and a fixture's `root.join("app/folio.exe")` would name no
/// file, so a verbatim drive or share prefix is spelled back the ordinary way. A path with no
/// prefix (every Unix path) is the canonical answer itself.
///
/// # Panics
///
/// When the temporary directory does not exist, which no test can stand in.
#[must_use]
pub fn link_free_temp_dir() -> PathBuf {
    let real = std::fs::canonicalize(std::env::temp_dir()).expect("the temporary directory exists");
    let mut components = real.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return real;
    };
    let head = match prefix.kind() {
        Prefix::VerbatimDisk(letter) => format!("{}:\\", char::from(letter)),
        Prefix::VerbatimUNC(server, share) => format!(
            r"\\{}\{}\",
            server.to_string_lossy(),
            share.to_string_lossy()
        ),
        _ => return real,
    };
    let rest: PathBuf = components
        .filter(|component| !matches!(component, Component::RootDir))
        .collect();
    PathBuf::from(head).join(rest)
}

/// [`unique_name`] under [`link_free_temp_dir`]. Nothing is created.
#[must_use]
pub fn link_free_temp_path(tag: &str) -> PathBuf {
    link_free_temp_dir().join(unique_name(tag))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RED — **two calls with the same tag, on threads racing each other, are
    /// two names.**
    ///
    /// This is the case the pid-and-clock shape lost: the same helper, entered
    /// by many tests at once. Sixteen threads take sixty-four names each from
    /// one tag with a space and CJK in it, released together by a barrier so
    /// they really do overlap, and every one of the 1024 names is different.
    ///
    /// MUTATION: format the name without `{ordinal}` and every thread's names
    /// are one name — red at the first duplicate.
    #[test]
    fn racing_calls_with_one_tag_never_share_a_name() {
        const THREADS: usize = 16;
        const EACH: usize = 64;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(THREADS));
        let workers: Vec<_> = (0..THREADS)
            .map(|_| {
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    (0..EACH)
                        .map(|_| unique_name("folio 图 片"))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let mut seen = std::collections::BTreeSet::new();
        for worker in workers {
            for name in worker.join().expect("a worker that only formats") {
                assert!(seen.insert(name.clone()), "{name} was handed out twice");
            }
        }
        assert_eq!(seen.len(), THREADS * EACH);
    }

    /// RED — **the name says whose it is: the tag first, then this process.**
    ///
    /// The process id is what keeps two test processes apart, so it has to be
    /// in the name; the tag has to lead it so a listing of the temporary
    /// directory reads as what each entry is for.
    ///
    /// MUTATION: drop `std::process::id()` from the format and the second
    /// assertion is red; put the ordinal before the tag and the first is.
    #[test]
    fn a_name_is_the_tag_then_this_process() {
        let name = unique_name("渲染-probe");
        assert!(name.starts_with("渲染-probe-"), "{name}");
        let rest = &name["渲染-probe-".len()..];
        let (pid, ordinal) = rest.split_once('-').expect("pid-ordinal");
        assert_eq!(pid, std::process::id().to_string(), "{name}");
        assert!(ordinal.parse::<u64>().is_ok(), "{name}");
        assert_eq!(
            temp_path("渲染-probe").parent(),
            Some(std::env::temp_dir().as_path())
        );
    }

    /// RED — **a link-free scratch path has no link above it, and is spelled the ordinary way.**
    ///
    /// Every ancestor of a fresh directory under [`link_free_temp_path`] is asked about itself
    /// (`symlink_metadata` never follows), and the path must be the one that directory reads back
    /// as its canonical name — with no verbatim `\\?\` head, which a fixture's `join("a/b")` cannot
    /// cross.
    ///
    /// MUTATION: return `std::env::temp_dir()` from `link_free_temp_dir` and the macOS run is red
    /// at `/var`; drop the verbatim respelling and the Windows run is red at the prefix.
    #[test]
    fn a_link_free_scratch_path_has_no_link_above_it() {
        let directory = link_free_temp_path("無鏈 probe");
        std::fs::create_dir(&directory).expect("a fresh scratch directory");
        let linked: Vec<_> = directory
            .ancestors()
            .filter(|ancestor| {
                std::fs::symlink_metadata(ancestor)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
            })
            .map(std::path::Path::to_path_buf)
            .collect();
        let verbatim = matches!(
            directory.components().next(),
            Some(Component::Prefix(prefix)) if prefix.kind().is_verbatim()
        );
        std::fs::remove_dir(&directory).expect("the scratch directory goes");
        assert!(linked.is_empty(), "links above {directory:?}: {linked:?}");
        assert!(!verbatim, "{directory:?} is spelled verbatim");
        assert_eq!(directory.parent(), Some(link_free_temp_dir().as_path()));
    }
}
