# English copy audit, 7 September 2026

Proposals only. Nothing in this file has been applied; `crates/bt-app/src/i18n.rs`,
`README.md`, `docs/PRIVACY.md`, `SECURITY.md` and the 0.2.2 release draft are
untouched on this branch.

Standard: [`user-facing-copy-guide.md`](user-facing-copy-guide.md) — its ten
principles, its per-surface checklist, and its twenty-four before/after rewrites.
Four further instructions from the user, applied throughout:

- no sentence whose only job is to say what the software will not do;
- no internal mechanism in user-facing text unless the reader needs it to act;
- benefit before mechanism;
- one job per sentence, and no em-dashes in UI strings.

The Chinese column is out of scope here and was not read for judgement; a second
line is auditing it on `docs/zh-copy-audit`.

## 1. Summary

| Surface | Reviewed | Proposed to change |
| --- | --- | --- |
| `crates/bt-app/src/i18n.rs`, every English value | 591 | 83 |
| `README.md`, passage by passage | 28 | 15 |
| `docs/PRIVACY.md`, English half | 9 | 3 |
| `SECURITY.md`, user-facing parts | 6 | 3 |
| `docs/plans/release/release-note-v0.2.2-preview.md`, English half | 15 | 11 |
| **Total** | **649** | **115** |

`docs/shortcuts.md` is generated and is not audited as a surface of its own; the
47 ids that feed it are listed in section 8, and a change to any of them means
re-running `scripts/generate-shortcuts-table.ps1`.

Every proposal was measured against the width it has to live in (section 2). No
proposal in this file breaks one.

### The five most common faults

**1. Mechanism the reader cannot act on** — 14 strings as the primary fault, and
a clause inside a dozen more. The reader is told how the result was produced
rather than what the result is.

> `WebFailCrashSay`: "This page stopped running. Its render process exited."
> The second sentence names a process the reader cannot see, restart, or inspect.
> Proposed: "This page stopped running."

**2. An em-dash inside a UI string** — 21 strings, every one of them in the
proposals. The dash is doing the work of a colon or a full stop, and the user has
ruled it out of UI text.

> `PreviewRefusalBinary`: "No preview — this looks like a binary file".
> Proposed: "No preview: this looks like a binary file".

**3. Length that buys nothing** — 13 strings. Fifteen settings sentences already
stand at the three-line cap; several of them are at the cap because of a clause
that repeats the title or defends the design.

> `ShortcutUndelivered`: "Windows keeps some combinations for itself; one that
> never reaches the box cannot be recorded here". "The box" is this file's word
> for the recorder, and "here" repeats it. Proposed: "Windows keeps some
> combinations for itself. One that never reaches Folio cannot be recorded."

**4. A physical metaphor where the interface has a word** — 10 strings. Things
stand, park, are packed, go away, are heard, and are claimed from Windows.

> `DescQuakeDismiss`: "The summoned terminal goes away when the keyboard moves to
> another window." Proposed: "Hides the summoned terminal when another window
> takes the focus."

**5. Benefit last** — 6 strings, all of them on rows that write into another
tool's configuration. The file being edited is announced before the reason to
edit it, so the row reads as a cost with the payment unstated.

> `DescClaudeHooks`: "Adds hooks to your ~/.claude/settings.json, or
> CLAUDE_CONFIG_DIR, so Claude Code tells this window when it is waiting for you.
> Nothing is written into a project folder." Proposed: "Marks the tab when Claude
> Code is waiting for you. Adds a hook to your ~/.claude/settings.json, or
> CLAUDE_CONFIG_DIR."

The remaining primary reasons: jargon (5), capitalisation and sentence case (3),
two jobs in one sentence (3), mixed terminology (3).

## 2. The widths a proposal has to fit

These are read off the code, not estimated.

**Settings row descriptions — three lines.** `settings.rs` caps a row's sentence
at `ROW_DESC_MAX_LINES = 3` and `no_settings_sentence_needs_a_fourth_line`
measures every row of every page against the design's own control column rather
than the machine's. That test's arithmetic: the dialog is 720 logical px, the row
span is `720 - 2 - (168 + 44 + 4) = 502`, an ordinary row's text column is
`502 - 118 - 16 = 368`, and the test's `measure` is a flat half-em per character
at the 12px description face, so **61 characters to a line and roughly 170 to a
sentence**. A stacked row (`ProfileProgram`, `ProfileArgs`, `QuakeCommand`,
`ProfileEnv`) gets the whole 502, so **83 characters to a line**.

Reproducing that greedy wrap over the shipped table gives fifteen sentences
already at three lines: `DescTurnEndNotifications`, `DescSidebar`,
`DescQuakeRestore`, `DescQuakeHeight`, `DescPowerShellOffer`,
`DescNotifications`, `DescMinimumContrast`, `DescKeyHints`, `DescFocusMode`,
`DescFocusCardHeight`, `DescExplorerMenuNoPackage`, `DescExplorerMenu`,
`DescCopilotHooks`, `DescCodexNotify`, `DescClaudeHooks`. Nine of the fifteen
come down to two lines under these proposals; none goes up.

**Toasts — one line.** Nothing proposed here is longer than what it replaces
except `ContextMenuNoExecutable` (38 to 43 characters) and the three failure
toasts that gain "CLI" or a capital letter.

**First-run card rows — short, and never wrapped.** `first_run.rs` builds a
`RowContent` with no `description_lines` at all: "what a row says is one line by
construction". The card is 440 logical px wide and the longest shipped row line
is 53 characters (`FirstRunRowPowerShell`). Every card-row proposal here is
shorter than the line it replaces.

**Hover hints — 60 characters or so.** `TIP_MAX_WIDTH_LOGICAL_PX` is 360 at an
11px face, so a tip wraps rather than clips; the target is one line. The longest
proposed tip is `FirstRunTipExplorer` at 45 characters.

**Shortcuts page row titles.** The one title that grows is
`ShortcutWindowAddress`, 12 to 32 characters, which is the length of
`ShortcutJumpAttention` ("Jump to the longest waiting pane") already on that
page.

## 3. `crates/bt-app/src/i18n.rs`

Thirteen tables, one per surface, every one of the 591 English values listed. A
row with an empty `proposed` cell and `unchanged` in `reason` was read and left
alone.

### Window chrome, tabs and panes

41 strings reviewed, 2 proposed to change.

| id | current | proposed | reason |
| --- | --- | --- | --- |
| `Settings` | Settings | | unchanged |
| `ToggleSidebar` | Toggle sidebar | | unchanged |
| `Minimize` | Minimize | | unchanged |
| `Maximize` | Maximize | | unchanged |
| `CloseWindow` | Close | | unchanged |
| `RailTabs` | Tabs | | unchanged |
| `RailNewTab` | New tab | | unchanged |
| `ChooseProfile` | Choose a profile | | unchanged |
| `Pin` | Pin | | unchanged |
| `Unpin` | Unpin — a pinned tab closes only after unpinning | Unpin. A pinned tab closes only after unpinning | em-dash |
| `NameSourceManual` | Named by you | | unchanged |
| `NameSourceProgram` | Set by the program | | unchanged |
| `NameSourceCwd` | Working folder | | unchanged |
| `TabTipPinned` | `\n`Pinned — restored next launch | `\n`Pinned. Restored next launch | em-dash |
| `MarkWorking` | Working | | unchanged |
| `MarkWorkingIndeterminate` | Working… | | unchanged |
| `SeatTerminal` | Terminal | | unchanged |
| `SeatFiles` | Files | | unchanged |
| `SeatPreview` | Preview | | unchanged |
| `SeatUnavailable` | Unavailable | | unchanged |
| `PlaceholderSeatNotice` | This pane was saved by a newer version of Folio | | unchanged |
| `PaneMenuSplitWith` | Split with | | unchanged |
| `PaneMenuNewInFolder` | New terminal in folder… | | unchanged |
| `PaneMenuDuplicate` | Duplicate pane | | unchanged |
| `PaneMenuMoveToNewTab` | Move pane to new tab | | unchanged |
| `PaneMenuSplitCaption` | SPLIT | | unchanged |
| `ClosePane` | Close pane | | unchanged |
| `PaneChevronTip` | Split and more | | unchanged |
| `HeadPopOut` | Open in a floating window | | unchanged |
| `PaneMenuMoveToNewWindow` | Move pane to new window | | unchanged |
| `MoveRefusedGone` | That tab or that window closed before the move. Nothing moved. | | unchanged |
| `MoveRefusedAlreadyThere` | The tab is already in that window. | | unchanged |
| `MoveRefusedPaneIsNowATab` | The pane is a tab of this window. | | unchanged |
| `PaneMenuZoom` | Zoom pane | | unchanged |
| `PaneMenuRestore` | Restore pane | | unchanged |
| `PaneMenuMoveToWindow` | Move to window | | unchanged |
| `TabMenuRename` | Rename tab | | unchanged |
| `TabMenuUnpin` | Unpin | | unchanged |
| `TabMenuDuplicate` | Duplicate tab | | unchanged |
| `TabMenuMoveToNewWindow` | Move tab to new window | | unchanged |
| `TabMenuClose` | Close tab | | unchanged |

### Settings: General, Appearance, Terminal, Rendered blocks

117 strings reviewed, 9 proposed to change.

