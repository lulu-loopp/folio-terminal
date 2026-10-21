//! The turns a check of a packaged program and a pane's birth of it take (`packaged_turns`).

use super::*;
use crate::lane::{Hold, receive, wait_until};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// The Store PowerShell's family, as the alias names it.
fn powershell() -> PackageFamily {
    PackageFamily::named("Microsoft.PowerShell_8wekyb3d8bbwe")
}

/// A process-lifetime set of turns of a test's own, shared with the workers it starts.
fn turns() -> &'static PackagedTurns {
    Box::leak(Box::new(PackagedTurns::new()))
}

/// Start `body` on a worker the thread door started.
fn on_a_worker(name: &'static str, body: impl FnOnce(&WorkerCtx) + Send + 'static) {
    bt_platform::spawn_at_priority(name, bt_platform::ThreadPriority::BelowNormal, body)
        .expect("the thread door starts a thread");
}

/// The lines a birth said, kept for the test.
fn lines() -> (Arc<Mutex<Vec<String>>>, impl Fn(&str) + Send + 'static) {
    let said = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&said);
    (said, move |line: &str| {
        kept.lock().expect("the lines").push(line.to_owned());
    })
}

/// A scripted check of `package`: its worker takes the check's turn, says so, stands in `hold`,
/// and lets the turn go — the check settling — once the hold is released.
fn scripted_check(
    turns: &'static PackagedTurns,
    package: PackageFamily,
    hold: &Arc<Hold>,
) -> mpsc::Receiver<()> {
    let (held, holding) = mpsc::channel();
    let hold = Arc::clone(hold);
    on_a_worker("packaged-turns-check", move |worker| {
        let turn = turns.check_turn(worker, Some(package), "PowerShell 7");
        let _ = held.send(());
        hold.pass();
        drop(turn);
    });
    holding
}

/// A birth of `package` through the product's [`born_in_turn`]: answers once its `birth` ran, and
/// with the lines it said.
fn birth_of(
    turns: &'static PackagedTurns,
    package: Option<PackageFamily>,
) -> (mpsc::Receiver<()>, Arc<Mutex<Vec<String>>>) {
    let (born, births) = mpsc::channel();
    let (said, note) = lines();
    on_a_worker("packaged-turns-birth", move |worker| {
        born_in_turn(turns, worker, package, &note, || {
            let _ = born.send(());
        });
    });
    (births, said)
}

/// RED (T-CHECK-NEVER-OVERLAPS-A-PACKAGED-BIRTH) — **a pane of a packaged program is not born
/// while a check of its package runs**: a scripted check holds the Store PowerShell's turn, the
/// birth waits for it, and is born the moment the check settles — saying once, in
/// `diagnostics.log`'s words, how long it waited and for which check.
///
/// MUTATION: in `born_in_turn`, call `birth` without taking `turns.birth_turn` (the door
/// bypassed) — the birth runs while the check is held, and no birth is ever seen waiting.
#[test]
fn a_packaged_birth_waits_for_a_running_check_of_its_program() {
    let turns = turns();
    let hold = Arc::new(Hold::default());
    hold.hold();
    let holding = scripted_check(turns, powershell(), &hold);
    receive(&holding, "the check holding its package");
    let (births, said) = birth_of(turns, Some(powershell()));
    wait_until("the birth waiting for the check", || {
        turns.waiting(&powershell()).0 == 1
    });
    assert!(births.try_recv().is_err(), "born while the check ran");
    hold.release();
    receive(&births, "the birth once the check settled");
    let said = said.lock().expect("the lines").clone();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("Folio: a pane's birth waited ")
            && said[0].ends_with(" ms for the PowerShell 7 check to settle"),
        "{said:?}"
    );
    wait_until("the package let go", || turns.idle(&powershell()));
}

