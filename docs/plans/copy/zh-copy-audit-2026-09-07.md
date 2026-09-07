# Chinese copy audit — 2026-09-07

> Reviewed by: Claude Opus 4.6 (claude-opus-4-6\[1m\])
> Reference voice: `README.zh-CN.md` (main, 5792bda)
> Style rules: `docs/plans/copy/zh-style-notes.md`, `docs/plans/copy/user-facing-copy-guide.md` sections 1, 2, 4

## Summary

| Metric | Count |
| --- | --- |
| Strings reviewed (i18n.rs `Text` enum) | 285 |
| Strings reviewed (i18n.rs format functions) | 73 |
| Strings reviewed (PRIVACY.md Chinese half) | 1 section (~30 items) |
| Strings reviewed (release note 0.2.2 Chinese half) | 1 section (~15 items) |
| Strings reviewed (shortcuts.md Chinese half) | 1 section (generated from i18n) |
| Strings reviewed (README.zh-CN.md) | 1 document |
| Total proposals to change | 52 |
| Unchanged (pass) | ~310 |

### Five most common faults

| # | Fault | Count | Example |
| --- | --- | --- | --- |
| 1 | 防御句 / excess reassurance | 14 | `DescUpdateCheck`: "不下载任何内容" — the defensive negative "it downloads nothing" adds nothing when the row is about checking, not downloading |
| 2 | 机制暴露 / internal mechanism | 11 | `DescQuakeHotkey`: "它向 Windows 认领" — unnatural metaphor for hotkey registration |
| 3 | 行话 / jargon the user ruling bans | 8 | `DescQuakeRestore`: "只填在提示符后" — should use 输入行 instead of 提示符 per user ruling |
| 4 | 翻译腔 / translationese | 10 | `WebFailDownloadSay`: "请求里带着普通链接重放不出来的东西" — mirrors English clause structure, reads unnaturally |
| 5 | 术语不统一 / terminology drift | 9 | `RowQuakeProfile` says "使用的 profile" while the rest of the product says 配置/配置文件 |

---

## Surface 1: i18n.rs — `Text` enum (static strings)

### Window chrome

| id | current | proposed | why |
| --- | --- | --- | --- |
| `Settings` | 设置 | — | 无需改 |
| `ToggleSidebar` | 切换侧栏 | — | 无需改 |
| `Minimize` | 最小化 | — | 无需改 |
| `Maximize` | 最大化 | — | 无需改 |
| `CloseWindow` | 关闭 | — | 无需改 |

### Tab strip and rail

| id | current | proposed | why |
| --- | --- | --- | --- |
| `RailTabs` | 标签 | — | 无需改 |
| `RailNewTab` | 新建标签 | — | 无需改 |
| `ChooseProfile` | 选择配置文件 | — | 无需改 |
| `Pin` | 固定 | — | 无需改 |
| `Unpin` | 取消固定 —— 固定的标签要先取消固定才能关闭 | — | 无需改 |

### Tab tips

| id | current | proposed | why |
| --- | --- | --- | --- |
| `NameSourceManual` | 你起的名字 | — | 无需改 |
| `NameSourceProgram` | 程序设置的 | — | 无需改 |
| `NameSourceCwd` | 工作目录 | — | 无需改 |
| `TabTipPinned` | \n已固定 —— 下次启动会恢复 | — | 无需改 |

### Mark slot

| id | current | proposed | why |
| --- | --- | --- | --- |
| `MarkWorking` | 运行中 | — | 无需改 |
| `MarkWorkingIndeterminate` | 运行中… | — | 无需改 |

### Pane heads

| id | current | proposed | why |
| --- | --- | --- | --- |
| `SeatTerminal` | 终端 | — | 无需改 |
| `SeatFiles` | 文件 | — | 无需改 |
| `SeatPreview` | 预览 | — | 无需改 |
| `SeatUnavailable` | 不可用 | — | 无需改 |
| `PlaceholderSeatNotice` | 这个窗格由更新版本的 Folio 保存 | — | 无需改 |

### Settings dialog — categories and nav

| id | current | proposed | why |
| --- | --- | --- | --- |
| `CategoryGeneral` | 常规 | — | 无需改 |
| `CategoryAppearance` | 外观 | — | 无需改 |
| `CategorySummonedTerminal` | 快捷终端 | — | 无需改 |
| `CategoryTerminal` | 终端 | — | 无需改 |
| `CategoryRenderedBlocks` | 渲染块 | — | 无需改 |
| `CategoryShortcuts` | 快捷键 | — | 无需改 |
| `NavGeneral` | 常规 | — | 无需改 |
| `NavAppearance` | 外观 | — | 无需改 |
| `NavSummonedTerminal` | 快捷终端 | — | 无需改 |
| `NavTerminal` | 终端 | — | 无需改 |
| `NavRenderedBlocks` | 渲染块 | — | 无需改 |
| `NavShortcuts` | 快捷键 | — | 无需改 |

### Settings — row titles

| id | current | proposed | why |
| --- | --- | --- | --- |
| `RowTheme` | 主题 | — | 无需改 |
| `RowCursor` | 光标 | — | 无需改 |
| `RowFormulas` | 行间公式 | — | 无需改 |
| `RowInlineFormulas` | 行内公式 | — | 无需改 |
| `RowGitPanel` | Git 面板 | — | 无需改 |
| `RowUpdateCheck` | 检查新版 | — | 无需改 |
| `RowContextMenu` | 资源管理器菜单 | — | 无需改 |
| `RowTabLayout` | 标签布局 | — | 无需改 |
| `RowSidebar` | 侧栏 | — | 无需改 |
| `RowSplitDirection` | 拆分方向 | — | 无需改 |
| `RowDefaultProfile` | 默认配置文件 | — | 无需改 |
| `RowLanguage` | 语言 | — | 无需改 |
| `RowTerminalFont` | 终端字体 | — | 无需改 |
| `RowFontSize` | 字号 | — | 无需改 |
| `RowPsReadLine` | PSReadLine 补丁 | — | 无需改 |
| `RowLightScheme` | 浅色配色 | — | 无需改 |
| `RowDarkScheme` | 深色配色 | — | 无需改 |
| `RowBackgroundImage` | 背景图片 | — | 无需改 |
| `RowImageFit` | 图片适配 | — | 无需改 |
| `RowImageOpacity` | 图片不透明度 | — | 无需改 |
| `RowBackgroundOpacity` | 背景不透明度 | — | 无需改 |
| `RowAcrylic` | 亚克力 | — | 无需改 |
| `RowAlwaysOnTop` | 总在最前 | — | 无需改 |
| `RowTables` | 表格 | — | 无需改 |
| `RowBlockMaxHeight` | 最大高度 | — | 无需改 |
| `RowScrollback` | 回滚 | — | 无需改 |
| `RowLineWrapping` | 自动折行 | — | 无需改 |
| `RowFocusMode` | 卡片 | — | 无需改 |
| `RowMinimumContrast` | 最小对比度 | — | 无需改 |
| `RowNotifications` | 通知 | — | 无需改 |
| `RowTurnEndNotifications` | 回合结束也提醒 | — | 无需改 |
| `RowKeyHints` | 快捷键提示 | — | 无需改 |
| `RowCopyOnSelect` | 选中即复制 | — | 无需改 |
| `RowSearchEngine` | 搜索引擎 | — | 无需改 |
| `RowPowerShellOffer` | PowerShell 整合提示 | — | 无需改 |
| `RowClaudeHooks` | Claude Code 通知 | — | 无需改 |
| `RowCodexNotify` | Codex 通知 | — | 无需改 |
| `RowCopilotHooks` | Copilot CLI 通知 | — | 无需改 |
| `RowFocusCardHeight` | 卡片高度 | — | 无需改 |

