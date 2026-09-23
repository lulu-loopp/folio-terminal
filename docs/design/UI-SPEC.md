# Folio UI spec: the current product

Read off `main` at `b031cfd2`, 2026-09-22. Companion: `UI-DEVIATIONS.md` in this directory, the ranked list of every place that does not follow these rules yet.

This spec states Folio's UI as it ships. It describes the current UI; it does not redesign it. It proposes no new values, no new components and no new look. Each **Rule** is the value most surfaces already use, or the value a dated ruling chose, with the constants and files that carry it. Each **Deviation** is a surface that uses a different value, written as `file.rs::CONSTANT = current → rule` with a one-line note on whether bringing it into line would be visible. Deviation IDs (R1, T3, G1, …) match `UI-DEVIATIONS.md`.

Anchors are file and constant names only, never line numbers. Files are under `crates/bt-app/src/` unless noted. `theme.rs`, `motion.rs`, `rounded_rect.rs`, `contrast.rs` and `lib.rs` are under `crates/bt-render/src/`. "pt" means logical px (the constants' `_LOGICAL_PX` unit, scaled by the monitor factor at draw time).

The why behind the look lives in `docs/UI-UX.md`; the history of each decision lives in `docs/DESIGN.md`. This file holds the numbers. Four questions the product used to answer two ways were ruled on 2026-09-22 and are written below as rules: the terminal pane's radius (§2), the icon-to-label gap (§3), the head title size (§5) and the boolean control (§10.1).

---

## 1. Principles (what the current UI demonstrably does)

1. **Accent means "something over there needs you", or transient feedback under the hand. It never marks where you already are.** Standing rule: `docs/UI-UX.md` §二 ("accent = 注意力，不是位置"). Its table allows accent for unread / needs-you and for transient focus rings, drop previews and divider drags, and forbids it for the focused pane, the active tab and the current item (which use a contrast step). The product keeps this. The focused pane head is not filled. Unfocused pane marks drop to 50% (`seats.rs::PANE_MARK_UNFOCUSED_OPACITY = 0.5`). Menus are not coloured. A running tab's mark breathes in accent. **One stated exception:** the first-run card's switches paint their track in the accent when on (§10.1). It is the only persistent state the accent marks, and it lives on that one card.
2. **A dialog's recommended answer may wear the accent. A dialog with a destructive answer paints no button in the accent.** The restore prompt and first-run `Done` are accent primaries (`restore.rs`, `BUTTON_PRIMARY_HOVER_BRIGHTNESS`). The dirty gate / quit card paints "**No button** … in the accent, `Save` included", because one of its answers destroys work.
3. **One region, one language.** A hover gives a background fill only, never a border or a rule (`docs/UI-UX.md` §7.2). Every hover in the product is a fill.
4. **Seams are a 1-pt edge.** Chrome/content boundaries, card frames, tag frames, menu frames and pane dividers all draw at 1.0 pt (§6.3). Panel grey is chrome and white/terminal ground is content (`docs/UI-UX.md` §三).
5. **Every floating surface goes through one chassis.** `settings.rs::push_float_window` (rounded rect + hairline + two-ring shadow) serves tooltip, toast, key hint, card hint, palette, peek card, peek strip, float window, menus, first-run, restore, settings, search and the web sheet. So every elevation deviation is an argument passed to that function, not a separate drawing.
6. **Every menu is built by one chassis.** `profiles.rs` builds the terminal, files, tab, pane `⌄` and git menus. `context_menu.rs` and `explorer_menu.rs` define no metrics of their own.
7. **Every chrome colour comes from the scheme.** A user scheme repaints the whole window, and one contrast authority (`contrast::raise_against` / `raise_to_floor`) lifts text to its floor (`docs/DESIGN.md` §7.28).
8. **There is one scrim value, and only dialogs that block the window lay it** (§6.2).
9. **Motion is a closed system**: three spans, three curves, one travel distance and a gate (§7).
10. **Icons use one geometry and one pen**, held by an optical gate (§8).

---

## 2. Radius

**Rule: an element's radius is set by what the element is.** The ladder is **2 · 3 · 4 · 5 · 6 · 7 · 8 · 10**, plus the pill (radius = half the height) and the terminal pane at rest (0). 9 is not on the ladder.

| Class | Radius | Members (`file.rs::CONSTANT`) |
|---|---|---|
| Strokes, ticks, thumbs, grips | **2** | `cmdrail.rs::TICK_RADIUS_LOGICAL_PX`, `termscroll.rs::THUMB_RADIUS_LOGICAL_PX`, `theme.rs::SEAT_DIVIDER_GRIP_RADIUS_LOGICAL_PX` |
| Miniatures and in-text washes | **3** | `theme.rs::FOCUS_MINI_RADIUS_LOGICAL_PX`, `profiles.rs::PICKER_ZONE_RADIUS_LOGICAL_PX`, `lib.rs::SEARCH_MATCH_RADIUS_LOGICAL_PX` |
| Chips, badges, key caps, crumbs (≤ 20 tall); close boxes of 16–17 pt inside a tab or head | **4** | `theme.rs::WINDOW_TAB_BADGE_RADIUS_LOGICAL_PX`, `theme.rs::WINDOW_TAB_CLOSE_RADIUS_LOGICAL_PX`, `theme.rs::SEAT_PANE_CLOSE_RADIUS_LOGICAL_PX`, `theme.rs::FOCUS_CARD_CLOSE_RADIUS_LOGICAL_PX`, `settings.rs::CAP_RADIUS_LOGICAL_PX`, `settings.rs::PROFILE_BADGE_RADIUS_LOGICAL_PX`, `seats.rs::PREVIEW_CRUMB_RADIUS_LOGICAL_PX`, `file_peek.rs::PEEK_TYPE_RADIUS_LOGICAL_PX`, `main.rs::MARKDOWN_CHIP_RADIUS_LOGICAL_PX`, `peek_strip.rs::LEAF_RADIUS_LOGICAL_PX`, `profiles.rs::PICKER_PANE_RADIUS_LOGICAL_PX`, `cmdrail.rs::JUMP_FLASH_RADIUS_LOGICAL_PX`, `cmdrail.rs::JUMP_FLASH_RING_RADIUS_LOGICAL_PX` |
| Icon/tool boxes of 18–22 pt; rows in a menu or the files tree; small fields | **5** | `seats.rs::PREVIEW_TOOL_RADIUS_LOGICAL_PX`, `seats.rs::PANE_HEAD_TRIGGER_RADIUS_LOGICAL_PX`, `seats.rs::PREVIEW_SWITCH_RADIUS_LOGICAL_PX`, `seats.rs::FILES_ROOT_BUTTON_RADIUS_LOGICAL_PX`, `git_graph.rs::GRAPH_TOOL_RADIUS_LOGICAL_PX`, `git_panel.rs::GIT_ACT_RADIUS_LOGICAL_PX`, `float.rs::FLOAT_BUTTON_RADIUS_LOGICAL_PX`, `lib.rs::MATH_TOOL_PILL_RADIUS_LOGICAL_PX`, `profiles.rs::ITEM_RADIUS_LOGICAL_PX`, `settings.rs::ITEM_RADIUS_LOGICAL_PX`, `seats.rs::FILES_ROW_RADIUS_LOGICAL_PX`, `profiles.rs::GIT_PROMPT_FIELD_RADIUS_LOGICAL_PX` |
| Buttons, combos, 30-pt close boxes, strip boxes (new-tab `+`/`⌄`, panel toggle), rows in any other list (settings nav, vertical rail, git panel, restore list), buttons on in-pane cards | **6** | `settings.rs::BUTTON_RADIUS_LOGICAL_PX`, `settings.rs::COMBO_RADIUS_LOGICAL_PX`, `settings.rs::CLOSE_RADIUS_LOGICAL_PX`, `settings.rs::NAV_ITEM_RADIUS_LOGICAL_PX`, `settings.rs::FOCUS_RING_RADIUS_LOGICAL_PX`, `first_run.rs::BUTTON_RADIUS_LOGICAL_PX`, `restore.rs::BUTTON_RADIUS_LOGICAL_PX`, `restore.rs::ROW_RADIUS_LOGICAL_PX`, `websheet.rs::VERB_RADIUS_LOGICAL_PX`, `seats.rs::PREVIEW_CARD_BUTTON_RADIUS_LOGICAL_PX`, `seats.rs::NEWS_PILL_RADIUS_LOGICAL_PX`, `seats.rs::WINDOW_PANEL_TOGGLE_RADIUS_LOGICAL_PX`, `theme.rs::WINDOW_NEW_TAB_RADIUS_LOGICAL_PX`, `theme.rs::RAIL_TAB_RADIUS_LOGICAL_PX`, `theme.rs::DOCK_SHIFT_RADIUS_LOGICAL_PX`, `git_panel.rs::GIT_ROW_RADIUS_LOGICAL_PX`, `file_peek.rs::PEEK_IMAGE_RADIUS_LOGICAL_PX` |
| A tab and the drag ghost that stands for one; the markdown code ground | **7** | `theme.rs::WINDOW_TAB_RADIUS_LOGICAL_PX` (Windows attached tab and Mac floating pill), `theme.rs::DRAG_GHOST_RADIUS_LOGICAL_PX`, `theme.rs::PREVIEW_CODE_GROUND_RADIUS_LOGICAL_PX` |
| Floating surfaces of the menu / float-tag family: menus, toast, key hint, card hint, peek card, peek strip, peek tip, search capsule, resizing card, dock preview | **8** | `profiles.rs::MENU_RADIUS_LOGICAL_PX`, `settings.rs::MENU_RADIUS_LOGICAL_PX`, `toast.rs::TOAST_RADIUS_LOGICAL_PX`, `keyhint.rs::KEY_HINT_RADIUS_LOGICAL_PX` (card hint reuses it), `file_peek.rs::PEEK_RADIUS_LOGICAL_PX`, `peek_strip.rs::PEEK_RADIUS_LOGICAL_PX`, `tooltip.rs::PEEK_RADIUS_LOGICAL_PX`, `search.rs::CAPSULE_RADIUS_LOGICAL_PX`, `theme.rs::SEAT_RESIZING_CARD_RADIUS_LOGICAL_PX`, `theme.rs::DOCK_PREVIEW_RADIUS_LOGICAL_PX` |
| Dialogs and windows within the window: settings, first-run, restore, quit gate, palette, float window, focus card | **10** | `theme.rs::FLOAT_WINDOW_RADIUS_LOGICAL_PX` (settings, first-run and restore all pass this constant), `palette.rs::PALETTE_RADIUS_LOGICAL_PX`, `theme.rs::FOCUS_CARD_RADIUS_LOGICAL_PX` |
| Pill (fully round ends) | **h/2** | `seats.rs::PREVIEW_COUNT_RADIUS_LOGICAL_PX` (8 on 14 tall), `git_panel.rs::GIT_PILL_RADIUS_LOGICAL_PX` (9 on 16), `git_graph.rs::GRAPH_REF_RADIUS_LOGICAL_PX` (9 on 16). Each is clamped and draws round. |
| Terminal pane container | **0 at rest; 8 only while a divider is held** | See below. |

**The terminal pane is square at rest** (ruled 2026-09-22). Panes fill their slots flush, separated by a 1-pt hairline (`theme.rs::SEAT_DIVIDER_VISUAL_LOGICAL_PX`); there is no radius constant for the resting pane because there is no radius. While a divider is held, both panes inset 5 and become r8 cards with a shadow (`theme.rs::SEAT_RESIZING_CARD_RADIUS_LOGICAL_PX` 8, `theme.rs::SEAT_RESIZING_CARD_MARGIN_LOGICAL_PX` 5), as ruled in `docs/UI-UX.md` §六. The change of shape is the drag signal, so a resting radius would weaken it, and it would cost every pane the margin its corners need against `MIN_PANE_W` / `MIN_PANE_H`.

Icon bodies carry their own `rx` on the 16-unit grid (`marks.rs`; `HOUSE_OUTER_RADIUS_UNITS = 2.0`) and are governed by §8, not this ladder.

**Deviations**

- **R1** `tooltip.rs::TIP_RADIUS_LOGICAL_PX = 5 → 8`. The tooltip is in the float-tag family (`docs/DESIGN.md` §7.28), and every other member is r8, including the peek tip in the same file. *Visible: the tooltip appears on every hover.*
- **R2** `websheet.rs::RADIUS_LOGICAL_PX = 8 → 10`. The web sheet is scrimmed and modal, and its padding is a dialog's. *Visible side by side; rare surface.*
- **R3** `palette.rs::ROW_RADIUS_LOGICAL_PX = 7 → 6` (list row). The constant with the same name in `restore.rs` is 6. *Barely visible.*
- **R4** `git_graph.rs::GRAPH_ROW_RADIUS_LOGICAL_PX = 7 → 6` (list row). *Barely visible.*
- **R5** `git_panel.rs::GIT_SECTION_RADIUS_LOGICAL_PX = 9 → 8`. This is the only 9 on something that is not a pill. *Barely visible; large plates, 1 pt.*
- **R6** `notice.rs::VERB_RADIUS_LOGICAL_PX = 5 → 6` (button). The constant with the same name in `websheet.rs` is 6. *Invisible at 22 tall.*
- **R7** `toast.rs::TOAST_ACTION_RADIUS_LOGICAL_PX = 5 → 6` (button). *Invisible.*
- **R8** 22-pt boxes drawn at 6 → 5: `search.rs::BUTTON_RADIUS_LOGICAL_PX`, `notice.rs::CLOSE_RADIUS_LOGICAL_PX`, `websheet.rs::CLOSE_RADIUS_LOGICAL_PX`, `seats.rs::PANE_GHOST_RADIUS_LOGICAL_PX`. *Invisible.*
- **R9** `settings.rs::MENU_ACT_RADIUS_LOGICAL_PX = 4 → 5` (18-pt box). *Invisible.*
- **R10** Close boxes of 16–18 pt drawn at 5 → 4: `toast.rs::TOAST_CLOSE_RADIUS_LOGICAL_PX`, and the float head's 17-pt close box (`float.rs::FLOAT_CLOSE_BOX_LOGICAL_PX`), which draws with `float.rs::FLOAT_BUTTON_RADIUS_LOGICAL_PX = 5`. *Invisible.*
- **R11** `git_panel.rs::GIT_BADGE_RADIUS_LOGICAL_PX = 5 → 4` (badge). *Invisible.*
- **R12** `seats.rs::PREVIEW_ADDRESS_RADIUS_LOGICAL_PX = 5 → 4`. The web head's address is a 20-pt chip, like the file head's crumbs (`PREVIEW_CRUMB_RADIUS_LOGICAL_PX = 4`). *Invisible.*
- **R13** The pill constants (`PREVIEW_COUNT_RADIUS_LOGICAL_PX` 8, `GIT_PILL_RADIUS_LOGICAL_PX` 9, `GRAPH_REF_RADIUS_LOGICAL_PX` 9) state a number larger than they draw. *Invisible; spelling only (they draw h/2).*

---

## 3. Spacing

There is no single step. The rules are per role, and each role's value is the one most of its members use, or the one a ruling chose.

| Role | Rule | Members |
|---|---|---|
| Dialog padding | **20 top · 22 sides · 16 bottom** | `restore.rs::DIALOG_PADDING_TOP_LOGICAL_PX` / `DIALOG_PADDING_X_LOGICAL_PX` / `DIALOG_PADDING_BOTTOM_LOGICAL_PX`; `first_run.rs::PADDING_TOP_LOGICAL_PX` / `PADDING_X_LOGICAL_PX` / `PADDING_BOTTOM_LOGICAL_PX`. Settings uses 22 on the sides (`settings.rs::HEADER_PADDING_LEFT_LOGICAL_PX`, `CONTENT_PADDING_X_LOGICAL_PX`). |
| Multi-line float tag (toast, key hint, card hint) | **12 × 10** | `toast.rs::TOAST_PADDING_X_LOGICAL_PX` / `TOAST_PADDING_Y_LOGICAL_PX`, `keyhint.rs::KEY_HINT_PADDING_X_LOGICAL_PX` / `KEY_HINT_PADDING_Y_LOGICAL_PX` (card hint reuses these) |
| Single-line tag; small head or foot line | **10 × 5** | `tooltip.rs::PEEK_PADDING_X_LOGICAL_PX` / `PEEK_PADDING_Y_LOGICAL_PX`, `file_peek.rs::PEEK_FOOT_PADDING_X_LOGICAL_PX` / `PEEK_FOOT_PADDING_Y_LOGICAL_PX`, `file_peek.rs::PEEK_HEAD_PADDING_X_LOGICAL_PX` |
| Menu popup | **4 all round** | `profiles.rs::MENU_PADDING_LOGICAL_PX`, `settings.rs::MENU_PADDING_LOGICAL_PX` |
| Strip (a 30-pt head or bar): leading · trailing | **12 · 6** | `theme.rs::SEAT_TITLE_PADDING_LOGICAL_PX` / `SEAT_TITLE_TRAILING_PADDING_LOGICAL_PX`, `theme.rs::WINDOW_TAB_PADDING_LEFT_LOGICAL_PX` / `WINDOW_TAB_PADDING_RIGHT_LOGICAL_PX`, `seats.rs::FILES_FOOT_PADDING_X_LOGICAL_PX` (12) |
| Text inset in a selectable row | **10** | `profiles.rs::ITEM_PADDING_X_LOGICAL_PX`, `settings.rs::ITEM_PADDING_X_LOGICAL_PX`, `palette.rs::ROW_PADDING_X_LOGICAL_PX`, `theme.rs::RAIL_TAB_PADDING_LEFT_LOGICAL_PX`. The files tree gets the same 10 from `FILES_TREE_PADDING_X_LOGICAL_PX` 4 + `FILES_ROW_PADDING_X_LOGICAL_PX` 6, then its indent ladder `FILES_ROW_INDENT_LOGICAL_PX` 14. |
| Icon to its label | **8** | `theme.rs::WINDOW_TAB_GAP_LOGICAL_PX`, `toast.rs::TOAST_MARK_GAP_LOGICAL_PX`, `git_panel.rs::GIT_ROW_GAP_LOGICAL_PX`, `git_panel.rs::GIT_HEAD_GAP_LOGICAL_PX`, `restore.rs::ROW_GAP_LOGICAL_PX`, `settings.rs::ITEM_GAP_LOGICAL_PX`. Ruled 2026-09-22: one gap everywhere a mark sits beside the text it names, including a label and the dot or badge that follows it. |
| Preview body | **12 × 10** | `seats.rs::PREVIEW_TEXT_PADDING_X_LOGICAL_PX` / `PREVIEW_TEXT_PADDING_Y_LOGICAL_PX`, `theme.rs::PREVIEW_BODY_INSET_LOGICAL_PX` (12) |
| Overlay to window edge | **8** anchored, **24** centred | 8: `profiles.rs::MENU_EDGE_MARGIN_LOGICAL_PX`, `theme.rs::FLOAT_WINDOW_VIEWPORT_MARGIN_LOGICAL_PX`, `file_peek.rs::PEEK_VIEWPORT_MARGIN_LOGICAL_PX`, `toast.rs::TOAST_ANCHOR_INSET_LOGICAL_PX`. 24: `palette.rs::PALETTE_EDGE_MARGIN_LOGICAL_PX`, `websheet.rs::MARGIN_LOGICAL_PX` |
| Float layer to window corner | **16** | `toast.rs::TOAST_WINDOW_INSET_LOGICAL_PX`, `keyhint.rs::KEY_HINT_WINDOW_INSET_LOGICAL_PX` |
| Space above an uppercase section label in a dialog or panel | **10** | `settings.rs::GROUP_LABEL_MARGIN_TOP_LOGICAL_PX`. Menus (3, `profiles.rs::SECTION_LABEL_PADDING_TOP_LOGICAL_PX`) and the rail (4, `theme.rs::RAIL_LABEL_PADDING_TOP_LOGICAL_PX`) are tighter containers by kind. |
| Button padding | **14 × 6** | `first_run.rs::BUTTON_PADDING_X_LOGICAL_PX` / `BUTTON_PADDING_Y_LOGICAL_PX`, `restore.rs::BUTTON_PADDING_X_LOGICAL_PX` / `BUTTON_PADDING_Y_LOGICAL_PX` |
| Between dialog buttons | **8** | `first_run.rs::BUTTON_GAP_LOGICAL_PX`, `restore.rs::ACTIONS_GAP_LOGICAL_PX` |

The icon-to-label gap is for a mark at a slot size (§8) beside its text. The gap inside a card whose mark is a hero mark (30 pt; the 22-pt app-mark tile) is the card's own stack gap and is not this rule.

Layout floors are ruled and not touched: `bt-layout::MIN_PANE_W` 260, `MIN_PANE_H` 120, `FILES_W` 240, `FILES_W_MIN` 170 (`crates/bt-layout/src/lib.rs`; `docs/DESIGN.md` §7.1.1; `docs/UI-UX.md` §五).

**Deviations**

- **S1** Tooltip: `tooltip.rs::TIP_PADDING_X_LOGICAL_PX = 7 → 10`, `tooltip.rs::TIP_PADDING_Y_LOGICAL_PX = 3 → 5` (single-line tag; the peek tip in the same file already uses 10 × 5). *Visible: the tooltip is the tightest box in the product.*
- **S2** Web sheet: `websheet.rs::PADDING_LOGICAL_PX = 22` on all four sides → 20 / 22 / 16. *Visible: 6 pt more air at the bottom than any other dialog.*
- **S3** `first_run.rs::SURFACE_MARGIN_LOGICAL_PX = 34 → 24` (centred overlay to window edge). *Visible only in small windows.*
- **S4** `git_panel.rs::GIT_LABEL_PADDING_TOP_LOGICAL_PX = 14 → 10` (section label in a panel). *Visible: the git panel's section breaks are looser than Settings'.*
- **S5** Float head: `float.rs::FLOAT_HEAD_PADDING_LEFT_LOGICAL_PX = 10 → 12`, `float.rs::FLOAT_HEAD_PADDING_RIGHT_LOGICAL_PX = 5 → 6`. A float window is a popped-out pane and its head should match the pane head. *Barely visible.*
- **S6** Notice strip: `notice.rs::PADDING_LEFT_LOGICAL_PX = 11 → 12`, `notice.rs::PADDING_RIGHT_LOGICAL_PX = 8 → 6`. *Invisible.*
- **S7** `video_seat.rs::BAR_PADDING_X_LOGICAL_PX = 10 → 12` (strip). *Invisible.*
- **S8** Row text inset → 10: `git_panel.rs::GIT_ROW_PADDING_X_LOGICAL_PX` 7, `git_graph.rs::GRAPH_ROW_PADDING_X_LOGICAL_PX` 8, `restore.rs::ROW_PADDING_X_LOGICAL_PX` 8, `settings.rs::NAV_ITEM_PADDING_LEFT_LOGICAL_PX` 12. *Barely visible: a 2–3 pt shift in where row text starts.*
- **S9** `theme.rs::DRAG_GHOST_PADDING_X_LOGICAL_PX = 12 → 10` (single-line tag). *Invisible; only seen mid-drag.*
- **S10** `settings.rs::CONTENT_PADDING_BOTTOM_LOGICAL_PX = 18 → 16` (dialog bottom). *Invisible; the content scrolls.*
- **G1** Icon-to-label gap 6 → 8: `seats.rs::FILES_ROW_GAP_LOGICAL_PX`, `seats.rs::FILES_FOOT_GAP_LOGICAL_PX`, `float.rs::FLOAT_HEAD_GAP_LOGICAL_PX`, `theme.rs::FOCUS_CARD_HEAD_GAP_LOGICAL_PX`, `file_peek.rs::PEEK_HEAD_GAP_LOGICAL_PX`, `git_panel.rs::FILES_BADGE_GAP_LOGICAL_PX`. *Visible side by side: the files column and the small heads loosen by 2 pt.*
- **G2** Icon-to-label gap 10 → 8: `profiles.rs::ITEM_GAP_LOGICAL_PX`, which every menu `profiles.rs` builds uses. *Visible side by side: every right-click and `⌄` menu's text moves 2 pt toward its icon.*
- **G3** Icon-to-label gap 7 or 9 → 8: `theme.rs::SEAT_TITLE_GAP_LOGICAL_PX` 7, `theme.rs::DRAG_GHOST_GAP_LOGICAL_PX` 7, `palette.rs::DOT_GAP_LOGICAL_PX` 7, `palette.rs::ROW_GAP_LOGICAL_PX` 9, `git_graph.rs::GRAPH_ROW_GAP_LOGICAL_PX` 9. *Barely visible: 1 pt each; the pane head's is on every pane.*

---

## 4. Heights (density)

| Role | Rule | Members |
|---|---|---|
| Window title band | **40** | `theme.rs::WINDOW_TITLE_BAR_LOGICAL_PX`; Windows caption slots 46 × 40 (`theme.rs::WINDOW_CAPTION_BUTTON_LOGICAL_PX`). Fluent metrics, ruled in `docs/UI-UX.md` §1.1 and §三. |
| Tab | **34** attached (Windows), **30** floating pill (Mac) | `theme.rs::WINDOW_TAB_HEIGHT_LOGICAL_PX`, `theme.rs::WINDOW_TAB_FLOAT_HEIGHT_LOGICAL_PX` (`docs/DESIGN.md` §13.48) |
| Strip, head, 30-pt list row | **30** | `theme.rs::SEAT_TITLE_BAR_LOGICAL_PX`, `seats.rs::FILES_SEG_BAR_LOGICAL_PX`, `notice.rs::BAR_HEIGHT_LOGICAL_PX`, `palette.rs::ROW_HEIGHT_LOGICAL_PX`, `settings.rs::NAV_ITEM_HEIGHT_LOGICAL_PX`, `theme.rs::RAIL_TAB_HEIGHT_LOGICAL_PX`, `git_graph.rs::GRAPH_ROW_HEIGHT_LOGICAL_PX`, `settings.rs::CLOSE_SIDE_LOGICAL_PX` |
| Menu row | **29.5** | `profiles.rs::ITEM_HEIGHT_LOGICAL_PX` |
| Button, combo | **27.5** | `settings.rs::BUTTON_HEIGHT_LOGICAL_PX`, `settings.rs::COMBO_HEIGHT_LOGICAL_PX`. First-run and restore buttons compute the same: 6 + 15.5 + 6 (`BUTTON_PADDING_Y_LOGICAL_PX`, `BUTTON_LINE_LOGICAL_PX`). |
| Strip box (new-tab `+` / `⌄`, rail chevron) | **28** | `theme.rs::WINDOW_NEW_TAB_BOX_LOGICAL_PX`, `theme.rs::RAIL_NEW_CHEVRON_BOX_LOGICAL_PX`, `seats.rs::NEWS_PILL_HEIGHT_LOGICAL_PX` |
| Tree row, key-hint row | **24** | `seats.rs::FILES_ROW_HEIGHT_LOGICAL_PX`, `keyhint.rs::KEY_HINT_ROW_HEIGHT_LOGICAL_PX`, `settings.rs::ENV_ADD_HEIGHT_LOGICAL_PX` |
| Tool / icon box in a head, verb pill, toggle | **22** | `seats.rs::PREVIEW_TOOL_BOX_LOGICAL_PX`, `git_graph.rs::GRAPH_TOOL_HEIGHT_LOGICAL_PX`, `search.rs::BUTTON_BOX_LOGICAL_PX`, `search.rs::TOGGLE_HEIGHT_LOGICAL_PX`, `notice.rs::VERB_HEIGHT_LOGICAL_PX`, `notice.rs::CLOSE_BOX_LOGICAL_PX`, `websheet.rs::CLOSE_BOX_LOGICAL_PX`, `seats.rs::PANE_GHOST_BOX_LOGICAL_PX`, `palette.rs::HEADING_HEIGHT_LOGICAL_PX` |
| Chip (crumb, address, key cap, hover tag) | **20** | `seats.rs::PREVIEW_CRUMB_HEIGHT_LOGICAL_PX`, `seats.rs::PREVIEW_ADDRESS_HEIGHT_LOGICAL_PX`, `settings.rs::CAP_HEIGHT_LOGICAL_PX`, `seats.rs::PAGE_HOVER_TAG_HEIGHT_LOGICAL_PX` |
| Close box in a tab or head | **17** | `theme.rs::WINDOW_TAB_CLOSE_BOX_LOGICAL_PX`, `theme.rs::SEAT_PANE_CLOSE_BOX_LOGICAL_PX`, `float.rs::FLOAT_CLOSE_BOX_LOGICAL_PX` |
| Badge / pill | **15–16** | `theme.rs::WINDOW_TAB_BADGE_HEIGHT_LOGICAL_PX` 15, `settings.rs::PROFILE_BADGE_HEIGHT_LOGICAL_PX` 15, `git_panel.rs::GIT_PILL_HEIGHT_LOGICAL_PX` 16, `git_graph.rs::GRAPH_REF_HEIGHT_LOGICAL_PX` 16 |

The palette field (`palette.rs::FIELD_HEIGHT_LOGICAL_PX` 42) and the settings header (`settings.rs::HEADER_HEIGHT_LOGICAL_PX` 56) are each the only member of their role.

**Deviations**

- **H1** `settings.rs::ITEM_HEIGHT_LOGICAL_PX = 27.5 → 29.5`. The settings combo's drop-down is a menu, and every other menu row is 29.5. *Visible side by side: the combo list is denser than the right-click menus.*
- **H2** `git_panel.rs::GIT_ROW_HEIGHT_LOGICAL_PX = 27 → 24`. The git page is a page of the files column, whose rows are 24. *Visible when switching `Files` ↔ `Git`.*
- **H3** 19-pt boxes → 22 (tool box): `seats.rs::PANE_HEAD_TRIGGER_BOX_LOGICAL_PX`, `seats.rs::PREVIEW_SWITCH_HEIGHT_LOGICAL_PX`, `float.rs::FLOAT_DOCK_HEIGHT_LOGICAL_PX`, `seats.rs::FILES_ROOT_BUTTON_HEIGHT_LOGICAL_PX`. *Visible on hover: the pane head `⌄` plate grows by 3 pt.*
- **H4** `first_run.rs::ROW_HEIGHT_LOGICAL_PX = 42 → 38.5`. 38.5 is the settings single-line row: `settings.rs::ROW_PADDING_Y_LOGICAL_PX` 11 × 2 + `settings.rs::ROW_TITLE_LINE_LOGICAL_PX` 16.5. *Visible: the first-run card is taller and airier than the same options in Settings.*
- **H5** `seats.rs::FILES_FOOT_BAR_LOGICAL_PX = 28 → 30` (strip). *Barely visible.*
- **H6** `video_seat.rs::VIDEO_BAR_HEIGHT_LOGICAL_PX = 34 → 30` (strip). *Visible on the video bar; rare surface.*
- **H7** Close boxes → 17: `toast.rs::TOAST_CLOSE_LOGICAL_PX` 18, `theme.rs::FOCUS_CARD_CLOSE_BOX_LOGICAL_PX` 16. *Invisible.*

---

## 5. Type

**Faces.** Chrome text uses the system UI face. On Windows that is Segoe UI Variable → Segoe UI (`lib.rs::CHROME_SANS_FONT_FILES`). On macOS it is SF Pro Text → .AppleSystemUIFont → Helvetica Neue (`lib.rs::MACOS_CHROME_SANS_FAMILIES`).

The terminal uses `DEFAULT_PRIMARY_FONT_FAMILY` (Consolas / Menlo) at `lib.rs::DEFAULT_TERMINAL_FONT_SIZE_LOGICAL_PX` 16, line ratio `LINE_HEIGHT_TO_FONT_SIZE_RATIO` 22/16. CJK text uses two chains: `CJK_FALLBACK_FAMILIES` (proportional) and `grid_cjk_fallback_families()` (grid, NSimSun first by ruling).

No UI font is bundled. `docs/UI-UX.md` §二's "Inter / JetBrains Mono, 内嵌" describes the HTML prototype. It is not a rule of the shipped UI.

**Weight** is a three-value enum, `lib.rs::ChromeLabelWeight`. The focused pane's title is marked by weight, not colour (`docs/UI-UX.md` §二).

**Sizes by role.** The chrome ladder is **15 · 13 · 12.5 · 12 · 11 · 10**, plus the terminal's 16 (a user setting). 11.5 and 13.5 are not on the ladder.

**Every head title is 11** (ruled 2026-09-22): the pane head, the float window's head and the glance card's head are one kind of head and share one size.

| Role | Size | Members |
|---|---|---|
| Dialog title | **15** | `first_run.rs::TITLE_FONT_LOGICAL_PX`, `restore.rs::TITLE_FONT_LOGICAL_PX` |
| Primary text, i.e. anything you read to choose (tab, menu item, tree row, list row, button, combo, field, dialog row), plus markdown body | **13** | `theme.rs::WINDOW_TAB_FONT_LOGICAL_PX`, `profiles.rs::ITEM_FONT_LOGICAL_PX`, `seats.rs::FILES_TREE_FONT_LOGICAL_PX`, `settings.rs::BUTTON_FONT_LOGICAL_PX`, `settings.rs::COMBO_FONT_LOGICAL_PX`, `settings.rs::FIELD_FONT_LOGICAL_PX`, `first_run.rs::ROW_FONT_LOGICAL_PX`, `first_run.rs::BUTTON_FONT_LOGICAL_PX`, `restore.rs::ROW_FONT_LOGICAL_PX`, `restore.rs::BUTTON_FONT_LOGICAL_PX`, `preview.rs::PREVIEW_MD_FONT_LOGICAL_PX` |
| Secondary names and sentences: a tag's title, a strip's name, a dialog's sub-line, mono text in chrome | **12.5** | `toast.rs::TOAST_TITLE_FONT_LOGICAL_PX`, `seats.rs::APP_TITLE_FONT_LOGICAL_PX`, `seats.rs::PREVIEW_NAME_FONT_LOGICAL_PX`, `theme.rs::DRAG_GHOST_FONT_LOGICAL_PX`, `theme.rs::DOCK_PREVIEW_FONT_LOGICAL_PX`, `restore.rs::SUB_FONT_LOGICAL_PX`, `websheet.rs::SAY_FONT_LOGICAL_PX`, `seats.rs::PREVIEW_CARD_FONT_LOGICAL_PX`, `seats.rs::PREVIEW_TEXT_FONT_LOGICAL_PX`, `settings.rs::FIELD_MONO_FONT_LOGICAL_PX`, `settings.rs::PROFILE_ACT_FONT_LOGICAL_PX`, `settings.rs::SLIDER_VALUE_FONT_LOGICAL_PX`, `profiles.rs::GIT_PROMPT_FONT_LOGICAL_PX`, `git_graph.rs::GRAPH_REPO_FONT_LOGICAL_PX` |
| Body in a float tag; descriptions; inline verbs | **12** | `toast.rs::TOAST_BODY_FONT_LOGICAL_PX`, `toast.rs::TOAST_ACTION_FONT_LOGICAL_PX`, `notice.rs::FONT_LOGICAL_PX`, `keyhint.rs::KEY_HINT_NAME_FONT_LOGICAL_PX`, `tooltip.rs::PEEK_FONT_LOGICAL_PX`, `settings.rs::ROW_DESC_FONT_LOGICAL_PX`, `palette.rs::EMPTY_FONT_LOGICAL_PX`, `websheet.rs::VERB_FONT_LOGICAL_PX`, `seats.rs::PREVIEW_CARD_BUTTON_FONT_LOGICAL_PX`, `seats.rs::PREVIEW_RAIL_FONT_LOGICAL_PX`, `seats.rs::PREVIEW_TABLE_FONT_LOGICAL_PX` |
| Caption: head titles, labels, hints, tooltip, foot lines | **11** | `theme.rs::HEAD_TITLE_FONT_LOGICAL_PX` (float and peek heads), `theme.rs::RAIL_LABEL_FONT_LOGICAL_PX`, `theme.rs::FOCUS_CARD_FONT_LOGICAL_PX`, `tooltip.rs::TIP_FONT_LOGICAL_PX`, `palette.rs::HEADING_FONT_LOGICAL_PX`, `palette.rs::HINT_FONT_LOGICAL_PX`, `profiles.rs::HINT_FONT_LOGICAL_PX`, `settings.rs::GROUP_LABEL_FONT_LOGICAL_PX`, `settings.rs::TICK_FONT_LOGICAL_PX`, `seats.rs::FILES_FOOT_FONT_LOGICAL_PX`, `float.rs::FLOAT_FOOT_FONT_LOGICAL_PX`, `first_run.rs::SETTINGS_LINE_FONT_LOGICAL_PX`, `file_peek.rs::PEEK_NONE_FONT_LOGICAL_PX`, `search.rs::TOGGLE_FONT_LOGICAL_PX`, `git_graph.rs::GRAPH_AUTHOR_FONT_LOGICAL_PX`, `peek_strip.rs::LIST_FONT_LOGICAL_PX`, `cardhint.rs::CARD_HINT_PLUS_FONT_LOGICAL_PX` |
| Badge, pill, type tag, time | **10** | `theme.rs::WINDOW_TAB_BADGE_FONT_LOGICAL_PX`, `git_panel.rs::GIT_BADGE_FONT_LOGICAL_PX`, `git_panel.rs::GIT_TIME_FONT_LOGICAL_PX`, `file_peek.rs::PEEK_TYPE_FONT_LOGICAL_PX`, `file_peek.rs::PEEK_FOOT_FONT_LOGICAL_PX`, `float.rs::FLOAT_DOCK_FONT_LOGICAL_PX` |
| Uppercase section label | **11**, tracking **0.05 em**, line 13 | `settings.rs::GROUP_LABEL_FONT_LOGICAL_PX` / `GROUP_LABEL_TRACKING_EM` / `GROUP_LABEL_LINE_LOGICAL_PX`, `theme.rs::RAIL_LABEL_FONT_LOGICAL_PX` / `RAIL_LABEL_LINE_LOGICAL_PX` |
| Miniature (focus-mode thumbnails) | 7.5 / 8 | `theme.rs::FOCUS_MINI_TERM_FONT_LOGICAL_PX`, `theme.rs::FOCUS_MINI_FILES_FONT_LOGICAL_PX`. These draw a pane at reduced scale and are outside the ladder by kind. |
| Markdown face | em-based (`preview.rs::PREVIEW_MD_*`) | Keeps its own em recipe (code 0.85, heading/table/quote ems, `PREVIEW_MD_LANG_FONT_LOGICAL_PX` 9.5, line 1.6, `PREVIEW_MD_LANG_TRACKING_EM` 0.08); outside this ladder. |

**Tracking**: 0.05 em on uppercase section labels and badges (`settings.rs::GROUP_LABEL_TRACKING_EM`, `settings.rs::PROFILE_BADGE_TRACKING_EM`, `profiles.rs::SECTION_LABEL_TRACKING_EM`); 0.04 em on head titles (`theme.rs::HEAD_TITLE_TRACKING_EM`, `DOCK_PREVIEW_LETTER_SPACING_EM`).

**Line height**: a single-line label sits on a line box of about 1.2 (`restore.rs::TITLE_LINE_LOGICAL_PX` 18 for 15; `settings.rs::ROW_TITLE_LINE_LOGICAL_PX`, `ROW_DESC_LINE_LOGICAL_PX`, `GROUP_LABEL_LINE_LOGICAL_PX`; `restore.rs::BUTTON_LINE_LOGICAL_PX` 15.5 for 13). Running text in a float tag uses **1.4** (`CHROME_LINE_HEIGHT` in `toast.rs`, `tooltip.rs`, `peek_strip.rs`, `seats.rs`).

**Deviations**

- **T1** Pane head title: `theme.rs::SEAT_TITLE_FONT_LOGICAL_PX = 11.5 → 11`. Head titles are 11; the float window's head and the glance card's head already use `HEAD_TITLE_FONT_LOGICAL_PX` 11. *Visible on every pane, subtle (0.5 pt).* With T5 also done, 11.5 leaves the product.
- **T2** `settings.rs::HEADER_TITLE_FONT_LOGICAL_PX = 16 → 15` (dialog title). *Visible: the settings title is larger than every other dialog's.*
- **T3** 13.5 → 13 (primary): `settings.rs::ROW_TITLE_FONT_LOGICAL_PX`, `palette.rs::FIELD_FONT_LOGICAL_PX`, `git_panel.rs::GIT_HEAD_FONT_LOGICAL_PX`. *Visible on every settings row title (0.5 pt).*
- **T4** List-row labels → 13 (primary): `palette.rs::ROW_FONT_LOGICAL_PX` 12.5, `git_panel.rs::GIT_ROW_FONT_LOGICAL_PX` 12.5, `settings.rs::NAV_ITEM_FONT_LOGICAL_PX` 12.5, `git_graph.rs::GRAPH_BODY_FONT_LOGICAL_PX` 12. *Visible: the settings nav and the palette list sit below the menus beside them.*
- **T5** Other 11.5 → 11 (caption): `seats.rs::FILES_SEG_FONT_LOGICAL_PX`, `settings.rs::CAP_FONT_LOGICAL_PX`, `restore.rs::ROW_CWD_FONT_LOGICAL_PX`, `seats.rs::PREVIEW_CARD_DETAIL_FONT_LOGICAL_PX`, `websheet.rs::DETAIL_FONT_LOGICAL_PX`, `git_panel.rs::GIT_EMPTY_FONT_LOGICAL_PX`, `git_graph.rs::GRAPH_TOOL_FONT_LOGICAL_PX`. *Barely visible.*
- **T6** 10.5 → 10 (badge/pill): `git_panel.rs::GIT_HASH_FONT_LOGICAL_PX`, `git_panel.rs::GIT_PILL_FONT_LOGICAL_PX`, `git_graph.rs::GRAPH_REF_FONT_LOGICAL_PX`, `settings.rs::PROFILE_BADGE_FONT_LOGICAL_PX`, `peek_strip.rs::LEAF_FONT_LOGICAL_PX`. *Barely visible.*
- **T7** Section labels → 11 / 0.05 em / line 13: `git_panel.rs::GIT_LABEL_FONT_LOGICAL_PX` 9.5, `git_panel.rs::GIT_LABEL_TRACKING_EM` 0.09, `git_panel.rs::GIT_LABEL_LINE_LOGICAL_PX` 12; `profiles.rs::SECTION_LABEL_FONT_LOGICAL_PX` 10.5, `profiles.rs::SECTION_LABEL_LINE_LOGICAL_PX` 12.5; `theme.rs::RAIL_LABEL_TRACKING_EM` 0.04. *Visible for git: its labels are the smallest and most widely spaced text in the window.*
- **T8** `first_run.rs::TITLE_LINE_LOGICAL_PX = 21 → 18`. The same 15-pt dialog title sits on an 18 line in `restore.rs`. *Barely visible.*
- **T9** `search.rs::FIELD_FONT_LOGICAL_PX = 12 → 13` (field). *Visible in the search capsule, whose text grows.*
- **T10** Float-tag running text → 1.4: `tooltip.rs::PEEK_LINE_HEIGHT` 1.5, `websheet.rs::LINE_HEIGHT` 1.5. *Invisible on one line; slight when text wraps.*

---

## 6. Elevation

### 6.1 Shadow

**Rule:** a floating surface casts two black rings with a cubic falloff (`rounded_rect.rs::rounded_rect_shadow_coverage`). The reach is **3 pt** (`theme.rs::FLOAT_WINDOW_SHADOW_LOGICAL_PX`). That constant is passed by cardhint, first-run, float, keyhint, palette, peek strip, all ten menu builders in `profiles.rs`, restore, search, settings, seats, toast and tooltip.

The ring alphas come from `ChromePalette` per surface class. Each pair is transcribed from the mock-up's own `box-shadow` for that class, and dark is heavier than light by design ("The dark canvas needs nearly four times the light one", `theme.rs`):

| Field pair | Dark in/out | Light in/out | Used by |
|---|---|---|---|
| `menu_shadow_*_alpha` | 46/23 | 18/9 | menus, toast, web sheet |
| `menu_popup_shadow_*_alpha` | 46/23 | 46/23 | popup (combo) menus |
| `tip_shadow_*_alpha` | 115/57 | 26/13 | tooltip |
| `drag_ghost_shadow_*_alpha` | 64/32 | 64/32 | drag ghost |
| `float_shadow_*_alpha` | 128/64 | 51/26 | transient float |
| `float_pinned_shadow_*_alpha` | 148/74 | 61/31 | pinned float |
| `peek_card_shadow_*_alpha` | 128/64 | 46/23 | glance card |

Three other lifts are special cases with their own reasons:
- The flight shadow of a tab or pane in motion: `seats.rs::FLIGHT_SHADOW_SPREAD_LOGICAL_PX` 3, `FLIGHT_SHADOW_ALPHA` 0.18.
- The switch knob: `first_run.rs::SWITCH_KNOB_SHADOW_LOGICAL_PX` 3, `SWITCH_KNOB_SHADOW_ALPHA` 0.25.
- The rail's one-sided shade, which is a gradient and "not a box-shadow" by ruling (`theme.rs`, `RAIL_SHADE_WIDTH_LOGICAL_PX`).

**Deviations**

- **E1** `file_peek.rs::PEEK_SHADOW_LOGICAL_PX = 28 → 3`. *Clearly visible: the glance card is the only card with a wide, soft shadow, about nine times the reach of every other float.*
- **E2** `websheet.rs::SHADOW_SPREAD_LOGICAL_PX = 24 → 3` (a `u8`). *Visible on the web sheet; rare surface.*

### 6.2 Scrim

**Rule:** there is one scrim, `ChromePalette::modal_scrim` `[0x0f, 0x0f, 0x0f]` at `modal_scrim_alpha` 89 (0.35). It is the same on both canvases, by ruling ("a scrim is not a surface of either palette", stated in `theme.rs` on `modal_scrim` and held by its test).

- **A dialog that blocks the window lays it**: settings, first-run, the dirty gate / quit card, the web sheet.
- **A surface that leaves the window usable lays none**: the restore prompt draws "**no scrim**. The absence is the design" (`restore.rs`), because the terminal behind it stays live; the palette's box "has no scrim and that is deliberate — the world stays visible behind it" (`palette.rs`).

The single value shows much less on dark than on light. It is a standing ruling and is not a deviation.

### 6.3 Edge

**Rule:** every frame and seam is **1.0 pt**. Members:
- `theme.rs`: `FLOAT_WINDOW_BORDER_LOGICAL_PX`, `SEAT_TITLE_EDGE_LOGICAL_PX`, `SEAT_DIVIDER_VISUAL_LOGICAL_PX`, `RAIL_BORDER_LOGICAL_PX`, `FOCUS_CARD_BORDER_LOGICAL_PX`, `FOCUS_MINI_BORDER_LOGICAL_PX`, `DRAG_GHOST_BORDER_LOGICAL_PX`
- float tags and cards: `toast.rs::TOAST_BORDER_LOGICAL_PX`, `tooltip.rs::TIP_BORDER_LOGICAL_PX`, `keyhint.rs::KEY_HINT_BORDER_LOGICAL_PX`, `file_peek.rs::PEEK_BORDER_LOGICAL_PX`, `peek_strip.rs::PEEK_BORDER_LOGICAL_PX`, `peek_strip.rs::LEAF_BORDER_LOGICAL_PX`
- palette and search: `palette.rs::PALETTE_BORDER_LOGICAL_PX`, `palette.rs::FIELD_RULE_LOGICAL_PX`, `search.rs::CAPSULE_BORDER_LOGICAL_PX`, `search.rs::SEPARATOR_WIDTH_LOGICAL_PX`
- menus and settings: `profiles.rs::SEPARATOR_THICKNESS_LOGICAL_PX`, `profiles.rs::PICKER_EDGE_LOGICAL_PX`, `settings.rs::MENU_SEPARATOR_HEIGHT_LOGICAL_PX`
- other: `seats.rs::PREVIEW_CARD_BUTTON_BORDER_LOGICAL_PX`, `git_panel.rs::GIT_PILL_EDGE_LOGICAL_PX`, `git_graph.rs::GRAPH_REF_EDGE_LOGICAL_PX`, `video_seat.rs::BAR_EDGE_LOGICAL_PX`

These are not edges and are outside the rule:
- A **focus ring** is 2 (§10.10).
- A **drop-target outline** is 1.5: `seats.rs::TAB_LAND_RING_LOGICAL_PX`, `theme.rs::DOCK_PREVIEW_BORDER_LOGICAL_PX`. Both are transient accent feedback.
- **Progress rings** are 2: `theme.rs::WINDOW_TAB_RING_STROKE_LOGICAL_PX`, `WINDOW_TAB_STATUS_DOT_RING_STROKE_LOGICAL_PX`.
- **Graph strokes** are drawings. The git panel draws its graph on a 14-pt lane (`git_panel.rs::GIT_GRAPH_WIDTH_LOGICAL_PX`) and the graph page on a 16-pt lane (`git_graph.rs::GRAPH_LANE_WIDTH_LOGICAL_PX`). Dot and stroke keep the same ratio to the lane (3.1/14 ≈ 3.6/16; 1.5/14 ≈ 1.7/16), so the two are one drawing at two sizes, not two values.
- The **page spinner** is 1.4: `main.rs::PAGE_SPINNER_STROKE_LOGICAL_PX`.

There are no edge deviations.

---

## 7. Motion (`docs/DESIGN.md` §7.18 motion tokens, §7.19 overlay enter/exit)

The existing system is the rule, unchanged:

- **Three spans**, archived as `MOTION_ARCHIVE_MS`:
  - `motion.rs::MOTION_FAST_MS` 90: opacity and colour on chrome that is not the subject.
  - `MOTION_BASE_MS` 140: one interaction, something appearing or going away.
  - `MOTION_SLOW_MS` 200: motion that moves the layout.
- **Three curves**:
  - `EASE` (0.25, 0.1, 0.25, 1).
  - `EASE_IN_OUT` (0.42, 0, 0.58, 1): breathing and the waiting halo.
  - `GRAB_EASE` (0.2, 0, 0, 1): anything a hand is holding or has just let go of.

  The §7.18 heading in `docs/DESIGN.md` says two curves; it predates `GRAB_EASE`, and the code's three are the rule.
- **One travel**: `MOTION_TRAVEL_LOGICAL_PX` 4, always away from the summoner (`Travel::away_from`). Four directions, no diagonals, and nothing travels on exit.
- **Intent delays** are a separate named family, not spans:
  - `tooltip.rs::TOOLTIP_DELAY` 380 (ruled in `docs/UI-UX.md` §二)
  - `float.rs::FLOAT_OPEN_INTENT_DELAY` 180
  - `keyhint.rs::KEY_HINT_DELAY` 800
  - the rest of the register
- **The gate**: `main.rs` test `every_duration_in_this_window_is_an_archived_span_a_stated_exemption_or_a_named_wait`. Six exemptions are periods or holds with written reasons.
- **Drag autoscroll**: `motion.rs::DRAG_AUTOSCROLL_EDGE_LOGICAL_PX` 24, one band for every list.

There are no motion deviations.

---

## 8. Icons (`docs/DESIGN.md` §7.18 icons)

**Rule:** every mark is a vector body on a 16-unit grid (`marks.rs::SYMBOL_BODY`), drawn with pen `PROFILE_LINE_STROKE_UNITS` 1.2. An optical gate holds the drawn pen to `icons.rs::OPTICAL_STROKE_BAND_LOGICAL_PX` [0.95, 1.15] px. Verbs map to shapes through `icons.rs::ActionIcon` (92 entries) with a reverse-index gate.

A filled mark is allowed only with a written reason, recorded in the `icons.rs` test list `FILLED_WITH_A_REASON`:
- `i-folder` / `i-folder-open` (identity)
- `i-gear` (the Material gear, kept by ruling 裁5, 2026-08-26)
- `i-tri` (the disclosure triangle in the tree and on submenu rows, ruling 裁6)
- `i-play`

**Drawn sizes:**

| Role | Size | Members |
|---|---|---|
| Menu and toolbar slot | **14** | `icons.rs::MENU_HOUSE_BOX_LOGICAL_PX`, `TOOLBAR_HOUSE_BOX_LOGICAL_PX`; `palette.rs::ROW_ICON_LOGICAL_PX`, `toast.rs::TOAST_MARK_LOGICAL_PX`, `restore.rs::ROW_MARK_LOGICAL_PX`, `seats.rs::WINDOW_PANEL_TOGGLE_GLYPH_LOGICAL_PX`, `seats.rs::PANE_HEAD_FLOAT_GLYPH_LOGICAL_PX`, `theme.rs::WINDOW_CAPTION_GEAR_GLYPH_LOGICAL_PX` |
| Compact head slot | **13** | `icons.rs::COMPACT_HEAD_HOUSE_BOX_LOGICAL_PX`; `seats.rs::PREVIEW_TOOL_GLYPH_LOGICAL_PX`, `PANE_HEAD_FILES_GLYPH_LOGICAL_PX`, `PANE_ZOOM_MARK_LOGICAL_PX`, `FILES_FOOT_MARK_LOGICAL_PX`, `WINDOW_TAB_PIN_GLYPH_LOGICAL_PX`; `theme.rs::WINDOW_NEW_TAB_GLYPH_LOGICAL_PX`; `float.rs::FLOAT_HEAD_MARK_LOGICAL_PX`, `FLOAT_DOCK_GLYPH_LOGICAL_PX`; `git_graph.rs::GRAPH_REFRESH_MARK_LOGICAL_PX` |
| Caption slot (window buttons; small close marks) | **10** | `icons.rs::CAPTION_EDGE_TO_EDGE_BOX_LOGICAL_PX`; `theme.rs::WINDOW_CAPTION_GLYPH_LOGICAL_PX`, `notice.rs::CLOSE_GLYPH_LOGICAL_PX`, `search.rs::BUTTON_GLYPH_LOGICAL_PX`, `git_graph.rs::GRAPH_TOOL_MARK_LOGICAL_PX`, `git_panel.rs::GIT_REMOTES_MARK_LOGICAL_PX` |
| Identity mark (profile / file / folder beside a name) | **15** | `theme.rs::WINDOW_TAB_MARK_LOGICAL_PX`, `theme.rs::PANE_HEAD_PROFILE_MARK_LOGICAL_PX`, `seats.rs::FILES_ROW_ICON_LOGICAL_PX`, `file_peek.rs::PEEK_MARK_LOGICAL_PX`, `settings.rs::PROFILE_MARK_LOGICAL_PX` |
| Close × inside a 17 box | **8** | `theme.rs::WINDOW_TAB_CLOSE_GLYPH_LOGICAL_PX`, `theme.rs::SEAT_PANE_CLOSE_GLYPH_LOGICAL_PX` |
| Card hero mark | **30** | `seats.rs::PREVIEW_CARD_ICON_LOGICAL_PX`, `websheet.rs::MARK_LOGICAL_PX` |

The pane head draws its folder at 13 (`PANE_HEAD_FOLDER_MARK_LOGICAL_PX`) and its file at 14 (`PANE_HEAD_FILE_MARK_LOGICAL_PX`) beside the 15 profile mark. The three sizes are held apart deliberately to balance optical weight and are not deviations. `first_run.rs::MARK_LOGICAL_PX` 22 is the app-mark tile, the only one of its kind.

A drop-down opener wears the vector `⌄` mark: the new-tab `⌄`, the pane head `⌄`, the files root chevron and the preview's `Open ⌄`.

**Deviations**

- **I1** `settings.rs::COMBO_CHEVRON = "\u{25bc}"` at `settings.rs::COMBO_CHEVRON_FONT_LOGICAL_PX = 8.5` → the vector `⌄` mark in the 10 slot. It is a solid text triangle outside the mark system and is not on `FILLED_WITH_A_REASON`; its comment records only that the mock-up used the character. *Visible: it appears on every settings combo, and it is the only solid glyph besides the ruled gear and tree triangle.*
- **I2** Sizes off the ladder → nearest slot: `git_panel.rs::GIT_ACT_GLYPH_LOGICAL_PX` 11 → 10, `peek_strip.rs::LIST_MARK_LOGICAL_PX` 11 → 10, `seats.rs::WINDOW_TAB_SPEAKER_GLYPH_LOGICAL_PX` 12 → 13, `video_seat.rs::BAR_MARK_LOGICAL_PX` 16 → 14, `git_graph.rs::GRAPH_REF_TAG_MARK_LOGICAL_PX` 9 → 10, `peek_strip.rs::LEAF_MARK_LOGICAL_PX` 9 → 10. *Invisible to barely visible (1 pt).*

---

## 9. Colour and tone

**Rule:** a scheme is a 22-key JSON file (`assets/schemes/folio-light.json`, `folio-dark.json`). Only five keys are not ANSI: `background`, `foreground`, `cursorColor`, `selectionBackground` and `accent`. The accent is `#3059d8` on light and `#7a99ff` on dark (cobalt, by the 2026-08-10 ruling in `docs/UI-UX.md` §二).

Every chrome colour is derived at compile time in `theme.rs::ChromePalette` (164 fields) as `ink_over(canvas, ink, ‰)` from named sources (`PANEL_DARK #252525`, `MENU_DARK #2A2A2A`, `LIGHT_INK_SOURCE #37352F`). The only run-time adjustment is the Oklab lift to a 4.5 floor (`contrast::raise_against`, `theme.rs::CHROME_TEXT_MINIMUM_CONTRAST`).

Tone roles follow `docs/UI-UX.md` §二 and §三:
- Panel grey (`#F7F7F5` / `#252525`) is chrome; the window and terminal ground is content.
- The focused pane is not filled. "You are here" is shown by a contrast step (ink3 → ink, plus weight) and by `PANE_MARK_UNFOCUSED_OPACITY` 0.5 on the other panes.
- Semantic red (error) and amber (waiting) are not accent.
- The first-run switch track is accent when on (§10.1); that is the one persistent state the accent marks.

**Opacity roles the product repeats:**

| Role | Value | Members |
|---|---|---|
| Disabled / hidden / dead mark | **0.35** | `settings.rs::UNAVAILABLE_MARK_OPACITY`, `profiles.rs::UNAVAILABLE_MARK_OPACITY`, `settings.rs::PROFILE_HIDDEN_MARK_OPACITY`, `theme.rs::WINDOW_TAB_DEAD_MARK_OPACITY` |
| Unfocused pane's identity mark | **0.5** | `seats.rs::PANE_MARK_UNFOCUSED_OPACITY` (ruled, `docs/UI-UX.md` §二) |
| Hover-revealed control at rest | **0.6** | `seats.rs::TAB_FILES_TRIGGER_REVEAL`, `seats.rs::FILES_ROOT_CHEVRON_OPACITY` |

**Deviations**

- **C1** `seats.rs::PANE_HEAD_TRIGGER_REVEAL = 0.7 → 0.6`. *Invisible.*

---

## 10. Controls

Each control's metrics come from §2–§5, so this section names the values and points to the deviation IDs above.

**10.1 Boolean control.** Folio has two, each in its own place (ruled 2026-09-22):
- **Settings uses a combo** reading `On` / `Off` for every boolean. `settings.rs::SettingsControl` has `Combo`, `Slider`, `Field`, `Chord` and `FieldPair`, and no switch. A two-state row and a many-state row look the same, so the page has one control grammar.
- **The first-run card uses switches**: `first_run.rs::SWITCH_WIDTH_LOGICAL_PX` 30 × `SWITCH_HEIGHT_LOGICAL_PX` 18, `SWITCH_KNOB_INSET_LOGICAL_PX` 2, knob shadow `SWITCH_KNOB_SHADOW_LOGICAL_PX` 3 at `SWITCH_KNOB_SHADOW_ALPHA` 0.25, with an accent track when on.

A switch appears nowhere else, and Settings does not gain one.

**10.2 Combo.** 27.5 tall (`COMBO_HEIGHT_LOGICAL_PX`), r6, minimum width 118 (`COMBO_MIN_WIDTH_LOGICAL_PX`), text 13, value-to-chevron gap 10 (`COMBO_GAP_LOGICAL_PX`). Its drop-down is a menu (10.4). Deviations: I1 (chevron glyph), H1 (drop-down row height).

**10.3 Button.** 27.5 tall (14 × 6 padding on a 15.5 line), r6, text 13, buttons 8 apart. The recommended answer is accent-filled (`BUTTON_PRIMARY_HOVER_BRIGHTNESS` 1.07 on hover), except in a dialog with a destructive answer, which has no accent button (§1.2). Inline verbs in tags and cards are r6 with text 12. Deviations: R6, R7.

**10.4 Menu row.** The popup is r8 with padding 4, a 1-pt edge, `menu_shadow` (or `menu_popup_shadow` for popups) at 3-pt reach, and travels 4 from its anchor. Rows are 29.5 tall, r5, text inset 10, icon column 14 (`profiles.rs::ITEM_ICON_COLUMN_LOGICAL_PX`, `settings.rs::OPTION_ICON_COLUMN_LOGICAL_PX`), icon-to-label gap 8, text 13. Separators are 1 pt. Deviations: H1, G2.

**10.5 Tab.**
- Windows: 34 attached; r7 top corners with a bottom skirt; leading 12 / trailing 6; mark 15 with an 8 gap; title 13; count badge 15 tall, r4, text 10; close box 17, r4, 8 glyph; width 46–200 (`WINDOW_TAB_MIN_WIDTH_LOGICAL_PX`, `WINDOW_TAB_MAX_WIDTH_LOGICAL_PX`; equal widths by ruling, `docs/UI-UX.md` §四).
- Mac: 30 floating pill, r7 on all four corners (`docs/DESIGN.md` §13.48).
- Vertical rail: 220 wide (46 parked); rows 30, r6.

No deviations.

**10.6 Pane head.** 30 tall with a 1-pt bottom edge; leading 12 / trailing 6; profile mark 15 with an 8 gap; title 11 at 0.04 tracking; heavier weight when focused; no fill (`docs/UI-UX.md` §二). The `⌄` trigger is revealed on hover. The pane under the head is square at rest (§2). Deviations: T1 (title size), G3 (gap); H3 and C1 (trigger box and reveal).

**10.7 Dialog.** r10 via `FLOAT_WINDOW_RADIUS_LOGICAL_PX`, 1-pt edge, 3-pt shadow, padding 20 / 22 / 16, title 15, scrim when it blocks the window (§6.2). Widths: settings ≤ 720 × 600, first-run ≤ 440, restore ≤ 400, web sheet ≤ 420. Deviations: R2, S2, S3, E2 (web sheet); T2 (settings title); H4, T8 (first-run).

**10.8 Card / float tag** (tooltip, toast, key hint, card hint, peek card, peek strip; one palette function by `docs/DESIGN.md` §7.28). r8, 1-pt edge, 3-pt reach, padding 12 × 10 when multi-line or 10 × 5 when single-line, body 12 on a 1.4 line, head title 11 with an 8 icon gap. Deviations: R1, S1 (tooltip); E1, G1 (peek card); T10.

**10.9 Toast.** 360 wide (minimum 200), r8, padding 12 × 10, mark 14 with an 8 gap, title 12.5, body 12, action r6 at 12. Deviations: R7, R10, H7.

**10.10 Focus ring.** 2 pt wide, offset 1, r6 (`settings.rs::FOCUS_RING_WIDTH_LOGICAL_PX`, `FOCUS_RING_OFFSET_LOGICAL_PX`, `FOCUS_RING_RADIUS_LOGICAL_PX`; `first_run.rs::FOCUS_RING_WIDTH_LOGICAL_PX`). It may be accent because a focus ring is transient (`docs/UI-UX.md` §二). Deviation:
- **F1** `seats.rs::FILES_ROW_FOCUS_RING_LOGICAL_PX = 1.5 → 2`. *Barely visible.*

**10.11 Capsule (search).** r8, 1-pt edge, padding 6 × 5, toggles and buttons 22 tall, toggle text 11. Deviations: R8 (button radius), T9 (field text).

**10.12 Palette.** 540 wide, r10, 1-pt edge, no scrim (§6.2). The field is 42 tall with a 1-pt rule. Rows are 30 tall, with a 16 icon column kept even when a row has no icon (`docs/DESIGN.md` §7.55 ⑦). Deviations: R3, T3, T4, G3.

**10.13 Files column and git page.** Segment bar 30; tree rows 24, r5, icon 15 with an 8 gap, indent 14; root button r5; foot bar with a 13 mark and 11 text. The git page (sections, rows, pills, graph) shares the column. Deviations: G1, H2, H3, H5, R5, R11, S4, S8, T4, T5, T6, T7, I2.

**10.14 Float window.** A popped-out pane: r10, 3-pt reach, head with a 13 mark, 8 icon gap and title 11. Deviations: G1, S5, R10, H3.

---

## 11. Platforms

Windows and macOS share the same constants. The only differences are these, and all are ruled:

- **Windows:** caption slots are 46 × 40 and square (`WINDOW_CAPTION_BUTTON_LOGICAL_PX`; Fluent, `docs/UI-UX.md` §1.1). The gear sits left of the caption buttons in the same metric.
- **macOS:**
  - Traffic lights are centred at (20, 20) with pitch 13 / 36 / 59.
  - The gear is mirrored 20 from the trailing edge (`theme.rs::WINDOW_CAPTION_GEAR_INSET_LOGICAL_PX`).
  - Tabs are 30-pt floating pills (`docs/DESIGN.md` §13.48).
  - The menu bar is a native `NSMenu` (`menubar.rs`).

`seats.rs::MAC_TITLE_BAR_LOGICAL_PX` is a test fixture for a measured native title bar, not a design value.

Every deviation in this spec is in shared code, so each applies to both platforms. None is platform-specific.

---

## 12. Not covered

These are layout and vocabulary choices, not values, and this spec does not rule on them:
- the two preview head grammars (crumbs + `Open ⌄` vs centred address + `↗`)
- the `Files` / `Git` word tabs
- whether the files foot bar exists
- per-type file icons
