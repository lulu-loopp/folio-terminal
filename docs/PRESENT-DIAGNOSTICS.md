# Presentation diagnostics (0.4.3)

These instruments observe the existing presentation path. They do not suppress,
defer, retry, pace, or change the mode or backend options of any presentation.
A landing means that the queue present and Folio's composition commit returned
successfully. Neither API acknowledges physical scanout.

## Attempt trace

With `BT_PERF_TRACE` set to a nonempty value, each redraw or retained-picture
attempt writes one additional `stderr` line. A redraw that delegates to the
retained path counts once. A bare redraw with nothing to draw says `no_picture`.
Unchanged frames, early returns, renderer failures, and commit failures also
write a record. Existing trace lines remain available.

```text
BT_PERF_TRACE attempt win=7 gen=3 seq=11 src=Resize retained=0 outcome=presented mode=Mailbox latency=1 wait=Wait native_iconic=0 native_cloaked=0 native_client=1280x800 native_style_visible=1 folio_shown=1 attention_exposed=1 attention_age_us=4200 configure_us=20 acquire_us=15 encode_us=4000 submit_us=30 present_us=1400000 commit_us=100 since_last_present_us=1416000 pending_age_us=1404165
```

`win` is the numeric winit window ID. `seq` increases per window, including
attempts that produce no image; `gen` is the renderer's surface generation.
Reconfiguration advances it. The actual frame trigger supplies `src`; the
retained path's existing trigger is `Expose`. `outcome` is `presented`,
`unchanged`, `without_text`, `skipped`, `not_visible`, `reconfigure`,
`no_picture`, `failed:render`, or `failed:commit`. Error categories never include
the error message. A failure in caller preparation is `failed:render`.

`mode` and `latency` are read from the renderer's current surface configuration.
`wait` is read from the DX12 option captured from the descriptor actually handed
to `Instance::new`, including device rebuilds; it is `None` on other backends.
Windows currently inherits Mailbox from `get_default_config`; this instrument
does not select it. There is no external readiness wait.

`native_iconic`, `native_cloaked`, `native_client`, and
`native_style_visible` are fresh native readings taken only when writing a line.
An unreadable value is `unknown`, including all four Windows facts on macOS.
Style visibility is not compositor occlusion. `folio_shown` is Folio's own show
state. `attention_exposed` is the existing cached notification heuristic,
accompanied by `attention_age_us`; an unsampled cache has age `unknown`.
Neither attention nor native facts change presentation admission. The existing
`window_hidden = minimized || cloaked` consumers retain their existing values
and short-circuit behavior.

All durations are microseconds. Renderer phase callbacks bracket actual
configuration (including recovery), acquire, queue submit (including the empty
failure-path upload drain), and queue present. `encode_us` accumulates the
renderer work outside those boundaries: preparation, shaping, atlas uploads,
layout, command encoding, and cleanup. `commit_us` brackets covered-size update
and composition commit together. An unvisited phase is zero. A returning error
closes the phase it was in. These are elapsed observations, not CPU timings or
evidence of a timeout, readiness result, or DWM cause.

`since_last_present_us` is measured against the last successful present plus
commit, including textless pictures, and is zero before the first landing.
`pending_age_us` starts when outstanding picture debt is first observed; replacing
the pending frame does not restart it. The finish sample is the successful
commit's return time, or the failed/empty attempt's return time.

## Picture freshness

Always on, independent of `BT_PERF_TRACE` and hang detection, this writes directly
to `diagnostics.log` through `diagnostics::note`, even when traces use the console.

```text
Folio: window 7 has shown no new picture for 1000 ms — last present 1016 ms ago (gen 3, seq 11, outcome not_visible); 8 attempts since, 8 of them not_visible; the window thread dispatched 42 events and turned 19 times in that span; last_landed_gen=3 last_landed_seq=3; native_iconic=0 native_cloaked=0 native_client=1280x800 native_style_visible=1
```

One second is deliberately well above normal frame pacing but within the
reported one-to-two-second freezes. The next thresholds are 10, 100, 1,000
seconds, and so on. Only the highest newly crossed decade is reported when a
turn arrives late. A subsequent landing writes the same sentence with
`; a picture landed`, then ends that episode. A single blocking attempt can
produce both the crossing and landing lines when it returns.

The check uses existing window turns and attempt completion. It adds no timer,
deadline owner, polling loop, or wake. A blocked thread cannot run it until it
returns; an entirely idle loop writes nothing. No outstanding picture debt means
no stale-picture line, however old the last present is. Debt is observed from
the pending frame, pending chrome, unpainted pane output, admitted resize, or an
attempt already spending that debt. Visibility is Folio's shown flag and its
minimized knowledge from the native minimize reading the resize path already
makes. Cloaking and the exposure heuristic do not gate this line.

Counts cover the debt episode. The reason reported is the most frequent attempt
outcome in that span (`none` when there have been no attempts). The parenthesized
generation, sequence, and outcome identify the latest attempt; `last_landed_gen`
and `last_landed_seq` identify the previous successful landing. An absent prior
landing prints `last present unknown ms ago` and IDs zero. Dispatch progress
counts the window thread's winit window-event and user-event callbacks across
all windows; turns count its `about_to_wait` callbacks. Thus activity in another
window still proves that this shared input thread was live.

## Joining a stall

Present stations in the hang watch's fixed call tree carry
`[win=7 gen=3 seq=11 outcome=in_progress:present]` (or the corresponding prepare,
configure, acquire, encode, submit, commit, or acknowledge stage). The identity
survives return from the attempt and distinguishes multiple windows/attempts
inside one slow turn. A long-hang report and its announcement also carry the
active attempt when available. `in_progress` records the operation being
observed; it does not predict its final outcome. Detail-capacity overflow remains
explicit and falls back to the existing coarse ledger.

Both new line families contain only numeric IDs, counters, native facts, and
fixed enum vocabulary. They never contain paths, titles, document content,
keystrokes, or error text.
