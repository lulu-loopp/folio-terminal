# 删除旧版 Folio 之后的清理

这篇文档适用于删除了 Folio 0.4.2 或更早版本的文件夹后，发现它在别处留下的痕迹仍然存在的情况。Folio 0.4.3 及之后的版本会让这些痕迹各自变得无害——PowerShell 那一行改为有保护的加载，新版本还提供 `folio --uninstall-cleanup` 命令一次性清除其余痕迹——但已经删掉的程序无法再做这些事，所以下面全部是手动操作，不需要 Folio。

没有任何一项是紧急的。只看与自己遇到的问题对应的段落即可。

## Windows

### 每次打开 PowerShell 都报一条找不到 `folio.ps1` 的错

**看到的现象。** 新打开的 PowerShell 窗口在第一行命令之前就输出了一条错误，提示某个文件找不到，路径以 `folio.ps1` 结尾。

**原因。** 如果你开启过 PowerShell 整合，Folio 在 `$PROFILE`——PowerShell 启动时为你执行的启动文件，属于你而不属于 Folio——末尾写入了一行。这行的形式是以下两种之一：

```powershell
. "$env:APPDATA\Folio\shell-integration\folio.ps1"
. 'D:\some\other\path\folio.ps1'
```

这行指向的脚本存放在 `%APPDATA%\Folio` 里，所以只删除程序文件夹时这行仍然有效。连 `%APPDATA%\Folio` 一起删除后脚本就不存在了，PowerShell 每次启动都会报错。

**修复方法。** 在出现错误的那个窗口里操作，这样可以确定当前 shell 读取的就是你要改的文件。Windows PowerShell 5.1 和 PowerShell 7 使用**不同的**配置文件，文档文件夹重定向到 OneDrive 的机器路径又不一样——所以问 shell 本身，不要自己拼路径：

```powershell
$PROFILE.CurrentUserCurrentHost
```

先看看那一行是什么：

```powershell
Select-String -LiteralPath $PROFILE.CurrentUserCurrentHost -Pattern 'folio.ps1' -SimpleMatch
```

输出行号和那一行的内容。然后备份文件并打开：

```powershell
Copy-Item -LiteralPath $PROFILE.CurrentUserCurrentHost -Destination "$($PROFILE.CurrentUserCurrentHost).before-removing-folio"
notepad $PROFILE.CurrentUserCurrentHost
```

只删掉那一行，保存。文件里的其他内容都是你自己的。如果看到不止一行，它们是同一行写了两次，全部删掉。

Folio 第一次写入那行时还在旁边留了一份带日期的备份 `<profile>.bak-<YYYYMMDD>`。可以留着也可以删，没有任何程序会读取它。

**验证方法。** 打开一个新的 PowerShell 窗口，应当直接到达命令行，没有错误输出。如果 PowerShell 7 和 Windows PowerShell 5.1 都报错，在两者中分别重复上述步骤——它们是两个文件。

### 编程 agent 的配置里仍然指向已经不存在的 `folio.exe`

**看到的现象。** 通常什么也看不到。这些条目是静默的：Claude Code 会抑制 `async` 钩子的完成提示，Codex 的 `notify` 启动失败只是它自己日志里的一行，Copilot 记录一条日志后继续工作。它们只是不整洁，不会造成损害，不想管也可以留着。下面是清除方法。

**原因。** 如果你开启过某个 agent 对应的行，Folio 在该 agent 自己的用户级配置中写入了一条指向当时运行的 `folio.exe` 的条目。每条记录带有标记，可以和你自己写的内容区分开。

**Claude Code** — `~\.claude\settings.json`，如果设置了 `%CLAUDE_CONFIG_DIR%` 则在该目录下的 `settings.json`。Folio 写入的条目是 `command` 中含有 `attention claude-code:` 的那些，每个事件一条，形如：

