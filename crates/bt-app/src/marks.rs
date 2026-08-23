//! The chrome's marks, taken from `design/ui-mockup.html` and rasterized.
//!
//! The mock-up draws every glyph in the window's own chrome as SVG: the caption
//! buttons are `<symbol id="i-gear|i-min|i-max|i-close">`, a tab and a terminal
//! pane head wear the session's profile mark (`#p-pwsh`), a preview head wears
//! `#i-file` and a files head `#i-folder`, both in `--accent`. Their bodies are
//! reproduced here verbatim, which is the whole point: a mark drawn twice — once
//! in the design and once in Rust — is two marks that drift, and the second one
//! is the one nobody looks at.
//!
//! The active tab's *shape* is here for the same reason and one more. CSS gives
//! it `border-radius: var(--tabr) var(--tabr) 0 0` plus two `::before`/`::after`
//! skirt corners filled with `--termbg`, and a browser renders all four with
//! analytic coverage. Approximating that with nested rectangles produces a
//! staircase, so the silhouette is emitted as one closed path and handed to the
//! same rasterizer, which antialiases it the way the design's own renderer does.
//!
//! Nothing in this module decides *where* a mark goes — `seats.rs` does, from the
//! solver's rectangles. This module answers only "what does it look like, at this
//! many physical pixels, in this colour".

use std::{collections::HashMap, fmt::Write as _, sync::Arc};

use bt_render::{ChromeIcon, ChromeLabel, OverlayQuad};

/// Which corner of a card [`ChromeMark::CardCorner`] fills the floor around.
///
/// Named for the corner of the *card*, not for the direction the bite faces:
/// the caller places it at the card's top-left and asks for `TopLeft`, and the
/// mark works out which way to curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// The eight colours a profile of the user's own can be struck in.
///
/// **Eight fixed hexes and not a colour picker.** §7.1.6c-4a already ruled that
/// this dialog has no colour picker and is not going to grow one, and a 15px
/// identity mark is the case that ruling was written for: the mark's whole job
/// is to be recognised across a strip at a glance, and a continuous colour space
/// is exactly how somebody ends up with two blues they cannot tell apart.
///
/// They are **struck values and not scheme colours** (mock-up line 2148): a mark
/// carries its own colours and does not turn over with the theme, so each of
/// these was measured to hold its silhouette on both grounds — the same
/// constraint that made `#p-cmd` charcoal rather than console black.
///
/// The list is `design/ui-mockup.html`'s own `data-combo="pfcolour"`, in its
/// order. **It is not the plan's list**: plan §1.5 wrote `Orange` where the
/// mock-up struck `Magenta`, and the mock-up is the contract — it landed after
/// the plan and its eight were measured on both grounds, while an orange one
/// place along from Ubuntu's own would have been a ninth profile wearing a
/// brand's colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MarkColour {
    Blue,
    Teal,
    Green,
    Amber,
    Red,
    Magenta,
    Violet,
    Slate,
}

impl MarkColour {
    /// Every colour, in the combo's own order.
    pub const ALL: [Self; 8] = [
        Self::Blue,
        Self::Teal,
        Self::Green,
        Self::Amber,
        Self::Red,
        Self::Magenta,
        Self::Violet,
        Self::Slate,
    ];

    /// What the mark is filled with — the mock-up's own hex, to the digit.
    #[must_use]
    pub fn hex(self) -> [u8; 3] {
        match self {
            Self::Blue => [0x2f, 0x6f, 0xb5],
            Self::Teal => [0x1f, 0x7a, 0x6b],
            Self::Green => [0x3f, 0x7d, 0x3a],
            Self::Amber => [0xb0, 0x7a, 0x16],
            Self::Red => [0xa8, 0x3a, 0x3a],
            Self::Magenta => [0xa0, 0x3a, 0x78],
            Self::Violet => [0x6b, 0x4e, 0x9e],
            Self::Slate => [0x4a, 0x55, 0x68],
        }
    }

    /// The word `profiles.json` writes.
    ///
    /// Lower case and English, on `mark_from_file`'s own ruling one module over:
    /// this file is read by people, and a wire word is not a Rust spelling.
    #[must_use]
    pub fn wire(self) -> &'static str {
        match self {
            Self::Blue => "blue",
            Self::Teal => "teal",
            Self::Green => "green",
            Self::Amber => "amber",
            Self::Red => "red",
            Self::Magenta => "magenta",
            Self::Violet => "violet",
            Self::Slate => "slate",
        }
    }

    /// One of [`Self::wire`]'s words, or `None` for anything else — a colour
    /// this build does not know is a key the reader skips, not a file it
    /// refuses.
    #[must_use]
    pub fn from_wire(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|colour| colour.wire() == word)
    }
}

/// One mark the chrome can wear. Every variant except [`ChromeMark::ActiveTab`]
/// is a `<symbol>` lifted straight out of the mock-up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromeMark {
    /// `#i-gear` — the settings caption button.
    Gear,
    /// `#i-min`.
    WindowMinimize,
    /// `#i-max`.
    WindowMaximize,
    /// `#i-close`.
    WindowClose,
    /// The same `#i-close` source, at the tab control's smaller 8px size.
    TabClose,
    /// The same `#i-close` again, at the pane head's 8px
    /// (`.panehead .pane-close svg`, mock-up 1657).
    ///
    /// A third name for one drawing, on the precedent [`Self::WindowClose`] and
    /// [`Self::TabClose`] already set: they too share a symbol and differ only
    /// in the box they are asked for. What makes them separate variants is that
    /// `mark_key` keys the raster cache on [`ChromeMark::id`], so a control that
    /// might one day be re-struck at another size — or given its own stroke
    /// weight, which an 8px `×` inside a 17px box on a *caption* rather than on
    /// a strip could well want — has a slot of its own to be re-struck in.
    PaneClose,
    /// `#i-plus` — the new-tab button.
    Plus,
    /// `#i-chev` — the profile picker beside the `+`, at some angle through its
    /// turn.
    ///
    /// The mock-up rotates the one arrow rather than swapping glyphs, and it
    /// rotates it *continuously*: `.chevbtn svg { transition: transform 140ms
    /// cubic-bezier(.2,0,0,1) }` with `.chevbtn.open svg { transform:
    /// rotate(180deg) }` (mock-up 415-420). There is one `#i-chev` in the
    /// symbol sheet and there is no second one to swap to — the arrow points
    /// down at a list that is folded away, up at one that is on screen, and the
    /// *turn between them* is the sentence. Cutting that to two end frames
    /// throws away the sentence and keeps the punctuation.
    ///
    /// `turned_degrees` is therefore an angle and not a state: 0 is the resting
    /// arrow, 180 is the turned one, and every value between is a real frame of
    /// the transition. It is whole degrees quantized to
    /// [`CHEVRON_TURN_QUANTUM_DEGREES`] rather than a float because this is
    /// half of the raster cache's key, which has to hash and compare exactly —
    /// the same reason [`Self::ProgressRing`] counts milliturns. Build one
    /// through [`ChromeMark::chevron`] rather than by hand, which is what
    /// applies the quantization.
    Chevron { turned_degrees: u16 },
    /// `#i-folder-open` — a directory row that is open (C34).
    ///
    /// A second glyph rather than a state of [`Self::Folder`], because unlike
    /// the chevron beside it this really is a second drawing: the mock-up's open
    /// folder is a different silhouette with a different number of paths, and no
    /// rotation of the closed one produces it.
    FolderOpen,
    /// `#i-tri` — the disclosure triangle at some angle through its turn (C33).
    ///
    /// An angle and not a boolean for the reason [`Self::Chevron`] gives at
    /// length: `.frow .tri { transition: transform 120ms }` means every value
    /// between the two ends is a frame someone can see. Quantized on the same
    /// [`CHEVRON_TURN_QUANTUM_DEGREES`] so the two rotating glyphs cannot drift
    /// into two different ideas of how fine an angle is worth a raster.
    ///
    /// Unlike the chevron it needs no raster bleed: the triangle's farthest
    /// vertex is 3.33 units from the centre of a 10-unit box, so the swept
    /// circle clears the edges at every angle.
    TreeDisclosure { turned_degrees: u16 },
    /// `#p-pwsh` — the PowerShell profile mark, which carries its own colours.
    /// The mock-up is explicit that a mark's colour is its own and the active
    /// tab does not recolour it (`.pmark`, and the comment above it).
    ProfilePowerShell,
    /// `#p-ubuntu` — the WSL profile's mark: Ubuntu's Circle of Friends.
    ///
    /// The orientation is a hard constraint the mock-up states in as many words
    /// (lines 2181-2188): ring and friends are concentric on (8,8), the three
    /// friends sit exactly on `r=4.1`, and **one friend points up**, which makes
    /// the mark mirror-symmetric about the vertical. The earlier one-left-two-
    /// right arrangement was geometrically centred and read as right-heavy — the
    /// eye mistook it for a ring that was not centred, which is a worse failure
    /// than being off-centre, because the thing it accuses is the wrong part.
    ///
    /// It is also the one round profile mark, and that is a live cross-block
    /// tension rather than a detail: the tab's progress ring (Tab block D36) is
    /// demonstrated on the *square* PowerShell mark on purpose, because an arc
    /// drawn round a circular logo reads as part of the logo. See
    /// `docs/PROBLEM-LIST.md` and the profiles inventory's F56 — the ring's
    /// legibility over this mark is owed a look the day both are on screen.
    ProfileUbuntu,
    /// `#p-git` — the Git Bash profile's mark: Git's orange lozenge with the
    /// branch gesture and its three nodes.
    ProfileGit,
    /// `#p-cmd` — the Command Prompt profile's mark.
    ///
    /// Charcoal with a lighter edge, and **not** true console black: the mock-up
    /// (lines 2205-2208) rules that a `#1E1E1E` panel on the dark theme's
    /// `#1B1B1B` ground disappears, leaving a chevron floating with no window
    /// around it. A mark has to hold its silhouette on both grounds, so it may
    /// not be the same colour as either — which is why this is `#3A3A3A` inside
    /// a `#606060` hairline and why nobody may "fix" it back to black.
    ProfileCmd,
    /// `#p-shell` — the chassis a profile of the user's own wears, in one of
    /// [`MarkColour`]'s eight.
    ///
    /// `#p-pwsh` and `#p-cmd` are already the same drawing twice — one rounded
    /// panel, one chevron, one underline — differing only in what they are
    /// filled with. So this is that drawing a third time, and it reads as "a
    /// shell of your own" rather than as a brand nobody owns: a profile drawn
    /// from nothing has no logo to inherit, and inventing one for it would be
    /// this list asserting a provenance the row does not have. A profile
    /// *duplicated* from a built-in is the other case and keeps the built-in's
    /// own mark, because a copy of a PowerShell really is a PowerShell.
    ///
    /// **No letter in it.** The house rule is that a profile icon is a MARK and
    /// not a character — "认出 PowerShell 靠的是那块蓝,不是读字形" — and an
    /// initial in a box is exactly the thing that rule refuses. The mock-up
    /// states it at `#p-shell` in as many words, so that nobody adds one later
    /// on the argument that a nameless panel is anonymous: what tells two of
    /// these apart is the colour, which is what the eight are for.
    ///
    /// The panel takes the colour as a **literal hex** and the chevron stays
    /// white, which is why this variant's body is generated rather than quoted
    /// from `SYMBOL_BODY`: a constant cannot carry a parameter, and running the
    /// fill through `currentColor` would have handed the mark to the theme —
    /// see [`Self::takes_current_color`], which a profile mark answers `false`.
    ProfileGeneric { colour: MarkColour },
    /// `#i-file`.
    File,
    /// `#i-globe` — **the web class's own mark** (Web 预览块 W2 片③, 片④).
    ///
    /// What a page wears wherever it is drawn small: the switcher row, the
    /// Recent row, the restore prompt's row, the page's own head and the strip
    /// row above it. `docs/DESIGN.md` §7.7 ⑤ puts it in the *same* box as
    /// `#i-file` so that the two kinds of row line up down one left edge and
    /// neither reads as the guest — which is why it is asked for at the file
    /// mark's own size and not at one of its own. The rows reach it through
    /// [`preview_row_mark`] and the head and strip through `seats::pane_mark`,
    /// so there is one glyph and a named place it is chosen.
    ///
    /// **The site's own favicon is not this mark and is not drawn yet.** §7.7 ②
    /// asks for the favicon where a site has one and this globe where it has
    /// not, and the two halves are not the same kind of work: every variant of
    /// this enum is a vector symbol struck from a path list and cached by name,
    /// and a favicon is a raster the engine fetches at runtime — a second
    /// pipeline into the chrome atlas that no mark has. It is slice ⑥'s, and is
    /// recorded as an open item rather than faked: a coloured square with a
    /// letter in it would be this window inventing an icon for somebody else's
    /// site.
    Globe,
    /// `#i-folder`.
    Folder,
    /// `#i-panel` — a pane whose kind this build cannot name.
    Panel,
    /// `#i-copy` — "put this row's path on the clipboard" (K143).
    ///
    /// Two overlapping rounded rectangles, the back one drawn as an open corner
    /// rather than a whole outline: the idiom reads as *one thing now standing
    /// in two places*, which is what a copy is, and the mock-up draws it that
    /// way at line 2162.
    Copy,
    /// `#i-paste` — "put this row's path into the shell's input line" (K143).
    ///
    /// A clipboard with a filled tab and three ruled lines. It is the *paste*
    /// glyph and not a second copy glyph because the verb it labels is the one
    /// that moves text into somewhere — the same distinction the two words keep
    /// everywhere else, and the reason the mock-up carries both (line 2239).
    Paste,
    /// `#i-split` — "cut this pane in two" (user ruling, 2026-08-15), the
    /// `.pane-split` button on a terminal head and the mark on both Split rows
    /// of the pane-head menu.
    ///
    /// The 田: the house frame with a divider on each axis. Struck for this
    /// product rather than lifted from anyone's sheet, and drawn as *division
    /// itself* rather than as one particular cut, because the button does not
    /// commit to an axis — it takes the pane's longer side, which is a different
    /// axis in a tall pane than in a wide one. A glyph showing one vertical rule
    /// would be a promise the button breaks half the time.
    ///
    /// Deliberately not [`Self::Panel`]'s picture, which is the same frame with
    /// *one off-centre* rule: that one reads as a sidebar beside a body, which
    /// is a statement about what a pane contains. Both rules, both centred, is a
    /// statement about a pane being cut, and at a pane head's 13 pixels the two
    /// have to be told apart at a glance.
    Split,
    /// `#i-split-right` — the pane-head menu's `Split right` row.
    ///
    /// Two panes side by side with a gutter between them: the *result*, not the
    /// cut. Drawn this way rather than as [`Self::Split`]'s frame with a single
    /// centred rule for the reason that variant's own note gives — a frame with
    /// one internal rule is already [`Self::Panel`], and three marks in one
    /// build that differ by where a line falls are three marks nobody can tell
    /// apart at a menu row's fourteen pixels.
    SplitRight,
    /// `#i-split-down` — the pane-head menu's `Split down` row.
    ///
    /// [`Self::SplitRight`] turned a quarter: two panes stacked, same gutter.
    /// The pair has to be a pair — same rects, same gap, same weight — because
    /// the only thing a reader is being asked to see is which way they lie.
    SplitDown,
    /// `#i-float` — "pop this column out into a floating window" (B18/B19), the
    /// `.pane-float` button on a files head.
    ///
    /// A rounded square with an arrow leaving it through the top right: the
    /// standard "opens outside this frame" idiom, and the mock-up's own drawing
    /// of it (line 2236).
    Float,
    /// `#i-external` — "hand this content to the program the machine keeps for
    /// it" (user ruling 2026-08-20), the `.pv-tool.pv-browser` on a page's
    /// preview head.
    ///
    /// **A bare arrow and not [`Self::Float`]'s framed one**, because the two
    /// stand next to each other in that head and say different things: the frame
    /// says *this pane* leaves the tree, the bare arrow says *this content*
    /// leaves the window. Two framed arrows side by side would be one drawing
    /// asked to mean both, at thirteen pixels, in the same run.
    ///
    /// It is `#i-float`'s own arrow — same 1.2 stroke, same round caps, same
    /// forty-five degrees — struck at the size a glyph with nothing around it has
    /// to be to hold the box the framed one fills.
    External,
    /// `#i-dock-left` — "put this floating window back on the left as a pane"
    /// (B20), the `DOCK` button in a float's header.
    ///
    /// **Side-honest**, and the mock-up says so at line 2233: the filled panel
    /// sits on the side the pane will land on. Files dock left; previews dock
    /// right, which is why the symbol sheet carries a mirrored twin.
    DockLeft,
    /// `#i-dock-right` — the mirrored twin, and the preview float's own DOCK
    /// (P53/P54).
    ///
    /// The day the note above was waiting for. Same window, same panel, same
    /// opacity; the filled bar sits at `x = 9.4` instead of `1.6`, because a
    /// preview docks into the far-right root column and an icon that pointed left
    /// while the pane appeared on the right was a user-reported mismatch — which
    /// is why "side-honest" is written into the drawing rather than left to the
    /// caption beside it.
    DockRight,
    /// `#i-check` — the tick that confirms a folder was revealed in the OS file
    /// manager (B24).
    ///
    /// It stands in the open folder's slot for 1300ms and then hands it back. A
    /// separate glyph rather than a state of that one, because it says something
    /// about an *action* and not about the thing the icon names.
    Check,
    /// `.float-win .fly-resize::after` — the corner chevron that says a pinned
    /// float can be resized (G79).
    ///
    /// Not a symbol in the mock-up's sheet: it is a CSS `::after` with a right
    /// and a bottom border and a `border-bottom-right-radius`, which at this
    /// size is one quarter-arc. It is drawn here as that arc rather than as two
    /// straight rules meeting at a corner, because the radius is the whole point
    /// — the mock-up's note (line 710-712) is that the grip's curve "nests
    /// concentrically inside the window's 10px corner (10 − ~3px inset ≈ 7px)",
    /// and two straight rules have no curve to nest.
    ResizeGrip,
    /// `#i-pin` / `#i-pinned` — one pin at one angle, whose state rides on the
    /// fill (mock-up lines 2074-2111).
    ///
    /// The angle is not a choice and it is not the state: 45°, head upper-right,
    /// is Microsoft's house angle, unbroken from Segoe MDL2 through Segoe
    /// Fluent, and Windows 11 deleted the old horizontal-vs-diagonal pairing by
    /// making `Pin` and `Pinned` pixel-identical. What is left to carry the
    /// state is the fill axis, per Fluent 2: regular is the *action* ("you could
    /// pin this"), filled is the *state* ("it is pinned"). Hence one `d`, two
    /// paint attributes, and no slashed "pin off" glyph — `Pin Off` names the
    /// unpin action, never a resting state.
    ///
    /// Two variants rather than one flag read at draw time because `mark_key`
    /// keys on `ChromeMark::id`: sharing an id would share a cache slot, and
    /// the second pin on screen would silently wear the first one's pixels.
    Pin { filled: bool },
    /// `#i-lock` / `#i-locked` — the preview pane's own control (§7.7 ⑧,
    /// Claude 定 2026-08-23), and the reason it is not a second pin.
    ///
    /// **A pin in this window means one thing, said twice.** A tab's pin keeps
    /// that tab across a launch and refuses to close before it is taken off; a
    /// row's pin keeps that file, folder or page at the top of a list. Both are
    /// *membership*: this one stays in the list. The preview pane's control is
    /// not about membership at all — it says "do not reuse this pane", and the
    /// list it is not in is the reader's own. One drawing for both was the
    /// finding W1 filed and left open: same glyph, same word, two hundred pixels
    /// apart on the same head, meaning two different things.
    ///
    /// **A padlock, because that is the idiom for "held as it is".** The state
    /// rides on TWO axes at once and deliberately so, unlike [`Self::Pin`],
    /// whose angle Windows 11 fixed and whose state therefore had only the fill
    /// left: the shackle opens or closes AND the body goes from outline to
    /// fill. Fluent 2's reading of the fill axis is unchanged — regular is the
    /// action ("you could lock this"), filled is the state ("it is locked") —
    /// and the shackle says the same thing a second time, at fourteen pixels,
    /// where one axis is thin evidence.
    ///
    /// **The body is the same rectangle in both**, so the two rasters differ
    /// only where the meaning does. Two variants and not one flag read at draw
    /// time, for [`Self::Pin`]'s reason: `mark_key` keys on [`Self::id`], and a
    /// shared id would hand the second lock on screen the first one's pixels.
    Lock { engaged: bool },
    /// `#i-save` — the preview header's explicit save (mock-up 2252, P27).
    ///
    /// The floppy, which is the one idiom in this whole interface that names a
    /// medium nobody has used in twenty years and is still the only drawing every
    /// user reads instantly. The mock-up keeps it and so does this.
    Save,
    /// `#i-eye` — "show me the rendered view", the markdown flip's *other* face
    /// (mock-up 2171, P28).
    ///
    /// The pair is one control with two states and two drawings, unlike the pin
    /// beside it: an eye and a pair of angle brackets say opposite things and
    /// neither is a fill of the other. Which one is shown is which view you are
    /// going *to*, not which one you are in — the mock-up's own title strings
    /// ("Rendered view" / "Edit source") say the destination.
    Eye,
    /// `#i-code` — "show me the source", the markdown flip's first face (mock-up
    /// 2170, P28).
    Code,
    /// The active tab's silhouette: `--tabr` top corners and the two outward
    /// skirt corners that join it to the content plane. `radius_px` is `--tabr`
    /// in physical pixels, so the shape is generated at the size it is drawn.
    ActiveTab { radius_px: u32 },
    /// A regular tab's rounded body, used only for the inactive hover fill.
    TabBody { radius_px: u32 },
    /// The same body as a ring drawn *inside* its own edge — CSS `box-shadow:
    /// inset 0 0 0 Npx`, which is what `@keyframes tab-land` puts on a tab
    /// coming to rest (mock-up 964).
    ///
    /// It is its own mark rather than two nested [`Self::TabBody`] fills for the
    /// reason every mark here exists: the hole has the same rounded corners the
    /// outline does, and cutting one shape out of another with opaque fills
    /// works only when the surface underneath is known — which for a translucent
    /// wash over a moving tab it is not.
    TabBodyRing { radius_px: u32, stroke_px: u32 },
    /// A title-bar control's hover pill: one round on all four corners, filled
    /// edge to edge. `.newtab` wears it at 6px and `.tab .close` at 4px.
    ///
    /// It is a mark rather than a [`bt_render::ChromeQuad`] because a quad is a
    /// rectangle and this one is not: drawn as a quad the `+`'s highlight came
    /// out a hard-edged grey block sitting against the tab's own round, which is
    /// what this variant exists to delete. Going through the rasterizer gets the
    /// same analytic coverage the active tab's corners already get, instead of a
    /// staircase assembled from nested rectangles.
    ///
    /// Opaque, like every other chrome fill: the palette pre-composites each
    /// pill over the surface it lands on, because this pipeline blends in linear
    /// light and the design's does not — see `ChromePalette::tab_close_pill_on_content`.
    ControlPill { radius_px: u32 },
    /// [`Self::ControlPill`] rounded on its **bottom two** corners only — the
    /// foot of a card, where a body meets the silhouette above it.
    ///
    /// §7.1.6b′ F2's `.fc-tab-mini` inside `.fcard { border-radius: 10px;
    /// overflow: hidden }`. The body has a different ground from the head above
    /// it (`--termbg` against `--hover`), so it is its own fill; and because it
    /// reaches the card's foot, that fill has to wear the card's own round or the
    /// two bottom corners square off underneath a silhouette that is curved.
    ///
    /// **Its own variant rather than the card's corners repainted in the panel's
    /// colour.** The panel under a card is a *ground* and carries the window's
    /// alpha, while every mark here is ink and is opaque — so painting the two
    /// corners back in `--panel` would put two opaque chips on a translucent
    /// window, visible against the desktop on exactly the windows that show it.
    /// A shape that is actually the shape is the same amount of code and cannot
    /// be wrong at 30% opacity.
    ControlPillFoot { radius_px: u32 },
    /// [`Self::ControlPill`] as a ring drawn *inside* its own edge — what
    /// [`Self::TabBodyRing`] is to [`Self::TabBody`], on the shape that is round
    /// on all four corners.
    ///
    /// It exists because `@keyframes tab-land`'s inset ring has to follow
    /// whatever silhouette the row it lands in actually wears, and the rail's is
    /// not the strip's: `.tab` is `border-radius: var(--tabr) var(--tabr) 0 0`
    /// and runs into the content plane, while `.vtab { border-radius: 6px }` is
    /// a closed box in a column. A landing ring borrowed from the strip closes
    /// across a foot the rail's rows do not have, and leaves two square corners
    /// under a fill that is round — which is a ring visibly not belonging to the
    /// thing it is drawn around.
    ControlPillRing { radius_px: u32, stroke_px: u32 },
    /// One corner of the floor showing around a rounded card: a `radius × radius`
    /// square with a quarter-disc bitten out of it, filled edge to edge.
    ///
    /// It exists for F63, `.slot.resizing .pane { margin: 5px; border-radius: 8px }`
    /// — the two panes a divider drag is resizing pull in from their own edges
    /// and the `--panel` floor shows through the gap. The gap's four straight
    /// sides are rectangles and go out as [`bt_render::ChromeQuad`]s; only the
    /// four corners are curved, and this is one of them.
    ///
    /// **Why four small marks rather than one frame-shaped one.** The obvious
    /// mark is the whole gap: the pane's rectangle with a rounded rectangle
    /// punched out. That mark's raster is keyed on the pane's size — and the
    /// pane's size is the one thing that changes on *every frame* of the gesture
    /// this shape only ever appears during. It would miss the cache every frame
    /// and rasterize a pane-sized SVG at pointer rate, which is the cost F69
    /// ("拖动期间只改 flexGrow") exists to keep off this path. A corner is
    /// `radius × radius` whatever the pane does, so all four are rasterized once
    /// and then hit the cache for the rest of the drag.
    ///
    /// The bite is subtracted with `fill-rule="evenodd"` over a square and a
    /// whole circle rather than drawn as an arc, so the shape does not depend on
    /// getting a sweep flag right in four different orientations.
    CardCorner { radius_px: u32, corner: Corner },
    /// A plain filled rectangle — the tab-name editor's caret and its selection
    /// band, and nothing else so far.
    ///
    /// [`Self::ControlPill`] exists because a quad is a rectangle and a pill is
    /// not; this exists because of the *other* half of that sentence. A
    /// [`bt_render::ChromeQuad`] is the right primitive for a rectangle but the
    /// wrong **layer** for this one: quads are drawn before every mark, and both
    /// of these have to land on top of [`Self::ActiveTab`] — the opaque
    /// silhouette the editing tab is wearing. A caret painted under the tab it
    /// is inside is a caret nobody sees.
    ///
    /// It cannot be a degenerate `ControlPill` either: `control_pill_path`
    /// refuses anything narrower than two pixels, and the caret is a hairline at
    /// 100% DPI by deliberate choice (`CURSOR_BAR_WIDTH_LOGICAL_PX`).
    Fill,
    /// One arc of the progress ring (`.pring`, mock-up lines 268-284).
    ///
    /// The mock-up's ring is a *pair* of concentric circles — a full-turn track
    /// under a partial arc, in two different colours — and a sprite carries one
    /// colour, so a ring on screen is two of these: a `sweep_milliturns: 1000`
    /// track and the arc over it, sharing a box. That is not a workaround but
    /// the better decomposition: every ring in the strip shares one cached track
    /// raster no matter what its arc is doing, and the arc is the only thing
    /// that has to be redrawn when the progress moves.
    ///
    /// Angles are thousandths of a turn clockwise from 12 o'clock. Turns rather
    /// than degrees because every angle this mark is ever given starts life as a
    /// fraction — a percentage, or a phase through a spin — and integers rather
    /// than a float because this is half of the raster cache's key, which has to
    /// hash and compare exactly.
    ProgressRing {
        start_milliturns: u16,
        sweep_milliturns: u16,
        /// `.pring circle { stroke-width: 2 }`, in physical pixels.
        stroke_px: u32,
    },
    /// The branch fork — **struck here, not lifted** (R4/R27, 2026-08-15).
    ///
    /// The first mark in this module with no `<symbol>` behind it, and the
    /// reason is that the mock-up has none either: its Git masthead spends a
    /// bare `⎇` (U+2387, ALTERNATE KEY SYMBOL) at line 4937, the only glyph in
    /// the whole design drawn as a character rather than as art. R4 struck that
    /// down and this is what replaces it. A codepoint is not a drawing: whether
    /// `⎇` exists at all is a fact about the font the machine happens to have,
    /// its width is another, and a masthead whose first mark is a hollow box on
    /// a machine without it is worse than no mark. What is drawn instead is the
    /// shape every git client draws — a trunk, a branch leaving it, three nodes
    /// — at the same 16-unit box and the same 1.15 stroke as `#i-file`, so it
    /// belongs to this sheet rather than merely standing beside it.
    ///
    /// One mark and not two: R27 asks the command palette's "Git panel" entry to
    /// wear a branch fork, and this *is* the branch fork. Striking a second one
    /// for the palette would be two drawings of one idea, which is the mistake
    /// this module's own header names.
    GitBranch,
    /// `#i-plus`'s other half — the horizontal stroke alone.
    ///
    /// The Git page's "unstage" verb, and the mock-up spells it `−` (U+2212 MINUS
    /// SIGN, deliberately not an ASCII hyphen, line 4927). It is a **mark** here
    /// and not a character for the reason R12 gives: the three verbs on a change
    /// row reveal through `.pv-tool`'s three rungs — absent, seven-tenths, whole
    /// — and an opacity is something a sprite has and a text run does not. Once
    /// one of the three is a mark all three must be, or the row's buttons would
    /// fade at different rates.
    ///
    /// Cut from `#i-plus`'s own path so the pair are one drawing minus a stroke:
    /// same ten-unit box, same 1.2 weight, same round cap. A minus struck
    /// independently would be a different length or a different weight from the
    /// plus sitting a row above it, which on two buttons that mean opposite
    /// things is exactly where it would show.
    Minus,
    /// **A tag** — the label somebody nailed on a commit (T7, v2 ③).
    ///
    /// Struck for this product: the mock-up's sheet has no tag, because the page
    /// it was drawn for had no tags on it. The silhouette is the one every tool
    /// in this space uses — a rectangle with one corner drawn out to a point,
    /// and a punched hole where the string goes — and it is drawn from geometry
    /// rather than reached for in a font, which is this module's whole law: a
    /// glyph is a fact about whichever typeface happened to be installed, and a
    /// mark has to hold its silhouette on both grounds at ten pixels.
    ///
    /// The house sixteen, at `#i-file`'s own 1.15 stroke, so it sits at the same
    /// optical weight as the marks it stands beside.
    Tag,
    /// **Read the repository again** (T5, v2 ③) — the graph toolbar's refresh.
    ///
    /// A circular arrow with a gap at the top and a solid head, which is the one
    /// drawing this idea has anywhere. Struck rather than found: this sheet had
    /// nothing circular-and-arrowed in it — [`Self::ProgressRing`] is an arc
    /// *without* a head and means "something is running", which is very nearly
    /// the opposite claim, and giving it a head would have made one drawing say
    /// both.
    ///
    /// The head is filled while the arc is stroked, and that is deliberate: an
    /// arrowhead outlined at this size is three hairlines meeting, which reads as
    /// a smudge rather than as a direction.
    Refresh,
    /// `#i-select` — **`Select all`**, on the terminal's own context menu
    /// (ticket #62, mock-up 2373).
    ///
    /// Four corner brackets with a rule through the middle: the brackets are the
    /// marquee a selection is drawn with everywhere, and the rule is the content
    /// they have been drawn around. It is deliberately not a filled rectangle —
    /// a solid block at fourteen pixels reads as a swatch, and the one thing
    /// this mark has to say is *the edges are what moved*.
    SelectAll,
    /// `#i-broom` — **`Clear screen`** (mock-up 2374).
    ///
    /// A broom, and the reason it is a broom rather than a second eraser is the
    /// ruling this pair of rows exists to make legible (§7.1.6): sweeping is
    /// what you do to a surface, and the surface goes on being there afterwards.
    /// The rows it sweeps scroll out into the transcript exactly as they always
    /// did, which is precisely the difference from the row underneath it.
    Broom,
    /// `#i-clear` — **`Clear scrollback…`** (mock-up 2371).
    ///
    /// An eraser on its rubbing line: the one drawing in the sheet that means
    /// *this is gone*, and it stands over the row that runs the full §3.1 ED3
    /// deletion. The broom above it sweeps a surface; this rubs a record out.
    Eraser,
    /// `#i-pencil` — **open this file** (user ruling 2026-08-19).
    ///
    /// The first of the two verbs a colour scheme of the reader's own reveals in
    /// the picker that lists it. A pencil and not a gear or an arrow, because
    /// what the press opens is a text file with the keyboard already in it: the
    /// mark says *you are about to write in this*, which is the one thing a
    /// reader has to know before they press it.
    ///
    /// It is drawn at twelve against the ×'s ten beside it, and that is one
    /// optical size rather than two: this glyph carries its ink inside a
    /// sixteen-unit box's margins, `#i-close` fills its ten corner to corner.
    Pencil,
    /// The mini graph's merge curve — a branch's history joining this line.
    ///
    /// The mock-up's own path (line 4933), in the mock-up's own `0 0 14 27` box,
    /// which is the whole reason this is a mark rather than arithmetic: the curve
    /// has to start at the *top right* of a 27-pixel row and land on the edge of
    /// a dot at its middle, and those two facts are stated once, in the design, as
    /// four control points. A rounded quad could not draw it and a re-derivation
    /// in Rust would be a second authority for a shape that already has one.
    GitMergeCurve,
    /// Three commits and the two edges between them — a DAG, in miniature.
    ///
    /// **Struck now and drawn later.** R27 asks for it beside the fork, and its
    /// caller is the command palette's "Git graph" entry, which arrives with the
    /// graph itself in G-4. It is here rather than there for the reason
    /// [`crate::git`]'s unused readers are: a pair of marks cut in one sitting
    /// from one geometry is a pair that matches, and a second one cut months
    /// later against a screenshot is not. The mock-up borrows `#i-app` for this
    /// entry today, which is a mark about applications and says nothing about a
    /// repository.
    #[allow(dead_code)]
    GitGraph,
    /// **One edge of the full graph** (G-4): a branch leaving a commit, or
    /// arriving at one.
    ///
    /// Generated rather than quoted, which is the whole difference between this
    /// and [`Self::GitMergeCurve`] beside it. The mini graph has exactly one
    /// curve — a side branch joining the single road from the top right — so the
    /// design could state it as four control points in a fixed 14×27 box. The
    /// full graph's edges reach across *any* number of lanes and in either
    /// direction, and a fixed path stretched into a box that is sometimes 16
    /// pixels wide and sometimes 112 is a stroke that is sometimes seven times
    /// too fat: `stroke-width` is in the path's own units, so a non-uniform
    /// scale is a non-uniform pen.
    ///
    /// So the shape is one quadratic and the box is the edge's own bounding
    /// rectangle: the curve leaves the dot **horizontally** and arrives at the
    /// lane **vertically**, which is the tangent profile the mock-up's two
    /// curves both have (§G G73/G74) and the reason a lane looks like a road
    /// that a branch merges onto rather than a wire soldered to it. The four
    /// cases — opening or closing, leftward or rightward — are the same drawing
    /// mirrored, so they are two booleans rather than four marks.
    ///
    /// The path is inset by half the stroke on every side and the caller pads
    /// the rectangle by the same, because a stroke centres on its path: written
    /// corner to corner it would lose half its width to the edge of its own
    /// raster at both ends, which reads as an edge that goes thin exactly where
    /// it meets the line it is joining.
    GraphCurve {
        stroke_px: u32,
        /// The dot is at the box's right rather than its left — the edge runs
        /// leftward.
        flip_x: bool,
        /// The dot is at the box's bottom rather than its top — the edge is
        /// arriving (a close) rather than leaving (an open).
        flip_y: bool,
    },
}

