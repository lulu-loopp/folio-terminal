//! **C1 as a gate: a request door returns at once** (T-REQUEST-DOOR-GUARD, S-2b of
//! `docs/plans/design/side-effects-contract-2026-10-11.md`, revision (b)2 and (c)2). A guard: its
//! subject is how the code is written.
//!
//! Each `# request-doors` row of `crates/bt-app/src/window_waits.tsv` asked on the window thread is
//! walked over `bt_source::call_graph` from its door to its boundary, and the walk fails on:
//!
//! * **(i) an effect**: a foreign callee that is a `# vocabulary` path, or under `std::fs`,
//!   `std::net`, `std::process`, `std::os::{windows, unix}::fs` or the `std::io` read and write
//!   traits and handles, a `Path`'s question to the disk (`is_file`, `exists`, `metadata`, …), or
//!   `std::env::set_current_dir` (C1 names the file system; ruling Q3), but never a method of a
//!   `Metadata`, `FileType` or `Permissions` value, which reads the value ([`PURE_FS_VALUES`]),
//!   nor of a collected child's `ExitStatus` or `Output` ([`PURE_PROCESS_VALUES`]); a
//!   function of the `windows` crate's file-system, registry and I/O modules, or a shell call that
//!   starts a process or opens a file (the coordinator's ruling extending Q3,
//!   [`WIN32_EFFECT_MODULES`], [`EFFECT_FUNCTIONS`]); a first-party `# vocabulary` entry reached;
//! * **(ii) an `# entrances` item** reached;
//! * **(iii) an `owner-door-body`** of `# effects` reached, and **(iii-b) a `worker-door-body`**
//!   reached (a worker body run inline), or a `handed` item called rather than named as a value;
//! * **(iv) a blocking primitive** ([`BLOCKING`]) whose caller is not one of the row's `mailbox`
//!   items; and a call of a `mailbox` item that takes the lock as an argument (a generic helper,
//!   `lane::lock(&Mutex<T>)`, which locks whatever its caller hands it) from an item that is not
//!   itself one of the row's `mailbox` items: the cell names the methods that lock, and a helper is
//!   exempt only through them;
//! * **(v) every unresolved edge** ([`Unresolved`]) except a macro of [`ALLOWED_MACROS`] and a
//!   callee a `derive` or `#[from]` generates (its body calls the derived trait's impls on the
//!   fields, which (vii) walks); `Cutoff` and `Unparsed` fail always and no list excuses them;
//! * **(vi) another ledger door reached** that runs on the window thread and does not conform (a
//!   `row <n>` or `D-<n>` status), or an `owner-thread` door (C7, listable per (b)6), or a door
//!   this guard does not walk (asked before the loop or in a door process: `NotWalked`, which a
//!   reach from the window thread contradicts);
//! * **a boundary entry never met**: the row names a call its walk does not reach.
//!
//! **(vii)** every first-party impl of a foreign trait (`Resolver::implicit`: a `Drop`, an operator,
//! a callback) is walked once as a root of its own under the union of every row's cells. Such an
//! impl may run on any door's road or on none, so what it meets is the impl's, not the site's: one
//! `Implicit` row per impl method, its `sites` the findings behind it; a row behind which is an
//! effect, a blocking primitive or a door-level finding takes only `violation`, as that site would.
//!
//! The walk follows every resolved edge, an `Ambiguous` edge with exactly one candidate (the door
//! reach's rule), and the items a reviewed row names (`follows`). A row's `boundary` names the calls
//! at which its request leaves the thread (an enqueue, a notify, the thread door, a first-party
//! item that hands a closure to a worker, or another request door), and for a call that takes the
//! worker's body, its argument (`lane::Executor::start_worker@2`, counted as
//! [`call_graph::Handed::argument`] counts): the walk does not enter a closure literal written at
//! that argument, which is the worker's; every other closure written at the call, which the
//! boundary item may run on the caller's thread (`admit_between`'s `between`, a refusal's voice),
//! and the call's own body are walked. `—` is a door that hands nothing over. A `handed`
//! item (a worker's entry named as a value) is a stop. It does not enter a `const`'s or a plain
//! `static`'s initializer, which the compiler evaluates (a function it names runs where that field
//! is called, a `Dynamic` edge), and does enter a `LazyLock`'s or a `OnceLock`'s, which runs on its
//! first use. Every other ledger door, an owner door's or a worker's body and an entrance are stops:
//! the finding names the breach, and what runs behind it is judged by its own row.
//!
//! **A cut door** (plan row 52, the owner's ruling of 2026-11-02): a `# request-door-cuts` row names
//! a gesture-level door whose road goes on past its enqueue — it answers, draws, closes a tab — and
//! cuts the walk one level down, at the door's own body. Its grammar, closed:
//!
//! * every **direct callee** of the door — an edge whose `from` is the door's own item, to a
//!   first-party item (resolved, or an `Ambiguous` edge with one candidate) — is named in
//!   `enqueue` (walked under C1 to the row's boundary, as a door is) or in `beside` (not entered),
//!   or is already a stop (another ledger door, a `handed` item, an owner's or a worker's body, an
//!   entrance, a door not walked);
//! * an `enqueue` item is called in the door's body itself, never only inside a closure literal;
//! * a site written in a closure literal handed to a `beside` call is `beside`'s, and one in a
//!   closure handed to an `enqueue` call is the enqueue's;
//! * every name in either cell is a direct callee of the door, the two cells share no item, and the
//!   row's `reason` is dated, `YYYY-MM-DD: why`.
//!
//! Every other shape is refused, naming the registry's line and the call's `file:line`. The door's
//! own body is judged as any door's; nothing a `beside` item calls is followed further (a hand-over
//! inside one stays under the row's reason). **The beside walk**: the `beside` side of the door is
//! walked once more, and what it meets stays on the list when it is a breach whatever is read (an
//! effect, a blocking primitive, a door-level finding) or a row already read; an `unreviewed`
//! unresolved edge met only beside a cut is counted and rendered (`beside.tsv`), not listed.
//!
//! A boundary entry on a foreign callee names no argument (`@N`) — the registry reads no foreign
//! signature — except the standard library's thread spawns of [`STD_SPAWNS`], whose closure
//! argument is the thread's body (ruling R6).
//!
//! A key of a row's cells names one item per `cfg` arm (`bt_platform::FolderPicker::request` is
//! one per platform's module); a key that names two items on one arm (a bare name two modules
//! declare) is refused, and so is a first-party `# vocabulary` or `# entrances` path naming nothing.
//!
//! Every failure is a row of `docs/plans/REQUEST-DOOR-SITES.tsv`, keyed by site (`file`, `item`,
//! `kind`, `name`; no line), with its count, its verdict and its reason: `std` and `follows` (read,
//! nothing behind it), `unreviewed` (counted, the list shrinks) or `violation` (a C1 breach, its
//! finding named). The guard holds the list equal to the tree; `scripts/ci/check-request-door-sites.ps1`
//! holds it to the merge base.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_source::call_graph::{
    Arms, CallGraph, CallSite, ItemId, Node, Resolver, Target, Unresolved, Via,
};
use bt_source::{
    DiskScope, Index, Span, TargetId, TargetKind, TargetRoot, Universe, Vendor, Workspace,
    is_vendored, report, universes,
};

// ── the rules ─────────────────────────────────────────────────────────────

/// The macros whose expansion is pure, or the panic road A§11 rules (ruling Q6: the print, write
/// and `dbg!` family is not here — in a resident run the standard handles are `diagnostics.log` or
/// a console pipe).
const ALLOWED_MACROS: &[&str] = &[
    "format",
    "format_args",
    "concat",
    "stringify",
    "vec",
    "matches",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "unreachable",
    "panic",
    "todo",
    "unimplemented",
    "line",
    "column",
    "file",
    "module_path",
    "cfg",
    "env",
    "option_env",
    "include_str",
    "include_bytes",
];

/// The blocking synchronisation primitives, by full path.
const BLOCKING: &[&str] = &[
    "std::sync::Mutex::lock",
    "std::sync::RwLock::read",
    "std::sync::RwLock::write",
    "std::sync::Condvar::wait",
    "std::sync::Condvar::wait_timeout",
    "std::sync::Condvar::wait_while",
    "std::sync::Condvar::wait_timeout_while",
    "std::sync::mpsc::Receiver::recv",
    "std::sync::mpsc::Receiver::recv_timeout",
    "std::sync::mpsc::Receiver::iter",
    "std::sync::mpsc::SyncSender::send",
    "std::sync::Barrier::wait",
    "std::thread::JoinHandle::join",
    "std::thread::ScopedJoinHandle::join",
    "std::thread::scope",
    "std::thread::park",
    "std::thread::park_timeout",
    "std::thread::sleep",
    "std::sync::Once::call_once",
    "std::sync::Once::call_once_force",
    "windows::Win32::System::Threading::WaitForSingleObject",
    "windows::Win32::System::Threading::WaitForSingleObjectEx",
    "windows::Win32::System::Threading::WaitForMultipleObjects",
    "windows::Win32::System::Threading::WaitForMultipleObjectsEx",
    "windows::Win32::System::Threading::SignalObjectAndWait",
    "windows::Win32::System::Threading::SleepEx",
];

/// The [`BLOCKING`] primitives that are not window-thread waits, each with why: every other
/// `BLOCKING` path is a `# vocabulary` path of `crates/bt-app/src/window_waits.tsv`, and
/// [`every_blocking_primitive_is_a_window_wait_or_named_as_not_one`] holds the two lists so
/// (T-BARE-SITE-VOCABULARY-WAIT-WHILE-BLIND-SPOT, plan row 176, ruling R2).
const NOT_A_WINDOW_WAIT: &[(&str, &str)] = &[
    (
        "std::sync::Mutex::lock",
        "a lock is C1's concern, exempt in a row's mailbox; the window thread takes hundreds, and \
         the vocabulary becomes the product's `disallowed-methods` at A2e",
    ),
    ("std::sync::RwLock::read", "a lock, as `Mutex::lock`"),
    ("std::sync::RwLock::write", "a lock, as `Mutex::lock`"),
    (
        "std::sync::mpsc::Receiver::iter",
        "bt-app's name-based `Drop` reading would take every collection's `.iter()` for it",
    ),
];

/// The namespaces C1 names whole (ruling Q3): the file system, the network, a child process, and
/// the standard I/O traits and handles.
const EFFECT_NAMESPACES: &[&str] = &[
    "std::fs::",
    "std::net::",
    "std::process::",
    "std::io::Read::",
    "std::io::Write::",
    "std::io::BufRead::",
    "std::io::Seek::",
    "std::io::Stdin::",
    "std::io::Stdout::",
    "std::io::Stderr::",
    "std::os::windows::fs::",
    "std::os::unix::fs::",
];

/// Single functions C1 names: the file-system questions a path answers by asking the disk
/// (`Path::is_file` is `fs::metadata`), the process's working directory, and the shell calls that
/// start a process or open a file.
const EFFECT_FUNCTIONS: &[&str] = &[
    "std::path::Path::exists",
    "std::path::Path::try_exists",
    "std::path::Path::is_file",
    "std::path::Path::is_dir",
    "std::path::Path::is_symlink",
    "std::path::Path::metadata",
    "std::path::Path::symlink_metadata",
    "std::path::Path::read_dir",
    "std::path::Path::read_link",
    "std::path::Path::canonicalize",
    "std::path::PathBuf::exists",
    "std::path::PathBuf::try_exists",
    "std::path::PathBuf::is_file",
    "std::path::PathBuf::is_dir",
    "std::path::PathBuf::is_symlink",
    "std::path::PathBuf::metadata",
    "std::path::PathBuf::symlink_metadata",
    "std::path::PathBuf::read_dir",
    "std::path::PathBuf::read_link",
    "std::path::PathBuf::canonicalize",
    "std::env::set_current_dir",
    "windows::Win32::UI::Shell::ShellExecuteW",
    "windows::Win32::UI::Shell::ShellExecuteExW",
    "windows::Win32::UI::Shell::SHCreateStreamOnFileEx",
];

/// **The standard file-system values whose methods read only the value** (ruling D2 of
/// T-UNTYPED-RECEIVER-FS-CALLS-TRIAGE, 2026-10-26): a `Metadata`, a `FileType` or a `Permissions`
/// is what a disk question already answered, and every method of it (and of the platform
/// extension traits that read it: `MetadataExt`, `FileTypeExt`, `PermissionsExt`) reads or sets
/// that value, never the disk. Under [`EFFECT_NAMESPACES`]' `std::fs::` and
/// `std::os::{windows, unix}::fs::` they are no effect; the question that made the value
/// (`std::fs::metadata`, `File::metadata`, `DirEntry::file_type`, …) still is.
const PURE_FS_VALUES: &[&str] = &[
    "std::fs::Metadata::",
    "std::fs::FileType::",
    "std::fs::Permissions::",
    "std::os::windows::fs::MetadataExt::",
    "std::os::windows::fs::FileTypeExt::",
    "std::os::unix::fs::MetadataExt::",
    "std::os::unix::fs::FileTypeExt::",
    "std::os::unix::fs::PermissionsExt::",
];

/// **The standard process values whose methods read only the value** (row 116,
/// T-EFFECT-GUARD-EXITSTATUS-READ, 2026-10-31, ruled as D2 above): an `ExitStatus` or an `Output`
/// is what a wait already collected, and every method of it reads or builds that value, never the
/// child or the OS: `code`, `success`, `exit_ok`, the platform `ExitStatusExt` methods (`signal`,
/// `core_dumped`, `stopped_signal`, `continued`, `into_raw`, `from_raw`, which the call graph writes
/// on the type, `std::process::ExitStatus::from_raw`), and `Output`'s derived traits; its fields
/// are no calls. Under [`EFFECT_NAMESPACES`]' `std::process::` they are no effect; the wait that
/// collected the value (`Child::wait`, `Child::try_wait`, `Child::wait_with_output`,
/// `Command::output`, `Command::status`) still is, as is every other `std::process` callee.
/// `ExitStatusError` and `ExitCode` are out of this rule's scope and stay effects (the prefixes end
/// in `::`, so neither matches).
const PURE_PROCESS_VALUES: &[&str] = &["std::process::ExitStatus::", "std::process::Output::"];

/// The `windows` crate's modules of the file system, the registry and file I/O (the coordinator's
/// ruling extending Q3, the modules the tree uses): a function written directly in one
/// (`…::FileSystem::CreateFileW`, `…::Registry::RegSetValueExW`) is an effect. Its types and
/// constants are the SDK's capitals (`FILE_SHARE_MODE`, `HKEY`), and a type's own method
/// (`HKEY::default`) is not the module's function: neither is.
const WIN32_EFFECT_MODULES: &[&str] = &[
    "windows::Win32::Storage::FileSystem",
    "windows::Win32::System::Registry",
    "windows::Win32::System::IO",
];

/// **The standard library's thread spawns whose closure argument a boundary entry may name**
/// (`std::thread::Builder::spawn@0`; plan row 52, ruling R6, 2026-11-02), the argument counted as
/// [`call_graph::Handed::argument`] counts (a method call's receiver is not). A closed table: a
/// foreign callee's signature is read nowhere else, so any other foreign `@N` is refused.
const STD_SPAWNS: &[(&str, usize)] = &[
    ("std::thread::spawn", 0),
    ("std::thread::Builder::spawn", 0),
];

/// A list row's key: the site's file, the item (or value) it is written in, the kind and the name.
type SiteKey = (String, String, String, String);

/// A reviewed row's verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Verdict {
    /// Read: a standard or third-party callee that waits on nothing.
    Std,
    /// Read: the callee is these first-party items, which the walk then follows.
    Follows(Vec<String>),
    /// Not read yet: counted, and the list only shrinks.
    Unreviewed,
    /// A C1 breach, its finding named in the reason.
    Violation,
}

