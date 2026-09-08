> Draft for the GitHub Release body. The published text is settled by the user at release time. 草稿，发布时由用户审定。

# Folio 0.2.3-preview

**Download:** [zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.2.3-preview/folio-0.2.3-windows-x64.zip) (Windows 10 1809+ / 11, 64-bit). Unpack, run folio.exe.

**下载:**上方 zip 即为完整下载,其余为校验和、物料清单与源码。

0.2.3 is about reading what a pane is showing, and about the panes that were
left out. Command Prompt now has a command rail and reports the folder it is
standing in, and the first WSL tab of a window is integrated like every one
after it. A tab's card keeps up with its pane whatever shell is inside it, and
draws everything that pane is showing. Markdown gets a wider column, a wide
table that scrolls sideways under an ordinary wheel, and links that are drawn as
links; a terminal scroll bar is there only when there is somewhere to scroll to,
and a tick on the command strip lands on its own command after a split. Settings
loses a row that asked the same question twice, a change to the Explorer menu
now reaches a File Explorer that is already running, and every string in the
window has been read by a native writer in both languages.

## Command Prompt has a command rail, and knows which folder it is in

- A `cmd.exe` pane used to have an empty rail however many commands you had run:
  nothing to click, and `Ctrl+Shift+↑`/`↓` with nowhere to go. Every prompt now
  gets a tick that lands on its own prompt row, so the strip walks a `cmd`
  session the way it walks a `bash` one.
- The pane reports the folder it is standing in, so the tab and the files column
  follow a `cd`, and a path it prints is a link.
- Whatever `PROMPT` you had set is kept. Folio reports in front of it, never in
  place of it, so a prompt you wrote yourself looks exactly as it did, and a
  `cmd` started from a `cmd` pane does not report twice.
- **Two things `cmd` cannot say, and Folio does not pretend otherwise.** A tick
  carries no exit code, because `PROMPT` has no way to read one. And nothing in
  `cmd` marks where a typed line ends, so **an inline `$…$` in a Command Prompt
  pane is still shown as text rather than typeset**. Display formulas and image
  previews in its output are exactly where they were.
- The Profiles page says so on the Command Prompt row, in both languages:
  "Prompt marks, directory and hyperlinks; no exit codes".

## The first WSL tab of a window is integrated like every one after it

- A WSL pane gets its ticks, its working directory and its clickable paths from
  a small script handed to the shell your distribution logs you into. The first
  WSL pane of every run used to go out before Folio knew which shell that was,
  and so was started without the script: no ticks on the rail, no folder in the
  tab or the files column, no `Ctrl+Shift+↑`/`↓`, and a card that never moved
  while a command ran. The second WSL tab you opened worked, which is no
  consolation on a machine whose default profile is WSL, where the first pane is
  the only one there is.
- Every WSL pane is composed the same way now, the first one included, because
  the pane asks its own distribution which shell it logs into instead of waiting
  on an answer that arrived too late. A distribution that logs you into zsh or
  fish keeps its shell, untouched, exactly as before.

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

## A tab's card keeps up with its pane, whatever shell is inside it

- The cards in the tab column were refreshed by a clock that only an integrated
  shell starts. A PowerShell or Git Bash card followed every row, while a
  Command Prompt card caught up at the next prompt and a WSL card caught up
  whenever something else happened to repaint the window: the last rows of a
  burst could sit unreproduced on a card until you moved the pointer over it.
- A card now refreshes because the pane it is a picture of changed, which is the
  same thing for every shell. It costs no more than it did: a card still redraws
  at most ten times a second, and a card you cannot see, in a collapsed column
  or a tab scrolled out of the list, costs no frame at all.

## A tab's card fills with the pane's own history

- A pane that had scrolled and was then made taller goes on showing lines from
  its own history above the ones still on the screen. Its card was reading the
  live screen alone, so it drew nine rows at the top and left two thirds of
  itself empty while the pane below it showed twenty-four.
- A card now reads the same places the pane reads, so it fills from the top and
  is a picture of what the pane is actually showing. It was reported against Git
  Bash sitting beside PowerShell 7, and it was never about either shell: any
  pane is in that position after the window is made taller.

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

