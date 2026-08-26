//! The window's own words, in the two languages this build speaks.
//!
//! # Why a table in Rust and not a resource file
//!
//! Every string in this product arrived with a *reason* beside it — a mock-up
//! line number, a wording that was tried and rejected, a length budget measured
//! against a pane. Those reasons are the expensive half; the words are the cheap
//! half. A `.ftl` or a `.json` would move the words out of the file and leave the
//! reasons behind, which is the one refactor that makes this codebase worse: the
//! next person to shorten a sentence would no longer be standing next to the
//! paragraph explaining why it is that length. So the table is Rust, the two
//! literals sit on one line of one `match` arm, and the doc comment that argued
//! for the English one now argues for both.
//!
//! It also keeps the signature the rest of the app already has.
//! [`crate::settings::SettingsRow::title`], `seat_title`, `DirFault::notice`,
//! `PreviewRefusal::notice` and a dozen others return `&'static str`, and their
//! callers hand the answer straight to a `ChromeLabel`. Choosing between two
//! literals is still `&'static str`, so this whole pass costs **zero
//! allocations** — where a resource lookup would have turned every one of those
//! into a `String` on a dialog that redraws on hover.
//!
//! # Why the language switches where you are standing
//!
//! It did not, until 2026-08-17, and the reason it did not is worth keeping
//! because it is the reason the fix has the shape it has. Theme could always be
//! switched live because the theme had a *revision*:
//! `bt_render::theme_revision()` feeds `LayoutKey.theme_rev`, so every cached
//! measurement is invalidated the instant the palette changes. Language had no
//! such number, and a language needs one more than a palette does — it changes
//! the **widths**, and this window measures widths in a great many places: the
//! `˅` menu's `min-width: 180`, the root menu's 190, the settings picker's 118,
//! `preview_button_width`, the files foot's `ellipsized_left` result, the
//! restore prompt's wrapped lines.
//!
//! What made the switch safe was reading those places instead of counting them.
//! Every one of them is re-derived by `Runtime::refresh_chrome`, which builds
//! the strip, the tooltip anchors, the feet and the whole overlay stack from
//! scratch on the frame it is called — the settings dialog measures its widest
//! option per call and says so in `settings_layout`'s own comment. So the
//! language's invalidation list is **not** the terminal font's: a face change
//! moves the cell every glyph in the grid is drawn in, and a language change
//! moves nothing in the grid at all. It moves the window's own words, and the
//! window's own words are rebuilt every frame that asks for them.
//!
//! Which leaves exactly three things that outlive a `refresh_chrome`, and they
//! are the three this slice touches:
//!
//! 1. **[`current`] itself** — an atomic now, not a `OnceLock`, so [`install`]
//!    can be called again. It is read from the layout code on the UI thread and
//!    from the PSReadLine probe's thread, hence `Relaxed` loads of a plain word
//!    rather than a lock.
//! 2. **[`lang_revision`]** — the number that did not exist. It feeds
//!    `bt_doc::LayoutKey::lang_rev`, which is what makes the language a member
//!    of the layout's identity rather than an ambient fact about it.
//! 3. **`crate::psreadline::row_description`'s three `OnceLock<String>`** — the
//!    only process-lifetime string cache in this app that is built out of this
//!    table. It is keyed by [`Lang`] now, which is why it has room for six.
//!
//! Everything else was audited and is language-neutral by construction:
//! `profiles::title`'s cache holds names this file deliberately does not carry,
//! `settings::monospace_families` and `settings::scheme_labels` hold names out
//! of the machine and out of a folder, and `cmdrail::RailCache` contains no
//! string from this table at all.
//!
//! **What is not retranslated is what has already been said.** A toast on
//! screen, the files pane's 1300ms answer to a double-click, a preview's failure
//! banner: those are reports of a moment, they were true in the language they
//! were said in, and half of them carry an `io::Error`'s own sentence which has
//! no second column anywhere. They are not state the window is describing; they
//! are things the window told you.
//!
//! # What is deliberately **not** in this table
//!
//! * **The five profile titles** — `PowerShell 7`, `Windows PowerShell 5.1`,
//!   `WSL`, `Git Bash`, `Command Prompt`. They are compared byte for byte against
//!   the OSC 2 title the shell reports (`main.rs`'s `title_layer(...).filter(...)`
//!   guard), so a translated title would stop matching and every tab would
//!   silently start showing the program's name instead of the profile's. If
//!   Chinese profile names are ever wanted, `Profile` grows a `display_title`
//!   beside `title`; the comparison keeps the original.
//! * **`bt_platform::PROGRAM_REFUSED`** — a sentinel matched with `.contains`,
//!   which happens to read like [`Text::FilesProgramRefused`] and is not it.
//! * **Shell text.** OSC 2 titles, OSC 7 folders, `io::Error` strings and every
//!   byte a program writes into the grid are the shell's words, not ours.
//! * **Technical tokens** inside sentences: `git`, `LaTeX`, `$$…$$`, `Folio`,
//!   `64 KB`, key caps. They stay in both languages because they are names.

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

/// The language the window is drawn in — the *resolved* answer.
///
/// Two variants and not a locale string, because the table has two columns. A
/// third language is a third variant and a third literal per arm, which is the
/// shape this was chosen for; a `&str` key would be the shape that compiles with
/// a typo in it.
///
/// `repr(u8)` because the live answer is an atomic word — see [`Current`]. The
/// discriminants are also the indices of the per-language cache slots
/// `crate::psreadline::row_description` keeps, which is what [`Self::COUNT`] and
/// [`Self::from_index`] are for: a third variant then fails to compile in the
/// one place that would otherwise have silently kept two.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum Lang {
    #[default]
    English = 0,
    Chinese = 1,
}

impl Lang {
    /// How many languages this build speaks.
    pub const COUNT: usize = 2;

    /// Every one of them, for the tests that have to read both columns.
    ///
    /// Test-only, like [`Text::ALL`]: nothing the window does walks the
    /// languages — it draws the one in force — and a constant the product
    /// carried only so that a test could read it would be shipped weight.
    #[cfg(test)]
    pub const ALL: [Self; Self::COUNT] = [Self::English, Self::Chinese];

    /// Which slot of a per-language array this language owns.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The inverse, for reading the answer back out of an atomic word.
    ///
    /// Anything that is not a discriminant is [`Self::English`] rather than a
    /// panic, and the only writer is [`Current::set`], so the arm cannot be
    /// reached — it is the same "English is the source language, not a
    /// fallback" answer [`resolve`] gives, expressed where the compiler asks
    /// for it.
    #[must_use]
    const fn from_index(index: u8) -> Self {
        match index {
            1 => Self::Chinese,
            _ => Self::English,
        }
    }
}

/// What the user asked for — the *stored* preference.
///
/// Distinct from [`Lang`] for exactly the reason `ThemeModeV1` is distinct from
/// the palette in use: `System` is an answer about the question. Someone who
/// picks it is saying "ask Windows", and storing the resolved answer instead
/// would freeze today's Windows setting into the file — so that a user who later
/// switched Windows to Chinese would find Folio still in English with no record
/// of why.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LanguageMode {
    #[default]
    System,
    English,
    Chinese,
}

/// Which language a mode comes out as on a machine whose Windows reads
/// `os_ui_language`.
///
/// The tag is matched on its **primary subtag only** (`zh`), which is the whole
/// of the rule: `zh-Hans-CN`, `zh-CN`, `zh-TW`, `zh-Hant-HK` and a bare `zh` all
/// want Chinese, and this build has one Chinese to give them. Traditional is a
/// separate column the day somebody asks for it, and it will be a third
/// [`Lang`] variant rather than a regex on this tag.
///
/// Everything else is English, because English is the source language rather
/// than a fallback: a Japanese Windows gets the words the product was written
/// in, which is the same thing every user got before this table existed.
#[must_use]
pub fn resolve(mode: LanguageMode, os_ui_language: &str) -> Lang {
    match mode {
        LanguageMode::English => Lang::English,
        LanguageMode::Chinese => Lang::Chinese,
        LanguageMode::System => {
            let primary = os_ui_language
                .split(['-', '_'])
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if primary == "zh" {
                Lang::Chinese
            } else {
                Lang::English
            }
        }
    }
}

/// The language in force, and how many times it has moved.
///
/// A type rather than two loose statics, and the reason is testability and not
/// tidiness: the whole of the swap's contract — the answer changes, the revision
/// advances, installing what is already in force advances nothing, and every
/// thread sees it — is a property of *this*, and a test can build one of its own
/// and pin all four without touching the process's. Flipping the real one under
/// a test suite that runs its cases in parallel would leave every other test
/// that reads a word off this table racing a language it did not ask for.
///
/// Two words and not one `RwLock<(Lang, u64)>`, because [`current`] is called
/// from inside layout loops that measure one label at a time: it has to be an
/// unlocked load. The pair is never read atomically together and never needs to
/// be — a reader that saw the new language with the old revision would be a
/// frame that redrew correctly and re-drew again, and there is no reader that
/// takes the revision alone and does something with it.
struct Current {
    lang: AtomicU8,
    revision: AtomicU64,
}

impl Current {
    const fn new() -> Self {
        Self {
            lang: AtomicU8::new(Lang::English as u8),
            revision: AtomicU64::new(0),
        }
    }

    fn get(&self) -> Lang {
        Lang::from_index(self.lang.load(Ordering::Relaxed))
    }

    fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }

    /// Put a language in force, and say whether that was a change.
    ///
    /// The revision advances only on a real change, which is what lets
    /// `Runtime::adopt_new_language` treat the answer as "is there anything to
    /// repaint": a settings row re-pressed on the value it already wears must
    /// not cost a re-layout of every typeset band in the window.
    fn set(&self, lang: Lang) -> bool {
        if self.get() == lang {
            return false;
        }
        self.lang.store(lang as u8, Ordering::Relaxed);
        self.revision.fetch_add(1, Ordering::Relaxed);
        true
    }
}

/// The one this process draws in.
static CURRENT: Current = Current::new();

/// Name the language this window draws in, and say whether that moved it.
///
/// **Called at startup and again on every press of the Language row.** It used
/// to be a one-shot — see this module's header for what had to exist before it
/// could stop being one — and the caller's obligation is now the obligation the
/// theme's switch already had: a `true` answer means a repaint is owed, and
/// `Runtime::adopt_new_language` is where that obligation is discharged.
pub fn install(lang: Lang) -> bool {
    CURRENT.set(lang)
}

/// The language every [`Text`] is answered in.
///
/// English before [`install`] has run: English is the source language rather
/// than a fallback, so a word read before the settings file was opened is the
/// word this product was written with, not a placeholder.
#[must_use]
pub fn current() -> Lang {
    CURRENT.get()
}

/// How many times the window's language has changed.
///
/// The number `bt_doc::LayoutKey::lang_rev` carries, and the whole reason a hot
/// switch is possible: it is to the language what `bt_render::theme_revision` is
/// to the palette. Starts at zero and advances only on a real change, so a
/// window whose user never opens the Language row pays nothing for it.
#[must_use]
pub fn lang_revision() -> u64 {
    CURRENT.revision()
}

/// One arm of the table: the English the product was written in, and the Chinese
/// it is read as.
const fn pick(lang: Lang, english: &'static str, chinese: &'static str) -> &'static str {
    match lang {
        Lang::English => english,
        Lang::Chinese => chinese,
    }
}

