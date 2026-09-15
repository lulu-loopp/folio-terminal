//! **What `folio.exe` knows about itself that Rust cannot tell it.**
//!
//! Two things, and they are the same thing twice: which build this is.
//!
//! * `FOLIO_COMMIT` — the short hash of the commit the binary was built from,
//!   for the three diagnostic files. A version number alone answers "which
//!   release"; only the hash answers "which build", which is the question a
//!   panic log from a preview is actually asked.
//! * The `.res` file carrying the application icon and the `VERSIONINFO` block.
//!   Those are the two facts about this executable that live in the *file*
//!   rather than in the program — the icon Explorer draws and the version its
//!   Properties page shows — and neither can be a Rust constant. See
//!   `bt_winres` for why the bytes are written here rather than by `rc.exe`.
//!
//! **Both derive from `CARGO_PKG_VERSION`**, which is the workspace's one
//! version line. Nothing in this file spells a version.

use std::path::{Path, PathBuf};
use std::process::Command;

/// What the commit is called when there is no git to ask.
///
/// A tarball, a vendored copy, a build from an exported tree: all real, and none
/// of them a reason to fail a build. The diagnostic files then say `unknown`,
/// which is honest and is still enough with the version beside it.
const NO_COMMIT: &str = "unknown";

fn main() {
    let manifest_dir = PathBuf::from(env("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("bt-app must remain under WORKSPACE/crates/bt-app")
        .to_path_buf();

    let commit = commit_of(&workspace);
    println!("cargo:rustc-env=FOLIO_COMMIT={commit}");

    let icon = workspace.join("assets").join("app-icon").join("folio.ico");
    println!("cargo:rerun-if-changed={}", icon.display());

    let resource = PathBuf::from(env("OUT_DIR")).join("folio.res");
    std::fs::write(&resource, resource_bytes(&icon, &env("CARGO_PKG_VERSION")))
        .unwrap_or_else(|error| panic!("write {}: {error}", resource.display()));
    // Read by `bt_app::version`'s gate, which checks that the version in the
    // resource is the version in the binary. It is set for every target of this
    // crate, including the test one, which is the point.
    println!(
        "cargo:rustc-env=FOLIO_VERSION_RESOURCE={}",
        resource.display()
    );
    // `link.exe` takes a `.res` as an ordinary input file. `-bins` and not the
    // whole crate: a test binary linking the icon would be harmless and would
    // also be a second executable on the machine wearing the product's face.
    if env("CARGO_CFG_TARGET_ENV") == "msvc" {
        println!("cargo:rustc-link-arg-bins={}", resource.display());
    }
}

/// The icon and the version, as the bytes of a `.res` file.
fn resource_bytes(icon: &Path, version: &str) -> Vec<u8> {
    let ico = std::fs::read(icon)
        .unwrap_or_else(|error| panic!("read the application icon {}: {error}", icon.display()));
    let icon = bt_winres::IconGroup::parse(&ico)
        .unwrap_or_else(|error| panic!("the application icon is not usable: {error}"));
    let numbers = bt_winres::FileVersion::parse_semver(version)
        .unwrap_or_else(|error| panic!("the workspace version is not usable: {error}"));

    let mut file = bt_winres::ResourceFile::new();
    // Group one, because Explorer draws an executable with its lowest-numbered
    // icon group and `bt_platform::context_menu_shape` registers `folio.exe,0`
    // meaning exactly that one.
    file.add_icon(1, &icon);
    file.add_version_info(&bt_winres::VersionInfo {
        file_version: numbers,
        product_version: numbers,
        strings: vec![
            // `CompanyName` is deliberately the product and not a legal entity:
            // there is no company, and a blank field renders as a blank line on
            // the Properties page rather than as an absence.
            ("CompanyName".to_owned(), "Folio".to_owned()),
            ("ProductName".to_owned(), "Folio".to_owned()),
            // Task Manager and the Properties page print this field as the
            // program's name, so it is the name and nothing after it.
            ("FileDescription".to_owned(), "Folio".to_owned()),
            // The text form keeps whatever the manifest said, suffix and all;
            // the four numbers above cannot carry one.
            ("FileVersion".to_owned(), version.to_owned()),
            ("ProductVersion".to_owned(), version.to_owned()),
            ("InternalName".to_owned(), "folio".to_owned()),
            ("OriginalFilename".to_owned(), "folio.exe".to_owned()),
            // **The same notice the two licence files carry**, word for word:
            // `LICENSE-MIT` line 3 and the appendix of `LICENSE-APACHE`. The
            // field is named for a copyright notice, so it holds one — the
            // licence follows it because a reader who opens the Properties page
            // is asking both questions at once, and this is the only place in
            // the shipped binary where either is written.
            (
                "LegalCopyright".to_owned(),
                "Copyright (c) 2026 Weiyi Shi and Folio contributors. \
                 Licensed under MIT OR Apache-2.0."
                    .to_owned(),
            ),
        ],
    });
    file.finish()
}

/// The short commit hash of `workspace`, or [`NO_COMMIT`].
///
/// The rebuild triggers are the files a commit moves: `HEAD` itself, and — when
/// `HEAD` names a branch — wherever that branch's tip is written. Without them
/// the hash would be frozen at whatever it was the first time this crate was
/// compiled, which is worse than not having one: a stale hash in a panic log
/// points at the wrong source.
///
/// **Where those files live is git's question to answer, not ours.** A repository
/// is not one directory: a linked worktree has its own `HEAD` under
/// `.git/worktrees/<name>/`, while branches stay in the *common* directory at
/// `.git/refs/heads/`, so a path built by joining names onto one git dir names a
/// file that exists in neither place. And a tip is not always a file at all —
/// once `git gc` packs it, the loose ref is gone and the tip lives in
/// `packed-refs`. Both cases matter to cargo the same way: a
/// `rerun-if-changed` path that does not exist makes the build script *always*
/// dirty, so this crate — the workspace's longest link — would be rebuilt on
/// every single `cargo` invocation.
///
/// So each file is located with `rev-parse --git-path`, which knows the
/// per-worktree/common split; the loose ref is declared only when it is really
/// there, and `packed-refs` alongside it, because a packed tip moves that file
/// instead.
fn commit_of(workspace: &Path) -> String {
    // Also the "is there a git here at all" question: it fails when git is
    // absent and when this is not a repository.
    let Some(head) = git_path(workspace, "HEAD") else {
        return NO_COMMIT.to_owned();
    };
    println!("cargo:rerun-if-changed={}", head.display());
    if let Some(reference) = git(workspace, &["symbolic-ref", "--quiet", "HEAD"]) {
        // Absent while the tip is packed, and written the moment a commit lands
        // on this branch — which is the transition that has to be noticed.
        if let Some(path) = git_path(workspace, &reference).filter(|path| path.exists()) {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    // The other half of that pair: while the tip is packed, this is the file it
    // is packed in, and unpacking it rewrites this file too.
    if let Some(path) = git_path(workspace, "packed-refs").filter(|path| path.exists()) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    git(workspace, &["rev-parse", "--short=10", "HEAD"]).unwrap_or_else(|| NO_COMMIT.to_owned())
}

/// Where git keeps the file it calls `name`, as an absolute path.
///
/// `--git-path` answers relative to the current directory — `.git/HEAD` in an
/// ordinary checkout — so the answer is resolved against `workspace`, which is
/// the directory the command ran in. An answer that is already absolute, as it
/// is from a linked worktree, survives the join unchanged.
fn git_path(workspace: &Path, name: &str) -> Option<PathBuf> {
    git(workspace, &["rev-parse", "--git-path", name]).map(|path| workspace.join(path))
}

/// One `git` command's trimmed output, or `None` when git is absent, this is not
/// a repository, or the command failed.
fn git(workspace: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .current_dir(workspace)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("Cargo did not set {name}"))
}
