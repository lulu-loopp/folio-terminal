# Internal file-row drag review — 2026-09-17

Reviewed `8b947d6a8bc6accc810e0bee78d38f82f2053139` against `ad7f67af`.
All repository line references below are at the reviewed commit; `main.rs`, `seats.rs`,
`profiles.rs`, `shell_literal.rs`, and `input.rs` mean `crates/bt-app/src/<file>`.
Scope: the six requested attacks, in order. Product code remained read-only.

**Verdict: merge with must-fixes. The 0.4.2 bar is NOT met by this commit.**
The new PTY write consumes an old landing, which can name a terminal the release did not aim at.
Fix the release target/identity and refusal checks below before merging into 0.4.2;
otherwise hold this feature for 0.4.3. Sharing the quoting function does not establish safe targeting.

## 1. Wrong-pane delivery and the preview's promise

**P1 — revalidate the release point and bind the destination to its tab/session.**
`main.rs:89695` stores the survey result in `drag.landing`; `main.rs:29435` explicitly calls it
the last pointer move's answer. Mouse-up routes cached pointer state (`main.rs:92965`) to
`release_drag` (`main.rs:91189`), which takes the drag and reads that landing (`main.rs:89951`,
`main.rs:89974`). It does not query the release point or call `survey_drop` again.
`plan_inputs_for` retains the old landing while cloning the current tree (`main.rs:45976`).
`row_verb` constructs `PastePath(target)` at commit time (`main.rs:90067`, `main.rs:29337`),
but **the target number was selected at hover time**. `main.rs:90087` writes to that number.

Source-traced failure sequence: survey terminal B's centre; move to terminal A's centre without
an intervening delivered motion; release. The valid new plan is still for B, and B gets the bytes.
The last box and commit can agree with each other while both disagree with the release position.
If a newer motion was handled but its frame was not presented, commit can also name A while
the last visible box still named B: there is no check against a presented preview.
The hover update only requests a redraw (`main.rs:89730`, `main.rs:88451`).
`dock_overlay_layers` reads the shown inputs (`main.rs:46160`); commit reads the drag inputs.

Closing B alone is safe: `seats.rs:1582` rejects a missing seat, and `main.rs:97924` has no
focused-terminal fallback. Normal same-tree allocation increments `next_seat` (`seats.rs:672`,
`seats.rs:686`), so closing a pane does not immediately recycle its id.
Closing a sibling/reflowing while B survives is unsafe: close updates geometry and chrome hover
(`main.rs:54024`, `main.rs:53581`), not `drag.landing`; B can move away from the release point.

An active-tab close is worse: row drags have no `drag.tab()` (`main.rs:29594`), so the close
cancellation at `main.rs:39527` misses them. `main.rs:39583` activates the adjacent tab;
`seats.rs:454` shows ids restart at 1 in independent trees. A cached centre id can therefore
resolve to a different terminal in that tab, and `main.rs:97923` uses its session map.
This can happen without pointer motion when shells exit (`main.rs:98969`).
Required regression cases: missed final motion, surviving target moved by reflow, vanished target,
and active-tab closure with overlapping seat numbers. Resolve the physical release against the
current layout, retain/validate tab and session identity, and refuse uncertain or changed offers
instead of silently redirecting a text write. Do not merely refresh the plan around the old id.

**P1 — honor a refused content plan before pasting.** `seats.rs:1592` can return a content plan
whose layout is absent because it does not fit. `main.rs:46168` then suppresses the caption,
and `seats.rs:21154` renders a refusal. Nevertheless, `main.rs:90085` pastes before any
`plan.fits()` check. Force the viewport below the layout minimum after hover, then release:
the refused offer can still write. Guard this arm with the same acceptance condition used to
show `Paste path`, and cover a non-fitting content plan. The common verb table alone is insufficient.

## 2. Alternate screen, running child, and bracketed paste

No external-only busy-PTY gate was found. One level above delivery, `flush_dropped_files`
selects a seat and calls `paste_paths_into` directly (`main.rs:96021`); its entry merely collects
paths (`main.rs:113364`, `main.rs:96054`). Neither tests foreground-child or alternate-screen state.
Both routes inherit recipient encoding/refusal notices (`main.rs:97927`, `main.rs:97947`) and
the actual recipient's bracketed-paste mode (`main.rs:113955`, `input.rs:796`). Without that mode,
the child receives ordinary sanitized bytes. There is no guarantee an interactive tool will treat
them as inert shell text. This is shared existing behavior, not an internal bypass; “nothing is run”
in `CHANGELOG.md:20`/`docs/DESIGN.md:1028` must not imply a guarantee about a running child's reaction.