### Settings — descriptions (the main audit target)

| id | current | proposed | why |
| --- | --- | --- | --- |
| `DescTheme` | 浅色、深色，或跟随系统设置 | — | 无需改 |
| `DescCursor` | 你正在输入的那个窗格里，光标是什么形状。 | 光标在当前输入窗格中的形状。 | 翻译腔：句子为了照搬英语定冠词结构而变得冗长 |
| `DescFormulas` | 排版命令输出里的 $$…$$ 块。关闭时显示 LaTeX 源码原文。 | — | 无需改 |
| `DescInlineFormulas` | 排版命令输出里的 $…$。关闭时显示源码原文。 | — | 无需改 |
| `DescUpdateCheck` | 每天问一次发布页有没有更新的版本。不下载任何内容。 | 每天查一次有没有新版本，有的话在设置中提示。 | 防御句："不下载任何内容"是多余的否定；英文同样需要改 |
| `DescGitPanel` | 在文件列里加一页 Git。关闭时，Folio 不会读取任何仓库。 | 在文件列中加一页 Git。关闭时不读取仓库。 | 防御句："Folio 不会读取任何仓库"中"Folio"主语多余、"任何"过度强调；保留关闭行为即可 |
| `DescExplorerMenu` | 在资源管理器右键菜单的第一页和「显示更多选项」页都加入「在 Folio 中打开」。第一页需要注册 folio.msix（与 folio.exe 同文件夹）到当前账户。 | — | 无需改 |
| `DescExplorerMenuNoFirstPage` | 在资源管理器右键菜单中加入「在 Folio 中打开」。 | — | 无需改 |
| `DescExplorerMenuNoPackage` | 在资源管理器右键菜单的「显示更多选项」页加入「在 Folio 中打开」。本文件夹中缺少 folio.msix，该文件随压缩包放在 folio.exe 旁，第一页需要它。 | — | 无需改 |
| `DescExplorerFirstPageElsewhere` | 那一页上的条目指向另一个文件夹。Folio 会在下一次能做到的启动里把它改回来。 | 那一页上的条目指向另一个文件夹。下次启动时 Folio 会改回来。 | 翻译腔："下一次能做到的启动里"是直译 "next launch that can"，读起来不自然 |
| `DescTabLayout` | 标签是横排在窗口顶部，还是竖排在侧边。 | — | 无需改 |
| `DescSidebar` | 「展开」让竖排标签栏一直开在终端旁边；「图标」把它收成一条窄条，需要时覆盖在终端上打开。 | — | 无需改 |
| `DescSplitDirection` | 拆分时没有指明方向的话，新窗格落在哪一边。 | — | 无需改 |
| `DescDefaultProfile` | 新建标签时打开哪个配置，以及 Folio 启动时用哪个。 | — | 无需改 |
| `DescLanguage` | English、中文，或跟随系统设置 | — | 无需改 |
| `DescTerminalFont` | 终端文字使用的字体。标签、菜单和这个对话框保持各自的字体。 | — | 无需改 |
| `DescFontSize` | 终端文字有多大，尚未计入显示器的缩放。 | 终端文字的大小，不含显示器缩放。 | 翻译腔："尚未计入"过于文言，"有多大"口语化不统一 |
| `DescLightScheme` | 浅色窗口用的配色，终端与窗口本身共用一套。 | — | 无需改 |
| `DescDarkScheme` | 深色窗口同理。你自己的配色文件放在 %APPDATA%\\Folio\\schemes。 | — | 无需改 |
| `DescBackgroundImage` | 在整扇窗口背后画一张图片，位于所有窗格之下。 | 在窗口背后画一张图片，位于所有窗格之下。 | 术语："整扇窗口"中"扇"是量词误用，窗口用"个"即可，但此处直接省略更自然 |
| `DescImageFit` | 图片与窗口形状不同时怎么铺开：拉伸、填充或平铺。 | — | 无需改 |
| `DescImageOpacity` | 图片显示出多少。为 0 时窗口不画它。 | — | 无需改 |
| `DescBackgroundOpacity` | 桌面透过窗格和它们背后的窗口显出多少。文字与菜单保持不透明。 | — | 无需改 |
| `DescAcrylic` | 把窗口背后的东西模糊掉。仅在背景不透明度低于 100% 时可见。 | — | 无需改 |
| `DescAlwaysOnTop` | 这扇窗口保持在其他所有窗口之上。 | 窗口始终在其他窗口之上。 | 术语："这扇窗口"中"扇"同上，"所有"多余 |
| `DescAcrylicUnavailable` | 这个版本的 Windows 不提供这种模糊。 | — | 无需改 |
| `DescBackgroundOpacityUnavailable` | 这扇窗口以不透明方式绘制，无法让桌面透过来。 | 窗口以不透明方式绘制，桌面无法透过来。 | 术语："这扇窗口"同上 |
| `DescScrollback` | 每个窗格保留多少行。超出之后，最旧的行会被丢掉。 | — | 无需改 |
| `DescLineWrappingOn` | 比窗格宽的行在边缘折行。 | — | 无需改 |
| `DescLineWrappingOff` | 比窗格宽的行一直延伸，窗格可以横向滚动：Shift+滚轮，或底边那条横条。 | — | 无需改 |
| `DescFocusMode` | 标签条变成一列卡片，一张卡一个标签；选中的那个标签占满整扇窗口。Ctrl+Shift+Z 拨的是同一个开关。 | 标签条变成一列卡片，一张卡一个标签；选中的标签占满整个窗口。Ctrl+Shift+Z 拨的是同一个开关。 | 术语："整扇窗口"→"整个窗口" |
| `DescMinimumContrast` | 把终端文字提亮或压暗，直到与它背后的单元格达到这个对比度。背景一律不动；Off 以上的档位会覆盖程序指定的颜色。 | — | 无需改 |
| `DescNotifications` | 程序主动请求时，可以在你的桌面上放一条消息。它所在的窗格正显示在聚焦的窗口里时，什么都不弹。 | 程序请求时可在桌面弹出通知。窗格正显示在当前窗口中时不弹出。 | 翻译腔："程序主动请求时"的"主动"多余、"什么都不弹"口语化不一致；此处重写更简洁 |
| `DescTurnEndNotifications` | 回合结束时同样提醒，即使 agent 并非在等待输入：窗口不在你看得见的地方时闪烁任务栏按钮；窗口最小化或任务栏自动隐藏时发送系统通知。关闭后仅在等待输入时提醒。 | — | 无需改（信息量大但每个事实都有用） |
| `DescKeyHints` | 按住修饰键片刻，窗口中列出以该键开头的快捷键。此列表不会截走任何按键。 | 按住修饰键片刻，窗口列出以该键开头的快捷键。列表不截走按键。 | 防御句："此列表不会截走任何按键"中"此""任何"偏正式；缩短即可 |
| `DescCopyOnSelect` | 在窗格里松开一段选区，它就被写进剪贴板。除此之外没有别的提示 | 松开选区即复制到剪贴板，不另作提示。 | 防御句 + 翻译腔："除此之外没有别的提示"是防御句；"在窗格里松开一段选区，它就被写进"是直译英文被动语态 |
| `DescSearchEngine` | 在网页预览的地址栏里输入的不是地址时，交给哪个搜索引擎去搜。 | — | 无需改 |
| `DescTables` | 画出命令输出里的 markdown 表格。关闭时显示管道符原文。 | — | 无需改 |
| `DescBlockMaxHeight` | 超过这个高度的渲染块会在自己内部滚动，而不是继续变高。 | — | 无需改 |
| `DescFocusCardHeight` | 每张卡片里那幅标签缩图有多高。在卡片中某个窗格上 Alt+滚轮，可以一行一行地滚动那幅图。 | — | 无需改 |
| `DescPowerShellOffer` | PowerShell 窗格的 $PROFILE 未加载 folio.ps1 时，提示将其加入。整合提供命令标记、当前目录跟随，以及输出中行内公式 $…$ 的渲染。关闭后不再提示。 | — | 无需改 |

