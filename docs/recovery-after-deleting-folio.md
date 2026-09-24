# Recovering after deleting an older Folio

This page is for anyone who removed Folio 0.4.2 or earlier by deleting its
folder, and found that something it had written elsewhere stayed behind. Folio
0.4.3 and later make every one of these marks harmless on their own — the
PowerShell line becomes a guarded load, and newer versions provide a
`folio --uninstall-cleanup` command that takes the rest out in one step — but
none of that can reach a copy that is already gone, so everything below is done
by hand and needs no Folio at all.

Nothing here is urgent. Work through only the sections that describe what you
are actually seeing.

## Windows

### Every PowerShell starts with an error naming `folio.ps1`

**What you see.** A new PowerShell window prints an error before its first
prompt. The text says a file cannot be found and names a path ending in
`folio.ps1`.

**Why.** If you switched the PowerShell integration on, Folio added one line to
your `$PROFILE` — the startup file PowerShell runs for you, which is yours and
not Folio's. The line is one of these two shapes:

```powershell
. "$env:APPDATA\Folio\shell-integration\folio.ps1"
. 'D:\some\other\path\folio.ps1'
```

The script it names lives in `%APPDATA%\Folio`, so deleting the program folder
alone leaves the line working. Deleting `%APPDATA%\Folio` as well takes the
script away, and PowerShell reports the missing file at every start.

**The fix.** Do this in the window that shows the error, so you are certainly
working on the file that shell reads. Windows PowerShell 5.1 and PowerShell 7
keep **different** profile files, and a machine whose Documents folder is
redirected to OneDrive keeps them somewhere else again — so ask the shell rather
than typing a path:

```powershell
$PROFILE.CurrentUserCurrentHost
```

Look at the line before you touch it:

```powershell
Select-String -LiteralPath $PROFILE.CurrentUserCurrentHost -Pattern 'folio.ps1' -SimpleMatch
```

That prints the line number and the line. Then keep a copy of the file and open
it:

```powershell
Copy-Item -LiteralPath $PROFILE.CurrentUserCurrentHost -Destination "$($PROFILE.CurrentUserCurrentHost).before-removing-folio"
notepad $PROFILE.CurrentUserCurrentHost
```

Delete **only** that one line and save. Everything else in the file is your own.
If you see more than one such line, they are all the same line written twice and
all of them can go.

Folio also left a dated copy of this file, `<profile>.bak-<YYYYMMDD>`, beside it
on the day it first wrote the line. You may keep or delete that copy as you
like; nothing reads it.

**How to check.** Open a new PowerShell window. It reaches its prompt with
nothing printed. If you see the error in both PowerShell 7 and Windows
PowerShell 5.1, repeat the steps in each — they are two files.

### Your coding agents still name a `folio.exe` that is gone

**What you see.** Usually nothing. These entries are silent: Claude Code
suppresses the completion notice of an `async` hook, Codex's `notify` spawn
failure is a line in its own log, and Copilot logs the miss and carries on. They
are untidy, not harmful, and you can leave them if you would rather. What
follows is how to take them out.

**Why.** If you switched an agent row on, Folio wrote an entry into that agent's
own user-level configuration naming the `folio.exe` it was running from. Each
entry carries a marker so it can be told apart from anything you wrote yourself.

**Claude Code** — `~\.claude\settings.json`, or `settings.json` under
`%CLAUDE_CONFIG_DIR%` if you have set that variable. Folio's entries are the
ones whose `command` contains `attention claude-code:`, one per event, and they
look like this:

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

Look at what is there:

```powershell
Select-String -LiteralPath (Join-Path $HOME '.claude\settings.json') -Pattern 'attention claude-code:' -SimpleMatch
```

Either open the file and delete each group whose command contains the marker —
and the event name with it, if that group was the only one under it — or paste
this, which does the same thing and keeps a copy of the file beside itself
first:

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

It keeps every hook that is not Folio's. It does not keep the file's original
key order or indentation, which is why the copy is taken first.

**Codex** — `~\.codex\config.toml`, or `config.toml` under `%CODEX_HOME%`.
Folio's mark is the whole `notify` key, a single line at the top level:

```toml
notify = ["C:\\folio\\folio.exe", "attention", "codex:agent-turn-complete", "--json"]
```

Open the file and delete that one line. Delete it only if it names `attention`
and `codex:` as above; a `notify` naming some other program is not Folio's.

**Copilot CLI** — `~\.copilot\hooks\folio.json`, or the same path under
`%COPILOT_HOME%`. The whole file is Folio's, so the file goes:

```powershell
Remove-Item -LiteralPath (Join-Path $HOME '.copilot\hooks\folio.json')
```

**How to check.** Run the `Select-String` above again for Claude Code and for
Codex (with `attention codex:` as the pattern); nothing is printed. For Copilot,
the file is no longer there.

