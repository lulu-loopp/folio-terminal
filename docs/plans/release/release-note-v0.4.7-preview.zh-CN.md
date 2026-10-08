> 本版本 GitHub Release 正文的中文版。

# Folio 0.4.7

**下载：**[zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.7-preview/folio-0.4.7-windows-x64.zip)（Windows 10 1809 及以上 / Windows 11，64 位）· [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.7-preview/Folio-0.4.7-macos-arm64.dmg)（macOS 14 及以上，Apple 芯片）

**下载：** 上方 zip 与 dmg 即为完整下载，其余为校验和、物料清单与源码。[English release note](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.7-preview/docs/plans/release/release-note-v0.4.7-preview.md)

## 亮点

- 请求键盘协议的程序——Claude Code、Codex、neovim、fish、helix——可以区分 Ctrl+Enter、Shift+Enter、Alt+Enter 和 Enter，以及 Shift+Tab、Esc 和 Tab 与单独的 Escape。
- PowerShell 窗格无需设置即可使用命令标记、当前目录和行内公式。
- 保持运行的 Folio 会在一天内发现新版本，关于 ▸ 版本提供更新。
- 设置中新增卸载行，一步完成卸载。
- Folio 运行期间安装的命令，在新标签页中即可找到，无需重启 Folio。

## 变更

### 新增

- **kitty 键盘协议（第一层）和 xterm 的 modifyOtherKeys**，供请求它们的程序使用，如 Claude Code、Codex 和 neovim。
- **Windows 上，Ctrl+Enter、Shift+Enter 和 Alt+Enter** 以原键送达 Codex 等控制台程序。
- **无法自动设置的 PowerShell 配置文件**可在设置中一键经 `$PROFILE` 启用，支持撤销。
- **设置 ▸ 卸载 Folio**；压缩包中的卸载脚本同样一步完成卸载，且使用当前语言输出。设置和数据保留，除非另行要求。
- **图片或视频预览中**，‹ › 和方向键切换到同类型的上一个或下一个文件。
- **窗格菜单 ▸ 重置终端模式**撤销程序遗留的终端模式。
- **Claude Code 中**，输入行里的 `[Image #N]` 是指向所粘贴图片的链接。
- **tmux、screen、zellij 或 herdr 中的窗格**按窗格独立排版公式。

### 变更

- **启动时运行命令的 PowerShell 配置文件**（如 Developer PowerShell 或 conda 环境）保留原有命令，同样获得整合。
- **Folio 的 PowerShell 脚本**在其他终端读取同一 `$PROFILE` 时不执行任何操作。
- **新窗格继承 Folio 启动后的环境变更。**
- **新版本通知、更新按钮和自动检查开关**移至关于 ▸ 版本。
- **PowerShell 中**，Shift+Enter 换行，Ctrl+Enter 在上方插入一行，与 Windows Terminal 一致；Enter 运行命令。
- **F1–F12 送达程序**，Shift、Alt 和 Ctrl 修饰键按 xterm 方式编码。
- **README 说明各安装方式的卸载方法。**
- **大纲文件夹图标**采用实心文件夹的轮廓。

### 修复

- **两个 Folio 窗口不再同时开始同一个更新。**
- **更新在 Folio 运行期间失败时**，当前 Folio 窗口会告知。
- **macOS 上，更新完成期间打开 Folio** 不再撤销已成功的新版本。
- **Windows 上，当系统无法启动新版本也无法启动旧版本时**，Folio 仍会打开新版本。
- **新版本未能记录自身启动的更新**在下次启动或登录时完成，不再丢失新版本所做的更改。
- **更新消息更加准确**：中断的更新、需要更新版 Folio 的发布、被其他程序占用的文件、已恢复或未恢复的旧版本。
- **Folio 探测本机信息时运行的命令**不再遗留后台进程。
- **重启 shell、复制窗格和拆分**在窗格打开时所在的文件夹中启动。
- **Windows 上，Ctrl+Alt 加字母或数字**正确送达请求新键盘模式的程序。
- **链接**：不带 `https://` 的地址在冒号或 `=` 之后被识别；程序发送的链接在中文文字前结束；文件链接在各处以相同方式解码。
- **macOS 文件名**不再因仅适用于 Windows 的规则被拒绝。
- **粘贴到 csh 和 tcsh 中的路径**正确转义。
- **中文字体排序**保持原有顺序。
- **指向窗格通知条或被遮挡的标题栏控件时**，不再触发其下方或旁边的元素。
- **macOS 上，Folio 不再在每个数据文件夹中留下空的锁文件。**

