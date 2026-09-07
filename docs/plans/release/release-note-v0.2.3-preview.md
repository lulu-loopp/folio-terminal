> Draft for the GitHub Release body. The published text is settled by the user at release time. 草稿，发布时由用户审定。

# Folio 0.2.3-preview

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.2.3-preview/folio-0.2.3-windows-x64.zip) (Windows 10 1809+ / 11, 64-bit). Unpack, run folio.exe.

**下载:**上方 zip 即为完整下载,其余为校验和、物料清单与源码。

0.2.3 is about reading what a pane is showing: a wider Markdown column, a wide
table that scrolls sideways under an ordinary wheel, links that are drawn as
links, and a terminal scroll bar that is there only when there is something to
scroll to. Settings loses a row that asked the same question twice and gains
three that can each record a chord, and every string in the window has been read
by a native writer in both languages.

## A Markdown document uses the width you gave the window

- The reading column was capped at 702 logical pixels, which on a maximised
  window left a document reading down a strip in the middle of the pane. The cap
  is now about a thousand.
- Everything else about the column is unchanged: it is still centred, a pane too
  narrow for it still gets the whole pane, and tables and code blocks are set in
  the same column as the prose.

## A wide table scrolls sideways, with the wheel most mice already have

- A table whose columns need more room than the page can give has always had a
  bar along its own foot and a `Shift`+wheel that moved it. A tilt wheel, and a
  touchpad's second finger, were reported to the window all along and dropped
  before they reached the page.
- They are not dropped any more and they need no modifier. The table under the
  pointer is the one that moves, so a page with several wide tables has several
  scrolling regions, exactly as a browser does. `Shift`+wheel still does what it
  did, everywhere it did it.

## A link whose text is set in code, or carries emphasis, is drawn as a link

