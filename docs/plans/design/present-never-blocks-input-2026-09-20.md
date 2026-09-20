# Present never blocks input: a budget for the compositor's answer

Design only, 2026-09-20. No implementation, build, test, application launch or measurement belongs to this stage; nothing below was run. Inspected `origin/main` at `95c5883e`. Registry paths are relative to `C:/Users/Weiyi/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`; `crates/...` is this worktree. This note answers a complaint, not a ticket: *while typing, the window freezes for one to two seconds, several times an hour, and then everything catches up.*

It has a predecessor. `docs/plans/design/render-handoff-2026-09-16.md` already designs option D below in full — a per-window presentation lane — and that design stands. This note is narrower and earlier: it asks what may be done **before** that lane exists, and states the rule the lane would inherit.

## 1. The fact, its owner, and the invariant

**The fact**: *may the window thread wait for the compositor, and for how long?*

**Today the fact has no owner and no stated answer.** The wait is implicit and unbounded. It is spent inside two library calls made from the one funnel `Runtime::present_seats_and_commit` (`crates/bt-app/src/main.rs:104162`), which runs on the winit thread that also delivers every keystroke and every IME event. The hang watch's station chain says so without inference:

```
window_event 1 ms (redraw 0 ms (present_retained_picture 0 ms (present_seats_and_commit 0 ms
  (swapchain present 655 ms, text shaping 4 ms, atlas upload 1 ms))))
  · thread CPU 0.000 ms / wall 669 ms                      (diagnostics.log, stall #3, turn 44060)
```

**Proposed invariant.** *The input path never waits on the compositor for longer than one display interval. A frame that cannot be shown inside that budget is dropped or deferred; it is never waited for.*

**The budget is one display interval, and the window already owns that number**: `pace::FrameClock` holds one interval taken from the display the window is on (`MonitorHandle::refresh_rate_millihertz`, 16 ms when the platform will not say) — DESIGN §7.1.5p ⑬, `crates/bt-app/src/pace.rs`. One interval is right because it is already the rate at which this window is allowed to ask for a frame (`Runtime::animation_frame_is_due`): a wait longer than the interval cannot buy a picture the pacer would have let us draw anyway, so it buys nothing and costs a keystroke. It is not zero, because on macOS the drawable wait **is** the pacer (§3) and a zero budget there would hand pacing to nobody. Where a readiness signal exists (§4 B) the budget is nevertheless *spent* as zero — asking costs nothing — and one interval is then the deadline after which a deferred frame is dropped in favour of a newer one.

## 2. What the evidence says, and what it does not

Population, stated exactly, because "more than half" is true of one build and not of the log:

- `%APPDATA%\Folio\diagnostics.log` holds **2224** stall lines across many builds. Over that whole population `swapchain present` is the largest station on 46 and `surface acquire` on 14.
- The audits' extract (`scratchpad/stall-audit/stall-lines.txt`, 400 lines, six sessions, next78–next81) contains **67** lines from the build that reports sub-stations. On **35 of those 67 (52%)** the largest station is `swapchain present` (30; min 373 ms, median 881 ms, max 2332 ms) or `surface acquire` (5; 146, 489, 644, 732, 1022 ms). This is the "more than half".
- Today's build adds the window thread's own CPU time. **Seven** such lines exist so far; on four, present or acquire dominates and the thread is demonstrably *not computing*: `swapchain present 655 ms … thread CPU 0.000 ms / wall 669 ms`; `swapchain present 1405 ms … thread CPU 15.625 ms / wall 1421 ms`; `surface acquire 773 ms … thread CPU 31.250 ms / wall 903 ms`. Format: `crates/bt-app/src/hang_watch.rs:1519-1524`; CPU source `bt_platform::mem::thread_cpu_us` via `armed_thread_cpu_us` (`hang_watch.rs:1981-1984`).
- **Bursts, not isolated frames.** Four episodes of two or three consecutive turns: 869+1075+598 ms (turns 178927–929), 1312+509+373 (289973–975), 546+381, 727+412, 881+702. The thread unblocks and immediately blocks again: the blocker persists across turns.
- Both GPUs — the AMD iGPU run (`asked=LowPower`) and the NVIDIA dGPU run both produce present stalls, so this is not an adapter choice.
- **The machine itself hitches** on a fixed 910 s wall-clock beat, 1–5 s, with an outside sampler process freezing at the same instants; the trigger is unknown and is not Folio. Other programs drop a frame. Folio freezes typing, because present is on the thread that reads the keyboard. **This note does not propose to fix the machine.** It proposes that a stalling compositor stop being visible as a frozen caret.

