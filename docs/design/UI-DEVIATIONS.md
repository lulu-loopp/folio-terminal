# Folio UI: deviations from the spec

Read off `main` at `b031cfd2`, 2026-09-22. Companion to `UI-SPEC.md` in this directory; the IDs match its sections. This is the work list for 0.4.4 and 0.4.5.

Every row is a place where a surface does not use its rule's value. Bringing a row into line changes a constant's value to the rule's; it adds no component and changes no layout.

**How to read it**
- **Ranking:** first by **visibility**, then by how many constants share the wrong value.
- **Visibility classes:**
  - **A**: a user notices it in normal use.
  - **B**: noticeable side by side, or on a rare surface.
  - **C**: barely visible (≤ 1 pt, or seen only mid-gesture).
  - **D**: invisible (tidying only).
- **Files:** every constant below is under `crates/bt-app/src/`, except `theme.rs`, which is under `crates/bt-render/src/`. No line numbers are given, because the files move.
- **Platform:** every deviation is in code shared by the Windows and macOS builds, so every row applies to **both**.
- **Rulings of 2026-09-22 folded in:** the icon-to-label gap is 8 everywhere (G1–G3); every head title is 11 (T1). The terminal pane's resting radius (0) and the two boolean controls (combo in Settings, switch on the first-run card) are rules as they stand and produce no rows.

**Totals: 29 deviations, touching 47 constants.** A 0 · B 9 · C 10 · D 10.

---

## Class A: visible in normal use

| # | ID | Surface | Constant(s) | Now | Rule | Why this rule |
|---|---|---|---|---|---|---|

## Class B: visible side by side, or on a rare surface

| # | ID | Surface | Constant(s) | Now | Rule | Why this rule |
|---|---|---|---|---|---|---|
| 14 | G1 | Icon-to-label gap: files row, files foot, float head, focus-card head | `seats.rs::FILES_ROW_GAP_LOGICAL_PX`, `seats.rs::FILES_FOOT_GAP_LOGICAL_PX`, `float.rs::FLOAT_HEAD_GAP_LOGICAL_PX`, `theme.rs::FOCUS_CARD_HEAD_GAP_LOGICAL_PX` | 6 | 8 | One icon-to-label gap everywhere (ruled 2026-09-22); the tab, toast, git rows, restore rows and settings menu items already use 8. The files column and the small heads loosen by 2 pt. |
| 15 | G2 | Icon-to-label gap in every menu `profiles.rs` builds | `profiles.rs::ITEM_GAP_LOGICAL_PX` | 10 | 8 | Same ruling. Every right-click and `⌄` menu's text moves 2 pt toward its icon; `settings.rs::ITEM_GAP_LOGICAL_PX` is already 8. |
| 17 | S2 | Web sheet ("download not replayed") padding | `websheet.rs::PADDING_LOGICAL_PX` | 22 on all sides | 20 top / 22 sides / 16 bottom | Dialog padding (`restore.rs::DIALOG_PADDING_*`, `first_run.rs::PADDING_*`). |
| 18 | E2 | Web sheet shadow | `websheet.rs::SHADOW_SPREAD_LOGICAL_PX` | 24 (u8) | 3 | `theme.rs::FLOAT_WINDOW_SHADOW_LOGICAL_PX`. |
| 19 | R2 | Web sheet radius | `websheet.rs::RADIUS_LOGICAL_PX` | 8 | 10 | Dialog (`theme.rs::FLOAT_WINDOW_RADIUS_LOGICAL_PX`). The web sheet is scrimmed and modal. |
| 20 | T9 | Search capsule field text | `search.rs::FIELD_FONT_LOGICAL_PX` | 12 | 13 | Field (`settings.rs::FIELD_FONT_LOGICAL_PX`). |
| 21 | H6 | Video bar | `video_seat.rs::VIDEO_BAR_HEIGHT_LOGICAL_PX` | 34 | 30 | Strip (`theme.rs::SEAT_TITLE_BAR_LOGICAL_PX`, `notice.rs::BAR_HEIGHT_LOGICAL_PX`, `seats.rs::FILES_SEG_BAR_LOGICAL_PX`). |
| 23 | S3 | First-run card to window edge | `first_run.rs::SURFACE_MARGIN_LOGICAL_PX` | 34 | 24 | Centred overlay (`palette.rs::PALETTE_EDGE_MARGIN_LOGICAL_PX`, `websheet.rs::MARGIN_LOGICAL_PX`). Seen in small windows. |
| 24 | H5 | Files column foot bar | `seats.rs::FILES_FOOT_BAR_LOGICAL_PX` | 28 | 30 | Strip. |