### Settings — descriptions (agent rows)

| id | current | proposed | why |
| --- | --- | --- | --- |
| `DescClaudeHooks` | 打开后，Folio 在 Claude Code 的用户级设置（~/.claude/settings.json）中写入 hook；等待你输入时对应标签显示提醒标记，不改动任何项目目录。 | 打开后，Folio 在 Claude Code 的用户级设置（~/.claude/settings.json）中写入 hook。等待你输入时标签页显示提醒标记。 | 防御句："不改动任何项目目录"是防御性否定；英文同样需要改 |
| `DescCodexNotify` | 打开后，Folio 在 Codex 的用户级配置（~/.codex/config.toml）中写入 notify 程序。Codex 仅在回合结束时发出通知，不区分是否在等待输入。 | 打开后，Folio 在 Codex 的用户级配置（~/.codex/config.toml）中写入 notify 程序。Codex 回合结束时发出通知。 | 防御句："不区分是否在等待输入"对多数用户没有意义，简化后保留核心事实 |
| `DescCopilotHooks` | 打开后，Folio 在 Copilot CLI 的用户级目录（~/.copilot/hooks/）中写入一个 hook 文件；等待你输入时对应标签显示提醒标记，不改动任何项目目录。 | 打开后，Folio 在 Copilot CLI 的用户级目录（~/.copilot/hooks/）中写入 hook 文件。等待你输入时标签页显示提醒标记。 | 防御句：同 DescClaudeHooks；英文同样需要改 |
| `DescCopilotHooksTooOld` | 本机的 Copilot CLI 版本低于 1.0.26。更早的版本会把并未向你显示过的权限询问也报告出来。 | — | 无需改（必要限制说明） |
| `DescCopilotHooksDisabled` | 你的 ~/.copilot/settings.json 中已关闭 hooks，这里写入的文件不会运行。 | — | 无需改（必要前提说明） |

### Settings — descriptions (quake terminal)

| id | current | proposed | why |
| --- | --- | --- | --- |
| `DescQuakeHeight` | 唤出的终端盖住鼠标所在那块屏幕高度的多少。它从那块屏幕的顶端挂下来。 | 唤出的终端占鼠标所在屏幕高度的多少，从屏幕顶端向下展开。 | 翻译腔："盖住""那块屏幕的顶端挂下来"拗口，合并为一句更自然 |
| `DescQuakeWidth` | 它盖住那块屏幕宽度的多少。剩下的部分左右均分，它居中。 | 唤出的终端占该屏幕宽度的多少，居中显示。 | 翻译腔："它盖住""剩下的部分左右均分，它居中"过于直译 |
| `DescQuakeHotkey` | 把终端叫下来的按键。它向 Windows 认领，所以在别的程序拿着键盘时也生效。 | 唤出终端的快捷键。向 Windows 注册为全局快捷键，其他程序拿着焦点时也生效。 | 机制 + 隐喻："向 Windows 认领"和"拿着键盘"是不自然的隐喻；英文同样需要改 |
| `DescQuakeHotkeyTaken` | 唤出它的按键已被另一个程序占用。 | — | 无需改 |
| `DescQuakeProfile` | 唤出的终端里新开的 tab 用哪个 shell 启动。 | 唤出的终端里新标签页用哪个 shell。 | 术语不统一：此处用 tab，其余处用标签/标签页 |
| `DescQuakeCommand` | 每次启动 Folio 后，第一次唤出时运行这条命令一次。唤出的终端恢复的其它内容都不会被运行。 | 每次 Folio 启动后首次唤出时运行一次。恢复的其他内容只填入输入行，不运行。 | 防御句 + 行话："都不会被运行"是防御句（英文同样需要改）；第二句按用户裁决用"输入行"代替隐含的"提示符" |
| `DescQuakeTopGap` | 它从那块屏幕的顶端往下留多少像素。 | 终端顶部与屏幕顶端的间距，单位为像素。 | 翻译腔："它从那块屏幕的顶端往下留多少像素"照搬英文句式 |
| `DescQuakeRestore` | 重新启动 Folio 后，唤出的终端里放回什么。恢复的命令只填在提示符后，不会运行；只有会上报命令的 shell 才有命令可恢复。 | Folio 重新启动后唤出的终端恢复哪些内容。恢复的命令只填入输入行，不运行；只有支持 shell 整合的配置才有命令可恢复。 | 行话 + 机制："提示符"→"输入行"（用户裁决）；"会上报命令的 shell"是机制语言 |
| `DescQuakeDismiss` | 键盘转到其他窗口时，唤出的终端随即收起。 | 焦点转到其他窗口时自动收起。 | 隐喻："键盘转到其他窗口"是直译 "keyboard moves to another window"，不如用"焦点" |

### Settings — options and pickers

