//! Every filesystem operand in these tests is below a freshly created sandbox.
use super::*;
use std::fs;

// ── `bt-source`, for the one reader in this file that used to ask `main.rs`
//    for its text (`docs/plans/bt-app-split-prep.md` §6.3, ticket P17)
//
// **The pattern is `main.rs::pty_drain_budget_tests`' and is not re-derived**;
// only the three helpers that reader needs are copied. Its six points hold
// here word for word — one index per process, a body pin that names an
// identity rather than a file, and a `QueryFailure` that panics instead of
// narrowing the question.
//
// **This file is reached by `#[path]`** from `uninstall.rs`, so its own text is
// inside the universe `Index::of_package("bt-app")` declares (§2.6): a reader
// here that searched the crate would have to exclude its own needle. The pin
// below is a *body* reading of one named method, so the literal it looks for
// never meets the copy of itself written on this page — which is the other half
// of why a body pin is the right shape for a call-site fact.

/// **This crate, indexed once per process** — the workspace read, this
/// package's own `src/` declared as the universe and lowered, on the first ask
/// of the process, behind one call.
///
/// The package is named here and nowhere else in this file.
fn source() -> &'static bt_source::Index {
    bt_source::Index::of_package("bt-app")
}

/// The body of `owner::name`, braces included — the identity of §2.4 rather
/// than a line of whatever file holds it today.
fn item_body(query: &bt_source::ItemQuery) -> &'static str {
    source()
        .body_of(query)
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// The body of one inherent method of `owner`. The owner is an argument and
/// not a guess.
fn method_body(owner: &str, name: &str) -> &'static str {
    item_body(&bt_source::ItemQuery::method(owner, name))
}

/// **The temporary directory with every link above it resolved, in its ordinary spelling.**
///
/// The door refuses a root with a link anywhere among its ancestors, on purpose — a planted link
/// must never turn a deletion into authority over its target. A sandbox is a place the door is
/// pointed at, so its own spelling must carry no link, or every row reads as a planted one. On
/// macOS it would: `$TMPDIR` is `/var/folders/…`, and `/var` is the system's link to
/// `/private/var`, so every sandbox under the unresolved name was refused whole (12 tests red on
/// the Mac, ticket 72). `canonicalize` is the resolution; on Windows it answers the verbatim
/// `\\?\` form, where `/` is not a separator and the fixtures' `root.join("app/folio.exe")` would
/// name no file, so a verbatim drive or share prefix is spelled back the ordinary way. Nothing here
/// names a platform: a path with no prefix (every Unix path) is the canonical answer itself.
///
/// Production resolves the same link in its own roots since ticket 73, but only in the head the
/// operating system names (`purge_root`); a sandbox root is not such a head, so it is resolved here.
fn link_free_temp_dir() -> PathBuf {
    use std::path::{Component, Prefix};
    let real = fs::canonicalize(std::env::temp_dir()).expect("the temporary directory exists");
    let mut components = real.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return real;
    };
    let head = match prefix.kind() {
        Prefix::VerbatimDisk(letter) => format!("{}:\\", char::from(letter)),
        Prefix::VerbatimUNC(server, share) => format!(
            r"\\{}\{}\",
            server.to_string_lossy(),
            share.to_string_lossy()
        ),
        _ => return real,
    };
    let rest: PathBuf = components
        .filter(|component| !matches!(component, Component::RootDir))
        .collect();
    PathBuf::from(head).join(rest)
}

/// The spelling production hands to the removal boundary: resolved to its
/// existing target, with Windows' verbatim prefix returned to an ordinary
/// drive or share spelling.
fn resolved(path: &Path) -> PathBuf {
    bt_platform::handoff::strip_verbatim_prefix(&bt_platform::instance::canonical_path(path))
}

fn sandbox(tag: &str) -> (PathBuf, Scope) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root =
        link_free_temp_dir().join(format!("folio-uninstall-{tag}-{}-{n}", std::process::id()));
    fs::create_dir_all(root.join("app")).unwrap();
    let exe = root.join("app/folio.exe");
    fs::write(&exe, b"fixture executable").unwrap();
    let scope = Scope::sandbox(&root, exe).unwrap();
    (root, scope)
}

/// The same fixture tree resolved the way production resolves it — `sandbox: None` —
/// so that a rule claimed to hold everywhere can be tested where no sandbox contains
/// anything. **Never purge with this scope**: its temp rows resolve to the real
/// clipboard directory, exactly as production's do.
fn unsandboxed(tag: &str) -> (PathBuf, Scope) {
    let (root, _) = sandbox(tag);
    let scope = fixture_scope(&root, root.join("temp"), false);
    assert!(scope.sandbox.is_none());
    (root, scope)
}

/// The fixture tree's variables, with the temporary directory and the sandbox flag handed in.
fn fixture_scope(root: &Path, temp: PathBuf, sandbox: bool) -> Scope {
    let mapped = root.to_path_buf();
    Scope::resolve(
        root.join("app/folio.exe"),
        bt_platform::host_platform(),
        move |name| {
            Some(
                mapped
                    .join(match name {
                        "APPDATA" => "roaming",
                        "LOCALAPPDATA" => "local",
                        "HOME" | "USERPROFILE" => "home",
                        "XDG_DATA_HOME" => "xdg",
                        "BT_POWERSHELL_PROFILE" => "profiles/profile.ps1",
                        "BT_PSREADLINE_DOCUMENTS" => "documents",
                        "CLAUDE_CONFIG_DIR" => "home/.claude",
                        "CODEX_HOME" => "home/.codex",
                        "COPILOT_HOME" => "home/.copilot",
                        _ => return None,
                    })
                    .into_os_string(),
            )
        },
        temp,
        sandbox,
    )
    .unwrap()
}

fn system_absent(_: Remover) -> Vec<Entry> {
    vec![Entry::new("injected registration", Fate::Absent)]
}

// Windows claims are kernel objects keyed only to the injected path. Unix's default
// claim also creates a runtime file outside the sandbox, so these tests inject the gate.
fn execute(scope: &Scope, purge: bool, system: impl FnMut(Remover) -> Vec<Entry>) -> Report {
    execute_then(scope, purge, system, None)
}

/// [`execute`] with `--uninstall`'s program step, as `run_within` hands it.
fn execute_then(
    scope: &Scope,
    purge: bool,
    system: impl FnMut(Remover) -> Vec<Entry>,
    program: Option<ProgramStep<'_>>,
) -> Report {
    #[cfg(windows)]
    {
        super::execute(scope, purge, system, program)
    }
    #[cfg(not(windows))]
    {
        super::execute_with_claim(scope, purge, system, |_| Some(()), program)
    }
}

fn seed(scope: &Scope, owner: &Path) {
    let data = &scope.data[0];
    fs::create_dir_all(data).unwrap();
    let profile = &scope.profiles.as_ref().unwrap()[0];
    fs::create_dir_all(profile.parent().unwrap()).unwrap();
    fs::write(
        profile,
        crate::shell_integration::profile_marks::LEGACY_LINE,
    )
    .unwrap();
    crate::psreadline::install_recorded(&scope.documents[0], data).unwrap();
    for (index, roots) in scope.agents.iter().enumerate() {
        let path = agent_path(index, &roots[0]);
        assert_eq!(
            agent_apply(index, &path, Decision::Install, owner, data),
            Outcome::Installed
        );
    }
}

#[test]
fn uninstall_everything_then_rerun_is_absent() {
    let (root, scope) = sandbox("all");
    seed(&scope, &scope.exe);
    let mut registrations = [true, true, true];
    let mut system = |remover| {
        let index = match remover {
            Remover::Explorer => 0,
            Remover::Toast => 1,
            _ => 2,
        };
        let fate = if std::mem::take(&mut registrations[index]) {
            Fate::Removed
        } else {
            Fate::Absent
        };
        if index == 0 {
            vec![
                Entry::new("Explorer classic (injected)", fate.clone()),
                Entry::new("Explorer package (injected)", fate),
            ]
        } else if index == 1 {
            vec![Entry::new("Toast identity (injected)", fate)]
        } else {
            vec![Entry::new("Update entrance (injected)", fate)]
        }
    };
    let report = execute(&scope, false, &mut system);
    assert_eq!(report.code, 0, "{}", report.stderr());
    assert_eq!(
        report
            .entries
            .iter()
            .filter(|e| e.fate == Fate::Removed)
            .count(),
        9
    );
    println!("{}exit={}\n", report.stdout(), report.code);
    let second = execute(&scope, false, &mut system);
    assert_eq!(second.code, 0, "{}", second.stderr());
    assert!(second.entries.iter().all(|e| e.fate == Fate::Absent));
    println!("{}exit={}\n", second.stdout(), second.code);
    fs::remove_dir_all(root).unwrap();
}

