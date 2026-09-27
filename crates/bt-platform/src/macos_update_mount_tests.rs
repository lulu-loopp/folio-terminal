//! Tests of the image mount (0.4.6 ticket U-17): the grammar read from
//! `hdiutil attach`, every exit road of an attach and a use detaching what it
//! attached (a stand-in `hdiutil` that keeps its own mount table), the attach's
//! deadline, and on macOS a real image attached under a temporary home and
//! found in the real mount table without any record.

use super::*;

/// What `hdiutil attach -nobrowse -readonly -noautoopen -mountrandom` printed
/// on macOS 26 for an APFS image made with `hdiutil create -srcfolder` (the
/// checksum lines in English here; the machine said them in its own
/// language), with the home and the random name made up.
const APFS: &str = "Checksumming Protective Master Boot Record (MBR : 0)\u{2026}\n\
    Protective Master Boot Record (MBR :: verified   CRC32 $23DA1B04\n\
    Checksumming GPT Header (Primary GPT Header : 1)\u{2026}\n\
    \x20GPT Header (Primary GPT Header : 1): verified   CRC32 $20EF7009\n\
    Checksumming disk image (Apple_APFS : 4)\u{2026}\n\
    \x20           disk image (Apple_APFS : 4): verified   CRC32 $E1E9C630\n\
    verified   CRC32 $FA4A591A\n\
    /dev/disk4          \tGUID_partition_scheme          \t\n\
    /dev/disk4s1        \tApple_APFS                     \t\n\
    /dev/disk5          \tEF57347C-0000-11AA-AA11-0030654\t\n\
    /dev/disk5s1        \t41504653-0000-11AA-AA11-0030654\t/Applications/.Folio.app.folio-update/00112233445566778899aabbccddeeff/mnt/dmg.Gb7xig\n";

/// The same for an HFS+ image made with `hdiutil create -size 2m -fs HFS+`.
const HFS: &str = "/dev/disk4          \tGUID_partition_scheme          \t\n\
    /dev/disk4s1        \tApple_HFS                      \t/Applications/.Folio.app.folio-update/00112233445566778899aabbccddeeff/mnt/dmg.qqOut4\n";

/// RED (U-17) — **the mount point is read from `hdiutil attach`'s device
/// table: the third tab-separated column of a `/dev/` line, to the end of the
/// line, and nothing from the checksum lines before it.**
///
/// The home beside a bundle can hold a space or a tab (`/Volumes/My Apps/`),
/// so the column is taken whole rather than split on whitespace; an APFS image
/// lists its synthesized container, which has no mount point.
///
/// MUTATION: in `mount_points`, take `split_whitespace().last()` of each
/// `/dev/` line instead of the third tab-separated column.
#[test]
fn the_mount_point_is_parsed_from_hdiutil() {
    let home = "/Applications/.Folio.app.folio-update/00112233445566778899aabbccddeeff/mnt";
    assert_eq!(
        mount_points(APFS),
        vec![PathBuf::from(format!("{home}/dmg.Gb7xig"))]
    );
    assert_eq!(
        mount_points(HFS),
        vec![PathBuf::from(format!("{home}/dmg.qqOut4"))]
    );
    let spaced = HFS.replace("/Applications/", "/Volumes/My Apps\tand more/");
    assert_eq!(
        mount_points(&spaced),
        vec![PathBuf::from(
            "/Volumes/My Apps\tand more/.Folio.app.folio-update/00112233445566778899aabbccddeeff/mnt/dmg.qqOut4"
        )]
    );
    assert_eq!(
        mount_points("/dev/disk4\tGUID_partition_scheme\t\n"),
        Vec::<PathBuf>::new()
    );
    assert_eq!(
        mount_points("hdiutil: attach failed - no mountable file systems\n"),
        Vec::<PathBuf>::new()
    );
}

