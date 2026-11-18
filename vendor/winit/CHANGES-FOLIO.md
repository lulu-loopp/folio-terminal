# Folio's changes to `winit` 0.30.13

`vendor/winit/` is the crates.io archive of `winit` 0.30.13
(<https://github.com/rust-windowing/winit>, licensed Apache-2.0; the licence
file that came with it, `LICENSE`, is in this directory unchanged) with changes
by the Folio contributors. Section 4(b) of the Apache License asks every
modified file to carry a prominent notice saying so; every file listed below
does, in its first few lines, and `scripts/check-vendor-notices.ps1` keeps
"differs from upstream" and "carries the notice" the same set of files.

Upstream ships no `NOTICE` file with this crate, so there is no attribution
notice to propagate under section 4(d).

## Why this crate is vendored at all

0.4.10 plan row 87, part 87a (ruling R11), which carries row 41
(T-WINIT-EVENT-TARGET-CLOSE). On Windows winit keeps one window per event-loop
thread that nobody sees and nobody closes: the thread event target, a visible,
unowned, layered and transparent popup through which every
`EventLoopProxy::send_event` wakes the loop. Its procedure handed `WM_CLOSE` to
`DefWindowProcW`, which destroys the window, and from then on every wake was
lost without a word: a worker's answer waited 471 seconds on the clean-machine
run of 2026-10-21, where .NET's `CloseMainWindow` picked this window as the
process's main one. Once Folio's last window can be put away (hidden, with the
run going on), the event target is the process's only visible top-level window,
and every tool that closes "the window" of a process finds it.

The upstream change is proposed separately; this copy goes when a winit release
carries it.

**Not a workspace member**, unlike the other vendored crates: the change is one
arm of a Windows window procedure, upstream's own suite (`tests/`) is three
compile-time assertions about `Send`, `Sync` and serde, and membership would pull
the dev-dependencies of its examples and doctests into `Cargo.lock` and its
upstream code under the workspace's clippy. The arm is exercised where it
matters, on a clean machine (the VM harness's close cell).

## `src/platform_impl/windows/event_loop.rs` — one arm

`thread_event_target_callback` answers `WM_CLOSE` with 0 — processed, and
nothing done — instead of letting it fall to the default arm. Nothing else in
the crate changes.
