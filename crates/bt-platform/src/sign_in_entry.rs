//! **The sign-in entry: one value named `Folio` under the current user's `Run` key** (0.4.10 plan
//! row 22a, T-START-AT-LOGON-SWITCH; rulings R1, R2 and R9 of 2026-11-11).
//!
//! `Settings ▸ General ▸ Start at sign-in` is this one value: [`ENTRY_NAME`] under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` ([`crate::logon_hook::RUN_KEY`], the key the
//! update's entrance is written under), a `REG_SZ` whose text is exactly the quoted path of a copy's
//! `folio.exe` and nothing after it ([`command_for`]). **No flag**: every word on that line is read
//! by whichever build sits at that path at a later sign-in, and a build older than this one answers
//! a word it does not know with a usage box (R2).
//!
//! **Windows' own switch for it** is a second value of the same name, under
//! `…\Explorer\StartupApproved\Run` ([`STARTUP_APPROVED_KEY`]): Task Manager's Startup apps and
//! Settings ▸ Apps ▸ Startup write a `REG_BINARY` there whose first byte's low bit says the entry
//! is turned off ([`EntryRead::turned_off`]; the layout is the clean-VM measurement M-W1's to
//! confirm). Absent is the system's default, on. Folio reads it, and deletes the value of its own
//! name only when the switch is pressed `On` or the entry is removed ([`enable_entry_in`],
//! [`remove_entry_in`]; R9).
//!
//! **Only the one name is ever read** — never a listing of either key — so the update's entrances
//! (`FolioUpdate-<txn8>`, `crate::logon_hook::clean_by`) and every other program's values are not
//! this door's, and this door is not theirs: `Folio` is not of the entrance's shape.
//!
//! **What is judged here and what is not.** This door knows the value's shape: [`EntryFound::Command`]
//! is a `REG_SZ` that is one quoted path and nothing else, [`EntryFound::Other`] everything else under the
//! name. Whose copy that path is — this one, another that is there, one that is gone — is `bt-app`'s
//! `start_at_sign_in`, which has the ownership rules.
//!
//! The functions ending in `_in` take the registry as a [`Registry`] and the keys as [`EntryKeys`],
//! so a test works under keys of its own (`HKCU\Software\Folio-Test\<tag>-<pid>-<ordinal>`) or over
//! a fake, and never names the account's keys; the product hands them [`ACCOUNT_KEYS`] and
//! [`CurrentUser`]. Off Windows [`CurrentUser`] refuses every call by name. **Worker only**: a
//! registry call is a wait the window thread does not make (`bt-app`'s carry pass and the press's
//! own worker are the callers).

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::logon_hook::{
    COMMAND_LIMIT, REG_SZ, RUN_KEY, Registry, string_data, string_text, units,
};

pub use crate::logon_hook::CurrentUser;

/// **The value's name** under both keys.
pub const ENTRY_NAME: &str = "Folio";

/// **Where Windows keeps its own switch for a `Run` value**, below `HKEY_CURRENT_USER`.
pub const STARTUP_APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// `REG_BINARY`, the type the system's switch is written as.
pub const REG_BINARY: u32 = 3;

/// **The two keys the entry lives under**: the `Run` key and the system's switch beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryKeys<'a> {
    pub run: &'a str,
    pub approved: &'a str,
}

/// The account's own two keys — the product's, and nothing a test names.
pub const ACCOUNT_KEYS: EntryKeys<'static> = EntryKeys {
    run: RUN_KEY,
    approved: STARTUP_APPROVED_KEY,
};

/// **The entry's command for a copy**: its `folio.exe`, quoted, and nothing after it.
#[must_use]
pub fn command_for(exe: &Path) -> OsString {
    let mut line = OsString::from("\"");
    line.push(exe.as_os_str());
    line.push("\"");
    line
}

/// The program a command of [`command_for`]'s shape starts; `None` for any other text.
fn entry_program_of(command: &OsStr) -> Option<PathBuf> {
    let inner = command.to_str()?.strip_prefix('"')?.strip_suffix('"')?;
    (!inner.is_empty() && !inner.contains('"')).then(|| PathBuf::from(inner))
}

