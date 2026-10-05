//! One non-interactive cleanup door. Ownership stays with each writer.
//! The inventory is executable data; historical locations are discovery, never ownership.
use crate::install_channel::Manager;
use crate::{
    attention_ownership::{Decision, Outcome},
    i18n::{Lang, Text},
    shell_integration::profile_marks::Marks,
};
use bt_platform::HostPlatform;
use bt_platform::admission::WorkerCtx;
use bt_platform::deferred_removal::{FailureWords, FileIdentity, Item, Removal, WaitFor};
use bt_platform::install_flip::Running;
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    PerCopy,
    PerAccount,
    Data,
}
/// **Where a data row lives: a directory the operating system names, never Folio.**
///
/// Every variant is a head the system (or the account, through the system's own variable)
/// names, and every row's relative path is wholly Folio's name below it. `Scope::resolve`
/// resolves the head and never the relative part: a link above the head is the machine's
/// own layout (macOS's `/var` → `private/var`), a link in the relative part is somebody's
/// plant under a Folio name, and the door refuses only the second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Base {
    /// `%APPDATA%`.
    Roaming,
    /// `%LOCALAPPDATA%`.
    Local,
    /// `~/Library/<folder>` on macOS: the home, and a folder of the Library that macOS names.
    Library(&'static str),
    /// The system's temporary directory (`std::env::temp_dir`; the clipboard staging
    /// folder's is `bt_platform::instance::temporary_directory`'s parent).
    Temp,
    /// `$XDG_DATA_HOME`, or `~/.local/share`, which the XDG base-directory rule names.
    Xdg,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Remover {
    Profiles,
    PsReadLine,
    Agent(usize),
    Explorer,
    Toast,
    /// The update's entrance at logon (U-22): `HKCU\...\Run\FolioUpdate-*`.
    Entrance,
    Absent,
    RecoverySnapshots,
    RuntimeClaims,
    /// Linux settings and keybindings in the data-claim-tagged XDG config namespace.
    #[cfg(target_os = "linux")]
    LinuxConfig,
    /// Linux Chromium cache in the data-claim-tagged XDG cache namespace.
    #[cfg(target_os = "linux")]
    LinuxCache,
    /// macOS: every update entrance in `~/Library/LaunchAgents`, by the
    /// entrance door's own name shape (`bt_platform::launch_agent::sweep`).
    UpdateEntrances,
    /// macOS: this bundle's installation home beside it (`update_txn::Home`).
    UpdateHome,
    Data(HostPlatform, Base, &'static str),
}
struct Mark {
    /// The mark's key, in the words `--uninstall-cleanup` has always printed.
    name: &'static str,
    /// The same words as a row of the table, so the door can say them in the
    /// settings' language (T-UNINSTALL-UX); the English column is `name`.
    says: Text,
    kind: Kind,
    remover: Remover,
    writer: &'static str,
}
// No sound syntax-only test can infer the destination of arbitrary Path arguments or OS
// framework writes. The source guard pins these named owners and the archive manifest.
const INVENTORY: &[Mark] = &[
    Mark {
        name: "PowerShell profiles",
        says: Text::CleanupMarkProfiles,
        kind: Kind::PerAccount,
        remover: Remover::Profiles,
        writer: "shell_integration.rs:add_to_profile",
    },
    Mark {
        name: "PSReadLine module",
        says: Text::CleanupMarkPsReadLine,
        kind: Kind::PerAccount,
        remover: Remover::PsReadLine,
        writer: "psreadline.rs:install_recorded",
    },
    Mark {
        name: "Claude Code hooks",
        says: Text::CleanupMarkClaude,
        kind: Kind::PerCopy,
        remover: Remover::Agent(0),
        writer: "attention_hooks.rs:apply_at",
    },
    Mark {
        name: "Codex notify",
        says: Text::CleanupMarkCodex,
        kind: Kind::PerCopy,
        remover: Remover::Agent(1),
        writer: "attention_codex.rs:apply_at",
    },
    Mark {
        name: "Copilot hooks",
        says: Text::CleanupMarkCopilot,
        kind: Kind::PerCopy,
        remover: Remover::Agent(2),
        writer: "attention_copilot.rs:apply_at",
    },
    Mark {
        name: "Explorer registrations",
        says: Text::CleanupMarkExplorer,
        kind: Kind::PerCopy,
        remover: Remover::Explorer,
        writer: "context_menu.rs:apply;explorer_menu.rs:request",
    },
    Mark {
        name: "Toast identity",
        says: Text::CleanupMarkToast,
        kind: Kind::PerAccount,
        remover: Remover::Toast,
        writer: "../bt-platform/src/lib.rs:Notifier::new;../bt-platform/src/lib.rs:Notifier::register_identity",
    },
    Mark {
        name: "Update entrance",
        says: Text::CleanupMarkEntrance,
        kind: Kind::PerCopy,
        remover: Remover::Entrance,
        writer: "../bt-platform/src/logon_hook.rs:arm_in",
    },
    Mark {
        name: "Start-menu shortcut",
        says: Text::CleanupMarkStartMenu,
        kind: Kind::PerAccount,
        remover: Remover::Absent,
        writer: "none (no shortcut writer)",
    },
    Mark {
        name: "Autostart / login item",
        says: Text::CleanupMarkAutostart,
        kind: Kind::PerAccount,
        remover: Remover::Absent,
        writer: "none (no persistent writer)",
    },
    Mark {
        name: "Quake / global hotkeys",
        says: Text::CleanupMarkHotkeys,
        kind: Kind::PerAccount,
        remover: Remover::Absent,
        writer: "../bt-platform/src/hotkey.rs:register (process lifetime)",
    },
    Mark {
        name: "Roaming data",
        says: Text::CleanupMarkRoaming,
        kind: Kind::Data,
        remover: Remover::Data(HostPlatform::Windows, Base::Roaming, "Folio"),
        writer: "persist.rs:storage_location",
    },
    Mark {
        name: "Legacy data",
        says: Text::CleanupMarkLegacy,
        kind: Kind::Data,
        remover: Remover::Data(HostPlatform::Windows, Base::Roaming, "BetterTerminal"),
        writer: "persist.rs:storage_location",
    },
    Mark {
        name: "Local data (including WebView2)",
        says: Text::CleanupMarkLocal,
        kind: Kind::Data,
        remover: Remover::Data(HostPlatform::Windows, Base::Local, "Folio"),
        writer: "webhost.rs:user_data_folder_in",
    },
    Mark {
        name: "Application Support",
        says: Text::CleanupMarkApplicationSupport,
        kind: Kind::Data,
        remover: Remover::Data(
            HostPlatform::MacOs,
            Base::Library("Application Support"),
            "Folio",
        ),
        writer: "persist.rs:storage_location;webhost.rs:web_engine_folder",
    },
    Mark {
        name: "WebKit",
        says: Text::CleanupMarkWebKit,
        kind: Kind::Data,
        remover: Remover::Data(
            HostPlatform::MacOs,
            Base::Library("WebKit"),
            "io.github.lulu-loopp.folio",
        ),
        writer: "../bt-platform/src/macos_webview.rs (WebKit, bundle identity)",
    },
    Mark {
        name: "Caches",
        says: Text::CleanupMarkCaches,
        kind: Kind::Data,
        remover: Remover::Data(
            HostPlatform::MacOs,
            Base::Library("Caches"),
            "io.github.lulu-loopp.folio",
        ),
        writer: "../bt-platform/src/macos_webview.rs (WebKit, bundle identity)",
    },
    Mark {
        name: "HTTPStorages",
        says: Text::CleanupMarkHttpStorages,
        kind: Kind::Data,
        remover: Remover::Data(
            HostPlatform::MacOs,
            Base::Library("HTTPStorages"),
            "io.github.lulu-loopp.folio",
        ),
        writer: "../bt-platform/src/macos_webview.rs (WebKit, bundle identity)",
    },
    Mark {
        name: "Preferences",
        says: Text::CleanupMarkPreferences,
        kind: Kind::Data,
        remover: Remover::Data(
            HostPlatform::MacOs,
            Base::Library("Preferences"),
            "io.github.lulu-loopp.folio.plist",
        ),
        writer: "../bt-platform/src/macos_app.rs (AppKit, bundle identity)",
    },
    Mark {
        name: "Saved Application State",
        says: Text::CleanupMarkSavedState,
        kind: Kind::Data,
        remover: Remover::Data(
            HostPlatform::MacOs,
            Base::Library("Saved Application State"),
            "io.github.lulu-loopp.folio.savedState",
        ),
        writer: "../bt-platform/src/macos_app.rs (AppKit, bundle identity)",
    },
    Mark {
        name: "Unix data",
        says: Text::CleanupMarkUnixData,
        kind: Kind::Data,
        remover: Remover::Data(HostPlatform::OtherUnix, Base::Xdg, "Folio"),
        writer: "persist.rs:storage_location",
    },
    #[cfg(target_os = "linux")]
    Mark {
        name: "Unix configuration",
        says: Text::CleanupMarkUnixConfig,
        kind: Kind::Data,
        remover: Remover::LinuxConfig,
        writer: "persist.rs:SettingsStore::open;persist.rs:KeybindingsStore::open",
    },
    #[cfg(target_os = "linux")]
    Mark {
        name: "Unix Chromium cache",
        says: Text::CleanupMarkUnixCache,
        kind: Kind::Data,
        remover: Remover::LinuxCache,
        writer: "../bt-platform/src/linux_web_dirs.rs:prepare_linux_web_dirs",
    },
    Mark {
        name: "User configuration recovery copies",
        says: Text::CleanupMarkRecovery,
        kind: Kind::Data,
        remover: Remover::RecoverySnapshots,
        writer: "shell_integration.rs:replace_profile;attention_hooks.rs:Config::land",
    },
    // The updater's two marks outside the bundle on macOS (0.4.6 U-26;
    // self-update design revision (b), §(b).3). The Windows home and entrance
    // are not here: the home is inside the install folder, and the `Run`
    // value's row is U-22's.
    Mark {
        name: "Update entrances (LaunchAgents)",
        says: Text::CleanupMarkUpdateEntrances,
        kind: Kind::PerAccount,
        remover: Remover::UpdateEntrances,
        writer: "../bt-platform/src/launch_agent.rs:arm",
    },
    Mark {
        name: "Update home beside the bundle",
        says: Text::CleanupMarkUpdateHome,
        kind: Kind::PerCopy,
        remover: Remover::UpdateHome,
        writer: "update_txn.rs:Home::for_bundle (made by the macOS Prepare, U-27)",
    },
    Mark {
        name: "Unix runtime claims",
        says: Text::CleanupMarkRuntimeClaims,
        kind: Kind::Data,
        remover: Remover::RuntimeClaims,
        writer: "../bt-platform/src/instance.rs:try_claim_data_directory",
    },
    // Temp rows apply on every platform (OtherUnix is the all-platform sentinel for Base::Temp).
    Mark {
        name: "Clipboard staging",
        says: Text::CleanupMarkClipboard,
        kind: Kind::Data,
        remover: Remover::Data(HostPlatform::OtherUnix, Base::Temp, "folio/clipboard"),
        writer: "clipboard_picture.rs:directory;clipboard_picture.rs:save",
    },
    Mark {
        name: "Panic log",
        says: Text::CleanupMarkPanicLog,
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
    /// Folio's own files went and these stood beside them, so these stayed
    /// (audit 3, E-1). Not [`Fate::Left`], which is about another *Folio* copy
    /// still installed on the machine.
    LeftNotOurs(Vec<PathBuf>),
    Refused(Why),
    Kept(Text),
    /// The program's own files: handed to the remover, which takes them once
    /// the process that asked has gone (T-UNINSTALL-UX).
    Scheduled,
    /// The program's own files, left to the package manager that installed
    /// them; its command (T-UNINSTALL-UX).
    Managed(String),
}

/// **Why a mark was refused**, kept as the table's row where the door itself
/// is the one refusing, so the sentence is said in the language the door
/// speaks (T-UNINSTALL-UX). A reason another owner or the operating system
/// gave is carried in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Why {
    /// One of the door's own sentences.
    Said(Text),
    /// One of the door's own sentences, and what it is about (a path, a name).
    SaidOf(Text, String),
    /// Somebody else's words.
    Other(String),
}

impl Why {
    fn in_lang(&self, lang: Lang) -> String {
        match self {
            Self::Said(text) => text.in_lang(lang).to_owned(),
            Self::SaidOf(text, about) => format!("{}: {about}", text.in_lang(lang)),
            Self::Other(words) => words.clone(),
        }
    }

    /// The reason an error carries: the door's own sentence when the door made
    /// it ([`Said`], [`HeldFile`]), the error's own words otherwise.
    fn of(error: &io::Error) -> Self {
        let inner = error.get_ref();
        if let Some(said) = inner.and_then(|inner| inner.downcast_ref::<Said>()) {
            return said.why();
        }
        if let Some(held) = inner.and_then(|inner| inner.downcast_ref::<HeldFile>()) {
            return Self::SaidOf(Text::CleanupBusy, held.name().display().to_string());
        }
        Self::Other(error.to_string())
    }
}

/// **One of the door's own refusals, carried on an `io::Error`** so that the
/// row it ends in can say it in the door's language ([`Why::of`]). Its
/// `Display` is the English, for any reader that only has the error.
#[derive(Debug)]
struct Said(Text, Option<String>);

impl Said {
    fn why(&self) -> Why {
        match &self.1 {
            None => Why::Said(self.0),
            Some(about) => Why::SaidOf(self.0, about.clone()),
        }
    }
}

impl std::fmt::Display for Said {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.why().in_lang(Lang::English))
    }
}

