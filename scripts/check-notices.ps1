# the_notices_file_matches_the_lock
#
# A checked-in THIRD-PARTY-NOTICES.md is a promise about `Cargo.lock`, and the
# next `cargo update` breaks it silently: nothing about a stale attribution file
# fails a build. So regenerate it into a temporary file and demand the bytes
# match.
#
# This is also why the file is generated rather than written. A file a human
# maintains is a file a human forgets.

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot
$checked = Join-Path $repo "THIRD-PARTY-NOTICES.md"

if (-not (Test-Path -LiteralPath $checked)) {
    throw "THIRD-PARTY-NOTICES.md is missing — run ./scripts/generate-notices.ps1"
}

# The Microsoft Terminal MIT text lives twice on purpose: beside the ConPTY
# packages it covers, and in licenses/ where a reader looks for licence texts.
# Twice is fine; twice and different is not.
$a = [IO.File]::ReadAllBytes((Join-Path $repo "vendor/conpty/LICENSE-MICROSOFT-TERMINAL"))
$b = [IO.File]::ReadAllBytes((Join-Path $repo "licenses/microsoft-terminal-LICENSE.txt"))
if (-not [Linq.Enumerable]::SequenceEqual($a, $b)) {
    throw "vendor/conpty/LICENSE-MICROSOFT-TERMINAL and licenses/microsoft-terminal-LICENSE.txt have drifted apart"
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("folio-notices-" + [Guid]::NewGuid().ToString("N") + ".md")
try {
    & (Join-Path $PSScriptRoot "generate-notices.ps1") -OutFile $tmp | Out-Null

    # Compare with line endings normalised. The generator always writes LF, but a
    # clone made with core.autocrlf=true has CRLF in the working tree, and that is
    # git's business rather than a drift in what the file says.
    $want = ([Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes($tmp))).Replace("`r`n", "`n")
    $have = ([Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes($checked))).Replace("`r`n", "`n")

    if ($want -cne $have) {
        # Say where, not just that: a 500 KB diff is unreadable, one line number is not.
        $wantLines = $want -split "`n"
        $haveLines = $have -split "`n"
        $n = [Math]::Min($wantLines.Length, $haveLines.Length)
        $at = $null
        for ($i = 0; $i -lt $n; $i++) {
            if ($wantLines[$i] -cne $haveLines[$i]) { $at = $i; break }
        }
        $detail = if ($null -ne $at) {
            "first difference at line $($at + 1):" + [Environment]::NewLine +
            "  checked in: $($haveLines[$at])" + [Environment]::NewLine +
            "  regenerated: $($wantLines[$at])"
        } else {
            "identical for $n lines, then lengths differ: checked in $($haveLines.Length), regenerated $($wantLines.Length)"
        }
        throw ("THIRD-PARTY-NOTICES.md does not match Cargo.lock and licenses/." +
            [Environment]::NewLine + $detail + [Environment]::NewLine +
            "Run ./scripts/generate-notices.ps1 and commit the result.")
    }

    Write-Host "THIRD-PARTY-NOTICES.md matches Cargo.lock ($($have.Length) characters)"
} finally {
    Remove-Item -LiteralPath $tmp -ErrorAction SilentlyContinue
}