| id | current | proposed | reason |
| --- | --- | --- | --- |
| `CategoryGeneral` | GENERAL | | unchanged |
| `CategoryAppearance` | APPEARANCE | | unchanged |
| `CategorySummonedTerminal` | SUMMONED TERMINAL | | unchanged |
| `CategoryTerminal` | TERMINAL | | unchanged |
| `CategoryRenderedBlocks` | RENDERED BLOCKS | | unchanged |
| `CategoryShortcuts` | SHORTCUTS | | unchanged |
| `NavGeneral` | General | | unchanged |
| `NavAppearance` | Appearance | | unchanged |
| `NavSummonedTerminal` | Summoned terminal | | unchanged |
| `NavTerminal` | Terminal | | unchanged |
| `NavRenderedBlocks` | Rendered blocks | | unchanged |
| `NavShortcuts` | Shortcuts | | unchanged |
| `RowTheme` | Theme | | unchanged |
| `RowCursor` | Cursor | | unchanged |
| `RowFormulas` | Display formulas | | unchanged |
| `RowInlineFormulas` | Inline formulas | | unchanged |
| `RowGitPanel` | Git panel | | unchanged |
| `RowContextMenu` | Explorer context menu | | unchanged |
| `RowTabLayout` | Tab layout | | unchanged |
| `RowSidebar` | Sidebar | | unchanged |
| `RowSplitDirection` | Split direction | | unchanged |
| `RowDefaultProfile` | Default profile | | unchanged |
| `RowLanguage` | Language | | unchanged |
| `RowTerminalFont` | Terminal font | | unchanged |
| `RowFontSize` | Font size | | unchanged |
| `RowPsReadLine` | PSReadLine patch | | unchanged |
| `DescTheme` | Light, dark, or follow your system setting | | unchanged |
| `DescCursor` | The shape the cursor takes in the pane you are typing in. | | unchanged |
| `DescFormulas` | Typesets $$…$$ blocks in command output. Off, the LaTeX source is shown as it was printed. | | unchanged |
| `DescInlineFormulas` | Typesets $…$ in command output. Off, the source is shown as it was printed. | | unchanged |
| `RowUpdateCheck` | Update check | | unchanged |
| `DescUpdateCheck` | Asks the releases page once a day whether a newer version is out. It downloads nothing. | Checks once a day for a new version and names it here. | defensive |
| `DescGitPanel` | Adds a Git page to the files column. Off, Folio never reads a repository. | | unchanged |
| `DescTabLayout` | Whether tabs run along the top of the window or down its side. | | unchanged |
| `DescSidebar` | Expanded keeps the vertical tab strip open beside the terminal. Icons parks it as a narrow strip that opens over the terminal. | Expanded keeps the vertical tab strip open beside the terminal. Icons narrows it to a strip that opens over the terminal. | metaphor |
| `DescSplitDirection` | Where a new pane lands when the split you asked for names no direction of its own. | Which way a pane splits when the action you used names no direction. | too long |
| `DescDefaultProfile` | Which profile a new tab opens, and which one Folio starts with. | | unchanged |
| `DescLanguage` | English, 中文, or follow your system setting | | unchanged |
| `DescTerminalFont` | The font terminal text is drawn in. Tabs, menus and this dialog keep their own. | | unchanged |
| `DescFontSize` | How large terminal text is, before your display's scaling is applied. | | unchanged |
| `RowLightScheme` | Light scheme | | unchanged |
| `RowDarkScheme` | Dark scheme | | unchanged |
| `DescLightScheme` | The palette a light window uses, for terminal text and for the window around it. | | unchanged |
| `DescDarkScheme` | The same for a dark window. Your own scheme files go in %APPDATA%\\Folio\\schemes. | | unchanged |
| `SchemeFileSkipped` | Colour scheme skipped | | unchanged |
| `RowBackgroundImage` | Background image | | unchanged |
| `RowImageFit` | Image fit | | unchanged |
| `RowImageOpacity` | Image opacity | | unchanged |
| `RowBackgroundOpacity` | Background opacity | | unchanged |
| `RowAcrylic` | Acrylic | | unchanged |
| `RowAlwaysOnTop` | Always on top | | unchanged |
| `DescBackgroundImage` | A picture drawn behind the whole window, under every pane. | | unchanged |
| `DescImageFit` | How the picture meets a window that is not its shape: stretched, filled or tiled. | | unchanged |
| `DescImageOpacity` | How much of the picture you see. At 0 the window is drawn without it. | | unchanged |
| `DescBackgroundOpacity` | How much of the desktop shows through panes and the window behind them. Text and menus stay solid. | | unchanged |
| `DescAcrylic` | Blurs whatever sits behind the window. Visible only when background opacity is below 100%. | | unchanged |
| `DescAlwaysOnTop` | This window stays above every other window. | | unchanged |
| `DescAcrylicUnavailable` | This version of Windows does not offer the blur. | | unchanged |
| `DescBackgroundOpacityUnavailable` | This window is drawn opaque and cannot let the desktop through. | | unchanged |
| `OptionImageNone` | None | | unchanged |
| `OptionImageChoose` | Choose… | | unchanged |
| `OptionFitStretch` | Stretch | | unchanged |
| `OptionFitFill` | Fill | | unchanged |
| `OptionFitTile` | Tile | | unchanged |
| `OptionSystem` | System | | unchanged |
| `OptionLight` | Light | | unchanged |
| `OptionDark` | Dark | | unchanged |
| `OptionCursorBar` | Bar | | unchanged |
| `OptionCursorBlock` | Block | | unchanged |
| `OptionCursorUnderline` | Underline | | unchanged |
| `OptionHorizontal` | Horizontal | | unchanged |
| `OptionVertical` | Vertical | | unchanged |
| `OptionOn` | On | | unchanged |
| `OptionOff` | Off | | unchanged |
| `OptionExpanded` | Expanded | | unchanged |
| `OptionIcons` | Icons | | unchanged |
| `OptionSplitAuto` | Auto (longer edge) | | unchanged |
| `OptionSplitRight` | Right | | unchanged |
| `OptionSplitDown` | Down | | unchanged |
| `PsReadLineProbing` | Checking which PSReadLine this machine has | | unchanged |
| `PsReadLineRowGone` | The copy Folio installed is no longer on disk | | unchanged |
| `PsReadLineInviteTitle` | Fix the input line after a resize? | | unchanged |
| `PsReadLineInstall` | Install | | unchanged |
| `PsReadLineNotNow` | Not now | | unchanged |
| `PsReadLineRemovedToast` | Removed. New PowerShell sessions use the module Windows ships | | unchanged |
| `AdvancedGroup` | ADVANCED | | unchanged |
| `ResetAdvanced` | Reset to defaults | | unchanged |
| `OpenReleasesPage` | Open releases page | | unchanged |
| `AddScheme` | Add scheme… | | unchanged |
| `InstallFonts` | Install fonts… | | unchanged |
| `SchemeInUseBroken` | Colour scheme not reloaded | | unchanged |
| `SchemeInUseGone` | Colour scheme not found | | unchanged |
| `SchemeDeleted` | Colour scheme deleted | | unchanged |
| `BackgroundPictureRefused` | Background picture not shown | | unchanged |
| `NavProfiles` | Profiles | | unchanged |
| `CategoryProfiles` | PROFILES | | unchanged |
| `RowTables` | Tables | | unchanged |
| `DescTables` | Draws markdown tables in command output. Off, the pipe characters are shown as they were printed. | | unchanged |
| `RowBlockMaxHeight` | Maximum height | | unchanged |
| `DescBlockMaxHeight` | A rendered block taller than this scrolls inside itself instead of growing. | | unchanged |
| `OptionBlockHeightNone` | No limit | | unchanged |
| `PsReadLineUpdate` | Update | | unchanged |
| `RowScrollback` | Scrollback | | unchanged |
| `DescScrollback` | How many lines each pane keeps. Past that, the oldest lines are dropped. | | unchanged |
| `RowLineWrapping` | Line wrapping | | unchanged |
| `DescLineWrappingOn` | Lines longer than the pane fold at its edge. | | unchanged |
| `DescLineWrappingOff` | Lines longer than the pane run on, and the pane scrolls sideways with Shift+wheel or the bar along its foot. | Lines longer than the pane run on. Scroll sideways with Shift+wheel or the horizontal scrollbar. | metaphor |
| `RowFocusMode` | Cards | | unchanged |
| `DescFocusMode` | The tab strip becomes a column of cards, one per tab, and the tab you pick fills the window whole. Ctrl+Shift+Z turns this same setting. | The tab strip becomes a column of cards, one per tab, and the tab you pick fills the window. Ctrl+Shift+Z does the same. | jargon |
| `RowMinimumContrast` | Minimum contrast | | unchanged |
| `DescMinimumContrast` | Lightens or darkens terminal text until it meets this ratio against the cell behind it. Backgrounds never change, and above Off this overrides the colours a program asked for. | Lightens or darkens terminal text until it meets this ratio against its background. Above Off, it overrides the colours a program asked for. | defensive |
| `RowFocusCardHeight` | Card height | | unchanged |
| `DescFocusCardHeight` | How tall the picture of a tab stands inside its card. Alt+wheel over one of its panes scrolls that picture a row at a time. | How tall the picture of a tab is inside its card. Alt+wheel over a pane scrolls it a row at a time. | too long |
| `RowCopyOnSelect` | Copy on select | | unchanged |
| `DescCopyOnSelect` | Letting go of a selection in a pane writes it to the clipboard. There is no other sign that it happened | Copy selected text to the clipboard when you release the mouse button. | defensive |
| `RowSearchEngine` | Search engine | | unchanged |
| `DescSearchEngine` | Where a web preview searches when what you type in its address bar is not an address. | Which search engine a web preview uses when what you type is not an address. | too long |

### Settings: the Explorer menu row

12 strings reviewed, 3 proposed to change.

| id | current | proposed | reason |
| --- | --- | --- | --- |
| `DescExplorerMenu` | Puts Folio in Explorer's right-click menu, on the first page and under Show more options. The first page registers folio.msix, beside folio.exe, for this account. | Puts Folio in Explorer's right-click menu, on the first page and under Show more options. The first page registers folio.msix for this account. | too long |
| `DescExplorerMenuNoFirstPage` | Puts Folio in Explorer's right-click menu. | | unchanged |
| `DescExplorerMenuNoPackage` | Puts Folio under Show more options in Explorer's menu. folio.msix is not in this folder. It ships in the archive beside folio.exe, and the first page needs it. | | unchanged |
| `DescExplorerFirstPageElsewhere` | The entry on that page points at another folder. Folio puts it back on the next launch that can. | That entry points at another folder. Folio takes it back at the next launch. | too long |
| `ContextMenuVerb` | Open Folio here | | unchanged |
| `ContextMenuAddedToast` | Open Folio here is in Explorer's menu, under Show more options | | unchanged |
| `ContextMenuRemovedToast` | Removed from Explorer's menu | | unchanged |
| `ContextMenuNoExecutable` | Windows did not say where folio.exe is | Folio could not work out where folio.exe is | mechanism |
| `ExplorerCommandVerb` | Open in Folio | | unchanged |
| `ExplorerFirstPageAddedToast` | Open in Folio is on the first page of Explorer's menu | | unchanged |
| `ExplorerFirstPageNoPackage` | folio.msix is not beside folio.exe | | unchanged |
| `MenuRevealInExplorer` | Reveal in Explorer | | unchanged |

### Settings: Summoned terminal

22 strings reviewed, 8 proposed to change.