## A new Explorer menu entry shows up without a sign-in

- File Explorer reads the right-click tables once and goes on drawing what it
  read, and Folio never told it anything had changed. So turning the Explorer
  row on could be registered correctly, be verifiably there on the machine, and
  still be invisible until the next sign-in. Folio now announces every change it
  makes to that menu, on a refusal as well as on a success, and a File Explorer
  that is already running picks up "Open Folio here" straight away.
- The page Windows 11 opens first is the one Windows does not promise to refresh
  on that announcement. So where Folio registered the package during this run,
  the Settings row and the message that follows the registration both say so,
  and both give the one step that always works: sign out and back in. Folio does
  not restart your File Explorer.

## The picture-in-picture rows are off the Shortcuts page until that window exists

- Those four rows offered a key for a window Folio cannot summon yet: a chord
  could be recorded into one, pressing it did nothing, and the only place that
  was said was a line under the row that appeared once the chord was already
  there. A shortcut you can set and cannot use is not a setting, so the rows are
  off the page and out of `docs/shortcuts.md`.
- Nobody loses a chord they had already recorded. `keybindings.json` still names
  all four slots, a chord written into one stays in the file exactly as it was,
  and `Restore all defaults` still clears it. The rows come back, with their
  names and their Record buttons, the day the window does.

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
- **A tab is named after its shell rather than the folder it is standing in**, in
  PowerShell, Git Bash and WSL panes. The pane head and the files column both
  name the folder.
- **A Unix-style path is not a link.** `D:\Demo\figure.png` printed in a Git Bash
  or WSL pane is clickable; the same file written `/d/Demo/figure.png` or
  `/mnt/d/Demo/figure.png` is not.

The full list is in `CHANGELOG.md` in the repository.

---

<!-- zh: opus46 -->

# Folio 0.2.3-preview

0.2.3 围绕读懂窗格正在显示的内容，以及此前被遗漏的窗格。Command Prompt 有了命令条，并报告所在目录；窗口的第一个 WSL 标签页与之后每一个一样完整。标签页卡片跟上窗格的变化，无论其中运行哪种 shell，绘制窗格正在显示的一切。Markdown 排版列更宽，宽表格用普通滚轮即可横向滚动，带格式的链接文字正常显示为链接；终端滚动条只在可滚动时才出现，拆分后命令条上的标记仍落在对应命令上。设置中两行资源管理器选项合为一行开关，右键菜单的变更到达已在运行的资源管理器，中英文界面经过母语写作者逐条审读。

## Command Prompt 有了命令条，并报告所在目录

- `cmd.exe` 窗格此前无论运行了多少条命令，命令条都是空的：没有可点击的标记，`Ctrl+Shift+↑`/`↓` 无处可去。现在每个输入行都获得标记，命令条对 `cmd` 会话的导航方式与 `bash` 相同。
- 窗格报告当前所在目录，标签页和文件列跟随 `cd` 更新，输出中的路径是可点击的链接。
- 已有的 `PROMPT` 设置不受影响。Folio 在它前面报告，不替换它，自定义的输入行外观不变，从 `cmd` 窗格启动的 `cmd` 不会重复报告。
- **`cmd` 无法提供的两项信息，Folio 不假装提供。** 标记不带退出码，因为 `PROMPT` 无法读取退出码。`cmd` 也不标记输入行的结束位置，因此 **Command Prompt 窗格中的行内 `$…$` 仍显示为文本而非排版公式**。输出中的独立公式和图片预览照常显示。
- 配置页在 Command Prompt 行上以中英文标明这一点："Prompt marks, directory and hyperlinks; no exit codes"。

## 窗口的第一个 WSL 标签页与之后每一个一样完整

