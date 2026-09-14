<!-- zh: pending -->
# Installing Folio

<!-- zh: pending -->
[`README.zh-CN.md`](../README.zh-CN.md) has the three lines most people need.
This is the rest: what each download contains, what a machine has to be, what
the first run asks, and what to do if the system puts a panel in front of you.

## Windows

从[发布页](https://github.com/lulu-loopp/folio-terminal/releases)下载
`folio-<version>-windows-x64.zip`，解压到任意目录，运行 `folio.exe`。无需安装。`SHA256SUMS.txt` 是下载文件的校验和。系统要求：**Windows 10 1809 及以上或 Windows 11，64 位**。

压缩包共九个文件，需放在同一文件夹中。`folio.exe` 是主程序；`conpty.dll` 和 `OpenConsole.exe` 是启动 shell 的必需组件；`folio.msix` 是签名包，用于注册右键菜单第一页入口，指向解压目录；`folio-here.cmd` 供 VS Code 调用；其余是两份许可证、第三方声明和商标说明。

网页预览需要 **WebView2 Runtime**。Windows 11 已内置；Windows 10 通常也有，若缺少可安装 [Evergreen Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)。缺少时预览窗格提示。

<!-- winget: add when live -->

## macOS

<!-- zh pending opus46 -->
Take `Folio-<version>-macos-arm64.dmg` from the same
[releases page](https://github.com/lulu-loopp/folio-terminal/releases), open it,
and drag **Folio** to Applications. Needs an **Apple silicon Mac running macOS 14
or newer**; there is no Intel build in this preview. `SHA256SUMS.txt` is the hash
of what you downloaded.

<!-- zh: pending -->
Or, with [Homebrew](https://brew.sh):

```sh
brew install --cask lulu-loopp/folio/folio
```

<!-- zh pending opus46 -->
The web preview uses the WebKit already on the machine. There is nothing to
install.

<!-- zh: pending -->
## If Windows or macOS shows a warning

<!-- zh: pending -->
Every release is signed, and the macOS one is notarized by Apple as well;
[`RELEASING.md`](RELEASING.md) says what is signed on each platform and who
holds the certificate. A machine that has not seen a signature before still puts
one panel in front of you the first time.

<!-- zh: pending -->
- **Windows** — "Windows protected your PC". **More info** names the publisher,
  and **Run anyway** starts it. Check that the name there is the holder
  `RELEASING.md` names.
- **macOS** — a panel naming the developer, with **Open** in it. If it opens
  without offering **Open**, right-click the application and choose **Open**
  instead. It asks once and not again.

<!-- zh pending opus46 -->
What must **not** appear on a Mac is a panel saying the developer **cannot be
verified**, or that Folio is **damaged and can't be opened**. Either means what
you have is not what was published — an interrupted download, or a copy altered
after it was signed. Check it against `SHA256SUMS.txt` and take it from the
releases page again.

## 初次启动

首次运行 Folio 的机器会看到一张初次设置卡（**欢迎使用 Folio**），仅出现一次。卡片为本机能做的每件事列一行，行尾是开关：有新版本时通知（唯一默认开启的选项）、在右键菜单中添加 Folio、PowerShell 整合，以及为本机已安装的 Claude Code、Codex 和 Copilot CLI 各设一行标签页提醒。三个 agent 行只在对应工具装在本机时出现，所以卡片少则两行，多则六行。不涉及主题、字体、大小、语言或布局——这些在设置中随时能改，选错了也没什么。

<!-- zh pending opus46 -->
**The card offers only the rows the machine can honour.** The folder right-click
menu and the PowerShell integration are Windows facilities, so a Mac is asked
about the update check and the agents and nothing else.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/first-run-dark.png">
  <img src="screenshots/first-run-light.png" width="100%"
       alt="刚启动的窗口上方显示初次设置卡：Folio 图标旁标题为「欢迎使用
       Folio」，下方各行右侧各有开关。「有新版本时通知」已开启；
       「在右键菜单中打开 Folio」和「PowerShell 整合，支持命令间跳转」
       已关闭；空开一段间距后，Claude Code 等待时、Codex 回合结束时、
       Copilot CLI 等待时点亮标签页三项均关闭。底部小字写着「以上选项
       均可在设置中更改」，然后是「暂不」和「完成」。卡片背后是一个
       标签页和等待输入的命令行。">
</picture>

**悬停在某行上可查看具体说明**——包括该选项写入哪个文件，以及写入前将原文件带日期备份。

**卡片上的每一行也是设置中的一行**，随时可以更改。**完成**应用当前开启的选项；**暂不**或 `Esc` 保留默认值——检查更新开启，其余关闭。卡片只出现一次。已用过 Folio 的机器不会再看到它。

<!-- zh pending opus46 -->
The first tab opens the first shell your machine actually has. On Windows the
five shipped profiles are looked for in order — PowerShell 7, Windows
PowerShell, WSL, Git Bash, Command Prompt; on a Mac it is the shell your account
already uses, then zsh, then bash, then `/bin/sh`. One whose program is not
installed does not appear in the menus that start a shell; it stays on the
Profiles page in Settings, greyed out and naming the program that was looked
for. The seven agent profiles are found the same way, on the `PATH`, and that is
the same lookup the card's agent rows use.

<!-- zh pending opus46 -->
**On Windows**, the PowerShell integration adds one line —
`. "$env:APPDATA\Folio\shell-integration\folio.ps1"` — to the `$PROFILE` a
PowerShell names for itself, after copying the file as it stood to a dated
backup beside it; delete that line to undo it. The change takes effect in the
next PowerShell session, and Settings > Terminal says so until it does. Git Bash
and WSL need none of it, and neither do zsh and bash on a Mac: Folio hands those
their own integration as it starts them, out of its own directory, and writes
nothing of yours.

如果初次设置卡未询问此项，Folio 会在 PowerShell 窗格首次输出时弹出提示条。**添加到 `$PROFILE`** 立即写入，**不再提示**关闭后续询问，直接关闭提示条不做决定——下次 PowerShell 启动时再问一次。命令标记和行内 `$…$` 公式排版依赖此整合。Git Bash 和 WSL 无需此整合。

<!-- zh pending opus46 -->
**On a Mac**, what Folio remembers lives in
`~/Library/Application Support/Folio`.

<!-- zh pending opus46 -->
The first time a waiting agent's mark has to leave the window, macOS asks
whether Folio may send notifications. Answer it once. Say no and the Agent page
in Settings says so rather than going quiet, the dot on the tab and the Dock
icon go on working, and Folio does not ask again.

<!-- zh pending opus46 -->
Finder's right-click menu gets **Open in Folio**, under **Services** — Folio
registers it the first time it runs, so there is nothing to switch on and no
need to sign out. On a folder it opens a tab standing in that folder; on a file,
a tab standing in the folder the file is in. Folio also has a menu bar of its
own, and every item in it carries the same key as the row it has in
[快捷键](shortcuts.md).

Agent 页的三个开关**默认关闭**，各自读取对应工具的配置文件并显示当前状态。新机器上配置文件不存在，三项均显示关闭。

## 已知问题

- **曾有一例报告窗口上半部全黑**，发生在移至第二显示器后，尚未复现。如遇到请附上 `%APPDATA%\Folio\diagnostics.log`（Mac 上是 `~/Library/Application Support/Folio/diagnostics.log`）。 <!-- zh pending opus46 -->
- **Windows：`.webm` 需要从 Microsoft Store 安装 VP9 或 AV1 Video Extension**。Windows 默认未包含，缺少时无法播放。 <!-- zh pending opus46 -->
- **macOS: a run that ends in a crash** leaves its report where the system puts
  every one, `~/Library/Logs/DiagnosticReports`; the next Folio names the file
  in its own log rather than copying it anywhere.
- 其余已知问题见 [`CHANGELOG.md`](../CHANGELOG.md)。
