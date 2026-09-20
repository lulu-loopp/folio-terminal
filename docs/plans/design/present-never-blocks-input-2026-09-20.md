# Present never blocks input: a budget for the compositor's answer

Status: **revision 2, 2026-09-20, after an adversarial review (verdict HOLD on options A and B as written).** Sections 1 to 7 are revision 1, left unedited so the review can be read against them; **§8 at the end supersedes them wherever they disagree.**

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

**Present is not the only unbounded wait on this thread, only the frequent one.** Device-loss recovery sleeps by design — `DeviceLossPilot::answer` rests 150 ms then 450 ms across three attempts (`crates/bt-render/src/lib.rs:3993-4022`, `3855`, `3865`) and production passes `std::thread::sleep` into it from the window thread (`crates/bt-app/src/main.rs:116601-116603`). That is deliberate and rare, and this note does not propose to change it; it is named here because the invariant below would otherwise appear to forbid it. The invariant is about *the compositor*, and a lost device is not the compositor being slow.

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

Three corrections follow, and both audits and this note's own brief need all three.

**Folio's `CurrentSurfaceTexture::Timeout` arm is unreachable on Windows** (`crates/bt-render/src/lib.rs:11155`). So is the `Occluded` arm — Folio's own comment already says so (`lib.rs:11158-11165`: "Only ever seen on macOS, where … `wgpu-hal`'s Metal surface reads `NSWindowOcclusionState`"). **The occluded fast path is macOS-only because the backend, not Folio, decides it**; DX12 will never say a window is invisible. The claimed "self-sustaining loop after a timeout-skip" therefore does not occur on Windows: a hidden window does not skip at all. It presents, into a compositor that is not consuming, and blocks — worse than the loop that was described.

**macOS `nextDrawable` is not bounded at one second.** wgpu-hal sets `setAllowsNextDrawableTimeout(false)` (`metal/surface.rs:285-288`) and ignores the caller's timeout entirely (`surface.rs:302`, `_timeout: … // TODO`). It can block indefinitely. `maximumDrawableCount` is `latency + 1` (`surface.rs:283`); `displaySyncEnabled` follows `PresentMode` (`surface.rs:223-227`, `289-291`), and Mailbox/FifoRelaxed are `unreachable!()` on Metal.

Present modes on DX12: `[Mailbox, Fifo]` always, `Immediate` only when the factory reports tearing support (`dx12/adapter.rs:1316-1319`) — computed **independently of the surface target**, so a DirectComposition visual advertises the same list and gets the same `FLIP_DISCARD` and the same `ALLOW_TEARING` flag (`mod.rs:1477-1486`, `1538-1582`). Mapping: Immediate → interval 0 + tearing, Mailbox → interval 0, Fifo → interval 1 (`mod.rs:1832-1838`); `FifoRelaxed` is `unreachable!()`.

**A third correction, and the sharpest of the three: a real Folio window does not run Fifo on Windows — it runs Mailbox, and nobody decided that.** `configure_window_surface` starts from `surface.get_default_config(...)` (`crates/bt-render/src/lib.rs:4746`) and overrides only format, alpha mode and `desired_maximum_frame_latency = 1` (`lib.rs:4766`). `get_default_config` takes `present_mode: *caps.present_modes.first()?` (`wgpu-30.0.0/src/api/surface.rs:97`) — and DX12 lists **Mailbox first** (`dx12/adapter.rs:1316`) while Metal lists **Fifo first** (`metal/adapter.rs:463-467`, Fifo being the only mode when `display_sync` is unavailable). So **Windows presents at sync interval 0 and macOS at interval 1, from the same source line**. The only explicit `PresentMode::Fifo` in the tree is the offscreen probe's configuration (`lib.rs:7690`), where present mode is inert; `desired_maximum_frame_latency = 1` genuinely has the two sites the brief names (`lib.rs:4766`, `7691`). Both audits, and this note's own brief, say Fifo on Windows. That is wrong, and it matters: the platform-shaped half of the swapchain's behaviour is inherited from whichever mode a backend happens to list first, while the one parameter Folio *does* decide is the one that makes the queue shallowest — `get_default_config` defaults latency to 2 and Folio narrows it to 1 (permitted range `1..=16` on DX12, `1..=2` on modern Metal: `dx12/adapter.rs:1358`, `metal/adapter.rs:450-460`). **The present mode is a second unowned fact, and it is unowned in the same way as the first.**

