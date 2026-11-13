//! **The timing-bound tests are exactly the ones on the list** — T-GATES-047,
//! the discovery half of `docs/plans/TIMING-BOUND-TESTS.tsv`, widened by
//! T-TIMING-BOUND-STRAYS (2026-10-24).
//!
//! The list names every test whose verdict can depend on real elapsed time, so
//! that the number only goes down: `scripts/ci/check-timing-bound.ps1` refuses a
//! row added against the merge base, and this test refuses a timing-bound test
//! that is not a row, or a row that names no such test. Between them a new test
//! that waits on the real clock is red either way — unlisted here, or listed
//! there.
//!
//! # What "timing-bound" means here, exactly
//!
//! A **test** is a function carrying `#[test]` (outside comments), in any
//! first-party package's `src/`, `tests/`, `examples/` or `benches/`. It is
//! **timing-bound** when one of three shapes holds.
//!
//! 1. **A real wait or a measurement in its own body** (T-GATES-047): the body —
//!    the braces of that function, closures and nested items included — calls
//!    one of [`REAL_WAITS`] or [`MEASURES`]: a stable standard-library wait whose
//!    length is real time (`thread::sleep`, `Receiver::recv_timeout`,
//!    `Condvar::wait_timeout`, `thread::park_timeout` and their variants) or a
//!    reading of how much real time passed (`Instant::elapsed`). Widened by
//!    T-FIXED-MILLISECOND-KERNEL-WAITS-IN-BT-PLATFORM (2026-11-02, plan row 163)
//!    to the kernel's waits: a call of one of [`KERNEL_WAITS`] — a Win32 wait or
//!    timeout (`WaitForSingleObject`, `WaitForMultipleObjects`, `WaitNamedPipeW`,
//!    `GetQueuedCompletionStatus`, `WinHttpSetTimeouts`, … and their variants) or
//!    a std socket timeout (`set_read_timeout`, `set_write_timeout`,
//!    `connect_timeout`), `WaitForInputIdle`, `WaitForDebugEvent`, `WSAPoll` or
//!    macOS's `CFRunLoop::run_in_mode`, matched by the called name as the std
//!    waits are; or a call of one of [`KERNEL_WAIT_PATHS`] (`libc::poll`,
//!    `libc::select`, `libc::kevent`, …, and Winsock's `WinSock::select`),
//!    matched by that path, because their last names are common words the
//!    workspace declares (`HangWatch::poll`, `select`). The
//!    bound argument is not read: a literal, a constant, an expression,
//!    `INFINITE` and `0` all count, as `recv_timeout(Duration::ZERO)` does.
//!    `GetOverlappedResult`, `waitpid` and `waitid` are not real waits here: they
//!    take no length of time — they wait for an event, or not at all — and a
//!    test stuck in one is the suite watchdog's hang, not a timing verdict.
//! 2. **Clock arithmetic in its own body** (T-TIMING-BOUND-STRAYS): an operand
//!    of `<`, `<=`, `>`, `>=` or binary `-`, or the receiver and the argument of
//!    `duration_since` / `saturating_duration_since` / `checked_duration_since`,
//!    where **both** sides hold a real-clock reading and the readings are not the
//!    same ones. A reading is a call of a path ending `Instant::now` or
//!    `SystemTime::now` (any qualification, `<Instant>::now` too). A side holds the
//!    readings written in it and those of every name bound (`let`, `=`, `+=`)
//!    from an expression holding readings, through the bindings in scope. So
//!    `Instant::now() < give_up`, where `give_up` was bound from an earlier
//!    reading, is two readings; `now + Duration::from_millis(5) > now` is one
//!    reading and its offset, and `Instant::now() - Duration::from_secs(1)` is a
//!    reading and no other. Inside macros the arguments are read when they parse
//!    as comma-separated expressions (or `a; n`).
//! 3. **A wait or a measurement in a test-only helper** (T-TIMING-BOUND-STRAYS;
//!    widened by T-TIMING-GUARD-FOLLOWS-WAITING-HELPERS, 2026-11-12, plan row
//!    208): the test reaches, through edges `bt_source::call_graph` resolves (the
//!    root is the test; only test-only callables of the test's own target are
//!    followed, transitively; a product function ends the walk, and so does a
//!    door, below), a test-only callable whose own body holds shape 1 or shape 2:
//!    a real wait of any of the lists above, an `elapsed()` call, or clock
//!    arithmetic between two readings. A helper that **keeps** its measurement —
//!    returned, stored, pushed, handed to a call, bound to a name that is then
//!    kept — rather than only comparing (`<`, `==`, …, or `cmp`/`lt`/… methods)
//!    or formatting it is a **measuring** helper, and its row says "measures in
//!    its helper"; every other one — a wait, a stall timer that only asks "has
//!    it been too long", a remaining patience handed to a wait — is a
//!    **waiting** helper, and its row says "waits in its helper". An `elapsed`
//!    a helper only prints or formats (the arguments of `format!`, `print!`,
//!    `write!`, `panic!` and their kin, inline captures `{took:?}` included)
//!    decides nothing and is no clock there (row 20 review N2). So the list
//!    depends on what a test does, not on whether its wait is written in its own
//!    body or one call away.
//!
//!    **A door** is a waiting or measuring test-only helper that is itself a row
//!    of the list: the walk stops there, and the tests that reach it are not rows
//!    through it (the suites' patience helpers — `lane::receive`,
//!    `door_skipped::received`, `persist`'s `on_a_worker` — are doors, so a new
//!    test that awaits its event through one adds no row, while a new waiting
//!    helper is a new row or makes every test reaching it one). A door's row
//!    names it as a method is named (`lane::Hold::wait_standing`); it must still
//!    wait or measure and be reached from a test — through an edge, or as a
//!    candidate of a call the graph does not resolve, whose refusal its row
//!    answers — or run only by an implicit call, or it is stale. An inline wait moved into a call of a door leaves its test's
//!    row stale: a shrink, recorded on a `# shrunk` line naming the door.
//!
//! The scan matches the called name for shape 1, not the callee, so the names
//! are only the standard library's while nothing in the workspace declares a
//! function or method by one of them — and that is asserted, not assumed
//! ([`no_workspace_function_is_named_like_a_real_wait`]). A controlled-clock
//! helper named like a trigger is a finding to rename, never a test silently
//! listed as timing-bound; `CardLoop::sleep_until` in `focus_thumb`'s tests,
//! which advances a model clock, is why `sleep_until` is not a trigger (it is
//! not a stable `std::thread` item either).
//!
//! # What each reading refuses, by file and line
//!
//! * A test body (or a reached helper) whose text does not parse.
//! * Macro arguments that do not parse as expressions and name a clock value
//!   (a reading's path, `elapsed`, a `duration_since`, or a name holding a
//!   reading or a measurement).
//! * An edge the call graph leaves unresolved — `Dynamic`, `Ambiguous`,
//!   `OtherArm` — any of whose candidates is a test-only callable of the test's
//!   target, callable from the caller under Rust's privacy, that waits or
//!   measures or reaches one that does (a door answers no), or of which that
//!   cannot be told (its body or a call on its way is refused in turn); an
//!   unresolved path whose last segment names such a callable; an unparsed body
//!   or a cutoff on the walk; a macro whose `macro_rules!` definition in the
//!   package names `elapsed`, `Instant` or a wait.
//! * A test-only impl that only an implicit call runs (`call_graph`'s
//!   `Resolver::implicit`: a `Drop` at the end of a scope, `for`'s `next`, an
//!   operator) whose body waits or measures, or reaches a test-only helper that
//!   does, unless it is a door: no edge leads into it, so no test that runs it
//!   could be seen (plan row 208).
//! * Anywhere in a first-party package, product code included (a test module's
//!   `use super::*` brings its parent's imports): a [`KERNEL_WAIT_PATHS`] wait
//!   named by its path and not called right there (`use libc::poll;`,
//!   `use libc::poll as p;`, `let f = libc::poll;`), except that a path which
//!   also names a type ([`KERNEL_WAIT_TYPES`]: the `libc::kevent` struct) is
//!   refused only where it is imported; a group or a glob of a path's root
//!   (`use libc::{…}`, `use libc::*`, `WinSock::{…}`), on a file's last line
//!   too, and a rename of the root itself (`use libc as c`), each of which
//!   could bring a wait in under a name its path does not spell; a
//!   [`KERNEL_WAITS`] name renamed (`use …::WaitForSingleObject as wait`); and a
//!   [`KERNEL_WAITS`] name taken as a value (`let wait = WaitForSingleObject;`),
//!   which a call through the value would hide. Imported (inside a `use` tree)
//!   and called under its own name, a [`KERNEL_WAITS`] wait is found, so an
//!   import of one is not refused; a function declared by one is
//!   [`no_workspace_function_is_named_like_a_real_wait`]'s.
//!
//! # What it does not see, and who does
//!
//! * **A clock reached through a product function the test calls.** A test that
//!   hands `Duration::from_secs(5)` to a product function that polls, or whose
//!   assertion compares its own reading with an instant the product stamped (a
//!   present, a heartbeat's origin), carries a wall-clock bound this scan does
//!   not follow: the walk ends at a product function, because "a test reaches a
//!   product clock reading" holds for nearly every window test and decides
//!   nothing about distance. Review owns discovering those; the product half of
//!   that shape — a product function that is handed `now` and reads the clock
//!   again — is held by [`no_product_function_reads_again_the_clock_it_was_handed`].
//! * **A clock handed through a parameter.** A helper's `started: Instant`
//!   parameter is a value the reading does not trace to a reading; an
//!   `elapsed()` on it is still a measurement.
//! * **A reading handed back by a call** (shape 2). `let deadline = deadline_in(secs);
//!   while Instant::now() < deadline` compares one reading with the answer of a
//!   call; the answer is not traced to the reading inside the callee, so this is
//!   neither found nor refused. Review owns it.
//! * **An instant handed under another name** (the handed-now guard). Only a
//!   parameter named `now` of type `Instant`, `&Instant` or `Option<Instant>` is
//!   read as the turn's instant; `at: Instant`, `deadline: Instant` (which a
//!   function rightly compares with a fresh reading) and an instant carried in
//!   another struct's field are not. A **clock** — a closure, pointer, trait or
//!   clock-carrying type that answers an instant — is read whatever its name
//!   (plan row 205); what it still does not see is listed on the guard.
//! * **Test support behind a feature, in another package.** `bt_pty::test_shell`
//!   (`establish`'s poll) and `bt_platform::trust_harness` (`stopped_at_exit`'s
//!   `WaitForDebugEvent`) are compiled under `cfg(any(test, feature = "…"))`,
//!   which bt-source reads as a product build may hold, and the walk follows
//!   only the test's own target: a test of another package that waits through
//!   them is not seen (0.4.10 plan row 95 follows them).
//! * **A std wait taken as a value** ([`REAL_WAITS`]) is not seen; a kernel wait
//!   taken that way is refused.
//! * **A wait in a `static`'s or `const`'s initializer.** A `LazyLock` or a
//!   `const` in test code is reached by its use (`call_graph`'s
//!   `Target::Value`), and shape 3's walk follows items only: a wait or a
//!   measurement in such an initializer is neither followed nor refused (row 20
//!   review N1). Review owns it.
//! * **A wait taken as a value under a name that is also a type**
//!   ([`KERNEL_WAIT_TYPES`]): `let f = libc::kevent;` or
//!   `register(libc::kevent)` is neither found nor refused, because a token
//!   scan cannot tell the function from the struct used as a type; only its
//!   call is read and only its import is refused. Review owns the value.
//! * **A kernel wait outside the closed lists** ([`KERNEL_WAITS`],
//!   [`KERNEL_WAIT_PATHS`]), and a first-party `extern "C"` wait called bare:
//!   a new wait name in a test body is review's, and joins a list when it is
//!   first written.
//! * **A product function's own kernel wait** (`attention_pipe`'s
//!   `READ_DEADLINE`, `open_client`'s busy wait, WinHTTP's phase timeouts): a
//!   product window, swept by hand by plan row 163 on 2026-11-02 and recorded in
//!   the list's `# noted` line, as rows 125 and 153 swept theirs.
//! * **`Instant` and `Duration` as values.** Most of the tree's tests build a
//!   controlled clock from one `Instant::now()` plus `Duration::from_*` offsets,
//!   and their verdict does not move with the machine's speed. That is exactly
//!   the shape 2 leaves out.
//!
//! Each run leaves the list it expects in
//! `target/timing-bound-tests/TIMING-BOUND-TESTS.tsv`, with a seam column
//! chosen by what the body calls, for a ticket that converts or removes tests
//! to copy rows from — never to add them.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_source::call_graph::{Arms, CallGraph, ItemId, Node, Resolver, Target, Unresolved};
use bt_source::{
    DiskScope, Index, ItemKind, ItemRecord, MacroKind, Package, Pattern, Search, Span, TargetId,
    TargetKind, TargetRoot, TraitId, TraitResolver, Universe, Vendor, View, Workspace, is_vendored,
    needle, normalized, report, universes,
};

/// The list, from the workspace root.
const LIST: &str = "docs/plans/TIMING-BOUND-TESTS.tsv";

/// The list's column header.
const HEADER: &str = "test\tcrate\twall_clock_assumption\tdeterministic_seam";

/// Stable standard-library calls that wait for a length of real time:
/// `thread::sleep`, `thread::sleep_ms`, `Receiver::recv_timeout`,
/// `Condvar::wait_timeout`, `Condvar::wait_timeout_ms`,
/// `Condvar::wait_timeout_while`, `thread::park_timeout`,
/// `thread::park_timeout_ms`.
const REAL_WAITS: [&str; 8] = [
    "sleep",
    "sleep_ms",
    "recv_timeout",
    "wait_timeout",
    "wait_timeout_ms",
    "wait_timeout_while",
    "park_timeout",
    "park_timeout_ms",
];

/// Calls that read how much real time passed.
const MEASURES: [&str; 1] = ["elapsed"];

/// **The kernel's waits, matched by the called name** (plan row 163): calls
/// that wait for a length of real time, or arm one on a stream's next read,
/// write or connect. Win32: the object waits (`WaitForSingleObject`,
/// `WaitForSingleObjectEx`, `WaitForMultipleObjects`, `WaitForMultipleObjectsEx`,
/// `MsgWaitForMultipleObjects`, `MsgWaitForMultipleObjectsEx`,
/// `SignalObjectAndWait`), the sleeps (`SleepEx`, `Sleep`), a busy pipe's wait
/// (`WaitNamedPipeW`), a completion port's (`GetQueuedCompletionStatus`,
/// `GetQueuedCompletionStatusEx`), an overlapped result's with a timeout
/// (`GetOverlappedResultEx`) and WinHTTP's phase timeouts
/// (`WinHttpSetTimeouts`), a new process's input idle (`WaitForInputIdle`), a
/// debuggee's next event (`WaitForDebugEvent`, `WaitForDebugEventEx`), the
/// condition, address and COM waits (`SleepConditionVariableSRW`,
/// `SleepConditionVariableCS`, `WaitOnAddress`, `CoWaitForMultipleHandles`) and
/// Winsock's (`WSAPoll`, `WSAWaitForMultipleEvents`; `select` is a path, in
/// [`KERNEL_WAIT_PATHS`]); macOS: a run loop's turn for a length of time
/// (`CFRunLoopRunInMode`, `CFRunLoop::run_in_mode`); std: a socket's timeouts
/// (`set_read_timeout`, `set_write_timeout`, `connect_timeout`). Bare, imported
/// (inside a `use` tree) or by whole
/// `windows::Win32::…` path, a call is one shape: the name followed by `(`. No
/// workspace function may be declared by one of these names
/// ([`no_workspace_function_is_named_like_a_real_wait`]).
///
/// Left out on purpose: `GetOverlappedResult` (no timeout argument), `waitpid`
/// and `waitid` (blocking or `WNOHANG`). Each waits for an event or not at all;
/// none waits for a length of time.
const KERNEL_WAITS: [&str; 28] = [
    "WaitForSingleObject",
    "WaitForSingleObjectEx",
    "WaitForMultipleObjects",
    "WaitForMultipleObjectsEx",
    "MsgWaitForMultipleObjects",
    "MsgWaitForMultipleObjectsEx",
    "SignalObjectAndWait",
    "SleepEx",
    "Sleep",
    "WaitNamedPipeW",
    "GetQueuedCompletionStatus",
    "GetQueuedCompletionStatusEx",
    "GetOverlappedResultEx",
    "WinHttpSetTimeouts",
    "WaitForInputIdle",
    "WaitForDebugEvent",
    "WaitForDebugEventEx",
    "SleepConditionVariableSRW",
    "SleepConditionVariableCS",
    "WaitOnAddress",
    "CoWaitForMultipleHandles",
    "WSAPoll",
    "WSAWaitForMultipleEvents",
    "CFRunLoopRunInMode",
    "run_in_mode",
    "set_read_timeout",
    "set_write_timeout",
    "connect_timeout",
];

/// **The `libc` waits and Winsock's `select`, matched by path** (plan row 163):
/// their last names are common words (bt-app declares `HangWatch::poll` and
/// `select` methods), so a use of one is a call of `<root>::<name>` —
/// `::libc::poll` and a longer path ending so included — and every other
/// spelling that could hide one is refused ([`kernel_wait_refusals`]).
/// `libc::sleep` is [`REAL_WAITS`]' `sleep`.
const KERNEL_WAIT_PATHS: [&str; 13] = [
    "libc::poll",
    "libc::ppoll",
    "libc::select",
    "libc::pselect",
    "libc::kevent",
    "libc::epoll_wait",
    "libc::nanosleep",
    "libc::usleep",
    "libc::pthread_cond_timedwait",
    "libc::sem_timedwait",
    "libc::clock_nanosleep",
    "libc::sigtimedwait",
    "WinSock::select",
];

/// The [`KERNEL_WAIT_PATHS`] that also name a type: `libc::kevent` is the
/// kqueue event struct as well as the wait, so only its call is the wait, and
/// only its import (which brings the function in too) is refused.
const KERNEL_WAIT_TYPES: [&str; 1] = ["libc::kevent"];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The whole of a first-party package; `bt-source`'s `tests/fixtures/` are
/// source *about* compilation and are declared out, as `real_workspace` does.
pub(crate) fn universe_for(package: &Package) -> Universe {
    universes::first_party_whole(package, Vendor::Excluded).expect("a package of this workspace")
}

/// Every span of `pattern` in `view`, sorted.
fn spans(index: &Index, search: &Search) -> Vec<Span> {
    let mut found = index
        .search(search)
        .unwrap_or_else(|failure| panic!("{failure}"))
        .spans();
    found.sort();
    found
}

/// The spans of `sorted` that lie inside `outer`.
fn inside(sorted: &[Span], outer: Span) -> impl Iterator<Item = &Span> {
    let first = sorted.partition_point(|span| span.start() < outer.start());
    sorted[first..]
        .iter()
        .take_while(move |span| span.start() < outer.end())
        .filter(move |span| span.within(outer))
}

/// The names a test answers to, one per target that compiles it:
/// `<package>:<kind>:<target> <module path>::<name>`, with the crate root's
/// `crate` left off — the target and the filter `cargo test` takes.
fn identities(index: &Index, item: &ItemRecord) -> BTreeSet<String> {
    identities_named(index, item, item.name())
}

/// The names a callable answers to as a row of the list (a door, plan row 208): as
/// [`identities`], with a method's type in front of its name (`lane::Hold::wait_standing`),
/// so that two types' methods of one name in one module are two rows.
fn callable_identities(index: &Index, item: &ItemRecord) -> BTreeSet<String> {
    match item.type_owner() {
        Some(owner) => identities_named(index, item, &format!("{owner}::{}", item.name())),
        None => identities(index, item),
    }
}

/// [`identities`], under `named`.
fn identities_named(index: &Index, item: &ItemRecord, named: &str) -> BTreeSet<String> {
    let owners = index.file_of(item).owners();
    let mut found = BTreeSet::new();
    for declared in item.module_paths() {
        let owner = owners
            .iter()
            .filter(|owner| {
                declared == owner.module_path
                    || declared.starts_with(&format!("{}::", owner.module_path))
            })
            .max_by_key(|owner| owner.module_path.len())
            .expect("an item's module path extends its file's");
        let path = declared
            .strip_prefix("crate")
            .map_or(declared, |rest| rest.trim_start_matches("::"));
        let name = if path.is_empty() {
            named.to_owned()
        } else {
            format!("{path}::{named}")
        };
        found.insert(format!("{} {name}", owner.target));
    }
    found
}

/// `file:line` of a union offset, the file relative to the workspace root.
fn place(index: &Index, offset: usize) -> String {
    let root = normalized(&workspace_root());
    index.locate(offset).map_or_else(
        || format!("offset {offset}"),
        |location| {
            let file = normalized(location.file);
            let shown = file.strip_prefix(&root).unwrap_or(&file);
            format!(
                "{}:{}",
                shown.display().to_string().replace('\\', "/"),
                location.line
            )
        },
    )
}

/// Whether no product build compiles `record`: its file, or an arm it stands on,
/// is test-gated (an inline `#[cfg(test)] mod` included).
fn test_only(index: &Index, record: &ItemRecord) -> bool {
    !index.file_of(record).permits_product()
        || !record
            .identities()
            .any(|identity| identity.variant.permits_product())
}

/// The targets that compile `record`'s file.
fn targets(index: &Index, record: &ItemRecord) -> BTreeSet<String> {
    index
        .file_of(record)
        .owners()
        .iter()
        .map(|owner| owner.target.to_string())
        .collect()
}

/// Every `#[test]` attribute of `index`, sorted.
fn test_attributes(index: &Index) -> Vec<Span> {
    spans(
        index,
        &Search::new(needle!(Pattern::text("#[test]")), View::CodeKeepingLiterals),
    )
}

/// Whether `item` is a test: a callable carrying `#[test]`.
fn is_test(attributes: &[Span], item: &ItemRecord) -> bool {
    item.body().is_some()
        && item.kind().is_callable()
        && inside(attributes, item.declaration()).next().is_some()
}

/// One callable's text, read for its clock (see [`reading`]).
fn read_callable(index: &Index, record: &ItemRecord) -> Result<reading::Reading, String> {
    let text = index.text(record.whole());
    let (_, block) = reading::callable(
        text,
        record.kind() == ItemKind::Function,
        record.type_owner().is_some(),
    )
    .map_err(|error| {
        format!(
            "{}: the text of `{}` does not parse ({error}), so its clock cannot be read",
            place(index, record.whole().start()),
            record.name()
        )
    })?;
    Ok(block.map(|block| reading::read(&block)).unwrap_or_default())
}