/// RED — **a check does not start while a birth of its package is under way**: a birth stands in
/// its spawn (held), a check of the same package waits for it, and takes its turn once the spawn
/// has returned.
///
/// MUTATION: in `PackagedTurns::check_turn`, drop the wait on the package's births — the check
/// takes its turn while the birth is under way, and none is ever seen waiting.
#[test]
fn a_check_does_not_start_while_a_birth_of_its_program_is_pending() {
    let turns = turns();
    let hold = Arc::new(Hold::default());
    hold.hold();
    let (born, births) = mpsc::channel();
    let (said, note) = lines();
    let in_birth = Arc::clone(&hold);
    on_a_worker("packaged-turns-birth", move |worker| {
        born_in_turn(turns, worker, Some(powershell()), &note, || {
            in_birth.pass();
            let _ = born.send(());
        });
    });
    hold.wait_standing("the birth in its spawn");
    let (checked, checks) = mpsc::channel();
    on_a_worker("packaged-turns-check", move |worker| {
        let turn = turns.check_turn(worker, Some(powershell()), "PowerShell 7");
        let _ = checked.send(());
        drop(turn);
    });
    wait_until("the check waiting for the birth", || {
        turns.waiting(&powershell()).1 == 1
    });
    assert!(
        checks.try_recv().is_err(),
        "the check started during the birth"
    );
    hold.release();
    receive(&births, "the birth's spawn returning");
    receive(&checks, "the check once the birth was over");
    assert!(
        said.lock().expect("the lines").is_empty(),
        "the birth waited for nothing"
    );
}

/// RED (round 2) — **a waiting birth goes first**: while a birth waits for a check that holds its
/// package, a second check of that package asked meanwhile does not take a turn beside the first;
/// it starts only after the birth has spawned. So a birth waits only for the checks that held the
/// package when it came, never for a chain of later ones.
///
/// MUTATION: in `PackagedTurns::check_turn`, wait only while births are under way, not while one
/// waits — the second check takes its turn at once beside the first.
#[test]
fn a_check_asked_while_a_birth_waits_starts_after_the_birth() {
    let turns = turns();
    let first = Arc::new(Hold::default());
    first.hold();
    let holding = scripted_check(turns, powershell(), &first);
    receive(&holding, "the first check holding its package");
    let spawn = Arc::new(Hold::default());
    spawn.hold();
    let (born, births) = mpsc::channel();
    let in_birth = Arc::clone(&spawn);
    on_a_worker("packaged-turns-birth", move |worker| {
        born_in_turn(turns, worker, Some(powershell()), &|_| (), || {
            in_birth.pass();
            let _ = born.send(());
        });
    });
    wait_until("the birth waiting for the first check", || {
        turns.waiting(&powershell()).0 == 1
    });
    let (checked, checks) = mpsc::channel();
    on_a_worker("packaged-turns-check", move |worker| {
        let turn = turns.check_turn(worker, Some(powershell()), "PowerShell 7");
        let _ = checked.send(());
        drop(turn);
    });
    wait_until("the second check waiting behind the birth", || {
        turns.waiting(&powershell()).1 == 1
    });
    first.release();
    spawn.wait_standing("the birth in its spawn");
    assert!(
        checks.try_recv().is_err(),
        "the second check started before the birth"
    );
    spawn.release();
    receive(&births, "the birth's spawn returning");
    receive(&checks, "the second check once the birth was over");
    wait_until("the package let go", || turns.idle(&powershell()));
}

/// RED — **a birth of another program never waits**: while a check of the Store PowerShell runs,
/// a pane of a program in no package (Windows PowerShell 5.1, `cmd.exe`: `None`) and one of
/// another package (PowerShell 7 Preview, a family of its own) are born at once, saying nothing.
///
/// MUTATION: key every turn by one package whatever it is (`turning` matching any entry) — the
/// Preview's birth waits for the Store PowerShell's check.
#[test]
fn a_birth_of_another_program_never_waits() {
    let turns = turns();
    let hold = Arc::new(Hold::default());
    hold.hold();
    let holding = scripted_check(turns, powershell(), &hold);
    receive(&holding, "the check holding its package");
    let preview = PackageFamily::named("Microsoft.PowerShellPreview_8wekyb3d8bbwe");
    for package in [None, Some(preview)] {
        let (births, said) = birth_of(turns, package.clone());
        receive(&births, "a birth of another program");
        assert!(
            said.lock().expect("the lines").is_empty(),
            "{package:?} waited"
        );
    }
    assert_eq!(turns.waiting(&powershell()), (0, 0));
    hold.release();
    wait_until("the package let go", || turns.idle(&powershell()));
}