/// Every fixed string this window puts on screen.
///
/// The variant names say **where the string is**, not what it says, because what
/// it says is the thing that changes: `PreviewRefusalBinary` still names the same
/// line after somebody rewrites the sentence, where `NoPreviewBinaryFile` would
/// have to be renamed with it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Text {
    // ── window chrome ──────────────────────────────────────────────────────
    /// The gear's tip **and** the dialog's own `h1` — one string, because they
    /// are one word for one thing and two literals is how two surfaces come to
    /// disagree.
    Settings,
    ToggleSidebar,
    Minimize,
    Maximize,
    CloseWindow,

    // ── tab strip and vertical rail ────────────────────────────────────────
    /// The rail's heading. **Length-sensitive**: it is drawn only while the rail
    /// is wide enough to hold it, and dropped silently otherwise.
    RailTabs,
    /// The rail's `+` row. Same rule as [`Self::RailTabs`].
    RailNewTab,
    ChooseProfile,
    Pin,
    Unpin,

    // ── a tab's tip ────────────────────────────────────────────────────────
    NameSourceManual,
    NameSourceProgram,
    NameSourceCwd,
    /// The third line, leading newline included — it is a fact about the tab's
    /// future and earns its own line.
    TabTipPinned,

    // ── the mark slot ──────────────────────────────────────────────────────
    /// No ellipsis: this is a state, not a running commentary.
    MarkWorking,
    /// **A different string** from [`Self::MarkWorking`]: the indeterminate arc
    /// has no length to mean anything, so its tip trails off where the state's
    /// does not.
    MarkWorkingIndeterminate,

    // ── pane heads ─────────────────────────────────────────────────────────
    SeatTerminal,
    SeatFiles,
    SeatPreview,
    SeatUnavailable,
    PlaceholderSeatNotice,

    // ── the settings dialog ────────────────────────────────────────────────
    CategoryGeneral,
    CategoryAppearance,
    CategoryTerminal,
    CategoryRenderedBlocks,
    CategoryShortcuts,
    /// The rail's word — a second string and not the heading lower-cased,
    /// because Chinese has no case to raise and a `to_uppercase` rule would have
    /// to be unlearned rather than read.
    NavGeneral,
    NavAppearance,
    NavTerminal,
    NavRenderedBlocks,
    NavShortcuts,

    RowTheme,
    RowCursor,
    RowFormulas,
    RowInlineFormulas,
    RowGitPanel,
    /// **The Explorer verb's switch** (§7.4), on the General page.
    ///
    /// Names the surface it changes — Explorer's menu — rather than the thing it
    /// writes, which is a registry key nobody asked to hear about.
    RowContextMenu,
    RowTabLayout,
    RowSidebar,
    RowSplitDirection,
    RowDefaultProfile,
    RowLanguage,
    /// The grid's face. **Not the window's** — the description says so, because
    /// "font" in a terminal's settings is ambiguous and the ambiguity is the
    /// whole complaint a reader would have.
    RowTerminalFont,
    RowFontSize,
    /// The Terminal page's first row, and therefore the row that puts that page
    /// in the rail at all.
    RowPsReadLine,

    DescTheme,
    DescCursor,
    DescFormulas,
    DescInlineFormulas,
    DescGitPanel,
    /// Where the entry will be found, which on Windows 11 is the surprising
    /// half — see [`Self::ContextMenuVerb`].
    DescContextMenu,
    DescTabLayout,
    DescSidebar,
    DescSplitDirection,
    DescDefaultProfile,
    DescLanguage,
    DescTerminalFont,
    DescFontSize,

    /// Shared by Theme and Language, which is the point: it is one word meaning
    /// one thing — "ask Windows" — in both rows.
    OptionSystem,
    OptionLight,
    OptionDark,
    OptionCursorBar,
    OptionCursorBlock,
    OptionCursorUnderline,
    OptionHorizontal,
    OptionVertical,
    OptionOn,
    OptionOff,
    OptionExpanded,
    /// The mode's name, not a sentence about it (user ruling 2026-08-10) — the
    /// translation must not put the explanation back.
    OptionIcons,
    OptionSplitAuto,
    OptionSplitRight,
    OptionSplitDown,

    // A card telling the reader to relaunch stood here until §7.1.6c-3c. It was
    // raised because the Language row was the one row whose tick moved while
    // nothing else in the window did. The window changes language under the
    // press now, so the card has nothing to report — and a card announcing a
    // change the reader is looking at would be the window telling them what they
    // can see. Removed rather than left unused: an unused arm in this table is a
    // sentence somebody will one day find a place for.
    // `no_string_in_this_table_asks_for_a_relaunch` is the pin, and it reads this
    // file, so the words it forbids are not spelled out here.

    // ── the PSReadLine row and its invitation ──────────────────────
    /// The row's line while the out-of-band probe has not answered yet. It is a
    /// real state and not a placeholder: the probe starts a process, and on a
    /// cold machine that takes long enough to see.
    PsReadLineProbing,
    /// `settings.json` says Folio installed the module and the directory is not
    /// there. Something outside Folio removed it, which is a fact the row owes
    /// the reader rather than a state to silently correct.
    PsReadLineRowGone,
    /// The invitation's own question. It names the symptom the reader has
    /// already seen — the input line that does not follow the window — rather
    /// than the module, because the module is not what they noticed.
    PsReadLineInviteTitle,
    PsReadLineInstall,
    /// The refusal, worded so it is plainly not the last chance: the Terminal
    /// page keeps the row.
    PsReadLineNotNow,
    PsReadLineRemovedToast,

    // ── Explorer's context menu (§7.4) ─────────────────────────────────────
    /// **The words in Explorer's own menu**, and the one string in this table
    /// that is written into the registry rather than drawn by this window.
    ///
    /// It follows the language like every other string here, and the launch's
    /// re-assertion is what carries a change into the menu — see
    /// `context_menu::desired`. `MUIVerb`, which would let Explorer resolve the
    /// words per user, needs a string resource in the binary and is recorded in
    /// `docs/DESIGN.md` §7.4 as the route the bilingual plan will take.
    ContextMenuVerb,
    /// The card raised when the entry lands.
    ContextMenuAddedToast,
    /// And when it is taken back out.
    ContextMenuRemovedToast,
    /// The refusal when Windows will not say where this process's own binary is.
    ///
    /// There is nothing to write into a `command` value without it, and no guess
    /// worth making: a path invented here would be a menu entry that launches
    /// whatever happens to be at it.
    ContextMenuNoExecutable,

    // ── the `˅` profile menu ───────────────────────────────────────────────
    /// Lower case is deliberate: it is an annotation, not a label.
    ProfileHintDefault,
    /// **Length-sensitive**: the menu is `min-width: 180px` and this grey shares
    /// a line with the profile's title.
    ProfileHintUnavailable,
    ProfileHintCurrent,
    ProfileFilesPane,
    ProfileFilesPaneHint,
    ProfileRecentSection,

    // ── the root menu ──────────────────────────────────────────────────────
    RootSection,
    /// The ellipsis is a real character and it carries meaning ("you will be
    /// asked again"), so it survives translation.
    RootBrowse,
    RootNoteHome,
    /// **Length-sensitive**: `min-width: 190px`, shared with a folder name.
    RootNoteTerminal,
    RootNoteParent,

    // ── what the user said to keep (`pins.json`) ───────────────────────────
    /// The heading over the pinned rows, in **both** menus that have them: the
    /// root menu's folders and the preview switcher's files. One string, because
    /// it is one idea and a reader who learns it in one menu has learnt it in
    /// the other.
    PinnedSection,

    // ── a file row's menu ──────────────────────────────────────────────────
    FileMenuOpenPreview,
    /// **Hand it to the machine's own default program** (user ruling
    /// 2026-08-24) — the breadcrumb row's `Open ⌄` names the door rather than
    /// the destination, so this row is the one that says which door.
    FileMenuOpenDefaultApp,
    // `FileMenuShowInFiles` is **retired** (user ruling 2026-08-25) with the row
    // it captioned: every segment of the breadcrumb above the menu now stands
    // the column on that level without leaving the tree, so a row saying the
    // same thing was the surface repeating itself.
    FileMenuCopyPath,
    /// The widest row, and therefore the one that decides the menu's width.
    FileMenuInsertPath,
    /// The `Open ⌄` pill's caption on a preview's breadcrumb row.
    ///
    /// One word, because the pill is a *door* and the menu behind it is where
    /// the destinations are named — a caption that said "Open with…" would be a
    /// button restating the list it opens.
    PreviewRailOpen,
    /// That pill's **tip** (user ruling 2026-08-25 — 「新控件全部挂 tooltip」).
    ///
    /// [`Self::PaneChevronTip`]'s shape and its reason: a chevron says only
    /// "there is a list here" and never what is on it, so the tip names the
    /// first thing on the list and says there is more. The pill's own caption is
    /// one word, so the tip is not a repetition of it.
    PreviewRailOpenTip,
    /// `⧉` on a **page's** address row — the tip on the button that copies the
    /// address (user ruling 2026-08-25).
    ///
    /// Its own entry beside [`Self::FileMenuCopyPath`], which is the tip the
    /// same glyph carries on a breadcrumb: one verb, two kinds of address, and
    /// the tip is where the difference is said. A single string for both would
    /// call a URL a path.
    PreviewCopyAddress,
    /// `</>` while the pane is showing a rendered document — press it for the
    /// source (user ruling 2026-08-25).
    PreviewFlipToSource,
    /// `</>` while the pane is showing the source — press it for the rendering.
    ///
    /// Two entries and not one, on [`Self::PreviewWebReload`]'s precedent: the
    /// button changes, so a single word for both would be the row describing
    /// what it was a press ago.
    PreviewFlipToRendered,

    // ── the files tree ─────────────────────────────────────────────────────
    FilesLoading,
    FilesEmpty,
    FilesUnrooted,
    FilesPermissionDenied,
    FilesNotFound,
    FilesUnreadable,
    /// **Length-sensitive**: the foot clips this from the left with
    /// `ellipsized_left`, so the tail is the half that survives.
    FilesRevealed,

    // ── the preview pane ───────────────────────────────────────────────────
    PreviewEmptyState,
    PreviewRefusalType,
    PreviewRefusalBinary,
    /// The longest of the six, and the one that wraps inside its card.
    PreviewRefusalNetworkPath,
    PreviewRefusalPermissionDenied,
    PreviewRefusalNotFound,
    PreviewRefusalUnreadable,
    /// **Shares a button box with [`Self::PreviewOpened`]** — two translations
    /// of very different widths would make the button jump.
    PreviewOpenExternally,
    PreviewOpened,
    PreviewTruncated,
    PreviewSaved,
    PreviewConflict,
    /// The clause that gets filled into [`not_saved`].
    PreviewNothingToSave,
    PreviewFailedImageLoad,
    /// One string for the three call sites that used to spell it out separately.
    PreviewFailedImageWorker,
    PreviewFailedSeatTooSmall,

    // ── the float and the folder trigger ───────────────────────────────────
    /// Upper case is presentation, not the word — but the constant has always
    /// carried the shouted form, and the Chinese has no case to shout in.
    FloatDock,
    /// One sentence, two surfaces (H108): the tab's trigger and the pane head's.
    FloatTriggerTip,

    // ── the restore prompt ─────────────────────────────────────────────────
    RestoreTitle,
    /// The paragraph that wraps — see `restore::wrap`, which had to learn how
    /// Chinese breaks before this line could be shown in it.
    RestoreSub,
    RestoreDecline,
    RestoreAccept,

    // ── the terminal's own status overlay ──────────────────────────────────
    /// **36 characters, measured**: the overlay is right-aligned into the pane
    /// and keeps the *tail*, so anything longer loses the half that says what
    /// happened.
    FilesProgramRefused,
    /// These three are deliberately one sentence pattern — a worker dying is a
    /// feature going away, and all three have to say which half still works.
    MathWorkerStopped,
    FilesWorkerStopped,
    PreviewWorkerStopped,

    // ── the hyperlink overlay ──────────────────────────────────────────────
    /// The suffix, separator included. Its **length** is arithmetic: it is
    /// subtracted from the column budget before the URI is trimmed.
    HyperlinkBlockedSuffix,
    /// The same word alone, for a pane too narrow for the URI as well. The short
    /// form must stay a substring of the long one.
    HyperlinkBlocked,

    // ── a drag's landing caption ───────────────────────────────────────────
    DragSwapPanes,
    DragReplacePane,

    // ── the two colour-scheme rows (§7.1.6c-4a) ────────────────────────────
    /// Appended here rather than beside the other `Row*` entries: this table is
    /// a list whose order is its own, and a slice that inserted into the middle
    /// of it would show as a diff across every line below the insertion.
    RowLightScheme,
    RowDarkScheme,
    DescLightScheme,
    DescDarkScheme,
    /// The title of the card raised for a scheme file that would not parse. Its
    /// body names the file and the reason and is therefore a value-carrying
    /// string — see [`scheme_file_skipped`].
    SchemeFileSkipped,

    // ── the window's ground and the window's posture (§7.1.6c-4b) ──────────
    /// Appended at the end for the reason the scheme rows were, which is now a
    /// standing rule for this table: a slice that inserted into the middle
    /// would show as a diff across every line below it.
    RowBackgroundImage,
    RowImageFit,
    RowImageOpacity,
    RowBackgroundOpacity,
    RowAcrylic,
    RowAlwaysOnTop,
    DescBackgroundImage,
    DescImageFit,
    DescImageOpacity,
    DescBackgroundOpacity,
    DescAcrylic,
    DescAlwaysOnTop,
    /// The sentence a greyed Acrylic row carries **instead of** its ordinary
    /// description. The reason lives in the description line rather than in a
    /// slot of its own, which is `psreadline::row_description`'s ruling: there
    /// is one muted line under a title, and a row that is off has exactly one
    /// thing worth saying there.
    DescAcrylicUnavailable,
    /// The same for a greyed Background opacity row.
    DescBackgroundOpacityUnavailable,
    OptionImageNone,
    OptionImageChoose,
    OptionFitStretch,
    OptionFitFill,
    OptionFitTile,

    // ── the files column's own switch (§7.1.6c-5) ──────────────────────────
    /// The `Files | Git` segmented control above a files column's body — the
    /// last two bare literals in the chrome, printed straight into
    /// `seats::push_files_seg` since the Git page was born.
    ///
    /// Two entries and not a reuse of [`Self::SeatFiles`], although the English
    /// word is the same one: that string names a *kind of pane* in a head 13.5px
    /// high, this one names a *page* in an 11.5px segmented control whose two
    /// halves are measured against each other every frame. One literal serving
    /// both would make shortening either of them a change to the other.
    FilesViewFiles,
    /// The second half, and the one entry in this table whose two columns are
    /// the same string — see `UNTRANSLATED`.
    FilesViewGit,

    // ── the Advanced disclosure and its Reset (§7.1.6c-5) ──────────────────
    /// The heading of the collapsed group at the foot of a page. Progressive
    /// disclosure **per page** (user ruling 2026-08-17), so the word is a page's
    /// own and not a switch the whole dialog shares.
    ///
    /// **Upper-cased at the source** (user ruling 2026-08-19), for
    /// [`Text::CategoryAppearance`]'s reason and now for its own: the header
    /// wears `.group-label`'s type, the chrome text path has no
    /// `text-transform`, and "大写是内容不是样式" — a `to_uppercase`
    /// here would be a rule somebody has to remember does not apply to 高级.
    AdvancedGroup,
    /// The verb that ends every Advanced group: it puts that page's advanced
    /// rows back, and nothing else on the page.
    ResetAdvanced,

    // ── customising a colour scheme (§7.1.6c-4c) ─────────────────────────
    // Moved onto the picker's own rows by the 2026-08-19 ruling.
    /// **The verb both scheme pickers end with**, behind a hairline: it copies
    /// the scheme in force into the reader's own folder and opens the copy.
    ///
    /// It is `Customise scheme…` in everything but where it is written, and the
    /// words got shorter because the place answered half of them. The scheme in
    /// force is the item ticked two rows above the verb, so the verb no longer
    /// has to say which scheme it will copy — the reader can see it. The
    /// ellipsis stays: pressing it does not finish the job, it hands you a file.
    AddScheme,
    /// **The verb the font picker ends with**, on the same terms and behind the
    /// same hairline: it opens the system's own Fonts page, where a font is
    /// installed by dropping a file on it.
    ///
    /// The list is every monospaced family DirectWrite reports, which makes the
    /// honest answer to "my font isn't here" *install it* — and until this row
    /// existed the picker ended without saying so, leaving a reader who had
    /// downloaded a Nerd Font ten minutes earlier no way to learn that the
    /// machine, not this product, was the thing that had not heard of it yet.
    InstallFonts,
    /// The title of the card raised when the scheme **in force** stops parsing.
    ///
    /// Separate from [`Self::SchemeFileSkipped`], which is the startup card
    /// about a file in the folder: that one says "this scheme is not in your
    /// list", and this one has to say "the file you are editing is broken and
    /// the window kept the colours it had". Two different pieces of news about
    /// one kind of file.
    SchemeInUseBroken,
    /// And the title of the card raised when it stops existing.
    SchemeInUseGone,

    // ── deleting a colour scheme (§7.1.6c-4d, user ruling 2026-08-18) ──────
    //
    // **Three strings left this table on 2026-08-19** — `Delete scheme…` and the
    // two halves of the line above it — and none of them was replaced. The verb
    // is a `×` on the row it acts on now, which needs no word; the line said
    // which scheme the button would act on, and a mark on a row says that by
    // standing on it; and the refusal said a bundled scheme has no file, which a
    // bundled row now says by revealing no marks at all. A sentence deleted
    // because the picture already says it is the best kind of translation work
    // there is.
    /// The title of the card raised when a scheme file has been recycled.
    SchemeDeleted,

    // ── the window's ground picture, refused (§7.1.6c-4d) ──────────────────
    /// The title of the card raised when the chosen picture is not on screen.
    ///
    /// It says the *effect* and leaves the reason to the body, because the two
    /// halves answer different questions: a reader who chose a wallpaper and got
    /// no wallpaper is first asking whether the window heard them.
    BackgroundPictureRefused,

    // ── the Profiles page (§7.1.6c-6) ──────────────────────
    //
    // **The profiles' own names are not here and never will be.** `PowerShell 7`,
    // `WSL`, `Git Bash` and `Command Prompt` are product names, a Chinese window
    // spells them the way an English one does (§G S103), and every one of them is
    // byte-compared against what a shell announces about itself — which is what
    // `no_profile_title_has_been_pulled_into_the_language_table` pins.
    /// The rail's second word and the page's heading, which are two entries for
    /// [`Self::NavGeneral`]'s reason: this file upper-cases at the source.
    NavProfiles,
    CategoryProfiles,
    /// The one verb this slice ships live on a row.
    ///
    /// The arrows carry no words: they are glyphs, and the sentence a *dark*
    /// arrow owes ("already first") needs a tooltip, which is a channel this
    /// dialog has never had. It arrives with the row menu in the next slice,
    /// where the refusals that have reasons have to live anyway.
    ProfilesDuplicate,
    /// The badges. They report and are not controls, which is why they wear the
    /// group label's type and the ink an unavailable thing wears.
    ProfilesBadgeDefault,
    ProfilesBadgeHidden,

    // ── the honest capability sentences (J85) ──────────────────
    //
    // The authority is `docs/shell-integration.md`'s "What each profile actually
    // gets"; this page does not build a second matrix, it draws one row of that
    // one. Each names the markers it HAS and the ones it has NOT — no row claims
    // a capability by omission — and each is written to about fifty-eight
    // characters, which is what the text column holds once the action run has
    // taken its fixed reserve on the right.
    /// A bash the terminal hands its own init file to: everything, unconditional.
    CapFull,
    /// PowerShell, both editions. **The condition is in the sentence** because
    /// this page cannot probe for it: whether `folio.ps1` is dot-sourced into
    /// the user's profile script is known only to a live session that has
    /// already sent OSC 133, and a settings page has never had that. Said this
    /// way the sentence is true in every state and needs no probe.
    CapPowerShell,
    /// WSL. The same everything, and the same honest condition — a `zsh` or
    /// `fish` login never reads the init file the launcher was handed.
    CapWslBash,
    /// `cmd`, whose whole integration is the one marker that fits in `PROMPT`.
    /// It is the only shipped row that has to say what it has not got.
    CapCmd,
    /// A profile served by no script at all.
    CapNone,

    // ── the Rendered blocks page's third row (2026-08-18) ──────────────────
    //
    // Appended here rather than beside `RowInlineFormulas`, which is where this
    // file's own grouping discipline would put it, and the reason is the day
    // rather than the subject: three lanes were adding rows to this table at
    // once, and a new entry in the middle of the settings family is a conflict
    // in every one of the three. The order of this enum carries no meaning —
    // `visible_rows` decides what the dialog shows and in what order — so the
    // cost of appending is one paragraph of explanation and the cost of slotting
    // was three merges.
    RowTables,
    DescTables,

    // ── the profile editor (§7.1.6c-6b) ────────────────────
    //
    // The row's remaining verbs, the sub-page's fields and the two refusals a
    // menu item can carry. Every sentence states a fact about the profile or the
    // machine and none of them says "we" — `ui-copy-no-editorial`'s ruling, met
    // on the page with the most sentences in the dialog.
    /// The one verb in the open on a row, which is what a person came to this
    /// list to do. The other four are behind the `⋯`.
    ProfilesEdit,
    ProfilesHide,
    ProfilesShow,
    ProfilesDelete,
    /// The menu item ruled onto the row (user ruling 2026-08-17, Q4): the
    /// picker itself stays on the General page, so the default still has one
    /// place it is *chosen*, and this is the same choice made from the table it
    /// is a fact about.
    ProfilesSetDefault,
    /// Why `Set as default` is dark on the row that already is one — named as a
    /// state rather than as a rule, which is the register every refusal in this
    /// menu wears.
    ///
    /// **The two arrows' own sentences are not here** ("already first", "already
    /// last", "move up", "move down"). This dialog has never drawn a tooltip and
    /// still has not got one: the refusals that arrived with this slice are menu
    /// *items*, which have a line to carry theirs. A string with nothing to draw
    /// it would be a translation nobody can ever read, so those four ship with
    /// the channel and not before it.
    ProfilesAlreadyDefault,
    /// The foot of the list.
    ProfilesNew,
    /// The everyday half of the editor, in the order somebody decides a profile:
    /// what it is called, what it runs, where it starts, what colour names it.
    ProfilesRowName,
    ProfilesRowNameDesc,
    ProfilesRowProgram,
    ProfilesRowProgramDesc,
    ProfilesRowStartingDir,
    ProfilesRowStartingDirDesc,
    ProfilesRowColour,
    ProfilesRowColourDesc,
    /// The three answers the starting-directory picker carries.
    ProfilesInherit,
    ProfilesHome,
    ProfilesChooseFolder,
    /// The eight struck colours, named. A swatch alone cannot be spoken, read
    /// aloud or searched, and these are not product names — unlike a profile's
    /// own title, which is why those stay out of this table and these come in.
    ProfilesColourBlue,
    ProfilesColourTeal,
    ProfilesColourGreen,
    ProfilesColourAmber,
    ProfilesColourRed,
    ProfilesColourMagenta,
    ProfilesColourViolet,
    ProfilesColourSlate,
    /// What the Colour picker's button says on a profile duplicated from a
    /// built-in: it wears that built-in's mark, which is none of the eight and
    /// is telling the truth about what it is a copy of. Nothing in the list is
    /// ticked while this is the value.
    ProfilesColourInherited,
    /// And what it says on one of the five this build ships: the mark came with
    /// the product and is not one of the eight — the row is greyed whole and its
    /// sentence carries the reason.
    ProfilesColourFixed,
    /// The Advanced group's rows.
    ProfilesRowArgs,
    ProfilesRowArgsDesc,
    ProfilesRowEnv,
    /// **The plan's own sentence, earned** (§7.1.6c-6c). It said `Stored for
    /// this profile. Nothing reads it into a session yet` for exactly as long as
    /// that was true, which was one slice; the spawn path reads it now, and the
    /// three words that matter are `over what Folio sets` — the layering rule
    /// (`profiles::Profile::env`) said in the place a reader is standing.
    ProfilesRowEnvDesc,
    ProfilesRowHyperlink,
    ProfilesRowHyperlinkDesc,
    ProfilesRowIntegration,
    ProfilesEnvAdd,
    ProfilesEnvName,
    ProfilesEnvValue,
    /// `Auto`, `On`, `Off` — the hyperlink answer's three, where `Auto` is the
    /// terminal's own declaration and the other two are this profile overruling
    /// it.
    ProfilesAuto,
    ProfilesOn,
    ProfilesOff,
    /// The two verbs a foot can carry, and only ever one of them: a built-in
    /// drops its overrides, a profile of the reader's own is deleted.
    ProfilesRestoreAll,
    ProfilesDeleteBtn,
    ProfilesBrowse,
    /// Why a `Hide` is dark. Both are guards and not politeness: hiding the
    /// default leaves no new tab to open, and hiding the floor puts a hole in the
    /// bottom of every degradation chain in the product.
    ProfilesCannotHideDefault,
    ProfilesCannotHideFallback,
    /// Why the name field is refusing. Two sentences and not one, because they
    /// are two different things to do about it.
    ProfilesNameBlank,
    ProfilesNameTaken,
    /// The verb on the deletion card — the way back this product already struck
    /// once as `Ctrl+Shift+T`, which is what a deletion with no confirmation is
    /// owed.
    ProfilesUndo,

    // ── the environment, the hyperlink answer and the door (§7.1.6c-6c) ────
    //
    // Appended at the end of the table for the reason the `Tables` pair one
    // family up gives: three lanes were adding rows at once and a new entry in
    // the middle of the settings family is a conflict in every one of them. The
    // order of this enum carries no meaning — `visible_rows` and each row's own
    // `option_label` decide what is shown and where.
    /// The four doors, as the `Shell integration` picker names them. `Auto` is
    /// [`Self::ProfilesAuto`] with the derived door in parentheses, composed by
    /// [`profile_integration_auto`], which is the mock-up's own `Auto (None)`.
    ///
    /// Named after the **mechanism** rather than after the shell, because that
    /// is what the row chooses: a `zsh` under WSL and a Git Bash are both served
    /// by the init file, and a picker that said `Git Bash` on a row starting
    /// `zsh` would be naming the wrong thing.
    ProfilesIntegrationPowerShell,
    ProfilesIntegrationBash,
    ProfilesIntegrationCmd,
    ProfilesIntegrationNone,
    /// The four capability sentences with their links struck out — a profile
    /// whose own `FORCE_HYPERLINK` row says `0`, or whose `TERM_PROGRAM`
    /// override stops `folio.ps1` recognising the session it is in.
    ///
    /// Twins rather than a clause bolted on, because each sentence already lists
    /// what it has and what it has not, and a sentence that named hyperlinks in
    /// both halves would be reading as a correction of itself.
    CapFullNoLinks,
    CapPowerShellNoLinks,
    CapWslBashNoLinks,
    CapCmdNoLinks,
    /// `No shell integration` said at the length the editor's own row has room
    /// for. The list's third line shares its row with an action run and holds
    /// about fifty-eight characters; the editor's `Shell integration` row has the
    /// page's whole width, and a reader standing in front of that picker is owed
    /// what the answer costs rather than the category it falls in.
    CapNoneLong,
    CapNoneLongNoLinks,

    // ── the Rendered blocks page's fourth row (2026-08-18) ─────────────────
    //
    // Appended for `RowTables`' own reason, which has not changed: the order of
    // this enum carries no meaning — `visible_rows` decides what the dialog
    // shows and in what order — and a new entry in the middle of the settings
    // family is a conflict in every lane that is adding one.
    RowBlockMaxHeight,
    DescBlockMaxHeight,
    /// **The PSReadLine row's fourth value** (2026-08-18): an older Folio build's
    /// module is installed and this one carries a newer patch. It stands where a
    /// picker draws its ticked item, because that is the one place on the row a
    /// reader is already looking for its state — and the two items behind it are
    /// still `On` and `Off`, which is what the ruling asked for.
    PsReadLineUpdate,
    /// The uncapped end of the `Maximum height` picker: the one option that is a
    /// word rather than a quantity, which is why it is in this table and the
    /// three numbers beside it are not.
    OptionBlockHeightNone,

    // ── the tail: every surface born after §7.1.6c-3a ──────────────────────
    //
    // One contiguous block at the end, for the reason the four families above
    // it give and which is now this table's standing rule: the order of this
    // enum carries no meaning, and an entry slotted into the middle of a family
    // is a conflict in every lane that is adding one.
    //
    // What arrives here is the sweep the string inventory promised: the
    // shortcut table's own names, the Git panel and the graph, the command
    // rail's glance card, the search capsule, the terminal's right-click menu
    // and the pane's `⌄`, the confirmation gate's four newer questions, and the
    // handful of sentences that escaped the earlier slices. The rules are the
    // ones the header states — `&'static str` throughout, technical tokens
    // (`git`, `HEAD`, `PiP`, key caps, `folio.ps1`) untranslated because they
    // are names, and no sentence in which this window is a party to the
    // conversation.

    // ── the Shortcuts page (`crate::shortcuts::BINDINGS`) ──────────────────
    //
    // **The row's name is a `Text` and not a `&'static str`, and the id beside
    // it is still a `&'static str`.** That split is the whole of what makes the
    // page translatable without making `keybindings.json` a translated file: a
    // user's file names rows by `"new-tab"`, which is a byte string this table
    // must never touch, while the words over them are read by a person.
    //
    // `New tab` and `Settings` are not here. They are [`Self::RailNewTab`] and
    // [`Self::Settings`], because they are the same two verbs the rail's `+` and
    // the gear already name — the second of those is the precedent this file set
    // in its first slice, and one verb with two spellings is how two surfaces
    // come to disagree.
    /// **Nine entries and not one with a number in it**, which is the shape the
    /// signature forces rather than a preference: a title is a `&'static str`,
    /// and `转到标签 5` cannot be built at compile time out of a `5`. They are
    /// nine rows in `keybindings.json`, nine chords in the audit and nine names
    /// here; the editor folds them into [`Self::ShortcutFamilyGotoTab`] and only
    /// a chord conflict ever names one of them alone.
    ShortcutGotoTab1,
    ShortcutGotoTab2,
    ShortcutGotoTab3,
    ShortcutGotoTab4,
    ShortcutGotoTab5,
    ShortcutGotoTab6,
    ShortcutGotoTab7,
    ShortcutGotoTab8,
    ShortcutGotoTab9,
    ShortcutNextTab,
    ShortcutPrevTab,
    ShortcutReopenClosed,
    ShortcutJumpAttention,
    ShortcutCommandPalette,
    ShortcutSplitHorizontal,
    ShortcutSplitVertical,
    ShortcutDuplicatePaneSplit,
    ShortcutFilesPane,
    ShortcutGitPage,
    ShortcutSavePreview,
    ShortcutPrevCommandMark,
    ShortcutNextCommandMark,
    ShortcutOpenSearch,
    /// The walk's two rows. **Not the search capsule's two tooltips**
    /// ([`Self::SearchTipNext`]): those carry their key caps in parentheses
    /// because a tooltip is the only place that chord is written down, and this
    /// page draws the caps in a column of their own.
    ShortcutNextMatch,
    ShortcutPrevMatch,
    /// `PiP` stays `PiP` — it is the feature's name, and the four slots are
    /// numbered for the same reason the nine tabs are.
    ShortcutSummonPip1,
    ShortcutSummonPip2,
    ShortcutSummonPip3,
    ShortcutSummonPip4,
    /// The two folded rows' own names. The en dash is a range and survives
    /// translation the way [`Self::RootBrowse`]'s ellipsis does.
    ShortcutFamilyGotoTab,
    ShortcutFamilySummonPip,
    /// Where a scoped row is in force — the first clause of the muted line under
    /// its name.
    ShortcutScopePreview,
    ShortcutScopeTerminalPrimary,
    ShortcutScopeSearchOpen,
    /// And the second clause: the key is claimed, the verb behind it has not
    /// arrived (§7.1.5e).
    ShortcutNotePending,
    /// What a folded row says about the chords it folded. **Three entries and
    /// not one with a clause bolted on**, because the join is a `; ` in English
    /// and a `；` in Chinese, and a formatter that composed them would have had
    /// to carry a punctuation table nobody asked for.
    ShortcutNoteOnePerMember,
    ShortcutNoteNoneAssigned,
    ShortcutNoteSomeUnassigned,
    /// What stands where the caps would be on a row with no chord.
    ShortcutUnbound,
    /// The two rows the audit listed and declined, and the line under them.
    ShortcutReservedMoveFocus,
    ShortcutReservedResizePane,
    ShortcutReservedAltArrow,
    /// The recorder's two standing refusals. The chords in them are key caps and
    /// stay as they are; what is translated is the reason.
    ShortcutHintAltGrZone,
    ShortcutHintShellControlLetter,

    // ── the Git page (`crate::git_panel`) ──────────────────────────────────
    //
    // `git`, `HEAD`, `git commit` and `git.exe` are the program's own words and
    // are spelled the same in both columns — the header's "technical tokens"
    // rule, applied to the surface that has the most of them.
    GitNotARepository,
    GitReading,
    /// Who is speaking on a card raised by a refused write. **The one entry
    /// here whose two columns are the same string** — see `UNTRANSLATED`.
    GitToastTitle,
    /// Lower case: it is a clause fitted after a branch name, not a sentence.
    GitUnborn,
    GitDetached,
    GitLoadMore,
    GitCommitNoFiles,
    /// The three status groups. **Upper case at the source** (N4): this pipeline
    /// has no `text-transform`, and the Chinese has no case to raise.
    GitGroupStaged,
    GitGroupChanges,
    GitGroupUntracked,
    /// Their teaching sentences. They are the only place this page explains what
    /// the index is, so the Chinese teaches too rather than naming.
    ///
    /// **Read by the rows and no longer by the heading** (user report,
    /// 2026-08-25) — see `crate::git_panel::group_tooltip`. The heading's own
    /// two sentences, `GitBranchesTip` and `GitCommitsTip`, went altogether:
    /// what they promised is on the rows that keep the promise.
    GitGroupStagedTip,
    GitGroupChangesTip,
    GitGroupUntrackedTip,
    GitBranchesHeading,
    GitRemotesHeading,
    GitRemotesTipShut,
    GitRemotesTipOpen,
    GitCommitsHeading,
    /// One verb on one repository, seen from the panel's masthead and the
    /// graph's toolbar — one string, for [`Self::Settings`]'s reason.
    GitRefreshTip,
    /// The row verbs' tooltips. `Stage` and `Unstage` are also what the row menu
    /// writes, and they are one entry each: the menu row and the hover button
    /// are the same act on the same file.
    GitActStage,
    GitActUnstage,
    /// The two discards, which are one word to the reader and two commands to
    /// git — the tooltip is where that difference is said.
    GitActDeleteFile,
    GitActDiscardChanges,
    GitActStageAll,
    GitActUnstageAll,
    GitActLoadMore,
    GitActOpenGraph,
    /// A history with nothing in it. A capital where [`Self::GitUnborn`] has
    /// none, because this one stands alone on a row.
    GitNoCommits,
    GitFaultTimedOut,
    /// The fifth of the worker sentences, in the shape the other four wear: a
    /// worker dying is a feature going away, and the line has to say which half
    /// still works.
    GitWorkerStopped,
    /// It says what to do about it and not only what is wrong (user ruling
    /// 2026-08-16). `Git for Windows` is a product's name and stays.
    GitNotFound,

    // ── the commit graph (`crate::git_graph`) ──────────────────────────────
    /// The meta line's two leads. They are prefixes to a value, so the Chinese
    /// carries the full-width colon the value hangs off.
    GraphMetaParents,
    GraphMetaCommittedBy,
    /// The newer end of a comparison when it is not a commit.
    GraphCompareWorkingTree,
    /// The five column heads, in `.glabel` grammar and upper-cased at the source
    /// for [`Self::GitGroupStaged`]'s reason. **Length-sensitive**: `GRAPH` is
    /// dropped below 44 logical pixels of column, and the four beside it share a
    /// row with the values under them.
    GraphHeadingGraph,
    GraphHeadingDescription,
    GraphHeadingAuthor,
    GraphHeadingDate,
    GraphHeadingCommit,
    /// The working tree's own row, which is not a commit.
    GraphUncommitted,
    /// What stands in its date column.
    GraphUncommittedTime,
    GraphSearchPlaceholder,
    GraphSearchNone,
    /// The filter's resting word, and the menu's first row.
    GraphFilterAll,
    GraphToolFilterTip,
    GraphToolSearchTip,
    GraphToolSearchClearTip,
    /// A file git counted no lines in.
    GraphFileBinary,
    /// **One sentence, two surfaces**: the panel's commit row and the graph's.
    /// They were two literals differing only in a dash until this slice, which
    /// is exactly the drift the table exists to end.
    GraphMergeCommit,
    /// The graph's own last line on a commit row — the panel's rows have no
    /// double-click and do not say this.
    GraphDoubleClickCheckout,
    /// And the uncommitted row's, which unfolds rather than checks out.
    GraphClickToList,

    // ── the command rail's glance card (`crate::cmdrail`) ──────────────────
    /// What the card says about a mark whose text the ledger never got, and
    /// about a hit whose line the transcript has since evicted. Both are
    /// categories rather than quotations, which is why they are lower case and
    /// drawn in the muted ink.
    RailPeekEmptyCommand,
    RailPeekEmptyLine,

    // ── the search capsule (`crate::search`) ───────────────────────────────
    //
    // `Aa`, `ab`, `.*`, `3/17`, `0/0` and the em dash the broken pattern shows
    // are not here: they are symbols and quantities, and a translated `Aa` would
    // be a toggle nobody recognises.
    SearchPlaceholder,
    SearchTipCase,
    SearchTipWord,
    SearchTipRegex,
    /// The three that carry a chord. The caps inside the parentheses are keys
    /// and stay in both columns.
    SearchTipPrevious,
    SearchTipNext,
    SearchTipClose,

    // ── the terminal's right-click menu (`crate::profiles`) ────────────────
    //
    // **Length-sensitive as a set**: the menu is measured against its widest row
    // and every row is drawn at that width, so the Chinese column is written to
    // stay inside the English one's box.
    TermMenuCopy,
    TermMenuPaste,
    TermMenuSelectAll,
    /// The ellipsis is this window's promise that a row asks before it acts.
    TermMenuFind,
    TermMenuClearScreen,
    TermMenuClearScrollback,
    /// The seventh row: the shell in this pane is ended and a new one takes its
    /// seat, on the same profile, in the last folder it reported.
    TermMenuShellAgain,

    // ── the pane's `⌄` menu (`crate::profiles`) ────────────────────────────
    /// The submenu head. The `▸` is drawn rather than written.
    PaneMenuSplitWith,
    PaneMenuNewInFolder,
    PaneMenuDuplicate,
    PaneMenuMoveToNewTab,
    /// The picker's caption, upper-cased at the source with the group labels it
    /// shares its grammar with.
    PaneMenuSplitCaption,
    /// **One verb, three surfaces**: the pane head's `×`, this menu's own row
    /// and the shortcut table's line for `Ctrl+Shift+W`.
    ClosePane,
    /// The tip on the `⌄` itself. It names what the menu is *for* rather than
    /// what it is, and the second clause is there because the sixth entry closes
    /// the pane.
    PaneChevronTip,

    // ── a git row's menu and the branch filter (`crate::profiles`) ─────────
    GitMenuCheckout,
    GitMenuCreateBranch,
    GitMenuCreateTag,
    GitMenuRename,
    GitMenuDeleteTag,
    GitMenuCheckoutTracking,
    GitMenuOpenDiff,
    /// **`Reveal in Explorer` — one verb, and since 2026-08-25 three menus.**
    ///
    /// It was written for the Git row's menu and it sits in that block still,
    /// because moving a string is how a merge loses one. What changed is who
    /// says it: a tree's file row and a tree's folder row offer the same verb,
    /// and a window that spelled it `Show in Explorer` on one menu and
    /// `Reveal in Explorer` on another would be teaching a reader that the two
    /// do different things. `Runtime::reveal_in_explorer` already makes that
    /// argument about the three *feet*; this is the same argument about the
    /// words.
    ///
    /// `Explorer` is the name of a program on this machine and stays.
    MenuRevealInExplorer,
    GitMenuCopyHash,
    GitMenuCopySubject,
    GitMenuCopyName,
    GitMenuCompareSelected,
    GitMenuCompareWorking,
    GitFilterShowRemotes,
    GitFilterShowTags,
    /// What the three named prompts' empty fields say they want.
    GitPromptBranchName,
    GitPromptTagName,
    GitPromptNewName,

    // ── the confirmation gate (`crate::restore`) ───────────────────────────
    //
    // The gate had one question when this table was written and has six now.
    // Its two buttons are shared with the menus whose verbs they carry, which is
    // the gate's own rule stated in `GATE_DELETE_TEXT`: a gate headed
    // `Delete this branch?` whose other button said `Discard` would be two words
    // for one act.
    GateDiscard,
    GateCancel,
    GateDelete,
    GateClear,
    GateTitleUnsaved,
    /// It says *changes*, not *unsaved changes*: the file on disk is saved, and
    /// what is about to go is the difference between it and what git has.
    GateTitleGitDiscard,
    GateTitleGitDelete,
    GateTitleGitDeleteBranch,
    GateTitleGitDeleteTag,
    GateTitleClearScrollback,

    // ── a ref name the field can already tell is impossible (`crate::git`) ──
    //
    // git's rule in the reader's words rather than git's: `check-ref-format`
    // answers with an exit status and has no sentence to quote. The characters
    // named in them are characters and are not translated.
    RefNameEmpty,
    RefNameSpace,
    RefNameRange,
    RefNameReserved,
    RefNameDash,
    RefNameLock,
    RefNameShape,

    // ── the file peek card and the diff document ───────────────────────────
    /// The card's foot. `Enter` is a key cap.
    PeekFoot,
    /// Its refusal — the preview pane's own sentence said in one line, and a
    /// different sentence from [`Self::PreviewRefusalBinary`] because this one
    /// covers the unrecognised type as well.
    PeekUnknown,
    /// A diff with nothing in it, which is not a failure: a commit's reading of
    /// a file it did not touch, or two copies that agree.
    GitDocumentEmpty,

    // ── a drag's landing caption, the two that escaped ─────────────────────
    /// The captions a file row and a folder row earn over a preview seat, beside
    /// [`Self::DragSwapPanes`] and [`Self::DragReplacePane`].
    DragOpenInPreview,
    DragRootTreeHere,

    // ── the Terminal page's Scrollback row (P2-9 slice 2, 2026-08-19) ──────
    //
    // One contiguous block at the end, per this table's standing rule.
    /// The row that names how much of its own past a pane keeps.
    RowScrollback,
    /// Its sentence. Says **per pane**, because that is the half a reader
    /// cannot infer: the number is spent once for every terminal on screen,
    /// and a window with four of them is holding four of these.
    DescScrollback,
    /// The row directly under it: how a pane sets a line too long to fit it.
    RowLineWrapping,
    /// Its sentence when wrapping is on, and one of the two halves of the only
    /// description on this page that reads its own value.
    DescLineWrappingOn,
    /// And its sentence when wrapping is off, which **names the gesture**.
    ///
    /// The one line in this product that talks about reading sideways. A pane
    /// that has stopped folding is a pane whose right-hand text is off the edge
    /// with nothing on screen saying how to reach it, and there is no other
    /// surface where that would be said: the horizontal bar appears only once
    /// there is something to scroll to, which is exactly when a reader has
    /// already concluded the text is gone. So the row pays the debt in a clause
    /// on a sentence that was going to be written anyway — the same trade
    /// §7.1.6b′ made on [`Self::DescFocusCardHeight`] rather than growing a
    /// second UI surface to explain the first.
    DescLineWrappingOff,
    // ── multiwindow slice C ───────────────────────────────────────────────
    //
    // One contiguous block at the end, per this table's standing rule.
    /// The Shortcuts row that opens a second window.
    ///
    /// **Its own words and not [`Self::RailNewTab`]'s**, which is the opposite
    /// of the ruling one screen up about `New tab` and `Settings`: those two are
    /// the same verb the rail and the gear already name, and this is a different
    /// verb that happens to start with the same word. A row reading `New tab`
    /// for a chord that opens a window would be the page lying about what the
    /// key does.
    ShortcutNewWindow,

    // ── standing somewhere else (user ruling, 2026-08-19) ──────────────────
    //
    // One contiguous block at the end, per this table's standing rule. The
    // three gate entries belong to `crate::restore` and the two graph entries
    // to `crate::git_graph`; they are together here because they are one
    // ruling, and a reader who changes the word for one of them has to see the
    // rest.
    /// The gate over a checkout that leaves `HEAD` on no branch. It names the
    /// **state you come out in**, which is the thing the row you pressed does
    /// not show — nothing is lost, and the title does not pretend otherwise.
    GateTitleGitDetach,
    /// And over any checkout at all while the working tree has changes, which
    /// is the more serious half whenever both are true.
    GateTitleGitDirtyCheckout,
    /// The word on the button that goes through with either — the row's own
    /// verb, on [`Self::GateDelete`]'s rule.
    GateCheckout,
    /// What the door out of a detached `HEAD` says when the pointer rests on
    /// it.
    GraphToolLeaveDetachedTip,
    /// And what the door itself says, before the branch's own name. **A
    /// destination and not a verb**: `Checkout` is the word the row menu uses
    /// for going anywhere, and this button is for the one place a reader in
    /// this state is trying to get to.
    GraphLeaveDetached,

    // ── focus mode (§7.1.6b′, slice F1, 2026-08-19) ────────────────────────
    //
    // One contiguous block at the end, per this table's standing rule. **Three
    // entries, and only three**: the mode's surfaces are the `Appearance` row,
    // the column's own heading and the `Exit` button on it, and the first two
    // share one string because they name one bit — the precedent is `RailNewTab`,
    // which titles both the rail's `+` and the `new-tab` binding. It was five
    // entries for one day; the two that named the pane menu's row left with that
    // door on 2026-08-19, because a string table that still holds the words of a
    // withdrawn surface is how a withdrawn surface comes back by accident.
    /// `Appearance ▸ Focus mode` — the row, and the name of the thing itself.
    RowFocusMode,
    /// Its sentence. Says what the column *is* and names the chord, because this
    /// row and that chord are the only two ways in — and, since 2026-08-20, the
    /// only two ways out as well (the 08-19 ruling withdrew the pane-header
    /// double-click and the pane menu's row; the 08-20 one withdrew the `Exit`
    /// button, its heading row and the `Esc` rung).
    DescFocusMode,
    // `FocusExit` — the word on a button at the head of the card column — left
    // this table with the button on 2026-08-20, for the same reason the pane
    // menu's two rows left it on 08-19: a string table that still holds the words
    // of a withdrawn surface is how a withdrawn surface comes back by accident.
    // **Two entries for a four-item picker**, and the arithmetic is
    // `BlockMaxHeight`'s: the row's name and its sentence are words, `Off` is a
    // word this table already holds ([`Self::OptionOff`]), and `2:1` / `3:1` /
    // `4.5:1` are quantities, which this table's own header excludes. A ratio
    // written in Chinese would be the same three characters it is in English.
    /// `Appearance ▸ Advanced ▸ Minimum contrast` — the row.
    RowMinimumContrast,
    /// Its sentence. Names what moves (the ink), what does not (the paper), and
    /// the fact a reader has to know before they turn it on — that this is the
    /// one row in the dialog that overrides a colour a program asked for.
    DescMinimumContrast,

    // ── desktop notifications (§7.6, Windows landing slice 3, 2026-08-20) ──
    //
    // One contiguous block at the end, per this table's standing rule. **Two
    // entries**, the row and its sentence, because the notification's own words
    // are never this table's: a title is the tab's name and a body is what the
    // program printed, and a string table with a spare notification sentence in
    // it is a table waiting for someone to use it as a default.
    /// `Terminal ▸ Notifications` — the row.
    RowNotifications,
    /// Its sentence. Says the two things a reader cannot infer from the word:
    /// **a program has to ask** (this is not a bell, and not every shell speaks
    /// the sequence), and **nothing arrives while you are looking at the pane** —
    /// which is the answer to "will this interrupt me while I work".
    DescNotifications,

    // ── the turn-end lane (`attention` plan §11.7, ruled 2026-08-25) ───────
    //
    // One contiguous block at the end, per this table's standing rule. **Four
    // entries**: the row, its sentence, and the two words a turn-end
    // interruption wears when the program that reported it wrote none of its
    // own. The last two are the exception the block above declared it would not
    // make, and the difference is the one that licenses it: an `OSC 9` toast
    // carries text the *program* wrote, so a default sentence there would be a
    // table waiting to be used as one — whereas a hook `Stop` and a bare bell
    // carry no words at all, and something has to be said. What is said is a
    // statement of the fact and nothing else.
    /// `Terminal ▸ Turn finished` — the row.
    RowTurnEndNotifications,
    /// Its sentence. Says the two things the word does not: **what reaches the
    /// desktop** (a taskbar flash, or a toast when the window is not on any
    /// screen), and **what does not change** — the marks inside the window,
    /// which are not this row's to switch off.
    DescTurnEndNotifications,
    /// The body of a turn-end interruption from a source that wrote no sentence
    /// of its own. States the fact; names nothing to act on.
    ToastTurnFinished,
    /// The body of a queued request's one interruption when no program lent it
    /// words. The queue's own claim, said in the voice the dot says it in.
    ToastWaitingForYou,
    /// The heading of the card raised in this window when the desktop refused a
    /// notification (user ruling 2026-08-25). A refusal that only reached
    /// `stderr` is a refusal nobody sees, and the one thing worse than not being
    /// notified is not being notified silently.
    NotifyRefusedTitle,
    /// Its sentence — what was lost and what was not. The platform's own words
    /// are appended after it, because the reason is the platform's to give and
    /// this table must not paraphrase an error it has not seen.
    NotifyRefusedBody,

    // ── the PowerShell integration notice (§7.1.6j, ruled 2026-08-21) ──────
    //
    // One contiguous block at the end, per this table's standing rule. **Four
    // entries and not five**: the strip's second state offers `Restart shell…`,
    // and that verb already has a string — the pane menu's
    // [`Self::TermMenuShellAgain`] — which both surfaces spend on the same
    // function. A second spelling of one verb is two places to translate and one
    // of them eventually different.
    /// The sentence a PowerShell pane wears when its `$PROFILE` does not
    /// dot-source the script: what is missing, and what this terminal uses it
    /// for. `$PROFILE`, `Folio` and `folio.ps1` are names and stay in Latin in
    /// both columns.
    PowerShellNoticeBody,
    /// The verb that writes the line. It names the file it writes into, because
    /// the one thing a reader must know before pressing it is *which* file
    /// changes.
    PowerShellNoticeAdd,
    /// The verb that answers the question for good. Not "never" and not "no
    /// thanks": it says what pressing it stops, which is the asking.
    PowerShellNoticeNever,
    /// What the strip says after the write. **Two facts and no thanks**: the
    /// line is in the file, and the shell already running was started before it
    /// and cannot see it.
    PowerShellNoticeAdded,

    // ── its settings row (Terminal page) ───────────────────────────────────
    /// `Terminal ▸ Offer PowerShell integration` — the row that turns the offer
    /// back on after `Don't show again`.
    RowPowerShellOffer,
    /// Its sentence. Says the two things the row's four words cannot: which
    /// panes are ever asked, and that the asking is all this does.
    DescPowerShellOffer,

    // ── Claude Code's hooks (attention plan §3.3, Terminal page) ────────────
    /// `Terminal ▸ Claude Code hooks` — the row that lets an agent say it is waiting.
    RowClaudeHooks,
    /// Its sentence. Says the two things the row's three words cannot: what is written, and into
    /// whose file — a terminal editing another program's configuration is worth saying out loud.
    DescClaudeHooks,
    /// The card raised when the block lands.
    ClaudeHooksAddedToast,
    /// And when it is taken back out.
    ClaudeHooksRemovedToast,
    /// The refusal, with the reason appended by the caller.
    ///
    /// One string and not four, because the four reasons are all "the file could not be written"
    /// wearing different hats and a reader takes the same action for every one of them.
    ClaudeHooksFailedToast,
    // ── the focus card's height (§7.1.6b′, user ruling 2026-08-21) ──────────
    //
    // One contiguous block at the end, per this table's standing rule. **Two
    // entries**, the row and its sentence, because the three items are
    // quantities — `160 px` / `240 px` / `320 px` — and this table's own header
    // excludes those: a pixel count written in Chinese is the same three
    // characters it is in English. The same arithmetic `Scrollback` came in on.
    /// `Appearance ▸ Focus card height` — the row.
    RowFocusCardHeight,
    /// Its sentence. Says the one thing a picker of pixels cannot: that what a
    /// reader is choosing is **rows**, and how many each rung buys. It names no
    /// program — the case that produced the ruling was an agent whose status bar
    /// filled the card, and a settings row that named somebody's tool would be a
    /// row that goes stale the day they change tools.
    DescFocusCardHeight,

    // ── the page's hand-off arrow (§7.1.5g, user ruling 2026-08-20) ─────────
    //
    // One contiguous block at the end, per this table's standing rule. **One
    // entry**: the tip on the `↗` a preview head wears while it is showing a
    // page. It says what pressing it does and names no program — which browser a
    // machine keeps is the machine's business, and a tip that said `Chrome` would
    // be this window guessing.
    /// `.pv-tool.pv-browser`'s tip.
    PreviewOpenInBrowser,

    // ── the web seat's own chrome (§7.7, W2 slice ④, 2026-08-22) ────────────
    //
    // One contiguous block at the end, per this table's standing rule. Fifteen
    // entries and no more: the three navigation buttons and the stop they turn
    // into, the four rows this slice puts in the shortcut table, and the five
    // failure cards' sentences and verbs. Everything else the seat says is
    // either a value (a host name, a scheme, an error string — see
    // `web_fail_did_not_respond` below) or a word this table already owns
    // (`HyperlinkBlockedSuffix` is the foot's `· blocked`, and it is the same
    // ruling's same word).
    /// The `Address` row of the shortcut table, and the name of the verb
    /// `Ctrl+L` runs. Not `Address bar`: there is no bar — the page's title *is*
    /// the field, and the row names what the caret lands in.
    ShortcutWebAddress,
    /// The `Developer tools` row, and the tip on the head's `</>`. One entry for
    /// both on `RowFocusMode`'s precedent: the chord and the button turn the
    /// same verb, so they are one name.
    ShortcutWebDevTools,
    /// The `Close search` row — bare `Escape`, in force only while the capsule
    /// is up. See `shortcuts::BINDINGS` for why a page makes this a row of the
    /// table rather than a rung the ladder can keep to itself.
    ShortcutCloseSearch,
    /// The tag the shortcut editor prints under a row that is only in force
    /// while a page holds the keyboard.
    ShortcutScopeWebPage,
    /// The tag for a row in force on either of the search capsule's two hosts.
    ShortcutScopeSearchHost,
    /// `.pv-nav.pv-back`'s tip.
    PreviewWebBack,
    /// `.pv-nav.pv-fwd`'s tip.
    PreviewWebForward,
    /// `.pv-nav.pv-reload`'s tip while nothing is in flight — and the single
    /// verb on the two failure cards that offer one (§7.7 ④). One entry, because
    /// the button and the card's button are the same word for the same call.
    PreviewWebReload,
    /// The same button's tip while a navigation is in flight, when it is a stop.
    PreviewWebStop,
    /// The `Runtime missing` card's sentence.
    WebFailRuntimeSay,
    /// Its one verb.
    WebFailRuntimeVerb,
    /// **The sixth card's sentence** (user ruling 2026-08-25) — the runtime is
    /// installed and the engine still did not come up. States the failure and
    /// stops: what call failed and with what code is the line underneath, which
    /// is a fact and not a sentence.
    WebFailEngineSay,
    /// Its one verb. `Retry` and not `Reload`: nothing was loaded, so there is
    /// nothing to load again — what is being asked for again is the engine.
    WebFailEngineVerb,
    /// The `Process stopped` card's sentence. Names the render process, because
    /// that is the fact a reader can act on: the window did not die, one page's
    /// renderer did.
    WebFailCrashSay,
    /// The `Blocked` card's sentence when the address carries no scheme to name.
    /// The spelling that *can* name one is `web_fail_blocked_scheme` below.
    WebFailBlockedSay,
    /// Its one verb — the address is on the card, so the verb is to take it.
    WebFailBlockedVerb,
    /// The `Download refused` card's two sentences. The second is the fact, not
    /// prose: it says which property of the request cannot cross a plain link,
    /// which is the difference between this card and one that could offer a
    /// retry.
    WebFailDownloadSay,
    /// Its one verb. The download is gone; the page that asked for it is not,
    /// and handing *that* over is the one action that still gets the file.
    WebFailDownloadVerb,
    /// `General ▸ Search engine` — the row.
    RowSearchEngine,
    /// Its sentence. Says the one thing the three names cannot: **when** this
    /// row is consulted at all.
    DescSearchEngine,
    /// **The glance card over a name that opens as a page** (user ruling
    /// 2026-08-23; `docs/DESIGN.md` §7.10 ⑥). The card cannot draw a page, so it
    /// says what the row does instead of what the card cannot do.
    PeekOpensAsPage,
    /// **`Quit` — the shortcut row's own name** (multiwindow slice E2).
    ///
    /// Not `Close window`: that row already exists three lines up in this table
    /// and means the other thing. This one takes the whole application with it.
    ShortcutQuit,
    /// The summary card's question (§2.9).
    ///
    /// It names the moment — *before quitting* — because that is the whole of
    /// what separates this card from the shut gate a reader has met before: the
    /// gate stands in front of one window and this one stands in front of the
    /// process.
    QuitTitle,
    /// The word on the button that writes everything back before leaving.
    QuitSave,
    /// **The card a quit that could not write `session.json` raises** (slice E2
    /// phase ③).
    ///
    /// Two facts and no advice: the file did not go down, and nothing was closed.
    /// The second half is the one a reader cannot see — the windows are still
    /// there, so the only way to know the quit did not silently half-happen is to
    /// be told.
    QuitSessionNotWritten,

    // ── the preview pane's lock (§7.7 ⑧, Claude 定 2026-08-23) ─────────────
    //
    // One contiguous block at the end, per this table's standing rule. **Two
    // entries, and the reason there are any is the naming ticket W1 filed**:
    // this control drew a pin and answered to the word `pin`, and so do two
    // other controls in this window that mean the opposite kind of thing —
    // a tab's pin and a switcher row's both say "this one stays in the list".
    // It is a padlock now, and a padlock in a preview head is the one mark
    // there this product has not taught elsewhere, so it says what it does.
    //
    // TWO STRINGS AND NOT ONE, on `PreviewWebReload`/`PreviewWebStop`'s own
    // precedent: the button changes, so the tip changes with it. A single tip
    // reading `Lock` over an engaged lock would be the head describing what it
    // was a press ago.
    //
    // THEY SAY WHAT HAPPENS AND STOP, per the copy ruling of 2026-08-17 — what
    // the press does and what follows from it, no first person and no advice.
    // Neither says "file" or "page": a preview seat holds either, and a tip
    // that named one would be wrong on the other half of the seats.
    /// The tip while the pane is unlocked — the action.
    PreviewLock,
    /// The tip while the pane is locked — the state, and the way out of it.
    PreviewUnlock,
    // ── multiwindow slice F1c ─────────────────────────────────────────────
    //
    // One contiguous block at the end, per this table's standing rule.
    /// **The pane menu's sixth verb**: this pane leaves for a window of its own.
    ///
    /// Its neighbour one row up says `new tab`, and the only word that differs
    /// is the container — which is the whole of the difference between the two
    /// verbs, so it is the only word that may differ.
    PaneMenuMoveToNewWindow,
    /// The card when the tab or the window stopped being open between the press
    /// and the move. A timing the platform is entitled to, said plainly.
    MoveRefusedGone,
    /// The card when the tab is already standing in the window it was asked
    /// into.
    MoveRefusedAlreadyThere,
    /// **The second half of a refusal that came after a pane was promoted**
    /// (§2.10, F1c).
    ///
    /// `Move pane to new window` is two steps — the pane becomes a tab, and the
    /// tab moves — and only the second can be refused. When it is, the first has
    /// already happened, and a card reporting only the refusal would leave the
    /// reader to discover the tab for themselves.
    MoveRefusedPaneIsNowATab,

    // ── the window's own address door (§7.7 ⑨, Claude 定 2026-08-24) ────────
    //
    // One contiguous block at the end, per this table's standing rule. One
    // entry.
    /// **The `Open address` row of the shortcut table** — `Ctrl+Shift+L`.
    ///
    /// Not `Address`, which is the row two lines above it in the editor and
    /// means the other thing: that one puts the caret in a field that is already
    /// on the glass, and this one is answerable for there being a field at all.
    /// The verb is the difference, so the verb is the word that separates them.
    ShortcutWindowAddress,
    // ── single-pane zoom (§7.1.6l, Claude 定 2026-08-24) ────────────────────
    //
    // One contiguous block at the end, per this table's standing rule. Two
    // entries, and two rather than one for `PreviewLock`/`PreviewUnlock`'s
    // reason: the row is a toggle, so it says one thing over a tiled pane and
    // another over the one on the stage, and a single string would be the menu
    // describing what the pane was a press ago.
    /// **The pane menu's zoom row while this tab is tiled** — the action.
    PaneMenuZoom,
    /// **The same row while this pane is the stage** — the state's way out.
    ///
    /// `Restore` and not `Unzoom`: the word names what comes back, which is the
    /// other panes, and it is the word this operating system has used for the
    /// window button that undoes a maximize for thirty years.
    PaneMenuRestore,

    // ── the hint a held modifier raises (§7.1.5e′, Claude 定 2026-08-25) ─────
    //
    // One contiguous block at the end, per this table's standing rule. Two
    // entries: a row and its sentence. The card's own body carries no words of
    // its own at all — every line of it is a verb's name out of the shortcut
    // table, which is the whole of「不新造文案」.
    /// **`General ▸ Shortcut hints`** — the row.
    ///
    /// It names what appears rather than what raises it. `Hold Ctrl to see
    /// shortcuts` would be a title that teaches the gesture and then has nothing
    /// left to say in the line under it, and a title that is an instruction reads
    /// as a step in a wizard rather than as a thing that is either on or off.
    RowKeyHints,
    /// Its sentence — the gesture, because the title deliberately does not carry
    /// it, and the one fact a reader needs before deciding: **it never takes a
    /// key**.
    DescKeyHints,
    // ── the tree row's menu, completed (user ruling 2026-08-25) ────────────
    //
    // One contiguous block at the end, per this table's standing rule. Four
    // entries, and **`Reveal in Explorer` is not among them**: that verb already
    // has a string in this table ([`Text::MenuRevealInExplorer`], written for the
    // Git row's menu) and it means exactly the same thing on a tree row. One
    // verb printed on three menus is one verb — the rule
    // `Runtime::reveal_in_explorer` already states about the three feet, said
    // about their words.
    /// **`Open with default app`** — the file menu's second door out.
    ///
    /// Names the *handler* and not the program, because which program that is
    /// belongs to the machine and changes with the file: a row that promised
    /// `Open in Notepad` would be this window guessing at a registry it does not
    /// own. `default app` is Windows' own phrase for the same association.
    FileMenuOpenWith,
    /// **A folded folder row's first verb.**
    ///
    /// Two strings and not one, on `PaneMenuZoom`/`PaneMenuRestore`'s precedent:
    /// the row is a toggle, so it says one thing over a shut folder and another
    /// over an open one, and a single string would be the menu describing what
    /// the row was a press ago.
    FolderMenuExpand,
    /// **An unfolded folder row's first verb** — the state's way out.
    FolderMenuCollapse,
    /// **`New terminal here`** — a folder row's own shell.
    ///
    /// The pane menu's `New terminal in folder…` without its ellipsis, and the
    /// ellipsis is the whole of the difference: that row asks you which folder
    /// before anything happens, and this one already knows — you right-clicked
    /// it. Keeping the three dots here would be a promise of a chooser that
    /// never comes.
    FolderMenuNewTerminal,

    // ── the file menu's rename row (B5, user ruling 2026-08-25) ────────────
    //
    // One contiguous block at the end, per this table's standing rule. **One
    // entry, and only one**: the two faces that offer it — a file row and a
    // folder row — say the same word, because what the row does is the same on
    // both and a menu that said `Rename file` over one and `Rename folder` over
    // the other would be naming the subject the row above it already named. The
    // breadcrumb's tail has no string of its own either: it is a double click on
    // a name, and a gesture is not a word.
    /// **`Rename`** — a tree row's name, changed where it stands.
    FileMenuRename,
    /// **`Discard all`** — the exit card's destructive answer (B1, user ruling
    /// 2026-08-25).
    ///
    /// A second entry beside [`Self::GateDiscard`] and not a replacement for it:
    /// the other gates ask about **one** thing — a branch, a tag, a pane's
    /// transcript — and `Discard all` over a single subject would be a plural
    /// with nothing to be plural about.
    GateDiscardAll,
    /// **`Move to window`** — the pane menu's third exit (B9, user ruling
    /// 2026-08-25).
    ///
    /// No ellipsis and no plural: the row opens a list rather than a chooser,
    /// and the list is what says which windows there are.
    PaneMenuMoveToWindow,

    // ── the Agents page (user ruling 2026-08-25) ───────────────────────────
    //
    // One contiguous block at the end, per this table's standing rule. **Seven
    // entries**: the page's own two words — the heading and the word in the rail,
    // which are two literals rather than one transformed, per this file's own
    // ruling on case — and the codex installer's row, its sentence and its three
    // cards.
    /// `AGENTS` — the page's heading.
    CategoryAgents,
    /// `Agents` — the word in the rail.
    NavAgents,
    /// `Agents ▸ Codex notify` — the row.
    RowCodexNotify,
    /// Its sentence. Two facts and a boundary, on `DescClaudeHooks`'s shape: what
    /// is written and where, and then **which** of the two things an agent can
    /// say this one carries. The third clause earns its place — the row beside it
    /// arranges for a pane to say *it is waiting for you*, and a reader who
    /// assumed this did the same would install it, watch an approval box sit
    /// there with nothing on the tab, and conclude the switch was broken.
    DescCodexNotify,
    CodexNotifyAddedToast,
    CodexNotifyRemovedToast,
    /// The refusal's own half of the sentence; the reason follows it, in the
    /// installer's words.
    CodexNotifyFailedToast,
}

