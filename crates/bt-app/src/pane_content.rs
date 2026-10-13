//! **What one preview surface is showing** — [`crate::PreviewPane`]'s content as
//! one enum (the ownership contract's §2.2 invariant 2, ticket O-8).
//!
//! A surface shows nothing, a pool buffer, or a picture. A pool buffer is a
//! document, a page or a commit graph, and each of the three is a
//! [`DocumentView`]: the buffer's name and how this surface is looking at its
//! text. A picture is not a pool buffer; it is a [`PictureView`].
//!
//! The fields are private to this module, so every write is one of the doors
//! below, and a document view is built only from a [`preview::Held`] — a source
//! its pool was found holding.

use crate::{
    DocumentPictures, ImageZoom, PeekThumbnailTarget, PictureRefusal, PreviewDocument,
    PreviewDocumentKey, PreviewImageState, PreviewLink, PreviewTextBox, PreviewViewState, Reparse,
    preview, preview_edit, preview_live, preview_select,
};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

/// **What this surface is showing.** Exactly one of these at a time, which is
/// what "a picture and a buffer clear each other on the way in" says as a type.
#[derive(Default)]
pub(crate) enum PaneContent {
    /// Nothing: a surface that has never shown anything, or has let go of what
    /// it showed.
    #[default]
    Empty,
    /// A file, or one of git's readings of a file (a diff, a commit's show, a
    /// range).
    Document(DocumentView),
    /// A page. It is a buffer in the same pool (`docs/DESIGN.md` §7.7 ①), and
    /// its view is the one the page's source face reads through when the
    /// surface is turned to it (§7.7 ⑭).
    Page(DocumentView),
    /// One repository's commit graph — a buffer in the same pool, keyed by the
    /// repository. The graph itself is drawn from the tab's graph state; the
    /// view is what every pool buffer's body is built and keyed through.
    GitGraph(DocumentView),
    /// A picture, decoded down its own lane. Not a pool buffer.
    Picture(PictureView),
}

impl PaneContent {
    /// The variant `view`'s source is: a page, a graph, or a document.
    fn of(view: DocumentView) -> Self {
        match &view.source {
            preview::PreviewSource::Web(_) => Self::Page(view),
            preview::PreviewSource::GitGraph { .. } => Self::GitGraph(view),
            preview::PreviewSource::File(_)
            | preview::PreviewSource::GitDiff { .. }
            | preview::PreviewSource::GitShow { .. }
            | preview::PreviewSource::GitDiffRange { .. } => Self::Document(view),
        }
    }

    /// **Show the buffer `held` names.** A view already on a buffer keeps how it
    /// was looking (its caret, its scroll, its parse) and only changes which
    /// buffer it names — a page that navigated, a file under its new name, a
    /// restored seat. Anything else gets a fresh view.
    pub(crate) fn showing(self, held: preview::Held) -> Self {
        let view = match self {
            Self::Document(mut view) | Self::Page(mut view) | Self::GitGraph(mut view) => {
                view.follow(held);
                view
            }
            Self::Empty | Self::Picture(_) => DocumentView::on(held),
        };
        Self::of(view)
    }

    /// **Show this picture.** A picture replacing a picture keeps the zoom (the
    /// door that opens a picture resets it on purpose; see
    /// [`PictureView::zoom`]); anything else starts at fit.
    pub(crate) fn picturing(self, image: PreviewImageState) -> Self {
        let zoom = match &self {
            Self::Picture(picture) => picture.zoom,
            Self::Empty | Self::Document(_) | Self::Page(_) | Self::GitGraph(_) => {
                ImageZoom::default()
            }
        };
        Self::Picture(PictureView { image, zoom })
    }

    /// The view of the pool buffer this surface is on, when it is on one.
    pub(crate) fn document(&self) -> Option<&DocumentView> {
        match self {
            Self::Document(view) | Self::Page(view) | Self::GitGraph(view) => Some(view),
            Self::Empty | Self::Picture(_) => None,
        }
    }