## Class C: barely visible

| # | ID | Surface | Constant(s) | Now | Rule | Why this rule |
|---|---|---|---|---|---|---|
| 25 | T5 | Other 11.5-pt captions: files `Files`/`Git` words, restore row path, no-preview card detail, web sheet detail | `seats.rs::FILES_SEG_FONT_LOGICAL_PX`, `restore.rs::ROW_CWD_FONT_LOGICAL_PX`, `seats.rs::PREVIEW_CARD_DETAIL_FONT_LOGICAL_PX`, `websheet.rs::DETAIL_FONT_LOGICAL_PX` | 11.5 | 11 | Caption (17 constants at 11). With T1, 11.5 leaves the product. |
| 26 | I2 | Icon sizes off the ladder | `peek_strip.rs::LIST_MARK_LOGICAL_PX`, `seats.rs::WINDOW_TAB_SPEAKER_GLYPH_LOGICAL_PX`, `video_seat.rs::BAR_MARK_LOGICAL_PX`, `peek_strip.rs::LEAF_MARK_LOGICAL_PX` | 11, 12, 16, 9 | 10, 13, 14, 10 | Nearest `icons.rs::MarkSlot` size (14 / 13 / 10). |
| 27 | T6 | 10.5-pt badges, pills and hashes | `peek_strip.rs::LEAF_FONT_LOGICAL_PX` | 10.5 | 10 | Badge (`theme.rs::WINDOW_TAB_BADGE_FONT_LOGICAL_PX` and 5 more). |
| 28 | G3 | Icon-to-label gap: pane head, palette dot, palette row | `theme.rs::SEAT_TITLE_GAP_LOGICAL_PX`, `palette.rs::DOT_GAP_LOGICAL_PX`, `palette.rs::ROW_GAP_LOGICAL_PX` | 7, 7, 9 | 8 | One icon-to-label gap everywhere (ruled 2026-09-22). 1 pt each; the pane head's is on every pane. |
| 29 | S8 | Row text inset: restore list | `restore.rs::ROW_PADDING_X_LOGICAL_PX` | 8 | 10 | `profiles.rs::ITEM_PADDING_X_LOGICAL_PX`, `settings.rs::ITEM_PADDING_X_LOGICAL_PX`, `palette.rs::ROW_PADDING_X_LOGICAL_PX`, `theme.rs::RAIL_TAB_PADDING_LEFT_LOGICAL_PX`. |
| 30 | S5 | Float window head padding | `float.rs::FLOAT_HEAD_PADDING_LEFT_LOGICAL_PX`, `float.rs::FLOAT_HEAD_PADDING_RIGHT_LOGICAL_PX` | 10 · 5 | 12 · 6 | Strip / pane head (`theme.rs::SEAT_TITLE_PADDING_LOGICAL_PX`, `SEAT_TITLE_TRAILING_PADDING_LOGICAL_PX`). |
| 31 | R3 | Command palette rows | `palette.rs::ROW_RADIUS_LOGICAL_PX` | 7 | 6 | List row. The `restore.rs` constant with the same name is 6. |
| 33 | T8 | First-run title line box | `first_run.rs::TITLE_LINE_LOGICAL_PX` | 21 | 18 | The same 15-pt title sits on `restore.rs::TITLE_LINE_LOGICAL_PX` 18. |
| 34 | F1 | Files-row keyboard focus ring | `seats.rs::FILES_ROW_FOCUS_RING_LOGICAL_PX` | 1.5 | 2 | `settings.rs::FOCUS_RING_WIDTH_LOGICAL_PX`, `first_run.rs::FOCUS_RING_WIDTH_LOGICAL_PX`. |
| 37 | S7 | Video bar side padding | `video_seat.rs::BAR_PADDING_X_LOGICAL_PX` | 10 | 12 | Strip leading. |

## Class D: invisible (tidying only)

