//! **A path as the system's own object, and the one verb that moves one to the
//! trash** — the macOS twin of `windows_impl`'s `recycle` (ticket M2-2).
//!
//! # Why the two live together
//!
//! Not because they are the same subject. Because they are the same *sentence*:
//! **a path on this platform is bytes, and every crossing out of this process
//! has to keep them.** `NSURL` is where that crossing happens — the trash takes
//! one, and so does every verb in `handoff::macos_handoff` — and a second
//! conversion written beside the first is a second chance to write
//! `to_string_lossy` and hand the system a different file. So the conversion is
//! here, once, and the trash is here because it is the other thing this file's
//! rule is about.
//!
//! # The thread
//!
//! `NSFileManager` is documented thread-safe for the file operations —
//! *Threading Programming Guide*'s file-manager note and the class reference's
//! own "you can use the shared instance from multiple threads" — and
//! `defaultManager` is the shared instance. Nothing here is AppKit, nothing
//! here owns a view, and the `main`-thread discipline `macos_impl` is built
//! around does not apply; see `handoff::macos_handoff`'s header for the same
//! statement made about `NSWorkspace` and for the rule it is read under.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2_foundation::{NSFileManager, NSURL};

/// **One `NSURL` for one path, built out of the path's bytes.**
///
/// `fileURLWithPath:` takes an `NSString`, and going through one would mean
/// going through `to_string_lossy` — the round trip M1-1 wrote
/// `argument_after_ascii` to avoid. A name that is not UTF-8 comes back as a
/// *different name*, with `U+FFFD` where its bytes were, and the file that then
/// opens (or is deleted) is not the file the reader pointed at. APFS normalises
/// to UTF-8 and most such names cannot exist on a boot volume, but a mounted
/// SMB or exFAT share is a volume where they can, and a files column can be
/// rooted on one.
///
/// `fileURLWithFileSystemRepresentation:isDirectory:relativeToURL:` is the API
/// that exists for exactly this: it takes the path in the file system's own
/// encoding, which is what `OsStr::as_bytes` already holds.
///
/// `is_directory` is the caller's `stat`, not a guess. It decides only whether
/// the URL wears a trailing slash, and both the workspace and the file manager
/// resolve the real object underneath — but a caller that has already asked the
/// disk may as well say what it heard.
pub(crate) fn file_url(path: &Path, is_directory: bool) -> Result<Retained<NSURL>, String> {
    let bytes = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "path contains an embedded NUL".to_owned())?;
    let representation = NonNull::new(bytes.as_ptr().cast_mut())
        .ok_or_else(|| "path has no representation".to_owned())?;
    // SAFETY: `representation` points at `bytes`, a NUL-terminated C string
    // that outlives the call, which is the whole of what this initializer asks
    // of it. It copies what it is given and hands back an owned URL.
    Ok(unsafe {
        NSURL::fileURLWithFileSystemRepresentation_isDirectory_relativeToURL(
            representation,
            is_directory,
            None,
        )
    })
}