**What the evidence does not say.** No stall line carries any window's visibility, occlusion, minimised or monitor-power state — I checked the whole log; the only matches for "visible" are the station name `visible artifact detection`. So **how many of these stalls happened while nobody could see the window is unknown**, and option A's value cannot be estimated from what exists. The trace that would say it is one field per stall line: the four booleans that already compose `PresentConditions.visible` (§4 A), per window, at the moment the station opened.

## 3. What wgpu 30 permits (read at `wgpu*-30.0.0`, pinned in `Cargo.lock`)

1. **The timeout inside acquire is hard-coded and unreachable.** `wgpu-core-30.0.0/src/present.rs:32` `const FRAME_TIMEOUT_MS: u32 = 1000;`, passed unconditionally at `present.rs:177-180`. The public `Surface::get_current_texture` takes no timeout (`wgpu-30.0.0/src/api/surface.rs:148-199`) and there is no try-acquire variant. `surface acquire 1022 ms` is that constant expiring. **Acquire cannot be made short from outside.**
2. **The readiness signal, however, is public.** wgpu-hal's DX12 swapchain is always created with `DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT` (`wgpu-hal-30.0.0/src/dx12/mod.rs:1477`), keeps the handle (`mod.rs:613`, `1681-1687`) and **exposes it**: `pub unsafe fn waitable_handle(&self) -> Option<HANDLE>` (`mod.rs:654-658`), reachable through `wgpu::Surface::as_hal::<hal::api::Dx12>()` (`wgpu-30.0.0/src/api/surface.rs:233-239`). Folio can call `WaitForSingleObject(handle, 0)` itself, **without forking wgpu-hal**. `desired_maximum_frame_latency` reaches `SetMaximumFrameLatency` and the buffer count `(latency + 1).min(16)` (`mod.rs:1501`, `1678-1679`), so latency 1 means two buffers.
3. **On DX12 the expired wait is silently ignored.** `SwapChain::wait` returns `Ok(false)` on `WAIT_TIMEOUT` (`mod.rs:1439`); `acquire_texture` propagates the `Result` (`mod.rs:1738`) but never maps `Ok(false)` to `SurfaceError::Timeout`, and proceeds to `GetCurrentBackBufferIndex` anyway. `SurfaceError::Timeout` is **not constructed anywhere under `src/dx12/`**; it is constructed on Metal only, when `nextDrawable()` returns `None` (`wgpu-hal-30.0.0/src/metal/surface.rs:332`).

Two corrections follow, and both audits and this note's own brief need them.

**Folio's `CurrentSurfaceTexture::Timeout` arm is unreachable on Windows** (`crates/bt-render/src/lib.rs:11155`). So is the `Occluded` arm — Folio's own comment already says so (`lib.rs:11158-11165`: "Only ever seen on macOS, where … `wgpu-hal`'s Metal surface reads `NSWindowOcclusionState`"). **The occluded fast path is macOS-only because the backend, not Folio, decides it**; DX12 will never say a window is invisible. The claimed "self-sustaining loop after a timeout-skip" therefore does not occur on Windows: a hidden window does not skip at all. It presents, into a compositor that is not consuming, and blocks — worse than the loop that was described.

**macOS `nextDrawable` is not bounded at one second.** wgpu-hal sets `setAllowsNextDrawableTimeout(false)` (`metal/surface.rs:285-288`) and ignores the caller's timeout entirely (`surface.rs:302`, `_timeout: … // TODO`). It can block indefinitely. `maximumDrawableCount` is `latency + 1` (`surface.rs:283`); `displaySyncEnabled` follows `PresentMode` (`surface.rs:223-227`, `289-291`), and Mailbox/FifoRelaxed are `unreachable!()` on Metal.

