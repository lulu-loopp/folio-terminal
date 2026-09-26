//! **The update's entrance at logon: one value under the current user's `Run`
//! key** (0.4.6 ticket U-22; `docs/plans/design/self-update-2026-09-16.md`
//! revision (b), F-2, §(b).2's objects table, §(b).3 and experiment E-7).
//!
//! An update that dies between its first move and a durable end state must be
//! finished by somebody, even when the install can no longer start. On Windows
//! that somebody is the rescue build, `H\<txn>\rescue\folio.exe`, started at the
//! next logon by the value `FolioUpdate-<txn8>` under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` ([`RUN_KEY`]) — `Run`
//! and not `RunOnce`, so it runs at **every** logon until the transaction
//! removes it (F-2). The command is `"<rescue>" --update-recover`
//! ([`command`]); the rescue build finds the journal from its own path.
//!
//! * **[`arm`]**: the command is measured first, and one longer than the
//!   documented 260 characters ([`COMMAND_LIMIT`]) is refused before anything
//!   is written ([`Refusal::TooLong`]). Then the value is written, the key is
//!   flushed (`RegFlushKey`, through `install_txn::flush_current_user_key`),
//!   and the value is read back and compared byte for byte. Only then does the
//!   caller hold an [`Armed`] — **the one value `bt-app`'s `update_txn` accepts
//!   as the event `Armed`**, which on Windows only this module makes (on
//!   macOS, only `crate::launch_agent`). So the journal can
//!   say `Armed` only after the entrance is on disk. Any failure is a
//!   [`Refusal`] naming its [`Stage`], and nothing else is changed; the
//!   transaction then goes to `Abandoned`, whose retirement removes a value
//!   that was written before the failing step.
//! * **[`disarm`]**: the value is removed and the key flushed. A value that is
//!   not there is success, so a retirement that is repeated is harmless.
//! * **[`clean`]**: `--uninstall-cleanup`'s per-copy row (§(b).3): of the
//!   values whose name begins `FolioUpdate-`, those whose command names this
//!   copy's installation home, or a program that no longer exists, are removed.
//!   Every other value — another program's, and another copy's live entrance —
//!   is left as it is.
//!
//! **These names are the only registry surface the updater writes** (§(b).3):
//! the key is [`RUN_KEY`] and the values are [`VALUE_PREFIX`] names. The
//! functions ending in `_in` take the key as a parameter and the registry as a
//! [`Registry`], so a test works under a key of its own
//! (`HKCU\Software\Folio-Test\<random>`) or over a recording fake, and never
//! writes the real `Run` key; the product calls the three without the suffix,
//! which name [`RUN_KEY`] and [`CurrentUser`].
//!
//! **Two arms.** Windows is real. Everywhere else [`CurrentUser`] refuses each
//! call with an error naming this door (`io::ErrorKind::Unsupported`), never as
//! if it had happened; the macOS entrance is a LaunchAgent (U-26).
//!
//! **Worker only**, like `install_txn`: a flush waits for the device. The one
//! exception is the start's retirement of a finished transaction
//! (`bt-app::update_startup`), before the event loop exists.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// The key the entrance is written under, below `HKEY_CURRENT_USER`: the
/// current user's `Run` key, whose values run at every logon. Shared by the
/// writer and by `--uninstall-cleanup`'s row.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Every entrance's value name begins with this, followed by the first eight
/// hex digits of the transaction ([`value_name`]).
pub const VALUE_PREFIX: &str = "FolioUpdate-";

/// **The argv word the entrance starts the rescue build with** — frozen at v1
/// from 0.4.6 (F-8); `bt-app`'s `cli::UPDATE_RECOVER_FLAG` is this constant.
pub const RECOVER_FLAG: &str = "--update-recover";

/// The longest command a `Run` value may carry, in characters (UTF-16 units),
/// as Microsoft documents for the `Run` and `RunOnce` keys.
pub const COMMAND_LIMIT: usize = 260;

/// The registry's type for a string value, `REG_SZ`.
pub const REG_SZ: u32 = 1;

/// **The entrance's value name for a transaction**: [`VALUE_PREFIX`] and the
/// first four bytes of its 16-byte identity as lowercase hex.
#[must_use]
pub fn value_name(txn: &[u8; 16]) -> String {
    let mut name = String::from(VALUE_PREFIX);
    for byte in &txn[..4] {
        name.push_str(&format!("{byte:02x}"));
    }
    name
}

