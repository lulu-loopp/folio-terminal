# 设置页文案重写 — 逐行对照表（2026-08-26）

规范：`docs/plans/ui-style/copy-guide.md`。本页是按那份规范走完一遍全部设置行之后的**逐行账**。

**语义未改。** 每一条新句子说的行为都与代码一致；不确定的地方去读了代码（`RailMode` 的两档、`Alt`+滚轮瞄准的落点、`ContextMenuVerb` 是按当前界面语言写进注册表的、`ImageOpacity` 的下限是 0 而不是 `MINIMUM_BACKGROUND_OPACITY`），没有猜。

**红门**（本片新加两道，先红后绿）：

- `i18n::tests::no_settings_sentence_uses_the_words_this_window_only_says_to_itself` —— 禁语表，英中各一张，逐条走 `Text::ALL` 两列。
  红证：`DescTerminalFont says "the grid", which belongs in the source and not on the screen: "The face the grid is drawn in; the window's own labels keep theirs"`
- `i18n::tests::chinese_sentences_are_punctuated_the_chinese_way` —— 汉字后面不许直接跟半角 `,` `;` `:` `!` `?`。
  红证：`ProfilesRowEnvDesc punctuates ',' the half-width way after a Han character: "为该档案的会话设置,覆盖 Folio 设置的值"`

既有钉子全绿：`every_entry_of_the_table_is_listed_once_and_reads_in_both_languages`、`the_card_column_is_called_cards_on_every_surface_that_names_it`（`Focus mode` 不得复现）、`the_focus_mode_row_names_the_chord_and_no_gesture_this_build_withdrew`、`no_string_in_the_window_speaks_in_the_first_person`、`no_string_in_this_table_asks_for_a_relaunch`、`no_entry_ships_the_english_word_as_its_own_translation`、`every_chinese_entry_carries_at_least_one_han_character`、`settings::tests::no_settings_sentence_needs_a_fourth_line`（三行上限）。

**`Text::ALL` 505 → 508**：只改串本来不动变体，但**实机双语截图查出这一页还有三个控件根本没进过表**——见 §10。

共 **80** 条改动（74 条重写 + 3 条新入表 + 3 条实机复核后的中文再改，后者计在 §11）。

---

## 0. 三条术语统一（跨页）

一个对象只许有一个名字。三处两名并存，本片收口：

| 对象 | 原先并存 | 收口为 | 涉及条目 |
|---|---|---|---|
| profile | 「档案」14 处 vs「配置文件」3 处 | ~~档案~~ → **配置文件**（用户 2026-08-29 推翻，见下） | `RowDefaultProfile`、`ChooseProfile`、`unknown_profile_banner_text` |
| integration | 「集成」vs「整合」 | ~~集成~~ → **整合**（同上） | `RowPowerShellOffer` |
| split | 「拆分」（菜单/快捷键）vs「分屏」（设置行） | **拆分** | `RowSplitDirection`、`DescSplitDirection`、`ShortcutSplitVertical`（纵向→竖向，跟 `OptionVertical` 走） |

另有 `ProfilesInherit` 的中文里裸着一个英文 `pane`（「当前 pane 的文件夹」），已改「窗格」。

**2026-08-29 用户推翻前两行。** `profile` 收口到 **「配置文件」**（行内指代「这个配置」「默认配置」），`integration` 收口到 **「整合」**。理由不是哪个词更好听，是**读者从哪儿来**：中文版 Windows Terminal 管同一个对象叫「配置文件」，而「整合」这个词在提示条上本来就已经在用了。同批还改了 `agent` 一族的中文口径（见本仓 §7.41 与 `copy-guide.md` §3b）。

---

## 1. General