/// One timing-bound test the scan found.
#[derive(Default)]
struct Found {
    package: String,
    waits: BTreeSet<&'static str>,
    measures: bool,
    /// Shape 2: clock arithmetic in its own body.
    arithmetic: bool,
    /// Shape 3: the test-only helpers that measure, which it reaches.
    helpers: BTreeSet<String>,
    /// Shape 3 (plan row 208): the test-only helpers it reaches that wait, or measure without
    /// keeping it — a wait, a stall timer, clock arithmetic in their own body — and that are not
    /// doors.
    waiting: BTreeSet<String>,
    /// A door (plan row 208): a test-only helper that waits, which is itself a row; the walk
    /// stops there, and the tests that reach it are not rows through it.
    door: Option<DoorKind>,
}

/// What a door row says it is (row 20 review N4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DoorKind {
    /// A helper within whose wait the tests that reach it await their event.
    Waits,
    /// A helper that keeps a measurement of the operation the tests that reach it run.
    Measures,
    /// An impl only an implicit call runs, in a test target that declares no `#[test]` (built
    /// without the test harness): the target's own run is the test.
    OwnRun,
    /// An impl only an implicit call runs, in an example: no test runs it.
    Example,
}

impl Found {
    /// The row this test would have if it were listed today.
    fn row(&self, identity: &str) -> String {
        if let Some(kind) = self.door {
            let (assumption, seam) = match kind {
                DoorKind::Waits => (
                    "the tests that reach it await their event within its wait (a door: waits in its \
                     own body; its callers are not rows)",
                    "completion signal, controlled receiver or controlled sleeper",
                ),
                DoorKind::Measures => (
                    "the tests that reach it finish their operation within the real time it measures \
                     (a door: measures in its own body; its callers are not rows)",
                    "operation counter or controlled clock",
                ),
                DoorKind::OwnRun => (
                    "the target's own run awaits its event within this wait (a test target without \
                     the test harness: no #[test] can name it, so the impl that waits, or the \
                     helper only such an impl reaches, is the row)",
                    "completion signal, controlled receiver or controlled sleeper",
                ),
                DoorKind::Example => (
                    "no test runs it: an example's own run waits or measures within it (a row because \
                     only an implicit call runs or reaches it)",
                    "none while no test runs the example",
                ),
            };
            return format!("{identity}\t{}\t{assumption}\t{seam}", self.package);
        }
        if !self.waits.is_empty() || self.measures {
            let calls: Vec<&str> = self
                .waits
                .iter()
                .copied()
                .chain(self.measures.then_some("elapsed"))
                .collect();
            let (assumption, seam) = match (self.waits.is_empty(), self.measures) {
                (false, false) => (
                    "the awaited event happens within the numeric wait",
                    "completion signal, controlled receiver or controlled sleeper",
                ),
                (true, true) => (
                    "the operation finishes within the measured real time",
                    "operation counter or controlled clock",
                ),
                _ => (
                    "the awaited event happens within the numeric wait, and the measured real time stays within its bound",
                    "completion signal plus operation counter or controlled clock",
                ),
            };
            return format!(
                "{identity}\t{}\t{assumption} (calls {} in its own body)\t{seam}",
                self.package,
                calls.join(", ")
            );
        }
        let mut assumptions = Vec::new();
        let mut seams = Vec::new();
        if self.arithmetic {
            assumptions.push(
                "two real-clock readings stand within the distance its comparison allows (clock arithmetic in its own body)"
                    .to_owned(),
            );
            seams.push("one reading handed down, or a completion signal");
        }
        if !self.helpers.is_empty() {
            assumptions.push(format!(
                "the operation finishes within the real time its helper measures (measures in its helper {})",
                self.helpers.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
            seams.push("operation counter or controlled clock");
        }
        if !self.waiting.is_empty() {
            assumptions.push(format!(
                "the awaited event happens within the real time its helper waits or measures (waits in its helper {})",
                self.waiting.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
            seams.push("completion signal, controlled receiver or controlled sleeper");
        }
        format!(
            "{identity}\t{}\t{}\t{}",
            self.package,
            assumptions.join("; "),
            seams.join(", plus ")
        )
    }
}

/// The kernel waits a scan reads: [`KERNEL`] in every scan of the tree; a test
/// of the scan hands an empty one to show what the widening alone finds.
struct Kernel<'a> {
    /// Matched by the called name, as [`KERNEL_WAITS`].
    calls: &'a [&'static str],
    /// Matched by path, as [`KERNEL_WAIT_PATHS`].
    paths: &'a [&'static str],
    /// The paths that also name a type, as [`KERNEL_WAIT_TYPES`].
    typed: &'a [&'static str],
}

/// The kernel waits of plan row 163.
const KERNEL: Kernel<'static> = Kernel {
    calls: &KERNEL_WAITS,
    paths: &KERNEL_WAIT_PATHS,
    typed: &KERNEL_WAIT_TYPES,
};

/// Every test of one index whose own body holds shape 1 or shape 2, added to
/// `found` by identity; what shape 2's reading cannot decide goes to `refusals`.
fn scan_index(
    index: &Index,
    package: &str,
    found: &mut BTreeMap<String, Found>,
    refusals: &mut Vec<String>,
) {
    scan_index_reading(index, package, &KERNEL, found, refusals);
}

/// **Every shape-1 site of one index**: the calls of [`REAL_WAITS`], of the kernel's waits and of
/// [`MEASURES`], searched once and read twice — for each test's own body (shape 1) and for each
/// test-only helper the walk reaches (shape 3, plan row 208).
struct Sites {
    /// Each trigger, whether it is a measurement, and its sites, sorted.
    hits: Vec<(&'static str, bool, Vec<Span>)>,
}

impl Sites {
    fn of(index: &Index, kernel: &Kernel<'_>) -> Self {
        let hits = REAL_WAITS
            .iter()
            .chain(kernel.calls.iter())
            .map(|name| (*name, false, Pattern::call(name)))
            .chain(
                MEASURES
                    .iter()
                    .map(|name| (*name, true, Pattern::call(name))),
            )
            .chain(
                kernel
                    .paths
                    .iter()
                    .map(|path| (*path, false, Pattern::path(path))),
            )
            .map(|(name, measures, pattern)| {
                let path = matches!(pattern, Pattern::Path(_));
                let search = Search::new(needle!(pattern), View::Identifiers);
                let mut sites = spans(index, &search);
                // A path wait is a wait where it is called: `libc::kevent` is a
                // struct as well, and every other mention is refused.
                if path {
                    sites.retain(|site| called_after(index, *site));
                }
                (name, measures, sites)
            })
            .collect();
        Self { hits }
    }

    /// The waits called inside `body`, and how many measurements are.
    fn called_inside(&self, body: Span) -> (BTreeSet<&'static str>, usize) {
        let mut waits = BTreeSet::new();
        let mut measures = 0;
        for (name, measuring, sites) in &self.hits {
            let called = inside(sites, body).count();
            if *measuring {
                measures += called;
            } else if called > 0 {
                waits.insert(*name);
            }
        }
        (waits, measures)
    }
}

/// [`scan_index`] with the kernel waits `kernel`.
fn scan_index_reading(
    index: &Index,
    package: &str,
    kernel: &Kernel<'_>,
    found: &mut BTreeMap<String, Found>,
    refusals: &mut Vec<String>,
) {
    let attributes = test_attributes(index);
    let sites = Sites::of(index, kernel);
    refusals.extend(kernel_wait_refusals(index, kernel));
    for item in index.items() {
        let Some(body) = item.body() else { continue };
        if !is_test(&attributes, item) {
            continue;
        }
        let (waits, measures) = sites.called_inside(body);
        let measured = measures > 0;
        let arithmetic = match read_callable(index, item) {
            Ok(read) => {
                let base = item.whole().start();
                refusals.extend(
                    read.refusals
                        .iter()
                        .map(|(at, why)| format!("{}: {why}", place(index, base + at))),
                );
                !read.arithmetic.is_empty()
            }
            Err(refusal) => {
                refusals.push(refusal);
                false
            }
        };
        if waits.is_empty() && !measured && !arithmetic {
            continue;
        }
        for identity in identities(index, item) {
            let entry = found.entry(identity).or_default();
            entry.package = package.to_owned();
            entry.waits.extend(waits.iter().copied());
            entry.measures |= measured;
            entry.arithmetic |= arithmetic;
        }
    }
}

/// Whether a test-only callable waits or measures, or reaches, through test-only callables of
/// the target that are not doors, one that does, or why that cannot be told.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Reach {
    Yes,
    No,
    /// Its body, or a call on its way, cannot be read: the reasons, by file and line.
    CannotTell(Vec<String>),
}

/// What a test-only callable's own body holds of the real clock (shape 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Holds {
    /// A measurement whose value is kept: returned, stored, pushed, handed on (T-TIMING-BOUND-STRAYS).
    Measurement,
    /// A real wait, an `elapsed` whose value is not kept (a stall timer), or clock arithmetic
    /// between two readings: shape 1 or shape 2 written in a helper (plan row 208).
    Wait,
    /// Neither.
    Nothing,
}

/// What one walk from a test reached.
#[derive(Default)]
struct Walked {
    /// The test-only callables that keep a measurement.
    measuring: BTreeSet<ItemId>,
    /// The test-only callables that wait (see [`Holds::Wait`]) and are not doors.
    waiting: BTreeSet<ItemId>,
    /// The doors reached: the walk stops at each.
    doors: BTreeSet<ItemId>,
    /// What the walk cannot decide, by file and line.
    refusals: Vec<String>,
}

/// **Shape 3**: the test-only helpers that wait or measure, and the walk to them from a test
/// through the call graph's resolved edges, which stops at a door.
struct Helpers<'r, 'i> {
    resolver: &'r Resolver<'i>,
    graph: CallGraph<'r, 'i>,
    kernel: &'r Kernel<'r>,
    /// The identities of the list's rows: a test-only callable answering to one is a door.
    doors: &'r BTreeSet<String>,
    /// Per package, its shape-1 sites.
    sites: RefCell<HashMap<String, Arc<Sites>>>,
    readings: RefCell<HashMap<ItemId, Result<Holds, String>>>,
    reaches: RefCell<HashMap<ItemId, Reach>>,
    /// The callables whose reach is being asked now, outermost first: one asked
    /// again on its own way answers `No` for that way, and an answer decided on
    /// such an open one is not kept.
    asking: RefCell<Vec<ItemId>>,
    leaned_on_open: Cell<bool>,
    /// Per package, its test-only callables.
    test_callables: RefCell<HashMap<String, Vec<ItemId>>>,
    /// The doors a walk or a candidate's reach has stopped at: a door met only as a candidate
    /// of a call the graph does not resolve is in use as much as one met through an edge (its
    /// row is what keeps that call from being refused).
    met: RefCell<BTreeSet<ItemId>>,
}

impl<'r, 'i> Helpers<'r, 'i> {
    fn new(
        resolver: &'r Resolver<'i>,
        kernel: &'r Kernel<'r>,
        doors: &'r BTreeSet<String>,
    ) -> Self {
        Self {
            resolver,
            graph: CallGraph::new(resolver),
            kernel,
            doors,
            sites: RefCell::default(),
            readings: RefCell::default(),
            reaches: RefCell::default(),
            asking: RefCell::default(),
            leaned_on_open: Cell::new(false),
            test_callables: RefCell::default(),
            met: RefCell::default(),
        }
    }

    /// Whether the language lets `from`'s body call `id`: a function or an inherent
    /// method written without `pub` is private to the module that declares it and
    /// its descendants; a trait's method, or anything `pub`, is taken as visible.
    fn visible_from(&self, id: ItemId, from: ItemId) -> bool {
        let index = self.resolver.index_of(id);
        let record = self.resolver.record(id);
        if record.trait_name().is_some()
            || !reading::private(
                index.text(record.whole()),
                record.kind() == ItemKind::Function,
                record.type_owner().is_some(),
            )
        {
            return true;
        }
        let caller = self.resolver.record(from);
        record.module_paths().iter().any(|declared| {
            caller
                .module_paths()
                .iter()
                .any(|calling| calling == declared || calling.starts_with(&format!("{declared}::")))
        })
    }

    fn test_only(&self, id: ItemId) -> bool {
        test_only(self.resolver.index_of(id), self.resolver.record(id))
    }

    fn of_target(&self, id: ItemId, within: &BTreeSet<String>) -> bool {
        !targets(self.resolver.index_of(id), self.resolver.record(id)).is_disjoint(within)
    }

    /// Whether test-only `id` is a door: a row of the list names it.
    fn door(&self, id: ItemId) -> bool {
        callable_identities(self.resolver.index_of(id), self.resolver.record(id))
            .iter()
            .any(|identity| self.doors.contains(identity))
    }

    /// `id`'s package's shape-1 sites.
    fn shape_one_sites_of(&self, id: ItemId) -> Arc<Sites> {
        let package = self.resolver.package(id);
        if let Some(known) = self.sites.borrow().get(package) {
            return Arc::clone(known);
        }
        let sites = Arc::new(Sites::of(self.resolver.index_of(id), self.kernel));
        self.sites
            .borrow_mut()
            .insert(package.to_owned(), Arc::clone(&sites));
        sites
    }

    /// What test-only `id`'s own body holds of the real clock, or why it cannot be told. A
    /// wait or a measurement written in it decides the answer whatever its reading refuses;
    /// what the reading alone could say (a kept measurement, clock arithmetic, a measurement
    /// only printed) is asked of it only when it reads. An `elapsed` whose every value is only
    /// printed or formatted decides nothing, so it is no clock (row 20 review N2); one the
    /// reading does not follow to such a use still counts.
    fn clock_in_body(&self, id: ItemId) -> Result<Holds, String> {
        if let Some(known) = self.readings.borrow().get(&id) {
            return known.clone();
        }
        let index = self.resolver.index_of(id);
        let record = self.resolver.record(id);
        let (waits, measures) = record
            .body()
            .map(|body| self.shape_one_sites_of(id).called_inside(body))
            .unwrap_or_default();
        let written = !waits.is_empty() || measures > 0;
        let answer = match read_callable(index, record) {
            Ok(read) if read.refusals.is_empty() => Ok(if !read.kept.is_empty() {
                Holds::Measurement
            } else if !waits.is_empty()
                || !read.arithmetic.is_empty()
                || measures > read.only_printed()
            {
                Holds::Wait
            } else {
                Holds::Nothing
            }),
            _ if written => Ok(Holds::Wait),
            Ok(read) => {
                let (at, why) = &read.refusals[0];
                Err(format!(
                    "{}: {why}",
                    place(index, record.whole().start() + at)
                ))
            }
            Err(refusal) => Err(refusal),
        };
        self.readings.borrow_mut().insert(id, answer.clone());
        answer
    }

    fn name(&self, id: ItemId) -> String {
        let record = self.resolver.record(id);
        record.type_owner().map_or_else(
            || record.name().to_owned(),
            |owner| format!("{owner}::{}", record.name()),
        )
    }

    /// `package`'s test-only callables.
    fn test_callables(&self, package: &str) -> Vec<ItemId> {
        if let Some(known) = self.test_callables.borrow().get(package) {
            return known.clone();
        }
        let found: Vec<ItemId> = self
            .resolver
            .callables(package)
            .into_iter()
            .filter(|id| self.test_only(*id))
            .collect();
        self.test_callables
            .borrow_mut()
            .insert(package.to_owned(), found.clone());
        found
    }

    /// Whether test-only `id` waits or measures, or reaches, through test-only callables of
    /// `within` that are not doors, one that does; a door answers `No` (the tests that reach it
    /// are not rows through it); what its own body or the walk from it cannot decide makes the
    /// answer [`Reach::CannotTell`], never `No`.
    fn reach(&self, id: ItemId, within: &BTreeSet<String>) -> Reach {
        if self.door(id) {
            self.met.borrow_mut().insert(id);
            return Reach::No;
        }
        if let Some(known) = self.reaches.borrow().get(&id) {
            return known.clone();
        }
        if self.asking.borrow().contains(&id) {
            self.leaned_on_open.set(true);
            return Reach::No;
        }
        let outer = self.leaned_on_open.replace(false);
        self.asking.borrow_mut().push(id);
        let answer = match self.clock_in_body(id) {
            Ok(Holds::Measurement | Holds::Wait) => Reach::Yes,
            Err(refusal) => Reach::CannotTell(vec![refusal]),
            Ok(Holds::Nothing) => {
                let walked = self.walk(id, within);
                if !walked.measuring.is_empty() || !walked.waiting.is_empty() {
                    Reach::Yes
                } else if walked.refusals.is_empty() {
                    Reach::No
                } else {
                    Reach::CannotTell(walked.refusals)
                }
            }
        };
        self.asking.borrow_mut().pop();
        let leaned = self.leaned_on_open.get();
        if !leaned || self.asking.borrow().is_empty() {
            self.reaches.borrow_mut().insert(id, answer.clone());
        }
        self.leaned_on_open.set(outer || leaned);
        answer
    }

    /// The reach of each of `candidates` the language lets `from` call, as a
    /// refusal for `what` when any of them waits, measures, reaches one that does,
    /// or cannot be told.
    fn refuse_candidates(
        &self,
        candidates: impl IntoIterator<Item = ItemId>,
        from: ItemId,
        within: &BTreeSet<String>,
        what: &str,
    ) -> Option<String> {
        let mut measuring = Vec::new();
        let mut unknown = Vec::new();
        for candidate in candidates {
            if !self.test_only(candidate)
                || !self.of_target(candidate, within)
                || !self.visible_from(candidate, from)
            {
                continue;
            }
            match self.reach(candidate, within) {
                Reach::Yes => measuring.push(self.name(candidate)),
                Reach::No => {}
                Reach::CannotTell(why) => {
                    unknown.push(format!("{} ({})", self.name(candidate), why.join("; ")));
                }
            }
        }
        if !measuring.is_empty() {
            return Some(format!(
                "{what}, and a candidate measures real time or waits on it, or reaches a helper \
                 that does: {}",
                measuring.join(", ")
            ));
        }
        if !unknown.is_empty() {
            return Some(format!(
                "{what}, and whether a candidate reaches a wait or a measurement cannot be told: {}",
                unknown.join(", ")
            ));
        }
        None
    }

