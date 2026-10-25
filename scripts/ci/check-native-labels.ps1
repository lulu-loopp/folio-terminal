# the_native_labels_allow_lists_only_shrink
#
# `crates/bt-source/tests/native_labels.rs` (T-NATIVE-LABELS-FROM-THE-CALLER, widened by
# T-NATIVE-LABELS-GUARD-REACH) keeps the words a person reads on a native menu or dialog out of
# `bt-platform`: a literal with a word in the guarded scope, or in what it reaches, is refused
# unless a row of its `ALLOWED` list (file, text, reason) allows it; a call of a worded
# `macro_rules!` is refused unless a row of its `MACROS` list (name, reason) allows it; the walk
# stops at the items of its `BOUNDARY` list (item, reason); and an edge the call graph cannot
# follow is refused unless a row of its `UNREAD` list (caller, name, reason) reviews it. The guard
# holds the tree to the lists both ways (a row nothing uses is refused there); this script is the
# history half: it compares the four lists with the merge base's, so they only shrink. A list the
# base does not have is empty there.
#
# A row is its list and its columns but the reason; a reason reworded is the same row.
# **A move is not growth** (`moved-rows.ps1`, -NoItem): an ALLOWED row whose text is a gone base
# row's, at another file, is that row moved, when the pairing is one-to-one; a move that arrives
# with more rows than it left is refused as grown.
#
# **A widening admits exactly its rows** (as `check-timing-bound.ps1`): a comment line
# `// widened <yyyy-mm-dd> by <ticket> (+<n> rows): <what>` in the guard that is new against the
# merge base admits exactly <n> added rows across the four lists. More than one new widening line, a
# count that is not the number of rows added, a `+0` widening, or a widening in a change that
# leaves the guard's grammar as the base has it (the file with its four lists, its comment
# lines and its blank lines taken out) is refused; rows added with no new widening line are refused.

param(
    [string]$Repo = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)),
    [string]$Relative = "crates/bt-source/tests/native_labels.rs",
    [string]$MainRef = "origin/main"
)

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

. (Join-Path $PSScriptRoot "moved-rows.ps1")

$guard = Join-Path $Repo $Relative
if (-not (Test-Path -LiteralPath $guard)) {
    throw "$Relative is not in the tree - it is the native-labels guard"
}

