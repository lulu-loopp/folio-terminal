//! **The macOS entrance of an update transaction: a LaunchAgent plist**
//! (0.4.6 ticket U-26; `docs/plans/design/self-update-2026-09-16.md`
//! revision (b), F-3, §(b).2's objects table and §(b).3).
//!
//! An update that is interrupted — the machine loses power while the bundle is
//! being exchanged, or the trial dies — must be finished or undone at the next
//! login even if nobody starts Folio. On macOS that entrance is a per-user
//! LaunchAgent,
//! `~/Library/LaunchAgents/io.github.lulu-loopp.folio.update-<txn8>.plist`,
//! whose `RunAtLoad` runs the rescue clone's executable with
//! `--update-recover <home>`. **Writing the file is the whole registration**:
//! `launchd` reads the folder at the next login, so no `launchctl` call is
//! made, and nothing here asks the system anything.
//!
//! * [`arm`] writes the plist through `install_txn`'s durable write (a
//!   temporary file, `F_FULLFSYNC` on it, a rename, `F_FULLFSYNC` on the
//!   folder), reads it back and compares it byte for byte, and only then hands
//!   out the [`Armed`] proof the journal records `Armed` on. Any failure is a
//!   [`Refusal`] naming its step; the journal then never says `Armed`.
//! * [`disarm`] removes the plist and flushes the folder. A plist that is not
//!   there is success: retiring an entrance is repeated until it sticks.
//! * [`sweep`] is `--uninstall-cleanup`'s row: every plist in the folder whose
//!   name is exactly this door's shape ([`is_ours`]) is removed, and nothing
//!   else.
//!
//! **The folder is a parameter** of every call. The product passes
//! `~/Library/LaunchAgents`; a test passes a temporary folder and never
//! touches the real one.
//!
//! **Where it runs.** [`arm`] refuses by name off macOS ([`Refusal::NotHere`]):
//! a plist means nothing to Windows. [`disarm`] and [`sweep`] only remove files
//! of this door's own name, through `install_txn::durable_remove`, and are left
//! to that door's arms. Worker only, like `install_txn`: every call waits for
//! the disk.

use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::HostPlatform;
use crate::file_reads::{self, Lane};
use crate::install_txn::{self, Armed, Failure, Surface};

/// Every entrance's label starts with this; the rest is the transaction's
/// first eight hex digits. The bundle identifier's own prefix, so the owner of
/// the file is plain to anyone who opens the folder.
pub const LABEL_PREFIX: &str = "io.github.lulu-loopp.folio.update-";

/// The file an entrance is written to is its label and this.
pub const PLIST_SUFFIX: &str = ".plist";

/// The argument the rescue build is started with at login, before the home.
/// `bt-app`'s `cli::UPDATE_RECOVER_FLAG` is the same word, and a test there
/// holds the two equal.
pub const RECOVER_FLAG: &str = "--update-recover";

/// A transaction's first four bytes, as the eight lowercase hex digits an
/// entrance is named by (`<txn8>`).
fn txn8(txn: &[u8; 16]) -> String {
    txn[..4].iter().map(|byte| format!("{byte:02x}")).collect()
}

/// **The entrance's `Label`**: [`LABEL_PREFIX`] and `<txn8>`.
#[must_use]
pub fn label(txn: &[u8; 16]) -> String {
    format!("{LABEL_PREFIX}{}", txn8(txn))
}

/// **The entrance's file name** in the LaunchAgents folder.
#[must_use]
pub fn file_name(txn: &[u8; 16]) -> String {
    format!("{}{PLIST_SUFFIX}", label(txn))
}

