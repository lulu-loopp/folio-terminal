//! **The exchange door: a document's new bytes take its name, and the bytes
//! they displaced stay on disk under another name** (T-EXCHANGE-DOOR; design
//! note `docs/plans/design/settings-reread-2026-10-11.md`, revisions (b)1,
//! (b)2 and (c)1).
//!
//! [`replace_keeping_displaced`] is the exchange step of the storage worker's
//! conditional write. The caller has read the target and found the bytes it
//! expected, and has written and flushed a temporary sibling. The door puts
//! the temporary file under the target's name and answers where the bytes the
//! target held now are ([`Displaced`]). It never deletes them: whether they
//! are the bytes the caller expected, and when they may go, are the caller's
//! questions. After a successful exchange the target holds the caller's bytes.
//!
//! **The contract, per platform.**
//!
//! * **The call.** Windows: `ReplaceFileW(target, temp, displaced)`. macOS:
//!   `renamex_np(temp, target, RENAME_SWAP)`. Linux:
//!   `renameat2(temp, target, RENAME_EXCHANGE)`. Every other platform has no
//!   exchange and refuses.
//! * **Metadata.** Windows: the call merges the DACL, the alternate data
//!   streams, the creation time, compression and encryption into the new file;
//!   the attribute word (hidden, system, archive) is staged on the temporary
//!   file first, and read-only is withheld until after the call
//!   (`file_replace`'s ladder). macOS and Linux: an exchange trades directory
//!   entries only, so the temporary file first takes the target's owner, mode,
//!   access-control list and extended attributes ([`crate::carry_metadata`])
//!   and is flushed again (`F_FULLFSYNC` on macOS, `fsync` on Linux).
//! * **The displaced name.** Windows: a fresh sibling
//!   `<name>.displaced-<pid>-<nanos>-<n>` ([`crate::sibling_path`]). The name
//!   is asked of the filesystem first, and a name that is taken is refused
//!   ([`ExchangeRefusal::BackupNameTaken`]) and never passed to the call:
//!   Microsoft does not specify what the call does with one. macOS and Linux:
//!   no name is made; after the exchange the temporary file's name holds the
//!   displaced file.
//! * **Partial failure.** Windows: the call's partial states go through the
//!   one ladder `file_replace` owns. `ERROR_UNABLE_TO_MOVE_REPLACEMENT_2`
//!   (1177) renames the displaced copy back under the target's name; 1175 and
//!   1176, and every other code, are classified by asking the filesystem
//!   whether the target is still there and the copy was never made, before
//!   anything is cleaned. A refusal whose restore fails says where the bytes
//!   are. macOS and Linux: the call is one operation, and it happened or it
//!   did not.
//! * **Links, every platform.** A target that is a symbolic link
//!   (`symlink_metadata`) or has more than one name
//!   ([`crate::file_link_count`]) is refused with [`ExchangeRefusal::Link`]
//!   before anything is written or called.
//! * **No exchange.** Where the volume or the platform cannot exchange, the
//!   answer is [`ExchangeRefusal::ExchangeUnavailable`] and the target is not
//!   touched: Windows, the codes `crate::volume_cannot_replace` names, with the
//!   filesystem confirming nothing changed; macOS, `ENOTSUP` or `EINVAL`, or a
//!   volume that says it cannot swap (`VOL_CAP_INT_RENAME_SWAP`, the
//!   capability `volumeSupportsSwapRenaming` reads), asked before the call;
//!   Linux, `EINVAL`, `ENOSYS` or `EXDEV`. **There is no plain rename in this
//!   door**: a rename would replace a file the caller has not read.
//! * **Durability.** `REPLACEFILE_WRITE_THROUGH` is documented as unsupported,
//!   and a swap is a directory change, so after the call the target's
//!   directory is flushed ([`flush_directory`]: `FlushFileBuffers` on a handle
//!   opened with `FILE_FLAG_BACKUP_SEMANTICS` on Windows, `F_FULLFSYNC` on the
//!   directory's descriptor on macOS — both `install_txn`'s step — and `fsync`
//!   of the directory on Linux), and the temporary file's directory too when
//!   it is another one. `Ok` means: the temporary file flushed, exchanged, and
//!   the directory flushed. A flush that fails after the exchange is
//!   [`ExchangeRefusal::NotDurable`], which still says where the displaced
//!   bytes are.
//! * **Worker only.** Every door here takes a [`WorkerCtx`], which no window
//!   thread holds: each blocks on the disk.
//!
//! **The first write** (no file under the name yet) is not an exchange:
//! [`rename_without_replacing_durably`] is [`crate::rename_without_replacing`]
//! followed by the same directory flush. It needs no link check: the rename
//! refuses any entry already under the name, a link included, so there is no
//! link it could replace.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::admission::WorkerCtx;
use crate::install_txn::directory_of;

