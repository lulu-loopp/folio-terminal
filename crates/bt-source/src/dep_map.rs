//! **The measured dependency map inside a package** — J3 (T-RUNTIME-DEP-MAP,
//! 2026-10-11; the ownership contract's §3.1, ticket O-1), read from one
//! [`FieldCensus`] and nothing else.
//!
//! Four tables, each a fact the field moves and the topic moves of the split
//! need before they open a file:
//!
//! * **fields** — every item × field of the census's structs, by kind: the
//!   census's four columns and the `read` column ([`FieldCensus::reads`]).
//! * **topics** — every call edge between two files of the package, counted
//!   by call site, with the busiest caller → callee pair as its example. A
//!   topic is a file, named by its path under the crate root's directory.
//! * **doors** — every item outside a field's own type that is its only
//!   writer, or one of two: the door a field move turns into a method.
//! * **unclassified** — every free function and every `Runtime` method still
//!   written in the crate root's file, with the topics its callers are written
//!   in and the topics of what it calls, least-referenced first: the order the
//!   topic moves take them in.
//!
//! **What the map does not see** is the census's own limit, written into each
//! table's header as a count rather than left out: a read through a receiver
//! the rules do not type, and a call through a trait object, a stored
//! closure, a function value or an untyped receiver.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::census::{Callable, Column, FieldCensus};
use crate::index::Index;

/// The type whose methods the crate root still declares with no topic home.
const RUNTIME: &str = "Runtime";

/// How many topics an unclassified item's row names.
const TOPICS_NAMED: usize = 3;

/// A field door has at most this many writers.
const DOOR_WRITERS: usize = 2;

/// One item × field × kind.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FieldUse {
    pub structure: String,
    pub field: String,
    pub item: String,
    /// `read`, `access`, `write`, `membership` or `inner`.
    pub kind: &'static str,
    pub count: usize,
}

/// Call sites from one topic into another.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TopicEdge {
    pub from: String,
    pub to: String,
    pub calls: usize,
    /// `caller -> callee`: the pair with the most sites on this edge.
    pub example: String,
}

/// An item outside a field's type that is its only writer, or one of two.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Door {
    pub structure: String,
    pub field: String,
    pub door: String,
    /// Write sites (membership sites for a hub) in the door.
    pub writes: usize,
    /// The other writer, or `-`.
    pub other_writers: String,
    /// `complete`, or `incomplete` when an unknown site names the field.
    pub status: &'static str,
}

/// A free function or `Runtime` method written in the crate root's file.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Unclassified {
    pub item: String,
    /// `function` or `method`.
    pub shape: &'static str,
    /// Call sites in other items that reach it.
    pub called_by: usize,
    /// Its call sites that reach another item of the package.
    pub calls: usize,
    /// The topics its callers are written in, most call sites first: `topic
    /// n`, at most three, or `-` — the candidate home.
    pub called_from: String,
    /// The topics of the items it calls, the same way.
    pub calls_into: String,
}

/// The map, measured.
#[derive(Clone, Debug)]
pub struct DependencyMap {
    fields: Vec<FieldUse>,
    topics: Vec<TopicEdge>,
    doors: Vec<Door>,
    unclassified: Vec<Unclassified>,
    root_topic: String,
    unattributed_reads: usize,
    unfollowed: BTreeMap<String, usize>,
}