impl std::error::Error for Said {}

/// The door's own refusal, as an error.
fn said(text: Text) -> io::Error {
    io::Error::other(Said(text, None))
}

/// The door's own refusal about `about`, as an error.
fn said_of(text: Text, about: impl std::fmt::Display) -> io::Error {
    io::Error::other(Said(text, Some(about.to_string())))
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
    fn line(&self, lang: Lang) -> String {
        let paths = |paths: &[PathBuf]| {
            paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let detail = match &self.fate {
            Fate::Removed => Text::CleanupRemoved.in_lang(lang).to_owned(),
            Fate::Absent => Text::CleanupAbsent.in_lang(lang).to_owned(),
            Fate::Left(left) => format!("{} {}", Text::CleanupLeft.in_lang(lang), paths(left)),
            Fate::LeftNotOurs(left) => {
                format!("{} {}", Text::CleanupNotOurs.in_lang(lang), paths(left))
            }
            Fate::Kept(reason) => reason.in_lang(lang).to_owned(),
            Fate::Refused(why) => format!(
                "{} ({})",
                Text::CleanupRefused.in_lang(lang),
                why.in_lang(lang).replace(['\n', '\r'], " ")
            ),
            Fate::Scheduled => Text::UninstallProgramScheduled.in_lang(lang).to_owned(),
            Fate::Managed(command) => {
                format!("{} {command}", Text::UninstallProgramManaged.in_lang(lang))
            }
        };
        format!("{}: {detail}\n", self.mark.replace(['\n', '\r'], " "))
    }
}
fn english(text: Text) -> &'static str {
    text.in_lang(Lang::English)
}
/// **A mark's label**: its name and its kind, in `lang` — `PowerShell profiles
/// (per-account)`.
fn label(mark: &Mark, lang: Lang) -> String {
    format!(
        "{} ({})",
        mark.says.in_lang(lang),
        match mark.kind {
            Kind::PerCopy => Text::CleanupKindPerCopy,
            Kind::PerAccount => Text::CleanupKindPerAccount,
            Kind::Data => Text::CleanupKindData,
        }
        .in_lang(lang)
    )
}
/// The inventory's row for `remover`.
fn mark_of(remover: Remover) -> &'static Mark {
    INVENTORY
        .iter()
        .find(|mark| mark.remover == remover)
        .expect("every remover the door runs is a row of the inventory")
}
struct Report {
    code: i32,
    /// What the door could not establish on this platform, said before the rows it
    /// could. A notice is not a fate: it changes no exit code and names no mark.
    notices: Vec<Text>,
    entries: Vec<Entry>,
    /// The language every line is said in (T-UNINSTALL-UX).
    lang: Lang,
}
impl Report {
    fn new(entries: Vec<Entry>) -> Self {
        Self {
            code: i32::from(entries.iter().any(|e| matches!(e.fate, Fate::Refused(_)))),
            notices: Vec::new(),
            entries,
            lang: Lang::English,
        }
    }
    fn noticing(mut self, notices: Vec<Text>) -> Self {
        self.notices = notices;
        self
    }
    fn in_lang(mut self, lang: Lang) -> Self {
        self.lang = lang;
        self
    }
    fn blocked(reason: Why) -> Self {
        Self {
            code: 2,
            notices: Vec::new(),
            entries: vec![Entry::new("Folio", Fate::Refused(reason))],
            lang: Lang::English,
        }
    }
    fn stdout(&self) -> String {
        self.notices
            .iter()
            .map(|notice| format!("{}\n", notice.in_lang(self.lang)))
            .chain(self.entries.iter().map(|e| e.line(self.lang)))
            .collect()
    }
    fn stderr(&self) -> String {
        self.entries
            .iter()
            .filter(|e| matches!(e.fate, Fate::Refused(_)))
            .map(|e| e.line(self.lang))
            .collect()
    }
}

