STATUS COMPLETE

# Verification of the five post-release-review merges against source

Target: `c4258ba0` (0.4.2 candidate), worktree `D:/Developer/bt-wt/ds-verify4`, detached.
Method: read-only inspection (git/grep/read only; no cargo build/test, no launch, no edit).
Scope: crash, lifetime and thread-boundary defects only — the release review's own brief —
not style, naming or documentation. Each merge's net diff is the tree at its first-parent
ancestor versus the merge commit.

## 1. dab0d6b3 — fix/trace-sink-never-blocks (trace_sink, diagnostics, hang_watch, main, bt-platform)

**Verdict: SOUND.**

The watchdog no longer prints: `write_report` returns `Reported { path, said }`
(`hang_watch.rs:2134,2141`) and `watch_forever` says everything through `diagnostics::note`
*after* the report file is on disk (`hang_watch.rs:2101-2107`). `note` (`diagnostics.rs:279`)
appends to `diagnostics.log` via a per-line open/append/close (`append_note`, `:298`), then
offers the line to the trace sink; the sink's writer reaches stderr through `ProcessStderr` →
`bt_platform::write_std_error` (`trace_sink.rs:340`, `lib.rs:9878`, `portable_impl.rs:1451,1491`),
a direct `WriteFile`/`libc::write` that never takes Rust's process-wide `Stderr` lock, so a
console whose reader stopped holds nobody up. Producers never wait: `Queue::offer` is
`try_lock` + `try_send` returning `bool` (`trace_sink.rs:107`), `Poisoned`/`WouldBlock` both
handled. No panic path and no unbounded loop on the new roads (`write_std_error` retries EINTR
and refuses zero-progress; `append_note` is best-effort `.is_ok()`). The one deliberate cost is
stated in the code: the watchdog may now block on the *local* disk instead of a console, which
is the point.

## 2. 79227801 — fix/pane-close-off-thread (bt-pty `retire_session`, main quit/close paths)

**Verdict: SOUND.**

The window thread cannot reach a taken session: `close_pane` removes the leaf from `sessions`
and `take()`s its pty before `retire_session` (`main.rs:54079-54129`), and every later access is
seat-keyed, so the off-thread retirement owns the session exclusively. No double close:
`PtySession::shutdown` is idempotent (`take()`/`close()` return `None`; `bt-pty/src/lib.rs:1802-1895`
with `Drop` at `:1898-1902`). Teardown order is correct — child killed/reaped before
`ClosePseudoConsole`, job object closes and kills descendants at the end of the `if let` block
(confirmed `vendor/conpty/portable-pty/src/win/mod.rs:81,120-123`). `wait_for_retirements` is a
bounded `Condvar::wait_timeout` (`bt-pty/src/lib.rs:479-516`), so quit proceeds after
`RETIREMENT_BUDGET` (4 s) with the child already dead. Nothing here is Windows-only; the whole
path is portable `SIGKILL` + pty-fd close on macOS.

## 3. d0fd85ec — fix/device-loss-never-panics (bt-render)

**Verdict: SOUND.**

`on_uncaptured_error` latches the first fault into `Arc<OnceLock<String>>` and `eprintln!`s;
`OnceLock::set` returns a `Result` with no `unwrap`, so there is no panic path and no
window-thread state touched (`bt-render/src/lib.rs` handler + `RenderError` `:1564-1626`).
`vertex_buffer` const-asserts 4-byte alignment and all unconditional call sites fall back to a
non-empty `empty_rect` (`:8821`), so no zero-size buffer is ever created. Rebuild replaces both
latches last (`rebuild_after_device_loss`, `:6263-6404`) and the pilot is bounded by
`DEVICE_REBUILD_ATTEMPTS=3` (`:3823`, `:3961-3990`). A pure `Wgpu` fault (device not lost) still
stops the process via `fail` (`main.rs:112617-112638`) — strictly better than the previous
`panic!`. The `eprintln!` in the handler could only panic on a failed stderr write, no worse
than the default it replaced.