/// RED GATE (audit 3, E-1) — **the door takes Folio's nine files out of the
/// module directory and leaves everything it did not write, including the
/// directory itself.**
///
/// The leaf this fixture builds is the state a Folio *before* the install guard
/// produced on a real machine: our module, with PowerShellGet's own record of
/// the module we replaced still standing beside it. Until today the door
/// answered that with `remove_dir_all` on the version leaf, so an uninstall took
/// `PSGetModuleInfo.xml`, `en-US\` and the catalog with it — files this product
/// never wrote and cannot hand back.
///
/// The remover is the row's own (`psreadline::remove_from`); what is pinned here
/// is that the unattended door goes through it and reports what it left.
#[test]
fn uninstall_psreadline_leaves_every_file_folio_never_wrote() {
    let (root, scope) = sandbox("psreadline-sidecars");
    let data = &scope.data[0];
    fs::create_dir_all(data).unwrap();
    crate::psreadline::install_recorded(&scope.documents[0], data).unwrap();
    let leaf = crate::psreadline::module_directory(&scope.documents[0]);
    let sidecars = [
        ("PSGetModuleInfo.xml", "<Objs/>"),
        ("PSReadLine.cat", "catalog"),
        ("en-US/about_PSReadLine.help.txt", "TOPIC"),
    ];
    for (name, body) in sidecars {
        let path = leaf.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body.as_bytes()).unwrap();
    }

    let report = execute(&scope, false, system_absent);
    assert_eq!(report.code, 0, "{}", report.stderr());
    for (name, _) in crate::psreadline::BUNDLED_FILES {
        assert!(!leaf.join(name).exists(), "{name} is Folio's and stayed");
    }
    for (name, body) in sidecars {
        assert_eq!(
            fs::read(leaf.join(name)).unwrap(),
            body.as_bytes(),
            "{name} is not Folio's and went"
        );
    }
    assert!(leaf.is_dir(), "and the directory holding them stays");

    let line = report
        .entries
        .iter()
        .map(|entry| entry.line(Lang::English))
        .find(|line| line.contains("PSReadLine module"))
        .expect("the door prints a line per mark");
    assert!(line.contains("not Folio's files"), "{line}");
    assert!(
        line.contains(&leaf.join("PSGetModuleInfo.xml").display().to_string()),
        "and names them: {line}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn uninstall_live_other_copy_is_left_and_dead_copy_is_removed() {
    let (root, scope) = sandbox("owners");
    // A second copy is the same program in another folder, never another name: an operand is
    // Folio's only if its own file name is one Folio installs itself under.
    let other = root.join("other/folio.exe");
    fs::create_dir_all(other.parent().unwrap()).unwrap();
    fs::write(&other, b"other copy").unwrap();
    seed(&scope, &other);
    let report = execute(&scope, false, system_absent);
    assert_eq!(report.code, 0);
    assert_eq!(
        report
            .entries
            .iter()
            .filter(|e| matches!(e.fate, Fate::Left(_)))
            .count(),
        3
    );
    fs::remove_file(other).unwrap();
    let report = execute(&scope, false, system_absent);
    assert_eq!(report.code, 0);
    assert_eq!(
        report
            .entries
            .iter()
            .filter(|e| e.fate == Fate::Removed)
            .count(),
        3
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn uninstall_refused_agent_is_identical_and_others_still_removed() {
    for case in ["malformed", "future", "readonly"] {
        let (root, scope) = sandbox(case);
        seed(&scope, &scope.exe);
        let path = agent_path(2, &scope.agents[2][0]);
        match case {
            "malformed" => fs::write(&path, b"{broken").unwrap(),
            "future" => fs::write(&path, br#"{"version":999,"hooks":{}}"#).unwrap(),
            _ => {
                let mut permissions = fs::metadata(&path).unwrap().permissions();
                permissions.set_readonly(true);
                fs::set_permissions(&path, permissions).unwrap();
            }
        }
        let before = fs::read(&path).unwrap();
        let report = execute(&scope, false, system_absent);
        assert_eq!(report.code, 1, "{case}: {}", report.stdout());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(report.stderr().contains(&path.display().to_string()));
        assert_eq!(
            report
                .entries
                .iter()
                .filter(|e| e.fate == Fate::Removed)
                .count(),
            4
        );
        if case == "readonly" {
            // Restore the original fixture's permissions; never relax a real file.
            let permissions = fs::metadata(&scope.exe).unwrap().permissions();
            fs::set_permissions(&path, permissions).unwrap();
        }
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn uninstall_running_claim_refuses_before_any_remover() {
    let (root, scope) = sandbox("running");
    seed(&scope, &scope.exe);
    let profile = scope.profiles.as_ref().unwrap()[0].clone();
    let before = fs::read(&profile).unwrap();
    let report = super::execute_with_claim(
        &scope,
        true,
        |_| panic!("must not reach a system remover"),
        |_| None::<()>,
        None,
    );
    assert_eq!(report.code, 2);
    // **The door's sentence, byte for byte.** The refusal a script reads when a
    // Folio is running comes from the instance claim above and not from the
    // marks lock, which is why Folio's own writers learning to wait for that
    // lock (2026-09-21) leaves this transcript exactly where it was.
    assert_eq!(
        report.stdout(),
        "Folio: refused (A Folio instance is running; nothing was changed.)\n"
    );
    assert_eq!(report.stdout(), report.stderr());
    assert_eq!(fs::read(profile).unwrap(), before);
    assert!(scope.data[0].exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn uninstall_purge_only_resolved_roots_and_never_application() {
    let (root, scope) = sandbox("purge");
    seed(&scope, &scope.exe);
    let sibling = root.join("sibling/sentinel");
    fs::create_dir_all(sibling.parent().unwrap()).unwrap();
    fs::write(&sibling, b"keep").unwrap();
    for (mark, path) in &scope.purge_roots {
        if !path.exists() {
            if matches!(mark.name, "Panic log" | "Preferences") {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, b"data").unwrap();
            } else {
                fs::create_dir_all(path).unwrap();
                fs::write(path.join("sentinel"), b"data").unwrap();
            }
        }
    }
    let report = execute(&scope, true, system_absent);
    assert_eq!(report.code, 0, "{}", report.stderr());
    assert!(scope.purge_roots.iter().all(|(_, path)| !path.exists()));
    assert_eq!(fs::read(sibling).unwrap(), b"keep");
    assert!(scope.exe.exists());
    fs::remove_dir_all(root).unwrap();
}

/// PIN (U-6) — **`--purge` removes `update-check.json` at schema v2 as it removed v1**: the
/// file lives in the data root the purge removes whole, and schema v2 adds no file elsewhere.
///
/// The document is written by the product's own owner (`update::OfferState::skip`), so the
/// file on disk is the v2 shape a real Skip leaves.
#[test]
fn uninstall_purge_removes_the_v2_update_check_file() {
    let (root, scope) = sandbox("purge-update-check");
    let data = &scope.data[0];
    fs::create_dir_all(data).unwrap();
    crate::update::OfferState::load(data, true)
        .skip("v0.5.0")
        .expect("a Skip written into the sandbox's data root");
    let file = data.join(crate::update::STATE_FILE_NAME);
    let written: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(written["schema_version"], serde_json::json!(2));
    assert_eq!(written["skipped_tag"], serde_json::json!("v0.5.0"));

    let report = execute(&scope, true, system_absent);
    assert_eq!(report.code, 0, "{}", report.stderr());
    assert!(!file.exists(), "the v2 file went with the data root");
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn uninstall_purge_junction_refuses_root_without_touching_target() {
    let (root, scope) = sandbox("junction");
    let outside = root.join("sibling");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"keep").unwrap();
    fs::create_dir_all(&scope.data[0]).unwrap();
    let junction = scope.data[0].join("escape");
    // Native PowerShell creates a junction without developer-mode symlink privileges; started
    // through `bt_pty::test_shell` (no profile, a temporary HOME/APPDATA).
    let hygiene = bt_pty::test_shell::Hygiene::new();
    let status = hygiene.command("powershell.exe", bt_platform::quiet_command)
        .args(["-NonInteractive", "-Command", "New-Item -ItemType Junction -Path $env:FOLIO_TEST_JUNCTION -Target $env:FOLIO_TEST_TARGET -ErrorAction Stop | Out-Null"])
        .env("FOLIO_TEST_JUNCTION", &junction).env("FOLIO_TEST_TARGET", &outside).status().unwrap();
    assert!(status.success());
    let report = execute(&scope, true, system_absent);
    assert_eq!(report.code, 1);
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"keep");
    assert!(junction.exists());
    fs::remove_dir(&junction).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn uninstall_purge_busy_file_returns_two_before_touching_marks() {
    use std::os::windows::fs::OpenOptionsExt;
    let (root, scope) = sandbox("busy");
    seed(&scope, &scope.exe);
    let path = scope.data[0].join("busy");
    fs::write(&path, b"held data").unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    let profile = scope.profiles.as_ref().unwrap()[0].clone();
    let before = fs::read(&profile).unwrap();
    let report = execute(&scope, true, |_| panic!("must not reach registry"));
    assert_eq!(report.code, 2);
    assert_eq!(fs::read(profile).unwrap(), before);
    // The rule is unchanged; the refusal names the file the reader has to close.
    assert!(report.stderr().contains("(busy)"), "{}", report.stderr());
    assert!(
        report
            .stderr()
            .contains(&scope.data[0].display().to_string())
    );
    drop(held);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn uninstall_cli_rejects_purge_alone_and_extra_arguments() {
    use crate::cli::UninstallDoor::Cleanup;
    let parse = |a: &[&str]| crate::cli::uninstall_cleanup(a.iter().map(std::ffi::OsString::from));
    assert_eq!(
        parse(&["--uninstall-cleanup"]),
        Some(Ok(Cleanup { purge: false }))
    );
    assert_eq!(
        parse(&["--uninstall-cleanup", "--purge"]),
        Some(Ok(Cleanup { purge: true }))
    );
    assert_eq!(
        parse(&["--purge", "--uninstall-cleanup"]),
        Some(Ok(Cleanup { purge: true }))
    );
    assert!(parse(&["--purge"]).unwrap().is_err());
    assert!(
        parse(&["--uninstall-cleanup", "--cwd", "x"])
            .unwrap()
            .is_err()
    );
    assert_eq!(parse(&["--version"]), None);
}

#[test]
fn uninstall_registry_decisions_use_only_injected_readings() {
    use crate::explorer_menu::{MenuRemoval, PackageState};
    assert_eq!(
        bt_platform::cleanup::remove_toast_with(Ok(false), || panic!("absent")),
        Ok(false)
    );
    assert_eq!(
        bt_platform::cleanup::remove_toast_with(Ok(true), || Ok(())),
        Ok(true)
    );
    assert!(
        bt_platform::cleanup::remove_toast_with(Err("unreadable".to_owned()), || panic!(
            "unreadable"
        ))
        .is_err()
    );
    assert!(
        bt_platform::cleanup::remove_toast_with(Ok(true), || Err("denied".to_owned())).is_err()
    );
    assert_eq!(
        crate::explorer_menu::package_removal(
            &PackageState::Absent,
            |_| panic!("absent"),
            |_| panic!("absent")
        ),
        MenuRemoval::Nothing
    );
    let exe = Path::new(r"C:\Users\alice\Folio\folio.exe");
    let desired = bt_platform::context_menu_shape(exe, "Folio");
    let found = vec![bt_platform::ContextMenuTree::Written(
        bt_platform::context_menu_shape(Path::new(r"C:\Users\alice\Other\folio.exe"), "Folio"),
    )];
    assert_eq!(
        crate::context_menu::classic_removal(&found, Some(&desired), |_| true),
        MenuRemoval::AnotherCopy
    );
    assert_eq!(
        crate::context_menu::classic_removal(&found, Some(&desired), |_| false),
        MenuRemoval::Remove
    );
}

#[test]
fn uninstall_history_finds_redirected_module_and_agent_roots() {
    let (root, scope) = sandbox("history");
    let historical = root.join("old-documents");
    crate::psreadline::install_recorded(&historical, &scope.data[0]).unwrap();
    let old_agent = root.join("old-codex");
    assert_eq!(
        agent_apply(
            1,
            &agent_path(1, &old_agent),
            Decision::Install,
            &scope.exe,
            &scope.data[0]
        ),
        Outcome::Installed
    );
    let report = execute(&scope, false, system_absent);
    assert_eq!(report.code, 0, "{}", report.stderr());
    assert!(!crate::psreadline::module_directory(&historical).exists());
    assert!(
        !fs::read_to_string(agent_path(1, &old_agent))
            .unwrap()
            .contains("notify =")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn uninstall_sandbox_never_uses_recorded_paths_outside_its_root() {
    let (root, scope) = sandbox("isolation");
    let sibling = root.with_extension("sibling");
    fs::create_dir_all(&sibling).unwrap();
    fs::create_dir_all(root.join("home")).unwrap();
    let path = agent_path(0, &sibling);
    assert_eq!(
        agent_apply(0, &path, Decision::Install, &scope.exe, &scope.data[0]),
        Outcome::Installed
    );
    let before = fs::read(&path).unwrap();
    let mut marks = Marks::read(&scope.data[0]).unwrap();
    marks
        .agent_config_roots
        .claude
        .push(root.join("home/../..").join(sibling.file_name().unwrap()));
    marks.powershell_profiles.push(path.clone());
    marks.write(&scope.data[0]).unwrap();
    let report = execute(&scope, true, system_absent);
    assert_eq!(report.code, 1);
    assert_eq!(fs::read(path).unwrap(), before);
    fs::remove_dir_all(sibling).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn uninstall_purge_refuses_the_application_folder() {
    let (root, mut scope) = sandbox("app-protection");
    // A data row's own mark, pointed at a root that holds the application.
    scope.purge_roots.push((
        mark_of(Remover::Data(HostPlatform::Windows, Base::Roaming, "Folio")),
        root.clone(),
    ));
    let report = execute(&scope, true, system_absent);
    assert_eq!(report.code, 1);
    assert_eq!(fs::read(&scope.exe).unwrap(), b"fixture executable");
    fs::remove_dir_all(root).unwrap();
}

/// Also: `make_standard_streams_uninheritable` comes before every door that can start a child
/// (T-UNINSTALL-SELFHOLD; MUTATION: remove it from `main`, and this is red —
/// `the_door_holds_nothing_of_its_askers_so_remove_data_removes_its_log` drives the call itself).
#[test]
fn uninstall_door_precedes_every_startup_effect() {
    let source = include_str!("main.rs");
    let main = source.split_once("\nfn main() -> Result<()> {").unwrap().1;
    let door = main.find("cli::uninstall_cleanup(").unwrap();
    let uninheritable = main
        .find("bt_platform::make_standard_streams_uninheritable();")
        .expect("main makes its standard streams uninheritable");
    assert!(uninheritable < main.find("cli::console_members(").unwrap());
    for later in [
        "install_panic_log_hook();",
        "cli::attention(",
        "cli::parse(",
        "launch_wire::hand_over(",
        "diagnostics::enter_resident_run(",
        "EventLoop::<AppEvent>::with_user_event()",
    ] {
        assert!(door < main.find(later).unwrap(), "{later}");
    }
    let orchestration = include_str!("uninstall.rs")
        .split("#[cfg(test)]")
        .next()
        .unwrap();
    for forbidden in [
        "persist::storage_dir()",
        "attention claude-code:",
        "codex:agent-turn-complete",
        "CreateProcess",
        "taskkill",
    ] {
        assert!(
            !orchestration.contains(forbidden),
            "the door acquired another owner: {forbidden}"
        );
    }
}

#[test]
fn uninstall_source_guard_pins_known_writers_and_inventory() {
    use syn::visit::{self, Visit};
    #[derive(Default)]
    struct Functions {
        found: Vec<String>,
        owner: String,
    }
    impl<'ast> Visit<'ast> for Functions {
        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            self.found.push(item.sig.ident.to_string());
            visit::visit_item_fn(self, item);
        }
        fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
            let name = match item.self_ty.as_ref() {
                syn::Type::Path(path) => path.path.segments.last().unwrap().ident.to_string(),
                _ => String::new(),
            };
            let previous = std::mem::replace(&mut self.owner, name);
            visit::visit_item_impl(self, item);
            self.owner = previous;
        }
        fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
            self.found
                .push(format!("{}::{}", self.owner, item.sig.ident));
            visit::visit_impl_item_fn(self, item);
        }
    }
    // Like the content-read guard, parse Rust rather than matching comment decoys.
    // Syntax cannot establish whether a generic Path came from data, an export picker,
    // or an OS framework. We pin the audited writer owners, not a false universal promise.
    for (remover, source, writer) in [
        (
            Remover::Profiles,
            include_str!("shell_integration.rs"),
            "add_to_profile",
        ),
        (
            Remover::PsReadLine,
            include_str!("psreadline.rs"),
            "install_recorded",
        ),
        (
            Remover::Agent(0),
            include_str!("attention_hooks.rs"),
            "apply_at",
        ),
        (
            Remover::Agent(1),
            include_str!("attention_codex.rs"),
            "apply_at",
        ),
        (
            Remover::Agent(2),
            include_str!("attention_copilot.rs"),
            "apply_at",
        ),
        (Remover::Explorer, include_str!("context_menu.rs"), "apply"),
        (
            Remover::Explorer,
            include_str!("explorer_menu.rs"),
            "request",
        ),
        (
            Remover::Toast,
            include_str!("../../bt-platform/src/lib.rs"),
            "Notifier::new",
        ),
        (
            Remover::RecoverySnapshots,
            include_str!("shell_integration.rs"),
            "replace_profile",
        ),
        (
            Remover::RecoverySnapshots,
            include_str!("attention_hooks.rs"),
            // A method on the one resolution an operation makes, since the T-B follow-ups: the
            // walker above names an impl item `Owner::method`, and the dated copy beside somebody
            // else's configuration is written by that one and by nothing else.
            "Config::land",
        ),
        (
            Remover::RuntimeClaims,
            include_str!("../../bt-platform/src/instance.rs"),
            "try_claim_data_directory",
        ),
        (
            Remover::Data(HostPlatform::Windows, Base::Roaming, "Folio"),
            include_str!("persist.rs"),
            "storage_location",
        ),
        (
            Remover::Data(HostPlatform::Windows, Base::Roaming, "BetterTerminal"),
            include_str!("persist.rs"),
            "storage_location",
        ),
        (
            Remover::Data(HostPlatform::Windows, Base::Local, "Folio"),
            include_str!("webhost.rs"),
            "user_data_folder_in",
        ),
        (
            Remover::Data(
                HostPlatform::MacOs,
                Base::Library("Application Support"),
                "Folio",
            ),
            include_str!("webhost.rs"),
            "web_engine_folder",
        ),
        (
            Remover::Data(HostPlatform::OtherUnix, Base::Xdg, "Folio"),
            include_str!("persist.rs"),
            "storage_location",
        ),
        (
            Remover::Data(HostPlatform::OtherUnix, Base::Temp, "folio/clipboard"),
            include_str!("clipboard_picture.rs"),
            "save",
        ),
        (
            Remover::Data(HostPlatform::OtherUnix, Base::Temp, "folio-panic.log"),
            include_str!("main.rs"),
            "install_panic_log_hook",
        ),
        (
            Remover::Staging,
            include_str!("../../bt-platform/src/deferred_removal.rs"),
            "schedule",
        ),
    ] {
        let mut functions = Functions::default();
        functions.visit_file(&syn::parse_file(source).unwrap());
        assert!(
            functions.found.iter().any(|f| f == writer),
            "writer moved: {writer}"
        );
        assert!(
            INVENTORY
                .iter()
                .any(|mark| mark.remover == remover && mark.writer.contains(writer)),
            "writer has no undo: {writer}"
        );
    }
    assert_eq!(
        INVENTORY
            .iter()
            .filter(|m| matches!(m.remover, Remover::Data(HostPlatform::MacOs, ..)))
            .count(),
        6
    );
    assert_eq!(
        INVENTORY
            .iter()
            .filter(|m| matches!(m.remover, Remover::Data(HostPlatform::Windows, ..)))
            .count(),
        3
    );
    // `root` is borrowed since the record moved inside the writer's own
    // occupancy check (audit 3, E-1); what is pinned is that the module root
    // still reaches `integration-marks.json` before the first byte is written.
    assert!(
        include_str!("psreadline.rs")
            .contains("marks.psreadline_module_roots.push(root.to_owned())")
    );
    // And that the one caller of the recording writer is the row's own press.
    // This used to be `include_str!("main.rs").contains(…)` — a positive over a
    // whole file, which says *somewhere in that file* and goes silent the day
    // the method it means moves out of it. The subject is
    // `Runtime::apply_psreadline`, whose Step 2a destination is
    // `runtime/first_run.rs`, so the identity is what is asked for and the file
    // is not mentioned.
    assert!(
        method_body("Runtime", "apply_psreadline").contains("psreadline::apply_recorded("),
        "`Runtime::apply_psreadline` no longer reaches `psreadline::apply_recorded`, \
         which is the call that records the module root this door later reads out of \
         `integration-marks.json` to remove"
    );
    for name in ["Folio", "BetterTerminal"] {
        assert!(INVENTORY.iter().any(|m| matches!(m.remover, Remover::Data(HostPlatform::Windows, Base::Roaming, relative) if relative == name)));
    }
    assert_eq!(
        crate::webhost::user_data_folder_in(Path::new("local"))
            .parent()
            .unwrap(),
        Path::new("local/Folio")
    );
    assert_eq!(INVENTORY.len(), 28);
    // The update entrance's writer is in `bt-platform` and is asked for by its
    // identity through `bt-source`, not by a file (U-22): `logon_hook::arm_in`
    // is the one function that writes a `Run` value, and the row names it.
    let entrance_writer = bt_source::Index::of_package("bt-platform")
        .body_of(&bt_source::ItemQuery::function("arm_in"))
        .unwrap_or_else(|failure| panic!("{failure}"));
    assert!(
        entrance_writer.contains(".set(key, &name, REG_SZ, &data)"),
        "`logon_hook::arm_in` no longer writes the entrance"
    );
    assert!(
        INVENTORY
            .iter()
            .any(|mark| mark.remover == Remover::Entrance
                && mark.kind == Kind::PerCopy
                && mark.writer.ends_with("logon_hook.rs:arm_in")),
        "the update entrance has no undo"
    );
    assert!(
        include_str!("../../bt-platform/src/macos_webview.rs")
            .contains("WKWebsiteDataStore::defaultDataStore(mtm)")
    );
    assert!(
        include_str!("../../../packaging/macos/Info.plist.in")
            .contains("io.github.lulu-loopp.folio")
    );
}

/// RED (U-22) — **the update entrance's row removes this copy's `Run` values —
/// the one naming its installation home and the one naming a program that is
/// gone — leaves another copy's live entrance and every other value, and says
/// each.**
///
/// §(b).3: "`--uninstall-cleanup`: a per-copy row that removes the value if it
/// names this copy's `H` or a path that no longer exists". The row's remover is
/// the entrance door's own (`logon_hook::clean_in`), run here over an in-memory
/// registry; the real registry is `logon_hook`'s own test, under a key of its
/// own. Off Windows there is no such entrance, and the row says `not present`.
///
/// MUTATION: in `entrance_entries`, hand `clean` the folder above the install
/// instead of the home (another copy's entrance is then taken too).
#[test]
fn uninstall_entrance_row_removes_this_copys_values_and_leaves_the_rest() {
    use bt_platform::logon_hook::{self, REG_SZ, Registry};
    #[derive(Default)]
    struct Memory(std::collections::BTreeMap<String, (u32, Vec<u8>)>);
    impl Registry for Memory {
        fn set(&mut self, _: &str, name: &str, kind: u32, data: &[u8]) -> io::Result<()> {
            self.0.insert(name.to_owned(), (kind, data.to_vec()));
            Ok(())
        }
        fn flush(&mut self, _: &str) -> io::Result<()> {
            Ok(())
        }
        fn get(&mut self, _: &str, name: &str) -> io::Result<Option<(u32, Vec<u8>)>> {
            Ok(self.0.get(name).cloned())
        }
        fn delete(&mut self, _: &str, name: &str) -> io::Result<bool> {
            Ok(self.0.remove(name).is_some())
        }
        fn names(&mut self, _: &str) -> io::Result<Vec<String>> {
            Ok(self.0.keys().cloned().collect())
        }
    }
    let (root, scope) = sandbox("entrance");
    let home = scope.exe.parent().unwrap().join(".folio-update");
    let rescue = |home: &Path, txn: u8| {
        let program = home
            .join(format!("{txn:02x}"))
            .join("rescue")
            .join("folio.exe");
        fs::create_dir_all(program.parent().unwrap()).unwrap();
        fs::write(&program, b"rescue").unwrap();
        program
    };
    let other_home = root.join("other").join(".folio-update");
    let ours = rescue(&home, 0xaa);
    let another_copy = rescue(&other_home, 0xbb);
    let mut memory = Memory::default();
    for (txn, program) in [
        (0xaa, ours),
        (0xbb, another_copy.clone()),
        (0xcc, root.join("gone").join("rescue").join("folio.exe")),
    ] {
        let _armed = logon_hook::arm_in(&mut memory, "run", &[txn; 16], &program).unwrap();
    }
    memory
        .set("run", "SomeoneElse", REG_SZ, b"x\0\0\0")
        .unwrap();

    let entries = entrance_entries(HostPlatform::Windows, &scope.exe, Lang::English, |home| {
        logon_hook::clean_in(&mut memory, "run", home)
    });
    let fates: Vec<(String, Fate)> = entries.into_iter().map(|e| (e.mark, e.fate)).collect();
    assert_eq!(
        fates,
        vec![
            (
                "Update entrance (per-copy): FolioUpdate-aaaaaaaa".to_owned(),
                Fate::Removed
            ),
            (
                "Update entrance (per-copy): FolioUpdate-bbbbbbbb".to_owned(),
                Fate::Left(vec![another_copy])
            ),
            (
                "Update entrance (per-copy): FolioUpdate-cccccccc".to_owned(),
                Fate::Removed
            ),
        ]
    );
    let mut left: Vec<String> = memory.0.keys().cloned().collect();
    left.sort();
    assert_eq!(left, vec!["FolioUpdate-bbbbbbbb", "SomeoneElse"]);

    let nothing = entrance_entries(HostPlatform::Windows, &scope.exe, Lang::English, |home| {
        logon_hook::clean_in(&mut Memory::default(), "run", home)
    });
    assert_eq!(nothing.len(), 1);
    assert_eq!(nothing[0].fate, Fate::Absent);
    let elsewhere = entrance_entries(HostPlatform::MacOs, &scope.exe, Lang::English, |_| {
        panic!("no entrance off Windows")
    });
    assert_eq!(elsewhere[0].fate, Fate::Absent);
    fs::remove_dir_all(root).unwrap();
}

/// PIN (T-UNINSTALL-UX; U-9, T-UNINSTALL-DOCS) — **the archive is its ten members, and
/// `uninstall.cmd` is one press: one question in both languages where Enter keeps settings and
/// data, then the door's `--uninstall` — with `--remove-data` only on `n` — and a pause; a line in
/// both languages when the door answers that Folio is running; success says removal continues
/// after the window closes rather than claiming completion; no pid, and no deletion of its own.**
///
/// The script names no pid because a batch file cannot learn its own and need not: it is the
/// door's parent, and the door's remover waits for its parent. It deletes nothing itself — the
/// door decides what is Folio's — and never runs the package managers' `--uninstall-cleanup`,
/// which would leave the folder behind.
///
/// MUTATION: run `"%~dp0folio.exe" --uninstall-cleanup` in the script again.
#[test]
fn uninstall_archive_has_ten_files_and_a_one_press_wrapper() {
    // The archive's members are one list, `scripts/release/archive-members.txt`
    // (0.4.6 ticket U-9): `package.ps1` packs from it and `build.rs` builds the
    // release manifest from it. It is read here with the grammar both use, and
    // `package.ps1` is held to reading it rather than to a list of its own.
    let package = include_str!("../../../scripts/release/package.ps1");
    assert!(
        package.contains("$listed = Get-ArchiveMemberList"),
        "package.ps1 packs from archive-members.txt"
    );
    let listed = bt_winres::release_manifest::parse_member_list(include_str!(
        "../../../scripts/release/archive-members.txt"
    ))
    .expect("the archive's member list parses");
    let names: Vec<_> = listed.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "folio.exe",
            "folio.msix",
            "conpty.dll",
            "OpenConsole.exe",
            "folio-here.cmd",
            "uninstall.cmd",
            "LICENSE-MIT",
            "LICENSE-APACHE",
            "THIRD-PARTY-NOTICES.md",
            "TRADEMARK.md"
        ]
    );
    let wrapper = include_str!("../../../packaging/uninstall.cmd");
    assert!(wrapper.contains("\"%~dp0folio.exe\" --uninstall %remove%"));
    assert!(wrapper.contains("if /i \"%answer%\"==\"n\" set \"remove=--remove-data\""));
    assert!(wrapper.contains("if \"%door%\"==\"2\""));
    assert!(wrapper.contains("if \"%door%\"==\"0\""));
    assert!(wrapper.contains("Removal continues after this window closes."));
    assert!(wrapper.contains("pause"));
    for text in [Text::UninstallScriptQuestion, Text::UninstallScriptRunning] {
        // Both columns: the script prints each line in English, then in Chinese
        // (T-UNINSTALL-DOCS), so a sentence changed in the table and not in the
        // script, in either language, is red here.
        assert!(wrapper.contains(english(text)), "{text:?}");
        assert!(wrapper.contains(text.in_lang(Lang::Chinese)), "{text:?}");
    }
    for forbidden in [
        "--uninstall-cleanup",
        "--purge",
        "--after-pid",
        "rmdir",
        "rd /",
        " del ",
        "Remove-Item",
    ] {
        assert!(!wrapper.contains(forbidden), "{forbidden}");
    }
}

/// RED (0.4.7 uninstall fix) — **the shipped `uninstall.cmd`, run by the real `cmd.exe`, asks
/// its one question and starts the `folio.exe` beside it with `--uninstall` — and
/// `--remove-data` only on `n` — whatever code page the console had when it started.**
///
/// The script is the archive's own bytes; the `folio.exe` beside it is a stand-in whose exit code
/// is the length of the command line it was started with
/// (`trust_harness::Behaviour::SaysItsCommandLineLength`), which the script hands back as its own.
/// It runs once in the console's own code page and once after `chcp 65001`, as the clean-VM
/// rehearsal ran it; the answer is read from a file, as a typed line is.
///
/// The clean-VM rehearsal of 0.4.7 met the script shipped with LF line endings: `cmd.exe` reads a
/// batch file a line at a time and finds its place again by offset, and in a file of LF-only
/// lines holding UTF-8 text it lost its place — fragments of lines ran as commands, and
/// `folio.exe` was never started.
///
/// MUTATION: write the script with its CRs removed (the shipped bytes before this fix); the
/// stand-in is not started with the expected line.
#[test]
#[cfg(windows)]
fn the_shipped_uninstall_script_answers_its_question_in_the_real_cmd() {
    let (root, _) = sandbox("uninstall-cmd");
    let folder = root.join("folio");
    fs::create_dir_all(&folder).unwrap();
    let stand_in = folder.join("folio.exe");
    bt_platform::trust_harness::program(
        &stand_in,
        bt_platform::trust::FileVersion([0, 4, 7, 0]),
        bt_platform::trust_harness::Behaviour::SaysItsCommandLineLength,
    )
    .unwrap();
    let script = folder.join("uninstall.cmd");
    fs::write(&script, include_bytes!("../../../packaging/uninstall.cmd")).unwrap();
    let answer = root.join("answer.txt");
    // The test shell's door: `cmd.exe` with its AutoRun refused (`/d`), under a temporary home.
    let hygiene = bt_pty::test_shell::Hygiene::new();
    // A script `cmd.exe` mis-reads runs fragments of its lines as commands — on the clean VM,
    // `Folio` among them. So it finds programs in the system folder alone (where `chcp` is) and
    // starts in a folder with nothing in it: no fragment reaches a program on this machine's
    // `PATH`, such as the `folio.exe` a build puts there.
    let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
    let empty = root.join("empty");
    fs::create_dir_all(&empty).unwrap();
    let run = |typed: &str, first: &str| {
        use std::os::windows::process::CommandExt;
        fs::write(&answer, format!("{typed}\r\n")).unwrap();
        let output = hygiene
            .command("cmd.exe", bt_platform::quiet_command)
            .arg("/c")
            // `cmd /c` takes away the first and the last quote of a line that begins with one.
            .raw_arg(format!("\"{first}\"{}\"\"", script.display()))
            .env("PATH", &system)
            .current_dir(&empty)
            .stdin(fs::File::open(&answer).unwrap())
            .output()
            .unwrap();
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
    };
    // `cmd.exe` starts a program with the program as the line wrote it, a space, and the rest of
    // the line as written — its leading space included, and the space an empty `%remove%` leaves.
    let started_with = |rest: &str| {
        let line = format!("\"{}\" {rest}", stand_in.display());
        i32::try_from(2 * line.encode_utf16().count()).unwrap()
    };
    for first in ["", "chcp 65001 >nul <nul & "] {
        for (typed, tail) in [
            ("", " --uninstall "),
            ("Y", " --uninstall "),
            ("n", " --uninstall --remove-data"),
        ] {
            let (code, stdout) = run(typed, first);
            assert_eq!(
                code,
                Some(started_with(tail)),
                "{first:?} {typed:?}:
{stdout}"
            );
            assert!(stdout.contains("Keep settings and data? [Y/n]"), "{stdout}");
        }
    }
    fs::remove_dir_all(root).unwrap();
}

/// An uninstaller must not bring anything into existence. An account that never ran
/// Folio has no marks to remove, and the door has to be able to say so without writing
/// the record — or its lock, or the data root — to say it.
#[test]
fn uninstall_creates_nothing_on_an_account_that_never_ran_folio() {
    let (root, _) = sandbox("pristine");
    let account = root.join("account");
    let scope = Scope::sandbox(&account, root.join("app/folio.exe")).unwrap();
    assert!(!account.exists());
    let report = execute(&scope, false, system_absent);
    assert_eq!(report.code, 0, "{}", report.stderr());
    assert!(
        report.entries.iter().all(|e| e.fate == Fate::Absent),
        "{}",
        report.stdout()
    );
    assert!(!account.exists(), "cleanup created {}", account.display());
    let purge = execute(&scope, true, system_absent);
    assert_eq!(purge.code, 0, "{}", purge.stderr());
    assert!(!account.exists(), "purge created {}", account.display());
    let listing: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(listing, ["app"], "the door left {listing:?} behind");
    fs::remove_dir_all(root).unwrap();
}

/// A path read out of the record is data, not authority — and the rule holds without a
/// sandbox to contain it.
#[test]
fn uninstall_refuses_hostile_recorded_paths_with_no_sandbox_to_contain_them() {
    let (root, scope) = unsandboxed("hostile");
    seed(&scope, &scope.exe);
    let data = scope.data[0].clone();
    let document = root.join("home/thesis.txt");
    fs::write(&document, b"the reader's own file").unwrap();
    let agent = agent_path(0, &root.join("elsewhere"));
    assert_eq!(
        agent_apply(0, &agent, Decision::Install, &scope.exe, &data),
        Outcome::Installed
    );
    let agent_before = fs::read(&agent).unwrap();
    let documents = root.join("documents-elsewhere");
    crate::psreadline::install_recorded(&documents, &data).unwrap();
    let module = crate::psreadline::module_directory(&documents);
    assert!(module.exists());
    let hostile = [
        root.join("home/../elsewhere"),
        data.ancestors().last().unwrap().to_path_buf(),
        document.clone(),
    ];
    let mut marks = Marks::read(&data).unwrap();
    marks.agent_config_roots.claude = hostile.to_vec();
    marks.psreadline_module_roots = vec![
        root.join("home/..")
            .join("documents-elsewhere")
            .join(crate::psreadline::MODULE_RELATIVE_PATH)
            .join(crate::psreadline::PATCHED_VERSION),
    ];
    marks.write(&data).unwrap();
    let report = execute(&scope, false, system_absent);
    assert_eq!(report.code, 1, "{}", report.stdout());
    assert_eq!(fs::read(&agent).unwrap(), agent_before);
    assert_eq!(fs::read(&document).unwrap(), b"the reader's own file");
    assert!(module.exists(), "a recorded `..` reached a module root");
    for refused in hostile
        .iter()
        .map(|path| path.display().to_string())
        .chain(["documents-elsewhere".to_owned()])
    {
        assert!(report.stderr().contains(&refused), "{refused}");
    }
    // A relative recorded path never reaches the door: the record's own reader
    // refuses the whole file, and every mark it names stays as it is.
    let mut relative = serde_json::to_value(&marks).unwrap();
    relative["agent_config_roots"]["claude"] = serde_json::json!(["relative/.claude"]);
    let bytes = serde_json::to_vec(&relative).unwrap();
    fs::write(
        data.join(crate::shell_integration::profile_marks::RECORD_FILE),
        &bytes,
    )
    .unwrap();
    let report = execute(&scope, false, system_absent);
    assert_eq!(report.code, 1, "{}", report.stdout());
    assert_eq!(fs::read(&agent).unwrap(), agent_before);
    assert_eq!(fs::read(&document).unwrap(), b"the reader's own file");
    assert!(module.exists());
    fs::remove_dir_all(root).unwrap();
}

/// The sandbox door is a test instrument: a shipped build does not read it, so no
/// stray value can turn a production cleanup into a report that everything is gone.
#[test]
fn uninstall_sandbox_door_is_not_read_by_a_shipped_build() {
    let value = Some(OsString::from(r"C:\scratch"));
    assert_eq!(sandbox_root(false, value.clone()), None);
    assert_eq!(
        sandbox_root(true, value),
        Some(PathBuf::from(r"C:\scratch"))
    );
    assert_eq!(sandbox_root(true, None), None);
    const { assert!(SANDBOX_DOOR, "a test build keeps the door") };
    let source = include_str!("uninstall.rs");
    assert!(source.contains("const SANDBOX_DOOR: bool = cfg!(any(debug_assertions, test));"));
    assert!(
        source.contains(r#"sandbox_root(SANDBOX_DOOR, std::env::var_os("BT_UNINSTALL_ROOT"))"#)
    );
    for script in [
        include_str!("../../../packaging/uninstall.cmd"),
        include_str!("../../../scripts/release/ci-build-tests.ps1"),
        include_str!("../../../scripts/release/cleanvm/in-guest.ps1"),
    ] {
        assert!(!script.contains("BT_UNINSTALL_ROOT"));
    }
}

/// What a purge row reports is what was true at the check that decided the deletion.
///
/// The row is the clipboard staging folder because it is a data root on every platform (a temp
/// row); the Windows-only `Local data (including WebView2)` this used to pick has no root on macOS
/// or Linux, so there the test found no row at all (ticket 72).
#[test]
fn uninstall_purge_reports_the_root_it_found_at_the_deciding_check() {
    let (root, scope) = sandbox("recreated");
    let (name, late) = scope
        .purge_roots
        .iter()
        .find_map(|(mark, path)| {
            (mark.name == "Clipboard staging").then(|| (mark.name, path.clone()))
        })
        .unwrap();
    assert!(!late.exists());
    let report = execute(&scope, true, |_| {
        // The inventory's system removers run between the preflight and the deletion.
        fs::create_dir_all(&late).unwrap();
        fs::write(late.join("arrived"), b"data").unwrap();
        vec![Entry::new("injected registration", Fate::Absent)]
    });
    assert_eq!(report.code, 0, "{}", report.stderr());
    assert!(!late.exists());
    let row = report
        .entries
        .iter()
        .find(|entry| entry.mark.starts_with(&format!("{name} (data)")))
        .unwrap();
    assert_eq!(row.fate, Fate::Removed, "{}", report.stdout());
    fs::remove_dir_all(root).unwrap();
}

/// A directory link at `link` naming `target`: a symlink on Unix, a junction on Windows (which
/// needs no developer-mode privilege, as `uninstall_purge_junction_refuses_root_without_touching_target`
/// already relies on).
fn plant_directory_link(link: &Path, target: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        // Through `bt_pty::test_shell`: no profile, a temporary HOME/APPDATA.
        let hygiene = bt_pty::test_shell::Hygiene::new();
        let status = hygiene
            .command("powershell.exe", bt_platform::quiet_command)
            .args([
                "-NonInteractive",
                "-Command",
                "New-Item -ItemType Junction -Path $env:FOLIO_TEST_JUNCTION -Target $env:FOLIO_TEST_TARGET -ErrorAction Stop | Out-Null",
            ])
            .env("FOLIO_TEST_JUNCTION", link)
            .env("FOLIO_TEST_TARGET", target)
            .status()
            .unwrap();
        assert!(status.success());
    }
    assert!(bt_platform::cleanup::is_link(
        &fs::symlink_metadata(link).unwrap()
    ));
}

/// The link itself, never what it names.
fn remove_directory_link(link: &Path) {
    #[cfg(unix)]
    fs::remove_file(link).unwrap();
    #[cfg(windows)]
    fs::remove_dir(link).unwrap();
}

/// RED (73) — **on macOS a temporary directory the system reaches through a link is not a planted
/// link: the two temporary rows are removed.**
///
/// macOS's own temporary directory is `/var/folders/…/T/`, and `/var` is the system's link to
/// `private/var`. The door refused every root with a link anywhere above it, so `--purge` reported
/// `Clipboard staging` and `Panic log` refused on every Mac (ticket 72's F1). The shape is built
/// here inside a sandbox — `var` → `private/var`, and the temporary directory named through `var`
/// — and handed to the resolver exactly as production hands it `std::env::temp_dir()`. The rows
/// resolve to the linked-to directory and both go.
///
/// MUTATION: in `purge_root`, join `folio` onto `head` as given instead of onto its canonical
/// spelling, and both rows are refused (`A symlink or junction was found`).
#[cfg(target_os = "macos")]
#[test]
fn an_os_named_temporary_base_behind_a_link_is_not_a_refusal() {
    let (root, _) = sandbox("os-link");
    let real = root.join("private/var/T");
    fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink("private/var", root.join("var")).unwrap();
    let scope = fixture_scope(&root, root.join("var/T"), true);
    let rows = ["Clipboard staging", "Panic log"];
    for (mark, path) in scope
        .purge_roots
        .iter()
        .filter(|(mark, _)| rows.contains(&mark.name))
    {
        assert!(path.starts_with(&real), "{}: {}", mark.name, path.display());
        if mark.name == "Panic log" {
            fs::write(path, b"panic").unwrap();
        } else {
            fs::create_dir_all(path).unwrap();
            fs::write(path.join("20260926-120000.png"), b"picture").unwrap();
        }
    }
    let report = execute(&scope, true, system_absent);
    assert_eq!(report.code, 0, "{}", report.stdout());
    for name in rows {
        let row = report
            .entries
            .iter()
            .find(|entry| entry.mark.starts_with(&format!("{name} (data)")))
            .unwrap();
        assert_eq!(row.fate, Fate::Removed, "{}", report.stdout());
    }
    assert!(!real.join("folio/clipboard").exists());
    assert!(!real.join("folio-panic.log").exists());
    fs::remove_file(root.join("var")).unwrap();
    fs::remove_dir_all(root).unwrap();
}

/// RED (73) — **a link inside the part of a root that Folio names is still refused, and what it
/// names is untouched.**
///
/// Resolving the system's head must not resolve Folio's own part with it: a link somebody plants at
/// `<temp>/folio`, where Folio's clipboard staging folder lives, would otherwise become authority
/// over its target, which is the whole reason the door refuses links. The link is planted before
/// the scope is resolved, so a resolver that resolved the whole root would see it and spell it
/// away.
///
/// MUTATION: in `purge_root`, canonicalize `head.join(folio)` instead of `head` alone, and the row
/// is removed through the link — the sentinel behind it goes too.
#[test]
fn a_link_inside_the_folio_named_part_is_still_refused() {
    let (root, _) = sandbox("folio-link");
    let outside = root.join("outside");
    fs::create_dir_all(outside.join("clipboard")).unwrap();
    fs::write(outside.join("clipboard/sentinel"), b"keep").unwrap();
    fs::create_dir_all(root.join("temp")).unwrap();
    let link = root.join("temp/folio");
    plant_directory_link(&link, &outside);
    let scope = Scope::sandbox(&root, root.join("app/folio.exe")).unwrap();
    let report = execute(&scope, true, system_absent);
    assert_eq!(report.code, 1, "{}", report.stdout());
    let row = report
        .entries
        .iter()
        .find(|entry| entry.mark.starts_with("Clipboard staging (data)"))
        .unwrap();
    assert!(
        matches!(&row.fate, Fate::Refused(why) if why.in_lang(Lang::English).contains(english(Text::CleanupLink))),
        "{}",
        report.stdout()
    );
    assert_eq!(
        fs::read(outside.join("clipboard/sentinel")).unwrap(),
        b"keep"
    );
    assert!(fs::symlink_metadata(&link).is_ok());
    remove_directory_link(&link);
    fs::remove_dir_all(root).unwrap();
}

/// RED (73) — **the boundary is the head the operating system names: that part is resolved, and
/// Folio's part below it is appended exactly as written.**
///
/// The head is spelled the way `std::env::temp_dir()` spells it (through `/var` on macOS, possibly
/// a short 8.3 name on Windows) and must come back as its canonical, ordinary spelling — the same
/// one this file's `link_free_temp_dir` derives independently. Folio's part carries a link
/// (`folio` → `elsewhere`) and must come back as `folio/clipboard`, not as the link's target. A
/// head that does not exist yet is resolved as far as it exists.
///
/// MUTATIONS: drop the canonicalize from `purge_root` and the head keeps its spelling (red on
/// macOS); canonicalize the whole root and `folio` becomes `elsewhere` (red everywhere).
#[test]
fn the_boundary_is_the_os_named_head() {
    let name = format!("folio-uninstall-boundary-{}", std::process::id());
    let spelled = std::env::temp_dir().join(&name);
    let resolved = link_free_temp_dir().join(&name);
    fs::create_dir_all(resolved.join("elsewhere")).unwrap();
    plant_directory_link(&resolved.join("folio"), &resolved.join("elsewhere"));
    assert_eq!(
        purge_root(&spelled, Path::new("folio/clipboard")),
        resolved.join("folio/clipboard")
    );
    assert_eq!(
        purge_root(&spelled.join("not-yet"), Path::new("folio")),
        resolved.join("not-yet/folio")
    );
    remove_directory_link(&resolved.join("folio"));
    fs::remove_dir_all(resolved).unwrap();
}

/// macOS cannot be asked whether a file is held, so a purge there says so instead of
/// implying a check that was never made.
#[test]
fn uninstall_purge_says_on_macos_that_held_data_cannot_be_told() {
    let notice = english(Text::CleanupMacHeld);
    assert_eq!(
        purge_notices(HostPlatform::MacOs, true),
        [Text::CleanupMacHeld]
    );
    assert!(purge_notices(HostPlatform::MacOs, false).is_empty());
    assert!(purge_notices(HostPlatform::Windows, true).is_empty());
    let report = Report::new(vec![Entry::new("Roaming data (data)", Fate::Removed)])
        .noticing(purge_notices(HostPlatform::MacOs, true));
    assert_eq!(report.code, 0);
    assert!(report.stdout().starts_with(&format!("{notice}\n")));
    assert!(report.stderr().is_empty());
    assert!(
        include_str!("../../bt-platform/src/cleanup.rs").contains("let _ = path;"),
        "probe_file is no longer the no-op this notice exists for"
    );
}

#[cfg(windows)]
#[test]
fn uninstall_existing_instance_mechanism_blocks_the_door() {
    let (root, scope) = sandbox("native-claim");
    seed(&scope, &scope.exe);
    let profile = scope.profiles.as_ref().unwrap()[0].clone();
    let before = fs::read(&profile).unwrap();
    let claim = bt_platform::instance::claim_data_directory(&scope.data[0]).unwrap();
    let report = execute(&scope, true, |_| panic!("must not reach any remover"));
    assert_eq!(report.code, 2);
    assert_eq!(fs::read(profile).unwrap(), before);
    drop(claim);
    fs::remove_dir_all(root).unwrap();
}

/// A bundle-shaped application under a sandbox root, as `/Applications/Folio.app`
/// is laid out: the executable at `Contents/MacOS/folio`.
fn bundle_exe(root: &Path) -> PathBuf {
    root.join("Applications/Folio.app/Contents/MacOS/folio")
}

/// RED (U-26) — **on macOS the door finds the update entrances in the account's
/// `~/Library/LaunchAgents` and the installation home beside the bundle it runs
/// from; on every other platform it has neither.**
///
/// §(b).3: both are written outside Folio's own folder on macOS, so both need an
/// `--uninstall-cleanup` row, and the home is found from the bundle's path (the
/// locator of F-3), never from a data root. The Windows home is inside the install
/// folder and needs no row.
///
/// MUTATION: in `Scope::resolve`, derive the home from the data root
/// (`data[0].join(".Folio.app.folio-update")`) instead of `update_txn::Home::of`.
#[test]
fn uninstall_finds_the_update_marks_beside_the_bundle_on_macos() {
    let (root, _) = sandbox("update-marks");
    let mapped = root.clone();
    let env = move |name: &str| {
        Some(
            mapped
                .join(match name {
                    "APPDATA" => "roaming",
                    "LOCALAPPDATA" => "local",
                    "HOME" | "USERPROFILE" => "home",
                    "BT_PSREADLINE_DOCUMENTS" => "documents",
                    _ => return None,
                })
                .into_os_string(),
        )
    };
    let scope = Scope::resolve(
        bundle_exe(&root),
        HostPlatform::MacOs,
        &env,
        root.join("temp"),
        false,
    )
    .unwrap();
    assert_eq!(
        scope.launch_agents,
        Some(root.join("home/Library/LaunchAgents"))
    );
    assert_eq!(
        scope.update_home,
        Some(root.join("Applications/.Folio.app.folio-update"))
    );
    for platform in [HostPlatform::Windows, HostPlatform::OtherUnix] {
        let scope =
            Scope::resolve(bundle_exe(&root), platform, &env, root.join("temp"), false).unwrap();
        assert_eq!(scope.launch_agents, None, "{platform:?}");
        assert_eq!(scope.update_home, None, "{platform:?}");
    }
    fs::remove_dir_all(root).unwrap();
}

/// RED (U-26) — **the two update rows remove our entrances and our home, and
/// nothing beside them**: another program's agent, a plist of a near-miss name,
/// the bundle itself and a home that belongs to another bundle all stay; a rerun
/// finds nothing.
///
/// (b).3's rows, run through the whole door with the real removers over a
/// temporary tree standing in for `~/Library/LaunchAgents` and `/Applications`.
///
/// MUTATION: in `update_entrances`, sweep with a remover that takes every file in
/// the folder (replace `launch_agent::sweep`'s `is_ours` filter with `true`).
#[test]
fn uninstall_update_rows_remove_only_ours() {
    if bt_platform::host_platform() == HostPlatform::OtherUnix {
        // No arm of the update door removes anything here, and no row runs.
        return;
    }
    let (root, mut scope) = sandbox("update-rows");
    let agents = root.join("home/Library/LaunchAgents");
    let applications = root.join("Applications");
    let home = applications.join(".Folio.app.folio-update");
    fs::create_dir_all(&agents).unwrap();
    fs::create_dir_all(home.join("0123456789abcdef0123456789abcdef/rescue/Folio.app")).unwrap();
    fs::write(home.join("journal.json"), b"{}").unwrap();
    fs::write(home.join("lock"), b"").unwrap();
    fs::create_dir_all(applications.join("Folio.app/Contents/MacOS")).unwrap();
    fs::create_dir_all(applications.join(".Other.app.folio-update")).unwrap();
    let ours = [
        bt_platform::launch_agent::file_name(&[0x01; 16]),
        bt_platform::launch_agent::file_name(&[0xab; 16]),
    ];
    for name in &ours {
        fs::write(agents.join(name), b"plist").unwrap();
    }
    let theirs = [
        "com.example.agent.plist",
        "io.github.lulu-loopp.folio.update-01010101.plist.bak",
    ];
    for name in theirs {
        fs::write(agents.join(name), b"theirs").unwrap();
    }
    scope.launch_agents = Some(agents.clone());
    scope.update_home = Some(home.clone());

    let report = execute(&scope, false, system_absent);
    // Only the two update rows are asked about: on macOS every other row of a
    // sandbox under `$TMPDIR` refuses, because `/var` is a link to `/private/var`
    // and those rows refuse a path with a link among its ancestors.
    let update_refusals: Vec<String> = report
        .entries
        .iter()
        .filter(|e| e.mark.starts_with("Update ") && matches!(e.fate, Fate::Refused(_)))
        .map(|entry| entry.line(Lang::English))
        .collect();
    assert!(update_refusals.is_empty(), "{update_refusals:?}");
    let stdout = report.stdout();
    for name in &ours {
        assert!(!agents.join(name).exists(), "{name}");
        assert!(
            stdout.contains(&format!("{}: removed", agents.join(name).display())),
            "{stdout}"
        );
    }
    for name in theirs {
        assert!(agents.join(name).exists(), "{name}");
    }
    assert!(!home.exists());
    assert!(
        stdout.contains(&format!(
            "Update home beside the bundle (per-copy): {}: removed",
            home.display()
        )),
        "{stdout}"
    );
    assert!(applications.join("Folio.app/Contents/MacOS").exists());
    assert!(applications.join(".Other.app.folio-update").exists());

    let second = execute(&scope, false, system_absent).stdout();
    assert!(
        second.contains("Update entrances (LaunchAgents) (per-account): not present"),
        "{second}"
    );
    assert!(
        second.contains(&format!(
            "Update home beside the bundle (per-copy): {}: not present",
            home.display()
        )),
        "{second}"
    );
    fs::remove_dir_all(root).unwrap();
}

/// RED (U-27) — **the update home's row detaches an image still mounted inside
/// the home before it removes the home**, so a leftover mount no longer leaves
/// the home behind (macOS: the only platform that mounts an update's image).
///
/// U-17's debt 7, the coordinator's ruling: a durable removal descends into a
/// read-only volume and fails. The image is attached the way a dead Prepare
/// leaves it, under `H/<txn>/mnt`, with no record.
///
/// MUTATION: in `update_home`, drop the `detach_images_under` call.
#[test]
fn uninstall_update_home_detaches_before_removing() {
    use crate::update_prepare_macos::tests::fixture;
    if !fixture::on_macos() {
        return;
    }
    let scratch = fixture::Scratch::new("uninstall-home");
    let home = scratch.root.join(".Folio.app.folio-update");
    let image = fixture::blank_image(&scratch.root.join("left.dmg"));
    fixture::attach(&image, &home.join("0123456789abcdef0123456789abcdef/mnt"));
    fs::write(home.join("journal.json"), b"{}").unwrap();
    let entry = super::update_home("Update home beside the bundle (per-copy)", Some(&home));
    assert!(
        matches!(entry.fate, Fate::Removed),
        "{}",
        entry.line(Lang::English)
    );
    assert!(
        fixture::mounted(&scratch.root).is_empty(),
        "the image is detached"
    );
    assert!(!home.exists(), "and the home removed");
}

/// PIN (U-26) — **each update row names the writer it undoes, and the writer is
/// where the row says**: the entrance door writes its plist through the durable
/// write and sweeps only its own names, and the home is the locator's.
///
/// Read through `bt_source` (the standing rule for a guard over Folio's own
/// source), by identity rather than by file.
#[test]
fn uninstall_update_rows_name_their_writers() {
    let platform = bt_source::Index::of_package("bt-platform");
    let body = |name: &str| {
        platform
            .body_of(&bt_source::ItemQuery::function(name).in_module("crate::launch_agent"))
            .unwrap_or_else(|failure| panic!("{failure}"))
    };
    assert!(body("arm").contains("arm_with("));
    assert!(body("arm_with").contains("durable_write_with("));
    assert!(body("sweep").contains("is_ours("));
    assert!(method_body("Home", "for_bundle").contains("MACOS_HOME_SUFFIX"));
    for (remover, writer) in [
        (Remover::UpdateEntrances, "launch_agent.rs:arm"),
        (Remover::UpdateHome, "update_txn.rs:Home::for_bundle"),
    ] {
        assert!(
            INVENTORY
                .iter()
                .any(|mark| mark.remover == remover && mark.writer.contains(writer)),
            "writer has no undo: {writer}"
        );
    }
}

// ── `--uninstall` (T-UNINSTALL-UX) ──────────────────────────────────────────
//
// Every test below runs the door's own functions over a sandbox (`sandbox`), on a worker the
// thread door started — the door's main thread is a worker (`uninstall::standalone`), and the
// program's walk is a worker's door. No test runs a Folio binary, and the remover is started
// for real only over a sandbox folder, waiting for a child this test started.

/// Run `body` on a worker the thread door started, and wait for it.
fn on_a_worker<T: Send + 'static>(body: impl FnOnce(&WorkerCtx) -> T + Send + 'static) -> T {
    match bt_platform::spawn_at_priority(
        "bt-uninstall-test",
        bt_platform::ThreadPriority::BelowNormal,
        body,
    )
    .expect("the thread door starts a thread")
    .join()
    {
        Ok(answer) => answer,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// The fixture program's folder: `folio.exe` (the fixture), and beside it two files the release
/// installs and the update's installation home with a file in it.
fn seed_program(scope: &Scope) -> PathBuf {
    let app = scope.exe.parent().unwrap().to_path_buf();
    fs::write(app.join("conpty.dll"), b"sidecar").unwrap();
    fs::write(app.join("uninstall.cmd"), b"@echo off").unwrap();
    fs::create_dir_all(app.join(".folio-update/aa")).unwrap();
    fs::write(app.join(".folio-update/aa/journal.json"), b"{}").unwrap();
    app
}

/// The members a release manifest would list for the fixture.
fn members(exe: &Path) -> Result<Vec<bt_winres::release_manifest::Member>, String> {
    ["conpty.dll", "uninstall.cmd"]
        .into_iter()
        .map(|name| {
            let bytes = fs::read(exe.parent().unwrap().join(name)).map_err(|e| e.to_string())?;
            Ok(bt_winres::release_manifest::Member {
                name: name.to_owned(),
                sha256: bt_winres::digest::hex(&bt_winres::digest::sha256(&bytes)),
                size: bytes.len() as u64,
            })
        })
        .collect()
}

/// A data root with a settings file naming `language`.
fn settings_speaking(scope: &Scope, language: bt_persist::LanguageV1) {
    fs::create_dir_all(&scope.data[0]).unwrap();
    bt_persist::write_settings_atomic(
        &scope.data[0].join(crate::persist::SETTINGS_FILE_NAME),
        &bt_persist::SettingsV1 {
            language,
            ..bt_persist::SettingsV1::default()
        },
    )
    .unwrap();
}

/// RED (T-UNINSTALL-UX) — **`--uninstall` keeps settings and data, and hands the program's own
/// files to the remover — the release's members, the executable, the update's home and then the
/// folder if it is empty — to go once the process that asked has gone.**
///
/// The cleanup is the door's own (`execute`, as `--uninstall-cleanup` runs it), the plan is the
/// real one over the fixture's folder, and the remover is a recording stand-in: what the door
/// would hand `deferred_removal::schedule` is what is asserted. The data root, with a settings
/// file in it, is still there afterwards.
///
/// MUTATION: in `uninstall`, pass `true` for `purge` to the cleanup (the door ignoring
/// `--remove-data`'s absence), and the data root is gone.
#[test]
fn the_uninstall_keeps_settings_and_data_and_hands_the_program_to_the_remover() {
    let (root, scope) = sandbox("uninstall-keeps");
    seed(&scope, &scope.exe);
    settings_speaking(&scope, bt_persist::LanguageV1::English);
    let app = seed_program(&scope);
    let scope = std::sync::Arc::new(scope);
    let asked = std::sync::Arc::new(Mutex::new(None::<Removal>));
    let report = {
        let (scope, asked) = (scope.clone(), asked.clone());
        on_a_worker(move |worker| {
            uninstall(
                worker,
                &scope,
                None,
                AFTER_PID_WITHIN,
                |scope, program| execute_then(scope, false, system_absent, Some(program)),
                |scope| {
                    remove_the_program(
                        worker,
                        scope,
                        crate::install_channel::Channel::Ours,
                        HostPlatform::Windows,
                        members,
                        &[],
                        |_, removal, _| {
                            *asked.lock().unwrap() = Some(removal.clone());
                            Ok(())
                        },
                    )
                },
            )
            .stdout()
        })
    };
    let removal = asked.lock().unwrap().take().expect("the remover was asked");
    let app =
        bt_platform::handoff::strip_verbatim_prefix(&bt_platform::instance::canonical_path(&app));
    assert_eq!(
        removal.items.iter().map(Item::path).collect::<Vec<_>>(),
        [
            app.join("conpty.dll"),
            app.join("uninstall.cmd"),
            app.join("folio.exe"),
            app.join(".folio-update/aa/journal.json"),
            app.join(".folio-update/aa"),
            app.join(".folio-update"),
        ]
    );
    assert_eq!(removal.folder, Some(app.clone()));
    assert!(
        scope.data[0]
            .join(crate::persist::SETTINGS_FILE_NAME)
            .exists(),
        "settings and data stay"
    );
    assert!(
        report.contains(&format!(
            "Program files (per-copy): {}: removed when this window closes\n",
            app.display()
        )),
        "{report}"
    );
    fs::remove_dir_all(root).unwrap();
}

/// RED (T-UNINSTALL-UX) — **`--uninstall --remove-data` removes the data roots too, exactly as
/// `--uninstall-cleanup --purge` does, and still hands the program to the remover.**
///
/// MUTATION: in `run_within`, hand the `Uninstall` verb's cleanup `false` whatever
/// `remove_data` says — this test's cleanup is `execute(scope, true, …)`, so the mutation is the
/// door's; here what is pinned is that a purge inside `uninstall` leaves the program row intact.
#[test]
fn the_uninstall_with_remove_data_removes_the_data_roots_and_the_program() {
    let (root, scope) = sandbox("uninstall-removes");
    seed(&scope, &scope.exe);
    settings_speaking(&scope, bt_persist::LanguageV1::English);
    seed_program(&scope);
    let scope = std::sync::Arc::new(scope);
    let scheduled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let report = {
        let (scope, scheduled) = (scope.clone(), scheduled.clone());
        on_a_worker(move |worker| {
            let report = uninstall(
                worker,
                &scope,
                None,
                AFTER_PID_WITHIN,
                |scope, program| execute_then(scope, true, system_absent, Some(program)),
                |scope| {
                    remove_the_program(
                        worker,
                        scope,
                        crate::install_channel::Channel::Ours,
                        HostPlatform::Windows,
                        members,
                        &[],
                        |_, _, _| {
                            scheduled.store(true, std::sync::atomic::Ordering::Relaxed);
                            Ok(())
                        },
                    )
                },
            );
            (report.code, report.stdout())
        })
    };
    assert_eq!(report.0, 0, "{}", report.1);
    assert!(!scope.data[0].exists(), "the data root is removed");
    assert!(scheduled.load(std::sync::atomic::Ordering::Relaxed));
    fs::remove_dir_all(root).unwrap();
}

/// PIN (release read M1) — **nothing irreversible happens before every step that can refuse has
/// been decided**: with `--remove-data`, a removal row that refused keeps the program *and* the
/// data, and every data row says it was kept; a program step that refused keeps the data too;
/// and when the purge does run, the program's step has already been decided — the data root is
/// still there when the program's step is asked.
///
/// RED (mutations: `purge_regardless` — the purge runs whatever was refused before it, as it did:
/// the data root is gone beside a kept program; `purge_before_program` — the purge runs before
/// the program's step: the step finds the data root gone).
#[test]
fn a_refusal_before_the_purge_keeps_the_data_and_the_purge_comes_last() {
    let refusing = |remover| match remover {
        Remover::Explorer => vec![Entry::new(
            "Explorer registrations (injected)",
            Fate::Refused(Why::Other("held by another program".to_owned())),
        )],
        _ => system_absent(remover),
    };
    for (row_refuses, program_refuses) in [(true, false), (false, true), (false, false)] {
        let (root, scope) = sandbox("uninstall-order");
        seed(&scope, &scope.exe);
        settings_speaking(&scope, bt_persist::LanguageV1::English);
        let scope = std::sync::Arc::new(scope);
        let data = scope.data[0].clone();
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (code, stdout) = {
            let (scope, asked, data) = (scope.clone(), asked.clone(), data.clone());
            on_a_worker(move |worker| {
                let report = uninstall(
                    worker,
                    &scope,
                    None,
                    AFTER_PID_WITHIN,
                    |scope, program| {
                        if row_refuses {
                            execute_then(scope, true, refusing, Some(program))
                        } else {
                            execute_then(scope, true, system_absent, Some(program))
                        }
                    },
                    |scope| {
                        asked.store(true, std::sync::atomic::Ordering::SeqCst);
                        assert!(data.exists(), "the program's step comes before the purge");
                        vec![Entry::new(
                            program_label(scope.lang),
                            if program_refuses {
                                Fate::Refused(Why::Said(Text::CleanupLink))
                            } else {
                                Fate::Scheduled
                            },
                        )]
                    },
                );
                (report.code, report.stdout())
            })
        };
        let case = format!("row refuses {row_refuses}, program refuses {program_refuses}");
        if row_refuses || program_refuses {
            assert_eq!(code, 1, "{case}: {stdout}");
            assert!(data.exists(), "{case}: the data is kept: {stdout}");
            assert!(
                stdout.contains(&format!(
                    "{}: kept (the cleanup did not complete)\n",
                    data.display()
                )),
                "{case}: {stdout}"
            );
            assert_eq!(
                asked.load(std::sync::atomic::Ordering::SeqCst),
                !row_refuses,
                "{case}"
            );
            if row_refuses {
                assert!(
                    stdout.ends_with(
                        "Program files (per-copy): kept (the cleanup did not complete)\n"
                    ),
                    "{case}: {stdout}"
                );
            }
        } else {
            assert_eq!(code, 0, "{case}: {stdout}");
            assert!(!data.exists(), "{case}: the data is purged last");
            assert!(asked.load(std::sync::atomic::Ordering::SeqCst));
        }
        fs::remove_dir_all(root).unwrap();
    }
}

/// RED (T-UNINSTALL-UX) — **a link among the program's files refuses the whole removal: nothing
/// is handed to the remover, the row says `refused`, and the exit code is 1.**
///
/// The update's home is replaced by a directory link to a folder outside the program's
/// (a junction on Windows), with a file behind it that must survive.
///
/// MUTATION: in `program_plan`, push `Item::Directory(path)` without `links_below` first, and the
/// remover is asked.
#[test]
fn a_link_among_the_programs_files_refuses_the_removal() {
    let (root, scope) = sandbox("uninstall-link");
    let app = seed_program(&scope);
    let outside = root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"keep").unwrap();
    fs::remove_dir_all(app.join(".folio-update/aa")).unwrap();
    plant_directory_link(&app.join(".folio-update/aa"), &outside);
    let scope = std::sync::Arc::new(scope);
    let (code, stdout) = {
        let scope = scope.clone();
        on_a_worker(move |worker| {
            let report = uninstall(
                worker,
                &scope,
                None,
                AFTER_PID_WITHIN,
                |scope, program| execute_then(scope, false, system_absent, Some(program)),
                |scope| {
                    remove_the_program(
                        worker,
                        scope,
                        crate::install_channel::Channel::Ours,
                        HostPlatform::Windows,
                        members,
                        &[],
                        |_, _, _| panic!("a link among the program's files is never handed over"),
                    )
                },
            );
            (report.code, report.stdout())
        })
    };
    assert_eq!(code, 1, "{stdout}");
    assert!(
        stdout.contains("Program files (per-copy): refused (A symlink or junction was found"),
        "{stdout}"
    );
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"keep");
    remove_directory_link(&app.join(".folio-update/aa"));
    fs::remove_dir_all(root).unwrap();
}

/// RED (T-UNINSTALL-UX) — **the program is the folder the running executable lives in and only
/// what the release put there: a file of the person's own beside it stays, is named, and keeps
/// the folder; nothing outside that folder is ever an item.**
///
/// There is no parameter through which a caller could name another folder: the plan is derived
/// from the executable alone.
///
/// MUTATION: in `program_plan`, remove the folder with `Item::Directory(root)` instead of its
/// members and the empty-folder step — the person's file is then an item.
#[test]
fn a_file_the_release_did_not_install_stays_and_is_named() {
    let (root, scope) = sandbox("uninstall-foreign");
    let app = seed_program(&scope);
    fs::write(app.join("thesis.pdf"), b"mine").unwrap();
    // A nested owned file with this basename must not hide the root-level
    // personal file from the report.
    fs::write(app.join("journal.json"), b"mine too").unwrap();
    let exe = scope.exe.clone();
    let plan = on_a_worker(move |worker| {
        program_plan(worker, &exe, HostPlatform::Windows, members).expect("a plan")
    });
    let app =
        bt_platform::handoff::strip_verbatim_prefix(&bt_platform::instance::canonical_path(&app));
    assert_eq!(plan.root, app);
    assert!(plan.items.iter().all(|item| item.path().starts_with(&app)));
    assert!(
        !plan
            .items
            .iter()
            .any(|item| item.path() == app.join("thesis.pdf"))
    );
    assert_eq!(
        plan.not_ours,
        [app.join("journal.json"), app.join("thesis.pdf")]
    );
    assert_eq!(plan.folder, Some(app));
    fs::remove_dir_all(root).unwrap();
}

/// RED (T-UNINSTALL-UX round 2, mutation `manifest_name_is_identity`) — a
/// same-name file whose bytes differ from the manifest survives and is named as
/// not Folio's. The name alone grants no deletion authority.
#[test]
fn a_pre_existing_same_name_personal_file_survives_and_is_named() {
    let (root, scope) = sandbox("uninstall-personal-name");
    let app = seed_program(&scope);
    let expected = bt_winres::release_manifest::Member {
        name: "conpty.dll".to_owned(),
        sha256: bt_winres::digest::hex(&bt_winres::digest::sha256(b"released sidecar")),
        size: b"released sidecar".len() as u64,
    };
    fs::write(app.join("conpty.dll"), b"my personal bytes").unwrap();
    let exe = scope.exe.clone();
    let plan = on_a_worker(move |worker| {
        program_plan(worker, &exe, HostPlatform::Windows, |_| Ok(vec![expected])).expect("a plan")
    });
    let app = resolved(&app);
    assert!(
        !plan
            .items
            .iter()
            .any(|item| item.path() == app.join("conpty.dll"))
    );
    assert!(plan.not_ours.contains(&app.join("conpty.dll")));
    assert_eq!(
        fs::read(app.join("conpty.dll")).unwrap(),
        b"my personal bytes"
    );
    fs::remove_dir_all(root).unwrap();
}

/// RED (T-UNINSTALL-UX round 2, mutation `case_fold_not_ours`) — a case-only
/// twin is not the exact scheduled member and remains in the not-ours report.
#[test]
fn a_case_only_twin_is_reported_as_not_ours() {
    let names = vec![OsString::from("conpty.dll"), OsString::from("CONPTY.DLL")];
    let left = unowned_names(names, &[OsString::from("conpty.dll")]);
    assert_eq!(left, [OsString::from("CONPTY.DLL")]);

    let (root, scope) = sandbox("uninstall-case-twin");
    let app = seed_program(&scope);
    if !bt_platform::directory_folds_case(&app) {
        fs::write(app.join("CONPTY.DLL"), b"personal twin").unwrap();
        let exe = scope.exe.clone();
        let plan = on_a_worker(move |worker| {
            program_plan(worker, &exe, HostPlatform::Windows, members).expect("a plan")
        });
        let app = resolved(&app);
        assert!(plan.not_ours.contains(&app.join("CONPTY.DLL")));
        assert_eq!(fs::read(app.join("CONPTY.DLL")).unwrap(), b"personal twin");
    }
    fs::remove_dir_all(root).unwrap();
}

/// RED (T-UNINSTALL-UX round 2, mutation `accept_manifest_hard_link`) — a
/// manifest-named hard link refuses the whole plan and both names survive.
#[test]
fn a_hard_linked_program_file_is_refused_and_reported() {
    let (root, scope) = sandbox("uninstall-hard-link");
    let app = seed_program(&scope);
    let other = root.join("personal-sidecar.dll");
    fs::remove_file(app.join("conpty.dll")).unwrap();
    fs::write(&other, b"sidecar").unwrap();
    fs::hard_link(&other, app.join("conpty.dll")).unwrap();
    let exe = scope.exe.clone();
    let error = on_a_worker(move |worker| {
        program_plan(worker, &exe, HostPlatform::Windows, members).unwrap_err()
    });
    assert!(error.in_lang(Lang::English).contains("hard link"));
    assert_eq!(fs::read(&other).unwrap(), b"sidecar");
    assert_eq!(fs::read(app.join("conpty.dll")).unwrap(), b"sidecar");
    fs::remove_dir_all(root).unwrap();
}

/// RED (T-UNINSTALL-UX) — **on a macOS bundle the program is the bundle, whole, and outside one
/// it is the executable alone.**
///
/// MUTATION: in `program_plan`, answer the executable's folder for a bundle too, and the bundle's
/// `Contents/MacOS` is the item.
#[test]
fn a_bundle_is_removed_whole_and_a_loose_executable_alone() {
    let (root, _) = sandbox("uninstall-bundle");
    let exe = bundle_exe(&root);
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, b"fixture executable").unwrap();
    fs::write(
        root.join("Applications/Folio.app/Contents/Info.plist"),
        b"plist",
    )
    .unwrap();
    let loose = root.join("app/folio.exe");
    let (bundle, alone) = on_a_worker(move |worker| {
        (
            program_plan(worker, &exe, HostPlatform::MacOs, |_| {
                panic!("a bundle has no manifest to ask")
            })
            .expect("a plan"),
            program_plan(worker, &loose, HostPlatform::MacOs, |_| {
                panic!("a loose executable has no manifest to ask")
            })
            .expect("a plan"),
        )
    });
    let bundle_root = resolved(&root.join("Applications/Folio.app"));
    assert_eq!(bundle.items.len(), 5);
    assert!(
        bundle
            .items
            .iter()
            .all(|item| item.path().starts_with(&bundle_root))
    );
    assert!(bundle.items.iter().any(|item| {
        matches!(item, Item::File { path, .. } if path == &bundle_root.join("Contents/MacOS/folio"))
    }));
    assert!(bundle.items.iter().any(|item| {
        matches!(item, Item::File { path, .. } if path == &bundle_root.join("Contents/Info.plist"))
    }));
    assert!(bundle.items.contains(&Item::Directory(bundle_root.clone())));
    assert_eq!(bundle.folder, None);
    assert_eq!(
        alone.items.iter().map(Item::path).collect::<Vec<_>>(),
        [resolved(&root.join("app/folio.exe"))]
    );
    assert_eq!(alone.folder, None);
    fs::remove_dir_all(root).unwrap();
}

/// RED (T-UNINSTALL-UX round 2, mutation `canonicalize_before_bundle_link`) —
/// `/tmp/Folio.app -> real/Folio.app` is refused before canonicalization can
/// erase the launch spelling.
#[test]
fn a_bundle_reached_through_a_symlink_is_refused() {
    let (root, _) = sandbox("uninstall-bundle-link");
    let real_bundle = root.join("real/Folio.app");
    let real_exe = real_bundle.join("Contents/MacOS/folio");
    fs::create_dir_all(real_exe.parent().unwrap()).unwrap();
    fs::write(&real_exe, b"fixture executable").unwrap();
    let link = root.join("Folio.app");
    plant_directory_link(&link, &real_bundle);
    let linked_exe = link.join("Contents/MacOS/folio");
    let error = on_a_worker(move |worker| {
        program_plan(worker, &linked_exe, HostPlatform::MacOs, |_| {
            panic!("a linked bundle is refused before its manifest")
        })
        .unwrap_err()
    });
    assert!(error.in_lang(Lang::English).contains("symlink or junction"));
    remove_directory_link(&link);
    fs::remove_dir_all(root).unwrap();
}

/// RED (T-UNINSTALL-UX round 6, mutation `refuse_bundle_ancestor_links`) — an
/// ancestor above the `.app` name may be a symlink: macOS's ordinary `/tmp` and
/// `/var` spellings require ancestors to be canonicalized. The bundle name and
/// everything below it remain subject to the adjacent refusal test.
#[test]
fn a_bundle_under_a_symlinked_ancestor_is_planned_by_its_canonical_path() {
    let (root, _) = sandbox("uninstall-bundle-linked-ancestor");
    let real_parent = root.join("real");
    let real_bundle = real_parent.join("Folio.app");
    let real_exe = real_bundle.join("Contents/MacOS/folio");
    fs::create_dir_all(real_exe.parent().unwrap()).unwrap();
    fs::write(&real_exe, b"fixture executable").unwrap();
    fs::write(real_bundle.join("Contents/Info.plist"), b"plist").unwrap();
    let alias = root.join("alias");
    plant_directory_link(&alias, &real_parent);
    let linked_exe = alias.join("Folio.app/Contents/MacOS/folio");
    let plan = on_a_worker(move |worker| {
        program_plan(worker, &linked_exe, HostPlatform::MacOs, |_| {
            panic!("a bundle has no manifest to ask")
        })
        .expect("a linked ancestor is canonicalized into a program plan")
    });
    let canonical_bundle = resolved(&real_bundle);
    assert_eq!(plan.root, canonical_bundle);
    assert!(
        plan.items
            .iter()
            .all(|item| item.path().starts_with(&canonical_bundle))
    );
    remove_directory_link(&alias);
    fs::remove_dir_all(root).unwrap();
}

/// RED (T-UNINSTALL-UX) — **a copy a package manager installed is left to its manager: nothing is
/// handed to the remover, and the row names the manager's command — with the cleanup before it
/// for a manager that runs none.**
///
/// MUTATION: in `remove_the_program`, drop the `Channel::Managed` arm, and the remover is asked.
#[test]
fn a_managed_copy_is_left_to_its_manager() {
    let (root, scope) = sandbox("uninstall-managed");
    seed_program(&scope);
    let scope = std::sync::Arc::new(scope);
    let lines = {
        let scope = scope.clone();
        on_a_worker(move |worker| {
            [(Manager::Scoop, true), (Manager::Winget, false)].map(|(manager, uninstall_hook)| {
                remove_the_program(
                    worker,
                    &scope,
                    crate::install_channel::Channel::Managed {
                        manager,
                        uninstall_hook,
                    },
                    HostPlatform::Windows,
                    members,
                    &[],
                    |_, _, _| panic!("a managed copy is never handed to the remover"),
                )
                .into_iter()
                .map(|entry| entry.line(Lang::English))
                .collect::<String>()
            })
        })
    };
    assert_eq!(
        lines,
        [
            "Program files (per-copy): left to the package manager: scoop uninstall folio\n",
            "Program files (per-copy): left to the package manager: cmd /c \"folio \
             --uninstall-cleanup && winget uninstall --id WeiyiShi.Folio --exact\"\n",
        ]
    );
    fs::remove_dir_all(root).unwrap();
}

/// RED (T-UNINSTALL-UX) — **a cleanup that did not complete keeps the program, and says so: the
/// program is the one thing that can run the cleanup again.**
///
/// MUTATION: in `uninstall`, go on to `program` whatever the cleanup's code.
#[test]
fn a_cleanup_that_did_not_complete_keeps_the_program() {
    let (root, scope) = sandbox("uninstall-incomplete");
    let scope = std::sync::Arc::new(scope);
    let (code, stdout) = {
        let scope = scope.clone();
        on_a_worker(move |worker| {
            let report = uninstall(
                worker,
                &scope,
                None,
                AFTER_PID_WITHIN,
                |_, _| {
                    Report::new(vec![Entry::new(
                        "Claude Code hooks (per-copy)",
                        Fate::Refused(Why::Said(Text::CleanupRecorded)),
                    )])
                },
                |_| panic!("the program is not touched after an incomplete cleanup"),
            );
            (report.code, report.stdout())
        })
    };
    assert_eq!(code, 1);
    assert!(
        stdout.ends_with("Program files (per-copy): kept (the cleanup did not complete)\n"),
        "{stdout}"
    );
    fs::remove_dir_all(root).unwrap();
}

/// RED (T-UNINSTALL-UX) — **while a Folio holds the data, `--uninstall` answers exit 2, changes
/// nothing, and keeps the program.**
///
/// The instance claim is held by the test (the door's own gate, `execute_with_claim` answering
/// that the claim is taken), exactly as a running Folio holds it.
///
/// MUTATION: in `uninstall`, return the cleanup's report before the program row is added
/// only when its code is 1 (so a 2 goes on to the program).
#[test]
fn a_running_folio_is_exit_two_and_nothing_is_removed() {
    let (root, scope) = sandbox("uninstall-running");
    seed(&scope, &scope.exe);
    let profile = scope.profiles.as_ref().unwrap()[0].clone();
    let before = fs::read(&profile).unwrap();
    let scope = std::sync::Arc::new(scope);
    let (code, stdout) = {
        let scope = scope.clone();
        on_a_worker(move |worker| {
            let report = uninstall(
                worker,
                &scope,
                None,
                AFTER_PID_WITHIN,
                |scope, program| {
                    super::execute_with_claim(
                        scope,
                        false,
                        |_| panic!("must not reach a system remover"),
                        |_| None::<()>,
                        Some(program),
                    )
                },
                |_| panic!("a running Folio keeps the program"),
            );
            (report.code, report.stdout())
        })
    };
    assert_eq!(code, 2, "{stdout}");
    assert!(
        stdout.starts_with("Folio: refused (A Folio instance is running; nothing was changed.)\n")
    );
    assert_eq!(fs::read(profile).unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}

/// RED (T-UNINSTALL-UX) — **with `--after-pid`, the door waits for the asker to end before it
/// touches anything; an asker still running at the bound is answered as a running Folio, with
/// nothing touched.**
///
/// The wait seam announces that it has started and cannot finish until the
/// test releases it. The controlled receiver is the ordering proof; no
/// wall-clock delay decides whether the ordering passed.
///
/// MUTATION: in `uninstall`, run the cleanup before `waited_for` (or skip the wait).
#[test]
fn the_door_waits_for_the_folio_that_asked_before_it_touches_anything() {
    let (root, scope) = sandbox("uninstall-after");
    let scope = std::sync::Arc::new(scope);
    let asker = Running {
        pid: 4242,
        started: 101,
    };
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);
    let touched = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let joined = {
        let scope = scope.clone();
        let touched = touched.clone();
        bt_platform::spawn_at_priority(
            "bt-uninstall-order-test",
            bt_platform::ThreadPriority::BelowNormal,
            move |worker| {
                uninstall_waiting(
                    worker,
                    &scope,
                    Some(asker),
                    AFTER_PID_WITHIN,
                    |_, process, _| {
                        assert_eq!(process, asker);
                        entered_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        true
                    },
                    |scope, _| {
                        touched.store(true, std::sync::atomic::Ordering::SeqCst);
                        Report::new(Vec::new()).in_lang(scope.lang)
                    },
                    |_| Vec::new(),
                )
            },
        )
        .unwrap()
    };
    entered_rx
        .recv()
        .expect("the wait announces readiness through the controlled receiver");
    assert!(!touched.load(std::sync::atomic::Ordering::SeqCst));
    release_tx.send(()).unwrap();
    assert_eq!(joined.join().unwrap().code, 0);
    assert!(touched.load(std::sync::atomic::Ordering::SeqCst));

    let code = on_a_worker({
        let scope = scope.clone();
        move |worker| {
            uninstall_waiting(
                worker,
                &scope,
                Some(asker),
                AFTER_PID_WITHIN,
                |_, process, _| {
                    assert_eq!(process, asker);
                    false
                },
                |_, _| panic!("nothing is touched when the identity-safe wait reaches its bound"),
                |_| panic!("nothing is touched when the identity-safe wait reaches its bound"),
            )
            .code
        }
    });
    assert_eq!(code, 2);
    fs::remove_dir_all(root).unwrap();
}

/// RED (0.4.7 uninstall fix) — **the door waits for the Folio that asked until that process has
/// let go of what it held, not until it has said its exit code.**
///
/// The asker is a real process — a copy of this test binary — that takes the data directory's
/// claim the way a Folio does (`persist::is_writer_of`) and leaves by Folio's own way out
/// (`leave_process`). It is held at its end (`trust_harness::stopped_at_exit`): its exit code
/// said, its claim not yet let go. There the door, with the real wait (`waited_for`) at a bound of
/// nothing and the real cleanup (`execute`, the kernel claim), answers that a Folio is running
/// **from its wait**, before any claim is asked — and nothing is touched. Once the asker has gone,
/// the same door completes and reaches the program's step.
///
/// This is the clean-VM rehearsal's failure (two of two in-app uninstalls): the door saw the
/// asker's exit code, took the asker for gone, and was refused the claim the asker still held —
/// "A Folio instance is running" with "Program files (per-copy): kept". No earlier test met it:
/// every door test handed the wait a seam or no asker, and none had a real process holding the
/// claim while it left.
///
/// MUTATION: in `bt_platform::install_flip`'s Windows arm, `creation_of` asks the exit code
/// instead of whether the process object is signalled; the door then answers from the claim, with
/// the program row "kept".
#[test]
#[cfg(windows)]
fn the_door_waits_until_the_asker_has_let_go_of_its_claim() {
    if let Ok(data) = std::env::var("BT_UNINSTALL_ASKER_CHILD") {
        // The asker: the data directory's claim, as a Folio takes it; its identity; Folio's way
        // out.
        assert!(crate::persist::is_writer_of(Path::new(&data)));
        let me = std::process::id();
        let started = bt_platform::install_flip::started_of(me).unwrap();
        fs::write(Path::new(&data).join("asker"), format!("{me} {started}")).unwrap();
        bt_platform::leave_process(0);
    }
    let (root, scope) = sandbox("uninstall-asker");
    fs::create_dir_all(&scope.data[0]).unwrap();
    let scope = std::sync::Arc::new(scope);
    let mut command = bt_platform::quiet_command(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "uninstall::tests::the_door_waits_until_the_asker_has_let_go_of_its_claim",
            "--nocapture",
        ])
        .env("BT_UNINSTALL_ASKER_CHILD", &scope.data[0])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let asker = || {
        let said = fs::read_to_string(scope.data[0].join("asker")).unwrap();
        let (pid, started) = said.split_once(' ').unwrap();
        Running {
            pid: pid.parse().unwrap(),
            started: started.parse().unwrap(),
        }
    };
    let door = |within: Duration| {
        let scope = scope.clone();
        let asker = asker();
        let reached = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let report = {
            let reached = reached.clone();
            on_a_worker(move |worker| {
                uninstall(
                    worker,
                    &scope,
                    Some(asker),
                    within,
                    |scope, program| super::execute(scope, false, system_absent, Some(program)),
                    |_| {
                        reached.store(true, std::sync::atomic::Ordering::SeqCst);
                        Vec::new()
                    },
                )
            })
        };
        (
            report.code,
            report.stdout(),
            reached.load(std::sync::atomic::Ordering::SeqCst),
        )
    };
    let (at_its_end, status) =
        bt_platform::trust_harness::stopped_at_exit(&mut command, |_, _| door(Duration::ZERO))
            .unwrap();
    assert_eq!(status.code(), Some(0));
    let running = Report::blocked(Why::Said(Text::CleanupRunning))
        .in_lang(scope.lang)
        .stdout();
    assert_eq!(at_its_end, (2, running, false));
    let (code, stdout, reached) = door(AFTER_PID_WITHIN);
    assert_eq!((code, reached), (0, true), "{stdout}");
    fs::remove_dir_all(&root).unwrap();
}

/// RED (T-UNINSTALL-UX) — **the door's lines are in the language `settings.json` names; with no
/// settings file, in the OS's; and a file that says `System` is the OS's too.**
///
/// Every line of a real cleanup over a seeded sandbox, rendered in the language the door decides
/// (`door_language`) with the OS language handed in: Chinese words where the settings say Chinese
/// on an English OS, English where they say English on a Chinese OS, and the OS's where there is
/// no file.
///
/// MUTATION: in `door_language`, answer `resolved_language_on(System, os)` without reading the
/// settings file, and the first case prints English.
#[test]
fn the_door_speaks_the_settings_language_and_the_os_language_without_one() {
    let lines = |language: Option<bt_persist::LanguageV1>, os: &str| {
        let (root, scope) = sandbox("uninstall-language");
        seed(&scope, &scope.exe);
        if let Some(language) = language {
            settings_speaking(&scope, language);
        }
        let lang = door_language(&scope, os);
        let stdout = execute(&scope.speaking(lang), false, system_absent).stdout();
        fs::remove_dir_all(root).unwrap();
        (lang, stdout)
    };
    let (lang, chinese) = lines(Some(bt_persist::LanguageV1::Chinese), "en-US");
    assert_eq!(lang, Lang::Chinese);
    assert!(
        chinese.contains(&format!(
            "{} ({}): ",
            Text::CleanupMarkClaude.in_lang(Lang::Chinese),
            Text::CleanupKindPerCopy.in_lang(Lang::Chinese)
        )),
        "{chinese}"
    );
    assert!(
        chinese.contains(Text::CleanupRemoved.in_lang(Lang::Chinese)),
        "{chinese}"
    );
    assert!(
        !chinese.contains(": removed\n") && !chinese.contains("not present"),
        "{chinese}"
    );

    assert_eq!(
        lines(Some(bt_persist::LanguageV1::English), "zh-CN").0,
        Lang::English
    );
    assert_eq!(
        lines(Some(bt_persist::LanguageV1::System), "zh-CN").0,
        Lang::Chinese
    );
    let (lang, os_chinese) = lines(None, "zh-CN");
    assert_eq!(lang, Lang::Chinese);
    assert!(os_chinese.contains(Text::CleanupRemoved.in_lang(Lang::Chinese)));
    assert_eq!(lines(None, "en-US").0, Lang::English);
}

/// PIN (T-UNINSTALL-UX) — **`--uninstall-cleanup` still prints what it always printed: on each
/// platform, every mark the door prints there has its name as its English, and every kind's
/// English is the word the door used to write.**
///
/// The package managers' hooks read this transcript; the language arrived for `--uninstall`
/// only. Per platform, because a row's English may differ by platform (`pick_platform`): "Local
/// data (including WebView2)" is a Windows data root's row and is printed nowhere else, and the
/// same text on macOS says "Local data". A data mark is checked where it is a data root
/// (`Remover::data_root_on`); every other mark on every platform.
///
/// MUTATIONS: change a mark's `says` to a row whose English differs from its `name`; or make
/// `data_root_on` answer for every platform — the Windows-only Local data row is then checked on
/// macOS, where its English is "Local data".
#[test]
fn the_cleanup_verb_prints_the_same_english_as_before() {
    for platform in [
        HostPlatform::Windows,
        HostPlatform::MacOs,
        HostPlatform::OtherUnix,
    ] {
        for mark in INVENTORY {
            if matches!(mark.remover, Remover::Data(..))
                && mark.remover.data_root_on(platform).is_none()
            {
                continue;
            }
            assert_eq!(
                mark.says.on(Lang::English, platform),
                mark.name,
                "{platform:?}: {}",
                mark.name
            );
        }
    }
    for (text, word) in [
        (Text::CleanupKindPerCopy, "per-copy"),
        (Text::CleanupKindPerAccount, "per-account"),
        (Text::CleanupKindData, "data"),
    ] {
        assert_eq!(english(text), word);
    }
    assert_eq!(
        english(Text::CleanupMarkExplorerPackage),
        "Explorer sparse package"
    );
    assert_eq!(
        english(Text::CleanupMarkExplorerClassic),
        "Explorer classic verbs"
    );
}

/// RED (T-UNINSTALL-UX) — **the words Folio's way out starts the door with are the door's own
/// grammar: `--uninstall`, `--remove-data` when asked, and `--after-pid` naming the process.**
///
/// MUTATION: in `door_words`, spell `--after-pid` as `--after`, and the door answers the usage
/// line.
#[test]
fn the_way_out_starts_the_door_with_its_own_grammar() {
    use crate::cli::UninstallDoor::Uninstall;
    for remove_data in [false, true] {
        assert_eq!(
            crate::cli::uninstall_cleanup(door_words(remove_data, 4242)),
            Some(Ok(Uninstall {
                remove_data,
                after: Some(4242)
            }))
        );
    }
    let parse = |a: &[&str]| crate::cli::uninstall_cleanup(a.iter().map(std::ffi::OsString::from));
    assert_eq!(
        parse(&["--uninstall"]),
        Some(Ok(Uninstall {
            remove_data: false,
            after: None
        }))
    );
    for refused in [
        &["--uninstall", "--purge"][..],
        &["--uninstall-cleanup", "--remove-data"],
        &["--uninstall", "--after-pid"],
        &["--uninstall", "--after-pid", "x1"],
        &["--uninstall", "--after-pid", "1", "--after-pid", "2"],
        &["--remove-data"],
        &["--uninstall", "--uninstall"],
        &["--uninstall", "--cwd", "x"],
    ] {
        assert!(parse(refused).unwrap().is_err(), "{refused:?}");
    }
}

/// RED (T-UNINSTALL-UX round 2, mutation `drop_explicit_wait_identity`) —
/// **the door hands every explicit pid/start identity to the native remover,
/// while the data root stays.** Completion and destructive-boundary behavior
/// are exercised by `bt-platform::deferred_removal` without a timed child.
#[test]
fn the_program_plan_hands_the_process_identity_to_the_native_remover_and_keeps_data() {
    let (root, scope) = sandbox("uninstall-end-to-end");
    seed(&scope, &scope.exe);
    seed_program(&scope);
    let waited = Running {
        pid: 4242,
        started: 101,
    };
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(None));
    let scope = std::sync::Arc::new(scope);
    let lines = {
        let scope = scope.clone();
        let recorded = recorded.clone();
        on_a_worker(move |worker| {
            remove_the_program(
                worker,
                &scope,
                crate::install_channel::Channel::Ours,
                HostPlatform::Windows,
                members,
                &[waited],
                |_, removal, _| {
                    *recorded.lock().unwrap() = Some(removal.clone());
                    Ok(())
                },
            )
            .into_iter()
            .map(|entry| entry.line(Lang::English))
            .collect::<String>()
        })
    };
    assert!(lines.contains("removed when this window closes"), "{lines}");
    let removal = recorded.lock().unwrap().clone().expect("a removal plan");
    assert_eq!(
        removal
            .after
            .iter()
            .map(|wait| wait.process)
            .collect::<Vec<_>>(),
        [waited]
    );
    let program = resolved(&scope.exe);
    assert_eq!(removal.program, program);
    assert_eq!(
        removal.program_identity,
        bt_platform::deferred_removal::FileIdentity::of(&program).unwrap()
    );
    assert!(scope.data[0].exists(), "the data root stays");
    fs::remove_dir_all(root).unwrap();
}

/// RED (T-UNINSTALL-UX round 2, mutation `skip_install_image_census`) — every
/// live image of the installed `folio.exe`, not merely the explicit asker and
/// parent, is handed to the native remover by exact image identity.
#[test]
fn every_running_installed_image_is_in_the_removers_wait_census() {
    use std::io::{BufRead, Write};
    use std::process::Stdio;

    if std::env::var_os("FOLIO_TEST_UNINSTALL_RUNNING_HELPER").is_some() {
        writeln!(std::io::stdout(), "ready").unwrap();
        std::io::stdout().flush().unwrap();
        let mut line = String::new();
        let _ = std::io::BufReader::new(std::io::stdin()).read_line(&mut line);
        return;
    }

    let (root, scope) = sandbox("uninstall-image-census");
    fs::remove_file(&scope.exe).unwrap();
    fs::copy(std::env::current_exe().unwrap(), &scope.exe).unwrap();
    seed_program(&scope);
    let mut child = bt_platform::quiet_command(&scope.exe);
    child
        .args([
            "--exact",
            "uninstall::tests::every_running_installed_image_is_in_the_removers_wait_census",
            "--nocapture",
        ])
        .env("FOLIO_TEST_UNINSTALL_RUNNING_HELPER", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = child.spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let reader = bt_platform::spawn_at_priority(
        "bt-uninstall-running-helper-ready",
        bt_platform::ThreadPriority::BelowNormal,
        move |_| {
            let mut reader = std::io::BufReader::new(stdout);
            let mut ready_tx = Some(ready_tx);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        if let Some(ready_tx) = ready_tx.take() {
                            let _ = ready_tx.send(Err(std::io::Error::new(
                                std::io::ErrorKind::UnexpectedEof,
                                "the installed image ended before readiness",
                            )));
                        }
                        break;
                    }
                    Ok(_) if line.trim_end() == "ready" => {
                        if let Some(ready_tx) = ready_tx.take() {
                            let _ = ready_tx.send(Ok(()));
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        if let Some(ready_tx) = ready_tx.take() {
                            let _ = ready_tx.send(Err(error));
                        }
                        break;
                    }
                }
            }
        },
    )
    .unwrap();
    ready_rx
        .recv()
        .expect("the installed image announces readiness through the controlled receiver")
        .unwrap();
    let running = Running {
        pid: child.id(),
        started: bt_platform::install_flip::started_of(child.id()).unwrap(),
    };

    let recorded = std::sync::Arc::new(std::sync::Mutex::new(None));
    let scope = std::sync::Arc::new(scope);
    {
        let scope = scope.clone();
        let recorded = recorded.clone();
        on_a_worker(move |worker| {
            remove_the_program(
                worker,
                &scope,
                crate::install_channel::Channel::Ours,
                HostPlatform::Windows,
                members,
                &[],
                |_, removal, _| {
                    *recorded.lock().unwrap() = Some(removal.clone());
                    Ok(())
                },
            )
        });
    }
    let removal = recorded.lock().unwrap().clone().expect("a removal plan");
    assert!(removal.after.iter().any(|wait| wait.process == running));
    drop(child.stdin.take());
    child.wait().unwrap();
    reader.join().unwrap();
    fs::remove_dir_all(root).unwrap();
}