/// **What stands under [`ENTRY_NAME`] in the `Run` key.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryFound {
    /// No value of the name.
    Absent,
    /// A `REG_SZ` that is exactly [`command_for`] of this program.
    Command(PathBuf),
    /// Anything else under the name: another type, or any other text. Never judged.
    Other,
}

/// **One reading of the entry**: what the `Run` key holds, and whether the system's switch says it
/// is turned off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryRead {
    pub found: EntryFound,
    /// The system's switch: a `REG_BINARY` of [`ENTRY_NAME`] under [`EntryKeys::approved`] whose
    /// first byte has its low bit set. Absent, empty or of another type is on.
    pub turned_off: bool,
}

/// **The step of the entry that failed.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryStage {
    /// Reading a value.
    Read,
    /// Writing the value.
    Write,
    /// Reading the value back.
    ReadBack,
    /// Removing a value.
    Remove,
}

impl EntryStage {
    /// The stage, as a diagnostics line says it.
    #[must_use]
    pub fn stage_word(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::ReadBack => "read-back",
            Self::Remove => "remove",
        }
    }
}

/// **Why the entry was not read, written or removed.** [`EntryRefusal::TooLong`] wrote nothing.
#[derive(Debug)]
pub enum EntryRefusal {
    /// The command is longer than [`COMMAND_LIMIT`]; nothing was written.
    TooLong { characters: usize },
    /// The registry refused one step.
    Failed { stage: EntryStage, error: io::Error },
    /// The value read back is missing, or is not the bytes that were written.
    ReadBackDiffers,
}

impl EntryRefusal {
    /// The stage it stopped at, as a diagnostics line says it — never a path.
    #[must_use]
    pub fn stage_said(&self) -> &'static str {
        match self {
            Self::TooLong { .. } => "too long",
            Self::Failed { stage, .. } => stage.stage_word(),
            Self::ReadBackDiffers => "read-back",
        }
    }
}

impl fmt::Display for EntryRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong { characters } => write!(
                f,
                "sign_in_entry: the command is {characters} characters, over the \
                 {COMMAND_LIMIT} a Run value runs; nothing was written"
            ),
            Self::Failed { stage, error } => {
                write!(f, "sign_in_entry {}: {error}", stage.stage_word())
            }
            Self::ReadBackDiffers => {
                f.write_str("sign_in_entry read-back: the value read back is not the value written")
            }
        }
    }
}

impl std::error::Error for EntryRefusal {}

fn entry_failed(stage: EntryStage) -> impl FnOnce(io::Error) -> EntryRefusal {
    move |error| EntryRefusal::Failed { stage, error }
}

/// **Read the entry under `keys`**: the one value of [`ENTRY_NAME`] in each key, nothing else.
///
/// # Errors
/// [`EntryRefusal::Failed`] at [`EntryStage::Read`].
pub fn read_entry_in(
    registry: &mut impl Registry,
    keys: EntryKeys<'_>,
) -> Result<EntryRead, EntryRefusal> {
    let found = match registry
        .get(keys.run, ENTRY_NAME)
        .map_err(entry_failed(EntryStage::Read))?
    {
        None => EntryFound::Absent,
        Some((REG_SZ, data)) => {
            entry_program_of(&string_text(&data)).map_or(EntryFound::Other, EntryFound::Command)
        }
        Some(_) => EntryFound::Other,
    };
    let turned_off = matches!(
        registry
            .get(keys.approved, ENTRY_NAME)
            .map_err(entry_failed(EntryStage::Read))?,
        Some((REG_BINARY, data)) if data.first().is_some_and(|first| first & 1 == 1)
    );
    Ok(EntryRead { found, turned_off })
}