    /// From `root`, every test-only callable of `within` that waits or measures and is reached
    /// through resolved edges, the doors where the walk stops, and what the walk cannot decide.
    fn walk(&self, root: ItemId, within: &BTreeSet<String>) -> Walked {
        let mut stack = vec![root];
        let mut seen = BTreeSet::from([root]);
        let mut walked = Walked::default();
        while let Some(at) = stack.pop() {
            let index = self.resolver.index_of(at);
            let package = self.resolver.package(at);
            for edge in self.graph.edges(Node::Item(at)).iter() {
                let site = || place(index, edge.site.span.start());
                match &edge.to {
                    Target::Item(to) => {
                        if !self.test_only(*to) || !self.of_target(*to, within) {
                            continue;
                        }
                        if self.door(*to) {
                            walked.doors.insert(*to);
                            self.met.borrow_mut().insert(*to);
                            continue;
                        }
                        match self.clock_in_body(*to) {
                            Ok(Holds::Measurement) => {
                                walked.measuring.insert(*to);
                            }
                            Ok(Holds::Wait) => {
                                walked.waiting.insert(*to);
                            }
                            Ok(Holds::Nothing) => {}
                            Err(refusal) => walked.refusals.push(refusal),
                        }
                        if seen.insert(*to) {
                            stack.push(*to);
                        }
                    }
                    Target::Unresolved(
                        Unresolved::Dynamic { candidates, .. }
                        | Unresolved::Ambiguous { candidates, .. }
                        | Unresolved::OtherArm { candidates },
                    ) => {
                        let what = format!(
                            "{}: a call to `{}` the call graph does not resolve",
                            site(),
                            edge.site.name
                        );
                        walked.refusals.extend(self.refuse_candidates(
                            candidates.iter().copied(),
                            at,
                            within,
                            &what,
                        ));
                    }
                    Target::Unresolved(Unresolved::Path { written, .. }) => {
                        let last = written.rsplit("::").next().unwrap_or(written);
                        let named = self
                            .test_callables(package)
                            .into_iter()
                            .filter(|id| self.resolver.record(*id).name() == last);
                        let what = format!(
                            "{}: the path `{written}` names nothing the call graph places, and is \
                             read as any test-only callable of that name",
                            site()
                        );
                        walked
                            .refusals
                            .extend(self.refuse_candidates(named, at, within, &what));
                    }
                    Target::Unresolved(Unresolved::Unparsed { why }) => walked.refusals.push(
                        format!("{}: a body on the walk does not parse ({why})", site()),
                    ),
                    Target::Unresolved(Unresolved::Cutoff { what }) => walked
                        .refusals
                        .push(format!("{}: the walk was cut off at {what}", site())),
                    Target::Unresolved(Unresolved::Macro { name }) => {
                        let defined = index.macros().iter().any(|record| {
                            record.kind() == MacroKind::Definition
                                && defined_macro(index, record) == name.rsplit("::").next()
                                && names_a_clock_or_a_wait(index.text(record.tokens()))
                        });
                        if defined {
                            walked.refusals.push(format!(
                                "{}: the macro `{name}!`'s expansion is not followed, and its definition here names a clock or a wait",
                                site()
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        walked
    }
}

/// Whether a `macro_rules!` definition's tokens name a clock or a wait: `elapsed`, `Instant`,
/// a [`REAL_WAITS`] or [`KERNEL_WAITS`] name, or a [`KERNEL_WAIT_PATHS`] path's two names.
fn names_a_clock_or_a_wait(tokens: &str) -> bool {
    let names: BTreeSet<&str> = tokens
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|name| !name.is_empty())
        .collect();
    ["elapsed", "Instant"]
        .iter()
        .chain(REAL_WAITS.iter())
        .chain(KERNEL_WAITS.iter())
        .any(|name| names.contains(name))
        || KERNEL_WAIT_PATHS
            .iter()
            .any(|path| path.split("::").all(|segment| names.contains(segment)))
}

/// The name a `macro_rules!` definition defines.
fn defined_macro<'i>(index: &'i Index, record: &bt_source::MacroRecord) -> Option<&'i str> {
    let written = index.text(record.span());
    let rest = written.split_once('!')?.1.trim_start();
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

/// Every test of every package that reaches a test-only helper that waits or measures (shape
/// 3), added to `found`; every door of `doors` that waits or measures and is reached — through
/// an edge, or as a candidate of a call the graph does not resolve — or runs only by an implicit
/// call, added to `found` as a door; what the walk cannot decide, and a
/// test-only impl only an implicit call runs that waits and is no door, go to `refusals`.
/// Answers, for each helper a test reached that waits or measures, and each door reached, how
/// many tests reach it ([`leave_rendering`] lists them for a ticket choosing its doors).
fn scan_helpers(
    indices: &[(String, Arc<Index>)],
    doors: &BTreeSet<String>,
    found: &mut BTreeMap<String, Found>,
    refusals: &mut Vec<String>,
) -> BTreeMap<String, HelperUse> {
    let packages: Vec<(&str, &Index)> = indices
        .iter()
        .map(|(name, index)| (name.as_str(), &**index))
        .collect();
    let resolver = Resolver::new(&packages, Arms::filtered(|_| true))
        .unwrap_or_else(|refused| panic!("{refused:?}"));
    let helpers = Helpers::new(&resolver, &KERNEL, doors);
    let mut reached: BTreeMap<String, HelperUse> = BTreeMap::new();
    // The targets that declare a `#[test]`: an implicit door elsewhere is no test's door.
    let mut tested: BTreeSet<String> = BTreeSet::new();
    for (package, index) in indices {
        let attributes = test_attributes(index);
        for record in index.items() {
            if !is_test(&attributes, record) {
                continue;
            }
            let Some(id) = resolver.id_of(package, record) else {
                continue;
            };
            let within = targets(index, record);
            tested.extend(within.iter().cloned());
            let walked = helpers.walk(id, &within);
            refusals.extend(walked.refusals);
            let tests = identities(index, record).len();
            for (hit, door) in walked
                .measuring
                .iter()
                .chain(&walked.waiting)
                .map(|hit| (hit, false))
                .chain(walked.doors.iter().map(|hit| (hit, true)))
            {
                for helper in callable_identities(resolver.index_of(*hit), resolver.record(*hit)) {
                    let entry = reached.entry(helper).or_default();
                    entry.door = door;
                    entry.tests += tests;
                }
            }
            if walked.measuring.is_empty() && walked.waiting.is_empty() {
                continue;
            }
            for identity in identities(index, record) {
                let entry = found.entry(identity).or_default();
                entry.package = package.clone();
                entry
                    .helpers
                    .extend(walked.measuring.iter().map(|hit| helpers.name(*hit)));
                entry
                    .waiting
                    .extend(walked.waiting.iter().map(|hit| helpers.name(*hit)));
            }
        }
    }
    // A scope's drop, an iterator's `next` behind `for`, an operator: no edge leads into them,
    // so a test-only one that waits is a door or refused.
    let implicit: BTreeMap<ItemId, String> = resolver
        .implicit()
        .into_iter()
        .flat_map(|(path, ids)| ids.into_iter().map(move |id| (id, path.clone())))
        .collect();
    for (id, path) in &implicit {
        if !helpers.test_only(*id) || helpers.door(*id) {
            continue;
        }
        let index = resolver.index_of(*id);
        let record = resolver.record(*id);
        let at = place(index, record.whole().start());
        // Its own body or any test-only helper it reaches (row 20 review N1): the walk is
        // transitive here as it is from a test.
        match helpers.reach(*id, &targets(index, record)) {
            Reach::Yes => refusals.push(format!(
                "{at}: `{}` is a test-only impl of `{path}` that only an implicit call runs, and \
                 it waits on or measures real time, or reaches a test-only helper that does; no \
                 call-graph edge leads into it, so no test that runs it is seen — make it a door \
                 row, or move the wait out of it",
                helpers.name(*id)
            )),
            Reach::No => {}
            Reach::CannotTell(why) => refusals.push(format!(
                "{at}: `{}` is a test-only impl of `{path}` that only an implicit call runs, and \
                 whether it waits cannot be told ({})",
                helpers.name(*id),
                why.join("; ")
            )),
        }
    }
    for (package, _) in indices {
        for id in helpers.test_callables(package) {
            if !helpers.door(id)
                || !(helpers.met.borrow().contains(&id) || implicit.contains_key(&id))
            {
                continue;
            }
            let measures = match helpers.clock_in_body(id) {
                Ok(Holds::Measurement) => true,
                Ok(Holds::Wait) => false,
                _ => continue,
            };
            let index = resolver.index_of(id);
            let record = resolver.record(id);
            let compiled_by = targets(index, record);
            let kind = if !compiled_by.is_disjoint(&tested) {
                if measures {
                    DoorKind::Measures
                } else {
                    DoorKind::Waits
                }
            } else if compiled_by
                .iter()
                .all(|target| target.contains(":example:"))
            {
                DoorKind::Example
            } else {
                DoorKind::OwnRun
            };
            for identity in callable_identities(index, record) {
                if !doors.contains(&identity) {
                    continue;
                }
                let entry = found.entry(identity).or_default();
                entry.package = package.clone();
                entry.door = Some(kind);
            }
        }
    }
    reached
}

/// A helper the walk reached that waits or measures, or a door.
#[derive(Default)]
struct HelperUse {
    door: bool,
    /// The tests that reach it (one per identity).
    tests: usize,
}

/// Every function or method an index declares under a trigger's name.
fn declared_triggers(index: &Index) -> Vec<String> {
    index
        .items()
        .iter()
        .filter(|item| item.kind().is_callable())
        .filter(|item| {
            REAL_WAITS.contains(&item.name())
                || KERNEL_WAITS.contains(&item.name())
                || MEASURES.contains(&item.name())
        })
        .flat_map(|item| item.identities().map(|identity| identity.to_string()))
        .collect()
}

/// Where the file holding `span` ends, in the union.
fn file_end(index: &Index, span: Span) -> usize {
    index
        .file_at(span.start())
        .map_or(index.union().len(), |file| file.span().end())
}

/// The code of the union from `start` to `end`, its comments left out.
fn code_between(index: &Index, start: usize, end: usize) -> String {
    let union = index.union();
    let comments = index.comments();
    let first = comments.partition_point(|comment| comment.span().end() <= start);
    let mut code = String::new();
    let mut at = start;
    for comment in comments[first..]
        .iter()
        .take_while(|comment| comment.span().start() < end)
    {
        let span = comment.span();
        code.push_str(&union[at..span.start().max(at)]);
        at = at.max(span.end().min(end));
    }
    code.push_str(&union[at..end]);
    code
}

/// Whether the code right after `span`, in its file and past any comment,
/// opens a call.
fn called_after(index: &Index, span: Span) -> bool {
    let union = index.union();
    let comments = index.comments();
    let stop = file_end(index, span);
    let mut next_comment = comments.partition_point(|comment| comment.span().end() <= span.end());
    let mut at = span.end();
    while at < stop {
        if let Some(comment) = comments.get(next_comment)
            && comment.span().start() <= at
        {
            at = at.max(comment.span().end());
            next_comment += 1;
            continue;
        }
        let Some(next) = union[at..stop].chars().next() else {
            return false;
        };
        if !next.is_whitespace() {
            return next == '(';
        }
        at += next.len_utf8();
    }
    false
}

/// **What [`KERNEL_WAITS`] and [`KERNEL_WAIT_PATHS`] cannot read, refused by
/// file and line** (plan row 163), anywhere in `index`: a path wait named by its
/// path and not called right there (a [`KERNEL_WAIT_TYPES`] path only where it
/// is imported), a group or glob of a path's root, a rename of the root, a
/// renamed kernel wait, and a kernel wait taken as a value. Each would let a
/// test call a wait under a name the scan does not match; none is written in the
/// tree.
fn kernel_wait_refusals(index: &Index, kernel: &Kernel<'_>) -> Vec<String> {
    let tokens = index.tokens();
    let name = |at: usize| index.text(tokens[at].name_span());
    let file = |at: usize| {
        index
            .file_at(tokens[at].span().start())
            .map(|file| file.span().start())
    };
    // The token after `at` when it is in the same file.
    let next = |at: usize| (at + 1 < tokens.len() && file(at) == file(at + 1)).then_some(at + 1);
    // The code between `at` and the next token in its file, or the rest of the
    // file after the last one, comments left out and trimmed.
    let gap = |at: usize| {
        let end = match next(at) {
            Some(after) => tokens[after].span().start(),
            None => file_end(index, tokens[at].span()),
        };
        code_between(index, tokens[at].span().end(), end)
            .trim()
            .to_owned()
    };
    let called = |at: usize| called_after(index, tokens[at].span());
    let renamed =
        |at: usize| next(at).is_some_and(|after| name(after) == "as") && gap(at).is_empty();
    // Inside a `use` tree: back to a `use`, only names (`as` among them), the
    // tree's own punctuation and comments stand between.
    let imported = |at: usize| {
        let mut at = at;
        while at > 0 && file(at - 1) == file(at) {
            let between = code_between(
                index,
                tokens[at - 1].span().end(),
                tokens[at].span().start(),
            );
            if !between
                .chars()
                .all(|c| c.is_whitespace() || matches!(c, ':' | '{' | '}' | ','))
            {
                return false;
            }
            at -= 1;
            if name(at) == "use" {
                return true;
            }
        }
        false
    };
    let declared = |at: usize| at > 0 && file(at - 1) == file(at) && name(at - 1) == "fn";
    let roots: BTreeSet<&str> = kernel
        .paths
        .iter()
        .filter_map(|path| path.split("::").next())
        .collect();
    let path_wait = |root: &str, segment: &str| {
        kernel.paths.iter().copied().find(|path| {
            path.strip_prefix(root)
                .and_then(|rest| rest.strip_prefix("::"))
                == Some(segment)
        })
    };
    let mut refusals = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        let here = index.text(token.name_span());
        let why = if kernel.calls.contains(&here) {
            if renamed(at) {
                Some(format!(
                    "`{here}` is renamed; the timing scan reads a kernel wait by its own called \
                     name, so a test calling the new name would not be seen — call it as `{here}`"
                ))
            } else if !called(at) && !imported(at) && !declared(at) {
                Some(format!(
                    "`{here}` is taken as a value here, not called; the timing scan reads a \
                     kernel wait as a call of its own name, so a call through the value would \
                     not be seen — call `{here}(` where it waits"
                ))
            } else {
                None
            }
        } else if roots.contains(here) {
            let segment = next(at).map(|after| (after, name(after)));
            let between = gap(at);
            match (between.as_str(), segment) {
                ("", Some((_, "as"))) => Some(format!(
                    "`{here}` is renamed; the timing scan reads a {here} wait by its `{here}::` \
                     path, so a wait under the new name would not be seen"
                )),
                ("::", Some((after, segment))) => match path_wait(here, segment) {
                    Some(path)
                        if !called(after) && (imported(at) || !kernel.typed.contains(&path)) =>
                    {
                        Some(format!(
                            "`{path}` is named and not called here (an import, a rename or a \
                             value); the timing scan reads a {here} wait only as a call of its \
                             path, so a test calling it under another name would not be seen — \
                             call `{path}(` where it waits"
                        ))
                    }
                    _ => None,
                },
                (between, _)
                    if between.starts_with("::")
                        && (between.contains('{') || between.contains('*')) =>
                {
                    Some(format!(
                        "a group or a glob of `{here}` is imported; it could bring a {here} wait \
                         in under a name the timing scan does not read — name each {here} item \
                         by its path"
                    ))
                }
                _ => None,
            }
        } else {
            None
        };
        if let Some(why) = why {
            refusals.push(format!("{}: {why}", place(index, token.span().start())));
        }
    }
    refusals
}

/// Every first-party package's index: the one this process holds for each package's
/// [`universes::first_party_whole`], shared with every other guard that asks for it.
pub(crate) fn workspace_indices() -> Vec<(String, Arc<Index>)> {
    let workspace = Workspace::read(&workspace_root()).expect("this workspace");
    workspace
        .packages()
        .iter()
        .filter(|package| !is_vendored(package.directory()))
        .map(|package| {
            let index = Index::shared(&universe_for(package))
                .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
            (package.name().to_owned(), index)
        })
        .collect()
}

/// A fixture directory under `tests/fixtures/`, as the library `package`.
fn fixture_named(directory: &str, package: &str) -> Index {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(directory);
    let universe = Universe::declare(
        format!("the {package} fixture"),
        vec![TargetRoot {
            id: TargetId {
                package: package.to_owned(),
                kind: TargetKind::Library,
                name: package.to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

/// The `timing` fixture: a controlled clock whose method is named like a wait,
/// and a real sleep.
fn fixture() -> Index {
    fixture_named("timing", "timing")
}

/// The committed rows, by test identity, with their crate column.
fn committed() -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(workspace_root().join(LIST)).expect("the list is committed");
    let mut lines = text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    assert_eq!(lines.next(), Some(HEADER), "{LIST}'s column header");
    let mut rows = BTreeMap::new();
    for line in lines {
        let columns: Vec<&str> = line.split('\t').collect();
        assert_eq!(columns.len(), 4, "{LIST}: a row is four columns: {line}");
        assert!(
            rows.insert(columns[0].to_owned(), columns[1].to_owned())
                .is_none(),
            "{LIST} names {} twice",
            columns[0]
        );
    }
    rows
}

/// Leave the list this scan expects where a ticket can copy rows from it.
fn leave_rendering(found: &BTreeMap<String, Found>, reached: &BTreeMap<String, HelperUse>) {
    let out = workspace_root().join("target").join("timing-bound-tests");
    std::fs::create_dir_all(&out).expect("target/ is writable");
    let mut text = String::from(HEADER);
    text.push('\n');
    for (identity, test) in found {
        text.push_str(&test.row(identity));
        text.push('\n');
    }
    std::fs::write(out.join("TIMING-BOUND-TESTS.tsv"), text).expect("written");
    let mut helpers = String::from("helper\tdoor\ttests reaching it\n");
    for (identity, helper) in reached {
        helpers.push_str(&format!("{identity}\t{}\t{}\n", helper.door, helper.tests));
    }
    std::fs::write(out.join("WAITING-HELPERS.tsv"), helpers).expect("written");
}

/// RED (T-GATES-047; widened by T-TIMING-BOUND-STRAYS) — **every test whose
/// verdict the scan's three shapes tie to the real clock is a row of the
/// timing-bound list, and every row is such a test; nothing the readings cannot
/// decide is passed over.**
///
/// The list was first written by attributing clock calls to the nearest test
/// above them, which listed source-reading tests for a product function's
/// `Instant` and could not see a new test at all. This is the list's other
/// half: the list only shrinks against the merge base
/// (`scripts/ci/check-timing-bound.ps1`), and here the tree is held to the
/// list, so a new timing-bound test cannot land by leaving the list alone.
///
/// MUTATION: add `#[test] fn planted() { std::thread::sleep(std::time::Duration::from_millis(1)); }`
/// to any crate and this names it as unlisted; so does
/// `#[test] fn planted() { let a = std::time::Instant::now(); assert!(std::time::Instant::now() > a); }`;
/// delete one row and this names that test.
#[test]
fn the_timing_bound_tests_are_exactly_the_listed_ones() {
    let indices = workspace_indices();
    let mut found = BTreeMap::new();
    let mut misnamed = Vec::new();
    let mut refusals = Vec::new();
    for (package, index) in &indices {
        scan_index(index, package, &mut found, &mut refusals);
        misnamed.extend(declared_triggers(index));
    }
    let listed = committed();
    let doors: BTreeSet<String> = listed.keys().cloned().collect();
    let reached = scan_helpers(&indices, &doors, &mut found, &mut refusals);
    leave_rendering(&found, &reached);
    assert!(
        misnamed.is_empty(),
        "the scan reads these names as the standard library's real waits and measurements, and \
         the workspace declares functions by them — rename each (a controlled clock is not a \
         real wait), so no test calling it is listed as timing-bound:\n  {}",
        misnamed.join("\n  ")
    );
    assert!(
        refusals.is_empty(),
        "the timing scan cannot decide these, and does not pass over them ({}):\n  {}",
        refusals.len(),
        refusals.join("\n  ")
    );
    println!(
        "{} timing-bound tests found, {} listed",
        found.len(),
        listed.len()
    );

    let unlisted: Vec<String> = found
        .iter()
        .filter(|(identity, _)| !listed.contains_key(*identity))
        .map(|(identity, test)| format!("  {}", test.row(identity)))
        .collect();
    let stale: Vec<String> = listed
        .keys()
        .filter(|identity| !found.contains_key(*identity))
        .map(|identity| format!("  {identity}"))
        .collect();
    let wrong_crate: Vec<String> = found
        .iter()
        .filter_map(|(identity, test)| {
            let written = listed.get(identity)?;
            (written != &test.package)
                .then(|| format!("  {identity}: {written} is not {}", test.package))
        })
        .collect();
    assert!(
        unlisted.is_empty() && stale.is_empty() && wrong_crate.is_empty(),
        "{LIST} and the tree disagree.\n\n\
         Timing-bound tests that are not listed ({}) — give each a seam instead of the real \
         clock (a completion signal, a controlled receiver, sleeper or clock, one reading handed \
         down); a row is never added to let one past, and scripts/ci/check-timing-bound.ps1 \
         refuses one:\n{}\n\n\
         Rows that name no timing-bound test ({}) — the test was converted, renamed or moved; \
         remove the row (a moved test that still waits is a new row, and refused):\n{}\n\n\
         Rows with the wrong crate ({}):\n{}\n\n\
         The list this tree implies is in target/timing-bound-tests/TIMING-BOUND-TESTS.tsv.",
        unlisted.len(),
        unlisted.join("\n"),
        stale.len(),
        stale.join("\n"),
        wrong_crate.len(),
        wrong_crate.join("\n"),
    );
}

/// RED (T-GATES-047 round 4) — **a controlled clock's method named like a wait
/// is not a real wait, and a real `std::thread::sleep` still is.**
///
/// The fixture declares `CardLoop::sleep_until`, which advances a model clock
/// the way `focus_thumb`'s card loop does, and two tests: one that calls it and
/// one that sleeps. The scan must find only the second, and the fixture's own
/// `sleep_until` must not be a declared trigger.
///
/// MUTATION: put `"sleep_until"` back into `REAL_WAITS` and the controlled
/// test is found.
#[test]
fn a_controlled_sleep_until_is_not_a_real_wait_and_a_real_sleep_is() {
    let index = fixture();
    let mut found = BTreeMap::new();
    let mut refusals = Vec::new();
    scan_index(&index, "timing", &mut found, &mut refusals);
    let names: Vec<&str> = found.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        ["timing:lib:timing a_test_that_really_sleeps"],
        "only the real sleep is timing-bound"
    );
    assert_eq!(refusals, Vec::<String>::new());
    assert_eq!(declared_triggers(&index), Vec::<String>::new());
}

/// RED (T-TIMING-BOUND-STRAYS) — **clock arithmetic between two readings is
/// timing-bound, one reading and its offsets is not, and a clock inside macro
/// arguments this reading cannot parse is refused by file and line.**
///
/// MUTATIONS: in `reading::Walk::two_readings`, answer `true` when either side
/// holds a reading — `one_origin_and_its_offsets` is found; drop the
/// `duration_since` family from `reading::DISTANCES` — `a_reading_inside_an_assertion`
/// is not; parse no macro arguments — it is not either, and the refusal is gone.
#[test]
fn clock_arithmetic_is_two_readings_and_a_clock_in_an_unparsed_macro_is_refused() {
    let index = fixture_named("timing_strays", "strays");
    let mut found = BTreeMap::new();
    let mut refusals = Vec::new();
    scan_index(&index, "strays", &mut found, &mut refusals);
    let arithmetic: Vec<&str> = found
        .iter()
        .filter(|(_, test)| test.arithmetic)
        .map(|(identity, _)| identity.as_str())
        .collect();
    assert_eq!(
        arithmetic,
        [
            "strays:lib:strays a_measurement_by_subtraction",
            "strays:lib:strays a_poll_on_a_second_reading",
            "strays:lib:strays a_reading_inside_an_assertion",
        ],
        "only two readings compared or subtracted are clock arithmetic"
    );
    assert_eq!(refusals.len(), 1, "{refusals:#?}");
    assert!(
        refusals[0].starts_with("crates/bt-source/tests/fixtures/timing_strays/lib.rs:49: ")
            && refusals[0].contains("`late_by!`"),
        "the refusal names the macro by file and line: {refusals:#?}"
    );
}

/// RED (T-FIXED-MILLISECOND-KERNEL-WAITS-IN-BT-PLATFORM, plan row 163) — **a
/// kernel wait in a test's own body is a real wait, whatever its bound and
/// however its name is reached; a first-party `poll` is not; a wait only in a
/// helper is outside shape 1; each spelling that would hide a wait is refused by
/// file and line; and a function declared by a kernel wait's name is a declared
/// trigger.**
///
/// The fixture's shapes: F1 an imported bare `WaitForSingleObject` with a
/// literal bound, F2 a whole-path `WaitForMultipleObjects` on a constant, F3 the
/// same wait on `INFINITE` and on `0`, F4 `set_read_timeout` and
/// `TcpStream::connect_timeout`, F5 `libc::poll` in a closure — each found; F6 a
/// first-party `poll` called bare and as a method, F8 a wait only in a helper —
/// neither found; F7 `use libc::poll;` and a renamed `WaitForMultipleObjectsEx`
/// — refused; F9 a function named `WaitNamedPipeW` — a declared trigger; F10
/// `::libc::poll` and a longer path ending in `libc::poll`, F11 the rest of the
/// closed list (`WaitForInputIdle`, `WaitForDebugEvent`, `WSAPoll`, Winsock's
/// `select` by path, `CFRunLoop::run_in_mode`, `CFRunLoopRunInMode`) — found;
/// F12 a `libc::kevent` struct — neither found nor refused; F13 a libc wait and
/// a kernel wait taken as values, a group of `libc`, a rename of `libc`, an
/// import of `libc::kevent` and of Winsock's `select`, and F14 a glob of `libc`
/// on the file's last line with no newline — each refused.
///
/// MUTATION (the removal check, run here): scan with no kernel waits and every
/// one of F1–F5 leaves the found set, and F7's refusals with them; match the
/// `libc` waits by their last name instead of their path and F6's first-party
/// `poll`s are found.
#[test]
fn a_kernel_wait_in_a_test_body_is_a_real_wait_and_a_hidden_one_is_refused() {
    let index = fixture_named("timing_kernel_waits", "kernel");
    let mut found = BTreeMap::new();
    let mut refusals = Vec::new();
    scan_index(&index, "kernel", &mut found, &mut refusals);
    let waits: Vec<(&str, Vec<&str>)> = found
        .iter()
        .map(|(identity, test)| (identity.as_str(), test.waits.iter().copied().collect()))
        .collect();
    assert_eq!(
        waits,
        [
            (
                "kernel:lib:kernel a_connect_with_a_timeout",
                vec!["connect_timeout"]
            ),
            (
                "kernel:lib:kernel a_libc_poll_in_a_closure",
                vec!["libc::poll"]
            ),
            ("kernel:lib:kernel a_longer_libc_path", vec!["libc::poll"]),
            (
                "kernel:lib:kernel a_look_at_once",
                vec!["WaitForSingleObject"]
            ),
            ("kernel:lib:kernel a_rooted_libc_path", vec!["libc::poll"]),
            (
                "kernel:lib:kernel a_socket_read_timeout",
                vec!["set_read_timeout"]
            ),
            (
                "kernel:lib:kernel a_whole_path_wait_on_a_constant",
                vec!["WaitForMultipleObjects"]
            ),
            (
                "kernel:lib:kernel an_imported_bare_object_wait",
                vec!["WaitForSingleObject"]
            ),
            (
                "kernel:lib:kernel an_unbounded_wait",
                vec!["WaitForSingleObject"]
            ),
            (
                "kernel:lib:kernel the_other_kernel_waits",
                vec![
                    "CFRunLoopRunInMode",
                    "WSAPoll",
                    "WaitForDebugEvent",
                    "WaitForInputIdle",
                    "WinSock::select",
                    "run_in_mode"
                ]
            ),
        ],
        "F1-F5, F10 and F11 are real waits; F6's first-party polls, F8's helper, \
         F12's struct and F13's values are not"
    );
    let file = "crates/bt-source/tests/fixtures/timing_kernel_waits/lib.rs";
    assert_eq!(refusals.len(), 9, "{refusals:#?}");
    assert!(
        refusals[0].starts_with(&format!("{file}:85: "))
            && refusals[0].contains("`libc::poll` is named and not called"),
        "F7: the libc import is refused by file and line: {refusals:#?}"
    );
    assert!(
        refusals[1].starts_with(&format!("{file}:86: "))
            && refusals[1].contains("`WaitForMultipleObjectsEx` is renamed"),
        "F7: the rename is refused by file and line: {refusals:#?}"
    );
    let rest = [
        (138, "`libc::poll` is named and not called"),
        (143, "`WaitForSingleObject` is taken as a value"),
        (147, "a group or a glob of `libc`"),
        (151, "`libc` is renamed"),
        (155, "`libc::kevent` is named and not called"),
        (159, "`WinSock::select` is named and not called"),
        (163, "a group or a glob of `libc`"),
    ];
    for ((line, why), refusal) in rest.iter().zip(&refusals[2..]) {
        assert!(
            refusal.starts_with(&format!("{file}:{line}: ")) && refusal.contains(why),
            "F13/F14: {why} at line {line}: {refusals:#?}"
        );
    }
    let declared = declared_triggers(&index);
    assert!(
        declared.len() == 1 && declared[0].contains("WaitNamedPipeW"),
        "F9: a function named like a kernel wait is a declared trigger: {declared:#?}"
    );

    let mut unwidened = BTreeMap::new();
    let mut unrefused = Vec::new();
    let none = Kernel {
        calls: &[],
        paths: &[],
        typed: &[],
    };
    scan_index_reading(&index, "kernel", &none, &mut unwidened, &mut unrefused);
    assert_eq!(
        unwidened.keys().collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "the removal check: with no kernel waits the fixture holds no timing-bound test"
    );
    assert_eq!(unrefused, Vec::<String>::new(), "and nothing is refused");
}

/// RED (T-TIMING-BOUND-STRAYS) — **a test that reaches a test-only helper that
/// measures and hands the measurement back is timing-bound, one helper down or
/// two; a helper whose readings only answer "too long?", or that waits, is a
/// waiting helper (plan row 208), not a measuring one; an unresolved call whose
/// candidates measure is refused by file and line.**
///
/// Two untyped receivers deep (`probe.go()` whose `go` calls `ticker.tick()`),
/// the outer call is refused because whether its candidate reaches a
/// measurement cannot be told; an unresolved path naming a helper that only
/// reaches a measurement (`crate::support::relay()`, `relay` calling `costs`) is
/// refused as well (T-TIMING-BOUND-STRAYS review F1, F2).
///
/// MUTATIONS: in `reading::Walk::spend`, count a consumed measurement as kept —
/// `a_stall_timer_in_a_helper` is found measuring; in `Helpers::walk`, stop at the first
/// test-only callable instead of following it — `a_budget_measured_two_helpers_down`
/// is not found; skip the `Ambiguous` arm — the `cost` refusal is gone; in
/// `Helpers::reach`, answer `No` where it answers `CannotTell` — the `go`
/// refusal is gone; in the `Path` arm, ask `measures` instead of `reach` — the
/// `relay` refusal is gone.
#[test]
fn a_budget_measured_in_a_helper_is_found_through_the_call_graph() {
    let index = fixture_named("timing_strays", "strays");
    let held = vec![("strays".to_owned(), Arc::new(index))];
    let mut found = BTreeMap::new();
    let mut refusals = Vec::new();
    scan_helpers(&held, &BTreeSet::new(), &mut found, &mut refusals);
    let reached: Vec<(String, Vec<String>)> = found
        .iter()
        .filter(|(_, test)| !test.helpers.is_empty())
        .map(|(identity, test)| (identity.clone(), test.helpers.iter().cloned().collect()))
        .collect();
    assert_eq!(
        reached,
        [
            (
                "strays:lib:strays tests::a_budget_measured_in_a_helper".to_owned(),
                vec!["costs".to_owned()]
            ),
            (
                "strays:lib:strays tests::a_budget_measured_two_helpers_down".to_owned(),
                vec!["costs".to_owned()]
            ),
        ],
        "the two budgets are found, through one helper and through two"
    );
    let waiting: Vec<(&str, Vec<&str>)> = found
        .iter()
        .filter(|(_, test)| !test.waiting.is_empty())
        .map(|(identity, test)| {
            (
                identity.as_str(),
                test.waiting.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        waiting,
        [
            (
                "strays:lib:strays tests::a_remaining_patience_handed_to_a_wait",
                vec!["waits_out"]
            ),
            (
                "strays:lib:strays tests::a_stall_timer_in_a_helper",
                vec!["stalls"]
            ),
        ],
        "a stall timer and a wait in a helper are waiting helpers (plan row 208)"
    );
    assert_eq!(refusals.len(), 3, "{refusals:#?}");
    let refused = |test_site: &str, says: &[&str]| {
        assert!(
            refusals.iter().any(|refusal| {
                refusal.starts_with("crates/bt-source/tests/fixtures/timing_strays/lib.rs:")
                    && refusal.contains(test_site)
                    && says.iter().all(|said| refusal.contains(said))
            }),
            "{test_site}: {says:?} in {refusals:#?}"
        );
    };
    refused("`cost`", &["a candidate measures", "Measured::cost"]);
    refused(
        "`go`",
        &["cannot be told", "Probe::go", "`tick`", "Ticker::tick"],
    );
    refused(
        "`support::relay`",
        &[
            "names nothing the call graph places",
            "a candidate measures",
            "relay",
        ],
    );
}

/// RED (T-TIMING-GUARD-FOLLOWS-WAITING-HELPERS, plan row 208) — **a test that waits through a
/// test-only helper is timing-bound however the wait is spelled — a std wait, a kernel wait, a
/// `libc` wait in a closure, a stall timer, a zero bound, one helper down or two; a helper that is
/// a row of the list is a door, where the walk stops, and is found as a door; a product wait ends
/// the walk; a drop that waits and is no door, an untyped receiver whose candidate waits and a
/// macro that waits are refused by file and line; a door met only as an untyped receiver's
/// candidate answers that call and is found, but does not hide a waiting candidate beside it;
/// a drop that waits through a helper is refused as one that waits itself; an `elapsed` a
/// helper only prints is no clock; a door that waits on nothing is not one.**
///
/// The list's own rows are the doors: here `tests::door_wait`, `tests::Cleans::drop`,
/// `tests::Patient::bide`, `tests::Calm::settle_in` and `tests::quiet` (which waits on nothing,
/// so it is no door and its row would be stale).
///
/// MUTATIONS: in `Helpers::clock_in_body`, answer `Nothing` for a body whose only clock is a
/// wait or an `elapsed` it does not keep — every waiting test leaves the found set; in
/// `Helpers::walk`, do not stop at a door — `a_wait_through_a_door` and
/// `a_wait_through_a_helper_that_calls_a_door`
/// are found; skip the implicit impls — the `Ends::drop` refusal is gone and `Cleans::drop` is
/// no longer found as a door; in `names_a_clock_or_a_wait`, read only `elapsed` and `Instant` —
/// the `settle!` refusal is gone; in `Helpers::reach`, do not record a door met as a candidate —
/// `Patient::bide` is no longer found; in `scan_helpers`' implicit loop, ask
/// `Helpers::clock_in_body` instead of `Helpers::reach` — the `Settles::drop` refusal is gone
/// (row 20 review N1); in `Helpers::refuse_candidates`, answer `None` once a candidate is a
/// door — the `Restless::settle_in` refusal is gone (N9); in `Helpers::clock_in_body`, count an
/// only-printed `elapsed` — `a_printed_elapsed_in_a_helper_is_no_clock` is found (N2).
#[test]
fn a_wait_in_a_helper_is_found_through_the_call_graph() {
    let index = fixture_named("timing_waiting_helpers", "waiting");
    let held = vec![("waiting".to_owned(), Arc::new(index))];
    let doors: BTreeSet<String> = [
        "tests::door_wait",
        "tests::Cleans::drop",
        "tests::Patient::bide",
        "tests::Calm::settle_in",
        "tests::quiet",
    ]
    .iter()
    .map(|door| format!("waiting:lib:waiting {door}"))
    .collect();
    let mut found = BTreeMap::new();
    let mut refusals = Vec::new();
    scan_helpers(&held, &doors, &mut found, &mut refusals);
    let read: Vec<(&str, Vec<&str>, Vec<&str>, bool)> = found
        .iter()
        .map(|(identity, test)| {
            (
                identity
                    .strip_prefix("waiting:lib:waiting ")
                    .unwrap_or(identity),
                test.waiting.iter().map(String::as_str).collect(),
                test.helpers.iter().map(String::as_str).collect(),
                test.door.is_some(),
            )
        })
        .collect();
    let none = Vec::<&str>::new;
    assert_eq!(
        read,
        [
            ("tests::Calm::settle_in", none(), none(), true),
            ("tests::Cleans::drop", none(), none(), true),
            ("tests::Patient::bide", none(), none(), true),
            (
                "tests::a_budget_in_a_helper_is_still_a_measurement",
                none(),
                vec!["costs"],
                false
            ),
            (
                "tests::a_kernel_wait_in_a_helper",
                vec!["Child::gone"],
                none(),
                false
            ),
            (
                "tests::a_libc_wait_in_a_helpers_closure",
                vec!["polls"],
                none(),
                false
            ),
            (
                "tests::a_stall_timer_in_a_helper",
                vec!["quiet_for_a_second"],
                none(),
                false
            ),
            (
                "tests::a_wait_in_a_helper",
                vec!["receive_within"],
                none(),
                false
            ),
            (
                "tests::a_wait_two_helpers_down",
                vec!["pauses"],
                none(),
                false
            ),
            (
                "tests::a_zero_bound_wait_in_a_helper",
                vec!["looks"],
                none(),
                false
            ),
            ("tests::door_wait", none(), none(), true),
        ],
        "H1-H5 and H12 wait through a helper; the door, the drop behind a door and the door met as a candidate are doors; \
         the tests through the door, the product wait, the printed elapsed and the control are not \
         found"
    );
    assert_eq!(refusals.len(), 5, "{refusals:#?}");
    let file = "crates/bt-source/tests/fixtures/timing_waiting_helpers/lib.rs:";
    for says in [
        &["`Ends::drop`", "implicit call", "make it a door row"][..],
        &[
            "a call to `hold_on`",
            "a candidate measures real time or waits on it",
            "Waiter::hold_on",
        ],
        &["the macro `settle!`", "names a clock or a wait"],
        &[
            "`Settles::drop`",
            "implicit call",
            "reaches a test-only helper that does",
        ],
        &[
            "a call to `settle_in`",
            "a candidate measures real time or waits on it",
            "Restless::settle_in",
        ],
    ] {
        assert!(
            refusals.iter().any(|refusal| refusal.starts_with(file)
                && says.iter().all(|said| refusal.contains(said))),
            "{says:?} in {refusals:#?}"
        );
    }
    assert!(
        refusals
            .iter()
            .all(|refusal| !refusal.contains("Calm::settle_in")),
        "the door candidate answers no beside the waiter it does not hide: {refusals:#?}"
    );
}

/// RED (T-GATES-047 round 4) — **nothing in the workspace declares a function
/// or method by a trigger's name**, so every call the scan matches is the
/// standard library's.
///
/// The list test asserts the same thing over the same indices before it
/// compares; this one says it on its own, fixture-free.
///
/// MUTATION: put `"sleep_until"` back into `REAL_WAITS` and this names
/// `focus_thumb`'s `CardLoop::sleep_until`.
#[test]
fn no_workspace_function_is_named_like_a_real_wait() {
    let misnamed: Vec<String> = workspace_indices()
        .iter()
        .flat_map(|(_, index)| declared_triggers(index))
        .collect();
    assert!(misnamed.is_empty(), "{misnamed:#?}");
}

// ── a product function handed `now` that reads the clock again ─────────────

/// **The product functions handed a clock that read the real clock again on
/// purpose**, each with how many such readings it holds and why
/// (T-TIMING-BOUND-STRAYS; the clock-typed rows since plan row 205).
const READ_AGAIN: [(&str, usize, &str); 2] = [
    (
        "bt-app crate::FolioApp::settle_quit",
        1,
        "the Retire arm reads the clock after `pty_door::wait_for_retirements`, a real wait in \
         the same pass of up to `pane_retirement_deadline`, and the page teardown's bound is \
         counted from when that wait ended",
    ),
    (
        "bt-app crate::update_apply::take_the_window_within_using",
        3,
        "only the election lock's wait is read on the handed `LockClock`; `until`, `read_until` \
         and `write_until` stay on the real clock by row 137's ruling R5 - their retries are \
         spent only when `wait` answers true, and the counted-clock racers hand a `wait` that \
         never does",
    ),
];

/// **The first-party clock traits** of one set of indices: each trait one of
/// whose methods answers an instant from its receiver alone, and each trait
/// with such a supertrait, transitively; with what cannot be read.
fn clock_traits(
    indices: &[(&str, &Index)],
    resolver: &TraitResolver<'_>,
) -> (BTreeMap<TraitId, reading::Kinds>, Vec<String>) {
    let mut clocks: BTreeMap<TraitId, reading::Kinds> = BTreeMap::new();
    let mut every = BTreeSet::new();
    let mut refusals = Vec::new();
    for (package, index) in indices {
        for declaration in index.traits() {
            let identities = resolver.identities(package, declaration);
            match syn::parse_str::<syn::ItemTrait>(index.text(declaration.whole())) {
                Ok(declared) => {
                    let kinds = reading::clock_trait(&declared);
                    if !kinds.is_empty() {
                        for id in &identities {
                            clocks.insert(id.clone(), kinds);
                        }
                    }
                }
                Err(error) => refusals.push(format!(
                    "{}: the trait `{}` does not parse ({error})",
                    place(index, declaration.whole().start()),
                    declaration.name()
                )),
            }
            every.extend(identities);
        }
    }
    loop {
        let mut grew = false;
        for id in &every {
            let supertraits = resolver.supertraits_checked(id).unwrap_or_else(|cutoff| {
                refusals.push(format!(
                    "the trait {id}: its supertraits chain past the trait resolver's hop limit"
                ));
                cutoff.0
            });
            let inherited = supertraits
                .iter()
                .filter_map(|supertrait| clocks.get(supertrait))
                .fold(reading::Kinds::default(), |kinds, more| kinds.union(*more));
            if inherited.is_empty() {
                continue;
            }
            let known = clocks.get(id).copied().unwrap_or_default();
            if known.union(inherited) != known {
                clocks.insert(id.clone(), known.union(inherited));
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    refusals.sort();
    refusals.dedup();
    (clocks, refusals)
}

/// A first-party data type or type alias: its package's position in the
/// indices, its module path, its name.
type TypeKey = (usize, String, String);

/// **Which first-party data type or type alias a type path names**, read from
/// the indices' module records, their data types, their type aliases and their
/// `use` items — the type counterpart of [`TraitResolver`]: `crate::`,
/// `self::`, `super::`, another package's crate name, a child module, a name a
/// `use` brings in (renamed or through a glob), and a `pub use` re-export, each
/// module-and-name lookup made once. A `use` inside a function body is not
/// read. A path whose leading segment names nothing first-party — no module,
/// no package, no name a `use` brings in, not `crate`, `self` or `super` —
/// names, by the language's rules, an outside crate's type or an associated
/// type (`wgpu::Instance`, `C::Road`, `Self::Road`), and resolution says so.
struct TypeResolver {
    crates: BTreeMap<String, usize>,
    modules: BTreeSet<(usize, String)>,
    declared: BTreeSet<TypeKey>,
    names: BTreeMap<ModuleKey, Vec<UsedName>>,
    globs: BTreeMap<ModuleKey, Vec<Vec<String>>>,
}

/// A module of one package: the package's place in the indices, and the
/// module's path.
type ModuleKey = (usize, String);

/// A name a `use` brings into a module, and the path it names.
type UsedName = (String, Vec<String>);

/// How many module-and-name lookups one type path's resolution makes before it
/// stops: each lookup is made once (a `use` cycle ends there), and this bounds
/// the walk.
const TYPE_SEEN: usize = 4096;

impl TypeResolver {
    fn new(indices: &[(&str, &Index)]) -> Self {
        let mut resolver = Self {
            crates: BTreeMap::new(),
            modules: BTreeSet::new(),
            declared: BTreeSet::new(),
            names: BTreeMap::new(),
            globs: BTreeMap::new(),
        };
        for (at, (package, index)) in indices.iter().enumerate() {
            resolver.crates.insert(package.replace('-', "_"), at);
            for module in index.modules() {
                for path in module.module_paths() {
                    resolver.modules.insert((at, path.clone()));
                }
            }
            for record in index
                .items()
                .iter()
                .filter(|record| record.kind().is_type())
            {
                for module in record.module_paths() {
                    resolver
                        .declared
                        .insert((at, module.to_owned(), record.name().to_owned()));
                }
            }
            for alias in index.aliases() {
                for module in alias.module_paths() {
                    resolver
                        .declared
                        .insert((at, module.to_owned(), alias.name().to_owned()));
                }
            }
            for used in index.uses() {
                for module in used.module_paths() {
                    let key = (at, module.to_owned());
                    resolver
                        .names
                        .entry(key.clone())
                        .or_default()
                        .extend(used.names().iter().cloned());
                    resolver
                        .globs
                        .entry(key)
                        .or_default()
                        .extend(used.globs().iter().cloned());
                }
            }
        }
        resolver
    }

    fn parent(module: &str) -> String {
        module
            .rsplit_once("::")
            .map_or_else(|| module.to_owned(), |(parent, _)| parent.to_owned())
    }

    /// The modules a leading path segment names, written in `module`.
    fn modules_named(
        &self,
        at: usize,
        module: &str,
        segment: &str,
        seen: &mut BTreeSet<(usize, String, Vec<String>)>,
    ) -> Vec<(usize, String)> {
        match segment {
            "crate" => return vec![(at, "crate".to_owned())],
            "self" => return vec![(at, module.to_owned())],
            "super" => return vec![(at, Self::parent(module))],
            _ => {}
        }
        let mut found = Vec::new();
        let child = format!("{module}::{segment}");
        if self.modules.contains(&(at, child.clone())) {
            found.push((at, child));
        }
        if let Some(other) = self.crates.get(segment) {
            found.push((*other, "crate".to_owned()));
        }
        if !seen.insert((at, module.to_owned(), vec![segment.to_owned()])) || seen.len() > TYPE_SEEN
        {
            return found;
        }
        let key = (at, module.to_owned());
        for (name, path) in self.names.get(&key).into_iter().flatten() {
            if name == segment {
                found.extend(self.module_path(at, module, path, seen));
            }
        }
        for glob in self.globs.get(&key).into_iter().flatten() {
            let mut path = glob.clone();
            path.push(segment.to_owned());
            found.extend(self.module_path(at, module, &path, seen));
        }
        found
    }

    /// The modules a whole path names, written in `module`.
    fn module_path(
        &self,
        at: usize,
        module: &str,
        path: &[String],
        seen: &mut BTreeSet<(usize, String, Vec<String>)>,
    ) -> Vec<(usize, String)> {
        let Some((first, rest)) = path.split_first() else {
            return Vec::new();
        };
        let mut current = self.modules_named(at, module, first, seen);
        for segment in rest {
            let mut next = Vec::new();
            for (at, module) in &current {
                next.extend(self.modules_named(*at, module, segment, seen));
            }
            current = next;
        }
        current.sort();
        current.dedup();
        current
    }

    /// The first-party data types and type aliases a type path names, written
    /// in `module` of the package at `at`, and whether some way of reading it
    /// leads outside: to a path whose leading segment names nothing
    /// first-party.
    fn resolve(&self, at: usize, module: &str, path: &[String]) -> (BTreeSet<TypeKey>, bool) {
        let mut outside = false;
        let found = self.resolve_seen(at, module, path, &mut BTreeSet::new(), &mut outside);
        (found, outside)
    }

    fn resolve_seen(
        &self,
        at: usize,
        module: &str,
        path: &[String],
        seen: &mut BTreeSet<(usize, String, Vec<String>)>,
        outside: &mut bool,
    ) -> BTreeSet<TypeKey> {
        let mut found = BTreeSet::new();
        let Some((name, prefix)) = path.split_last() else {
            return found;
        };
        let homes = if let Some(leading) = prefix.first() {
            if self
                .modules_named(at, module, leading, &mut BTreeSet::new())
                .is_empty()
            {
                *outside = true;
                return found;
            }
            self.module_path(at, module, prefix, seen)
        } else {
            vec![(at, module.to_owned())]
        };
        for (home_at, home) in homes {
            let key = (home_at, home.clone(), name.clone());
            if self.declared.contains(&key) {
                found.insert(key);
                continue;
            }
            let mut tag = vec![String::new()];
            tag.push(name.clone());
            if !seen.insert((home_at, home.clone(), tag)) || seen.len() > TYPE_SEEN {
                continue;
            }
            let at_home = (home_at, home.clone());
            for (alias, target) in self.names.get(&at_home).into_iter().flatten() {
                if alias == name {
                    found.extend(self.resolve_seen(home_at, &home, target, seen, outside));
                }
            }
            for glob in self.globs.get(&at_home).into_iter().flatten() {
                let mut target = glob.clone();
                target.push(name.clone());
                found.extend(self.resolve_seen(home_at, &home, &target, seen, outside));
            }
        }
        found
    }
}

/// How a path, written in `modules` of the package at `at`, reads to the
/// handed-now reading: as a bound (a clock trait, or unread when the trait
/// resolver gives up or cannot follow a clock trait's name) and as a type (a
/// type that carries a clock, or unread when the type resolver cannot follow
/// a path whose last segment names one; a path that leads outside, to an
/// outside crate's type or an associated type, carries none).
struct Reader<'r, 'i> {
    resolver: &'r TraitResolver<'i>,
    clocks: &'r BTreeMap<TraitId, reading::Kinds>,
    types: &'r TypeResolver,
    owners: &'r BTreeMap<TypeKey, reading::Kinds>,
    owner_names: &'r BTreeSet<String>,
    package: &'r str,
    at: usize,
    modules: Vec<String>,
}

impl Reader<'_, '_> {
    fn segments(path: &syn::Path) -> Vec<String> {
        path.segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect()
    }

    fn bound(&self, path: &syn::Path) -> reading::Bound {
        let segments = Self::segments(path);
        let mut named = false;
        for module in &self.modules {
            match self
                .resolver
                .resolve_checked(self.package, module, &segments)
            {
                Ok(found) => {
                    let kinds = found
                        .iter()
                        .filter_map(|id| self.clocks.get(id))
                        .fold(reading::Kinds::default(), |kinds, more| kinds.union(*more));
                    if !kinds.is_empty() {
                        return reading::Bound::Clock("trait", kinds);
                    }
                    named |= !found.is_empty();
                }
                Err(_) => {
                    return reading::Bound::Unread(format!(
                        "the bound `{}` chains past the trait resolver's hop limit",
                        segments.join("::")
                    ));
                }
            }
        }
        let last = segments.last().map(String::as_str);
        if !named && self.clocks.keys().any(|id| Some(id.name.as_str()) == last) {
            return reading::Bound::Unread(format!(
                "the bound `{}` names a clock trait by a path the trait resolver cannot follow",
                segments.join("::")
            ));
        }
        reading::Bound::Not
    }

    fn carrier(&self, path: &syn::Path) -> reading::Bound {
        let segments = Self::segments(path);
        let Some(last) = segments.last() else {
            return reading::Bound::Not;
        };
        if !self.owner_names.contains(last) {
            return reading::Bound::Not;
        }
        let mut named = false;
        for module in &self.modules {
            let (found, outside) = self.types.resolve(self.at, module, &segments);
            let kinds = found
                .iter()
                .filter_map(|key| self.owners.get(key))
                .fold(reading::Kinds::default(), |kinds, more| kinds.union(*more));
            if !kinds.is_empty() {
                return reading::Bound::Clock("carrier", kinds);
            }
            named |= !found.is_empty() || outside;
        }
        if named {
            reading::Bound::Not
        } else {
            reading::Bound::Unread(format!(
                "the type `{}` is named like a type that carries a clock, by a path the type \
                 resolver cannot follow",
                segments.join("::")
            ))
        }
    }
}

/// What one `impl` or trait block hands its methods: the type parameters its
/// header bounds by a clock, and how its `self` is handed, if it is.
struct BlockClock {
    generics: reading::ClockParameters,
    self_how: Option<(&'static str, reading::Kinds)>,
}

/// What the handed-`now` reading found in one set of indices.
struct HandedSet {
    /// Per function handed a clock that reads a real clock again, the places.
    found: BTreeMap<String, Vec<String>>,
    /// What it refuses.
    refusals: Vec<String>,
    /// Every function handed a clock: how, the kinds, and how many readings
    /// again.
    handed: BTreeMap<String, (String, &'static str, usize)>,
}

/// What the handed-`now` reading finds in one set of indices: the functions
/// handed a clock, those that read a real clock again with the places, and
/// what it refuses.
fn handed_now(indices: &[(&str, &Index)]) -> HandedSet {
    let resolver = TraitResolver::new(indices);
    let (clocks, mut refusals) = clock_traits(indices, &resolver);
    let types = TypeResolver::new(indices);
    let modules_of = |record: &ItemRecord| -> Vec<String> {
        record
            .module_paths()
            .into_iter()
            .map(str::to_owned)
            .collect()
    };
    // The product data types, parsed once.
    let mut data = Vec::new();
    for (at, (_, index)) in indices.iter().enumerate() {
        for record in index
            .items()
            .iter()
            .filter(|record| record.kind().is_type())
        {
            if test_only(index, record) {
                continue;
            }
            match reading::data_type(index.text(record.whole())) {
                Ok(declared) => data.push((at, record, declared)),
                Err(error) => refusals.push(format!(
                    "{}: the text of `{}` does not parse ({error})",
                    place(index, record.whole().start()),
                    record.name()
                )),
            }
        }
    }
    // The types that carry a clock, to a fixed point: a field of a type that
    // carries one makes its holder carry one too.
    let mut owners: BTreeMap<TypeKey, reading::Kinds> = BTreeMap::new();
    let mut owner_names: BTreeSet<String> = BTreeSet::new();
    let mut last_pass = false;
    loop {
        let mut grew = false;
        for (at, record, declared) in &data {
            let key_of = |module: &str| (*at, module.to_owned(), record.name().to_owned());
            let reader = Reader {
                resolver: &resolver,
                clocks: &clocks,
                types: &types,
                owners: &owners,
                owner_names: &owner_names,
                package: indices[*at].0,
                at: *at,
                modules: modules_of(record),
            };
            let bound = |path: &syn::Path| reader.bound(path);
            let carrier = |path: &syn::Path| reader.carrier(path);
            let names = reading::Names {
                bound: &bound,
                carrier: &carrier,
            };
            let (carries, unread) = reading::carries_a_clock(declared, names);
            if last_pass {
                let index = indices[*at].1;
                let base = record.whole().start();
                refusals.extend(
                    unread
                        .iter()
                        .map(|(offset, why)| format!("{}: {why}", place(index, base + offset))),
                );
            } else if !carries.is_empty() {
                for module in record.module_paths() {
                    let known = owners.get(&key_of(module)).copied().unwrap_or_default();
                    if known.union(carries) != known {
                        owners.insert(key_of(module), known.union(carries));
                        grew = true;
                    }
                }
                owner_names.insert(record.name().to_owned());
            }
        }
        if last_pass {
            break;
        }
        last_pass = !grew;
        // A name a `use` brings in under another name, for a type carrying a
        // clock, is a carrier's name too (`use deep::Held as Kept;`).
        loop {
            let renamed: Vec<String> = indices
                .iter()
                .flat_map(|(_, index)| index.uses())
                .flat_map(|used| used.names())
                .filter(|(name, path)| {
                    !owner_names.contains(name)
                        && path.last().is_some_and(|last| owner_names.contains(last))
                })
                .map(|(name, _)| name.clone())
                .collect();
            if renamed.is_empty() {
                break;
            }
            owner_names.extend(renamed);
        }
    }
    let mut found = BTreeMap::new();
    let mut handed = BTreeMap::new();
    for (at, (package, index)) in indices.iter().enumerate() {
        let package: &str = package;
        let reader_for = |modules: Vec<String>| Reader {
            resolver: &resolver,
            clocks: &clocks,
            types: &types,
            owners: &owners,
            owner_names: &owner_names,
            package,
            at,
            modules,
        };
        let mut blocks: Vec<(usize, usize, usize, bool)> = index
            .impls()
            .iter()
            .enumerate()
            .map(|(n, block)| (block.whole().start(), block.whole().end(), n, true))
            .chain(index.traits().iter().enumerate().map(|(n, declaration)| {
                (
                    declaration.whole().start(),
                    declaration.whole().end(),
                    n,
                    false,
                )
            }))
            .collect();
        blocks.sort_unstable();
        let mut read_blocks: BTreeMap<(usize, bool), BlockClock> = BTreeMap::new();
        for record in index.items() {
            if test_only(index, record) {
                continue;
            }
            if record.kind() == ItemKind::Field && record.name() == "now" {
                if reading::instant_field(index.text(record.whole())) {
                    refusals.push(format!(
                        "{}: a product type carries `now` of a type holding an instant ({}); a \
                         function handed it could read the clock again without a parameter \
                         named `now`",
                        place(index, record.whole().start()),
                        record.type_owner().unwrap_or("?")
                    ));
                }
                continue;
            }
            if !record.kind().is_callable() || record.body().is_none() {
                continue;
            }
            let text = index.text(record.whole());
            let parsed = reading::callable(
                text,
                record.kind() == ItemKind::Function,
                record.type_owner().is_some(),
            );
            let (signature, Some(block)) = (match parsed {
                Ok(parsed) => parsed,
                Err(error) => {
                    refusals.push(format!(
                        "{}: the text of `{}` does not parse ({error})",
                        place(index, record.whole().start()),
                        record.name()
                    ));
                    continue;
                }
            }) else {
                continue;
            };
            // The innermost `impl` or trait block a method is written in, read once.
            let whole = record.whole();
            let enclosing = (record.kind() == ItemKind::AssociatedFunction)
                .then(|| {
                    let before = blocks.partition_point(|(start, ..)| *start <= whole.start());
                    blocks[..before]
                        .iter()
                        .rev()
                        .find(|(_, end, ..)| *end >= whole.end())
                        .map(|(_, _, n, is_impl)| (*n, *is_impl))
                })
                .flatten();
            let (generics, self_how) = match enclosing {
                None => (reading::ClockParameters::new(), None),
                Some(key) => {
                    let read = read_blocks.entry(key).or_insert_with(|| {
                        let (n, is_impl) = key;
                        let (whole, body, modules, self_how) = if is_impl {
                            let block = &index.impls()[n];
                            let modules: Vec<String> = block
                                .module_paths()
                                .into_iter()
                                .map(str::to_owned)
                                .collect();
                            let owner = block.type_owner();
                            let carried = modules
                                .iter()
                                .filter_map(|module| {
                                    owners.get(&(at, module.clone(), owner.to_owned()))
                                })
                                .fold(reading::Kinds::default(), |kinds, more| kinds.union(*more));
                            if owner_names.contains(owner)
                                && !modules.iter().any(|module| {
                                    types
                                        .declared
                                        .contains(&(at, module.clone(), owner.to_owned()))
                                })
                            {
                                refusals.push(format!(
                                    "{}: an `impl` of `{owner}`, a name that carries a clock, is \
                                     written away from its type, so its methods cannot be \
                                     matched to the clock",
                                    place(index, block.whole().start())
                                ));
                            }
                            (
                                block.whole(),
                                block.body(),
                                modules,
                                (!carried.is_empty()).then_some(("self field", carried)),
                            )
                        } else {
                            let declaration = &index.traits()[n];
                            let modules: Vec<String> = declaration
                                .module_paths()
                                .into_iter()
                                .map(str::to_owned)
                                .collect();
                            let clock = resolver
                                .identities(package, declaration)
                                .iter()
                                .filter_map(|id| clocks.get(id))
                                .fold(reading::Kinds::default(), |kinds, more| kinds.union(*more));
                            (
                                declaration.whole(),
                                declaration.body(),
                                modules,
                                (!clock.is_empty()).then_some(("trait default", clock)),
                            )
                        };
                        let header = &index.text(whole)[..body.start() - whole.start()];
                        let reader = reader_for(modules);
                        let bound = |path: &syn::Path| reader.bound(path);
                        let carrier = |path: &syn::Path| reader.carrier(path);
                        let names = reading::Names {
                            bound: &bound,
                            carrier: &carrier,
                        };
                        let mut generics = match reading::header_generics(header, names) {
                            Ok((generics, unread)) => {
                                let base = whole.start();
                                refusals.extend(unread.iter().map(|(offset, why)| {
                                    format!("{}: {why}", place(index, base + offset))
                                }));
                                generics
                            }
                            Err(error) => {
                                refusals.push(format!(
                                    "{}: the header of a block does not parse ({error})",
                                    place(index, whole.start())
                                ));
                                reading::ClockParameters::new()
                            }
                        };
                        let bounded = generics
                            .values()
                            .fold(reading::Kinds::default(), |kinds, more| kinds.union(*more));
                        let self_how = self_how.or_else(|| {
                            (is_impl && !bounded.is_empty()).then_some(("impl bound", bounded))
                        });
                        if let Some((_, kinds)) = self_how {
                            generics.insert("Self".to_owned(), kinds);
                        }
                        BlockClock { generics, self_how }
                    });
                    (read.generics.clone(), read.self_how)
                }
            };
            let reader = reader_for(modules_of(record));
            let bound = |path: &syn::Path| reader.bound(path);
            let carrier = |path: &syn::Path| reader.carrier(path);
            let scope = reading::Scope {
                names: reading::Names {
                    bound: &bound,
                    carrier: &carrier,
                },
                generics,
                self_how,
            };
            let read = reading::handed_now(&signature, &block, &scope);
            let base = record.whole().start();
            refusals.extend(
                read.refusals
                    .iter()
                    .map(|(offset, why)| format!("{}: {why}", place(index, base + offset))),
            );
            if read.how.is_empty() && read.again.is_empty() {
                continue;
            }
            let identity = record
                .identities()
                .next()
                .map(|identity| identity.to_string())
                .unwrap_or_else(|| record.name().to_owned());
            let key = format!("{package} {identity}");
            let how: Vec<&str> = if read.how.is_empty() {
                vec!["in a closure or a nested fn"]
            } else {
                read.how.iter().copied().collect()
            };
            handed.insert(
                key.clone(),
                (how.join(","), read.kinds.label(), read.again.len()),
            );
            if !read.again.is_empty() {
                found.insert(
                    key,
                    read.again
                        .iter()
                        .map(|offset| place(index, base + offset))
                        .collect(),
                );
            }
        }
    }
    refusals.sort();
    refusals.dedup();
    HandedSet {
        found,
        refusals,
        handed,
    }
}

/// Leaves the handed set where a reviewer or the next row can read it:
/// `target/timing-bound-tests/HANDED-CLOCKS.tsv`, one row per product function
/// handed a clock, with how it is handed, the kinds of clock (`instant`,
/// `wall`, `both`; `none` for a function handed none whose closure or nested
/// `fn` is) and how many readings it holds again.
fn leave_handed(handed: &BTreeMap<String, (String, &'static str, usize)>) {
    let out = workspace_root().join("target").join("timing-bound-tests");
    std::fs::create_dir_all(&out).expect("target/ is writable");
    let mut text = String::from("function\thanded_by\tclock\treadings_again\n");
    for (function, (how, kinds, again)) in handed {
        text.push_str(&format!("{function}\t{how}\t{kinds}\t{again}\n"));
    }
    std::fs::write(out.join("HANDED-CLOCKS.tsv"), text).expect("written");
}

/// RED (T-TIMING-BOUND-STRAYS; widened by plan row 205) — **no product function
/// handed a clock reads a real clock again, but for the named list; a reading
/// that is only the origin of a measurement is not one.**
///
/// A function handed the turn's `now`, or a clock to read it on, is answering
/// for that clock; a second reading of the real one in its body makes the
/// answer depend on how long the turn took before it got there, and a test
/// handing it `now` or a counted clock cannot hold that still — the class
/// behind `the_final_write_is_judged_and_the_verdict_is_what_decides`, whose
/// quit judged its write on a reading of its own three seconds of a loaded
/// fixture later. The monotonic clock and the wall clock are both in scope:
/// `Instant` and `SystemTime` read alike throughout.
///
/// # The grammar
///
/// * **Holding an instant**: a type that names `Instant` or `SystemTime`
///   anywhere — behind a reference or a pointer, in a tuple, an array, a slice
///   or any path's type arguments (`Option<Instant>`, `Box<Instant>`,
///   `(Instant, u64)`, `[SystemTime]`).
/// * **A clock trait**: a first-party trait, resolved in the writer's module
///   by [`TraitResolver`], one of whose methods takes its receiver and nothing
///   else and answers a type holding an instant (`GitClock::now`,
///   `Media::load_clock`), or one with such a supertrait, to any depth. A
///   getter of a stored instant (`web_spare::SpareSeat::next_deadline`) reads
///   the same, so its trait is a clock here too: the text cannot tell a
///   present from a stored instant, and the reading errs toward handed —
///   stricter, never silent.
/// * **A clock-carrying type**: a first-party struct, enum or union declaring
///   a field of a clock type, to a fixed point (a field of a clock-carrying
///   type carries one: `struct Outer { beat: Heartbeat }`); a type path is
///   resolved to it from the writer's module (`crate::`, `self::`, `super::`,
///   another package's crate name, child modules, `use` items renamed or by
///   glob, `pub use` re-exports; a type alias resolves as a type, its target
///   not followed). A path whose leading segment names nothing first-party is,
///   by the language's rules, an outside crate's type or an associated type
///   (`wgpu::Instance`, `C::Road`), and carries none.
/// * **A clock type**: `impl B` or `dyn B` where a bound `B` is `Fn() -> A`,
///   `FnMut() -> A` or `FnOnce() -> A` with no inputs and `A` holding an
///   instant, or a clock trait; a `fn() -> A` pointer; a clock-carrying type;
///   `Self` in a block whose `self` is handed; a type parameter bounded by a
///   clock, inline or in a `where` clause, of the function or of its `impl` or
///   trait block — each also behind `&`, `&mut`, a pointer, in any path's type
///   arguments (`Box`, `Arc`, `Option`, `Ops<C>`), a tuple, an array or a
///   slice.
/// * **Not a clock**: a closure with inputs (`Fn(u32) -> Instant` computes an
///   instant from what it is given), a trait whose instant method takes
///   inputs, a type that carries no clock (what it reads, it reads in its own
///   methods, which are read on their own).
/// * **Handed**, on a product function or method, or on a closure or a nested
///   `fn` inside one: a parameter of a clock type, **whatever its name, called
///   or only handed on**; a parameter named `now` of a type holding an
///   instant, through tuple patterns over tuple types (a deadline under
///   another name, `deadline: Instant`, is a value the function rightly
///   compares with a fresh reading); a method's `self` when its type carries a
///   clock (`Heartbeat::clock`, `update_archive::Deadline::now`), when its
///   `impl` header bounds a type parameter by a clock (`impl<M: Media>
///   Pump<M>`), or when it is a default method of a clock trait. A closure
///   keeps what its enclosing body is handed and adds the kinds of its own
///   clock-typed parameters; a closure parameter `now` of a written type that
///   holds no instant clears nothing. A nested `fn` captures nothing: it is
///   handed what its own signature hands it.
///   `target/timing-bound-tests/HANDED-CLOCKS.tsv` lists every handed product
///   function, how, the kinds and its readings again, each run.
/// * **Reading again**: in a handed body, closures included, a call of a path
///   ending `Instant::now` or `SystemTime::now`, either handed on as a function
///   value (a reading deferred), and every `.elapsed()` (it is `now()` minus its
///   receiver), whatever the receiver, but an admitted origin's (below). **A
///   reading counts by kind** (plan row 205 R8): `Instant::now()` counts in a
///   body handed an instant (a `now` holding one, or a clock answering one),
///   `SystemTime::now()` in a body handed a wall time, both in a body handed
///   both — a first reading of the other clock is no second answer for the one
///   handed. An `.elapsed()` is a reading of its receiver's kind when the
///   receiver's type is known (a binding written `Instant` or `SystemTime`, an
///   origin, a reading), and of both kinds when it is not — erring toward
///   handed.
///   **Calling the handed clock is never a reading again**: a counted clock
///   answers every call.
/// * **Admitted**: a reading bound by `let x = Instant::now();` (or
///   `SystemTime::now()`) where every use of that binding is the receiver of
///   `.elapsed()`, and those `.elapsed()` calls — one measurement of real time
///   spent, its origin and its end, not a second answer for the turn. A use is
///   matched to the binding by scope: a later name `x` that a pattern, a
///   parameter or another `let` binds is another binding. An admitted origin's own
///   `.elapsed()` is the end of that same admitted measurement, so it is never
///   a reading again; every other `.elapsed()` is.
/// * **Refused**, by file and line: a product body, data type, trait or block
///   header that does not parse; macro arguments that do not parse and name a
///   clock reading in a handed body; a reading in a closure handed a `now`
///   whose type is not written; a product type with a field `now` of a type
///   holding an instant; a parameter or field type the reading does not read
///   (`_`, verbatim tokens, or a macro whose arguments name `Instant`,
///   `SystemTime`, a `Fn` trait, a clock trait or a clock-carrying type —
///   `Token![,]` is read as no clock); a bound kept as verbatim tokens; a `now`
///   bound by a pattern it cannot pair with the type; a bound the trait
///   resolver gives up on, or that names a clock trait by a path it cannot
///   follow; a type named like a clock-carrying type by a path the type
///   resolver cannot follow; a `where` clause bounding by a clock a type that
///   is not a parameter; an `impl` of a clock-carrying name written in a module
///   that declares no such type.
///
/// # What it does not see
///
/// * A reading through a call: a helper that reads the clock, or **a concrete
///   real clock called directly** in a handed body (`WallClock.now()` or
///   `RealProbeClock.now()` where the handed `clock.now()` was meant), or one
///   held in a `static` or `const` (`static CLOCK: fn() -> Instant =
///   Instant::now;`, then `CLOCK()`) — the call graph's ground, which this scan
///   does not walk.
/// * `Instant` or `SystemTime` under another name: `use std::time::Instant as
///   I;`, `type Now = Instant;`, or any `type` alias of a clock type.
/// * An untyped closure parameter of another name than `now` (`|clock| …`).
/// * A `use` item inside a function body, which neither resolver reads: a
///   bound or a type named through it is read as no clock.
/// * An associated type (`E::Clock`, `<E as Engine>::Clock`).
/// * A method-level `where` clause on its block's own parameter (`impl<C>
///   Pump<C> { fn f(&self) where C: Clock }`): the method's `self` is not tied
///   to it.
/// * An `impl` nested inside a function body: its methods are walked with the
///   outer body's handedness, and their signatures are not read.
/// * A clock a closure captures from outside the function rather than being
///   handed it.
/// * A clock that answers anything but a type holding an instant: a trait whose
///   reading is a `u64` of milliseconds (`update_apply::TrialClock::now_ms`) is
///   outside the grammar, so `reserve_last_trial`, handed one, is not handed,
///   and the `&WallClock` it hands `Journaled::of` beside it is not seen.
///
/// MUTATION: put `let now = Instant::now();` back in `settle_quit`'s `Write`
/// arm — this names it, by count; drop the `elapsed` admission — this names
/// `warm_web_engine`; hand `self.open.missed` `|| Instant::now()` in both
/// `Engine::state_at`s (`plant-205-closure-now-read-again.py`) — this names
/// both arms' `state_at` from either machine.
#[test]
fn no_product_function_reads_again_the_clock_it_was_handed() {
    let held = workspace_indices();
    let indices: Vec<(&str, &Index)> = held
        .iter()
        .map(|(name, index)| (name.as_str(), &**index))
        .collect();
    let HandedSet {
        found,
        refusals,
        handed,
    } = handed_now(&indices);
    leave_handed(&handed);
    println!(
        "{} product functions are handed a clock (target/timing-bound-tests/HANDED-CLOCKS.tsv)",
        handed.len()
    );
    assert!(
        refusals.is_empty(),
        "the handed-now reading cannot decide these:\n  {}",
        refusals.join("\n  ")
    );
    let listed: BTreeMap<&str, usize> = READ_AGAIN
        .iter()
        .map(|(key, count, _)| (*key, *count))
        .collect();
    let mut wrong = Vec::new();
    for (key, places) in &found {
        if listed.get(key.as_str()) != Some(&places.len()) {
            wrong.push(format!(
                "  {key} reads the real clock again {} time(s) ({}) — read it on the clock it \
                 is handed, or, when a real wait stands between, list it in READ_AGAIN with why",
                places.len(),
                places.join(", ")
            ));
        }
    }
    for key in listed.keys() {
        if !found.contains_key(*key) {
            wrong.push(format!(
                "  {key} is listed in READ_AGAIN and reads nothing again"
            ));
        }
    }
    println!(
        "{} product functions handed a clock read the real one again",
        found.len()
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// RED (T-TIMING-BOUND-STRAYS) — **the handed-now reading over its fixture**:
/// the function handed `now` that reads the clock again is found, the one whose
/// reading is a measurement's origin is not, and an untyped closure `now` and a
/// type carrying `now` are refused by file and line.
///
/// MUTATIONS: drop the measurement admission in `reading::handed_now` —
/// `handed_and_measured` is found; read closures as handed nothing — the
/// closure's refusal is gone.
#[test]
fn the_handed_now_reading_finds_refuses_and_admits_what_it_says() {
    let index = fixture_named("timing_strays", "strays");
    let HandedSet {
        found, refusals, ..
    } = handed_now(&[("strays", &index)]);
    let names: Vec<&str> = found.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        [
            "strays crate::handed_and_read_again",
            "strays crate::handed_maybe_and_read_again"
        ]
    );
    let counts: Vec<usize> = found.values().map(Vec::len).collect();
    assert_eq!(counts, [1, 2], "{found:#?}");
    assert_eq!(refusals.len(), 2, "{refusals:#?}");
    assert!(
        refusals
            .iter()
            .any(|refusal| refusal.contains("lib.rs:80: ") && refusal.contains("closure")),
        "{refusals:#?}"
    );
    assert!(
        refusals
            .iter()
            .any(|refusal| refusal.contains("lib.rs:88: ") && refusal.contains("Carries")),
        "{refusals:#?}"
    );
}

/// RED (plan row 205) — **the handed-now reading reads every clock shape its
/// grammar lists**: over the `timing_clocks` fixture, a function handed a clock
/// in each shape — closures behind `impl`, `&dyn`, `&mut dyn` and `Box`, a
/// `fn()` pointer, generics bounded inline and in a `where` clause, a tuple
/// parameter, a tuple answer, a clock trait and one by its supertrait, a struct
/// parameter carrying a clock-bounded parameter, a type with a clock field, an
/// `impl` bounding its parameter by a clock — that also reads the real clock is
/// found, once, and so is one that hands `Instant::now` on as a value; and,
/// since the row's review, a clock-carrying type handed as a parameter (by
/// reference, in `Option<Box<_>>`, by a module path, through a renamed `use`,
/// as `Self`, one and two fields down, a struct bounding its own parameter, an
/// enum, a union, across the package line), every `.elapsed()` that is not an
/// admitted origin's (`forwards`' `due.elapsed()` among them), a clock trait's
/// default method and a trait two supertraits down, a `now` holding an instant
/// in a `Box`, a borrowed `Option` or a slice, the wall clock read again, a
/// clock behind a pointer, in an array or a slice, an `impl` bounding by a
/// clock in its `where` clause, and a closure or a nested `fn` handed a clock
/// inside a function handed none; since the second review, a closure `now: u64`
/// in a handed body (it clears nothing), an instant read in a wall-clock closure
/// of a body handed an instant, both kinds read in a closure handed a wall `now`
/// in a body handed an instant, `.elapsed()` of a written wall time and of a
/// receiver whose type is not written in a body handed a wall time, and an
/// `.elapsed()` on a binding an inner block's origin shares a name with; and an
/// untyped closure `now` in a body handed an instant, whose instant readings are
/// found and whose wall reading is refused. An
/// owner whose method calls only its clock, the real clock's own `now`, a
/// trait or closure with inputs, a `now: u64`, a `Token![,]`-like macro type, a
/// carrier's name led by an outside crate, through an associated type or
/// through a type alias, and a reading of the other kind than the one handed
/// (an instant handed, the wall clock read, and the other way round, through a
/// `now`, a closure, a carrier and a trait) are not; a body handed both kinds
/// is found for each; an unread type, a `now` in an unpairable pattern, a clock trait by an
/// unfollowable path, an `impl` written away from its type, a macro type naming
/// a `Fn` trait or a clock trait, a `where` clause bounding a non-parameter by
/// a clock and a carrier's name by an unfollowable path are refused. Two
/// refusals have no case here, as building one would mean a fixture past a
/// resolver's hop limit or a header the index accepts and `syn` does not: a
/// trait or type resolution cut off at its hop limit, and an `impl` or trait
/// header that does not parse.
///
/// MUTATIONS: read a clock type only on a parameter named `now` — `by_dyn_fn`
/// is gone; drop the supertrait walk — `by_supertrait` and `by_deep_supertrait`
/// are gone; drop the value-path reading — `by_value_path` and
/// `by_where_generic` are gone; drop the field owners — `Beat::late` is gone;
/// stop the owner fixed point after one pass — `Outermost::late` is gone; read
/// carriers only as `self` — `by_carrier` is gone; exempt `.elapsed()` on any
/// receiver — `Beat::by_stored_elapsed` and `forwards` are gone; let a closure
/// replace its body's kinds — `instant_body_wall_closure` is gone; let a closure
/// `now` of a non-clock type clear its body — `a_number_now_in_a_handed_body` is
/// gone; read every `.elapsed()` as monotonic — `wall_elapsed` and
/// `untyped_elapsed` are gone; match origins by name — `origin_in_an_inner_block`
/// is gone.
#[test]
fn the_handed_clock_reading_reads_every_clock_shape() {
    let index = fixture_named("timing_clocks", "clocks");
    let other = fixture_named("timing_clocks_other", "other");
    let HandedSet {
        found, refusals, ..
    } = handed_now(&[("clocks", &index), ("other", &other)]);
    let counts: BTreeMap<String, usize> = found
        .iter()
        .map(|(key, places)| (key.clone(), places.len()))
        .collect();
    let expected: BTreeMap<String, usize> = [
        "by_impl_once",
        "by_dyn_fn",
        "by_dyn_fn_mut",
        "by_boxed",
        "by_pointer",
        "by_inline_generic",
        "by_where_generic",
        "by_tuple",
        "by_tuple_answer",
        "by_trait_object",
        "by_supertrait",
        "by_value_path",
        "by_inner_closure",
        "by_carried",
        "Beat::late",
        "Pump::late",
        "forwards",
        "by_carrier",
        "by_optional_carrier",
        "Outer::late",
        "Outermost::late",
        "Beat::same",
        "Beat::by_stored_elapsed",
        "by_pathed_carrier",
        "by_used_carrier",
        "Timed::due_in",
        "by_deep_supertrait",
        "by_boxed_now",
        "by_borrowed_option_now",
        "by_now_slice",
        "by_wall_now",
        "by_wall_clock",
        "by_pointer_to_clock",
        "by_array",
        "by_slice",
        "Pumped::late",
        "by_bounded_carrier",
        "by_enum_carrier",
        "by_union_carrier",
        "by_closure_parameter",
        "by_nested_fn",
        "by_other_trait",
        "by_other_carrier",
        "by_wall_carrier",
        "by_wall_trait",
        "a_number_now_in_a_handed_body",
        "instant_body_wall_closure",
        "wall_elapsed",
        "untyped_elapsed",
        "origin_in_an_inner_block",
    ]
    .iter()
    .map(|name| (format!("clocks crate::{name}"), 1))
    .chain([
        ("clocks crate::handed_both".to_owned(), 2),
        ("clocks crate::both_inside_a_closure".to_owned(), 2),
        ("clocks crate::untyped_now_of_the_other_kind".to_owned(), 2),
    ])
    .collect();
    assert_eq!(counts, expected);
    assert_eq!(refusals.len(), 9, "{refusals:#?}");
    for (line, says) in [
        (165, "written by a macro"),
        (169, "cannot pair"),
        (173, "cannot follow"),
        (178, "written away"),
        (381, "written by a macro"),
        (386, "written by a macro"),
        (393, "not a parameter"),
        (399, "type resolver cannot follow"),
        (532, "whose type is not written"),
    ] {
        assert!(
            refusals
                .iter()
                .any(|refusal| refusal.contains(&format!("lib.rs:{line}: "))
                    && refusal.contains(says)),
            "lib.rs:{line} `{says}`: {refusals:#?}"
        );
    }
}

// ── reading a body's clock ──────────────────────────────────────────────────

/// **What a body says about the real clock, read from its parse**
/// (T-TIMING-BOUND-STRAYS): shape 2's clock arithmetic, shape 3's kept
/// measurements, and the handed-`now` readings; every offset is a byte offset
/// into the text handed in.
mod reading {
    use std::collections::{BTreeMap, BTreeSet};

    use proc_macro2::{TokenStream, TokenTree};
    use syn::parse::Parser;
    use syn::punctuated::Punctuated;
    use syn::spanned::Spanned;
    use syn::visit::{self, Visit};
    use syn::{BinOp, Expr, FnArg, Pat, ReturnType, Stmt, Token, Type, TypeParamBound};

    /// A byte offset in the text handed to [`callable`].
    pub type Offset = usize;

    /// The methods whose receiver and argument are two instants whose distance
    /// is asked.
    const DISTANCES: [&str; 3] = [
        "duration_since",
        "saturating_duration_since",
        "checked_duration_since",
    ];

    /// The methods that compare their receiver with their argument.
    const COMPARING: [&str; 8] = ["cmp", "partial_cmp", "eq", "ne", "lt", "le", "gt", "ge"];

    /// `Duration`'s and `Instant`'s methods whose answer carries their argument's
    /// value as much as their receiver's: a measurement handed to one goes where
    /// the answer goes.
    const CARRYING: [&str; 14] = [
        "saturating_sub",
        "checked_sub",
        "saturating_add",
        "checked_add",
        "saturating_mul",
        "checked_mul",
        "checked_div",
        "abs_diff",
        "min",
        "max",
        "mul_f32",
        "mul_f64",
        "div_f32",
        "div_f64",
    ];

    /// Macros whose arguments are formatted or asserted on: a value handed to
    /// one is consumed, not kept.
    const CONSUMING: [&str; 24] = [
        "format",
        "format_args",
        "print",
        "println",
        "eprint",
        "eprintln",
        "write",
        "writeln",
        "panic",
        "unreachable",
        "todo",
        "unimplemented",
        "assert",
        "assert_eq",
        "assert_ne",
        "debug_assert",
        "debug_assert_eq",
        "debug_assert_ne",
        "dbg",
        "trace",
        "debug",
        "matches",
        "assert_matches",
        "debug_assert_matches",
    ];

    /// The macros of [`CONSUMING`] that only print or format what they are handed (row 20
    /// review N2): a measurement handed only to one of them is shown, and decides nothing.
    const PRINTING: [&str; 15] = [
        "format",
        "format_args",
        "print",
        "println",
        "eprint",
        "eprintln",
        "write",
        "writeln",
        "panic",
        "unreachable",
        "todo",
        "unimplemented",
        "dbg",
        "trace",
        "debug",
    ];

    /// What one body says.
    #[derive(Debug, Default)]
    pub struct Reading {
        /// Clock arithmetic between two different readings (shape 2).
        pub arithmetic: Vec<Offset>,
        /// Measurements whose value is kept (shape 3).
        pub kept: Vec<Offset>,
        /// Measurements printed or formatted ([`PRINTING`]), by where they are measured.
        pub printed: BTreeSet<Offset>,
        /// Measurements compared or discarded, by where they are measured.
        pub consumed: BTreeSet<Offset>,
        /// What this reading cannot decide, and why.
        pub refusals: Vec<(Offset, String)>,
    }

    impl Reading {
        /// How many measurements are printed and put to no other use: none of them decides
        /// anything (row 20 review N2).
        pub fn only_printed(&self) -> usize {
            self.printed
                .iter()
                .filter(|at| !self.consumed.contains(at) && !self.kept.contains(at))
                .count()
        }
    }

    /// The names a format string captures inline (`{took:?}`, `{took}`), in order; `{{` is
    /// a literal brace, and a position or an empty `{}` captures nothing.
    fn captured_names(text: &str) -> Vec<String> {
        let mut names = Vec::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            rest = &rest[open + 1..];
            if let Some(after) = rest.strip_prefix('{') {
                rest = after;
                continue;
            }
            let Some(close) = rest.find('}') else { break };
            let name = rest[..close].split(':').next().unwrap_or_default().trim();
            if name.starts_with(|c: char| c.is_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                names.push(name.to_owned());
            }
            rest = &rest[close + 1..];
        }
        names
    }

    /// The signature and body of a callable's whole text, parsed as the item it
    /// is.
    pub fn callable(
        text: &str,
        function: bool,
        owned: bool,
    ) -> syn::Result<(syn::Signature, Option<syn::Block>)> {
        if function {
            syn::parse_str::<syn::ItemFn>(text).map(|parsed| (parsed.sig, Some(*parsed.block)))
        } else if owned {
            syn::parse_str::<syn::ImplItemFn>(text).map(|parsed| (parsed.sig, Some(parsed.block)))
        } else {
            syn::parse_str::<syn::TraitItemFn>(text).map(|parsed| (parsed.sig, parsed.default))
        }
    }

    fn offset(spanned: &impl Spanned) -> Offset {
        spanned.span().byte_range().start
    }

    /// Which clock a call reads, if it is a reading.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Clock {
        Instant,
        SystemTime,
    }

    /// `Instant::now()` / `SystemTime::now()` in any qualification,
    /// `<Instant>::now()` too.
    pub fn read_of(expr: &Expr) -> Option<Clock> {
        let Expr::Call(call) = expr else { return None };
        if !call.args.is_empty() {
            return None;
        }
        let Expr::Path(path) = &*call.func else {
            return None;
        };
        named_now(path)
    }

    /// The clock a path names when it is `Instant::now` / `SystemTime::now` in
    /// any qualification, `<Instant>::now` too: called, a reading; handed on as
    /// a function value, a reading deferred.
    pub fn named_now(path: &syn::ExprPath) -> Option<Clock> {
        let mut segments = path.path.segments.iter().rev();
        let last = segments.next()?;
        if last.ident != "now" || !last.arguments.is_none() {
            return None;
        }
        let owner = match (segments.next(), &path.qself) {
            (Some(owner), None) => owner.ident.to_string(),
            (None, Some(qself)) => {
                let Type::Path(ty) = &*qself.ty else {
                    return None;
                };
                ty.path.segments.last()?.ident.to_string()
            }
            _ => return None,
        };
        match owner.as_str() {
            "Instant" => Some(Clock::Instant),
            "SystemTime" => Some(Clock::SystemTime),
            _ => None,
        }
    }

    /// The name an expression is, when it is a lone identifier.
    fn lone_name(expr: &Expr) -> Option<String> {
        let Expr::Path(path) = expr else { return None };
        if path.qself.is_some() || path.path.segments.len() != 1 {
            return None;
        }
        Some(path.path.segments[0].ident.to_string())
    }

    /// A macro's arguments, when they parse as comma-separated expressions or as
    /// `a; n`.
    fn arguments(mac: &syn::Macro) -> Option<Vec<Expr>> {
        if let Ok(list) = mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated) {
            return Some(list.into_iter().collect());
        }
        let repeat = |input: syn::parse::ParseStream<'_>| -> syn::Result<Vec<Expr>> {
            let value: Expr = input.parse()?;
            input.parse::<Token![;]>()?;
            let count: Expr = input.parse()?;
            Ok(vec![value, count])
        };
        if let Ok(read) = repeat.parse2(mac.tokens.clone()) {
            return Some(read);
        }
        let matching = |input: syn::parse::ParseStream<'_>| -> syn::Result<Vec<Expr>> {
            let value: Expr = input.parse()?;
            input.parse::<Token![,]>()?;
            Pat::parse_multi_with_leading_vert(input)?;
            let mut read = vec![value];
            if input.peek(Token![if]) {
                input.parse::<Token![if]>()?;
                read.push(input.parse()?);
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
                if !input.is_empty() {
                    let rest = Punctuated::<Expr, Token![,]>::parse_terminated(input)?;
                    read.extend(rest);
                }
            }
            Ok(read)
        };
        matching.parse2(mac.tokens.clone()).ok()
    }

    /// Whether a callable's whole text declares it without `pub`.
    pub fn private(text: &str, function: bool, owned: bool) -> bool {
        let inherited = |vis: &syn::Visibility| matches!(vis, syn::Visibility::Inherited);
        if function {
            syn::parse_str::<syn::ItemFn>(text).is_ok_and(|parsed| inherited(&parsed.vis))
        } else if owned {
            syn::parse_str::<syn::ImplItemFn>(text).is_ok_and(|parsed| inherited(&parsed.vis))
        } else {
            false
        }
    }

    /// Whether a field's whole text declares a field of a type holding an
    /// instant ([`holds_an_instant`]).
    pub fn instant_field(text: &str) -> bool {
        syn::Field::parse_named
            .parse_str(text.trim_end_matches(','))
            .is_ok_and(|field| holds_an_instant(&field.ty))
    }

    fn names_in(tokens: TokenStream, out: &mut Vec<String>) {
        for tree in tokens {
            match tree {
                TokenTree::Ident(ident) => out.push(ident.to_string()),
                TokenTree::Group(group) => names_in(group.stream(), out),
                _ => {}
            }
        }
    }

    /// Every name a pattern binds.
    fn pattern_names(pattern: &Pat, out: &mut Vec<String>) {
        match pattern {
            Pat::Ident(ident) => {
                out.push(ident.ident.to_string());
                if let Some((_, inner)) = &ident.subpat {
                    pattern_names(inner, out);
                }
            }
            Pat::Tuple(tuple) => tuple.elems.iter().for_each(|p| pattern_names(p, out)),
            Pat::TupleStruct(tuple) => tuple.elems.iter().for_each(|p| pattern_names(p, out)),
            Pat::Struct(fields) => fields
                .fields
                .iter()
                .for_each(|field| pattern_names(&field.pat, out)),
            Pat::Slice(slice) => slice.elems.iter().for_each(|p| pattern_names(p, out)),
            Pat::Or(or) => or.cases.iter().for_each(|p| pattern_names(p, out)),
            Pat::Reference(reference) => pattern_names(&reference.pat, out),
            Pat::Type(typed) => pattern_names(&typed.pat, out),
            Pat::Paren(paren) => pattern_names(&paren.pat, out),
            _ => {}
        }
    }

    // ── shapes 2 and 3 ──

    /// What a name holds: the readings it was bound from, and the measurements
    /// whose value it carries.
    #[derive(Clone, Debug, Default)]
    struct Binding {
        readings: BTreeSet<Offset>,
        measured: Vec<Offset>,
    }

    /// What becomes of an expression's value.
    #[derive(Clone, Debug)]
    enum Use {
        /// Returned, stored, handed on: a measurement here is kept.
        Kept,
        /// Compared, asserted on, discarded: a measurement here is not.
        Consumed,
        /// Printed or formatted only ([`PRINTING`]): a measurement here decides nothing.
        OnlyPrinted,
        /// Bound to these names, whose own uses decide.
        Bind(Vec<String>),
    }

    #[derive(Default)]
    struct Walk {
        scopes: Vec<BTreeMap<String, Binding>>,
        /// The names a `let` is binding while its initialiser is read.
        pending: BTreeMap<String, Vec<Offset>>,
        out: Reading,
    }

    /// Every reading an expression holds, through the bindings in scope.
    struct Readings<'w> {
        walk: &'w Walk,
        found: BTreeSet<Offset>,
    }

    impl<'ast> Visit<'ast> for Readings<'_> {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            if read_of(expr).is_some() {
                self.found.insert(offset(expr));
                return;
            }
            if let Some(name) = lone_name(expr) {
                if let Some(binding) = self.walk.lookup(&name) {
                    self.found.extend(binding.readings.iter().copied());
                }
                return;
            }
            visit::visit_expr(self, expr);
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            if let Some(args) = arguments(mac) {
                for arg in &args {
                    self.visit_expr(arg);
                }
            }
        }
    }

    impl Walk {
        fn lookup(&self, name: &str) -> Option<&Binding> {
            self.scopes.iter().rev().find_map(|scope| scope.get(name))
        }

        fn lookup_mut(&mut self, name: &str) -> Option<&mut Binding> {
            self.scopes
                .iter_mut()
                .rev()
                .find_map(|scope| scope.get_mut(name))
        }

        fn bind(&mut self, name: String, binding: Binding) {
            if self.scopes.is_empty() {
                self.scopes.push(BTreeMap::new());
            }
            self.scopes
                .last_mut()
                .expect("a scope")
                .insert(name, binding);
        }

        fn scoped(&mut self, inner: impl FnOnce(&mut Self)) {
            self.scopes.push(BTreeMap::new());
            inner(self);
            self.scopes.pop();
        }

        fn readings(&self, expr: &Expr) -> BTreeSet<Offset> {
            let mut readings = Readings {
                walk: self,
                found: BTreeSet::new(),
            };
            readings.visit_expr(expr);
            readings.found
        }

        /// Shape 2's question: do both sides hold readings, and not the same ones?
        fn two_readings(&self, left: &Expr, right: &Expr) -> bool {
            let (left, right) = (self.readings(left), self.readings(right));
            !left.is_empty() && !right.is_empty() && left != right
        }

        /// A measurement (or what a name carries) meets its use.
        fn spend(&mut self, measured: &[Offset], how: &Use) {
            if measured.is_empty() {
                return;
            }
            match how {
                Use::Kept => self.out.kept.extend_from_slice(measured),
                Use::Consumed => self.out.consumed.extend(measured.iter().copied()),
                Use::OnlyPrinted => self.out.printed.extend(measured.iter().copied()),
                Use::Bind(names) => {
                    for name in names {
                        if let Some(pending) = self.pending.get_mut(name) {
                            pending.extend_from_slice(measured);
                        } else if let Some(binding) = self.lookup_mut(name) {
                            binding.measured.extend_from_slice(measured);
                        } else {
                            self.out.kept.extend_from_slice(measured);
                        }
                    }
                }
            }
        }

        /// Whether macro tokens name a clock value this walk would have to follow.
        fn names_a_clock(&self, tokens: &TokenStream) -> bool {
            let mut names = Vec::new();
            names_in(tokens.clone(), &mut names);
            let reads = names.iter().any(|name| name == "now")
                && names
                    .iter()
                    .any(|name| name == "Instant" || name == "SystemTime");
            reads
                || names
                    .iter()
                    .any(|name| name == "elapsed" || DISTANCES.contains(&name.as_str()))
                || names.iter().any(|name| {
                    self.lookup(name).is_some_and(|binding| {
                        !binding.readings.is_empty() || !binding.measured.is_empty()
                    })
                })
        }

        fn mac(&mut self, mac: &syn::Macro, how: &Use) {
            let name = mac
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .unwrap_or_default();
            match arguments(mac) {
                Some(args) => {
                    let each = if PRINTING.contains(&name.as_str()) {
                        self.captured(&args);
                        Use::OnlyPrinted
                    } else if CONSUMING.contains(&name.as_str()) {
                        Use::Consumed
                    } else {
                        Use::Kept
                    };
                    let _ = how;
                    for arg in &args {
                        self.expr(arg, &each);
                    }
                }
                None => {
                    if self.names_a_clock(&mac.tokens) {
                        self.out.refusals.push((
                            offset(mac),
                            format!(
                                "the arguments of `{name}!` do not parse as expressions, and they \
                                 name a clock value this reading would have to follow"
                            ),
                        ));
                    }
                }
            }
        }

        /// A format string's inline captures (`{elapsed:?}`): each a use of the name it
        /// captures, printed (row 20 review N2).
        fn captured(&mut self, args: &[Expr]) {
            let Some(text) = args.iter().find_map(|arg| match arg {
                Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(text),
                    ..
                }) => Some(text.value()),
                _ => None,
            }) else {
                return;
            };
            for name in captured_names(&text) {
                let measured = self
                    .lookup(&name)
                    .map(|binding| binding.measured.clone())
                    .unwrap_or_default();
                self.spend(&measured, &Use::OnlyPrinted);
            }
        }

        fn block(&mut self, block: &syn::Block, how: &Use) {
            self.scopes.push(BTreeMap::new());
            let last = block.stmts.len().checked_sub(1);
            for (at, stmt) in block.stmts.iter().enumerate() {
                let tail = Some(at) == last;
                match stmt {
                    Stmt::Local(local) => self.local(local),
                    Stmt::Expr(expr, semi) => {
                        let how = if tail && semi.is_none() {
                            how.clone()
                        } else {
                            Use::Consumed
                        };
                        self.expr(expr, &how);
                    }
                    Stmt::Macro(mac) => {
                        let how = if tail && mac.semi_token.is_none() {
                            how.clone()
                        } else {
                            Use::Consumed
                        };
                        self.mac(&mac.mac, &how);
                    }
                    Stmt::Item(item) => self.visit_item(item),
                }
            }
            self.scopes.pop();
        }

        fn local(&mut self, local: &syn::Local) {
            let mut names = Vec::new();
            pattern_names(&local.pat, &mut names);
            let mut readings = BTreeSet::new();
            if let Some(init) = &local.init {
                readings = self.readings(&init.expr);
                for name in &names {
                    self.pending.insert(name.clone(), Vec::new());
                }
                self.expr(&init.expr, &Use::Bind(names.clone()));
                if let Some((_, diverge)) = &init.diverge {
                    self.expr(diverge, &Use::Consumed);
                }
            }
            for name in names {
                let measured = self.pending.remove(&name).unwrap_or_default();
                self.bind(
                    name,
                    Binding {
                        readings: readings.clone(),
                        measured,
                    },
                );
            }
        }

        fn bind_pattern(&mut self, pattern: &Pat) {
            let mut names = Vec::new();
            pattern_names(pattern, &mut names);
            for name in names {
                self.bind(name, Binding::default());
            }
        }

        #[allow(clippy::too_many_lines)]
        fn expr(&mut self, expr: &Expr, how: &Use) {
            if read_of(expr).is_some() {
                return;
            }
            let at = offset(expr);
            match expr {
                Expr::Path(_) => {
                    if let Some(name) = lone_name(expr) {
                        let measured = self
                            .lookup(&name)
                            .map(|binding| binding.measured.clone())
                            .unwrap_or_default();
                        self.spend(&measured, how);
                    }
                }
                Expr::MethodCall(call) => {
                    let method = call.method.to_string();
                    if method == "elapsed" && call.args.is_empty() {
                        self.spend(&[at], how);
                        self.expr(&call.receiver, &Use::Kept);
                        return;
                    }
                    if DISTANCES.contains(&method.as_str()) && call.args.len() == 1 {
                        if self.two_readings(&call.receiver, &call.args[0]) {
                            self.out.arithmetic.push(at);
                            self.spend(&[at], how);
                        }
                        self.expr(&call.receiver, &Use::Kept);
                        self.expr(&call.args[0], &Use::Kept);
                        return;
                    }
                    if COMPARING.contains(&method.as_str()) {
                        self.expr(&call.receiver, &Use::Consumed);
                        for arg in &call.args {
                            self.expr(arg, &Use::Consumed);
                        }
                        return;
                    }
                    let each = if super::REAL_WAITS.contains(&method.as_str()) {
                        Use::Consumed
                    } else if CARRYING.contains(&method.as_str()) {
                        how.clone()
                    } else {
                        Use::Kept
                    };
                    self.expr(&call.receiver, how);
                    for arg in &call.args {
                        self.expr(arg, &each);
                    }
                }
                Expr::Call(call) => {
                    let waits = match &*call.func {
                        Expr::Path(path) => path.path.segments.last().is_some_and(|segment| {
                            super::REAL_WAITS.contains(&segment.ident.to_string().as_str())
                        }),
                        _ => false,
                    };
                    self.expr(&call.func, &Use::Consumed);
                    let each = if waits { Use::Consumed } else { Use::Kept };
                    for arg in &call.args {
                        self.expr(arg, &each);
                    }
                }
                Expr::Binary(binary) => match binary.op {
                    BinOp::Lt(_) | BinOp::Le(_) | BinOp::Gt(_) | BinOp::Ge(_) => {
                        if self.two_readings(&binary.left, &binary.right) {
                            self.out.arithmetic.push(at);
                        }
                        self.expr(&binary.left, &Use::Consumed);
                        self.expr(&binary.right, &Use::Consumed);
                    }
                    BinOp::Eq(_) | BinOp::Ne(_) => {
                        self.expr(&binary.left, &Use::Consumed);
                        self.expr(&binary.right, &Use::Consumed);
                    }
                    BinOp::Sub(_) => {
                        if self.two_readings(&binary.left, &binary.right) {
                            self.out.arithmetic.push(at);
                            self.spend(&[at], how);
                        }
                        self.expr(&binary.left, how);
                        self.expr(&binary.right, how);
                    }
                    BinOp::Add(_) | BinOp::Mul(_) | BinOp::Div(_) | BinOp::Rem(_) => {
                        self.expr(&binary.left, how);
                        self.expr(&binary.right, how);
                    }
                    BinOp::AddAssign(_)
                    | BinOp::SubAssign(_)
                    | BinOp::MulAssign(_)
                    | BinOp::DivAssign(_)
                    | BinOp::RemAssign(_) => {
                        if let Some(name) = lone_name(&binary.left) {
                            let readings = self.readings(&binary.right);
                            self.expr(&binary.right, &Use::Bind(vec![name.clone()]));
                            if let Some(binding) = self.lookup_mut(&name) {
                                binding.readings.extend(readings);
                            }
                        } else {
                            self.expr(&binary.left, &Use::Kept);
                            self.expr(&binary.right, &Use::Kept);
                        }
                    }
                    _ => {
                        self.expr(&binary.left, &Use::Consumed);
                        self.expr(&binary.right, &Use::Consumed);
                    }
                },
                Expr::Assign(assign) => {
                    if let Some(name) = lone_name(&assign.left) {
                        let readings = self.readings(&assign.right);
                        self.expr(&assign.right, &Use::Bind(vec![name.clone()]));
                        if let Some(binding) = self.lookup_mut(&name) {
                            binding.readings = readings;
                        }
                    } else {
                        self.expr(&assign.left, &Use::Kept);
                        self.expr(&assign.right, &Use::Kept);
                    }
                }
                Expr::Cast(cast) => self.expr(&cast.expr, how),
                Expr::Paren(paren) => self.expr(&paren.expr, how),
                Expr::Group(group) => self.expr(&group.expr, how),
                Expr::Reference(reference) => self.expr(&reference.expr, how),
                Expr::Unary(unary) => self.expr(&unary.expr, how),
                Expr::Try(attempt) => self.expr(&attempt.expr, how),
                Expr::Block(block) => self.block(&block.block, how),
                Expr::Unsafe(block) => self.block(&block.block, how),
                Expr::If(branch) => {
                    self.expr(&branch.cond, &Use::Consumed);
                    self.block(&branch.then_branch, how);
                    if let Some((_, otherwise)) = &branch.else_branch {
                        self.expr(otherwise, how);
                    }
                }
                Expr::Match(choice) => {
                    self.expr(&choice.expr, &Use::Kept);
                    for arm in &choice.arms {
                        self.scoped(|walk| {
                            walk.bind_pattern(&arm.pat);
                            if let Some((_, guard)) = &arm.guard {
                                walk.expr(guard, &Use::Consumed);
                            }
                            walk.expr(&arm.body, how);
                        });
                    }
                }
                Expr::Closure(closure) => self.scoped(|walk| {
                    for input in &closure.inputs {
                        walk.bind_pattern(input);
                    }
                    walk.expr(&closure.body, &Use::Kept);
                }),
                Expr::ForLoop(each) => {
                    self.expr(&each.expr, &Use::Kept);
                    self.scoped(|walk| {
                        walk.bind_pattern(&each.pat);
                        walk.block(&each.body, &Use::Consumed);
                    });
                }
                Expr::While(each) => {
                    self.expr(&each.cond, &Use::Consumed);
                    self.block(&each.body, &Use::Consumed);
                }
                Expr::Loop(each) => self.block(&each.body, &Use::Consumed),
                Expr::Let(binding) => {
                    let mut names = Vec::new();
                    pattern_names(&binding.pat, &mut names);
                    let readings = self.readings(&binding.expr);
                    for name in &names {
                        self.pending.insert(name.clone(), Vec::new());
                    }
                    self.expr(&binding.expr, &Use::Bind(names.clone()));
                    for name in names {
                        let measured = self.pending.remove(&name).unwrap_or_default();
                        self.bind(
                            name,
                            Binding {
                                readings: readings.clone(),
                                measured,
                            },
                        );
                    }
                }
                Expr::Macro(mac) => self.mac(&mac.mac, how),
                _ => visit::visit_expr(self, expr),
            }
        }
    }

    impl<'ast> Visit<'ast> for Walk {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            self.expr(expr, &Use::Kept);
        }

        fn visit_block(&mut self, block: &'ast syn::Block) {
            self.block(block, &Use::Kept);
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            self.mac(mac, &Use::Kept);
        }

        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            let outer = std::mem::take(&mut self.scopes);
            self.block(&item.block, &Use::Kept);
            self.scopes = outer;
        }
    }

    /// Shapes 2 and 3 of one body: its tail is the function's answer, so a
    /// measurement there is kept.
    pub fn read(block: &syn::Block) -> Reading {
        let mut walk = Walk::default();
        walk.block(block, &Use::Kept);
        walk.out
    }

    // ── the handed-now reading ──

    /// **Which kinds of clock** a type answers or a body is handed: the
    /// monotonic one (`Instant`) and the wall one (`SystemTime`).
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Kinds {
        pub instant: bool,
        pub wall: bool,
    }

    impl Kinds {
        pub const INSTANT: Self = Self {
            instant: true,
            wall: false,
        };
        pub const WALL: Self = Self {
            instant: false,
            wall: true,
        };
        pub const BOTH: Self = Self {
            instant: true,
            wall: true,
        };

        /// The kind a reading of `clock` is.
        pub const fn of(clock: Clock) -> Self {
            match clock {
                Clock::Instant => Self::INSTANT,
                Clock::SystemTime => Self::WALL,
            }
        }

        pub const fn union(self, other: Self) -> Self {
            Self {
                instant: self.instant || other.instant,
                wall: self.wall || other.wall,
            }
        }

        pub const fn is_empty(self) -> bool {
            !self.instant && !self.wall
        }

        /// Whether the two sets share a kind.
        pub const fn meets(self, other: Self) -> bool {
            (self.instant && other.instant) || (self.wall && other.wall)
        }

        /// The set, as `HANDED-CLOCKS.tsv` writes it.
        pub const fn label(self) -> &'static str {
            match (self.instant, self.wall) {
                (true, true) => "both",
                (true, false) => "instant",
                (false, true) => "wall",
                (false, false) => "none",
            }
        }
    }

    /// What the handed-now reading says of one function.
    #[derive(Debug, Default)]
    pub struct Handed {
        /// How the function is handed a clock, when it is: `now`, `closure`,
        /// `trait`, `generic`, `carrier`, `self field`, `impl bound`,
        /// `trait default`.
        pub how: BTreeSet<&'static str>,
        /// The kinds of clock its signature hands it.
        pub kinds: Kinds,
        /// Readings of a real clock of a kind the body it stands in is handed —
        /// `Instant::now()` when handed an instant, `SystemTime::now()` when
        /// handed a wall time, either `now` handed on as a function value too,
        /// and `.elapsed()` of its receiver's kind (both kinds when the
        /// receiver's type is not written) — measurement origins aside.
        pub again: Vec<Offset>,
        /// What it cannot decide, and why.
        pub refusals: Vec<(Offset, String)>,
    }

    /// **What a path written as a bound, or a type, is** to the handed-now
    /// reading, as the caller reads it in the writer's module.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Bound {
        /// A clock, how it was told (`closure`, `trait`, `generic`,
        /// `carrier`), and the kinds it answers.
        Clock(&'static str, Kinds),
        /// Not a clock.
        Not,
        /// What the reading cannot tell, and why.
        Unread(String),
    }

    impl Bound {
        /// The two answers together: a clock over a refusal over nothing; two
        /// clocks answer both their kinds.
        fn or(self, other: Self) -> Self {
            match (self, other) {
                (Self::Clock(how, one), Self::Clock(_, two)) => Self::Clock(how, one.union(two)),
                (Self::Clock(how, kinds), _) | (_, Self::Clock(how, kinds)) => {
                    Self::Clock(how, kinds)
                }
                (Self::Unread(why), _) | (_, Self::Unread(why)) => Self::Unread(why),
                (Self::Not, Self::Not) => Self::Not,
            }
        }
    }

    /// How a path reads, in the module of what is being read.
    pub type ReadPath<'s> = &'s dyn Fn(&syn::Path) -> Bound;

    /// **How the reading tells a clock from a name**: `bound` reads a trait
    /// path written as a bound (a clock trait or not); `carrier` reads a type
    /// path (a type that carries a clock or not).
    #[derive(Clone, Copy)]
    pub struct Names<'s> {
        pub bound: ReadPath<'s>,
        pub carrier: ReadPath<'s>,
    }

    /// **What the handed-now reading knows of a callable beyond its own
    /// text**: how names read in its module, the type parameters the block it
    /// is written in (`impl` or trait) bounds by a clock, and how its `self`
    /// is handed, if it is.
    pub struct Scope<'s> {
        pub names: Names<'s>,
        pub generics: ClockParameters,
        pub self_how: Option<(&'static str, Kinds)>,
    }

    /// The type parameters in force that are bounded by a clock, with the
    /// kinds each answers.
    pub type ClockParameters = BTreeMap<String, Kinds>;

    /// **What a body is handed**: the kinds of clock, and whether a closure
    /// parameter `now` with no written type stands over it (a reading of a
    /// kind it is not handed is then refused, not passed).
    #[derive(Clone, Copy, Default, PartialEq, Eq)]
    struct Hand {
        kinds: Kinds,
        unknown: bool,
    }

    impl Hand {
        const fn is_handed(self) -> bool {
            !self.kinds.is_empty() || self.unknown
        }
    }

    /// The clock reading's type a path names: its last segment is `Instant`
    /// (the monotonic clock) or `SystemTime` (the wall clock).
    fn names_a_time(path: &syn::Path) -> Kinds {
        match path.segments.last() {
            Some(segment) if segment.ident == "Instant" => Kinds::INSTANT,
            Some(segment) if segment.ident == "SystemTime" => Kinds::WALL,
            _ => Kinds::default(),
        }
    }

    /// **The kinds of instant a type holds anywhere**: an `Instant` or a
    /// `SystemTime`, behind a reference or a pointer, in a tuple, an array, a
    /// slice, or any path's type arguments (`Option<Instant>`, `Box<Instant>`,
    /// `Result<Instant, E>`, `(Instant, u64)`, `[SystemTime]`).
    fn time_kinds(ty: &Type) -> Kinds {
        match ty {
            Type::Reference(reference) => time_kinds(&reference.elem),
            Type::Ptr(pointer) => time_kinds(&pointer.elem),
            Type::Paren(inner) => time_kinds(&inner.elem),
            Type::Group(inner) => time_kinds(&inner.elem),
            Type::Array(array) => time_kinds(&array.elem),
            Type::Slice(slice) => time_kinds(&slice.elem),
            Type::Tuple(tuple) => tuple
                .elems
                .iter()
                .fold(Kinds::default(), |kinds, ty| kinds.union(time_kinds(ty))),
            Type::Path(path) => type_arguments(&path.path)
                .fold(names_a_time(&path.path), |kinds, ty| {
                    kinds.union(time_kinds(ty))
                }),
            _ => Kinds::default(),
        }
    }

    /// Whether a type holds an instant of either kind anywhere.
    fn holds_an_instant(ty: &Type) -> bool {
        !time_kinds(ty).is_empty()
    }

    /// Every type argument a path's segments carry.
    fn type_arguments(path: &syn::Path) -> impl Iterator<Item = &Type> {
        path.segments
            .iter()
            .filter_map(|segment| match &segment.arguments {
                syn::PathArguments::AngleBracketed(arguments) => Some(&arguments.args),
                _ => None,
            })
            .flatten()
            .filter_map(|argument| match argument {
                syn::GenericArgument::Type(ty) => Some(ty),
                _ => None,
            })
    }

    /// The kinds of instant an answer holds.
    fn answer_kinds(output: &ReturnType) -> Kinds {
        match output {
            ReturnType::Type(_, ty) => time_kinds(ty),
            ReturnType::Default => Kinds::default(),
        }
    }

    /// What one bound is: `Fn() -> A`, `FnMut() -> A` or `FnOnce() -> A`
    /// with no inputs and an answer holding an instant is a clock, one with
    /// inputs is not; any other trait is what `bound` makes of its path; a
    /// bound kept as verbatim tokens is not read.
    fn bound_kind(bound: &TypeParamBound, names: Names<'_>) -> Bound {
        let bound = match bound {
            TypeParamBound::Trait(bound) => bound,
            TypeParamBound::Verbatim(_) => {
                return Bound::Unread(
                    "a bound the handed-now reading does not read (tokens kept verbatim)"
                        .to_owned(),
                );
            }
            _ => return Bound::Not,
        };
        let Some(last) = bound.path.segments.last() else {
            return Bound::Not;
        };
        if let syn::PathArguments::Parenthesized(arguments) = &last.arguments
            && matches!(last.ident.to_string().as_str(), "Fn" | "FnMut" | "FnOnce")
        {
            let kinds = answer_kinds(&arguments.output);
            return if arguments.inputs.is_empty() && !kinds.is_empty() {
                Bound::Clock("closure", kinds)
            } else {
                Bound::Not
            };
        }
        (names.bound)(&bound.path)
    }

    fn bounds_kind<'b>(
        bounds: impl IntoIterator<Item = &'b TypeParamBound>,
        names: Names<'_>,
    ) -> Bound {
        bounds
            .into_iter()
            .fold(Bound::Not, |kind, bound| kind.or(bound_kind(bound, names)))
    }