Present modes on DX12: `[Mailbox, Fifo]` always, `Immediate` only when the factory reports tearing support (`dx12/adapter.rs:1316-1319`) — computed **independently of the surface target**, so a DirectComposition visual advertises the same list and gets the same `FLIP_DISCARD` and the same `ALLOW_TEARING` flag (`mod.rs:1477-1486`, `1538-1582`). Mapping: Immediate → interval 0 + tearing, Mailbox → interval 0, Fifo → interval 1 (`mod.rs:1832-1838`); `FifoRelaxed` is `unreachable!()`. Folio configures `PresentMode::Fifo` with `desired_maximum_frame_latency = 1` at two sites: `crates/bt-render/src/lib.rs:4766` and `7690-7691`.

## 4. The options, smallest first

### A — do not present what nobody can see

**Mechanism.** One early return in the one funnel. `Runtime::present_conditions` (`crates/bt-app/src/main.rs:104363-104368`) **already computes the fact**: `window_shown && window.is_visible() == Some(true) && !window_hidden && window_exposed`. Today it feeds one decision only — `PresentGate::unchanged` (`crates/bt-app/src/present_gate.rs:77-82`), which *requires* `conditions.visible`, so an invisible window is never judged unchanged and always proceeds to acquire and present. A second decision goes in front of the gate test at `main.rs:104184`: when the window is not visible, return `Ok(None)` without touching the GPU and leave `gate.last` alone. The quake window while hidden, and a zero-sized surface, answer the same question the same way.

**What it buys.** It removes present attempts made into a compositor that is not consuming this window's swapchain — the condition under which Fifo/latency-1 has nothing to hand back. It is the only cheap candidate that could explain the *consecutive-turn bursts*, because a hidden window's compositor does not become ready between turns.

**Honest size of the prize: unknown.** Nothing recorded says which stalls were invisible (§2). It could be most of them or none. The trace named in §2 is what would say, and it should land first.

**What it costs.** Almost nothing, and the risks are named ones. `PresentGate`'s `visible` term exists so that "a hidden pre-clear cannot pay the first presentation after `ShowWindow`" (`present_gate.rs:26`; test `a_hidden_preclear_does_not_replace_the_first_visible_present`, `present_gate.rs:169`) — skipping earlier *strengthens* that property, because a skipped pre-clear writes no signature at all, and the obligation must be re-proved rather than assumed. The restore path must still repaint: `SkipUntilVisible` already exists as a policy (`crates/bt-render/src/lib.rs:4043`) and macOS already waits for the `Occluded(false)` that is the same bit read from the other side; Windows needs its own wake on show/restore/expose. The re-request loop is bounded by construction: the funnel's existing `unchanged` skip already returns `Ok(None)` without re-asking and demonstrably does not spin. Pacing, IME caret placement, `present_retained_picture` and `present_seats_and_commit` are untouched — no frame that reaches the glass today stops reaching it.

### B — a gate in front of acquire

**Mechanism.** Before acquiring, ask the compositor whether it has consumed the last frame, with a **zero** timeout: on Windows `WaitForSingleObject(waitable_handle(), 0)` through `as_hal` (§3 fact 2). If it has not, keep the composed picture pending, return to the event loop, and present on the next turn that finds the signal set. The pending frame is replaced by any newer one — one slot, never a queue.

**What it buys.** The one option that removes the *single long present* and the *`surface acquire` timeouts* from the input path, because both are entered only after a positive readiness answer. It also removes the bursts: a compositor stuck for two seconds produces deferrals, not three blocking turns.

**What it does not buy, plainly**: a signalled waitable does not *prove* that `Present` will return quickly. B bounds what we wait for **before** entering the driver; a present that blocks after a ready signal is still possible and would still appear as a stall line. Only D removes that residual.

**Ownership.** The deferral needs a deadline of its own, and the mechanism exists: `DEADLINE_OWNERS`, a 49-entry `[&str; 49]` table folded with a parallel array by `earliest_named_deadline` (`crates/bt-app/src/main.rs:105442`, `16418-16427`, folded at `105739`). "present retry" becomes entry 50, booked one display interval out. **Starvation** is the real hazard — a compositor that never becomes ready would defer for ever. The deadline answers it: after N consecutive deferrals (N a stated budget, not a tuning knob) the frame is presented anyway, blocking, so a wedged compositor degrades to today's behaviour rather than to a window that never paints.