struct Scope {
    exe: PathBuf,
    data: Vec<PathBuf>,
    profiles: Option<Vec<PathBuf>>,
    documents: Vec<PathBuf>,
    agents: [Vec<PathBuf>; 3],
    purge_roots: Vec<(&'static Mark, PathBuf)>,
    /// macOS: `~/Library/LaunchAgents`, where the update entrances are.
    launch_agents: Option<PathBuf>,
    /// macOS: this bundle's installation home, `<parent>/.<Bundle>.folio-update`.
    update_home: Option<PathBuf>,
    /// A private random child directory is made below this per-user root for
    /// the native remover. In a sandbox it stays below `BT_UNINSTALL_ROOT`.
    remover_home: PathBuf,
    sandbox: Option<PathBuf>,
    /// The language the door speaks: English for `--uninstall-cleanup`, the
    /// settings' for `--uninstall` ([`Scope::speaking`]).
    lang: Lang,
}
impl Scope {
    /// The same scope, speaking `lang`.
    fn speaking(mut self, lang: Lang) -> Self {
        self.lang = lang;
        self
    }
    fn sandbox(root: &Path, exe: PathBuf) -> io::Result<Self> {
        if !root.is_absolute() || has_parent_component(root) {
            return Err(said(Text::CleanupRoot));
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
            let p = env(name)
                .map(PathBuf::from)
                .ok_or_else(|| said_of(Text::CleanupRoot, name))?;
            if !p.is_absolute() {
                return Err(said_of(Text::CleanupRoot, name));
            }
            Ok(p)
        };
        let mut purge_roots = Vec::new();
        let mut data = Vec::new();
        for mark in INVENTORY {
            debug_assert!(!mark.writer.is_empty());
            if let Remover::Data(host, base, relative) = mark.remover {
                if host != platform && base != Base::Temp {
                    continue;
                }
                let head = match base {
                    Base::Roaming => named("APPDATA")?,
                    Base::Local => named("LOCALAPPDATA")?,
                    Base::Library(folder) => named("HOME")?.join("Library").join(folder),
                    Base::Temp => temp.clone(),
                    Base::Xdg => {
                        if env("XDG_DATA_HOME").is_some() {
                            named("XDG_DATA_HOME")?
                        } else {
                            named("HOME")?.join(".local/share")
                        }
                    }
                };
                let (head, folio) = if mark.name == "Clipboard staging" && !sandbox {
                    clipboard_staging()
                } else {
                    (head, PathBuf::from(relative))
                };
                // The data roots are claimed under the spelling Folio's own writer claims them
                // (`persist::storage_location` reads the same variables): on Windows a claim's
                // name folds case and nothing else, so a resolved spelling could miss a running
                // Folio's claim.
                if matches!(
                    mark.name,
                    "Roaming data" | "Legacy data" | "Application Support" | "Unix data"
                ) {
                    data.push(head.join(&folio));
                }
                purge_roots.push((mark, purge_root(&head, &folio)));
            }
        }
        #[cfg(target_os = "linux")]
        if platform == HostPlatform::OtherUnix {
            let Some(data_root) = data.first() else {
                return Err(said(Text::CleanupRoot));
            };
            let namespace =
                PathBuf::from("Folio").join(bt_platform::instance::directory_tag(data_root));
            let home = env("HOME")
                .map(PathBuf::from)
                .filter(|path| !path.as_os_str().is_empty() && path.is_absolute());
            for (remover, variable, default, child) in [
                (Remover::LinuxConfig, "XDG_CONFIG_HOME", ".config", None),
                (
                    Remover::LinuxCache,
                    "XDG_CACHE_HOME",
                    ".cache",
                    Some("Chromium"),
                ),
            ] {
                let root = env(variable)
                    .map(PathBuf::from)
                    .filter(|path| !path.as_os_str().is_empty() && path.is_absolute())
                    .or_else(|| home.as_ref().map(|home| home.join(default)));
                if let Some(root) = root {
                    let relative =
                        child.map_or_else(|| namespace.clone(), |name| namespace.join(name));
                    purge_roots.push((mark_of(remover), purge_root(&root, &relative)));
                }
            }
        }
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
        let (launch_agents, update_home) = if platform == HostPlatform::MacOs {
            (
                Some(home.join("Library/LaunchAgents")),
                crate::update_txn::Home::of(platform, &exe).map(|h| h.root().to_path_buf()),
            )
        } else {
            (None, None)
        };
        let remover_home = if sandbox {
            temp.join("Folio")
        } else {
            match platform {
                HostPlatform::Windows => named("LOCALAPPDATA")?.join("Folio"),
                HostPlatform::MacOs => home.join("Library/Application Support/Folio"),
                HostPlatform::OtherUnix => temp.join("folio"),
            }
        };
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
            launch_agents,
            update_home,
            remover_home,
            sandbox: sandbox.then(|| temp.clone()),
            lang: Lang::English,
        })
    }
}
/// **A purge root: the head the operating system names, resolved, and Folio's name below it as
/// written.**
///
/// The door refuses a root with a link anywhere above it or inside it ([`prepare_tree`]), so
/// that a link somebody plants under a Folio-named path never turns a deletion into authority
/// over its target. The head is not such a path: nobody plants a trap for Folio at `/var`,
/// `%APPDATA%` or `~/Library/Caches` — they are the machine's layout, and on macOS the system's
/// own temporary directory is reached through the link `/var` → `private/var`. So the head is
/// resolved here, before the rule applies, and the part Folio names is appended untouched: a
/// link there is still found by the walk and still refused. Resolved in its ordinary spelling
/// (no Windows verbatim prefix), because that is the spelling the rows print. A head that does
/// not exist yet is resolved as far as it exists, the rest appended as written
/// (`bt_platform::instance::canonical_path`).
fn purge_root(head: &Path, folio: &Path) -> PathBuf {
    bt_platform::handoff::strip_verbatim_prefix(&bt_platform::instance::canonical_path(head))
        .join(folio)
}