impl Text {
    /// The words, in the language this process was started in.
    #[must_use]
    pub fn text(self) -> &'static str {
        self.in_lang(current())
    }

    /// The words in a named language — the whole table, and the entry point for
    /// every test that has to read both columns at once.
    #[must_use]
    pub fn in_lang(self, lang: Lang) -> &'static str {
        match self {
            // ── window chrome ──────────────────────────────────────────────
            Self::Settings => pick(lang, "Settings", "设置"),
            // Mock-up 2270's own text, which names the verb in both directions.
            Self::ToggleSidebar => pick(lang, "Toggle sidebar", "切换侧栏"),
            // Windows' own words for its own buttons.
            Self::Minimize => pick(lang, "Minimize", "最小化"),
            Self::Maximize => pick(lang, "Maximize", "最大化"),
            Self::CloseWindow => pick(lang, "Close", "关闭"),

            // ── tab strip and rail ─────────────────────────────────────────
            Self::RailTabs => pick(lang, "Tabs", "标签"),
            Self::RailNewTab => pick(lang, "New tab", "新建标签"),
            Self::ChooseProfile => pick(lang, "Choose a profile", "选择配置文件"),
            Self::Pin => pick(lang, "Pin", "固定"),
            // Names the verb *and* the reason, because "Unpin" alone does not
            // explain where the close button went (mock-up 4204).
            Self::Unpin => pick(
                lang,
                "Unpin — a pinned tab closes only after unpinning",
                "取消固定 —— 固定的标签要先取消固定才能关闭",
            ),

            // ── a tab's tip ────────────────────────────────────────────────
            Self::NameSourceManual => pick(lang, "Named by you", "你起的名字"),
            Self::NameSourceProgram => pick(lang, "Set by the program", "程序设置的"),
            Self::NameSourceCwd => pick(lang, "Working folder", "工作目录"),
            Self::TabTipPinned => pick(
                lang,
                "\nPinned — restored next launch",
                "\n已固定 —— 下次启动会恢复",
            ),

            // ── the mark slot ──────────────────────────────────────────────
            Self::MarkWorking => pick(lang, "Working", "运行中"),
            Self::MarkWorkingIndeterminate => pick(lang, "Working…", "运行中…"),

            // ── pane heads ─────────────────────────────────────────────────
            Self::SeatTerminal => pick(lang, "Terminal", "终端"),
            Self::SeatFiles => pick(lang, "Files", "文件"),
            Self::SeatPreview => pick(lang, "Preview", "预览"),
            Self::SeatUnavailable => pick(lang, "Unavailable", "不可用"),
            Self::PlaceholderSeatNotice => pick(
                lang,
                "This pane was saved by a newer version of Folio",
                "这个窗格由更新版本的 Folio 保存",
            ),

            // ── the settings dialog ────────────────────────────────────────
            Self::CategoryGeneral => pick(lang, "GENERAL", "常规"),
            Self::CategoryAppearance => pick(lang, "APPEARANCE", "外观"),
            Self::CategoryTerminal => pick(lang, "TERMINAL", "终端"),
            Self::CategoryRenderedBlocks => pick(lang, "RENDERED BLOCKS", "渲染块"),
            Self::CategoryShortcuts => pick(lang, "SHORTCUTS", "快捷键"),
            Self::NavGeneral => pick(lang, "General", "常规"),
            Self::NavAppearance => pick(lang, "Appearance", "外观"),
            Self::NavTerminal => pick(lang, "Terminal", "终端"),
            Self::NavRenderedBlocks => pick(lang, "Rendered blocks", "渲染块"),
            Self::NavShortcuts => pick(lang, "Shortcuts", "快捷键"),

            Self::RowTheme => pick(lang, "Theme", "主题"),
            Self::RowCursor => pick(lang, "Cursor", "光标"),
            // 「行间」/「行内」is the pair Chinese typesetting already uses for
            // display and inline math, so the two rows read as a pair here for
            // the same reason they do in English.
            Self::RowFormulas => pick(lang, "Display formulas", "行间公式"),
            Self::RowInlineFormulas => pick(lang, "Inline formulas", "行内公式"),
            Self::RowGitPanel => pick(lang, "Git panel", "Git 面板"),
            Self::RowContextMenu => pick(lang, "Explorer context menu", "资源管理器右键菜单"),
            Self::RowTabLayout => pick(lang, "Tab layout", "标签布局"),
            Self::RowSidebar => pick(lang, "Sidebar", "侧栏"),
            Self::RowSplitDirection => pick(lang, "Split direction", "分屏方向"),
            Self::RowDefaultProfile => pick(lang, "Default profile", "默认配置文件"),
            Self::RowLanguage => pick(lang, "Language", "语言"),
            Self::RowTerminalFont => pick(lang, "Terminal font", "终端字体"),
            Self::RowFontSize => pick(lang, "Font size", "字号"),
            Self::RowPsReadLine => pick(lang, "PSReadLine patch", "PSReadLine 补丁"),

            Self::DescTheme => pick(
                lang,
                "Light, dark, or follow your system setting",
                "浅色、深色，或跟随系统设置",
            ),
            Self::DescCursor => pick(lang, "Focused cursor shape", "聚焦时的光标形状"),
            Self::DescFormulas => pick(
                lang,
                "Typeset $$…$$ blocks; off shows the LaTeX source",
                "排版 $$…$$ 块；关闭则显示 LaTeX 源码",
            ),
            Self::DescInlineFormulas => pick(
                lang,
                "Typeset $…$ in command output; off shows the source",
                "排版命令输出里的 $…$；关闭则显示源码",
            ),
            Self::DescGitPanel => pick(
                lang,
                "A Git page beside the file tree; off reads no repository",
                "文件树旁的 Git 页；关闭则不读取任何仓库",
            ),
            // **Two facts and no opinion about either.** What the entry says,
            // and where Windows 11 puts it — the second because it is the half a
            // reader would otherwise discover by switching this on, right-clicking
            // a folder, and seeing no change at all. See `context_menu`'s header
            // for why the classic registration cannot reach the short menu.
            //
            // **Length-sensitive, and measured rather than guessed.** A row's
            // sentence is drawn in the text column and ellipsises: at the
            // dialog's width there are about 54 characters of 12px English, and
            // the first draft ("Adds Open Folio here; Windows 11 files it under
            // Show more options", 64) came out cut at `Show m…` — losing exactly
            // the half the line exists for. The verb `Adds` went rather than the
            // place, because a row whose picker reads `On`/`Off` has already said
            // that something is being added.
            Self::DescContextMenu => pick(
                lang,
                "Open Folio here, under Windows 11's Show more options",
                "Open Folio here，在 Windows 11 的显示更多选项里",
            ),
            Self::DescTabLayout => pick(
                lang,
                "Choose where tabs appear in the window",
                "选择标签在窗口里的位置",
            ),
            Self::DescSidebar => pick(
                lang,
                "How the vertical tab sidebar rests",
                "竖向标签栏平时的形态",
            ),
            Self::DescSplitDirection => pick(
                lang,
                "Where a split with no direction of its own puts the new pane",
                "没有自带方向的分屏把新窗格放在哪边",
            ),
            Self::DescDefaultProfile => pick(
                lang,
                "What opens on a new tab, and when Folio starts",
                "新建标签时、以及 Folio 启动时打开什么",
            ),
            // **It used to say when it took effect** — "Applies the next time
            // Folio starts" — because that was the one surprising thing about
            // the row. It is not true any more (§7.1.6c-3c) and there is nothing
            // surprising left to say, so the line does what every other picker's
            // description in this dialog does: it names what is on offer. Theme's
            // own line is its model, down to the third item, because the third
            // item is the same item.
            Self::DescLanguage => pick(
                lang,
                "English, 中文, or follow your system setting",
                "English、中文，或跟随系统设置",
            ),
            // Names what it does *not* move, because that is the question a
            // reader has when a terminal offers one font row: the chrome keeps
            // its own face, and a line that said only "the font" would promise
            // otherwise.
            Self::DescTerminalFont => pick(
                lang,
                "The face the grid is drawn in; the window's own labels keep theirs",
                "网格所用的字体；窗口自身的文字保持原样",
            ),
            Self::DescFontSize => pick(
                lang,
                "How large grid text is drawn, before the display's scaling",
                "网格文字画多大，尚未乘显示器的缩放",
            ),
            Self::RowLightScheme => pick(lang, "Light scheme", "浅色配色"),
            Self::RowDarkScheme => pick(lang, "Dark scheme", "深色配色"),
            // The pair carries one fact each and the folder is named once, on
            // the second of the two, for the reason the font pair states its
            // left-out once: they are read together and a path repeated twice
            // reads as two paths.
            Self::DescLightScheme => pick(
                lang,
                "The colours a Light window is drawn in, terminal and chrome alike",
                "浅色窗口所用的颜色，终端与窗口自身同出一套",
            ),
            Self::DescDarkScheme => pick(
                lang,
                "The same for a Dark window. Files: %APPDATA%\\Folio\\schemes",
                "深色窗口同理。文件放在 %APPDATA%\\Folio\\schemes",
            ),
            Self::SchemeFileSkipped => pick(lang, "Colour scheme skipped", "配色文件已跳过"),

            // ── the window's ground (§7.1.6c-4b) ───────────────────────────
            Self::RowBackgroundImage => pick(lang, "Background image", "背景图片"),
            Self::RowImageFit => pick(lang, "Image fit", "图片适配"),
            Self::RowImageOpacity => pick(lang, "Image opacity", "图片不透明度"),
            Self::RowBackgroundOpacity => pick(lang, "Background opacity", "背景不透明度"),
            Self::RowAcrylic => pick(lang, "Acrylic", "亚克力"),
            Self::RowAlwaysOnTop => pick(lang, "Always on top", "总在最前"),
            Self::DescBackgroundImage => pick(
                lang,
                "Drawn once behind the window, beneath every pane",
                "在整扇窗口背后画一次，位于每个窗格之下",
            ),
            Self::DescImageFit => pick(
                lang,
                "How the picture meets a window that is not its shape",
                "图片与形状不同的窗口如何相配",
            ),
            Self::DescImageOpacity => pick(
                lang,
                "How much of the picture reaches the window",
                "图片有多少落到窗口上",
            ),
            Self::DescBackgroundOpacity => pick(
                lang,
                "Panes and the window ground; text and menus stay opaque",
                "作用于窗格与窗口底色；文字与菜单保持不透明",
            ),
            Self::DescAcrylic => pick(
                lang,
                "A Windows blur behind the ground. Visible only below full opacity",
                "底色之后的 Windows 模糊。仅在不透明度低于 100% 时可见",
            ),
            Self::DescAlwaysOnTop => pick(
                lang,
                "The window stays above other windows",
                "窗口保持在其他窗口之上",
            ),
            Self::DescAcrylicUnavailable => pick(
                lang,
                "This Windows has no system backdrop to draw",
                "此版本 Windows 没有可用的系统背景材质",
            ),
            Self::DescBackgroundOpacityUnavailable => pick(
                lang,
                "This window is composited opaque",
                "此窗口以不透明方式合成",
            ),
            Self::OptionImageNone => pick(lang, "None", "无"),
            Self::OptionImageChoose => pick(lang, "Choose…", "选择…"),
            Self::OptionFitStretch => pick(lang, "Stretch", "拉伸"),
            Self::OptionFitFill => pick(lang, "Fill", "填充"),
            Self::OptionFitTile => pick(lang, "Tile", "平铺"),

            Self::OptionSystem => pick(lang, "System", "系统"),
            Self::OptionLight => pick(lang, "Light", "浅色"),
            Self::OptionDark => pick(lang, "Dark", "深色"),
            Self::OptionCursorBar => pick(lang, "Bar", "竖线"),
            Self::OptionCursorBlock => pick(lang, "Block", "方块"),
            Self::OptionCursorUnderline => pick(lang, "Underline", "下划线"),
            Self::OptionHorizontal => pick(lang, "Horizontal", "横向"),
            Self::OptionVertical => pick(lang, "Vertical", "竖向"),
            Self::OptionOn => pick(lang, "On", "开"),
            Self::OptionOff => pick(lang, "Off", "关"),
            Self::OptionExpanded => pick(lang, "Expanded", "展开"),
            Self::OptionIcons => pick(lang, "Icons", "图标"),
            Self::OptionSplitAuto => pick(lang, "Auto (longer edge)", "自动（沿长边）"),
            Self::OptionSplitRight => pick(lang, "Right", "右侧"),
            Self::OptionSplitDown => pick(lang, "Down", "下方"),

            Self::PsReadLineProbing => pick(
                lang,
                "Checking this machine's PSReadLine",
                "正在检查本机的 PSReadLine",
            ),
            Self::PsReadLineRowGone => pick(
                lang,
                "The copy Folio installed is no longer on disk",
                "Folio 安装的那一份已不在磁盘上",
            ),
            // **Length-sensitive.** The dialog's title is one unwrapped line in
            // a 400px float, exactly as the dirty gate's is; the first draft
            // ("…when the window resizes?") ran off the right edge on a real
            // window. It still names the symptom the reader has already seen
            // rather than the module, which is the half that matters.
            Self::PsReadLineInviteTitle => pick(
                lang,
                "Fix the input line after a resize?",
                "修复缩放后输入行错位？",
            ),
            Self::PsReadLineInstall => pick(lang, "Install", "安装"),
            Self::PsReadLineNotNow => pick(lang, "Not now", "以后再说"),
            Self::PsReadLineRemovedToast => pick(
                lang,
                "Removed. New PowerShell sessions use the module Windows ships",
                "已移除。新开的 PowerShell 会话将使用 Windows 自带的模块",
            ),

            // ── Explorer's context menu (§7.4) ─────────────────────────────
            //
            // **`ContextMenuVerb` is length-sensitive in a way nothing else in
            // this table is**: it is drawn by Explorer, in Explorer's own menu,
            // beside whatever else that machine has registered. Short enough to
            // sit among them, and it names the product because in that menu
            // there is nothing else to say which program this is.
            Self::ContextMenuVerb => pick(lang, "Open Folio here", "在 Folio 中打开"),
            Self::ContextMenuAddedToast => pick(
                lang,
                "Open Folio here is in Explorer's menu, under Show more options",
                "Open Folio here 已进入资源管理器菜单，在显示更多选项里",
            ),
            Self::ContextMenuRemovedToast => pick(
                lang,
                "Removed from Explorer's menu",
                "已从资源管理器菜单移除",
            ),
            Self::ContextMenuNoExecutable => pick(
                lang,
                "Windows did not say where folio.exe is",
                "Windows 没有说出 folio.exe 的位置",
            ),

            // ── the `˅` profile menu ───────────────────────────────────────
            Self::ProfileHintDefault => pick(lang, "default", "默认"),
            Self::ProfileHintUnavailable => pick(lang, "not installed", "未安装"),
            Self::ProfileHintCurrent => pick(lang, "current", "当前"),
            Self::ProfileFilesPane => pick(lang, "Files pane", "文件窗格"),
            Self::ProfileFilesPaneHint => pick(lang, "this tab", "本标签"),
            Self::ProfileRecentSection => pick(lang, "RECENTLY OPENED", "最近打开"),

            // ── the root menu ──────────────────────────────────────────────
            Self::RootSection => pick(lang, "OPEN FOLDER", "打开文件夹"),
            Self::RootBrowse => pick(lang, "Browse…", "浏览…"),
            Self::RootNoteHome => pick(lang, "home", "主目录"),
            Self::RootNoteTerminal => pick(lang, "a terminal is here", "终端在这里"),
            Self::RootNoteParent => pick(lang, "parent", "上一级"),

            // ── what the user said to keep ─────────────────────────────────
            Self::PinnedSection => pick(lang, "PINNED", "已钉住"),

            // ── a file row's menu ──────────────────────────────────────────
            Self::FileMenuOpenPreview => pick(lang, "Open preview", "打开预览"),
            Self::FileMenuOpenDefaultApp => pick(lang, "Open in default app", "用系统默认程序打开"),
            Self::PreviewRailOpen => pick(lang, "Open", "打开"),
            Self::PreviewRailOpenTip => pick(lang, "Open and more", "打开等操作"),
            Self::PreviewCopyAddress => pick(lang, "Copy address", "复制地址"),
            Self::PreviewFlipToSource => pick(lang, "View source", "查看源码"),
            Self::PreviewFlipToRendered => pick(lang, "View rendered", "查看渲染结果"),
            Self::FileMenuCopyPath => pick(lang, "Copy path", "复制路径"),
            Self::FileMenuInsertPath => pick(lang, "Insert path into terminal", "把路径插入终端"),

            // ── the files tree ─────────────────────────────────────────────
            Self::FilesLoading => pick(lang, "Loading…", "载入中…"),
            Self::FilesEmpty => pick(lang, "Empty folder", "空文件夹"),
            Self::FilesUnrooted => pick(lang, "No folder opened", "未打开文件夹"),
            Self::FilesPermissionDenied => pick(lang, "Permission denied", "没有权限"),
            Self::FilesNotFound => pick(lang, "Folder not found", "文件夹不存在"),
            Self::FilesUnreadable => pick(lang, "Could not read folder", "无法读取文件夹"),
            // "File Explorer" is a Windows product and has a Chinese name of its
            // own; using the English one here would send the reader looking for
            // something their Start menu does not have.
            Self::FilesRevealed => pick(
                lang,
                "Revealed in File Explorer",
                "已在文件资源管理器中显示",
            ),

            // ── the preview pane ───────────────────────────────────────────
            Self::PreviewEmptyState => pick(
                lang,
                "Click a dotted path to preview it here",
                "点一条带点线的路径，在这里预览",
            ),
            Self::PreviewRefusalType => pick(
                lang,
                "No preview for this file type",
                "这种文件类型没有预览",
            ),
            Self::PreviewRefusalBinary => pick(
                lang,
                "No preview — this looks like a binary file",
                "无法预览 —— 这看起来是二进制文件",
            ),
            Self::PreviewRefusalNetworkPath => pick(
                lang,
                "No preview — network paths are not read automatically",
                "无法预览 —— 网络路径不会自动读取",
            ),
            Self::PreviewRefusalPermissionDenied => pick(
                lang,
                "No preview — permission denied",
                "无法预览 —— 没有权限",
            ),
            Self::PreviewRefusalNotFound => pick(
                lang,
                "No preview — file not found",
                "无法预览 —— 文件不存在",
            ),
            Self::PreviewRefusalUnreadable => pick(
                lang,
                "No preview — could not read this file",
                "无法预览 —— 无法读取这个文件",
            ),
            Self::PreviewOpenExternally => pick(lang, "Open in default app", "用默认程序打开"),
            Self::PreviewOpened => pick(lang, "Opened \u{2713}", "已打开 \u{2713}"),
            // `64 KB` is a quantity, not a word.
            Self::PreviewTruncated => pick(lang, "Read-only · 64 KB", "只读 · 64 KB"),
            Self::PreviewSaved => pick(lang, "Saved", "已保存"),
            Self::PreviewConflict => pick(
                lang,
                "Not saved · changed on disk · edits kept",
                "未保存 · 磁盘上已改动 · 编辑仍在",
            ),
            // Lower case and no full stop, because it is a clause fitted into
            // [`not_saved`] rather than a sentence.
            Self::PreviewNothingToSave => {
                pick(lang, "there is nothing to save", "没有要保存的内容")
            }
            Self::PreviewFailedImageLoad => pick(
                lang,
                "Preview failed: image could not be loaded",
                "预览失败：图片无法载入",
            ),
            Self::PreviewFailedImageWorker => pick(
                lang,
                "Preview failed: image worker is unavailable",
                "预览失败：图片工作线程不可用",
            ),
            Self::PreviewFailedSeatTooSmall => pick(
                lang,
                "Preview failed: preview seat is too small",
                "预览失败：预览窗格太小",
            ),

            // ── the float and the folder trigger ───────────────────────────
            Self::FloatDock => pick(lang, "DOCK", "停靠"),
            Self::FloatTriggerTip => pick(lang, "Peek files here", "在这里速览文件"),

            // ── the restore prompt ─────────────────────────────────────────
            Self::RestoreTitle => pick(lang, "Reopen your other tabs?", "重新打开你的其他标签？"),
            // **Two facts and no aside** (user ruling, 2026-08-17: UI copy does
            // not editorialise). The sentence used to close on "— the output is
            // not ours to keep", which is the product explaining itself in the
            // first person about something it is not doing; what the reader has
            // to know is that the folders come back and the sessions are new,
            // and both of those are said before the clause began. The Chinese
            // loses the same clause and stops in the same place.
            Self::RestoreSub => pick(
                lang,
                "These were open when you last closed Folio. They come back in the \
                 folders you left them, as new shells.",
                "上次关闭 Folio 时它们还开着。它们会回到你离开时所在的目录，但都是新的会话。",
            ),
            Self::RestoreDecline => pick(lang, "No thanks", "不用了"),
            Self::RestoreAccept => pick(lang, "Restore", "恢复"),

            // ── the terminal's own status overlay ──────────────────────────
            Self::FilesProgramRefused => pick(
                lang,
                "The files tree does not run programs",
                "文件树不会运行程序",
            ),
            Self::MathWorkerStopped => pick(
                lang,
                "Formula rendering stopped; terminal input and output remain available",
                "公式渲染已停止；终端的输入输出仍然可用",
            ),
            Self::FilesWorkerStopped => pick(
                lang,
                "Directory reading stopped; terminal input and output remain available",
                "目录读取已停止；终端的输入输出仍然可用",
            ),
            Self::PreviewWorkerStopped => pick(
                lang,
                "File preview reading stopped; terminal input and output remain available",
                "文件预览读取已停止；终端的输入输出仍然可用",
            ),

            // ── the hyperlink overlay ──────────────────────────────────────
            Self::HyperlinkBlockedSuffix => pick(lang, " · blocked", " · 已拦截"),
            Self::HyperlinkBlocked => pick(lang, "blocked", "已拦截"),

            // ── a drag's landing caption ───────────────────────────────────
            Self::DragSwapPanes => pick(lang, "Swap panes", "交换窗格"),
            Self::DragReplacePane => pick(lang, "Replace pane", "替换窗格"),

            // ── the files column's own switch ──────────────────────────────
            Self::FilesViewFiles => pick(lang, "Files", "文件"),
            // A program's name, and the same three letters in a Chinese window
            // as in an English one — `git` is already left untranslated inside
            // every sentence in this table for exactly this reason.
            Self::FilesViewGit => pick(lang, "Git", "Git"),

            // ── the Advanced disclosure and its Reset ──────────────────────
            Self::AdvancedGroup => pick(lang, "ADVANCED", "高级"),
            Self::ResetAdvanced => pick(lang, "Reset to defaults", "恢复默认值"),
            Self::AddScheme => pick(lang, "Add scheme…", "添加配色…"),
            Self::InstallFonts => pick(lang, "Install fonts…", "安装字体…"),
            Self::SchemeInUseBroken => pick(lang, "Colour scheme not reloaded", "配色未重新载入"),
            Self::SchemeInUseGone => pick(lang, "Colour scheme not found", "找不到配色"),
            Self::SchemeDeleted => pick(lang, "Colour scheme deleted", "配色已删除"),
            Self::BackgroundPictureRefused => {
                pick(lang, "Background picture not shown", "背景图未显示")
            }

            // ── the Profiles page (§7.1.6c-6) ──────────────────
            Self::NavProfiles => pick(lang, "Profiles", "档案"),
            Self::CategoryProfiles => pick(lang, "PROFILES", "档案"),
            Self::ProfilesDuplicate => pick(lang, "Duplicate", "复制"),
            // Lower case, because it is a badge and not a heading: the type
            // raises it in the English and there is no case to raise in the
            // Chinese, which is the same ruling `CategoryGeneral` carries.
            Self::ProfilesBadgeDefault => pick(lang, "default", "默认"),
            Self::ProfilesBadgeHidden => pick(lang, "hidden", "已隐藏"),
            Self::CapFull => pick(
                lang,
                "Prompt marks, directory, exit codes and hyperlinks",
                "提示符标记、目录、退出码与超链接",
            ),
            Self::CapPowerShell => pick(
                lang,
                "Prompt marks, directory, exit codes and hyperlinks — with folio.ps1 dot-sourced",
                "提示符标记、目录、退出码与超链接 —— 前提是已点源 folio.ps1",
            ),
            Self::CapWslBash => pick(
                lang,
                "Prompt marks, directory, exit codes and hyperlinks — on a bash login only",
                "提示符标记、目录、退出码与超链接 —— 仅限 bash 登录",
            ),
            Self::CapCmd => pick(
                lang,
                "Directory and hyperlinks; no prompt marks, no exit codes",
                "目录与超链接；没有提示符标记，没有退出码",
            ),
            Self::CapNone => pick(lang, "No shell integration", "无 shell 集成"),
            // 「表格」is what every Chinese markdown editor calls this object, so
            // the row reads as the same noun the reader already knows.
            Self::RowTables => pick(lang, "Tables", "表格"),
            // Says what Off does, which is the rule `DescFormulas` states: a
            // reader who does not see the word "text" will expect the table to
            // disappear rather than to go back to being the pipes it was
            // printed as.
            Self::DescTables => pick(
                lang,
                "Draw markdown tables in output; off shows the pipe text",
                "绘制输出里的 markdown 表格；关闭则显示管道符原文",
            ),
            // 「最大高度」is the row, and the sentence is the mock-up's own
            // (9941): it says what the cap *does* rather than what it forbids,
            // which is the difference between a cap a reader will reach for and
            // one they will assume truncates their output. It fits the 150px
            // sentence column the dialog gives every row, which the mock-up's
            // longer second version did not.
            Self::RowBlockMaxHeight => pick(lang, "Maximum height", "最大高度"),
            Self::DescBlockMaxHeight => pick(
                lang,
                "Blocks taller than this scroll inside themselves",
                "更高的块在自己内部滚动",
            ),
            Self::OptionBlockHeightNone => pick(lang, "No limit", "不限"),
            // A verb, because that is what pressing `On` here does. 「更新」is
            // the word every Chinese software updater uses for exactly this.
            Self::PsReadLineUpdate => pick(lang, "Update", "更新"),

            // ── the profile editor (§7.1.6c-6b) ────────────────
            Self::ProfilesEdit => pick(lang, "Edit", "编辑"),
            Self::ProfilesHide => pick(lang, "Hide", "隐藏"),
            Self::ProfilesShow => pick(lang, "Show", "显示"),
            Self::ProfilesDelete => pick(lang, "Delete", "删除"),
            Self::ProfilesSetDefault => pick(lang, "Set as default", "设为默认"),
            Self::ProfilesAlreadyDefault => pick(lang, "Already the default", "已经是默认"),
            Self::ProfilesNew => pick(lang, "New profile", "新建档案"),
            Self::ProfilesRowName => pick(lang, "Name", "名称"),
            Self::ProfilesRowNameDesc => pick(
                lang,
                "What this profile is called on tabs, in the picker and in this list",
                "标签、选择器与本列表上写的名字",
            ),
            Self::ProfilesRowProgram => pick(lang, "Program", "程序"),
            Self::ProfilesRowProgramDesc => pick(
                lang,
                "The executable a new tab of this profile starts",
                "该档案新建标签时启动的可执行文件",
            ),
            Self::ProfilesRowStartingDir => pick(lang, "Starting directory", "起始目录"),
            Self::ProfilesRowStartingDirDesc => pick(
                lang,
                "Where a new tab of this profile opens",
                "该档案的新标签在哪里打开",
            ),
            Self::ProfilesRowColour => pick(lang, "Colour", "颜色"),
            Self::ProfilesRowColourDesc => pick(
                lang,
                "The mark that names this profile across the window",
                "窗口各处标示该档案的那个标记",
            ),
            Self::ProfilesInherit => pick(lang, "The current pane's folder", "当前 pane 的文件夹"),
            Self::ProfilesHome => pick(lang, "Home", "主目录"),
            Self::ProfilesChooseFolder => pick(lang, "Choose a folder…", "选择文件夹…"),
            Self::ProfilesColourBlue => pick(lang, "Blue", "蓝"),
            Self::ProfilesColourTeal => pick(lang, "Teal", "青"),
            Self::ProfilesColourGreen => pick(lang, "Green", "绿"),
            Self::ProfilesColourAmber => pick(lang, "Amber", "琥珀"),
            Self::ProfilesColourRed => pick(lang, "Red", "红"),
            Self::ProfilesColourMagenta => pick(lang, "Magenta", "品红"),
            Self::ProfilesColourViolet => pick(lang, "Violet", "紫"),
            Self::ProfilesColourSlate => pick(lang, "Slate", "灰蓝"),
            Self::ProfilesColourInherited => pick(lang, "Inherited", "沿用"),
            Self::ProfilesColourFixed => pick(lang, "Its own", "自带"),
            Self::ProfilesRowArgs => pick(lang, "Arguments", "参数"),
            Self::ProfilesRowArgsDesc => pick(
                lang,
                "Passed to the program ahead of anything the shell reads · spaces separate, double quotes group",
                "在 shell 读取任何东西之前传给程序 · 空格分词，双引号成组",
            ),
            Self::ProfilesRowEnv => pick(lang, "Environment", "环境变量"),
            Self::ProfilesRowEnvDesc => pick(
                lang,
                "Set for this profile's sessions, over what Folio sets",
                "为该档案的会话设置,覆盖 Folio 设置的值",
            ),
            Self::ProfilesRowHyperlink => pick(lang, "Force hyperlinks", "强制超链接"),
            Self::ProfilesRowHyperlinkDesc => pick(
                lang,
                "FORCE_HYPERLINK, the answer programs read before emitting a link",
                "FORCE_HYPERLINK，程序发出链接前读的那个回答",
            ),
            Self::ProfilesRowIntegration => pick(lang, "Shell integration", "Shell 集成"),
            Self::ProfilesEnvAdd => pick(lang, "Add", "添加"),
            Self::ProfilesEnvName => pick(lang, "Name", "名称"),
            Self::ProfilesEnvValue => pick(lang, "Value", "值"),
            Self::ProfilesAuto => pick(lang, "Auto", "自动"),
            Self::ProfilesOn => pick(lang, "On", "开"),
            Self::ProfilesOff => pick(lang, "Off", "关"),
            Self::ProfilesRestoreAll => pick(lang, "Restore all defaults", "全部恢复默认"),
            Self::ProfilesDeleteBtn => pick(lang, "Delete profile", "删除档案"),
            Self::ProfilesBrowse => pick(lang, "Browse…", "浏览…"),
            Self::ProfilesCannotHideDefault => pick(
                lang,
                "The default profile stays in the picker",
                "默认档案留在选择器里",
            ),
            Self::ProfilesCannotHideFallback => pick(
                lang,
                "Every fallback lands on this profile",
                "每一次降级都落在这个档案上",
            ),
            Self::ProfilesNameBlank => pick(lang, "A profile needs a name", "档案需要一个名字"),
            Self::ProfilesNameTaken => pick(
                lang,
                "Another profile is already called this",
                "已经有别的档案叫这个名字",
            ),
            Self::ProfilesUndo => pick(lang, "Undo", "撤销"),

            // ── §7.1.6c-6c ─────────────────────────────────────────────────
            Self::ProfilesIntegrationPowerShell => {
                pick(lang, "PowerShell script", "PowerShell 脚本")
            }
            Self::ProfilesIntegrationBash => pick(lang, "Bash init file", "Bash 初始文件"),
            Self::ProfilesIntegrationCmd => pick(lang, "Command Prompt", "命令提示符"),
            Self::ProfilesIntegrationNone => pick(lang, "None", "无"),
            Self::CapFullNoLinks => pick(
                lang,
                "Prompt marks, directory and exit codes; no hyperlinks",
                "提示符标记、目录与退出码;没有超链接",
            ),
            Self::CapPowerShellNoLinks => pick(
                lang,
                "Prompt marks, directory and exit codes with folio.ps1 dot-sourced; no hyperlinks",
                "已点源 folio.ps1 时有提示符标记、目录与退出码;没有超链接",
            ),
            Self::CapWslBashNoLinks => pick(
                lang,
                "Prompt marks, directory and exit codes on a bash login only; no hyperlinks",
                "仅 bash 登录时有提示符标记、目录与退出码;没有超链接",
            ),
            Self::CapCmdNoLinks => pick(
                lang,
                "Directory; no prompt marks, no exit codes, no hyperlinks",
                "目录;没有提示符标记,没有退出码,没有超链接",
            ),
            // The mock-up's own wording for this row (`.pf-view[data-view=edit]`,
            // the `Shell integration` line), which says what is lost and then
            // says the one thing that is not.
            Self::CapNoneLong => pick(
                lang,
                "No prompt marks, no directory, no exit codes. Hyperlinks are declared anyway",
                "没有提示符标记、没有目录、没有退出码。超链接仍然会被声明",
            ),
            Self::CapNoneLongNoLinks => pick(
                lang,
                "No prompt marks, no directory, no exit codes, no hyperlinks",
                "没有提示符标记、没有目录、没有退出码、没有超链接",
            ),

            // ── the Shortcuts page ─────────────────────────────────────────
            Self::ShortcutGotoTab1 => pick(lang, "Go to tab 1", "转到标签 1"),
            Self::ShortcutGotoTab2 => pick(lang, "Go to tab 2", "转到标签 2"),
            Self::ShortcutGotoTab3 => pick(lang, "Go to tab 3", "转到标签 3"),
            Self::ShortcutGotoTab4 => pick(lang, "Go to tab 4", "转到标签 4"),
            Self::ShortcutGotoTab5 => pick(lang, "Go to tab 5", "转到标签 5"),
            Self::ShortcutGotoTab6 => pick(lang, "Go to tab 6", "转到标签 6"),
            Self::ShortcutGotoTab7 => pick(lang, "Go to tab 7", "转到标签 7"),
            Self::ShortcutGotoTab8 => pick(lang, "Go to tab 8", "转到标签 8"),
            Self::ShortcutGotoTab9 => pick(lang, "Go to tab 9", "转到标签 9"),
            Self::ShortcutNextTab => pick(lang, "Next tab", "下一个标签"),
            Self::ShortcutPrevTab => pick(lang, "Previous tab", "上一个标签"),
            Self::ShortcutReopenClosed => {
                pick(lang, "Reopen the last closed tab", "重新打开最近关闭的标签")
            }
            Self::ShortcutJumpAttention => {
                pick(lang, "Jump to the longest waiting", "跳到等待最久的那个")
            }
            Self::ShortcutCommandPalette => pick(lang, "Command palette", "命令面板"),
            Self::ShortcutSplitHorizontal => pick(lang, "Split horizontally", "横向拆分"),
            Self::ShortcutSplitVertical => pick(lang, "Split vertically", "纵向拆分"),
            Self::ShortcutDuplicatePaneSplit => pick(
                lang,
                "Duplicate pane into a split",
                "把窗格复制到一个拆分里",
            ),
            Self::ShortcutFilesPane => pick(lang, "Files column", "文件列"),
            Self::ShortcutGitPage => pick(lang, "Turn the files column to Git", "把文件列切到 Git"),
            Self::ShortcutSavePreview => pick(lang, "Save the open document", "保存打开的文档"),
            Self::ShortcutPrevCommandMark => pick(lang, "Previous command", "上一条命令"),
            Self::ShortcutNextCommandMark => pick(lang, "Next command", "下一条命令"),
            // **"in this pane" since 2026-08-22**, and the row's scope is why:
            // the capsule has two hosts now (§7.7 ②) and a title naming one of
            // them would be a shortcut page telling a reader the key does not
            // work where it does. It names the surface both hosts are — a pane.
            Self::ShortcutOpenSearch => pick(lang, "Find in this pane", "在这个窗格里查找"),
            Self::ShortcutNextMatch => pick(lang, "Next match", "下一处匹配"),
            Self::ShortcutPrevMatch => pick(lang, "Previous match", "上一处匹配"),
            Self::ShortcutSummonPip1 => pick(lang, "Summon PiP slot 1", "唤出 PiP 槽位 1"),
            Self::ShortcutSummonPip2 => pick(lang, "Summon PiP slot 2", "唤出 PiP 槽位 2"),
            Self::ShortcutSummonPip3 => pick(lang, "Summon PiP slot 3", "唤出 PiP 槽位 3"),
            Self::ShortcutSummonPip4 => pick(lang, "Summon PiP slot 4", "唤出 PiP 槽位 4"),
            Self::ShortcutFamilyGotoTab => pick(lang, "Go to tab 1–9", "转到标签 1–9"),
            Self::ShortcutFamilySummonPip => pick(lang, "Summon PiP slot 1–4", "唤出 PiP 槽位 1–4"),
            Self::ShortcutScopePreview => pick(lang, "In a preview", "在预览里"),
            Self::ShortcutScopeTerminalPrimary => {
                pick(lang, "On a terminal's own scrollback", "在终端自己的回滚里")
            }
            Self::ShortcutScopeSearchOpen => pick(lang, "While the search is open", "查找打开时"),
            Self::ShortcutNotePending => pick(
                lang,
                "Bound; the verb behind it is still to come",
                "已绑定；它背后的功能还没有到",
            ),
            Self::ShortcutNoteOnePerMember => pick(lang, "One chord for each", "每一个各有一组键"),
            Self::ShortcutNoteNoneAssigned => pick(
                lang,
                "One chord for each; none is set yet",
                "每一个各有一组键；目前一个都没设",
            ),
            Self::ShortcutNoteSomeUnassigned => pick(
                lang,
                "One chord for each; some are not set yet",
                "每一个各有一组键；有些还没设",
            ),
            Self::ShortcutUnbound => pick(lang, "Not set", "未设置"),
            Self::ShortcutReservedMoveFocus => {
                pick(lang, "Move the focus between panes", "在窗格之间移动焦点")
            }
            Self::ShortcutReservedResizePane => pick(lang, "Resize a pane", "调整窗格大小"),
            Self::ShortcutReservedAltArrow => pick(
                lang,
                "Reserved — readline reads Alt+arrow as word movement",
                "保留 —— readline 把 Alt+方向键读作按词移动",
            ),
            Self::ShortcutHintAltGrZone => pick(
                lang,
                "Ctrl+Alt is reserved for AltGr keyboards",
                "Ctrl+Alt 留给 AltGr 键盘",
            ),
            Self::ShortcutHintShellControlLetter => pick(
                lang,
                "Ctrl+letter belongs to the shell",
                "Ctrl+字母属于 shell",
            ),

            // ── the Git page ───────────────────────────────────────────────
            Self::GitNotARepository => pick(lang, "Not a git repository", "这里不是 git 仓库"),
            Self::GitReading => pick(lang, "Reading the repository…", "正在读取仓库…"),
            Self::GitToastTitle => pick(lang, "Git", "Git"),
            Self::GitUnborn => pick(lang, "no commits yet", "还没有提交"),
            Self::GitDetached => pick(lang, "detached HEAD", "HEAD 已分离"),
            Self::GitLoadMore => pick(lang, "Load more", "加载更多"),
            Self::GitCommitNoFiles => pick(
                lang,
                "No files against the first parent",
                "对照第一个父提交没有文件",
            ),
            Self::GitGroupStaged => pick(lang, "STAGED", "已暂存"),
            Self::GitGroupChanges => pick(lang, "CHANGES", "已改动"),
            Self::GitGroupUntracked => pick(lang, "UNTRACKED", "未跟踪"),
            Self::GitGroupStagedTip => pick(
                lang,
                "Packed for the next commit — 'git commit' ships exactly these",
                "已为下一次提交打包 —— 'git commit' 提交的正是这些",
            ),
            Self::GitGroupChangesTip => pick(lang, "Edited but not packed yet", "已改，但还没打包"),
            Self::GitGroupUntrackedTip => pick(
                lang,
                "Not in the repository yet — git is not watching these",
                "还不在仓库里 —— git 没有盯着这些",
            ),
            Self::GitBranchesHeading => pick(lang, "BRANCHES", "分支"),
            Self::GitRemotesHeading => pick(lang, "REMOTES", "远程"),
            Self::GitRemotesTipShut => pick(
                lang,
                "Branches on remotes — click to show them",
                "远程上的分支 —— 点一下展开",
            ),
            Self::GitRemotesTipOpen => pick(
                lang,
                "Branches on remotes — click to fold them away",
                "远程上的分支 —— 点一下收起",
            ),
            Self::GitCommitsHeading => pick(lang, "COMMITS", "提交"),
            Self::GitRefreshTip => pick(lang, "Read the repository again", "再读一次仓库"),
            Self::GitActStage => pick(lang, "Stage", "暂存"),
            Self::GitActUnstage => pick(lang, "Unstage", "取消暂存"),
            Self::GitActDeleteFile => pick(lang, "Delete this file", "删除这个文件"),
            Self::GitActDiscardChanges => pick(lang, "Discard changes", "丢弃改动"),
            Self::GitActStageAll => pick(lang, "Stage all", "全部暂存"),
            Self::GitActUnstageAll => pick(lang, "Unstage all", "全部取消暂存"),
            Self::GitActLoadMore => pick(lang, "Load fifty more commits", "再加载五十条提交"),
            Self::GitActOpenGraph => pick(lang, "Open the full commit graph", "打开完整的提交图"),
            Self::GitNoCommits => pick(lang, "No commits yet", "还没有提交"),
            Self::GitFaultTimedOut => pick(
                lang,
                "git did not answer and was stopped",
                "git 没有回答，已被停止",
            ),
            Self::GitWorkerStopped => pick(
                lang,
                "Git reading stopped; terminal input and output remain available",
                "git 读取已停止；终端的输入输出仍然可用",
            ),
            Self::GitNotFound => pick(
                lang,
                "git.exe was not found on this machine — install Git for Windows to use this page",
                "这台机器上找不到 git.exe —— 装上 Git for Windows 才能用这个页面",
            ),

            // ── the commit graph ───────────────────────────────────────────
            Self::GraphMetaParents => pick(lang, "parents: ", "父提交："),
            Self::GraphMetaCommittedBy => pick(lang, "committed by ", "提交者："),
            Self::GraphCompareWorkingTree => pick(lang, "working tree", "工作区"),
            Self::GraphHeadingGraph => pick(lang, "GRAPH", "图"),
            Self::GraphHeadingDescription => pick(lang, "DESCRIPTION", "说明"),
            Self::GraphHeadingAuthor => pick(lang, "AUTHOR", "作者"),
            Self::GraphHeadingDate => pick(lang, "DATE", "日期"),
            Self::GraphHeadingCommit => pick(lang, "COMMIT", "提交"),
            Self::GraphUncommitted => pick(lang, "Uncommitted Changes", "未提交的改动"),
            Self::GraphUncommittedTime => pick(lang, "now", "现在"),
            Self::GraphSearchPlaceholder => pick(lang, "Search commits", "搜索提交"),
            Self::GraphSearchNone => pick(lang, "no matches", "没有匹配"),
            Self::GraphFilterAll => pick(lang, "All branches", "全部分支"),
            Self::GraphToolFilterTip => pick(
                lang,
                "Which branches this graph is of",
                "这张图画的是哪些分支",
            ),
            Self::GraphToolSearchTip => pick(
                lang,
                "Search commits by message, author or hash",
                "按信息、作者或哈希搜索提交",
            ),
            Self::GraphToolSearchClearTip => pick(lang, "Clear the search", "清空搜索"),
            Self::GraphFileBinary => pick(
                lang,
                "Binary — git has no lines to count here",
                "二进制 —— git 在这里没有行可数",
            ),
            Self::GraphMergeCommit => pick(
                lang,
                "Merge commit — another branch's history joins here",
                "合并提交 —— 另一个分支的历史在这里并入",
            ),
            Self::GraphDoubleClickCheckout => pick(
                lang,
                "Double-click to check this commit out",
                "双击检出这个提交",
            ),
            Self::GraphClickToList => pick(lang, "Click to list them", "点一下列出它们"),

            // ── the command rail's glance card ─────────────────────────────
            Self::RailPeekEmptyCommand => pick(lang, "command", "命令"),
            Self::RailPeekEmptyLine => pick(lang, "line", "行"),

            // ── the search capsule ─────────────────────────────────────────
            Self::SearchPlaceholder => pick(lang, "Find", "查找"),
            Self::SearchTipCase => pick(lang, "Match case", "区分大小写"),
            Self::SearchTipWord => pick(lang, "Whole word", "全词匹配"),
            Self::SearchTipRegex => pick(lang, "Regular expression", "正则表达式"),
            Self::SearchTipPrevious => pick(
                lang,
                "Previous match (Shift+Enter)",
                "上一处匹配 (Shift+Enter)",
            ),
            Self::SearchTipNext => pick(lang, "Next match (Enter)", "下一处匹配 (Enter)"),
            Self::SearchTipClose => pick(lang, "Close (Esc)", "关闭 (Esc)"),

            // ── the terminal's right-click menu ────────────────────────────
            Self::TermMenuCopy => pick(lang, "Copy", "复制"),
            Self::TermMenuPaste => pick(lang, "Paste", "粘贴"),
            Self::TermMenuSelectAll => pick(lang, "Select all", "全选"),
            Self::TermMenuFind => pick(lang, "Find…", "查找…"),
            Self::TermMenuClearScreen => pick(lang, "Clear screen", "清屏"),
            Self::TermMenuClearScrollback => pick(lang, "Clear scrollback…", "清除回滚…"),
            Self::TermMenuShellAgain => pick(lang, "Restart shell…", "重启 shell…"),

            // ── the pane's `⌄` menu ────────────────────────────────────────
            Self::PaneMenuSplitWith => pick(lang, "Split with", "拆分并运行"),
            Self::PaneMenuNewInFolder => {
                pick(lang, "New terminal in folder…", "在文件夹里新建终端…")
            }
            Self::PaneMenuDuplicate => pick(lang, "Duplicate pane", "复制窗格"),
            Self::PaneMenuMoveToNewTab => pick(lang, "Move pane to new tab", "把窗格移到新标签"),
            Self::PaneMenuSplitCaption => pick(lang, "SPLIT", "拆分"),
            Self::ClosePane => pick(lang, "Close pane", "关闭窗格"),
            Self::PaneChevronTip => pick(lang, "Split and more", "拆分等操作"),

            // ── a git row's menu and the branch filter ─────────────────────
            Self::GitMenuCheckout => pick(lang, "Checkout", "检出"),
            Self::GitMenuCreateBranch => pick(lang, "Create branch here…", "在这里新建分支…"),
            Self::GitMenuCreateTag => pick(lang, "Add tag here…", "在这里加标签…"),
            Self::GitMenuRename => pick(lang, "Rename…", "重命名…"),
            Self::GitMenuDeleteTag => pick(lang, "Delete tag", "删除标签"),
            Self::GitMenuCheckoutTracking => {
                pick(lang, "Checkout as local branch", "检出为本地分支")
            }
            Self::GitMenuOpenDiff => pick(lang, "Open diff", "打开差异"),
            Self::MenuRevealInExplorer => pick(lang, "Reveal in Explorer", "在资源管理器中显示"),
            Self::GitMenuCopyHash => pick(lang, "Copy hash", "复制哈希"),
            Self::GitMenuCopySubject => pick(lang, "Copy subject", "复制标题"),
            Self::GitMenuCopyName => pick(lang, "Copy name", "复制名称"),
            Self::GitMenuCompareSelected => pick(lang, "Compare with selected", "与选中的比较"),
            Self::GitMenuCompareWorking => pick(lang, "Compare with working tree", "与工作区比较"),
            Self::GitFilterShowRemotes => pick(lang, "Show remote branches", "显示远程分支"),
            Self::GitFilterShowTags => pick(lang, "Show tags", "显示标签"),
            Self::GitPromptBranchName => pick(lang, "Branch name", "分支名"),
            Self::GitPromptTagName => pick(lang, "Tag name", "标签名"),
            Self::GitPromptNewName => pick(lang, "New name", "新名称"),

            // ── the confirmation gate ──────────────────────────────────────
            Self::GateDiscard => pick(lang, "Discard", "放弃"),
            Self::GateCancel => pick(lang, "Cancel", "取消"),
            Self::GateDelete => pick(lang, "Delete", "删除"),
            Self::GateClear => pick(lang, "Clear", "清除"),
            Self::GateTitleUnsaved => pick(lang, "Discard unsaved changes?", "放弃未保存的改动？"),
            Self::GateTitleGitDiscard => pick(lang, "Discard changes?", "放弃改动？"),
            Self::GateTitleGitDelete => pick(lang, "Delete this file?", "删除这个文件？"),
            Self::GateTitleGitDeleteBranch => pick(lang, "Delete this branch?", "删除这个分支？"),
            Self::GateTitleGitDeleteTag => pick(lang, "Delete this tag?", "删除这个标签？"),
            Self::GateTitleClearScrollback => pick(lang, "Clear scrollback?", "清除回滚？"),

            // ── a ref name the field refuses ───────────────────────────────
            Self::RefNameEmpty => pick(lang, "A name is needed.", "需要一个名字。"),
            Self::RefNameSpace => pick(lang, "No spaces.", "不能有空格。"),
            Self::RefNameRange => pick(lang, "No `..`.", "不能有 `..`。"),
            Self::RefNameReserved => pick(lang, "None of ~ ^ : ? * [ \\", "不能有 ~ ^ : ? * [ \\"),
            Self::RefNameDash => pick(lang, "Cannot start with `-`.", "不能以 `-` 开头。"),
            Self::RefNameLock => pick(lang, "Cannot end with `.lock`.", "不能以 `.lock` 结尾。"),
            Self::RefNameShape => pick(
                lang,
                "git will not accept this name.",
                "git 不会接受这个名字。",
            ),

            // ── the file peek card and the diff document ───────────────────
            Self::PeekFoot => pick(
                lang,
                "Enter / double-click opens the preview pane",
                "Enter / 双击打开预览窗格",
            ),
            Self::PeekUnknown => pick(
                lang,
                "No preview — binary or unrecognized type.",
                "无法预览 —— 二进制或无法识别的类型。",
            ),
            // A statement of what the row does. The card has no engine, so it
            // cannot show the page; what it can do is stop saying the opposite.
            Self::PeekOpensAsPage => pick(lang, "Opens as a page.", "打开为页面。"),

            // ── quitting (multiwindow slice E2) ────────────────────────────
            Self::ShortcutQuit => pick(lang, "Quit", "退出"),
            Self::QuitTitle => pick(
                lang,
                "Save changes before quitting?",
                "退出前保存这些改动？",
            ),
            // **`all`, on both of the exit card's affirmative buttons** (user
            // ruling 2026-08-25, B1). The card lists files, one per line; a
            // button reading `Save` over four of them leaves a reader to work
            // out whether it means all four or the one they are looking at.
            Self::QuitSave => pick(lang, "Save all", "全部保存"),
            Self::QuitSessionNotWritten => pick(
                lang,
                "session.json could not be written. Nothing was closed.",
                "session.json 写入失败。没有关闭任何窗口。",
            ),

            // ── the preview pane's lock (§7.7 ⑧) ───────────────────────────
            Self::PreviewLock => pick(
                lang,
                "Lock this pane — what opens next opens in a new preview",
                "锁定此窗格 —— 接下来打开的内容会开在新的预览里",
            ),
            Self::PreviewUnlock => pick(
                lang,
                "Unlock — this pane becomes the reusable preview again",
                "解锁 —— 此窗格重新成为可复用的预览",
            ),
            // ── moving a pane out of this window (multiwindow slice F1c) ───
            Self::PaneMenuMoveToNewWindow => {
                pick(lang, "Move pane to new window", "把窗格移到新窗口")
            }
            Self::MoveRefusedGone => pick(
                lang,
                "That tab or that window closed before the move. Nothing moved.",
                "那个 tab 或那扇窗在移动之前已经关了。什么都没有移动。",
            ),
            Self::MoveRefusedAlreadyThere => pick(
                lang,
                "The tab is already in that window.",
                "这个 tab 已经在那扇窗里了。",
            ),
            Self::MoveRefusedPaneIsNowATab => pick(
                lang,
                "The pane is a tab of this window.",
                "这个窗格现在是本窗的一个 tab。",
            ),

            // The verb, because the verb is what separates this row from
            // `ShortcutWebAddress` two lines above it on the same page.
            Self::ShortcutWindowAddress => pick(lang, "Open address", "打开地址"),

            // ── single-pane zoom (§7.1.6l) ─────────────────────────────────
            //
            // Both say what the press does and stop, per the copy ruling of
            // 2026-08-17: no first person, no advice, no explaining that the
            // other panes are still there — the tree is not a thing this window
            // has to reassure anybody about.
            Self::PaneMenuZoom => pick(lang, "Zoom pane", "放大窗格"),
            Self::PaneMenuRestore => pick(lang, "Restore pane", "还原窗格"),

            // ── the hint a held modifier raises (§7.1.5e′) ─────────────────
            //
            // The copy ruling of 2026-08-17 applies here more than anywhere: the
            // sentence states two facts and stops. No first person, no advice,
            // and in particular no「让你不用记」— what a reader does with the
            // list is not this window's to narrate.
            Self::RowKeyHints => pick(lang, "Shortcut hints", "快捷键提示"),
            Self::DescKeyHints => pick(
                lang,
                "Holding a modifier for a moment lists the shortcuts it starts. \
                 The card never takes a key.",
                "按住修饰键片刻,列出以它开头的快捷键。这张卡片不吃任何按键。",
            ),
            // ── the tree row's menu, completed (user ruling 2026-08-25) ────
            //
            // `default app` is Windows' own phrase for the association it means,
            // and 「默认应用」is the phrase Windows' own Chinese settings use for
            // the same page — so neither column is this window inventing a word
            // for something the machine already names.
            Self::FileMenuOpenWith => pick(lang, "Open with default app", "用默认应用打开"),
            Self::FolderMenuExpand => pick(lang, "Expand", "展开"),
            Self::FolderMenuCollapse => pick(lang, "Collapse", "折叠"),
            Self::FolderMenuNewTerminal => pick(lang, "New terminal here", "在这里新建终端"),

            Self::GitDocumentEmpty => pick(lang, "No changes to show", "没有可显示的改动"),

            // ── a drag's landing caption ───────────────────────────────────
            Self::DragOpenInPreview => pick(lang, "Open in this preview", "在这个预览里打开"),
            Self::DragRootTreeHere => pick(lang, "Root this tree here", "把这棵树的根设到这里"),

            // ── the Terminal page's Scrollback row ─────────────────────────
            //
            // 「回滚」is the word this product already uses for the same thing
            // in the pane menu (`TermMenuClearScrollback`) and in the gate that
            // asks before emptying it; a second word here would make the row
            // and the verb look like two features.
            Self::RowScrollback => pick(lang, "Scrollback", "回滚"),
            // One sentence for the whole picker, the same constraint
            // `DescBlockMaxHeight` is written under: a description is a
            // `&'static str` and does not read the value. It says *per pane*
            // and it says *oldest first*, which are the two things the numerals
            // beside it do not.
            Self::DescScrollback => pick(
                lang,
                "Each pane keeps this many lines; the oldest go first",
                "每个窗格保留这么多行,最旧的先走",
            ),
            // 「自动折行」and not 「换行」: 「换行」is what a newline in the
            // output is, and this row is about what the pane does to a line the
            // program never broke. One product does not use one word for both.
            Self::RowLineWrapping => pick(lang, "Line wrapping", "自动折行"),
            // The two ends of this row's picker, said in two sentences rather
            // than one — `DescScrollback`'s constraint lifted, because
            // `SettingsRow::description` takes the values and a boolean row has
            // only two states to write out. The mock-up varied this line first
            // (2561, 7897) and this is that.
            Self::DescLineWrappingOn => pick(
                lang,
                "Long lines fold at the pane's edge.",
                "长行在窗格边缘折行。",
            ),
            // Two clauses: what happens to the line, and how to follow it. The
            // second is the row's whole reason for varying — see the variant.
            // It states the two ways and stops (copy ruling, 2026-08-17).
            Self::DescLineWrappingOff => pick(
                lang,
                "Long lines run on and the pane scrolls sideways: Shift+wheel, or the bar along the pane's foot.",
                "长行一直延伸,窗格横向滚动:Shift+滚轮,或窗格底边那条横条。",
            ),
            // 「窗口」and not 「窗」: the settings page already says 「窗口」in
            // `CloseWindow`, and one product does not have two words for the
            // thing every one of its chords is scoped to.
            Self::ShortcutNewWindow => pick(lang, "New window", "新建窗口"),

            // ── standing somewhere else ────────────────────────────────────
            Self::GateTitleGitDetach => pick(lang, "Stand on this commit?", "站到这个提交上？"),
            Self::GateTitleGitDirtyCheckout => pick(lang, "Move the working tree?", "移动工作区？"),
            Self::GateCheckout => pick(lang, "Checkout", "检出"),
            Self::GraphToolLeaveDetachedTip => pick(
                lang,
                "HEAD is on no branch — stand on one again",
                "HEAD 不在任何分支上 —— 重新站到一条分支上",
            ),
            // The trailing space is part of it: the branch's own name is
            // appended, and 「回到」takes no space before a name.
            Self::GraphLeaveDetached => pick(lang, "Back to ", "回到 "),
            // 「聚焦」rather than 「专注」: the word is about where the window's
            // attention is pointed, which is the same sense the pane head's
            // focus already carries in this product, and 「专注」would read as a
            // do-not-disturb mode this is not.
            // **`Cards`, and not `Focus mode`** (user ruling 2026-08-25). The
            // old name described an intention — that the window was for
            // concentrating on one thing — and the surface it turns on describes
            // itself: a column of cards, one per tab. A reader who has seen the
            // column once knows what `Cards` means, and a reader who has not is
            // told by the sentence below. 「卡片」is the word the column's own
            // sentence already uses for the things in it, so the row and what it
            // makes are now one word in both languages.
            //
            // The rename is of the name only. `LayoutMode::Focus`,
            // `solve_focused` and the `focus-mode` binding id are identifiers,
            // and an identifier is not something a reader meets.
            Self::RowFocusMode => pick(lang, "Cards", "卡片"),
            // Three clauses, in the order a reader meets them: what replaces the
            // tab strip, what happens to the tab you pick, and the other door.
            // It states facts and names the chord — no persuasion, and no "we".
            // The two pane-level doors it used to name were withdrawn on
            // 2026-08-19, and the sentence lost them the same day: a row that
            // advertises a gesture the build does not have is worse than a row
            // that says less.
            Self::DescFocusMode => pick(
                lang,
                "The tab strip becomes a column of cards, one per tab; the tab you pick fills the window whole, splits and all. Ctrl+Shift+Z turns this same setting",
                "标签条变成一列卡片,一张卡一个标签;点中的那个标签整个占满窗口,分屏原样。Ctrl+Shift+Z 拨的是同一个开关",
            ),
            // 「最小对比度」is the term of art both WCAG's Chinese translations
            // and VS Code's own Chinese locale use for this quantity, so the row
            // is findable by the name a reader already has for it.
            Self::RowMinimumContrast => pick(lang, "Minimum contrast", "最小对比度"),
            // Three clauses, in the order a reader needs them: what it does,
            // what it will not touch, and the cost. The third clause is the one
            // that earns its place — every rung above `Off` changes a colour the
            // program named, and a reader who turns this on without knowing that
            // will file the result as a bug. It states the fact and stops.
            //
            // **Shortened to three lines** (2026-08-25, the wrap ruling's own
            // consequence — see `DescFocusCardHeight`). All three clauses are
            // still here; what went was the passive voice each of them was
            // wearing.
            Self::DescMinimumContrast => pick(
                lang,
                "Lightens or darkens terminal text to meet this ratio against its cell. Backgrounds never change, nor does anything outside the grid. Above Off it overrides a program's colours",
                "把终端文字提亮或压暗,直到与它所在格子的底色达到这个对比度。底色一律不动,网格以外的一切也不动。Off 以上的档位会覆盖程序指定的颜色",
            ),
            // 「通知」and not 「桌面通知」: Windows itself calls the surface
            // 「通知」in its own Settings, and the row's sentence says where it
            // lands. English keeps the plural for the same reason — the row is
            // about a kind of thing, not one of them.
            Self::RowNotifications => pick(lang, "Notifications", "通知"),
            // It states facts and names nothing the reader has to act on. The
            // second clause is the one that earns its place: without it the row
            // reads as "may this program interrupt me", and the answer to that
            // question is no while the pane is in front of you.
            Self::DescNotifications => pick(
                lang,
                "Programs that ask can put a message on the desktop. Nothing arrives while the pane is on screen in a focused window",
                "程序主动要求时,可以在桌面上放一条消息。窗格正在这扇窗里显示、而这扇窗又是聚焦的,就什么都不弹",
            ),
            // ── the turn-end lane ──────────────────────────────────────────
            // 「回合结束」and not 「Agent 说完了」: the row is about a moment,
            // and the moment is the one an agent's own hooks, bells and
            // sequences all report. English keeps the same shape.
            Self::RowTurnEndNotifications => pick(lang, "Turn finished", "回合结束"),
            // Two clauses and no third. The first is what happens, ordered by
            // how far away the reader is; the second is the boundary — this row
            // owns the desktop and owns nothing inside the window, which is the
            // question a reader asks before switching it off.
            Self::DescTurnEndNotifications => pick(
                lang,
                "When an agent stops talking, flash this window's taskbar button, or put a message on the desktop if the window is minimised. The marks inside the window are unaffected",
                "Agent 说完一个回合时,闪这扇窗的任务栏按钮;窗最小化了就在桌面上放一条消息。窗内的记号不受这一行影响",
            ),
            Self::ToastTurnFinished => pick(lang, "Turn finished", "回合结束"),
            Self::ToastWaitingForYou => pick(lang, "Waiting for you", "正在等你回答"),
            Self::NotifyRefusedTitle => {
                pick(lang, "Desktop notification refused", "桌面通知被拒绝")
            }
            // What was lost, then what was not. No advice: there is nothing here
            // a reader can press, and the platform's own sentence follows.
            Self::NotifyRefusedBody => pick(
                lang,
                "Windows would not take it. The marks inside this window are unaffected.",
                "Windows 没有接收这条通知。窗内的记号不受影响。",
            ),
            // ── the PowerShell integration notice ──────────────────────────
            // Two sentences and no third: what is missing, and what it is used
            // for. Not why it is opt-in, not what the reader is being asked to
            // trust us with — the strip has two verbs and they say the rest.
            Self::PowerShellNoticeBody => pick(
                lang,
                "PowerShell integration is not installed. Folio uses it for command marks and status.",
                "PowerShell 整合未安装。Folio 用它来标记命令与显示状态。",
            ),
            Self::PowerShellNoticeAdd => pick(lang, "Add to $PROFILE", "加进 $PROFILE"),
            Self::PowerShellNoticeNever => pick(lang, "Don't show again", "不再提示"),
            // "Takes effect in a new shell" and not "restart to apply": the
            // second is an instruction, and there is a verb beside this sentence
            // that carries out the instruction. This states when the line starts
            // working, which is also true of the shell the reader opens tomorrow.
            Self::PowerShellNoticeAdded => pick(
                lang,
                "Added to $PROFILE. Takes effect in a new shell.",
                "已加进 $PROFILE。新开的 shell 生效。",
            ),
            Self::RowPowerShellOffer => pick(
                lang,
                "Offer PowerShell integration",
                "提示安装 PowerShell 整合",
            ),
            Self::DescPowerShellOffer => pick(
                lang,
                "A PowerShell pane whose $PROFILE does not load folio.ps1 shows one line offering to add it. Off stops the offer; a $PROFILE that already loads it is never asked about",
                "PowerShell 窗格的 $PROFILE 没有加载 folio.ps1 时,在那个窗格里显示一行,提供加入的动作。关闭后不再提示;已经加载的 $PROFILE 本来就不会被问",
            ),
            Self::RowClaudeHooks => pick(lang, "Claude Code hooks", "Claude Code 钩子"),
            Self::DescClaudeHooks => pick(
                lang,
                "Adds hooks to your own ~/.claude/settings.json so Claude Code tells this window when it is waiting for you. Nothing is written into a folder or a repository",
                "向你自己的 ~/.claude/settings.json 写入 hooks,Claude Code 在等你回答时会告诉这扇窗。不往任何文件夹或仓库里写",
            ),
            Self::ClaudeHooksAddedToast => pick(
                lang,
                "Added to ~/.claude/settings.json. Takes effect in a new Claude Code session.",
                "已加进 ~/.claude/settings.json。新开的 Claude Code 会话生效。",
            ),
            Self::ClaudeHooksRemovedToast => pick(
                lang,
                "Removed from ~/.claude/settings.json",
                "已从 ~/.claude/settings.json 移除",
            ),
            Self::ClaudeHooksFailedToast => pick(
                lang,
                "Claude Code's settings were not changed",
                "Claude Code 的设置没有被改动",
            ),
            // 「卡片高度」rather than 「卡片身高」: the column's own word for the
            // thing is 卡片 and 高度 is what a settings row measures, which is the
            // pair every other quantity row here is built out of.
            // Named after the mode, so it moved when the mode's name did (user
            // ruling 2026-08-25): a row still saying `Focus card height`
            // directly under a row saying `Cards` would be this dialog holding
            // two names for one feature.
            Self::RowFocusCardHeight => pick(lang, "Card height", "卡片高度"),
            // Three clauses: what the number is spent on, how many rows each
            // rung buys at the default face, and where in a seat those rows are
            // taken from. The second earns its place because the reader is
            // choosing pixels and counting rows, and no picker of pixels can say
            // the conversion.
            //
            // The third is this row paying a debt that is not its own (user
            // ruling 2026-08-21, the day the aim moved onto `Alt`). A gesture
            // that can only be found by holding down a key nobody was told about
            // does not exist, and every other surface that could have named it
            // was ruled out by something already written: the column refuses
            // tooltips (§7.1.6b′, "the cards write the name out in full"), a
            // card is not a `⌄` menu's subject, and a new affordance on the
            // column is exactly what the 2026-08-20 ruling took off it. What is
            // left is the one row in this file that is already about a card's
            // picture — and "how tall the picture is" and "which rows it shows"
            // are the same reader's question one sentence apart.
            // **Shortened to three lines** (2026-08-25). The wrap ruling caps a
            // row's sentence at three and says in as many words that a fourth is
            // copy to shorten; `no_settings_sentence_needs_a_fourth_line` found
            // this one, which was the longest in the dialog. Every fact it
            // carried is still here — what the number measures, how many rows
            // each rung buys at the default face, and that Alt+wheel scrolls the
            // picture a row at a time — said in fewer words.
            Self::DescFocusCardHeight => pick(
                lang,
                "How tall each card's picture of its tab stands. At the default face a lone pane holds 12 rows at 160, 20 at 240, 27 at 320. Alt+wheel over a seat scrolls it a row per notch",
                "每张卡片里那幅 tab 缩图的高度。默认字面下,单 pane 在 160 装得下 12 行、240 装 20 行、320 装 27 行。在格子上 Alt+滚轮滚动这幅图,一格一行",
            ),
            // A fact and no more: where the page goes. Not "open this page in
            // your default browser" — "your default" is the machine's setting
            // and not something this button chose — and not a name, because
            // which browser answers is the machine's business.
            Self::PreviewOpenInBrowser => pick(lang, "Open in browser", "在浏览器中打开"),
            Self::ShortcutWebAddress => pick(lang, "Address", "地址"),
            Self::ShortcutWebDevTools => pick(lang, "Developer tools", "开发者工具"),
            Self::ShortcutCloseSearch => pick(lang, "Close search", "关闭搜索"),
            // "On a page" and not "In the web preview": every other tag in this
            // family names where the keyboard is standing, and the reader of
            // this line is looking for the state their hands are in.
            Self::ShortcutScopeWebPage => pick(lang, "On a page", "键盘在网页里时"),
            // Names what is true of both hosts rather than listing them: a tag
            // reading "On a terminal's own scrollback or on a page" is a
            // sentence, and every other tag in this family is a place.
            Self::ShortcutScopeSearchHost => {
                pick(lang, "Where there is text to search", "有正文可查找的地方")
            }
            Self::PreviewWebBack => pick(lang, "Back", "后退"),
            Self::PreviewWebForward => pick(lang, "Forward", "前进"),
            Self::PreviewWebReload => pick(lang, "Reload", "重新加载"),
            Self::PreviewWebStop => pick(lang, "Stop", "停止"),
            // The product name stays in Latin in both columns because it is the
            // name of the thing to install, and a reader searching for it in a
            // list of installed programs will read it there in Latin.
            Self::WebFailRuntimeSay => pick(
                lang,
                "Microsoft Edge WebView2 Runtime is not installed.",
                "未安装 Microsoft Edge WebView2 Runtime。",
            ),
            Self::WebFailRuntimeVerb => pick(lang, "Download the runtime", "下载运行时"),
            // States what happened and stops. It does not guess why — the code
            // on the line below is the only thing this window actually knows —
            // and it does not apologise.
            Self::WebFailEngineSay => {
                pick(lang, "The web engine did not start.", "网页引擎没有启动。")
            }
            Self::WebFailEngineVerb => pick(lang, "Retry", "重试"),
            Self::WebFailCrashSay => pick(
                lang,
                "This page stopped running. Its render process exited.",
                "这个页面停止运行了。它的渲染进程已退出。",
            ),
            Self::WebFailBlockedSay => pick(
                lang,
                "This address does not open in a preview.",
                "这个地址不在预览中打开。",
            ),
            Self::WebFailBlockedVerb => pick(lang, "Copy address", "复制地址"),
            Self::WebFailDownloadSay => pick(
                lang,
                "This download cannot be handed to your browser. The request carried data a plain link cannot replay.",
                "这次下载没法交给浏览器。请求里带着普通链接重放不出来的东西。",
            ),
            Self::WebFailDownloadVerb => pick(
                lang,
                "Open this page in your browser",
                "在浏览器中打开这个页面",
            ),
            // 「重命名」and not 「改名」: it is the word Explorer's own Chinese
            // uses on this exact row, so a reader meets it here already knowing
            // what it does.
            Self::FileMenuRename => pick(lang, "Rename", "重命名"),
            Self::GateDiscardAll => pick(lang, "Discard all", "全部放弃"),
            // 「移到窗口」and not 「移动到窗口」: the two rows above it read
            // 「移到新标签」/「移到新窗口」, and a third exit spelling the same
            // verb differently would read as a different verb.
            Self::PaneMenuMoveToWindow => pick(lang, "Move to window", "移到窗口"),
            // ── the Agents page ────────────────────────────────────────────
            // 「Agents」stays in Latin in both columns and it is the honest
            // reading rather than a gap in the table: what the page is about is
            // the class of program that calls itself an agent, and every one of
            // them — Claude Code, codex — is written this way in Chinese prose
            // too. Upper-cased at the source for the heading, as every other page
            // is, and lower-cased by hand for the rail.
            Self::CategoryAgents => pick(lang, "AGENTS", "AGENTS"),
            Self::NavAgents => pick(lang, "Agents", "Agents"),
            // 「通知程序」and not 「通知」: what the row installs is a *program*
            // codex runs, and the row three lines up already owns the word for
            // the thing a program puts on your desktop.
            Self::RowCodexNotify => pick(lang, "Codex notify", "Codex 通知程序"),
            Self::DescCodexNotify => pick(
                lang,
                "Adds a notify program to your own ~/.codex/config.toml so codex says when a turn has ended. It does not report a codex waiting for you, and writes nothing into a repository",
                "向你自己的 ~/.codex/config.toml 写入 notify 程序,codex 结束一个回合时会说一声。它不报告 codex 在等你回答,也不往仓库里写",
            ),
            Self::CodexNotifyAddedToast => pick(
                lang,
                "Added to ~/.codex/config.toml. Takes effect in a new codex session.",
                "已加进 ~/.codex/config.toml。新开的 codex 会话生效。",
            ),
            Self::CodexNotifyRemovedToast => pick(
                lang,
                "Removed from ~/.codex/config.toml",
                "已从 ~/.codex/config.toml 移除",
            ),
            Self::CodexNotifyFailedToast => pick(
                lang,
                "codex's configuration was not changed",
                "codex 的配置没有被改动",
            ),
            Self::RowSearchEngine => pick(lang, "Search engine", "搜索引擎"),
            // Names the field rather than the feature, because that is where a
            // reader meets it: they typed a word into the place an address goes
            // and something happened. It says nothing about which engine is
            // better — the three names are on the picker beside this line.
            Self::DescSearchEngine => pick(
                lang,
                "Where a web preview's address field sends text that is not an address",
                "在网页预览的地址栏里打了一段不是地址的文字时,交给哪个搜索引擎",
            ),
        }
    }

    /// Every entry, for the tests that have to walk the whole table.
    ///
    /// Written out rather than derived, and that is the point: a variant added
    /// without a line here fails
    /// `every_entry_of_the_table_is_listed_once_and_reads_in_both_languages`,
    /// which is the only mechanism that keeps a new string from shipping with
    /// one column filled in.
    ///
    /// Test-only, like `SettingsValues::sample`: nothing the window does needs
    /// the list, and a constant the product carried only so that a test could
    /// read it would be shipped weight.
    #[cfg(test)]
    pub const ALL: [Self; 495] = [
        Self::Settings,
        Self::ToggleSidebar,
        Self::Minimize,
        Self::Maximize,
        Self::CloseWindow,
        Self::RailTabs,
        Self::RailNewTab,
        Self::ChooseProfile,
        Self::Pin,
        Self::Unpin,
        Self::NameSourceManual,
        Self::NameSourceProgram,
        Self::NameSourceCwd,
        Self::TabTipPinned,
        Self::MarkWorking,
        Self::MarkWorkingIndeterminate,
        Self::SeatTerminal,
        Self::SeatFiles,
        Self::SeatPreview,
        Self::SeatUnavailable,
        Self::PlaceholderSeatNotice,
        Self::CategoryGeneral,
        Self::CategoryAppearance,
        Self::CategoryTerminal,
        Self::CategoryRenderedBlocks,
        Self::CategoryShortcuts,
        Self::NavGeneral,
        Self::NavAppearance,
        Self::NavTerminal,
        Self::NavRenderedBlocks,
        Self::NavShortcuts,
        Self::RowTheme,
        Self::RowCursor,
        Self::RowFormulas,
        Self::RowInlineFormulas,
        Self::RowGitPanel,
        Self::RowContextMenu,
        Self::RowTabLayout,
        Self::RowSidebar,
        Self::RowSplitDirection,
        Self::RowDefaultProfile,
        Self::RowLanguage,
        Self::RowTerminalFont,
        Self::RowFontSize,
        Self::RowPsReadLine,
        Self::DescTheme,
        Self::DescCursor,
        Self::DescFormulas,
        Self::DescInlineFormulas,
        Self::DescGitPanel,
        Self::DescContextMenu,
        Self::DescTabLayout,
        Self::DescSidebar,
        Self::DescSplitDirection,
        Self::DescDefaultProfile,
        Self::DescLanguage,
        Self::DescTerminalFont,
        Self::DescFontSize,
        Self::OptionSystem,
        Self::OptionLight,
        Self::OptionDark,
        Self::OptionCursorBar,
        Self::OptionCursorBlock,
        Self::OptionCursorUnderline,
        Self::OptionHorizontal,
        Self::OptionVertical,
        Self::OptionOn,
        Self::OptionOff,
        Self::OptionExpanded,
        Self::OptionIcons,
        Self::OptionSplitAuto,
        Self::OptionSplitRight,
        Self::OptionSplitDown,
        Self::PsReadLineProbing,
        Self::PsReadLineRowGone,
        Self::PsReadLineInviteTitle,
        Self::PsReadLineInstall,
        Self::PsReadLineNotNow,
        Self::PsReadLineRemovedToast,
        Self::ContextMenuVerb,
        Self::ContextMenuAddedToast,
        Self::ContextMenuRemovedToast,
        Self::ContextMenuNoExecutable,
        Self::ProfileHintDefault,
        Self::ProfileHintUnavailable,
        Self::ProfileHintCurrent,
        Self::ProfileFilesPane,
        Self::ProfileFilesPaneHint,
        Self::ProfileRecentSection,
        Self::RootSection,
        Self::RootBrowse,
        Self::RootNoteHome,
        Self::RootNoteTerminal,
        Self::RootNoteParent,
        Self::PinnedSection,
        Self::FileMenuOpenPreview,
        Self::FileMenuOpenDefaultApp,
        Self::PreviewRailOpen,
        Self::PreviewRailOpenTip,
        Self::PreviewCopyAddress,
        Self::PreviewFlipToSource,
        Self::PreviewFlipToRendered,
        Self::FileMenuCopyPath,
        Self::FileMenuInsertPath,
        Self::FilesLoading,
        Self::FilesEmpty,
        Self::FilesUnrooted,
        Self::FilesPermissionDenied,
        Self::FilesNotFound,
        Self::FilesUnreadable,
        Self::FilesRevealed,
        Self::PreviewEmptyState,
        Self::PreviewRefusalType,
        Self::PreviewRefusalBinary,
        Self::PreviewRefusalNetworkPath,
        Self::PreviewRefusalPermissionDenied,
        Self::PreviewRefusalNotFound,
        Self::PreviewRefusalUnreadable,
        Self::PreviewOpenExternally,
        Self::PreviewOpened,
        Self::PreviewTruncated,
        Self::PreviewSaved,
        Self::PreviewConflict,
        Self::PreviewNothingToSave,
        Self::PreviewFailedImageLoad,
        Self::PreviewFailedImageWorker,
        Self::PreviewFailedSeatTooSmall,
        Self::FloatDock,
        Self::FloatTriggerTip,
        Self::RestoreTitle,
        Self::RestoreSub,
        Self::RestoreDecline,
        Self::RestoreAccept,
        Self::FilesProgramRefused,
        Self::MathWorkerStopped,
        Self::FilesWorkerStopped,
        Self::PreviewWorkerStopped,
        Self::HyperlinkBlockedSuffix,
        Self::HyperlinkBlocked,
        Self::DragSwapPanes,
        Self::DragReplacePane,
        Self::RowLightScheme,
        Self::RowDarkScheme,
        Self::DescLightScheme,
        Self::DescDarkScheme,
        Self::SchemeFileSkipped,
        Self::RowBackgroundImage,
        Self::RowImageFit,
        Self::RowImageOpacity,
        Self::RowBackgroundOpacity,
        Self::RowAcrylic,
        Self::RowAlwaysOnTop,
        Self::DescBackgroundImage,
        Self::DescImageFit,
        Self::DescImageOpacity,
        Self::DescBackgroundOpacity,
        Self::DescAcrylic,
        Self::DescAlwaysOnTop,
        Self::DescAcrylicUnavailable,
        Self::DescBackgroundOpacityUnavailable,
        Self::OptionImageNone,
        Self::OptionImageChoose,
        Self::OptionFitStretch,
        Self::OptionFitFill,
        Self::OptionFitTile,
        Self::FilesViewFiles,
        Self::FilesViewGit,
        Self::AdvancedGroup,
        Self::ResetAdvanced,
        Self::AddScheme,
        Self::InstallFonts,
        Self::SchemeInUseBroken,
        Self::SchemeInUseGone,
        Self::SchemeDeleted,
        Self::BackgroundPictureRefused,
        Self::NavProfiles,
        Self::CategoryProfiles,
        Self::ProfilesDuplicate,
        Self::ProfilesBadgeDefault,
        Self::ProfilesBadgeHidden,
        Self::CapFull,
        Self::CapPowerShell,
        Self::CapWslBash,
        Self::CapCmd,
        Self::CapNone,
        Self::RowTables,
        Self::DescTables,
        Self::ProfilesEdit,
        Self::ProfilesHide,
        Self::ProfilesShow,
        Self::ProfilesDelete,
        Self::ProfilesSetDefault,
        Self::ProfilesAlreadyDefault,
        Self::ProfilesNew,
        Self::ProfilesRowName,
        Self::ProfilesRowNameDesc,
        Self::ProfilesRowProgram,
        Self::ProfilesRowProgramDesc,
        Self::ProfilesRowStartingDir,
        Self::ProfilesRowStartingDirDesc,
        Self::ProfilesRowColour,
        Self::ProfilesRowColourDesc,
        Self::ProfilesInherit,
        Self::ProfilesHome,
        Self::ProfilesChooseFolder,
        Self::ProfilesColourBlue,
        Self::ProfilesColourTeal,
        Self::ProfilesColourGreen,
        Self::ProfilesColourAmber,
        Self::ProfilesColourRed,
        Self::ProfilesColourMagenta,
        Self::ProfilesColourViolet,
        Self::ProfilesColourSlate,
        Self::ProfilesColourInherited,
        Self::ProfilesColourFixed,
        Self::ProfilesRowArgs,
        Self::ProfilesRowArgsDesc,
        Self::ProfilesRowEnv,
        Self::ProfilesRowEnvDesc,
        Self::ProfilesRowHyperlink,
        Self::ProfilesRowHyperlinkDesc,
        Self::ProfilesRowIntegration,
        Self::ProfilesEnvAdd,
        Self::ProfilesEnvName,
        Self::ProfilesEnvValue,
        Self::ProfilesAuto,
        Self::ProfilesOn,
        Self::ProfilesOff,
        Self::ProfilesRestoreAll,
        Self::ProfilesDeleteBtn,
        Self::ProfilesBrowse,
        Self::ProfilesCannotHideDefault,
        Self::ProfilesCannotHideFallback,
        Self::ProfilesNameBlank,
        Self::ProfilesNameTaken,
        Self::ProfilesUndo,
        Self::ProfilesIntegrationPowerShell,
        Self::ProfilesIntegrationBash,
        Self::ProfilesIntegrationCmd,
        Self::ProfilesIntegrationNone,
        Self::CapFullNoLinks,
        Self::CapPowerShellNoLinks,
        Self::CapWslBashNoLinks,
        Self::CapCmdNoLinks,
        Self::CapNoneLong,
        Self::CapNoneLongNoLinks,
        Self::RowBlockMaxHeight,
        Self::DescBlockMaxHeight,
        Self::OptionBlockHeightNone,
        Self::PsReadLineUpdate,
        Self::ShortcutGotoTab1,
        Self::ShortcutGotoTab2,
        Self::ShortcutGotoTab3,
        Self::ShortcutGotoTab4,
        Self::ShortcutGotoTab5,
        Self::ShortcutGotoTab6,
        Self::ShortcutGotoTab7,
        Self::ShortcutGotoTab8,
        Self::ShortcutGotoTab9,
        Self::ShortcutNextTab,
        Self::ShortcutPrevTab,
        Self::ShortcutReopenClosed,
        Self::ShortcutJumpAttention,
        Self::ShortcutCommandPalette,
        Self::ShortcutSplitHorizontal,
        Self::ShortcutSplitVertical,
        Self::ShortcutDuplicatePaneSplit,
        Self::ShortcutFilesPane,
        Self::ShortcutGitPage,
        Self::ShortcutSavePreview,
        Self::ShortcutPrevCommandMark,
        Self::ShortcutNextCommandMark,
        Self::ShortcutOpenSearch,
        Self::ShortcutNextMatch,
        Self::ShortcutPrevMatch,
        Self::ShortcutSummonPip1,
        Self::ShortcutSummonPip2,
        Self::ShortcutSummonPip3,
        Self::ShortcutSummonPip4,
        Self::ShortcutFamilyGotoTab,
        Self::ShortcutFamilySummonPip,
        Self::ShortcutScopePreview,
        Self::ShortcutScopeTerminalPrimary,
        Self::ShortcutScopeSearchOpen,
        Self::ShortcutNotePending,
        Self::ShortcutNoteOnePerMember,
        Self::ShortcutNoteNoneAssigned,
        Self::ShortcutNoteSomeUnassigned,
        Self::ShortcutUnbound,
        Self::ShortcutReservedMoveFocus,
        Self::ShortcutReservedResizePane,
        Self::ShortcutReservedAltArrow,
        Self::ShortcutHintAltGrZone,
        Self::ShortcutHintShellControlLetter,
        Self::GitNotARepository,
        Self::GitReading,
        Self::GitToastTitle,
        Self::GitUnborn,
        Self::GitDetached,
        Self::GitLoadMore,
        Self::GitCommitNoFiles,
        Self::GitGroupStaged,
        Self::GitGroupChanges,
        Self::GitGroupUntracked,
        Self::GitGroupStagedTip,
        Self::GitGroupChangesTip,
        Self::GitGroupUntrackedTip,
        Self::GitBranchesHeading,
        Self::GitRemotesHeading,
        Self::GitRemotesTipShut,
        Self::GitRemotesTipOpen,
        Self::GitCommitsHeading,
        Self::GitRefreshTip,
        Self::GitActStage,
        Self::GitActUnstage,
        Self::GitActDeleteFile,
        Self::GitActDiscardChanges,
        Self::GitActStageAll,
        Self::GitActUnstageAll,
        Self::GitActLoadMore,
        Self::GitActOpenGraph,
        Self::GitNoCommits,
        Self::GitFaultTimedOut,
        Self::GitWorkerStopped,
        Self::GitNotFound,
        Self::GraphMetaParents,
        Self::GraphMetaCommittedBy,
        Self::GraphCompareWorkingTree,
        Self::GraphHeadingGraph,
        Self::GraphHeadingDescription,
        Self::GraphHeadingAuthor,
        Self::GraphHeadingDate,
        Self::GraphHeadingCommit,
        Self::GraphUncommitted,
        Self::GraphUncommittedTime,
        Self::GraphSearchPlaceholder,
        Self::GraphSearchNone,
        Self::GraphFilterAll,
        Self::GraphToolFilterTip,
        Self::GraphToolSearchTip,
        Self::GraphToolSearchClearTip,
        Self::GraphFileBinary,
        Self::GraphMergeCommit,
        Self::GraphDoubleClickCheckout,
        Self::GraphClickToList,
        Self::RailPeekEmptyCommand,
        Self::RailPeekEmptyLine,
        Self::SearchPlaceholder,
        Self::SearchTipCase,
        Self::SearchTipWord,
        Self::SearchTipRegex,
        Self::SearchTipPrevious,
        Self::SearchTipNext,
        Self::SearchTipClose,
        Self::TermMenuCopy,
        Self::TermMenuPaste,
        Self::TermMenuSelectAll,
        Self::TermMenuFind,
        Self::TermMenuClearScreen,
        Self::TermMenuClearScrollback,
        Self::TermMenuShellAgain,
        Self::PaneMenuSplitWith,
        Self::PaneMenuNewInFolder,
        Self::PaneMenuDuplicate,
        Self::PaneMenuMoveToNewTab,
        Self::PaneMenuSplitCaption,
        Self::ClosePane,
        Self::PaneChevronTip,
        Self::GitMenuCheckout,
        Self::GitMenuCreateBranch,
        Self::GitMenuCreateTag,
        Self::GitMenuRename,
        Self::GitMenuDeleteTag,
        Self::GitMenuCheckoutTracking,
        Self::GitMenuOpenDiff,
        Self::MenuRevealInExplorer,
        Self::GitMenuCopyHash,
        Self::GitMenuCopySubject,
        Self::GitMenuCopyName,
        Self::GitMenuCompareSelected,
        Self::GitMenuCompareWorking,
        Self::GitFilterShowRemotes,
        Self::GitFilterShowTags,
        Self::GitPromptBranchName,
        Self::GitPromptTagName,
        Self::GitPromptNewName,
        Self::GateDiscard,
        Self::GateCancel,
        Self::GateDelete,
        Self::GateClear,
        Self::GateTitleUnsaved,
        Self::GateTitleGitDiscard,
        Self::GateTitleGitDelete,
        Self::GateTitleGitDeleteBranch,
        Self::GateTitleGitDeleteTag,
        Self::GateTitleClearScrollback,
        Self::RefNameEmpty,
        Self::RefNameSpace,
        Self::RefNameRange,
        Self::RefNameReserved,
        Self::RefNameDash,
        Self::RefNameLock,
        Self::RefNameShape,
        Self::PeekFoot,
        Self::PeekUnknown,
        Self::GitDocumentEmpty,
        Self::DragOpenInPreview,
        Self::DragRootTreeHere,
        Self::RowScrollback,
        Self::DescScrollback,
        Self::RowLineWrapping,
        Self::DescLineWrappingOn,
        Self::DescLineWrappingOff,
        Self::ShortcutNewWindow,
        Self::GateTitleGitDetach,
        Self::GateTitleGitDirtyCheckout,
        Self::GateCheckout,
        Self::GraphToolLeaveDetachedTip,
        Self::GraphLeaveDetached,
        Self::RowFocusMode,
        Self::DescFocusMode,
        Self::RowMinimumContrast,
        Self::DescMinimumContrast,
        Self::RowNotifications,
        Self::DescNotifications,
        Self::RowTurnEndNotifications,
        Self::DescTurnEndNotifications,
        Self::ToastTurnFinished,
        Self::ToastWaitingForYou,
        Self::NotifyRefusedTitle,
        Self::NotifyRefusedBody,
        Self::PowerShellNoticeBody,
        Self::PowerShellNoticeAdd,
        Self::PowerShellNoticeNever,
        Self::PowerShellNoticeAdded,
        Self::RowPowerShellOffer,
        Self::DescPowerShellOffer,
        Self::RowClaudeHooks,
        Self::DescClaudeHooks,
        Self::ClaudeHooksAddedToast,
        Self::ClaudeHooksRemovedToast,
        Self::ClaudeHooksFailedToast,
        Self::RowFocusCardHeight,
        Self::DescFocusCardHeight,
        Self::PreviewOpenInBrowser,
        Self::ShortcutWebAddress,
        Self::ShortcutWebDevTools,
        Self::ShortcutCloseSearch,
        Self::ShortcutScopeWebPage,
        Self::ShortcutScopeSearchHost,
        Self::PreviewWebBack,
        Self::PreviewWebForward,
        Self::PreviewWebReload,
        Self::PreviewWebStop,
        Self::WebFailRuntimeSay,
        Self::WebFailRuntimeVerb,
        Self::WebFailEngineSay,
        Self::WebFailEngineVerb,
        Self::WebFailCrashSay,
        Self::WebFailBlockedSay,
        Self::WebFailBlockedVerb,
        Self::WebFailDownloadSay,
        Self::WebFailDownloadVerb,
        Self::RowSearchEngine,
        Self::DescSearchEngine,
        Self::PeekOpensAsPage,
        Self::ShortcutQuit,
        Self::QuitTitle,
        Self::QuitSave,
        Self::QuitSessionNotWritten,
        Self::PreviewLock,
        Self::PreviewUnlock,
        Self::PaneMenuMoveToNewWindow,
        Self::MoveRefusedGone,
        Self::MoveRefusedAlreadyThere,
        Self::MoveRefusedPaneIsNowATab,
        Self::ShortcutWindowAddress,
        Self::PaneMenuZoom,
        Self::PaneMenuRestore,
        Self::RowKeyHints,
        Self::DescKeyHints,
        Self::FileMenuOpenWith,
        Self::FolderMenuExpand,
        Self::FolderMenuCollapse,
        Self::FolderMenuNewTerminal,
        Self::FileMenuRename,
        Self::GateDiscardAll,
        Self::PaneMenuMoveToWindow,
        Self::CategoryAgents,
        Self::NavAgents,
        Self::RowCodexNotify,
        Self::DescCodexNotify,
        Self::CodexNotifyAddedToast,
        Self::CodexNotifyRemovedToast,
        Self::CodexNotifyFailedToast,
    ];

    /// The entries whose two columns are allowed to be the same string.
    ///
    /// Listed by hand, one line per reason, which is what the two pins below
    /// promised when they were written: an entry that is *only* a name has
    /// nothing to translate, and translating it would be inventing a word the
    /// program is not called. Every entry not on this list must differ between
    /// the columns and must carry Chinese in the Chinese one.
    #[cfg(test)]
    const UNTRANSLATED: [Self; 4] = [
        // The class of program the page is about, and the word every one of them
        // calls itself — Claude Code, codex — in Chinese prose as much as in
        // English. A page named 「代理」 would be naming a proxy server (user
        // ruling 2026-08-25).
        Self::CategoryAgents,
        // The same word in the rail. A second entry and not a reuse of the
        // heading's, for `FilesViewFiles`'s reason: case is content in this
        // table, and the two columns are two literals.
        Self::NavAgents,
        // The program's name. It appears untranslated inside a dozen sentences
        // in this table already; a tab that named it in Chinese would be naming
        // something else.
        Self::FilesViewGit,
        // The same proper noun again, on the card a refused write raises.
        // A second entry and not a reuse of the tab's, for the reason
        // `FilesViewFiles` is not `SeatFiles`: shortening the segmented
        // control must not be a change to a toast.
        Self::GitToastTitle,
    ];
}

