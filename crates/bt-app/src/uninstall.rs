//! One non-interactive cleanup door. Ownership stays with each writer.
//! The inventory is executable data; historical locations are discovery, never ownership.
use crate::{
    attention_ownership::{Decision, Outcome},
    i18n::{Lang, Text},
    shell_integration::profile_marks::Marks,
};
use bt_platform::HostPlatform;
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    PerCopy,
    PerAccount,
    Data,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Base {
    Roaming,
    Local,
    Home,
    Temp,
    Xdg,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Remover {
    Profiles,
    PsReadLine,
    Agent(usize),
    Explorer,
    Toast,
    Absent,
    RecoverySnapshots,
    RuntimeClaims,
    Data(HostPlatform, Base, &'static str),
}
struct Mark {
    name: &'static str,
    kind: Kind,
    remover: Remover,
    writer: &'static str,
}
// No sound syntax-only test can infer the destination of arbitrary Path arguments or OS
// framework writes. The source guard pins these named owners and the archive manifest.
const INVENTORY: &[Mark] = &[
    Mark {
        name: "PowerShell profiles",
        kind: Kind::PerAccount,
        remover: Remover::Profiles,
        writer: "shell_integration.rs:add_to_profile",
    },
    Mark {
        name: "PSReadLine module",
        kind: Kind::PerAccount,
        remover: Remover::PsReadLine,
        writer: "psreadline.rs:install_recorded",
    },
    Mark {
        name: "Claude Code hooks",
        kind: Kind::PerCopy,
        remover: Remover::Agent(0),
        writer: "attention_hooks.rs:apply_at",
    },
    Mark {
        name: "Codex notify",
        kind: Kind::PerCopy,
        remover: Remover::Agent(1),
        writer: "attention_codex.rs:apply_at",
    },
    Mark {
        name: "Copilot hooks",
        kind: Kind::PerCopy,
        remover: Remover::Agent(2),
        writer: "attention_copilot.rs:apply_at",
    },
    Mark {
        name: "Explorer registrations",
        kind: Kind::PerCopy,
        remover: Remover::Explorer,
        writer: "context_menu.rs:apply;explorer_menu.rs:request",
    },
    Mark {
        name: "Toast identity",
        kind: Kind::PerAccount,
        remover: Remover::Toast,
        writer: "../bt-platform/src/lib.rs:Notifier::new",
    },
    Mark {
        name: "Start-menu shortcut",
        kind: Kind::PerAccount,
        remover: Remover::Absent,
        writer: "none (no shortcut writer)",
    },
    Mark {
        name: "Autostart / login item",
        kind: Kind::PerAccount,
        remover: Remover::Absent,
        writer: "none (no persistent writer)",
    },
    Mark {
        name: "Quake / global hotkeys",
        kind: Kind::PerAccount,
        remover: Remover::Absent,
        writer: "../bt-platform/src/hotkey.rs:register (process lifetime)",
    },
    Mark {
        name: "Roaming data",
        kind: Kind::Data,
        remover: Remover::Data(HostPlatform::Windows, Base::Roaming, "Folio"),
        writer: "persist.rs:storage_location",
    },
    Mark {
        name: "Legacy data",
        kind: Kind::Data,
        remover: Remover::Data(HostPlatform::Windows, Base::Roaming, "BetterTerminal"),
        writer: "persist.rs:storage_location",
    },
    Mark {
        name: "Local data (including WebView2)",
        kind: Kind::Data,
        remover: Remover::Data(HostPlatform::Windows, Base::Local, "Folio"),
        writer: "webhost.rs:user_data_folder_in",
    },
    Mark {
        name: "Application Support",
        kind: Kind::Data,
        remover: Remover::Data(
            HostPlatform::MacOs,
            Base::Home,
            "Library/Application Support/Folio",
        ),
        writer: "persist.rs:storage_location;webhost.rs:web_engine_folder",
    },
    Mark {
        name: "WebKit",
        kind: Kind::Data,
        remover: Remover::Data(
            HostPlatform::MacOs,
            Base::Home,
            "Library/WebKit/io.github.lulu-loopp.folio",
        ),
        writer: "../bt-platform/src/macos_webview.rs (WebKit, bundle identity)",
    },
    Mark {
        name: "Caches",
        kind: Kind::Data,
        remover: Remover::Data(
            HostPlatform::MacOs,
            Base::Home,
            "Library/Caches/io.github.lulu-loopp.folio",
        ),
        writer: "../bt-platform/src/macos_webview.rs (WebKit, bundle identity)",
    },
    Mark {
        name: "HTTPStorages",
        kind: Kind::Data,
        remover: Remover::Data(
            HostPlatform::MacOs,
            Base::Home,
            "Library/HTTPStorages/io.github.lulu-loopp.folio",
        ),
        writer: "../bt-platform/src/macos_webview.rs (WebKit, bundle identity)",
    },
    Mark {
        name: "Preferences",
        kind: Kind::Data,
        remover: Remover::Data(
            HostPlatform::MacOs,
            Base::Home,
            "Library/Preferences/io.github.lulu-loopp.folio.plist",
        ),
        writer: "../bt-platform/src/macos_app.rs (AppKit, bundle identity)",
    },
    Mark {
        name: "Saved Application State",
        kind: Kind::Data,
        remover: Remover::Data(
            HostPlatform::MacOs,
            Base::Home,
            "Library/Saved Application State/io.github.lulu-loopp.folio.savedState",
        ),
        writer: "../bt-platform/src/macos_app.rs (AppKit, bundle identity)",
    },
    Mark {
        name: "Unix data",
        kind: Kind::Data,
        remover: Remover::Data(HostPlatform::OtherUnix, Base::Xdg, "Folio"),
        writer: "persist.rs:storage_location",
    },
    Mark {
        name: "User configuration recovery copies",
        kind: Kind::Data,
        remover: Remover::RecoverySnapshots,
        writer: "shell_integration.rs:replace_profile;attention_hooks.rs:land",
    },
    Mark {
        name: "Unix runtime claims",
        kind: Kind::Data,
        remover: Remover::RuntimeClaims,
        writer: "../bt-platform/src/instance.rs:claim_data_directory",
    },
    // Temp rows apply on every platform (OtherUnix is the all-platform sentinel for Base::Temp).
    Mark {
        name: "Clipboard staging",
        kind: Kind::Data,
        remover: Remover::Data(HostPlatform::OtherUnix, Base::Temp, "folio/clipboard"),
        writer: "clipboard_picture.rs:directory;clipboard_picture.rs:save",
    },
    Mark {
        name: "Panic log",
        kind: Kind::Data,
        remover: Remover::Data(HostPlatform::OtherUnix, Base::Temp, "folio-panic.log"),
        writer: "main.rs:install_panic_log_hook",
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Fate {
    Removed,
    Absent,
    Left(Vec<PathBuf>),
    Refused(String),
    Kept(&'static str),
}
struct Entry {
    mark: String,
    fate: Fate,
}
impl Entry {
    fn new(mark: impl Into<String>, fate: Fate) -> Self {
        Self {
            mark: mark.into(),
            fate,
        }
    }
    fn line(&self) -> String {
        let detail = match &self.fate {
            Fate::Removed => english(Text::CleanupRemoved).to_owned(),
            Fate::Absent => english(Text::CleanupAbsent).to_owned(),
            Fate::Left(paths) => format!(
                "{} {}",
                english(Text::CleanupLeft),
                paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Fate::Kept(reason) => (*reason).to_owned(),
            Fate::Refused(reason) => format!(
                "{} ({})",
                english(Text::CleanupRefused),
                reason.replace(['\n', '\r'], " ")
            ),
        };
        format!("{}: {detail}\n", self.mark.replace(['\n', '\r'], " "))
    }
}
fn english(text: Text) -> &'static str {
    text.in_lang(Lang::English)
}
struct Report {
    code: i32,
    entries: Vec<Entry>,
}
impl Report {
    fn new(entries: Vec<Entry>) -> Self {
        Self {
            code: i32::from(entries.iter().any(|e| matches!(e.fate, Fate::Refused(_)))),
            entries,
        }
    }
    fn blocked(reason: &str) -> Self {
        Self {
            code: 2,
            entries: vec![Entry::new("Folio", Fate::Refused(reason.to_owned()))],
        }
    }
    fn stdout(&self) -> String {
        self.entries.iter().map(Entry::line).collect()
    }
    fn stderr(&self) -> String {
        self.entries
            .iter()
            .filter(|e| matches!(e.fate, Fate::Refused(_)))
            .map(Entry::line)
            .collect()
    }
}

struct Scope {
    exe: PathBuf,
    data: Vec<PathBuf>,
    profiles: Option<Vec<PathBuf>>,
    documents: Vec<PathBuf>,
    agents: [Vec<PathBuf>; 3],
    purge_roots: Vec<(&'static str, PathBuf)>,
    sandbox: Option<PathBuf>,
}
impl Scope {
    fn sandbox(root: &Path, exe: PathBuf) -> io::Result<Self> {
        if !root.is_absolute() || has_parent_component(root) {
            return Err(io::Error::other(english(Text::CleanupRoot)));
        }
        let mut scope = Self::resolve(
            exe,
            bt_platform::host_platform(),
            |name| {
                Some(
                    root.join(match name {
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
            true,
        )?;
        scope.sandbox = Some(root.to_owned());
        Ok(scope)
    }
    fn resolve(
        exe: PathBuf,
        platform: HostPlatform,
        env: impl Fn(&str) -> Option<OsString>,
        temp: PathBuf,
        sandbox: bool,
    ) -> io::Result<Self> {
        let named = |name: &str| -> io::Result<PathBuf> {
            let p = env(name).map(PathBuf::from).ok_or_else(|| {
                io::Error::other(format!("{}: {name}", english(Text::CleanupRoot)))
            })?;
            if !p.is_absolute() {
                return Err(io::Error::other(format!(
                    "{}: {name}",
                    english(Text::CleanupRoot)
                )));
            }
            Ok(p)
        };
        let mut purge_roots = Vec::new();
        for mark in INVENTORY {
            debug_assert!(!mark.writer.is_empty());
            if let Remover::Data(host, base, relative) = mark.remover {
                if host != platform && base != Base::Temp {
                    continue;
                }
                let base = match base {
                    Base::Roaming => named("APPDATA")?,
                    Base::Local => named("LOCALAPPDATA")?,
                    Base::Home => named("HOME")?,
                    Base::Temp => temp.clone(),
                    Base::Xdg => {
                        if env("XDG_DATA_HOME").is_some() {
                            named("XDG_DATA_HOME")?
                        } else {
                            named("HOME")?.join(".local/share")
                        }
                    }
                };
                let path = if mark.name == "Clipboard staging" && !sandbox {
                    crate::clipboard_picture::directory()
                } else {
                    base.join(relative)
                };
                purge_roots.push((mark.name, path));
            }
        }
        let data = purge_roots
            .iter()
            .filter(|(name, _)| {
                matches!(
                    *name,
                    "Roaming data" | "Legacy data" | "Application Support" | "Unix data"
                )
            })
            .map(|(_, p)| p.clone())
            .collect();
        let profiles = env("BT_POWERSHELL_PROFILE")
            .map(|_| named("BT_POWERSHELL_PROFILE").map(|p| vec![p]))
            .transpose()?;
        let documents = if env("BT_PSREADLINE_DOCUMENTS").is_some() {
            vec![named("BT_PSREADLINE_DOCUMENTS")?]
        } else {
            crate::psreadline::documents_directory()
                .into_iter()
                .collect()
        };
        let home = named(if platform == HostPlatform::Windows {
            "USERPROFILE"
        } else {
            "HOME"
        })?;
        let mut agents: [Vec<PathBuf>; 3] = Default::default();
        for (index, (variable, default)) in [
            ("CLAUDE_CONFIG_DIR", ".claude"),
            ("CODEX_HOME", ".codex"),
            ("COPILOT_HOME", ".copilot"),
        ]
        .into_iter()
        .enumerate()
        {
            agents[index].push(home.join(default));
            if env(variable).is_some() {
                push_unique(&mut agents[index], named(variable)?);
            }
        }
        Ok(Self {
            exe,
            data,
            profiles,
            documents,
            agents,
            purge_roots,
            sandbox: sandbox.then(|| temp.clone()),
        })
    }
}
fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths
        .iter()
        .any(|p| p == &path || crate::explorer_menu::same_path(p, &path))
    {
        paths.push(path);
    }
}
fn agent_path(index: usize, root: &Path) -> PathBuf {
    root.join(match index {
        0 => "settings.json",
        1 => "config.toml",
        _ => "hooks/folio.json",
    })
}
fn agent_apply(index: usize, path: &Path, decision: Decision, exe: &Path, data: &Path) -> Outcome {
    match index {
        0 => crate::attention_hooks::apply_at(path, decision, exe, data),
        1 => crate::attention_codex::apply_at(path, decision, exe, data),
        _ => crate::attention_copilot::apply_at(path, decision, exe, data),
    }
}
fn agent_fate(outcome: Outcome) -> Fate {
    match outcome {
        Outcome::Removed => Fate::Removed,
        Outcome::Unchanged => Fate::Absent,
        Outcome::LeftOther(paths) => Fate::Left(paths),
        Outcome::Refused(reason) => Fate::Refused(reason.to_owned()),
        Outcome::Installed | Outcome::TakeOverRequired(_) => {
            Fate::Refused(english(Text::CleanupUnexpected).to_owned())
        }
    }
}

/// Acquires and retains the existing instance claims BEFORE reading records or removing anything.
/// A mock system callback is mandatory in unit tests; they never query the real registry.
fn execute(scope: &Scope, purge: bool, system: impl FnMut(Remover) -> Vec<Entry>) -> Report {
    execute_with_claim(
        scope,
        purge,
        system,
        bt_platform::instance::claim_data_directory,
    )
}

fn execute_with_claim<T>(
    scope: &Scope,
    purge: bool,
    mut system: impl FnMut(Remover) -> Vec<Entry>,
    mut claim: impl FnMut(&Path) -> Option<T>,
) -> Report {
    if scope.data.is_empty() {
        return Report::new(vec![Entry::new(
            "Folio",
            Fate::Refused(english(Text::CleanupRoot).to_owned()),
        )]);
    }
    let mut claims = Vec::new();
    for data in &scope.data {
        let Some(claim) = claim(data) else {
            return Report::blocked(english(Text::CleanupRunning));
        };
        claims.push(claim);
    }
    let mut prepared = Vec::new();
    if purge {
        for (_, root) in &scope.purge_roots {
            let result = prepare_tree(root, &scope.exe);
            if let Err(error) = &result
                && is_busy(error)
            {
                return Report::blocked(&format!(
                    "{}: {} ({error})",
                    english(Text::CleanupBusy),
                    root.display()
                ));
            }
            prepared.push(result);
        }
    }
    let mut entries = Vec::new();
    let mut agents = scope.agents.clone();
    let mut documents = scope.documents.clone();
    for data in &scope.data {
        match Marks::read(data) {
            Ok(marks) => {
                for (index, roots) in [
                    marks.agent_config_roots.claude,
                    marks.agent_config_roots.codex,
                    marks.agent_config_roots.copilot,
                ]
                .into_iter()
                .enumerate()
                {
                    for root in roots {
                        if scope.sandbox.as_ref().is_none_or(|sandbox| {
                            root.starts_with(sandbox) && !has_parent_component(&root)
                        }) {
                            push_unique(&mut agents[index], root);
                        } else {
                            entries.push(Entry::new(
                                root.display().to_string(),
                                Fate::Refused(english(Text::CleanupRoot).to_owned()),
                            ));
                        }
                    }
                }
                for root in marks.psreadline_module_roots {
                    match crate::psreadline::documents_for_module_root(&root) {
                        Some(path)
                            if scope.sandbox.as_ref().is_none_or(|sandbox| {
                                path.starts_with(sandbox) && !has_parent_component(&path)
                            }) =>
                        {
                            push_unique(&mut documents, path)
                        }
                        _ => entries.push(Entry::new(
                            root.display().to_string(),
                            Fate::Refused(english(Text::CleanupRoot).to_owned()),
                        )),
                    }
                }
            }
            Err(e) => entries.push(Entry::new(
                data.join(crate::shell_integration::profile_marks::RECORD_FILE)
                    .display()
                    .to_string(),
                Fate::Refused(e.to_string()),
            )),
        }
    }
    for mark in INVENTORY {
        let label = format!(
            "{} ({})",
            mark.name,
            match mark.kind {
                Kind::PerCopy => "per-copy",
                Kind::PerAccount => "per-account",
                Kind::Data => "data",
            }
        );
        match mark.remover {
            Remover::Profiles => {
                let mut found = false;
                for (index, data) in scope.data.iter().enumerate() {
                    if index != 0
                        && !data
                            .join(crate::shell_integration::profile_marks::RECORD_FILE)
                            .exists()
                    {
                        continue;
                    }
                    let report = crate::shell_integration::remove_shell_integration_at(
                        data,
                        scope.profiles.as_deref(),
                    );
                    for file in report.files {
                        use crate::shell_integration::profile_marks::Fate as ShellFate;
                        found = true;
                        entries.push(Entry::new(
                            format!("{label}: {}", file.path.display()),
                            match file.fate {
                                ShellFate::Removed => Fate::Removed,
                                ShellFate::Unchanged => Fate::Absent,
                                ShellFate::Refused(reason) => Fate::Refused(reason),
                                ShellFate::Migrated => {
                                    Fate::Refused(english(Text::CleanupUnexpected).to_owned())
                                }
                            },
                        ));
                    }
                }
                if !found {
                    entries.push(Entry::new(label, Fate::Absent));
                }
            }
            Remover::PsReadLine => {
                if documents.is_empty() {
                    entries.push(Entry::new(&label, Fate::Absent));
                }
                for documents in &documents {
                    let path = crate::psreadline::module_directory(documents);
                    let fate = match prepare_tree(&path, &scope.exe)
                        .and_then(|_| crate::psreadline::remove_from(documents))
                    {
                        Ok(true) => Fate::Removed,
                        Ok(false) => Fate::Absent,
                        Err(e) => Fate::Refused(e.to_string()),
                    };
                    entries.push(Entry::new(format!("{label}: {}", path.display()), fate));
                }
            }
            Remover::Agent(index) => {
                for root in &agents[index] {
                    let path = agent_path(index, root);
                    entries.push(Entry::new(
                        format!("{label}: {}", path.display()),
                        agent_fate(agent_apply(
                            index,
                            &path,
                            Decision::Remove,
                            &scope.exe,
                            &scope.data[0],
                        )),
                    ));
                }
            }
            Remover::Explorer | Remover::Toast => entries.extend(system(mark.remover)),
            Remover::Absent => entries.push(Entry::new(label, Fate::Absent)),
            Remover::RecoverySnapshots if purge => entries.push(Entry::new(
                label,
                Fate::Kept(english(Text::CleanupRecovery)),
            )),
            Remover::RuntimeClaims
                if purge && bt_platform::host_platform() != HostPlatform::Windows =>
            {
                entries.push(Entry::new(label, Fate::Kept(english(Text::CleanupRuntime))))
            }
            Remover::Data(..) | Remover::RecoverySnapshots | Remover::RuntimeClaims => {}
        }
    }
    if purge {
        for ((name, root), prepared) in scope.purge_roots.iter().zip(prepared) {
            let result = prepared.and_then(|present| {
                // Shell removal may have persisted the account's Off decision in a root
                // absent at preflight. Purge also removes that newly created record.
                if prepare_tree(root, &scope.exe)? {
                    remove_tree(root)?;
                }
                Ok(if present { Fate::Removed } else { Fate::Absent })
            });
            entries.push(Entry::new(
                format!("{name} (data): {}", root.display()),
                result.unwrap_or_else(|e| Fate::Refused(e.to_string())),
            ));
        }
    }
    Report::new(entries)
}

fn has_parent_component(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
}

fn is_busy(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(32 | 33))
}
/// Preflight the WHOLE tree before deleting a leaf. Links in ancestors or descendants refuse.
/// There is no canonicalize-and-delete: that would turn a link into authority over its target.
fn prepare_tree(root: &Path, exe: &Path) -> io::Result<bool> {
    if !root.is_absolute() || root.parent().is_none() || has_parent_component(root) {
        return Err(io::Error::other(english(Text::CleanupRoot)));
    }
    let canonical = bt_platform::instance::canonical_path(root);
    let app = bt_platform::instance::canonical_path(
        exe.parent()
            .ok_or_else(|| io::Error::other(english(Text::CleanupRoot)))?,
    );
    if app.starts_with(&canonical) || canonical.starts_with(&app) {
        return Err(io::Error::other(english(Text::CleanupApplication)));
    }
    for parent in root.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        match fs::symlink_metadata(parent) {
            Ok(meta) if bt_platform::cleanup::is_link(&meta) => {
                return Err(io::Error::other(english(Text::CleanupLink)));
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    match fs::symlink_metadata(root) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
        Ok(meta) => {
            inspect_tree(root, &meta)?;
            Ok(true)
        }
    }
}
fn inspect_tree(path: &Path, meta: &fs::Metadata) -> io::Result<()> {
    if bt_platform::cleanup::is_link(meta) {
        return Err(io::Error::other(format!(
            "{}: {}",
            english(Text::CleanupLink),
            path.display()
        )));
    }
    if meta.is_dir() {
        for item in fs::read_dir(path)? {
            let path = item?.path();
            inspect_tree(&path, &fs::symlink_metadata(&path)?)?;
        }
    } else if meta.is_file() {
        if meta.permissions().readonly() {
            return Err(io::Error::other(
                crate::i18n::Text::ShellProfileReadOnly.text(),
            ));
        }
        bt_platform::cleanup::probe_file(path)?;
    } else {
        return Err(io::Error::other(english(Text::CleanupRoot)));
    }
    Ok(())
}
fn remove_tree(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if bt_platform::cleanup::is_link(&meta) {
        return Err(io::Error::other(english(Text::CleanupLink)));
    }
    if meta.is_dir() {
        // Use std's native recursive deletion (which does not traverse directory
        // links), after the whole-tree refusal pass. Do not build a path-based
        // recursive deleter that could follow a parent replaced during the walk.
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}
fn system_remove(remover: Remover) -> Vec<Entry> {
    match remover {
        Remover::Explorer => crate::explorer_menu::cleanup_registrations()
            .into_iter()
            .map(|(name, result)| {
                use crate::explorer_menu::CleanupRegistration;
                Entry::new(
                    name,
                    match result {
                        CleanupRegistration::Removed => Fate::Removed,
                        CleanupRegistration::Absent => Fate::Absent,
                        CleanupRegistration::Other(path) => Fate::Left(vec![path]),
                        CleanupRegistration::Refused(reason) => Fate::Refused(reason),
                    },
                )
            })
            .collect(),
        Remover::Toast => vec![Entry::new(
            "Toast identity (per-account)",
            match bt_platform::cleanup::remove_toast_identity() {
                Ok(true) => Fate::Removed,
                Ok(false) => Fate::Absent,
                Err(e) => Fate::Refused(e),
            },
        )],
        _ => Vec::new(),
    }
}
pub(crate) fn run(purge: bool) -> i32 {
    let scope = std::env::current_exe().and_then(|exe| {
        if let Some(root) = std::env::var_os("BT_UNINSTALL_ROOT") {
            Scope::sandbox(&PathBuf::from(root), exe)
        } else {
            Scope::resolve(
                exe,
                bt_platform::host_platform(),
                |key| std::env::var_os(key),
                std::env::temp_dir(),
                false,
            )
        }
    });
    let report = match scope {
        Ok(scope) => execute(&scope, purge, |remover| {
            if scope.sandbox.is_some() {
                vec![Entry::new(format!("{remover:?} (sandbox)"), Fate::Absent)]
            } else {
                system_remove(remover)
            }
        }),
        Err(e) => Report::new(vec![Entry::new("Folio", Fate::Refused(e.to_string()))]),
    };
    bt_platform::write_to_console(&report.stdout());
    bt_platform::write_std_error(report.stderr().as_bytes());
    report.code
}

#[cfg(test)]
#[path = "uninstall_tests.rs"]
mod tests;
