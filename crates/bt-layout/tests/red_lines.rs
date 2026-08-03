//! Structural red lines, guarded the way the spec says to guard them.
//!
//! D3 and L7 cannot be caught by asserting on output. There is exactly one
//! allocation path in this crate, so a float in it agrees with itself perfectly
//! — the drift D3 fears appears only when a *second* path exists, which is the
//! thing discipline ① forbids in the first place. Both red lines are therefore
//! guarded at the source level, which is the mechanism the spec names for L7
//! ("CI can grep their `use`") applied to its sibling.

use std::fs;
use std::path::PathBuf;

fn sources() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out: Vec<(String, String)> = fs::read_dir(&dir)
        .expect("the crate has a src directory")
        .filter_map(|entry| {
            let path = entry.expect("readable entry").path();
            (path.extension()?.to_str()? == "rs").then(|| {
                (
                    path.file_name()
                        .expect("a file")
                        .to_string_lossy()
                        .into_owned(),
                    fs::read_to_string(&path).expect("readable source"),
                )
            })
        })
        .collect();
    out.sort();
    assert!(out.len() >= 7, "the scan must actually see the crate");
    out
}

/// Lines that carry code, with block-comment bodies and `///` docs removed.
fn code_lines(source: &str) -> Vec<(usize, String)> {
    let mut in_block = false;
    let mut out = Vec::new();
    for (n, raw) in source.lines().enumerate() {
        let line = raw.trim();
        if in_block {
            if let Some(rest) = line.split_once("*/") {
                in_block = false;
                out.push((n + 1, rest.1.to_string()));
            }
            continue;
        }
        if line.starts_with("//") {
            continue;
        }
        if let Some((before, _)) = line.split_once("/*") {
            in_block = !line.contains("*/");
            out.push((n + 1, before.to_string()));
            continue;
        }
        let code = line.split_once("//").map_or(line, |(before, _)| before);
        out.push((n + 1, code.to_string()));
    }
    out
}

/// D3: fixed point, never floating point.
///
/// D1 wants bit-identical output, and `avail * ratio` in floating point can
/// differ by one ULP between two builds or two code paths, which after rounding
/// becomes a whole physical pixel of difference — the "two geometries always
/// drift" failure stated in the small.
#[test]
fn the_solver_uses_no_floating_point() {
    let mut found = Vec::new();
    for (name, source) in sources() {
        for (line_no, line) in code_lines(&source) {
            for needle in ["f32", "f64"] {
                if line.contains(needle) {
                    found.push(format!("{name}:{line_no}: {}", line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "red line D3: floating point on a solve path:\n{}",
        found.join("\n")
    );
}

/// L7: `bt-layout` depends on nothing, and on `bt-viewport` / `bt-doc` /
/// `bt-term` / `bt-render` least of all.
///
/// The judgement in one sentence: the solver answers what shape a tree unfolds
/// into, never what is drawn inside that shape. It therefore does not know cell
/// sizes, cols/rows, scroll anchors, height trees or layout keys.
#[test]
fn the_solver_depends_on_no_other_crate() {
    let mut found = Vec::new();
    for (name, source) in sources() {
        for (line_no, line) in code_lines(&source) {
            if line.contains("use bt_") || line.contains("bt_viewport") || line.contains("bt_doc") {
                found.push(format!("{name}:{line_no}: {}", line.trim()));
            }
        }
    }
    assert!(found.is_empty(), "red line L7: {}", found.join("\n"));

    // And nothing at all in the manifest, so the graph cannot grow one quietly.
    let manifest = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("readable manifest");
    let deps = manifest
        .split_once("[dependencies]")
        .expect("the manifest states its dependency section explicitly")
        .1;
    let declared: Vec<&str> = deps
        .lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with('['))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    assert!(
        declared.is_empty(),
        "bt-layout grew a dependency: {declared:?}"
    );
}

/// L8: no hash container's iteration order may reach a geometric decision.
///
/// A traversal order that happens to be stable is only an order your samples
/// have not falsified yet — the same "no heuristics" rule that governs the VT
/// layer, applied to geometry.
#[test]
fn geometry_never_depends_on_hash_iteration_order() {
    let mut found = Vec::new();
    for (name, source) in sources() {
        for (line_no, line) in code_lines(&source) {
            if line.contains("HashMap") || line.contains("HashSet") {
                found.push(format!("{name}:{line_no}: {}", line.trim()));
            }
        }
    }
    assert!(found.is_empty(), "red line L8: {}", found.join("\n"));
}