/// **Point the entry at `exe`**: measure the command, write it, read it back and compare — in
/// that order, and nothing after a step that fails. The system's switch is not touched
/// ([`enable_entry_in`] is the press's). No `RegFlushKey`: the entry is not a mark a crash must
/// find, and a write that did not reach the disk reads as `Off` at the next reading.
///
/// # Errors
/// [`EntryRefusal::TooLong`] before any call; [`EntryRefusal::Failed`] at the step the registry refused;
/// [`EntryRefusal::ReadBackDiffers`] when the value read back is not the one written.
pub fn write_entry_in(
    registry: &mut impl Registry,
    keys: EntryKeys<'_>,
    exe: &Path,
) -> Result<(), EntryRefusal> {
    let line = units(&command_for(exe));
    if line.len() > COMMAND_LIMIT {
        return Err(EntryRefusal::TooLong {
            characters: line.len(),
        });
    }
    let data = string_data(&line);
    registry
        .set(keys.run, ENTRY_NAME, REG_SZ, &data)
        .map_err(entry_failed(EntryStage::Write))?;
    match registry.get(keys.run, ENTRY_NAME) {
        Ok(Some((REG_SZ, stored))) if stored == data => Ok(()),
        Ok(_) => Err(EntryRefusal::ReadBackDiffers),
        Err(error) => Err(EntryRefusal::Failed {
            stage: EntryStage::ReadBack,
            error,
        }),
    }
}

/// **Take the system's switch off the entry**: the value of [`ENTRY_NAME`] under
/// [`EntryKeys::approved`] removed, which is the system's default (on). `false` when there was none.
///
/// # Errors
/// [`EntryRefusal::Failed`] at [`EntryStage::Remove`].
pub fn enable_entry_in(
    registry: &mut impl Registry,
    keys: EntryKeys<'_>,
) -> Result<bool, EntryRefusal> {
    registry
        .delete(keys.approved, ENTRY_NAME)
        .map_err(entry_failed(EntryStage::Remove))
}

