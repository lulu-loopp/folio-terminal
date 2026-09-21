//! One spelling for one file.
//!
//! Two paths that name the same file have to compare equal, because the whole
//! of the cross-check is a set difference between what the declarations reach
//! and what is on the disk. A `#[path = "../beside/it.rs"]` and a directory walk
//! arrive at the same file by different routes, and `Path` compares components.

use std::path::{Component, Path, PathBuf};

/// `path` with its `.` and `..` components resolved, without touching the disk.
///
/// Deliberately **not** [`std::fs::canonicalize`], which is the obvious choice
/// and the wrong one twice over: on Windows it returns a `\\?\` verbatim prefix
/// that no other path in this crate carries, and everywhere it resolves symbolic
/// links, so a tree checked out through one would compare unequal to the paths
/// the manifest names. What is wanted is a normal form, not the file's identity.
///
/// A `..` that would climb above the root is kept, because there is nothing
/// sensible to resolve it to and swallowing it would turn a wrong path into a
/// plausible one.
#[must_use]
pub fn normalized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let climbed = out
                    .components()
                    .next_back()
                    .is_some_and(|last| matches!(last, Component::Normal(_)));
                if climbed {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Whether `path` lies inside the directory `root`, both taken as normal forms.
#[must_use]
pub fn is_inside(path: &Path, root: &Path) -> bool {
    normalized(path).starts_with(normalized(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — the normal form is what two readings of one file agree on.
    #[test]
    fn one_file_has_one_spelling() {
        let walked = Path::new("crates").join("bt-app").join("src").join("x.rs");
        let named = Path::new("crates/bt-app/src/attention/../x.rs");
        assert_eq!(normalized(&walked), normalized(named));
        assert_eq!(
            normalized(Path::new("a/./b/../c")),
            PathBuf::from("a").join("c")
        );
        // A climb past the top is kept rather than swallowed.
        assert_eq!(normalized(Path::new("../a")), PathBuf::from("..").join("a"));
        assert!(is_inside(Path::new("a/b/c.rs"), Path::new("a/b")));
        assert!(!is_inside(Path::new("a/bb/c.rs"), Path::new("a/b")));
    }
}
