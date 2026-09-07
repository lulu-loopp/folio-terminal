> Draft for the GitHub Release body. The published text is settled by the user at release time. 草稿，发布时由用户审定。

# Folio 0.2.2-preview

Fixes and polish for 0.2.1-preview, and one card: a machine that has never run
Folio is welcomed once and asked its boundary questions together, in one
place.

## Welcome to Folio, once, on a machine that has never run it

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="https://raw.githubusercontent.com/lulu-loopp/folio-terminal/v0.2.2-preview/docs/screenshots/first-run-dark.png">
  <img src="https://raw.githubusercontent.com/lulu-loopp/folio-terminal/v0.2.2-preview/docs/screenshots/first-run-light.png" width="100%"
       alt="The card over a window that has just started: Folio's mark beside Welcome to Folio, then six rows of one line each with a switch at the right of every one. Get told when a new version of Folio is out is on; Open any folder in Folio from its right-click menu and PowerShell integration lets you jump between commands are off; below a hairline, Its tab lights up when Claude Code is waiting, when a Codex turn ends, and when Copilot CLI is waiting, all three off. At the foot, a faint line reading Every row here is also a row in Settings, then Not now and Done.">
</picture>

- The card asks every question whose answer writes something **outside**
  `%APPDATA%\Folio`, and it asks them together, because they are one decision
  about how much of this machine Folio may touch. Six rows of one line each, and
  the line is what you get: be told when a new version is out, which is the only
  row that arrives on; open any folder in Folio from its right-click menu; the
  PowerShell integration; and a tab that lights up for each of Claude Code,
  Codex and Copilot CLI this machine actually has. Nothing about theme, font,
  size, language or layout — those are one click away and cost nothing while
  they are wrong.
- **Rest the pointer on a row and it says how**, including which of your own
  files the switch writes and that the file is copied to a dated backup first.
- **Every row on the card is a row in Settings**, and the card presses those rows
  rather than doing anything of its own. Nothing on it is a last chance, and the
  switch you find in Settings an hour later is the same switch, in the same
  place, on the same shape of row.
- **Done** applies the rows that are on. **Not now** and `Esc` close the card
  with the shipped values — the update check on, the rest off — and change
  nothing. Either way it does not come back, and the shell behind it has been
  running the whole time.
- A row is only offered if it can be honoured. An agent that is not on this
  machine, or whose configuration already calls Folio, is not listed, and when
  none of the three is there the rule above them goes with them. On Windows 11
  unpacked without `folio.msix`, the Explorer row still offers the entry it can
  offer, worded for it.
- The PowerShell row records an intent rather than acting: where your `$PROFILE`
  is comes from the shell, so the line is added by the next PowerShell that
  starts, and **Settings > Terminal** says so until it does. The file as it stood
  is copied to a dated backup beside it first, as always.
- **If you were already using Folio, you never see it.** The step that brings
  your `settings.json` up to date is what records that.

## A pane can be dropped *between* two tabs

- The join between two entries in the tab list is a band eight logical pixels
  either side, and a pointer inside it makes the pane a new tab there rather than
  handing it to the tab it happens to be over. The list opens a slot and the pane
  stands in it, which is where letting go puts it — the picture is the row
  itself, and nothing is drawn across the join.
- It takes four more pixels to leave the band than to enter it, so the open slot
  and the tab highlight do not trade places under a hand that is holding still.
- The horizontal tab strip, the vertical rail and the card column all read the
  same rule.

## A menu that starts a shell lists only what this machine can start

- `Split with`, on a pane head and in the terminal menu alike, drew every profile
  it knew and greyed the ones whose program is not installed — on an ordinary
  machine five of the seven agent rows, filling half the submenu with lines a
  press could not spend. Those rows are simply not on those lists now, and
  neither are shells this machine has not got.
- All twelve profiles are still on the **Profiles** page in Settings, greyed
  there and naming the program each one looked for, which is where installing one
  and having its row come back can be read about.
- A profile you made yourself is never left off a menu, whatever its program
  resolves to.

## A pane dragged over a web preview can be dropped there