| id | current | proposed | reason |
| --- | --- | --- | --- |
| `ShortcutSummonQuake` | Summon the terminal | | unchanged |
| `RowQuakeHeight` | Summoned terminal height | | unchanged |
| `DescQuakeHeight` | How much of the height of the screen the pointer is on the summoned terminal covers. It hangs from the top of that screen. | How much of the screen's height the summoned terminal covers. It hangs from the top of the screen the pointer is on. | two jobs |
| `RowQuakeWidth` | Summoned terminal width | | unchanged |
| `DescQuakeWidth` | How much of the width of that screen it covers. It is centred in the rest. | How much of the screen's width the summoned terminal covers. It stays centred. | two jobs |
| `DescQuakeHotkeyTaken` | Another program is already using the key that summons it. | Another program is already using this key. | too long |
| `RowQuakeHotkey` | Summon key | | unchanged |
| `DescQuakeHotkey` | The key that calls the terminal down. It is claimed from Windows, so it works while another program has the keyboard. | Shows and hides the summoned terminal, from inside any other program. | mechanism |
| `RowQuakeProfile` | Profile | | unchanged |
| `DescQuakeProfile` | Which shell a new tab in the summoned terminal starts. | | unchanged |
| `OptionQuakeProfileDefault` | Default profile | | unchanged |
| `RowQuakeCommand` | Command on first summon | | unchanged |
| `DescQuakeCommand` | This command is run once each time Folio starts, on the first summon. Nothing else the summoned terminal restores is run. | Runs once, the first time you summon the terminal after starting Folio. | defensive |
| `RowQuakeTopGap` | Gap above it | | unchanged |
| `DescQuakeTopGap` | How far below the top of that screen it hangs, in pixels. | How far below the top of the screen the summoned terminal hangs, in pixels. | two jobs |
| `RowQuakeRestore` | What comes back | | unchanged |
| `DescQuakeRestore` | What a new run of Folio puts back into the summoned terminal. A restored command is typed at the prompt and not run, and only a shell that reports its commands has one to restore. | What a new run of Folio puts back into the summoned terminal. A restored command is typed at the prompt, not run, and needs shell integration. | too long |
| `OptionQuakeRestoreNothing` | Nothing | | unchanged |
| `OptionQuakeRestoreFolders` | Tabs and folders | | unchanged |
| `OptionQuakeRestoreFoldersAndCommands` | Tabs, folders, and a pinned tab's last command typed at its prompt | | unchanged |
| `RowQuakeDismiss` | Hide it when it loses focus | | unchanged |
| `DescQuakeDismiss` | The summoned terminal goes away when the keyboard moves to another window. | Hides the summoned terminal when another window takes the focus. | metaphor |

### Settings: Agents, notifications and the PowerShell notice

34 strings reviewed, 16 proposed to change.

| id | current | proposed | reason |
| --- | --- | --- | --- |
| `RowNotifications` | Notifications | | unchanged |
| `DescNotifications` | A program that asks for one can put a message on your desktop. Nothing appears while its pane is on screen in the focused window. | Lets a program put a message on your desktop. Nothing appears while its pane is on screen in the focused window. | benefit-last |
| `RowTurnEndNotifications` | Turn finished | | unchanged |
| `DescTurnEndNotifications` | When an agent finishes a turn and the window is out of sight, its taskbar button flashes, or a message goes to your desktop if it is minimised. Marks inside the window are unaffected. | Flashes the taskbar when an agent finishes a turn and the window is out of sight, or sends a desktop message if it is minimised. | defensive |
| `ToastTurnFinished` | Turn finished | | unchanged |
| `ToastWaitingForYou` | Waiting for you | | unchanged |
| `NotifyRefusedTitle` | Desktop notification refused | | unchanged |
| `NotifyRefusedBody` | Windows would not take it. The marks inside this window are unaffected. | Windows would not take it. Tab marks still work. | too long |
| `PowerShellNoticeBody` | PowerShell integration is not installed. It marks commands, follows the current directory and typesets inline $…$ formulas in output. | | unchanged |
| `PowerShellNoticeAdd` | Add to $PROFILE | | unchanged |
| `PowerShellNoticeNever` | Don't show again | | unchanged |
| `PowerShellNoticeAdded` | Added to $PROFILE. Takes effect in a new shell. | | unchanged |
| `RowPowerShellOffer` | Offer PowerShell integration | | unchanged |
| `DescPowerShellOffer` | A PowerShell pane whose $PROFILE does not load folio.ps1 offers to add it. Integration marks commands, follows the directory and typesets inline $…$ formulas. | A PowerShell pane that has no integration offers to add it. Integration marks commands, follows the directory and typesets inline $…$ formulas. | mechanism |
| `RowClaudeHooks` | Claude Code hooks | | unchanged |
| `DescClaudeHooks` | Adds hooks to your ~/.claude/settings.json, or CLAUDE_CONFIG_DIR, so Claude Code tells this window when it is waiting for you. Nothing is written into a project folder. | Marks the tab when Claude Code is waiting for you. Adds a hook to your ~/.claude/settings.json, or CLAUDE_CONFIG_DIR. | benefit-last |
| `ClaudeHooksAddedToast` | Added to ~/.claude/settings.json. Takes effect in a new Claude Code session. | Hook added. Takes effect in a new Claude Code session. | mechanism |
| `ClaudeHooksRemovedToast` | Removed from ~/.claude/settings.json | Claude Code hook removed | mechanism |
| `ClaudeHooksFailedToast` | Claude Code's settings were not changed | | unchanged |
| `CategoryAgents` | AGENTS | | unchanged |
| `NavAgents` | Agents | | unchanged |
| `RowCodexNotify` | Codex notify | | unchanged |
| `DescCodexNotify` | Adds a notify program to your ~/.codex/config.toml, or CODEX_HOME, so codex tells this window when a turn has ended. It does not report a codex waiting for you. | Marks the tab when Codex finishes a turn. Adds a notify program to your ~/.codex/config.toml, or CODEX_HOME. | benefit-last |
| `CodexNotifyAddedToast` | Added to ~/.codex/config.toml. Takes effect in a new codex session. | Notify program added. Takes effect in a new Codex session. | mechanism |
| `CodexNotifyRemovedToast` | Removed from ~/.codex/config.toml | Codex notify program removed | mechanism |
| `CodexNotifyFailedToast` | codex's configuration was not changed | Codex's configuration was not changed | capitalisation |
| `RowCopilotHooks` | Copilot CLI hooks | | unchanged |
| `DescCopilotHooks` | Adds a hook file to your ~/.copilot/hooks/, or COPILOT_HOME, so Copilot CLI tells this window when it is waiting for you. Nothing is written into a project folder. | Marks the tab when Copilot CLI is waiting for you. Adds a hook file to your ~/.copilot/hooks/, or COPILOT_HOME. | benefit-last |
| `DescCopilotHooksTooOld` | This machine's Copilot CLI is older than 1.0.26. Earlier versions report permission prompts you were never shown. | | unchanged |
| `DescCopilotHooksDisabled` | Hooks are switched off in your ~/.copilot/settings.json, so a file installed here would not run. | | unchanged |
| `CopilotHooksAddedToast` | Added to ~/.copilot/hooks/folio.json. Takes effect in a new copilot session. | Hook added. Takes effect in a new Copilot CLI session. | mechanism |
| `CopilotHooksRemovedToast` | Removed ~/.copilot/hooks/folio.json | Copilot CLI hook removed | mechanism |
| `CopilotHooksFailedToast` | copilot's hooks were not changed | Copilot CLI's hooks were not changed | capitalisation |
| `ShellIntegrationPending` | Joins the next PowerShell that starts | Takes effect in the next PowerShell session | metaphor |

### Settings: Profiles

69 strings reviewed, 4 proposed to change.

| id | current | proposed | reason |
| --- | --- | --- | --- |
| `ProfilesDuplicate` | Duplicate | | unchanged |
| `ProfilesBadgeDefault` | default | | unchanged |
| `ProfilesBadgeDefaultAutomatic` | automatic default | | unchanged |
| `ProfilesBadgeHidden` | hidden | | unchanged |
| `ProfilesAgentInsideWsl` | An agent installed inside WSL can be started from the WSL profile, or from a new profile with program wsl.exe and arguments -e <command> | | unchanged |
| `CapFull` | Prompt marks, directory, exit codes and hyperlinks | | unchanged |
| `CapPowerShell` | Prompt marks, directory, exit codes and hyperlinks — with folio.ps1 dot-sourced | Prompt marks, directory, exit codes and hyperlinks, with folio.ps1 dot-sourced | em-dash |
| `CapWslBash` | Prompt marks, directory, exit codes and hyperlinks — on a bash login only | Prompt marks, directory, exit codes and hyperlinks, on a bash login only | em-dash |
| `CapCmd` | Directory and hyperlinks; no prompt marks, no exit codes | | unchanged |
| `CapNone` | No shell integration | | unchanged |
| `ProfilesEdit` | Edit | | unchanged |
| `ProfilesHide` | Hide | | unchanged |
| `ProfilesShow` | Show | | unchanged |
| `ProfilesDelete` | Delete | | unchanged |
| `ProfilesSetDefault` | Set as default | | unchanged |
| `ProfilesAlreadyDefault` | Already the default | | unchanged |
| `ProfilesNew` | New profile | | unchanged |
| `ProfilesRowName` | Name | | unchanged |
| `ProfilesRowNameDesc` | What this profile is called on tabs, in the profile picker and in this list. | | unchanged |
| `ProfilesRowProgram` | Program | | unchanged |
| `ProfilesRowProgramDesc` | The program a new tab of this profile starts. | | unchanged |
| `ProfilesRowStartingDir` | Starting directory | | unchanged |
| `ProfilesRowStartingDirDesc` | The folder a new tab of this profile opens in. | | unchanged |
| `ProfilesRowColour` | Colour | | unchanged |
| `ProfilesRowColourDesc` | The colour that marks this profile on tabs and in menus. | | unchanged |
| `ProfilesInherit` | The current pane's folder | | unchanged |
| `ProfilesHome` | Home | | unchanged |
| `ProfilesChooseFolder` | Choose a folder… | | unchanged |
| `ProfilesColourBlue` | Blue | | unchanged |
| `ProfilesColourTeal` | Teal | | unchanged |
| `ProfilesColourGreen` | Green | | unchanged |
| `ProfilesColourAmber` | Amber | | unchanged |
| `ProfilesColourRed` | Red | | unchanged |
| `ProfilesColourMagenta` | Magenta | | unchanged |
| `ProfilesColourViolet` | Violet | | unchanged |
| `ProfilesColourSlate` | Slate | | unchanged |
| `ProfilesColourInherited` | Inherited | | unchanged |
| `ProfilesColourFixed` | Its own | | unchanged |
| `ProfilesRowArgs` | Arguments | | unchanged |
| `ProfilesRowArgsDesc` | Passed to the program before it reads anything of its own. Spaces separate; double quotes group. e.g. -NoExit -File D:\\me\\start.ps1 | Passed to the program when it starts. Spaces separate; double quotes group. e.g. -NoExit -File D:\\me\\start.ps1 | mechanism |
| `ProfilesRowEnv` | Environment | | unchanged |
| `ProfilesRowEnvDesc` | Set for every session this profile starts, over what Folio sets itself. | | unchanged |
| `ProfilesRowHyperlink` | Force hyperlinks | | unchanged |
| `ProfilesRowHyperlinkDesc` | Sets FORCE_HYPERLINK, which programs read before deciding to emit a link. | | unchanged |
| `ProfilesRowIntegration` | Shell integration | | unchanged |
| `ProfilesEnvAdd` | Add | | unchanged |
| `ProfilesEnvName` | Name | | unchanged |
| `ProfilesEnvValue` | Value | | unchanged |
| `ProfilesAuto` | Auto | | unchanged |
| `ProfilesOn` | On | | unchanged |
| `ProfilesOff` | Off | | unchanged |
| `ProfilesRestoreAll` | Restore all defaults | | unchanged |
| `ProfilesDeleteBtn` | Delete profile | | unchanged |
| `ProfilesBrowse` | Browse… | | unchanged |
| `ProfilesCannotHideDefault` | The default profile stays in the picker | | unchanged |
| `ProfilesCannotHideFallback` | Folio falls back to this profile when another cannot start | | unchanged |
| `ProfilesNameBlank` | A profile needs a name | | unchanged |
| `ProfilesNameTaken` | Another profile is already called this | | unchanged |
| `ProfilesUndo` | Undo | | unchanged |
| `ProfilesIntegrationPowerShell` | PowerShell script | | unchanged |
| `ProfilesIntegrationBash` | Bash init file | | unchanged |
| `ProfilesIntegrationCmd` | Command Prompt | | unchanged |
| `ProfilesIntegrationNone` | None | | unchanged |
| `CapFullNoLinks` | Prompt marks, directory and exit codes; no hyperlinks | | unchanged |
| `CapPowerShellNoLinks` | Prompt marks, directory and exit codes with folio.ps1 dot-sourced; no hyperlinks | | unchanged |
| `CapWslBashNoLinks` | Prompt marks, directory and exit codes on a bash login only; no hyperlinks | | unchanged |
| `CapCmdNoLinks` | Directory; no prompt marks, no exit codes, no hyperlinks | | unchanged |
| `CapNoneLong` | No prompt marks, no directory, no exit codes. Hyperlinks are declared anyway | No prompt marks, no directory, no exit codes. Hyperlinks still work | jargon |
| `CapNoneLongNoLinks` | No prompt marks, no directory, no exit codes, no hyperlinks | | unchanged |