## 4. The options, smallest first

### A — do not present what nobody can see

**Mechanism.** One early return in the one funnel. `Runtime::present_conditions` (`crates/bt-app/src/main.rs:104363-104368`) **already computes the fact**: `window_shown && window.is_visible() == Some(true) && !window_hidden && window_exposed`. Today it feeds one decision only — `PresentGate::unchanged` (`crates/bt-app/src/present_gate.rs:77-82`), which *requires* `conditions.visible`, so an invisible window is never judged unchanged and always proceeds to acquire and present. A second decision goes in front of the gate test at `main.rs:104184`: when the window is not visible, return `Ok(None)` without touching the GPU and leave `gate.last` alone. The quake window while hidden, and a zero-sized surface, answer the same question the same way.

**What it buys.** It removes present attempts made into a compositor that is not consuming this window's swapchain — the condition under which two buffers have nothing to hand back. It is the only cheap candidate that could explain the *consecutive-turn bursts*, because a hidden window's compositor does not become ready between turns. That a hidden Windows surface really does go all the way through is not an inference: the code says so where it hides the window — "A hidden surface can either accept the clear or report Occluded. In the latter case `redraw()` republishes the frame; the second call presents immediately after `ShowWindow`" (`crates/bt-app/src/main.rs:39867-39868`). Accepting the clear *is* a present into an invisible window.

**Honest size of the prize: unknown.** Nothing recorded says which stalls were invisible (§2). It could be most of them or none. The trace named in §2 is what would say, and it should land first.

**What it costs.** Almost nothing, and the risks are named ones. `PresentGate`'s `visible` term exists so that "a hidden pre-clear cannot pay the first presentation after `ShowWindow`" (`present_gate.rs:26`; test `a_hidden_preclear_does_not_replace_the_first_visible_present`, `present_gate.rs:169`) — skipping earlier *strengthens* that property, because a skipped pre-clear writes no signature at all, and the obligation must be re-proved rather than assumed. The restore path must still repaint: `SkipUntilVisible` already exists as a policy (`crates/bt-render/src/lib.rs:4043`) and macOS already waits for the `Occluded(false)` that is the same bit read from the other side; Windows needs its own wake on show/restore/expose. The re-request loop is bounded by construction: the funnel's existing `unchanged` skip already returns `Ok(None)` without re-asking and demonstrably does not spin. Pacing, IME caret placement, `present_retained_picture` and `present_seats_and_commit` are untouched — no frame that reaches the glass today stops reaching it.

### B — a gate in front of acquire

**Mechanism.** Before acquiring, ask the compositor whether it has consumed the last frame, with a **zero** timeout: on Windows `WaitForSingleObject(waitable_handle(), 0)` through `as_hal` (§3 fact 2). If it has not, keep the composed picture pending, return to the event loop, and present on the next turn that finds the signal set. The pending frame is replaced by any newer one — one slot, never a queue.

**What it buys.** The one option that removes the *single long present* and the *`surface acquire` timeouts* from the input path, because both are entered only after a positive readiness answer. It also removes the bursts: a compositor stuck for two seconds produces deferrals, not three blocking turns.

**What it does not buy, plainly**: a signalled waitable does not *prove* that `Present` will return quickly. B bounds what we wait for **before** entering the driver; a present that blocks after a ready signal is still possible and would still appear as a stall line. Only D removes that residual.

**Ownership.** The deferral needs a deadline of its own, and the mechanism exists: `DEADLINE_OWNERS`, a 49-entry `[&str; 49]` table folded with a parallel array by `earliest_named_deadline` (`crates/bt-app/src/main.rs:105442`, `16418-16427`, folded at `105739`). "present retry" becomes entry 50, booked one display interval out. **Starvation** is the real hazard — a compositor that never becomes ready would defer for ever. The deadline answers it: after N consecutive deferrals (N a stated budget, not a tuning knob) the frame is presented anyway, blocking, so a wedged compositor degrades to today's behaviour rather than to a window that never paints.

