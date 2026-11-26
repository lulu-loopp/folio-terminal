> 本版本 GitHub Release 正文的中文版。

# Folio 0.4.10

**下载：**[zip](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.10-preview/folio-0.4.10-windows-x64-unsigned.zip)（Windows 10 1809 及以上 / Windows 11，64 位）· [dmg](https://github.com/lulu-loopp/folio-terminal/releases/download/v0.4.10-preview/Folio-0.4.10-macos-arm64.dmg)（macOS 14 及以上，Apple 芯片）

**下载：** 上方 zip 与 dmg 即为完整下载，其余为校验和、发布声明、物料清单与源码。[English release note](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.10-preview/docs/plans/release/release-note-v0.4.10-preview.md)

本版本同时包含为 0.4.9 所做的改动；0.4.9 没有公开发布。

**这个 Windows 版本没有签名，Windows 上的 Folio 0.4.8 也无法自行更新到此版本。**请手动安装，并在 Windows 发出警告时允许运行；做法见「已知问题」的前两条。

## 亮点

- 重启后，运行过 Claude Code 或 Codex 的窗格会重新出现，该 agent 在同一文件夹中、带着当初的启动参数接着同一段对话。
- 关闭最后一个窗口后，Folio 继续运行，shell 和 agent 也不会结束；Folio 留在 Windows 的通知区域或 macOS 的程序坞中。
- 在 Windows 上，Folio 可以在登录时启动；Windows 因更新重启后，Folio 会带着窗口重新打开；关机后再次启动时，标签页会恢复。
- 在 Windows 上，粘贴从程序内部复制的文件（例如邮件附件或 zip 文件夹里的文件）时，Folio 会先保存它，再粘贴它的路径。
- 从本版本起，Folio 用自己的发布密钥校验更新；证书被吊销或版本没有签名，都不再阻止更新。

## 变更

### 新增

- **重启后恢复 agent 窗格**：Windows 上在 Folio 或 Windows 重启后，macOS 上在 Folio 重启后，窗格接着同一段对话，文件夹和启动参数不变；设置 ▸ Agent ▸「重启后恢复 agent」可将其关闭。
- **关闭最后一个窗口后，Folio 继续运行**：Windows 上留在通知区域，图标显示 agent 正在工作、已完成或正在等你；macOS 上留在程序坞中。设置 ▸ 常规 ▸「关闭后继续运行」可将其关闭。
- **Windows 上，设置 ▸ 常规 ▸「登录时启动」**会在登录 Windows 时启动 Folio，默认关闭。
- **Windows 上，粘贴从程序内部复制的文件**时，Folio 把它保存到自己的剪贴板文件夹并粘贴它的路径，与粘贴截图时一样。
- **Windows 上，每个终端窗格都是一个文本区域**，讲述人等辅助功能工具可以找到它、读取其中的文字并跟随光标。
- **视频**：单击画面播放或暂停，双击放大所在窗格，滚轮调节音量，进度条显示指针处的时间，`Home` 和 `End` 跳到开头和结尾，暂停时 `,` 和 `.` 逐帧后退或前进，播完的视频再次播放时从头开始。
- **方向键可在放大的图片上移动**；在文件列中用鼠标中键单击文件，会在当前窗格旁新开一个窗格显示它。
- **设置 ▸ Agent** 会说明某个 agent 的 hook 为什么不属于这个 Folio，以及恢复会话还需要哪些 agent 的 hook。
- **这个 Folio 无法读取较新版本的 Folio 保存的文件时**，会说明一次并保留一份副本；较新版本的 Folio 再次启动时，会询问是否找回该副本。
- **Windows 上，新版本没有签名时**，更新卡片会显示「Windows 可能会对此更新发出警告。」；正在运行的 Folio 没有签名时，关于 ▸ 版本也会注明。

### 变更

- **Folio 用自己的发布密钥校验更新**：每个版本都附有一份签名的发布声明，列出每个文件及其校验值；Folio 只安装声明中列出的内容，声明不担保的更新会停止，并显示「更新未获发布声明担保。」
- **有 agent 正在工作或有命令正在运行时退出 Folio**，会先用一张卡片询问，并说明两者各有多少在运行；注销、重启或关机时不会为此等待。
- **标签页中有命令运行时**，窗口被唤醒的次数约为以前的一半，标签页运行标记每一步所做的工作也更少；显示的内容没有变化。
- **Folio 为每个无法读取的文件保留最新的三个副本**；若文件由较新版本的 Folio 保存，它的最新副本从不移除。

### 修复

- **Windows 上，重启或关机后标签页又能恢复了**；被系统结束的 shell 会把窗格留在原处，并加以注明。
- **Windows 上，系统因更新重启后，或在开启了「重启应用」时登录后**，Folio 会带着窗口重新打开；安装程序要求关闭时，Folio 会关闭。
- **Windows 上，开启对比度主题时**，Folio 跟随该主题。
- **Windows 上，Folio 的图标**出现在贴靠布局、Alt+Tab、任务视图、任务栏的预览和 Folio 的消息框中；指针停在最大化按钮上会打开贴靠布局。
- **Windows 上，在所有键盘布局下**，放大镜和表情面板等跟随文本光标的工具都能跟随光标。
- **Windows 上，触摸的松开丢失后，单指滑动又能滚动了**；在读取鼠标的程序上单指滑动时，滚动的距离与手指移动的距离大致相同；在图片或视频上滑动不再缩放图片或改变音量。
- **设置和配置文件的输入框可以用输入法输入**；在一处开始输入的文字，单击别处或切换标签页后不再落到其他地方。
- **在名称、查找栏、搜索框、设置中的输入框、搜索面板或 Git 分支名输入框中单击**，文本光标会放在单击处；按住 Shift 单击和拖动可以选择文字，选区会高亮显示。
- **被其他程序占用的图片或视频**不再被当作 Folio 无法显示的文件；视频播放器启动较慢时，视频也不再被当作无法显示。
- **网址不以 `.pdf` 结尾的 PDF**，在缩放的显示器上不再被拉伸。
- **在没有显卡驱动的 Windows 10 电脑上**，Folio 不再在首次启动或打开设置时关闭。
- **图片两侧的上一张和下一张按钮**在指针悬停时会亮起。
- **开启或关闭 agent 的 hook，以及卸载 Folio**，只改动 agent 设置文件中 Folio 自己的条目；指向已删除的那份 Folio 的 hook，会改为指向这份 Folio。
- **Codex 0.148 及更高版本通过 Codex hook 通知 Folio**，因此 Folio 可以与同样使用 Codex notify 行的其他程序并存。
- **标签页的蓝色圆点**只标记你在别处时结束的 agent 回合，不再出现在每个后台 agent 标签页上。
- **首次运行的欢迎卡片**不再在窗口启动期间读取各 agent 的文件，也只提议添加 Folio 能添加的 hook。
- **窗格响铃时**，即使另一个窗格正在快速输出，Folio 也会立即提醒。
- **Windows 上，程序短暂占用更新的某个文件时**，更新不再失败，说明更改没有保留的卡片也不再重复出现；已删除的那份 Folio 留下的更新条目会被移除。
- **macOS 上，更新在应用旁留下的隐藏文件夹**会在该应用被删除后移除。
- **同时完成的粘贴、复制和保存**按操作的先后顺序处理。
- **Windows 上，旧版 Folio 写入 PowerShell 配置文件的那一行**会改写为当前的写法，文件中的其他内容不变。
- **Windows 上，中止的卸载**不再留下移除程序所用的那份 Folio。
- **Windows 上，从 PowerShell 或命令提示符窗口启动的 Folio**，按「重启以更新」完成更新时，不再可能只留下「更新未完成。」对话框而没有 Folio 窗口。
- **Markdown 页面中有很长的段落时**，打开页面和按键所需的时间随段落长度增长，不再随长度的平方增长。
- **回滚区已满的窗格**接收新行的速度与回滚区填满之前相同；以前约慢二十倍。
- **改变大小后的窗格**不再为它输出的全部内容另存一份副本；程序持续输出时改变窗格大小，公式会在一秒内重新排版。
- **通过旧版 winget 的 `folio` 链接启动时**，关于 ▸ 许可证会打开这份 Folio 自带的许可声明。
- **Folio 因内部错误关闭、关闭过程中再次出错时**，不再把窗口留在屏幕上。

## 已知问题

- **Windows 上的 Folio 0.4.8 无法自行更新到此版本。**为 0.4.8 签名的证书在发布后被吊销，而 0.4.8 在更新前会校验自身的签名，因此卡片显示「更新未通过验证。」，Folio 没有做任何改动。请从本页下载 zip，解压覆盖原来的 Folio 文件夹，设置会保留。用 Scoop 安装的副本，运行 `scoop update folio` 即可。从本版本起，Folio 用自己的发布密钥校验更新，之后的版本又可以在关于页中更新。
- **这个 Windows 版本没有签名**，压缩包名称中的 `-unsigned` 即表示这一点。首次启动前，Windows 会发出警告：SmartScreen 显示「Windows 已保护你的电脑」，发布者显示为「未知发布者」；点击**更多信息** ▸ **仍要运行**即可通过。关于 ▸ 版本会注明这份 Folio 没有签名。本页的发布声明担保这些文件就是发布的文件（见安装说明）。
- **Windows 上的 Folio 0.4.7 和 0.4.6 也无法自行更新到此版本**：它们只接受已签名的压缩包，卡片显示「下载已停止。」，Folio 没有做任何改动。请按第一条的方法手动安装。
- **开启「智能应用控制」时**，Windows 不会启动此版本；请继续使用现有版本。
- **在 Windows 11 上，此版本无法把 Folio 加入资源管理器右键菜单的第一级**，因为 Windows 只注册已签名的包；早先版本加入的菜单项照常可用。
- **部分杀毒软件会拦截或移除新出现的未签名程序**；如遇此情况，请把 Folio 所在的文件夹加入该软件的信任列表。
- 从 0.4.6 更新仍使用 0.4.6 的更新器：在 macOS 上，更新收尾时手动打开 Folio，更新器可能把正常运行的 0.4.10 换回 0.4.6。
- macOS：首次自行更新时，macOS 会提示"已添加可在后台运行的软件"；该登录项是更新器的恢复入口，更新完成后自动移除。
- 0.4.8 之前用 Homebrew 安装的副本，这次更新请用 `brew upgrade`；之后即可在关于页中自行更新。
- Windows，从微软商店安装了 PowerShell 7 时：如果 Folio 恰在启动该 PowerShell 以了解其信息时被外部结束或崩溃，该 PowerShell 可能暂停并一直留在后台，直到在任务管理器中结束它。macOS：Folio 被强制退出或崩溃后，它为了解本机环境而启动的命令会自行运行结束，而不是被停止。

<details>
<summary>安装说明（SmartScreen、Gatekeeper、校验和、发布声明）</summary>

### Windows

| 文件 | 说明 |
| --- | --- |
| `folio-0.4.10-windows-x64-unsigned.zip` | 十个归属文件，打包在一个文件夹中 — `sha256:310064202eea9d7dd6fe5315eba01e464466180b9ad53cac4762649ffa0d0d67` |
| `folio-windows-x64.zip` | 同一份压缩包，名称在各版本间固定不变 — 同一个 `sha256` |
| `SHA256SUMS.txt` | 压缩包两个名称和物料清单的哈希，格式为 `sha256sum -c` 可读 |
| `folio-0.4.10.cdx.json` | CycloneDX 物料清单 |
| `RELEASE-STATEMENT.txt` | 发布声明：本页其余每个文件以及 zip 中每个文件的 SHA-256 和大小 |
| `RELEASE-STATEMENT.txt.sig` | 发布声明的 Ed25519 签名，用 Folio 自己的发布密钥签署 |

将 zip 解压到存放程序的位置，运行 `folio.exe`。没有安装程序；保持解压后的文件在同一个文件夹中，覆盖旧文件夹即可保留设置。需要 **Windows 10 1809 或更高版本，或 Windows 11，64 位**。网页预览需要 **WebView2 Runtime**，Windows 11 已自带，Windows 10 通常也有。

也可以用 [Scoop](https://scoop.sh)：

```powershell
scoop bucket add folio https://github.com/lulu-loopp/scoop-folio
scoop install folio
```

此版本的 `folio.exe` 和 `folio.msix` **没有签名**。首次运行时，SmartScreen 会弹出 **"Windows 已保护你的电脑"**，发布者显示为「未知发布者」：点击**更多信息** ▸ **仍要运行**即可通过。

文件是否就是发布的文件，由发布声明担保。发布声明用 Folio 自己的发布密钥签署，0.4.10 及以后的 Folio 都用这把密钥校验更新；声明列出本页其余每个文件以及 zip 中每个文件的哈希。用普通的 OpenSSL 校验的方法和公钥见 [RELEASING.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.10-preview/docs/RELEASING.md#the-release-statement)。

校验下载文件，将 zip 和 `SHA256SUMS.txt` 放在同一个文件夹中：

```powershell
Get-FileHash folio-0.4.10-windows-x64-unsigned.zip -Algorithm SHA256
```

或者在有 `sha256sum` 的环境——Git Bash、WSL、Linux：

```sh
sha256sum -c SHA256SUMS.txt
```

### macOS

| 文件 | 说明 |
| --- | --- |
| `Folio-0.4.10-macos-arm64.dmg` | 应用程序，已签名并经过 Apple 公证 — `sha256:920b9ab3c17213043f34f3190ff63c30310fb0cd2f6d18039c948a11b50611ee` |
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

完整列表见 [CHANGELOG.md](https://github.com/lulu-loopp/folio-terminal/blob/v0.4.10-preview/CHANGELOG.md#0410-preview--2026-11-25) · [v0.4.8-preview…v0.4.10-preview](https://github.com/lulu-loopp/folio-terminal/compare/v0.4.8-preview...v0.4.10-preview)