/// **Where the bytes the target held before the exchange now are.** The file
/// is on disk and is the caller's: the door never removes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Displaced {
    pub path: PathBuf,
}

/// Why [`replace_keeping_displaced`] did not answer with a durable exchange.
#[derive(Debug)]
pub enum ExchangeRefusal {
    /// The target is a symbolic link or has more than one name. Nothing was
    /// written and nothing was called.
    Link,
    /// The volume or the platform has no atomic exchange. The target was not
    /// touched.
    ExchangeUnavailable(io::Error),
    /// Windows: the fresh name for the displaced copy is already taken. The
    /// call was not made, and the file under that name was not touched.
    BackupNameTaken(PathBuf),
    /// The exchange happened — the target holds the new bytes and the
    /// displaced bytes are where `displaced` says — but a directory flush
    /// after it failed, so the exchange is not known to be on the device.
    NotDurable {
        displaced: Displaced,
        error: io::Error,
    },
    /// Anything else: the error is the operating system's, and says where the
    /// target's bytes are when the call left them anywhere but under the
    /// target's name.
    Failed(io::Error),
}

/// **Put `temp` under `target`'s name, and keep what `target` held.**
///
/// `temp` is the caller's: written and flushed beside `target`. On `Ok`, the
/// target holds `temp`'s bytes and [`Displaced`] names the file that holds the
/// bytes the target held; on a refusal, the temporary file is still the
/// caller's to remove. See the module's contract for each platform.
///
/// # Errors
/// An [`ExchangeRefusal`].
pub fn replace_keeping_displaced(
    worker: &WorkerCtx,
    temp: &Path,
    target: &Path,
) -> Result<Displaced, ExchangeRefusal> {
    refuse_a_link(target)?;
    // An exchange trades directory entries only: what the target's object
    // carries goes onto the new object first, and the new object is flushed
    // again because the carry changed it.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        crate::carry_metadata(temp, target);
        let staged = fs::File::open(temp).map_err(ExchangeRefusal::Failed)?;
        staged.sync_all().map_err(ExchangeRefusal::Failed)?;
    }
    let displaced = displaced_name(temp, target).map_err(ExchangeRefusal::Failed)?;
    exchange_with(&OnDisk { worker }, temp, target, &displaced)
}

/// **Flush `directory`'s entries to the device**: `FlushFileBuffers` on a
/// handle opened with `FILE_FLAG_BACKUP_SEMANTICS` on Windows and
/// `F_FULLFSYNC` on the directory's descriptor on macOS (both
/// `install_txn`'s step), `fsync` of the directory elsewhere.
///
/// # Errors
/// The operating system's error for the open or the flush.
pub fn flush_directory(_worker: &WorkerCtx, directory: &Path) -> io::Result<()> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        crate::install_txn::flush_directory_on_disk(directory).map_err(|failure| failure.error)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let handle = fs::File::open(directory)?;
        handle.sync_all()
    }
}

/// **The first write**: [`crate::rename_without_replacing`], then the
/// destination's directory flushed, and the source's when it is another one.
/// An entry already under `to` is refused with `AlreadyExists` and nothing is
/// flushed.
///
/// # Errors
/// The rename's error, or a flush's; after a flush's error the rename has
/// happened.
pub fn rename_without_replacing_durably(
    worker: &WorkerCtx,
    from: &Path,
    to: &Path,
) -> io::Result<()> {
    crate::rename_without_replacing(from, to)?;
    let destination = directory_of(to);
    flush_directory(worker, destination)?;
    let source = directory_of(from);
    if source != destination {
        flush_directory(worker, source)?;
    }
    Ok(())
}

/// A symbolic link at the target, or a target with a second name, is refused
/// before anything is written: an exchange would put the new bytes under this
/// one name and leave every other name with the old ones.
fn refuse_a_link(target: &Path) -> Result<(), ExchangeRefusal> {
    let entry = fs::symlink_metadata(target).map_err(ExchangeRefusal::Failed)?;
    if entry.is_symlink() {
        return Err(ExchangeRefusal::Link);
    }
    let file = fs::File::open(target).map_err(ExchangeRefusal::Failed)?;
    let names = crate::file_link_count(&file).map_err(ExchangeRefusal::Failed)?;
    if names > 1 {
        return Err(ExchangeRefusal::Link);
    }
    Ok(())
}

/// The name the displaced bytes take: a fresh sibling of the target where the
/// call moves the target's file aside (Windows), the temporary file's own name
/// where the call trades the two entries.
fn displaced_name(temp: &Path, target: &Path) -> io::Result<PathBuf> {
    #[cfg(windows)]
    {
        let _ = temp;
        crate::sibling_path(target, "displaced")
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        Ok(temp.to_path_buf())
    }
}