| # | ID | Surface | Constant(s) | Now | Rule | Why this rule |
|---|---|---|---|---|---|---|
| 38 | R8 | 22-pt boxes: search buttons, notice close, web sheet close, pane ghost | `search.rs::BUTTON_RADIUS_LOGICAL_PX`, `notice.rs::CLOSE_RADIUS_LOGICAL_PX`, `websheet.rs::CLOSE_RADIUS_LOGICAL_PX`, `seats.rs::PANE_GHOST_RADIUS_LOGICAL_PX` | 6 | 5 | Tool box radius (`seats.rs::PREVIEW_TOOL_RADIUS_LOGICAL_PX` and 11 more). |
| 39 | R13 | Pill constants written larger than they draw | `seats.rs::PREVIEW_COUNT_RADIUS_LOGICAL_PX` | 8 | h/2 (7) | Pill. They already draw round, so this is spelling only. |
| 40 | S6 | Notice strip padding | `notice.rs::PADDING_LEFT_LOGICAL_PX`, `notice.rs::PADDING_RIGHT_LOGICAL_PX` | 11 · 8 | 12 · 6 | Strip. |
| 41 | R10 | Toast close box; float head close box | `toast.rs::TOAST_CLOSE_RADIUS_LOGICAL_PX`, `float.rs::FLOAT_BUTTON_RADIUS_LOGICAL_PX` (as used for the 17-pt `float.rs::FLOAT_CLOSE_BOX_LOGICAL_PX`) | 5 | 4 | Close box in a head (`theme.rs::WINDOW_TAB_CLOSE_RADIUS_LOGICAL_PX`, `SEAT_PANE_CLOSE_RADIUS_LOGICAL_PX`). **Ticket note:** `FLOAT_BUTTON_RADIUS_LOGICAL_PX` also rounds the float's other head buttons; split the close box off rather than change the shared constant. |
| 42 | H7 | Close boxes 18 / 16 | `toast.rs::TOAST_CLOSE_LOGICAL_PX`, `theme.rs::FOCUS_CARD_CLOSE_BOX_LOGICAL_PX` | 18, 16 | 17 | `theme.rs::WINDOW_TAB_CLOSE_BOX_LOGICAL_PX`, `SEAT_PANE_CLOSE_BOX_LOGICAL_PX`, `float.rs::FLOAT_CLOSE_BOX_LOGICAL_PX`. |
| 43 | T10 | Float-tag line height | `websheet.rs::LINE_HEIGHT` | 1.5 | 1.4 | `CHROME_LINE_HEIGHT` in `toast.rs`, `tooltip.rs`, `peek_strip.rs`, `seats.rs`. Slight when text wraps. |
| 44 | R6 | Notice verb button | `notice.rs::VERB_RADIUS_LOGICAL_PX` | 5 | 6 | Button. The `websheet.rs` constant with the same name is 6. |
| 45 | R7 | Toast action button | `toast.rs::TOAST_ACTION_RADIUS_LOGICAL_PX` | 5 | 6 | Button. |
| 48 | R12 | Web preview address field | `seats.rs::PREVIEW_ADDRESS_RADIUS_LOGICAL_PX` | 5 | 4 | 20-pt chip, like `seats.rs::PREVIEW_CRUMB_RADIUS_LOGICAL_PX`. |
| 49 | C1 | Pane head `⌄` reveal opacity | `seats.rs::PANE_HEAD_TRIGGER_REVEAL` | 0.7 | 0.6 | Hover-revealed control (`seats.rs::TAB_FILES_TRIGGER_REVEAL`, `seats.rs::FILES_ROOT_CHEVRON_OPACITY`). |

---

## Ticket groups (by file, so one change touches one module)

Each group is one ticket. A row whose constants live in several files is split across the groups that own them; the IDs below say which part.

- **Float-tag family** (`tooltip.rs`, `file_peek.rs`, the drag ghost in `theme.rs`): E1, S1, R1 fixed in ticket 16; remaining members fixed in ticket 23 (listed below).
- **Settings** (`settings.rs`): I1, T2, H1 and the Settings members of T3/T4 fixed in ticket 17; S10, R9 and the Settings members of T5/T6/S8 fixed in ticket 24. No Settings members remain open.
- **First-run** (`first_run.rs`): H4, S3, T8.
- **Git panel + graph** (`git_panel.rs`, `git_graph.rs`): class-A members fixed in ticket 18; remaining members fixed in ticket 25 (listed below).
- **Web sheet** (`websheet.rs`): S2, E2, R2; the `websheet.rs` members of T5, R8, T10.
- **Pane head, files column, float window** (`seats.rs`, `theme.rs`, `float.rs`): T1, H3, H5, S5, F1, C1, R12; G1 `FILES_ROW_GAP_LOGICAL_PX`, `FILES_FOOT_GAP_LOGICAL_PX`, `FLOAT_HEAD_GAP_LOGICAL_PX`, `FOCUS_CARD_HEAD_GAP_LOGICAL_PX`; G3 `SEAT_TITLE_GAP_LOGICAL_PX`; the `seats.rs` members of T5 and I2; T7's `RAIL_LABEL_TRACKING_EM`.
- **Menus** (`profiles.rs`): G2; T7's `profiles.rs` members.
- **Palette** (`palette.rs`): R3; the `palette.rs` members of T3 and T4; G3 `DOT_GAP_LOGICAL_PX`, `ROW_GAP_LOGICAL_PX`.
- **Everything else** (`notice.rs`, `toast.rs`, `search.rs`, `video_seat.rs`, `peek_strip.rs`, `restore.rs`, and the members of the rows above that no group names): T9, H6, S7, S6, R6, R7, R8, R10, H7; the leftover members of I2, T5, T6 and S8.