    /// The same, mutably.
    pub(crate) fn document_mut(&mut self) -> Option<&mut DocumentView> {
        match self {
            Self::Document(view) | Self::Page(view) | Self::GitGraph(view) => Some(view),
            Self::Empty | Self::Picture(_) => None,
        }
    }

    /// The picture this surface is showing, when it is showing one.
    pub(crate) fn picture(&self) -> Option<&PictureView> {
        match self {
            Self::Picture(picture) => Some(picture),
            Self::Empty | Self::Document(_) | Self::Page(_) | Self::GitGraph(_) => None,
        }
    }

    /// The same, mutably.
    pub(crate) fn picture_mut(&mut self) -> Option<&mut PictureView> {
        match self {
            Self::Picture(picture) => Some(picture),
            Self::Empty | Self::Document(_) | Self::Page(_) | Self::GitGraph(_) => None,
        }
    }
}

/// **How one surface is looking at one pool buffer**: which buffer, its parsed
/// body and the measurement it was parsed at, and the view of it — scroll,
/// per-block scroll, the caret, both selection models, the links and pieces
/// measured this frame, the sentence the foot owes.
///
/// Built only by [`Self::on`], from a [`preview::Held`], so the buffer it names
/// was in its pool when the view was made — and the pool's eviction law never
/// evicts a buffer a pane is reading.
pub(crate) struct DocumentView {
    /// Which buffer this surface is showing, by [`preview::PreviewSource`].
    ///
    /// An identity rather than an index or a reference: the pool is a `Vec`
    /// whose order is its history, and an index into a list that evicts is an
    /// index that goes stale between the frame that took it and the frame that
    /// reads it.
    source: preview::PreviewSource,
    /// The parsed body, and what it was parsed from.
    doc: PreviewDocument,
    doc_key: Option<PreviewDocumentKey>,
    /// How wide one monospace cell is at the body's type size, measured when the
    /// document was parsed. It is what the horizontal extent is derived from.
    mono_advance: f32,
    /// How far this surface's body is scrolled, in physical pixels.
    ///
    /// `[x, y]`: `white-space: pre` means a line that does not fit runs off the
    /// edge, and a body that can only be scrolled downward would leave the end
    /// of every long line permanently out of reach.
    scroll: [f32; 2],
    /// How far each markdown block that is wider than the page has been scrolled
    /// inside itself (user ruling, 2026-08-13). One entry per block, by index.
    md_block_scroll: Vec<f32>,
    /// Every markdown link on screen, boxed where it was drawn — a paint
    /// artifact rather than state, rebuilt with the body.
    links: Vec<PreviewLink>,
    /// **What is selected on this surface's rendered page**, in the document's
    /// own terms (user report 2026-08-28).
    ///
    /// [`Self::caret`]'s opposite number, and the split is the same one the
    /// surface's face flip draws: that is the *source* face's caret, indexing
    /// one flat string, and this is the *rendered* face's selection, addressing
    /// blocks and pieces. A surface shows one face at a time and the other's
    /// state stands by; both are the surface's rather than the buffer's, for
    /// ruling 8⑧'s reason — the file is shared, the way you are looking at it is
    /// not.
    ///
    /// It survives a scroll because a [`preview_select::Place`] names the
    /// document rather than the glass, and it does **not** survive a re-parse:
    /// see [`Self::show_document`].
    md_select: Option<preview_select::Selection>,
    /// **Whether the caret is standing on this surface's *rendered* face**
    /// (T5, §7.1.3t) — which is the whole of "this page is being edited".
    ///
    /// The one piece of state entering and leaving are made of. A press in the
    /// body sets it, `Esc` clears it, and while it is set the block the caret is
    /// in is drawn as the file's own bytes ([`preview_live`]) and every key is
    /// the editor's.
    ///
    /// **It is not the keyboard focus**, and the gap between the two is
    /// deliberate. `Runtime::preview_edit_focus` is dropped by a press anywhere
    /// else in the window — into the terminal beside this pane, onto another
    /// tab — and a page that re-flowed itself every time a hand left it would
    /// be a page that moves while nobody is looking at it (§7.1.3q). So losing
    /// the keyboard puts the *caret* out (`MarkdownCaretPaint::lit`) and leaves
    /// the source block standing; only leaving on purpose renders it again.
    /// What survives either way is the caret's own byte offset, which is
    /// [`Self::caret`] and is shared with the source face.
    md_caret: bool,
    /// **A press that asked to edit and has not been able to yet** (T5 ①) — the
    /// file byte it named, waiting for the body it names to arrive.
    ///
    /// [`Self::goto_line`]'s shape and its reason exactly. A press inside a page
    /// whose glance only bought the file's head buys the rest of it (T2 ③), and
    /// the read is a read: the bytes are on the worker for as long as a disk
    /// takes. Without this the press would be spent — the reader would click,
    /// watch nothing happen, and click again — so the offset waits here until
    /// `Runtime::settle_preview_caret` has a body to put it in.
    ///
    /// A file offset and not the point pressed, because the head is a prefix of
    /// the whole file: the byte the pointer named in the head is the same byte
    /// of the same file when the rest of it lands, whatever the page does to
    /// re-flow around it.
    md_caret_wanted: Option<usize>,
    /// Every piece of the rendered page on screen, boxed where it was drawn — a
    /// paint artifact rather than state, rebuilt with the body exactly as
    /// [`Self::links`] is, and read by the pointer that arrives between two
    /// frames.
    md_text: Vec<PreviewTextBox>,
    /// **The caret's prose block, as the shaper that drew it laid it out**
    /// (§7.1.3w) — [`Self::md_text`]'s sibling, and a paint artifact for the
    /// same reason it is one: it is what the last body build asked the shaper,
    /// and the next thing to ask about it is a pointer or a keystroke that
    /// arrives between two frames.
    ///
    /// `None` on every page whose caret is in no block, and on every page whose
    /// caret's block is drawn in the monospace face — that one is a grid and
    /// answers out of its own arithmetic (`markdown_source_cell`).
    ///
    /// **Five readers, one value**: the caret's own bar, the band under a
    /// selection, the candidate box an IME hangs, the byte a press names, and
    /// the rows Up and Down walk. §7.1.3u is the account of what a second
    /// derivation of any one of those costs.
    md_prose: Option<preview_live::ProseRows>,
    /// **Every prose source block on the glass, as the shaper laid it out**
    /// (2026-09-23) — [`Self::md_prose`] for each block a selection has drawn as
    /// source and not only the caret's, which is always one of them.
    ///
    /// Two readers: the band a selection draws over each of those blocks, and
    /// the byte a press inside one of them names. The caret, the composition
    /// and the arrow keys stand in exactly one block and go on reading
    /// [`Self::md_prose`].
    md_prose_blocks: Vec<preview_live::ProseRows>,
    /// Where the caret is in this surface's buffer, and what it has selected.
    caret: preview_edit::EditCaret,
    /// The sentence this surface's body owes about its last save, and when it
    /// was said.
    notice: Option<(String, Instant)>,
    /// A line this surface was opened at and has not been able to reach yet
    /// (§7.1.5j, `path:line[:col]`).
    ///
    /// It is an **intent rather than a scroll** because the body is read off a
    /// worker: the frame that opens the file knows its name and nothing about its
    /// lines, so a number written straight into [`Self::scroll`] would be clamped
    /// against an empty document and healed back to zero. It waits here — on the
    /// pane, so it travels with the pane into a torn-off window exactly as the
    /// rescale ledger does — until `Runtime::settle_preview_goto` has something
    /// to measure, and is cleared the moment it is spent.
    goto_line: Option<u32>,
}

