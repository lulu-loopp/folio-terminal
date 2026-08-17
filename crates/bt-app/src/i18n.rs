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

    // ── a file row's menu ──────────────────────────────────────────────────
    FileMenuOpenPreview,
    FileMenuCopyPath,
    /// The widest row, and therefore the one that decides the menu's width.
    FileMenuInsertPath,

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
    AdvancedGroup,
    /// The verb that ends every Advanced group: it puts that page's advanced
    /// rows back, and nothing else on the page.
    ResetAdvanced,
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

            // ── a file row's menu ──────────────────────────────────────────
            Self::FileMenuOpenPreview => pick(lang, "Open preview", "打开预览"),
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
            Self::AdvancedGroup => pick(lang, "Advanced", "高级"),
            Self::ResetAdvanced => pick(lang, "Reset to defaults", "恢复默认值"),
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
    pub const ALL: [Self; 156] = [
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
        Self::FileMenuOpenPreview,
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
    ];

    /// The entries whose two columns are allowed to be the same string.
    ///
    /// Listed by hand, one line per reason, which is what the two pins below
    /// promised when they were written: an entry that is *only* a name has
    /// nothing to translate, and translating it would be inventing a word the
    /// program is not called. Every entry not on this list must differ between
    /// the columns and must carry Chinese in the Chinese one.
    #[cfg(test)]
    const UNTRANSLATED: [Self; 1] = [
        // The program's name. It appears untranslated inside a dozen sentences
        // in this table already; a tab that named it in Chinese would be naming
        // something else.
        Self::FilesViewGit,
    ];
}

// ── the strings that carry a value ─────────────────────────────────────────
//
// A separate family rather than a `Text` variant with holes in it, because these
// return `String` and every one of the table's entries is `&'static str`. Mixing
// the two would make the whole table allocate to serve fifteen of its members.

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
}