**Pacing.** DESIGN §7.1.5p ⑬ makes `Runtime::next_animation_frame` "the last present plus one frame", and the fact it reads is `self.window.last_present_at` (`crates/bt-app/src/main.rs:85807-85811`). A **deferred present is not a present** and must not stamp that field, or the pacer would refuse frames that never reached the glass — the same defect class as the animation registry's stale copy in `pacing-review-codex-2026-09-18.md`. "The landing is never paced" (`FormulaToggleMotion::lands_at`; red gate `a_frame_composed_for_any_reason_carries_the_journeys_and_the_landing_is_never_paced`) survives unchanged: a landing still composes, and only its *presentation* may slip one interval.

**The hang watch should report a deferral, not a stall.** The counter's home already exists and has a shape: `SurfaceFailureTally` (`crates/bt-render/src/lib.rs:4076-4200`) is one atomic per failure kind, one `Display` impl that spells the whole tally in one wording, and a decade-throttled line at counts 10/100/1000 (`lib.rs:4154-4168`); the same tally is printed in the hang report's `run counters` footer (`crates/bt-app/src/hang_watch.rs:2464-2472`). "Presents deferred" is one more slot in that tally, not a new mechanism, and the stall line gains `deferred ×k`.

**What it does not cost.** The focus cards are not a second surface and not a second pass: `focus_thumb` builds one shrunk projection per seat and it is carried as ordinary chrome into the *same* window's single present (`crates/bt-app/src/focus_thumb.rs`, `crates/bt-app/src/seats.rs:8798`, wired at `main.rs:41965`). A deferred present defers the cards with the frame they ride on, and nothing needs to know about them. `present_retained_picture` (`main.rs:104499-104639`) is likewise unaffected in kind: it already funnels through the same door and would simply be deferred like any other frame.

**macOS.** There is no equivalent readiness signal: `nextDrawable` blocks with the layer's own timeout disabled (§3), and `maximumDrawableCount`/`displaySyncEnabled` are the only levers. So **B ships on Windows only**; macOS keeps waiting on the drawable, which there is also the pacer (Fifo, §3).

### C — relax the queue

**Mailbox is not available as a remedy on Windows, because it is already what runs there** (§3). That disposes of half of this option before it is argued. What remains is `desired_maximum_frame_latency` 1 → 2, which buys exactly one more buffer (`(latency+1).min(16)`, `dx12/mod.rs:1501`) — **one frame of slack against a stall of one to two seconds**, under one percent of the problem, paid for with a frame of input latency on every keystroke. Going the other way, to `Immediate`, would add tearing on a composition visual for the same negligible slack. **C is listed in order to be refused.** It is a knob, not an answer, and `render-handoff-2026-09-16.md` §F's instruction to leave latency and present mode alone while ownership changes holds: an experiment that moves the swapchain and the ownership at once explains nothing. The one thing §3 does earn C is a *question* for later, not a change now: latency 1 was chosen deliberately against a default of 2, and whether that choice is still right under Mailbox on Windows is worth re-deciding once the fact has an owner.

### D — a render/present thread

**The destination, already designed**: `docs/plans/design/render-handoff-2026-09-16.md` — a per-window presentation lane owning acquire, submit and present, with one pending CPU frame per window, one active frame and one process-wide preparation permit; the window thread keeps composition, the compositor's COM apartment and the native views.

**Why it is large here**, restated from that note rather than re-derived: the render pass needs the acquired view, so no finished command buffer can be queued before acquire (§A); `configure` can wait for GPU idle and panics if a live `SurfaceTexture` exists (§C); WebView2 is composition-hosted in the same visual tree (`crates/bt-platform/src/lib.rs:4042-4063`) and its COM calls are on the window thread; and the Metal backend's `acquire_texture` walks the layer's delegate to `NSView.window` and sends `occlusionState` with no main-thread check (`wgpu-hal-30.0.0/src/metal/surface.rs:307-323`) — a **blocker for macOS, not a performance note**. Estimate there: 1,500–2,500 handwritten lines plus 600–1,000 mechanically extracted.

