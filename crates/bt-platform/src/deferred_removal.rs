//! **A removal that waits for processes to end** — the uninstaller's last step
//! (0.4.7 ticket T-UNINSTALL-UX; `docs/DESIGN.md`, 2026-09-29, *One press
//! uninstalls Folio*).
//!
//! A program cannot remove the folder it runs from while it runs: on Windows
//! its image and every console standing in the folder hold it. So the door
//! hands this module the items to remove and the processes to wait for, and
//! [`schedule`] starts a process that outlives the door, through
//! [`crate::quiet_command`], and returns at once. That process waits until
//! none of the given processes runs, removes the items, removes the folder
//! the caller names only if it is then empty, and leaves.
//!
//! **What is removed is the caller's decision, never this module's**: it is
//! handed absolute paths and removes exactly those. The caller (`bt-app`'s
//! `uninstall`) derives them from the running executable and refuses a link
//! among them before it asks.
//!
//! * **Windows** — a script written into a folder the caller names (the
//!   temporary directory in the product) through `install_txn::durable_create`
//!   (a new file only), run by `cmd.exe /d /s /c`. **Every path reaches the
//!   script through its environment, never through its text**: the script is
//!   ASCII built only from this module's words and numbers, so no code page,
//!   no `%`, `&`, `^` or `!` in a path can change what it runs. It waits by
//!   asking `tasklist` for each process by pid **and** image name — a pid
//!   Windows has handed to another program is not the one waited for — each
//!   program by its path under `%SystemRoot%\System32` and never by a bare
//!   name, because the `PATH` a console inherits may put another `find` first
//!   (Git's GNU `find`, for a script run from Git Bash) — a
//!   second between looks; then `del /a /f /q` for each file, `rd /s /q` for
//!   each directory (which removes a junction inside it without entering it),
//!   `rd` without `/s` for the folder, and the script deletes itself.
//! * **macOS and Linux** — `/bin/sh -c` with the items as positional
//!   arguments, in a process group of its own and deaf to `SIGHUP`, so the
//!   terminal the door was run from can close; it waits with `kill -0` a
//!   second at a time, then `/bin/rm -rf --` each item and `/bin/rmdir` the
//!   folder — programs by path, as on Windows.
//!
//! Nothing here waits: the processes are looked up once, the script is
//! written, the process is started, and the answer is whether it started.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::install_flip::Running;

/// **One thing to remove**, by its absolute path and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// A file (or a link, which is removed and never followed).
    File(PathBuf),
    /// A directory, removed whole.
    Directory(PathBuf),
}

impl Item {
    /// The path this item removes.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::File(path) | Self::Directory(path) => path,
        }
    }
}

/// **What the started process is told**: the processes to outlive, the items,
/// and the folder to remove afterwards if nothing is left in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Removal {
    /// The processes whose end is waited for. One that has already ended, or
    /// whose pid now names a process that started at another instant, is not
    /// waited for.
    pub after: Vec<Running>,
    /// What is removed, in this order.
    pub items: Vec<Item>,
    /// Removed afterwards only if it is empty; `None` for none.
    pub folder: Option<PathBuf>,
}

/// **The environment variable a path or a process reaches the script by** —
/// `FOLIO_REMOVAL_<what>_<n>`, numbered from 1.
fn variable(what: &str, index: usize) -> String {
    format!("FOLIO_REMOVAL_{what}_{}", index + 1)
}

/// **Start the process that performs `removal`**, writing its script (on
/// Windows) into `scripts`, and answer once it has started.
///
/// # Errors
/// The script could not be written, or the process could not be started;
/// nothing is then removed. `Unsupported` where there is no arm.
pub fn schedule(removal: &Removal, scripts: &Path) -> io::Result<()> {
    let waited: Vec<(u32, OsString)> = removal
        .after
        .iter()
        .filter(|process| crate::install_flip::still_running(**process))
        .filter_map(|process| Some((process.pid, arm::image_name(process.pid)?)))
        .collect();
    arm::start(&waited, removal, scripts)
}

