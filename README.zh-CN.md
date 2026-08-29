<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="assets/readme/hero-dark.svg">
  <img src="assets/readme/hero-light.svg" width="100%"
       alt="Folio——会把数学排出来的 Windows 终端，还会告诉你哪个 agent 在等你。
       名字旁边是一个终端窗格：一条命令把一个文件打印了出来，文件里那个
       独立成行的公式——e 的负 x 平方在全实轴上的积分等于根号 π——就排在
       输出里，排在下一个提示符上面。">
</picture>

一个 Windows 终端：命令打印出来的公式就地排版，它点到的文件就在旁边预览，哪个 agent
在等你回话它会说——这些都不用你切到别的窗口去。

[English](README.md) · [快捷键](docs/shortcuts.md) · [安全](SECURITY.md) ·
[更新记录](CHANGELOG.md)

> **预览版。** 0.1.0 是第一个公开版本，没有代码签名，见下面的
> [SmartScreen](#smartscreen)。

---

## Windows 上没有终端会排 LaTeX

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/terminal-math-dark.png">
  <img src="docs/screenshots/terminal-math-light.png" width="100%"
       alt="一个终端窗格，一条命令的输出就在打印的地方排好了版：几段文字里嵌着短的行内
       公式，上、中、下各有一条独立成行的公式——高斯归一化积分、傅里叶变换对，
       以及指数函数的级数。">
</picture>

Folio 会。命令输出里的 `$…$` 和 `$$…$$` 就排在命令打印它的地方；旁边的 markdown 预览
除了这两种，还认 `\(…\)`、`\[…\]` 和裸的 amsmath 环境。两边走的是同一套排版
（LaTeX 经 MiTeX 交给 Typst），所以同一个公式在哪边看都一样。排不出来的，按打印出来的
原样显示。

## 为什么是现在

如今在终端里说话的是 agent，而 agent 不用大白话回答你。它回的是 markdown：标题、表格、
公式，还有它刚改过的那些文件的路径。终端只会显示字符。于是回答落下来，是一份没人排版的
文档源码；它点到的文件，在另一扇窗后面。

下面这些，就是把这条缝合上。

## 这里没有一步需要你切走

- **想知道一个文件是什么**，不用打开它。鼠标停在文件列的名字上：PDF 的第一页、视频的
  第一帧、文本的头几行、图片本身。*(悬停卡片)*
- **想好好读它**，不用离开终端。它开在提示符旁边的窗格里——markdown 排好版，PDF 一页
  一页翻，视频直接播，网页带地址栏和后退。*(预览窗格)*
- **想跟着终端印出来的路径走**，不用再抄一遍。点一下就开在旁边；按住 `Ctrl` 再点，才
  交给系统的默认程序。没做过标记的裸路径也认，前提是这个文件真的在。
- **想一眼看完所有会话**，不用一个标签一个标签点过去。`Ctrl+Shift+Z` 把标签条换成一列
  卡片，一张卡片就是一个标签，卡片里按这个标签自己的布局画着它的每一个窗格。*(卡片)*
- **想知道哪个 agent 在等你**，不用专门去盯。它所在的标签上亮一个点；你人在别的程序
  里，任务栏跟着闪；窗口最小化了、或者不在这个桌面上，才发一条系统通知。同一次请求最多
  打扰你一次；点也不会因为你瞟一眼就灭，得你在那个窗格里真的回了话，或者那个程序自己
  撤回。`Ctrl+Shift+A` 跳到等得最久的那个。
- **想看仓库现在什么样**，不用另开一个 git 客户端。`Ctrl+Shift+G` 把文件列整列切成 git
  面板：分支、工作区、暂存与未暂存、提交图；选中哪个文件，它的差异就出现在预览里。
- **想把一个窗格挪个地方**，不用关掉再开一个。拖着标签或者单个窗格出去自成一扇窗，也能
  拖进另一扇窗；没被碰到的窗格，宽度一个像素都不变。*(浮窗)*

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/preview-pdf-dark.png">
  <img src="docs/screenshots/preview-pdf-light.png" width="100%"
       alt="鼠标停在文件列的一个文件名上，底下弹出一张卡片，画着这份 PDF 的第一页，
       下面写着页数和大小；滚轮可以在卡片里一页页往下翻。">
</picture>

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/cards-dark.png">
  <img src="docs/screenshots/cards-light.png" width="100%"
       alt="标签条变成了一列卡片。这一张卡片代表一个装了八个窗格的标签，八个窗格都按
       原布局缩画在卡片里；按住 Alt 滚轮，可以一行一行地滚动卡片里的画面。">
</picture>

Claude Code、Codex、GitHub Copilot CLI 在设置的 Agents 页上各占一行开关，打开就往那个
工具自己的配置文件里写一个通知 hook，关掉就原样撤回。Codex 报告的是回合结束，另外两个
报告的是「我在等你回答」。不装也行：任何往终端里写 `OSC 1337;RequestAttention=yes` 的
程序都听得见。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="assets/readme/surfaces-dark.png">
  <img src="assets/readme/surfaces-light.png" width="100%"
       alt="另外三个面：摊成卡片的一扇窗、排在预览窗格里的 markdown 文档，
       以及开在预览窗格里的一个网页，上面是一条面包屑式的地址栏。">
</picture>

## Windows 上的细活

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="docs/screenshots/main-window-dark.png">
  <img src="docs/screenshots/main-window-light.png" width="100%"
       alt="窗口的默认样子：左边是文件列，中间两个终端窗格并排，右边的预览窗格里开着
       一份 markdown 文档。默认字体、默认配色。">
</picture>

宽字符是这个项目最早花时间的地方：一个字占几列、光标能停在哪、选区能切在哪，有一份录
下来的宽度用例逐字节守着。微软拼音、微信输入法、搜狗三家在真机上逐项敲过；搜狗不把正在
拼的那一串报给应用，所以拼音只画在它自己的候选窗里，上屏的字是对的。

两块缩放不同的屏在真硬件上量过：窗口在两块屏之间拖、最大化、往两个方向拉到超出屏幕。

资源管理器的右键菜单里有「在此处打开 Folio」，在「显示更多选项」下面。

Windows PowerShell 5.1 自带的 PSReadLine 还是 2.0.0：它的编辑锚点是拖动之前数出来的
格数，所以把窗口拖窄，输入行会画回原来的位置，盖住已经挪走的文字。Folio 把一份打过补丁
的 2.4.6 编在自己身上，你点一下，它就装进你的模块目录。

---

## 怎么开始

### 下载

从 releases 页面取 `folio-0.1.0-windows-x64.zip`，解压到你放程序的地方，运行
`folio.exe`。没有安装程序；在你运行它之前，解压目录之外不会被写入任何东西。
`SHA256SUMS.txt` 是它的哈希。需要 64 位的 Windows 10 1809 及以上，或者 Windows 11。

网页预览要用 **WebView2 运行时**。Windows 11 自带；Windows 10 上通常也有了，没有的话从
[微软这里](https://developer.microsoft.com/microsoft-edge/webview2/)装一个 Evergreen
运行时。没有它，除网页预览之外一切照常，预览面会直说缺的是什么。

### SmartScreen

0.1.0 没有代码签名，所以第一次双击 `folio.exe` 可能会看到「Windows 已保护你的电脑」。
点「更多信息」，确认程序名是 `folio.exe`，再点「仍要运行」。不要为此关掉 SmartScreen。

### 第一次运行

第一个标签开的是你机器上排在最前的那个 shell。五条内置 shell 配置按 PowerShell 7、Windows
PowerShell、WSL、Git Bash、命令提示符的顺序找；机器上没装的那条在选择器里是灰的，而不是
被藏起来。

第一个 PowerShell 窗格打印出东西之后，窗格上会浮出一条提示，说 PowerShell 整合没装。点
「加进 `$PROFILE`」，它往你的 PowerShell 配置文件末尾加一行
`. "$env:APPDATA\Folio\shell-integration\folio.ps1"`，加之前先把原文件按日期另存一份放在
旁边；想撤销，删掉那一行就行，文件里别的一个字都没动过。点「不再提示」就从此不问；把
提示条直接关掉不算做了决定，下一个 PowerShell 还会再问一次。装上之后才有命令标记：
`Ctrl+Shift+↑` 和 `Ctrl+Shift+↓` 在命令之间跳，失败的命令会被标出来。行内公式 `$…$`
也靠它与 shell 变量区分开，所以没装时行内公式保持源码，而 `$$…$$` 块照常排版。Git Bash
和 WSL 不用你动手，磁盘上也不留东西。

Agent 页上那三行**默认不装**，而且它们不是「默认关」的开关——每次都去读对应工具自己的
配置文件，如实报告里面有什么。新机器上三个文件都不存在，所以三行都显示关。

---

## 你该放心什么

### 隐私

Folio 不向任何地方发送数据：没有遥测、没有统计、没有崩溃上报、没有更新检查，自己也没有
网络客户端。唯一联网的是你在网页预览里打开的那个页面。

它记住的东西分在两个目录：`%APPDATA%\Folio` 放设置、配置文件、配色和会话，
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
- **有过一次「窗口挪到副屏之后上半扇是黑的」的报告，没能复现。** 真撞上了，请附上
  `%APPDATA%\Folio\diagnostics.log`。
- **「在此处打开 Folio」不在 Windows 11 右键菜单的第一层。** 要进第一层，程序得先签名。
- **`.webm` 的卡片可能没有画面。** 那一帧要靠机器上装着的解码器，出厂的 Windows 未必
  有 VP9 或 AV1；时长、大小和播放都不受影响。
- 其余的都记在 [`CHANGELOG.md`](CHANGELOG.md)。

### 许可

MIT 或 Apache-2.0，任选其一。每个依赖的许可与完整声明在
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)。

双许可给的是版权与专利许可，不含别的。Folio 这个名字与标识不在其中——
[`TRADEMARK.md`](TRADEMARK.md) 写了这对改版分发意味着什么。

从源码构建见 [`docs/BUILDING.md`](docs/BUILDING.md)，参与开发见
[`CONTRIBUTING.md`](CONTRIBUTING.md)，安全问题走 [`SECURITY.md`](SECURITY.md) 里的私下
渠道，不要开公开 issue。

---

## 接下来

**一个只管看桌面的帮手。** 每个标签在做什么、谁在等你，这扇窗已经看得见；下一步是让它
替你记账——现在开着哪些事、还欠着什么、上次做到哪。它只做整理，不接活；代码还是你已经
在用的那个 agent 写。

**对着窗说话。** 说出来的话，就是往窗格里打的字。

**从手机连回这台电脑。** agent 在电脑上跑，手机上看谁在等你、顺手回它一句。默认是关
的，你打开才有，而且先只走你自己的局域网——上面隐私那一节，在你亲手打开之前一直成立。

这些是方向，不是日期。

## 它不是什么

**不是一个更快的终端。** 画在 GPU 上、说 ConPTY，这是入场的门票，不是它的主张。

**不是替你写代码的 AI。** 这里面没有模型，也没有 API key。它服务你已经在用的那个
agent；你一个都不用，它照常是个终端。

**不跨平台。** Windows 是选择，不是起点。ConPTY、资源管理器菜单、WebView2、每显示器
DPI、PSReadLine——这几样，只有认准一个平台才做得干净。
