//! Fixed-capacity exclusive call tree. Only the window thread writes; snapshots
//! are copied to the watchdog with the existing slow-hold queue. No hot-path
//! allocation, locking, formatting, or additional clock reads.
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use super::Station;

pub(super) const CAPACITY: usize = 256;
pub(super) const ROOT: usize = CAPACITY;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Node {
    key: u64,
    pane: u64,
    ms: u64,
    bytes: u64,
    accepted: u64,
    count: u64,
    attempt: [u64; 3],
}

impl Node {
    fn station(self) -> Station {
        Station::from_byte((self.key & 255) as u8)
    }

    fn parent(self) -> usize {
        ((self.key >> 8) - 1) as usize
    }
}

#[derive(Debug)]
struct AtomicNode {
    key: AtomicU64,
    pane: AtomicU64,
    ms: AtomicU64,
    bytes: AtomicU64,
    accepted: AtomicU64,
    count: AtomicU64,
    attempt: [AtomicU64; 3],
}

impl AtomicNode {
    fn new() -> Self {
        Self {
            key: AtomicU64::new(0),
            pane: AtomicU64::new(0),
            ms: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            accepted: AtomicU64::new(0),
            count: AtomicU64::new(0),
            attempt: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    fn snapshot(&self) -> Node {
        Node {
            key: self.key.load(Relaxed),
            pane: self.pane.load(Relaxed),
            ms: self.ms.load(Relaxed),
            bytes: self.bytes.load(Relaxed),
            accepted: self.accepted.load(Relaxed),
            count: self.count.load(Relaxed),
            attempt: std::array::from_fn(|i| self.attempt[i].load(Relaxed)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Tree {
    nodes: [Node; CAPACITY],
    overflow: bool,
}

impl Default for Tree {
    fn default() -> Self {
        Self {
            nodes: [Node::default(); CAPACITY],
            overflow: false,
        }
    }
}

#[derive(Debug)]
pub(super) struct Ledger {
    nodes: [AtomicNode; CAPACITY],
    current: AtomicU64,
    scope: AtomicU64,
    overflow: AtomicU64,
    attempt: [AtomicU64; 3],
}

impl Ledger {
    pub(super) fn new() -> Self {
        Self {
            nodes: std::array::from_fn(|_| AtomicNode::new()),
            current: AtomicU64::new(ROOT as u64),
            scope: AtomicU64::new(ROOT as u64),
            overflow: AtomicU64::new(0),
            attempt: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    pub(super) fn attempt(&self, values: [u64; 3]) {
        for (cell, value) in self.attempt.iter().zip(values) {
            cell.store(value, Relaxed);
        }
    }

    pub(super) fn generation(&self, generation: u64) {
        self.attempt[1].store(generation, Relaxed);
    }

    pub(super) fn clear(&self) {
        for node in &self.nodes {
            node.key.store(0, Relaxed);
        }
        self.current.store(ROOT as u64, Relaxed);
        self.scope.store(ROOT as u64, Relaxed);
        self.overflow.store(0, Relaxed);
    }

    pub(super) fn current(&self) -> usize {
        self.current.load(Relaxed) as usize
    }

    pub(super) fn scope(&self) -> usize {
        self.scope.load(Relaxed) as usize
    }
    pub(super) fn set_scope(&self, scope: usize) {
        self.scope.store(scope as u64, Relaxed);
    }

    pub(super) fn charge(&self, ms: u64) {
        if let Some(node) = self.nodes.get(self.current()) {
            node.ms.fetch_add(ms, Relaxed);
        }
    }

    pub(super) fn restore(&self, node: usize) {
        self.current.store(node as u64, Relaxed);
    }

    pub(super) fn enter(&self, station: Station, parent: usize, pane: u64) {
        let key = ((parent as u64 + 1) << 8) | station as u64;
        let attempt = std::array::from_fn::<_, 3, _>(|i| self.attempt[i].load(Relaxed));
        let hash = attempt.iter().fold(0u64, |hash, value| {
            hash.wrapping_mul(31).wrapping_add(*value)
        }) ^ key.wrapping_mul(0x9e3779b97f4a7c15)
            ^ pane.wrapping_mul(0x517cc1b727220a95);
        let start = ((hash ^ (hash >> 32)) as usize) % CAPACITY;
        for probe in 0..CAPACITY {
            let slot = (start + probe) % CAPACITY;
            let node = &self.nodes[slot];
            let old = node.key.load(Relaxed);
            if old == 0 {
                node.key.store(key, Relaxed);
                node.pane.store(pane, Relaxed);
                node.ms.store(0, Relaxed);
                node.bytes.store(0, Relaxed);
                node.accepted.store(0, Relaxed);
                node.count.store(0, Relaxed);
                for (cell, value) in node.attempt.iter().zip(attempt) {
                    cell.store(value, Relaxed);
                }
            }
            if old == 0
                || (old == key
                    && node.pane.load(Relaxed) == pane
                    && node
                        .attempt
                        .iter()
                        .zip(attempt)
                        .all(|(cell, value)| cell.load(Relaxed) == value))
            {
                self.restore(slot);
                return;
            }
        }
        // Never invent attribution if capacity is exhausted. The report falls
        // back to the complete coarse ledger and explicitly says so.
        self.overflow.store(1, Relaxed);
        self.restore(ROOT);
    }

    pub(super) fn at(&self, station: Station) {
        let mut ancestor = self.current();
        for _ in 0..CAPACITY {
            let Some(node) = self.nodes.get(ancestor) else {
                break;
            };
            let node = node.snapshot();
            if node.key == 0 {
                break;
            }
            if node.station() == station {
                self.restore(ancestor);
                return;
            }
            ancestor = node.parent();
        }
        self.enter(station, self.scope(), 0);
    }

    pub(super) fn phase(&self, station: Station) {
        let parent = self
            .nodes
            .get(self.current())
            .map_or(ROOT, |node| node.snapshot().parent());
        self.enter(station, parent, 0);
    }

    pub(super) fn counters(&self, bytes: usize, accepted: usize, count: usize) {
        if let Some(node) = self.nodes.get(self.current()) {
            node.bytes.fetch_add(bytes as u64, Relaxed);
            node.accepted.fetch_add(accepted as u64, Relaxed);
            node.count.fetch_add(count as u64, Relaxed);
        }
    }

    pub(super) fn active_attempt(&self) -> Option<String> {
        let attempt = std::array::from_fn::<_, 3, _>(|i| self.attempt[i].load(Relaxed));
        if attempt[2] == 0 {
            return None;
        }
        let node = self.nodes.get(self.current())?.snapshot();
        if node.key == 0 || node.attempt != attempt {
            return None;
        }
        Some(format!(
            " [win={} gen={} seq={} outcome={}]",
            node.attempt[0],
            node.attempt[1],
            node.attempt[2],
            super::present_progress(node.station())
        ))
    }

    pub(super) fn snapshot(&self) -> Tree {
        Tree {
            nodes: std::array::from_fn(|i| self.nodes[i].snapshot()),
            overflow: self.overflow.load(Relaxed) != 0,
        }
    }
}

impl Tree {
    #[cfg(test)]
    pub(super) fn total_ms(&self) -> u64 {
        self.nodes
            .iter()
            .filter(|node| node.key != 0)
            .map(|node| node.ms)
            .sum()
    }
    fn inclusive(&self, index: usize) -> u64 {
        self.nodes[index].ms
            + self
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, node)| node.key != 0 && node.parent() == index)
                .map(|(child, _)| self.inclusive(child))
                .sum::<u64>()
    }

    /// The node the pump's note is printed beside: the `message pump` node
    /// that was charged the most, when there is one (ticket 64).
    fn pump_node(&self) -> Option<usize> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.key != 0 && node.station() == Station::Pump)
            .max_by_key(|(index, node)| (node.ms, std::cmp::Reverse(*index)))
            .map(|(index, _)| index)
    }

    fn children(&self, parent: usize, pump: Option<(usize, &str)>) -> String {
        let mut children: Vec<_> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.key != 0 && node.parent() == parent)
            .filter(|(index, _)| self.inclusive(*index) >= 1)
            .collect();
        children
            .sort_by_key(|(index, node)| (std::cmp::Reverse(node.ms), node.key, node.pane, *index));
        children
            .into_iter()
            .map(|(index, node)| {
                let mut line = format!("{} {} ms", node.station(), node.ms);
                if let Some((pump_node, note)) = pump
                    && pump_node == index
                {
                    line.push(' ');
                    line.push_str(note);
                }
                if node.pane != 0 {
                    let kind = if node.station() == Station::DrainTab {
                        "tab"
                    } else {
                        "pane"
                    };
                    line.push_str(&format!(" [{kind}={}]", node.pane - 1));
                }
                if node.bytes != 0 || node.count != 0 {
                    line.push_str(&format!(
                        " [bytes={}, accepted={}, count={}]",
                        node.bytes, node.accepted, node.count
                    ));
                }
                if node.attempt[2] != 0 {
                    line.push_str(&format!(
                        " [win={} gen={} seq={} outcome={}]",
                        node.attempt[0],
                        node.attempt[1],
                        node.attempt[2],
                        super::present_progress(node.station())
                    ));
                }
                let nested = self.children(index, pump);
                if !nested.is_empty() {
                    line.push_str(&format!(" ({nested})"));
                }
                line
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The tree as one line, with `pump` — the note of which message the
    /// pump's time went to — beside the `message pump` node it belongs to.
    pub(super) fn line(&self, pump: Option<&str>) -> Option<String> {
        if self.overflow || self.nodes.iter().all(|node| node.key == 0) {
            return None;
        }
        let pump = pump.and_then(|note| Some((self.pump_node()?, note)));
        Some(self.children(ROOT, pump))
    }

    pub(super) fn overflowed(&self) -> bool {
        self.overflow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Eq, PartialEq)]
    struct Parsed {
        label: String,
        ms: u64,
        metadata: String,
        children: Vec<Self>,
    }

    impl Parsed {
        // Independent recursive reader for the existing `label N ms` token
        // plus the new parentheses. Metadata does not change the time grammar.
        fn read(input: &mut &str) -> Vec<Self> {
            let mut nodes = Vec::new();
            loop {
                let (head, tail) = input.split_once(" ms").unwrap();
                let (label, ms) = head.rsplit_once(' ').unwrap();
                *input = tail;
                let mut metadata = String::new();
                while input.starts_with(" [") {
                    let end = input.find(']').unwrap() + 1;
                    metadata.push_str(&input[..end]);
                    *input = &input[end..];
                }
                let children = if let Some(rest) = input.strip_prefix(" (") {
                    *input = rest;
                    let children = Self::read(input);
                    *input = input.strip_prefix(')').unwrap();
                    children
                } else {
                    Vec::new()
                };
                nodes.push(Self {
                    label: label.into(),
                    ms: ms.parse().unwrap(),
                    metadata,
                    children,
                });
                let Some(rest) = input.strip_prefix(", ") else {
                    break;
                };
                *input = rest;
            }
            nodes
        }

        fn write(nodes: &[Self]) -> String {
            nodes
                .iter()
                .map(|node| {
                    let mut text = format!("{} {} ms{}", node.label, node.ms, node.metadata);
                    if !node.children.is_empty() {
                        text.push_str(&format!(" ({})", Self::write(&node.children)));
                    }
                    text
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

    #[test]
    fn present_stations_keep_window_generation_sequence_and_progress_after_return() {
        let ledger = Ledger::new();
        ledger.at(Station::Event);
        let root = ledger.current();
        for (window, generation, sequence) in [(7, 3, 11), (8, 4, 1)] {
            ledger.attempt([window, generation, sequence]);
            ledger.enter(Station::SwapchainPresent, root, 0);
            ledger.charge(600);
        }
        assert!(
            ledger
                .active_attempt()
                .unwrap()
                .contains("win=8 gen=4 seq=1")
        );
        ledger.attempt([9, 4, 1]); // the next window can have the same local sequence
        assert_eq!(ledger.active_attempt(), None);
        ledger.attempt([0; 3]);
        assert_eq!(ledger.active_attempt(), None);
        ledger.restore(root);
        ledger.enter(Station::Event, root, 0);
        ledger.charge(1);
        let line = ledger.snapshot().line(None).unwrap();
        assert!(line.contains("[win=7 gen=3 seq=11 outcome=in_progress:present]"));
        assert!(line.contains("[win=8 gen=4 seq=1 outcome=in_progress:present]"));
        assert_eq!(line.matches("[win=").count(), 2);
        let mut remaining = line.as_str();
        let parsed = Parsed::read(&mut remaining);
        assert!(remaining.is_empty());
        assert_eq!(Parsed::write(&parsed), line);
    }

    #[test]
    fn exclusive_tree_sums_order_cutoff_and_round_trip() {
        let ledger = Ledger::new();
        ledger.at(Station::Event);
        let event = ledger.current();
        ledger.charge(7);
        ledger.enter(Station::ImeCommit, event, 0);
        let ime = ledger.current();
        ledger.charge(3);
        ledger.enter(Station::PtyInput, ime, 0);
        ledger.charge(1300);
        ledger.counters(6, 6, 1);
        ledger.restore(ime);
        ledger.enter(Station::ImeCursorArea, ime, 0);
        ledger.charge(1);
        ledger.enter(Station::ImeCaretDestroy, ime, 0); // zero: omitted
        let tree = ledger.snapshot();
        assert_eq!(tree.inclusive(event), 1311);
        assert_eq!(tree.nodes.iter().map(|node| node.ms).sum::<u64>(), 1311);
        let line = tree.line(None).unwrap();
        assert_eq!(
            line,
            "window_event 7 ms (IME Commit 3 ms (PtySession::write input enqueue 1300 ms [bytes=6, accepted=6, count=1], IME set_cursor_area 1 ms))"
        );
        // Independent reader: each exclusive figure occurs once, irrespective
        // of nesting and optional metadata. Same `label N ms` token as before.
        let amounts: Vec<u64> = line
            .split(" ms")
            .take(4)
            .map(|prefix| prefix.rsplit_once(' ').unwrap().1.parse().unwrap())
            .collect();
        assert_eq!(amounts, [7, 3, 1300, 1]);
        assert_eq!(amounts.iter().sum::<u64>(), tree.inclusive(event));
        assert!(!line.contains("destroy"));
        let mut remaining = line.as_str();
        let parsed = Parsed::read(&mut remaining);
        assert!(remaining.is_empty());
        assert_eq!(Parsed::write(&parsed), line);
        assert_eq!(
            parsed[0].children[0].children[0].label,
            "PtySession::write input enqueue"
        );
    }

    #[test]
    fn repeated_calls_keep_their_parent_and_pane_and_reset_between_turns() {
        let ledger = Ledger::new();
        ledger.at(Station::Drain);
        let drain = ledger.current();
        for pane in [1, 2, 1] {
            ledger.enter(Station::DrainPane, drain, pane);
            let parent = ledger.current();
            ledger.charge(1);
            ledger.counters(3, 0, 1);
            ledger.enter(Station::PtyInput, parent, 0);
            ledger.charge(2);
        }
        ledger.at(Station::Event);
        ledger.enter(Station::PtyInput, ledger.current(), 0);
        ledger.charge(9);
        let line = ledger.snapshot().line(None).unwrap();
        assert_eq!(line.matches("input enqueue").count(), 3);
        assert!(line.contains("drain pane 2 ms [pane=0] [bytes=6, accepted=0, count=2]"));
        assert!(line.contains("drain pane 1 ms [pane=1]"));
        assert_eq!(ledger.snapshot().inclusive(drain), 9);
        ledger.clear();
        assert_eq!(ledger.snapshot().line(None), None);
    }

    #[test]
    fn exhausted_detail_capacity_is_explicit_and_never_invents_a_parent() {
        let ledger = Ledger::new();
        for pane in 0..=CAPACITY {
            ledger.enter(Station::DrainPane, ROOT, pane as u64);
            ledger.charge(1);
        }
        assert!(ledger.snapshot().overflowed());
        assert_eq!(ledger.snapshot().line(None), None);
    }
}
