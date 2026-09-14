# Folio 的功能

首页的详细版。[`README.zh-CN.md`](../README.zh-CN.md) 是简要版；[`install.zh-CN.md`](install.zh-CN.md) 介绍安装和初次启动。

下文列出的快捷键为 Windows 版本。[快捷键](shortcuts.md)中有两列对照：Mac 上应用操作使用 **Command** 而非 **Ctrl**，这样两个平台上 **Control** 都留给了终端。

## 终端中的 LaTeX 排版

命令输出中的 LaTeX 在打印位置直接排版。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/terminal-math-dark.png">
  <img src="screenshots/terminal-math-light.png" width="100%"
       alt="一个终端窗格显示一条命令的排版输出：段落中穿插行内公式，另有
       三个展示公式分占独立行——高斯归一化积分、傅里叶变换对和指数级数。">
</picture>

- 命令输出中的 `$…$` 和 `$$…$$` 在打印所在行排版。
- 预览窗格还支持 `\(…\)`、`\[…\]` 和 `amsmath` 环境。
- 不支持的语法保持原样显示。
- 行内 `$…$` 通过 shell 整合与 shell 变量区分——Windows 上是 PowerShell，Mac 上是 zsh 或 bash。未启用整合时行内公式保持原文，`$$…$$` 块仍正常排版。

## 为 agent 而设计

等待你操作的 agent 会在标签页上标记，无需逐个检查。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/settings-agents-dark.png">
  <img src="screenshots/settings-agents-light.png" width="100%"
       alt="设置中的 Agent 页：Claude Code、Codex 和 GitHub Copilot CLI
       各一行，每行说明开关写入哪个文件，三个开关均关闭。末尾是回合
       结束通知选项。">
</picture>

- 等待中的 agent 在标签页上亮起一个圆点。其他程序在前台时，Windows 闪烁任务栏按钮，macOS 弹跳 Dock 图标；窗口最小化或在其他桌面时，发送系统通知。Mac 上首次发送通知时 macOS 会询问是否允许——回答一次即可，如果拒绝，Agent 页会显示此状态。
- 每次请求最多提醒一次，回应后或程序撤回请求后标记消失。`Ctrl+Shift+A` 跳转到等待最久的 agent。
- Claude Code、Codex 和 GitHub Copilot CLI 在设置的 Agent 页各有一个开关，开启时向对应工具的配置文件写入通知钩子，关闭时移除。默认不安装。
- 七个配置自带 agent 启动——Claude Code、Codex、Copilot CLI、Kimi Code、pi、Hermes、OpenCode——在 `PATH` 上查找；Windows 上装在 WSL 内的从 WSL 配置启动。任何程序写入 `OSC 1337;RequestAttention=yes` 即可触发标签页标记。

## 终端旁的预览：文件、PDF、视频、网页

文件内容可在终端旁直接查看，无需打开其他程序。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/preview-pdf-dark.png">
  <img src="screenshots/preview-pdf-light.png" width="100%"
       alt="鼠标停在文件列的一个文件名上，下方弹出卡片显示 PDF 第一页，
       以及页数和文件大小。滚轮可翻页。">
</picture>

- 鼠标悬停在文件列的文件名上时弹出卡片：PDF 逐页显示，视频直接播放，文本显示前几行，图片直接展示。
- 预览窗格打开文件：Markdown 排版显示，PDF 逐页翻阅，视频播放，网页带地址栏和后退按钮。
- 终端输出的路径点击后在预览窗格打开，`Ctrl`+点击交给系统默认程序——两个平台上都是 `Ctrl`，Mac 上不是 Command。没有标记的裸路径只要文件确认存在，同样识别。
- 网址同理：点击在预览窗格打开，`Ctrl`+点击交给浏览器。
- 一个窗口可以打开与预览窗格数量相同的页面。打开第二个页面时使用新窗格而非覆盖第一个，锁定页面后再打开新页面可并排显示。拖拽页面到指定窗格则在该窗格打开。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="../assets/readme/surfaces-dark.png">
  <img src="../assets/readme/surfaces-light.png" width="100%"
       alt="另外三种界面：以卡片布局显示的窗口、预览窗格中排版的 Markdown
       文档、预览窗格中的网页（上方有面包屑地址栏）。">
</picture>

## 在阅读的地方编辑 Markdown

`.md` 文件就在阅读它的那一页上输入，用的还是阅读时的字体。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/preview-markdown-dark.png">
  <img src="screenshots/preview-markdown-light.png" width="100%"
       alt="终端旁的预览窗格中排版显示的 Markdown 文档：标题、表格、
       代码块和展示公式同时可见，都用阅读时的字体。">
</picture>

