> Draft for the GitHub Release body. The published text is settled by the user at release time. 草稿，发布时由用户审定。

# Folio 0.2.0-preview

Two surfaces that were not there before — a terminal that comes down on a key
from anywhere, and one box that searches everything the window knows about — and
the first release that carries a signature.

## A terminal on a hotkey

![The summoned terminal across the top of a screen](https://raw.githubusercontent.com/lulu-loopp/folio-terminal/v0.2.0-preview/docs/screenshots/quake-light.png)

- ``Win+` `` drops a terminal across the top of whichever screen the pointer is
  on, over whatever was standing there. Pressing it again puts the window away
  and hands the keyboard back to the program it came down over.
- It is the same Folio — tabs, panes, the files column, the preview, every
  shortcut — and it keeps its shells and its scrollback between summons.
- A rectangle you move or resize by hand is remembered for the display it is on.
- It lives and dies with Folio: no icon of its own, and nothing left running
  behind the key. Closing the last window you can see ends the run.
- **Settings > Summoned terminal** holds the key, which profile a new tab opens
  on, the height, width and the gap below the top of the screen, whether the
  window hides when the keyboard leaves it, and a command to run on the first
  summon of each run.
- At the next launch a pinned tab's last command can come back typed at its
  prompt — typed, and not run. Nothing on that row ever runs anything.

## Search everything

![The palette open over a window, five sections of results](https://raw.githubusercontent.com/lulu-loopp/folio-terminal/v0.2.0-preview/docs/screenshots/palette-light.png)

- `Ctrl+Shift+P` raises a box over the top of the window with five sections that
  never mix: what Folio can do, the panes and tabs this window has open, the
  commands it has run, the files under the folder its column is standing in, and
  the settings.
- Typing narrows all five together and the arrow keys walk them. `Enter` on a
  pane raises it, on a file opens it in the preview pane, on a setting opens
  Settings at that row, and on an action does it.
- A command still running is pointed at with a ring around the pane it is running
  in, rather than a scroll to a line that has gone past.

## Also

- **The folder menu remembers where you have been.** The list under a files
  column's folder button has a third group: the last five folders you pointed a
  column at, newest first, each marked `recent`. A shell running `cd` is not one
  of them, so the list stays the places you meant to go.
- **`folio-here.cmd` ships in the archive.** Folio cannot be a panel inside VS
  Code, but it can be the terminal VS Code opens beside itself: point
  `terminal.external.windowsExec` at that file.
- **Paths an application wrapped across rows are one link again**, including a
  block of indented rows and a hanging indent under a bullet; a path after a
  full-width colon (`早就在:dist\folio.exe`) is found; and a path holding an 8.3
  short name (`PROGRA~1\tools\a.txt`) is a path.
- **The summon key no longer types itself.** ``Win+` `` used to leave a
  `` ` `` at the prompt of the terminal it had just called up.

## Signed

From 0.2.0 the `folio.exe` in this archive is signed by **Weiyi Shi**, with a
certificate issued by Microsoft's Artifact Signing service and countersigned by
Microsoft's time stamping service — so the signature keeps verifying long after
the three-day certificate that made it has expired. `conpty.dll` and
`OpenConsole.exe` are Microsoft's and keep Microsoft's own signature.

A signature is not a reputation. Until enough people have run a build carrying
this one, SmartScreen may still raise **"Windows protected your PC"** on the
first run. **More info** now names **Weiyi Shi** as the publisher and
`folio.exe` as the application; **Run anyway** is the way through, and switching
SmartScreen off is not.

## Upgrading from 0.1.1

- `settings.json` and `session.json` are migrated the first time 0.2.0 reads
  them. Nothing has to be deleted, moved or re-entered.
- The summoned terminal **does not** hide when you click into another program.
  That is Settings > Summoned terminal > **Hide when it loses focus**, and it
  ships off.
- There is **no notification-area icon**. The summoned terminal lives and dies
  with Folio, so closing the last window you can see ends the run and takes the
  hidden window with it.
- ``Win+` `` is bound out of the box. Change or clear it on the Shortcuts page,
  or on Settings > Summoned terminal.

## Download and run

Take `folio-0.2.0-windows-x64.zip` from this release, unpack it wherever you keep
programs, and run `folio.exe`. There is no installer, and nothing is written
outside that folder until you run it. `SHA256SUMS.txt` is the hash of what you
downloaded.

Needs **Windows 10 1809 or newer, or Windows 11, 64-bit**.

## Known issues

- **A new signature has no reputation yet.** See the note above.
- **"Open Folio here" is not on the first page** of the Windows 11 context menu.
  That page needs a packaged application as well as a signed one; it is planned
  for 0.2.1.
- **Folio cannot be a panel inside Visual Studio Code.** `folio-here.cmd` makes
  it the external terminal VS Code opens instead.
- **A window saved on a monitor that enumerates late** comes back on the primary
  display. The displays are counted once, before the window is made.
- **`.webm` needs the VP9 or AV1 Video Extension** from the Microsoft Store. A
  stock Windows has neither, and without one there is no still and no playback.

The full list is in `CHANGELOG.md` in the repository.

---

# Folio 0.2.0-preview

两项此前没有的功能——一个快捷键从任何位置呼出的终端，以及一处搜索窗口所知全部内容的面板——本次亦是第一个带签名的版本。

## 快捷键呼出的终端

![横贯屏幕顶部的快捷终端](https://raw.githubusercontent.com/lulu-loopp/folio-terminal/v0.2.0-preview/docs/screenshots/quake-light.png)

- ``Win+` `` 将终端下拉至指针所在显示器的顶部，覆盖其下原有的画面；再按一次收起窗口，键盘交还给它覆盖前的程序。
- 它就是同一个 Folio：标签页、窗格、文件列、预览与全部快捷键均在，两次呼出之间 shell 与历史输出保持不变。
- 用户手动移动或调整后的矩形按显示器分别记录。
- 它与 Folio 同生同死：没有独立图标，键后也不留驻留进程；关闭最后一扇可见窗口即结束本次运行。
- **设置 > 快捷终端** 中可设置呼出键、新标签页使用的配置、窗口高度与宽度、距屏幕上沿的间距、失去键盘焦点时是否收起，以及每次运行首次呼出时执行的命令。
- 下次启动时，钉住的标签页可将上次运行的命令填回提示符——只填入，不执行。该行的任何一项都不会代用户执行命令。

## 一处搜索全部内容

![窗口上方打开的面板，五段结果](https://raw.githubusercontent.com/lulu-loopp/folio-terminal/v0.2.0-preview/docs/screenshots/palette-light.png)

- `Ctrl+Shift+P` 在窗口上方弹出面板，分五段且互不混排：Folio 可执行的动作、本窗已打开的窗格与标签页、本窗运行过的命令、文件列所在目录下的文件，以及设置项。
- 输入内容同时收窄五段，方向键在其中移动。回车落在窗格上即切至该窗格，落在文件上即在预览窗格中打开，落在设置项上即打开设置并定位到该行，落在动作上即执行该动作。
- 命令仍在运行时，Folio 在其所在窗格周围画出一圈提示，而非滚动到一条已经翻过去的行。

## 其他

- **文件夹菜单记住去过的位置。** 文件列的文件夹按钮下多出一段：最近指向过的五个文件夹，最新的在前，各标 `recent`。shell 执行 `cd` 不计入其中，因此这一段始终是用户自己要去的位置。
- **压缩包内附带 `folio-here.cmd`。** Folio 不能作为 VS Code 内嵌的终端面板，但可以作为 VS Code 在旁边打开的外部终端：将 `terminal.external.windowsExec` 指向该文件即可。
- **被程序折行的路径重新拼为一条链接**，包括整块缩进的多行与项目符号下的悬挂缩进；全角冒号之后的路径（`早就在:dist\folio.exe`）可识别；含 8.3 短名的路径（`PROGRA~1\tools\a.txt`）也是路径。
- **呼出键不再把自己打进终端。** 此前 ``Win+` `` 会在刚呼出的终端提示符处留下一个 `` ` ``。

## 签名

自 0.2.0 起，本压缩包内的 `folio.exe` 由 **Weiyi Shi** 签名，证书由微软 Artifact Signing 服务签发，并由微软时间戳服务加盖时间戳——因此签名在签发它的三天期证书过期之后仍然长期可验证。`conpty.dll` 与 `OpenConsole.exe` 属于微软，保留微软自己的签名。

签名不等于信誉。在携带此签名的版本积累到足够的运行量之前，首次运行时仍可能出现「Windows 已保护你的电脑」提示。此时「更多信息」中所列发布者为 **Weiyi Shi**，程序名为 `folio.exe`；点击「仍要运行」即可，请勿为此关闭 SmartScreen。

## 从 0.1.1 升级

- `settings.json` 与 `session.json` 在 0.2.0 首次读取时自动迁移，无需删除、移动或重新填写任何内容。
- 快捷终端在用户点进其他程序时**不会**自动收起。该项位于 设置 > 快捷终端 > **失去焦点时收起**，出厂为关闭。
- **没有通知区域图标。** 快捷终端与 Folio 同生同死，关闭最后一扇可见窗口即结束本次运行，隐藏的窗口随之退出。
- ``Win+` `` 出厂即绑定。可在快捷键页或 设置 > 快捷终端 中更改或清除。

## 下载与运行

从本次发布中获取 `folio-0.2.0-windows-x64.zip`，解压至存放程序的目录，运行 `folio.exe`。无安装程序；运行之前，解压目录之外不会写入任何内容。`SHA256SUMS.txt` 为所下载文件的哈希值。

需 **Windows 10 1809 或更高版本，或 Windows 11，64 位**。

## 已知问题

- **新签名尚无信誉。** 见上一节。
- **「在此处打开 Folio」不在 Windows 11 右键菜单的首层。** 首层菜单除签名外还要求程序已打包，该功能计划于 0.2.1 实现。
- **Folio 不能作为 Visual Studio Code 的内嵌终端面板。** `folio-here.cmd` 使其成为 VS Code 打开的外部终端。
- **存放于枚举较晚的显示器上的窗口在主显示器上打开。** 显示器只在窗口创建之前清点一次。
- **`.webm` 需要 Microsoft Store 的 VP9 或 AV1 视频扩展。** 出厂 Windows 两者均未安装；缺少扩展时，首帧与播放均不可用。

完整清单见仓库中的 `CHANGELOG.md`。
