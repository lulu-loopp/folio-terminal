# Folio user-facing copy guide

Research reviewed 7 September 2026. Applies to UI strings, settings, toasts, the first-run card, both READMEs, and GitHub release notes. The examples below are proposals, not changes to the product.

Folio is an open-source Windows terminal. Its readers need to run commands, find files, arrange panes, and notice when an agent needs attention. Write for those tasks. A developer audience understands technical terms, but still needs a reason to read each sentence.

The recurring problem in this repository is extra explanation after the useful fact: how a result is implemented, why the design is defensible, or what else the software does not do. Some passages also replace familiar interface terms with elaborate physical metaphors. The Chinese often inherits both problems.

## 1. Principles

These are Folio editorial rules synthesized from the sources, not quotations or universal platform requirements. Bad/good pairs in this section are illustrative.

### 1. Answer the user's immediate question

**Rule: Include a fact when it helps the reader choose, act, recover, or understand the result.**

Why: Information competes for attention. GOV.UK starts content planning with an identifiable user need; Microsoft recommends putting decisions and next steps first. Apply this test to every clause. [GOV.UK user needs](https://guidance.publishing.service.gov.uk/writing-to-gov-uk-standards/plan-manage-content/identify-user-needs/), [Microsoft style tips](https://learn.microsoft.com/en-us/style-guide/top-10-tips-style-voice).

- Bad: “The card uses the same settings targets as the Settings dialog.”
- Good: “You can change these options in Settings.”

### 2. Lead with the visible effect

**Rule: Name the action and its result before explaining how it works.**

Why: A setting needs to explain the choice. Stripe's “Send emails when card payments fail” names an action and trigger directly. Apple recommends explaining what a setting does when enabled. [Stripe customer emails](https://docs.stripe.com/billing/revenue-recovery/customer-emails), [Apple HIG writing](https://developer.apple.com/design/human-interface-guidelines/writing?changes=l_1).

- Bad: “Registers a global hotkey with Windows.”
- Good: “Show or hide the terminal from any app.”

### 3. Give each detail an appropriate home

**Rule: Keep implementation details in reference material unless they change the user's decision or next step.**

Why: A configuration reference can explain keys, defaults, exceptions, and dependencies. A setting row rarely needs all four. Windows Terminal and Ghostty use reference pages for this depth. [Windows Terminal settings](https://learn.microsoft.com/en-us/windows/terminal/customize-settings/interaction), [Ghostty configuration](https://ghostty.org/docs/config/reference).

- Bad: “The renderer process exited after the web engine failed.”
- Good: “This page stopped running.”

For Folio, put renderer timing, event routing, registry implementation, and migration keys in technical documentation or linked issues. Keep a required runtime name beside its download action. Keep a file path when the reader must inspect or edit that file. Put the fact that a switch edits another tool's configuration where the reader can learn it before enabling the switch.

### 4. Remove defensive negatives

**Rule: Describe the supported behavior and retain a negative only when it prevents a likely mistake.**

Why: Explaining every absent behavior makes a simple choice sound uncertain. Apple's setting guidance specifically avoids spelling out the obvious disabled state. [Apple HIG writing](https://developer.apple.com/design/human-interface-guidelines/writing?changes=l_1).

- Bad: “Copies the selection. There is no other sign that it happened.”
- Good: “Copy selected text to the clipboard when you release the mouse button.”

Use this decision test: What mistake would removing the negative cause? Keep “Restored commands are not run” beside command restoration because automatic execution is a consequential ambiguity. Keep factual collection statements in Privacy. Keep limitations that determine whether a feature works. Remove “nothing else happens,” project-folder assurances after an already clear user-level scope, and repeated explanations of the off state. A bug fix may naturally say “no longer” when it identifies the old failure.

### 5. Be exact about scope and timing

**Rule: Preserve the condition, affected object, and time when the change takes effect.**

Why: Short copy is wrong if it promises more than the feature delivers. VS Code release notes retain prerequisites such as a compatible font or shell integration when those determine whether the feature works. [VS Code 1.96 terminal changes](https://code.visualstudio.com/updates/v1_96#_terminal).

- Bad: “Codex needs your input.”
- Good: “Codex finished its turn.”

Folio's Codex signal reports turn completion; the Claude Code and Copilot CLI integrations report waiting. Keep that distinction. Likewise, distinguish saving an option now from applying it in a new shell session. Avoid absolute claims such as “any folder,” “always,” and “nothing is written” without checking the scope.

### 6. Use ordinary interface language

**Rule: Use familiar nouns and direct verbs instead of metaphors for software behavior.**

Why: Readers scan for known objects and actions. Google's guidance favors active voice and conditions before instructions; GOV.UK requires plain language. Technical precision can coexist with ordinary phrasing. [Google style highlights](https://developers.google.com/style/highlights), [GOV.UK clear language](https://guidance.publishing.service.gov.uk/writing-to-gov-uk-standards/writing-guidelines/clear-language/).

- Bad: “The keyboard moves to another window and the terminal goes away.”
- Good: “Hide the terminal when another window gains focus.”

Use “tab,” “pane,” “window,” “file list,” “preview pane,” and “command palette” consistently. A tab contains panes; a window contains tabs. Do not alternate “pane” and “panel” for the same object. Preserve existing UI names in instructions until a rename is adopted across the product. “Shell,” “profile,” and “clipboard” are useful terms for this audience. “The hand,” “the row's answer,” and “the folder the column is standing in” usually obscure it.

### 7. Make actions and failures concrete

**Rule: Name what the action does, or what failed and the next supported step.**

Why: Apple recommends action labels and recoverable, unblaming errors. Vercel's redeploy notification appears when a settings change needs that action to take effect. [Apple HIG writing](https://developer.apple.com/design/human-interface-guidelines/writing?changes=l_1), [Vercel redeploy notification](https://vercel.com/changelog/redeploy-without-leaving-project-settings).

- Bad: “We couldn't complete the operation.”
- Good: “Couldn't save settings. The settings file is read-only.”

Only name a cause when the application knows it. Preserve useful returned error details and paths. A retry instruction requires an actual retry path and a reason retrying could help. A success toast should confirm the completed action; mention a new session if required. Do not claim that an integration is already active when only its configuration has been saved.

### 8. Give each sentence one job

**Rule: Split the action, prerequisite, and explanation when they compete in one sentence.**

Why: Front-loaded keywords and concise sentences support scanning. A row description should add information to its label, rather than repeat it in a full sentence. [Microsoft style tips](https://learn.microsoft.com/en-us/style-guide/top-10-tips-style-voice).

- Bad: “This command is run once each time Folio starts, on the first summon, and nothing else restored is run.”
- Good: “Run this command the first time you summon the terminal after starting Folio.”

Use sentence case for English UI text and headings, preserving product names. Give complete explanatory sentences periods; omit periods from labels and buttons. Use a question mark only for an actual question. Avoid em-dashes in rules and default to sentences in product explanations. Use descriptive links instead of “here.”

### 9. Describe the release through recognizable behavior

**Rule: State the affected workflow and the change a user can observe.**

Why: GitHub's release-note guidance asks who is affected, what behavior they experienced, and what action is required. Raycast's fixes commonly name the affected feature and symptom. [GitHub release-note guidance](https://docs.github.com/en/contributing/style-guide-and-content-model/style-guide#release-notes), [Raycast Windows changelog](https://www.raycast.com/changelog/windows).

- Bad: “The release event now reaches the gesture below the browser router.”
- Good: “Fixed panes failing to drop over a web preview.”

Use “Added,” “Changed,” or “Fixed” when helpful. Include a setting name or shortcut when it lets readers try the change. Put low-level causes in an issue or implementation note. Avoid “polish,” “improved experience,” and author-effort narratives without a concrete result. In known issues, preserve real uncertainty such as an unconfirmed report; cut explanations of why the team has not fixed it.

### 10. Localize the task and meaning

**Rule: Write each language naturally while preserving behavior, conditions, and terminology.**

Why: Google emphasizes a global audience, and Apple calls for language that works across localization and accessibility. Matching English syntax is not a measure of translation quality. [Google style highlights](https://developers.google.com/style/highlights), [Apple HIG writing](https://developer.apple.com/design/human-interface-guidelines/writing?changes=l_1).

- Bad: “The configuration of notifications will be performed by us.”
- Good: “Set up notifications.”

For Chinese, work from a short behavior brief and the actual surface. Review naturalness without the English first, then compare facts against the source. The specific Chinese checks are in section 4.

## 2. Per-surface checklist

Lengths below are Folio editing targets, not limits attributed to the sources or verified layout constraints. English counts are words. Chinese counts are approximate Han characters, with Latin names and paths consuming additional width. Test actual wrapping before shipping. Never remove a necessary condition to hit a count.

| Surface | What belongs | What does not belong | Length guidance |
| --- | --- | --- | --- |
| UI string | A recognizable object, action, or state. Name an action for a button. Preserve exact product names and shortcut notation. | Explanations of dispatch, storage, rendering, or design intent; redundant “you can”; cute labels; unexplained pronouns. | Label: 1–4 English words or about 2–8 Chinese characters. Status: usually 3–10 words or 4–18 characters. Allow longer names when clarity requires them. |
| Setting description | What enabling or changing it does. Units, scope, dependency, or effective time if needed. For an unavailable option, the relevant reason and supported remedy. | Repeating the label; both sides of an obvious toggle; implementation history; blanket promises about untouched files; sibling-feature descriptions. | Usually 1 sentence, 10–25 words or 15–40 characters. Up to 2 sentences, about 40 words or 60 characters, for meaningful consequences. Check the row's actual wrap budget. |
| Toast | Completed action, failure, or required next step. Name the affected object when multiple operations are possible. Preserve a useful error reason. | Reassurance about everything left unchanged; celebratory filler; stack traces in the summary; guessed remedies; long instructions that disappear before they can be read. | Success: 3–12 words or 4–20 characters. Failure or deferred effect: 1–2 sentences, usually under 30 words or 50 characters. Long diagnostics need a persistent, accessible location. |
| First-run question or toggle | One understandable choice and its outcome. Clear default state. Brief disclosure of changes to another tool before consent. Explain applying or skipping choices and where to change them later. | A design manifesto; file-by-file internals in the label; claims that every option has the same consequences; unrelated preferences; a question paired with an ambiguous “Yes.” | Toggle label or question: 6–14 words or 10–25 characters. Supporting disclosure: 1 short sentence. Shared footer: 1 sentence. A toggle may use an action phrase without a question mark. |
| README section | The task or feature, how to use it, requirements, and a useful link. Download instructions, Windows version, preview status, signing identity, and relevant dependencies. Matching facts in both languages. | Exhaustive package anatomy; renderer architecture inside feature bullets; descriptions of invisible layout mechanics; duplicated release history; promises inferred from source-code intent. | Intro: 1–2 sentences, about 25–45 words. Paragraph: 40–80 words or 60–140 characters. Feature bullet: 10–30 words or 15–50 characters. Split a section when it serves a second task. |
| Release note bullet | Feature or fix, trigger or affected workflow, observable result. Add migration steps, platform conditions, or a workaround when necessary. | Pointer hitbox dimensions, event ordering, internal counters, thread models, unchanged-file inventories, repeated “nothing to do,” and marketing claims. | Usually 1 sentence, 12–30 words or 20–50 characters. Up to 2 sentences, about 50 words or 80 characters, for a prerequisite or workaround. Link longer explanations. |

For the first-run card, “Done” can remain because it ends the flow. Supporting text must match its actual behavior. “Not now” and Esc retain the shipped choices, including update checks being on. Avoid describing them as disabling all integrations or making no persistent changes. File edits and their scope belong in concise supporting disclosure; a hover tooltip must not be the only way a keyboard or assistive-technology user can discover a consequential change. This is a placement recommendation for a future UI review, not a claim that the current card has those capabilities.

For READMEs, keep the download path and basic usage close to the top. Keep precise privacy information in the Privacy section and link to `docs/PRIVACY.md` for detail. Replace a broad statement such as “sends nothing about you anywhere” with verified collection and network facts. Signing and SmartScreen guidance should identify the publisher and relevant prompt without routine encouragement to bypass an unexpected publisher warning. Explain runtime installation only where it affects a feature.

Screenshot alt text is also user-facing copy. Describe the meaningful content or demonstrated action, not every shadow, separator, and pixel. Essential setup facts must also exist as ordinary text. Both READMEs need accurate localized navigation paths; the current Chinese README mixes “设置” and “General.”

### Patterns observed in terminals and developer tools

These are selective observations of published examples, not claims that every sentence in these products is exemplary. Settings documentation is not always the literal label shown in the app.

| Product | Settings evidence | Release-note evidence | Apply to Folio |
| --- | --- | --- | --- |
| Windows Terminal | “Automatically copy selection to clipboard” names the result. The reference separately documents `copyOnSelect`, its default, and related mouse behavior. [Interaction settings](https://learn.microsoft.com/en-us/windows/terminal/customize-settings/interaction#automatically-copy-selection-to-clipboard). | A fix identifies incorrect underline colors during an active text selection. [Releases](https://github.com/microsoft/terminal/releases). | Use recognizable actions and triggers. Detailed descriptions of both values belong in a reference when they explain a meaningful difference. |
| Ghostty | `confirm-close-surface` documents its purpose, default, and `always` value. [Option reference](https://ghostty.org/docs/config/reference#confirm-close-surface). | Version 1.0.1 connects the new `always` value to always showing close confirmation. [1.0.1 notes](https://ghostty.org/docs/install/release-notes/1-0-1). | Keep a configuration key when users can act on it. Avoid importing reference-level terms such as “surface” into ordinary pane controls. |
| Warp | Notification documentation starts with command completion and input requests, then explains app and OS permissions. [Desktop notifications](https://docs.warp.dev/terminal/more-features/notifications). | The 9 April 2025 fixes identify copying selected code with Agent Mode enabled and missing Homebrew shells on macOS. [2025 changelog](https://docs.warp.dev/changelog/2025/). | State the event first, then prerequisites. Include a platform or mode only when it narrows the affected workflow. |
| VS Code | The actual `terminal.integrated.copyOnSelection` description names selecting terminal text and copying it to the clipboard. [Configuration source](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/contrib/terminal/common/terminalConfiguration.ts). | Version 1.96 explains terminal ligatures, the enabling setting, and the font requirement. [1.96 notes](https://code.visualstudio.com/updates/v1_96#_terminal). | Retain the setting users need to find and the prerequisite they need to satisfy. Folio can omit the formulaic “Controls whether” opening. |
| Zed | “Terminal: Copy On Select” separates the behavior description, key, default, and example. [All settings](https://zed.dev/docs/reference/all-settings#terminal-copy-on-select). | Its stable notes identify word movement and selection stopping on the wrong side of punctuation. [Stable releases](https://zed.dev/releases/stable). | Put the symptom in the bullet and link implementation discussion. Keep type/default/example detail out of compact setting descriptions. |

Stripe's action-and-trigger labels and Vercel's action-linked notification offer useful product-copy patterns. Raycast's feature-prefixed fix lists help scanning. Their promotional introductions, humor, or internal terminology are not part of Folio's proposed voice.

## 3. Before and after from this repository

Twenty-four examples follow. “Before” reproduces the selected English string or contiguous passage, with source line breaks and Markdown emphasis normalized. Where marked “excerpt,” only that passage is being replaced. All proposed replacement copy is English.

Locations refer to [i18n.rs](../../../crates/bt-app/src/i18n.rs), [README.md](../../../README.md), and the [0.2.2 release draft](../release/release-note-v0.2.2-preview.md). Identifiers and section names are the stable lookup keys. The Chinese README was reviewed for the diagnosis and checklist in section 4.

| # | Location | Before | Proposed after | Reason or detail placement |
| --- | --- | --- | --- | --- |
| 1 | i18n: `DescCopyOnSelect` | Letting go of a selection in a pane writes it to the clipboard. There is no other sign that it happened | Copy selected text to the clipboard when you release the mouse button. | Keep timing; remove the absent-feedback explanation. |
| 2 | i18n: `DescUpdateCheck` | Asks the releases page once a day whether a newer version is out. It downloads nothing. | Check for updates once a day and show available versions in Settings. | States the actual result. Put manual download steps on the update action or download page. |
| 3 | i18n: `DescClaudeHooks` | Adds hooks to your ~/.claude/settings.json, or CLAUDE_CONFIG_DIR, so Claude Code tells this window when it is waiting for you. Nothing is written into a project folder. | Mark the tab when Claude Code needs your input. Updates your user-level Claude Code settings. | Benefit plus configuration scope. Keep the effective path and backup information in supporting details; account for the environment override. |
| 4 | i18n: `DescCodexNotify` | Adds a notify program to your ~/.codex/config.toml, or CODEX_HOME, so codex tells this window when a turn has ended. It does not report a codex waiting for you. | Mark the tab when Codex finishes a turn. Updates your user-level Codex configuration. | Preserves the distinct trigger. Document the effective config path in details. Do not imply input-request detection. |
| 5 | i18n: `DescQuakeHotkey` | The key that calls the terminal down. It is claimed from Windows, so it works while another program has the keyboard. | Show or hide the terminal from any app with this shortcut. | Explains the benefit of a global shortcut. A registration conflict belongs in its own unavailable-state message. |
| 6 | i18n: `DescQuakeCommand` | This command is run once each time Folio starts, on the first summon. Nothing else the summoned terminal restores is run. | Run this command the first time you summon the terminal after starting Folio. | Keep the once-per-launch timing. Keep the non-execution disclosure with command restoration. |
| 7 | i18n: `WebFailCrashSay` | This page stopped running. Its render process exited. | This page stopped running. | The second sentence explains an internal cause without helping recovery. |
| 8 | i18n: `WebFailDownloadSay` | This download cannot be handed to your browser. The request carried data a plain link cannot replay. | Open this page in your browser to start the download there. | Uses the existing browser action. Does not promise that the pending request will transfer or resume. |
| 9 | i18n: `FirstRunSettingsLine` | Every row here is also a row in Settings. | You can change these options in Settings. | Answers whether the choice can be revisited. |
| 10 | i18n: `FirstRunRowUpdate` | Get told when a new version of Folio is out | Check for Folio updates | Names the choice without implying an OS notification or automatic installation. Its existing supporting text can explain the Settings indicator. |
| 11 | i18n: `FirstRunRowPowerShell` | PowerShell integration lets you jump between commands | Enable command navigation in PowerShell | Names the benefit directly. Retain the `$PROFILE` edit disclosure and deferred application time in supporting text. |
| 12 | i18n: `FirstRunRowCodex` | Its tab lights up when a Codex turn ends | Highlight the tab when Codex finishes a turn | Removes the dangling “Its”; preserves turn completion. |
| 13 | i18n: `ShellIntegrationPending` | Joins the next PowerShell that starts | Takes effect in the next PowerShell session | Explains pending state without anthropomorphism. |
| 14 | i18n: `CodexNotifyAddedToast` | Added to ~/.codex/config.toml. Takes effect in a new codex session. | Codex notifications configured. Start a new Codex session to use them. | Confirms configuration, not current activation. Avoids displaying a default path that an override can change. |
| 15 | README: introduction | Folio is a Windows terminal: formulas are typeset where a command prints them, files preview beside the prompt, and an agent that is waiting for you says so. | Folio is an open-source Windows terminal with LaTeX rendering, file previews, and tab indicators for supported coding agents. | Gives a scannable product description without a blanket claim about agent waiting signals. |
| 16 | README: Download, excerpt | There is no installer, and nothing is written outside that folder until you run it. | Keep the extracted files together in one folder. | The surrounding paragraph already says to unzip and run. Keeping companion files together is the useful instruction. |
| 17 | README: First run, excerpt | A machine that has never run Folio gets one card, once. It says Welcome to Folio and asks the six questions whose answers write something outside `%APPDATA%\Folio`, one line each: be told when a new version is out, which is the only row that arrives on; open any folder in Folio from its right-click menu; the PowerShell integration; and a tab that lights up for each of Claude Code, Codex and Copilot CLI this machine actually has. | On first launch, choose whether to check for updates, add Folio to the folder context menu, enable PowerShell integration, and configure notifications for available agents. Update checks are on by default; the other options are off. | Avoids a fixed row count and corrects the claim that every choice writes outside Folio's folder. Remove the following theme/font rationale as well; retain apply/skip behavior separately. |
| 18 | README: LaTeX feature bullet | One typesetter serves both — LaTeX through MiTeX into Typst. What it cannot set is shown as it was printed. | Unsupported LaTeX remains visible as source text. | Keeps the useful fallback. Put library names in architecture or build documentation. |
| 19 | README: Search everything, final bullet | The files come from an index of the folder the column is standing in, built off the window's own thread, so a deep tree does not make the box wait. | File search covers the folder shown in the file list. | Keeps search scope; removes thread mechanics and an unqualified responsiveness claim. Merge with the earlier scope bullet if editing the whole section. |
| 20 | Release: opening paragraph | Fixes and polish for 0.2.1-preview, and one card: a machine that has never run Folio is welcomed once and asked its boundary questions together, in one place. | This preview adds first-run setup and fixes pane dragging, web previews, and formula rendering. | Gives readers the changes they can evaluate. |
| 21 | Release: A pane can be dropped between two tabs, first bullet | The join between two entries in the tab list is a band eight logical pixels either side, and a pointer inside it makes the pane a new tab there rather than handing it to the tab it happens to be over. The list opens a slot and the pane stands in it, which is where letting go puts it — the picture is the row itself, and nothing is drawn across the join. | Drag a pane between two tabs to create a tab at that position. A gap shows where it will appear. | Describes the gesture and feedback. Hitbox dimensions belong in implementation notes. |
| 22 | Release: A pane dragged over a web preview can be dropped there, first bullet | The landing outline was drawn correctly over the page, but letting go did nothing: the press router handed every mouse button inside a page to the browser, releases included, and every gesture that spends a release — the drop, the divider, the video scrubber, the preview thumbs and pans, the terminal's own selection — is answered below that line. | Fixed panes failing to drop over a web preview even when the drop outline was visible. | Identifies the trigger and recognizable failure. |
| 23 | Release: A window holds as many pages as it has preview panes, first bullet | Opening a second page in one tab used to navigate the first pane and leave the new one standing on its empty placeholder. A page now lands where every other preview lands — the first preview pane that is not locked, or a new one when there is none — so locking a page and opening another puts them side by side, each with its own engine, sharing the one browser profile they always shared. | Fixed web pages opening in the wrong preview pane. Lock a page before opening another to view them side by side. | Keeps the workflow and locking condition. Engine ownership belongs in technical notes. |
| 24 | Release: Upgrading from 0.2.1, first paragraph | Nothing to do. `settings.json` gains the card's two keys and is brought up to date automatically, the first time 0.2.2 reads it — nothing to delete, nothing to re-enter, and no answer you have already given is overwritten. That same step records that the card has been shown, which is why a machine that was already running Folio never meets it. `session.json`, `profiles.json`, `keybindings.json` and `pins.json` are read and written exactly as 0.2.1 left them, and no shortcut, default or file location has moved. | Your existing settings are preserved when you upgrade. First-run setup appears only for new users. | Keep the following unzip-and-run instructions. Omit migration keys and the inventory of unchanged files. |

The correction in example 17 is supported by [first_run.rs](../../../crates/bt-app/src/first_run.rs): `RowKind::Update` is explicitly the option stored in Folio's own settings. The module also conditionally offers rows. A README should describe choices available to the reader rather than turn the design rationale into a filesystem guarantee.

These are editorial proposals grounded in repository behavior descriptions, not a runtime audit. Before adopting them, check complete label/description pairs, adjacent controls, environment overrides, and effective-time states. A better sentence cannot supply a missing accessible disclosure or recovery action.

## 4. Chinese-specific guidance

Target natural Simplified Chinese for a Windows developer tool. Preserve meaning and established product terms; allow different sentence boundaries, subjects, and information order. This section gives review principles and diagnostic fragments, not Chinese rewrites.

The present [Chinese README](../../../README.zh-CN.md) includes literal metaphors such as “键后也不留驻留进程” and dense modifiers such as “其服务对象为用户已运行的 agent.” The release draft's “其边界问题会在一个地方一次性问完” brings an internal design category into the user's task. In `i18n.rs`, “它向 Windows 认领” describes hotkey registration through an unnatural metaphor. The problem is both what is being explained and how it is phrased.

### Translationese tells

These are Folio review heuristics, not claims that a grammatical form is always wrong. Flag a pattern, then evaluate it in context. Chinese can naturally use a passive or omit a subject; forced active voice is not the goal.

| Tell | What to flag | Review instruction |
| --- | --- | --- |
| Passive voice | Repeated 被, 由…进行, 将被, 得到…处理 in ordinary status and setting text. | Find the real actor and action. Prefer a direct action or result when the actor adds nothing. Preserve a passive when the affected object is the useful topic or the actor is unknown. |
| Nominalisation | 进行配置, 执行…操作, 实现…功能, …的启用, 对…进行, overly formal 的形成原因. | Find the buried verb. Remove the generic operation noun unless it distinguishes a real product concept. |
| Stacked 的 | Several 的 clauses nested around the same noun. | Identify the head noun and separate its action, location, and condition. A count is a warning signal, not an automatic deletion rule. |
| English word order | A long English-shaped subject, an important condition postponed until the end, repeated 它/其/这, or connectors copied one for one. | Start with the topic or condition the reader needs. Reorder clauses for Chinese while preserving which condition governs which action. |
| Long pre-modifiers | A full event, location, and prerequisite placed before one noun, especially before 窗格, 配置, or 文件. | State the object early. Move independent conditions into another clause or sentence. Do not make the reader hold a paragraph before finding the object. |
| First-person plural | 我们, 咱们, or invitations equivalent to “let's” in errors, settings, and routine actions. | Identify who “we” means. Use the product name only when the actor matters; otherwise use the action or state. Also check unnecessary 你的 and 您的. |
| Literal metaphors | Software “answering questions,” “claiming” keys, rows “standing” somewhere, windows owning a “hand,” or the keyboard being handed back. | Describe the actual interface event. Do not substitute equally unusual colloquial imagery. |
| Formal translation register | Repeated 该, 此, 其, 均, 亦, 予以, 从而, or 一概 in small UI text. | Read aloud as an instruction to a colleague. Keep technical distinctions while removing administrative tone. |
| Excess explanation | 无需…, 不会…, 并非…, 而非…, 仅…, or repeated prose about what stays unchanged. | Run the same likely-mistake test as in English. Keep necessary limits and consent facts; remove the defense of the implementation. |
| Mixed terminology | Alternating 标签/标签页/tab, 固定/pin, or different names for the same settings page. | Use one agreed term per object and match the UI named in instructions. Treat a terminology change as a coordinated copy decision. |

The repository records specific terminology preferences: keep `agent` rather than introducing 智能体, and preserve the selected PowerShell 整合 term unless it is deliberately changed everywhere. Do not “correct” these by literal dictionary translation. Product names such as Folio, PowerShell, Claude Code, Codex, and Copilot CLI retain their official spelling. Avoid English plurals applied to Chinese category labels. Raw identifiers stay exact in troubleshooting text.

### Punctuation and spacing

W3C's Chinese layout document describes Chinese punctuation in mixed text and regional quotation conventions. It is layout guidance, not a mandate for one source-code spacing policy. The following are proposed Folio conventions. [W3C Chinese text layout](https://www.w3.org/TR/clreq/).

- Use Chinese sentence punctuation in Chinese prose: `，。；：？！` and `（）`. Labels and buttons have no final period; complete descriptions have one. Use `、` for short parallel items where appropriate.
- For horizontal Simplified Chinese prose, default to `“”` and nested `‘’`. Existing `「」` usage needs one consistent editorial decision, not isolated replacements. In Markdown instructions, bold exact UI labels instead of decorating every label with quotation marks.
- Use one ordinary space between Chinese text and a Latin name, number, or inline-code token where they directly meet. Keep full-width punctuation adjacent to surrounding text. This is a repository consistency choice, not a claim that Chinese grammar requires ASCII spaces.
- Preserve spelling and internal spaces in names. Keep versions, paths, URLs, environment variables, placeholders, shell commands, and shortcuts byte-for-byte intact. Never insert full-width punctuation or additional spaces inside executable syntax.
- Keep shortcut notation consistent, such as `Ctrl+Shift+P`. Follow the exact displayed shortcut in an instruction. Use one convention for measurements and percentages; avoid breaking a number away from its unit in layout.
- Use `……` for a prose ellipsis. Reserve a single `…` for an established UI convention when needed. Do not add ellipses to every action or use them as substitutes for clear progress text.
- Split long explanatory asides into sentences. Use Chinese paired dash punctuation sparingly in prose; do not import English dash chains or use them in this guide's rules.
- Inspect rendered wrapping: a closing punctuation mark should not begin a line, and an opening bracket or quote should not be stranded at a line end. Spacing alone cannot fix clipping or layout problems.

### Review checklist for a second model

Give the reviewer the string identifier or document section, its surface, neighboring labels/actions, the Chinese text, a factual behavior brief, placeholders, and any relevant default, scope, or timing. Then provide the English for the fidelity pass. Do not ask it to improve isolated strings without that context.

1. **Task:** State in one English sentence what the reader must choose, do, or learn. Flag every clause that does not serve that task.
2. **Naturalness:** Read the Chinese without the English. Identify the object, action, and condition immediately. Flag clauses that require mentally reconstructing an English sentence.
3. **Pattern scan:** Check all ten tells above. Quote the exact offending fragment and explain the reading problem. Do not fail a string solely because it contains 被, 的, 其, or 不会.
4. **Fidelity:** Compare triggers, scope, defaults, exceptions, quantities, and effective time. Distinguish input waiting from turn completion, saving from activation, hiding from closing, and typing a command from running it.
5. **Consequences:** Confirm that important file edits, permissions, dependencies, and executable actions remain clear. Distinguish useful disclosure from repetitive reassurance.
6. **Terminology:** Match existing object names, approved terminology, product capitalization, and the localized navigation path. Flag untranslated English UI names unless the destination really displays them.
7. **Syntax safety:** Compare placeholders and code spans exactly. Preserve braces, argument names, paths, environment-variable names, escapes, and shortcut characters. Flag the surrounding grammar if a dynamic value makes it awkward.
8. **Surface fit:** Apply section 2. Check complete label/description pairs for repetition. Flag important content available only on hover or in an ephemeral toast for a separate UI review.
9. **Typography:** Check punctuation, paired marks, Latin spacing, units, and line wrapping. Text-only review must mark rendered fit as unverified.
10. **Independent reread:** Read the complete Chinese flow aloud after the fidelity pass. Check that it sounds consistent across the README, first-run card, Settings, and resulting toast.

Return a table with `location | quoted fragment | issue | user impact | required editorial action | facts to preserve | verdict`. Use `pass`, `revise`, or `needs behavior verification`. Give editorial instructions, not Chinese replacement copy. Separate factual errors, meaning changes, and broken identifiers from optional style preferences. Do not approve until material fidelity issues are resolved; do not invent implementation facts to resolve them.

## 5. Sources

All sources below are first-party guides, documentation, source code, or published product notes, accessed 7 September 2026. Rolling changelogs and source files can change. Examples are selective evidence for writing patterns, not a ranking of products or an endorsement of every sentence. Numerical length targets and Chinese diagnostic heuristics are Folio recommendations.

| Source | Used for |
| --- | --- |
| [Microsoft: Top 10 tips for style and voice](https://learn.microsoft.com/en-us/style-guide/top-10-tips-style-voice) | Concision, familiar language, front-loading, sentence case, and label punctuation. |
| [Apple HIG: Writing](https://developer.apple.com/design/human-interface-guidelines/writing?changes=l_1) | Action labels, settings descriptions, errors, pronouns, consistency, and localization. The query variant exposes the same HIG article to the research reader. |
| [Google developer documentation: Highlights](https://developers.google.com/style/highlights) | Active voice, second person, condition placement, global readers, and formatting. |
| [GitHub Docs: Style guide](https://docs.github.com/en/contributing/style-guide-and-content-model/style-guide) | Clarity, selective alerts, and distinct feature, fix, change, and known-issue release notes. |
| [GOV.UK: Identify user needs](https://guidance.publishing.service.gov.uk/writing-to-gov-uk-standards/plan-manage-content/identify-user-needs/) | Testing whether content helps a reader complete a task. |
| [GOV.UK: Use clear language](https://guidance.publishing.service.gov.uk/writing-to-gov-uk-standards/writing-guidelines/clear-language/) | Plain language for readers with varied literacy and language backgrounds. |
| [Stripe: Automate customer emails](https://docs.stripe.com/billing/revenue-recovery/customer-emails) | Published action-and-trigger setting wording. |
| [Vercel: Redeploy without leaving project settings](https://vercel.com/changelog/redeploy-without-leaving-project-settings) | A toast tied to a required next action and a concrete release announcement. |
| [Raycast: Windows changelog](https://www.raycast.com/changelog/windows) | Feature-prefixed fixes and recognizable symptoms, including July 2026 entries. |
| [Windows Terminal: Interaction settings](https://learn.microsoft.com/en-us/windows/terminal/customize-settings/interaction) | Behavior-led settings headings and separate reference detail. |
| [Windows Terminal: Releases](https://github.com/microsoft/terminal/releases) | Fix bullets identifying selection, rendering, and workflow symptoms. |
| [Ghostty: Configuration reference](https://ghostty.org/docs/config/reference) | Defaults, values, and exceptions in reference documentation. |
| [Ghostty: 1.0.1 release notes](https://ghostty.org/docs/install/release-notes/1-0-1) | Linking a new option value to the resulting behavior. |
| [Warp: Desktop notifications](https://docs.warp.dev/terminal/more-features/notifications) | User events first, app and OS prerequisites afterward. |
| [Warp: 2025 changelog](https://docs.warp.dev/changelog/2025/) | Dated fixes scoped by action, mode, and platform. |
| [VS Code: Terminal configuration source](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/contrib/terminal/common/terminalConfiguration.ts) | Actual localizable setting descriptions, including copy on selection. |
| [VS Code: November 2024, version 1.96](https://code.visualstudio.com/updates/v1_96) | Task-oriented terminal changes with setting names and prerequisites. |
| [Zed: All settings](https://zed.dev/docs/reference/all-settings) | Behavior description separated from key, default, and example. |
| [Zed: Stable releases](https://zed.dev/releases/stable) | Observable fix descriptions and links to technical discussion. |
| [W3C: Requirements for Chinese Text Layout](https://www.w3.org/TR/clreq/) | Mixed Chinese/Latin typography, punctuation, quotation marks, and line-breaking conventions. |

Repository evidence: [English README](../../../README.md), [Simplified Chinese README](../../../README.zh-CN.md), [UI string catalog](../../../crates/bt-app/src/i18n.rs), [first-run behavior](../../../crates/bt-app/src/first_run.rs), and [0.2.2 release draft](../release/release-note-v0.2.2-preview.md).
