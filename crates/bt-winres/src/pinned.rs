//! **A file out of a vendored package whose bytes are pinned** — the one copy of the two steps
//! every such package is read with: hold the bytes to a pinned SHA-256, and take one entry out of
//! the zip archive the package is.
//!
//! Two build scripts read a package this way. `bt-pty`'s takes the ConPTY sidecar out of
//! `vendor/conpty/` (`bt_pty::conpty_sidecar`), and `bt-app`'s takes Folio's Windows PowerShell
//! line editor out of `vendor/psreadline/` (`bt_app::psreadline_artefact`). Both moved their
//! reading here so the workspace has one account of it (T-INTEGRATION-INJECT-2-BUILD); the
//! pins and the entries are each package's own.
//!
//! [`verify_pinned`] needs nothing. [`entry`] inflates, which is `miniz_oxide`'s work, so it is
//! behind this crate's `inflate` feature: the build scripts and the tests that hold them turn it
//! on, and the one thing that must compile this crate with no dependency at all — the macOS bundle
//! script's `render-info-plist` — does not.

use crate::digest::{hex, sha256};

/// **Refuses `bytes` unless their SHA-256 is `pinned`, and the refusal names `name`.**
///
/// `pinned` is lower-case hex, the way every pin in this repository is written.
pub fn verify_pinned(name: &str, bytes: &[u8], pinned: &str) -> Result<(), String> {
    let actual = hex(&sha256(bytes));
    if actual == pinned {
        Ok(())
    } else {
        Err(format!(
            "{name}: SHA-256 is {actual}, but the pinned official asset's is {pinned}"
        ))
    }
}

/// **The contents of the entry called `name` in the zip archive `archive`.**
///
/// Stored and deflated entries are read. An encrypted entry, another compression method, or a
/// ZIP64 archive (whose sizes do not fit the fields read here) is an error that says which. The
/// records are read through [`crate::zip`], the workspace's one copy of the format's layout, which
/// the updater's archive reader reads a release archive with too (0.4.6 ticket U-14).
#[cfg(feature = "inflate")]
pub fn entry(archive: &[u8], name: &str) -> Result<Vec<u8>, String> {
    use crate::zip;

    let tail_start = archive
        .len()
        .saturating_sub(zip::END_RECORD_BYTES + zip::MAX_COMMENT_BYTES);
    let (_, end) = zip::find_end(&archive[tail_start..])?;
    if end.entries == u16::MAX || end.directory_size == u32::MAX || end.directory_offset == u32::MAX
    {
        return Err("a ZIP64 archive, which this reader does not read".to_owned());
    }

    let mut at = end.directory_offset as usize;
    for _ in 0..end.entries {
        let (entry, next) = zip::central_entry(archive, at)?;
        at = next;
        if entry.name != name.as_bytes() {
            continue;
        }

        if entry.flags & zip::FLAG_ENCRYPTED != 0 {
            return Err(format!("{name} is encrypted"));
        }
        if entry.compressed_size == u32::MAX
            || entry.size == u32::MAX
            || entry.local_offset == u32::MAX
        {
            return Err(format!(
                "{name} is a ZIP64 entry, which this reader does not read"
            ));
        }
        let local = entry.local_offset as usize;
        let header = archive
            .get(local..)
            .ok_or_else(|| format!("{name}: its local header at {local} is past the end"))
            .and_then(|rest| {
                zip::local_header(rest).map_err(|error| format!("{name}: {error} (offset {local})"))
            })?;
        let data = zip::slice(
            archive,
            local + header.length,
            entry.compressed_size as usize,
        )?;
        let contents = match entry.method {
            zip::STORED => data.to_vec(),
            zip::DEFLATED => {
                miniz_oxide::inflate::decompress_to_vec_with_limit(data, entry.size as usize)
                    .map_err(|e| format!("{name} does not inflate: {:?}", e.status))?
            }
            other => return Err(format!("{name} uses compression method {other}")),
        };
        if contents.len() != entry.size as usize {
            return Err(format!(
                "{name} inflated to {} bytes, but its directory entry says {}",
                contents.len(),
                entry.size
            ));
        }
        return Ok(contents);
    }
    Err(format!("no entry named {name}"))
}
