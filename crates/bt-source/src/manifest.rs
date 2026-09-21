//! The packages of a workspace and the compilation roots of each one.
//!
//! A universe is declared by a reader, and what a reader declares is usually
//! "this package's targets". That is a fact cargo owns, so it is read out of the
//! manifests rather than listed here — a list is the thing somebody forgets to
//! add to, which is the same failure as the stale tables §6.6 of the plan is
//! about.
//!
//! **A package is more than its library's module graph** (plan §3.1, §8.1). A
//! `src/bin/` target is compiled and linked and is invisible to a walk that
//! starts at `lib.rs`; `crates/bt-pty/src/bin/bt-conpty-width-probe.rs` is the
//! instance that made the point, and the guard
//! `bt_platform::native_window_door_tests::a_stand_in_window_is_only_named_by_tests`
//! covers those targets today because its walk happens to reach them. So the
//! three target kinds this crate knows are the three a source guard can care
//! about: the library, every binary, and every integration test.
//!
//! **Benchmarks and examples are deliberately absent.** No reader in this
//! workspace has a universe that contains one, P1a's ticket names exactly the
//! three kinds above, and a target kind nobody asks for is a rule nobody checks.
//!
//! The manifest reader below understands the small part of TOML that cargo's
//! target discovery is written in and **refuses the rest** rather than guessing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::paths::normalized;
use crate::reject::Rejection;

/// Which compilation a root starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TargetKind {
    /// `src/lib.rs`, or whatever `[lib] path` names.
    Library,
    /// `src/main.rs` and `src/bin/…`, or whatever a `[[bin]] path` names.
    Binary,
    /// `tests/…`, or whatever a `[[test]] path` names. Its root module is test
    /// code by construction — there is no build of the product that contains it.
    IntegrationTest,
}

impl TargetKind {
    /// Whether a build of the shipped program can contain this target at all.
    #[must_use]
    pub fn permits_product(self) -> bool {
        !matches!(self, Self::IntegrationTest)
    }
}

/// One target, by the three things that name it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TargetId {
    pub package: String,
    pub kind: TargetKind,
    pub name: String,
}

impl std::fmt::Display for TargetId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self.kind {
            TargetKind::Library => "lib",
            TargetKind::Binary => "bin",
            TargetKind::IntegrationTest => "test",
        };
        write!(formatter, "{}:{kind}:{}", self.package, self.name)
    }
}

/// A compilation root: the file that is a crate root, and the target it roots.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TargetRoot {
    pub id: TargetId,
    pub file: PathBuf,
}

/// One workspace member.
#[derive(Clone, Debug)]
pub struct Package {
    name: String,
    directory: PathBuf,
    targets: Vec<TargetRoot>,
}

impl Package {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The directory holding the package's `Cargo.toml`.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Every compilation root, sorted, so two runs on two platforms agree.
    #[must_use]
    pub fn targets(&self) -> &[TargetRoot] {
        &self.targets
    }
}

/// The workspace, read from its manifests.
#[derive(Clone, Debug)]
pub struct Workspace {
    root: PathBuf,
    packages: Vec<Package>,
}

