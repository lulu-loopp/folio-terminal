<#
.SYNOPSIS
    What a script copied into the clean guest needs beside it, and whether the
    guest's Windows PowerShell 5.1 can read it. Dot-sourced by
    `run-smoke-in-vm.ps1` and `updater/run-row.ps1`; it runs nothing on its own.

.DESCRIPTION
    **The copy list is read out of the scripts, never written by hand.**
    `smoke.ps1` began dot-sourcing `release-manifest.ps1` (which reads
    `archive-members.txt` beside itself) and `run-smoke-in-vm.ps1` kept copying
    only `smoke.ps1` and `in-guest.ps1`: the 0.4.6 release smoke then died in
    the guest with `release-manifest.ps1 is not recognized` (RC-046, H-10). A
    list derived from the script's own text cannot fall behind it.

    `Get-GuestLoadedFiles` walks a script's syntax tree for every file it names
    beside itself — `Join-Path $here '<name>'` or `Join-Path $PSScriptRoot
    '<name>'`, the two spellings the release scripts use for "the file next to
    me" — and follows each `.ps1` among them that is dot-sourced, so a
    dot-sourced helper's own neighbours come too. `..` and `.` name folders,
    not neighbours, and are skipped. A neighbour that is named but missing on
    the host is an error here: the guest would fail on it later, farther away.

    `Assert-GuestReadable` is the two host-side checks every guest-bound script
    passes before it is copied (clean-vm.md §3.4a, §3.4b): a UTF-8 byte-order
    mark, without which 5.1 reads the file in the ANSI code page, and a parse
    by the host's own `powershell.exe`, which is the edition the guest runs.
#>

Set-StrictMode -Version Latest

function Get-GuestLoadedFiles {
    <#
    .SYNOPSIS
        Every file `Script` loads from beside itself, recursively through its
        dot-sources, as paths relative to the script's folder (the script itself
        is not in the list). Order: first named, first listed.
    #>
    param([Parameter(Mandatory)] [string] $Script)

    $Script = (Resolve-Path -LiteralPath $Script).Path
    $base = Split-Path -Parent $Script
    $found = New-Object System.Collections.Generic.List[string]
    $pending = New-Object System.Collections.Generic.Queue[string]
    $pending.Enqueue($Script)
    $visited = @{}
    while ($pending.Count -gt 0) {
        $file = $pending.Dequeue()
        if ($visited.ContainsKey($file)) { continue }
        $visited[$file] = $true
        $folder = Split-Path -Parent $file
        $tokens = $null
        $errors = $null
        $ast = [System.Management.Automation.Language.Parser]::ParseFile($file, [ref] $tokens, [ref] $errors)
        if ($errors -and $errors.Count -gt 0) {
            throw "$file does not parse: $($errors[0].Message)"
        }
        $commands = @($ast.FindAll({ param($node) $node -is [System.Management.Automation.Language.CommandAst] }, $true))
        $dots = @($commands | Where-Object {
                $_.InvocationOperator -eq [System.Management.Automation.Language.TokenKind]::Dot })
        foreach ($command in $commands) {
            if ($command.GetCommandName() -ne 'Join-Path') { continue }
            $arguments = @($command.CommandElements | Select-Object -Skip 1 |
                Where-Object { $_ -isnot [System.Management.Automation.Language.CommandParameterAst] })
            if ($arguments.Count -ne 2) { continue }
            $anchor = $arguments[0]
            $leaf = $arguments[1]
            if ($anchor -isnot [System.Management.Automation.Language.VariableExpressionAst]) { continue }
            if (@('here', 'PSScriptRoot') -notcontains $anchor.VariablePath.UserPath) { continue }
            if ($leaf -isnot [System.Management.Automation.Language.StringConstantExpressionAst]) { continue }
            $name = $leaf.Value
            if ($name -eq '..' -or $name -eq '.') { continue }
            $full = [IO.Path]::GetFullPath((Join-Path $folder $name))
            if (-not (Test-Path -LiteralPath $full -PathType Leaf)) {
                throw "$file names $name beside itself, and there is no such file at $full"
            }
            if (-not $full.StartsWith($base + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
                throw "$file names $name, which is outside $base; the guest copy keeps one folder"
            }
            $relative = $full.Substring($base.Length + 1)
            if ($full -ne $Script -and -not $found.Contains($relative)) { $found.Add($relative) }
            # A dot-sourced script's own neighbours are needed too.
            $inDot = @($dots | Where-Object {
                    $command.Extent.StartOffset -ge $_.Extent.StartOffset -and
                    $command.Extent.EndOffset -le $_.Extent.EndOffset })
            if ($inDot.Count -gt 0 -and $full -like '*.ps1') { $pending.Enqueue($full) }
        }
    }
    return , $found.ToArray()
}

function Assert-GuestReadable {
    <#
    .SYNOPSIS
        Throws unless every `.ps1` in `Paths` starts with a UTF-8 byte-order mark
        and parses under the host's Windows PowerShell 5.1. Other files pass.
    #>
    param([Parameter(Mandatory)] [string[]] $Paths)

    $scripts = @($Paths | Where-Object { $_ -like '*.ps1' })
    foreach ($path in $scripts) {
        $stream = [IO.File]::OpenRead((Resolve-Path -LiteralPath $path).Path)
        try {
            $head = New-Object byte[] 3
            $read = $stream.Read($head, 0, 3)
        }
        finally { $stream.Dispose() }
        if (-not ($read -eq 3 -and $head[0] -eq 0xEF -and $head[1] -eq 0xBB -and $head[2] -eq 0xBF)) {
            throw "$path has no UTF-8 byte-order mark; Windows PowerShell 5.1 in the guest would read it as ANSI"
        }
    }
    if ($scripts.Count -eq 0) { return }

    # **A parser catches syntax, and that is all it catches** (§3.4b): a
    # three-argument `Join-Path` parses and fails only when it runs. Running the
    # script under `powershell.exe` on the host is the check for that class.
    $windowsPowerShell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    if (-not (Test-Path -LiteralPath $windowsPowerShell -PathType Leaf)) {
        throw "no Windows PowerShell at $windowsPowerShell; the guest-bound scripts cannot be checked against the edition the guest runs"
    }
    $checker = Join-Path ([IO.Path]::GetTempPath()) ('folio-parse-' + [Guid]::NewGuid().ToString('n') + '.ps1')
    Set-Content -LiteralPath $checker -Encoding UTF8 -WhatIf:$false -Value @'
param([Parameter(Mandatory)] [string] $Path)
$errors = $null
[void][System.Management.Automation.Language.Parser]::ParseFile($Path, [ref] $null, [ref] $errors)
if ($errors -and $errors.Count -gt 0) {
    foreach ($problem in $errors) {
        Write-Output ('  line {0}, column {1}: {2}' -f
            $problem.Extent.StartLineNumber, $problem.Extent.StartColumnNumber, $problem.Message)
    }
    exit 1
}
exit 0
'@
    try {
        foreach ($path in $scripts) {
            $said = (& $windowsPowerShell -NoProfile -ExecutionPolicy Bypass `
                    -File $checker -Path $path 2>&1 | Out-String).TrimEnd()
            if ($LASTEXITCODE -ne 0) {
                throw @"
$path does not parse under Windows PowerShell 5.1, which is the only
edition the clean machine has:
$said
"@
            }
        }
    }
    finally { Remove-Item -LiteralPath $checker -Force -WhatIf:$false -ErrorAction SilentlyContinue }
}