| id | current | proposed | why |
| --- | --- | --- | --- |
| `OptionSystem` | 系统 | — | 无需改 |
| `OptionLight` | 浅色 | — | 无需改 |
| `OptionDark` | 深色 | — | 无需改 |
| `OptionCursorBar` | 竖线 | — | 无需改 |
| `OptionCursorBlock` | 方块 | — | 无需改 |
| `OptionCursorUnderline` | 下划线 | — | 无需改 |
| `OptionHorizontal` | 横向 | — | 无需改 |
| `OptionVertical` | 竖向 | — | 无需改 |
| `OptionOn` | 开 | — | 无需改 |
| `OptionOff` | 关 | — | 无需改 |
| `OptionExpanded` | 展开 | — | 无需改 |
| `OptionIcons` | 图标 | — | 无需改 |
| `OptionSplitAuto` | 自动（沿长边） | — | 无需改 |
| `OptionSplitRight` | 右侧 | — | 无需改 |
| `OptionSplitDown` | 下方 | — | 无需改 |
| `OptionImageNone` | 无 | — | 无需改 |
| `OptionImageChoose` | 选择… | — | 无需改 |
| `OptionFitStretch` | 拉伸 | — | 无需改 |
| `OptionFitFill` | 填充 | — | 无需改 |
| `OptionFitTile` | 平铺 | — | 无需改 |
| `OptionBlockHeightNone` | 不限 | — | 无需改 |
| `OptionQuakeProfileDefault` | 默认 profile | 默认配置 | 术语不统一：其他地方用"配置/配置文件" |
| `OptionQuakeRestoreNothing` | 不恢复 | — | 无需改 |
| `OptionQuakeRestoreFolders` | tab 与目录 | 标签页与目录 | 术语不统一：应用中文"标签页" |
| `OptionQuakeRestoreFoldersAndCommands` | tab、目录，以及 pin 的 tab 上一条命令填在提示符后 | 标签页、目录，以及固定标签页的上一条命令填入输入行 | 术语不统一 + 行话："tab"→"标签页"，"pin"→"固定"，"提示符"→"输入行" |
| `RowQuakeProfile` | 使用的 profile | 新标签页的配置 | 术语不统一："profile"应译为"配置" |
| `RowQuakeCommand` | 首次唤出时运行 | — | 无需改 |
| `RowQuakeTopGap` | 顶端留距 | — | 无需改 |
| `RowQuakeRestore` | 恢复内容 | — | 无需改 |
| `RowQuakeDismiss` | 失去焦点时收起 | — | 无需改 |
| `RowQuakeHeight` | 快捷终端高度 | — | 无需改 |
| `RowQuakeWidth` | 快捷终端宽度 | — | 无需改 |
| `RowQuakeHotkey` | 唤出按键 | — | 无需改 |

### PSReadLine rows

| id | current | proposed | why |
| --- | --- | --- | --- |
| `PsReadLineProbing` | 正在检查本机装的是哪个 PSReadLine | — | 无需改 |
| `PsReadLineRowGone` | Folio 安装的那一份已不在磁盘上 | — | 无需改 |
| `PsReadLineInviteTitle` | 修复缩放后输入行错位？ | — | 无需改 |
| `PsReadLineInstall` | 安装 | — | 无需改 |
| `PsReadLineNotNow` | 暂不 | — | 无需改 |
| `PsReadLineRemovedToast` | 已移除。新开的 PowerShell 会话将使用 Windows 自带的模块 | — | 无需改 |
| `PsReadLineUpdate` | 更新 | — | 无需改 |

### Explorer context menu

| id | current | proposed | why |
| --- | --- | --- | --- |
| `ContextMenuVerb` | 在 Folio 中打开 | — | 无需改 |
| `ContextMenuAddedToast` | 「在 Folio 中打开」已进入资源管理器菜单，在「显示更多选项」下面 | — | 无需改 |
| `ContextMenuRemovedToast` | 已从资源管理器菜单移除 | — | 无需改 |
| `ContextMenuNoExecutable` | Windows 没有说出 folio.exe 的位置 | — | 无需改 |
| `ExplorerCommandVerb` | 在 Folio 中打开 | — | 无需改 |
| `ExplorerFirstPageAddedToast` | 「在 Folio 中打开」已在资源管理器菜单的第一页 | — | 无需改 |
| `ExplorerFirstPageNoPackage` | folio.exe 旁边没有 folio.msix | — | 无需改 |

### Profile picker and root menu

| id | current | proposed | why |
| --- | --- | --- | --- |
| `ProfileHintDefault` | 默认 | — | 无需改 |
| `ProfileHintUnavailable` | 没找到 | — | 无需改 |
| `ProfileHintCurrent` | 当前 | — | 无需改 |
| `ProfileFilesPane` | 文件窗格 | — | 无需改 |
| `ProfileFilesPaneHint` | 本标签 | — | 无需改 |
| `ProfileRecentSection` | 最近打开 | — | 无需改 |
| `RootSection` | 打开文件夹 | — | 无需改 |
| `RootBrowse` | 浏览… | — | 无需改 |
| `RootNoteHome` | 主目录 | — | 无需改 |
| `RootNoteTerminal` | 终端在这里 | — | 无需改 |
| `RootNoteRecent` | 最近打开 | — | 无需改 |
| `RootNoteParent` | 上一级 | — | 无需改 |
| `PinnedSection` | 已钉住 | — | 无需改 |

### File menus, preview, drag