/// RED (T-UNINSTALL-SELFHOLD) — **the door Folio's way out starts holds nothing of the Folio
/// that asked, so a `--remove-data` run removes the folder that held that Folio's log.**
///
/// Three real processes, all copies of this test binary. The asker does what a resident Folio
/// does with its streams: `make_standard_streams_uninheritable` (the first line of `main`), then
/// `redirect_std_streams_to_file` onto `diagnostics.log` in the sandbox's data folder (a resident
/// run's log channel). It is started once with no streams (a Folio started by a person) and once
/// with that same log as its streams (a Folio started by a Folio — an update's trial). It records
/// its identity, starts the door through `door_command` (the way out's own builder) and leaves
/// without waiting, as `leave_armed` does. The door says one line on each stream, waits for the
/// asker's end with the real `waited_for` (`--after-pid`), and runs the real `uninstall` over the
/// sandbox with the door's cleanup (`execute`: the claims, the purge's preflight with the real
/// `cleanup::probe_file`, the purge). Asked to keep the data, the log holds none of the door's
/// words; asked to remove it, the door answers 0 and the data folder, log and all, is gone.
///
/// This is the clean-VM finding of 2026-10-07: every in-app uninstall with "Also remove settings
/// and data" refused with "A process holds Folio data … (diagnostics.log)", and the holder was the
/// door itself, holding the duplicates of the asker's streams it was started with.
///
/// MUTATIONS, each seen red on Windows: in `bt_platform::quiet_breakaway_command`, leave out the
/// three null streams — the door's line lands in the asker's log (on every platform), and the
/// remove-data runs answer 2 naming `diagnostics.log`; make
/// `make_standard_streams_uninheritable` do nothing — the handed asker's remove-data run answers 2
/// the same way; in `uninstall`, open the data folder's `diagnostics.log` for appending before
/// the cleanup and hold it — every remove-data run answers 2.
#[test]
fn the_door_holds_nothing_of_its_askers_so_remove_data_removes_its_log() {
    const ROLE: &str = "BT_UNINSTALL_SELFHOLD_CHILD";
    const NAME: &str =
        "uninstall::tests::the_door_holds_nothing_of_its_askers_so_remove_data_removes_its_log";
    const SAID: &str = "door line 卸载 seen";
    let libtest = |command: &mut std::process::Command| {
        command.args(["--exact", NAME, "--nocapture"]);
    };
    let identity = |root: &Path, name: &str| {
        let said = fs::read_to_string(root.join(name)).unwrap();
        let (pid, started) = said.split_once(' ').unwrap();
        Running {
            pid: pid.parse().unwrap(),
            started: started.parse().unwrap(),
        }
    };
    if let Some(role) = std::env::var_os(ROLE) {
        let role = role.into_string().unwrap();
        let (part, rest) = role.split_once('|').unwrap();
        let (remove, root) = rest.split_once('|').unwrap();
        let (remove, root) = (remove == "remove", PathBuf::from(root));
        let scope = Scope::sandbox(&root, root.join("app/folio.exe")).unwrap();
        if part == "asker" {
            bt_platform::make_standard_streams_uninheritable();
            assert!(bt_platform::redirect_std_streams_to_file(
                &scope.data[0].join(crate::diagnostics::LOG_FILENAME)
            ));
            let me = std::process::id();
            let started = bt_platform::install_flip::started_of(me).unwrap();
            fs::write(root.join("asker"), format!("{me} {started}")).unwrap();
            let mut door = door_command(std::env::current_exe().unwrap());
            libtest(&mut door);
            // A test harness may hold this process in a job that allows no breakaway, so the
            // door stays in the harness's job: `CREATE_NO_WINDOW` alone. Only the flags change;
            // the streams are the builder's.
            #[cfg(windows)]
            std::os::windows::process::CommandExt::creation_flags(&mut door, 0x0800_0000);
            let door = door.env(ROLE, format!("door|{rest}")).spawn().unwrap();
            let started = bt_platform::install_flip::started_of(door.id()).unwrap();
            fs::write(root.join("door"), format!("{} {started}", door.id())).unwrap();
            std::process::exit(0);
        }
        println!("{SAID}");
        eprintln!("{SAID}");
        let asker = identity(&root, "asker");
        let scope = std::sync::Arc::new(scope);
        let (code, stdout) = on_a_worker(move |worker| {
            let report = uninstall(
                worker,
                &scope,
                Some(asker),
                AFTER_PID_WITHIN,
                |scope, program| execute_then(scope, remove, system_absent, Some(program)),
                |_| Vec::new(),
            );
            (report.code, report.stdout())
        });
        fs::write(root.join("door-said"), format!("{code}\n{stdout}")).unwrap();
        std::process::exit(0);
    }
    for handed in [false, true] {
        for remove in [true, false] {
            let (root, scope) = sandbox("uninstall-selfhold");
            let data = scope.data[0].clone();
            settings_speaking(&scope, bt_persist::LanguageV1::English);
            let log = data.join(crate::diagnostics::LOG_FILENAME);
            let stream = || -> std::process::Stdio {
                if handed {
                    fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&log)
                        .unwrap()
                        .into()
                } else {
                    std::process::Stdio::null()
                }
            };
            // The command owns the log's handles until it is dropped, so it lives only for the
            // asker's run: past it, the only holders a probe can meet are the asker's and the
            // door's.
            let status = {
                let mut asker = bt_platform::quiet_command(std::env::current_exe().unwrap());
                libtest(&mut asker);
                let words = if remove { "remove" } else { "keep" };
                asker
                    .env(ROLE, format!("asker|{words}|{}", root.display()))
                    .stdin(std::process::Stdio::null())
                    .stdout(stream())
                    .stderr(stream())
                    .status()
                    .unwrap()
            };
            let case = format!("handed its streams: {handed}, remove data: {remove}");
            assert_eq!(status.code(), Some(0), "{case}");
            let door = identity(&root, "door");
            assert!(
                on_a_worker(move |worker| waited_for(worker, door, AFTER_PID_WITHIN)),
                "{case}"
            );
            let said = fs::read_to_string(root.join("door-said")).unwrap();
            let (code, report) = said.split_once('\n').unwrap();
            assert_eq!(code, "0", "{case}\n{report}");
            if remove {
                assert!(
                    !data.exists(),
                    "{case}: the data folder and its log are gone"
                );
            } else {
                let kept = fs::read_to_string(&log).unwrap();
                assert!(
                    !kept.contains(SAID),
                    "{case}: the door wrote into its asker's log:\n{kept}"
                );
                assert!(data.join(crate::persist::SETTINGS_FILE_NAME).exists());
            }
            fs::remove_dir_all(&root).unwrap();
        }
    }
}

