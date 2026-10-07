//! Linux file hand-off and trash operations.
//!
//! The desktop session chooses the default application. `gio` speaks the
//! desktop's file and trash APIs; `xdg-open` is the fallback when its command
//! is absent. A file reveal first asks the standard FileManager1 D-Bus method
//! to select the item, then opens its parent directory if that method is not
//! available. Open and reveal are reached from `ShellThread`; trash is a
//! worker-only transaction because its result changes application-owned state.

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};

use crate::NativeWindow;
use crate::admission::WorkerCtx;
use crate::handoff::{PROGRAM_REFUSED, VerifiedTarget, openable_unix_path};

/// Open a decoded local picture with its registered default application.
pub(crate) fn open_local_file(
    worker: &WorkerCtx,
    window: NativeWindow,
    path: &Path,
) -> Result<(), String> {
    let _ = window;
    openable_unix_path(path)?;
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if !crate::IMAGE_FILE_EXTENSIONS.contains(&extension.as_str()) {
        return Err("local image path extension is not supported".to_owned());
    }
    open_with_desktop(worker, path)
}

/// Open an address that already passed the caller's scheme policy.
pub(crate) fn shell_execute(
    worker: &WorkerCtx,
    window: NativeWindow,
    target: &str,
) -> Result<(), String> {
    let _ = window;
    if target.contains('\0') {
        return Err("address contains an embedded NUL".to_owned());
    }
    open_with_programs(
        worker,
        OsStr::new("gio"),
        OsStr::new("xdg-open"),
        OsStr::new(target),
        target,
        &[],
    )
}

/// Open a picked file or directory, refusing files that would run as programs.
pub(crate) fn open_local_path(
    worker: &WorkerCtx,
    window: NativeWindow,
    path: &Path,
) -> Result<(), String> {
    let _ = window;
    let resolved = openable_target(worker, path)?;
    open_with_desktop(worker, &resolved)
}

/// Open a target already checked by the path worker.
pub(crate) fn open_local_path_verified(
    worker: &WorkerCtx,
    window: NativeWindow,
    path: &Path,
    target: VerifiedTarget,
) -> Result<(), String> {
    let _ = window;
    openable_unix_path(path)?;
    if !target.exists {
        return Err(format!("{path:?}: not there"));
    }
    if target.executable {
        return Err(PROGRAM_REFUSED.to_owned());
    }
    let resolved = target.resolved.unwrap_or_else(|| path.to_path_buf());
    open_with_desktop(worker, &resolved)
}

/// Show a path in the file manager.
pub(crate) fn reveal_in_explorer(
    worker: &WorkerCtx,
    window: NativeWindow,
    path: &Path,
) -> Result<(), String> {
    let _ = window;
    openable_unix_path(path)?;
    let resolved = std::fs::canonicalize(path).map_err(|error| format!("{path:?}: {error}"))?;
    let metadata =
        std::fs::metadata(&resolved).map_err(|error| format!("{resolved:?}: {error}"))?;
    reveal_resolved(worker, &resolved, metadata.is_dir())
}

/// Show a target already checked by the path worker.
pub(crate) fn reveal_verified(
    worker: &WorkerCtx,
    window: NativeWindow,
    path: &Path,
    target: VerifiedTarget,
) -> Result<(), String> {
    let _ = window;
    if !target.exists {
        return Err(format!("{path:?}: not there"));
    }
    openable_unix_path(path)?;
    let resolved = target.resolved.unwrap_or_else(|| path.to_path_buf());
    reveal_resolved(worker, &resolved, target.is_directory)
}

/// Move a file or directory to the freedesktop trash on its worker.
///
/// A successful `gio trash` is reversible through the desktop's trash. A
/// refusal stays an error; this door never substitutes permanent deletion.
pub fn recycle_on_worker(worker: &WorkerCtx, path: &Path) -> Result<bool, String> {
    openable_unix_path(path)?;
    recycle_with_environment(worker, path, &[]).map(|()| true)
}

fn openable_target(worker: &WorkerCtx, path: &Path) -> Result<PathBuf, String> {
    let _ = worker;
    openable_unix_path(path)?;
    let resolved = std::fs::canonicalize(path).map_err(|error| format!("{path:?}: {error}"))?;
    let metadata =
        std::fs::metadata(&resolved).map_err(|error| format!("{resolved:?}: {error}"))?;
    if !metadata.is_dir() && metadata.permissions().mode() & 0o111 != 0 {
        return Err(PROGRAM_REFUSED.to_owned());
    }
    Ok(resolved)
}