/// **The Windows script's text** for `waited` processes, `files` files,
/// `directories` directories and whether there is a folder to remove after —
/// counts only: every path and name is read from the environment
/// ([`variable`]).
#[cfg_attr(
    not(any(windows, test)),
    expect(
        dead_code,
        reason = "permanent: the Windows arm's script, read by the Windows arm and by the tests on every host"
    )
)]
fn windows_script(waited: usize, items: &[bool], folder: bool) -> String {
    let mut script = String::from(
        "@echo off\r\n\
         setlocal DisableDelayedExpansion\r\n\
         cd /d \"%SystemRoot%\"\r\n\
         :wait\r\n",
    );
    for index in 0..waited {
        let pid = variable("PID", index);
        let image = variable("IMAGE", index);
        script.push_str(&format!(
            "\"%SystemRoot%\\System32\\tasklist.exe\" /nh /fi \"PID eq %{pid}%\" \
             /fi \"IMAGENAME eq %{image}%\" 2>nul \
             | \"%SystemRoot%\\System32\\find.exe\" /i \"%{image}%\" >nul && goto pause\r\n"
        ));
    }
    script.push_str(
        "goto remove\r\n:pause\r\n\"%SystemRoot%\\System32\\PING.EXE\" -n 2 127.0.0.1 >nul\r\n\
             goto wait\r\n:remove\r\n",
    );
    for (index, directory) in items.iter().enumerate() {
        let item = variable("ITEM", index);
        if *directory {
            script.push_str(&format!("rd /s /q \"%{item}%\" 2>nul\r\n"));
        } else {
            script.push_str(&format!("del /a /f /q \"%{item}%\" 2>nul\r\n"));
        }
    }
    if folder {
        script.push_str("rd \"%FOLIO_REMOVAL_FOLDER%\" 2>nul\r\n");
    }
    // The script's own file, last: `(goto)` leaves the batch context first, so
    // `cmd` does not go back to read a line of a file that is gone.
    script.push_str("(goto) 2>nul & del /f /q \"%~f0\"\r\n");
    script
}

/// **The Unix script** — constant: the pids come in one variable, the items
/// as positional arguments, the folder in another variable.
#[cfg_attr(
    not(any(unix, test)),
    expect(
        dead_code,
        reason = "permanent: the Unix arm's script, read by the Unix arm and by the tests on every host"
    )
)]
const UNIX_SCRIPT: &str = "trap '' HUP INT\n\
while :; do\n\
  alive=\n\
  for pid in $FOLIO_REMOVAL_PIDS; do\n\
    kill -0 \"$pid\" 2>/dev/null && alive=1\n\
  done\n\
  [ -z \"$alive\" ] && break\n\
  /bin/sleep 1\n\
done\n\
for item in \"$@\"; do\n\
  /bin/rm -rf -- \"$item\"\n\
done\n\
if [ -n \"$FOLIO_REMOVAL_FOLDER\" ]; then\n\
  /bin/rmdir -- \"$FOLIO_REMOVAL_FOLDER\" 2>/dev/null\n\
fi\n";

#[cfg(windows)]
mod arm {
    use super::{Item, Removal, variable, windows_script};
    use std::ffi::OsString;
    use std::io;
    use std::os::windows::process::CommandExt;
    use std::path::Path;

    pub(super) fn image_name(pid: u32) -> Option<OsString> {
        crate::process_image_path(pid)?
            .file_name()
            .map(std::ffi::OsStr::to_os_string)
    }

    pub(super) fn start(
        waited: &[(u32, OsString)],
        removal: &Removal,
        scripts: &Path,
    ) -> io::Result<()> {
        let kinds: Vec<bool> = removal
            .items
            .iter()
            .map(|item| matches!(item, Item::Directory(_)))
            .collect();
        let text = windows_script(waited.len(), &kinds, removal.folder.is_some());
        let script = scripts.join(format!(
            "folio-removal-{}-{}.cmd",
            std::process::id(),
            crate::install_flip::started_of(std::process::id()).unwrap_or(0)
        ));
        crate::install_txn::durable_create(&script, text.as_bytes())
            .map_err(|failure| io::Error::new(failure.error.kind(), failure.to_string()))?;
        let mut command = crate::quiet_command_named(Path::new("cmd.exe")).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "cmd.exe is not where Windows keeps it",
            )
        })?;
        // `/s`: the quotes around the whole command are taken off and the rest
        // run as written, so the script's path — itself read from the
        // environment — needs no rule about which characters it may hold.
        // Started standing in the scripts' folder, never in the one it removes.
        command
            .raw_arg("/d /s /c \"\"%FOLIO_REMOVAL_SCRIPT%\"\"")
            .current_dir(scripts)
            .env("FOLIO_REMOVAL_SCRIPT", &script);
        for (index, (pid, image)) in waited.iter().enumerate() {
            command
                .env(variable("PID", index), pid.to_string())
                .env(variable("IMAGE", index), image);
        }
        for (index, item) in removal.items.iter().enumerate() {
            command.env(variable("ITEM", index), item.path());
        }
        if let Some(folder) = &removal.folder {
            command.env("FOLIO_REMOVAL_FOLDER", folder);
        }
        match command.spawn() {
            Ok(_child) => Ok(()),
            Err(error) => {
                // Never started: the script it would have deleted is ours to delete.
                let _ = std::fs::remove_file(&script);
                Err(error)
            }
        }
    }
}

