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
//!   `std::env::set_current_dir` (C1 names the file system; ruling Q3); a
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

use bt_source::call_graph::{Arms, CallGraph, ItemId, Node, Resolver, Target, Unresolved, Via};
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
        || EFFECT_NAMESPACES
            .iter()
            .any(|namespace| path.starts_with(namespace))
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
    reviewed: &'g BTreeMap<SiteKey, (Verdict, String)>,
    /// Every callable by its key, for `follows`.
    by_key: &'g BTreeMap<String, ItemId>,
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

    /// The walk from `spec`'s roots. A door-level finding names its owner: the door, or for the
    /// implicit walk (`implicit`), the root the path starts at.
    fn walk(&self, spec: &DoorSpec, implicit: bool) -> Judged {
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
                        if let Some((Verdict::Follows(keys), _)) = self.reviewed.get(&found.key) {
                            next.extend(keys.iter().filter_map(|key| self.by_key.get(key)));
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
        for (at, entry) in spec.boundary.iter().enumerate() {
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
    list: &BTreeMap<SiteKey, (usize, Verdict, String)>,
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

/// The list's text read: `file item kind name sites verdict reason`.
fn read_list(text: &str) -> Result<BTreeMap<SiteKey, (usize, Verdict, String)>, String> {
    let mut rows = BTreeMap::new();
    let mut header = false;
    for line in text.lines() {
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
    }
    if !header {
        return Err("the list has no header".to_owned());
    }
    Ok(rows)
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

/// Every callable of `resolver`, by its key.
fn keyed(resolver: &Resolver<'_>) -> BTreeMap<String, ItemId> {
    resolver
        .packages()
        .iter()
        .flat_map(|package| resolver.callables(package))
        .filter(|id| resolver.admitted(*id))
        .map(|id| (resolver.key(id), id))
        .collect()
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
    let index = fixture_index("request_doors");
    let resolver = Resolver::new(&[(FIXTURE_HOME, &index)], Arms::product())
        .unwrap_or_else(|refusals| panic!("{refusals:?}"));
    let by_key = keyed(&resolver);
    let read = read_registry(&resolver, &by_key, registry, FIXTURE_HOME);
    assert!(read.refused.is_empty(), "{:?}", read.refused);
    let reviewed = fixture_reviewed();
    let root = workspace_root()
        .canonicalize()
        .expect("the workspace root resolves");
    let graph = CallGraph::new(&resolver);
    let judge = Judge {
        graph: &graph,
        rules: &read.rules,
        reviewed: &reviewed,
        by_key: &by_key,
        root,
    };
    let spec = read
        .specs
        .iter()
        .find(|spec| spec.key == door)
        .unwrap_or_else(|| panic!("`{door}` is a door of the fixture's registry"));
    judge.walk(spec, false)
}

/// The fixture's door `door` judged: its findings' kinds and names that no reviewed row reads,
/// sorted.
fn judge_fixture(door: &str) -> Vec<String> {
    let judged = fixture_walk(&fixture_registry(), door);
    assert!(judged.refused.is_empty(), "{:?}", judged.refused);
    let reviewed = fixture_reviewed();
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
    let by_key = keyed(&resolver);
    read_registry(&resolver, &by_key, registry, FIXTURE_HOME).refused
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

/// RED (review nits 3, 4) — **the registry's cells read or are refused**: a bare key that names
/// one name of two modules on one arm is refused, a key qualified by its module names one item,
/// and a key naming one item per platform's module is admitted; a first-party `# vocabulary` path
/// that names nothing is refused, as an `# entrances` one is; a boundary's worker argument past
/// the item's arguments is refused.
///
/// MUTATIONS: drop the one-arm check in [`key_items`] — `twin` is admitted; drop the empty
/// vocabulary refusal in [`read_registry`] — the planted path passes; drop the argument count —
/// `spawn@5` passes.
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
        ["the vocabulary path `bt_rd::nothing_here` names no item"]
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

/// One section of the registry: its rows' cells, the header checked.
fn section<'r>(registry: &'r str, name: &str, columns: &[&str]) -> Vec<Vec<&'r str>> {
    let marker = format!("# {name}");
    let mut lines = registry
        .lines()
        .skip_while(|line| *line != marker)
        .skip(1)
        .take_while(|line| !line.starts_with("# "))
        .filter(|line| !line.is_empty() && !line.starts_with("##"));
    let header: Vec<&str> = lines
        .next()
        .unwrap_or_else(|| panic!("the registry has a `{marker}` section"))
        .split('\t')
        .collect();
    assert_eq!(header, columns, "`{marker}`'s columns");
    lines
        .map(|line| {
            let cells: Vec<&str> = line.split('\t').collect();
            assert_eq!(cells.len(), columns.len(), "`{marker}`: {line}");
            cells
        })
        .collect()
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
/// argument's type, or a generic bound, names `Mutex` or `RwLock`. Such a helper locks whatever its
/// caller hands it.
fn takes_a_lock(signature: &syn::Signature) -> bool {
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
    names.0
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
/// `# completions`, `# request-doors`) read over `resolver`; a key without a crate is `home`'s.
fn read_registry(
    resolver: &Resolver<'_>,
    by_key: &BTreeMap<String, ItemId>,
    registry: &str,
    home: &str,
) -> Registry {
    let mut refused: Vec<String> = Vec::new();
    let resolve = |key: &str, refused: &mut Vec<String>| -> BTreeSet<ItemId> {
        key_items(resolver, home, key).unwrap_or_else(|why| {
            refused.push(why);
            BTreeSet::new()
        })
    };

    // The rules, from the registry.
    let mut rules = Rules::default();
    for cells in section(registry, "vocabulary", &["path", "targets", "reason"]) {
        match exported_items(resolver, cells[0]) {
            Some(items) => {
                if items.is_empty() {
                    refused.push(format!("the vocabulary path `{}` names no item", cells[0]));
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
    for cells in section(registry, "entrances", &["path", "door"]) {
        let items = exported_items(resolver, cells[0]).unwrap_or_default();
        if items.is_empty() {
            refused.push(format!("the entrance `{}` names no item", cells[0]));
        }
        for id in items {
            rules
                .entrances
                .insert(id, format!("entrance {} ({})", cells[0], cells[1]));
        }
    }
    let effects = section(
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
    for cells in &effects {
        let wanted = format!("{} {}", cells[0], cells[2]);
        let items: Vec<ItemId> = by_key
            .iter()
            .filter(|(key, _)| **key == wanted)
            .map(|(_, id)| *id)
            .collect();
        let label = if cells[6].is_empty() {
            wanted.clone()
        } else {
            format!("{wanted} ({})", cells[6])
        };
        for id in items {
            match cells[4] {
                "owner-door-body" | "interim-owner" => {
                    rules.owner_bodies.insert(id, label.clone());
                }
                "worker-door-body" => {
                    rules.worker_bodies.insert(id, label.clone());
                }
                _ => {}
            }
        }
    }

    // The ledger's doors and the request doors' rows.
    let completions = section(registry, "completions", &COMPLETION_COLUMNS);
    let rows = section(registry, "request-doors", &REQUEST_DOOR_COLUMNS);
    let request_doors: BTreeSet<&str> = rows.iter().map(|cells| cells[0]).collect();
    for cells in &completions {
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
                "`{}` runs on `{}` and has no `# request-doors` row",
                cells[1], cells[2]
            ));
        }
        for id in resolve(cells[1], &mut refused) {
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
    for cells in &rows {
        match cells[1] {
            "window" => {}
            "main (before the loop)" | "door process" => {
                let label = format!("{} ({})", cells[0], cells[1]);
                for id in resolve(cells[0], &mut refused) {
                    rules.not_walked.insert(id, label.clone());
                }
                not_walked.push(label);
                continue;
            }
            other => {
                refused.push(format!("`{}`: `asked on` is `{other}`", cells[0]));
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
                            "`{}`: `{spelled}`'s argument is not a number",
                            cells[0]
                        ));
                        continue;
                    }
                },
                None => (spelled, None),
            };
            let callee = if written.starts_with("std::") || written.starts_with("core::") {
                Callee::Foreign(written.to_owned())
            } else {
                let items = resolve(written, &mut refused);
                for id in items.iter().filter(|_| worker.is_some()) {
                    let count = signature(resolver, *id).as_ref().map(argument_count);
                    if count.is_none_or(|count| worker.is_some_and(|at| at >= count)) {
                        refused.push(format!(
                            "`{}`: `{spelled}` names argument {}, and `{}` takes {}",
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
            refused.push(format!("`{}` names no boundary", cells[0]));
        }
        let mailbox: BTreeSet<ItemId> = cell_list(cells[4])
            .into_iter()
            .flat_map(|key| resolve(key, &mut refused))
            .collect();
        let mut lock_helpers = BTreeSet::new();
        for id in &mailbox {
            match signature(resolver, *id) {
                Some(signature) => {
                    if takes_a_lock(&signature) {
                        lock_helpers.insert(*id);
                    }
                }
                None => refused.push(format!(
                    "`{}`: the mailbox item `{}` has no signature the guard reads",
                    cells[0],
                    resolver.key(*id)
                )),
            }
        }
        specs.push(DoorSpec {
            key: cells[0].to_owned(),
            roots: resolve(cells[0], &mut refused).into_iter().collect(),
            boundary,
            handed: cell_list(cells[3])
                .into_iter()
                .flat_map(|key| resolve(key, &mut refused))
                .collect(),
            mailbox,
            lock_helpers,
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
    let by_key = keyed(&resolver);
    let Registry {
        rules,
        specs,
        not_walked,
        refused,
    } = read_registry(&resolver, &by_key, &registry, "bt-app");
    assert!(
        refused.is_empty(),
        "the registry's request doors are not read:\n  {}",
        refused.join("\n  ")
    );

    // The list.
    let committed = std::fs::read_to_string(sites_list())
        .expect("docs/plans/REQUEST-DOOR-SITES.tsv, the committed list");
    let list = read_list(&committed).unwrap_or_else(|why| panic!("REQUEST-DOOR-SITES.tsv: {why}"));
    let reviewed: BTreeMap<SiteKey, (Verdict, String)> = list
        .iter()
        .map(|(key, (_, verdict, reason))| (key.clone(), (verdict.clone(), reason.clone())))
        .collect();
    let root = workspace_root()
        .canonicalize()
        .expect("the workspace root resolves");
    let graph = CallGraph::new(&resolver);
    let judge = Judge {
        graph: &graph,
        rules: &rules,
        reviewed: &reviewed,
        by_key: &by_key,
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