- 点进一个段落、标题、列表或引用，这一处就显示出自己的 Markdown——标题的 `#`、粗体两侧的 `**`、列表项前的 `- `、引用左侧的 `> `——字体和字号与阅读时相同，页面上其余内容仍保持排版后的样子。光标移开，它就重新排版。
- 代码块、表格和公式则换成等宽源码，因为这些地方字符怎么对齐本身就是内容的一部分。
- 光标在段落之间移动和在行之间移动一样，不需要进入或退出编辑状态：方向键、`Home`、`End`、`Enter` 和 `Backspace` 的行为与别处相同。选中什么就复制什么，从正在编辑的文档中复制会带上原有的标记，粘贴回去还是原来那些字。
- `Ctrl+S` 写入文件，`Ctrl+Z` 撤销上一次改动，`Ctrl+Y` 重做。连续输入的一段按一次就撤回，不是一个字母一个字母退；撤销回到上次保存的位置时，未保存的圆点随之消失。
- 保存只改动编辑过的部分，文件其余内容逐字节保持原样。开头几个字节声明了自身编码的文件——Windows PowerShell 写出的文件最常见——按原编码写回。换行符、行尾空格、末尾缺少的换行都照旧保留。
- `Esc` 退出这一页，点击正文旁的空白处或点到窗格之外也一样。页面回到阅读状态，光标停在离开时的位置。
- 超过 8 MB 的文件，以及无法整篇按文本读入的文件，照常打开、阅读和复制，但不能编辑；窗格底部说明原因。
- 文件列的右键菜单就地新建文件和文件夹：`New file…`（新建文件）和 `New folder…`（新建文件夹）在树中新行将要出现的位置放一个名称输入框，`Enter` 创建，`Esc` 取消。文件夹不接受的名称就在输入框中变红，不另开提示。`Delete`（删除）把文件或整个文件夹送进 Windows 的回收站或 Mac 的废纸篓，不先询问。
- 在文件列的空白处点右键，打开的是该列当前所在文件夹的菜单，空文件夹也能建出第一个文件。
- `Ctrl+Shift+P` 在文件列当前所在的文件夹下查找文件，`Enter` 在预览窗格中打开，可直接输入。

## 窗格、标签页，自由移动

布局随时调整，内部的会话保持运行，所有窗格同时可见。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/tab-into-pane-dark.gif">
  <img src="screenshots/tab-into-pane-light.gif" width="100%"
       alt="按住标签页向下拖出标签栏，窗口右半部分出现落点预览，松开后
       该标签页的 shell 变为右侧窗格并保持运行。随后将新窗格的标题栏
       拖到底部边缘，并排布局变为上下两栏。">
</picture>

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/cards-dark.png">
  <img src="screenshots/cards-light.png" width="100%"
       alt="标签栏变为卡片列。单张卡片代表一个包含八个窗格的标签页，以
       缩略图显示全部八个窗格。Alt 加滚轮可逐行滚动卡片内容。">
</picture>

- `Alt+Shift+-` 水平拆分窗格，`Alt+Shift+=` 垂直拆分。标签页或单个窗格可拖出为独立窗口，未拖动的窗格保持原有宽度。
- 窗格拖到两个标签页之间会成为新标签页：列表打开一个空位，窗格停在那里。拖到标签页上则加入该标签页的布局。水平标签栏、竖直标签栏和卡片列均支持此操作。
- `Ctrl+Shift+Z` 将标签栏切换为卡片列，每张卡片以缩略图显示对应标签页的窗格布局。
- `Ctrl+Shift+G` 将文件列切换为 Git 面板：分支、工作区、暂存与未暂存文件、提交图，选中文件的 diff 在预览窗格中显示。
- `Ctrl+Shift+↑` 和 `Ctrl+Shift+↓` 在滚动历史中逐条跳转命令，失败的命令有失败标记。
- 文件列上方的文件夹按钮列出各 shell 当前所在目录和最近五个访问过的目录（标注 `recent`）。

## 快捷终端

一个快捷键调出终端覆盖在屏幕上方，再按一次收回。

**快捷键因平台而异**，向系统注册而非依赖窗口焦点，因此在任何应用内都能响应：Windows 上是 ``Win+` ``，Mac 上是 ``⌃` ``。在快捷键页可以录入其他组合，立即生效；**恢复默认**可还原。两个平台均无需申请权限。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/quake-dark.png">
  <img src="screenshots/quake-light.png" width="100%"
       alt="屏幕顶部悬挂的终端窗口，略低于屏幕边缘并居中，覆盖在一个
       资源管理器窗口上。终端有一个标签页、齿轮按钮和关闭按钮，shell
       中显示四条提交记录和目录列表，下方是等待输入的空行。">
</picture>

- ``Win+` ``（Mac 上是 ``⌃` ``）在鼠标所在屏幕顶部拉下一个终端窗口，覆盖当前内容。再按一次收回窗口并将焦点还给之前的程序。
- 这是完整的 Folio——标签页、窗格、文件列、预览、所有快捷键都可用。shell 和滚动历史在每次呼出之间保持。
- 手动移动或调整过的窗口按显示器记忆，下次在该屏幕呼出时沿用。
- 快捷终端随 Folio 启停。关闭最后一个可见窗口即结束运行。
- **设置 > 快捷终端**可配置快捷键、新标签页配置、高度、宽度、顶部间距、失焦自动隐藏，以及每次启动首次呼出时运行的命令。
- 固定标签页在下次启动时可恢复上条命令到输入行——只填入，不运行。

## 搜索一切

