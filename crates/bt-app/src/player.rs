//! **The page this window writes so that a video can be played inside one**
//! (user ruling 2026-08-27, route A; `docs/DESIGN.md` §7.23 ⑩).
//!
//! # Why there is a page here at all
//!
//! §7.16 measured the alternative and it does not exist. A top-level navigation
//! to `file:///…/clip.mp4` is not a document to WebView2 — the engine has no
//! viewer for a bare media response, so it turns the navigation into a
//! **download**, and this product cancels every download unconditionally. What
//! arrives on the glass is `ConnectionAborted` under a "did not respond" card.
//! That was measured twice, on `.mp4` and on `.webm`, on this repository's
//! release build with an isolated profile.
//!
//! The same measurement found the one thing that *does* work, and found it on
//! the same seat with the same file: **an ordinary local page with a
//! `<video controls>` in it plays**, with a scrubber and a running time. So the
//! only route to playing a video here is to write that page, and this module is
//! the one place it is written.
//!
//! # Why it is a file on the disk and not a string handed to the engine
//!
//! Three reasons, and any one of them settles it.
//!
//! 1. **`NavigateToString` gives the document an opaque origin**, and an opaque
//!    origin cannot load a `file:` subresource. The video would be the one thing
//!    the shell exists to show and the one thing it could not fetch. The same
//!    sentence rules out a `data:` URL shell.
//! 2. **The gate has no room for it and should not be given any.**
//!    `webnav::check` refuses `data:` unconditionally
//!    ([`webnav::Refusal::ScriptOrInlineScheme`]), and a mint is a note about a
//!    *URL*; a string navigation has no URL to mint, so admitting one would mean
//!    a hole in the one door every navigation in this product goes through.
//! 3. **A file is the form that was measured.** The page §7.16 played the video
//!    in was a file on that disk reached by a `file:` URL, and this module
//!    writes the same shape of page and reaches it the same way. Choosing the
//!    unmeasured form over the measured one, to save writing seven hundred
//!    bytes, would be trading the one piece of evidence this slice has.
//!
//! # Where it lives, and who clears it
//!
//! `%LOCALAPPDATA%\Folio\player`, beside — and deliberately not inside — the
//! `WebView2` user-data folder the engine owns. `%LOCALAPPDATA%` for
//! [`crate::webhost::user_data_folder_in`]'s reason: it is a cache, and a
//! roaming profile would carry it between machines. Reading it from the
//! environment rather than assembling it is what lets a test or a taking of
//! evidence isolate the whole of this window's disk with one variable.
//!
//! **Nobody clears it, on purpose, and the leak is bounded by construction.**
//! The shell's name is a hash of the video's own URL, so it is one file per
//! distinct recording ever played — about seven hundred bytes each — and playing
//! the same file twice rewrites the same name. A per-seat name would have needed
//! a rule for the shell of a process that crashed, and a sweep would have needed
//! a rule for the shell of the *other* Folio running against the same
//! `%LOCALAPPDATA%`. Content-addressing has neither problem: two windows playing
//! one recording write identical bytes to one path, and a crash leaves behind a
//! file the next play will simply overwrite. It goes when the profile goes.
//!
//! # What is in it, and what is deliberately not
//!
//! A `<video>` that fills the pane and keeps its proportion, and a control bar
//! this product drew (user ruling 2026-08-27, §7.23 ⑪). **Neither of those was
//! true of the first build**, and both of the defects were the same mistake in
//! two places — leaving to the engine a thing this window has an opinion about:
//!
//! * The video was written `max-width:100%; max-height:100%; width:auto;
//!   height:auto`, which only ever shrinks. A 160×120 recording in a
//!   1200×800 pane was a postage stamp in the middle of it. It is now
//!   `width:100%; height:100%; object-fit:contain` — the picture fills the pane
//!   at its own proportion, and the letterbox is the pane's own ground.
//! * `controls` handed the face to Edge: a white Material-icon bar with a `⋮`
//!   menu offering Fullscreen, Playback speed and Picture in picture, none of
//!   which is this window's vocabulary and one of which (fullscreen) is a verb
//!   this product already has somewhere else — double-clicking a pane's head.
//!   The attribute is gone and the bar is written here.
//!
//! **The bar is the window's own, drawn out of the window's own tables**: the
//! colours are [`PlayerSkin`], read from `bt_render::chrome_palette` at the
//! moment the shell is written; the marks are the symbol sheet's, handed over as
//! SVG by [`crate::marks::ChromeMark::artwork`] rather than redrawn in CSS; the
//! fades are `MOTION_FAST_MS`; and the two waits are registered in the motion
//! archive like every other wait in the product. **What it does not have** is a
//! fullscreen button and a picture-in-picture button, both by the same ruling:
//! the first is a verb this window already owns, and the second takes a pane's
//! content out of the window that is showing it.
//!
//! **Still no message bridge.** `IsWebMessageEnabled` and `AreHostObjectsAllowed`
//! stay shut, which is the discipline §7.7 wrote. The page now has a script — a
//! bar with a scrubber in it is behaviour, and behaviour in a document is a
//! script — but it is the page's own inline one, with `default-src 'none'`
//! standing over it so there is no source a second one could come from, and it
//! talks to nothing outside the document. The host still does not read the
//! position, the duration or the volume: the reader does, off the bar. The day
//! one of those numbers is wanted *here* (a resume, a thumbnail strip) it will
//! be worth a door; today it would be a door with nothing behind it.

use std::path::{Path, PathBuf};

use bt_render::MOTION_FAST_MS;

use crate::marks::ChromeMark;
use crate::webnav::{Mint, Refusal};

/// The folder the shells are written into, under a given `%LOCALAPPDATA%`.
///
/// Split from [`shell_folder`] exactly as [`crate::webhost::user_data_folder_in`]
/// is split from its own environment reader, and for the same reason: the shape
/// of the path is a fact a test can assert, and where `%LOCALAPPDATA%` is is not.
#[must_use]
pub fn shell_folder_in(local_appdata: &Path) -> PathBuf {
    local_appdata.join("Folio").join("player")
}

/// The same, on this machine.
#[must_use]
pub fn shell_folder() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(|base| shell_folder_in(Path::new(&base)))
}

/// **The shell's file name for one video URL** — a hash and nothing else.
///
/// Not the recording's own name: two files called `capture.mp4` in two folders
/// are two recordings, and a name that collided would put one reader in front of
/// another's video. Not the recording's path spelled out either — a path is
/// longer than a file name may be, and it would put the whole of somebody's
/// directory tree into a cache folder's listing.
///
/// The hash is over the **minted URL** rather than over the `Path`, so that the
/// key and the thing the page actually points at are the same string: a shell is
/// correct for a video exactly when it was written from that video's URL.
#[must_use]
fn shell_name_of(video_url: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    video_url.hash(&mut hasher);
    format!("play-{:016x}.html", hasher.finish())
}