impl ChromeMark {
    fn id(self) -> &'static str {
        match self {
            Self::Gear => "i-gear",
            Self::WindowMinimize => "i-min",
            Self::WindowMaximize => "i-max",
            Self::WindowClose => "i-close",
            Self::TabClose => "tab-close",
            Self::PaneClose => "pane-close",
            Self::Plus => "i-plus",
            // One id for every angle: there is one arrow. What tells two frames
            // of the turn apart is the angle, which `mark_key` adds — see
            // `Self::ProgressRing`, whose id is likewise shared across sweeps.
            Self::Chevron { .. } => "i-chev",
            Self::ProfilePowerShell => "p-pwsh",
            Self::ProfileUbuntu => "p-ubuntu",
            Self::ProfileGit => "p-git",
            Self::ProfileCmd => "p-cmd",
            // One id for all eight colours, exactly as `Self::Chevron` has one
            // id for every angle: there is one chassis, and what tells two of
            // them apart is a parameter `mark_key` adds.
            Self::ProfileGeneric { .. } => "p-shell",
            Self::File => "i-file",
            Self::Globe => "i-globe",
            Self::Folder => "i-folder",
            Self::FolderOpen => "i-folder-open",
            // One id for every angle, on `Self::Chevron`'s precedent above.
            Self::TreeDisclosure { .. } => "i-tri",
            Self::Panel => "i-panel",
            Self::Copy => "i-copy",
            Self::Paste => "i-paste",
            Self::Split => "i-split",
            Self::SplitRight => "i-split-right",
            Self::SplitDown => "i-split-down",
            Self::Float => "i-float",
            Self::External => "i-external",
            Self::DockLeft => "i-dock-left",
            Self::DockRight => "i-dock-right",
            Self::ResizeGrip => "fly-resize",
            Self::Check => "i-check",
            Self::Pin { filled: false } => "i-pin",
            Self::Pin { filled: true } => "i-pinned",
            Self::Lock { engaged: false } => "i-lock",
            Self::Lock { engaged: true } => "i-locked",
            Self::Save => "i-save",
            Self::Eye => "i-eye",
            Self::Code => "i-code",
            Self::ActiveTab { .. } => "tab",
            Self::TabBody { .. } => "tab-body",
            Self::TabBodyRing { .. } => "tab-body-ring",
            Self::ControlPill { .. } => "control-pill",
            Self::ControlPillFoot { .. } => "control-pill-foot",
            Self::ControlPillRing { .. } => "control-pill-ring",
            // One id for four orientations, like the chevron's one id for every
            // angle: `mark_key` adds the corner, so the four rasters are four
            // cache slots under one name.
            Self::CardCorner { .. } => "card-corner",
            Self::Fill => "fill",
            Self::ProgressRing { .. } => "progress-ring",
            Self::GitBranch => "i-git-branch",
            Self::GitGraph => "i-git-graph",
            Self::Minus => "i-minus",
            Self::Tag => "i-tag",
            Self::Refresh => "i-refresh",
            Self::SelectAll => "i-select",
            Self::Broom => "i-broom",
            Self::Eraser => "i-clear",
            Self::Pencil => "i-pencil",
            Self::GitMergeCurve => "git-merge-curve",
            // One id for four mirrorings and every span, exactly as the chevron
            // has one id for every angle: `mark_key` adds the rest.
            Self::GraphCurve { .. } => "graph-curve",
        }
    }

    /// The chevron partway through its turn: `turn` is 0.0 at rest, 1.0 fully
    /// over, and anything between is a frame of `.chevbtn svg`'s transition.
    ///
    /// The angle is quantized here, at the one door every chevron comes
    /// through, because an angle is a cache key and a continuous one has no
    /// bottom: a 140ms turn on a 60Hz screen is nine frames, on a 360Hz screen
    /// fifty-four, and on the next screen more again. Quantizing bounds the
    /// work at [`CHEVRON_TURN_STEPS`] rasters for a whole sweep no matter how
    /// often it is sampled, and bounds it at the *same* number on every
    /// machine — which is what makes the cost of this animation a fact about
    /// the design rather than about the hardware it landed on.
    ///
    /// See [`CHEVRON_TURN_QUANTUM_DEGREES`] for why 5° is fine enough to be
    /// invisible.
    /// Written as "which step of the turn is this" rather than as "round this
    /// angle", because the two are the same arithmetic and only the first one
    /// cannot drift: the step count is the bound, so a bound stated anywhere
    /// but here would be a claim about the code instead of the code itself.
    #[must_use]
    pub fn chevron(turn: f32) -> Self {
        let steps = f32::from(CHEVRON_TURN_STEPS - 1);
        let step = (turn.clamp(0.0, 1.0) * steps).round() as u16;
        Self::Chevron {
            turned_degrees: step * CHEVRON_TURN_QUANTUM_DEGREES,
        }
    }

    /// Whether `color` reaches this mark at all. A profile mark paints itself.
    ///
    /// The house rule is the mock-up's own (line 2148, `Fixed colours, no theme
    /// swap`) and it is stated over the *class* rather than over PowerShell:
    /// "only the profile marks and the app icon carry their own colours; every
    /// other glyph is `currentColor` and follows the theme". Written as a match
    /// over the profile family rather than as `!= ProfilePowerShell`, because
    /// the second spelling is the same sentence only while there is one profile
    /// — and the day a second one arrives it silently recolours somebody's
    /// orange to the accent.
    fn takes_current_color(self) -> bool {
        !matches!(
            self,
            Self::ProfilePowerShell
                | Self::ProfileUbuntu
                | Self::ProfileGit
                | Self::ProfileCmd
                // The chassis is struck in one of eight fixed hexes and carries
                // it the way the other four carry theirs — the mark states its
                // own colour, which is precisely what this match is a list of.
                | Self::ProfileGeneric { .. }
        )
    }

    /// How many whole pixels of room this mark needs *outside* the box the
    /// layout gave it, as `(x, y)`.
    ///
    /// Every other mark in this module is drawn inside its own box, because
    /// every other mark holds still. The chevron turns, and a 9×6 arrow stood
    /// on end is 6×9: rasterized in its own box the middle of the turn would be
    /// sliced off at the top and bottom, and the animation would be a glyph
    /// growing stumps. So the *raster* gets a bigger box than the *layout* did,
    /// and the extra is bled symmetrically so the mark's centre — the point it
    /// turns about — does not move.
    ///
    /// Whole pixels, and symmetric, is what makes this free at the two ends of
    /// the turn: a box grown by an integer on each side maps the glyph to
    /// exactly the same surface pixels it landed on before, so the resting
    /// arrow is not re-aligned by having been given room it does not use. See
    /// [`chevron_raster`] for the arithmetic and
    /// `the_turning_box_leaves_the_resting_arrow_exactly_where_it_was` for the
    /// proof.
    fn raster_bleed(self, width_px: u32, height_px: u32) -> (u32, u32) {
        match self {
            Self::Chevron { .. } => {
                let geometry = chevron_raster(width_px, height_px);
                (geometry.pad_x_px, geometry.pad_y_px)
            }
            _ => (0, 0),
        }
    }

    /// The box the rasterizer draws into, which is the layout's box plus
    /// [`Self::raster_bleed`] on each side.
    fn raster_size(self, width_px: u32, height_px: u32) -> (u32, u32) {
        let (pad_x, pad_y) = self.raster_bleed(width_px, height_px);
        (width_px + 2 * pad_x, height_px + 2 * pad_y)
    }
}

/// `.chevbtn svg`'s turn, in degrees, rounded to this before it becomes a
/// cache key.
///
/// Five degrees is chosen against the one thing that can go wrong — the turn
/// reading as a series of steps instead of a sweep — which is a question about
/// how far the arrow's tip *moves* per step, not about the angle. The tip sits
/// about 5.0 symbol units from the centre it turns about, so 5° carries it
/// `5.0 * scale * 0.0873` physical pixels: 0.39px at 100%, 0.79px at 200%,
/// 1.18px at 300%. Under a pixel and change, at 60-plus frames a second, over
/// 140ms — the steps are below the grid the mark is drawn on.
///
/// It buys, in exchange, a hard ceiling: 37 distinct rasters for a whole sweep,
/// and — because `cubic-bezier(.2,0,0,1)` spends its long decelerating tail
/// crawling the last few degrees — a redraw that stops being asked for while
/// the arrow is still nominally moving but no longer visibly so.
pub const CHEVRON_TURN_QUANTUM_DEGREES: u16 = 5;

/// How many distinct angles a whole turn can quantize to, both ends included —
/// the upper bound on rasters one sweep can ask for.
pub const CHEVRON_TURN_STEPS: u16 = 180 / CHEVRON_TURN_QUANTUM_DEGREES + 1;

/// The centre of `#i-tri`'s ten-unit box, which CSS would have used by default.
const TREE_DISCLOSURE_TURN_CENTRE_UNITS: f32 = 5.0;

/// How far the disclosure triangle turns between shut and open: `rotate(90deg)`.
pub const TREE_DISCLOSURE_OPEN_DEGREES: u16 = 90;

/// The triangle at some fraction of its turn, quantized like the chevron.
///
/// `turn` is 0.0 shut, 1.0 open; values between are real frames of the 120ms
/// transition rather than a rounding of the two ends.
#[must_use]
pub fn tree_disclosure(turn: f32) -> ChromeMark {
    let degrees = f32::from(TREE_DISCLOSURE_OPEN_DEGREES) * turn.clamp(0.0, 1.0);
    let step = (degrees / f32::from(CHEVRON_TURN_QUANTUM_DEGREES)).round() as u16;
    ChromeMark::TreeDisclosure {
        turned_degrees: step * CHEVRON_TURN_QUANTUM_DEGREES,
    }
}

