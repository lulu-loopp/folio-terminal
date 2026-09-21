//! **The measurement of plan §5, and it decides.**
//!
//! P1b runs before any reader is migrated so that nothing afterwards depends on
//! an unmeasured assumption. Four numbers, four budgets:
//!
//! | Reported | Budget |
//! | --- | --- |
//! | wall time to build the index, cold, single process | ≤ 20 s |
//! | the index's steady-state size, after the parser objects are dropped | ≤ 600 MB |
//! | wall time of a representative query set | ≤ 200 ms |
//! | build cost × the harness processes CI runs | reported, with the peak-memory product stated |
//!
//! **Exceeding a budget is a finding, not a number to adjust** (§6.0 rule 5).
//! The fallbacks §5 names — serialising the lowered index under `target/` keyed
//! by content hash, narrowing the default universe from the union to the queried
//! crate — are the coordinator's to choose after reading these numbers, and
//! neither is adopted in advance.
//!
//! **What is asserted, and what is only reported.** The memory budget is
//! asserted on every run: it is a property of the tree and the lowering, the
//! same on an idle machine and a loaded one. The two wall-clock budgets are
//! asserted only when `BT_SOURCE_MEASURE_BUDGETS` is set, because this suite
//! shares a laptop with two other compile lanes and a CI runner with whatever
//! else that runner is doing, and a timing assertion there would go red for a
//! reason that is not about this code — which is the one thing a guard must
//! never do. Unconditionally, the test asserts that all four numbers were
//! produced and printed; a measurement that silently measured nothing is the
//! failure mode worth guarding.
//!
//! **This file holds one test on purpose.** It is the heavy operation CONVENTIONS
//! §八 is about — a 23 MB parse — and a second test in the same binary would run
//! beside it and make both numbers about the pair.
//!
//! **"Cold" here means a cold process, not a cold disk.** The lowering has not
//! been done in this process and `Index::build` does not look in the cache, so
//! the number is the whole of the parse and the lowering. It is not the whole of
//! the I/O: `tests/index.rs` and `tests/real_workspace.rs` read the same 124
//! files earlier in the same `cargo test` run, so the page cache is warm by the
//! time this one asks. On a fresh runner reading 23 MB off a disk for the first
//! time the build is longer by that read, and the budget has fourteen seconds of
//! room for it.

use std::path::Path;
use std::time::{Duration, Instant};

use bt_source::{Index, ItemQuery, Vendor, View, Workspace, report, universes};

/// §5's three budgets.
const BUILD_BUDGET: Duration = Duration::from_secs(20);
const MEMORY_BUDGET: usize = 600 * 1024 * 1024;
const QUERY_BUDGET: Duration = Duration::from_millis(200);

/// Set this to hold the two wall-clock budgets as assertions rather than as
/// reported numbers.
const BUDGETS_ARE_ASSERTED: &str = "BT_SOURCE_MEASURE_BUDGETS";

