# The package-manager hooks, rendered and run with no installer (0.4.6 ticket U-2)
#
# `packaging/scoop/folio.json` and `packaging/homebrew/folio.rb` are the sources
# of the scoop bucket's and the Homebrew tap's files. Their hooks write the
# install channel marker (`install_channel.rs`, U-1) and call Folio's cleanup
# door (`folio --uninstall-cleanup`, `uninstall.rs`). What can be known about
# them without scoop or brew is checked here; the installed checks are
# `scripts/release/check-scoop-hooks-in-vm.ps1` and
# `scripts/release/check-cask-hooks.sh`, run on a VM and a Mac by hand.
#
# 1. The scoop manifest is JSON with the fields scoop requires, and its hooks
#    are lists of lines.
# 2. The cask is Ruby: `ruby -c` where Ruby is installed, else a structural
#    check of its blocks and stanzas.
# 3. The release renderer (`scripts/release/update-manifests.ps1`) changes the
#    version, URL and hash lines and nothing else: the hooks reach the bucket
#    and the tap byte for byte.
# 4. Windows only — **exit_2_aborts_and_exit_1_continues**: the manifest's own
#    `pre_uninstall` lines, run the way scoop runs a hook (`Invoke-HookScript`:
#    a script block made from the lines, under a caller whose `$cmd` names the
#    scoop command and whose `$dir` is the version folder), against a stub
#    `folio.exe` that exits 2, 1 or 0. Exit 2 must throw (scoop stops with the
#    app intact), 1 and 0 must go on, and under `scoop update` the door must not
#    run at all. The stub is a window-subsystem program like Folio and answers
#    half a second late, so a hook that did not wait for it would read no exit
#    code. The `post_install` lines, run the same way, must write the marker
#    literal byte for byte. Both under PowerShell 7 and Windows PowerShell 5.1,
#    since scoop runs under either.
#
# The marker literal itself is read by U-1's own parser in
# `install_channel::tests::the_marker_each_package_manager_writes_reads_as_that_manager`,
# and the cleanup words by the door's grammar in
# `the_cleanup_line_each_package_manager_runs_is_the_doors_own`.
#
# `.github/workflows/ci.yml` runs this in `release-script-tests`.

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$scoopPath = Join-Path $repo "packaging/scoop/folio.json"
$caskPath = Join-Path $repo "packaging/homebrew/folio.rb"
$renderer = Join-Path $repo "scripts/release/update-manifests.ps1"
$pwsh = (Get-Process -Id $PID).Path

$work = Join-Path ([IO.Path]::GetTempPath()) "folio-manager-hooks-$PID"
if (Test-Path -LiteralPath $work) { Remove-Item -LiteralPath $work -Recurse -Force }
[IO.Directory]::CreateDirectory($work) | Out-Null

$failures = [System.Collections.Generic.List[string]]::new()
function Check([bool] $ok, [string] $what) {
    if ($ok) { Write-Host "ok   $what" } else { Write-Host "FAIL $what"; $failures.Add($what) }
}

# The one single-quoted JSON object in a hook: the marker literal.
function Get-QuotedMarker([string] $text) {
    $found = [regex]::Matches($text, "'(\{[^']*\})'")
    if ($found.Count -ne 1) { throw "expected one quoted marker, found $($found.Count) in: $text" }
    return $found[0].Groups[1].Value
}