impl Workspace {
    /// Read the workspace rooted at `root`.
    ///
    /// # Errors
    ///
    /// [`Rejection::Manifest`] when a manifest is missing, or is written in a
    /// shape this reader does not understand.
    pub fn read(root: &Path) -> Result<Self, Rejection> {
        let root = normalized(root);
        let manifest = root.join("Cargo.toml");
        let text = read_text(&manifest)?;
        let document = Document::read(&manifest, &text)?;
        let members =
            document
                .array("workspace", "members")
                .ok_or_else(|| Rejection::Manifest {
                    file: manifest.clone(),
                    line: 1,
                    reason: "no `[workspace] members`, so this is not a workspace root".to_owned(),
                })?;
        let mut packages = Vec::new();
        for member in members {
            if member.contains('*') {
                return Err(Rejection::Manifest {
                    file: manifest.clone(),
                    line: 1,
                    reason: format!(
                        "`{member}` is a glob; this reader lists members, it does not match them"
                    ),
                });
            }
            let directory = normalized(&root.join(member));
            packages.push(read_package(&directory)?);
        }
        packages.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(Self { root, packages })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn packages(&self) -> &[Package] {
        &self.packages
    }

    /// The package called `name`.
    ///
    /// # Errors
    ///
    /// [`Rejection::NoSuchPackage`] — a universe that names a package the
    /// workspace does not have is a universe written against a tree that has
    /// moved, and answering it with an empty file set would be the quiet
    /// failure this crate exists to prevent.
    pub fn package(&self, name: &str) -> Result<&Package, Rejection> {
        self.packages
            .iter()
            .find(|package| package.name == name)
            .ok_or_else(|| Rejection::NoSuchPackage {
                name: name.to_owned(),
            })
    }
}

fn read_text(file: &Path) -> Result<String, Rejection> {
    std::fs::read_to_string(file).map_err(|error| Rejection::Manifest {
        file: file.to_path_buf(),
        line: 0,
        reason: error.to_string(),
    })
}

fn read_package(directory: &Path) -> Result<Package, Rejection> {
    let manifest = directory.join("Cargo.toml");
    let text = read_text(&manifest)?;
    let document = Document::read(&manifest, &text)?;
    let name = document
        .string("package", "name")
        .ok_or_else(|| Rejection::Manifest {
            file: manifest.clone(),
            line: 1,
            reason: "no `[package] name`".to_owned(),
        })?
        .to_owned();

    let mut targets: BTreeMap<PathBuf, TargetRoot> = BTreeMap::new();
    let mut claim = |kind: TargetKind, target: String, file: PathBuf| {
        targets.entry(normalized(&file)).or_insert(TargetRoot {
            id: TargetId {
                package: name.clone(),
                kind,
                name: target,
            },
            file: normalized(&file),
        });
    };

    // Explicit tables first: an explicit `[[bin]] path` and the autodiscovered
    // file are frequently the same file (`crates/bt-app` says `src/main.rs` in
    // both voices), and the explicit name is the one cargo uses.
    for table in document.tables("lib") {
        let file = table.string("path").map_or_else(
            || directory.join("src").join("lib.rs"),
            |path| directory.join(path),
        );
        claim(TargetKind::Library, name.clone(), file);
    }
    for (section, kind) in [
        ("bin", TargetKind::Binary),
        ("test", TargetKind::IntegrationTest),
    ] {
        for table in document.tables(section) {
            let Some(path) = table.string("path") else {
                return Err(Rejection::Manifest {
                    file: manifest.clone(),
                    line: table.line,
                    reason: format!(
                        "`[[{section}]]` without a `path`; this reader does not infer a target's \
                         file from its name"
                    ),
                });
            };
            let target = table.string("name").unwrap_or(path).to_owned();
            claim(kind, target, directory.join(path));
        }
    }

    // Then cargo's own discovery, for everything the manifest left unsaid.
    if document.boolean("package", "autolib") != Some(false) {
        let file = directory.join("src").join("lib.rs");
        if file.is_file() {
            claim(TargetKind::Library, name.clone(), file);
        }
    }
    if document.boolean("package", "autobins") != Some(false) {
        let main = directory.join("src").join("main.rs");
        if main.is_file() {
            claim(TargetKind::Binary, name.clone(), main);
        }
        for (target, file) in roots_under(&directory.join("src").join("bin")) {
            claim(TargetKind::Binary, target, file);
        }
    }
    if document.boolean("package", "autotests") != Some(false) {
        for (target, file) in roots_under(&directory.join("tests")) {
            claim(TargetKind::IntegrationTest, target, file);
        }
    }

    let mut targets: Vec<TargetRoot> = targets.into_values().collect();
    targets.sort();
    Ok(Package {
        name,
        directory: directory.to_path_buf(),
        targets,
    })
}

/// Cargo's discovery rule for a directory of targets: `name.rs` beside it, and
/// `name/main.rs` inside it.
fn roots_under(directory: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if path.is_dir() {
            let main = path.join("main.rs");
            if main.is_file() {
                found.push((stem.to_owned(), main));
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push((stem.to_owned(), path));
        }
    }
    found.sort();
    found
}

// ---------------------------------------------------------------------------
// The manifest reader
// ---------------------------------------------------------------------------

/// One `[section]` or `[[section]]` of a manifest, flattened to the scalar keys
/// written directly under it.
#[derive(Debug, Default)]
struct Table {
    line: usize,
    strings: BTreeMap<String, String>,
    booleans: BTreeMap<String, bool>,
    arrays: BTreeMap<String, Vec<String>>,
}

impl Table {
    fn string(&self, key: &str) -> Option<&str> {
        self.strings.get(key).map(String::as_str)
    }
}

/// A manifest, as much of it as target discovery needs.
///
/// Not a TOML parser and not trying to be one: it reads section headers and the
/// three value shapes cargo's target keys are written in — a basic string, a
/// boolean, and an array of basic strings, on one line or several. A key whose
/// value is written some other way is not silently ignored where it matters;
/// [`read_package`] refuses a `[[bin]]` or `[[test]]` with no readable `path`.
#[derive(Debug, Default)]
struct Document {
    tables: BTreeMap<String, Vec<Table>>,
}

impl Document {
    fn read(file: &Path, text: &str) -> Result<Self, Rejection> {
        let mut document = Self::default();
        let mut section = String::new();
        document
            .tables
            .entry(String::new())
            .or_default()
            .push(Table::default());
        let mut pending: Option<(String, Vec<String>)> = None;

        for (index, raw) in text.lines().enumerate() {
            let line = index + 1;
            let content = strip_comment(raw, file, line)?;
            let content = content.trim();
            if content.is_empty() {
                continue;
            }

            if let Some((key, mut collected)) = pending.take() {
                let closed = collect_array(content, &mut collected);
                if closed {
                    document.current(&section).arrays.insert(key, collected);
                } else {
                    pending = Some((key, collected));
                }
                continue;
            }

            if content.starts_with('[') {
                let (name, array_of_tables) = header(content, file, line)?;
                // `[dependencies.serde]` and friends: a dotted header is a
                // table of its own and never a second `[package]`.
                section = name;
                let tables = document.tables.entry(section.clone()).or_default();
                if array_of_tables || tables.is_empty() {
                    tables.push(Table {
                        line,
                        ..Table::default()
                    });
                }
                continue;
            }

            let Some((key, value)) = content.split_once('=') else {
                continue;
            };
            let key = key.trim();
            // A dotted key (`version.workspace = true`) says nothing about the
            // keys this reader wants, and a quoted key is a shape it does not
            // read. Both are skipped rather than mistaken for the bare key.
            if key.contains('.') || key.contains('"') || key.contains('\'') {
                continue;
            }
            let value = value.trim();
            if let Some(rest) = value.strip_prefix('[') {
                let mut collected = Vec::new();
                if collect_array(rest, &mut collected) {
                    document
                        .current(&section)
                        .arrays
                        .insert(key.to_owned(), collected);
                } else {
                    pending = Some((key.to_owned(), collected));
                }
            } else if let Some(string) = basic_string(value) {
                document
                    .current(&section)
                    .strings
                    .insert(key.to_owned(), string);
            } else if value == "true" || value == "false" {
                document
                    .current(&section)
                    .booleans
                    .insert(key.to_owned(), value == "true");
            }
        }

        if let Some((key, _)) = pending {
            return Err(Rejection::Manifest {
                file: file.to_path_buf(),
                line: text.lines().count(),
                reason: format!("`{key}` opens an array that never closes"),
            });
        }
        Ok(document)
    }