- The landing outline was drawn correctly over the page, but letting go did
  nothing: the press router handed every mouse button inside a page to the
  browser, releases included, and every gesture that spends a release — the drop,
  the divider, the video scrubber, the preview thumbs and pans, the terminal's
  own selection — is answered below that line.
- A hand that is already carrying something no longer counts as a hand hovering a
  page, so the release reaches the gesture that started it. While you are
  carrying something the page also stops lighting its own links under the pointer
  and stops replacing the drag cursor with its own.

## A window holds as many pages as it has preview panes

- Opening a second page in one tab used to navigate the first pane and leave the
  new one standing on its empty placeholder. A page now lands where every other
  preview lands — the first preview pane that is not locked, or a new one when
  there is none — so locking a page and opening another puts them side by side,
  each with its own engine, sharing the one browser profile they always shared.
- A page that cannot be reached or downloaded shows its card over its own pane
  rather than over the first page in the tab.

## A formula on the screen of a program that repaints itself gets typeset

- A full-screen redraw writes every row, so every row arrives as a change even
  when not one byte of it moved, and a display block (`$$ … $$`) is only typeset
  once the rows it sits on have been still for a moment. A program that redraws
  more often than that — Claude Code redraws at a median of 106 ms, and the wait
  is 200 ms — kept restarting the wait, so a block that landed while the program
  was busy stayed as its own source text for as long as it was there, beside
  another block that had been drawn during a lull and was a picture.
- Both are pictures now: a row rewritten with the bytes it already had counts as
  unchanged, and a row that really does change — a character or a colour — still
  restarts its wait exactly as before. Markdown tables and the pictures drawn
  under image paths wait on the same stillness, so they come back on those
  screens too.

## A picture stays in its own preview pane

- Open an image, then open a page or a text file beside it, and the picture went
  somewhere else: drawn over the other pane's contents, or — where the other pane
  held a web page — hidden underneath it, leaving nothing but the size line where
  the picture should have been. The same thing happened when a preview pane was
  inserted between the picture and the terminal.
- Every frame moved the picture to whichever preview pane came first in the
  window, which was the picture's own only while a tab had a single one of them.
  It now travels with the pane that is holding it, through splits, insertions,
  divider drags and tab switches alike.

## Also fixed

- **A page dropped on a pane opens on that pane.** Dragging an `.html` or a
  `.pdf` onto a pane of its own split the layout where you aimed and then opened
  the page somewhere else, over whatever the first preview pane was showing.
- **A formula the pane was too narrow to hold on one row is typeset too.** A
  line the terminal folded lost the space the fold landed on, so `\quad g_i(x)`
  read back as `\quadg_i(x)` and the block stayed as source at that one width.
- **A `$$` block whose opening `$$` has been pushed off the top of a full-screen
  program is drawn again.** Every block below it was reading the wrong
  delimiter, and the last one on the screen was never paired at all.
- **Two preview panes can show two pictures at once.** An image and a video
  side by side left one of the two panes with nothing but its size line,
  whichever arrived second.
- **The knob on the card's switches wears a shadow rather than a ring**, drawn
  with the same falloff as every other lift in the window.

## Upgrading from 0.2.1

**Nothing to do.** `settings.json` gains the card's two keys and is brought up to
date **automatically**, the first time 0.2.2 reads it — nothing to delete,
nothing to re-enter, and no answer you have already given is overwritten. That
same step records that the card has been shown, which is why a machine that was
already running Folio never meets it. `session.json`, `profiles.json`,
`keybindings.json` and `pins.json` are read and written exactly as 0.2.1 left
them, and no shortcut, default or file location has moved.

Unpack over the old folder, or beside it, and run `folio.exe`. The archive holds
the same **nine** files 0.2.1's did, and `folio.msix` still belongs in the same
folder as `folio.exe`.

## Download and run

Take `folio-0.2.2-windows-x64.zip` from this release, unpack it wherever you keep
programs, and run `folio.exe`. There is no installer, and nothing is written
outside that folder until you run it. `SHA256SUMS.txt` is the hash of what you
downloaded.

