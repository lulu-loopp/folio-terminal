//! **C1 as a gate: a request door returns at once** (T-REQUEST-DOOR-GUARD, S-2b of
//! `docs/plans/design/side-effects-contract-2026-10-11.md`, revision (b)2 and (c)2). A guard: its
//! subject is how the code is written.
//!
//! Each `# request-doors` row of `crates/bt-app/src/window_waits.tsv` asked on the window thread is
//! walked over `bt_source::call_graph` from its door to its boundary, and the walk fails on:
//!
//! * **(i) an effect**: a foreign callee that is a `# vocabulary` path, or under `std::fs`,
//!   `std::net`, `std::process` or the `std::io` read and write traits and handles (C1 names the
//!   file system; ruling Q3); a first-party `# vocabulary` entry reached;
//! * **(ii) an `# entrances` item** reached;
//! * **(iii) an `owner-door-body`** of `# effects` reached, and **(iii-b) a `worker-door-body`**
//!   reached (a worker body run inline), or a `handed` item called rather than named as a value;
//! * **(iv) a blocking primitive** ([`BLOCKING`]) whose caller is not one of the row's `mailbox`
//!   items;
//! * **(v) every unresolved edge** ([`Unresolved`]) except a macro of [`ALLOWED_MACROS`] and a
//!   callee a `derive` or `#[from]` generates (its body calls the derived trait's impls on the
//!   fields, which (vii) walks); `Cutoff` and `Unparsed` fail always and no list excuses them;
//! * **(vi) another ledger door reached** that runs on the window thread and does not conform (a
//!   `row <n>` or `D-<n>` status), or an `owner-thread` door (C7, listable per (b)6);
//! * **a boundary entry never met**: the row names a call its walk does not reach.
//!
//! **(vii)** every first-party impl of a foreign trait (`Resolver::implicit`: a `Drop`, an operator,
//! a callback) is walked once as a root of its own under the union of every row's cells. Such an
//! impl may run on any door's road or on none, so what it meets is the impl's, not the site's: one
//! `Implicit` row per impl method, its `sites` the findings behind it.
//!
//! The walk follows every resolved edge, an `Ambiguous` edge with exactly one candidate (the door
//! reach's rule), and the items a reviewed row names (`follows`). A row's `boundary` names the calls
//! at which its request leaves the thread (an enqueue, a notify, the thread door, a first-party
//! item that hands a closure to a worker, or another request door): the walk does not enter a
//! closure literal written as an argument of one ([`call_graph::Handed`]), which is the worker's,
//! and walks the call's own body, which is not. `—` is a door that hands nothing over. A `handed`
//! item (a worker's entry named as a value) is a stop. It does not enter a `const`'s or a plain
//! `static`'s initializer, which the compiler evaluates (a function it names runs where that field
//! is called, a `Dynamic` edge), and does enter a `LazyLock`'s, which runs on its first deref.
//! Every other ledger door, an owner door's or a worker's body and an entrance are stops: the
//! finding names the breach, and what runs behind it is judged by its own row.
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
    "std::thread::sleep",
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
];

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
}

/// One boundary entry of a row.
#[derive(Clone, Debug)]
enum Entry {
    /// A foreign callee, by full path.
    Foreign(String),
    /// A first-party call, by its key and items.
    Items(String, BTreeSet<ItemId>),
}

impl Entry {
    fn spelled(&self) -> &str {
        match self {
            Self::Foreign(path) => path,
            Self::Items(key, _) => key,
        }
    }
}

