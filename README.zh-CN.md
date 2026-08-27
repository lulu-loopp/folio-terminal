# Folio

一个 Windows 终端。用 Rust 写，画在 GPU 上，带文件列、文档预览、git 面板，还能让命令行里的
agent 告诉你它在等你回话。

[English](README.md) · [快捷键](docs/shortcuts.md) · [安全](SECURITY.md) ·
[更新记录](CHANGELOG.md)

> **这是预览版。** 0.1.0 是第一个公开版本，**没有代码签名**——见下面的
> [SmartScreen](#smartscreen) 一节。

<!-- 截图在真机上拍，提交到 docs/screenshots/。要拍哪几张、怎么拍，见
     docs/screenshots/README.md。 -->

![Folio 窗口：左边是文件列，中间两个终端窗格并排，右边的预览窗格里开着一份
markdown 文档。](docs/screenshots/main-window-light.png)

---

## 它有什么

### 终端

窗格可以横着拆也可以竖着拆，标签装着它们；一个标签或一个窗格可以拖出去自成一扇窗，
也可以拖进另一扇窗。文字在 GPU 上排版和绘制，连字与彩色 emoji 都在内；每个窗格各有各的
回滚。

Shell 来自档案。内置五条：PowerShell 7、Windows PowerShell、WSL、Git Bash、命令提示符；
你也可以自己加，各带各的命令行、环境变量和配色。

### 命令标记

只要 shell 肯说，Folio 就知道每一段提示符、每一条命令、每一段输出各自从哪里开始到哪里
结束。`Ctrl+Shift+↑` 与 `Ctrl+Shift+↓` 在命令之间跳，失败的命令会被标出来。

Git Bash 与 WSL 是自动装好的：脚本只在命令行上指给这一个交互式 shell，磁盘上什么都不动。
PowerShell 没有对应的办法，所以第一次见到 PowerShell 窗格时 Folio 会问你要不要往
`$PROFILE` 里加一行；你不点，它就不加。

### 文件列

终端旁边的一列显示当前窗格所在的目录，跟着这个窗格一起换目录；它还盯着自己正在显示的
那几个文件夹，所以命令刚写出来的文件不用手动刷新就会出现。文件夹和文件可以收藏、打开、
改名、在资源管理器里定位，也可以就地开一个新的终端窗格。

### 预览

![悬停在文件列的一个文件名上弹出的卡片，画着这份 PDF 的第一页，下面写着页数和
大小。](docs/screenshots/preview-pdf-light.png)

从文件列打开、点终端里印出来的路径打开、或者从命令行带路径启动，文件都开在终端旁边，
而不是交给另一个程序：

- **Markdown**：排版后的样子——标题、表格、代码，`$…$` 里的数学由终端里画公式的同一套
  引擎排出来。
- **PDF**：一页一页翻。
- **图片**：按原始像素，或者缩到合适。
- **视频**：`.mp4`、`.m4v`、`.webm` 给出首帧、时长和大小。现在还不能播。
- **网页**：本地的或远端的，由 Windows 自带的 WebView2 引擎承载，带地址栏、后退，本地页
  还能翻到源码那一面。
- **其它一切**：按文本显示。

在文件列里把鼠标停在文件名上，同样的东西会先以一张小卡片出现，不用真的打开。

### Git

在一个仓库里，文件列可以整列切成 git 面板：分支、工作区、暂存与未暂存的文件、提交图；
选中某个文件，它的差异就出现在预览里。

### Agents

![设置里的 Agents 页，三行安装开关分别对应 Claude Code、Codex 与 Copilot
CLI。](docs/screenshots/settings-agents-light.png)

命令行里的 agent 停下来问你话时，可以给自己所在的窗格做个记号，于是一扇开满窗格的窗会
告诉你是哪一个在等。Claude Code、Codex 与 GitHub Copilot CLI 各在设置的 Agents 页上占一行
开关：打开就往那个工具自己的配置文件里写入一个通知钩子，关掉就原样撤回。**你不按，它就
不写。** 至于那些直接往终端里写注意力序列的程序，不装任何东西也听得见。

### 卡片

`Ctrl+Shift+Z` 把当前标签里的窗格摊成一张张卡片，每张按它自己该有的大小画，于是八个
窗格一眼能读完。再按一次收回去。

### 设置

一个对话框，中英文都有：字体、配色、光标、回滚行数、透明度、最小对比度、预览渲染哪些
东西、shell 档案，以及每一个快捷键。配色是一个文件夹里的文件，你可以往里加。

---

## 下载与安装

1. 从 releases 页面取 `folio-0.1.0-windows-x64.zip`。
   <!-- 第一次打 tag 之后把链接填进来。 -->
2. 解压到你放程序的地方。没有安装程序；在你运行它之前，解压目录之外不会被写入任何东西。
3. 运行 `folio.exe`。

压缩包里是 `folio.exe`、它启动 shell 用的两个 ConPTY 文件（`conpty.dll` 与
`OpenConsole.exe`）、这份说明、两份许可证原文，以及第三方声明。压缩包旁边的
`SHA256SUMS.txt` 是你下到的那份文件的哈希。

**需要 64 位的 Windows 10 1809 及以上，或 Windows 11。**

**WebView2 运行时**：网页预览和本地 HTML 预览要用它。Windows 11 自带；Windows 10 上通常
也已经有了，如果没有，从
<https://developer.microsoft.com/microsoft-edge/webview2/> 装 Evergreen 运行时。没有它，
除网页预览之外一切照常，预览面会直说缺的是什么，而不是默默不动。

**Visual C++ 运行库**：发布前确认。
<!-- 等静态链接 C 运行库的实验在干净机器上量完再填：要么写「不需要，运行库已静态链入」，
要么给微软可再发行组件的链接。这一条不许猜。 -->

---

## SmartScreen

Folio 0.1.0 **没有代码签名**。2026 年的行情是：签名已经买不到「第一次运行不弹窗」——微软
在 2024 年取消了 EV 证书的即时放行；而本项目打算申请的那个开源签名计划，要求项目**先发布
过**。所以第一个版本不签，下面是不签的样子。

用浏览器下载的文件会带一个「来自互联网」的标记，Windows 资源管理器解压时会把这个标记
传给解出来的文件。所以你双击 `folio.exe` 时，**可能**会看到一个标题为「Windows 已保护你的
电脑」的对话框，上面只有一个「不运行」按钮。

要继续运行：点左下角的「更多信息」，确认程序名是 `folio.exe`——发布者那一行会写「未知
发布者」，这正是「没签名」的意思——然后点「仍要运行」。

想确认拿到的就是发布出来的那份，先用 `SHA256SUMS.txt` 校验压缩包。不要为此关掉
SmartScreen，也不要听信「换个解压工具就没提示了」这类办法：那个标记在这里做的正是它该做
的事。

---

## 第一次运行

**窗口**开在 960 × 600，位置由 Windows 自己决定放在哪个角，**不居中**。你挪过、调过一次
之后，每扇窗各自的大小和位置就会被记住。

**第一个 shell。** 内置五条档案，按这个顺序找：PowerShell 7（`pwsh`）、Windows
PowerShell（`winps`）、WSL（`wsl`）、Git Bash（`gitbash`）、命令提示符（`cmd`）。机器上
没装的那条在选择器里是灰的，而不是被藏起来。在你于设置里挑定一条默认档案之前，第一个
标签开的是 **Windows PowerShell**——那是 Windows 上永远在场的一条。

**PowerShell 整合的提示条。** 第一个 PowerShell 窗格打印出东西之后，窗格上会浮出一条：

> PowerShell 整合未安装。Folio 用它来标记命令与显示状态。

- **加进 `$PROFILE`**：往你当前宿主的 PowerShell 配置文件末尾追加一行——
  `. "$env:APPDATA\Folio\shell-integration\folio.ps1"`——追加之前先把原文件按当天日期
  另存一份 `<配置文件名>.bak-<日期>` 放在旁边。这一行指向的脚本写在
  `%APPDATA%\Folio\shell-integration\` 里。想撤销，删掉那一行就行，文件里别的内容一个字
  都没动过。
- **不再提示**：从此不再问。
- 直接把提示条关掉不算做了决定，所以下一个 PowerShell 还会再问一次。

**Agent 的钩子默认不装。** Agents 页上那三行不是「默认关」的开关，而是每次都去读对应工具
自己的配置文件、如实报告里面有什么。新机器上三个文件都不存在，所以三行都显示关，而且
Folio 从不主动去写。按下某一行就当场写入，写之前先留一份带日期的副本；再按一次，写进去
的东西原样取出来。三个文件分别是哪一个、写进去的是什么，见 `SECURITY.md`。

---

## 快捷键

一共四十行，全都能在设置的快捷键页里改。
**[完整的表在 `docs/shortcuts.md`](docs/shortcuts.md)**，中英各一份，由源码生成而不是手写。

先记这几个就够用：

| 按键 | 作用 |
| --- | --- |
| `Ctrl+Shift+N` | 新建标签 |
| `Ctrl+Shift+W` | 关闭窗格 |
| `Alt+Shift+-` / `Alt+Shift+=` | 横向拆分 / 竖向拆分 |
| `Ctrl+Tab` | 下一个标签 |
| `Ctrl+Shift+B` | 文件列 |
| `Ctrl+Shift+G` | 文件列切到 Git |
| `Ctrl+F` | 在本窗格中查找 |
| `Ctrl+Shift+Z` | 卡片 |
| `Ctrl+,` | 设置 |

---

## 隐私

Folio 不向任何地方发送数据。没有遥测、没有统计、没有崩溃上报、没有更新检查，自己也没有
网络客户端。唯一联网的是你在网页预览里打开的页面，由 Windows 自带的 WebView2 引擎抓取。

Folio 记住的一切都在本机，分在两个目录里。

### `%APPDATA%\Folio` —— 设置与会话

漫游配置。删掉它，Folio 就回到第一次启动的样子。

| 文件 | 内容 |
| --- | --- |
| `settings.json` | 你的设置。 |
| `keybindings.json` | 你改过的快捷键。改过才会写。 |
| `profiles.json` | Shell 档案，含你填的命令行与环境变量。 |
| `schemes\` | 你添加的配色。 |
| `session.json`、`session.lock` | 待恢复的窗口、标签与窗格。见下。 |
| `pins.json` | 收藏的文件夹、文件与地址。 |
| `shell-integration\` | Folio 为 PowerShell 与 bash 整合写出的脚本。 |
| `diagnostics.log`、`diagnostics.prev.log` | 无控制台启动时的程序输出。只在启动时查一次：到 4 MiB 就把当前这份转成 `.prev.log`，顶掉上一代。 |
| `hang-reports\` | 只在窗口失去响应时写。记模块名与偏移，不记栈内容。 |

```powershell
# Folio 记住的全部设置与会话
Remove-Item -Recurse -Force "$env:APPDATA\Folio"

# 只清诊断
Remove-Item -Force "$env:APPDATA\Folio\diagnostics*.log", "$env:APPDATA\Folio\hang-reports" -Recurse -ErrorAction SilentlyContinue
```

### `%LOCALAPPDATA%\Folio\WebView2` —— 网页预览的 profile

这是另一个目录，不是上面那个。它是 WebView2 引擎给预览用的 profile：cookie、本地存储、
磁盘缓存，以及自动填充会用的 profile 目录。放在 local 而不是 roaming，是为了让缓存和
cookie 不跟着账户在机器之间跑。Folio 不会删它。

这个 profile 里的自动填充与密码保存是**关**的——不是留给引擎的默认值——所以你在预览页面
里填的表单不会存进去。cookie 和缓存照常存，和任何浏览器一样。

```powershell
# 清掉预览的 cookie、存储与缓存。先关掉 Folio。
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\Folio\WebView2"
```

### `session.json` 与 `pins.json` 里有什么

两份都是明文，不加密，任何以你的身份运行的程序都能读。这跟你的 shell 历史是一个待遇；
之所以值得说，是因为里面有这些：

- **完整地址**，含 query 与 fragment。你预览过一个带 token 的 URL，那个 token 就在文件里。
- 每个终端窗格的**工作目录**。
- **文件与目录路径**——每个文件列的根、展开过哪些、选中的是哪个，以及预览过的每个文件的
  路径。
- **你手动给窗格起的名字。**
- 预览过的**页面标题**。
- 最近关闭标签的**时间戳**。
- 你在提交图里筛选用的 **git 分支名**。
- **窗口几何、每显示器 DPI，以及一个显示器标识。**

`profiles.json` 里有你写进档案的命令行与环境变量。不要往里面放密钥。

### 其它位置

- `%TEMP%\bt-app-panic.log` —— Folio 崩溃时追加。
- 三个 hooks 安装器打开时，各写一个文件到 Claude Code、Codex、Copilot 自己的配置目录里；
  写之前先在旁边留一份带日期的原件副本。细节见 `SECURITY.md`。
- 若干 `BT_*` 环境变量会让 Folio 把终端内容写到你指定的文件——`BT_PTY_DUMP` 写的是每个
  窗格的每一个字节。你不设，它们就都不生效。全部列在 `docs/BT-ENVIRONMENT.md`。

`SECURITY.md` 写了这些机制各自的边界，以及怎么私下报告漏洞。

---

## 已知问题

- **没有签名。** 见 [SmartScreen](#smartscreen)。
- **资源管理器右键里的「在此处打开 Folio」在「显示更多选项」里面**，不在 Windows 11 右键
  菜单的第一层。要进第一层，程序得带包身份并且签过名，所以这一条排在签名之后。
- **视频不能播。** `.mp4`、`.m4v`、`.webm` 有首帧、时长和大小；`.mov`、`.mkv`、`.avi`
  三样都没有——引擎不肯播它们，所以也不给它们一份兑现不了的预览。
- **`.webm` 的卡片可能是空的。** 那一帧要靠你机器上装着的解码器，而一台出厂 Windows 未必
  有 VP9 或 AV1 的解码器。时长和大小照常显示。
- **第一扇窗不居中**，开在 960 × 600。
- **混合 DPI 与多显示器的路径测得薄。** 窗口按每个显示器各自问 Windows 要缩放、移动时
  重新光栅化，但写这些代码的机器只有一块屏。此前有一次「窗口上半黑」的报告，查下来
  原因在别处，不是 DPI 缺陷；只是这几条路径确实没在真硬件上跑过。
- **有些公式实时渲染不出来。** `\hbar` 一类的输入会退回按打印出来的原文显示。
- **换宽一点的字体，一张卡片能放下的列数会变少。** 卡片能显示多少列是按你正在用的字体
  算出来的，默认字体下读得清的卡片换一种字体可能就窄了。
- **不装 PowerShell 整合**，命令标记、命令状态，以及「程序退出后鼠标模式没收回」的那个
  修复，三样都不生效。

---

## 从源码构建

需要 [rustup](https://rustup.rs/) 与 MSVC 工具链（Visual Studio 生成工具的 C++ 工作
负载）。别的都不用——`rust-toolchain.toml` 钉死了编译器版本，rustup 会在第一次构建时把它
装上。

```powershell
git clone <本仓库>
cd folio
cargo build --release
```

产物是 `target\release\folio.exe`。想直接从检出目录跑，用 `cargo run --release`。

每一次改动都要过的三道门，也是 CI 跑的那三条：

```powershell
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

`cargo` 默认会用光所有核。如果你还要在同一台机器上干别的，用 `cargo build -j 4`，或者在
你自己的 cargo 配置里写一个 `jobs`，别让它把机器占满。

`vendor/alacritty_terminal` 是打过补丁的上游 VT 引擎，作为工作空间的一员一起构建；
和上游的每一处差异都记在 `vendor/alacritty_terminal/CHANGES-FOLIO.md` 里。
`vendor/conpty/` 放的是随压缩包一起发出去的那两个 ConPTY 文件。

---

## 许可

MIT 或 Apache-2.0，任选其一。原文在 [`LICENSE-MIT`](LICENSE-MIT) 与
[`LICENSE-APACHE`](LICENSE-APACHE)。

Folio 站在别人的工作上。每一个依赖的许可，以及那些要求原样传播的完整声明，都在
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) 里——它由锁文件生成，并有一道门盯着
它别和锁文件对不上。

---

## 参与开发

[`CONTRIBUTING.md`](CONTRIBUTING.md) 写了三道门、测试是干什么用的，以及设计决定记在哪里。
安全问题走 [`SECURITY.md`](SECURITY.md) 里的私下渠道，不要开公开 issue。