| 条目 | 旧英 | 新英 | 旧中 | 新中 | 理由 |
|---|---|---|---|---|---|
| `DescKeyHints` | Holding a modifier for a moment lists the shortcuts it starts. The card never takes a key. | Hold a modifier for a moment and this window lists the shortcuts that start with it. The list never takes a keystroke. | 按住修饰键片刻,列出以它开头的快捷键。这张卡片不吃任何按键。 | 按住修饰键片刻，这扇窗会列出以它开头的快捷键。这个列表不会截走任何按键。 | 「卡片」在这一行指的是弹出框，与 Appearance 的 `Cards` 撞名；中文半角逗号 |
| `DescGitPanel` | A Git page beside the file tree; off reads no repository | Adds a Git page to the files column. Off, Folio never reads a repository. | 文件树旁的 Git 页；关闭则不读取任何仓库 | 在文件列里加一页 Git。关闭时，Folio 不会读取任何仓库。 | 名词短语改成「打开时发生什么」；「file tree」统一为快捷键页已在用的 files column／文件列 |
| `DescSearchEngine` | Where a web preview's address field sends text that is not an address | Where a web preview searches when what you type in its address bar is not an address. | 在网页预览的地址栏里打了一段不是地址的文字时,交给哪个搜索引擎 | 在网页预览的地址栏里输入的不是地址时，交给哪个搜索引擎去搜。 | 第二人称直述；中文半角逗号 |
| `RowDefaultProfile` | Default profile | Default profile | 默认配置文件 | 默认档案 | 术语统一 |
| `DescDefaultProfile` | What opens on a new tab, and when Folio starts | Which profile a new tab opens, and which one Folio starts with. | 新建标签时、以及 Folio 启动时打开什么 | 新建标签时打开哪个档案，以及 Folio 启动时用哪个。 | 「when Folio starts」读起来像时刻而不是场合；说清楚选的是档案 |
| `ChooseProfile` | Choose a profile | Choose a profile | 选择配置文件 | 选择档案 | 术语统一 |
| `RowContextMenu` | Explorer context menu | Explorer context menu | 资源管理器右键菜单 | 资源管理器菜单 | 中文行名收进 7 字 |
| `DescContextMenu` | Open Folio here, under Windows 11's Show more options | Adds Open Folio here to Explorer's right-click menu. Windows 11 files it under Show more options. | Open Folio here，在 Windows 11 的显示更多选项里 | 在资源管理器的右键菜单里加上「在 Folio 中打开」。Windows 11 把它收在「显示更多选项」下面。 | 片语改成完整两句；**事实修正**：注册表里写的动词按界面语言走，中文窗口写的是「在 Folio 中打开」，原文案引的是英文串 |
| `ContextMenuAddedToast` | （不变） | （不变） | Open Folio here 已进入资源管理器菜单，在显示更多选项里 | 「在 Folio 中打开」已进入资源管理器菜单，在「显示更多选项」下面 | 同上；引号补全 |

`RowTheme` / `DescTheme` / `RowLanguage` / `DescLanguage` / `RowKeyHints` / `RowGitPanel` / `RowSearchEngine` 原样合格，未动（`DescLanguage` 另有钉子逐字钉死）。

---

## 2. Appearance