impl Verdict {
    fn read(cell: &str) -> Result<Self, String> {
        match cell {
            "std" => Ok(Self::Std),
            "unreviewed" => Ok(Self::Unreviewed),
            "violation" => Ok(Self::Violation),
            _ => match cell.strip_prefix("follows ") {
                Some(keys) if !keys.trim().is_empty() => {
                    Ok(Self::Follows(keys.split("; ").map(str::to_owned).collect()))
                }
                _ => Err(format!(
                    "`{cell}` is not `std`, `follows <key>; …`, `unreviewed` or `violation`"
                )),
            },
        }
    }

    fn spelled(&self) -> String {
        match self {
            Self::Std => "std".to_owned(),
            Self::Follows(keys) => format!("follows {}", keys.join("; ")),
            Self::Unreviewed => "unreviewed".to_owned(),
            Self::Violation => "violation".to_owned(),
        }
    }
}

/// The kinds a door-level row carries: no site behind them, only `violation` is their verdict.
const DOOR_KINDS: &[&str] = &[
    "OwnerDoor",
    "WorkerBody",
    "Entrance",
    "Inherits",
    "Unmet",
    "HandedCalled",
    "NotWalked",
];

/// Whether a row of `kind` is a breach whatever is read: an effect, a blocking primitive, a
/// door-level finding.
fn is_strict(kind: &str) -> bool {
    DOOR_KINDS.contains(&kind) || kind == "Effect" || kind == "Blocking"
}

/// How another ledger door stands when a walk reaches it.
#[derive(Clone, Debug)]
enum Standing {
    /// A request door, or a window door that conforms: judged by its own row.
    PassThrough,
    /// An `owner-thread` row: a C7 member.
    OwnerThread,
    /// A window-thread door whose status names its repair.
    Repair(String),
}

/// What a walk is judged against: the registry's sections as items and paths.
#[derive(Default)]
struct Rules {
    /// Foreign `# vocabulary` paths.
    vocabulary: BTreeSet<String>,
    /// First-party `# vocabulary` entries and `# entrances`, by item, with their label.
    entrances: BTreeMap<ItemId, String>,
    owner_bodies: BTreeMap<ItemId, String>,
    worker_bodies: BTreeMap<ItemId, String>,
    /// Every ledger door's items, with its key and standing.
    doors: BTreeMap<ItemId, (String, Standing)>,
    /// The doors this guard does not walk (asked before the loop, or in a door process), by item,
    /// with their label: a walked door that reaches one contradicts that row's `asked on`.
    not_walked: BTreeMap<ItemId, String>,
}

/// One boundary entry of a row: the call, and the argument that is the worker's body.
#[derive(Clone, Debug)]
struct Entry {
    /// The cell's spelling, `@N` included.
    spelled: String,
    callee: Callee,
    /// The argument whose closure literal is the worker's body (`key@N`), counted as
    /// [`call_graph::Handed::argument`] counts; `None` for a call that takes none.
    worker: Option<usize>,
}

/// What a boundary entry calls.
#[derive(Clone, Debug)]
enum Callee {
    /// A foreign callee, by full path.
    Foreign(String),
    /// A first-party call, by its items.
    Items(BTreeSet<ItemId>),
}

/// One walked door: its roots and its row's cells.
struct DoorSpec {
    key: String,
    roots: Vec<ItemId>,
    boundary: Vec<Entry>,
    handed: BTreeSet<ItemId>,
    mailbox: BTreeSet<ItemId>,
    /// The `mailbox` items that take the lock as an argument: exempt only through a `mailbox` item.
    lock_helpers: BTreeSet<ItemId>,
    /// The door's `# request-door-cuts` row, if it is cut at its enqueue.
    cut: Option<Cut>,
}

/// **A `# request-door-cuts` row** (plan row 52): where a gesture door's walk is cut.
#[derive(Clone, Debug)]
struct Cut {
    /// The row's one-based line in the registry, for a refusal.
    line: usize,
    /// Each cell's keys with the items each names, in the cell's order.
    enqueue: Vec<(String, BTreeSet<ItemId>)>,
    beside: Vec<(String, BTreeSet<ItemId>)>,
}

impl Cut {
    fn enqueues(&self, id: ItemId) -> bool {
        self.enqueue.iter().any(|(_, items)| items.contains(&id))
    }

    fn besides(&self, id: ItemId) -> bool {
        self.beside.iter().any(|(_, items)| items.contains(&id))
    }
}

/// Which side of a cut a walk takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    /// A door with no cut: the whole of its road.
    Whole,
    /// A cut door's C1 walk: its own body and its `enqueue` items, to the boundary.
    Enqueue,
    /// A cut door's beside walk: its `beside` items and the closures handed to them.
    Beside,
}

/// A beside walk's findings (plan row 52), split into those the list keeps — a breach whatever is
/// read ([`is_strict`]), or a row `reviewed` has already read — and the unread rest, which leaves.
fn split_beside(
    findings: Vec<Finding>,
    reviewed: &BTreeMap<SiteKey, (Verdict, String)>,
) -> (Vec<Finding>, Vec<Finding>) {
    findings.into_iter().partition(|finding| {
        is_strict(&finding.key.2)
            || reviewed
                .get(&finding.key)
                .is_some_and(|(verdict, _)| *verdict != Verdict::Unreviewed)
    })
}

/// Whether `site` is written in a closure literal handed to one of `calls` (by their sites).
fn handed_to_one_of(site: &CallSite, calls: &BTreeSet<Span>) -> bool {
    site.closures
        .iter()
        .any(|open| open.handed.is_some_and(|handed| calls.contains(&handed.call)))
}

/// The first-party item an edge calls: a resolved item, or an `Ambiguous` edge's one candidate.
fn first_party(target: &Target) -> Option<ItemId> {
    match target {
        Target::Item(id) => Some(*id),
        Target::Unresolved(unresolved) => unresolved.sole_candidate(),
        _ => None,
    }
}

/// A failure the walk met, keyed for the list.
#[derive(Clone, Debug)]
struct Finding {
    key: SiteKey,
    /// What makes two findings of one key two sites: the node and the call's span.
    site: Option<(Node, Span)>,
    /// The walk's path to it, keys.
    path: Vec<String>,
    /// The root the path starts at.
    root: Node,
    /// For an implicit impl's row, the finding it stands for: with `site`, what makes two of its
    /// findings two.
    under: Option<SiteKey>,
}

/// What one walk met.
#[derive(Default)]
struct Judged {
    reached: BTreeSet<ItemId>,
    values: usize,
    foreign: usize,
    unresolved: BTreeMap<&'static str, usize>,
    findings: Vec<Finding>,
    /// `Cutoff` and `Unparsed`: never listable.
    refused: Vec<String>,
}

fn kind_of(unresolved: &Unresolved) -> &'static str {
    match unresolved {
        Unresolved::Dynamic { .. } => "Dynamic",
        Unresolved::Ambiguous { .. } => "Ambiguous",
        Unresolved::Macro { .. } => "Macro",
        Unresolved::OtherArm { .. } => "OtherArm",
        Unresolved::Generated { .. } => "Generated",
        Unresolved::Path { .. } => "Path",
        Unresolved::Cutoff { .. } => "Cutoff",
        Unresolved::Unparsed { .. } => "Unparsed",
    }
}

/// The macro's own name: its path's last segment.
fn macro_name(name: &str) -> &str {
    name.rsplit("::")
        .next()
        .unwrap_or(name)
        .trim_end_matches('!')
}

/// Whether a generated callee is one (vii) covers: a `derive` or a `#[from]`.
fn generated_is_covered(by: &str) -> bool {
    by.starts_with("#[derive(") || by == "#[from]"
}

fn is_effect(rules: &Rules, path: &str) -> bool {
    rules.vocabulary.contains(path)
        || EFFECT_FUNCTIONS.contains(&path)
        || (EFFECT_NAMESPACES
            .iter()
            .any(|namespace| path.starts_with(namespace))
            && !PURE_FS_VALUES
                .iter()
                .chain(PURE_PROCESS_VALUES)
                .any(|value| path.starts_with(value)))
        || path.rsplit_once("::").is_some_and(|(module, name)| {
            WIN32_EFFECT_MODULES.contains(&module) && name.chars().any(|c| c.is_ascii_lowercase())
        })
}

/// The workspace-relative file a node's site is written in.
fn file_of(resolver: &Resolver<'_>, node: Node, at: usize, root: &Path) -> String {
    resolver
        .node_index(node)
        .file_at(at)
        .map(|file| {
            let path = file
                .path()
                .canonicalize()
                .unwrap_or_else(|_| file.path().to_path_buf());
            path.strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .unwrap_or_default()
}

/// The walk, judged.
struct Judge<'g, 'r, 'i> {
    graph: &'g CallGraph<'r, 'i>,
    rules: &'g Rules,
    /// The items each reviewed `follows` row names ([`followed`]), which the walk then follows.
    follows: &'g BTreeMap<SiteKey, Vec<ItemId>>,
    root: PathBuf,
}

impl Judge<'_, '_, '_> {
    fn resolver(&self) -> &Resolver<'_> {
        self.graph.resolver()
    }

    fn item_file(&self, id: ItemId) -> String {
        let record = self.resolver().record(id);
        file_of(
            self.resolver(),
            Node::Item(id),
            record.whole().start(),
            &self.root,
        )
    }

    /// The walk from `spec`'s roots — for a cut door, its C1 side ([`Side::Enqueue`]). A
    /// door-level finding names its owner: the door, or for the implicit walk (`implicit`), the
    /// root the path starts at.
    fn walk(&self, spec: &DoorSpec, implicit: bool) -> Judged {
        let side = if spec.cut.is_some() {
            Side::Enqueue
        } else {
            Side::Whole
        };
        self.walk_on(spec, implicit, side)
    }

    /// Whether `id` is a stop of `spec`'s walk whatever a cut says: another ledger door, a
    /// `handed` item, an owner's or a worker's body, an entrance, a door not walked.
    fn is_a_stop(&self, spec: &DoorSpec, id: ItemId) -> bool {
        spec.handed.contains(&id)
            || self.rules.doors.contains_key(&id)
            || self.rules.not_walked.contains_key(&id)
            || self.rules.owner_bodies.contains_key(&id)
            || self.rules.worker_bodies.contains_key(&id)
            || self.rules.entrances.contains_key(&id)
    }

    /// Where a site is written, as `file:line`.
    fn place(&self, node: Node, at: usize) -> String {
        let line = self
            .resolver()
            .node_index(node)
            .locate(at)
            .map_or(0, |location| location.line);
        format!("{}:{line}", file_of(self.resolver(), node, at, &self.root))
    }

    /// The walk from `spec`'s roots on `side` of its cut ([`Side::Whole`] for a door with none).
    fn walk_on(&self, spec: &DoorSpec, implicit: bool, side: Side) -> Judged {
        let resolver = self.resolver();
        let mut judged = Judged::default();
        let mut parent: BTreeMap<Node, Option<Node>> = BTreeMap::new();
        let mut queue: VecDeque<Node> = VecDeque::new();
        let roots: BTreeSet<ItemId> = spec.roots.iter().copied().collect();
        for root in &spec.roots {
            if parent.insert(Node::Item(*root), None).is_none() {
                queue.push_back(Node::Item(*root));
            }
        }
        let mut met: BTreeSet<usize> = BTreeSet::new();
        // The first-party items a cut door's own body calls, for the stale names of its row.
        let mut cut_met: BTreeSet<ItemId> = BTreeSet::new();
        let path_to = |parent: &BTreeMap<Node, Option<Node>>, node: Node| -> Vec<Node> {
            let mut path = vec![node];
            let mut at = node;
            while let Some(Some(up)) = parent.get(&at) {
                path.push(*up);
                at = *up;
            }
            path.reverse();
            path
        };
        let spell = |path: &[Node]| -> Vec<String> {
            path.iter().map(|node| resolver.node_key(*node)).collect()
        };
        let owner_of = |path: &[Node]| -> String {
            if implicit {
                path.first()
                    .map_or_else(String::new, |node| resolver.node_key(*node))
            } else {
                spec.key.clone()
            }
        };
        while let Some(node) = queue.pop_front() {
            let path = path_to(&parent, node);
            match node {
                Node::Item(id) => {
                    judged.reached.insert(id);
                    let door_level = |kind: &str, name: String| Finding {
                        key: (self.item_file(id), owner_of(&path), kind.to_owned(), name),
                        site: None,
                        path: spell(&path),
                        root: path[0],
                        under: None,
                    };
                    // Each is a stop: the finding names the breach, and what runs behind it is
                    // that door's, judged by its own row.
                    let mut stop = false;
                    for (kind, label) in [
                        ("OwnerDoor", self.rules.owner_bodies.get(&id)),
                        ("WorkerBody", self.rules.worker_bodies.get(&id)),
                        ("Entrance", self.rules.entrances.get(&id)),
                    ] {
                        if let Some(label) = label {
                            judged.findings.push(door_level(kind, label.clone()));
                            stop = true;
                        }
                    }
                    if stop && !roots.contains(&id) {
                        continue;
                    }
                }
                Node::Value(_) => judged.values += 1,
            }
            let edges = self.graph.edges(node);
            // A cut door's own body (plan row 52): the calls of its `beside` and `enqueue` items,
            // by site, so that a closure handed to one is read as that side's.
            let cut = spec
                .cut
                .as_ref()
                .filter(|_| matches!(node, Node::Item(id) if roots.contains(&id)));
            let calls_of = |named: &dyn Fn(ItemId) -> bool| -> BTreeSet<Span> {
                edges
                    .iter()
                    .filter(|edge| first_party(&edge.to).is_some_and(named))
                    .map(|edge| edge.site.span)
                    .collect()
            };
            let (beside_calls, enqueue_calls) = cut.map_or_else(Default::default, |cut| {
                (
                    calls_of(&|id| cut.besides(id)),
                    calls_of(&|id| cut.enqueues(id)),
                )
            });
            // The boundary calls met at this node, by site, with the arguments whose closure
            // literal is the worker's body: that closure is not walked; every other closure written
            // at the call runs where the boundary item runs it, and is.
            let mut handed_to: BTreeMap<Span, BTreeSet<usize>> = BTreeMap::new();
            let mut next: Vec<ItemId> = Vec::new();
            let mut next_values = Vec::new();
            for edge in edges.iter() {
                if edge.site.closures.iter().any(|open| {
                    open.handed.is_some_and(|handed| {
                        handed_to
                            .get(&handed.call)
                            .is_some_and(|workers| workers.contains(&handed.argument))
                    })
                }) {
                    continue;
                }
                if let Some(cut) = cut {
                    let callee = first_party(&edge.to);
                    if let Some(id) = callee {
                        cut_met.insert(id);
                    }
                    let enqueue = callee.filter(|id| cut.enqueues(*id));
                    if let Some(id) = enqueue
                        && side == Side::Enqueue
                        && !edge.site.closures.is_empty()
                    {
                        judged.refused.push(format!(
                            "window_waits.tsv:{}: `{}`'s enqueue `{}` is called at {} inside a \
                             closure literal: an enqueue is a call the door's own body makes",
                            cut.line,
                            spec.key,
                            resolver.key(id),
                            self.place(edge.from, edge.site.span.start())
                        ));
                        continue;
                    }
                    let beside = callee.is_some_and(|id| cut.besides(id))
                        || handed_to_one_of(&edge.site, &beside_calls);
                    match side {
                        Side::Beside if !beside => continue,
                        Side::Enqueue if beside => continue,
                        Side::Enqueue => {
                            if let Some(id) = callee
                                && enqueue.is_none()
                                && !roots.contains(&id)
                            {
                                if !handed_to_one_of(&edge.site, &enqueue_calls)
                                    && !self.is_a_stop(spec, id)
                                {
                                    judged.refused.push(format!(
                                        "window_waits.tsv:{}: `{}` calls `{}` at {}, which its \
                                         `# request-door-cuts` row names in neither `enqueue` nor \
                                         `beside`",
                                        cut.line,
                                        spec.key,
                                        resolver.key(id),
                                        self.place(edge.from, edge.site.span.start())
                                    ));
                                }
                            }
                        }
                        Side::Beside | Side::Whole => {}
                    }
                }
                for (at, entry) in spec.boundary.iter().enumerate() {
                    let hit = match (&entry.callee, &edge.to) {
                        (Callee::Foreign(path), Target::Foreign(callee)) => {
                            callee.join("::") == *path
                        }
                        (Callee::Items(items), Target::Item(id)) => items.contains(id),
                        _ => false,
                    };
                    if hit {
                        met.insert(at);
                        handed_to
                            .entry(edge.site.span)
                            .or_default()
                            .extend(entry.worker);
                    }
                }
                let site_key = |kind: &str, name: &str| -> SiteKey {
                    (
                        file_of(resolver, edge.from, edge.site.span.start(), &self.root),
                        resolver.node_key(edge.from),
                        kind.to_owned(),
                        name.to_owned(),
                    )
                };
                let finding = |kind: &str, name: &str| Finding {
                    key: site_key(kind, name),
                    site: Some((edge.from, edge.site.span)),
                    path: spell(&path),
                    root: path[0],
                    under: None,
                };
                match &edge.to {
                    Target::Item(id) => {
                        if roots.contains(id) {
                            continue;
                        }
                        if spec.handed.contains(id) {
                            if edge.via != Via::Value {
                                judged.findings.push(Finding {
                                    key: (
                                        self.item_file(*id),
                                        owner_of(&path),
                                        "HandedCalled".to_owned(),
                                        resolver.key(*id),
                                    ),
                                    site: None,
                                    path: spell(&path),
                                    root: path[0],
                                    under: None,
                                });
                            }
                            continue;
                        }
                        if let Some(door) = self.rules.not_walked.get(id) {
                            judged.findings.push(Finding {
                                key: (
                                    self.item_file(*id),
                                    owner_of(&path),
                                    "NotWalked".to_owned(),
                                    door.clone(),
                                ),
                                site: None,
                                path: spell(&path),
                                root: path[0],
                                under: None,
                            });
                            continue;
                        }
                        // A helper that locks what it is handed is exempt only where a declared
                        // mailbox item calls it: the cell cannot say which mutex another caller hands.
                        if spec.lock_helpers.contains(id)
                            && !matches!(edge.from, Node::Item(from) if spec.mailbox.contains(&from))
                        {
                            judged
                                .findings
                                .push(finding("Blocking", &resolver.key(*id)));
                        }
                        if let Some((door, standing)) = self.rules.doors.get(id) {
                            let kind = match standing {
                                Standing::PassThrough => None,
                                Standing::OwnerThread => Some(("OwnerDoor", door.clone())),
                                Standing::Repair(status) => {
                                    Some(("Inherits", format!("{door} ({status})")))
                                }
                            };
                            if let Some((kind, name)) = kind {
                                judged.findings.push(Finding {
                                    key: (
                                        self.item_file(*id),
                                        owner_of(&path),
                                        kind.to_owned(),
                                        name,
                                    ),
                                    site: None,
                                    path: spell(&path),
                                    root: path[0],
                                    under: None,
                                });
                            }
                            continue;
                        }
                        next.push(*id);
                    }
                    // A `LazyLock`'s or `OnceLock`'s initializer runs on its first use, perhaps here. A `const`'s
                    // or a plain `static`'s is evaluated by the compiler: nothing in it runs on
                    // this thread, and a function it names runs only where that field is called —
                    // a `Dynamic` edge, which the list reads.
                    Target::Value(id) if edge.via == Via::Deref => next_values.push(*id),
                    Target::Value(_) => {}
                    Target::Foreign(callee) => {
                        judged.foreign += 1;
                        let joined = callee.join("::");
                        if is_effect(self.rules, &joined) {
                            judged.findings.push(finding("Effect", &joined));
                        }
                        let in_mailbox =
                            matches!(edge.from, Node::Item(from) if spec.mailbox.contains(&from));
                        if BLOCKING.contains(&joined.as_str()) && !in_mailbox {
                            judged.findings.push(finding("Blocking", &joined));
                        }
                    }
                    Target::Unresolved(unresolved) => {
                        let kind = kind_of(unresolved);
                        *judged.unresolved.entry(kind).or_default() += 1;
                        match unresolved {
                            Unresolved::Cutoff { what } => {
                                judged.refused.push(format!(
                                    "a cutoff at `{}` in {}: {what}",
                                    edge.site.name,
                                    resolver.node_key(edge.from)
                                ));
                                continue;
                            }
                            Unresolved::Unparsed { why } => {
                                judged.refused.push(format!(
                                    "{} does not parse: {why}",
                                    resolver.node_key(edge.from)
                                ));
                                continue;
                            }
                            Unresolved::Macro { name }
                                if ALLOWED_MACROS.contains(&macro_name(name)) =>
                            {
                                continue;
                            }
                            Unresolved::Generated { by } if generated_is_covered(by) => continue,
                            Unresolved::Ambiguous { candidates, .. } if candidates.len() == 1 => {
                                next.push(candidates[0]);
                            }
                            _ => {}
                        }
                        let name = match unresolved {
                            Unresolved::Macro { name } => macro_name(name).to_owned(),
                            _ => edge.site.name.clone(),
                        };
                        let found = finding(kind, &name);
                        if let Some(items) = self.follows.get(&found.key) {
                            next.extend(items.iter().copied());
                        }
                        judged.findings.push(found);
                    }
                    Target::Constructor | Target::Closure(_) | Target::Nested(_) => {}
                }
            }
            for id in next {
                if let std::collections::btree_map::Entry::Vacant(slot) =
                    parent.entry(Node::Item(id))
                {
                    slot.insert(Some(node));
                    queue.push_back(Node::Item(id));
                }
            }
            for id in next_values {
                if let std::collections::btree_map::Entry::Vacant(slot) =
                    parent.entry(Node::Value(id))
                {
                    slot.insert(Some(node));
                    queue.push_back(Node::Value(id));
                }
            }
        }
        if let (Some(cut), Side::Enqueue) = (&spec.cut, side) {
            for (cell, named) in [("enqueue", &cut.enqueue), ("beside", &cut.beside)] {
                for (key, items) in named {
                    if items.is_disjoint(&cut_met) {
                        judged.refused.push(format!(
                            "window_waits.tsv:{}: `{}`'s `{cell}` names `{key}`, which the door's \
                             own body does not call",
                            cut.line, spec.key
                        ));
                    }
                }
            }
        }
        // The beside walk enters no enqueue: its boundary is the C1 walk's to meet.
        let owed = if side == Side::Beside {
            &[][..]
        } else {
            &spec.boundary[..]
        };
        for (at, entry) in owed.iter().enumerate() {
            if !met.contains(&at) {
                judged.findings.push(Finding {
                    key: (
                        spec.roots
                            .first()
                            .map(|id| self.item_file(*id))
                            .unwrap_or_default(),
                        spec.key.clone(),
                        "Unmet".to_owned(),
                        entry.spelled.clone(),
                    ),
                    site: None,
                    path: Vec::new(),
                    root: Node::Item(spec.roots[0]),
                    under: None,
                });
            }
        }
        judged
    }
}