- A line reading ``[`folio-0.2.3-windows-x64.zip`](https://…)`` used to print its
  own Markdown source, brackets and address and all, while a plain-text link
  beside it on the same line rendered. Anything at all inside a link's text — a
  code span, a formula, a picture — left the two brackets in different pieces of
  the line, and neither piece could see a pair.
- A link's text is now read as what CommonMark says it is: ordinary inline
  content. A code span in it stays monospace and takes the link colour, a bold
  word stays bold, a picture wrapped in a link draws its picture, and every one
  of them answers a click.

## A terminal pane's scroll bar says what the pane can do

- **A pane whose whole transcript fits wears no bar.** After one run of a script
  that prints display formulas, with the prompt back and empty space below it, a
  thumb was drawn down the right edge of a pane that could not be scrolled at
  all. A display formula makes its row taller than a row, and the blank rows
  under the prompt give that height back; those given-back pixels were being
  counted as somewhere the view could travel to.
- **A pane scrolled to the top shows its thumb at the top of the track**, and the
  thumb's length is the pane's share of the whole transcript.
- **The bar rides the pane's own edge.** Every other bar in the window puts its
  thumb against the inner edge of the surface it belongs to, and this one stood
  two logical pixels off its own. The bar along the foot moved with it. The
  reserved lane is the same eight pixels and the thumb is grabbed and dragged
  exactly where it was.

## A tick on the command strip lands on its own command after a split

- Pressing the newest tick used to drop the reader into the middle of that
  command's output, with the prompt line above the top of the pane, once the pane
  had been split or resized.
- A mark is now written down against the whole wrapped line it sits in — the one
  thing a re-wrap keeps — and put back on that line afterwards, whether the line
  is still on the screen or has scrolled out of it. Dragging an edge is a run of
  re-wraps rather than one, and a mark is carried through every step of the drag.
  A mark whose line the re-wrap genuinely lost leaves the strip instead of
  pointing somewhere the command never was.

## The two Explorer rows in Settings are one switch

- `Explorer context menu` and `First page of that menu` asked one question twice,
  and the second was meaningless without the first.
- There is one row now, and it is on or off like every other switch on the page.
  **On is everything this Windows can do**: on Windows 11 with `folio.msix`
  beside `folio.exe` it puts "Open Folio here" under `Show more options` and
  registers the package that puts "Open in Folio" on the page Windows 11 opens
  first; anywhere else it writes the menu entry alone. Off takes back whichever
  of the two is there.
- Because On means different amounts on different machines, the line under the
  row says what it does on yours — and on a Windows 10 that line names no page,
  because that Windows has one menu. Nothing about where the answer is stored
  changed: removing the package from `Settings > Apps > Installed apps` still
  moves the row.

## Four picture-in-picture slots, four rows, four chords

- They were one row saying `Not set` with nothing on it to press, standing
  between a row that has a Record button and two greyed rows that are keys this
  window leaves to readline — so there was no way to tell which of the two it
  was. It was neither: those chords were always yours to choose.
- `Summon picture in picture 1` to `4` are a row each now, with their own chord,
  their own Record button and their own `↺`. A chord another row already answers
  to is refused, with the offer to take it, and `Restore all defaults` empties
  all four again. `keybindings.json` is untouched: it named all four slots before
  this change and names them now.

## The interface reads the way a native writer would put it

- A review of every Chinese string a reader can see reworded 52 of them:
  sentences carrying a reassurance nobody asked for, lines that explained an
  internal mechanism instead of what the switch does, and words left in English
  where the rest of the Chinese says 标签页 and 配置.
- The English audit's 83 proposals are in, along with the passages it proposed in
  `README.md`, `docs/PRIVACY.md`, `SECURITY.md` and the 0.2.2 release page. No em-dash is left inside a
  string in the window, and a mechanism the reader cannot act on gives way to the
  result they get.
- No fact changed and no default moved in either language.

## The core compiles on macOS

- Everything below the application layer — the terminal grid, the renderer, the
  transcript, the document model, the detectors, the layout, the maths — compiles
  on a Mac, and a job on every push compiles it there so that it goes on doing
  so.
- **Nothing about Folio on Windows changes and there is no Mac build to
  download.** This is groundwork, not a port. What it buys is that the next
  feature cannot quietly assume Windows without somebody being told the same day.

## Also fixed

- **A first-run row names the file it will actually write.** The card's three
  agent rows spelled `~/.claude/settings.json`, `~/.codex/config.toml` and
  `~/.copilot/hooks/folio.json` whatever the machine was set to, while Claude
  Code reads `CLAUDE_CONFIG_DIR`, codex reads `CODEX_HOME` and Copilot CLI reads
  `COPILOT_HOME` first — and so does Folio's installer. On a machine that sets
  none of the three the spelling is the one it always was.
- **The welcome card draws no line between its rows**, does not fill a row under
  the pointer, and its focus ring waits for a key that moves something rather
  than for any key at all, and is no longer clipped at its right-hand side. The
  Settings page, whose row shape the card borrows, does all three.
- **A second copy of Folio no longer takes over your "Open Folio here" menu
  entry.** A launch rewrites that entry only when nothing is at the path it names
  or when that path is this very file, so a copy run once out of a downloads
  folder leaves the menu alone. Moving `folio.exe` still works exactly as before.
- **An agent installer that refuses says why in one sentence**, after a colon
  rather than a dash, and a reason long enough to need a second line gets one
  instead of being cut.
- **The README, the changelog and the design note no longer say the welcome card
  asks six questions.** It offers as few as two: an agent that is not on the
  machine is not listed at all.

## Upgrading from 0.2.2

**Nothing to do.** Your existing settings are preserved when you upgrade. Unpack
over the old folder, or beside it, and run `folio.exe`. The archive holds the
same **nine** files 0.2.2's did, and `folio.msix` still belongs in the same
folder as `folio.exe`.

## Download and run

Take `folio-0.2.3-windows-x64.zip` from this release, unpack it wherever you keep
programs, and run `folio.exe`. There is no installer; keep the extracted files
together in one folder. `SHA256SUMS.txt` is the hash of what you downloaded, and
`folio-0.2.3.cdx.json` is the bill of materials for what is in the build.

Needs **Windows 10 1809 or newer, or Windows 11, 64-bit**.

`folio.exe` and `folio.msix` are signed by **Weiyi Shi**, with a certificate from
Microsoft's Artifact Signing service and a Microsoft time stamp, as they were in
0.2.2.

**`folio.msix` is no longer an asset of its own.** It is in the zip, where it has
always also been, and that is the only place it works from: the package names the
folder it was extracted into, so a copy downloaded on its own points at a folder
with no `folio.exe` in it. Nothing about the Explorer menu changes for anybody
who unpacks the archive.

## Known issues

- **A new signature has no reputation yet.** SmartScreen can still raise
  **"Windows protected your PC"** on the first run. **More info** names
  **Weiyi Shi** as the publisher and `folio.exe` as the application; **Run
  anyway** is the way through, and switching SmartScreen off is not.
- **Folio cannot be a panel inside Visual Studio Code.** `folio-here.cmd` in the
  archive makes it the external terminal VS Code opens instead.
- **A window saved on a monitor that enumerates late** comes back on the primary
  display. The displays are counted once, before the window is made.
- **Two web previews can overlap for about a fifth of a second while panes are
  moving to new places.** Panes standing still never overlap, and a divider drag
  does not do it.
- **A window was once reported drawing its top half black** after a move to a
  second monitor, unreproduced.
- **`.webm` needs the VP9 or AV1 Video Extension** from the Microsoft Store. A
  stock Windows has neither, and without one there is no still and no playback.

The full list is in `CHANGELOG.md` in the repository.

---

<!-- zh: opus46 -->

# Folio 0.2.3-preview

0.2.3 围绕阅读体验：Markdown 排版列更宽，宽表格用滚轮即可横向滚动，带代码或强调的链接文字正常显示为链接，终端窗格只在可滚动时才显示滚动条。设置中两行资源管理器选项合为一行开关，四个画中画槽位各有自己的行和快捷键录制按钮，中英文界面经过逐条审读。

## Markdown 文档使用窗口给出的宽度

- 排版列此前上限为 702 逻辑像素，最大化窗口时文档只占窗格中间一窄条。上限现在约为一千像素。
- 排版列仍然居中，窗格比它窄时占满整个窗格，表格和代码块与正文在同一列内排版。

## 宽表格用滚轮即可横向滚动

- 宽度超出页面的表格一直有底部拖动条，也支持 `Shift`+滚轮。但倾斜滚轮和触控板双指横滑虽然一直被窗口接收，却在到达页面之前被丢弃了。
- 现在两者都能到达，无需修饰键。指针下方的表格就是被滚动的表格，一页中有多个宽表格时各自独立滚动，和浏览器的行为一致。`Shift`+滚轮在原来生效的地方照旧生效。

## 带代码或强调的链接文字正常显示为链接

- 此前 ``[`folio-0.2.3-windows-x64.zip`](https://…)`` 会打印 Markdown 源码，而同行的纯文本链接能正常渲染。链接文字内含代码段、公式或图片时，两端方括号落在不同的片段里，两边都无法配对。
- 链接文字现在按 CommonMark 规范解读为普通行内内容。代码段保持等宽并取链接颜色，粗体文字保持粗体，图片仍绘制图片，每一种都响应点击。

## 终端窗格的滚动条反映窗格状态

- **整个输出记录能放进窗格时不显示滚动条。** 此前在运行一段打印公式的脚本后，即使窗格不可滚动，右侧仍绘制了滑块。公式使行高超过标准行高，输入行下方的空行将多出的高度还了回去，而这些还回的像素仍被计入可滚动范围。
- **滚动到顶部时滑块在轨道顶部**，滑块长度为窗格占整个输出记录的比例。
- **滚动条贴合窗格自身的边缘。** 窗口中其他滚动条的滑块贴着所属区域的内边缘，终端的滑块此前偏了两个逻辑像素。底部的横条随之移动。预留的八像素宽道和拖动手感与此前相同。

## 拆分或调整大小后，命令条上的标记仍落在对应命令上

- 按下命令条最新的标记，此前会落到该命令输出的中间，输入行在窗格上沿之外，在窗格经过拆分或调整大小后才出现。
- 标记现在记录它所在的整个折行——折行是重排时唯一保留的单位——重排后重新放到该行上，无论该行还在屏幕内还是已滚出。拖动边缘是一连串重排，每一步都带着标记。某条折行在重排中确实消失时，其标记从命令条中移除，而非指向命令从未所在的位置。

## 设置中两行资源管理器选项合为一行开关

- `Explorer context menu` 和 `First page of that menu` 实为同一个问题的两行，第二行离开第一行没有意义。
- 现在只有一行，和页面上其他开关一样只分开和关。**开启的含义是这台 Windows 所能做到的全部**：在 Windows 11 上且 `folio.msix` 与 `folio.exe` 同目录时，将 "Open Folio here" 放入**显示更多选项**并注册将 "Open in Folio" 放到**第一页**的包；其他情况下只写入右键菜单项。关闭时撤回已有的条目。
- 因为开启在不同机器上含义不同，行下方的说明文字会告知具体行为——在 Windows 10 上不提第一页，因为它只有一层菜单。存储位置不变：从 **设置 > 应用 > 已安装的应用** 中移除该包，开关随之变化。

## 四个画中画槽位各有一行，各有快捷键

- 此前四个槽位共用一行，显示 `Not set`，没有可按的按钮，夹在带录制按钮的行和两行灰色 readline 保留键之间——无从判断它属于哪一种。它哪种都不是：这些快捷键一直可以自定义。
- `Summon picture in picture 1` 到 `4` 现在各占一行，各有快捷键、录制按钮和 `↺`。录制一个已被其他行占用的快捷键会被拒绝，并提供接管选项；`Restore all defaults` 清空全部四个。`keybindings.json` 不受影响：此变更前后它都列出四个槽位。

## 界面文字经过母语写作者审读

- 对窗口中所有中文字符串的审读修改了其中 52 条：删除了无人询问的安慰句，将解释内部机制的文字改为描述开关的实际作用，将残留的英文 `tab`、`profile` 等替换为标签页、配置。
- 英文审读的 83 条建议已落地，同时涵盖 `README.md`、`docs/PRIVACY.md` 和 `SECURITY.md` 中审读建议的段落。窗口字符串中不再有破折号，读者无法操作的机制描述让位于可见的结果。
- 两种语言均未更改任何事实，未移动任何默认值。

## 核心层在 macOS 上通过编译

- 应用层以下的所有模块——终端网格、渲染器、输出记录、文档模型、检测器、布局、数学库——在 Mac 上通过编译，每次推送的 CI 任务在 Mac 上编译它们以保持这一状态。
- **Windows 上的 Folio 没有任何变化，没有 Mac 版本可供下载。** 这是基础工作，不是移植。它的作用是让下一个功能无法悄悄依赖 Windows 而不被当天发现。

## 其他修复

- **初次设置卡中每行标注实际写入的文件。** 三个 agent 行此前无论机器如何配置，都显示 `~/.claude/settings.json`、`~/.codex/config.toml` 和 `~/.copilot/hooks/folio.json`，而 Claude Code 读取 `CLAUDE_CONFIG_DIR`，Codex 读取 `CODEX_HOME`，Copilot CLI 读取 `COPILOT_HOME`——Folio 的安装器也是。三个环境变量都未设置的机器上显示的路径与此前相同。
- **初次设置卡的行之间不再绘制分隔线**，指针悬停时不再填充行底色，焦点环等到有按键移动内容时才出现而非任意按键即出现，焦点环不再在右侧被裁切。设置页借用同一行样式，三者表现一致。
- **第二份 Folio 不再接管你的 "Open Folio here" 右键菜单项。** 启动时只在原路径不存在文件、或原路径就是当前文件时才重写该条目，因此从下载文件夹随手运行一次的副本不会动已有的菜单。移动 `folio.exe` 后菜单仍照常工作。
- **agent 安装失败时用一句话说明原因**，原因跟在冒号后面而非破折号后面，需要换行时能够换行而不被截断。
- **README、更新日志和设计文档不再说初次设置卡提了六个问题。** 实际最少可以只有两个：本机没有的 agent 不会被列出。

## 从 0.2.2 升级

**无需额外操作。** 升级时保留原有设置。解压覆盖到旧文件夹或旁边，运行 `folio.exe`。压缩包内与 0.2.2 一样是**九个**文件，`folio.msix` 仍与 `folio.exe` 放在同一文件夹。

## 下载与运行

从此发布页获取 `folio-0.2.3-windows-x64.zip`，解压到存放程序的任意位置，运行 `folio.exe`。无需安装程序，解压后保持所有文件在同一文件夹内。`SHA256SUMS.txt` 是所下载文件的校验和，`folio-0.2.3.cdx.json` 是构建内容的物料清单。

需要 **Windows 10 1809 或更高版本，或 Windows 11，64 位**。

`folio.exe` 与 `folio.msix` 由 **Weiyi Shi** 签名，证书来自 Microsoft 的 Artifact Signing 服务并带有 Microsoft 时间戳，与 0.2.2 相同。

**`folio.msix` 不再作为单独的发布资产。** 它在压缩包内——一直都在——压缩包是它唯一有效的位置：包内记录的是解压目录，单独下载的 `folio.msix` 指向一个没有 `folio.exe` 的文件夹。正常解压压缩包的用户不受影响。

## 已知问题

- **新签名尚无信誉。** SmartScreen 在首次运行时仍可能提示 **"Windows 已保护你的电脑"**。**更多信息**会标明发布者为 **Weiyi Shi**、应用程序为 `folio.exe`；**仍要运行**是通过的方式，关闭 SmartScreen 则不是。
- **Folio 无法作为 Visual Studio Code 内部的面板。** 压缩包中的 `folio-here.cmd` 使其成为 VS Code 打开的外部终端。
- **窗口上次所在的显示器出现得晚时，它会回到主显示器上。** 显示器只在窗口创建前计数一次。
- **版面重排时，两张网页预览可能互相遮挡约 200 毫秒。** 静止的窗格不会重叠，拖动分隔条时也不会。
- **曾有报告称窗口移到第二台显示器后上半部分绘制为黑色**，未能复现。
- **`.webm` 需要 Microsoft Store 中的 VP9 或 AV1 视频扩展**。出厂 Windows 两者皆无，缺少时既无静止画面也无播放。

完整列表见仓库中的 `CHANGELOG.md`。