#[cfg(unix)]
mod arm {
    use super::{Removal, UNIX_SCRIPT};
    use std::ffi::OsString;
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::path::Path;

    /// No name is needed: `kill -0` asks by pid alone.
    pub(super) fn image_name(_pid: u32) -> Option<OsString> {
        Some(OsString::new())
    }

    pub(super) fn start(
        waited: &[(u32, OsString)],
        removal: &Removal,
        _scripts: &Path,
    ) -> io::Result<()> {
        let pids: Vec<String> = waited.iter().map(|(pid, _)| pid.to_string()).collect();
        let mut command = crate::quiet_command("/bin/sh");
        command
            .arg("-c")
            .arg(UNIX_SCRIPT)
            .arg("folio-removal")
            .args(removal.items.iter().map(super::Item::path))
            .env("FOLIO_REMOVAL_PIDS", pids.join(" "))
            .env(
                "FOLIO_REMOVAL_FOLDER",
                removal.folder.as_deref().unwrap_or(Path::new("")),
            )
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .current_dir("/")
            // Not in the terminal's foreground group: a terminal that closes
            // hangs up that group, and the script outlives it.
            .process_group(0);
        command.spawn().map(drop)
    }
}

#[cfg(not(any(windows, unix)))]
mod arm {
    use super::Removal;
    use std::ffi::OsString;
    use std::io;
    use std::path::Path;

    pub(super) fn image_name(_pid: u32) -> Option<OsString> {
        None
    }