impl DocumentView {
    /// **A fresh view of the buffer `held` names**: at its top, nothing
    /// selected, nothing parsed yet.
    fn on(held: preview::Held) -> Self {
        Self {
            source: held.into_source(),
            doc: PreviewDocument::Empty,
            doc_key: None,
            mono_advance: 0.0,
            scroll: [0.0, 0.0],
            md_block_scroll: Vec::new(),
            links: Vec::new(),
            md_select: None,
            md_caret: false,
            md_caret_wanted: None,
            md_text: Vec::new(),
            md_prose: None,
            md_prose_blocks: Vec::new(),
            caret: preview_edit::EditCaret::default(),
            notice: None,
            goto_line: None,
        }
    }

    /// **Name the buffer `held` names instead**, keeping how this surface was
    /// looking.
    fn follow(&mut self, held: preview::Held) {
        self.source = held.into_source();
    }

    // ── what is read ────────────────────────────────────────────────────────

    pub(crate) fn source(&self) -> &preview::PreviewSource {
        &self.source
    }

    pub(crate) fn doc(&self) -> &PreviewDocument {
        &self.doc
    }

    pub(crate) fn doc_key(&self) -> Option<&PreviewDocumentKey> {
        self.doc_key.as_ref()
    }

    pub(crate) fn mono_advance(&self) -> f32 {
        self.mono_advance
    }

