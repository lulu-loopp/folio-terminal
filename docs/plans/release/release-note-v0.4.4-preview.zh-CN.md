> 本版本 GitHub Release 正文的中文版。

# Folio 0.4.4

**下载：**[zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.4-preview/folio-0.4.4-windows-x64.zip)（Windows 10 1809 及以上 / Windows 11，64 位）· [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.4-preview/Folio-0.4.4-macos-arm64.dmg)（macOS 14 及以上，Apple 芯片）

**下载：** 上方 zip 与 dmg 即为完整下载，其余为校验和、物料清单与源码。[English release note](https://github.com/lulu-loopp/folio-terminal/blob/main/docs/plans/release/release-note-v0.4.4-preview.md)

## 亮点

- 粘贴多行文本不再直接执行：在 PowerShell 中内容留在输入行等你按回车，在 cmd 中弹出卡片询问是**逐行运行**还是**合为一行**。
- Ctrl+点击网络共享路径或 `mailto:`、`vscode:` 等链接会交给 Windows 或 macOS 处理，交出文件时窗口不再卡住。
- 设置可以导出为一个文件并在另一台机器上导入，入口在 设置 ▸ 关于，旁边还有**打开设置文件夹**的按钮。
- 手指在窗格上滑动即可滚动，惯性由系统提供。
- 网页窗格跟随 Folio 的浅色或深色主题。

## 变更

### 多行粘贴

- **在 PowerShell 中粘贴多行内容会停在输入行**，等你按回车。
- **在逐行执行的 shell（如 cmd）中，粘贴前弹出卡片**供你选择：**逐行运行**、**合为一行**或取消。可在终端设置中关闭此提示。
- **卡片按标准双按钮对话框操作**：Tab 和 Shift+Tab 在按钮之间切换，回车确认高亮的按钮，Esc 取消。
- **粘贴的内容如果是一条续行命令**——cmd 中每行末尾为 `^`、PowerShell 中为反引号、Unix shell 中为 `\`——回车会合并为一行并去掉续行符。
- **卡片标题不再被关闭按钮遮挡**，过长的配置名称以省略号截断。

### 链接与交接

- **Ctrl+点击网络共享路径或 `mailto:`、`vscode:` 等链接**会交给 Windows 或 macOS 处理。预览文档中的链接也一样。
- **Ctrl+点击打开文件时窗口不再卡住**，资源管理器正常来到前台。
- **网页窗格显示 `http` 或 `https` 页面时，↗ 按钮在浏览器中打开该页面**；此前点击无反应。
- **预览中的本地 HTML 页面可以从网络加载样式表、脚本和字体。**
- **带空格的路径在文件存在时即为链接**，无论是否加了引号。
- **相对路径后紧跟全角冒号、逗号或句号**（`plot.png：…`、`notes.md。18 items`）时，文件存在即恢复为链接。
- **以斜杠结尾的文件夹路径**（`models/`）也是链接，点击后显示该文件夹。

### 触控

- **手指在窗格上滑动即可滚动**，惯性由系统提供；单指滑动不再选中文字。
- **触屏和发送触控事件的远程桌面可以像其他程序一样操作 Folio**：轻触是点击，长按打开菜单。

### 悬浮卡与 ⌄ 菜单

- **悬浮卡底部显示文件所在的文件夹**，鼠标悬停时高亮：点击可在文件列中定位该文件，Ctrl+点击在资源管理器中显示。
- **点击 ⌄ 打开的菜单保持展开**，直到按 Esc、点击其他位置或再次点击 ⌄；鼠标悬停仍然只在指针停留时显示。

### 设置

- **设置 ▸ 关于**新增**导出…**、**导入…**和**打开设置文件夹**；该页所有链接改为统一大小的按钮。
- **快捷键可以使用 Ctrl+字母组合**；行上标注该键不再传给 shell。
- **卡片设置在 macOS 上以及重新绑定后显示的快捷键已修正。**

### 网页窗格

- **网页窗格跟随 Folio 的浅色或深色主题**，也可在设置中锁定为其中一种。
- **与所在栏颜色接近的站点图标加一个小圆底。**

### 终端与预览

- **窗格右侧的命令标记占据独立一列**，不再遮挡文本的最后一列。
- **正在编辑的 Markdown 预览中，选区接触到的每个块显示 Markdown 源码**；选区仅穿过的表格保持排版。松开鼠标时块才切换，拖动过程中不变；选区覆盖正在编辑的块时也会高亮。
- **浮窗中放大的图片可以拖拽平移**，双击可缩放，与窗格内操作一致。
- **拖动分隔条时，指针下方的窗格保留右上角圆角。**
- **标题栏、菜单、搜索面板、Git 页和设置共用一套尺寸**，标题、行和按钮在窗口内对齐。

### macOS

- **Git 页可以找到 git。**
- **深色方案行不再显示 Windows 文件夹路径。**
- **设置 ▸ 常规中 Option 键的说明改为两行。**

### 稳定性

- **磁盘较慢时，从欢迎卡开启 PowerShell 整合不再报告锁错误。**

## 已知问题

- GPU 繁忙时打字可能卡顿 1–2 秒。修复已排期。
- 首次打开网页时窗口会卡几秒。修复已排期。

<details>
<summary>安装说明（SmartScreen、Gatekeeper、校验和）</summary>

### Windows

| 文件 | 说明 |
| --- | --- |
| `folio-0.4.4-windows-x64.zip` | 十个归属文件，打包在一个文件夹中 — `sha256:<!-- checksums -->` |
| `folio-windows-x64.zip` | 同一份压缩包，名称在各版本间固定不变 — 同一个 `sha256` |
| `SHA256SUMS.txt` | 压缩包两个名称和物料清单的哈希，格式为 `sha256sum -c` 可读 |
| `folio-0.4.4.cdx.json` | CycloneDX 物料清单 |

将 zip 解压到存放程序的位置，运行 `folio.exe`。没有安装程序；保持解压后的文件在同一个文件夹中，覆盖旧文件夹即可保留设置。需要 **Windows 10 1809 或更高版本，或 Windows 11，64 位**。网页预览需要 **WebView2 Runtime**，Windows 11 已自带，Windows 10 通常也有。

也可以用 [Scoop](https://scoop.sh)（来自 issue #9）：

```powershell
scoop bucket add folio https://github.com/lulu-loopp/scoop-folio
scoop install folio
```

`folio.exe` 和 `folio.msix` 由 **Weiyi Shi** 签名，自 0.2.0 起一贯如此，但签名不等于声誉，SmartScreen 在首次运行时仍可能弹出 **"Windows 已保护你的电脑"**：点击**更多信息**可以看到发布者为 **Weiyi Shi**，点击**仍要运行**即可通过。

校验下载文件，将 zip 和 `SHA256SUMS.txt` 放在同一个文件夹中：

```powershell
Get-FileHash folio-0.4.4-windows-x64.zip -Algorithm SHA256
```

或者在有 `sha256sum` 的环境——Git Bash、WSL、Linux：

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| 文件 | 说明 |
| --- | --- |
| `Folio-0.4.4-macos-arm64.dmg` | 应用程序，已签名并经过 Apple 公证 — `sha256:<!-- checksums -->` |
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

完整列表见 [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.4-preview/CHANGELOG.md#044-preview--2026-09-24) · [v0.4.3-preview…v0.4.4-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.3-preview...v0.4.4-preview)