/// The program a check that never settles runs: it sleeps far past any patience, whatever
/// PowerShell arguments it is handed — Windows PowerShell on Windows, a shell script elsewhere —
/// and its scratch directory, if any.
fn never_settling() -> (std::process::Command, Option<std::path::PathBuf>) {
    #[cfg(windows)]
    {
        let program = Path::new(&std::env::var_os("SystemRoot").expect("Windows sets SystemRoot"))
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        (bt_platform::quiet_command(program), None)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = bt_testpath::temp_path("packaged-turns-never-settles");
        std::fs::create_dir_all(&directory).expect("the stand-in's directory");
        let program = directory.join("pwsh");
        std::fs::write(&program, "#!/bin/sh\nexec sleep 600\n").expect("the stand-in");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
            .expect("the stand-in runs");
        (bt_platform::quiet_command(&program), Some(directory))
    }
}

/// RED — **a birth's wait is bounded by the check's own deadline, never a clock of its own**: a
/// check that never settles takes its turn and runs as a real probe under a short patience; the
/// waiting birth is born when that patience ends the probe — the check answers that it was ended,
/// the birth says it waited once — and nothing in the door counts time.
///
/// MUTATION: in `run_powershell_probe`'s overdue arm, keep the turn (`std::mem::forget(turn)`
/// for `drop(turn)`) — the check is ended but its package stays held, and the birth waits past
/// the suite's patience.
#[test]
fn the_wait_is_bounded_by_the_checks_deadline() {
    let turns = turns();
    let go = Arc::new(Hold::default());
    go.hold();
    let (command, scratch) = never_settling();
    let (held, holding) = mpsc::channel();
    let (answered, answers) = mpsc::channel();
    let start = Arc::clone(&go);
    on_a_worker("packaged-turns-check", move |worker| {
        let turn = turns.check_turn(worker, Some(powershell()), "PowerShell 7");
        let _ = held.send(());
        start.pass();
        let answer = crate::shell_integration::run_powershell_probe(
            command,
            Some(turn),
            None,
            Path::new("pwsh.exe"),
            "Start-Sleep -Seconds 600; 'never'",
            None,
            bt_platform::ProbePatience::fixed(Duration::from_millis(500)),
        );
        let _ = answered.send(answer);
    });
    receive(&holding, "the check holding its package");
    let (births, said) = birth_of(turns, Some(powershell()));
    wait_until("the birth waiting for the check", || {
        turns.waiting(&powershell()).0 == 1
    });
    go.release();
    receive(&births, "the birth once the check's deadline ended it");
    let answer = receive(&answers, "the check's answer");
    if let Some(scratch) = scratch {
        let _ = std::fs::remove_dir_all(scratch);
    }
    assert!(
        matches!(
            answer,
            Err(crate::shell_integration::ParseProbeFailure::Overdue { .. })
        ),
        "{answer:?}"
    );
    assert_eq!(said.lock().expect("the lines").len(), 1);
}

/// **The wait line** says the wait in whole milliseconds and the check's edition — a Chinese
/// edition name stands for any the line may carry.
#[test]
fn the_wait_line_names_its_length_and_its_check() {
    assert_eq!(
        waited_line(Waited {
            length: Duration::from_micros(1_234_567),
            edition: "PowerShell 7",
        }),
        "Folio: a pane's birth waited 1234 ms for the PowerShell 7 check to settle"
    );
    assert_eq!(
        waited_line(Waited {
            length: Duration::ZERO,
            edition: "Windows PowerShell（预览）",
        }),
        "Folio: a pane's birth waited 0 ms for the Windows PowerShell（预览） check to settle"
    );
}

/// The Store's PowerShell 7 alias on this machine and its package — only when it really is one.
/// `None` on a machine with no Store install (a CI runner), which the tests below say on stderr.
#[cfg(windows)]
fn store_alias() -> Option<(std::path::PathBuf, PackageFamily)> {
    let alias = Path::new(&std::env::var_os("LocalAppData")?)
        .join("Microsoft")
        .join("WindowsApps")
        .join("pwsh.exe");
    let package = bt_platform::package_family_of(&alias)?;
    Some((alias, package))
}

#[cfg(windows)]
fn skipped(test: &str) {
    eprintln!("packaged turns test skipped={test} reason=no-store-powershell");
}