Needs **Windows 10 1809 or newer, or Windows 11, 64-bit**.

`folio.exe` and `folio.msix` are signed by **Weiyi Shi**, with a certificate from
Microsoft's Artifact Signing service and a Microsoft time stamp, as they were in
0.2.1.

## Known issues

- **A new signature has no reputation yet.** SmartScreen can still raise
  **"Windows protected your PC"** on the first run. **More info** names
  **Weiyi Shi** as the publisher and `folio.exe` as the application; **Run
  anyway** is the way through, and switching SmartScreen off is not.
- **Folio cannot be a panel inside Visual Studio Code.** `folio-here.cmd` in the
  archive makes it the external terminal VS Code opens instead.
- **A window saved on a monitor that enumerates late** comes back on the primary
  display. The displays are counted once, before the window is made.
- **Two pages whose panes cross while the layout is rearranging can overlap for
  about 200 ms.** Under every hosted page is a plate of the window's colour, and
  the plate of the page opened first sits above the page opened after it. Two
  panes standing still never overlap; while they glide to new places — a split, a
  close, a pane dropped, torn out or merged — the rectangles can cross, and for
  that long one page is covered by the other's plate. A divider drag does not do
  it.
- **A window was once reported drawing its top half black** after a move to a
  second monitor, unreproduced.
- **`.webm` needs the VP9 or AV1 Video Extension** from the Microsoft Store. A
  stock Windows has neither, and without one there is no still and no playback.

The full list is in `CHANGELOG.md` in the repository.

---

# Folio 0.2.2-preview

对 0.2.1-preview 的修复与打磨，另加一张卡：从未运行过 Folio 的机器，其边界问题会在一个地方一次性问完。

## 欢迎使用 Folio——在从未运行过它的机器上第一次出现

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="https://raw.githubusercontent.com/lulu-loopp/folio-terminal/v0.2.2-preview/docs/screenshots/first-run-dark.png">
  <img src="https://raw.githubusercontent.com/lulu-loopp/folio-terminal/v0.2.2-preview/docs/screenshots/first-run-light.png" width="100%"
       alt="刚启动的窗口上方的一张卡：Folio 的标记旁写着欢迎使用 Folio，下方六行，每行只有一句话，右侧有一个开关。有 Folio 新版本时提醒处于开启；在右键菜单中用 Folio 打开文件夹和 PowerShell 整合让你在已运行命令间跳转处于关闭；分隔线之下是 Claude Code 等待时、Codex 回合结束时和 Copilot CLI 等待时标签页高亮，三项均关闭。底部一行浅色文字写着所有选项都可在设置中更改，然后是暂不与完成。">
</picture>

- 这张卡一次性把所有问题放在一起问，因为它们其实是同一个决定：允许 Folio 触及这台机器的多少部分。答案写入 `%APPDATA%\Folio` **之外**的文件。六行，每行只有一句话，那句话说的就是打开它之后得到的东西：有 Folio 新版本时提醒——唯一默认开启的一行；在右键菜单中用 Folio 打开文件夹；PowerShell 整合；以及本机实际装有的 Claude Code、Codex、Copilot CLI 各一行，开着且处于等待（Codex 是一回合结束）时，其所在标签页高亮。主题、字体、字号、语言和布局一概不涉及——那些只需一次点击即可更改，暂时不对也无任何代价。
- **指针停在某一行上时，会说明其作用**，包括该开关写入用户自己的哪个文件，以及该文件先复制一份带日期的备份。
- **卡上每一行都是设置中的一行**，这张卡只是在按这些行，并不做任何自己的事。卡上没有任何「最后一次机会」，一小时后在设置中找到的开关，与卡上的是同一个开关、同一处位置、同一行样式。
- **完成**应用已开启的行。**暂不**和 `Esc` 以出厂值关闭这张卡——更新检查开启、其余关闭——并且不改动任何东西。无论哪条路，它都不再出现，而它背后的 shell 一直在运行。
- 只有做得到的行才出现。这台机器上没有的、或配置里已经调用 Folio 的 agent 不被列出；三者都不存在时，上方的规则也随之消失。在解压压缩包时没有把 `folio.msix` 一起带上的 Windows 11 机器上，资源管理器那一行仍提供它能提供的入口，并按它的措辞来写。
- PowerShell 行记录的是一个意图而非直接行动：`$PROFILE` 的位置来自 shell，所以那一行要等下一个 PowerShell 启动时才被加入，**设置 > 终端**一直注明这一点，直到它加入为止。原有文件一如既往地先复制一份带日期的备份。
- **已经用过 Folio 的用户看不到这张卡。**把 `settings.json` 更新到最新版本的那个步骤，同时也就是记录这一点的动作。

