<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="assets/readme/hero-dark.svg">
  <img src="assets/readme/hero-light.svg" width="100%"
       alt="Folio——会把数学排出来的 Windows 终端，还会告诉你哪个 agent 在等你。
       名字旁边是一个终端窗格：一条命令把一个文件打印了出来，文件里那个
       独立成行的公式——e 的负 x 平方在全实轴上的积分等于根号 π——就排在
       输出里，排在下一个提示符上面。">
</picture>

Folio 是一个 Windows 终端：命令打印出来的公式就地排版，命令提到的文件就在提示符旁边预览，哪个 agent 在等你回话，它会告诉你。

[English](README.md) · [快捷键](docs/shortcuts.md) · [安全](SECURITY.md) · [更新记录](CHANGELOG.md)

> **预览版。** 0.1.0 是第一个公开版本，没有代码签名，见下面的 [SmartScreen](#smartscreen)。

---

## 主要功能

### 命令输出里的 LaTeX 排版

命令打印出来的公式，就排在它打印的位置上。

- **终端里。** 命令输出中的 `$…$` 和 `$$…$$` 会就地排版，不用你复制到别的地方去看。
- **预览里。** 旁边的 markdown 预览除了这两种写法，还支持 `\(…\)`、`\[…\]` 和不带包裹的 amsmath 环境。
- **同一套排版。** 两处都是 LaTeX 经 MiTeX 交给 Typst，同一个公式在两处显示一致；排不出来的内容按打印出来的原样显示。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/terminal-math-dark.png">
  <img src="docs/screenshots/terminal-math-light.png" width="100%"
       alt="一个终端窗格，一条命令的输出就在打印的地方排好了版：几段文字里嵌着短的行内
       公式，上、中、下各有一条独立成行的公式——高斯归一化积分、傅里叶变换对，
       以及指数函数的级数。">
</picture>

### agent 的提醒与配置

哪个 agent 在等你回话，不用你挨个标签页去看。

- **三档提醒。** 它所在的标签页上会亮一个点；你人在别的程序里时，任务栏跟着闪；窗口最小化或者不在当前桌面时，才发一条 Windows 通知。
- **一次请求最多打扰你一次。** 点不会因为你看了一眼就消失，要你在那个窗格里回了话，或者那个程序自己撤回，它才消失。`Ctrl+Shift+A` 跳到等待时间最长的那一个。
- **三行开关。** 设置的 Agent 页上，Claude Code、Codex、GitHub Copilot CLI 各有一行开关：打开会往对应工具自己的配置文件里写一个通知 hook，关掉会原样撤回。Codex 报告的是一个回合结束，另外两个报告的是正在等你回答。
- **不装也能收到。** 任何往终端里写 `OSC 1337;RequestAttention=yes` 的程序，不装任何东西也听得见。

### 文件、PDF、视频与网页的预览

文件在提示符旁边就能看，不用切到别的程序。

- **悬停看一眼。** 鼠标停在文件列的文件名上会弹出一张卡片：PDF 的第一页、视频的第一帧、文本的头几行，图片则是图片本身；滚轮可以在卡片里翻页。
- **在旁边打开。** 文件开在提示符旁边的窗格里：markdown 排好版，PDF 一页一页翻，视频可以播放，网页带地址栏和后退。
- **点终端里的路径。** 终端打印出来的路径可以直接点开，开在旁边的窗格里；按住 `Ctrl` 再点，交给系统的默认程序。没有做过标记的裸路径也认，前提是这个文件确实存在。
- **看仓库状态。** `Ctrl+Shift+G` 把文件列切换成 git 面板：分支、工作区、暂存与未暂存的文件、提交图；选中哪个文件，它的差异就显示在预览里。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/preview-pdf-dark.png">
  <img src="docs/screenshots/preview-pdf-light.png" width="100%"
       alt="鼠标停在文件列的一个文件名上，底下弹出一张卡片，画着这份 PDF 的第一页，
       下面写着页数和大小；滚轮可以在卡片里一页页往下翻。">
</picture>

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="assets/readme/surfaces-dark.png">
  <img src="assets/readme/surfaces-light.png" width="100%"
       alt="另外三个面：摊成卡片的一扇窗、排在预览窗格里的 markdown 文档，
       以及开在预览窗格里的一个网页，上面是一条面包屑式的地址栏。">
</picture>

### 标签页、窗格与窗口的排布

一个标签页里可以并排放多个窗格，标签页和窗格都可以拖出来自成一扇窗口。

- **一屏看完所有会话。** `Ctrl+Shift+Z` 把标签条换成一列卡片，一张卡片就是一个标签页，卡片里按这个标签页自己的布局画出它的每一个窗格。
- **分屏。** 一个标签页里可以并排放终端、文件列和预览窗格，宽度自己拉。
- **拖出去，拖进来。** 标签页和单个窗格都可以拖出来自成一扇窗口，也可以拖进另一扇窗口；没有被碰到的窗格，宽度一个像素都不变。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/cards-dark.png">
  <img src="docs/screenshots/cards-light.png" width="100%"
       alt="标签条变成了一列卡片。这一张卡片代表一个装了八个窗格的标签，八个窗格都按
       原布局缩画在卡片里；按住 Alt 滚轮，可以一行一行地滚动卡片里的画面。">
</picture>

### Windows 上的细节

中文、多显示器和 PowerShell 这几件事，都在真机上逐项测过。

- **宽字符与输入法。** 一个字占几列、光标能停在哪、选区能切在哪，由一份录下来的宽度用例逐字节守着。微软拼音、微信输入法、搜狗三家在真机上逐项敲过；搜狗不把正在拼的那一串报给应用，所以拼音只画在它自己的候选窗口里，上屏的文字是对的。
- **多显示器。** 两块缩放不同的屏幕在真硬件上量过：窗口在两块屏之间拖动、最大化、往两个方向拉到超出屏幕边缘。
- **资源管理器右键菜单。** 「在此处打开 Folio」在「显示更多选项」下面。
- **PSReadLine。** Windows PowerShell 5.1 自带的还是 PSReadLine 2.0.0，它的编辑锚点取自窗口改变大小之前数出的格数，所以把窗口拖窄，输入行会画回原来的位置，盖住已经移走的文字。Folio 内置了一份打过补丁的 2.4.6，你让它装，它就装进你的模块目录。机器的执行策略还是出厂的 Restricted 时，开关会说明原因，并给出让模块能加载的命令 `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/main-window-dark.png">
  <img src="docs/screenshots/main-window-light.png" width="100%"
       alt="窗口的默认样子：左边是文件列，中间两个终端窗格并排，右边的预览窗格里开着
       一份 markdown 文档。默认字体、默认配色。">
</picture>

---

## 下载与第一次运行

### 下载

从 releases 页面下载 `folio-0.1.0-windows-x64.zip`，解压到你放程序的目录，运行 `folio.exe`。没有安装程序；在你运行它之前，解压目录之外不会被写入任何东西。`SHA256SUMS.txt` 是你下载到的文件的哈希。需要 64 位的 Windows 10 1809 及以上版本，或者 Windows 11。

网页预览需要 **WebView2 运行时**。Windows 11 自带；Windows 10 通常也有，如果没有，可以从[微软的下载页](https://developer.microsoft.com/microsoft-edge/webview2/)装一个 Evergreen 运行时。没有它，除网页预览之外的功能照常，预览窗格会写明缺的是什么。

### SmartScreen

0.1.0 没有代码签名，所以第一次运行可能会出现「Windows 已保护你的电脑」。点「更多信息」，确认上面写的程序名是 `folio.exe`，再点「仍要运行」。不要为此关掉 SmartScreen。

### 第一次运行

第一个标签页打开的是你机器上排在最前的那个 shell。五条内置 shell 配置按 PowerShell 7、Windows PowerShell、WSL、Git Bash、命令提示符的顺序查找；机器上没装的那条在选择器里是灰的，而不是被藏起来。

另有七条配置直接启动 agent：Claude Code、Codex、Copilot CLI、Kimi Code、pi、Hermes、OpenCode，同样只在 Windows 的 PATH 上查找。装在 WSL 里的 agent，从 WSL 配置启动，或者自建一条程序为 `wsl.exe -e <名字>` 的配置。

第一个 PowerShell 窗格打印出内容之后，会浮出一条提示，说 PowerShell 整合还没有装。**加进 `$PROFILE`** 会往你的 PowerShell 配置文件末尾追加一行 `. "$env:APPDATA\Folio\shell-integration\folio.ps1"`，追加之前先把原文件按日期另存一份放在旁边；删掉这一行就能撤销。**不再提示** 从此不再问。把提示条直接关掉不算做出决定，下一个 PowerShell 还会再问一次。

命令标记靠的就是这套整合：`Ctrl+Shift+↑` 和 `Ctrl+Shift+↓` 在命令之间跳转，执行失败的命令会被标出来。行内公式 `$…$` 也靠它与 shell 变量区分，所以没装的时候行内公式保持源码原样，`$$…$$` 块照常排版。Git Bash 和 WSL 不用装，磁盘上也不留东西。

设置的 Agent 页上那三行**默认不装任何东西**，它们也不是「默认关」的开关：每一行都去读对应工具自己的配置文件，如实显示里面有什么。新机器上三个文件都不存在，所以三行都显示为关。

---

## 隐私

Folio 不向任何地方发送数据：没有遥测、没有统计、没有崩溃上报、没有更新检查，自己也没有网络客户端。唯一联网的是你在网页预览里打开的那个页面。Folio 里没有模型，也没有 API key，它服务的是你自己已经在跑的 agent。

它记住的东西放在两个目录：`%APPDATA%\Folio` 放设置、配置文件、配色和会话，`%LOCALAPPDATA%\Folio\WebView2` 放网页预览的 cookie 和缓存。删掉前一个，Folio 就回到第一次启动的样子。

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\Folio"
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\Folio\WebView2"
```

哪个文件里存了什么、`session.json` 里为什么会留下完整的地址，都写在 [`docs/PRIVACY.md`](docs/PRIVACY.md)。

## 已知问题

- **没有签名。** 见上面的 [SmartScreen](#smartscreen)。
- **`.mov` 不能播。** 它有首帧、时长和大小，窗格里会写明为什么没有播放按钮。`.mkv` 和 `.avi` 连预览都没有。
- **有过一次「窗口挪到副屏之后上半扇是黑的」的报告，没能复现。** 真撞上了，请附上 `%APPDATA%\Folio\diagnostics.log`。
- **「在此处打开 Folio」不在 Windows 11 右键菜单的第一层。** 要进第一层，程序得先签名并打包。
- **`.webm` 的卡片可能没有画面。** 那一帧要靠机器上装着的解码器，出厂的 Windows 未必有 VP9 或 AV1；时长、大小和播放都不受影响。
- 其余的记在 [`CHANGELOG.md`](CHANGELOG.md)。

## 许可与商标

MIT 或 Apache-2.0，任选其一。每个依赖的许可，以及它们要求随附的声明，都在 [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)。

这两份许可给的是版权与专利许可，不包含其它内容。Folio 这个名字和标识不在其中，[`TRADEMARK.md`](TRADEMARK.md) 写明了这对修改后的分发意味着什么。

从源码构建见 [`docs/BUILDING.md`](docs/BUILDING.md)，参与开发见 [`CONTRIBUTING.md`](CONTRIBUTING.md)，安全问题请走 [`SECURITY.md`](SECURITY.md) 里的私下渠道，不要提交公开 issue。

## 后续方向

- **快捷键呼出的临时终端。** 按一个全局热键，从屏幕顶部滑下一扇独立的窗口；失去焦点或者按 Esc 收回，再呼出时内容还在。
- **预览窗格里的 markdown 编辑。** 所见即所得，边写边排版。
- **macOS 与 Linux。** 绘制、布局与排版这一层不区分平台，要换的是与系统打交道的那一层；先 macOS，再 Linux。
- **在手机上看谁在等你。** agent 仍然跑在电脑上，手机负责显示哪个在等你回话，并且能回它一句。默认是关的，你打开才有，而且先只走你自己的网络。

这些是方向，不是日期。
