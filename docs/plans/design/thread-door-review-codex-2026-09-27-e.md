# Thread-door review — Codex confirmation (e), 2026-09-27

Static P4-only review of revision (f) against main at `e70c94bf`; no build or tests.
Counts below are source call-site counts, as defined by the table.

1. `DirWatch` (Windows): **matches** — `SetEvent` ×1, `join` ×1, then `close` ×3 in `dir`, `change`, `stop` order; `close` has only `CloseHandle` ×1.
2. `DirWatch` (macOS): **matches** — `Stopper::signal` ×1 then `join` ×1; `signal` has only the two declared Core Foundation leaves.
3. `trace_sink::Shutdown`: **matches** — `drop → flush → flush_sink → Queue::close`; two `sleep` sites, one `recv_timeout`, and one `join`; `Queue::close` has no further edge/effect.
4. `AttentionPipe` (Windows): **matches** — `SetEvent`, `join`, and `CloseHandle` each ×1; no first-party edge.
5. `AttentionPipe` (Unix): **matches** — `libc::write`, `join`, and `libc::close` each ×1; no first-party edge.
6. `LaunchPipe` (Windows): **matches** — `SetEvent`, `join`, and `CloseHandle` each ×1; no first-party edge.
7. `LaunchPipe` (Unix): **matches** — `libc::write`, `join`, and `libc::close` each ×1; no first-party edge.
8. `video::engine::Engine` (Windows): **matches** — `drop → shutdown` ×1; shutdown has one `sleep` site and one `join`, with no first-party edge.
9. `macos_player::Engine`: **matches** — `drop → shutdown` ×1; shutdown has one `sleep` site and one `join`, with no first-party edge.
10. `VideoSeat`: **matches** — `drop → VideoSeat::shutdown → Engine::shutdown`, each ×1; the target bodies match on Windows and macOS, and portable `no_player::Engine::shutdown` is the pinned uninhabited match with no edge/effect.
11. `VideoSeats`: **matches** — `drop → shutdown_all` ×1, with one loop call site to `VideoSeat::shutdown`; subsequent seat drop is implicit glue covered by row 10.
12. `PtySession`: **matches** — `finish` precedes `shutdown`; all listed helper edges and order match, including two `sync_data` sites, two `Child::try_wait` sites, two `PtyError::from` edges, and the listed `sleep`/`join` counts.

**Dispatch:** yes. A1e may be dispatched from the exception table as written; no P4 rows require correction.
