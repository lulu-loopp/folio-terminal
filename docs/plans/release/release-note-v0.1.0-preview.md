> Draft for the GitHub Release body. The published text is settled by the user at release time. 草稿，发布时由用户审定。

# Folio 0.1.0-preview

Folio is a Windows terminal: formulas are typeset where a command prints them,
files preview beside the prompt, and an agent that is waiting for you says so.

## What is in this release

### LaTeX in command output

- `$…$` and `$$…$$` in command output are typeset on the line the command
  printed them on. What cannot be typeset is shown as it was printed.
- The preview pane takes those, plus `\(…\)`, `\[…\]` and the bare `amsmath`
  environments. One typesetter serves both.
- Inline `$…$` is told apart from a shell variable by the PowerShell integration.
  Without it, inline formulas stay as source and `$$…$$` blocks still typeset.

### Made for agents

- A waiting agent lights a dot on its tab, flashes the taskbar when another
  program has the focus, and raises a Windows notification when the window is
  minimised or on another desktop. `Ctrl+Shift+A` jumps to the longest wait.
- Claude Code, Codex and GitHub Copilot CLI each have a switch on the Agent page
  in Settings that writes one notification hook into that tool's own
  configuration file and takes it back out again. Nothing is installed by
  default.
- Seven profiles start an agent — Claude Code, Codex, Copilot CLI, Kimi Code, pi,
  Hermes, OpenCode. Any program that writes `OSC 1337;RequestAttention=yes` is
  heard with nothing installed at all.

### Preview beside the prompt

- A card comes up under the pointer when it rests on a name in the files column:
  a PDF page by page, a video playing, the first lines of a text file, an image.
- The preview pane opens the file beside the prompt — markdown typeset, PDF page
  by page, video playing, a web page with an address field and Back.
- A path or a web address printed in the terminal opens in the preview pane on a
  click, and goes to the machine's own application or browser on `Ctrl`+click.
  Paths nobody marked up are found too, once the file is confirmed to exist.
- Video is decoded by Windows itself: `.mp4`, `.m4v`, `.mov`, `.mkv`, `.avi`,
  `.wmv` and `.webm`, with controls drawn by the window.

### Panes, tabs and windows

- `Alt+Shift+-` splits a pane across, `Alt+Shift+=` splits it down. A tab or a
  single pane can be dragged out into a window of its own, and the panes it did
  not touch keep their widths.
- `Ctrl+Shift+Z` turns the tab strip into a column of cards, one per tab, each
  drawing that tab's own panes in the layout they have.
- `Ctrl+Shift+G` turns the files column into a Git panel: branch, working tree,
  staged and unstaged files, the commit graph, and a selected file's diff in the
  preview.

### Windows integration

- "Open Folio here" is in the Explorer context menu, under "Show more options".
- Windows PowerShell 5.1 ships PSReadLine 2.0.0, which misplaces the input line
  after the window is resized. Folio carries a patched 2.4.6 and installs it into
  your module path on request.
- Settings is one dialog — font, colour scheme, scrollback, shell profiles, every
  shortcut key — in English and Chinese, switched without restarting.

The full list is in `CHANGELOG.md` in the repository.

## Download and run

Take `folio-0.1.0-windows-x64.zip` from this release, unpack it wherever you keep
programs, and run `folio.exe`. There is no installer, and nothing is written
outside that folder until you run it. `SHA256SUMS.txt` is the hash of what you
downloaded.

Needs **Windows 10 1809 or newer, or Windows 11, 64-bit**.

The web preview needs the **WebView2 Runtime**. Windows 11 has it; Windows 10
usually does, and if it does not, the Evergreen Runtime is at
<https://developer.microsoft.com/microsoft-edge/webview2/>. Without it everything
except the web preview works, and the preview says what is missing.

This build is not code-signed, so the first run may raise **"Windows protected
your PC"**. Click **"More info"**, check the application named there is
`folio.exe`, and click **"Run anyway"**. Do not switch SmartScreen off for this.

## Known issues

- **Not signed.** See the note above.
- **A window was once reported drawing its top half black** after a move to a
  second monitor, unreproduced. Attach `%APPDATA%\Folio\diagnostics.log` if you
  hit it.
- **"Open Folio here" is not on the first page** of the Windows 11 context menu.
  That page needs a signed, packaged application, so it waits on signing.
- **`.webm` needs the VP9 or AV1 Video Extension** from the Microsoft Store. A
  stock Windows has neither, and without one there is no still and no playback.

## Privacy

Folio sends nothing anywhere: no telemetry, no analytics, no crash reporting, no
update check, and no network client of its own. The only thing that reaches the
network is a page you open in the web preview.

## Licence

MIT or Apache-2.0, at your option. Every dependency's licence and the notices they
require are in `THIRD-PARTY-NOTICES.md`; the Folio name and the marks are covered
by `TRADEMARK.md`.

## Feedback

Open an issue on this repository. A security report goes through the private
channel in `SECURITY.md`, not an issue.

---

# Folio 0.1.0-preview

Folio 是一款 Windows 终端：命令输出的公式在其打印位置排版，命令提及的文件在提示符旁预览，等待用户响应的 agent 有明确标示。