```json
"Stop": [
  {
    "hooks": [
      {
        "async": true,
        "command": "\"C:\\folio\\folio.exe\" attention claude-code:Stop --json -",
        "type": "command"
      }
    ]
  }
]
```

先看看当前内容：

```powershell
Select-String -LiteralPath (Join-Path $HOME '.claude\settings.json') -Pattern 'attention claude-code:' -SimpleMatch
```

可以手动打开文件删掉每一组 command 含有该标记的条目——如果某个事件名下只有这一组，连事件名一起删掉——也可以粘贴下面的命令，效果相同，会先在文件旁边留一份备份：

```powershell
$dir  = if ($env:CLAUDE_CONFIG_DIR) { $env:CLAUDE_CONFIG_DIR } else { Join-Path $HOME '.claude' }
$file = Join-Path $dir 'settings.json'
Copy-Item -LiteralPath $file -Destination "$file.before-removing-folio"
$settings = Get-Content -LiteralPath $file -Raw | ConvertFrom-Json
if ($settings.hooks) {
    foreach ($event in @($settings.hooks.PSObject.Properties.Name)) {
        $kept = @($settings.hooks.$event | Where-Object { -not ($_.hooks.command -like '*attention claude-code:*') })
        if ($kept.Count -eq 0) { $settings.hooks.PSObject.Properties.Remove($event) }
        else { $settings.hooks.$event = $kept }
    }
    if (-not $settings.hooks.PSObject.Properties.Name) { $settings.PSObject.Properties.Remove('hooks') }
}
[System.IO.File]::WriteAllText($file, ($settings | ConvertTo-Json -Depth 100))
```

这段脚本保留所有不属于 Folio 的钩子，不保留原文件的键序和缩进格式，所以先做了备份。

**Codex** — `~\.codex\config.toml`，如果设置了 `%CODEX_HOME%` 则在该目录下的 `config.toml`。Folio 留下的是顶层的 `notify` 键，单独一行：

```toml
notify = ["C:\\folio\\folio.exe", "attention", "codex:agent-turn-complete", "--json"]
```

打开文件删掉这一行。只在它包含 `attention` 和 `codex:` 时才删；如果 `notify` 指向的是其他程序，那不是 Folio 写的。

**Copilot CLI** — `~\.copilot\hooks\folio.json`，如果设置了 `%COPILOT_HOME%` 则在该目录下对应路径。整个文件都是 Folio 的，直接删除：

```powershell
Remove-Item -LiteralPath (Join-Path $HOME '.copilot\hooks\folio.json')
```

**验证方法。** 对 Claude Code 和 Codex 重新运行上面的 `Select-String`（Codex 用 `attention codex:` 作为匹配文本），应当无输出。Copilot 的文件不再存在即可。

这三个文件各自旁边可能还有一份名为 `<file>.bak-<YYYYMMDD>` 的带日期备份，是 Folio 第一次写入当天留下的。那是写入前你的文件原样，可以留着也可以删。

### 右键菜单里仍然有"Open in Folio"

**看到的现象。** 右键点击文件夹仍然出现 **Open in Folio**，Windows 11 上在**显示更多选项**里，点击后没有反应。

**原因。** 这是两处独立的注册，都不是 Folio 文件夹里的文件。删除文件夹无法带走它们，因为删除过程中不会运行任何程序。

**修复方法。** 两处注册都在你自己的账户范围内，不需要管理员。经典菜单项是两个注册表键——两个都删掉：

```
reg delete "HKCU\Software\Classes\Directory\shell\Folio" /f
reg delete "HKCU\Software\Classes\Directory\Background\shell\Folio" /f
```

如果某个键本来就不存在，会提示 `ERROR: The system was unable to find the specified registry key or value`，说明没有需要删的东西。

Windows 11 第一页菜单项来自压缩包中附带的精简包 `folio.msix`，注册在你的账户下，同样不需要管理员。在 **Windows PowerShell** 中运行：