/// **Remove the entry**: the `Run` value and the system's switch of the same name. `false` when
/// there was no `Run` value. The caller has judged the value Folio's; this door removes by name.
///
/// # Errors
/// [`EntryRefusal::Failed`] at [`EntryStage::Remove`]; the switch is not removed when the value was not.
pub fn remove_entry_in(
    registry: &mut impl Registry,
    keys: EntryKeys<'_>,
) -> Result<bool, EntryRefusal> {
    let removed = registry
        .delete(keys.run, ENTRY_NAME)
        .map_err(entry_failed(EntryStage::Remove))?;
    enable_entry_in(registry, keys)?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    //! Every test but one runs over [`TwoKeys`], a fake of [`Registry`] that keeps each key's
    //! values apart and records every call. The Windows test uses the real registry under keys of
    //! its own, `HKCU\Software\Folio-Test\<tag>-<pid>-<ordinal>\{Run,StartupApproved}`, which
    //! [`SignInTestKeys`] deletes with everything in them; none names [`RUN_KEY`].
    use super::*;
    use std::collections::BTreeMap;

    const ENTRY_TEST_KEYS: EntryKeys<'static> = EntryKeys {
        run: r"Software\Folio-Test\sign-in\Run",
        approved: r"Software\Folio-Test\sign-in\StartupApproved",
    };

    /// **Two keys held in memory**, every call recorded as `(verb, key, name)`.
    #[derive(Default)]
    struct TwoKeys {
        values: BTreeMap<(String, String), (u32, Vec<u8>)>,
        calls: Vec<(&'static str, String, String)>,
        /// What a read of the `Run` value answers instead of the stored bytes.
        tamper: Option<Vec<u8>>,
    }

    impl TwoKeys {
        fn record_call(&mut self, verb: &'static str, key: &str, name: &str) {
            assert!(
                key.starts_with(r"Software\Folio-Test\"),
                "only the test's own keys: {key}"
            );
            self.calls.push((verb, key.to_owned(), name.to_owned()));
        }
    }

    impl Registry for TwoKeys {
        fn set(&mut self, key: &str, name: &str, kind: u32, data: &[u8]) -> io::Result<()> {
            self.record_call("set", key, name);
            self.values
                .insert((key.to_owned(), name.to_owned()), (kind, data.to_vec()));
            Ok(())
        }

        fn flush(&mut self, key: &str) -> io::Result<()> {
            self.record_call("flush", key, "");
            Ok(())
        }

        fn get(&mut self, key: &str, name: &str) -> io::Result<Option<(u32, Vec<u8>)>> {
            self.record_call("get", key, name);
            if key == ENTRY_TEST_KEYS.run
                && let Some(bytes) = &self.tamper
            {
                return Ok(Some((REG_SZ, bytes.clone())));
            }
            Ok(self.values.get(&(key.to_owned(), name.to_owned())).cloned())
        }

        fn delete(&mut self, key: &str, name: &str) -> io::Result<bool> {
            self.record_call("delete", key, name);
            Ok(self
                .values
                .remove(&(key.to_owned(), name.to_owned()))
                .is_some())
        }

        fn names(&mut self, key: &str) -> io::Result<Vec<String>> {
            self.record_call("names", key, "");
            Ok(self
                .values
                .keys()
                .filter(|(owner, _)| owner == key)
                .map(|(_, name)| name.clone())
                .collect())
        }
    }

    /// A `REG_SZ`'s bytes for `text`.
    fn entry_sz(text: &str) -> Vec<u8> {
        string_data(&units(OsStr::new(text)))
    }

    /// The program the tests point the entry at.
    fn entry_exe() -> PathBuf {
        PathBuf::from(r"C:\Tools\Folio\folio.exe")
    }

    /// RED (T-START-AT-LOGON-SWITCH, R2) — **the entry is the quoted path of the copy and nothing
    /// after it, written, read back, and read as that copy's command**; the system's switch is not
    /// touched by the write, and only the one name is ever asked of either key.
    ///
    /// MUTATION (plant `plant-22-flag-on-the-command.py`): append ` --from-sign-in` in
    /// [`command_for`] and the text compared here has a flag on it.
    #[test]
    fn the_entry_is_the_quoted_path_and_nothing_after_it() {
        let mut registry = TwoKeys::default();
        write_entry_in(&mut registry, ENTRY_TEST_KEYS, &entry_exe()).expect("written");
        assert_eq!(
            registry.values[&(ENTRY_TEST_KEYS.run.to_owned(), ENTRY_NAME.to_owned())],
            (REG_SZ, entry_sz(r#""C:\Tools\Folio\folio.exe""#)),
            "exactly the quoted path, NUL-terminated UTF-16"
        );
        assert_eq!(
            read_entry_in(&mut registry, ENTRY_TEST_KEYS).expect("read"),
            EntryRead {
                found: EntryFound::Command(entry_exe()),
                turned_off: false,
            }
        );
        assert!(
            registry
                .calls
                .iter()
                .all(|(verb, _, name)| *verb != "names" && name == ENTRY_NAME),
            "only the entry's own name, never a listing: {:?}",
            registry.calls
        );
        assert!(
            !registry
                .calls
                .iter()
                .any(|(verb, key, _)| *verb != "get" && key == ENTRY_TEST_KEYS.approved),
            "the write leaves the system's switch alone: {:?}",
            registry.calls
        );
    }

    /// RED (T-START-AT-LOGON-SWITCH) — **a command over the 260 characters a `Run` value runs is
    /// refused before any call, and a read-back that differs is refused.**
    #[test]
    fn a_long_path_is_refused_before_any_registry_call() {
        let mut registry = TwoKeys::default();
        let frame = units(&command_for(Path::new(""))).len();
        let stem = r"C:\p\";
        let tail = r"\folio.exe";
        let long = PathBuf::from(format!(
            "{stem}{}{tail}",
            "x".repeat(COMMAND_LIMIT + 1 - frame - stem.len() - tail.len())
        ));
        assert_eq!(units(&command_for(&long)).len(), COMMAND_LIMIT + 1);
        assert!(matches!(
            write_entry_in(&mut registry, ENTRY_TEST_KEYS, &long),
            Err(EntryRefusal::TooLong { characters }) if characters == COMMAND_LIMIT + 1
        ));
        assert!(registry.calls.is_empty(), "nothing asked of the registry");

        let mut tampered = TwoKeys {
            tamper: Some(entry_sz(r#""C:\Elsewhere\folio.exe""#)),
            ..TwoKeys::default()
        };
        assert!(matches!(
            write_entry_in(&mut tampered, ENTRY_TEST_KEYS, &entry_exe()),
            Err(EntryRefusal::ReadBackDiffers)
        ));
    }

    /// RED (T-START-AT-LOGON-SWITCH, R9) — **what stands under the name is read for its shape**: a
    /// quoted path alone is a command; any other text, another type, an empty quote, and a quote
    /// with something after it are [`EntryFound::Other`]; the system's switch reads off by the low bit
    /// of its first byte and on when absent, empty or of another type; on presses deletes only the
    /// switch of the entry's own name, and removing the entry takes both values of its name and
    /// leaves every other value of both keys.
    #[test]
    fn the_entry_and_the_systems_switch_are_read_by_their_one_name() {
        let shapes: [(u32, Vec<u8>, EntryFound); 6] = [
            (
                REG_SZ,
                entry_sz(r#""C:\T\folio.exe""#),
                EntryFound::Command(PathBuf::from(r"C:\T\folio.exe")),
            ),
            (
                REG_SZ,
                entry_sz(r#""C:\T\folio.exe" --x"#),
                EntryFound::Other,
            ),
            (REG_SZ, entry_sz(r"C:\T\folio.exe"), EntryFound::Other),
            (REG_SZ, entry_sz(r#""""#), EntryFound::Other),
            (
                2,
                entry_sz(r#""%LOCALAPPDATA%\folio.exe""#),
                EntryFound::Other,
            ),
            (4, vec![1, 0, 0, 0], EntryFound::Other),
        ];
        for (kind, data, found) in shapes {
            let mut registry = TwoKeys::default();
            registry.values.insert(
                (ENTRY_TEST_KEYS.run.to_owned(), ENTRY_NAME.to_owned()),
                (kind, data),
            );
            assert_eq!(
                read_entry_in(&mut registry, ENTRY_TEST_KEYS).unwrap().found,
                found
            );
        }
        for (approved, off) in [
            (None, false),
            (
                Some((REG_BINARY, vec![2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])),
                false,
            ),
            (
                Some((REG_BINARY, vec![3, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8])),
                true,
            ),
            (Some((REG_BINARY, vec![7])), true),
            (Some((REG_BINARY, vec![])), false),
            (Some((REG_SZ, entry_sz("3"))), false),
        ] {
            let mut registry = TwoKeys::default();
            if let Some(value) = approved {
                registry.values.insert(
                    (ENTRY_TEST_KEYS.approved.to_owned(), ENTRY_NAME.to_owned()),
                    value,
                );
            }
            assert_eq!(
                read_entry_in(&mut registry, ENTRY_TEST_KEYS)
                    .unwrap()
                    .turned_off,
                off
            );
        }

        let mut registry = TwoKeys::default();
        for (key, name) in [
            (ENTRY_TEST_KEYS.run, ENTRY_NAME),
            (ENTRY_TEST_KEYS.run, "FolioUpdate-abcd0123"),
            (ENTRY_TEST_KEYS.run, "SomebodyElse"),
            (ENTRY_TEST_KEYS.approved, ENTRY_NAME),
            (ENTRY_TEST_KEYS.approved, "SomebodyElse"),
        ] {
            registry.set(key, name, REG_BINARY, &[3]).unwrap();
        }
        assert!(enable_entry_in(&mut registry, ENTRY_TEST_KEYS).unwrap());
        assert!(
            !enable_entry_in(&mut registry, ENTRY_TEST_KEYS).unwrap(),
            "once"
        );
        registry
            .set(ENTRY_TEST_KEYS.approved, ENTRY_NAME, REG_BINARY, &[3])
            .unwrap();
        assert!(remove_entry_in(&mut registry, ENTRY_TEST_KEYS).unwrap());
        let mut left: Vec<(String, String)> = registry.values.keys().cloned().collect();
        left.sort();
        assert_eq!(
            left,
            [
                (
                    ENTRY_TEST_KEYS.run.to_owned(),
                    "FolioUpdate-abcd0123".to_owned()
                ),
                (ENTRY_TEST_KEYS.run.to_owned(), "SomebodyElse".to_owned()),
                (
                    ENTRY_TEST_KEYS.approved.to_owned(),
                    "SomebodyElse".to_owned()
                ),
            ]
        );
        assert!(
            !remove_entry_in(&mut registry, ENTRY_TEST_KEYS).unwrap(),
            "absent"
        );
    }

    /// Keys of this test's own under `HKCU\Software\Folio-Test\<tag>-<pid>-<ordinal>`, deleted
    /// with everything in them when dropped. Never the account's `Run` key.
    #[cfg(windows)]
    struct SignInTestKeys {
        root: String,
        run: String,
        approved: String,
    }

    #[cfg(windows)]
    impl SignInTestKeys {
        fn new(tag: &str) -> Self {
            let root = format!(r"Software\Folio-Test\{}", bt_testpath::unique_name(tag));
            let keys = Self {
                run: format!(r"{root}\Run"),
                approved: format!(r"{root}\StartupApproved"),
                root,
            };
            assert!(keys.run != RUN_KEY && keys.approved != STARTUP_APPROVED_KEY);
            keys
        }

        fn test_keys(&self) -> EntryKeys<'_> {
            EntryKeys {
                run: &self.run,
                approved: &self.approved,
            }
        }
    }

    #[cfg(windows)]
    impl Drop for SignInTestKeys {
        fn drop(&mut self) {
            use windows::Win32::System::Registry::{
                HKEY_CURRENT_USER, RegDeleteKeyW, RegDeleteTreeW,
            };
            use windows::core::PCWSTR;
            let wide = |text: &str| -> Vec<u16> {
                text.encode_utf16().chain(std::iter::once(0)).collect()
            };
            let root = wide(&self.root);
            let parent = wide(r"Software\Folio-Test");
            // SAFETY: both names are NUL-terminated and live across the calls. The parent goes
            // only when no other test's key is left in it.
            unsafe {
                let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(root.as_ptr()));
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(root.as_ptr()));
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(parent.as_ptr()));
            }
        }
    }

    /// RED (T-START-AT-LOGON-SWITCH) — **the real registry: the entry written under keys of the
    /// test's own reads back as written, the system's switch written beside it reads off, on
    /// takes it away, and removal takes both and is success again when nothing is left.**
    ///
    /// MUTATION: in [`read_entry_in`], read the switch under [`EntryKeys::run`] and it never reads
    /// off.
    #[cfg(windows)]
    #[test]
    fn the_sign_in_entry_round_trips_through_the_real_registry() {
        let test = SignInTestKeys::new("sign-in-entry");
        let keys = test.test_keys();
        let exe = Path::new(r"C:\Program Files\Folio\folio.exe");
        write_entry_in(&mut CurrentUser, keys, exe).expect("written");
        CurrentUser
            .set(keys.run, "SomebodyElse", REG_SZ, &entry_sz(r#""C:\x.exe""#))
            .unwrap();
        CurrentUser
            .set(keys.approved, ENTRY_NAME, REG_BINARY, &[3, 0, 0, 0])
            .unwrap();
        assert_eq!(
            read_entry_in(&mut CurrentUser, keys).unwrap(),
            EntryRead {
                found: EntryFound::Command(exe.to_path_buf()),
                turned_off: true,
            }
        );
        assert!(enable_entry_in(&mut CurrentUser, keys).unwrap());
        assert!(!read_entry_in(&mut CurrentUser, keys).unwrap().turned_off);
        assert!(remove_entry_in(&mut CurrentUser, keys).unwrap());
        assert_eq!(
            read_entry_in(&mut CurrentUser, keys).unwrap().found,
            EntryFound::Absent
        );
        assert_eq!(
            CurrentUser.names(keys.run).unwrap(),
            vec!["SomebodyElse".to_owned()],
            "every other value of the key stays"
        );
        assert!(!remove_entry_in(&mut CurrentUser, keys).unwrap());
    }
}