    fn kinds<'t>(
        types: impl Iterator<Item = &'t Type>,
        generics: &ClockParameters,
        names: Names<'_>,
    ) -> Bound {
        types.fold(Bound::Not, |kind, ty| {
            kind.or(type_kind(ty, generics, names))
        })
    }

    /// **What a type hands a body** (see the guard's grammar): a clock when it
    /// is one — a clock bound behind `impl` or `dyn`, a `fn() -> A` pointer, a
    /// type parameter in `generics` (`Self` among them when `self` is handed),
    /// a type that carries a clock — or holds one, behind a reference or a
    /// pointer, in a tuple, an array, a slice, or a path's type arguments
    /// (`Box<dyn Fn() -> Instant>`, `Option<&Deadline>`, `Ops<Clock>`).
    fn type_kind(ty: &Type, generics: &ClockParameters, names: Names<'_>) -> Bound {
        match ty {
            Type::ImplTrait(bounded) => bounds_kind(&bounded.bounds, names),
            Type::TraitObject(bounded) => bounds_kind(&bounded.bounds, names),
            Type::Reference(reference) => type_kind(&reference.elem, generics, names),
            Type::Ptr(pointer) => type_kind(&pointer.elem, generics, names),
            Type::Paren(inner) => type_kind(&inner.elem, generics, names),
            Type::Group(inner) => type_kind(&inner.elem, generics, names),
            Type::Array(array) => type_kind(&array.elem, generics, names),
            Type::Slice(slice) => type_kind(&slice.elem, generics, names),
            Type::Tuple(tuple) => kinds(tuple.elems.iter(), generics, names),
            Type::BareFn(pointer) => {
                let kinds = answer_kinds(&pointer.output);
                if pointer.inputs.is_empty() && !kinds.is_empty() {
                    Bound::Clock("closure", kinds)
                } else {
                    Bound::Not
                }
            }
            Type::Path(path) => {
                if path.qself.is_none()
                    && let Some(name) = path.path.get_ident()
                    && let Some(kinds) = generics.get(&name.to_string())
                {
                    let how = if name == "Self" { "carrier" } else { "generic" };
                    return Bound::Clock(how, *kinds);
                }
                let carried = if path.qself.is_none() {
                    (names.carrier)(&path.path)
                } else {
                    Bound::Not
                };
                carried.or(kinds(type_arguments(&path.path), generics, names))
            }
            Type::Never(_) => Bound::Not,
            Type::Macro(written) => {
                let mut tokens = Vec::new();
                names_in(written.mac.tokens.clone(), &mut tokens);
                let clockish = tokens
                    .iter()
                    .filter(|name| !name.starts_with("r#"))
                    .any(|name| {
                        let path =
                            syn::Path::from(syn::Ident::new(name, proc_macro2::Span::call_site()));
                        matches!(
                            name.as_str(),
                            "Instant" | "SystemTime" | "Fn" | "FnMut" | "FnOnce"
                        ) || (names.bound)(&path) != Bound::Not
                            || (names.carrier)(&path) != Bound::Not
                    });
                if clockish {
                    Bound::Unread(
                        "a type written by a macro whose arguments name a clock reading's \
                         type, a `Fn` trait, a clock trait or a type carrying a clock"
                            .to_owned(),
                    )
                } else {
                    Bound::Not
                }
            }
            _ => Bound::Unread(
                "a type the handed-now reading does not read (`_`, or tokens kept verbatim)"
                    .to_owned(),
            ),
        }
    }

    /// The type parameters a header bounds by a clock, and what it cannot read.
    pub type ClockGenerics = (ClockParameters, Vec<(Offset, String)>);

    /// **The type parameters `generics` bounds by a clock**, inline or in its
    /// `where` clause, and what it cannot read: a bound it cannot follow, and a
    /// `where` clause that bounds by a clock a type that is not a parameter.
    pub fn clock_generics(generics: &syn::Generics, names: Names<'_>) -> ClockGenerics {
        let mut clocks = ClockParameters::new();
        let mut refusals = Vec::new();
        for param in generics.type_params() {
            match bounds_kind(&param.bounds, names) {
                Bound::Clock(_, kinds) => {
                    let known = clocks.entry(param.ident.to_string()).or_default();
                    *known = known.union(kinds);
                }
                Bound::Unread(why) => refusals.push((offset(param), why)),
                Bound::Not => {}
            }
        }
        for predicate in generics
            .where_clause
            .iter()
            .flat_map(|clause| &clause.predicates)
        {
            let syn::WherePredicate::Type(predicate) = predicate else {
                continue;
            };
            let named = match &predicate.bounded_ty {
                Type::Path(path) if path.qself.is_none() => path.path.get_ident(),
                _ => None,
            };
            match (bounds_kind(&predicate.bounds, names), named) {
                (Bound::Clock(_, kinds), Some(name)) => {
                    let known = clocks.entry(name.to_string()).or_default();
                    *known = known.union(kinds);
                }
                (Bound::Clock(..), None) => refusals.push((
                    offset(predicate),
                    "a `where` clause bounds by a clock a type that is not a parameter".to_owned(),
                )),
                (Bound::Unread(why), _) => refusals.push((offset(predicate), why)),
                (Bound::Not, _) => {}
            }
        }
        (clocks, refusals)
    }

    /// The kinds of instant a pattern binds `now` to, through tuple patterns
    /// over tuple types: `Some` of them (none when `now` is of a type holding
    /// no instant), `None` when it binds no `now` the reading can pair with a
    /// type.
    fn binds_now(pattern: &Pat, ty: &Type) -> Option<Kinds> {
        match (pattern, ty) {
            (Pat::Ident(ident), _) if ident.ident == "now" => Some(time_kinds(ty)),
            (Pat::Tuple(patterns), Type::Tuple(types))
                if patterns.elems.len() == types.elems.len() =>
            {
                patterns
                    .elems
                    .iter()
                    .zip(&types.elems)
                    .filter_map(|(pattern, ty)| binds_now(pattern, ty))
                    .reduce(Kinds::union)
            }
            _ => None,
        }
    }

    /// What one typed parameter hands its body, of a signature or a closure.
    enum Gives {
        /// A clock, how it was told, and its kinds.
        Clock(&'static str, Kinds),
        /// A `now` of a type that holds no instant.
        OtherNow,
        /// Nothing to say.
        Nothing,
    }

    /// **What one typed parameter hands its body**: a clock of any name; a
    /// `now` of a type holding an instant (`now`); a `now` of another type;
    /// or nothing. A type it cannot read, and a `now` in a pattern it cannot
    /// pair with the type, are refused.
    fn parameter(
        pattern: &Pat,
        ty: &Type,
        generics: &ClockParameters,
        names: Names<'_>,
        refusals: &mut Vec<(Offset, String)>,
    ) -> Gives {
        match type_kind(ty, generics, names) {
            Bound::Clock(how, kinds) => return Gives::Clock(how, kinds),
            Bound::Unread(why) => refusals.push((offset(ty), why)),
            Bound::Not => {}
        }
        match binds_now(pattern, ty) {
            Some(kinds) if !kinds.is_empty() => Gives::Clock("now", kinds),
            Some(_) => Gives::OtherNow,
            None => {
                let mut bound = Vec::new();
                pattern_names(pattern, &mut bound);
                if bound.iter().any(|name| name == "now") {
                    refusals.push((
                        offset(pattern),
                        "a `now` bound by a pattern the handed-now reading cannot pair with its \
                         type"
                            .to_owned(),
                    ));
                }
                Gives::Nothing
            }
        }
    }

    /// How a signature hands its body a clock.
    struct Signed {
        how: BTreeSet<&'static str>,
        kinds: Kinds,
        /// The clock-bounded type parameters in force inside it.
        generics: ClockParameters,
    }

    /// How a signature hands its body a clock (see the guard's grammar), the
    /// kinds, and the clock-bounded type parameters in force inside it.
    fn signature_handed(
        signature: &syn::Signature,
        scope: &Scope<'_>,
        refusals: &mut Vec<(Offset, String)>,
    ) -> Signed {
        let (mut generics, unread) = clock_generics(&signature.generics, scope.names);
        refusals.extend(unread);
        for (name, kinds) in &scope.generics {
            generics.entry(name.clone()).or_insert(*kinds);
        }
        let mut how = BTreeSet::new();
        let mut handed_kinds = Kinds::default();
        if let Some((handed, kinds)) = scope.self_how
            && signature.receiver().is_some()
        {
            how.insert(handed);
            handed_kinds = handed_kinds.union(kinds);
        }
        for input in &signature.inputs {
            if let FnArg::Typed(typed) = input
                && let Gives::Clock(handed, kinds) =
                    parameter(&typed.pat, &typed.ty, &generics, scope.names, refusals)
            {
                how.insert(handed);
                handed_kinds = handed_kinds.union(kinds);
            }
        }
        Signed {
            how,
            kinds: handed_kinds,
            generics,
        }
    }

    /// **The kinds of clock a trait declaration is of its own**: those its
    /// methods that take their receiver and nothing else answer (none when it
    /// is no clock).
    pub fn clock_trait(declared: &syn::ItemTrait) -> Kinds {
        declared
            .items
            .iter()
            .filter_map(|item| match item {
                syn::TraitItem::Fn(method)
                    if method.sig.receiver().is_some() && method.sig.inputs.len() == 1 =>
                {
                    Some(answer_kinds(&method.sig.output))
                }
                _ => None,
            })
            .fold(Kinds::default(), Kinds::union)
    }

    /// A data type's whole text, parsed as an item (the caller reads only
    /// structs, enums and unions); an error when it does not parse.
    pub fn data_type(text: &str) -> syn::Result<syn::Item> {
        syn::parse_str::<syn::Item>(text)
    }

    /// **The kinds of clock a data type's fields hold** (its own clock-bounded
    /// parameters counted, and types that carry a clock as `names` reads them;
    /// none when it carries no clock), and what it cannot read.
    pub fn carries_a_clock(
        declared: &syn::Item,
        names: Names<'_>,
    ) -> (Kinds, Vec<(Offset, String)>) {
        let (generics, fields): (&syn::Generics, Vec<&syn::Field>) = match declared {
            syn::Item::Struct(item) => (&item.generics, item.fields.iter().collect()),
            syn::Item::Enum(item) => (
                &item.generics,
                item.variants
                    .iter()
                    .flat_map(|variant| variant.fields.iter())
                    .collect(),
            ),
            syn::Item::Union(item) => (&item.generics, item.fields.named.iter().collect()),
            _ => return (Kinds::default(), Vec::new()),
        };
        let (clocks, mut refusals) = clock_generics(generics, names);
        let mut carries = Kinds::default();
        for field in fields {
            match type_kind(&field.ty, &clocks, names) {
                Bound::Clock(_, kinds) => carries = carries.union(kinds),
                Bound::Unread(why) => refusals.push((offset(&field.ty), why)),
                Bound::Not => {}
            }
        }
        (carries, refusals)
    }

    /// **The type parameters an `impl` or trait header bounds by a clock**,
    /// from the header's text (everything before the block's braces).
    pub fn header_generics(header: &str, names: Names<'_>) -> syn::Result<ClockGenerics> {
        let text = format!("{header}{{}}");
        if let Ok(block) = syn::parse_str::<syn::ItemImpl>(&text) {
            return Ok(clock_generics(&block.generics, names));
        }
        let declared = syn::parse_str::<syn::ItemTrait>(&text)?;
        Ok(clock_generics(&declared.generics, names))
    }

    /// **What a name in a handed walk is bound to**: the kinds of instant its
    /// written type names (`None` when its type is not written, or names
    /// neither), and the measurement origin it holds, when it was bound by
    /// `let x = Instant::now();` or `let x = SystemTime::now();`.
    #[derive(Clone, Copy, Default)]
    struct Name {
        kinds: Option<Kinds>,
        origin: Option<usize>,
    }

    /// One measurement origin: where its reading stands, the kind it read, and
    /// how often its binding is used other than as the receiver of
    /// `.elapsed()`.
    struct Origin {
        at: Offset,
        kinds: Kinds,
        other: usize,
    }

    /// The kinds of instant a written type is, behind references: `Instant`,
    /// `SystemTime`, or `None` for any other type.
    fn written_kinds(ty: &Type) -> Option<Kinds> {
        match ty {
            Type::Reference(reference) => written_kinds(&reference.elem),
            Type::Paren(inner) => written_kinds(&inner.elem),
            Type::Group(inner) => written_kinds(&inner.elem),
            Type::Path(path) if path.qself.is_none() => {
                let kinds = names_a_time(&path.path);
                (!kinds.is_empty()).then_some(kinds)
            }
            _ => None,
        }
    }

    struct HandedWalk<'s> {
        names: Names<'s>,
        /// What each body in force is handed, innermost last.
        handed: Vec<Hand>,
        /// The clock-bounded type parameters in force, innermost last.
        generics: Vec<ClockParameters>,
        /// The names in scope, innermost block last.
        scopes: Vec<BTreeMap<String, Name>>,
        /// Readings found: where, what the body they stand in is handed, and
        /// the kind read.
        readings: Vec<(Offset, Hand, Kinds)>,
        /// `.elapsed()` calls: where, what the body is handed, the kinds read,
        /// and the origin its receiver holds, if it holds one.
        elapsed: Vec<(Offset, Hand, Kinds, Option<usize>)>,
        origins: Vec<Origin>,
        refusals: Vec<(Offset, String)>,
    }

    impl HandedWalk<'_> {
        fn current(&self) -> Hand {
            self.handed.last().copied().unwrap_or_default()
        }

        fn lookup(&self, name: &str) -> Option<Name> {
            self.scopes
                .iter()
                .rev()
                .find_map(|scope| scope.get(name).copied())
        }

        fn bind(&mut self, name: String, held: Name) {
            if let Some(scope) = self.scopes.last_mut() {
                scope.insert(name, held);
            }
        }

        /// Binds every name a pattern binds in the innermost scope: a lone
        /// name of a written type with that type's kinds, any other with none.
        fn bind_pattern(&mut self, pattern: &Pat, ty: Option<&Type>) {
            match (pattern, ty) {
                (Pat::Type(typed), _) => self.bind_pattern(&typed.pat, Some(&typed.ty)),
                (Pat::Ident(ident), Some(ty)) if ident.subpat.is_none() => {
                    let held = Name {
                        kinds: written_kinds(ty),
                        origin: None,
                    };
                    self.bind(ident.ident.to_string(), held);
                }
                _ => {
                    let mut bound = Vec::new();
                    pattern_names(pattern, &mut bound);
                    for name in bound {
                        self.bind(name, Name::default());
                    }
                }
            }
        }

        fn scoped(&mut self, walk: impl FnOnce(&mut Self)) {
            self.scopes.push(BTreeMap::new());
            walk(self);
            self.scopes.pop();
        }
    }

    impl<'ast> Visit<'ast> for HandedWalk<'_> {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            if let Some(clock) = read_of(expr) {
                self.readings
                    .push((offset(expr), self.current(), Kinds::of(clock)));
                return;
            }
            match expr {
                Expr::MethodCall(call) if call.method == "elapsed" && call.args.is_empty() => {
                    let (kinds, origin) = if let Some(clock) = read_of(&call.receiver) {
                        self.visit_expr(&call.receiver);
                        (Kinds::of(clock), None)
                    } else if let Some(name) = lone_name(&call.receiver) {
                        match self.lookup(&name) {
                            Some(Name {
                                origin: Some(origin),
                                ..
                            }) => (self.origins[origin].kinds, Some(origin)),
                            Some(Name {
                                kinds: Some(kinds), ..
                            }) => (kinds, None),
                            _ => (Kinds::BOTH, None),
                        }
                    } else {
                        self.visit_expr(&call.receiver);
                        (Kinds::BOTH, None)
                    };
                    self.elapsed
                        .push((offset(expr), self.current(), kinds, origin));
                }
                Expr::Path(path) => {
                    if let Some(clock) = named_now(path) {
                        self.readings
                            .push((offset(expr), self.current(), Kinds::of(clock)));
                    } else if let Some(name) = lone_name(expr)
                        && let Some(Name {
                            origin: Some(origin),
                            ..
                        }) = self.lookup(&name)
                    {
                        self.origins[origin].other += 1;
                    }
                }
                Expr::Closure(closure) => {
                    let generics = self.generics.last().cloned().unwrap_or_default();
                    let (mut own, mut untyped_now) = (Kinds::default(), false);
                    for input in &closure.inputs {
                        match input {
                            Pat::Type(typed) => {
                                if let Gives::Clock(_, kinds) = parameter(
                                    &typed.pat,
                                    &typed.ty,
                                    &generics,
                                    self.names,
                                    &mut self.refusals,
                                ) {
                                    own = own.union(kinds);
                                }
                            }
                            Pat::Ident(ident) if ident.ident == "now" => untyped_now = true,
                            _ => {}
                        }
                    }
                    let outer = self.current();
                    self.handed.push(Hand {
                        kinds: outer.kinds.union(own),
                        unknown: outer.unknown || untyped_now,
                    });
                    self.scoped(|walk| {
                        for input in &closure.inputs {
                            walk.bind_pattern(input, None);
                        }
                        walk.visit_expr(&closure.body);
                    });
                    self.handed.pop();
                }
                Expr::If(branch) => {
                    self.scoped(|walk| {
                        walk.visit_expr(&branch.cond);
                        walk.visit_block(&branch.then_branch);
                    });
                    if let Some((_, otherwise)) = &branch.else_branch {
                        self.visit_expr(otherwise);
                    }
                }
                Expr::While(looped) => self.scoped(|walk| {
                    walk.visit_expr(&looped.cond);
                    walk.visit_block(&looped.body);
                }),
                Expr::Let(bound) => {
                    self.visit_expr(&bound.expr);
                    self.bind_pattern(&bound.pat, None);
                }
                Expr::ForLoop(looped) => {
                    self.visit_expr(&looped.expr);
                    self.scoped(|walk| {
                        walk.bind_pattern(&looped.pat, None);
                        walk.visit_block(&looped.body);
                    });
                }
                _ => visit::visit_expr(self, expr),
            }
        }

        fn visit_arm(&mut self, arm: &'ast syn::Arm) {
            self.scoped(|walk| {
                walk.bind_pattern(&arm.pat, None);
                if let Some((_, guard)) = &arm.guard {
                    walk.visit_expr(guard);
                }
                walk.visit_expr(&arm.body);
            });
        }

        fn visit_block(&mut self, block: &'ast syn::Block) {
            self.scoped(|walk| visit::visit_block(walk, block));
        }

        fn visit_local(&mut self, local: &'ast syn::Local) {
            if let Some(init) = &local.init {
                self.visit_expr(&init.expr);
                if let Some((_, diverge)) = &init.diverge {
                    self.visit_expr(diverge);
                }
            }
            match (&local.pat, &local.init) {
                (Pat::Ident(ident), Some(init))
                    if ident.subpat.is_none() && init.diverge.is_none() =>
                {
                    if let Some(clock) = read_of(&init.expr) {
                        self.origins.push(Origin {
                            at: offset(&*init.expr),
                            kinds: Kinds::of(clock),
                            other: 0,
                        });
                        let held = Name {
                            kinds: Some(Kinds::of(clock)),
                            origin: Some(self.origins.len() - 1),
                        };
                        self.bind(ident.ident.to_string(), held);
                    } else {
                        self.bind(ident.ident.to_string(), Name::default());
                    }
                }
                (pattern, _) => self.bind_pattern(pattern, None),
            }
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            match arguments(mac) {
                Some(args) => {
                    for arg in &args {
                        self.visit_expr(arg);
                    }
                }
                None => {
                    let mut names = Vec::new();
                    names_in(mac.tokens.clone(), &mut names);
                    if self.current().is_handed()
                        && names.iter().any(|name| {
                            name == "Instant" || name == "SystemTime" || name == "elapsed"
                        })
                        && names.iter().any(|name| name == "now" || name == "elapsed")
                    {
                        self.refusals.push((
                            offset(mac),
                            "macro arguments that do not parse name a clock reading in a body \
                             handed a clock"
                                .to_owned(),
                        ));
                    }
                }
            }
        }

        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            let scope = Scope {
                names: self.names,
                generics: ClockParameters::new(),
                self_how: None,
            };
            let signed = signature_handed(&item.sig, &scope, &mut self.refusals);
            // A nested `fn` captures nothing: its names are its own.
            let outer = std::mem::replace(&mut self.scopes, vec![BTreeMap::new()]);
            for input in &item.sig.inputs {
                if let FnArg::Typed(typed) = input {
                    self.bind_pattern(&typed.pat, Some(&typed.ty));
                }
            }
            self.handed.push(Hand {
                kinds: signed.kinds,
                unknown: false,
            });
            self.generics.push(signed.generics);
            self.visit_block(&item.block);
            self.generics.pop();
            self.handed.pop();
            self.scopes = outer;
        }
    }

    /// The handed-now reading of one function (see the guard's grammar).
    pub fn handed_now(signature: &syn::Signature, block: &syn::Block, scope: &Scope<'_>) -> Handed {
        let mut refusals = Vec::new();
        let Signed {
            how,
            kinds,
            generics,
        } = signature_handed(signature, scope, &mut refusals);
        let mut walk = HandedWalk {
            names: scope.names,
            handed: vec![Hand {
                kinds,
                unknown: false,
            }],
            generics: vec![generics],
            scopes: vec![BTreeMap::new()],
            readings: Vec::new(),
            elapsed: Vec::new(),
            origins: Vec::new(),
            refusals,
        };
        for input in &signature.inputs {
            if let FnArg::Typed(typed) = input {
                walk.bind_pattern(&typed.pat, Some(&typed.ty));
            }
        }
        walk.visit_block(block);
        let admitted: BTreeSet<usize> = (0..walk.origins.len())
            .filter(|origin| walk.origins[*origin].other == 0)
            .collect();
        let admitted_at: BTreeSet<Offset> = admitted
            .iter()
            .map(|origin| walk.origins[*origin].at)
            .collect();
        let mut out = Handed {
            how,
            kinds,
            again: Vec::new(),
            refusals: walk.refusals,
        };
        let measured = walk
            .elapsed
            .into_iter()
            .filter(|(.., origin)| origin.is_none_or(|origin| !admitted.contains(&origin)))
            .map(|(at, hand, read, _)| (at, hand, read));
        let readings: Vec<(Offset, Hand, Kinds)> = walk
            .readings
            .into_iter()
            .filter(|(at, ..)| !admitted_at.contains(at))
            .chain(measured)
            .collect();
        for (at, hand, read) in readings {
            if hand.kinds.meets(read) {
                out.again.push(at);
            } else if hand.unknown {
                out.refusals.push((
                    at,
                    "a closure handed a `now` whose type is not written reads the clock".to_owned(),
                ));
            }
        }
        out.again.sort_unstable();
        out
    }
}