## 4. d8d6fb4f — fix/quit-timeout-proceeds (quit.rs, persist.rs)

**Verdict: SOUND, one low note (V-1).**

`WriteVerdict::TimedOut.leaves()` is `true` (`quit.rs:199`), so a save that ran out of budget
retires and the windows leave — the exact hang the deadline exists to end. `stalled` is sticky
but only ever *set* during a quit: `wait_for` is called only from the quit Write step, and once
set it short-circuits future waits to `TimedOut` (`persist.rs:508-542`). A TimedOut quit leaves
`session.lock` standing, so the next launch logs the prior run as `ExitState::Crashed` and shows
the restore prompt — not a block. `atomic_write` is a genuine same-dir temp + fsync + rename
(`bt-persist/src/atomic.rs:25-50,69-80`), and dropping the old `write_off_thread` fallback
removes the two-writer race the review flagged.

## 5. ebaabbaf — fix/picture-paste-review (clipboard_picture.rs, main paste code)

**Verdict: SOUND.**

Delivery-after-close is closed by a three-fact address: `PasteTarget { tab, seat, incarnation }`
(`main.rs:759`), minted from a process-wide counter (`next_incarnation`, `:741`) and checked at
spend time by `live_paste_target` (`:98102`), which drops the paste if the tab is no longer on
top, the seat is gone, or the shell was restarted — and `restart_shell` does mint a fresh
`LeafSession` (`:75251-75270`), so the incarnation really changes. The second-paste race is
closed by moving the generation and the slot under one lock: `deliver_clipboard_picture` stores
only if `wanted == generation` (`main.rs:872`), so a stale worker cannot erase a newer
answer. The size bound is at the copy (`bt-platform/src/clipboard.rs` `MAX_PICTURE_BYTES`) and
re-checked against the decoder's own `total_bytes` before any allocation
(`clipboard_picture.rs:330-386`, `fits` `:294`, DIB/Png rungs `:348-377`); the red-gate tests
cover the 32 768×32 768 header. `reserve` takes names with `create_new`, so two windows or two
Folios cannot share a name, and the worker has no `unwrap`/`panic` reachable from user bytes —
decode and fs are all `Result`, so a panicking worker is not a reachable state, and the
`ClipboardPictureJob` guard (`main.rs:906`) bounds in-flight decodes.

## Findings

### V-1 — orphaned `session.json.tmp-*` after a TimedOut quit (low, area 4)

- **File:** `crates/bt-persist/src/atomic.rs:37-50,82-111`; reachable via `persist.rs:508-542`
  (`wait_for` → `SaveRefusal::TimedOut`) + `quit.rs:199` (`leaves()`).
- **What happens:** quit while the save has stalled past its budget (a disk that stopped
  answering) now exits the process with the session writer still inside `write_temp`'s
  `file.sync_all()` (`atomic.rs:41`); the best-effort `remove_file` at `:47` and `:73` never
  runs, so one uniquely-named `session.json.tmp-<pid>-<nanos>-<n>` is left in the data
  directory. Nothing ever reads it; the next launch opens `session.json` as usual.
- **Smallest correct fix:** at `SessionStore::open`, before `probe_sentinel`, remove
  `session.json.tmp-*` siblings (one `read_dir` + best-effort `remove_file` loop, the same
  pattern `atomic.rs` already uses). Cosmetic, not a correctness bug — the crash window is
  documented and the temp name collides with nothing.

## Ranked summary

1. **All five merges are sound** — no critical, high or medium defects found.
2. **V-1 (low, area 4):** a TimedOut quit can orphan one inert `session.json.tmp-*` file; a
   startup sweep at open removes it.
3. Everything the review flagged — console-stall reachback (1), off-thread close (2),
   device-loss panics (3), quit timeout (4), and the three paste races / header decode (5) —
   is genuinely fixed, with no new reachable crash, hang, leak or wrong-target behavior.

STATUS COMPLETE
