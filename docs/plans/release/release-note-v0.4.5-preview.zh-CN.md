> 本版本 GitHub Release 正文的中文版。

# Folio 0.4.5

**下载：**[zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.5-preview/folio-0.4.5-windows-x64.zip)（Windows 10 1809 及以上 / Windows 11，64 位）· [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.5-preview/Folio-0.4.5-macos-arm64.dmg)（macOS 14 及以上，Apple 芯片）

**下载：** 上方 zip 与 dmg 即为完整下载，其余为校验和、物料清单与源码。[English release note](https://github.com/lulu-loopp/folio-terminal/blob/main/docs/plans/release/release-note-v0.4.5-preview.md)

## 亮点

- 打字不再卡顿：窗口不再为查询位置、查询任务栏、传递标题、列举字体或移动候选窗而停顿；打开过网页后，启动后的第一个网页可以即时显示。
- 每个终端窗格可以有独立的字号：Ctrl+= 和 Ctrl+-（Mac 上为 ⌘），或在窗格上 Ctrl+滚轮，Ctrl+0 恢复默认。
- 没有粗体字重的终端字体用同一字体加粗绘制，中文粗体不再有个别字符显示为常规字重。
- 提示气泡、卡片、菜单等带淡入淡出效果的界面整体渐变。
- 未保存的编辑不会丢失：在询问是否保存时关闭窗口不再丢弃更改，按键、粘贴和点击不再穿透恢复卡片。

## 变更

### 新增

- **每个终端窗格可以有独立的字号**：Ctrl+= 放大，Ctrl+- 缩小（Mac 上为 ⌘+= 和 ⌘+-），也可以在窗格上 Ctrl+滚轮调整，Ctrl+0 恢复默认。不在 100% 时窗格上显示当前大小，重启 Folio 后恢复。

### 变更

- **打开过网页后，启动后首次打开的网页可以即时显示**：Folio 在空闲时预备好一个网页窗格。
- **提示气泡、卡片、菜单等带淡入淡出效果的界面整体渐变**，边框和文字不再先于背景出现。
- **没有粗体字重的终端字体用同一字体加粗绘制。**
- **搜索面板、视频控件、通知、提示条、标签页预览、恢复列表和关闭按钮**的字号、间距和圆角与窗口其余部分统一。
- **下载通知的样式与其他对话框和浮动通知统一。**
- **右键菜单和下拉菜单的图标与文字间距与设置页统一。**
- **首次运行卡片的边距和标题行高与其他对话框统一。**
- **搜索面板行的圆角和图标间距与其他列表统一。**
- **窗格标题栏、文件列栏和浮窗**共用统一的间距、标题、图标和控件。
- **设置中的快捷键标签、配置徽标、导航间距、底部间距和菜单按钮圆角**与其他位置一致。
- **Git 面板和提交图**的间距、圆角、标题、徽标和图标与窗口其余部分统一。
- **悬浮卡标题、拖拽标签和预览标签**与其他浮动标签统一。
- **Windows 上的网页窗格使用系统的覆盖式滚动条。**
- **打开网页导致 Folio 停顿半秒以上时**，diagnostics.log 中会记录耗时的步骤。
- **升级时，Folio 自动更新自己安装的 PSReadLine 副本**；你自行安装的不改动。

### 修复

- **使用输入法打字时**，Folio 移动候选窗不再造成停顿。
- **打字时 Folio 不再因查询任务栏而停顿。**
- **打字时 Folio 不再反复查询窗口位置**，每轮只查询一次。
- **频繁更改窗口标题的程序不再拖慢打字。**
- **在历史较长的窗格中搜索时打字保持流畅**，较远的匹配在后续几帧中补充显示。
- **选择了终端字体后启动不再等待 Folio 列举系统全部字体。**
- **Folio 运行时安装的字体在下次打开设置时出现在字体列表中。**
- **没有粗体字重的终端字体遇到粗体文字时不再切换到其他字体。**
- **中文粗体不再有个别字符显示为常规字重。**
- **按键、粘贴、点击和滚轮不再穿透恢复卡片**；按 Esc 关闭卡片，下次启动时再次询问。
- **滚轮不再滚动对话框下方的窗格。**
- **在询问是否保存时关闭窗口不再丢弃未保存的更改。**

## 已知问题

- GPU 或显示驱动短暂卡顿时，画面可能在打字过程中冻结片刻；输入照常接收，冻结结束后画面立刻追上。
- 从未打开过网页的配置中首次打开网页仍需几秒；之后以及打开过网页的配置中可以即时显示。

<details>
<summary>安装说明（SmartScreen、Gatekeeper、校验和）</summary>

### Windows

| 文件 | 说明 |
| --- | --- |
| `folio-0.4.5-windows-x64.zip` | 十个归属文件，打包在一个文件夹中 — `sha256:<!-- checksums -->` |
| `folio-windows-x64.zip` | 同一份压缩包，名称在各版本间固定不变 — 同一个 `sha256` |
| `SHA256SUMS.txt` | 压缩包两个名称和物料清单的哈希，格式为 `sha256sum -c` 可读 |
| `folio-0.4.5.cdx.json` | CycloneDX 物料清单 |

将 zip 解压到存放程序的位置，运行 `folio.exe`。没有安装程序；保持解压后的文件在同一个文件夹中，覆盖旧文件夹即可保留设置。需要 **Windows 10 1809 或更高版本，或 Windows 11，64 位**。网页预览需要 **WebView2 Runtime**，Windows 11 已自带，Windows 10 通常也有。

也可以用 [Scoop](https://scoop.sh)：

```powershell
scoop bucket add folio https://github.com/lulu-loopp/scoop-folio
scoop install folio
```

`folio.exe` 和 `folio.msix` 由 **Weiyi Shi** 签名，自 0.2.0 起一贯如此，但签名不等于声誉，SmartScreen 在首次运行时仍可能弹出 **"Windows 已保护你的电脑"**：点击**更多信息**可以看到发布者为 **Weiyi Shi**，点击**仍要运行**即可通过。

校验下载文件，将 zip 和 `SHA256SUMS.txt` 放在同一个文件夹中：

```powershell
Get-FileHash folio-0.4.5-windows-x64.zip -Algorithm SHA256
```

或者在有 `sha256sum` 的环境——Git Bash、WSL、Linux：

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| 文件 | 说明 |
| --- | --- |
| `Folio-0.4.5-macos-arm64.dmg` | 应用程序，已签名并经过 Apple 公证 — `sha256:<!-- checksums -->` |
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

完整列表见 [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.5-preview/CHANGELOG.md#045-preview--2026-09-26) · [v0.4.4-preview…v0.4.5-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.4-preview...v0.4.5-preview)