Two of those costs are worth naming in this codebase's own terms, because they are what makes "just move present to a thread" untrue. **The ownership split is not per-window.** `WindowRenderer` (`crates/bt-render/src/lib.rs:4838`) is per window and owns the surface and its config; `GpuContext` (`lib.rs:4242`) is one per process and owns the device, the queue, the font system and **one shared glyph atlas** that every window's text renderer draws from — growing it for one window moves coordinates already handed to another (`lib.rs:8904-8918`). Every present therefore takes `(&mut GpuContext, &mut WindowRenderer)` together (`crates/bt-app/src/main.rs:104162-104169`), and two windows presenting concurrently is a change to the atlas's lifetime, not just to a thread. **And `present_seats_and_commit` must keep the commit**: wgpu's DX12 backend calls `IDCompositionVisual::SetContent` without `Commit` (`wgpu-hal-30.0.0/src/dx12/mod.rs:1619`), so Folio itself publishes the frame with `set_covered_size` + `commit()` right after `queue.present()` (`main.rs:104136-104145`, `104248-104260`) — that call is what makes the picture appear, and it belongs to the window thread's COM apartment.

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
5. **Why `Present` blocks at all under Mailbox with two buffers.** At sync interval 0 the call should queue and return; the recorded 655–2332 ms says DWM is not releasing a buffer. Whether that is the composition visual, the shared-with-WebView2 visual tree, or the machine's own beat is not answerable by reading. (It is *not* the DirectComposition commit hiding inside the station: `Station::SwapchainPresent` brackets `wgpu::Queue::present` alone — `crates/bt-app/src/hang_watch.rs:735`, `crates/bt-render/src/lib.rs:10225` — and Folio's own `compositor.commit()` is separately stationed and appears as `compositor commit 1 ms` on the stall lines.)
6. **The cost of `as_hal::<Dx12>()` per frame**, and whether the handle may be cached across a surface reconfigure. The doc says the handle is valid only while the swap chain is alive (`dx12/mod.rs:654-656`); the lifetime rule against a Folio `configure` is not established by reading.
7. **Whether the third adversarial audit's scope already covers the render path**, which decides whether step 1 belongs in 0.4.3 or 0.4.4.

## 8. Revision 2 — what the review changed (this section rules)

An adversarial review (read-only; report at `scratchpad/design043/present-design-review-codex.md`) returned **HOLD on A and B as written**, with five blocking obligations. I re-verified every cited line in this worktree before accepting any of it; all five stand, two of them on counterexamples that are decisive. Where the review was itself wrong, that is said below. The repository's stop rule applies: a finding changes the design when it is reachable in ordinary use and breaks a hard requirement or the headline scenario; the rest is recorded.

### 8.1 Corrections of fact

