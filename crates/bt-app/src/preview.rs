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
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::mpsc;
use std::time::SystemTime;

use anyhow::{Context, Result};
use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};
use winit::event_loop::EventLoopProxy;
use winit::window::WindowId;

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
    /// preview for this file type" now shows the video's own picture and says how
    /// long it is.
    ///
    /// So this class means one thing and only one: **a name this window can put a
    /// face on.** It does not mean the file will play — nothing in this build
    /// plays anything — and it does not even promise a picture, because the set
    /// of containers Media Foundation reads is not the set this table lists. See
    /// [`VIDEO_EXTENSIONS`] for that mismatch, which is deliberate and written
    /// down rather than smoothed over.
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
const PAGE_EXTENSIONS: [(&str, PageGlance); 3] = [
    ("html", PageGlance::Source),
    ("htm", PageGlance::Source),
    ("pdf", PageGlance::Facts),
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
        .find(|(page, _)| extension.eq_ignore_ascii_case(page))
        .map(|(_, glance)| *glance)
}

/// **Whether a member of the video class can be played here**, or only drawn
/// (user ruling 2026-08-27, the second of that day; `docs/DESIGN.md` §7.23 ⑩).
///
/// The second column of [`VIDEO_EXTENSIONS`], and it is a column rather than a
/// second table for [`PAGE_EXTENSIONS`]'s reason said again: one class, one
/// list, and the thing that differs between its members written **beside** them
/// where nobody can add a row to one half and forget the other.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoPlayback {
    /// **Measured to play**, inside a page, on the engine this product already
    /// hosts: `canPlayType` answered `maybe` for `video/mp4`, `video/webm` and
    /// `video/x-m4v` on 2026-08-25 (§7.16 ①), and the play verb was measured
    /// end to end on 2026-08-27 (§7.23 ⑩).
    Plays,
    /// **A face and no player.** This window's decoder reads the container and
    /// the engine that would have to play it does not.
    FaceOnly,
}

/// Extensions that name a **video** — [`PreviewFtype::Video`] asked of a name
/// (user ruling 2026-08-27; `docs/DESIGN.md` §7.23).
///
/// **Four spellings and two columns, and the second column is the ruling.**
/// Three of them play: they are the three the engine on the preview seat was
/// measured to accept inside a page (`canPlayType` answered `maybe` for
/// `video/mp4`, `video/webm` and `video/x-m4v` on 2026-08-25). The fourth,
/// `mov`, has a face and no player — Media Foundation reads it (`.mov` is the
/// same MPEG-4 File Source as `.mp4`) and the engine answers
/// `canPlayType('video/quicktime')` with the empty string, which is the
/// strongest "no" that API has.
///
/// **`mov` was outside this table until the play verb arrived, and the reason it
/// is inside it now is the reason it was outside it then.** The first cut of
/// this list refused `mov` on the argument that "the day a video can be played,
/// the file that shows a face and the file that plays must be the same file" —
/// a class that promised a face and then could not play would be cut back on
/// that day. That day is here, and it settled the question the other way round:
/// the two sets are **not** the same set, they were never going to be, and the
/// honest form of that is one class whose members say which of them they are —
/// not a class trimmed until the difference disappears. A `.mov` this window can
/// draw is a `.mov` a reader can recognise, and a pane that draws it and says
/// [`crate::i18n::Text::VideoFormatCannotPlay`] under it has told the whole
/// truth. Refusing to draw it never made it playable; it only made it
/// unrecognisable.
///
/// # The set that can be drawn is not the set that is listed, either
///
/// Both directions are real and neither is a hole:
///
/// * A `.webm` carrying VP9 or AV1 plays in that engine, which ships its own
///   decoders for both, and may have **no** Media Foundation decoder on a stock
///   machine — so it is [`VideoPlayback::Plays`] and its card may still have no
///   picture on it. That is [`bt_platform::video::first_frame`]'s ordinary
///   `None`, and the card states the facts it has instead.
/// * A `.mkv`, an `.avi` — neither of which this window's decoder reads — are
///   outside this table altogether and get the same "no preview for this file
///   type" card they have always had.
///
/// The rule that resolves all of it is that *this table is a promise about the
/// window*, not a report about the platform: a name in it is a name this window
/// has a face for, and its column says whether that face has a play button.
const VIDEO_EXTENSIONS: [(&str, VideoPlayback); 4] = [
    ("mp4", VideoPlayback::Plays),
    ("m4v", VideoPlayback::Plays),
    ("webm", VideoPlayback::Plays),
    ("mov", VideoPlayback::FaceOnly),
];

/// **Which column of [`VIDEO_EXTENSIONS`] a name is in**, and `None` for a name
/// that is not in it at all.
///
/// [`preview_ftype`]'s own reading of the table, lifted out so that the two
/// questions a video raises — "is this one" and "can it play" — are one lookup
/// in one list. A caller that asked the first and then re-spelled the second
/// would be the drift §7.10 ⑥ paid for.
#[must_use]
fn video_playback_of_extension(extension: &str) -> Option<VideoPlayback> {
    VIDEO_EXTENSIONS
        .iter()
        .find(|(video, _)| extension.eq_ignore_ascii_case(video))
        .map(|(_, playback)| *playback)
}

/// **Whether the video a path names can be played on a preview seat** — the
/// second column of [`VIDEO_EXTENSIONS`] asked of a `Path`.
///
/// `None` for anything that is not a video at all, so a caller cannot mistake
/// "not a video" for "a video that will not play": the first has no face and the
/// second has one.
#[must_use]
pub fn video_playback(path: &std::path::Path) -> Option<VideoPlayback> {
    video_playback_of_extension(path.extension()?.to_str()?)
}

/// **Whether the video this path names is one the play verb is offered for.**
///
/// The one predicate the button, the verb and the red gate all read, so that a
/// frame with a play button over it and a seat that would accept a play cannot
/// come apart — see [`crate::Runtime::play_video_on`], which refuses the same
/// way this answers.
#[must_use]
pub fn path_names_a_playable_video(path: &std::path::Path) -> bool {
    video_playback(path) == Some(VideoPlayback::Plays)
}