// ── the strings that carry a value ─────────────────────────────────────────
//
// A separate family rather than a `Text` variant with holes in it, because these
// return `String` and every one of the table's entries is `&'static str`. Mixing
// the two would make the whole table allocate to serve fifteen of its members.

/// The `Did not load` card's sentence (§7.7 ④), which names the host that was
/// asked and nothing else.
///
/// The host and not the whole URL: the URL is on the head, in full, three
/// centimetres above this card, and a sentence that repeated it would be the
/// window reading its own address field back to a reader who is looking at it.
/// What the sentence adds is *which name did not answer*, which is the half of
/// a URL that a connection failure is about.
#[must_use]
pub fn web_fail_did_not_respond(host: &str) -> String {
    match current() {
        Lang::English => format!("{host} did not respond."),
        Lang::Chinese => format!("{host} 没有响应。"),
    }
}

/// The `Blocked` card's sentence when the refused address named a scheme
/// (§7.7 ④). The scheme comes from `webnav::scheme_of` and from nowhere else —
/// a second reading of an address is a second answer about it.
///
/// The colon stays glued to the scheme in both columns because it is part of
/// the thing being named: `mailto` is a word and `mailto:` is an address's
/// first token, and it is the token a reader is looking at in their own field.
#[must_use]
pub fn web_fail_blocked_scheme(scheme: &str) -> String {
    match current() {
        Lang::English => format!("{scheme}: addresses do not open in a preview."),
        Lang::Chinese => format!("{scheme}: 开头的地址不在预览中打开。"),
    }
}