## 窗格可以拖到两个标签页*之间*

- 标签页列表中两项之间的接缝是一条两侧各八逻辑像素的带，指针在其中时，窗格成为那个位置的一个新标签页，而不是交给它恰好悬停在上面的那个标签页。列表空出一个槽位，窗格就落在那里面——松手即落在那里——图片本身就是那一行，不会有任何内容画在接缝上。
- 离开这条带需要比进入多移动四个像素，因此当手静止不动时，空出的槽位和标签页高亮不互换位置。
- 横排标签条、竖排标签栏和卡片列都遵循同一条规则。

## 启动 shell 的菜单只列出本机能够启动的项

- **拆分并运行**（窗格头部与终端菜单中均有）原先会列出它知道的所有配置，并把程序未安装的那些置灰——在一台普通机器上，七个 agent 行中有五个如此，子菜单一半都是按了也没用的行。现在这些行直接从菜单中消失，本机没有的 shell 也一样。
- 全部十二种配置仍位于设置的**配置文件页**，在那里置灰，并标明每一项所查找的程序——安装某个程序后其配置行如何恢复，可在此页了解。
- 用户自建的配置绝不会从菜单中消失，无论其程序解析到什么。

## 拖到网页预览上的窗格可以放手放下

- 落点轮廓在页面上绘制正确，但松手毫无反应：按下路由把页面内的每一个鼠标按键事件（包括释放）都交给了浏览器，而所有依赖释放的手势——放下、拖分隔条、视频拖动进度、预览的缩略图与平移、终端自身的选区——都在那一层之下无人应答。
- 已拖拽着东西的手不再被视为在悬停页面，因此释放事件能到达发起拖拽的手势。拖拽期间，页面也不会在指针下点亮自身链接，或用自己的光标替换拖拽光标。

## 一个窗口能容纳的页面数量与其预览窗格数量相同

- 过去在一个标签页中打开第二个页面会让第一个窗格转到新地址，新页面则停留在空占位符上。现在页面落到与其他预览相同的位置——第一个未锁定的预览窗格，若没有则新建一个——因此锁定一个页面再打开另一个，两者便并排显示，各有自己的引擎，共用它们一直共用的同一个浏览器配置。
- 无法访问或无法下载的页面，其提示卡片会显示在自己的窗格上，而不是标签页中的第一个页面之上。

## 在会自行重绘的程序屏幕上，公式会得到排版

- 全屏重绘会重写每一行，因此每一行都作为变更到达，即使没有一个字节发生变化；而显示块（`$$ … $$`）只有在其所在的行静止片刻后才会排版。重绘频率更高的程序——Claude Code 重绘间隔中位数为 106 毫秒，而等待时间为 200 毫秒——会不断重新开始等待，因此程序繁忙时出现的显示块会一直以其源码文本形式存在，旁边则是在间隙中绘制好、已经是图片的另一块。
- 现在两者都是图片：以原有字节重写的行被视为未变更，真正发生变更的行——一个字符或一种颜色——仍然照旧重新开始等待。Markdown 表格与图片路径下方绘制的图片也等待同样的静止条件，因此在那些屏幕上也能正常显示。

## 图片停留在自己的预览窗格中