/// **The clipboard staging folder, split where the system's name ends.**
///
/// `bt_platform::instance::temporary_directory` is "a folder of Folio's own inside the system's
/// temporary directory" — one component Folio names (`folio`, or `folio-<uid>` under the shared
/// `/tmp`) below the directory the system names — and the writer's own folder is below that.
fn clipboard_staging() -> (PathBuf, PathBuf) {
    let staging = crate::clipboard_picture::directory();
    let system = bt_platform::instance::temporary_directory()
        .parent()
        .expect("Folio's temporary folder is one name below the system's")
        .to_path_buf();
    let folio = staging
        .strip_prefix(&system)
        .expect("the clipboard staging folder is inside Folio's temporary folder")
        .to_path_buf();
    (system, folio)
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
        Outcome::Refused(reason) => Fate::Refused(Why::Other(reason.to_owned())),
        Outcome::Installed | Outcome::TakeOverRequired(_) => {
            Fate::Refused(Why::Said(Text::CleanupUnexpected))
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
    let lang = scope.lang;
    if scope.data.is_empty() {
        return Report::new(vec![Entry::new(
            "Folio",
            Fate::Refused(Why::Said(Text::CleanupRoot)),
        )])
        .in_lang(lang);
    }
    let mut claims = Vec::new();
    for data in &scope.data {
        let Some(claim) = claim(data) else {
            return Report::blocked(Why::Said(Text::CleanupRunning)).in_lang(lang);
        };
        claims.push(claim);
    }
    let mut prepared = Vec::new();
    if purge {
        for (_, root) in &scope.purge_roots {
            let result = prepare_tree(root, &scope.exe);
            if let Err(error) = &result
                && let Some(held) = held_file(error)
            {
                return Report::blocked(Why::SaidOf(
                    Text::CleanupBusy,
                    format!("{} ({})", root.display(), held.display()),
                ))
                .in_lang(lang);
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
                        if usable_recorded_directory(scope, &root) {
                            push_unique(&mut agents[index], root);
                        } else {
                            entries.push(Entry::new(
                                root.display().to_string(),
                                Fate::Refused(Why::Said(Text::CleanupRecorded)),
                            ));
                        }
                    }
                }
                for root in marks.psreadline_module_roots {
                    // The module directory's own shape answers "is this a module root":
                    // a Documents root exists only if joining the module's fixed relative
                    // path to it spells this recorded path back.
                    match crate::psreadline::documents_for_module_root(&root) {
                        Some(path)
                            if usable_recorded_directory(scope, &root)
                                && usable_recorded_directory(scope, &path) =>
                        {
                            push_unique(&mut documents, path)
                        }
                        _ => entries.push(Entry::new(
                            root.display().to_string(),
                            Fate::Refused(Why::Said(Text::CleanupRecorded)),
                        )),
                    }
                }
            }
            Err(e) => entries.push(Entry::new(
                data.join(crate::shell_integration::profile_marks::RECORD_FILE)
                    .display()
                    .to_string(),
                Fate::Refused(Why::of(&e)),
            )),
        }
    }
    for mark in INVENTORY {
        let label = label(mark, lang);
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
                                ShellFate::Refused(reason) => Fate::Refused(Why::Other(reason)),
                                ShellFate::Migrated => {
                                    Fate::Refused(Why::Said(Text::CleanupUnexpected))
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
                    use crate::psreadline::Removed;
                    let path = crate::psreadline::module_directory(documents);
                    // One remover, and it is the row's — the rule that a removal
                    // takes only what Folio wrote lives inside `remove_from`, so
                    // this door reports what it did rather than repeating it.
                    let fate = match prepare_tree(&path, &scope.exe)
                        .and_then(|_| crate::psreadline::remove_from(documents))
                    {
                        Ok(Removed::Took { left }) if left.is_empty() => Fate::Removed,
                        Ok(Removed::Took { left }) => Fate::LeftNotOurs(left),
                        Ok(Removed::Nothing) => Fate::Absent,
                        Ok(Removed::NotOurs) => Fate::LeftNotOurs(vec![path.clone()]),
                        Err(e) => Fate::Refused(Why::of(&e)),
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
            Remover::Explorer | Remover::Toast | Remover::Entrance => {
                entries.extend(system(mark.remover))
            }
            Remover::UpdateEntrances => {
                entries.extend(update_entrances(&label, scope.launch_agents.as_deref()));
            }
            Remover::UpdateHome => {
                entries.push(update_home(&label, scope.update_home.as_deref()));
            }
            Remover::Absent => entries.push(Entry::new(label, Fate::Absent)),
            Remover::RecoverySnapshots if purge => {
                entries.push(Entry::new(label, Fate::Kept(Text::CleanupRecovery)));
            }
            Remover::RuntimeClaims
                if purge && bt_platform::host_platform() != HostPlatform::Windows =>
            {
                entries.push(Entry::new(label, Fate::Kept(Text::CleanupRuntime)));
            }
            Remover::Data(..) | Remover::RecoverySnapshots | Remover::RuntimeClaims => {}
            #[cfg(target_os = "linux")]
            Remover::LinuxConfig | Remover::LinuxCache => {}
        }
    }
    if purge {
        for ((mark, root), prepared) in scope.purge_roots.iter().zip(prepared) {
            let result = prepared.and_then(|_preflight| {
                // The preflight decides whether the run may proceed; what the row
                // reports is what was true at the check that decided the deletion.
                // A root created after the preflight is removed, and says `removed`;
                // one that went away in between says `not present`.
                let present = prepare_tree(root, &scope.exe)?;
                if present {
                    remove_tree(root)?;
                }
                Ok(if present { Fate::Removed } else { Fate::Absent })
            });
            entries.push(Entry::new(
                format!("{}: {}", label(mark, lang), root.display()),
                result.unwrap_or_else(|e| Fate::Refused(Why::of(&e))),
            ));
        }
    }
    Report::new(entries)
        .noticing(purge_notices(bt_platform::host_platform(), purge))
        .in_lang(lang)
}

/// **The update entrances row**: every LaunchAgent plist of the entrance
/// door's own name, removed by that door; one line per plist, or one `not
/// present` line. Another program's agents are never looked at twice.
fn update_entrances(label: &str, agents: Option<&Path>) -> Vec<Entry> {
    let Some(agents) = agents else {
        return vec![Entry::new(label, Fate::Absent)];
    };
    match bt_platform::launch_agent::sweep(agents) {
        Ok(swept) if swept.is_empty() => vec![Entry::new(label, Fate::Absent)],
        Ok(swept) => swept
            .into_iter()
            .map(|(path, removed)| {
                Entry::new(
                    format!("{label}: {}", path.display()),
                    removed.map_or_else(
                        |e| Fate::Refused(Why::Other(e.to_string())),
                        |()| Fate::Removed,
                    ),
                )
            })
            .collect(),
        Err(e) => vec![Entry::new(label, Fate::Refused(Why::Other(e.to_string())))],
    }
}

/// **The update home row**: this bundle's installation home, removed whole
/// through the update door's own durable remove (a link at its place is
/// removed, never followed). The home is found from this bundle's path, so
/// it is this copy's by construction.
fn update_home(label: &str, home: Option<&Path>) -> Entry {
    let Some(home) = home else {
        return Entry::new(label, Fate::Absent);
    };
    let label = format!("{label}: {}", home.display());
    if fs::symlink_metadata(home).is_err_and(|e| e.kind() == io::ErrorKind::NotFound) {
        return Entry::new(label, Fate::Absent);
    }
    if let Err(refused) = detach_images_under(home) {
        return Entry::new(label, Fate::Refused(Why::Other(refused)));
    }
    match bt_platform::install_txn::durable_remove(home) {
        Ok(()) => Entry::new(label, Fate::Removed),
        Err(e) => Entry::new(label, Fate::Refused(Why::Other(e.to_string()))),
    }
}

/// **Every update image still mounted under the home, detached before the home
/// is removed** (U-17's debt 7, the coordinator's ruling in U-27): a read-only
/// volume inside it would stop the removal halfway. The mount table is read
/// first, which waits on nothing; only when it lists a mount is a worker
/// started for the detach (`bt_platform::macos_update::detach_all_under` takes
/// the worker's capability, and waits on `hdiutil`), and this cleanup — a
/// process with no window — waits for it. Only macOS mounts an update's image.
fn detach_images_under(home: &Path) -> Result<(), String> {
    if bt_platform::host_platform() != HostPlatform::MacOs {
        return Ok(());
    }
    let points =
        bt_platform::macos_update::mounts_under(home).map_err(|refusal| refusal.to_string())?;
    if points.is_empty() {
        return Ok(());
    }
    let home = home.to_path_buf();
    bt_platform::spawn_at_priority(
        "folio-update-home-detach",
        bt_platform::ThreadPriority::BelowNormal,
        move |worker| {
            bt_platform::macos_update::detach_all_under(worker, &home)
                .map_err(|refusal| refusal.to_string())
        },
    )
    .map_err(|error| error.to_string())?
    .join()
    .map_err(|_| "the detach's worker stopped".to_owned())?
}

