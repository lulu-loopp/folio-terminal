> 本版本 GitHub Release 正文的中文版。

# Folio 0.4.3

**下载：**[zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.3-preview/folio-0.4.3-windows-x64.zip)（Windows 10 1809 及以上 / Windows 11，64 位）· [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.3-preview/Folio-0.4.3-macos-arm64.dmg)（macOS 14 及以上，Apple 芯片）

**下载：** 上方 zip 与 dmg 即为完整下载，其余为校验和、物料清单与源码。[English release note](https://github.com/lulu-loopp/folio-terminal/blob/main/docs/plans/release/release-note-v0.4.3-preview.md)

## 亮点

- Folio 不再整个会话反复读取自己的 PSReadLine 模块——0.4.2 也有这个问题，当时只能靠重启缓解。
- 终端中文字体可以在设置里选择，Windows 上默认改为 NSimSun。
- 一条命令清除 Folio 在自身文件夹之外写入的所有内容，Windows 上双击 `uninstall.cmd` 也行。
- 预览编辑器保存文档时保留文件原有的属性——下载标记、权限、时间戳。
- 路径含中文的页面和 PDF 可以在预览中打开了。

## 变更

### 新增

- **`folio --uninstall-cleanup`** 清除 Folio 在自身文件夹之外写入的内容（`$PROFILE` 中的行、agent 钩子、资源管理器菜单项、PSReadLine 模块），不开窗口，逐项报告结果；`--purge` 还会删除设置、会话和浏览器数据。
- **`uninstall.cmd`** 位于 Windows 压缩包的 `folio.exe` 旁边，双击即执行同样的清除操作。
- **设置 ▸ 终端 ▸ 中文字体**：可选**自动**或任何已安装的中日韩字体，ASCII 等宽字体不变。
- **`folio --remove-shell-integration`** 从 PowerShell `$PROFILE` 中移除 Folio 写入的行，不开窗口。
- **`folio --remove-explorer-menu`** 从资源管理器右键菜单中移除 Folio 的两个条目，不开窗口。
- **修复 agent 重绘吃掉的矩阵行**，矩阵按原始排列显示；不影响复制公式的结果。
- **`docs/recovery-after-deleting-folio.md`**，供已经手动删掉旧版 Folio 的用户参考。

### 改动

- **预览编辑器的保存保留文件的 streams、权限、属性和时间戳**；内容保证写入，其余尽力保留，CHANGELOG 列出每条限制——**硬链接的另一个名字读到的还是旧内容，两步之间崩溃或断电可能让旧文档留在原位**。
- **Folio 只在目标位置为空或属于自己时才安装 PSReadLine 模块**，移除时只删自己的文件。**0.4.2 及之前的所有版本可能覆盖你自己安装的 PSReadLine**；CHANGELOG 说明了如何判断是否受影响以及如何恢复。
- **Folio 在 `$PROFILE` 中写入的行统一为带保护的形式**，脚本不存在时不报错，启动时自动改写旧格式。
- **agent 钩子归属于它指向的 `folio.exe`**：Folio 只动自己的和已失效的，另一份副本的钩子不动，接管需确认；更新钩子可能重新格式化 agent 的 JSON 设置文件，旁边保留带日期的备份。
- **启用 PowerShell 整合的提示改为询问你是否已有**，不再只问 Folio 是否写过。
- **Windows 上终端的中文字体默认为 NSimSun**，加粗不再换字体；比例文本使用独立字体链并明确指定。
- **窗口线程不再为程序打印的路径查询磁盘。**
- **卡顿行标出耗时的调用**，并在挂钟时间旁记录窗口线程自身的 CPU 时间。
- **`diagnostics.log` 标明 Folio 请求的 GPU 和实际获得的 GPU**，`BT_GPU_PREFERENCE=low` 可让双显卡笔记本使用集成显卡运行一次。
- **三条诊断记录始终开启**——窗口超过一秒没有新画面、一分钟的大量读盘且无输入、输入法组字序列异常——不记录任何输入的文字。
- **`BT_IME_TRACE` 不再记录组字或已提交的文字**；旧版本曾记录过。

### 修复

- **Folio 不再在事件循环的每一轮重新读取已安装的 PSReadLine 模块**（一旦 PowerShell 窗格上报过版本或打开过终端设置页）。**0.4.2 也有此问题，重启是唯一的解决办法**；CHANGELOG 有完整说明。
- **程序通过同步更新暂存的帧在调整窗口大小时不再丢失**，暂存块结束时保留被打断的转义序列。
- **提示符重绘键只发送给 shell 按正常顺序打开的提示符**，输出中碰巧含有 shell 标记的内容不再被打进实际在运行的程序里。
- **悬停或点击程序打印的路径不在窗口线程上做任何磁盘调用**，慢速或断开的共享不再冻结窗口；不存在的路径显示「无法预览 —— 文件不存在」；被拒绝的路径在程序再次打印时重新查询；静止指针下方的链接响应第一次点击。
- **设置页面在内容变化时才布局**，不再随指针移动而布局。
- **公式的两个标记读取当前帧的位置**，公式块落定后停止移动。
- **已获得焦点的窗格不再被发送多余的焦点报告**（在提示符前显示为 `^[[I`）。
- **等待操作的卡片光晕保留完整边距**，圆点正常跳动。
- **放大窗格的名字不再与放大标记重叠**，过窄的预览窗格不再绘制切换器也不再吞掉键盘。
- **粗体中文保持所选字体**；没有斜体的字体请求斜体时保持斜体。
- **终端中文字体明确指定而非由字体库搜索**：Windows 上网格使用 NSimSun，比例文本不再在中等字重时回落到 SimSun（#10）。
- **`\text{…}` 内的行尾、`\begin{equation}` 内的换行和嵌套的 `\begin{array}{cc}`** 正确修复。
- **本地 `file:` 地址按文件名读取**，路径含中文的页面或 PDF 可以打开（#7）。
- **粘贴截图只复制一种格式的图片**，剪贴板图片的尺寸在解码前先检查。
- **macOS：同一瞬间启动的第二个 Folio 不再保留会话不保存的窗口**，预览跳转到最后指定的地址。
- **空闲窗口让事件循环休眠。**
- **PowerShell 脚本在预览中带高亮显示。**
- **`diagnostics.log` 始终以标明构建版本的行开头。**

## 已知问题

- 部分中文输入法在系统卡顿时可能丢字，原因已定位，**本版本未修复**，`diagnostics.log` 会记录丢字时刻。
- 桌面合成器卡顿时打字也会卡，持续时间与合成器一致。**本版本未修复。**
- Windows：Ctrl+点击将文件交给默认程序时窗口会停约一秒。
- 程序打印的带空格路径只有加了引号才是链接。
- 没有粗体字重的字体在终端中以常规字重显示粗体中文——Windows 默认的 NSimSun 没有粗体。
- 表格的标题行刚好在视口上方时表格会消失，把标题行滚回来即可恢复。
- 网页预览回应页面消息框后显示的提示被页面遮挡。
- 窗口保存在较晚枚举的显示器上时，恢复时出现在主显示器上。
- Windows：Folio 不能作为 Visual Studio Code 的内嵌面板；压缩包中的 `folio-here.cmd` 可以让它成为 VS Code 打开的外部终端。
- Windows：`.webm` 需要从 Microsoft Store 安装 VP9 或 AV1 Video Extension。
- macOS：崩溃报告存放在系统位置，下次启动 Folio 时会在日志中记录文件名。

<details>
<summary>安装说明（SmartScreen、Gatekeeper、校验和）</summary>

### Windows

| 文件 | 说明 |
| --- | --- |
| `folio-0.4.3-windows-x64.zip` | 十个归属文件，打包在一个文件夹中 — `sha256:<fill at build>` |
| `folio-windows-x64.zip` | 同一份压缩包，名称在各版本间固定不变 — 同一个 `sha256` |
| `SHA256SUMS.txt` | 压缩包两个名称和物料清单的哈希，格式为 `sha256sum -c` 可读 |
| `folio-0.4.3.cdx.json` | CycloneDX 物料清单 |

将 zip 解压到存放程序的位置，运行 `folio.exe`。没有安装程序；保持解压后的文件在同一个文件夹中，覆盖旧文件夹即可保留设置。需要 **Windows 10 1809 或更高版本，或 Windows 11，64 位**。网页预览需要 **WebView2 Runtime**，Windows 11 已自带，Windows 10 通常也有。

也可以用 [Scoop](https://scoop.sh)（来自 issue #9）：

```powershell
scoop bucket add folio https://github.com/lulu-loopp/scoop-folio
scoop install folio
```

`folio.exe` 和 `folio.msix` 由 **Weiyi Shi** 签名，自 0.2.0 起一贯如此，但签名不等于声誉，SmartScreen 在首次运行时仍可能弹出 **"Windows 已保护你的电脑"**：点击**更多信息**可以看到发布者为 **Weiyi Shi**，点击**仍要运行**即可通过。

校验下载文件，将 zip 和 `SHA256SUMS.txt` 放在同一个文件夹中：

```powershell
Get-FileHash folio-0.4.3-windows-x64.zip -Algorithm SHA256
```

或者在有 `sha256sum` 的环境——Git Bash、WSL、Linux：

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| 文件 | 说明 |
| --- | --- |
| `Folio-0.4.3-macos-arm64.dmg` | 应用程序，已签名并经过 Apple 公证 — `sha256:<fill at build>` |
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

完整列表见 [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.3-preview/CHANGELOG.md#043-preview--<release date as in the heading>) · [v0.4.2-preview…v0.4.3-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.2-preview...v0.4.3-preview)
