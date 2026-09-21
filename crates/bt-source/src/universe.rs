//! A universe is declared, not derived (plan §3.1).
//!
//! A module graph rooted at a crate root is not a universe any guard in this
//! tree uses. Four different ones are in use today, three of them in one file,
//! and they differ in ways that matter: one includes `src/bin/` targets, one
//! excludes directories named `bin`, one reaches into `vendor/`, one is a single
//! crate's `src/`. Replacing any of them with "the crate's module graph" would
//! change what it covers without anybody deciding to.
//!
//! So there is no default. A reader states four things and the compiler makes it
//! state them:
//!
//! * **target roots** — which compilations the declaration walk starts from;
//! * **disk scopes** — which directories are read as text whatever compiles them;
//! * **exclusions** — directory names a scope never descends into;
//! * **vendor** — in or out, said out loud.
//!
//! Exclusions live on the scope rather than on the universe, which is the one
//! place this differs from the plan's sketch: the three walkers that exclude
//! `bin`/`tests`/`target` apply the same exclusions to every root they walk, so
//! the two shapes agree on today's readers, and a universe that wanted two
//! scopes with different exclusions can say so.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use crate::manifest::{Package, TargetRoot};
use crate::paths::normalized;
use crate::reject::Rejection;

/// Whether a universe covers the vendored crates.
///
/// Stated rather than inferred. `vendor/` is upstream code held to upstream's
/// choices — the workspace lint table refuses to apply this project's lints to
/// it for exactly that reason — so whether a rule covers it is a decision, and a
/// universe that reaches in without one is rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Vendor {
    Excluded,
    Included,
}

/// A directory read as text, and what it will not descend into.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DiskScope {
    root: PathBuf,
    excluded_directories: Vec<String>,
    required_component: Option<String>,
}

impl DiskScope {
    /// Everything under `root`, recursively.
    #[must_use]
    pub fn under(root: impl Into<PathBuf>) -> Self {
        Self {
            root: normalized(&root.into()),
            excluded_directories: Vec::new(),
            required_component: None,
        }
    }

    /// Never descend into a directory with any of these names.
    ///
    /// The three walkers that carry `bin`, `tests` and `target` each say why in
    /// their own words: a development binary is a console program on purpose, an
    /// integration test runs under the harness, and `target/` is not source.
    ///
    /// The names are kept sorted, so two universes that exclude the same
    /// directories compare equal however each one wrote them down.
    #[must_use]
    pub fn excluding(mut self, names: &[&str]) -> Self {
        self.excluded_directories = names.iter().map(|name| (*name).to_owned()).collect();
        self.excluded_directories.sort();
        self
    }

    /// Keep only files with a path component of this name.
    ///
    /// `bt-platform`'s two workspace-wide walkers both walk `crates/` and then
    /// retain paths containing a `src` component, which is how they take in
    /// `src/bin/` while leaving `build.rs` and `tests/` out. It is a filter on
    /// the *result*, not a root, so it is written here rather than folded into
    /// one.
    #[must_use]
    pub fn retaining_component(mut self, component: &str) -> Self {
        self.required_component = Some(component.to_owned());
        self
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn excluded_directories(&self) -> &[String] {
        &self.excluded_directories
    }

    #[must_use]
    pub fn required_component(&self) -> Option<&str> {
        self.required_component.as_deref()
    }

    /// Every `.rs` file this scope holds, sorted.
    ///
    /// # Errors
    ///
    /// [`Rejection::MissingDiskScope`] when the root is not a directory. A scope
    /// that quietly walks nothing is how a guard reads an empty universe and
    /// passes.
    pub fn files(&self) -> Result<BTreeSet<PathBuf>, Rejection> {
        if !self.root.is_dir() {
            return Err(Rejection::MissingDiskScope {
                root: self.root.clone(),
            });
        }
        let mut found = BTreeSet::new();
        self.walk(&self.root, &mut found);
        if let Some(component) = &self.required_component {
            found.retain(|path| {
                path.components()
                    .any(|part| part.as_os_str() == component.as_str())
            });
        }
        Ok(found)
    }

    fn walk(&self, directory: &Path, found: &mut BTreeSet<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                let excluded = entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| self.excluded_directories.iter().any(|it| it == name));
                if !excluded {
                    self.walk(&path, found);
                }
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.insert(normalized(&path));
            }
        }
    }
}

/// The four knobs, as one value.
///
/// Equality and [`Hash`] are over all four, which is what makes a universe its
/// own cache key: two readers that declare the same roots, the same scopes and
/// the same vendor answer share one lowered index (plan §5).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Universe {
    name: String,
    roots: Vec<TargetRoot>,
    scopes: Vec<DiskScope>,
    vendor: Vendor,
}

impl Universe {
    /// Declare a universe. There is no constructor that leaves a knob unset.
    ///
    /// # Errors
    ///
    /// [`Rejection::VendorNotDeclared`] when a root or a scope lies under a
    /// `vendor` directory and `vendor` is [`Vendor::Excluded`], and
    /// [`Rejection::MissingTargetRoot`] when a declared root is not there at
    /// all.
    pub fn declare(
        name: impl Into<String>,
        roots: Vec<TargetRoot>,
        scopes: Vec<DiskScope>,
        vendor: Vendor,
    ) -> Result<Self, Rejection> {
        let mut roots = roots;
        roots.sort();
        roots.dedup();
        let mut scopes = scopes;
        scopes.sort();
        scopes.dedup();
        for root in &roots {
            if !root.file.exists() {
                return Err(Rejection::MissingTargetRoot {
                    package: root.id.package.clone(),
                    target: root.id.name.clone(),
                    file: root.file.clone(),
                });
            }
        }
        if vendor == Vendor::Excluded {
            let vendored = roots
                .iter()
                .map(|root| root.file.clone())
                .chain(scopes.iter().map(|scope| scope.root.clone()))
                .find(|path| is_vendored(path));
            if let Some(path) = vendored {
                return Err(Rejection::VendorNotDeclared { path });
            }
        }
        Ok(Self {
            name: name.into(),
            roots,
            scopes,
            vendor,
        })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn roots(&self) -> &[TargetRoot] {
        &self.roots
    }

    #[must_use]
    pub fn disk_scopes(&self) -> &[DiskScope] {
        &self.scopes
    }

    #[must_use]
    pub fn vendor(&self) -> Vendor {
        self.vendor
    }

    /// Every `.rs` file this universe's disk scopes hold.
    ///
    /// # Errors
    ///
    /// Whatever [`DiskScope::files`] rejects.
    pub fn disk_files(&self) -> Result<BTreeSet<PathBuf>, Rejection> {
        let mut found = BTreeSet::new();
        for scope in &self.scopes {
            found.extend(scope.files()?);
        }
        Ok(found)
    }
}

/// Every target of `package`, as target roots.
///
/// This is the usual first line of a universe and it is a function rather than a
/// method on [`Universe`] so that a reader can take a package's targets, drop
/// the ones it does not mean, and still have said what it took.
#[must_use]
pub fn targets_of(package: &Package) -> Vec<TargetRoot> {
    package.targets().to_vec()
}

/// Whether `path` lies under a directory named `vendor`, which is the only
/// thing this crate means by "vendored".
#[must_use]
pub fn is_vendored(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::Normal(name) if name == "vendor"))
}