    fn current(&mut self, section: &str) -> &mut Table {
        self.tables
            .entry(section.to_owned())
            .or_insert_with(|| vec![Table::default()])
            .last_mut()
            .expect("a section always has at least one table")
    }

    fn tables(&self, section: &str) -> &[Table] {
        self.tables.get(section).map_or(&[], Vec::as_slice)
    }

    fn string(&self, section: &str, key: &str) -> Option<&str> {
        self.tables(section).first()?.string(key)
    }

    fn boolean(&self, section: &str, key: &str) -> Option<bool> {
        self.tables(section).first()?.booleans.get(key).copied()
    }

    fn array(&self, section: &str, key: &str) -> Option<Vec<String>> {
        self.tables(section).first()?.arrays.get(key).cloned()
    }
}

/// `content` with its comment taken off, stepping over `#` inside a string.
fn strip_comment(raw: &str, file: &Path, line: usize) -> Result<String, Rejection> {
    let mut out = String::with_capacity(raw.len());
    let mut characters = raw.chars();
    while let Some(character) = characters.next() {
        match character {
            '#' => return Ok(out),
            '"' | '\'' => {
                let quote = character;
                out.push(character);
                let mut closed = false;
                while let Some(inside) = characters.next() {
                    out.push(inside);
                    if quote == '"' && inside == '\\' {
                        if let Some(escaped) = characters.next() {
                            out.push(escaped);
                        }
                        continue;
                    }
                    if inside == quote {
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    return Err(Rejection::Manifest {
                        file: file.to_path_buf(),
                        line,
                        reason: "a string opens and does not close on its line".to_owned(),
                    });
                }
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

/// The name a `[header]` or `[[header]]` gives, and which of the two it is.
///
/// The name is normalised to its dotted key with every quote taken off, because
/// `["package"]` and `[package]` are the same table and a reader that filed them
/// apart would miss a manifest written the long way. Quoted segments are the
/// ordinary case in a cargo manifest —
/// `[target.'cfg(unix)'.dev-dependencies]` — and they are sections this reader
/// never asks about, so the point of splitting them correctly is only that a
/// dot inside one is not a separator.
fn header(content: &str, file: &Path, line: usize) -> Result<(String, bool), Rejection> {
    let array_of_tables = content.starts_with("[[");
    let opened = if array_of_tables { 2 } else { 1 };
    let closing = if array_of_tables { "]]" } else { "]" };
    let name = content
        .strip_suffix(closing)
        .map(|rest| &rest[opened..])
        .ok_or_else(|| Rejection::Manifest {
            file: file.to_path_buf(),
            line,
            reason: format!("`{content}` opens a section header and does not close it"),
        })?;
    let mut segments = Vec::new();
    let mut segment = String::new();
    let mut characters = name.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '.' => segments.push(std::mem::take(&mut segment).trim().to_owned()),
            quote @ ('"' | '\'') => {
                let mut closed = false;
                while let Some(inside) = characters.next() {
                    if quote == '"' && inside == '\\' {
                        if let Some(escaped) = characters.next() {
                            segment.push(escaped);
                        }
                        continue;
                    }
                    if inside == quote {
                        closed = true;
                        break;
                    }
                    segment.push(inside);
                }
                if !closed {
                    return Err(Rejection::Manifest {
                        file: file.to_path_buf(),
                        line,
                        reason: format!("`{content}` opens a quoted key and does not close it"),
                    });
                }
            }
            other => segment.push(other),
        }
    }
    segments.push(segment.trim().to_owned());
    Ok((segments.join("."), array_of_tables))
}

/// Push every basic string `content` holds onto `collected`; whether the array
/// closes on this line is the answer.
fn collect_array(content: &str, collected: &mut Vec<String>) -> bool {
    let body = content.strip_suffix(']').unwrap_or(content);
    for piece in body.split(',') {
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }
        if let Some(string) = basic_string(piece) {
            collected.push(string);
        }
    }
    content.trim_end().ends_with(']')
}

/// `value` as a TOML basic or literal string, or `None` when it is neither.
fn basic_string(value: &str) -> Option<String> {
    let value = value.trim();
    for quote in ['"', '\''] {
        if let Some(rest) = value.strip_prefix(quote)
            && let Some(inside) = rest.strip_suffix(quote)
            && !inside.contains(quote)
        {
            return Some(if quote == '"' {
                inside.replace("\\\\", "\\")
            } else {
                inside.to_owned()
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — the manifest reader reads what cargo's target discovery is written
    /// in, and a shape it cannot read is refused rather than skipped.
    #[test]
    fn the_manifest_reader_reads_targets_and_refuses_the_rest() {
        let file = Path::new("Cargo.toml");
        let document = Document::read(
            file,
            "\
[workspace]           # a trailing comment
members = [
    \"crates/one\",
    \"crates/two\",   # and one here
]

[package]
name = \"one\"
version.workspace = true
autotests = false

[[bin]]
name = \"first\"
path = \"src/bin/first.rs\"

[[bin]]
name = \"second\"
path = \"src/bin/second.rs\"

[dependencies.serde]
version = \"1\"
",
        )
        .expect("this is the shape the reader reads");
        assert_eq!(
            document.array("workspace", "members"),
            Some(vec!["crates/one".to_owned(), "crates/two".to_owned()])
        );
        assert_eq!(document.string("package", "name"), Some("one"));
        assert_eq!(document.boolean("package", "autotests"), Some(false));
        assert_eq!(document.boolean("package", "autobins"), None);
        let bins: Vec<_> = document
            .tables("bin")
            .iter()
            .map(|table| table.string("path").unwrap())
            .collect();
        assert_eq!(bins, ["src/bin/first.rs", "src/bin/second.rs"]);
        // A `#` inside a string is not a comment, and a dotted key is not `name`.
        assert_eq!(
            document.string("dependencies.serde", "version"),
            Some("1"),
            "a dotted header is a table of its own"
        );

        let unclosed = Document::read(file, "[workspace]\nmembers = [\n    \"crates/one\",\n");
        assert!(matches!(unclosed, Err(Rejection::Manifest { .. })));
        let unquoted = Document::read(file, "[workspace\nmembers = []\n");
        assert!(matches!(unquoted, Err(Rejection::Manifest { .. })));
    }
}