/// What the door cannot establish on this platform, and therefore does not claim.
///
/// `probe_file` is a no-op off Windows (`bt-platform/src/cleanup.rs`): there is no
/// mandatory sharing mode to ask, so §6.4's "requires that no Folio or WebView2 process
/// holds the data" cannot be checked on macOS. The door says so rather than implying a
/// check it did not make — the flock still covers a second native Folio, and unlinking
/// an open file is safe on Unix, so this is a promise gap and not data loss.
fn purge_notices(platform: HostPlatform, purge: bool) -> Vec<Text> {
    if purge && platform == HostPlatform::MacOs {
        vec![Text::CleanupMacHeld]
    } else {
        Vec::new()
    }
}

/// Whether a directory read out of the account record may be used.
///
/// **A recorded path is data, never authority.** `integration-marks.json` is an
/// ordinary file in the data folder; anything that can write it can name any path on
/// the machine. So the shape is checked on every run and not only under a test sandbox
/// (where the check is additionally containment): absolute, free of `..`, never a
/// filesystem root, and a directory rather than a file — the door only ever joins a
/// fixed relative name onto it, and a mark naming somebody's document names no
/// configuration root.
fn usable_recorded_directory(scope: &Scope, path: &Path) -> bool {
    path.is_absolute()
        && !has_parent_component(path)
        && path.parent().is_some()
        && !fs::metadata(path).is_ok_and(|meta| !meta.is_dir())
        && scope
            .sandbox
            .as_ref()
            .is_none_or(|sandbox| path.starts_with(sandbox))
}

fn has_parent_component(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
}

/// The file a process is holding, carried ON the error so the refusal can name it.
///
/// ERROR_SHARING_VIOLATION and ERROR_LOCK_VIOLATION say only that something is held;
/// which file it was is known at the probe and nowhere else, and a message a reader
/// cannot act on is half a refusal. The basename alone: the root is already printed
/// beside it, and the leaf is what has to be closed.
#[derive(Debug)]
struct HeldFile(OsString);
impl std::fmt::Display for HeldFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {}",
            english(Text::CleanupBusy),
            self.name().display()
        )
    }
}
impl std::error::Error for HeldFile {}
impl HeldFile {
    fn name(&self) -> &Path {
        Path::new(&self.0)
    }
}

/// The rule is unchanged — any process holding any file in a data root stops the whole
/// run — and only the sentence gains the name.
fn named_if_busy(error: io::Error, path: &Path) -> io::Error {
    if matches!(error.raw_os_error(), Some(32 | 33)) {
        return io::Error::new(
            error.kind(),
            HeldFile(path.file_name().unwrap_or(path.as_os_str()).to_owned()),
        );
    }
    error
}