/// A callable's topic: its file under `root`, with `/` between the parts.
/// `None` for a file outside it — another package's.
fn topic_of(callable: &Callable, root: &Path) -> Option<String> {
    let relative = callable.file.strip_prefix(root).ok()?;
    Some(
        relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// Whether a census function label is a method of `structure` itself.
fn is_own_method(function: &str, structure: &str) -> bool {
    function.starts_with(&format!("{structure}::"))
        || function.starts_with(&format!("<{structure} as "))
}

impl DependencyMap {
    /// The map of `census`'s subject. `subject` is the index the census was
    /// taken over: it names the items still written in the crate root's file.
    #[must_use]
    pub fn of(census: &FieldCensus, subject: &Index) -> Self {
        let root = census.root();
        let fields = Self::measure_fields(census);
        let doors = Self::measure_doors(census);
        let (topics, by_item) = Self::measure_topics(census, root);
        let root_topic = subject
            .universe()
            .roots()
            .first()
            .and_then(|target| target.file.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let unclassified = Self::measure_unclassified(subject, root, &root_topic, &by_item);
        let mut unfollowed = BTreeMap::new();
        for row in census.unfollowed() {
            let reason = row.reason.split(':').next().unwrap_or(&row.reason);
            *unfollowed.entry(reason.to_owned()).or_default() += row.sites;
        }
        Self {
            fields,
            topics,
            doors,
            unclassified,
            root_topic,
            unattributed_reads: census.unattributed_reads(),
            unfollowed,
        }
    }

    fn measure_fields(census: &FieldCensus) -> Vec<FieldUse> {
        let mut fields: Vec<FieldUse> = census
            .rows()
            .iter()
            .chain(census.reads())
            .filter_map(|row| {
                let (structure, field) = row.fact.split_once('.')?;
                Some(FieldUse {
                    structure: structure.to_owned(),
                    field: field.to_owned(),
                    item: format!("{}::{}", row.module, row.function),
                    kind: row.column.name(),
                    count: row.sites,
                })
            })
            .collect();
        fields.sort();
        fields
    }

    fn measure_doors(census: &FieldCensus) -> Vec<Door> {
        let mut writers: BTreeMap<&str, BTreeMap<String, (String, usize)>> = BTreeMap::new();
        for row in census.rows() {
            if matches!(row.column, Column::Write | Column::Membership) {
                let entry = writers
                    .entry(row.fact.as_str())
                    .or_default()
                    .entry(format!("{}::{}", row.module, row.function))
                    .or_insert_with(|| (row.function.clone(), 0));
                entry.1 += row.sites;
            }
        }
        let mut doors = Vec::new();
        for fact in census.facts() {
            let Some((structure, field)) = fact.fact.split_once('.') else {
                continue;
            };
            let Some(items) = writers.get(fact.fact.as_str()) else {
                continue;
            };
            if items.len() > DOOR_WRITERS {
                continue;
            }
            for (item, (function, writes)) in items {
                if is_own_method(function, structure) {
                    continue;
                }
                let others: Vec<&str> = items
                    .keys()
                    .filter(|other| *other != item)
                    .map(String::as_str)
                    .collect();
                doors.push(Door {
                    structure: structure.to_owned(),
                    field: field.to_owned(),
                    door: item.clone(),
                    writes: *writes,
                    other_writers: if others.is_empty() {
                        "-".to_owned()
                    } else {
                        others.join(",")
                    },
                    status: if fact.complete() {
                        "complete"
                    } else {
                        "incomplete"
                    },
                });
            }
        }
        doors.sort();
        doors
    }

    /// The topic edges, and for every item of the package the call sites
    /// joining it to each other item, both ways (an item's own recursion
    /// left out): `item → (other item, its topic) → sites`, with the calls it
    /// makes and the calls it receives apart.
    fn measure_topics(census: &FieldCensus, root: &Path) -> (Vec<TopicEdge>, ItemLinks) {
        let mut edges: BTreeMap<(String, String), BTreeMap<(String, String), usize>> =
            BTreeMap::new();
        let mut links = ItemLinks::default();
        for row in census.calls() {
            let Some(from) = topic_of(&row.caller, root) else {
                continue;
            };
            let caller = row.caller.key();
            let reached: BTreeSet<(String, String)> = row
                .callees
                .iter()
                .filter_map(|callee| Some((topic_of(callee, root)?, callee.key())))
                .collect();
            let callee_topics: BTreeSet<&String> = reached.iter().map(|(topic, _)| topic).collect();
            for to in callee_topics {
                if *to == from {
                    continue;
                }
                let callee = reached
                    .iter()
                    .find(|(topic, _)| topic == to)
                    .map(|(_, key)| key.clone())
                    .unwrap_or_default();
                *edges
                    .entry((from.clone(), to.clone()))
                    .or_default()
                    .entry((caller.clone(), callee))
                    .or_default() += row.sites;
            }
            let callees: BTreeSet<(&String, &String)> =
                reached.iter().map(|(topic, key)| (key, topic)).collect();
            for (callee, topic) in callees {
                if *callee == caller {
                    continue;
                }
                *links
                    .out
                    .entry(caller.clone())
                    .or_default()
                    .entry((callee.clone(), topic.clone()))
                    .or_default() += row.sites;
                *links
                    .into
                    .entry(callee.clone())
                    .or_default()
                    .entry((caller.clone(), from.clone()))
                    .or_default() += row.sites;
            }
        }
        let topics = edges
            .into_iter()
            .map(|((from, to), pairs)| {
                let calls = pairs.values().sum();
                let example = pairs
                    .iter()
                    .max_by(|(left, left_sites), (right, right_sites)| {
                        left_sites.cmp(right_sites).then_with(|| right.cmp(left))
                    })
                    .map(|((caller, callee), _)| format!("{caller} -> {callee}"))
                    .unwrap_or_default();
                TopicEdge {
                    from,
                    to,
                    calls,
                    example,
                }
            })
            .collect();
        (topics, links)
    }

    fn measure_unclassified(
        subject: &Index,
        root: &Path,
        root_topic: &str,
        links: &ItemLinks,
    ) -> Vec<Unclassified> {
        let package = subject
            .universe()
            .roots()
            .first()
            .map_or_else(String::new, |target| target.id.package.clone());
        let mut items: BTreeMap<String, &'static str> = BTreeMap::new();
        for item in subject.items() {
            if !item.kind().is_callable() || item.body().is_none() {
                continue;
            }
            let shape = match (item.type_owner(), item.trait_name()) {
                (None, None) => "function",
                (Some(RUNTIME), _) => "method",
                // Another type's method, or a trait's own default body.
                _ => continue,
            };
            let Some(callable) = Callable::of(subject, item, &package) else {
                continue;
            };
            if topic_of(&callable, root).as_deref() != Some(root_topic) {
                continue;
            }
            items.insert(callable.key(), shape);
        }
        let empty = BTreeMap::new();
        let mut rows: Vec<Unclassified> = items
            .into_iter()
            .map(|(item, shape)| {
                let out = links.out.get(&item).unwrap_or(&empty);
                let into = links.into.get(&item).unwrap_or(&empty);
                Unclassified {
                    item,
                    shape,
                    called_by: into.values().sum(),
                    calls: out.values().sum(),
                    called_from: ranked_topics(into),
                    calls_into: ranked_topics(out),
                }
            })
            .collect();
        rows.sort_by(|left, right| {
            left.called_by
                .cmp(&right.called_by)
                .then_with(|| left.item.cmp(&right.item))
        });
        rows
    }

    #[must_use]
    pub fn fields(&self) -> &[FieldUse] {
        &self.fields
    }

    #[must_use]
    pub fn topics(&self) -> &[TopicEdge] {
        &self.topics
    }

    #[must_use]
    pub fn doors(&self) -> &[Door] {
        &self.doors
    }

    #[must_use]
    pub fn unclassified(&self) -> &[Unclassified] {
        &self.unclassified
    }

    /// The four files, each as `(name, text)`, in the order the README
    /// reads them.
    #[must_use]
    pub fn render(&self) -> Vec<(&'static str, String)> {
        vec![
            ("fields.tsv", self.render_fields()),
            ("topics.tsv", self.render_topics()),
            ("doors.tsv", self.render_doors()),
            ("unclassified.tsv", self.render_unclassified()),
        ]
    }

    fn render_fields(&self) -> String {
        let mut out = header(&[
            "Reads and writes by item, per field of the census's structs (J3, the",
            "ownership contract's 3.1 item 1). kind is read, access, write, membership",
            "or inner, through the census's receiver resolution; count is sites.",
            &format!(
                "Read-shaped sites spelling a field name through a receiver the rules do not type, not attributed: {}.",
                self.unattributed_reads
            ),
        ]);
        out.push_str("struct\tfield\titem\tkind\tcount\n");
        for row in &self.fields {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                row.structure, row.field, row.item, row.kind, row.count
            ));
        }
        out
    }

    fn render_topics(&self) -> String {
        let mut lines = vec![
            "Topic -> topic call edges (J3, 3.1 item 2): a topic is a file under the",
            "crate root's directory; calls is call sites in from_topic reaching an item",
            "of to_topic; example_item is the caller -> callee pair with the most sites.",
            "A call within one topic is not an edge.",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        lines.push(format!(
            "Call sites naming a callable of the package and not followed: {}.",
            if self.unfollowed.is_empty() {
                "none".to_owned()
            } else {
                self.unfollowed
                    .iter()
                    .map(|(reason, sites)| format!("{sites} ({reason})"))
                    .collect::<Vec<_>>()
                    .join("; ")
            }
        ));
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        let mut out = header(&lines);
        out.push_str("from_topic\tto_topic\tcalls\texample_item\n");
        for row in &self.topics {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                row.from, row.to, row.calls, row.example
            ));
        }
        out
    }

    fn render_doors(&self) -> String {
        let mut out = header(&[
            "Doors already present (J3, 3.1 item 3): an item outside a field's own type",
            "that is its only writer, or one of two (write and membership sites).",
            "other_writers is the other writer or -; status is incomplete when an",
            "unknown census site names the field.",
        ]);
        out.push_str("struct\tfield\tdoor_item\twrites\tother_writers\tstatus\n");
        for row in &self.doors {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\n",
                row.structure, row.field, row.door, row.writes, row.other_writers, row.status
            ));
        }
        out
    }

    fn render_unclassified(&self) -> String {
        let first = format!(
            "The free functions and {RUNTIME} methods still written in {} (J3; K2's work",
            self.root_topic
        );
        let mut out = header(&[
            first.as_str(),
            "list), least-referenced first. called_by and calls are call sites in and out;",
            "called_from is the topics its callers are written in (the candidate home) and",
            "calls_into the topics of what it calls, most call sites first, at most three.",
        ]);
        out.push_str("item\tshape\tcalled_by\tcalls\tcalled_from\tcalls_into\n");
        for row in &self.unclassified {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\n",
                row.item, row.shape, row.called_by, row.calls, row.called_from, row.calls_into
            ));
        }
        out
    }
}

