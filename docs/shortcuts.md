# Shortcuts

<!-- Written from `BINDINGS` in `crates/bt-app/src/shortcuts.rs`, not by hand.
     `scripts/check-shortcuts-table.ps1` turns red when this file and that
     table disagree; `scripts/generate-shortcuts-table.ps1` writes it again. -->

## English

Every key here can be changed on the Shortcuts page in Settings. Changing one writes `%APPDATA%\Folio\keybindings.json`; the last column is the name a row has in that file.

| Key | What it does | Where it works | Name in the file |
| --- | --- | --- | --- |
| Ctrl+Shift+N | New tab |  | `new-tab` |
| Ctrl+Shift+M | New window |  | `new-window` |
| Ctrl+Shift+Q | Quit |  | `quit` |
| Ctrl+Shift+W | Close pane |  | `close-pane` |
| Ctrl+Tab | Next tab |  | `next-tab` |
| Ctrl+Shift+Tab | Previous tab |  | `prev-tab` |
| Ctrl+Shift+1 | Go to tab 1 |  | `goto-tab-1` |
| Ctrl+Shift+2 | Go to tab 2 |  | `goto-tab-2` |
| Ctrl+Shift+3 | Go to tab 3 |  | `goto-tab-3` |
| Ctrl+Shift+4 | Go to tab 4 |  | `goto-tab-4` |
| Ctrl+Shift+5 | Go to tab 5 |  | `goto-tab-5` |
| Ctrl+Shift+6 | Go to tab 6 |  | `goto-tab-6` |
| Ctrl+Shift+7 | Go to tab 7 |  | `goto-tab-7` |
| Ctrl+Shift+8 | Go to tab 8 |  | `goto-tab-8` |
| Ctrl+Shift+9 | Go to tab 9 |  | `goto-tab-9` |
| Ctrl+Shift+T | Reopen the last closed tab |  | `reopen-closed` |
| Ctrl+Shift+A | Jump to the longest waiting pane |  | `jump-attention` |
| Ctrl+Shift+P | Command palette | Bound, but the action behind it is not built yet | `command-palette` |
| Ctrl+Shift+Z | Cards |  | `focus-mode` |
| Alt+Shift+- | Split horizontally |  | `split-horizontal` |
| Alt+Shift+= | Split vertically |  | `split-vertical` |
| Ctrl+Shift+D | Duplicate pane into a split |  | `duplicate-pane-split` |
| Ctrl+Shift+X | Zoom pane |  | `zoom-pane` |
| Ctrl+Shift+B | Files column |  | `files-pane` |
| Ctrl+Shift+G | Turn the files column to Git |  | `git-page` |
| Ctrl+, | Settings |  | `open-settings` |
| Ctrl+S | Save the open document | In a preview | `save-preview` |
| Ctrl+Shift+↑ | Previous command | On a terminal's own scrollback | `prev-command-mark` |
| Ctrl+Shift+↓ | Next command | On a terminal's own scrollback | `next-command-mark` |
| Ctrl+F | Find in this pane | Where there is text to search | `open-search` |
| F3 | Next match | While the search is open | `next-match` |
| Shift+F3 | Previous match | While the search is open | `prev-match` |
| Esc | Close search | While the search is open | `close-search` |
| Ctrl+L | Address | On a page | `web-address` |
| Ctrl+Shift+L | Open address |  | `window-address` |
| F12 | Developer tools | On a page | `web-devtools` |
| Not set | Summon picture in picture 1 |  | `summon-pip-1` |
| Not set | Summon picture in picture 2 |  | `summon-pip-2` |
| Not set | Summon picture in picture 3 |  | `summon-pip-3` |
| Not set | Summon picture in picture 4 |  | `summon-pip-4` |

## 中文

下面每一组键都能在设置的快捷键页里改。改过之后写进 `%APPDATA%\Folio\keybindings.json`，最后一列就是这一行在那个文件里的名字。

| 按键 | 作用 | 在哪里生效 | 文件里的名字 |
| --- | --- | --- | --- |
| Ctrl+Shift+N | 新建标签 |  | `new-tab` |
| Ctrl+Shift+M | 新建窗口 |  | `new-window` |
| Ctrl+Shift+Q | 退出 |  | `quit` |
| Ctrl+Shift+W | 关闭窗格 |  | `close-pane` |
| Ctrl+Tab | 下一个标签 |  | `next-tab` |
| Ctrl+Shift+Tab | 上一个标签 |  | `prev-tab` |
| Ctrl+Shift+1 | 转到标签 1 |  | `goto-tab-1` |
| Ctrl+Shift+2 | 转到标签 2 |  | `goto-tab-2` |
| Ctrl+Shift+3 | 转到标签 3 |  | `goto-tab-3` |
| Ctrl+Shift+4 | 转到标签 4 |  | `goto-tab-4` |
| Ctrl+Shift+5 | 转到标签 5 |  | `goto-tab-5` |
| Ctrl+Shift+6 | 转到标签 6 |  | `goto-tab-6` |
| Ctrl+Shift+7 | 转到标签 7 |  | `goto-tab-7` |
| Ctrl+Shift+8 | 转到标签 8 |  | `goto-tab-8` |
| Ctrl+Shift+9 | 转到标签 9 |  | `goto-tab-9` |
| Ctrl+Shift+T | 重新打开最近关闭的标签 |  | `reopen-closed` |
| Ctrl+Shift+A | 跳到等待最久的窗格 |  | `jump-attention` |
| Ctrl+Shift+P | 命令面板 | 已绑定，但它背后的功能还没有做出来 | `command-palette` |
| Ctrl+Shift+Z | 卡片 |  | `focus-mode` |
| Alt+Shift+- | 横向拆分 |  | `split-horizontal` |
| Alt+Shift+= | 竖向拆分 |  | `split-vertical` |
| Ctrl+Shift+D | 复制窗格到拆分里 |  | `duplicate-pane-split` |
| Ctrl+Shift+X | 放大窗格 |  | `zoom-pane` |
| Ctrl+Shift+B | 文件列 |  | `files-pane` |
| Ctrl+Shift+G | 文件列切到 Git |  | `git-page` |
| Ctrl+, | 设置 |  | `open-settings` |
| Ctrl+S | 保存打开的文档 | 在预览里 | `save-preview` |
| Ctrl+Shift+↑ | 上一条命令 | 在终端自己的回滚里 | `prev-command-mark` |
| Ctrl+Shift+↓ | 下一条命令 | 在终端自己的回滚里 | `next-command-mark` |
| Ctrl+F | 在本窗格中查找 | 有正文可查找的地方 | `open-search` |
| F3 | 下一处匹配 | 查找打开时 | `next-match` |
| Shift+F3 | 上一处匹配 | 查找打开时 | `prev-match` |
| Esc | 关闭搜索 | 查找打开时 | `close-search` |
| Ctrl+L | 地址 | 在网页里时 | `web-address` |
| Ctrl+Shift+L | 打开地址 |  | `window-address` |
| F12 | 开发者工具 | 在网页里时 | `web-devtools` |
| 未设置 | 唤出画中画 1 |  | `summon-pip-1` |
| 未设置 | 唤出画中画 2 |  | `summon-pip-2` |
| 未设置 | 唤出画中画 3 |  | `summon-pip-3` |
| 未设置 | 唤出画中画 4 |  | `summon-pip-4` |