    pub(crate) fn scroll(&self) -> [f32; 2] {
        self.scroll
    }

    pub(crate) fn md_block_scroll(&self) -> &[f32] {
        &self.md_block_scroll
    }

    pub(crate) fn links(&self) -> &[PreviewLink] {
        &self.links
    }

    pub(crate) fn md_select(&self) -> Option<&preview_select::Selection> {
        self.md_select.as_ref()
    }

    pub(crate) fn md_caret(&self) -> bool {
        self.md_caret
    }

    pub(crate) fn md_caret_wanted(&self) -> Option<usize> {
        self.md_caret_wanted
    }

    pub(crate) fn md_text(&self) -> &[PreviewTextBox] {
        &self.md_text
    }

    pub(crate) fn md_prose(&self) -> Option<&preview_live::ProseRows> {
        self.md_prose.as_ref()
    }

    pub(crate) fn md_prose_blocks(&self) -> &[preview_live::ProseRows] {
        &self.md_prose_blocks
    }

    pub(crate) fn caret(&self) -> preview_edit::EditCaret {
        self.caret
    }

    pub(crate) fn notice(&self) -> Option<&(String, Instant)> {
        self.notice.as_ref()
    }

    pub(crate) fn goto_line(&self) -> Option<u32> {
        self.goto_line
    }

    /// The caret and the scroll, to be filed against this buffer.
    pub(crate) fn view_state(&self) -> PreviewViewState {
        PreviewViewState {
            caret: self.caret,
            scroll: self.scroll,
        }
    }

    // ── the doors ───────────────────────────────────────────────────────────

    /// **Stand where this surface was last left on this buffer**: the caret
    /// and the scroll filed for it.
    pub(crate) fn stand(&mut self, view: PreviewViewState) {
        self.caret = view.caret;
        self.scroll = view.scroll;
    }