| 条目 | 旧英 | 新英 | 旧中 | 新中 | 理由 |
|---|---|---|---|---|---|
| `DescLightScheme` | The colours a Light window is drawn in, terminal and chrome alike | The palette a light window uses, for terminal text and for the window around it. | 浅色窗口所用的颜色，终端与窗口自身同出一套 | 浅色窗口用的配色，终端与窗口本身共用一套。 | **禁语 `chrome`**；中文长度见 §11 |
| `DescDarkScheme` | The same for a Dark window. Files: %APPDATA%\Folio\schemes | The same for a dark window. Your own scheme files go in %APPDATA%\Folio\schemes. | 深色窗口同理。文件放在 %APPDATA%\Folio\schemes | 深色窗口同理。你自己的配色文件放在 %APPDATA%\Folio\schemes。 | 「Files:」是标签不是句子；说清楚那是**你自己的**文件 |
| `DescTerminalFont` | The face the grid is drawn in; the window's own labels keep theirs | The font terminal text is drawn in. Tabs, menus and this dialog keep their own. | 网格所用的字体；窗口自身的文字保持原样 | 终端文字使用的字体。标签、菜单和这个对话框保持各自的字体。 | **禁语 `the face` / `the grid`**；「窗口自身的文字」改成读者数得出来的三样 |
| `DescFontSize` | How large grid text is drawn, before the display's scaling | How large terminal text is, before your display's scaling is applied. | 网格文字画多大，尚未乘显示器的缩放 | 终端文字有多大，尚未计入显示器的缩放。 | **禁语 `grid`**；「乘」是实现细节 |
| `DescCursor` | Focused cursor shape | The shape the cursor takes in the pane you are typing in. | 聚焦时的光标形状 | 你正在输入的那个窗格里，光标是什么形状。 | 三字片语改成一句话；「聚焦」是内部说法 |
| `DescTabLayout` | Choose where tabs appear in the window | Whether tabs run along the top of the window or down its side. | 选择标签在窗口里的位置 | 标签是横排在窗口顶部，还是竖排在侧边。 | 说清楚两档各是什么，而不是叫读者去选 |
| `DescSidebar` | How the vertical tab sidebar rests | Expanded keeps the vertical tab strip open beside the terminal. Icons parks it as a narrow strip that opens over the terminal. | 竖向标签栏平时的形态 | 「展开」让竖排标签栏一直开在终端旁边；「图标」把它收成一条窄条，需要时覆盖在终端上打开。 | 「形态」没说任何事；两档各说一句（`RailMode` 的两个变体原义） |
| `DescFocusMode` | The tab strip becomes a column of cards, one per tab; the tab you pick fills the window whole, splits and all. Ctrl+Shift+Z turns this same setting | The tab strip becomes a column of cards, one per tab, and the tab you pick fills the window whole. Ctrl+Shift+Z turns this same setting. | 标签条变成一列卡片,一张卡一个标签;点中的那个标签整个占满窗口,分屏原样。Ctrl+Shift+Z 拨的是同一个开关 | 标签条变成一列卡片，一张卡一个标签；选中的那个标签占满整扇窗口。Ctrl+Shift+Z 拨的是同一个开关。 | 砍掉「splits and all／分屏原样」这一层附注；中文半角标点；`Ctrl+Shift+Z` 原样保留（钉子要求） |
| `DescFocusCardHeight` | How tall each card's picture of its tab stands. At the default face a lone pane holds 12 rows at 160, 20 at 240, 27 at 320. Alt+wheel over a seat scrolls it a row per notch | How tall the picture of a tab stands inside its card. Alt+wheel over one of its panes scrolls that picture a row at a time. | 每张卡片里那幅 tab 缩图的高度。默认字面下,单 pane 在 160 装得下 12 行、240 装 20 行、320 装 27 行。在格子上 Alt+滚轮滚动这幅图,一格一行 | 每张卡片里那幅标签缩图有多高。在卡片中某个窗格上 Alt+滚轮，可以一行一行地滚动那幅图。 | **用户挂的那一笔**：三句砍到两句，九个数字全部去掉（下拉里就写着 160/240/320）；**禁语 `seat` / 「格子」/「字面」**；裸着的 `tab` / `pane` 归中文。`Alt`+滚轮那一句保留——DESIGN §7.1.6b′ 明记这是那个手势唯一被写下来的地方 |
| `DescBackgroundImage` | Drawn once behind the window, beneath every pane | A picture drawn behind the whole window, under every pane. | 在整扇窗口背后画一次，位于每个窗格之下 | 在整扇窗口背后画一张图片，位于所有窗格之下。 | 「画一次」是实现细节（一次 vs 每格一次） |
| `DescImageFit` | How the picture meets a window that is not its shape | How the picture meets a window that is not its shape: stretched, filled or tiled. | 图片与形状不同的窗口如何相配 | 图片与窗口形状不同时怎么铺开：拉伸、填充或平铺。 | 补上三档各是什么，用词与下拉逐字对齐 |
| `DescImageOpacity` | How much of the picture reaches the window | How much of the picture you see. At 0 the window is drawn without it. | 图片有多少落到窗口上 | 图片显示出多少。为 0 时窗口不画它。 | 「落到窗口上」是画法；补上 0 这一端的行为（下限确实是 0） |
| `DescBackgroundOpacity` | Panes and the window ground; text and menus stay opaque | How much of the desktop shows through panes and the window behind them. Text and menus stay solid. | 作用于窗格与窗口底色；文字与菜单保持不透明 | 桌面透过窗格和它们背后的窗口显出多少。文字与菜单保持不透明。 | **禁语 `the ground`**；前半句原本只列了作用面，没说这一行到底干什么 |
| `DescBackgroundOpacityUnavailable` | This window is composited opaque | This window is drawn opaque and cannot let the desktop through. | 此窗口以不透明方式合成 | 这扇窗口以不透明方式绘制，无法让桌面透过来。 | 「composited／合成」是渲染词；补上后果 |
| `DescAcrylic` | A Windows blur behind the ground. Visible only below full opacity | Blurs whatever sits behind the window. Visible only when background opacity is below 100%. | 底色之后的 Windows 模糊。仅在不透明度低于 100% 时可见 | 把窗口背后的东西模糊掉。仅在背景不透明度低于 100% 时可见。 | **禁语 `the ground`**；「哪个不透明度」原文没说，读者面前有两个滑块 |
| `DescAcrylicUnavailable` | This Windows has no system backdrop to draw | This version of Windows does not offer the blur. | 此版本 Windows 没有可用的系统背景材质 | 这个版本的 Windows 不提供这种模糊。 | `system backdrop`／「系统背景材质」是 DWM 的 API 名，读者不认识 |
| `DescAlwaysOnTop` | The window stays above other windows | This window stays above every other window. | 窗口保持在其他窗口之上 | 这扇窗口保持在其他所有窗口之上。 | 「the window」指代不清（哪扇）；补句号 |
| `RowSplitDirection` | Split direction | Split direction | 分屏方向 | 拆分方向 | 术语统一 |
| `DescSplitDirection` | Where a split with no direction of its own puts the new pane | Where a new pane lands when the split you asked for names no direction of its own. | 没有自带方向的分屏把新窗格放在哪边 | 拆分时没有指明方向的话，新窗格落在哪一边。 | 主语从「分屏」换回读者的动作；术语统一 |
| `DescMinimumContrast` | Lightens or darkens terminal text to meet this ratio against its cell. Backgrounds never change, nor does anything outside the grid. Above Off it overrides a program's colours | Lightens or darkens terminal text until it meets this ratio against the cell behind it. Backgrounds never change, and above Off this overrides the colours a program asked for. | 把终端文字提亮或压暗,直到与它所在格子的底色达到这个对比度。底色一律不动,网格以外的一切也不动。Off 以上的档位会覆盖程序指定的颜色 | 把终端文字提亮或压暗，直到与它背后的单元格达到这个对比度。背景一律不动；Off 以上的档位会覆盖程序指定的颜色。 | **用户挂的那一笔**：三句并两句；**禁语 `the grid` / 「格子」**；「网格以外的一切也不动」这层由「终端文字」四个字带过 |