/// Whether `path` is `root` or below it, compared by component and, as Windows and macOS
/// compare names, without case.
fn within(path: &Path, root: &Path) -> bool {
    let folded = |p: &Path| -> Vec<String> {
        p.components()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect()
    };
    folded(path).starts_with(&folded(root))
}

/// RED (T-UNINSTALL-SELFHOLD, clean-VM row N9) — **the native remover's per-user folder is
/// inside no purge root, on any platform**: `--remove-data` purges the data folders after the
/// program's step has started the remover from its private folder there.
///
/// Every platform's scope, resolved the way production resolves it (no sandbox; nothing is
/// purged here).
///
/// MUTATION: in `Scope::resolve`, put the Windows arm of `remover_home` back at
/// `%LOCALAPPDATA%\Folio` — inside "Local data (including WebView2)"; or the macOS arm at
/// `~/Library/Application Support/Folio` — the data folder itself.
#[test]
fn the_removers_folder_is_inside_no_purge_root() {
    let (root, _) = sandbox("remover-home");
    for platform in [
        HostPlatform::Windows,
        HostPlatform::MacOs,
        HostPlatform::OtherUnix,
    ] {
        let mapped = root.clone();
        let scope = Scope::resolve(
            root.join("app/folio.exe"),
            platform,
            move |name| {
                Some(
                    mapped
                        .join(match name {
                            "APPDATA" => "roaming",
                            "LOCALAPPDATA" => "local",
                            "HOME" | "USERPROFILE" => "home",
                            "XDG_DATA_HOME" => "xdg",
                            _ => return None,
                        })
                        .into_os_string(),
                )
            },
            root.join("temp"),
            false,
        )
        .unwrap();
        for (mark, purged) in &scope.purge_roots {
            assert!(
                !within(&scope.remover_home, purged),
                "{platform:?}: the remover's folder {} is inside the purge root `{}` ({})",
                scope.remover_home.display(),
                mark.name,
                purged.display()
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

/// RED (T-UNINSTALL-SELFHOLD, clean-VM row N9) — **a remove-data run purges every data folder
/// around the remover it has just started, and says it completed.**
///
/// `uninstall.cmd` answered `n` printed "Local data (including WebView2) (data): …\Local\Folio:
/// refused (Access is denied. (os error 5))" and skipped "Removal continues after this window
/// closes." — the folder it could not remove was the remover's own, started by the program's step
/// just before the purge, its image in use.
///
/// The real door over a seeded sandbox — `uninstall`, `execute` with the purge, the real
/// `remove_the_program` — with the hand-over doing to the disk what `deferred_removal::schedule`
/// does: a private folder below the remover's home and a copy of the program in it, held open
/// as a running image holds its file (no delete sharing; Windows). The run answers 0 with no row
/// refused, every data folder is gone, and the remover's copy is still there for it to run from.
///
/// MUTATION: in `Scope::resolve`, put `remover_home` back inside the data folders
/// (`%LOCALAPPDATA%\Folio` on Windows, `~/Library/Application Support/Folio` on macOS): on
/// Windows the Local data row is refused with "Access is denied" and the run answers 1; on macOS
/// the purge deletes the remover's copy.
#[test]
fn a_remove_data_run_purges_around_the_remover_it_started() {
    let (root, scope) = sandbox("purge-around-remover");
    seed(&scope, &scope.exe);
    settings_speaking(&scope, bt_persist::LanguageV1::English);
    for (_, purged) in &scope.purge_roots {
        if purged.extension().is_none() {
            fs::create_dir_all(purged).unwrap();
            fs::write(purged.join("held by nobody"), b"data").unwrap();
        }
    }
    seed_program(&scope);
    let scope = std::sync::Arc::new(scope);
    let staged = std::sync::Arc::new(std::sync::Mutex::new(None));
    let (code, stdout) = {
        let (scope, staged) = (scope.clone(), staged.clone());
        on_a_worker(move |worker| {
            let report = uninstall(
                worker,
                &scope,
                None,
                AFTER_PID_WITHIN,
                |scope, program| execute_then(scope, true, system_absent, Some(program)),
                |scope| {
                    remove_the_program(
                        worker,
                        scope,
                        crate::install_channel::Channel::Ours,
                        bt_platform::host_platform(),
                        members,
                        &[],
                        |_, removal, home| {
                            let private = home.join("uninstall-0123456789abcdef");
                            fs::create_dir_all(&private)?;
                            let copy = private.join(removal.program.file_name().unwrap());
                            fs::copy(&removal.program, &copy)?;
                            let held =
                                bt_platform::trust_harness::hold_without_delete_sharing(&copy).ok();
                            *staged.lock().unwrap() = Some((copy, held));
                            Ok(())
                        },
                    )
                },
            );
            (report.code, report.stdout())
        })
    };
    assert_eq!(code, 0, "{stdout}");
    assert!(!stdout.contains("refused"), "{stdout}");
    for (mark, purged) in &scope.purge_roots {
        assert!(!purged.exists(), "{} stayed:\n{stdout}", mark.name);
    }
    let (copy, held) = staged
        .lock()
        .unwrap()
        .take()
        .expect("the remover was handed over");
    assert!(
        copy.exists(),
        "the purge took the remover's copy:\n{stdout}"
    );
    drop(held);
    fs::remove_dir_all(root).unwrap();
}

/// RED (T-UNINSTALL-SELFHOLD round 3) — **a remover's folder left behind is taken by the next
/// purge, and the zap list names it.**
///
/// A remover ended before it could retire leaves its private folder and copy in the per-user
/// folder (`REMOVER_HOME`). A later `--uninstall-cleanup --purge` — scoop's, winget's, or a
/// person's — takes it with the data folders and says so on its own row. Homebrew's
/// `brew uninstall --zap` does not run the purge, so the cask's own `trash` list names the same
/// folder, as the code resolves it on macOS.
///
/// MUTATIONS: in `execute`, skip the remover's folder on every run (not only the run that started
/// a remover) — the stale copy stays and its row is missing; remove
/// `~/Library/Application Support/Folio-uninstall` from `packaging/homebrew/folio.rb`'s `trash`.
#[test]
fn a_purge_takes_a_removers_folder_left_behind() {
    let (root, scope) = sandbox("stale-remover");
    seed(&scope, &scope.exe);
    let stale = scope
        .remover_home
        .join("uninstall-00000000000000000000000000000000");
    fs::create_dir_all(&stale).unwrap();
    fs::write(stale.join("folio.exe"), b"a remover that never retired").unwrap();
    let report = execute(&scope, true, system_absent);
    let stdout = report.stdout();
    assert_eq!(report.code, 0, "{stdout}");
    assert!(!scope.remover_home.exists(), "{stdout}");
    assert!(
        stdout.contains(&format!(
            "Uninstaller staging (data): {}: removed",
            scope.remover_home.display()
        )),
        "{stdout}"
    );
    fs::remove_dir_all(root).unwrap();

    let cask = include_str!("../../../packaging/homebrew/folio.rb");
    let zap = &cask[cask.find("zap script:").expect("a zap script")..];
    let trash = &zap[zap.find("trash:").expect("a trash list")..];
    let trash = &trash[..trash.find(']').unwrap()];
    // The cask spells the account's home `~`; the scope is resolved under a home of its own and
    // read back below it.
    let (home, _) = sandbox("zap-list");
    let mapped = home.clone();
    let mac = Scope::resolve(
        home.join("Folio.app/Contents/MacOS/folio"),
        HostPlatform::MacOs,
        move |name| (name == "HOME").then(|| mapped.clone().into_os_string()),
        home.join("temp"),
        true,
    )
    .unwrap();
    for folder in [&mac.data[0], &mac.remover_home] {
        let below: Vec<_> = folder
            .strip_prefix(&home)
            .unwrap()
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        let spelled = format!("~/{}", below.join("/"));
        assert!(
            trash.contains(&format!("\"{spelled}\"")),
            "the zap does not trash {spelled}:\n{trash}"
        );
    }
    fs::remove_dir_all(home).unwrap();
}