/// RED (wiring, Windows with a Store PowerShell 7) — **the product's PowerShell check takes its
/// turn**: while a pane of the Store PowerShell's package is being born (held in its spawn),
/// `check_powershell_in` of the Store alias waits, and runs once the birth is over. In turns of
/// its own, so no other test's PowerShell can stand in the same line.
///
/// **SKIPPED — passes without asserting — on a machine with no Store PowerShell 7** (every CI
/// runner): it says `packaged turns test skipped=… reason=no-store-powershell` on stderr. Its
/// proof is a run on a machine that has one (the owner's).
///
/// MUTATION: in `check_powershell_in`, take the turn on no package (`check_turn(worker, None, …)`,
/// the door bypassed) — the check runs during the birth, and none is seen waiting.
#[cfg(windows)]
#[test]
fn a_store_powershell_check_takes_its_turn_with_a_birth_of_its_package() {
    let Some((alias, package)) = store_alias() else {
        skipped("a_store_powershell_check_takes_its_turn_with_a_birth_of_its_package");
        return;
    };
    let turns = turns();
    let hold = Arc::new(Hold::default());
    hold.hold();
    let in_birth = Arc::clone(&hold);
    let birth_package = package.clone();
    let (born, births) = mpsc::channel();
    on_a_worker("packaged-turns-birth", move |worker| {
        born_in_turn(turns, worker, Some(birth_package), &|_| (), || {
            in_birth.pass();
            let _ = born.send(());
        });
    });
    hold.wait_standing("the birth in its spawn");
    let (checked, checks) = mpsc::channel();
    on_a_worker("packaged-turns-check", move |worker| {
        let answer = crate::shell_integration::check_powershell_in(
            turns,
            worker,
            &alias,
            "exit 0",
            None,
            crate::shell_integration::POWERSHELL_PROBE_PATIENCE,
            &crate::shell_integration::ProbeEnvironment::Inherited,
        );
        let _ = checked.send(answer.is_ok());
    });
    wait_until("the check waiting for the birth", || {
        turns.waiting(&package).1 == 1
    });
    assert!(checks.try_recv().is_err(), "the check ran during the birth");
    hold.release();
    receive(&births, "the birth's spawn returning");
    assert!(receive(&checks, "the check once the birth was over"));
}

/// RED (wiring, Windows with a Store PowerShell 7) — **the product's birth takes its turn**: a
/// shell of the Store alias asked through `pty_door::request_shell_in` while a check of its
/// package holds the turn is not born until the check settles. In turns of its own.
///
/// **SKIPPED — passes without asserting — on a machine with no Store PowerShell 7** (every CI
/// runner): it says `packaged turns test skipped=… reason=no-store-powershell` on stderr.
///
/// MUTATION: in `pty_door::bear`, call the spawn without `born_in_turn` (the door bypassed) — the
/// shell is born while the check is held, and no birth is seen waiting.
#[cfg(windows)]
#[test]
fn a_store_powershell_pane_takes_its_turn_with_a_check_of_its_package() {
    let Some((alias, package)) = store_alias() else {
        skipped("a_store_powershell_pane_takes_its_turn_with_a_check_of_its_package");
        return;
    };
    let turns = turns();
    let hold = Arc::new(Hold::default());
    hold.hold();
    let holding = scripted_check(turns, package.clone(), &hold);
    receive(&holding, "the check holding its package");
    let size = bt_pty::PtySize::cells(
        std::num::NonZeroU16::new(80).expect("columns"),
        std::num::NonZeroU16::new(24).expect("rows"),
    );
    let mut shell = crate::pty_door::request_shell_in(
        turns,
        crate::pty_door::ShellSpec {
            program: alias.into_os_string(),
            arguments: ["-NoLogo", "-NoProfile", "-Command", "exit 5"]
                .into_iter()
                .map(std::ffi::OsString::from)
                .collect(),
            powershell_integration: false,
            environment_derivation: crate::shell_integration::EnvironmentDerivation::default(),
            folio_environment: Vec::new(),
            profile_environment: Vec::new(),
            carried_environment: None,
            size,
            working_directory: None,
            unless_gone: None,
            recording: bt_pty::PtyRecording::none(),
        },
        Arc::new(|| ()),
    )
    .expect("a birth worker");
    wait_until("the shell's birth waiting for the check", || {
        turns.waiting(&package).0 == 1
    });
    assert!(!shell.answered(), "born while the check ran");
    hold.release();
    wait_until("the shell born once the check settled", || shell.answered());
    let born = shell.take().expect("the answer");
    assert!(born.session.is_ok(), "the shell was not started");
}