/// **The entrance's command**: the rescue program quoted, then
/// [`RECOVER_FLAG`].
#[must_use]
pub fn command(rescue: &Path) -> OsString {
    let mut line = OsString::from("\"");
    line.push(rescue.as_os_str());
    line.push("\" ");
    line.push(RECOVER_FLAG);
    line
}

/// **The proof that a transaction's entrance is durable** — one type for both
/// platforms, `install_txn::Armed`, re-exported here because this door makes
/// it on Windows: only [`arm_in`] (after the write, the flush and a read-back
/// that matched) and the macOS LaunchAgent door (`crate::launch_agent`)
/// construct one. It is not `Clone`, and the event `Armed` of `bt-app`'s
/// `update_txn` carries one, so the journal cannot record `Armed` for an
/// entrance that is not on disk. Its [`Armed::entrance`] is the value's name,
/// [`value_name`] of [`Armed::transaction`].
pub use crate::install_txn::Armed;

/// **The step of the entrance that failed.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Writing the value.
    Write,
    /// `RegFlushKey` on the key.
    Flush,
    /// Reading the value back.
    ReadBack,
    /// Removing a value.
    Remove,
    /// Listing the key's values (the cleanup row).
    List,
}

impl Stage {
    fn said(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Flush => "flush",
            Self::ReadBack => "read-back",
            Self::Remove => "remove",
            Self::List => "list",
        }
    }
}

/// **Why an entrance was not made, or not removed.** [`Refusal::TooLong`]
/// wrote nothing; [`Refusal::Failed`] names the step that failed.
#[derive(Debug)]
pub enum Refusal {
    /// The command is longer than [`COMMAND_LIMIT`]; nothing was written.
    TooLong { characters: usize },
    /// The registry refused one step.
    Failed { stage: Stage, error: io::Error },
    /// The value read back is missing, or is not the bytes that were written.
    ReadBackDiffers,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong { characters } => write!(
                f,
                "logon_hook: the command is {characters} characters, over the \
                 {COMMAND_LIMIT} a Run value runs; nothing was written"
            ),
            Self::Failed { stage, error } => write!(f, "logon_hook {}: {error}", stage.said()),
            Self::ReadBackDiffers => {
                f.write_str("logon_hook read-back: the value read back is not the value written")
            }
        }
    }
}

impl std::error::Error for Refusal {}

/// **The registry calls this door makes**, one method each, on keys below
/// `HKEY_CURRENT_USER`. [`CurrentUser`] is the real one; a test hands in a
/// recording fake to see the order of the calls.
pub trait Registry {
    /// Write the value `name` of `key` as `kind` with `data`, creating the key
    /// if it is missing.
    ///
    /// # Errors
    /// The registry's error.
    fn set(&mut self, key: &str, name: &str, kind: u32, data: &[u8]) -> io::Result<()>;
    /// `RegFlushKey` on `key`.
    ///
    /// # Errors
    /// The registry's error; a missing key is `NotFound`.
    fn flush(&mut self, key: &str) -> io::Result<()>;
    /// The value's type and bytes, or `None` when the key or the value is not
    /// there.
    ///
    /// # Errors
    /// The registry's error.
    fn get(&mut self, key: &str, name: &str) -> io::Result<Option<(u32, Vec<u8>)>>;
    /// Remove the value; `false` when the key or the value was not there.
    ///
    /// # Errors
    /// The registry's error.
    fn delete(&mut self, key: &str, name: &str) -> io::Result<bool>;
    /// The names of the key's values; none when the key is not there.
    ///
    /// # Errors
    /// The registry's error.
    fn names(&mut self, key: &str) -> io::Result<Vec<String>>;
}

/// **The current user's registry** — the real [`Registry`] on Windows; a
/// refusal naming this door everywhere else.
#[derive(Clone, Copy, Debug, Default)]
pub struct CurrentUser;

/// A command's UTF-16 units, without a terminator.
fn units(text: &OsStr) -> Vec<u16> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        text.encode_wide().collect()
    }
    #[cfg(not(windows))]
    {
        text.to_string_lossy().encode_utf16().collect()
    }
}

/// A `REG_SZ`'s bytes: the units, a terminating NUL, little-endian.
fn string_data(units: &[u16]) -> Vec<u8> {
    units
        .iter()
        .chain(std::iter::once(&0))
        .flat_map(|unit| unit.to_le_bytes())
        .collect()
}

/// A `REG_SZ`'s text: its units up to the first NUL.
fn string_text(data: &[u8]) -> OsString {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&units)
    }
    #[cfg(not(windows))]
    {
        OsString::from(String::from_utf16_lossy(&units))
    }
}