/// RED — the four numbers of §5, measured on the real `bt-app`.
///
/// MUTATION: lower each file twice and the build row doubles; keep the `syn`
/// trees alive in the index and `tests/index.rs` stops compiling before this
/// test can report anything about them.
#[test]
fn the_index_costs_what_the_plan_budgeted() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let workspace = Workspace::read(&root).expect("this workspace");
    let package = workspace.package("bt-app").expect("bt-app");
    let universe = universes::crate_sources(package, Vendor::Excluded).expect("bt-app's own src");

    // Cold in the sense the module doc gives: this process has never lowered
    // anything and `build` does not look in the cache. The page cache is warm.
    let started = Instant::now();
    let index =
        Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
    let build = started.elapsed();
    let footprint = index.footprint_bytes();

    let queries = Instant::now();
    let body = index
        .body_of(&ItemQuery::function("main"))
        .expect("`fn main` is one declaration of bt-app");
    let named = index.count_identifier("Runtime", View::Identifiers);
    let absent = index.contains("no_byte_of_bt_app_spells_this_needle", View::Raw);
    let owners = index
        .owners_of(&ItemQuery::function("main"))
        .expect("`fn main` is owned by the crate root");
    let query_set = queries.elapsed();

    assert!(
        body.starts_with('{'),
        "a body is its braces and what is in them"
    );
    assert!(named > 0, "`Runtime` is named somewhere in bt-app");
    assert!(!absent, "the negative is a negative");
    assert_eq!(
        owners
            .iter()
            .map(|owner| owner.module_path.as_str())
            .collect::<Vec<_>>(),
        ["crate"]
    );

    let harnesses = harness_processes(&workspace, &root);
    let widest = harnesses
        .iter()
        .map(|invocation| invocation.processes)
        .max()
        .expect("ci.yml runs `cargo test` at least once");
    let across_ci: usize = harnesses
        .iter()
        .map(|invocation| invocation.processes)
        .sum();

    let mut rows = String::new();
    rows.push_str(&format!(
        "build (cold, single process): {build:?}  [budget {BUILD_BUDGET:?}] {}\n",
        verdict(build <= BUILD_BUDGET)
    ));
    rows.push_str(&format!(
        "index size (owned-allocation accounting, parser objects dropped): {:.1} MB  \
         [budget {} MB] {}\n",
        megabytes(footprint),
        MEMORY_BUDGET / 1024 / 1024,
        verdict(footprint <= MEMORY_BUDGET)
    ));
    rows.push_str(&format!(
        "query set (body · identifier count · whole-source negative · owner set): {query_set:?}  \
         [budget {QUERY_BUDGET:?}] {}\n",
        verdict(query_set <= QUERY_BUDGET)
    ));
    rows.push_str(&format!(
        "CI product: {widest} harness processes in the widest `cargo test` of ci.yml, \
         {across_ci} across the whole workflow\n"
    ));
    rows.push_str(&format!(
        "  CPU:  {widest} x {build:?} = {:?} of lowering per CI job, if every harness asks\n",
        build * u32::try_from(widest).unwrap_or(u32::MAX)
    ));
    rows.push_str(&format!(
        "  peak: {:.1} MB one at a time — `cargo test` runs its test binaries in sequence, so \
         the indexes of two harnesses are never alive together; {:.1} MB is the ceiling if they \
         ever overlap\n",
        megabytes(footprint),
        megabytes(footprint * widest)
    ));
    // `widest` is the ceiling the migration walks towards, not the bill today.
    // What asks now is this crate's own harnesses and nothing else, because P1b
    // wires no consumer at all.
    let asking_today = workspace
        .package("bt-source")
        .expect("this crate")
        .targets()
        .len();
    rows.push_str(&format!(
        "  today: {asking_today} of those {widest} hold a reader that asks, so the bill now is \
         {:?}; the {widest} is what it becomes if every harness in the workspace ends up asking\n",
        build * u32::try_from(asking_today).unwrap_or(u32::MAX)
    ));

    println!("\n── the index, and what it costs ──");
    println!(
        "universe: {} — {} files, {} bytes, {} items, {} identifier tokens, {} literals, \
         {} comment masks",
        universe.name(),
        index.files().len(),
        index.union().len(),
        index.items().len(),
        index.tokens().len(),
        index.literals().len(),
        index.comments().len()
    );
    print!("{rows}");
    for invocation in &harnesses {
        println!(
            "  ci.yml: {} harness processes for `{}`",
            invocation.processes, invocation.command
        );
    }
    println!(
        "\nthe index's size is counted, not sampled: it is the sum of every allocation the index \
         owns — the union, each vector's capacity, each string inside them. It is NOT the \
         process's resident set: allocator bookkeeping and fragmentation are outside it, and so \
         is everything the harness around it holds. Measuring RSS instead would need either a \
         Win32 call, which `scripts/check-portable-core.ps1` forbids this crate, or an `unsafe` \
         global allocator, which the workspace's `unsafe_code = \"deny\"` forbids without a \
         deviation the owner rules on."
    );

    for label in ["build (cold", "index size", "query set", "CI product"] {
        assert!(
            rows.contains(label),
            "the measurement did not report `{label}`"
        );
    }
    assert!(build > Duration::ZERO && footprint > 0 && !harnesses.is_empty());

    // Not time-sensitive: held on every run.
    assert!(
        footprint <= MEMORY_BUDGET,
        "the index is {:.1} MB, over §5's {} MB budget — a finding, and the design changes",
        megabytes(footprint),
        MEMORY_BUDGET / 1024 / 1024
    );
    if std::env::var_os(BUDGETS_ARE_ASSERTED).is_some() {
        assert!(build <= BUILD_BUDGET, "over §5's build budget:\n{rows}");
        assert!(query_set <= QUERY_BUDGET, "over §5's query budget:\n{rows}");
    }
}

