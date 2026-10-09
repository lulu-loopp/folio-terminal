# Folio changes to `arboard`

This directory vendors crates.io `arboard` **3.6.1**. The source was copied from the local
crates.io registry cache; the archive SHA-256 is
`0348a1c054491f4bfe6ab86a7b6ab1e44e45d899005de92f58b3df180b36ddaf`.

The upstream package version, normalized and original Cargo manifests, lockfile, readme, examples,
tools, and license files are retained. `LICENSE-APACHE.txt` and `LICENSE-MIT.txt` remain unchanged.
The workspace uses a local Cargo patch for this pinned package; the root manifest and vendor notice
inventory register the patch.

`src/lib.rs`, `src/platform/linux/mod.rs`, and `src/platform/linux/x11.rs` carry Folio changes.
`Clipboard::new_for_x11` constructs the existing ICCCM owner directly, without choosing Wayland from
`WAYLAND_DISPLAY` or falling back between display systems. The independent `X11OwnerCandidate`
accepts a caller-owned X11 connection, keeps its text in a separate `Inner`, exposes the exact owner
window for same-connection acknowledgement, and reuses Arboard's existing text, `TARGETS`, `INCR`,
selection-request, and manager-handover logic. It does not use the process-global cached clipboard
owner, and its caller controls connection deadlines, serving cancellation, and bounded retirement.