/// One walked door: its roots and its row's cells.
struct DoorSpec {
    key: String,
    roots: Vec<ItemId>,
    boundary: Vec<Entry>,
    handed: BTreeSet<ItemId>,
    mailbox: BTreeSet<ItemId>,
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
        || EFFECT_NAMESPACES
            .iter()
            .any(|namespace| path.starts_with(namespace))
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
            let mut handed_to: BTreeSet<Span> = BTreeSet::new();
            let mut next: Vec<ItemId> = Vec::new();
            let mut next_values = Vec::new();
            for edge in edges.iter() {
                if edge.site.closures.iter().any(|open| {
                    open.handed
                        .is_some_and(|handed| handed_to.contains(&handed.call))
                }) {
                    continue;
                }
                for (at, entry) in spec.boundary.iter().enumerate() {
                    let hit = match (entry, &edge.to) {
                        (Entry::Foreign(path), Target::Foreign(callee)) => {
                            callee.join("::") == *path
                        }
                        (Entry::Items(_, items), Target::Item(id)) => items.contains(id),
                        _ => false,
                    };
                    if hit {
                        met.insert(at);
                        handed_to.insert(edge.site.span);
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
                    // A `LazyLock`'s initializer runs on its first deref, perhaps here. A `const`'s
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
                        entry.spelled().to_owned(),
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
/// kind does not admit.
fn list_failures(
    tree: &BTreeMap<SiteKey, usize>,
    list: &BTreeMap<SiteKey, (usize, Verdict, String)>,
    doors: &BTreeMap<SiteKey, BTreeSet<String>>,
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
        if (DOOR_KINDS.contains(&key.2.as_str()) || key.2 == "Effect" || key.2 == "Blocking")
            && *verdict != Verdict::Violation
        {
            {
                failures.push(format!(
                    "{key:?}: a `{}` row is a C1 breach, not a call to read: its verdict is \
                     `violation` with the finding named, never `{}`",
                    key.2,
                    verdict.spelled()
                ));
            }
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

fn fixture_index(directory: &str) -> Arc<Index> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("call_graph")
        .join(directory);
    let universe = Universe::declare(
        format!("the request-door guard's {directory} fixture"),
        vec![TargetRoot {
            id: TargetId {
                package: "rd".to_owned(),
                kind: TargetKind::Library,
                name: "rd".to_owned(),
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

/// The fixture's callable `rd crate::<key>`.
fn fixture_item(by_key: &BTreeMap<String, ItemId>, key: &str) -> ItemId {
    *by_key
        .get(&format!("rd crate::{key}"))
        .unwrap_or_else(|| panic!("`{key}` is a callable of the fixture: {:?}", by_key.keys()))
}

/// The fixture's door `door` judged: its findings' kinds and names, sorted, against the fixture's
/// own reviewed rows.
fn judge_fixture(door: &str) -> Vec<String> {
    let index = fixture_index("request_doors");
    let resolver = Resolver::new(&[("rd", &index)], Arms::product())
        .unwrap_or_else(|refusals| panic!("{refusals:?}"));
    let by_key = keyed(&resolver);
    let item = |key: &str| fixture_item(&by_key, key);
    let mut rules = Rules::default();
    rules.vocabulary.insert("std::thread::sleep".to_owned());
    rules
        .owner_bodies
        .insert(item("present"), "rd crate::present (Present)".to_owned());
    rules
        .worker_bodies
        .insert(item("Lane::serve"), "rd crate::Lane::serve".to_owned());
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("the workspace root resolves");
    let graph = CallGraph::new(&resolver);
    // The fixture's reviewed rows: the stored refusal hook, and the calls on a lock's result and
    // guard, which the census does not type (`LockResult::unwrap_or_else`, the guard's `len` and
    // `push_back`) - read, as the real tree's list reads them.
    let lib = file_of(
        &resolver,
        Node::Item(item("Lane::offer")),
        resolver.record(item("Lane::offer")).whole().start(),
        &root,
    );
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
        ("SEED", "Ambiguous", "is_ok", "an io::Result's"),
    ] {
        reviewed.insert(
            (
                lib.clone(),
                format!("rd crate::{owner}"),
                kind.to_owned(),
                name.to_owned(),
            ),
            (Verdict::Std, format!("2026-10-24: {why}")),
        );
    }
    let judge = Judge {
        graph: &graph,
        rules: &rules,
        reviewed: &reviewed,
        by_key: &by_key,
        root,
    };
    let spec = DoorSpec {
        key: format!("rd crate::{door}"),
        roots: vec![item(door)],
        boundary: vec![
            Entry::Foreign("std::sync::Condvar::notify_one".to_owned()),
            Entry::Items(
                "rd crate::spawn".to_owned(),
                std::iter::once(item("spawn")).collect(),
            ),
        ],
        handed: BTreeSet::new(),
        mailbox: std::iter::once(item("Lane::mailbox")).collect(),
    };
    let judged = judge.walk(&spec, false);
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

/// RED — **the clean door passes**: its mailbox lock, its notify, the worker closure it hands to
/// `spawn` (whose body sleeps and locks the worker's own state) and its reviewed hook are all
/// within C1.
///
/// MUTATION: in `Judge::walk`, do not skip a closure handed to a boundary call — the handed
/// worker's body, `Lane::serve`, is judged as run inline.
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
        ["OwnerDoor rd crate::present (Present)"]
    );
    assert_eq!(
        judge_fixture("serve_inline"),
        [
            "Unmet rd crate::spawn",
            "Unmet std::sync::Condvar::notify_one",
            "WorkerBody rd crate::Lane::serve",
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

/// RED — **the list refuses what it may not excuse**: a `Cutoff` row, and a door-level or effect
/// row whose verdict is not `violation`.
///
/// MUTATION: drop the verdict-by-kind check in [`list_failures`] — the `std` effect row passes.
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
    let mut list = BTreeMap::new();
    list.insert(key("Effect"), (1, Verdict::Std, String::new()));
    list.insert(key("Cutoff"), (1, Verdict::Unreviewed, String::new()));
    let failures = list_failures(&tree, &list, &BTreeMap::new());
    assert_eq!(failures.len(), 2, "{failures:?}");
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

/// A ledger key read as `window_waits_tests::door_key` reads it: an optional crate (`bt_…::`), an
/// optional module path, an optional `impl` type (a capitalised segment) or `<Type as Trait>`, and
/// the name. Its items are every admitted callable of that crate with that name, type, trait and
/// module; a key naming items of more than one module is refused.
fn key_items(resolver: &Resolver<'_>, key: &str) -> Result<BTreeSet<ItemId>, String> {
    let (package, rest) = match key.split_once("::") {
        Some((first, rest)) if first.starts_with("bt_") => (first.replace('_', "-"), rest),
        _ => ("bt-app".to_owned(), key),
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
    // A key names one item per `cfg` arm (`bt_platform::FolderPicker::request` is three, one per
    // platform's module); bt-app's `door_records` refuses two on one arm, so every item is walked.
    if found.is_empty() {
        return Err(format!("`{key}` names no item of the product"));
    }
    Ok(found)
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
    let mut refused: Vec<String> = Vec::new();
    let resolve = |key: &str, refused: &mut Vec<String>| -> BTreeSet<ItemId> {
        key_items(&resolver, key).unwrap_or_else(|why| {
            refused.push(why);
            BTreeSet::new()
        })
    };

    // The rules, from the registry.
    let mut rules = Rules::default();
    for cells in section(&registry, "vocabulary", &["path", "targets", "reason"]) {
        match exported_items(&resolver, cells[0]) {
            Some(items) => {
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
    for cells in section(&registry, "entrances", &["path", "door"]) {
        let items = exported_items(&resolver, cells[0]).unwrap_or_default();
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
        &registry,
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
    let completions = section(&registry, "completions", &COMPLETION_COLUMNS);
    let rows = section(&registry, "request-doors", &REQUEST_DOOR_COLUMNS);
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
                not_walked.push(format!("{} ({})", cells[0], cells[1]));
                continue;
            }
            other => {
                refused.push(format!("`{}`: `asked on` is `{other}`", cells[0]));
                continue;
            }
        }
        let mut boundary = Vec::new();
        for entry in cell_list(cells[2]) {
            if entry.starts_with("std::") || entry.starts_with("core::") {
                boundary.push(Entry::Foreign(entry.to_owned()));
            } else {
                boundary.push(Entry::Items(entry.to_owned(), resolve(entry, &mut refused)));
            }
        }
        // `—`: the door hands nothing to another thread (it arms a wait, or answers from what it
        // holds); its walk is judged all the same.
        if boundary.is_empty() && cells[2] != "—" {
            refused.push(format!("`{}` names no boundary", cells[0]));
        }
        specs.push(DoorSpec {
            key: cells[0].to_owned(),
            roots: resolve(cells[0], &mut refused).into_iter().collect(),
            boundary,
            handed: cell_list(cells[3])
                .into_iter()
                .flat_map(|key| resolve(key, &mut refused))
                .collect(),
            mailbox: cell_list(cells[4])
                .into_iter()
                .flat_map(|key| resolve(key, &mut refused))
                .collect(),
        });
    }
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
         # scripts/ci/check-request-door-sites.ps1 - a key added or grown, or a verdict that softens\n\
         # nothing but moves to unreviewed or violation, carries a reason 'YYYY-MM-DD: <why>'.\n",
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
    let failures = list_failures(&tree, &list, &doors_of);
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