    /// **Put a freshly parsed document on this surface**, and let go of
    /// anything selected in the last one.
    ///
    /// One door rather than an assignment, because the two writes must not be
    /// able to come apart: a [`preview_select::Place`] is an offset into a piece
    /// of a *particular* parse, and the commonest reason a parse is replaced is
    /// that the file changed under it — a save from another editor, or this
    /// surface's own source face. A selection left standing over that is a
    /// highlight drawn on somebody else's sentence, and a `Copy` that took it
    /// would put those bytes on the clipboard.
    ///
    /// The boxes go with it for a plainer reason: they are where the *last*
    /// document was drawn, and nothing has drawn this one yet.
    ///
    /// **Except when the parse is our own** (§7.1.3q, research open question 15
    /// — "the single most dangerous line in T4"). The rule above was written
    /// when the only thing that could replace a parse was the file changing
    /// underneath it. A document that re-parses on every keystroke re-parses for
    /// a second reason now, and it is the opposite reason: the bytes are the
    /// reader's own, typed a moment ago, and everything they had marked still
    /// names the words they marked. Dropping it there would mean a selection
    /// that could not survive being typed next to, which is the one thing an
    /// editor may not do. See [`Reparse`] for how the two are told apart.
    pub(crate) fn show_document(&mut self, doc: PreviewDocument, reparse: Reparse) {
        self.md_block_scroll = match &doc {
            PreviewDocument::Markdown { wrap, .. } => wrap.viewport.offsets(&self.md_block_scroll),
            PreviewDocument::Empty
            | PreviewDocument::Text { .. }
            | PreviewDocument::Diff(_)
            | PreviewDocument::Table { .. } => Vec::new(),
        };
        self.doc = doc;
        // **The boxes go whatever happened.** They are where the *last* document
        // was drawn — a geometry, not a mark — and this one has not been drawn
        // yet.
        self.md_text = Vec::new();
        if reparse == Reparse::Ours {
            return;
        }
        self.md_select = None;
        // **The caret is not thrown away; what it had dragged over is.** A
        // caret is a byte offset into the buffer's own body and every door that
        // moves that body already puts it back inside it
        // (`preview_edit::EditCaret::heal`, at the arrival of a disk copy and at
        // the head of every edit and every motion), while a *session* restores
        // one from [`PreviewViewState`] before the file it belongs to has even
        // landed — so a re-parse that zeroed it would throw away the place a
        // reader was last at every time a buffer was re-read. What cannot
        // survive is the mark: a selection is a claim about two ends of somebody
        // else's text, and both selection models on this surface drop it here.
        self.caret.anchor = self.caret.caret;
    }

    /// **The same document, laid out again** — a pane that changed width, or a
    /// formula whose picture has landed.
    ///
    /// [`Self::show_document`]'s other half, and a door of its own precisely so
    /// that the difference between them is stated rather than implied: the parse
    /// has not changed, so every place a selection names still names the same
    /// words, and dropping it would mean a reader lost their highlight to a
    /// window drag or to a picture arriving while they read.
    pub(crate) fn reflow_document(&mut self, doc: PreviewDocument) {
        self.doc = doc;
    }

    /// The parse, taken off this surface for a rebuild that hands it back
    /// through [`Self::show_document`] or [`Self::reflow_document`].
    pub(crate) fn take_document(&mut self) -> PreviewDocument {
        std::mem::take(&mut self.doc)
    }

    /// **The page's viewport is this tall now**, and nothing else about the
    /// layout has moved.
    pub(crate) fn fit_viewport_height(&mut self, height: f32) {
        if let PreviewDocument::Markdown { wrap, .. } = &mut self.doc {
            Arc::make_mut(wrap).viewport.fit_height(height);
        }
    }

    /// Drop a page's picture answers when any of them is of the file whose
    /// path key is `key`.
    pub(crate) fn forget_pictures_of(&mut self, key: &str) {
        if let PreviewDocument::Markdown { pictures, .. } = &mut self.doc
            && pictures
                .files
                .iter()
                .any(|file| bt_term::normalized_local_image_path_key(file) == key)
        {
            *pictures = DocumentPictures::default();
        }
    }

    /// Which parse this surface's document is, as its key says.
    pub(crate) fn key_document(&mut self, key: Option<PreviewDocumentKey>) {
        self.doc_key = key;
    }

    /// Which source blocks the parse in hand drew as source, written into its
    /// key.
    pub(crate) fn key_parsed_source(&mut self, parsed: Option<preview_live::SourceSpan>) {
        if let Some(key) = self.doc_key.as_mut() {
            key.source = parsed;
        }
    }