| id | current | proposed | why |
| --- | --- | --- | --- |
| `FileMenuOpenPreview` | 打开预览 | — | 无需改 |
| `FileMenuOpenDefaultApp` | 用系统默认程序打开 | — | 无需改 |
| `FileMenuCopyPath` | 复制路径 | — | 无需改 |
| `FileMenuInsertPath` | 把路径插入终端 | — | 无需改 |
| `FileMenuOpenWith` | 用默认应用打开 | — | 无需改 |
| `FileMenuRename` | 重命名 | — | 无需改 |
| `PreviewRailOpen` | 打开 | — | 无需改 |
| `PreviewRailOpenTip` | 打开等操作 | — | 无需改 |
| `PreviewCopyAddress` | 复制地址 | — | 无需改 |
| `PreviewFlipToSource` | 查看源码 | — | 无需改 |
| `PreviewFlipToRendered` | 查看渲染结果 | — | 无需改 |
| `PreviewFlipToPage` | 查看页面 | — | 无需改 |
| `VideoFormatCannotPlay` | 此格式无法播放 | — | 无需改 |
| `PreviewOpenExternally` | 用默认程序打开 | — | 无需改 |
| `PreviewOpened` | 已打开 ✓ | — | 无需改 |
| `PreviewTruncated` | 只读 · 64 KB | — | 无需改 |
| `PreviewSaved` | 已保存 | — | 无需改 |
| `PreviewConflict` | 未保存 · 磁盘上已改动 · 编辑仍在 | — | 无需改 |
| `PreviewNothingToSave` | 没有要保存的内容 | — | 无需改 |
| `PreviewFailedImageLoad` | 预览失败：图片无法载入 | — | 无需改 |
| `PreviewFailedImageWorker` | 预览失败：无法绘制这张图片 | — | 无需改 |
| `PreviewFailedSeatTooSmall` | 预览失败：预览窗格太小 | — | 无需改 |
| `PreviewOpenInBrowser` | 在浏览器中打开 | — | 无需改 |
| `PreviewStopPlaying` | 停止播放 | — | 无需改 |
| `HeadPopOut` | 在浮动窗口中打开 | — | 无需改 |
| `PreviewLock` | 锁定此窗格 —— 接下来打开的内容会开在新的预览里 | — | 无需改 |
| `PreviewUnlock` | 解锁 —— 此窗格重新成为可复用的预览 | — | 无需改 |
| `PreviewEmptyState` | 点击带虚线下划线的路径，即可在此预览 | — | 无需改 |
| `PreviewRefusalType` | 这种文件类型没有预览 | — | 无需改 |
| `PreviewRefusalBinary` | 无法预览 —— 这看起来是二进制文件 | — | 无需改 |
| `PreviewRefusalNetworkPath` | 无法预览 —— 网络路径不会自动读取 | — | 无需改 |
| `PreviewRefusalPermissionDenied` | 无法预览 —— 没有权限 | — | 无需改 |
| `PreviewRefusalNotFound` | 无法预览 —— 文件不存在 | — | 无需改 |
| `PreviewRefusalUnreadable` | 无法预览 —— 无法读取这个文件 | — | 无需改 |
| `PreviewDiskChanged` | 文件在磁盘上已更改。你未保存的修改还在。 | — | 无需改 |
| `PreviewDiskReload` | 重新加载 | — | 无需改 |
| `PreviewDiskKeep` | 保留我的修改 | — | 无需改 |
| `PreviewDiskDeleted` | 文件已被删除。你正在读的这一份还在。 | — | 无需改 |
| `FloatDock` | 停靠 | — | 无需改 |
| `FloatTriggerTip` | 在这里速览文件 | — | 无需改 |
| `DragSwapPanes` | 交换窗格 | — | 无需改 |
| `DragReplacePane` | 替换窗格 | — | 无需改 |
| `DragOpenInPreview` | 在这个预览里打开 | — | 无需改 |
| `DragRootTreeHere` | 把这棵树的根设到这里 | — | 无需改 |
| `MarkdownImageUnreadable` | 这张图片没有打开 | — | 无需改 |
| `MarkdownImageRemote` | Folio 没有网络客户端，所以这张图片不会被取回 | Folio 不抓取远程图片 | 机制："没有网络客户端"暴露内部原因；英文同样需要改 |

### Files tree, restore, status overlays

| id | current | proposed | why |
| --- | --- | --- | --- |
| `FilesLoading` | 载入中… | — | 无需改 |
| `FilesEmpty` | 空文件夹 | — | 无需改 |
| `FilesUnrooted` | 未打开文件夹 | — | 无需改 |
| `FilesPermissionDenied` | 没有权限 | — | 无需改 |
| `FilesNotFound` | 文件夹不存在 | — | 无需改 |
| `FilesUnreadable` | 无法读取文件夹 | — | 无需改 |
| `FilesRevealed` | 已在文件资源管理器中显示 | — | 无需改 |
| `FilesViewFiles` | 文件 | — | 无需改 |
| `FilesViewGit` | Git | — | 无需改 |
| `RestoreTitle` | 重新打开你的其他标签？ | — | 无需改 |
| `RestoreSub` | 上次关闭 Folio 时它们还开着。它们会回到你离开时所在的目录，但都是新的会话。 | — | 无需改 |
| `RestoreDecline` | 不用了 | — | 无需改 |
| `RestoreAccept` | 恢复 | — | 无需改 |
| `FilesProgramRefused` | 文件树不会运行程序 | — | 无需改（必要限制） |
| `MathWorkerStopped` | 公式渲染已停止；终端的输入输出仍然可用 | 公式渲染已停止，终端输入输出不受影响。 | 翻译腔：分号断句不如逗号自然；"仍然可用"可简化 |
| `FilesWorkerStopped` | 目录读取已停止；终端的输入输出仍然可用 | 目录读取已停止，终端输入输出不受影响。 | 同上 |
| `PreviewWorkerStopped` | 文件预览读取已停止；终端的输入输出仍然可用 | 文件预览已停止，终端输入输出不受影响。 | 同上 |
| `GitWorkerStopped` | git 读取已停止；终端的输入输出仍然可用 | git 读取已停止，终端输入输出不受影响。 | 同上 |

### Web fail cards

| id | current | proposed | why |
| --- | --- | --- | --- |
| `WebFailRuntimeSay` | 没找到 Microsoft Edge WebView2 Runtime。 | — | 无需改 |
| `WebFailRuntimeVerb` | 下载运行时 | — | 无需改 |
| `WebFailEngineSay` | 网页引擎没有启动。 | — | 无需改 |
| `WebFailEngineVerb` | 重试 | — | 无需改 |
| `WebFailCrashSay` | 这个页面停止运行了。它的渲染进程已退出。 | 这个页面停止运行了。 | 机制："它的渲染进程已退出"暴露内部机制且无助于恢复；英文同样需要改 |
| `WebFailBlockedSay` | 这个地址不在预览中打开。 | — | 无需改 |
| `WebFailBlockedVerb` | 复制地址 | — | 无需改 |
| `WebFailDownloadSay` | 这次下载没法交给浏览器。请求里带着普通链接重放不出来的东西。 | 在浏览器中打开此页面即可下载。 | 机制 + 翻译腔："请求里带着普通链接重放不出来的东西"暴露 HTTP 机制且句式拗口；英文同样需要改 |
| `WebFailDownloadVerb` | 在浏览器中打开这个页面 | — | 无需改 |

### Hyperlinks

| id | current | proposed | why |
| --- | --- | --- | --- |
| `HyperlinkBlockedSuffix` | · 已拦截 | — | 无需改 |
| `HyperlinkBlocked` | 已拦截 | — | 无需改 |
| `HyperlinkControlOpensExternally` | · Ctrl+点击用默认程序打开 | — | 无需改 |
| `HyperlinkControlReveals` | · Ctrl+点击在资源管理器中显示 | — | 无需改 |

### First-run card

