# Folio's changes to `vte` 0.15.0

`vendor/vte/` is the crates.io archive of `vte` 0.15.0
(<https://github.com/alacritty/vte>, licensed Apache-2.0 OR MIT; both licence
files that came with it, `LICENSE-APACHE` and `LICENSE-MIT`, are in this
directory unchanged) with changes by the Folio contributors. Section 4(b) of the
Apache License asks every modified file to carry a prominent notice saying so;
every file listed below does, in its first few lines, and
`scripts/check-vendor-notices.ps1` keeps "differs from upstream" and "carries the
notice" the same set of files.

Upstream ships no `NOTICE` file with this crate, so there is no attribution
notice to propagate under section 4(d).

## Why this crate is vendored at all

The kitty keyboard protocol and xterm's modifyOtherKeys
(`docs/plans/design/keyboard-protocol-2026-09-29.md` §3). `vte` 0.15 already
dispatches all four kitty sequences and both xterm ones, but three of its
`csi_dispatch` arms lose information Folio needs. Intercepting the sequences in
`bt-term`'s boundary parser instead was rejected: that parser runs ahead of the
processor and would have to replay its decisions at the processor's pace.

## `src/ansi.rs` — the three arms

1. **`('u', [b'<'])`, the pop.** Upstream read the count with
   `next_param_or(1)`, which treats an explicit `0` as omitted, so `CSI < 0 u`
   popped one. kitty's count "defaults to 1 if unspecified" and an explicit `0` is
   specified (Windows Terminal agrees): the arm now passes `1` when the count is
   omitted and the value as written otherwise, so `CSI < 0 u` reaches the handler
   as `0`.
2. **`('m', [b'>'])`, XTMODKEYS.** Upstream treated `CSI > m` (no parameters) as
   unhandled. xterm specifies that XTMODKEYS with no parameters resets every
   key-modifier resource to its initial value; the one a handler here can hold is
   modifyOtherKeys, so an added arm calls `set_modify_other_keys(Reset)`.
   `CSI > 0 m`, which names resource 0, is not a reset and still falls through.
3. **`('u', [b'>'])` and `('u', [b'='])`, the push and the set.** Upstream cast
   the flags to `u8` and `from_bits_truncate`d them before the handler saw them,
   so bits 5–15 of a request vanished. The arms now pass the parameter as its full
   `u16`, and `Handler::push_keyboard_mode` and `Handler::set_keyboard_mode` take a
   `u16` accordingly. The terminal masks it to the flags it honours and can name
   every bit that was asked for.

The tests for the three arms are at the end of the file's `mod tests`.

## `src/params.rs` and `src/lib.rs` — telling an omitted parameter from a `0`

Arm 1 needs to know whether the count was written, and `Params` could not say:
the parser pushes `0` for an omitted parameter and for an explicit one alike. So
`Params` carries a bit per entry saying whether it had a digit written
(`Params::written`), and the parser sets it (`Parser::param_written`, carried by
`push_param` and by the subparameter path). Nothing else reads it, and every
existing reader of `Params` sees the same values as before. This is the one
change beyond the three arms, and it is additive: the `Perform` trait is
unchanged, which is why `bt-corpus` — which uses `Parser` and `Perform` only — is
unaffected.

## Every other file

Byte for byte the published archive, including `Cargo.toml`, its `Cargo.lock`
and `.gitignore` (tracked here although that `.gitignore` names `Cargo.lock`,
because the notices gate compares file sets).
