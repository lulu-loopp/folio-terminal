//! The preview's content plane: what a file *is*, the tab's shared pool of live
//! buffers, and the thread that reads their heads off the event loop.
//!
//! **A buffer belongs to a FILE; a pane is a VIEW** (`DESIGN.md` §7.1.3, user
//! ruling 2026-07-17, which moved buffer ownership up from the pane to the tab).
//! Each tab owns one shared pool: a file open in two panes *is* the same buffer,
//! so edits cannot fork, and unsaved edits survive switching with no prompt at
//! all. The pool is a **history, not a second tab system** — clean buffers are
//! capped at [`PV_BUFFER_CAP`] and regrow on demand, while dirty ones and ones a
//! pane is currently showing are never evicted. Pane count grows only through the
//! explicit pin in the header.
//!
//! **Why a thread.** The same sentence `files` opens with: a file is not a data
//! structure, it is a question for a disk. §7.1.3 asks for an asynchronous,
//! cancellable head read of at most [`PREVIEW_HEAD_BYTES`], and this module owns
//! the shape `bt-files-worker` already owns — a named thread, a request channel,
//! a response channel, an [`AppEvent`] to wake the loop, newest-per-target
//! coalescing, and a one-way degradation when the thread is gone. Not a second
//! paradigm; the same one, on its own lane, so a slow file cannot sit behind a
//! slow directory.
//!
//! **What is decided here rather than there.** Two of §7.1.3's refusals are
//! answerable without touching the disk — a type this window has no reader for,
//! and a network path, which the design says is not to be read *automatically*
//! at all — so [`PreviewBuffer::new`] settles them and no request is ever sent.
//! The two that need bytes, a binary body and a file past the head limit, are the
//! worker's to answer.

use std::io::Read;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;
use std::time::SystemTime;

use anyhow::{Context, Result};
use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};
use winit::event_loop::EventLoopProxy;
use winit::window::WindowId;

use crate::preview_provenance::{BlockOrigins, TextOrigin};
use crate::{AppEvent, TabId};

/// How many buffers one tab's pool keeps.
///
/// `DESIGN.md` §7.1.3 writes the number down as the prototype's ("原型 8"),
/// which is to say it is a policy dial and not a law: what is load-bearing is
/// that the ceiling exists and that [`PreviewPool::open`] refuses to buy room
/// for it with anything a user would miss.
pub const PV_BUFFER_CAP: usize = 8;

/// How much of a file a preview reads.
///
/// §7.1.3's number, and the reason it is a *head* read rather than a whole one:
/// a preview is a look, and a look at the first screenful of a gigabyte costs
/// exactly as much as a look at the first screenful of a kilobyte. Past this the
/// buffer is marked truncated, which is what makes the read-only degradation the
/// design asks for expressible rather than silent.
pub const PREVIEW_HEAD_BYTES: usize = 64 * 1024;

/// How much of a file this window will take responsibility for editing.
///
/// **The head read stays the glance, and asking to edit buys a whole-file read**
/// (research §10 Q2, owner's ruling 2026-09-10). [`PREVIEW_HEAD_BYTES`] is what
/// a *look* costs, and it has to stay small for the reason written above it; but
/// a cap on the look became a cap on the feature, and the document that motivates
/// Markdown editing — `docs/DESIGN.md` — is far over 64KB. So there are two
/// reads on one lane now: the head, which every glance takes, and the whole
/// file, which is bought by asking to edit ([`PreviewBuffer::ask_for_the_whole_file`]).
///
/// This is the second read's own ceiling, and it exists because the first one's
/// reason does not go away — the body is a `String` in memory, re-parsed and
/// re-measured on every keystroke, and there is a size past which that is not an
/// editor but a hang. Past it the buffer keeps the head it has and stays
/// read-only, saying so through the same channel a truncated buffer already
/// speaks on.
pub const PREVIEW_EDIT_BYTES: usize = 8 * 1024 * 1024;

/// The notice shown once when the preview worker has stopped.
///
/// Worded like [`crate::files::files_worker_stopped_notice()`] and for the same
/// reason: a worker dying is a feature going away, not a session ending, and the
/// sentence has to say which half still works.
pub fn preview_worker_stopped_notice() -> &'static str {
    crate::i18n::Text::PreviewWorkerStopped.text()
}

/// What kind of document a name claims to be.
///
/// The mock-up's `previewFtype` (3087-3096), variant for variant. It is asked of
/// the *name* and nothing else — no magic numbers, no MIME sniffing — because the
/// answer has to be available before anything has been read, and because a name
/// is what a user is looking at when they decide what they expect to see. What a
/// name cannot know is answered later and separately: a `.txt` full of NULs is
/// still `Text` here and is refused by [`read_head`] on the evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewFtype {
    Image,
    Markdown,
    Table,
    Text,
    /// **A page** (Web 预览块 W2 片③) — the mock-up's `previewFtype === "web"`
    /// (`docs/DESIGN.md` §7.7 ①).
    ///
    /// **Two questions answer this class, and they answer it the same way**
    /// (user ruling 2026-08-23, "一个名字只该有一个含义"; §7.10 ⑥):
    ///
    /// * a *source* that is [`PreviewSource::Web`] is a page whatever it is
    ///   called — a page's name is its **title**, which is a sentence somebody
    ///   wrote, and asking [`preview_ftype`] about a page called
    ///   `release-notes.md` would draw a markdown document over a live browser;
    /// * a *name* in [`PAGE_EXTENSIONS`] is a page too, because it is opened as
    ///   one from every door in this window ([`crate::preview_open_lane`]).
    ///
    /// The second half is what the ruling added. Until it, `.html` was `Text`
    /// and `.pdf` was [`Self::Unknown`], so one file had two answers: the hover
    /// card said "no preview" while a double-click opened the page. A class is
    /// what a name *means*, and a name cannot mean two things at once.
    Web,
    /// **A video** (user ruling 2026-08-27; `docs/DESIGN.md` §7.23) — a name in
    /// [`VIDEO_EXTENSIONS`].
    ///
    /// It is **not** a page and this class is the record of that. §7.16's two
    /// pinned refusals stand exactly as they were measured: a top-level media
    /// response has no viewer in the engine on the preview seat, so a video sent
    /// down the page lane draws a browser error where an honest card used to be.
    /// What changed on 2026-08-27 is not where a video opens but what is *in* the
    /// thing it opens: this window grew a decoder of its own
    /// ([`bt_platform::video::first_frame`]), so the surface that used to say "no
    /// preview for this file type" shows the video's own picture and says how
    /// long it is. What changed on 2026-08-28 is that the same decoder learned
    /// to keep going: a name in this class **plays**, on this window's own
    /// glass, on all three surfaces that can hold one (§7.44).
    ///
    /// So this class means one thing and only one: **a name this window has a
    /// video lane for.** It still does not promise a picture on every machine —
    /// a codec that ships as a Store extension is absent until it is installed —
    /// and that is answered by the engine's own error at the moment of opening
    /// rather than by this table. See [`VIDEO_EXTENSIONS`].
    Video,
    /// No reader in this window. The "no preview" card, by name alone.
    Unknown,
}

impl PreviewFtype {
    /// The word the type chip prints (P147, mock-up 6422).
    ///
    /// The mock-up interpolates the ftype string itself into `.fpeek-type`, so
    /// the chip says exactly what the classifier calls the file and there is no
    /// second, prettier vocabulary to keep in step with it. These are those
    /// strings.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Markdown => "markdown",
            Self::Table => "table",
            Self::Text => "text",
            Self::Web => "web",
            Self::Video => "video",
            Self::Unknown => "unknown",
        }
    }
}

/// **The word the type chip prints over a named file** — its *type*, and never
/// the name of the lane that draws it (user ruling 2026-08-29).
///
/// [`PreviewFtype::label`] names the **class**, and for six of the seven the
/// class and the type are one word. [`PreviewFtype::Web`] is the seventh and it
/// is a lane: `html`, `htm` and `pdf` share it because one engine reads all
/// three, and the chip printing that lane's name put `web` in the corner of a
/// card over a report — the reader was told how this window draws the file
/// instead of what the file is.
///
/// So the class is asked first and the **name** decides only inside it, out of
/// [`PAGE_EXTENSIONS`]'s own third column. That order is what keeps a *page with
/// no file* honest: a live page's name is its title, it is in no table, and the
/// chip over it still says `web` — which is exactly what that thing is.
#[must_use]
pub fn type_label(name: &str, ftype: PreviewFtype) -> &'static str {
    let PreviewFtype::Web = ftype else {
        return ftype.label();
    };
    std::path::Path::new(name)
        .extension()
        .and_then(|extension| {
            PAGE_EXTENSIONS
                .iter()
                .find(|(page, ..)| extension.eq_ignore_ascii_case(page))
                .map(|(.., word)| *word)
        })
        .unwrap_or_else(|| ftype.label())
}

/// Extensions that name a picture — the mock-up's list (3090).
const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "svg", "gif", "webp"];

/// Extensions that name a **page** — [`PreviewFtype::Web`] asked of a name
/// (user ruling 2026-08-23; `docs/DESIGN.md` §7.10 ⑥).
///
/// **This is the page class, and it is written down exactly once.** Every door
/// in this window asks it: [`preview_ftype`] asks it of a *name*, and
/// [`path_names_a_page`] — which [`crate::path_opens_as_a_page`] is — asks it of
/// a *path*. Until 2026-08-25 the second was a hand-written `html || htm || pdf`
/// beside this table, and the two duplicate lists are precisely how a class
/// comes apart: §7.10 ⑥'s second ruling exists because a `.pdf` had already
/// drifted out of one of them.
///
/// `htm` sits beside `html` because the shortened spelling is the same object —
/// Windows registers both against the same handler. `pdf` sits beside them
/// because the browser this seat already hosts has a reader for it and this
/// window has none.
///
/// **No video spelling is here, and that is a measurement rather than a
/// preference** (2026-08-25; `docs/DESIGN.md` §7.16).
///
/// The ticket that opened this table on 2026-08-25 asked for `mp4` and its
/// neighbours on the argument that made PDF a member: the engine on this seat
/// has a player and this window has none. **The engine has a player and it will
/// not host one at the top level.** Measured in the real window, on this build:
/// `file:///…/clip.mp4` and `file:///…/screencast.webm` both complete as
/// `WebErrorStatus · ConnectionAborted` and draw the 「did not respond」 card,
/// while the very same `clip.mp4` plays with controls as a `<video>` **inside**
/// an ordinary local page in the same seat (`canPlayType` answers `maybe` for
/// `video/mp4`, `video/webm` and `video/x-m4v`, and `""` for `video/quicktime`).
/// A top-level media response has no viewer in WebView2 — it becomes a download,
/// and `bt_platform`'s `DownloadStarting` handler cancels every download
/// unconditionally, which is the abort.
///
/// **The second column is what a glance can show of the file itself** (user
/// ruling 2026-08-25) — see [`PageGlance`]. It rides on this table rather than
/// beside it for the reason the table exists at all: a page class written down
/// twice is a page class that comes apart, and "which of these can be read as
/// text" is a fact about the very extensions listed here.
///
/// So a video on this lane would replace an honest "no preview for this file
/// type" card with a browser error, which is strictly worse than the refusal it
/// replaced — and the ticket's own words were 「能播的才进,播不了的仍落卡」.
/// The lane stays shut until something hosts the file *inside* a page; that is a
/// design with its own questions (what the address bar says, what the head's `↗`
/// hands over, what `session.json` stores) and it is the user's to rule on.
/// PDF is not the counter-example: WebView2 ships a PDF viewer, so a top-level
/// PDF renders — verified in the same session.
///
/// **`html` and `htm` left [`TEXT_EXTENSIONS`] to get here**, and that is the
/// whole of the cost the ruling accepted: `.html` used to be shown as source by
/// default. What replaced the default is not nothing — the head's `</>` opens the
/// page's own developer tools and its `↗` hands the file to a browser — but it
/// is no longer what a double-click does, because a double-click on a name that
/// says "page" now means the page.
/// **The third column is the word the type chip prints** (user ruling
/// 2026-08-29): the chip names the **file's type**, and `Web` is the name of the
/// *lane* three spellings share. A `.pdf` card wore `web` in its corner for as
/// long as the chip printed [`PreviewFtype::label`] — a reader hovering a report
/// was told the name of an internal rendering route. The word rides this table
/// for the reason the glance column does: what a `.pdf` is called and which
/// reader opens it are facts about the same row, and written apart they drift.
///
/// `htm` prints `html` because the shortened spelling is the same object — the
/// sentence already written above this table about why the two rows exist at
/// all.
const PAGE_EXTENSIONS: [(&str, PageGlance, &str); 3] = [
    ("html", PageGlance::Source, "html"),
    ("htm", PageGlance::Source, "html"),
    ("pdf", PageGlance::Facts, "pdf"),
];

/// **What the glance card shows of a page's own file** (user ruling 2026-08-25;
/// `docs/DESIGN.md` §7.10 ⑥) — [`PAGE_EXTENSIONS`]'s second column.
///
/// The hover card has no engine: a page is drawn by one, on a seat. From the
/// 2026-08-23 ruling until this one, that fact was the whole of what the card
/// said about every member of the class — one line, `Opens as a page.` — and it
/// was right about the card and wrong about the file. `.html` **is** text; a
/// reader who rests the pointer on one is asking what is in it, and a card that
/// answers with a sentence about double-clicking has refused a question it could
/// have answered.
///
/// So the class splits on a property of the *bytes*, which is why this is a
/// column of the extension table and not a switch in the card: whether the file
/// a page is made of is something this window can already read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageGlance {
    /// **The page's own source.** It goes down the same lane every other text
    /// file takes — the same head read, the same mono body, the same 64KB cap —
    /// because it is text, and a second lane for it would be a second answer to
    /// "what does this file look like".
    Source,
    /// **A binary container**: nothing this window reads is *in* it. What the
    /// glance can state without opening a renderer is what the file is — how
    /// large, and how many pages — and that is what it states. See
    /// [`crate::pdf::page_count`] for the half of that which has to be read off
    /// the structure, and for what it does when the structure will not say.
    Facts,
}

/// **What a glance shows of the file at this path**, or `None` when the path is
/// not a page at all.
///
/// [`path_names_a_page`]'s own reading of the same table, carrying the second
/// column out with the answer — so every rule written on that function holds
/// here without being restated: the **real** extension and never a substring
/// (`index.htmlx` and `report.html.txt` are documents), case ignored, and a file
/// whose whole name is `.html` has no extension at all and is therefore not a
/// page (§7.1.5j ⑦(e)).
#[must_use]
pub fn path_page_glance(path: &std::path::Path) -> Option<PageGlance> {
    let extension = path.extension()?;
    PAGE_EXTENSIONS
        .iter()
        .find(|(page, ..)| extension.eq_ignore_ascii_case(page))
        .map(|(_, glance, _)| *glance)
}

/// Extensions that name a **video** — [`PreviewFtype::Video`] asked of a name
/// (user ruling 2026-08-27; extended and re-measured 2026-08-28, route B slice
/// ②; `docs/DESIGN.md` §7.23, §7.44 ⑥).
///
/// **Seven spellings and one column, and losing the second column is the
/// ruling.** While a video was played inside a page, "this window can draw it"
/// and "this window can play it" were two questions with two decoders behind
/// them: Media Foundation read the container for the still, and Chromium's
/// `canPlayType` decided the playback. They disagreed about `.mov` — the
/// platform opens it as an ordinary MPEG-4 file source and the engine answered
/// `canPlayType('video/quicktime')` with the empty string — so the table grew a
/// column saying which members had a face and no player.
///
/// Route B retired the browser from this lane (§7.44 ④). The still and the
/// playback now come off **one decoder**, so the two sets are the same set by
/// construction and a column distinguishing them can have no member in it. A
/// two-valued enum with one unreachable value is a distinction this window would
/// go on maintaining after it stopped being true, so it is gone and this is one
/// list again.
///
/// # The four that were added, and how
///
/// `.mov` came off the `FaceOnly` column, and `.mkv`, `.avi` and `.wmv` came
/// from outside the table altogether — §7.23 (f) had refused all three on the
/// grounds that "this window's decoder cannot read them", which was a claim
/// about the browser and not about the platform. **All four were opened**
/// (2026-08-28, this machine, `Engine::open` on a fixture of each, three frames
/// drawn and the playhead past 0.41s in every case; the numbers are in §7.44
/// ⑥). Media Foundation ships an AVI source and an ASF source out of the box and
/// reads Matroska, and `CanPlayType` answers `No` for every one of them — which
/// is §7.42 ⑧'s finding reaching this table: **the matrix is built by opening
/// files, not by asking that function.**
///
/// # What the table still is not
///
/// It is a promise about **this window**, not a report about the platform, and
/// two directions of slack are real and neither is a hole:
///
/// * A `.webm` carrying VP9, or an `.mp4` carrying HEVC, needs a decoder that
///   ships as a Store extension. Both were measured playing here because this
///   machine has them; on a stock Windows the same file is
///   `EngineError::Unsupported` at the moment it is opened. That is a fact about
///   a machine and it is said where a machine can say it — on the card, out of
///   the engine's own error — not in a constant compiled on a build server.
/// * A name outside this list gets the "no preview for this file type" card it
///   has always had. Adding a row is adding a lane, and a lane is worth adding
///   when there is a fixture that proves it.
const VIDEO_EXTENSIONS: [&str; 7] = ["mp4", "m4v", "webm", "mov", "mkv", "avi", "wmv"];

/// **Whether an extension is in [`VIDEO_EXTENSIONS`]**.
///
/// [`preview_ftype`]'s own reading of the table, lifted out so that the class
/// question is one lookup in one list rather than a spelling repeated at each
/// caller — the drift §7.10 ⑥ paid for.
#[must_use]
fn extension_names_a_video(extension: &str) -> bool {
    VIDEO_EXTENSIONS
        .iter()
        .any(|video| extension.eq_ignore_ascii_case(video))
}

/// **Whether a path's real extension names a video** — [`VIDEO_EXTENSIONS`]
/// asked of a `Path` rather than of a name.
///
/// **The one predicate the face, the play button, the play verb and the red
/// gates all read**, which is what collapsing the second column bought: a frame
/// with a play button over it and a seat that would accept a play cannot come
/// apart, because there is no longer a second question they could answer
/// differently. [`crate::Runtime::play_video_on`] refuses exactly the way this
/// answers.
///
/// [`path_names_a_page`]'s twin, and deliberately the same shape down to the
/// last clause, so every rule already written on that function holds here
/// without being restated: the **real** extension and never a substring of the
/// name (`clip.mp4.txt` is a text file that spells a video in the middle of
/// itself), case ignored, and a file whose whole name is `.mp4` has no extension
/// at all and is therefore not a video (§7.1.5j ⑦(e)).
#[must_use]
pub fn path_names_a_video(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(extension_names_a_video)
}

/// Extensions that name something this window can show as text (3093).
///
/// **`html` and `htm` are not here** (user ruling 2026-08-23): they moved to
/// [`PAGE_EXTENSIONS`]. They were in this table for as long as a page's source
/// was the only thing this window could do with one, and W2 ended that.
const TEXT_EXTENSIONS: [&str; 13] = [
    "rs",
    "py",
    "js",
    "ts",
    "json",
    "toml",
    "txt",
    "gitignore",
    "lock",
    "yml",
    "yaml",
    "diff",
    "patch",
];

/// The extension a name carries, lower-cased, or `""` when it carries none.
fn extension_of(name: &str) -> String {
    match name.rfind('.') {
        Some(dot) => name[dot + 1..].to_ascii_lowercase(),
        None => String::new(),
    }
}

/// Which of the five classes a file name belongs to.
pub fn preview_ftype(name: &str) -> PreviewFtype {
    let ext = extension_of(name);
    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return PreviewFtype::Image;
    }
    if ext == "md" {
        return PreviewFtype::Markdown;
    }
    if ext == "csv" {
        return PreviewFtype::Table;
    }
    // A name that begins with a dot is a dotfile — `.env`, `.gitattributes` —
    // and dotfiles are text by convention rather than by extension, which is
    // exactly why the name and not the extension is asked here.
    if TEXT_EXTENSIONS.contains(&ext.as_str()) || name.starts_with('.') {
        return PreviewFtype::Text;
    }
    // **After the dotfile clause, and that order is the ruling reaching one more
    // table.** A file whose whole name is `.html` has no extension as far as
    // `Path::extension` is concerned, so [`path_names_a_page`] answers
    // `false` for it (§7.1.5j ⑦(e)) and this window opens it as a document. Ask
    // the page question first and the two would disagree about one name, which
    // is the thing this ruling exists to end.
    if PAGE_EXTENSIONS.iter().any(|(page, ..)| *page == ext) {
        return PreviewFtype::Web;
    }
    // **After the page question and before `Unknown`**, which is where a class
    // that is not a page and is not nothing belongs. The two tables cannot
    // overlap — a video spelling in [`PAGE_EXTENSIONS`] is the thing §7.16's
    // pinned refusal exists to catch — so the order between them is a
    // formality, and it is written this way round because that refusal is the
    // older claim and reads first.
    if extension_names_a_video(&ext) {
        return PreviewFtype::Video;
    }
    PreviewFtype::Unknown
}

/// **Whether a path's real extension names a page** — [`PAGE_EXTENSIONS`] asked
/// of a `Path` rather than of a name.
///
/// [`crate::path_opens_as_a_page`] is this function, and that identity is the
/// point: a predicate that says which lane a name opens on and a table that says
/// which class a name is in are one claim, and while they were two lists one of
/// them drifted (§7.10 ⑥, the second ruling of 2026-08-23).
///
/// **The real extension and never a substring of the name**: `index.htmlx` is a
/// template dialect this window has no browser story for, and `report.html.txt`
/// is a text file that merely spells `.html` in the middle of itself. Both are
/// the preview seat's, and a `contains`/`ends_with` reading would take one of
/// them out of it.
///
/// **A whole name of `.html` is not a page here either**, and it does not need a
/// clause: `Path::extension` says a leading-dot name has no extension at all
/// (§7.1.5j ⑦(e)), which is the very answer [`preview_ftype`]'s dotfile arm
/// gives one line earlier. Two readings, one answer, without either knowing
/// about the other.
///
/// The comparison ignores case because a file system that does not distinguish
/// `TIMELINE.HTM` from `timeline.htm` would leave this window doing two things
/// with one file.
#[must_use]
pub fn path_names_a_page(path: &std::path::Path) -> bool {
    path.extension().is_some_and(|extension| {
        PAGE_EXTENSIONS
            .iter()
            .any(|(page, ..)| extension.eq_ignore_ascii_case(page))
    })
}

/// Whether a name is a patch, which is read and never edited.
///
/// `#[allow(dead_code)]`: this and [`is_editable`] are the block's *judgement*
/// about which surface a file gets, and the surfaces that consume it — the diff
/// view and the quick-edit textarea — are slices 2 and 3. The judgement is
/// written and pinned by test now rather than then, because the mock-up got it
/// wrong twice in two places (P58/P107) and a rule discovered at the second call
/// site is a rule the first one has already broken.
///
/// The judgement is the name's rather than the type's because `.diff` and
/// `.patch` sit inside the text list: they are text, they are shown as a diff,
/// and *editing a diff edits nothing real* (mock-up 4970-4978).
///
/// **This is about files, and it stays about files** (R24, 2026-08-15). A real
/// `.diff` on a disk earns the diff view here, by its name, as it always has.
/// What no longer comes through this door is a *git* diff: it is one because of
/// what [`PreviewBuffer::source`] says it is, decided in [`PreviewBuffer::view`],
/// and never because somebody gave it a display name ending in `.diff` so that
/// this rule would sweep it up.
#[allow(dead_code)]
pub fn is_diff_name(name: &str) -> bool {
    matches!(extension_of(name).as_str(), "diff" | "patch")
}

/// Whether this content would be shown on a surface that edits.
///
/// **The judgement is about the surface, not about the extension** (ruling 3,
/// 2026-08-12). The mock-up asked it twice and got two different answers — the
/// pane said `text || markdown`, the float said `text || table || (markdown &&
/// mdSource)` — and both were wrong in the same way: they named types rather
/// than the view those types actually get. A table is a grid and a diff is a
/// reading, so neither is editable however much its extension looks like text,
///
/// **`md_source` no longer decides anything, and that one word is ticket T5**
/// (§7.1.3t). It used to be the whole of the Markdown arm: a rendered page had
/// no caret to put anywhere, so the only editor a `.md` file had was its source
/// face. Both faces edit now — the rendered one by drawing the caret's block as
/// the file's own bytes (§7.1.3q) — so the *view* has stopped being part of the
/// judgement, and what is left is the name and the type, which is what this
/// function always said it was about. The parameter stays because the flip is
/// still the view's and every caller still has to say which face it is asking
/// about; the day one of them stops passing it is the day it has stopped
/// knowing.
pub fn is_editable(name: &str, ftype: PreviewFtype, md_source: bool) -> bool {
    let _ = md_source;
    if is_diff_name(name) {
        return false;
    }
    match ftype {
        PreviewFtype::Text | PreviewFtype::Markdown => true,
        // A page has no text of this window's to put a caret in: what is on the
        // glass belongs to the engine, and the one place typing goes is inside
        // the page itself. A video is a picture here, and for the same reason a
        // `.png` is not editable — there is nothing on the surface that is text.
        PreviewFtype::Image
        | PreviewFtype::Table
        | PreviewFtype::Web
        | PreviewFtype::Video
        | PreviewFtype::Unknown => false,
    }
}

/// Which body a buffer is drawn as.
///
/// The mock-up's `previewBodyHtml` (4942-4988) is a ladder of `if`s and **the
/// order is a ruling**, not an accident of writing. Kept as one function
/// returning one value so that every surface — the pane, the float, the hover
/// card, and whatever asks next — reads the same answer rather than
/// re-descending a ladder of its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewView {
    Image,
    /// Markdown, rendered. The source view is [`Self::Text`].
    Markdown,
    Table,
    Diff,
    Text,
    /// **One repository's commit graph** (G-4).
    ///
    /// Its own answer and not [`Self::None`], which is what it used to be while
    /// the surface did not exist. The two are opposites: `None` is "there is
    /// nothing to show here" and draws a card saying so, while this is "there is
    /// a great deal to show here and it is not text". Nothing in the *body*
    /// pipeline draws it — the picture is chrome, in the pane's own body
    /// rectangle ([`crate::git_graph::push_graph`]) — so what this variant buys
    /// is that every surface asking "what is this" gets the true answer rather
    /// than the one that happens to render the same.
    Graph,
    /// **One page, drawn by the engine** (Web 预览块 W2 片③).
    ///
    /// Its own answer for [`Self::Graph`]'s reason twice over: there is a great
    /// deal to show here and none of it is this window's to paint. The pixels
    /// arrive through the composition tree, under wgpu, through the hole
    /// `bt_render::WindowRenderer::set_web_holes` punches (§7.8 ②) — so what
    /// this variant buys is that a surface asking "what is this" is told the
    /// truth, and that no host quietly draws a "no preview" card over a live
    /// browser.
    Web,
    /// **One video, standing still** (user ruling 2026-08-27; §7.23).
    ///
    /// Its own answer rather than [`Self::Image`], and the difference is what it
    /// is a view *of*. The pixels take the same channel a decoded picture takes
    /// — they are one frame of RGBA and the host that draws a `.png` draws them
    /// without knowing — but a picture's meta line says how many pixels the file
    /// has and a video's says **how long it is**, and a view is precisely the
    /// thing that decides which sentence a surface writes.
    ///
    /// **It does not play, and that is this slice's whole boundary.** The frame
    /// is a face: it says which clip this is, how long, how large. Playing is a
    /// second slice with a route already ruled on (a page hosting a `<video>`;
    /// `docs/DESIGN.md` §7.23 ④), and it will arrive as a different view rather
    /// than as this one growing controls.
    Video,
    /// The "no preview" card.
    None,
}

impl PreviewView {
    /// **Which machine draws this body**, asked once for every host.
    ///
    /// [`PreviewView`] says what a document *is*; this says who paints it, and
    /// the two are not the same list — four of the seven views are one
    /// pipeline's and the remaining three are three different arrangements.
    /// The distinction earns its own type because a host that got it wrong drew
    /// nothing at all: the preview float's body was two `if`s, one for the
    /// document pipeline and one for the picture, so a commit graph torn off
    /// into a window arrived as a head, a foot and an empty rectangle (user
    /// report, 2026-08-20).
    ///
    /// **Exhaustive on purpose.** Both hosts `match` this rather than testing
    /// for the kinds they happen to know about, so the next content kind — the
    /// web block's page, when it comes — is a compiler error in every host on
    /// the day it is added, and not a second blank window discovered by
    /// somebody undocking one.
    #[must_use]
    pub fn chrome(self) -> PreviewChrome {
        match self {
            // Pixels, on the host's own image channel: a seat spends the
            // renderer's one `set_preview_image` slot, a float and the glance
            // card ride their layer.
            //
            // **A video's frame is on that channel too**, and joining it rather
            // than growing a fifth arrangement is the whole reason the frame is
            // handed back as RGBA: a still is a picture, whatever produced it,
            // and a second painter for the same shape of pixels would be a
            // second place for a float to be forgotten in (which is exactly how
            // [`Self::Graph`] arrived as an empty window in 2026-08-20).
            Self::Image | Self::Video => PreviewChrome::Picture,
            // Marks in the body rectangle, pushed by
            // [`crate::git_graph::push_graph`] — see [`Self::Graph`] for why the
            // picture is chrome and the document is empty.
            Self::Graph => PreviewChrome::Graph,
            // Paragraphs and quads through `PreviewBody`. [`Self::None`] is here
            // because the card is that pipeline's own answer to "nothing", and
            // not a fourth arrangement.
            Self::Markdown | Self::Table | Self::Diff | Self::Text | Self::None => {
                PreviewChrome::Document
            }
            // The page, composed under this surface by the engine and seen
            // through the hole punched in it — see [`Self::Web`].
            Self::Web => PreviewChrome::Web,
        }
    }
}

/// **What paints a preview surface's body** — see [`PreviewView::chrome`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewChrome {
    /// The document pipeline, and the "no preview" card it draws for nothing.
    Document,
    /// A decoded picture, on the host's image channel.
    Picture,
    /// One repository's commit graph, pushed into the body rectangle.
    Graph,
    /// **Nothing, on purpose** — a page's pixels are the engine's, composed
    /// under this surface and seen through the hole in it (§7.8 ②). A host that
    /// painted a body here would be painting over a browser.
    Web,
}

/// Which body this name, type and flip state earn.
pub fn preview_view(name: &str, ftype: PreviewFtype, md_source: bool) -> PreviewView {
    if ftype == PreviewFtype::Image {
        return PreviewView::Image;
    }
    if ftype == PreviewFtype::Video {
        return PreviewView::Video;
    }
    if ftype == PreviewFtype::Markdown && !md_source {
        return PreviewView::Markdown;
    }
    if ftype == PreviewFtype::Table {
        return PreviewView::Table;
    }
    // **Before the text surface, which is the whole point.** `.diff` is text by
    // extension, so asking the type first would hand a patch a textarea — and
    // editing a diff edits nothing real.
    if is_diff_name(name) {
        return PreviewView::Diff;
    }
    if ftype == PreviewFtype::Text || (ftype == PreviewFtype::Markdown && md_source) {
        return PreviewView::Text;
    }
    PreviewView::None
}

/// What one line of a diff is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffLineKind {
    /// `+++`, `---`, `diff …` — the header, which is about the *file*.
    Meta,
    /// `@@ … @@` — where in the file the next lines are.
    Hunk,
    Add,
    Del,
    /// Everything else: the unchanged lines a hunk carries for context.
    Context,
}

impl DiffLineKind {
    /// Whether this line stands on a coloured band.
    pub fn tints(self) -> bool {
        matches!(self, Self::Add | Self::Del)
    }
}

/// Classify one line of a diff.
///
/// **The three-character prefixes are asked first**, and that ordering is the
/// only thing keeping `--- a/src/main.rs` — a diff's own header — out of the red
/// band it would otherwise be painted in. The mock-up gets this right at
/// 4973-4976 and it is easy to get wrong by writing the shorter test first.
pub fn diff_line_kind(line: &str) -> DiffLineKind {
    if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") {
        DiffLineKind::Meta
    } else if line.starts_with("@@") {
        DiffLineKind::Hunk
    } else if line.starts_with('+') {
        DiffLineKind::Add
    } else if line.starts_with('-') {
        DiffLineKind::Del
    } else {
        DiffLineKind::Context
    }
}

/// How one run of inline text is set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpanStyle {
    Plain,
    /// `**a**` or `__a__` — CommonMark strong emphasis, two delimiters a side.
    Bold,
    /// `*a*` or `_a_` — CommonMark emphasis, one delimiter a side. Set in the
    /// family's italic face where it has one and a synthesised oblique where it
    /// does not (see [`bt_render::PreviewRun::italic`]); a face this window
    /// gained on 2026-08-28, having drawn every level of emphasis bold until
    /// then.
    Italic,
    /// `***a***` — emphasis inside strong emphasis, set bold **and** italic.
    BoldItalic,
    /// `` `like this` `` — set in the monospace face.
    Code,
    /// `[text](url)` — **the text only** is printed, in the accent colour; the
    /// target rides beside it in [`Span::target`].
    ///
    /// It used to be printed and nothing else: "what a click on a link *does*
    /// is a decision about opening browsers and relative paths that belongs to
    /// the day the preview grows a navigation model." That day is 2026-08-13
    /// and the decision is [`link_action`]. What has not changed is that the
    /// URL is never *printed* — that was wrong under every future ruling and it
    /// is still wrong under this one.
    Link,
    /// `$…$`, `\(…\)` or a bare `\begin{pmatrix}…\end{pmatrix}` — one inline
    /// formula, **stored with its delimiters**.
    ///
    /// The delimiters stay in [`Span::text`] because they are what the run draws
    /// while it has no picture yet, and because they are what it goes back to
    /// when the engine refuses the source: a formula that cannot be set is the
    /// author's literal text and must come back byte for byte. The LaTeX handed
    /// to the engine is [`Span::math_source`], which is the same string without
    /// the delimiters — and for an environment it is the same string, because an
    /// environment has no delimiters to drop: the `\begin` and the `\end` are the
    /// formula.
    Math,
    /// `![alt](src)` — a picture. [`Span::text`] is the alt text and
    /// [`Span::target`] is the source, unresolved, exactly as [`Self::Link`]
    /// carries a link's.
    ///
    /// **A span rather than only a block, because the scanner that finds one is
    /// the scanner that finds a link** — an image *is* a link with a `!` in front
    /// of it, and CommonMark reads the two in one pass. What becomes of the span
    /// afterwards is [`push_prose`]'s business: a paragraph is cut at its
    /// pictures so each one becomes a [`MarkdownBlock::Image`], and in the four
    /// places a paragraph cannot be cut — a heading, a table cell, a list item, a
    /// quote line — the run stands as its own alt text. That floor is the honest
    /// one: alt text is what a picture *says*, and it is what every reader that
    /// cannot show the picture has put in its place since there were pictures.
    Image,
}

/// One run of inline text inside a markdown block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: SpanStyle,
    /// Where a [`SpanStyle::Link`] points, exactly as the document wrote it.
    ///
    /// **Unresolved.** A relative target means nothing without the document it
    /// was written in, and the parser does not know which file it is reading —
    /// so it keeps the author's string and [`link_action`] does the resolving,
    /// where the document's own path is in hand. `None` for every other style,
    /// which is what makes "a run that answers a click" a thing the type can
    /// state rather than a convention two modules have to agree on.
    pub target: Option<String>,
}

impl Span {
    fn styled(text: &str, style: SpanStyle) -> Self {
        Self {
            text: text.to_owned(),
            style,
            target: None,
        }
    }

    pub fn plain(text: &str) -> Self {
        Self::styled(text, SpanStyle::Plain)
    }

    pub fn bold(text: &str) -> Self {
        Self::styled(text, SpanStyle::Bold)
    }

    pub fn italic(text: &str) -> Self {
        Self::styled(text, SpanStyle::Italic)
    }

    pub fn bold_italic(text: &str) -> Self {
        Self::styled(text, SpanStyle::BoldItalic)
    }

    pub fn code(text: &str) -> Self {
        Self::styled(text, SpanStyle::Code)
    }

    pub fn link(text: &str, target: &str) -> Self {
        Self {
            text: text.to_owned(),
            style: SpanStyle::Link,
            target: Some(target.to_owned()),
        }
    }

    /// One inline formula, `text` **including** whatever delimits it.
    pub fn math(text: &str) -> Self {
        Self::styled(text, SpanStyle::Math)
    }

    /// **This run, standing inside a link that points at `target`.**
    ///
    /// A link's label is inline content (CommonMark §6.3), so its runs keep the
    /// faces they were written in and what the label gives them all is the
    /// target: [`Span::target`] says "this run answers a click", and it is the
    /// one field the window reads to make a run answer one
    /// (`main::note_link_sites`). Plain words become [`SpanStyle::Link`], which
    /// is the style that means "a link's plain text"; a code span stays code, a
    /// bold word stays bold, a formula stays a formula.
    ///
    /// **A picture keeps its own target and is not made to answer**, because
    /// [`SpanStyle::Image`]'s target is where its pixels are and there is only
    /// one of those fields. `[![alt](img)](url)` is a badge, and a badge draws
    /// its picture — a picture is not a link in this window (2026-08-28, the
    /// ruling `main::note_link_sites` states), so what is lost is a click this
    /// window would not have offered on it anyway.
    fn linked(mut self, target: &str) -> Self {
        if self.style == SpanStyle::Image {
            return self;
        }
        if self.style == SpanStyle::Plain {
            return Self::link(&self.text, target);
        }
        self.target = Some(target.to_owned());
        self
    }

    /// One picture: the alt text it says, and the source it names.
    pub fn image(alt: &str, src: &str) -> Self {
        Self {
            text: alt.to_owned(),
            style: SpanStyle::Image,
            target: Some(src.to_owned()),
        }
    }

    /// The LaTeX between this run's delimiters, or `None` if it is not a formula.
    ///
    /// One accessor rather than a second copy of the source on the span, because
    /// the delimited text and the source are the same bytes read two ways, and a
    /// pair of fields is a pair that can disagree.
    ///
    /// A run delimited by neither `$…$` nor `\(…\)` is a bare environment, and a
    /// bare environment is its own source: there is nothing to take off it.
    #[must_use]
    pub fn math_source(&self) -> Option<&str> {
        if self.style != SpanStyle::Math {
            return None;
        }
        let text = self.text.as_str();
        Some(
            text.strip_prefix('$')
                .and_then(|rest| rest.strip_suffix('$'))
                .or_else(|| {
                    text.strip_prefix(INLINE_MATH_OPEN)
                        .and_then(|rest| rest.strip_suffix(INLINE_MATH_CLOSE))
                })
                .unwrap_or(text),
        )
    }
}

/// One row of a rendered markdown table: one cell per column, each already
/// split into its inline runs.
pub type TableRow = Vec<Vec<Span>>;

/// One block of a rendered markdown document.
///
/// **The support surface is the product's, not the prototype's.** The mock-up's
/// own renderer stops at headings, lists, fences and two inline styles and says
/// so in a comment — "completeness is the product's problem" (4914-4941). This
/// is the product, the file the user read it against is `docs/DESIGN.md`, and
/// what that file uses and the prototype could not draw is exactly the five
/// members below the first four.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MarkdownBlock {
    Heading {
        /// `1..=6`. The prototype allowed `1..=3` and printed the rest as
        /// paragraphs beginning with hashes, which is what a `####` in a real
        /// document looks like when it is not supported.
        level: u8,
        spans: Vec<Span>,
    },
    /// Consecutive list rows, gathered the way the mock-up's `flushList` gathers
    /// them: a list is one block, not one block per bullet.
    List {
        /// `Some(n)` for a `n.`-numbered list, `None` for a bulleted one.
        ///
        /// The *first* number rather than a flag, because a list that starts at
        /// `3.` is a list that starts at 3 — renumbering it from one is the
        /// renderer overruling the document about its own contents.
        ordered: Option<u64>,
        items: Vec<Vec<Span>>,
    },
    Code {
        lang: Option<String>,
        text: String,
    },
    /// `| a | b |` under a `|---|---|` — **and only under one**.
    ///
    /// The separator row is what makes a table a table. Without it a line full
    /// of pipes is a line full of pipes, which is the common case in prose about
    /// shell commands and in ASCII art, and a renderer that tabulated those
    /// would be wrong far more often than it was right.
    Table {
        /// The heading row first, then the body. Never empty: a table exists
        /// only where a heading row was found.
        rows: Vec<TableRow>,
        /// What the separator row's colons said about each column, one entry per
        /// column of the heading row. `None` where the column declared nothing.
        ///
        /// Read off the separator by `bt_detect::table::delimiter_row`, which is
        /// the same function the terminal's own table detector reads it with —
        /// one parser for `:--:`, because two would eventually disagree about
        /// which way a column is set in a file that is open in a pane while the
        /// same bytes scroll past in the pane beside it.
        alignments: Vec<bt_detect::table::ColumnAlignment>,
    },
    /// Consecutive `>` rows, one entry per line, gathered as one block so the
    /// accent bar down their left is one bar rather than several.
    Quote(Vec<Vec<Span>>),
    /// `---` or `***` alone on a line.
    Rule,
    Paragraph(Vec<Span>),
    /// `$$…$$`, `\[…\]` or a bare `\begin{align}…\end{align}` — one display
    /// formula, the LaTeX **without** its delimiters and with its own line
    /// breaks intact.
    ///
    /// An environment keeps its `\begin` and its `\end`, because those are not
    /// delimiters around a formula — they are the formula, and the alignment
    /// they set up is the whole reason the author reached for one.
    ///
    /// A block of its own rather than a paragraph carrying a wide run, because
    /// display mathematics is a block in every dialect that has it: it takes the
    /// page's own margins, it is set on its own baseline rather than the
    /// paragraph's, and — the part the type has to carry — it is measured from a
    /// picture whose height nothing else on the page can predict.
    ///
    /// The delimiters are dropped here and kept on [`SpanStyle::Math`] for the
    /// asymmetric reason that a display block draws its source over several
    /// lines while it waits for its picture, and reprinting `$$` around those
    /// lines is the renderer restating what it already knows; an inline run
    /// draws inside a sentence, where the dollars are the only thing that tells
    /// a reader the words beside them are not prose.
    Math {
        source: String,
    },
    /// One picture, standing on the page as a block of its own.
    ///
    /// **Every picture in this window's markdown is a block** (2026-08-28), and
    /// the reason is [`push_prose`]: a paragraph is cut at each of its pictures,
    /// so `text ![a](b) text` becomes prose, picture, prose rather than a
    /// sentence with a hole in it. A picture set *inside* a line — sized to the
    /// text around it and sitting on its baseline, which is what
    /// [`bt_render::PreviewRun::inline_box_px`] already does for a formula — is
    /// written down as owed rather than pretended at; see `docs/DESIGN.md`
    /// §7.1.3k.
    Image(MarkdownImage),
}

/// **One picture a markdown document asks for**, in the shape the page draws it
/// from: what it says, where its pixels are, and which of several files answers
/// for the theme in force.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MarkdownImage {
    /// The alt text, exactly as the document wrote it — what stands here when
    /// the pixels do not, and what a picture *says*.
    pub alt: String,
    /// `<picture>`'s `<source>` rows, in the order the document lists them.
    ///
    /// Empty for `![alt](src)` and for a bare `<img>`, and that emptiness is the
    /// whole of the difference between them: a document that named one file
    /// named one file, and asking it about the theme would be this window
    /// inventing a second name.
    pub sources: Vec<ImageCandidate>,
    /// The `<img>`'s own `src` — `<picture>`'s last word, and everything else's
    /// only one. **Unresolved**, for [`Span::target`]'s reason: a relative
    /// source means nothing without the document it was written in, and the
    /// parser does not know which file it is reading.
    pub src: String,
    /// `width="100%"` and nothing subtler (user ruling 2026-08-28).
    ///
    /// **A picture is drawn to the column and never past its own pixels**; this
    /// is the one attribute that lifts the second half of that, because
    /// `width="100%"` is the document saying "as wide as there is room for". A
    /// percentage layout engine is not what a markdown preview is, so every
    /// other spelling of a width is read and dropped.
    pub fill: bool,
}

impl MarkdownImage {
    /// One picture written the plain way.
    #[must_use]
    pub fn named(alt: &str, src: &str) -> Self {
        Self {
            alt: alt.to_owned(),
            sources: Vec::new(),
            src: src.to_owned(),
            fill: false,
        }
    }

    /// **The file this picture is, under the theme in force** (user ruling
    /// 2026-08-28).
    ///
    /// The first candidate whose `media` names this theme, then the first that
    /// names no theme at all, then the `<img>`'s own `src`. That is `<picture>`'s
    /// own rule with the parts this window cannot honour left out: a browser
    /// walks `<source>` rows asking each one every question it knows (`type`,
    /// `media`, viewport widths), and the one question a preview pane can answer
    /// honestly is which scheme it is painting in.
    #[must_use]
    pub fn source_for(&self, theme: bt_render::Theme) -> &str {
        self.sources
            .iter()
            .find(|candidate| candidate.scheme == Some(theme))
            .or_else(|| {
                self.sources
                    .iter()
                    .find(|candidate| candidate.scheme.is_none())
            })
            .map_or(self.src.as_str(), |candidate| candidate.src.as_str())
    }
}

/// One `<source>` row of a `<picture>`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageCandidate {
    /// The scheme `media="(prefers-color-scheme: …)"` names, or `None` for a row
    /// that names no scheme — which is a row that answers for both.
    pub scheme: Option<bt_render::Theme>,
    /// The row's `srcset`, **as one address**.
    ///
    /// A `srcset` may carry a comma-separated list with density descriptors, and
    /// this window takes the first entry's URL: the rest of the list answers a
    /// question about device pixel ratios that a picture drawn to a column and
    /// resampled to the pixels it lands on has already answered another way.
    pub src: String,
}

/// Split one line into its inline runs.
///
/// **Two claiming scans, one walk, one ruling — and the order is itself a
/// ruling.** Backticks are claimed first, which is the mock-up's order
/// (4915-4917) and the one that makes `` `**not bold**` `` come out as literal
/// code rather than as a bold run inside a code span; then mathematics inside
/// what is left, so that a formula's own `*`, `_` and `[` are the formula's and
/// not this renderer's. Those two produce [`ClaimedRun`]s — *byte ranges of the
/// line*, not slices of it — and [`scan_line`] then walks the line once, from
/// the first byte to the last, stepping over every claimed range and reading
/// brackets and delimiter runs in between. [`resolve_emphasis`] rules on the
/// delimiters at the end.
///
/// **Every one of those passes reads the line and not a piece of it, and that is
/// the whole of two reported bugs.** Emphasis was the first (2026-08-28):
/// `**a `b` c**` has its opener before a code span and its closer after it, and
/// a pass that read one leftover slice at a time saw no pair in either and
/// printed both pairs of asterisks. Links were the second, and identical
/// (2026-09-07): ``[`folio-0.2.2-windows-x64.zip`](https://…)`` has its `[`
/// before a code span and its `](…)` after it, so a link scan working on
/// leftovers saw a bracket with no partner and a partner with no bracket, and
/// printed the markup. Neither emphasis nor a link is a property of a slice.
/// Both are properties of the line, and the specification agrees: brackets are
/// matched during one left-to-right scan against a stack of openers (§6.3) and
/// the delimiter stack is processed after it (§6.2).
///
/// **A claim standing in front of a bracket is CommonMark's own precedence.**
/// "Code spans, autolinks and raw HTML tags bind more tightly than the brackets"
/// (§6.3), so `` [foo`](/uri)` `` is a literal `[foo` beside a code span and not
/// a link — which is exactly what a walk that steps *over* a claimed range does,
/// without a rule of its own.
///
/// **The backtick claim standing in front of the mathematics claim is the whole
/// of "a dollar inside code is a dollar"** — and of "a `\begin{align}` inside
/// code is a `\begin{align}`". There is no second rule and no list of things
/// that look like shell or like LaTeX: a code span has already been claimed by
/// the time the formulas are read, and a fenced block never reaches this
/// function at all.
///
/// **One door for every block that has text in it.** A table cell, a list item,
/// a quote line and a paragraph all come through here, which is the whole of why
/// `` `code` `` inside a table cell works without a line of its own: there is no
/// second inline parser to teach.
pub fn parse_inline(line: &str) -> Vec<Span> {
    parse_inline_marked(line, &mut Vec::new()).0
}

/// The same runs, **where in `line` each byte of each of them was copied from**,
/// and where each picture was spelled.
///
/// One [`TextOrigin`] per [`Span`], in the same order and beside it rather than
/// inside it: `Span` derives `Eq` and is compared by value across this file's
/// test module, so a map inside it would make every one of those comparisons a
/// comparison of provenance (research §9.2 and open question 4, which ruled the
/// same way for the block's range — ticket T6 follows T1's shape).
///
/// One entry in `images` per [`SpanStyle::Image`] run the walk returns, in the
/// order they stand, covering the whole of the `![alt](src)` that made it — the
/// `!` included and the closing parenthesis included — and beside it the two
/// positions the picture's own piece is spelled back out of. It is what
/// [`push_prose`] needs and only [`push_prose`] asks for: a paragraph is cut at
/// its pictures, and a cut that could not say where it fell would leave the two
/// halves and the picture without a source between them.
fn parse_inline_marked(line: &str, images: &mut Vec<ImageMark>) -> (Vec<Span>, Vec<TextOrigin>) {
    let mut pieces = scan_line_marked(line, images);
    resolve_emphasis(&mut pieces);
    settle(pieces)
}

/// **Where one picture was spelled**, so that the block the cut makes of it can
/// be spelled back out of the file.
///
/// [`crate::preview_select`] copies a picture as `![alt](src)`, which is *not*
/// the file's own bytes when the author wrote a title or wrapped the address in
/// `<…>`: what the piece draws is the alt text, the destination, and the four
/// pieces of punctuation around them. Each of those is a copy of a run of the
/// file, and these are the runs.
struct ImageMark {
    /// The whole spelling: the `!` and the closing parenthesis included.
    spelling: Range<usize>,
    /// Where each byte of the alt text was copied from.
    alt: TextOrigin,
    /// The `]` that ended the label — the `](` the piece draws stands here.
    destination_open: usize,
    /// The address, with a title and any `<…>` already off it, exactly as the
    /// piece draws it.
    destination: Range<usize>,
}

/// One thing the scanning passes found, before the emphasis pass has ruled.
///
/// **The reason this type exists is the last pass's reach.** The scanners each
/// hand their leftovers to the next and each works on *a slice* — which was fine
/// while emphasis was a `find("**")` inside one slice, and was exactly the bug
/// reported on 2026-08-28: a bold run whose text contained a code span had its
/// opener in one slice and its closer in another, so neither slice saw a pair
/// and both printed their asterisks. Emphasis is not a property of a slice. It
/// is a property of the line, so the scanners now deposit *pieces* into one list
/// for the whole line and the emphasis pass runs over that list once, at the
/// end — exactly where CommonMark puts it (§6.2: the delimiter stack is built
/// during the scan and processed after it).
#[derive(Debug)]
enum Piece {
    /// Text nothing has claimed. `bold` and `italic` are the emphasis pass's
    /// answer: a single-delimiter pair over this text sets `italic`, a
    /// double-delimiter pair sets `bold`, and a run enclosed by both (`***a***`)
    /// carries both — CommonMark's emphasis and strong emphasis, which nest.
    Text {
        text: String,
        /// Where each byte of `text` was copied from, in the line's own bytes.
        origin: TextOrigin,
        bold: bool,
        italic: bool,
    },
    /// A run already claimed by the code, mathematics or link pass, and where
    /// its text came from.
    ///
    /// It stands *inside* an emphasis span perfectly well — it simply cannot
    /// show it. [`Span`] is a flat model with one style per run, so a code span
    /// inside a bold phrase is set in the code face and not in a bold code face.
    /// That is a floor of the model rather than of this pass, and it is the same
    /// floor [`close_bracket`] already stands on.
    Claimed(Span, TextOrigin),
    /// A run of `*` or `_`, and what the flanking rule said about it.
    Delimiter(Delimiter),
    /// A `[` or a `![` that has not been shown to open anything.
    ///
    /// **Its own piece rather than a `[` inside a text piece, and the reason is
    /// the cut.** A bracket that turns out to open a link is markup and does not
    /// survive into the page, and everything the walk deposited after it is that
    /// link's label — so [`close_bracket`] splits the piece list at this piece
    /// and drops it. A `[` merged into the text around it could not be split off
    /// again without cutting a string, which is what the byte arithmetic this
    /// type replaces used to do.
    ///
    /// One that never closes anything is what the author typed, and it is set as
    /// the text it is — inside whatever emphasis encloses it, which is why it
    /// carries the same two flags a text piece does.
    Bracket {
        image: bool,
        /// Where its markup begins in the line — the `!` of a `![` — so that a
        /// bracket that closed nothing is drawn as a copy of the author's own
        /// character rather than as a character this pass spelled.
        at: usize,
        bold: bool,
        italic: bool,
    },
}

/// A run of `*` or `_`, weighed by CommonMark's flanking rule.
///
/// **A "delimiter run" is the whole run, not one character** (§6.2): `***` is one
/// run of three and the rules below are asked of it once. Splitting it into
/// three would lose the length, and the length is what the multiple-of-3 rule
/// and the two-versus-one choice are both written in terms of.
#[derive(Debug)]
struct Delimiter {
    /// `b'*'` or `b'_'`.
    marker: u8,
    /// Where the run begins in the line.
    ///
    /// **With [`Self::head_spent`] it is what makes an unspent asterisk a copy
    /// of the author's own byte.** A pair spends the delimiters *nearest the
    /// text* — the tail of an opener, the head of a closer — so what is left of
    /// `***a**` is the run's first character and what is left of `**a***` is its
    /// last, and the two are different bytes of the file.
    start: usize,
    /// How many characters a pair has spent off the head of the run, which is
    /// what a *closer* spends.
    head_spent: usize,
    /// How many a pair has spent off its tail, which is what an *opener* spends.
    tail_spent: usize,
    /// How many of the run's characters are still unspent. Whatever is left when
    /// the pass finishes is literal text — `***a**` is one asterisk and a bold
    /// run, because the pair uses the two nearest the text and the third was
    /// never markup.
    unspent: usize,
    /// The run's length **as written**, which never changes.
    ///
    /// The multiple-of-3 rule (§6.2, rules 9 and 10) is stated over the lengths
    /// of the *runs* the two delimiters came from and not over how much of them
    /// is left, so this is a second field rather than the same one read twice.
    length: usize,
    can_open: bool,
    can_close: bool,
    /// Struck off the stack: matched, or shown to be unmatchable, or closed over
    /// by a pair either side of it. It may still have characters left, and those
    /// characters are text.
    struck: bool,
    /// Whatever is left of it stands inside an emphasis span — `bold` if a
    /// double-delimiter pair enclosed it, `italic` if a single one, both if it
    /// sits inside both.
    bold: bool,
    italic: bool,
}

impl Delimiter {
    /// Weigh one run against the characters either side of it.
    ///
    /// **CommonMark §6.2, and the reason it is worth quoting is the Chinese full
    /// stop.** A run is *left-flanking* when it is not followed by whitespace and
    /// either is not followed by punctuation or else is preceded by whitespace or
    /// punctuation; *right-flanking* is the mirror of that. Both rules are
    /// written over **Unicode** punctuation — the P and S general categories —
    /// and a renderer that reads only ASCII punctuation gets a line of Chinese
    /// prose wrong in both directions, because 。 、 and （） are punctuation to
    /// the specification and letters to `u8::is_ascii_punctuation`.
    ///
    /// `None` for either neighbour is the start or the end of the line, which the
    /// specification counts as whitespace.
    fn weigh(
        marker: u8,
        start: usize,
        length: usize,
        before: Option<char>,
        after: Option<char>,
    ) -> Self {
        let before_space = before.is_none_or(char::is_whitespace);
        let after_space = after.is_none_or(char::is_whitespace);
        let before_mark = before.is_some_and(is_unicode_punctuation);
        let after_mark = after.is_some_and(is_unicode_punctuation);
        let left = !after_space && (!after_mark || before_space || before_mark);
        let right = !before_space && (!before_mark || after_space || after_mark);
        // `_` is the one that may not open or close inside a word, which is the
        // whole of why `snake_case_name` is a name rather than a name with
        // emphasis in the middle of it. `*` has no such rule: `a*b*c` is
        // emphasis, and an author who wants a literal one writes `\*`.
        let (can_open, can_close) = if marker == b'_' {
            (
                left && (!right || before_mark),
                right && (!left || after_mark),
            )
        } else {
            (left, right)
        };
        Self {
            marker,
            start,
            head_spent: 0,
            tail_spent: 0,
            unspent: length,
            length,
            can_open,
            can_close,
            struck: false,
            bold: false,
            italic: false,
        }
    }
}

/// A character CommonMark calls punctuation: the Unicode P or S categories.
fn is_unicode_punctuation(character: char) -> bool {
    matches!(
        character.general_category_group(),
        GeneralCategoryGroup::Punctuation | GeneralCategoryGroup::Symbol
    )
}

/// One run the code and mathematics scans claimed, **as a range of the line**.
///
/// A range and not a slice, because the walk that reads brackets and delimiters
/// reads the line itself and has to know where to step over. Claiming by range
/// is also what makes CommonMark's precedence (§6.3, "code spans … bind more
/// tightly than the brackets") fall out of the walk instead of being a rule
/// somebody has to remember: a `[` or a `]` inside a claimed range is never
/// looked at, because the walk is never there.
struct ClaimedRun {
    /// Where the run begins in the line, its delimiters included.
    start: usize,
    /// One past its last byte.
    end: usize,
    /// The run itself, already built.
    span: Span,
    /// Where each byte of [`Self::span`]'s text was copied from. A code span's
    /// text is the bytes between its backticks; a formula's is the whole claim,
    /// delimiters and all, because [`SpanStyle::Math`] keeps them.
    origin: TextOrigin,
}

/// **Every code span and every formula in one line, in the order they stand.**
///
/// Backticks first over the whole line and mathematics only in the gaps between
/// them — the order [`parse_inline`] documents, kept exactly, because it is the
/// whole of "a dollar inside code is a dollar".
fn claimed_runs(line: &str) -> Vec<ClaimedRun> {
    let mut runs = Vec::new();
    let mut at = 0usize;
    while let Some(open) = line[at..].find('`').map(|found| at + found) {
        // A backtick with no partner is a backtick, not the start of anything.
        let Some(close) = line[open + 1..].find('`').map(|found| open + 1 + found) else {
            break;
        };
        push_math_claims(line, at, open, &mut runs);
        runs.push(ClaimedRun {
            start: open,
            end: close + 1,
            span: Span::code(&line[open + 1..close]),
            origin: TextOrigin::slice(open + 1, close - open - 1),
        });
        at = close + 1;
    }
    push_math_claims(line, at, line.len(), &mut runs);
    runs
}

/// An unclosed `[` or `![`, and where its label began.
struct OpenBracket {
    /// The [`Piece::Bracket`] that stands for it, which is where its label's
    /// pieces start and where the list is cut when it closes.
    piece: usize,
    /// Where its markup begins in the line — the `!` of a `![`, so that the
    /// arithmetic that ends the label in front of it has one answer.
    at: usize,
    image: bool,
    /// **Links may not contain links** (CommonMark §6.3): making one puts out
    /// every `[` still standing in front of it. An `![` is not put out, because
    /// a picture inside a link is a badge and is the ordinary case in a README.
    active: bool,
}

/// Walk the line once and deposit every piece of it.
///
/// **One walk, one stack of brackets, and everything else is arithmetic.** At
/// each byte the walk is either at the head of a [`ClaimedRun`] — which it
/// deposits whole and steps over — or at a `[`, or at a `]`, or in prose. Prose
/// is flushed to [`push_delimiter_runs`] the moment a bracket or a claim
/// interrupts it, which is what keeps a delimiter run's neighbours the line's
/// own and not a chunk boundary's.
fn scan_line_marked(line: &str, images: &mut Vec<ImageMark>) -> Vec<Piece> {
    let claims = claimed_runs(line);
    let bytes = line.as_bytes();
    let mut pieces = Vec::new();
    let mut brackets: Vec<OpenBracket> = Vec::new();
    let mut claim = 0usize;
    // Where the prose the walk has not deposited yet begins.
    let mut plain = 0usize;
    let mut at = 0usize;
    while at < bytes.len() {
        // A claim inside a link's destination is a claim the walk never reaches,
        // because the destination was read off the line in one piece.
        while claims.get(claim).is_some_and(|run| run.start < at) {
            claim += 1;
        }
        if let Some(run) = claims.get(claim).filter(|run| run.start == at) {
            push_delimiter_runs(&line[plain..at], plain, line, &mut pieces);
            pieces.push(Piece::Claimed(run.span.clone(), run.origin.clone()));
            at = run.end;
            plain = at;
            claim += 1;
            continue;
        }
        match bytes[at] {
            b'[' => {
                // **A `!` immediately in front of the bracket makes this a
                // picture** — unless the author escaped it, in which case the
                // `!` is a `!` and what follows it is an ordinary link. The
                // parity is `bt_detect::delimiter_is_escaped`'s, which is the
                // same count the dollar and the asterisk are read with: one
                // definition, four callers. The `!` also has to be prose the
                // walk is still holding: one at the tail of a code span is that
                // span's own text and not this bracket's markup.
                let image = at > plain
                    && bytes[at - 1] == b'!'
                    && !bt_detect::delimiter_is_escaped(line, at - 1);
                let markup = if image { at - 1 } else { at };
                push_delimiter_runs(&line[plain..markup], plain, line, &mut pieces);
                brackets.push(OpenBracket {
                    piece: pieces.len(),
                    at: markup,
                    image,
                    active: true,
                });
                pieces.push(Piece::Bracket {
                    image,
                    at: markup,
                    bold: false,
                    italic: false,
                });
                at += 1;
                plain = at;
            }
            b']' => match close_bracket(line, at, plain, &mut brackets, &mut pieces, images) {
                Some(resume) => {
                    at = resume;
                    plain = resume;
                }
                // The bracket closed nothing, so the `]` is the author's own
                // and stays in the prose the walk is holding.
                None => at += 1,
            },
            // Stepping by bytes reads the same positions stepping by characters
            // would: `[`, `]` and `!` are ASCII, and a UTF-8 continuation byte
            // is never one of them.
            _ => at += 1,
        }
    }
    push_delimiter_runs(&line[plain..], plain, line, &mut pieces);
    pieces
}

/// Rule on the `]` at `at` — **CommonMark's "look for link or image"** (§6.3).
///
/// `Some(resume)` when it made one, and the byte the walk goes on from; `None`
/// when the bracket was punctuation after all, which leaves the `]` in the prose
/// where the walk found it. Either way the opener is off the stack: a `]` that
/// failed to close it has spent it, and a later `]` may not try the same one
/// again.
///
/// **The label is inline content and is parsed as such** — the whole of the
/// 2026-09-07 report. Everything the walk deposited since the opener *is* the
/// label, already read as code spans, formulas, brackets and delimiter runs, so
/// what is left is what the specification does next: process the label's own
/// emphasis, settle it into runs, and hang the target on every one of them. A
/// code span in a label stays a code span and gains a target; a bold word stays
/// bold and gains a target; plain words become [`SpanStyle::Link`]. There is no
/// arm for "a label that is one code span" because there is no such case — a
/// label is inline content, and one code span is what inline content sometimes
/// happens to be.
///
/// **A picture is the one place the label collapses**, because
/// [`SpanStyle::Image`] carries alt *text*: `![the `zip`](i.png)` says "the zip",
/// which is CommonMark's own answer (the alt of an image is the plain text of
/// its label). Emphasis is resolved before that text is taken, so a `**` that
/// found its partner is markup and one that did not is a pair of asterisks the
/// reader sees.
fn close_bracket(
    line: &str,
    at: usize,
    plain: usize,
    brackets: &mut Vec<OpenBracket>,
    pieces: &mut Vec<Piece>,
    images: &mut Vec<ImageMark>,
) -> Option<usize> {
    let opener = brackets.pop()?;
    if !opener.active {
        return None;
    }
    // The target has to follow the label immediately, which is what keeps
    // `[a] (b)` and a bare `[TODO]` out of this branch.
    let target = line[at + 1..].strip_prefix('(')?;
    let close = at + 2 + target.find(')')?;
    // `[]()` is punctuation, not an empty link. **`![](src)` is not**: an empty
    // alt is what a document writes when the picture says nothing a reader needs
    // told — a rule, a spacer, a badge whose meaning is its own face — and it is
    // a picture like any other.
    if at == opener.at + 1 && !opener.image {
        return None;
    }
    let inside = &line[at + 2..close];
    let destination = link_destination(inside);
    let target = destination.to_owned();
    // Where the address the piece draws stands in the line — not `inside`,
    // which may carry a title the page never shows and brackets it takes off.
    let destination = subslice_start(inside, destination).map_or(at + 2..close, |start| {
        at + 2 + start..at + 2 + start + destination.len()
    });
    push_delimiter_runs(&line[plain..at], plain, line, pieces);
    let mut label = pieces.split_off(opener.piece);
    // The bracket's own markup does not survive into the page.
    label.remove(0);
    resolve_emphasis(&mut label);
    let (spans, origins) = settle(label);
    if opener.image {
        let alt: String = spans.iter().map(|span| span.text.as_str()).collect();
        let mut alt_origin = TextOrigin::new();
        for origin in &origins {
            alt_origin.append(origin);
        }
        // A picture inside a picture's label became alt *text* on the line
        // above, so it is no longer a run of its own and the marks it left
        // behind are marks for a run that will not arrive. They go with it —
        // the walk deposits in source order, so everything at or past this
        // opener came out of this label.
        while images
            .last()
            .is_some_and(|marks| marks.spelling.start >= opener.at)
        {
            images.pop();
        }
        images.push(ImageMark {
            spelling: opener.at..close + 1,
            alt: alt_origin.clone(),
            destination_open: at,
            destination,
        });
        pieces.push(Piece::Claimed(Span::image(&alt, &target), alt_origin));
    } else {
        for bracket in brackets.iter_mut().filter(|bracket| !bracket.image) {
            bracket.active = false;
        }
        pieces.extend(
            spans
                .into_iter()
                .zip(origins)
                .map(|(span, origin)| Piece::Claimed(span.linked(&target), origin)),
        );
    }
    Some(close + 1)
}

/// The second claim: mathematics inside one gap the backtick scan left plain.
///
/// **The rule for the dollar is Pandoc's `tex_math_dollars`**, which is the written-down
/// standard for mathematics in a markdown document and is therefore something
/// this file can cite rather than something it had to guess:
///
/// * `\$` is a literal dollar and never a delimiter
///   ([`bt_detect::delimiter_is_escaped`], the same parity the terminal's
///   detector reads it with);
/// * an opener may **not** be followed by whitespace;
/// * a closer may **not** be preceded by whitespace, and may **not** be followed
///   by an ASCII digit;
/// * `$$` is display mathematics and is never read here as two inline
///   delimiters;
/// * and the span between them must not be empty.
///
/// The digit rule is the one that earns its keep on ordinary prose: `$5 and $10`
/// has a closer that passes every other test and is followed by a `1`, which is
/// what a second price looks like and what the end of a formula never does.
///
/// **Deliberately not the terminal's rule** ([`bt_detect::detect_inline_math`]),
/// and the difference is a difference of authority rather than of taste. There a
/// lone `$` is an accident of somebody else's output and the gates ask whether
/// the bytes *read* as mathematics — site, completeness, prose. Here the `$` is
/// markup the author of this file typed, in a file this window was asked to
/// render as markdown, and asking whether the author meant it would be this
/// renderer overruling the document about its own contents. What the two do
/// share is the escape and the refusal to read `$$` as two delimiters, and those
/// are shared by calling the same function rather than by writing it twice.
fn push_math_claims(line: &str, from: usize, to: usize, runs: &mut Vec<ClaimedRun>) {
    let mut at = from;
    while let Some((open, end)) = next_inline_math(&line[at..to]) {
        runs.push(ClaimedRun {
            start: at + open,
            end: at + end,
            span: Span::math(&line[at + open..at + end]),
            origin: TextOrigin::slice(at + open, end - open),
        });
        at += end;
    }
}

/// The byte range of the next inline formula, **delimiters included**.
///
/// **Three flavours, one scanner, and the earliest one wins.** `$…$` is
/// Pandoc's; `\(…\)` is the pair GitHub has read as mathematics since 2022 and
/// LaTeX has read as mathematics since there was LaTeX; a `\begin{pmatrix}`
/// standing inside a sentence is a formula that never had delimiters at all.
/// Which of the three an author reached for is not an order of precedence, so
/// the run that starts first is the run that is claimed first — `\(x\) and $y$`
/// is two formulas in the order they were written, not one flavour's sweep
/// followed by another's.
fn next_inline_math(text: &str) -> Option<(usize, usize)> {
    [
        next_dollar_math(text),
        next_delimited_math(text, INLINE_MATH_OPEN, INLINE_MATH_CLOSE),
        next_environment_math(text),
    ]
    .into_iter()
    .flatten()
    .min_by_key(|(open, _)| *open)
}

/// `\(…\)`, the pair whose only rule is that neither delimiter was escaped.
///
/// **None of the dollar's four rules apply here, and none of them need to.**
/// They exist because a `$` is also a price, a shell variable and a `$$` — a
/// `\(` is none of those, so asking whether the author meant it would be the
/// renderer overruling the document about its own contents with nothing to gain.
/// The first opener pairs with the first closer after it, which is TeX's own
/// rule; an opener with nothing after it to close it is two characters of text
/// and the scan stops there rather than reading on to the end of the line.
fn next_delimited_math(text: &str, open: &str, close: &str) -> Option<(usize, usize)> {
    let start = find_unescaped(text, open, 0)?;
    let end = find_unescaped(text, close, start + open.len())?;
    Some((start, end + close.len()))
}

/// A mathematics environment written inside a sentence.
///
/// An environment whose partner is missing is passed over rather than returned
/// empty-handed, because the next one along may well have its own — and an
/// environment that is not mathematics is passed over for the same reason it is
/// never a block: `\begin{itemize}` is a list.
fn next_environment_math(text: &str) -> Option<(usize, usize)> {
    let mut at = 0usize;
    while let Some(start) = find_unescaped(text, ENVIRONMENT_OPEN, at) {
        if let Some(end) = math_environment_name(&text[start..])
            .and_then(|name| environment_close(text, name, start))
        {
            return Some((start, end));
        }
        at = start + ENVIRONMENT_OPEN.len();
    }
    None
}

/// The byte offsets of the next `$…$`'s two delimiters, if there is one.
fn next_dollar_math(text: &str) -> Option<(usize, usize)> {
    let dollars: Vec<usize> = text
        .char_indices()
        .filter_map(|(byte, character)| (character == '$').then_some(byte))
        .collect();
    let mut index = 0usize;
    while index < dollars.len() {
        let open = dollars[index];
        if !opens_inline_math(text, open) {
            index += 1;
            continue;
        }
        // Nothing here rules out an empty span, and nothing needs to: a closer
        // one byte after the opener is the second half of a `$$`, and a `$$`
        // was already refused as an opener.
        if let Some(close) = dollars[index + 1..]
            .iter()
            .copied()
            .find(|close| closes_inline_math(text, *close))
        {
            return Some((open, close + 1));
        }
        index += 1;
    }
    None
}

/// A `$` opens a formula when it is not escaped, is not half of a `$$`, and has
/// something other than a space after it.
fn opens_inline_math(text: &str, byte: usize) -> bool {
    if bt_detect::delimiter_is_escaped(text, byte) || is_paired_dollar(text, byte) {
        return false;
    }
    text[byte + 1..]
        .chars()
        .next()
        .is_some_and(|character| !character.is_whitespace())
}

/// A `$` closes a formula when it is not escaped, is not half of a `$$`, has
/// something other than a space in front of it, and is not the sigil of the
/// price that follows it.
fn closes_inline_math(text: &str, byte: usize) -> bool {
    if bt_detect::delimiter_is_escaped(text, byte) || is_paired_dollar(text, byte) {
        return false;
    }
    let before = text[..byte].chars().next_back();
    let after = text[byte + 1..].chars().next();
    before.is_some_and(|character| !character.is_whitespace())
        && !after.is_some_and(|character| character.is_ascii_digit())
}

/// Is this `$` one half of a `$$`, on either side?
fn is_paired_dollar(text: &str, byte: usize) -> bool {
    text.as_bytes().get(byte + 1) == Some(&b'$')
        || byte
            .checked_sub(1)
            .is_some_and(|before| text.as_bytes().get(before) == Some(&b'$'))
}

/// `\(` — inline mathematics.
const INLINE_MATH_OPEN: &str = "\\(";
/// `\)` — the end of it.
const INLINE_MATH_CLOSE: &str = "\\)";
/// `\[` — display mathematics.
const DISPLAY_MATH_OPEN: &str = "\\[";
/// `\]` — the end of it.
const DISPLAY_MATH_CLOSE: &str = "\\]";
/// `\begin{` — the head of every environment, mathematical or not.
const ENVIRONMENT_OPEN: &str = "\\begin{";

/// The environments amsmath sets as mathematics, **without their stars**.
///
/// A written-down list rather than "every `\begin{…}`", because `\begin{itemize}`
/// is a list and `\begin{verbatim}` is a code sample: handing either to a formula
/// engine does not get a refusal, it gets a picture of something that was never a
/// formula. Names only — whether a given one *renders* is the engine's answer,
/// and §7.1.3i′⑩ already rules what happens when that answer is no: the author's
/// own text stands, unmarked.
///
/// The half of this list that is only ever legal *inside* mathematics in real
/// LaTeX — the matrix family, `cases`, `aligned`, `array` — is here because a
/// markdown document is not real LaTeX: GitHub and MathJax both let one stand on
/// its own, the user's corpus writes `\begin{pmatrix}` at the top of a paragraph,
/// and MiTeX sets it.
const MATH_ENVIRONMENTS: [&str; 23] = [
    "align",
    "alignat",
    "aligned",
    "alignedat",
    "array",
    "Bmatrix",
    "bmatrix",
    "cases",
    "dcases",
    "eqnarray",
    "equation",
    "flalign",
    "gather",
    "gathered",
    "matrix",
    "multline",
    "pmatrix",
    "rcases",
    "smallmatrix",
    "split",
    "subarray",
    "Vmatrix",
    "vmatrix",
];

/// The first occurrence of `needle` at or after `from` whose leading backslash
/// the author did not escape.
///
/// **`\\[2pt]` is the case this exists for.** A `\\` is a line break and the `[`
/// after it is that break's own optional argument — a display delimiter is a `\[`
/// whose backslash stands alone. The parity is
/// [`bt_detect::delimiter_is_escaped`], the same count the dollar pass and the
/// terminal's own detector read: one definition, three callers.
fn find_unescaped(text: &str, needle: &str, from: usize) -> Option<usize> {
    let mut at = from;
    while let Some(found) = text[at..].find(needle) {
        let found = at + found;
        if !bt_detect::delimiter_is_escaped(text, found) {
            return Some(found);
        }
        // Every needle here begins with the backslash whose parity was just
        // rejected, so one byte on is both past it and on a character boundary.
        at = found + 1;
    }
    None
}

/// The environment `\begin{…}` opens at the head of `text`, if it is one whose
/// contents are mathematics.
fn math_environment_name(text: &str) -> Option<&str> {
    let rest = text.strip_prefix(ENVIRONMENT_OPEN)?;
    let name = &rest[..rest.find('}')?];
    let unstarred = name.strip_suffix('*').unwrap_or(name);
    MATH_ENVIRONMENTS.contains(&unstarred).then_some(name)
}

/// Where the `\end{name}` that closes the `\begin{name}` at `open` ends.
///
/// **A depth count rather than the first `\end{name}` along**, because an
/// environment may hold another of its own name and the closer of the outer one
/// is the `\end` that brings the count back to zero. Names are matched whole —
/// `\end{aligned}` is not an `\end{align}` — which is what lets an `aligned`
/// inside an `align` be the ordinary case it is in a real document.
fn environment_close(text: &str, name: &str, open: usize) -> Option<usize> {
    let begin = format!("{ENVIRONMENT_OPEN}{name}}}");
    let end = format!("\\end{{{name}}}");
    debug_assert!(text[open..].starts_with(&begin));
    // The caller's own opener is the one already counted, so the walk starts
    // past it and the count can only come back to zero by returning.
    let mut depth = 1usize;
    let mut at = open + begin.len();
    loop {
        let close = find_unescaped(text, &end, at)?;
        match find_unescaped(text, &begin, at) {
            Some(nested) if nested < close => {
                depth += 1;
                at = nested + begin.len();
            }
            _ => {
                depth -= 1;
                at = close + end.len();
                if depth == 0 {
                    return Some(at);
                }
            }
        }
    }
}

/// **The address inside a `(…)`**, with the title CommonMark allows beside it
/// taken off and an angle-bracketed address unwrapped.
///
/// `![alt](path/to.png "A caption")` and `[text](page.md 'why')` are both the
/// grammar's ordinary shape — destination, whitespace, title — and a reader that
/// kept the whole of it would hand `path/to.png "A caption"` to the disk. The
/// title itself is **parsed off and not shown**: in a browser it is a tooltip,
/// and this window puts no tooltip on a picture, so keeping it would be keeping
/// a string nothing reads.
///
/// One function for both because it is one grammar; a link with a title was
/// carrying its own quotes into [`link_action`] until this was written.
///
/// **A slice of what it was handed** and not a string of its own, so that a
/// caller wanting to know *where* the address stands can ask
/// `str::substr_range` rather than search for it — which is what the picture's
/// own piece needs, a picture being copied as `![alt](src)` whatever the file
/// spelled around it (ticket T6).
fn link_destination(inside: &str) -> &str {
    let inside = inside.trim();
    // `<…>` wraps a destination that has spaces in it; the brackets are markup.
    if let Some(angled) = inside
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
    {
        return angled;
    }
    let Some(space) = inside.find(char::is_whitespace) else {
        return inside;
    };
    let (destination, title) = inside.split_at(space);
    let title = title.trim_start();
    // Only a *title* may follow the destination, and a title is quoted. Anything
    // else is an address with a space in it — rare, but the author's own bytes,
    // and cutting it at the space would silently open a shorter path.
    let quoted = matches!(title.as_bytes().first(), Some(b'"' | b'\'' | b'('));
    if quoted { destination } else { inside }
}

/// The last scanning pass: cut what is left into text and delimiter runs.
///
/// It claims nothing and decides nothing — [`resolve_emphasis`] does that, once,
/// over every piece of the line. What happens here is the reading of each run of
/// `*` or `_` **in the context of the whole line**, because that is where its
/// neighbours are: the character before a run that begins a chunk is the last
/// character of the chunk before it, and the flanking rule wants the character
/// rather than the chunk boundary.
///
/// A marker with an odd number of backslashes in front of it is the author's
/// literal asterisk and never a delimiter — the same parity
/// [`bt_detect::delimiter_is_escaped`] rules for the dollar, asked here so that
/// the two markups agree about what an escape is.
fn push_delimiter_runs(text: &str, at: usize, line: &str, pieces: &mut Vec<Piece>) {
    let bytes = text.as_bytes();
    let mut plain = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        let marker = bytes[index];
        // A `*` or a `_` is one byte and is never a continuation byte, so
        // stepping by bytes reads the same positions stepping by characters
        // would, and reads them without decoding the prose in between.
        if (marker != b'*' && marker != b'_') || bt_detect::delimiter_is_escaped(line, at + index) {
            index += 1;
            continue;
        }
        let mut end = index;
        while end < bytes.len() && bytes[end] == marker {
            end += 1;
        }
        push_piece_text(&text[plain..index], at + plain, pieces);
        pieces.push(Piece::Delimiter(Delimiter::weigh(
            marker,
            at + index,
            end - index,
            line[..at + index].chars().next_back(),
            line[at + end..].chars().next(),
        )));
        plain = end;
        index = end;
    }
    push_piece_text(&text[plain..], at + plain, pieces);
}

/// Add text to the piece list, joined to the text piece before it if there is
/// one — `at` being where it stands in the line, which is what the join has to
/// carry along with the bytes.
fn push_piece_text(text: &str, at: usize, pieces: &mut Vec<Piece>) {
    if text.is_empty() {
        return;
    }
    match pieces.last_mut() {
        Some(Piece::Text {
            text: last, origin, ..
        }) => {
            last.push_str(text);
            origin.copied(at, text.len());
        }
        _ => pieces.push(Piece::Text {
            text: text.to_owned(),
            origin: TextOrigin::slice(at, text.len()),
            bold: false,
            italic: false,
        }),
    }
}

/// Match the delimiter runs into emphasis spans — CommonMark's *process
/// emphasis* (§6.2), run once over the whole line.
///
/// **Walk forward to each closer; walk back from it to the nearest opener.** Two
/// delimiters pair when they carry the same marker, when the later one can close
/// and the earlier one can open, and when the multiple-of-3 rule does not forbid
/// it. A pair spends two characters from each side if both have two to spend and
/// one otherwise — strong emphasis is the greedy reading, which is why `**a**` is
/// bold rather than emphasis inside emphasis. Everything between them is inside
/// the span, and every delimiter between them is struck off: a pair that closed
/// over a delimiter has made that delimiter's leftovers into text.
///
/// **`openers_bottom` is what keeps this linear.** A closer that found no opener
/// has proved that no *later* closer of the same class can find one below where
/// this one stopped, so the floor rises and the backward walk never re-treads
/// ground. The class is the marker, the closer's run length modulo three and
/// whether the closer could also have opened — the three things the matching
/// rules read, so two closers that agree on all three are interchangeable.
///
/// **Emphasis comes out in two layers, keyed on how many delimiters a pair
/// spends** (2026-08-28, reversing the single-layer ruling below it). A pair
/// that spends two characters a side is strong emphasis and its span is bold; a
/// pair that spends one is emphasis and its span is italic; `***a***` is the two
/// nested, so `a` is bold and italic at once. [`close_emphasis`] reads `spent`
/// and sets the two flags; the italic is a real face where the family has one
/// and a synthesised oblique where it does not ([`bt_render::PreviewRun::italic`]).
///
/// Until this day every level came out bold, because the window had one emphatic
/// face and no italic in the stack — `*a*`, `**a**` and `***a***` were all that
/// one weight. That was a floor of the renderer, not of this pass: the delimiter
/// stack always knew which level it had matched, and the moment an oblique
/// existed the two readings could split the way CommonMark writes them.
fn resolve_emphasis(pieces: &mut [Piece]) {
    // [marker][closer run length % 3][the closer can open too]
    let mut openers_bottom = [[[0usize; 2]; 3]; 2];
    let mut closer = 0usize;
    while closer < pieces.len() {
        let Piece::Delimiter(run) = &pieces[closer] else {
            closer += 1;
            continue;
        };
        if run.struck || !run.can_close {
            closer += 1;
            continue;
        }
        let (marker, length, can_open) = (run.marker, run.length, run.can_open);
        let class =
            &mut openers_bottom[usize::from(marker == b'_')][length % 3][usize::from(can_open)];
        let floor = *class;
        let Some(opener) = find_opener(pieces, closer, floor, marker, length, can_open) else {
            *class = closer;
            // A run that closes nothing here and can never open is spent.
            if !can_open {
                strike(&mut pieces[closer]);
            }
            closer += 1;
            continue;
        };
        close_emphasis(pieces, opener, closer);
        let Piece::Delimiter(run) = &pieces[closer] else {
            unreachable!("the closer was a delimiter one statement ago")
        };
        if run.struck {
            closer += 1;
        }
    }
}

/// The nearest delimiter below `closer` that may open the span it closes.
fn find_opener(
    pieces: &[Piece],
    closer: usize,
    floor: usize,
    marker: u8,
    length: usize,
    can_open: bool,
) -> Option<usize> {
    let mut index = closer;
    while index > floor {
        index -= 1;
        let Piece::Delimiter(run) = &pieces[index] else {
            continue;
        };
        if run.struck || run.marker != marker || !run.can_open {
            continue;
        }
        // Rules 9 and 10: when either delimiter faces both ways, a pair whose two
        // run lengths sum to a multiple of three is refused — unless both lengths
        // are themselves multiples of three. It is the rule that makes
        // `*foo**bar**baz*` one emphasis around two bold runs instead of the
        // tangle the greedy reading gives.
        let ambiguous = (run.can_close || can_open)
            && (run.length + length).is_multiple_of(3)
            && !(run.length.is_multiple_of(3) && length.is_multiple_of(3));
        if ambiguous {
            continue;
        }
        return Some(index);
    }
    None
}

/// Spend the pair, mark what it encloses, and strike what it closed over.
fn close_emphasis(pieces: &mut [Piece], opener: usize, closer: usize) {
    let (Piece::Delimiter(open), Piece::Delimiter(close)) = (&pieces[opener], &pieces[closer])
    else {
        unreachable!("both ends were delimiters when they were chosen")
    };
    let spent = if open.unspent >= 2 && close.unspent >= 2 {
        2
    } else {
        1
    };
    // Two delimiters spent is strong emphasis (bold); one is emphasis (italic).
    // The two are `|=`'d rather than set so that the two passes `***a***` makes
    // — the outer pair spending two, the inner spending one — leave `a` carrying
    // both, which is CommonMark's emphasis nested inside strong emphasis.
    let (add_bold, add_italic) = if spent == 2 {
        (true, false)
    } else {
        (false, true)
    };
    for piece in &mut pieces[opener + 1..closer] {
        match piece {
            Piece::Text { bold, italic, .. } | Piece::Bracket { bold, italic, .. } => {
                *bold |= add_bold;
                *italic |= add_italic;
            }
            Piece::Delimiter(run) => {
                run.bold |= add_bold;
                run.italic |= add_italic;
                run.struck = true;
            }
            // A code span, a formula or a link keeps its own style inside an
            // emphasis span — see [`Piece::Claimed`].
            Piece::Claimed(..) => {}
        }
    }
    for end in [opener, closer] {
        let Piece::Delimiter(run) = &mut pieces[end] else {
            unreachable!("both ends were delimiters when they were chosen")
        };
        run.unspent -= spent;
        // **The delimiters a pair spends are the ones nearest the text**
        // (CommonMark §6.2): an opener spends off its tail and a closer off its
        // head, so `***a**` leaves the run's first character and `**a***` leaves
        // its last. What is left is the author's own byte, and which byte it is
        // is what puts an unspent asterisk back on the page as a copy.
        if end == opener {
            run.tail_spent += spent;
        } else {
            run.head_spent += spent;
        }
        if run.unspent == 0 {
            run.struck = true;
        }
    }
}

/// Take a delimiter off the stack without touching what is left of its text.
fn strike(piece: &mut Piece) {
    if let Piece::Delimiter(run) = piece {
        run.struck = true;
    }
}

/// Turn the ruled pieces into the runs the renderer draws, **and beside each of
/// them where its bytes came from**.
fn settle(pieces: Vec<Piece>) -> (Vec<Span>, Vec<TextOrigin>) {
    let mut spans = Vec::new();
    let mut origins = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Text {
                text,
                origin,
                bold,
                italic,
            } => push_text(&text, &origin, bold, italic, &mut spans, &mut origins),
            Piece::Claimed(span, origin) => {
                spans.push(span);
                origins.push(origin);
            }
            // A bracket that closed nothing is the character the author typed,
            // and it is drawn out of the file where the author typed it.
            Piece::Bracket {
                image,
                at,
                bold,
                italic,
            } => {
                let text = if image { "![" } else { "[" };
                push_text(
                    text,
                    &TextOrigin::slice(at, text.len()),
                    bold,
                    italic,
                    &mut spans,
                    &mut origins,
                );
            }
            // Whatever a delimiter run did not spend is what the author typed —
            // the head of the run when a closer spent off it, the tail when an
            // opener did.
            Piece::Delimiter(run) => push_text(
                &(run.marker as char).to_string().repeat(run.unspent),
                &TextOrigin::slice(run.start + run.head_spent, run.unspent),
                run.bold,
                run.italic,
                &mut spans,
                &mut origins,
            ),
        }
    }
    (spans, origins)
}

/// Add text, **joined to the run before it if that run is set the same way**.
///
/// The passes hand each other the text they did not claim, so a line the link
/// pass looked at and left alone comes back in two or three pieces. Two
/// adjacent plain runs shape and draw identically to one, but they are not one:
/// the shaper is given a rich-text sequence and a break opportunity between two
/// runs is not the same thing as one inside a run, so `a [TODO] note` split at
/// the bracket could wrap where the text does not permit it.
fn push_text(
    text: &str,
    origin: &TextOrigin,
    bold: bool,
    italic: bool,
    spans: &mut Vec<Span>,
    origins: &mut Vec<TextOrigin>,
) {
    if text.is_empty() {
        return;
    }
    let span = match (bold, italic) {
        (true, true) => Span::bold_italic(text),
        (true, false) => Span::bold(text),
        (false, true) => Span::italic(text),
        (false, false) => Span::plain(text),
    };
    match spans.last_mut() {
        Some(last) if last.style == span.style => {
            last.text.push_str(text);
            origins
                .last_mut()
                .expect("a span was pushed with its map beside it")
                .append(origin);
        }
        _ => {
            spans.push(span);
            origins.push(origin.clone());
        }
    }
}

/// The markdown renderer: headings, lists, fences, tables, quotes, rules, links
/// and three inline styles.
///
/// **Indexed rather than streamed, and the reason is the table.** A pipe row is
/// only a table row if the row *after* it is a separator, so the scanner needs
/// one line of lookahead; every other block is decidable from its own first
/// line. Written as an index walk rather than an iterator with a peek because
/// the table then consumes its own run in one place instead of leaving a
/// half-open state machine for the next four blocks to step around.
///
/// What is not negotiable is the honesty this inherits from the prototype: it
/// renders **the argument**, because the mock-up's first rendered view was a
/// static mock that showed the same document whatever the buffer held (P103).
///
/// **A paragraph is a run of lines, not a line** (user ruling, 2026-08-13, and
/// CommonMark §4.8). The prototype emitted one block per source line, which is
/// invisible against a document written unwrapped and is the whole of the
/// reported seam against one written wrapped: `docs/DESIGN.md` folds at eighty
/// columns, so every paragraph in it arrived as five blocks with a paragraph gap
/// between each pair. Consecutive non-blank lines are gathered here, joined with
/// a single space, and handed to [`parse_inline`] **once** — which is also
/// CommonMark's order, and the reason emphasis opened on one source line and
/// closed on the next comes out as one run instead of two literal asterisk
/// pairs. Every other block still interrupts prose on its own first line, so the
/// gathering can never swallow a heading, a fence, a rule, a table, a quote or a
/// list marker.
pub fn parse_markdown(src: &str) -> Vec<MarkdownBlock> {
    parse_markdown_ranged(src).0
}

/// The same walk, and **beside each block the source it was parsed from**.
///
/// One byte range per block, in block order, into the very `src` that was
/// handed in. See [`RangedBlocks`] for what a range covers and what the bytes
/// between two of them are.
///
/// **The second entry point rather than a field on the block** (research
/// `docs/plans/markdown-edit/research-2026-09-10.md` §9.2 and open question 4).
/// [`MarkdownBlock`] derives `Eq` and is compared by value across this file's
/// test module and shared with the terminal's own table renderer
/// (`table_block::from_rows`); a range inside it would make every one of those
/// comparisons range-sensitive for the sake of the one caller that wants the
/// ranges. [`parse_markdown`] is this function with them dropped, which is why
/// nothing else in the crate had to change.
pub fn parse_markdown_ranged(src: &str) -> (Vec<MarkdownBlock>, Vec<Range<usize>>) {
    let (blocks, ranges, _) = parse_markdown_mapped(src);
    (blocks, ranges)
}

/// The same walk again, and **beside each block a map from every byte its
/// pieces draw to the byte of `src` it was copied from** (ticket T6).
///
/// One [`BlockOrigins`] per block, its pieces numbered exactly as
/// [`crate::preview_select::pieces`] numbers them — that module is the authority
/// on the numbering and this walk agrees with it, which a test holds. The map is
/// what turns a click into a file offset and a file offset into a caret;
/// [`crate::preview_provenance`] is where the two directions are written and
/// where the marks the page does not draw are ruled, one table for all of them.
///
/// **A third entry point and not a wider second one**, for the reason
/// [`parse_markdown_ranged`] is beside [`parse_markdown`]: the ranges have
/// callers that do not want the maps, and a map is a vector per block.
pub fn parse_markdown_mapped(
    src: &str,
) -> (Vec<MarkdownBlock>, Vec<Range<usize>>, Vec<BlockOrigins>) {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = RangedBlocks::new(src);
    // Both accumulators hold **source text**, not spans, because both of them
    // join across source lines and inline parsing has to see the joined text.
    // Each carries the source lines it ate beside it, because both destroy that
    // source on the way past — the paragraph trims and joins, the list strips
    // markers — and a block cannot be handed a range it can no longer name.
    let mut list: Vec<String> = Vec::new();
    // Per item, where each byte of its joined text came from: a row's own text
    // is trimmed off its marker and a lazy continuation is joined on with a
    // space, so an item is a paragraph in miniature and is mapped as one.
    let mut list_source: Vec<TextOrigin> = Vec::new();
    let mut list_lines: Option<(usize, usize)> = None;
    let mut ordered: Option<u64> = None;
    let mut paragraph: Vec<&str> = Vec::new();
    let mut paragraph_lines: Vec<usize> = Vec::new();
    let mut index = 0usize;

    while index < lines.len() {
        // The line this turn of the walk starts on. Every branch below moves
        // `index`, several of them by more than one line, so the *first* line of
        // whatever is about to be pushed has to be taken before any of them do.
        let at = index;
        let line = lines[at];

        // ── the fence, which swallows everything until it closes ────────────
        if let Some(rest) = line.strip_prefix("```") {
            flush_paragraph(&mut paragraph, &mut paragraph_lines, &mut out);
            flush_list(
                &mut list,
                &mut list_source,
                &mut ordered,
                &mut list_lines,
                &mut out,
            );
            let lang = rest.trim();
            let lang = (!lang.is_empty()).then(|| lang.to_owned());
            let mut body = Vec::new();
            index += 1;
            while index < lines.len() && !lines[index].starts_with("```") {
                body.push(lines[index]);
                index += 1;
            }
            // A fence nobody closed still draws, rather than swallowing the rest
            // of the document in silence (mock-up 4939) — which is what the
            // `index < len` bound above means when the loop runs off the end.
            index += usize::from(index < lines.len());
            let text = body.join("\n");
            let origins = fence_origins(&text, &body, &out);
            out.push_lines(MarkdownBlock::Code { lang, text }, at, index - 1, origins);
            continue;
        }

        // ── display mathematics, which swallows its lines as a fence does ───
        //
        // **After the fence and before everything else.** A `$$` inside a fence
        // is a fence's business and the branch above has already taken it; a
        // `$$` anywhere else is the author opening a formula, and nothing below
        // may see those lines as prose, as a rule or as a table.
        if let Some(rest) = line.strip_prefix("$$") {
            flush_paragraph(&mut paragraph, &mut paragraph_lines, &mut out);
            flush_list(
                &mut list,
                &mut list_source,
                &mut ordered,
                &mut list_lines,
                &mut out,
            );
            index += 1;
            let mut body: Vec<&str> = Vec::new();
            // `$$E = mc^2$$` — opened and closed on one line. Asked of the line
            // with its trailing space gone, because a delimiter followed by
            // nothing but blanks is still the last thing on the line.
            match rest.trim_end().strip_suffix("$$") {
                Some(inner) if !inner.trim().is_empty() => body.push(inner.trim()),
                _ => {
                    // Whatever the opener carried after its delimiter is the
                    // formula's first line: `$$\begin{aligned}` is one way to
                    // write what `$$` on a line of its own writes in two.
                    if !rest.trim().is_empty() {
                        body.push(rest.trim());
                    }
                    while index < lines.len() {
                        let line = lines[index];
                        index += 1;
                        let Some(head) = line.trim_end().strip_suffix("$$") else {
                            body.push(line);
                            continue;
                        };
                        if !head.trim().is_empty() {
                            body.push(head.trim());
                        }
                        break;
                    }
                    // A formula nobody closed still renders, rather than
                    // swallowing the rest of the document in silence — the same
                    // ruling the unterminated fence above is decided by.
                }
            }
            let origins = math_origins(&body, &out);
            out.push_lines(
                MarkdownBlock::Math {
                    source: body.join("\n"),
                },
                at,
                index - 1,
                origins,
            );
            continue;
        }

        // ── the same block written the other two ways ───────────────────────
        //
        // `\[…\]` and a bare `\begin{align}…\end{align}` are display
        // mathematics on the same terms `$$` is, and stand in the same place in
        // this walk: after the fence, which has already taken anything inside
        // it, and before every branch that would otherwise read these lines as
        // prose, as a rule or as a table.
        if let Some((source, after, body)) = display_math_block(&lines, index, &out) {
            flush_paragraph(&mut paragraph, &mut paragraph_lines, &mut out);
            flush_list(
                &mut list,
                &mut list_source,
                &mut ordered,
                &mut list_lines,
                &mut out,
            );
            out.push_lines(
                MarkdownBlock::Math { source },
                at,
                after - 1,
                BlockOrigins::one(math_piece_origin(&body)),
            );
            index = after;
            continue;
        }

        // ── `<img>` and `<picture>`, the two tags that are a picture ────────
        //
        // An HTML block, and it stands where the other block-swallowing arms
        // stand: after the fence, which has already claimed anything inside it,
        // and before every branch that would read these lines as prose. It
        // refuses far more often than it accepts — see [`html_image_block`] —
        // and a refusal costs nothing, because the lines then travel on to the
        // very branch that would have taken them.
        if let Some((image, after)) = html_image_block(&lines, index) {
            flush_paragraph(&mut paragraph, &mut paragraph_lines, &mut out);
            flush_list(
                &mut list,
                &mut list_source,
                &mut ordered,
                &mut list_lines,
                &mut out,
            );
            // **The one picture whose piece the file does not spell.** A
            // `<picture>` copies as `![alt](src)` — markdown the document never
            // contained — so every byte of that piece is the page's own and a
            // caret anywhere in it answers with the block's first byte.
            let mut piece = TextOrigin::new();
            piece.drawn(crate::preview_select::image_piece(&image).len());
            out.push_lines(
                MarkdownBlock::Image(image),
                at,
                after - 1,
                BlockOrigins::one(piece),
            );
            index = after;
            continue;
        }

        // ── the table, which is the one block needing lookahead ─────────────
        if is_pipe_row(line)
            && lines
                .get(index + 1)
                .is_some_and(|next| is_table_separator(next))
        {
            flush_paragraph(&mut paragraph, &mut paragraph_lines, &mut out);
            flush_list(
                &mut list,
                &mut list_source,
                &mut ordered,
                &mut list_lines,
                &mut out,
            );
            let mut cells = Vec::new();
            let mut rows = vec![split_pipe_row(line, &out, &mut cells)];
            let index_of_separator = index + 1;
            // Past the separator, then every pipe row that follows without a
            // break. A blank line ends the table exactly as it ends a paragraph.
            index += 2;
            while index < lines.len() && is_pipe_row(lines[index]) {
                rows.push(split_pipe_row(lines[index], &out, &mut cells));
                index += 1;
            }
            let alignments = table_alignments(lines[index_of_separator], rows[0].len());
            // The separator row is markup and the pipes are markup: what is left
            // is one piece per cell, row by row, which is the order
            // `preview_select` numbers them in.
            out.push_lines(
                MarkdownBlock::Table { rows, alignments },
                at,
                index - 1,
                BlockOrigins::new(cells),
            );
            continue;
        }

        index += 1;

        if let Some((heading, piece)) = parse_heading(line, &out) {
            flush_paragraph(&mut paragraph, &mut paragraph_lines, &mut out);
            flush_list(
                &mut list,
                &mut list_source,
                &mut ordered,
                &mut list_lines,
                &mut out,
            );
            out.push_lines(heading, at, at, BlockOrigins::one(piece));
            continue;
        }
        // **After the table and before the list**, which is what keeps a `---`
        // that is a table's separator out of here and a `- item` out of the
        // rule: a separator is only ever reached by the branch above (which
        // consumed it), and a rule is three or more of one character with
        // nothing else on the line, which `- item` is not.
        if is_thematic_break(line) {
            flush_paragraph(&mut paragraph, &mut paragraph_lines, &mut out);
            flush_list(
                &mut list,
                &mut list_source,
                &mut ordered,
                &mut list_lines,
                &mut out,
            );
            // A rule is a line on the page and no words at all, so it has no
            // pieces and nothing to map — `preview_select` says the same.
            out.push_lines(MarkdownBlock::Rule, at, at, BlockOrigins::none());
            continue;
        }
        if let Some((number, item)) = parse_list_row(line) {
            flush_paragraph(&mut paragraph, &mut paragraph_lines, &mut out);
            // A bulleted list and a numbered one standing next to each other are
            // two lists, not one list that changes its mind halfway down.
            if !list.is_empty() && ordered.is_some() != number.is_some() {
                flush_list(
                    &mut list,
                    &mut list_source,
                    &mut ordered,
                    &mut list_lines,
                    &mut out,
                );
            }
            if list.is_empty() {
                ordered = number;
            }
            let text = item.trim();
            list.push(text.to_owned());
            list_source.push(TextOrigin::slice(out.offset_of(text), text.len()));
            list_lines = Some(list_lines.map_or((at, at), |(first, _)| (first, at)));
            continue;
        }
        if let Some(first) = strip_quote(line) {
            flush_paragraph(&mut paragraph, &mut paragraph_lines, &mut out);
            flush_list(
                &mut list,
                &mut list_source,
                &mut ordered,
                &mut list_lines,
                &mut out,
            );
            // A quote's own lines gather exactly as prose does — a wrapped quote
            // is one quoted paragraph — and a bare `>` is the blank line that
            // separates two of them.
            let mut quoted = Vec::new();
            let mut quoted_source: Vec<TextOrigin> = Vec::new();
            let mut run: Vec<&str> = Vec::new();
            let push_run = |run: &mut Vec<&str>,
                            quoted: &mut Vec<Vec<Span>>,
                            quoted_source: &mut Vec<TextOrigin>,
                            out: &RangedBlocks| {
                if !run.is_empty() {
                    // A quoted paragraph joins its lines exactly as prose does,
                    // so it is mapped exactly as prose is: the trimmed lines are
                    // copies and the space between two of them is the page's.
                    let joined = joined_origin(run, out);
                    let (spans, origins) =
                        parse_inline_marked(&join_source_lines(run), &mut vec![]);
                    quoted.push(spans);
                    quoted_source.push(piece_origin(&origins, &joined));
                    run.clear();
                }
            };
            let mut quoted_line = Some(first);
            while let Some(text) = quoted_line {
                if text.trim().is_empty() {
                    push_run(&mut run, &mut quoted, &mut quoted_source, &out);
                } else {
                    run.push(text);
                }
                quoted_line = lines.get(index).and_then(|line| strip_quote(line));
                index += usize::from(quoted_line.is_some());
            }
            push_run(&mut run, &mut quoted, &mut quoted_source, &out);
            out.push_lines(
                MarkdownBlock::Quote(quoted),
                at,
                index - 1,
                BlockOrigins::new(quoted_source),
            );
            continue;
        }
        if line.trim().is_empty() {
            flush_paragraph(&mut paragraph, &mut paragraph_lines, &mut out);
            flush_list(
                &mut list,
                &mut list_source,
                &mut ordered,
                &mut list_lines,
                &mut out,
            );
            continue;
        }
        // **Lazy continuation** (CommonMark §5.2): a plain line under an open
        // list belongs to the item above it, not to a paragraph of its own. A
        // bullet that wraps in the source is one bullet, which is the same
        // ruling the paragraph join is, applied where the text is indented under
        // a marker instead of standing on its own.
        match list.last_mut() {
            Some(item) if paragraph.is_empty() => {
                let text = line.trim();
                item.push(' ');
                item.push_str(text);
                if let Some(source) = list_source.last_mut() {
                    // The space is the join's own; the words are the file's.
                    source.drawn(1);
                    source.copied(out.offset_of(text), text.len());
                }
                list_lines = Some(list_lines.map_or((at, at), |(first, _)| (first, at)));
            }
            _ => {
                paragraph.push(line);
                paragraph_lines.push(at);
            }
        }
    }
    flush_paragraph(&mut paragraph, &mut paragraph_lines, &mut out);
    flush_list(
        &mut list,
        &mut list_source,
        &mut ordered,
        &mut list_lines,
        &mut out,
    );
    (out.blocks, out.ranges, out.origins)
}

/// The blocks of one document, and beside each the source it was parsed from.
///
/// **What a range covers**: the span the block was parsed from — the first byte
/// of its first line to the end of its last line, *that line's own ending
/// included*, so a CRLF file's ranges cover both bytes of the break and a file
/// that ends without one has a last range that ends at the last byte. A
/// paragraph cut at a picture is the one block that stops inside a line; see
/// [`push_prose`].
///
/// **What the bytes between two ranges are**: the document's connective tissue,
/// which belongs to no block — the blank line that ends a paragraph is consumed
/// by the flush and is a member of nothing. The ranges are ordered and never
/// overlap, so a save that rewrote one block splices it back between two runs of
/// bytes it must not touch. A file ending in a break has an empty last line that
/// [`crate::preview_edit::line_starts`] counts and `str::lines` denies: the walk
/// never reaches it, so it is tissue too, and the break itself belongs to the
/// block on the line above.
struct RangedBlocks<'a> {
    src: &'a str,
    /// Where every line of `src` begins, in bytes.
    ///
    /// **Byte offsets and not a running sum of line lengths**, because
    /// `str::lines` strips the `\r` of a CRLF as well as the `\n`: on a file
    /// written by any editor on this platform, a line's length is a byte short
    /// of its span, and a walk that added them up would drift by one byte a line
    /// until the ranges named the wrong text entirely. This is the editor's own
    /// vector, from the module that already had to answer the same question for
    /// the caret.
    starts: Vec<usize>,
    blocks: Vec<MarkdownBlock>,
    ranges: Vec<Range<usize>>,
    /// Per block, per piece of it, where every byte the page draws came from —
    /// ticket T6, and [`crate::preview_provenance`] for what the answers mean.
    origins: Vec<BlockOrigins>,
}

impl<'a> RangedBlocks<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            starts: crate::preview_edit::line_starts(src),
            blocks: Vec::new(),
            ranges: Vec::new(),
            origins: Vec::new(),
        }
    }

    /// **Where a slice of this walk's own source begins**, in bytes.
    ///
    /// The parser works in slices of `src` all the way down — `str::lines`,
    /// `trim`, `strip_prefix` and `split` all hand back subslices — so the
    /// position of a payload it kept is a fact the slice itself carries and not
    /// something a second walk has to reconstruct. A string built rather than
    /// sliced (a joined paragraph, a list item) has no such answer, and those are
    /// the four places this file builds a map by hand.
    fn offset_of(&self, slice: &str) -> usize {
        subslice_start(self.src, slice).unwrap_or(self.src.len())
    }

    /// Where line `line` begins, or the end of the file for a line past its end.
    fn line_start(&self, line: usize) -> usize {
        self.starts.get(line).copied().unwrap_or(self.src.len())
    }

    /// One past line `line`'s own ending: the next line's first byte, or the end
    /// of the file when there is no next line — which is the same answer for a
    /// last line that ends in a break and for one that does not.
    fn line_end(&self, line: usize) -> usize {
        self.line_start(line + 1)
    }

    /// A block parsed from source lines `first` through `last`, inclusive.
    fn push_lines(
        &mut self,
        block: MarkdownBlock,
        first: usize,
        last: usize,
        origins: BlockOrigins,
    ) {
        self.push(block, self.line_start(first)..self.line_end(last), origins);
    }

    fn push(&mut self, block: MarkdownBlock, range: Range<usize>, origins: BlockOrigins) {
        self.blocks.push(block);
        self.ranges.push(range);
        self.origins.push(origins);
    }
}

/// **Where a paragraph's joined text came from**, so that a cut in the joined
/// text can be made in the file.
///
/// [`join_source_lines`] trims each line and joins them with a single space, so
/// the string [`parse_inline`] reads is nobody's bytes: the indent is gone, the
/// breaks are gone, and a CRLF's carriage return is gone with them. This is the
/// map back — one entry per line, and every byte of joined text that is a copy
/// of a source byte answers with the byte it is a copy of.
struct ParagraphSource {
    /// The paragraph's own span, the last line's ending included.
    span: Range<usize>,
    /// Per line: where its trimmed text begins in the joined string, how many
    /// bytes of it there are, and where those same bytes begin in the source.
    lines: Vec<(usize, usize, usize)>,
}

impl ParagraphSource {
    fn new(paragraph: &[&str], at: &[usize], out: &RangedBlocks, span: Range<usize>) -> Self {
        let mut lines = Vec::with_capacity(paragraph.len());
        let mut joined = 0usize;
        for (line, index) in paragraph.iter().zip(at) {
            let text = line.trim();
            let indent = line.len() - line.trim_start().len();
            lines.push((joined, text.len(), out.line_start(*index) + indent));
            // The single space [`join_source_lines`] puts between two lines.
            joined += text.len() + 1;
        }
        Self { span, lines }
    }

    /// **The same map, per byte and in both directions** — what the joined text
    /// is a copy of, and where the joining spaces are.
    ///
    /// [`Self::source_start`] is total by construction: it has to be, because
    /// the cut it serves must name a byte for any offset it is handed. This one
    /// is honest instead: the space between two lines is a byte the page draws
    /// and the file does not spell, and a caret standing on it is standing on
    /// the renderer's own character. See [`crate::preview_provenance`].
    fn origin(&self) -> TextOrigin {
        let mut origin = TextOrigin::new();
        for (joined, len, source) in &self.lines {
            origin.drawn(joined.saturating_sub(origin.len()));
            origin.copied(*source, *len);
        }
        origin
    }

    /// The source byte the joined byte at `offset` is a copy of.
    ///
    /// The joining spaces are copies of nothing, so one of those answers with
    /// the first byte of the line it stands in front of. That keeps the map
    /// total and keeps it from ever running backwards, which is what the
    /// partition below relies on.
    fn source_start(&self, offset: usize) -> usize {
        self.at(offset, false)
    }

    /// One past the source byte the joined byte *before* `offset` is a copy of —
    /// the exclusive end that answers [`Self::source_start`]'s inclusive start.
    fn source_end(&self, offset: usize) -> usize {
        self.at(offset, true)
    }

    fn at(&self, offset: usize, end: bool) -> usize {
        for (joined, len, source) in &self.lines {
            if offset < joined + len || (end && offset == joined + len) {
                return source + offset.saturating_sub(*joined);
            }
        }
        self.span.end
    }
}

/// The display formula the line at `start` opens with `\[` or with a bare
/// `\begin{…}`, and the line the document goes on at — or `None`, meaning what
/// looked like an opener was text and this walk should read on as if it were.
///
/// **The opener is first on its line and the closer is last on its.** That is
/// the shape `$$` already has, and it is what keeps a `\[` inside a sentence the
/// author's own bracket: markdown's `\[` is *also* CommonMark's escape for a
/// literal `[`, so a renderer that read every one of them as mathematics would
/// be wrong in prose about markdown far more often than it was right in prose
/// about physics.
///
/// **Bounded by the paragraph, which is the one place this parts company with
/// `$$`.** An unclosed `$$` swallows the rest of the document and draws it,
/// because a `$$` standing first on a line is not something prose contains by
/// accident. A `\[` is — see above — and so the partner has to turn up before
/// the blank line that ends this paragraph; if it does not, nothing here was
/// mathematics and every line of it stays prose. Nothing is lost by the bound: a
/// formula with a blank line through the middle of it is not a formula TeX would
/// set either.
fn display_math_block(
    lines: &[&str],
    start: usize,
    out: &RangedBlocks,
) -> Option<(String, usize, TextOrigin)> {
    let first = *lines.get(start)?;
    if !first.starts_with(DISPLAY_MATH_OPEN) && !first.starts_with(ENVIRONMENT_OPEN) {
        return None;
    }
    let end = lines[start..]
        .iter()
        .position(|line| line.trim().is_empty())
        .map_or(lines.len(), |at| start + at);
    let paragraph = lines[start..end].join("\n");
    // The lines were joined into a string of this function's own, so every
    // offset below is an offset into *that*; this is the map back out of it, and
    // it is composed onto the answer before it leaves.
    let mut joined = TextOrigin::new();
    for (index, line) in lines[start..end].iter().enumerate() {
        if index > 0 {
            joined.drawn("\n".len());
        }
        joined.copied(out.offset_of(line), line.len());
    }
    let (body, close_end) = if paragraph.starts_with(DISPLAY_MATH_OPEN) {
        let close = find_unescaped(&paragraph, DISPLAY_MATH_CLOSE, DISPLAY_MATH_OPEN.len())?;
        (
            &paragraph[DISPLAY_MATH_OPEN.len()..close],
            close + DISPLAY_MATH_CLOSE.len(),
        )
    } else {
        // An environment keeps both of its ends: they are the formula.
        let name = math_environment_name(&paragraph)?;
        let close_end = environment_close(&paragraph, name, 0)?;
        (&paragraph[..close_end], close_end)
    };
    let tail = &paragraph[close_end..];
    if !tail
        .split_once('\n')
        .map_or(tail, |(rest_of_line, _)| rest_of_line)
        .trim()
        .is_empty()
    {
        return None;
    }
    let at = subslice_start(&paragraph, body).unwrap_or(0);
    let (text, origin) = math_block_body(body, at);
    Some((
        text,
        start + paragraph[..close_end].matches('\n').count() + 1,
        origin.through(&joined),
    ))
}

/// **The picture the HTML block at `start` is**, and the line the document goes
/// on at — or `None`, meaning these lines are not a picture and this walk should
/// read on as if this function had not been called (user ruling 2026-08-28;
/// `docs/DESIGN.md` §7.1.3k ②).
///
/// **Two elements and no more.** `<img>` and `<picture>` are the two ways a
/// markdown document in the wild puts a picture on a page that `![alt](src)`
/// cannot express, and they are the two this window reads. Every other tag stays
/// exactly what it is today — printed as the text it is — because a markdown
/// preview that grew half an HTML renderer would be wrong in a new way on every
/// document that used the other half.
///
/// **The block is CommonMark's** (§4.6, HTML block type 6): it opens on a line
/// whose first non-blank characters are one of these tags and it closes at the
/// blank line. Which is also why the whole run has to *be* the picture: a run
/// carrying a picture and a sentence is an HTML block this window cannot draw,
/// and printing it as it stands is the honest answer for it.
fn html_image_block(lines: &[&str], start: usize) -> Option<(MarkdownImage, usize)> {
    let first = lines.get(start)?.trim_start();
    if !opens_html_tag(first, "picture") && !opens_html_tag(first, "img") {
        return None;
    }
    let end = lines[start..]
        .iter()
        .position(|line| line.trim().is_empty())
        .map_or(lines.len(), |at| start + at);
    let block = lines[start..end].join("\n");
    Some((html_image(&block)?, end))
}

/// Whether `text` begins the named tag — `<img`, `<img>`, `<img/>`, and never
/// `<image`.
fn opens_html_tag(text: &str, name: &str) -> bool {
    let Some(rest) = text
        .strip_prefix('<')
        .filter(|rest| rest.len() >= name.len() && rest[..name.len()].eq_ignore_ascii_case(name))
        .map(|rest| &rest[name.len()..])
    else {
        return false;
    };
    rest.is_empty()
        || rest.starts_with(|character: char| character.is_whitespace() || character == '>')
        || rest.starts_with('/')
}

/// One `<img>` or `<picture>` element, read into the picture it names.
fn html_image(block: &str) -> Option<MarkdownImage> {
    let tags = html_tags(block)?;
    let image = tags.iter().find(|tag| tag.name == "img")?;
    // Every `<source>` before the `<img>`, which is where `<picture>` puts them
    // and the order it reads them in.
    let sources = tags
        .iter()
        .filter(|tag| tag.name == "source")
        .filter_map(|tag| {
            let src = first_srcset_url(tag.attribute("srcset")?)?;
            Some(ImageCandidate {
                scheme: match tag.attribute("media") {
                    // A row that names no media answers for every theme.
                    None => None,
                    // A media query this window cannot evaluate makes the row
                    // unanswerable, and an unanswerable row must not be picked:
                    // dropping it leaves `<img>`'s own `src`, which is exactly
                    // what a browser falls back to when no source matches.
                    Some(media) => Some(colour_scheme_of(media)?),
                },
                src,
            })
        })
        .collect();
    Some(MarkdownImage {
        alt: image
            .attribute("alt")
            .map(collapse_html_whitespace)
            .unwrap_or_default(),
        sources,
        src: image.attribute("src")?.trim().to_owned(),
        fill: image
            .attribute("width")
            .is_some_and(|width| width.trim() == "100%"),
    })
}

/// The scheme a `media` attribute names, or `None` for a query about anything
/// else.
fn colour_scheme_of(media: &str) -> Option<bt_render::Theme> {
    let media = media.to_ascii_lowercase();
    let inside = media
        .split_once("prefers-color-scheme")?
        .1
        .trim_start()
        .strip_prefix(':')?;
    let value = inside.trim_start();
    if value.starts_with("dark") {
        Some(bt_render::Theme::Dark)
    } else if value.starts_with("light") {
        Some(bt_render::Theme::Light)
    } else {
        None
    }
}

/// The first address in a `srcset` — see [`ImageCandidate::src`].
fn first_srcset_url(srcset: &str) -> Option<String> {
    let first = srcset.split(',').next()?.trim();
    let url = first.split_whitespace().next()?;
    (!url.is_empty()).then(|| url.to_owned())
}

/// An attribute value with its line breaks spent, which is what HTML does to
/// them: the `alt` on this repository's own hero is five source lines of one
/// sentence.
fn collapse_html_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One tag out of an HTML block this window reads.
struct HtmlTag {
    /// Lower-cased, and `/picture` for a closing tag: a name is a name and the
    /// slash is part of how this scanner spells it.
    name: String,
    attributes: Vec<(String, String)>,
}

impl HtmlTag {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// **Every tag in a block that is nothing but tags**, or `None`.
///
/// The refusal is the point: text between the tags means this run is not a
/// picture on its own, and a scanner that skipped over it would swallow a
/// caption the document meant to print.
fn html_tags(block: &str) -> Option<Vec<HtmlTag>> {
    let mut tags = Vec::new();
    let mut rest = block;
    while let Some(open) = rest.find('<') {
        if !rest[..open].trim().is_empty() {
            return None;
        }
        let close = html_tag_end(&rest[open..])? + open;
        tags.push(html_tag(&rest[open + 1..close]));
        rest = &rest[close + 1..];
    }
    rest.trim().is_empty().then_some(tags)
}

/// Where the `>` that closes the tag at the head of `text` is — **counting only
/// the ones outside quotes**, because an `alt` may perfectly well contain a `>`.
fn html_tag_end(text: &str) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (at, byte) in text.bytes().enumerate() {
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None if byte == b'"' || byte == b'\'' => quote = Some(byte),
            None if byte == b'>' => return Some(at),
            None => {}
        }
    }
    None
}

/// One tag's name and attributes, out of what stood between its angle brackets.
fn html_tag(inside: &str) -> HtmlTag {
    let inside = inside.trim().strip_suffix('/').unwrap_or(inside.trim());
    let mut rest = inside.trim_start();
    let name_end = rest
        .find(|character: char| character.is_whitespace())
        .unwrap_or(rest.len());
    let name = rest[..name_end].to_ascii_lowercase();
    rest = &rest[name_end..];
    let mut attributes = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let key_end = rest
            .find(|character: char| character.is_whitespace() || character == '=')
            .unwrap_or(rest.len());
        let key = rest[..key_end].to_ascii_lowercase();
        rest = rest[key_end..].trim_start();
        let value = match rest.strip_prefix('=') {
            None => String::new(),
            Some(after) => {
                let after = after.trim_start();
                match after.as_bytes().first() {
                    Some(quote @ (b'"' | b'\'')) => {
                        let quote = *quote as char;
                        let end = after[1..].find(quote).map_or(after.len(), |at| at + 1);
                        let value = after[1..end].to_owned();
                        rest = after.get(end + 1..).unwrap_or("");
                        value
                    }
                    _ => {
                        let end = after
                            .find(|character: char| character.is_whitespace())
                            .unwrap_or(after.len());
                        let value = after[..end].to_owned();
                        rest = &after[end..];
                        value
                    }
                }
            }
        };
        if key.is_empty() {
            break;
        }
        attributes.push((key, value));
    }
    HtmlTag { name, attributes }
}

/// A display formula's source with the blank its delimiters left behind taken
/// off.
///
/// **Only the first line and the last, and only their own ends.** What is
/// between them is the author's own layout: a `&` column that lost its leading
/// spaces is a column that no longer lines up in the source a reader falls back
/// to when the engine refuses.
/// **And beside it the map back**, `at` being where `body` itself stands in
/// whatever string it was cut out of: every line that survives is a slice of
/// `body`, so its position is a fact the slice carries, and the breaks between
/// them are the join's own.
fn math_block_body(body: &str, at: usize) -> (String, TextOrigin) {
    let mut lines: Vec<&str> = body
        .lines()
        .skip_while(|line| line.trim().is_empty())
        .collect();
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    if let Some(first) = lines.first_mut() {
        *first = first.trim();
    }
    if let Some(last) = lines.last_mut() {
        *last = last.trim();
    }
    let mut origin = TextOrigin::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            origin.drawn("\n".len());
        }
        let source = subslice_start(body, line).map_or(at, |start| at + start);
        origin.copied(source, line.len());
    }
    (lines.join("\n"), origin)
}

/// **Where a slice of `whole` begins in it**, or `None` when its bytes are not
/// `whole`'s.
///
/// This is `str::substr_range`, which is still unstable (#126769) and is
/// therefore written here rather than waited for. It is the fact the whole of
/// this ticket's parse side rests on: the parser works in slices all the way
/// down — `lines`, `trim`, `strip_prefix`, `split` — and a slice cut out of the
/// document carries its own position, so provenance is something to *read* off
/// the walk rather than a second walk to write.
fn subslice_start(whole: &str, part: &str) -> Option<usize> {
    let base = whole.as_ptr().addr();
    let at = part.as_ptr().addr();
    (at >= base && at + part.len() <= base + whole.len()).then(|| at - base)
}

/// The source lines of one paragraph, as the single line CommonMark reads them
/// as.
///
/// Joined with a space and each line trimmed, which is what a soft line break
/// renders as. Trimming is what makes an indented continuation line join
/// cleanly; joining rather than concatenating is what keeps the last word of one
/// source line from running into the first word of the next.
fn join_source_lines(lines: &[&str]) -> String {
    lines
        .iter()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join(" ")
}

fn flush_paragraph(paragraph: &mut Vec<&str>, at: &mut Vec<usize>, out: &mut RangedBlocks) {
    let (Some(first), Some(last)) = (at.first().copied(), at.last().copied()) else {
        return;
    };
    let text = join_source_lines(paragraph);
    let source = ParagraphSource::new(
        paragraph,
        at,
        out,
        out.line_start(first)..out.line_end(last),
    );
    paragraph.clear();
    at.clear();
    let mut images = Vec::new();
    let (spans, origins) = parse_inline_marked(&text, &mut images);
    push_prose(spans, origins, &images, &source, out);
}

/// **One piece's map into the file**: the spans of one run, one after another,
/// each of them read through the join that made the text they were cut from.
///
/// The two halves are the whole shape of this ticket. [`parse_inline_marked`]
/// answers in the *block's* text, because that is the string it read; the join
/// answers in the file, because that is where its lines came from; and a piece
/// is what a reader points at, so a piece is where the two have to meet.
fn piece_origin(spans: &[TextOrigin], joined: &TextOrigin) -> TextOrigin {
    let mut piece = TextOrigin::new();
    for span in spans {
        piece.append(span);
    }
    piece.through(joined)
}

/// **The map of a run of source lines trimmed and joined with a single space**,
/// which is [`join_source_lines`]'s own shape: the trimmed lines are copies and
/// the spaces between them are the page's.
fn joined_origin(lines: &[&str], out: &RangedBlocks) -> TextOrigin {
    let mut origin = TextOrigin::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            origin.drawn(1);
        }
        let text = line.trim();
        origin.copied(out.offset_of(text), text.len());
    }
    origin
}

/// **A fence's pieces**: one per body line, tabs expanded as the page expands
/// them.
fn fence_origins(text: &str, body: &[&str], out: &RangedBlocks) -> BlockOrigins {
    // `text.lines()` is `body` again — that is what `join` and `lines` are to
    // each other — except that a body ending in a blank line loses it to the
    // break that joined it. Zipping walks the pieces the page will draw, which
    // is the numbering that has to agree.
    BlockOrigins::new(
        text.lines()
            .zip(body)
            .map(|(_, line)| tab_origin(line, out.offset_of(line)))
            .collect(),
    )
}

/// **One fence line's map**, [`expand_tabs`]'s walk read for provenance rather
/// than for text.
///
/// A tab is one source byte drawn as up to four spaces, and none of those spaces
/// is a copy of it: they are the fence's own indentation, spelled by the
/// renderer. The characters either side of it are copies like any other.
fn tab_origin(line: &str, at: usize) -> TextOrigin {
    let mut origin = TextOrigin::new();
    if !line.contains('\t') {
        origin.copied(at, line.len());
        return origin;
    }
    let mut column = 0usize;
    let mut byte = 0usize;
    for cluster in bt_unicode::graphemes(line) {
        if cluster == "\t" {
            let advance = PREVIEW_TEXT_TAB_WIDTH - column % PREVIEW_TEXT_TAB_WIDTH;
            origin.drawn(advance);
            column += advance;
        } else {
            origin.copied(at + byte, cluster.len());
            column += bt_unicode::cluster_width(cluster);
        }
        byte += cluster.len();
    }
    origin
}

/// **A display formula's one piece**: `$$`, the body, `$$`.
///
/// The delimiters are the page's own — the block dropped whatever the file
/// spelled, which may have been `\[`, an environment's `\begin`, or a `$$` on a
/// line of its own — so `preview_select` putting them back is spelling and not
/// copying. `body` is the map of what stands between them.
fn math_piece_origin(body: &TextOrigin) -> TextOrigin {
    let mut piece = TextOrigin::new();
    piece.drawn("$$".len());
    piece.append(body);
    piece.drawn("$$".len());
    piece
}

/// The same, for the `$$` walk, which keeps its body as the source lines it
/// trimmed rather than as a string it has to find its way back out of.
fn math_origins(body: &[&str], out: &RangedBlocks) -> BlockOrigins {
    let mut lines = TextOrigin::new();
    for (index, line) in body.iter().enumerate() {
        if index > 0 {
            // The break between two lines of a formula: the file spells one and
            // the body carries one, but the body's is the join's, not a copy of
            // the author's own break and whatever blanks stood beside it.
            lines.drawn(1);
        }
        lines.copied(out.offset_of(line), line.len());
    }
    BlockOrigins::one(math_piece_origin(&lines))
}

/// **Cut one paragraph's runs into the blocks they are drawn as**: prose,
/// picture, prose.
///
/// See [`MarkdownBlock::Image`]. A picture standing alone in its paragraph — the
/// ordinary case, and every picture in this repository's own `README.md` — comes
/// out as one image block with no prose either side of it, because the runs
/// either side are empty.
///
/// **And the paragraph's source is cut with it.** The blocks this makes are the
/// only ones that do not each own whole lines, because a picture in the middle
/// of a sentence has prose to its left and prose to its right on the same line
/// and no two blocks may share a byte. So the paragraph's span is *partitioned*:
///
/// * a picture's range begins at the first byte of its own `![…](…)` spelling,
///   which is where the prose before it ends;
/// * the prose after a picture begins one past its closing parenthesis — so
///   when nothing but the line's break follows the picture, that break is the
///   following prose's;
/// * and whatever no block was made of — a line's indent, the space between two
///   pictures, the markup of a link wrapped round a picture, the break at the
///   end of a picture's own line — goes to the block on its left, or to the
///   block on its right when it stands before the first one.
///
/// So a picture alone on its line owns that line, break and all, and a picture
/// inside a sentence owns nothing but its spelling. Nothing between the
/// paragraph's first byte and its last is left to no block at all.
fn push_prose(
    spans: Vec<Span>,
    origins: Vec<TextOrigin>,
    images: &[ImageMark],
    source: &ParagraphSource,
    out: &mut RangedBlocks,
) {
    let joined = source.origin();
    let mut run: Vec<TextOrigin> = Vec::new();
    let mut prose: Vec<Span> = Vec::new();
    let mut marks = images.iter();
    let mut cursor = source.span.start;
    let first_block = out.blocks.len();
    for (span, origin) in spans.into_iter().zip(origins) {
        if span.style != SpanStyle::Image {
            prose.push(span);
            run.push(origin);
            continue;
        }
        // One mark per picture run, in the order they stand — see
        // [`parse_inline_marked`], which is the pass that made both.
        let marks = marks
            .next()
            .expect("every picture run was marked where the scan found it");
        let start = source.source_start(marks.spelling.start);
        let end = source.source_end(marks.spelling.end);
        if push_paragraph_run(&mut prose, &mut run, cursor..start, &joined, out) {
            cursor = start;
        }
        let target = span.target.unwrap_or_default();
        out.push(
            MarkdownBlock::Image(MarkdownImage::named(&span.text, &target)),
            cursor..end,
            BlockOrigins::one(image_piece_origin(marks, &joined)),
        );
        cursor = end;
    }
    push_paragraph_run(&mut prose, &mut run, cursor..source.span.end, &joined, out);
    // The tail nobody was made of, given to the last block there is.
    if out.blocks.len() > first_block
        && let Some(last) = out.ranges.last_mut()
    {
        last.end = source.span.end;
    }
}

/// The prose on one side of a picture, dropped when it is nothing but the space
/// that stood between two of them — and whether it was kept, which is what tells
/// the cut above whether those bytes found an owner.
fn push_paragraph_run(
    prose: &mut Vec<Span>,
    run: &mut Vec<TextOrigin>,
    range: Range<usize>,
    joined: &TextOrigin,
    out: &mut RangedBlocks,
) -> bool {
    if prose.iter().all(|span| span.text.trim().is_empty()) {
        prose.clear();
        run.clear();
        return false;
    }
    let origins = BlockOrigins::one(piece_origin(run, joined));
    run.clear();
    out.push(
        MarkdownBlock::Paragraph(std::mem::take(prose)),
        range,
        origins,
    );
    true
}

/// **What a picture's own piece is a copy of.**
///
/// `preview_select` copies a picture as `![alt](src)` — the whole of it, because
/// what is on the page is a picture and the honest plain text for a picture is
/// the picture named as one. Every byte of that spelling is in the file
/// somewhere, but not all of it in one run: a title the page never shows stands
/// between the address and the closing parenthesis, and `<…>` around an address
/// with a space in it is markup. So the piece is five copies — the `![`, the alt
/// text as the label drew it, the `](`, the address, and the `)` — and a caret
/// in any of them lands where the author would point.
fn image_piece_origin(marks: &ImageMark, joined: &TextOrigin) -> TextOrigin {
    let mut piece = TextOrigin::new();
    piece.copied(marks.spelling.start, "![".len());
    piece.append(&marks.alt);
    piece.copied(marks.destination_open, "](".len());
    piece.copied(marks.destination.start, marks.destination.len());
    piece.copied(marks.spelling.end - ")".len(), ")".len());
    piece.through(joined)
}

/// `#` through `######` followed by a space.
///
/// Six levels rather than the mock-up's three (`#{1,3}`), because a `####` in a
/// real document rendered as a paragraph beginning with four hashes is precisely
/// what "the prototype stops here" looks like from the outside.
fn parse_heading(line: &str, out: &RangedBlocks) -> Option<(MarkdownBlock, TextOrigin)> {
    let hashes = line.len() - line.trim_start_matches('#').len();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = line[hashes..].strip_prefix(' ')?;
    // The opening hashes and the space after them are markup and are not drawn;
    // a `##` at the *end* of the line is not, because this parser keeps it (see
    // above) and what it keeps is text like any other.
    let source = TextOrigin::slice(out.offset_of(rest), rest.len());
    let (spans, origins) = parse_inline_marked(rest, &mut Vec::new());
    Some((
        MarkdownBlock::Heading {
            level: hashes as u8,
            spans,
        },
        piece_origin(&origins, &source),
    ))
}

/// One list row: its number if it had one, and what it says.
fn parse_list_row(line: &str) -> Option<(Option<u64>, &str)> {
    if let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        return Some((None, item));
    }
    let digits = line.len()
        - line
            .trim_start_matches(|ch: char| ch.is_ascii_digit())
            .len();
    if digits == 0 {
        return None;
    }
    let item = line[digits..].strip_prefix(". ")?;
    // A number too long to be a number is prose that happens to start with
    // digits; `u64` overflowing is the honest edge to refuse at.
    Some((Some(line[..digits].parse().ok()?), item))
}

/// A quoted line, without its marker. `>` alone is an empty quoted line.
fn strip_quote(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('>')?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

/// `---`, `***` or `___` alone on a line.
///
/// Three or more of one character and nothing else. **Not** a setext heading
/// underline, which is what CommonMark would call a `---` under a paragraph —
/// the preview has no setext headings, so reading it as a rule is the reading
/// that is right in every case this renderer can distinguish.
fn is_thematic_break(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    matches!(first, '-' | '*' | '_')
        && trimmed.chars().count() >= 3
        && trimmed.chars().all(|ch| ch == first)
}

/// Whether a line is shaped like a table row: a pipe somewhere in it, and
/// something other than pipes and spaces.
fn is_pipe_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('|') && trimmed.chars().any(|ch| ch != '|' && !ch.is_whitespace())
}

/// Whether a line is the `|---|:--:|` under a heading row.
fn is_table_separator(line: &str) -> bool {
    let cells = split_pipe_cells(line);
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let cell = cell.trim().trim_start_matches(':').trim_end_matches(':');
            !cell.is_empty() && cell.chars().all(|ch| ch == '-')
        })
}

/// What the separator row said about each of `columns` columns.
///
/// Padded or trimmed to the heading row's width, because *this* parser — unlike the terminal's,
/// which refuses a table whose two top rows disagree — accepts a separator of another width, and a
/// column with no alignment entry would be a column the painter could not ask about.
fn table_alignments(separator: &str, columns: usize) -> Vec<bt_detect::table::ColumnAlignment> {
    let mut declared = bt_detect::table::delimiter_row(separator).unwrap_or_default();
    declared.resize(columns, bt_detect::table::ColumnAlignment::None);
    declared
}

/// One table row's cells, still as text.
///
/// The leading and trailing pipes are optional and are not cells: `| a | b |`
/// and `a | b` are the same two columns, which is what every markdown renderer
/// agrees on and what a document written by hand relies on.
fn split_pipe_cells(line: &str) -> Vec<&str> {
    let trimmed = line.trim();
    let trimmed = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix('|').unwrap_or(trimmed);
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed.split('|').collect()
}

/// One table row, each cell already split into its inline runs.
fn split_pipe_row(line: &str, out: &RangedBlocks, cells: &mut Vec<TextOrigin>) -> TableRow {
    split_pipe_cells(line)
        .into_iter()
        .map(|cell| {
            // The pipes are markup and the padding either side of a cell is
            // markup: a cell's piece is the cell trimmed, and it is one copy.
            let text = cell.trim();
            let source = TextOrigin::slice(out.offset_of(text), text.len());
            let (spans, origins) = parse_inline_marked(text, &mut Vec::new());
            cells.push(piece_origin(&origins, &source));
            spans
        })
        .collect()
}

fn flush_list(
    list: &mut Vec<String>,
    source: &mut Vec<TextOrigin>,
    ordered: &mut Option<u64>,
    at: &mut Option<(usize, usize)>,
    out: &mut RangedBlocks,
) {
    // The lines are recorded when a row or a continuation is taken, so they are
    // there exactly when there are rows to flush.
    if let Some((first, last)) = at.take() {
        let mut items = Vec::with_capacity(list.len());
        let mut pieces = Vec::with_capacity(list.len());
        // Parsed here rather than as each row arrives, because a row may still
        // grow: an item's continuation lines are appended to its source, and
        // inline runs cut before the last of them would split a code span or an
        // emphasis pair across the fold.
        for (item, joined) in std::mem::take(list).iter().zip(std::mem::take(source)) {
            let (spans, origins) = parse_inline_marked(item, &mut Vec::new());
            items.push(spans);
            // The marker the page draws is `Piece::prefix` and not `Piece::text`
            // — it is the list's mark, not the item's — so the map begins at the
            // item's first word.
            pieces.push(piece_origin(&origins, &joined));
        }
        out.push_lines(
            MarkdownBlock::List {
                ordered: *ordered,
                items,
            },
            first,
            last,
            BlockOrigins::new(pieces),
        );
    }
    *ordered = None;
}

// ── the rendered page's measure (mock-up 608-609, 1201-1211; re-proportioned
//    against Typora's GitHub theme, user report 2026-08-16) ──────────────────

/// `.pv-md { font-size: 13px }` (mock-up 629).
///
/// **Unchanged by the Typora pass, and deliberately.** Everything below is
/// expressed as a ratio of *this* number rather than as one of Typora's own
/// pixels, because Typora sets a document at 16px in a window and this is a
/// document in a pane three levels deep, beside a terminal grid at 12.5px. The
/// user's report was that the page reads dense — it was not that the letters
/// are small — so what is ported here is the **proportions**, and the base they
/// are proportional to stays the house's.
pub const PREVIEW_MD_FONT_LOGICAL_PX: f32 = 13.0;
/// `.pv-md { padding: 12px 16px }` (mock-up 629).
pub const PREVIEW_MD_PADDING_X_LOGICAL_PX: f32 = 16.0;
pub const PREVIEW_MD_PADDING_Y_LOGICAL_PX: f32 = 12.0;
/// `.md-code .lang { font-size: 9.5px; letter-spacing: .08em }` (mock-up 1290).
pub const PREVIEW_MD_LANG_FONT_LOGICAL_PX: f32 = 9.5;
pub const PREVIEW_MD_LANG_TRACKING_EM: f32 = 0.08;

/// How wide the prose column is allowed to get, in ems of the body.
///
/// **77em — a thousand logical pixels of this window's 13px body** (user ruling,
/// 2026-09-07), written as a round em because a measure is a reading decision
/// and not a pixel count.
///
/// It was 54em, and 54 was faithful: github.css sets `#write { max-width: 860px
/// }` on a 16px body, which is 53.75em, and 2026-08-16 ported Typora's *ratios*
/// rather than its pixels for the reason [`PREVIEW_MD_FONT_LOGICAL_PX`] states.
/// What the port could not carry across was that 860 pixels is a number chosen
/// for a 16px page in a window, and 54em of a 13px body in a pane three levels
/// deep is 702 pixels. The report is a screenshot of what that costs: a `.md`
/// file open as a tab of its own, 1770 logical pixels of pane, and the document
/// reading down a strip in the middle of it under 40% of the width — with a wide
/// table clipped at the strip's edge and no way to reach the rest.
///
/// **The cap is the only thing the ruling changes.** The page's own padding, the
/// centring and the narrow-pane rule are all as they were: see
/// [`markdown_measure_box`], which is untouched. And a cap there still is,
/// against "run to the pane" — prose whose line length is whatever the window
/// happens to be is prose that has to be re-learned at every window size, and
/// the eye that has to find the start of the next line pays for it every line.
/// Still an em rather than a pixel count, so that a 4K monitor at 200% gets a
/// column twice as wide and not a column of 1001 physical pixels with a mile of
/// nothing beside it.
///
/// The column is **centred** when the pane can hold it, which is `#write`'s own
/// `margin: 0 auto`; a pane narrower than the measure wraps at the pane exactly
/// as it did before, because a measure enforced on a 300px pane is a 300px pane
/// with a hole down one side.
pub const PREVIEW_PROSE_MEASURE_EM: f32 = 77.0;
/// github.css: `body { line-height: 1.6 }`.
///
/// The old value was the window's own `CHROME_LINE_HEIGHT` of 1.4, which is a
/// *chrome* number — a tab strip, a row of a file tree, a button — where a line
/// is a label and never has a line under it to find. Prose is the opposite case
/// and 1.4 is what the report called tight.
pub const PREVIEW_MD_LINE_HEIGHT: f32 = 1.6;
/// github.css: `p, blockquote, ul, ol, dl, table, pre { margin: 0 0 16px }` — a
/// flat 1em of the body between every pair of block siblings.
pub const PREVIEW_MD_PARAGRAPH_GAP_EM: f32 = 1.0;
/// github.css `h1 … h6 { font-size: 2em / 1.5em / 1.25em / 1em / .875em / .85em }`.
///
/// **This replaces a ladder this house chose for itself** on 2026-08-13
/// (`1.45 / 1.28 / 1.14 / 1.05 / 1.00 / 0.92`), which was a compression of the
/// same shape — every step present, every step smaller. Compressing it was the
/// mistake the report names: at 1.45 an `#` is a bold line of text and not a
/// title, so a long document arrived as one undifferentiated column. Typora's
/// h1 is twice the body and the structure is visible from across the room.
pub const PREVIEW_MD_HEADING_LADDER: [f32; 6] = [2.0, 1.5, 1.25, 1.0, 0.875, 0.85];
/// github.css: `h1 … h6 { line-height: 1.25 }` — tighter than the body's 1.6,
/// because a two-line heading set at 1.6 reads as two headings.
pub const PREVIEW_MD_HEADING_LINE_HEIGHT: f32 = 1.25;
/// github.css: `h1 … h6 { margin: 24px 0 16px }` — 1.5em above, 1em below, of
/// the *body* and not of the heading's own size, so an h1 and an h6 sit the
/// same distance off the paragraph above them.
pub const PREVIEW_MD_HEADING_MARGIN_TOP_EM: f32 = 1.5;
pub const PREVIEW_MD_HEADING_MARGIN_BOTTOM_EM: f32 = 1.0;
/// github.css: `h1, h2 { padding-bottom: .3em; border-bottom: 1px solid }`.
///
/// An em of the **heading's own** size, which is what CSS `padding` means and
/// what makes the rule under an h1 stand further off its letters than the rule
/// under an h2 stands off its.
pub const PREVIEW_MD_HEADING_RULE_PADDING_EM: f32 = 0.3;
/// How deep the underlined levels go: `h1, h2` and no further.
pub const PREVIEW_MD_HEADING_RULE_LEVELS: u8 = 2;
/// github.css: `ul, ol { padding-left: 30px }` on a 16px body — 1.875em.
pub const PREVIEW_MD_LIST_INDENT_EM: f32 = 1.875;
/// github.css: `li + li { margin-top: .25em }` — above the second item and
/// every one after it, never above the first, which is why the list's own top
/// margin is not this number.
pub const PREVIEW_MD_LIST_ITEM_GAP_EM: f32 = 0.25;
/// github.css: `blockquote { border-left: .25em solid }` (4px on 16px).
///
/// **The bar does not move**: .25em of 13px rounds to the 3px this window
/// already drew, chosen in 2026-08-13 as "the width a bar has to be before it
/// reads as a bar". The two arrived at the same pixel from opposite directions,
/// which is the strongest evidence either of them was right.
pub const PREVIEW_MD_QUOTE_BAR_EM: f32 = 0.25;
/// github.css: `blockquote { padding: 0 15px }` — .9375em beside the bar.
pub const PREVIEW_MD_QUOTE_PADDING_X_EM: f32 = 0.9375;
/// github.css says `0`; this house keeps 2px.
///
/// **A recorded deviation, and the reason is the bar.** In a browser a
/// blockquote's border runs the height of its line boxes and the leading above
/// the first line and below the last comes free from `line-height`. Here the bar
/// is a quad drawn to the block's measured height, so a zero here draws a bar
/// that begins exactly at the cap of the first letter and stops exactly at the
/// baseline of the last — an accent that looks cut rather than drawn.
pub const PREVIEW_MD_QUOTE_PADDING_Y_LOGICAL_PX: f32 = 2.0;
/// github.css: `code, pre { font-size: 85% }` — inline spans and fences alike.
///
/// The report's "inline code the same size as prose" in one number. A monospace
/// face at the same nominal size as the sans beside it *looks* a size larger,
/// because its x-height and its stems are built for a grid; 85% is the ratio
/// GitHub, Typora and every editor theme derived from them settled on.
pub const PREVIEW_MD_CODE_FONT_RATIO: f32 = 0.85;
/// github.css: `pre { line-height: 1.45 }` — a fence is code, and code does not
/// want prose leading between its lines.
pub const PREVIEW_MD_CODE_LINE_HEIGHT: f32 = 1.45;
/// github.css: `pre { padding: 16px }` — 1em of the body, on all four sides.
/// The mock-up drew `8px 12px` (1284); the fence was cramped and the report says
/// so, so the mock-up loses this one and the divergence is written down.
pub const PREVIEW_MD_CODE_PADDING_EM: f32 = 1.0;
/// github.css: `pre { margin: 0 0 16px }` — a fence is a block sibling and gets
/// a block sibling's air, not the mock-up's 6px (1284).
pub const PREVIEW_MD_CODE_MARGIN_EM: f32 = 1.0;
/// `.md-code { border-radius: 7px }` (mock-up 1284), carried for the day the
/// fill pass grows rounded corners; the fence is a square block today.
pub const PREVIEW_MD_CODE_RADIUS_LOGICAL_PX: f32 = 7.0;
/// github.css: `hr { margin: 24px 0 }` — 1.5em, a heading's own top margin,
/// because a rule and a heading are the same gesture at different volumes.
pub const PREVIEW_MD_RULE_MARGIN_EM: f32 = 1.5;
/// github.css: `table th, table td { padding: 6px 13px }` — .375em by .8125em.
///
/// The old numbers were the `.csv` grid's `4px 10px` (mock-up 610-613), borrowed
/// whole so "the two tables in this product look like one table". They still
/// nearly do: at 13px these come out one pixel larger on each axis, and the csv
/// grid is set at 12px, so the two remain within a pixel of each other while the
/// markdown table now carries the ratio its own theme states.
pub const PREVIEW_MD_TABLE_PADDING_X_EM: f32 = 0.8125;
pub const PREVIEW_MD_TABLE_PADDING_Y_EM: f32 = 0.375;

/// The metrics a rendered markdown body is set in.
///
/// **Every field is a ratio of [`PREVIEW_MD_FONT_LOGICAL_PX`] resolved at one
/// scale**, and the ratios are Typora's default GitHub theme — see the constants
/// above, each of which cites the `github.css` rule it comes from. Nothing here
/// is chosen freehand any more: the two fields that were (`paragraph_gap` and
/// `list_indent`, written down in 2026-08-13 as "chosen, so that the day they
/// are wrong there is a number to argue with") are now the theme's 1em and
/// 1.875em, and the day arrived on 2026-08-16.
///
/// It lives here rather than beside the seat geometry because it is a property
/// of **the document**, not of the furniture around it: the same numbers set the
/// page in a pane, in a preview float and in a hover peek card.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewMarkdownMetrics {
    pub font_size: f32,
    pub line_height: f32,
    pub padding_x: f32,
    pub padding_y: f32,
    /// The widest the prose column may be drawn, before the page's own padding.
    /// See [`PREVIEW_PROSE_MEASURE_EM`] and [`markdown_measure_box`].
    pub measure: f32,
    /// A heading's air above and below — asymmetric, unlike everything the
    /// mock-up inherited, because `24px 0 16px` is asymmetric.
    pub heading_margin_top: f32,
    pub heading_margin_bottom: f32,
    /// The hairline under an `h1`/`h2`: one device pixel of `--border`.
    pub heading_rule_thickness: f32,
    pub code_margin: f32,
    pub code_padding_x: f32,
    pub code_padding_y: f32,
    /// A fence's own size and leading — 85% and 1.45, not the body's.
    pub code_font: f32,
    pub code_line_height: f32,
    /// `.md-code { border-radius: 7px }`, carried for the day the fill pass
    /// grows rounded corners; the fence is a square block today.
    pub code_radius: f32,
    pub code_border: f32,
    pub lang_font: f32,
    /// `.md-code .lang { top: 5px; right: 9px }` (mock-up 1290).
    pub lang_inset_top: f32,
    pub lang_inset_right: f32,
    /// A block sibling's 1em, collapsed between neighbours.
    pub paragraph_gap: f32,
    /// `ul, ol { padding-left: 30px }`, with room for the bullet inside it.
    pub list_indent: f32,
    /// `li + li { margin-top: .25em }`.
    pub list_item_gap: f32,
    /// `blockquote`'s accent bar.
    pub quote_bar: f32,
    /// `blockquote { padding: 0 15px }`, beside the bar.
    pub quote_padding_x: f32,
    /// Where a quoted line's text starts: the bar plus that padding.
    pub quote_indent: f32,
    /// See [`PREVIEW_MD_QUOTE_PADDING_Y_LOGICAL_PX`] — the house's own 2px, kept.
    pub quote_padding_y: f32,
    /// `<hr>` — one device pixel of `--border`, whatever the scale.
    ///
    /// **github.css says `height: .25em` and this house says one pixel**, which
    /// is the deviation the mock-up already implies: every other divider this
    /// window draws is a hairline (`--border`, one device pixel), and a rule four
    /// pixels thick in the middle of a document would be the heaviest mark on the
    /// page. The *margin* around it is Typora's; only the weight is the house's.
    pub rule_thickness: f32,
    pub rule_margin: f32,
    pub table_border: f32,
    pub table_padding_x: f32,
    pub table_padding_y: f32,
    /// The narrowest a column may be squeezed to, however short its cells are:
    /// four characters' worth, below which a wrapped cell breaks every word.
    pub table_min_column: f32,
}

impl PreviewMarkdownMetrics {
    /// The size a heading of this level is set at — [`PREVIEW_MD_HEADING_LADDER`].
    pub fn heading_font(&self, level: u8) -> f32 {
        self.font_size * PREVIEW_MD_HEADING_LADDER[(level.clamp(1, 6) - 1) as usize]
    }

    /// The line box a heading of this level sits in — its own size at
    /// [`PREVIEW_MD_HEADING_LINE_HEIGHT`], not the body's 1.6.
    pub fn heading_line_height(&self, level: u8) -> f32 {
        (self.heading_font(level) * PREVIEW_MD_HEADING_LINE_HEIGHT)
            .round()
            .max(1.0)
    }

    /// The `.3em` of air between an underlined heading's last line and its rule.
    /// Zero for the levels that carry no rule.
    pub fn heading_rule_padding(&self, level: u8) -> f32 {
        if level > PREVIEW_MD_HEADING_RULE_LEVELS {
            return 0.0;
        }
        (self.heading_font(level) * PREVIEW_MD_HEADING_RULE_PADDING_EM).round()
    }

    /// How much taller than its text an underlined heading's box is: the padding
    /// and the hairline together. **One number, read by the pass that measures
    /// the block and by the pass that paints it** — the rule this file's
    /// neighbour states in as many words, applied to the one piece of chrome a
    /// heading owns.
    pub fn heading_rule_extent(&self, level: u8) -> f32 {
        if level > PREVIEW_MD_HEADING_RULE_LEVELS {
            return 0.0;
        }
        self.heading_rule_padding(level) + self.heading_rule_thickness
    }
}

/// The metrics at one scale.
pub fn markdown_metrics(scale: f32) -> PreviewMarkdownMetrics {
    let font_size = PREVIEW_MD_FONT_LOGICAL_PX * scale;
    let em = |ratio: f32| (font_size * ratio).round();
    let hairline = scale.round().max(1.0);
    let quote_bar = em(PREVIEW_MD_QUOTE_BAR_EM).max(1.0);
    let quote_padding_x = em(PREVIEW_MD_QUOTE_PADDING_X_EM);
    let code_font = font_size * PREVIEW_MD_CODE_FONT_RATIO;
    PreviewMarkdownMetrics {
        font_size,
        line_height: (font_size * PREVIEW_MD_LINE_HEIGHT).round().max(1.0),
        padding_x: (PREVIEW_MD_PADDING_X_LOGICAL_PX * scale).round(),
        padding_y: (PREVIEW_MD_PADDING_Y_LOGICAL_PX * scale).round(),
        measure: em(PREVIEW_PROSE_MEASURE_EM),
        heading_margin_top: em(PREVIEW_MD_HEADING_MARGIN_TOP_EM),
        heading_margin_bottom: em(PREVIEW_MD_HEADING_MARGIN_BOTTOM_EM),
        heading_rule_thickness: hairline,
        code_margin: em(PREVIEW_MD_CODE_MARGIN_EM),
        code_padding_x: em(PREVIEW_MD_CODE_PADDING_EM),
        code_padding_y: em(PREVIEW_MD_CODE_PADDING_EM),
        code_font,
        code_line_height: (code_font * PREVIEW_MD_CODE_LINE_HEIGHT).round().max(1.0),
        code_radius: (PREVIEW_MD_CODE_RADIUS_LOGICAL_PX * scale).round(),
        code_border: hairline,
        lang_font: PREVIEW_MD_LANG_FONT_LOGICAL_PX * scale,
        lang_inset_top: (5.0 * scale).round(),
        lang_inset_right: (9.0 * scale).round(),
        paragraph_gap: em(PREVIEW_MD_PARAGRAPH_GAP_EM),
        list_indent: em(PREVIEW_MD_LIST_INDENT_EM),
        list_item_gap: em(PREVIEW_MD_LIST_ITEM_GAP_EM),
        quote_bar,
        quote_padding_x,
        quote_indent: quote_bar + quote_padding_x,
        quote_padding_y: (PREVIEW_MD_QUOTE_PADDING_Y_LOGICAL_PX * scale).round(),
        rule_thickness: hairline,
        rule_margin: em(PREVIEW_MD_RULE_MARGIN_EM),
        table_border: hairline,
        table_padding_x: em(PREVIEW_MD_TABLE_PADDING_X_EM),
        table_padding_y: em(PREVIEW_MD_TABLE_PADDING_Y_EM),
        table_min_column: (font_size * 4.0).round(),
    }
}

/// Where the prose column stands inside a pane's body: `(left, right)`.
///
/// **`#write { max-width: 860px; margin: 0 auto }`, in two numbers.** A pane
/// narrower than the measure gets what it always got — the body inset by the
/// page's padding, prose folding at the pane — because a measure imposed on a
/// narrow column is a narrow column with a stripe of nothing beside it. A pane
/// wider than the measure gets the column *centred*, with the leftover split
/// evenly, which is the whole of the readability report: prose stops running the
/// width of a maximised window and starts running the width of a page.
///
/// **Markdown only.** A source file, a diff and a csv keep the pane's full width
/// — they are `pre`, their line breaks are the author's, and a measure applied
/// to them would be a claim about a document that the document never made.
///
/// One derivation, read by the painter, by the layout pass and by the scroll-bar
/// geometry alike: three places computing "where does the column start" is three
/// chances for a fence's scrollbar to be tested where it is not drawn.
pub fn markdown_measure_box(body: [f32; 4], metrics: PreviewMarkdownMetrics) -> (f32, f32) {
    let inner = (body[2] - body[0] - metrics.padding_x * 2.0).max(1.0);
    if inner <= metrics.measure {
        let left = body[0] + metrics.padding_x;
        return (left, left + inner);
    }
    // Rounded, not floored: an odd number of leftover pixels would otherwise
    // put the column half a pixel left of centre and blur every glyph on it.
    let left = (body[0] + (body[2] - body[0] - metrics.measure) / 2.0).round();
    (left, left + metrics.measure)
}

/// The vertical margin one markdown block asks for above and below itself.
///
/// **A pair rather than one number, which is the change** — every rule the
/// mock-up inherited was symmetric, and github.css's headings are not:
/// `margin: 24px 0 16px` puts more air above a heading than below it, and that
/// asymmetry is what glues a heading to the paragraph it introduces instead of
/// to the one it follows. The report's "headings glued to the paragraph above"
/// is exactly a symmetric margin seen from the outside.
///
/// `previous` is the block before this one, and it answers the two `:first-child`
/// rules github.css states: the first block of a document has no top margin (it
/// would push the whole page down off its own padding), and neither does a
/// heading that follows another heading (`## Section` directly under `# Title`
/// is one masthead, not two).
pub fn markdown_block_margins(
    block: &MarkdownBlock,
    previous: Option<&MarkdownBlock>,
    metrics: PreviewMarkdownMetrics,
) -> (f32, f32) {
    let (top, bottom) = match block {
        MarkdownBlock::Heading { .. } => {
            let top = if matches!(previous, Some(MarkdownBlock::Heading { .. })) {
                0.0
            } else {
                metrics.heading_margin_top
            };
            (top, metrics.heading_margin_bottom)
        }
        MarkdownBlock::Code { .. } => (metrics.code_margin, metrics.code_margin),
        MarkdownBlock::Rule => (metrics.rule_margin, metrics.rule_margin),
        // A list, a quote and a table all ask for a `<p>`'s own air: github.css
        // names them in the same rule and nothing about them argues for more.
        MarkdownBlock::List { .. }
        | MarkdownBlock::Paragraph(_)
        | MarkdownBlock::Quote(_)
        | MarkdownBlock::Table { .. }
        // Display mathematics is a block sibling and asks for a sibling's air.
        // github.css has no rule for it because github.css predates it; every
        // renderer that draws one — Typora, KaTeX's own `.katex-display` — sets
        // it in the same `1em` a paragraph stands in.
        | MarkdownBlock::Math { .. }
        // `img { }` — github.css gives a picture no margin of its own, because
        // in a browser the picture is inside the `<p>` that carries it and the
        // paragraph's own air is what stands around it. Here the picture *is*
        // the block, so the paragraph's air is what it asks for: the same
        // sentence said in the place this window keeps it.
        | MarkdownBlock::Image(_) => (metrics.paragraph_gap, metrics.paragraph_gap),
    };
    (if previous.is_none() { 0.0 } else { top }, bottom)
}

/// Split a comma-separated file into rows of cells.
///
/// **Quote-aware**, which the mock-up's `r.split(",")` is not. The prototype's
/// own fixture has no quoted fields so the naive split was never wrong there;
/// a real file has them, and a grid whose columns shift at the first quoted
/// comma is not a table. Everything else is the mock-up's (4963-4968): no
/// sorting, no frozen header, no paging.
pub fn csv_rows(content: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = content.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted => {
                // `""` inside a quoted field is one literal quote.
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            }
            '"' => quoted = true,
            ',' if !quoted => cells.push(std::mem::take(&mut cell)),
            '\n' if !quoted => {
                cells.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut cells));
            }
            '\r' if !quoted => {}
            other => cell.push(other),
        }
    }
    if !cell.is_empty() || !cells.is_empty() {
        cells.push(cell);
        rows.push(cells);
    }
    // A file that is nothing but whitespace is an empty table, not a table with
    // one empty cell — the mock-up trims before it splits.
    rows.retain(|row| row.iter().any(|cell| !cell.trim().is_empty()));
    rows
}

/// `.pv-edit { tab-size: 4 }` (mock-up 603).
///
/// Expanded into spaces on the way to the shaper rather than shaped as a tab,
/// because a tab is a *stop* and cosmic-text has no tab stops: what the CSS
/// property names is a column grid, and in a monospace face a column grid is
/// exactly N columns.
pub const PREVIEW_TEXT_TAB_WIDTH: usize = 4;

/// Expand tab stops the way `tab-size: 4` does.
///
/// **Column-aware, not a blind replace.** A tab advances to the next multiple of
/// four *columns*, so a tab after two characters is worth two spaces and a tab
/// at the start of a line is worth four. Replacing each with four spaces is what
/// misaligns every continuation line of an indented file, which is precisely
/// what a preview of source code is for. Columns and not characters, so a wide
/// character counts for the two cells it draws in.
pub fn expand_tabs(line: &str) -> String {
    if !line.contains('\t') {
        return line.to_owned();
    }
    let mut out = String::with_capacity(line.len());
    let mut column = 0usize;
    for cluster in bt_unicode::graphemes(line) {
        if cluster == "\t" {
            let advance = PREVIEW_TEXT_TAB_WIDTH - column % PREVIEW_TEXT_TAB_WIDTH;
            out.extend(std::iter::repeat_n(' ', advance));
            column += advance;
        } else {
            out.push_str(cluster);
            column += bt_unicode::cluster_width(cluster);
        }
    }
    out
}

/// How wide the widest line of a body is, in the columns it will draw as.
///
/// **Columns, not bytes and not characters.** It is what the horizontal
/// scroller's extent is derived from, so a wide character measured as one column
/// would leave the end of its own line permanently unreachable.
fn widest_line_columns(text: &str) -> usize {
    text.lines()
        .map(|line| bt_unicode::text_width(&expand_tabs(line)))
        .max()
        .unwrap_or(0)
}

/// The read-only degradation §7.1.3 asks for, **as the phrase the pane's foot
/// hangs on its right hand** (user ruling, 2026-08-15).
///
/// It was a sentence — "Read-only — showing the first 64 KB of this file" —
/// standing in a 28px bar of its own directly above the path strip. The ruling
/// retired that bar (two strips of identical height stacked at the bottom of one
/// pane), and a phrase that has to share a 28px strip with a path is a phrase
/// that says the two facts and stops: what the file is (read-only) and how much
/// of it you are looking at.
///
/// The size is [`PREVIEW_HEAD_BYTES`] said the way [`format_byte_size`] says it,
/// pinned by a test rather than left as two numbers that can drift apart.
pub fn preview_truncated_notice() -> &'static str {
    crate::i18n::Text::PreviewTruncated.text()
}

/// **What a body this window could not fully read says instead** (T2 ②,
/// 2026-09-10).
///
/// [`preview_truncated_notice`]'s neighbour on the same strip and in the same
/// shape — the two facts and then stop — because it answers the same reader's
/// question: why can I not type in this. Truncation is about the end of a file
/// that is missing; this is about bytes in the middle of it that did not decode,
/// and a save would write this window's guesses over them.
pub fn preview_lossy_notice() -> &'static str {
    crate::i18n::Text::PreviewLossy.text()
}

/// **What a file past [`PREVIEW_EDIT_BYTES`] says** (T2 ③, 2026-09-10).
///
/// The third phrase on that strip, and the one that means "this is as far as
/// asking to edit gets you": the whole-file read was made and the file is larger
/// than this window will put in memory and re-parse on every keystroke. The size
/// is the constant said the way [`format_byte_size`] says it, pinned by a test
/// rather than left as two numbers that can drift apart.
pub fn preview_too_large_notice() -> &'static str {
    crate::i18n::Text::PreviewTooLargeToEdit.text()
}

/// A byte count the way a file manager says it.
///
/// Binary units, because that is what Explorer's own column shows on this
/// platform and a preview that disagreed with the property sheet beside it would
/// be the one making the user check.
pub fn format_byte_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    match bytes {
        b if b < KB => format!("{b} B"),
        b if b < MB => format!("{} KB", b.div_ceil(KB / 2) / 2),
        b if b < GB => format!("{:.1} MB", b as f64 / MB as f64),
        b => format!("{:.1} GB", b as f64 / GB as f64),
    }
}

/// **The middle dot every fact line in this window is joined with**, and the
/// join itself (user ruling 2026-08-29).
///
/// A picture's meta strip, a video's, and the one line a glance card says about
/// its file are one sentence written at three sizes: *the facts this file has,
/// in the order a reader wants them, separated by a dot*. They were three
/// hand-rolled `Vec` pushes with the same literal in each of them, which is a
/// separator that can drift — and the day it drifts the same file reads as two
/// different files depending on which surface is showing it.
///
/// A field nothing has answered yet is left out rather than printed as a
/// placeholder, and nothing moves up to take its place because there is nothing
/// to move: the line is built out of the facts that exist. All of them missing
/// is `None` — a card with nothing to say says nothing, rather than drawing an
/// empty strip.
#[must_use]
pub fn join_facts(fields: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    let fields: Vec<String> = fields.into_iter().flatten().collect();
    (!fields.is_empty()).then(|| fields.join(" \u{b7} "))
}

/// **How large the thing in this file is, in its own pixels** — the one spelling
/// of a picture's or a recording's size (user ruling 2026-08-29).
///
/// The multiplication sign is `×` and not the letter `x`, and it is written once
/// here so that a photograph's card, a photograph's pane and a recording's
/// control strip cannot come to spell one file's dimensions three ways.
#[must_use]
pub fn format_pixel_size(width: u32, height: u32) -> String {
    format!("{width} \u{d7} {height}")
}

/// **What one open of a video learned besides its picture** (user ruling
/// 2026-08-27; §7.23).
///
/// Three optional facts and no picture, because the picture and the facts have
/// different lifetimes on the surfaces that draw them: pixels are held in the
/// window's decode cache and evicted like any other picture's, while these are a
/// sentence, cost nothing to keep, and are still true of a file whose frame this
/// machine has no decoder for. That last case is the whole reason they are a
/// type of their own — see [`video_fact_lines`], whose degraded form is written
/// entirely out of a `Self::default()` and a file name.
///
/// **`native` is the video's own size and not the frame's.** The two differ
/// whenever the decoder honoured the fit it was asked for, and the sentence a
/// reader wants is about the recording rather than about the thumbnail this
/// window happened to request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VideoFacts {
    /// How long it runs, in milliseconds. `None` for a container that declares
    /// no duration — a stream still being written, a capture with no index.
    pub duration_ms: Option<u64>,
    /// The recording's own pixel dimensions.
    pub native: Option<(u32, u32)>,
    /// How large the file is. Read by the same worker hop that opened it, so a
    /// file whose frame will not decode still has this one.
    pub bytes: Option<u64>,
}

/// A duration the way a player's counter says it: `m:ss`, and `h:mm:ss` once
/// there is an hour to say.
///
/// Minutes are **not** zero-padded and seconds always are, which is the shape
/// every media counter in the world has and the reason a reader can tell this
/// field from the resolution beside it without a label. Seconds are truncated
/// rather than rounded: a clip of 59.6 seconds says `0:59`, because a counter
/// that said `1:00` would be naming a moment the file does not reach.
#[must_use]
pub fn format_duration(milliseconds: u64) -> String {
    let total = milliseconds / 1000;
    let (hours, minutes, seconds) = (total / 3600, (total / 60) % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// **The two lines a video's card and a video's pane both print** (user ruling
/// 2026-08-27; §7.23).
///
/// One function for both surfaces, because they are one sentence said at two
/// sizes — the glance card and the preview pane are two readings of one file,
/// and the day they were written separately is the day one of them would start
/// saying something the other does not.
///
/// * **First, what the recording is**: how long it runs and how large its
///   picture is, joined by the same middle dot the picture's meta line uses.
/// * **Then, what the file is**: its size on disk.
///
/// # The degraded line, which is the point of the first argument
///
/// A machine with no decoder for this container answers the frame with nothing,
/// and then "how long" and "how large" are both unknown — but the reader is
/// still hovering a file and still owed an answer. So the first line falls back
/// to the **format**, spelled the way the meta line under a picture spells one:
/// the extension, upper-cased. `MP4` and a size is a poorer card than a frame
/// and a running time, and it is a card rather than the "no preview for this
/// file type" refusal this class was invented to stop showing.
///
/// Nothing is ever replaced by a placeholder and nothing moves up to fill a gap:
/// a duration that never arrives leaves the resolution standing alone, exactly
/// as [`crate::file_peek::facts_lines`] leaves a size standing alone under a PDF
/// whose page count could not be read.
#[must_use]
pub fn video_fact_lines(extension: Option<&str>, facts: VideoFacts) -> [Option<String>; 2] {
    let first = join_facts([
        facts.duration_ms.map(format_duration),
        facts
            .native
            .map(|(width, height)| format_pixel_size(width, height)),
    ])
    .or_else(|| {
        extension
            .map(str::to_uppercase)
            .filter(|extension| !extension.is_empty())
    });
    // **The second line says how large the file is, and nothing else any more**
    // (route B slice ②, 2026-08-28; §7.44 ⑥).
    //
    // It used to carry a second sentence for the one column of the class that
    // had a face and no player. That column is gone — the still and the
    // playback come off one decoder now, so every name in the table plays — and
    // a line that said "this format cannot be played" would be a line this
    // window no longer means. **A machine that genuinely cannot play a
    // particular file still says so**, but it says it out of the engine's own
    // error at the moment the file is opened (a Store codec that is not
    // installed), which is where a fact about a machine belongs; a constant
    // compiled on a build server was never able to know it.
    [first, facts.bytes.map(format_byte_size)]
}

/// Whether a path is one this window declines to read off a resting pointer.
///
/// DESIGN 7.1.3, with 3.4's attachment discipline behind it: **a network path is
/// not previewed automatically.** The cost of being wrong is not a slow frame,
/// it is a hover that dials a disconnected share and blocks for the operating
/// system's own timeout, and the read is not something the user asked for by
/// name.
///
/// **The rule itself lives one crate down**, in
/// [`bt_transcript::paths::may_read_unasked`], and this is its name inside the
/// preview: a share on another machine, a device path, a verbatim spelling and a
/// distribution's share are one question with one answer, and this file used to
/// hold a second reading of two thirds of it. The refusal it files is still
/// [`PreviewRefusal::NetworkPath`], because a share is the shape a reader
/// actually meets and the card has said so since 7.1.3.
///
/// [`PathNamer::ThisWindow`]: a preview source is a path this window already
/// holds — a row of a column, a file a person picked, a target the terminal's own
/// routing table has already put the pane's question to.
pub fn is_readable_unasked(path: &Path) -> bool {
    bt_transcript::paths::may_read_unasked(path, bt_transcript::paths::PathNamer::ThisWindow)
}

/// The four ways a file declines to be previewed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewRefusal {
    /// Nothing in this window reads this kind of file.
    Type,
    /// The head held a NUL, whatever the name claimed.
    Binary,
    /// A share on another machine, which §7.1.3 does not read unasked.
    NetworkPath,
    /// The disk was asked and said no.
    Fault(PreviewFault),
}

impl PreviewRefusal {
    /// The card's own words for itself.
    ///
    /// The mock-up's card says one sentence (4986); a real one has four things
    /// to say and saying the right one is the whole difference between "this
    /// window cannot" and "this file is not there".
    pub fn notice(self) -> &'static str {
        match self {
            Self::Type => crate::i18n::Text::PreviewRefusalType.text(),
            Self::Binary => crate::i18n::Text::PreviewRefusalBinary.text(),
            Self::NetworkPath => crate::i18n::Text::PreviewRefusalNetworkPath.text(),
            Self::Fault(fault) => fault.notice(),
        }
    }

    /// **Whether this card carries the button that hands the file to the
    /// machine** (R1-12).
    ///
    /// The button says 「这扇窗读不了它,系统也许能」, and that sentence is only
    /// true of a refusal about the *content*: a format nothing here reads, a
    /// head full of NULs. Every card wore it, so the two refusals it is false
    /// of wore it too, and one of them wore it dangerously.
    ///
    /// * [`Self::NetworkPath`] is not about the content at all — it is §7.1.3
    ///   declining to touch another machine's share, because reading one stalls
    ///   this window on somebody else's network for the operating system's own
    ///   timeout. `ShellExecuteW` is synchronous on the window thread, so a
    ///   button that hands the share over undoes that refusal on one press,
    ///   from the very card that announced it.
    /// * [`Self::Fault`] is the disk having already said no to this process.
    ///   The registered handler is another process on the same machine reading
    ///   the same file with the same rights, so the button would be offering to
    ///   fail again somewhere the reader cannot see it — and for
    ///   [`PreviewFault::NotFound`] there is nothing to open at all.
    ///
    /// A card with no button is still a card: it has the mark, the sentence and
    /// the seat, which is the whole of what those two refusals have to say.
    #[must_use]
    pub fn offers_the_default_app(self) -> bool {
        match self {
            Self::Type | Self::Binary => true,
            Self::NetworkPath | Self::Fault(_) => false,
        }
    }
}

/// The ways a file can decline to be read, mirroring [`crate::files::DirFault`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewFault {
    PermissionDenied,
    NotFound,
    Unreadable,
}

impl PreviewFault {
    /// Public because the disk is asked in two places now: [`read_head`], on the
    /// worker, and `Runtime::open_preview_web_file`, whose `canonicalize` is the
    /// step that decides whether a name that says page has a page behind it.
    /// Both report the same three faults, and a second mapping of
    /// `io::ErrorKind` would be a second vocabulary for one answer.
    pub fn from_io(error: &std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            std::io::ErrorKind::NotFound => Self::NotFound,
            _ => Self::Unreadable,
        }
    }

    pub fn notice(self) -> &'static str {
        match self {
            Self::PermissionDenied => crate::i18n::Text::PreviewRefusalPermissionDenied.text(),
            Self::NotFound => crate::i18n::Text::PreviewRefusalNotFound.text(),
            Self::Unreadable => crate::i18n::Text::PreviewRefusalUnreadable.text(),
        }
    }
}

/// How far along a buffer's body is.
///
/// Three states rather than an `Option<String>`, for the reason
/// [`crate::files::DirNode`] has three: "not read yet" and "will never be read"
/// are different answers and draw differently, and folding them is how a refusal
/// comes to look like a slow disk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreviewLoad {
    /// Asked, or about to be; the worker has not answered.
    Pending,
    /// Read. `content` holds the head.
    Ready,
    /// No body will ever arrive, and the card says why.
    Refused(PreviewRefusal),
    /// **A composed document whose composer would not answer** — git's own
    /// sentence, carried whole (G-3).
    ///
    /// A fourth state rather than a fifth [`PreviewRefusal`] because the two
    /// differ in what they offer, not only in what they say: the refusal card's
    /// one control is "open this in the default app", and a document composed
    /// out of a repository has no file for that button to hand over. So this
    /// prints where the "Loading …" line prints — one sentence, no card, no way
    /// out that leads nowhere.
    ///
    /// It carries a `String` because the sentence is git's, not this module's.
    /// Every other wording in this window is written here and can be a
    /// `&'static str`; "fatal: bad object deadbee" is written by the program we
    /// asked, and the whole of [`crate::git`]'s fail-soft discipline is that it
    /// reaches the user unedited.
    Unavailable(String),
}

/// **Where a buffer's content comes from** — the identity every preview surface
/// is keyed on.
///
/// This used to be a bare `PathBuf` carrying a comment that said "deliberately a
/// `PathBuf`", and that was right for as long as every preview was a file. The
/// Git block's two surfaces are not: a diff of a working-tree file and a repo's
/// commit graph are *documents this window composes*, with no file behind them
/// that could be opened, saved, or revealed. The mock-up's answer was to smuggle
/// them through as pseudo-paths — `gitgraph:{root}` and `git:{root}:{path}` —
/// which on this platform is not merely ugly but ambiguous: a Windows root
/// already contains a `:`, so `git:C:\w\repo:src\main.rs` cannot be split back
/// into its parts by any rule that does not already know the answer.
///
/// So the identity is a sum, and the disk file is one of its cases. Everything
/// downstream — the pool's key, a pane's pointer, the view a body is drawn as,
/// what gets written to `session.json` — asks the variant rather than parsing a
/// string, and a case that has no file simply has no file rather than having a
/// path that lies.
///
/// **Only [`Self::File`] is constructed today.** The git cases are the shape
/// G-1 (data plane) and G-3 (diff wiring) will fill; this slice moves the
/// skeleton and implements no git.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum PreviewSource {
    /// A file on a disk. Deliberately the whole path: two files called `main.rs`
    /// are two buffers.
    File(PathBuf),
    /// One file's diff in one repository, against one of three things.
    ///
    /// `root` is the repository's top level (`rev-parse --show-toplevel`) and
    /// `path` is repo-relative in git's own grammar — forward slashes, no drive
    /// — because that is what a `git diff` command takes and what its output
    /// names. [`GitDiffAgainst`] carries R25's `--cached` mapping and the third
    /// reading beside it: the three are three different documents of one file,
    /// so they are three different buffers.
    GitDiff {
        root: PathBuf,
        path: String,
        against: GitDiffAgainst,
    },
    /// **One commit's reading of one file** (R15) — `git show {hash} -- {path}`.
    ///
    /// A separate case rather than a `commit: Option<String>` on [`Self::GitDiff`]
    /// because `staged` has no meaning here and a field that is meaningless in
    /// one of its cases is a field every reader has to be told to ignore. A
    /// commit's diff is against that commit's parent, and there is no index in
    /// the question at all.
    GitShow {
        root: PathBuf,
        hash: String,
        path: String,
    },
    /// **One file across a range** (D6) — `git diff {a} [{b}] -- {path}`.
    ///
    /// The compare block's document. It is a third case rather than an
    /// `Option<String>` bolted onto [`Self::GitShow`] for that case's own
    /// reason: a `show` is *one* commit's reading and this is the difference
    /// between two places, and the two questions do not become one by sharing a
    /// field name.
    ///
    /// **`b` is an `Option` inside one variant** and not a fourth variant for
    /// the working-tree end, because that is what keeps every `match` on this
    /// enum one arm longer instead of two: `file_path`, `is_git`,
    /// `composed_lead`, `repo_file`, the load, the view, the question builder
    /// and the session writer all treat "a range" identically however its far
    /// end is spelled, and the one place the difference matters — the argument
    /// list handed to git — is the one place that reads the field.
    GitDiffRange {
        root: PathBuf,
        /// The older end, in the graph's own order.
        a: String,
        /// The newer end, or the working tree when absent.
        b: Option<String>,
        path: String,
    },
    /// One repository's commit graph. Keyed by the repo alone: there is one
    /// graph per repository and it is the same graph whoever asks.
    #[allow(dead_code)]
    GitGraph { root: PathBuf },
    /// **One page** (Web 预览块 W2 片③) — the normalised, whole URL.
    ///
    /// `docs/DESIGN.md` §7.7 ① settled that a page is *a preview buffer and not
    /// a fourth kind of leaf*: it goes into this same pool, is listed by the same
    /// switcher, is named by the same tab, is seeded into the same Recent and is
    /// keyed by this same enum — so that nowhere in this window does anything
    /// have to ask "is this a web page" before it can do its job.
    ///
    /// **The string is the switcher key and nothing else is** (`plan.md` §3
    /// 切换器确定性三则). It is `webnav::switcher_key` of the URL that *last
    /// successfully committed* — `webnav::switcher_identity` of
    /// `webhost::WebMachine::recoverable_url`, which is the same ledger the
    /// session file and the recovery machine read, never a second one. Query and
    /// fragment participate, because they are part of what was asked for; only a
    /// default port is dropped. A navigation that never committed has no identity
    /// and therefore no buffer, which is why a page in flight is a seat with no
    /// row rather than a row naming a page that never existed.
    Web(String),
}

impl PreviewSource {
    /// The common case, spelled short.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::File(path.into())
    }

    /// **The disk file behind this content, when there is one.**
    ///
    /// The one door every file-only capability asks through — saving, revealing
    /// in Explorer, opening with the system handler, resolving a relative
    /// markdown link, the head read. A git-backed document answers `None`, and
    /// `None` is not a failure: those verbs are about a file, and this content
    /// has none. What a git document offers in their place is [`Self::repo_file`]
    /// and [`Self::composed_lead`], below.
    pub fn file_path(&self) -> Option<&Path> {
        match self {
            Self::File(path) => Some(path),
            Self::GitDiff { .. }
            | Self::GitShow { .. }
            | Self::GitDiffRange { .. }
            | Self::GitGraph { .. }
            // A page is not a file and must never answer as one: saving,
            // revealing in Explorer, the head read and the relative-link
            // resolver all come through this door, and every one of them would
            // be wrong about a URL.
            | Self::Web(_) => None,
        }
    }

    /// Whether a repository composed this content.
    pub fn is_git(&self) -> bool {
        match self {
            Self::File(_) => false,
            Self::GitDiff { .. }
            | Self::GitShow { .. }
            | Self::GitDiffRange { .. }
            | Self::GitGraph { .. } => true,
            Self::Web(_) => false,
        }
    }

    /// **The page this buffer is, when it is one** — the switcher key, verbatim.
    ///
    /// [`Self::file_path`]'s opposite number, and separate from it for that
    /// method's own reason: the two answer different questions and a caller that
    /// took either for "where does this content live" would hand a URL to a
    /// filesystem or a path to a navigation. Every door that is about a *page* —
    /// the switcher's row, the pin's category, the session's `source`, the seat
    /// the engine is driving — asks this one.
    pub fn web_url(&self) -> Option<&str> {
        match self {
            Self::Web(url) => Some(url),
            Self::File(_)
            | Self::GitDiff { .. }
            | Self::GitShow { .. }
            | Self::GitDiffRange { .. }
            | Self::GitGraph { .. } => None,
        }
    }

    /// **What the foot prints on the left for a document that has no path**
    /// (G-3).
    ///
    /// The strip's left hand asks "where is this", and for a file the answer is
    /// its path. A diff has no path, but it has the two facts a path would have
    /// carried: which repository, and where in it. So it prints `folio ·
    /// crates/bt-app/src/main.rs`, in git's own spelling of the second half —
    /// which is also what the `diff --git a/… b/…` line in the body says, so the
    /// foot and the document agree letter for letter.
    ///
    /// `None` for a file, deliberately: this is not a general "describe
    /// yourself", it is the branch the foot takes when `file_path` had no
    /// answer, and a file's foot has never gone through it.
    pub fn composed_lead(&self) -> Option<String> {
        let repository = |root: &Path| {
            // A repository at a drive root has no last component. Naming it by
            // the whole root is not a fallback to nothing — it is the only name
            // it has, and an empty word here would be the foot going blank on
            // the one surface whose whole job is saying what you are looking at.
            root.file_name().map_or_else(
                || root.to_string_lossy().into_owned(),
                |name| name.to_string_lossy().into_owned(),
            )
        };
        match self {
            Self::File(_) => None,
            // **A page's foot is its address** (§7.7 ③). It comes through this
            // door and not through [`Self::file_path`] for the reason a git
            // document does: the strip's left hand asks "where does this live",
            // and a page lives at a URL — which is also the string this window
            // hands the default browser when the foot is pressed.
            Self::Web(url) => Some(url.clone()),
            Self::GitDiff { root, path, .. }
            | Self::GitShow { root, path, .. }
            | Self::GitDiffRange { root, path, .. } => {
                Some(format!("{} \u{b7} {path}", repository(root)))
            }
            Self::GitGraph { root } => Some(repository(root)),
        }
    }

    /// **The working-tree file a git document is about**, when it is about one.
    ///
    /// The one file verb a composed document keeps: Explorer can be pointed at
    /// the file a diff is of, because that file is genuinely there. It is *not*
    /// [`Self::file_path`] and must never become it — saving, editing, the head
    /// read and `session.json` all ask that question and all of them would be
    /// wrong about this file. What is true is only that the repository has a
    /// file at this path; whether it still exists is the caller's to check,
    /// because a diff of a deletion names a file that is gone.
    pub fn repo_file(&self) -> Option<PathBuf> {
        match self {
            Self::File(path) => Some(path.clone()),
            Self::GitDiff { root, path, .. }
            | Self::GitShow { root, path, .. }
            | Self::GitDiffRange { root, path, .. } => Some(root.join(path)),
            Self::GitGraph { .. } | Self::Web(_) => None,
        }
    }
}

/// What a composed document with nothing in it says.
///
/// **One way in, not two.** It used to be reached by an untracked file as well,
/// and that was the bug and not the sentence: a file git has never had a copy of
/// has no `git diff` reading, so `git diff -- <path>` printed nothing and exited
/// clean and the pane said *No changes to show* about a file that was nothing
/// but change (user report, 2026-08-17). The reading an untracked file has is
/// against **nothing** — see [`GitDiffAgainst::Nothing`] — and it is a whole
/// file of green. What is left here is the honest empty: a commit's reading of a
/// file it did not touch, and a tracked file whose two copies agree. Neither is
/// a failure, so neither gets the refusal card; they get one line where the body
/// would have been.
#[must_use]
pub fn git_document_empty() -> &'static str {
    crate::i18n::Text::GitDocumentEmpty.text()
}

/// Which of a repository's copies of a file a diff is taken **against**.
///
/// # Three, and not a `staged: bool`
///
/// A tracked file has two readings and a bool was enough for them: the index as
/// against `HEAD`, and the working tree as against the index (R25). An untracked
/// file has neither, and the bool had no way to say so — it answered `false`,
/// which means "the working tree against the index", and git's answer to that
/// about a file it has never seen is an empty patch and exit 0. The third state
/// is not a shade of the second; it is a different command, because the thing
/// the file is being compared to is not in the repository at all.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GitDiffAgainst {
    /// `git diff --cached` — the index as against `HEAD`. R25's STAGED rows.
    Index,
    /// `git diff` — the working tree as against the index. CHANGES rows.
    WorkingTree,
    /// `git diff --no-index -- /dev/null <path>` — the file as against nothing,
    /// which reads as one addition of the whole of it. UNTRACKED rows.
    Nothing,
}

/// One buffer's live content.
///
/// Owned by the tab's [`PreviewPool`] and *referred* to by the panes showing it,
/// which is what makes "a file open in two panes is one buffer" true by
/// construction rather than by two panes agreeing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewBuffer {
    /// The identity — see [`PreviewSource`].
    pub source: PreviewSource,
    /// What the header and the switcher call it.
    pub name: String,
    pub ftype: PreviewFtype,
    /// The head of the file, once read.
    pub content: Option<String>,
    /// Whether [`PREVIEW_HEAD_BYTES`] cut the body short. The read-only
    /// degradation §7.1.3 asks for hangs off this; slice 1 carries the fact and
    /// the view that says so is slice 2's.
    pub truncated: bool,
    /// Unsaved edits.
    ///
    /// **The body is somewhere other than where the disk last saw it**, and
    /// since ticket T3 that is a question about [`Self::undo`] rather than a bit
    /// that only a save could turn off. The log remembers the position it stood
    /// at when the file was written, so undoing back to it is being clean again
    /// and editing away from it is being dirty again — which is what every
    /// editor's dot does and what this one could not do while there was no
    /// history to compare against.
    ///
    /// **Retyping what you deleted still leaves it dirty**, and that is the same
    /// ruling it always was rather than an exception to the new one: two changes
    /// that happen to cancel out are two entries in the log, the position has
    /// moved twice, and the only way back to the saved position is back through
    /// them. What cleans the dot is the road back, never the coincidence.
    ///
    /// A saved position the log can no longer reach — thrown away with a redo
    /// tail, or fallen off the front of a very long session — is dirty from then
    /// on, because the bytes the file holds are no longer anywhere in this
    /// history.
    pub dirty: bool,
    /// How many times this body has changed.
    ///
    /// **A revision, not a length.** Everything derived from the content — the
    /// parsed document, its measured blocks — is cached against this, and the
    /// length it replaced was a counter that could not see a same-length edit:
    /// swapping one letter for another left the cache convinced it was still
    /// looking at the old text.
    pub revision: u64,
    /// **Whether the body this revision names was typed here** (ticket T4;
    /// `docs/DESIGN.md` §7.1.3q).
    ///
    /// One bit, read by exactly one question: when a pane throws away its parse
    /// and builds another, does what the reader had marked go with it? It has to,
    /// when the bytes changed under the parse because another program wrote the
    /// file — a highlight over somebody else's sentence would put those bytes on
    /// the clipboard. It must **not**, when the bytes changed because the reader
    /// typed them, because a document that re-parses on every keystroke would
    /// then drop its own selection on every keystroke.
    ///
    /// **On the buffer and not on the pane**, for [`Self::undo`]'s reason: two
    /// panes on one file are one buffer, and an edit made through either of them
    /// is this window's doing as far as the other one is concerned.
    ///
    /// `false` to begin with, because a buffer's first body is a file's.
    edited_here: bool,
    /// When the file was last written, as of the read this body came from.
    ///
    /// The other half of ruling 8⑨'s minimum concurrent-edit story: a save
    /// compares this against what the disk says *now*, and a disagreement means
    /// somebody else wrote the file while it was open here. `None` for a buffer
    /// that has no body yet — there is nothing to be stale.
    pub disk_mtime: Option<SystemTime>,
    pub load: PreviewLoad,
    /// **A head read is out with the worker** — the question, filed.
    ///
    /// [`PreviewLoad::Pending`] cannot carry this: it says "asked, *or about to
    /// be*", and folding the two was harmless for as long as every caller of
    /// this lane was an event — a file opened, a tab restored, a hand resting on
    /// a row. The focus column's cards are not events: a card is looked at on
    /// every frame it is visible, and a caller on that beat that could not tell
    /// "asked" from "about to be asked" would re-read the file sixty times a
    /// second until the answer landed.
    ///
    /// So it is a second bit beside the load, and it is exactly
    /// [`crate::files::DirNode::Pending`] one lane over: that variant is how the
    /// files column has always known not to ask twice, and this is the same
    /// ledger for the same reason rather than a second one. It is written
    /// through one door ([`Self::claim_head_read`]) and closed by the answer
    /// ([`Self::accept`], [`Self::decline`]).
    head_asked: bool,
    /// **What this window last heard about the file under this body** (user
    /// ruling 2026-08-29).
    ///
    /// [`Self::stale`]'s sibling and deliberately not a fourth bit beside it:
    /// `stale` is a *question that has been asked* — a read is owed and the old
    /// paragraphs stand until the new ones land — while this is a *sentence the
    /// reader is owed*, and the two are owed in exactly the cases the other is
    /// not. A clean buffer whose file moved is re-read and says nothing; a
    /// dirty one is never re-read and must say so; a deleted file's buffer is
    /// neither re-read nor thrown away, and saying so is the whole of what is
    /// left to do about it.
    ///
    /// Written through one door ([`Self::note_disk_moved`]) and closed by three:
    /// the two verbs on the strip and the answer to a read.
    pub disk: DiskNews,
    /// **The disk moved under this body** (W2 slice 5, `preview_watch`).
    ///
    /// A third bit beside the load and beside [`Self::head_asked`], for the
    /// reason that one is a bit rather than a state: `PreviewLoad::Pending` is
    /// what makes the pane print "Loading <name>", and a file saved in an editor
    /// must not make the body it is already showing flash away and come back.
    /// So a re-read is asked for *without* unloading what is on the glass - the
    /// old paragraphs stay until the new ones land, which is the whole of what a
    /// reader wants from a page that refreshes itself.
    ///
    /// Written through one door ([`Self::mark_stale`]) and closed by the answer
    /// ([`Self::accept`], [`Self::decline`]), exactly as `head_asked` is.
    stale: bool,
    /// The widest line of [`Self::content`], in drawn columns.
    ///
    /// Derived once, when the body lands, rather than per frame: it is what the
    /// horizontal scroller's extent is, a scroll happens sixty times a second,
    /// and re-walking sixty-four kilobytes to answer it each time would put the
    /// file's size into the frame budget the head read exists to keep it out of.
    pub max_columns: usize,
    /// **What the bytes said when this buffer was last read** (user ruling
    /// 2026-08-27; `docs/DESIGN.md` §7.32).
    ///
    /// [`HeadOutcome::Read::content_says_text`], kept — because the question is
    /// asked twice and the disk is only visited once. The first asking is the
    /// promotion in [`Self::accept`]; the second is [`Self::rename`], where a
    /// file that has just been called something nobody listed must not lose the
    /// answer this window already has about it. A `script.ps1` renamed to
    /// `script.ps1.old` is the same bytes it was a moment ago, and a pane that
    /// went blank on the rename would be re-asking a question it had answered.
    ///
    /// `false` until a head lands, which is the honest answer for a buffer that
    /// has not been read: nothing has said anything about these bytes yet.
    content_says_text: bool,
    /// **What the file said it was, kept so that a save can say it back** (T2 ①,
    /// 2026-09-10).
    ///
    /// The buffer holds a `String`, and a `String` has no encoding — by the time
    /// the body is here the mark has been eaten and UTF-16 has become `char`s.
    /// That was the defect: [`Self::save`] wrote UTF-8 octets over a file that
    /// had told this window twice what it was, so one edited line rewrote every
    /// byte of a PowerShell transcript. The encoding rides beside the body from
    /// the read that decoded it to the write that encodes it back, and
    /// [`HeadEncoding::encode`] is the one place the mark is put back on.
    ///
    /// [`HeadEncoding::Utf8`] until a body lands, which is what a file with no
    /// mark is anyway and therefore the only default that cannot invent a
    /// sentence the file never said.
    pub encoding: HeadEncoding,
    /// **The decode had to invent characters this file does not contain** (T2 ②,
    /// 2026-09-10).
    ///
    /// [`decode_head`]'s second answer, kept for the one caller that cares:
    /// [`Self::is_editable`]. A lossy body is shown — that is what lossy is
    /// *for*, and a preview that refused a Latin-1 log file would be refusing
    /// log files — but it is never offered a caret, because the save would put
    /// each U+FFFD on the disk over the byte it was standing in for and nothing
    /// would have said so.
    lossy: bool,
    /// **The reader asked to edit this file, so its reads are whole-file reads
    /// now** (T2 ③, owner's ruling 2026-09-10; research §10 Q2).
    ///
    /// Written through one door ([`Self::ask_for_the_whole_file`]) and never
    /// cleared, and *never cleared* is the load-bearing half. The re-read a
    /// watcher asks for goes down the same lane the first read went down
    /// ([`Self::claim_head_read`]), so a buffer that forgot this would answer an
    /// external change by replacing the whole document it is being edited in
    /// with the first 64KB of it and going read-only under the reader's hands.
    reads_whole: bool,
    /// **This file is past [`PREVIEW_EDIT_BYTES`]** — [`HeadOutcome::TooLargeToEdit`]
    /// filed.
    ///
    /// The bit that stops [`Self::reads_whole`] from asking for ever: the whole
    /// read came back saying the file is too large, so the head on the glass
    /// stands, the buffer stays read-only, and no further read is owed. It is a
    /// fact about the file rather than about the reading, so like
    /// [`Self::lossy`] it is answered by [`Self::read_only_notice`].
    too_large_to_edit: bool,
    /// **What this body has been through** — the undo log (ticket T3,
    /// 2026-09-10; [`crate::preview_undo`]).
    ///
    /// **On the buffer and not on the pane**, which is research §9.2's ruling and
    /// this file's own: a file open in two panes is one buffer, so a history kept
    /// beside a pane's caret would fork the one thing the pool exists to keep
    /// unforked. An undo pressed in either pane therefore takes back the buffer's
    /// last change, whichever pane made it, and the caret it restores is the one
    /// the pane that made it was using — the other pane's caret is clamped into
    /// range the next time it is used and otherwise left alone, exactly as it
    /// already is when the *other* pane types.
    ///
    /// Not persisted, and emptied whenever the body is replaced by the disk's:
    /// every offset in it names a place in the body it was recorded against.
    pub undo: crate::preview_undo::UndoLog,
}

impl PreviewBuffer {
    /// A buffer for this source, with everything answerable without a disk
    /// already answered.
    ///
    /// **`ftype` stays the name's judgement for every source**, and that is the
    /// point of asking it of the name in the first place (see [`preview_ftype`]):
    /// a git diff of `main.rs` is named `main.rs` and is text for exactly the
    /// reasons the file is. What the source decides is the *load* — whether
    /// there is a disk to go to — and, at [`Self::view`], which body is drawn.
    pub fn new(source: PreviewSource, name: String) -> Self {
        // **The name's judgement for every source that has a name to judge, and
        // the source's for the one that has not.** A git diff of `main.rs` is
        // named `main.rs` and is text for exactly the reasons the file is; a page
        // is named by its *title*, which is a sentence somebody wrote, and a
        // title ending `.md` is not a markdown document (see
        // [`PreviewFtype::Web`]).
        let ftype = match &source {
            PreviewSource::Web(_) => PreviewFtype::Web,
            PreviewSource::File(_)
            | PreviewSource::GitDiff { .. }
            | PreviewSource::GitShow { .. }
            | PreviewSource::GitDiffRange { .. }
            | PreviewSource::GitGraph { .. } => preview_ftype(&name),
        };
        let load = match &source {
            PreviewSource::File(path) => {
                if !is_readable_unasked(path) {
                    PreviewLoad::Refused(PreviewRefusal::NetworkPath)
                } else {
                    match ftype {
                        PreviewFtype::Text | PreviewFtype::Markdown | PreviewFtype::Table => {
                            PreviewLoad::Pending
                        }
                        // A picture's pixels come down the decode lane that
                        // already exists, so its buffer is complete the moment
                        // it is made.
                        // A video's frame comes down that same decode lane, so
                        // its buffer is complete on the same terms — see
                        // [`PreviewFtype::Video`]. It is `Ready` rather than
                        // `Refused(Type)` because the type is no longer the
                        // problem: this window has a face for it, and a buffer
                        // that said otherwise would be the 2026-08-23 defect
                        // over again with a different extension.
                        PreviewFtype::Image | PreviewFtype::Video => PreviewLoad::Ready,
                        // **A page-named file in the document pool is a page the
                        // page lane could not open**, and its card must say
                        // which disk answer that was — not `Refused(Type)`,
                        // which is a sentence about the file's *kind* when the
                        // kind was never the problem (user ruling 2026-08-23;
                        // §7.10 ⑥ is the account this pays).
                        //
                        // Every door that lands a source goes through
                        // `Runtime::open_preview_source_on`, which turns a name
                        // that says page back onto the engine's lane, so the
                        // only way here is `Runtime::open_preview_web_file`
                        // having asked the disk and been refused — and that door
                        // files the real refusal on this buffer in the same
                        // breath it lands it. `Pending` is what it lands in
                        // until then, and no read is ever asked for it: see
                        // [`Self::wants_head_read`], which has never listed
                        // `Web` and must not start — this window does not read a
                        // page as text under any name.
                        PreviewFtype::Web => PreviewLoad::Pending,
                        // **A name nobody listed waits for its own bytes** (user
                        // ruling 2026-08-27; §7.32). It used to be refused here,
                        // on the name alone, and that is the whole of the
                        // reported defect: `.ps1` is not in any table this
                        // window keeps, so a PowerShell script — the one kind of
                        // file a Windows terminal is most likely to be looking
                        // at — got the card that says nothing in this window
                        // reads it. What actually decides is [`read_head`]'s
                        // [`head_reads_as_text`], and `Pending` is how a buffer
                        // says the answer is on its way. If the bytes say no,
                        // [`Self::accept`] files exactly the refusal that used
                        // to be filed here.
                        PreviewFtype::Unknown => PreviewLoad::Pending,
                    }
                }
            }
            // Git-backed content is composed rather than read: nothing about it
            // is answerable without asking a repository, so it waits in the same
            // `Pending` a head read waits in — but for the git worker G-1 builds,
            // never for [`PreviewWorker`], which reads disks (see
            // [`Self::wants_head_read`]).
            PreviewSource::GitDiff { .. }
            | PreviewSource::GitShow { .. }
            | PreviewSource::GitDiffRange { .. } => PreviewLoad::Pending,
            // **The graph has no body and never waits for one** (G-4). Its two
            // siblings are documents whose text arrives from a subprocess, and
            // `Pending` is what says the text is on its way; the graph's content
            // is the picture the chrome draws over this pane, and there is no
            // second thing coming. Left `Pending` it would sit under a
            // "Loading …" line forever, which was exactly what the first real
            // frame of it showed.
            PreviewSource::GitGraph { .. } => PreviewLoad::Ready,
            // **A page is complete the moment it is made**, on the graph's own
            // sentence one lane over: `Pending` says text is on its way, and for
            // a page nothing is — the pixels are the engine's and arrive through
            // the composition tree, not through this crate. Left `Pending` it
            // would sit under a "Loading …" line for ever.
            PreviewSource::Web(_) => PreviewLoad::Ready,
        };
        Self {
            source,
            name,
            ftype,
            content: None,
            truncated: false,
            dirty: false,
            revision: 0,
            edited_here: false,
            disk_mtime: None,
            load,
            head_asked: false,
            stale: false,
            disk: DiskNews::Level,
            max_columns: 0,
            content_says_text: false,
            encoding: HeadEncoding::Utf8,
            lossy: false,
            reads_whole: false,
            too_large_to_edit: false,
            undo: crate::preview_undo::UndoLog::default(),
        }
    }

    /// **This buffer's file was renamed** — take the new name, and re-ask what
    /// it means without throwing away what the bytes already said (user ruling
    /// 2026-08-27; `docs/DESIGN.md` §7.32).
    ///
    /// The suffix is what the body is drawn from, so a rename that changes it
    /// changes the view: `notes.md` to `notes.txt` really does turn a rendered
    /// document into a text editor, which is the filesystem's answer and not
    /// ours to soften.
    ///
    /// **What the name cannot take back is the sniff.** A file this window read
    /// and found to be text is text; renaming it to something no table lists
    /// does not make its bytes unreadable, and a pane that answered "no preview
    /// for this file type" about the document it was showing one keystroke
    /// earlier would be this window forgetting an answer it already has. So the
    /// name's judgement is asked first and `Unknown` — and only `Unknown` — is
    /// overruled by the evidence.
    pub fn rename(&mut self, name: String) {
        self.name = name;
        self.ftype = match preview_ftype(&self.name) {
            PreviewFtype::Unknown if self.content_says_text => PreviewFtype::Text,
            named => named,
        };
    }

    /// **The buffer a glance takes over a file** (user ruling 2026-08-25;
    /// `docs/DESIGN.md` §7.10 ⑥).
    ///
    /// [`Self::new`] with one difference, and the difference is the ruling: a
    /// file whose name says page and whose **bytes are text**
    /// ([`PageGlance::Source`] — `.html`, `.htm`) is a text buffer here. Not a
    /// second lane for it, not a second reader: the same `Text` ftype an `.rs`
    /// file has, so the same head read fetches it, the same [`preview_view`]
    /// draws it and the same 64KB cap applies.
    ///
    /// **Why the glance and not the pane** — *and why the pane now asks too.*
    /// The pane opens a page *as a page*:
    /// [`crate::Runtime::open_preview_source_on`] turns a page-named path onto
    /// the engine's lane before a document buffer is ever made, so a pane that
    /// held one of these **by that door** would be the page lane having failed,
    /// and the refusal it files is the sentence that seat owes. That is still
    /// true and nothing below has changed it. What the 2026-08-26 ruling added
    /// is a **second door with a hand on it**: the `</>` on a local page's
    /// address row, which is not the page lane failing but a reader asking for
    /// the other face of a file the seat is already standing on. The card has no
    /// engine and never will; the pane has one and keeps it — see
    /// [`Self::read_a_pages_bytes_as_text`], which both doors go through.
    ///
    /// **The chip still says `web`.** What the head prints is the *name's*
    /// judgement ([`preview_ftype`], asked by `Runtime::file_peek_subject`), and
    /// this is a judgement about which reader the glance uses. The two are
    /// different questions and the card is where they meet: `web` in the corner,
    /// the markup underneath.
    ///
    /// Only a file's own buffer is touched. A git-composed document named
    /// `index.html` is a reading of a repository — it has no disk to read and
    /// [`Self::view`] draws it as a diff whatever its ftype says — so it is left
    /// exactly as [`Self::new`] made it.
    pub fn glancing(source: PreviewSource, name: String) -> Self {
        let mut buffer = Self::new(source, name);
        buffer.read_a_pages_bytes_as_text();
        buffer
    }

    /// **A page whose bytes are text is read as text** — the rule itself, with
    /// no opinion about who asked for it.
    ///
    /// It was the whole body of [`Self::glancing`] until the pane grew a second
    /// reader (user ruling 2026-08-26: the `</>` on a local page's address row).
    /// A hover and a press now ask the same question of the same file, and the
    /// answer is one sentence in one place — two copies of it are two lanes that
    /// come apart the first time [`PAGE_EXTENSIONS`] grows a row, which is the
    /// lesson §7.10 ⑥ was written out of.
    ///
    /// **`Pending` is the guard and not a formality.** It is the load a
    /// page-named file lands in and the one a text file waits in, so asking it
    /// here keeps a network share's `Refused(NetworkPath)` card exactly where it
    /// was: this promotes a buffer that was going to sit empty, and never one
    /// that already has its answer.
    ///
    /// Idempotent, because the pane's door may run it over a buffer the pool
    /// already holds: a second call finds `PreviewFtype::Text` and changes
    /// nothing.
    pub fn read_a_pages_bytes_as_text(&mut self) {
        let reads_as_text = match &self.source {
            PreviewSource::File(path) => path_page_glance(path) == Some(PageGlance::Source),
            _ => false,
        };
        if reads_as_text && self.load == PreviewLoad::Pending {
            self.ftype = PreviewFtype::Text;
        }
    }

    /// Whether this buffer is still waiting on a head read.
    ///
    /// **A head read is a question for a disk**, so it is asked only of a source
    /// that has one. This is the gate that keeps a git-backed buffer off
    /// [`PreviewWorker`]'s lane entirely rather than letting it arrive there and
    /// be dropped.
    /// **And whether one is still owed.** A read already out with the worker is
    /// not a read to ask for — see [`Self::head_asked`].
    ///
    /// **`Unknown` is on this lane since 2026-08-27** (user ruling; §7.32), and
    /// it is the whole of how a name nobody listed gets an answer: the sniff is
    /// a question about bytes, bytes come off a disk, and the disk is this
    /// worker's. Nothing new reads a file — the read that was already the
    /// preview's one trip is the read the verdict comes back on.
    /// **And a body that is only the head of a file somebody has asked to edit
    /// is owed the rest of it** (T2 ③, 2026-09-10). The third clause is what
    /// makes the two-stage read converge rather than stall: the read the flip
    /// files is a `Whole` one, but a head read already out with the worker can
    /// land after it and put the first 64KB back, and this is the line that
    /// notices and asks again. [`Self::too_large_to_edit`] is what stops it,
    /// because past the editing cap the head is the honest final answer.
    pub fn wants_head_read(&self) -> bool {
        self.source.file_path().is_some()
            && (self.load == PreviewLoad::Pending
                || self.stale
                || (self.reads_whole && self.truncated && !self.too_large_to_edit))
            && !self.head_asked
            && matches!(
                self.ftype,
                PreviewFtype::Text
                    | PreviewFtype::Markdown
                    | PreviewFtype::Table
                    | PreviewFtype::Unknown
            )
    }

    /// **The reader has asked to edit this file, so the next read is the whole
    /// of it** (T2 ③, owner's ruling on research §10 Q2, 2026-09-10).
    ///
    /// The one door onto [`Self::reads_whole`], and the whole of what "asking to
    /// edit buys a whole-file read" means on this side. Answers whether a read
    /// is now owed, so that the caller can put the question on the worker's lane
    /// through [`Self::claim_head_read`] exactly as every other read goes.
    ///
    /// **The two gestures that call it, and why they are the two** (T2's own
    /// choice, written down here because it is the sort of thing a later reader
    /// has to be able to find):
    ///
    /// 1. **The flip to the source face of a Markdown buffer.** The rendered
    ///    page has nothing to type into and its source has, so the flip *is* the
    ///    asking.
    /// 2. **A press inside the body of a surface whose face edits** — a text
    ///    file, or a Markdown file already flipped. A reader who has just put
    ///    the pointer in a document has said what they intend.
    ///
    /// They are the two places a *person* asks, and that is the whole of the
    /// choice: every other consultation of [`Self::is_editable`] is a frame
    /// drawing itself — a head button, a foot notice, a shortcut table — and a
    /// disk read on that beat is sixty a second. A glance, a hover card and a
    /// focus card therefore still cost exactly one head read, which is the whole
    /// point of the head read.
    ///
    /// **Nothing is asked for a body that could not be edited anyway**: a lossy
    /// decode, a file past the editing cap, a face with no caret. A read whose
    /// answer changes nothing is a trip to a disk for nothing. `md_source` is
    /// the *view's*, exactly as [`Self::is_editable`] takes it and for that
    /// method's own recorded reason — a press inside a rendered Markdown page is
    /// not somebody asking to edit, and the flip is what turns that page into a
    /// surface with a caret.
    pub fn ask_for_the_whole_file(&mut self, md_source: bool) -> bool {
        if self.reads_whole
            || !self.truncated
            || self.lossy
            || self.too_large_to_edit
            || self.source.file_path().is_none()
            || !is_editable(&self.name, self.ftype, md_source)
        {
            return false;
        }
        self.reads_whole = true;
        // A truncated buffer is read-only and therefore cannot be dirty, so
        // `mark_stale`'s refusals are all about kinds this one has already
        // passed — but it is still the door onto the bit, and the quiet re-read
        // it arms is exactly the one wanted here: the head on the glass stays
        // until the whole document lands on top of it.
        self.mark_stale()
    }

    /// **A whole-file read is out and the body it will replace is still the
    /// head** (T5 ①, 2026-09-10).
    ///
    /// The one question a gesture has to ask between the press that bought the
    /// file and the frame the file lands on: this buffer is not editable *yet*
    /// and the only thing standing between it and editable is a read already on
    /// the worker. A press that gets this answer keeps its caret on the pane
    /// until the body arrives ([`Runtime::settle_preview_caret`]) instead of
    /// making the reader click a second time.
    ///
    /// Both halves are needed and neither is the other. `reads_whole` alone is
    /// true for ever after the first ask, including long after the file landed;
    /// `truncated` alone is true for every head nobody has asked about.
    #[must_use]
    pub fn awaits_the_whole_file(&self) -> bool {
        self.reads_whole && self.truncated
    }

    /// **Which of the two reads this buffer is owed** (T2 ③, 2026-09-10).
    ///
    /// [`Self::claim_head_read`]'s answer, asked without taking the read — for
    /// the one caller that has to name the question before it can file it, the
    /// revived tab's walk over its own panes. A buffer that has bought the whole
    /// file is owed the whole file every time afterwards, including the read
    /// that brings a restored tab back: reviving a document somebody is editing
    /// as the first 64KB of itself would take the caret away and the ceiling
    /// back.
    #[must_use]
    pub fn read_want(&self) -> PreviewWant {
        if self.reads_whole {
            PreviewWant::Whole
        } else {
            PreviewWant::Head
        }
    }

    /// **This body is behind its file** — [`Self::stale`] read from outside.
    ///
    /// The one caller is `Runtime::request_stale_previews`, which walks a pool
    /// rather than a set of panes and therefore cannot use `wants_head_read`:
    /// that predicate answers `true` for a buffer that has never been read at
    /// all, and a pool full of those is exactly the eight-files-to-show-one read
    /// the restore door refuses to make.
    #[must_use]
    pub fn is_behind_the_disk(&self) -> bool {
        self.stale
    }

    /// Whether a read of this buffer's head is **out with the worker right now**.
    ///
    /// [`Self::wants_head_read`]'s other side, and the one `BT_PREVIEW_TRACE`'s
    /// build station prints: a buffer holding no text because its answer is still
    /// on the disk and a buffer holding no text because its answer was taken by
    /// somebody else and dropped look identical from the glass, and this is the
    /// bit that tells them apart (user report, 2026-08-23).
    #[must_use]
    pub fn awaiting_head_read(&self) -> bool {
        self.head_asked
    }

    /// **Take this buffer's head read** — [`Self::wants_head_read`] and the
    /// filing of the question, in one breath.
    ///
    /// The one door onto [`Self::head_asked`], for the reason
    /// [`PreviewPool::open`] is one door: a caller that asked and forgot to file
    /// it is a file read again on the next frame, and a caller that filed
    /// without asking is a document that never arrives. Every send on
    /// [`PreviewWorker`]'s channel comes through here, so "one document, one
    /// read" is true by construction rather than by five call sites agreeing.
    ///
    /// **And it says which read it took** (T2 ③, 2026-09-10). Since a buffer can
    /// be owed either the head or the whole file, the answer is the
    /// [`PreviewWant`] to send rather than a `bool` the caller then pairs with a
    /// want of its own: a call site that filed the question and then asked for
    /// the first 64KB of a document being edited would be a silent truncation,
    /// and this shape makes that unspellable.
    ///
    /// The lane keeps the word *head* in its name — here, in
    /// [`Self::wants_head_read`] and in [`Self::head_asked`] — because it is the
    /// same one question about the same file with the same ledger in front of
    /// it. Only how much comes back has changed.
    #[must_use]
    pub fn claim_head_read(&mut self) -> Option<PreviewWant> {
        if !self.wants_head_read() {
            return None;
        }
        self.head_asked = true;
        Some(self.read_want())
    }

    /// **Give up on the read that is still out** (F1b, `plan.md` v4 增补 ②).
    ///
    /// The pane showing this buffer has been given a new [`crate::LeafId`] — the
    /// tab is not the tab it asked from — so the answer is addressed to a pool
    /// that no longer holds it and is dropped where it lands. Nothing asks twice
    /// while `head_asked` stands, so this is the bit that lets the next frame ask
    /// again; without it the pane draws its head, its foot and the two hairlines
    /// between them for the rest of the session, which is exactly the 2026-08-23
    /// report one gesture over.
    pub fn forget_head_read(&mut self) {
        self.head_asked = false;
    }

    /// **The file behind this buffer was written by somebody else** (W2 slice
    /// 5) - ask the disk again, without taking down what is on the glass.
    ///
    /// The one door onto [`Self::stale`], on [`Self::claim_head_read`]'s own
    /// reasoning: the bit is the whole of "this body is behind the disk", and a
    /// caller that set it without meaning it is a file read on a beat.
    ///
    /// **A buffer with unsaved edits is not re-read, and that is a ruling.**
    /// The person's text is the newer of the two, and a watcher that overwrote
    /// it would make an editor's save in another window destroy work in this
    /// one. The disagreement itself is not lost: it is what ruling 8-9's
    /// `disk_mtime` check reports at the moment of saving, which is the moment
    /// somebody can answer it.
    ///
    /// Answers whether anything was owed, so that a caller can tell a file that
    /// moved from a file that moved under something with nothing to re-read -
    /// a picture, whose pixels come down the decode lane, and a git-backed
    /// document, which has no disk to ask.
    pub fn mark_stale(&mut self) -> bool {
        if self.dirty || self.stale || self.source.file_path().is_none() {
            return false;
        }
        if !matches!(
            self.ftype,
            PreviewFtype::Text | PreviewFtype::Markdown | PreviewFtype::Table
        ) {
            return false;
        }
        self.stale = true;
        true
    }

    /// **The watcher says this file moved** — the one door onto [`Self::disk`]
    /// (user ruling 2026-08-29).
    ///
    /// `present` is what the disk answered at the moment the stamp was compared
    /// (`preview_watch::FileNews`), carried in rather than asked again here: a
    /// file deleted and recreated between the two calls would give a second
    /// answer that does not describe the notification being answered.
    ///
    /// The ruling's three cases, in the order they are decided:
    ///
    /// 1. **The file is gone.** The buffer is *kept* — the reader's document
    ///    does not evaporate because something outside this window removed the
    ///    file it came from — and the strip says so. Nothing is read: a read
    ///    would answer `Refused(Fault)` and put "no such file" where a document
    ///    is standing, which is the outcome this case exists to avoid.
    /// 2. **The file moved under unsaved edits.** Not overwritten, and that is
    ///    [`Self::mark_stale`]'s standing ruling; what is new is that the
    ///    disagreement is *said* at the moment it happens rather than kept until
    ///    somebody presses save. The verbs on the strip are the two answers a
    ///    person can give it.
    /// 3. **The file moved under a clean body.** Re-read, quietly, exactly as
    ///    before — there is no disagreement to report, only a document that is
    ///    now behind its file.
    #[must_use]
    pub fn note_disk_moved(&mut self, present: bool) -> DiskVerdict {
        if self.source.file_path().is_none() {
            return DiskVerdict::Nothing;
        }
        if !matches!(
            self.ftype,
            PreviewFtype::Text | PreviewFtype::Markdown | PreviewFtype::Table
        ) {
            return DiskVerdict::Nothing;
        }
        if !present {
            return DiskVerdict::from_said(self.say(DiskNews::Deleted));
        }
        if self.dirty {
            return DiskVerdict::from_said(self.say(DiskNews::Changed));
        }
        // The file is there and this body has nothing of its own to lose, so the
        // sentence — if one was standing, from a delete that has been undone —
        // comes down and the bytes are asked for.
        let said = self.say(DiskNews::Level);
        if self.mark_stale() {
            DiskVerdict::ReadAgain
        } else {
            DiskVerdict::from_said(said)
        }
    }

    /// Put a sentence up, take one down, or leave the standing one alone.
    /// Answers whether the glass owes a frame.
    fn say(&mut self, news: DiskNews) -> bool {
        std::mem::replace(&mut self.disk, news) != news
    }

    /// **Take the disk's copy** — the strip's `Reload` (user ruling 2026-08-29).
    ///
    /// The one door in this product that discards an unsaved edit without being
    /// asked twice, and it is allowed to be that because it *is* the second
    /// asking: the strip that carries it exists only while this window is
    /// telling the reader their copy and the file's have parted, and the other
    /// verb beside it keeps the edits. Answers whether a head read is now owed.
    pub fn take_the_disks_copy(&mut self) -> bool {
        if self.source.file_path().is_none() {
            return false;
        }
        self.disk = DiskNews::Level;
        self.dirty = false;
        // The edits this discards are not edits an undo may reach back into: the
        // body they were recorded against is about to be replaced by the file's.
        self.undo.forget();
        self.mark_stale()
    }

    /// **Keep this body** — the strip's other verb, and its `×` (user ruling
    /// 2026-08-29).
    ///
    /// Nothing is read and nothing is written: the sentence goes down and the
    /// edits stand. The disagreement itself is not forgotten by the *product* —
    /// [`Self::save`] still compares `disk_mtime` and still answers
    /// [`SaveOutcome::Conflict`], which is ruling 8-9 and the thing that stops a
    /// dismissal here from becoming an overwrite later.
    ///
    /// Answers whether the glass owes a frame.
    pub fn keep_this_body(&mut self) -> bool {
        self.say(DiskNews::Level)
    }

    /// Whether this buffer would be shown on a surface that edits, **as the
    /// surface asking is showing it**.
    ///
    /// The name's judgement ([`is_editable`]) **and two facts only a body
    /// knows**. A buffer with no body has nothing to put a caret in; a
    /// *truncated* one has only the first 64KB of its file, and an edit surface
    /// over the head of a file is a save button wired to `truncate`. §7.1.3's
    /// "超大文件只读降级" is exactly this line — the degradation is read-only,
    /// and read-only has to be enforced where the editing is, not where the
    /// notice is printed.
    ///
    /// `md_source` is the *view's*, not the buffer's, and that is the 2026-08-13
    /// ruling: a rendered markdown page has nothing to type into and its source
    /// has, so whether this file is editable right now is a question about the
    /// surface looking at it. Two surfaces on one markdown file can answer it
    /// differently at the same moment, and both are right.
    ///
    /// **And one fact only the source knows**: an edit surface exists to write
    /// bytes back, and a document with no file behind it has nowhere to write
    /// them. A git diff is a reading of a repository, not a second place to type
    /// into it.
    /// **And one fact only the decode knows** (T2 ②, 2026-09-10): a body that
    /// came back with characters this window invented is not a body it will
    /// write. `truncated` is the same sentence about a different half of the
    /// file — what is missing off the end — and since T2 it is answerable: the
    /// reader asks to edit, [`Self::ask_for_the_whole_file`] buys the rest, and
    /// the clause below stops refusing on its own. What it never stops refusing
    /// is a file past [`PREVIEW_EDIT_BYTES`], where the head is all there will
    /// ever be.
    pub fn is_editable(&self, md_source: bool) -> bool {
        self.source.file_path().is_some()
            && self.load == PreviewLoad::Ready
            && self.content.is_some()
            && !self.truncated
            && !self.lossy
            && is_editable(&self.name, self.ftype, md_source)
    }

    /// Hand the body to an edit, and file everything one implies.
    ///
    /// **One door**, for the reason [`PreviewPool::open`] is one door: an edit
    /// owes three things — the dirty bit, the revision every cache is keyed on,
    /// and the widest line the horizontal scroller is derived from — and three
    /// call sites each remembering all three is three chances to forget one.
    /// The closure reports whether anything actually changed, so an insert of
    /// nothing does not dirty a file.
    ///
    /// **Compiled for the tests alone since ticket T3**, and that is a statement
    /// about this window rather than about this method: every edit a body
    /// actually receives comes from the keyboard and carries a caret, so the one
    /// production door is [`Self::edit_by_caret`] and this is the shape the
    /// assertions poke bytes through. The caret it files is the one the change
    /// implies (`preview_undo::Change::implied`); the day a verb edits a document
    /// without a hand on it — a formatter, a rename across a file — this is the
    /// door it comes through and the `#[cfg(test)]` comes off.
    #[cfg(test)]
    pub fn edit_content(&mut self, edit: impl FnOnce(&mut String) -> bool) -> bool {
        let Some(content) = self.content.as_mut() else {
            return false;
        };
        let was = content.clone();
        if !edit(content) {
            return false;
        }
        let change = crate::preview_undo::Change::implied(&was, content);
        self.file_the_edit(change);
        true
    }

    /// **Whether the body this buffer is holding was typed here** — see
    /// [`Self::edited_here`].
    ///
    /// A reader rather than a public field, because it is written in exactly
    /// three places and every one of them is a door in this file: the two the
    /// disk arrives through and the one every change of ours settles in.
    pub fn was_edited_here(&self) -> bool {
        self.edited_here
    }

    /// **The keyboard's own door** — [`Self::edit_content`] with the caret that
    /// made the edit (ticket T3).
    ///
    /// The caret is the *view's* (ruling 8⑧) and the log is the buffer's, which
    /// is why the two arrive here from different places and are filed together:
    /// an entry has to remember where the caret stood before the keystroke and
    /// where it ended up, or an undo can put the bytes back and not the hand.
    ///
    /// **The change is worked out from the bytes** and not reported by the
    /// closure. This door takes a closure precisely because a keystroke's effect
    /// is the closure's to decide, so a closure that also described its own edit
    /// would be a second account of it — and the two accounts would part company
    /// the first time somebody wrote a third kind of edit. Comparing costs a copy
    /// of the body per keystroke, which is a memory move next to the
    /// whole-document re-parse already standing beside it.
    pub fn edit_by_caret(
        &mut self,
        caret: &mut crate::preview_edit::EditCaret,
        edit: impl FnOnce(&mut String, &mut crate::preview_edit::EditCaret) -> bool,
    ) -> bool {
        let Some(content) = self.content.as_mut() else {
            return false;
        };
        let before = *caret;
        let was = content.clone();
        if !edit(content, caret) {
            return false;
        }
        let change = crate::preview_undo::Change::between(&was, content, before, *caret);
        self.file_the_edit(change);
        true
    }

    /// **Take back the buffer's last change**, and answer with the caret of
    /// whoever made it (ticket T3).
    ///
    /// `None` when there is nothing left to take back, which is a press with
    /// nothing to say rather than a failure — the same silence [`Self::save`]
    /// keeps over a clean buffer.
    pub fn undo_edit(&mut self) -> Option<crate::preview_edit::EditCaret> {
        let content = self.content.as_mut()?;
        let caret = self.undo.undo(content)?;
        self.settle_after_a_change();
        Some(caret)
    }

    /// The same, forwards.
    pub fn redo_edit(&mut self) -> Option<crate::preview_edit::EditCaret> {
        let content = self.content.as_mut()?;
        let caret = self.undo.redo(content)?;
        self.settle_after_a_change();
        Some(caret)
    }

    /// File a change and everything it implies.
    fn file_the_edit(&mut self, change: Option<crate::preview_undo::Change>) {
        // **A change that moved no bytes is not filed**, which is the same
        // sentence the `if !edit(content)` guard above says one level up: an
        // insert that replaced a selection with the very text it already held
        // reports `true` and has changed nothing a reader or a disk could see.
        // The revision still moves, because a cache keyed on it was invalidated
        // by the asking and re-deriving is cheap beside being wrong.
        if let Some(change) = change {
            self.undo.record(change);
        }
        self.settle_after_a_change();
    }

    /// What every move of the body owes, whichever direction it went in.
    ///
    /// **One place**, for [`Self::edit_content`]'s own reason: an edit owes the
    /// widest line the horizontal scroller is derived from, the revision every
    /// cache is keyed on, and the dirty bit — and an undo owes exactly the same
    /// three, because an undo is a change to the body like any other.
    fn settle_after_a_change(&mut self) {
        self.max_columns = widest_line_columns(self.content.as_deref().unwrap_or_default());
        self.revision += 1;
        self.dirty = self.undo.is_dirty();
        // Every road into here is a hand on this window's keyboard — a
        // keystroke, an undo, a redo — which is the whole of what
        // [`Self::edited_here`] means. The disk's two doors ([`Self::accept`],
        // [`Self::decline`]) move the revision without coming through here, and
        // each of them says so on its own line.
        self.edited_here = true;
    }

    /// Write the body back to its file.
    ///
    /// Three refusals and one write, in this order and for this reason:
    ///
    /// * A buffer with no body has nothing to write, and writing "nothing"
    ///   would empty the file a failed read was about. **A buffer with no
    ///   *file*** — a git-backed document — is the same refusal one level up:
    ///   there is no path a save could name, which is why it is asked first.
    /// * A body the disk has moved on from is [`SaveOutcome::Conflict`]
    ///   (ruling 8⑨). **Not a prompt and not a blind write** — this slice's
    ///   minimum is that the window says so and keeps the edits, because the
    ///   one unrecoverable outcome is overwriting a change nobody has seen.
    /// * The write itself is atomic ([`bt_persist::atomic_write`]).
    ///
    /// **In the encoding the file was read in, mark included** (T2 ①,
    /// 2026-09-10). See [`Self::encoding`] for the defect this pays: the body is
    /// a `String` and a `String` remembers nothing, so a save that reached for
    /// `as_bytes` rewrote every marked and every UTF-16 file it touched.
    ///
    /// **And through one atomic writer** (research §10 Q14). There were two
    /// implementations of the same temp-sibling-fsync-rename algorithm — this
    /// module's and `bt-persist`'s — and `bt-persist`'s is the one that stays:
    /// it is not welded to the config directory (its staging path is derived
    /// from the target's own parent) and it already writes user files elsewhere
    /// in this window, the PowerShell profile among them. One algorithm, one
    /// place to fix.
    ///
    /// **Two gaps stay open and are named rather than papered over**: neither
    /// writer clears a read-only or hidden attribute on the target, and neither
    /// asks whether the target is a symlink — a rename replaces the link, not
    /// what it points at, which is the question the read side asks with
    /// `may_read_unasked_through_links` and the write side still does not.
    ///
    /// The mtime is re-read from the file that was just written rather than
    /// remembered from the write, so the next save compares against what the
    /// filesystem actually recorded.
    pub fn save(&mut self) -> SaveOutcome {
        let Some(path) = self.source.file_path().map(Path::to_path_buf) else {
            return SaveOutcome::Failed("there is no file behind this view".to_owned());
        };
        let Some(content) = self.content.as_deref() else {
            return SaveOutcome::Failed(crate::i18n::Text::PreviewNothingToSave.text().to_owned());
        };
        if file_mtime(&path) != self.disk_mtime {
            return SaveOutcome::Conflict;
        }
        if let Err(error) = bt_persist::atomic_write(&path, &self.encoding.encode(content)) {
            return SaveOutcome::Failed(error.to_string());
        }
        self.disk_mtime = file_mtime(&path);
        // **Where the file now stands in this body's history** (ticket T3). The
        // dirty bit is read off the log rather than set beside it, so a save and
        // an undo back to a save cannot come to disagree about what clean means;
        // and the run is closed, so the next keystroke starts an entry of its own
        // instead of joining one the reader has already watched being written.
        self.undo.mark_saved();
        self.dirty = self.undo.is_dirty();
        SaveOutcome::Saved
    }

    /// **The sentence a body that cannot be edited owes its reader**, if it owes
    /// one.
    ///
    /// §7.1.3's "超大文件只读降级": the degradation is not that the file failed,
    /// it is that what is on screen is the beginning of it — and a preview that
    /// showed the first 64KB without saying so would be a preview quietly
    /// claiming the file ends there.
    ///
    /// **It was `read_only_notice` until 2026-09-10**, when T2 gave the reader
    /// two more ways to be told the same thing, and one channel is the ruling
    /// here: a refused edit is explained in the right hand of the pane's foot
    /// and nowhere else, so a second notice surface for "this file decoded
    /// lossily" would be a second place to look for one kind of answer.
    ///
    /// The order is the order a reader can act on. **Lossy first**, because it
    /// is a fact about bytes and no amount of reading more of them changes it.
    /// **Then the editing cap**, which is why a truncated body is staying
    /// truncated. **Then truncation itself**, which since T2 is the temporary
    /// one — the head of a file nobody has asked to edit yet.
    pub fn read_only_notice(&self) -> Option<&'static str> {
        if self.lossy {
            return Some(preview_lossy_notice());
        }
        if self.too_large_to_edit {
            return Some(preview_too_large_notice());
        }
        self.truncated.then_some(preview_truncated_notice())
    }

    /// Which body this buffer is drawn as **on the surface asking**.
    ///
    /// Parameterised for [`Self::is_editable`]'s reason: the flip is the view's,
    /// so a markdown file is a rendered page in one pane and a text surface in
    /// another at the same instant.
    ///
    /// **The source is asked before the name** (R24, 2026-08-15). A file's body
    /// is still decided by [`preview_view`]'s ladder, extension and all — that
    /// ladder is about what a *file* is and nothing here changes it, including
    /// the `.diff`/`.patch` suffix that earns a real patch file its diff view.
    /// What the mock-up did instead was hand a git diff a *display name* ending
    /// in `.diff` so that the same suffix rule would sweep it into the diff view
    /// by accident, and that is the mechanism this branch retires: a git diff is
    /// a diff because of what it *is*, stated here, and never because of how it
    /// happens to be spelled.
    pub fn view(&self, md_source: bool) -> PreviewView {
        match &self.source {
            PreviewSource::File(_) => preview_view(&self.name, self.ftype, md_source),
            // **A page is a page because of what it is**, which is this method's
            // own standing rule said about the fourth kind of content: the
            // extension ladder is about what a *file* is, and a page has no file
            // and no source view to flip to.
            PreviewSource::Web(_) => PreviewView::Web,
            PreviewSource::GitDiff { .. }
            | PreviewSource::GitShow { .. }
            | PreviewSource::GitDiffRange { .. } => PreviewView::Diff,
            // G-4's full graph: its own surface, drawn as chrome over this
            // pane's body. See [`PreviewView::Graph`].
            PreviewSource::GitGraph { .. } => PreviewView::Graph,
        }
    }

    /// **The word a focus card's face says under this buffer's name**
    /// (`docs/DESIGN.md` §7.1.6b′ F2).
    ///
    /// A card's mini preview seat does not shrink the document — a page of prose
    /// at 7.5px is a grey smear, and v1 says so outright — so what the seat shows
    /// instead is the two facts that identify it: what it is showing, and what
    /// kind of thing that is. This is the second of those.
    ///
    /// **The source is asked before the name**, exactly as [`Self::view`] asks
    /// it and for that method's own recorded reason: a commit graph is a graph
    /// because of what it is. Everything else answers with its own extension in
    /// capitals — which is the honest type of a file and needs no table of words
    /// to keep translated — and a name with no extension at all falls through to
    /// the kind's own noun, the same one an empty preview head prints.
    #[must_use]
    pub fn kind_word(&self) -> String {
        if matches!(self.source, PreviewSource::GitGraph { .. }) {
            return crate::i18n::Text::GraphHeadingGraph.text().to_owned();
        }
        std::path::Path::new(&self.name)
            .extension()
            .and_then(|extension| extension.to_str())
            .filter(|extension| !extension.is_empty())
            .map(str::to_uppercase)
            .unwrap_or_else(|| crate::seats::seat_title(bt_layout::SeatKind::Preview).to_owned())
    }

    /// Why there is no body to show, when there is none.
    ///
    /// **The card's question**, and [`PreviewLoad::Unavailable`] is deliberately
    /// not an answer to it: see that variant for why a composed document's
    /// refusal is a line and not a card.
    pub fn refusal(&self) -> Option<PreviewRefusal> {
        match self.load {
            PreviewLoad::Refused(refusal) => Some(refusal),
            PreviewLoad::Pending | PreviewLoad::Ready | PreviewLoad::Unavailable(_) => None,
        }
    }

    /// **The one line a body prints instead of itself** (G-3).
    ///
    /// Two cases and both of them are about composed content: a repository that
    /// would not answer, in git's own words, and a document that came back
    /// empty. Neither is a state a *file* can be in — an empty file is a file
    /// with nothing in it, and printing "No changes to show" over one would be
    /// a sentence about a repository laid across a zero-byte `.gitkeep`.
    ///
    /// The "Loading …" line is not here because it belongs to a different
    /// question: it is what a pane says while it is *waiting*, which a picture
    /// does too, and that lane already answers for both.
    pub fn body_notice(&self) -> Option<&str> {
        if let PreviewLoad::Unavailable(words) = &self.load {
            return Some(words);
        }
        // The graph is exempt for [`PreviewBuffer::new`]'s reason: a document
        // with no *text* is not a document with nothing in it when the thing in
        // it is a picture.
        (self.source.is_git()
            && !matches!(self.source, PreviewSource::GitGraph { .. })
            && self.load == PreviewLoad::Ready
            && self.content.as_ref().is_none_or(|body| body.is_empty()))
        .then_some(git_document_empty())
    }

    /// **The repository would not answer** (G-3) — git's sentence, kept whole.
    ///
    /// [`Self::accept`]'s opposite number for the git lane, and a separate door
    /// rather than a fifth [`HeadOutcome`] because `HeadOutcome` is what a
    /// *disk* answers: its refusals are a permission, a missing file, a NUL in
    /// the head. A repository's refusal is a sentence, and giving the disk lane
    /// a variant only the git lane can produce would be a case every reader of
    /// a head read has to be told never happens.
    pub fn decline(&mut self, words: String) {
        self.revision += 1;
        // Whatever body was here is the disk's to take away — see
        // [`Self::edited_here`].
        self.edited_here = false;
        // The question is closed by its answer, whichever lane answered it.
        self.head_asked = false;
        self.stale = false;
        // And the sentence: a landed answer is the disk's own word, whatever it
        // says, so nothing is left standing that claims the two have parted.
        self.disk = DiskNews::Level;
        self.content = None;
        self.truncated = false;
        // [`Self::accept`]'s refusal arm's own line: a buffer with no body
        // decoded nothing, so it says nothing about how.
        self.encoding = HeadEncoding::Utf8;
        self.lossy = false;
        self.max_columns = 0;
        self.disk_mtime = None;
        self.load = PreviewLoad::Unavailable(words);
    }

    /// File the worker's answer.
    pub fn accept(&mut self, outcome: HeadOutcome) {
        self.revision += 1;
        // **These bytes are the file's, not the reader's** — see
        // [`Self::edited_here`]. The same sentence the line below writes about
        // the undo log, about the other thing that cannot survive a body being
        // replaced: what a reader had marked is a claim about the text that is
        // going away.
        self.edited_here = false;
        // **A body arriving from a disk is a different body**, so the history of
        // the one it replaces goes with it (ticket T3). This is the door
        // [`Self::take_the_disks_copy`]'s own line ends at — the reload asks for
        // the read and the read lands here — and it is also every other way a new
        // body can arrive: a first read, a re-read after the file moved, a buffer
        // evicted and fetched again. Every offset in the log names a place in the
        // body that is being thrown away.
        self.undo.forget();
        // The question is closed by its answer — and the load it lands in
        // (`Ready`, `Refused`) is already not one this lane asks about, so
        // clearing the bit re-opens nothing. It keeps the bit meaning exactly
        // "a read is out", which is what a reader of it has to be able to
        // believe.
        self.head_asked = false;
        // And so is the watcher's: the body on the glass is the disk's again.
        self.stale = false;
        self.disk = DiskNews::Level;
        match outcome {
            HeadOutcome::Read {
                text,
                truncated,
                mtime,
                content_says_text,
                encoding,
                lossy,
            } => {
                self.content_says_text = content_says_text;
                // **The sniff, and the one place it is read** (user ruling
                // 2026-08-27; §7.32). A name in a table has already been
                // answered and this cannot touch it — `.txt` full of Latin-1 is
                // text because it is called `.txt`, and the strict verdict below
                // would say otherwise. Only a name that fell all the way to
                // `Unknown` asks the bytes, and only then is the answer here the
                // answer at all.
                if self.ftype == PreviewFtype::Unknown {
                    if !content_says_text {
                        // Exactly the refusal `PreviewBuffer::new` used to file
                        // on the name alone: nothing in this window reads this
                        // kind of file. It is the same card, reached by
                        // evidence instead of by a table.
                        self.content = None;
                        self.truncated = false;
                        self.max_columns = 0;
                        self.disk_mtime = None;
                        self.load = PreviewLoad::Refused(PreviewRefusal::Type);
                        return;
                    }
                    self.ftype = PreviewFtype::Text;
                }
                self.max_columns = widest_line_columns(&text);
                self.content = Some(text);
                self.truncated = truncated;
                // **Both facts about the decode land with the body they are
                // about** (T2, 2026-09-10). Re-filed rather than accumulated:
                // this is a *new* reading of the file, and a file rewritten in
                // another encoding, or rewritten as valid UTF-8, has to be able
                // to say so.
                self.encoding = encoding;
                self.lossy = lossy;
                self.disk_mtime = mtime;
                self.load = PreviewLoad::Ready;
            }
            HeadOutcome::Refused(refusal) => {
                // **A name that claimed nothing cannot be contradicted** (user
                // ruling 2026-08-27; §7.32). [`PreviewRefusal::Binary`]'s own
                // words are "the head held a NUL, *whatever the name claimed*" —
                // it is a sentence about a file that said it was text and was
                // not, and a name no table lists never said anything. So what it
                // gets is the card it has always got, and the two ways of
                // failing the sniff do not produce two different sentences about
                // one file.
                //
                // A **fault** is never turned: a disk saying no is news whatever
                // the file is called, and it is the one refusal here whose
                // subject is not the bytes.
                let refusal = match (self.ftype, refusal) {
                    (PreviewFtype::Unknown, PreviewRefusal::Binary) => PreviewRefusal::Type,
                    (_, refusal) => refusal,
                };
                self.content = None;
                self.truncated = false;
                // A buffer with no body has nothing that was decoded, so it says
                // nothing about an encoding either — `Utf8` is what a file with
                // no mark is, and therefore the only default that does not
                // invent a sentence this file never said.
                self.encoding = HeadEncoding::Utf8;
                self.lossy = false;
                self.max_columns = 0;
                self.disk_mtime = None;
                self.load = PreviewLoad::Refused(refusal);
            }
            // **The file read, and it is too big to take responsibility for**
            // (T2 ③). Nothing on the glass is replaced and nothing about the
            // body is re-filed: the head this buffer is already showing is the
            // right head, it is simply the last one there is going to be. What
            // changes is one bit and the sentence in the foot that hangs off it.
            //
            // The read that was out is closed by this answer exactly as the two
            // arms above close it, which is what keeps
            // [`Self::wants_head_read`]'s third clause from asking again for
            // ever.
            HeadOutcome::TooLargeToEdit => {
                self.too_large_to_edit = true;
            }
        }
    }
}

/// One tab's shared pool of live buffers.
///
/// A `Vec` and not a map, because the order *is* the history the filename
/// switcher lists and the order the eviction law is written in terms of
/// ("the earliest clean one"). One entry per [`PreviewSource`] is an invariant
/// of [`Self::open`], which is the only door in.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PreviewPool {
    buffers: Vec<PreviewBuffer>,
}

impl PreviewPool {
    /// How many buffers are alive. The header's count badge is slice 4's; this
    /// is what the cap is asserted against.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    pub fn get(&self, source: &PreviewSource) -> Option<&PreviewBuffer> {
        self.buffers.iter().find(|buffer| &buffer.source == source)
    }

    pub fn get_mut(&mut self, source: &PreviewSource) -> Option<&mut PreviewBuffer> {
        self.buffers
            .iter_mut()
            .find(|buffer| &buffer.source == source)
    }

    /// The buffer for this file, found or made — the one door into the pool.
    ///
    /// **Finding beats making**, which is the whole of "already-open file means
    /// the same buffer, edits intact, whichever pane showed it" (mock-up
    /// 5023-5026).
    ///
    /// Making one may push the pool over [`PV_BUFFER_CAP`], and then the law is
    /// the mock-up's word for word (3152-3156): evict the *earliest* buffer that
    /// is not dirty, is not the one just opened, and is not on any pane's screen
    /// — and if there is no such buffer, **evict nothing**. A pool over its cap
    /// is a pool holding nothing but state a user would miss, and a ceiling is
    /// not worth losing an unsaved edit for.
    pub fn open(
        &mut self,
        source: PreviewSource,
        name: String,
        displayed: &[PreviewSource],
    ) -> &mut PreviewBuffer {
        if let Some(index) = self.index_of(&source) {
            return &mut self.buffers[index];
        }
        self.buffers.push(PreviewBuffer::new(source.clone(), name));
        while self.buffers.len() > PV_BUFFER_CAP {
            let Some(index) = self.buffers.iter().position(|buffer| {
                !buffer.dirty && buffer.source != source && !displayed.contains(&buffer.source)
            }) else {
                break;
            };
            self.buffers.remove(index);
        }
        let index = self
            .index_of(&source)
            .expect("the buffer just opened is never the one evicted");
        &mut self.buffers[index]
    }

    /// Lift a buffer **out** of this pool, whole.
    ///
    /// The migrating half of the one case where a buffer changes tabs: a preview
    /// float docked into a tab that is not the one it was born in. It is not
    /// `open`'s business and must not be — `open` is the only door that ever
    /// reads a disk, and a buffer that has travelled is a buffer whose edits and
    /// whose dirty bit have to arrive intact rather than be read again.
    pub fn take(&mut self, source: &PreviewSource) -> Option<PreviewBuffer> {
        let index = self.index_of(source)?;
        Some(self.buffers.remove(index))
    }

    /// Put a buffer **in**, replacing whatever was under that path.
    ///
    /// [`Self::take`]'s other half. It does not evict: the cap is `open`'s law
    /// and is about a pool growing by *browsing*, while this is one buffer moving
    /// house — and a migration that silently dropped somebody's unsaved edit to
    /// keep a ceiling would be the ceiling costing exactly what it is not worth
    /// (see `open`'s own note).
    pub fn insert(&mut self, buffer: PreviewBuffer) {
        match self.index_of(&buffer.source) {
            Some(index) => self.buffers[index] = buffer,
            None => self.buffers.push(buffer),
        }
    }

    /// **P122 — one buffer arriving from another tab's pool, merged in under the
    /// law: one buffer per file, dirty wins, a tie stays with the incumbent.**
    ///
    /// The single-buffer form because there are two callers and they differ only
    /// in how many buffers they hand over: a whole tab merging in gives its whole
    /// pool ([`Self::merge_from`]), a float docking into a tab it was not born in
    /// gives exactly the one it is carrying. Writing the law twice is how the two
    /// gestures would come to disagree about what "dirty wins" means.
    ///
    /// **Why a tie leaves the incumbent standing.** Two clean copies of a file
    /// are the same file, so nothing is at stake and the cheapest correct answer
    /// is to change nothing — and two *dirty* copies are the one case §7.1.3
    /// declines to arbitrate ("共享范围到 tab 为止,同文件跨 tab 的并发编辑留给
    /// 产品端磁盘冲突检测"). Preferring the arrival there would silently discard
    /// unsaved work that has been sitting in this tab, which is the one outcome
    /// the whole dirty-gate apparatus exists to prevent.
    ///
    /// **The winner takes the loser's place in the list, not the end of it.** The
    /// order is the history the switcher lists (see the type's own note), and it
    /// is the *staying* tab's history: a file that has been open here all along
    /// does not jump to the front because a copy of it walked in.
    ///
    /// **The revision is bumped past both.** A revision counts changes to *one*
    /// buffer's body, so two buffers' counters mean nothing to each other — and a
    /// surface showing this path caches its parsed document against
    /// `(path, revision)`. Without the bump, an arrival whose counter happens to
    /// match the buffer it replaced would leave every pane on that file drawing
    /// the body it no longer holds.
    ///
    /// Panes need no redirecting afterwards, and that falls out of the port
    /// rather than being arranged: a surface names its buffer **by source**
    /// ([`crate::PreviewPane::buffer`] is a [`PreviewSource`]), so "one buffer
    /// per file" and "every pane showing the loser now reads the winner" are the
    /// same sentence here. The mock-up needed a redirect pass because its panes
    /// held object references.
    pub fn merge_buffer(&mut self, mut incoming: PreviewBuffer) {
        let Some(index) = self.index_of(&incoming.source) else {
            self.buffers.push(incoming);
            return;
        };
        let twin = &self.buffers[index];
        let (twin_is_dirty, twin_revision) = (twin.dirty, twin.revision);
        if incoming.dirty && !twin_is_dirty {
            incoming.revision = incoming.revision.max(twin_revision) + 1;
            self.buffers[index] = incoming;
        }
    }

    /// **P122/P127 — a whole pool arriving, buffer by buffer.**
    ///
    /// Taken **by value** because that is the ruling: the pool travels, it is not
    /// copied. §7.1.3's "整池随行" is about orphaned dirty buffers staying
    /// reachable somewhere, and a source left holding a second copy of everything
    /// would be exactly the fork the one-buffer-per-file law forbids — with the
    /// added cruelty that the copy nobody can reach is the one the dirty gates
    /// would go on asking about.
    ///
    /// In pool order, so the arriving tab's own history keeps its shape among the
    /// entries this tab has never seen.
    pub fn merge_from(&mut self, incoming: PreviewPool) {
        for buffer in incoming.buffers {
            self.merge_buffer(buffer);
        }
    }

    fn index_of(&self, source: &PreviewSource) -> Option<usize> {
        self.buffers
            .iter()
            .position(|buffer| &buffer.source == source)
    }

    /// Every buffer in the pool, in the order the switcher lists them.
    ///
    /// The order *is* the history (see the type's own note), so this is the one
    /// door the switcher, the count badge and the dirty gates all read through.
    pub fn buffers(&self) -> impl Iterator<Item = &PreviewBuffer> {
        self.buffers.iter()
    }

    /// The names of every dirty buffer, **except the one a pane is showing**
    /// when `shown` names it.
    ///
    /// Two readers with one question between them. The header's count badge asks
    /// it with `shown = Some(the buffer on screen)`, because the pane's own dot
    /// already speaks for that one and a badge that also lit for it would be the
    /// same fact twice (P19's `othersDirty`); the three dirty gates ask it with
    /// `None`, because a gate is about *everything* that would be lost (P120).
    pub fn dirty_names(&self, shown: Option<&PreviewSource>) -> impl Iterator<Item = &str> {
        self.buffers
            .iter()
            .filter(move |buffer| buffer.dirty && Some(&buffer.source) != shown)
            .map(|buffer| buffer.name.as_str())
    }

    /// **Write every dirty buffer in this pool back**, in the pool's own order,
    /// and report what each one came to (multiwindow slice E2 phase ①).
    ///
    /// One door for the quit card's `Save`, and it goes through [`
    /// PreviewBuffer::save`] — the same conflict check and the same atomic write
    /// `Ctrl+S` uses, per buffer. A second spelling of "write the pool back"
    /// would be a second answer to what a save *is*, and the one that got the
    /// mtime comparison wrong would be the one nobody pressed often enough to
    /// notice.
    ///
    /// **Every dirty buffer is tried, including the ones after a failure.**
    /// Stopping at the first refusal would hand the reader a list they cannot
    /// act on: half of it saved, half of it never attempted, and no way to tell
    /// which is which.
    ///
    /// [`PreviewBuffer::save`]: PreviewBuffer::save
    pub fn save_dirty(&mut self) -> Vec<(String, SaveOutcome)> {
        self.buffers
            .iter_mut()
            .filter(|buffer| buffer.dirty)
            .map(|buffer| (buffer.name.clone(), buffer.save()))
            .collect()
    }

    /// Forget everything. The two gates that take a pool's *home* away call it
    /// once the user has said the edits may go (P123/P124): closing the last
    /// preview pane strands the pool, and closing the tab is the pool's owner
    /// going away.
    pub fn clear(&mut self) {
        self.buffers.clear();
    }

    /// Forget only what the user just agreed to lose.
    ///
    /// **The shut gate's half of [`Self::clear`], and the difference is that a
    /// shut does not take the pool's home away** — the tab is about to be
    /// written to `session.json`, and its pool goes with it as the list of files
    /// the switcher will list on the next launch. Emptying it outright would
    /// answer "discard my unsaved changes" by also discarding the browsing
    /// history, which is a second thing the user was never asked about.
    ///
    /// It still satisfies what the gate needs of it: the gate raises itself off
    /// [`Self::dirty_names`], so a pool with no dirty buffer left in it cannot
    /// ask the question a second time.
    pub fn discard_dirty(&mut self) {
        self.buffers.retain(|buffer| !buffer.dirty);
    }
}

/// What is being asked about a file.
///
/// Two questions on one lane rather than two lanes, because they are the same
/// kind of question about the same file and neither is worth a second thread:
/// what separates them is only that one is answered by bytes and the other by a
/// directory entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewWant {
    /// At most [`PREVIEW_HEAD_BYTES`] of the body.
    Head,
    /// **The whole file, up to [`PREVIEW_EDIT_BYTES`]** — what asking to edit
    /// buys (T2 ③, research §10 Q2, owner's ruling 2026-09-10).
    ///
    /// [`Self::Head`]'s own question with a larger answer, and on this lane
    /// rather than a second one for the reason this enum exists: it is the same
    /// question about the same file, answered by the same bytes off the same
    /// thread. What separates the two is only how much of the file comes back,
    /// and [`PreviewBuffer::claim_head_read`] is the one place that decides
    /// which of them a buffer is owed.
    Whole,
    /// How large the whole file is — the third field of a picture's meta line
    /// (mock-up 4955), which is the only thing on that line the decoder cannot
    /// answer for itself.
    Size,
    /// **How many pages a document holds** (user ruling 2026-08-25) — the one
    /// fact a glance states about a page it cannot render that nothing on the
    /// drawing thread can answer.
    ///
    /// It reads the file's own structure ([`crate::pdf::page_count`]), which is
    /// a walk over as many bytes as the file has — and, for a document that
    /// compressed that structure out of a walk's reach, a parse of it
    /// (2026-09-05) — so it is a question for the disk's lane and never for the
    /// frame.
    ///
    /// **It used to carry the file's size with it** and stopped on 2026-08-29:
    /// the card states its facts on one line now, and the size on that line is
    /// the card's *own* — read off the very `metadata` call the card already
    /// makes on every frame to know the file is still there (§7.29 ⑬). A second
    /// stat on a worker was a second author for one number, and the number
    /// arrived a frame or two after the card that could have had it at once.
    PageCount,
}

/// "Answer this about this file for this tab of this window."
///
/// Addressed by [`TabId`] rather than by a seat, because the pool is the tab's:
/// the answer belongs to the buffer, and which pane happens to be showing it
/// when the disk answers is none of the worker's business.
///
/// **And by the window the tab is in** (user report, 2026-08-23). A `TabId` is
/// minted by a counter that starts again in every window, so `TabId(1)` names a
/// tab in each of them at once; an answer carrying only the tab is an answer any
/// window can mistake for its own. The pair is what is unique, and it is the
/// same sentence [`crate::LeafId`] says one level down about a seat inside a
/// tab.
///
/// **Addressed by [`PreviewSource`] rather than by a path** for the reason the
/// buffer is: the answer has to find its way back to one entry of a pool keyed
/// on sources, and a route that re-wrapped a path on arrival would be a second
/// place that decides what a path means. This lane only ever carries
/// [`PreviewSource::File`] — a source with no disk behind it never gets here,
/// because [`PreviewBuffer::wants_head_read`] is the only thing that sends.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewRequest {
    pub window: WindowId,
    pub tab: TabId,
    pub source: PreviewSource,
    pub want: PreviewWant,
}

impl PreviewRequest {
    /// Two requests are the same question when they ask the same thing about
    /// the same file of the same tab.
    ///
    /// **`want` is part of the target.** Without it a size question would
    /// supersede the head read of the same picture and the body would never
    /// arrive, which is coalescing turned into cancellation.
    ///
    /// **And the window is part of it**: two windows asking the same thing about
    /// the same file of their own `TabId(1)` are two questions, and one answer
    /// standing in for both leaves one of them waiting for ever.
    fn same_target(&self, other: &Self) -> bool {
        self.window == other.window
            && self.tab == other.tab
            && self.source == other.source
            && self.want == other.want
    }
}

/// What the worker found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewResponse {
    /// The window the asking tab was in — see [`PreviewRequest::window`].
    pub window: WindowId,
    pub tab: TabId,
    pub source: PreviewSource,
    pub answer: PreviewAnswer,
}

impl PreviewResponse {
    /// **Who this answer belongs to** (F1b): the tab whose pool or whose glance
    /// slot is holding the question, wherever that tab is standing now.
    ///
    /// This lane has only the one owner. A head read is claimed by a buffer in a
    /// tab's pool (§7.1.3), and the hover glance that has no pool entry is still
    /// checked against a tab of *this* window before it is offered the answer.
    pub fn owner(&self) -> crate::AnswerOwner {
        crate::AnswerOwner::Tab(self.tab)
    }
}

/// One answer to one [`PreviewWant`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreviewAnswer {
    Head(HeadOutcome),
    /// `None` when the file could not be stat'ed, which the meta line simply
    /// leaves out rather than turning into an error of its own.
    Size(Option<u64>),
    /// **How many pages the document at this path holds**, or `None` for a file
    /// whose structure would not say — which the card leaves unsaid rather than
    /// turning into an error of its own (user ruling 2026-08-25).
    PageCount(Option<u32>),
}

/// How large a file is, without reading it.
pub fn read_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|meta| meta.len())
}

/// **What this window last heard about the file under a buffer** (user ruling
/// 2026-08-29).
///
/// Three states and not a pair of bools, because the two that are not `Level`
/// are mutually exclusive by construction — a file cannot be both gone and
/// rewritten — and a pair would let a caller ask about a fourth state that
/// cannot happen.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DiskNews {
    /// Nothing outstanding. The ordinary state of every buffer in this window.
    #[default]
    Level,
    /// Somebody else wrote the file while this buffer held unsaved edits. The
    /// edits stand; the strip offers the two answers.
    Changed,
    /// The file is not on the disk any more. The body stands; the strip says so.
    Deleted,
}

/// What a caller owes after [`PreviewBuffer::note_disk_moved`].
///
/// Three answers rather than a `bool` because the two kinds of work are
/// genuinely different and the caller does different things with them: a read
/// goes out through the head worker's one-question ledger, while a sentence
/// changes a rectangle in the chrome and owes a frame. A caller handed one bool
/// would have to do both or guess.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiskVerdict {
    /// Nothing moved that this buffer can act on.
    Nothing,
    /// The head is owed a read. **The strip is not involved** — a clean body
    /// re-read is the quiet case, and the whole of what a reader sees is the
    /// paragraphs changing under them.
    ReadAgain,
    /// The strip's sentence changed. Nothing is read; a frame is owed.
    Say,
}

impl DiskVerdict {
    /// A sentence that moved is [`Self::Say`]; one that did not is
    /// [`Self::Nothing`].
    const fn from_said(said: bool) -> Self {
        if said { Self::Say } else { Self::Nothing }
    }
}

/// A head either reads or it does not, and both are answers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HeadOutcome {
    Read {
        text: String,
        /// Whether [`PREVIEW_HEAD_BYTES`] cut it short.
        truncated: bool,
        /// When the file said it was last written, **asked of the handle these
        /// bytes came out of**. A second `metadata` call by path could answer
        /// about a file that had already been replaced between the two, which
        /// is precisely the race the answer exists to detect.
        mtime: Option<SystemTime>,
        /// **Whether the bytes themselves say this is text** —
        /// [`head_reads_as_text`]'s verdict, carried back with the head that
        /// answered it (user ruling 2026-08-27; `docs/DESIGN.md` §7.32).
        ///
        /// It rides on the head rather than being a question of its own because
        /// it is a fact about the very bytes that were just read: a second
        /// `PreviewWant` for it would be a second trip to the same disk for the
        /// same 64KB, and the two trips could disagree about a file being
        /// written to right now.
        ///
        /// **Only a buffer whose *name* nobody could classify reads it.** A name
        /// in [`TEXT_EXTENSIONS`] is text because it is listed, and the body it
        /// gets is a lossy decode on purpose — a preview that refused a Latin-1
        /// log file over one byte is a preview that refuses log files. This is
        /// the stricter question the fast path never has to ask, and
        /// [`PreviewBuffer::accept`] is the one place it is asked.
        ///
        /// A composed document — a git diff, a git show — passes `true`: its
        /// text came out of a program that handed this window a `String`, and
        /// there are no bytes here to be in doubt about.
        content_says_text: bool,
        /// **What the file said it was**, carried back with the body that was
        /// decoded through it (T2 ①, 2026-09-10).
        ///
        /// It rides on the answer for `content_says_text`'s reason exactly: it
        /// is a fact about the very bytes that were just read, and a second trip
        /// to ask it could answer about a file that had since been replaced. The
        /// buffer keeps it so that a save can write the file back in it — see
        /// [`HeadEncoding::encode`] for why the mark is not this window's to
        /// drop.
        ///
        /// A composed document — a git diff, a git show — passes
        /// [`HeadEncoding::Utf8`]: its text came out of a program as a `String`
        /// and there is no file behind it to write back to.
        encoding: HeadEncoding,
        /// **Whether the decode had to invent a character** (T2 ②, 2026-09-10).
        ///
        /// [`decode_head`] is lossy on purpose, and that is right for a *look*:
        /// a preview that refused a file over one bad byte is a preview that
        /// refuses log files. It is not right for an *edit*, because the save
        /// would put every invention on the disk over the byte it stood in for.
        /// So the fact travels with the body and
        /// [`PreviewBuffer::is_editable`] reads it.
        lossy: bool,
    },
    Refused(PreviewRefusal),
    /// **The file is past [`PREVIEW_EDIT_BYTES`]** — the one answer only
    /// [`read_whole`] gives (T2 ③, 2026-09-10).
    ///
    /// Not a [`Self::Refused`], because nothing was refused: the file read
    /// perfectly well and the reader is looking at the head of it. What could
    /// not be granted is the *edit*, and this is the buffer being told so —
    /// nothing it holds is replaced, and the sentence it puts up is the one a
    /// truncated buffer already speaks
    /// ([`PreviewBuffer::read_only_notice`]).
    TooLargeToEdit,
}

/// **How many bytes of a head decide whether an unnamed kind of file is text**
/// (user ruling 2026-08-27; `docs/DESIGN.md` §7.32).
///
/// Git's own number. `buffer_is_binary` looks at the first 8000 bytes of a blob
/// and calls it binary if it finds a NUL, and the reason to borrow the constant
/// rather than to pick one is that this window is answering git's question — "is
/// there any point showing this to a human as text" — about the same files, on
/// the same machines, and an answer that disagreed with the tool the reader
/// already trusts would be a second opinion nobody asked for.
///
/// It is deliberately **less** than [`PREVIEW_HEAD_BYTES`]: the head is what
/// gets *drawn*, and this is what gets *judged*. A judgement that walked the
/// whole 64KB would spend four times the work to answer a question that is
/// settled, in every real binary format, inside the first few dozen bytes.
pub const TEXT_SNIFF_BYTES: usize = 8000;

/// **The byte-order mark a head begins with**, and therefore how the bytes after
/// it are to be read (user ruling 2026-08-27; `docs/DESIGN.md` §7.32).
///
/// Marks and nothing else. There is no statistical guessing here and there will
/// not be: a mark is the file *saying* what it is, and everything else on this
/// platform that is not UTF-8 is a code page this window has no way to name. So
/// three marks answer, and every other head is read as UTF-8 — which is what it
/// almost always is, and what the lossy decode below is written for.
///
/// **UTF-16 earns its two arms because Windows writes it.** `Out-File` and every
/// `>` redirect in Windows PowerShell 5.1 produce UTF-16 LE with a mark, so a
/// window that treated a NUL as proof of binary would refuse the transcripts its
/// own shell writes — which is exactly what this window did until this ruling.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HeadEncoding {
    /// No mark. Read as UTF-8.
    #[default]
    Utf8,
    /// `EF BB BF`.
    Utf8Bom,
    /// `FF FE`.
    Utf16Le,
    /// `FE FF`.
    Utf16Be,
}

impl HeadEncoding {
    /// Which mark, if any, these bytes begin with.
    #[must_use]
    pub fn of(head: &[u8]) -> Self {
        if head.starts_with(&[0xEF, 0xBB, 0xBF]) {
            Self::Utf8Bom
        } else if head.starts_with(&[0xFF, 0xFE]) {
            Self::Utf16Le
        } else if head.starts_with(&[0xFE, 0xFF]) {
            Self::Utf16Be
        } else {
            Self::Utf8
        }
    }

    /// How many bytes of the head the mark itself occupies.
    #[must_use]
    fn mark_len(self) -> usize {
        match self {
            Self::Utf8 => 0,
            Self::Utf16Le | Self::Utf16Be => 2,
            Self::Utf8Bom => 3,
        }
    }

    /// Whether the bytes after the mark are pairs rather than octets.
    #[must_use]
    fn is_utf16(self) -> bool {
        matches!(self, Self::Utf16Le | Self::Utf16Be)
    }

    /// The bytes this head carries **after** its mark.
    #[must_use]
    fn body(self, head: &[u8]) -> &[u8] {
        &head[self.mark_len().min(head.len())..]
    }

    /// The bytes a file in this encoding holds for this text — [`Self::body`]
    /// and [`decode_head`] run backwards, **mark included**.
    ///
    /// The mark is written back because it was read: a file that begins by
    /// saying what it is has said something, and a save that dropped the
    /// sentence would be this window answering a question it was not asked. That
    /// is the whole of the 2026-09-10 defect — a UTF-16 transcript, which is what
    /// Windows PowerShell 5.1 writes, came back as unmarked UTF-8 after one line
    /// of it was edited, and every byte of it outside that line had changed.
    ///
    /// Nothing here touches line endings or the final newline: they are
    /// characters in the body, they survived the read, and they survive this.
    #[must_use]
    pub fn encode(self, text: &str) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(text.len() + self.mark_len());
        match self {
            Self::Utf8 => {}
            Self::Utf8Bom => bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]),
            Self::Utf16Le => bytes.extend_from_slice(&[0xFF, 0xFE]),
            Self::Utf16Be => bytes.extend_from_slice(&[0xFE, 0xFF]),
        }
        match self {
            Self::Utf8 | Self::Utf8Bom => bytes.extend_from_slice(text.as_bytes()),
            Self::Utf16Le => bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes)),
            Self::Utf16Be => bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes)),
        }
        bytes
    }
}

/// **Whether these bytes are text on their own evidence** (user ruling
/// 2026-08-27; `docs/DESIGN.md` §7.32).
///
/// The judgement a name that nobody listed is promoted on, and the whole of it:
/// the head decodes under the encoding its mark declares, and the first
/// [`TEXT_SNIFF_BYTES`] of it hold no NUL.
///
/// **Why decoding is asked as well as the NUL.** A NUL alone is git's rule and
/// it is the right rule for a *byte* stream, but the two UTF-16 arms are full of
/// NULs by construction — every ASCII character in a UTF-16 LE file is a letter
/// followed by one — so the question has to be asked of the characters and not
/// of the octets. Once it is asked of characters, "does it decode" is already
/// most of the answer, and asking it costs one pass either way.
///
/// **A sequence the cut broke in half is the cut's fault, not the file's.** The
/// window ends at a fixed offset, so the last character of a large file's window
/// is very often incomplete; refusing on it would make a file's classification
/// depend on where 8000 bytes happens to land inside it.
#[must_use]
pub fn head_reads_as_text(head: &[u8]) -> bool {
    let encoding = HeadEncoding::of(head);
    let body = encoding.body(head);
    let cut = body.len() > TEXT_SNIFF_BYTES;
    let window = &body[..body.len().min(TEXT_SNIFF_BYTES)];
    if encoding.is_utf16() {
        // The trailing half-pair is forgiven **only when there was a cut**, on
        // the UTF-8 arm's own terms below: a file that simply ends on a lone
        // high surrogate is malformed, and nothing here should call it text.
        let units = utf16_units(window, encoding == HeadEncoding::Utf16Le, cut);
        return char::decode_utf16(units)
            .all(|decoded| matches!(decoded, Ok(character) if character != '\0'));
    }
    match std::str::from_utf8(window) {
        Ok(text) => !text.contains('\0'),
        // The one error this forgives, and only when there really was a cut: a
        // sequence that runs off the end of the window (`error_len() == None`)
        // with everything before it valid.
        Err(error) => {
            cut && error.error_len().is_none() && !window[..error.valid_up_to()].contains(&0)
        }
    }
}

/// The `u16`s a run of UTF-16 bytes spells.
///
/// A trailing odd byte is dropped and so — when `whole_pairs` is set — is a
/// trailing lone high surrogate: both are artefacts of where the caller stopped
/// reading rather than facts about the file, which is [`trim_partial_utf8`]'s own
/// sentence one encoding over.
fn utf16_units(bytes: &[u8], little_endian: bool, whole_pairs: bool) -> Vec<u16> {
    let mut units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| {
            if little_endian {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    if whole_pairs
        && units
            .last()
            .is_some_and(|unit| (0xD800..0xDC00).contains(unit))
    {
        units.pop();
    }
    units
}

/// How a save turned out.
///
/// Three outcomes rather than a `Result`, because the middle one is not a
/// failure: the disk moved and the window is declining to guess, which is a
/// sentence the user is owed and a state the buffer survives intact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SaveOutcome {
    Saved,
    /// The file on disk is not the file that was read (ruling 8⑨).
    Conflict,
    Failed(String),
}

/// The acknowledgement a save gets, and how long it stands.
///
/// Ruling 6 (2026-08-12): the mock-up's four feedback durations collapse to the
/// one the foot's "Revealed" already used. The word belongs to the pane foot,
/// and since 2026-08-15 it is printed there on every surface that has one — a
/// docked pane, a torn-off float — in the strip's **left** hand, where the
/// reveal's confirmation goes, while the strip's right hand steps aside for as
/// long as it stands.
pub fn preview_saved_notice() -> &'static str {
    crate::i18n::Text::PreviewSaved.text()
}

/// What the window says instead of overwriting somebody else's write.
///
/// It says what happened, what was *not* done, and what is still true — the
/// edits are still here — because a conflict notice that only announces failure
/// leaves the user believing their work is gone.
///
/// **All three facts, in a phrase** (user ruling, 2026-08-15). It used to be a
/// full sentence on a floating strip inside the body; the ruling moved every
/// standing notice to the right hand of the path foot, so the sentence had to
/// become something that fits beside a path. What it must not lose is the third
/// clause, and it has not: a user who reads only "Not saved" is the reader this
/// wording exists for.
pub fn preview_conflict_notice() -> &'static str {
    crate::i18n::Text::PreviewConflict.text()
}

/// When a file was last written, or `None` if it will not say.
pub fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
}

/// Read at most [`PREVIEW_HEAD_BYTES`] of a file, and decide what it is.
///
/// **The size question and the binary question are the same read.** Both are
/// facts about the first 64KB, so asking the disk twice would buy nothing but
/// two chances to disagree — the head is taken once, its length answers
/// truncation, and its bytes answer whether this is text at all.
pub fn read_head(path: &Path) -> HeadOutcome {
    read_up_to(path, PREVIEW_HEAD_BYTES)
}

/// Read the whole file, up to [`PREVIEW_EDIT_BYTES`] — **the read asking to edit
/// buys** (T2 ③, research §10 Q2).
///
/// [`read_head`]'s own body with a larger limit, and deliberately the same
/// function underneath: it goes through the same
/// [`bt_transcript::paths::may_read_unasked_through_links`] door, on the same
/// worker thread, and comes back as the same [`HeadOutcome`] that the same
/// [`PreviewBuffer::accept`] files — so the disk news, the stamp and the sniff
/// are answered once each and by one author, not twice by two.
///
/// The one thing it says that a head read cannot: a file past the editing cap is
/// [`HeadOutcome::TooLargeToEdit`] rather than another truncated body. Truncated
/// is what a *glance* is, and a second truncated body would replace the 64KB on
/// the glass with 8MB of the same document to no one's benefit; what the reader
/// is owed here is the sentence that this file stays read-only, and the head
/// they are already reading.
pub fn read_whole(path: &Path) -> HeadOutcome {
    match read_up_to(path, PREVIEW_EDIT_BYTES) {
        // Truncated at *this* limit means the file is past the editing cap —
        // there is nothing else a whole-file read can be cut short by.
        HeadOutcome::Read {
            truncated: true, ..
        } => HeadOutcome::TooLargeToEdit,
        outcome => outcome,
    }
}

/// The read both lanes are, with the limit as the only difference.
fn read_up_to(path: &Path, limit: usize) -> HeadOutcome {
    // **The read is behind this line, so the question is asked in front of it** (route B of the
    // untrusted-path audit, 2026-09-08). `File::open` followed by `read_to_end` has no end when
    // what was opened is a door somebody else is holding — `\\.\pipe\name` accepts and never
    // writes — and this worker serves one request at a time, so one such open silences every
    // preview after it. `PreviewBuffer::new` asks the same question before it ever files a read;
    // this is the same predicate asked where the blocking call actually is, and it asks the disk's
    // half of it as well, because a drive-rooted name can be a local spelling of a share.
    if !bt_transcript::paths::may_read_unasked_through_links(
        path,
        bt_transcript::paths::PathNamer::ThisWindow,
    ) {
        return HeadOutcome::Refused(PreviewRefusal::NetworkPath);
    }
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            return HeadOutcome::Refused(PreviewRefusal::Fault(PreviewFault::from_io(&error)));
        }
    };
    // One byte past the limit, which is the cheapest honest way to learn that
    // there *is* more: a length is a second question and a metadata read can
    // disagree with the bytes on a file being written to right now.
    let mut head = Vec::new();
    if let Err(error) = file.by_ref().take(limit as u64 + 1).read_to_end(&mut head) {
        return HeadOutcome::Refused(PreviewRefusal::Fault(PreviewFault::from_io(&error)));
    }
    let truncated = head.len() > limit;
    head.truncate(limit);
    // Asked of the handle the bytes came out of, not of the path: between two
    // calls by name a file can be replaced entirely, and a stamp belonging to a
    // file other than the one that was read is worse than no stamp at all.
    let mtime = file.metadata().ok().and_then(|meta| meta.modified().ok());
    // **The strict question, asked of every head and read by almost none of
    // them** (user ruling 2026-08-27; §7.32). It is what a name nobody listed is
    // promoted on — see [`HeadOutcome::Read::content_says_text`] — and it is
    // computed here because here is where the bytes are.
    let content_says_text = head_reads_as_text(&head);
    let encoding = HeadEncoding::of(&head);
    // The one sniff §7.1.3 asks for, and the only one that is nearly free and
    // nearly never wrong: text does not hold a NUL, and every binary format
    // worth refusing holds one in its first few bytes.
    //
    // **Except behind a UTF-16 mark**, where a NUL is what an ASCII letter is
    // spelled with. A file that says it is UTF-16 and then decodes is text
    // whatever its octets look like; a file that says it is UTF-16 and does
    // *not* decode is a binary file that happened to begin with those two
    // bytes, and it is refused on the same word this line has always refused
    // on.
    let holds_a_nul = if encoding.is_utf16() {
        !content_says_text
    } else {
        head.contains(&0)
    };
    if holds_a_nul {
        return HeadOutcome::Refused(PreviewRefusal::Binary);
    }
    let (text, lossy) = decode_head(&head, truncated);
    HeadOutcome::Read {
        text,
        truncated,
        mtime,
        content_says_text,
        encoding,
        lossy,
    }
}

/// Turn a head of bytes into text.
///
/// Lossy, because a preview that refuses a file over one bad byte is a preview
/// that refuses log files. The one thing done first is dropping a multi-byte
/// character the *cut* broke in half: that replacement character would be an
/// artefact of the limit rather than of the file, and it would sit at the end of
/// every truncated CJK document.
///
/// **The mark is read first** (user ruling 2026-08-27; §7.32). A head that
/// declares UTF-16 is decoded as UTF-16 and a UTF-8 mark is eaten rather than
/// drawn as `` at the top of the body — one function, because the encoding a
/// file is *judged* under ([`head_reads_as_text`]) and the encoding it is *shown*
/// in have to be the same one or a promoted file would be drawn as mojibake.
///
/// **And it says whether it had to invent anything** (T2 ②, 2026-09-10). The
/// second half of the answer is the whole reason an edit can be refused
/// honestly: a body holding replacement characters this function put there is a
/// body that no longer knows what some of the file's bytes were, and saving it
/// would write those inventions over the originals. The lossiness is a fact
/// about the decode, so it is reported by the decode rather than guessed at
/// afterwards by looking for U+FFFD — which would also find the ones a file
/// genuinely contains.
fn decode_head(head: &[u8], truncated: bool) -> (String, bool) {
    let encoding = HeadEncoding::of(head);
    let body = encoding.body(head);
    if encoding.is_utf16() {
        // The pairing is `truncated`'s business for [`trim_partial_utf8`]'s
        // reason exactly: a high surrogate whose low half is past the cut is the
        // cut's artefact, and a replacement character parked at the end of every
        // long UTF-16 document is a lie about the file.
        let units = utf16_units(body, encoding == HeadEncoding::Utf16Le, truncated);
        let mut lossy = false;
        let text = char::decode_utf16(units)
            .map(|decoded| {
                decoded.unwrap_or_else(|_| {
                    lossy = true;
                    char::REPLACEMENT_CHARACTER
                })
            })
            .collect();
        // An odd trailing byte is a code unit this decode never saw, and at the
        // end of a whole file that is a byte the save would drop.
        return (text, lossy || (!truncated && !body.len().is_multiple_of(2)));
    }
    let body = if truncated {
        trim_partial_utf8(body)
    } else {
        body
    };
    match String::from_utf8_lossy(body) {
        // Borrowed is exactly "every byte decoded as itself"; owned is
        // `from_utf8_lossy` having built a new string around a replacement
        // character, which is the one case there is to report.
        std::borrow::Cow::Borrowed(text) => (text.to_owned(), false),
        std::borrow::Cow::Owned(text) => (text, true),
    }
}

/// Drop a trailing UTF-8 sequence the caller's cut left incomplete.
fn trim_partial_utf8(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    // A UTF-8 sequence is at most four bytes, so at most three continuations can
    // precede the lead byte being looked for.
    let mut continuations = 0usize;
    while end > 0 && continuations < 4 {
        let byte = bytes[end - 1];
        if byte & 0b1100_0000 == 0b1000_0000 {
            end -= 1;
            continuations += 1;
            continue;
        }
        let needed = if byte < 0x80 {
            1
        } else if byte >> 5 == 0b110 {
            2
        } else if byte >> 4 == 0b1110 {
            3
        } else if byte >> 3 == 0b1_1110 {
            4
        } else {
            // A stray continuation or an illegal lead: not a sequence this can
            // reason about, so leave it to the lossy decode.
            1
        };
        return if needed <= continuations + 1 {
            bytes
        } else {
            &bytes[..end - 1]
        };
    }
    bytes
}

/// The queue in front of the disk, newest question per target.
#[derive(Default)]
struct PendingPreviewRequests {
    requests: std::collections::VecDeque<PreviewRequest>,
}

impl PendingPreviewRequests {
    fn push_latest(&mut self, request: PreviewRequest) {
        if let Some(index) = self
            .requests
            .iter()
            .position(|queued| queued.same_target(&request))
        {
            self.requests.remove(index);
        }
        self.requests.push_back(request);
    }

    fn pop_front(&mut self) -> Option<PreviewRequest> {
        self.requests.pop_front()
    }

    fn contains_target(&self, request: &PreviewRequest) -> bool {
        self.requests
            .iter()
            .any(|queued| queued.same_target(request))
    }

    fn drain_channel(&mut self, receiver: &mpsc::Receiver<PreviewRequest>) {
        while let Ok(request) = receiver.try_recv() {
            self.push_latest(request);
        }
    }
}

/// Serve file questions, newest per target first.
///
/// Split from [`PreviewWorker::spawn`] so the coalescing can be tested without a
/// filesystem or an event loop, exactly as `run_dir_worker` is.
fn run_preview_worker(
    receiver: mpsc::Receiver<PreviewRequest>,
    mut execute: impl FnMut(PreviewRequest),
) {
    let mut pending = PendingPreviewRequests::default();
    while let Ok(request) = receiver.recv() {
        pending.push_latest(request);
        pending.drain_channel(&receiver);
        while let Some(request) = pending.pop_front() {
            pending.drain_channel(&receiver);
            if pending.contains_target(&request) {
                continue;
            }
            execute(request);
        }
    }
}

/// The thread, and the two ends of the conversation with it.
pub struct PreviewWorker {
    requests: mpsc::Sender<PreviewRequest>,
    pub responses: mpsc::Receiver<PreviewResponse>,
}

impl PreviewWorker {
    pub fn spawn(proxy: EventLoopProxy<AppEvent>) -> Result<Self> {
        let (request_tx, request_rx) = mpsc::channel::<PreviewRequest>();
        let (response_tx, response_rx) = mpsc::channel::<PreviewResponse>();
        bt_platform::spawn_at_priority(
            "bt-preview-worker",
            bt_platform::ThreadPriority::BelowNormal,
            move || {
                run_preview_worker(request_rx, |request| {
                    // **This thread is a disk**, and every one of its questions
                    // is about bytes at a path. A source with nothing at a path is
                    // never sent here — [`PreviewBuffer::wants_head_read`] is the
                    // gate, and a picture's size is asked of a file the decode
                    // lane already holds — so this is the same shape a request
                    // for a tab that has since closed takes: no answer, which is
                    // the cancellation §7.1.3 asks for.
                    let Some(path) = request.source.file_path() else {
                        return;
                    };
                    let answer = match request.want {
                        PreviewWant::Head => PreviewAnswer::Head(read_head(path)),
                        // The same lane and the same answer shape — see
                        // [`PreviewWant::Whole`].
                        PreviewWant::Whole => PreviewAnswer::Head(read_whole(path)),
                        PreviewWant::Size => PreviewAnswer::Size(read_size(path)),
                        // Straight off the file's structure: the wrapper that
                        // used to stat the file beside this call is gone with
                        // the size it read (see [`PreviewWant::PageCount`]).
                        PreviewWant::PageCount => {
                            PreviewAnswer::PageCount(crate::pdf::page_count(path))
                        }
                    };
                    if response_tx
                        .send(PreviewResponse {
                            window: request.window,
                            tab: request.tab,
                            source: request.source,
                            answer,
                        })
                        .is_ok()
                    {
                        let _ = proxy.send_event(AppEvent::PreviewReady);
                    }
                });
            },
        )
        .context("spawn file preview reading worker")?;
        Ok(Self {
            requests: request_tx,
            responses: response_rx,
        })
    }

    /// Ask, reporting whether the worker was still there to be asked.
    #[must_use]
    pub fn request(&self, request: PreviewRequest) -> bool {
        self.requests.send(request).is_ok()
    }
}

/// Turn preview reading off for the rest of the run, once.
pub fn disable_preview_worker_state(running: &mut bool, notice_pending: &mut bool) -> bool {
    if !*running {
        return false;
    }
    *running = false;
    *notice_pending = true;
    eprintln!("file preview reading worker stopped; terminal input and output remain available");
    true
}

pub fn take_preview_worker_notice(notice_pending: &mut bool) -> Option<&'static str> {
    if std::mem::take(notice_pending) {
        Some(preview_worker_stopped_notice())
    } else {
        None
    }
}

/// **What a link target written in a document names** (user ruling,
/// 2026-08-13; the web arm re-read 2026-08-29, §7.1.5g ⑦).
///
/// A resolver and not a verb table, which is the whole of what changed on
/// 2026-08-29. The web arm used to be called `Browse` and to *mean* it: a plain
/// press on it went straight to `ShellExecuteW`, and this function was the only
/// thing anybody asked. That was written before [`crate::ClickIntent`] existed
/// (2026-08-13 against 2026-08-20) and it had drifted to the **opposite** answer
/// from the terminal's — 平点 = 交出去 here, 平点 = 留在窗内 there, one product
/// with two rules for one gesture, which is exactly the disagreement
/// `ClickIntent` was minted to end.
///
/// So the modifier is not read here at all. This says which of three kinds of
/// thing the string names; **which door a press on it leaves by** is
/// [`crate::preview_link_activation`]'s question, and it answers it out of the
/// terminal's own `http(s)` row rather than out of a second opinion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkAction {
    /// A file, resolved — it opens **here**, in this window's own preview.
    Preview(PathBuf),
    /// A web address. **Not a verb**: see the note above.
    Web(String),
    /// **A file this window will not read off a hover** (route E of the untrusted-path audit,
    /// 2026-09-08) — a share, a device path, a distribution nobody here is standing in.
    ///
    /// Apart from [`Self::Nowhere`] because the two readers of this table owe a reader two
    /// different sentences about it. A *link* wearing it is not a link: pressing it does nothing,
    /// exactly as pressing a `mailto:` does nothing, and the row says so by wearing no finger. An
    /// *image source* wearing it is a picture this window will not fetch, which is the "not shown"
    /// placeholder a markdown page already draws over a source it cannot read — and drawing
    /// "resolves to nothing" over a source that resolves perfectly well would be the page saying
    /// something untrue about a file that is there.
    ///
    /// The path travels because both of those sentences are about a file somebody named, and a
    /// diagnostic that could not name it would be a diagnostic about nothing.
    Refused(PathBuf),
    /// Nothing this window will act on.
    Nowhere,
}

/// Resolve a link target written in `document` into what pressing it does.
///
/// # The ruling
///
/// **A link that points at a file is a way of pointing at a file**, and this
/// window has exactly one answer for that: show it in the preview. It is the
/// same sentence [`crate::files_row_activation`] makes about a row in the tree
/// and the file menu makes about its first item — 指到文件=预览它 — and a
/// third answer for the third door would be three things to keep in step.
/// Anything the preview cannot read is *still* previewed: it lands on the seat
/// as an unknown buffer and the card offers 「Open in default app」, which is
/// the escape hatch chosen rather than the fork fallen down.
///
/// `http`/`https` come back as [`LinkAction::Web`] and go no further here: what
/// a press on a web address spends is the terminal's own `http(s)` row, read
/// once for both surfaces ([`crate::web_address_activation`]). **Every other
/// scheme is refused** — `mailto:`, `ftp:`, `javascript:` and whatever else a
/// document may carry — for the reason the terminal's own OSC-8 handler refuses
/// them: a document is untrusted text, and handing an arbitrary scheme to
/// `ShellExecute` is handing it whatever the machine has registered for that
/// scheme.
///
/// **「Open the containing folder」 is not here**, deliberately. That is the
/// foot's Reveal button and it stays the foot's: a link names a *file*, and
/// answering it with its parent directory is answering a question nobody asked.
///
/// Resolution rules, in order:
///
/// * an empty target, or a bare `#fragment`, is nothing — there is no
///   within-document navigation to do yet, and jumping to the top would be a
///   worse answer than none;
/// * a trailing `#fragment` is **cut** off a path first: `DESIGN.md#7.1.2`
///   names `DESIGN.md`, and the anchor is simply a part of the address this
///   window cannot honour yet;
/// * `file:` is unwrapped to the path it carries, percent-escapes and all;
/// * anything else carrying a `scheme:` is refused, *except* that a bare
///   Windows drive letter (`C:\x`) is a path and not a scheme — one letter
///   before the colon cannot be a scheme, and RFC 3986 says so too;
/// * an absolute path is taken as it stands; a relative one is resolved
///   against the **document's own directory**, which is the only frame a
///   relative link has ever meant.
#[must_use]
pub fn link_action(target: &str, document: &Path) -> LinkAction {
    let target = target.trim();
    if target.is_empty() || target.starts_with('#') {
        return LinkAction::Nowhere;
    }
    let lower = target.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return LinkAction::Web(target.to_owned());
    }
    let path = if lower.starts_with("file:") {
        let Some(path) = file_url_path(target) else {
            return LinkAction::Nowhere;
        };
        path
    } else if let Some(scheme) = scheme_of(target) {
        // A drive letter is not a scheme; every real scheme left here is one
        // this window does not open.
        if scheme.len() > 1 {
            return LinkAction::Nowhere;
        }
        PathBuf::from(strip_fragment(target))
    } else {
        PathBuf::from(strip_fragment(target))
    };
    if path.as_os_str().is_empty() {
        return LinkAction::Nowhere;
    }
    if path.is_absolute() {
        return resolved_link(normalized(&path));
    }
    match document.parent() {
        Some(directory) => resolved_link(normalized(&directory.join(path))),
        // A document with no directory is one with no relative frame; there is
        // nowhere for the link to be relative *to*.
        None => LinkAction::Nowhere,
    }
}

/// The resolved path, sorted into the arm this window may act on.
///
/// **A document is text somebody else wrote, and its targets are that person's**
/// (route E of the untrusted-path audit, 2026-09-08). A markdown page rendered
/// on a hover carries the link targets and image sources its author put in it,
/// and a page that named `\\attacker\share\x.png` had its picture asked for
/// during the render — an SMB probe with no click anywhere in it. The gate is
/// [`is_readable_unasked`], which is the same one the buffer beside it is built
/// through, so a target the pane would refuse to open is a target the page will
/// not go looking at either.
fn resolved_link(path: PathBuf) -> LinkAction {
    if is_readable_unasked(&path) {
        LinkAction::Preview(path)
    } else {
        LinkAction::Refused(path)
    }
}

/// Fold `.` and `..` out of a path, **textually**.
///
/// # Why lexically, and why at all (user report, 2026-08-13)
///
/// The first version left the climb in — `…\preview-samples\../../docs/DESIGN.md`
/// — on the reasoning that the file system resolves `..` anyway and folding it
/// here would be guessing about symlinks. Opening the file worked. Everything
/// *else* did not: the foot printed that string at the user, and Explorer's
/// `/select` was handed it and quietly opened the wrong folder. A path that
/// leaves this window — into a caption, into another program's command line —
/// has to be the path a person would have written.
///
/// **Lexically is the correct algorithm here, not a shortcut.** A markdown link
/// is resolved the way a URL reference is (RFC 3986 §5.2.4 removes `..`
/// segments by pure string surgery, before anything is dereferenced), so
/// folding the text *is* what the author meant. `canonicalize` would be the
/// wrong tool twice over: it asks the disk, so it fails for a link to a file
/// that does not exist yet, and it returns a `\\?\` extended path that no
/// caption should ever show.
fn normalized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // A `..` climbs over a real name; one at the root has nothing
                // above it to climb, and one already following a `..` is part
                // of the same climb rather than the end of it.
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else if out.has_root() {
                    // Above the root there is nothing. Windows agrees: `C:\..`
                    // is `C:\`.
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The `scheme` of `scheme:rest`, when the text in front of the first colon
/// looks like one (RFC 3986: a letter, then letters, digits, `+`, `-`, `.`).
fn scheme_of(target: &str) -> Option<&str> {
    let colon = target.find(':')?;
    let scheme = &target[..colon];
    let mut characters = scheme.chars();
    let first = characters.next()?;
    (first.is_ascii_alphabetic()
        && characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    .then_some(scheme)
}

/// Everything before a trailing `#anchor`.
fn strip_fragment(target: &str) -> &str {
    target.split_once('#').map_or(target, |(path, _)| path)
}

/// The path inside a `file:` URL — `file:///C:/a/b`, `file://host/share/a` and
/// the abbreviated `file:/C:/a` alike, with percent-escapes undone.
fn file_url_path(target: &str) -> Option<PathBuf> {
    let rest = strip_fragment(target).get("file:".len()..)?;
    // `file://host/share` is a UNC path and keeps its two leading slashes;
    // `file:///C:/x` and `file:/C:/x` are local and lose all of theirs.
    let local = rest.strip_prefix("//").map_or(rest, |authority| {
        authority.strip_prefix('/').unwrap_or(authority)
    });
    let text = if rest.starts_with("//") && !rest.starts_with("///") {
        format!(r"\\{}", percent_decode(local))
    } else {
        percent_decode(local.trim_start_matches('/'))
    };
    (!text.is_empty()).then(|| PathBuf::from(text.replace('/', r"\")))
}

/// `%20` and its kin, undone. A `%` that does not begin a valid escape is a
/// literal `%`, which is what every lenient reader does and what a hand-written
/// link most often means.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escape = (bytes[index] == b'%')
            .then(|| {
                text.get(index + 1..index + 3)
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            })
            .flatten();
        match escape {
            Some(byte) => {
                out.push(byte);
                index += 3;
            }
            None => {
                out.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// How thick a scrolling region's bar is *drawn*.
pub const BLOCK_SCROLL_THICKNESS_LOGICAL_PX: f32 = 2.0;
/// How thick it is to a **hand** — the divider's `SEAT_DIVIDER_HIT_LOGICAL_PX`
/// and for its reason: one drawn pixel is not a target.
///
/// The reported bug (2026-08-12) was that the bar could not be dragged at all;
/// half of the answer was giving it a drag, and the other half is admitting
/// that two pixels is not something anyone can put a pointer on. The band is
/// grown around the drawn rule on every side, so the tolerance is the same
/// whether the approach is from inside the block or from the gap below it.
pub const BLOCK_SCROLL_HIT_LOGICAL_PX: f32 = 7.0;
/// How far **inward** a bar riding a *surface's own edge* reaches for a hand
/// (real-machine finding, 2026-08-14).
///
/// # The pixels at a surface's edge are already somebody's
///
/// [`BLOCK_SCROLL_HIT_LOGICAL_PX`] grows a bar's target on every side, and that
/// is right for a block's bar: it lies inside a document, and the pixels just
/// past it are the same document's. A *surface's* bar is at the surface's own
/// edge, and what lies past that edge is never content — it is the next
/// sovereign band along, and both of the ones this window has are wider than the
/// growth:
///
/// * a **divider** between two panes claims [`crate::seats`]'s
///   `SEAT_DIVIDER_HIT_LOGICAL_PX` around the seam, and it claims it first —
///   the pane bar of a preview with a neighbour lies entirely inside that band;
/// * the **window's own resize border** claims eight logical pixels of the far
///   edge before the application is asked at all. A press there is a
///   `WM_NCHITTEST` answer, not a pointer event: the pane bar of the right-most
///   preview never reached this window's code.
///
/// Measured on a real window (2026-08-14): a docked preview's bar could be seen,
/// tracked the wheel exactly, and could not be taken by any hand — at the window
/// edge because the press was a resize, at a seam because the press was a
/// divider.
///
/// So the *picture* stays where the ruling put it — a rule on the surface's own
/// far edge — and the *target* grows inward instead, which is what every overlay
/// scrollbar on the desk does when a pointer approaches it. Nothing is taken
/// from either sovereign: the band grows into the surface's own content, and the
/// far side is clamped to the edge rather than reaching across it.
pub const BODY_SCROLL_INWARD_HIT_LOGICAL_PX: f32 = 16.0;

/// The shortest a thumb may be *drawn* (ruling 2026-08-14, both axes): the
/// honest proportional share of a long document collapses toward one pixel,
/// and a thumb that cannot be seen cannot be taken. Every desktop scrollbar
/// floors its thumb for the same reason.
pub const BLOCK_SCROLL_MIN_THUMB_LOGICAL_PX: f32 = 24.0;

/// Which way a scrolling region runs beneath its bar.
///
/// The axis is carried on the bar rather than known by its callers because the
/// *drag* has to read it back: "how far along the track did the hand get" is a
/// question about x for one of these and about y for the other, and a drag that
/// guessed would move the content sideways when the thumb went down.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollAxis {
    /// A markdown block too wide for its page: a rule along the bottom edge, the
    /// thumb travelling left to right.
    Horizontal,
    /// A glance card whose document is taller than the card: a rule down the
    /// right edge, the thumb travelling top to bottom.
    Vertical,
}

/// The scroll bar a region wears along the edge it overflows past.
///
/// One answer for the painter, the hit test and the drag alike: a thumb drawn
/// somewhere the pointer is not tested is a thumb that looks draggable and
/// is not, which is the whole of the bug this replaced.
///
/// **Grown to two axes on 2026-08-14**, when the glance card became a surface a
/// hand could scroll. The card's bar is this one stood on its end: the same
/// proportion, the same thickness, the same grab tolerance, the same linear map
/// from thumb to offset. Copying it into a second function is how two scrollbars
/// that are the same scrollbar drift apart — the block's bar has already been
/// through one round of "the picture and the hit test disagreed", and that is
/// the bug a copy re-opens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollBar {
    pub axis: ScrollAxis,
    /// The full-length rule the thumb runs along.
    pub track: [f32; 4],
    /// The visible share of the content, drawn in proportion.
    pub thumb: [f32; 4],
    /// The thumb widened to something a hand can land on.
    pub grab: [f32; 4],
    /// How far the thumb's leading edge may travel along the track.
    pub travel: f32,
    /// How far the content may travel under the region's own rectangle.
    pub overflow: f32,
}

impl ScrollBar {
    /// Where the track begins, on the bar's own axis — the origin every
    /// travelled distance is measured from.
    #[must_use]
    pub fn track_start(&self) -> f32 {
        match self.axis {
            ScrollAxis::Horizontal => self.track[0],
            ScrollAxis::Vertical => self.track[1],
        }
    }

    /// The same bar with its target reaching `inward` pixels **into** the
    /// surface, and not one pixel past the surface's own edge.
    ///
    /// See [`BODY_SCROLL_INWARD_HIT_LOGICAL_PX`] for what lives on the other
    /// side of that edge and why nothing may be taken from it. The growth along
    /// the bar's own axis is left exactly as [`scroll_bar`] made it — the ends
    /// of a thumb are as hard to land on as they ever were, and nothing on that
    /// axis belongs to anybody else.
    #[must_use]
    pub fn grown_inward(self, inward: f32) -> Self {
        let grab = match self.axis {
            ScrollAxis::Vertical => [
                (self.thumb[0] - inward).min(self.grab[0]),
                self.grab[1],
                self.track[2],
                self.grab[3],
            ],
            ScrollAxis::Horizontal => [
                self.grab[0],
                (self.thumb[1] - inward).min(self.grab[1]),
                self.grab[2],
                self.track[3],
            ],
        };
        Self { grab, ..self }
    }

    /// Where a pointer at `at` stands on the bar's own axis.
    #[must_use]
    pub fn along(&self, at: [f32; 2]) -> f32 {
        match self.axis {
            ScrollAxis::Horizontal => at[0],
            ScrollAxis::Vertical => at[1],
        }
    }
}

/// The bar for a region of `content` pixels shown through `clip` along `axis`,
/// scrolled by `offset` — or `None` when the whole of it fits and there is
/// nothing to say.
///
/// The two axes are the same six lines of arithmetic read against different
/// components of the rectangle, which is why they are one function: the page is
/// the clip's extent along the axis, the rule lies against the clip's far edge
/// across it, and the thumb is the visible share of the content placed in
/// proportion. Nothing about "wide block" or "tall card" survives into the
/// numbers.
#[must_use]
pub fn scroll_bar(
    clip: [f32; 4],
    axis: ScrollAxis,
    offset: f32,
    content: f32,
    scale: f32,
) -> Option<ScrollBar> {
    // `near`/`far` bound the page along the axis; `edge` is the side the rule
    // lies against — the bottom of a horizontal region, the right of a vertical
    // one, which is where every scrollbar on the desk puts it.
    let (near, far, edge) = match axis {
        ScrollAxis::Horizontal => (clip[0], clip[2], clip[3]),
        ScrollAxis::Vertical => (clip[1], clip[3], clip[2]),
    };
    let page = (far - near).max(1.0);
    let overflow = content - page;
    if overflow <= 0.0 {
        return None;
    }
    let thickness = (BLOCK_SCROLL_THICKNESS_LOGICAL_PX * scale).round().max(1.0);
    let rule = edge - thickness;
    // The proportional length, floored at a graspable minimum (ruling
    // 2026-08-14): a document long enough shrinks the honest share to a
    // one-pixel sliver, and a thumb that cannot be seen cannot be taken. The
    // floor is capped by the page itself; the travel mapping below stays linear
    // over whatever travel remains.
    let length = (page * (page / content))
        .max(BLOCK_SCROLL_MIN_THUMB_LOGICAL_PX * scale)
        .min(page)
        .max(1.0);
    let travel = (page - length).max(0.0);
    let start = near + travel * (offset.clamp(0.0, overflow) / overflow);
    let (track, thumb) = match axis {
        ScrollAxis::Horizontal => ([near, rule, far, edge], [start, rule, start + length, edge]),
        ScrollAxis::Vertical => ([rule, near, edge, far], [rule, start, edge, start + length]),
    };
    // Grown on every side by the same amount, the way `seats::hit_band` grows a
    // divider: the tolerance is a property of the hand, not of the direction it
    // comes from.
    let grow = ((BLOCK_SCROLL_HIT_LOGICAL_PX * scale - thickness) / 2.0).max(0.0);
    Some(ScrollBar {
        axis,
        track,
        thumb,
        grab: [
            thumb[0] - grow,
            thumb[1] - grow,
            thumb[2] + grow,
            thumb[3] + grow,
        ],
        travel,
        overflow,
    })
}

/// Where a thumb dragged to `along` — the pointer's coordinate on the bar's own
/// axis, held `grab` pixels from the thumb's own leading edge — leaves the
/// region's offset.
///
/// **Linear in the track, clamped at both ends by the same numbers the wheel
/// clamps by**: a thumb is a picture of the offset, so dragging it is reading
/// that picture backwards and nothing else. A track with no travel (a thumb as
/// long as its track, which cannot happen while `overflow > 0`) answers zero
/// rather than dividing by it.
#[must_use]
pub fn scroll_dragged_to(bar: &ScrollBar, along: f32, grab: f32) -> f32 {
    if bar.travel <= 0.0 {
        return 0.0;
    }
    let travelled = (along - grab - bar.track_start()) / bar.travel;
    (travelled * bar.overflow).clamp(0.0, bar.overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ruling 2026-08-14, both axes: however long the document, the thumb is
    /// never drawn shorter than a hand can see and take — and the floor costs
    /// the mapping nothing, because the drag still reads linearly over the
    /// travel that remains and reaches both ends of the clamp.
    #[test]
    fn a_thumb_is_never_thinner_than_a_hand_and_still_reaches_both_ends() {
        let scale = 2.0;
        for axis in [ScrollAxis::Vertical, ScrollAxis::Horizontal] {
            let bar = scroll_bar([0.0, 0.0, 300.0, 264.0], axis, 0.0, 50_000.0, scale)
                .expect("fifty thousand pixels overflow any card");
            let length = match axis {
                ScrollAxis::Horizontal => bar.thumb[2] - bar.thumb[0],
                ScrollAxis::Vertical => bar.thumb[3] - bar.thumb[1],
            };
            assert!(
                length >= BLOCK_SCROLL_MIN_THUMB_LOGICAL_PX * scale,
                "{axis:?}: a {length}px thumb is a sliver, not a handle"
            );
            assert_eq!(
                scroll_dragged_to(&bar, bar.track_start(), 0.0),
                0.0,
                "{axis:?}: the near end of the track is still offset zero"
            );
            assert_eq!(
                scroll_dragged_to(&bar, bar.track_start() + bar.travel, 0.0),
                bar.overflow,
                "{axis:?}: the far end of the travel is still the full overflow"
            );
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bt-preview-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn buffer<'pool>(pool: &'pool PreviewPool, path: &str) -> &'pool PreviewBuffer {
        pool.get(&PreviewSource::file(path))
            .expect("the pool holds this path")
    }

    /// A worker answer for a body that never came off a disk.
    fn read(text: &str, truncated: bool) -> HeadOutcome {
        HeadOutcome::Read {
            text: text.to_owned(),
            truncated,
            mtime: None,
            content_says_text: true,
            encoding: HeadEncoding::Utf8,
            lossy: false,
        }
    }

    /// The file a buffer built by [`opened`] is reading.
    fn on_disk(buffer: &PreviewBuffer) -> &Path {
        buffer
            .source
            .file_path()
            .expect("this fixture's buffers are files")
    }

    /// A file on disk, with a buffer already reading from it.
    fn opened(dir: &Path, name: &str, body: &str) -> PreviewBuffer {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        let mut buffer = PreviewBuffer::new(PreviewSource::file(path.clone()), name.to_owned());
        buffer.accept(read_head(&path));
        buffer
    }

    /// **G-0 — the identity is a structure, and a pseudo-path was not one.**
    ///
    /// The mock-up named a git diff `git:{root}:{path}` and a graph
    /// `gitgraph:{root}`, and on this platform that grammar cannot be read back:
    /// a Windows root already carries a `:`, so `git:C:\w\repo:src/main.rs` has
    /// three colons and no rule that says which one was the separator. Worse
    /// than unreadable, it is *lossy* — there is nowhere in it for `staged`, so
    /// the two diffs of one file (working tree, and `--cached`) would be one
    /// identity and therefore one buffer, showing whichever landed last.
    ///
    /// This asserts the four things the sum type buys: colon-bearing roots stay
    /// apart, `staged` is part of who you are, a repeat of the same triple is the
    /// same buffer, and a *file* whose name happens to spell a pseudo-path is
    /// still a file. It runs through [`PreviewPool`] rather than on `==` alone
    /// because the pool is where an identity is actually used.
    ///
    /// MUTATION: collapse the key back to a bare path — give `PreviewBuffer` a
    /// `path: PathBuf` again and render each source into `git:{root}:{path}` on
    /// the way in. The staged/unstaged pair collides, `pool.len()` reads 4
    /// instead of 5, and the "a file is not a diff" lookup finds the wrong
    /// buffer.
    #[test]
    fn two_repositories_and_two_stages_are_four_identities_no_string_could_keep_apart() {
        let repo_a = PathBuf::from(r"C:\w\repo");
        let repo_b = PathBuf::from(r"D:\w\repo");
        let diff = |root: &PathBuf, against| PreviewSource::GitDiff {
            root: root.clone(),
            path: "src/main.rs".to_owned(),
            against,
        };

        let mut pool = PreviewPool::default();
        for (source, name) in [
            (diff(&repo_a, GitDiffAgainst::WorkingTree), "main.rs"),
            (diff(&repo_a, GitDiffAgainst::Index), "main.rs"),
            (diff(&repo_b, GitDiffAgainst::WorkingTree), "main.rs"),
            (diff(&repo_b, GitDiffAgainst::Index), "main.rs"),
            // The pseudo-path the mock-up would have minted for the first of
            // them, arriving as what it literally is: a file name.
            (PreviewSource::file(r"git:C:\w\repo:src/main.rs"), "main.rs"),
        ] {
            pool.open(source, name.to_owned(), &[]);
        }
        assert_eq!(
            pool.len(),
            5,
            "two repositories on two drives, two stages each, and one file that \
             merely looks like one of them"
        );

        // Asked for again, each is the buffer that is already there — the whole
        // of "finding beats making", now for an identity that has no path.
        pool.open(
            diff(&repo_a, GitDiffAgainst::Index),
            "main.rs".to_owned(),
            &[],
        )
        .dirty = true;
        assert_eq!(pool.len(), 5, "a repeat of one triple opens nothing new");
        assert!(
            pool.get(&diff(&repo_a, GitDiffAgainst::Index))
                .expect("staged, repo A")
                .dirty,
            "and it is the same buffer, edits and all"
        );
        assert!(
            !pool
                .get(&diff(&repo_a, GitDiffAgainst::WorkingTree))
                .expect("unstaged, repo A")
                .dirty,
            "while the *unstaged* diff of the same file in the same repo is a \
             different buffer — the fact the mock-up's string had no room for"
        );
        assert!(
            !pool
                .get(&diff(&repo_b, GitDiffAgainst::Index))
                .expect("staged, repo B")
                .dirty,
            "and so is the same question asked of another repository"
        );

        // A source has no path unless it is a file, and a file's path is never
        // read as anything but a path.
        assert_eq!(diff(&repo_a, GitDiffAgainst::WorkingTree).file_path(), None);
        assert_eq!(PreviewSource::GitGraph { root: repo_a }.file_path(), None);
        // G-4 — the graph is its own view and waits for no body: it is a
        // picture the chrome draws, not text a subprocess is fetching.
        let graph = PreviewBuffer::new(
            PreviewSource::GitGraph {
                root: PathBuf::from(r"C:\w\repo"),
            },
            "repo".to_owned(),
        );
        assert_eq!(graph.view(false), PreviewView::Graph);
        assert_eq!(graph.load, PreviewLoad::Ready);
        assert_eq!(graph.body_notice(), None, "a picture is not an empty diff");
        assert!(!graph.is_editable(false));
        assert_eq!(
            pool.get(&PreviewSource::file(r"git:C:\w\repo:src/main.rs"))
                .and_then(|buffer| buffer.source.file_path()),
            Some(Path::new(r"git:C:\w\repo:src/main.rs")),
            "a file that spells a pseudo-path is a file with a strange name"
        );
    }

    /// PIN (user report, 2026-08-20) — **every view names the machine that
    /// draws it, and a commit graph does not name the document pipeline.**
    ///
    /// The defect this closes is one a host could hold without noticing: a
    /// graph's `PreviewDocument` is empty by design, because the picture is
    /// chrome pushed into the body rectangle. So a surface that asked only "is
    /// there a picture" and then fell through to the document pipeline drew
    /// *nothing at all* and had no failure to report — which is exactly what
    /// the preview float did until this date, and what its head, its foot and
    /// its empty rectangle looked like.
    ///
    /// Red before the fix: the float's effective answer for
    /// [`PreviewView::Graph`] was the document pipeline. The three machines are
    /// asserted distinct because that is the claim the hosts' `match`es rest on
    /// — an enum whose arms collapse is a ladder again, and a ladder is what
    /// grew a missing rung.
    #[test]
    fn every_view_names_the_machine_that_draws_it() {
        for (view, chrome) in [
            (PreviewView::Image, PreviewChrome::Picture),
            (PreviewView::Graph, PreviewChrome::Graph),
            (PreviewView::Markdown, PreviewChrome::Document),
            (PreviewView::Table, PreviewChrome::Document),
            (PreviewView::Diff, PreviewChrome::Document),
            (PreviewView::Text, PreviewChrome::Document),
            // The "no preview" card is the document pipeline's own answer to
            // nothing, and not a fourth arrangement.
            (PreviewView::None, PreviewChrome::Document),
        ] {
            assert_eq!(view.chrome(), chrome, "{view:?} is drawn by {chrome:?}");
        }
        assert_ne!(
            PreviewChrome::Graph,
            PreviewChrome::Document,
            "a graph is not drawn by the pipeline whose answer for it is an empty body"
        );
        assert_ne!(PreviewChrome::Graph, PreviewChrome::Picture);
        // And the buffer a graph door opens answers with that view, so the two
        // halves of this ladder meet.
        let graph = PreviewBuffer::new(
            PreviewSource::GitGraph {
                root: PathBuf::from(r"C:\w\repo"),
            },
            "repo".to_owned(),
        );
        assert_eq!(graph.view(false).chrome(), PreviewChrome::Graph);
        assert_eq!(
            graph.view(true).chrome(),
            PreviewChrome::Graph,
            "and the markdown flip is not a question a graph has an answer to"
        );
    }

    /// **G-0 — git-backed content never reaches the lane that reads disks.**
    ///
    /// [`PreviewBuffer::wants_head_read`] is the only thing that puts a request
    /// on [`PreviewWorker`]'s channel, and the worker's two questions are both
    /// "what is at this path". A source with no path there would be a request
    /// nothing could answer and a `Pending` that never resolved.
    ///
    /// MUTATION: drop the `self.source.file_path().is_some()` clause from
    /// `wants_head_read` — a git diff, which is `Pending` and whose *name* is
    /// text, starts asking a disk for a file that is not there.
    #[test]
    fn a_git_backed_buffer_waits_for_the_git_worker_and_never_for_a_disk() {
        let diff = PreviewBuffer::new(
            PreviewSource::GitDiff {
                root: PathBuf::from(r"C:\w\repo"),
                path: "src/main.rs".to_owned(),
                against: GitDiffAgainst::WorkingTree,
            },
            "main.rs".to_owned(),
        );
        assert_eq!(diff.load, PreviewLoad::Pending, "nothing has answered yet");
        assert_eq!(
            diff.ftype,
            PreviewFtype::Text,
            "the name is still the name's judgement"
        );
        assert!(
            !diff.wants_head_read(),
            "but the disk is not who is being waited on"
        );
        assert_eq!(
            diff.view(false),
            PreviewView::Diff,
            "and the body it earns is decided by what it *is* (R24), not by a \
             display name ending in `.diff`"
        );
        assert!(
            !diff.is_editable(false),
            "a reading of a repository is not a second place to type into it"
        );

        // The same file, as a file, still goes down the lane it always did.
        let file = PreviewBuffer::new(
            PreviewSource::file(r"C:\w\repo\src\main.rs"),
            "main.rs".to_owned(),
        );
        assert!(file.wants_head_read());
        assert_eq!(file.view(false), PreviewView::Text);
    }

    /// **One document, one read** (user ruling 2026-08-21) — the ledger that
    /// lets a *per-frame* caller ask.
    ///
    /// Every caller of this lane before the focus column was an event: a file
    /// opened, a tab restored, a hand resting on a row. The card that projects a
    /// background tab's preview seat is not — it is looked at sixty times a
    /// second — and `PreviewLoad::Pending` cannot tell "asked" from "about to be
    /// asked" (read its own doc comment), so a caller on that beat would re-read
    /// the file on every frame until the answer landed.
    ///
    /// So the question itself is filed, exactly as [`crate::files::DirNode`]'s
    /// `Pending` files a directory's: one ledger, on the buffer, and
    /// [`PreviewBuffer::claim_head_read`] is the only door that writes it.
    ///
    /// **And a refusal is not retried.** The failure states are answers, so the
    /// door stays shut over them for the reason it stays shut over a body that
    /// arrived: there is nothing left to ask.
    ///
    /// MUTATION: let `claim_head_read` return `wants_head_read()` without filing
    /// anything — the second frame reads the file again.
    #[test]
    fn a_head_read_is_claimed_once_and_a_refusal_is_not_retried() {
        let mut buffer = PreviewBuffer::new(
            PreviewSource::file(r"C:\w\repo\notes.md"),
            "notes.md".to_owned(),
        );
        assert!(
            buffer.wants_head_read(),
            "nobody has asked for this body yet"
        );
        assert!(
            buffer.claim_head_read().is_some(),
            "the first caller takes the read"
        );
        assert!(
            buffer.claim_head_read().is_none(),
            "and every caller after it finds the question already asked"
        );
        assert!(
            !buffer.wants_head_read(),
            "a question outstanding is not a question to ask"
        );
        assert_eq!(
            buffer.load,
            PreviewLoad::Pending,
            "the body is still on its way, which is what the pane is drawing"
        );

        buffer.accept(HeadOutcome::Refused(PreviewRefusal::Fault(
            PreviewFault::PermissionDenied,
        )));
        assert!(
            buffer.claim_head_read().is_none(),
            "a refusal is an answer, and the card draws the sentence it earns \
             rather than asking again"
        );

        // A body that arrives is the same shut door, by the other clause.
        let mut read = PreviewBuffer::new(
            PreviewSource::file(r"C:\w\repo\main.rs"),
            "main.rs".to_owned(),
        );
        assert!(read.claim_head_read().is_some());
        read.accept(HeadOutcome::Read {
            text: "fn main() {}\n".to_owned(),
            truncated: false,
            mtime: None,
            content_says_text: true,
            encoding: HeadEncoding::Utf8,
            lossy: false,
        });
        assert!(
            read.claim_head_read().is_none(),
            "there is nothing left to ask"
        );
    }

    /// ① One file, one buffer — a second open of the same path is the same
    /// buffer, edits and all.
    ///
    /// Mutation: make [`PreviewPool::open`] push unconditionally instead of
    /// looking for the path first.
    #[test]
    fn a_second_open_of_the_same_path_is_the_same_buffer() {
        let mut pool = PreviewPool::default();
        pool.open(PreviewSource::file(r"C:\w\a.rs"), "a.rs".to_owned(), &[])
            .dirty = true;
        pool.open(PreviewSource::file(r"C:\w\a.rs"), "a.rs".to_owned(), &[]);
        assert_eq!(pool.len(), 1);
        assert!(buffer(&pool, r"C:\w\a.rs").dirty);
    }

    /// ② The cap evicts the oldest clean buffer nobody is showing, and nothing
    /// else.
    ///
    /// Mutation: drop the `!buffer.dirty` clause, or the `displayed` clause,
    /// from the eviction predicate.
    #[test]
    fn the_cap_evicts_the_oldest_clean_unshown_buffer() {
        let mut pool = PreviewPool::default();
        for index in 0..PV_BUFFER_CAP {
            let source = PreviewSource::file(format!(r"C:\w\f{index}.rs"));
            pool.open(source, format!("f{index}.rs"), &[]);
        }
        // The oldest is dirty and the second oldest is on screen, so the third
        // is the first evictable one.
        pool.get_mut(&PreviewSource::file(r"C:\w\f0.rs"))
            .unwrap()
            .dirty = true;
        let shown = vec![PreviewSource::file(r"C:\w\f1.rs")];
        pool.open(
            PreviewSource::file(r"C:\w\new.rs"),
            "new.rs".to_owned(),
            &shown,
        );
        assert_eq!(pool.len(), PV_BUFFER_CAP);
        assert!(pool.get(&PreviewSource::file(r"C:\w\f0.rs")).is_some());
        assert!(pool.get(&PreviewSource::file(r"C:\w\f1.rs")).is_some());
        assert!(pool.get(&PreviewSource::file(r"C:\w\f2.rs")).is_none());
        assert!(pool.get(&PreviewSource::file(r"C:\w\new.rs")).is_some());
    }

    /// ② (b) When everything left is dirty or on screen, nothing is evicted.
    ///
    /// Mutation: replace the `else { break }` in [`PreviewPool::open`] with a
    /// `remove(0)`.
    #[test]
    fn a_pool_of_dirty_buffers_grows_past_the_cap_rather_than_lose_one() {
        let mut pool = PreviewPool::default();
        for index in 0..=PV_BUFFER_CAP {
            let source = PreviewSource::file(format!(r"C:\w\f{index}.rs"));
            pool.open(source.clone(), format!("f{index}.rs"), &[]);
            pool.get_mut(&source).unwrap().dirty = true;
        }
        assert_eq!(pool.len(), PV_BUFFER_CAP + 1);
    }

    /// PIN (P19/P120) — **the pool's two dirty questions, and the one answer
    /// that must not be the other.**
    ///
    /// The header's count badge asks "is anything I am *not* showing dirty",
    /// because the pane already wears its own dot; the three gates ask "what
    /// would be lost", which includes the buffer on screen. Folding them would
    /// light the badge for the file you are looking at (a fact already stated
    /// beside it) or, far worse, leave the file you are looking at out of the
    /// gate that is about to discard it.
    ///
    /// MUTATIONS:
    /// ① drop the `shown` filter — the badge assertion goes red;
    /// ② apply the filter unconditionally — the gate assertion goes red, and it
    ///    is the one that loses work.
    #[test]
    fn the_pool_answers_two_different_dirty_questions() {
        let mut pool = PreviewPool::default();
        for name in ["a.txt", "b.md", "c.rs"] {
            let source = PreviewSource::file(format!(r"C:\w\{name}"));
            pool.open(source.clone(), name.to_owned(), &[]);
            pool.get_mut(&source).unwrap().dirty = name != "c.rs";
        }
        let shown = PreviewSource::file(r"C:\w\a.txt");
        // The badge: everything dirty except the one on screen.
        assert_eq!(
            pool.dirty_names(Some(&shown)).collect::<Vec<_>>(),
            vec!["b.md"]
        );
        // The gates: everything dirty, in the pool's own order, by name.
        assert_eq!(
            pool.dirty_names(None).collect::<Vec<_>>(),
            vec!["a.txt", "b.md"]
        );
        // And a pool with nothing dirty asks nothing of anybody.
        pool.get_mut(&shown).unwrap().dirty = false;
        pool.get_mut(&PreviewSource::file(r"C:\w\b.md"))
            .unwrap()
            .dirty = false;
        assert_eq!(pool.dirty_names(None).count(), 0);
        pool.clear();
        assert_eq!(pool.len(), 0);
    }

    /// **A shut discards the edits, not the history** (slice 7).
    ///
    /// The shut gate is the one gate whose pool has somewhere to go afterwards:
    /// the tab is about to be written to `session.json`, and its pool is the
    /// list of files next launch's switcher will show. `clear()` there answered
    /// one question by silently deciding a second — measured on the real
    /// machine, where a shut with one dirty buffer wrote `"pool": []` and a
    /// three-file history came back empty.
    ///
    /// MUTATION: put `clear()` back in `discard_dirty` and the survivors
    /// assertion goes red; drop the `retain` predicate's `!` and the gate can
    /// ask its question forever.
    #[test]
    fn a_discarded_edit_takes_its_own_buffer_and_leaves_the_history() {
        let mut pool = PreviewPool::default();
        for name in ["a.txt", "b.md", "c.rs"] {
            let source = PreviewSource::file(format!(r"C:\w\{name}"));
            pool.open(source.clone(), name.to_owned(), &[]);
            pool.get_mut(&source).unwrap().dirty = name == "b.md";
        }
        pool.discard_dirty();
        assert_eq!(
            pool.buffers().map(|b| b.name.as_str()).collect::<Vec<_>>(),
            vec!["a.txt", "c.rs"],
            "the clean history survives, in its own order"
        );
        assert_eq!(
            pool.dirty_names(None).count(),
            0,
            "and the gate has nothing left to ask about, so it cannot re-raise"
        );
    }

    /// ③ The extension table, class by class.
    ///
    /// Mutation: move `"svg"` out of [`IMAGE_EXTENSIONS`], or drop the
    /// `name.starts_with('.')` clause.
    #[test]
    fn the_extension_table_answers_each_class() {
        for name in ["a.png", "a.jpg", "a.jpeg", "a.svg", "a.gif", "a.webp"] {
            assert_eq!(preview_ftype(name), PreviewFtype::Image, "{name}");
        }
        assert_eq!(preview_ftype("README.md"), PreviewFtype::Markdown);
        assert_eq!(preview_ftype("cases.csv"), PreviewFtype::Table);
        // **The page class** (user ruling 2026-08-23). `a.html` stood in the
        // list below until then; it is here now, and the rest of that list has
        // not moved — see `a_name_that_says_page_is_a_page_in_this_table_too`.
        for name in ["a.html", "a.htm", "a.pdf"] {
            assert_eq!(preview_ftype(name), PreviewFtype::Web, "{name}");
        }
        for name in [
            "a.rs",
            "a.py",
            "a.js",
            "a.ts",
            "a.json",
            "a.toml",
            "a.txt",
            "a.gitignore",
            "Cargo.lock",
            "a.yml",
            "a.yaml",
            "a.diff",
            "a.patch",
        ] {
            assert_eq!(preview_ftype(name), PreviewFtype::Text, "{name}");
        }
        // A name that is nothing but an extension is a dotfile, and dotfiles are
        // text.
        assert_eq!(preview_ftype(".gitignore"), PreviewFtype::Text);
        assert_eq!(preview_ftype(".env"), PreviewFtype::Text);
        for name in ["a.exe", "a.dll", "a", "a.zip", "a.PNG.zip"] {
            assert_eq!(preview_ftype(name), PreviewFtype::Unknown, "{name}");
        }
        // The table is case-insensitive on the extension.
        assert_eq!(preview_ftype("A.PNG"), PreviewFtype::Image);
        assert_eq!(preview_ftype("A.RS"), PreviewFtype::Text);
    }

    /// PIN (W2 slice 5) - **the disk moved under a body, so the body is read
    /// again - without being taken off the glass, and never over unsaved work.**
    ///
    /// Three claims, and each is a separate decision this slice had to make:
    ///
    /// 1. a stale buffer wants a head read again, so the watcher's news joins
    ///    the same one-question lane every other door uses;
    /// 2. its `load` stays `Ready` and its `content` stays where it is, because
    ///    `PreviewLoad::Pending` is what makes the pane print "Loading <name>"
    ///    and a file saved in an editor must not make the page flash away and
    ///    come back sixty milliseconds later;
    /// 3. **a buffer with unsaved edits is not re-read.** The person's text is
    ///    the newer of the two, and a watcher that overwrote it would let a save
    ///    in another window destroy work in this one. The disagreement is
    ///    reported by ruling 8-9's `disk_mtime` check at the moment of saving,
    ///    which is the moment somebody can answer it.
    ///
    /// RED GATE: make `mark_stale` set `load = Pending` and the second claim
    /// fails; drop its `dirty` guard and the third does.
    #[test]
    fn a_saved_file_is_read_again_without_unloading_it_and_never_over_an_edit() {
        let mut buffer = PreviewBuffer::new(PreviewSource::file(r"D:\notes\a.md"), "a.md".into());
        assert!(buffer.claim_head_read().is_some(), "the opening read");
        buffer.accept(HeadOutcome::Read {
            text: "# one\n".into(),
            truncated: false,
            mtime: None,
            content_says_text: true,
            encoding: HeadEncoding::Utf8,
            lossy: false,
        });
        assert_eq!(buffer.load, PreviewLoad::Ready);
        assert!(!buffer.wants_head_read(), "nothing is owed");

        assert!(buffer.mark_stale(), "the disk moved");
        assert!(buffer.wants_head_read(), "so the head is owed again");
        assert_eq!(
            buffer.load,
            PreviewLoad::Ready,
            "and the pane is not sent back to `Loading`"
        );
        assert_eq!(
            buffer.content.as_deref(),
            Some("# one\n"),
            "the body stays on the glass until the new one lands"
        );
        assert!(
            !buffer.mark_stale(),
            "and a second notification about the same unread change owes nothing new"
        );

        assert!(buffer.claim_head_read().is_some());
        assert!(!buffer.wants_head_read(), "one question, once");
        buffer.accept(HeadOutcome::Read {
            text: "# two\n".into(),
            truncated: false,
            mtime: None,
            content_says_text: true,
            encoding: HeadEncoding::Utf8,
            lossy: false,
        });
        assert!(!buffer.wants_head_read(), "and the answer closes it");

        // The edited buffer. The disk is not the authority here.
        buffer.edit_content(|content| {
            content.push_str("mine\n");
            true
        });
        assert!(buffer.dirty);
        assert!(
            !buffer.mark_stale(),
            "a buffer with unsaved edits is not re-read from underneath"
        );
        assert!(!buffer.wants_head_read());
    }

    /// PIN (W2 slice 5) - **the lanes with no head to read say so.**
    ///
    /// `mark_stale` answers whether anything was owed, and three kinds of
    /// content owe nothing however loudly the folder they live in speaks: a
    /// picture, whose pixels come down the decode lane; a name this window has
    /// no reader for; and a page, which is not a file at all and takes an
    /// engine `Reload` instead (that half is `WebMachine::reload`).
    #[test]
    fn a_picture_a_page_and_an_unreadable_name_owe_no_re_read() {
        let mut picture =
            PreviewBuffer::new(PreviewSource::file(r"D:\shots\a.png"), "a.png".into());
        assert!(!picture.mark_stale());
        let mut unknown = PreviewBuffer::new(PreviewSource::file(r"D:\bin\a.exe"), "a.exe".into());
        assert!(!unknown.mark_stale());
        let mut page = PreviewBuffer::new(
            PreviewSource::Web("http://localhost:5173/app".into()),
            "App".into(),
        );
        assert!(!page.mark_stale());
        assert!(!page.wants_head_read(), "there is no disk to ask");
    }

    /// PIN (W2 slice 5) - **`.htm` and `.html` are one object in every table.**
    ///
    /// The account this pays was opened by the head's hand-off arrow and
    /// recorded in `docs/handoff/HANDOFF-2026-08-21.md` section 5, item 18:
    /// the path-side predicate has read both spellings since the day it was
    /// written (Windows registers them against the same handler) while this
    /// table listed only `html`. So a `.htm` file drew the "no preview for this
    /// file type" card *and* the head's hand-off arrow at the same time: one
    /// pane, two buttons, one door.
    ///
    /// **The class the two agree on is now `Web`** (user ruling 2026-08-23) —
    /// this test says what it always said, that the two spellings are one
    /// object, and it says it about the class they are both in today.
    ///
    /// MUTATION: take `"htm"` back out of [`PAGE_EXTENSIONS`].
    #[test]
    fn the_two_spellings_of_a_page_are_one_file_type() {
        assert_eq!(preview_ftype("timeline.htm"), PreviewFtype::Web);
        assert_eq!(preview_ftype("timeline.html"), PreviewFtype::Web);
        assert_eq!(preview_ftype("TIMELINE.HTM"), PreviewFtype::Web);
        // And the neighbour that must not be swept up with them: an extension is
        // the real one and never a substring.
        assert_eq!(preview_ftype("index.htmlx"), PreviewFtype::Unknown);
    }

    /// PIN — **a name that says page is a page here too** (user ruling
    /// 2026-08-23, "一个名字只该有一个含义"; `docs/DESIGN.md` §7.10 ⑥).
    ///
    /// The account slice ⑤ opened knowingly: `path_opens_as_a_page` routed
    /// `.html`, `.htm` and `.pdf` onto the engine's lane while this table called
    /// the first two `Text` and the third `Unknown`. So one `.pdf` row had two
    /// answers at once — the hover card said "no preview" and a double-click
    /// opened the page — and the card was the one that was lying.
    ///
    /// The near-misses are the same three the routing table refuses, and they
    /// are here rather than only there because a second reading of "which names
    /// are pages" is exactly the thing the two tables just stopped having.
    ///
    /// RED GATE: drop the `pdf` entry from [`PAGE_EXTENSIONS`] and the first
    /// group fails on `report.pdf` alone — which is how much of this feature PDF
    /// is; drop the whole table and every line of the first group fails.
    #[test]
    fn a_name_that_says_page_is_a_page_in_this_table_too() {
        for name in [
            "index.html",
            "index.htm",
            "INDEX.HTM",
            "report.pdf",
            "REPORT.PDF",
            "中文页.html",
        ] {
            assert_eq!(preview_ftype(name), PreviewFtype::Web, "{name}");
        }
        // The neighbours a substring reading would sweep up, and the one a
        // careless ordering would: `.html` as a *whole name* is a dotfile, which
        // `Path::extension` — and therefore [`path_names_a_page`] — reads
        // as having no extension at all (§7.1.5j ⑦(e)). Two tables, one answer.
        assert_eq!(preview_ftype("index.htmlx"), PreviewFtype::Unknown);
        assert_eq!(preview_ftype("notes.pdfx"), PreviewFtype::Unknown);
        assert_eq!(preview_ftype("report.html.txt"), PreviewFtype::Text);
        assert_eq!(preview_ftype(".html"), PreviewFtype::Text);
        assert_eq!(preview_ftype(".pdf"), PreviewFtype::Text);
        // And a page is not an editable surface: what is on the glass belongs to
        // the engine.
        assert!(!is_editable("index.html", PreviewFtype::Web, false));
        assert!(!is_editable("report.pdf", PreviewFtype::Web, true));
    }

    /// PIN — **no video spelling is a page, because the engine will not host one
    /// at the top level** (measured 2026-08-25; `docs/DESIGN.md` §7.16).
    ///
    /// This is a *negative* pin and it is deliberately a pin rather than an
    /// absence. The 2026-08-25 ticket asked for `mp4` here on PDF's own argument
    /// — the engine has a player, this window has none — and the real window
    /// answered: a top-level `file:` navigation to `clip.mp4` or
    /// `screencast.webm` completes as `WebErrorStatus · ConnectionAborted` and
    /// draws the 「did not respond」 card, because WebView2 has no viewer for a
    /// media response, turns it into a download, and every download is cancelled
    /// unconditionally at the platform bridge. The same file plays with controls
    /// as a `<video>` **inside** a page in the same seat. So the class stays as
    /// it is until something hosts the file inside a page, and this test is what
    /// tells whoever builds that where the one line is.
    ///
    /// # Something now does host one inside a page, and the line did not move
    ///
    /// 2026-08-27's second ruling built exactly the thing this name was reserving
    /// room for: the play verb writes a shell page and hands the recording to the
    /// `<video controls>` inside it (§7.23 ⑩). **And [`PAGE_EXTENSIONS`] did not
    /// gain a row**, which is the whole shape of that answer. What is on the page
    /// lane is the *shell* — a `.html` this window wrote, already a member of this
    /// class by its own spelling — and the recording is a **subresource**, which
    /// is not a lane at all. A video is still not a page: no door opens one as
    /// one, the address bar still refuses one, and the seat's rail still spells
    /// the recording rather than the shell.
    ///
    /// So the exception this test's name anticipated has arrived and this test is
    /// unchanged, because it was always asserting the right thing. What would
    /// make it red is what would have made it red on the day it was written: a
    /// build that tried to play a video by navigating **to** it.
    ///
    /// RED GATE: put `mp4` (or `webm`, or `m4v`) into [`PAGE_EXTENSIONS`] on the
    /// argument that the engine can play it, and this goes red with the reason
    /// written on it.
    #[test]
    fn no_video_spelling_is_a_page_until_something_hosts_it_inside_one() {
        for name in [
            "clip.mp4",
            "CLIP.MP4",
            "trailer.m4v",
            "screencast.webm",
            "clip.mov",
            "clip.mkv",
            "clip.avi",
            "clip.wmv",
            "clip.flv",
            "录屏.mp4",
        ] {
            assert_ne!(
                preview_ftype(name),
                PreviewFtype::Web,
                "{name}: WebView2 aborts a top-level media navigation, so this \
                 lane would replace the honest refusal card with a browser error"
            );
        }
        // **Where they land instead** (user ruling 2026-08-27; §7.23). Until
        // that day the answer was `Unknown` for every one of them and the card
        // was the "no preview for this file type" refusal. Now three of the
        // spellings above have a class of their own — a face this window can
        // draw without an engine — and the rest are still the refusal. Neither
        // half is [`PreviewFtype::Web`], which is the whole of what this test
        // is about and the reason both halves are asserted here rather than
        // somewhere a reader of the ruling would not find them.
        assert_eq!(preview_ftype("clip.mp4"), PreviewFtype::Video);
        assert_eq!(preview_ftype("screencast.webm"), PreviewFtype::Video);
        // **And `.mov`, `.mkv`, `.avi`, `.wmv` since route B opened them**
        // (2026-08-28; §7.44 ⑥). All four were refused a lane while the player
        // was a browser; all four were opened by the platform's own decoder and
        // all four are videos now — and none of them is a page, which is what
        // this test is about.
        for name in ["clip.mov", "clip.mkv", "clip.avi", "clip.wmv"] {
            assert_eq!(preview_ftype(name), PreviewFtype::Video, "{name}");
        }
        assert_eq!(preview_ftype("clip.mpg"), PreviewFtype::Unknown);
    }

    /// RED — **the video class has one list, and the two readings of it agree on
    /// every entry** (user ruling 2026-08-27; §7.23).
    ///
    /// The page class's own pin said one file down, and it is here for the
    /// identical reason: [`preview_ftype`] asks the table of a *name* and
    /// [`path_names_a_video`] asks it of a *path*, and while a class has two
    /// readings one of them can drift — which is the whole account §7.10 ⑥
    /// pays. Every rule that governs the page reading governs this one, and each
    /// is asserted rather than assumed: the real extension and never a
    /// substring, case ignored, and a whole name of `.mp4` is a dotfile with no
    /// extension at all.
    ///
    /// RED GATE: spell the class again anywhere — a `matches!(ext, "mp4" |
    /// "m4v" | "webm")` beside the table — and drop one spelling from the copy;
    /// the two halves of this test disagree on exactly that name.
    #[test]
    fn the_video_class_reads_the_same_from_a_name_and_from_a_path() {
        for name in [
            "clip.mp4",
            "CLIP.MP4",
            "trailer.m4v",
            "screencast.WebM",
            "capture.MOV",
        ] {
            assert_eq!(preview_ftype(name), PreviewFtype::Video, "{name}");
            assert!(
                path_names_a_video(std::path::Path::new(&format!(r"D:\shots\{name}"))),
                "{name}"
            );
        }
        for name in [
            // Outside the class by measurement, not by omission: no fixture has
            // been opened in either container, so neither has a lane.
            "clip.mpg",
            "clip.flv",
            // The two neighbours a substring reading would sweep up.
            "clip.mp4.txt",
            "clip.webmx",
            // A whole name of `.mp4` has no extension at all (§7.1.5j ⑦(e)),
            // and the dotfile arm above this one in `preview_ftype` says so
            // first — which is the same answer the path reading gives without
            // either knowing about the other.
            ".mp4",
        ] {
            assert_ne!(preview_ftype(name), PreviewFtype::Video, "{name}");
            assert!(
                !path_names_a_video(std::path::Path::new(&format!(r"D:\shots\{name}"))),
                "{name}"
            );
        }
        // And the class is disjoint from the page's, which is what makes the
        // order of the two questions in `preview_ftype` a formality rather than
        // a rule somebody could get wrong.
        for video in VIDEO_EXTENSIONS {
            assert!(
                !PAGE_EXTENSIONS.iter().any(|(page, ..)| *page == video),
                "{video} cannot be both a video and a page"
            );
        }
    }

    /// RED — **every member of the class plays, and the class is the seven that
    /// were opened** (route B slice ②, 2026-08-28; §7.44 ⑥).
    ///
    /// This replaces `every_video_has_a_face_and_only_the_measured_three_have_a_player`,
    /// and the replacement is the ruling. That test asserted a *second* column —
    /// a member with a face and no player — which existed only because the still
    /// came from Media Foundation and the playback came from Chromium. One
    /// decoder now answers both, so the column has no member and is gone; what
    /// is asserted instead is that the two questions **cannot** be asked
    /// separately, because there is only one predicate to ask.
    ///
    /// The seven are the seven that were opened on the machine — every one of
    /// them handed to `Engine::open`, three frames drawn and the playhead past
    /// 0.41s. `.mov`, `.mkv`, `.avi` and `.wmv` are the four that were outside
    /// the playable set on 2026-08-27 and are inside it now, and they are named
    /// here one at a time so that removing a row is a red test rather than a
    /// quieter product.
    ///
    /// RED GATE ①: take any of the four back out of [`VIDEO_EXTENSIONS`] and the
    /// first block fails on that name — which is the state that shipped in
    /// `next12`, where a `.mov` drew a face under a line saying it could not be
    /// played and an `.mkv` drew nothing at all.
    /// RED GATE ②: put a `path_names_a_playable_video` back beside
    /// [`path_names_a_video`] that answers differently for one row, and the
    /// second block names it: there is one predicate, and a class whose face and
    /// whose play button read two functions is the drift this ruling ended.
    #[test]
    fn every_name_in_the_class_plays_and_the_class_is_the_seven_that_were_opened() {
        let path = |name: &str| std::path::PathBuf::from(format!(r"D:\shots\{name}"));
        // ① The seven, including the four that route A could not play.
        for name in [
            "clip.mp4",
            "trailer.m4v",
            "screencast.webm",
            "capture.mov",
            "CAPTURE.MOV",
            "episode.mkv",
            "ancient.avi",
            "recording.wmv",
        ] {
            assert!(
                path_names_a_video(&path(name)),
                "{name} was opened on the machine and must be in the class"
            );
            assert_eq!(preview_ftype(name), PreviewFtype::Video, "{name}");
        }
        // ② One predicate. The face and the play button read the same function,
        // so a name cannot be drawable-but-not-playable by construction.
        for name in ["capture.mov", "episode.mkv", "clip.mp4"] {
            assert_eq!(
                path_names_a_video(&path(name)),
                preview_ftype(name) == PreviewFtype::Video,
                "{name}: the class and the lane are one answer"
            );
        }
        // ③ And the table is still a table, not "anything with a dot".
        for name in ["notes.md", "report.html", "clip.mpg", "clip.flv"] {
            assert!(!path_names_a_video(&path(name)), "{name}");
        }
    }

    /// PIN — **the counter under a video reads like a counter.**
    ///
    /// MUTATION: zero-pad the minutes and `0:06` becomes `00:06`, which is a
    /// duration nobody writes; drop the seconds' padding and `1:05` becomes
    /// `1:5`.
    #[test]
    fn a_length_is_said_the_way_a_player_says_it() {
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(6_200), "0:06");
        assert_eq!(format_duration(65_000), "1:05");
        assert_eq!(format_duration(600_000), "10:00");
        assert_eq!(format_duration(3_600_000), "1:00:00");
        assert_eq!(format_duration(7_384_000), "2:03:04");
        // Truncated and never rounded: a clip does not reach the second it has
        // not got to.
        assert_eq!(format_duration(59_600), "0:59");
    }

    /// RED — **the two lines a video prints, and the one it falls back to**
    /// (user ruling 2026-08-27; §7.23).
    ///
    /// One function for the glance card and for the preview pane, so the whole
    /// of what either surface says about a recording is asserted in one place.
    ///
    /// The last case is the slice's degradation path and the reason the ruling
    /// asked for a fallback at all: a container this machine has no decoder for
    /// gives up no frame, no length and no resolution, and the card must still
    /// be a card. What is left is the format and the size — which is poorer than
    /// a picture and is not the "No preview for this file type" refusal this
    /// class was invented to stop showing.
    ///
    /// RED GATE: delete the `first.is_empty()` fallback and a video whose frame
    /// would not decode goes back to a card with one line on it — the size, and
    /// nothing that says what the file even is.
    #[test]
    fn a_video_says_how_long_how_large_and_how_big() {
        assert_eq!(
            video_fact_lines(
                Some("mp4"),
                VideoFacts {
                    duration_ms: Some(6_200),
                    native: Some((1920, 1080)),
                    bytes: Some(12_582_912),
                },
            ),
            [
                Some("0:06 \u{b7} 1920 \u{d7} 1080".to_owned()),
                Some("12.0 MB".to_owned())
            ]
        );
        // A fact that never arrives is left unsaid and nothing moves up to fill
        // its place: the length alone still reads as a length.
        assert_eq!(
            video_fact_lines(
                Some("webm"),
                VideoFacts {
                    duration_ms: None,
                    native: Some((640, 480)),
                    bytes: None,
                },
            ),
            [Some("640 \u{d7} 480".to_owned()), None]
        );
        // Nothing decoded at all: the format, upper-cased, and the size.
        assert_eq!(
            video_fact_lines(
                Some("webm"),
                VideoFacts {
                    duration_ms: None,
                    native: None,
                    bytes: Some(4_096),
                },
            ),
            [Some("WEBM".to_owned()), Some("4 KB".to_owned())]
        );
        // And a file with no extension and no answer says nothing rather than
        // printing an empty chip where a format would be.
        assert_eq!(video_fact_lines(None, VideoFacts::default()), [None, None]);
    }

    /// PIN — **the page class has one list, and the two readings of it agree on
    /// every entry** (user ruling 2026-08-25).
    ///
    /// §7.10 ⑥ was written because the class had two lists and a `.pdf` fell
    /// between them: the hover card said "no preview" while a double click
    /// opened the page. The second ruling of that day fixed the *symptom* by
    /// pointing one at the other; this pins the shape that makes the class
    /// unable to split again — [`path_names_a_page`] and [`preview_ftype`] read
    /// the same table, so there is no second place to spell a member wrong.
    ///
    /// It asks through [`crate::path_opens_as_a_page`] rather than through
    /// [`path_names_a_page`] directly, because the claim is about *that* name:
    /// the predicate the whole of `main.rs` routes on has to be this table and
    /// not a copy of it.
    ///
    /// RED GATE: give [`crate::path_opens_as_a_page`] a list of its own again.
    #[test]
    fn every_member_of_the_page_class_is_a_page_to_both_readings() {
        for (extension, ..) in PAGE_EXTENSIONS {
            let name = format!("subject.{extension}");
            assert_eq!(preview_ftype(&name), PreviewFtype::Web, "{name}");
            assert!(
                crate::path_opens_as_a_page(std::path::Path::new(&format!(r"D:\work\{name}"))),
                "{name}"
            );
            // And upper case is the same member, on both sides.
            let shouted = format!("SUBJECT.{}", extension.to_ascii_uppercase());
            assert_eq!(preview_ftype(&shouted), PreviewFtype::Web, "{shouted}");
            assert!(crate::path_opens_as_a_page(std::path::Path::new(&format!(
                r"D:\work\{shouted}"
            ))));
        }
        // A leading-dot whole name is not a page to either, and neither needs a
        // clause for it: `Path::extension` says it has no extension, and the
        // dotfile arm of `preview_ftype` runs before the page arm.
        for (whole, ..) in PAGE_EXTENSIONS {
            let name = format!(".{whole}");
            assert_eq!(preview_ftype(&name), PreviewFtype::Text, "{name}");
            assert!(
                !crate::path_opens_as_a_page(std::path::Path::new(&name)),
                "{name}"
            );
        }
    }

    /// PIN — **a page-named file is never refused for its type** (user ruling
    /// 2026-08-23; `docs/DESIGN.md` §7.10 ⑥).
    ///
    /// This is the trap the ruling had to walk past. Teaching [`preview_ftype`]
    /// to answer `Web` by name puts every `PreviewSource::File` whose name is a
    /// page into an arm that used to say `Refused(PreviewRefusal::Type)` — so a
    /// `.html` arriving from a drop, from the switcher or from a session file
    /// would have stopped showing its source and started showing a card saying
    /// this window has no reader for it. That is a worse answer than the one it
    /// replaced, and it is not what the ruling asked for.
    ///
    /// The answer is that no such buffer is ever *shown*: every door goes
    /// through `Runtime::open_preview_source_on`, which turns a page-named
    /// source back onto the engine's lane. What is pinned here is the other
    /// half — that if one is built anyway, its load is not a verdict about the
    /// file's kind, and nothing reads a page off a disk as text.
    ///
    /// RED GATE: give [`PreviewFtype::Web`] `Refused(PreviewRefusal::Type)` back
    /// in [`PreviewBuffer::new`]'s file arm.
    #[test]
    fn a_page_named_file_is_never_refused_for_its_type() {
        for (path, name) in [
            (r"D:\site\index.html", "index.html"),
            (r"D:\site\index.htm", "index.htm"),
            (r"D:\reports\report.pdf", "report.pdf"),
        ] {
            let buffer = PreviewBuffer::new(PreviewSource::file(path), name.to_owned());
            assert_eq!(buffer.ftype, PreviewFtype::Web, "{name}");
            assert_ne!(
                buffer.load,
                PreviewLoad::Refused(PreviewRefusal::Type),
                "a page whose page lane refused it earns the disk's own answer, \
                 never a sentence about its kind: {name}"
            );
            assert!(
                !buffer.wants_head_read(),
                "and this window never reads a page off a disk as text: {name}"
            );
        }
        // A share is still the network card, and it is answered before the type
        // is even looked at — so the one refusal a page name can carry is the
        // one that is about the path rather than about the kind.
        let share = PreviewBuffer::new(
            PreviewSource::file(r"\\server\share\index.html"),
            "index.html".to_owned(),
        );
        assert_eq!(
            share.load,
            PreviewLoad::Refused(PreviewRefusal::NetworkPath)
        );
        // And the regression half, **as §7.32 left it**: a name with no reader
        // at all is no longer refused *here* — it waits for its own bytes — but
        // it is still refused for its type when they come back saying binary,
        // which is the card this line has always been about. See
        // `an_unreadable_type_asks_the_disk_once_and_refuses_on_the_answer`.
        let mut unknown =
            PreviewBuffer::new(PreviewSource::file(r"C:\w\a.exe"), "a.exe".to_owned());
        assert_eq!(unknown.load, PreviewLoad::Pending);
        unknown.accept(HeadOutcome::Refused(PreviewRefusal::Binary));
        assert_eq!(unknown.load, PreviewLoad::Refused(PreviewRefusal::Type));
    }

    /// PIN (user ruling 2026-08-25; `docs/DESIGN.md` §7.10 ⑥) — **a glance over
    /// a page whose bytes are text reads them, and reads them as text.**
    ///
    /// The sentence above this test — "this window never reads a page off a disk
    /// as text" — was true of every surface until the hover card, and it is
    /// still true of every surface that can *show* a page. The card cannot: it
    /// has no engine and never will, and one line about double-clicking is not an
    /// answer to "what is in this file". So the glance's own buffer reads `.html`
    /// the way it reads `.rs`, which is what puts the source in the card without
    /// a second reader, a second lane or a second cap being written anywhere.
    ///
    /// `.pdf` is the control and it is the whole reason the table has two rows
    /// rather than a `!= pdf`: nothing in it is text, so nothing is read, and the
    /// card states facts instead.
    ///
    /// RED GATE: return `Self::new(source, name)` from
    /// [`PreviewBuffer::glancing`] — the `.html` half comes back `Web`, wants no
    /// read, and the card that was showing markup a moment ago shows an empty
    /// box for ever.
    #[test]
    fn a_glance_reads_a_pages_source_when_the_page_is_made_of_text() {
        let glance = |path: &str, name: &str| {
            PreviewBuffer::glancing(PreviewSource::file(path), name.to_owned())
        };
        for (path, name) in [
            (r"D:\site\index.html", "index.html"),
            (r"D:\site\index.htm", "index.htm"),
            (r"D:\site\INDEX.HTM", "INDEX.HTM"),
        ] {
            let buffer = glance(path, name);
            assert_eq!(
                buffer.ftype,
                PreviewFtype::Text,
                "a page made of text is read as text: {name}"
            );
            assert!(
                buffer.wants_head_read(),
                "and the read is really asked for: {name}"
            );
            assert_eq!(
                preview_view(name, buffer.ftype, false),
                PreviewView::Text,
                "and it is drawn as source rather than as nothing: {name}"
            );
        }
        // A page with no text in it is left exactly as a pane would have it: no
        // read, no document, and the card's own facts instead.
        let pdf = glance(r"D:\reports\report.pdf", "report.pdf");
        assert_eq!(pdf.ftype, PreviewFtype::Web);
        assert!(!pdf.wants_head_read(), "nothing reads a PDF as text");
        // The three names that merely look like pages keep the answers the page
        // class already gives them — the real extension and never a substring.
        for (path, name, ftype) in [
            (r"D:\site\index.htmlx", "index.htmlx", PreviewFtype::Unknown),
            (
                r"D:\site\report.html.txt",
                "report.html.txt",
                PreviewFtype::Text,
            ),
            (r"D:\site\.html", ".html", PreviewFtype::Text),
        ] {
            assert_eq!(glance(path, name).ftype, ftype, "{name}");
        }
        // A share's `.html` keeps the network refusal §7.1.3 has always shown:
        // the promotion is of a buffer that was going to sit empty, never of one
        // that already has its answer.
        let share = glance(r"\\server\share\index.html", "index.html");
        assert_eq!(
            share.load,
            PreviewLoad::Refused(PreviewRefusal::NetworkPath),
            "a page on a share is refused before its bytes are anybody's business"
        );
        assert!(!share.wants_head_read(), "and no disk is dialled for it");
        // A repository's own reading of a page-named file has no disk to read
        // and is a diff whatever its name says.
        let composed = PreviewBuffer::glancing(
            PreviewSource::GitDiff {
                root: PathBuf::from(r"D:\repo"),
                path: "design/ui-mockup.html".to_owned(),
                against: GitDiffAgainst::WorkingTree,
            },
            "ui-mockup.html".to_owned(),
        );
        assert_eq!(composed.ftype, PreviewFtype::Web);
        assert_eq!(composed.view(false), PreviewView::Diff);
        assert!(!composed.wants_head_read());
    }

    /// PIN — **the page class's second column is read off the path's real
    /// extension**, which is the same reading its first column gets.
    ///
    /// MUTATION: ask the *name* for its extension instead (`rfind('.')`) and the
    /// dotfile case answers `Source` — a file whose whole name is `.html` becomes
    /// a page, which is the one spelling §7.1.5j ⑦(e) keeps out of the class at
    /// both readings.
    #[test]
    fn what_a_glance_shows_of_a_page_is_a_property_of_the_class() {
        let glance = |path: &str| path_page_glance(Path::new(path));
        assert_eq!(glance(r"D:\site\index.html"), Some(PageGlance::Source));
        assert_eq!(glance(r"D:\site\index.htm"), Some(PageGlance::Source));
        assert_eq!(glance(r"D:\site\INDEX.HTM"), Some(PageGlance::Source));
        assert_eq!(glance(r"D:\reports\report.pdf"), Some(PageGlance::Facts));
        assert_eq!(glance(r"D:\reports\REPORT.PDF"), Some(PageGlance::Facts));
        assert_eq!(glance(r"D:\site\index.htmlx"), None);
        assert_eq!(glance(r"D:\site\report.html.txt"), None);
        assert_eq!(glance(r"D:\site\.html"), None);
        assert_eq!(glance(r"D:\notes\notes.md"), None);
        // **One table, and this is the assertion that keeps it one.** Every
        // member of the page class has a column entry and nothing else has one:
        // a spelling added to the class without a decision about what a glance
        // shows of it would be a card with nothing to draw.
        for name in ["index.html", "index.htm", "report.pdf"] {
            let path = PathBuf::from(format!(r"D:\site\{name}"));
            assert_eq!(
                path_names_a_page(&path),
                path_page_glance(&path).is_some(),
                "the two readings of one table disagree about {name}"
            );
        }
    }

    /// ④ `editable` names the surface that actually edits.
    ///
    /// Mutation: return `true` for [`PreviewFtype::Table`], or delete the
    /// [`is_diff_name`] guard.
    #[test]
    fn editable_is_the_surface_that_actually_edits() {
        assert!(is_editable("a.rs", PreviewFtype::Text, false));
        assert!(!is_editable("cases.csv", PreviewFtype::Table, false));
        assert!(!is_editable("a.diff", PreviewFtype::Text, false));
        assert!(!is_editable("a.patch", PreviewFtype::Text, false));
        assert!(!is_editable("a.PATCH", PreviewFtype::Text, false));
        // **Both faces of a Markdown file edit** (T5, §7.1.3t). The flip used to
        // be the whole of the answer here — a rendered page had no caret in it,
        // so it had nothing to type into — and now the block under the caret is
        // the file's own bytes on either face.
        assert!(is_editable("README.md", PreviewFtype::Markdown, false));
        assert!(is_editable("README.md", PreviewFtype::Markdown, true));
        assert!(!is_editable("a.png", PreviewFtype::Image, true));
        assert!(!is_editable("a.exe", PreviewFtype::Unknown, true));
        // **And the refusals are the name's, on both faces** (T5 ⑥): a diff and
        // a table have no source face to fall back to and never had a caret, so
        // turning the rendered page on must not turn them on with it.
        assert!(!is_editable("changes.md.diff", PreviewFtype::Text, false));
        assert!(!is_editable("cases.csv", PreviewFtype::Table, true));
    }

    /// ⑤ A NUL in the head is a binary file, whatever its name claims.
    ///
    /// Mutation: delete the `head.contains(&0)` guard in [`read_head`].
    #[test]
    fn a_nul_in_the_head_is_a_binary_file() {
        let dir = scratch("binary");
        let path = dir.join("looks-like-text.txt");
        std::fs::write(&path, b"MZ\x90\x00\x03text after the nul").unwrap();
        assert_eq!(
            read_head(&path),
            HeadOutcome::Refused(PreviewRefusal::Binary)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ⑥ A file past the head limit is read short and says so.
    ///
    /// Mutation: `take(PREVIEW_HEAD_BYTES as u64)` instead of `+ 1`, which makes
    /// every file look complete.
    #[test]
    fn a_file_past_the_head_limit_is_read_short_and_says_so() {
        let dir = scratch("big");
        let big = dir.join("big.txt");
        std::fs::write(&big, "x".repeat(PREVIEW_HEAD_BYTES + 4096)).unwrap();
        match read_head(&big) {
            HeadOutcome::Read {
                text, truncated, ..
            } => {
                assert!(truncated);
                assert_eq!(text.len(), PREVIEW_HEAD_BYTES);
            }
            other => panic!("expected a truncated read, got {other:?}"),
        }
        let small = dir.join("small.txt");
        std::fs::write(&small, "one line\n").unwrap();
        assert_eq!(
            read_head(&small),
            HeadOutcome::Read {
                text: "one line\n".to_owned(),
                truncated: false,
                mtime: file_mtime(&small),
                content_says_text: true,
                encoding: HeadEncoding::Utf8,
                lossy: false,
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A cut that lands inside a character drops the half rather than showing a
    /// replacement the file does not contain.
    ///
    /// **And the decode says which of those two it did** (T2 ②): the half the
    /// *limit* made is not a lossy decode — nothing about the file was lost, the
    /// reader is simply looking at less of it — while the same bytes at the real
    /// end of a file are, and only the second may be refused an edit.
    #[test]
    fn a_cut_inside_a_character_drops_the_half_it_made() {
        // "你" is three bytes; keep two of them.
        let broken = [0xE4, 0xBD];
        assert_eq!(decode_head(&broken, true), (String::new(), false));
        // The same bytes from a file that simply ends there are the file's own
        // problem, not the limit's, and are shown lossily.
        assert_eq!(
            decode_head(&broken, false),
            ("\u{fffd}".to_owned(), true),
            "and a body this window had to invent a character for says so"
        );
        // A whole character at the cut survives.
        let whole = [0xE4, 0xBD, 0xA0];
        assert_eq!(decode_head(&whole, true), ("\u{4f60}".to_owned(), false));
    }

    /// ⑦ A network path is refused without a read.
    ///
    /// Mutation: make [`bt_transcript::paths::may_read_unasked`] answer from
    /// `starts_with(r"\\")` on the string, which drags `\\?\C:\…` in with it.
    #[test]
    fn a_network_path_is_refused_without_a_read() {
        assert!(!is_readable_unasked(Path::new(r"\\server\share\notes.txt")));
        assert!(!is_readable_unasked(Path::new(
            r"\\?\UNC\server\share\notes.txt"
        )));
        assert!(is_readable_unasked(Path::new(r"C:\w\notes.txt")));
        // **A verbatim path is refused here too, and so is a device one** (route B of the
        // untrusted-path audit, 2026-09-08). `\\?\C:\…` used to be admitted on the strength of
        // being local, which it is; what it is not is a spelling anything in this window produces,
        // and a second spelling of one file is a second answer about it in every memo keyed by
        // path. `\\.\pipe\…` was admitted on the same line and is not a file at all.
        assert!(!is_readable_unasked(Path::new(r"\\?\C:\w\notes.txt")));
        assert!(!is_readable_unasked(Path::new(r"\\.\pipe\folio-probe")));
        // **A WSL distribution's share is not a network path** (user ruling 2026-09-07, §7.30): the
        // filesystem behind it runs on this machine and a read of it dials nothing. It is the one
        // authority exempted, and the exemption is the share's own — a distribution named `server`
        // would still be reached through `wsl.localhost`, and `\\server\…` above is still refused.
        assert!(is_readable_unasked(Path::new(
            r"\\wsl.localhost\Ubuntu\etc\hosts"
        )));
        assert!(is_readable_unasked(Path::new(
            r"\\WSL.LOCALHOST\Ubuntu\etc"
        )));
        assert!(
            !is_readable_unasked(Path::new(r"\\wsl.localhost.example.test\Ubuntu\etc\hosts")),
            "a host that merely begins with the share's name is somebody else's machine"
        );

        let buffer = PreviewBuffer::new(
            PreviewSource::file(r"\\server\share\notes.txt"),
            "notes.txt".to_owned(),
        );
        assert_eq!(
            buffer.load,
            PreviewLoad::Refused(PreviewRefusal::NetworkPath)
        );
        assert!(!buffer.wants_head_read());
    }

    /// A type with no reader **asks the disk once** and refuses on the answer
    /// (user ruling 2026-08-27; `docs/DESIGN.md` §7.32).
    ///
    /// It used to refuse here, on the name, and this test asserted that. The
    /// ruling overturned it in one sentence — 文本由内容判定,不由扩展名 — because
    /// a list of suffixes is never finished and `.ps1` was the proof. What
    /// survives unchanged is the other half of the old assertion: **a name the
    /// tables *do* know is never sniffed**, and a network path is refused before
    /// any of this, because that refusal is about the path and not about the
    /// bytes.
    ///
    /// Mutation: give [`PreviewFtype::Unknown`] `PreviewLoad::Refused` back and
    /// the first block fails; let a `.rs` file's ftype be decided by
    /// [`PreviewBuffer::accept`] and the last block does.
    #[test]
    fn an_unreadable_type_asks_the_disk_once_and_refuses_on_the_answer() {
        let mut buffer = PreviewBuffer::new(PreviewSource::file(r"C:\w\a.exe"), "a.exe".to_owned());
        assert_eq!(
            buffer.load,
            PreviewLoad::Pending,
            "the name has no opinion, so the bytes are asked"
        );
        assert!(buffer.wants_head_read());
        assert!(buffer.claim_head_read().is_some());
        buffer.accept(HeadOutcome::Read {
            text: "MZ\u{0}".to_owned(),
            truncated: false,
            mtime: None,
            content_says_text: false,
            encoding: HeadEncoding::Utf8,
            lossy: false,
        });
        assert_eq!(
            buffer.load,
            PreviewLoad::Refused(PreviewRefusal::Type),
            "and the card it lands in is the one it always got"
        );
        assert!(!buffer.wants_head_read(), "asked once, answered once");

        let mut text = PreviewBuffer::new(PreviewSource::file(r"C:\w\a.rs"), "a.rs".to_owned());
        assert_eq!(text.load, PreviewLoad::Pending);
        assert!(text.wants_head_read());
        // A listed name is text because it is listed. The strict verdict is not
        // even consulted — which is what keeps a Latin-1 log file previewable.
        text.accept(HeadOutcome::Read {
            text: "caf\u{fffd} au lait\n".to_owned(),
            truncated: false,
            mtime: None,
            content_says_text: false,
            encoding: HeadEncoding::Utf8,
            lossy: false,
        });
        assert_eq!(text.ftype, PreviewFtype::Text);
        assert_eq!(text.load, PreviewLoad::Ready);
    }

    // ── slice 2: the read-only view family ──────────────────────────────────

    /// PIN — the dispatch asks its questions in the mock-up's order, and **a
    /// name beats a type**.
    ///
    /// `.diff` and `.patch` sit inside the text extension list, so by type they
    /// are text and by name they are a reading. The mock-up settles it by asking
    /// the name *before* the textarea (4970 before 4980), and that order is the
    /// whole of ruling 3: a diff never gets an edit surface and therefore never
    /// gets a save button.
    ///
    /// Mutation: move the diff arm below the text arm in [`preview_view`].
    #[test]
    fn a_diff_is_a_diff_by_name_even_though_it_is_text_by_type() {
        let view = |name: &str, md_source: bool| preview_view(name, preview_ftype(name), md_source);
        assert_eq!(preview_ftype("a.diff"), PreviewFtype::Text);
        assert_eq!(view("a.diff", false), PreviewView::Diff);
        assert_eq!(view("a.patch", false), PreviewView::Diff);
        assert_eq!(view("A.PATCH", false), PreviewView::Diff);
        assert_eq!(view("a.rs", false), PreviewView::Text);
        assert_eq!(view("cases.csv", false), PreviewView::Table);
        assert_eq!(view("README.md", false), PreviewView::Markdown);
        // Flipped to source, a markdown buffer is the text surface — slice 4
        // owns the control, the rule lives here.
        assert_eq!(view("README.md", true), PreviewView::Text);
        assert_eq!(view("a.png", false), PreviewView::Image);
        assert_eq!(view("a.exe", false), PreviewView::None);
    }

    /// PIN — the diff's five line classes, in the order the prefixes overlap.
    ///
    /// `---` is a deletion's prefix *and* a meta line's; `+++` likewise. The
    /// mock-up asks the three-character forms first (4973-4976) and that is the
    /// only thing keeping a diff's own header out of the red band.
    ///
    /// Mutation: test `starts_with("-")` before `starts_with("---")`.
    #[test]
    fn a_diffs_own_header_is_not_a_deletion() {
        use DiffLineKind::*;
        assert_eq!(diff_line_kind("--- a/src/main.rs"), Meta);
        assert_eq!(diff_line_kind("+++ b/src/main.rs"), Meta);
        assert_eq!(diff_line_kind("diff --git a/x b/x"), Meta);
        assert_eq!(diff_line_kind("@@ -1,7 +1,9 @@ fn main()"), Hunk);
        assert_eq!(diff_line_kind("+    let x = 1;"), Add);
        assert_eq!(diff_line_kind("-    let x = 0;"), Del);
        assert_eq!(diff_line_kind("     unchanged"), Context);
        assert_eq!(diff_line_kind(""), Context);
        assert!(diff_line_kind("+    let x = 1;").tints());
        assert!(diff_line_kind("-    let x = 0;").tints());
        assert!(!diff_line_kind("@@ -1 +1 @@").tints());
        assert!(!diff_line_kind("--- a/x").tints());
    }

    /// PIN — the small renderer's whole support surface (mock-up 4914-4941).
    ///
    /// Mutation: accept `####` as a heading (the mock-up's regex is `#{1,3}`),
    /// or stop flushing the open list before a heading.
    #[test]
    fn the_markdown_renderer_supports_exactly_what_the_mock_up_draws() {
        let doc = parse_markdown(
            "# Title\n\
             \n\
             A paragraph with `code` and **bold**.\n\
             - first\n\
             * second\n\
             ## Sub\n\
             ```rust\n\
             let x = 1;\n\
             ```\n\
             ####### not a heading\n",
        );
        assert_eq!(
            doc,
            vec![
                MarkdownBlock::Heading {
                    level: 1,
                    spans: vec![Span::plain("Title")],
                },
                MarkdownBlock::Paragraph(vec![
                    Span::plain("A paragraph with "),
                    Span::code("code"),
                    Span::plain(" and "),
                    Span::bold("bold"),
                    Span::plain("."),
                ]),
                // Both bullet characters, one list — and the list is closed by
                // the heading rather than swallowing it.
                MarkdownBlock::List {
                    ordered: None,
                    items: vec![vec![Span::plain("first")], vec![Span::plain("second")]],
                },
                MarkdownBlock::Heading {
                    level: 2,
                    spans: vec![Span::plain("Sub")],
                },
                MarkdownBlock::Code {
                    lang: Some("rust".to_owned()),
                    text: "let x = 1;".to_owned(),
                },
                // Seven hashes is not a heading in any dialect, and the ceiling
                // has to be *somewhere* or `#` would be a heading marker for a
                // line of nothing but hashes.
                MarkdownBlock::Paragraph(vec![Span::plain("####### not a heading")]),
            ]
        );
        // A fence never closed still renders, rather than eating the rest of the
        // document in silence (mock-up 4939).
        assert_eq!(
            parse_markdown("```\nunfinished\n"),
            vec![MarkdownBlock::Code {
                lang: None,
                text: "unfinished".to_owned(),
            }]
        );
        // A blank line is a separator, not a paragraph.
        assert_eq!(parse_markdown("\n\n"), vec![]);
    }

    /// PIN (user report, 2026-08-28: a paragraph of `docs/plans/release/clean-vm.md`
    /// that GitHub sets in bold printed its own `**` in this window) — **a
    /// delimiter run is matched against the whole line, code spans and all.**
    ///
    /// MUTATIONS: give each code-delimited chunk its own emphasis pass; refuse a
    /// closer that follows a Unicode punctuation character.
    #[test]
    fn a_bold_run_closes_across_the_code_spans_it_contains() {
        // The reporter's own line, joined over its fold the way a paragraph is.
        assert_eq!(
            parse_inline(
                "**本次没做的事(明确说清):没有启动任何虚机、没有下载任何 ISO、没有改 \
                 `.gitignore`、没有改任何 `.rs`。** 虚机由用户建,ISO 由用户下。"
            ),
            vec![
                Span::bold("本次没做的事(明确说清):没有启动任何虚机、没有下载任何 ISO、没有改 "),
                Span::code(".gitignore"),
                Span::bold("、没有改任何 "),
                Span::code(".rs"),
                Span::bold("。"),
                Span::plain(" 虚机由用户建,ISO 由用户下。"),
            ]
        );
    }

    /// PIN (user ruling, 2026-08-28: `*a*` and `**a**` both came out bold, since
    /// the window drew every level of emphasis in its one bold face) — **one
    /// delimiter a side is italic, two are bold, and three are both.**
    ///
    /// The whole of the two-layer reading in one line: `*a*` is
    /// [`SpanStyle::Italic`], `**b**` is [`SpanStyle::Bold`], and `***c***` is
    /// [`SpanStyle::BoldItalic`]. This is what the old single-face renderer could
    /// not say — it folded `<em>` into `<strong>` because it had no italic to set
    /// `<em>` in — and what a synthesised oblique now lets it say ([`DESIGN.md`
    /// §7.1.3i″ ⑥], [`resolve_emphasis`]).
    ///
    /// MUTATIONS: map a one-delimiter pair to bold (row 1 goes red); map a
    /// two-delimiter pair to italic (row 2); drop the nesting that makes `***c***`
    /// carry both flags (row 3).
    #[test]
    fn a_single_star_run_is_italic_and_a_double_star_run_is_bold() {
        assert_eq!(
            parse_inline("*a* **b** ***c***"),
            vec![
                Span::italic("a"),
                Span::plain(" "),
                Span::bold("b"),
                Span::plain(" "),
                Span::bold_italic("c"),
            ]
        );
    }

    /// PIN — **CommonMark's emphasis, checked against CommonMark's own examples.**
    ///
    /// One table, one row per case, and the left column is the specification's
    /// input verbatim (§6.2, "Emphasis and strong emphasis", version 0.31.2).
    /// The right column is that example's HTML read as *structure*: `<em>` is
    /// [`SpanStyle::Italic`], `<strong>` is [`SpanStyle::Bold`], and `<em>`
    /// nested in `<strong>` (or the reverse) is [`SpanStyle::BoldItalic`] — the
    /// two-layer reading this window gained on 2026-08-28. **This overturns the
    /// row expectations recorded until that day**, when the window had no italic
    /// face and every `<em>` was folded into `SpanStyle::Bold` alongside every
    /// `<strong>`; the ruling on 2026-08-28 (`DESIGN.md` §7.1.3i″ ⑥) gave
    /// emphasis a synthesised oblique, so a single-delimiter pair is now italic
    /// and a double one is bold, exactly as the two tags divide them. Every
    /// other property of the answer — which delimiters were spent, which were
    /// left as text, and where the text broke — is compared exactly.
    ///
    /// MUTATIONS, and each takes at least one row down: read only ASCII
    /// punctuation in the flanking rule (the six Chinese rows); drop the `_`
    /// word-internal restriction (`foo_bar_`, `5_6_78`, `foo__bar__`); drop the
    /// multiple-of-3 rule (`*foo**bar*`, `*foo**bar**baz*`); spend one delimiter
    /// where two are available (`**foo bar**`); spend two where one side has one
    /// (`**foo*`); match a closer that is not right-flanking (`*foo bar *`);
    /// match an opener that is not left-flanking (`a * foo bar*`); stop striking
    /// the delimiters a pair closed over (`*foo**bar*`).
    #[test]
    fn emphasis_is_matched_by_the_specifications_flanking_rules() {
        let cases: &[(&str, Vec<Span>)] = &[
            // § the plain pair, either marker: one delimiter a side is italic,
            // two are bold.
            ("*foo bar*", vec![Span::italic("foo bar")]),
            ("**foo bar**", vec![Span::bold("foo bar")]),
            ("_foo bar_", vec![Span::italic("foo bar")]),
            ("__foo bar__", vec![Span::bold("foo bar")]),
            // § a run that whitespace makes non-flanking opens and closes nothing.
            ("a * foo bar*", vec![Span::plain("a * foo bar*")]),
            ("*foo bar *", vec![Span::plain("*foo bar *")]),
            ("_ foo bar_", vec![Span::plain("_ foo bar_")]),
            ("** foo bar**", vec![Span::plain("** foo bar**")]),
            // § punctuation on one side and a letter on the other: `a*"foo"*` is
            // the specification's own row, and the opener there is refused
            // because it is followed by punctuation without being preceded by
            // any.
            ("a*\"foo\"*", vec![Span::plain("a*\"foo\"*")]),
            ("*(*foo)", vec![Span::plain("*(*foo)")]),
            // § `*` inside a word is emphasis and `_` inside a word is not.
            ("foo*bar*", vec![Span::plain("foo"), Span::italic("bar")]),
            (
                "5*6*78",
                vec![Span::plain("5"), Span::italic("6"), Span::plain("78")],
            ),
            ("foo_bar_", vec![Span::plain("foo_bar_")]),
            ("5_6_78", vec![Span::plain("5_6_78")]),
            ("foo__bar__", vec![Span::plain("foo__bar__")]),
            // § the two markers never pair with each other.
            ("_foo*", vec![Span::plain("_foo*")]),
            // § nesting, and the leftovers of a run that spent part of itself.
            // Emphasis inside emphasis is italic inside italic — still italic;
            // emphasis inside strong (`***`) is bold and italic at once.
            ("*(*foo*)*", vec![Span::italic("(foo)")]),
            ("***foo***", vec![Span::bold_italic("foo")]),
            ("*foo*bar", vec![Span::italic("foo"), Span::plain("bar")]),
            ("**foo*", vec![Span::plain("*"), Span::italic("foo")]),
            ("*foo**", vec![Span::italic("foo"), Span::plain("*")]),
            ("__foo, __bar__, baz__", vec![Span::bold("foo, bar, baz")]),
            // § the multiple-of-3 rule: the inner `**` of the first row cannot
            // pair with either single `*`, so it stays as text inside the span
            // the singles make; the second row is the same rule letting a real
            // pair through.
            ("*foo**bar*", vec![Span::italic("foo**bar")]),
            (
                "*foo**bar**baz*",
                vec![
                    Span::italic("foo"),
                    Span::bold_italic("bar"),
                    Span::italic("baz"),
                ],
            ),
            // § Chinese punctuation is punctuation. Every row here flips if the
            // flanking rule is asked of ASCII alone: 。 、 （ ） are the P
            // categories the specification names, and a table that does not know
            // them reads this prose as if the marks were letters.
            ("**中文**。", vec![Span::bold("中文"), Span::plain("。")]),
            (
                "（**中文**）",
                vec![Span::plain("（"), Span::bold("中文"), Span::plain("）")],
            ),
            // A closer preceded by punctuation and followed by a letter is not
            // right-flanking, so this line has no bold in it at all.
            ("**中文。**后面", vec![Span::plain("**中文。**后面")]),
            // The mirror: an opener followed by punctuation and preceded by a
            // letter is not left-flanking.
            ("前面**（中文）**", vec![Span::plain("前面**（中文）**")]),
            // And the case the report turns on: the same closer, followed by a
            // space, is right-flanking and does close.
            (
                "**中文。** 后面",
                vec![Span::bold("中文。"), Span::plain(" 后面")],
            ),
            // And the specification's own answer where a reader might want a
            // different one: an opener that follows a Chinese character and
            // precedes a bracket is not left-flanking, so this line has no bold
            // in it — the same answer GitHub gives, for the same reason, and a
            // renderer that "fixed" it would be disagreeing with every other
            // renderer about what the document says.
            (
                "他说**「是」**,然后走了",
                vec![Span::plain("他说**「是」**,然后走了")],
            ),
        ];
        for (source, want) in cases {
            assert_eq!(parse_inline(source), *want, "input: {source}");
        }
    }

    /// PIN — **emphasis is read over the line, and the other three passes are
    /// read first.**
    ///
    /// The rows above are prose and nothing else, which is the one shape the old
    /// per-chunk pass got right. These are the shapes it got wrong, plus the
    /// shapes it got right that the delimiter stack must not break.
    ///
    /// MUTATIONS: give each code-delimited chunk its own emphasis pass (rows 1
    /// and 2); read a delimiter run's neighbours from its chunk rather than from
    /// the line (row 3); let the emphasis pass see inside a code span, a formula
    /// or a link target (rows 4 to 6); treat `\*` as a delimiter (row 7).
    #[test]
    fn emphasis_reaches_across_the_runs_the_other_passes_claimed() {
        // A pair whose text contains a code span — the reported shape, in ASCII.
        assert_eq!(
            parse_inline("**a `b` c**"),
            vec![Span::bold("a "), Span::code("b"), Span::bold(" c"),]
        );
        // A pair whose text contains a link, and a pair around one.
        assert_eq!(
            parse_inline("**see [docs](a.md) now**"),
            vec![
                Span::bold("see "),
                Span::link("docs", "a.md"),
                Span::bold(" now"),
            ]
        );
        assert_eq!(
            parse_inline("**[docs](a.md)**"),
            vec![Span::link("docs", "a.md")]
        );
        // The character before a run that begins a chunk is the last character
        // of the chunk before it: here that is a backtick, which is
        // punctuation, so the run is right-flanking and closes.
        assert_eq!(
            parse_inline("**a `b`** c"),
            vec![Span::bold("a "), Span::code("b"), Span::plain(" c")]
        );
        // Asterisks inside a code span, a formula and a link target are never
        // delimiters — the three passes in front of this one claimed them.
        assert_eq!(
            parse_inline("`**not bold**`"),
            vec![Span::code("**not bold**")]
        );
        assert_eq!(parse_inline("$a * b * c$"), vec![Span::math("$a * b * c$")]);
        assert_eq!(parse_inline("[a](b*c*d)"), vec![Span::link("a", "b*c*d")]);
        // A marker with a backslash in front of it is the author's own, the same
        // parity the dollar is read with. The backslash stands, because this
        // renderer does not process backslash escapes at all yet — the same
        // answer `costs \$5 today` gives.
        assert_eq!(
            parse_inline("\\*not emphasis\\*"),
            vec![Span::plain("\\*not emphasis\\*")]
        );
        // Nothing to match is nothing, and it is text.
        assert_eq!(parse_inline("****"), vec![Span::plain("****")]);
        assert_eq!(parse_inline("a * b"), vec![Span::plain("a * b")]);
    }

    /// PIN (user report, 2026-08-25: formulas in a `.md` file stood as literal
    /// text in the preview) — **`$$…$$` is a block of mathematics**, on its own
    /// line or spread over several, and the delimiters are not part of it.
    ///
    /// MUTATIONS: emit the delimiters inside `source`; accept a `$$` that opens
    /// mid-word; let the multi-line arm run past a closing `$$`.
    #[test]
    fn display_dollars_open_a_block_of_mathematics() {
        assert_eq!(
            parse_markdown("$$E = mc^2$$\n"),
            vec![MarkdownBlock::Math {
                source: "E = mc^2".to_owned(),
            }]
        );
        assert_eq!(
            parse_markdown("$$\n\\begin{aligned}\na &= b \\\\\nc &= d\n\\end{aligned}\n$$\n"),
            vec![MarkdownBlock::Math {
                source: "\\begin{aligned}\na &= b \\\\\nc &= d\n\\end{aligned}".to_owned(),
            }]
        );
        // Prose either side of it keeps its own blocks, and the formula does not
        // swallow them.
        assert_eq!(
            parse_markdown("before\n\n$$x^2$$\n\nafter\n"),
            vec![
                MarkdownBlock::Paragraph(vec![Span::plain("before")]),
                MarkdownBlock::Math {
                    source: "x^2".to_owned(),
                },
                MarkdownBlock::Paragraph(vec![Span::plain("after")]),
            ]
        );
        // A block nobody closed still renders, on the same terms an unclosed
        // fence does.
        assert_eq!(
            parse_markdown("$$\nx^2\n"),
            vec![MarkdownBlock::Math {
                source: "x^2".to_owned(),
            }]
        );
    }

    /// PIN (same report) — **a fence is not mathematics**, and neither is a code
    /// span. The structural protection markdown already has is the whole of the
    /// rule: no dollar inside either is ever a delimiter.
    ///
    /// MUTATION: run the math pass over the raw line before the backtick pass.
    #[test]
    fn a_dollar_inside_code_is_a_dollar() {
        assert_eq!(
            parse_markdown("```text\n$$x^2 + y^2$$\n```\n"),
            vec![MarkdownBlock::Code {
                lang: Some("text".to_owned()),
                text: "$$x^2 + y^2$$".to_owned(),
            }]
        );
        assert_eq!(
            parse_inline("run `echo $HOME` and `$x$` twice"),
            vec![
                Span::plain("run "),
                Span::code("echo $HOME"),
                Span::plain(" and "),
                Span::code("$x$"),
                Span::plain(" twice"),
            ]
        );
    }

    /// PIN (same report) — **inline `$…$` is mathematics inside prose**, and the
    /// span keeps the delimiters it was written with so that a formula that never
    /// renders can still be printed back exactly as the author typed it.
    ///
    /// MUTATIONS: strip the delimiters into `text`; drop the "no space after the
    /// opener" rule; drop the "no space before the closer" rule.
    #[test]
    fn inline_dollars_delimit_mathematics_inside_prose() {
        assert_eq!(
            parse_inline("energy $E = mc^2$ rules"),
            vec![
                Span::plain("energy "),
                Span::math("$E = mc^2$"),
                Span::plain(" rules"),
            ]
        );
        assert_eq!(
            Span::math("$E = mc^2$").math_source(),
            Some("E = mc^2"),
            "the LaTeX a run renders is the text between its delimiters",
        );
        // The Chinese habit of writing a formula with no space around it keeps
        // both delimiters, exactly as `bt_detect` already rules for the terminal.
        assert_eq!(
            parse_inline("能量$E$的值"),
            vec![Span::plain("能量"), Span::math("$E$"), Span::plain("的值"),]
        );
        // Two runs on one line are two runs.
        assert_eq!(
            parse_inline("$a^2$ and $b^2$"),
            vec![
                Span::math("$a^2$"),
                Span::plain(" and "),
                Span::math("$b^2$"),
            ]
        );
        // Emphasis inside a formula is the formula's, not the renderer's: the
        // math pass claims the run before the asterisk pass ever sees it.
        assert_eq!(parse_inline("$a * b * c$"), vec![Span::math("$a * b * c$")]);
    }

    /// PIN (same report) — **the three things a lone dollar is not**: an escaped
    /// dollar, a price, and an opener with nothing to close it.
    ///
    /// The rule is Pandoc's `tex_math_dollars`, which is the written-down
    /// standard for dollars in a markdown document and therefore not a guess:
    /// `\$` is a literal, an opener may not be followed by whitespace, a closer
    /// may be neither preceded by whitespace nor followed by a digit.
    ///
    /// MUTATIONS, one per assertion: honour `\$` as a delimiter; drop the digit
    /// rule; drop the whitespace rules.
    #[test]
    fn a_price_and_an_escaped_dollar_are_not_mathematics() {
        assert_eq!(
            parse_inline("costs \\$5 today"),
            vec![Span::plain("costs \\$5 today")]
        );
        // The closer is followed by a digit, which is what a second price looks
        // like and what a formula never does.
        assert_eq!(
            parse_inline("this $5 and that $10"),
            vec![Span::plain("this $5 and that $10")]
        );
        assert_eq!(
            parse_inline("这件 $5 那件 $10 一共 $15"),
            vec![Span::plain("这件 $5 那件 $10 一共 $15")]
        );
        // An opener with no partner is a dollar sign.
        assert_eq!(parse_inline("echo $PATH"), vec![Span::plain("echo $PATH")]);
        // `$ x $` is not a formula: the opener is followed by a space.
        assert_eq!(parse_inline("a $ x $ b"), vec![Span::plain("a $ x $ b")]);
        // Nothing between the delimiters is nothing, not an empty formula.
        assert_eq!(parse_inline("a $$ b"), vec![Span::plain("a $$ b")]);
    }

    /// PIN (user report, 2026-08-25, second pass: two whole families in
    /// `tests/assets/latex-render-check.md` stood as literal text) —
    /// **`\[…\]` is display mathematics**, on one line or spread over several,
    /// and it is the same block `$$…$$` opens.
    ///
    /// The delimiter is GitHub's since 2022 and Pandoc's
    /// `tex_math_single_backslash` before that; it is block-only here for the
    /// same reason `$$` is, and the delimiters are markup rather than source.
    ///
    /// MUTATIONS: emit the delimiters inside `source`; accept an opener that is
    /// not the first thing on its line; let the multi-line arm run past `\]`.
    #[test]
    fn backslash_brackets_open_a_block_of_mathematics() {
        assert_eq!(
            parse_markdown("\\[\\hat{H}\\psi = E\\psi\\]\n"),
            vec![MarkdownBlock::Math {
                source: "\\hat{H}\\psi = E\\psi".to_owned(),
            }]
        );
        assert_eq!(
            parse_markdown("\\[\n\\oint_{\\partial \\Sigma} \\mathbf{E} = 0\n\\]\n"),
            vec![MarkdownBlock::Math {
                source: "\\oint_{\\partial \\Sigma} \\mathbf{E} = 0".to_owned(),
            }]
        );
        // Prose either side of it keeps its own blocks.
        assert_eq!(
            parse_markdown("before\n\n\\[x^2\\]\n\nafter\n"),
            vec![
                MarkdownBlock::Paragraph(vec![Span::plain("before")]),
                MarkdownBlock::Math {
                    source: "x^2".to_owned(),
                },
                MarkdownBlock::Paragraph(vec![Span::plain("after")]),
            ]
        );
    }

    /// PIN (same report) — **a bare mathematics environment is a block of
    /// mathematics**, `\begin{…}` first on its line through the `\end{…}` that
    /// closes it, and the environment stays in the source because the
    /// environment *is* the formula.
    ///
    /// MUTATIONS: drop the environment out of `source` the way `$$` is dropped;
    /// close on the first `\end{…}` of any name; accept any environment name.
    #[test]
    fn a_bare_mathematics_environment_is_a_block() {
        assert_eq!(
            parse_markdown("\\begin{align}\na &= b \\\\\nc &= d\n\\end{align}\n"),
            vec![MarkdownBlock::Math {
                source: "\\begin{align}\na &= b \\\\\nc &= d\n\\end{align}".to_owned(),
            }]
        );
        assert_eq!(
            parse_markdown("\\begin{pmatrix}\na & b \\\\\nc & d\n\\end{pmatrix}\n"),
            vec![MarkdownBlock::Math {
                source: "\\begin{pmatrix}\na & b \\\\\nc & d\n\\end{pmatrix}".to_owned(),
            }]
        );
        assert_eq!(
            parse_markdown("\\begin{cases}\nx, & x \\geq 0 \\\\\n-x, & x < 0\n\\end{cases}\n"),
            vec![MarkdownBlock::Math {
                source: "\\begin{cases}\nx, & x \\geq 0 \\\\\n-x, & x < 0\n\\end{cases}".to_owned(),
            }]
        );
        // A starred environment is the same environment.
        assert_eq!(
            parse_markdown("\\begin{align*}\na &= b\n\\end{align*}\n"),
            vec![MarkdownBlock::Math {
                source: "\\begin{align*}\na &= b\n\\end{align*}".to_owned(),
            }]
        );
        // The `\end` that closes is the one that matches, not the first one.
        assert_eq!(
            parse_markdown(
                "\\begin{align}\n\\begin{aligned}\na &= b\n\\end{aligned}\n\\end{align}\n"
            ),
            vec![MarkdownBlock::Math {
                source: "\\begin{align}\n\\begin{aligned}\na &= b\n\\end{aligned}\n\\end{align}"
                    .to_owned(),
            }]
        );
    }

    /// PIN (same report) — **`\(…\)` is mathematics inside prose**, and so is a
    /// mathematics environment written inside a sentence. Both keep the bytes
    /// they were written with, so a formula the engine refuses prints back
    /// exactly as the author typed it.
    ///
    /// MUTATIONS: strip the delimiters into `text`; let `\(` pair with a `\)`
    /// that is not there; hand the engine the delimiters as source.
    #[test]
    fn backslash_parentheses_delimit_mathematics_inside_prose() {
        assert_eq!(
            parse_inline("勾股 \\(a^2 + b^2 = c^2\\) 定理"),
            vec![
                Span::plain("勾股 "),
                Span::math("\\(a^2 + b^2 = c^2\\)"),
                Span::plain(" 定理"),
            ]
        );
        assert_eq!(
            Span::math("\\(a^2 + b^2 = c^2\\)").math_source(),
            Some("a^2 + b^2 = c^2"),
        );
        assert_eq!(
            parse_inline("矩阵 \\begin{pmatrix}a & b\\end{pmatrix} 在句子里"),
            vec![
                Span::plain("矩阵 "),
                Span::math("\\begin{pmatrix}a & b\\end{pmatrix}"),
                Span::plain(" 在句子里"),
            ]
        );
        // An environment carries no delimiters to drop: it is its own source.
        assert_eq!(
            Span::math("\\begin{pmatrix}a & b\\end{pmatrix}").math_source(),
            Some("\\begin{pmatrix}a & b\\end{pmatrix}"),
        );
        // Whichever comes first in the line comes first in the spans.
        assert_eq!(
            parse_inline("\\(x\\) and $y$"),
            vec![
                Span::math("\\(x\\)"),
                Span::plain(" and "),
                Span::math("$y$"),
            ]
        );
    }

    /// PIN (same report) — **the five things a backslash is not.** A fence and a
    /// code span protect their contents here exactly as they protect a dollar;
    /// an opener the author never closed is text and the scan that looks for its
    /// partner stops at the end of the paragraph rather than eating the rest of
    /// the document; `\\[2pt]` is a line break with a gap after it; and an
    /// environment that is not mathematics is not handed to a mathematics
    /// engine.
    ///
    /// MUTATIONS, one per assertion: run the backslash pass over the raw line
    /// before the backtick pass; scan for `\]` past the blank line; read the
    /// `[` of `\\[2pt]` as an opener; accept every `\begin{…}`.
    #[test]
    fn a_backslash_inside_code_and_an_unclosed_one_are_not_mathematics() {
        assert_eq!(
            parse_markdown("```text\n\\[x^2\\]\n\\begin{align}a\\end{align}\n```\n"),
            vec![MarkdownBlock::Code {
                lang: Some("text".to_owned()),
                text: "\\[x^2\\]\n\\begin{align}a\\end{align}".to_owned(),
            }]
        );
        assert_eq!(
            parse_inline("write `\\(x\\)` or `\\begin{align}a\\end{align}`"),
            vec![
                Span::plain("write "),
                Span::code("\\(x\\)"),
                Span::plain(" or "),
                Span::code("\\begin{align}a\\end{align}"),
            ]
        );
        // An opener with no partner is text, and the paragraph after it is its
        // own paragraph.
        assert_eq!(
            parse_markdown("\\[ unfinished\n\nnext paragraph\n"),
            vec![
                MarkdownBlock::Paragraph(vec![Span::plain("\\[ unfinished")]),
                MarkdownBlock::Paragraph(vec![Span::plain("next paragraph")]),
            ]
        );
        assert_eq!(
            parse_markdown("\\begin{align}\nunfinished\n\nnext paragraph\n"),
            vec![
                MarkdownBlock::Paragraph(vec![Span::plain("\\begin{align} unfinished"),]),
                MarkdownBlock::Paragraph(vec![Span::plain("next paragraph")]),
            ]
        );
        assert_eq!(
            parse_inline("open \\( and never close it"),
            vec![Span::plain("open \\( and never close it")]
        );
        // `\\` is an escaped backslash, so the `[` after it opens nothing.
        assert_eq!(
            parse_markdown("\\\\[2pt] and on\n"),
            vec![MarkdownBlock::Paragraph(vec![Span::plain(
                "\\\\[2pt] and on"
            )])]
        );
        assert_eq!(
            parse_inline("a \\\\(x\\\\) b"),
            vec![Span::plain("a \\\\(x\\\\) b")]
        );
        // An environment that sets a list is not an environment that sets a
        // formula, whatever the engine would make of it.
        assert_eq!(
            parse_markdown("\\begin{itemize}\n\\item a\n\\end{itemize}\n"),
            vec![MarkdownBlock::Paragraph(vec![Span::plain(
                "\\begin{itemize} \\item a \\end{itemize}"
            )])]
        );
    }

    /// PIN (same report) — **the user's own corpus, read as blocks.** The three
    /// tests above are the rules; this is the file the rules were reported
    /// against, and it is here because a rule can be right on the sentence it
    /// was written for and still miss the section it was written about.
    ///
    /// `include_str!` rather than a copy: a corpus that moves is a compile
    /// error, and a corpus that is edited is a test that reads the edit.
    #[test]
    fn the_latex_corpus_sets_every_section_it_promises() {
        let blocks = parse_markdown(include_str!("../../../tests/assets/latex-render-check.md"));
        let sources: Vec<&str> = blocks
            .iter()
            .filter_map(|block| match block {
                MarkdownBlock::Math { source } => Some(source.as_str()),
                _ => None,
            })
            .collect();
        // §2 — the backslash-bracket pair, delimiters off.
        assert!(
            sources.contains(&"\\hat{H}\\psi = E\\psi"),
            "§2 is display mathematics: {sources:?}",
        );
        // §5, §6 and §7 — one block each, the environment kept at both ends.
        for environment in ["align", "pmatrix", "bmatrix", "vmatrix", "cases"] {
            let head = format!("\\begin{{{environment}}}");
            let foot = format!("\\end{{{environment}}}");
            assert!(
                sources
                    .iter()
                    .any(|source| source.starts_with(&head) && source.ends_with(&foot)),
                "{environment} is one block that keeps its own ends",
            );
        }
        // And the fenced formula at the foot of the file is still a fence.
        assert!(!sources.contains(&"x^2 + y^2"));
    }

    /// PIN (user report, 2026-08-13: "做得不太好") — **the five block kinds the
    /// prototype could not draw**, each classified from its own first line.
    ///
    /// `docs/DESIGN.md` is the file the report was made against and it uses
    /// every one of them. Asserted as classification rather than as pixels,
    /// because classification is where all five of them can go wrong: a table
    /// that is really two paragraphs of pipes, a `####` that is really a
    /// paragraph of hashes, a `>` that is really prose beginning with a chevron.
    ///
    /// MUTATIONS, one per member:
    /// ① drop the `1..=6` bound back to `1..=3` — the `####` row goes red;
    /// ② accept a pipe row without looking ahead for the separator — the last
    ///    assertion in this test (a pipe row that is *not* a table) goes red;
    /// ③ drop the `ordered` split in `flush_list` — the numbered list arrives as
    ///    a bulleted one;
    /// ④ let `is_thematic_break` run before the table branch — the separator row
    ///    is eaten as a rule and the table loses its heading;
    /// ⑤ drop `strip_quote` — the quote arrives as two paragraphs with chevrons.
    ///
    /// The quote's own shape changed with the paragraph ruling of 2026-08-13 and
    /// the expectation below moved with it: its two source lines are now one
    /// quoted paragraph.
    #[test]
    fn the_five_blocks_the_prototype_could_not_draw() {
        let doc = parse_markdown(
            "#### Fourth\n\
             ##### Fifth\n\
             ###### Sixth\n\
             1. one\n\
             2. two\n\
             - bullet\n\
             > quoted\n\
             > still quoted\n\
             ---\n\
             | a | `b` |\n\
             |---|:--:|\n\
             | 1 | **2** |\n\
             \n\
             not | a | table\n",
        );
        assert_eq!(
            doc,
            vec![
                MarkdownBlock::Heading {
                    level: 4,
                    spans: vec![Span::plain("Fourth")],
                },
                MarkdownBlock::Heading {
                    level: 5,
                    spans: vec![Span::plain("Fifth")],
                },
                MarkdownBlock::Heading {
                    level: 6,
                    spans: vec![Span::plain("Sixth")],
                },
                MarkdownBlock::List {
                    ordered: Some(1),
                    items: vec![vec![Span::plain("one")], vec![Span::plain("two")]],
                },
                // The bullet does not join the numbers: two lists.
                MarkdownBlock::List {
                    ordered: None,
                    items: vec![vec![Span::plain("bullet")]],
                },
                // Two source lines, one quoted paragraph — the same join prose
                // gets (user ruling, 2026-08-13). A bare `>` is what separates
                // two of them; see
                // `a_hard_wrapped_paragraph_is_one_paragraph`.
                MarkdownBlock::Quote(vec![vec![Span::plain("quoted still quoted")]]),
                MarkdownBlock::Rule,
                MarkdownBlock::Table {
                    rows: vec![
                        vec![vec![Span::plain("a")], vec![Span::code("b")]],
                        vec![vec![Span::plain("1")], vec![Span::bold("2")]],
                    ],
                    // The separator was `|---|:--:|`: the second column asked to
                    // be centred and the first asked for nothing. Until the
                    // tables slice this pair was parsed and thrown away.
                    alignments: vec![
                        bt_detect::table::ColumnAlignment::None,
                        bt_detect::table::ColumnAlignment::Center,
                    ],
                },
                // **The pipe row with no separator under it stays prose.** This
                // is the assertion that keeps every sentence about `a | b` in a
                // shell out of a grid.
                MarkdownBlock::Paragraph(vec![Span::plain("not | a | table")]),
            ]
        );
        // A list that starts at three is a list that starts at three.
        assert_eq!(
            parse_markdown("3. third\n4. fourth"),
            vec![MarkdownBlock::List {
                ordered: Some(3),
                items: vec![vec![Span::plain("third")], vec![Span::plain("fourth")]],
            }]
        );
    }

    /// PIN — `[text](url)` renders its text and **never its url**, keeps that
    /// url to answer a press with, and the three inline passes keep their order.
    ///
    /// MUTATION ①: emit `text (url)` and the second assertion goes red — which
    /// is the one failure mode a link renderer must not have, because a printed
    /// URL is wrong under every ruling about what a click does.
    /// MUTATION ②: run the link pass before the code pass and the third
    /// assertion goes red: a bracket inside a code span stops being literal.
    /// MUTATION ③: drop the target on the floor again (`target: None`) and the
    /// first assertion goes red — the press would have nothing to resolve.
    #[test]
    fn a_link_renders_its_label_and_keeps_its_target_unprinted() {
        assert_eq!(
            parse_inline("see [the design](docs/DESIGN.md) first"),
            vec![
                Span::plain("see "),
                Span::link("the design", "docs/DESIGN.md"),
                Span::plain(" first"),
            ]
        );
        let rendered: String = parse_inline("[a](http://example.com/x)")
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(rendered, "a", "the target is not printed anywhere");
        // Backticks first: a bracket inside a code span is a bracket.
        assert_eq!(
            parse_inline("`[a](b)`"),
            vec![Span::code("[a](b)")],
            "the code pass runs before the link pass"
        );
        // Punctuation that only looks like a link stays punctuation.
        assert_eq!(
            parse_inline("a [TODO] note"),
            vec![Span::plain("a [TODO] note")]
        );
        assert_eq!(parse_inline("[unclosed"), vec![Span::plain("[unclosed")]);
    }

    /// PIN (user ruling, 2026-08-13) — **a link that names a file opens it in
    /// our own preview**; the web still goes to the browser; nothing else goes
    /// anywhere.
    ///
    /// The report was that pressing a file link opened the containing folder in
    /// Explorer. It could not have: nothing in this build read a preview link
    /// at all — [`push_link_runs`] threw the target away, so there was no click
    /// handler for one and could not be. What the user reached was the foot's
    /// Reveal, which opens a folder because that is Reveal's whole job. The
    /// ruling settles both halves: the link gets the tree's own answer (指到
    /// 文件=预览它), and 「open the containing folder」 stays with the foot.
    ///
    /// MUTATIONS:
    /// ① answer a file link with its parent directory — the first assertion
    ///    goes red, and that is the reported behaviour written down;
    /// ② resolve a relative target against the process's working directory
    ///    rather than the document's — the second goes red, and every link in
    ///    every document read from anywhere else points at nothing;
    /// ③ let any scheme through to the browser — the `mailto:` assertion goes
    ///    red and a document gets to name a handler;
    /// ④ stop folding the climb out (`directory.join(path)` raw) and the first
    ///    assertion goes red — which is the second half of the same report: the
    ///    file opened, but the foot printed
    ///    `…\preview-samples\../../docs/DESIGN.md` at the user and Explorer's
    ///    `/select` was handed it and opened the wrong folder.
    #[test]
    fn a_file_link_opens_in_the_preview_and_only_the_web_leaves_the_window() {
        let document = Path::new(r"D:\repo\test-assets\preview-samples\stress.md");
        let here = Path::new(r"D:\repo\test-assets\preview-samples");

        // ① A relative file link, resolved against the document's own folder —
        //    and the climb folded out, because this path is about to be printed
        //    in a caption and handed to another program.
        assert_eq!(
            link_action("../../docs/DESIGN.md", document),
            LinkAction::Preview(PathBuf::from(r"D:\repo\docs\DESIGN.md")),
            "a file link is a way of pointing at a file, and this window \
             previews files"
        );
        assert_eq!(
            link_action("./sample.csv", document),
            LinkAction::Preview(here.join("sample.csv")),
            "and a `.` is not part of anybody's idea of a path"
        );
        assert_eq!(
            link_action(r"..\sample.csv", document),
            LinkAction::Preview(PathBuf::from(r"D:\repo\test-assets\sample.csv")),
            "a backslash-written link is a link too"
        );
        assert_eq!(
            link_action("../../../../../../x.md", document),
            LinkAction::Preview(PathBuf::from(r"D:\x.md")),
            "and a climb past the root stops at the root, as Windows does"
        );

        // ② An absolute one stands as it is; a `file:` URL is unwrapped.
        assert_eq!(
            link_action(r"C:\notes\a.md", document),
            LinkAction::Preview(PathBuf::from(r"C:\notes\a.md")),
            "a drive letter is a path, not a scheme"
        );
        assert_eq!(
            link_action("file:///C:/notes/a%20b.md", document),
            LinkAction::Preview(PathBuf::from(r"C:\notes\a b.md")),
            "unwrapped, and its escapes undone"
        );

        // ③ The web leaves the window, and nothing else does.
        assert_eq!(
            link_action("https://example.com/x", document),
            LinkAction::Web("https://example.com/x".to_owned())
        );
        for refused in [
            "mailto:someone@example.com",
            "ftp://example.com/x",
            "javascript:alert(1)",
        ] {
            assert_eq!(
                link_action(refused, document),
                LinkAction::Nowhere,
                "{refused}: a document does not get to name a handler"
            );
        }

        // ④ An anchor is cut off a path, and an anchor alone goes nowhere —
        //    there is no within-document navigation to do yet.
        assert_eq!(
            link_action("DESIGN.md#7.1.2", document),
            LinkAction::Preview(here.join("DESIGN.md"))
        );
        assert_eq!(link_action("#section", document), LinkAction::Nowhere);
        assert_eq!(link_action("   ", document), LinkAction::Nowhere);
    }

    /// PIN (user ruling, 2026-08-13) — **a hard-wrapped source paragraph is one
    /// paragraph**, the way CommonMark says and every markdown reader draws it.
    ///
    /// The prototype made one block per *source line* (`renderMarkdownMock`,
    /// 4914-4941), and against a real document that is the seam the report was
    /// about: `docs/DESIGN.md` is written wrapped at eighty columns, so every
    /// paragraph in it came out as five separately-wrapped blocks with a
    /// paragraph gap between each pair — a page of prose printed as a page of
    /// stanzas, with the last word of each source line stranded on a line of its
    /// own whenever the pane was wide.
    ///
    /// Joining is done on the **source text, before the inline pass**, which is
    /// also what CommonMark does and what makes the second assertion here
    /// possible: emphasis opened on one source line and closed on the next is one
    /// bold run, where a per-line parser produced two literal asterisk pairs.
    ///
    /// MUTATIONS:
    /// ① go back to one block per line (`blocks.push(Paragraph(parse_inline(
    ///    line)))` in the fall-through) — the first assertion sees three
    ///    paragraphs instead of one;
    /// ② join across the blank line as well (drop the blank-line flush) — the
    ///    first assertion sees one paragraph where there must be two;
    /// ③ let a heading, a fence or a list marker be swallowed as continuation
    ///    text — the third assertion loses its block boundaries.
    #[test]
    fn a_hard_wrapped_paragraph_is_one_paragraph() {
        assert_eq!(
            parse_markdown(
                "The rule is that consecutive non-blank lines\n\
                 are one paragraph, and a blank line ends it.\n\
                 This is the third source line.\n\
                 \n\
                 A second paragraph.\n",
            ),
            vec![
                MarkdownBlock::Paragraph(vec![Span::plain(
                    "The rule is that consecutive non-blank lines \
                     are one paragraph, and a blank line ends it. \
                     This is the third source line."
                )]),
                MarkdownBlock::Paragraph(vec![Span::plain("A second paragraph.")]),
            ]
        );
        // Inline runs are parsed over the joined paragraph, so emphasis may
        // straddle a source break.
        assert_eq!(
            parse_markdown("a **bold phrase\nspanning the fold** end\n"),
            vec![MarkdownBlock::Paragraph(vec![
                Span::plain("a "),
                Span::bold("bold phrase spanning the fold"),
                Span::plain(" end"),
            ])]
        );
        // Every other block still interrupts prose on its own first line.
        assert_eq!(
            parse_markdown(
                "prose\n\
                 # heading\n\
                 prose\n\
                 - item\n\
                 prose again\n\
                 \n\
                 prose\n\
                 > quoted\n\
                 prose\n\
                 ---\n\
                 prose\n\
                 ```\n\
                 fenced\n\
                 ```\n",
            ),
            vec![
                MarkdownBlock::Paragraph(vec![Span::plain("prose")]),
                MarkdownBlock::Heading {
                    level: 1,
                    spans: vec![Span::plain("heading")],
                },
                MarkdownBlock::Paragraph(vec![Span::plain("prose")]),
                // **The continuation line joins the item**, which is the other
                // half of the ruling: a wrapped bullet is one bullet.
                MarkdownBlock::List {
                    ordered: None,
                    items: vec![vec![Span::plain("item prose again")]],
                },
                MarkdownBlock::Paragraph(vec![Span::plain("prose")]),
                MarkdownBlock::Quote(vec![vec![Span::plain("quoted")]]),
                MarkdownBlock::Paragraph(vec![Span::plain("prose")]),
                MarkdownBlock::Rule,
                MarkdownBlock::Paragraph(vec![Span::plain("prose")]),
                MarkdownBlock::Code {
                    lang: None,
                    text: "fenced".to_owned(),
                },
            ]
        );
        // A quote wrapped in the source is one quoted paragraph, for the same
        // reason and by the same join; a bare `>` separates two of them.
        assert_eq!(
            parse_markdown("> one line\n> and its fold\n>\n> a second\n"),
            vec![MarkdownBlock::Quote(vec![
                vec![Span::plain("one line and its fold")],
                vec![Span::plain("a second")],
            ])]
        );
    }

    /// PIN — the rendered view renders **the content**.
    ///
    /// P103's named lie: the prototype's first rendered view was a static mock
    /// that showed the same document whatever the buffer held. Asserted as the
    /// property rather than as a string — whatever the buffer says has to come
    /// out the other end.
    ///
    /// Mutation: make `parse_markdown` ignore its argument and return a fixed
    /// document.
    #[test]
    fn the_rendered_view_renders_the_buffers_own_text() {
        for body in ["# One", "# Two", "# Three"] {
            assert_eq!(
                parse_markdown(body),
                vec![MarkdownBlock::Heading {
                    level: 1,
                    spans: vec![Span::plain(&body[2..])],
                }]
            );
        }
    }

    /// PIN — the first row is headings and the rest are cells.
    ///
    /// Mutation: drop the `skip(1)`/`first` split so every row is a data row.
    #[test]
    fn a_tables_first_row_is_its_heading_row() {
        let rows = csv_rows("case,cols,expect\ncjk-width,2,PASS\nemoji-vs16,2,FAIL\n");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], vec!["case", "cols", "expect"]);
        assert_eq!(rows[2], vec!["emoji-vs16", "2", "FAIL"]);
        // A quoted field keeps the comma inside it. The mock-up splits naively
        // because its own fixture has no quotes; a real file does, and a grid
        // that shifts a column at the first quoted comma is not a table.
        assert_eq!(
            csv_rows("a,\"b,c\",d"),
            vec![vec!["a".to_owned(), "b,c".to_owned(), "d".to_owned()]]
        );
        assert_eq!(
            csv_rows("\"say \"\"hi\"\"\",2"),
            vec![vec!["say \"hi\"".to_owned(), "2".to_owned()]]
        );
        assert!(csv_rows("   \n").is_empty());
    }

    /// PIN — a truncated buffer says so, and a whole one says nothing.
    ///
    /// The read-only degradation §7.1.3 asks for. Slice 1 carried the fact;
    /// this is the sentence it earns.
    ///
    /// Mutation: return the notice unconditionally.
    #[test]
    fn only_a_truncated_buffer_carries_the_read_only_notice() {
        let mut buffer = PreviewBuffer::new(PreviewSource::file(r"C:\w\a.rs"), "a.rs".to_owned());
        buffer.accept(read("fn main() {}\n", false));
        assert_eq!(buffer.read_only_notice(), None);
        buffer.accept(read("fn main() {}\n", true));
        assert_eq!(buffer.read_only_notice(), Some(preview_truncated_notice()));
    }

    /// PIN (user ruling, 2026-08-15) — **the conflict phrase still says all
    /// three things it was written to say.**
    ///
    /// The ruling moved every standing notice out of the body and onto the right
    /// hand of the path foot, which meant a sentence had to become something
    /// that fits beside a path. That is exactly the edit that quietly loses a
    /// clause, and the clause at risk is the third one: a user who reads only
    /// "Not saved" believes their work is gone. It is not — the buffer survives
    /// the refusal intact, and saying so is the whole reason this string is not
    /// two words.
    ///
    /// Mutation: shorten it to "Not saved · changed on disk" — a perfectly
    /// reasonable-looking abbreviation — and the third assertion goes red.
    #[test]
    fn the_conflict_phrase_says_what_happened_what_was_not_done_and_what_survived() {
        let phrase = preview_conflict_notice();
        assert!(phrase.contains("Not saved"), "what was not done: {phrase}");
        assert!(
            phrase.contains("changed on disk"),
            "what happened: {phrase}"
        );
        assert!(
            phrase.contains("edits kept"),
            "and what is still true — the clause a shortening drops first: {phrase}"
        );
        // Short enough to share a 28px strip with a path, which is the whole
        // reason it stopped being a sentence.
        assert!(
            phrase.chars().count() < preview_truncated_notice().chars().count() * 3,
            "and it is a phrase, not a paragraph: {phrase}"
        );
    }

    /// PIN — the widest line is measured in **columns**, not bytes or chars,
    /// and after tabs have become the spaces they draw as.
    ///
    /// It is what the horizontal scroller's extent is derived from, so a wide
    /// character that measured one column would leave the end of its own line
    /// unreachable.
    ///
    /// Mutation: `line.chars().count()` instead of the display width.
    #[test]
    fn the_widest_line_is_measured_in_the_columns_it_will_draw_as() {
        let mut buffer = PreviewBuffer::new(PreviewSource::file(r"C:\w\a.rs"), "a.rs".to_owned());
        buffer.accept(read("ab\n\t\tx\n\u{4f60}\u{597d}\n", false));
        // Two tabs are eight columns, plus the `x`.
        assert_eq!(buffer.max_columns, 9);
        buffer.accept(read("\u{4f60}\u{597d}\u{4e16}\u{754c}\u{ff01}", false));
        assert_eq!(
            buffer.max_columns, 10,
            "five wide characters are ten columns"
        );
    }

    /// PIN — a tab advances to the next stop rather than to four more spaces.
    ///
    /// `tab-size: 4` (mock-up 603) names a column grid, and replacing each tab
    /// with four spaces is what misaligns every continuation line of an indented
    /// file — which is precisely what a preview of source code is for.
    ///
    /// Mutation: `repeat_n(' ', PREVIEW_TEXT_TAB_WIDTH)` instead of the computed
    /// advance.
    #[test]
    fn a_tab_advances_to_the_next_stop_rather_than_to_four_more_spaces() {
        assert_eq!(expand_tabs("\tfn main() {"), "    fn main() {");
        assert_eq!(expand_tabs("ab\tc"), "ab  c", "two columns in, two to go");
        assert_eq!(
            expand_tabs("abcd\te"),
            "abcd    e",
            "a full stop is skipped"
        );
        assert_eq!(expand_tabs("a\t\tb"), "a       b", "stops compose");
        assert_eq!(expand_tabs("no tabs here"), "no tabs here");
        // A wide character is two columns, so the stop after it is two away.
        assert_eq!(expand_tabs("\u{4f60}\tx"), "\u{4f60}  x");
    }

    /// PIN — the meta line's size is a real one.
    ///
    /// Mutation: divide by 1000 instead of 1024.
    #[test]
    fn a_files_size_reads_the_way_a_file_manager_says_it() {
        assert_eq!(format_byte_size(0), "0 B");
        assert_eq!(format_byte_size(945), "945 B");
        assert_eq!(format_byte_size(219_136), "214 KB");
        assert_eq!(format_byte_size(1024), "1 KB");
        assert_eq!(format_byte_size(5 * 1024 * 1024 + 512 * 1024), "5.5 MB");
        assert_eq!(format_byte_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    /// A size question is its own question on the same lane, and is coalesced
    /// separately from the head read of the same file.
    ///
    /// Mutation: leave `want` out of `same_target`.
    #[test]
    fn a_size_and_a_head_of_one_file_are_two_questions() {
        let dir = scratch("size");
        let path = dir.join("a.png");
        std::fs::write(&path, vec![7u8; 4096]).unwrap();
        assert_eq!(read_size(&path), Some(4096));
        assert_eq!(read_size(&dir.join("nope.png")), None);

        let (sender, receiver) = std::sync::mpsc::channel();
        let ask = |want| PreviewRequest {
            window: winit::window::WindowId::from(1_u64),
            tab: crate::TabId(1),
            source: PreviewSource::file("a.png"),
            want,
        };
        sender.send(ask(PreviewWant::Head)).unwrap();
        sender.send(ask(PreviewWant::Size)).unwrap();
        drop(sender);
        let mut asked = Vec::new();
        run_preview_worker(receiver, |request| asked.push(request.want));
        assert_eq!(
            asked,
            vec![PreviewWant::Head, PreviewWant::Size],
            "neither supersedes the other"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **Two windows asking one thing are two questions** (user report,
    /// 2026-08-23).
    ///
    /// The coalescing half of the window's half of the address. The lane keeps
    /// only the newest request per target, and `TabId(1)` is a tab in *every*
    /// window — so a target that could not tell two windows apart would let one
    /// window's question supersede the other's, and the window whose question was
    /// dropped would wait for an answer that was never coming.
    ///
    /// Mutation: leave `window` out of `same_target` and only one read is made.
    #[test]
    fn one_question_asked_by_two_windows_is_not_coalesced_into_one() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let ask = |window: u64| PreviewRequest {
            window: winit::window::WindowId::from(window),
            tab: crate::TabId(1),
            source: PreviewSource::file("plan.md"),
            want: PreviewWant::Head,
        };
        sender.send(ask(1)).unwrap();
        sender.send(ask(2)).unwrap();
        drop(sender);
        let mut asked = Vec::new();
        run_preview_worker(receiver, |request| asked.push(request.window));
        assert_eq!(
            asked,
            vec![
                winit::window::WindowId::from(1_u64),
                winit::window::WindowId::from(2_u64)
            ],
            "neither window's read supersedes the other's"
        );
    }

    // ── slice 3: quick edit ─────────────────────────────────────────────────

    /// ① The first real change dirties the buffer, and editing back to the
    /// original does not clean it.
    ///
    /// The second half is the ruling, and ticket T3 left it standing: a dot that
    /// goes out because you happened to retype what you deleted is a dot nobody
    /// can trust the one time it matters. Retyping is two entries in the log and
    /// not a road back to the saved position, so the bit stays set. What T3 did
    /// change is that there is now a second way to clean it, and it is the road
    /// back itself — see
    /// `undoing_back_to_the_last_save_cleans_the_buffer_and_a_change_away_dirties_it`.
    ///
    /// Mutation: set `dirty` from `content != original` rather than from the
    /// log's position; or drop the `if !edit(content)` guard, which dirties a
    /// file for a keystroke that changed nothing.
    #[test]
    fn the_first_real_change_dirties_the_buffer_and_nothing_cleans_it_but_a_save() {
        let mut buffer = PreviewBuffer::new(PreviewSource::file(r"C:\w\a.rs"), "a.rs".to_owned());
        buffer.accept(read("fn main() {}\n", false));
        assert!(!buffer.dirty, "a freshly read buffer is clean");

        // A no-op edit is not an edit.
        assert!(!buffer.edit_content(|_| false));
        assert!(!buffer.dirty);

        assert!(buffer.edit_content(|content| {
            content.insert_str(0, "// ");
            true
        }));
        assert!(buffer.dirty);
        assert_eq!(buffer.content.as_deref(), Some("// fn main() {}\n"));

        // Back to the words it started with — still dirty.
        assert!(buffer.edit_content(|content| {
            content.replace_range(0..3, "");
            true
        }));
        assert_eq!(buffer.content.as_deref(), Some("fn main() {}\n"));
        assert!(buffer.dirty, "editing back to the original does not clean");
    }

    /// RED (ticket T3 ③, 2026-09-10) — **the dirty bit is honest now: undoing
    /// back to the last save cleans the buffer, and a change away from it
    /// dirties it again.**
    ///
    /// The sentence this file used to carry — "the undo history a real editor
    /// would compare against does not exist here" — is what made the bit
    /// one-way. It exists, so the bit is a question about a position in it.
    ///
    /// MUTATIONS:
    /// ① set `dirty = true` in `settle_after_a_change` again — every assertion
    ///    after the first undo goes red;
    /// ② drop `undo.mark_saved()` from `PreviewBuffer::save` — the buffer is
    ///    clean only at the top of the file, which is where the log started;
    /// ③ keep the log across `take_the_disks_copy` — the last block goes red and
    ///    an undo replays an offset from a body the file has replaced.
    #[test]
    fn undoing_back_to_the_last_save_cleans_the_buffer_and_a_change_away_dirties_it() {
        let mut caret = crate::preview_edit::EditCaret::default();
        let mut buffer = PreviewBuffer::new(PreviewSource::file(r"C:\w\a.md"), "a.md".to_owned());
        buffer.accept(read("one\n", false));
        assert!(!buffer.dirty);

        let type_in = |buffer: &mut PreviewBuffer, caret: &mut _, text: &str| {
            buffer.edit_by_caret(caret, |content, caret| {
                crate::preview_edit::insert(content, caret, text)
            })
        };
        caret.place("one\n", 4, false);
        assert!(type_in(&mut buffer, &mut caret, "t"));
        assert!(type_in(&mut buffer, &mut caret, "w"));
        assert!(type_in(&mut buffer, &mut caret, "o"));
        assert_eq!(buffer.content.as_deref(), Some("one\ntwo"));
        assert!(buffer.dirty);

        // One press, because three keystrokes in a row are one run.
        let back = buffer.undo_edit().expect("there is a change to take back");
        assert_eq!(buffer.content.as_deref(), Some("one\n"));
        assert_eq!(back.caret, 4, "and the caret the run started from");
        assert!(!buffer.dirty, "back where the disk left it");

        let forward = buffer.redo_edit().expect("and it can be played again");
        assert_eq!(buffer.content.as_deref(), Some("one\ntwo"));
        assert_eq!(forward.caret, 7);
        assert!(buffer.dirty);

        // An undo is a change to the body like any other: the revision moves and
        // the widest line follows it.
        let revision = buffer.revision;
        buffer.undo_edit().expect("one more");
        assert_ne!(buffer.revision, revision);
        assert_eq!(buffer.max_columns, 3);

        // The disk's copy empties the log outright.
        assert!(type_in(&mut buffer, &mut caret, "x"));
        assert!(buffer.dirty);
        buffer.take_the_disks_copy();
        assert!(!buffer.dirty);
        assert_eq!(buffer.undo_edit(), None, "there is no history to walk");
    }

    /// RED (ticket T3 ④) — **two panes are one buffer, so an undo pressed in
    /// either takes back the buffer's last change.**
    ///
    /// The pool is what makes this true and this asserts it through the pool
    /// rather than around it: one source, two readers, one history. The caret
    /// that comes back belongs to whoever made the change — the pane pressing
    /// takes it, and the other pane's caret is nobody's business here (it is
    /// clamped into range the next time it is used).
    ///
    /// MUTATION: put the log on the pane instead — there is no pane in this test
    /// at all, which is the point: a history that needed one could not be
    /// asserted here.
    #[test]
    fn either_pane_undoes_the_one_buffers_last_change() {
        let mut caret = crate::preview_edit::EditCaret::default();
        let mut pool = PreviewPool::default();
        let source = PreviewSource::file(r"C:\w\shared.md");
        pool.open(source.clone(), "shared.md".to_owned(), &[])
            .accept(read("shared\n", false));

        // The first pane types.
        caret.place("shared\n", 7, false);
        assert!(
            pool.get_mut(&source)
                .expect("one buffer")
                .edit_by_caret(&mut caret, |content, caret| {
                    crate::preview_edit::insert(content, caret, "!")
                })
        );
        assert_eq!(
            pool.get(&source)
                .and_then(|buffer| buffer.content.as_deref()),
            Some("shared\n!")
        );

        // The second pane presses undo, and reaches the same buffer.
        let caret_back = pool
            .get_mut(&source)
            .expect("one buffer")
            .undo_edit()
            .expect("the change the other pane made");
        assert_eq!(
            pool.get(&source)
                .and_then(|buffer| buffer.content.as_deref()),
            Some("shared\n")
        );
        assert_eq!(caret_back.caret, 7, "the caret of whoever typed it");
        assert!(!pool.get(&source).expect("one buffer").dirty);
    }

    /// ⑥ An edit is counted, so a cache keyed on the count sees a change that
    /// kept the length.
    ///
    /// The named bug: `content_len` was the revision counter until this slice,
    /// and swapping one letter for another left every derived document
    /// convinced it was still looking at the old text.
    ///
    /// Mutation: stop incrementing `revision` in [`PreviewBuffer::edit_content`].
    #[test]
    fn an_edit_that_keeps_the_length_still_counts() {
        let mut buffer = PreviewBuffer::new(PreviewSource::file(r"C:\w\a.rs"), "a.rs".to_owned());
        buffer.accept(read("abc", false));
        let before = buffer.revision;
        assert!(buffer.edit_content(|content| {
            content.replace_range(2..3, "d");
            true
        }));
        assert_eq!(buffer.content.as_deref(), Some("abd"));
        assert_ne!(
            buffer.revision, before,
            "a same-length edit is still an edit"
        );
        // And the widest line follows the edit rather than the read.
        assert!(buffer.edit_content(|content| {
            content.push_str("\nlonger line");
            true
        }));
        assert_eq!(buffer.max_columns, 11);
    }

    /// The read-only degradation is enforced where the editing is.
    ///
    /// §7.1.3's "超大文件只读降级": a truncated buffer holds the first 64KB of
    /// its file, so an edit surface over it is a save button wired to
    /// `truncate`.
    ///
    /// Mutation: drop the `!self.truncated` clause from
    /// [`PreviewBuffer::is_editable`].
    #[test]
    fn a_truncated_buffer_is_read_only_however_editable_its_name_is() {
        let mut buffer = PreviewBuffer::new(PreviewSource::file(r"C:\w\a.rs"), "a.rs".to_owned());
        assert!(
            !buffer.is_editable(false),
            "a buffer with no body has no caret"
        );
        buffer.accept(read("fn main() {}\n", false));
        assert!(buffer.is_editable(false));
        buffer.accept(read("fn main() {}\n", true));
        assert!(!buffer.is_editable(false));
    }

    /// The bytes a file in this encoding holds for this text.
    ///
    /// The fixture's own spelling of the four encodings, written out here rather
    /// than borrowed from [`HeadEncoding::encode`], so that the round trip below
    /// is asserted by a second author and not by the code under test agreeing
    /// with itself.
    fn file_bytes(encoding: HeadEncoding, text: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        match encoding {
            HeadEncoding::Utf8 => bytes.extend_from_slice(text.as_bytes()),
            HeadEncoding::Utf8Bom => {
                bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
                bytes.extend_from_slice(text.as_bytes());
            }
            HeadEncoding::Utf16Le => {
                bytes.extend_from_slice(&[0xFF, 0xFE]);
                bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
            }
            HeadEncoding::Utf16Be => {
                bytes.extend_from_slice(&[0xFE, 0xFF]);
                bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
            }
        }
        bytes
    }

    /// RED (T2 ①, `docs/plans/markdown-edit/research-2026-09-10.md` §4) — **a
    /// file that says what it is keeps saying it after a save.**
    ///
    /// The reported defect: the mark is read, the body is decoded through it,
    /// and then the encoding is dropped on the floor — the buffer never held it,
    /// [`PreviewBuffer::accept`] was handed a body with the mark already
    /// stripped, and the write put UTF-8 octets down whatever the file had been.
    /// So editing one line of a UTF-16 transcript — which is what Windows
    /// PowerShell 5.1 writes, and therefore what a great many files on this
    /// platform are — silently rewrote the whole file in another encoding, and
    /// the bytes outside the edited line were all different afterwards.
    ///
    /// Sixteen fixtures: the four encodings this window reads, each on LF and on
    /// CRLF, each ending with a newline and without one. Every one of them is
    /// edited in the middle and saved, and what is asserted is the strong claim
    /// — not "it reads back the same", which a re-encode would also satisfy, but
    /// **the bytes before and after the edited span are the file's own, byte for
    /// byte**.
    ///
    /// Red gate: before the encoding rides on the buffer, the twelve marked
    /// fixtures fail on their first byte.
    #[test]
    fn a_save_writes_the_file_back_in_the_encoding_it_was_read_in() {
        let dir = scratch("encoding-round-trip");
        let mut cases = 0;
        for encoding in [
            HeadEncoding::Utf8,
            HeadEncoding::Utf8Bom,
            HeadEncoding::Utf16Le,
            HeadEncoding::Utf16Be,
        ] {
            for newline in ["\n", "\r\n"] {
                for last in ["", "\n"] {
                    let last = if last.is_empty() { "" } else { newline };
                    // Trailing whitespace on the third line and a missing final
                    // newline on half the fixtures: both already survive a read,
                    // and both are exactly what a re-encode would tidy away.
                    let text = format!("alpha{newline}beta{newline}gamma  {newline}omega{last}");
                    let original = file_bytes(encoding, &text);
                    let path = dir.join(format!("case-{cases}.txt"));
                    std::fs::write(&path, &original).unwrap();
                    cases += 1;

                    let mut buffer = PreviewBuffer::new(
                        PreviewSource::file(path.clone()),
                        "case.txt".to_owned(),
                    );
                    buffer.accept(read_head(&path));
                    assert_eq!(
                        buffer.content.as_deref(),
                        Some(text.as_str()),
                        "the fixture reads back as itself before anything is edited"
                    );

                    // One small edit in the middle, and a non-ASCII one so that
                    // the encoding has to do real work on the way out.
                    let at = text.find("beta").expect("the fixture's middle word");
                    assert!(buffer.edit_content(|content| {
                        content.replace_range(at..at + "beta".len(), "bêta");
                        true
                    }));
                    assert_eq!(buffer.save(), SaveOutcome::Saved);

                    let saved = std::fs::read(&path).unwrap();
                    let mark = file_bytes(encoding, "").len();
                    let head = file_bytes(encoding, &text[..at]);
                    let tail = file_bytes(encoding, &text[at + "beta".len()..]);
                    let tail = &tail[mark..];
                    assert!(
                        original.starts_with(&head) && original.ends_with(tail),
                        "the fixture's own bytes bracket the edit: {encoding:?}"
                    );
                    assert_eq!(
                        &saved[..head.len()],
                        &head[..],
                        "every byte before the edit is the file's own: {encoding:?}"
                    );
                    assert_eq!(
                        &saved[saved.len() - tail.len()..],
                        tail,
                        "and every byte after it: {encoding:?}"
                    );
                    assert_eq!(
                        HeadEncoding::of(&saved),
                        encoding,
                        "the mark and its endianness are still what the file said"
                    );
                }
            }
        }
        assert_eq!(cases, 16, "four encodings, two line endings, two endings");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// RED (T2 ②) — **a body this window could not read is not a body it offers
    /// to write.**
    ///
    /// [`decode_head`] is lossy on purpose: a preview that refused a file over
    /// one bad byte is a preview that refuses log files. What must not follow is
    /// an edit surface over that body, because a save would put every
    /// replacement character it invented into somebody's file — the bytes it
    /// could not read would be gone, and nothing would have said so.
    ///
    /// The reason is said through the channel a refused edit already speaks on
    /// ([`PreviewBuffer::read_only_notice`]), which is the right hand of the
    /// pane's foot.
    ///
    /// Red gate: before the lossy bit exists, the file is editable and the
    /// notice is `None`.
    #[test]
    fn a_body_that_decoded_lossily_is_shown_and_not_edited() {
        let dir = scratch("lossy");
        let path = dir.join("latin.txt");
        // Latin-1, which is not UTF-8 and is exactly what an old log file is.
        std::fs::write(&path, b"caf\xE9 latte\nand a second line\n").unwrap();

        let mut buffer =
            PreviewBuffer::new(PreviewSource::file(path.clone()), "latin.txt".to_owned());
        buffer.accept(read_head(&path));
        assert!(
            buffer
                .content
                .as_deref()
                .is_some_and(|body| body.contains(char::REPLACEMENT_CHARACTER)),
            "it is still shown — that is what lossy is for"
        );
        assert!(
            !buffer.is_editable(false),
            "and it is not written back over the bytes it could not read"
        );
        assert_eq!(
            buffer.read_only_notice(),
            Some(preview_lossy_notice()),
            "and the reason is said where a refused edit is already explained"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// RED (T2 ③, research §10 Q2 — the owner's ruling) — **the glance reads the
    /// head; asking to edit buys the rest of the file.**
    ///
    /// The cap on the *look* had become a cap on the feature: past
    /// [`PREVIEW_HEAD_BYTES`] a buffer is truncated, a truncated buffer refuses
    /// a caret, and `docs/DESIGN.md` — the document that motivates editing
    /// Markdown in this pane at all — is far over 64KB. So the read is in two
    /// stages now, and this is both of them: the glance costs one head read and
    /// the file stays read-only; the asking costs one whole-file read and the
    /// same buffer becomes editable, without ever having unloaded what was on
    /// the glass.
    ///
    /// Red gate: without the third clause of
    /// [`PreviewBuffer::wants_head_read`] and the `Whole` want beside it, the
    /// second half of the file never arrives and the buffer is read-only for
    /// ever.
    #[test]
    fn asking_to_edit_a_file_too_big_to_glance_at_buys_the_whole_of_it() {
        let dir = scratch("whole-read");
        let path = dir.join("long.md");
        let body = "a line of a long document\n".repeat(4000);
        assert!(
            body.len() > PREVIEW_HEAD_BYTES,
            "the fixture is over the cap"
        );
        std::fs::write(&path, &body).unwrap();

        let mut buffer =
            PreviewBuffer::new(PreviewSource::file(path.clone()), "long.md".to_owned());
        assert_eq!(buffer.claim_head_read(), Some(PreviewWant::Head));
        buffer.accept(read_head(&path));
        assert!(buffer.truncated, "the glance took the head and said so");
        assert!(!buffer.is_editable(true));
        assert_eq!(buffer.read_only_notice(), Some(preview_truncated_notice()));
        // **A press in the rendered page is somebody asking to edit too**
        // (T5 ①, §7.1.3t). It was not when this test was written — a rendered
        // page had no caret to put anywhere, so the flip was the only gesture
        // that could mean it — and now it is the commonest of the two: the
        // reader clicks into the text, and the click that buys the rest of the
        // file is the click that gets the caret when it lands.
        assert!(
            buffer.ask_for_the_whole_file(false),
            "a press in a rendered page asks to edit"
        );
        assert!(
            !buffer.ask_for_the_whole_file(true),
            "and asking twice buys one read: the flip after it finds the ask made"
        );
        // The head is still on the glass while the rest is on its way: nothing
        // was unloaded, so the page does not flash.
        assert!(buffer.content.is_some() && buffer.load == PreviewLoad::Ready);
        assert_eq!(buffer.claim_head_read(), Some(PreviewWant::Whole));
        buffer.accept(read_whole(&path));

        assert!(!buffer.truncated, "the whole file is here");
        assert_eq!(buffer.content.as_deref(), Some(body.as_str()));
        assert!(buffer.is_editable(true), "so there is something to type in");
        assert_eq!(buffer.read_only_notice(), None);
        assert!(!buffer.wants_head_read(), "and nothing further is owed");

        // **And it stays a whole-file reader.** A watcher's re-read that came
        // back as a head would put the reader on the first 64KB of the document
        // they are editing, with the ceiling back.
        assert!(buffer.mark_stale());
        assert_eq!(buffer.claim_head_read(), Some(PreviewWant::Whole));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// RED (T2 ③) — **past the editing ceiling the head stands and says why.**
    ///
    /// [`PREVIEW_EDIT_BYTES`] is the second read's own cap, and what happens at
    /// it is not a second truncated body: the whole-file read answers
    /// [`HeadOutcome::TooLargeToEdit`], nothing the reader is looking at is
    /// replaced, and the buffer says through the one notice channel that this is
    /// as far as asking gets. The bytes are built here rather than committed,
    /// for the obvious reason.
    ///
    /// Red gate: let `read_whole` return its truncated body and the buffer
    /// silently swaps 64KB of document for 8MB of it and is still read-only,
    /// with nothing said.
    #[test]
    fn a_file_past_the_editing_ceiling_keeps_its_head_and_says_so() {
        let dir = scratch("edit-ceiling");
        let path = dir.join("enormous.txt");
        let line = "an enormous file, one line at a time\n";
        let mut body = String::with_capacity(PREVIEW_EDIT_BYTES + line.len());
        while body.len() <= PREVIEW_EDIT_BYTES {
            body.push_str(line);
        }
        std::fs::write(&path, &body).unwrap();

        let mut buffer =
            PreviewBuffer::new(PreviewSource::file(path.clone()), "enormous.txt".to_owned());
        assert_eq!(buffer.claim_head_read(), Some(PreviewWant::Head));
        buffer.accept(read_head(&path));
        let head = buffer.content.clone().expect("the glance landed");

        assert!(
            buffer.ask_for_the_whole_file(false),
            "a text file's face edits"
        );
        assert_eq!(buffer.claim_head_read(), Some(PreviewWant::Whole));
        assert_eq!(read_whole(&path), HeadOutcome::TooLargeToEdit);
        buffer.accept(HeadOutcome::TooLargeToEdit);

        assert_eq!(
            buffer.content.as_deref(),
            Some(head.as_str()),
            "what the reader is looking at is untouched"
        );
        assert!(buffer.truncated && !buffer.is_editable(false));
        assert_eq!(buffer.read_only_notice(), Some(preview_too_large_notice()));
        assert!(
            !buffer.wants_head_read(),
            "and the answer is final — nothing asks again"
        );
        assert!(
            !buffer.ask_for_the_whole_file(false),
            "including the next time somebody presses in the body"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// PIN — **the editing ceiling's phrase names the real ceiling.**
    ///
    /// `the_read_only_fact_hangs_on_the_path_foots_right_hand` says this about
    /// [`preview_truncated_notice`] and [`PREVIEW_HEAD_BYTES`]; this is the same
    /// line held for the second number, so that a ceiling that moves cannot
    /// leave a phrase behind claiming the old one.
    ///
    /// Mutation: change [`PREVIEW_EDIT_BYTES`] and leave the string alone.
    #[test]
    fn the_editing_ceilings_phrase_names_the_size_it_is() {
        assert_eq!(
            preview_too_large_notice(),
            format!(
                "Read-only · {}",
                format_byte_size(PREVIEW_EDIT_BYTES as u64)
            ),
            "the phrase names the editing cap's real size, not a number typed twice"
        );
    }

    /// ② A save writes the body to the disk and cleans the buffer.
    ///
    /// Mutation: return [`SaveOutcome::Saved`] without calling
    /// [`save_atomically`], or leave `dirty` set.
    #[test]
    fn a_save_writes_the_body_and_cleans_the_buffer() {
        let dir = scratch("save");
        let mut buffer = opened(&dir, "notes.txt", "one\ntwo\n");
        buffer.edit_content(|content| {
            content.push_str("three\n");
            true
        });
        assert!(buffer.dirty);
        assert_eq!(buffer.save(), SaveOutcome::Saved);
        assert!(!buffer.dirty, "a saved buffer is clean");
        assert_eq!(
            std::fs::read_to_string(on_disk(&buffer)).unwrap(),
            "one\ntwo\nthree\n",
            "the bytes on the disk are the bytes in the buffer"
        );
        // The stamp moved with the write, so the very next save is not a
        // conflict with itself.
        buffer.edit_content(|content| {
            content.push_str("four\n");
            true
        });
        assert_eq!(buffer.save(), SaveOutcome::Saved);
        // Nothing is left beside the file: the staging sibling went with the
        // rename that spent it.
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "one entry in the directory, and it is the file that was saved"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ③ A write the filesystem refuses is reported, and leaves nothing behind.
    ///
    /// The refusal is injected the one way a filesystem allows without a full
    /// volume or an access-control edit: the target is a **directory**, which
    /// neither Windows nor Unix will let a file be renamed over. The buffer
    /// keeps its edits, the entry it was aimed at is untouched, and — the part
    /// that used to cost this window a temp file per retry — the staging sibling
    /// does not survive the failure.
    ///
    /// **The atomicity itself is pinned one crate over** since T2 folded the two
    /// writers into one (research §10 Q14): `bt_persist::atomic`'s own
    /// `interrupted_write_leaves_old_file_intact` stops between the two phases
    /// and asserts the target is still the old bytes, which is the crash window
    /// the staging exists for and a test that can only be written where the
    /// phases are.
    ///
    /// Mutation: return `SaveOutcome::Saved` regardless of what the writer
    /// answered, and the first assertion goes red.
    #[test]
    fn a_write_the_disk_refuses_is_reported_and_leaves_nothing_behind() {
        let dir = scratch("atomic");
        let path = dir.join("notes.txt");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("inside.txt"), "somebody else's file\n").unwrap();

        let mut buffer =
            PreviewBuffer::new(PreviewSource::file(path.clone()), "notes.txt".to_owned());
        buffer.accept(HeadOutcome::Read {
            text: "the replacement\n".to_owned(),
            truncated: false,
            mtime: file_mtime(&path),
            content_says_text: true,
            encoding: HeadEncoding::Utf8,
            lossy: false,
        });
        buffer.edit_content(|content| {
            content.push_str("and a second line\n");
            true
        });

        let SaveOutcome::Failed(said) = buffer.save() else {
            panic!("a file cannot be renamed over a directory");
        };
        assert!(!said.is_empty(), "and the window is told what happened");
        assert!(buffer.dirty, "the edits are still here");
        assert_eq!(
            std::fs::read_to_string(path.join("inside.txt")).unwrap(),
            "somebody else's file\n",
            "and what the target held was never touched"
        );
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "a refused write leaves no staging file behind to be retried into"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ④ A file the disk has moved on from is not overwritten.
    ///
    /// Ruling 8⑨'s minimum: the window says so and keeps the edits. The stamp
    /// is set to a time no file has rather than raced against the clock, so the
    /// test asserts the comparison and not the resolution of a filesystem's
    /// timestamps.
    ///
    /// Mutation: drop the `file_mtime(&self.path) != self.disk_mtime` guard,
    /// which is exactly the blind write the ruling forbids.
    #[test]
    fn a_file_that_changed_on_disk_is_not_blindly_overwritten() {
        let dir = scratch("conflict");
        let mut buffer = opened(&dir, "notes.txt", "as it was read\n");
        buffer.edit_content(|content| {
            content.push_str("and as it was edited\n");
            true
        });
        // Somebody else wrote the file after this buffer read it.
        buffer.disk_mtime = Some(SystemTime::UNIX_EPOCH);

        assert_eq!(buffer.save(), SaveOutcome::Conflict);
        assert_eq!(
            std::fs::read_to_string(on_disk(&buffer)).unwrap(),
            "as it was read\n",
            "the other writer's file is still theirs"
        );
        assert!(buffer.dirty, "and the edits are still here");
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "a refusal this early stages nothing at all"
        );

        // Re-reading the file settles the conflict, and the same save lands.
        buffer.disk_mtime = file_mtime(on_disk(&buffer));
        assert_eq!(buffer.save(), SaveOutcome::Saved);
        assert_eq!(
            std::fs::read_to_string(on_disk(&buffer)).unwrap(),
            "as it was read\nand as it was edited\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// RED (multiwindow slice E2 phase ①, acceptance gate 1 + v3 复审 ④-a) —
    /// **the quit's save branch tries every dirty buffer, and reports what each
    /// one came to.**
    ///
    /// Three buffers and three fates, injected the way the two cases already
    /// pinned above inject theirs — a stamp the disk has moved past, and a
    /// target a directory is sitting on — so nothing here is mocked: the real
    /// conflict check refuses one and the real `atomic_write` refuses another.
    ///
    /// What the case holds is the three sentences the ruling is made of. **The
    /// one that could be written is honestly clean** — it is not rolled back to
    /// make the report tidy, and its bytes are on the disk. **The two that could
    /// not are still dirty and still named**, so the window that stays open goes
    /// on showing them. And **the third buffer was tried at all**, which is the
    /// half a loop that stopped at the first failure would silently lose.
    ///
    /// Red gate: make `save_dirty` stop at the first non-`Saved` outcome and the
    /// list is one long; skip the dirty filter and the clean buffer appears in a
    /// report about unsaved work.
    #[test]
    fn the_quits_save_branch_tries_every_dirty_buffer_and_names_what_refused() {
        let dir = scratch("quit-save");
        let mut pool = PreviewPool::default();

        let mut lands = opened(&dir, "lands.txt", "one\n");
        lands.edit_content(|content| {
            content.push_str("two\n");
            true
        });
        // Somebody else wrote this one after the buffer read it.
        let mut conflicted = opened(&dir, "conflicted.txt", "theirs\n");
        conflicted.edit_content(|content| {
            content.push_str("mine\n");
            true
        });
        conflicted.disk_mtime = Some(SystemTime::UNIX_EPOCH);
        // And this one's target is a directory, which nothing can be renamed
        // over, so the atomic write refuses.
        let refused_at = dir.join("refused.txt");
        std::fs::create_dir(&refused_at).unwrap();
        let mut refused = PreviewBuffer::new(
            PreviewSource::file(refused_at.clone()),
            "refused.txt".to_owned(),
        );
        refused.accept(HeadOutcome::Read {
            text: "as it was\n".to_owned(),
            truncated: false,
            mtime: file_mtime(&refused_at),
            content_says_text: true,
            encoding: HeadEncoding::Utf8,
            lossy: false,
        });
        refused.edit_content(|content| {
            content.push_str("and as it is\n");
            true
        });
        // A clean buffer, which a save branch has no business writing at all.
        let untouched = opened(&dir, "clean.txt", "unchanged\n");

        for buffer in [lands, conflicted, refused, untouched] {
            pool.insert(buffer);
        }

        let report = pool.save_dirty();
        assert_eq!(
            report
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["lands.txt", "conflicted.txt", "refused.txt"],
            "every dirty buffer is tried, in the pool's own order, and no clean one is"
        );
        assert_eq!(report[0].1, SaveOutcome::Saved);
        assert_eq!(report[1].1, SaveOutcome::Conflict);
        assert!(matches!(report[2].1, SaveOutcome::Failed(_)));

        assert_eq!(
            std::fs::read_to_string(dir.join("lands.txt")).unwrap(),
            "one\ntwo\n",
            "what could be written is on the disk"
        );
        assert_eq!(
            pool.dirty_names(None).collect::<Vec<_>>(),
            ["conflicted.txt", "refused.txt"],
            "and only what could not is still dirty and still named"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("conflicted.txt")).unwrap(),
            "theirs\n",
            "the other writer's file is still theirs"
        );
        assert!(
            dir.join("refused.txt").is_dir()
                && std::fs::read_dir(dir.join("refused.txt"))
                    .unwrap()
                    .next()
                    .is_none(),
            "and what the write could not reach is exactly as it was found — \
             not replaced, and with no staging file dropped inside it"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A superseded question is dropped rather than asked — the files worker's
    /// coalescing, on this lane.
    ///
    /// Mutation: delete the `contains_target` check in [`run_preview_worker`].
    #[test]
    fn a_question_superseded_while_reading_is_dropped_rather_than_asked() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let ask = |path: &str| PreviewRequest {
            window: winit::window::WindowId::from(1_u64),
            tab: crate::TabId(1),
            source: PreviewSource::file(path),
            want: PreviewWant::Head,
        };
        sender.send(ask("a.rs")).unwrap();
        sender.send(ask("b.rs")).unwrap();
        sender.send(ask("a.rs")).unwrap();
        drop(sender);
        let mut asked = Vec::new();
        run_preview_worker(receiver, |request| asked.push(request.source.clone()));
        assert_eq!(
            asked,
            vec![PreviewSource::file("b.rs"), PreviewSource::file("a.rs")],
            "the superseded first question is never read"
        );
    }

    // ── G-3 ────────────────────────────────────────────────────────────────

    /// **⑤ The foot's left hand, for a document that has no path to print.**
    ///
    /// The strip asks "where is this", and a file answers with its path. A diff
    /// composed out of a repository has no such answer — but it does have the
    /// two facts that make one: *which repository*, and *where in it*. The
    /// mock-up wrote a pseudo-path here and the pseudo-path is gone (G-0), so
    /// this is what stands in its place.
    ///
    /// MUTATION: drop [`PreviewSource::composed_lead`] and let the callers fall
    /// back to `file_path().unwrap_or_default()` as they did before this slice.
    /// Every git document's foot goes blank — a strip whose whole job is saying
    /// what you are looking at, saying nothing, on the one surface where the
    /// name alone (`main.rs.diff`) does not say which of two repositories it
    /// came from.
    #[test]
    fn a_composed_document_s_foot_names_its_repository_and_its_place_in_it() {
        let root = PathBuf::from(r"D:\work\folio");
        assert_eq!(
            PreviewSource::GitDiff {
                root: root.clone(),
                path: "crates/bt-app/src/main.rs".to_owned(),
                against: GitDiffAgainst::Index,
            }
            .composed_lead()
            .as_deref(),
            Some("folio \u{b7} crates/bt-app/src/main.rs")
        );
        assert_eq!(
            PreviewSource::GitShow {
                root: root.clone(),
                hash: "a".repeat(40),
                path: "README.md".to_owned(),
            }
            .composed_lead()
            .as_deref(),
            Some("folio \u{b7} README.md"),
            "a commit's reading of a file is still that file, in that repository"
        );
        assert_eq!(
            PreviewSource::GitGraph { root: root.clone() }
                .composed_lead()
                .as_deref(),
            Some("folio"),
            "a graph is about the whole repository and names no file"
        );
        // D6 (v2 ②) — a range is one file in one repository too, however many
        // ends the diff has.
        assert_eq!(
            PreviewSource::GitDiffRange {
                root: root.clone(),
                a: "a".repeat(40),
                b: Some("b".repeat(40)),
                path: "README.md".to_owned(),
            }
            .composed_lead()
            .as_deref(),
            Some("folio \u{b7} README.md")
        );
        assert_eq!(
            PreviewSource::file(r"D:\work\folio\README.md").composed_lead(),
            None,
            "a file's foot is its path, and this function does not answer for it"
        );
        // A repository at a drive root has no last component to name, and the
        // honest answer there is the root itself rather than an empty word.
        assert_eq!(
            PreviewSource::GitGraph {
                root: PathBuf::from(r"D:\"),
            }
            .composed_lead()
            .as_deref(),
            Some(r"D:\")
        );

        // The working-tree file a git document is *about* — the one door
        // Explorer can be pointed at, and the only file verb a composed
        // document keeps.
        assert_eq!(
            PreviewSource::GitDiff {
                root: root.clone(),
                path: "crates/bt-app/src/main.rs".to_owned(),
                against: GitDiffAgainst::WorkingTree,
            }
            .repo_file(),
            Some(root.join("crates/bt-app/src/main.rs"))
        );
        let range = PreviewSource::GitDiffRange {
            root: root.clone(),
            a: "a".repeat(40),
            // The far end absent is the working tree, and it changes nothing
            // about which file this is.
            b: None,
            path: "crates/bt-app/src/main.rs".to_owned(),
        };
        assert_eq!(
            range.repo_file(),
            Some(root.join("crates/bt-app/src/main.rs"))
        );
        assert!(range.is_git());
        assert_eq!(
            range.file_path(),
            None,
            "a range has no file on a disk to save, reveal or write to a session"
        );
        assert_eq!(
            PreviewBuffer::new(range, "main.rs".to_owned()).view(false),
            PreviewView::Diff
        );
        assert_eq!(PreviewSource::GitGraph { root }.repo_file(), None);
    }

    /// **⑥ Two honest answers a diff has to be able to give.**
    ///
    /// git says "Binary files … differ" in one line and that line *is* the diff;
    /// and a question with no answer — an untracked file's working-tree diff, a
    /// commit that did not touch the file — produces nothing at all, which a
    /// pane must say rather than draw as a blank.
    ///
    /// MUTATION: let [`PreviewBuffer::body_notice`] answer for any empty body
    /// rather than only a composed one. An empty *file* — a zero-byte
    /// `.gitkeep`, opened from the tree — then reads "No changes to show",
    /// which is a sentence about a repository printed over a file that is
    /// simply empty.
    #[test]
    fn an_empty_diff_says_so_and_a_binary_one_keeps_git_s_own_line() {
        let source = PreviewSource::GitDiff {
            root: PathBuf::from(r"D:\repo"),
            path: "logo.png".to_owned(),
            against: GitDiffAgainst::WorkingTree,
        };
        let mut binary = PreviewBuffer::new(source.clone(), "logo.png.diff".to_owned());
        binary.accept(read(
            "diff --git a/logo.png b/logo.png\nBinary files a/logo.png and b/logo.png differ\n",
            false,
        ));
        assert_eq!(binary.load, PreviewLoad::Ready);
        assert_eq!(
            binary.body_notice(),
            None,
            "there is a body, so it is drawn"
        );
        assert_eq!(binary.view(false), PreviewView::Diff);
        assert!(
            binary
                .content
                .as_deref()
                .expect("the body is git's own words")
                .contains("Binary files a/logo.png and b/logo.png differ"),
            "git already says this in one line and nothing here rewrites it"
        );

        let mut empty = PreviewBuffer::new(source.clone(), "logo.png.diff".to_owned());
        empty.accept(read("", false));
        assert_eq!(empty.load, PreviewLoad::Ready);
        assert_eq!(
            empty.body_notice(),
            Some(git_document_empty()),
            "a diff with nothing in it says so"
        );

        // An empty *file* is not an empty diff, and says nothing.
        let mut file = PreviewBuffer::new(
            PreviewSource::file(r"D:\repo\.gitkeep"),
            ".gitkeep".to_owned(),
        );
        file.accept(read("", false));
        assert_eq!(file.body_notice(), None);

        // And a repository that would not answer prints git's own refusal on
        // the same line — not the "no preview" card, whose one control opens a
        // *file* and a composed document has none.
        let mut refused = PreviewBuffer::new(source, "logo.png.diff".to_owned());
        refused.decline("fatal: bad object deadbee".to_owned());
        assert_eq!(refused.body_notice(), Some("fatal: bad object deadbee"));
        assert_eq!(
            refused.refusal(),
            None,
            "and the card, with its file-shaped way out, stays down"
        );
    }

    // ── slice: the page's measure (Typora's GitHub theme, 2026-08-16) ────────

    /// PIN — **the page is set at Typora's proportions of *our* body size**, not
    /// at Typora's pixels.
    ///
    /// The user's report on 2026-08-16 was that a long Chinese/English document
    /// reads dense beside Typora: tight leading, paragraphs that touch, headings
    /// glued to the prose above them, inline code the size of the words around
    /// it. Every one of those is a ratio, and the ratios are `github.css`'s. This
    /// test is the mapping table in executable form — if a number here moves, the
    /// table in `docs/DESIGN.md` §7.1.3i moved with it or one of the two is
    /// lying.
    ///
    /// MUTATION: put `line_height` back on the window's chrome constant of 1.4
    /// and the first assertion goes red, which is the reported density in one
    /// number.
    #[test]
    fn the_rendered_page_carries_typoras_ratios_of_the_houses_own_body_size() {
        let metrics = markdown_metrics(1.0);
        assert_eq!(
            metrics.font_size, 13.0,
            "the base is unchanged, deliberately"
        );

        // body { line-height: 1.6 } — was CHROME_LINE_HEIGHT, 1.4.
        assert_eq!(metrics.line_height, 21.0);
        // p, blockquote, ul, ol, table, pre { margin: 0 0 16px } — 1em.
        assert_eq!(metrics.paragraph_gap, 13.0);
        // 77em — github.css's `#write { max-width: 860px }` re-decided for this
        // window's own body size (user ruling, 2026-09-07).
        assert_eq!(metrics.measure, 1001.0);

        // h1 … h6 { font-size: 2 / 1.5 / 1.25 / 1 / .875 / .85 em }.
        assert_eq!(metrics.heading_font(1), 26.0);
        assert_eq!(metrics.heading_font(2), 19.5);
        assert_eq!(metrics.heading_font(3), 16.25);
        assert_eq!(metrics.heading_font(4), 13.0);
        assert_eq!(metrics.heading_font(5), 13.0 * 0.875);
        assert_eq!(metrics.heading_font(6), 13.0 * 0.85);
        // … { line-height: 1.25 }, tighter than the body's 1.6.
        assert_eq!(metrics.heading_line_height(1), 33.0);
        assert_eq!(metrics.heading_line_height(4), 16.0);
        // … { margin: 24px 0 16px } — 1.5em above, 1em below.
        assert_eq!(metrics.heading_margin_top, 20.0);
        assert_eq!(metrics.heading_margin_bottom, 13.0);

        // ul, ol { padding-left: 30px } and li + li { margin-top: .25em }.
        assert_eq!(metrics.list_indent, 24.0);
        assert_eq!(metrics.list_item_gap, 3.0);

        // blockquote { border-left: 4px; padding: 0 15px }.
        assert_eq!(metrics.quote_bar, 3.0, "4px on 16 is 3px on 13 — unmoved");
        assert_eq!(metrics.quote_padding_x, 12.0);
        assert_eq!(metrics.quote_indent, 15.0);

        // code, pre { font-size: 85% }; pre { line-height: 1.45; padding: 16px }.
        assert_eq!(metrics.code_font, 13.0 * 0.85);
        assert_eq!(metrics.code_line_height, 16.0);
        assert_eq!(metrics.code_padding_x, 13.0);
        assert_eq!(metrics.code_padding_y, 13.0);
        assert_eq!(metrics.code_margin, 13.0);

        // hr { margin: 24px 0 }, one hairline tall — the house's own weight.
        assert_eq!(metrics.rule_margin, 20.0);
        assert_eq!(metrics.rule_thickness, 1.0);

        // table th, td { padding: 6px 13px; border: 1px }.
        assert_eq!(metrics.table_padding_x, 11.0);
        assert_eq!(metrics.table_padding_y, 5.0);
        assert_eq!(metrics.table_border, 1.0);
    }

    /// PIN — **every one of those is a ratio and survives the scale**, which is
    /// the whole reason they are written as ems rather than as pixels.
    ///
    /// At 150% nothing may be pinned to a logical pixel it happened to measure
    /// once; the measure in particular has to grow with the type, or a document
    /// on a 4K monitor would hold a column of 702 physical pixels with a mile of
    /// nothing beside it.
    ///
    /// MUTATION: write any of these as a `* scale` of a hard pixel count instead
    /// of a ratio of `font_size` and the multiples below stop lining up.
    #[test]
    fn the_measure_and_its_metrics_are_ratios_and_therefore_scale() {
        let one = markdown_metrics(1.0);
        let half = markdown_metrics(1.5);
        assert_eq!(half.font_size, one.font_size * 1.5);
        assert_eq!(
            half.measure,
            (13.0f32 * 1.5 * PREVIEW_PROSE_MEASURE_EM).round()
        );
        assert_eq!(
            half.line_height,
            (half.font_size * PREVIEW_MD_LINE_HEIGHT).round()
        );
        assert_eq!(half.paragraph_gap, half.font_size.round());
        assert_eq!(half.code_font, half.font_size * PREVIEW_MD_CODE_FONT_RATIO);
        assert!(half.list_indent > one.list_indent);
        assert!(half.quote_indent > one.quote_indent);
        assert!(half.heading_margin_top > one.heading_margin_top);
    }

    /// PIN — **a wide pane centres the prose column; a narrow one keeps the
    /// pane** (`#write { max-width: 860px; margin: 0 auto }`).
    ///
    /// This is the report's headline complaint in two rectangles. A maximised
    /// window used to set a paragraph of Chinese a hundred and forty characters
    /// to the line, which is well past the width at which an eye finds the start
    /// of the next one. Past the measure the leftover is split evenly and the
    /// column stops growing; below it nothing changes, because a measure imposed
    /// on a 400px pane is a 400px pane with a stripe of nothing down one side.
    ///
    /// MUTATION: drop the `inner <= measure` arm and the narrow case centres a
    /// column it cannot afford; drop the centring and the wide case pins the
    /// column to the left edge with the whole surplus on the right.
    #[test]
    fn a_pane_wider_than_the_measure_centres_the_column_and_a_narrower_one_does_not() {
        let metrics = markdown_metrics(1.0);

        let narrow = [0.0, 0.0, 400.0, 600.0];
        let (left, right) = markdown_measure_box(narrow, metrics);
        assert_eq!(left, metrics.padding_x, "the page's own padding, as before");
        assert_eq!(right, 400.0 - metrics.padding_x);
        assert_eq!(right - left, 368.0);

        let wide = [100.0, 0.0, 1301.0, 600.0];
        let (left, right) = markdown_measure_box(wide, metrics);
        assert_eq!(
            right - left,
            metrics.measure,
            "the column stops at the measure"
        );
        assert_eq!(
            left - wide[0],
            wide[2] - right,
            "and what is left over is split evenly — `margin: 0 auto`"
        );
        assert_eq!(left, 200.0);
        assert_eq!(right, 1201.0);

        // The hinge is exactly the measure plus the two paddings; a pane one
        // pixel narrower than that is still a pane and gets the pane's rule.
        let hinge = metrics.measure + metrics.padding_x * 2.0;
        let (left, right) = markdown_measure_box([0.0, 0.0, hinge, 600.0], metrics);
        assert_eq!(
            (left, right),
            (metrics.padding_x, hinge - metrics.padding_x)
        );
        let (left, _) = markdown_measure_box([0.0, 0.0, hinge + 2.0, 600.0], metrics);
        assert_eq!(
            left,
            metrics.padding_x + 1.0,
            "one pixel over and it centres"
        );
    }

    /// PIN — **the column's cap is about a thousand logical pixels, and a pane
    /// under it still gets the pane** (user ruling, 2026-09-07).
    ///
    /// The report is a screenshot: a `.md` file open as a tab of its own, 1770
    /// logical pixels of pane, and the document reading down a 730-pixel strip in
    /// the middle of it with a wide table clipped at the strip's edge. 54em of a
    /// 13px body is 702 pixels, and 702 was Typora's own `860px` carried across
    /// to a smaller body — a faithful port of a number chosen for a 16px page.
    /// The ruling raises the cap and nothing else: the margins, the centring and
    /// the pane rule underneath are all as they were.
    ///
    /// Three widths, because a cap is a claim about all three: a pane too narrow
    /// for even the old column, a pane between the old cap and the new one — which
    /// used to be capped and now is not — and a pane wide enough to be capped.
    ///
    /// MUTATION: put `PREVIEW_PROSE_MEASURE_EM` back to 54 and the middle width
    /// goes red with a column of 702 in a pane that can hold 868; the wide width
    /// goes red on the cap itself.
    #[test]
    fn the_prose_column_is_capped_at_about_a_thousand_logical_pixels() {
        let metrics = markdown_metrics(1.0);
        assert_eq!(
            metrics.measure, 1001.0,
            "77em of a 13px body — a thousand logical pixels, near enough"
        );

        // ① Under the old cap: the pane, exactly as before.
        let narrow = [0.0, 0.0, 500.0, 600.0];
        let (left, right) = markdown_measure_box(narrow, metrics);
        assert_eq!(
            (left, right),
            (metrics.padding_x, 500.0 - metrics.padding_x),
            "a narrow pane is untouched by the ruling"
        );

        // ② Between the two caps: this pane used to be capped at 702 and now
        // gets the whole of itself, which is the half of the ruling a single
        // number cannot state.
        let middle = [0.0, 0.0, 900.0, 600.0];
        let (left, right) = markdown_measure_box(middle, metrics);
        assert_eq!(
            (left, right),
            (metrics.padding_x, 900.0 - metrics.padding_x),
            "a pane that used to be capped now holds the column whole"
        );
        assert!(
            right - left > 702.0,
            "and it is wider than the column this pane used to be given"
        );

        // ③ The user's own pane, near enough: the cap holds and the column is
        // centred, which is what keeps this from being "run to the pane".
        let wide = [0.0, 0.0, 1771.0, 900.0];
        let (left, right) = markdown_measure_box(wide, metrics);
        assert_eq!(right - left, 1001.0, "capped");
        assert_eq!(left - wide[0], wide[2] - right, "and still centred");
        assert!(
            right - left < (wide[2] - wide[0]) * 0.6,
            "a pane's worth of prose is the thing the cap exists to refuse"
        );
    }

    /// PIN — **a heading gets more air above it than below, and none at all when
    /// it is the first thing on the page or the second heading in a row.**
    ///
    /// `h1 … h6 { margin: 24px 0 16px }` plus github.css's two `:first-child`
    /// rules. The asymmetry is the fix for "headings glued to the paragraph
    /// above": a symmetric margin puts a heading exactly as far from the prose it
    /// follows as from the prose it introduces, so it belongs to neither.
    ///
    /// MUTATION: return `(heading_margin_top, heading_margin_top)` for a heading
    /// and the third assertion goes red; drop the `previous.is_none()` clamp and
    /// the first block of every document starts one and a half ems below its own
    /// padding.
    #[test]
    fn a_heading_takes_its_air_from_above_and_the_first_block_takes_none() {
        let metrics = markdown_metrics(1.0);
        let heading = |level: u8| MarkdownBlock::Heading {
            level,
            spans: parse_inline("Title"),
        };
        let prose = MarkdownBlock::Paragraph(parse_inline("Body."));

        // `:first-child` — nothing above the first block, whatever it is.
        assert_eq!(markdown_block_margins(&heading(1), None, metrics).0, 0.0);
        assert_eq!(markdown_block_margins(&prose, None, metrics).0, 0.0);

        let (top, bottom) = markdown_block_margins(&heading(2), Some(&prose), metrics);
        assert_eq!((top, bottom), (20.0, 13.0), "24px 0 16px, in ems of 13");
        assert!(top > bottom, "which is what binds it to the prose below it");

        // `## Section` directly under `# Title` is one masthead, not two.
        assert_eq!(
            markdown_block_margins(&heading(2), Some(&heading(1)), metrics).0,
            0.0
        );

        // Every other block is a block sibling and gets a block sibling's 1em;
        // a fence gets one too now, where the mock-up gave it 6px, and a rule
        // gets a heading's own 1.5em.
        for block in [
            MarkdownBlock::Paragraph(parse_inline("x")),
            MarkdownBlock::Quote(vec![parse_inline("x")]),
            MarkdownBlock::List {
                ordered: None,
                items: vec![parse_inline("x")],
            },
            MarkdownBlock::Table {
                rows: vec![vec![parse_inline("x")]],
                alignments: vec![bt_detect::table::ColumnAlignment::None],
            },
        ] {
            assert_eq!(
                markdown_block_margins(&block, Some(&prose), metrics),
                (metrics.paragraph_gap, metrics.paragraph_gap),
                "{block:?} is a block sibling"
            );
        }
        let fence = MarkdownBlock::Code {
            lang: None,
            text: "x".to_owned(),
        };
        assert_eq!(
            markdown_block_margins(&fence, Some(&prose), metrics),
            (metrics.code_margin, metrics.code_margin)
        );
        assert_eq!(
            markdown_block_margins(&MarkdownBlock::Rule, Some(&prose), metrics),
            (metrics.rule_margin, metrics.rule_margin)
        );
    }

    /// PIN — **only `h1` and `h2` carry a rule, and it is a hairline.**
    ///
    /// `h1, h2 { padding-bottom: .3em; border-bottom: 1px solid }`. The padding
    /// is an em of the *heading's own* size, which is why the rule under an `h1`
    /// stands further off its letters than the one under an `h2` — and the extent
    /// is one number so the pass that reserves the space and the pass that paints
    /// the quad can never disagree about it.
    ///
    /// MUTATION: raise `PREVIEW_MD_HEADING_RULE_LEVELS` to 3 and an `###` grows a
    /// line under it, which is four rules to a page in any real document.
    #[test]
    fn the_first_two_heading_levels_are_underlined_and_no_others_are() {
        let metrics = markdown_metrics(1.0);
        assert_eq!(metrics.heading_rule_padding(1), 8.0, ".3em of 26");
        assert_eq!(metrics.heading_rule_padding(2), 6.0, ".3em of 19.5");
        assert_eq!(metrics.heading_rule_extent(1), 9.0);
        assert_eq!(metrics.heading_rule_extent(2), 7.0);
        for level in 3..=6 {
            assert_eq!(metrics.heading_rule_padding(level), 0.0);
            assert_eq!(metrics.heading_rule_extent(level), 0.0);
        }
        assert_eq!(
            metrics.heading_rule_thickness, 1.0,
            "a hairline, like every other divider this window draws"
        );
    }

    /// PIN — the fixture the report was made against still carries every block
    /// the new metrics have an opinion about.
    ///
    /// A guard rather than a measurement: `stress.md` grew a list section on
    /// 2026-08-16 so the item gap and the 30px indent have something to be
    /// asserted against, and the rest of the suite reads this file for the blocks
    /// that refuse to reflow. If a later edit takes one of them out, the tests
    /// that depend on it fail somewhere far less obvious than here.
    #[test]
    fn the_stress_sample_carries_every_block_the_measure_has_a_rule_for() {
        let source = include_str!("../../../tests/assets/preview-samples/stress.md");
        let blocks = parse_markdown(source);
        let count = |f: fn(&MarkdownBlock) -> bool| blocks.iter().filter(|b| f(b)).count();
        assert!(count(|b| matches!(b, MarkdownBlock::Heading { level: 1, .. })) >= 1);
        assert!(count(|b| matches!(b, MarkdownBlock::Heading { level: 2, .. })) >= 4);
        assert!(count(|b| matches!(b, MarkdownBlock::Paragraph(_))) >= 2);
        assert!(count(|b| matches!(b, MarkdownBlock::Code { .. })) >= 1);
        assert!(count(|b| matches!(b, MarkdownBlock::Table { .. })) == 2);
        assert!(count(|b| matches!(b, MarkdownBlock::Quote(_))) >= 1);
        assert!(count(|b| matches!(b, MarkdownBlock::Rule)) >= 1);
        let lists: Vec<&MarkdownBlock> = blocks
            .iter()
            .filter(|b| matches!(b, MarkdownBlock::List { .. }))
            .collect();
        assert_eq!(lists.len(), 2, "a bulleted list and an ordered one");
        let Some(MarkdownBlock::List { items, .. }) = lists.first().copied() else {
            unreachable!("filtered above")
        };
        assert!(
            items.len() >= 3,
            "enough items for `li + li` to mean something"
        );
        assert!(
            items
                .iter()
                .any(|item| item.iter().any(|span| span.style == SpanStyle::Code)),
            "and one of them carries an inline code span, which is set at 85%"
        );
    }

    // ── W2 slice ③: a page is a preview buffer ─────────────────────────────

    /// **A page's name is its title, so a page is not classified by its name**
    /// (`docs/DESIGN.md` §7.7 ①).
    ///
    /// The one class [`preview_ftype`] never answers, and the reason is a real
    /// collision rather than tidiness: page titles routinely end in something
    /// that reads as an extension, and a build that put a title through the
    /// name-classifier would draw the markdown reader over a live browser.
    ///
    /// Red gate: `PreviewBuffer::new` asks `preview_ftype(&name)` for every
    /// source, so a page called `release-notes.md` is `Markdown`, its view is
    /// `Markdown`, and the document pipeline is asked to paint it.
    #[test]
    fn a_page_is_not_classified_by_the_title_it_happens_to_wear() {
        for title in ["release-notes.md", "sunset.png", "data.csv", "Folio site"] {
            let buffer = PreviewBuffer::new(
                PreviewSource::Web("http://localhost:5173/".to_owned()),
                title.to_owned(),
            );
            assert_eq!(
                buffer.ftype,
                PreviewFtype::Web,
                "a page is a page whatever its title says: {title}"
            );
            assert_eq!(buffer.view(false), PreviewView::Web);
            assert_eq!(
                buffer.view(true),
                PreviewView::Web,
                "and the source flip is a question about text, which this window
                 holds none of for a page"
            );
            assert_eq!(
                buffer.view(false).chrome(),
                PreviewChrome::Web,
                "and nothing in this window paints its body"
            );
        }
    }

    /// **A page never waits for a disk** — the graph's own rule, one lane over.
    ///
    /// `Pending` means "the text is on its way", and for a page nothing is on
    /// its way: the pixels are the engine's and arrive through the composition
    /// tree. Left `Pending` a page sits under a "Loading …" line for ever, which
    /// is exactly what the graph's first real frame did (see
    /// [`PreviewBuffer::new`]).
    ///
    /// The three doors that ask about a disk are pinned in the same breath,
    /// because each would be a different wrong thing: a head read for a URL, an
    /// edit surface over a page, and a save with nowhere to write.
    ///
    /// Red gate: `PreviewSource::Web(_) => PreviewLoad::Pending`.
    #[test]
    fn a_page_is_ready_the_moment_it_is_made_and_asks_no_disk_anything() {
        let mut buffer = PreviewBuffer::new(
            PreviewSource::Web("http://localhost:5173/app".to_owned()),
            "Folio site".to_owned(),
        );
        assert_eq!(buffer.load, PreviewLoad::Ready);
        assert!(!buffer.wants_head_read(), "there is no disk to ask");
        assert!(buffer.claim_head_read().is_none());
        assert!(!buffer.is_editable(false) && !buffer.is_editable(true));
        assert!(matches!(buffer.save(), SaveOutcome::Failed(_)));
        assert_eq!(
            buffer.body_notice(),
            None,
            "and it is not an empty document"
        );
        assert_eq!(buffer.refusal(), None, "nor a refused one");
    }

    /// **A page has no file and no repository, and its foot says its address.**
    ///
    /// `file_path` is the door every file-only verb asks through — saving,
    /// revealing in Explorer, the head read, a relative markdown link — and each
    /// of them would be wrong about a URL. `composed_lead` is the other half:
    /// the foot asks "where does this live", and for a page the answer is the
    /// address, which is what §7.7 ③ has the strip print.
    ///
    /// Red gate: `Self::File(_) | Self::Web(_) => None` in `composed_lead`.
    #[test]
    fn a_page_answers_no_file_no_repository_and_its_own_address() {
        const URL: &str = "http://localhost:5173/app?tab=logs#line-42";
        let source = PreviewSource::Web(URL.to_owned());
        assert_eq!(source.file_path(), None);
        assert_eq!(source.repo_file(), None);
        assert!(!source.is_git());
        assert_eq!(source.web_url(), Some(URL));
        assert_eq!(
            source.composed_lead(),
            Some(URL.to_owned()),
            "the foot of a page says the page, query and fragment included"
        );
        assert_eq!(
            PreviewSource::file(r"C:\a\b.md").web_url(),
            None,
            "and a file is not a page asked the other way"
        );
    }

    /// **Two pages that differ only in query or fragment are two buffers, and
    /// one URL twice is one** (`plan.md` §3 切换器确定性三则).
    ///
    /// The pool is the switcher's list, so this is the de-duplication rule said
    /// where it actually happens. Query and fragment participate because they
    /// are part of what was asked for; the identity the caller hands in is
    /// `webnav::switcher_key`'s, which is the other half of the rule and is
    /// pinned where the caller lives.
    #[test]
    fn a_pool_holds_one_row_per_page_and_query_and_fragment_are_part_of_which() {
        let mut pool = PreviewPool::default();
        let open = |pool: &mut PreviewPool, url: &str| {
            pool.open(
                PreviewSource::Web(url.to_owned()),
                "Folio site".to_owned(),
                &[],
            );
        };
        open(&mut pool, "http://localhost:5173/app");
        open(&mut pool, "http://localhost:5173/app");
        assert_eq!(pool.len(), 1, "one URL twice is one row");
        open(&mut pool, "http://localhost:5173/app?tab=logs");
        open(&mut pool, "http://localhost:5173/app#top");
        assert_eq!(pool.len(), 3, "and three questions are three rows");
        assert!(
            pool.get(&PreviewSource::Web("http://localhost:5173/app".to_owned()))
                .is_some()
        );
        // A file and a page cannot collide even if somebody manages to spell one
        // as the other: they are different variants, not different strings.
        open(&mut pool, r"C:\notes.md");
        pool.open(
            PreviewSource::file(r"C:\notes.md"),
            "notes.md".to_owned(),
            &[],
        );
        assert_eq!(pool.len(), 5);
    }

    // ── §7.32: text is decided by content when the name will not say ────────

    /// **RED GATE ①** (user report 2026-08-27; `docs/DESIGN.md` §7.32).
    ///
    /// A PowerShell script has no row in [`TEXT_EXTENSIONS`] and never will —
    /// the point of the ruling is that no list of suffixes is ever finished — so
    /// the whole of its preview is the sniff: the name falls to `Unknown`, the
    /// buffer waits instead of refusing, and the head that comes back promotes
    /// it.
    ///
    /// Mutation: put `PreviewFtype::Unknown => PreviewLoad::Refused(...Type)`
    /// back in [`PreviewBuffer::new`] and the first assertion fails; drop
    /// `PreviewFtype::Unknown` from [`PreviewBuffer::wants_head_read`] and the
    /// second does; drop the promotion in [`PreviewBuffer::accept`] and the
    /// last two do.
    #[test]
    fn a_script_nobody_listed_previews_as_text() {
        let dir = scratch("script");
        let path = dir.join("Deploy.ps1");
        std::fs::write(&path, "param(\n    [string]$Target\n)\n").unwrap();

        assert_eq!(
            preview_ftype("Deploy.ps1"),
            PreviewFtype::Unknown,
            "the table has no opinion about it, which is the whole premise"
        );
        let mut buffer = PreviewBuffer::new(PreviewSource::file(&path), "Deploy.ps1".to_owned());
        assert_eq!(
            buffer.load,
            PreviewLoad::Pending,
            "so the name refuses nothing — the bytes have not been asked yet"
        );
        assert!(
            buffer.claim_head_read().is_some(),
            "and the question that asks them is the preview's own one read"
        );

        buffer.accept(read_head(&path));
        assert_eq!(
            buffer.ftype,
            PreviewFtype::Text,
            "the bytes said text, so the file is text"
        );
        assert_eq!(buffer.load, PreviewLoad::Ready);
        assert_eq!(
            buffer.content.as_deref(),
            Some("param(\n    [string]$Target\n)\n")
        );
        assert_eq!(
            buffer.view(false),
            PreviewView::Text,
            "and a text buffer is drawn as text, on the surface every other one is"
        );
        assert!(
            buffer.is_editable(false),
            "including the caret — a text preview is an editor (§7.1.6c-4c)"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **RED GATE ②** — the sniff refuses as readily as it promotes.
    ///
    /// The card a name nobody listed used to get by default is exactly the card
    /// it still gets when its bytes are not text, and it must be *that* card:
    /// "nothing in this window reads this kind of file", not a sentence about
    /// the disk.
    ///
    /// Mutation: promote unconditionally in [`PreviewBuffer::accept`] and the
    /// ftype assertion fails; make [`head_reads_as_text`] ignore the NUL and it
    /// fails one line earlier.
    #[test]
    fn a_binary_with_an_unknown_name_stays_unknown() {
        let dir = scratch("unknown-binary");
        let path = dir.join("bundle.pak");
        std::fs::write(&path, b"PAK\x01\x00\x00\x00\x08entries follow").unwrap();

        assert!(
            !head_reads_as_text(b"PAK\x01\x00\x00\x00\x08entries follow"),
            "a NUL inside the sniff window is the whole of git's own rule"
        );
        let mut buffer = PreviewBuffer::new(PreviewSource::file(&path), "bundle.pak".to_owned());
        assert!(buffer.claim_head_read().is_some());
        buffer.accept(read_head(&path));
        assert_eq!(
            buffer.ftype,
            PreviewFtype::Unknown,
            "the bytes did not say text, so nothing promoted it"
        );
        assert_eq!(
            buffer.load,
            PreviewLoad::Refused(PreviewRefusal::Type),
            "and the card is the one this name has always got"
        );
        assert_eq!(buffer.content, None);
        assert_eq!(buffer.view(false), PreviewView::None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **RED GATE ③** — a mark is the file saying what it is, and NUL is not
    /// evidence against a file that has said UTF-16.
    ///
    /// This is not a curiosity: `Out-File` and every `>` in Windows PowerShell
    /// 5.1 write UTF-16 LE with a mark, so before this ruling the transcripts
    /// this product's own shell produces were refused as binary.
    ///
    /// Mutation: delete the UTF-16 arms of [`HeadEncoding::of`] and every
    /// assertion below fails — the head is refused as binary before it is ever
    /// classified.
    #[test]
    fn a_utf16_file_with_a_bom_is_text() {
        let dir = scratch("utf16");
        let mut le = vec![0xFF, 0xFE];
        for unit in "Write-Host 'hi'\n".encode_utf16() {
            le.extend_from_slice(&unit.to_le_bytes());
        }
        let mut be = vec![0xFE, 0xFF];
        for unit in "Write-Host 'hi'\n".encode_utf16() {
            be.extend_from_slice(&unit.to_be_bytes());
        }
        assert!(head_reads_as_text(&le), "little-endian, marked");
        assert!(head_reads_as_text(&be), "big-endian, marked");
        // The same bytes with the mark taken off are a file that has said
        // nothing, and then the NULs are all there is to go on.
        assert!(!head_reads_as_text(&le[2..]), "unmarked UTF-16 is not text");

        let path = dir.join("transcript.log1");
        std::fs::write(&path, &le).unwrap();
        let mut buffer = PreviewBuffer::new(PreviewSource::file(&path), "transcript.log1".into());
        assert!(buffer.claim_head_read().is_some());
        buffer.accept(read_head(&path));
        assert_eq!(buffer.ftype, PreviewFtype::Text);
        assert_eq!(
            buffer.content.as_deref(),
            Some("Write-Host 'hi'\n"),
            "and it is *decoded*, not shown as one letter per two bytes"
        );

        // A file that claims UTF-16 and then does not decode is a binary file
        // that happened to start with those two bytes, and it is refused on the
        // word this window has always refused on.
        let liar = dir.join("liar.bin1");
        std::fs::write(&liar, b"\xFF\xFE\x00\xD8\x00\x00\x01\x00").unwrap();
        assert_eq!(
            read_head(&liar),
            HeadOutcome::Refused(PreviewRefusal::Binary)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **RED GATE ④** — the sniff is a disk read, and disk reads are the
    /// worker's.
    ///
    /// The window thread cannot be made to block on a file: that is
    /// [`PreviewWorker`]'s whole reason for existing, and a sniff bolted onto
    /// [`preview_ftype`] — which is asked of a *name*, on the frame that draws
    /// the row — would have put a `File::open` inside the paint.
    ///
    /// **Pinned by reading this file's own source**, because what is being
    /// asserted is a fact about the call graph rather than about a value: there
    /// is exactly one non-test caller of [`read_head`] in this crate and it is
    /// the closure [`PreviewWorker::spawn`] hands to `spawn_at_priority`.
    ///
    /// Mutation: call `read_head` (or `head_reads_as_text`) from
    /// [`preview_ftype`] or from [`PreviewBuffer::new`] and the count moves.
    #[test]
    fn sniffing_happens_off_the_window_thread() {
        const SOURCE: &str = include_str!("preview.rs");
        let (module, tests) = SOURCE
            .split_once("\n#[cfg(test)]\nmod tests {")
            .expect("this file carries its tests at the end");
        assert!(!tests.is_empty(), "and the split found them");

        // The definition, and the one call — **for each of the two reads**
        // (T2 ③, 2026-09-10). The whole-file read is a second, larger trip to
        // the same disk, and the line it must stay behind is this one: a body
        // read on the window thread is the frame budget spent on a file's size,
        // and 8MB of it would be worse than 64KB by exactly the factor between
        // them.
        let spawned_at = module
            .find("pub fn spawn(proxy: EventLoopProxy<AppEvent>)")
            .expect("PreviewWorker::spawn is declared here");
        for reader in ["read_head(", "read_whole("] {
            assert_eq!(
                module.matches(reader).count(),
                2,
                "{reader} is defined once and called once outside the tests"
            );
            let called_at = module
                .rfind(reader)
                .expect("the call the count above just found");
            assert!(
                called_at > spawned_at,
                "and {reader}'s one call is inside the worker's thread body"
            );
        }

        // Neither door the window thread asks is allowed to touch a disk. Both
        // are pure functions of a name and a source, and the assertions say so
        // in the vocabulary a reviewer would use.
        for signature in [
            "pub fn preview_ftype(name: &str) -> PreviewFtype {",
            "pub fn new(source: PreviewSource, name: String) -> Self {",
        ] {
            let body = one_function(module, signature);
            // **Calls, not mentions.** Both of these functions are allowed to
            // *say* where the answer comes from — that is what their comments
            // are for — and neither may go and get it.
            for disk in [
                "read_head(",
                "head_reads_as_text(",
                "File::open(",
                "fs::read",
                "metadata(",
            ] {
                assert!(
                    !body.contains(disk),
                    "{signature} must not reach the disk, and it calls {disk}"
                );
            }
        }
    }

    /// RED GATE (user report, 2026-08-28: 「md 预览不渲染图片」) — **the
    /// `<picture>` element picks the file that answers for the theme in force**,
    /// and this repository's own `README.md` is the fixture.
    ///
    /// MUTATION: drop the `<source>` walk in `html_image` and both themes get
    /// the light file; drop the theme test in `MarkdownImage::source_for` and
    /// the dark page draws the light hero.
    ///
    /// The alt text is checked with it because in that file it is five source
    /// lines of one sentence, and a reader that kept the line breaks would put
    /// them on the card the picture stands on.
    #[test]
    fn a_picture_element_picks_the_source_for_the_theme_in_force() {
        let blocks = parse_markdown(concat!(
            "<picture>\n",
            "  <source media=\"(prefers-color-scheme: dark)\"\n",
            "          srcset=\"assets/readme/hero-dark.svg\">\n",
            "  <img src=\"assets/readme/hero-light.svg\" width=\"100%\"\n",
            "       alt=\"Folio - the Windows terminal\n",
            "       that renders math.\">\n",
            "</picture>\n",
            "\n",
            "Prose after it.\n",
        ));
        let [MarkdownBlock::Image(image), MarkdownBlock::Paragraph(prose)] = blocks.as_slice()
        else {
            panic!("the element is one image block and the prose is its own: {blocks:#?}");
        };
        assert_eq!(prose, &vec![Span::plain("Prose after it.")]);
        assert_eq!(
            image.source_for(bt_render::Theme::Dark),
            "assets/readme/hero-dark.svg"
        );
        assert_eq!(
            image.source_for(bt_render::Theme::Light),
            "assets/readme/hero-light.svg"
        );
        assert_eq!(image.src, "assets/readme/hero-light.svg");
        assert!(image.fill, "a width of 100% is the document asking to fill");
        assert_eq!(
            image.alt, "Folio - the Windows terminal that renders math.",
            "an attribute's line breaks are spaces, as they are in HTML"
        );
    }

    /// A `<source>` this window cannot evaluate is a `<source>` it must not
    /// pick: what a browser does when nothing matches is fall back to `<img>`,
    /// and so does this.
    #[test]
    fn a_source_whose_media_is_not_about_the_scheme_is_not_chosen() {
        let blocks = parse_markdown(concat!(
            "<picture>\n",
            "<source media=\"(min-width: 600px)\" srcset=\"wide.png\">\n",
            "<img src=\"plain.png\" alt=\"a\">\n",
            "</picture>\n",
        ));
        let [MarkdownBlock::Image(image)] = blocks.as_slice() else {
            panic!("{blocks:#?}");
        };
        assert!(image.sources.is_empty());
        assert_eq!(image.source_for(bt_render::Theme::Dark), "plain.png");
    }

    /// A bare `<img>` is a picture on the same terms, and a `srcset` list gives
    /// up its first address.
    #[test]
    fn a_bare_img_tag_is_a_picture() {
        assert_eq!(
            parse_markdown("<img src=\"a.png\" alt=\"an a\">\n"),
            vec![MarkdownBlock::Image(MarkdownImage::named("an a", "a.png"))]
        );
        assert_eq!(
            first_srcset_url("a.png 1x, a@2x.png 2x").as_deref(),
            Some("a.png")
        );
    }

    /// **Every other tag is still printed as it stands** — the ruling's own
    /// scope line, and what keeps this from being half an HTML renderer. A run
    /// carrying a picture *and* a sentence is not a picture either: it is an
    /// HTML block this window cannot draw, and drawing half of it would swallow
    /// the other half.
    #[test]
    fn html_that_is_not_one_of_the_two_tags_is_printed_as_it_stands() {
        for source in [
            "<div align=\"center\">\n<b>hello</b>\n</div>\n",
            "<picture>\n<img src=\"a.png\">\n</picture>\nand a caption\n",
            // A media element that is not one of the two, spelled as `audio`
            // rather than `video` because the retirement gate for the old
            // player page (`the_shell_page_is_gone`) reads this file and
            // refuses any opening video element, fixture or not.
            "<audio src=\"a.mp3\"></audio>\n",
            "<image src=\"a.png\">\n",
        ] {
            let blocks = parse_markdown(source);
            assert!(
                !blocks
                    .iter()
                    .any(|block| matches!(block, MarkdownBlock::Image(_))),
                "{source:?} is not a picture this window reads: {blocks:#?}"
            );
        }
    }

    /// RED GATE (same report) — **`![alt](src)` is a picture and not a `!`
    /// followed by a link**, which is all the link pass alone could make of it.
    ///
    /// MUTATION: take the `!` test out of `push_link_runs` and the first
    /// assertion comes back as `Span::plain("!")` beside a `Span::link`.
    #[test]
    fn a_bang_in_front_of_a_link_makes_it_a_picture() {
        assert_eq!(
            parse_inline("![a shot](docs/shot.png)"),
            vec![Span::image("a shot", "docs/shot.png")]
        );
        // The title CommonMark allows beside the destination is parsed off, so
        // what reaches the disk is a path and not a path with a caption on it.
        assert_eq!(
            parse_inline("![a](docs/shot.png \"A caption\")"),
            vec![Span::image("a", "docs/shot.png")]
        );
        // And the same grammar under a link, which was carrying its own quotes
        // into `link_action` until the two shared one reader.
        assert_eq!(
            parse_inline("[a](page.md 'why')"),
            vec![Span::link("a", "page.md")]
        );
        // An empty alt is a picture; an empty label is still punctuation.
        assert_eq!(parse_inline("![](x.png)"), vec![Span::image("", "x.png")]);
        assert_eq!(parse_inline("[]()"), vec![Span::plain("[]()")]);
        // An escaped bang is the author's own bang.
        assert_eq!(
            parse_inline("\\![a](b)"),
            vec![Span::plain("\\!"), Span::link("a", "b")]
        );
        // And the backtick pass still stands in front: a picture written inside
        // a code span is the text of that code span.
        assert_eq!(parse_inline("`![a](b)`"), vec![Span::code("![a](b)")]);
    }

    /// A run set in `style`, pointing at `target` — what a link's label leaves
    /// behind on every run inside it.
    fn targeted(text: &str, style: SpanStyle, target: &str) -> Span {
        Span {
            text: text.to_owned(),
            style,
            target: Some(target.to_owned()),
        }
    }

    /// RED GATE (user report, 2026-09-07: in this window's own markdown preview
    /// the download line of `README.zh-CN.md` printed
    /// ``[`folio-0.2.2-windows-x64.zip`](https://…)`` as raw markdown, while the
    /// plain-text link on the same line rendered) — **a link's label is inline
    /// content, parsed by CommonMark's bracket rules, and never a string.**
    ///
    /// The cause was the one emphasis had on 2026-08-28, in a second
    /// construct — `docs/DESIGN.md` §7.1.3i‴. The link pass ran on the
    /// *leftovers* of the code and mathematics passes, so a `[` deposited in one
    /// leftover and the `](…)` that answers it in another were two halves no
    /// single leftover could see. Anything at all inside a label — a code span,
    /// a formula, an inner picture — split the label in two and printed the
    /// markup.
    ///
    /// Every row is checked before any of them is reported, because the matrix
    /// is the evidence: a stop at the first row says which shape broke and not
    /// which rule did.
    ///
    /// MUTATIONS: read brackets inside one pass's leftovers rather than along
    /// the line (rows 1 to 6 and 8 to 10); hand the label to the renderer as one
    /// string rather than as runs (rows 2, 3, 7 and 8); let a bracket inside a
    /// claimed range open one (rows 4, 5 and 11); drop the target from every run
    /// of a label but the first (rows 1 to 3); leave `![` alight when a link is
    /// made (row 9).
    #[test]
    fn a_links_label_is_inline_content_and_not_a_string() {
        let zip = "https://github.com/lulu-loopp/folio-terminal/releases/download/v0.2.2-preview/folio-0.2.2-windows-x64.zip";
        let reported = format!("[`folio-0.2.2-windows-x64.zip`]({zip})");
        let cases: [(&str, Vec<Span>); 11] = [
            // 1. The reported shape: a label that is one code span.
            (
                reported.as_str(),
                vec![targeted(
                    "folio-0.2.2-windows-x64.zip",
                    SpanStyle::Code,
                    zip,
                )],
            ),
            // 2. A label carrying emphasis, which was a floor and is now the rule.
            (
                "[**bold** text](u)",
                vec![
                    targeted("bold", SpanStyle::Bold, "u"),
                    Span::link(" text", "u"),
                ],
            ),
            // 3. And a label carrying both, in Chinese prose.
            (
                "从[发布页](p.md)下载 [`folio.zip` *现在*](z.zip)，解压",
                vec![
                    Span::plain("从"),
                    Span::link("发布页", "p.md"),
                    Span::plain("下载 "),
                    targeted("folio.zip", SpanStyle::Code, "z.zip"),
                    Span::link(" ", "z.zip"),
                    targeted("现在", SpanStyle::Italic, "z.zip"),
                    Span::plain("，解压"),
                ],
            ),
            // 4. A code span in a label may hold the very characters the grammar
            //    is written in: the claim was made before the walk read a bracket.
            (
                "[a `b]c` d](u)",
                vec![
                    Span::link("a ", "u"),
                    targeted("b]c", SpanStyle::Code, "u"),
                    Span::link(" d", "u"),
                ],
            ),
            // 5. The same for a parenthesis.
            ("[`f(x)`](u)", vec![targeted("f(x)", SpanStyle::Code, "u")]),
            // 6. Brackets nest: an inner pair that opens no link is text in the
            //    label of the outer one.
            ("[see [1] here](u)", vec![Span::link("see [1] here", "u")]),
            // 7. A picture's alt is the plain text of its label — CommonMark's
            //    own answer, so a code span in it says what it says.
            (
                "![the `zip` file](i.png)",
                vec![Span::image("the zip file", "i.png")],
            ),
            // 8. A formula in a label is still a formula, and it answers the click.
            (
                "[$x^2$ explained](m.md)",
                vec![
                    targeted("$x^2$", SpanStyle::Math, "m.md"),
                    Span::link(" explained", "m.md"),
                ],
            ),
            // 9. A badge: a picture inside a link keeps its own source, because
            //    a picture is not a link in this window.
            (
                "[![build](b.svg)](ci.html)",
                vec![Span::image("build", "b.svg")],
            ),
            // 10. Links may not contain links (CommonMark §6.3): the inner one
            //     wins and the outer bracket is text.
            (
                "[a [b](c) d](e)",
                vec![
                    Span::plain("[a "),
                    Span::link("b", "c"),
                    Span::plain(" d](e)"),
                ],
            ),
            // 11. PIN, CommonMark §6.3's own example: a code span binds more
            //     tightly than the brackets, so this is not a link at all.
            (
                "[foo`](/uri)`",
                vec![Span::plain("[foo"), Span::code("](/uri)")],
            ),
        ];
        let mut wrong = Vec::new();
        for (row, (source, want)) in cases.iter().enumerate() {
            let got = parse_inline(source);
            if got != *want {
                wrong.push(format!(
                    "row {}: {source}\n  want {want:?}\n  got  {got:?}",
                    row + 1
                ));
            }
        }
        assert!(
            wrong.is_empty(),
            "a link's label is inline content:\n{}",
            wrong.join("\n")
        );
    }

    /// **A paragraph is cut at its pictures** — prose, picture, prose — which is
    /// how every picture in this window's markdown becomes a block of its own.
    #[test]
    fn a_paragraph_is_cut_at_the_pictures_it_carries() {
        assert_eq!(
            parse_markdown("![alone](a.png)\n"),
            vec![MarkdownBlock::Image(MarkdownImage::named("alone", "a.png"))]
        );
        assert_eq!(
            parse_markdown("see ![this](a.png) here\n"),
            vec![
                MarkdownBlock::Paragraph(vec![Span::plain("see ")]),
                MarkdownBlock::Image(MarkdownImage::named("this", "a.png")),
                MarkdownBlock::Paragraph(vec![Span::plain(" here")]),
            ]
        );
    }

    /// One function's text, from its signature to whichever closing brace comes
    /// first — [`sniffing_happens_off_the_window_thread`]'s reader.
    ///
    /// Both indentations are tried and the **nearer** wins, because the two
    /// functions read here are declared at different depths: a free function
    /// closes at column zero and a method closes four spaces in, and taking the
    /// first of the two that merely *exists* would hand a method the whole rest
    /// of its `impl`.
    fn one_function<'src>(source: &'src str, signature: &str) -> &'src str {
        let start = source
            .find(signature)
            .unwrap_or_else(|| panic!("{signature} is declared in this file"));
        let rest = &source[start + signature.len()..];
        let end = ["\n}", "\n    }"]
            .into_iter()
            .filter_map(|close| rest.find(close))
            .min()
            .unwrap_or(rest.len());
        &rest[..end]
    }

    /// PIN (R1-12) — **the card's button belongs to the refusals that are about
    /// the content, and to no others.**
    ///
    /// The button says "open this in whatever the machine has registered", and
    /// that is an answer to 「这扇窗读不了这个内容」: an unknown format, a head
    /// full of NULs, a reader this build does not have. It is not an answer to
    /// 「这条路径不在这台机器上」 — §7.1.3 declines to read a share precisely
    /// because touching one stalls the window on somebody else's network, and a
    /// button that hands the same share to a synchronous `ShellExecuteW` undoes
    /// that refusal on one press, from the very card that announced it.
    ///
    /// MUTATION: give every refusal the same button and the network card hands
    /// the share over.
    #[test]
    fn only_a_refusal_about_the_content_offers_the_machine_its_handler() {
        assert!(PreviewRefusal::Type.offers_the_default_app());
        assert!(PreviewRefusal::Binary.offers_the_default_app());
        assert!(!PreviewRefusal::NetworkPath.offers_the_default_app());
        for fault in [
            PreviewFault::PermissionDenied,
            PreviewFault::NotFound,
            PreviewFault::Unreadable,
        ] {
            assert!(
                !PreviewRefusal::Fault(fault).offers_the_default_app(),
                "the disk already said no about {fault:?}"
            );
        }
    }

    // ── the source each block was parsed from ───────────────────────────────

    /// One document with every block this parser has in it, written as its lines
    /// so that a line ending in blanks can say so in a way no editor and no
    /// whitespace-trimming hook can quietly take back.
    fn ranged_page() -> String {
        [
            // A leading blank line: the file opens on bytes no block owns.
            "",
            "# A heading",
            "",
            "The first paragraph wraps",
            // Trailing whitespace, spelled so it survives being read by a tool.
            "across two lines.\u{20}\u{20}\u{20}",
            "",
            "Second prose with ![a picture](one.png) in the middle of it.",
            "",
            "![alone](two.png)",
            "",
            "<img src=\"three.png\" alt=\"a picture in html\">",
            "",
            "```rust",
            "let x = 1;",
            "```",
            "",
            "$$",
            "E = mc^2",
            "$$",
            "",
            "| a | b |",
            "|---|---|",
            "| 1 | 2 |",
            "",
            "> a quote",
            "> that wraps",
            "",
            "---",
            "",
            "1. first",
            "2. second",
            "",
            "- bullet",
            "  its lazy continuation",
            "- another",
        ]
        .join("\n")
            + "\n"
    }

    /// A fence nobody closed swallows the rest of the document, so it gets a
    /// document of its own.
    fn ranged_open_fence() -> String {
        ["Prose.", "", "```text", "still inside", ""].join("\n")
    }

    /// The same bytes as a file written on this platform: every break two bytes,
    /// which `str::lines` hides and a byte range must not.
    fn crlf(src: &str) -> String {
        src.replace('\n', "\r\n")
    }

    /// Every document the range tests are asked of, LF and CRLF, with and
    /// without the break the last line usually ends on.
    fn ranged_fixtures() -> Vec<(String, String)> {
        let mut fixtures = Vec::new();
        for (name, src) in [
            ("the page", ranged_page()),
            ("the open fence", ranged_open_fence()),
            ("nothing at all", String::new()),
            ("one break", "\n".to_owned()),
            ("one word", "word".to_owned()),
            (
                "a picture in a link",
                "[![alt](one.png)](where)\n".to_owned(),
            ),
            (
                "two pictures on one line",
                "![one](one.png) ![two](two.png)\n".to_owned(),
            ),
            // Two documents nobody wrote for this test: the product's own front
            // page, which is markdown with pictures, tables, fences and an HTML
            // `<picture>` in it, and this file, which is not markdown at all and
            // is therefore the walk asked about text it was never shaped for.
            (
                "this repository's front page",
                include_str!("../../../README.md").to_owned(),
            ),
            (
                "this file's own source",
                include_str!("preview.rs").to_owned(),
            ),
        ] {
            let bare = src.strip_suffix('\n').map(str::to_owned);
            fixtures.push((format!("{name}, LF"), src.clone()));
            fixtures.push((format!("{name}, CRLF"), crlf(&src)));
            if let Some(bare) = bare {
                fixtures.push((format!("{name}, LF, no final break"), bare.clone()));
                fixtures.push((format!("{name}, CRLF, no final break"), crlf(&bare)));
            }
        }
        fixtures
    }

    /// **RED GATE** — the ranges are a partition of the file (§9.2, ticket T1).
    ///
    /// Ordered, non-overlapping, each one a real slice of the source, and the
    /// file comes back byte for byte from the ranges plus the bytes between
    /// them. That last clause is the one that matters: it is what lets a save
    /// splice one block back into the document and leave every other byte —
    /// carriage returns, trailing blanks, the missing final break — exactly
    /// where the author left it.
    ///
    /// MUTATION: sum `line.len()` instead of reading
    /// [`crate::preview_edit::line_starts`] and every CRLF fixture comes back
    /// short by a byte a line.
    #[test]
    fn the_block_ranges_partition_the_file_and_the_file_comes_back_whole() {
        for (name, src) in ranged_fixtures() {
            let (blocks, ranges) = parse_markdown_ranged(&src);
            assert_eq!(blocks.len(), ranges.len(), "{name}: one range per block");
            let mut rebuilt = String::new();
            let mut cursor = 0usize;
            for (block, range) in blocks.iter().zip(&ranges) {
                assert!(
                    range.start < range.end,
                    "{name}: {block:?} was parsed from no bytes at all"
                );
                assert!(
                    cursor <= range.start,
                    "{name}: {block:?} at {range:?} runs back over the block before it"
                );
                assert!(
                    range.end <= src.len(),
                    "{name}: {block:?} at {range:?} runs off the end of {} bytes",
                    src.len()
                );
                rebuilt.push_str(&src[cursor..range.start]);
                rebuilt.push_str(&src[range.clone()]);
                cursor = range.end;
            }
            rebuilt.push_str(&src[cursor..]);
            assert_eq!(rebuilt, src, "{name}: the file does not come back");
        }
    }

    /// The connective tissue between two blocks is blank and nothing else.
    ///
    /// Not a restatement of the test above: that one says the bytes are all
    /// accounted for, this one says the accounting is honest. A block that
    /// shortened its range by a line would still reconstruct — the line would
    /// simply become a gap — and this is what notices.
    #[test]
    fn the_bytes_no_block_was_parsed_from_are_blank() {
        for (name, src) in ranged_fixtures() {
            let (_, ranges) = parse_markdown_ranged(&src);
            let mut cursor = 0usize;
            for range in &ranges {
                assert!(
                    src[cursor..range.start].trim().is_empty(),
                    "{name}: {:?} belongs to no block",
                    &src[cursor..range.start]
                );
                cursor = range.end;
            }
            assert!(
                src[cursor..].trim().is_empty(),
                "{name}: {:?} is left over at the end",
                &src[cursor..]
            );
        }
    }

    /// The wrapper changes nothing: [`parse_markdown`] is the ranged walk with
    /// the ranges dropped, which is what keeps `attention_words` and every
    /// assertion in this module and in `main` reading the same blocks.
    #[test]
    fn the_wrapper_hands_back_the_blocks_the_ranged_walk_found() {
        for (name, src) in ranged_fixtures() {
            assert_eq!(
                parse_markdown(&src),
                parse_markdown_ranged(&src).0,
                "{name}: the wrapper and the walk disagree"
            );
        }
    }

    /// Per kind: what a range covers is the block's own lines, their own line
    /// endings included.
    #[test]
    fn a_range_covers_its_blocks_lines_and_the_breaks_that_end_them() {
        let lf = "# Title\n\ntwo\nlines\n\n```rust\nlet x = 1;\n```\n";
        for (src, break_bytes) in [(lf.to_owned(), "\n"), (crlf(lf), "\r\n")] {
            let (blocks, ranges) = parse_markdown_ranged(&src);
            let text: Vec<&str> = ranges.iter().map(|range| &src[range.clone()]).collect();
            assert!(
                matches!(blocks[0], MarkdownBlock::Heading { .. }),
                "the first block is the heading"
            );
            assert_eq!(
                text[0],
                format!("# Title{break_bytes}"),
                "a heading's range covers its hashes, its text and its break"
            );
            assert_eq!(
                text[1],
                format!("two{break_bytes}lines{break_bytes}"),
                "a paragraph's range is all of its lines"
            );
            assert!(
                matches!(blocks[2], MarkdownBlock::Code { .. }),
                "the third block is the fence"
            );
            assert_eq!(
                text[2],
                format!("```rust{break_bytes}let x = 1;{break_bytes}```{break_bytes}"),
                "a fence's range covers both of its fence lines"
            );
        }
    }

    /// A file that ends without a break: the last block ends at the last byte.
    #[test]
    fn the_last_block_of_a_file_with_no_final_break_ends_at_the_last_byte() {
        for src in [
            "# Title\n\nlast line",
            "# Title\r\n\r\nlast line",
            "one\ntwo",
        ] {
            let (_, ranges) = parse_markdown_ranged(src);
            assert_eq!(
                ranges.last().expect("the file has blocks in it").end,
                src.len(),
                "{src:?}: the last block stops short of the end"
            );
        }
    }

    /// **The picture cut out of a sentence** — the case the ticket asks to be
    /// written down. A picture's range is its own `![…](…)` spelling and the
    /// prose either side keeps the rest of the line, so no two blocks share a
    /// byte and the line still reconstructs.
    #[test]
    fn a_picture_inside_a_sentence_owns_its_spelling_and_the_prose_keeps_the_rest() {
        let src = "before ![alt](one.png) after\n";
        let (blocks, ranges) = parse_markdown_ranged(src);
        let text: Vec<&str> = ranges.iter().map(|range| &src[range.clone()]).collect();
        assert_eq!(blocks.len(), 3, "prose, picture, prose");
        assert!(matches!(blocks[1], MarkdownBlock::Image(_)));
        assert_eq!(text, ["before ", "![alt](one.png)", " after\n"]);
    }

    /// **The picture alone on its line** owns the line, its break included:
    /// nothing else on the line was parsed into anything, so there is nobody
    /// else for those bytes to belong to.
    #[test]
    fn a_picture_alone_on_its_line_owns_the_line() {
        let src = "![alt](one.png)\n";
        let (blocks, ranges) = parse_markdown_ranged(src);
        assert_eq!(blocks.len(), 1);
        assert_eq!(&src[ranges[0].clone()], "![alt](one.png)\n");
    }

    /// A picture on its own line inside a wrapped paragraph: the prose above it
    /// ends where its spelling begins and the prose below it starts one past the
    /// closing parenthesis, which is the break that ended the picture's line.
    #[test]
    fn a_picture_on_its_own_line_inside_a_paragraph_cuts_the_paragraph_at_it() {
        let src = "before\n![alt](one.png)\nafter\n";
        let (blocks, ranges) = parse_markdown_ranged(src);
        let text: Vec<&str> = ranges.iter().map(|range| &src[range.clone()]).collect();
        assert_eq!(blocks.len(), 3);
        assert_eq!(text, ["before\n", "![alt](one.png)", "\nafter\n"]);
    }
}