fn held_file(error: &io::Error) -> Option<&Path> {
    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<HeldFile>())
        .map(HeldFile::name)
}
/// Preflight the WHOLE tree before deleting a leaf. Links in ancestors or descendants refuse.
/// There is no canonicalize-and-delete: that would turn a link into authority over its target.
/// A purge root's operating-system head arrives already resolved ([`purge_root`]), so what this
/// walk can refuse is a link in the part Folio names.
fn prepare_tree(root: &Path, exe: &Path) -> io::Result<bool> {
    if !root.is_absolute() || root.parent().is_none() || has_parent_component(root) {
        return Err(said(Text::CleanupRoot));
    }
    let canonical = bt_platform::instance::canonical_path(root);
    let app =
        bt_platform::instance::canonical_path(exe.parent().ok_or_else(|| said(Text::CleanupRoot))?);
    if app.starts_with(&canonical) || canonical.starts_with(&app) {
        return Err(said(Text::CleanupApplication));
    }
    for parent in root.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        match fs::symlink_metadata(parent) {
            Ok(meta) if bt_platform::cleanup::is_link(&meta) => {
                return Err(said(Text::CleanupLink));
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
        return Err(said_of(Text::CleanupLink, path.display()));
    }
    if meta.is_dir() {
        for item in fs::read_dir(path)? {
            let path = item?.path();
            inspect_tree(&path, &fs::symlink_metadata(&path)?)?;
        }
    } else if meta.is_file() {
        if meta.permissions().readonly() {
            return Err(said(Text::ShellProfileReadOnly));
        }
        bt_platform::cleanup::probe_file(path).map_err(|e| named_if_busy(e, path))?;
    } else {
        return Err(said(Text::CleanupRoot));
    }
    Ok(())
}
fn remove_tree(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if bt_platform::cleanup::is_link(&meta) {
        return Err(said(Text::CleanupLink));
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
fn system_remove(worker: &WorkerCtx, remover: Remover, exe: &Path, lang: Lang) -> Vec<Entry> {
    match remover {
        Remover::Explorer => crate::explorer_menu::cleanup_registrations(worker)
            .into_iter()
            .map(|(name, result)| {
                use crate::explorer_menu::CleanupRegistration;
                Entry::new(
                    format!(
                        "{} ({})",
                        name.in_lang(lang),
                        Text::CleanupKindPerCopy.in_lang(lang)
                    ),
                    match result {
                        CleanupRegistration::Removed => Fate::Removed,
                        CleanupRegistration::Absent => Fate::Absent,
                        CleanupRegistration::Other(path) => Fate::Left(vec![path]),
                        CleanupRegistration::Refused(reason) => Fate::Refused(Why::Other(reason)),
                    },
                )
            })
            .collect(),
        Remover::Toast => vec![Entry::new(
            label(mark_of(Remover::Toast), lang),
            match bt_platform::cleanup::remove_toast_identity() {
                Ok(true) => Fate::Removed,
                Ok(false) => Fate::Absent,
                Err(e) => Fate::Refused(Why::Other(e)),
            },
        )],
        Remover::Entrance => entrance_entries(
            bt_platform::host_platform(),
            exe,
            lang,
            bt_platform::logon_hook::clean,
        ),
        _ => Vec::new(),
    }
}

/// **The update entrance's row** (U-22; `docs/plans/design/self-update-2026-09-16.md`
/// §(b).3): this copy's `FolioUpdate-*` values under the `Run` key — those naming
/// its installation home `H`, or a program that no longer exists — are removed by
/// `clean`, the entrance door's own remover; another copy's live entrance is left
/// and said. Only Windows has this entrance (the macOS LaunchAgent is U-26's).
fn entrance_entries(
    platform: HostPlatform,
    exe: &Path,
    lang: Lang,
    clean: impl FnOnce(
        &Path,
    ) -> Result<
        Vec<(String, bt_platform::logon_hook::Cleaned)>,
        bt_platform::logon_hook::Refusal,
    >,
) -> Vec<Entry> {
    use bt_platform::logon_hook::Cleaned;
    let label = label(mark_of(Remover::Entrance), lang);
    let home = match platform {
        HostPlatform::Windows => crate::update_txn::Home::of(platform, exe),
        HostPlatform::MacOs | HostPlatform::OtherUnix => None,
    };
    let Some(home) = home else {
        return vec![Entry::new(label, Fate::Absent)];
    };
    match clean(home.root()) {
        Err(refusal) => vec![Entry::new(
            label,
            Fate::Refused(Why::Other(refusal.to_string())),
        )],
        Ok(cleaned) if cleaned.is_empty() => vec![Entry::new(label, Fate::Absent)],
        Ok(cleaned) => cleaned
            .into_iter()
            .map(|(name, fate)| {
                Entry::new(
                    format!("{label}: {name}"),
                    match fate {
                        Cleaned::Removed => Fate::Removed,
                        Cleaned::Left(program) => Fate::Left(vec![program]),
                        Cleaned::Refused(refusal) => Fate::Refused(Why::Other(refusal.to_string())),
                    },
                )
            })
            .collect(),
    }
}
/// **`BT_UNINSTALL_ROOT` is a test instrument, and a shipped build does not read it.**
///
/// It redirects the whole door — every data root, profile, module, agent root and temp
/// file — into a fixture tree. In a release build that is not a sandbox but a silent
/// failure: a stray or inherited value would make a production cleanup report every
/// real integration "not present" and exit 0 with the machine untouched, and the one
/// thing a person runs an uninstaller to learn is whether their machine is clean. A
/// notice on stdout would leave the same run doing nothing; refusing to read the
/// variable at all leaves no run that can lie. Debug and test builds keep the door (the
/// fixture layout is in docs/BT-ENVIRONMENT.md); nothing in `scripts/` sets it, so no
/// release-binary test relies on it.
const SANDBOX_DOOR: bool = cfg!(any(debug_assertions, test));

fn sandbox_root(honoured: bool, value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|_| honoured).map(PathBuf::from)
}

/// **The standalone entry both verbs' processes take, once** — the process's
/// main thread becomes this worker for good, and every wait the door makes (the
/// Explorer registrations' worker, the asker's end) is a worker's wait
/// (`docs/plans/design/thread-door-2026-09-26.md` revision (c)6).
pub(crate) const DOOR_ENTRY: &str = "folio-uninstall-cleanup";

/// **Enter the door's one standalone main** and run `body` on it.
///
/// Both uninstall verbs are answered first in `fn main`, above the argument
/// parse, on a main thread that has no role yet, and each enters once: the
/// entry cannot be refused there.
pub(crate) fn standalone<R>(body: impl FnOnce(&WorkerCtx) -> R) -> R {
    bt_platform::admission::enter_standalone_main(DOOR_ENTRY, body).expect(
        "the uninstaller's door is answered first in `fn main`, on a main thread with no role, \
         and enters once",
    )
}

/// Enter the copied native remover's own standalone main. Its private argv
/// word is checked before console adoption and before any window path.
pub(crate) fn remover_standalone<R>(body: impl FnOnce(&WorkerCtx) -> R) -> R {
    bt_platform::admission::enter_standalone_main("folio-uninstall-remove", body).expect(
        "the native remover is answered first in `fn main`, on a main thread with no role, and \
         enters once",
    )
}

/// The door, for either verb: `--uninstall-cleanup [--purge]`, or
/// `--uninstall [--remove-data] [--after-pid <pid>]` (T-UNINSTALL-UX).
pub(crate) fn run(door: crate::cli::UninstallDoor) -> i32 {
    standalone(|worker| run_within(worker, door))
}

fn run_within(worker: &WorkerCtx, door: crate::cli::UninstallDoor) -> i32 {
    let (purge, asked) = match door {
        crate::cli::UninstallDoor::Cleanup { purge } => (purge, None),
        crate::cli::UninstallDoor::Uninstall { remove_data, after } => (remove_data, Some(after)),
    };
    let scope = std::env::current_exe().and_then(|exe| {
        if let Some(root) = sandbox_root(SANDBOX_DOOR, std::env::var_os("BT_UNINSTALL_ROOT")) {
            Scope::sandbox(&root, exe)
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
        Err(e) => Report::new(vec![Entry::new("Folio", Fate::Refused(Why::of(&e)))]),
        Ok(scope) => {
            let system = |scope: &Scope, remover| {
                if scope.sandbox.is_some() {
                    vec![Entry::new(format!("{remover:?} (sandbox)"), Fate::Absent)]
                } else {
                    system_remove(worker, remover, &scope.exe, scope.lang)
                }
            };
            match asked {
                None => execute(&scope, purge, |remover| system(&scope, remover)),
                Some(after) => {
                    let lang = door_language(&scope, &bt_platform::os_ui_language());
                    // Every owner's own words too: the door's process speaks one language.
                    crate::i18n::install(lang);
                    let scope = scope.speaking(lang);
                    let asker = after.and_then(|pid| {
                        Some(Running {
                            pid,
                            started: bt_platform::install_flip::started_of(pid)?,
                        })
                    });
                    uninstall(
                        worker,
                        &scope,
                        asker,
                        AFTER_PID_WITHIN,
                        |scope| execute(scope, purge, |remover| system(scope, remover)),
                        |scope| {
                            remove_the_program(
                                worker,
                                scope,
                                crate::install_channel::channel_of(&scope.exe),
                                bt_platform::host_platform(),
                                installed_members,
                                &the_processes_to_outlive(asker),
                                bt_platform::deferred_removal::schedule,
                            )
                        },
                    )
                }
            }
        }
    };
    bt_platform::write_to_console(&report.stdout());
    bt_platform::write_std_error(report.stderr().as_bytes());
    // **The Settings road has nobody reading a console**: the Folio that asked
    // has gone, and this process was started with none. A run that did not
    // complete is said in the box a windowless process can raise; a run that
    // did says nothing, because the program's disappearance is the answer.
    if matches!(asked, Some(Some(_))) && report.code != 0 {
        bt_platform::standalone_alert(crate::APP_NAME, &report.stdout());
    }
    report.code
}

/// **How long the door waits for the Folio that asked for its own uninstall
/// to end** (`--after-pid`) before it answers that a Folio is running: the
/// asker is past its last act when it starts the door, and what is left of it
/// is the process's own exit.
pub(crate) const AFTER_PID_WITHIN: Duration = Duration::from_secs(60);

/// How often the door looks again for the asker's end.
const AFTER_PID_POLL: Duration = Duration::from_millis(100);

/// **The language the `--uninstall` door speaks**: the one `settings.json`
/// names in the data directory Folio would open (`persist::as_it_stands` over
/// the current and the previous name), resolved against `os` — the OS's UI
/// language — when it names `System` or there is no file, and English past
/// that (`i18n::resolve`). Read through `SettingsStore::peek_language`, as
/// every door process reads it: it asks nobody who writes the directory.
fn door_language(scope: &Scope, os: &str) -> Lang {
    let home = match scope.data.as_slice() {
        [current, previous, ..] => Some(crate::persist::as_it_stands(
            current.clone(),
            previous.clone(),
        )),
        [current] => Some(current.clone()),
        [] => None,
    };
    crate::resolved_language_on(
        home.map_or(bt_persist::LanguageV1::System, |home| {
            crate::persist::SettingsStore::peek_language(&home)
        }),
        os,
    )
}

/// **Whether `asker` has ended within `within`**, looked at every
/// [`AFTER_PID_POLL`] on the door's worker: by its pid and start instant, so a
/// pid handed to another process is not waited for.
fn waited_for(worker: &WorkerCtx, asker: Running, within: Duration) -> bool {
    let until = Instant::now() + within;
    while bt_platform::install_flip::still_running(asker) {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        bt_platform::wait::sleep_within(worker, AFTER_PID_POLL.min(left));
    }
    true
}

/// **`--uninstall`, after its scope is known** (T-UNINSTALL-UX): the asker's
/// end first, when one was named; then the cleanup (`cleanup`, the door's
/// `execute`); then, only after a cleanup that completed, the program's own
/// files (`program`). A cleanup that did not complete keeps the program — it
/// is the one thing that can run the cleanup again — and says so.
fn uninstall(
    worker: &WorkerCtx,
    scope: &Scope,
    asker: Option<Running>,
    within: Duration,
    cleanup: impl FnOnce(&Scope) -> Report,
    program: impl FnOnce(&Scope) -> Vec<Entry>,
) -> Report {
    uninstall_waiting(worker, scope, asker, within, waited_for, cleanup, program)
}

/// The same ordering with the process wait supplied as a behavior seam. Tests
/// use a readiness/release handshake here, so the ordering proof contains no
/// sleep chosen to make a child "probably" still alive.
fn uninstall_waiting(
    worker: &WorkerCtx,
    scope: &Scope,
    asker: Option<Running>,
    within: Duration,
    wait: impl FnOnce(&WorkerCtx, Running, Duration) -> bool,
    cleanup: impl FnOnce(&Scope) -> Report,
    program: impl FnOnce(&Scope) -> Vec<Entry>,
) -> Report {
    if let Some(asker) = asker
        && !wait(worker, asker, within)
    {
        return Report::blocked(Why::Said(Text::CleanupRunning)).in_lang(scope.lang);
    }
    let mut report = cleanup(scope);
    if report.code != 0 {
        report.entries.push(Entry::new(
            program_label(scope.lang),
            Fate::Kept(Text::UninstallProgramKept),
        ));
        return report;
    }
    report.entries.extend(program(scope));
    report.code = i32::from(
        report
            .entries
            .iter()
            .any(|entry| matches!(entry.fate, Fate::Refused(_))),
    );
    report
}

/// The program row's label: `Program files (per-copy)`.
fn program_label(lang: Lang) -> String {
    format!(
        "{} ({})",
        Text::UninstallProgramMark.in_lang(lang),
        Text::CleanupKindPerCopy.in_lang(lang)
    )
}

/// **Whom the program's removal waits for**: the door's parent — the process
/// showing the door's lines, the script's console on the zip's road — the door
/// itself, and the asker when one was named.
fn the_processes_to_outlive(asker: Option<Running>) -> Vec<Running> {
    let me = std::process::id();
    bt_platform::install_flip::parent_of_this_process()
        .into_iter()
        .chain(
            bt_platform::install_flip::started_of(me).map(|started| Running { pid: me, started }),
        )
        .chain(asker)
        .collect()
}

/// **The command a package manager uninstalls its copy with**, with the
/// cleanup before it for a manager that runs none of its own (no uninstall
/// hook).
pub(crate) fn manager_uninstall(manager: Manager, uninstall_hook: bool) -> String {
    let command = match manager {
        Manager::Scoop => "scoop uninstall folio",
        Manager::Homebrew => "brew uninstall --zap folio",
        Manager::Winget => "winget uninstall --id WeiyiShi.Folio --exact",
    };
    if uninstall_hook {
        command.to_owned()
    } else {
        format!("folio --uninstall-cleanup; {command}")
    }
}

/// **The package manager that installed this copy, and its uninstall line**, or
/// `None` for a copy that may remove itself — and while the channel is not yet
/// known (`install_channel::channel`), which offers the button: the uninstaller
/// asks the question again for itself, and leaves a managed copy to its manager
/// (T-UNINSTALL-UX). Read by the Settings rows.
pub(crate) fn managed_uninstall() -> Option<(Manager, String)> {
    match crate::install_channel::channel()? {
        crate::install_channel::Channel::Managed {
            manager,
            uninstall_hook,
        } => Some((manager, manager_uninstall(manager, uninstall_hook))),
        _ => None,
    }
}

/// **A package manager's own name**, as it writes it: a proper noun, in no
/// column of the table.
#[must_use]
pub(crate) const fn manager_name(manager: Manager) -> &'static str {
    match manager {
        Manager::Scoop => "Scoop",
        Manager::Homebrew => "Homebrew",
        Manager::Winget => "winget",
    }
}

/// **The program's own files, handed to the remover** (T-UNINSTALL-UX) — or,
/// for a copy a package manager installed, its command and nothing removed.
/// What is removed is derived from the running executable ([`program_plan`]);
/// `schedule` is `bt_platform::deferred_removal::schedule` in the product, and
/// the copied native remover lives in a random private directory below the
/// account's Folio local-data directory (the sandbox's in a test).
fn remove_the_program(
    worker: &WorkerCtx,
    scope: &Scope,
    channel: crate::install_channel::Channel,
    platform: HostPlatform,
    members: impl FnOnce(&Path) -> Result<Vec<bt_winres::release_manifest::Member>, String>,
    after: &[Running],
    schedule: impl FnOnce(&WorkerCtx, &Removal, &Path) -> io::Result<()>,
) -> Vec<Entry> {
    let label = program_label(scope.lang);
    if let crate::install_channel::Channel::Managed {
        manager,
        uninstall_hook,
    } = channel
    {
        return vec![Entry::new(
            label,
            Fate::Managed(manager_uninstall(manager, uninstall_hook)),
        )];
    }
    let plan = match program_plan(worker, &scope.exe, platform, members) {
        Ok(plan) => plan,
        Err(why) => return vec![Entry::new(label, Fate::Refused(why))],
    };
    let row = format!("{label}: {}", plan.root.display());
    let mut waited = after.to_vec();
    let program: &Path = &plan.program;
    match bt_platform::install_flip::running_from(program) {
        Ok(running) => {
            for process in running {
                if !waited.contains(&process) {
                    waited.push(process);
                }
            }
        }
        Err(error) => {
            return vec![Entry::new(label, Fate::Refused(Why::of(&error)))];
        }
    }
    let fallback_name = program
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("folio"))
        .to_os_string();
    let removal = Removal {
        program: plan.program,
        program_identity: plan.program_identity,
        after: waited
            .into_iter()
            .map(|process| WaitFor {
                process,
                name: bt_platform::install_flip::image_name(process)
                    .unwrap_or_else(|| fallback_name.clone()),
            })
            .collect(),
        items: plan.items,
        folder: plan.folder,
        words: FailureWords {
            title: crate::APP_NAME.to_owned(),
            still_running: Text::UninstallStillRunning.in_lang(scope.lang).to_owned(),
            files_left: Text::UninstallFilesLeft.in_lang(scope.lang).to_owned(),
            result_at: Text::UninstallResultAt.in_lang(scope.lang).to_owned(),
        },
    };
    let mut entries = vec![Entry::new(
        &row,
        match schedule(worker, &removal, &scope.remover_home) {
            Ok(()) => Fate::Scheduled,
            Err(error) => Fate::Refused(Why::of(&error)),
        },
    )];
    if !plan.not_ours.is_empty() {
        entries.push(Entry::new(row, Fate::LeftNotOurs(plan.not_ours)));
    }
    entries
}

/// **What the program is, on this machine**: the folder or bundle it runs
/// from, what in it is removed, the folder to remove afterwards if it is then
/// empty, and what stands in it that the build did not install.
#[derive(Debug, PartialEq, Eq)]
struct ProgramPlan {
    root: PathBuf,
    program: PathBuf,
    program_identity: FileIdentity,
    items: Vec<Item>,
    folder: Option<PathBuf>,
    not_ours: Vec<PathBuf>,
}

/// **The program's files, derived from the running executable and nothing
/// else** (T-UNINSTALL-UX, "the deleter refuses to remove anything but the
/// folder the running executable lives in").
///
/// The executable's path is resolved first, so the folder above what is
/// removed carries no link; then:
/// - **Windows**: the release manifest's members (`members`, the running
///   `folio.exe`'s own manifest in the product), the executable, the Explorer
///   package, the install marker and the update's installation home, each
///   only where it exists — and the folder itself, afterwards, only if empty.
///   Everything else in the folder is named as not Folio's and stays.
/// - **A macOS bundle**: the bundle, whole.
/// - **Anything else**: the executable.
///
/// Every item is looked at without following links, and a directory is walked
/// whole: a link or junction at the bundle/item name or below it refuses the
/// whole plan. Ancestors above that name are canonicalized; macOS itself has
/// linked ancestors in ordinary launch paths.
fn program_plan(
    worker: &WorkerCtx,
    exe: &Path,
    platform: HostPlatform,
    members: impl FnOnce(&Path) -> Result<Vec<bt_winres::release_manifest::Member>, String>,
) -> Result<ProgramPlan, Why> {
    let unresolved_root = crate::install_channel::install_root(exe, platform)
        .filter(|root| root.parent().is_some())
        .ok_or(Why::Said(Text::CleanupRoot))?;
    if platform == HostPlatform::MacOs && exe.parent() != Some(unresolved_root.as_path()) {
        refuse_bundle_chain(worker, &unresolved_root, exe)?;
    }
    let exe =
        bt_platform::handoff::strip_verbatim_prefix(&bt_platform::instance::canonical_path(exe));
    let program_identity = FileIdentity::of(&exe).map_err(|error| Why::of(&error))?;
    let root = crate::install_channel::install_root(&exe, platform)
        .filter(|root| root.parent().is_some())
        .ok_or(Why::Said(Text::CleanupRoot))?;
    if exe.parent() != Some(root.as_path()) {
        let items = tree_items(worker, &root)?;
        return Ok(ProgramPlan {
            items,
            root,
            program: exe,
            program_identity,
            folder: None,
            not_ours: Vec::new(),
        });
    }
    if platform != HostPlatform::Windows {
        return Ok(ProgramPlan {
            items: vec![Item::File {
                path: exe.clone(),
                expected: program_identity.clone(),
            }],
            root,
            program: exe,
            program_identity,
            folder: None,
            not_ours: Vec::new(),
        });
    }
    let members = members(&exe).map_err(Why::Other)?;
    let mut items = Vec::new();
    let mut not_ours = Vec::new();
    for member in members {
        let path = root.join(&member.name);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(Why::of(&error)),
            Ok(meta) if bt_platform::cleanup::is_link(&meta) => {
                return Err(Why::SaidOf(Text::CleanupLink, path.display().to_string()));
            }
            Ok(meta) if meta.is_file() => {
                let actual = FileIdentity::of(&path).map_err(|error| Why::of(&error))?;
                let expected = FileIdentity {
                    size: member.size,
                    sha256: member.sha256,
                };
                if actual == expected {
                    items.push(Item::File { path, expected });
                } else {
                    not_ours.push(path);
                }
            }
            Ok(_) => not_ours.push(path),
        }
    }
    items.push(Item::File {
        path: exe.clone(),
        expected: program_identity.clone(),
    });
    for name in [
        bt_platform::msix::PACKAGE_FILE_NAME,
        crate::install_channel::MARKER_FILE_NAME,
    ] {
        let path = root.join(name);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(Why::of(&error)),
            Ok(meta) if bt_platform::cleanup::is_link(&meta) => {
                return Err(Why::SaidOf(Text::CleanupLink, path.display().to_string()));
            }
            Ok(meta) if meta.is_file() => {
                let expected = FileIdentity::of(&path).map_err(|error| Why::of(&error))?;
                items.push(Item::File { path, expected });
            }
            Ok(_) => not_ours.push(path),
        }
    }
    let update_home = root.join(crate::update_txn::WINDOWS_HOME);
    match fs::symlink_metadata(&update_home) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(Why::of(&error)),
        Ok(meta) if bt_platform::cleanup::is_link(&meta) => {
            return Err(Why::SaidOf(
                Text::CleanupLink,
                update_home.display().to_string(),
            ));
        }
        Ok(meta) if meta.is_dir() => {
            items.extend(tree_items(worker, &update_home)?);
        }
        Ok(_) => not_ours.push(update_home),
    }
    let scheduled_names: Vec<OsString> = items
        .iter()
        .filter(|item| item.path().parent() == Some(root.as_path()))
        .filter_map(|item| item.path().file_name().map(std::ffi::OsStr::to_os_string))
        .collect();
    not_ours.extend(
        unowned_names(
            names_in(worker, &root).map_err(|error| Why::of(&error))?,
            &scheduled_names,
        )
        .into_iter()
        .map(|name| root.join(name)),
    );
    not_ours.sort();
    not_ours.dedup();
    Ok(ProgramPlan {
        folder: Some(root.clone()),
        root,
        program: exe,
        program_identity,
        items,
        not_ours,
    })
}

