<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="assets/readme/hero-dark.svg">
  <img src="assets/readme/hero-light.svg" width="100%"
       alt="Folio——一个为编码 agent 做的 Windows 终端，哪个 agent 在等你，它会说。
       名字旁边是一个终端窗格，里面跑了两条命令；每条命令在左侧空白处各有一道竖线，
       标出它走到哪里，失败的那条用失败色画。">
</picture>

给同时开着好几个 AI 编码 agent 的人做的 Windows 终端。哪个 agent 在等你回话，它会说；
文件、预览、公式和 git 都在同一扇窗里。

[English](README.md) · [快捷键](docs/shortcuts.md) · [安全](SECURITY.md) ·
[更新记录](CHANGELOG.md)

> **预览版。** 0.1.0 是第一个公开版本，没有代码签名，见下面的
> [SmartScreen](#smartscreen)。

---

## 哪个 agent 在等你，这扇窗知道

同时跑三四个 agent，最耗神的不是它们干活的时候，是它们停下来问你的时候：你不知道该看
哪一个，只能一个标签一个标签点过去。

Folio 把这件事接了过来。一个窗格里的 agent 停下来等你，它所在的标签上就亮一个点。你人
在别的程序里，任务栏跟着闪；窗口最小化了、或者压根不在这个桌面上，才发一条系统通知，
通知里写的是那个程序自己的话。同一次等待最多打扰你一次。`Ctrl+Shift+A` 直接跳到等得最久
的那个窗格。

亮着的点不会因为你瞟一眼就灭：你在那个窗格里真的回了话，或者那个程序自己撤回，它才灭。
不然切一下标签，就把「还有人在等」这件事抹掉了。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/cards-dark.png">
  <img src="docs/screenshots/cards-light.png" width="100%"
       alt="标签条变成了一列卡片：这一张卡片代表一个标签，卡片里按这个标签自己的布局
       画着它的八个窗格；右边是同一个标签的八个终端窗格，各跑各的东西。">
</picture>

会话一多，标签条上就只剩一排名字了。`Ctrl+Shift+Z` 把标签条换成一列卡片，一张卡片就是
一个标签，卡片里按这个标签自己的布局画着它的每一个窗格。哪个标签里在发生什么，不用点
进去就看得见；再按一次收回去。

Claude Code、Codex、GitHub Copilot CLI 在设置的 Agents 页上各占一行开关。打开就往那个工具
自己的配置文件里写一个通知 hook——分别是 `~/.claude/settings.json`、`~/.codex/config.toml`
和 `~/.copilot/hooks/`——关掉就原样撤回。写之前先在旁边留一份带日期的原件副本，你不按，
它一个字都不写。Claude Code 和 Copilot CLI 报的是「我在等你回答」；Codex 只报「这一回合
完了」，它能说的就只有这个。

不用这三家也行：任何往终端里写 `OSC 1337;RequestAttention=yes` 的程序，什么都不装也听得见。

## 起点是想在终端里看见一个公式

这个软件是从一个很小的念头开始的：命令里打印出一个公式，我想看见公式，不想看见
`\frac{1}{2}` 这一串源码。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/preview-markdown-dark.png">
  <img src="docs/screenshots/preview-markdown-light.png" width="100%"
       alt="终端旁边的预览窗格里排着一份 markdown 文档：一个标题、一张表、一段代码，
       还有一个独立成行的求和公式。">
</picture>

现在命令输出里的数学会就地排出来，排不了的就按打印出来的原样显示。markdown 预览里的
`$…$`、`\(…\)`、`\[…\]` 和裸的 amsmath 环境走的是同一套排版（LaTeX 经 MiTeX 交给 Typst），
所以同一个公式在终端里和在文档里长得一样。

念头后来长出了别的东西。既然公式能开在终端旁边，文档也可以：markdown 连标题、表格、代码
一起排；PDF 一页一页翻；图片按原始像素或者缩到合适；`.mp4`、`.m4v`、`.webm` 就在窗格里
播；网页由 Windows 自带的 WebView2 承载，带地址栏和后退，本地 HTML 还能翻到源码那一面。
左边的文件列跟着当前窗格换目录，命令刚写出来的文件不用刷新就出现；同一列按 `Ctrl+Shift+G`
整列切成 git 面板，分支、工作区、暂存与未暂存、提交图都在，选中哪个文件，它的差异就出现
在预览里。

最顺手的是这一处：终端里打印出来的路径，点一下就在旁边开成预览；按住 `Ctrl` 再点，才交给
系统的默认程序。没做过标记的裸路径也认，前提是这个文件真的在。URL 同理：留在窗内还是
交出去，由你按不按 `Ctrl` 决定。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="assets/readme/surfaces-dark.png">
  <img src="assets/readme/surfaces-light.png" width="100%"
       alt="另外三个面：摊成卡片的一扇窗、排在预览窗格里的 markdown 文档，
       以及开在预览窗格里的一个网页，上面是它的地址栏。">
</picture>

## 原生、快、对中文认真

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/main-window-dark.png">
  <img src="docs/screenshots/main-window-light.png" width="100%"
       alt="Folio 窗口：左边是文件列，中间两个终端窗格并排，右边的预览窗格里开着一份
       markdown 文档。默认字体、默认配色。">
</picture>

Rust 写的，文字的排版和绘制都在 GPU 上。终端本身不是一张网页，底下也没垫着浏览器引擎；
整扇窗里只有网页预览用 WebView2，那还是 Windows 本来就有的那一个。

对中文认真不是一句口号。宽字符占几列、光标该停在哪、选区该切在哪，是这个项目最早花时间
的地方，有一份录下来的宽度用例逐字节守着它；微软拼音、微信输入法、搜狗三家在真机上逐项
敲过。搜狗有个已知的脾气：正在拼的那一串它不报给应用，所以拼音只画在它自己的候选窗里；
上屏的字是对的。

Windows PowerShell 5.1 自带的 PSReadLine 还是 2.0.0：把窗口拖窄，输入行会画回它原来的
位置，盖住已经挪走的文字。这是上游的老毛病，换个终端一样犯。Folio 把一份打过补丁的
2.4.6 编在自己身上，你点一下，它就装进你自己的模块目录。

装起来就是一个文件夹：解压出来就能跑，没有安装程序，没有遥测，自己不联网。

## 其它

每一个快捷键都能改；配色是一个文件夹里的文件，放进去就能选；一个进程管好几扇窗；标签和
窗格都能拖出去自成一扇窗，也能拖回来，而且是搬家不是关了重开，没被碰到的窗格宽度一个
像素都不变。内置 PowerShell 7、Windows PowerShell、WSL、Git Bash、命令提示符五条档案，
每一条都能逐字段改，也能自己加。设置里中英文随时切，不用重启。

---

## 怎么开始

### 下载

从 releases 页面取 `folio-0.1.0-windows-x64.zip`，解压到你放程序的地方，运行 `folio.exe`。
没有安装程序；在你运行它之前，解压目录之外不会被写入任何东西。压缩包旁边的
`SHA256SUMS.txt` 是它的哈希。

需要 64 位的 Windows 10 1809 及以上，或者 Windows 11。

网页预览要用 **WebView2 运行时**。Windows 11 自带；Windows 10 上通常也已经有了，没有的话
从[微软这里](https://developer.microsoft.com/microsoft-edge/webview2/)装一个 Evergreen
运行时。没有它，除网页预览之外一切照常，预览面会直说缺的是什么。

### SmartScreen

0.1.0 没有代码签名，所以第一次双击 `folio.exe` 可能会看到「Windows 已保护你的电脑」。
点「更多信息」，确认程序名是 `folio.exe`，再点「仍要运行」。不要为此关掉 SmartScreen。

### 第一次运行

第一个标签开的是你机器上排在最前的那个 shell。五条内置档案按 PowerShell 7、
Windows PowerShell、WSL、Git Bash、命令提示符的顺序找；机器上没装的那条在选择器里是灰的，
而不是被藏起来。

第一个 PowerShell 窗格打印出东西之后，窗格上会浮出一条提示，说 PowerShell 集成没装。点
「加进 `$PROFILE`」，它往你的 PowerShell 配置文件末尾加一行
`. "$env:APPDATA\Folio\shell-integration\folio.ps1"`，加之前先把原文件按日期另存一份放在
旁边；想撤销，删掉那一行就行。点「不再提示」就从此不问。把提示条直接关掉不算做了决定，
下一个 PowerShell 还会再问一次。装上之后才有命令标记：`Ctrl+Shift+↑` 和 `Ctrl+Shift+↓`
在命令之间跳，失败的命令会被标出来。Git Bash 和 WSL 不用你动手，磁盘上也不留东西。

Agents 页上那三行**默认不装**，而且它们不是「默认关」的开关——每次都去读对应工具自己的
配置文件，如实报告里面有什么。新机器上三个文件都不存在，所以三行都显示关。

---

## 你该放心什么

### 隐私

Folio 不向任何地方发送数据：没有遥测、没有统计、没有崩溃上报、没有更新检查，自己也没有
网络客户端。唯一联网的是你在网页预览里打开的那个页面。

它记住的东西分在两个目录：`%APPDATA%\Folio` 放设置、档案、配色和会话，
`%LOCALAPPDATA%\Folio\WebView2` 是网页预览自己的 profile，放 cookie 和缓存。删掉前一个，
Folio 就回到第一次启动的样子。

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\Folio"
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\Folio\WebView2"
```

哪个文件里装着什么、`session.json` 里为什么会留下完整的地址，都写在
[`docs/PRIVACY.md`](docs/PRIVACY.md)。

### 已知问题

- **没有签名。** 见上面的 SmartScreen。
- **`.mov` 不能播。** 它有首帧、时长和大小，窗格下面会写明为什么没有播放键。`.mkv`
  和 `.avi` 连预览都没有。
- **有过一次「窗口挪到副屏之后上半扇是黑的」的报告，没能复现。** 混合 DPI 的双屏本身在
  真硬件上量过。真撞上了，请附上 `%APPDATA%\Folio\diagnostics.log`。
- **资源管理器右键里的「在此处打开 Folio」在「显示更多选项」里面。** 要进第一层，程序得
  先签名。
- **`.webm` 的卡片可能没有画面。** 那一帧要靠机器上装着的解码器，出厂的 Windows 未必
  有 VP9 或 AV1；时长、大小和播放都不受影响。
- 其余的都记在 [`CHANGELOG.md`](CHANGELOG.md)。

### 许可

MIT 或 Apache-2.0，任选其一。每个依赖的许可与完整声明在
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)。

从源码构建见 [`docs/BUILDING.md`](docs/BUILDING.md)，参与开发见
[`CONTRIBUTING.md`](CONTRIBUTING.md)，安全问题走 [`SECURITY.md`](SECURITY.md) 里的私下
渠道，不要开公开 issue。