## 这一版有什么

### 命令输出中的 LaTeX 排版

- 命令输出中的 `$…$` 与 `$$…$$` 在对应打印行内完成排版；无法排版的内容按其打印原样显示。
- 预览窗格除上述两种写法外，还支持 `\(…\)`、`\[…\]` 及不带包裹的 amsmath 环境，两处共用同一排版引擎。
- 行内 `$…$` 依赖 PowerShell 整合与 shell 变量进行区分；未安装整合时，行内公式保持源码原样，`$$…$$` 块仍照常排版。

### agent 的提醒与配置

- 等待响应的 agent 在其标签页显示一个圆点；焦点位于其他程序时，任务栏闪烁；窗口最小化或位于其他桌面时，发出 Windows 通知。`Ctrl+Shift+A` 跳转至等待时间最长的一项。
- 设置的 Agent 页中，Claude Code、Codex 与 GitHub Copilot CLI 各有一行开关：开启时向对应工具自身的配置文件写入一个通知 hook，关闭时将其撤回。默认不安装任何内容。
- 七条配置可直接启动 agent：Claude Code、Codex、Copilot CLI、Kimi Code、pi、Hermes、OpenCode。任何向终端写入 `OSC 1337;RequestAttention=yes` 的程序，无需安装任何内容，Folio 也能收到。

### 提示符旁的预览

- 指针悬停于文件列中的文件名上时，弹出卡片：PDF 可逐页翻看，视频可播放，文本显示开头数行，图片显示图片本身。
- 文件在提示符旁的预览窗格中打开：markdown 完成排版，PDF 逐页显示，视频可播放，网页带有地址栏与后退功能。
- 终端输出的路径与网址单击后在预览窗格中打开，按住 `Ctrl` 单击交由系统默认程序或浏览器处理。未作标记的裸路径在确认文件存在后亦可识别。
- 视频由 Windows 自身解码：`.mp4`、`.m4v`、`.mov`、`.mkv`、`.avi`、`.wmv` 与 `.webm`，控件由窗口自行绘制。

### 标签页、窗格与窗口

- `Alt+Shift+-` 横向分屏，`Alt+Shift+=` 纵向分屏。标签页或单个窗格可拖出并独立成窗，未触及的窗格宽度保持不变。
- `Ctrl+Shift+Z` 将标签条切换为一列卡片，一张卡片对应一个标签页，卡片内按该标签页自身的布局绘出全部窗格。
- `Ctrl+Shift+G` 将文件列切换为 git 面板：显示分支、工作区、暂存与未暂存文件、提交图；选中文件后，其差异显示于预览窗格。

### 与 Windows 的集成

- 「在此处打开 Folio」位于资源管理器右键菜单的「显示更多选项」之下。
- Windows PowerShell 5.1 自带 PSReadLine 2.0.0，该版本在窗口改变大小后会错放输入行。Folio 内置一份已修补的 2.4.6 版本，用户可按需将其安装至模块目录。
- 设置集中于一个对话框：字体、配色、回滚行数、shell 配置文件与全部快捷键；中英双语，切换后立即生效，无需重启。

完整清单见仓库中的 `CHANGELOG.md`。

## 下载与运行

从本次发布中获取 `folio-0.1.0-windows-x64.zip`，解压至存放程序的目录，运行 `folio.exe`。无安装程序；运行之前，解压目录之外不会写入任何内容。`SHA256SUMS.txt` 为所下载文件的哈希值。

需 **Windows 10 1809 或更高版本，或 Windows 11，64 位**。

网页预览需 **WebView2 运行时**。Windows 11 自带该运行时；Windows 10 通常亦已安装，若未安装，可从 <https://developer.microsoft.com/microsoft-edge/webview2/> 获取 Evergreen 运行时。缺少该运行时，除网页预览外的所有功能均正常，预览窗格会说明缺失项。

本版本未进行代码签名，首次运行时可能出现「Windows 已保护你的电脑」提示。请点击「更多信息」，确认所列程序名为 `folio.exe`，再点击「仍要运行」。请勿为此关闭 SmartScreen。

## 已知问题

- **未签名。** 见上一节。
- **曾有窗口移至第二显示器后上半部分显示为黑色的报告，未能复现。** 如遇此问题，请附上 `%APPDATA%\Folio\diagnostics.log`。
- **「在此处打开 Folio」不在 Windows 11 右键菜单的首层。** 首层菜单要求程序已完成签名并打包，该功能待签名后实现。
- **`.webm` 需要 Microsoft Store 的 VP9 或 AV1 视频扩展。** 出厂 Windows 两者均未安装；缺少扩展时，首帧与播放均不可用。

## 隐私

Folio 不向任何位置发送数据：无遥测、无统计、无崩溃上报、无更新检查，亦无自有网络客户端。唯一联网行为为用户在网页预览中打开的页面。

## 许可

MIT 或 Apache-2.0，任选其一。各依赖项的许可及其要求的声明列于 `THIRD-PARTY-NOTICES.md`；Folio 名称与标识见 `TRADEMARK.md`。

## 反馈

请在本仓库提交 issue。安全问题请通过 `SECURITY.md` 中的私密渠道提交，勿以公开 issue 形式提出。