/// **Send one file to the Trash** (§7.1.6c-4d, and the `Delete` row of the
/// files column).
///
/// The promise is the product's and it does not change at the border: the
/// CHANGELOG says a deleted file goes somewhere it can be fetched back from,
/// so `remove_file` is not an implementation of this door on any platform.
/// `trashItemAtURL:resultingItemURL:error:` is the one call that keeps it —
/// the same move Finder's own Delete makes, into the same `~/.Trash`, with the
/// same Put Back.
///
/// # `Ok(false)` cannot happen here, and that is the difference worth writing
/// down
///
/// The Windows arm has three answers because Windows has three. `SHFileOperationW`
/// is given `FOF_ALLOWUNDO | FOF_WANTNUKEWARNING`, and that pairing exists for
/// one case: a file the Recycle Bin **cannot take** — too large for the bin, or
/// on a volume with no bin — raises the shell's own *this will be deleted
/// permanently* prompt, and a reader who answers no gets `Ok(false)`: nothing
/// happened, and it is not an error.
///
/// **macOS has no twin of that prompt and no twin of that answer.** Finder's
/// trash never asks and never offers to delete instead; a volume that cannot
/// hold a Trash — a network mount, a read-only image, a `macFUSE` volume —
/// makes `trashItemAtURL:` *fail*, with `NSFeatureUnsupportedError` in
/// `NSCocoaErrorDomain` (Apple's own note on the method: "Some macOS volumes
/// may not support a Trash folder or it may be disabled, so these methods will
/// report failure by returning NO or nil and an NSError with
/// NSFeatureUnsupportedError"). So this arm answers `Ok(true)` or `Err`, and
/// the `Err` carries the system's sentence to the toast the files column and
/// the scheme card already show for it.
///
/// **And it does not fall back to deleting the file anyway.** That is the
/// obvious second half of the Windows prompt and it is refused deliberately:
/// the prompt exists so that a *person* decides, and there is no person to ask
/// here — a `Delete` row that silently destroyed a file on a network share
/// because the share has no trash would be the product telling a sentence it
/// did not keep, which is the failure `FOF_WANTNUKEWARNING` was chosen to avoid
/// in the first place. The reader is told why, and the file is still there.
///
/// # A folder goes whole
///
/// One call naming the folder, exactly as the Windows arm makes one call: the
/// system moves the tree and puts it back the same way, and a walk that trashed
/// the children one at a time would leave a reader restoring a folder file by
/// file.
pub fn recycle(path: &Path) -> Result<bool, String> {
    let directory = std::fs::metadata(path)
        .map_err(|error| format!("{path:?}: {error}"))?
        .is_dir();
    let url = file_url(path, directory)?;
    let manager = NSFileManager::defaultManager();
    match manager.trashItemAtURL_resultingItemURL_error(&url, None) {
        Ok(()) => Ok(true),
        Err(error) => Err(format!(
            "{} ({} {})",
            error.localizedDescription(),
            error.domain(),
            error.code()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RED — **a file sent to the trash is in the trash**, which is the whole of
    /// what this door promises and the only claim `remove_file` could not also
    /// make.
    ///
    /// The check is the reader's own: the name is looked for in `~/.Trash`,
    /// where Finder's Put Back would find it. It is then removed from there, so
    /// that a machine running this suite a hundred times does not accumulate a
    /// hundred files somebody has to empty.
    ///
    /// **Renaming is expected and is not a failure.** The trash already
    /// holding a file of that name makes the system append a suffix, so the
    /// search is over names that *start with* the stem rather than over the
    /// exact name — which is also why the stem carries this process's id.
    ///
    /// MUTATION: swap the call for `std::fs::remove_file` and the file is gone
    /// from the disk and absent from the trash, which is the promise broken in
    /// the one direction nobody can undo.
    #[test]
    fn a_file_sent_to_the_trash_is_in_the_trash() {
        let root = std::env::temp_dir().join(format!("folio-trash-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("a scratch directory");
        let stem = format!("folio-m2-2-{}", std::process::id());
        let file = root.join(format!("{stem}.txt"));
        std::fs::write(&file, b"M2-2 trashed this.\n").expect("a file to throw away");

        assert_eq!(recycle(&file), Ok(true), "the system took the file");
        assert!(
            !file.exists(),
            "a file that was trashed is no longer where it was"
        );

        let trash =
            std::path::PathBuf::from(std::env::var_os("HOME").expect("an account")).join(".Trash");
        let found: Vec<std::path::PathBuf> = std::fs::read_dir(&trash)
            .expect("the account's trash")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(&stem))
            })
            .collect();
        assert!(
            !found.is_empty(),
            "{stem} is not in {trash:?}, so it was destroyed rather than trashed"
        );
        for path in found {
            let _ = std::fs::remove_file(&path);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED — **a path that is not there is a refusal with the system's words**,
    /// and the refusal comes before anything is moved.
    ///
    /// The door's `Result` is the caller's only account of what happened: the
    /// files column turns an `Err` into a toast naming the row. A door that
    /// answered `Ok(false)` for a missing file would be telling the column that
    /// a person declined a prompt that this platform never shows.
    #[test]
    fn a_path_that_is_not_there_is_refused_rather_than_answered_no() {
        let missing = std::env::temp_dir().join(format!("folio-gone-{}", std::process::id()));
        let refusal = recycle(&missing).expect_err("nothing to trash");
        assert!(
            !refusal.is_empty(),
            "a refusal carries the reason the system gave"
        );
    }
}