/// A mark, the physical box it fills, and the colour `currentColor` resolves to.
///
/// Deliberately plain data with no pixels in it: the chrome builder is a pure
/// function of the solver's rectangles, and it stays testable — and cheap to run
/// on every pointer move — because deciding *what* to draw never rasterizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChromeSprite {
    pub mark: ChromeMark,
    /// `[left, top, right, bottom]`, in physical pixels of the whole surface.
    pub rect: [f32; 4],
    pub color: [u8; 3],
    /// A uniform fade over the whole mark, `0.0 ..= 1.0`.
    ///
    /// The mock-up asks for this twice and both times the artwork is unchanged
    /// and only its presence varies: `.ticon.working` breathes between 1 and
    /// .28, and `.ticon-wrap.dead .ticon` holds .35. It therefore rides to the
    /// renderer beside the pixels rather than into them — see
    /// [`ChromeIcon::opacity`] for why that distinction is load-bearing.
    pub opacity: f32,
    /// `filter: grayscale(1)` — draw the mark with its hue removed.
    ///
    /// Unlike [`Self::opacity`] this *is* baked into the raster and keyed with
    /// it, because it changes each pixel's colour rather than its coverage, and
    /// because the mark it exists for is the profile mark, which carries
    /// colours of its own that no palette entry can stand in for.
    pub grayscale: bool,
}

impl ChromeSprite {
    /// A mark at full strength in its own colours — what all but the tab's own
    /// state channels want.
    pub fn new(mark: ChromeMark, rect: [f32; 4], color: [u8; 3]) -> Self {
        Self {
            mark,
            rect,
            color,
            opacity: 1.0,
            grayscale: false,
        }
    }

    /// The same mark, faded — `opacity` on the element, not a paler ink.
    ///
    /// The distinction is the whole point of the field: a paler ink is a
    /// different colour, and the marks that ask for this are the ones carrying
    /// colours of their own that no palette entry can restate.
    #[must_use]
    pub fn with_opacity(self, opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0),
            ..self
        }
    }
}

/// One stacking layer of the modal overlay as a builder leaves it: its fills, its
/// captions, and its marks still named rather than rasterized.
///
/// The unrasterized twin of [`bt_render::OverlayLayer`], and it exists for the
/// same reason: the overlay's three channels have a fixed order inside the render
/// pass, so a popup is only above a row it covers if the two are on different
/// layers. Anything that must cover something else goes on a later layer; being
/// pushed later into the same layer buys nothing across channels.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayLayer {
    /// The rectangles of this layer that **are** the window — see
    /// [`bt_render::OverlayGround`]. Drawn under the layer's fills, and empty for
    /// every layer but the rail's, which is the one floating level that is a
    /// panel of the window rather than a surface laid over it.
    pub grounds: Vec<bt_render::OverlayGround>,
    pub quads: Vec<OverlayQuad>,
    pub labels: Vec<ChromeLabel>,
    pub sprites: Vec<ChromeSprite>,
    /// The layer's own `opacity` — see [`bt_render::OverlayLayer::opacity`]. It
    /// rides through the rasterizer untouched: how faded a layer is has nothing
    /// to do with which marks it names.
    pub opacity: f32,
    /// A scrolled document inside this layer — see
    /// [`bt_render::OverlayLayer::body`]. The preview float is the one tenant
    /// that has one, and it rides here for the z-order's sake: the document has
    /// to be drawn above its own window's face, which is drawn a whole pass after
    /// the seat's documents are.
    pub body: Option<bt_render::PreviewBody>,
    /// Rasters this layer draws that are **not** marks: pixels that arrived from
    /// somewhere else already sized, with nothing to name and nothing to
    /// rasterize.
    ///
    /// A mark is an identity — "the file glyph, 15px, in the accent" — and
    /// [`ChromeMarkRasters`] exists to turn a fixed set of those into textures and
    /// keep them. A picture is not one of those: the glance card's thumbnail is
    /// one file's pixels resampled to one box, cached by whoever owns the file,
    /// and pushed through here at full size. So it skips the rasterizer and joins
    /// the same draw list on the other side, after the layer's marks — a picture
    /// is content, and the marks around it are chrome.
    pub images: Vec<bt_render::ChromeIcon>,
}

impl Default for OverlayLayer {
    /// An empty layer at full strength — CSS's own initial `opacity: 1`, for the
    /// reason spelled out on the renderer's twin.
    fn default() -> Self {
        Self {
            grounds: Vec::new(),
            quads: Vec::new(),
            labels: Vec::new(),
            sprites: Vec::new(),
            opacity: 1.0,
            body: None,
            images: Vec::new(),
        }
    }
}

impl OverlayLayer {
    /// Whether the layer draws nothing at all — an empty layer is not a layer,
    /// and handing one to the renderer would cost a text renderer and a pass
    /// through three channels to draw nothing. A layer faded out of existence is
    /// empty by the same argument.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        (self.grounds.is_empty()
            && self.quads.is_empty()
            && self.labels.is_empty()
            && self.sprites.is_empty()
            && self.images.is_empty())
            || self.opacity <= 0.0
    }
}

/// Rasterized marks, keyed by mark + physical size + colour.
///
/// The map is rebuilt from the sprites of each frame, so it holds exactly what
/// is on screen: a DPI change, a theme change or a window narrow enough to
/// shorten the tab retires the old rasters on the next chrome rebuild instead of
/// accumulating one entry per size ever seen.
#[derive(Default)]
pub struct ChromeMarkRasters {
    rasters: HashMap<String, Raster>,
}

#[derive(Clone)]
struct Raster {
    rgba: Arc<[u8]>,
    width_px: u32,
    height_px: u32,
}

impl ChromeMarkRasters {
    /// Rasterize whatever is not already in hand and return the draw list, in
    /// the order the sprites were requested — which is the order they paint.
    pub fn resolve(&mut self, sprites: &[ChromeSprite]) -> Vec<ChromeIcon> {
        let mut kept: HashMap<String, Raster> = HashMap::with_capacity(sprites.len());
        let icons = self.icons_for(sprites, &mut kept);
        self.rasters = kept;
        icons
    }

    /// The same for a whole overlay stack, layer by layer.
    ///
    /// One pass over every layer rather than one call each, because the map is
    /// trimmed to what the *frame* asked for: resolving layer by layer would let
    /// the popup's marks retire the dialog's on the way past, and every frame
    /// would rasterize both again.
    pub fn resolve_overlay(&mut self, layers: Vec<OverlayLayer>) -> Vec<bt_render::OverlayLayer> {
        let mut kept: HashMap<String, Raster> = HashMap::new();
        let resolved = layers
            .into_iter()
            .map(|layer| bt_render::OverlayLayer {
                grounds: layer.grounds,
                quads: layer.quads,
                labels: layer.labels,
                icons: {
                    // The layer's own marks first, then whatever pixels it
                    // brought with it: the icon channel paints in order, and a
                    // picture belongs over the ground its card drew for it.
                    let mut icons = self.icons_for(&layer.sprites, &mut kept);
                    icons.extend(layer.images);
                    icons
                },
                opacity: layer.opacity,
                body: layer.body,
            })
            .collect();
        self.rasters = kept;
        resolved
    }

    /// Rasterize what `kept` does not already hold, adding to it as it goes.
    fn icons_for(
        &self,
        sprites: &[ChromeSprite],
        kept: &mut HashMap<String, Raster>,
    ) -> Vec<ChromeIcon> {
        let mut icons = Vec::with_capacity(sprites.len());
        for sprite in sprites {
            let width_px = (sprite.rect[2] - sprite.rect[0]).round();
            let height_px = (sprite.rect[3] - sprite.rect[1]).round();
            if !(width_px >= 1.0 && height_px >= 1.0) {
                continue;
            }
            let (width_px, height_px) = (width_px as u32, height_px as u32);
            let key = mark_key(sprite, width_px, height_px);
            let raster = match kept.get(&key).or_else(|| self.rasters.get(&key)) {
                Some(raster) => raster.clone(),
                None => {
                    let Some(raster) = rasterize(sprite, width_px, height_px) else {
                        continue;
                    };
                    raster
                }
            };
            // A mark that needs room outside its own box is *drawn* over the
            // grown box, or the extra pixels would be squeezed back into the
            // layout's rectangle and the mark would come out scaled down. The
            // bleed is whole pixels on each side, so this moves nothing: it
            // states the same glyph over a wider window onto the same surface.
            let (pad_x, pad_y) = sprite.mark.raster_bleed(width_px, height_px);
            let [left, top, right, bottom] = sprite.rect;
            let (pad_x, pad_y) = (pad_x as f32, pad_y as f32);
            icons.push(ChromeIcon {
                key: key.clone(),
                rect: [left - pad_x, top - pad_y, right + pad_x, bottom + pad_y],
                rgba: Arc::clone(&raster.rgba),
                width_px: raster.width_px,
                height_px: raster.height_px,
                opacity: sprite.opacity,
                // A mark is laid out to fit the control it belongs to, so there
                // is nothing for it to be cropped by — see
                // [`bt_render::ChromeIcon::clip`], whose one caller is a picture.
                clip: None,
            });
            kept.insert(key, raster);
        }
        icons
    }
}

fn mark_key(sprite: &ChromeSprite, width_px: u32, height_px: u32) -> String {
    let [r, g, b] = sprite.color;
    let mut key = format!("chrome-mark:{}", sprite.mark.id());
    if let ChromeMark::ActiveTab { radius_px } | ChromeMark::TabBody { radius_px } = sprite.mark {
        let _ = write!(key, ":r{radius_px}");
    }
    if let ChromeMark::ControlPill { radius_px } | ChromeMark::ControlPillFoot { radius_px } =
        sprite.mark
    {
        let _ = write!(key, ":r{radius_px}");
    }
    // The colour is the only thing that tells one chassis from another — one
    // id, one box, eight different fills. It cannot ride in on `sprite.color`'s
    // hex below, because a profile mark does not take the caller's colour at
    // all (`takes_current_color`), so the three bytes there are the same for
    // all eight and would collapse them into one cache slot.
    if let ChromeMark::ProfileGeneric { colour } = sprite.mark {
        let _ = write!(key, ":{}", colour.wire());
    }
    // The corner is the only thing that tells one card corner from another —
    // one id, one box, one colour, four different bites.
    if let ChromeMark::CardCorner { radius_px, corner } = sprite.mark {
        let _ = write!(key, ":r{radius_px}{corner:?}");
    }
    if let ChromeMark::TabBodyRing {
        radius_px,
        stroke_px,
    }
    | ChromeMark::ControlPillRing {
        radius_px,
        stroke_px,
    } = sprite.mark
    {
        let _ = write!(key, ":r{radius_px}w{stroke_px}");
    }
    if let ChromeMark::ProgressRing {
        start_milliturns,
        sweep_milliturns,
        stroke_px,
    } = sprite.mark
    {
        let _ = write!(key, ":a{start_milliturns}+{sweep_milliturns}w{stroke_px}");
    }
    // The span is in the box, so what is left to say is which way the drawing
    // is turned over and how fat the pen is.
    if let ChromeMark::GraphCurve {
        stroke_px,
        flip_x,
        flip_y,
    } = sprite.mark
    {
        let _ = write!(key, ":w{stroke_px}{}{}", u8::from(flip_x), u8::from(flip_y));
    }
    // The angle is the only thing that tells one frame of the turn from
    // another: one id, one box, one colour, and different pixels.
    if let ChromeMark::Chevron { turned_degrees } | ChromeMark::TreeDisclosure { turned_degrees } =
        sprite.mark
    {
        let _ = write!(key, ":d{turned_degrees}");
    }
    let _ = write!(key, ":{width_px}x{height_px}");
    if sprite.mark.takes_current_color() {
        let _ = write!(key, ":{r:02x}{g:02x}{b:02x}");
    }
    // Desaturation is in the pixels, so it has to be in the identity of the
    // pixels. Opacity deliberately is not — it never touches the raster.
    if sprite.grayscale {
        key.push_str(":grey");
    }
    key
}

fn rasterize(sprite: &ChromeSprite, width_px: u32, height_px: u32) -> Option<Raster> {
    let document = svg_document(sprite, width_px, height_px)?;
    let raster = bt_math::rasterize_svg_document(document.as_bytes()).ok()?;
    let mut rgba = raster.rgba;
    if sprite.grayscale {
        desaturate(&mut rgba);
    }
    Some(Raster {
        rgba: Arc::from(rgba),
        width_px: raster.width_px,
        height_px: raster.height_px,
    })
}

/// `filter: grayscale(1)` over straight sRGB RGBA, in place.
///
/// The coefficients are the ones CSS names for this filter: the
/// `feColorMatrix` `type="saturate" values="0"` matrix of the Filter Effects
/// spec, which is Rec. 709 luma applied to the sRGB values as they are stored.
/// A browser applies it in exactly that space, so converting to linear light
/// first would be a *different* grey than the one the design asks for.
///
/// Alpha is untouched: desaturating changes what colour a pixel is, never
/// whether it is there — the mark's silhouette and its antialiasing survive.
fn desaturate(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let luma = 0.2126 * f32::from(pixel[0])
            + 0.7152 * f32::from(pixel[1])
            + 0.0722 * f32::from(pixel[2]);
        let grey = luma.round().clamp(0.0, 255.0) as u8;
        pixel[0] = grey;
        pixel[1] = grey;
        pixel[2] = grey;
    }
}