`RowLightScheme` / `RowDarkScheme` / `RowTerminalFont` / `RowFontSize` / `RowCursor` / `RowTabLayout` / `RowSidebar` / `RowFocusMode`(`Cards`) / `RowFocusCardHeight` / `RowBackgroundImage` / `RowImageFit` / `RowImageOpacity` / `RowBackgroundOpacity` / `RowAcrylic` / `RowAlwaysOnTop` / `RowMinimumContrast` 行名原样合格。

---

## 3. Terminal

| 条目 | 旧英 | 新英 | 旧中 | 新中 | 理由 |
|---|---|---|---|---|---|
| `PsReadLineProbing` | Checking this machine's PSReadLine | Checking which PSReadLine this machine has | 正在检查本机的 PSReadLine | 正在检查本机装的是哪个 PSReadLine | 说清楚在查什么（版本），而不是「在查它」 |
| `psreadline_row_outdated_in` | {found} on this machine · the resize repair does nothing | {found} on this machine · resizing still misplaces the input line | 本机为 {found} · 缩放修复不起作用 | 本机为 {found} · 缩放窗口时输入行仍会错位 | 「修复不起作用」只说了内部动作；读者要知道的是屏幕上会发生什么 |
| `psreadline_row_current_in` | {found} on this machine · already anchors itself | {found} on this machine · already keeps the input line in place | 本机为 {found} · 已自带正确的锚点 | 本机为 {found} · 已能自己守住输入行 | 「锚点」是实现词 |
| `RowPowerShellOffer` | Offer PowerShell integration | Offer PowerShell integration | 提示安装 PowerShell 整合 | PowerShell 集成提示 | 术语统一（整合→集成）；行名收短 |
| `DescPowerShellOffer` | A PowerShell pane whose $PROFILE does not load folio.ps1 shows one line offering to add it. Off stops the offer; a $PROFILE that already loads it is never asked about | A PowerShell pane whose $PROFILE does not load folio.ps1 offers to add it. Off, the offer never appears. | PowerShell 窗格的 $PROFILE 没有加载 folio.ps1 时,在那个窗格里显示一行,提供加入的动作。关闭后不再提示;已经加载的 $PROFILE 本来就不会被问 | PowerShell 窗格的 $PROFILE 没有加载 folio.ps1 时，会提示把它加进去。关闭后不再提示。 | 第三小句是第一句的逆否，删；中文半角标点 |
| `DescScrollback` | Each pane keeps this many lines; the oldest go first | How many lines each pane keeps. Past that, the oldest lines are dropped. | 每个窗格保留这么多行,最旧的先走 | 每个窗格保留多少行。超出之后，最旧的行会被丢掉。 | 「the oldest go first」没说前提（超出之后）；中文半角逗号 |
| `DescLineWrappingOn` | Long lines fold at the pane's edge. | Lines longer than the pane fold at its edge. | 长行在窗格边缘折行。 | 比窗格宽的行在边缘折行。 | 「long」相对谁？说清楚是相对窗格宽度 |
| `DescLineWrappingOff` | Long lines run on and the pane scrolls sideways: Shift+wheel, or the bar along the pane's foot. | Lines longer than the pane run on, and the pane scrolls sideways with Shift+wheel or the bar along its foot. | 长行一直延伸,窗格横向滚动:Shift+滚轮,或窗格底边那条横条。 | 比窗格宽的行一直延伸，窗格可以横向滚动：Shift+滚轮，或底边那条横条。 | 与 On 那句同句式；冒号后面接手势读成定义，改成 with；中文半角标点 |
| `DescNotifications` | Programs that ask can put a message on the desktop. Nothing arrives while the pane is on screen in a focused window | A program that asks for one can put a message on your desktop. Nothing appears while its pane is on screen in the focused window. | 程序主动要求时,可以在桌面上放一条消息。窗格正在这扇窗里显示、而这扇窗又是聚焦的,就什么都不弹 | 程序主动请求时，可以在你的桌面上放一条消息。它所在的窗格正显示在聚焦的窗口里时，什么都不弹。 | 「the pane」指代不清（哪个窗格）；中文后半句从两个并列小句收成一句；中文半角标点 |