fn megabytes(bytes: usize) -> f64 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a byte count printed to one decimal place of a megabyte"
    )]
    let value = bytes as f64;
    value / 1024.0 / 1024.0
}

fn verdict(within: bool) -> &'static str {
    if within { "PASS" } else { "FAIL" }
}

/// One `cargo test` invocation of the workflow, and how many test binaries it
/// starts.
struct Invocation {
    command: String,
    processes: usize,
}

/// **How many harness processes CI actually runs**, read out of the workflow
/// rather than assumed.
///
/// **The counting rule, in one sentence:** one process per compiled test target
/// an invocation runs — a package's library, each of its binaries, each of its
/// integration tests — summed over the packages it selects, or, when it names
/// targets with `--lib`/`--bin`/`--test`, just those.
///
/// Each of those targets is its own executable and each would build its own
/// index, which is the multiplier §5's last row is about. Doc tests are not
/// counted: a doc test is not where a source guard lives. An invocation carrying
/// `--no-run` compiles and starts nothing, so it is not here either.
fn harness_processes(workspace: &Workspace, root: &Path) -> Vec<Invocation> {
    let yaml = std::fs::read_to_string(root.join(".github").join("workflows").join("ci.yml"))
        .expect("the workflow is in the tree");
    commands(&yaml)
        .into_iter()
        .filter(|command| !command.contains("--no-run"))
        .map(|command| {
            let words: Vec<&str> = command.split_whitespace().collect();
            let named_targets = argument_values(&words, &["--test", "--bin"]).len()
                + usize::from(words.contains(&"--lib"));
            let selected: Vec<&str> = argument_values(&words, &["-p", "--package"]);
            let excluded: Vec<&str> = argument_values(&words, &["--exclude"]);
            let packages = workspace.packages().iter().filter(|package| {
                if words.contains(&"--workspace") {
                    !excluded.contains(&package.name())
                } else {
                    selected.contains(&package.name())
                }
            });
            let processes = if named_targets > 0 {
                // `--test measure` runs one binary, however many the package has.
                named_targets * packages.count().max(1)
            } else {
                packages.map(|package| package.targets().len()).sum()
            };
            Invocation { command, processes }
        })
        .collect()
}

/// Every `cargo test …` in the workflow, folded back into one line each.
///
/// A folded YAML scalar (`run: >`) continues on every following line indented
/// under it, and the continuation lines of this workflow's `cargo test` blocks
/// are argument lists. A line that opens a new list item or a new key ends the
/// command; `-p` is not a list item, because a YAML list item is a dash and a
/// space.
///
/// A comment is not a command. This workflow's comments talk about
/// `cargo test` in prose, and a reading that counted those would count a
/// sentence as a job — the same defect, one level up, that §6.6 of the plan is
/// about.
fn commands(yaml: &str) -> Vec<String> {
    let lines: Vec<&str> = yaml.lines().collect();
    let mut found = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        let Some(start) = line.find("cargo test") else {
            continue;
        };
        let mut command = line[start..].to_owned();
        for next in &lines[at + 1..] {
            let trimmed = next.trim();
            if trimmed.is_empty()
                || trimmed.starts_with("- ")
                || trimmed.starts_with('#')
                || opens_a_key(trimmed)
            {
                break;
            }
            command.push(' ');
            command.push_str(trimmed);
        }
        found.push(command);
    }
    found
}

/// `name:`, `run:`, `shell:` — a key opens a new scalar and ends the one before.
fn opens_a_key(line: &str) -> bool {
    let Some(colon) = line.find(':') else {
        return false;
    };
    !line[..colon].is_empty()
        && line[..colon]
            .chars()
            .all(|character| character.is_ascii_lowercase() || character == '-')
        && line[colon..].starts_with(": ")
}

/// The values of every `--flag value` pair in `words`.
fn argument_values<'a>(words: &[&'a str], flags: &[&str]) -> Vec<&'a str> {
    words
        .windows(2)
        .filter(|pair| flags.contains(&pair[0]))
        .map(|pair| pair[1])
        .collect()
}