**Pacing.** DESIGN §7.1.5p ⑬ makes `Runtime::next_animation_frame` "the last present plus one frame", and the fact it reads is `self.window.last_present_at` (`crates/bt-app/src/main.rs:85807-85811`). A **deferred present is not a present** and must not stamp that field, or the pacer would refuse frames that never reached the glass — the same defect class as the animation registry's stale copy in `pacing-review-codex-2026-09-18.md`. "The landing is never paced" (`FormulaToggleMotion::lands_at`; red gate `a_frame_composed_for_any_reason_carries_the_journeys_and_the_landing_is_never_paced`) survives unchanged: a landing still composes, and only its *presentation* may slip one interval.

**The hang watch should report a deferral, not a stall.** The report already carries a `run counters` section with a surface-acquire tally (`crates/bt-app/src/hang_watch.rs:2464-2472`); "presents deferred" joins it, and the stall line gains `deferred ×k`.

**macOS.** There is no equivalent readiness signal: `nextDrawable` blocks with the layer's own timeout disabled (§3), and `maximumDrawableCount`/`displaySyncEnabled` are the only levers. So **B ships on Windows only**; macOS keeps waiting on the drawable, which there is also the pacer.

### C — relax the queue

`desired_maximum_frame_latency` 1 → 2, or `PresentMode::Mailbox`. Latency 2 buys exactly one more buffer (`(latency+1).min(16)`, `dx12/mod.rs:1501`) — **one frame of slack against a stall of one to two seconds**, under one percent of the problem, paid for with a frame of input latency on every keystroke. Mailbox (interval 0) stops the wait *for vblank* but not the wait for a compositor that is not consuming, and on Windows the display's rate no longer paces this window anyway (§7.1.5p ⑬ moved pacing to `pace::FrameClock`), so Mailbox would mostly spend power. **C is listed in order to be refused.** It is a knob, not an answer, and `render-handoff-2026-09-16.md` §F's instruction to leave latency and present mode alone while ownership changes holds: an experiment that moves both at once explains nothing.

### D — a render/present thread

**The destination, already designed**: `docs/plans/design/render-handoff-2026-09-16.md` — a per-window presentation lane owning acquire, submit and present, with one pending CPU frame per window, one active frame and one process-wide preparation permit; the window thread keeps composition, the compositor's COM apartment and the native views.

**Why it is large here**, restated from that note rather than re-derived: the render pass needs the acquired view, so no finished command buffer can be queued before acquire (§A); device, queue, font system and glyph atlas live together and their frame lifetime is serial across windows (§B); every window's surface is presented from the one thread today; `configure` can wait for GPU idle and panics if a live `SurfaceTexture` exists (§C); `present_seats_and_commit` performs `set_covered_size` and the DirectComposition commit, which must stay on the window thread; WebView2 is composition-hosted and its COM calls are on that thread; and the Metal backend's `acquire_texture` walks the layer's delegate to `NSView.window` and sends `occlusionState` with no main-thread check (`wgpu-hal-30.0.0/src/metal/surface.rs:307-323`) — a **blocker for macOS, not a performance note**. Estimate there: 1,500–2,500 handwritten lines plus 600–1,000 mechanically extracted.

**The smallest seam**, as data crossing the boundary: a surface lease (an ownership token), a frame key (window epoch, device epoch, surface generation, sequence), a finished `CommandBuffer`, and a completion carrying the two timestamps. No `WindowRenderer`, no `Window`, no compositor, and no `SurfaceTexture` coming back.

## 5. Recommended sequence

I tested the expectation I was given and adopt it with one change: **A is smaller than it looks, but its prize is unmeasured, so the trace ships with it rather than after it.**

1. **0.4.3 — the visibility field on the stall line, and A.** The field is diagnostics only and settles §2's open question for every later step. A is one early return at `main.rs:104184` plus the restore wake; it touches no pacing, IME, atlas or compositor code. Default-off behind `BT_PRESENT_SKIP_HIDDEN`, read through `diagnostics::switched_on` (`crates/bt-app/src/diagnostics.rs:135`), which is the house shape — present and non-empty means on. It belongs in 0.4.3 only if the third adversarial audit is not thereby made to re-review the render path; if it is, both move to 0.4.4.
2. **0.4.4 — B, designed, spiked, then shipped.** Windows only. Default-off on the same switch shape (`BT_PRESENT_GATE`); the default flips to on, and the `=off` door is added, in the same release that flips it, once a real-machine recording shows deferrals replacing stalls.
3. **D — recorded as the destination, not scheduled.** It is the only fix for a present that blocks after a ready signal, and the only one for macOS; it carries a macOS dependency prerequisite that must be resolved before any of it lands. It does not belong in a patch release and must not be promised for 0.4.5, which is the updater's.

