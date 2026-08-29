# 设置页文案规范（copy guide · v1，2026-08-26 立）

用户裁决（2026-08-26，验 next11）：「设置里的文字到时候都要重写，你现在写的叫做注释，而不是面向用户的。」

此前仍然有效的那条：**UI 文案只陈述事实，不夹议**（2026-08-17）——没有「我们」、没有「不是我们该替你留的」式旁白。

本页是**写给读者的那一行字**的规范。写给维护者的那一段——为什么这样设计、翻过哪些案、量过哪些像素——照旧留在 `DESIGN.md` 和源码的文档注释里。两者不换位置：源码的注释可以长，屏幕上的字必须短。

参照物：Windows 11 设置、macOS 系统设置、VS Code Settings。三家的共同点是**一行标题说「它是什么」，一行说明说「打开会发生什么」**，而不是解释它怎么实现。

---

## 1. 行名（title）

- **名词或动宾短语。** `Scrollback`、`Line wrapping`、`Offer PowerShell integration`。
- **长度：英文 ≤3 词，中文 ≤6 字。** 唯一的放宽是行名里含专有名（`PowerShell`、`Claude Code`、`Explorer`、`PSReadLine`），那部分不计入，但整行仍要读得完。
- **首字母大写，其余小写**（sentence case），**不加句号**。
- **说「它是什么」，不说「它怎么实现」。** `Cards` 不是 `Focus mode`；`Tables` 不是 `GFM pipe table rendering`。
- **一个对象只有一个名字。** 同一件东西在行名、说明、菜单、快捷键表里必须是同一个词。中文尤其：`profile` 全表写「配置文件」，行内指代写「这个配置」「默认配置」，不许再出现「档案」；`integration` 全表写「整合」，不许一处「集成」一处「整合」（**用户裁 2026-08-29**，推翻本页原先定的「档案」「集成」——「配置文件」是中文版 Windows Terminal 对同一个对象的叫法，读者从那边过来读到的是同一个词）。

## 2. 说明（description）

- **一到两句。**
- **第一句说「打开时发生什么」**，用第二人称直述，主语是这个产品或这台机器在做的事：
  - `Typesets $$…$$ blocks in command output.`
  - `Adds hooks to your ~/.claude/settings.json, so Claude Code tells this window when it is waiting for you.`
- **第二句（如需）说三件事之一**：
  1. **关掉 / 另一档时发生什么** —— `Off, the LaTeX source is shown as it was printed.`
  2. **它影响谁、不影响谁** —— `Menus, tabs and this dialog keep their own.`
  3. **读者非知道不可的一个前提** —— `Visible only when background opacity is below 100%.`
- **不讲内部机制。** 不写渲染路径、不写缓存、不写「一帧」「一次 refresh」。
- **不引代码名。** 不写类型名、函数名、字段名。**用户自己文件里的键名不算代码名**（`FORCE_HYPERLINK`、`$PROFILE`）——那是读者要去找的东西。
- **不讲历史。** 「2026-08-19 用户裁…」「本片把它改成…」「原来是 X，现在是 Y」这类句子一律不许出现在屏幕上。
- **文件路径只在读者真的要去找那个文件时才写。** 三个 hooks 安装行写 `~/.claude/settings.json`、`~/.codex/config.toml`、`~/.copilot/hooks/`，因为那是读者自己的文件、读者会去看；`Dark scheme` 写 `%APPDATA%\Folio\schemes`，因为读者要往那儿放配色文件。除此以外不写路径。
- **不解释为什么这样设计。** 那是 `DESIGN.md` 的事。

## 3. 禁语表

**英文**（整词匹配；带空格的按短语匹配）：

`chrome` · `seat` / `seats` · `the grid` · `the face` · `the ground` · `slice` · `mock-up` / `mockup` · `ruling` · `red gate` · `this build` · `the product` · `schema` · `vendor` · `pipeline` · `buffer` · `widget` · `§` · `TODO` · `FIXME` · `P1` / `P2`

**中文**：

「裁决」·「红线」·「本产品」·「本软件」·「座位」·「格子」·「字面」·「小样」·「钉子」·「切片」·「内核」·「管线」·「缓冲区」·「控件」· `chrome` · `§` · `schema` · `vendor` · `slice`

替换表（下面这一列是屏幕上该写的词）：

| 内部词 | 屏幕上写 |
|---|---|
| chrome | the window's own bars / 窗口自身 |
| seat | pane / 窗格 |
| the grid | terminal text / 终端文字 |
| the face | the font / 字体 |
| the ground | the background / 背景 |
| 格子、字面 | 单元格、默认字号 |

**不在禁语表里的**（它们是读者的词，不是内部词）：`Search engine`／搜索引擎、`WebView2 Runtime`（微软的产品名）、`hook`／`hooks`（上游产品自己的叫法）、`profile`／配置文件、`pane`／窗格、`tab`／标签、`agent`／agent（**不许译「智能体」**，用户裁 2026-08-29）。

红门：`i18n.rs` 的 `no_settings_sentence_uses_the_words_this_window_only_says_to_itself`，逐条走 `Text::ALL` 的两列。

## 3b. 只说读者看得见的东西（用户裁 2026-08-29）