```powershell
Get-AppxPackage -Name WeiyiShi.Folio | Remove-AppxPackage
```

**验证方法。** 右键点击一个文件夹。如果那一行还在，是资源管理器仍在使用之前读取的信息：注销再登录，或重启 `explorer.exe`，就会消失。`reg query "HKCU\Software\Classes\Directory\shell\Folio"` 和 `Get-AppxPackage -Name WeiyiShi.Folio` 都应当返回空结果。

### Folio 的设置和数据还在磁盘上

**看到的现象。** 什么也看不到，除非你主动去找。Folio 把它记住的内容存放在自己文件夹之外，这样新版本可以接续你的设置——这也正是删除文件夹后留下来的东西。

**原因。** 有两个数据目录，不是一个，第二个容易漏掉。

**修复方法。** 删掉不再需要的：

```powershell
Remove-Item -LiteralPath "$env:APPDATA\Folio" -Recurse -Force
Remove-Item -LiteralPath "$env:LOCALAPPDATA\Folio" -Recurse -Force
```

第一个存放设置、恢复的会话、配置、固定项、配色方案、shell 整合脚本、诊断日志和挂起报告。第二个存放网页预览的浏览器配置文件，包括缓存和 cookie。

如果你曾使用过产品改名为 Folio 之前的构建，还有第三个目录：`%APPDATA%\BetterTerminal`。新版本首次运行时会将它迁移过来，否则就留在原处。

```powershell
Remove-Item -LiteralPath "$env:APPDATA\BetterTerminal" -Recurse -Force
```

临时目录中还有两个文件——`%TEMP%\folio-panic.log` 和文件夹 `%TEMP%\folio\clipboard`——Windows 会自行清理临时目录。另外有一个小的注册表键将 Folio 登记为通知发送者，它不起任何作用，删不删都行：

```
reg delete "HKCU\Software\Classes\AppUserModelId\Folio.Terminal" /f
```

**验证方法。** `Test-Path "$env:APPDATA\Folio"` 和 `Test-Path "$env:LOCALAPPDATA\Folio"` 都返回 `False`。

## macOS

将 **Folio** 拖进废纸篓几乎就带走了一切：Finder 的 **Open in Folio** 声明在应用包内部，zsh 和 bash 的整合脚本也不曾写入 `.zshrc` 或 `.bash_profile`——Folio 在启动它们时从自己的目录传入，不修改用户文件。没有需要删除的 PowerShell 行。

留下来的有两样。第一是 agent 钩子条目，在与 Windows 相同的三个文件中——`~/.claude/settings.json`、`~/.codex/config.toml` 和 `~/.copilot/hooks/folio.json`，如果设置了 `CLAUDE_CONFIG_DIR`、`CODEX_HOME`、`COPILOT_HOME` 则在对应目录下——按相同的标记识别，同样是静默的。查看方法：

```sh
grep -n 'attention claude-code:' ~/.claude/settings.json
grep -n 'attention codex:' ~/.codex/config.toml
ls ~/.copilot/hooks/folio.json
```

清除方式相同：删掉 Claude Code 中 command 带有该标记的每一组，删掉 Codex 的 `notify` 行，删掉 Copilot 的整个文件。

第二是 Folio 和系统为它保存的数据，在你的 Library 下：

```sh
rm -rf ~/Library/Application\ Support/Folio \
       ~/Library/WebKit/io.github.lulu-loopp.folio \
       ~/Library/Caches/io.github.lulu-loopp.folio \
       ~/Library/HTTPStorages/io.github.lulu-loopp.folio \
       ~/Library/Saved\ Application\ State/io.github.lulu-loopp.folio.savedState \
       ~/Library/Preferences/io.github.lulu-loopp.folio.plist
```

第一个是设置和会话；其余是网页预览和系统保存的内容，其中有些可能不存在。通知权限由 macOS 根据 bundle identifier 管理，不属于 Folio；没有开销，系统下次清理时会一并移除。
