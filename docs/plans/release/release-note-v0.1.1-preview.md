> Draft for the GitHub Release body. The published text is settled by the user at release time. 草稿，发布时由用户审定。

# Folio 0.1.1-preview

0.1.1 is a fixes-and-polish release for 0.1.0-preview. Nothing is driven
differently. What changes is what the window tells you, and what it tells the
programs running inside it.

## Fixes

- An image opened in a pane follows the file on disk. Writing a different file
  over the same name is a change, and so is deleting it and writing it again;
  opening an image always reads the disk.
- A graphics device the driver takes away — a laptop changing power source is
  the everyday cause — is rebuilt where it stood, with every session, every
  terminal's contents and every open document untouched. It used to end the run.
- Minimising a window no longer lays that window out for the rectangle of its
  icon, which used to leave every shell in it two or three columns wide for as
  long as the window stayed down.
- Dragging a divider sends the shell one size, at the moment the hand lets go,
  instead of every width the hand passed through.
- A restored window is fitted to the display it lands on, so a size recorded on
  a larger screen no longer opens below the bottom of a smaller one. A second
  window's recorded place survives the next start.
- A floating preview shows the `Reload` / `Keep` strip when its document changes
  on disk, and a pane dragged into another window is carried by a card that
  draws the pane rather than a blank one.
- A drag held at the foot of the card column no longer changes its mind between
  dropping into a card and becoming a card of its own.

## Notifications

- The notice raised when an agent finishes a turn carries that agent's own first
  sentence instead of the words "Turn finished". Claude Code's comes from the
  transcript its `Stop` hook names, Codex's from the payload it hands its notify
  command. Nothing is read off the screen.
- A window you can see, a second monitor included, is marked inside the window
  and nowhere else: no taskbar flash and no message on the desktop. A covered,
  minimised or hidden window escalates as before.

## Update check

- Folio asks the releases page once a day whether a newer version exists. When
  there is one, the settings gear wears a dot and Settings > General names the
  version. Nothing is downloaded and nothing is replaced.
- The request carries one header, `User-Agent: Folio`, and nothing else. Switch
  it off at Settings > General > **Update check**. What is asked and what is
  stored is written out in `docs/PRIVACY.md`.

## Download and run

Take `folio-0.1.1-windows-x64.zip` from this release, unpack it wherever you keep
programs, and run `folio.exe`. There is no installer, and nothing is written
outside that folder until you run it. `SHA256SUMS.txt` is the hash of what you
downloaded.

Needs **Windows 10 1809 or newer, or Windows 11, 64-bit**.

This release is not code-signed, so the first run may raise **"Windows protected
your PC"**. Click **"More info"**, check the application named there is
`folio.exe`, and click **"Run anyway"**. Do not switch SmartScreen off for this.

## Known issues

- **Not signed.** See the note above.
- **"Open Folio here" is not on the first page** of the Windows 11 context menu.
  That page needs a signed, packaged application, so it waits on signing.
- **A window saved on a monitor that enumerates late** comes back on the primary
  display. The displays are counted once, before the window is made.
- **`.webm` needs the VP9 or AV1 Video Extension** from the Microsoft Store. A
  stock Windows has neither, and without one there is no still and no playback.

The full list is in `CHANGELOG.md` in the repository.

---

# Folio 0.1.1-preview

0.1.1 是 0.1.0-preview 的修复与打磨版本。操作方式不变，变化在于窗口向用户说明什么，以及窗口向其中运行的程序说明什么。

## 修复

- 窗格中打开的图片跟随磁盘上的文件。以同名文件覆盖原文件属于变动，删除后重新写入同样属于变动；每次打开图片，Folio 一律重新读取磁盘。
- 驱动收回显示设备时（笔记本电脑更换供电方式为常见成因），Folio 在原处重建该设备，会话、终端内容与已打开的文档全部保持原样。此前该情形直接结束运行。
- 最小化窗口不再按图标矩形解算布局。此前窗口保持最小化期间，其中每个 shell 的宽度只有两三列。
- 拖动分隔条时，Folio 只在松手一刻把一个尺寸告知 shell，不再逐个报出手经过的每一个宽度。
- 恢复出的窗口按其落点所在的显示器收口尺寸，在大屏上记下的尺寸不再在小屏上越出屏幕下沿。第二扇窗口存下的位置在下次启动后保持不变。
- 浮窗中的预览在文档于磁盘上改动时显示 `Reload` / `Keep` 提示条；拖入另一扇窗口的窗格，其替身卡片绘出该窗格的真实画面，不再是一张空卡。
- 拖拽停在卡片列脚下时，落点不再在「并入卡片」与「自成一卡」之间反复变化。

## 通知

- agent 结束一个回合时，通知带上该 agent 自己的第一句话，不再只写「Turn finished」。Claude Code 的来自其 `Stop` hook 指明的 transcript，Codex 的来自其 notify 命令收到的载荷。Folio 不从屏幕上读取任何内容。
- 用户看得见的窗口（含副屏）只在窗内留下标记：任务栏不闪烁，桌面不弹出消息。窗口位于其他窗口之下、处于最小化，或位于其他桌面时，通知照旧升级。

## 新版本提示

- Folio 每天至多一次向 releases 页面询问是否存在更新的版本。存在更新的版本时，设置齿轮上出现一个点，设置 > General 中的一行给出版本号。Folio 不下载任何内容，也不替换任何文件。
- 该请求只携带一个请求头 `User-Agent: Folio`，此外别无内容。关闭位置为设置 > General > **检查新版**。请求内容与存储内容详见 `docs/PRIVACY.md`。

## 下载与运行

从本次发布中获取 `folio-0.1.1-windows-x64.zip`，解压至存放程序的目录，运行 `folio.exe`。无安装程序；运行之前，解压目录之外不会写入任何内容。`SHA256SUMS.txt` 为所下载文件的哈希值。

需 **Windows 10 1809 或更高版本，或 Windows 11，64 位**。

本版本未进行代码签名，首次运行时可能出现「Windows 已保护你的电脑」提示。请点击「更多信息」，确认所列程序名为 `folio.exe`，再点击「仍要运行」。请勿为此关闭 SmartScreen。

## 已知问题

- **未签名。** 见上一节。
- **「在此处打开 Folio」不在 Windows 11 右键菜单的首层。** 首层菜单要求程序已完成签名并打包，该功能待签名后实现。
- **存放于枚举较晚的显示器上的窗口在主显示器上打开。** 显示器只在窗口创建之前清点一次。
- **`.webm` 需要 Microsoft Store 的 VP9 或 AV1 视频扩展。** 出厂 Windows 两者均未安装；缺少扩展时，首帧与播放均不可用。

完整清单见仓库中的 `CHANGELOG.md`。
