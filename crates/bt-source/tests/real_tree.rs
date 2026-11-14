//! **The guards that read the real tree, in one process** (0.4.9 plan row 166).
//!
//! [`bt_source::Index::shared`] and [`bt_source::Index::of_package`] hold one index per
//! universe per *process*, and every `tests/*.rs` file is a process of its own: while each of
//! these guards was its own binary, each lowered the workspace again — 21 binaries, about
//! 400 s of the fast Linux runner's bt-source step, most of it the same lowering repeated
//! (row 165). Here they are modules of one binary, so each universe is lowered once and every
//! guard that asks for it is handed the same index: the `crate_sources` of a package through
//! `of_package`, and the whole of a first-party package through
//! [`bt_source::universes::first_party_whole`].
//!
//! **Each file stays where it was, and keeps its name in the output.** A module is reached by
//! `#[path]`, so `file!()` — what every `needle!` site resolves by — and every list keyed by a
//! guard's file are what they were; the module path is the old binary's name, so libtest prints
//! `census::the_committed_runtime_dep_map_is_the_code` and a filter names a guard as
//! `--test real_tree -- --exact census::<name>`.
//!
//! **What is not here, and why.** `measure` lowers bt-app cold, in a process that has lowered
//! nothing, and holds one test on purpose; `real_workspace` and `universes` read the tree but
//! lower nothing, so they have nothing to share; `dep_map`, `function_paths`, `ownership`,
//! `rejections` and `views` read fixtures only. Each is its own `[[test]]` in the manifest,
//! which says `autotests = false`: a file under `tests/` that is neither a `[[test]]` nor a
//! module below is compiled by nothing, and `real_workspace`'s unreached check names it.
//!
//! **What the guards share is the index cache and nothing else.** No module reads another's
//! items — the one test below reads the whole-package guards' declarations and readers, to hold
//! them to the one shared index; the process-globals a module keeps (`census`'s census,
//! `tripwire`'s code) are its own and are rows of `docs/plans/TEST-GLOBALS.tsv`.

#[path = "call_graph.rs"]
mod call_graph;
#[path = "census.rs"]
mod census;
#[path = "chains.rs"]
mod chains;
#[path = "clock_guard.rs"]
mod clock_guard;
#[path = "contained_panic.rs"]
mod contained_panic;
#[path = "contract.rs"]
mod contract;
#[path = "early_returns.rs"]
mod early_returns;
#[path = "env_doors.rs"]
mod env_doors;
#[path = "hardcode_owners.rs"]
mod hardcode_owners;
#[path = "index.rs"]
mod index;
#[path = "lane_drains.rs"]
mod lane_drains;
#[path = "native_labels.rs"]
mod native_labels;
#[path = "request_doors.rs"]
mod request_doors;
#[path = "root_topic_paths.rs"]
mod root_topic_paths;
#[path = "runtime_voice.rs"]
mod runtime_voice;
#[path = "temp_paths.rs"]
mod temp_paths;
#[path = "test_globals.rs"]
mod test_globals;
#[path = "test_order.rs"]
mod test_order;
#[path = "test_shells.rs"]
mod test_shells;
#[path = "timing.rs"]
mod timing;
#[path = "tripwire.rs"]
mod tripwire;
#[path = "window_seam.rs"]
mod window_seam;

use std::path::Path;
use std::sync::Arc;

use bt_source::{Index, Package, Vendor, Workspace, is_vendored, universes};

/// RED (plan row 166) — **the guards that read every first-party package whole are handed one
/// index per package, the one this process holds**, and never lower one of their own.
///
/// Two claims, each on every first-party package. Each guard's declaration is the one
/// declaration, `universes::first_party_whole`, by value — the cache is keyed by the value, so
/// two declarations that drifted apart would be two lowerings of nearly the same files. And each
/// index a guard's reader hands out is the very object `Index::shared` holds for its universe.
///
/// MUTATION: lower in one of the readers with `Arc::new(Index::build(…))` instead of
/// `Index::shared(…)` and the second assertion goes red naming that guard; declare
/// `whole_package` in one guard's `universe_for` and the first one does, naming the guard and
/// the package.
#[test]
fn the_whole_package_guards_share_one_index_per_package() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let workspace = Workspace::read(&root).expect("this workspace");
    let first_party: Vec<&Package> = workspace
        .packages()
        .iter()
        .filter(|package| !is_vendored(package.directory()))
        .collect();
    for package in &first_party {
        let declared = universes::first_party_whole(package, Vendor::Excluded)
            .expect("a package of this workspace");
        let declarations = [
            ("early_returns", early_returns::universe_for(package)),
            ("env_doors", env_doors::universe_for(package)),
            ("temp_paths", temp_paths::universe_for(package)),
            ("test_globals", test_globals::universe_for(package)),
            ("test_order", test_order::universe_for(package)),
            ("test_shells", test_shells::universe_for(package)),
            ("timing", timing::universe_for(package)),
            ("tripwire", tripwire::package_universe(package)),
        ];
        for (guard, universe) in declarations {
            assert_eq!(
                universe,
                declared,
                "{guard} declares {} in a way of its own",
                package.name()
            );
        }
    }

    let readers: [(&str, Vec<Arc<Index>>, usize); 8] = [
        (
            "early_returns",
            early_returns::workspace_indices(),
            first_party.len(),
        ),
        (
            "env_doors",
            env_doors::workspace_indices(),
            first_party.len(),
        ),
        (
            "temp_paths",
            temp_paths::workspace_indices(),
            first_party.len() - 1,
        ),
        (
            "test_globals",
            test_globals::workspace()
                .0
                .into_iter()
                .map(|(_, index)| index)
                .collect(),
            first_party.len(),
        ),
        (
            "test_order",
            test_order::workspace_indices()
                .into_iter()
                .map(|(_, index)| index)
                .collect(),
            first_party.len() - 1,
        ),
        (
            "test_shells",
            test_shells::workspace_indices()
                .into_iter()
                .map(|(_, index)| index)
                .collect(),
            first_party.len(),
        ),
        (
            "timing",
            timing::workspace_indices()
                .into_iter()
                .map(|(_, index)| index)
                .collect(),
            first_party.len(),
        ),
        (
            "tripwire",
            tripwire::first_party_indices(&root)
                .into_iter()
                .map(|(_, index)| index)
                .collect(),
            first_party.len(),
        ),
    ];
    for (guard, indices, expected) in readers {
        assert_eq!(
            indices.len(),
            expected,
            "{guard} reads a different set of packages"
        );
        for index in indices {
            let held = Index::shared(index.universe())
                .unwrap_or_else(|rejections| panic!("{}", bt_source::report(&rejections)));
            assert!(
                Arc::ptr_eq(&index, &held),
                "{guard} lowered `{}` itself instead of taking the one this process holds",
                index.universe().name()
            );
        }
    }
}
