# Shortcuts

<!-- Written from `BINDINGS` in `crates/bt-app/src/shortcuts.rs`, not by hand.
     `scripts/check-shortcuts-table.ps1` turns red when this file and that
     table disagree; `scripts/generate-shortcuts-table.ps1` writes it again. -->

## English

Every key here can be changed on the Shortcuts page in Settings. Changing one writes `keybindings.json` in Folio's settings folder (`%APPDATA%\Folio` on Windows); the last column is the name a row has in that file. The first two columns are one row in the two dialects Folio speaks: an application verb wears Ctrl on Windows and Command on macOS, so that Control is left to the terminal on both.

| Windows | macOS | What it does | Where it works | Name in the file |
| --- | --- | --- | --- | --- |
| Ctrl+Shift+N | Cmd+T | New tab |  | `new-tab` |
| Ctrl+Shift+M | Cmd+N | New window |  | `new-window` |
| Ctrl+Shift+Q | Cmd+Q | Quit |  | `quit` |
| Ctrl+Shift+W | Cmd+W | Close pane |  | `close-pane` |
| Ctrl+Tab | Shift+Cmd+] | Next tab |  | `next-tab` |
| Ctrl+Shift+Tab | Shift+Cmd+[ | Previous tab |  | `prev-tab` |
| Ctrl+Shift+1 | Cmd+1 | Go to tab 1 |  | `goto-tab-1` |
| Ctrl+Shift+2 | Cmd+2 | Go to tab 2 |  | `goto-tab-2` |
| Ctrl+Shift+3 | Cmd+3 | Go to tab 3 |  | `goto-tab-3` |
| Ctrl+Shift+4 | Cmd+4 | Go to tab 4 |  | `goto-tab-4` |
| Ctrl+Shift+5 | Cmd+5 | Go to tab 5 |  | `goto-tab-5` |
| Ctrl+Shift+6 | Cmd+6 | Go to tab 6 |  | `goto-tab-6` |
| Ctrl+Shift+7 | Cmd+7 | Go to tab 7 |  | `goto-tab-7` |
| Ctrl+Shift+8 | Cmd+8 | Go to tab 8 |  | `goto-tab-8` |
| Ctrl+Shift+9 | Cmd+9 | Go to tab 9 |  | `goto-tab-9` |
| Ctrl+Shift+T | Shift+Cmd+T | Reopen the last closed tab |  | `reopen-closed` |
| Ctrl+Shift+A | Shift+Cmd+A | Jump to the longest waiting pane |  | `jump-attention` |
| Ctrl+Shift+P | Shift+Cmd+P | Command palette |  | `command-palette` |
| Ctrl+Shift+Z | Shift+Cmd+E | Cards |  | `focus-mode` |
| Alt+Shift+- | Shift+Cmd+D | Split horizontally |  | `split-horizontal` |
| Alt+Shift+= | Cmd+D | Split vertically |  | `split-vertical` |
| Ctrl+Shift+D | Shift+Cmd+U | Duplicate pane into a split |  | `duplicate-pane-split` |
| Ctrl+Shift+X | Shift+Cmd+X | Zoom pane |  | `zoom-pane` |
| Ctrl+Shift+B | Shift+Cmd+B | Files column |  | `files-pane` |
| Ctrl+Shift+G | Shift+Cmd+R | Show Git in the files column |  | `git-page` |
| Ctrl+, | Cmd+, | Settings |  | `open-settings` |
| Ctrl+S | Cmd+S | Save the open document | In a preview | `save-preview` |
| Ctrl+Z | Cmd+Z | Undo the last edit | In a preview | `undo-preview` |
| Ctrl+Y | Shift+Cmd+Z | Redo the last edit | In a preview | `redo-preview` |
| Ctrl+Shift+↑ | Shift+Cmd+↑ | Previous command | On a terminal's own scrollback | `prev-command-mark` |
| Ctrl+Shift+↓ | Shift+Cmd+↓ | Next command | On a terminal's own scrollback | `next-command-mark` |
| Ctrl+F | Cmd+F | Find in this pane | Where there is text to search | `open-search` |
| F3 | Cmd+G | Next match | While the search is open | `next-match` |
| Shift+F3 | Shift+Cmd+G | Previous match | While the search is open | `prev-match` |
| Esc | Esc | Close search | While the search is open | `close-search` |
| Ctrl+L | Cmd+L | Address | On a page | `web-address` |
| Ctrl+Shift+L | Shift+Cmd+L | Open an address in a new preview |  | `window-address` |
| F12 | Shift+Cmd+I | Developer tools | On a page | `web-devtools` |
| Win+` | Not set | Summon the terminal |  | `summon-quake` |

## 中文

下面每一组键都能在设置的快捷键页里改。改过之后写进 Folio 设置目录里的 `keybindings.json`（Windows 上是 `%APPDATA%\Folio`），最后一列就是这一行在那个文件里的名字。前两列是同一行的两种说法：窗口自己的动作在 Windows 上按 Ctrl，在 macOS 上按 Command，两边都把 Control 留给终端。

| Windows | macOS | 作用 | 在哪里生效 | 文件里的名字 |
| --- | --- | --- | --- | --- |
| Ctrl+Shift+N | Cmd+T | 新建标签 |  | `new-tab` |
| Ctrl+Shift+M | Cmd+N | 新建窗口 |  | `new-window` |
| Ctrl+Shift+Q | Cmd+Q | 退出 |  | `quit` |
| Ctrl+Shift+W | Cmd+W | 关闭窗格 |  | `close-pane` |
| Ctrl+Tab | Shift+Cmd+] | 下一个标签 |  | `next-tab` |
| Ctrl+Shift+Tab | Shift+Cmd+[ | 上一个标签 |  | `prev-tab` |
| Ctrl+Shift+1 | Cmd+1 | 转到标签 1 |  | `goto-tab-1` |
| Ctrl+Shift+2 | Cmd+2 | 转到标签 2 |  | `goto-tab-2` |
| Ctrl+Shift+3 | Cmd+3 | 转到标签 3 |  | `goto-tab-3` |
| Ctrl+Shift+4 | Cmd+4 | 转到标签 4 |  | `goto-tab-4` |
| Ctrl+Shift+5 | Cmd+5 | 转到标签 5 |  | `goto-tab-5` |
| Ctrl+Shift+6 | Cmd+6 | 转到标签 6 |  | `goto-tab-6` |
| Ctrl+Shift+7 | Cmd+7 | 转到标签 7 |  | `goto-tab-7` |
| Ctrl+Shift+8 | Cmd+8 | 转到标签 8 |  | `goto-tab-8` |
| Ctrl+Shift+9 | Cmd+9 | 转到标签 9 |  | `goto-tab-9` |
| Ctrl+Shift+T | Shift+Cmd+T | 重新打开最近关闭的标签 |  | `reopen-closed` |
| Ctrl+Shift+A | Shift+Cmd+A | 跳到等待最久的窗格 |  | `jump-attention` |
| Ctrl+Shift+P | Shift+Cmd+P | 搜索面板 |  | `command-palette` |
| Ctrl+Shift+Z | Shift+Cmd+E | 卡片 |  | `focus-mode` |
| Alt+Shift+- | Shift+Cmd+D | 横向拆分 |  | `split-horizontal` |
| Alt+Shift+= | Cmd+D | 竖向拆分 |  | `split-vertical` |
| Ctrl+Shift+D | Shift+Cmd+U | 复制窗格并拆分 |  | `duplicate-pane-split` |
| Ctrl+Shift+X | Shift+Cmd+X | 放大窗格 |  | `zoom-pane` |
| Ctrl+Shift+B | Shift+Cmd+B | 文件列 |  | `files-pane` |
| Ctrl+Shift+G | Shift+Cmd+R | 文件列切到 Git |  | `git-page` |
| Ctrl+, | Cmd+, | 设置 |  | `open-settings` |
| Ctrl+S | Cmd+S | 保存打开的文档 | 在预览里 | `save-preview` |
| Ctrl+Z | Cmd+Z | 撤销上次改动 | 在预览里 | `undo-preview` |
| Ctrl+Y | Shift+Cmd+Z | 重做上次改动 | 在预览里 | `redo-preview` |
| Ctrl+Shift+↑ | Shift+Cmd+↑ | 上一条命令 | 在终端回滚区中 | `prev-command-mark` |
| Ctrl+Shift+↓ | Shift+Cmd+↓ | 下一条命令 | 在终端回滚区中 | `next-command-mark` |
| Ctrl+F | Cmd+F | 在本窗格中查找 | 有内容可搜索的地方 | `open-search` |
| F3 | Cmd+G | 下一处匹配 | 查找打开时 | `next-match` |
| Shift+F3 | Shift+Cmd+G | 上一处匹配 | 查找打开时 | `prev-match` |
| Esc | Esc | 关闭搜索 | 查找打开时 | `close-search` |
| Ctrl+L | Cmd+L | 地址 | 在网页里时 | `web-address` |
| Ctrl+Shift+L | Shift+Cmd+L | 打开地址 |  | `window-address` |
| F12 | Shift+Cmd+I | 开发者工具 | 在网页里时 | `web-devtools` |
| Win+` | 未设置 | 唤出终端 |  | `summon-quake` |