上面那张表管的是**措辞**。这一条管的是**一句话可以指什么**，比它更严：一个名字可以是完全平常的中文，却指着一个只有这个仓库里才存在的东西。

判例是「按通知规则提醒」——每个字都是平常中文，而它指的「规则」是注意力的三档触达梯子，读者从来没被展示过。同类还有「第一档」「第三档」`episode``image worker`。

**一句话允许指的东西**：屏幕上看得见的（标签、提醒标记、任务栏按钮、系统通知）、设置里那一行**印出来的名字**、读者自己文件里的路径或键名、产品名。一行要说另一行干什么，就**点那一行的名字**，或者干脆把那个行为直接写出来。

红门：`i18n.rs` 的 `no_interface_string_speaks_the_repositorys_private_words`，中英两列同扫。单字内部词（「档」「座」「片」）只在**独立成词**时算数——「卡片」「片刻」「档位」里的那一个字不是。

## 4. 中文版

- **按中文习惯重写，不逐字翻译。** 英文的分号从句在中文里通常该断成两句。
- **标点全角。** 「，」「。」「；」「：」「？」「！」。汉字后面直接跟半角 `,` `;` `:` `!` `?` 是 bug——红门 `chinese_sentences_are_punctuated_the_chinese_way` 逐条查。
  - 例外只有一种：半角符号出现在一个英文/路径 token 内部（`https://`、`folio.ps1`），那时它前面不是汉字，红门本来就不看它。
- **专有名保留原文**：`Claude Code`、`Codex`、`Copilot CLI`、`PowerShell`、`WSL`、`PSReadLine`、`Folio`、`Git`、`markdown`、`LaTeX`。
- **量词和数字用半角数字加空格**：「保留 5000 行」「低于 100% 时」。
- **不许把英文列直接抄成中文列**（红门 `no_entry_ships_the_english_word_as_its_own_translation` 已在守）。
- **不要用短拉丁词开头。** 设置行的换行优先在半角空格处断，只有当一整个空格分隔的「词」放不下整行时才退回逐字断。中文句子里的空格只出现在拉丁 token 两边，所以以 `Agent`、`Git`、`codex` 这类短词开头的中文句子会把那个词单独留在第一行，整句压到它下面。把拉丁 token 挪到句子中间，或者干脆不写它——中文常常可以省掉主语，英文不能。
- **句号不许单独落在一行。** 那说明这句中文超出了整行一两个字，收短它。

## 5. 长度上限

- **说明最多三行。** 设置行的说明带三行换行；第三行被截成 `…` 就是**文案 bug**，不是几何 bug。
- 红门：`settings.rs` 的 `no_settings_sentence_needs_a_fourth_line`（英文列）与 `no_chinese_settings_sentence_needs_a_fourth_line_either`（中文列），按设计的 118px 控件列量，报出**每一条**超长的行名。
- 实务上：英文说明 ≤ 约 140 字符、中文 ≤ 约 60 字，基本不会碰到上限。

- **一句更短的中文不等于一段更短的排版**（2026-08-29 量到）。换行**在空格处断**，只有单个超宽的词才逐字拆；而中文句子的空格只在拉丁词两边，所以**两个拉丁词之间的一整串汉字是一个词**。「打开后，Folio 在 Claude Code」占了 368px 一行里的 168px，下一个词是 306px 的一整串汉字加路径——放不下，那一行剩下的 200px 就空着。这就是 §4「不要用短拉丁词开头」那条的真实机制。**写完用中文列的门量一遍，不要用字数估。**

## 6. 一致性：同族行同句式

同一族的行必须是同一个模板填不同的空，读者扫一眼就知道它们是一类东西。

- **三个安装行**（Claude Code hooks / Codex notify / Copilot CLI hooks）：
  `Adds <什么> to your <路径>, so <程序> tells this window when <事件>. <一句边界>.`
  中文：`在你的 <路径> 里加上 <什么>，让 <程序> 在 <事件> 时告诉这扇窗。<一句边界>。`
- **三个 Rendered blocks 开关**（Display formulas / Inline formulas / Tables）：
  `<动词> <什么> in command output. Off, <原文> is shown as it was printed.`
  中文：`把命令输出里的 <什么> <动词>。关闭时，按打印出来的<原文>显示。`
- **两个配色行**（Light scheme / Dark scheme）：前一行说是什么，后一行说 `The same for a dark window.` 加文件夹。
- **两个不可用说明**（Acrylic / Background opacity 被整行灰掉时）：一句陈述这台机器做不到什么，不道歉、不给建议。
- **四个 PiP 唤出行**：只有末尾的数字不同。

## 7. 复核清单

写完一行，逐条对：

1. 行名是名词或动宾短语，≤3 词 / ≤6 字，不带句号？
2. 第一句说了「打开时发生什么」？
3. 第二句（如果有）说的是关掉/另一档、或影响范围、或一个前提？
4. 全句没有禁语表里的词？
5. 中文标点全角？专有名保留？
6. 三行放得下？
7. 同族的兄弟行是同一句式？
8. 说的行为**与代码一致**？（不确定就去读代码或写测试确认，不许猜）