## 已知问题

- 从 0.4.6 更新仍使用 0.4.6 的更新器：在 macOS 上，更新收尾时手动打开 Folio，更新器可能把正常运行的 0.4.7 换回 0.4.6。
- 从 0.4.6 更新：如果更新刚完成时另一程序占用 Folio 的更新记录长达数分钟，0.4.7 照常运行，但其所做的更改要到下次启动或登录完成更新后才会保存。
- 从 0.4.6 更新：如果更新在另一个 0.4.6 窗口打开时失败，该窗口不会告知。
- macOS：首次自行更新时，macOS 会提示"已添加可在后台运行的软件"；该登录项是更新器的恢复入口，更新完成后自动移除。
- 极少数情况下，若更新时磁盘出错且旧 Folio 关闭较慢，更新后不会打开 Folio 窗口；再次启动 Folio 即可完成更新。
- 如果开启了新键盘模式的程序——例如 Claude Code、Codex 或 neovim——被杀死或崩溃，在该窗格中输入的按键可能以 `[99;5u` 等文本形式出现；窗格菜单 ▸ 重置终端模式可恢复正常。
- Windows 上，启动时恢复的标签页若其配置文件在 PowerShell 中运行启动命令（如 Developer PowerShell 或 conda 环境），启动时没有命令标记和当前目录；Folio 运行后新开的该配置文件标签页则有。
- 极少数情况下，Folio 的首个窗口在启动时会短暂等待，因为正在查找本机已安装的程序。
- Windows，从微软商店安装了 PowerShell 7 时：如果 Folio 恰在启动该 PowerShell 以了解其信息时被外部结束或崩溃，该 PowerShell 可能暂停并一直留在后台，直到在任务管理器中结束它。macOS：Folio 被强制退出或崩溃后，它为了解本机环境而启动的命令会自行运行结束，而不是被停止。

<details>
<summary>安装说明（SmartScreen、Gatekeeper、校验和）</summary>

### Windows

| 文件 | 说明 |
| --- | --- |
| `folio-0.4.7-windows-x64.zip` | 十个归属文件，打包在一个文件夹中 — `sha256:<ZIP_SHA256>` |
| `folio-windows-x64.zip` | 同一份压缩包，名称在各版本间固定不变 — 同一个 `sha256` |
| `SHA256SUMS.txt` | 压缩包两个名称和物料清单的哈希，格式为 `sha256sum -c` 可读 |
| `folio-0.4.7.cdx.json` | CycloneDX 物料清单 |

将 zip 解压到存放程序的位置，运行 `folio.exe`。没有安装程序；保持解压后的文件在同一个文件夹中，覆盖旧文件夹即可保留设置。需要 **Windows 10 1809 或更高版本，或 Windows 11，64 位**。网页预览需要 **WebView2 Runtime**，Windows 11 已自带，Windows 10 通常也有。

也可以用 [Scoop](https://scoop.sh)：

```powershell
scoop bucket add folio https://github.com/lulu-loopp/scoop-folio
scoop install folio
```

`folio.exe` 和 `folio.msix` 由 **Weiyi Shi** 签名，自 0.2.0 起一贯如此，但签名不等于声誉，SmartScreen 在首次运行时仍可能弹出 **"Windows 已保护你的电脑"**：点击**更多信息**可以看到发布者为 **Weiyi Shi**，点击**仍要运行**即可通过。

校验下载文件，将 zip 和 `SHA256SUMS.txt` 放在同一个文件夹中：

```powershell
Get-FileHash folio-0.4.7-windows-x64.zip -Algorithm SHA256
```

或者在有 `sha256sum` 的环境——Git Bash、WSL、Linux：

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| 文件 | 说明 |
| --- | --- |
| `Folio-0.4.7-macos-arm64.dmg` | 应用程序，已签名并经过 Apple 公证 — `sha256:<DMG_SHA256>` |
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

完整列表见 [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.7-preview/CHANGELOG.md#047-preview--2026-10-DD) · [v0.4.6-preview…v0.4.7-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.6-preview...v0.4.7-preview)
