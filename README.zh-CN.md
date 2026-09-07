<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="assets/readme/hero-dark.svg">
  <img src="assets/readme/hero-light.svg" width="100%"
       alt="Folio — 一个能排版公式、标出哪个 agent 在等你的 Windows 终端。名称旁边的终端窗格运行了一条打印文件的命令，文件中的展示公式——e 的负 x 平方在整条实数轴上的积分等于根号 π——排版在输出中，位于下一行提示符的上方。">
</picture>

Folio 是一个 Windows 终端：命令输出的公式直接排版在原处，文件在提示符旁边预览，agent 在等你时会标在标签页上。

[English](README.md) · [快捷键](docs/shortcuts.md) ·
[安全](SECURITY.md) · [更新记录](CHANGELOG.md)

> **预览版。** 0.2.2 是预览版本，由 Weiyi Shi 签名，详见下方[下载](#下载)。

---

## 下载

从[发布页](https://github.com/lulu-loopp/folio-terminal/releases)下载 [`folio-0.2.2-windows-x64.zip`](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.2.2-preview/folio-0.2.2-windows-x64.zip)，解压到你放程序的地方，运行 `folio.exe`。没有安装程序，运行前不会在解压目录外写任何文件。`SHA256SUMS.txt` 是下载文件的校验和。需要 **Windows 10 1809 或更新版本，或 Windows 11，64 位**。

`folio.exe` 和 `folio.msix` 由 **Weiyi Shi** 签名，证书来自 Microsoft Artifact Signing 服务。首次运行时 Windows 可能弹出**「Windows 已保护你的电脑」**：点击**「更多信息」**，再点**「仍要运行」**，显示的发布者是 **Weiyi Shi**。

压缩包内是一个文件夹，共九个文件，缺一不可：`folio.exe`；`conpty.dll` 和 `OpenConsole.exe`，缺少它们 shell 无法启动；`folio.msix`，几 KB 大小的包，用于在右键菜单第一页注册入口，包中记录了解压目录的路径；`folio-here.cmd`，供 VS Code 使用；以及两份许可证、第三方声明和商标说明。

网页预览需要 **WebView2 Runtime**。Windows 11 自带；Windows 10 通常也有，如果没有，可以从[这里](https://developer.microsoft.com/microsoft-edge/webview2/)安装 Evergreen Runtime。缺少它时，除网页预览外一切正常，预览窗格会提示缺少什么。

## 第一次运行

一台从未运行过 Folio 的机器会看到一张卡片，仅此一次。卡片标题是**欢迎使用 Folio**，列出六个问题，每个问题一行，回答它们会在 `%APPDATA%\Folio` 之外写文件：收到新版本通知（唯一默认开启的一行）；在右键菜单中添加「在 Folio 中打开」；PowerShell 整合；以及为本机实际安装了的 Claude Code、Codex、Copilot CLI 各亮一个标签页提示。主题、字体、窗口大小、语言、布局都不在卡片上——它们随时可以改，改之前也没有代价。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/first-run-dark.png">
  <img src="docs/screenshots/first-run-light.png" width="100%"
       alt="刚启动的窗口上方覆盖着一张卡片：Folio 图标旁写着「欢迎使用 Folio」，下面是六行，每行右侧有一个开关。「有新版本时通知你」默认开启；「在右键菜单中添加打开方式」和「PowerShell 整合」默认关闭；分隔线下方是三行 agent 提醒——Claude Code、Codex、Copilot CLI——全部关闭。底部有一行浅色文字「卡片上的每一行也是设置中的一行」，然后是「暂不」和「完成」两个按钮。卡片后面是一个标签页和一个提示符。">
</picture>

**把指针停在某一行上会看到具体说明**——包括这个开关会写你的哪个文件，以及写之前会先把原文件备份为带日期的副本。卡片上没有其他说明文字，因为其他行不会写你未被告知的地方。

**卡片上的每一行在设置中都有对应的行**，所以卡片上没有「错过就没有了」的选项。**完成**应用已开启的行；**暂不**和 `Esc` 以出厂值关闭卡片——更新检查开启，其余关闭——不改变任何东西。无论哪种方式，卡片都不会再出现，卡片后面的 shell 一直在运行。如果你在此版本之前就在用 Folio，不会看到这张卡片：你的 `settings.json` 已经记录了设置。

第一个标签页打开的是本机实际存在的第一个 shell。出厂带五个配置：PowerShell 7、Windows PowerShell、WSL、Git Bash、命令提示符，按此顺序查找。未安装的不会出现在启动 shell 的菜单里；它留在设置的配置页中，显示为灰色，并注明查找的程序路径。七个 agent 配置同样按 Windows PATH 查找，和卡片上 agent 行的查找方式相同。

PowerShell 整合会在 `$PROFILE` 中添加一行——`. "$env:APPDATA\Folio\shell-integration\folio.ps1"`——添加前会将 `$PROFILE` 原文件备份为带日期的副本；删除这一行即可还原。`$PROFILE` 的位置由 PowerShell 自身决定，所以卡片上留着开启的行会在下一次启动 PowerShell 时生效，在此之前设置 > 终端会提示尚未生效。如果你没有在卡片上被问到，第一次在 PowerShell 窗格中输出内容时会出现一条提示条：**添加到 `$PROFILE`** 执行操作，**不再提示**关闭询问，直接关掉提示条不做任何决定，下次打开 PowerShell 时会再问一次。命令标记和行内 `$…$` 公式依赖这个整合。Git Bash 和 WSL 不需要它，也不会在磁盘上留下任何文件。

设置中 Agent 页的三行**默认不安装任何东西**，也不是「碰巧关了的默认值」：每一行读取对应工具自身的配置文件，显示其中的内容。新机器上三个文件都不存在，所以三行都显示关闭。

---

## 主要功能

### 终端中的 LaTeX 排版

命令输出的 LaTeX 直接排版在打印的位置。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/terminal-math-dark.png">
  <img src="docs/screenshots/terminal-math-light.png" width="100%"
       alt="一个终端窗格，显示一条命令的排版输出：几段带有行内公式的文字，以及三个独占一行的展示公式——高斯归一化积分、傅里叶变换对和指数函数的级数展开。">
</picture>

- 命令输出中的 `$…$` 和 `$$…$$` 就地排版在命令打印它们的那一行。
- 预览窗格除了这两种，还支持 `\(…\)`、`\[…\]` 和裸 `amsmath` 环境。
- 两个场景共用一个排版引擎——LaTeX 经 MiTeX 送入 Typst。无法排版的内容按原样显示。
- 行内 `$…$` 需要下面的 PowerShell 整合来区分公式和 shell 变量。没有整合时，行内公式保留为源码，`$$…$$` 块仍然排版。

### 为 agent 而设计

等你回复的 agent 会标在它的标签页上，不需要切过去查看。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/settings-agents-dark.png">
  <img src="docs/screenshots/settings-agents-light.png" width="100%"
       alt="设置中的 Agent 页面：Claude Code、Codex 和 GitHub Copilot CLI 各一行，每行说明其开关会在哪个文件中写入通知钩子，三个都是关闭状态；末尾还有一行关于回合结束通知的设置。">
</picture>

- 等待中的 agent 在标签页上亮一个点；如果另一个程序在前台，任务栏会闪烁；如果窗口最小化或在其他桌面上，会弹出 Windows 通知。
- 一次请求最多打断你一次。你在那个窗格中回复或程序撤回请求后，提示点消失。`Ctrl+Shift+A` 跳到等待最久的 agent。
- Claude Code、Codex 和 GitHub Copilot CLI 各有一个开关，位于设置的 Agent 页。开关写入的是对应工具自身配置文件中的一个通知钩子，关闭时移除。默认不安装任何东西。
- 七个配置可以启动 agent——Claude Code、Codex、Copilot CLI、Kimi Code、pi、Hermes、OpenCode——通过 Windows PATH 查找；装在 WSL 里的从 WSL 配置启动。任何写 `OSC 1337;RequestAttention=yes` 的程序无需安装任何钩子即可被识别。

### 提示符旁边的预览：文件、PDF、视频、网页

文件的内容可以在提示符旁边直接查看，不需要打开其他应用。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/preview-pdf-dark.png">
  <img src="docs/screenshots/preview-pdf-light.png" width="100%"
       alt="指针停在文件列中的一个文件名上，下方弹出一张卡片，显示 PDF 的第一页，以及页数和文件大小。滚轮可以翻页。">
</picture>

- 指针停在文件列中的文件名上会弹出一张卡片：PDF 逐页显示，视频直接播放，文本显示前几行，图片直接显示。
- 预览窗格在提示符旁边打开文件——Markdown 排版显示，PDF 逐页浏览，视频播放，网页带地址栏和返回按钮。
- 终端输出的路径点击后在预览窗格中打开，`Ctrl`+点击交给系统默认应用。没有标记的裸路径也能识别，只要文件确认存在。
- 网址同理：点击在预览窗格中打开，`Ctrl`+点击交给浏览器。
- 一个窗口可以打开和预览窗格一样多的页面。第二个页面会在新的窗格中打开，而不是替换第一个，所以可以锁住一个页面同时在旁边打开另一个；把页面拖到某个窗格上，它就在那个窗格中打开。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="assets/readme/surfaces-dark.png">
  <img src="assets/readme/surfaces-light.png" width="100%"
       alt="另外三个界面：以卡片形式排列的窗口、预览窗格中排版的 Markdown 文档、预览窗格中的网页（上方有面包屑地址栏）。">
</picture>

### 窗格、标签页和窗口的移动

布局可以随时调整，里面的会话继续运行，所有内容可以同时看到。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/tab-into-pane-dark.gif">
  <img src="docs/screenshots/tab-into-pane-light.gif" width="100%"
       alt="一个标签页被按住并从标签栏向下拖出；在窗口右半部分出现落点预览，松开后标签页的 shell 变成右侧窗格，继续运行。然后把新窗格的标题拖到底边，并排布局变成上下两栏。">
</picture>

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/cards-dark.png">
  <img src="docs/screenshots/cards-light.png" width="100%"
       alt="标签栏变成了一列卡片。这张卡片代表一个包含八个窗格的标签页，以缩略图形式画出了全部八个；按住 Alt 滚动鼠标滚轮可以逐行滚动卡片内的画面。">
</picture>

- `Alt+Shift+-` 横向分割窗格，`Alt+Shift+=` 纵向分割。标签页或单个窗格可以拖出成为独立窗口，未动的窗格保持原有宽度。
- 窗格拖到两个标签页之间的接缝处会变成一个新标签页，插在它们中间：列表打开一个空位，窗格站在那里，松手时就在那里。拖到标签页本身则加入该标签页的布局。横向标签栏、纵向标签栏和卡片列都以相同方式识别接缝。
- `Ctrl+Shift+Z` 把标签栏切换为卡片列，每个标签页一张卡片，卡片上按实际布局绘制该标签页的窗格。
- `Ctrl+Shift+G` 把文件列切换为 Git 面板：分支、工作区、已暂存和未暂存的文件、提交图，以及选中文件的 diff 显示在预览窗格中。
- `Ctrl+Shift+↑` 和 `Ctrl+Shift+↓` 在滚动历史中按命令跳转，执行失败的命令会有失败标记。
- 文件列上方的文件夹按钮列出各 shell 当前所在的目录，然后是最近五个曾打开过的目录，各标注 `recent`。

### 快捷终端

一个按键把终端拉到屏幕顶部，同一个按键把它收回去。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/quake-dark.png">
  <img src="docs/screenshots/quake-light.png" width="100%"
       alt="一个终端窗口悬挂在屏幕顶部，略低于屏幕边缘，居中，覆盖在资源管理器窗口上方。终端有一个标签页、一个齿轮和一个关闭按钮，shell 已打印了四条 commit 和一个目录列表，下方是空的提示符。">
</picture>

- ``Win+` `` 在指针所在屏幕的顶部拉出一个终端，覆盖在当前内容上方。再按一次收起窗口，键盘焦点回到被覆盖的程序。
- 它就是完整的 Folio——标签页、窗格、文件列、预览、所有快捷键——并且在两次召唤之间保留 shell 和滚动历史。
- 手动移动或调整大小后，位置会按显示器记住，下次在该屏幕上召唤时出现在上次放置的位置。
- 它与 Folio 共存亡：没有独立的图标，按键背后没有后台进程。关掉最后一个可见窗口就结束整个程序。
- **设置 > 快捷终端**可以调整按键、新标签页的配置、高度、宽度、距屏幕顶部的间距、失去焦点时是否自动隐藏，以及每次启动首次召唤时执行的命令。
- 下次启动时，固定标签页的上一条命令可以恢复到提示符处——只是填入，不会执行。

### 搜索面板

一个输入框同时回答五个问题，`Enter` 直达目标。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/palette-dark.png">
  <img src="docs/screenshots/palette-light.png" width="100%"
       alt="窗口顶部浮着一个搜索框，输入了一个查询词，下方的结果分为五个互不混合的类别：一个操作、一个窗格、一条已运行的命令、一个文件和一项设置。第一行高亮，每行中匹配的字母有标记。">
</picture>

- `Ctrl+Shift+P` 在窗口顶部弹出搜索面板，分五个区域且互不混合：Folio 的操作、当前窗口的窗格和标签页、运行过的命令、文件列所在目录下的文件、设置项。
- 输入时五个区域同时筛选，方向键在结果间移动。`Enter` 选中窗格时跳到该窗格，选中文件时在预览窗格中打开，选中设置时打开设置并定位到该行，选中操作时执行该操作。
- 正在运行的命令用窗格外框的圆环指示其所在位置，而不是滚动到一个可能已经滚过去的行。
- 文件列表来自对文件列所在目录的索引，索引在窗口线程之外构建，深目录树不会让搜索框卡住。

### Windows 集成

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/main-window-dark.png">
  <img src="docs/screenshots/main-window-light.png" width="100%"
       alt="默认字体和配色方案下的窗口：左侧是文件列，中间是两个并排的终端窗格，右侧的预览窗格中打开了一篇 Markdown 文档。">
</picture>

- **设置 > 通用 > 资源管理器右键菜单**决定 Folio 在资源管理器右键菜单中的位置，有三个选项。*显示更多选项下*在 `HKEY_CURRENT_USER\Software\Classes` 下写入两个注册表项；菜单项显示「Open Folio here」，这也是 Windows 10 唯一的入口。*第一页*保留该入口，同时在 Windows 11 右键直接打开的第一页上添加「Open in Folio」，不需要点「显示更多选项」。第一页只接受已签名的包，所以这个选项会为当前账户注册 `folio.msix`——压缩包里 `folio.exe` 旁边的那个文件——不需要提权，不在账户外写任何东西，切回去就移除。在 Windows 10 上和 `folio.msix` 不在 `folio.exe` 旁边时，这个选项显示为灰色，原因写在行下方。
- Windows PowerShell 5.1 自带的 PSReadLine 2.0.0 在窗口调整大小后输入行会错位。Folio 附带修正版 2.4.6，按需安装到你的模块路径。如果执行策略还是出厂的 `Restricted`，开关会提示你，并给出解除限制的 `Set-ExecutionPolicy` 命令。

### Visual Studio Code

压缩包里 `folio.exe` 旁边有一个 `folio-here.cmd`，只有一行：

```bat
@"%~dp0folio.exe" --cwd "%CD%"
```

把 VS Code 的外部终端指向它——在设置中修改，或写入 `settings.json`：

```json
"terminal.external.windowsExec": "C:\\Tools\\folio\\folio-here.cmd"
```

**Terminal > Open in External Terminal**（`Ctrl+Shift+C`）会在编辑器当前目录下打开 Folio。这个 `.cmd` 文件存在的原因是该设置不传参数，而 `--cwd` 是告诉 Folio 启动位置的方式。

---

## 隐私

Folio 不向任何地方发送关于你的信息：没有遥测，没有分析，没有崩溃报告。程序中没有模型，没有 API key；它为你已经在运行的 agent 服务。两件事会访问网络：你在网页预览中打开的页面，以及更新检查。

更新检查是对 `https://api.github.com/repos/lulu-loopp/folio-terminal/releases` 的一次 `GET` 请求，所有窗口合计每天最多一次，`User-Agent` 为 `Folio`，不携带版本号、标识符或查询参数。它能做的只是在设置齿轮上画一个点、在设置中显示一行文字；不下载任何东西，不替换任何东西。在设置 > 通用 > **更新检查**中关闭，或在 `settings.json` 中设置 `"update_check": false`。

程序的数据保存在两个目录中：`%APPDATA%\Folio` 存放设置、配置、配色方案和会话，`%LOCALAPPDATA%\Folio\WebView2` 存放网页预览的 cookie 和缓存。删除前者，Folio 恢复到全新状态。

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\Folio"
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\Folio\WebView2"
```

每个文件的具体内容，以及为什么完整地址会出现在 `session.json` 中，见 [`docs/PRIVACY.md`](docs/PRIVACY.md)。

## 已知问题

- **曾有一次报告窗口移到第二块屏幕后上半部分全黑**，未能复现。如果遇到，请附上 `%APPDATA%\Folio\diagnostics.log`。
- **`.webm` 需要从 Microsoft Store 安装 VP9 或 AV1 Video Extension**。出厂的 Windows 两者都没有，缺少时没有预览画面也无法播放。
- 其余问题见 [`CHANGELOG.md`](CHANGELOG.md)。

## 许可

MIT 或 Apache-2.0，任选其一。每个依赖的许可证及其要求的声明在 [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) 中。

两份许可证授予的是著作权和专利许可，不涉及其他。Folio 名称和标识不在许可范围内——[`TRADEMARK.md`](TRADEMARK.md) 说明了这对修改后的分发意味着什么。

## 构建与参与

从源码构建见 [`docs/BUILDING.md`](docs/BUILDING.md)；[`CONTRIBUTING.md`](CONTRIBUTING.md) 说明如何提交修改；安全问题通过 [`SECURITY.md`](SECURITY.md) 中的私密渠道报告，不要开 issue。

## 后续方向

- 预览窗格中的 Markdown 编辑。
- macOS 和 Linux。
- 从手机上使用终端。

这些是方向，不是时间表。