/// `title="New tab (${defaultProfile().title})"` — mock-up 4367 and 4369, worn
/// by the strip's `+` and the rail's alike.
///
/// The Chinese takes full-width brackets, which is the same decision the
/// parentheses were: a half-width `(` after a Chinese character reads as a typo
/// in a way it does not after a Latin one.
#[must_use]
pub fn new_tab_tip(profile_title: &str) -> String {
    match current() {
        Lang::English => format!("New tab ({profile_title})"),
        Lang::Chinese => format!("新建标签（{profile_title}）"),
    }
}

/// The invitation's body: what the machine has, what it costs today, what
/// installing writes and where, and how it comes back out.
///
/// Four facts and no persuasion. Both versions are parameters because the
/// module version this build ships is a Rust constant that `folio.ps1`'s own
/// gate reads too — a literal here would be the third place it is written, and
/// the one nothing would catch when it moved.
#[must_use]
pub fn psreadline_invite_body(found: &str, patched: &str, path: &str) -> String {
    match current() {
        Lang::English => format!(
            "Windows PowerShell ships with PSReadLine {found}. On that version the input line \
             lands in the wrong place when the window is resized, and the repair Folio sends \
             does nothing. Installing writes PSReadLine {patched} to {path}. It takes effect in \
             PowerShell sessions started after that, and Settings ▸ Terminal removes it."
        ),
        Lang::Chinese => format!(
            "Windows PowerShell 自带的 PSReadLine 是 {found}。在这个版本上，缩放窗口后输入行会\
             错位，Folio 发出的修复指令不起作用。安装会把 PSReadLine {patched} 写入 \
             {path}，对此后新开的 PowerShell 会话生效，并可在设置 ▸ 终端中移除。"
        ),
    }
}