# The text of `const <Name>: ... = &[ ... ];` - from the `[` after `= &` to its matching `]`,
# strings and comments skipped - and where it stands, or $null when the file has no such list.
function Find-List([string]$Text, [string]$Name) {
    $head = [regex]::Match($Text, "(?m)^const $Name\s*:[^=]*=\s*&\[")
    if (-not $head.Success) { return $null }
    $open = $head.Index + $head.Length - 1
    $depth = 0
    $i = $open
    while ($i -lt $Text.Length) {
        $c = $Text[$i]
        if ($c -eq '"') {
            $i += 1
            while ($i -lt $Text.Length -and $Text[$i] -ne '"') {
                if ($Text[$i] -eq '\') { $i += 1 }
                $i += 1
            }
        } elseif ($c -eq '/' -and $i + 1 -lt $Text.Length -and $Text[$i + 1] -eq '/') {
            while ($i -lt $Text.Length -and $Text[$i] -ne "`n") { $i += 1 }
        } elseif ($c -eq '[') {
            $depth += 1
        } elseif ($c -eq ']') {
            $depth -= 1
            if ($depth -eq 0) {
                return [pscustomobject]@{ Start = $head.Index; End = $i + 1; Body = $Text.Substring($open + 1, $i - $open - 1) }
            }
        }
        $i += 1
    }
    throw "$Relative`: the list $Name never closes"
}

$stringPattern = '"((?:[^"\\]|\\.)*)"'

# The rows of one list: tuples of string literals, `arity` of them each.
function Read-Rows([string]$Text, [string]$Name, [int]$Arity) {
    $list = Find-List $Text $Name
    if ($null -eq $list) { throw "$Relative has no list $Name" }
    $parts = @(1..$Arity | ForEach-Object { $stringPattern }) -join '\s*,\s*'
    $tuple = '\(\s*' + $parts + '\s*,?\s*\)'
    $rows = @()
    $body = [regex]::Replace($list.Body, '(?m)//[^\n]*$', '')
    $rest = [regex]::Replace($body, $tuple, '', [Text.RegularExpressions.RegexOptions]::Singleline)
    if (($rest -replace '[\s,]', '').Length -gt 0) {
        throw "$Relative`: the list $Name holds something that is not a row of $Arity strings: $($rest.Trim())"
    }
    foreach ($match in [regex]::Matches($body, $tuple, [Text.RegularExpressions.RegexOptions]::Singleline)) {
        $values = @(1..$Arity | ForEach-Object { $match.Groups[$_].Value })
        if (@($values | Where-Object { $_.Length -eq 0 }).Count -gt 0) {
            throw "$Relative`: a row of $Name has an empty column: $($match.Value)"
        }
        $rows += , $values
    }
    return , $rows
}

# The lists, each with its arity; the last column of a row is its reason.
$lists = [ordered]@{ ALLOWED = 3; MACROS = 2; BOUNDARY = 2; UNREAD = 3 }

# The rows of the four lists, each a pscustomobject (Row = the identity text, Identity, Location,
# Tail). Only an ALLOWED row has a location (its file); the others are never moved.
function Read-AllRows([string]$Text) {
    $rows = @()
    foreach ($name in $lists.Keys) {
        foreach ($values in (Read-Rows $Text $name $lists[$name])) {
            $columns = @($values[0..($values.Count - 2)])
            if ($name -eq "ALLOWED") {
                $rows += [pscustomobject]@{ Row = "ALLOWED`t$($columns[0])`t$($columns[1])"; Identity = "ALLOWED`t$($columns[1])"; Location = $columns[0]; Tail = $columns[1] }
            } else {
                $row = "$name`t" + ($columns -join "`t")
                $rows += [pscustomobject]@{ Row = $row; Identity = $row; Location = $null; Tail = ($columns -join "`t") }
            }
        }
    }
    return , $rows
}

function Read-Widenings([string]$Text) {
    $found = @()
    foreach ($line in ($Text -split "`r?`n")) {
        if ($line -match '^\s*// widened (\d{4}-\d{2}-\d{2}) by (\S+) \(\+(\d+) rows?\)') {
            $found += [pscustomobject]@{ Line = $line.Trim(); Count = [int]$Matches[3] }
        }
    }
    return , $found
}

# The guard's grammar: the file with its four lists, every comment line and every blank line
# taken out.
function Get-Grammar([string]$Text) {
    foreach ($name in $lists.Keys) {
        $list = Find-List $Text $name
        if ($null -ne $list) { $Text = $Text.Substring(0, $list.Start) + $Text.Substring($list.End) }
    }
    return (($Text -split "`r?`n") | Where-Object { $_ -notmatch '^\s*(//|$)' } | ForEach-Object { $_.TrimEnd() }) -join "`n"
}

Push-Location $Repo
try {
    $text = [IO.File]::ReadAllText($guard)
    $now = Read-AllRows $text
    $widenedNow = Read-Widenings $text
    & git rev-parse --verify --quiet $MainRef *> $null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "no $MainRef in this clone - the native-labels comparison did not run"
        exit 2
    }
    $found = & git merge-base HEAD $MainRef 2>$null
    $base = if ($LASTEXITCODE -eq 0) { @($found)[0] } else { $null }
    if (-not $base) {
        Write-Host "no merge base with $MainRef in this clone - the native-labels comparison did not run"
        exit 2
    }
    $baseText = (& git show "${base}:${Relative}" 2>$null) -join "`n"
    if ($LASTEXITCODE -ne 0) {
        Write-Host "$Relative is not in $base - no base lists exist; the guard has $($now.Count) rows. PASS"
        exit 0
    }
} finally {
    Pop-Location
}