/// **Whether a file name is one this door writes**: [`LABEL_PREFIX`], exactly
/// eight lowercase hex digits, and [`PLIST_SUFFIX`]. The one test
/// [`sweep`] asks, so that it removes nothing another program put there.
#[must_use]
pub fn is_ours(name: &OsStr) -> bool {
    name.to_str()
        .and_then(|name| name.strip_prefix(LABEL_PREFIX))
        .and_then(|rest| rest.strip_suffix(PLIST_SUFFIX))
        .is_some_and(|tag| {
            tag.len() == 8
                && tag
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

/// **Why an entrance was not armed, disarmed or swept.**
#[derive(Debug)]
pub enum Refusal {
    /// This platform has no LaunchAgents.
    NotHere,
    /// A path the plist would carry cannot be written in one: it is not UTF-8,
    /// or holds a character XML 1.0 has no spelling for.
    Unwritable { what: &'static str, path: PathBuf },
    /// The durable write of the plist stopped at the step it names.
    Write(Failure),
    /// The plist could not be read back after it was written.
    ReadBack { path: PathBuf, error: io::Error },
    /// The plist read back is not the one written.
    Differs { path: PathBuf },
    /// Removing a plist stopped at the step it names.
    Remove(Failure),
    /// The LaunchAgents folder could not be listed.
    List { path: PathBuf, error: io::Error },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotHere => write!(f, "launch_agent: no LaunchAgent on this platform"),
            Self::Unwritable { what, path } => write!(
                f,
                "launch_agent: the {what} cannot be written in a plist: {}",
                path.display()
            ),
            Self::Write(failure) => write!(f, "launch_agent write: {failure}"),
            Self::ReadBack { path, error } => {
                write!(f, "launch_agent read-back {}: {error}", path.display())
            }
            Self::Differs { path } => write!(
                f,
                "launch_agent read-back {}: not the plist written",
                path.display()
            ),
            Self::Remove(failure) => write!(f, "launch_agent remove: {failure}"),
            Self::List { path, error } => {
                write!(f, "launch_agent list {}: {error}", path.display())
            }
        }
    }
}

impl std::error::Error for Refusal {}

/// A path as the text of a plist `<string>`, escaped; a refusal names what
/// could not be written.
fn plist_string(what: &'static str, path: &Path) -> Result<String, Refusal> {
    let unwritable = || Refusal::Unwritable {
        what,
        path: path.to_path_buf(),
    };
    let text = path.to_str().ok_or_else(unwritable)?;
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            // XML 1.0's `Char`: tab, line feed and carriage return are the only
            // controls it admits, and a plist has no escape for the others.
            '\t' | '\n' | '\r' => escaped.push(character),
            control if control < ' ' || ('\u{7f}'..='\u{9f}').contains(&control) => {
                return Err(unwritable());
            }
            '\u{fffe}' | '\u{ffff}' => return Err(unwritable()),
            other => escaped.push(other),
        }
    }
    Ok(escaped)
}

/// **The plist, from its fixed template**: `Label`, `ProgramArguments` =
/// `[rescue_exe, "--update-recover", home]` and `RunAtLoad` = true. Nothing
/// else — no `KeepAlive`, no environment — because the rescue build decides
/// everything else from the journal in `home`.
///
/// # Errors
/// [`Refusal::Unwritable`] for a path a plist cannot carry.
pub fn plist(txn: &[u8; 16], rescue_exe: &Path, home: &Path) -> Result<String, Refusal> {
    let program = plist_string("rescue executable", rescue_exe)?;
    let home = plist_string("installation home", home)?;
    let label = label(txn);
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \t<key>Label</key>\n\
         \t<string>{label}</string>\n\
         \t<key>ProgramArguments</key>\n\
         \t<array>\n\
         \t\t<string>{program}</string>\n\
         \t\t<string>{RECOVER_FLAG}</string>\n\
         \t\t<string>{home}</string>\n\
         \t</array>\n\
         \t<key>RunAtLoad</key>\n\
         \t<true/>\n\
         </dict>\n\
         </plist>\n"
    ))
}

/// **Arm the transaction `txn`'s entrance** in the LaunchAgents folder
/// `agents`: the plist written durably (file and folder `F_FULLFSYNC`'d), read
/// back and compared, and only then the [`Armed`] proof.
///
/// A refusal after the write (the read-back) leaves the plist where it is: the
/// journal does not say `Armed`, and the caller retires the entrance with
/// [`disarm`] as (b).2's W4/M4 rows say.
///
/// # Errors
/// [`Refusal::NotHere`] off macOS; otherwise the step that failed.
pub fn arm(
    agents: &Path,
    txn: &[u8; 16],
    rescue_exe: &Path,
    home: &Path,
) -> Result<Armed, Refusal> {
    if crate::host_platform() != HostPlatform::MacOs {
        return Err(Refusal::NotHere);
    }
    arm_with(
        &mut install_txn::Os,
        |_, path| file_reads::read(Lane::Update, path),
        agents,
        txn,
        rescue_exe,
        home,
    )
}