## 3. Quoting parity

For the same destination, parity holds by construction: both drops call `paste_paths_into`,
which reads that seat's spawn-captured `paste_recipient` (`main.rs:97923`, `main.rs:34592`).
It calls the clipboard preparer (`main.rs:113925`); neither source rereads keyboard focus or
chooses a separate profile. WSL spelling/`paste_paths_as` are in `shell_literal.rs:176`;
cmd refusal/quoting and POSIX quoting are at `shell_literal.rs:136` and `shell_literal.rs:113`.
Zsh derives POSIX grammar (`profiles.rs:901`), with recipient overrides captured at `profiles.rs:940`.
Thus WSL/cmd/macOS zsh have different encodings from PowerShell, but no source-dependent divergence.
The new test (`main.rs:170572`) only exercises PowerShell recipients and calls the same preparer
twice; its companion at `main.rs:170632` checks source text, not event delivery. Neither catches
section 1: a stale target can select another seat's correct encoder and still paste into the wrong PTY.
The paste arm leaves keyboard focus unchanged (`main.rs:90085`, `main.rs:97917`, `main.rs:97940`).

## 4. Folder rows and unchanged edges

Folder-to-terminal-centre remains `Refused` (`main.rs:29328`, `main.rs:29340`), returning without
a write (`main.rs:90071`). No caption is acceptable under the existing refusal convention:
it is a visible dashed outline, not silent lack of feedback (`seats.rs:21154`, `main.rs:46165`).
Edges/rim still yield `Split` without inspecting target kind (`main.rs:29327`); the resulting
files leaf is filled with the folder root (`main.rs:90145`, `main.rs:69099`).
Compared with the parent, `survey_drop`, `aim_at_layout`, `row_arrival_seat`, `plan_drop`,
`adopt_drop`, `fill_row_leaf`, and the split tail of `commit_layout_drop` are byte-identical.
That supports unchanged folder-edge behavior; the complete commit function is not byte-identical.

## 5. Mac and coordinate coverage

The row survey accepts physical coordinates (`main.rs:86968`, `main.rs:88900`); normalized
edge fractions and the scaled rim are platform-neutral (`seats.rs:7527`, `seats.rs:7537`).
The 1000/2000 milli-DPI test (`seats.rs:49762`) supplies points derived from solved rectangles;
it does not exercise AppKit coordinates, event ordering, mixed-monitor transitions, or actual frames.
The locked dependency is winit 0.30.13 (`Cargo.lock:5656`). Inspecting its local source shows
`src/platform_impl/macos/view.rs:1084` converts points to physical pixels, and `:591` emits motion
before mouse-up. Windows `src/platform_impl/windows/event_loop.rs:1797` emits mouse-up without
that refresh. Mac therefore has extra release-time protection, but can still commit a refreshed
offer before it was visibly presented. No native Mac run was performed; no scaling defect was found.

## 6. DESIGN consistency

Searched `view verbs`, `0717`, `internal drag`, and Chinese equivalents across DESIGN and plans.
**P2 — update contradictory current guidance.** `docs/DESIGN.md:2885` still promises terminal-centre
refusal and says the table is unchanged. `docs/plans/ui-style/invisible-gestures-2026-08-26.md:104`
also says any terminal-centre landing refuses; mark that historical statement superseded.
The new `docs/DESIGN.md:1028` itself says preview/files panes have one whole-pane meaning and
“no middle to find,” contradicting unchanged edge/centre surveying (`main.rs:88900`, `main.rs:29327`).
Retained code comments also cite the old rule as current (`main.rs:96074`, `main.rs:113360`).

## Validation

Read the stat, complete diff, callers, release routing, allocation, encoder, and dependency backends.
Windows: `cargo test -p bt-app --bin folio <filter> -j 4`; 15 passed, 0 failed across four filters.
Filters: `clipboard_path_tests` (11), `row_splits` (2), `a_rows_centre_says` (1), and
`a_pane_offers_one_middle_and_four_bands_at_every_scale` (1). No stale-release event test was executed.
No application was launched or process terminated; no scratch tests were added.