# A base without one of the lists (the guard before T-NATIVE-LABELS-GUARD-REACH) has no rows of it.
foreach ($name in $lists.Keys) {
    if ($null -eq (Find-List $baseText $name)) { $baseText += "`nconst ${name}: &[()] = &[];`n" }
}
$before = Read-AllRows $baseText
$widenedBefore = @((Read-Widenings $baseText) | ForEach-Object { $_.Line })
$grammarChanged = (Get-Grammar $text) -cne (Get-Grammar $baseText)

$ordinal = [StringComparer]::Ordinal
$left = [Collections.Generic.Dictionary[string, int]]::new($ordinal)
foreach ($row in $before) { if ($left.ContainsKey($row.Row)) { $left[$row.Row] += 1 } else { $left[$row.Row] = 1 } }
$unmatched = @()
foreach ($row in $now) {
    if ($left.ContainsKey($row.Row) -and $left[$row.Row] -gt 0) { $left[$row.Row] -= 1 } else { $unmatched += $row }
}
$leftover = @()
$spent = [Collections.Generic.Dictionary[string, int]]::new($ordinal)
foreach ($row in $before) {
    if (-not $spent.ContainsKey($row.Row)) { $spent[$row.Row] = 0 }
    $spent[$row.Row] += 1
    if ($spent[$row.Row] -le $left[$row.Row]) { $leftover += $row }
}

$moves = 0
$added = @()
$refused = @()
foreach ($result in (Find-MovedRows -Unmatched $unmatched -Leftover $leftover -Tree $now -Base $before -NoItem)) {
    $spelled = $result.Row -replace "`t", " "
    switch ($result.Verdict) {
        "moved" { $moves += 1; Write-Host "moved     $spelled (from $($result.Was))" }
        "added" { $added += $spelled }
        default { $refused += "    ${spelled}: $(Format-MoveRefusal $result)" }
    }
}
$removed = $leftover.Count - $moves
Write-Host "$Relative against $($base.Substring(0, 12)): $($before.Count) rows -> $($now.Count), $($added.Count) added, $removed removed, $moves moved."
if ($refused.Count -gt 0) {
    throw ("$($refused.Count) row(s) moved in a way a move cannot:" + [Environment]::NewLine + ($refused -join [Environment]::NewLine))
}

$widenings = @($widenedNow | Where-Object { $widenedBefore -notcontains $_.Line })
if ($widenings.Count -gt 1) {
    throw ("$($widenings.Count) widening lines are new against the base, and one change widens the guard once:" +
        [Environment]::NewLine + (($widenings | ForEach-Object { "    $($_.Line)" }) -join [Environment]::NewLine))
}
if ($widenings.Count -eq 1) {
    $widening = $widenings[0]
    if ($widening.Count -eq 0) {
        throw ("empty widening: the line admits no rows, and a widening that admits nothing is not one:" +
            [Environment]::NewLine + "    $($widening.Line)")
    }
    if (-not $grammarChanged) {
        throw ("unchanged grammar: the widening line is new, and the guard outside its lists and comments is as $($base.Substring(0, 12)) has it - a widening is a change to the guard's grammar:" +
            [Environment]::NewLine + "    $($widening.Line)")
    }
    if ($widening.Count -ne $added.Count) {
        throw ("the widening line admits $($widening.Count) row(s) and $($added.Count) were added:" +
            [Environment]::NewLine + "    $($widening.Line)")
    }
    Write-Host "the widening admits its $($added.Count) row(s): $($widening.Line)"
    Write-Host "the native-labels lists grew by exactly their widening. PASS"
    exit 0
}
if ($added.Count -gt 0) {
    throw ("$($added.Count) native-labels row(s) were added, and these lists only shrink:" +
        [Environment]::NewLine + (($added | ForEach-Object { "    $_" }) -join [Environment]::NewLine) + [Environment]::NewLine +
        "Pass the words in from the caller's i18n table instead, or widen the guard's grammar with a dated '// widened' line.")
}

Write-Host "the native-labels lists only shrank. PASS"
exit 0
