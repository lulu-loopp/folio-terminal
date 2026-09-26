//! The exclusive-create door on real folders (0.4.6 ticket U-14).

use super::Directory;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "bt-platform-exclusive-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// A file symlink at `link` naming `target`, or why this machine will not
/// make one (Windows without Developer Mode or the symlink privilege).
fn file_link(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(target, link);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);
    made
}

fn dir_link(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(target, link);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);
    made
}

/// RED (U-14) — **a name that exists is never written through: an existing
/// file, directory, symlink or dangling symlink is refused as existing, and
/// what it names is untouched.**
///
/// F-12's "exclusive creation … never following an existing link": a staging
/// directory that somebody else could write into must not turn a member's
/// create into a write to wherever a link they left points. The create fails on
/// the name itself, so there is nothing to follow; this holds it on each kind
/// of entry a name can be. Windows refuses a symlink to an account without
/// Developer Mode, so there the two link cases say they were skipped and the
/// file and directory cases still run (CI's Windows runner has the privilege).
///
/// MUTATION: create with `FILE_OPEN_IF` / `O_CREAT` without `O_EXCL` and the
/// existing file is opened and written.
#[test]
fn an_existing_file_or_link_is_refused_and_untouched() {
    let root = scratch("existing");
    let staging = root.join("staging");
    std::fs::create_dir(&staging).unwrap();
    let outside = root.join("outside.txt");
    std::fs::write(&outside, b"not yours").unwrap();
    std::fs::write(staging.join("conpty.dll"), b"there first").unwrap();
    std::fs::create_dir(staging.join("folder")).unwrap();

    let directory = Directory::open(&staging).expect("a real directory opens");
    for name in ["conpty.dll", "folder"] {
        let error = directory.create_new(name).expect_err(name);
        assert_eq!(error.kind(), ErrorKind::AlreadyExists, "{name}: {error}");
    }
    assert_eq!(
        std::fs::read(staging.join("conpty.dll")).unwrap(),
        b"there first"
    );

    match file_link(&outside, &staging.join("uninstall.cmd")) {
        Ok(()) => {
            file_link(&root.join("nowhere.txt"), &staging.join("dangling.cmd")).unwrap();
            for name in ["uninstall.cmd", "dangling.cmd"] {
                let error = directory.create_new(name).expect_err(name);
                assert_eq!(error.kind(), ErrorKind::AlreadyExists, "{name}: {error}");
            }
            assert_eq!(std::fs::read(&outside).unwrap(), b"not yours");
            assert!(
                !root.join("nowhere.txt").exists(),
                "the dangling link made nothing"
            );
        }
        Err(error) => eprintln!(
            "SKIPPED the symlink half of an_existing_file_or_link_is_refused_and_untouched: \
             this account may not create a symlink ({error})"
        ),
    }

    let mut made = directory
        .create_new("folio.exe")
        .expect("a new name is created");
    made.write_all(b"new").unwrap();
    drop(made);
    assert_eq!(std::fs::read(staging.join("folio.exe")).unwrap(), b"new");
    if cfg!(windows) {
        let error = directory
            .create_new("FOLIO.EXE")
            .expect_err("Windows compares case");
        assert_eq!(error.kind(), ErrorKind::AlreadyExists, "{error}");
    }
    drop(directory);
    std::fs::remove_dir_all(&root).unwrap();
}

/// RED (U-14) — **the directory is opened as itself, never through a link,
/// and every create lands in the directory that was opened.**
///
/// A link standing where the staging directory should be is refused at the
/// open. Once open, the directory is the handle: on Windows it cannot be
/// renamed away while held (the handle does not share delete), and on Unix,
/// where it can, a create after the rename still lands in the directory that
/// was opened — never in whatever is now at the old path.
///
/// MUTATION: create by `path().join(name)` instead of relative to the handle,
/// and on Unix the file lands in the new directory at the old path.
#[test]
fn a_create_lands_in_the_directory_that_was_opened() {
    let root = scratch("held");
    let staging = root.join("staging");
    std::fs::create_dir(&staging).unwrap();

    let file = root.join("file.txt");
    std::fs::write(&file, b"").unwrap();
    let error = Directory::open(&file).expect_err("a file is not a directory");
    assert!(
        matches!(
            error.kind(),
            ErrorKind::InvalidInput | ErrorKind::NotADirectory
        ),
        "{error}"
    );
    match dir_link(&staging, &root.join("linked")) {
        Ok(()) => {
            let error = Directory::open(&root.join("linked")).expect_err("a link is refused");
            assert_eq!(error.kind(), ErrorKind::InvalidInput, "{error}");
        }
        Err(error) => eprintln!(
            "SKIPPED the link half of a_create_lands_in_the_directory_that_was_opened: \
             this account may not create a symlink ({error})"
        ),
    }

    let directory = Directory::open(&staging).unwrap();
    let moved = root.join("moved");
    let renamed = std::fs::rename(&staging, &moved);
    if cfg!(windows) {
        assert!(renamed.is_err(), "a held directory cannot be renamed away");
        drop(directory.create_new("a.txt").unwrap());
        assert!(staging.join("a.txt").exists());
    } else {
        renamed.expect("Unix renames an open directory");
        std::fs::create_dir(&staging).unwrap();
        drop(directory.create_new("a.txt").unwrap());
        assert!(
            moved.join("a.txt").exists(),
            "in the directory that was opened"
        );
        assert!(
            !staging.join("a.txt").exists(),
            "not in the one now at its path"
        );
    }
    drop(directory);
    std::fs::remove_dir_all(&root).unwrap();
}

/// RED (U-14) — **a rename inside the held directory never replaces a name,
/// and moves the file whole when the name is free.**
///
/// The archive reader writes each member under a temporary name and gives it
/// its final one only when every member has been checked; the final name must
/// not be taken from something already there.
///
/// MUTATION: rename with `ReplaceIfExists = true` / `renameat` and the
/// existing `conpty.dll` is replaced.
#[test]
fn a_rename_never_replaces_an_existing_name() {
    let root = scratch("rename");
    let directory = Directory::open(&root).unwrap();
    let mut temporary = directory.create_new("~conpty.dll").unwrap();
    temporary.write_all(b"checked").unwrap();
    drop(temporary);
    std::fs::write(root.join("taken.dll"), b"there first").unwrap();

    let error = directory
        .rename_new("~conpty.dll", "taken.dll")
        .expect_err("an existing name is not replaced");
    assert_eq!(error.kind(), ErrorKind::AlreadyExists, "{error}");
    assert_eq!(
        std::fs::read(root.join("taken.dll")).unwrap(),
        b"there first"
    );

    directory.rename_new("~conpty.dll", "conpty.dll").unwrap();
    assert!(!root.join("~conpty.dll").exists());
    assert_eq!(std::fs::read(root.join("conpty.dll")).unwrap(), b"checked");
    drop(directory);
    std::fs::remove_dir_all(&root).unwrap();
}

/// PIN (U-14) — **a name is one component**: the door writes in the directory
/// it holds and nowhere else, and a stream name is not a file name.
#[test]
fn a_name_that_is_not_one_component_is_refused() {
    let root = scratch("component");
    let directory = Directory::open(&root).unwrap();
    for name in ["", ".", "..", "a/b", "a\\b", "..\\x", "a:b", "a\0b"] {
        let error = directory.create_new(name).expect_err(name);
        assert_eq!(error.kind(), ErrorKind::InvalidInput, "{name:?}: {error}");
    }
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    drop(directory);
    std::fs::remove_dir_all(&root).unwrap();
}