fn reveal_resolved(worker: &WorkerCtx, path: &Path, is_directory: bool) -> Result<(), String> {
    if is_directory {
        return open_with_desktop(worker, path);
    }

    match show_item_with_program(worker, OsStr::new("gdbus"), path, &[]) {
        Ok(()) => Ok(()),
        Err(dbus_error) => reveal_parent(worker, path).map_err(|open_error| {
            format!("{dbus_error}; could not open its folder either: {open_error}")
        }),
    }
}

fn reveal_parent(worker: &WorkerCtx, path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{path:?} has no containing folder"))?;
    open_with_desktop(worker, parent)
}

fn open_with_desktop(worker: &WorkerCtx, path: &Path) -> Result<(), String> {
    let subject = format!("{path:?}");
    open_with_programs(
        worker,
        OsStr::new("gio"),
        OsStr::new("xdg-open"),
        path.as_os_str(),
        &subject,
        &[],
    )
}

fn open_with_programs(
    worker: &WorkerCtx,
    gio: &OsStr,
    xdg_open: &OsStr,
    target: &OsStr,
    subject: &str,
    environment: &[(OsString, OsString)],
) -> Result<(), String> {
    let gio_args = [OsString::from("open"), target.to_owned()];
    match command_output(worker, gio, &gio_args, environment) {
        Ok(output) => command_result("gio", subject, output),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let xdg_args = [target.to_owned()];
            command_output(worker, xdg_open, &xdg_args, environment)
                .map_err(|error| format!("could not start xdg-open to open {subject}: {error}"))
                .and_then(|output| command_result("xdg-open", subject, output))
        }
        Err(error) => Err(format!("could not start gio to open {subject}: {error}")),
    }
}

fn recycle_with_environment(
    worker: &WorkerCtx,
    path: &Path,
    environment: &[(OsString, OsString)],
) -> Result<(), String> {
    let args = [
        OsString::from("trash"),
        OsString::from("--"),
        path.as_os_str().to_owned(),
    ];
    let subject = format!("{path:?}");
    command_output(worker, OsStr::new("gio"), &args, environment)
        .map_err(|error| format!("could not start gio to trash {path:?}: {error}"))
        .and_then(|output| command_result("gio trash", &subject, output))
}

fn command_output(
    worker: &WorkerCtx,
    program: &OsStr,
    arguments: &[OsString],
    environment: &[(OsString, OsString)],
) -> io::Result<Output> {
    let mut command = crate::quiet_command(program);
    command.args(arguments);
    command.envs(environment.iter().cloned());
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let readers = crate::linux_process::OutputReaders::start(worker, &mut child)?;
    let status = match child.wait() {
        Ok(status) => status,
        Err(error) => {
            crate::linux_process::kill_reap_and_join(worker, &mut child, readers);
            return Err(error);
        }
    };
    let (stdout, stderr) = readers.finish(worker).map_err(|error| {
        io::Error::other(format!("could not read desktop helper output: {error}"))
    })?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn command_result(program: &str, subject: &str, output: Output) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }
    Err(command_failure(program, subject, &output))
}

fn command_failure(program: &str, subject: &str, output: &Output) -> String {
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if detail.is_empty() {
        format!("{program} refused {subject} (status {})", output.status)
    } else {
        format!("{program} refused {subject}: {detail}")
    }
}

fn show_item_with_program(
    worker: &WorkerCtx,
    gdbus: &OsStr,
    path: &Path,
    environment: &[(OsString, OsString)],
) -> Result<(), String> {
    let uri = file_uri(path)?;
    let args = [
        OsString::from("call"),
        OsString::from("--session"),
        OsString::from("--dest"),
        OsString::from("org.freedesktop.FileManager1"),
        OsString::from("--object-path"),
        OsString::from("/org/freedesktop/FileManager1"),
        OsString::from("--method"),
        OsString::from("org.freedesktop.FileManager1.ShowItems"),
        OsString::from(format!("['{uri}']")),
        OsString::from("''"),
    ];
    let subject = format!("{path:?}");
    command_output(worker, gdbus, &args, environment)
        .map_err(|error| format!("could not start gdbus to reveal {subject}: {error}"))
        .and_then(|output| command_result("gdbus", &subject, output))
}