/// The calls that touch the disk, so every branch of [`exchange_with`] is
/// reached by a test with operations that answer as a volume would.
trait Surface {
    /// The platform's exchange. Windows: `ReplaceFileW(target, temp,
    /// displaced)`. macOS and Linux: the swap of `temp` and `target`, after
    /// which `displaced` (the temporary file's own name) holds the old bytes.
    fn exchange(&self, temp: &Path, target: &Path, displaced: &Path) -> io::Result<()>;
    /// The ladder's restore after 1177 (Windows).
    #[cfg(windows)]
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// The ladder's second restore, for a volume that refuses the rename
    /// (Windows).
    #[cfg(windows)]
    fn link(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// Whether the volume `directory` is on says it can swap two names
    /// (macOS); `true` on Linux, where the call itself is the only answer.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn volume_exchanges(&self, directory: &Path) -> io::Result<bool>;
    fn flush_directory(&self, directory: &Path) -> io::Result<()>;
}

/// The exchange, then the directory flush: the target's directory, and the
/// temporary file's when it is another one. A refusal flushes nothing.
fn exchange_with(
    surface: &impl Surface,
    temp: &Path,
    target: &Path,
    displaced: &Path,
) -> Result<Displaced, ExchangeRefusal> {
    exchange_on(surface, temp, target, displaced)?;
    let displaced = Displaced {
        path: displaced.to_path_buf(),
    };
    let first = directory_of(target);
    let second = directory_of(temp);
    let flushed = surface.flush_directory(first).and_then(|()| {
        if second == first {
            Ok(())
        } else {
            surface.flush_directory(second)
        }
    });
    match flushed {
        Ok(()) => Ok(displaced),
        Err(error) => Err(ExchangeRefusal::NotDurable { displaced, error }),
    }
}

/// Windows: the name for the displaced copy is asked absent, then
/// `ReplaceFileW` and its ladder, with the copy kept.
#[cfg(windows)]
fn exchange_on(
    surface: &impl Surface,
    temp: &Path,
    target: &Path,
    displaced: &Path,
) -> Result<(), ExchangeRefusal> {
    match fs::symlink_metadata(displaced) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Ok(_) => return Err(ExchangeRefusal::BackupNameTaken(displaced.to_path_buf())),
        Err(error) => return Err(ExchangeRefusal::Failed(error)),
    }
    crate::file_replace::replace_keeping_backup_with(
        temp,
        target,
        displaced,
        |temp, target, displaced| surface.exchange(temp, target, displaced),
        |from, to| surface.rename(from, to),
        |from, to| surface.link(from, to),
    )
    .map_err(|refusal| match refusal {
        crate::ReplaceRefusal::VolumeCannot(error) => ExchangeRefusal::ExchangeUnavailable(error),
        crate::ReplaceRefusal::Refused(error) => ExchangeRefusal::Failed(error),
    })
}

/// macOS and Linux: the volume asked, then the one swap.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn exchange_on(
    surface: &impl Surface,
    temp: &Path,
    target: &Path,
    displaced: &Path,
) -> Result<(), ExchangeRefusal> {
    if !surface
        .volume_exchanges(directory_of(target))
        .map_err(ExchangeRefusal::Failed)?
    {
        return Err(ExchangeRefusal::ExchangeUnavailable(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "the volume holding {} cannot exchange two names",
                target.display()
            ),
        )));
    }
    surface.exchange(temp, target, displaced).map_err(|error| {
        if swap_unavailable(error.raw_os_error()) {
            ExchangeRefusal::ExchangeUnavailable(error)
        } else {
            ExchangeRefusal::Failed(error)
        }
    })
}

/// Every other platform: no exchange, so every write is refused.
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn exchange_on(
    surface: &impl Surface,
    temp: &Path,
    target: &Path,
    displaced: &Path,
) -> Result<(), ExchangeRefusal> {
    let _ = (surface, temp, target, displaced);
    Err(ExchangeRefusal::ExchangeUnavailable(io::Error::new(
        io::ErrorKind::Unsupported,
        "replace_keeping_displaced has no exchange on this platform",
    )))
}

/// The `errno` values that mean *this volume cannot swap two names*, as
/// against *this swap did not work*: `renamex_np(2)`'s `ENOTSUP` for a
/// filesystem without `RENAME_SWAP`, and `EINVAL` for one that rejects the
/// flag.
#[cfg(target_os = "macos")]
fn swap_unavailable(code: Option<i32>) -> bool {
    matches!(code, Some(libc::ENOTSUP | libc::EINVAL))
}

/// The `errno` values that mean *no exchange here*: `renameat2(2)`'s `EINVAL`
/// for a filesystem without `RENAME_EXCHANGE`, `ENOSYS` for a kernel without
/// the call, and `EXDEV` for two names on two mounts.
#[cfg(target_os = "linux")]
fn swap_unavailable(code: Option<i32>) -> bool {
    matches!(code, Some(libc::EINVAL | libc::ENOSYS | libc::EXDEV))
}