/// [`arm`] over any surface, with the read-back as a separate call so that a
/// recording fake sees it in order with the write.
pub(crate) fn arm_with<S: Surface>(
    surface: &mut S,
    mut read_back: impl FnMut(&mut S, &Path) -> io::Result<Vec<u8>>,
    agents: &Path,
    txn: &[u8; 16],
    rescue_exe: &Path,
    home: &Path,
) -> Result<Armed, Refusal> {
    let bytes = plist(txn, rescue_exe, home)?.into_bytes();
    let path = agents.join(file_name(txn));
    install_txn::durable_write_with(surface, &path, &bytes).map_err(Refusal::Write)?;
    let back = read_back(surface, &path).map_err(|error| Refusal::ReadBack {
        path: path.clone(),
        error,
    })?;
    if back != bytes {
        return Err(Refusal::Differs { path });
    }
    Ok(Armed::proved(*txn, file_name(txn)))
}

/// **Retire the transaction `txn`'s entrance**: its plist removed and the
/// folder flushed. No plist there is success.
///
/// # Errors
/// [`Refusal::Remove`] naming the step that failed.
pub fn disarm(agents: &Path, txn: &[u8; 16]) -> Result<(), Refusal> {
    install_txn::durable_remove(&agents.join(file_name(txn))).map_err(Refusal::Remove)
}

/// What [`sweep`] did: each plist of ours it found, with its removal's answer.
pub type Swept = Vec<(PathBuf, Result<(), Refusal>)>;

