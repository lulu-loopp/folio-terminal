# Stall hunt — next68 (build 2bfc781d), window_event holds

Read-only. Trace: `D:\Developer\trace\next68\`. Prior art: `D:\Developer\bt-wt\perf-audit\docs\plans\review\perf-review-2026-09-16.md` (next67, findings P-1..P-9) — cited, not re-derived.

## 0. The five slow holds in this run

`grep -n "held control" stderr.log`:

| stderr line | turn | total | window_event | other stations | faults |
|---|---|---|---|---|---|
| 101 | 1 | 2402 ms | 33 ms | woken 1541, publish_frame_inner 735 | +92556 (startup fault-in — not a defect) |
| 3716 | 15341 | 3971 ms | **3960 ms** | publish_frame_inner 5, woken 5 | +0 |
| 13684 | 42836 | 5013 ms | **4990 ms** | drain_pty 9, publish_frame_inner 8, refresh_chrome 3 | +0 |
| 14986 | 53384 | 523 ms | **486 ms** | publish_frame_inner 15, drain_pty 9 | +0 |
| 18509 | 67016 | 2008 ms | **1220 ms** | publish_frame_inner 748, refresh_chrome 17, settle_dpi 9 | +2815 |

Turn 53384 (486 ms) was not in the brief; it is the same shape as the other two and is a fourth sample of the same fault.

---

## 1. Why the handler is unnamed: `redraw` has no station, and the one station that exists stops its clock too early

Three facts from the source. Together they are the whole diagnosis.

**(a) `window_event` is a sticky bare stamp, not a bracket.**
`hang_watch::at(hang_watch::Station::Event)` — `crates/bt-app/src/main.rs:112020`, the first statement of `fn window_event` (`main.rs:112014`). `at` (`hang_watch.rs:1264-1266` → `Heartbeat::at_station`, `hang_watch.rs:1170`) only *moves* the station; it does not save or restore. `enter` (`hang_watch.rs:1286-1290`) is the bracketing form, and the dispatcher does not use it. So every millisecond from `at(Event)` until the *next* stamp anywhere in the process is billed to `window_event` — including everything a handler calls that never stamps.

**(b) The GPU present is one of those things.** The dispatcher arm `WindowEvent::RedrawRequested => runtime.redraw()` is `main.rs:112175`. `Runtime::redraw` is **`main.rs:100425`** in this build (the perf-review's `:100187` is the 2a261ebfb8 worktree; all its `main.rs` line numbers are offset from 2bfc781d). It calls `present_seats_and_commit` (`main.rs:100142`), `present_retained_picture` (`main.rs:100280`) and `trace_present` (`main.rs:100240`), which between them do the synchronous acquire / encode / submit / `queue.present` (`crates/bt-render/src/lib.rs:9632`, `:9635`) and the DirectComposition `commit`. **Not one of those four functions contains a single `hang_watch` call** — verified by scanning `main.rs:100142-100465`. `Station::Present` is stamped in exactly one place in the whole binary — `main.rs:64825`, the first line of `publish_frame_inner` (`main.rs:64824`) — which is *composition into the latest-frame slot*, not presentation. Verified: `grep -n "Station::Present" crates/bt-app/src/main.rs` returns one hit.

So the ledger's `publish_frame_inner` and the reader's intuition "the frame" are different things. `publish_frame_inner 8 ms` on turn 42836 does **not** clear the renderer.

Because the stamp is sticky, *which* bucket `redraw` lands in depends on how it was reached, and both ways are wrong in a different direction:

- Entered from the `RedrawRequested` arm (`main.rs:112175`), the station is still `Event`, so **all of `redraw` is billed to `window_event`** — the ordinary typing case, turns 15341 / 42836 / 53384.
- Entered from `resize()` (`main.rs:97568`), `publish_frame` has just stamped `Present`, so **all of `redraw` is billed to `publish_frame_inner`** — the resize case, turn 67016. `resize()` itself (`main.rs:97475-97569`) stamps nothing, verified by scanning that range.

Either way the present is charged to a name that did not do it.

**(c) The renderer's own receipt stops before the line that reports it.**
`crates/bt-render/src/lib.rs:9820` — `let total_elapsed = frame_started.elapsed();` — then the digest, then at `:9824` the ~1.1 KB `eprintln!("BT_PERF_TRACE frame=…")`. `total_us`, `submit_present_us` and `acquire_us` are all sampled **before** that `eprintln!`. A `BT_PERF_TRACE frame=…` line that blocks for five seconds is therefore invisible in every number on that line and is charged in full to `window_event`, with zero page faults. The `BT_PERF_TRACE present` line (`main.rs:100259`) is printed from inside `redraw` for the same reason and with the same invisibility.

That is exactly, and only, the signature in rows 15341 / 42836 / 53384: seconds inside `window_event`, `publish_frame_inner` in single-digit ms, `faults +0`, and a renderer receipt that says the frame was cheap.

---

## 2. The proven mechanism: a blocked synchronous stderr write on the window thread

The symbolized report `hang-20260916024945518` (5.9 s) puts the window thread in
`ntdll!ZwWriteFile` ← std stderr `synchronous_write` ← `_eprint` ← `bt_render` `compose_frame` (the `eprintln!` at `bt-render/src/lib.rs:9824`) ← `present_seats_and_commit` ← `redraw`.
`run-trace.ps1` gives Folio a **pipe** for stderr (PowerShell `-RedirectStandardError`), and PowerShell's pump stopped draining it for ~6 s. Rust's stderr is unbuffered: one `WriteFile` per `eprintln!`, no queue, no drop-on-full.

Volume in this run (`next68/stderr.log`, 8.16 MB / 28 296 lines):

- `BT_PERF_TRACE frame=` — **8105** lines, ~1.1 KB each (`bt-render/src/lib.rs:9824`)
- `BT_PERF_TRACE present` — **8105** lines (`main.rs:100259`)
- `BT_PERF_TRACE projection` — one per publish (`main.rs:64916`)
- `BT_RESIZE_TRACE` — 535 lines / 175 068 bytes, emitted in bursts (§5)

≈ 2.5 stderr lines and ~1.3 KB **per frame**, every one written synchronously by the window thread from inside a `window_event` handler. A Windows anonymous pipe's default buffer is 4 KiB; three or four frames fill it. The moment the reader stalls, the window thread stalls with it — for exactly as long, with zero page faults, invisible to every instrument except the hold ledger.

**This is an artefact of the traced runs, not of shipped Folio** — but it is poisoning the instrument the owner is using to hunt the real stall, which is worse than a bug of its own.

---

## 3. (1) Every trace write that runs inside a `window_event` handler

### To **stderr** — synchronous, unbuffered, window thread. The dangerous class.

| site | gate | cadence | reached from |
|---|---|---|---|
| `bt-render/src/lib.rs:9824` | `BT_PERF_TRACE` | **every composed frame**, ~1.1 KB | `redraw` → `window_event` |
| `main.rs:100259` | `BT_PERF_TRACE` | **every present** | `redraw` → `window_event` |
| `main.rs:64916` (projection) | `BT_PERF_TRACE` | every publish | `publish_frame_inner` |
| `main.rs:65070` (`skip=unchanged`) | `BT_PERF_TRACE` | every suppressed publish | `publish_frame_inner` |
| `main.rs:64952` (`hold=presentation`) | `BT_PERF_TRACE` | per held frame | `publish_frame_inner` |
| `main.rs:82043`, in `flush_resize_trace` (`main.rs:82026`) | `BT_RESIZE_TRACE` | **a `for` loop, one `eprintln!` per newly buffered ConPTY event** | called at `main.rs:65086` — **inside `publish_frame_inner`** |
| `main.rs:83955` (`conpty tab=…`) | `BT_RESIZE_TRACE` | per pane per resize | resize path |
| `main.rs:82018` (`defer=synchronized-update`) | `BT_PERF_TRACE` | per deferred frame | drain/publish |
| `main.rs:64762` (`resize_frame`) | `BT_PERF_TRACE` | per resize frame | resize path |
| `main.rs:42451` (`search_scan`) | `BT_PERF_TRACE` | per search refresh | publish |
| `main.rs:63491`, `:64634`, `:1411` | `BT_PERF_TRACE` | per image zoom / resample / scale | input + worker paths |
| `bt-render/src/lib.rs:3704`, `:3936`, `:4116`, `:6743`, `:10022`, `:10033` | various | per device rebuild / atlas repack / refusal | render path |

`main.rs` holds **146** `eprintln!` calls in total; the per-frame ones above are the only ones at frame cadence.

### To a **file** — `crates/bt-app/src/trace.rs`

`BT_CARD_TRACE`, `BT_MOUSE_TRACE` and `BT_ATTENTION_TRACE` all go through the shared `Trace` type:

- `trace.rs:48-51` — `pub struct Trace { file: Mutex<File>, … }`. **A raw `File`. No `BufWriter`.**
- `trace.rs:63` — `OpenOptions::new().create(true).append(true).open(path)`, opened once.
- `trace.rs:102-114`, `fn write` — takes the process-wide `Mutex`, `writeln!` straight to the `File`, then **`let _ = file.flush();`** — one `WriteFile` **per line, flushed per line, on the calling (window) thread**, by explicit design (`trace.rs:97-101`: "Flushed per line on purpose: the failure this apparatus exists for may end in a crash").

This run wrote **45 828 lines / 5.4 MB of `card.log`** that way, on the frame path (`card walk … why=frame`), plus 11 147 lines of `thumb.log`. These go to a local SSD, so each write is fast — but it is still an unbatched, un-backgrounded kernel write per line on the window thread.

### `BT_IME_TRACE` — worst-shaped writer of the lot

`main.rs:97203-97211`, the **first statement** of `fn ime_input` (`main.rs:97195`):

```rust
if let Some(path) = diagnostics::named_file(std::env::var_os("BT_IME_TRACE")) {
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{:?} {:?}", Instant::now(), event);
    }
}
```

`std::env::var_os` **plus a full `CreateFile` / `WriteFile` / `CloseHandle` per IME event**, on the window thread, inside `window_event`. It does not use `trace.rs`'s cached handle. 1370 opens this run. On a machine with a filesystem filter driver (Defender, Syncthing, a backup agent) an append-open is the kind of call that can take hundreds of ms. Not a plausible 4-second cause on its own, but it is the one trace writer whose cost is per-event rather than amortized.

### `BT_PTY_DUMP`

Per perf-review §A.4, dumps run **on the PTY reader thread with deferred flushing**, not on the window thread. `pty.bin` (20 MB) is not window-thread I/O. Cleared.

---

## 4. (2)(3)(4) Ranking the candidates for 3.96 s and 4.99 s

Every candidate must satisfy: 4–5 s; `faults +0`; four times in one hour; while typing; `publish_frame_inner` single-digit ms; renderer receipt cheap; and **`alt=1`, 7038 cells, `projected_lines=0`, `lines_measured=0`, projection `refresh_us` 7–12 µs on every frame either side of both stalls**.

### Log alignment — what could and could not be placed

| log | timestamps | aligns? |
|---|---|---|
| `ime.log` | absolute `Instant { t: … }` | **yes** |
| `card.log` | `elapsed_ms` from trace open | **partially** |
| `mouse.log` | *no parseable timestamp on any line* | **no — cannot be placed at all** |
| `thumb.log` | none | no |
| `stderr.log` | none (no wall clock on any line) | only by `since_previous_us` chaining |

**Turn 15341 (3971 ms) — placed exactly.** `ime.log` line 347→348 is a **3.975 s** gap *mid-composition*:
`222890.632 Preedit("shen'he'y")` → `222894.608 Preedit("shen'he")`.
3975 ms against a 3971 ms hold — a 4 ms match. Every other large `ime.log` gap (245 s, 194 s, 113 s, 88 s, 28 s, 20 s, 15 s) is a `Disabled → Enabled` pair, i.e. the user simply not composing; this is the only multi-second gap *between two preedits of one composition*, which is a person typing into a window that has stopped answering. **The IME was composing during turn 15341**, and the next event is one syllable *shorter* — they hit backspace. Confirms the owner's recollection.

**Turn 42836 (5013 ms) — placed weakly.** `card.log` has a **5.011 s** gap (lines 16263→16264, elapsed 553.812 s → 558.822 s), 2 ms from the 5013 ms hold, between two `card walk … why=frame` lines. Weakened by three other ~5 s `card.log` gaps (5.060, 5.045, 5.288) that are ordinary idle. Best independent placement: projecting `ime.log`'s epoch onto `card.log`'s puts the 5 s hold inside `ime.log`'s 28.5 s `Disabled → Enabled` window (223029.7 → 223058.2) — i.e. **the IME was *not* composing; this one was plain ASCII typing.** The two stalls therefore do not share an IME cause.

**A web pane never existed.** All 11 147 `thumb.log` lines carry `page-frames=0 page-hidden=0 page-closing=0 page-blank=0 page-inflight=0 page-throttled=0 page-unchanged=0 page-stale=0`, with `captures=0 pictures=0` on every line. **Every WebView2 / cross-apartment-COM candidate is cleared for this run** — `send_to_web_page`, `web.send_key`, `send_mouse`, `advance_web_page` (`main.rs:98597`), `sync_web_page` (`main.rs:98189`), `apply_web_outcomes` (`main.rs:98914`), and the one genuinely synchronous IPC, `ICoreWebView2Controller::Close` (`main.rs:98672` → `webhost.rs:2667` → `bt-platform/src/webview.rs:2978`, perf-review §D). Nothing to close; nothing to drive.

### Ranked

**1. Blocked synchronous stderr write inside `redraw` — `bt-render/src/lib.rs:9824` and `main.rs:100259`. (Very high; effectively proven for the traced runs.)**
Explains all four fields at once and needs no new hypothesis: symbolized stack (§2); `faults +0` (a `ZwWriteFile` on a pipe is a kernel wait that touches nothing); inside `window_event` because `redraw` stamps no station (§1a/b); invisible to `total_us` because the receipt is sampled at `lib.rs:9820` before the print (§1c); and *four times in an hour* because it depends on the PowerShell pump's scheduling rather than on anything Folio does. The 4.99 s hold sits exactly where the `present` line is printed. It also covers the 486 ms hold and part of the 1220 ms one. **Caveat:** it explains the *traced* stalls only. It cannot explain a stall in an unredirected 0.4.0 run, so it does not close the investigation — it invalidates next67/next68 as evidence for anything else.

**2. Blocking `queue.present` / acquire on the window thread — perf-review P-1 (`main.rs:100337`; `bt-render/src/lib.rs:9632`, `:9635`, `:9701`). (High, independently confirmed here.)**
Frame 4163, the frame at the 42836 stall, records `submit_present_us=1269534` — **1.27 s in present alone**, with `acquire_us=44 encode_us=45`. Frame 4160 has 80 899 µs; frame 1199 has 105 490 µs. Real, on the window thread, zero page faults, billed to `window_event` for the same station reason, and *not* the pipe. Not 5 s by itself, but a second, shipping, non-artefact multi-second-class mechanism at the same site. P-1 already owns the fix.

**3. `flush_resize_trace` stderr burst inside `publish_frame_inner` — `main.rs:82026-82046`, called at `main.rs:65086`. (High, but resize-only — §5.)**

**4. `BT_IME_TRACE`'s open-write-close per event — `main.rs:97203-97211`. (Low-medium.)**
Real unbatched kernel I/O on the window thread, and turn 15341 *is* IME-bracketed. But the `ime.log` timestamp is written *inside* the `writeln!`, so line 347's write had already succeeded when the clock started; the 3.975 s is what happened *after* it. Demoted to a stall the instrument could cause only on a machine with a slow filter driver. Worth fixing regardless: route it through `trace.rs`'s cached handle.

**5. O(history) work on the window thread — P-5 (projection, `bt-term/src/session.rs:8207-8300`), P-8 (`note_user_typing` → `crates/bt-term/src/command_marks.rs:378`, `:442-444`, linear mark scan), P-7 (search rescan), P-6 (wrapped-line materialization). (Effectively excluded for these two stalls.)**
`faults +0` makes resident-data walking attractive in the abstract, and this is where the brief pointed. The logs refuse it: every frame on both sides of both stalls prints `projection … refresh_us=7..12 lines_measured=0 projected_lines=0 rebuilt=0 band_moved=0` — the focused pane has **no frozen history to walk**, being Claude Code on the alternate screen. `row_cache_hits=44/45` with `rows_reshaped=1` says no reshaping either. P-8's `C` is unbounded in principle, but the perf-review found only three OSC 133 markers per PTY prefix. A 5-second CPU-bound walk would also have been caught by the watchdog as a non-answering thread, and **no hang report was written for either turn**. These remain *shipping* costs; they are not tonight's stalls.

An independent pass over the keyboard chain sharpened P-5's mechanism and is worth recording for the P-5 ticket even though it is not tonight's cause: `ViewportProjection::project` (`crates/bt-viewport/src/lib.rs:4439-4457`) is called unconditionally at the end of `DualPlaneSession::refresh_projection` (`crates/bt-term/src/session.rs:8248`), and **the append-only fast path skips only the second, measuring loop — the first loop still pushes every entry of the primary `BTreeMap` document (`crates/bt-doc/src/document.rs:44`) into two fresh vectors.** Neither that loop nor the three further full scans in `sync_projection_state` (`bt-term/src/session.rs:8267`, `:8294`, `:8323` — frozen decorations, then `inline_images.values()` twice) consults `ScreenId`; the one place that *does* gate on alternate-vs-primary, `continuous_frame` (`bt-viewport/src/lib.rs:2558`, ~`:2601`), sits **downstream** of that bookkeeping. So on a pane with real primary history these four O(history) passes run before a single alternate-screen pixel is composed. Bound: `scrollback_lines`, default `SPIKE_DEFAULT_FROZEN_QUOTA = 100_000` (`crates/bt-transcript/src/lib.rs:13`), user-editable upward.

**This is nevertheless refuted as tonight's cause by the telemetry**, which is why it stays at rank 5: every `BT_PERF_TRACE projection` line on both sides of both stalls reads `refresh_us=7..12 lines_measured=0 projected_lines=0 rebuilt=0`. Seven microseconds is the cost of walking an *empty* document. The owner's focused pane was a fresh Claude Code shell with no frozen primary history behind it, so the loops ran over nothing. The finding is a real latent cost that will bite a pane with 100 000 scrollback lines; it did not cost four seconds tonight.

### (2) The IME path: winit's IMM32 work is outside our bracket, and the ledger proves it cost 5 ms

winit 0.30.13 handles `WM_IME_STARTCOMPOSITION` / `WM_IME_COMPOSITION` / `WM_IME_ENDCOMPOSITION` **inside its own window procedure** — `~/.cargo/registry/src/index.crates.io-*/winit-0.30.13/src/platform_impl/windows/event_loop.rs:1513`, `:1527`, `:1581` — and that is where every `ImmGetCompositionStringW` call lives (`.../windows/ime.rs:76`, `:91`, `:99`). Only after reading the composition does it *dispatch* `WindowEvent::Ime(Ime::Preedit(...))` (`event_loop.rs:1571`) or `Ime::Commit(...)` (`:1556`, `:1598`) to the application.

Our bracket opens at `hang_watch::at(Station::Event)`, `main.rs:112020`, which is inside **our** `fn window_event` and therefore strictly downstream of all of that. So IMM32/TSF composition reads are billed to `Parked`/`Woken`, never to `window_event` — as the brief anticipated.

**The hold line settles it.** Turn 15341 reads `window_event 3960 ms, publish_frame_inner 5 ms, woken 5 ms`. Winit's entire IMM32 round trip for that composition is inside those **5 ms of `woken`**. Whatever consumed the 3.96 s happened after our handler was entered. **IMM32, TSF and the candidate window are excluded arithmetically, not by argument** — which also means the Chinese composition is a coincidence of timing rather than a cause, and the second stall (turn 42836, IME disabled, §4) is the control that confirms it.

Within our own handler, `fn ime_input` (`main.rs:97195`) has one genuinely blocking call on the Preedit path — the `BT_IME_TRACE` open-write-close at `main.rs:97203-97211` (§3) — and then composes the preedit into a frame, which schedules the redraw that §1/§2 indict.

### (2) The keyboard path in this build, and why the stamp proves the stall is late

Line numbers for 2bfc781d (the brief's figures are the 2a261ebfb8 worktree):
`keyboard_input` **`main.rs:96106`**; caret-reveal `publish_frame` **`main.rs:96163`**; `refresh_chrome` **`main.rs:96192`** and **`:96395`**; `note_user_typing` **`main.rs:86170`** (a single `HashMap::get_mut` then `Session::note_user_input` — the P-8 linear mark scan lives below it in `bt-term/src/command_marks.rs:378`); `send_user_input` **`main.rs:86177`**.

The decisive line is **`main.rs:86189`**, `self.pending_keyboard_at = Some(Instant::now());` — the Keyboard stamp, taken *inside* `send_user_input`, after return-to-live and before `write_pty_input`. Frame 4164's receipt reads `event_to_submit_us=5029635` against `event_to_present_us=5030267`: **5.03 s between that stamp and submit, with only 632 µs after submit.** So the five seconds elapsed *after* `main.rs:86189` had already run — i.e. after routing, after provenance, after return-to-live, after the PTY write. Everything the brief nominated as pre-send work (`note_user_typing`, the command ledger, attention, return-to-live scroll) is upstream of the stamp and is therefore arithmetically excluded from this stall. What remains downstream is the caret-reveal `publish_frame` at `main.rs:96163` and the redraw it schedules — which is where §1 and §2 put it.

`write_pty_input` is also cleared as a blocking call: per perf-review §A.3 it reaches `InputRing::try_push` (`crates/bt-pty/src/lib.rs:956`), a short mutex and a byte copy that returns `InputRefused` on a full queue rather than waiting for pipe space; the `write_all`/`flush` owner is the writer thread spawned at `bt-pty/src/lib.rs:1438`.

No clipboard call is on the ordinary typing path, so `Station::ClipboardRead` (`main.rs:97138` in this build) not appearing in the hold lines is consistent rather than evidence of a missed entry.

### The two lines to instrument

Both are one-line brackets that would prove or clear #1 and #2 on the next traced run.

**For #1 — bracket the stderr write itself.** In `crates/bt-render/src/lib.rs`, around the `eprintln!` at `:9824`:

```rust
let print_started = Instant::now();                    // immediately before line 9824
eprintln!("BT_PERF_TRACE frame=… print_us={}", …);     // carry the PREVIOUS frame's value
let print_us = print_started.elapsed().as_micros();    // report on the NEXT frame's line
```

The value must be carried to the *next* line, not the current one, precisely because the current line is the thing being measured. A `print_us` of 4 000 000 on the line after a hold ends the question. Mirror it at `main.rs:100259`.

**For #2 — give `redraw` its own station.** At the top of `fn redraw` (`main.rs:100425`):

```rust
let leaving = hang_watch::enter(hang_watch::Station::Redraw);   // new variant
…
hang_watch::at(leaving);                                        // before every return
```

plus a second `enter`/`at` pair around just the acquire+submit+present span in `bt-render/src/lib.rs:9632-9640`. This is the single highest-value station the ledger is missing: it turns every `window_event N ms` in the §0 table into either `redraw`, `present`, or a genuinely unnamed handler, and it costs two relaxed stores. It subsumes what the station-split ticket is doing for the rest.

---

## 5. (5) The resize stall — turn 67016

`window_event 1220 ms, publish_frame_inner 748 ms, refresh_chrome 17 ms, settle_dpi 9 ms - faults +2815, working set 304 -> 278 MB`.

This was a **shrink**: cells go **7038 -> 2160** (72x30), the renderer's `row_cache_resident_bytes` collapses **33 534 173 -> 910 802** between frame 4165 and frame 5532, working set *falls* 26 MB, and frame 5533 shows `rows_reshaped=14, row_cache_misses=14`.

The station boundary here is the opposite of the typing case (see 1b): `resize()` calls `publish_frame` at `main.rs:97562` and then `self.redraw()` at `main.rs:97568`, and stamps nothing of its own. So the two buckets split cleanly at `main.rs:97562`.

### The 1220 ms in `window_event` — everything in `resize()` before the frame

`fn resized` (`main.rs:97684`) -> `fn resize` (`main.rs:97475`) runs, synchronously: `compositor.set_window_size` (`:97495`), `renderer.resize` (`:97507`, geometry only — `bt-render/src/lib.rs:7959` just records `config.width/height`), `reconcile_authoritative_dpi` (`:97511`, which resizes and re-solves the layout *again*, `main.rs:97785`), `resolve_seat_layout` (`:97536`), then the expensive one — `resize_leaves_to_layout` (`:97545`) -> `schedule_leaf_grid_change` (`main.rs:18425`) -> for every **shown** pane, unconditionally `leaf.session.resize_at(...)` (`main.rs:18478`).

`Session::resize_at` (`crates/bt-term/src/session.rs:3149`) opens a resize transaction -> `begin_resize_transaction` (`session.rs:9242`) -> `ResizeAdapter::arm_resize_canonical` (`crates/bt-term/src/adapter.rs:966`). That function calls **`self.term.fork(...)` twice** — `adapter.rs:968` (the canonical branch) and `adapter.rs:975` (a disposable seed) — and then replays the buffered parser tail through the second with `processor.advance(&mut seed_term, &self.parser_tail)` (`adapter.rs:978`).

`Term::fork` is **`self.clone()`** — `vendor/alacritty_terminal/src/term/mod.rs:567-575` ("Clone protocol and grid state..."). `Term` owns **both** `grid: Grid<Cell>` (`mod.rs:414`) and `inactive_grid: Grid<Cell>` (`mod.rs:420`), i.e. the primary *and* alternate screen buffers **including scrollback**. So one resize transaction deep-copies both grids twice, on the window thread, per shown pane, before any frame is composed.

**This is the best explanation for the 1220 ms bucket, and it is the one thing on that turn that fits `faults +2815`.** 2815 faults is roughly 11 MB of freshly-touched pages — exactly the shape of allocating two grid clones — and the *net* working-set drop of 26 MB is the larger maximized-state buffers being released once the transaction's canonical branch is installed and the old allocation dropped. It is CPU and allocator work, not I/O, and it happens strictly before the station switches to `Present`.

### The 748 ms in `publish_frame_inner` — and note it contains the whole of `redraw`

Three things share that bucket, because `redraw` inherits the sticky `Present` stamp on this path:

1. **`flush_resize_trace` (`main.rs:82026`), called from inside `publish_frame_inner` at `main.rs:65086`** — a `for` loop that `eprintln!`s one line per newly buffered ConPTY event (`main.rs:82043`), each about 300 bytes including the repeated 110-char `conpty_source=` prefix. Transaction 4 alone reached **ordinal 227**; the run wrote 535 such lines / 175 KB, overwhelmingly in bursts like this one. Under the pipe mechanism of section 2, a burst of ~200 synchronous writes is a first-class stall candidate, and it exists only because `BT_RESIZE_TRACE` was set.
2. **The fully-damaged re-projection.** `begin_resize_transaction` marks the grid fully damaged, so `refresh_projection` + `viewport_frame` re-shape every visible row rather than reusing cached shaped text — for the focused pane in `publish_frame_inner` (`main.rs:64899-64917`) and for every other visible pane inside `redraw` (`main.rs:100495-100515`). `rows_reshaped=14, row_cache_misses=14` on frame 5533 is this, and it is the honest, non-artefact part.
3. **The deferred swapchain reconfigure.** `Renderer::resize` only recorded geometry; the real `wgpu::Surface::configure` happens later, in `configure_surface_if_needed` immediately before acquire (`crates/bt-render/src/lib.rs:9279`, whose own comment explains the ordering) — a genuine driver/compositor round trip that can block until outstanding GPU work on the old swapchain retires. Frame 5532's receipt agrees: `event_to_present_us=276647` against `event_to_submit_us=171373`, i.e. 105 ms *after* submit.

### The ConPTY round trip is **not** the culprit on this turn

The "~2.9 s transaction 4" is the *sidecar's own* `elapsed_micros` clock counting from `TransactionStart`, not window-thread time: ordinals 213-217 all carry the identical `elapsed_micros: 2275038` because they are buffered in the sidecar and replayed into the log together, and ordinals 219/220 (`PtyChunkArrived`, 8192 + 3220 bytes) at 2.57 s are the **reader thread**. The transaction spans 2.78 s of wall time across many turns.

`ResizePseudoConsole` genuinely is a synchronous round trip into conhost — `flush_pending_pty_resize` (`main.rs:83870`) -> `commit_leaf_resize` (`main.rs:18336`) -> `PtySession::resize` (`crates/bt-pty/src/lib.rs:1499`), and `hang_watch.rs:314-316` documents it as such. But it has its **own station**, `Station::PtyResize`, entered at `main.rs:83871`, and it is debounced behind a 200 ms quiet window (`main.rs:97529`) so it does not run in the resize handler at all. **`PtyResize` does not appear in turn 67016's hold line**, which means it cost under a millisecond on that turn. Cleared for this stall — though it remains the right thing to watch on a turn where it *does* appear.

Also refuted: "relayout of every pane". `schedule_leaf_grid_change` (`main.rs:18425`) sends hidden leaves down the `LeafOnStage::Behind` path, which defers to the same quiet-window debounce; only **shown** panes reflow synchronously (perf-review section E).

### Ranking for the resize hold

1. **Two `Term::fork()` deep clones of both grids** (`adapter.rs:968`, `:975` -> `vendor/alacritty_terminal/src/term/mod.rs:567`) — the 1220 ms bucket; the only candidate that explains `+2815` faults and the RSS drop. Unlike the typing stalls, this one is **real shipping behaviour, not a trace artefact**.
2. **`flush_resize_trace` stderr burst** (`main.rs:82026`, called at `:65086`) — a large share of the 748 ms, but trace-only.
3. **Fully-damaged re-projection + `Surface::configure`** (`bt-render/src/lib.rs:9279`) — the rest of the 748 ms; genuine.
4. **Synchronous `ResizePseudoConsole`** — cleared by the absence of `PtyResize` from the line.

## 6. What this settles and what it does not

**Settles.** Why the handler is unnamed: `redraw` stamps no station and `hang_watch::at` is sticky, so the present is always billed to whichever name happened to be standing — `window_event` when reached from `RedrawRequested`, `publish_frame_inner` when reached from `resize()`. Why `publish_frame_inner` looked innocent on the typing stalls (it is composition, not presentation). Why the renderer receipt looked innocent (`bt-render/src/lib.rs:9820` stops the clock before the `eprintln!` at `:9824`). That **no web pane existed** in this run, clearing every WebView2/COM candidate. That the focused pane had **no frozen history to walk**, clearing P-5/P-6/P-7/P-8 as tonight's cause. That winit's IMM32 work is outside our bracket and cost 5 ms (`woken`), clearing IME/TSF for turn 15341. That ConPTY did not block the window thread on the resize.

**The one genuine, shipping defect found.** The resize hold is *not* a trace artefact: `arm_resize_canonical` (`crates/bt-term/src/adapter.rs:966`) deep-clones both the primary and alternate `Grid<Cell>` — scrollback included — **twice** per resize transaction per shown pane, synchronously on the window thread (`adapter.rs:968`, `:975`; `vendor/alacritty_terminal/src/term/mod.rs:567-575`, `:414`, `:420`). That is the 1220 ms and the `+2815` faults, and it will scale with scrollback on any resize, traced or not. It deserves its own ticket independent of everything else here.

**Does not settle.** The owner's stalls in **untraced 0.4.0** runs. With `BT_PERF_TRACE` and `BT_RESIZE_TRACE` unset every `eprintln!` in section 3 is gated off and the pipe mechanism cannot fire. For those the live candidates are P-1 (blocking present, `crates/bt-render/src/lib.rs:9635`) — which needs no trace flag and is measured at 1.27 s on frame 4163 of this very run — the `Term::fork` clones above on any resize, and whatever `scratchpad/review3/perf-diagnostics.log` holds name once `redraw` has a station to name them with.

**Operational.** The next traced run must send stderr to a **file**, not a PowerShell pipe (`run-trace.ps1`'s `-RedirectStandardError`), or the instrument will keep measuring itself and every future hold line will be suspect. Do that before adding the two probes in section 4, not after — otherwise the `print_us` probe will simply report the artefact again.