/// RED (U-17) — **a mount belongs to a root only when it lies strictly below
/// it**: the root itself, a sibling that shares its spelling's prefix, and
/// anything elsewhere are not the home's.
///
/// MUTATION: in `is_below`, compare the strings with `str::starts_with`
/// (so `H.other/…` counts as under `H`).
#[test]
fn only_a_mount_strictly_below_the_home_is_the_homes() {
    let home = Path::new("/Applications/.Folio.app.folio-update");
    assert!(is_below(
        Path::new("/Applications/.Folio.app.folio-update/t/mnt/dmg.1"),
        home
    ));
    assert!(!is_below(home, home));
    assert!(!is_below(
        Path::new("/Applications/.Folio.app.folio-update-other/t/mnt/dmg.1"),
        home
    ));
    assert!(!is_below(Path::new("/Volumes/Folio"), home));
}

/// Run `body` on a thread the thread door starts, with its capability, and
/// hand its answer (or its panic) back.
fn on_a_worker<T: Send + 'static>(
    body: impl FnOnce(&crate::admission::WorkerCtx) -> T + Send + 'static,
) -> T {
    let worker = crate::spawn_at_priority("bt-u17-test", crate::ThreadPriority::BelowNormal, body)
        .expect("the thread door starts a thread");
    match worker.join() {
        Ok(answer) => answer,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// RED (U-17) — **off macOS the mount road is refused by name**, before any
/// child starts.
///
/// MUTATION: drop the platform check at the top of `attach`.
#[test]
fn off_macos_the_mount_is_refused_by_name() {
    if crate::host_platform() == HostPlatform::MacOs {
        return;
    }
    on_a_worker(|worker| {
        let refused = attach(worker, Path::new("Folio.dmg"), Path::new("mnt"));
        assert!(matches!(refused, Err(Refusal::NotHere)), "{refused:?}");
        assert!(refused.unwrap_err().to_string().starts_with("macos_update"));
        assert!(matches!(
            mounts_under(Path::new("home")),
            Err(Refusal::NotHere)
        ));
        assert!(matches!(
            detach_all_under(worker, Path::new("home")),
            Err(Refusal::NotHere)
        ));
        let used = with_image(worker, Path::new("Folio.dmg"), Path::new("mnt"), |_| {
            Ok::<(), ()>(())
        });
        assert!(matches!(
            used.outcome,
            Err(Failed::Attach(Refusal::NotHere))
        ));
    });
}

/// A stand-in `hdiutil` (a shell script) with its own mount table: `attach`
/// makes a directory under `-mountrandom`'s directory and writes it to
/// `table`; `detach` takes it out again. Every call is appended to `calls`.
/// What each verb does is read from the files `attach` and `detach`.
#[cfg(unix)]
mod stand_in {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const SCRIPT: &str = r#"#!/bin/sh
state='@STATE@'
printf '%s\n' "$*" >> "$state/calls"
verb=$1
shift
if [ "$verb" = attach ]; then
  dir=
  while [ $# -gt 1 ]; do
    if [ "$1" = -mountrandom ]; then dir=$2; shift; fi
    shift
  done
  mode=$(cat "$state/attach")
  if [ "$mode" = refuse ]; then
    echo 'hdiutil: attach failed - no mountable file systems' >&2
    exit 1
  fi
  point="$dir/dmg.$$"
  if [ "$mode" = outside ]; then point="$state/elsewhere"; fi
  mkdir -p "$point"
  printf '%s\n' "$point" >> "$state/table"
  if [ "$mode" = twice ]; then
    mkdir -p "$point.b"
    printf '%s\n' "$point.b" >> "$state/table"
  fi
  case "$mode" in
    ok|outside) printf 'Checksumming whole disk\n/dev/disk9          \tGUID_partition_scheme          \t\n/dev/disk9s1        \tApple_HFS                      \t%s\n' "$point" ;;
    twice) printf '/dev/disk9s1\tApple_HFS\t%s\n/dev/disk9s2\tApple_HFS\t%s.b\n' "$point" "$point" ;;
    garbled) echo 'attached, somewhere' ;;
    dies) echo 'hdiutil: attach failed after mounting' >&2; exit 1 ;;
    hangs) echo $$ > "$state/pid"; exec sleep 30 ;;
  esac
  exit 0
fi
force=
if [ "$1" = -force ]; then force=1; shift; fi
point=$1
mode=$(cat "$state/detach")
if [ "$mode" = busy ] || { [ "$mode" = busy-once ] && [ -z "$force" ]; }; then
  echo 'hdiutil: could not unmount - Resource busy' >&2
  exit 16