/// **Make `txn`'s entrance durable** under [`RUN_KEY`], naming `rescue`.
///
/// # Errors
/// A [`Refusal`]: see [`arm_in`].
pub fn arm(txn: &[u8; 16], rescue: &Path) -> Result<Armed, Refusal> {
    arm_in(&mut CurrentUser, RUN_KEY, txn, rescue)
}

/// **Make `txn`'s entrance durable under `key` of `registry`**: measure the
/// command, write it, flush the key, read it back and compare — in that order,
/// and nothing after a step that fails.
///
/// # Errors
/// [`Refusal::TooLong`] before any call; [`Refusal::Failed`] at the step the
/// registry refused; [`Refusal::ReadBackDiffers`] when the value read back is
/// not the one written.
pub fn arm_in(
    registry: &mut impl Registry,
    key: &str,
    txn: &[u8; 16],
    rescue: &Path,
) -> Result<Armed, Refusal> {
    let line = units(&command(rescue));
    if line.len() > COMMAND_LIMIT {
        return Err(Refusal::TooLong {
            characters: line.len(),
        });
    }
    let name = value_name(txn);
    let data = string_data(&line);
    registry
        .set(key, &name, REG_SZ, &data)
        .map_err(|error| Refusal::Failed {
            stage: Stage::Write,
            error,
        })?;
    registry.flush(key).map_err(|error| Refusal::Failed {
        stage: Stage::Flush,
        error,
    })?;
    match registry.get(key, &name) {
        Ok(Some((REG_SZ, stored))) if stored == data => Ok(Armed::proved(*txn, name)),
        Ok(_) => Err(Refusal::ReadBackDiffers),
        Err(error) => Err(Refusal::Failed {
            stage: Stage::ReadBack,
            error,
        }),
    }
}

/// **Remove `txn`'s entrance** from [`RUN_KEY`].
///
/// # Errors
/// A [`Refusal`]: see [`disarm_in`].
pub fn disarm(txn: &[u8; 16]) -> Result<(), Refusal> {
    disarm_in(&mut CurrentUser, RUN_KEY, txn)
}

/// **Remove `txn`'s entrance from `key`, then flush the key.** A value that is
/// not there is success and nothing is flushed, because nothing changed.
///
/// # Errors
/// [`Refusal::Failed`] at [`Stage::Remove`] or [`Stage::Flush`].
pub fn disarm_in(registry: &mut impl Registry, key: &str, txn: &[u8; 16]) -> Result<(), Refusal> {
    let removed = registry
        .delete(key, &value_name(txn))
        .map_err(|error| Refusal::Failed {
            stage: Stage::Remove,
            error,
        })?;
    if removed {
        registry.flush(key).map_err(|error| Refusal::Failed {
            stage: Stage::Flush,
            error,
        })?;
    }
    Ok(())
}

/// **What the cleanup row did with one entrance value.**
#[derive(Debug)]
pub enum Cleaned {
    /// It named this copy's home, or a program that no longer exists, and it
    /// is gone.
    Removed,
    /// It names a program that exists outside this copy's home — another
    /// copy's transaction — or no program this door can read; it stays.
    Left(PathBuf),
    /// It was this copy's, and reading or removing it failed.
    Refused(Refusal),
}

/// **`--uninstall-cleanup`'s row for the entrance**, over [`RUN_KEY`].
///
/// # Errors
/// A [`Refusal`]: see [`clean_in`].
pub fn clean(home: &Path) -> Result<Vec<(String, Cleaned)>, Refusal> {
    clean_in(&mut CurrentUser, RUN_KEY, home)
}