    /// How wide one monospace cell is at the body's type size.
    pub(crate) fn measure_advance(&mut self, advance: f32) {
        self.mono_advance = advance;
    }

    /// Put the caret here.
    pub(crate) fn place_caret(&mut self, caret: preview_edit::EditCaret) {
        self.caret = caret;
    }

    /// Pull the caret back inside `content`, the body that has just arrived
    /// under it.
    pub(crate) fn heal_caret(&mut self, content: &str) {
        self.caret.heal(content);
    }

    /// Select this on the rendered page, or nothing.
    pub(crate) fn select(&mut self, selection: Option<preview_select::Selection>) {
        self.md_select = selection;
    }

    /// **Let go of what is marked on both faces**: the caret's anchor comes to
    /// the caret, and the rendered page's selection goes.
    pub(crate) fn drop_selection(&mut self) {
        self.caret.anchor = self.caret.caret;
        self.md_select = None;
    }

    /// **The caret stands on the rendered page now** — entering: the page
    /// draws the caret's block as source, and the press that was waiting for a
    /// body is spent.
    pub(crate) fn enter_page(&mut self) {
        self.md_caret = true;
        self.md_caret_wanted = None;
    }

    /// **The caret leaves the rendered page** — the page draws every block as
    /// prose again, and no press is waiting any more.
    pub(crate) fn leave_page(&mut self) {
        self.md_caret = false;
        self.md_caret_wanted = None;
    }

    /// A press asked to edit at this file byte, or no press is waiting.
    pub(crate) fn want_caret_at(&mut self, offset: Option<usize>) {
        self.md_caret_wanted = offset;
    }

    /// Scroll the body to `[x, y]`, in physical pixels.
    pub(crate) fn scroll_to(&mut self, scroll: [f32; 2]) {
        self.scroll = scroll;
    }

    /// Scroll the body down to `top`, leaving the sideways offset alone.
    pub(crate) fn scroll_top_to(&mut self, top: f32) {
        self.scroll[1] = top;
    }

    /// **Scroll the wide block at `index` sideways to `offset`.** Answers
    /// whether it moved.
    pub(crate) fn scroll_block(&mut self, index: usize, offset: f32) -> bool {
        // Grown here rather than at the rebuild, because a document is parsed
        // far more often than a block is scrolled and a vector of zeros per
        // parse is a cost paid for a case that usually does not arise.
        if self.md_block_scroll.len() <= index {
            self.md_block_scroll.resize(index + 1, 0.0);
        }
        if self.md_block_scroll[index] == offset {
            return false;
        }
        self.md_block_scroll[index] = offset;
        true
    }

    /// Keep the boxes the body build drew the links in.
    pub(crate) fn keep_links(&mut self, links: Vec<PreviewLink>) {
        self.links = links;
    }

    /// Keep the boxes the body build drew the rendered page's pieces in.
    pub(crate) fn keep_text_boxes(&mut self, boxes: Vec<PreviewTextBox>) {
        self.md_text = boxes;
    }

    /// Keep the prose rows the shaper laid out: the caret's block, and every
    /// prose block drawn as source.
    pub(crate) fn keep_prose(
        &mut self,
        prose: Option<preview_live::ProseRows>,
        blocks: Vec<preview_live::ProseRows>,
    ) {
        self.md_prose = prose;
        self.md_prose_blocks = blocks;
    }

    /// The sentence the body owes about its last save, or none.
    pub(crate) fn say(&mut self, notice: Option<(String, Instant)>) {
        self.notice = notice;
    }

    /// Open at this line once the body has rows, or the intent is spent.
    pub(crate) fn aim_at_line(&mut self, line: Option<u32>) {
        self.goto_line = line;
    }
}