### Settings: Shortcuts page

59 strings reviewed, 5 proposed to change.

| id | current | proposed | reason |
| --- | --- | --- | --- |
| `ShortcutGotoTab1` | Go to tab 1 | | unchanged |
| `ShortcutGotoTab2` | Go to tab 2 | | unchanged |
| `ShortcutGotoTab3` | Go to tab 3 | | unchanged |
| `ShortcutGotoTab4` | Go to tab 4 | | unchanged |
| `ShortcutGotoTab5` | Go to tab 5 | | unchanged |
| `ShortcutGotoTab6` | Go to tab 6 | | unchanged |
| `ShortcutGotoTab7` | Go to tab 7 | | unchanged |
| `ShortcutGotoTab8` | Go to tab 8 | | unchanged |
| `ShortcutGotoTab9` | Go to tab 9 | | unchanged |
| `ShortcutNextTab` | Next tab | | unchanged |
| `ShortcutPrevTab` | Previous tab | | unchanged |
| `ShortcutReopenClosed` | Reopen the last closed tab | | unchanged |
| `ShortcutJumpAttention` | Jump to the longest waiting pane | | unchanged |
| `ShortcutCommandPalette` | Command palette | | unchanged |
| `ShortcutSplitHorizontal` | Split horizontally | | unchanged |
| `ShortcutSplitVertical` | Split vertically | | unchanged |
| `ShortcutDuplicatePaneSplit` | Duplicate pane into a split | | unchanged |
| `ShortcutFilesPane` | Files column | | unchanged |
| `ShortcutGitPage` | Turn the files column to Git | Show Git in the files column | metaphor |
| `ShortcutSavePreview` | Save the open document | | unchanged |
| `ShortcutPrevCommandMark` | Previous command | | unchanged |
| `ShortcutNextCommandMark` | Next command | | unchanged |
| `ShortcutOpenSearch` | Find in this pane | | unchanged |
| `ShortcutNextMatch` | Next match | | unchanged |
| `ShortcutPrevMatch` | Previous match | | unchanged |
| `ShortcutSummonPip1` | Summon picture in picture 1 | | unchanged |
| `ShortcutSummonPip2` | Summon picture in picture 2 | | unchanged |
| `ShortcutSummonPip3` | Summon picture in picture 3 | | unchanged |
| `ShortcutSummonPip4` | Summon picture in picture 4 | | unchanged |
| `ShortcutFamilyGotoTab` | Go to tab 1–9 | | unchanged |
| `ShortcutScopePreview` | In a preview | | unchanged |
| `ShortcutScopeTerminalPrimary` | On a terminal's own scrollback | | unchanged |
| `ShortcutScopeSearchOpen` | While the search is open | | unchanged |
| `ShortcutNotePending` | This feature is not built yet | | unchanged |
| `ShortcutNoteOnePerMember` | One chord for each | | unchanged |
| `ShortcutNoteNoneAssigned` | One chord for each; none is set yet | | unchanged |
| `ShortcutNoteSomeUnassigned` | One chord for each; some are not set yet | | unchanged |
| `ShortcutUnbound` | Not set | | unchanged |
| `ShortcutReservedMoveFocus` | Move the focus between panes | | unchanged |
| `ShortcutReservedResizePane` | Resize a pane | | unchanged |
| `ShortcutReservedAltArrow` | Reserved — readline reads Alt+arrow as word movement | Reserved: readline reads Alt+arrow as word movement | em-dash |
| `ShortcutHintAltGrZone` | Ctrl+Alt is reserved for AltGr keyboards | | unchanged |
| `ShortcutHintShellControlLetter` | Ctrl+letter belongs to the shell | | unchanged |
| `ShortcutQuit` | Quit | | unchanged |
| `ShortcutWindowAddress` | Open address | Open an address in a new preview | too long |
| `RowKeyHints` | Shortcut hints | | unchanged |
| `DescKeyHints` | Hold a modifier for a moment and this window lists the shortcuts that start with it. The list never takes a keystroke. | Hold a modifier for a moment and this window lists the shortcuts that start with it. | defensive |
| `ShortcutNewWindow` | New window | | unchanged |
| `ShortcutWebAddress` | Address | | unchanged |
| `ShortcutWebDevTools` | Developer tools | | unchanged |
| `ShortcutCloseSearch` | Close search | | unchanged |
| `ShortcutScopeWebPage` | On a page | | unchanged |
| `ShortcutScopeSearchHost` | Where there is text to search | | unchanged |
| `ShortcutRecordPrompt` | Enter keeps it · Esc cancels · Del clears | | unchanged |
| `ShortcutRecordUnusable` | That combination cannot be saved | | unchanged |
| `ShortcutUndelivered` | Windows keeps some combinations for itself; one that never reaches the box cannot be recorded here | Windows keeps some combinations for itself. One that never reaches Folio cannot be recorded. | metaphor |
| `ShortcutRecord` | Record | | unchanged |
| `ShortcutRecordListening` | Press… | | unchanged |
| `ShortcutRestoreAll` | Restore all defaults | | unchanged |

### Files column, root menu and file menus

38 strings reviewed, 3 proposed to change.

| id | current | proposed | reason |
| --- | --- | --- | --- |
| `ProfileHintDefault` | default | | unchanged |
| `ProfileHintUnavailable` | not installed | | unchanged |
| `ProfileHintCurrent` | current | | unchanged |
| `ProfileFilesPane` | Files pane | | unchanged |
| `ProfileFilesPaneHint` | this tab | | unchanged |
| `ProfileRecentSection` | RECENTLY OPENED | | unchanged |
| `RootSection` | OPEN FOLDER | | unchanged |
| `RootBrowse` | Browse… | | unchanged |
| `RootNoteHome` | home | | unchanged |
| `RootNoteTerminal` | a terminal is here | | unchanged |
| `RootNoteRecent` | recent | | unchanged |
| `RootNoteParent` | parent | | unchanged |
| `PinnedSection` | PINNED | | unchanged |
| `FileMenuOpenDefaultApp` | Open in default app | | unchanged |
| `FileMenuCopyPath` | Copy path | | unchanged |
| `FileMenuInsertPath` | Insert path into terminal | | unchanged |
| `FilesLoading` | Loading… | | unchanged |
| `FilesEmpty` | Empty folder | | unchanged |
| `FilesUnrooted` | No folder opened | | unchanged |
| `FilesPermissionDenied` | Permission denied | | unchanged |
| `FilesNotFound` | Folder not found | | unchanged |
| `FilesUnreadable` | Could not read folder | | unchanged |
| `FilesRevealed` | Revealed in File Explorer | | unchanged |
| `FloatDock` | DOCK | | unchanged |
| `FloatTriggerTip` | Peek files here | | unchanged |
| `FilesProgramRefused` | The files tree does not run programs | The files column does not run programs | mixed terminology |
| `FilesWorkerStopped` | Directory reading stopped; terminal input and output remain available | Directory reading stopped. The terminal is unaffected | too long |
| `DragSwapPanes` | Swap panes | | unchanged |
| `DragReplacePane` | Replace pane | | unchanged |
| `FilesViewFiles` | Files | | unchanged |
| `FilesViewGit` | Git | | unchanged |
| `FileMenuOpenWith` | Open with default app | | unchanged |
| `FolderMenuExpand` | Expand | | unchanged |
| `FolderMenuCollapse` | Collapse | | unchanged |
| `FolderMenuNewTerminal` | New terminal here | | unchanged |
| `DragOpenInPreview` | Open in this preview | | unchanged |
| `DragRootTreeHere` | Root this tree here | Root the files column here | mixed terminology |
| `FileMenuRename` | Rename | | unchanged |

### Preview pane, peek card and the web preview

52 strings reviewed, 13 proposed to change.

