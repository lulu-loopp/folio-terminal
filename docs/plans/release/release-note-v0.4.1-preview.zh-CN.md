> 本版本 GitHub Release 正文的中文版。

# Folio 0.4.1

**下载：**[zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.1-preview/folio-0.4.1-windows-x64.zip)（Windows 10 1809 及以上 / Windows 11，64 位）· [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.1-preview/Folio-0.4.1-macos-arm64.dmg)（macOS 14 及以上，Apple 芯片）

**下载：** 上方 zip 与 dmg 即为完整下载，其余为校验和、物料清单与源码。[English release note](https://github.com/lulu-loopp/folio-terminal/blob/main/docs/plans/release/release-note-v0.4.1-preview.md)

## 亮点

- 粘贴在资源管理器或访达中复制的文件，成为一个带引号的参数，路径按该窗格 shell 的写法拼出。
- 不由按键触发的文字——来自手机键盘、听写或代替你输入的程序——送达你正在输入的位置。
- 窗格大量输出时键入保持流畅，输出期间搜索不再暂停，点击齿轮后设置立即打开，无论装了多少字体。新增的关于页标明当前 Folio 的版本，以及在哪里阅读说明、报告缺陷或查看许可证。
- 排版后的公式有独立的块和四周留白，在排版图和源码之间切换时平滑缩放到位而非跳变。
- 在 Mac 上，空闲窗口不再占用一个处理器核心，没有窗口时可以退出 Folio，文件列中的替身打开它指向的内容而非执行它，预览的页面只在确实受限时才标记为受限。

## 变更

### 新增

- **设置新增关于页**，标明版本和构建来源、适用的系统，并提供发布说明、缺陷报告和 Folio 所有依赖许可证的入口。
- **复制的文件或路径粘贴为一个带引号的参数**，路径按该窗格 shell 的写法拼出。

### 改动

- **排版后的公式有独立的块和四周留白**——上下空行、两侧各留出一列空间，两个标记位于右侧边缘之内。
- **公式在排版图和源码之间切换时带有动画**，不再跳变；减少动效时仍然在一帧内完成。
- **发布页同时提供固定名称的下载**，指向最新构建的链接不会过期。
- **校验和文件可直接在下载文件夹中验证**，无需事先编辑。
- **两个文件移出仓库顶层**；引用其路径的 fork 同步更新了路径。
- **编译门统一了编译内容**，门通过后运行的脚本无需再编译。

### 修复

- **窗格大量输出时键入保持流畅。**构建日志或长篇回答滚动时不再让窗口卡住数秒。
- **搜索框打开时，窗格输出期间键入不再暂停**，历史记录不会被从头搜索。
- **在 Mac 上，空闲窗口不再占用一个处理器核心。**
- **在 Mac 上，被遮挡或隐藏的窗口不再以全速绘制**，隐藏期间准备的图像不再堆积在 GPU 上。
- **手机键盘发送的文字或代替你输入的程序现在送达终端**，落在你正在输入的位置。
- **打开设置不再冻结窗口**，无论装了多少字体。
- **复制公式不再让窗口持续忙碌**直到关闭。
- **将公式切换为源码不再卡顿**，即使背后有长历史。
- **宏无限展开的公式被拒绝**，而非耗尽内存。
- **公式的两个标记跟随公式**，切换标签页、关闭窗格或指向非焦点窗格时不再丢失。
- **用尖括号书写的内积和 bra-ket 现在正确排版。**
- **用滚轮定位卡片窗口时跟得上手速**，即使窗格有长历史。
- **在 Mac 上，没有窗口打开时可以退出 Folio。**
- **在 Mac 上，从文件列打开替身时打开它指向的内容**，指向程序的替身被拒绝。
- **在 Mac 上，预览的页面只在自身确实带有限制规则时才标记为受限。**
- **在 Mac 上，Folio 将两份许可证、第三方声明和商标声明打包在应用内。**

<details>
<summary>安装说明（SmartScreen、Gatekeeper、校验和）</summary>

### Windows

| 文件 | 说明 |
| --- | --- |
| `folio-0.4.1-windows-x64.zip` | 九个归属文件，打包在一个文件夹中 — `sha256:00e1cdce977cd80176e63b73b64bb72105352ebed7bf065402a7b65fe31f90fd` |
| `folio-windows-x64.zip` | 同一份压缩包，名称在各版本间固定不变 — 同一个 `sha256` |
| `SHA256SUMS.txt` | 压缩包两个名称和物料清单的哈希，格式为 `sha256sum -c` 可读 |
| `folio-0.4.1.cdx.json` | CycloneDX 物料清单 |

将 zip 解压到存放程序的位置，运行 `folio.exe`。没有安装程序；保持解压后的文件在同一个文件夹中，覆盖旧文件夹即可保留设置。需要 **Windows 10 1809 或更高版本，或 Windows 11，64 位**。网页预览需要 **WebView2 Runtime**，Windows 11 已自带，Windows 10 通常也有。

`folio.exe` 和 `folio.msix` 由 **Weiyi Shi** 签名，自 0.2.0 起一贯如此，但签名不等于声誉，SmartScreen 在首次运行时仍可能弹出 **"Windows 已保护你的电脑"**：点击**更多信息**可以看到发布者为 **Weiyi Shi**，点击**仍要运行**即可通过。

校验下载文件，将 zip 和 `SHA256SUMS.txt` 放在同一个文件夹中：

```powershell
Get-FileHash folio-0.4.1-windows-x64.zip -Algorithm SHA256
```

或者在有 `sha256sum` 的环境——Git Bash、WSL、Linux：

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| 文件 | 说明 |
| --- | --- |
| `Folio-0.4.1-macos-arm64.dmg` | 应用程序，已签名并经过 Apple 公证 — `sha256:5e01fca25218abb30ec94f04a915edfa01c3c8754cc62ecf806f9806e03db20d` |
| `Folio-macos-arm64.dmg` | 同一个磁盘映像，名称在各版本间固定不变 — 同一个 `sha256` |
| `SHA256SUMS-macos.txt` | 磁盘映像两个名称的哈希，格式为 `shasum -c` 可读 |

打开磁盘映像，将 **Folio** 拖入"应用程序"文件夹。需要 **Apple 芯片 Mac，运行 macOS 14 或更高版本**；此预览版没有 Intel 构建。也可以用 [Homebrew](https://brew.sh)：

```sh
brew install --cask lulu-loopp/folio/folio
```

磁盘映像及其中的应用程序使用 Developer ID 签名，并经过 Apple 公证。首次打开时会弹出一个面板显示开发者名称，面板中有**打开**按钮；如果打开时没有出现**打开**选项，右键点击应用程序并选择**打开**，只需确认一次。如果弹出的面板提示无法验证开发者，或提示应用程序已损坏无法打开，说明下载到的内容与发布的不一致——对照校验和文件检查，并从发布页重新下载。

校验下载文件，将磁盘映像和 `SHA256SUMS-macos.txt` 放在同一个文件夹中：

```sh
shasum -c SHA256SUMS-macos.txt
```

**Folio 没有遥测、没有数据分析、没有崩溃上报。**连接网络的操作只有两项：在网页预览中打开的页面，以及更新检查——可在 设置 ▸ 常规 ▸ **检查新版** 中关闭。

</details>

完整列表见 [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.1-preview/CHANGELOG.md#041-preview--2026-09-16) · [v0.4.0-preview…v0.4.1-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.0-preview...v0.4.1-preview)