`RowPsReadLine` / `PsReadLineRowGone` / `psreadline_row_installed_in` / `psreadline_row_update_in` / `RowScrollback` / `RowLineWrapping` / `RowNotifications` / `PsReadLineRemovedToast` 原样合格。

---

## 4. Agents（三个安装行同句式）

模板：`Adds <什么> to your <路径>, so <程序> tells this window when <事件>. <一句边界>.`

| 条目 | 旧英 | 新英 | 旧中 | 新中 | 理由 |
|---|---|---|---|---|---|
| `DescClaudeHooks` | Adds hooks to your own ~/.claude/settings.json so Claude Code tells this window when it is waiting for you. Nothing is written into a folder or a repository | Adds hooks to your ~/.claude/settings.json, so Claude Code tells this window when it is waiting for you. Nothing is written into a project folder. | 向你自己的 ~/.claude/settings.json 写入 hooks,Claude Code 在等你回答时会告诉这扇窗。不往任何文件夹或仓库里写 | 在你的 ~/.claude/settings.json 里加上 hooks，让 Claude Code 在等你回答时告诉这扇窗。不会写进任何项目文件夹。 | 三行统一句式；「a folder or a repository」两个词说一件事，收成 project folder／项目文件夹 |
| `DescCodexNotify` | Adds a notify program to your own ~/.codex/config.toml so codex says when a turn has ended. It does not report a codex waiting for you, and writes nothing into a repository | Adds a notify program to your ~/.codex/config.toml, so codex tells this window when a turn has ended. It does not report a codex waiting for you. | 向你自己的 ~/.codex/config.toml 写入 notify 程序,codex 结束一个回合时会说一声。它不报告 codex 在等你回答,也不往仓库里写 | 在你的 ~/.codex/config.toml 里加上 notify 程序，让 codex 在一个回合结束时告诉这扇窗。它不报告它在等你回答。 | **用户挂的那一笔**：并列小句砍掉一条，三行同句式；中文半角逗号；末句长度见 §11 |
| `DescCopilotHooks` | Adds a hook file to your own ~/.copilot/hooks/ so copilot CLI tells this window when it is waiting for you. Nothing is written into a folder or a repository | Adds a hook file to your ~/.copilot/hooks/, so Copilot CLI tells this window when it is waiting for you. Nothing is written into a project folder. | 向你自己的 ~/.copilot/hooks/ 写入一个 hook 文件,copilot CLI 在等你回答时会告诉这扇窗。不往任何文件夹或仓库里写 | 在你的 ~/.copilot/hooks/ 里加上一个 hook 文件，让 Copilot CLI 在等你回答时告诉这扇窗。不会写进任何项目文件夹。 | 同上；`Copilot CLI` 与行名同一种写法 |
| `DescCopilotHooksTooOld` | This machine's copilot CLI is older than 1.0.26, which is the first version that reports a permission prompt only when one was shown to you | This machine's Copilot CLI is older than 1.0.26. Earlier versions report permission prompts you were never shown. | 这台机器上的 copilot CLI 早于 1.0.26,而 1.0.26 才是只在真的问了你时才报权限提示的第一个版本 | 本机的 Copilot CLI 早于 1.0.26。更早的版本会报告并没有真的问过你的权限提示。 | 一句绕两圈的从句改成两句正述；中文半角逗号 |
| `DescCopilotHooksDisabled` | Hooks are switched off in your own ~/.copilot/settings.json, so a hook file installed here would not run | Hooks are switched off in your ~/.copilot/settings.json, so a file installed here would not run. | 你自己的 ~/.copilot/settings.json 里关掉了 hooks,装在这里的 hook 文件不会运行 | 你的 ~/.copilot/settings.json 里关掉了 hooks，装在这里的文件不会运行。 | 「your own／你自己的」三行统一去掉「own」；中文半角逗号 |
| `DescTurnEndNotifications` | When an agent stops talking, flash this window's taskbar button, or put a message on the desktop if the window is minimised. The marks inside the window are unaffected | When an agent finishes a turn, this window's taskbar button flashes, or a message goes to your desktop if the window is minimised. Marks inside the window are unaffected. | Agent 说完一个回合时,闪这扇窗的任务栏按钮;窗最小化了就在桌面上放一条消息。窗内的记号不受这一行影响 | 一个回合结束时，闪动这扇窗的任务栏按钮；窗口最小化时改为在桌面上放一条消息。窗内的记号不受影响。 | 祈使句（flash / put）改陈述句；「stops talking」与行名 `Turn finished` 不是一个词；中文半角标点；中文丢主语的理由见 §11 |