/// The card a scheme file that would not parse raises: which file, and why.
///
/// Both halves are parameters because both come from outside the product — the
/// name is whatever the user called the file, and the reason is
/// `SchemeParseError`'s own sentence, which names the offending key. A card that
/// said only "a scheme could not be read" would leave a folder of eleven files
/// to be opened one at a time.
#[must_use]
pub fn scheme_file_skipped(file: &str, reason: &str) -> String {
    match current() {
        Lang::English => format!("{file} — {reason}"),
        Lang::Chinese => format!("{file} —— {reason}"),
    }
}

/// The card raised when a copy of the scheme in force has been written.
///
/// The file name is the parameter because it is the thing the reader has to be
/// able to find again, and it is not one this product chose: it carries whatever
/// [`crate::schemes::unique_custom`] had to fall back to when a copy of that
/// name already existed. The second clause is a fact about what happens next and
/// not an instruction — the file is already open in front of them.
#[must_use]
pub fn scheme_customised(file: &str) -> String {
    match current() {
        Lang::English => format!("{file} — saving it applies the colours"),
        Lang::Chinese => format!("{file} —— 保存后颜色即生效"),
    }
}

/// The card raised when the scheme in force stops parsing.
///
/// Same two halves as [`scheme_file_skipped`] plus the one thing the reader
/// cannot see for themselves: nothing on screen changed, so without this
/// sentence a broken save and a save with no visible effect read identically.
#[must_use]
pub fn scheme_in_use_broken(file: &str, reason: &str) -> String {
    match current() {
        Lang::English => format!("{file} — {reason}. The colours in force are unchanged."),
        Lang::Chinese => format!("{file} —— {reason}。当前颜色未变。"),
    }
}

/// The card raised when the scheme in force is no longer in the folder.
///
/// Both names are parameters because both are the user's: the one they chose and
/// the one that is now in force in its place.
#[must_use]
pub fn scheme_in_use_gone(name: &str, fallback: &str) -> String {
    match current() {
        Lang::English => {
            format!("{name} is no longer in the schemes folder. {fallback} is in force.")
        }
        Lang::Chinese => format!("配色 {name} 已不在 schemes 文件夹中，当前使用 {fallback}。"),
    }
}

/// Why the Install button is dark.
///
/// Windows' script execution policy governs `.psm1` files, and PSReadLine is a
/// script module: under `AllSigned` or `Restricted` a module Folio writes would
/// be refused at import. The button is disabled rather than allowed to write
/// files that cannot load, and the reason is on screen rather than in a card
/// after the fact.
#[must_use]
pub fn psreadline_policy_reason(policy: &str) -> String {
    match current() {
        Lang::English => format!(
            "Windows' execution policy is {policy}, which refuses to load an unsigned module."
        ),
        Lang::Chinese => format!("Windows 的执行策略是 {policy}，不会加载未签名的模块。"),
    }
}

/// The row's line when the machine's PSReadLine is older than the patched one.
///
/// **The three PSReadLine row lines take their language as an argument**, where
/// every other value-carrying string in this file reads [`current`]. They are
/// the only three the app caches for the life of the process — see
/// `crate::psreadline::row_description`, which keeps one slot per [`Lang`] — and
/// a cache filling itself from an ambient answer is a cache that cannot be asked
/// for the other column.
#[must_use]
pub fn psreadline_row_outdated_in(lang: Lang, found: &str) -> String {
    match lang {
        Lang::English => format!("{found} on this machine · the resize repair does nothing"),
        Lang::Chinese => format!("本机为 {found} · 缩放修复不起作用"),
    }
}

/// The row's line after Folio has written the module.
#[must_use]
pub fn psreadline_row_installed_in(lang: Lang, patched: &str) -> String {
    match lang {
        Lang::English => format!("{patched} · installed by Folio"),
        Lang::Chinese => format!("{patched} · 由 Folio 安装"),
    }
}

/// The row's line when an older Folio build's module is the one on disk
/// (2026-08-18).
///
/// It names **both** builds because the reader's question in this state is not
/// "what have I got" but "what would pressing On change", and a sentence naming
/// only one of the two answers neither half of it.
#[must_use]
pub fn psreadline_row_update_in(lang: Lang, installed: &str, available: &str) -> String {
    match lang {
        Lang::English => format!("{installed} · installed by Folio · {available} available"),
        Lang::Chinese => format!("{installed} · 由 Folio 安装 · 有 {available} 可用"),
    }
}

/// The row's line when the machine already had a new enough module of its own.
#[must_use]
pub fn psreadline_row_current_in(lang: Lang, found: &str) -> String {
    match lang {
        Lang::English => format!("{found} on this machine · already anchors itself"),
        Lang::Chinese => format!("本机为 {found} · 已自带正确的锚点"),
    }
}

/// The card raised when the module lands.
#[must_use]
pub fn psreadline_installed_toast(patched: &str) -> String {
    match current() {
        Lang::English => {
            format!("PSReadLine {patched} installed. PowerShell sessions started after this use it")
        }
        Lang::Chinese => {
            format!("已安装 PSReadLine {patched}。此后新开的 PowerShell 会话将使用它")
        }
    }
}

/// The card raised when writing the module failed, carrying what Windows said.
///
/// The operating system's own sentence is passed through rather than
/// summarised: it is the only text that distinguishes a full disk from a
/// redirected Documents folder from a file another process is holding open.
#[must_use]
pub fn psreadline_install_failed(message: &str) -> String {
    match current() {
        Lang::English => format!("Could not install PSReadLine: {message}"),
        Lang::Chinese => format!("安装 PSReadLine 失败：{message}"),
    }
}

#[must_use]
pub fn psreadline_remove_failed(message: &str) -> String {
    match current() {
        Lang::English => format!("Could not remove PSReadLine: {message}"),
        Lang::Chinese => format!("移除 PSReadLine 失败：{message}"),
    }
}

/// The card raised when the registry would not take the Explorer verb, carrying
/// what Windows said.
///
/// One sentence for both directions, because what a reader can act on is the
/// operating system's own code and not which way the switch was moving — and
/// the row beside the card already says which state the machine ended up in.
/// [`psreadline_install_failed`]'s ruling for passing the message through whole.
#[must_use]
pub fn context_menu_failed(message: &str) -> String {
    match current() {
        Lang::English => format!("Could not change Explorer's menu: {message}"),
        Lang::Chinese => format!("修改资源管理器菜单失败：{message}"),
    }
}

/// What the card says after an Advanced group was reset (§7.1.6c-5).
///
/// **The page is a parameter and not a word in the sentence**, because the verb
/// is a page's own: every category may grow an Advanced group, and a card that
/// named `Appearance` in its literal would be a card that lies on the second
/// page to grow one. The reset touches that page's advanced rows and nothing
/// else, and saying which page is the whole of the report.
#[must_use]
pub fn advanced_reset_toast(page: &str) -> String {
    match current() {
        Lang::English => format!("{page} ▸ Advanced is back to its defaults"),
        Lang::Chinese => format!("{page} ▸ 高级已恢复默认值"),
    }
}

/// A progress reading the mark slot can put a number on.
#[must_use]
pub fn progress_percent(percent: u32) -> String {
    format!("{percent}%")
}

/// **A magnification, as a whole percent** — a picture's and a page's, one
/// spelling (user ruling 2026-08-25).
///
/// Here rather than left as a `format!` beside each of the two things that zoom,
/// for [`image_preview_title`]'s reason and with its ruling: there is nothing in
/// it to translate — a number and a `%` — but the one place it is written should
/// be the place a translator would look, and the day a language wants `120％` or
/// a space before the sign, this is the one arm that grows a second column.
///
/// **Whole percents**, because the ladder's rungs are two decimal places
/// (`0.67`) and a reader asked to tell `67%` from `66.7%` is being handed the
/// engine's arithmetic instead of an answer.
#[must_use]
pub fn zoom_percent(factor: f64) -> String {
    format!("{}%", (factor * 100.0).round())
}

/// …and the two that qualify it.
#[must_use]
pub fn progress_error(percent: u32) -> String {
    match current() {
        Lang::English => format!("{percent}% — error"),
        Lang::Chinese => format!("{percent}% —— 出错"),
    }
}

#[must_use]
pub fn progress_paused(percent: u32) -> String {
    match current() {
        Lang::English => format!("{percent}% — paused"),
        Lang::Chinese => format!("{percent}% —— 已暂停"),
    }
}

/// The tip on a profile row this machine cannot start.
#[must_use]
pub fn unavailable_profile_tip(profile_title: &str) -> String {
    match current() {
        Lang::English => format!("{profile_title} — not found on this machine"),
        Lang::Chinese => format!("{profile_title} —— 本机上没有找到"),
    }
}

/// Why a row of the Profiles page is greyed whole.
///
/// **The row's sentence becomes the reason** (mock-up, `.row.unavailable`): a
/// shell that is not on this machine has no capabilities to report, and the one
/// line the row has room for is the reason it cannot run.
#[must_use]
pub fn profile_not_installed(profile_title: &str) -> String {
    match current() {
        Lang::English => format!("{profile_title} is not installed"),
        Lang::Chinese => format!("未安装 {profile_title}"),
    }
}

/// Why a built-in's `Colour` row is dark — **the row's sentence becomes the
/// reason**, which is this dialog's own idiom for a control that is not the
/// reader's to press.
///
/// The mock-up writes it of the profile it pictures (`PowerShell's mark is its
/// own`), so the name is a value rather than a word in the string: the five
/// identity colours are not this product's to repaint (S98/S31), and the row
/// says which of the five it is standing on.
/// `Auto (Bash init file)` — the `Shell integration` picker's button while the
/// row is on the rule rather than on an answer.
///
/// **The derived door is in the caption and not only in the list**, which is the
/// mock-up's own drawing (`Auto (None)`) and this dialog's own rule twice over:
/// a control has to be able to say its own value, and `Auto` alone says which
/// *rule* is in force while saying nothing about what the rule decided. The
/// menu's first item stays the bare word, because an item is a thing to choose
/// and what it will derive is a fact about the row rather than about the choice.
#[must_use]
pub fn profile_integration_auto(door: &str) -> String {
    match current() {
        Lang::English => format!("{} ({door})", Text::ProfilesAuto.text()),
        Lang::Chinese => format!("{}({door})", Text::ProfilesAuto.text()),
    }
}

#[must_use]
pub fn profile_mark_is_its_own(profile_title: &str) -> String {
    match current() {
        Lang::English => format!("{profile_title}'s mark is its own"),
        Lang::Chinese => format!("{profile_title} 的标记属于它自己"),
    }
}

/// The deletion card, which names **one fact this window actually holds**: how
/// many panes in it are running the profile, and that they keep running.
///
/// It does not count seeds on disk. A number read out of a session file at
/// delete time can be wrong by the day it matters, and the honest place for that
/// fact is the degrade banner the restarting seat already prints
/// (`M2-restart-shell-contract` §3).
///
/// The zero case is a different sentence rather than `0 panes`: a reader who was
/// running none is not being told about panes at all.
#[must_use]
pub fn profile_deleted(profile_title: &str, panes: usize) -> String {
    match (current(), panes) {
        (Lang::English, 0) => format!("{profile_title} is gone"),
        (Lang::English, 1) => {
            format!("{profile_title} is gone. One pane is running it, and it keeps running")
        }
        (Lang::English, panes) => {
            format!("{profile_title} is gone. {panes} panes are running it, and they keep running")
        }
        (Lang::Chinese, 0) => format!("{profile_title} 已删除"),
        (Lang::Chinese, panes) => {
            format!("{profile_title} 已删除。有 {panes} 个 pane 仍在运行它，它们继续运行")
        }
    }
}

/// What each of the eight struck colours is called.
///
/// A function here rather than a method on the colour, because the colours are a
/// fact about a drawing (`marks.rs` owns the hexes and holds no opinion about
/// language) and their names are a fact about a picker. The mapping is total, so
/// a ninth colour cannot ship without a word in both columns.
#[must_use]
pub fn colour_name(colour: crate::marks::MarkColour) -> Text {
    match colour {
        crate::marks::MarkColour::Blue => Text::ProfilesColourBlue,
        crate::marks::MarkColour::Teal => Text::ProfilesColourTeal,
        crate::marks::MarkColour::Green => Text::ProfilesColourGreen,
        crate::marks::MarkColour::Amber => Text::ProfilesColourAmber,
        crate::marks::MarkColour::Red => Text::ProfilesColourRed,
        crate::marks::MarkColour::Magenta => Text::ProfilesColourMagenta,
        crate::marks::MarkColour::Violet => Text::ProfilesColourViolet,
        crate::marks::MarkColour::Slate => Text::ProfilesColourSlate,
    }
}

/// What `profiles.json` could not be honoured for, named entry by entry.
///
/// `schemes`' register applied to a table: skip the entry, **name it**, say it
/// once, never crash, never go quiet. The id is in the sentence because the id
/// is what the reader has to search their own file for.
#[must_use]
pub fn profile_entry_fault(fault: &crate::profiles::ProfileFault) -> String {
    match (current(), fault) {
        (Lang::English, crate::profiles::ProfileFault::Unusable { id }) => {
            format!("profiles.json: {id} names no program and was skipped")
        }
        (Lang::Chinese, crate::profiles::ProfileFault::Unusable { id }) => {
            format!("profiles.json：{id} 没有写程序，已跳过")
        }
        (Lang::English, crate::profiles::ProfileFault::Duplicate { id }) => {
            format!("profiles.json: {id} appears twice; the first was kept")
        }
        (Lang::Chinese, crate::profiles::ProfileFault::Duplicate { id }) => {
            format!("profiles.json：{id} 出现了两次，保留了第一条")
        }
    }
}

/// A Recent row's age. **Three rungs and the ladder stops at hours**, which is
/// the English's own rule and not a limitation of the translation.
#[must_use]
pub fn ago_just_now() -> String {
    match current() {
        Lang::English => "just now".to_owned(),
        Lang::Chinese => "刚刚".to_owned(),
    }
}

#[must_use]
pub fn ago_minutes(minutes: u64) -> String {
    match current() {
        Lang::English => format!("{minutes}m ago"),
        Lang::Chinese => format!("{minutes} 分钟前"),
    }
}

#[must_use]
pub fn ago_hours(hours: u64) -> String {
    match current() {
        Lang::English => format!("{hours}h ago"),
        Lang::Chinese => format!("{hours} 小时前"),
    }
}

/// The tree's row for the entries past `DIR_ENTRY_CAP`.
///
/// English has no plural to get wrong here (`more` does not inflect) and Chinese
/// has none at all, which is the one place this pair is simpler in translation
/// than in the original.
#[must_use]
pub fn files_more_not_shown(count: usize) -> String {
    match current() {
        Lang::English => format!("{count} more not shown"),
        Lang::Chinese => format!("还有 {count} 项未显示"),
    }
}

/// What a preview says while it is being read — **one template for the two call
/// sites** (an image's and a document's) that used to spell it out separately.
#[must_use]
pub fn preview_loading(file_name: &str) -> String {
    match current() {
        Lang::English => format!("Loading {file_name}\u{2026}"),
        Lang::Chinese => format!("正在载入 {file_name}\u{2026}"),
    }
}

/// An image preview's head: the file's name and its pixel dimensions.
///
/// **The same string in both languages**, and it is here rather than left as a
/// literal so that the one place it is written is the place a translator would
/// look for it. There is nothing in it to translate — a name, a `×` (U+00D7) and
/// two numbers — and inventing a Chinese variant would be a difference with no
/// reason behind it.
#[must_use]
pub fn image_preview_title(name: &str, width: u32, height: u32) -> String {
    format!("{name} \u{2014} {width}\u{00d7}{height}")
}

/// The preview head's own refusal when the filesystem would not let a file go
/// (user ruling 2026-08-19).
///
/// **The only refusal this rename speaks.** Every other one — an empty name, an
/// unchanged name, a reserved character, a name another file in the folder
/// already has — is a fact about the draft, and the draft is in a box the reader
/// is looking at: the name snaps back and the picture has said it. A handle
/// another program is holding is a fact about the machine, which nothing on
/// screen can say.
///
/// The `{reason}` is an `io::Error` — the OS's own sentence in the OS's own
/// language, a known seam recorded in the inventory's §E and not papered over
/// here, exactly as [`not_saved`] leaves it.
#[must_use]
pub fn not_renamed(reason: &str) -> String {
    match current() {
        Lang::English => format!("Not renamed — {reason}"),
        Lang::Chinese => format!("未重命名 —— {reason}"),
    }
}

/// A save that did not happen, and why. The `{reason}` is either
/// [`Text::PreviewNothingToSave`] or an `io::Error`, and the second of those is
/// the OS's own sentence in the OS's own language — a known seam, recorded in the
/// inventory's §E and not papered over here.
#[must_use]
pub fn not_saved(reason: &str) -> String {
    match current() {
        Lang::English => format!("Not saved — {reason}"),
        Lang::Chinese => format!("未保存 —— {reason}"),
    }
}

// `quit_unsaved_message` — `Unsaved: a.txt, b.md`, the summary card's one
// sentence — **left this file on 2026-08-25** (B1). The card lists the files one
// per line now, and a line that is a file name is not a sentence: what it needed
// from this table was a preposition it no longer has anywhere to put. The lines
// themselves are built by `restore::unsaved_lines` and `unsaved_line`, which are
// a *name* and a *tab's own title* joined by an em dash — two strings this table
// never owned.

/// **What the save branch could not save** (slice E2 phase ①, v3 复审 ④-a).
///
/// The names and nothing else. A quit that stopped here has left every window
/// exactly where it was, so the card's whole job is to say which of the things
/// it was asked to write are still only in memory — the reader can see the rest
/// of the state for themselves, because it is still on the screen.
#[must_use]
pub fn quit_not_saved(names: &str) -> String {
    quit_not_saved_in(current(), names)
}

fn quit_not_saved_in(lang: Lang, names: &str) -> String {
    match lang {
        Lang::English => format!("Not saved: {names}"),
        Lang::Chinese => format!("未能保存：{names}"),
    }
}

/// **One row of `Move to window ▸`** (B9, user ruling 2026-08-25).
///
/// A window in this product has no name of its own — the title bar borrows the
/// active tab's — so the row names it the two ways a reader can actually tell
/// two windows apart from a menu: **where it stands in the run**, which is the
/// order they were opened in and the order this list is drawn in, and **how much
/// is in it**. The ruling wrote the shape out: 「Window 1 · 3 tabs」.
///
/// The active tab's title is deliberately not borrowed here. It is the one
/// string about a window that changes while the menu is open — a shell prints a
/// new title, a tab is renamed — and a row that renamed itself under a hand
/// already moving toward it is the failure `text_when`'s snapshot exists to
/// avoid one surface up.
#[must_use]
pub fn window_row(ordinal: usize, tabs: usize) -> String {
    window_row_in(current(), ordinal, tabs)
}

fn window_row_in(lang: Lang, ordinal: usize, tabs: usize) -> String {
    match lang {
        // English counts its nouns, so the row does too: `1 tab` and `2 tabs`.
        // A row reading `1 tabs` is a sentence written by a program.
        Lang::English if tabs == 1 => format!("Window {ordinal} · 1 tab"),
        Lang::English => format!("Window {ordinal} · {tabs} tabs"),
        Lang::Chinese => format!("窗口 {ordinal} · {tabs} 个标签"),
    }
}

/// The preview's failure banner when the reason came from the OS.
#[must_use]
pub fn preview_failed(reason: &str) -> String {
    match current() {
        Lang::English => format!("Preview failed: {reason}"),
        Lang::Chinese => format!("预览失败：{reason}"),
    }
}

/// A pane that came up as something other than what was asked for.
#[must_use]
pub fn fallback_banner_text(requested: &str, started: &str) -> String {
    match current() {
        Lang::English => format!("{requested} failed to start; using {started} instead."),
        Lang::Chinese => format!("{requested} 启动失败，改用 {started}。"),
    }
}

/// A pane whose saved profile this build has never heard of.
///
/// The id keeps its `{:?}` quoting in both languages, because the quotes are what
/// make it clear where the id starts and stops — it is a string out of a file,
/// and it may contain anything.
#[must_use]
pub fn unknown_profile_banner_text(unknown: &str, started: &str) -> String {
    match current() {
        Lang::English => {
            format!("Profile {unknown:?} is not in this build; using {started} instead.")
        }
        Lang::Chinese => format!("这个版本里没有配置文件 {unknown:?}，改用 {started}。"),
    }
}

/// The card raised when a scheme file has been sent to the Recycle Bin.
///
/// **Both facts, and no advice.** The file name is what the reader has to be
/// able to find again and the Recycle Bin is where they will find it — a delete
/// that says only "deleted" leaves somebody who pressed the wrong button with
/// nothing to do. The second clause names the scheme now in force, for
/// [`scheme_in_use_gone`]'s reason: the row moved, and a row that moves without
/// saying where is a row the reader has to go and check.
/// **The second clause only when a row actually moved** (user report
/// 2026-08-18). Since the verb became a menu over the whole folder, the ordinary
/// case is deleting a scheme nobody is wearing — and a card announcing which
/// scheme "is in force" after an operation that changed no colours would be
/// reporting a move that did not happen.
#[must_use]
pub fn scheme_deleted(file: &str, fallback: Option<&str>) -> String {
    match (current(), fallback) {
        (Lang::English, Some(fallback)) => {
            format!("{file} — moved to the Recycle Bin. {fallback} is in force.")
        }
        (Lang::Chinese, Some(fallback)) => {
            format!("{file} —— 已移入回收站。当前使用 {fallback}。")
        }
        (Lang::English, None) => format!("{file} — moved to the Recycle Bin."),
        (Lang::Chinese, None) => format!("{file} —— 已移入回收站。"),
    }
}

/// A delete that did not happen, and why. [`not_saved`]'s shape one verb over,
/// and the `{reason}` is the shell's own sentence on the same known seam.
#[must_use]
pub fn not_deleted(reason: &str) -> String {
    match current() {
        Lang::English => format!("Not deleted — {reason}"),
        Lang::Chinese => format!("未删除 —— {reason}"),
    }
}

/// The card raised when the chosen background picture is not on screen.
///
/// **The file, then what was asked of it.** The sentence the report caught —
/// "Preview failed: inline image exceeds its decode limit" — got both halves
/// wrong: it named no file, and it explained the failure in terms of a
/// mechanism (an inline image) that the reader of the Appearance page has never
/// met and whose limit was not the one applied.
///
/// It takes the **error** and not a rendered string, which is the whole reason
/// `bt_term::BackgroundImageError` carries facts rather than a sentence: a
/// `to_string()` on the far side of a worker would have arrived here in English
/// with nothing left to translate. The two seams that stay in the OS's own
/// language are the `io::Error` and the decoder's, and those are recorded in the
/// inventory's §E exactly as [`not_saved`]'s is.
#[must_use]
pub fn background_picture_refused(file: &str, error: &bt_term::BackgroundImageError) -> String {
    refusal_in_lang(current(), file, error)
}

/// [`background_picture_refused`] with the language named, which is
/// [`Text::in_lang`]'s split for the same reason: `CURRENT` is one global, and a
/// test that switched it to read the other column would be switching it under
/// every other test in the process.
fn refusal_in_lang(lang: Lang, file: &str, error: &bt_term::BackgroundImageError) -> String {
    use bt_term::BackgroundImageError as Refusal;
    match (lang, error) {
        (Lang::English, Refusal::InvalidPath) => {
            format!("{file} is not a picture file this window can open")
        }
        (Lang::Chinese, Refusal::InvalidPath) => format!("{file} 不是这扇窗能打开的图片文件"),
        (Lang::English, Refusal::Io(reason)) => format!("{file} could not be read: {reason}"),
        (Lang::Chinese, Refusal::Io(reason)) => format!("{file} 无法读取：{reason}"),
        (Lang::English, Refusal::TooLarge { bytes, limit }) => format!(
            "{file} is {} and a background picture may be up to {}",
            bt_term::mebibytes(*bytes),
            bt_term::mebibytes(*limit)
        ),
        (Lang::Chinese, Refusal::TooLarge { bytes, limit }) => format!(
            "{file} 有 {}，背景图最大 {}",
            bt_term::mebibytes(*bytes),
            bt_term::mebibytes(*limit)
        ),
        (Lang::English, Refusal::UnsupportedFormat) => {
            format!("{file} is not a picture format this build decodes")
        }
        (Lang::Chinese, Refusal::UnsupportedFormat) => format!("{file} 的格式这个版本无法解码"),
        (Lang::English, Refusal::Decode(reason)) => {
            format!("{file} could not be decoded: {reason}")
        }
        (Lang::Chinese, Refusal::Decode(reason)) => format!("{file} 解码失败：{reason}"),
        (Lang::English, Refusal::InvalidDimensions) => {
            format!("{file} has no pixels, or more of them than this build will decode")
        }
        (Lang::Chinese, Refusal::InvalidDimensions) => {
            format!("{file} 没有像素，或像素数超出这个版本的解码上限")
        }
    }
}

