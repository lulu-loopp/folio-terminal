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
//!
//! And a third, which is not about which build this is but about what it may
//! do: **whether it may update itself** (0.4.6 ticket U-8). `FOLIO_UPDATER=on`
//! in the build's environment emits the cfg `folio_updater`, which
//! `bt_app::update::eligible` reads; unset or empty emits nothing; any other
//! value stops the build. Only the release pipeline sets it. The decision is
//! `src/update_eligibility.rs`, reached below by `#[path]` so its tests run with
//! the crate's.
//!
//! And a fourth, which is about the archive the executable ships in: **what this
//! release contains** (0.4.6 ticket U-9). For a Windows target the `.res` also
//! carries `FOLIO_RELEASE_MANIFEST`, an `RCDATA` block listing the name, SHA-256
//! and size of every archive member but `folio.exe` itself and `folio.msix`, so
//! that the executable's own signature signs the rest of the archive. This
//! script is that fact's one owner: the members are the lines of
//! `scripts/release/archive-members.txt` — the list `package.ps1` packs from —
//! and the bytes are the tree's and `bt-pty`'s ConPTY sidecar, whose paths
//! arrive through that crate's `links` metadata. The format is
//! `bt_winres::release_manifest`.

use std::path::{Path, PathBuf};
use std::process::Command;

use bt_winres::digest::{hex, sha256};
use bt_winres::release_manifest::{
    MEMBER_LIST, MIN_UPDATER, Manifest, Member, PRODUCT, PROTOCOL, RESOURCE_NAME, Source,
    archive_arch, archive_root, parse_member_list, sidecar_key,
};

#[path = "src/update_eligibility.rs"]
mod update_eligibility;

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

    let version = env("CARGO_PKG_VERSION");
    let manifest = release_manifest(&workspace, &version);
    let resource = PathBuf::from(env("OUT_DIR")).join("folio.res");
    std::fs::write(
        &resource,
        resource_bytes(&icon, &version, manifest.as_deref()),
    )
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

    updater_flag();
}

/// **Whether this build may update itself**, from `FOLIO_UPDATER` and nothing
/// else. See `src/update_eligibility.rs` for the one value and why it is one.
///
/// The cfg is declared to `check-cfg` whether or not it is set, so that the
/// crate's `cfg!(folio_updater)` is a known name in both kinds of build. A value
/// that is not Unicode is quoted in its lossy form: it cannot be `on`, so it is
/// refused like any other stranger.
fn updater_flag() {
    use update_eligibility::{CFG, VARIABLE, decide};

    println!("cargo:rerun-if-env-changed={VARIABLE}");
    println!("cargo:rustc-check-cfg=cfg({CFG})");
    let raw = std::env::var_os(VARIABLE);
    let value = raw.as_ref().map(|value| value.to_string_lossy());
    match decide(value.as_deref()) {
        Ok(true) => println!("cargo:rustc-cfg={CFG}"),
        Ok(false) => {}
        Err(refusal) => panic!("{refusal}"),
    }
}

/// **What this release contains**, as the text of the manifest `folio.exe`
/// carries — for a Windows target, and `None` for every other, whose release is
/// not this archive (a macOS bundle's seal covers its files, and its two update
/// keys are `Info.plist`'s).
///
/// Every line of [`MEMBER_LIST`] whose source the manifest lists, in the list's
/// order, hashed from where that source is: the ConPTY sidecar from the path
/// `bt-pty`'s build script exported (`DEP_CONPTY_<KEY>`), `packaging/` and the
/// repository root from this checkout. `.gitattributes` holds those text files
/// to LF on every machine, so the runner that builds and the machine that
/// packages hash the same bytes. Each file is a rebuild trigger: a manifest
/// built before a licence changed would name bytes the archive no longer has,
/// and `package.ps1` would refuse it — correctly, and one build too late.
///
/// The text is also written to `OUT_DIR` and named by `FOLIO_RELEASE_MANIFEST`,
/// so the crate's tests read the manifest this build embedded rather than a
/// second computation of it.
fn release_manifest(workspace: &Path, version: &str) -> Option<String> {
    if env("CARGO_CFG_TARGET_OS") != "windows" {
        return None;
    }
    let list_path = workspace.join(MEMBER_LIST);
    println!("cargo:rerun-if-changed={}", list_path.display());
    let list = std::fs::read_to_string(&list_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", list_path.display()));
    let listed = parse_member_list(&list).unwrap_or_else(|error| panic!("{error}"));

    let members = listed
        .iter()
        .filter_map(|item| {
            let path = match item.source {
                Source::Exe | Source::Msix => return None,
                Source::Sidecar => {
                    let variable = format!(
                        "DEP_CONPTY_{}",
                        sidecar_key(&item.name).to_ascii_uppercase()
                    );
                    PathBuf::from(std::env::var_os(&variable).unwrap_or_else(|| {
                        panic!(
                            "{MEMBER_LIST} lists {} as a sidecar and bt-pty's build script \
                             exported no {variable}",
                            item.name
                        )
                    }))
                }
                Source::Packaging => workspace.join("packaging").join(&item.name),
                Source::Documents => workspace.join(&item.name),
            };
            println!("cargo:rerun-if-changed={}", path.display());
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            Some(Member {
                name: item.name.clone(),
                sha256: hex(&sha256(&bytes)),
                size: bytes.len() as u64,
            })
        })
        .collect();

    let text = Manifest {
        product: PRODUCT.to_owned(),
        version: version.to_owned(),
        arch: archive_arch(&env("CARGO_CFG_TARGET_ARCH")).to_owned(),
        archive_root: archive_root(version),
        protocol: PROTOCOL,
        min_updater: MIN_UPDATER.to_owned(),
        members,
    }
    .encode();

    let written = PathBuf::from(env("OUT_DIR")).join("release-manifest.txt");
    std::fs::write(&written, &text)
        .unwrap_or_else(|error| panic!("write {}: {error}", written.display()));
    println!(
        "cargo:rustc-env=FOLIO_RELEASE_MANIFEST={}",
        written.display()
    );
    Some(text)
}

/// The icon, the version and — for a Windows target — the release manifest, as
/// the bytes of a `.res` file.
fn resource_bytes(icon: &Path, version: &str, manifest: Option<&str>) -> Vec<u8> {
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
    if let Some(manifest) = manifest {
        file.add_named_rcdata(RESOURCE_NAME, manifest.as_bytes());
    }
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