/// **How one surface is looking at one picture**: the picture, and the zoom.
///
/// No `&mut` to either field leaves this type, and neither does one to a
/// [`DocumentView`] field: the ownership census counts a `&mut` lend as an
/// access and not as a write, so a write made through one would be a second
/// writer it cannot see. Every change is one of the doors below.
pub(crate) struct PictureView {
    image: PreviewImageState,
    /// How this surface is looking at the picture (ticket #60) —
    /// [`DocumentView`]'s scroll's opposite number, and on the surface for the
    /// same reason it is: a document's scroll and a picture's zoom are the two
    /// answers to "where in this content are you", and both belong to the
    /// surface rather than to the thing being looked at.
    ///
    /// Not persisted and not on the buffer. It is kept when a picture is
    /// replaced by another ([`PaneContent::picturing`]), so the door that opens
    /// a picture resets it explicitly (`Runtime::open_preview_image_on`),
    /// because a new file arriving at 250% because the last one was left there
    /// is the surface remembering something about a picture it has never seen.
    zoom: ImageZoom,
}

impl PictureView {
    pub(crate) fn image(&self) -> &PreviewImageState {
        &self.image
    }

    /// Why there is no picture here, or nothing to say.
    pub(crate) fn refuse(&mut self, why: Option<PictureRefusal>) {
        self.image.failure = why;
    }

    /// Why there is no picture here — unless a reason is already standing.
    pub(crate) fn refuse_unless_said(&mut self, why: PictureRefusal) {
        self.image.failure.get_or_insert(why);
    }

    /// A resample at this size has been asked for, and nothing is refused.
    pub(crate) fn ask_scale(&mut self, target: PeekThumbnailTarget) {
        self.image.pending = Some(target);
        self.image.failure = None;
    }

    /// A resample at this size was asked for and its work failed: nothing is out any more, so
    /// the surface's next resolve asks again.
    pub(crate) fn forget_scale(&mut self, target: &PeekThumbnailTarget) {
        if self.image.pending.as_ref() == Some(target) {
            self.image.pending = None;
        }
    }

    /// A resample has landed; answers whether it was the one asked for.
    pub(crate) fn accept_scaled(&mut self, scaled: bt_term::ScaledInlineImage) -> bool {
        self.image.accept_scaled(scaled)
    }

    /// A gesture moved the boundary the exact resample waits for.
    pub(crate) fn defer_scale_settle(&mut self, observed_at: Instant) {
        self.image.defer_scale_settle(observed_at);
    }

    /// Whether the boundary has gone quiet, settling it if it has.
    pub(crate) fn finish_scale_settle_if_quiet(&mut self, now: Instant) -> bool {
        self.image.finish_scale_settle_if_quiet(now)
    }

    /// Where the picture was drawn this frame, or that it was not.
    pub(crate) fn place_drawn(&mut self, drawn: Option<[f32; 4]>) {
        self.image.drawn = drawn;
    }

    /// The decode's native size, and the size the file states when the two
    /// part.
    pub(crate) fn hear_native(&mut self, native: (u32, u32), stated: Option<(u32, u32)>) {
        self.image.native = Some(native);
        self.image.stated_size = stated;
    }

    /// **The picture at `path` is `bytes` long**, if it is this one. Answers
    /// whether it was.
    pub(crate) fn hear_size(&mut self, path: &Path, bytes: Option<u64>) -> bool {
        if self.image.path != path {
            return false;
        }
        self.image.bytes = bytes;
        true
    }

    /// Drop the decoded pixels when this is the picture whose path key is
    /// `key`.
    pub(crate) fn forget_pixels_of(&mut self, key: &str) {
        if bt_term::normalized_local_image_path_key(&self.image.path) == key {
            self.image.forget_its_pixels();
        }
    }

    pub(crate) fn zoom(&self) -> ImageZoom {
        self.zoom
    }

    /// Look at the picture at this zoom.
    pub(crate) fn zoom_to(&mut self, zoom: ImageZoom) {
        self.zoom = zoom;
    }
}