/// Every finding of `judged`, by key, counted by site (a door-level key counts once).
fn count(findings: &[Finding]) -> BTreeMap<SiteKey, BTreeSet<Site>> {
    let mut by_key: BTreeMap<SiteKey, BTreeSet<Site>> = BTreeMap::new();
    for finding in findings {
        by_key
            .entry(finding.key.clone())
            .or_default()
            .insert((finding.site, finding.under.clone()));
    }
    by_key
}

/// What makes two findings of one key two sites: the call's node and span, and for an implicit
/// impl's row the finding it stands for.
type Site = (Option<(Node, Span)>, Option<SiteKey>);

/// What the list says against what the tree has: every difference, and every row whose verdict its
/// kind does not admit — an `Implicit` row by the kinds behind it (`under`).
fn list_failures(
    tree: &BTreeMap<SiteKey, usize>,
    list: &ListRows,
    doors: &BTreeMap<SiteKey, BTreeSet<String>>,
    under: &BTreeMap<SiteKey, BTreeSet<String>>,
) -> Vec<String> {
    let met = |key: &SiteKey| {
        doors.get(key).map_or_else(String::new, |doors| {
            if doors.is_empty() {
                ", met walking the implicit impls".to_owned()
            } else {
                format!(
                    ", met walking {}",
                    doors.iter().cloned().collect::<Vec<_>>().join(", ")
                )
            }
        })
    };
    let mut failures = Vec::new();
    for (key, sites) in tree {
        match list.get(key) {
            None => failures.push(format!(
                "in the tree, not on the list: {key:?} ({sites}){}",
                met(key)
            )),
            Some((listed, ..)) if listed != sites => {
                failures.push(format!(
                    "{key:?}: {sites} in the tree, {listed} on the list{}",
                    met(key)
                ));
            }
            Some(_) => {}
        }
    }
    for (key, (_, verdict, _)) in list {
        if !tree.contains_key(key) {
            failures.push(format!("on the list, not in the tree: {key:?}"));
        }
        let behind = under
            .get(key)
            .into_iter()
            .flatten()
            .find(|kind| is_strict(kind));
        if (is_strict(&key.2) || (key.2 == "Implicit" && behind.is_some()))
            && *verdict != Verdict::Violation
        {
            failures.push(format!(
                "{key:?}: a `{}` row{} is a C1 breach, not a call to read: its verdict is \
                 `violation` with the finding named, never `{}`",
                key.2,
                behind.map_or_else(String::new, |kind| format!(" with a `{kind}` behind it")),
                verdict.spelled()
            ));
        }
        if key.2 == "Cutoff" || key.2 == "Unparsed" {
            failures.push(format!("{key:?}: a {} is never listable", key.2));
        }
    }
    failures
}

/// A list's rows by key: `sites`, the verdict and the reason.
type ListRows = BTreeMap<SiteKey, (usize, Verdict, String)>;

/// The list's text read: `file item kind name sites verdict reason`, and each row's one-based
/// line, for a refusal that names the row.
fn read_list(text: &str) -> Result<(ListRows, BTreeMap<SiteKey, usize>), String> {
    let mut rows = BTreeMap::new();
    let mut lines = BTreeMap::new();
    let mut header = false;
    for (at, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !header {
            if line != LIST_HEADER {
                return Err(format!("the list's header is `{line}`"));
            }
            header = true;
            continue;
        }
        let cells: Vec<&str> = line.split('\t').collect();
        let [file, item, kind, name, sites, verdict, reason] = cells.as_slice() else {
            return Err(format!("a row of seven columns: {line}"));
        };
        let sites: usize = sites
            .parse()
            .map_err(|_| format!("a count in `sites`: {line}"))?;
        let verdict = Verdict::read(verdict).map_err(|why| format!("{why}: {line}"))?;
        let key = (
            (*file).to_owned(),
            (*item).to_owned(),
            (*kind).to_owned(),
            (*name).to_owned(),
        );
        if rows
            .insert(key.clone(), (sites, verdict, (*reason).to_owned()))
            .is_some()
        {
            return Err(format!("{key:?} on two rows; each key has one row"));
        }
        lines.insert(key, at + 1);
    }
    if !header {
        return Err("the list has no header".to_owned());
    }
    Ok((rows, lines))
}

const LIST_HEADER: &str = "file\titem\tkind\tname\tsites\tverdict\treason";

// ── the fixtures: the three mutations of (b)2 and the classes ─────────────

/// The fixtures' package: a first-party crate as `bt_source` names one.
const FIXTURE_HOME: &str = "bt-rd";

/// The workspace-relative path of the request-door fixture's source.
const FIXTURE_LIB: &str = "crates/bt-source/tests/fixtures/call_graph/request_doors/lib.rs";

fn fixture_root(directory: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("call_graph")
        .join(directory)
}