fi
grep -vxF "$point" "$state/table" > "$state/table.next"
mv "$state/table.next" "$state/table"
rmdir "$point"
echo '"disk9" ejected.'
"#;

    /// Short deadlines: a stand-in answers in milliseconds.
    pub(super) const QUICK: Within = Within {
        attach: Duration::from_secs(10),
        detach: Duration::from_secs(10),
        busy: Duration::from_millis(50),
    };

    pub(super) struct Stand {
        pub(super) root: PathBuf,
        pub(super) state: PathBuf,
        pub(super) tool: PathBuf,
        pub(super) image: PathBuf,
        /// `H/<txn>/mnt` of a made-up home.
        pub(super) mount_dir: PathBuf,
    }

    impl Stand {
        pub(super) fn new(tag: &str) -> Self {
            let root = std::fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .join(format!("bt-u17-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let state = root.join("state");
            let mount_dir = root.join("H/txn/mnt");
            std::fs::create_dir_all(&state).unwrap();
            std::fs::create_dir_all(&mount_dir).unwrap();
            let tool = root.join("hdiutil");
            std::fs::write(
                &tool,
                SCRIPT.replace("@STATE@", state.to_str().expect("a UTF-8 temp dir")),
            )
            .unwrap();
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
            let image = root.join("Folio.dmg");
            std::fs::write(&image, b"not really an image").unwrap();
            std::fs::write(state.join("table"), b"").unwrap();
            let stand = Self {
                root,
                state,
                tool,
                image,
                mount_dir,
            };
            stand.set("ok", "ok");
            stand
        }

        /// What the next `attach` and `detach` do.
        pub(super) fn set(&self, attach: &str, detach: &str) {
            std::fs::write(self.state.join("attach"), attach).unwrap();
            std::fs::write(self.state.join("detach"), detach).unwrap();
            std::fs::write(self.state.join("calls"), b"").unwrap();
        }

        pub(super) fn calls(&self) -> Vec<String> {
            std::fs::read_to_string(self.state.join("calls"))
                .unwrap()
                .lines()
                .map(str::to_owned)
                .collect()
        }

        /// The stand-in's mount table.
        pub(super) fn table(&self) -> Vec<PathBuf> {
            std::fs::read_to_string(self.state.join("table"))
                .unwrap()
                .lines()
                .map(PathBuf::from)
                .collect()
        }

        /// [`points_under`], over the stand-in's table.
        pub(super) fn under(&self, root: &Path) -> Result<Vec<PathBuf>, Refusal> {
            let root = std::fs::canonicalize(root).map_err(Refusal::MountTable)?;
            Ok(self
                .table()
                .into_iter()
                .filter(|point| is_below(point, &root))
                .collect())
        }

        /// What is mounted under the home, by the stand-in's table.
        pub(super) fn mounted_in_home(&self) -> Vec<PathBuf> {
            self.under(&self.root.join("H")).unwrap()
        }
    }

    impl Drop for Stand {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

/// RED (U-17) — **every exit road of an image's use ends with what it attached
/// detached**: the body's answer and its refusal; `hdiutil` that refuses
/// before mounting, that mounts and then fails, that mounts and says nothing
/// this reads, that mounts two volumes; a detach refused as busy once (then
/// `-force`); and a detach refused for good, which is debt the home's sweep
/// clears. A mount that is not under the mount directory (an image already
/// attached elsewhere) is somebody else's and is never detached.
///
/// The stand-in keeps its own mount table, so "nothing left mounted" is read
/// from the same table the attach wrote — the fake's version of `getfsstat`.
///
/// MUTATION: in `attach_with`, return `Refusal::MountPoints` without passing
/// it through `refused` (no sweep when `hdiutil` said something unreadable).
#[cfg(unix)]
#[test]
fn every_exit_path_detaches_what_it_attached() {
    use stand_in::{QUICK, Stand};
    let stand = Stand::new("roads");
    let run = |attach: &str,
               detach: &str,
               body_ok: bool|
     -> (Used<&'static str, &'static str>, Vec<String>) {
        stand.set(attach, detach);
        let table = |root: &Path| stand.under(root);
        let tools = Tools {
            hdiutil: &stand.tool,
            table: &table,
            within: QUICK,
        };
        let mut seen = None;
        let used = with_image_with(&tools, &stand.image, &stand.mount_dir, |point| {
            seen = Some(point.to_path_buf());
            assert!(point.is_dir(), "the body is handed a mounted directory");
            if body_ok {
                Ok("verified")
            } else {
                Err("verification refused")
            }
        });
        if let Some(point) = seen {
            assert!(
                stand
                    .calls()
                    .contains(&format!("detach {}", point.display()))
                    || stand
                        .calls()
                        .contains(&format!("detach -force {}", point.display())),
                "{attach}/{detach}: the mount handed to the body is detached: {:?}",
                stand.calls()
            );
        }
        (used, stand.calls())
    };
    let attached = |calls: &[String]| calls.iter().filter(|c| c.starts_with("attach ")).count();
    let detached = |calls: &[String]| calls.iter().filter(|c| c.starts_with("detach ")).count();

    // The body answers, and the body refuses: the image comes off either way.
    let (used, calls) = run("ok", "ok", true);
    assert!(matches!(used.outcome, Ok("verified")), "{used:?}");
    assert!(used.detached.is_ok(), "{used:?}");
    assert_eq!((attached(&calls), detached(&calls)), (1, 1), "{calls:?}");
    assert_eq!(stand.mounted_in_home(), Vec::<PathBuf>::new());

    let (used, calls) = run("ok", "ok", false);
    assert!(
        matches!(used.outcome, Err(Failed::Body("verification refused"))),
        "{used:?}"
    );
    assert!(used.detached.is_ok(), "{used:?}");
    assert_eq!(detached(&calls), 1, "{calls:?}");
    assert_eq!(stand.mounted_in_home(), Vec::<PathBuf>::new());

    // `hdiutil` refuses before it mounts anything: nothing to detach.
    let (used, calls) = run("refuse", "ok", true);
    assert!(
        matches!(used.outcome, Err(Failed::Attach(Refusal::Program { .. }))),
        "{used:?}"
    );
    assert_eq!(detached(&calls), 0, "{calls:?}");
    assert_eq!(stand.mounted_in_home(), Vec::<PathBuf>::new());

    // It mounts and then fails; it mounts and says nothing readable; it mounts
    // two volumes: each is found under the mount directory and detached.
    for (mode, mounts) in [("dies", 1), ("garbled", 1), ("twice", 2)] {
        let (used, calls) = run(mode, "ok", true);
        assert!(
            matches!(used.outcome, Err(Failed::Attach(_))),
            "{mode}: {used:?}"
        );
        assert!(used.detached.is_ok(), "{mode}: {used:?}");
        assert_eq!(detached(&calls), mounts, "{mode}: {calls:?}");
        assert_eq!(stand.mounted_in_home(), Vec::<PathBuf>::new(), "{mode}");
    }

    // A detach refused as busy waits, then forces once.
    let (used, calls) = run("ok", "busy-once", true);
    assert!(used.detached.is_ok(), "{used:?}");
    assert_eq!(detached(&calls), 2, "{calls:?}");
    assert!(
        calls.last().unwrap().starts_with("detach -force "),
        "{calls:?}"
    );
    assert_eq!(stand.mounted_in_home(), Vec::<PathBuf>::new());

    // A detach refused for good is debt: named, left under the home, and
    // cleared by the home's sweep once the volume lets go.
    let (used, calls) = run("ok", "busy", true);
    assert!(matches!(used.outcome, Ok("verified")), "{used:?}");
    assert!(
        matches!(
            used.detached,
            Err(Refusal::Program {
                program: HDIUTIL,
                ..
            })
        ),
        "{used:?}"
    );
    assert_eq!(detached(&calls), 2, "{calls:?}");
    assert_eq!(stand.mounted_in_home().len(), 1);
    stand.set("ok", "ok");
    let table = |root: &Path| stand.under(root);
    let tools = Tools {
        hdiutil: &stand.tool,
        table: &table,
        within: QUICK,
    };
    detach_all_with(&tools, &stand.root.join("H")).unwrap();
    assert_eq!(stand.mounted_in_home(), Vec::<PathBuf>::new());

    // An attach that fails while its sweep is refused says both.
    let (used, _) = run("dies", "busy", true);
    assert!(
        matches!(
            used.outcome,
            Err(Failed::Attach(Refusal::Undetached { ref sweep, .. }))
                if matches!(**sweep, Refusal::LeftMounted(ref left) if left.len() == 1)
        ),
        "{used:?}"
    );
    stand.set("ok", "ok");
    detach_all_with(&tools, &stand.root.join("H")).unwrap();
    assert_eq!(stand.mounted_in_home(), Vec::<PathBuf>::new());

    // An image already mounted elsewhere is answered with that mount point:
    // not ours, refused, and left alone.
    let (used, calls) = run("outside", "ok", true);
    assert!(
        matches!(
            used.outcome,
            Err(Failed::Attach(Refusal::MountPoints { .. }))
        ),
        "{used:?}"
    );
    assert_eq!(detached(&calls), 0, "{calls:?}");
    assert_eq!(stand.table(), vec![stand.state.join("elsewhere")]);
}

/// RED (U-17) — **an attach that overruns its deadline is refused as
/// `TimedOut`, its child is ended (not left running), and what it mounted
/// before it was ended is detached.**
///
/// The stand-in mounts, writes its pid, and `exec`s `sleep 30` in place, so
/// the pid it wrote is the process the deadline has to end; after the refusal
/// `kill(pid, 0)` answers `ESRCH`. The call returns within the deadline plus
/// the sweep, far inside the thirty seconds the stand-in would sleep.
///
/// MUTATION: in `run_at`, wait for the child with no deadline
/// (`child.wait()` in place of the `try_wait` loop).
#[cfg(unix)]
#[test]
fn attach_is_bounded() {
    use stand_in::{QUICK, Stand};
    let stand = Stand::new("bounded");
    stand.set("hangs", "ok");
    let table = |root: &Path| stand.under(root);
    let tools = Tools {
        hdiutil: &stand.tool,
        table: &table,
        within: Within {
            attach: Duration::from_secs(2),
            ..QUICK
        },
    };
    let started = Instant::now();
    let refused = attach_with(&tools, &stand.image, &stand.mount_dir);
    let took = started.elapsed();
    assert!(
        matches!(
            refused,
            Err(Refusal::TimedOut {
                program: HDIUTIL,
                ..
            })
        ),
        "{refused:?}"
    );
    assert!(took < Duration::from_secs(10), "{took:?}");
    let pid: libc::pid_t = std::fs::read_to_string(stand.state.join("pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // SAFETY: signal 0 sends nothing; it asks whether `pid` exists.
    let alive = unsafe { libc::kill(pid, 0) };
    assert_eq!(
        (alive, io::Error::last_os_error().raw_os_error()),
        (-1, Some(libc::ESRCH)),
        "the stand-in's pid {pid} is not left running"
    );
    assert_eq!(stand.mounted_in_home(), Vec::<PathBuf>::new());
    assert!(
        stand.calls().iter().any(|call| call.starts_with("detach ")),
        "{:?}",
        stand.calls()
    );
}

/// The macOS half: real `hdiutil`, a tiny image made by `hdiutil create` in
/// the test's own temporary directory, attached under a temporary home and
/// never anywhere else. Each test detaches what it attached even when it
/// fails (the [`Scratch`] guard), and checks the real mount table before and
/// after.
#[cfg(target_os = "macos")]
mod real {
    use super::*;

    /// A temporary home with a small image beside it. Dropped — on success or
    /// on a failed assertion — it force-detaches whatever the mount table
    /// still lists under it, then removes it.
    struct Scratch {
        root: PathBuf,
        home: PathBuf,
        mount_dir: PathBuf,
        image: PathBuf,
    }

    impl Scratch {
        fn new(tag: &str) -> Self {
            let root = std::fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .join(format!("bt-u17-real-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let home = root.join(".Folio.app.folio-update");
            let mount_dir = home.join("00112233445566778899aabbccddeeff/mnt");
            std::fs::create_dir_all(&mount_dir).unwrap();
            let image = root.join("test.dmg");
            let output = crate::quiet_command(HDIUTIL)
                .args([
                    "create", "-size", "2m", "-fs", "HFS+", "-volname", "U17", "-quiet",
                ])
                .arg(&image)
                .output()
                .unwrap();
            assert!(output.status.success(), "{}", said(&output));
            let scratch = Self {
                root,
                home,
                mount_dir,
                image,
            };
            assert_eq!(
                in_the_table_under(&scratch.root),
                Vec::<PathBuf>::new(),
                "nothing is mounted under a new scratch"
            );
            scratch
        }
    }

    /// The mount table's mount points under `root`, read without the filter
    /// the tests exercise, so a mutation of that filter cannot hide a mount
    /// from the clean-up or from the final check.
    fn in_the_table_under(root: &Path) -> Vec<PathBuf> {
        mounted()
            .expect("the mount table")
            .into_iter()
            .filter(|point| point.starts_with(root))
            .collect()
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            for point in in_the_table_under(&self.root) {
                let _ = crate::quiet_command(HDIUTIL)
                    .args(["detach", "-force"])
                    .arg(&point)
                    .output();
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// RED (U-17) — **a mount under the home is found from the mount table
    /// without any record, and detached from there.** F-17 and M1: the image
    /// is attached with `-mountrandom <home>/<txn>/mnt`; a process that died
    /// between the attach and writing its answer down has left a mount nobody
    /// recorded, and the next actor detaches it before it deletes `H/<txn>`.
    ///
    /// The attach here is `hdiutil` itself, run the way a Prepare would, with
    /// its answer thrown away; the mount is then found by `mounts_under(home)`
    /// alone, detached by `detach_all_under(home)`, and the real mount table
    /// is read again: nothing under the home.
    ///
    /// MUTATION: in `points_under`, keep a mount point only when its parent is
    /// `root` (`point.parent() == Some(&root)`) instead of `is_below` — the
    /// mount lies two levels down, at `<home>/<txn>/mnt/dmg.XXXXXX`, and is not
    /// found.
    #[test]
    fn a_mount_under_the_home_is_found_without_a_record() {
        let scratch = Scratch::new("found");
        let output = crate::quiet_command(HDIUTIL)
            .args([
                "attach",
                "-nobrowse",
                "-readonly",
                "-noautoopen",
                "-mountrandom",
            ])
            .arg(&scratch.mount_dir)
            .arg(&scratch.image)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", said(&output));
        drop(output); // no record: the answer is never read

        let found = mounts_under(&scratch.home).unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with(&scratch.mount_dir), "{found:?}");
        assert!(found[0].join(".").is_dir());
        assert_eq!(
            mounts_under(&scratch.root.join("elsewhere")).unwrap(),
            Vec::<PathBuf>::new()
        );

        let home = scratch.home.clone();
        on_a_worker(move |worker| detach_all_under(worker, &home)).unwrap();
        assert_eq!(
            in_the_table_under(&scratch.root),
            Vec::<PathBuf>::new(),
            "the mount table lists nothing under the scratch after the sweep"
        );
    }

    /// RED (U-17) — **a real image is attached under its mount directory,
    /// handed to the body read-only, and detached when the body is done.**
    /// The one real attach of the note's U-12 row, through the product road.
    ///
    /// MUTATION: in `with_image_with`, answer without `detach_with` (the
    /// `Mount` is dropped): the mount table still lists it afterwards.
    #[test]
    fn a_real_image_is_attached_under_its_mount_dir_and_detached() {
        let scratch = Scratch::new("used");
        let (image, mount_dir) = (scratch.image.clone(), scratch.mount_dir.clone());
        let used = on_a_worker(move |worker| {
            let used = with_image(worker, &image, &mount_dir, |point| {
                let listed = mounts_under(&mount_dir).map_err(|refusal| refusal.to_string())?;
                let refused = std::fs::write(point.join("written"), b"x").is_err();
                Ok::<_, String>((point.to_path_buf(), listed, refused))
            });
            (
                used.outcome.map_err(|failed| format!("{failed:?}")),
                used.detached.map_err(|r| r.to_string()),
            )
        });
        let (point, listed, refused) = used.0.unwrap();
        used.1.unwrap();
        assert!(point.starts_with(&scratch.mount_dir), "{point:?}");
        assert_eq!(listed, vec![point.clone()], "the body sees it in the table");
        assert!(refused, "the image is mounted read-only");
        assert_eq!(
            in_the_table_under(&scratch.root),
            Vec::<PathBuf>::new(),
            "detached when the body is done"
        );
    }
}