/// **The cleanup row over `key`**: each value whose name begins
/// [`VALUE_PREFIX`] is removed if its command's program lies under `home` or
/// no longer exists (RULES §41: a mark naming a vanished path is nobody's);
/// every other value of the key is never read. The key is flushed once if
/// anything was removed.
///
/// # Errors
/// [`Refusal::Failed`] at [`Stage::List`] when the values cannot be listed,
/// or at [`Stage::Flush`] after a removal.
pub fn clean_in(
    registry: &mut impl Registry,
    key: &str,
    home: &Path,
) -> Result<Vec<(String, Cleaned)>, Refusal> {
    let names = registry.names(key).map_err(|error| Refusal::Failed {
        stage: Stage::List,
        error,
    })?;
    let home = crate::instance::canonical_path(home);
    let mut cleaned = Vec::new();
    let mut removed_any = false;
    for name in names
        .into_iter()
        .filter(|name| name.starts_with(VALUE_PREFIX))
    {
        let stored = match registry.get(key, &name) {
            Ok(Some((REG_SZ, data))) => string_text(&data),
            Ok(Some((_, _))) => {
                cleaned.push((name, Cleaned::Left(PathBuf::new())));
                continue;
            }
            Ok(None) => continue,
            Err(error) => {
                let refusal = Refusal::Failed {
                    stage: Stage::ReadBack,
                    error,
                };
                cleaned.push((name, Cleaned::Refused(refusal)));
                continue;
            }
        };
        let Some(program) = program_of(&stored) else {
            cleaned.push((name, Cleaned::Left(PathBuf::from(stored))));
            continue;
        };
        let ours =
            !program.exists() || crate::instance::canonical_path(&program).starts_with(&home);
        if !ours {
            cleaned.push((name, Cleaned::Left(program)));
            continue;
        }
        let fate = match registry.delete(key, &name) {
            Ok(_) => {
                removed_any = true;
                Cleaned::Removed
            }
            Err(error) => Cleaned::Refused(Refusal::Failed {
                stage: Stage::Remove,
                error,
            }),
        };
        cleaned.push((name, fate));
    }
    if removed_any {
        registry.flush(key).map_err(|error| Refusal::Failed {
            stage: Stage::Flush,
            error,
        })?;
    }
    Ok(cleaned)
}

/// The program a command starts: the text between its opening quote and the
/// next one, as [`command`] writes it; `None` for a command of another shape.
fn program_of(command: &OsStr) -> Option<PathBuf> {
    let text = command.to_str()?;
    let rest = text.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(PathBuf::from(&rest[..end]))
}

/// **The Windows arm.**
#[cfg(windows)]
mod os {
    use super::{CurrentUser, Registry};
    use std::io;
    use windows::Win32::Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, WIN32_ERROR,
    };
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
        REG_SAM_FLAGS, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW, RegDeleteValueW,
        RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW, RegQueryValueExW, RegSetValueExW,
    };
    use windows::core::{PCWSTR, PWSTR};

    pub(super) fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn checked(code: WIN32_ERROR) -> io::Result<()> {
        if code == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(code.0.cast_signed()))
        }
    }

    /// An open key, closed when dropped.
    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful open and is closed
            // once, here.
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    /// `None` when the key is not there.
    fn open(key: &str, access: REG_SAM_FLAGS) -> io::Result<Option<Key>> {
        let name = wide(key);
        let mut opened = HKEY::default();
        // SAFETY: the name is NUL-terminated and lives across the call;
        // `opened` is written only on success.
        let code = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(name.as_ptr()),
                None,
                access,
                &raw mut opened,
            )
        };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        checked(code)?;
        Ok(Some(Key(opened)))
    }

    impl Registry for CurrentUser {
        fn set(&mut self, key: &str, name: &str, kind: u32, data: &[u8]) -> io::Result<()> {
            let key_name = wide(key);
            let mut opened = HKEY::default();
            // SAFETY: the name is NUL-terminated and lives across the call;
            // `opened` is written only on success and closed by `Key`.
            checked(unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    PCWSTR(key_name.as_ptr()),
                    None,
                    PCWSTR::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE,
                    None,
                    &raw mut opened,
                    None,
                )
            })?;
            let opened = Key(opened);
            let value = wide(name);
            // SAFETY: the name is NUL-terminated and `data` is the whole
            // buffer handed over; both live across the call.
            checked(unsafe {
                RegSetValueExW(
                    opened.0,
                    PCWSTR(value.as_ptr()),
                    None,
                    REG_VALUE_TYPE(kind),
                    Some(data),
                )
            })
        }

        fn flush(&mut self, key: &str) -> io::Result<()> {
            crate::install_txn::flush_current_user_key(key).map_err(|failure| failure.error)
        }

        fn get(&mut self, key: &str, name: &str) -> io::Result<Option<(u32, Vec<u8>)>> {
            let Some(opened) = open(key, KEY_QUERY_VALUE)? else {
                return Ok(None);
            };
            let value = wide(name);
            let mut kind = REG_VALUE_TYPE::default();
            let mut size = 0u32;
            // SAFETY: a null data pointer with a live size asks for the size;
            // the out-parameters are owned locals.
            let code = unsafe {
                RegQueryValueExW(
                    opened.0,
                    PCWSTR(value.as_ptr()),
                    None,
                    Some(&raw mut kind),
                    None,
                    Some(&raw mut size),
                )
            };
            if code == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            checked(code)?;
            let mut data = vec![0u8; size as usize];
            // SAFETY: `data` holds `size` bytes and lives across the call;
            // `size` is updated to what was written.
            checked(unsafe {
                RegQueryValueExW(
                    opened.0,
                    PCWSTR(value.as_ptr()),
                    None,
                    Some(&raw mut kind),
                    Some(data.as_mut_ptr()),
                    Some(&raw mut size),
                )
            })?;
            data.truncate(size as usize);
            Ok(Some((kind.0, data)))
        }

        fn delete(&mut self, key: &str, name: &str) -> io::Result<bool> {
            let Some(opened) = open(key, KEY_SET_VALUE)? else {
                return Ok(false);
            };
            let value = wide(name);
            // SAFETY: the name is NUL-terminated and lives across the call.
            let code = unsafe { RegDeleteValueW(opened.0, PCWSTR(value.as_ptr())) };
            if code == ERROR_FILE_NOT_FOUND {
                return Ok(false);
            }
            checked(code)?;
            Ok(true)
        }

        fn names(&mut self, key: &str) -> io::Result<Vec<String>> {
            let Some(opened) = open(key, KEY_QUERY_VALUE)? else {
                return Ok(Vec::new());
            };
            let mut longest = 0u32;
            // SAFETY: only the longest value name's length is asked for, into
            // an owned local.
            checked(unsafe {
                RegQueryInfoKeyW(
                    opened.0,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(&raw mut longest),
                    None,
                    None,
                    None,
                )
            })?;
            let mut names = Vec::new();
            let mut buffer = vec![0u16; longest as usize + 1];
            for index in 0u32.. {
                let mut length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
                // SAFETY: `buffer` holds `length` units and lives across the
                // call; `length` is updated to the name's length.
                let code = unsafe {
                    RegEnumValueW(
                        opened.0,
                        index,
                        Some(PWSTR(buffer.as_mut_ptr())),
                        &raw mut length,
                        None,
                        None,
                        None,
                        None,
                    )
                };
                if code == ERROR_NO_MORE_ITEMS {
                    break;
                }
                checked(code)?;
                names.push(String::from_utf16_lossy(&buffer[..length as usize]));
            }
            Ok(names)
        }
    }
}