`RowClaudeHooks` / `RowCodexNotify` / `RowCopilotHooks` / `RowTurnEndNotifications` 以及三条 Added/Removed/Failed toast 原样合格。

---

## 5. Rendered blocks（三个开关同句式）

模板：`<动词> <什么> in command output. Off, <原文> is shown as it was printed.`

| 条目 | 旧英 | 新英 | 旧中 | 新中 | 理由 |
|---|---|---|---|---|---|
| `DescFormulas` | Typeset $$…$$ blocks; off shows the LaTeX source | Typesets $$…$$ blocks in command output. Off, the LaTeX source is shown as it was printed. | 排版 $$…$$ 块；关闭则显示 LaTeX 源码 | 排版命令输出里的 $$…$$ 块。关闭时显示 LaTeX 源码原文。 | 三行统一句式；补上「in command output／命令输出里的」这个作用域（原来只有两行有）；中文长度见 §11 |
| `DescInlineFormulas` | Typeset $…$ in command output; off shows the source | Typesets $…$ in command output. Off, the source is shown as it was printed. | 排版命令输出里的 $…$；关闭则显示源码 | 排版命令输出里的 $…$。关闭时显示源码原文。 | 同上 |
| `DescTables` | Draw markdown tables in output; off shows the pipe text | Draws markdown tables in command output. Off, the pipe characters are shown as they were printed. | 绘制输出里的 markdown 表格；关闭则显示管道符原文 | 画出命令输出里的 markdown 表格。关闭时显示管道符原文。 | 同上；`in output` → `in command output` 与两个兄弟行对齐 |
| `DescBlockMaxHeight` | Blocks taller than this scroll inside themselves | A rendered block taller than this scrolls inside itself instead of growing. | 更高的块在自己内部滚动 | 超过这个高度的渲染块会在自己内部滚动，而不是继续变高。 | 「Blocks」是哪种块？页名叫 Rendered blocks，句子里说全；补上「而不是继续变高」这一半 |

`RowFormulas` / `RowInlineFormulas` / `RowTables` / `RowBlockMaxHeight` / `OptionBlockHeightNone` 原样合格。

---

## 6. Profiles

| 条目 | 旧英 | 新英 | 旧中 | 新中 | 理由 |
|---|---|---|---|---|---|
| `ProfilesRowNameDesc` | What this profile is called on tabs, in the picker and in this list | What this profile is called on tabs, in the profile picker and in this list. | 标签、选择器与本列表上写的名字 | 这个档案在标签、选择器和这份列表里显示的名字。 | 中文丢了主语（谁的名字）；`the picker` 指代不清 |
| `ProfilesRowProgramDesc` | The executable a new tab of this profile starts | The program a new tab of this profile starts. | 该档案新建标签时启动的可执行文件 | 这个档案新建标签时启动的程序。 | 行名叫 Program／程序，说明里改口叫 executable／可执行文件 |
| `ProfilesRowStartingDirDesc` | Where a new tab of this profile opens | The folder a new tab of this profile opens in. | 该档案的新标签在哪里打开 | 这个档案的新标签在哪个文件夹里打开。 | 「哪里」不够具体，行名说的是目录 |
| `ProfilesRowColourDesc` | The mark that names this profile across the window | The colour that marks this profile on tabs and in menus. | 窗口各处标示该档案的那个标记 | 在标签和菜单里标示这个档案的颜色。 | 行名叫 Colour／颜色，说明里改口叫 mark／标记；「across the window／窗口各处」不落地 |
| `ProfilesRowArgsDesc` | Passed to the program ahead of anything the shell reads · spaces separate, double quotes group | Passed to the program before it reads anything of its own. Spaces separate; double quotes group. | 在 shell 读取任何东西之前传给程序 · 空格分词，双引号成组 | 在程序读取自己的任何内容之前传给它。空格分词，双引号成组。 | `·` 分隔两句不同性质的话，拆成两句；这一行对任何程序成立，不只是 shell |
| `ProfilesRowEnvDesc` | Set for this profile's sessions, over what Folio sets | Set for every session this profile starts, over what Folio sets itself. | 为该档案的会话设置,覆盖 Folio 设置的值 | 为这个档案启动的每个会话设置，覆盖 Folio 自己设的值。 | 中文半角逗号；「Folio 设置的值」漏了「自己」，读起来像是覆盖用户设的 |
| `ProfilesRowHyperlinkDesc` | FORCE_HYPERLINK, the answer programs read before emitting a link | Sets FORCE_HYPERLINK, which programs read before deciding to emit a link. | FORCE_HYPERLINK，程序发出链接前读的那个回答 | 设置 FORCE_HYPERLINK，程序在决定是否输出链接前会读它。 | 同位语片语改成一句；说清楚这一行**设置**它 |
| `ProfilesInherit` | The current pane's folder | The current pane's folder | 当前 pane 的文件夹 | 当前窗格的文件夹 | 中文里裸着一个英文 `pane` |
| `ProfilesCannotHideFallback` | Every fallback lands on this profile | Folio falls back to this profile when another cannot start | 每一次降级都落在这个档案上 | 别的档案起不来时，Folio 会退到这个档案 | `fallback`／「降级」是内部说法，说清楚什么时候会发生 |
| `CapFullNoLinks` | （不变） | （不变） | 提示符标记、目录与退出码;没有超链接 | 提示符标记、目录与退出码；没有超链接 | 中文半角分号 |
| `CapPowerShellNoLinks` | （不变） | （不变） | 已点源 folio.ps1 时有…;没有超链接 | 已点源 folio.ps1 时有…；没有超链接 | 同上 |
| `CapWslBashNoLinks` | （不变） | （不变） | 仅 bash 登录时有…;没有超链接 | 仅 bash 登录时有…；没有超链接 | 同上 |
| `CapCmdNoLinks` | （不变） | （不变） | 目录;没有提示符标记,没有退出码,没有超链接 | 目录；没有提示符标记，没有退出码，没有超链接 | 同上 |