| id | current | proposed | why |
| --- | --- | --- | --- |
| `FirstRunTitle` | 欢迎使用 Folio | — | 无需改 |
| `FirstRunSettingsLine` | 所有选项都可在设置中更改 | — | 无需改 |
| `FirstRunLater` | 暂不 | — | 无需改 |
| `FirstRunDone` | 完成 | — | 无需改 |
| `FirstRunRowUpdate` | 有 Folio 新版本时提醒 | — | 无需改 |
| `FirstRunRowExplorer11` | 在右键菜单中用 Folio 打开文件夹 | — | 无需改 |
| `FirstRunRowExplorer10` | 在 Show more options 中用 Folio 打开文件夹 | — | 无需改 |
| `FirstRunRowPowerShell` | PowerShell 整合让你在已运行命令间跳转 | PowerShell 整合，支持命令间跳转 | 翻译腔："让你在已运行命令间跳转"直译英文结构 |
| `FirstRunRowClaude` | Claude Code 等待时标签页高亮 | — | 无需改 |
| `FirstRunRowCodex` | Codex 回合结束时标签页高亮 | — | 无需改 |
| `FirstRunRowCopilot` | Copilot CLI 等待时标签页高亮 | — | 无需改 |
| `FirstRunTipUpdate` | 设置显示新版本号并提供发布页 | — | 无需改 |
| `FirstRunTipExplorer` | 为当前 Windows 账户注册此菜单项 | — | 无需改 |
| `FirstRunTipPowerShell` | 先做带日期的副本，再追加一行到 $PROFILE | — | 无需改 |
| `FirstRunTipClaude` | 先做带日期的副本，再写入 ~/.claude/settings.json | — | 无需改 |
| `FirstRunTipCodex` | 先做带日期的副本，再写入 ~/.codex/config.toml | — | 无需改 |
| `FirstRunTipCopilot` | 先做带日期的副本，再写入 ~/.copilot/hooks/folio.json | — | 无需改 |
| `ShellIntegrationPending` | 下次启动 PowerShell 时生效 | — | 无需改 |

### PowerShell integration notice

| id | current | proposed | why |
| --- | --- | --- | --- |
| `PowerShellNoticeBody` | 尚未安装 PowerShell 整合。它提供命令标记、当前目录跟随，以及输出中行内公式 $…$ 的渲染。 | — | 无需改 |
| `PowerShellNoticeAdd` | 加进 $PROFILE | — | 无需改 |
| `PowerShellNoticeNever` | 不再提示 | — | 无需改 |
| `PowerShellNoticeAdded` | 已加进 $PROFILE。新开的 shell 生效。 | — | 无需改 |

### Toasts

| id | current | proposed | why |
| --- | --- | --- | --- |
| `ClaudeHooksAddedToast` | 已写入 ~/.claude/settings.json。新的 Claude Code 会话开始生效。 | — | 无需改 |
| `ClaudeHooksRemovedToast` | 已从 ~/.claude/settings.json 移除 | — | 无需改 |
| `ClaudeHooksFailedToast` | Claude Code 的设置未改动 | — | 无需改 |
| `CodexNotifyAddedToast` | 已写入 ~/.codex/config.toml。新的 codex 会话开始生效。 | — | 无需改 |
| `CodexNotifyRemovedToast` | 已从 ~/.codex/config.toml 移除 | — | 无需改 |
| `CodexNotifyFailedToast` | codex 的配置未改动 | — | 无需改 |
| `CopilotHooksAddedToast` | 已写入 ~/.copilot/hooks/folio.json。新的 copilot 会话开始生效。 | — | 无需改 |
| `CopilotHooksRemovedToast` | 已删除 ~/.copilot/hooks/folio.json | — | 无需改 |
| `CopilotHooksFailedToast` | copilot 的 hooks 未改动 | — | 无需改 |
| `ToastTurnFinished` | 回合结束 | — | 无需改 |
| `ToastWaitingForYou` | 正在等你回答 | — | 无需改 |
| `NotifyRefusedTitle` | 桌面通知被拒绝 | — | 无需改 |
| `NotifyRefusedBody` | Windows 没有接收这条通知。窗内的记号不受影响。 | — | 无需改 |
| `QuitSessionNotWritten` | session.json 写入失败。没有关闭任何窗口。 | — | 无需改 |
| `SchemeFileSkipped` | 配色文件已跳过 | — | 无需改 |
| `SchemeInUseBroken` | 配色未重新载入 | — | 无需改 |
| `SchemeInUseGone` | 找不到配色 | — | 无需改 |
| `SchemeDeleted` | 配色已删除 | — | 无需改 |
| `BackgroundPictureRefused` | 背景图未显示 | — | 无需改 |

### Pane and tab menus

| id | current | proposed | why |
| --- | --- | --- | --- |
| `PaneMenuSplitWith` | 拆分并运行 | — | 无需改 |
| `PaneMenuNewInFolder` | 在文件夹里新建终端… | — | 无需改 |
| `PaneMenuDuplicate` | 复制窗格 | — | 无需改 |
| `PaneMenuMoveToNewTab` | 把窗格移到新标签 | — | 无需改 |
| `PaneMenuMoveToNewWindow` | 把窗格移到新窗口 | — | 无需改 |
| `PaneMenuMoveToWindow` | 移到窗口 | — | 无需改 |
| `PaneMenuSplitCaption` | 拆分 | — | 无需改 |
| `PaneMenuZoom` | 放大窗格 | — | 无需改 |
| `PaneMenuRestore` | 还原窗格 | — | 无需改 |
| `ClosePane` | 关闭窗格 | — | 无需改 |
| `PaneChevronTip` | 拆分等操作 | — | 无需改 |
| `TabMenuRename` | 重命名标签 | — | 无需改 |
| `TabMenuUnpin` | 取消固定 | — | 无需改 |
| `TabMenuDuplicate` | 复制标签 | — | 无需改 |
| `TabMenuMoveToNewWindow` | 把标签移到新窗口 | — | 无需改 |
| `TabMenuClose` | 关闭标签 | — | 无需改 |
| `TermMenuCopy` | 复制 | — | 无需改 |
| `TermMenuPaste` | 粘贴 | — | 无需改 |
| `TermMenuSelectAll` | 全选 | — | 无需改 |
| `TermMenuFind` | 查找… | — | 无需改 |
| `TermMenuClearScreen` | 清屏 | — | 无需改 |
| `TermMenuClearScrollback` | 清除回滚… | — | 无需改 |
| `TermMenuShellAgain` | 重启 shell… | — | 无需改 |

### Move refusals

| id | current | proposed | why |
| --- | --- | --- | --- |
| `MoveRefusedGone` | 那个 tab 或那扇窗在移动之前已经关了。什么都没有移动。 | 目标标签页或窗口已关闭，未移动。 | 术语不统一 + 翻译腔："tab"→"标签页"；"那扇窗"→"窗口"；"什么都没有移动"多余 |
| `MoveRefusedAlreadyThere` | 这个 tab 已经在那扇窗里了。 | 这个标签页已在该窗口中。 | 术语不统一："tab"→"标签页"；"那扇窗"→"窗口" |
| `MoveRefusedPaneIsNowATab` | 这个窗格现在是本窗的一个 tab。 | 这个窗格已是本窗口中的一个标签页。 | 术语不统一："tab"→"标签页" |

### Shortcuts page

Most shortcut names are clean translations. Listing only those that need change:

| id | current | proposed | why |
| --- | --- | --- | --- |
| `ShortcutCommandPalette` | 命令面板 | 搜索面板 | 术语不统一：产品术语表定义搜索面板，zh-style-notes.md 明确记录 command palette = 搜索面板 |
| `ShortcutDuplicatePaneSplit` | 复制窗格到拆分里 | 复制窗格并拆分 | 翻译腔："到拆分里"不自然 |
| `ShortcutScopeTerminalPrimary` | 在终端自己的回滚里 | 在终端回滚区中 | 翻译腔："终端自己的"直译 "terminal's own" |
| `ShortcutScopeSearchHost` | 有正文可查找的地方 | 有内容可搜索的地方 | 术语："正文"不是产品惯用词 |
| `CardGestureHint` | 滚动卡片内容 | — | 无需改 |

### Profiles page

| id | current | proposed | why |
| --- | --- | --- | --- |
| `NavProfiles` | 配置文件 | — | 无需改 |
| `CategoryProfiles` | 配置文件 | — | 无需改 |
| `ProfilesDuplicate` | 复制 | — | 无需改 |
| `ProfilesBadgeDefault` | 默认 | — | 无需改 |
| `ProfilesBadgeDefaultAutomatic` | 自动默认 | — | 无需改 |
| `ProfilesBadgeHidden` | 已隐藏 | — | 无需改 |
| `ProfilesAgentInsideWsl` | 安装在 WSL 内的 agent，可从 WSL 配置启动，或新建配置：程序 wsl.exe，参数 -e <命令名> | — | 无需改 |
| `ProfilesRowNameDesc` | 这个配置在标签、选择器和这份列表中显示的名称。 | — | 无需改 |
| `ProfilesRowProgramDesc` | 这个配置新建标签时启动的程序。 | — | 无需改 |
| `ProfilesRowStartingDirDesc` | 这个配置的新标签在哪个文件夹中打开。 | — | 无需改 |
| `ProfilesRowColourDesc` | 在标签和菜单中标示这个配置的颜色。 | — | 无需改 |
| `ProfilesRowArgsDesc` | 在程序读取自己的任何内容之前传给它。空格分词，双引号成组。例：-NoExit -File D:\\me\\start.ps1 | — | 无需改 |
| `ProfilesRowEnvDesc` | 为这个配置启动的每个会话设置，覆盖 Folio 自己设定的值。 | — | 无需改 |
| `ProfilesRowHyperlinkDesc` | 设置 FORCE_HYPERLINK，程序在决定是否输出链接前会读取它。 | — | 无需改 |
| `ProfilesCannotHideDefault` | 默认配置始终留在选择器中 | — | 无需改 |
| `ProfilesCannotHideFallback` | 其他配置无法启动时，Folio 退回到这个配置 | — | 无需改 |
| `ProfilesNameBlank` | 配置文件需要一个名称 | — | 无需改 |
| `ProfilesNameTaken` | 已有另一个配置使用这个名称 | — | 无需改 |

### Git page and commit graph

| id | current | proposed | why |
| --- | --- | --- | --- |
| `GitGroupStagedTip` | 已为下一次提交打包 —— 'git commit' 提交的正是这些 | — | 无需改 |
| `GitGroupChangesTip` | 已改，但还没打包 | — | 无需改 |
| `GitGroupUntrackedTip` | 还不在仓库里 —— git 没有盯着这些 | — | 无需改 |
| `GitNotFound` | 这台机器上找不到 git.exe —— 装上 Git for Windows 才能用这个页面 | — | 无需改 |
| `GraphToolFilterTip` | 这张图画的是哪些分支 | — | 无需改 |
| `GraphToolSearchTip` | 按信息、作者或哈希搜索提交 | — | 无需改 |
| `GraphFileBinary` | 二进制 —— git 在这里没有行可数 | — | 无需改 |
| `GraphMergeCommit` | 合并提交 —— 另一个分支的历史在这里并入 | — | 无需改 |
| `GraphDoubleClickCheckout` | 双击检出这个提交 | — | 无需改 |
| All other Git strings | — | — | 无需改（全部通过） |

### Search capsule

| id | current | proposed | why |
| --- | --- | --- | --- |
| All search strings | — | — | 无需改 |

### Shortcut recorder

| id | current | proposed | why |
| --- | --- | --- | --- |
| `ShortcutRecordPrompt` | Enter 留下 · Esc 取消 · Del 清空 | — | 无需改 |
| `ShortcutRecordUnusable` | 这个组合键无法保存 | — | 无需改 |
| `ShortcutUndelivered` | Windows 会自己截走一部分组合键；始终到不了框里的，这里录不到 | — | 无需改 |

### Confirmation gates

| id | current | proposed | why |
| --- | --- | --- | --- |
| All gate strings | — | — | 无需改 |

---

## Surface 2: i18n.rs — format functions

| function / id | current | proposed | why |
| --- | --- | --- | --- |
| `psreadline_invite_body` | Windows PowerShell 自带的 PSReadLine 是 {found}。在这个版本上，缩放窗口后输入行会错位，Folio 发出的修复指令不起作用。安装会把 PSReadLine {patched} 写入 {path}，对此后新开的 PowerShell 会话生效，并可在设置 ▸ 终端中移除。 | — | 无需改 |
| `update_row_available_in` | {version} 已发布。打开发布页会在浏览器里打开它。 | {version} 已发布。点击「打开发布页」可在浏览器中查看。 | 翻译腔："打开发布页会在浏览器里打开它"重复"打开"且句式绕 |
| `psreadline_policy_refused` | 未安装 PSReadLine {patched}。Windows 的执行策略是 {policy}。在 PowerShell 中运行 {remedy} 后再试。模块本应写入 {path}。 | — | 无需改 |
| `psreadline_no_documents` | 未安装 PSReadLine {patched}。Windows 没有给出本用户的 Documents 文件夹位置，而 PowerShell 在它下面找这个模块。 | — | 无需改 |
| `psreadline_already_current` | 本机自带的 PSReadLine {found} 已能守住输入行，未向 {path} 写入任何东西。 | — | 无需改 |
| `profile_deleted` (panes>0) | {profile_title} 已删除。有 {panes} 个 pane 仍在运行它，它们继续运行 | {profile_title} 已删除。{panes} 个窗格仍在运行，不受影响 | 术语不统一："pane"→"窗格" |
| `web_fail_blocked_scheme` | {scheme}: 开头的地址不在预览中打开。 | — | 无需改 |
| All other format functions | — | — | 无需改 |

---

## Surface 3: PRIVACY.md Chinese half