/// **Every other platform**: no registry, and every call says so by name.
#[cfg(not(windows))]
mod os {
    use super::{CurrentUser, Registry};
    use std::io;

    fn refused<T>(what: &str) -> io::Result<T> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("logon_hook has no registry on this platform ({what})"),
        ))
    }

    impl Registry for CurrentUser {
        fn set(&mut self, _: &str, _: &str, _: u32, _: &[u8]) -> io::Result<()> {
            refused("write")
        }

        fn flush(&mut self, _: &str) -> io::Result<()> {
            refused("flush")
        }

        fn get(&mut self, _: &str, _: &str) -> io::Result<Option<(u32, Vec<u8>)>> {
            refused("read")
        }

        fn delete(&mut self, _: &str, _: &str) -> io::Result<bool> {
            refused("remove")
        }

        fn names(&mut self, _: &str) -> io::Result<Vec<String>> {
            refused("list")
        }
    }
}

#[cfg(test)]
mod tests {
    //! Every test but two runs over [`Recorder`], a recording fake of
    //! [`Registry`]. The two Windows tests use the real registry under a key of
    //! their own, `HKCU\Software\Folio-Test\<random>`, which [`TestKey`] deletes
    //! with everything in it; none of them names [`RUN_KEY`].
    use super::*;
    use std::collections::BTreeMap;

    /// One call the door made.
    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Call {
        Set(String),
        Flush,
        Get(String),
        Delete(String),
        Names,
    }

    /// **A recording fake of the registry**: one key's values in memory,
    /// every call logged, and any one kind of call made to fail.
    #[derive(Default)]
    struct Recorder {
        values: BTreeMap<String, (u32, Vec<u8>)>,
        calls: Vec<Call>,
        fail: Option<fn(&Call) -> bool>,
        /// What a read answers instead of the stored bytes.
        tamper: Option<Vec<u8>>,
    }

    impl Recorder {
        fn log(&mut self, call: Call) -> io::Result<()> {
            let failed = self.fail.is_some_and(|fail| fail(&call));
            self.calls.push(call);
            if failed {
                Err(io::Error::other("the fake refuses this call"))
            } else {
                Ok(())
            }
        }
    }

