<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="assets/readme/hero-dark.svg">
  <img src="assets/readme/hero-light.svg" width="100%"
       alt="Folio——能排版公式的 Windows 终端，并提示哪个 agent 在等你。
       图中终端窗格运行了一条命令并输出文件内容，其中的展示公式——e
       的负 x 平方在整条实数轴上的积分等于根号 π——以排版形式显示在
       输出中，位于下一行输入上方。">
</picture>

Folio 是一个 Windows 终端。公式在命令输出中原位排版，文件在终端旁预览，agent 等待时标签页亮灯提醒。

[English](README.md) · [快捷键](docs/shortcuts.md) ·
[安全](SECURITY.md) · [更新记录](CHANGELOG.md)

> **预览版。** 0.2.5 是预览构建，由 Weiyi Shi 签名——见下方[下载](#下载)。

---

## 下载

从[发布页](https://github.com/lulu-loopp/folio-terminal/releases)下载
[`folio-0.2.5-windows-x64.zip`](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.2.5-preview/folio-0.2.5-windows-x64.zip)，解压到任意目录，运行 `folio.exe`。无需安装。`SHA256SUMS.txt` 是下载文件的校验和。系统要求：**Windows 10 1809 及以上或 Windows 11，64 位**。

`folio.exe` 和 `folio.msix` 由 **Weiyi Shi** 签名，证书来自 Microsoft Artifact Signing 服务。首次运行时 Windows 可能弹出 **"Windows 已保护你的电脑"**：点击 **"更多信息"**，再点 **"仍要运行"**，其中显示的发布者为 **Weiyi Shi**。

压缩包共九个文件，需放在同一文件夹中。`folio.exe` 是主程序；`conpty.dll` 和 `OpenConsole.exe` 是启动 shell 的必需组件；`folio.msix` 是签名包，用于注册右键菜单第一页入口，指向解压目录；`folio-here.cmd` 供 VS Code 调用；其余是两份许可证、第三方声明和商标说明。

网页预览需要 **WebView2 Runtime**。Windows 11 已内置；Windows 10 通常也有，若缺少可安装 [Evergreen Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)。缺少时预览窗格提示。

## 初次启动

首次运行 Folio 的机器会看到一张初次设置卡（**欢迎使用 Folio**），仅出现一次。卡片为本机能做的每件事列一行，行尾是开关：有新版本时通知（唯一默认开启的选项）、在右键菜单中添加 Folio、PowerShell 整合，以及为本机已安装的 Claude Code、Codex 和 Copilot CLI 各设一行标签页提醒。三个 agent 行只在对应工具装在本机时出现，所以卡片少则两行，多则六行。不涉及主题、字体、大小、语言或布局——这些在设置中随时能改，选错了也没什么。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/first-run-dark.png">
  <img src="docs/screenshots/first-run-light.png" width="100%"
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

首个标签页打开本机第一个可用的 shell。内置五个配置，按以下顺序查找：PowerShell 7、Windows PowerShell、WSL、Git Bash、命令提示符。未安装的 shell 不出现在新建菜单中，但保留在设置的配置页，标灰并注明所查找的程序。七个 agent 配置按同样方式在 Windows PATH 中查找，初次设置卡的 agent 行用的也是同一个查找结果。

PowerShell 整合在 `$PROFILE` 中添加一行 `. "$env:APPDATA\Folio\shell-integration\folio.ps1"`，添加前将原文件带日期备份在旁。删除该行即可还原。更改在下一个 PowerShell 窗口生效，设置 > 终端会在生效前提示。

如果初次设置卡未询问此项，Folio 会在 PowerShell 窗格首次输出时弹出提示条。**添加到 `$PROFILE`** 立即写入，**不再提示**关闭后续询问，直接关闭提示条不做决定——下次 PowerShell 启动时再问一次。命令标记和行内 `$…$` 公式排版依赖此整合。Git Bash 和 WSL 无需此整合。

Agent 页的三个开关**默认关闭**，各自读取对应工具的配置文件并显示当前状态。新机器上配置文件不存在，三项均显示关闭。

---

## 功能

### 终端中的 LaTeX 排版

命令输出中的 LaTeX 在打印位置直接排版。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/terminal-math-dark.png">
  <img src="docs/screenshots/terminal-math-light.png" width="100%"
       alt="一个终端窗格显示一条命令的排版输出：段落中穿插行内公式，另有
       三个展示公式分占独立行——高斯归一化积分、傅里叶变换对和指数级数。">
</picture>

- 命令输出中的 `$…$` 和 `$$…$$` 在打印所在行排版。
- 预览窗格还支持 `\(…\)`、`\[…\]` 和 `amsmath` 环境。
- 不支持的语法保持原样显示。
- 行内 `$…$` 通过 PowerShell 整合与 shell 变量区分。未启用整合时行内公式保持原文，`$$…$$` 块仍正常排版。

### 为 agent 而设计

等待你操作的 agent 会在标签页上标记，无需逐个检查。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/settings-agents-dark.png">
  <img src="docs/screenshots/settings-agents-light.png" width="100%"
       alt="设置中的 Agent 页：Claude Code、Codex 和 GitHub Copilot CLI
       各一行，每行说明开关写入哪个文件，三个开关均关闭。末尾是回合
       结束通知选项。">
</picture>

- agent 等待时标签页亮一个点；焦点在其他程序时闪烁任务栏；窗口最小化或在其他桌面时弹出 Windows 通知。
- 每次请求最多提醒一次，回应后或程序撤回请求后标记消失。`Ctrl+Shift+A` 跳转到等待最久的 agent。
- Claude Code、Codex 和 GitHub Copilot CLI 在设置的 Agent 页各有一个开关，开启时向对应工具的配置文件写入通知钩子，关闭时移除。默认不安装。
- 七个 agent 配置：Claude Code、Codex、Copilot CLI、Kimi Code、pi、Hermes、OpenCode，通过 Windows PATH 查找；安装在 WSL 内的从 WSL 配置启动。任何程序发送 `OSC 1337;RequestAttention=yes` 即可被识别。

### 终端旁的预览：文件、PDF、视频、网页

文件内容可在终端旁直接查看，无需打开其他程序。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/preview-pdf-dark.png">
  <img src="docs/screenshots/preview-pdf-light.png" width="100%"
       alt="鼠标停在文件列的一个文件名上，下方弹出卡片显示 PDF 第一页，
       以及页数和文件大小。滚轮可翻页。">
</picture>

- 鼠标悬停在文件列的文件名上时弹出卡片：PDF 逐页显示，视频直接播放，文本显示前几行，图片直接展示。
- 预览窗格打开文件：Markdown 排版显示，PDF 逐页翻阅，视频播放，网页带地址栏和后退按钮。
- 终端中输出的路径点击后在预览窗格打开，`Ctrl`+点击在系统默认程序中打开。未标记的路径在确认文件存在后也能识别。
- 网址同理：点击在预览窗格打开，`Ctrl`+点击交给浏览器。
- 一个窗口可以打开与预览窗格数量相同的页面。打开第二个页面时使用新窗格而非覆盖第一个，锁定页面后再打开新页面可并排显示。拖拽页面到指定窗格则在该窗格打开。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="assets/readme/surfaces-dark.png">
  <img src="assets/readme/surfaces-light.png" width="100%"
       alt="另外三种界面：以卡片布局显示的窗口、预览窗格中排版的 Markdown
       文档、预览窗格中的网页（上方有面包屑地址栏）。">
</picture>

### 窗格、标签页，自由移动

布局随时调整，内部的会话保持运行，所有窗格同时可见。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/tab-into-pane-dark.gif">
  <img src="docs/screenshots/tab-into-pane-light.gif" width="100%"
       alt="按住标签页向下拖出标签栏，窗口右半部分出现落点预览，松开后
       该标签页的 shell 变为右侧窗格并保持运行。随后将新窗格的标题栏
       拖到底部边缘，并排布局变为上下两栏。">
</picture>

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/cards-dark.png">
  <img src="docs/screenshots/cards-light.png" width="100%"
       alt="标签栏变为卡片列。单张卡片代表一个包含八个窗格的标签页，以
       缩略图显示全部八个窗格。Alt 加滚轮可逐行滚动卡片内容。">
</picture>

- `Alt+Shift+-` 水平拆分窗格，`Alt+Shift+=` 垂直拆分。标签页或单个窗格可拖出为独立窗口，未拖动的窗格保持原有宽度。
- 窗格拖到两个标签页之间会成为新标签页：列表打开一个空位，窗格停在那里。拖到标签页上则加入该标签页的布局。水平标签栏、竖直标签栏和卡片列均支持此操作。
- `Ctrl+Shift+Z` 将标签栏切换为卡片列，每张卡片以缩略图显示对应标签页的窗格布局。
- `Ctrl+Shift+G` 将文件列切换为 Git 面板：分支、工作区、暂存与未暂存文件、提交图，选中文件的 diff 在预览窗格中显示。
- `Ctrl+Shift+↑` 和 `Ctrl+Shift+↓` 在滚动历史中逐条跳转命令，失败的命令有失败标记。
- 文件列上方的文件夹按钮列出各 shell 当前所在目录和最近五个访问过的目录（标注 `recent`）。

### 快捷终端

一个快捷键调出终端覆盖在屏幕上方，再按一次收回。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/quake-dark.png">
  <img src="docs/screenshots/quake-light.png" width="100%"
       alt="屏幕顶部悬挂的终端窗口，略低于屏幕边缘并居中，覆盖在一个
       资源管理器窗口上。终端有一个标签页、齿轮按钮和关闭按钮，shell
       中显示四条提交记录和目录列表，下方是等待输入的空行。">
</picture>

- ``Win+` `` 在鼠标所在屏幕顶部拉下一个终端窗口，覆盖当前内容。再按一次收回窗口并将焦点还给之前的程序。
- 这是完整的 Folio——标签页、窗格、文件列、预览、所有快捷键都可用。shell 和滚动历史在每次呼出之间保持。
- 手动移动或调整过的窗口按显示器记忆，下次在该屏幕呼出时沿用。
- 快捷终端随 Folio 启停。关闭最后一个可见窗口即结束运行。
- **设置 > 快捷终端**可配置快捷键、新标签页配置、高度、宽度、顶部间距、失焦自动隐藏，以及每次启动首次呼出时运行的命令。
- 固定标签页在下次启动时可恢复上条命令到输入行——只填入，不运行。

### 搜索一切

一个搜索框同时查五类内容，`Enter` 直达目标。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/palette-dark.png">
  <img src="docs/screenshots/palette-light.png" width="100%"
       alt="窗口顶部浮动的搜索框，输入栏中有查询文字，下方结果分五个
       区域且不混排：一个操作、一个窗格、一条已执行的命令、一个文件、
       一个设置项。第一行高亮，每行中匹配的字母有标记。">
</picture>

- `Ctrl+Shift+P` 打开搜索面板，分五个区域：Folio 可执行的操作、当前窗口的窗格和标签页、执行过的命令、文件列所在文件夹下的文件、设置项。
- 输入文字同时筛选五个区域，方向键在结果间移动。`Enter` 可切换到窗格、在预览窗格打开文件、打开对应设置，或直接执行操作。
- 仍在运行的命令以窗格边框高亮指示，而非滚动到已过去的行。
- 文件搜索覆盖文件列当前所在的文件夹。

### Windows 集成

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/main-window-dark.png">
  <img src="docs/screenshots/main-window-light.png" width="100%"
       alt="默认字体和配色的窗口：左侧文件列，中间两个终端窗格并排，
       右侧预览窗格中打开了一个 Markdown 文档。">
</picture>

- **设置 > General > 资源管理器菜单**：打开时写入两个注册表键到 `HKEY_CURRENT_USER\Software\Classes`，加入「在 Folio 中打开」。在 Windows 11 上这项在「显示更多选项」页；在 Windows 10 上它在唯一的菜单中。如果 Windows 11 的文件夹里有 `folio.msix`，则同时注册该包到当前账户，使菜单项出现在第一页。无需管理员。关闭时移除已注册的项。正在运行的资源管理器只在启动时读取第一页的条目，如果「在 Folio 中打开」还没有出现，注销后重新登录。
- Windows PowerShell 5.1 自带的 PSReadLine 2.0.0 在窗口缩放后会错位输入行。Folio 附带修补版 2.4.6，可按需安装到用户模块目录。执行策略为 `Restricted` 时开关会提示，并给出对应的 `Set-ExecutionPolicy` 命令。

### Visual Studio Code

压缩包中 `folio.exe` 旁的 `folio-here.cmd` 只有一行：

```bat
@"%~dp0folio.exe" --cwd "%CD%"
```

在 VS Code 中将外部终端指向该文件——通过设置界面或 `settings.json`：

```json
"terminal.external.windowsExec": "C:\\Tools\\folio\\folio-here.cmd"
```

之后 **Terminal > Open in External Terminal**（`Ctrl+Shift+C`）即可在编辑器当前目录打开 Folio。该 `.cmd` 文件的作用是传入 `--cwd` 参数，因为此设置不会向程序传递参数。

---

## 隐私

Folio 不发送遥测、分析数据或崩溃报告。程序中没有模型和 API 密钥，它为你已有的 agent 提供终端环境。会访问网络的只有两项：在预览窗格中打开的网页，以及更新检查。

更新检查每天最多一次（所有窗口合计），向
`https://api.github.com/repos/lulu-loopp/folio-terminal/releases`
发送一次 `GET` 请求，`User-Agent` 仅为 `Folio`，不携带版本号、标识符或查询参数。检查结果仅用于在设置齿轮上标记和在设置中显示提示。关闭方式：设置 > 通用 > **检查更新**，或在 `settings.json` 中设置 `"update_check": false`。

数据存储在两个目录：`%APPDATA%\Folio`（设置、配置、配色方案、会话）和 `%LOCALAPPDATA%\Folio\WebView2`（网页预览的 Cookie 和缓存）。删除前者即恢复为全新状态。

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\Folio"
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\Folio\WebView2"
```

各文件的具体内容见 [`docs/PRIVACY.md`](docs/PRIVACY.md)。

## 已知问题

- **曾有一例报告窗口上半部全黑**，发生在移至第二显示器后，尚未复现。如遇到请附上 `%APPDATA%\Folio\diagnostics.log`。
- **`.webm` 需要从 Microsoft Store 安装 VP9 或 AV1 Video Extension**。Windows 默认未包含，缺少时无法播放。
- 其余已知问题见 [`CHANGELOG.md`](CHANGELOG.md)。

## 许可证

MIT 或 Apache-2.0，任选其一。所有依赖的许可证及其要求的声明见 [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)。

两份许可证授予版权和专利许可。Folio 名称和标识不在授权范围内——[`TRADEMARK.md`](TRADEMARK.md) 说明了对修改版分发的要求。

## 构建与贡献

从源码构建见 [`docs/BUILDING.md`](docs/BUILDING.md)；参与贡献见 [`CONTRIBUTING.md`](CONTRIBUTING.md)；安全问题请通过 [`SECURITY.md`](SECURITY.md) 中的私密渠道报告。

## 下一步

- 预览窗格中编辑 Markdown。
- macOS 和 Linux 支持。
- 从手机连接终端。

以上是方向，不是时间表。