## Fixed in ticket 23

- Row 14 / G1: `file_peek.rs::PEEK_HEAD_GAP_LOGICAL_PX` fixed at 8; other members remain open.
- Row 28 / G3: `theme.rs::DRAG_GHOST_GAP_LOGICAL_PX` fixed at 8; other members remain open.
- Row 35 / S9: `theme.rs::DRAG_GHOST_PADDING_X_LOGICAL_PX` fixed at 10; row closed.
- Row 43 / T10: `tooltip.rs::PEEK_LINE_HEIGHT` fixed at 1.4; the web-sheet member remains open.

## Fixed in ticket 24

- Row 25 / T5: `settings.rs::CAP_FONT_LOGICAL_PX` fixed at 11; other members remain open.
- Row 27 / T6: `settings.rs::PROFILE_BADGE_FONT_LOGICAL_PX` fixed at 10; other members remain open.
- Row 29 / S8: `settings.rs::NAV_ITEM_PADDING_LEFT_LOGICAL_PX` fixed at 10; other members remain open.
- Row 36 / S10: `settings.rs::CONTENT_PADDING_BOTTOM_LOGICAL_PX` fixed at 16; row closed.
- Row 46 / R9: `settings.rs::MENU_ACT_RADIUS_LOGICAL_PX` fixed at 5; row closed.

## Fixed in ticket 25

- Row 16 / S4: `git_panel.rs::GIT_LABEL_PADDING_TOP_LOGICAL_PX` fixed at 10; row closed.
- Row 22 / R5: `git_panel.rs::GIT_SECTION_RADIUS_LOGICAL_PX` fixed at 8; row closed.
- Row 32 / R4: `git_graph.rs::GRAPH_ROW_RADIUS_LOGICAL_PX` fixed at 6; row closed.
- Row 47 / R11: `git_panel.rs::GIT_BADGE_RADIUS_LOGICAL_PX` fixed at 4; row closed.
- Row 14 / G1: `git_panel.rs::FILES_BADGE_GAP_LOGICAL_PX` fixed at 8; other members remain open.
- Row 25 / T5: `git_panel.rs::GIT_EMPTY_FONT_LOGICAL_PX` and `git_graph.rs::GRAPH_TOOL_FONT_LOGICAL_PX` fixed at 11; other members remain open.
- Row 26 / I2: `git_panel.rs::GIT_ACT_GLYPH_LOGICAL_PX` and `git_graph.rs::GRAPH_REF_TAG_MARK_LOGICAL_PX` fixed at 10; other members remain open.
- Row 27 / T6: `git_panel.rs::GIT_HASH_FONT_LOGICAL_PX`, `git_panel.rs::GIT_PILL_FONT_LOGICAL_PX` and `git_graph.rs::GRAPH_REF_FONT_LOGICAL_PX` fixed at 10; other members remain open.
- Row 28 / G3: `git_graph.rs::GRAPH_ROW_GAP_LOGICAL_PX` fixed at 8; other members remain open.
- Row 29 / S8: `git_panel.rs::GIT_ROW_PADDING_X_LOGICAL_PX` and `git_graph.rs::GRAPH_ROW_PADDING_X_LOGICAL_PX` fixed at 10; other members remain open.
- Row 39 / R13: `git_panel.rs::GIT_PILL_RADIUS_LOGICAL_PX` and `git_graph.rs::GRAPH_REF_RADIUS_LOGICAL_PX` fixed at half their respective height constants (8); the preview-count member remains open.