    impl Registry for Recorder {
        fn set(&mut self, _: &str, name: &str, kind: u32, data: &[u8]) -> io::Result<()> {
            self.log(Call::Set(name.to_owned()))?;
            self.values.insert(name.to_owned(), (kind, data.to_vec()));
            Ok(())
        }

        fn flush(&mut self, _: &str) -> io::Result<()> {
            self.log(Call::Flush)
        }

        fn get(&mut self, _: &str, name: &str) -> io::Result<Option<(u32, Vec<u8>)>> {
            self.log(Call::Get(name.to_owned()))?;
            Ok(match &self.tamper {
                Some(bytes) => Some((REG_SZ, bytes.clone())),
                None => self.values.get(name).cloned(),
            })
        }

        fn delete(&mut self, _: &str, name: &str) -> io::Result<bool> {
            self.log(Call::Delete(name.to_owned()))?;
            Ok(self.values.remove(name).is_some())
        }

        fn names(&mut self, _: &str) -> io::Result<Vec<String>> {
            self.log(Call::Names)?;
            Ok(self.values.keys().cloned().collect())
        }
    }

    const TXN: [u8; 16] = [0xab, 0xcd, 0x01, 0x23, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9];

    /// A rescue path whose command is exactly `characters` long.
    fn rescue_of_command_length(characters: usize) -> PathBuf {
        let frame = units(&command(Path::new(""))).len();
        let stem = r"C:\p\";
        let tail = r"\rescue\folio.exe";
        let fill = characters - frame - stem.len() - tail.len();
        PathBuf::from(format!("{stem}{}{tail}", "x".repeat(fill)))
    }