/// The operating system's calls, with the worker's capability for the flush.
struct OnDisk<'a> {
    worker: &'a WorkerCtx,
}

impl Surface for OnDisk<'_> {
    fn exchange(&self, temp: &Path, target: &Path, displaced: &Path) -> io::Result<()> {
        #[cfg(windows)]
        {
            crate::file_replace::replace_file_w(temp, target, displaced)
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let _ = displaced;
            swap(temp, target)
        }
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        {
            let _ = (temp, target, displaced);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "replace_keeping_displaced has no exchange on this platform",
            ))
        }
    }

    #[cfg(windows)]
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        crate::file_replace::rename_path(from, to)
    }

    #[cfg(windows)]
    fn link(&self, from: &Path, to: &Path) -> io::Result<()> {
        crate::file_replace::link_path(from, to)
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn volume_exchanges(&self, directory: &Path) -> io::Result<bool> {
        #[cfg(target_os = "macos")]
        {
            volume_swaps(directory)
        }
        #[cfg(target_os = "linux")]
        {
            let _ = directory;
            Ok(true)
        }
    }

    fn flush_directory(&self, directory: &Path) -> io::Result<()> {
        flush_directory(self.worker, directory)
    }
}

/// The two names traded in one call: `renamex_np(RENAME_SWAP)` on macOS,
/// `renameat2(RENAME_EXCHANGE)` on Linux.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn swap(temp: &Path, target: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;

    let c_path = |path: &Path| {
        std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in a path"))
    };
    let (temp, target) = (c_path(temp)?, c_path(target)?);
    // SAFETY: both strings are NUL-terminated and live across the call.
    #[cfg(target_os = "macos")]
    let swapped = unsafe { libc::renamex_np(temp.as_ptr(), target.as_ptr(), libc::RENAME_SWAP) };
    // SAFETY: as above; `AT_FDCWD` resolves both paths the way `rename` does.
    #[cfg(target_os = "linux")]
    let swapped = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            temp.as_ptr(),
            libc::AT_FDCWD,
            target.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    if swapped == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// **Whether the volume `directory` is on can swap two names** — the