/// One string, safe to sit inside a double-quoted HTML attribute.
///
/// **All five and not the two that a Windows path can hold**, because the thing
/// being escaped is a URL and a URL is not only a path: `&` and `'` are ordinary
/// in a Windows file name, `<`, `>` and `"` are not — and an escaper that knew
/// which of its inputs were which would be an escaper somebody would later reach
/// for with a different input.
#[must_use]
fn attribute_escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// **How long a pointer has to be in the page before the bar comes up.**
///
/// A wait, not a transition, and it is the house's own hover intent to the
/// millisecond — `profiles::CHEVRON_HOVER_OPEN_DELAY`, the rest a `⌄` asks for
/// before it opens its menu. A player's bar and a chevron's menu are the same
/// promise made twice: *this is what your hand was reaching for*, and two
/// different answers to it would be the window disagreeing with itself about
/// how long a hand takes to mean something.
///
/// It is armed once when the pointer starts moving and **not** restarted by the
/// moves after it. Restarting on every move would mean a bar that never came up
/// while the pointer was travelling, which is the whole of the time a reader is
/// reaching for it.
pub const PLAYER_BAR_REVEAL_INTENT_MS: u64 = 250;

/// **How long the bar stands after the last thing the reader did.**
///
/// The dwell *after*, on `termscroll::THUMB_REST`'s precedent: a control with no
/// reason left to be up goes away, and two seconds is the reading time for the
/// one thing on the bar a reader might have come to read — the clock.
///
/// Three states are not "no action" and hold it up: a **paused** video (a
/// player with nothing happening has no second way to say what it is doing), a
/// pointer resting **on the bar itself**, and a **scrubber being dragged**.
pub const PLAYER_BAR_IDLE_REST_MS: u64 = 2_000;

/// **The colours, the face and the size the shell page is drawn in** — the
/// window's own tables, carried across the one boundary that cannot read them.
///
/// A page is not a pane: it cannot ask `bt_render` for a palette, so the palette
/// has to be written into it. This struct is that crossing made explicit, and it
/// is the whole of it — every colour on the bar, and the two type facts, come
/// through here or they do not exist.
///
/// **Baked at the moment the shell is written**, exactly as the ground alone was
/// before it (§7.23 ⑩): a theme changed while a video is playing does not
/// repaint the bar until the next play. The alternative was re-writing the file
/// and navigating to it again, which restarts the recording from zero — a theme
/// switch that threw away a reader's place in a video would be a far louder bug
/// than a bar that is one theme behind for the rest of one playback. See
/// [`PlayerSkin::in_force`] for where the reading happens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerSkin {
    /// `--termbg`: the pane's own ground, which is what the picture is
    /// letterboxed against.
    pub ground: [u8; 3],
    /// `--panel`: the bar's own surface, the same one a markdown code fence
    /// lays over a preview body.
    pub bar: [u8; 3],
    /// `--border-soft` over the bar's ground — the one hairline on the page.
    pub hairline: [u8; 3],
    /// `--ink`: what a control under the pointer is drawn in.
    pub ink: [u8; 3],
    /// `--ink2` on the bar's ground: what a control at rest, and every number
    /// on the bar, is drawn in.
    pub ink_muted: [u8; 3],
    /// `--accent`: the played part of the scrubber and the filled part of the
    /// volume, and nothing else on the page.
    pub accent: [u8; 3],
    /// The chrome's own CSS stack.
    pub font_family: &'static str,
    /// The size a pane's own title is set at, in logical pixels — which is what
    /// a CSS pixel is here, because the host sets the engine's
    /// `RasterizationScale` itself.
    pub font_px: f32,
}

/// The chrome's face, as CSS spells it.
///
/// The mock-up's own stack (`font-family: "Inter", "Segoe UI Variable Text",
/// "Segoe UI"`), with the generic tail a CSS stack has to end in. The window's
/// loader walks the same list and for the same reason — see
/// `bt_render`'s `CHROME_SANS_FONT_FILES` — so a caption in the window and a
/// clock on this bar are the same face on the same machine, including on the
/// machine where the first two entries are absent.
const CHROME_FONT_STACK: &str = "\"Inter\",\"Segoe UI Variable Text\",\"Segoe UI\",sans-serif";

impl PlayerSkin {
    /// **The skin this window is wearing right now.**
    ///
    /// Split from [`shell_html`] the way every reader of an environment in this
    /// crate is split from the thing that uses it: what the tokens *are* is a
    /// fact a test can assert, and which of the two palettes is in force is not.
    #[must_use]
    pub fn in_force() -> Self {
        Self::of(&bt_render::chrome_palette())
    }

    /// The same, from a palette handed in — the pure half.
    #[must_use]
    pub fn of(palette: &bt_render::ChromePalette) -> Self {
        Self {
            ground: palette.seat_body,
            // The fence's ground and the fence's edge, and they are borrowed
            // rather than given names of their own for the reason the palette
            // itself gives about `--panel`: this bar *is* a panel laid over a
            // preview body, which is the one surface pairing this window
            // already has a resolved hairline for. A second pair of tokens
            // holding the same two numbers would be a second authority for
            // them.
            bar: palette.preview_code_ground,
            hairline: palette.preview_code_border,
            ink: palette.preview_body_text,
            ink_muted: palette.preview_code_text,
            accent: palette.accent,
            font_family: CHROME_FONT_STACK,
            font_px: bt_render::SEAT_TITLE_FONT_LOGICAL_PX,
        }
    }

    /// The `:root` block — every token this page has, and the page has no
    /// colour that is not one of them.
    ///
    /// Written as custom properties rather than substituted into the rules
    /// because that is what makes the claim checkable: a rule that reads
    /// `var(--accent)` cannot be wearing a colour somebody typed, and a red gate
    /// can read this one block and know it has seen the whole palette.
    fn css_root(self) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(512);
        out.push_str(":root{\n");
        for (name, value) in [
            ("--ground", self.ground),
            ("--bar", self.bar),
            ("--hairline", self.hairline),
            ("--ink", self.ink),
            ("--ink-muted", self.ink_muted),
            ("--accent", self.accent),
        ] {
            let [red, green, blue] = value;
            let _ = writeln!(out, "  {name}:#{red:02x}{green:02x}{blue:02x};");
        }
        let _ = writeln!(out, "  --ui-font:{};", self.font_family);
        let _ = writeln!(out, "  --ui-size:{}px;", self.font_px);
        let _ = writeln!(out, "  --fast:{MOTION_FAST_MS}ms;");
        let [ex, ey, ez, ew] = bt_render::EASE;
        let _ = writeln!(out, "  --ease:cubic-bezier({ex},{ey},{ez},{ew});");
        out.push_str("}\n");
        out
    }
}