| location | current | proposed | why |
| --- | --- | --- | --- |
| 开头 | Folio 不向任何地方发送与你有关的数据。没有遥测、没有统计、没有崩溃上报。 | — | 无需改（隐私声明中的否定句是必要的事实） |
| 更新检查表格 "如何关闭" | 设置 > General > **检查新版** | 设置 > 常规 > **检查新版** | 术语不统一："General"应用中文"常规"（i18n 里 NavGeneral = 常规） |
| 更新检查表格 "如何关闭" | 出生即开 | 默认开启 | 翻译腔："出生即开"是不必要的比喻 |
| 更新检查表格 "如何关闭" | 首次配置卡 | 初次设置卡 | 术语不统一：README 和应用中说"初次设置卡" |
| Shell 整合脚本行 | Folio 为 PowerShell 与 bash 整合写出的脚本 | — | 无需改 |
| session.json 快捷终端条目 | 快捷终端中被钉住的标签最后运行过的命令——存的是那一行本身，命令里带 token，token 就在文件里。只有这一种标签会写，且只在装了 shell 整合、有东西说明哪一行是命令时才有。是否保留由 设置 > 快捷终端 > **恢复内容** 决定 | — | 无需改（虽然长，但每个事实都是隐私相关的必要说明） |
| 其它位置 - 首次配置卡 | 首次配置卡 | 初次设置卡 | 术语不统一：同上 |
| 其余 | — | — | 无需改 |

---

## Surface 4: Release note 0.2.2 Chinese half

| location | current | proposed | why |
| --- | --- | --- | --- |
| 开头 | 对 0.2.1-preview 的修复与打磨，另加一张卡：从未运行过 Folio 的机器，其边界问题会在一个地方一次性问完。 | 0.2.1-preview 的修复，加上初次设置卡：首次运行 Folio 时在一处完成所有选项的设置。 | 行话 + 翻译腔："边界问题"是英文 "boundary questions" 的直译，用户不会这么说；英文同样需要改 |
| 第一条 | 这张卡一次性把所有问题放在一起问，因为它们其实是同一个决定：允许 Folio 触及这台机器的多少部分。答案写入 `%APPDATA%\Folio` **之外**的文件。六行…… | 首次运行时以一张卡呈现所有需要写入 `%APPDATA%\Folio` 之外文件的选项。六行…… | 机制 + 过长：开头两句是设计意图解释而非用户信息 |
| 第一条末尾 | 主题、字体、字号、语言和布局一概不涉及——那些只需一次点击即可更改，暂时不对也无任何代价。 | （建议删除） | 防御句：解释卡片上不包含什么，对读者无帮助；英文同样需要改 |
| "卡上每一行都是设置中的一行" | 这张卡只是在按这些行，并不做任何自己的事。卡上没有任何「最后一次机会」 | 这张卡和设置里的开关完全相同，随时可以更改。 | 防御句 + 机制："并不做任何自己的事""没有任何最后一次机会"是防御句；英文同样需要改 |
| 从 0.2.1 升级 | **无需任何操作。** `settings.json` 会新增该卡涉及的两个键……`session.json`、`profiles.json`、`keybindings.json` 与 `pins.json` 的读写方式与 0.2.1 完全一致，且没有移动任何快捷键、默认值或文件位置。 | **升级时保留原有设置。** 初次设置卡仅面向新用户。 | 防御句：逐个列举未变的文件是典型的"未受影响的清单"；英文同样需要改 |
| 其余（技术修复条目） | — | — | 无需改（大部分技术修复条目的中文翻译准确通顺） |

---

## Surface 5: docs/shortcuts.md Chinese section

This section is generated from i18n.rs strings. Changes above (notably `ShortcutCommandPalette` → 搜索面板) will propagate when the table is regenerated. No additional changes needed beyond what i18n.rs changes produce.

---

## Surface 6: README.zh-CN.md

The README was rewritten today and serves as the reference voice. Checking it against the style rules:

| location | current | proposed | why |
| --- | --- | --- | --- |
| 下载 > WebView2 段末 | 缺少时预览窗格会提示，其余功能不受影响。 | 缺少时预览窗格提示。 | 防御句："其余功能不受影响"是典型的安慰句 |
| 初次启动 > 首个标签页段 | 七个 agent 配置同样通过 Windows PATH 查找，初次设置卡的 agent 行使用相同的查找结果。 | （建议删除或移至开发文档） | 机制：PATH 查找是实现细节，对用户无帮助 |
| 初次启动 > PowerShell 整合段 | Git Bash 和 WSL 无需整合，不写入任何文件。 | （建议删除） | 防御句：说明不做什么 |
| 功能 > agent > 末尾 | 任何程序发送 `OSC 1337;RequestAttention=yes` 即可被识别。 | — | 无需改（开发者受众需要的技术事实） |
| 快捷终端 > 末尾 | 快捷终端随 Folio 启停，没有独立图标。 | 快捷终端随 Folio 启停。 | 防御句："没有独立图标"说明不存在的东西 |
| 已知问题 > .webm | 缺少时无法显示静帧和播放。 | 缺少时无法播放。 | 过长："无法显示静帧和播放"可简化 |
| 隐私 | Folio 不发送遥测、分析数据或崩溃报告。程序中没有模型和 API 密钥，它为你已有的 agent 提供终端环境。 | — | 无需改（隐私声明中的否定是必要事实） |
| 其余 | — | — | 无需改 |

---

## English ids that also need editorial changes

These English strings exhibit the same issues (defensive negatives, internal mechanisms) the guide identifies. They should be addressed in the English pass:

| id | issue |
| --- | --- |
| `DescUpdateCheck` | "It downloads nothing" — defensive negative |
| `DescClaudeHooks` | "Nothing is written into a project folder" — defensive negative |
| `DescCopilotHooks` | "Nothing is written into a project folder" — defensive negative |
| `DescQuakeCommand` | "Nothing else the summoned terminal restores is run" — defensive negative |
| `DescQuakeHotkey` | "It is claimed from Windows, so it works while another program has the keyboard" — mechanism |
| `WebFailCrashSay` | "Its render process exited" — mechanism |
| `WebFailDownloadSay` | "The request carried data a plain link cannot replay" — mechanism |
| `MarkdownImageRemote` | "Folio has no network client, so this image is not fetched" — mechanism |

---

## Strings whose meaning I could not fully verify

| id | question |
| --- | --- |
| `DescExplorerFirstPageElsewhere` | "Folio 会在下一次能做到的启动里把它改回来" — does this mean "next launch where the package is registered" or "next launch period"? The English "next launch that can" is ambiguous about what condition must be met. |
| `GraphToolLeaveDetachedTip` | "HEAD 不在任何分支上 —— 重新站到一条分支上" — is "站到" the intended verb here? It reads as an instruction but this is a tooltip, not a button. If it describes the button's action, "回到一条分支上" might read more naturally. |
| `CapPowerShell` / `CapPowerShellNoLinks` | "点源 folio.ps1 之后才有" — "点源" is a literal rendering of "dot-sourced". Developers who use PowerShell will know this term, but it is jargon. Verify whether this is intentionally kept for the developer audience or should be rephrased as "加载 folio.ps1 后生效". |
