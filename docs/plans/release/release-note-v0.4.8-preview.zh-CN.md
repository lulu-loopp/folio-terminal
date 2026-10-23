> 本版本 GitHub Release 正文的中文版。

# Folio 0.4.8

**下载：**[zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.8-preview/folio-0.4.8-windows-x64.zip)（Windows 10 1809 及以上 / Windows 11，64 位）· [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.8-preview/Folio-0.4.8-macos-arm64.dmg)（macOS 14 及以上，Apple 芯片）

**下载：** 上方 zip 与 dmg 即为完整下载，其余为校验和、物料清单与源码。[English release note](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.8-preview/docs/plans/release/release-note-v0.4.8-preview.md)

## 亮点

- 关机、重启或注销 Windows 时，预览中未保存的编辑不再丢失；退出前会先保存设置、快捷键、配置文件和钉住条目。
- 杀毒软件或其他程序短暂占用 Folio 的文件时，更新会等它放开；一直被占用时，会指明是哪个文件。
- 在缩放为 125%、150% 或 175% 的显示器上，预览窗格中的 PDF 文字清晰。
- 用 Homebrew 安装的 Folio 可以在关于页中自行更新。
- Windows PowerShell 窗格调整大小时，长命令不再错乱，且无需安装任何东西。

## 变更

### 新增

- **搜索面板中的文件搜索**在后台保持更新，打开时即已就绪。
- **Folio 运行期间安装的程序**无需打开菜单即出现在 + 菜单和默认配置文件中。
- **macOS 上**，磁盘挂载后，等待它的文件夹随即被读取；系统语言更改后，新窗格随之使用新语言。
- **用 Homebrew 安装的 Folio** 可在关于页中就地更新；Homebrew 仍负责管理和卸载。
- **图形故障之后**，下次启动时会告诉你未保存的编辑保留在哪里，并附一个打开该文件夹的按钮。

### 变更

- **Windows PowerShell 窗格**使用 Folio 自带的 PSReadLine 2.4.5；不为你的账户安装任何东西，其他终端仍使用原有的 PSReadLine。
- **设置中的 PSReadLine 行和提供安装的卡片**已移除；旧版 Folio 装在「文档」中的那份可在设置 ▸ 终端中移除。
- **在其他编辑器中保存的设置、快捷键、配置文件和钉住条目**在 Folio 运行期间即可生效，由一张卡片列出哪些取自文件。
- **退出前**，先保存设置、快捷键、配置文件或钉住条目中尚未写入磁盘的改动；某项无法保存时，Folio 保持打开并指明该文件。
- **公式的右键菜单和 Windows 文件选择框中的图片筛选项**使用 Folio 的界面语言。
- **窗口在 shell 启动前出现**，输入的内容保留到 shell 就绪。
- **网页窗格中的长地址**折叠路径中段，保留站点。
- **Folio 启动后的第一个公式或图片**会多花一点时间，因为公式引擎在首次需要时才加载。
- **在无法一步替换文件的磁盘上**（如部分 U 盘），预览先在文件旁写一份副本再保存，该磁盘上第一次保存时会说明。
- **用 winget 安装的副本**仍通过 `winget upgrade` 更新；更新卡片用于 zip、scoop 和 Homebrew 安装的副本。

### 修复

- **在缩放为 125%、150% 或 175% 的显示器上预览的 PDF** 不再出现笔画粗细不均、单词中间断开的文字；在这类显示器上，PDF 查看器自身的控件画得更大，窗格较窄时工具栏会缩短，也可能不显示。PDF 打开后再调整窗格大小，页面不会按新的宽度重新排布，重新打开该 PDF 才会；以前的版本也是如此。
- **关机、重启或注销 Windows 时**，预览中未保存的编辑不再丢失；来不及写入的内容保留在 `recovered` 文件夹中，下次启动时会提示。
- **在退出卡片上选择保存**不再让窗口卡住；保存后立即关闭会等待保存完成，不再询问是否放弃改动。
- **Windows 上，程序短暂占用 Folio 的某个文件**不再导致更新回滚或无法完成：Folio 对每个文件最多等待十秒。
- **更新无法完成、Folio 从为此保留的副本打开时**，窗口会说明更新未完成并指明其文件夹，且不会再提供更新。
- **更新卡片说明实际发生了什么**——更新在新版本启动前停止，或新版本已在运行而更新未完成——以及新版本中所做的更改何时没有保留。
- **按下重启以更新后**，Folio 总会重新打开；已下载的更新不会因断电或崩溃而丢失。
- **旧版 Folio 发现较新版本发起的更新时**，会留给较新版本完成并加以说明。
- **其他程序占用剪贴板时**，复制和粘贴不再让窗口卡顿；快速粘贴两次，两次都按顺序到达。
- **Markdown 预览和终端以相同方式识别代码围栏**，与 CommonMark 的定义一致：`~~~` 和缩进的围栏是代码，较短的围栏不再提前结束代码块。
- **按住的拖动、选择或滚动滑块**会在其对象消失时结束；按住期间，另一个鼠标键、⌄ 菜单、工具提示、悬停预览和指针下的网页都不响应。
- **窗口等待的计时结束后**，不再让一个处理器核心一直忙碌；这在负载很高时曾让终端绘制变慢。
- **Windows 上，Esc 每次都能送达程序**；在隐藏终端光标的程序中用输入法输入时，正在组字的内容会显示出来。
- **Windows 刚启动时**，若 Folio 的 PowerShell 7 标签页来自 Microsoft Store，Folio 不再随即关闭；启动时停止的 shell 会保留其窗格。
- **通过 winget 安装的链接启动的 Folio**，窗格运行在随附的控制台主机上。
- **Mac 上装有两份 Folio 时**，打开第二份会并入已在运行的那份。
- **Git 页上**，每个仓库的操作依次执行，不再等待其他仓库。
- **Cygwin 和 WSL 的 shell 打印的路径**都是链接；粘贴到 Cygwin 中的路径按 Cygwin 的写法给出。
- **网页预览中**，返回 404 的页面显示服务器发来的内容，地址栏保留你请求的地址，请求新窗口的链接在页面旁打开。
- **系统无法打开的链接或页面**会说明原因：无程序可打开、找不到或拒绝访问。
- **设置、快捷键、配置文件或钉住条目的改动无法保存时**，在 Folio 运行期间就会报告；保存或启动 shell 时出现内部故障不再关闭 Folio。
- **Folio 不再启动它无法再停止的辅助程序。**