**B is achievable on wgpu 30 without forking wgpu-hal** (§3 fact 2) — on Windows. On macOS it is not: there is no readiness signal and the drawable timeout is disabled by the backend. What is achievable there short of D is a watchdog-bounded present on a helper thread that **owns only the present call** — the window thread composes and encodes as today, hands over the finished `SurfaceTexture` and command buffer, and never waits for the return. That is a strictly smaller cut of D, and is where macOS should start.

## 6. Evidence authority and acceptance

Pinned by tests, with no wall-clock time in them:

- **A**: `a_hidden_window_acquires_nothing` — the funnel driven with `PresentConditions { visible: false }` over a fake target counts zero acquires and zero presents, the counter being the reachability witness. `a_hidden_preclear_does_not_replace_the_first_visible_present` (`present_gate.rs:169`) stays green unchanged and gains a sibling asserting that a skipped hidden frame writes no signature. `a_window_that_becomes_visible_repaints_without_another_event`.
- **B**: the gate's decision as a **pure function of injected readiness** — `ready → present`; `not ready → defer and book "present retry" one interval out`; `k consecutive deferrals → present anyway`; `a newer frame replaces the pending one and the old one is counted coalesced, never queued`. Plus `a_deferred_present_does_not_stamp_the_pace_clock` (the §7.1.5p ⑬ obligation), and `the_landing_is_never_paced` re-run unchanged.

Only a real machine can show the rest, and exactly one line proves it. **Today** the 910 s beat produces `… swapchain present 1405 ms … thread CPU 15.625 ms / wall 1421 ms`. **After B**, a recording across at least three beats on the owner's machine must show, for the same beat, `presents deferred ×k` in the report's run counters and **no stall line above threshold whose largest station is `swapchain present` or `surface acquire` with `thread CPU ≈ 0`**. A stall line dominated by a CPU station is a different finding and does not block. Acceptance for A on a real machine is weaker and should be stated as such: over a session of ordinary use the new visibility field says what share of present stalls were invisible. A is worth keeping if that share is non-trivial, and worth keeping anyway if it is zero, because presenting into a window nobody can see is waste either way.

**Rollback.** Each step is one environment switch, default off, flipped only after the recording above: `BT_PRESENT_SKIP_HIDDEN` for A, `BT_PRESENT_GATE` for B. A default-on switch needs an `=off` door, which is a new shape beside `diagnostics::switched_on`; it is added in the release that flips the default, not before, so no shipped build ever carries an untested off path.

## 7. What I could not determine by reading

1. **What share of the recorded present stalls happened while the window was invisible or occluded.** Nothing recorded carries it. This is A's whole case, and it is currently unmade.
2. **Whether `Present` still blocks after the waitable has signalled**, on this machine, under the 910 s beat. This decides whether B is sufficient or only necessary. No reading answers it; a recording does.
3. **What triggers the 910 s beat.** Outside Folio and outside this note; the two store tasks that matched its clock were passengers — removing them changed nothing.
4. **Whether the DirectComposition visual changes DWM's consumption behaviour** versus an HWND swapchain. wgpu-hal treats them identically (§3), but DWM need not.
5. **Whether `swapchain present` time is spent in `IDXGISwapChain::Present` or in the DirectComposition commit that follows it** — the station brackets both. Splitting it is a small instrumentation change and should precede B.
6. **The cost of `as_hal::<Dx12>()` per frame**, and whether the handle may be cached across a surface reconfigure. The doc says the handle is valid only while the swap chain is alive (`dx12/mod.rs:654-656`); the lifetime rule against a Folio `configure` is not established by reading.
7. **Whether the third adversarial audit's scope already covers the render path**, which decides whether step 1 belongs in 0.4.3 or 0.4.4.