`ProfilesRowName/Program/StartingDir/Colour/Args/Env/Hyperlink/Integration` 行名、`ProfilesCannotHideDefault`、五条 `Cap*` 正常变体原样合格。

---

## 7. Shortcuts

| 条目 | 旧英 | 新英 | 旧中 | 新中 | 理由 |
|---|---|---|---|---|---|
| `ShortcutJumpAttention` | Jump to the longest waiting | Jump to the longest waiting pane | 跳到等待最久的那个 | 跳到等待最久的窗格 | 「the longest waiting／最久的那个」缺宾语 |
| `ShortcutDuplicatePaneSplit` | （不变） | （不变） | 把窗格复制到一个拆分里 | 复制窗格到拆分里 | 中文动宾收短，与同页其他行同重 |
| `ShortcutGitPage` | （不变） | （不变） | 把文件列切到 Git | 文件列切到 Git | 同上 |
| `ShortcutOpenSearch` | （不变） | （不变） | 在这个窗格里查找 | 在本窗格中查找 | 同上 |
| `ShortcutSplitVertical` | （不变） | （不变） | 纵向拆分 | 竖向拆分 | 与 `OptionVertical`（竖向）统一 |
| `ShortcutSummonPip1`–`4` | Summon PiP slot N | Summon picture in picture N | 唤出 PiP 槽位 N | 唤出画中画 N | `PiP` 是缩写、`slot`／「槽位」是内部说法 |
| `ShortcutFamilySummonPip` | Summon PiP slot 1–4 | Summon picture in picture 1–4 | 唤出 PiP 槽位 1–4 | 唤出画中画 1–4 | 同上（四行 + 族名同句式） |
| `ShortcutScopeWebPage` | On a page | On a page | 键盘在网页里时 | 在网页里时 | 中文比英文多解释了一层「键盘」 |
| `ShortcutNotePending` | Bound; the verb behind it is still to come | Bound, but the action behind it is not built yet | 已绑定；它背后的功能还没有到 | 已绑定，但它背后的功能还没有做出来 | 「the verb／动词」是内部说法 |
| `ShortcutUndelivered` | Windows does not hand every chord to a program; one that never appears in the box cannot be recorded here | Windows keeps some combinations for itself; one that never reaches the box cannot be recorded here | Windows 不会把每一个和弦都交给程序;在框里始终不出现的和弦,这里录不到 | Windows 会自己截走一部分组合键；始终到不了框里的，这里录不到 | 「和弦」不是中文里的组合键；中文半角标点 |
| `ShortcutRecordUnusable` | That key cannot be written down | That combination cannot be saved | 这个键写不进文件 | 这个组合键无法保存 | 「写不进文件」暴露了持久化；录的是组合不是单键 |

---

## 8. 顺手清掉的一条（不在设置页，同一个禁语）

| 条目 | 旧英 | 新英 | 旧中 | 新中 | 理由 |
|---|---|---|---|---|---|
| `PreviewFailedSeatTooSmall` | Preview failed: preview seat is too small | Preview failed: the preview pane is too small | 预览失败：预览窗格太小 | 预览失败：预览窗格太小 | **禁语 `seat`**——中文列早就写的是「窗格」，英文列没跟上 |
| `unknown_profile_banner_text` | Profile {id} is not in this build; using {other} instead. | There is no profile {id}; using {other} instead. | 这个版本里没有配置文件 {id}，改用 {other}。 | 没有名为 {id} 的档案，改用 {other}。 | **禁语 `this build`**；术语统一 |

