# Folio changes to `wl-clipboard-rs`

This directory vendors crates.io `wl-clipboard-rs` **0.9.4**. The source was compared with
`wl-clipboard-rs-0.9.4.crate` from the local crates.io cache; the archive SHA-256 is
`4d7888ccd4896447b2d14d3a9350a85df2aeb6f181e2e7a31349d104ac46cac1`.

The upstream package version, lockfile, and license files are retained.
`LICENSE-APACHE` and `LICENSE-MIT` are byte-for-byte identical to the archive. The
workspace uses a local Cargo patch for this pinned package; workspace patch and vendor-notice
inventory are maintained outside this directory.

## Upstream source inventory

All upstream library source files are copied from the archive:

`src/common.rs`, `src/copy.rs`, `src/data_control.rs`, `src/lib.rs`, `src/paste.rs`,
`src/seat_data.rs`, `src/utils.rs`, and `src/watch.rs`.

All upstream test source files are copied from the archive:

`src/tests/copy.rs`, `src/tests/mod.rs`, `src/tests/paste.rs`, `src/tests/state.rs`,
`src/tests/utils.rs`, and `src/tests/watch.rs`.

`src/common.rs`, `src/copy.rs`, `src/paste.rs`, `src/watch.rs`,
`src/tests/copy.rs`, `src/tests/mod.rs`, `src/tests/paste.rs`, and
`src/tests/state.rs` differ among the copied source files. `Cargo.toml` and `Cargo.toml.orig` also
enable rustix's `net` feature for the safe nonblocking connection API.

## Source deltas

### `src/copy.rs`

- Adds `copy_multi_owned_until`, which asks the caller for a named worker spawner and returns a
  joinable handle before native setup completes. `PreparedCopy` and its connection stay on that
  worker because the Wayland event queue is not `Send`.
- Uses the operation deadline and cancellation during nonblocking connect, registry/seat setup,
  claim flush, and the cancellable `wl_display.sync` callback barrier. It reports a confirmed claim
  only after that same-connection server reply, not after `flush` alone.
- Adds same-connection `reconcile_until`, which queues a later sync barrier without re-claiming
  selection. An unresolved candidate remains live and joinable while serving; only a server reply
  or observed source destruction resolves its ownership state.
- Adds cancellable serving that releases only this copy's live data sources on their owning
  connection. When another client has replaced the source, the compositor's cancellation event
  ends the old server without selecting or clearing anything on that client's behalf.
- Owned copies keep transfer writes nonblocking and poll for cancellation while a receiver is not
  draining its pipe. The legacy `serve` path keeps its blocking transfer behavior.
- Adds owner retirement that observes the caller's cancellation and absolute cutoff. Cancellation
  destroys the candidate's sources on their owning connection and flushes. Retirement joins a
  finished worker; an unfinished handle remains available when the cutoff expires.
- Owned handles cancel without waiting on drop. The unbounded join entrypoint is removed; callers
  and tests use explicit bounded retirement.

### `src/common.rs`

- Moves the nonblocking Wayland socket connector and the cancellable `wl_display.sync` roundtrip
  loop into one shared path for reads and owned copies. Copy reconciliation retains its callback
  on the event-queue thread and can wait on both operation cancellation and owner retirement.

### `src/tests/copy.rs`

- Adds a protocol-server test that replaces an owned copy with a later source, cancels and joins
  the old server, then reads the later source's bytes.
- Adds an unread-pipe test that fills a transfer pipe, requests cancellation, and joins the owner;
  an external `timeout` process contains the deliberately blocking pre-fix probe.
- Adds a flush-gated compositor fixture that installs the selection but withholds server replies;
  the copy remains unconfirmed until same-connection reconciliation, after which a separate read
  gets the copied text.

### `Cargo.toml` and `Cargo.toml.orig`

- Enable rustix's safe Unix socket APIs for nonblocking session connection acquisition.

### `src/paste.rs`

- Adds `OfferSession`, which selects one clipboard offer and retains it while callers try an
  ordered set of MIME types. The session filters retained MIME candidates and uses the existing
  Wayland data-control offer implementation.
- Adds cancellation and one absolute deadline to registry discovery, protocol round trips, and
  pipe transfer. Event waits poll the Wayland connection and transfer pipe, and bounded reads keep
  only up to the caller's byte limit while detecting one excess byte.
- Opens path-based connections for `OfferSession` with a nonblocking AF_UNIX connect, so a full
  listener backlog returns the existing transport error instead of bypassing the session's
  deadline and cancellation checks. Environment-based connections retain `WAYLAND_SOCKET` fd
  ownership and removal behavior, absolute `WAYLAND_DISPLAY` paths, and relative paths under an
  absolute `XDG_RUNTIME_DIR`.
- Adds named cancellation, deadline, over-limit, and transfer errors; dropping the session
  destroys the selected offer.
- Adapts the existing one-shot `get_contents` state to share the new registry dispatch without
  changing its public entrypoints.
- Reuses the shared connection and roundtrip helpers from `common.rs`; the read-side deadlines and
  cancellation behavior remain unchanged.

### `src/watch.rs`

- Adapts to the shared initializer's returned connection while preserving watch behavior.

### `src/tests/paste.rs`

- Adds compositor-backed tests for retaining the original offer across clipboard replacement,
  reading successive MIME types from that offer, the exact byte cap, and cancellation/deadline
  admission.
- Adds a barrier-controlled stalled transfer test. The barrier coordinates cancellation; the test
  does not use a clock sleep to make the transfer stall.
- Adds a controlled-child regression for a full default Wayland socket backlog through the public
  `OfferSession` path. The child has a four-second session deadline and an external watchdog so the
  pre-fix blocking connect cannot leave a test process behind.

### `src/tests/state.rs`

- Lets test offer resources retain the `OfferInfo` they advertised. This models the Wayland rule
  that a selected offer remains the source after a later selection replaces it.
- Adds a barrier-controlled stalled offer used by the cancellation test.
- Lets the protocol test hold client-output flushing after a selection request to prove that owned
  copy readiness requires server confirmation.

The source files carry local notices at their edit points. No other upstream source file is
modified.