/// **Whether a path's real extension names a video** — [`VIDEO_EXTENSIONS`]
/// asked of a `Path` rather than of a name.
///
/// [`path_names_a_page`]'s twin, and deliberately the same shape down to the
/// last clause, so every rule already written on that function holds here
/// without being restated: the **real** extension and never a substring of the
/// name (`clip.mp4.txt` is a text file that spells a video in the middle of
/// itself), case ignored, and a file whose whole name is `.mp4` has no extension
/// at all and is therefore not a video (§7.1.5j ⑦(e)).
#[must_use]
pub fn path_names_a_video(path: &std::path::Path) -> bool {
    video_playback(path).is_some()
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
    if PAGE_EXTENSIONS.iter().any(|(page, _)| *page == ext) {
        return PreviewFtype::Web;
    }
    // **After the page question and before `Unknown`**, which is where a class
    // that is not a page and is not nothing belongs. The two tables cannot
    // overlap — a video spelling in [`PAGE_EXTENSIONS`] is the thing §7.16's
    // pinned refusal exists to catch — so the order between them is a
    // formality, and it is written this way round because that refusal is the
    // older claim and reads first.
    if video_playback_of_extension(&ext).is_some() {
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
            .any(|(page, _)| extension.eq_ignore_ascii_case(page))
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
/// and a rendered markdown view is not an editor until it has been flipped to
/// source.
pub fn is_editable(name: &str, ftype: PreviewFtype, md_source: bool) -> bool {
    if is_diff_name(name) {
        return false;
    }
    match ftype {
        PreviewFtype::Text => true,
        PreviewFtype::Markdown => md_source,
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
    Bold,
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
}

/// Split one line into its inline runs.
///
/// **Four scans and one ruling, and the order of the scans is itself a ruling.**
/// Backticks first, which is the mock-up's order (4915-4917) and the one that
/// makes `` `**not bold**` `` come out as literal code rather than as a bold run
/// inside a code span; then mathematics, so that a formula's own `*`, `_` and `[`
/// are the formula's and not this renderer's; then links, so a
/// `**[bold link](url)**`'s brackets are gone before the asterisks are read; then
/// the asterisks and underscores over whatever is still plain.
///
/// **What the fourth scan produces is candidates, not runs.** Each of the first
/// three works on a *slice* and hands its leftovers to the next, so a line comes
/// out of them in pieces — and emphasis does not live in a piece. It lives in the
/// line: `**a `b` c**` has its opener in the first piece and its closer in the
/// third, and a pass that read one piece at a time saw no pair in either and
/// printed both pairs of asterisks (user report, 2026-08-28). So the scans
/// deposit [`Piece`]s into one list for the whole line, and
/// [`resolve_emphasis`] — CommonMark's delimiter stack, §6.2 — runs over that
/// list once, at the end, which is where the specification puts it too.
///
/// **The backtick pass standing in front of the mathematics pass is the whole of
/// "a dollar inside code is a dollar"** — and of "a `\begin{align}` inside code
/// is a `\begin{align}`". There is no second rule and no list of things that look
/// like shell or like LaTeX: a code span has already been claimed by the time the
/// formulas are read, and a fenced block never reaches this function at all.
///
/// **One door for every block that has text in it.** A table cell, a list item,
/// a quote line and a paragraph all come through here, which is the whole of why
/// `` `code` `` inside a table cell works without a line of its own: there is no
/// second inline parser to teach.
pub fn parse_inline(line: &str) -> Vec<Span> {
    let mut pieces = Vec::new();
    let mut claimed = 0usize;
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        // A backtick with no partner is a backtick, not the start of anything.
        let Some(close) = rest[open + 1..].find('`') else {
            break;
        };
        push_math_runs(&rest[..open], claimed, line, &mut pieces);
        pieces.push(Piece::Claimed(Span::code(
            &rest[open + 1..open + 1 + close],
        )));
        let eaten = open + 1 + close + 1;
        claimed += eaten;
        rest = &rest[eaten..];
    }
    push_math_runs(rest, claimed, line, &mut pieces);
    resolve_emphasis(&mut pieces);
    settle(pieces)
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
    /// Text nothing has claimed. `strong` is the emphasis pass's answer.
    Text { text: String, strong: bool },
    /// A run already claimed by the code, mathematics or link pass.
    ///
    /// It stands *inside* an emphasis span perfectly well — it simply cannot
    /// show it. [`Span`] is a flat model with one style per run, so a code span
    /// inside a bold phrase is set in the code face and not in a bold code face.
    /// That is a floor of the model rather than of this pass, and it is the same
    /// floor [`push_link_runs`] already stands on.
    Claimed(Span),
    /// A run of `*` or `_`, and what the flanking rule said about it.
    Delimiter(Delimiter),
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
    /// Whatever is left of it stands inside an emphasis span.
    strong: bool,
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
    fn weigh(marker: u8, length: usize, before: Option<char>, after: Option<char>) -> Self {
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
            unspent: length,
            length,
            can_open,
            can_close,
            struck: false,
            strong: false,
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

/// The second pass: mathematics inside whatever the backtick pass left plain.
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
fn push_math_runs(text: &str, at: usize, line: &str, pieces: &mut Vec<Piece>) {
    let mut rest = text;
    let mut at = at;
    while let Some((open, end)) = next_inline_math(rest) {
        push_link_runs(&rest[..open], at, line, pieces);
        pieces.push(Piece::Claimed(Span::math(&rest[open..end])));
        at += end;
        rest = &rest[end..];
    }
    push_link_runs(rest, at, line, pieces);
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

/// The second pass: `[text](url)` inside whatever the code pass left plain.
///
/// The label is emitted as one run and the target rides beside it, unresolved.
/// A label carrying its own emphasis (`[**a**](b)`) keeps the asterisks visible
/// rather than nesting two styles in one run — a deliberate floor, because the
/// alternative is a span model with a stack in it and the case is vanishing.
fn push_link_runs(text: &str, at: usize, line: &str, pieces: &mut Vec<Piece>) {
    let mut rest = text;
    let mut at = at;
    while let Some(open) = rest.find('[') {
        let Some(label_end) = rest[open..].find(']').map(|found| open + found) else {
            break;
        };
        // The target has to follow the label immediately, which is what keeps
        // `[a] (b)` and a bare `[TODO]` out of this branch.
        if !rest[label_end + 1..].starts_with('(') {
            push_delimiter_runs(&rest[..label_end + 1], at, line, pieces);
            at += label_end + 1;
            rest = &rest[label_end + 1..];
            continue;
        }
        let target_open = label_end + 1;
        let Some(target_end) = rest[target_open..]
            .find(')')
            .map(|found| target_open + found)
        else {
            break;
        };
        // `[]()` is punctuation, not an empty link.
        if label_end == open + 1 {
            push_delimiter_runs(&rest[..target_end + 1], at, line, pieces);
            at += target_end + 1;
            rest = &rest[target_end + 1..];
            continue;
        }
        push_delimiter_runs(&rest[..open], at, line, pieces);
        pieces.push(Piece::Claimed(Span::link(
            &rest[open + 1..label_end],
            rest[target_open + 1..target_end].trim(),
        )));
        at += target_end + 1;
        rest = &rest[target_end + 1..];
    }
    push_delimiter_runs(rest, at, line, pieces);
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
        push_piece_text(&text[plain..index], pieces);
        pieces.push(Piece::Delimiter(Delimiter::weigh(
            marker,
            end - index,
            line[..at + index].chars().next_back(),
            line[at + end..].chars().next(),
        )));
        plain = end;
        index = end;
    }
    push_piece_text(&text[plain..], pieces);
}

/// Add text to the piece list, joined to the text piece before it if there is one.
fn push_piece_text(text: &str, pieces: &mut Vec<Piece>) {
    if text.is_empty() {
        return;
    }
    match pieces.last_mut() {
        Some(Piece::Text { text: last, .. }) => last.push_str(text),
        _ => pieces.push(Piece::Text {
            text: text.to_owned(),
            strong: false,
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
/// **Every level of emphasis comes out bold, and that is a face this window does
/// not own rather than a rule it does not know.** The renderer sets a run in one
/// upright face at one weight or the other ([`bt_render::PreviewRun`]); there is
/// no italic in the stack, so `*a*`, `**a**` and `***a***` are all set in the one
/// emphatic face there is. Printing `*a*`'s asterisks instead — which is what
/// this pass used to do — shows the reader markup where the author wrote
/// emphasis, and that is the worse of the two losses.
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
    for piece in &mut pieces[opener + 1..closer] {
        match piece {
            Piece::Text { strong, .. } => *strong = true,
            Piece::Delimiter(run) => {
                run.strong = true;
                run.struck = true;
            }
            // A code span, a formula or a link keeps its own style inside an
            // emphasis span — see [`Piece::Claimed`].
            Piece::Claimed(_) => {}
        }
    }
    for end in [opener, closer] {
        let Piece::Delimiter(run) = &mut pieces[end] else {
            unreachable!("both ends were delimiters when they were chosen")
        };
        run.unspent -= spent;
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

/// Turn the ruled pieces into the runs the renderer draws.
fn settle(pieces: Vec<Piece>) -> Vec<Span> {
    let mut spans = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Text { text, strong } => push_text(&text, strong, &mut spans),
            Piece::Claimed(span) => spans.push(span),
            // Whatever a delimiter run did not spend is what the author typed.
            Piece::Delimiter(run) => push_text(
                &(run.marker as char).to_string().repeat(run.unspent),
                run.strong,
                &mut spans,
            ),
        }
    }
    spans
}

/// Add text, **joined to the run before it if that run is set the same way**.
///
/// The passes hand each other the text they did not claim, so a line the link
/// pass looked at and left alone comes back in two or three pieces. Two
/// adjacent plain runs shape and draw identically to one, but they are not one:
/// the shaper is given a rich-text sequence and a break opportunity between two
/// runs is not the same thing as one inside a run, so `a [TODO] note` split at
/// the bracket could wrap where the text does not permit it.
fn push_text(text: &str, strong: bool, spans: &mut Vec<Span>) {
    if text.is_empty() {
        return;
    }
    let span = if strong {
        Span::bold(text)
    } else {
        Span::plain(text)
    };
    match spans.last_mut() {
        Some(last) if last.style == span.style => last.text.push_str(text),
        _ => spans.push(span),
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
    let lines: Vec<&str> = src.lines().collect();
    let mut blocks = Vec::new();
    // Both accumulators hold **source text**, not spans, because both of them
    // join across source lines and inline parsing has to see the joined text.
    let mut list: Vec<String> = Vec::new();
    let mut ordered: Option<u64> = None;
    let mut paragraph: Vec<&str> = Vec::new();
    let mut index = 0usize;

    while index < lines.len() {
        let line = lines[index];

        // ── the fence, which swallows everything until it closes ────────────
        if let Some(rest) = line.strip_prefix("```") {
            flush_paragraph(&mut paragraph, &mut blocks);
            flush_list(&mut list, &mut ordered, &mut blocks);
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
            blocks.push(MarkdownBlock::Code {
                lang,
                text: body.join("\n"),
            });
            continue;
        }

        // ── display mathematics, which swallows its lines as a fence does ───
        //
        // **After the fence and before everything else.** A `$$` inside a fence
        // is a fence's business and the branch above has already taken it; a
        // `$$` anywhere else is the author opening a formula, and nothing below
        // may see those lines as prose, as a rule or as a table.
        if let Some(rest) = line.strip_prefix("$$") {
            flush_paragraph(&mut paragraph, &mut blocks);
            flush_list(&mut list, &mut ordered, &mut blocks);
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
            blocks.push(MarkdownBlock::Math {
                source: body.join("\n"),
            });
            continue;
        }

        // ── the same block written the other two ways ───────────────────────
        //
        // `\[…\]` and a bare `\begin{align}…\end{align}` are display
        // mathematics on the same terms `$$` is, and stand in the same place in
        // this walk: after the fence, which has already taken anything inside
        // it, and before every branch that would otherwise read these lines as
        // prose, as a rule or as a table.
        if let Some((source, after)) = display_math_block(&lines, index) {
            flush_paragraph(&mut paragraph, &mut blocks);
            flush_list(&mut list, &mut ordered, &mut blocks);
            blocks.push(MarkdownBlock::Math { source });
            index = after;
            continue;
        }

        // ── the table, which is the one block needing lookahead ─────────────
        if is_pipe_row(line)
            && lines
                .get(index + 1)
                .is_some_and(|next| is_table_separator(next))
        {
            flush_paragraph(&mut paragraph, &mut blocks);
            flush_list(&mut list, &mut ordered, &mut blocks);
            let mut rows = vec![split_pipe_row(line)];
            let index_of_separator = index + 1;
            // Past the separator, then every pipe row that follows without a
            // break. A blank line ends the table exactly as it ends a paragraph.
            index += 2;
            while index < lines.len() && is_pipe_row(lines[index]) {
                rows.push(split_pipe_row(lines[index]));
                index += 1;
            }
            let alignments = table_alignments(lines[index_of_separator], rows[0].len());
            blocks.push(MarkdownBlock::Table { rows, alignments });
            continue;
        }

        index += 1;

        if let Some(heading) = parse_heading(line) {
            flush_paragraph(&mut paragraph, &mut blocks);
            flush_list(&mut list, &mut ordered, &mut blocks);
            blocks.push(heading);
            continue;
        }
        // **After the table and before the list**, which is what keeps a `---`
        // that is a table's separator out of here and a `- item` out of the
        // rule: a separator is only ever reached by the branch above (which
        // consumed it), and a rule is three or more of one character with
        // nothing else on the line, which `- item` is not.
        if is_thematic_break(line) {
            flush_paragraph(&mut paragraph, &mut blocks);
            flush_list(&mut list, &mut ordered, &mut blocks);
            blocks.push(MarkdownBlock::Rule);
            continue;
        }
        if let Some((number, item)) = parse_list_row(line) {
            flush_paragraph(&mut paragraph, &mut blocks);
            // A bulleted list and a numbered one standing next to each other are
            // two lists, not one list that changes its mind halfway down.
            if !list.is_empty() && ordered.is_some() != number.is_some() {
                flush_list(&mut list, &mut ordered, &mut blocks);
            }
            if list.is_empty() {
                ordered = number;
            }
            list.push(item.trim().to_owned());
            continue;
        }
        if let Some(first) = strip_quote(line) {
            flush_paragraph(&mut paragraph, &mut blocks);
            flush_list(&mut list, &mut ordered, &mut blocks);
            // A quote's own lines gather exactly as prose does — a wrapped quote
            // is one quoted paragraph — and a bare `>` is the blank line that
            // separates two of them.
            let mut quoted = Vec::new();
            let mut run: Vec<&str> = Vec::new();
            let push_run = |run: &mut Vec<&str>, quoted: &mut Vec<Vec<Span>>| {
                if !run.is_empty() {
                    quoted.push(parse_inline(&join_source_lines(run)));
                    run.clear();
                }
            };
            let mut quoted_line = Some(first);
            while let Some(text) = quoted_line {
                if text.trim().is_empty() {
                    push_run(&mut run, &mut quoted);
                } else {
                    run.push(text);
                }
                quoted_line = lines.get(index).and_then(|line| strip_quote(line));
                index += usize::from(quoted_line.is_some());
            }
            push_run(&mut run, &mut quoted);
            blocks.push(MarkdownBlock::Quote(quoted));
            continue;
        }
        if line.trim().is_empty() {
            flush_paragraph(&mut paragraph, &mut blocks);
            flush_list(&mut list, &mut ordered, &mut blocks);
            continue;
        }
        // **Lazy continuation** (CommonMark §5.2): a plain line under an open
        // list belongs to the item above it, not to a paragraph of its own. A
        // bullet that wraps in the source is one bullet, which is the same
        // ruling the paragraph join is, applied where the text is indented under
        // a marker instead of standing on its own.
        match list.last_mut() {
            Some(item) if paragraph.is_empty() => {
                item.push(' ');
                item.push_str(line.trim());
            }
            _ => paragraph.push(line),
        }
    }
    flush_paragraph(&mut paragraph, &mut blocks);
    flush_list(&mut list, &mut ordered, &mut blocks);
    blocks
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
fn display_math_block(lines: &[&str], start: usize) -> Option<(String, usize)> {
    let first = *lines.get(start)?;
    if !first.starts_with(DISPLAY_MATH_OPEN) && !first.starts_with(ENVIRONMENT_OPEN) {
        return None;
    }
    let end = lines[start..]
        .iter()
        .position(|line| line.trim().is_empty())
        .map_or(lines.len(), |at| start + at);
    let paragraph = lines[start..end].join("\n");
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
    Some((
        math_block_body(body),
        start + paragraph[..close_end].matches('\n').count() + 1,
    ))
}

/// A display formula's source with the blank its delimiters left behind taken
/// off.
///
/// **Only the first line and the last, and only their own ends.** What is
/// between them is the author's own layout: a `&` column that lost its leading
/// spaces is a column that no longer lines up in the source a reader falls back
/// to when the engine refuses.
fn math_block_body(body: &str) -> String {
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
    lines.join("\n")
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

fn flush_paragraph(paragraph: &mut Vec<&str>, blocks: &mut Vec<MarkdownBlock>) {
    if paragraph.is_empty() {
        return;
    }
    let text = join_source_lines(paragraph);
    paragraph.clear();
    blocks.push(MarkdownBlock::Paragraph(parse_inline(&text)));
}

/// `#` through `######` followed by a space.
///
/// Six levels rather than the mock-up's three (`#{1,3}`), because a `####` in a
/// real document rendered as a paragraph beginning with four hashes is precisely
/// what "the prototype stops here" looks like from the outside.
fn parse_heading(line: &str) -> Option<MarkdownBlock> {
    let hashes = line.len() - line.trim_start_matches('#').len();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = line[hashes..].strip_prefix(' ')?;
    Some(MarkdownBlock::Heading {
        level: hashes as u8,
        spans: parse_inline(rest),
    })
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
fn split_pipe_row(line: &str) -> TableRow {
    split_pipe_cells(line)
        .into_iter()
        .map(|cell| parse_inline(cell.trim()))
        .collect()
}

fn flush_list(list: &mut Vec<String>, ordered: &mut Option<u64>, blocks: &mut Vec<MarkdownBlock>) {
    if !list.is_empty() {
        blocks.push(MarkdownBlock::List {
            ordered: *ordered,
            // Parsed here rather than as each row arrives, because a row may
            // still grow: an item's continuation lines are appended to its
            // source, and inline runs cut before the last of them would split a
            // code span or an emphasis pair across the fold.
            items: std::mem::take(list)
                .iter()
                .map(|item| parse_inline(item))
                .collect(),
        });
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
/// github.css: `#write { max-width: 860px }` on a 16px body — 53.75em, written
/// as 54 because a measure is a reading decision and not a pixel count. It is
/// the single number the user's report is really about: at 1600 physical pixels
/// a paragraph of Chinese ran a hundred and forty characters to the line, and
/// nobody's eye finds the start of the next line after that. Every typographic
/// manual in print puts the comfortable measure between 45 and 75 characters
/// and Typora's 860px lands in the middle of it.
///
/// The column is **centred** when the pane can hold it, which is `#write`'s own
/// `margin: 0 auto`; a pane narrower than the measure wraps at the pane exactly
/// as it did before, because a measure enforced on a 300px pane is a 300px pane
/// with a hole down one side.
pub const PREVIEW_PROSE_MEASURE_EM: f32 = 54.0;
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
        | MarkdownBlock::Math { .. } => (metrics.paragraph_gap, metrics.paragraph_gap),
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
    let mut first: Vec<String> = Vec::new();
    if let Some(duration_ms) = facts.duration_ms {
        first.push(format_duration(duration_ms));
    }
    if let Some((width, height)) = facts.native {
        first.push(format!("{width} \u{00d7} {height}"));
    }
    if first.is_empty() {
        first.extend(
            extension
                .map(str::to_uppercase)
                .filter(|extension| !extension.is_empty()),
        );
    }
    // **The second line says what the file is, and for one column of the class
    // that includes saying it cannot be played** (user ruling 2026-08-27; §7.23
    // ⑩).
    //
    // On the size's line and not on a third one, because the card has two lines
    // and a third would be a card that changed shape for one kind of file
    // (`a_video_card_shows_a_frame_over_two_lines`); and on the *second* rather
    // than the first because the first line is about the recording — how long it
    // runs, how large its picture is — while this is about the container it is
    // in, which is the same thing the size is about.
    //
    // Read off the extension that is already here rather than taken as a second
    // argument: the playable column is a property of the spelling, this function
    // has the spelling, and a caller that had to pass the answer in would be a
    // second place the table is read.
    let mut second: Vec<String> = Vec::new();
    second.extend(facts.bytes.map(format_byte_size));
    if extension.is_some_and(|extension| {
        video_playback_of_extension(extension) == Some(VideoPlayback::FaceOnly)
    }) {
        second.push(crate::i18n::Text::VideoFormatCannotPlay.text().to_owned());
    }
    [
        (!first.is_empty()).then(|| first.join(" \u{b7} ")),
        (!second.is_empty()).then(|| second.join(" \u{b7} ")),
    ]
}

/// Whether a path names a share on another machine.
///
/// §7.1.3, with §3.4's attachment discipline behind it: **a network path is not
/// previewed automatically.** The cost of being wrong is not a slow frame, it is
/// a hover that dials a disconnected share and blocks for the operating system's
/// own timeout, and the read is not something the user asked for by name.
///
/// Decided from the path's *prefix* rather than by looking for two backslashes,
/// because `\\?\C:\…` also starts with two and is as local as a path gets.
pub fn is_network_path(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..))
    )
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
    /// **Set by the first real change and cleared only by a save.** Editing a
    /// file back to the words it started with does not clean it: the buffer has
    /// been through a state the disk never saw, the undo history a real editor
    /// would compare against does not exist here, and a dot that went out
    /// because you happened to retype what you deleted is a dot that cannot be
    /// trusted the one time it matters.
    pub dirty: bool,
    /// How many times this body has changed.
    ///
    /// **A revision, not a length.** Everything derived from the content — the
    /// parsed document, its measured blocks — is cached against this, and the
    /// length it replaced was a counter that could not see a same-length edit:
    /// swapping one letter for another left the cache convinced it was still
    /// looking at the old text.
    pub revision: u64,
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
                if is_network_path(path) {
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
                        PreviewFtype::Unknown => PreviewLoad::Refused(PreviewRefusal::Type),
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
            disk_mtime: None,
            load,
            head_asked: false,
            stale: false,
            max_columns: 0,
        }
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
    pub fn wants_head_read(&self) -> bool {
        self.source.file_path().is_some()
            && (self.load == PreviewLoad::Pending || self.stale)
            && !self.head_asked
            && matches!(
                self.ftype,
                PreviewFtype::Text | PreviewFtype::Markdown | PreviewFtype::Table
            )
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
    #[must_use]
    pub fn claim_head_read(&mut self) -> bool {
        if !self.wants_head_read() {
            return false;
        }
        self.head_asked = true;
        true
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
    pub fn is_editable(&self, md_source: bool) -> bool {
        self.source.file_path().is_some()
            && self.load == PreviewLoad::Ready
            && self.content.is_some()
            && !self.truncated
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
    pub fn edit_content(&mut self, edit: impl FnOnce(&mut String) -> bool) -> bool {
        let Some(content) = self.content.as_mut() else {
            return false;
        };
        if !edit(content) {
            return false;
        }
        self.max_columns = widest_line_columns(self.content.as_deref().unwrap_or_default());
        self.revision += 1;
        self.dirty = true;
        true
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
    /// * The write itself is atomic ([`save_atomically`]).
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
        if let Err(error) = save_atomically(&path, content) {
            return SaveOutcome::Failed(error.to_string());
        }
        self.disk_mtime = file_mtime(&path);
        self.dirty = false;
        SaveOutcome::Saved
    }

    /// The sentence a body that was cut short owes its reader, if it was.
    ///
    /// §7.1.3's "超大文件只读降级": the degradation is not that the file failed,
    /// it is that what is on screen is the beginning of it — and a preview that
    /// showed the first 64KB without saying so would be a preview quietly
    /// claiming the file ends there.
    pub fn truncation_notice(&self) -> Option<&'static str> {
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
        // The question is closed by its answer, whichever lane answered it.
        self.head_asked = false;
        self.stale = false;
        self.content = None;
        self.truncated = false;
        self.max_columns = 0;
        self.disk_mtime = None;
        self.load = PreviewLoad::Unavailable(words);
    }

    /// File the worker's answer.
    pub fn accept(&mut self, outcome: HeadOutcome) {
        self.revision += 1;
        // The question is closed by its answer — and the load it lands in
        // (`Ready`, `Refused`) is already not one this lane asks about, so
        // clearing the bit re-opens nothing. It keeps the bit meaning exactly
        // "a read is out", which is what a reader of it has to be able to
        // believe.
        self.head_asked = false;
        // And so is the watcher's: the body on the glass is the disk's again.
        self.stale = false;
        match outcome {
            HeadOutcome::Read {
                text,
                truncated,
                mtime,
            } => {
                self.max_columns = widest_line_columns(&text);
                self.content = Some(text);
                self.truncated = truncated;
                self.disk_mtime = mtime;
                self.load = PreviewLoad::Ready;
            }
            HeadOutcome::Refused(refusal) => {
                self.content = None;
                self.truncated = false;
                self.max_columns = 0;
                self.disk_mtime = None;
                self.load = PreviewLoad::Refused(refusal);
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
    /// How large the whole file is — the third field of a picture's meta line
    /// (mock-up 4955), which is the only thing on that line the decoder cannot
    /// answer for itself.
    Size,
    /// **The two facts a glance states about a page it cannot render** (user
    /// ruling 2026-08-25) — see [`PageFacts`].
    ///
    /// Its own want rather than [`Self::Size`] and a count asked elsewhere,
    /// because the card prints one body out of both and a body assembled from
    /// two answers landing on two frames is a card that changes under the
    /// pointer.
    PageFacts,
}

/// **What a glance says about a page whose bytes it cannot show** (user ruling
/// 2026-08-25; `docs/DESIGN.md` §7.10 ⑥).
///
/// Both fields are optional and they are optional for the same reason: this is
/// a card that would rather say less than say something untrue. A file the disk
/// will not stat has no size to print, and a structure that will not yield a
/// count has no page number to print — the ruling's own instruction for the
/// second case is "then show the size alone", and the type is what makes that
/// the ordinary path rather than an error case.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PageFacts {
    pub bytes: Option<u64>,
    pub pages: Option<u32>,
}

/// [`PageFacts`] for one file, read on the worker's thread.
///
/// The size comes from a `metadata` call and the count from
/// [`crate::pdf::page_count`], which checks the file's own header before it
/// counts anything — so a `.pdf` that is not a PDF answers with its size and no
/// number, exactly as a PDF whose page tree is out of reach does.
#[must_use]
pub fn read_page_facts(path: &Path) -> PageFacts {
    PageFacts {
        bytes: read_size(path),
        pages: crate::pdf::page_count(path),
    }
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
    /// What a glance card states about a page it cannot render.
    PageFacts(PageFacts),
}

/// How large a file is, without reading it.
pub fn read_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|meta| meta.len())
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
    },
    Refused(PreviewRefusal),
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

/// Where a save stages its bytes before they become the file.
///
/// A sibling of the target and not a `%TEMP%` entry, for the one reason that
/// decides it: [`save_atomically`]'s last step is a rename, and a rename is only
/// atomic *within a volume*. A staging file on another drive would turn the
/// whole guarantee into a copy — which is the non-atomic write this exists to
/// avoid, wearing a temporary name.
///
/// Named from the process as well as the file so two windows saving the same
/// path cannot stage over each other, and dot-prefixed so it is hidden by the
/// same convention every tool on this platform already honours.
pub fn preview_temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.bt-save-{}", std::process::id()))
}

/// Write a file so that it is either the old one or the new one, never half of
/// either.
///
/// **Staged and renamed.** Opening the target and writing into it is the way
/// every editor loses a file to a full disk: the truncate has already happened
/// when the write fails, and what is left is neither version. Here the target is
/// not touched at all until the bytes are on the disk and flushed, and the last
/// step is a single rename the filesystem either performs or does not.
pub fn save_atomically(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    let temp = preview_temp_path(path);
    let staged = (|| {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(contents.as_bytes())?;
        // Flushed before the rename, so a crash between the two cannot leave the
        // *name* switched over to a body that never reached the platter.
        file.sync_all()
    })();
    if let Err(error) = staged {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

/// Read at most [`PREVIEW_HEAD_BYTES`] of a file, and decide what it is.
///
/// **The size question and the binary question are the same read.** Both are
/// facts about the first 64KB, so asking the disk twice would buy nothing but
/// two chances to disagree — the head is taken once, its length answers
/// truncation, and its bytes answer whether this is text at all.
pub fn read_head(path: &Path) -> HeadOutcome {
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
    if let Err(error) = file
        .by_ref()
        .take(PREVIEW_HEAD_BYTES as u64 + 1)
        .read_to_end(&mut head)
    {
        return HeadOutcome::Refused(PreviewRefusal::Fault(PreviewFault::from_io(&error)));
    }
    let truncated = head.len() > PREVIEW_HEAD_BYTES;
    head.truncate(PREVIEW_HEAD_BYTES);
    // Asked of the handle the bytes came out of, not of the path: between two
    // calls by name a file can be replaced entirely, and a stamp belonging to a
    // file other than the one that was read is worse than no stamp at all.
    let mtime = file.metadata().ok().and_then(|meta| meta.modified().ok());
    // The one sniff §7.1.3 asks for, and the only one that is nearly free and
    // nearly never wrong: text does not hold a NUL, and every binary format
    // worth refusing holds one in its first few bytes.
    if head.contains(&0) {
        return HeadOutcome::Refused(PreviewRefusal::Binary);
    }
    HeadOutcome::Read {
        text: decode_head(&head, truncated),
        truncated,
        mtime,
    }
}

/// Turn a head of bytes into text.
///
/// Lossy, because a preview that refuses a file over one bad byte is a preview
/// that refuses log files. The one thing done first is dropping a multi-byte
/// character the *cut* broke in half: that replacement character would be an
/// artefact of the limit rather than of the file, and it would sit at the end of
/// every truncated CJK document.
fn decode_head(head: &[u8], truncated: bool) -> String {
    let head = if truncated {
        trim_partial_utf8(head)
    } else {
        head
    };
    String::from_utf8_lossy(head).into_owned()
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
                        PreviewWant::Size => PreviewAnswer::Size(read_size(path)),
                        PreviewWant::PageFacts => PreviewAnswer::PageFacts(read_page_facts(path)),
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

/// What a click on a markdown link does (user ruling, 2026-08-13).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkAction {
    /// A file, resolved — it opens **here**, in this window's own preview.
    Preview(PathBuf),
    /// A web address, for the system browser.
    Browse(String),
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
/// `http`/`https` keep going to the system browser, which is where the web
/// has always gone. **Every other scheme is refused** — `mailto:`, `ftp:`,
/// `javascript:` and whatever else a document may carry — for the reason the
/// terminal's own OSC-8 handler refuses them: a document is untrusted text, and
/// handing an arbitrary scheme to `ShellExecute` is handing it whatever the
/// machine has registered for that scheme.
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
        return LinkAction::Browse(target.to_owned());
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
        return LinkAction::Preview(normalized(&path));
    }
    match document.parent() {
        Some(directory) => LinkAction::Preview(normalized(&directory.join(path))),
        // A document with no directory is one with no relative frame; there is
        // nowhere for the link to be relative *to*.
        None => LinkAction::Nowhere,
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
        assert!(buffer.claim_head_read(), "the first caller takes the read");
        assert!(
            !buffer.claim_head_read(),
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
            !buffer.claim_head_read(),
            "a refusal is an answer, and the card draws the sentence it earns \
             rather than asking again"
        );

        // A body that arrives is the same shut door, by the other clause.
        let mut read = PreviewBuffer::new(
            PreviewSource::file(r"C:\w\repo\main.rs"),
            "main.rs".to_owned(),
        );
        assert!(read.claim_head_read());
        read.accept(HeadOutcome::Read {
            text: "fn main() {}\n".to_owned(),
            truncated: false,
            mtime: None,
        });
        assert!(!read.claim_head_read(), "there is nothing left to ask");
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
        assert!(buffer.claim_head_read(), "the opening read");
        buffer.accept(HeadOutcome::Read {
            text: "# one\n".into(),
            truncated: false,
            mtime: None,
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

        assert!(buffer.claim_head_read());
        assert!(!buffer.wants_head_read(), "one question, once");
        buffer.accept(HeadOutcome::Read {
            text: "# two\n".into(),
            truncated: false,
            mtime: None,
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
    /// recorded in `docs/HANDOFF-2026-08-21.md` section 5, item 18:
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
        // **And `.mov` since the play verb arrived** (user ruling 2026-08-27,
        // the second of that day; §7.23 ⑩). It is a video with a face and no
        // player, which is a column of [`VIDEO_EXTENSIONS`] and not a class of
        // its own — and it is still not a page, which is what this test is
        // about.
        assert_eq!(preview_ftype("clip.mov"), PreviewFtype::Video);
        assert_eq!(preview_ftype("clip.mkv"), PreviewFtype::Unknown);
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
            // Outside the class by measurement, not by omission.
            "clip.mkv",
            "clip.avi",
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
        for (video, _) in VIDEO_EXTENSIONS {
            assert!(
                !PAGE_EXTENSIONS.iter().any(|(page, _)| *page == video),
                "{video} cannot be both a video and a page"
            );
        }
    }

    /// RED — **the class has a face for every member and a player for only
    /// three of them** (user ruling 2026-08-27, the second of that day; §7.23
    /// ⑩).
    ///
    /// The two columns of one table, asserted as two claims about the same four
    /// names, because the whole of this ruling is that they are *different*
    /// sets and that saying so out loud is better than trimming the class until
    /// they agree. The three that play are the three the engine was measured
    /// with; `.mov` has a face and no player and is the reason the column
    /// exists.
    ///
    /// RED GATE ①: give `mov` [`VideoPlayback::Plays`] and the second half
    /// fails — which is what a build that offered a play button over a
    /// `canPlayType('video/quicktime') == ""` file would be doing.
    /// RED GATE ②: take `mov` back out of the table and the first half fails on
    /// its face, because a name outside the table has no face at all and this
    /// test would be reading the same `None` for both questions.
    #[test]
    fn every_video_has_a_face_and_only_the_measured_three_have_a_player() {
        let path = |name: &str| std::path::PathBuf::from(format!(r"D:\shots\{name}"));
        for name in ["clip.mp4", "trailer.m4v", "screencast.webm"] {
            assert_eq!(
                video_playback(&path(name)),
                Some(VideoPlayback::Plays),
                "{name}"
            );
            assert!(path_names_a_playable_video(&path(name)), "{name}");
        }
        for name in ["capture.mov", "CAPTURE.MOV"] {
            assert_eq!(
                video_playback(&path(name)),
                Some(VideoPlayback::FaceOnly),
                "{name}"
            );
            assert!(!path_names_a_playable_video(&path(name)), "{name}");
            // It has a face, and that is the half a reader loses if the row is
            // deleted rather than marked.
            assert_eq!(preview_ftype(name), PreviewFtype::Video, "{name}");
        }
        for name in ["clip.mkv", "notes.md", "report.html"] {
            assert_eq!(video_playback(&path(name)), None, "{name}");
            assert!(!path_names_a_playable_video(&path(name)), "{name}");
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
        for (extension, _) in PAGE_EXTENSIONS {
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
        for (whole, _) in PAGE_EXTENSIONS {
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
        // And the regression half: a name with no reader at all is refused for
        // its type exactly as it always was.
        let unknown = PreviewBuffer::new(PreviewSource::file(r"C:\w\a.exe"), "a.exe".to_owned());
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
        assert!(!is_editable("README.md", PreviewFtype::Markdown, false));
        assert!(is_editable("README.md", PreviewFtype::Markdown, true));
        assert!(!is_editable("a.png", PreviewFtype::Image, true));
        assert!(!is_editable("a.exe", PreviewFtype::Unknown, true));
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
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A cut that lands inside a character drops the half rather than showing a
    /// replacement the file does not contain.
    #[test]
    fn a_cut_inside_a_character_drops_the_half_it_made() {
        // "你" is three bytes; keep two of them.
        let broken = [0xE4, 0xBD];
        assert_eq!(decode_head(&broken, true), "");
        // The same bytes from a file that simply ends there are the file's own
        // problem, not the limit's, and are shown lossily.
        assert_eq!(decode_head(&broken, false), "\u{fffd}");
        // A whole character at the cut survives.
        let whole = [0xE4, 0xBD, 0xA0];
        assert_eq!(decode_head(&whole, true), "\u{4f60}");
    }

    /// ⑦ A network path is refused without a read.
    ///
    /// Mutation: make [`is_network_path`] test `starts_with(r"\\")` on the
    /// string, which drags `\\?\C:\…` in with it.
    #[test]
    fn a_network_path_is_refused_without_a_read() {
        assert!(is_network_path(Path::new(r"\\server\share\notes.txt")));
        assert!(is_network_path(Path::new(
            r"\\?\UNC\server\share\notes.txt"
        )));
        assert!(!is_network_path(Path::new(r"C:\w\notes.txt")));
        assert!(!is_network_path(Path::new(r"\\?\C:\w\notes.txt")));

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

    /// A type with no reader never asks the disk either.
    ///
    /// Mutation: give [`PreviewFtype::Unknown`] `PreviewLoad::Pending`.
    #[test]
    fn an_unreadable_type_is_refused_before_the_disk_is_asked() {
        let buffer = PreviewBuffer::new(PreviewSource::file(r"C:\w\a.exe"), "a.exe".to_owned());
        assert_eq!(buffer.load, PreviewLoad::Refused(PreviewRefusal::Type));
        assert!(!buffer.wants_head_read());
        let text = PreviewBuffer::new(PreviewSource::file(r"C:\w\a.rs"), "a.rs".to_owned());
        assert_eq!(text.load, PreviewLoad::Pending);
        assert!(text.wants_head_read());
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

    /// PIN — **CommonMark's emphasis, checked against CommonMark's own examples.**
    ///
    /// One table, one row per case, and the left column is the specification's
    /// input verbatim (§6.2, "Emphasis and strong emphasis", version 0.31.2).
    /// The right column is that example's HTML read as *structure*: this window
    /// has no italic face, so `<em>` and `<strong>` both arrive as
    /// [`SpanStyle::Bold`] and the comparison is over where the spans begin and
    /// end rather than over which of the two tags the specification wrote (see
    /// [`resolve_emphasis`]). Every other property of the answer — which
    /// delimiters were spent, which were left as text, and where the text broke —
    /// is compared exactly.
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
            // § the plain pair, either marker.
            ("*foo bar*", vec![Span::bold("foo bar")]),
            ("**foo bar**", vec![Span::bold("foo bar")]),
            ("_foo bar_", vec![Span::bold("foo bar")]),
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
            ("foo*bar*", vec![Span::plain("foo"), Span::bold("bar")]),
            (
                "5*6*78",
                vec![Span::plain("5"), Span::bold("6"), Span::plain("78")],
            ),
            ("foo_bar_", vec![Span::plain("foo_bar_")]),
            ("5_6_78", vec![Span::plain("5_6_78")]),
            ("foo__bar__", vec![Span::plain("foo__bar__")]),
            // § the two markers never pair with each other.
            ("_foo*", vec![Span::plain("_foo*")]),
            // § nesting, and the leftovers of a run that spent part of itself.
            ("*(*foo*)*", vec![Span::bold("(foo)")]),
            ("***foo***", vec![Span::bold("foo")]),
            ("*foo*bar", vec![Span::bold("foo"), Span::plain("bar")]),
            ("**foo*", vec![Span::plain("*"), Span::bold("foo")]),
            ("*foo**", vec![Span::bold("foo"), Span::plain("*")]),
            ("__foo, __bar__, baz__", vec![Span::bold("foo, bar, baz")]),
            // § the multiple-of-3 rule: the inner `**` of the first row cannot
            // pair with either single `*`, so it stays as text inside the span
            // the singles make; the second row is the same rule letting a real
            // pair through.
            ("*foo**bar*", vec![Span::bold("foo**bar")]),
            ("*foo**bar**baz*", vec![Span::bold("foobarbaz")]),
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
    /// `test-assets/latex-render-check.md` stood as literal text) —
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
        let blocks = parse_markdown(include_str!("../../../test-assets/latex-render-check.md"));
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
            LinkAction::Browse("https://example.com/x".to_owned())
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
        assert_eq!(buffer.truncation_notice(), None);
        buffer.accept(read("fn main() {}\n", true));
        assert_eq!(buffer.truncation_notice(), Some(preview_truncated_notice()));
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
    /// The second half is the ruling and the reason the bit is monotonic: the
    /// buffer has been through a state the disk never saw, and a dot that goes
    /// out because you happened to retype what you deleted is a dot nobody can
    /// trust the one time it matters. Only a save cleans it.
    ///
    /// Mutation: set `dirty` from `content != original` rather than from the
    /// fact of an edit; or drop the `if !edit(content)` guard, which dirties a
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
        // Nothing is left behind beside the file.
        assert!(!preview_temp_path(on_disk(&buffer)).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ③ A write that fails leaves the file it was aimed at untouched.
    ///
    /// The failure is injected the only honest way a filesystem allows: the
    /// staging path is occupied by a *directory*, so creating the staging file
    /// cannot succeed. Under the mutation the target is opened directly, the
    /// truncate has already happened when the failure arrives, and what is left
    /// on the disk is neither version.
    ///
    /// Mutation: write straight to `path` in [`save_atomically`] instead of
    /// staging and renaming.
    #[test]
    fn a_failed_write_leaves_the_original_file_whole() {
        let dir = scratch("atomic");
        let path = dir.join("notes.txt");
        std::fs::write(&path, "the original\n").unwrap();
        std::fs::create_dir_all(preview_temp_path(&path)).unwrap();

        let error = save_atomically(&path, "the replacement\n")
            .expect_err("a staging file cannot be created over a directory");
        assert!(!error.to_string().is_empty());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "the original\n",
            "the target was never opened"
        );

        // With the staging path free again the same call goes through.
        std::fs::remove_dir_all(preview_temp_path(&path)).unwrap();
        save_atomically(&path, "the replacement\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "the replacement\n");
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
        assert!(!preview_temp_path(on_disk(&buffer)).exists());

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
    /// staging path a directory is sitting on — so nothing here is mocked: the
    /// real conflict check refuses one and the real `atomic_write` refuses
    /// another.
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
        // And this one's staging path is occupied, so the atomic write refuses.
        let mut refused = opened(&dir, "refused.txt", "as it was\n");
        refused.edit_content(|content| {
            content.push_str("and as it is\n");
            true
        });
        std::fs::create_dir_all(preview_temp_path(on_disk(&refused))).unwrap();
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
        assert_eq!(
            std::fs::read_to_string(dir.join("refused.txt")).unwrap(),
            "as it was\n",
            "and the file the write could not reach was never opened"
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
        // #write { max-width: 860px } on a 16px body.
        assert_eq!(metrics.measure, 702.0);

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

        let wide = [100.0, 0.0, 1300.0, 600.0];
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
        assert_eq!(left, 349.0);
        assert_eq!(right, 1051.0);

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
        let source = include_str!("../../../test-assets/preview-samples/stress.md");
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
        assert!(!buffer.claim_head_read());
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
}