    /// RED (U-22) — **the entrance is written, flushed and read back, in that
    /// order, before the caller holds the `Armed` that lets the journal record
    /// `Armed`.**
    ///
    /// F-2: "written, then `RegFlushKey`, then read back, **before** the journal
    /// records `Armed`". [`Armed`] has no constructor outside [`arm_in`], and
    /// `bt-app`'s `update_txn::Event::Armed` carries one, so the order of these
    /// calls is the order the journal can follow. A flush that fails leaves no
    /// proof, and no read-back is made after it.
    ///
    /// MUTATION: skip the read-back in `arm_in` (answer `Ok(Armed { .. })`
    /// right after the flush), or skip the flush.
    #[test]
    fn armed_is_never_written_before_the_entrance_is_flushed() {
        let mut registry = Recorder::default();
        let rescue = Path::new(r"C:\Folio\.folio-update\abcd\rescue\folio.exe");
        let armed = arm_in(&mut registry, "k", &TXN, rescue).expect("armed");
        assert_eq!(
            registry.calls,
            vec![
                Call::Set("FolioUpdate-abcd0123".to_owned()),
                Call::Flush,
                Call::Get("FolioUpdate-abcd0123".to_owned()),
            ]
        );
        assert_eq!(armed.transaction(), &TXN);
        assert_eq!(armed.entrance(), "FolioUpdate-abcd0123");
        let (kind, data) = &registry.values["FolioUpdate-abcd0123"];
        assert_eq!(*kind, REG_SZ);
        assert_eq!(
            string_text(data),
            OsString::from(r#""C:\Folio\.folio-update\abcd\rescue\folio.exe" --update-recover"#)
        );

        let mut refusing = Recorder {
            fail: Some(|call| *call == Call::Flush),
            ..Recorder::default()
        };
        let refused = arm_in(&mut refusing, "k", &TXN, rescue).unwrap_err();
        assert!(matches!(
            refused,
            Refusal::Failed {
                stage: Stage::Flush,
                ..
            }
        ));
        assert_eq!(
            refusing.calls,
            vec![Call::Set("FolioUpdate-abcd0123".to_owned()), Call::Flush],
            "nothing is read back, and no proof exists, after a failed flush"
        );
    }

    /// RED (U-22) — **a command longer than 260 characters is refused before
    /// anything is written, so the transaction never reaches `Armed` and no file
    /// is moved.**
    ///
    /// F-2: the command "is checked against the documented 260-character limit
    /// before `Armed`. Too long means *Nothing changed*." Exactly 260 is
    /// accepted.
    ///
    /// MUTATION: measure the command after the write in `arm_in`.
    #[test]
    fn a_long_path_is_refused_before_any_move() {
        let mut registry = Recorder::default();
        let long = rescue_of_command_length(COMMAND_LIMIT + 1);
        assert_eq!(units(&command(&long)).len(), 261);
        let refused = arm_in(&mut registry, "k", &TXN, &long).unwrap_err();
        assert!(matches!(refused, Refusal::TooLong { characters: 261 }));
        assert!(
            registry.calls.is_empty(),
            "the fake saw {:?}",
            registry.calls
        );
        assert!(refused.to_string().contains("nothing was written"));

        let exact = rescue_of_command_length(COMMAND_LIMIT);
        assert!(arm_in(&mut registry, "k", &TXN, &exact).is_ok());
    }

    /// RED (U-22) — **a value read back that is not the one written is a
    /// refusal, and a registry that refuses the write stops there.**
    ///
    /// MUTATION: accept any `Some(_)` from the read-back in `arm_in`.
    #[test]
    fn a_read_back_that_differs_is_refused() {
        let rescue = Path::new(r"C:\F\.folio-update\t\rescue\folio.exe");
        let other = units(OsStr::new("\"C:\\other.exe\" --update-recover"));
        let mut tampered = Recorder {
            tamper: Some(string_data(&other)),
            ..Recorder::default()
        };
        assert!(matches!(
            arm_in(&mut tampered, "k", &TXN, rescue),
            Err(Refusal::ReadBackDiffers)
        ));

        let mut refusing = Recorder {
            fail: Some(|call| matches!(call, Call::Set(_))),
            ..Recorder::default()
        };
        let refused = arm_in(&mut refusing, "k", &TXN, rescue).unwrap_err();
        assert!(matches!(
            refused,
            Refusal::Failed {
                stage: Stage::Write,
                ..
            }
        ));
        assert!(refused.to_string().starts_with("logon_hook write: "));
        assert_eq!(refusing.calls.len(), 1);
    }

    /// RED (U-22) — **disarming removes the value and flushes the key; a value
    /// that is not there is success, and nothing is flushed for it.**
    ///
    /// MUTATION: answer a missing value with an error in `disarm_in`.
    #[test]
    fn disarm_of_an_absent_value_succeeds() {
        let mut registry = Recorder::default();
        let rescue = Path::new(r"C:\F\.folio-update\t\rescue\folio.exe");
        let _armed = arm_in(&mut registry, "k", &TXN, rescue).expect("armed");
        registry.calls.clear();
        disarm_in(&mut registry, "k", &TXN).expect("removed");
        assert_eq!(
            registry.calls,
            vec![Call::Delete("FolioUpdate-abcd0123".to_owned()), Call::Flush]
        );
        assert!(registry.values.is_empty());
        registry.calls.clear();
        disarm_in(&mut registry, "k", &TXN).expect("absent is success");
        assert_eq!(
            registry.calls,
            vec![Call::Delete("FolioUpdate-abcd0123".to_owned())]
        );
    }

    /// **The cleanup row reads back the program a command names**, and a
    /// command of another shape names none.
    #[test]
    fn the_cleanup_row_reads_the_program_a_command_names() {
        let rescue = Path::new(r"C:\A B\.folio-update\t\rescue\folio.exe");
        assert_eq!(program_of(&command(rescue)).as_deref(), Some(rescue));
        assert_eq!(program_of(OsStr::new("C:\\x.exe --flag")), None);
    }

    /// A key of this test's own: `HKCU\Software\Folio-Test\<random>`, deleted
    /// with everything in it when dropped. Never the `Run` key.
    #[cfg(windows)]
    struct TestKey(String);

    #[cfg(windows)]
    impl TestKey {
        fn new(tag: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos());
            let key = format!(
                r"Software\Folio-Test\{tag}-{}-{nanos}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            assert!(key.starts_with(r"Software\Folio-Test\") && key != RUN_KEY);
            Self(key)
        }
    }

    #[cfg(windows)]
    impl Drop for TestKey {
        fn drop(&mut self) {
            use windows::Win32::System::Registry::{
                HKEY_CURRENT_USER, RegDeleteKeyW, RegDeleteTreeW,
            };
            use windows::core::PCWSTR;
            let key = os::wide(&self.0);
            let parent = os::wide(r"Software\Folio-Test");
            // SAFETY: both names are NUL-terminated and live across the calls.
            // The parent goes only when no other test's key is left in it.
            unsafe {
                let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()));
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()));
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(parent.as_ptr()));
            }
        }
    }

    /// RED (U-22) — **the real registry: an entrance written under a key of
    /// the test's own reads back as written, is removed, and a second removal
    /// is success.**
    ///
    /// MUTATION: in the Windows arm's `get`, read into a buffer two bytes
    /// longer than the size query answered and keep it untruncated (the value
    /// read back is then not the bytes written).
    #[cfg(windows)]
    #[test]
    fn the_entrance_round_trips_through_the_real_registry() {
        let key = TestKey::new("round-trip");
        let rescue = Path::new(r"C:\Program Files\Folio\.folio-update\abcd\rescue\folio.exe");
        let armed = arm_in(&mut CurrentUser, &key.0, &TXN, rescue).expect("armed");
        assert_eq!(armed.entrance(), "FolioUpdate-abcd0123");
        let (kind, data) = CurrentUser
            .get(&key.0, "FolioUpdate-abcd0123")
            .unwrap()
            .expect("the value is there");
        assert_eq!(kind, REG_SZ);
        assert_eq!(string_text(&data), command(rescue));
        assert_eq!(
            CurrentUser.names(&key.0).unwrap(),
            vec!["FolioUpdate-abcd0123".to_owned()]
        );
        disarm_in(&mut CurrentUser, &key.0, &TXN).expect("removed");
        assert_eq!(
            CurrentUser.get(&key.0, "FolioUpdate-abcd0123").unwrap(),
            None
        );
        disarm_in(&mut CurrentUser, &key.0, &TXN).expect("absent is success");
    }

    /// RED (U-22) — **the cleanup row removes this copy's entrances — the one
    /// naming its home and the one naming a program that is gone — and leaves
    /// another program's value and another copy's live entrance.**
    ///
    /// §(b).3: "a per-copy row that removes the value if it names this copy's
    /// `H` or a path that no longer exists"; RULES §41. The real registry,
    /// under a key of the test's own, and real files for "exists".
    ///
    /// MUTATION: remove every `FolioUpdate-` value in `clean_in` (make `ours`
    /// always true).
    #[cfg(windows)]
    #[test]
    fn the_cleanup_row_removes_only_this_copys_entrances() {
        let key = TestKey::new("cleanup");
        let root = std::env::temp_dir().join(format!("bt-logon-hook-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rescue_in = |install: &str| {
            root.join(install)
                .join(".folio-update")
                .join("abcd0123")
                .join("rescue")
                .join("folio.exe")
        };
        let home = root.join("install").join(".folio-update");
        let ours = rescue_in("install");
        let other = rescue_in("other");
        let gone = rescue_in("gone");
        for program in [&ours, &other] {
            std::fs::create_dir_all(program.parent().unwrap()).unwrap();
            std::fs::write(program, b"fixture").unwrap();
        }
        for (name, text) in [
            ("FolioUpdate-abcd0123", command(&ours)),
            ("FolioUpdate-99999999", command(&gone)),
            ("FolioUpdate-77777777", command(&other)),
            (
                "SomeoneElse",
                OsString::from(r#""C:\gone\tool.exe" --start"#),
            ),
        ] {
            CurrentUser
                .set(&key.0, name, REG_SZ, &string_data(&units(&text)))
                .unwrap();
        }

        let cleaned = clean_in(&mut CurrentUser, &key.0, &home).expect("listed");
        let fates: BTreeMap<String, String> = cleaned
            .iter()
            .map(|(name, fate)| (name.clone(), format!("{fate:?}")))
            .collect();
        assert_eq!(fates.len(), 3, "{fates:?}");
        assert_eq!(fates["FolioUpdate-abcd0123"], "Removed");
        assert_eq!(fates["FolioUpdate-99999999"], "Removed");
        assert!(fates["FolioUpdate-77777777"].starts_with("Left("));
        let mut left = CurrentUser.names(&key.0).unwrap();
        left.sort();
        assert_eq!(
            left,
            vec!["FolioUpdate-77777777".to_owned(), "SomeoneElse".to_owned()]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-22) — **off Windows the door refuses by name**, and claims no
    /// write it did not make.
    ///
    /// MUTATION: answer `Ok(())` from the portable arm's `set`.
    #[cfg(not(windows))]
    #[test]
    fn the_portable_arm_refuses_by_name() {
        let refused = arm(&TXN, Path::new("/x/rescue/folio")).unwrap_err();
        assert!(matches!(
            refused,
            Refusal::Failed {
                stage: Stage::Write,
                ..
            }
        ));
        assert!(refused.to_string().contains("logon_hook has no registry"));
        assert!(disarm(&TXN).is_err());
    }
}