    pub(super) fn start(
        _waited: &[(u32, OsString)],
        _removal: &Removal,
        _scripts: &Path,
    ) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "deferred_removal starts its remover on Windows and Unix only",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{Item, Removal, UNIX_SCRIPT, schedule, windows_script};
    use crate::install_flip::Running;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    /// A fresh folder under the temporary directory, with this test's tag.
    fn sandbox(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "folio-deferred-removal-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// A child that lives about `seconds` and then ends by itself.
    fn short_lived(seconds: u32) -> std::process::Child {
        #[cfg(windows)]
        let mut command = {
            let mut command = crate::quiet_command_named(std::path::Path::new("ping.exe")).unwrap();
            command.args(["-n", &(seconds + 1).to_string(), "127.0.0.1"]);
            command
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut command = crate::quiet_command("/bin/sleep");
            command.arg(seconds.to_string());
            command
        };
        command.stdout(std::process::Stdio::null()).spawn().unwrap()
    }

    fn running(child: &std::process::Child) -> Running {
        let pid = child.id();
        Running {
            pid,
            started: crate::install_flip::started_of(pid).unwrap_or(0),
        }
    }

    /// Wait (bounded) until `done` holds.
    fn eventually(within: Duration, mut done: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < within {
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        done()
    }

    /// RED (T-UNINSTALL-UX) — **the remover removes nothing while a process it
    /// waits for runs, and once that process has ended it removes exactly the
    /// items it was handed, then the folder they emptied, and then its own
    /// script — and nothing beside them.**
    ///
    /// The real producer end to end: a real script in a real temporary folder,
    /// started through the real door, waiting on a real child that ends by
    /// itself a few seconds later. The folder holds a file and a directory
    /// with a file in it (the program and its update home), and a sibling of
    /// the folder stands outside it.
    ///
    /// MUTATION: in the Windows script, jump to `:remove` without the
    /// `tasklist` look (or, on Unix, drop the `kill -0` loop): the folder is
    /// gone while the child still runs.
    #[test]
    fn the_folder_goes_only_after_the_waited_process_ends_and_nothing_beside_it() {
        if cfg!(not(any(windows, unix))) {
            return;
        }
        let root = sandbox("waits");
        let folder = root.join("Folio");
        let home = folder.join(".folio-update");
        std::fs::create_dir_all(home.join("aa")).unwrap();
        std::fs::write(folder.join("folio.exe"), b"program").unwrap();
        std::fs::write(home.join("aa").join("journal.json"), b"{}").unwrap();
        let sibling = root.join("sibling.txt");
        std::fs::write(&sibling, b"keep").unwrap();
        let scripts = root.join("temp");
        std::fs::create_dir_all(&scripts).unwrap();

        let mut child = short_lived(3);
        let removal = Removal {
            after: vec![running(&child)],
            items: vec![
                Item::File(folder.join("folio.exe")),
                Item::Directory(home.clone()),
            ],
            folder: Some(folder.clone()),
        };
        schedule(&removal, &scripts).expect("the remover starts");

        std::thread::sleep(Duration::from_millis(1200));
        assert!(
            child.try_wait().unwrap().is_none(),
            "the child still runs, so what follows is a look while it runs"
        );
        assert!(
            folder.join("folio.exe").exists() && home.exists(),
            "nothing is removed while the waited process runs"
        );

        child.wait().unwrap();
        assert!(
            eventually(Duration::from_secs(30), || !folder.exists()),
            "the items and the folder they emptied are gone once the process has ended"
        );
        assert_eq!(std::fs::read(&sibling).unwrap(), b"keep");
        assert!(
            eventually(Duration::from_secs(10), || std::fs::read_dir(&scripts)
                .unwrap()
                .next()
                .is_none()),
            "the script deleted itself"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// RED (T-UNINSTALL-UX) — **a folder that still holds something the remover
    /// was not handed stays, with that something in it.**
    ///
    /// The person's own file beside `folio.exe` (a zip unpacked into
    /// `Downloads`): the program's files go, the folder and the file stay.
    ///
    /// MUTATION: remove the folder with `rd /s /q` (Unix: `rm -rf`) instead of
    /// `rd` alone (`rmdir`).
    #[test]
    fn a_folder_with_a_file_nobody_named_keeps_it() {
        if cfg!(not(any(windows, unix))) {
            return;
        }
        let root = sandbox("keeps");
        let folder = root.join("Downloads");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("folio.exe"), b"program").unwrap();
        std::fs::write(folder.join("thesis.pdf"), b"mine").unwrap();
        let scripts = root.join("temp");
        std::fs::create_dir_all(&scripts).unwrap();

        let removal = Removal {
            after: Vec::new(),
            items: vec![Item::File(folder.join("folio.exe"))],
            folder: Some(folder.clone()),
        };
        schedule(&removal, &scripts).expect("the remover starts");
        assert!(
            eventually(Duration::from_secs(30), || !folder
                .join("folio.exe")
                .exists()),
            "the program's file goes"
        );
        assert!(
            eventually(Duration::from_secs(10), || std::fs::read_dir(&scripts)
                .unwrap()
                .next()
                .is_none()),
            "the script has finished"
        );
        assert_eq!(std::fs::read(folder.join("thesis.pdf")).unwrap(), b"mine");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// PIN (T-UNINSTALL-UX) — **the Windows script's text is this module's own
    /// words and numbers: every path and name is an environment variable.**
    ///
    /// What keeps a `%`, a `&` or a non-ASCII letter in a path from changing
    /// what the script runs is that no path is ever in its text.
    ///
    /// MUTATION: write an item's path into the script instead of `%FOLIO_REMOVAL_ITEM_n%`.
    #[test]
    fn the_windows_script_names_every_path_by_a_variable() {
        let script = windows_script(2, &[false, true], true);
        assert!(script.is_ascii());
        for needed in [
            "%FOLIO_REMOVAL_PID_1%",
            "%FOLIO_REMOVAL_IMAGE_2%",
            "del /a /f /q \"%FOLIO_REMOVAL_ITEM_1%\"",
            "rd /s /q \"%FOLIO_REMOVAL_ITEM_2%\"",
            "rd \"%FOLIO_REMOVAL_FOLDER%\"",
            "del /f /q \"%~f0\"",
        ] {
            assert!(script.contains(needed), "{needed}\n{script}");
        }
        assert!(!script.contains("rd /s /q \"%FOLIO_REMOVAL_FOLDER%\""));
        // Every program by its path: a `find` earlier on `PATH` (Git's) never answers.
        for program in ["tasklist.exe", "find.exe", "PING.EXE"] {
            assert!(
                script.contains(&format!("\"%SystemRoot%\\System32\\{program}\"")),
                "{program}"
            );
        }
        assert!(UNIX_SCRIPT.contains("/bin/rm -rf -- \"$item\""));
        assert!(UNIX_SCRIPT.contains("/bin/rmdir -- \"$FOLIO_REMOVAL_FOLDER\""));
    }
}