- WSL 窗格的标记、工作目录和可点击路径来自交给登录 shell 的一段脚本。此前每次运行的第一个 WSL 窗格在 Folio 得知登录 shell 之前就已启动，因此没有脚本：命令条上没有标记，标签页和文件列中没有目录，`Ctrl+Shift+↑`/`↓` 无处可去，卡片在命令运行时不刷新。第二个 WSL 标签页正常工作——但默认配置是 WSL 的机器上，第一个窗格就是唯一的窗格。
- 现在每个 WSL 窗格都以同样方式整合，包括第一个，因为窗格向自己的发行版询问登录 shell，不再等待来得太迟的答案。登录 shell 是 zsh 或 fish 的发行版照旧保持原样。

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

## 标签页卡片跟上窗格的变化，无论其中运行哪种 shell

- 卡片列中的卡片此前在不同 shell 中的刷新频率不同。PowerShell 和 Git Bash 的卡片跟上每一行输出，Command Prompt 的卡片到下一次输入行才更新，WSL 的卡片在窗口碰巧重绘时才更新：一段输出的末尾几行可能一直停在卡片上，直到指针移过去才被复现。
- 卡片现在因为窗格内容变化而刷新，对每种 shell 都一样。开销不变：卡片每秒最多重绘十次，看不到的卡片——标签列折叠或标签页滚出列表时——不消耗任何帧。

## 标签页卡片绘制窗格的完整内容

- 窗格滚动后被拉高时，屏幕上方会显示历史输出行。卡片此前只读取当前屏幕，因此顶部画了九行后其余部分留空，而窗格显示二十四行。
- 卡片现在读取与窗格相同的内容，从顶部填充，是窗格实际显示内容的映射。该问题在 Git Bash 与 PowerShell 7 并排时被报告，但与 shell 无关：窗口拉高后任何窗格都处于同一状态。

## 设置中两行资源管理器选项合为一行开关

- `Explorer context menu` 和 `First page of that menu` 实为同一个问题的两行，第二行离开第一行没有意义。
- 现在只有一行，和页面上其他开关一样只分开和关。**开启的含义是这台 Windows 所能做到的全部**：在 Windows 11 上且 `folio.msix` 与 `folio.exe` 同目录时，将 "Open Folio here" 放入**显示更多选项**并注册将 "Open in Folio" 放到**第一页**的包；其他情况下只写入右键菜单项。关闭时撤回已有的条目。
- 因为开启在不同机器上含义不同，行下方的说明文字会告知具体行为——在 Windows 10 上不提第一页，因为它只有一层菜单。存储位置不变：从 **设置 > 应用 > 已安装的应用** 中移除该包，开关随之变化。

## 新的资源管理器菜单项无需重新登录即可生效

- 资源管理器启动时读取一次右键菜单注册表，此后使用缓存。Folio 此前从未通知系统有变更，因此开启资源管理器选项后，即使注册成功且可验证存在，仍可能在下一次登录前不可见。Folio 现在在每次变更后发送通知，无论成功或失败，已在运行的资源管理器立刻显示 "Open Folio here"。
- Windows 11 第一页由打包系统管理，Windows 不保证对该通知作出刷新。因此当 Folio 在本次运行中注册了包，设置中的行和注册后的提示都会说明这一点，并给出一定有效的步骤：注销再登录。Folio 不会重启资源管理器。

## 画中画行从快捷键页暂时移除，待该窗口实现后恢复

- 四行画中画快捷键此前为一个 Folio 还无法呼出的窗口提供按键录制：可以录入快捷键，按下后无事发生，唯一的说明文字在录入完成后才出现。能设置却无法使用的快捷键不是设置，因此这些行从页面和 `docs/shortcuts.md` 中移除。
- 已录入的快捷键不会丢失。`keybindings.json` 仍列出四个槽位，写入其中的快捷键原样保留，**Restore all defaults** 仍可清除。这些行会在该窗口实现之日带着名称和录制按钮回来。

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
- **标签页以 shell 而非所在目录命名**，在 PowerShell、Git Bash 和 WSL 窗格中。窗格头部和文件列都显示目录名。
- **Unix 风格路径不是链接。** Git Bash 或 WSL 窗格中打印的 `D:\Demo\figure.png` 可点击；同一文件写作 `/d/Demo/figure.png` 或 `/mnt/d/Demo/figure.png` 则不可点击。

完整列表见仓库中的 `CHANGELOG.md`。