try {
    # ── 1. the scoop manifest ─────────────────────────────────────────────────
    $scoopText = [IO.File]::ReadAllText($scoopPath)
    $scoop = $scoopText | ConvertFrom-Json
    $arch = $scoop.architecture.'64bit'
    foreach ($field in @('version', 'description', 'homepage', 'license', 'bin', 'checkver', 'autoupdate')) {
        Check ($null -ne $scoop.$field -and "$($scoop.$field)" -ne '') "scoop manifest has $field"
    }
    Check ($arch.url -match '^https://' -and $arch.hash -match '^[0-9a-f]{64}$' -and $arch.extract_dir) `
        'scoop manifest has a 64-bit url, hash and extract_dir'
    # Every key scoop's manifest schema knows at the top level; one it does not
    # is a typo scoop would ignore, which for a hook means a hook that never runs.
    $known = @('##', '$schema', 'version', 'description', 'homepage', 'license', 'notes', 'depends',
        'suggest', 'url', 'hash', 'extract_dir', 'extract_to', 'architecture', 'bin', 'shortcuts',
        'persist', 'env_add_path', 'env_set', 'pre_install', 'post_install', 'installer',
        'pre_uninstall', 'post_uninstall', 'uninstaller', 'checkver', 'autoupdate', 'innosetup',
        'cookie', 'psmodule')
    foreach ($key in $scoop.PSObject.Properties.Name) {
        Check ($known -contains $key) "scoop manifest key '$key' is one scoop reads"
    }
    foreach ($hook in @('post_install', 'pre_uninstall')) {
        $lines = @($scoop.$hook)
        Check ($lines.Count -gt 0 -and @($lines | Where-Object { $_ -isnot [string] }).Count -eq 0) `
            "scoop manifest's $hook is a list of lines"
    }
    $marker = Get-QuotedMarker (@($scoop.post_install) -join "`n")

    # ── 2. the cask ───────────────────────────────────────────────────────────
    $caskText = [IO.File]::ReadAllText($caskPath)
    $ruby = Get-Command ruby -ErrorAction SilentlyContinue
    if ($ruby) {
        $said = & $ruby.Source -c $caskPath 2>&1
        Check ($LASTEXITCODE -eq 0) "ruby -c packaging/homebrew/folio.rb ($said)"
    }
    else {
        # No Ruby here: every `do` closed by an `end` at its own indentation,
        # every bracket closed, and the stanzas the hooks are made of present.
        $opened = [System.Collections.Generic.Stack[int]]::new()
        $balanced = $true
        foreach ($line in $caskText -split "`n") {
            $indent = $line.Length - $line.TrimStart().Length
            if ($line -match '\bdo\s*$') { $opened.Push($indent) }
            elseif ($line.Trim() -eq 'end') {
                if ($opened.Count -eq 0 -or $opened.Pop() -ne $indent) { $balanced = $false }
            }
        }
        Check ($balanced -and $opened.Count -eq 0) 'the cask closes every block it opens (no Ruby here: structural check)'
        foreach ($pair in @(@('[', ']'), @('{', '}'), @('(', ')'))) {
            $open = @($caskText.ToCharArray() | Where-Object { $_ -eq $pair[0] }).Count
            $close = @($caskText.ToCharArray() | Where-Object { $_ -eq $pair[1] }).Count
            Check ($open -eq $close) "the cask closes every $($pair[0])"
        }
    }
    foreach ($stanza in @('^cask "folio" do$', '^  version "', '^  sha256 "', '^  url "', '^  app "Folio\.app"$',
            '^  depends_on macos: :sonoma$', '^  postflight_steps do$', '^  zap script: \{$', '^        must_succeed: false,$')) {
        Check ([regex]::IsMatch($caskText, "(?m)$stanza")) "the cask has /$stanza/"
    }
    $caskMarker = Get-QuotedMarker $caskText
    Check ($caskMarker -ceq '{"v":1,"manager":"homebrew","uninstall_hook":false}') `
        "the cask's postflight_steps write the homebrew marker without a hook ($caskMarker)"

    # ── 3. the renderer keeps the hooks ───────────────────────────────────────
    $package = Join-Path $work 'package'
    $rendered = Join-Path $work 'rendered'
    [IO.Directory]::CreateDirectory($package) | Out-Null
    $zipHash = 'a' * 64
    $dmgHash = 'b' * 64
    [IO.File]::WriteAllText((Join-Path $package 'SHA256SUMS.txt'), "$zipHash  folio-9.8.7-windows-x64.zip`n")
    [IO.File]::WriteAllText((Join-Path $package 'SHA256SUMS-macos.txt'), "$dmgHash  Folio-9.8.7-macos-arm64.dmg`n")
    $said = & $pwsh -NoProfile -NonInteractive -File $renderer -Version 9.8.7 -PackageDirectory $package `
        -CaskFile $caskPath -ScoopFile $scoopPath -OutDirectory $rendered 2>&1
    Check ($LASTEXITCODE -eq 0) "update-manifests.ps1 renders the two sources ($(@($said)[-1]))"
    $renderedScoop = [IO.File]::ReadAllText((Join-Path $rendered 'folio.json'))
    $renderedCask = [IO.File]::ReadAllText((Join-Path $rendered 'folio.rb'))
    foreach ($case in @(
            @('the bucket file', $scoopText, $renderedScoop, 4, @('"version": "9.8.7"', 'v9.8.7-preview', $zipHash, '"folio-9.8.7"')),
            @('the cask', $caskText, $renderedCask, 2, @('version "9.8.7"', $dmgHash)))) {
        $before = $case[1] -split "`n"
        $after = $case[2] -split "`n"
        $changed = @(for ($i = 0; $i -lt $before.Count; $i++) { if ($before[$i] -cne $after[$i]) { $i } })
        Check ($before.Count -eq $after.Count -and $changed.Count -eq $case[3]) `
            "the renderer changes $($case[3]) lines of $($case[0]) and keeps the rest byte for byte ($($changed.Count) changed)"
        foreach ($value in $case[4]) {
            Check ($case[2].Contains($value)) "$($case[0]) as rendered says $value"
        }
    }

    # ── 4. the hooks, run as scoop runs them ──────────────────────────────────
    if (-not $IsWindows) {
        Write-Host 'skip exit_2_aborts_and_exit_1_continues: scoop hooks run on Windows only'
    }
    else {
        # The stub: a window-subsystem program, as folio.exe is, that records the
        # words it was given and answers with the code written beside it, late.
        $stubSource = Join-Path $work 'stub.cs'
        [IO.File]::WriteAllText($stubSource, @'
using System;
using System.IO;
static class Stub {
    static int Main(string[] args) {
        string here = AppDomain.CurrentDomain.BaseDirectory;
        File.WriteAllText(Path.Combine(here, "stub-args.txt"), string.Join("\n", args));
        int code = int.Parse(File.ReadAllText(Path.Combine(here, "stub-exit.txt")).Trim());
        System.Threading.Thread.Sleep(500);
        Console.Out.Write("Folio: stub answered " + code + "\n");
        Console.Out.Flush();
        return code;
    }
}
'@)
        $csc = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
        $stubDir = Join-Path $work 'apps\folio\9.8.7'
        [IO.Directory]::CreateDirectory($stubDir) | Out-Null
        $stub = Join-Path $stubDir 'folio.exe'
        $said = & $csc -nologo -target:winexe "-out:$stub" $stubSource 2>&1
        if ($LASTEXITCODE -ne 0) { throw "csc could not build the stub: $said" }

        # scoop's Invoke-HookScript, and the caller scope it is reached from:
        # `exec` in lib/commands.ps1 holds `$cmd`, the command's script holds
        # `$dir`, and the hook is a script block made from the manifest's lines.
        $harness = Join-Path $work 'run-hook.ps1'
        [IO.File]::WriteAllText($harness, @'
param([string] $ManifestPath, [string] $Hook, [string] $Command, [string] $VersionFolder)
Set-StrictMode -Off
$installed = [IO.File]::ReadAllText($ManifestPath) | ConvertFrom-Json
function Invoke-HookScript([string] $HookType, $Manifest) {
    $script = $Manifest.$HookType
    if ($script) {
        Invoke-Command ([scriptblock]::Create($script -join "`r`n"))
    }
}
function exec([string] $cmd) {
    $dir = $VersionFolder
    try {
        Invoke-HookScript $Hook $installed
        'HOOK CONTINUED'
    }
    catch {
        "HOOK THREW: $($_.Exception.Message)"
    }
}
exec $Command
'@)

        $shells = @(
            @('PowerShell 7', $pwsh),
            @('Windows PowerShell 5.1', (Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe')))
        foreach ($shell in $shells) {
            $name = $shell[0]
            function Invoke-Hook([string] $hook, [string] $command, [string] $folder) {
                $lines = & $shell[1] -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $harness `
                    -ManifestPath $scoopPath -Hook $hook -Command $command -VersionFolder $folder 2>&1
                return (@($lines | ForEach-Object { "$_" } | Where-Object { $_ -like 'HOOK *' }) | Select-Object -Last 1)
            }

            # post_install: the marker, byte for byte, in the version folder.
            $fresh = Join-Path $work "post-install-$($name -replace '\W', '')"
            [IO.Directory]::CreateDirectory($fresh) | Out-Null
            $answer = Invoke-Hook 'post_install' 'install' $fresh
            $written = Join-Path $fresh 'folio-install.json'
            $bytes = if (Test-Path -LiteralPath $written) { [IO.File]::ReadAllBytes($written) } else { @() }
            Check ($answer -eq 'HOOK CONTINUED' -and
                [Text.Encoding]::UTF8.GetString($bytes) -ceq $marker -and $bytes.Count -eq $marker.Length) `
                "${name}: post_install writes folio-install.json as the literal $marker, no BOM"

            # pre_uninstall: exit 2 aborts, exit 1 and 0 continue, and the door
            # hears exactly --uninstall-cleanup.
            foreach ($case in @(@(2, 'HOOK THREW'), @(1, 'HOOK CONTINUED'), @(0, 'HOOK CONTINUED'))) {
                $code = $case[0]
                Remove-Item -LiteralPath (Join-Path $stubDir 'stub-args.txt') -ErrorAction SilentlyContinue
                [IO.File]::WriteAllText((Join-Path $stubDir 'stub-exit.txt'), "$code")
                $answer = Invoke-Hook 'pre_uninstall' 'uninstall' $stubDir
                $argsFile = Join-Path $stubDir 'stub-args.txt'
                $heard = if (Test-Path -LiteralPath $argsFile) { [IO.File]::ReadAllText($argsFile) } else { '<not run>' }
                Check ("$answer".StartsWith($case[1]) -and $heard -ceq '--uninstall-cleanup') `
                    "${name}: exit_2_aborts_and_exit_1_continues — the door exits $code, the hook answers '$answer' (door heard '$heard')"
                if ($code -eq 2) {
                    Check ("$answer".Contains('Folio: stub answered 2')) "${name}: the throw carries the line the door printed"
                }
            }

            # scoop update runs pre_uninstall too: the door must not run.
            Remove-Item -LiteralPath (Join-Path $stubDir 'stub-args.txt') -ErrorAction SilentlyContinue
            [IO.File]::WriteAllText((Join-Path $stubDir 'stub-exit.txt'), '2')
            $answer = Invoke-Hook 'pre_uninstall' 'update' $stubDir
            Check ($answer -eq 'HOOK CONTINUED' -and -not (Test-Path -LiteralPath (Join-Path $stubDir 'stub-args.txt'))) `
                "${name}: under 'scoop update' the hook goes on and the door is not run ('$answer')"
        }
    }
}
finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

if ($failures.Count -gt 0) {
    Write-Host ''
    Write-Host "$($failures.Count) check(s) failed:"
    $failures | ForEach-Object { Write-Host "  $_" }
    exit 1
}
Write-Host ''
Write-Host 'the package-manager hooks render, parse and map the door''s exit codes'
exit 0