// ── the front door's own words (Windows landing block, slice 0) ────────────

/// Everything `folio.exe` says about its own command line.
///
/// **A second small table rather than ten free functions**, and the shape is
/// [`Text`]'s and not the value-carrying family's above: every one of these
/// carries a value, so none of them can be a `&'static str` — but every one of
/// them also has to be readable *in a named language*, which the ambient
/// `current()` the family below reads cannot give a test. `crate::seats`'s own
/// note states the suite's rule in as many words: [`install`] is a process-wide
/// answer and a test that moved it would move it under every other test running
/// beside it. So the column is a parameter, exactly as it is for `Text::in_lang`
/// and for the three PSReadLine row lines, and [`Self::text`] is the ambient
/// reading the product actually uses.
///
/// Five of these are said on a card the moment a window opens; five are said in
/// a console or a message box on a launch that never opens one. The pins for
/// what each of them is *for* are in `crate::cli`, beside the fault and the
/// refusal that raise them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CliText<'a> {
    /// The whole of what `--help` answers with, and what sits under every
    /// refusal.
    ///
    /// **The profile ids are the payload**, built from `profiles::PROFILES` at
    /// the call site. Written into the literal they would be two lists of five,
    /// in two languages, that nothing would update the day a profile is added —
    /// and a usage block that lies about what `--profile` takes is worse than
    /// one that omits the list.
    ///
    /// The flag names are not translated, for the reason this file's header
    /// gives about `git` and `LaTeX`: they are what a person types, and a
    /// translated `--cwd` would be a switch this program has not got.
    Usage { profile_ids: &'a str },
    /// A flag that takes a value, given without one.
    MissingValue(&'a str),
    /// A flag this build has not got.
    UnknownFlag(&'a str),
    /// A flag given twice — refused rather than resolved, see
    /// `crate::cli::CliFault::Repeated`.
    RepeatedFlag(&'a str),
    /// A second bare path. One command line names one place.
    ExtraPath(&'a str),
    /// `--cwd` named something that is not a folder.
    ///
    /// The second sentence is the half the reader cannot see for themselves: the
    /// window in front of them looks exactly like a window opened with no
    /// arguments at all, and without it a folder that was deleted and a shortcut
    /// that lost its arguments read identically.
    NoSuchFolder(&'a str),
    /// `--profile` named an id this build has not got. The id keeps its quotes
    /// for [`unknown_profile_banner_text`]'s reason: it came from outside and may
    /// be anything, including nothing but spaces.
    NoSuchProfile(&'a str),
    /// The bare path named nothing at all.
    NoSuchPath(&'a str),
    /// A real folder the chosen profile has no name for — a UNC share handed to
    /// WSL, and nothing else on this machine.
    UnreachableFolder {
        profile_title: &'a str,
        folder: &'a str,
    },
    /// A bare folder given alongside `--cwd`.
    PlaceAlreadyNamed(&'a str),
}

impl CliText<'_> {
    /// This line in the language the window is drawing in.
    #[must_use]
    pub fn text(&self) -> String {
        self.in_lang(current())
    }

    /// This line in a named language.
    #[must_use]
    pub fn in_lang(&self, lang: Lang) -> String {
        match self {
            Self::Usage { profile_ids } => match lang {
                Lang::English => format!(
                    "folio [--cwd <folder>] [--profile <id>] [<path>]\n\n\
                     \x20 --cwd <folder>    the first pane opens in that folder\n\
                     \x20 --profile <id>    the first pane's shell: {profile_ids}\n\
                     \x20 <path>            a folder opens a pane there; a file opens a preview\n\
                     \x20 -h, --help        this text"
                ),
                Lang::Chinese => format!(
                    "folio [--cwd <文件夹>] [--profile <id>] [<路径>]\n\n\
                     \x20 --cwd <文件夹>    第一个窗格在这个文件夹里打开\n\
                     \x20 --profile <id>    第一个窗格用哪种 shell：{profile_ids}\n\
                     \x20 <路径>            文件夹等同 --cwd，文件则打开预览\n\
                     \x20 -h, --help        显示这段说明"
                ),
            },
            Self::MissingValue(flag) => match lang {
                Lang::English => format!("{flag} takes a value."),
                Lang::Chinese => format!("{flag} 后面要跟一个值。"),
            },
            Self::UnknownFlag(flag) => match lang {
                Lang::English => format!("There is no {flag} option."),
                Lang::Chinese => format!("没有 {flag} 这个选项。"),
            },
            Self::RepeatedFlag(flag) => match lang {
                Lang::English => format!("{flag} was given twice."),
                Lang::Chinese => format!("{flag} 给了两次。"),
            },
            Self::ExtraPath(path) => match lang {
                Lang::English => format!("One path at a time — {path} is the second."),
                Lang::Chinese => format!("一次只能给一个路径，{path} 是第二个。"),
            },
            Self::NoSuchFolder(folder) => match lang {
                Lang::English => {
                    format!("No folder at {folder}. This pane opened where a new one would.")
                }
                Lang::Chinese => format!("{folder} 不是一个文件夹，这个窗格开在新窗格的默认位置。"),
            },
            Self::NoSuchProfile(id) => match lang {
                Lang::English => {
                    format!("No profile called {id:?}. This pane opened with the default.")
                }
                Lang::Chinese => format!("没有叫 {id:?} 的配置，这个窗格用了默认配置。"),
            },
            Self::NoSuchPath(path) => match lang {
                Lang::English => format!("There is no {path}."),
                Lang::Chinese => format!("{path} 不存在。"),
            },
            Self::UnreachableFolder {
                profile_title,
                folder,
            } => match lang {
                Lang::English => format!(
                    "{profile_title} has no name for {folder}. This pane opened at its own start."
                ),
                Lang::Chinese => {
                    format!("{profile_title} 无法表示 {folder}，这个窗格开在它自己的起始位置。")
                }
            },
            Self::PlaceAlreadyNamed(folder) => match lang {
                Lang::English => {
                    format!("--cwd already said where to open, so {folder} was not used.")
                }
                Lang::Chinese => format!("--cwd 已经说了在哪里打开，{folder} 没有被采用。"),
            },
        }
    }

    /// One of each, for the tests that have to walk the whole table.
    ///
    /// Written out rather than derived, for [`Text::ALL`]'s reason and with the
    /// one difference this table's payloads force: a variant added without a line
    /// here fails nothing at compile time, so the `match` in
    /// `every_line_of_the_front_door_reads_in_both_languages` is exhaustive over
    /// the enum and this list is checked against its own length.
    ///
    /// The samples deliberately contain no Han characters of their own, so that
    /// the Chinese column's own words are what the Han test finds.
    #[cfg(test)]
    pub const SAMPLES: [Self; 10] = [
        Self::Usage {
            profile_ids: "pwsh, winps",
        },
        Self::MissingValue("--cwd"),
        Self::UnknownFlag("--nope"),
        Self::RepeatedFlag("--profile"),
        Self::ExtraPath(r"D:\b"),
        Self::NoSuchFolder(r"D:\gone"),
        Self::NoSuchProfile("fish"),
        Self::NoSuchPath(r"D:\gone"),
        Self::UnreachableFolder {
            profile_title: "WSL",
            folder: r"\\server\share",
        },
        Self::PlaceAlreadyNamed(r"D:\b"),
    ];
}

// ── the tail's own value-carrying strings (§7.1.6c-7) ──────────────────────
//
// Every one of these is a sentence with a number, a name or a path in it, which
// is why it is here and not in the table: the table is `&'static str` and these
// allocate. They are written as a private `…_in(lang, …)` with a public wrapper
// that reads the language in force, which is
// [`psreadline_row_outdated_in`]'s shape and is here for its reason — a test
// that had to `install` a language in order to read a sentence would be a test
// racing every other test in this file for the process's own answer.

/// The recorder's third refusal: the chord is taken, and by which row.
///
/// **No swap is offered**, so the sentence's whole job is to name the row that
/// has it — which is what a reader needs in order to go and clear it themselves.
#[must_use]
pub fn shortcut_already_used(title: &str) -> String {
    shortcut_already_used_in(current(), title)
}

fn shortcut_already_used_in(lang: Lang, title: &str) -> String {
    match lang {
        Lang::English => format!("Already used by {title}"),
        Lang::Chinese => format!("已被「{title}」占用"),
    }
}

/// **The hint card's last cell, when the window could not hold every line**
/// (§7.1.5e′).
///
/// [`git_more_changed_files`]'s own rule one surface over, and it is the rule
/// rather than a courtesy: a card that showed fourteen of sixteen without saying
/// so would be this window teaching a reader that two chords do not exist.
#[must_use]
pub fn key_hint_more(count: usize) -> String {
    key_hint_more_in(current(), count)
}

fn key_hint_more_in(lang: Lang, count: usize) -> String {
    match lang {
        Lang::English => format!("{count} more not shown"),
        Lang::Chinese => format!("还有 {count} 个没显示"),
    }
}

/// **How many pages the glance card says a document holds** (user ruling
/// 2026-08-25; `docs/DESIGN.md` §7.10 ⑥).
///
/// The plural is one `if`, on [`git_pill_ahead`]'s own reasoning: a one-page PDF
/// is an everyday thing — a receipt, a boarding pass, a signed form — and "1
/// pages" on a card whose whole job is stating a fact would undo the fact.
#[must_use]
pub fn peek_page_count(count: usize) -> String {
    peek_page_count_in(current(), count)
}

fn peek_page_count_in(lang: Lang, count: usize) -> String {
    match lang {
        Lang::English if count == 1 => "1 page".to_owned(),
        Lang::English => format!("{count} pages"),
        Lang::Chinese => format!("{count} 页"),
    }
}

/// R33's own row: the status read everything and the page shows the first two
/// thousand, and it says so rather than quietly being short.
#[must_use]
pub fn git_more_changed_files(count: usize) -> String {
    git_more_changed_files_in(current(), count)
}

fn git_more_changed_files_in(lang: Lang, count: usize) -> String {
    match lang {
        Lang::English => format!("{count} more changed files not shown"),
        Lang::Chinese => format!("还有 {count} 个改动的文件没有显示"),
    }
}

/// A file git followed to a new name — **one sentence for the panel and the
/// graph**, which spelled it with two different dashes until this slice.
#[must_use]
pub fn git_renamed_from(path: &str, from: &str) -> String {
    git_renamed_from_in(current(), path, from)
}

fn git_renamed_from_in(lang: Lang, path: &str, from: &str) -> String {
    match lang {
        Lang::English => format!("{path} — renamed from {from}"),
        Lang::Chinese => format!("{path} —— 从 {from} 重命名而来"),
    }
}

/// The three sentences a branch row carries (G36).
///
/// A remote row carries no verb (T9), so it says what it *is* instead of
/// promising a checkout it will not do.
#[must_use]
pub fn git_branch_remote_tip(name: &str) -> String {
    git_branch_remote_tip_in(current(), name)
}

fn git_branch_remote_tip_in(lang: Lang, name: &str) -> String {
    match lang {
        Lang::English => format!("{name} - a branch on a remote"),
        Lang::Chinese => format!("{name} —— 远程上的一个分支"),
    }
}

#[must_use]
pub fn git_branch_current_tip(name: &str) -> String {
    git_branch_current_tip_in(current(), name)
}

fn git_branch_current_tip_in(lang: Lang, name: &str) -> String {
    match lang {
        Lang::English => format!("{name} - the branch you are on"),
        Lang::Chinese => format!("{name} —— 你正在的分支"),
    }
}

#[must_use]
pub fn git_branch_checkout_tip(name: &str) -> String {
    git_branch_checkout_tip_in(current(), name)
}

fn git_branch_checkout_tip_in(lang: Lang, name: &str) -> String {
    match lang {
        Lang::English => format!("Check out {name}"),
        Lang::Chinese => format!("检出 {name}"),
    }
}

/// The masthead of a repository whose first commit has not happened (R7): it has
/// a branch name, and saying only the name would claim a branch that has never
/// existed.
#[must_use]
pub fn git_unborn_masthead(name: &str) -> String {
    git_unborn_masthead_in(current(), name)
}

fn git_unborn_masthead_in(lang: Lang, name: &str) -> String {
    format!("{name} — {}", Text::GitUnborn.in_lang(lang))
}

/// What a count pill says (G22). **The plural is one `if`**, and the alternative
/// is a sentence that is wrong every time the count is one — which on a branch
/// you have just committed to is most of the time.
#[must_use]
pub fn git_pill_ahead(count: usize) -> String {
    git_pill_ahead_in(current(), count)
}

fn git_pill_ahead_in(lang: Lang, count: usize) -> String {
    match lang {
        Lang::English if count == 1 => "1 commit ahead".to_owned(),
        Lang::English => format!("{count} commits ahead"),
        Lang::Chinese => format!("领先 {count} 个提交"),
    }
}

#[must_use]
pub fn git_pill_behind(count: usize) -> String {
    git_pill_behind_in(current(), count)
}

fn git_pill_behind_in(lang: Lang, count: usize) -> String {
    match lang {
        Lang::English if count == 1 => "1 commit behind".to_owned(),
        Lang::English => format!("{count} commits behind"),
        Lang::Chinese => format!("落后 {count} 个提交"),
    }
}

/// `Comparing abc1234 → def5678` (D6). The arrow is a picture and is the same
/// in both columns.
#[must_use]
pub fn graph_compare(left: &str, right: &str) -> String {
    graph_compare_in(current(), left, right)
}

fn graph_compare_in(lang: Lang, left: &str, right: &str) -> String {
    match lang {
        Lang::English => format!("Comparing {left} → {right}"),
        Lang::Chinese => format!("正在比较 {left} → {right}"),
    }
}

/// `3 of 17` — where the search is, out of what it found (T4).
///
/// **The Chinese is a fraction and not a sentence**, and that is a length
/// ruling rather than a translation: the count is drawn at the right-hand end of
/// a 180-pixel field, and `第 3 个，共 17 个` would take the field's own width to
/// say what `3/17` says.
#[must_use]
pub fn graph_search_position(at: usize, total: usize) -> String {
    graph_search_position_in(current(), at, total)
}

fn graph_search_position_in(lang: Lang, at: usize, total: usize) -> String {
    match lang {
        Lang::English => format!("{at} of {total}"),
        Lang::Chinese => format!("{at}/{total}"),
    }
}

/// What the filter's button says once branches have been picked by hand.
#[must_use]
pub fn graph_filter_branches(count: usize) -> String {
    graph_filter_branches_in(current(), count)
}

fn graph_filter_branches_in(lang: Lang, count: usize) -> String {
    match lang {
        // `1 branches` is not English, which is the whole reason this is a
        // function and not a suffix.
        Lang::English if count == 1 => "1 branch".to_owned(),
        Lang::English => format!("{count} branches"),
        Lang::Chinese => format!("{count} 个分支"),
    }
}

/// The counts a file row draws as two numbers, said in words — because two
/// numbers side by side do not say which is which.
#[must_use]
pub fn graph_file_stat(added: u32, removed: u32) -> String {
    graph_file_stat_in(current(), added, removed)
}

fn graph_file_stat_in(lang: Lang, added: u32, removed: u32) -> String {
    match lang {
        Lang::English => {
            let added_lines = if added == 1 { "line" } else { "lines" };
            let removed_lines = if removed == 1 { "line" } else { "lines" };
            format!("{added} {added_lines} added, {removed} {removed_lines} removed")
        }
        Lang::Chinese => format!("增加 {added} 行，删除 {removed} 行"),
    }
}

/// What the Uncommitted Changes row says when you rest on it (V5).
#[must_use]
pub fn graph_uncommitted_tip(count: usize) -> String {
    graph_uncommitted_tip_in(current(), count)
}

fn graph_uncommitted_tip_in(lang: Lang, count: usize) -> String {
    let head = Text::GraphUncommitted.in_lang(lang);
    let foot = Text::GraphClickToList.in_lang(lang);
    match lang {
        Lang::English => {
            let files = if count == 1 { "file" } else { "files" };
            format!("{head} - {count} {files} the working tree has something to say about\n{foot}")
        }
        Lang::Chinese => {
            format!("{head} —— 工作区对 {count} 个文件有话要说\n{foot}")
        }
    }
}

/// What the window says when something has gone on the clipboard (D7).
///
/// The caller has already shortened what it is quoting — see
/// `git_graph::GRAPH_COPIED_MAX_CHARS`, which is where the ruling about how much
/// of a subject a card may repeat lives.
#[must_use]
pub fn graph_copied(said: &str) -> String {
    graph_copied_in(current(), said)
}

fn graph_copied_in(lang: Lang, said: &str) -> String {
    match lang {
        Lang::English => format!("Copied {said}"),
        Lang::Chinese => format!("已复制 {said}"),
    }
}

/// What the window says when a seek runs out of history (D2) — a notice and not
/// a refusal: the commit is further back than this window has read, which is a
/// fact about the reading rather than about the repository.
#[must_use]
pub fn graph_seek_gave_up(short: &str) -> String {
    graph_seek_gave_up_in(current(), short)
}

fn graph_seek_gave_up_in(lang: Lang, short: &str) -> String {
    match lang {
        Lang::English => format!("Commit {short} is further back than the loaded history"),
        Lang::Chinese => format!("提交 {short} 比已加载的历史还要早"),
    }
}

/// What a **folded** tick's glance card counts, and the one it then quotes.
///
/// Two functions rather than one that takes three numbers: the count is a
/// sentence about a bucket and the quotation is the newest thing in it, and the
/// caller composes them because it is the caller that decides whether the tick
/// is folded at all.
#[must_use]
pub fn rail_glance_count(matched: usize, commanded: usize) -> String {
    rail_glance_count_in(current(), matched, commanded)
}

fn rail_glance_count_in(lang: Lang, matched: usize, commanded: usize) -> String {
    match (lang, matched, commanded) {
        (Lang::English, 0, commanded) => format!("{commanded} commands"),
        (Lang::English, matched, 0) => format!("{matched} lines"),
        (Lang::English, matched, commanded) => {
            format!("{matched} lines, {commanded} commands")
        }
        (Lang::Chinese, 0, commanded) => format!("{commanded} 条命令"),
        (Lang::Chinese, matched, 0) => format!("{matched} 行"),
        (Lang::Chinese, matched, commanded) => format!("{matched} 行，{commanded} 条命令"),
    }
}

/// `Window · 3 tabs` — what a Recent row that stands for a whole closed window
/// says when the pointer rests on it (multiwindow slice D, ruling ②).
///
/// A function and not a [`Text`] row because it carries a number, on the same
/// footing as every other counted sentence in this file. The word is in the tip
/// rather than in the row's label because the label is one measured line and
/// already says which tab the window opened with — see `seed::Seed::first_tab`.
///
/// Singular is spelled out in English because "1 tabs" is the kind of sentence a
/// product says when nobody was reading it; Chinese needs no such branch.
#[must_use]
pub fn recent_window_tip(tabs: usize) -> String {
    recent_window_tip_in(current(), tabs)
}

fn recent_window_tip_in(lang: Lang, tabs: usize) -> String {
    match (lang, tabs) {
        (Lang::English, 1) => "Window · 1 tab".to_owned(),
        (Lang::English, tabs) => format!("Window · {tabs} tabs"),
        (Lang::Chinese, tabs) => format!("窗口 · {tabs} 个标签页"),
    }
}

/// `12 lines · latest: cargo test` — the count, and the newest member's own
/// line after it.
#[must_use]
pub fn rail_glance_latest(count: &str, body: &str) -> String {
    rail_glance_latest_in(current(), count, body)
}

fn rail_glance_latest_in(lang: Lang, count: &str, body: &str) -> String {
    match lang {
        Lang::English => format!("{count} · latest: {body}"),
        Lang::Chinese => format!("{count} · 最新：{body}"),
    }
}

/// A command with no `D` yet. Only ever a command: a matched line has no
/// lifetime of its own to report.
#[must_use]
pub fn rail_glance_running(body: &str) -> String {
    rail_glance_running_in(current(), body)
}

fn rail_glance_running_in(lang: Lang, body: &str) -> String {
    match lang {
        Lang::English => format!("running · {body}"),
        Lang::Chinese => format!("运行中 · {body}"),
    }
}

/// How a command the shell called failed ended. Zero is not reported — a card
/// that said `exit 0` would spend its width on the ordinary case.
#[must_use]
pub fn rail_exit_code(code: i32) -> String {
    rail_exit_code_in(current(), code)
}

fn rail_exit_code_in(lang: Lang, code: i32) -> String {
    match lang {
        Lang::English => format!("exit {code}"),
        Lang::Chinese => format!("退出 {code}"),
    }
}

/// The gate's own sentence over unsaved buffers — **by name, always**
/// (§7.1.3). A gate that said "some files have unsaved changes" would be asking
/// you to guess what you are about to lose.
#[must_use]
pub fn gate_unsaved_message(names: &str) -> String {
    gate_unsaved_message_in(current(), names)
}

fn gate_unsaved_message_in(lang: Lang, names: &str) -> String {
    match lang {
        Lang::English => format!("Discard unsaved changes to {names}?"),
        Lang::Chinese => format!("放弃对 {names} 未保存的改动？"),
    }
}

/// And over a working-tree discard (R14), which is not about a buffer at all.
#[must_use]
pub fn gate_git_discard_message(names: &str) -> String {
    gate_git_discard_message_in(current(), names)
}

fn gate_git_discard_message_in(lang: Lang, names: &str) -> String {
    match lang {
        Lang::English => format!(
            "{names} goes back to the last staged or committed version. This cannot be undone."
        ),
        Lang::Chinese => format!("{names} 会回到最后一次暂存或提交的样子。这一步无法撤销。"),
    }
}

/// The same act on a file git has never seen, where *discard* means delete.
#[must_use]
pub fn gate_git_delete_message(names: &str) -> String {
    gate_git_delete_message_in(current(), names)
}

fn gate_git_delete_message_in(lang: Lang, names: &str) -> String {
    match lang {
        Lang::English => {
            format!("{names} is deleted. git has no copy of it, so this cannot be undone.")
        }
        Lang::Chinese => format!("{names} 会被删除。git 没有它的副本，所以无法撤销。"),
    }
}

/// **Short and honest** (v2 ④): it says what the command is going to be — `-d`
/// — without saying the word, because "git will refuse if it is not merged" is
/// what `-d` means and is the one thing a reader needs before pressing.
#[must_use]
pub fn gate_delete_branch_message(names: &str) -> String {
    gate_delete_branch_message_in(current(), names)
}

fn gate_delete_branch_message_in(lang: Lang, names: &str) -> String {
    match lang {
        Lang::English => format!("Delete branch {names}? git will refuse if it is not merged."),
        Lang::Chinese => format!("删除分支 {names}？如果它还没有合并，git 会拒绝。"),
    }
}

/// And a tag, which git never refuses.
#[must_use]
pub fn gate_delete_tag_message(names: &str) -> String {
    gate_delete_tag_message_in(current(), names)
}

fn gate_delete_tag_message_in(lang: Lang, names: &str) -> String {
    match lang {
        Lang::English => format!("Delete tag {names}? The commit it names stays where it is."),
        Lang::Chinese => format!("删除标签 {names}？它指向的提交留在原处。"),
    }
}

/// **By count, because there is no name**: what this one deletes is a pane's own
/// past, which is not called anything.
#[must_use]
pub fn gate_clear_scrollback_message(amount: &str) -> String {
    gate_clear_scrollback_message_in(current(), amount)
}

fn gate_clear_scrollback_message_in(lang: Lang, amount: &str) -> String {
    match lang {
        Lang::English => {
            format!("{amount} of past output is deleted. Search over it will find nothing.")
        }
        Lang::Chinese => format!("{amount} 的历史输出会被删除。在其中搜索将一无所获。"),
    }
}

/// **It says what state you come out in**, because that is the whole of what
/// makes this one worth asking. Nothing is lost and the sentence does not
/// pretend otherwise; what it names is the thing a reader cannot see from the
/// row they pressed — that the repository stops being on a branch.
#[must_use]
pub fn gate_detach_message(names: &str) -> String {
    gate_detach_message_in(current(), names)
}

fn gate_detach_message_in(lang: Lang, names: &str) -> String {
    match lang {
        Lang::English => format!(
            "The working tree moves to {names}. HEAD stops being on a branch \
             until you check one out again."
        ),
        Lang::Chinese => {
            format!("工作区会移到 {names}。在你重新检出一条分支之前，HEAD 不再站在任何分支上。")
        }
    }
}

/// And with work in the tree, the other half first: what could be carried into
/// somewhere else is what a reader would want to know before anything about
/// where they are standing.
#[must_use]
pub fn gate_dirty_checkout_message(names: &str, detach: bool) -> String {
    gate_dirty_checkout_message_in(current(), names, detach)
}

fn gate_dirty_checkout_message_in(lang: Lang, names: &str, detach: bool) -> String {
    match lang {
        Lang::English => format!(
            "Uncommitted changes are carried across to {names}{}. git refuses \
             the ones it would have to overwrite.",
            if detach {
                ", and HEAD stops being on a branch"
            } else {
                ""
            }
        ),
        Lang::Chinese => format!(
            "未提交的改动会被带到 {names}{}。git 会拒绝那些必须覆盖才能完成的移动。",
            if detach {
                "，并且 HEAD 不再站在任何分支上"
            } else {
                ""
            }
        ),
    }
}

/// What the card says after a checkout onto a branch.
///
/// **The name and nothing else.** A notice about a move whose whole content is
/// where you moved to has no second clause worth reading, and the card's verb
/// says what the alternative is.
#[must_use]
pub fn checkout_notice(branch: &str) -> String {
    checkout_notice_in(current(), branch)
}

fn checkout_notice_in(lang: Lang, branch: &str) -> String {
    match lang {
        Lang::English => format!("Now on {branch}"),
        Lang::Chinese => format!("现在在 {branch}"),
    }
}

/// And after one onto a commit or a tag, where the state matters more than the
/// place.
#[must_use]
pub fn checkout_detached_notice(short: &str) -> String {
    checkout_detached_notice_in(current(), short)
}

fn checkout_detached_notice_in(lang: Lang, short: &str) -> String {
    match lang {
        Lang::English => format!("Standing on {short} \u{2014} HEAD is on no branch"),
        Lang::Chinese => format!("站在 {short} 上 \u{2014} HEAD 不在任何分支上"),
    }
}

/// The line over a named prompt's field — **what is being named, and where.**
#[must_use]
pub fn git_prompt_new_branch(subject: &str) -> String {
    git_prompt_new_branch_in(current(), subject)
}

fn git_prompt_new_branch_in(lang: Lang, subject: &str) -> String {
    match lang {
        Lang::English => format!("New branch at {subject}"),
        Lang::Chinese => format!("在 {subject} 处新建分支"),
    }
}

#[must_use]
pub fn git_prompt_new_tag(subject: &str) -> String {
    git_prompt_new_tag_in(current(), subject)
}

fn git_prompt_new_tag_in(lang: Lang, subject: &str) -> String {
    match lang {
        Lang::English => format!("New tag at {subject}"),
        Lang::Chinese => format!("在 {subject} 处新建标签"),
    }
}

#[must_use]
pub fn git_prompt_rename(subject: &str) -> String {
    git_prompt_rename_in(current(), subject)
}

fn git_prompt_rename_in(lang: Lang, subject: &str) -> String {
    match lang {
        Lang::English => format!("Rename {subject}"),
        Lang::Chinese => format!("重命名 {subject}"),
    }
}

/// A file on disk that would not read, named (§5.3) — the file's own name and
/// what is in force instead of it.
///
/// Two functions and not one with the noun passed in: what is standing in for a
/// damaged `keybindings.json` is a *table of chords* and what stands in for a
/// damaged `profiles.json` is a *list of profiles*, and a sentence that called
/// both "the defaults" would be the vaguer of the two everywhere.
#[must_use]
pub fn keybindings_file_unreadable(file: &str) -> String {
    keybindings_file_unreadable_in(current(), file)
}

fn keybindings_file_unreadable_in(lang: Lang, file: &str) -> String {
    match lang {
        Lang::English => format!("{file} could not be read; the default shortcuts are in force"),
        Lang::Chinese => format!("{file} 无法读取；现在生效的是默认快捷键"),
    }
}

#[must_use]
pub fn profiles_file_unreadable(file: &str) -> String {
    profiles_file_unreadable_in(current(), file)
}

fn profiles_file_unreadable_in(lang: Lang, file: &str) -> String {
    match lang {
        Lang::English => {
            format!("{file} could not be read; the profiles this build ships are in force")
        }
        Lang::Chinese => format!("{file} 无法读取；现在生效的是这个版本自带的档案"),
    }
}

/// And the same file refusing to parse **after** a window has been running on it
/// (§7.1.6c-6d), which is a different sentence because a different thing is in
/// force.
///
/// A third function rather than a parameter on the one above, for that pair's
/// own reason: the two differ in what stands in for the damaged file, and that
/// is the only fact either of them exists to say. At startup the answer is the
/// shipped table; mid-session it is the table the reader has been using all
/// along, which is the answer `reread_schemes` gives for a scheme file that
/// stops parsing under the hands of the person editing it.
#[must_use]
pub fn profiles_file_kept(file: &str) -> String {
    profiles_file_kept_in(current(), file)
}

fn profiles_file_kept_in(lang: Lang, file: &str) -> String {
    match lang {
        Lang::English => {
            format!("{file} could not be read; the profiles already in force were kept")
        }
        Lang::Chinese => format!("{file} 无法读取；保留了正在生效的档案"),
    }
}

/// `pins.json` refusing to parse at startup, when there is nothing in force to
/// keep — so what stands in for it is nothing at all, and the sentence says so.
#[must_use]
pub fn pins_file_unreadable(file: &str) -> String {
    pins_file_unreadable_in(current(), file)
}

fn pins_file_unreadable_in(lang: Lang, file: &str) -> String {
    match lang {
        Lang::English => format!("{file} could not be read; nothing is pinned this session"),
        Lang::Chinese => format!("{file} 无法读取；这次启动没有钉住的条目"),
    }
}

/// And the same file refusing to parse **after** a window has been running on
/// it, which is [`profiles_file_kept`]'s distinction one file over: what stands
/// in for the damaged file is the table the reader has been using all along.
#[must_use]
pub fn pins_file_kept(file: &str) -> String {
    pins_file_kept_in(current(), file)
}

fn pins_file_kept_in(lang: Lang, file: &str) -> String {
    match lang {
        Lang::English => format!("{file} could not be read; the pins already in force were kept"),
        Lang::Chinese => format!("{file} 无法读取；保留了正在生效的钉住条目"),
    }
}

/// **A switcher row the pin would not keep** (§7.7 ⑤, 2026-08-24).
///
/// A pin is a press and a press is owed an answer. §7.7 ③'s 「什么都没发生比一个
/// 确认框更诚实」 is about *going* somewhere — a seat that does not move is a
/// seat the reader can see did not move — and it does not carry over to a
/// control whose whole effect is in a file: a pin that refused in silence is
/// indistinguishable from a pin that is broken, which is precisely how this one
/// was reported.
///
/// Two facts and no opinion: what was not kept, and that this window would not
/// open it now. The target is passed through whole because which string was
/// refused is the only thing that separates a page whose file has been moved
/// from a row somebody edited by hand.
#[must_use]
pub fn switcher_pin_refused(target: &str) -> String {
    switcher_pin_refused_in(current(), target)
}

fn switcher_pin_refused_in(lang: Lang, target: &str) -> String {
    match lang {
        Lang::English => format!("Not pinned: this window would not open {target} now"),
        Lang::Chinese => format!("没有钉住：本窗现在不会打开 {target}"),
    }
}

/// **The engine would not hand its page over** (multiwindow slice F1c, §2.10's
/// `PageKept`).
///
/// The platform's own sentence is passed through rather than summarised, on
/// [`psreadline_install_failed`]'s reason: it is the only text that tells a
/// controller that had already gone from a parent window Windows would not set,
/// and neither of those is a thing this window can word better than the report
/// it was given.
#[must_use]
pub fn move_refused_page_kept(why: &str) -> String {
    move_refused_page_kept_in(current(), why)
}

fn move_refused_page_kept_in(lang: Lang, why: &str) -> String {
    match lang {
        Lang::English => format!("The page stayed in this window: {why}. The tab did not move."),
        Lang::Chinese => format!("这张页留在了本窗：{why}。tab 没有移动。"),
    }
}

/// **Two facts on one card**, when a refusal arrived after the pane had already
/// become a tab (multiwindow slice F1c).
///
/// The join is the language's own and not a space in both: written English
/// separates sentences with one, and written Chinese does not.
#[must_use]
pub fn move_refusal_notice(said: &str, pane_is_now_a_tab: bool) -> String {
    move_refusal_notice_in(current(), said, pane_is_now_a_tab)
}

fn move_refusal_notice_in(lang: Lang, said: &str, pane_is_now_a_tab: bool) -> String {
    if !pane_is_now_a_tab {
        return said.to_owned();
    }
    let also = Text::MoveRefusedPaneIsNowATab.in_lang(lang);
    match lang {
        Lang::English => format!("{said} {also}"),
        Lang::Chinese => format!("{said}{also}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — `System` resolves against the Windows UI language, and every
    /// spelling of Chinese is Chinese.
    ///
    /// The four tags are the four shapes Windows actually hands back:
    /// `GetUserPreferredUILanguages` gives `zh-Hans-CN` on a modern Simplified
    /// install and `zh-TW` on a Traditional one, the LANGID floor gives a bare
    /// `zh`, and everything else is some flavour of not-Chinese.
    #[test]
    fn system_follows_the_windows_ui_language_and_every_zh_is_chinese() {
        for tag in ["zh-CN", "zh-Hans-CN", "zh-TW", "zh-Hant-HK", "zh", "ZH-cn"] {
            assert_eq!(
                resolve(LanguageMode::System, tag),
                Lang::Chinese,
                "{tag} is a Chinese Windows"
            );
        }
        for tag in ["en-US", "en-GB", "ja-JP", "ko-KR", "de-DE", ""] {
            assert_eq!(
                resolve(LanguageMode::System, tag),
                Lang::English,
                "{tag} gets the source language"
            );
        }
    }

    /// PIN — an explicit choice does not consult Windows at all.
    ///
    /// The failure this guards is the plausible one: a `resolve` written as "look
    /// at the OS, then override" reads the OS on every path and would put a
    /// Chinese Windows into Chinese even for a user who had said `English` out
    /// loud.
    #[test]
    fn a_named_language_ignores_the_machine_it_is_running_on() {
        for tag in ["zh-CN", "en-US", "ja-JP"] {
            assert_eq!(resolve(LanguageMode::English, tag), Lang::English);
            assert_eq!(resolve(LanguageMode::Chinese, tag), Lang::Chinese);
        }
    }

    /// PIN — every entry appears in [`Text::ALL`] exactly once and reads in both
    /// languages.
    ///
    /// The uniqueness half is what makes the count meaningful: a copy-paste that
    /// listed one variant twice would keep the length right while leaving another
    /// one unlisted, and the unlisted one is the one that would ship with an
    /// empty column.
    #[test]
    fn every_entry_of_the_table_is_listed_once_and_reads_in_both_languages() {
        let mut seen: Vec<Text> = Vec::new();
        for entry in Text::ALL {
            assert!(!seen.contains(&entry), "{entry:?} is listed twice");
            seen.push(entry);
            for lang in [Lang::English, Lang::Chinese] {
                let text = entry.in_lang(lang);
                assert!(
                    !text.trim().is_empty(),
                    "{entry:?} has nothing to say in {lang:?}"
                );
            }
        }
        assert_eq!(seen.len(), Text::ALL.len());
    }

    /// PIN (§7.1.6b′, user rulings 2026-08-19 and 2026-08-20) — **the focus-mode
    /// row advertises the doors that exist, and no others.**
    ///
    /// The mode shipped with five doors and is down to two. Withdrawn on 08-19:
    /// a double-click on a pane header (it reads as "make this pane bigger",
    /// which is the opposite of what the mode does) and that pane's `⌄` menu (a
    /// list whose every other line acts on the pane under it is no place for a
    /// verb about the window). Withdrawn on 08-20: the column's own `Exit`
    /// button, the heading row it stood on, and the `Esc` rung —
    /// 「它是设置里的一个设置，怎么这么容易就退出」. What is left is the chord
    /// and this row itself — and this sentence is the one place in the product
    /// that *tells* a reader where the doors are, so it is the one place a
    /// withdrawn door can go on being promised after the code that answered it
    /// is gone.
    ///
    /// Red gate: put any withdrawn door's name back into the sentence without
    /// building the door, and this goes red in whichever language it was added
    /// to.
    #[test]
    fn the_focus_mode_row_names_the_chord_and_no_gesture_this_build_withdrew() {
        for lang in [Lang::English, Lang::Chinese] {
            let sentence = Text::DescFocusMode.in_lang(lang);
            assert!(
                sentence.contains("Ctrl+Shift+Z"),
                "{lang:?}: the row names the one chord that turns it"
            );
            for withdrawn in [
                "double-click",
                "双击",
                "⌄",
                "pane's header",
                "窗格标题栏",
                "Exit",
                "退出钮",
                "Esc",
            ] {
                assert!(
                    !sentence.contains(withdrawn),
                    "{lang:?}: the sentence still promises `{withdrawn}`, a door this \
                     build does not have"
                );
            }
        }
    }

    /// PIN (user ruling 2026-08-25) — **the mode is called `Cards` everywhere a
    /// reader can read it, and `Focus mode` nowhere.**
    ///
    /// The rename is of the *name*, not of the machine: `LayoutMode::Focus`,
    /// `solve_focused` and the `focus-mode` binding id are identifiers and stay
    /// exactly as they are — an identifier is a thing this table has no opinion
    /// about. What the ruling moved is the word on the `Appearance` row, and
    /// therefore the word on the shortcut card as well, because that card titles
    /// its row out of this table.
    ///
    /// The height row moves with it. `Focus card height` was named after the
    /// mode; a row still calling itself that, sitting directly under a row that
    /// now says `Cards`, would be this dialog holding two names for one feature
    /// — which is the exact failure §7.1.6b′ names when it forbids inventing a
    /// second vocabulary for one object.
    ///
    /// Red gate: leave either row's old words in place and this goes red in
    /// whichever language kept them.
    #[test]
    fn the_card_column_is_called_cards_on_every_surface_that_names_it() {
        assert_eq!(Text::RowFocusMode.in_lang(Lang::English), "Cards");
        assert_eq!(Text::RowFocusMode.in_lang(Lang::Chinese), "卡片");
        assert_eq!(
            Text::RowFocusCardHeight.in_lang(Lang::English),
            "Card height"
        );
        assert_eq!(Text::RowFocusCardHeight.in_lang(Lang::Chinese), "卡片高度");
        for entry in Text::ALL {
            for lang in [Lang::English, Lang::Chinese] {
                let text = entry.in_lang(lang);
                for withdrawn in ["Focus mode", "聚焦模式", "Focus card", "聚焦卡片"] {
                    assert!(
                        !text.contains(withdrawn),
                        "{entry:?} in {lang:?} still calls the mode `{withdrawn}`"
                    );
                }
            }
        }
    }

    /// PIN — **the front door's table has two columns too**, and the same three
    /// properties hold on it as on `Text`.
    ///
    /// Written as one test over one list rather than folded into the three
    /// above, because `CliText` is a different shape: its lines carry values, so
    /// they are `String`s built per call and cannot go in an array of
    /// `&'static str`. What is being pinned is identical — a variant with one
    /// column filled in, a variant whose "translation" is the English, and a
    /// Chinese column with no Chinese in it.
    ///
    /// The exhaustive `match` is the mechanism that makes the list complete: a
    /// variant added without a line in `SAMPLES` fails to compile here.
    #[test]
    fn every_line_of_the_front_door_reads_in_both_languages() {
        for sample in CliText::SAMPLES {
            // Exhaustive on purpose — see this test's own note.
            match sample {
                CliText::Usage { .. }
                | CliText::MissingValue(_)
                | CliText::UnknownFlag(_)
                | CliText::RepeatedFlag(_)
                | CliText::ExtraPath(_)
                | CliText::NoSuchFolder(_)
                | CliText::NoSuchProfile(_)
                | CliText::NoSuchPath(_)
                | CliText::UnreachableFolder { .. }
                | CliText::PlaceAlreadyNamed(_) => {}
            }
            let english = sample.in_lang(Lang::English);
            let chinese = sample.in_lang(Lang::Chinese);
            assert!(
                !english.trim().is_empty() && !chinese.trim().is_empty(),
                "{sample:?} has nothing to say in one of them"
            );
            assert_ne!(
                english, chinese,
                "{sample:?} was never translated — it says {english:?} in both columns"
            );
            assert!(
                chinese
                    .chars()
                    .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "{sample:?} reads {chinese:?}, which has no Chinese in it"
            );
            for word in english.split(|c: char| !c.is_ascii_alphabetic()) {
                assert!(
                    !["we", "our", "ours", "us", "ourselves"]
                        .contains(&word.to_ascii_lowercase().as_str()),
                    "{sample:?} says {word:?}: {english:?}"
                );
            }
        }
    }

    /// PIN — the usage block is a block: one line per thing it can be told, each
    /// line indented under the summary, in both languages.
    ///
    /// A `\n` lost from the middle of that literal produces a paragraph that
    /// still contains every word and is unreadable in a console, which nothing
    /// else here would catch.
    #[test]
    fn the_usage_block_keeps_its_shape_in_both_languages() {
        for lang in Lang::ALL {
            let usage = CliText::Usage {
                profile_ids: "pwsh, winps",
            }
            .in_lang(lang);
            let lines: Vec<&str> = usage.lines().collect();
            assert_eq!(lines.len(), 6, "{lang:?}: {usage}");
            assert!(lines[0].starts_with("folio [--cwd "), "{lang:?}");
            assert!(lines[1].is_empty(), "{lang:?}");
            for line in &lines[2..] {
                assert!(line.starts_with("  "), "{lang:?}: {line:?} is not indented");
            }
            assert!(usage.contains("pwsh, winps"), "{lang:?}");
        }
    }

    /// PIN — the two columns are actually two.
    ///
    /// A `pick(lang, "Saved", "Saved")` compiles, passes the test above and
    /// leaves an English word standing in a Chinese dialog. The exceptions are
    /// listed by hand and each one has a reason: a technical token, a quantity,
    /// or a Windows product name that is the same word in both.
    #[test]
    fn no_entry_ships_the_english_word_as_its_own_translation() {
        for entry in Text::ALL {
            if Text::UNTRANSLATED.contains(&entry) {
                continue;
            }
            let english = entry.in_lang(Lang::English);
            let chinese = entry.in_lang(Lang::Chinese);
            assert_ne!(
                english, chinese,
                "{entry:?} was never translated — it says {english:?} in both columns"
            );
        }
    }

    /// PIN — every Chinese entry actually contains Chinese.
    ///
    /// The failure this catches is the half-translation: a sentence whose
    /// technical tokens were kept and whose words were forgotten still differs
    /// from its English, so the test above would pass it.
    #[test]
    fn every_chinese_entry_carries_at_least_one_han_character() {
        for entry in Text::ALL {
            if Text::UNTRANSLATED.contains(&entry) {
                continue;
            }
            let chinese = entry.in_lang(Lang::Chinese);
            assert!(
                chinese
                    .chars()
                    .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "{entry:?} reads {chinese:?}, which has no Chinese in it"
            );
        }
    }

    /// PIN (user report 2026-08-18) — **a refused background picture names the
    /// file and says what was asked of it, in both languages, and never
    /// mentions an inline image.**
    ///
    /// The reported card read `Preview failed: inline image exceeds its decode
    /// limit`. Three things were wrong with it and only one was the limit: it
    /// named no file, it explained the failure through a mechanism the reader of
    /// the Appearance page has never met, and the Chinese window said the same
    /// English clause. All three are asserted here.
    ///
    /// Red gate: pass `error.to_string()` through a one-line formatter again and
    /// the Chinese half goes red, because there is nothing left to translate.
    #[test]
    fn a_refused_background_picture_names_the_file_in_both_languages() {
        use bt_term::BackgroundImageError as Refusal;
        const FILE: &str = "DSCF1264.JPG";
        for error in [
            Refusal::InvalidPath,
            Refusal::Io("access is denied".to_owned()),
            Refusal::TooLarge {
                bytes: 200 * 1024 * 1024,
                limit: bt_term::MAX_BACKGROUND_IMAGE_BYTES,
            },
            Refusal::UnsupportedFormat,
            Refusal::Decode("truncated stream".to_owned()),
            Refusal::InvalidDimensions,
        ] {
            let english = refusal_in_lang(Lang::English, FILE, &error);
            let chinese = refusal_in_lang(Lang::Chinese, FILE, &error);
            for (lang, said) in [(Lang::English, &english), (Lang::Chinese, &chinese)] {
                assert!(
                    said.starts_with(FILE),
                    "{error:?} in {lang:?} does not name the file: {said}"
                );
                assert!(
                    !said.contains("inline"),
                    "{error:?} in {lang:?} still speaks of inline images: {said}"
                );
            }
            assert_ne!(english, chinese, "{error:?} was never translated");
            assert!(
                chinese
                    .chars()
                    .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "{error:?} reads {chinese:?}, which has no Chinese in it"
            );
        }
        assert!(
            refusal_in_lang(
                Lang::English,
                FILE,
                &Refusal::TooLarge {
                    bytes: 200 * 1024 * 1024,
                    limit: bt_term::MAX_BACKGROUND_IMAGE_BYTES,
                }
            )
            .contains("64 MB"),
            "and the limit it names is the background's own, not the inline one"
        );
    }

    /// PIN (user ruling, 2026-08-17) — **no string in this window speaks in the
    /// first person.**
    ///
    /// The window is not a party to the conversation. "The output is not ours to
    /// keep" was the one line that forgot it: a product explaining itself, in the
    /// first person, about something it had already said it was not doing. What
    /// a reader needs from that sentence — the folders come back, the sessions
    /// are new — was over before the clause began.
    ///
    /// Mechanically this can only catch the pronouns, so it catches the
    /// pronouns. The other half of the ruling — no aside after a dash that
    /// justifies or moralises, no "is not X's to Y" — is a reading, and the
    /// readings are recorded next to the strings they were applied to. What is
    /// *not* forbidden is the second person: "your other tabs" and "follow your
    /// system setting" are addressed to the reader, which is the register this
    /// product has always used.
    #[test]
    fn no_string_in_the_window_speaks_in_the_first_person() {
        const FORBIDDEN_ENGLISH: [&str; 5] = ["we", "our", "ours", "us", "ourselves"];
        for entry in Text::ALL {
            let english = entry.in_lang(Lang::English);
            for word in english.split(|c: char| !c.is_ascii_alphabetic()) {
                assert!(
                    !FORBIDDEN_ENGLISH.contains(&word.to_ascii_lowercase().as_str()),
                    "{entry:?} says {word:?} — the window is not a party to the \
                     conversation: {english:?}"
                );
            }
            let chinese = entry.in_lang(Lang::Chinese);
            for pronoun in ["我们", "咱们", "本程序", "本软件"] {
                assert!(
                    !chinese.contains(pronoun),
                    "{entry:?} says {pronoun} — {chinese:?}"
                );
            }
        }
    }

    /// PIN (§B of the string inventory) — **the five profile titles are not in
    /// this file.**
    ///
    /// They are compared byte for byte against the OSC 2 title the shell reports.
    /// Translating one would not break a test anywhere near it: tab names would
    /// simply start showing the program's name instead of the profile's, on the
    /// machines where the shell speaks up, months later. So the guard is here, at
    /// the door translations come in through, and it reads the source file
    /// itself.
    /// It reads only the half above `mod tests`, because the needles it is
    /// hunting for have to be written out down here to be hunted for.
    #[test]
    fn no_profile_title_has_been_pulled_into_the_language_table() {
        let source = include_str!("i18n.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("a split always yields a first piece");
        for title in [
            "PowerShell 7",
            "Windows PowerShell 5.1",
            "Git Bash",
            "Command Prompt",
        ] {
            assert!(
                !source.contains(&format!("\"{title}\"")),
                "{title:?} is compared against the shell's own OSC 2 title and \
                 must never become a translatable string"
            );
        }
        // `WSL` on its own is a substring of English prose, so it is pinned as
        // the literal a table entry would be written as.
        assert!(!source.contains("\"WSL\""));
    }

    /// PIN — the short form of the blocked suffix is a substring of the long one.
    ///
    /// The narrow pane shows the short one; a translation that used different
    /// words for the two would make a link's state read differently depending on
    /// how wide the pane happened to be.
    #[test]
    fn the_blocked_word_is_the_same_word_with_and_without_its_separator() {
        for lang in [Lang::English, Lang::Chinese] {
            let long = Text::HyperlinkBlockedSuffix.in_lang(lang);
            let short = Text::HyperlinkBlocked.in_lang(lang);
            assert!(
                long.ends_with(short),
                "{lang:?}: {long:?} must end with {short:?}"
            );
        }
    }

    /// PIN — `System` is one word shared by Theme and Language.
    ///
    /// Not two entries that happen to agree: the inventory (T046) asks for the
    /// reuse by name, because "跟随系统" in one row and "系统" in the next would
    /// read as two different promises.
    #[test]
    fn the_theme_and_the_language_rows_say_system_with_one_string() {
        assert_eq!(Text::OptionSystem.in_lang(Lang::Chinese), "系统");
        assert_eq!(Text::OptionSystem.in_lang(Lang::English), "System");
    }

    /// PIN — before anyone has said otherwise, the window speaks the language it
    /// was written in.
    #[test]
    fn the_table_answers_english_until_a_language_is_installed() {
        // This test process may or may not have had `install` called by another
        // test; either way the invariant is that `current` is one of the two and
        // `text()` agrees with it.
        assert_eq!(Text::Settings.text(), Text::Settings.in_lang(current()));
        assert_eq!(LanguageMode::default(), LanguageMode::System);
        assert_eq!(Lang::default(), Lang::English);
    }

    // ── the hot switch (§7.1.6c-3c) ────────────────────────────────────────
    //
    // These build their own [`Current`] rather than moving the process's, and
    // that is a decision and not a shortcut. `cargo test` runs this crate's
    // cases in parallel in one process; a case that put the window into Chinese
    // for even a microsecond would race every other case in `seats.rs` that
    // asserts a pane head reads `"Terminal"`. The global is three lines of
    // delegation over this type, and this is the type with the contract in it.

    /// PIN — **a switch changes the answer and advances the revision, and a
    /// re-press of what is already in force does neither.**
    ///
    /// The second half is the one worth a test. `LayoutKey.lang_rev` is what a
    /// re-layout of every typeset band in the window hangs off, so a revision
    /// that ticked on every press would make pressing the ticked item cost a
    /// full re-typeset — which nothing on screen would show, and a profiler
    /// would find months later.
    #[test]
    fn installing_a_language_moves_the_answer_and_the_revision_together() {
        let current = Current::new();
        assert_eq!(current.get(), Lang::English);
        assert_eq!(current.revision(), 0);

        assert!(current.set(Lang::Chinese));
        assert_eq!(current.get(), Lang::Chinese);
        assert_eq!(current.revision(), 1);

        assert!(
            !current.set(Lang::Chinese),
            "the language already in force did not change"
        );
        assert_eq!(current.revision(), 1, "and so nothing was invalidated");

        assert!(current.set(Lang::English));
        assert_eq!(current.get(), Lang::English);
        assert_eq!(
            current.revision(),
            2,
            "switching back is a switch — the caches measured in Chinese are \
             just as stale as the ones measured in English were"
        );
    }

    /// PIN — **every thread this app runs reads the language that is in force**,
    /// including one that was already running when it moved.
    ///
    /// Not a hypothetical: `crate::psreadline`'s probe answers off its own
    /// thread and its row description is built from this table, and the settings
    /// dialog is the surface that both moves the language and draws that row. A
    /// language behind a `thread_local` — or behind a `OnceLock` each thread
    /// initialised for itself — would give that row the language of whichever
    /// thread happened to build it.
    #[test]
    fn a_language_installed_on_one_thread_is_read_on_every_other() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;

        let current = Arc::new(Current::new());
        let switched = Arc::new(AtomicBool::new(false));
        let reader = {
            let current = Arc::clone(&current);
            let switched = Arc::clone(&switched);
            std::thread::spawn(move || {
                while !switched.load(Ordering::Acquire) {
                    std::hint::spin_loop();
                }
                (current.get(), current.revision())
            })
        };
        assert!(current.set(Lang::Chinese));
        switched.store(true, Ordering::Release);
        let (lang, revision) = reader.join().expect("the reading thread");
        assert_eq!(lang, Lang::Chinese);
        assert_eq!(revision, 1);
    }

    /// PIN — the process-wide functions are that type and nothing else.
    ///
    /// Read-only, so it can stand beside a suite that reads this table on other
    /// threads. What it pins is that [`current`] answers off the same word
    /// [`lang_revision`] counts, rather than the two having drifted onto
    /// separate statics that could disagree.
    #[test]
    fn the_processs_language_and_its_revision_are_one_pair() {
        assert_eq!(current(), CURRENT.get());
        assert_eq!(lang_revision(), CURRENT.revision());
    }

    /// PIN — a discriminant survives the round trip through the atomic word.
    #[test]
    fn every_language_is_its_own_slot() {
        for lang in Lang::ALL {
            assert_eq!(Lang::from_index(lang as u8), lang);
        }
        let indices: Vec<usize> = Lang::ALL.iter().map(|lang| lang.index()).collect();
        assert_eq!(indices, (0..Lang::COUNT).collect::<Vec<_>>());
    }

    /// PIN (§7.1.6c-3c) — **no string in this window asks to be relaunched, and
    /// the Language row does not describe a moment.**
    ///
    /// Written against the source rather than against one entry, because the
    /// failure it guards is a re-addition somewhere else in the table: the card
    /// that used to stand here was reasonable while the switch needed a restart,
    /// and the next person to want one will write it as a new variant rather
    /// than by reviving the old name.
    #[test]
    fn no_string_in_this_table_asks_for_a_relaunch() {
        let source = include_str!("i18n.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("a split always yields a first piece");
        for needle in ["Restart", "restart", "重启"] {
            assert!(
                !source.contains(needle),
                "{needle:?} is still in this table — the language now switches \
                 where the reader is standing"
            );
        }
        // Not a source-wide needle: `TabTipPinned` says 下次启动会恢复 about a
        // pinned tab, which is a true sentence about a *tab* and has nothing to
        // do with this row.
        for lang in Lang::ALL {
            assert!(!Text::DescLanguage.in_lang(lang).contains("下次启动"));
        }
        for lang in Lang::ALL {
            let description = Text::DescLanguage.in_lang(lang);
            assert!(
                description.contains("中文"),
                "{lang:?}: the row names what is on offer — {description:?}"
            );
        }
        assert_eq!(
            Text::DescLanguage.in_lang(Lang::English),
            "English, 中文, or follow your system setting"
        );
        assert_eq!(
            Text::DescLanguage.in_lang(Lang::Chinese),
            "English、中文，或跟随系统设置"
        );
    }

    /// **THE WORD `pin` BELONGS TO THE TWO VERBS THAT MEAN MEMBERSHIP, AND THE
    /// PREVIEW PANE'S CONTROL IS NOT ONE OF THEM** (§7.7 ⑧, Claude 定
    /// 2026-08-23).
    ///
    /// The naming ticket W1 filed and left open (`docs/plans/web-preview/plan.md`
    /// §5) was that one word and one glyph carried two meanings two hundred
    /// pixels apart: a tab's pin keeps that tab across a launch, a switcher
    /// row's keeps that place in a list, and the preview head's held a *pane*
    /// against reuse. The first two are one idea said twice; the third is a
    /// different idea, and it is a lock now.
    ///
    /// Red gate, both directions, because a rename can fail either way:
    /// ① the lock's two tips carry no `pin`/`钉` — put the word back and this
    ///    goes red, which is the collision itself;
    /// ② the two real pins still say `Pin` and `PINNED` — a rename that swept
    ///    them along would have taken the word away from the verbs it is right
    ///    for, and left the window with no word for membership at all.
    /// ③ neither tip names a *file* or a *page*: a preview seat holds either,
    ///    and a tip that named one would be wrong on half the seats.
    /// The tips are also asserted to name what follows from the press, because
    /// "Lock" alone would be a control explaining its own noun.
    #[test]
    fn the_preview_panes_control_says_lock_and_leaves_pin_to_the_two_it_belongs_to() {
        for entry in [Text::PreviewLock, Text::PreviewUnlock] {
            for lang in Lang::ALL {
                let said = entry.in_lang(lang);
                for needle in ["pin", "Pin", "钉"] {
                    assert!(
                        !said.contains(needle),
                        "{entry:?} in {lang:?} still says {needle:?} — {said:?}"
                    );
                }
                for needle in ["file", "page", "文件", "网页"] {
                    assert!(
                        !said.contains(needle),
                        "{entry:?} in {lang:?} names {needle:?}, and a preview \
                         seat holds either kind — {said:?}"
                    );
                }
                assert!(
                    said.contains("preview") || said.contains("预览"),
                    "{entry:?} in {lang:?} says what follows from the press \
                     rather than naming itself — {said:?}"
                );
            }
        }
        assert!(Text::PreviewLock.in_lang(Lang::English).starts_with("Lock"));
        assert!(
            Text::PreviewUnlock
                .in_lang(Lang::English)
                .starts_with("Unlock")
        );
        // ② The word is not gone from the window; it went home.
        assert_eq!(Text::Pin.in_lang(Lang::English), "Pin");
        assert!(Text::Unpin.in_lang(Lang::English).starts_with("Unpin"));
        assert_eq!(Text::PinnedSection.in_lang(Lang::English), "PINNED");
    }

    /// PIN — **the tail's value-carrying sentences read in both languages too.**
    ///
    /// The three properties the table's own gates hold are held here by hand,
    /// because these are functions rather than arms and no `ALL` can list them:
    /// both columns say something, the two columns differ, and the Chinese one
    /// has Chinese in it. Each line calls the private `…_in`, so nothing here
    /// touches the process's own language and the case can run beside every
    /// other test in this file.
    ///
    /// A sentence added to that family without a line here is not caught by a
    /// compiler — this list is not exhaustive the way `Text::ALL` is made to be
    /// — but a sentence added *with* one is caught the moment its second column
    /// is forgotten, which is the failure this file has actually shipped.
    #[test]
    fn every_sentence_the_tail_composes_reads_in_both_languages() {
        // The one entry whose Chinese carries no Chinese, and the reason:
        // `3 of 17` becomes `3/17`, a fraction rather than a sentence, because
        // the count is drawn at the right-hand end of a 180-pixel search field
        // and `第 3 个，共 17 个` would take the whole of it.
        let numeric = ["graph_search_position"];
        let said = |lang: Lang| {
            vec![
                (
                    "shortcut_already_used",
                    shortcut_already_used_in(lang, "Close pane"),
                ),
                (
                    "git_more_changed_files",
                    git_more_changed_files_in(lang, 12),
                ),
                ("key_hint_more", key_hint_more_in(lang, 4)),
                ("peek_page_count", peek_page_count_in(lang, 1)),
                ("peek_page_count_many", peek_page_count_in(lang, 3)),
                (
                    "git_renamed_from",
                    git_renamed_from_in(lang, "b.rs", "a.rs"),
                ),
                (
                    "git_branch_remote_tip",
                    git_branch_remote_tip_in(lang, "origin/main"),
                ),
                (
                    "git_branch_current_tip",
                    git_branch_current_tip_in(lang, "main"),
                ),
                (
                    "git_branch_checkout_tip",
                    git_branch_checkout_tip_in(lang, "side"),
                ),
                ("git_unborn_masthead", git_unborn_masthead_in(lang, "main")),
                ("git_pill_ahead", git_pill_ahead_in(lang, 1)),
                ("git_pill_ahead_many", git_pill_ahead_in(lang, 4)),
                ("git_pill_behind", git_pill_behind_in(lang, 1)),
                ("git_pill_behind_many", git_pill_behind_in(lang, 4)),
                (
                    "graph_compare",
                    graph_compare_in(lang, "abc1234", "def5678"),
                ),
                (
                    "graph_search_position",
                    graph_search_position_in(lang, 3, 17),
                ),
                ("graph_filter_branches", graph_filter_branches_in(lang, 1)),
                (
                    "graph_filter_branches_many",
                    graph_filter_branches_in(lang, 3),
                ),
                ("graph_file_stat", graph_file_stat_in(lang, 1, 9)),
                ("graph_uncommitted_tip", graph_uncommitted_tip_in(lang, 2)),
                ("graph_copied", graph_copied_in(lang, "36d3949")),
                ("graph_seek_gave_up", graph_seek_gave_up_in(lang, "fffffff")),
                ("rail_glance_count", rail_glance_count_in(lang, 4, 2)),
                (
                    "rail_glance_count_commands",
                    rail_glance_count_in(lang, 0, 2),
                ),
                ("rail_glance_count_lines", rail_glance_count_in(lang, 4, 0)),
                (
                    "rail_glance_latest",
                    rail_glance_latest_in(lang, "4 lines", "cargo test"),
                ),
                (
                    "rail_glance_running",
                    rail_glance_running_in(lang, "cargo test"),
                ),
                ("rail_exit_code", rail_exit_code_in(lang, 130)),
                (
                    "gate_unsaved_message",
                    gate_unsaved_message_in(lang, "a.txt, b.md"),
                ),
                ("quit_not_saved", quit_not_saved_in(lang, "b.md")),
                ("window_row", window_row_in(lang, 2, 3)),
                ("window_row_one", window_row_in(lang, 2, 1)),
                (
                    "gate_git_discard_message",
                    gate_git_discard_message_in(lang, "a.txt"),
                ),
                (
                    "gate_git_delete_message",
                    gate_git_delete_message_in(lang, "a.txt"),
                ),
                (
                    "gate_delete_branch_message",
                    gate_delete_branch_message_in(lang, "side"),
                ),
                (
                    "gate_delete_tag_message",
                    gate_delete_tag_message_in(lang, "v1"),
                ),
                (
                    "gate_clear_scrollback_message",
                    gate_clear_scrollback_message_in(lang, "2,048 lines"),
                ),
                (
                    "gate_detach_message",
                    gate_detach_message_in(lang, "36d3949"),
                ),
                (
                    "gate_dirty_checkout_message",
                    gate_dirty_checkout_message_in(lang, "main", false),
                ),
                (
                    "gate_dirty_checkout_message_detach",
                    gate_dirty_checkout_message_in(lang, "v1", true),
                ),
                ("checkout_notice", checkout_notice_in(lang, "main")),
                (
                    "checkout_detached_notice",
                    checkout_detached_notice_in(lang, "36d3949"),
                ),
                (
                    "git_prompt_new_branch",
                    git_prompt_new_branch_in(lang, "36d3949"),
                ),
                ("git_prompt_new_tag", git_prompt_new_tag_in(lang, "36d3949")),
                ("git_prompt_rename", git_prompt_rename_in(lang, "side")),
                (
                    "keybindings_file_unreadable",
                    keybindings_file_unreadable_in(lang, "keybindings.json"),
                ),
                (
                    "profiles_file_unreadable",
                    profiles_file_unreadable_in(lang, "profiles.json"),
                ),
                (
                    "profiles_file_kept",
                    profiles_file_kept_in(lang, "profiles.json"),
                ),
                (
                    "pins_file_unreadable",
                    pins_file_unreadable_in(lang, "pins.json"),
                ),
                ("pins_file_kept", pins_file_kept_in(lang, "pins.json")),
                (
                    "switcher_pin_refused",
                    switcher_pin_refused_in(lang, "file:///D:/site/report.html"),
                ),
            ]
        };
        let english = said(Lang::English);
        let chinese = said(Lang::Chinese);
        assert_eq!(english.len(), chinese.len());
        for ((name, english), (_, chinese)) in english.into_iter().zip(chinese) {
            assert!(!english.trim().is_empty(), "{name} says nothing in English");
            assert!(!chinese.trim().is_empty(), "{name} says nothing in Chinese");
            assert_ne!(
                english, chinese,
                "{name} was never translated — it says {english:?} in both columns"
            );
            for word in english.split(|c: char| !c.is_ascii_alphabetic()) {
                assert!(
                    !["we", "our", "ours", "us", "ourselves"]
                        .contains(&word.to_ascii_lowercase().as_str()),
                    "{name} says {word:?}: {english:?}"
                );
            }
            if numeric.contains(&name) {
                continue;
            }
            assert!(
                chinese
                    .chars()
                    .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "{name} reads {chinese:?}, which has no Chinese in it"
            );
        }
    }

    /// PIN — **every row of the shortcut table has a name in both languages.**
    ///
    /// The page's own gate, and it is not the table's: `Text::ALL` proves that
    /// every *entry* reads in both columns, and this proves that every *row*
    /// points at one. A row added to `BINDINGS` with a title borrowed from the
    /// wrong family — the failure this catches — reads perfectly well in both
    /// languages and is still the wrong name.
    #[test]
    fn every_shortcut_row_is_named_in_both_languages() {
        let mut seen: Vec<&'static str> = Vec::new();
        for binding in crate::shortcuts::BINDINGS {
            for lang in Lang::ALL {
                let title = binding.title.in_lang(lang);
                assert!(!title.trim().is_empty(), "{} has no name", binding.id);
            }
            let english = binding.title.in_lang(Lang::English);
            assert!(
                !seen.contains(&english),
                "{} shares the name {english:?} with an earlier row",
                binding.id
            );
            seen.push(english);
        }
    }
}