Each of these three files may also have a dated copy beside it named
`<file>.bak-<YYYYMMDD>`, taken on the day Folio first wrote to it. That is your
file as it stood before, kept for you; delete it or keep it.

### "Open in Folio" is still in a folder's right-click menu

**What you see.** Right-clicking a folder still offers **Open in Folio**, under
**Show more options** on Windows 11, and clicking it does nothing.

**Why.** There are two separate registrations, and neither is a file in Folio's
folder. Deleting the folder cannot take either off, because nothing runs on the
way out.

**The fix.** Both live in your own account's part of the registry, so neither of
these needs an administrator. The classic entry is two keys — remove both:

```
reg delete "HKCU\Software\Classes\Directory\shell\Folio" /f
reg delete "HKCU\Software\Classes\Directory\Background\shell\Folio" /f
```

A key that was never written answers `ERROR: The system was unable to find the
specified registry key or value`, which means there was nothing to remove.

The first-page entry on Windows 11 comes from the sparse package `folio.msix`
that shipped in the archive. It is registered for your account only, so this
needs no administrator either. Run it in **Windows PowerShell**:

```powershell
Get-AppxPackage -Name WeiyiShi.Folio | Remove-AppxPackage
```

**How to check.** Right-click a folder. If the row is still drawn, Explorer is
still using what it read earlier: sign out and back in, or restart
`explorer.exe`, and it is gone. `reg query "HKCU\Software\Classes\Directory\shell\Folio"`
and `Get-AppxPackage -Name WeiyiShi.Folio` should both come back with nothing.

### Folio's settings and data are still on disk

**What you see.** Nothing, unless you go looking. Folio keeps what it remembers
outside its own folder so that a new version picks your settings back up, which
is exactly what survives deleting the folder.

**Why.** There are two roots, not one, and the second is easy to miss.

**The fix.** Delete what you no longer want:

```powershell
Remove-Item -LiteralPath "$env:APPDATA\Folio" -Recurse -Force
Remove-Item -LiteralPath "$env:LOCALAPPDATA\Folio" -Recurse -Force
```

The first holds your settings, the restored session, profiles, pins, colour
schemes, the shell-integration scripts, the diagnostics log and any hang
reports. The second holds the web preview's browser profile, including its cache
and cookies.

If you ever ran a build from before the product was named Folio, there is a
third: `%APPDATA%\BetterTerminal`, which newer builds move across on first run
and which is otherwise left where it is.

```powershell
Remove-Item -LiteralPath "$env:APPDATA\BetterTerminal" -Recurse -Force
```

Two more files sit in your temporary directory — `%TEMP%\folio-panic.log` and
the folder `%TEMP%\folio\clipboard` — and Windows clears that directory on its
own. There is also one small registry key that names Folio as the sender of its
notifications. It is inert, and removing it is optional:

```
reg delete "HKCU\Software\Classes\AppUserModelId\Folio.Terminal" /f
```

**How to check.** `Test-Path "$env:APPDATA\Folio"` and
`Test-Path "$env:LOCALAPPDATA\Folio"` both answer `False`.

## macOS

Dragging **Folio** to the Bin takes nearly everything with it: Finder's **Open
in Folio** is declared inside the application bundle, and nothing was ever
written into your `.zshrc` or `.bash_profile` — Folio hands zsh and bash their
integration as it starts them, out of its own directory. There is no PowerShell
line to remove.

Two things stay. The first is the agent hook entries, in the same three files as
on Windows — `~/.claude/settings.json`, `~/.codex/config.toml` and
`~/.copilot/hooks/folio.json`, or the same names under `CLAUDE_CONFIG_DIR`,
`CODEX_HOME` and `COPILOT_HOME` — recognised by the same markers, and just as
quiet. Look at them with:

```sh
grep -n 'attention claude-code:' ~/.claude/settings.json
grep -n 'attention codex:' ~/.codex/config.toml
ls ~/.copilot/hooks/folio.json
```

Take them out the same way: delete each Claude Code group whose command carries
the marker, delete Codex's `notify` line, and delete Copilot's file, which is
Folio's whole.

The second is what Folio and the system kept for it, under your own Library:

```sh
rm -rf ~/Library/Application\ Support/Folio \
       ~/Library/WebKit/io.github.lulu-loopp.folio \
       ~/Library/Caches/io.github.lulu-loopp.folio \
       ~/Library/HTTPStorages/io.github.lulu-loopp.folio \
       ~/Library/Saved\ Application\ State/io.github.lulu-loopp.folio.savedState \
       ~/Library/Preferences/io.github.lulu-loopp.folio.plist
```

The first of those is your settings and session; the rest are the web preview's
and the system's, and some of them may not exist. The notification permission
you granted is held by macOS against the bundle identifier, not by Folio; it
costs nothing and goes when the system next prunes it.