**In revision 1:**
- **"`SurfaceError::Timeout` … is constructed on Metal only" is false.** It is also constructed at `wgpu-hal-30.0.0/src/noop/mod.rs:215` and `vulkan/swapchain/native.rs:455,476`. True as it should have been written — *among the backends Folio ships* — and with no consequence for the design, since Folio ships neither noop nor Vulkan.
- **"`surface acquire 1022 ms` is that constant expiring" was stated as fact and is an inference.** A station's elapsed time is not a recorded `WAIT_TIMEOUT`. CONVENTIONS rule 2 requires observation and hypothesis to be written in different places; it is marked **[inferred]** from here on, and §8.7's diagnostics exist so that nothing has to be inferred from a duration again.
- **"B removes the single long present" overclaims**, two sentences before revision 1 itself admits the residual. Corrected in §8.5.
- **The rollback doors would have turned the features on.** `diagnostics::switched_on` is "present and non-empty" (`crates/bt-app/src/diagnostics.rs:135-136`), so `BT_PRESENT_GATE=off` and `=0` both read as **on**. See §8.7.
- **"A present-only helper thread is where macOS should start" is withdrawn.** On Metal the blocking call is `nextDrawable` inside *acquire*, not present (`metal/surface.rs:302`, `327-332`, with the layer's own timeout disabled at `285-288`). A helper owning only `present` moves nothing. `render-handoff-2026-09-16.md` §C already names what a macOS answer needs: the acquire handoff, the AppKit prerequisite and the shutdown ownership.

**In the review:** it writes that `wgpu-types-30.0.0/src/instance.rs:67-73` "does not read environment overrides" and concludes that no environment door exists. wgpu **does** have one — `InstanceDescriptor::with_env` (`instance.rs:106-117`) chains to `Dx12UseFrameLatencyWaitableObject::from_env` (`backend.rs:883-890`), which reads `WGPU_DX12_USE_FRAME_LATENCY_WAITABLE_OBJECT`. The review's *conclusion for Folio* is nonetheless right, for a different reason I verified: Folio builds every instance with `InstanceDescriptor::new_without_display_handle()` and never calls `with_env` (`crates/bt-render/src/lib.rs:6322` `GpuContext::open`, `6444` `rebuild_after_device_loss`, `6635` `headless_on`). That matters twice — those three sites are where the option must be set, and `with_env` is the **wrong** door, because it would admit every other wgpu variable at once.

### 8.2 O1 — a suppression is never a presentation

Verified, and it is the sharpest finding: **`None` is matched in the same arm as `Presented`** — `crates/bt-app/src/main.rs:104854`, `outcome @ (Some(PresentOutcome::Presented(_)) | None) =>`. `None` is safe there **only** because the one thing that produces it today is the unchanged gate, which requires `conditions.visible` and an identical signature, so the glass really does hold that picture. Revision 1's `Ok(None)` for a hidden window with *changed* content breaks that precondition, and it is a wrong-picture bug.

**The debt an A-shaped suppression must leave standing**, enumerated from both callers:

| # | What that arm does today | Site |
|---|---|---|
| 1 | `presented_picture_revision = terminal_content_revision` — the licence the animation path reads before answering a tick from the screen | `main.rs:104873` |
| 2 | `unpainted_pane_output = false` | `104878` |
| 3 | first-visible-present DPI reconciliation marked done | `104879-104882` |
| 4 | every painted pane's `last_presented_frame` replaced, and `mark_leaf_painted` | `104921-104931` |
| 5 | the window's own `last_presented_frame` replaced | `104932` |
| 6 | `rescan_pane_references` re-derives the pointer's reference list **from cells that never reached the glass** | `104937-104939` |
| 7 | `pending_resize_present = None` — admitted resize debt discharged | `104940` |
| 8 | `textless_frames = 0` | `104863` |
| 9 | `chrome_present_pending`, cleared at entry to `present_retained_picture` and **not restored** by its `None` arm | `104501`, `104595` vs `104629` |

Two things are already right and must stay so: `device_loss_pilot.a_frame_reached_the_glass()` is guarded by `receipt.is_some()` (`104867`), and `last_present_at` — the pacer's fact — is stamped only inside `trace_present`, which is reached only with a receipt (`104315-104320`, `104884`), so a skip already cannot poison the pacer.

**The mechanism.** A does not return `Ok(None)`; it returns a new `PresentOutcome::SkippedHidden` beside `SkippedNotVisible` (`crates/bt-render/src/lib.rs:1657-1712`), mapping to `SurfaceFailurePolicy::SkipUntilVisible` (`lib.rs:4043`). One refinement of the review, which I checked: **all four non-`Presented` outcomes already preserve the debt** — the frame is re-filed by `pending_frames.publish` in every one of them (`main.rs:104961-104964`). What is unique to `SkippedNotVisible` is that `ask_again_after` returns `false` for it alone (`main.rs:113182-113215`), declining to ask for the turn that would pay the debt. A needs both properties, which is exactly that variant's shape. Both owed arms enumerate their variants explicitly, so a new variant is a compile error at every site until answered — the property this repository prefers to a hand-kept list. The existing test cannot stand in for the new ones: `a_hidden_preclear_does_not_replace_the_first_visible_present` (`present_gate.rs:169-175`) only exercises `PartialEq` on two signatures and would not notice a skip credited as glass.

### 8.3 O2 — which invisibility facts may veto a present

The three-probe predicate is removed from hard admission. It fails on two independent grounds, and the second is the one I would put first.

**It is not a proof.** `exposure_probe_points` (`crates/bt-platform/src/lib.rs:7155-7176`) samples the centre and two quarter-insets — all three inside the window's central half — and `exposed_from_probe` (`7209-7220`) answers false when all three miss. A window covering the middle half of Folio covers all three while a wide border and the whole tab strip stay visible. The function's own doc already says it: "**It is three samples and not a proof.**"

**Its errors are deliberately biased the wrong way for this use.** The same doc records a user ruling of 2026-09-01: a window whose rectangle cannot be read "is reported exposed", because "of the two wrong answers, one leaves the reader with the marks inside the window they are looking at, and the other puts a toast on a desktop they can see. Only the second is an interruption." That is a *notification* cost model. Promote the predicate to a presentation veto and a false negative stops costing a suppressed toast and starts costing a frozen window. **A predicate may not be reused across a change in the cost of being wrong** — and no added wake cures it, because the predicate itself is what is false. Two further facts close it: winit's `Occluded` is unsupported on Windows (`winit-0.30.13/src/event.rs:421`), so nothing tells Folio the cover moved; and `window_exposed` is a cached field refreshed on only three paths (`main.rs:84767-84771` per drain turn, `85489-85491` behind the animation pacer, `117503-117506` on an attention event), so a quiet window can hold a stale sample indefinitely.

**Authoritative on Windows, and adopted** — each read **fresh at the attempt**, never from a cached sample:
- **Minimised** — `IsIconic` (`platform:6943-6946`). Wake: `WM_SIZE` always produces `Resized` (`winit/platform_impl/windows/event_loop.rs:1397-1419`) and does not distinguish minimise, so the veto must be **re-read** on that event, never remembered.
- **Zero client extent** — the current client rectangle at the attempt. Not part of `PresentConditions` today at all (`present_gate.rs:46-50`), and both resize paths already skip zero and leave the old config live (`main.rs:101481-101485`, `render:8428-8438`). Wake: a nonzero `Resized`.
- **Hidden by Folio itself** — `window_shown`, and the quake window's `set_visible(false)` (`main.rs:39853-39858`). Folio wrote it, so Folio owns it; the show path already publishes and redraws both before and after `ShowWindow` (`39862-39892`).

**Not adopted, and recorded as such:** occlusion by another window (above); **cloaked** — `DwmGetWindowAttribute(DWMWA_CLOAKED)` is read (`platform:6967-6998`) but no `EVENT_OBJECT_UNCLOAKED` subscription exists anywhere in `crates/` (verified by search), so without a named owner-thread wake it cannot be a lasting veto; **monitor asleep, session locked, RDP disconnected** — no `WM_POWERBROADCAST`, `GUID_SESSION_DISPLAY_STATUS`, `WM_WTSSESSION_CHANGE` or `WTSRegisterSessionNotification` registration exists either. All are carried in the diagnostics of §8.7 and acted on by nothing.

One implementation consequence: `window_hidden` today is `is_window_minimized() || is_window_cloaked()` fused into one bit (`main.rs:25219`). **A cannot adopt minimised without splitting that bit**, since one half is authoritative and the other is not.

**And the rule that covers everything not listed: unknown always permits rendering.** A reading that fails, a rectangle that cannot be had, a state with no wake — all present. The veto carries the burden of proof; the frame never does.

### 8.4 O3 — B's mechanism, corrected

**Revision 1's gate would have created the stall it removes.** The frame-latency waitable is a semaphore, and `WaitForSingleObject(h, 0)` is a **consuming** wait, not a peek. Under Folio's current setting — `Dx12UseFrameLatencyWaitableObject::Wait`, which is `#[default]` (`wgpu-types-30.0.0/src/backend.rs:860-873`) — an external poll consumes the credit, and `get_current_texture` then waits again on a semaphore with nothing left, for up to the hard 1000 ms, on a frame that was ready.

The corrected mechanism:
- **`DontWait` is required**, not optional (`backend.rs:869-872`; the dispatch that skips the internal wait is `wgpu-hal-30.0.0/src/dx12/mod.rs:1734-1740`). Set explicitly at all three instance sites (`render:6322`, `6444`, `6635`), never through `with_env`.
- **`DontWait` is not a neutral baseline.** With it wgpu waits for nothing, so the frame-latency back-pressure disappears and Folio inherits it. The backend option and the gate are therefore **one switch, not two**, and any rollback must restore `Wait` on every fresh *and* rebuilt context — `rebuild_after_device_loss` is its own site.
- **One readiness-credit owner per surface generation.** Poll only once a present is actually owed. A successful poll funds exactly one `Present`, or is retained across a recoverable pre-present failure, or is retired with its chain. Never poll again because an unchanged, hidden or coalesced path returned early. A missing or failed handle is its own outcome, not "not ready".
- **The handle may not be cached across `configure`/`unconfigure`.** `configure` takes the old swapchain and calls `release_resources`, which frees the handle (`dx12/mod.rs:1503-1508`, `1416-1422`), then obtains a new one (`1681-1685`); `unconfigure` frees it too (`1710-1724`). `as_hal`'s guard retains the surface, not a configuration (`wgpu-30.0.0/src/api/surface.rs:224-229`). Re-fetch per generation.
- **Ordering against configure is an open cost, not a detail.** The renderer configures immediately before acquire (`render:9705-9711`). A gate outside that can consume old-generation credit; a gate inside leaves `configure`'s own wait-for-GPU-idle on the input path (`wgpu-30.0.0/src/api/surface.rs:103-106`). Neither is free; the spike measures which.
- **Device loss outranks all of it.** The unchanged skip is already disabled when loss is latched (`main.rs:104184`) and the gate must be too; the loss callback only records a latch (`render:6208-6219`), so the gate must not hold a dead handle or carry old identities into a rebuilt surface.

### 8.5 O4 — what B promises, and what it cannot

**Force-a-present-after-N-deferrals is withdrawn.** It restores exactly the multi-second block the invariant forbids, and a deadline can schedule an attempt but cannot bound `Present`.

- **The promise, entire: the input path never *waits* for readiness.** Not that a picture arrives.
- **Not promised: that `Present` returns promptly once readiness was signalled.** There is no non-blocking present on this backend — neither `DXGI_PRESENT_DO_NOT_WAIT` nor `DXGI_PRESENT_TEST` appears anywhere in wgpu-hal's DX12 path (verified by search), and Mailbox's sync interval 0 is not a guarantee. Only D closes this.
- **Under long starvation the picture goes stale and stays stale**, while input, PTY drain and document landings stay live. That is the trade, stated plainly: a window that is behind is better than a window that is deaf. It is not silent — §8.7's freshness line says so — and whether the *user* is told is a question for the owner, not one this note settles.
- **A landing is not bounded by one interval.** Revision 1 said a landing "may slip one interval"; under repeated deferral that is false. §7.1.5p ⑬ already makes a landing a document operation with its own unpaced deadline, and it must stay one for an arbitrarily long deferral; the owed endpoint picture is tracked separately from any frame.
- **IME must be specified, not assumed untouched.** The caret is offered from the just-composed frame **before** slot publication (`main.rs:67343-67359`), through a throttle (`62415-62426`), into the native candidate anchor and the system caret (`84433-84441`). It is coupled to *publish*, not to a successful present — so a long deferral leaves candidates anchored to text that is not on screen. The spike names which picture owns anchoring during deferral and carries a delayed-present IME test.
- **Budgets are aggregate.** Windows are serial on the one thread (`main.rs:113873-113885`), so per-window waits add. Budget total attempts and preparation across all windows, and require **zero polling when nothing is owed** (CONVENTIONS rule 5: a quiescent subsystem's budget is zero).
- **Resize and uploads.** An admitted resize frame may not be erased by "newest replaces anything" (`render-handoff-2026-09-16.md` §B's protected-resize rule), and a deferral after uploads must retain or drain them and close the atlas obligation the way the failure path already does (`render:11081-11085`).

### 8.6 O5 — acceptance that can see a frozen picture

Revision 1's criterion — "no stall line above threshold whose largest station is present" — **is passed by a build that renders nothing**, and the tally it leaned on is process-wide (`render:4066-4072`) and printed inside a hang report that a frozen-but-responsive window never produces. It is replaced by the two lines specified in §8.7, both emitted **independently of any hang**: a per-attempt line that attributes every attempt, and a freshness line that fires precisely when the picture is old while the loop is healthy. Acceptance additionally requires a **fresh baseline recorded before any suppression is enabled**, both switch branches exercised, and — for B — a rollback test proving backend `Wait` is restored on a rebuilt context.

### 8.7 Sequence and switches, revised

1. **0.4.3 — diagnostics only**, and only if the final-tree audit can take them. Nothing suppresses, defers or reorders a present. Plus one recorded fact with no owner yet: **Windows runs Mailbox by inheritance** (§3), to be given an owner in 0.4.4.
2. **0.4.4 — A redesigned** per O1/O2 and landed behind its counterexample tests (false-negative exposure, hidden changed content, shell-less retained debt, first show and quake restore, minimised and zero-extent restore, no false acknowledgment, no retry spin, an automatic fresh frame on restore). **B spiked** in the same release with `DontWait` and a **consuming-credit** fake — not injected booleans — covering first frame, unchanged frame, generation change, failure after consumption, loss and rebuild, protected resize, IME, and several real windows. Default off.
3. **D remains the destination**, scheduled against the invariant only if post-readiness `Present` still blocks — which B cannot certify.

**The per-attempt line** (`BT_PERF_TRACE`, default off, joining the existing family at `main.rs:104339`), one per attempt including those that present nothing. Native observations are kept strictly apart from the attention heuristic, and **no field infers a cause from a duration**:

```
BT_PERF_TRACE attempt win=<id> gen=<surface_gen> seq=<n> src=<Keyboard|PtyOutput|Resize|Expose> retained=<0|1>
  outcome=<presented|unchanged|without_text|skipped|not_visible|hidden|reconfigure|failed:<kind>>
  mode=<Mailbox|Fifo|Immediate> latency=<n> wait=<Wait|DontWait|None>
  native_iconic=<0|1|unknown> native_cloaked=<0|1|unknown> native_client=<W>x<H> native_style_visible=<0|1>
  folio_shown=<0|1> attention_exposed=<0|1> attention_age_us=<n>
  configure_us=<n> acquire_us=<n> encode_us=<n> submit_us=<n> present_us=<n> commit_us=<n>
  since_last_present_us=<n> pending_age_us=<n>
```

**The freshness line**, which is the one that catches a responsive window showing a stale picture. Emitted when a window's picture age first crosses a threshold, at decades after, and once more when a picture finally lands:

```
Folio: window <id> has shown no new picture for <n> ms — last present <n> ms ago
  (gen <g>, seq <n>, outcome <o>); <k> attempts since, <r> of them <reason>;
  the window thread dispatched <m> events and turned <p> times in that span
```

The last clause is the whole point: it separates *frozen picture, live input* from *frozen everything*, which is the distinction revision 1's acceptance could not make.

**Switches.** 0.4.3's diagnostics are default-off through `diagnostics::switched_on`, which is the right shape for a default-off switch and needs no new code. **Any later default-on feature needs a real off door, and `switched_on` cannot be it** — it reads `off` and `0` as on. That parser is new work with its own table (unset, empty, `0`, `off`, `false`, `no`, `1`, `on`, mixed case), and it lands in the release that flips the default, not before, so no shipped build ever carries an untested off path.

### 8.8 Still undetermined

Revision 1's list stands — item 5, why `Present` blocks at all under Mailbox with two buffers, is still the deepest of them — less its item 6 (the handle may not be cached; §8.4) and item 7 (settled by the sequence in §8.7). Added:
1. **Which invisibility state actually accompanied the recorded stalls.** Unchanged from revision 1, and now the entire job of 0.4.3.
2. **Whether `IsIconic` + zero client extent + Folio's own hidden bit covers enough of the hidden cases to make A worth doing at all**, once the three-probe heuristic is out of the veto. What remains may be nearly nothing, and the diagnostics will say so before anything is built.
3. **Whether the gate belongs before or after `configure`**, given that each placement has a wait of its own.
4. **What wakes a cloaked window**, if cloaking is ever to become a veto.