/// The topics of the other ends of an item's call sites, most sites first
/// (then by name): `topic n`, at most [`TOPICS_NAMED`], or `-`.
fn ranked_topics(links: &BTreeMap<(String, String), usize>) -> String {
    let mut by_topic: BTreeMap<&str, usize> = BTreeMap::new();
    for ((_, topic), sites) in links {
        *by_topic.entry(topic.as_str()).or_default() += sites;
    }
    let mut ranked: Vec<(&str, usize)> = by_topic.into_iter().collect();
    ranked.sort_by(|(left, left_n), (right, right_n)| {
        right_n.cmp(left_n).then_with(|| left.cmp(right))
    });
    if ranked.is_empty() {
        return "-".to_owned();
    }
    ranked
        .iter()
        .take(TOPICS_NAMED)
        .map(|(topic, n)| format!("{topic} {n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Call sites between items: what each item calls, and what calls it, each
/// keyed by the other item and that item's topic.
#[derive(Debug, Default)]
struct ItemLinks {
    out: BTreeMap<String, BTreeMap<(String, String), usize>>,
    into: BTreeMap<String, BTreeMap<(String, String), usize>>,
}

/// The header every file of the map opens with.
fn header(lines: &[&str]) -> String {
    let mut out = String::from(GENERATED);
    for line in lines {
        out.push_str("# ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

const GENERATED: &str = "\
# Generated by bt_source::DependencyMap (bt-source's census test target); do not
# edit - run scripts/generate-runtime-dep-map.ps1.
";

/// The line the generating script writes under the header, naming the commit
/// the map was measured at. It is the one line the guard does not compare.
pub const MEASURED_AT: &str = "# measured at ";
