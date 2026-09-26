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

fn sandbox(tag: &str) -> (PathBuf, Scope) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root =
        std::env::temp_dir().join(format!("folio-uninstall-{tag}-{}-{n}", std::process::id()));
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
    let mapped = root.clone();
    let scope = Scope::resolve(
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
        root.join("temp"),
        false,
    )
    .unwrap();
    assert!(scope.sandbox.is_none());
    (root, scope)
}

fn system_absent(_: Remover) -> Vec<Entry> {
    vec![Entry::new("injected registration", Fate::Absent)]
}

// Windows claims are kernel objects keyed only to the injected path. Unix's default
// claim also creates a runtime file outside the sandbox, so these tests inject the gate.
fn execute(scope: &Scope, purge: bool, system: impl FnMut(Remover) -> Vec<Entry>) -> Report {
    #[cfg(windows)]
    {
        super::execute(scope, purge, system)
    }
    #[cfg(not(windows))]
    {
        super::execute_with_claim(scope, purge, system, |_| Some(()))
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
        .map(Entry::line)
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
    for (name, path) in &scope.purge_roots {
        if !path.exists() {
            if matches!(*name, "Panic log" | "Preferences") {
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
    // Native PowerShell creates a junction without developer-mode symlink privileges.
    let status = bt_platform::quiet_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "New-Item -ItemType Junction -Path $env:FOLIO_TEST_JUNCTION -Target $env:FOLIO_TEST_TARGET -ErrorAction Stop | Out-Null"])
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
    let parse = |a: &[&str]| crate::cli::uninstall_cleanup(a.iter().map(std::ffi::OsString::from));
    assert_eq!(parse(&["--uninstall-cleanup"]), Some(Ok(false)));
    assert_eq!(parse(&["--uninstall-cleanup", "--purge"]), Some(Ok(true)));
    assert_eq!(parse(&["--purge", "--uninstall-cleanup"]), Some(Ok(true)));
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
    scope
        .purge_roots
        .push(("invalid application root", root.clone()));
    let report = execute(&scope, true, system_absent);
    assert_eq!(report.code, 1);
    assert_eq!(fs::read(&scope.exe).unwrap(), b"fixture executable");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn uninstall_door_precedes_every_startup_effect() {
    let source = include_str!("main.rs");
    let main = source.split_once("\nfn main() -> Result<()> {").unwrap().1;
    let door = main.find("cli::uninstall_cleanup(").unwrap();
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
                Base::Home,
                "Library/Application Support/Folio",
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
    assert_eq!(INVENTORY.len(), 27);
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
        logon_hook::arm_in(&mut memory, "run", &[txn; 16], &program).unwrap();
    }
    memory
        .set("run", "SomeoneElse", REG_SZ, b"x\0\0\0")
        .unwrap();

    let entries = entrance_entries(HostPlatform::Windows, &scope.exe, |home| {
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

    let nothing = entrance_entries(HostPlatform::Windows, &scope.exe, |home| {
        logon_hook::clean_in(&mut Memory::default(), "run", home)
    });
    assert_eq!(nothing.len(), 1);
    assert_eq!(nothing[0].fate, Fate::Absent);
    let elsewhere = entrance_entries(HostPlatform::MacOs, &scope.exe, |_| {
        panic!("no entrance off Windows")
    });
    assert_eq!(elsewhere[0].fate, Fate::Absent);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn uninstall_archive_has_ten_files_and_cleanup_only_wrapper() {
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
    assert!(wrapper.contains("\"%~dp0folio.exe\" --uninstall-cleanup"));
    assert!(wrapper.contains("pause"));
    for text in [
        Text::CleanupArchiveExit,
        Text::CleanupArchiveReady,
        Text::CleanupArchiveIncomplete,
    ] {
        assert!(wrapper.contains(english(text)));
    }
    for forbidden in ["--purge", "rmdir", " del ", "Remove-Item"] {
        assert!(!wrapper.contains(forbidden));
    }
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
#[test]
fn uninstall_purge_reports_the_root_it_found_at_the_deciding_check() {
    let (root, scope) = sandbox("recreated");
    let (name, late) = scope
        .purge_roots
        .iter()
        .find_map(|(name, path)| {
            (*name == "Local data (including WebView2)").then(|| (*name, path.clone()))
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

/// macOS cannot be asked whether a file is held, so a purge there says so instead of
/// implying a check that was never made.
#[test]
fn uninstall_purge_says_on_macos_that_held_data_cannot_be_told() {
    let notice = english(Text::CleanupMacHeld);
    assert_eq!(purge_notices(HostPlatform::MacOs, true), [notice]);
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
        .map(Entry::line)
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