/// One mark off the symbol sheet, as an `<svg>` the page can wear.
///
/// The drawing is [`crate::marks::ChromeMark::artwork`]'s, verbatim, with
/// `currentColor` left standing so the button's own `color` reaches it. `class`
/// is what the two faces of a control are swapped by, and `aria-hidden` because
/// the button around it is the thing with the meaning.
fn mark_svg(mark: ChromeMark, class: &str) -> String {
    let (view_box, body) = mark
        .artwork()
        .expect("every mark the player wears is one of the sheet's quoted symbols");
    format!(r#"<svg class="{class}" viewBox="{view_box}" aria-hidden="true">{body}</svg>"#)
}

/// The page, as text.
///
/// **The picture fills the pane and keeps its proportion** (user ruling
/// 2026-08-27, §7.23 ⑪): `width:100%; height:100%; object-fit:contain`. The
/// rule it replaces — `max-width:100%; max-height:100%; width:auto; height:auto`
/// — only ever *shrinks*, so a recording smaller than the pane was drawn at its
/// own pixel size and a 160×120 clip in a full-height pane was a stamp in the
/// middle of a field of ground. `contain` is the half of the ruling that is not
/// "fill": the picture is never stretched, and what fills the rest is
/// [`PlayerSkin::ground`], the pane's own colour.
///
/// **No `controls`.** What that attribute drew was Edge's own bar — white,
/// Material-iconed, with a `⋮` offering Fullscreen, Mute, Playback speed and
/// Picture in picture. Two of those four are verbs this product has ruled on
/// elsewhere (a pane zooms by a double-click on its head; a pane's content does
/// not leave the window it is in), and all four were drawn in a vocabulary
/// nothing else on the glass shares. The bar below replaces it.
///
/// **`autoplay` because the reader already pressed play.** The gesture happened
/// on this window's own button; the page cannot see it, and the shell is being
/// navigated to *because* of it. A shell that then sat waiting to be pressed a
/// second time would be asking the reader to say the same thing twice.
///
/// The `Content-Security-Policy` says out loud what the shell is: it may load a
/// `file:` for its media, it may lay out its own inline style and it may run its
/// own inline script, and there is no source at all for a frame, an image or a
/// connection. **The script is new and the sentence about it has changed, so it
/// is written down rather than quietly widened**: a bar with a scrubber, a
/// clock and a keyboard is behaviour, and there is no way to put behaviour in a
/// document except a script. What has *not* changed is the thing that discipline
/// was ever about — `default-src 'none'` still stands, so nothing can be
/// fetched, nothing can be framed and no second script can arrive; the message
/// bridge and host objects are still shut; and the page still talks to nothing
/// outside itself. A page that cannot fetch is a page that cannot leak the path
/// it was given, script or no script.
///
/// **It was measured, and it admits the recording** (2026-08-27, on this
/// repository's release build with an isolated profile): the `<video>` sized
/// itself to the fixture's own 160×120, which is a `loadedmetadata` and
/// therefore a media load that `media-src file:` let through. It was briefly
/// suspected of blocking one — the page came up black, at what looked like a
/// default size — and the real answer was Chromium's autoplay policy plus a
/// fixture whose first frames are deliberately black. The suspicion is recorded
/// because the next reader will have it too.
#[must_use]
pub fn shell_html(video_url: &str, skin: PlayerSkin) -> String {
    let mut page = String::with_capacity(12 * 1024);
    page.push_str(concat!(
        "<!doctype html>\n",
        "<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n",
        "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; ",
        "media-src file:; style-src 'unsafe-inline'; script-src 'unsafe-inline'\">\n",
        "<title>Folio player</title>\n",
        "<style>\n",
    ));
    page.push_str(&skin.css_root());
    page.push_str(SHELL_CSS);
    page.push_str("</style>\n</head>\n<body>\n");
    page.push_str(&format!(
        "<video id=\"v\" autoplay preload=\"metadata\" src=\"{source}\"></video>\n",
        source = attribute_escaped(video_url),
    ));
    page.push_str(&format!(
        concat!(
            "<div id=\"bar\">\n",
            "<button id=\"pp\" class=\"ib\" type=\"button\">{play}{pause}</button>\n",
            "<span id=\"at\" class=\"tt\">0:00</span>\n",
            "<div id=\"seek\" class=\"track\"><i id=\"seekfill\" class=\"fill\"></i>",
            "<i id=\"seekknob\" class=\"knob\"></i></div>\n",
            "<span id=\"dur\" class=\"tt\">0:00</span>\n",
            "<button id=\"mu\" class=\"ib\" type=\"button\">{speaker}{muted}</button>\n",
            "<div id=\"vol\" class=\"track\"><i id=\"volfill\" class=\"fill\"></i>",
            "<i id=\"volknob\" class=\"knob\"></i></div>\n",
            "<button id=\"rate\" class=\"tb\" type=\"button\">1\u{d7}</button>\n",
            "</div>\n",
        ),
        play = mark_svg(ChromeMark::Play, "f-play"),
        pause = mark_svg(ChromeMark::Pause, "f-pause"),
        speaker = mark_svg(ChromeMark::Speaker, "f-loud"),
        muted = mark_svg(ChromeMark::SpeakerMuted, "f-muted"),
    ));
    page.push_str("<script>\n");
    page.push_str(&format!(
        "var REVEAL_MS={PLAYER_BAR_REVEAL_INTENT_MS},REST_MS={PLAYER_BAR_IDLE_REST_MS};\n"
    ));
    page.push_str(SHELL_JS);
    page.push_str("</script>\n</body>\n</html>\n");
    page
}

/// The bar's rules, in the tokens [`PlayerSkin::css_root`] wrote.
///
/// **Every colour in here is a `var()`.** That is not tidiness: it is what makes
/// "the bar wears the window's palette" a thing a test can read off one block
/// instead of a claim about a thousand characters of CSS.
///
/// The shapes are the house's, which here mostly means the shapes it refuses:
/// **no radius on anything that is not a dot**, no filled pill behind a button,
/// **one hairline** — the bar's top edge — and a control that is not under the
/// pointer says so by its ink rather than by growing a background. The two
/// timings are [`bt_render::MOTION_FAST_MS`] and [`bt_render::EASE`], the same
/// pair every hover fade in the window is drawn on.
const SHELL_CSS: &str = concat!(
    "html,body{margin:0;padding:0;width:100%;height:100%;background:var(--ground);overflow:hidden}\n",
    "body{font-family:var(--ui-font);font-size:var(--ui-size);color:var(--ink);\n",
    "  -webkit-user-select:none;user-select:none;cursor:default}\n",
    // The picture: the pane, all of it, at its own proportion.
    "video{display:block;width:100%;height:100%;object-fit:contain;\n",
    "  background:var(--ground);outline:none}\n",
    // The bar: a panel laid on the bottom edge, one hairline along its top, and
    // it fades rather than appearing.
    "#bar{position:fixed;left:0;right:0;bottom:0;box-sizing:border-box;height:34px;\n",
    "  display:flex;align-items:center;gap:8px;padding:0 10px;\n",
    "  background:var(--bar);border-top:1px solid var(--hairline);\n",
    "  opacity:0;pointer-events:none;transition:opacity var(--fast) var(--ease)}\n",
    "#bar.on{opacity:1;pointer-events:auto}\n",
    // The controls: ink only. A button at rest is `--ink2`; under the pointer,
    // or holding the keyboard, it is `--ink`.
    "button{appearance:none;-webkit-appearance:none;background:none;border:0;margin:0;padding:0;\n",
    "  font:inherit;color:var(--ink-muted);cursor:default;display:flex;align-items:center;\n",
    "  justify-content:center;transition:color var(--fast) var(--ease)}\n",
    "button:hover,button:focus-visible{color:var(--ink)}\n",
    "button:focus{outline:none}\n",
    ".ib{flex:0 0 auto;width:22px;height:22px}\n",
    // No `display` here on purpose: the two-faced controls set theirs, and a
    // `.ib svg{display:block}` would out-specify a `.f-pause{display:none}`
    // (one class and one type beats one class) and put both faces on the bar
    // at once. The button is a flex box, so its children are blockified anyway.
    ".ib svg{width:16px;height:16px}\n",
    ".tb{flex:0 0 auto;min-width:32px;height:22px;justify-content:flex-end;\n",
    "  font-variant-numeric:tabular-nums}\n",
    ".tt{flex:0 0 auto;color:var(--ink-muted);font-variant-numeric:tabular-nums}\n",
    // The two tracks. A rail of the page's own hairline, a fill of the accent,
    // and a dot that is only there while the hand is.
    ".track{position:relative;height:20px;flex:1 1 auto;min-width:24px}\n",
    ".track::before{content:\"\";position:absolute;left:0;right:0;top:9px;height:2px;\n",
    "  background:var(--hairline)}\n",
    ".track .fill{position:absolute;left:0;top:9px;height:2px;width:0;background:var(--accent)}\n",
    ".track .knob{position:absolute;top:10px;left:0;width:7px;height:7px;\n",
    "  margin:-3.5px 0 0 -3.5px;border-radius:50%;background:var(--accent);\n",
    "  opacity:0;transition:opacity var(--fast) var(--ease)}\n",
    ".track:hover .knob,.track.grab .knob{opacity:1}\n",
    "#vol{flex:0 0 54px}\n",
    // The two faces of each two-faced control. One is in the document at a
    // time, and which one is a class on the body — so the state is read off the
    // element that has it (`<video>`) and written in exactly one place.
    ".f-pause,.f-muted{display:none}\n",
    "body.playing .f-play{display:none}\n",
    "body.playing .f-pause{display:block}\n",
    "body.muted .f-loud{display:none}\n",
    "body.muted .f-muted{display:block}\n",
);

/// The bar's behaviour.
///
/// Small enough to read in one sitting, which is the point: it is the only
/// script this product ships to an engine, and the day somebody has to audit
/// what a Folio page can do, this is the whole answer. It reads and writes one
/// `<video>` element and nothing else — no fetch, no storage, no timers beyond
/// the two waits above, and no way to reach the host.
///
/// **The state is the element's, never a copy of it.** Every face on the bar is
/// painted from `v.paused`, `v.muted`, `v.currentTime` and friends, in one
/// function, called on the media events. A player that remembered what it had
/// been told is a player that lies the first time something else changes it —
/// and something else does: a video that reaches its end pauses without anybody
/// pressing anything.
const SHELL_JS: &str = concat!(
    "(function(){\n",
    "var v=document.getElementById(\"v\"),bar=document.getElementById(\"bar\");\n",
    "var at=document.getElementById(\"at\"),dur=document.getElementById(\"dur\");\n",
    "var seek=document.getElementById(\"seek\"),seekfill=document.getElementById(\"seekfill\");\n",
    "var seekknob=document.getElementById(\"seekknob\");\n",
    "var vol=document.getElementById(\"vol\"),volfill=document.getElementById(\"volfill\");\n",
    "var volknob=document.getElementById(\"volknob\");\n",
    "var pp=document.getElementById(\"pp\"),mu=document.getElementById(\"mu\");\n",
    "var rate=document.getElementById(\"rate\");\n",
    "var RATES=[1,1.25,1.5,2];\n",
    "var revealTimer=null,restTimer=null,grabbing=null,overBar=false;\n",
    // `m:ss`, and `h:mm:ss` once there is an hour to say — the same shape the
    // pane's own facts line already prints a duration in.
    "function clock(s){if(!isFinite(s)||s<0){s=0;}s=Math.floor(s);\n",
    "  var h=Math.floor(s/3600),m=Math.floor(s/60)%60,x=s%60;\n",
    "  var ss=(x<10?\"0\":\"\")+x;\n",
    "  return h?h+\":\"+((m<10?\"0\":\"\")+m)+\":\"+ss:m+\":\"+ss;}\n",
    "function paint(){\n",
    "  var d=v.duration,f=(isFinite(d)&&d>0)?v.currentTime/d:0;\n",
    "  if(f<0){f=0;}if(f>1){f=1;}\n",
    "  seekfill.style.width=(f*100)+\"%\";seekknob.style.left=(f*100)+\"%\";\n",
    "  at.textContent=clock(v.currentTime);dur.textContent=clock(isFinite(d)?d:0);\n",
    "  var g=v.muted?0:v.volume;\n",
    "  volfill.style.width=(g*100)+\"%\";volknob.style.left=(g*100)+\"%\";\n",
    "  document.body.classList.toggle(\"playing\",!v.paused&&!v.ended);\n",
    "  document.body.classList.toggle(\"muted\",v.muted||v.volume===0);\n",
    "  rate.textContent=(Math.round(v.playbackRate*100)/100)+\"\\u00d7\";}\n",
    "function arm(){window.clearTimeout(restTimer);restTimer=window.setTimeout(rest,REST_MS);}\n",
    "function show(){bar.classList.add(\"on\");\n",
    "  window.clearTimeout(revealTimer);revealTimer=null;arm();}\n",
    // `:focus-visible` and not `document.activeElement`, and it was the machine
    // that said so: a click on the play button *focuses* it, so a bar held up by
    // "something in here has the focus" went up on the first press and never
    // came down again. The browser already draws the line this wants — a
    // control the keyboard is on versus one a mouse merely pressed — and it is
    // the same line the stylesheet above lights a control by.
    "function rest(){if(v.paused||overBar||grabbing||bar.querySelector(\":focus-visible\")){arm();return;}\n",
    "  bar.classList.remove(\"on\");}\n",
    "function intent(){if(bar.classList.contains(\"on\")){arm();return;}\n",
    "  if(revealTimer!==null){return;}\n",
    "  revealTimer=window.setTimeout(function(){revealTimer=null;show();},REVEAL_MS);}\n",
    "function toggle(){if(v.paused||v.ended){var p=v.play();if(p&&p.catch){p.catch(function(){});}}\n",
    "  else{v.pause();}}\n",
    "function step(n){var d=v.duration,t=v.currentTime+n;\n",
    "  if(t<0){t=0;}if(isFinite(d)&&t>d){t=d;}v.currentTime=t;}\n",
    "function gain(n){var g=v.volume+n;if(g<0){g=0;}if(g>1){g=1;}\n",
    "  v.volume=g;if(g>0){v.muted=false;}}\n",
    "function frac(track,e){var r=track.getBoundingClientRect();\n",
    "  if(r.width<=0){return 0;}var f=(e.clientX-r.left)/r.width;\n",
    "  return f<0?0:(f>1?1:f);}\n",
    "function draggable(track,apply){\n",
    "  track.addEventListener(\"pointerdown\",function(e){if(e.button!==0){return;}\n",
    "    grabbing=track;track.classList.add(\"grab\");track.setPointerCapture(e.pointerId);\n",
    "    apply(frac(track,e));show();e.preventDefault();});\n",
    "  track.addEventListener(\"pointermove\",function(e){\n",
    "    if(grabbing===track){apply(frac(track,e));show();}});\n",
    "  track.addEventListener(\"pointerup\",function(e){if(grabbing!==track){return;}\n",
    "    grabbing=null;track.classList.remove(\"grab\");\n",
    "    track.releasePointerCapture(e.pointerId);arm();});\n",
    "  track.addEventListener(\"pointercancel\",function(){if(grabbing!==track){return;}\n",
    "    grabbing=null;track.classList.remove(\"grab\");arm();});}\n",
    "draggable(seek,function(f){var d=v.duration;\n",
    "  if(isFinite(d)&&d>0){v.currentTime=f*d;}paint();});\n",
    "draggable(vol,function(f){v.volume=f;v.muted=(f===0);paint();});\n",
    "pp.addEventListener(\"click\",function(){toggle();show();});\n",
    "mu.addEventListener(\"click\",function(){v.muted=!v.muted;\n",
    "  if(!v.muted&&v.volume===0){v.volume=0.5;}paint();show();});\n",
    "rate.addEventListener(\"click\",function(){\n",
    "  var i=RATES.indexOf(v.playbackRate);\n",
    "  v.playbackRate=RATES[(i+1)%RATES.length];paint();show();});\n",
    // The keys, and they reach the page only when the page has the focus — which
    // is what "only in the shell" means for a document inside a pane.
    "document.addEventListener(\"keydown\",function(e){\n",
    "  if(e.ctrlKey||e.altKey||e.metaKey){return;}\n",
    "  var k=e.key;\n",
    "  if(k===\" \"||k===\"Spacebar\"){toggle();}\n",
    "  else if(k===\"ArrowLeft\"){step(-5);}\n",
    "  else if(k===\"ArrowRight\"){step(5);}\n",
    "  else if(k===\"ArrowUp\"){gain(0.05);}\n",
    "  else if(k===\"ArrowDown\"){gain(-0.05);}\n",
    "  else if(k===\"m\"||k===\"M\"){v.muted=!v.muted;}\n",
    "  else{return;}\n",
    "  e.preventDefault();paint();show();});\n",
    "document.addEventListener(\"pointermove\",intent);\n",
    "document.addEventListener(\"pointerdown\",show);\n",
    "document.documentElement.addEventListener(\"pointerleave\",function(){\n",
    "  window.clearTimeout(revealTimer);revealTimer=null;rest();});\n",
    "bar.addEventListener(\"pointerenter\",function(){overBar=true;show();});\n",
    "bar.addEventListener(\"pointerleave\",function(){overBar=false;arm();});\n",
    "var events=[\"loadedmetadata\",\"durationchange\",\"timeupdate\",\"play\",\"pause\",\n",
    "  \"ended\",\"volumechange\",\"ratechange\",\"seeked\"];\n",
    "for(var i=0;i<events.length;i++){v.addEventListener(events[i],paint);}\n",
    "v.addEventListener(\"pause\",show);\n",
    "paint();show();\n",
    "})();\n",
);

/// **Write the shell for one video and mint the URL of it** — the whole of what
/// the play verb has to do before it can ask for a navigation.
///
/// **Two spellings of one file, and they are not interchangeable** (found on the
/// machine, 2026-08-27).
///
/// * `canonical` is what the disk says — `std::fs::canonicalize`'s answer, which
///   on Windows is a **verbatim** path (`\\?\D:\…`). It is what the URL inside
///   the shell is minted from, exactly as `Runtime::open_preview_web_file` mints
///   from a canonicalised path: the disk is what says what a path is, and the URL
///   has to be the one a reader's `↗` would resolve back to.
/// * `named` is what this window calls the file — the spelling on the pane's own
///   `PreviewImageState`, which is the string a reader typed, double-clicked or
///   dropped. It is what the mint *records*, because every surface that reads it
///   back compares it against that same pane.
///
/// **The first build passed the canonical path for both and the player closed
/// itself inside a frame.** `a_page_was_replaced` compares the recording the
/// mint names against the path the pane is showing; `\\?\D:\…\clip.mp4` and
/// `D:\…\clip.mp4` are the same file and different strings, so the pane looked
/// like a pane something else had landed on, the orphan rule retired the browser
/// on the next turn, and what a reader saw was a page's chrome flashing up and
/// the first frame coming back. Two parameters rather than one, because the two
/// jobs are genuinely two: one names a file to the operating system and the
/// other names it to this window.
///
/// The video's own URL is made by [`Mint::file`] and not by a second encoder
/// here. That is the point of asking it: `notes#1.mp4` is a file and not a
/// fragment, and there is one place in this product that knows so.
///
/// Errors are a [`Refusal`] rather than an `io::Error` because the caller has
/// exactly one thing to do with either — decline to play and say why — and
/// because a network path has to be refused here for `Mint::file`'s own reason,
/// which is not an I/O failure at all.
pub fn mint_player_shell(
    named: &Path,
    canonical: &Path,
    skin: PlayerSkin,
) -> Result<Mint, Refusal> {
    let video_url = match Mint::file(canonical)? {
        Mint::File(url) => url,
        // Unreachable by construction: `Mint::file` returns `File` or an error,
        // and it has done so since the door was written. Spelled as a refusal
        // rather than as a panic because a mint that changed shape is a thing to
        // decline, not a thing to crash a terminal over.
        _ => return Err(Refusal::NotMinted),
    };
    let folder = shell_folder().ok_or(Refusal::NotMinted)?;
    std::fs::create_dir_all(&folder).map_err(|_| Refusal::NotMinted)?;
    let shell = folder.join(shell_name_of(&video_url));
    std::fs::write(&shell, shell_html(&video_url, skin)).map_err(|_| Refusal::NotMinted)?;
    let Mint::File(url) = Mint::file(&shell)? else {
        return Err(Refusal::NotMinted);
    };
    Ok(Mint::VideoShell {
        url,
        video: named.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A skin with six colours nobody could type by accident, so that "this
    /// colour reached the page" is a claim about *this* palette and not about
    /// black happening to appear in a stylesheet.
    fn a_skin() -> PlayerSkin {
        PlayerSkin {
            ground: [0x1b, 0x1b, 0x1b],
            bar: [0x25, 0x26, 0x27],
            hairline: [0x33, 0x34, 0x35],
            ink: [0xd8, 0xdd, 0xe6],
            ink_muted: [0x9a, 0x9b, 0x9c],
            accent: [0x7a, 0x99, 0xff],
            font_family: CHROME_FONT_STACK,
            font_px: 11.5,
        }
    }

    /// RED — **the shell points at the video and at nothing else, and it is the
    /// mint's own spelling of it** (user ruling 2026-08-27; §7.23 ⑩).
    ///
    /// Three claims about one string: the element that plays is a `<video>`; it
    /// is told to start, because the reader already pressed play on this
    /// window's button; and its source is the URL [`Mint::file`] writes,
    /// character for character, so that a `#` in a file name is a `#` in a file
    /// name.
    ///
    /// **The bridge is still shut and the page still cannot fetch**, which is
    /// what the "no script" half of this gate was ever about — see
    /// [`the_shell_wears_no_engine_controls_and_no_engine_face`], where the
    /// script that did arrive is pinned to being the page's own.
    ///
    /// RED GATE: put the path in raw instead of through `Mint::file` and the
    /// third assertion fails on the `#`, which is the exact defect that would
    /// otherwise be found by a reader whose recording is called `take#2.mp4`.
    #[test]
    fn the_shell_plays_one_video_with_no_bridge_to_this_window() {
        let video = Path::new(r"D:\shots\take#2.mp4");
        let Mint::File(url) = Mint::file(video).expect("a drive path mints") else {
            unreachable!("Mint::file answers File")
        };
        let html = shell_html(&url, a_skin());
        assert!(html.contains("<video id=\"v\" autoplay"), "{html}");
        assert!(
            html.contains(r#"src="file:///D:/shots/take%232.mp4""#),
            "{html}"
        );
        assert!(!html.contains("chrome.webview"), "{html}");
        // And the policy that says the page may load its media and nothing
        // else — measured to admit the recording; see [`shell_html`].
        assert!(html.contains("media-src file:"), "{html}");
        assert!(html.contains("default-src 'none'"), "{html}");
        // The page runs its own script and may not be sent a second one: there
        // is no `script-src` host, only the document's own inline block.
        assert!(html.contains("script-src 'unsafe-inline'"), "{html}");
        for absent in ["connect-src", "img-src", "frame-src", "http://", "https://"] {
            assert!(!html.contains(absent), "{absent} is in the shell:\n{html}");
        }
    }

    /// RED — **the engine draws neither the picture's size nor the player's
    /// face** (user ruling 2026-08-27, the two defects reported off `next12`;
    /// `docs/DESIGN.md` §7.23 ⑪).
    ///
    /// The two defects were one mistake in two places — leaving to Edge a thing
    /// this window has an opinion about — so they are one gate:
    ///
    /// 1. **The picture fills the pane.** The shipped rule was
    ///    `max-width:100%;max-height:100%;width:auto;height:auto`, which only
    ///    shrinks: a 160×120 clip in a full-height pane was drawn at 160×120.
    ///    Both halves are asserted, because either alone is a build that is
    ///    still wrong — `width:100%;height:100%` without `object-fit:contain`
    ///    stretches the recording out of shape.
    /// 2. **The face is this product's.** No `controls` attribute anywhere in
    ///    the document, and a bar of our own with the four controls the ruling
    ///    named on it.
    ///
    /// RED GATE: restore either half of the old sizing rule and the first block
    /// fails; put `controls` back on the `<video>` and the second does; delete
    /// the bar and the third does. All three were the state of `next12`.
    #[test]
    fn the_shell_wears_no_engine_controls_and_no_engine_face() {
        let html = shell_html("file:///D:/a/clip.mp4", a_skin());
        // ① the picture fills the pane, at its own proportion.
        assert!(
            html.contains("width:100%;height:100%;object-fit:contain"),
            "the recording does not fill the pane:\n{html}"
        );
        for shrink_only in [
            "max-width:100%",
            "max-height:100%",
            "width:auto",
            "height:auto",
        ] {
            assert!(
                !html.contains(shrink_only),
                "{shrink_only} is the rule that only ever shrinks:\n{html}"
            );
        }
        // ② the engine's own bar is not asked for. Spelled as the attribute
        // between a space and a delimiter so that the word inside `controls` in
        // a comment or an id cannot answer for it.
        for spelling in [" controls>", " controls ", " controls=", "\"controls\""] {
            assert!(
                !html.contains(spelling),
                "the engine is still drawing the face:\n{html}"
            );
        }
        // ③ ours is, and it has the four the ruling named on it.
        assert!(html.contains("<div id=\"bar\">"), "{html}");
        for control in [
            "id=\"pp\"",   // play / pause
            "id=\"seek\"", // the scrubber
            "id=\"at\"",   // where it is
            "id=\"dur\"",  // how long it is
            "id=\"mu\"",   // mute
            "id=\"vol\"",  // volume
            "id=\"rate\"", // speed
        ] {
            assert!(html.contains(control), "{control} is missing:\n{html}");
        }
        // ④ and the two the ruling refused. Neither verb exists on this page:
        // a pane zooms from its own head, and a pane's content does not leave
        // the window it is in.
        for refused in [
            "requestFullscreen",
            "webkitRequestFullscreen",
            "requestPictureInPicture",
            "fullscreen",
        ] {
            assert!(!html.contains(refused), "{refused} is on the page:\n{html}");
        }
    }

    /// RED — **every colour on the bar is one of this window's, and it arrives
    /// as a `:root` token** (user ruling 2026-08-27; §7.23 ⑪).
    ///
    /// Two halves, and the second is what makes the first worth having:
    ///
    /// * Each of the six tokens is declared in `:root` with the value the
    ///   palette handed over. A build that dropped one would leave a rule
    ///   reading a `var()` that does not resolve, which is a transparent bar,
    ///   not a loud failure.
    /// * **No rule outside `:root` writes a colour of its own.** Every `#rrggbb`
    ///   in the stylesheet is inside the `:root` block; everything after it says
    ///   `var(--…)`. Without this half, "the page wears the palette" would be
    ///   true of a page that also wore three greys somebody typed.
    ///
    /// RED GATE: put a literal `#fff` into any rule below `:root` — which is
    /// what a hand-drawn control bar looks like on its first day — and the
    /// second half names it.
    #[test]
    fn the_bar_wears_the_windows_own_palette_and_writes_no_colour_of_its_own() {
        let skin = a_skin();
        let html = shell_html("file:///D:/a/clip.mp4", skin);
        for (token, value) in [
            ("--ground", skin.ground),
            ("--bar", skin.bar),
            ("--hairline", skin.hairline),
            ("--ink", skin.ink),
            ("--ink-muted", skin.ink_muted),
            ("--accent", skin.accent),
        ] {
            let [red, green, blue] = value;
            let declaration = format!("{token}:#{red:02x}{green:02x}{blue:02x};");
            assert!(
                html.contains(&declaration),
                "{declaration} is not in:\n{html}"
            );
        }
        // The face and the size come across the same boundary and by the same
        // means: a page cannot read `bt_render`, so it is written into it.
        assert!(html.contains("--ui-font:\"Inter\""), "{html}");
        assert!(html.contains("--ui-size:11.5px;"), "{html}");
        // And the fade is the house's, not a number this page chose.
        assert!(
            html.contains(&format!("--fast:{MOTION_FAST_MS}ms;")),
            "{html}"
        );

        let style = html
            .split_once("<style>")
            .expect("the shell has a stylesheet")
            .1
            .split_once("</style>")
            .expect("which is closed")
            .0;
        let after_root = style.split_once("}\n").expect("the `:root` block ends").1;
        // A `#` below `:root` is an id selector and there are several; a `#`
        // followed by hex digits is a colour and there may be none.
        for (at, _) in after_root.match_indices('#') {
            let following: String = after_root[at + 1..].chars().take(3).collect();
            assert!(
                !following.chars().all(|c| c.is_ascii_hexdigit()) || following.len() < 3,
                "a rule below `:root` writes a colour of its own — #{following}…:\n{after_root}"
            );
        }
        assert!(after_root.contains("var(--accent)"), "{after_root}");
    }

    /// RED — **the bar's marks are the symbol sheet's, not a second set drawn
    /// in CSS** (user ruling 2026-08-27; §7.23 ⑪).
    ///
    /// The four faces the two two-faced controls wear, each asserted against
    /// [`crate::marks::ChromeMark::artwork`]'s own body rather than against a
    /// path written out here — a copy of the artwork in a test is the second set
    /// of drawings this gate exists to forbid.
    ///
    /// `currentColor` survives the crossing, which is the difference between
    /// handing a document a drawing and handing a texture one: a texture has no
    /// cascade and gets a hex substituted, a document has one and takes the ink
    /// of the button it is in.
    ///
    /// RED GATE: draw the pause bars with a `<div>` and a border, or substitute
    /// a hex for `currentColor`, and this fails on the mark it was done to.
    #[test]
    fn the_bars_marks_come_off_the_symbol_sheet() {
        let html = shell_html("file:///D:/a/clip.mp4", a_skin());
        for mark in [
            ChromeMark::Play,
            ChromeMark::Pause,
            ChromeMark::Speaker,
            ChromeMark::SpeakerMuted,
        ] {
            let (view_box, body) = mark.artwork().expect("a quoted symbol");
            assert!(
                html.contains(body),
                "{mark:?} is not the sheet's drawing:\n{html}"
            );
            assert!(html.contains(&format!("viewBox=\"{view_box}\"")), "{html}");
        }
        assert!(html.contains("currentColor"), "{html}");
        // Four faces, two controls: each pair is one element showing at a time,
        // and which one is a class on the body rather than a thing the script
        // remembers.
        for face in ["f-play", "f-pause", "f-loud", "f-muted"] {
            assert!(html.contains(face), "{face} is missing:\n{html}");
        }
        assert!(html.contains("body.playing .f-pause"), "{html}");
        assert!(html.contains("body.muted .f-muted"), "{html}");
    }

    /// RED — **the bar's two waits are the registered ones, and the page is
    /// given them rather than told two numbers** (user ruling 2026-08-27;
    /// §7.23 ⑪).
    ///
    /// The reveal is the house's hover intent to the millisecond, and the rest
    /// is the dwell after. Both are Rust constants written into the script, so
    /// the motion register can hold them the way it holds every other wait in
    /// this window — a duration that lived only inside a JavaScript string
    /// would be the one span in the product no gate could see.
    ///
    /// RED GATE: type `250` into the script instead and the first assertion
    /// still passes, so the *second* is the gate: the constants have to be the
    /// ones the register names.
    #[test]
    fn the_bars_waits_are_the_ones_the_motion_register_holds() {
        let html = shell_html("file:///D:/a/clip.mp4", a_skin());
        assert!(
            html.contains(&format!(
                "var REVEAL_MS={PLAYER_BAR_REVEAL_INTENT_MS},REST_MS={PLAYER_BAR_IDLE_REST_MS};"
            )),
            "{html}"
        );
        assert_eq!(
            PLAYER_BAR_REVEAL_INTENT_MS,
            u64::try_from(crate::profiles::CHEVRON_HOVER_OPEN_DELAY.as_millis())
                .expect("a wait in this window is not four billion ms"),
            "a player's bar and a `⌄`'s menu are the same promise, made twice"
        );
        assert_eq!(PLAYER_BAR_IDLE_REST_MS, 2_000);
        // And the script reads the element rather than remembering what it was
        // told: the faces are painted from `v.paused` and `v.muted`.
        assert!(html.contains("!v.paused&&!v.ended"), "{html}");
        assert!(html.contains("v.muted||v.volume===0"), "{html}");
    }

    /// RED — **the keys the ruling named, and only while the page has the
    /// focus** (user ruling 2026-08-27; §7.23 ⑪).
    ///
    /// Space plays and pauses, `←`/`→` step five seconds, `↑`/`↓` move the
    /// volume, `M` mutes. They are hung on the document, which is what "only in
    /// the shell" means for a page inside a pane: a `keydown` reaches a document
    /// only when that document has the focus, so there is no second rule needed
    /// and no state to keep about whether the pane is the one being typed at.
    ///
    /// **A modified key is not one of these.** `Ctrl`, `Alt` and `Win` are the
    /// window's and the shell's shortcuts are unmodified, so a page that
    /// swallowed `Ctrl+←` would be eating one of the window's own.
    #[test]
    fn the_shell_takes_the_five_keys_and_leaves_every_modified_one() {
        let html = shell_html("file:///D:/a/clip.mp4", a_skin());
        for key in [
            "\" \"",
            "\"ArrowLeft\"",
            "\"ArrowRight\"",
            "\"ArrowUp\"",
            "\"ArrowDown\"",
            "\"m\"",
        ] {
            assert!(html.contains(key), "{key} is not bound:\n{html}");
        }
        assert!(
            html.contains("step(-5)") && html.contains("step(5)"),
            "{html}"
        );
        assert!(
            html.contains("if(e.ctrlKey||e.altKey||e.metaKey){return;}"),
            "a modified key belongs to the window:\n{html}"
        );
    }

    /// RED — **the four speeds the ruling named, in order, and they cycle**
    /// (user ruling 2026-08-27; §7.23 ⑪).
    #[test]
    fn the_speeds_are_the_four_that_were_ruled_on() {
        let html = shell_html("file:///D:/a/clip.mp4", a_skin());
        assert!(html.contains("var RATES=[1,1.25,1.5,2];"), "{html}");
        assert!(
            html.contains("RATES[(i+1)%RATES.length]"),
            "the speeds do not come round again:\n{html}"
        );
    }

    /// RED — **the mint records the name this window knows, and mints the URL
    /// from the name the disk knows** (found on the machine, 2026-08-27;
    /// `docs/DESIGN.md` §7.23 ⑩).
    ///
    /// The defect this is the gate for, in full: the first build handed the
    /// canonicalised path to both halves. On Windows `std::fs::canonicalize`
    /// answers a **verbatim** path — `\\?\D:\shots\clip.mp4` — while the pane's
    /// `PreviewImageState` holds `D:\shots\clip.mp4`, the spelling the reader
    /// arrived with. Those are one file and two strings, so `a_page_was_replaced`
    /// read the pane as one something else had landed on, the orphan rule retired
    /// the browser on the next turn, and what the machine showed was a page's
    /// chrome appearing for a frame and the first frame coming back.
    ///
    /// RED GATE: pass `canonical` for both and the first assertion fails on the
    /// prefix — which is exactly the frame that shipped and was caught.
    #[test]
    fn the_mint_records_the_name_and_mints_the_url_from_the_disk() {
        let named = Path::new(r"D:\shots\clip.mp4");
        let canonical = Path::new(r"\\?\D:\shots\clip.mp4");
        let mint =
            mint_player_shell(named, canonical, a_skin()).expect("a drive path mints a shell");
        assert_eq!(
            mint.video_behind_the_shell(),
            Some(named),
            "the window's own spelling is what every surface compares against"
        );
        // And the shell it wrote points at the disk's spelling, with the verbatim
        // prefix stripped by the one encoder that knows how — `Mint::file`.
        let shell = mint.target().expect("a shell mint names its URL");
        let path = Mint::path_and_tail_of_file_url(shell)
            .expect("this door minted it")
            .0;
        let html = std::fs::read_to_string(&path).expect("the shell is on the disk");
        assert!(
            html.contains("src=\"file:///D:/shots/clip.mp4\""),
            "the recording is reached by the path the disk agreed on:\n{html}"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// RED — **an attribute cannot be left by a file name** (2026-08-27).
    ///
    /// A Windows path may hold `&` and `'`, and a URL that carried either into a
    /// double-quoted attribute unescaped would end the attribute or start an
    /// entity. `<`, `>` and `"` cannot occur in a Windows path and are escaped
    /// anyway, because this function's contract is about HTML and not about
    /// NTFS — the day it is handed a string from somewhere else, it must still
    /// be true.
    #[test]
    fn no_file_name_can_end_the_attribute_it_is_written_into() {
        assert_eq!(
            attribute_escaped(r#"a&b'c<d>e"f"#),
            "a&amp;b&#39;c&lt;d&gt;e&quot;f"
        );
        let html = shell_html(r"file:///D:/a&b/o'clock.mp4", a_skin());
        assert!(html.contains("a&amp;b/o&#39;clock.mp4"), "{html}");
        assert!(!html.contains("a&b/"), "{html}");
    }

    /// RED — **one recording is one shell, and two recordings are two**
    /// (2026-08-27).
    ///
    /// The whole of the "who clears it" answer rests on this: the name is a
    /// function of the video's URL, so playing one file a hundred times writes
    /// one file, and two files that happen to share a name in two folders do not
    /// collide. It also pins the shape — a `.html`, because the engine is being
    /// asked for a document and an extension it does not know is how a page
    /// becomes a download again.
    #[test]
    fn a_shell_is_named_after_the_one_recording_it_is_for() {
        let one = shell_name_of("file:///D:/a/capture.mp4");
        let two = shell_name_of("file:///D:/b/capture.mp4");
        assert_ne!(one, two, "two folders are two recordings");
        assert_eq!(one, shell_name_of("file:///D:/a/capture.mp4"));
        assert!(one.ends_with(".html"), "{one}");
        assert!(one.starts_with("play-"), "{one}");
    }

    /// RED — **`%LOCALAPPDATA%`, beside the engine's folder and not inside it**
    /// (2026-08-27).
    ///
    /// Inside it would be writing into a directory another process owns and
    /// rewrites; `%APPDATA%` would roam a cache between machines, which is the
    /// mistake `webhost::user_data_folder_in` already has a test against.
    #[test]
    fn the_shells_live_beside_the_engines_profile_and_not_in_it() {
        let base = Path::new(r"C:\Users\x\AppData\Local");
        assert_eq!(
            shell_folder_in(base),
            PathBuf::from(r"C:\Users\x\AppData\Local\Folio\player")
        );
        assert_ne!(
            shell_folder_in(base),
            crate::webhost::user_data_folder_in(base)
        );
        assert!(!shell_folder_in(base).starts_with(crate::webhost::user_data_folder_in(base)));
    }
}