一个搜索框同时查五类内容，`Enter` 直达目标。

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/palette-dark.png">
  <img src="screenshots/palette-light.png" width="100%"
       alt="窗口顶部浮动的搜索框，输入栏中有查询文字，下方结果分五个
       区域且不混排：一个操作、一个窗格、一条已执行的命令、一个文件、
       一个设置项。第一行高亮，每行中匹配的字母有标记。">
</picture>

- `Ctrl+Shift+P` 打开搜索面板，分五个区域：Folio 可执行的操作、当前窗口的窗格和标签页、执行过的命令、文件列所在文件夹下的文件、设置项。
- 输入文字同时筛选五个区域，方向键在结果间移动。`Enter` 可切换到窗格、在预览窗格打开文件、打开对应设置，或直接执行操作。
- 仍在运行的命令以窗格边框高亮指示，而非滚动到已过去的行。
- 文件搜索覆盖文件列当前所在的文件夹。

## Windows 集成

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/main-window-dark.png">
  <img src="screenshots/main-window-light.png" width="100%"
       alt="默认字体和配色的窗口：左侧文件列，中间两个终端窗格并排，
       右侧预览窗格中打开了一个 Markdown 文档。">
</picture>

- **设置 > General > 资源管理器菜单**：打开时写入两个注册表键到 `HKEY_CURRENT_USER\Software\Classes`，加入「在 Folio 中打开」。在 Windows 11 上这项在「显示更多选项」页；在 Windows 10 上它在唯一的菜单中。如果 Windows 11 的文件夹里有 `folio.msix`，则同时注册该包到当前账户，使菜单项出现在第一页。无需管理员。关闭时移除已注册的项。正在运行的资源管理器只在启动时读取第一页的条目，如果「在 Folio 中打开」还没有出现，注销后重新登录。该菜单项在上次使用的窗口中将文件夹打开为标签页，并将窗口带到前台。从任务栏、快捷方式或 folio.exe 再次启动默认开新窗口，可在同页**再次启动 Folio** 行改为标签页。
- Windows PowerShell 5.1 自带的 PSReadLine 2.0.0 在窗口缩放后会错位输入行。Folio 附带修补版 2.4.6，可按需安装到用户模块目录。执行策略为 `Restricted` 时开关会提示，并给出对应的 `Set-ExecutionPolicy` 命令。

## macOS 集成

- **Finder 右键菜单**的**服务**下有 **Open in Folio**。Folio 首次运行时自动注册，无需手动开启，也不需要注销。在文件夹上点击时打开一个标签页进入该文件夹；在文件上点击时进入文件所在的文件夹。两者都在上次使用的窗口中打开，不会启动第二个 Folio。
- **菜单栏就是快捷键表。** 每一项的快捷键取自[快捷键](shortcuts.md)表中的同一行，一个操作在任何地方都是同一个名称和同一个快捷键。
- **Dock 图标是提醒通道。** 等待中的 agent 弹跳 Dock 图标直到切回；报告进度的命令将进度显示为图标角标。
- **路径从主目录起写。** 主目录下的文件在文件列中显示为 `~ › …`，`~` 本身也可以点击。
- **三项设置在 Mac 上不存在**——资源管理器菜单、PowerShell 整合和 PSReadLine 修复，因为 macOS 没有对应功能。Mac 独有一项：**Option 键发送 Alt**，默认关闭，保持 Option 键输入其印刷字符的行为。
- **macOS 上没有的功能：** 右键菜单第一页的签名包注册，以及 Store 视频扩展。当前预览版 macOS 仅支持 **arm64**。

## Visual Studio Code

**Windows.** 压缩包中 `folio.exe` 旁的 `folio-here.cmd` 只有一行：

```bat
@"%~dp0folio.exe" --from-here --cwd "%CD%"
```

在 VS Code 中将外部终端指向该文件——通过设置界面或 `settings.json`：

```json
"terminal.external.windowsExec": "C:\\Tools\\folio\\folio-here.cmd"
```

之后 **Terminal > Open in External Terminal**（`Ctrl+Shift+C`）即可在编辑器当前目录打开 Folio。该 `.cmd` 文件向 Folio 传入 `--cwd` 和 `--from-here`，因为此设置不向程序传递参数。`--from-here` 表示在某个文件夹打开终端，不是再启动一个 Folio。无论**设置 > General > 再次启动 Folio** 怎样选，都在上次使用的窗口里开标签页。

macOS 上该设置指向应用而非命令，因此没有 `folio-here` 可用。把文件夹交给 shell 的方式是上文提到的 Finder 右键菜单 **Open in Folio**。

## 中英双语

界面中所有文字都有中英两种语言，随时可切换。

- **设置 > 通用 > 语言**提供英文、中文和跟随系统三个选项，切换后所有窗格、菜单和对话框立即生效，无需重启。
- 首页同样有两份：[`README.md`](../README.md) 和 [`README.zh-CN.md`](../README.zh-CN.md)，本文档也有[英文版](features.md)。
- 快捷键表、设置描述和窗格中的提示信息都同时提供两种语言，切换后不会出现一行中文夹一行英文的情况。