## 已知问题

- 从 0.4.7 或 0.4.6 更新仍使用该版本的更新器：程序占用 Folio 的更新记录或某个文件几秒钟，仍可能使更新回滚；从保留的副本打开的窗口不显示卡片。从 0.4.8 起的更新使用 0.4.8 的更新器，这两点都已处理。
- 从 0.4.6 更新仍使用 0.4.6 的更新器：在 macOS 上，更新收尾时手动打开 Folio，更新器可能把正常运行的 0.4.8 换回 0.4.6。
- macOS：首次自行更新时，macOS 会提示"已添加可在后台运行的软件"；该登录项是更新器的恢复入口，更新完成后自动移除。
- 0.4.8 之前用 Homebrew 安装的副本，这次更新请用 `brew upgrade`；之后即可在关于页中自行更新。
- 地址不以 `.pdf` 结尾的 PDF，在缩放的显示器上仍会被拉伸。
- Windows，从微软商店安装了 PowerShell 7 时：如果 Folio 恰在启动该 PowerShell 以了解其信息时被外部结束或崩溃，该 PowerShell 可能暂停并一直留在后台，直到在任务管理器中结束它。macOS：Folio 被强制退出或崩溃后，它为了解本机环境而启动的命令会自行运行结束，而不是被停止。

<details>
<summary>安装说明（SmartScreen、Gatekeeper、校验和）</summary>

### Windows

| 文件 | 说明 |
| --- | --- |
| `folio-0.4.8-windows-x64.zip` | 十个归属文件，打包在一个文件夹中 — `sha256:9512cbf99b569387de4a47bd6d143d32c068214989495a2eeaa2ae85ce6fc42f` |
| `folio-windows-x64.zip` | 同一份压缩包，名称在各版本间固定不变 — 同一个 `sha256` |
| `SHA256SUMS.txt` | 压缩包两个名称和物料清单的哈希，格式为 `sha256sum -c` 可读 |
| `folio-0.4.8.cdx.json` | CycloneDX 物料清单 |

将 zip 解压到存放程序的位置，运行 `folio.exe`。没有安装程序；保持解压后的文件在同一个文件夹中，覆盖旧文件夹即可保留设置。需要 **Windows 10 1809 或更高版本，或 Windows 11，64 位**。网页预览需要 **WebView2 Runtime**，Windows 11 已自带，Windows 10 通常也有。

也可以用 [Scoop](https://scoop.sh)：

```powershell
scoop bucket add folio https://github.com/lulu-loopp/scoop-folio
scoop install folio
```

`folio.exe` 和 `folio.msix` 由 **Weiyi Shi** 签名，自 0.2.0 起一贯如此，但签名不等于声誉，SmartScreen 在首次运行时仍可能弹出 **"Windows 已保护你的电脑"**：点击**更多信息**可以看到发布者为 **Weiyi Shi**，点击**仍要运行**即可通过。

校验下载文件，将 zip 和 `SHA256SUMS.txt` 放在同一个文件夹中：

```powershell
Get-FileHash folio-0.4.8-windows-x64.zip -Algorithm SHA256
```

或者在有 `sha256sum` 的环境——Git Bash、WSL、Linux：

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| 文件 | 说明 |
| --- | --- |
| `Folio-0.4.8-macos-arm64.dmg` | 应用程序，已签名并经过 Apple 公证 — `sha256:fd3d532160ce36abff881b5a3d7bd425b36a387b5d1c12b63f1999e90bf391ea` |
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

**Folio 没有遥测、没有数据分析、没有崩溃上报。**连接网络的操作只有两项：在网页预览中打开的页面，以及更新检查及其下载。关于 ▸ 版本 ▸ **自动检查** 可关闭每日检查；关闭后手动按**检查**仍然可以查询。

</details>

完整列表见 [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.8-preview/CHANGELOG.md#048-preview--2026-10-23) · [v0.4.7-preview…v0.4.8-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.7-preview...v0.4.8-preview)
