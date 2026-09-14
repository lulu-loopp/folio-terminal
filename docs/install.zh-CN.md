# 安装 Folio

[`README.zh-CN.md`](../README.zh-CN.md) 是多数人需要的简要版。这里是详细内容：每个下载包里有什么、系统要求、初次启动时的设置，以及系统弹出提示时该怎么办。

## Windows

从[发布页](https://github.com/lulu-loopp/folio-terminal/releases)下载
`folio-<version>-windows-x64.zip`，解压到任意目录，运行 `folio.exe`。无需安装。`SHA256SUMS.txt` 是下载文件的校验和。系统要求：**Windows 10 1809 及以上或 Windows 11，64 位**。

压缩包共九个文件，需放在同一文件夹中。`folio.exe` 是主程序；`conpty.dll` 和 `OpenConsole.exe` 是启动 shell 的必需组件；`folio.msix` 是签名包，用于注册右键菜单第一页入口，指向解压目录；`folio-here.cmd` 供 VS Code 调用；其余是两份许可证、第三方声明和商标说明。

网页预览需要 **WebView2 Runtime**。Windows 11 已内置；Windows 10 通常也有，若缺少可安装 [Evergreen Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)。缺少时预览窗格提示。

<!-- winget: add when live -->

## macOS

从同一[发布页](https://github.com/lulu-loopp/folio-terminal/releases)下载 `Folio-<version>-macos-arm64.dmg`，打开后将 **Folio** 拖入 Applications。需要 **Apple silicon Mac，macOS 14 及以上**；当前预览版无 Intel 构建。`SHA256SUMS.txt` 是下载文件的校验和。

也可以用 [Homebrew](https://brew.sh)：

```sh
brew install --cask lulu-loopp/folio/folio
```

网页预览使用系统自带的 WebKit，无需额外安装。

## 如果 Windows 或 macOS 弹出提示

发布包已签名，macOS 版本还经过 Apple 公证；[`RELEASING.md`](RELEASING.md) 说明各平台的签名方式和证书持有人。首次运行时系统可能仍会弹出一个提示。

- **Windows**——提示"Windows 已保护你的电脑"。点击**更多信息**可看到发布者名称，点击**仍要运行**即可启动。核实发布者名称与 `RELEASING.md` 中一致。
- **macOS**——弹出一个显示开发者名称的面板，其中有**打开**按钮。如果面板没有提供**打开**，右键点击应用选择**打开**。只询问一次。

Mac 上**不应**看到提示说开发者**无法验证**，或者 Folio **已损坏且无法打开**。出现这类提示意味着文件与发布版本不一致——可能是下载中断或签名后被修改。用 `SHA256SUMS.txt` 校验后重新从发布页下载。自行编译并临时签名的版本也会触发同样的提示；处理方式见 [`BUILDING.md`](BUILDING.md)。

## 初次启动

首次运行 Folio 的机器会看到一张初次设置卡（**欢迎使用 Folio**），仅出现一次。卡片为本机能做的每件事列一行，行尾是开关：有新版本时通知（唯一默认开启的选项）、在右键菜单中添加 Folio、PowerShell 整合，以及为本机已安装的 Claude Code、Codex 和 Copilot CLI 各设一行标签页提醒。三个 agent 行只在对应工具装在本机时出现，所以卡片少则两行，多则六行。不涉及主题、字体、大小、语言或布局——这些在设置中随时能改，选错了也没什么。

**卡片只列出本机可用的选项。** 右键菜单和 PowerShell 整合是 Windows 功能，Mac 上只询问更新检查和 agent 相关选项。

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

首个标签页打开本机实际安装的第一个 shell。Windows 上按顺序查找五个内置配置——PowerShell 7、Windows PowerShell、WSL、Git Bash、命令提示符；Mac 上先用账户当前设置的 shell，然后依次是 zsh、bash、`/bin/sh`。未安装的 shell 不出现在启动菜单中，但保留在设置的配置页，显示为灰色并标注所查找的程序。七个 agent 配置以同样方式在 `PATH` 上查找，初次设置卡的 agent 行也使用同一查找结果。

**Windows 上**，PowerShell 整合向 PowerShell 指定的 `$PROFILE` 末尾添加一行 `. "$env:APPDATA\Folio\shell-integration\folio.ps1"`，写入前将原文件带日期备份在旁边；删除该行即可撤销。改动在下一个 PowerShell 会话中生效，设置 > 终端在生效前会提示。Git Bash 和 WSL 无需此操作，Mac 上的 zsh 和 bash 也不需要：Folio 在启动它们时从自己的目录传入整合脚本，不写入用户文件。

如果初次设置卡未询问此项，Folio 会在 PowerShell 窗格首次输出时弹出提示条。**添加到 `$PROFILE`** 立即写入，**不再提示**关闭后续询问，直接关闭提示条不做决定——下次 PowerShell 启动时再问一次。命令标记和行内 `$…$` 公式排版依赖此整合。Git Bash 和 WSL 无需此整合。

**Mac 上**，Folio 的数据存放在 `~/Library/Application Support/Folio`。

agent 等待标记首次需要在窗口之外提醒时，macOS 会询问是否允许 Folio 发送通知。回答一次即可。如果拒绝，设置中的 Agent 页会显示此状态，标签页上的圆点和 Dock 图标提醒仍然有效，Folio 不会再次询问。

Finder 右键菜单的**服务**下有 **Open in Folio**——Folio 首次运行时自动注册，无需手动开启，也不需要注销。在文件夹上点击时打开一个标签页进入该文件夹；在文件上点击时进入文件所在的文件夹。Folio 还有自己的菜单栏，每一项的快捷键与[快捷键](shortcuts.md)表中对应行一致。

Agent 页的三个开关**默认关闭**，各自读取对应工具的配置文件并显示当前状态。新机器上配置文件不存在，三项均显示关闭。

## 已知问题

- **曾有一例报告窗口上半部全黑**，发生在移至第二显示器后，尚未复现。如遇到请附上 `%APPDATA%\Folio\diagnostics.log`（Mac 上是 `~/Library/Application Support/Folio/diagnostics.log`）。
- **Windows：`.webm` 需要从 Microsoft Store 安装 VP9 或 AV1 Video Extension**。Windows 默认未包含，缺少时无法播放。
- **macOS：崩溃退出时**，系统将崩溃报告放在 `~/Library/Logs/DiagnosticReports`；下次启动时 Folio 在自己的日志中记录该报告的路径。
- 其余已知问题见 [`CHANGELOG.md`](../CHANGELOG.md)。