/// capability Foundation's `volumeSupportsSwapRenaming` reads:
/// `VOL_CAP_INT_RENAME_SWAP`, valid and set, in the interfaces word of the
/// volume's `ATTR_VOL_CAPABILITIES`. Volume attributes are asked of the
/// volume's root, which `statfs` names.
#[cfg(target_os = "macos")]
fn volume_swaps(directory: &Path) -> io::Result<bool> {
    use std::os::unix::ffi::OsStrExt;

    /// `getattrlist`'s answer for `ATTR_VOL_INFO | ATTR_VOL_CAPABILITIES`:
    /// the length, then the one attribute asked for.
    #[repr(C)]
    struct Answer {
        length: u32,
        capabilities: libc::vol_capabilities_attr_t,
    }

    let directory = std::ffi::CString::new(directory.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in a path"))?;
    // SAFETY: `statfs` is plain old data; the call fills it whole or fails.
    let mut volume: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: a live NUL-terminated path and an exclusively borrowed buffer.
    if unsafe { libc::statfs(directory.as_ptr(), &raw mut volume) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `f_mntonname` is the kernel's NUL-terminated mount point.
    let root = unsafe { std::ffi::CStr::from_ptr(volume.f_mntonname.as_ptr()) };
    let mut request = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: 0,
        volattr: libc::ATTR_VOL_INFO | libc::ATTR_VOL_CAPABILITIES,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    // SAFETY: `Answer` is plain old data the call fills.
    let mut answer: Answer = unsafe { std::mem::zeroed() };
    // SAFETY: a live NUL-terminated path, the request and the answer
    // exclusively borrowed, and the answer's true size beside it.
    let asked = unsafe {
        libc::getattrlist(
            root.as_ptr(),
            (&raw mut request).cast(),
            (&raw mut answer).cast(),
            size_of::<Answer>(),
            0,
        )
    };
    if asked != 0 {
        return Err(io::Error::last_os_error());
    }
    let interfaces = libc::VOL_CAPABILITIES_INTERFACES;
    let swap = libc::VOL_CAP_INT_RENAME_SWAP;
    Ok(answer.capabilities.valid[interfaces] & swap != 0
        && answer.capabilities.capabilities[interfaces] & swap != 0)
}

#[cfg(test)]
mod exchange_tests {
    use super::*;
    use std::cell::RefCell;

    fn temp_root(tag: &str) -> PathBuf {
        let root = bt_testpath::temp_path(&format!("bt-exchange-{tag}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// `body` on a worker the thread door started, as the storage worker is.
    fn on_a_worker<T: Send + 'static>(body: impl FnOnce(&WorkerCtx) -> T + Send + 'static) -> T {
        crate::spawn_at_priority("bt-exchange-test", crate::ThreadPriority::Normal, body)
            .expect("start a worker for the exchange")
            .join()
            .expect("the worker answered")
    }

    /// The real door, on a worker.
    fn exchange(temp: &Path, target: &Path) -> Result<Displaced, ExchangeRefusal> {
        let (temp, target) = (temp.to_path_buf(), target.to_path_buf());
        on_a_worker(move |worker| replace_keeping_displaced(worker, &temp, &target))
    }

    /// What a test's volume does when the exchange is asked of it.
    type Answer<'a> = Box<dyn Fn(&Path, &Path, &Path) -> io::Result<()> + 'a>;

    /// Operations that answer as a volume would, and a record of what was
    /// asked of them, in order.
    struct Recording<'a> {
        exchange: Answer<'a>,
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        volume_exchanges: bool,
        asked: RefCell<Vec<String>>,
    }

    impl<'a> Recording<'a> {
        fn new(exchange: impl Fn(&Path, &Path, &Path) -> io::Result<()> + 'a) -> Self {
            Self {
                exchange: Box::new(exchange),
                #[cfg(any(target_os = "macos", target_os = "linux"))]
                volume_exchanges: true,
                asked: RefCell::new(Vec::new()),
            }
        }

        fn asked(&self) -> Vec<String> {
            self.asked.borrow().clone()
        }
    }

    impl Surface for Recording<'_> {
        fn exchange(&self, temp: &Path, target: &Path, displaced: &Path) -> io::Result<()> {
            self.asked.borrow_mut().push("exchange".to_owned());
            (self.exchange)(temp, target, displaced)
        }

        #[cfg(windows)]
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.asked.borrow_mut().push("rename".to_owned());
            fs::rename(from, to)
        }

        #[cfg(windows)]
        fn link(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.asked.borrow_mut().push("link".to_owned());
            fs::hard_link(from, to)
        }

        #[cfg(any(target_os = "macos", target_os = "linux"))]
        fn volume_exchanges(&self, _directory: &Path) -> io::Result<bool> {
            Ok(self.volume_exchanges)
        }

        fn flush_directory(&self, directory: &Path) -> io::Result<()> {
            self.asked
                .borrow_mut()
                .push(format!("flush {}", directory.display()));
            Ok(())
        }
    }

    /// The code this platform's exchange gives for *this volume has no such
    /// operation*.
    fn an_unavailable_answer() -> io::Error {
        #[cfg(windows)]
        {
            io::Error::from_raw_os_error(50)
        }
        #[cfg(unix)]
        {
            io::Error::from_raw_os_error(libc::EINVAL)
        }
    }

    /// The name the displaced bytes would take, for a seam test.
    fn displaced_for(temp: &Path, target: &Path) -> PathBuf {
        displaced_name(temp, target).unwrap()
    }

    /// RED ((b)14-1) — **where the exchange is unavailable, the write is
    /// refused and the target is not touched.** Revision (a)'s answer was a
    /// plain rename, which reopens the race between the read that found the
    /// expected bytes and the rename that writes over whatever is there now.
    ///
    /// The volume answers its "no such operation" code and changes nothing:
    /// the door says `ExchangeUnavailable`, the target holds its bytes, the
    /// temporary file still holds the caller's, and nothing is flushed. On
    /// macOS a volume that says it cannot swap is refused before the call.
    ///
    /// MUTATION: on `ExchangeUnavailable`, `fs::rename(temp, target)` (a plain
    /// rename fallback) and the target assertion goes red.
    #[test]
    fn an_unsupported_exchange_refuses_and_leaves_the_target() {
        let root = temp_root("unsupported");
        let target = root.join("settings.json");
        let temp = root.join("settings.json.tmp-1");
        fs::write(&target, b"{\"font\": \"as it was\"}").unwrap();
        fs::write(&temp, b"{\"font\": \"as it would be\"}").unwrap();
        let displaced = displaced_for(&temp, &target);
        let surface = Recording::new(|_, _, _| Err(an_unavailable_answer()));

        let refusal = exchange_with(&surface, &temp, &target, &displaced)
            .expect_err("the volume has no exchange");

        assert!(
            matches!(refusal, ExchangeRefusal::ExchangeUnavailable(_)),
            "a volume without the operation is refused as one: {refusal:?}"
        );
        assert_eq!(fs::read(&target).unwrap(), b"{\"font\": \"as it was\"}");
        assert_eq!(fs::read(&temp).unwrap(), b"{\"font\": \"as it would be\"}");
        assert_eq!(surface.asked(), ["exchange"], "and nothing is flushed");
        #[cfg(windows)]
        assert!(!displaced.exists(), "and no copy is left beside it");

        #[cfg(target_os = "macos")]
        {
            let mut cannot = Recording::new(|_, _, _| Ok(()));
            cannot.volume_exchanges = false;
            let refusal = exchange_with(&cannot, &temp, &target, &displaced)
                .expect_err("the volume says it cannot swap");
            assert!(matches!(refusal, ExchangeRefusal::ExchangeUnavailable(_)));
            assert!(cannot.asked().is_empty(), "the call is never made");
            assert_eq!(fs::read(&target).unwrap(), b"{\"font\": \"as it was\"}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    /// RED ((b)14-2, Linux) — **`renameat2(RENAME_EXCHANGE)` puts the new
    /// bytes under the target's name and hands the old ones back under the
    /// temporary file's.**
    ///
    /// MUTATION: `RENAME_NOREPLACE` in place of `RENAME_EXCHANGE`: the call
    /// answers `EEXIST`, the door `Failed`, and the `expect` goes red.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_rename_exchange_hands_back_the_displaced_file() {
        let root = temp_root("linux");
        let target = root.join("keybindings.json");
        let temp = root.join("keybindings.json.tmp-1");
        fs::write(&target, b"[\"theirs\"]").unwrap();
        fs::write(&temp, b"[\"ours\"]").unwrap();

        let displaced = exchange(&temp, &target).expect("ext4 exchanges two names");

        assert_eq!(
            displaced.path, temp,
            "the temporary name holds the old file"
        );
        assert_eq!(fs::read(&target).unwrap(), b"[\"ours\"]");
        assert_eq!(fs::read(&displaced.path).unwrap(), b"[\"theirs\"]");
        fs::remove_dir_all(root).unwrap();
    }

    /// What the call did to the two names before it answered.
    #[cfg(windows)]
    type PartialState = dyn Fn(&Path, &Path) -> io::Result<()>;

    /// RED ((b)14-3, Windows) — **every partial state of `ReplaceFileW` leaves
    /// the document under its own name**, driven through the injected
    /// operations.
    ///
    /// * 1175 (`ERROR_UNABLE_TO_REMOVE_REPLACED`) and 1176
    ///   (`ERROR_UNABLE_TO_MOVE_REPLACEMENT`): with a backup name given, both
    ///   names keep their files. 1176 is driven with a copy already made at
    ///   the backup name, which the ladder removes because the target is
    ///   still the document.
    /// * 1177 (`ERROR_UNABLE_TO_MOVE_REPLACEMENT_2`): the target's file has
    ///   moved to the backup name and the new one did not take its place; the
    ///   ladder renames it back.
    ///
    /// Each is `Failed`, flushes nothing, and leaves no copy beside the
    /// document.
    ///
    /// MUTATION: drop the 1177 restore (`restore_replaced`) and the third
    /// case finds no file under the target's name.
    #[cfg(windows)]
    #[test]
    fn replace_file_partial_states_1175_1176_1177_keep_the_document() {
        let root = temp_root("partial");
        let cases: [(i32, &PartialState); 3] = [
            (1175, &|_target: &Path, _backup: &Path| Ok(())),
            (1176, &|target: &Path, backup: &Path| {
                fs::copy(target, backup).map(|_| ())
            }),
            (1177, &|target: &Path, backup: &Path| {
                fs::rename(target, backup)
            }),
        ];
        for (code, partial) in cases {
            let target = root.join(format!("profiles-{code}.json"));
            let temp = root.join(format!("profiles-{code}.json.tmp-1"));
            fs::write(&target, b"the document").unwrap();
            fs::write(&temp, b"the replacement").unwrap();
            let displaced = displaced_for(&temp, &target);
            let surface = Recording::new(|_temp, target, backup| {
                partial(target, backup)?;
                Err(io::Error::from_raw_os_error(code))
            });

            let refusal = exchange_with(&surface, &temp, &target, &displaced)
                .expect_err("the call stopped part of the way");

            assert!(
                matches!(refusal, ExchangeRefusal::Failed(_)),
                "{code}: {refusal:?}"
            );
            assert_eq!(
                fs::read(&target).unwrap(),
                b"the document",
                "{code}: the document is under its own name"
            );
            assert!(!displaced.exists(), "{code}: with no copy beside it");
            assert!(
                surface
                    .asked()
                    .iter()
                    .all(|asked| !asked.starts_with("flush")),
                "{code}: a refusal flushes nothing"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    /// RED ((b)14-3, Windows) — **a backup name that is taken is refused, and
    /// never passed to `ReplaceFileW`**, whose behaviour for one Microsoft does
    /// not specify.
    ///
    /// MUTATION: drop the absent check before the call (pass the existing name
    /// to the call) and the exchange is asked, the answer is `Ok`, and the
    /// file that held the name is gone.
    #[cfg(windows)]
    #[test]
    fn an_existing_backup_name_is_refused() {
        let root = temp_root("taken");
        let target = root.join("pins.json");
        let temp = root.join("pins.json.tmp-1");
        let taken = root.join("pins.json.displaced-taken");
        fs::write(&target, b"the document").unwrap();
        fs::write(&temp, b"the replacement").unwrap();
        fs::write(&taken, b"somebody's file").unwrap();
        let surface = Recording::new(|temp, target, backup| {
            fs::rename(target, backup)?;
            fs::rename(temp, target)
        });

        let refusal =
            exchange_with(&surface, &temp, &target, &taken).expect_err("the backup name is taken");

        assert!(
            matches!(&refusal, ExchangeRefusal::BackupNameTaken(name) if *name == taken),
            "{refusal:?}"
        );
        assert!(surface.asked().is_empty(), "the call is never made");
        assert_eq!(fs::read(&taken).unwrap(), b"somebody's file");
        assert_eq!(fs::read(&target).unwrap(), b"the document");
        fs::remove_dir_all(root).unwrap();
    }

    /// RED ((b)14-13) — **a document that is a symbolic link, or has a second
    /// name, is refused before anything is written.** An exchange would put
    /// the new bytes under this one name and leave the other name, or the
    /// link's target, with the old ones.
    ///
    /// The hard link is made on every platform. The symbolic link is made on
    /// Unix, and on Windows when the account may create one (Developer Mode or
    /// the privilege); where Windows refuses it the test says so on its output
    /// and asserts the hard-linked half alone.
    ///
    /// MUTATION: drop `refuse_a_link` from the door and the hard-linked case
    /// is exchanged: the answer is `Ok` and the other name keeps the old
    /// bytes while this one has the new.
    #[test]
    fn a_symlinked_or_hard_linked_document_is_refused_not_replaced() {
        let root = temp_root("links");
        let target = root.join("settings.json");
        let other = root.join("dotfiles-settings.json");
        let temp = root.join("settings.json.tmp-1");
        fs::write(&target, b"linked").unwrap();
        fs::hard_link(&target, &other).unwrap();
        fs::write(&temp, b"new").unwrap();

        let refusal = exchange(&temp, &target).expect_err("a hard-linked document");

        assert!(matches!(refusal, ExchangeRefusal::Link), "{refusal:?}");
        assert_eq!(fs::read(&target).unwrap(), b"linked");
        assert_eq!(fs::read(&other).unwrap(), b"linked");
        assert_eq!(
            fs::read(&temp).unwrap(),
            b"new",
            "the temporary file is untouched"
        );
        assert_eq!(
            fs::read_dir(&root).unwrap().count(),
            3,
            "and nothing was made beside them"
        );

        let real = root.join("real-keybindings.json");
        let link = root.join("keybindings.json");
        fs::write(&real, b"pointed at").unwrap();
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&real, &link);
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&real, &link);
        match made {
            Ok(()) => {
                let refusal = exchange(&temp, &link).expect_err("a symbolic link");
                assert!(matches!(refusal, ExchangeRefusal::Link), "{refusal:?}");
                assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
                assert_eq!(fs::read(&real).unwrap(), b"pointed at");
                assert_eq!(fs::read(&temp).unwrap(), b"new");
            }
            Err(error) => {
                #[cfg(unix)]
                panic!("Unix makes a symbolic link without a privilege: {error}");
                #[cfg(windows)]
                println!(
                    "symbolic-link half not run: this account may not create one ({error}); \
                     the hard-linked half stands"
                );
            }
        }
        fs::remove_dir_all(root).unwrap();
    }

    /// RED — **after the exchange the target holds the new bytes and the file
    /// the door names holds the old ones**, on this machine's real filesystem
    /// through the real door. The bytes are mixed-script so a reader that
    /// re-encodes them would be caught.
    ///
    /// MUTATION: answer `Displaced { path: target }` and the displaced file's
    /// bytes are the new ones.
    #[test]
    fn the_displaced_file_holds_the_old_bytes_and_the_target_the_new() {
        let root = temp_root("bytes");
        let target = root.join("settings.json");
        let temp = root.join("settings.json.tmp-1");
        let old = "{\"font\": \"Fira Code\", \"title\": \"caf\u{e9} \u{3b1}\u{3b2}\"}\n";
        let new = "{\"font\": \"Iosevka\", \"title\": \"na\u{ef}ve \u{3b3}\u{3b4}\"}\n";
        fs::write(&target, old).unwrap();
        fs::write(&temp, new).unwrap();

        let displaced = exchange(&temp, &target).expect("a local volume exchanges");

        assert_ne!(displaced.path, target);
        assert_eq!(fs::read_to_string(&target).unwrap(), new);
        assert_eq!(fs::read_to_string(&displaced.path).unwrap(), old);
        assert_eq!(
            displaced.path.parent(),
            target.parent(),
            "the displaced file is the target's sibling"
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// RED — **a successful exchange is followed by one flush of the target's
    /// directory**, and nothing comes before the exchange: `Ok` means the
    /// entries are on the device, which `ReplaceFileW` (whose write-through
    /// flag is documented as unsupported) and a swap do not promise by
    /// themselves.
    ///
    /// MUTATION: skip the flush in `exchange_with` and the record is the
    /// exchange alone.
    #[test]
    fn the_directory_is_flushed_once_after_a_successful_exchange() {
        let root = temp_root("flush");
        let target = root.join("profiles.json");
        let temp = root.join("profiles.json.tmp-1");
        fs::write(&target, b"old").unwrap();
        fs::write(&temp, b"new").unwrap();
        let displaced = displaced_for(&temp, &target);
        let surface = Recording::new(|_, _, _| Ok(()));

        let answer = exchange_with(&surface, &temp, &target, &displaced).expect("exchanged");

        assert_eq!(answer.path, displaced);
        assert_eq!(
            surface.asked(),
            ["exchange".to_owned(), format!("flush {}", root.display())]
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// RED (Unix; E-2's metadata half on macOS) — **the new file takes the
    /// mode and the extended attributes of the file it displaces**: a swap
    /// trades entries, so without the carry the target would wear the
    /// temporary file's `0600` and none of its attributes.
    ///
    /// MUTATION: drop `carry_metadata` from the door and the mode assertion
    /// goes red.
    #[cfg(unix)]
    #[test]
    fn the_exchange_carries_mode_and_extended_attributes_onto_the_new_file() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_root("carry");
        let target = root.join("settings.json");
        let temp = root.join("settings.json.tmp-1");
        fs::write(&target, b"old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        let carried = set_user_attribute(&target, b"kept");
        #[cfg(target_os = "macos")]
        assert!(carried, "APFS holds extended attributes");
        fs::write(&temp, b"new").unwrap();
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o600)).unwrap();

        exchange(&temp, &target).expect("exchanged");

        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o644
        );
        if carried {
            assert_eq!(user_attribute(&target).as_deref(), Some(&b"kept"[..]));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    const USER_ATTRIBUTE: &str = "user.folio.exchange";

    /// `false` when the filesystem under the fixture holds no extended
    /// attribute (a pre-6.6 `tmpfs`).
    #[cfg(unix)]
    fn set_user_attribute(path: &Path, value: &[u8]) -> bool {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let name = std::ffi::CString::new(USER_ATTRIBUTE).unwrap();
        // SAFETY: live terminated path and name, and a buffer whose length is
        // the one passed beside it.
        #[cfg(target_os = "macos")]
        let set = unsafe {
            libc::setxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        // SAFETY: as above.
        #[cfg(not(target_os = "macos"))]
        let set = unsafe {
            libc::setxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        };
        set == 0
    }

    #[cfg(unix)]
    fn user_attribute(path: &Path) -> Option<Vec<u8>> {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        let name = std::ffi::CString::new(USER_ATTRIBUTE).ok()?;
        let mut value = vec![0_u8; 64];
        // SAFETY: as above; a negative answer is read as the failure it is.
        #[cfg(target_os = "macos")]
        let read = unsafe {
            libc::getxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        // SAFETY: as above.
        #[cfg(not(target_os = "macos"))]
        let read = unsafe {
            libc::getxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
            )
        };
        value.truncate(usize::try_from(read).ok()?);
        Some(value)
    }

    /// RED (macOS, E-2) — **the volume the tests run on says it can swap**:
    /// the capability read answers `true` on APFS, so the door's refusal of a
    /// volume that says no is a refusal of that volume and not of every one.
    ///
    /// MUTATION: read the capability word (`VOL_CAPABILITIES_FORMAT`) in place
    /// of the interfaces word and it answers `false`.
    #[cfg(target_os = "macos")]
    #[test]
    fn an_apfs_volume_says_it_can_swap() {
        let root = temp_root("volume");
        assert!(volume_swaps(&root).expect("the volume answers"));
        fs::remove_dir_all(root).unwrap();
    }

    /// RED — **the first write lands, and never over an entry already under
    /// the name**; the directory flush follows the rename.
    ///
    /// MUTATION: `std::fs::rename` in place of `rename_without_replacing` and
    /// the second write replaces the first.
    #[test]
    fn the_first_write_lands_and_never_replaces() {
        let root = temp_root("first");
        let target = root.join("pins.json");
        let temp = root.join("pins.json.tmp-1");
        let second = root.join("pins.json.tmp-2");
        fs::write(&temp, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        let (from, to) = (temp.clone(), target.clone());
        on_a_worker(move |worker| rename_without_replacing_durably(worker, &from, &to))
            .expect("nothing is under the name");
        let (from, to) = (second.clone(), target.clone());
        let refused =
            on_a_worker(move |worker| rename_without_replacing_durably(worker, &from, &to))
                .expect_err("the name is taken");

        assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&target).unwrap(), b"first");
        assert_eq!(fs::read(&second).unwrap(), b"second");
        fs::remove_dir_all(root).unwrap();
    }
}