| id | current | proposed | reason |
| --- | --- | --- | --- |
| `FileMenuOpenPreview` | Open preview | | unchanged |
| `PreviewRailOpen` | Open | | unchanged |
| `PreviewRailOpenTip` | Open and more | | unchanged |
| `PreviewCopyAddress` | Copy address | | unchanged |
| `PreviewFlipToSource` | View source | | unchanged |
| `VideoFormatCannotPlay` | This format cannot be played | | unchanged |
| `PreviewFlipToRendered` | View rendered | | unchanged |
| `PreviewFlipToPage` | View page | | unchanged |
| `PreviewEmptyState` | Click a path with a dashed underline to preview it here | | unchanged |
| `PreviewRefusalType` | No preview for this file type | | unchanged |
| `PreviewRefusalBinary` | No preview — this looks like a binary file | No preview: this looks like a binary file | em-dash |
| `PreviewRefusalNetworkPath` | No preview — network paths are not read automatically | No preview: network paths are not read automatically | em-dash |
| `PreviewRefusalPermissionDenied` | No preview — permission denied | No preview: permission denied | em-dash |
| `PreviewRefusalNotFound` | No preview — file not found | No preview: file not found | em-dash |
| `PreviewRefusalUnreadable` | No preview — could not read this file | No preview: could not read this file | em-dash |
| `PreviewOpenExternally` | Open in default app | | unchanged |
| `PreviewOpened` | Opened \u{2713} | | unchanged |
| `PreviewTruncated` | Read-only · 64 KB | | unchanged |
| `PreviewSaved` | Saved | | unchanged |
| `PreviewConflict` | Not saved · changed on disk · edits kept | | unchanged |
| `PreviewNothingToSave` | there is nothing to save | | unchanged |
| `PreviewFailedImageLoad` | Preview failed: image could not be loaded | | unchanged |
| `PreviewFailedImageWorker` | Preview failed: this picture could not be drawn | Preview failed: this image could not be drawn | mixed terminology |
| `PreviewFailedSeatTooSmall` | Preview failed: the preview pane is too small | | unchanged |
| `PreviewWorkerStopped` | File preview reading stopped; terminal input and output remain available | File preview reading stopped. The terminal is unaffected | too long |
| `PeekFoot` | Enter / double-click opens the preview pane | | unchanged |
| `PeekUnknown` | No preview — binary or unrecognized type. | No preview: binary or unrecognised type. | em-dash |
| `PeekFileGone` | The file is no longer there. | | unchanged |
| `PeekOpensAsPage` | Opens as a page. | | unchanged |
| `PreviewLock` | Lock this pane — what opens next opens in a new preview | Lock this pane. The next file opens in a new preview | em-dash |
| `PreviewUnlock` | Unlock — this pane becomes the reusable preview again | Unlock. This pane becomes the reusable preview again | em-dash |
| `PreviewStopPlaying` | Stop playing | | unchanged |
| `MarkdownImageUnreadable` | This image did not open | | unchanged |
| `MarkdownImageRemote` | Folio has no network client, so this image is not fetched | Images from the web are not fetched in a preview | mechanism |
| `PreviewDiskChanged` | This file changed on disk. Your unsaved edits are still here. | | unchanged |
| `PreviewDiskReload` | Reload | | unchanged |
| `PreviewDiskKeep` | Keep my edits | | unchanged |
| `PreviewDiskDeleted` | This file was deleted. What you are reading is still here. | | unchanged |
| `PreviewOpenInBrowser` | Open in browser | | unchanged |
| `PreviewWebBack` | Back | | unchanged |
| `PreviewWebForward` | Forward | | unchanged |
| `PreviewWebReload` | Reload | | unchanged |
| `PreviewWebStop` | Stop | | unchanged |
| `WebFailRuntimeSay` | Microsoft Edge WebView2 Runtime is not installed. | | unchanged |
| `WebFailRuntimeVerb` | Download the runtime | | unchanged |
| `WebFailEngineSay` | The web engine did not start. | | unchanged |
| `WebFailEngineVerb` | Retry | | unchanged |
| `WebFailCrashSay` | This page stopped running. Its render process exited. | This page stopped running. | mechanism |
| `WebFailBlockedSay` | This address does not open in a preview. | | unchanged |
| `WebFailBlockedVerb` | Copy address | | unchanged |
| `WebFailDownloadSay` | This download cannot be handed to your browser. The request carried data a plain link cannot replay. | Start this download in your browser instead. | mechanism |
| `WebFailDownloadVerb` | Open this page in your browser | | unchanged |

### Git page and commit graph

78 strings reviewed, 11 proposed to change.