/// Names not scheduled, compared exactly as the directory returned them. On a
/// case-sensitive Windows directory `conpty.dll` and `CONPTY.DLL` are two
/// objects; the latter is neither Folio's file nor hidden from the report.
fn unowned_names(names: Vec<OsString>, scheduled: &[OsString]) -> Vec<OsString> {
    names
        .into_iter()
        .filter(|name| !scheduled.contains(name))
        .collect()
}

/// Refuse the unresolved `.app` name and every lexical component below it
/// through the launch executable before canonicalization can erase such a
/// symlink. Ancestors above the bundle name are canonicalized, not refused.
fn refuse_bundle_chain(worker: &WorkerCtx, bundle: &Path, exe: &Path) -> Result<(), Why> {
    let relative = exe
        .strip_prefix(bundle)
        .map_err(|_| Why::Said(Text::CleanupRoot))?;
    let mut path = bundle.to_path_buf();
    refuse_one_link(worker, &path)?;
    for component in relative.components() {
        path.push(component.as_os_str());
        refuse_one_link(worker, &path)?;
    }
    Ok(())
}

fn refuse_one_link(_worker: &WorkerCtx, path: &Path) -> Result<(), Why> {
    let metadata = fs::symlink_metadata(path).map_err(|error| Why::of(&error))?;
    if bt_platform::cleanup::is_link(&metadata) {
        return Err(Why::SaidOf(Text::CleanupLink, path.display().to_string()));
    }
    Ok(())
}

