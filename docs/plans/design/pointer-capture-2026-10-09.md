# Pointer ownership as one model: one router over the whole stack, one capture per latched gesture — design note, 2026-10-09

T-POINTER-CAPTURE, step 1 (design only; no product code in this step). Base: main `5c4a08b0`. **Revision (b), 2026-10-09**, answers Codex's review of revision (a) at `d871b43e` (verdict REWORK: nine HOLD items, seven NITs; `reports/F3-design.review-codex.md`). **Revision (c), 2026-10-09**, answers its second review of (b) at `7feae4b2` (REWORK, six HOLD items of which one — the owner's Q1 — is queued with the owner; `reports/F3-design.review-codex-r2.md`). What changed is listed at the end; the sections below are the current text. Follows T-STRIP-HOVER-THROUGH (four commits and three review rounds of 2026-10-04, DESIGN entries of that day from *A pane's notice strip owns the points it is drawn on* to *Third follow-up: a latched gesture records its shell and its button*). Every claim about today's behaviour carries a `file:line` on that base (paths under `crates/bt-app/src/` unless another crate is named). Every proposed rule names the cells of §3 behind it. Companion file: `pointer-capture-2026-10-09.readers.tsv` (the census of §1.0).

Rulings that bind this note: the capsule and the strip stay where they are (owner, 2026-10-04: "do not move things now"); the capsule is above the strip for paint, hover and press (same day); the `⌄` grammar — a peek on a 250 ms rest, a pin on a click, Esc / a click elsewhere / a second click to close (2026-08-16, amended 2026-09-23); a minimum is law to the program and advice to the hand (2026-08-10), so a divider drag keeps ignoring minima. The 2026-09-21 not-a-nanny ruling, as recorded (`DESIGN.md:11603`), is about modified-click link hand-off; this note relies on nothing in it — it simply adds no refusal, prompt or confirmation.

---

## 0. The decisions, in one screen

1. **The census** (§1.0, mechanical and reproducible): **425 functions in `bt-app` read the pointer** — 323 directly (52 read the pointer fields, 159 take a pointer position, 49 name a button or wheel event, 135 are hit tests; classes overlap) and 102 more that take coordinates and are reached, by name, from one of those (`cmdrail::nearest`, `StripRun::aim`, `preview_live::press`, `settings_geometry::pointer_moved`, the video seat's `grab` and `drag_to` among them) — plus 63 that call a reader and are none (an over-approximation). §1.1–§1.3 explain the ones that decide routing. The ticket's eight known gaps all map to readers (§1.4 K1–K8); reading the bodies found **twenty-six more** (G-1–G-26). The largest class: **every latched gesture except a `MouseRoute` is ended in `chrome_mouse_input`'s release ladder, which the arms above it in `mouse_input` can pre-empt**, so a release that lands on a menu, a card, the palette, the settings sheet or (for some latches) a hosted page leaves the gesture attached to the bare pointer (G-1).
2. **The router.** One function, `pointer_layer_at`, walks one list — the overlay's paint list read top first, plus the pane's own planes under it — over a per-frame `PointerFacts`, and answers every event kind: move, press, a release no capture owns, wheel, tip, cursor, the `⌄` rest clocks, the hosted page's hover. Paint order is pointer order by construction; the walk neither shapes text nor allocates, and both are tested (R-1–R-4).
3. **The capture.** **One application slot**, `App::pointer_capture: Option<PointerCapture>`, replaces the twenty latch fields behind the 22 gestures (`mouse_route` alone carries three). It records the window, the owner (with its payload, its tab included), the button, the router's answer at the press, the OS capture at the press, and, for the two owners that must tell somebody, a release snapshot (R-5–R-9, R-14).
4. **The release goes to the owner** before any layer is asked, wherever the pointer is (R-6) — the general form of the 2026-10-04 rule "the surfaces decide where a gesture starts, not where it ends", which today holds only for `MouseRoute`.
5. **A second button during a gesture goes nowhere, in whichever window it arrives** (R-8; **pending the owner's Q1**; it supersedes one 2026-10-04 sentence); a press of the *same* button proves the held capture stale and cancels it first (R-5). **Every capture answers its endings** — its release, a cancel (blur, OS capture lost, window hidden, a scale change), an OS drag hand-off (its own ending, not a cancel), its owner gone, and, for tab-bound owners, its owner off the glass — by an exhaustive match, called from a lifecycle-door matrix (R-9). A drag is outside "off the glass": its spring switching tabs is its own aim.
6. **The hosted page on Windows is a layer like any other**: it is composition-hosted and hears only what Folio forwards. The mechanism for "a divider release over a page reaches its owner" is the capture slot: no overlay window, no second `SetCapture`. A press inside a page is a view-level capture owned by that page; a page seat never leaves the glass with a button down, and a scale change releases it in the old space first (R-13, §2.4). **macOS pages are out of this ticket**, with a consequence table — and today's macOS behaviour for a page press that reaches Folio's surface view (consumed, focus and raise, nothing forwarded) is kept through every generic cut and pinned (§2.5).
7. **The behaviour table** (§3) has **274 decided cells: 153 change, 121 stay**, plus 4 not applicable — counted from the tables by the rule stated in §3. Wheel-while-held is a column of its own; the stale-capture press and the second button in another window are cells of their own.
8. **Migration** is **ten cuts in fourteen commits** (cut 8 is five). Cut 1 is dead-path scaffolding plus the guard: a `bt-source` index test over **every** `bt-app` item, whose needles are the six places pointer data can enter Folio (the pointer fields, the position type, the button and wheel types, the system cursor reads, the pointer `WindowEvent` variants, the touch pan step), with no allowlist to grow and a debt file generated by the same query, so it passes on the day it lands (§4.2). Cut 1's cost tests use a float stack parameterised up to 512 windows with an exact visit formula, and pin one router walk per complete event. Cut 2 only mirrors. Cut 5 carries a lifecycle-door matrix of exact items enforced by index pins and behavioural tests. Cut 7 tests a real `WebSeat`'s presence order.
9. **No new window-thread wait**: the same `GetCapture` sample the divider already takes, the same `SendMouseInput` door the page already uses. `window_waits.tsv` (231 physical lines at base) does not change (§4.4).
10. **Three questions for the owner** (§6), each with the reviewer's agreement: refuse a second button (**pending the owner; recorded before cut 4 is built**); tell a program whose tab was switched away that its button came up (with its last modifiers); let a page keep a drag that leaves its rectangle (Windows).
11. **Boundaries** (§7): touch pans under a capture are discarded; pointer-less activation never builds a capture; the summoned window is one more lifecycle door; an 8 kHz mouse has an absolute and tail budget; a future OLE drag-out hands the capture to the system synchronously (`OsDragHandoff`, an ending of its own: no home settle, no release, no cancel verb).

---

## 1. Inventory

### 1.0 The census — what "every pointer reader" means, and how it is counted

The first revision counted 164 readers by hand, and the review found eight it missed (`file_peek_card_layers`, `file_peek_foot_grasp`, `file_peek_head_grasp`, `pane_menu_row_under_pointer`, the close-time re-ask in `toggle_settings_panel`, the three re-asks in `settings_mouse_input`, `aim_focus_card_window`, `scroll_settings`). The count is now mechanical. **A pointer reader is a product function of `bt-app`** (every `crates/bt-app/src` file except `*_tests.rs`, `tests.rs`, `test_support.rs`, and every `#[cfg(test)]` item) **in one of five classes**:

- **P** — it reads `pointer_position` or `pointer_last_seen`;
- **E** — it takes or holds a `PhysicalPosition<f64>` (a window pointer position; window positions are `PhysicalPosition<i32>` and are not counted);
- **B** — it names `MouseButton`, `ElementState` or `MouseScrollDelta`;
- **H** — a hit test: its name says so (`hit`, `_at`, `contains`, `covers`, `under`, `_holds`, `_part`, `claim`, `_point`) and its signature takes an `x`/`y` pair, a `[f32; 2]` or a `PhysicalPosition`;

- **C** — a **semantic** reader the four lexical classes miss: a function whose signature takes coordinates (an `x`/`y` scalar pair, a `[f32; 2]`, an `(f32, f32)`/`(f64, f64)` tuple, or a parameter named `point`, `at`, `pointer` or `position`) and that is **reachable through calls from a P, B or H function**, the calls resolved by name to a fixed point (revision (c); the second review named six it found by reading: `cmdrail::nearest`, `cmdrail.rs:1308`; `StripRun::aim`, `seats.rs:8289`; `preview_live::press`, `preview_live.rs:654`; `settings_geometry::pointer_moved`, `settings_geometry.rs:185`; `VideoSeat::grab` and `drag_to`, `video_seat.rs:1061`, `1070` — all six are in C);

plus **R** — a function that calls a P/E/B/H function by a name of ten characters or more and is in no other class (one level; an over-approximation, because a long name can be shared by an unrelated function). The programs and the one command that merges and sorts their outputs are Appendix A; the output, one row per function, is `pointer-capture-2026-10-09.readers.tsv`.

| class | functions |
|---|---|
| P | 52 |
| E | 159 |
| B | 49 |
| H | 135 |
| distinct P ∪ E ∪ B ∪ H | 323 |
| C (and in none of the four) | 102 |
| **readers, P ∪ E ∪ B ∪ H ∪ C** | **425** |
| R only (calls a reader, is not one) | 63 |

The eight the first review named are in it (P: `peek.rs:1539`, `2108`, `2897`; `panes.rs:2656`; `main.rs:46102`; PEB: `main.rs:47451`; PB: `main.rs:52570`, `52821`), and so are the six of the second. Class C resolves calls by name, so a shared name (`press`, `aim`) pulls in every function of that name that takes coordinates: it over-counts, which is the safe direction for a census. Limits, written down: the programs read text, not the index, so a raw string with braces inside a test item can end a skip early (over-counting again).

**What the census is for, and what it is not.** It is the map of where routing decisions are taken today (cut 8 and cut 9 work from it). **It is not the guard's needle set.** The guard (§4.2) does not try to recognise every function that tests a point — no lexical or name rule can, as the second review showed. It guards the four places pointer data **enters** Folio, over every `bt-app` item, so a function outside the router can only ever see coordinates the router handed it.

§1.1–§1.3 below are the readers that decide **routing**, explained one by one.

The columns: **reader** → **file:line** → **what it decides** → **what it latches** → **how that ends** → **what it ignores**. Paint order, bottom to top, is `OverlayStack::flattened` (`main.rs:31902-31975`): preview bars, video bars, terminal bars, command rail, formula tools, rail, flight, ground, in-pane (strip under capsule), web sheet, layout peek, float, modal, file menu, pane menu, git menu, term menu, tab menu, palette, toast, key hint, card hint, tooltip, file peek, drag ghost, window ring. The hosted page is under all of it (the DirectComposition visual below the wgpu overlay, DESIGN §7.8 ②).

### 1.1 The four doors and the router they share (13)

| reader | file:line | decides | latches | ends | ignores |
|---|---|---|---|---|---|
| `pointer_moved` (`CursorMoved`) | `runtime/mouse.rs:1570-2299` | in this order: the page's hover (1581), the `⌄` rest clocks (1587), the rail zone (1612), nine latched drives that return (1618-1690), pane-furniture hovers (1702-1713), seven modal/card hovers that return (1719-1815), toasts (1823), ten menu/palette hovers that return (1830-1991), the peek-head and glance-head promotions (1997-2005), the float carry (2010), the divider (2016), the three six-pixel latches (2030-2068), the drag (2072), chrome hover (2075), flyout and glance intents (2091-2121), layout peek (2129), capsule/strip/rail hover (2163-2170), the tip (2175-2188), the hovered pane's formula, link and reference hovers (2193-2237), a routed selection or forwarded drag (2246-2258), the motion report (2272-2298) | `pointer_position`, `pointer_last_seen` (1571-1572) | — | its order is written by hand and is not the paint order (G-9) |
| `pointer_left` (`CursorLeft`) | `runtime/mouse.rs:1441-1535` | clears every hover; every page hears an off-page move (1445-1456) | — | — | deliberately not a cancel: winit holds the Win32 capture from button-down to button-up (1484-1492) |
| `mouse_input` (`MouseInput`) | `runtime/mouse.rs:4383-5525` | the macOS secondary click (4419-4431); every press hides the tip, the key hint and the layout peek and blurs the search field (4437-4472); the owned release of a `MouseRoute` (4481); then: seven full-window cards (4487-4570), toasts (4587), settings (4596), restore card (4606), git, term, file, pane, tab menus (4622-4821), palette (4830), graph filter (4860), glance card (4906-4916), four right-press openers (4937-4998), the peek-head clear (5007), the float carry's release (5012), a float's page (5036), `press_float` (5049), profile, root, preview menus (5060-5237), in-pane surfaces (5258), the page (5270), `chrome_mouse_input` (5281-5290), download sheet (5297), command rail (5318), formula block (5325), the rendered page's menu (5397), the terminal menu (5424), the forwarded press (5481), the local selection (5514) | `mouse_route` (5335, 5481) | a `MouseRoute` at 4481; everything else inside `chrome_mouse_input` | not the paint order (G-9); every arm above 5287 that returns on a release can take a release a latched gesture owes (G-1) |
| `chrome_mouse_input` | `runtime/mouse.rs:3193-4203` | middle press on a tab closes it (3205-3221); non-left buttons are not its (3222); **the release ladder** (3225-3369): video bar, body thumb, terminal thumb, column thumb, block thumb, picture pan, edit selection, rendered selection, drag, divider, pane/row press, tab press; **the press**: video (3391), glance hidden (3398), `focus_pane_at` (3408), rename blur (3424-3563), editor blur (3571), title bar (3618), terminal thumbs (3649, 3661), the preview body ladder (3672), then the chrome target (3684-4200) | divider (3728), pane press (3783, 4062, 4089), row press via `arm_row_press` (3838), tab press via `press_tab` (4095) | its own ladder | reached only for releases no arm above it took |
| `queue_wheel` / `flush_wheel` / `mouse_wheel` (`MouseWheel`) | `runtime/mouse.rs:5578`, `5676`, `5771-6378` | stations: glance card body (5826), card hidden (5838), first run (5849), settings (5855), any modal (5864), toast (5873), the topmost band of `OVER_IN_PANE_TOP_FIRST` (5891), an in-pane surface (5898), palette (5926), page (5944), **files** float (5967), rail or tab strip (6001-6029), files column (6035), graph (6063), picture (6080), preview body (6102), then the terminal under the pointer (6128-6377) | `wheel_burst`, the remainders | per burst | a preview or page float is not a station of its own (G-6) |
| `pointer_target_at` | `runtime/mouse.rs:2339-2375` | floats (2343-2347), then `IN_PANE_SURFACES_TOP_FIRST` yielding to `OVER_IN_PANE_TOP_FIRST` (2351-2372), then the docked ladder (2373) | — | — | everything painted above the floats: glance card, toasts, palette, menus, modal band (K5) |
| `chrome_target_at` | `runtime/frame.rs:811-822` | the router's `Chrome` arm | — | — | as above |
| `in_pane_surface_at` | `runtime/mouse.rs:2386-2398` | the router's in-pane arm, or a float's own pill (`notice_at`) | — | — | as above |
| `painted_over_in_pane_at`, `topmost_band_over_in_pane_at`, `over_in_pane_claims` | `runtime/mouse.rs:2409-2504` | whether a band above the in-pane surfaces covers a point; `Float` answers `false` by design (2491-2497) | — | — | — |
| `update_chrome_hover` | `runtime/mouse.rs:2550-2606` | chrome hover and `.pane:hover`; the download sheet first (2565) | `seat_pointer.hover`, `.pane_hover` | next move, `pointer_left` | the bands above the floats (reached only after their arms returned) |
| `apply_pointer_cursor` | `runtime/mouse.rs:2620-2695` | the shape; inside a page the page's own (2661-2674) | — | — | the page test subtracts no float, menu or toast (G-3) |
| `owned_tooltip_anchor_at` | `runtime/mouse.rs:2535-2548` | filters the flat tip list by the in-pane owner | — | — | floats and the glance card (K6) |
| `a_gesture_holds_the_pointer` | `runtime/mouse.rs:6553-6575` | whether a latch is held, so `web_page_at` answers nothing (`web.rs:1734`) | — | — | `preview_selecting`, `tab_press`, `pane_press`, `row_press`, `file_peek_press`, `MouseRoute::Forward` (G-1) |

### 1.2 The latched gestures (22)

"Button" says whether the latch records the button that started it. "Blur" is the `WindowEvent::Focused(false)` arm (`main.rs:67427-67505`): it runs `cancel_drag`, `cancel_divider_drag`, `cancel_preview_text_drag`, clears `tab_press`, `pane_press`, `row_press` and calls `hide_file_peek`, and nothing else. Esc (`runtime/keyboard.rs:1117-1129`) hides the glance card and cancels a drag or a divider, and nothing else. `activate_tab` (`runtime/tabs.rs:194-196`) drops `mouse_route` and `divider_drag` and nothing else; the latches marked *per tab* live on `TabState` and are reached through the `Runtime` deref to the active tab.

| # | gesture | state | set | driven | ended today | button | blur | owner gone |
|---|---|---|---|---|---|---|---|---|
| C1 | divider | `divider_drag` (window), `DividerDrag::capture` | `runtime/mouse.rs:3728` | `runtime/panes.rs:3811` (from `mouse.rs:2016`) | left release `mouse.rs:3305`; Esc; blur; OS capture lost (`panes.rs:3922`, sampled each turn from `dpi.rs:183`); split gone (`panes.rs:3824`); tab switch drops it without restoring (`tabs.rs:196`) | no | cancel | dropped |
| C2 | tab press | `tab_press` (window) | `runtime/tabs.rs:1800` (from `mouse.rs:4095`) | travel `mouse.rs:2030-2040` | left release `mouse.rs:3358`; drag start/cancel; blur; tab closed (`tabs.rs:289-295`) | no | cleared | cleared |
| C3 | pane press | `pane_press` (window) | `mouse.rs:3783`, `4062`, `4089` | travel `mouse.rs:2041-2052` → `begin_pane_drag` (`panes.rs:4283`), which builds its leaf from the tab active *at travel* (`panes.rs:4308-4311`) | left release `mouse.rs:3321`; blur | no | cleared | not cleared (G-17) |
| C4 | row press | `row_press` (window) | `mouse.rs:447` (from `986`, `3838`) | travel `mouse.rs:2053-2067` | left release `mouse.rs:3322`; every left press (`3403`); blur | no | cleared | payload resolved at travel |
| C5 | tab, pane or row drag | `drag` (window) + `app.drag_broker` | `mouse.rs:2769` (`begin_drag`, which opens the broker) | `mouse.rs:2913` | left release `mouse.rs:3297` → `3040`; Esc; blur; broker guard (`main.rs:65107`); source gone (`mouse.rs:2921`) | no | cancel | dropped |
| C6 | peek header press | `float_head_press` (window) | `mouse.rs:1016` | promotion `mouse.rs:1997` → `1217` | any event that reaches `mouse.rs:5007` | no | **not cleared** | promotion refused (`1231`) |
| C7 | float move / resize | `float_drag` (window) | `mouse.rs:1000`, `1021`, `1239` | `mouse.rs:2010` → `1249` | any release reaching `mouse.rs:5012`; float gone (`1257`) | no | **not cleared** | dropped |
| C8 | glance head press | `file_peek_press` (window) | `runtime/peek.rs:1989` | `mouse.rs:2003` → `peek.rs:2237` | **any** button's release `mouse.rs:4911`; `hide_file_peek` | no | cleared | cleared |
| C9 | glance thumb | `FilePeek::thumb_grab` | `runtime/peek.rs:2002` | `mouse.rs:1647` → `peek.rs:2193` | **any** button's release `mouse.rs:4907`; `hide_file_peek` | no | cleared | cleared |
| C10 | video scrub / volume | `video_bar_drag` (window, `main.rs:15295`) | `runtime/preview.rs:5018` | `mouse.rs:1618` → `preview.rs:5104` | left release `mouse.rs:3230` only | no | **no** | silent stale latch |
| C11 | preview body thumb | `preview_body_drag` (per tab, `main.rs:12203`) | `preview.rs:5321` | `mouse.rs:1673` | left release `mouse.rs:3243`; surface swept (`preview.rs:2337-2342`); buffer left (`3140-3145`) | no | **no** | healed |
| C12 | block thumb | `preview_block_drag` (per tab, `main.rs:12197`) | `preview.rs:6486` | `mouse.rs:1688` | left release `mouse.rs:3267`; swept (`preview.rs:2328`); buffer left (`3125-3130`); document rebuilt (`8686`) | no | **no** (peek only) | healed |
| C13 | picture pan | `preview_image_drag` (per tab, `main.rs:12279`) | `preview.rs:7924` | `mouse.rs:1667` | left release `mouse.rs:3276` **only** | no | **no** | **never healed** (G-16) |
| C14 | edit-surface selection | `preview_selecting` (per tab, `main.rs:12266`) | `preview.rs:7889` | `mouse.rs:1655` | left release `mouse.rs:3284`; swept (`preview.rs:2307-2312`); buffer left (`3112`) | no | **no** | healed |
| C15 | rendered-text selection | `preview_text_drag` (per tab, `main.rs:12272`) | `preview.rs:5885` | `mouse.rs:1660` | left release `mouse.rs:3291` → `preview.rs:6261`; `cancel_preview_text_drag` (`preview.rs:5972`) | no | cancel | healed |
| C16 | terminal thumb | `terminal_thumb_drag` (per tab, `main.rs:12214`) | `main.rs:49077` | `mouse.rs:1680` | left release `mouse.rs:3252` **only** | no | **no** | swallows every move until a release (G-14) |
| C17 | terminal foot mark | `terminal_column_drag` (per tab, `main.rs:12235`) | `main.rs:49004` | `mouse.rs:1685` | left release `mouse.rs:3260` **only** | no | **no** | as C16 |
| C18 | terminal selection | `MouseRoute::Local` (window, `main.rs:14209`) with `owner: PasteTarget` | `runtime/terminal.rs:1409` (from `mouse.rs:5521`) | `mouse.rs:2246` → `terminal.rs:1447` | left release `mouse.rs:4268-4279`, ahead of every arm; tab switch; overwritten by a formula press (`mouse.rs:5335`) | left only by construction | **no** | dropped |
| C19 | forwarded press | `MouseRoute::Forward {button, sgr, owner}` | `main.rs:21614-21667` (from `mouse.rs:5481`) | `mouse.rs:2256` → `4349` | the latched button's release `mouse.rs:4280-4329`; tab switch (`tabs.rs:195`); owner not live (`runtime/clipboard.rs:523-528`: tab not on top) → dropped, **nothing sent** | yes | **no** | dropped, nothing sent |
| C20 | formula block | `MouseRoute::MathBlock` (no fields) + `math_tool_pressed` | `mouse.rs:5335`, **without asking whether a route is held** | none (`pointer_moved` returns at `2238`) | **any** button's release `mouse.rs:4258-4264`; tab switch | no | **no** | no owner recorded |
| C21 | a press inside a hosted page | none in Folio; the page's own `WebSeat::buttons` mask (`webhost.rs:4078-4085`) | `runtime/web.rs:1852` | `web.rs:1808`, **only while inside the page's bounds** | the release is forwarded only if it lands on a page (`mouse.rs:5270`) | — | **no** | — |
| C22 | settings slider / menu bar | `settings_slider_drag`, `settings_menu_bar_drag` (window, `main.rs:13628`, `13670`) | `main.rs:47557`, `47488` | `settings_geometry.rs:185` (from `mouse.rs:1775`) | **any** button's release while the sheet is up (`main.rs:47462-47464`) | no | **no** | survives the sheet closing (G-15) |

### 1.3 Hit tests, hover instruments, forwarders and re-ask sites (129 routing readers, a curated subset of §1.0)

Counted one per function named below that reads a pointer position or a pointer event, and one per re-ask site. Functions named only for context — the tip-anchor builders, the popup closers, `send_to_web_page`, `WebSeat::send_mouse` — are not counted.

**Floats.** `FloatHost::drawn` (`float.rs:1508`, paint order, dismissed windows included) and `FloatHost::hit_order` (`float.rs:1542`, live only, front first) are two lists. `float_hit_at` (`runtime/floats.rs:2487-2630`) walks `hit_order`, is total inside a frame (`Body`/`Head` fallback), and measures two captions through the font on every call (2495-2507) and collects ids into a `Vec` (2513); it ignores every band above the floats. `pointer_over_a_float` (`mouse.rs:1344`) walks `drawn()` with the fade frame instead and feeds the cell hover root (G-20). `float_trigger_at` (`floats.rs:1919`) raises the files card, through the router. `drive_float_hover` (`mouse.rs:853`) holds or releases the peek's grace (`float.rs:1933`, `1942`); `FloatHost::observe` (`float.rs:1713`) latches the flyout intent; neither is cleared on blur. `press_float` (`mouse.rs:898`, left only at 5049) claims the whole frame, the pill included (G-11). `drive_float_drag` (`mouse.rs:1249`), `promote_float_head_press` (`mouse.rs:1217`).

**Menus and the palette.** `drive_term_menu_hover` (`mouse.rs:547`), `drive_pane_menu_hover` (`runtime/panes.rs:2501`), `drive_tab_menu_hover` (`runtime/tabs.rs:1047`), `drive_palette_hover` (`runtime/palette.rs:234`; hover selects the row). Pure geometry: `profiles::hit` (`profiles.rs:6409`), `root_menu_hit` (7810), `file_menu_hit` (8885), `git_menu_hit` (9745), `term_menu_hit` (10736), `pane_menu_hit` (12587), `tab_menu_hit` (13549), `git_filter_menu_hit` (13901), `preview_menu_hit` (14364), `palette::hit` (`palette.rs:1191`), `palette::wheel_part` (`palette.rs:1241`). `press_on_its_own_trigger` (`mouse.rs:818`) and `popover_trigger_at` (`floats.rs:2132`, through the router) decide the own-trigger rule. `close_popups_except` (`floats.rs:1157`) keeps the `Popup` menus mutually exclusive; `close_every_popup` (`floats.rs:1251`) runs only on a tab switch (`tabs.rs:212`), not when a modal card comes up.

**The `⌄` clocks.** `observe_chevrons` (`floats.rs:1974`) runs at the top of `pointer_moved` (1587) and stops only for a drag or a float carry (1985-1988) and under another *hover* panel (2001-2004); a menu or the palette being up does not stop it, and it reads the router (2013). `advance_chevrons` (`floats.rs:2274`) re-asks `chrome_target_at` (2297) and `popover_trigger_at` (2317) when the rest matures. `rearm_hover_intents` (`mouse.rs:349-369`) re-arms the chevrons, the flyout, the glance and the layout peek from the remembered pointer with no modal, toast or menu check.

**Tips.** `rebuild_tooltip_anchors` (`runtime/tooltips.rs:35-526`) builds one flat list, first match wins, and is skipped during a drag; no float, palette, toast or glance frame is consulted. `tooltip_anchor_at` (`tooltips.rs:580`) → `TooltipAnchors::at` (`tooltip.rs:624`). `note_tooltip` (`tooltips.rs:569`). `preview_rail_tip_anchors` (`preview.rs:3645-3666`) pushes docked rails before float rails. `command_tick_anchor` (`main.rs:45137`) and `preview_hex_anchor` answer ahead of the list (`mouse.rs:2175-2187`).

**In-pane surfaces.** `docked_notice_at` (`runtime/attention.rs:592`, `notice::claim`), `notice_at` (`attention.rs:567`, through the router), `drive_notice_hover` (`attention.rs:505`), `press_notice` (`attention.rs:525`), `drive_search_hover` (`runtime/search.rs:33`), `search_at` (683), `search_part_at` (695), `press_search` (704). The capsule has no drag or caret-by-click latch. It is laid out from the pane's head bottom plus 8 logical px (`search::lay_out`, `search.rs:1443`, via `seats::search_capsule_host`, `seats.rs:18151`); a strip takes the 30 px under the head (`seats::pane_notice_strip`, `seats.rs:2673`; `notice::lay_out`, `notice.rs:336`), so the capsule always covers the strip's right end — the controls the strip lays out from the right (K7).

**Toasts and the glance card.** `toast::at` (`toast.rs:770`), `drive_toast_hover` (`attention.rs:247`), `press_toast` (`attention.rs:278`), `toast_pointer` (`attention.rs:182`, a paint-time read). `file_peek_holds` (`peek.rs:724`), `observe_file_peek` (`peek.rs:400`), `glancing_row_at` (`main.rs:46089`), `row_under` (`main.rs:50654`), `press_file_peek` (`peek.rs:1954`), `drag_file_peek_thumb` (`peek.rs:2193`), `promote_file_peek_press` (`peek.rs:2237`), `release_file_peek_press` (`peek.rs:2374`), `release_file_peek_thumb` (`peek.rs:2397`).

**Command rail and layout peek.** `drive_command_rail_hover` (`panes.rs:613`) and `command_rail_at` (`panes.rs:732`) read bare `seats::pane_at` (`panes.rs:695`, `733`), not the router (G-12); `press_command_rail` (`panes.rs:787`) is safe by its place in the press road. `layout_peek_target_at` (`peek.rs:80`), `note_layout_peek` (`peek.rs:94`).

**The hosted page.** `web_page_at` (`runtime/web.rs:1709-1764`) subtracts held gestures (1734), the tab list (1737), the download sheets (1740-1746) and an open capsule (1748-1753), and no float, menu, palette, toast or glance card (G-3). `point_is_on_the_web_page` (1767), `send_to_web_page` (1775), `drive_web_pointer` (1808-1846), `press_web_page` (1852-1929), `scroll_web_page` (1936), `press_web_sheet` (1517). `WebSeat::send_mouse` (`webhost.rs:4067-4087`) forwards points outside the bounds on purpose (`SendMouseInput(LEAVE)` is refused by the engine, `webhost.rs:4061-4066`).

**The docked ladder.** `docked_chrome_target_at` (`panes.rs:3986`, one production caller: the router) asks `tab_list_target_at` (`tabs.rs:1727`), window chrome, `hit_files_root` (`seats.rs:17178`), `hit_text_size` (`seats.rs:17272`), `hit_preview_head` (`seats.rs:18605`), `hit_preview_rail` (`seats.rs:17135`), `hit_pane_ghost` (`seats.rs:6452`), `hit_chrome_in_motion` (`seats.rs:6343`, called at `panes.rs:4091` with `pane_transforms`), `hit_files_foot`, `hit_preview_card_button`, `hit_files_seg`, `hit_git_panel`, `hit_preview_play`, `hit_git_graph`, `hit_files_tree`. Only `hit_chrome_in_motion` reads the pane's motion frame (G-13). Beside the ladder: `seats::title_bar_drag_point` (`seats.rs:6657`), `seats::tab_strip_contains` (`seats.rs:6171`), `seats::files_body_at` (`seats.rs:17624`), `rail_contains` (`panes.rs:5157`), `panel_covers` (`panes.rs:5147`).

**Pane-body hit tests.** `seats::pane_at` (`seats.rs:7665`, solved rectangles, nothing else), `pane_hit_context` (`runtime/math.rs:1364`, floats via `pointer_over_a_float`, the capsule and the strip; not menus, palette, toast, glance card, an open rail or motion), `pane_frame_hit` (`panes.rs:3514`), `frame_hit` (`frame.rs:565`), `math_hit` (`math.rs:1430`), `forwarded_mouse_hit` / `_in` (`mouse.rs:1375`, `1391`), `drag_hit_in_pane` (`panes.rs:3645`, clamps into the origin's body — right for a capture), `focus_pane_at` (`main.rs:48603`, bare `pane_at`, G-18), `preview_surface_at` (`preview.rs:2448`, float **bodies** then docked bodies, inclusive edges, G-6, G-25), `preview_rendered_surface_at` (`preview.rs:5741`), `preview_edit_body` (`preview.rs:8004`), `file_row_under` (`files.rs:1727`, through the router), `terminal_column_bar_under` (`terminal.rs:382`), the drag survey `survey_drop` (`mouse.rs:2827`) and `pointer_is_on_our_own_glass` (`mouse.rs:2909`).

**Re-ask sites** — "the answer changed without the pointer moving", each reading `pointer_position` and asking a hit test or the chrome hover again: `mouse.rs:755` (autoscroll), `panes.rs:1498` (pane closed), `2633`/`2657` (pane menu), `3967` (divider cancelled), `5059` (rail state), `5131` (rail scroll), `tabs.rs:1150` (tab menu), `3397` (strip scroll), `main.rs:52287` (focus mode), `keyboard.rs:1309`, `1328` (settings walked by keys), `git.rs:4546`, `4632`, `palette.rs:291`, `preview.rs:212`, `4569`, `4920`, `13632`, `profiles.rs:545`, `attention.rs:48`, `math.rs:2022`. Each reads `chrome_target_at` or a layer's own test, so none of them knows about a menu or the palette standing over the point (G-8).

**Outside the window's pointer.** `platform_pointer_now` (`mouse.rs:5763`, a drop's point, asked of the system on purpose) and `quake.rs:238` (which monitor the summoned window opens on) read the system cursor, not the window's pointer, and are not router questions; they stay as they are.

**Paint-time and modal readers (the first revision missed them).** Seven functions read the pointer while a frame is being **built** and decide what lights with their own hit test, not the router: `file_peek_card_layers` (`runtime/peek.rs:1539`, the glance card's lit parts), `file_peek_foot_grasp` (`peek.rs:2108`) and `file_peek_head_grasp` (`peek.rs:2897`, the hand shapes over the card), `toast_pointer` (`runtime/attention.rs:182`), `video_play_mark_layer` (`preview.rs:4912`), `image_grasp` (`preview.rs:13628`), `preview_neighbour_buttons` (`preview.rs:202`) (G-25). Four read it inside the modal band: `toggle_settings_panel` (`main.rs:46102`, re-asks the chrome hover on close), `settings_mouse_input` (`main.rs:47451`, three re-asks of `settings::hit`), `scroll_settings` (`main.rs:52821`, the sheet's wheel) and `aim_focus_card_window` (`main.rs:52570`, `Alt`+wheel aiming a card's window) (G-26). `pane_menu_row_under_pointer` (`panes.rs:2656`) is the re-ask site listed above at `2657`.

### 1.4 Gaps

**The ticket's eight, mapped to readers.**

- **K1. A hosted page's press handler takes releases.** The page arm (`mouse.rs:5270`) stands above `chrome_mouse_input` (5287). Since 0.2.2 `web_page_at` answers nothing while `a_gesture_holds_the_pointer` (`mouse.rs:6553`, read at `web.rs:1734`) — so a drag, divider, scrub, thumb, picture pan or rendered selection released over a page **does** reach its owner today. Still open: an edit-surface selection (`preview_selecting` is not in the predicate) and the three six-pixel presses (`tab_press`, `pane_press`, `row_press`). This is one case of G-1.
- **K2. `MouseRoute::MathBlock` records no button or owner.** Confirmed: no fields (`main.rs:21402`); any release ends it (`mouse.rs:4258`); the press (`mouse.rs:5325-5335`) does not ask whether a route is held, so a formula pressed with the right button during a forwarded left drag **overwrites** `Forward` and the child never hears its left release (G-5).
- **K3. Owner gone mid-drag.** `activate_tab` drops `mouse_route` (`tabs.rs:195`) and `live_paste_target` refuses a tab not on top (`clipboard.rs:523-528`), so a forwarded press never gets its release (the arm says so, `mouse.rs:4292-4297`). The per-tab latches C11–C17 are stranded on the tab that was left (G-14).
- **K4. Wheel over a float's head or foot falls through.** The float station answers files floats only (`mouse.rs:5967-5992`); a preview or page float is found by `preview_surface_at`, which tests bodies only (`preview.rs:2466`), so a notch on its head, foot or rail reaches the docked preview, the files column or the terminal under it. Wider: the rail, strip and files-column stations (`mouse.rs:6001-6053`) stand above `preview_surface_at` (6063), so even a preview float's **body** standing over a files column or the tab list scrolls those (G-6).
- **K5. The router answers `Float` for points a palette or menu covers.** Confirmed: floats are asked first (`mouse.rs:2343`) and `OverInPane::Float` is `false` (2497). The top-level arms protect the press and hover in practice; the readers that call the router directly do not: the `⌄` rest clock (G-10), the re-ask sites, `rearm_hover_intents`, `layout_peek_target_at`.
- **K6. Tip anchors ignore floating windows.** Confirmed: nothing at registration (`tooltips.rs:35-526`) or lookup (`tooltips.rs:580`), and `owned_tooltip_anchor_at` lets a float point through as "not in a pane" (`mouse.rs:2545`). The glance card is not a layer for the tip either (G-7).
- **K7. The capsule partly covers a strip.** Placement is ruled out of scope; layering is ruled and implemented (`IN_PANE_SURFACES_TOP_FIRST`, `main.rs:34532`). Under the model the covered strip controls claim nothing (R-3), which is today's answer.
- **K8. Every chrome-latched route clears its own state on release wherever the pointer is — keep it.** True today only for a release that reaches `chrome_mouse_input`; R-6 makes it unconditional.

**New, from the bodies.**

- **G-1. A release that never reaches the ladder leaves the gesture on the bare pointer.** The arms above `mouse.rs:5287` that return on a release: the seven cards (4487-4570, every state), settings (4596), restore (4606), every menu's hit arm (git 4626-4633, term 4650-4666, file 4683-4690, pane 4735-4755, tab 4797-4813), a palette row (4835-4842), graph filter (4865-4872), the glance head (4911), the float carry (5012), profile/root/preview menus (5070-5103, 5129-5173, 5203-5223), the page (5270) for the latches K1 names. The nine latched drives in `pointer_moved` (1618-1690) run with no button test, so the stranded thumb, scrub or selection goes on following the hand; a stranded six-pixel press becomes a drag on the next move with no button held (`DragLatch::travelled`, `main.rs:21893`, measures distance only); a stranded divider is cancelled a turn later by the capture sample (its ratio jumps back); a stranded drag is cancelled by the broker's guard and goes home. Reachable by a menu a second button opened (G-5), by a card that arrives under the hand (the update card, a paste card), by the palette opened from the keyboard, by the `⌄` rest clock (G-10).
- **G-2. A gesture begun inside a hosted page has no owner.** Moves outside the bounds are not forwarded (`web.rs:1815-1818`, `1841-1845`), a release outside is never forwarded (`mouse.rs:5270`), and the page's button mask stays down (`webhost.rs:4078-4085`): a page selection stops at the edge, and the next hover over the page extends it with no button held.
- **G-3. `web_page_at` subtracts no float, menu, palette, toast or glance card.** A page under a float hears hover moves (`web.rs:1814-1818`), gets right and middle presses (`press_float` is left only, `mouse.rs:5049`; the page arm is 5270), scrolls under a notch on the float (the page station 5944 stands above the float station 5967), and lends its cursor to the float (`mouse.rs:2661-2674`). Under a menu, the palette, a toast or the glance card it hears hover moves and shows its cursor.
- **G-4. Blur and OS capture loss end 8 of the 22 latches.** On blur: C1–C5, C8, C9, C15; on capture loss without a blur: C1 and C5 only. Not: C6, C7, C10–C14, C16–C22. Alt+Tab while dragging a thumb or a selection, and the next move scrolls or selects with no button held; a forwarded press leaves the child pressed.
- **G-5. A second button goes down the whole press road while a gesture is held.** A right press raises the tab menu (`mouse.rs:4937-4945`), a file-row menu (4948-4955), the pane menu (4967-4981), a git row's menu (4992-4997), the page menu (5397-5406) or the terminal menu (5424-5434); a formula press replaces any route (5335). Only the cell road refuses it under a forwarded press (5511). The menu it raises then eats the first button's release (G-1). Glance latches (C8, C9) and settings drags (C22) end on any button's release.
- **G-6. Wheel: K4, plus** a preview float over a files column, the tab strip or the rail scrolls the thing under it (`mouse.rs:6001-6053` before `6063`), and a notch on a top float's head over a lower float's body scrolls the lower float (`preview.rs:2454-2468` is body-only).
- **G-7. Tips: K6, plus** a float's rail control over a docked rail control speaks the docked one's tip (docked anchors pushed first, `preview.rs:3654-3665`), and the glance card is not a layer for the tip, the chrome hover (`mouse.rs:2573`), the preview-body hover (1702) or the rail.
- **G-8. Readers that call the router directly get K5's answer**: the re-ask sites (§1.3), the `⌄` clocks (`floats.rs:2013`, `2297`, `2317`), `rearm_hover_intents` (`mouse.rs:349-369`), `layout_peek_target_at` (`peek.rs:84`), `git_menu_target_at` (`git.rs:2306`).
- **G-9. The hover order and the press order are not the paint order.** Hover (`pointer_moved`): toast, then profile, root, preview menus (painted lowest of the menus), file, git, term, graph filter, pane, tab, palette (painted highest). Press (`mouse_input`): the seven cards before toasts and menus that are painted above them; git before term; file before pane; palette after five menus; **toast before the glance card** (4589 vs 4914) though the card is painted above it, and the toast's hover arm returns (1826) before the glance card's hover (2118) is asked; **`press_float` (5049) before the profile, root and preview menus** (5060-5237) though they are painted above floats. Between the `Popup` menus the disorder is latent (only one is up), but against the cards, the toasts, the glance card and the floats it is reachable: the new-tab picker over a pinned float sends a press on a profile row to the float; a card that arrives under an open menu takes the menu's presses.
- **G-10. The `⌄` rest clock runs under held gestures and under menus.** It stops only for a drag or a float carry (`floats.rs:1985`). A divider, a thumb or a selection resting 250 ms on a `⌄` opens its menu mid-gesture; a rest on a tab-menu row or the palette over a pane head's `⌄` opens the pane menu and closes the menu under the hand (`close_popups_except`, through `toggle_pane_menu`).
- **G-11. A float's own pill cannot be pressed.** `press_float` claims the point as `Body` (`floats.rs:2580-2624` never names the pill) and walks the body ladder (`mouse.rs:1078`); `press_in_pane_surface` (5258) is never reached. The comment at `mouse.rs:931-938` says the opposite.
- **G-12. The command rail hovers under floats and the glance card** (bare `pane_at`, `panes.rs:695`, `733`; the gate at `mouse.rs:2168-2169` excludes only search, strip and routes): the hidden rail fans out, the cursor becomes a finger, and its tick card is the first tip candidate (`mouse.rs:2176`).
- **G-13. Census #21, wider than counted.** The five tests (`hit_pane_ghost` `seats.rs:6452`, `hit_preview_rail` 17135, `hit_files_root` 17178, `hit_text_size` 17272, `hit_preview_head` 18605) test the solved rectangle. The painter draws on `geometry.content` (solved plus the FLIP offset) clipped to `geometry.clip` (`seats.rs:9867-9868`, `clip_pane_chrome` at 10957, 11205; `PaneTransform::applied_to`, `main.rs:30446`), and on `card_rect_of` for a pane riding its resize card (`seats.rs:9831`). During the ~200 ms a control answers where it is not drawn, and where it is drawn the ladder answers nothing, `chrome_mouse_input` gets `None`, and the press goes to `pane_frame_hit` (`mouse.rs:5407`) on the solved grid: a selection or a forwarded press at a cell the glass is not showing. `hit_chrome_in_motion` (`seats.rs:6343-6420`, via `pane_chrome_box`, `seats.rs:9564`) is the one rung that does it right. The other seven rungs of the ladder, `pane_at`, `pane_hit_context`, `focus_pane_at`, `files_body_at` and `terminal_column_bar_under` take no motion either, and no rung follows `card_rect_of`.
- **G-14. Per-tab latches are stranded by a tab switch** (`tabs.rs:194-196` clears window fields only): back on the old tab the thumb follows the hand, the stale latch eats the next release ahead of lower rungs (`mouse.rs:3243-3330`), and while it stands every page in the window refuses input (`a_gesture_holds_the_pointer`). With `preview_text_drag` the page keeps its span (`preview.rs:8187-8195`).
- **G-15. Settings drags end on any button's release and survive the sheet** (`main.rs:47462` is the only clear): Esc mid-slider leaves `settings_slider_drag` set, and `a_gesture_holds_the_pointer` then switches off every page in the window until the sheet is reopened and a button released inside it.
- **G-16. A picture pan is never healed**: the pane closed or the buffer swapped, every move is swallowed or pans another picture until a release reaches the ladder.
- **G-17. A pane press records a seat, not a leaf**; `begin_pane_drag` reads the active tab at travel (`panes.rs:4308-4311`), so Ctrl+Tab while holding a head, then a move, carries the same seat number in the other tab.
- **G-18. `focus_pane_at` reads bare `pane_at`** (`main.rs:48603-48606`), at `mouse.rs:3408` for every left press that reaches the chrome router: a press on an open icon rail's row or the focus column standing over panes moves the layout and keyboard focus to the pane under it. The hover gates the same case with `panel_covers` (`mouse.rs:2602`).
- **G-19. Cell readers under docked chrome.** `row_under`'s last arm (`main.rs:50654`) and `pane_hit_context` arm the glance card and underline a link for a path printed under the ghost `⌄`/folder, the text-size pill or an open rail (the underline excludes only the command rail and search, `mouse.rs:2200`).
- **G-20. Two float tests.** `pointer_over_a_float` walks `drawn()` with the fade frame; the router walks `hit_order()` with `float_hit_at`; a fading window is a layer to one and not the other.
- **G-21. Glance latches record no button** (`peek.rs:1989`, `2002`; releases at `mouse.rs:4906-4913` test none): left on the card's head, right press and release, and the preview opens with the left button still down.
- **G-22. The router runs three to seven times per move** (`drive_web_pointer` through `in_pane_surface_at`, `note_video_hover`, the `free` gate, `update_chrome_hover`, `drive_search_hover`, `drive_notice_hover`, `owned_tooltip_anchor_at`), each with two caption measurements when a float is up (§5).
- **G-23. Stale comments**: `mouse.rs:6545-6552` (a forwarded route's release "does not always clear" — it does since `release_owned_gesture`), `mouse.rs:931-938` (G-11).
- **G-24. `preview_surface_at` uses inclusive edges** (`preview.rs:2467`) where every other test is half-open, so a shared border has two owners.
- **G-25. Paint-time hover reads ignore the layers above them** (§1.3's seven): each tests the pointer against its own surface while the frame is built, so the glance card's foot, a toast's `×`, a video's play mark or a picture's hand can light under a menu, the palette or a float that covers them. They become readers of the router's memo (cut 8d).
- **G-26. Readers in the modal band outside the doors** (§1.3's four): they read the pointer fields directly. Their behaviour is not wrong today (the sheet covers the window), but they are readers the router and the guard must own; they move behind the router in cut 8d and are debt rows until then.

**Census #22** (paste while the files column holds the keyboard) is not a pointer defect: `a_surface_above_the_clipboard_rung_holds_the_keyboard` (`runtime/keyboard.rs:2101-2117`) does not list the files column, so `clipboard_seat`/`paste_seat` (`menubar.rs:699-725`) fall to the terminal. The pointer's only part is that a press is one way the column gets the keyboard (`focus_pane_at`). It stays with T-HARDCODE-047 slice 2.

---

## 2. The model

Two objects. **The router** answers "what is under the pointer". **The capture** answers "who owns this gesture". Each pointer event is offered to the capture first; only an event the capture does not own reaches the router.

### 2.1 The router — one walk over the whole stack, in paint order

`Runtime::pointer_layer_at(&self, facts: &PointerFacts, position) -> Option<PointerHit>` walks **one list**, `POINTER_LAYERS_TOP_FIRST`. The list is `OverlayStack::flattened`'s bands read top first, minus the bands that take no pointer, with the pane's own planes appended beneath the overlay:

| # | layer | claims | hit test it calls (kept, moved behind the router) |
|---|---|---|---|
| 1 | glance card | its frame | `file_peek_holds`, `file_peek::press_at` |
| 2 | toasts | each card's frame | `toast::at` |
| 3 | palette | its frame (list, field, padding) | `palette::hit`, `palette::wheel_part` |
| 4–8 | tab, term, git, pane, file menu | each menu's frame, child list included | `profiles::*_menu_hit` |
| 9 | modal band | the whole window while a full-window card or the settings sheet is up; otherwise the frames of the profile, root, graph-filter and preview menus | `a_modal_covers_the_window`, the four menus' hit tests, `settings::hit` |
| 10 | floats, front first | each risen frame, whole, its pill included | `float::float_hit` (+ the pill's `notice::claim`) |
| 11 | download sheet | scrim and card, per page | `websheet::covers`, `websheet::hit` |
| 12 | in-pane surfaces, `IN_PANE_SURFACES_TOP_FIRST` | the capsule's frame; a strip's frame when it has something to press | `search::hit`, `notice::claim` |
| 13 | docked chrome | tab list, rail body, heads and their controls, dividers, files rows, Git page, caption buttons, the title-bar handle | the `seats::hit_*` ladder, `seats::title_bar_drag_point` |
| 14 | pane furniture | formula tools, command rail, terminal thumb lane and foot mark, preview body and block bars, video bar | their own tests (`termscroll::thumb_holds`, `lane_holds`, `video_seat::slot_at`, …) |
| 15 | hosted page | the page's shown bounds (Windows; macOS see §2.5) | `WebSeat::shown_at` |
| 16 | pane body | preview body ladder (`PreviewBodyRung`), else terminal cells (formula band first) | `preview_surface_at`'s rectangles, the grid's `hit_test_frame`, the renderer's math hit |

Never asked: layout peek, key hint, card hint, tooltip, drag ghost, window ring (today's `BANDS_OVER_IN_PANE_THAT_TAKE_NO_POINTER`, `main.rs:34637`).

`PointerHit` names the layer and its part (`Float(id, FloatPart)`, `Menu(Popup, hit)`, `InPane(InPaneSurface, part)`, `Chrome(ChromeTarget)`, `Page(LeafId)`, `Body(…)`, …). Today's `PointerTarget` (`main.rs:34489`) is its lower half.

**`PointerFacts` — what the walk reads.** A value built once per frame where the overlay is built (the same pass that already measures every caption the walk needs, so measuring moves rather than multiplies): per layer the rectangles the paint used, with clip and motion applied; per float its risen frame, its part geometry and its two caption widths (today measured inside every `float_hit_at` call, `floats.rs:2495-2507`); the Git and graph geometry a float shows, **borrowed** by index rather than cloned (today cloned per call, `floats.rs:2536`, `2552`). Its vectors are cleared and refilled, never dropped, so a steady window reallocates nothing. The walk takes `&self` and `&PointerFacts` and nothing else — no `&mut` renderer, so it **cannot** shape text — and iterates `hit_order` in place (no `Vec` of ids, today `floats.rs:2513`).

- **R-1 (one router).** Every reader that asks where the pointer is takes the answer from `pointer_layer_at`: hover, press, a release no capture owns, wheel, tip, cursor shape, the flyout, glance and layout-peek intents, the `⌄` rest clocks, the hosted page's hover, every re-ask site and every paint-time hover read (§1.3's last paragraph). No reader walks a list of its own and none calls a layer's hit test directly. *Cells: B1–B5, B9–B12, B14, B16, B17.*
- **R-2 (paint order is pointer order).** `POINTER_LAYERS_TOP_FIRST` is derived from the list `OverlayStack::flattened` paints; reordering the paint reorders the router. The first claim is the whole answer. Where today's press order stands a layer above one painted over it (the seven cards above the menus, G-9), the disagreement is removed by making it unreachable: raising a full-window card closes every popup (`close_every_popup`), as a tab switch already does. *Cells: B1, B12, B13.*
- **R-3 (a layer claims what it paints).** A layer claims exactly the pixels it painted on the frame on the glass, with its clip and its motion: the docked ladder and the pane-body tests take `pane_transforms` as `hit_chrome_in_motion` does, a control is hit only inside both its drawn box and its clip, and a pane on its resize card is hit on the card. Edges are half-open everywhere. *Cells: B15, B18.*
- **R-4 (one walk per event, no shaping, no allocation).** The walk runs at most once per pointer event; every reader of that event reads the memoised answer, emptied at the top of the event (precedent: `pointer_reference`, `mouse.rs:529-537`). The walk allocates nothing and shapes nothing; both are pinned by tests (§4.1 cut 1). *Cells: none (cost only, §5).*

### 2.2 The capture — one record per latched gesture, one slot per process

```text
struct PointerCapture {
    window:  WindowId,                // the window whose press latched it
    owner:   CaptureOwner,            // which gesture, carrying its own payload (one variant per row of §1.2)
    button:  MouseButton,             // the button that latched it, after the macOS secondary-click translation
    started: PointerHit,              // the router's answer at the press
    os:      Option<NativeWindow>,    // GetCapture at the press (DividerDrag::capture and DragGuard, generalised)
    last:    Option<ReleaseSnapshot>, // only for the two owners that must tell somebody (below)
}
```

**Storage: `App::pointer_capture: Option<PointerCapture>` — one slot for the application, not one per window.** A window's runtime reaches it through `self.app` (as it already reaches `app.drag_broker`). The slot replaces `divider_drag`, `tab_press`, `pane_press`, `row_press`, `drag`, `float_head_press`, `float_drag`, `file_peek_press`, `thumb_grab`, `video_bar_drag`, `preview_body_drag`, `preview_block_drag`, `preview_image_drag`, `preview_selecting`, `preview_text_drag`, `terminal_thumb_drag`, `terminal_column_drag`, `mouse_route`, `math_tool_pressed`'s route half, and the two settings drags. Their payloads become `CaptureOwner`'s variants, and each payload that names a pane names it by `LeafId` or `PasteTarget` (tab included), so a per-tab latch can no longer be stranded on a tab nobody is looking at and a pane press cannot change tabs (G-14, G-17). One slot is the argument `DividerGrip`'s own doc makes for one field instead of two (`main.rs:19646-19651`): "two at once", "two windows at once" and "a latch nobody owns" stop being representable. A six-pixel press that becomes a drag is one capture whose variant changes in place. `DragBroker` (`main.rs:34088`) stays the drag's cross-window state; its `guard` becomes the capture's `os` field plus the screen sample.

`ReleaseSnapshot` exists for the two owners whose release is a message to somebody else:
- the forwarded press: `{target: PasteTarget, cell: GridHit, sgr: bool, button, modifiers}` — the cell and **the modifiers of the last mouse report actually sent** (press or motion). A synthetic release carries those modifiers, never the keys held when the ending happened: a release caused by `Ctrl+Tab` must not report `Ctrl` (Q2).
- the page press: `{leaf}` only. **The point and the buttons are the seat's, not the capture's**: `WebSeat` already keeps the button mask it hands the engine (`buttons`, updated at `webhost.rs:4078-4085`), and cut 7 adds one field beside it, `last_point: Option<(i32, i32)>`, written in `WebSeat::send_mouse` from the `point` it already computes **relative to the page's own bounds** (`webhost.rs:4076`) on every event it forwards. That is the engine's coordinate space and does not depend on where the page sits in the window. The seat's own hide transition (§2.4) reads `buttons` and `last_point` from itself; the application capture never carries them, so there is one copy and it cannot disagree with what the engine was last told.

Every other owner's release is a state change of Folio's own that needs no geometry.

- **R-5 (one capture per process) and the press precedence.** A press that a layer takes and whose gesture needs its release latches the application's one `PointerCapture`. **Every press, in every window, is first compared with the slot, by button and never by window:**
  1. **slot empty** → the press is routed;
  2. **slot held and the press is of the slot's own button** → the held capture is **stale** (a button cannot go down twice without coming up; its release was lost — eaten by a platform, delivered to another process, or never sent): the capture ends with *cancel*, then the press is routed, in whichever window it arrived;
  3. **slot held and the press is of another button** → R-8: the press is swallowed and **its button is remembered** in the application's `swallowed: ButtonSet` (beside the slot on `App`), in whichever window it arrives.

  **A swallowed button stays swallowed until its own release**, whatever happens to the capture meanwhile: a release is first compared with `swallowed` — a release of a remembered button is swallowed and clears that one bit — and only then with the slot. So the chord *left held in A → right down in B (swallowed) → left up in A (the capture ends, the slot empties) → right up in B* ends with the right release swallowed, not routed to whatever is under the pointer in B (cell B22). A remembered bit is cleared only by its matching release, or — if that release was lost to another process — by the next **press** of the same button, which proves the bit stale exactly as rule 2 proves a stale capture, and is then routed. The window plays no part here either.

  The window plays no part in the decision, which is what makes it hold on macOS, where there is no process-wide capture sample: on Windows, rule 3 in another window is unreachable while the OS capture stands (the capturing window receives all mouse input), and rule 2 is how a lost release is recovered on either platform. *Cells: A·0 for every row (unchanged); B20 (rule 2), B21 (rule 3 in another window).*
- **R-6 (the release goes to the owner).** While a capture is held, the release of its button is delivered to the owner, wherever the pointer is and in whichever window it arrives, before any layer is asked; the router's answer at that point is passed to the owner as information (a tab press asks where it was let go, a drag lands, a rendered selection opens its link only on a still click) and no layer acts on the release. The capture ends. *Cells: A·a.*
- **R-7 (moves go to the owner; non-owner hover is frozen).** While a capture is held, each move drives the owner — and for a page owner the move **is** forwarded to that page (its DOM hover and its own element capture are the page's business). Every *other* hover affordance neither arms nor lights: chrome hover, tips, the glance, flyout and layout-peek intents, the `⌄` rest clocks, every non-owner page's hover (each hears only the off-page move). Two position facts still run because owners need them: the rail zone (a carried pane opens the icon rail, `mouse.rs:1588-1612`) and the drag survey. *Cells: A·h.*
- **R-8 (other buttons go nowhere).** While a capture is held, a press of any other button is delivered to nothing — no layer, no menu, no child program, no page — and **that button's release is delivered to nothing too, even if it arrives after the capture has ended** (R-5 rule 3's `swallowed` memory). The gesture ends by its own release or by an ending of R-9. This **supersedes** the 2026-10-04 rule that a non-matching release under a forwarded press "is handed back to `mouse_input`'s ordinary road" (`DESIGN.md:13766`); it needs the owner's yes (Q1) and lands with a dated DESIGN entry saying so. **Status: pending the owner** — queued 2026-10-09; the approval is recorded in this note and in DESIGN before cut 4 is built, and cuts 1–3 do not depend on it. *Cells: A·b, B21, B22.*
- **R-9 (every capture can end without its release).** Each owner answers, by an exhaustive match, these endings:
  - **cancel** — the capturing window's blur; the OS capture no longer the one taken at the press (sampled each turn while a capture is held, the divider's sample at `dpi.rs:183` generalised); the capturing window hidden or closed (the summoned window's `hide_quake_window`, `runtime/quake.rs:107`, included); `ScaleFactorChanged` for the owners whose payload is a pixel quantity taken at the press (the float's grab, the thumbs' grab, the picture's anchor) and for a page press (§2.4).
  - **`OsDragHandoff`** — **its own ending, not a cancel**: taken synchronously by the code that is about to enter an OS drag-out loop (§7). It runs none of the cancel verbs: a drag does **not** go home, nothing is restored, no release verb runs; the payload has been given to the system, so the capture and the broker are simply cleared. Only the drag owners (C5, and C4 once a row press has become a drag) answer it; every other owner's arm is empty, because only a drag can start a drag-out.
  - **owner gone** — the tab, pane, float, surface, page or settings sheet it names is closed or replaced.
  - **owner off the glass** — applies only to owners whose payload names a **tab-bound surface** (C1, C3, C4, C10–C21; a tab press, C2, names a tab of the strip, which is the window's): its tab is no longer in front, or its pane is zoomed out of view, while it still exists. **It does not apply to a drag (C5)**: a drag's owner is the payload in the hand, drawn as the ghost over whichever tab is in front, and the spring that switches tabs (`advance_drag_spring`, `mouse.rs:632-647`) is the gesture's own aim, not its owner leaving. Nor to the window-level owners C2, C6–C9 and C22, whose surfaces (a peek, a float, the glance card, the settings sheet) end by *owner gone*.
  - Esc keeps exactly today's meaning (it cancels a drag and a divider, `keyboard.rs:1124-1126`); the model adds no keyboard route.

  What each ending does is the owner's: a divider puts its ratio back (taken **before** the active tab changes, because `cancel_divider_drag` reads the active tab's seats, `panes.rs:3936-3940`); a drag goes home; a six-pixel press is dropped; a thumb, scrub, pan, carry or selection keeps what it already wrote and drops the release's verb (a rendered selection's link, as `cancel_preview_text_drag` does); a forwarded press or a page press **sends its release from `last`** when its target still exists — before the tab's seats change, and for a page before its presence turns hidden (§2.4). *Cells: A·c, A·d, A·e, A·f, A·g.*
- **R-14 (keyboard focus is orthogonal).** A capture neither grants nor keeps keyboard focus. The press that latches it moves focus exactly as today (`focus_pane_at`, `press_web_page`'s `focus_page`, `web.rs:1912`, which ends at `MoveFocus(PROGRAMMATIC)`, `bt-platform/src/webview.rs:3180`), and no ending moves focus. Focus and tab-switch logic stay authoritative.

### 2.3 Wheel, tips and cursor by the same router

- **R-10 (the wheel).** A notch goes to the layer the router names, with two rules for a held capture: while a capture **not** owned by a page is held, the page layer answers nothing (a notch over a page is swallowed — today's subtraction, `web.rs:1734`, made to cover every capture); while a **page** owns the capture, the notch goes to that page. Each layer declares, by an exhaustive match, a station of its own or *swallow* (today's `OverInPane::wheel`, `main.rs:34582-34598`, extended to every layer). A floating window is one layer over its whole frame: its body scrolls its tenant (tree, Git page, graph, document, picture, page), and its head, foot, rail and grip swallow. Nothing beneath a claimed point scrolls. *Cells: A·w, B4 (wheel), B6–B8.*
- **R-11 (tips).** A tip anchor is registered under the layer it belongs to. The tip under the pointer is the anchor whose layer is the router's answer and whose box holds the point; today's in-pane filter (`owned_tooltip_anchor_at`) is the in-pane case of this rule. *Cells: B9, B10, B11 (tip).*
- **R-12 (cursor).** The shape is the capture owner's while a capture is held (pinned today by `the_pointer_keeps_one_shape_for_the_whole_drag`), and the router layer's otherwise; a hosted page's own cursor applies only when the router names the page or the page owns the capture. *Cells: B4, B5, B11 (cursor).*

### 2.4 The hosted page on Windows: a layer, and a press in it is a capture

The page is composition-hosted (`CreateCoreWebView2CompositionController`, `bt-platform/src/webview.rs:1796-1819`): it receives no window messages, and every move, press and release it hears is forwarded by `WebSeat::send_mouse` (`webhost.rs:4067`) through `SendMouseInput` (`webview.rs:3204`). The router sees it like any other layer.

- **R-13 (a page owns the gesture it began).** A press the router gives to the page latches `CaptureOwner::Page(leaf)`. Its moves and its release are forwarded to that page wherever the pointer is, unclamped (a point outside the bounds is accepted, `webhost.rs:4061-4066`). This is a **view-level** capture: Folio keeps feeding the one view, and the engine applies its own element-level `setPointerCapture`/`releasePointerCapture` inside it, which Folio neither sees nor needs to; a script's `releasePointerCapture` does not end Folio's capture — only the button's release or an R-9 ending does. While any other capture is held, every page hears only "the pointer left". (Q3.) *Cells: row C21.*
- **Ordering, part 1 — a page is never hidden with a button down.** `WebSeat::send_mouse` sends nothing once the seat is not `Shown` (`webhost.rs:4073`), so a synthetic release after the page is hidden is lost. The rule is the seat's own, so no caller can get the order wrong: **the transition of a seat from `Shown` to anything else first sends a button-up for every bit still set in its `buttons` mask (`webhost.rs:4078-4085`), at `last_point`, through the bounds still in force**, then hides. The capture's *off the glass* and *owner gone* endings for a page are therefore "clear the slot" — the seat has already spoken, or speaks in the same call. The transition is decided in `sync_web_page` (`runtime/web.rs:318`) from `webhost::web_presence` (`webhost.rs:962-980`) and written into the seat by exactly two items, `WebSeat::place` (`webhost.rs:3570`, `self.wanted = presence` at 3577) and `WebSeat::park_for_handoff` (`webhost.rs:2457`, 2458) — the doors a page leaves the glass by (tab switch, a modal, a zero rectangle, a fit step that drops the seat, a hand-off to another window). Cut 7 puts the button-up in both, before `wanted` changes.
- **Ordering, part 2 — DPI.** `last_point` is page-local, so a move of the page inside the window does not stale it. A **scale** change does: the engine's coordinate space is the controller's raster at the old scale. So `scale_factor_changed` (`runtime/dpi.rs:424`) ends a page capture with *cancel* as its **first** statement — before `resize` re-lays the seats and the controller's bounds and rasterization scale change — and the seat's button-up goes out in the old space. A forwarded press's `last` is a cell, so it needs no rebasing.

### 2.5 macOS pages are out of this ticket

`WKWebView` is a real `NSView` that AppKit delivers to directly; `send_mouse` is a no-op there (`bt-platform/src/macos_webview.rs:2016-2031`), and whether a press over a page reaches the page or Folio's surface view — the page's slot stands **under** that view (§13.24) — is the open hit-testing question of DESIGN §13.29 (`DESIGN.md:9705`). This ticket does not settle it. Consequences, cell by cell:

| cell(s) | Windows (this ticket) | macOS (this ticket) |
|---|---|---|
| C21·a, b, d, e, g, h, w (the page owns its gesture) | → per Table A | **not built**: `CaptureOwner::Page` is never latched; whatever AppKit and §13.29 decide stays as it is today |
| B4 hover, right/middle press, wheel, cursor (a float over a page) | → per Table B | **as today**: if events reach the surface view the router answers them like Windows; if they reach the `WKWebView`, AppKit does |
| B5 hover, cursor (a menu, palette, toast or glance card over a page) | → per Table B | as the row above |
| every Folio-owned capture crossing a page (C1–C20, C22) | R-6 by the capture slot | **holds by the platform**: AppKit sends `mouseDragged:`/`mouseUp:` to the view that took `mouseDown:` (Folio's surface view), wherever the pointer goes |
| router layer 15, a **press** that reaches Folio's surface view over a page | press, hover, wheel, cursor; a press latches `CaptureOwner::Page` | **today's behaviour, kept and pinned** (below): the press is consumed; it blurs a name editor, focuses the pane or raises the float holding the page, hands the page the keyboard (`press_web_page`, `web.rs:1852-1929`); nothing is forwarded (`send_mouse` is a no-op) and **no capture is latched** |
| router layer 15, hover, wheel, cursor | as Table B | as today |

**The macOS fallback is kept through every generic cut.** Today a press over a page that reaches Folio's surface view is consumed by the page arm (`mouse.rs:5270`) and runs `press_web_page`'s focus and raise work. When cut 8a replaces the press road with one dispatch on `PointerHit`, the `Page` answer keeps calling that same door on every platform — the only platform difference is whether the press then latches `CaptureOwner::Page`, decided by a pure function of the platform, `page_press_latches_a_capture(bt_platform::host_platform())` (the house passes the platform in rather than reading `cfg!`, as `upright_wheel` does, `mouse.rs:5615-5619`). So a surface-delivered press on macOS can never fall through to the pane body. Pinned by `a_page_press_that_reaches_the_surface_view_is_consumed_and_focuses` (cut 8a): with the press dispatch handed `HostPlatform::MacOs`, a press over a page's shown bounds is consumed, the pane under it takes the focus (or the float holding it is raised), the page's seat is named as holding the keyboard, nothing reaches the pane body, and the slot stays empty; mutation: let the `Page` answer fall through when no capture is latched. It runs on every platform's test build and in the Mac gate. This is cell U23.

Of §3's 153 changed cells, these 13 (C21 ×7, B4 ×4, B5 ×2) are Windows-only. The page-capture tests of cut 7 are `cfg(windows)`. The macOS page press path beyond today's fallback is a follow-up owned by §13.29's ticket; it inherits R-13 if that ticket routes page presses through Folio's view.

### 2.6 The rules as RULES row 28 would carry them

Row 28 says the press half is "not yet folded" because the rung order of `mouse_input` is unwritten (`docs/RULES.md:775-779`). The model is that order written down. When the last cut lands, row 28 gains:

> **Pointer ownership (T-POINTER-CAPTURE).** (1) One router, `pointer_layer_at`, answers every pointer question by walking `POINTER_LAYERS_TOP_FIRST`, the overlay's paint list read top first plus the pane's planes, over the frame's `PointerFacts`; the first claim is the whole answer, a layer claims exactly the pixels it painted, motion and clip included, and the walk neither shapes text nor allocates. (2) A press whose gesture needs its release latches the application's one `PointerCapture` (window, owner, button, the router's answer, the OS capture). (3) While captured, the release of that button goes to the owner wherever the pointer is, before any layer; moves go to the owner; every other hover is frozen except the rail zone and the drag survey; any other button goes nowhere. (4) A press of the held capture's own button proves it stale and cancels it before being routed; a press of another button goes nowhere; neither depends on the window. Every capture answers cancel (blur, OS capture lost, window hidden, a scale change for pixel-quantity owners), owner gone and, for tab-bound owners, owner off the glass; a drag also answers an OS drag hand-off, which clears it with no cancel verb; a forwarded press sends its release from its last cell with its last modifiers; a page seat never leaves the glass with a button down. (5) The wheel, the tip and the cursor take the router's layer; every layer either scrolls a notch or swallows it; under a capture no page but the owner hears the wheel. (6) A press inside a hosted page on Windows is a view-level capture owned by that page. (7) Keyboard focus is not the capture's. (8) The pointer fields, the `PhysicalPosition` type, a button or wheel event type, a system cursor read, a pointer `WindowEvent` variant or the touch `PanStep` named by any `bt-app` item outside `crate::runtime::pointer` fails `every_pointer_read_is_the_routers_or_a_captures`.

---

## 3. Behaviour table

Notation: **=** the cell is unchanged; **→** the model's answer, after today's; **—** not applicable. The *cut* is the §4 cut that flips the cell. **How the totals are computed:** every cell of Tables A and B is exactly one of `=`, `→`, `—`; the Σ column of Table A is that row's count (changed / unchanged / n/a), and the totals are the column sums; Table B counts its rows the same way; each Table C row is one unchanged cell. A behaviour that is already a cell of Table A or B is **not** repeated in Table C — it is written as that cell's pin (§3.1's pin list). Table C therefore holds only cells no A or B row covers.

### 3.1 Table A — a held gesture (22 rows × 10 events = 220 cells)

Events: **0** the release of its own button where nothing eats it; **a** the release of its own button over a layer that eats releases today (a full-window card, the settings sheet, a menu's frame, a palette row, the glance head, a float carry's release arm, a hosted page); **b** another button pressed and released; **c** Esc; **d** blur; **e** OS capture lost without a blur; **f** owner gone; **g** owner off the glass (R-9 scope); **h** a non-owner hover affordance under the held gesture (`⌄` rest, tip, flyout, page hover); **w** a wheel notch while held (pointer over a hosted page; elsewhere every row is `=` and Table B's float/wheel cells apply).

| # | gesture | 0 | a | b | c | d | e | f | g | h | w | Σ →/=/— |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| C1 | divider | = commits | eaten, then cancelled by the next turn's capture sample (ratio jumps back) → commits (cut 3) | right press opens a menu or reaches the shell → nothing (cut 4) | = cancel | = cancel | = cancel | = dropped | dropped keeping the half-dragged ratio → cancelled, ratio restored before the tab changes (cut 5) | `⌄` rest opens its menu mid-drag → frozen (cut 6) | = (page subtracted today) | 4/6/0 |
| C2 | tab press | = click / activation | eaten; next move > 6 px drags the tab with no button → the click lands (cut 3) | right press raises the tab menu under the held press → nothing (cut 4) | = (no-op; the release is still a click) | = cleared | stale → dropped (cut 5) | = cleared | — | tip and `⌄` clocks re-arm under the held press → frozen (cut 6) | a page under the pointer scrolls → swallowed (cut 6) | 5/4/1 |
| C3 | pane press | = focus / double-click zoom | eaten (e.g. 2 px into a page below the head); phantom pane drag → the release lands (cut 3) | right press opens the pane menu → nothing (cut 4) | = | = cleared | stale → dropped (cut 5) | stale; the next move begins a drag of a dead seat (side effects run first) → dropped (cut 5) | carries the same seat number in the new tab → dropped (cuts 3, 5) | → frozen (cut 6) | page scrolls → swallowed (cut 6) | 7/3/0 |
| C4 | row press | = select / fold / double-click open | eaten; phantom row drag → lands (cut 3) | right press opens the file menu → nothing (cut 4) | = | = cleared | stale → dropped (cut 5) | = (payload resolves to nothing) | survives; payload resolved against the new tab's column → dropped (cut 5) | → frozen (cut 6) | page scrolls → swallowed (cut 6) | 6/4/0 |
| C5 | tab / pane / row drag | = lands per the release table | eaten, then the broker's guard cancels it home → lands where released (cut 3) | right press opens menus under the carry → nothing (cut 4) | = home | = home | = home (broker guard) | = dropped | = (not in R-9's off-glass scope: the spring is the drag's own aim) | = (already frozen; the rail zone runs) | = (page subtracted) | 2/8/0 |
| C6 | peek header press | = nothing (cleared) | eaten above `mouse.rs:5007`; next move promotes and carries the window → nothing (cut 3) | the second press clears it → refused, the press stays (cut 4) | = | not cleared → dropped (cut 5) | stale → dropped (cut 5) | = refused | = (window-level) | → frozen (cut 6) | page scrolls → swallowed (cut 6) | 6/4/0 |
| C7 | float move / resize | = ends where it is | eaten by a card or menu above 5012; the window follows the hand → ends (cut 3) | right press opens menus → nothing (cut 4) | = (no-op) | not cleared; follows the hand after Alt+Tab → ends where it is (cut 5) | stale → ends (cut 5) | = dropped | = (window-level) | = (already frozen) | = (page subtracted) | 4/6/0 |
| C8 | glance head press | = opens the card's door | eaten by a card; next move pins the card into a float → opens (cut 3) | the right button's release opens the door with left still down → nothing (cut 4) | = (Esc hides the card) | = cleared | stale → dropped (cut 5) | = cleared | — | `⌄` clock runs at the top of the move → frozen (cut 6) | page scrolls → swallowed (cut 6) | 5/4/1 |
| C9 | glance thumb | = ends | eaten by a card → ends (cut 3) | the right release ends the left drag → nothing (cut 4) | = | = cleared | stale → dropped (cut 5) | = cleared | — | → frozen (cut 6) | = (page subtracted) | 4/5/1 |
| C10 | video scrub / volume | = ends, dwell restarts | eaten; the scrub follows the hand → ends (cut 3) | → nothing (cut 4) | = (no-op) | not ended → ends keeping the fraction (cut 5) | → ends (cut 5) | silent stale latch that eats the next left release → dropped (cut 5) | stale on the old tab's surface → dropped (cut 5) | → frozen (cut 6) | = | 7/3/0 |
| C11 | preview body thumb | = ends | eaten → ends (cut 3) | → nothing (cut 4) | = | not ended → ends (cut 5) | → ends (cut 5) | = healed | stranded on the old tab → ends (cuts 3, 5) | → frozen (cut 6) | = | 6/4/0 |
| C12 | block thumb | = ends | eaten → ends (cut 3) | → nothing (cut 4) | = | not ended (docked) → ends (cut 5) | → ends (cut 5) | = healed | stranded → ends (cuts 3, 5) | → frozen (cut 6) | = | 6/4/0 |
| C13 | picture pan | = ends | eaten → ends (cut 3) | → nothing (cut 4) | = | not ended → ends (cut 5) | → ends (cut 5) | never healed; swallows or pans another picture → dropped (cut 5) | stranded → ends (cuts 3, 5) | → frozen (cut 6) | = | 7/3/0 |
| C14 | edit-surface selection | = ends | eaten, a page included (not in `a_gesture_holds_the_pointer`); extends with no button → ends (cut 3) | → nothing (cut 4) | = | not ended → ends keeping the selection (cut 5) | → ends (cut 5) | = healed | stranded → ends (cuts 3, 5) | → frozen (cut 6) | page scrolls → swallowed (cut 6) | 7/3/0 |
| C15 | rendered-text selection | = ends; a still click opens its link | eaten by a card or menu (a page is subtracted) → ends (cut 3) | → nothing (cut 4) | = | = cancel | → cancel (cut 5) | = healed | stranded, the page holds its span → cancel (cuts 3, 5) | → frozen (cut 6) | = | 5/5/0 |
| C16 | terminal thumb | = ends | eaten → ends (cut 3) | → nothing (cut 4) | = | not ended → ends (cut 5) | → ends (cut 5) | swallows every move until a release → dropped (cut 5) | stranded → ends (cuts 3, 5) | → frozen (cut 6) | = | 7/3/0 |
| C17 | terminal foot mark | = ends | eaten → ends (cut 3) | → nothing (cut 4) | = | not ended → ends (cut 5) | → ends (cut 5) | as C16 → dropped (cut 5) | stranded → ends (cuts 3, 5) | → frozen (cut 6) | = | 7/3/0 |
| C18 | terminal selection | = finished in its pane | = (owned release runs first) | right press raises the terminal menu; a formula press overwrites the route → nothing (cut 4) | = (Esc goes to the shell) | not ended; moves extend it after Alt+Tab → ends keeping the selection (cut 5) | → ends (cut 5) | = dropped | = dropped | tips and `⌄` clocks run under it (`mouse.rs:2175`, `1587`) → frozen (cut 6) | = | 4/6/0 |
| C19 | forwarded press | = released in its encoding | = (owned release runs first) | other roads (menus, the formula block) take it; the formula press overwrites the route → nothing (cut 4) | = (Esc goes to the program) | latched; the child stays pressed → release from `last`, last modifiers (cut 5) | → release from `last` (cut 5) | = dropped, nothing sent | dropped, the child stays pressed → release from `last` before the seats change (cut 5, Q2) | tips and `⌄` clocks run under it → frozen (cut 6) | page scrolls (Forward is not in the subtraction) → swallowed (cut 6) | 6/4/0 |
| C20 | formula block | = ink off | = | any other button's release ends it; a second press replaces it → nothing (cut 4) | = | not ended → ink off (cut 5) | → ink off (cut 5) | = (the record now names the leaf) | = dropped | `⌄` clock runs → frozen (cut 6) | = | 4/6/0 |
| C21 | press inside a page (Windows) | = forwarded | released outside: never forwarded, the page's button stays down → forwarded to the owning page (cut 7) | forwarded to whatever page is under the pointer → nothing (cut 7) | = (the page has the keyboard) | not told → button-up at `last_point` (cut 7) | → button-up (cut 7) | = nothing | page hidden, not told → the seat sends its button-up before hiding (cut 7) | moves outside the page are not forwarded → forwarded unclamped; no other layer hovers (cut 7) | notch over another page scrolls that page → goes to the owning page (cut 7) | 7/3/0 |
| C22 | settings slider / bar | = ends | = (the sheet is modal) | a right release ends the left drag → nothing (cut 4) | Esc closes the sheet and the latch survives, switching every page off → dropped (cut 5) | not ended → dropped (cut 5) | → dropped (cut 5) | sheet closed: survives → dropped (cut 5) | — | the `⌄` rest clock runs under the scrim (`mouse.rs:1587` is asked before the sheet's arm at `1775`) → frozen (cut 6) | = (the sheet takes the notch) | 6/3/1 |
| | **Σ** | | | | | | | | | | | **122 / 94 / 4** |

**Pins of Table A cells** (what were Table C rows in the first revision, kept as pins, not counted twice): C1·0 — the divider-drag tests of `app_panes_tests.rs:977-1813` drive the release into `chrome_mouse_input` directly, so they pin the commit and **not** full dispatch; cut 3 adds the full-dispatch pin. C1·0 also carries minimum-size sovereignty (`panes.rs:3870-3891`, `SizePolicy::Sovereign`; `app_dpi_tests.rs:18-67`). C5·0 — `app_mouse_tests.rs:2962-3229`. C5·e — `main.rs:76171` `one_guard_answers_every_way_a_cross_window_gesture_is_taken_away`. C5·h (the rail zone runs) — `app_mouse_tests.rs:1599`. C19·0 over the capsule and the strip — `notice_app_tests.rs:208`. C19·b on the cell road — `app_mouse_tests.rs:3789` `a_chord_under_a_forwarded_gesture_leaves_it_whole` (kept as written; R-8 is stricter). C19·f — `app_mouse_tests.rs:3689` (its tab-not-on-top half moves to C19·g).

### 3.2 Table B — no gesture held, and four cases under a held gesture (35 cells)

| # | point | event | today | model | cut |
|---|---|---|---|---|---|
| B1 | a profile, root or preview menu row over a pinned float | press | the float takes it (`mouse.rs:5049` before 5060-5237) | the row | 8a |
| | | hover | = the row lights | = | — |
| B2 | a tab-menu row, the palette, or any menu over a pane head's `⌄` or a float rail's `Open ⌄` | rest 250 ms | the pane menu opens and the menu under the hand closes | nothing | 8e |
| B3 | a menu or the palette over chrome or a float | re-ask sites, `rearm_hover_intents`, layout peek | chrome hover, flyout or peek armed under the menu | nothing | 8d |
| B4 | a float over a docked page (Windows; §2.5) | hover | the page hears the moves | it does not | 8d |
| | | right / middle press | the page | the float (swallowed, as its body) | 8a |
| | | wheel | the page scrolls | the float's tenant scrolls, or swallowed | 8b |
| | | cursor | the page's | the float's | 8d |
| | | left press | = the float | = | — |
| B5 | a menu, the palette, a toast or the glance card over a page (Windows; §2.5) | hover | the page hears the moves | it does not | 8d |
| | | cursor | the page's | the layer's | 8d |
| | | press | = the layer | = | — |
| | | wheel | = the layer | = | — |
| B6 | a preview or page float's head, foot, rail or grip over a pane, column, strip or rail | wheel | whatever is under it scrolls | swallowed | 8b |
| B7 | a preview float's body over a files column, the tab strip or the rail | wheel | the thing under it scrolls | the float's document | 8b |
| B8 | a top float's head over a lower float's body | wheel | the lower float scrolls | swallowed | 8b |
| B9 | a docked control under a float or the glance card | tip | the hidden control's tip | the covering layer's own, or none | 8c |
| B10 | a float's rail control over a docked rail control | tip | the docked control's tip | the float's | 8c |
| B11 | the command rail under a float or the glance card | hover (fan-out) | the hidden rail fans out | nothing | 8d |
| | | cursor | finger | the covering layer's | 8d |
| | | tip | the tick card | none | 8c |
| B12 | the glance card over a toast | press | the toast | the card | 8a |
| | | hover | the toast holds; the card's grace and foot are never asked | the card | 8d |
| B13 | a full-window card arriving while a menu is open | press on the menu | the card takes it | the menu is gone (the card closes popups at raise) | 8a |
| B14 | a float's Reload / Keep pill | press | the body ladder (caret) | the pill's verb | 8a |
| B15 | a head control, the ghost, the text-size pill, the preview rail, a files root or any other docked rung during a ~200 ms open/close | press at the solved spot (not drawn) | the control answers | nothing there | 9 |
| | | press at the drawn spot | falls through to the terminal at a solved cell | the control | 9 |
| B16 | an open icon rail or focus column over panes | press | layout and keyboard focus move to the covered pane | the rail's row only | 8a |
| B17 | the ghost `⌄`/folder, the text-size pill or an open rail over printed text | glance arming | arms for the path under it | nothing | 8d |
| | | link underline | underlines under it | nothing | 8d |
| B18 | the border shared by two preview surfaces | hover / press / wheel | both claim it | the upper one (half-open edges) | 9 |
| B19 | a touch pan (`GID_PAN`) while any capture is held | the parked pan is spent | its opening point is a `pointer_moved` that drives the held owner to the finger (`mouse.rs:5645-5658`), then its travel scrolls | discarded while a capture is held (§7) | 6 |
| B20 | a press of the held capture's own button (its release was lost) — in the capturing window or another | the press | the stale latch stays: it goes on driving the bare pointer and eats the next release ahead of lower rungs (G-1); the new press runs beside it | the stale capture is cancelled, then the press is routed (R-5 rule 2) | 3 |
| B21 | a press of another button while a capture is held, arriving in **another** window (reachable where the platform delivers it there: macOS, or a Windows capture the system already took away) | the press and its release | routed in that window as an ordinary press (menus, a forwarded press, a page) | swallowed, press and release (R-5 rule 3, R-8; pending Q1) | 4 |
| B22 | the chord *left held in window A → right down in window B (swallowed) → left up in A (capture ends) → right up in B*, with the slot already empty | the right release | (today the right press itself was routed in B, so its release is too) — under cut 4 without memory it would reach B's ordinary road with no press behind it | swallowed: `swallowed` still holds the right button until this, its matching release (R-5 rule 3) | 4 |

Count: **31 changed, 4 unchanged.** (The first revision printed 28 for a table of 27 changed cells; the sum was wrong, not a cell.)

### 3.3 Table C — kept as they are, not covered by A or B (23 cells)

Each must stay green through every cut (rewritten where §4.3 says its *text* has to move, never weakened in what it asserts). Cells with **no named pin today** get one in the cut named.

| # | cell | pinned by |
|---|---|---|
| U1 | a release after the pointer left the window is routed at the last-seen point | `app_mouse_tests.rs:1434` |
| U2 | an in-pane surface: left is its verb, every other button swallowed | `app_mouse_tests.rs:3609` |
| U3 | an in-pane surface swallows a notch; a `Saved` pill claims nothing | `app_mouse_tests.rs:3609`; `notice::tests::a_strip_claims_its_frame_unless_it_has_nothing_to_press` |
| U4 | any press takes the tip down | `mouse.rs:4437-4445` — no named pin; cut 8a adds one |
| U5 | a press outside the capsule hands the caret back | `mouse.rs:4463-4471` — no named pin; cut 8a adds one |
| U6 | a press outside a menu closes it and goes on; a press on its own trigger is spent closing it | `main.rs:53825` `a_press_on_a_popovers_own_trigger_is_spent_closing_it` |
| U7 | the `⌄` peek / pin grammar | `app_mouse_tests.rs:4047`, `4129` |
| U8 | the palette's list scrolls and its field swallows | `app_mouse_tests.rs:3874` |
| U9 | a full-window card swallows every press, its scrim included | `seats.rs:27347` `a_modal_swallows_the_divider_the_seat_and_the_terminal_under_it` (seat level); the arms `mouse.rs:4487-4617` |
| U10 | a toast takes its press before the modal family | `mouse.rs:4571-4592` — no named pin; cut 8a adds one |
| U11 | the wheel's stations, in order, where nothing overlaps and nothing is held | `app_mouse_tests.rs:161-1467`, `3275` |
| U12 | `Ctrl`/`⌘` + wheel over a terminal steps its text size; over a page zooms it | `text_size_tests.rs:1117`; the page half `web.rs:1960-1991` — no named pin; cut 8b adds one |
| U13 | a right press on a tab raises its menu and leaves the active tab alone | `app_mouse_tests.rs:1515` |
| U14 | a middle press on a tab closes it | `mouse.rs:3205-3214` — no named pin; cut 8a adds one |
| U15 | `Shift` takes a tracked press back; only the left button can be taken from a tracking program | `app_mouse_tests.rs:2484-2557` |
| U16 | Control+click is the secondary click on a Mac | `main.rs:58940` |
| U17 | a press on the title-bar handle is the platform's | `seats.rs:27716` `the_drag_boundary_follows_the_tabs_off_the_title_bar` |
| U18 | one cursor shape for the whole drag; the grip's arrow | `app_mouse_tests.rs:2681`, `2849`; `app_floats_tests.rs:337` |
| U19 | a link opens only if the hand held still | `app_mouse_tests.rs:3310` |
| U20 | a float is opaque to the hover and the press of docked rows | `app_mouse_tests.rs:3398-3472` |
| U21 | one hover panel on the glass at a time | `app_mouse_tests.rs:4271` |
| U22 | a press inside a pane moves the focus there (outside B16) | `seats.rs:23418` `a_press_anywhere_in_a_pane_head_included_moves_focus_to_it` |
| U23 | macOS: a press that reaches Folio's surface view over a page is consumed, focuses the pane or raises its float, hands the page the keyboard, forwards nothing and latches no capture (§2.5) | `mouse.rs:5270`, `web.rs:1852-1929` — no named pin today; cut 8a adds `a_page_press_that_reaches_the_surface_view_is_consumed_and_focuses` |

**Totals, from the tables: 274 decided cells — 153 changed (A 122, B 31), 121 unchanged (A 94, B 4, C 23) — and 4 not applicable.** Of §3's 153, 13 are Windows-only (§2.5).

---

## 4. Migration

### 4.1 The cuts — ten cuts, fourteen commits

Each commit is green on the guard set every `bt-app` change runs (`hang_watch::`, `cross_window_drag_tests::`, `every_bare_site_is_a_row`, the three `bt-source` tests, clippy `-D warnings`, every `scripts/ci/check-*.ps1`); each carries its own test observed red under the named mutation (CONVENTIONS §三); each flips only the §3 cells marked with its cut. **All legacy routing tests stay as they are until the cut that flips their cells** (§4.3).

1. **The guard, `PointerFacts` and the router — dead-path scaffolding, not an equivalent router.** `PointerFacts` built per frame (§2.1), `POINTER_LAYERS_TOP_FIRST` from the band list `OverlayStack::flattened` folds, `pointer_layer_at` as a pure walk over the facts, the per-event memo. Nothing in the product reads the walk's answer yet; its overlap answers *intentionally differ* from today's first-handler order, which keeps answering every event until cut 8. The guard of §4.2 lands here with its generated debt file and passes on the day it lands (§4.2, bootstrap). So that doors five and six have one owner from the start, cut 1 also moves, without changing behaviour, the dispatcher's five pointer arms behind `runtime::pointer::is_pointer_event`/`Runtime::pointer_event`, the hang-watch classifier's five pointer rows behind `runtime::pointer::station_of`, and the touch door's opening, `ParkedPans` and `spend_parked_pans` into `runtime::pointer::touch` (pinned by `hang_watch::` and the existing pan tests, `a_pan_enters_the_wheel_road_as_pixels`, `a_parked_pan_is_spent_through_the_wheels_own_entrance`). *Tests:*
   - `every_band_is_a_pointer_layer_or_takes_no_pointer` (supersedes `cmdrail_app_tests.rs:27`; mutation: swap two bands in `flattened`).
   - **The worst supported stack is a parameter, not a fixture.** Floats are an uncapped `Vec<FloatWin>` (`float.rs:1276`) and this ticket adds no cap, so there is no maximum to point at; the walk's cost is instead a **formula** in the stack's size, and the tests check the formula at sizes up to well past any real desktop. The stack: every fixed layer up (the glance card, three toasts, the palette, a pane menu with its child list, the profile menu, two download sheets, the capsule over a strip, a sixteen-pane layout with an open icon rail and one pane mid-animation) plus **F** floats of every tenant kind, F ∈ {0, 1, 8, 64, 512}, all of whose frames cover the probe point. The walk visits (a test-only counter on `PointerFacts`): one rectangle per fixed layer it reaches, **one frame test per float down to the first that claims the point**, and then **the parts of that one float only**. So for a probe point on the top float the bound is exact — `visits == L_reached + 1 + parts(top float)` — and for a probe point covered by no float but under all of them it is `L_reached + F`.
   - `the_walk_visits_one_frame_per_float_and_the_parts_of_one` asserts those equalities at every F (mutation: walk every float's parts unconditionally — at F = 8 with the top float claiming, the count becomes `L_reached + 8 + Σ parts(all 8)`, which is larger than the exact bound by the parts of seven floats, so the equality fails; at F = 0 the mutation cannot show, which is why F = 0 is not the only case).
   - `the_walk_allocates_nothing` — a `#[cfg(test)]` counting global allocator whose counter is thread-local, armed around one walk at each F, required to read 0 (mutation: reintroduce the id `Vec` of `floats.rs:2513`).
   - `a_complete_event_walks_once` — the router's own call counter (test-only), reset before and read after **a whole door**: one `pointer_moved` with every hover instrument live (chevrons, page, video, tips, capsule, strip, rail, glance, flyout, layout peek, cursor), one `mouse_input` press and one release, one `queue_wheel` + `flush_wheel`; each must read exactly 1 (mutation: let any one reader call `pointer_layer_at` directly instead of reading the memo — the count becomes 2). Until cut 8 the legacy readers do not read the memo, so this test lands in cut 1 against a door that calls the walk once and is extended, family by family, by each cut-8 commit; in cut 8e it covers every reader.
   - The walk cannot shape text by type (it has no `&mut` renderer), so that needs no test.
2. **The capture record, mirrored and unread — behaviour-neutral by construction.** `PointerCapture`, `CaptureOwner`, and the application slot, written beside the legacy fields by `capture_mirror_begin`/`_end` at every site that sets or clears a legacy latch. Because today's latches *can* overlap (a stale latch beside a new one, a settings drag beside anything, a formula route over a forwarded one), the mirror in this cut is a `Vec<PointerCapture>` that records every overlap rather than resolving it; nothing reads it, so no behaviour changes. *Test:* `the_mirror_names_every_live_legacy_latch` across scripted scenarios, including the three overlaps above (mutation: drop the mirror call from one set site). The test also counts overlaps; cut 3 must bring that count to zero.
3. **The slot becomes the authority; R-6; R-5's rule 2.** The legacy fields go; their payloads move into the variants (leaf-keyed: C3·g's identity half, C11–C17·g's stranding); the `Vec` becomes `Option`. **The press precedence of R-5 is one pure function**, `press_against_the_slot(slot: Option<&PointerCapture>, button) -> PressVerdict { Route, CancelThenRoute, Swallow }` (the house's shape for such rules: `release_verdict`, `press_after_blur`), and the only door that consults the slot on a press. In this cut it answers `Route` for an empty slot and `CancelThenRoute` for a press of the slot's own button (B20); a press of another button keeps today's routing until cut 4. One `release_capture(button, position)` replaces `release_owned_gesture`, the ladder's latched half (`mouse.rs:3225-3366`), the float carry's arm (5012), the glance releases (4906-4913) and the settings release (`main.rs:47462`). *Tests:* `a_release_reaches_its_owner_over_every_layer_that_eats_releases` — table-driven over `CaptureOwner` × {each card, settings, each menu, palette row, glance head, page} (mutation: put the page arm back above the release); `a_press_of_the_held_button_cancels_the_stale_capture_first` — B20, for every owner, with the press arriving in the capturing window and in another window of the same `App` (mutation: route without cancelling — the stale owner then still holds the slot and the new press latches nothing). Flips A·a, B20.
4. **R-8 — pending the owner's Q1.** `press_against_the_slot` answers `Swallow` for a press of another button and records it in `App::swallowed`; `release_against_the_slot` (its pure twin for releases) consults `swallowed` before the slot, so a swallowed button's release is swallowed even after the capture has ended. DESIGN entry superseding the 2026-10-04 "non-matching release returns to the ordinary road" (`DESIGN.md:13766`), written only once the owner's approval is recorded. *Tests:* `a_second_button_under_any_capture_reaches_nothing` (each owner; mutation: let the press through); **the two-window case** `a_second_button_in_another_window_is_swallowed_too` — the verdict table driven with the slot held by window A and the press in window B, for both rules (same button → `CancelThenRoute`, other button → `Swallow`), plus an index pin that every window's `mouse_input` reaches the slot only through `press_against_the_slot` and `release_against_the_slot` on `self.app` (mutation: compare the press's window with the slot's and route a press in B); **the ordering case** `a_swallowed_button_stays_swallowed_until_its_own_release` — left captured in A, right down in B, left up in A, right up in B: the right release is swallowed with the slot already empty, and a later right press in either window is routed (mutation: clear `swallowed` when the capture ends — the right release then reaches B's ordinary road). Flips A·b, B21, B22.
5. **R-9 — the endings, with a lifecycle-door matrix of exact items.** Every door below calls `end_capture(Ending::…)` for the owners it can end, **before** it changes the state the owner names. Each door is one named item, so the index can pin it; three are new items the cut extracts from places that are not items today.

   | door (exact item) | file:line | ending |
   |---|---|---|
   | `Runtime::window_blurred` — **new**, extracted from the `WindowEvent::Focused(false)` arm | `main.rs:67427-67505` | cancel |
   | `Runtime::flush_pending_pty_resize`, through `Runtime::end_a_capture_that_lost_its_pointer` (the generalised `end_a_divider_drag_that_lost_its_pointer`, `runtime/panes.rs:3922`) | `runtime/dpi.rs:173` (the call at 183) | cancel |
   | `Runtime::scale_factor_changed` (first statement) | `runtime/dpi.rs:424` | cancel (pixel-quantity owners, page) |
   | `Runtime::hide_quake_window` | `runtime/quake.rs:107` | cancel |
   | `Runtime::close_window` (the close itself, not the request) | `runtime/windows.rs:1998` | cancel |
   | `Runtime::activate_tab` (before `active_tab` is written) | `runtime/tabs.rs:160` | off the glass |
   | `Runtime::close_tab` | `runtime/tabs.rs:251` | owner gone |
   | `Runtime::close_pane` | `runtime/panes.rs:1396` | owner gone |
   | `Runtime::toggle_pane_zoom` | `runtime/panes.rs:2911` | off the glass |
   | `Runtime::sweep_preview_panes` | `runtime/preview.rs:2233` | owner gone |
   | `Runtime::leave_preview_buffer_in` | `runtime/preview.rs:3062` | owner gone |
   | `Runtime::rebuild_preview_document` | `runtime/preview.rs:8142` | owner gone |
   | `Runtime::dismiss_float` | `runtime/floats.rs:3472` | owner gone |
   | `Runtime::hide_file_peek` | `runtime/peek.rs:915` | owner gone |
   | `Runtime::settings_closed` — **new**, called by `Runtime::toggle_settings_panel` (`main.rs:46102`) when it closes the sheet and by `Runtime::keyboard_input`'s `SettingsKeyVerdict::Closed` arm (`runtime/keyboard.rs:1308`) | new item | owner gone |
   | `Runtime::drag_out_to_the_system` — **new with the feature** (§7), the only caller of the OS drag loop | new item | `OsDragHandoff` |
   | `WebSeat::place` and `WebSeat::park_for_handoff` (cut 7) | `webhost.rs:3570`, `2457` | the seat's own button-up |

   *Enforcement, two halves:* (i) `every_lifecycle_door_ends_the_capture` — a `bt-source` index pin, one row per item above, that the item's body calls `end_capture` (the `WebSeat` rows: that it calls the seat's release); plus, for the two doors whose closing is reached from more than one place, that the closers reach them: every caller of `SettingsPanel::close`/`toggle` (`settings.rs:7555`) is `settings_closed` or a caller of it, and the `Focused(false)` arm's only statement of substance is the `window_blurred` call. A door removed from the matrix, or the call removed from a door, names the door (mutation: remove the call from `hide_quake_window`). (ii) `each_door_ends_a_held_capture` — behavioural, one scenario per row with a capture of every owner the door can end held (mutation: an exhaustive-match arm that does nothing). The exhaustive match on `CaptureOwner` proves every variant has an answer; the two halves prove every door asks. `a_divider_drag_that_loses_its_pointer_stops_holding_the_resize` is kept and generalised. Flips A·c (C22), A·d, A·e, A·f, A·g.
6. **R-7 and R-10's capture half.** `observe_chevrons`, the tip, the flyout and glance intents and non-owner pages' hover ask the slot; the rail zone and the survey do not; a page under a non-page capture answers no notch; parked pans are discarded while held. *Tests:* `a_held_gesture_resting_on_a_chevron_opens_nothing`; `no_page_but_the_owner_hears_the_wheel_under_a_capture`; `a_pan_under_a_held_gesture_moves_nothing` (mutations: drop each gate). Flips A·h, A·w (except C21), B19.
7. **R-13 (Windows).** `CaptureOwner::Page`, `WebSeat::last_point`, the seat-level "never hidden with a button down" rule in `WebSeat::place` and `WebSeat::park_for_handoff`, and the scale-change ordering. **Presence-order coverage on a real `WebSeat`:** `WebSeat::send_mouse`'s one call into the platform host (`webhost.rs:4088`) goes through a `PageMouse` sink the seat owns — the platform host in the product, a recorder in tests — so the test drives the **real** seat through its real presence transition (`web_presence` → `WebSeat::place`, the path `sync_web_page` takes) and asserts the recorded order: `LeftDown`, moves outside the bounds, then on `Ctrl+Tab` a `LeftUp` at `last_point` **while the seat is still `Shown`**, then the hide. *Tests:* `a_page_selection_dragged_out_of_the_page_keeps_the_page`; `a_page_seat_never_hides_with_a_button_down` (mutation: hide before the release, which the recorder shows as a release with no `Shown` bounds, i.e. none sent); `a_parked_page_seat_releases_before_it_is_handed_over` (the `park_for_handoff` door; same mutation); `a_scale_change_releases_the_page_in_the_old_space` (mutation: move the cancel after `resize`). `cfg(windows)`. Flips row C21.
8. **Every reader onto the router**, five commits so each flips one family: **8a** the press road (`mouse_input`'s arms become one dispatch on `PointerHit`; a full-window card closes popups at raise; the float's pill; `focus_pane_at` asks the router; **the `Page` answer keeps today's consuming, focusing door on every platform and latches a capture only where `page_press_latches_a_capture` says so — U23's pin `a_page_press_that_reaches_the_surface_view_is_consumed_and_focuses`, driven with `HostPlatform::MacOs`, and run in the Mac gate**; pins for U4, U5, U10, U14) — B1, B4 (press), B12 (press), B13, B14, B16; **8b** the wheel (stations keyed by layer; floats own their frame; pin for U12's page half) — B4 (wheel), B6–B8; **8c** tips — B9, B10, B11 (tip); **8d** hover, cursor, every paint-time hover read and the page's hover (`web_page_at` becomes the router's `Page` answer; the command rail, `row_under`, `pane_hit_context`, the re-ask sites read the router) — B3, B4, B5, B11, B12 (hover), B17; **8e** the `⌄` clocks — B2. Each commit's test is the B rows it flips, driven at the points named (mutation: restore the old reader), and each extends `a_complete_event_walks_once` to the readers it moved. The legacy routing tests listed in §4.3 are rewritten in the commit that flips their cells, not before.
9. **R-3: motion-aware hit tests.** The five census-#21 rungs and the seven others take `pane_transforms` and the card rectangle through `pane_chrome_box`, as `hit_chrome_in_motion` does; `preview_surface_at` becomes half-open. *Test:* `a_control_is_hit_where_it_is_drawn_while_its_pane_moves` (per rung, mid-animation, at the solved and the drawn spot; mutation: drop the transform). Flips B15, B18.
10. **The rules and the debt at zero.** RULES row 28 gains §2.6's text; ARCHITECTURE §8's "the rung order is the de-facto specification and nobody wrote it down" is answered for the pointer by a pointer to `POINTER_LAYERS_TOP_FIRST`; the debt file and its check are deleted together (§4.2). One DESIGN entry per commit, as every merged change does.

### 4.2 The planted-violation guard

`every_pointer_read_is_the_routers_or_a_captures`, a `bt-app` test on the `bt-source` index (the mechanism `the_press_and_the_hover_ask_one_router`, `app_mouse_tests.rs:3503`, and `hardcode_owners.rs` already use: it asks the index about identifiers, types and calls, never reads source text as a behaviour oracle).

- **Why the needles are the entrances, not the readers.** The second review showed that no lexical rule finds every function that tests a point (`cmdrail::nearest`, `StripRun::aim`, `preview_live::press` take bare `x`/`y`). The guard therefore does not try. Pointer data has exactly six ways into Folio, and the guard closes all six, **over every item of `bt-app`** (not only `crate::runtime`):
  - **N-P** — the fields `pointer_position` and `pointer_last_seen` (where every `CursorMoved` is kept);
  - **N-E** — the type `PhysicalPosition` anywhere in an item: a parameter, a field, a local, a return, a constructor (where a pointer position is carried: `Drag::pointer`, `DragLatch::origin`, `drive_*`'s arguments; `bt-app` uses the type for nothing else at base);
  - **N-B** — the types `MouseButton`, `ElementState`, `MouseScrollDelta` (a button or wheel event; the wire encoders — `input::mouse_bytes`, `protocol_mouse_button`, `web_mouse_button`, `WheelBurst` — are owners like any other and move into `runtime::pointer::wire` in the cut that touches them, debt rows until then);
  - **N-S** — calls of the system cursor reads `bt_platform::pointer_position` (`bt-platform/src/lib.rs:13628`) and `bt_platform::pointer_position_in_window` (the drop point, `mouse.rs:5763`; the summoned window's monitor, `quake.rs:237`).
  - **N-W (door five)** — the pointer variants of `winit::event::WindowEvent`: `CursorMoved`, `CursorLeft`, `CursorEntered`, `MouseInput`, `MouseWheel`. A `match` arm can destructure them with **inferred** field types — the dispatcher does exactly that today (`WindowEvent::CursorMoved { position, .. } => runtime.pointer_moved(position)`, `main.rs:67381-67384`) — so neither N-E nor N-B sees a new destructuring owner. The needle is the variant path itself, whether it binds a field or matches `{ .. }`, so the station classifier that names them (`main.rs:67747-67750`, `WindowEvent::CursorMoved { .. } => Station::EventPointer`) is an owner too. Cut 1 gives both one door inside the module: the dispatcher's five arms become one arm guarded by `runtime::pointer::is_pointer_event(&event)` that calls `runtime.pointer_event(event)`, and the classifier's five rows become `runtime::pointer::station_of(&event)`; `FolioApp::window_event` then names no pointer variant.
  - **N-T (door six)** — the translated-touch road. `bt_platform::PanStep` carries a client point (`began_at: Option<(i32, i32)>`, `bt-platform/src/touch_pan.rs:28-40`) into `bt-app` through the closure `let_the_system_translate_touch` builds (`main.rs:72625-72650`), which parks each step in `ParkedPans` for `Runtime::spend_parked_pans` (`mouse.rs:5645`) and `pan_on_the_wheel_road` (`main.rs:72684`). The needles are the type name `PanStep` and calls of `bt_platform::let_the_system_translate_touch`; the door's opening, the parking slot's type and the spending move into `runtime::pointer::touch`.

  **The index queries**, one `bt_source::Search` each, run over `Index::of_package("bt-app")` in `View::Identifiers`, kept to the product with `.in_the_product(index)` and reported by `Found::owners` — the shape `hardcode_owners.rs` already uses:

  | door | needle |
  |---|---|
  | N-P | `Pattern::identifier("pointer_position")`, `Pattern::identifier("pointer_last_seen")` |
  | N-E | `Pattern::identifier("PhysicalPosition")` — every occurrence, constructors (`PhysicalPosition::new`, 13 product sites today) included. `bt-app` has no `PhysicalPosition<i32>` at base (`grep` finds none; window positions travel as other types), so every occurrence is a pointer position; should a window position ever use the type, it is one more owner the generator lists and the reviewer sees |
  | N-B | `Pattern::identifier("MouseButton")`, `Pattern::identifier("ElementState")`, `Pattern::identifier("MouseScrollDelta")` |
  | N-S | `Pattern::call("pointer_position")`, `Pattern::call("pointer_position_in_window")` |
  | N-W | `Pattern::path("WindowEvent::CursorMoved")`, `…::CursorLeft`, `…::CursorEntered`, `…::MouseInput`, `…::MouseWheel` |
  | N-T | `Pattern::identifier("PanStep")`, `Pattern::call("let_the_system_translate_touch")` |

  Not doors, and why: the drag broker's `DragBroker::pointer: (f64, f64)` and the target window's survey are plain numbers handed on from a capture owner's own event (an admitted blind spot of the "plain numbers" kind below; the census records `drive_drag_broker` and `foreign_strip_landing` as R rows); the IME caret area is outbound geometry; WebView2 composition hosting delivers no mouse callback to Folio (`SendMouseInput` is outbound), and macOS's `WKWebView` receives AppKit's events without feeding them back through `bt-app`.

  A function outside the router can then only ever see coordinates the router handed it as plain numbers — `cmdrail::nearest` is called **by** the router with numbers the router chose, and that call is inside `runtime::pointer`. The census (§1.0, classes H and C) is the map of which geometry functions the router will call; it is not a needle list, so it does not have to be complete for the guard to be sound.
- **The rule, with no allowlist to grow.** Every product owner of an N-P, N-E, N-B, N-S, N-W or N-T occurrence (`Found::owners`) is an item of the module `crate::runtime::pointer` (the router, `PointerFacts`, the capture slot, the four event doors, the owners' drive and release functions, `wire`) **or** a row of the debt file. There is no list of allowed names: adding a reader means writing it inside `runtime::pointer`, which a reviewer sees as such. After cut 1 the dispatcher hands each pointer `WindowEvent` to `Runtime::pointer_event` through `is_pointer_event` and names no pointer variant or needle type, so `FolioApp::window_event` is not an owner (N-W).
- **The debt file and its day-one bootstrap.** `docs/plans/POINTER-DEBT.tsv` is written by `scripts/dev/generate-pointer-debt.ps1` from the **same index query the test runs**: **one row per occurrence**, `owner<TAB>needle` (for example `crate::runtime::tabs::Runtime::scroll_tab_strip<TAB>N-P`), the row repeated once for every occurrence of that needle in that owner, sorted. **Why one row per occurrence and not a count column:** the gate `check-migration-debt.ps1` compares whole row strings with multiplicity (`$allowed[$row]`, lines 87-97), so a count column would turn "3 → 2" into one removed row plus one added row and be refused; with repeated rows, lowering an owner's occurrences is deleting identical rows, which that gate accepts unchanged, and adding one is a new row, which it refuses. No line numbers or file names are in a row, so moving code without changing what it reads changes no row. Today that is roughly the census's P ∪ E ∪ B rows (238 functions), the structs whose fields hold a `PhysicalPosition<f64>` and the two N-S callers; the generator, not this note, decides the exact list. The test requires the file to **equal** the violation set, so cut 1 passes on the day it lands by construction (the file it commits is the query's answer), a reader moved behind the router must leave the file in the same commit, and a new reader fails naming its owner and the file. `scripts/ci/check-pointer-debt.ps1` compares the file with the merge base exactly as `check-migration-debt.ps1` does — whole rows, multiplicity-preserving — **including its bootstrap rule**: a file absent at the merge base is the commit that introduces it and passes (`check-migration-debt.ps1`, the `is not in $base - this is the commit that introduces it` exit). That is cut 1. Every later cut may only remove rows.
- **Cut 10: the file and its check are deleted together.** From then the test's expected violation set is the empty set, a constant in the test, so a reintroduced debt file would change nothing: any new owner fails the test whether or not a file lists it. Removing the check script from `scripts/ci/` is part of the same commit; no gate has to reason about an absent file.
- **What is still gameable, written down.** (1) An item inside `runtime::pointer` that answers a layer question it should not — review is the guard there, and the module is small. (2) The router handing a coordinate out to a function that then decides something the router should have — visible as a call in `runtime::pointer`, i.e. in review. (3) A position written by the router into a field of a non-pointer type (`[f32; 2]`, as today's `TermMenu::pointer_was`) and read later elsewhere — the writer is in the module and the reader is not a needle owner; the census's C class is how the cut-8 commits find such fields.
- **Its own red:** add a read of `pointer_position` to `scroll_tab_strip` after its row has left the debt file.

### 4.3 Tests that pin today's behaviour and must change

- **Change in what they assert** (their cells flip): `app_mouse_tests.rs:3689` `a_forwarded_gesture_is_delivered_to_the_shell_it_was_handed_to` (the tab-not-on-top half: dropped → released from `last`, Q2; cut 5); `main.rs:59425` `every_carry_this_window_can_hold_is_named_by_the_one_predicate` (superseded by the slot, which covers `preview_selecting` and the presses; cut 3); `main.rs:59400` `the_hand_is_asked_before_any_page_is` and `main.rs:59516` `the_tab_list_is_asked_before_any_page_is` (the page becomes a router layer; cut 8d).
- **Change in their text only** (they read source to pin an order the model moves; each rewritten against the new item, asserting the same thing, in the cut that moves the code): `app_mouse_tests.rs:3503` (superseded by §4.2 in cut 1), `3553`, `3609`, `3874`, `3984`, `4216`, `1515`, `1599`, `3398`, `3440`, `3472`; `cmdrail_app_tests.rs:27`; `app_panes_tests.rs:2745`; `main.rs:53792` (`router()` and the tests at 53825-53935), `54560`, `54968`, `56117`, `56171`, `57299`, `57540`, `58940`, `58973`; `app_configuration_tests.rs:82`; `app_peek_tests.rs:872`; `app_tabs_tests.rs:3055`, `3610`.
- **Kept as written**: Table C and the pins under Table A.

The two tests T-POINTER-CAPTURE's ticket also records as flaky under load (`settings::tests::settings_pointer_complete_operation_layout_budget`, the update-lock election) are not pointer tests; the first's shared counter is scoped to its test in its own commit outside this sequence.

### 4.4 The window-waits inventory

No new bare site and no new door. The capture sample is the `GetCapture` read (`bt_platform::thread_mouse_capture`, `bt-platform/src/lib.rs:13580`) the divider already takes every turn, now taken for whichever capture is held (one call per turn, as today, and none when nothing is held). The page's moves and releases go through the door they already use (`WebSeat::send_mouse` → `SendMouseInput`); R-13 sends more of them only while a page holds a capture. Neither `GetCapture`, `pointer_position_in_window` nor `SendMouseInput` is a row of the registry today, and none becomes one. The cursor keeps its one door row (`SetCursor`, `owner_door::set_cursor` from `Runtime::apply_pointer_cursor`). At base `5c4a08b0`, `crates/bt-app/src/window_waits.tsv` has **231 physical lines** and `docs/plans/window-thread-bare-sites.tsv` sums to **231 bare occurrences**; the "233" in ARCHITECTURE §4.4's T-FRESH-FACTS row (`ARCHITECTURE.md:760`) is stale prose, not the registry. Neither file changes; `scripts/ci/check-window-waits.ps1` is unchanged.

---

## 5. Hazards

**Latency on the window thread.** Today one `CursorMoved` evaluates `pointer_target_at` three to seven times (G-22), each, with a float up, measuring two captions (`floats.rs:2495-2507`), allocating an id `Vec` (2513) and cloning the Git page and graph a float shows (2536, 2552).
- **What the design guarantees mechanically:** one walk per complete event, pinned at the door (`a_complete_event_walks_once`); no shaping (by type); no allocation (cut 1's counting-allocator test, at every stack size); and a visit count that is an **exact** function of the stack — one rectangle per fixed layer reached, one frame per float down to the claimant, the parts of the claimant only — checked at F ∈ {0, 1, 8, 64, 512} floats (cut 1). Floats are uncapped (`float.rs:1276`) and this ticket adds no cap, so the walk is linear in the floats over the point and constant in everything else; the 512-float case is there to show the per-float cost is one frame test. These are deterministic tests, not timing tests (CONVENTIONS §三 forbids timing-bound tests).
- **The absolute budget, measured in cut 1's report and held in every later cut's report:** at the parameterised stack with F = 8 (a heavy real desktop), in a release build on the build desktop, **one walk ≤ 20 µs at p99 and ≤ 50 µs at the maximum** over a 10 000-event sweep, and **the whole `pointer_moved` with nothing held ≤ 100 µs at p99**; at F = 512 the report states the measured slope per float (expected: one frame test, well under a microsecond). The number comes from the worst device, not from today's mean: an 8 kHz mouse delivers at most one report every 125 µs, and the window thread must leave room for the frame it is also drawing. The sweep is the router tests' `pointer_moved` seam (no window, no injected input), timed by the hang-watch station totals (`Station::EventPointer`, `hang_watch.rs:701`) under `BT_MOUSE_TRACE`.
- **Coalescing policy: Folio adds none and processes every delivered event.** Windows does not queue one `WM_MOUSEMOVE` per hardware report: it synthesises a single pending move when the queue is read, so delivery is bounded by how often the loop reads; AppKit coalesces mouse-moved events by default. The budget above holds even if a platform delivered every report. The wheel keeps its own merge (`WheelBurst`, `mouse.rs:5578-5631`).
- **Not moved:** the drag survey (`survey_drop`, one strip solve per move, `mouse.rs:2822-2826`) is the drag owner's and costs what it costs today.

**The hosted page's press handler (Windows).** The page is composition-hosted and hears only what Folio forwards; winit holds the Win32 capture from button-down to button-up on the top-level window (`mouse.rs:1484-1492`). A divider released over a page already arrives in `mouse_input`; what loses some releases is the order of the arms (K1, G-1). **Mechanism: the capture slot (R-6) delivers the release before any layer, the page included.** Rejected: a transparent capture overlay (a second source of truth and one more visual DirectComposition must order); a second `SetCapture` (winit already holds the capture for exactly the gesture's life, and a second holder would compete with it for the button-up); the engine's own mouse events (the composition controller reports none to its host). The ordering and DPI rules for a page's own release are §2.4's.

**DPI and scale.** Pointer positions and every layer's geometry are the window's physical pixels; `PointerFacts` is rebuilt on the frame after a scale change. A capture's payloads that are pixel quantities taken at the press are cancelled on `ScaleFactorChanged` (R-9); a forwarded press's `last` is a cell; a page's `last` is page-local and the page is released in the old space before the change is applied (§2.4). `DragGuard` already ends a cross-window drag on a virtual-screen change (`main.rs:33941-33962`).

**Multi-window drags (tab and pane tear-out).** The slot is the application's, so "one capture" holds across windows by representation (R-5). The cross-window half stays `DragBroker` (`main.rs:34131`, `FolioApp::drive_drag_broker`, `main.rs:65090-65130`): target windows receive no pointer events while the button is down and are fed by the broker's clock; the release keeps going through `hand_over_across_windows` (`mouse.rs:3060`). Today a release eaten above the ladder is cancelled home by the broker's guard a turn later (C5·a); after cut 3 it lands where it was released — cut 3's test covers a release over a card in the source window while the pointer is over another window's glass.

**Tab switch under a divider or a forwarded press.** Both endings read state the switch replaces (`cancel_divider_drag` reads the active tab's seats, `panes.rs:3936-3940`; a forwarded release needs the owner's seat), so the tab-switch door runs `end_capture` before `active_tab` changes (§4.1 cut 5's matrix).

**macOS twin.** The same router and slot; the seams are the platform's: (1) *OS capture* — AppKit sends `mouseDragged:` and `mouseUp:` to the view that took `mouseDown:`, `thread_mouse_capture` answers `None` at the press and after (`main.rs:19640-19644`; `bt-platform/src/portable_impl.rs:995-997`), so the capture sample never fires and blur (the window resigning key) and the window hidden are the cancels; (2) *the hosted page* — out of this ticket, with the consequence table of §2.5; (3) *secondary click* — Control+click becomes `Right` before every router (`mouse.rs:4419-4431`), and the slot records the translated button, so R-6 and R-8 compare like with like; (4) *the title-bar handle* — a press on it is the platform's (`press_owned_title_bar`, `mouse.rs:3633`), latches no capture, and on Windows never arrives (`HTCAPTION`).

---

## 6. Open questions for the owner

The review (Codex, 2026-10-09) agrees with all three recommendations; its additions are adopted below.

**Q1. A second button while one is held: refuse it, or let it act?** — **status: pending the owner** (queued 2026-10-09). The answer is recorded here and in a dated DESIGN entry before cut 4 is built; cuts 1–3 do not depend on it, and if the owner chooses the alternative, cut 4 and cells A·b and B21 are rewritten to it before building.
The rule holds in whichever window the second press arrives (R-5 rule 3); a press of the *held* button is not a second button — it proves the capture stale (rule 2, B20), which needs no ruling.
Today a right press during a held left gesture goes down the whole press road and can raise a tab, file-row, pane, git, page or terminal menu, or replace a forwarded press with the formula block's (G-5); the menu then eats the left release and leaves the gesture on the bare pointer (G-1).
*Recommendation: refuse (R-8).* The other button's press and release go nowhere while a gesture is held. This **supersedes** the 2026-10-04 rule that a non-matching release under a forwarded press goes back to the ordinary road (`DESIGN.md:13766`), so it needs the owner's yes and lands with a dated DESIGN entry saying it supersedes that sentence. *Alternative:* the second button cancels the held gesture and then acts — a divider would jump back on a right click.

**Q2. A program's mouse press whose tab is switched away while the button is held: tell the program, or drop the gesture?**
Today the route is dropped and nothing is sent (`tabs.rs:195`; `clipboard.rs:523-528`), so the program stays logically pressed.
*Recommendation: tell it (R-9, off the glass)* — at the last cell it was told about, in the press's encoding, **with the modifiers of the last mouse report sent** (not the `Ctrl` of the `Ctrl+Tab` that caused the ending), and **before the tab's seats change**. A pane closed or a shell restarted is still owed nothing, as ruled on 2026-10-04. *Alternative:* keep the drop.

**Q3. A drag that starts inside a hosted page and leaves its rectangle: does the page keep it?**
Today it does not (G-2), and the page's button stays down.
*Recommendation: yes, on Windows (R-13)*, as a view-level capture that leaves the engine's own element capture to the engine. On macOS the answer waits on §13.29 (§2.5). *Alternative:* clamp the moves into the page's bounds, or keep today's edge and only send the page its button-up.

Not asked, because already ruled or not in reach: the capsule and the strip stay where they are and the capsule is above the strip (owner, 2026-10-04); the `⌄` peek/pin grammar (2026-09-23) is unchanged — the model only stops its rest clock under a held gesture and under a menu; a divider ignores minima (2026-08-10).

---

## 7. Boundaries

- **Touch and pen.** Folio consumes no raw `WindowEvent::Touch`: Windows' own translation sees the touch messages (`bt-platform/src/lib.rs:10265-10290`), taps and presses arrive promoted to the mouse and are covered by this model as mouse input, and a one-finger pan is answered as the wheel — `GID_PAN` parked, then spent by `spend_parked_pans` as a `pointer_moved` to the pan's point plus a `queue_wheel` (`mouse.rs:5645-5658`). **Under a held capture a parked pan is discarded** (B19): its pointer move would otherwise drag the held owner to the finger, and a second pointer is not something this model has. Pen pressure, the eraser and true multi-pointer capture are out of scope.
- **Accessibility activation.** Folio's own chrome exposes no UI Automation activation road in this code; a page's accessibility is the engine's. A pointer-less activation (keyboard, a command, a future UIA invoke) **never manufactures a `PointerHit` or a capture**: it goes through the keyboard/command doors, and the router is not asked. An invoke that arrives while a capture is held does not end it.
- **The summoned window.** It is a second top-level window on the same thread, topmost, re-placed on the monitor under the pointer at each summon (`quake.rs:237`, `runtime/quake.rs:33`). Routing is window-local physical pixels, so its geometry needs nothing new. The application slot (R-5) is what keeps a capture taken in one of the two windows from being doubled by the other, and hiding it is a lifecycle door (`hide_quake_window`, `runtime/quake.rs:107`, §4.1 cut 5) that cancels a capture it holds. A summon by hotkey while the main window holds a capture blurs the main window, which cancels it.
- **8 kHz mice.** The absolute and tail budget and the parameterised stack are §5's; the zero-allocation, exact-visit and one-walk-per-event tests are cut 1's. Folio adds no coalescing of its own.
- **Files dragged out to the system (OLE drag-out).** There is no `DoDragDrop`/`IDropSource` today; only inbound `DroppedFile` is wired (`main.rs:67406`). When a drag-out lands, `DoDragDrop` runs a modal loop that takes the pointer and the capture away synchronously, so the per-turn capture sample would arrive too late. The model reserves **`OsDragHandoff`, an ending of its own and not a cancel** (R-9). The difference is the whole point: a cancel sends a drag home (`settle_home`), puts a divider's ratio back, and drops a release's verb; a hand-off does **none** of that, because the gesture did not fail — its payload was given to the system, and the system's drop decides what happens to it. So the one door that may enter the OS loop, `Runtime::drag_out_to_the_system` (new with the feature, §4.1 cut 5's matrix), first takes the drag's capture with `end_capture(Ending::OsDragHandoff)` — which clears the slot and the broker and runs no settle, no landing and no release — and only then enters the loop. Only a drag owner answers it (C5, or C4 once its press has become a drag); every other owner's arm is empty. Its tests land with the feature: `a_drag_out_hands_the_capture_over_before_the_os_loop` and `a_handed_over_drag_does_not_go_home` (mutation: route the hand-off through the cancel arm — the tab or pane then slides home under the system's drag).

---

## Appendix A — the census programs and the command that builds the committed file

Run from the repository root with GNU awk 5, the four programs below saved beside you. The last command writes `pointer-capture-2026-10-09.readers.tsv`: its six header lines, then the three outputs merged and sorted by file and line. Rerun at base `5c4a08b0`, the body reproduces the committed file byte for byte.

```sh
files=$(git ls-files 'crates/bt-app/src/*.rs' 'crates/bt-app/src/**/*.rs' | grep -v -E '(_tests|tests|test_support)\.rs$' | sort -u)
awk -f sweep.awk $files > sweep.tsv                                     # P, E, B, H
awk -F'\t' 'length($3) >= 10 { print $3 }' sweep.tsv | sort -u > names.txt
awk -v NAMES=names.txt -f calls.awk $files > calls.tsv                  # callers of a reader
awk -F'\t' 'NR==FNR { k[$1 "\t" $3] = 1; next } !(($1 "\t" $3) in k)' sweep.tsv calls.tsv > r_only.tsv
awk -f edges.awk $files > edges.tsv                                     # functions, coordinate flags, call edges
awk -F'\t' -f closure.awk sweep.tsv edges.tsv > cclass.tsv              # C
awk -F'\t' 'NR==FNR { c[$1 "\t" $3] = 1; next } !(($1 "\t" $3) in c)' cclass.tsv r_only.tsv > r_only2.tsv   # R, less C
out=docs/plans/design/pointer-capture-2026-10-09.readers.tsv
head -6 "$out" > header.txt                                            # the six header lines, kept as written
{ cat header.txt; cat sweep.tsv cclass.tsv r_only2.tsv | LC_ALL=C sort -t"$(printf '\t')" -k1,1 -k2,2n; } > "$out"
```

`sweep.awk` (classes P, E, B, H):

```awk
# Pointer-reader sweep over one product .rs file.
# Prints: file<TAB>line<TAB>fn<TAB>classes   (one row per function, classes = union of P/E/B/H)
# P: reads pointer_position / pointer_last_seen
# E: takes or holds a PhysicalPosition<f64> (a window pointer position)
# B: names MouseButton / ElementState / MouseScrollDelta (a button or wheel event)
# H: a geometry hit test: name matches hit|_at|contains|under and the signature takes an x/y pair or a point
# #[cfg(test)] items are skipped by brace depth.
function flush() {
  if (fn != "" && cls != "") {
    out = ""
    if (cls ~ /P/) out = out "P"
    if (cls ~ /E/) out = out "E"
    if (cls ~ /B/) out = out "B"
    if (cls ~ /H/) out = out "H"
    printf "%s\t%d\t%s\t%s\n", FILENAME, fnline, fn, out
  }
  cls = ""
}
BEGIN { skip = 0; depth = 0; pending = 0; fn = ""; cls = ""; insig = 0 }
{
  line = $0
  gsub(/"([^"\\]|\\.)*"/, "\"\"", line); gsub(/'(\\.|[^'\\])'/, "'c'", line); sub(/\/\/.*$/, "", line)
  if (skip) {
    n = gsub(/\{/, "{", line); m = gsub(/\}/, "}", line)
    depth += n - m
    if (depth <= 0) { skip = 0; depth = 0 }
    next
  }
  if (line ~ /^[ \t]*#\[cfg\(test\)\]/) { pending = 1; next }
  if (pending) {
    if (line ~ /^[ \t]*#\[/) next
    pending = 0
    if (line ~ /\{/) {
      n = gsub(/\{/, "{", line); m = gsub(/\}/, "}", line)
      depth = n - m
      if (depth > 0) { skip = 1 }
      next
    }
    if (line ~ /;[ \t]*$/) next
    # multi-line signature: skip until the opening brace
    skip = 1; depth = 0
    next
  }
  if (match(line, /fn [a-z_0-9]+[<(]/)) {
    flush()
    fn = substr(line, RSTART + 3, RLENGTH - 4)
    fnline = FNR
    sig = line
    insig = (line !~ /\{/ && line !~ /;[ \t]*$/)
    name_is_hit = (fn ~ /(^hit|_hit$|_hit_|hit_test|^at$|_at$|contains|covers|_under$|under_|_holds$|_part$|^claim$|_point$)/)
  } else if (insig) {
    sig = sig " " line
    if (line ~ /\{/ || line ~ /;[ \t]*$/) insig = 0
  }
  if (fn != "") {
    if (line ~ /pointer_position|pointer_last_seen/) cls = cls "P"
    if (line ~ /PhysicalPosition<f64>/) cls = cls "E"
    if (line ~ /MouseButton|ElementState|MouseScrollDelta/) cls = cls "B"
    if (name_is_hit && !insig && sig ~ /(\<x: f(32|64).*\<y: f(32|64)|\[f32; 2\]|PhysicalPosition)/) { cls = cls "H"; name_is_hit = 0 }
  }
}
ENDFILE { flush(); fn = ""; skip = 0; depth = 0; pending = 0; insig = 0 }
```

`calls.awk` (class R):

```awk
BEGIN { while ((getline n < NAMES) > 0) want[n]=1; skip=0; depth=0; pending=0; fn="" }
function flush(){ if (fn!="" && hit) printf "%s\t%d\t%s\tR\n", FILENAME, fnline, fn; hit=0 }
{ line=$0
  gsub(/"([^"\\]|\\.)*"/, "\"\"", line); gsub(/'(\\.|[^'\\])'/, "'c'", line); sub(/\/\/.*$/, "", line)
  if (skip) { n=gsub(/\{/,"{",line); m=gsub(/\}/,"}",line); depth+=n-m; if (depth<=0){skip=0;depth=0}; next }
  if (line ~ /^[ \t]*#\[cfg\(test\)\]/) { pending=1; next }
  if (pending) { if (line ~ /^[ \t]*#\[/) next; pending=0
    if (line ~ /\{/) { n=gsub(/\{/,"{",line); m=gsub(/\}/,"}",line); depth=n-m; if(depth>0) skip=1; next }
    if (line ~ /;[ \t]*$/) next; skip=1; depth=0; next }
  if (match(line, /fn [a-z_0-9]+[<(]/)) { flush(); fn=substr(line,RSTART+3,RLENGTH-4); fnline=FNR; next }
  if (fn=="") next
  s=line
  while (match(s, /[a-z_][a-z_0-9]*\(/)) { c=substr(s,RSTART,RLENGTH-1); if ((c in want) && c!=fn) hit=1; s=substr(s,RSTART+RLENGTH) }
}
ENDFILE { flush(); fn=""; skip=0; depth=0; pending=0 }
```

`edges.awk` (every function, whether its signature takes coordinates, and its call-shaped names):

```awk
# Emits, per product function: "F<TAB>file<TAB>line<TAB>fn<TAB>coord" (coord = 1 when the signature
# takes a scalar x/y pair, a [f32; 2], a (f32, f32) / (f64, f64) tuple, or a parameter named point/at/pointer/position)
# and "E<TAB>caller<TAB>callee" for every call-shaped name in its body. #[cfg(test)] items are skipped.
BEGIN { skip=0; depth=0; pending=0; fn=""; insig=0 }
function flush() { if (fn != "") printf "F\t%s\t%d\t%s\t%d\n", FILENAME, fnline, fn, (sig ~ /(\<x: f(32|64)[,)].*\<y: f(32|64)|\[f32; 2\]|\((f32|f64), (f32|f64)\)|\<(point|at|pointer|position): )/) }
{ line=$0
  gsub(/"([^"\\]|\\.)*"/, "\"\"", line); gsub(/'(\\.|[^'\\])'/, "'c'", line); sub(/\/\/.*$/, "", line)
  if (skip) { n=gsub(/\{/,"{",line); m=gsub(/\}/,"}",line); depth+=n-m; if (depth<=0){skip=0;depth=0}; next }
  if (line ~ /^[ \t]*#\[cfg\(test\)\]/) { pending=1; next }
  if (pending) { if (line ~ /^[ \t]*#\[/) next; pending=0
    if (line ~ /\{/) { n=gsub(/\{/,"{",line); m=gsub(/\}/,"}",line); depth=n-m; if(depth>0) skip=1; next }
    if (line ~ /;[ \t]*$/) next; skip=1; depth=0; next }
  if (match(line, /fn [a-z_0-9]+[<(]/)) {
    flush(); fn=substr(line,RSTART+3,RLENGTH-4); fnline=FNR; sig=line
    insig = (line !~ /\{/ && line !~ /;[ \t]*$/)
    rest = substr(line, RSTART+RLENGTH)
  } else if (insig) { sig = sig " " line; if (line ~ /\{/ || line ~ /;[ \t]*$/) insig=0; rest=line }
  else rest=line
  if (fn=="") next
  s=rest
  while (match(s, /[a-z_][a-z_0-9]*(::<[^>]*>)?\(/)) { c=substr(s,RSTART,RLENGTH-1); sub(/::<.*/,"",c); if (c!=fn) printf "E\t%s\t%s\n", fn, c; s=substr(s,RSTART+RLENGTH) }
}
ENDFILE { flush(); fn=""; skip=0; depth=0; pending=0; insig=0 }
```

`closure.awk` (class C: coordinate-taking functions reachable by name from a P, B or H function):

```awk
# Class C: functions that take coordinates and are reachable, by name, from a routing reader.
# Inputs: sweep.tsv (file, line, fn, classes) then edges.tsv (F/E rows). Roots: every fn whose sweep class
# has P, B or H. Edges are resolved by name, so a shared name pulls in every function of that name
# (an over-approximation, the safe direction for a census).
FNR == NR { if ($4 ~ /[PBH]/) root[$3] = 1; insweep[$1 "\t" $3] = 1; next }
$1 == "F" { if ($5 == 1) { coordfn[$4] = 1; where[$4] = where[$4] $2 "\t" $3 "\n" } next }
$1 == "E" { calls[$2] = calls[$2] " " $3; next }
END {
  for (r in root) { reached[r] = 1; queue[++qn] = r }
  for (i = 1; i <= qn; i++) {
    n = split(calls[queue[i]], cs, " ")
    for (j = 1; j <= n; j++) {
      c = cs[j]
      if ((c in coordfn) && !(c in reached)) { reached[c] = 1; queue[++qn] = c }
    }
  }
  for (c in reached) if (c in coordfn) {
    m = split(where[c], ws, "\n")
    for (k = 1; k < m; k++) { split(ws[k], p, "\t"); if (!((p[1] "\t" c) in insweep)) printf "%s\t%s\t%s\tC\n", p[1], p[2], c }
  }
}
```

---

## Revision (b), 2026-10-09 — what changed after Codex's review (verdict REWORK, `reports/F3-design.review-codex.md`)

- **Inventory** (HOLD 1): §1.0 adds a mechanical census (method and output committed beside this note) — 323 direct readers and 71 one-level callers — of which the Codex-named readers are part; the needle and debt sets of §4.2 are now derived from it by the index, not written by hand.
- **R-5** (HOLD 2): the capture is one application slot, not one per window; a press proving a held capture stale cancels it first.
- **R-9 vs C5** (HOLD 3): *off the glass* is scoped to tab-bound owners; a drag is outside it because the spring is its own aim.
- **Wheel during a capture** (HOLD 4): Table A gains column `w`; R-10 states the capture half.
- **C22·h and U5** (HOLD 5): C22·h changed; U5 and seven other C rows that restated A cells became pins; totals recomputed from the tables (§3's method) — 270 decided cells, 150 changed, 120 unchanged, 4 n/a (Table B's first-revision sum of 28 was itself an arithmetic slip for 27; it is 28 now with B19).
- **Windows page ordering and DPI** (HOLD 6): §2.4 — the seat never hides with a button down; the page's `last` is page-local and a scale change releases it first.
- **macOS pages** (HOLD 7): scoped out with a consequence table (§2.5); 13 changed cells are Windows-only.
- **No allocation** (HOLD 8): `PointerFacts` per frame, a pure walk with no renderer access, a zero-allocation test, a visit-count test, an absolute and tail budget.
- **Cuts** (HOLD 9): cut 1 is dead-path scaffolding; cut 2 is a mirror that records overlaps and is read by nothing; cut 5 has a lifecycle-door matrix enforced by index pins and behavioural tests; cut 7 tests a real `WebSeat`'s presence order through a recording sink.
- **NITs:** the window-waits facts (231/231, "233" stale); ten cuts, fourteen commits; the not-a-nanny citation narrowed; R-7 freezes non-owner hover; synthetic-release modifiers; R-14 keyboard focus; the debt file's deletion at cut 10; R-8 marked as superseding the 2026-10-04 sentence.
- **§7 Boundaries** added (touch/pen, accessibility activation, the summoned window, 8 kHz mice, `OsDragHandoff`).

## Revision (c), 2026-10-09 — what changed after Codex's second review (REWORK, `reports/F3-design.review-codex-r2.md`)

- **Census and guard** (HOLD 1): §1.0 adds class C — coordinate-taking functions reachable by name from a P/B/H function — which finds all six readers the review named (`cmdrail::nearest`, `StripRun::aim`, `preview_live::press`, `settings_geometry::pointer_moved`, `VideoSeat::grab`, `drag_to`): 425 readers plus 63 callers, the companion TSV regenerated, and Appendix A shows every program and the final merge-and-sort command (rerun at base, the body reproduces byte for byte). §4.2 no longer tries to recognise readers: its needles are the four entrances of pointer data (the pointer fields, `PhysicalPosition<f64>` anywhere, the button/wheel types, the system cursor reads), checked over **every** `bt-app` item; the debt file is generated by the test's own query and passes on day one under `check-migration-debt.ps1`'s bootstrap rule; at cut 10 the file and its check are deleted together and the test's expected set is the empty constant.
- **R-5 vs R-8 across windows** (HOLD 2): the press precedence is one pure function, by button and never by window — same button: the held capture is stale, cancel then route (B20, cut 3); other button: swallowed, press and release, in any window (B21, cut 4); a two-window verdict test and an index pin that every window reaches the slot only through it.
- **macOS page press** (HOLD 3): today's surface-view fallback (consume, focus or raise, keyboard to the page, nothing forwarded) is kept through cut 8a by the `Page` answer calling the same door on every platform, with only the capture latch decided by `page_press_latches_a_capture(host_platform())`; pinned as U23 by a test driven with `HostPlatform::MacOs` and run in the Mac gate.
- **Cost proof** (HOLD 4): no fake maximum — floats are uncapped, so the stack is a parameter (F up to 512) and the visit count an exact formula (one frame per float down to the claimant, the claimant's parts only), which the stated mutation breaks at F = 8; `a_complete_event_walks_once` pins one walk per whole door.
- **Cut 3 and cut 5** (HOLD 5): the stale-capture press is cell B20 (cut 3); every lifecycle row names one exact item and file, three of them new extracted items (`window_blurred`, `settings_closed`, `drag_out_to_the_system`), with the window-closed row on `Runtime::close_window` (`runtime/windows.rs:1998`).
- **Q1** (HOLD 6): marked **pending the owner** in R-8, §0 and §6; cuts 1–3 do not depend on it.
- **NITs:** "Of the 151" is now "Of §3's 152" (the totals moved with B20, B21 and U23: 273 decided, 152 changed, 121 unchanged, 4 n/a); Appendix A's final command; `last_point` lives in `WebSeat` (written by `send_mouse`), and the capture's page snapshot is the leaf alone; `OsDragHandoff` is its own ending with no cancel verbs, and its own test.

## Revision (d), 2026-10-09 — Codex's third review (APPROVE WITH CHANGES, `reports/F3-design.review-codex-r3.md`)

- **Doors five and six** (§4.2): the pointer `WindowEvent` variants (N-W), which a `match` can destructure with inferred types, and the translated-touch `PanStep` road (N-T); each door's `bt_source` query is tabled; N-E is now the bare identifier `PhysicalPosition` (bt-app has no other use of the type), so constructors are covered; cut 1 moves the dispatcher's pointer arms, the station classifier's pointer rows and the touch door into `runtime::pointer`.
- **Debt rows** (§4.2): one row per occurrence (`owner<TAB>needle`, repeated), so the unchanged whole-row, multiplicity-preserving gate of `check-migration-debt.ps1` accepts every decrease and refuses every increase; no count column.
- **Swallowed buttons** (R-5 rule 3, R-8, cut 4): `App::swallowed` remembers a swallowed button until its own release, even after the capture ends; cell B22 (the two-window chord) and `a_swallowed_button_stays_swallowed_until_its_own_release`.
- **Totals** (§0, §2.5, §3): 274 decided cells — 153 changed (A 122, B 31), 121 unchanged (A 94, B 4, C 23), 4 n/a. The §3 totals line said "Of the 151" through revision (c) although revision (c)'s log said it had been changed; it now reads "Of §3's 153". Every other nit reported as taken in revisions (b) and (c) was re-checked in the text.
- **Status:** with these changes this is the approved design (Codex round 3: APPROVE WITH CHANGES; the changes are this revision). R-8 remains pending the owner's Q1 before cut 4 is built.