| id | current | proposed | reason |
| --- | --- | --- | --- |
| `GitNotARepository` | Not a git repository | | unchanged |
| `GitReading` | Reading the repository… | | unchanged |
| `GitToastTitle` | Git | | unchanged |
| `GitUnborn` | no commits yet | | unchanged |
| `GitDetached` | detached HEAD | | unchanged |
| `GitLoadMore` | Load more | | unchanged |
| `GitCommitNoFiles` | No files against the first parent | | unchanged |
| `GitGroupStaged` | STAGED | | unchanged |
| `GitGroupChanges` | CHANGES | | unchanged |
| `GitGroupUntracked` | UNTRACKED | | unchanged |
| `GitGroupStagedTip` | Packed for the next commit — 'git commit' ships exactly these | What the next git commit will ship | metaphor |
| `GitGroupChangesTip` | Edited but not packed yet | Edited but not staged yet | metaphor |
| `GitGroupUntrackedTip` | Not in the repository yet — git is not watching these | Not in the repository yet. git does not track these | em-dash |
| `GitBranchesHeading` | BRANCHES | | unchanged |
| `GitRemotesHeading` | REMOTES | | unchanged |
| `GitRemotesTipShut` | Branches on remotes — click to show them | Branches on remotes. Click to show them | em-dash |
| `GitRemotesTipOpen` | Branches on remotes — click to fold them away | Branches on remotes. Click to hide them | em-dash |
| `GitCommitsHeading` | COMMITS | | unchanged |
| `GitRefreshTip` | Read the repository again | | unchanged |
| `GitActStage` | Stage | | unchanged |
| `GitActUnstage` | Unstage | | unchanged |
| `GitActDeleteFile` | Delete this file | | unchanged |
| `GitActDiscardChanges` | Discard changes | | unchanged |
| `GitActStageAll` | Stage all | | unchanged |
| `GitActUnstageAll` | Unstage all | | unchanged |
| `GitActLoadMore` | Load fifty more commits | | unchanged |
| `GitActOpenGraph` | Open the full commit graph | | unchanged |
| `GitNoCommits` | No commits yet | | unchanged |
| `GitFaultTimedOut` | git did not answer and was stopped | | unchanged |
| `GitWorkerStopped` | Git reading stopped; terminal input and output remain available | Git reading stopped. The terminal is unaffected | too long |
| `GitNotFound` | git.exe was not found on this machine — install Git for Windows to use this page | git.exe was not found. Install Git for Windows to use this page | em-dash |
| `GraphMetaParents` | parents: | | unchanged |
| `GraphMetaCommittedBy` | committed by | | unchanged |
| `GraphCompareWorkingTree` | working tree | | unchanged |
| `GraphHeadingGraph` | GRAPH | | unchanged |
| `GraphHeadingDescription` | DESCRIPTION | | unchanged |
| `GraphHeadingAuthor` | AUTHOR | | unchanged |
| `GraphHeadingDate` | DATE | | unchanged |
| `GraphHeadingCommit` | COMMIT | | unchanged |
| `GraphUncommitted` | Uncommitted Changes | Uncommitted changes | capitalisation |
| `GraphUncommittedTime` | now | | unchanged |
| `GraphSearchPlaceholder` | Search commits | | unchanged |
| `GraphSearchNone` | no matches | | unchanged |
| `GraphFilterAll` | All branches | | unchanged |
| `GraphToolFilterTip` | Which branches this graph is of | | unchanged |
| `GraphToolSearchTip` | Search commits by message, author or hash | | unchanged |
| `GraphToolSearchClearTip` | Clear the search | | unchanged |
| `GraphFileBinary` | Binary — git has no lines to count here | Binary. git counts no lines here | em-dash |
| `GraphMergeCommit` | Merge commit — another branch's history joins here | Merge commit. Another branch's history joins here | em-dash |
| `GraphDoubleClickCheckout` | Double-click to check this commit out | | unchanged |
| `GraphClickToList` | Click to list them | | unchanged |
| `GitMenuCheckout` | Checkout | | unchanged |
| `GitMenuCreateBranch` | Create branch here… | | unchanged |
| `GitMenuCreateTag` | Add tag here… | | unchanged |
| `GitMenuRename` | Rename… | | unchanged |
| `GitMenuDeleteTag` | Delete tag | | unchanged |
| `GitMenuCheckoutTracking` | Checkout as local branch | | unchanged |
| `GitMenuOpenDiff` | Open diff | | unchanged |
| `GitMenuCopyHash` | Copy hash | | unchanged |
| `GitMenuCopySubject` | Copy subject | | unchanged |
| `GitMenuCopyName` | Copy name | | unchanged |
| `GitMenuCompareSelected` | Compare with selected | | unchanged |
| `GitMenuCompareWorking` | Compare with working tree | | unchanged |
| `GitFilterShowRemotes` | Show remote branches | | unchanged |
| `GitFilterShowTags` | Show tags | | unchanged |
| `GitPromptBranchName` | Branch name | | unchanged |
| `GitPromptTagName` | Tag name | | unchanged |
| `GitPromptNewName` | New name | | unchanged |
| `RefNameEmpty` | A name is needed. | | unchanged |
| `RefNameSpace` | No spaces. | | unchanged |
| `RefNameRange` | No `..`. | | unchanged |
| `RefNameReserved` | None of ~ ^ : ? * [ \\ | | unchanged |
| `RefNameDash` | Cannot start with `-`. | | unchanged |
| `RefNameLock` | Cannot end with `.lock`. | | unchanged |
| `RefNameShape` | git will not accept this name. | | unchanged |
| `GitDocumentEmpty` | No changes to show | | unchanged |
| `GraphToolLeaveDetachedTip` | HEAD is on no branch — stand on one again | HEAD is on no branch. Check one out again | em-dash |
| `GraphLeaveDetached` | Back to | | unchanged |

### Terminal overlays, search and the command palette

31 strings reviewed, 1 proposed to change.

| id | current | proposed | reason |
| --- | --- | --- | --- |
| `MathWorkerStopped` | Formula rendering stopped; terminal input and output remain available | Formula rendering stopped. The terminal is unaffected | too long |
| `HyperlinkBlockedSuffix` |  · blocked | | unchanged |
| `HyperlinkBlocked` | blocked | | unchanged |
| `PaletteFieldPlaceholder` | Type a pane, a command, a file or a setting… | | unchanged |
| `PaletteSectionActions` | Actions | | unchanged |
| `PaletteSectionPlaces` | Tabs and panes | | unchanged |
| `PaletteSectionCommands` | Commands | | unchanged |
| `PaletteSectionFiles` | Files | | unchanged |
| `PaletteSectionSettings` | Settings | | unchanged |
| `PaletteIndexing` | Indexing… | | unchanged |
| `PaletteFilesTruncated` | This folder holds more files than are indexed | | unchanged |
| `PaletteNoMatches` | Nothing matches | | unchanged |
| `RailPeekEmptyCommand` | command | | unchanged |
| `RailPeekEmptyLine` | line | | unchanged |
| `SearchPlaceholder` | Find | | unchanged |
| `SearchTipCase` | Match case | | unchanged |
| `SearchTipWord` | Whole word | | unchanged |
| `SearchTipRegex` | Regular expression | | unchanged |
| `SearchTipPrevious` | Previous match (Shift+Enter) | | unchanged |
| `SearchTipNext` | Next match (Enter) | | unchanged |
| `SearchTipClose` | Close (Esc) | | unchanged |
| `TermMenuCopy` | Copy | | unchanged |
| `TermMenuPaste` | Paste | | unchanged |
| `TermMenuSelectAll` | Select all | | unchanged |
| `TermMenuFind` | Find… | | unchanged |
| `TermMenuClearScreen` | Clear screen | | unchanged |
| `TermMenuClearScrollback` | Clear scrollback… | | unchanged |
| `TermMenuShellAgain` | Restart shell… | | unchanged |
| `CardGestureHint` | scroll a card | | unchanged |
| `HyperlinkControlOpensExternally` |  · Ctrl+click opens in default app | | unchanged |
| `HyperlinkControlReveals` |  · Ctrl+click shows it in Explorer | | unchanged |

### Restore, quit and confirmation gates

21 strings reviewed, 2 proposed to change.

| id | current | proposed | reason |
| --- | --- | --- | --- |
| `RestoreTitle` | Reopen your other tabs? | | unchanged |
| `RestoreSub` | These were open when you last closed Folio. They come back in the folders you left them, as new shells. | | unchanged |
| `RestoreDecline` | No thanks | | unchanged |
| `RestoreAccept` | Restore | | unchanged |
| `GateDiscard` | Discard | | unchanged |
| `GateCancel` | Cancel | | unchanged |
| `GateDelete` | Delete | | unchanged |
| `GateClear` | Clear | | unchanged |
| `GateTitleUnsaved` | Discard unsaved changes? | | unchanged |
| `GateTitleGitDiscard` | Discard changes? | | unchanged |
| `GateTitleGitDelete` | Delete this file? | | unchanged |
| `GateTitleGitDeleteBranch` | Delete this branch? | | unchanged |
| `GateTitleGitDeleteTag` | Delete this tag? | | unchanged |
| `GateTitleClearScrollback` | Clear scrollback? | | unchanged |
| `QuitTitle` | Save changes before quitting? | | unchanged |
| `QuitSave` | Save all | | unchanged |
| `QuitSessionNotWritten` | session.json could not be written. Nothing was closed. | | unchanged |
| `GateTitleGitDetach` | Stand on this commit? | Check out this commit? | metaphor |
| `GateTitleGitDirtyCheckout` | Move the working tree? | Check out with uncommitted changes? | metaphor |
| `GateCheckout` | Checkout | | unchanged |
| `GateDiscardAll` | Discard all | | unchanged |

### First-run card

17 strings reviewed, 6 proposed to change.

| id | current | proposed | reason |
| --- | --- | --- | --- |
| `FirstRunTitle` | Welcome to Folio | | unchanged |
| `FirstRunSettingsLine` | You can change these options in Settings. | | unchanged |
| `FirstRunLater` | Not now | | unchanged |
| `FirstRunDone` | Done | | unchanged |
| `FirstRunRowUpdate` | Get told when a new version of Folio is out | Check for Folio updates | benefit-last |
| `FirstRunRowExplorer11` | Open any folder in Folio from its right-click menu | | unchanged |
| `FirstRunRowExplorer10` | Open any folder in Folio under Show more options | | unchanged |
| `FirstRunRowPowerShell` | PowerShell integration lets you jump between commands | Jump between commands in PowerShell | benefit-last |
| `FirstRunRowClaude` | Its tab lights up when Claude Code is waiting | Mark the tab when Claude Code is waiting | jargon |
| `FirstRunRowCodex` | Its tab lights up when a Codex turn ends | Mark the tab when a Codex turn ends | jargon |
| `FirstRunRowCopilot` | Its tab lights up when Copilot CLI is waiting | Mark the tab when Copilot CLI is waiting | jargon |
| `FirstRunTipUpdate` | When a new version is out, Settings names it and offers the releases page. | | unchanged |
| `FirstRunTipExplorer` | Registers the menu entry for this Windows account. | Adds the entry for this Windows account only. | mechanism |
| `FirstRunTipPowerShell` | Takes a dated copy of your $PROFILE, then appends one line to it. | | unchanged |
| `FirstRunTipClaude` | Takes a dated copy of ~/.claude/settings.json, then writes the hook. | | unchanged |
| `FirstRunTipCodex` | Takes a dated copy of ~/.codex/config.toml, then writes the notify program. | | unchanged |
| `FirstRunTipCopilot` | Takes a dated copy of ~/.copilot/hooks/folio.json, then writes the hook. | | unchanged |

## 4. `README.md`

28 passages reviewed, 15 proposed to change. Line numbers are the file as it
stands at `5792bda`.

| location | current | proposed | reason |
| --- | --- | --- | --- |
| L5–9, hero alt text | "Folio — the Windows terminal that renders math, and says which agent is waiting for you. Beside the name, a terminal pane has run a command that printed a file, and the display formula in that file — the integral of e to the minus x squared over the whole real line, equal to the square root of pi — stands typeset in the output, above the next prompt." | "Folio, a Windows terminal that typesets LaTeX and marks the tab of an agent waiting for you. Beside the name, a terminal pane shows a display formula typeset in a command's output, above the next prompt." | too long |
| L12–13, introduction | "Folio is a Windows terminal: formulas are typeset where a command prints them, files preview beside the prompt, and an agent that is waiting for you says so." | "Folio is an open-source Windows terminal. It typesets LaTeX where a command prints it, previews files beside the prompt, and marks the tab of an agent that is waiting for you." | benefit-last |
| L15–16, nav line | "[中文说明] · [Shortcuts] · [Security] · [Changes]" | | unchanged |
| L18–19, preview banner | "**Preview.** 0.2.2 is a preview build, signed by Weiyi Shi — see [Download](#download) below." | | unchanged |
| L25–30, download paragraph | "…unpack it wherever you keep programs, and run `folio.exe`. There is no installer, and nothing is written outside that folder until you run it." | "…unpack it wherever you keep programs, and run `folio.exe`. There is no installer; keep the extracted files together in one folder." | defensive |
| L32–35, signing paragraph | "On first run Windows may show **\"Windows protected your PC\"**: click **\"More info\"**, then **\"Run anyway\"**, where the publisher shown is **Weiyi Shi**." | "On first run Windows may show **\"Windows protected your PC\"**. **\"More info\"** names the publisher: check that it reads **Weiyi Shi** before running it." | defensive |
| L37–41, archive contents | "The archive holds nine files in one folder, and they belong together: `folio.exe`, `conpty.dll` and `OpenConsole.exe`, which it will not start a shell without; `folio.msix`, the few-kilobyte package the Explorer menu row registers to reach the first page, which names the folder it was extracted into; `folio-here.cmd` for VS Code; and the two licences, the third-party notices and the trademark note." | "The archive holds nine files that belong together: `folio.exe`, the two console libraries it needs to start a shell, `folio.msix` for the Explorer menu, `folio-here.cmd` for VS Code, and the licences and notices." | too long |
| L43–46, WebView2 paragraph | "The web preview needs the **WebView2 Runtime**…" | | unchanged |
| L50–57, first run | "A machine that has never run Folio gets one card, once. It says **Welcome to Folio** and asks the six questions whose answers write something outside `%APPDATA%\\Folio`, one line each: …" | "A machine that has never run Folio gets one card, once. It says **Welcome to Folio** and asks whether to check for updates, add Folio to the folder right-click menu, enable the PowerShell integration, and mark the tab for each of Claude Code, Codex and Copilot CLI this machine has. Update checks arrive on; the rest arrive off. Nothing about theme, font, size, language or layout — those are one click away and cost nothing while they are wrong." | two jobs |
| L63–71, first-run screenshot alt | "…then six rows of one line each…" | rewrite alongside the card rows | too long |
| L74–77, hover disclosure | "**Rest the pointer on a row and it says how** — including which of your own files the switch writes, and that the file is copied to a dated backup first. Nothing else on the card explains itself, because nothing else on it writes anywhere you have not been told about." | "**Rest the pointer on a row and it says how** — including which of your own files the switch writes, and that the file is copied to a dated backup first." | defensive |
| L79–84, Done and Not now | "**Every row on the card is also a row in Settings**, so nothing on it is a last chance. **Done** applies the rows that are on; **Not now** and `Esc` close the card with the shipped values — the update check on, the rest off — and change nothing. Either way it does not come back, and the shell behind it has been running the whole time. If you were already using Folio before this version, you never see it: your `settings.json` says so." | "**Every row on the card is also a row in Settings**, so nothing on it is a last chance. **Done** applies the rows that are on. **Not now** and `Esc` keep the shipped values: the update check on, the rest off. Either way the card does not come back, and if you were already using Folio you never see it." | too long |
| L86–92, first tab and profiles | "The first tab opens the first shell your machine actually has…" | | unchanged |
| L94–104, PowerShell integration | one 11-line paragraph carrying the added line, the backup, the deferred effect, the offer strip, its three buttons, and what runs on the integration | split into three: what it adds and how to undo it; when it takes effect and where Settings says so; the offer strip and its buttons | two jobs |
| L106–109, Agent page | "The three rows on the Agent page **install nothing by default**, and they are not defaults that happen to be off: each reads the tool's own configuration file and reports what is in it. On a new machine all three files are absent, so all three read Off." | "The three rows on the Agent page read the tool's own configuration file and report what is in it. On a new machine all three files are absent, so all three read Off." | defensive |
| L117, LaTeX intro | "The LaTeX a command prints is typeset where it was printed." | | unchanged |
| L129–136, LaTeX bullets | "One typesetter serves both — LaTeX through MiTeX into Typst. What it cannot set is shown as it was printed." | "Unsupported LaTeX remains visible as source text." | mechanism |
| L140–164, agent bullets | "Any program that writes `OSC 1337;RequestAttention=yes` is heard with nothing installed at all." | "Any program that writes `OSC 1337;RequestAttention=yes` raises the mark, with nothing installed at all." | metaphor |
| L168–193, preview bullets | five bullets on the peek card, the preview pane, clickable paths, addresses and pages per pane | | unchanged |
| L229–245, pane bullets | "A pane dropped on the join between two tabs becomes a tab *between* them: the list opens a slot and the pane stands in it, which is where letting go puts it." | "A pane dropped on the join between two tabs becomes a tab *between* them: a gap opens where it will land." | metaphor |
| L262–276, hotkey bullets | "It lives and dies with Folio: no icon of its own, and nothing left running behind the key. Closing the last window you can see ends the run." | "It has no icon of its own, and closing the last window you can see ends the run." | defensive |
| L292–302, search bullets | "The files come from an index of the folder the column is standing in, built off the window's own thread, so a deep tree does not make the box wait." | "File search covers the folder the files column is showing." | mechanism |
| L315–325, Explorer switch bullet | 11 lines carrying two registry keys, both menu placements, the package registration, elevation, and what Off takes back | "**Settings > General > Explorer context menu** puts Folio in the folder right-click menu, and On is everything your Windows can do. Windows 11 files \"Open Folio here\" under \"Show more options\"; on Windows 10 it stands in the only menu there is. On a Windows 11 with `folio.msix` beside `folio.exe`, On also puts \"Open in Folio\" on the page Windows 11 opens first. Off takes back whichever is registered, and the line under the row says which of them On reaches on your machine. `docs/PRIVACY.md` lists what is written." | too long |
| L326–330, PSReadLine bullet | "Windows PowerShell 5.1 ships PSReadLine 2.0.0, which misplaces the input line after the window is resized…" | | unchanged |
| L334–348, VS Code | `folio-here.cmd`, the JSON, and the menu path | | unchanged |
| L354–357, Privacy opener | "Folio sends nothing about you anywhere: no telemetry, no analytics, no crash reporting. There is no model and no API key in it; it serves the agents you already run. Two things reach the network: a page you open in the web preview, and the update check." | "Folio has no telemetry, no analytics and no crash reporting. There is no model and no API key in it; it serves the agents you already run. Two things reach the network: a page you open in the web preview, and the update check." | defensive |
| L359–365, update check paragraph | "…carrying a `User-Agent` of `Folio` and nothing else - no version, no identifier, no query string." | "…carrying a `User-Agent` of `Folio` and nothing else: no version, no identifier, no query string." | punctuation |
| L367–377, storage paragraph | "What it remembers lives in two directories…" | | unchanged |
| L379–409, known issues, licence, building, what's next | four short sections | | unchanged |

## 5. `docs/PRIVACY.md`, English half

A reference document, and the guide's principle 3 puts depth exactly here. Nine
passages reviewed, three proposed to change.

| location | current | proposed | reason |
| --- | --- | --- | --- |
| L10–15, opening | "Folio sends nothing about you anywhere. There is no telemetry, no analytics and no crash reporting." | "Folio has no telemetry, no analytics and no crash reporting." | defensive |
| L17–21, update-check lead | "Folio asks GitHub whether a newer release exists, and does nothing else with the answer…" | | unchanged |
| L23–34, update-check table | six rows of address, method, payload, frequency, storage and how to switch it off | | unchanged |
| L28, frequency cell | "A failure - no network, a proxy, a rate limit - counts as the attempt for that day and is not retried." | "A failure — no network, a proxy, a rate limit — counts as the attempt for that day and is not retried." | punctuation |
| L36–59, `%APPDATA%\\Folio` table | eleven files and what each holds | | unchanged |
| L61–75, WebView2 profile | what the profile holds and what is switched off in it | | unchanged |
| L77–104, what is in `session.json` | ten bullets of what a plain-text file carries | | unchanged |
| L105–120, elsewhere, first four bullets | panic log, registry keys, the three agent installers, the PowerShell line | | unchanged |
| L121–125, the first-run bullet | "**All five of those, and the update check, are what the first-run card asks about.**" | "**Those, and the update check, are what the first-run card can ask about.** It offers only the rows this machine can honour, so a machine with no agent installed sees fewer." | stale |

## 6. `SECURITY.md`, user-facing parts

The threat-model sections are exempt from the mechanism rule: mechanism is their
subject, and a reader deciding whether to trust the attention pipe needs the
descriptor. Six passages reviewed, three proposed to change.

| location | current | proposed | reason |
| --- | --- | --- | --- |
| L3–6, opening | "This file says what each of those is bounded by, and — more importantly — what it is **not** bounded by." | "This file says what bounds each of those, and what does not." | two jobs |
| L10–12, how to report | "Use this repository's **Security** tab → **Report a vulnerability**…" | | unchanged |
| L14–15, scope | "If you are not sure whether what you found is in scope, report it privately anyway; deciding that is our job, not yours." | "If you are not sure whether what you found is in scope, report it privately anyway. Working out whether it is in scope is the maintainers' job." | first person |
| L17–28, threat model | two paragraphs bounding the logon session | | unchanged |
| L30–181, the pipe, the installers, the web preview | mechanism as the subject | | unchanged |
| L208–219, "What Folio does not do" | a section of negatives | | unchanged; a security document is where the negative is the content, and every one of these three is a claim a reader checks rather than a defence of a design |

## 7. `docs/plans/release/release-note-v0.2.2-preview.md`, English half

15 passages reviewed, 11 proposed to change. This is the published 0.2.2 page
text; the page can be edited after these are agreed.

| location | current | proposed | reason |
| --- | --- | --- | --- |
| L5–7, opening | "Fixes and polish for 0.2.1-preview, and one card: a machine that has never run Folio is welcomed once and asked its boundary questions together, in one place." | "0.2.2 adds a first-run setup card and fixes pane dragging, web previews, image previews and formula rendering." | mechanism |
| L15, screenshot alt | "…then six rows of one line each…" | rewrite with the card rows; the card is two to six rows, not six | stale |
| L18–26, card bullet 1 | "The card asks every question whose answer writes something **outside** `%APPDATA%\\Folio`, and it asks them together, because they are one decision about how much of this machine Folio may touch. Six rows of one line each…" | "On first launch the card asks whether to check for updates, add Folio to the folder right-click menu, enable the PowerShell integration, and mark the tab for each agent this machine has. Update checks arrive on; the rest arrive off. Nothing about theme, font, size, language or layout." | stale |
| L27–28, card bullet 2 | "**Rest the pointer on a row and it says how**, including which of your own files the switch writes and that the file is copied to a dated backup first." | | unchanged |
| L29–32, card bullet 3 | "**Every row on the card is a row in Settings**, and the card presses those rows rather than doing anything of its own. Nothing on it is a last chance, and the switch you find in Settings an hour later is the same switch, in the same place, on the same shape of row." | "**Every row on the card is also a row in Settings**, so nothing on it is a last chance." | too long |
| L33–36, card bullet 4 | "**Done** applies the rows that are on. **Not now** and `Esc` close the card with the shipped values…" | | unchanged |
| L37–41, card bullet 5 | "A row is only offered if it can be honoured. An agent that is not on this machine, or whose configuration already calls Folio, is not listed, and when none of the three is there the rule above them goes with them." | "A row is only offered if it can be honoured. An agent that is not installed, or whose configuration already calls Folio, is not listed." | too long |
| L42–45, card bullet 6 | "The PowerShell row records an intent rather than acting: where your `$PROFILE` is comes from the shell, so the line is added by the next PowerShell that starts, and **Settings > Terminal** says so until it does." | "The PowerShell line is added by the next PowerShell session, because that is what names your `$PROFILE`. **Settings > Terminal** says so until it does." | mechanism |
| L46–47, card bullet 7 | "**If you were already using Folio, you never see it.** The step that brings your `settings.json` up to date is what records that." | "**If you were already using Folio, you never see it.**" | mechanism |
| L49–59, pane between two tabs | "The join between two entries in the tab list is a band eight logical pixels either side, and a pointer inside it makes the pane a new tab there rather than handing it to the tab it happens to be over. The list opens a slot and the pane stands in it, which is where letting go puts it — the picture is the row itself, and nothing is drawn across the join." | "Drag a pane between two tabs to create a tab at that position. A gap shows where it will appear. The horizontal strip, the vertical rail and the card column all read the join the same way." | mechanism |
| L61–72, shell menu | three bullets on `Split with`, the Profiles page and user-made profiles | | unchanged |
| L74–84, pane over a web preview | "The landing outline was drawn correctly over the page, but letting go did nothing: the press router handed every mouse button inside a page to the browser, releases included, and every gesture that spends a release — the drop, the divider, the video scrubber, the preview thumbs and pans, the terminal's own selection — is answered below that line." | "Fixed panes failing to drop over a web preview even when the drop outline was visible. While you are carrying something, a page no longer lights its links or replaces the drag cursor." | mechanism |
| L86–94, pages per pane | "Opening a second page in one tab used to navigate the first pane and leave the new one standing on its empty placeholder. A page now lands where every other preview lands — the first preview pane that is not locked, or a new one when there is none — so locking a page and opening another puts them side by side, each with its own engine, sharing the one browser profile they always shared." | "Fixed web pages opening in the wrong preview pane. Lock a page before opening another to view them side by side." | mechanism |
| L96–109, formula on a repainting screen | two bullets carrying the 200 ms stillness window and Claude Code's 106 ms median redraw | "A display formula printed by a program that redraws constantly, such as Claude Code, is typeset now. It used to stay as source text for as long as the program was busy. Markdown tables and images under image paths come back on those screens too." | mechanism |
| L111–121, picture in its own pane | "Every frame moved the picture to whichever preview pane came first in the window, which was the picture's own only while a tab had a single one of them." | "An image now stays in the pane that opened it, through splits, insertions, divider drags and tab switches." | mechanism |
| L123–143, also fixed | six bullets, symptom-led already | | unchanged |
| L145–157, upgrading | "**Nothing to do.** `settings.json` gains the card's two keys and is brought up to date **automatically**, the first time 0.2.2 reads it — nothing to delete, nothing to re-enter, and no answer you have already given is overwritten. That same step records that the card has been shown, which is why a machine that was already running Folio never meets it. `session.json`, `profiles.json`, `keybindings.json` and `pins.json` are read and written exactly as 0.2.1 left them, and no shortcut, default or file location has moved." | "**Nothing to do.** Your existing settings are preserved when you upgrade, and first-run setup appears only on a machine that has never run Folio. Unpack over the old folder, or beside it, and run `folio.exe`." | defensive |
| L159–170, download and run | "There is no installer, and nothing is written outside that folder until you run it." | "There is no installer; keep the extracted files together in one folder." | defensive |
| L172–194, known issues | six bullets | the 200 ms overlap bullet: "Two web previews can overlap for about a fifth of a second while panes are moving to new places. Panes standing still never overlap, and a divider drag does not do it." The other five unchanged, including the SmartScreen bullet's "switching SmartScreen off is not", which prevents a real mistake. | mechanism |

## 8. `docs/shortcuts.md` — which ids feed it

The file is rendered from `shortcuts::BINDINGS` by the
`docs_shortcuts_md_is_the_bindings_table` test and rewritten by
`scripts/generate-shortcuts-table.ps1`; `scripts/check-shortcuts-table.ps1` turns
red on drift. Its lead paragraph and column headings are literals inside
`shortcuts_document()` in `crates/bt-app/src/shortcuts.rs`, not `i18n.rs`.

47 ids reach the English table. Row titles: `RailNewTab`, `ShortcutNewWindow`,
`ShortcutQuit`, `ClosePane`, `ShortcutNextTab`, `ShortcutPrevTab`,
`ShortcutGotoTab1`–`ShortcutGotoTab9`, `ShortcutReopenClosed`,
`ShortcutJumpAttention`, `ShortcutCommandPalette`, `RowFocusMode`,
`ShortcutSplitHorizontal`, `ShortcutSplitVertical`, `ShortcutDuplicatePaneSplit`,
`PaneMenuZoom`, `ShortcutFilesPane`, `ShortcutGitPage`, `Settings`,
`ShortcutSavePreview`, `ShortcutPrevCommandMark`, `ShortcutNextCommandMark`,
`ShortcutOpenSearch`, `ShortcutNextMatch`, `ShortcutPrevMatch`,
`ShortcutCloseSearch`, `ShortcutWebAddress`, `ShortcutWindowAddress`,
`ShortcutWebDevTools`, `ShortcutSummonQuake`, `ShortcutSummonPip1`–`4`. Scope
column: `ShortcutScopePreview`, `ShortcutScopeTerminalPrimary`,
`ShortcutScopeSearchHost`, `ShortcutScopeSearchOpen`, `ShortcutScopeWebPage`. Key
column: `ShortcutUnbound`.

Two of the proposals touch this file: `ShortcutGitPage` and
`ShortcutWindowAddress`. Applying either means regenerating `docs/shortcuts.md`
in the same commit.

One note that is not a copy fault: `RowFocusMode` is a Settings row title reused
as a binding title, so the generated "What it does" column reads **Cards**, a
noun where every other cell is a verb phrase. Fixing it means a second string,
not a rewrite of the row title.

## 9. The guide's twenty-four rewrites

Twenty-three adopted — twelve verbatim, eleven adapted — and one declined on
wording while keeping its diagnosis.

| # | verdict | note |
| --- | --- | --- |
| 1 `DescCopyOnSelect` | kept verbatim | |
| 2 `DescUpdateCheck` | adapted | "show available versions in Settings" describes a list; the row draws one name and a mark on the gear. Proposed: "Checks once a day for a new version and names it here." |
| 3 `DescClaudeHooks` | adapted | benefit-first and the project-folder assurance cut, as proposed; the path and `CLAUDE_CONFIG_DIR` kept, because a reader who wants to inspect or undo the write has no other on-screen place to learn them. |
| 4 `DescCodexNotify` | adapted | same trade, and the same for `DescCopilotHooks`, which the guide did not list. |
| 5 `DescQuakeHotkey` | adapted | declarative rather than imperative, to match every other sentence in the dialog. The conflict message the guide asked for already exists as `DescQuakeHotkeyTaken`. |
| 6 `DescQuakeCommand` | adapted | declarative. `DescQuakeRestore` does already carry the "typed and not run" disclosure the guide asked to keep. |
| 7 `WebFailCrashSay` | kept verbatim | |
| 8 `WebFailDownloadSay` | **declined on wording** | the proposal "Open this page in your browser to start the download there" is word for word the button beside it (`WebFailDownloadVerb`), so the card would say the same thing twice. Proposed instead: "Start this download in your browser instead." The diagnosis — the second sentence is mechanism — is accepted. |
| 9 `FirstRunSettingsLine` | already applied on `main` | |
| 10 `FirstRunRowUpdate` | kept verbatim | |
| 11 `FirstRunRowPowerShell` | adapted | "Enable" is redundant beside a switch, and no other card row uses it. Proposed: "Jump between commands in PowerShell". |
| 12 `FirstRunRowCodex` | adapted and extended | the dangling "Its" is on all three agent rows, so all three are proposed. |
| 13 `ShellIntegrationPending` | kept verbatim | |
| 14 `CodexNotifyAddedToast` | adapted | shortened to one line, and the same fix applied to the Claude Code and Copilot CLI toasts and to all three Removed toasts, which carry the same overridable path. |
| 15 README introduction | adapted | "open-source" and the concrete agent claim taken; the sentence rhythm kept. |
| 16 README download excerpt | kept verbatim | |
| 17 README first run | kept in substance | and it is a factual correction, not only an editorial one — see section 10. |
| 18 README LaTeX bullet | kept verbatim | |
| 19 README search bullet | adapted | "file list" is not a name this product uses; the term is **files column**. |
| 20 Release opening | adapted | the fix list widened to name the image-preview fix as well. |
| 21 Release tab-join bullet | kept verbatim | the third bullet of that section is kept as it stands. |
| 22 Release web-preview drop | kept verbatim | with the second bullet's user-visible half retained. |
| 23 Release pages per pane | kept verbatim | |
| 24 Release upgrading | kept verbatim | with the unzip-and-run instruction retained, as the guide asked. |

## 10. Facts that are stale against the code

1. **README L50–57 and release note L18–26 both say the card asks "six
   questions".** `first_run::rows` always offers Update and Explorer, adds the
   PowerShell row only when `powershell_integration_installed` is false, and adds
   an agent row only when that agent is both `found` and `installable`. The card
   can be two rows. Both screenshot alt texts repeat "six rows".

2. **README L51–52 says those are "the six questions whose answers write
   something outside `%APPDATA%\\Folio`".** `RowKind::Update` spends
   `Application::UpdateCheck`, which is Folio's own `settings.json` inside
   `%APPDATA%\\Folio`. The claim is wrong for the one row that arrives on.

3. **`docs/PRIVACY.md` L121 says "All five of those, and the update check, are
   what the first-run card asks about."** Same conditionality: the card asks
   about what this machine can honour.

4. **Six toasts print a path an environment variable can move.**
   `ClaudeHooksAddedToast`, `ClaudeHooksRemovedToast`, `CodexNotifyAddedToast`,
   `CodexNotifyRemovedToast`, `CopilotHooksAddedToast` and
   `CopilotHooksRemovedToast` are literals naming `~/.claude/settings.json`,
   `~/.codex/config.toml` and `~/.copilot/hooks/folio.json`, but the installers
   read `CLAUDE_CONFIG_DIR`, `CODEX_HOME` and `COPILOT_HOME` first —
   `SECURITY.md`'s own table says so. On a machine with one of those set, the
   toast names a file that was not written. The proposals drop the path from the
   toast. `FirstRunTipClaude`, `FirstRunTipCodex` and `FirstRunTipCopilot` carry
   the same conditional inaccuracy and are proposed **unchanged**, because there
   the path is the consent disclosure and the default is right on the machines
   that see the card.

5. **`DescBackgroundOpacityUnavailable` is not a Windows-version fact.** The row
   is greyed when the render surface reports no premultiplied alpha, unlike
   `DescAcrylicUnavailable`, which really is a Windows capability. The current
   sentence states the effect and names no cause, which is correct; it should not
   be "improved" into a sentence about Windows versions.

6. **`PeekUnknown` spells "unrecognized"** where the rest of the table is
   British: "Colour scheme", "centred", "minimised", "typesets". Proposed
   "unrecognised".

7. **Two names for the files column.** `FilesProgramRefused` says "the files
   tree" and `DragRootTreeHere` says "this tree", where
   `ShortcutFilesPane`, `DescGitPanel`, `ProfileFilesPane` and the README all say
   **files column**. Proposed to unify.

## 11. Ids whose meaning I could not verify

Each is a question rather than a proposal; none of the eleven is in section 3's
proposals except where noted.

| id | question |
| --- | --- |
| `PreviewNothingToSave` | "there is nothing to save" is lower case with no full stop because it is fitted into `preview::not_saved`. What is the whole sentence a reader sees? The fragment cannot be judged alone. |
| `GraphLeaveDetached` | "Back to " ends in a space and is concatenated with a branch name. Does the result read "Back to main", and what happens when the name is long enough to clip? |
| `HyperlinkBlocked` / `HyperlinkBlockedSuffix` | what blocks a hyperlink, and is there anything the reader can do about it? A one-word state with no remedy may be a UI question rather than a copy one. |
| `ContextMenuNoExecutable` | after "Windows did not say where folio.exe is", is there a step the reader can take? The proposal only removes the blame; it cannot add a remedy that does not exist. |
| `ClaudeHooksFailedToast`, `CodexNotifyFailedToast`, `CopilotHooksFailedToast` | the installers return a refusal **with a reason** (`attention_copilot.rs` returns "copilot 1.0.26 or newer is needed for this", among others). Does any surface show that reason? If not, these three toasts are the shape principle 7 rejects, and no rewrite fixes them. |
| `MoveRefusedPaneIsNowATab` | "The pane is a tab of this window." Is this a refusal or a report that a move already happened? The sentence reads as a statement either way. |
| `CapNoneLong` | "Hyperlinks are declared anyway" — declared to whom? My proposal ("Hyperlinks still work") assumes the terminal renders OSC 8 links with no shell integration at all. Confirm before applying. |
| `ProfilesColourFixed` | "Its own" is read alone in a picker beside "Inherited". Its own what? |
| `DescExplorerFirstPageElsewhere` | "Folio puts it back on the next launch that can" — which condition stops a launch from putting it back? The proposal drops the hedge; if the condition is real the hedge has to come back in a form that names it. |
| `PlaceholderSeatNotice` | "This pane was saved by a newer version of Folio". Is the pane recoverable by installing that version, and should the sentence say so? |
| `RowFocusMode` in `docs/shortcuts.md` | the generated table's "What it does" cell reads "Cards". Should the binding carry its own verb string? |