fn fixture_index(directory: &str) -> Arc<Index> {
    let root = fixture_root(directory);
    let universe = Universe::declare(
        format!("the request-door guard's {directory} fixture"),
        vec![TargetRoot {
            id: TargetId {
                package: FIXTURE_HOME.to_owned(),
                kind: TargetKind::Library,
                name: "bt_rd".to_owned(),
            },
            file: root.join("lib.rs"),
        }],
        vec![DiskScope::under(&root)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::shared(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

/// Every callable of `resolver`, by its key ([`Resolver::key`]): one key may name several items
/// (one module path on two `cfg` arms, two `impl` blocks of one type).
fn keyed(resolver: &Resolver<'_>) -> BTreeMap<String, Vec<ItemId>> {
    let mut keyed: BTreeMap<String, Vec<ItemId>> = BTreeMap::new();
    for package in resolver.packages() {
        for id in resolver.callables(package) {
            if resolver.admitted(id) {
                keyed.entry(resolver.key(id)).or_default().push(id);
            }
        }
    }
    keyed
}

/// The items every reviewed row's `follows <key>; …` names, by the row's key (the review of plan
/// row 76, F1); `place` names a row for a refusal (the list's `REQUEST-DOOR-SITES.tsv:<line>`).
/// A key is [`Resolver::key`]'s spelling of an admitted callable and names one item per `cfg` arm
/// ([`arm_of`]), each followed. A key that names no item, or two items on one arm, is refused:
/// the walk would lose the callee's road with no word, or follow two items where the row read
/// one. Before the review, the walk looked each key up in a map of one item per key and dropped
/// a key it did not find.
fn followed(
    resolver: &Resolver<'_>,
    reviewed: &BTreeMap<SiteKey, (Verdict, String)>,
    place: impl Fn(&SiteKey) -> String,
) -> (BTreeMap<SiteKey, Vec<ItemId>>, Vec<String>) {
    let by_key = keyed(resolver);
    let mut follows = BTreeMap::new();
    let mut refused = Vec::new();
    for (row, (verdict, _)) in reviewed {
        let Verdict::Follows(keys) = verdict else {
            continue;
        };
        let mut items = Vec::new();
        for key in keys {
            let Some(found) = by_key.get(key) else {
                refused.push(format!(
                    "{}: `follows {key}` names no item of the product",
                    place(row)
                ));
                continue;
            };
            let mut arms: BTreeMap<String, usize> = BTreeMap::new();
            for id in found {
                *arms.entry(arm_of(resolver, *id)).or_default() += 1;
            }
            if let Some(two) = arms.values().find(|count| **count > 1) {
                refused.push(format!(
                    "{}: `follows {key}` names {two} items on one `cfg` arm",
                    place(row)
                ));
                continue;
            }
            items.extend(found.iter().copied());
        }
        follows.insert(row.clone(), items);
    }
    (follows, refused)
}

/// The fixture's registry, in `window_waits.tsv`'s shape: its rows are the fixture's doors.
fn fixture_registry() -> String {
    std::fs::read_to_string(fixture_root("request_doors").join("registry.tsv"))
        .expect("the fixture's registry")
}

/// The fixture's reviewed rows: the stored hooks, the closures a boundary item runs, and the calls
/// on a lock's result and guard, which the census does not type (`LockResult::unwrap_or_else`, the
/// guard's `len` and `push_back`) — read, as the real tree's list reads them.
fn fixture_reviewed() -> BTreeMap<SiteKey, (Verdict, String)> {
    let mut reviewed = BTreeMap::new();
    for (owner, kind, name, why) in [
        ("Lane::offer", "Dynamic", "on_full", "the fixture's hook"),
        (
            "Lane::mailbox",
            "Ambiguous",
            "unwrap_or_else",
            "a LockResult's",
        ),
        (
            "Lane::mailbox",
            "Ambiguous",
            "len",
            "a VecDeque's, under its guard",
        ),
        (
            "Lane::mailbox",
            "Ambiguous",
            "push_back",
            "a VecDeque's, under its guard",
        ),
        (
            "Lane::mailbox_through",
            "Ambiguous",
            "push_back",
            "a VecDeque's, under its guard",
        ),
        (
            "Lane::admit_with",
            "Ambiguous",
            "unwrap_or_else",
            "a LockResult's",
        ),
        (
            "Lane::admit_with",
            "Ambiguous",
            "push_back",
            "a VecDeque's, under its guard",
        ),
        (
            "Lane::admit_with",
            "Dynamic",
            "between",
            "runs the closure its caller writes, which is walked as the caller's",
        ),
        ("lock", "Ambiguous", "unwrap_or_else", "a LockResult's"),
        ("lock_cell", "Ambiguous", "unwrap_or_else", "a LockResult's"),
        (
            "Lane::peek",
            "Ambiguous",
            "unwrap_or_else",
            "a LockResult's",
        ),
        (
            "Lane::serve",
            "Ambiguous",
            "unwrap_or_else",
            "a LockResult's",
        ),
        (
            "spawn",
            "Dynamic",
            "work",
            "the thread door runs its argument on the thread it starts",
        ),
        (
            "spawn_with",
            "Dynamic",
            "say",
            "runs the closure its caller writes, which is walked as the caller's",
        ),
        (
            "spawn_with",
            "Dynamic",
            "work",
            "the thread door runs its argument on the thread it starts",
        ),
        ("SEED", "Ambiguous", "is_ok", "an io::Result's"),
        (
            "answer_with",
            "Dynamic",
            "then",
            "runs the closure its caller writes, which is walked as the caller's",
        ),
    ] {
        reviewed.insert(
            (
                FIXTURE_LIB.to_owned(),
                format!("{FIXTURE_HOME} crate::{owner}"),
                kind.to_owned(),
                name.to_owned(),
            ),
            (Verdict::Std, format!("2026-10-24: {why}")),
        );
    }
    reviewed
}

/// The fixture's door `door` walked under `registry` (the fixture's own, or a planted copy).
fn fixture_walk(registry: &str, door: &str) -> Judged {
    fixture_walk_under(registry, door, &fixture_reviewed(), None)
        .unwrap_or_else(|refused| panic!("the fixture's reviewed rows are not read: {refused:?}"))
}

/// The fixture's cut door `door`'s beside walk under `registry` (plan row 52).
fn fixture_beside(registry: &str, door: &str) -> Judged {
    fixture_walk_under(registry, door, &fixture_reviewed(), Some(Side::Beside))
        .unwrap_or_else(|refused| panic!("the fixture's reviewed rows are not read: {refused:?}"))
}

/// The fixture's door `door` walked under `registry` and the reviewed rows `reviewed` — as
/// [`Judge::walk`] walks it, or on `side` of its cut — or what [`followed`] refuses of their
/// `follows` rows.
fn fixture_walk_under(
    registry: &str,
    door: &str,
    reviewed: &BTreeMap<SiteKey, (Verdict, String)>,
    side: Option<Side>,
) -> Result<Judged, Vec<String>> {
    let index = fixture_index("request_doors");
    let resolver = Resolver::new(&[(FIXTURE_HOME, &index)], Arms::product())
        .unwrap_or_else(|refusals| panic!("{refusals:?}"));
    let read = read_registry(&resolver, registry, FIXTURE_HOME);
    assert!(read.refused.is_empty(), "{:?}", read.refused);
    let (follows, refused) = followed(&resolver, reviewed, |row| {
        format!("the reviewed row {row:?}")
    });
    if !refused.is_empty() {
        return Err(refused);
    }
    let root = workspace_root()
        .canonicalize()
        .expect("the workspace root resolves");
    let graph = CallGraph::new(&resolver);
    let judge = Judge {
        graph: &graph,
        rules: &read.rules,
        follows: &follows,
        root,
    };
    let spec = read
        .specs
        .iter()
        .find(|spec| spec.key == door)
        .unwrap_or_else(|| panic!("`{door}` is a door of the fixture's registry"));
    Ok(side.map_or_else(
        || judge.walk(spec, false),
        |side| judge.walk_on(spec, false, side),
    ))
}

/// The fixture's door `door` judged: its findings' kinds and names that no reviewed row reads,
/// sorted.
fn judge_fixture(door: &str) -> Vec<String> {
    unread(
        &fixture_walk(&fixture_registry(), door),
        &fixture_reviewed(),
    )
}

/// The findings of `judged` that no row of `reviewed` reads, as `kind name`, sorted.
fn unread(judged: &Judged, reviewed: &BTreeMap<SiteKey, (Verdict, String)>) -> Vec<String> {
    assert!(judged.refused.is_empty(), "{:?}", judged.refused);
    let mut found: Vec<String> = judged
        .findings
        .iter()
        .filter(|finding| {
            reviewed
                .get(&finding.key)
                .is_none_or(|(verdict, _)| *verdict == Verdict::Unreviewed)
        })
        .map(|finding| format!("{} {}", finding.key.2, finding.key.3))
        .collect();
    found.sort();
    found.dedup();
    found
}

/// What the registry reader refuses in `registry` over the fixture.
fn fixture_refusals(registry: &str) -> Vec<String> {
    let index = fixture_index("request_doors");
    let resolver = Resolver::new(&[(FIXTURE_HOME, &index)], Arms::product())
        .unwrap_or_else(|refusals| panic!("{refusals:?}"));
    read_registry(&resolver, registry, FIXTURE_HOME).refused
}

/// The number of items the fixture's key `key` names, or why it is refused.
fn fixture_key(key: &str) -> Result<usize, String> {
    let index = fixture_index("request_doors");
    let resolver = Resolver::new(&[(FIXTURE_HOME, &index)], Arms::product())
        .unwrap_or_else(|refusals| panic!("{refusals:?}"));
    key_items(&resolver, FIXTURE_HOME, key).map(|items| items.len())
}

/// RED — **the clean door passes**: its mailbox lock, its notify, the worker closure it hands to
/// `spawn` (whose body sleeps and locks the worker's own state) and its reviewed hook are all
/// within C1.
///
/// MUTATION: in `Judge::walk`, do not skip a closure handed at a boundary call's worker argument —
/// the handed worker's body, `Lane::serve`, is judged as run inline.
#[test]
fn a_door_that_only_enqueues_passes() {
    assert_eq!(judge_fixture("submit"), Vec::<String>::new());
}

/// RED — **(b)2 mutation 1: a helper of the door that writes a file** fails on the effect.
///
/// MUTATION: drop `std::fs::` from [`EFFECT_NAMESPACES`] — the write passes.
#[test]
fn a_helper_that_writes_a_file_fails_the_door() {
    assert_eq!(judge_fixture("submit_writes"), ["Effect std::fs::write"]);
}

/// RED — **(b)2 mutation 2: a `Mutex::lock` on the lock the worker holds, in a helper of the
/// door**, fails: the helper is not the row's mailbox.
///
/// MUTATION: judge a blocking primitive as allowed whatever item takes it — the lock passes.
#[test]
fn a_lock_the_worker_holds_fails_the_door() {
    assert_eq!(
        judge_fixture("submit_peeks"),
        ["Blocking std::sync::Mutex::lock"]
    );
}

/// RED — **(b)2 mutation 3: a `Box<dyn Fn>` called on the door's road that no row lists** fails
/// closed.
///
/// MUTATION: let a `Dynamic` edge pass unlisted — the call passes.
#[test]
fn an_unlisted_dynamic_call_fails_the_door() {
    assert_eq!(judge_fixture("submit_calls_back"), ["Dynamic on_done"]);
}

/// RED — **an owner door's body reached, and a worker body run inline**, each fail the door, and
/// each is a stop: what runs behind it (the worker's lock and sleep) is that body's, not the
/// door's. So does a boundary the walk never meets.
///
/// MUTATION: drop the `owner_bodies` check in `Judge::walk` — `present` passes.
#[test]
fn an_owner_door_or_an_inline_worker_fails_the_door() {
    assert_eq!(
        judge_fixture("submit_and_draw"),
        ["OwnerDoor bt-rd crate::present (Present)"]
    );
    assert_eq!(
        judge_fixture("serve_inline"),
        [
            "Unmet spawn@0",
            "Unmet std::sync::Condvar::notify_one",
            "WorkerBody bt-rd crate::Lane::serve",
        ]
    );
}

/// RED — **a `const` is the compiler's, a `LazyLock` is this thread's**: a constant that names the
/// worker's writer runs nothing on the door's road; a lazy initializer the road derefs may run there,
/// and its file read is the door's.
///
/// MUTATION: walk a `Target::Value` reached by `Via::Value` too — the constant's `record` and its
/// `std::fs::write` are judged as the door's.
#[test]
fn a_constant_runs_nothing_and_a_lazy_initializer_runs_here() {
    assert_eq!(judge_fixture("submit_with_hooks"), Vec::<String>::new());
    assert_eq!(judge_fixture("submit_seeded"), ["Effect std::fs::read"]);
}

/// RED (review H1) — **a closure the boundary item runs itself is the door's**: an enqueue that
/// runs its caller's closure under the lock (`admit_between`'s shape), and a thread door's voice
/// (a refusal's `say`, `wake`) beside its work: each closure's file write is the door's, and the
/// work handed at the worker's argument is not.
///
/// MUTATION: in `Judge::walk`, skip every closure handed to a boundary call, whatever the argument
/// (`.is_some_and(|workers| workers.contains(&handed.argument))` → `.is_some()`) — both pass.
#[test]
fn a_closure_the_boundary_item_runs_is_the_doors() {
    assert_eq!(judge_fixture("submit_between"), ["Effect std::fs::write"]);
    assert_eq!(
        judge_fixture("submit_with_voice"),
        ["Effect std::fs::write"]
    );
}

/// RED (review H2) — **a walked door that reaches a door this guard does not walk fails**: a door
/// asked before the loop (`video::prewarm`'s shape) reached from the window thread contradicts its
/// row, and nothing behind it is judged.
///
/// MUTATION: drop the `not_walked` check in `Judge::walk` — `prewarm` is a ledger door and passes as
/// a silent stop.
#[test]
fn a_door_reaching_a_door_not_walked_fails() {
    assert_eq!(
        judge_fixture("submit_prewarms"),
        ["NotWalked prewarm (main (before the loop))"]
    );
}

/// RED (review H3) — **a generic lock helper is exempt only through a declared mailbox method**:
/// the mailbox taken through `lock(&self.jobs)` by `Lane::mailbox_through` passes; the worker's lock
/// taken through `lock(&self.worker)` by `Lane::peek_through`, which no row declares, fails.
///
/// MUTATION: drop the lock-helper check in `Judge::walk` — the worker's lock passes.
#[test]
fn a_lock_helper_is_exempt_only_through_a_declared_method() {
    assert_eq!(judge_fixture("submit_through"), Vec::<String>::new());
    assert_eq!(
        judge_fixture("submit_peeks_through"),
        ["Blocking bt-rd crate::lock"]
    );
}

/// RED (re-check of the review, R2) — **a helper whose argument is a lock written through a type
/// alias is a lock helper**: `lock_cell(cell: &Held)` under `type Held = Mutex<u32>`, declared in
/// the row's mailbox, locks the worker's lock for `Lane::peek_through_alias`, which no row
/// declares, and fails it.
///
/// MUTATION: drop the placed paths from [`takes_a_lock`] (the written syntax only) — `lock_cell`
/// is no lock helper, and the worker's lock passes.
#[test]
fn a_lock_helper_written_through_an_alias_is_one() {
    assert_eq!(
        judge_fixture("submit_peeks_through_alias"),
        ["Blocking bt-rd crate::lock_cell"]
    );
}

/// RED — **(ii) an entrance and (i) a first-party vocabulary entry** reached each fail the door.
///
/// MUTATION: drop the `Entrance` check in `Judge::walk` — both pass.
#[test]
fn an_entrance_and_a_first_party_effect_fail_the_door() {
    assert_eq!(
        judge_fixture("submit_pokes"),
        [
            "Entrance entrance bt_rd::Lane::poke (the poke door)",
            "Entrance vocabulary bt_rd::resize",
        ]
    );
}

/// RED — **(vi) a window door under repair is inherited** by the door that reaches it.
///
/// MUTATION: read every window door as `Standing::PassThrough` — `note` passes.
#[test]
fn a_window_door_under_repair_is_inherited() {
    assert_eq!(judge_fixture("submit_noting"), ["Inherits note (row 20)"]);
}

/// RED — **a `handed` worker named as a value is the worker's; called on the road, it fails**.
///
/// MUTATION: in `Judge::walk`, let a `handed` item reached by a call pass as one named — the call
/// passes.
#[test]
fn a_handed_worker_named_passes_and_called_fails() {
    assert_eq!(judge_fixture("submit_handing"), Vec::<String>::new());
    assert_eq!(
        judge_fixture("submit_runs_handed"),
        ["HandedCalled bt-rd crate::work_fn"]
    );
}

/// RED (ruling Q6) — **a print macro fails; `format!` passes**.
///
/// MUTATION: add `eprintln` to [`ALLOWED_MACROS`] — the line passes.
#[test]
fn a_print_macro_fails_and_format_passes() {
    assert_eq!(judge_fixture("submit_says"), ["Macro eprintln"]);
}

/// RED — **(v) every unresolved kind fails closed, a one-candidate call is followed, and a derive
/// passes**: a name nothing holds (`Path`), a call on another arm (`OtherArm`), an untyped
/// receiver's call (`Ambiguous`, whose one candidate's file write is then met); a `#[derive]`'s
/// callee passes. The engine generates a `Generated` edge only for a derive or `#[from]`, both of
/// which pass, so no other `Generated` exists to fail.
///
/// MUTATIONS: let a `Path` edge pass (`Unresolved::Path { .. } => continue`) — the two `Path` rows
/// go; do not follow a one-candidate `Ambiguous` — the `Effect` goes; drop
/// [`generated_is_covered`] — the derive fails.
#[test]
fn every_unresolved_kind_fails_closed_and_a_derive_passes() {
    assert_eq!(
        judge_fixture("submit_strays"),
        [
            "Ambiguous writes_once",
            "Effect std::fs::write",
            "OtherArm test_only",
            "Path nowhere",
            "Path nowhere_made",
        ]
    );
    assert_eq!(judge_fixture("submit_copies"), Vec::<String>::new());
}

/// RED — **a `Cutoff` met in the walk is refused**, never listable (an `Unparsed` edge, the same
/// road, has no fixture: the index refuses a file `syn` does not parse).
///
/// MUTATION: in `Judge::walk`, let a `Cutoff` fall through to an ordinary finding — the refusal is
/// gone.
#[test]
fn a_cutoff_in_the_walk_is_refused() {
    let judged = fixture_walk(&fixture_registry(), "submit_far");
    assert!(
        judged
            .refused
            .iter()
            .any(|refusal| refusal.starts_with("a cutoff at `link20`")),
        "{:?}",
        judged.refused
    );
}

/// RED (review nit 5) — **the effect and blocking lists cover the tree's Win32 file calls, a path's
/// questions to the disk and a park**: `FileSystem::DeleteFileW`, `Path::exists` and
/// `thread::park` fail; `FileSystem::FILE_SHARE_MODE(..)`, a type of the module, is no call of it.
///
/// MUTATION: drop the [`WIN32_EFFECT_MODULES`] arm of [`is_effect`] — `DeleteFileW` passes; drop
/// its lowercase rule — `FILE_SHARE_MODE` fails.
#[test]
fn the_effect_and_blocking_lists_cover_win32_paths_and_parks() {
    assert_eq!(
        judge_fixture("submit_asks_the_disk"),
        [
            "Blocking std::thread::park",
            "Effect std::path::Path::exists",
            "Effect windows::Win32::Storage::FileSystem::DeleteFileW",
        ]
    );
}

/// RED (ruling D2 of T-UNTYPED-RECEIVER-FS-CALLS-TRIAGE) — **a method of a file-system value is
/// no effect; the question that made it is**: `Metadata::is_file`, `Metadata::len`,
/// `Permissions::from_mode` and `MetadataExt::file_attributes` pass, `std::fs::metadata` fails.
///
/// MUTATION: drop the [`PURE_FS_VALUES`] exception from [`is_effect`] — the four accessors are
/// `Effect` findings.
#[test]
fn a_file_system_values_accessor_is_no_effect_and_its_question_is() {
    assert_eq!(
        judge_fixture("submit_reads_a_value"),
        ["Effect std::fs::metadata"]
    );
}

/// RED (row 116, T-EFFECT-GUARD-EXITSTATUS-READ) — **a collected child's status and output are
/// values; the wait that collected them is the effect**: `ExitStatus::code`, `ExitStatus::success`,
/// `ExitStatus::from_raw` and `Output::clone` pass, `Child::wait` and `Child::try_wait` fail, and
/// so does `ExitStatusError::code`, which the rule leaves out of its scope (review N2).
///
/// MUTATIONS: drop the [`PURE_PROCESS_VALUES`] exception from [`is_effect`] — the four are `Effect`
/// findings; drop the trailing `::` of `"std::process::ExitStatus::"` — `ExitStatusError::code`
/// passes.
#[test]
fn a_child_status_read_is_no_effect_and_its_wait_is() {
    assert_eq!(judge_fixture("submit_reads_a_status"), Vec::<String>::new());
    assert_eq!(
        judge_fixture("submit_waits_for_a_child"),
        [
            "Effect std::process::Child::try_wait",
            "Effect std::process::Child::wait",
            "Effect std::process::ExitStatusError::code",
        ]
    );
}

/// RED (T-UNTYPED-RECEIVER-FS-CALLS-TRIAGE) — **a path question on an untyped receiver is not
/// classified; written on a typed one it is an effect**: the closure handed to a
/// `&dyn Fn(&Path) -> bool` parameter (as `bt_platform::program_in_directories` takes one) gets no
/// type from it, so `path.is_file()` is `Ambiguous` and no `Effect`; with its parameter written
/// `&Path` it is `Effect std::path::Path::is_file`. The call of the parameter is `Dynamic` in both.
///
/// MUTATION: write the untyped door's parameter as `path: &std::path::Path` — its `Ambiguous`
/// finding becomes the `Effect`; drop `std::path::Path::is_file` from [`EFFECT_FUNCTIONS`] — the
/// typed door's `Effect` goes.
#[test]
fn a_path_question_is_an_effect_only_on_a_typed_receiver() {
    assert_eq!(
        judge_fixture("submit_probes_untyped"),
        ["Ambiguous is_file", "Dynamic is_file"]
    );
    assert_eq!(
        judge_fixture("submit_probes_typed"),
        ["Dynamic is_file", "Effect std::path::Path::is_file"]
    );
}

/// RED (review nits 3, 4) — **the registry's cells read or are refused**: a bare key that names
/// one name of two modules on one arm is refused, a key qualified by its module names one item,
/// and a key naming one item per platform's module is admitted; a first-party `# vocabulary` path
/// that names nothing is refused, as an `# entrances` one is; a boundary's worker argument past
/// the item's arguments is refused, and so is any `@N` on a foreign callee, whose signature the
/// guard does not read (re-check of the review, R3).
///
/// MUTATIONS: drop the one-arm check in [`key_items`] — `twin` is admitted; drop the empty
/// vocabulary refusal in [`read_registry`] — the planted path passes; drop the argument count —
/// `spawn@5` passes; drop the foreign `@N` refusal — `notify_one@0` passes.
#[test]
fn the_registry_refuses_a_key_on_two_modules_and_a_path_naming_nothing() {
    let twin = fixture_key("twin");
    assert!(
        twin.as_ref()
            .is_err_and(|why| why.contains("items on one `cfg` arm")),
        "{twin:?}"
    );
    assert_eq!(fixture_key("a::twin"), Ok(1));
    assert!(fixture_key("armed").is_ok());
    let registry = fixture_registry();
    assert_eq!(fixture_refusals(&registry), Vec::<String>::new());
    let planted = registry.replacen(
        "path\ttargets\treason\n",
        "path\ttargets\treason\nbt_rd::nothing_here\tall\tplanted\n",
        1,
    );
    assert_eq!(
        fixture_refusals(&planted),
        ["window_waits.tsv:5: the vocabulary path `bt_rd::nothing_here` names no item"]
    );
    let past = registry.replacen(
        "submit_with_voice\twindow\tspawn_with@1",
        "submit_with_voice\twindow\tspawn_with@5",
        1,
    );
    let refusals = fixture_refusals(&past);
    assert!(
        refusals
            .iter()
            .any(|refusal| refusal.contains("`spawn_with@5` names argument 5")),
        "{refusals:?}"
    );
    let foreign = registry.replacen(
        "submit\twindow\tstd::sync::Condvar::notify_one, spawn@0",
        "submit\twindow\tstd::sync::Condvar::notify_one@0, spawn@0",
        1,
    );
    assert_eq!(
        fixture_refusals(&foreign),
        [
            "window_waits.tsv:24: `submit`: `std::sync::Condvar::notify_one@0` names an \
             argument of a foreign callee, whose signature the guard does not read"
        ]
    );
}

/// RED (plan row 52) — **a cut door is walked to its boundary from its enqueue, and its beside
/// side keeps its breaches**: `gesture` enqueues through `gesture_enqueue` and answers through
/// `gesture_answer` (a file write, an owner door's body) and `gesture_glances` (two names nothing
/// holds). The C1 walk passes; the beside walk meets the write, the owner door's body and both
/// names. (a) and (f) of the report: the breaches are kept, a read loose row is kept, an unread one
/// leaves.
///
/// MUTATIONS: in `Judge::walk_on`, do not skip a `beside` edge on the enqueue side — the C1 walk
/// meets the write; drop the beside walk (`Side::Beside if !beside => continue` as `{}`) — every
/// edge of the body is met, `gesture_enqueue`'s too; in [`split_beside`], keep only the strict
/// kinds — the read row leaves.
#[test]
fn a_cut_door_is_walked_from_its_enqueue_and_its_beside_side_keeps_its_breaches() {
    assert_eq!(judge_fixture("gesture"), Vec::<String>::new());
    let beside = fixture_beside(&fixture_registry(), "gesture");
    assert!(beside.refused.is_empty(), "{:?}", beside.refused);
    assert_eq!(
        unread(&beside, &fixture_reviewed()),
        [
            "Effect std::fs::write",
            "OwnerDoor bt-rd crate::present (Present)",
            "Path nowhere",
            "Path nowhere_made",
        ]
    );
    let mut reviewed = fixture_reviewed();
    let read = beside
        .findings
        .iter()
        .find(|finding| finding.key.3 == "nowhere_made")
        .expect("the beside walk meets `nowhere_made`")
        .key
        .clone();
    reviewed.insert(read, (Verdict::Std, "2026-11-02: planted".to_owned()));
    let (kept, left) = split_beside(beside.findings, &reviewed);
    let names = |findings: &[Finding]| -> Vec<String> {
        let mut names: Vec<String> = findings
            .iter()
            .map(|finding| format!("{} {}", finding.key.2, finding.key.3))
            .collect();
        names.sort();
        names.dedup();
        names
    };
    assert_eq!(
        names(&kept),
        [
            "Effect std::fs::write",
            "OwnerDoor bt-rd crate::present (Present)",
            "Path nowhere_made",
        ]
    );
    assert_eq!(names(&left), ["Path nowhere"]);
}

/// RED (plan row 52) — **a closure handed to a `beside` call is beside's**: `gesture_hands_beside`
/// writes a file in the closure it hands to `answer_with`. The C1 walk passes, and the beside walk
/// meets the write.
///
/// MUTATION: in `Judge::walk_on`, read only the callee (drop `handed_to_one_of(.., &beside_calls)`)
/// — the closure's write is met on the enqueue side.
#[test]
fn a_closure_handed_to_a_beside_call_is_beside() {
    assert_eq!(judge_fixture("gesture_hands_beside"), Vec::<String>::new());
    let beside = fixture_beside(&fixture_registry(), "gesture_hands_beside");
    assert_eq!(
        unread(&beside, &fixture_reviewed()),
        ["Effect std::fs::write"]
    );
}

/// RED (plan row 52) — **every shape a cut does not read is refused, naming the registry's line and
/// the call's `file:line`** (b–e of the report): a direct callee named in neither cell
/// (`gesture_strays`' `Lane::peek`); an enqueue called only inside a closure literal
/// (`gesture_in_a_closure`); a name in either cell the door's body does not call; an enqueue road
/// that meets no boundary entry (`Unmet`).
///
/// MUTATIONS: in `Judge::walk_on`, drop the neither-cell refusal; drop the closure-literal refusal
/// (the enqueue is then walked as beside's closure, unread); drop the stale-name pass; skip the
/// `Unmet` pass on the enqueue side as on the beside side.
#[test]
fn every_shape_a_cut_does_not_read_is_refused() {
    let registry = fixture_registry();
    let line_of = |door: &str| -> usize {
        registry
            .lines()
            .skip_while(|line| *line != "# request-door-cuts")
            .position(|line| line.starts_with(&format!("{door}\t")))
            .map(|at| {
                registry
                    .lines()
                    .position(|line| line == "# request-door-cuts")
                    .unwrap_or_default()
                    + at
                    + 1
            })
            .expect("the door has a cut row")
    };
    let file = "crates/bt-source/tests/fixtures/call_graph/request_doors/lib.rs";
    let at = |name: &str| -> usize {
        std::fs::read_to_string(fixture_root("request_doors").join("lib.rs"))
            .expect("the fixture")
            .lines()
            .position(|line| line.contains(name))
            .expect("the fixture writes the call")
            + 1
    };
    assert_eq!(
        fixture_walk(&registry, "gesture_strays").refused,
        [format!(
            "window_waits.tsv:{}: `gesture_strays` calls `crate::Lane::peek` at {file}:{}, which \
             its `# request-door-cuts` row names in neither `enqueue` nor `beside`",
            line_of("gesture_strays"),
            at("let _ = lane.peek(); // gesture_strays")
        )]
    );
    assert_eq!(
        fixture_walk(&registry, "gesture_in_a_closure").refused,
        [format!(
            "window_waits.tsv:{}: `gesture_in_a_closure`'s enqueue `crate::gesture_enqueue` is \
             called at {file}:{} inside a closure literal: an enqueue is a call the door's own body \
             makes",
            line_of("gesture_in_a_closure"),
            at("answer_with(job, move || gesture_enqueue(lane, job)); // gesture_in_a_closure")
        )]
    );
    let stale = registry.replacen(
        "gesture\tgesture_enqueue\tgesture_answer, gesture_glances\t",
        "gesture\tgesture_enqueue\tgesture_answer, gesture_glances, present\t",
        1,
    );
    assert_eq!(
        fixture_walk(&stale, "gesture").refused,
        [format!(
            "window_waits.tsv:{}: `gesture`'s `beside` names `present`, which the door's own body \
             does not call",
            line_of("gesture")
        )]
    );
    let unmet = registry.replacen(
        "gesture\tgesture_enqueue\tgesture_answer, gesture_glances\t",
        "gesture\tgesture_answer\tgesture_enqueue, gesture_glances\t",
        1,
    );
    let judged = fixture_walk(&unmet, "gesture");
    assert!(judged.refused.is_empty(), "{:?}", judged.refused);
    let unmet: Vec<String> = unread(&judged, &fixture_reviewed())
        .into_iter()
        .filter(|row| row.starts_with("Unmet "))
        .collect();
    assert_eq!(
        unmet,
        [
            "Unmet spawn@0",
            "Unmet std::sync::Condvar::notify_one",
        ]
    );
}

/// RED (plan row 52) — **a `# request-door-cuts` row is read whole or refused by its line**: a cut
/// on a door that is no window `# request-doors` row, a door cut twice, an undated reason, no
/// enqueue, and one item in both cells.
///
/// MUTATION: drop any one check of [`read_registry`]'s cut pass — its refusal is gone.
#[test]
fn a_cut_row_is_read_whole_or_refused_by_its_line() {
    let registry = fixture_registry();
    assert_eq!(fixture_refusals(&registry), Vec::<String>::new());
    let header = "# request-door-cuts\ndoor\tenqueue\tbeside\treason\n";
    let first = registry
        .lines()
        .position(|line| line == "# request-door-cuts")
        .expect("the fixture's cuts")
        + 3;
    let with = |row: &str| registry.replacen(header, &format!("{header}{row}\n"), 1);
    assert_eq!(
        fixture_refusals(&with("prewarm\tgesture_enqueue\t—\t2026-11-02: planted")),
        [format!(
            "window_waits.tsv:{first}: the cut `prewarm` is not a `# request-doors` row asked on \
             the window"
        )]
    );
    assert_eq!(
        fixture_refusals(&with("gesture\tgesture_enqueue\tgesture_answer\t2026-11-02: planted")),
        [format!(
            "window_waits.tsv:{}: `gesture` is cut on two rows",
            first + 1
        )]
    );
    assert_eq!(
        fixture_refusals(&with("submit\tLane::offer\t—\tplanted")),
        [format!(
            "window_waits.tsv:{first}: `submit`'s cut reason is not dated (`YYYY-MM-DD: why`)"
        )]
    );
    assert_eq!(
        fixture_refusals(&with("submit\t—\tLane::offer\t2026-11-02: planted")),
        [format!("window_waits.tsv:{first}: `submit`'s cut names no enqueue")]
    );
    assert_eq!(
        fixture_refusals(&with("submit\tLane::offer\tLane::offer\t2026-11-02: planted")),
        [format!(
            "window_waits.tsv:{first}: `submit`'s cut names `Lane::offer` in both `enqueue` and \
             `beside`"
        )]
    );
}

/// RED (plan row 52, R6) — **a standard thread spawn's closure argument may be the boundary's
/// worker**: `submit_on_a_builder` hands `Lane::serve` to a typed `std::thread::Builder`'s
/// `spawn`. Named `std::thread::Builder::spawn@0` the walk passes; named without its argument the
/// worker's body is met; any other argument is refused by the table's word.
///
/// MUTATION: drop [`STD_SPAWNS`]' admission in [`read_registry`] — the clean row is refused.
#[test]
fn a_standard_spawn_names_its_closure_argument_from_the_table() {
    assert_eq!(judge_fixture("submit_on_a_builder"), Vec::<String>::new());
    let registry = fixture_registry();
    let row = "submit_on_a_builder\twindow\tstd::sync::Condvar::notify_one, std::thread::Builder::spawn@0";
    let bare = registry.replacen(row, &row.replacen("spawn@0", "spawn", 1), 1);
    assert_eq!(
        unread(&fixture_walk(&bare, "submit_on_a_builder"), &fixture_reviewed()),
        ["WorkerBody bt-rd crate::Lane::serve"]
    );
    let line = registry
        .lines()
        .position(|line| line.starts_with(row))
        .expect("the row")
        + 1;
    assert_eq!(
        fixture_refusals(&registry.replacen(row, &row.replacen("spawn@0", "spawn@1", 1), 1)),
        [format!(
            "window_waits.tsv:{line}: `submit_on_a_builder`: `std::thread::Builder::spawn@1` names \
             argument 1, and `std::thread::Builder::spawn`'s thread body is argument 0 (the guard's \
             table of standard spawns)"
        )]
    );
}

/// RED (the review of plan row 76, F1) — **a `follows` row names the items it follows, or is
/// refused by its row**: the fixture's stored hook `on_full`, met on `submit`'s road, read as
/// `follows <key>; …`. A key naming one item is followed (`record`'s file write is met), a key
/// naming one item per arm follows both (`media::quiet`); a key that names nothing (a callee
/// moved or renamed) and a key naming two items on one arm (`Pair::get`, two `impl` blocks) are
/// refused by the row, the other keys of the row read as written. Before the review, the walk
/// looked each key up in a map of one item per key: the stale key was dropped with no word (the
/// hook's road lost), and `Pair::get` followed whichever impl the map kept.
///
/// MUTATIONS: drop the empty refusal in [`followed`] — the stale key passes and `record` alone is
/// met; drop its arm count — `Pair::get` is followed.
#[test]
fn a_follows_row_names_its_items_or_is_refused_by_its_row() {
    let row: SiteKey = (
        FIXTURE_LIB.to_owned(),
        format!("{FIXTURE_HOME} crate::Lane::offer"),
        "Dynamic".to_owned(),
        "on_full".to_owned(),
    );
    let planted = |verdict: &str| -> Result<Vec<String>, Vec<String>> {
        let mut reviewed = fixture_reviewed();
        reviewed.insert(
            row.clone(),
            (
                Verdict::read(verdict).expect("a planted verdict reads"),
                "2026-10-29: planted".to_owned(),
            ),
        );
        let judged = fixture_walk_under(&fixture_registry(), "submit", &reviewed, None)?;
        Ok(unread(&judged, &reviewed))
    };
    let place = format!("the reviewed row {row:?}");
    assert_eq!(planted("std"), Ok(Vec::new()), "the fixture's own reading");
    assert_eq!(
        planted("follows bt-rd crate::record"),
        Ok(vec!["Effect std::fs::write".to_owned()]),
        "the followed item's write is met"
    );
    assert_eq!(
        planted("follows bt-rd crate::media::quiet"),
        Ok(Vec::new()),
        "one item per arm, both followed"
    );
    assert_eq!(
        planted("follows bt-rd crate::record; bt-rd crate::recorded"),
        Err(vec![format!(
            "{place}: `follows bt-rd crate::recorded` names no item of the product"
        )]),
        "a stale key is refused by its row"
    );
    assert_eq!(
        planted("follows bt-rd crate::Pair::get"),
        Err(vec![format!(
            "{place}: `follows bt-rd crate::Pair::get` names 2 items on one `cfg` arm"
        )]),
        "two items on one arm are refused"
    );
}

/// RED (plan row 76) — **an `# effects` row names one item of its arm, or is refused by its
/// line**: a key that names no item (a door body moved or renamed, as K2 batch 22's
/// `crate::read_video_glance` was), a key on two arms of one module path read under `-` or under
/// an arm it does not stand on, a key naming two items on one arm, and a kind off the registry's
/// closed list are each refused naming `window_waits.tsv:<line>`; the arm the item stands on is
/// admitted. Before the row, the loader looked a row up by its first-path key string and
/// inserted what it found: the stale key, the wrong arm and the unknown kind all came back with
/// no refusal, and a two-arm key kept whichever item the map's order put last.
///
/// MUTATIONS: drop the empty refusal in [`effect_item`] — the stale key passes; stop reading the
/// arm — `-` names `media::quiet` again; drop the kind check — `worker-door-bdy` passes.
#[test]
fn an_effects_row_names_one_item_of_its_arm_or_is_refused_by_its_line() {
    let registry = fixture_registry();
    let header = "crate\tarm\titem\teffects\tkind\tauthority\tdoor\n";
    let line = registry
        .lines()
        .position(|line| line == header.trim_end())
        .expect("the fixture has an `# effects` header")
        + 2;
    let planted = |row: &str| registry.replacen(header, &format!("{header}{row}\n"), 1);

    assert_eq!(
        fixture_refusals(&planted(
            "bt-rd\t-\tcrate::Lane::serve_gone\tworks\tworker-door-body\tthe worker\t"
        )),
        [format!(
            "window_waits.tsv:{line}: `# effects` `bt-rd crate::Lane::serve_gone` (arm `-`) names no \
             item of the product"
        )]
    );
    assert_eq!(
        fixture_refusals(&planted(
            "bt-rd\t[windows]\tcrate::media::quiet\tquiets\towner-door-body\tthe window\t"
        )),
        Vec::<String>::new()
    );
    assert_eq!(
        fixture_refusals(&planted(
            "bt-rd\t[not(windows)]\tcrate::media::quiet\tquiets\towner-door-body\tthe window\t"
        )),
        Vec::<String>::new()
    );
    assert_eq!(
        fixture_refusals(&planted(
            "bt-rd\t-\tcrate::media::quiet\tquiets\towner-door-body\tthe window\t"
        )),
        [format!(
            "window_waits.tsv:{line}: `# effects` `bt-rd crate::media::quiet` (arm `-`) names no item \
             on that arm; the item stands on [not(windows)], [windows]"
        )]
    );
    assert_eq!(
        fixture_refusals(&planted(
            "bt-rd\t[unix]\tcrate::media::quiet\tquiets\towner-door-body\tthe window\t"
        )),
        [format!(
            "window_waits.tsv:{line}: `# effects` `bt-rd crate::media::quiet` (arm `[unix]`) names no \
             item on that arm; the item stands on [not(windows)], [windows]"
        )]
    );
    assert_eq!(
        fixture_refusals(&planted(
            "bt-rd\t-\tcrate::Pair::get\tgets\tworker-door-body\tthe worker\t"
        )),
        [format!(
            "window_waits.tsv:{line}: `# effects` `bt-rd crate::Pair::get` (arm `-`) names 2 items on \
             that arm"
        )]
    );
    assert_eq!(
        fixture_refusals(&planted(
            "bt-rd\t-\tcrate::Lane::serve\tworks\tworker-door-bdy\tthe worker\t"
        )),
        [format!(
            "window_waits.tsv:{line}: `# effects` `bt-rd crate::Lane::serve`'s kind `worker-door-bdy` \
             is not one of owner-door-body, interim-owner, worker-door-body, transport, \
             drop-exception, shared-body, emergency"
        )]
    );
}

/// RED — **the list refuses what it may not excuse**: a `Cutoff` row, and a door-level or effect
/// row whose verdict is not `violation`.
///
/// An `Implicit` row behind which is an effect (a `Drop` that writes a file) is held as the effect
/// would be (review nit 1).
///
/// MUTATION: drop the verdict-by-kind check in [`list_failures`] — the `std` effect row passes; drop
/// its `under` half — the `std` implicit row passes.
#[test]
fn the_list_refuses_a_cutoff_and_a_soft_verdict_on_an_effect() {
    let key = |kind: &str| -> SiteKey {
        (
            "f.rs".to_owned(),
            "rd crate::x".to_owned(),
            kind.to_owned(),
            "n".to_owned(),
        )
    };
    let mut tree = BTreeMap::new();
    tree.insert(key("Effect"), 1);
    tree.insert(key("Cutoff"), 1);
    tree.insert(key("Implicit"), 1);
    let mut list = BTreeMap::new();
    list.insert(key("Effect"), (1, Verdict::Std, String::new()));
    list.insert(key("Cutoff"), (1, Verdict::Unreviewed, String::new()));
    list.insert(key("Implicit"), (1, Verdict::Std, String::new()));
    let mut under = BTreeMap::new();
    under.insert(
        key("Implicit"),
        std::iter::once("Effect".to_owned()).collect(),
    );
    let failures = list_failures(&tree, &list, &BTreeMap::new(), &under);
    assert_eq!(failures.len(), 3, "{failures:?}");
    assert!(
        failures
            .iter()
            .any(|failure| failure.contains("\"Implicit\"")
                && failure.contains("with a `Effect` behind it")),
        "{failures:?}"
    );
    assert!(
        failures
            .iter()
            .any(|failure| failure.contains("\"Effect\"") && failure.contains("is `violation`")),
        "{failures:?}"
    );
    assert!(
        failures
            .iter()
            .any(|failure| failure.contains("\"Cutoff\"") && failure.contains("never listable")),
        "{failures:?}"
    );
}

// ── the real tree ─────────────────────────────────────────────────────────

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn real_tree() -> Vec<(String, Arc<Index>)> {
    let workspace = Workspace::read(&workspace_root()).expect("this workspace");
    workspace
        .packages()
        .iter()
        .filter(|package| package.name() != "bt-source")
        .map(|package| {
            let vendor = if is_vendored(package.directory()) {
                Vendor::Included
            } else {
                Vendor::Excluded
            };
            let universe = universes::crate_sources(package, vendor)
                .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
            let index = Index::shared(&universe)
                .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
            (package.name().to_owned(), index)
        })
        .collect()
}

/// One section of the registry: each row's one-based line in the registry and its cells, the
/// header checked — for a refusal that names the row by its line.
fn numbered_section<'r>(
    registry: &'r str,
    name: &str,
    columns: &[&str],
) -> Vec<(usize, Vec<&'r str>)> {
    let marker = format!("# {name}");
    let mut lines = registry
        .lines()
        .enumerate()
        .skip_while(|(_, line)| *line != marker)
        .skip(1)
        .take_while(|(_, line)| !line.starts_with("# "))
        .filter(|(_, line)| !line.is_empty() && !line.starts_with("##"));
    let header: Vec<&str> = lines
        .next()
        .unwrap_or_else(|| panic!("the registry has a `{marker}` section"))
        .1
        .split('\t')
        .collect();
    assert_eq!(header, columns, "`{marker}`'s columns");
    lines
        .map(|(at, line)| {
            let cells: Vec<&str> = line.split('\t').collect();
            assert_eq!(cells.len(), columns.len(), "`{marker}`: {line}");
            (at + 1, cells)
        })
        .collect()
}

/// [`BLOCKING`] against the `# vocabulary` of `registry` (`window_waits.tsv`'s shape) and
/// [`NOT_A_WINDOW_WAIT`]: each failure names its path.
fn blocking_alignment_failures(registry: &str) -> Vec<String> {
    let vocabulary: BTreeSet<&str> =
        numbered_section(registry, "vocabulary", &["path", "targets", "reason"])
            .into_iter()
            .map(|(_, cells)| cells[0])
            .collect();
    let not_a_wait: BTreeSet<&str> = NOT_A_WINDOW_WAIT.iter().map(|(path, _)| *path).collect();
    let mut failures = Vec::new();
    for path in BLOCKING {
        match (vocabulary.contains(path), not_a_wait.contains(path)) {
            (true, false) | (false, true) => {}
            (false, false) => failures.push(format!(
                "`{path}` is a blocking primitive the vocabulary does not name and \
                 `NOT_A_WINDOW_WAIT` does not excuse"
            )),
            (true, true) => failures.push(format!(
                "`{path}` is both a vocabulary path and `NOT_A_WINDOW_WAIT`"
            )),
        }
    }
    for path in &not_a_wait {
        if !BLOCKING.contains(path) {
            failures.push(format!(
                "`{path}` is in `NOT_A_WINDOW_WAIT` and is no blocking primitive"
            ));
        }
    }
    failures
}

/// RED (T-BARE-SITE-VOCABULARY-WAIT-WHILE-BLIND-SPOT, plan row 176, ruling R4) — **every blocking
/// primitive is a window-thread wait or is named as not one**: each [`BLOCKING`] path is a
/// `# vocabulary` path of `crates/bt-app/src/window_waits.tsv` or a [`NOT_A_WINDOW_WAIT`] path,
/// never both, and every `NOT_A_WINDOW_WAIT` path is a `BLOCKING` one. Row 145 met the gap this
/// closes: `Condvar::wait_timeout_while` was here and not in the vocabulary, so a door written
/// with it left the bare-site inventory without going through a door. Against the live registry
/// it passes; with its `std::thread::park_timeout` line taken out, and with a
/// `std::sync::Mutex::lock` line put in, each is refused by name. (`timing.rs`' `REAL_WAITS` is a
/// census of test timing by bare name, a different subject, and is not held here: ruling R5.)
///
/// MUTATION: delete the registry's `std::sync::Condvar::wait_timeout_while` line and the live
/// half names it.
#[test]
fn every_blocking_primitive_is_a_window_wait_or_named_as_not_one() {
    let registry = std::fs::read_to_string(
        workspace_root()
            .join("crates")
            .join("bt-app")
            .join("src")
            .join("window_waits.tsv"),
    )
    .expect("the registry");
    let failures = blocking_alignment_failures(&registry);
    assert!(
        failures.is_empty(),
        "BLOCKING and the registry's `# vocabulary` disagree:\n  {}",
        failures.join("\n  ")
    );

    let park_timeout = "std::thread::park_timeout\tall\t";
    let without: String = registry
        .lines()
        .filter(|line| !line.starts_with(park_timeout))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_ne!(without, registry, "the live registry names `park_timeout`");
    assert_eq!(
        blocking_alignment_failures(&without),
        [
            "`std::thread::park_timeout` is a blocking primitive the vocabulary does not name and \
             `NOT_A_WINDOW_WAIT` does not excuse"
        ]
    );

    let wait = "std::sync::Condvar::wait\tall\twaits for a notification\n";
    assert!(
        registry.contains(wait),
        "the live registry names `Condvar::wait`"
    );
    let with_a_lock = registry.replacen(
        wait,
        &format!("{wait}std::sync::Mutex::lock\tall\ta planted lock\n"),
        1,
    );
    assert_eq!(
        blocking_alignment_failures(&with_a_lock),
        ["`std::sync::Mutex::lock` is both a vocabulary path and `NOT_A_WINDOW_WAIT`"]
    );
}

/// The kinds an `# effects` row may carry — `window_waits_tests::EFFECT_KINDS`, the registry's
/// own closed list. The first three are stops of the walk (a door body the walk reports and does
/// not enter); the rest are read by bt-app's tests and not by the walk.
const EFFECT_KINDS: [&str; 7] = [
    "owner-door-body",
    "interim-owner",
    "worker-door-body",
    "transport",
    "drop-exception",
    "shared-body",
    "emergency",
];

/// The item an `# effects` row names (plan row 76): the admitted callable of `package` with an
/// identity in the product whose spelling is `item` — its module path, then `Owner::`,
/// `<Owner as Trait>::` or `Trait::`, then the name (`crate::git::GitLocation::located`) — and
/// whose arm is `arm`: `-` for an identity on no predicate, else each predicate of the identity
/// (the declarations that reach the file, then the ones inside it) as `[predicate]`, joined by a
/// space (`[windows]`, `[not(windows)] [target_os = "macos"]`), as
/// `window_waits_tests::item_and_arm` spells a row. Every module path an item answers to is read,
/// not only the first. A row that names no item, or two on its arm, is refused: the walk's stop
/// would be lost, or would stop at whichever of the two the order put first.
fn effect_item(
    resolver: &Resolver<'_>,
    package: &str,
    arm: &str,
    item: &str,
) -> Result<ItemId, String> {
    let mut found: Vec<ItemId> = Vec::new();
    let mut elsewhere: BTreeSet<String> = BTreeSet::new();
    for id in resolver.callables(package) {
        if !resolver.admitted(id) {
            continue;
        }
        for identity in resolver.record(id).identities() {
            if !identity.variant.permits_product() {
                continue;
            }
            let mut spelled = format!("{}::", identity.module_path);
            match (&identity.type_owner, &identity.trait_name) {
                (Some(owner), Some(trait_name)) => {
                    spelled.push_str(&format!("<{owner} as {trait_name}>::"));
                }
                (Some(owner), None) => spelled.push_str(&format!("{owner}::")),
                (None, Some(trait_name)) => spelled.push_str(&format!("{trait_name}::")),
                (None, None) => {}
            }
            spelled.push_str(&identity.name);
            if spelled != item {
                continue;
            }
            let predicates = identity.variant.predicates();
            let on = if predicates.is_empty() {
                "-".to_owned()
            } else {
                predicates
                    .iter()
                    .map(|predicate| format!("[{predicate}]"))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            if on == arm {
                if !found.contains(&id) {
                    found.push(id);
                }
            } else {
                elsewhere.insert(on);
            }
        }
    }
    match found.as_slice() {
        [one] => Ok(*one),
        [] if elsewhere.is_empty() => Err("names no item of the product".to_owned()),
        [] => Err(format!(
            "names no item on that arm; the item stands on {}",
            elsewhere.into_iter().collect::<Vec<_>>().join(", ")
        )),
        two => Err(format!("names {} items on that arm", two.len())),
    }
}

/// A ledger key read as `window_waits_tests::door_key` reads it: an optional crate (`bt_…::`, else
/// `home`), an optional module path, an optional `impl` type (a capitalised segment) or
/// `<Type as Trait>`, and the name. Its items are every admitted callable of that crate with that
/// name, type, trait and module, at most one per `cfg` arm ([`arm_of`]): a key naming two items on
/// one arm (a bare name that two modules declare) is refused, and the module is to be written.
fn key_items(resolver: &Resolver<'_>, home: &str, key: &str) -> Result<BTreeSet<ItemId>, String> {
    let (package, rest) = match key.split_once("::") {
        Some((first, rest)) if first.starts_with("bt_") => (first.replace('_', "-"), rest),
        _ => (home.to_owned(), key),
    };
    let (module, owner, trait_name, name) = if let Some(inner) = rest.strip_prefix('<')
        && let Some((owner, after)) = inner.split_once(" as ")
        && let Some((trait_name, name)) = after.split_once(">::")
    {
        (
            None,
            Some(owner.to_owned()),
            Some(trait_name.to_owned()),
            name.to_owned(),
        )
    } else {
        let segments: Vec<&str> = rest.split("::").collect();
        let (name, before) = segments.split_last().ok_or("an empty key")?;
        let (owner, modules) = match before.split_last() {
            Some((owner, modules)) if owner.starts_with(|c: char| c.is_ascii_uppercase()) => {
                (Some((*owner).to_owned()), modules)
            }
            _ => (None, before),
        };
        (
            (!modules.is_empty()).then(|| format!("crate::{}", modules.join("::"))),
            owner,
            None,
            (*name).to_owned(),
        )
    };
    let found: BTreeSet<ItemId> = resolver
        .callables(&package)
        .into_iter()
        .filter(|id| {
            let record = resolver.record(*id);
            resolver.admitted(*id)
                && record.name() == name
                && record.type_owner() == owner.as_deref()
                && record.trait_name() == trait_name.as_deref()
                && module
                    .as_deref()
                    .is_none_or(|module| record.module_paths().contains(&module))
        })
        .collect();
    if found.is_empty() {
        return Err(format!("`{key}` names no item of the product"));
    }
    // A key names one item per `cfg` arm (`bt_platform::FolderPicker::request` is three, one per
    // platform's module), every one walked.
    let mut arms: BTreeMap<String, Vec<ItemId>> = BTreeMap::new();
    for id in &found {
        arms.entry(arm_of(resolver, *id)).or_default().push(*id);
    }
    if let Some(two) = arms.values().find(|ids| ids.len() > 1) {
        return Err(format!(
            "`{key}` names {} items on one `cfg` arm ({}): write the module",
            two.len(),
            two.iter()
                .map(|id| resolver.key(*id))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(found)
}

/// The `cfg` arm an item stands on: the predicates of every declaration path that reaches its file,
/// and those written inside the file.
fn arm_of(resolver: &Resolver<'_>, id: ItemId) -> String {
    let record = resolver.record(id);
    let paths: Vec<&[String]> = record
        .declaration_paths()
        .iter()
        .map(|path| path.predicates())
        .collect();
    format!("{paths:?} {:?}", record.variant())
}

/// The item's signature, read from its declaration (attributes, visibility, signature).
fn signature(resolver: &Resolver<'_>, id: ItemId) -> Option<syn::Signature> {
    let record = resolver.record(id);
    let text = resolver.index_of(id).text(record.declaration());
    syn::parse_str::<syn::ImplItemFn>(&format!("{} {{}}", text.trim_end().trim_end_matches(';')))
        .ok()
        .map(|function| function.sig)
}

/// The arguments a call writes for the item, its receiver not counted.
fn argument_count(signature: &syn::Signature) -> usize {
    signature
        .inputs
        .iter()
        .filter(|input| matches!(input, syn::FnArg::Typed(_)))
        .count()
}

/// Whether the item takes the lock it takes as an argument (`lane::lock<T>(&Mutex<T>)`): an
/// argument's type, or a generic bound, names `Mutex` or `RwLock`, as written or through a type
/// alias (`paths`, the argument types' paths with every alias followed: `fn lock(cell: &Cell)`
/// under `type Cell = Mutex<State>` takes a lock; the re-check of the review, R2). A type the
/// census does not type (a qualified associated type, `impl Trait`) is read as written only. Such
/// a helper locks whatever its caller hands it.
fn takes_a_lock(signature: &syn::Signature, paths: &BTreeSet<String>) -> bool {
    let placed = paths.iter().any(|path| {
        let name = path.rsplit("::").next().unwrap_or(path);
        name == "Mutex" || name == "RwLock"
    });
    struct Names(bool);
    impl<'ast> syn::visit::Visit<'ast> for Names {
        fn visit_path_segment(&mut self, segment: &'ast syn::PathSegment) {
            self.0 |= segment.ident == "Mutex" || segment.ident == "RwLock";
            syn::visit::visit_path_segment(self, segment);
        }
    }
    let mut names = Names(false);
    for input in &signature.inputs {
        if let syn::FnArg::Typed(typed) = input {
            syn::visit::Visit::visit_type(&mut names, &typed.ty);
        }
    }
    syn::visit::Visit::visit_generics(&mut names, &signature.generics);
    placed || names.0
}

/// A first-party path of `# vocabulary` or `# entrances` (`bt_pty::PtySession::resize`): every
/// admitted callable of that crate with that type and name, whatever module declares it (the path
/// is the crate's export).
fn exported_items(resolver: &Resolver<'_>, path: &str) -> Option<BTreeSet<ItemId>> {
    let (first, rest) = path.split_once("::")?;
    if !first.starts_with("bt_") {
        return None;
    }
    let package = first.replace('_', "-");
    let segments: Vec<&str> = rest.split("::").collect();
    let (name, before) = segments.split_last()?;
    let owner = before
        .last()
        .filter(|owner| owner.starts_with(|c: char| c.is_ascii_uppercase()))
        .copied();
    let found: BTreeSet<ItemId> = resolver
        .callables(&package)
        .into_iter()
        .filter(|id| {
            let record = resolver.record(*id);
            resolver.admitted(*id) && record.name() == *name && record.type_owner() == owner
        })
        .collect();
    Some(found)
}

const COMPLETION_COLUMNS: [&str; 8] = [
    "class",
    "door",
    "thread",
    "signal",
    "failure",
    "supersession",
    "sink",
    "status",
];
const REQUEST_DOOR_COLUMNS: [&str; 5] = ["door", "asked on", "boundary", "handed", "mailbox"];
const CUT_COLUMNS: [&str; 4] = ["door", "enqueue", "beside", "reason"];

/// Whether `reason` opens with its date, `YYYY-MM-DD: why`.
fn is_dated(reason: &str) -> bool {
    let bytes = reason.as_bytes();
    bytes.len() > 12
        && bytes[..10].iter().enumerate().all(|(at, byte)| {
            if at == 4 || at == 7 {
                *byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
        && bytes[10] == b':'
        && bytes[11] == b' '
}

/// The threads of a `# completions` row that are the window's or `main`'s own.
const OWNER_THREADS: [&str; 2] = ["window", "main"];

/// The cells of one list, split on `, `; `—` is none.
fn cell_list(cell: &str) -> Vec<&str> {
    if cell == "—" {
        Vec::new()
    } else {
        cell.split(", ").collect()
    }
}

/// The list's path.
fn sites_list() -> PathBuf {
    workspace_root()
        .join("docs")
        .join("plans")
        .join("REQUEST-DOOR-SITES.tsv")
}

/// The registry read: the rules every walk is judged against, the walked doors, the doors not
/// walked, and every cell that does not read.
struct Registry {
    rules: Rules,
    specs: Vec<DoorSpec>,
    not_walked: Vec<String>,
    refused: Vec<String>,
}

/// `registry` (`window_waits.tsv`'s shape: `# vocabulary`, `# entrances`, `# effects`,
/// `# completions`, `# request-doors`, `# request-door-cuts`) read over `resolver`; a key without a crate is `home`'s.
fn read_registry(resolver: &Resolver<'_>, registry: &str, home: &str) -> Registry {
    let mut refused: Vec<String> = Vec::new();
    let resolve = |line: usize, key: &str, refused: &mut Vec<String>| -> BTreeSet<ItemId> {
        key_items(resolver, home, key).unwrap_or_else(|why| {
            refused.push(format!("window_waits.tsv:{line}: {why}"));
            BTreeSet::new()
        })
    };

    // The rules, from the registry.
    let mut rules = Rules::default();
    for (line, cells) in numbered_section(registry, "vocabulary", &["path", "targets", "reason"]) {
        match exported_items(resolver, cells[0]) {
            Some(items) => {
                if items.is_empty() {
                    refused.push(format!(
                        "window_waits.tsv:{line}: the vocabulary path `{}` names no item",
                        cells[0]
                    ));
                }
                for id in items {
                    rules
                        .entrances
                        .insert(id, format!("vocabulary {}", cells[0]));
                }
            }
            None => {
                rules.vocabulary.insert(cells[0].to_owned());
            }
        }
    }
    for (line, cells) in numbered_section(registry, "entrances", &["path", "door"]) {
        let items = exported_items(resolver, cells[0]).unwrap_or_default();
        if items.is_empty() {
            refused.push(format!(
                "window_waits.tsv:{line}: the entrance `{}` names no item",
                cells[0]
            ));
        }
        for id in items {
            rules
                .entrances
                .insert(id, format!("entrance {} ({})", cells[0], cells[1]));
        }
    }
    let effects = numbered_section(
        registry,
        "effects",
        &[
            "crate",
            "arm",
            "item",
            "effects",
            "kind",
            "authority",
            "door",
        ],
    );
    for (line, cells) in &effects {
        let wanted = format!("{} {}", cells[0], cells[2]);
        if !EFFECT_KINDS.contains(&cells[4]) {
            refused.push(format!(
                "window_waits.tsv:{line}: `# effects` `{wanted}`'s kind `{}` is not one of {}",
                cells[4],
                EFFECT_KINDS.join(", ")
            ));
            continue;
        }
        let id = match effect_item(resolver, cells[0], cells[1], cells[2]) {
            Ok(id) => id,
            Err(why) => {
                refused.push(format!(
                    "window_waits.tsv:{line}: `# effects` `{wanted}` (arm `{}`) {why}",
                    cells[1]
                ));
                continue;
            }
        };
        let label = if cells[6].is_empty() {
            wanted
        } else {
            format!("{wanted} ({})", cells[6])
        };
        match cells[4] {
            "owner-door-body" | "interim-owner" => {
                rules.owner_bodies.insert(id, label);
            }
            "worker-door-body" => {
                rules.worker_bodies.insert(id, label);
            }
            _ => {}
        }
    }

    // The ledger's doors and the request doors' rows.
    let completions = numbered_section(registry, "completions", &COMPLETION_COLUMNS);
    let rows = numbered_section(registry, "request-doors", &REQUEST_DOOR_COLUMNS);
    let request_doors: BTreeSet<&str> = rows.iter().map(|(_, cells)| cells[0]).collect();
    for (line, cells) in &completions {
        let threads: Vec<&str> = cells[2]
            .split(", ")
            .map(|part| part.split_once(" (row ").map_or(part, |(thread, _)| thread))
            .collect();
        let owner = threads.iter().all(|thread| OWNER_THREADS.contains(thread));
        let standing = if !owner || cells[7] == "conforms" {
            Standing::PassThrough
        } else if cells[7] == "owner-thread" {
            Standing::OwnerThread
        } else {
            Standing::Repair(cells[7].to_owned())
        };
        if !owner && !request_doors.contains(cells[1]) {
            refused.push(format!(
                "window_waits.tsv:{line}: `{}` runs on `{}` and has no `# request-doors` row",
                cells[1], cells[2]
            ));
        }
        for id in resolve(*line, cells[1], &mut refused) {
            // A door on several rows keeps its strictest standing.
            let entry = rules
                .doors
                .entry(id)
                .or_insert_with(|| (cells[1].to_owned(), standing.clone()));
            if matches!(entry.1, Standing::PassThrough) {
                entry.1 = standing.clone();
            }
        }
    }
    let mut specs: Vec<DoorSpec> = Vec::new();
    let mut not_walked: Vec<String> = Vec::new();
    for (line, cells) in &rows {
        match cells[1] {
            "window" => {}
            "main (before the loop)" | "door process" => {
                let label = format!("{} ({})", cells[0], cells[1]);
                for id in resolve(*line, cells[0], &mut refused) {
                    rules.not_walked.insert(id, label.clone());
                }
                not_walked.push(label);
                continue;
            }
            other => {
                refused.push(format!(
                    "window_waits.tsv:{line}: `{}`: `asked on` is `{other}`",
                    cells[0]
                ));
                continue;
            }
        }
        let mut boundary = Vec::new();
        for spelled in cell_list(cells[2]) {
            let (written, worker) = match spelled.rsplit_once('@') {
                Some((written, at)) => match at.parse::<usize>() {
                    Ok(at) => (written, Some(at)),
                    Err(_) => {
                        refused.push(format!(
                            "window_waits.tsv:{line}: `{}`: `{spelled}`'s argument is not a number",
                            cells[0]
                        ));
                        continue;
                    }
                },
                None => (spelled, None),
            };
            let callee = if written.starts_with("std::") || written.starts_with("core::") {
                // A foreign callee's arguments are not read (the registry names no signature of
                // it), so its `@N` could be counted against nothing: refused (the re-check of
                // the review, R3) — except a standard thread spawn's closure, by [`STD_SPAWNS`]
                // (plan row 52, R6).
                if let Some(at) = worker
                    && !STD_SPAWNS.contains(&(written, at))
                {
                    let spawn = STD_SPAWNS.iter().find(|(path, _)| *path == written);
                    refused.push(match spawn {
                        Some((_, closure)) => format!(
                            "window_waits.tsv:{line}: `{}`: `{spelled}` names argument {at}, and \
                             `{written}`'s thread body is argument {closure} (the guard's table of \
                             standard spawns)",
                            cells[0]
                        ),
                        None => format!(
                            "window_waits.tsv:{line}: `{}`: `{spelled}` names an argument of a foreign callee, whose signature \
                             the guard does not read",
                            cells[0]
                        ),
                    });
                    continue;
                }
                Callee::Foreign(written.to_owned())
            } else {
                let items = resolve(*line, written, &mut refused);
                for id in items.iter().filter(|_| worker.is_some()) {
                    let count = signature(resolver, *id).as_ref().map(argument_count);
                    if count.is_none_or(|count| worker.is_some_and(|at| at >= count)) {
                        refused.push(format!(
                            "window_waits.tsv:{line}: `{}`: `{spelled}` names argument {}, and `{}` takes {}",
                            cells[0],
                            worker.unwrap_or_default(),
                            resolver.key(*id),
                            count.map_or_else(
                                || "no signature the guard reads".to_owned(),
                                |count| format!("{count}")
                            )
                        ));
                    }
                }
                Callee::Items(items)
            };
            boundary.push(Entry {
                spelled: spelled.to_owned(),
                callee,
                worker,
            });
        }
        // `—`: the door hands nothing to another thread (it arms a wait, or answers from what it
        // holds); its walk is judged all the same.
        if boundary.is_empty() && cells[2] != "—" {
            refused.push(format!(
                "window_waits.tsv:{line}: `{}` names no boundary",
                cells[0]
            ));
        }
        let mailbox: BTreeSet<ItemId> = cell_list(cells[4])
            .into_iter()
            .flat_map(|key| resolve(*line, key, &mut refused))
            .collect();
        let mut lock_helpers = BTreeSet::new();
        for id in &mailbox {
            match (signature(resolver, *id), resolver.argument_type_paths(*id)) {
                (Some(signature), Ok(paths)) => {
                    if takes_a_lock(&signature, &paths) {
                        lock_helpers.insert(*id);
                    }
                }
                (None, _) => refused.push(format!(
                    "window_waits.tsv:{line}: `{}`: the mailbox item `{}` has no signature the guard reads",
                    cells[0],
                    resolver.key(*id)
                )),
                (Some(_), Err(unread)) => refused.push(format!(
                    "window_waits.tsv:{line}: `{}`: the mailbox item `{}`'s argument types are not all placed: {}",
                    cells[0],
                    resolver.key(*id),
                    unread.join("; ")
                )),
            }
        }
        specs.push(DoorSpec {
            key: cells[0].to_owned(),
            roots: resolve(*line, cells[0], &mut refused).into_iter().collect(),
            boundary,
            handed: cell_list(cells[3])
                .into_iter()
                .flat_map(|key| resolve(*line, key, &mut refused))
                .collect(),
            mailbox,
            lock_helpers,
            cut: None,
        });
    }
    // The cuts (plan row 52): each row names a walked door, once, with a dated reason, an enqueue,
    // and cells that share no item.
    for (line, cells) in numbered_section(registry, "request-door-cuts", &CUT_COLUMNS) {
        let Some(spec) = specs.iter_mut().find(|spec| spec.key == cells[0]) else {
            refused.push(format!(
                "window_waits.tsv:{line}: the cut `{}` is not a `# request-doors` row asked on the \
                 window",
                cells[0]
            ));
            continue;
        };
        if spec.cut.is_some() {
            refused.push(format!(
                "window_waits.tsv:{line}: `{}` is cut on two rows",
                cells[0]
            ));
            continue;
        }
        if !is_dated(cells[3]) {
            refused.push(format!(
                "window_waits.tsv:{line}: `{}`'s cut reason is not dated (`YYYY-MM-DD: why`)",
                cells[0]
            ));
        }
        let mut read = |cell: &str| -> Vec<(String, BTreeSet<ItemId>)> {
            cell_list(cell)
                .into_iter()
                .map(|key| (key.to_owned(), resolve(line, key, &mut refused)))
                .collect()
        };
        let enqueue = read(cells[1]);
        let beside = read(cells[2]);
        if enqueue.is_empty() {
            refused.push(format!(
                "window_waits.tsv:{line}: `{}`'s cut names no enqueue",
                cells[0]
            ));
        }
        for (key, items) in &beside {
            if enqueue.iter().any(|(_, other)| !other.is_disjoint(items)) {
                refused.push(format!(
                    "window_waits.tsv:{line}: `{}`'s cut names `{key}` in both `enqueue` and \
                     `beside`",
                    cells[0]
                ));
            }
        }
        spec.cut = Some(Cut {
            line,
            enqueue,
            beside,
        });
    }
    Registry {
        rules,
        specs,
        not_walked,
        refused,
    }
}

/// RED — **every request door returns at once** (C1, the side-effects contract (b)2): each
/// `# request-doors` row asked on the window thread, walked to its boundary, and the program's
/// implicit impls walked once, meet nothing [`Judge::walk`] refuses that
/// `docs/plans/REQUEST-DOOR-SITES.tsv` does not list — and the list is the tree's, key by key and
/// count by count. Prints each door's numbers (`REQUEST_DOOR`) and the list's counts by verdict;
/// leaves the rendering (the committed verdicts and reasons kept, a new key `unreviewed`) in
/// `target/request-door-sites/`, which `scripts/generate-request-door-sites.ps1` copies.
///
/// MUTATIONS (the `gates-can-fail` canaries): a helper writing a file called from
/// `handoff_lane::HandoffLane::submit`, and a `Box<dyn Fn()>` called there — each is a key in the
/// tree and not on the list, and the test names it.
#[test]
fn every_request_door_returns_at_once() {
    let registry = std::fs::read_to_string(
        workspace_root()
            .join("crates")
            .join("bt-app")
            .join("src")
            .join("window_waits.tsv"),
    )
    .expect("the registry");
    let held = real_tree();
    let packages: Vec<(&str, &Index)> = held
        .iter()
        .map(|(name, index)| (name.as_str(), &**index))
        .collect();
    let resolver =
        Resolver::new(&packages, Arms::product()).unwrap_or_else(|refusals| panic!("{refusals:?}"));
    let Registry {
        rules,
        specs,
        not_walked,
        refused,
    } = read_registry(&resolver, &registry, "bt-app");
    assert!(
        refused.is_empty(),
        "the registry's request doors are not read:\n  {}",
        refused.join("\n  ")
    );

    // The list.
    let committed = std::fs::read_to_string(sites_list())
        .expect("docs/plans/REQUEST-DOOR-SITES.tsv, the committed list");
    let (list, lines) =
        read_list(&committed).unwrap_or_else(|why| panic!("REQUEST-DOOR-SITES.tsv: {why}"));
    let reviewed: BTreeMap<SiteKey, (Verdict, String)> = list
        .iter()
        .map(|(key, (_, verdict, reason))| (key.clone(), (verdict.clone(), reason.clone())))
        .collect();
    let (follows, unfollowed) = followed(&resolver, &reviewed, |row| {
        format!("REQUEST-DOOR-SITES.tsv:{}", lines[row])
    });
    assert!(
        unfollowed.is_empty(),
        "the list's `follows` rows are not read:\n  {}",
        unfollowed.join("\n  ")
    );
    let root = workspace_root()
        .canonicalize()
        .expect("the workspace root resolves");
    let graph = CallGraph::new(&resolver);
    let judge = Judge {
        graph: &graph,
        rules: &rules,
        follows: &follows,
        root,
    };

    // The walks.
    let mut findings: Vec<Finding> = Vec::new();
    // Which doors meet each key: the rendering's `paths.tsv` says so, for the reader of a row.
    let mut doors_of: BTreeMap<SiteKey, BTreeSet<String>> = BTreeMap::new();
    let mut never: Vec<String> = Vec::new();
    for spec in &specs {
        let judged = judge.walk(spec, false);
        let open: BTreeMap<SiteKey, BTreeSet<Site>> = count(&judged.findings);
        let mut by_verdict: BTreeMap<String, usize> = BTreeMap::new();
        for key in open.keys() {
            let verdict = reviewed.get(key).map_or_else(
                || "unlisted".to_owned(),
                |(verdict, _)| match verdict {
                    Verdict::Follows(_) => "follows".to_owned(),
                    other => other.spelled(),
                },
            );
            *by_verdict.entry(verdict).or_default() += 1;
        }
        eprintln!(
            "REQUEST_DOOR\t{}\treached {}\tvalues {}\tforeign {}\tunresolved {:?}\tkeys {:?}",
            spec.key,
            judged.reached.len(),
            judged.values,
            judged.foreign,
            judged.unresolved,
            by_verdict
        );
        never.extend(judged.refused);
        for finding in &judged.findings {
            doors_of
                .entry(finding.key.clone())
                .or_default()
                .insert(spec.key.clone());
        }
        findings.extend(judged.findings);
    }
    // The beside walks (plan row 52): what a cut door's `beside` side meets stays listed when it
    // is a breach whatever is read, or a row already read; an unreviewed edge met there is
    // counted and rendered, not listed.
    let mut beside_left: BTreeMap<SiteKey, BTreeSet<String>> = BTreeMap::new();
    for spec in specs.iter().filter(|spec| spec.cut.is_some()) {
        let judged = judge.walk_on(spec, false, Side::Beside);
        never.extend(judged.refused);
        let (kept, left) = split_beside(judged.findings, &reviewed);
        eprintln!(
            "REQUEST_DOOR_BESIDE\t{}\tkept {}\tleft {}",
            spec.key,
            kept.len(),
            left.len()
        );
        for finding in &kept {
            doors_of
                .entry(finding.key.clone())
                .or_default()
                .insert(format!("{} (beside)", spec.key));
        }
        findings.extend(kept);
        for finding in left {
            beside_left
                .entry(finding.key)
                .or_default()
                .insert(spec.key.clone());
        }
    }
    let implicit_roots: Vec<ItemId> = resolver.implicit().into_values().flatten().collect();
    let implicit = DoorSpec {
        key: "implicit".to_owned(),
        roots: implicit_roots,
        boundary: specs
            .iter()
            .flat_map(|spec| spec.boundary.iter().cloned())
            .collect(),
        handed: specs
            .iter()
            .flat_map(|spec| spec.handed.iter().copied())
            .collect(),
        mailbox: specs
            .iter()
            .flat_map(|spec| spec.mailbox.iter().copied())
            .collect(),
        lock_helpers: specs
            .iter()
            .flat_map(|spec| spec.lock_helpers.iter().copied())
            .collect(),
        cut: None,
    };
    let judged = judge.walk(&implicit, true);
    eprintln!(
        "REQUEST_DOOR\t(implicit, {} impl methods)\treached {}\tforeign {}\tunresolved {:?}",
        implicit.roots.len(),
        judged.reached.len(),
        judged.foreign,
        judged.unresolved
    );
    never.extend(judged.refused);
    // An implicit impl may run on any door's road or on none: its findings are not the site's but
    // the impl's, one row per impl method, its `sites` the distinct sites behind it, its verdict
    // read against where the impl's values live (a worker's own `Read`, the closed `Drop`
    // inventory D-78 to D-82) or `violation`.
    findings.extend(
        judged
            .findings
            .into_iter()
            .filter(|finding| finding.key.2 != "Unmet")
            .map(|finding| {
                let Node::Item(root) = finding.root else {
                    unreachable!("the implicit walk's roots are impl methods")
                };
                let record = resolver.record(root);
                Finding {
                    key: (
                        judge.item_file(root),
                        resolver.key(root),
                        "Implicit".to_owned(),
                        record.trait_name().unwrap_or_default().to_owned(),
                    ),
                    under: Some(finding.key.clone()),
                    ..finding
                }
            }),
    );
    for finding in &findings {
        doors_of.entry(finding.key.clone()).or_default();
    }
    for door in &not_walked {
        eprintln!("REQUEST_DOOR\t{door}\tnot walked: C1 is the window thread's");
    }
    let never: BTreeSet<String> = never.into_iter().collect();
    assert!(
        never.is_empty(),
        "the walks met what no list may excuse:\n  {}",
        never.into_iter().collect::<Vec<_>>().join("\n  ")
    );

    // The tree against the list, and the rendering.
    let mut example: BTreeMap<SiteKey, Vec<String>> = BTreeMap::new();
    for finding in &findings {
        example
            .entry(finding.key.clone())
            .or_insert_with(|| finding.path.clone());
    }
    let tree: BTreeMap<SiteKey, usize> = count(&findings)
        .into_iter()
        .map(|(key, sites)| (key, sites.len()))
        .collect();
    // The keys met only beside a cut, unread: rendered, not listed.
    beside_left.retain(|key, _| !tree.contains_key(key));
    let mut rendering = String::from(
        "# REQUEST-DOOR-SITES - generated by scripts/generate-request-door-sites.ps1 from bt-source's\n\
         # request_doors test (T-REQUEST-DOOR-GUARD): every failure the C1 guard meets walking each\n\
         # request door of crates/bt-app/src/window_waits.tsv's `# request-doors` to its boundary, and\n\
         # the program's implicit impls once, by site (file, item, kind, name; never a line) with the\n\
         # number of sites behind it. Verdicts: std / follows <key>; ... (read: nothing behind it),\n\
         # unreviewed (not read yet), violation (a C1 breach; the reason names its finding). Held equal to\n\
         # the tree by the test; shrink-only against the merge base by\n\
         # scripts/ci/check-request-door-sites.ps1 - a key added or grown, a verdict moved to unreviewed\n\
         # or violation or away from violation, or a follows row re-pointed, carries a new reason\n\
         # 'YYYY-MM-DD: <why>'. An unread row's reason is the one Q8 (B) gives the seed.\n",
    );
    rendering.push_str(&format!(
        "# 2026-11-02 (plan row 52): a door of `# request-door-cuts` is walked to its boundary from its\n\
         # enqueue; its `beside` side keeps a breach and a read row here, and {} unread keys met only\n\
         # beside a cut are not listed (target/request-door-sites/beside.tsv names them).\n",
        beside_left.len()
    ));
    rendering.push_str(LIST_HEADER);
    rendering.push('\n');
    let mut totals: BTreeMap<String, usize> = BTreeMap::new();
    for (key, sites) in &tree {
        let (verdict, reason) = list.get(key).map_or_else(
            || (Verdict::Unreviewed.spelled(), String::new()),
            |(_, verdict, reason)| (verdict.spelled(), reason.clone()),
        );
        *totals
            .entry(verdict.split(' ').next().unwrap_or_default().to_owned())
            .or_default() += 1;
        rendering.push_str(&format!(
            "{}\t{}\t{}\t{}\t{sites}\t{verdict}\t{reason}\n",
            key.0, key.1, key.2, key.3
        ));
    }
    let out = workspace_root().join("target").join("request-door-sites");
    std::fs::create_dir_all(&out).expect("target/request-door-sites");
    std::fs::write(out.join("REQUEST-DOOR-SITES.tsv"), &rendering).expect("the rendering");
    let mut paths = String::new();
    for (key, path) in &example {
        let doors = doors_of.get(key).map_or_else(String::new, |doors| {
            if doors.is_empty() {
                "(implicit)".to_owned()
            } else {
                doors.iter().cloned().collect::<Vec<_>>().join(", ")
            }
        });
        paths.push_str(&format!(
            "{}\t{}\t{}\t{}\t{doors}\t{}\n",
            key.0,
            key.1,
            key.2,
            key.3,
            path.join(" > ")
        ));
    }
    std::fs::write(out.join("paths.tsv"), &paths).expect("the paths");
    let mut beside = String::new();
    for (key, doors) in &beside_left {
        beside.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            key.0,
            key.1,
            key.2,
            key.3,
            doors.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    std::fs::write(out.join("beside.tsv"), &beside).expect("the keys met only beside a cut");
    eprintln!(
        "REQUEST_DOOR_SITES\tkeys {}\tsites {}\tby verdict {totals:?}",
        tree.len(),
        tree.values().sum::<usize>()
    );
    // What stands behind each implicit impl's row: its verdict is held by those kinds.
    let mut under: BTreeMap<SiteKey, BTreeSet<String>> = BTreeMap::new();
    for finding in &findings {
        if let Some(behind) = &finding.under {
            under
                .entry(finding.key.clone())
                .or_default()
                .insert(behind.2.clone());
        }
    }
    // For the reader of an `Implicit` row: the kinds behind it.
    let mut behind = String::new();
    for (key, kinds) in &under {
        behind.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            key.0,
            key.1,
            key.2,
            key.3,
            kinds.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    std::fs::write(out.join("implicit-under.tsv"), &behind).expect("the implicit rows' kinds");
    let failures = list_failures(&tree, &list, &doors_of, &under);
    assert!(
        failures.is_empty(),
        "docs/plans/REQUEST-DOOR-SITES.tsv and the tree disagree ({} differences; the rendering is in \
         target/request-door-sites/, regenerate with scripts/generate-request-door-sites.ps1):\n  {}",
        failures.len(),
        failures
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}