- 打开一张图片，再在旁边打开页面或文本文件，图片就跑去了别处：绘制到其他窗格的内容之上，或者——当另一窗格是网页时——藏在网页底下，只留下尺寸行，图片本该在的位置空空如也。在图片与终端之间插入一个预览窗格时，也会发生同样的情况。
- 此前每一帧都会把图片移到窗口中顺序最靠前的预览窗格；只有当标签页只有一个预览窗格时，那才恰好是图片自己的窗格。现在图片跟随承载它的窗格移动，无论是拆分、插入、拖动分隔条还是切换标签页。

## 其他修复

- **拖到窗格上的页面在该窗格打开。**把 `.html` 或 `.pdf` 从文件列拖出来、让它单独占一个窗格时，布局确实在用户瞄准的位置分开了，但页面却在别处打开，盖住第一个预览窗格当时显示的内容。
- **窗格宽度不足以单行容纳的公式也被排版。**终端折叠某一行时把折行处那个空格也丢掉，所以 `\quad g_i(x)` 读成 `\quadg_i(x)`，在那一个宽度下该块就一直以源代码形式显示。
- **全屏程序中，开头的 `$$` 已被推出屏幕顶部的 `$$` 块重新绘制。**它下面的每个块此前都在读错误的定界符，而屏幕上最后一个块根本没有配对。
- **两个预览窗格可以同时显示两幅图片。**图片和视频并排时，两个窗格中总有一个只剩下尺寸行——后到达的那个是谁都一样。
- **卡上开关的旋钮现在带阴影而不是圆环**，阴影的衰减方式与窗口中其他所有凸起元素一致。

## 从 0.2.1 升级

**无需任何操作。** `settings.json` 会新增该卡涉及的两个键，并在 0.2.2 首次读取时**自动**更新——无需删除、无需重新输入，已给出的任何答案都不会被覆盖。同一步骤也会记录卡已展示，这正是已在运行 Folio 的机器不会遇到它的原因。`session.json`、`profiles.json`、`keybindings.json` 与 `pins.json` 的读写方式与 0.2.1 完全一致，且没有移动任何快捷键、默认值或文件位置。

解压覆盖到旧文件夹或旁边，运行 `folio.exe`。压缩包内与 0.2.1 一样是**九个**文件，`folio.msix` 仍与 `folio.exe` 放在同一文件夹。

## 下载与运行

从此版本获取 `folio-0.2.2-windows-x64.zip`，解压到存放程序的任意位置，运行 `folio.exe`。无需安装程序，运行之前不会在该文件夹之外写入任何内容。`SHA256SUMS.txt` 是所下载文件的哈希。

需要 **Windows 10 1809 或更高版本，或 Windows 11，64 位**。

`folio.exe` 与 `folio.msix` 由 **Weiyi Shi** 签名，证书来自 Microsoft 的 Artifact Signing 服务并带有 Microsoft 时间戳，与 0.2.1 相同。

## 已知问题

- **新签名尚无信誉。** SmartScreen 在首次运行时仍可能提示 **「Windows 已保护你的电脑」**。**更多信息**会标明发布者为 **Weiyi Shi**、应用程序为 `folio.exe`；**仍要运行**是通过的方式，关闭 SmartScreen 则不是。
- **Folio 无法作为 Visual Studio Code 内部的面板。** 压缩包中的 `folio-here.cmd` 使其成为 VS Code 打开的外部终端。
- **窗口上次所在的显示器出现得晚时，它会回到主显示器上。** 显示器只在窗口创建前计数一次。
- **版面重排时，两张页面可能互相遮挡约 200 毫秒。** 每个托管页面下方都有一块窗口颜色的底板，先打开页面的底板位于后打开页面之上。静止的窗格永远不会重叠；当它们滑向新位置——拆分、关闭、放下、拖出或合并窗格——矩形可能相交，短时间内一个页面被另一个的底板遮住。拖动分隔条时不会出现此问题。
- **曾有报告称窗口移到第二台显示器后上半部分绘制为黑色**，未能复现。
- **`.webm` 需要 Microsoft Store 中的 VP9 或 AV1 视频扩展**。出厂 Windows 两者皆无，缺少时既无静止画面也无播放。

完整列表见仓库中的 `CHANGELOG.md`。