---

## 9. 顺带修的两处测试

复制文案的测试在改文案的那天变红，而不是在某一行不再画出自己的句子的那天变红——这不是钉子该有的样子。

- `settings.rs` 新增 `drawn_text(&labels)`：把一页画出来的每个 label 按绘制顺序接成一个串。行的说明**不是一个 label**，它换几行取决于句子今天有多长，所以「这一行画了它的句子吗」这个问题只能这样问。`the_display_formulas_row_*`、`the_tables_row_*`、`the_maximum_height_row_*`、`the_scrollback_row_*`、`the_new_rows_wear_the_mock_ups_own_words` 五条改成读 `row.description(&values())` 再查包含，不再抄一份文案。
- `the_startup_row_*` 与 `the_line_wrapping_rows_*` 仍逐字钉死（那是文案的钉子，改文案本来就该来这里改一次），注释改成指向本片的裁决与规范页。

---

## 10. 实机截图查出的三个从没进过表的控件（`Text::ALL` 505 → 508）

中文那一版的截图上，快捷键页每一行的按钮写着 **`Record`**。这三个是 `settings.rs` 里的 `&'static str` 常量，一次翻译扫荡也没扫到过——`ShortcutRecordPrompt` 那一条的注释甚至写着「它们是这一页最后剩下的英文字面量」，而它说的时候，这三个正躺在它上面十行的地方。**一个 `&'static str` 常量没法自己报告这件事，只有把页面用两种语言各拍一张才看得见。**

| 条目 | 原先 | 新英 | 新中 | 说明 |
|---|---|---|---|---|
| `ShortcutRecord`（原 `RECORD_BUTTON_LABEL`） | 硬编码 `"Record"` | Record | 录制 | 中文窗口里的英文按钮 |
| `ShortcutRecordListening`（原 `RECORD_LISTENING_LABEL`） | 硬编码 `"Press…"` | Press… | 按键… | 同上；「按键…」而不是「录制中…」——省略号已经说了时钟在走，读者要做的是按下去 |
| `ShortcutRestoreAll`（原 `RESTORE_ALL_LABEL`） | 硬编码 `"Restore all defaults"` | Restore all defaults | 全部恢复默认 | 与 `ProfilesRestoreAll` 一模一样的英文却是第二份字面量：一份翻了，一份没翻。两页的「默认」不是同一种东西（键位 vs 档案字段），所以是**两个条目**而不是合并成一个——合并会让翻其中一页的动词把另一页也搬走 |

三处调用点改成读表的函数；`settings.rs` 里那条比对 mock-up 的断言仍逐字写 `"Restore all defaults"`，因为 mock-up 是一份英文文档，中文运行下去表里取词会去那份文件里找一个它从来没有过的词。

---

## 11. 实机双语截图之后的第二轮（只动中文列）

设置行的换行**优先在半角空格处断**，只有当一整个「词」（两个空格之间的一串）放不下整行时才退回到逐字断。中文句子里的空格只出现在拉丁 token 两边，于是：

| 条目 | 屏幕上的毛病 | 新中 |
|---|---|---|
| `DescTurnEndNotifications` | 「Agent」单独占了第一行，整句被压到它下面 | 一个回合结束时，闪动这扇窗的任务栏按钮；窗口最小化时改为在桌面上放一条消息。窗内的记号不受影响。 |
| `DescLightScheme` | 超出一个字，句号「。」单独落在第二行 | 浅色窗口用的配色，终端与窗口本身共用一套。 |
| `DescFormulas` | 三个开关各自「短半行 + 长一行」，一列三行全是参差的 | 排版命令输出里的 $$…$$ 块。关闭时显示 LaTeX 源码原文。 |
| `DescInlineFormulas` | 同上 | 排版命令输出里的 $…$。关闭时显示源码原文。 |
| `DescTables` | 同上 | 画出命令输出里的 markdown 表格。关闭时显示管道符原文。 |
| `DescCodexNotify` | codex 出现两次，「正在等你回答。」单独落在第三行 | 在你的 ~/.codex/config.toml 里加上 notify 程序，让 codex 在一个回合结束时告诉这扇窗。它不报告它在等你回答。 |

改法都是**把拉丁 token 从句首挪开、或把句子收进整行**——三个渲染块开关现在各是一行，Agents 四行各是齐整的两行。英文列一个字未动。

**中文行首的主语。** 「Agent 结束一个回合时」在中文里被丢掉了主语而英文保留 `an agent`，这是规范里「按中文习惯重写、不逐字翻译」那一条：页名是 AGENTS、行名是「回合结束」、上面三行分别写着 Claude Code / Codex / Copilot CLI，中文再说一次主语是啰嗦的，英文不说则是不成句的。