fn file_uri(path: &Path) -> Result<String, String> {
    openable_unix_path(path)?;
    if path.to_str().is_none() {
        return Err(format!("{path:?} cannot be represented as a file URI"));
    }
    Ok(bt_transcript::paths::local_path_to_file_uri(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::admission::WorkerCtx;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/linux-port-team/worker3-test")
                .join(format!(
                    "{label}-{}-{}",
                    std::process::id(),
                    NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
                ));
            std::fs::create_dir_all(&root).expect("create a disposable test tree under target");
            Self(root)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn helper(root: &Path, name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = root.join(name);
        std::fs::write(
            &path,
            "#!/bin/sh\nprintf '%s\\0' \"$@\" > \"$FOLIO_TEST_ARGS_FILE\"\nexit \"${FOLIO_TEST_EXIT:-0}\"\n",
        )
        .expect("write controlled helper");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("make controlled helper executable");
        path
    }

    fn arguments(path: &Path) -> Vec<OsString> {
        std::fs::read(path)
            .expect("helper recorded its argument vector")
            .split(|byte| *byte == 0)
            .filter(|argument| !argument.is_empty())
            .map(|argument| OsString::from_vec(argument.to_vec()))
            .collect()
    }

    fn on_worker<T: Send + 'static>(work: impl FnOnce(&WorkerCtx) -> T + Send + 'static) -> T {
        match crate::spawn_at_priority("linux-files-test", crate::ThreadPriority::Normal, work)
            .expect("start a controlled worker")
            .join()
        {
            Ok(answer) => answer,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    #[test]
    fn a_default_open_preserves_non_utf8_path_bytes_and_falls_back_when_gio_is_absent() {
        let scratch = Scratch::new("open-helper");
        let gio = helper(&scratch.0, "gio-helper");
        let xdg = helper(&scratch.0, "xdg-helper");
        let recorded = scratch.0.join("argv");
        let env = [(
            OsString::from("FOLIO_TEST_ARGS_FILE"),
            recorded.clone().into_os_string(),
        )];
        let path = scratch.0.join(PathBuf::from(OsString::from_vec(
            b"name with spaces-\xff.txt".to_vec(),
        )));

        let subject = format!("{path:?}");
        let absent_gio = scratch.0.join("gio-is-absent");
        on_worker(move |worker| {
            open_with_programs(
                worker,
                gio.as_os_str(),
                xdg.as_os_str(),
                path.as_os_str(),
                &subject,
                &env,
            )
            .expect("controlled gio accepts the default-open request");
            assert_eq!(
                arguments(&recorded),
                [OsString::from("open"), path.as_os_str().to_owned()]
            );

            open_with_programs(
                worker,
                absent_gio.as_os_str(),
                xdg.as_os_str(),
                path.as_os_str(),
                &subject,
                &env,
            )
            .expect("the xdg-open fallback accepts the request");
            assert_eq!(arguments(&recorded), [path.as_os_str().to_owned()]);
        });
    }

    #[test]
    fn a_file_reveal_uses_filemanager1_with_the_shared_uri_encoder() {
        let scratch = Scratch::new("reveal-helper");
        let gdbus = helper(&scratch.0, "gdbus-helper");
        let recorded = scratch.0.join("argv");
        let env = [(
            OsString::from("FOLIO_TEST_ARGS_FILE"),
            recorded.clone().into_os_string(),
        )];
        let path = PathBuf::from("/tmp/a é #b.txt");

        let uri = file_uri(&path).expect("encode an absolute local path");
        let args = [
            OsString::from("call"),
            OsString::from("--session"),
            OsString::from("--dest"),
            OsString::from("org.freedesktop.FileManager1"),
            OsString::from("--object-path"),
            OsString::from("/org/freedesktop/FileManager1"),
            OsString::from("--method"),
            OsString::from("org.freedesktop.FileManager1.ShowItems"),
            OsString::from(format!("['{uri}']")),
            OsString::from("''"),
        ];
        let non_utf8 = PathBuf::from(OsString::from_vec(b"/tmp/a \xff.txt".to_vec()));
        on_worker(move |worker| {
            show_item_with_program(worker, gdbus.as_os_str(), &path, &env)
                .expect("the controlled D-Bus helper accepts the reveal request");
            assert_eq!(arguments(&recorded), args.to_vec());
            assert_eq!(uri, "file:///tmp/a%20%C3%A9%20%23b.txt");
            assert!(
                file_uri(&non_utf8).is_err(),
                "lossy paths must not reach FileManager1"
            );
        });
    }

    #[test]
    fn a_resolved_executable_is_refused_before_the_desktop_is_called() {
        let scratch = Scratch::new("executable-policy");
        let program = scratch.0.join("payload");
        std::fs::write(&program, b"not a program body").expect("make an executable file");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
            .expect("mark the file executable");

        on_worker(move |worker| {
            assert_eq!(
                openable_target(worker, &program),
                Err(PROGRAM_REFUSED.to_owned())
            );
            assert!(
                openable_target(worker, &scratch.0).is_ok(),
                "a directory is not a program"
            );
            assert_eq!(
                open_local_path_verified(
                    worker,
                    NativeWindow::stand_in(0),
                    &program,
                    VerifiedTarget {
                        exists: true,
                        is_directory: false,
                        executable: true,
                        resolved: Some(program.clone()),
                    }
                ),
                Err(PROGRAM_REFUSED.to_owned())
            );
        });
    }

    #[test]
    fn shell_execute_refuses_embedded_nul_before_starting_a_desktop_command() {
        on_worker(|worker| {
            assert_eq!(
                shell_execute(
                    worker,
                    NativeWindow::stand_in(0),
                    "https://example.invalid/\0file",
                ),
                Err("address contains an embedded NUL".to_owned())
            );
        });
    }

    #[test]
    fn recycle_keeps_its_existing_absolute_path_rule_on_the_worker() {
        on_worker(|worker| {
            assert_eq!(
                recycle_on_worker(worker, Path::new("relative")),
                Err("path must be absolute".to_owned())
            );
        });
    }

    #[test]
    fn recycle_moves_files_and_whole_folders_into_a_private_freedesktop_trash() {
        let scratch = Scratch::new("private-trash");
        let data_home = scratch.0.join("data");
        let home = scratch.0.join("home");
        let source = scratch.0.join("source");
        std::fs::create_dir_all(&source).expect("make a source directory on the test filesystem");
        std::fs::create_dir_all(&data_home).expect("make a private XDG data home");
        std::fs::create_dir_all(&home).expect("make a private home");

        let file = source.join("recoverable note.txt");
        std::fs::write(&file, b"kept in the private trash").expect("make a file to trash");
        let non_utf8 = source.join(PathBuf::from(OsString::from_vec(
            b"recoverable-\xff.txt".to_vec(),
        )));
        std::fs::write(&non_utf8, b"kept under its original byte name")
            .expect("make a non-UTF8 file to trash");
        let folder = source.join("whole folder");
        std::fs::create_dir_all(folder.join("nested")).expect("make a nested folder");
        std::fs::write(folder.join("nested/child.txt"), b"kept with its parent")
            .expect("make a child file");

        let env = [
            (
                OsString::from("XDG_DATA_HOME"),
                data_home.clone().into_os_string(),
            ),
            (OsString::from("HOME"), home.into_os_string()),
        ];
        let file_for_worker = file.clone();
        let non_utf8_for_worker = non_utf8.clone();
        let folder_for_worker = folder.clone();
        on_worker(move |worker| {
            recycle_with_environment(worker, &file_for_worker, &env)
                .expect("gio moves the file into the private trash");
            recycle_with_environment(worker, &non_utf8_for_worker, &env)
                .expect("gio moves a non-UTF8 file without changing its name bytes");
            recycle_with_environment(worker, &folder_for_worker, &env)
                .expect("gio moves the directory as one item");
        });

        let trashed_files = data_home.join("Trash/files");
        assert!(!file.exists(), "the original file has moved");
        assert!(!non_utf8.exists(), "the non-UTF8 original file has moved");
        assert!(!folder.exists(), "the original folder has moved");
        assert_eq!(
            std::fs::read(trashed_files.join("recoverable note.txt"))
                .expect("the file is recoverable under the private trash"),
            b"kept in the private trash"
        );
        assert_eq!(
            std::fs::read(
                trashed_files.join(PathBuf::from(
                    non_utf8
                        .file_name()
                        .expect("the non-UTF8 path has a name")
                        .to_os_string(),
                ))
            )
            .expect("the non-UTF8 filename is recoverable under the private trash"),
            b"kept under its original byte name"
        );
        assert_eq!(
            std::fs::read(trashed_files.join("whole folder/nested/child.txt"))
                .expect("the folder and its child remain together"),
            b"kept with its parent"
        );
        assert!(
            data_home
                .join("Trash/info/recoverable note.txt.trashinfo")
                .is_file()
        );
    }
}