/// A standalone document at exactly the physical size the box wants. The
/// `viewBox` is the symbol's own coordinate system, so the mark is scaled by the
/// renderer's own transform rather than by pre-multiplied path data — which is
/// what keeps one source of truth for the geometry across every DPI.
fn svg_document(sprite: &ChromeSprite, width_px: u32, height_px: u32) -> Option<String> {
    // For every mark but the turning chevron this is the box it was given; see
    // [`ChromeMark::raster_bleed`] for the one that needs more.
    let (raster_width_px, raster_height_px) = sprite.mark.raster_size(width_px, height_px);
    let (view_box, body) = match sprite.mark {
        ChromeMark::ActiveTab { radius_px } => {
            let path = active_tab_path(width_px, height_px, radius_px)?;
            (
                format!("0 0 {width_px} {height_px}"),
                format!(r#"<path fill="currentColor" d="{path}"/>"#),
            )
        }
        ChromeMark::TabBody { radius_px } => {
            let path = tab_body_path(width_px, height_px, radius_px)?;
            (
                format!("0 0 {width_px} {height_px}"),
                format!(r#"<path fill="currentColor" d="{path}"/>"#),
            )
        }
        // A stroked path, not a filled one: `stroke-width` centres on the path,
        // so the path is the tab's own box pulled in by half the stroke and the
        // ring lands exactly inside the edge — which is what `inset` means.
        ChromeMark::TabBodyRing {
            radius_px,
            stroke_px,
        } => {
            let inset = f32::from(u16::try_from(stroke_px).ok()?) / 2.0;
            let path = tab_body_inset_path(width_px, height_px, radius_px, inset)?;
            (
                format!("0 0 {width_px} {height_px}"),
                format!(
                    r#"<path fill="none" stroke="currentColor" stroke-width="{stroke_px}" d="{path}"/>"#
                ),
            )
        }
        ChromeMark::ControlPill { radius_px } => {
            let path = control_pill_path(width_px, height_px, radius_px)?;
            (
                format!("0 0 {width_px} {height_px}"),
                format!(r#"<path fill="currentColor" d="{path}"/>"#),
            )
        }
        ChromeMark::ControlPillFoot { radius_px } => {
            let path = control_pill_foot_path(width_px, height_px, radius_px)?;
            (
                format!("0 0 {width_px} {height_px}"),
                format!(r#"<path fill="currentColor" d="{path}"/>"#),
            )
        }
        // Stroked and inset by half the stroke, exactly as `TabBodyRing` is and
        // for the same reason: `stroke-width` centres on the path, so the path
        // has to be the box pulled in by half of it for the ring to land inside
        // the edge.
        ChromeMark::ControlPillRing {
            radius_px,
            stroke_px,
        } => {
            let inset = f32::from(u16::try_from(stroke_px).ok()?) / 2.0;
            let path = control_pill_inset_path(width_px, height_px, radius_px, inset)?;
            (
                format!("0 0 {width_px} {height_px}"),
                format!(
                    r#"<path fill="none" stroke="currentColor" stroke-width="{stroke_px}" d="{path}"/>"#
                ),
            )
        }
        ChromeMark::CardCorner { radius_px, corner } => {
            // The bite's centre is the card's own arc centre, which is `radius`
            // inside the box on each axis the card runs away along.
            let radius = f64::from(radius_px.max(1));
            let (centre_x, centre_y) = match corner {
                Corner::TopLeft => (radius, radius),
                Corner::TopRight => (0.0, radius),
                Corner::BottomLeft => (radius, 0.0),
                Corner::BottomRight => (0.0, 0.0),
            };
            (
                format!("0 0 {width_px} {height_px}"),
                // Square minus circle, `evenodd`. The circle is written as two
                // half-arcs because a single arc from a point to itself is a
                // degenerate `A` command every renderer is entitled to drop.
                format!(
                    r#"<path fill="currentColor" fill-rule="evenodd" d="M0 0H{width_px}V{height_px}H0Z M{left} {centre_y} a{radius} {radius} 0 1 0 {diameter} 0 a{radius} {radius} 0 1 0 -{diameter} 0Z"/>"#,
                    left = centre_x - radius,
                    diameter = radius * 2.0,
                ),
            )
        }
        // No path and no rounds: the whole box, which is the only shape a
        // rectangle has. It still goes through the rasterizer rather than
        // shortcutting to a quad because the *layer* is what it came here for.
        ChromeMark::Fill => (
            format!("0 0 {width_px} {height_px}"),
            format!(r#"<rect width="{width_px}" height="{height_px}" fill="currentColor"/>"#),
        ),
        // One quadratic corner to corner, inset by half the pen so the whole
        // stroke lands inside its own raster. The control point sits over the
        // lane end at the dot's own height, which is what makes the curve leave
        // the dot flat and arrive at the lane upright.
        ChromeMark::GraphCurve {
            stroke_px,
            flip_x,
            flip_y,
        } => {
            let inset = f64::from(stroke_px.max(1)) / 2.0;
            let right = f64::from(width_px) - inset;
            let bottom = f64::from(height_px) - inset;
            let (dot_x, lane_x) = if flip_x {
                (right, inset)
            } else {
                (inset, right)
            };
            let (dot_y, lane_y) = if flip_y {
                (bottom, inset)
            } else {
                (inset, bottom)
            };
            (
                format!("0 0 {width_px} {height_px}"),
                format!(
                    r#"<path fill="none" stroke="currentColor" stroke-width="{stroke_px}" stroke-linecap="round" d="M{dot_x} {dot_y} Q{lane_x} {dot_y} {lane_x} {lane_y}"/>"#
                ),
            )
        }
        // The chassis, filled from the eight. The panel's hex is written into
        // the document rather than handed to the `currentColor` substitution
        // below, because that substitution is what a mark following the theme
        // goes through and this one does not follow it — see
        // `ChromeMark::takes_current_color`. The chevron and the underline stay
        // `#fff`, which is what `#p-pwsh` and `#p-shell` both draw and what
        // keeps the glyph legible on all eight grounds.
        ChromeMark::ProfileGeneric { colour } => {
            let [r, g, b] = colour.hex();
            (
                PROFILE_CHASSIS_VIEW_BOX.to_owned(),
                format!(
                    // The three parts are concatenated rather than written as
                    // one long literal to keep them one line each, as their
                    // quoted siblings in `SYMBOL_BODY` are — which costs the
                    // implicit `{r}` capture, because a `concat!` is not a
                    // literal to `format!` and only a literal supports it.
                    concat!(
                        r##"<rect x="1" y="2.5" width="14" height="11" rx="1.8" fill="#{r:02x}{g:02x}{b:02x}"/>"##,
                        r##"<path d="M4.4 5.7L7.3 8l-2.9 2.3" fill="none" stroke="#fff" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round"/>"##,
                        r##"<path d="M8.5 10.9h3.2" stroke="#fff" stroke-width="1.35" stroke-linecap="round"/>"##,
                    ),
                    r = r,
                    g = g,
                    b = b,
                ),
            )
        }
        ChromeMark::ProgressRing {
            start_milliturns,
            sweep_milliturns,
            stroke_px,
        } => (
            format!("0 0 {width_px} {height_px}"),
            progress_ring_body(
                width_px,
                height_px,
                stroke_px,
                start_milliturns,
                sweep_milliturns,
            )?,
        ),
        // One arrow, caught at an angle: `.chevbtn svg`'s transition, which
        // turns the resting glyph about its own centre all the way to
        // `rotate(180deg)` and every degree in between.
        //
        // Stated in pixels rather than left to the `viewBox` because the box is
        // no longer the symbol's: the turn needs room outside it, and the
        // padding has to land on the *outside* without disturbing where the
        // glyph itself falls. So the document's own coordinates are the padded
        // raster's pixels, and the transform reproduces, term for term, what
        // `preserveAspectRatio="xMidYMid meet"` would have done with the
        // unpadded box — the same uniform scale, the same centring inset — with
        // the pad added on afterwards as whole pixels. `rotate` is applied
        // first, in the symbol's own units, about the symbol's own centre.
        ChromeMark::Chevron { turned_degrees } => {
            let geometry = chevron_raster(width_px, height_px);
            let [centre_x, centre_y] = CHEVRON_TURN_CENTRE_UNITS;
            (
                format!("0 0 {raster_width_px} {raster_height_px}"),
                format!(
                    r#"<g transform="translate({x} {y}) scale({scale}) rotate({turned_degrees} {centre_x} {centre_y})">{body}</g>"#,
                    x = geometry.origin_x_px,
                    y = geometry.origin_y_px,
                    scale = geometry.scale,
                    body = SYMBOL_BODY[symbol_index(sprite.mark)],
                ),
            )
        }
        // The triangle turns inside its own box. No padded raster and no
        // re-derived fit: at every angle the swept glyph clears the ten-unit
        // box (see the variant's note), so the ordinary `viewBox` still holds
        // and the transform is the CSS one term for term — `rotate(deg)` about
        // a 10×10 box's default origin, which is its centre.
        ChromeMark::TreeDisclosure { turned_degrees } => (
            SYMBOL_VIEW_BOX[symbol_index(sprite.mark)].to_owned(),
            format!(
                r#"<g transform="rotate({turned_degrees} {centre} {centre})">{body}</g>"#,
                centre = TREE_DISCLOSURE_TURN_CENTRE_UNITS,
                body = SYMBOL_BODY[symbol_index(sprite.mark)],
            ),
        ),
        mark => (SYMBOL_VIEW_BOX[symbol_index(mark)].to_owned(), {
            SYMBOL_BODY[symbol_index(mark)].to_owned()
        }),
    };
    let [r, g, b] = sprite.color;
    let body = if sprite.mark.takes_current_color() {
        body.replace("currentColor", &format!("#{r:02x}{g:02x}{b:02x}"))
    } else {
        body
    };
    Some(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{raster_width_px}" height="{raster_height_px}" viewBox="{view_box}">{body}</svg>"#
    ))
}

/// `#i-chev`'s own `viewBox`, as units, and the point `.chevbtn.open svg`
/// turns about.
///
/// The centre is the box's middle, which is what CSS's own default
/// `transform-origin: 50% 50%` names — the rotation the mock-up asks for is
/// about the *element*, and the element is the symbol's box.
const CHEVRON_VIEW_BOX_UNITS: [f64; 2] = [10.0, 6.0];
const CHEVRON_TURN_CENTRE_UNITS: [f64; 2] = [
    CHEVRON_VIEW_BOX_UNITS[0] / 2.0,
    CHEVRON_VIEW_BOX_UNITS[1] / 2.0,
];

/// Where a chevron's symbol units land inside the padded raster, and how much
/// padding that took.
struct ChevronRaster {
    pad_x_px: u32,
    pad_y_px: u32,
    /// The uniform scale from symbol units to physical pixels.
    scale: f64,
    /// Where the symbol's origin — the `viewBox`'s top-left, not the arrow —
    /// lands in the padded raster.
    origin_x_px: f64,
    origin_y_px: f64,
}

/// The turning chevron's raster geometry for a glyph box of `width_px` by
/// `height_px`.
///
/// Two halves, and the whole design is in keeping them apart.
///
/// The first half reproduces the placement the glyph already had. An SVG whose
/// `viewBox` is the symbol's and whose box is the layout's is fitted under
/// `preserveAspectRatio="xMidYMid meet"`: one uniform scale, the smaller of the
/// two ratios, and whatever slack the other axis has left split evenly on
/// either side. Written out, that is `scale` and the two insets below — the
/// same numbers, so the same pixels.
///
/// The second half is the room the turn needs. Every angle of a rotation about
/// the box's centre stays inside the circle that box is inscribed in, so the
/// radius is the box's own half-diagonal — the bound is the *box's* corner
/// reach, not the arrow's, which is what makes it hold for any artwork the
/// symbol could carry rather than for this one path. The padding is then that
/// radius less what the box already offers on each side, rounded **up to a
/// whole pixel**: a fractional pad would slide the glyph off the pixel grid it
/// was rasterized on and rewrite its antialiasing, which is precisely the
/// "resting arrow is unchanged" promise this exists to keep. Symmetric, so the
/// centre stays put; whole, so the sampling does.
fn chevron_raster(width_px: u32, height_px: u32) -> ChevronRaster {
    let [view_width, view_height] = CHEVRON_VIEW_BOX_UNITS;
    let (box_width, box_height) = (f64::from(width_px), f64::from(height_px));
    let scale = (box_width / view_width).min(box_height / view_height);
    let inset_x = (box_width - view_width * scale) / 2.0;
    let inset_y = (box_height - view_height * scale) / 2.0;
    let radius = view_width.hypot(view_height) / 2.0 * scale;
    let pad_x_px = (radius - box_width / 2.0).ceil().max(0.0) as u32;
    let pad_y_px = (radius - box_height / 2.0).ceil().max(0.0) as u32;
    ChevronRaster {
        pad_x_px,
        pad_y_px,
        scale,
        origin_x_px: f64::from(pad_x_px) + inset_x,
        origin_y_px: f64::from(pad_y_px) + inset_y,
    }
}

/// One arc of `.pring`, as the mock-up draws it: a stroked `<circle>` with a
/// dash pattern selecting the live part of it.
///
/// Every decision here is quoted rather than invented:
///
/// * `fill: none; stroke-width: 2` and `stroke-linecap: round` are `.pring
///   circle` and `.pring .arc` (mock-up lines 277-279).
/// * the dash pattern is how the mock-up itself states an arc — `stroke-dasharray`
///   against `PRING_C`, with `stroke-dashoffset` walking it (line 4118-4130) —
///   rather than a hand-built elliptical-arc path, which would have to
///   special-case the full turn that a dash pattern gets for free.
/// * `rotate(-90 …)` is `.pring { transform: … rotate(-90deg) }` (line 273): an
///   SVG circle's own path begins at 3 o'clock, and progress begins at 12.
///
/// The radius is the box's half-width less half the stroke, so the stroke —
/// which straddles its path — lands exactly inside the box and clips nowhere.
fn progress_ring_body(
    width_px: u32,
    height_px: u32,
    stroke_px: u32,
    start_milliturns: u16,
    sweep_milliturns: u16,
) -> Option<String> {
    let side = width_px.min(height_px) as f32;
    let stroke = stroke_px as f32;
    let radius = (side - stroke) / 2.0;
    // Below this the ring is not a ring: the hole has closed and what is left is
    // a blob. A degenerate ring would be a second, unruled shape.
    if stroke < 1.0 || radius <= stroke / 2.0 {
        return None;
    }
    let sweep = f32::from(sweep_milliturns.min(1000)) / 1000.0;
    if sweep <= 0.0 {
        // Not an error: a determinate ring at 0% is a track and no arc, and the
        // caller draws the track as its own sprite.
        return Some(String::new());
    }
    let (centre_x, centre_y) = (width_px as f32 / 2.0, height_px as f32 / 2.0);
    let circumference = std::f32::consts::TAU * radius;
    let dash = circumference * sweep;
    let start = circumference * f32::from(start_milliturns % 1000) / 1000.0;
    // A round cap adds half a stroke beyond each end of the dash. On a full turn
    // that overshoot would wrap the two caps past one another; on a partial arc
    // it is the mock-up's own `stroke-linecap: round`.
    let linecap = if sweep >= 1.0 { "butt" } else { "round" };
    Some(format!(
        r#"<circle cx="{centre_x}" cy="{centre_y}" r="{radius}" fill="none" stroke="currentColor" stroke-width="{stroke}" stroke-linecap="{linecap}" stroke-dasharray="{dash} {circumference}" stroke-dashoffset="{offset}" transform="rotate(-90 {centre_x} {centre_y})"/>"#,
        offset = -start,
    ))
}

fn symbol_index(mark: ChromeMark) -> usize {
    match mark {
        ChromeMark::Gear => 0,
        ChromeMark::WindowMinimize => 1,
        ChromeMark::WindowMaximize => 2,
        ChromeMark::WindowClose => 3,
        ChromeMark::TabClose => 3,
        ChromeMark::PaneClose => 3,
        ChromeMark::Plus => 4,
        ChromeMark::ProfilePowerShell => 5,
        ChromeMark::File => 6,
        ChromeMark::Globe => 41,
        ChromeMark::Folder => 7,
        ChromeMark::FolderOpen => 15,
        ChromeMark::TreeDisclosure { .. } => 16,
        ChromeMark::Panel => 8,
        ChromeMark::Chevron { .. } => 9,
        ChromeMark::Pin { filled: false } => 10,
        ChromeMark::Pin { filled: true } => 11,
        ChromeMark::Lock { engaged: false } => 42,
        ChromeMark::Lock { engaged: true } => 43,
        ChromeMark::ProfileUbuntu => 12,
        ChromeMark::ProfileGit => 13,
        ChromeMark::ProfileCmd => 14,
        ChromeMark::Float => 17,
        ChromeMark::External => 40,
        ChromeMark::DockLeft => 18,
        ChromeMark::DockRight => 26,
        ChromeMark::ResizeGrip => 19,
        ChromeMark::Check => 20,
        ChromeMark::Copy => 21,
        ChromeMark::Paste => 22,
        ChromeMark::Save => 23,
        ChromeMark::Eye => 24,
        ChromeMark::Code => 25,
        ChromeMark::GitBranch => 27,
        ChromeMark::GitGraph => 28,
        ChromeMark::Minus => 29,
        ChromeMark::Tag => 34,
        ChromeMark::Refresh => 35,
        ChromeMark::SelectAll => 36,
        ChromeMark::Broom => 37,
        ChromeMark::Eraser => 38,
        ChromeMark::Pencil => 39,
        ChromeMark::GitMergeCurve => 30,
        ChromeMark::Split => 31,
        ChromeMark::SplitRight => 32,
        ChromeMark::SplitDown => 33,
        // Handled before this function is reached; their geometry is generated,
        // not quoted.
        ChromeMark::ActiveTab { .. } => 8,
        ChromeMark::TabBody { .. } => 8,
        ChromeMark::TabBodyRing { .. } => 8,
        ChromeMark::ControlPill { .. } => 8,
        ChromeMark::ControlPillFoot { .. } => 8,
        ChromeMark::ControlPillRing { .. } => 8,
        ChromeMark::CardCorner { .. } => 8,
        ChromeMark::Fill => 8,
        ChromeMark::ProgressRing { .. } => 8,
        ChromeMark::GraphCurve { .. } => 8,
        ChromeMark::ProfileGeneric { .. } => 8,
    }
}

/// The profile marks' own box, and the chassis's — `#p-shell` is `#p-pwsh` and
/// `#p-cmd`'s drawing a third time, so it is drawn in their coordinates.
///
/// Named rather than spelled inside the format string because the chassis's body
/// is generated and its siblings' are quoted, and two spellings of one box is
/// how a generated glyph comes to sit a pixel off the ones beside it.
/// **The mark a preview buffer wears wherever it is drawn small** — the
/// switcher's row, the Recent row, the restore prompt's row (Web 预览块 W2 片③).
///
/// One function and not a `?:` at each of the three, which is the mock-up's own
/// note on `pvMarkId` said in Rust: "so a page can never be a globe in the
/// switcher and a page icon in the strip". A file is `#i-file`, a page is
/// `#i-globe`, and every surface that draws a preview row asks here.
///
/// A `bool` and not a `PreviewSource`, because two of the three callers have no
/// buffer to ask: a Recent row and a pinned URL stand for a page nothing has
/// opened, and what they have is the vault's or the pin file's own word for
/// which of the two the row is.
#[must_use]
pub fn preview_row_mark(is_page: bool) -> ChromeMark {
    if is_page {
        ChromeMark::Globe
    } else {
        ChromeMark::File
    }
}

const PROFILE_CHASSIS_VIEW_BOX: &str = "0 0 16 16";

const SYMBOL_VIEW_BOX: [&str; 44] = [
    "0 0 24 24",
    "0 0 10 10",
    "0 0 10 10",
    "0 0 10 10",
    "0 0 10 10",
    "0 0 16 16",
    "0 0 16 16",
    "0 0 16 16",
    "0 0 16 16",
    "0 0 10 6",
    "0 0 16 16",
    "0 0 16 16",
    "0 0 16 16",
    "0 0 16 16",
    "0 0 16 16",
    "0 0 16 16",
    // `#i-tri` is the one glyph the mock-up draws in its own ten-unit box
    // rather than the sixteen everything else uses, and it stays that way: the
    // disclosure triangle is 10×10 on screen, so a 10-unit box is a one-to-one
    // map and re-cutting it to 16 would round its edges differently.
    "0 0 10 10",
    // `#i-float` and `#i-dock-left`, both the house sixteen.
    "0 0 16 16",
    "0 0 16 16",
    // The resize grip's chevron is `::after`'s own 8×8 box, not a symbol from
    // the sheet, and it keeps that box for `#i-tri`'s reason: the CSS radius and
    // border width are quoted in those units, and re-cutting them to sixteen
    // would mean re-deriving both instead of reading them off.
    "0 0 8 8",
    // `#i-check`, the house sixteen again.
    "0 0 16 16",
    // `#i-copy` and `#i-paste`, the house sixteen a third and fourth time.
    "0 0 16 16",
    "0 0 16 16",
    // `#i-save`, `#i-eye`, `#i-code` — the preview header's three tools, all
    // sixteen.
    "0 0 16 16",
    "0 0 16 16",
    "0 0 16 16",
    // `#i-dock-right`, the mirrored dock, in its twin's box.
    "0 0 16 16",
    // `#i-git-branch` and `#i-git-graph` — struck for this product rather than
    // lifted from the sheet, and cut to the house sixteen so they sit at the
    // same optical weight as everything above them.
    "0 0 16 16",
    "0 0 16 16",
    // `#i-minus` keeps `#i-plus`'s ten-unit box, because it is that path with
    // one stroke removed.
    "0 0 10 10",
    // The mini graph's merge curve, in `.ggr`'s own box: 14 wide by 27 tall, the
    // height of one commit row. Not the house sixteen, for `#i-tri`'s reason —
    // the design quotes this curve's control points in these units.
    "0 0 14 27",
    // `#i-split`, the house sixteen — it stands beside `#i-folder` in the same
    // run and has to be cut in the same units to sit at the same weight.
    "0 0 16 16",
    // `#i-split-right` and `#i-split-down`, the same sixteen: they are the same
    // drawing turned, and a turn is only legible if nothing else changed.
    "0 0 16 16",
    "0 0 16 16",
    // `#i-tag` and `#i-refresh`, struck for v2 (3) and cut to the house sixteen
    // for the reason the two git marks above them are: a mark that shares a row
    // with `#i-file` has to be cut in the same units to sit at the same weight.
    "0 0 16 16",
    "0 0 16 16",
    // `#i-select`, `#i-broom` and `#i-clear` — the terminal menu's three own
    // marks (ticket #62), lifted from the sheet in the sheet's own sixteen.
    "0 0 16 16",
    "0 0 16 16",
    "0 0 16 16",
    // `#i-pencil`, the house sixteen: it stands in a run beside `#i-close` and
    // has to be cut in the same units as every other mark in this dialog.
    "0 0 16 16",
    // `#i-external`, the house sixteen — it is `#i-float`'s arrow drawn without
    // the frame, and the two share a run, so they share a box.
    "0 0 16 16",
    // `#i-globe`, the house sixteen: it stands in the same 13px box `#i-file`
    // does, down one left edge with it (§7.7 ⑤), so it has to be cut in the same
    // units or it sits at a different optical weight in the same column.
    "0 0 16 16",
    // `#i-lock` and `#i-locked`, the house sixteen: they stand in the preview
    // head's run beside `#i-save` and `#i-float`, which are cut there, and a
    // mark in a run has to share the run's units or it sits at a different
    // weight in the same row.
    "0 0 16 16",
    "0 0 16 16",
];

/// The `<symbol>` bodies, byte for byte from `design/ui-mockup.html` (the
/// `<svg style="display:none">` block near the top of `<body>`).
const SYMBOL_BODY: [&str; 44] = [
    // #i-gear
    r#"<path fill="currentColor" d="M19.14 12.94c.04-.3.06-.61.06-.94 0-.32-.02-.64-.07-.94l2.03-1.58a.49.49 0 0 0 .12-.61l-1.92-3.32a.488.488 0 0 0-.59-.22l-2.39.96c-.5-.38-1.03-.7-1.62-.94l-.36-2.54a.484.484 0 0 0-.48-.41h-3.84c-.24 0-.43.17-.47.41l-.36 2.54c-.59.24-1.13.57-1.62.94l-2.39-.96c-.22-.08-.47 0-.59.22L2.74 8.87c-.12.21-.08.47.12.61l2.03 1.58c-.05.3-.09.63-.09.94s.02.64.07.94l-2.03 1.58a.49.49 0 0 0-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h3.84c.24 0 .44-.17.47-.41l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32c.12-.22.07-.47-.12-.61l-2.01-1.58zM12 15.6c-1.98 0-3.6-1.62-3.6-3.6s1.62-3.6 3.6-3.6 3.6 1.62 3.6 3.6-1.62 3.6-3.6 3.6z"/>"#,
    // #i-min
    r#"<path d="M0 5h10" fill="none" stroke="currentColor" stroke-width="1"/>"#,
    // #i-max
    r#"<rect x="0.5" y="0.5" width="9" height="9" rx="1.8" fill="none" stroke="currentColor" stroke-width="1"/>"#,
    // #i-close
    r#"<path d="M0.5 0.5l9 9M9.5 0.5l-9 9" fill="none" stroke="currentColor" stroke-width="1"/>"#,
    // #i-plus. `fill="none"` is ours: the mock-up's own path carries no fill
    // attribute and therefore fills black, which costs a browser nothing because
    // two straight subpaths enclose no area — but it is a lie about the glyph,
    // and this rasterizer has no reason to be handed one.
    r#"<path d="M5 0.5v9M0.5 5h9" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#,
    // #p-pwsh — flat, and its own colours (a mark carries its own).
    concat!(
        r##"<rect x="1" y="2.5" width="14" height="11" rx="1.8" fill="#2C5C9E"/>"##,
        r##"<path d="M4.4 5.7L7.3 8l-2.9 2.3" fill="none" stroke="#fff" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round"/>"##,
        r##"<path d="M8.5 10.9h3.2" stroke="#fff" stroke-width="1.35" stroke-linecap="round"/>"##,
    ),
    // #i-file
    concat!(
        r#"<path d="M3.5 1.8h5.2l3.8 3.8v8.6c0 .3-.2.5-.5.5H3.5c-.3 0-.5-.2-.5-.5V2.3c0-.3.2-.5.5-.5z" fill="none" stroke="currentColor" stroke-width="1.15"/>"#,
        r#"<path d="M8.6 1.9v3.8h3.8" fill="none" stroke="currentColor" stroke-width="1.15" stroke-linejoin="round"/>"#,
    ),
    // #i-folder
    r#"<path d="M1.6 4.2c0-.6.5-1.1 1.1-1.1h3.1l1.3 1.5h6.2c.6 0 1.1.5 1.1 1.1v6.6c0 .6-.5 1.1-1.1 1.1H2.7c-.6 0-1.1-.5-1.1-1.1z" fill="currentColor"/>"#,
    // #i-panel
    concat!(
        r#"<rect x="1.5" y="2.5" width="13" height="11" rx="2" fill="none" stroke="currentColor" stroke-width="1.1"/>"#,
        r#"<path d="M6.2 2.5v11" stroke="currentColor" stroke-width="1.1"/>"#,
    ),
    // #i-chev
    r#"<path d="M1 1l4 3.6L9 1" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>"#,
    // #i-pin — the action. Outline, and the 45° turn is the symbol's own: the
    // `<g transform>` is quoted along with the path, so nothing here has to be
    // re-applied at draw time the way the open chevron's flip is.
    r#"<g transform="rotate(45 8 8)"><path d="M5.5 1.6h5a.8.8 0 010 1.6h-.7v2.9l2.15 2.25c.42.44.65 1.03.65 1.64a.6.6 0 01-.6.6H8.8v4.2a.8.8 0 01-1.6 0v-4.2H4a.6.6 0 01-.6-.6c0-.61.23-1.2.65-1.64L6.2 6.1V3.2h-.7a.8.8 0 010-1.6z" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linejoin="round"/></g>"#,
    // #i-pinned — the state. Same group, same `d`, and the only difference in
    // the file is that the outline's three stroke attributes become one fill.
    r#"<g transform="rotate(45 8 8)"><path d="M5.5 1.6h5a.8.8 0 010 1.6h-.7v2.9l2.15 2.25c.42.44.65 1.03.65 1.64a.6.6 0 01-.6.6H8.8v4.2a.8.8 0 01-1.6 0v-4.2H4a.6.6 0 01-.6-.6c0-.61.23-1.2.65-1.64L6.2 6.1V3.2h-.7a.8.8 0 010-1.6z" fill="currentColor"/></g>"#,
    // #p-ubuntu — the Circle of Friends, and its own colours. The three friends
    // are stroked in the disc's own orange, which *punches* the gaps in the
    // white ring rather than drawing the ring in three separate segments: one
    // ring, three holes, and no seam to line up.
    concat!(
        r##"<circle cx="8" cy="8" r="7" fill="#E95420"/>"##,
        r##"<circle cx="8" cy="8" r="4.1" fill="none" stroke="#fff" stroke-width="1"/>"##,
        r##"<g stroke="#E95420" stroke-width="1.5">"##,
        r##"<circle cx="8" cy="3.9" r="1.5" fill="#fff"/>"##,
        r##"<circle cx="11.55" cy="10.05" r="1.5" fill="#fff"/>"##,
        r##"<circle cx="4.45" cy="10.05" r="1.5" fill="#fff"/>"##,
        r##"</g>"##,
    ),
    // #p-git — the lozenge, the branch, and its three nodes.
    concat!(
        r##"<path d="M8 1.3L14.7 8 8 14.7 1.3 8z" fill="#F05033" stroke="#F05033" stroke-width="1.1" stroke-linejoin="round"/>"##,
        r##"<g stroke="#fff" stroke-width="1.25" fill="none" stroke-linecap="round">"##,
        r##"<path d="M5.7 10.3l4.6-4.6"/><path d="M8 8l2.3 2.3"/>"##,
        r##"</g>"##,
        r##"<g fill="#fff"><circle cx="5.7" cy="10.3" r="1.2"/><circle cx="10.3" cy="5.7" r="1.2"/><circle cx="10.3" cy="10.3" r="1.2"/></g>"##,
    ),
    // #p-cmd — charcoal with a lighter edge, never true console black; see the
    // `ProfileCmd` variant's own note for the ruling that forbids "fixing" it.
    // The chevron and underline are `#p-pwsh`'s geometry to the digit, in
    // `#CCCCCC` instead of white: the two are the same console idiom, and what
    // tells them apart is the panel they sit in.
    concat!(
        r##"<rect x="1.4" y="2.9" width="13.2" height="10.2" rx="1.7" fill="#3A3A3A" stroke="#606060" stroke-width=".8"/>"##,
        r##"<path d="M4.4 5.7L7.3 8l-2.9 2.3" fill="none" stroke="#CCCCCC" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round"/>"##,
        r##"<path d="M8.5 10.9h3.2" stroke="#CCCCCC" stroke-width="1.35" stroke-linecap="round"/>"##,
    ),
    // #i-folder-open — the same folder seen from the front: a translucent back
    // plate still in the closed folder's silhouette, and a solid front plate
    // skewed off it. The `.55` is the mock-up's own, and it is what makes an
    // open folder read as *the same object* rather than a second icon.
    concat!(
        r##"<path d="M1.6 4.2c0-.6.5-1.1 1.1-1.1h3.1l1.3 1.5h6.2c.6 0 1.1.5 1.1 1.1v1H4.3c-.5 0-.9.3-1 .8L1.6 12z" fill="currentColor" opacity=".55"/>"##,
        r##"<path d="M3.3 7.4c.1-.5.5-.8 1-.8h10.3c.7 0 1.2.7 1 1.4l-1.2 4.3c-.1.5-.6.8-1.1.8H2.4c-.7 0-1.2-.6-1-1.3z" fill="currentColor"/>"##,
    ),
    // #i-tri — the disclosure triangle, pointing right at rest. It is turned
    // rather than swapped for a second glyph, on the precedent `#i-chev` sets:
    // the turn is the sentence, and two end frames are only its punctuation.
    r##"<path d="M3.2 2.2L6.6 5 3.2 7.8z" fill="currentColor"/>"##,
    // #i-float — a rounded frame with one corner opened for the arrow leaving
    // through it. The frame's own top-right corner is missing from the `d` on
    // purpose: the arrow is drawn *through* the gap, so the two paths read as
    // one object rather than as an arrow laid over a closed box.
    concat!(
        r##"<path d="M8 3.2H4.4c-.7 0-1.2.6-1.2 1.2v7.2c0 .7.6 1.2 1.2 1.2h7.2c.7 0 1.2-.6 1.2-1.2V8" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"##,
        r##"<path d="M10.2 2.8h3v3M13.2 2.8L8.3 7.7" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>"##,
    ),
    // #i-dock-left — the window, and the panel that will appear inside it, on
    // the side it will actually appear on.
    concat!(
        r##"<rect x="1.6" y="2.6" width="12.8" height="10.8" rx="2.2" fill="none" stroke="currentColor" stroke-width="1.2"/>"##,
        r##"<rect x="1.6" y="2.6" width="5" height="10.8" rx="2.2" fill="currentColor" opacity=".7"/>"##,
    ),
    // The resize grip, translated from CSS rather than quoted from the sheet.
    //
    // `border-right: 1.5px` and `border-bottom: 1.5px` on an 8×8 box put the two
    // strokes' centre lines 0.75 in from each edge, at x = y = 7.25;
    // `border-bottom-right-radius: 7px` is the *outer* radius, so the centre
    // line turns on 7 − 0.75 = 6.25 about (1, 1). The right border therefore
    // runs from the top edge down to where the arc starts at y = 1, the arc
    // sweeps a quarter turn to (1, 7.25), and the bottom border runs on to the
    // left edge — which at this radius is almost the whole of the glyph.
    //
    // The `.55` is the mock-up's own, baked in here on `#i-folder-open`'s
    // precedent: it is part of the drawing, not a state of it.
    r##"<path d="M7.25 0V1A6.25 6.25 0 0 1 1 7.25H0" fill="none" stroke="currentColor" stroke-width="1.5" opacity=".55"/>"##,
    // #i-check
    r##"<path d="M3 8.4l3.2 3.2L13 4.8" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"##,
    // #i-copy
    concat!(
        r#"<rect x="5.4" y="2.4" width="8.2" height="8.2" rx="1.6" fill="none" stroke="currentColor" stroke-width="1.3"/>"#,
        r#"<path d="M10.6 13.6H3.9a1.5 1.5 0 01-1.5-1.5V5.4" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#,
    ),
    // #i-paste
    concat!(
        r#"<rect x="3" y="2.8" width="10" height="11" rx="1.6" fill="none" stroke="currentColor" stroke-width="1.3"/>"#,
        r#"<rect x="5.6" y="1.4" width="4.8" height="2.8" rx=".9" fill="currentColor"/>"#,
        r#"<path d="M5.4 7.2h5.2M5.4 9.6h5.2M5.4 12h3.2" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/>"#,
    ),
    // #i-save — the body with its cut corner, the shutter above and the label
    // below. Quoted from the sheet at line 2252.
    concat!(
        r#"<path d="M3.2 2.6h7.9l2.3 2.3v8c0 .3-.3.6-.6.6H3.2c-.3 0-.6-.3-.6-.6V3.2c0-.3.3-.6.6-.6z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"/>"#,
        r#"<path d="M5.3 2.8v2.9h5.2V2.8M5.1 13.4V9.3h5.8v4.1" fill="none" stroke="currentColor" stroke-width="1.15"/>"#,
    ),
    // #i-eye — mock-up 2171.
    concat!(
        r#"<path d="M1.4 8S3.8 3.9 8 3.9 14.6 8 14.6 8 12.2 12.1 8 12.1 1.4 8 1.4 8z" fill="none" stroke="currentColor" stroke-width="1.3"/>"#,
        r#"<circle cx="8" cy="8" r="1.9" fill="none" stroke="currentColor" stroke-width="1.3"/>"#,
    ),
    // #i-code — mock-up 2170.
    r#"<path d="M5.8 4.4L2.2 8l3.6 3.6M10.2 4.4L13.8 8l-3.6 3.6" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>"#,
    // #i-dock-right — `#i-dock-left` mirrored about the frame's own centre: the
    // window is unchanged and the panel moves to x = 9.4, which is 16 − 1.6 − 5.
    concat!(
        r##"<rect x="1.6" y="2.6" width="12.8" height="10.8" rx="2.2" fill="none" stroke="currentColor" stroke-width="1.2"/>"##,
        r##"<rect x="9.4" y="2.6" width="5" height="10.8" rx="2.2" fill="currentColor" opacity=".7"/>"##,
    ),
    // `#i-git-branch` — a trunk with three nodes and one branch leaving it.
    //
    // Struck for this product (R4): the design had a bare `⎇` here. The
    // geometry is the one every git client draws, laid on this sheet's own grid
    // — nodes at radius 1.85 so they read at the 13px the masthead asks for,
    // and 1.15 stroke, which is `#i-file`'s, because these two are seen a
    // centimetre apart in the same column.
    //
    // The branch is a cubic and not an arc: every curve on this sheet is a
    // cubic, and one shape rasterized through a different primitive is one
    // shape that antialiases differently.
    concat!(
        r#"<g fill="none" stroke="currentColor" stroke-width="1.15" stroke-linecap="round">"#,
        r#"<circle cx="4.7" cy="3.5" r="1.85"/>"#,
        r#"<circle cx="4.7" cy="12.5" r="1.85"/>"#,
        r#"<circle cx="11.3" cy="3.5" r="1.85"/>"#,
        r#"<path d="M4.7 5.35v5.3"/>"#,
        r#"<path d="M11.3 5.35v1.5C11.3 8.8 9.75 10.35 7.8 10.35H4.7"/>"#,
        r#"</g>"#,
    ),
    // `#i-git-graph` — three commits, two edges, two lanes (R27).
    //
    // The smallest honest picture of a directed acyclic graph: a trunk running
    // top to bottom, and one commit off to the side joining it. Two edges,
    // because two is the fewest that can show a *join* — one edge would be a
    // line and no graph at all.
    concat!(
        r#"<g fill="none" stroke="currentColor" stroke-width="1.15" stroke-linecap="round">"#,
        r#"<circle cx="4.7" cy="3.3" r="1.8"/>"#,
        r#"<circle cx="4.7" cy="12.7" r="1.8"/>"#,
        r#"<circle cx="11.6" cy="8" r="1.8"/>"#,
        r#"<path d="M4.7 5.1v5.8"/>"#,
        r#"<path d="M9.8 8H8.2C6.25 8 4.7 6.45 4.7 4.5"/>"#,
        r#"</g>"#,
    ),
    // #i-minus — `#i-plus`'s second subpath, alone.
    r#"<path d="M0.5 5h9" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#,
    // The merge curve — mock-up 4933, `.ggr path.side`, verbatim. It enters at
    // the row's top right and lands on the dot's edge at (7.6, 12.6), which is
    // just short of the 3.1-radius circle centred at (7, 13.5) — so the line
    // stops *at* the node rather than under it.
    r#"<path d="M13 0 C 13 8, 9.5 10.5, 7.6 12.6" fill="none" stroke="currentColor" stroke-width="1.5"/>"#,
    // `#i-split` — the house frame, divided on both axes. Same rect and same
    // 1.1 stroke as `#i-panel`, deliberately: they are the same object seen
    // twice, and only where the rules fall separates "a pane with a sidebar"
    // from "a pane being cut". The dividers run edge to edge rather than
    // stopping short, because a rule that stops short is a rule floating inside
    // a box; a division reaches the walls.
    concat!(
        r#"<rect x="1.5" y="2.5" width="13" height="11" rx="2" fill="none" stroke="currentColor" stroke-width="1.1"/>"#,
        r#"<path d="M8 2.5v11M1.5 8h13" fill="none" stroke="currentColor" stroke-width="1.1"/>"#,
    ),
    // `#i-split-right` — the two panes the cut leaves, side by side. The gutter
    // is 1.2 units, which at a menu row's fourteen pixels is one pixel of gap:
    // enough to be a gap and not enough to be a third shape.
    concat!(
        r#"<rect x="1.5" y="2.5" width="5.9" height="11" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.1"/>"#,
        r#"<rect x="8.6" y="2.5" width="5.9" height="11" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.1"/>"#,
    ),
    // `#i-split-down` — the same two panes, stacked. Same gutter, same stroke,
    // same outer box; only the axis differs, because that is the only thing the
    // reader is being asked to tell apart.
    concat!(
        r#"<rect x="1.5" y="2.5" width="13" height="4.9" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.1"/>"#,
        r#"<rect x="1.5" y="8.6" width="13" height="4.9" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.1"/>"#,
    ),
    // `#i-tag` — the label with a punched hole (T7). The outline runs from the
    // top-left corner along the top, out to the point at the right, back along
    // the bottom and home; the hole is filled rather than stroked because a
    // one-unit ring at this size closes up into a dot anyway, and a dot is what
    // it is meant to read as.
    concat!(
        r#"<path d="M2.9 2.2h4.9c.27 0 .53.11.72.3l5.28 5.28c.4.4.4 1.04 0 1.44l-4.26 4.26c-.4.4-1.04.4-1.44 0L2.5 7.88c-.19-.19-.3-.45-.3-.72V2.9c0-.39.31-.7.7-.7z" fill="none" stroke="currentColor" stroke-width="1.15" stroke-linejoin="round"/>"#,
        r#"<circle cx="5.1" cy="5.1" r="1" fill="currentColor"/>"#,
    ),
    // `#i-refresh` — a circle with a bite out of the top and a head on the end
    // (T5). The arc is written from the gap's far side *clockwise* the long way
    // round, so it arrives at twelve o'clock travelling to the right, which is
    // where the head points: the direction of travel and the direction of the
    // head are one fact, and an arc written the other way would have needed the
    // head turned round by hand.
    concat!(
        r#"<path d="M12.33 5.5a5 5 0 1 1-4.33-2.5" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#,
        r#"<path d="M8 1.3 11 3 8 4.7z" fill="currentColor"/>"#,
    ),
    // `#i-select` — mock-up 2373, verbatim. Four corner brackets and the rule
    // between them; the brackets are drawn as one path so the marquee reads as
    // one gesture rather than as four ticks that happen to line up.
    concat!(
        r#"<path d="M2.4 4.4v-2h2M11.6 2.4h2v2M13.6 11.6v2h-2M4.4 13.6h-2v-2" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>"#,
        r#"<path d="M4.8 8h6.4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#,
    ),
    // `#i-broom` — mock-up 2374, verbatim: the handle, and the head splayed
    // where it meets the floor.
    concat!(
        r#"<path d="M9.6 2.2l4.2 4.2" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#,
        r#"<path d="M11.7 6.4L9.6 4.3 3.4 9.7c-.9.8-1.2 2-.8 3.1l.6 1.5 1.5.6c1.1.4 2.3.1 3.1-.8l3.9-4.4z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/>"#,
    ),
    // `#i-clear` — mock-up 2371, verbatim: the eraser held at an angle, and the
    // line it has been rubbing along.
    concat!(
        r#"<path d="M6.4 12.6H13" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#,
        r#"<path d="M9.1 3.3l3.6 3.6c.4.4.4 1 0 1.4l-3.8 3.8c-.4.4-1 .4-1.4 0L3.9 8.5c-.4-.4-.4-1 0-1.4l3.8-3.8c.4-.4 1-.4 1.4 0z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"/>"#,
    ),
    // `#i-pencil` — mock-up 3452, verbatim: the ferrule, then the body of the
    // pencil with its point.
    concat!(
        r#"<path d="M10.6 2.4l3 3" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#,
        r#"<path d="M11.5 1.5l3 3-8.4 8.4-3.9.9.9-3.9z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"/>"#,
    ),
    // `#i-external` — the hand-off arrow, one path: the corner bracket that is
    // the head, and the shaft it stands on, drawn without the frame `#i-float`
    // wraps its own arrow in. The arms are two fifths of the shaft, which is what
    // keeps a bare arrowhead reading as one at thirteen pixels.
    r#"<path d="M7.4 3.6h5v5M12.4 3.6L3.6 12.4" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>"#,
    // `#i-globe` — byte for byte from `design/ui-mockup.html:4358`: a circle, the
    // two latitudes, and the ellipse that is every meridian seen edge on.
    concat!(
        r#"<circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.15"/>"#,
        r#"<path d="M2.6 6.2h10.8M2.6 9.8h10.8" stroke="currentColor" stroke-width="1.15" stroke-linecap="round"/>"#,
        r#"<ellipse cx="8" cy="8" rx="2.7" ry="6" fill="none" stroke="currentColor" stroke-width="1.15"/>"#,
    ),
    // `#i-lock` — the action, "you could lock this". The shackle is OPEN: it
    // stands on the body's right-hand leg only and leans away, which is how
    // every padlock in every icon set says the thing is not shut. Outline body,
    // per Fluent 2's fill axis, and stroke 1.25 — the pin's weight, because the
    // two are read in the same window at the same size and a lock drawn lighter
    // would read as a lock in a different mood rather than a different glyph.
    concat!(
        r#"<path d="M10.4 7.2V5a2.4 2.4 0 014.8 0v1" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round"/>"#,
        r#"<rect x="3.1" y="7.2" width="9.8" height="6.6" rx="1.6" fill="none" stroke="currentColor" stroke-width="1.25"/>"#,
    ),
    // `#i-locked` — the state, "it is locked". SAME BODY RECTANGLE, so the two
    // rasters differ only where the meaning does; the shackle comes down over
    // it and the body fills. Two axes for one fact, deliberately: at fourteen
    // pixels the fill alone is what the pin has to live on because Windows fixed
    // its angle, and a padlock has a second axis available, so it is spent.
    concat!(
        r#"<path d="M5.6 7.2V5a2.4 2.4 0 014.8 0v2.2" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round"/>"#,
        r#"<rect x="3.1" y="7.2" width="9.8" height="6.6" rx="1.6" fill="currentColor"/>"#,
    ),
];

/// The active tab's closed outline, in physical pixels, clockwise from the
/// bottom-left of the left skirt.
///
/// `width` spans the tab *and* both skirts, so the tab body itself is
/// `width - 2 * radius` wide — which is exactly how the mock-up lays it out: the
/// strip's `padding-left: var(--tabr)` puts the first tab one corner in, and
/// `.tab.active::before` fills that corner, so the silhouette starts at the
/// window's own left edge.
///
/// Four arcs, two of each kind:
///
/// * the two top corners are `border-radius: var(--tabr) var(--tabr) 0 0` —
///   convex, sweeping in the positive direction;
/// * the two skirt corners are the `radial-gradient(circle var(--tabr) at 0 0,
///   transparent 98%, var(--termbg) 100%)` pair — concave, centred *outside* the
///   silhouette, which is what makes the tab flare into the content plane
///   instead of ending in a hard corner.
///
/// The bottom edge is the closing straight line at `y = height`: the tab's fill
/// is `--termbg` and so is the surface directly below it, so there is nothing to
/// draw there and nothing may be drawn there.
fn active_tab_path(width: u32, height: u32, radius: u32) -> Option<String> {
    let (w, h, r) = (width as i64, height as i64, radius as i64);
    // Four radii across and two down: below that the shape is not a rounded tab
    // with skirts any more, and inventing a degenerate one would be a second,
    // unruled silhouette.
    if r < 1 || w < 4 * r || h < 2 * r {
        return None;
    }
    Some(format!(
        "M0,{h} \
         A{r},{r} 0 0 0 {r},{skirt_top} \
         L{r},{r} \
         A{r},{r} 0 0 1 {two_r},0 \
         L{top_right},0 \
         A{r},{r} 0 0 1 {body_right},{r} \
         L{body_right},{skirt_top} \
         A{r},{r} 0 0 0 {w},{h} Z",
        skirt_top = h - r,
        two_r = 2 * r,
        top_right = w - 2 * r,
        body_right = w - r,
    ))
}

/// A `border-radius: Npx` box, all four corners, in physical pixels.
///
/// The radius is clamped to half the shorter side the way a browser clamps it,
/// so a pill asked for more round than it has room for becomes a stadium rather
/// than a self-intersecting path.
fn control_pill_path(width: u32, height: u32, radius: u32) -> Option<String> {
    let (w, h) = (width as i64, height as i64);
    let r = (radius as i64).min(w / 2).min(h / 2);
    if r < 1 || w < 2 || h < 2 {
        return None;
    }
    Some(format!(
        "M{r},0 L{right},0 A{r},{r} 0 0 1 {w},{r} \
         L{w},{bottom} A{r},{r} 0 0 1 {right},{h} \
         L{r},{h} A{r},{r} 0 0 1 0,{bottom} \
         L0,{r} A{r},{r} 0 0 1 {r},0 Z",
        right = w - r,
        bottom = h - r,
    ))
}

/// [`control_pill_path`] with its top two corners left square — the foot of a
/// card (§7.1.6b′ F2, [`ChromeMark::ControlPillFoot`]).
///
/// A **square** top and not a smaller round: the head above it is a flat band
/// across the card's full width, so the body's top edge is an interior line and
/// has no corner to turn. Rounding it would draw two notches of the head's
/// colour into the body a third of the way down the card.
fn control_pill_foot_path(width: u32, height: u32, radius: u32) -> Option<String> {
    let (w, h) = (width as i64, height as i64);
    let r = (radius as i64).min(w / 2).min(h);
    if r < 1 || w < 2 || h < 1 {
        return None;
    }
    Some(format!(
        "M0,0 L{w},0 L{w},{bottom} A{r},{r} 0 0 1 {right},{h} \
         L{r},{h} A{r},{r} 0 0 1 0,{bottom} Z",
        right = w - r,
        bottom = h - r,
    ))
}

/// [`control_pill_path`] pulled `inset` pixels in on all four sides.
///
/// Unlike [`tab_body_inset_path`] this one closes: the shape it rings has a foot
/// of its own — `.vtab` is a box in a column, not a tab running into the content
/// plane — so a ring that stopped short of it would be an open outline drawn
/// around a closed fill.
fn control_pill_inset_path(width: u32, height: u32, radius: u32, inset: f32) -> Option<String> {
    let (l, t) = (inset, inset);
    let (w, h) = (width as f32 - inset, height as f32 - inset);
    // The round shrinks with the box exactly as a browser's inset shadow does.
    let r = (radius as f32 - inset).max(0.0);
    if r < 0.5 || w - l < 2.0 * r || h - t < 2.0 * r {
        return None;
    }
    Some(format!(
        "M{left_r},{t} L{right},{t} A{r},{r} 0 0 1 {w},{top_r} \
         L{w},{bottom} A{r},{r} 0 0 1 {right},{h} \
         L{left_r},{h} A{r},{r} 0 0 1 {l},{bottom} \
         L{l},{top_r} A{r},{r} 0 0 1 {left_r},{t} Z",
        left_r = l + r,
        top_r = t + r,
        right = w - r,
        bottom = h - r,
    ))
}

fn tab_body_path(width: u32, height: u32, radius: u32) -> Option<String> {
    let (w, h, r) = (width as i64, height as i64, radius as i64);
    if r < 1 || w < 2 * r || h < r {
        return None;
    }
    Some(format!(
        "M0,{h} L0,{r} A{r},{r} 0 0 1 {r},0 L{right},0 A{r},{r} 0 0 1 {w},{r} L{w},{h} Z",
        right = w - r,
    ))
}

/// [`tab_body_path`] pulled `inset` pixels in on every side it has an edge on.
///
/// The foot is left where it is: the tab has no bottom edge — it runs into the
/// content plane — and a ring that closed across the bottom would draw a line
/// through the join the whole silhouette exists to make.
fn tab_body_inset_path(width: u32, height: u32, radius: u32, inset: f32) -> Option<String> {
    let (w, h) = (width as f32 - inset, height as f32);
    let l = inset;
    // The round shrinks with the box, exactly as a browser's inset shadow does:
    // a corner pulled in by `inset` has `inset` less radius, and never less
    // than none at all.
    let r = (radius as f32 - inset).max(0.0);
    let t = inset;
    if r < 0.5 || w - l < 2.0 * r || h - t < r {
        return None;
    }
    Some(format!(
        "M{l},{h} L{l},{top_r} A{r},{r} 0 0 1 {left_r},{t}          L{right},{t} A{r},{r} 0 0 1 {w},{top_r} L{w},{h}",
        top_r = t + r,
        left_r = l + r,
        right = w - r,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sprite(mark: ChromeMark, width: f32, height: f32, color: [u8; 3]) -> ChromeSprite {
        ChromeSprite::new(mark, [0.0, 0.0, width, height], color)
    }

    fn alpha_at(icon: &ChromeIcon, x: u32, y: u32) -> u8 {
        icon.rgba[((y * icon.width_px + x) * 4 + 3) as usize]
    }

    fn rgb_at(icon: &ChromeIcon, x: u32, y: u32) -> [u8; 3] {
        let base = ((y * icon.width_px + x) * 4) as usize;
        [icon.rgba[base], icon.rgba[base + 1], icon.rgba[base + 2]]
    }

    /// A point on the ring's own stroke centreline, `turns` clockwise from 12.
    fn on_ring(icon: &ChromeIcon, turns: f32, stroke_px: f32) -> (u32, u32) {
        let centre = icon.width_px as f32 / 2.0;
        let radius = (icon.width_px as f32 - stroke_px) / 2.0;
        let angle = turns * std::f32::consts::TAU;
        (
            (centre + radius * angle.sin()).round() as u32,
            (centre - radius * angle.cos()).round() as u32,
        )
    }

    fn ring(sweep_milliturns: u16, stroke_px: u32, side: f32) -> ChromeSprite {
        sprite(
            ChromeMark::ProgressRing {
                start_milliturns: 0,
                sweep_milliturns,
                stroke_px,
            },
            side,
            side,
            [0x7a, 0x99, 0xff],
        )
    }

    /// PIN (T2 progress ring): the arc starts at 12 o'clock and runs clockwise.
    ///
    /// The mock-up gets both for free — its `.pring` is `rotate(-90deg)` over an
    /// SVG `<circle>`, whose path a browser walks clockwise from 3 o'clock — and
    /// so the design never had to say either out loud. Generated geometry has
    /// to, because every one of the ways to get this wrong (starting at 3
    /// o'clock, running anticlockwise, or both) still draws a perfectly
    /// plausible ring, and only a progress report that runs backwards tells you.
    #[test]
    fn the_progress_arc_starts_at_noon_and_sweeps_clockwise() {
        let stroke = 4.0_f32;
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[ring(250, stroke as u32, 30.0)]);
        let icon = &icons[0];
        // A quarter turn covers noon through 3 o'clock and nothing else.
        for turns in [0.02_f32, 0.10, 0.15, 0.23] {
            let (x, y) = on_ring(icon, turns, stroke);
            assert!(
                alpha_at(icon, x, y) > 128,
                "a quarter sweep must ink {turns} turns from noon, at ({x},{y})"
            );
        }
        for turns in [0.35_f32, 0.5, 0.75, 0.95] {
            let (x, y) = on_ring(icon, turns, stroke);
            assert_eq!(
                alpha_at(icon, x, y),
                0,
                "a quarter sweep must leave {turns} turns from noon bare, at ({x},{y})"
            );
        }
    }

    /// PIN (T2 progress ring): a longer sweep is always more ink.
    ///
    /// A ring that saturates — drawing the same arc for 40% as for 90% — is the
    /// failure that looks perfect in a screenshot and is useless in motion.
    #[test]
    fn a_longer_sweep_is_always_more_ink() {
        let mut inked = Vec::new();
        for sweep in [0_u16, 125, 250, 500, 750, 1000] {
            let mut rasters = ChromeMarkRasters::default();
            let icons = rasters.resolve(&[ring(sweep, 4, 30.0)]);
            inked.push(
                icons
                    .first()
                    .map(|icon| {
                        icon.rgba
                            .chunks_exact(4)
                            .map(|pixel| u32::from(pixel[3]))
                            .sum::<u32>()
                    })
                    .unwrap_or_default(),
            );
        }
        assert_eq!(inked[0], 0, "a zero sweep draws no arc at all");
        for window in inked.windows(2) {
            assert!(
                window[1] > window[0],
                "sweep coverage must be monotonic, got {inked:?}"
            );
        }
    }

    /// PIN (T2 progress ring): the ring fills the slot it replaces, clips
    /// nowhere, and stays hollow.
    ///
    /// The ring takes the mark's own box (user ruling), so its stroke has to
    /// live entirely inside it: a stroke straddles its path, and a radius
    /// chosen as if it did not shaves the ring flat against all four edges.
    #[test]
    fn the_ring_fills_its_slot_without_clipping_and_stays_hollow() {
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[ring(1000, 4, 30.0)]);
        let icon = &icons[0];
        let centre = icon.width_px / 2;
        // Noon, 3, 6 and 9 o'clock each reach the box's own edge.
        assert!(
            alpha_at(icon, centre, 0) > 0,
            "the ring is clipped at the top"
        );
        assert!(
            alpha_at(icon, centre, icon.height_px - 1) > 0,
            "the ring is clipped at the bottom"
        );
        assert!(
            alpha_at(icon, 0, centre) > 0,
            "the ring is clipped at the left"
        );
        assert!(
            alpha_at(icon, icon.width_px - 1, centre) > 0,
            "the ring is clipped at the right"
        );
        assert_eq!(
            alpha_at(icon, centre, centre),
            0,
            "the middle of a ring is a hole, not a fill"
        );
        assert_eq!(alpha_at(icon, 0, 0), 0, "a ring has no corners");
    }

    /// PIN (T2 unread dot): the dot is a circle, and the chrome already had a
    /// primitive for it.
    ///
    /// `.unreaddot { width: 6px; height: 6px; border-radius: 50% }` is a square
    /// with its round set to half its side, which is exactly what `ControlPill`
    /// clamps to — so the dot needs no shape of its own, and giving it one
    /// would be a second circle to keep in step with the first.
    #[test]
    fn the_unread_dot_is_a_control_pill_rounded_into_a_circle() {
        let accent = [0x7a, 0x99, 0xff];
        let side = 12.0_f32;
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[sprite(
            ChromeMark::ControlPill {
                radius_px: side as u32 / 2,
            },
            side,
            side,
            accent,
        )]);
        let icon = &icons[0];
        let centre = icon.width_px / 2;
        assert_eq!(alpha_at(icon, centre, centre), 255, "the dot is filled");
        assert_eq!(
            rgb_at(icon, centre, centre),
            accent,
            "the dot wears its claim"
        );
        for (x, y) in [(0, 0), (icon.width_px - 1, 0), (0, icon.height_px - 1)] {
            assert_eq!(
                alpha_at(icon, x, y),
                0,
                "a circle has no corner at ({x},{y})"
            );
        }
        assert!(
            alpha_at(icon, centre, 0) > 0,
            "the dot must reach its own top"
        );
        assert!(
            alpha_at(icon, 0, centre) > 0,
            "the dot must reach its own left"
        );
    }

    /// PIN (T2 dead marks): `filter: grayscale(1)` (mock-up line 285) is baked
    /// into the raster, and it is part of the mark's content identity.
    ///
    /// It has to be baked, unlike opacity: desaturating changes which colour
    /// each pixel is, and the mark this lands on is the profile mark, which
    /// carries colours of its own that no palette entry can stand in for.
    /// Because it changes the pixels it must also change the key — two rasters
    /// that differ while sharing a key is the one cache bug that shows up as
    /// the wrong picture.
    #[test]
    fn a_dead_mark_is_desaturated_in_the_raster_and_keyed_apart() {
        let accent = [0x7a, 0x99, 0xff];
        let live = sprite(ChromeMark::ProfilePowerShell, 30.0, 30.0, accent);
        let mut dead = live;
        dead.grayscale = true;
        dead.rect = [40.0, 0.0, 70.0, 30.0];
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[live, dead]);
        assert_eq!(
            icons.len(),
            2,
            "two marks, not one shared by a colliding key"
        );
        assert_ne!(
            icons[0].key, icons[1].key,
            "a desaturated mark is different pixels and needs a different key"
        );
        // The PowerShell mark's panel is a saturated blue; dead, it must be grey.
        let (x, y) = (icons[0].width_px / 5, icons[0].height_px / 2);
        let [r, g, b] = rgb_at(&icons[0], x, y);
        assert!(
            u32::from(b) > u32::from(r) + 24,
            "the living mark is blue ({r},{g},{b})"
        );
        let [r, g, b] = rgb_at(&icons[1], x, y);
        let spread = u32::from(r.max(g).max(b)) - u32::from(r.min(g).min(b));
        assert!(spread <= 2, "a dead mark keeps no hue, got ({r},{g},{b})");
    }

    /// PIN (T2 breathing): a sprite's opacity rides to the renderer on the icon
    /// and never into the cache key.
    ///
    /// Two marks differing only in how faded they are must be the *same* raster,
    /// or a 1.7s breath mints a fresh texture on every frame it is drawn. This
    /// is the assertion that keeps the breath free.
    #[test]
    fn opacity_travels_on_the_icon_and_never_into_the_cache_key() {
        let accent = [0x7a, 0x99, 0xff];
        let full = sprite(ChromeMark::Folder, 26.0, 26.0, accent);
        let mut faded = full;
        faded.opacity = 0.28;
        faded.rect = [40.0, 0.0, 66.0, 26.0];
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[full, faded]);
        assert_eq!(icons[0].key, icons[1].key, "one mark, one raster");
        assert!(
            Arc::ptr_eq(&icons[0].rgba, &icons[1].rgba),
            "the faded mark must reuse the pixels, not re-rasterize them"
        );
        assert_eq!(icons[0].opacity, 1.0);
        assert_eq!(icons[1].opacity, 0.28);
        // The raster itself is untouched by the fade — the alpha the renderer
        // multiplies is still the mark's own coverage.
        assert_eq!(
            alpha_at(&icons[1], icons[1].width_px / 2, icons[1].height_px * 2 / 3),
            255
        );
    }

    /// F63: a card corner fills the floor *outside* the card's round, and each
    /// of the four faces its own way.
    ///
    /// The shape is the one thing about this mark that can be wrong without
    /// looking wrong in a count of sprites, so it is checked where it matters:
    /// the box's outer corner — the one pointing away from the card — is solid
    /// floor, and the diagonally opposite pixel, which is deep inside the card,
    /// is empty. Getting a sweep or a centre backwards swaps exactly those two.
    ///
    /// Red gate: give every orientation the same centre and three of the four
    /// pairs invert.
    #[test]
    fn a_card_corner_fills_the_floor_outside_the_round_and_faces_its_own_way() {
        let floor = [0x25, 0x25, 0x25];
        let radius = 12u32;
        let last = radius - 1;
        // (corner, the pixel pointing away from the card, the pixel inside it)
        let cases = [
            (Corner::TopLeft, (0, 0), (last, last)),
            (Corner::TopRight, (last, 0), (0, last)),
            (Corner::BottomLeft, (0, last), (last, 0)),
            (Corner::BottomRight, (last, last), (0, 0)),
        ];
        let mut rasters = ChromeMarkRasters::default();
        for (corner, outside, inside) in cases {
            let mark = ChromeMark::CardCorner {
                radius_px: radius,
                corner,
            };
            let icons = rasters.resolve(&[sprite(mark, radius as f32, radius as f32, floor)]);
            let icon = icons.first().expect("a card corner rasterizes");
            assert_eq!(icon.width_px, radius);
            assert_eq!(icon.height_px, radius);
            assert_eq!(
                alpha_at(icon, outside.0, outside.1),
                255,
                "{corner:?}: the corner away from the card is floor"
            );
            assert_eq!(
                rgb_at(icon, outside.0, outside.1),
                floor,
                "{corner:?}: and it wears the colour it was handed"
            );
            assert_eq!(
                alpha_at(icon, inside.0, inside.1),
                0,
                "{corner:?}: and the card's own area is not painted over"
            );
        }
        // Four orientations, four cache slots: one id with the corner in the
        // key, the way the chevron keeps one id per angle.
        let all: Vec<_> = [
            Corner::TopLeft,
            Corner::TopRight,
            Corner::BottomLeft,
            Corner::BottomRight,
        ]
        .into_iter()
        .map(|corner| {
            sprite(
                ChromeMark::CardCorner {
                    radius_px: radius,
                    corner,
                },
                radius as f32,
                radius as f32,
                floor,
            )
        })
        .collect();
        let icons = rasters.resolve(&all);
        let keys: std::collections::HashSet<_> = icons.iter().map(|icon| &icon.key).collect();
        assert_eq!(
            keys.len(),
            4,
            "four different bites are four different keys"
        );
    }

    /// PIN (visual fidelity pass): every mark the chrome wears is a real raster
    /// with ink in it, at exactly the physical box asked for, and a mark that
    /// takes `currentColor` wears the colour it was handed.
    ///
    /// Red gate: a placeholder block would be opaque *everywhere*, so each mark
    /// is also required to leave transparent pixels — that is what tells a glyph
    /// apart from the filled square this pass exists to delete.
    #[test]
    fn every_chrome_mark_rasterizes_to_its_box_with_ink_and_negative_space() {
        let accent = [0x7a, 0x99, 0xff];
        let cases = [
            (ChromeMark::Gear, 14.0_f32),
            (ChromeMark::WindowMinimize, 10.0),
            (ChromeMark::WindowMaximize, 10.0),
            (ChromeMark::WindowClose, 10.0),
            (ChromeMark::TabClose, 8.0),
            // The same source at the same size, on a caption rather than on the
            // strip: `.panehead .pane-close svg { width: 8px; height: 8px }`.
            (ChromeMark::PaneClose, 8.0),
            (ChromeMark::Plus, 10.0),
            (ChromeMark::ProfilePowerShell, 15.0),
            // The other three profiles wear the same 15px `.pmark` slot.
            (ChromeMark::ProfileUbuntu, 15.0),
            (ChromeMark::ProfileGit, 15.0),
            (ChromeMark::ProfileCmd, 15.0),
            (ChromeMark::File, 14.0),
            // The page's mark, at the file mark's size: they share one column.
            (ChromeMark::Globe, 14.0),
            (ChromeMark::Folder, 13.0),
            (ChromeMark::Panel, 13.0),
            // `.panehead .pane-split svg { width: 13px }` — the folder's size,
            // and it is asked at the folder's size because the two stand in one
            // run and are read together or not at all.
            (ChromeMark::Split, 13.0),
            // The menu's icon column is 14 (`ITEM_ICON_COLUMN_LOGICAL_PX`), and
            // these two are only ever drawn there.
            (ChromeMark::SplitRight, 14.0),
            (ChromeMark::SplitDown, 14.0),
            // 13 in a 17px box, deliberately not the close mark's 8 (mock-up
            // lines 362-365): the pin carries a state and a glyph that has to
            // survive a 45° turn, and both cost silhouette.
            (ChromeMark::Pin { filled: false }, 13.0),
            (ChromeMark::Pin { filled: true }, 13.0),
            // The preview head's own control, in the 13px the head's tools wear
            // — it stands in a run with `#i-save` and `#i-float` and is read
            // with them or not at all.
            (ChromeMark::Lock { engaged: false }, 13.0),
            (ChromeMark::Lock { engaged: true }, 13.0),
        ];
        for (mark, size) in cases {
            for scale in [1.0_f32, 1.5, 2.0] {
                let side = (size * scale).round();
                let mut rasters = ChromeMarkRasters::default();
                let icons = rasters.resolve(&[sprite(mark, side, side, accent)]);
                let icon = icons
                    .first()
                    .unwrap_or_else(|| panic!("{mark:?} must rasterize at {side}px"));
                assert_eq!(
                    (icon.width_px, icon.height_px),
                    (side as u32, side as u32),
                    "{mark:?} must fill exactly the box it was given"
                );
                assert_eq!(
                    icon.rgba.len(),
                    (side as usize) * (side as usize) * 4,
                    "{mark:?} raster is not its own dimensions"
                );
                let inked = icon.rgba.chunks_exact(4).filter(|p| p[3] > 0).count();
                let clear = icon.rgba.chunks_exact(4).filter(|p| p[3] == 0).count();
                let strongest = icon
                    .rgba
                    .chunks_exact(4)
                    .map(|p| p[3])
                    .max()
                    .unwrap_or_default();
                assert!(inked > 0, "{mark:?} at {side}px drew nothing");
                // A one-pixel hairline centred on a pixel boundary splits its
                // coverage over two rows, so the floor is "clearly visible",
                // not "opaque" — but a mark that is all haze is still a bug.
                assert!(
                    strongest >= 100,
                    "{mark:?} at {side}px is too faint to read ({strongest})"
                );
                assert!(
                    clear > 0,
                    "{mark:?} at {side}px is a solid block — the placeholder this pass removes"
                );
            }
        }
    }

    /// PIN: `currentColor` marks answer to the palette, and a profile mark does
    /// not — the mock-up rules that a mark carries its own colour (`.pmark`).
    #[test]
    fn current_color_marks_take_the_palette_and_a_profile_mark_keeps_its_own() {
        let accent = [0x7a, 0x99, 0xff];
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[
            sprite(ChromeMark::Folder, 26.0, 26.0, accent),
            sprite(ChromeMark::ProfilePowerShell, 30.0, 30.0, accent),
        ]);
        let folder = &icons[0];
        // Dead centre of a filled folder body is solid accent.
        let centre = rgb_at(folder, folder.width_px / 2, folder.height_px * 2 / 3);
        assert_eq!(
            alpha_at(folder, folder.width_px / 2, folder.height_px * 2 / 3),
            255
        );
        assert_eq!(centre, accent, "a folder mark is drawn in --accent");

        let pwsh = &icons[1];
        let panel = rgb_at(pwsh, pwsh.width_px / 5, pwsh.height_px / 2);
        assert_ne!(panel, accent, "a profile mark is never recoloured");
        assert_eq!(
            panel,
            [0x2c, 0x5c, 0x9e],
            "the PowerShell panel keeps the mock-up's own #2C5C9E"
        );
    }

    /// PIN — **all four** profile marks carry their own colours, and each one
    /// carries its *own*.
    ///
    /// Two claims, and the second is the one that needed a test. The first is
    /// read off the pixels: at a 32px raster a `viewBox="0 0 16 16"` unit is
    /// exactly two pixels, so each sample below is a named point of the mock-up's
    /// own artwork — the PowerShell panel, the Ubuntu disc inside its ring, the
    /// Git lozenge clear of its branch, the Command Prompt panel left of its
    /// chevron — and each must come back as the literal the design states.
    ///
    /// The second is read off the **cache key**, and that is the red gate.
    /// `takes_current_color` used to be spelled `self != ProfilePowerShell`,
    /// which is the right sentence only while there is one profile mark. Under
    /// that spelling the three marks added here would be declared
    /// palette-following — and *nothing on screen would change*, because their
    /// bodies contain no `currentColor` for the substitution to find. The only
    /// visible consequence is in `mark_key`, which folds the ink into the key
    /// for a palette-following mark: the same orange disc drawn beside two
    /// different accents would be rasterized and cached twice, forever, in
    /// silence. So the assertion is that one mark under two inks is one key.
    #[test]
    fn every_profile_mark_paints_itself_and_caches_once_whatever_ink_it_is_handed() {
        // 32px over a 16-unit box: one unit is two pixels, so a sample point can
        // be stated in the mock-up's own coordinates.
        const SIDE: f32 = 32.0;
        let unit = |u: f32| (u * SIDE / 16.0) as u32;
        let cases = [
            // The panel, left of the chevron that starts at x=4.4.
            (
                ChromeMark::ProfilePowerShell,
                (3.0, 8.0),
                [0x2c, 0x5c, 0x9e],
            ),
            // Dead centre of the disc: the ring is `fill="none"` at r=4.1 and the
            // three friends sit on it, so the middle is bare orange.
            (ChromeMark::ProfileUbuntu, (8.0, 8.0), [0xe9, 0x54, 0x20]),
            // Inside the lozenge and clear of both the branch and its nodes.
            (ChromeMark::ProfileGit, (8.0, 3.6), [0xf0, 0x50, 0x33]),
            // The charcoal panel — *not* console black, by the mock-up's ruling.
            (ChromeMark::ProfileCmd, (3.0, 8.0), [0x3a, 0x3a, 0x3a]),
            // The chassis a profile of the user's own wears: the same panel
            // again, filled from the eight. It joins this list because it is a
            // profile mark and the rule is stated over the *family* — a mark
            // struck in Amber that quietly took the accent instead would be the
            // very bug this case's second half describes.
            (
                ChromeMark::ProfileGeneric {
                    colour: MarkColour::Amber,
                },
                (3.0, 8.0),
                [0xb0, 0x7a, 0x16],
            ),
        ];
        for (mark, (x, y), expected) in cases {
            let inks = [[0x7a, 0x99, 0xff], [0xff, 0x00, 0x00]];
            let mut rasters = ChromeMarkRasters::default();
            let icons: Vec<_> = inks
                .iter()
                .map(|ink| rasters.resolve(&[sprite(mark, SIDE, SIDE, *ink)]).remove(0))
                .collect();
            for (ink, icon) in inks.iter().zip(&icons) {
                assert_eq!(
                    rgb_at(icon, unit(x), unit(y)),
                    expected,
                    "{mark:?} at unit ({x}, {y}) must be its own colour, not {ink:?}"
                );
                assert_eq!(
                    alpha_at(icon, unit(x), unit(y)),
                    255,
                    "{mark:?} at unit ({x}, {y}) must be solid artwork, not an edge"
                );
            }
            assert_eq!(
                icons[0].key, icons[1].key,
                "{mark:?} paints itself, so the ink must not reach its cache key"
            );
        }

        // And the four are four different drawings, not one drawing four times.
        let ink = [0x7a, 0x99, 0xff];
        let mut rasters = ChromeMarkRasters::default();
        let marks = [
            ChromeMark::ProfilePowerShell,
            ChromeMark::ProfileUbuntu,
            ChromeMark::ProfileGit,
            ChromeMark::ProfileCmd,
        ];
        let icons = rasters.resolve(
            &marks
                .iter()
                .map(|mark| sprite(*mark, SIDE, SIDE, ink))
                .collect::<Vec<_>>(),
        );
        let keys: std::collections::HashSet<_> = icons.iter().map(|icon| &icon.key).collect();
        assert_eq!(keys.len(), marks.len(), "four profiles, four rasters");
        for (index, left) in icons.iter().enumerate() {
            for right in &icons[index + 1..] {
                assert_ne!(
                    left.rgba, right.rgba,
                    "two profile marks rasterized to the same pixels"
                );
            }
        }
    }

    /// PIN — **eight colours are eight rasters under one id**, and each panel is
    /// the hex the mock-up struck while the chevron over it stays white.
    ///
    /// Two red gates in one case, and they fail in opposite directions.
    ///
    /// Leave the colour out of [`mark_key`] and all eight collapse into one
    /// cache slot: the first profile drawn wins, and every other profile of the
    /// user's own wears its colour for the rest of the process — silently,
    /// because nothing about the drawing is wrong, only which drawing was kept.
    /// That is the same failure `ProgressRing` and `CardCorner` add their own
    /// parameters to the key to avoid.
    ///
    /// Send the fill through the `currentColor` substitution instead of writing
    /// the hex into the document, and the mark follows the theme — which is the
    /// one thing a profile mark is forbidden to do (mock-up line 2148), and
    /// which `takes_current_color` answering `false` for this variant is what
    /// prevents.
    ///
    /// The white chevron is asserted because it is the part the eight have in
    /// common: a chassis that tinted its glyph as well as its panel would go
    /// illegible on the pale end of the palette, and would stop being the
    /// drawing `#p-pwsh` and `#p-cmd` already are twice.
    #[test]
    fn the_chassis_a_profile_of_your_own_wears_is_eight_panels_under_one_white_chevron() {
        const SIDE: f32 = 32.0;
        let unit = |u: f32| (u * SIDE / 16.0) as u32;
        let ink = [0x7a, 0x99, 0xff];
        let mut rasters = ChromeMarkRasters::default();
        let sprites: Vec<_> = MarkColour::ALL
            .iter()
            .map(|colour| {
                sprite(
                    ChromeMark::ProfileGeneric { colour: *colour },
                    SIDE,
                    SIDE,
                    ink,
                )
            })
            .collect();
        let icons = rasters.resolve(&sprites);
        let keys: std::collections::HashSet<_> = icons.iter().map(|icon| &icon.key).collect();
        assert_eq!(
            keys.len(),
            MarkColour::ALL.len(),
            "eight colours, eight rasters: {keys:?}"
        );
        for key in &keys {
            assert!(
                key.contains("p-shell"),
                "and all eight are the one chassis: {key}"
            );
        }
        for (colour, icon) in MarkColour::ALL.iter().zip(&icons) {
            // Left of the chevron, which starts at x=4.4: solid panel.
            assert_eq!(
                rgb_at(icon, unit(3.0), unit(8.0)),
                colour.hex(),
                "{colour:?}'s panel must be its own struck hex"
            );
            // The underline at y=10.9 runs x=8.5..11.7 — white, on every one.
            assert_eq!(
                rgb_at(icon, unit(10.0), unit(10.9)),
                [0xff, 0xff, 0xff],
                "{colour:?}'s glyph must stay white"
            );
        }
    }

    /// PIN — the wire words and the drawing agree, both ways, for all eight.
    ///
    /// Red gate: add a ninth colour to [`MarkColour::ALL`] and forget one of the
    /// four `match` arms, or spell a wire word twice. Either leaves a colour a
    /// `profiles.json` can name but the window cannot draw, or two colours the
    /// file cannot tell apart.
    #[test]
    fn every_struck_colour_can_be_written_down_and_read_back() {
        let mut seen = std::collections::HashSet::new();
        for colour in MarkColour::ALL {
            assert!(seen.insert(colour.wire()), "{colour:?} shares a wire word");
            assert_eq!(MarkColour::from_wire(colour.wire()), Some(colour));
            assert!(
                colour
                    .wire()
                    .chars()
                    .all(|glyph| glyph.is_ascii_lowercase()),
                "{colour:?}: the drawn word and the written one are one name"
            );
        }
        assert_eq!(
            MarkColour::from_wire("chartreuse"),
            None,
            "a colour this build has not struck is a key to skip, not a file to refuse"
        );
    }

    /// PIN (tab shape): the active tab is round on top, square-cut at the
    /// bottom, and flares outward into the content plane at both lower corners.
    ///
    /// Every claim is read off the raster rather than off the path string:
    ///
    /// * the top-left corner pixel is empty and the pixel a radius in is full —
    ///   that is the `--tabr` round;
    /// * the bottom row is solid across the tab body *and* keeps going past both
    ///   of its sides into the skirts, so the tab and the surface below it are
    ///   one continuous field of `--termbg`;
    /// * the pixel at the skirt's own circle centre is *empty* — the flare is
    ///   concave, which is what a nested-rectangle staircase can never be, and
    ///   what distinguishes this from a plain rounded rectangle.
    #[test]
    fn the_active_tab_silhouette_is_round_on_top_and_flares_into_the_content_plane() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let radius = (7.0 * scale).round() as u32;
            let height = (34.0 * scale).round() as u32;
            let width = (200.0 * scale).round() as u32 + 2 * radius;
            let mut rasters = ChromeMarkRasters::default();
            let icons = rasters.resolve(&[ChromeSprite::new(
                ChromeMark::ActiveTab { radius_px: radius },
                [0.0, 0.0, width as f32, height as f32],
                [0x1b, 0x1b, 0x1b],
            )]);
            let tab = icons.first().expect("the tab silhouette must rasterize");
            let last_row = height - 1;

            // Round on top: the tab body starts at x = radius, and its own
            // top-left corner is cut away.
            assert_eq!(
                alpha_at(tab, radius, 0),
                0,
                "scale {scale}: the tab's top-left corner pixel must be outside the round"
            );
            assert_eq!(
                alpha_at(tab, 2 * radius, 0),
                255,
                "scale {scale}: one radius in, the top edge is solid"
            );
            assert_eq!(
                alpha_at(tab, width - 2 * radius - 1, 0),
                255,
                "scale {scale}: the top edge is solid up to the right round"
            );
            assert_eq!(
                alpha_at(tab, width - radius - 1, 0),
                0,
                "scale {scale}: the tab's top-right corner is cut away too"
            );

            // Square-cut at the bottom: the tab body's own span is solid on the
            // last row, with no rounding and no hairline of its own.
            for x in [radius, width / 2, width - radius - 1] {
                assert_eq!(
                    alpha_at(tab, x, last_row),
                    255,
                    "scale {scale}: the bottom edge must be unbroken at x={x} — \
                     the tab and the content plane are one surface"
                );
            }
            // Red gate: a plain rounded rectangle ends at the body's own sides,
            // so these two pixels — one on each skirt — would be empty.
            assert_eq!(
                alpha_at(tab, radius - 1, last_row),
                255,
                "scale {scale}: the left skirt must carry the fill past the tab's own side"
            );
            assert_eq!(
                alpha_at(tab, width - radius, last_row),
                255,
                "scale {scale}: the right skirt must carry the fill past the tab's own side"
            );
            // And the flare thins out towards the silhouette's outer corner
            // rather than stopping short of it: one pixel in from either end of
            // the bottom row still carries ink, and it is *partial* ink, which
            // is the curve tapering to the corner point.
            for (side, x) in [("left", 1), ("right", width - 2)] {
                let alpha = alpha_at(tab, x, last_row);
                assert!(
                    alpha > 0 && alpha < 255,
                    "scale {scale}: the {side} skirt must taper to the outer corner, saw {alpha}"
                );
            }

            // Concave, not convex: the skirt's gradient circle is centred at the
            // silhouette's outer bottom corner, so the pixel at that centre is
            // empty on both sides. A convex corner would fill it.
            assert_eq!(
                alpha_at(tab, 0, height - radius),
                0,
                "scale {scale}: the left skirt must curve away from its own corner"
            );
            assert_eq!(
                alpha_at(tab, width - 1, height - radius),
                0,
                "scale {scale}: the right skirt must curve away from its own corner"
            );
        }
    }

    /// The corners are antialiased rather than stepped: along the top-left
    /// round there are partial-coverage pixels, which is the whole difference
    /// between an analytic curve and the nested quads it replaces.
    #[test]
    fn the_tab_corners_carry_partial_coverage_instead_of_a_staircase() {
        let radius = 14_u32;
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[ChromeSprite::new(
            ChromeMark::ActiveTab { radius_px: radius },
            [0.0, 0.0, 240.0, 68.0],
            [0x1b, 0x1b, 0x1b],
        )]);
        let tab = &icons[0];
        let partial = (0..radius * 2)
            .flat_map(|y| (0..radius * 2).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let alpha = alpha_at(tab, *x, *y);
                alpha > 0 && alpha < 255
            })
            .count();
        assert!(
            partial >= radius as usize,
            "an antialiased quarter-circle spends at least one partial pixel per row, saw {partial}"
        );
    }

    /// PIN (`+` hover fidelity) — a control's hover fill is **round**, and the
    /// round is analytic: its corners spend partial-coverage pixels, which is
    /// the whole difference between a curve and the staircase a stack of nested
    /// rectangles leaves.
    ///
    /// Red gate: this is the shape the `+` did not have. Drawn as a
    /// `ChromeQuad` its corner pixel was fully opaque — assertion one — and
    /// there were no partial pixels anywhere in it — assertion three.
    #[test]
    fn a_control_pill_is_round_at_its_corners_and_solid_in_its_middle() {
        for (side, radius) in [(28_u32, 6_u32), (42, 9), (56, 12), (17, 4), (34, 8)] {
            let mut rasters = ChromeMarkRasters::default();
            let icons = rasters.resolve(&[sprite(
                ChromeMark::ControlPill { radius_px: radius },
                side as f32,
                side as f32,
                [0x33, 0x44, 0x55],
            )]);
            let pill = icons.first().expect("a pill must rasterize");
            assert_eq!((pill.width_px, pill.height_px), (side, side));

            // Round: the box's own corner pixel is outside the shape, and a
            // radius in from it the fill is solid.
            for (x, y) in [(0, 0), (side - 1, 0), (0, side - 1), (side - 1, side - 1)] {
                assert_eq!(
                    alpha_at(pill, x, y),
                    0,
                    "radius {radius}: the pill's corner pixel ({x},{y}) must be cut away"
                );
            }
            assert_eq!(alpha_at(pill, side / 2, side / 2), 255);
            assert_eq!(
                alpha_at(pill, side / 2, 0),
                255,
                "radius {radius}: the top edge between the corners is solid"
            );
            assert_eq!(alpha_at(pill, 0, side / 2), 255);

            // Analytic, not stepped: a quarter circle spends at least one
            // partial pixel per row it crosses.
            let partial = (0..radius)
                .flat_map(|y| (0..radius).map(move |x| (x, y)))
                .filter(|(x, y)| {
                    let alpha = alpha_at(pill, *x, *y);
                    alpha > 0 && alpha < 255
                })
                .count();
            assert!(
                partial >= radius as usize,
                "radius {radius}: a rounded corner is not a staircase, saw {partial} partial pixels"
            );
        }
    }

    /// PIN (Q172/Q175) — the rail's silhouette is round on **all four** corners,
    /// and the strip's is not. One assertion pair, because the whole bug was
    /// reaching for the wrong one of the two.
    ///
    /// Red gate: this is what a rail row and the rail's `New tab` button were
    /// drawn with. `.vtab` and `.newtab` are `border-radius: 6px` — a closed box
    /// in a column — while `.tab` is `var(--tabr) var(--tabr) 0 0`, square-footed
    /// because a tab runs into the terminal and has no bottom edge. Painted with
    /// `TabBody`, the rail's fills came out with two square corners under a
    /// design that rounds them, which at the foot of the panel reads as a button
    /// somebody cropped — which is exactly how it was reported.
    #[test]
    fn the_column_and_the_strip_are_not_the_same_silhouette() {
        let (side, radius) = (30_u32, 6_u32);
        let raster = |mark| {
            let mut rasters = ChromeMarkRasters::default();
            let icons =
                rasters.resolve(&[sprite(mark, side as f32, side as f32, [0x33, 0x44, 0x55])]);
            icons.into_iter().next().expect("a body must rasterize")
        };
        let column = raster(ChromeMark::ControlPill { radius_px: radius });
        let strip = raster(ChromeMark::TabBody { radius_px: radius });
        for (x, y) in [(0, side - 1), (side - 1, side - 1)] {
            assert_eq!(
                alpha_at(&column, x, y),
                0,
                "`.vtab {{ border-radius: 6px }}` — the foot is cut away at ({x},{y})"
            );
            assert_eq!(
                alpha_at(&strip, x, y),
                255,
                "and `.tab`'s foot is square at ({x},{y}), on purpose — it joins the terminal"
            );
        }
        for (x, y) in [(0, 0), (side - 1, 0)] {
            assert_eq!(alpha_at(&column, x, y), 0, "both are round at the head");
            assert_eq!(alpha_at(&strip, x, y), 0);
        }
    }

    /// PIN (K121 in the rail) — the landing ring follows the fill it rings.
    ///
    /// `@keyframes tab-land` is an *inset* `box-shadow`, so the ring is hollow,
    /// lands inside its own edge, and — on this axis — closes across a foot the
    /// strip's ring deliberately leaves open.
    #[test]
    fn a_pill_ring_is_hollow_inset_and_closed_all_the_way_round() {
        let (side, radius, stroke) = (30_u32, 6_u32, 2_u32);
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[
            sprite(
                ChromeMark::ControlPillRing {
                    radius_px: radius,
                    stroke_px: stroke,
                },
                side as f32,
                side as f32,
                [0x33, 0x44, 0x55],
            ),
            sprite(
                ChromeMark::TabBodyRing {
                    radius_px: radius,
                    stroke_px: stroke,
                },
                side as f32,
                side as f32,
                [0x33, 0x44, 0x55],
            ),
        ]);
        let pill = icons.first().expect("a pill ring must rasterize");
        let tab = icons.get(1).expect("and its square-footed sibling");
        assert_eq!(
            alpha_at(pill, side / 2, side / 2),
            0,
            "a ring is hollow — `box-shadow: inset 0 0 0 Npx` paints an edge, not a fill"
        );
        assert_eq!(
            alpha_at(pill, side / 2, 0),
            255,
            "and it lands inside its own top edge"
        );
        assert_eq!(
            alpha_at(pill, side / 2, side - 1),
            255,
            "and closes across the foot, because `.vtab` has one"
        );
        assert_eq!(
            alpha_at(tab, side / 2, side - 1),
            0,
            "which is precisely where the strip's ring stops: a tab has no bottom edge, \
             and a ring closed across that join would draw a line through it"
        );
        for (x, y) in [(0, 0), (side - 1, 0), (0, side - 1), (side - 1, side - 1)] {
            assert_eq!(
                alpha_at(pill, x, y),
                0,
                "the ring's own corner ({x},{y}) is round, like the fill it rings"
            );
        }
        // Two shapes under one id would be one cache slot, and the second ring on
        // screen would silently wear the first one's pixels.
        assert_ne!(pill.key, tab.key, "and the two are two rasters, not one");
    }

    /// PIN — a pill is **opaque**, and the two radii the chrome uses are two
    /// rasters rather than one stretched.
    ///
    /// Opacity is the claim that matters. A translucent fill would have to be
    /// blended by the pipeline, and the pipeline blends in *linear* light while
    /// the design's renderer blends in sRGB: handing it `--active`'s own .09
    /// over the dark tab lands at 89 where the mock-up puts 48. Every chrome
    /// fill is pre-composited for exactly that reason, and this one is no
    /// exception.
    #[test]
    fn a_pill_is_opaque_and_each_radius_is_its_own_raster() {
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[
            sprite(
                ChromeMark::ControlPill { radius_px: 4 },
                17.0,
                17.0,
                [0xed, 0xed, 0xec],
            ),
            sprite(
                ChromeMark::ControlPill { radius_px: 6 },
                17.0,
                17.0,
                [0xed, 0xed, 0xec],
            ),
        ]);
        for icon in &icons {
            assert_eq!(
                alpha_at(icon, 8, 8),
                255,
                "a chrome fill carries no alpha of its own"
            );
            assert_eq!(rgb_at(icon, 8, 8), [0xed, 0xed, 0xec]);
        }
        assert_ne!(icons[0].key, icons[1].key, "the radius is part of identity");
    }

    /// The ink a mark carries on one row of its raster.
    fn row_ink(icon: &ChromeIcon, y: u32) -> u32 {
        (0..icon.width_px)
            .map(|x| u32::from(alpha_at(icon, x, y)))
            .sum()
    }

    /// The tight box around a mark's ink — `[left, top, right, bottom]`,
    /// inclusive — which is how wide and how tall the drawing actually is
    /// rather than how big a box it was handed.
    fn ink_box(icon: &ChromeIcon) -> [u32; 4] {
        let mut bounds: Option<[u32; 4]> = None;
        for y in 0..icon.height_px {
            for x in 0..icon.width_px {
                if alpha_at(icon, x, y) == 0 {
                    continue;
                }
                bounds = Some(match bounds {
                    None => [x, y, x, y],
                    Some([left, top, right, bottom]) => {
                        [left.min(x), top.min(y), right.max(x), bottom.max(y)]
                    }
                });
            }
        }
        bounds.expect("the mark drew something")
    }

    fn ink_span(icon: &ChromeIcon) -> (u32, u32) {
        let [left, top, right, bottom] = ink_box(icon);
        (right - left + 1, bottom - top + 1)
    }

    /// A whole turn, sampled on the quantum's own grid so every frame is a
    /// distinct angle and none is a rounding of its neighbour.
    fn turn_sweep(ink: [u8; 3], width: f32, height: f32) -> Vec<ChromeIcon> {
        let steps = i32::from(CHEVRON_TURN_STEPS) - 1;
        let sprites: Vec<ChromeSprite> = (0..=steps)
            .map(|step| {
                let turn = step as f32 / steps as f32;
                sprite(ChromeMark::chevron(turn), width, height, ink)
            })
            .collect();
        ChromeMarkRasters::default().resolve(&sprites)
    }

    /// PIN — the chevron is ONE arrow that turns over, and it turns *through*
    /// the angles between rather than jumping between two drawings.
    ///
    /// `.chevbtn svg { transition: transform 140ms cubic-bezier(.2,0,0,1) }`
    /// with `.chevbtn.open svg { transform: rotate(180deg) }` (mock-up
    /// 415-420). The symbol sheet holds one `#i-chev` and there is no second
    /// one to swap to, so the mark is keyed by an *angle* and not by a state,
    /// and every angle in between is a frame that really exists.
    ///
    /// Red gate: the assertions a swap cannot pass are the ones about the
    /// middle. Two glyphs traded at the halfway point give a mark that is
    /// always wider than it is tall and that has exactly two shapes; a real
    /// rotation stands the arrow on end at 90°, where it is taller than it is
    /// wide, and walks there in steps no bigger than a pixel or two. Delete the
    /// rotation and keep the two end frames and the count of distinct shapes
    /// falls from dozens to two.
    #[test]
    fn the_chevron_turns_over_instead_of_swapping_glyphs() {
        let ink = [0x9d, 0x9d, 0x9d];
        // One glyph, one name for it: what tells two frames apart is the angle.
        assert_eq!(ChromeMark::chevron(0.0).id(), ChromeMark::chevron(1.0).id());
        assert_eq!(ChromeMark::chevron(0.0).id(), "i-chev");
        assert_eq!(
            ChromeMark::chevron(0.0),
            ChromeMark::Chevron { turned_degrees: 0 },
            "at rest the arrow is the symbol as drawn"
        );
        assert_eq!(
            ChromeMark::chevron(1.0),
            ChromeMark::Chevron {
                turned_degrees: 180
            },
            "`.chevbtn.open svg {{ transform: rotate(180deg) }}`"
        );

        let icons = turn_sweep(ink, 18.0, 12.0);
        let last = icons.len() - 1;
        let (down, side, up) = (&icons[0], &icons[last / 2], &icons[last]);
        assert_ne!(down.key, up.key);

        // A down chevron carries its two arms at the top and its point at the
        // bottom; the turn swaps which row is which.
        let inked_rows = |icon: &ChromeIcon| {
            let [_, top, _, bottom] = ink_box(icon);
            (top, bottom)
        };
        let (down_top, down_bottom) = inked_rows(down);
        assert!(
            row_ink(down, down_top + 1) > row_ink(down, down_bottom - 1),
            "a resting chevron points down"
        );
        let (up_top, up_bottom) = inked_rows(up);
        assert!(
            row_ink(up, up_bottom - 1) > row_ink(up, up_top + 1),
            "a fully turned chevron points up"
        );

        // The two ends are the same glyph mirrored, because 180° about the
        // box's centre *is* that mirror — and the raster's padding is
        // symmetric, so the mirror is the raster's own mid-line.
        for y in 0..down.height_px {
            let mirrored = down.height_px - 1 - y;
            let (a, b) = (row_ink(down, y), row_ink(up, mirrored));
            assert!(
                a.abs_diff(b) * 20 <= a.max(b).max(1) * 3,
                "row {y} of the turned arrow is not row {mirrored} of the resting one \
                 ({a} against {b}) — it is a second glyph, not the same one rotated"
            );
        }

        // Halfway through, the arrow is standing on end. Nothing that swaps two
        // 9x6 drawings ever produces this frame.
        let (down_w, down_h) = ink_span(down);
        let (side_w, side_h) = ink_span(side);
        assert!(
            down_w > down_h,
            "the resting arrow lies across its box, saw {down_w}x{down_h}"
        );
        assert!(
            side_h > side_w,
            "at 90° the arrow stands on end, saw {side_w}x{side_h} — \
             a mark that is still lying flat halfway through is a swap, not a turn"
        );

        // And it got there continuously. A swap has two drawings and one jump
        // between them; a turn has a drawing per angle and no step worth
        // seeing. Every frame of the sweep is its own pixels — not merely its
        // own cache key, which a swap could fake by keying on an angle it then
        // rounds to one of two glyphs.
        let distinct: std::collections::HashSet<&[u8]> =
            icons.iter().map(|icon| &*icon.rgba).collect();
        assert_eq!(
            distinct.len(),
            icons.len(),
            "{} of the turn's {} angles rasterized to pixels some other angle already had",
            icons.len() - distinct.len(),
            icons.len()
        );
        let spans: Vec<(u32, u32)> = icons.iter().map(ink_span).collect();
        for pair in spans.windows(2) {
            let [(_, before), (_, after)] = [pair[0], pair[1]];
            assert!(
                before.abs_diff(after) <= 3,
                "the arrow's height jumped {before}px to {after}px between adjacent frames \
                 of the turn — the transition has a step in it"
            );
        }
    }

    /// PIN — the turn needs room the layout never gave it, and taking that room
    /// does not move the arrow.
    ///
    /// A 9x6 arrow at 90° is 6x9, so the raster box is grown past the CSS box
    /// or the middle of the transition is drawn with its ends sliced off. The
    /// price of growing it would be re-aligning the *resting* arrow, which is on
    /// screen essentially always — so the growth is whole pixels, symmetric, and
    /// this proves what that buys: the glyph lands on exactly the pixels it
    /// landed on before the turn existed, byte for byte, and the wider raster is
    /// drawn over a correspondingly wider rectangle so nothing is rescaled.
    ///
    /// The comparison is against a document built from the symbol sheet's own
    /// `viewBox` and body — the form this mark had before it could turn — so
    /// the claim is checked against the old rendering rather than against a
    /// remembered number.
    #[test]
    fn the_turning_box_leaves_the_resting_arrow_exactly_where_it_was() {
        assert_eq!(
            SYMBOL_VIEW_BOX[symbol_index(ChromeMark::chevron(0.0))],
            "0 0 10 6",
            "the padding arithmetic is written against this box"
        );
        assert_eq!(CHEVRON_VIEW_BOX_UNITS, [10.0, 6.0]);

        let ink = [0x9d, 0x9d, 0x9d];
        for (glyph_width, glyph_height) in [(9u32, 6u32), (11, 8), (18, 12), (27, 18)] {
            for (turn, rotation) in [(0.0_f32, ""), (1.0, r#" transform="rotate(180 5 3)""#)] {
                let mark = ChromeMark::chevron(turn);
                let mut rasters = ChromeMarkRasters::default();
                let icons =
                    rasters.resolve(&[sprite(mark, glyph_width as f32, glyph_height as f32, ink)]);
                let icon = &icons[0];

                // The mark as it was drawn before the turn: the symbol's own
                // viewBox fitted to the layout's box, which is how every other
                // mark in this module is still drawn.
                let [r, g, b] = ink;
                let body = SYMBOL_BODY[symbol_index(mark)]
                    .replace("currentColor", &format!("#{r:02x}{g:02x}{b:02x}"));
                let legacy = format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" width="{glyph_width}" height="{glyph_height}" viewBox="0 0 10 6"><g{rotation}>{body}</g></svg>"#
                );
                let legacy = bt_math::rasterize_svg_document(legacy.as_bytes())
                    .expect("the symbol sheet's own form still rasterizes");

                let pad_x = (icon.width_px - glyph_width) / 2;
                let pad_y = (icon.height_px - glyph_height) / 2;
                assert_eq!(
                    (icon.width_px, icon.height_px),
                    (glyph_width + 2 * pad_x, glyph_height + 2 * pad_y),
                    "the raster grew by whole pixels on each side or the centre moved"
                );
                // Grown symmetrically about the same centre, and drawn over the
                // matching rectangle: the glyph's place on the surface is the
                // sprite's rect plus the same padding, so it has not moved.
                assert_eq!(
                    icon.rect,
                    [
                        -(pad_x as f32),
                        -(pad_y as f32),
                        glyph_width as f32 + pad_x as f32,
                        glyph_height as f32 + pad_y as f32,
                    ],
                    "the padded raster must be drawn over the padded rectangle"
                );

                for y in 0..icon.height_px {
                    for x in 0..icon.width_px {
                        let inside = (pad_x..pad_x + glyph_width).contains(&x)
                            && (pad_y..pad_y + glyph_height).contains(&y);
                        let index = ((y * icon.width_px + x) * 4) as usize;
                        let here = &icon.rgba[index..index + 4];
                        if inside {
                            let (lx, ly) = (x - pad_x, y - pad_y);
                            let legacy_index = ((ly * glyph_width + lx) * 4) as usize;
                            assert_eq!(
                                here,
                                &legacy.rgba[legacy_index..legacy_index + 4],
                                "at {glyph_width}x{glyph_height}, turn {turn}: pixel ({lx}, {ly}) \
                                 of the resting arrow changed when the box grew"
                            );
                        } else {
                            assert_eq!(
                                here[3], 0,
                                "at {glyph_width}x{glyph_height}, turn {turn}: the padding \
                                 carries ink, so the glyph is not where it was"
                            );
                        }
                    }
                }
            }
        }
    }

    /// PIN — the box is big enough for *every* angle, not just the two ends.
    ///
    /// Stated as ink conserved rather than as a margin measured, because that is
    /// the actual failure: a box that fits the arrow lying down slices its ends
    /// off when it stands up, and slicing shows up as ink that went missing. A
    /// rotation moves ink, it never spends any.
    #[test]
    fn no_angle_of_the_turn_is_clipped_by_its_own_box() {
        for (width, height) in [(9.0_f32, 6.0_f32), (14.0, 9.0), (18.0, 12.0), (27.0, 18.0)] {
            let icons = turn_sweep([0xed, 0xed, 0xec], width, height);
            let resting = ink_mass(&icons[0]);
            for (step, icon) in icons.iter().enumerate() {
                let mass = ink_mass(icon);
                assert!(
                    mass * 100 >= resting * 96 && mass * 96 <= resting * 100,
                    "at {width}x{height}, {}° of the turn carries {mass} of ink against the \
                     resting arrow's {resting} — the raster box is cutting the arrow off",
                    step * usize::from(CHEVRON_TURN_QUANTUM_DEGREES)
                );
            }
        }
    }

    /// PIN — the turn's angles are quantized, and the quantum is what bounds
    /// the work.
    ///
    /// An angle is half a cache key, and a continuous one has no bottom: the
    /// number of rasters a 140ms turn asks for would be however many times the
    /// compositor happened to sample it, which is a fact about the monitor
    /// rather than about the design. Quantizing fixes it at
    /// [`CHEVRON_TURN_STEPS`] for a whole sweep on any machine — and the
    /// resident cost stays at one raster, because the map is trimmed to the
    /// frame that asked.
    #[test]
    fn the_turn_s_angles_quantize_so_the_raster_cache_cannot_run_away() {
        assert_eq!(
            180_u16 % CHEVRON_TURN_QUANTUM_DEGREES,
            0,
            "both ends are on grid"
        );
        assert_eq!(CHEVRON_TURN_STEPS, 37);

        // However finely the turn is sampled, it lands on the grid and nowhere
        // else — and it never leaves the two ends behind.
        let mut seen = std::collections::HashSet::new();
        for sample in 0..=4_000 {
            let mark = ChromeMark::chevron(sample as f32 / 4_000.0);
            let ChromeMark::Chevron { turned_degrees } = mark else {
                panic!("`chevron` builds chevrons");
            };
            assert_eq!(turned_degrees % CHEVRON_TURN_QUANTUM_DEGREES, 0);
            assert!(turned_degrees <= 180);
            seen.insert(turned_degrees);
        }
        assert_eq!(
            seen.len(),
            usize::from(CHEVRON_TURN_STEPS),
            "four thousand samples of one turn may cost at most {CHEVRON_TURN_STEPS} rasters"
        );
        // Out of range is clamped rather than wrapped: an arrow told to turn
        // twice round is an arrow that is over, not one that is back.
        assert_eq!(ChromeMark::chevron(-1.0), ChromeMark::chevron(0.0));
        assert_eq!(ChromeMark::chevron(9.0), ChromeMark::chevron(1.0));

        let ink = [0x9d, 0x9d, 0x9d];
        let mut rasters = ChromeMarkRasters::default();
        let quantum = f32::from(CHEVRON_TURN_QUANTUM_DEGREES) / 180.0;
        let icons = rasters.resolve(&[
            sprite(ChromeMark::chevron(quantum), 18.0, 12.0, ink),
            // Close enough to be the same frame of the turn: one raster, shared.
            sprite(ChromeMark::chevron(quantum * 1.02), 18.0, 12.0, ink),
            sprite(ChromeMark::chevron(quantum * 2.0), 18.0, 12.0, ink),
        ]);
        assert_eq!(icons[0].key, icons[1].key);
        assert!(
            Arc::ptr_eq(&icons[0].rgba, &icons[1].rgba),
            "two samples inside one quantum must not rasterize twice"
        );
        assert_ne!(
            icons[0].key, icons[2].key,
            "the angle is part of identity — two frames of the turn are not one raster"
        );
        assert_ne!(icons[0].rgba, icons[2].rgba);
        assert_eq!(
            rasters.rasters.len(),
            2,
            "and nothing beyond what this frame drew stays resident"
        );
    }

    /// How wide a mark's ink is along the anti-diagonal scanline `x - y = k`:
    /// first inked pixel on it to last, inclusive, in pixels of `x`.
    ///
    /// A 45° mark is upright in these coordinates — `rotate(45 8 8)` maps a
    /// horizontal cut through the unrotated glyph onto exactly one of these
    /// scanlines — so this reads the glyph's own width at a chosen height, and
    /// `k` chooses that height. Larger `k` is further towards the upper right.
    fn diagonal_extent(icon: &ChromeIcon, k: i32) -> u32 {
        let inked: Vec<u32> = (0..icon.width_px)
            .filter(|&x| {
                let y = x as i32 - k;
                y >= 0 && (y as u32) < icon.height_px && alpha_at(icon, x, y as u32) > 0
            })
            .collect();
        match (inked.first(), inked.last()) {
            (Some(first), Some(last)) => last - first + 1,
            _ => 0,
        }
    }

    /// How much ink a mark carries: the sum of its coverage, not the count of
    /// pixels it touches.
    ///
    /// The distinction is the whole difference between a fill and an outline of
    /// the same silhouette, and it goes the *opposite* way to the intuition: a
    /// stroke straddles the path it follows, so the outline spills half a stroke
    /// beyond the fill's own edge and lands on **more** pixels than the fill
    /// does (224 against 195 for the pin at 26px) while carrying less coverage
    /// on them. Counting touched pixels would therefore call the outline the
    /// heavier of the two.
    fn ink_mass(icon: &ChromeIcon) -> u32 {
        icon.rgba
            .chunks_exact(4)
            .map(|pixel| u32::from(pixel[3]))
            .sum()
    }

    /// Pixels the mark covers completely — the solid core inside the outline's
    /// hole, which is what a fill adds and a stroke cannot.
    fn opaque_pixels(icon: &ChromeIcon) -> usize {
        icon.rgba.chunks_exact(4).filter(|p| p[3] == 255).count()
    }

    /// T5/T7 (v2 ③) — the two marks this slice struck draw, fill their boxes,
    /// and are two different silhouettes.
    ///
    /// **Both drawn from geometry**, which is the ruling this test is here to
    /// hold: a tag and a refresh arrow are available as font glyphs, and one
    /// reached for in a font is a fact about whichever typeface happened to be
    /// installed. If either of these ever stopped being a path in this sheet,
    /// `svg_document` would have nothing to rasterize and the ink mass would go
    /// to zero.
    ///
    /// The tag is asserted **hollow with a hole**: an outline and a punched dot
    /// is what says "label", and a solid lozenge is what a careless fill would
    /// leave — so the mark has to cover a good part of its box's edge and very
    /// little of its middle.
    #[test]
    fn the_tag_and_the_refresh_are_struck_from_geometry_and_are_not_one_drawing() {
        let ink = [0x7a, 0x99, 0xff];
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[
            sprite(ChromeMark::Tag, 32.0, 32.0, ink),
            sprite(ChromeMark::Refresh, 32.0, 32.0, ink),
        ]);
        let (tag, refresh) = (&icons[0], &icons[1]);
        assert!(ink_mass(tag) > 0, "the tag drew something");
        assert!(ink_mass(refresh) > 0, "and so did the refresh");
        assert_ne!(
            tag.rgba, refresh.rgba,
            "two marks, and not one drawing under two names"
        );
        // Each fills most of its own box: a glyph cowering in a corner is a
        // path whose viewBox does not match the units it was written in.
        for (name, icon) in [("tag", tag), ("refresh", refresh)] {
            let (width, height) = ink_span(icon);
            assert!(
                width >= 24 && height >= 24,
                "{name} covers {width}x{height} of a 32px box"
            );
        }
        // The tag is an outline: its own centre is empty, because the label's
        // body is not filled in.
        assert_eq!(
            alpha_at(tag, 16, 20),
            0,
            "a solid lozenge is what a careless fill would leave"
        );
        // And the refresh has its bite: the gap in the ring is up and to the
        // right of centre, between twelve and one o'clock, which is where the
        // arrowhead is not.
        assert_eq!(
            alpha_at(refresh, 26, 8),
            0,
            "the arc is open where the head does not reach"
        );
    }

    /// PIN (ticket #62) — **the terminal menu's three marks draw, and they are
    /// three different drawings.**
    ///
    /// Lifted from the mock-up's own sheet rather than struck here, so what this
    /// test guards is the lift: a `viewBox` or a body copied into the wrong slot
    /// of the two parallel tables would still compile, still rasterize, and put
    /// a broom on the row that deletes a transcript. Comparing every pair's
    /// pixels is what catches that — two rows wearing the same picture is
    /// exactly the failure a wrong index produces.
    ///
    /// Red gate: swap any two of the three entries in `SYMBOL_BODY` and the
    /// inequality that names them goes red; empty one and its ink mass does.
    #[test]
    fn the_terminal_menus_three_marks_are_struck_and_are_three_different_drawings() {
        let ink = [0x7a, 0x99, 0xff];
        let mut rasters = ChromeMarkRasters::default();
        let marks = [
            ("select", ChromeMark::SelectAll),
            ("broom", ChromeMark::Broom),
            ("eraser", ChromeMark::Eraser),
        ];
        let icons = rasters.resolve(
            &marks
                .iter()
                .map(|(_, mark)| sprite(*mark, 32.0, 32.0, ink))
                .collect::<Vec<_>>(),
        );
        for ((name, _), icon) in marks.iter().zip(icons.iter()) {
            assert!(
                ink_mass(icon) > 0,
                "the {name} mark rasterized to nothing at all"
            );
            assert_eq!((icon.width_px, icon.height_px), (32, 32));
        }
        for first in 0..marks.len() {
            for second in (first + 1)..marks.len() {
                assert_ne!(
                    icons[first].rgba, icons[second].rgba,
                    "{} and {} are the same drawing, so one of them is in the wrong slot",
                    marks[first].0, marks[second].0
                );
            }
        }
    }

    /// PAGE (user ruling 2026-08-20) — **the hand-off arrow and the pop-out are
    /// two drawings, and they stand next to each other.**
    ///
    /// The two are `#i-float` with and without its frame, and they share a run in
    /// a page's preview head. What this guards is the lift: a body or a `viewBox`
    /// copied into the wrong slot of the two parallel tables would still compile,
    /// still rasterize, and put a framed arrow beside a framed arrow — which is
    /// the failure the bare drawing exists to prevent, arriving as a table index
    /// instead of as a design decision.
    ///
    /// Red gate: point `ChromeMark::External` at `#i-float`'s slot and the
    /// inequality goes red; empty its body and the ink mass does.
    #[test]
    fn the_hand_off_arrow_is_not_the_pop_outs_framed_one() {
        let ink = [0x7a, 0x99, 0xff];
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[
            sprite(ChromeMark::External, 32.0, 32.0, ink),
            sprite(ChromeMark::Float, 32.0, 32.0, ink),
        ]);
        assert!(
            ink_mass(&icons[0]) > 0,
            "the hand-off arrow rasterized to nothing at all"
        );
        assert_ne!(
            icons[0].rgba, icons[1].rgba,
            "the arrow and the pop-out are the same drawing, so one of them is in the wrong slot"
        );
        // A bare arrow carries less ink than a frame with the same arrow through
        // it — which is the whole of what tells them apart at thirteen pixels.
        assert!(
            ink_mass(&icons[0]) < ink_mass(&icons[1]),
            "the bare arrow is not lighter than the framed one, so it is not bare"
        );
    }

    /// PIN — ONE pin at ONE angle: 45°, head upper-right, needle lower-left
    /// (mock-up lines 2074-2111). The state rides on the **fill**, never on the
    /// angle and never on a slash.
    ///
    /// The angle is Microsoft's house angle, unbroken from Segoe MDL2 to Segoe
    /// Fluent — no Microsoft pin glyph is ever upright, and upright is Google's
    /// `push_pin` geometry, which this design explicitly rejects. Windows 11
    /// then deleted the old horizontal-vs-diagonal action/state pairing outright
    /// (`Pin` and `Pinned` are pixel-identical in Segoe Fluent) and re-encoded
    /// the distinction on the fill axis.
    ///
    /// Red gate: read across the 45° grain rather than at a point, because that
    /// is where the two ends differ. At a 32px box, `k = 16` cuts the glyph at
    /// its head — the crossbar, 5 units of the 16-unit viewBox wide, capped out
    /// to 6.6 — and `k = -14` cuts it at its needle, 1.6 units wide. Turn the
    /// pin upright, mirror it to head-upper-*left*, or move the state onto the
    /// angle so the two disagree, and the broad cut stops being the upper-right
    /// one. The floor is stated as half again rather than the ~4x the fill
    /// actually shows because the outline's own 1.25-unit stroke pads the narrow
    /// end proportionally more than the broad one.
    #[test]
    fn the_pin_carries_its_state_on_the_fill_and_not_the_angle() {
        let ink = [0x7a, 0x99, 0xff];
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[
            sprite(ChromeMark::Pin { filled: false }, 32.0, 32.0, ink),
            sprite(ChromeMark::Pin { filled: true }, 32.0, 32.0, ink),
        ]);
        for (name, icon) in [("outline", &icons[0]), ("filled", &icons[1])] {
            let head = diagonal_extent(icon, 16);
            let needle = diagonal_extent(icon, -14);
            assert!(
                needle > 0,
                "the {name} pin has no needle towards the lower left at all"
            );
            assert!(
                head * 2 >= needle * 3,
                "the {name} pin's broad end must lie towards the upper right — \
                 saw {head}px across it against {needle}px across the lower-left end"
            );
        }
    }

    /// PIN — regular is the ACTION ("you could pin this"), filled is the STATE
    /// ("it is pinned"), per Fluent 2. Two marks, two rasters.
    ///
    /// Red gate: the two bodies differ only in paint attributes, so the one way
    /// to get this wrong is to let them share a cache identity — `mark_key`
    /// builds from `ChromeMark::id`, and a single `"i-pin"` for both would
    /// hand whichever pin was rasterized first to the other one. That failure
    /// draws a perfectly good pin in the wrong state, which is the whole
    /// message, so it is asserted four ways: the keys differ, the pixels are not
    /// the same allocation, the bytes are not equal, and the filled pin is the
    /// heavier of the two — which is what "filled against regular" means and all
    /// that it means. See [`ink_mass`] for why "heavier" is coverage summed and not
    /// pixels touched; the outline touches more of those.
    #[test]
    fn the_filled_pin_and_the_outline_pin_are_two_rasters_and_never_one() {
        let ink = [0xed, 0xed, 0xec];
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[
            sprite(ChromeMark::Pin { filled: false }, 26.0, 26.0, ink),
            sprite(ChromeMark::Pin { filled: true }, 26.0, 26.0, ink),
        ]);
        assert_eq!(
            icons.len(),
            2,
            "two pins, not one shared by a colliding key"
        );
        let (outline, filled) = (&icons[0], &icons[1]);
        assert_ne!(
            outline.key, filled.key,
            "the action pin and the state pin are different pixels and need different keys"
        );
        assert!(
            !Arc::ptr_eq(&outline.rgba, &filled.rgba),
            "the filled pin must not be handed the outline pin's raster"
        );
        assert_ne!(
            outline.rgba, filled.rgba,
            "the two pins rasterized to the same bytes — the fill axis carries nothing"
        );
        assert!(
            ink_mass(filled) > ink_mass(outline),
            "a filled pin carries more ink than an outlined one, saw {} against {}",
            ink_mass(filled),
            ink_mass(outline)
        );
        assert!(
            opaque_pixels(filled) > opaque_pixels(outline),
            "a filled pin has the larger solid core, saw {} against {}",
            opaque_pixels(filled),
            opaque_pixels(outline)
        );
        // The outline is a wall around a hole; the filled one has no hole. Read
        // at the pin's own centre, which the 45° turn leaves at the box centre.
        let (cx, cy) = (filled.width_px / 2, filled.height_px / 2);
        assert_eq!(
            alpha_at(filled, cx, cy),
            255,
            "the state pin is solid through its middle"
        );
        assert_eq!(
            alpha_at(outline, cx, cy),
            0,
            "the action pin is a hollow outline"
        );
    }

    /// **THE PREVIEW PANE'S CONTROL IS A LOCK AND NOT A THIRD PIN** (§7.7 ⑧,
    /// Claude 定 2026-08-23, on the naming ticket W1 filed and left open).
    ///
    /// The finding was that one drawing and one word were carrying two meanings
    /// two hundred pixels apart: a tab's pin and a switcher row's both say "this
    /// one stays in the list", and the preview head's said "do not reuse this
    /// pane". This test holds the answer's picture.
    ///
    /// Red gate, three ways, because there are three ways to lose it:
    /// ① **it is not the pin** — the two ids differ, so a `Lock` that fell back
    ///    to `"i-pin"` would take the pin's cache slot and its pixels with it;
    /// ② **the shackle carries the state** — read the band ABOVE the body and
    ///    left of centre: the open shackle stands on the body's right leg alone
    ///    and puts nothing there, the shut one comes down over the body and
    ///    does. Draw both shackles alike and this goes red while the fill still
    ///    passes, which is the failure that leaves a reader guessing at 13px;
    /// ③ **the fill carries it too**, Fluent 2's axis, [`ink_mass`]'s reading —
    ///    coverage summed rather than pixels touched, because a stroke lands on
    ///    more pixels than the fill it surrounds.
    /// The body is asserted identical between the two, because "same body,
    /// different shackle" is the whole of what makes the pair readable as one
    /// control in two states rather than as two marks.
    #[test]
    fn the_lock_says_its_state_on_the_shackle_as_well_as_the_fill() {
        assert_ne!(
            ChromeMark::Lock { engaged: false }.id(),
            ChromeMark::Pin { filled: false }.id(),
            "the preview pane's control may not share the pin's identity — that \
             is the collision this ruling exists to end"
        );
        let ink = [0xed, 0xed, 0xec];
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[
            sprite(ChromeMark::Lock { engaged: false }, 32.0, 32.0, ink),
            sprite(ChromeMark::Lock { engaged: true }, 32.0, 32.0, ink),
        ]);
        assert_eq!(
            icons.len(),
            2,
            "two locks, not one shared by a colliding key"
        );
        let (open, shut) = (&icons[0], &icons[1]);
        assert_ne!(open.key, shut.key, "the action and the state need two keys");
        assert_ne!(open.rgba, shut.rgba, "the two locks rasterized the same");

        // ② The shackle. The body's top edge is at 7.2 of the 16-unit box and
        // its 1.25 stroke straddles that, so the body reaches up to 6.575; rows
        // above **6** units are shackle and nothing else. Centre is 8.
        let side = open.width_px;
        let band = (side * 6) / 16;
        let left_of_centre_above_the_body = |icon: &ChromeIcon| -> u32 {
            (0..band)
                .flat_map(|y| (0..side / 2).map(move |x| (x, y)))
                .filter(|&(x, y)| alpha_at(icon, x, y) > 0)
                .count() as u32
        };
        assert_eq!(
            left_of_centre_above_the_body(open),
            0,
            "an OPEN shackle stands on the body's right-hand leg alone and \
             reaches nothing to the left of centre"
        );
        assert!(
            left_of_centre_above_the_body(shut) > 0,
            "a SHUT shackle comes down over the body and crosses its left half"
        );

        // ③ The fill, and the body that must not move under it.
        assert!(
            ink_mass(shut) > ink_mass(open),
            "the state lock carries more ink than the action lock, saw {} against {}",
            ink_mass(shut),
            ink_mass(open)
        );
        let body_row = (side * 11) / 16;
        let inked_columns = |icon: &ChromeIcon| -> (u32, u32) {
            let mut first = None;
            let mut last = 0;
            for x in 0..side {
                if alpha_at(icon, x, body_row) > 0 {
                    first.get_or_insert(x);
                    last = x;
                }
            }
            (first.expect("the body crosses this row"), last)
        };
        // ONE BODY IN BOTH, read the only way a raster can say it: a stroke
        // straddles the path it follows, so the outlined body spills half a
        // stroke wider than the filled one draws — the fill lies INSIDE the
        // outline, and the two share a centre. Asserting the two spans equal
        // would be asserting that a stroke has no width.
        let (open_first, open_last) = inked_columns(open);
        let (shut_first, shut_last) = inked_columns(shut);
        assert!(
            open_first <= shut_first && shut_last <= open_last,
            "the filled body must lie inside the outlined one, saw \
             {shut_first}..={shut_last} against {open_first}..={open_last}"
        );
        assert_eq!(
            open_first + open_last,
            shut_first + shut_last,
            "the body did not move between the two states — only its paint did"
        );
    }

    /// A raster is produced once and reused while it stays on screen, and the
    /// map is trimmed to what the current frame actually asked for.
    #[test]
    fn rasters_are_reused_across_frames_and_retired_when_unused() {
        let mut rasters = ChromeMarkRasters::default();
        let first = rasters.resolve(&[sprite(ChromeMark::Folder, 13.0, 13.0, [1, 2, 3])]);
        let again = rasters.resolve(&[sprite(ChromeMark::Folder, 13.0, 13.0, [1, 2, 3])]);
        assert_eq!(first[0].key, again[0].key);
        assert!(
            Arc::ptr_eq(&first[0].rgba, &again[0].rgba),
            "an unchanged mark must not be rasterized twice"
        );
        assert_eq!(rasters.rasters.len(), 1);

        let recoloured = rasters.resolve(&[sprite(ChromeMark::Folder, 13.0, 13.0, [9, 9, 9])]);
        assert_ne!(
            first[0].key, recoloured[0].key,
            "colour is part of identity"
        );
        assert_eq!(
            rasters.rasters.len(),
            1,
            "the retired colour must not stay resident"
        );
    }

    /// A box with no pixels in it is not drawn — and asking for one does not
    /// poison the frame's other marks.
    #[test]
    fn a_collapsed_box_produces_no_mark() {
        let mut rasters = ChromeMarkRasters::default();
        let icons = rasters.resolve(&[
            sprite(ChromeMark::Folder, 0.0, 13.0, [1, 2, 3]),
            sprite(ChromeMark::Folder, 13.0, 13.0, [1, 2, 3]),
        ]);
        assert_eq!(icons.len(), 1);
    }
}