/// **Remove every entrance this door ever wrote in `agents`** — the
/// `--uninstall-cleanup` row. Each plist whose name [`is_ours`] is removed
/// through [`install_txn::durable_remove`], and its own answer is returned
/// beside its path; a folder that is not there has nothing in it. Anything
/// else in the folder, and a folder of our name, is left alone.
///
/// # Errors
/// [`Refusal::List`] when the folder exists but cannot be listed.
pub fn sweep(agents: &Path) -> Result<Swept, Refusal> {
    let listed = match std::fs::read_dir(agents) {
        Ok(listed) => listed,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(Refusal::List {
                path: agents.to_path_buf(),
                error,
            });
        }
    };
    let mut ours = Vec::new();
    for entry in listed {
        let entry = entry.map_err(|error| Refusal::List {
            path: agents.to_path_buf(),
            error,
        })?;
        let is_directory = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if is_ours(&entry.file_name()) && !is_directory {
            ours.push(entry.path());
        }
    }
    ours.sort();
    Ok(ours
        .into_iter()
        .map(|path| {
            let removed = install_txn::durable_remove(&path).map_err(Refusal::Remove);
            (path, removed)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_txn::Stage;
    use crate::install_txn::recording::{Call, Fail, Recorder};

    const TXN: [u8; 16] = [0x7a, 0x01, 0xbe, 0xef, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

    fn agents() -> PathBuf {
        PathBuf::from("agents")
    }

    fn rescue() -> PathBuf {
        PathBuf::from(
            "/Applications/.Folio.app.folio-update/7a01beef000102030405060708090a0b/rescue/Folio.app/Contents/MacOS/folio",
        )
    }

    fn home() -> PathBuf {
        PathBuf::from("/Applications/.Folio.app.folio-update")
    }

    /// The bytes the recording fake holds at `path`: what was written to the
    /// temporary file the last rename onto `path` took.
    fn stored(calls: &[Call], path: &Path) -> Option<Vec<u8>> {
        let temporary = calls.iter().rev().find_map(|call| match call {
            Call::Rename(from, to, _) if to == path => Some(from.clone()),
            _ => None,
        })?;
        calls.iter().rev().find_map(|call| match call {
            Call::Write(written, bytes) if *written == temporary => Some(bytes.clone()),
            _ => None,
        })
    }

    /// The read-back over the recording fake: the call is written down, and
    /// the bytes are the ones the fake's file system holds.
    fn read_back(fake: &mut Recorder, path: &Path) -> io::Result<Vec<u8>> {
        fake.calls.push(Call::Read(path.to_path_buf()));
        stored(&fake.calls, path).ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    /// RED (U-26) — **`Armed` follows a full fsync of the plist: the file,
    /// then its folder, then a read-back, and only then the proof.**
    ///
    /// F-3: the LaunchAgent plist "is written and `F_FULLFSYNC`'d (file and
    /// directory) before the journal records `Armed`". A proof handed out
    /// before the folder's flush lets the journal say `Armed` while the plist's
    /// name may not be on the device: after a power cut the transaction is
    /// past the point where nothing had changed, and no login runs the rescue.
    /// The read-back is the check that the file the next login reads is the
    /// one meant.
    ///
    /// MUTATION: skip the directory fsync (`flush_directory`'s
    /// `surface.flush(&mut handle)` in `install_txn`), or answer `Ok(Armed)`
    /// from `arm_with` before the read-back.
    #[test]
    fn armed_follows_a_full_fsync_of_the_plist() {
        let mut fake = Recorder::default();
        let armed = arm_with(&mut fake, read_back, &agents(), &TXN, &rescue(), &home()).unwrap();
        let target = agents().join("io.github.lulu-loopp.folio.update-7a01beef.plist");
        assert_eq!(armed.transaction(), &TXN);
        assert_eq!(
            armed.entrance(),
            "io.github.lulu-loopp.folio.update-7a01beef.plist"
        );
        let Some(Call::Create(temporary)) = fake.calls.first().cloned() else {
            panic!(
                "the plist is written through a temporary file: {:?}",
                fake.calls
            );
        };
        assert_eq!(temporary.parent(), Some(agents().as_path()));
        let written = plist(&TXN, &rescue(), &home()).unwrap().into_bytes();
        assert_eq!(
            fake.calls,
            vec![
                Call::Create(temporary.clone()),
                Call::Write(temporary.clone(), written),
                Call::Flush {
                    path: temporary.clone(),
                    directory: false
                },
                Call::Close(temporary.clone()),
                Call::Rename(
                    temporary,
                    target.clone(),
                    crate::install_txn::Replace::Existing
                ),
                Call::OpenDirectory(agents()),
                Call::Flush {
                    path: agents(),
                    directory: true
                },
                Call::Close(agents()),
                Call::Read(target),
            ],
            "write, F_FULLFSYNC the file, rename, F_FULLFSYNC the folder, read back — then Armed"
        );
    }

    /// RED (U-26) — **a failed folder flush, or a plist that reads back
    /// different, is a refusal and never a proof.**
    ///
    /// MUTATION: in `arm_with`, drop the `back != bytes` comparison.
    #[test]
    fn a_plist_not_flushed_or_not_read_back_equal_is_never_armed() {
        let mut fake = Recorder::failing(Fail::FlushDirectory);
        let refused = arm_with(&mut fake, read_back, &agents(), &TXN, &rescue(), &home());
        assert!(
            matches!(&refused, Err(Refusal::Write(failure)) if failure.stage == Stage::FlushDirectory),
            "{refused:?}"
        );
        assert!(!fake.calls.iter().any(|call| matches!(call, Call::Read(_))));

        let mut fake = Recorder::default();
        let refused = arm_with(
            &mut fake,
            |_, _| Ok(b"<plist/>".to_vec()),
            &agents(),
            &TXN,
            &rescue(),
            &home(),
        );
        assert!(
            matches!(refused, Err(Refusal::Differs { .. })),
            "{refused:?}"
        );
    }

    /// RED (U-26) — **the template is fixed: the label, the three program
    /// arguments in order, `RunAtLoad`, and every path escaped.** A path a
    /// plist cannot carry is refused by name, before anything is written.
    ///
    /// MUTATION: in `plist_string`, push `&` as itself.
    #[test]
    fn the_plist_carries_the_label_the_rescue_the_word_and_the_home() {
        let text = plist(&TXN, &rescue(), &home()).unwrap();
        assert!(text.contains(
            "<key>Label</key>\n\t<string>io.github.lulu-loopp.folio.update-7a01beef</string>"
        ));
        let arguments = format!(
            "<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{}</string>\n\t\t<string>--update-recover</string>\n\t\t<string>{}</string>\n\t</array>",
            rescue().display(),
            home().display()
        );
        assert!(text.contains(&arguments), "{text}");
        assert!(text.contains("<key>RunAtLoad</key>\n\t<true/>"));
        assert_eq!(text.matches("<key>").count(), 3, "three keys and no more");

        let odd = PathBuf::from("/Volumes/A & B <x>/.Folio.app.folio-update");
        let text = plist(&TXN, &rescue(), &odd).unwrap();
        assert!(
            text.contains("<string>/Volumes/A &amp; B &lt;x&gt;/.Folio.app.folio-update</string>")
        );

        let control = PathBuf::from("/Applications/\u{1}.Folio.app.folio-update");
        let mut fake = Recorder::default();
        let refused = arm_with(&mut fake, read_back, &agents(), &TXN, &rescue(), &control);
        assert!(
            matches!(
                refused,
                Err(Refusal::Unwritable {
                    what: "installation home",
                    ..
                })
            ),
            "{refused:?}"
        );
        assert!(
            fake.calls.is_empty(),
            "nothing is written: {:?}",
            fake.calls
        );
    }

    /// RED (U-26) — **only a name of this door's exact shape is ours.**
    ///
    /// MUTATION: in `is_ours`, accept any tag length.
    #[test]
    fn only_a_name_of_the_doors_own_shape_is_ours() {
        assert!(is_ours(OsStr::new(&file_name(&TXN))));
        for foreign in [
            "io.github.lulu-loopp.folio.update-7a01beef.plist.bak",
            "io.github.lulu-loopp.folio.update-7A01BEEF.plist",
            "io.github.lulu-loopp.folio.update-7a01bee.plist",
            "io.github.lulu-loopp.folio.update-7a01beef0.plist",
            "io.github.lulu-loopp.folio.update-.plist",
            "io.github.lulu-loopp.folio.plist",
            "com.example.agent.plist",
        ] {
            assert!(!is_ours(OsStr::new(foreign)), "{foreign}");
        }
    }

    /// A folder of its own under the system's temporary folder, for the tests
    /// that touch a real file system.
    fn scratch(tag: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("bt-launch-agent-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn removal_works_here() -> bool {
        crate::host_platform() != HostPlatform::OtherUnix
    }

    /// RED (U-26) — **disarming an entrance that is not there succeeds, and
    /// disarming one that is removes exactly it.**
    ///
    /// Retirement is repeated at every start and login until it sticks
    /// ((b).2 W12/M11), so an absent plist must be an answer, not a failure.
    ///
    /// MUTATION: map `NotFound` from the removal to a refusal (drop the
    /// `NotFound` arm of `install_txn::durable_remove_with`).
    #[test]
    fn disarm_of_an_absent_plist_succeeds() {
        if !removal_works_here() {
            return;
        }
        let agents = scratch("disarm");
        disarm(&agents, &TXN).unwrap();
        std::fs::write(agents.join(file_name(&TXN)), b"plist").unwrap();
        std::fs::write(agents.join("com.example.agent.plist"), b"theirs").unwrap();
        disarm(&agents, &TXN).unwrap();
        assert!(!agents.join(file_name(&TXN)).exists());
        assert!(agents.join("com.example.agent.plist").exists());
        disarm(&agents, &TXN).unwrap();
        let _ = std::fs::remove_dir_all(&agents);
    }

    /// RED (U-26) — **the cleanup sweep removes only our entrances.**
    ///
    /// (b).3: the LaunchAgent plist is written outside Folio's own folder and
    /// has an `--uninstall-cleanup` row. The folder it is written to is shared
    /// by every program the person runs, so the row removes exactly the names
    /// this door writes.
    ///
    /// MUTATION: in `sweep`, push every entry rather than only `is_ours`.
    #[test]
    fn the_sweep_removes_only_our_entrances() {
        if !removal_works_here() {
            return;
        }
        let agents = scratch("sweep");
        let ours = [file_name(&TXN), file_name(&[0x10; 16])];
        for name in &ours {
            std::fs::write(agents.join(name), b"ours").unwrap();
        }
        for name in [
            "com.example.agent.plist",
            "io.github.lulu-loopp.folio.update-7a01beef.plist.bak",
        ] {
            std::fs::write(agents.join(name), b"theirs").unwrap();
        }
        std::fs::create_dir(agents.join(file_name(&[0x20; 16]))).unwrap();
        let swept = sweep(&agents).unwrap();
        let mut removed: Vec<String> = swept
            .iter()
            .map(|(path, result)| {
                assert!(result.is_ok(), "{result:?}");
                path.file_name().unwrap().to_string_lossy().into_owned()
            })
            .collect();
        removed.sort();
        let mut expected = ours.to_vec();
        expected.sort();
        assert_eq!(removed, expected);
        let mut left: Vec<String> = std::fs::read_dir(&agents)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            vec![
                String::from("com.example.agent.plist"),
                file_name(&[0x20; 16]),
                String::from("io.github.lulu-loopp.folio.update-7a01beef.plist.bak"),
            ]
        );
        assert!(sweep(&agents.join("absent")).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&agents);
    }

    /// RED (U-26) — **off macOS the entrance is refused by name, and nothing
    /// is written.**
    ///
    /// MUTATION: drop the platform check at the top of `arm`.
    #[test]
    fn off_macos_arming_is_refused_by_name() {
        if crate::host_platform() == HostPlatform::MacOs {
            return;
        }
        let agents = scratch("elsewhere");
        let refused = arm(&agents, &TXN, &rescue(), &home());
        assert!(matches!(refused, Err(Refusal::NotHere)), "{refused:?}");
        assert!(refused.unwrap_err().to_string().contains("launch_agent"));
        assert_eq!(std::fs::read_dir(&agents).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&agents);
    }

    /// RED (U-26) — **the real door writes a plist `plutil -lint` accepts,
    /// whose label, program arguments and `RunAtLoad` read back as written**,
    /// in a temporary folder standing in for `~/Library/LaunchAgents`.
    ///
    /// `plutil` is the system's own reader of the format `launchd` reads; the
    /// template passing it is the evidence the file is a plist at all.
    ///
    /// MUTATION: close `<array>` with `</dict>` in `plist`'s template.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_plist_round_trips_through_plutil() {
        let agents = scratch("plutil");
        let rescue = agents.join("rescue & co/Folio.app/Contents/MacOS/folio");
        let home = agents.join(".Folio.app.folio-update");
        let armed = arm(&agents, &TXN, &rescue, &home).unwrap();
        let path = agents.join(armed.entrance());
        let plutil = |arguments: &[&OsStr]| {
            let output = crate::quiet_command("/usr/bin/plutil")
                .args(arguments)
                .output()
                .unwrap();
            (
                output.status.success(),
                String::from_utf8(output.stdout).unwrap(),
            )
        };
        let (linted, said) = plutil(&[OsStr::new("-lint"), path.as_os_str()]);
        assert!(linted, "{said}");
        let extract = |key: &str| {
            let (ok, text) = plutil(&[
                OsStr::new("-extract"),
                OsStr::new(key),
                OsStr::new("raw"),
                OsStr::new("-o"),
                OsStr::new("-"),
                path.as_os_str(),
            ]);
            assert!(ok, "{key}: {text}");
            text.trim_end_matches('\n').to_owned()
        };
        assert_eq!(extract("Label"), label(&TXN));
        assert_eq!(extract("ProgramArguments.0"), rescue.to_str().unwrap());
        assert_eq!(extract("ProgramArguments.1"), RECOVER_FLAG);
        assert_eq!(extract("ProgramArguments.2"), home.to_str().unwrap());
        assert_eq!(extract("RunAtLoad"), "true");
        disarm(&agents, &TXN).unwrap();
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&agents);
    }
}