/// **Every name in `folder`**, on the door's worker (a worker's door: a
/// directory read).
fn names_in(_worker: &WorkerCtx, folder: &Path) -> io::Result<Vec<OsString>> {
    fs::read_dir(folder)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect()
}

/// **Identify every file in the tree at `path` and every directory to remove
/// after its files**, looked at without following links, on the door's worker.
/// Directories are deepest first and are removed only when empty, so a file
/// inserted after planning is never swept up by a recursive delete.
fn tree_items(_worker: &WorkerCtx, path: &Path) -> Result<Vec<Item>, Why> {
    let mut pending = vec![path.to_path_buf()];
    let mut files = Vec::new();
    let mut directories = Vec::new();
    while let Some(path) = pending.pop() {
        let meta = fs::symlink_metadata(&path).map_err(|error| Why::of(&error))?;
        if bt_platform::cleanup::is_link(&meta) {
            return Err(Why::SaidOf(Text::CleanupLink, path.display().to_string()));
        }
        if meta.is_file() {
            let expected = FileIdentity::of(&path).map_err(|error| Why::of(&error))?;
            files.push(Item::File { path, expected });
        } else if meta.is_dir() {
            directories.push(path.clone());
            for entry in fs::read_dir(&path).map_err(|error| Why::of(&error))? {
                pending.push(entry.map_err(|error| Why::of(&error))?.path());
            }
        } else {
            return Err(Why::Other(format!(
                "{} is not a regular file or directory",
                path.display()
            )));
        }
    }
    directories.sort_by(|left, right| {
        right
            .components()
            .count()
            .cmp(&left.components().count())
            .then_with(|| left.cmp(right))
    });
    files.extend(directories.into_iter().map(Item::Directory));
    Ok(files)
}

/// **The members of the release manifest the executable at `exe` carries** —
/// the text the updater reads (`update_archive::EmbeddedManifest`), so the
/// program's files are the files this build was installed as.
fn installed_members(exe: &Path) -> Result<Vec<bt_winres::release_manifest::Member>, String> {
    use crate::update_archive::ManifestSource;
    let text = crate::update_archive::EmbeddedManifest.manifest_text(exe)?;
    let manifest =
        bt_winres::release_manifest::Manifest::parse(&text).map_err(|error| error.to_string())?;
    Ok(manifest.members)
}

/// **Whether Folio's way out owes the door a start, and with what** — set when
/// *Uninstall* is pressed on the Settings card ([`arm`]), taken once at the
/// process's end ([`leave_armed`]), put back to nothing when the quit that was
/// asked for is abandoned ([`disarm`]).
static ARMED: Mutex<Option<bool>> = Mutex::new(None);

/// **Arm the way out** with the door's `--remove-data` answer.
pub(crate) fn arm(remove_data: bool) {
    if let Ok(mut armed) = ARMED.lock() {
        *armed = Some(remove_data);
    }
}

/// **Disarm the way out**: the quit asked for was abandoned, and this process
/// stays.
pub(crate) fn disarm() {
    if let Ok(mut armed) = ARMED.lock() {
        *armed = None;
    }
}

/// **The door's words for this process's way out**: `--uninstall`, then
/// `--remove-data` when asked, then `--after-pid <pid>`.
fn door_words(remove_data: bool, pid: u32) -> Vec<OsString> {
    let mut words = vec![OsString::from(crate::cli::UNINSTALL_FLAG)];
    if remove_data {
        words.push(OsString::from(crate::cli::REMOVE_DATA_FLAG));
    }
    words.push(OsString::from(crate::cli::AFTER_PID_FLAG));
    words.push(OsString::from(pid.to_string()));
    words
}

/// **The process's last act after *Uninstall* on the Settings card**: start its
/// own executable as the door with `--after-pid` naming itself, and return —
/// the door waits for this process to end before it touches anything. Nothing
/// when nothing was armed. A start that fails is said in the box a windowless
/// process can raise, by this process, before it leaves: nobody is uninstalled,
/// and the person who pressed the button has to be told.
pub(crate) fn leave_armed() {
    let Some(remove_data) = ARMED.lock().ok().and_then(|mut armed| armed.take()) else {
        return;
    };
    let started = std::env::current_exe().and_then(|exe| {
        bt_platform::quiet_breakaway_command(exe)
            .args(door_words(remove_data, std::process::id()))
            .spawn()
    });
    if let Err(error) = started {
        crate::diagnostics::note(&format!("Folio: the uninstaller did not start: {error}"));
        bt_platform::standalone_alert(
            crate::APP_NAME,
            &format!("{}\n{error}", Text::UninstallNotStarted.text()),
        );
    }
}

#[cfg(test)]
#[path = "uninstall_tests.rs"]
mod tests;
