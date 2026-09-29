> 本版本 GitHub Release 正文的中文版。

# Folio 0.4.6

**下载：**[zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.6-preview/folio-0.4.6-windows-x64.zip)（Windows 10 1809 及以上 / Windows 11，64 位）· [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.6-preview/Folio-0.4.6-macos-arm64.dmg)（macOS 14 及以上，Apple 芯片）

**下载：** 上方 zip 与 dmg 即为完整下载，其余为校验和、物料清单与源码。[English release note](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.6-preview/docs/plans/release/release-note-v0.4.6-preview.md)

## 亮点

- Folio 可以自行更新：有新版本时，卡片提供更新、下载并重启，新版本未能启动时自动恢复旧版。
- Mac 上同样可以自行更新。<!-- U-32 -->
- 重启或注销时保存布局，效果与正常退出一致，固定标签页及其窗格均保留。
- 在搜索栏、设置字段或 Git 面板的搜索和分支输入框中粘贴时，文本进入该字段而非背后的终端。
- 中文界面翻译完成，设置描述为书面陈述句，同一事物在每个页面使用统一名称。

## 变更

### 新增

- **Windows：Folio 可以从更新卡片自行更新**；设置 ▸ 常规 有"重启以更新"行，可再次调出卡片。
- **macOS：Folio 以同样方式自行更新。** <!-- U-32 -->
- **通过 scoop 或 winget 安装的副本**显示包管理器的更新命令，不自行更新。
- **通过 Homebrew 安装的副本**同样显示包管理器的命令。<!-- U-32 -->
- **更新未能完成时**，已安装的 Folio 重新打开并显示"更新未完成"卡片；CHANGELOG 中说明了更新中断时的处理方式。
- **已下载但未安装的更新**——选择了"以后"，或因断电中断——在下次启动时再次提供，可直接重启。

### 变更

- **中文设置描述采用书面陈述句**，同一事物在每个页面使用统一名称。
- **更新卡片、更新行、快捷键面板的缩放操作、配置编辑器的登录开关和关闭界面**均已翻译为中文。
- **设置齿轮显示更新圆点时**，点击它打开设置的更新行；关于页显示较新版本号。
- **右键菜单默认仅在能移除它的安装方式中开启。**

### 修复

- **重启或注销时保存布局**，效果与正常退出一致；固定标签页及其窗格均恢复。
- **紧跟中文文字、冒号或 `=` 之后的网址被识别为链接**，后跟左括号的网址在括号前结束。
- **文本字段获得焦点时粘贴**——搜索栏、Git 图的搜索框、分支输入框、设置字段——文本进入该字段。
- **将文件重命名为仅大小写不同的名称时**，不再在区分大小写的文件夹中替换另一个文件。
- **从资源管理器右键菜单打开 Folio**，或在另一个 Folio 启动时运行 `folio --version`，不再导致新窗口无法保存设置和标签页。
- **粘贴图片兼容剪贴板能提供的所有位图格式**，包括浏览器复制和长截图。
- **网页窗格在浏览器引擎自行更新后继续工作。**
- **分多次到达的重绘不再让公式源码闪现一帧。**
- **macOS：内置 shell 以登录 shell 启动**，Homebrew 的工具可被找到；配置中可开关登录 shell。
- **macOS：以 fish 方式报告目录的 shell 将该目录赋予窗格。**
- **Markdown 预览：macOS 上 `file:///Users/…` 链接可以打开**，含 `%20` 的链接在两个平台上均能找到文件。
- **macOS：使用 `--purge` 卸载时移除剪贴板暂存文件夹和崩溃日志。**
- **无法打开的 trace 文件不再阻塞日志记录。**

## 已知问题

- Ctrl+Enter 和 Shift+Enter 仍然以 Enter 发送给程序；区分它们的 kitty 键盘协议将在 0.4.7 实现（[#13](https://github.com/lulu-loopp/folio-terminal/issues/13)）。
- 更新完成后，如果另一程序在数分钟内持续占用 Folio 的更新记录，新版本运行期间无法保存更改，下次启动时恢复旧版。
- macOS：首次自行更新时，macOS 会提示"已添加可在后台运行的软件"；该登录项是更新器的恢复入口，更新完成后自动移除。 <!-- U-32 -->

<details>
<summary>安装说明（SmartScreen、Gatekeeper、校验和）</summary>

### Windows

| 文件 | 说明 |
| --- | --- |
| `folio-0.4.6-windows-x64.zip` | 十个归属文件，打包在一个文件夹中 — `sha256:<!-- checksums -->` |
| `folio-windows-x64.zip` | 同一份压缩包，名称在各版本间固定不变 — 同一个 `sha256` |
| `SHA256SUMS.txt` | 压缩包两个名称和物料清单的哈希，格式为 `sha256sum -c` 可读 |
| `folio-0.4.6.cdx.json` | CycloneDX 物料清单 |

将 zip 解压到存放程序的位置，运行 `folio.exe`。没有安装程序；保持解压后的文件在同一个文件夹中，覆盖旧文件夹即可保留设置。需要 **Windows 10 1809 或更高版本，或 Windows 11，64 位**。网页预览需要 **WebView2 Runtime**，Windows 11 已自带，Windows 10 通常也有。

也可以用 [Scoop](https://scoop.sh)：

```powershell
scoop bucket add folio https://github.com/lulu-loopp/scoop-folio
scoop install folio
```

`folio.exe` 和 `folio.msix` 由 **Weiyi Shi** 签名，自 0.2.0 起一贯如此，但签名不等于声誉，SmartScreen 在首次运行时仍可能弹出 **"Windows 已保护你的电脑"**：点击**更多信息**可以看到发布者为 **Weiyi Shi**，点击**仍要运行**即可通过。

校验下载文件，将 zip 和 `SHA256SUMS.txt` 放在同一个文件夹中：

```powershell
Get-FileHash folio-0.4.6-windows-x64.zip -Algorithm SHA256
```

或者在有 `sha256sum` 的环境——Git Bash、WSL、Linux：

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| 文件 | 说明 |
| --- | --- |
| `Folio-0.4.6-macos-arm64.dmg` | 应用程序，已签名并经过 Apple 公证 — `sha256:<!-- checksums -->` |
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

**Folio 没有遥测、没有数据分析、没有崩溃上报。**连接网络的操作只有两项：在网页预览中打开的页面，以及更新检查及其下载——可在 设置 ▸ 常规 ▸ **检查新版** 中关闭。

</details>

完整列表见 [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.6-preview/CHANGELOG.md#046-preview--2026-09-28) · [v0.4.5-preview…v0.4.6-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.5-preview...v0.4.6-preview)
