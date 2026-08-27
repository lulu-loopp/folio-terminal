# Writes THIRD-PARTY-NOTICES.md.
#
# Three sources, concatenated in this order:
#   licenses/notices-preamble.md   — what the file is
#   licenses/bundled-assets.md     — the components Cargo.lock cannot see, with
#                                    every `<!-- verbatim: path -->` marker
#                                    replaced by that file's exact bytes
#   cargo about                    — every package in Cargo.lock
#
# Deterministic: same lock file and same inputs, same bytes out. `check-notices.ps1`
# depends on that.
#
#   -OutFile <path>   write somewhere else (the drift check writes to a temp file)

param([string]$OutFile = "")

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot
if (-not $OutFile) { $OutFile = Join-Path $repo "THIRD-PARTY-NOTICES.md" }

function Read-Text([string]$path) {
    # Normalise to LF here rather than trusting whatever checked the file out:
    # the drift check compares bytes, and a fresh clone on a machine with
    # core.autocrlf=true must not read as a drift.
    $bytes = [IO.File]::ReadAllBytes($path)
    $text = [Text.Encoding]::UTF8.GetString($bytes)
    if ($text.Length -gt 0 -and $text[0] -eq [char]0xFEFF) { $text = $text.Substring(1) }
    return $text.Replace("`r`n", "`n")
}

function Get-Fence([string]$body) {
    # A fence must be longer than the longest run of backticks inside what it
    # fences, or a licence text that happens to contain a fence of its own cuts
    # the block in half.
    $tick = [char]0x60
    $longest = 0
    foreach ($m in [regex]::Matches($body, [regex]::Escape([string]$tick) + "+")) {
        if ($m.Length -gt $longest) { $longest = $m.Length }
    }
    return [string]::new($tick, [Math]::Max(3, $longest + 1))
}

$out = New-Object System.Text.StringBuilder

[void]$out.Append((Read-Text (Join-Path $repo "licenses/notices-preamble.md")).TrimEnd("`n"))
[void]$out.Append("`n`n---`n`n")

$assets = Read-Text (Join-Path $repo "licenses/bundled-assets.md")
$marker = [regex]"(?m)^<!--[ \t]*verbatim:[ \t]*(?<path>[^>]+?)[ \t]*-->[ \t]*$"

$expanded = New-Object System.Text.StringBuilder
$pos = 0
$found = 0
foreach ($m in $marker.Matches($assets)) {
    [void]$expanded.Append($assets.Substring($pos, $m.Index - $pos))
    $rel = $m.Groups["path"].Value
    $full = Join-Path $repo $rel
    if (-not (Test-Path -LiteralPath $full)) {
        throw "licenses/bundled-assets.md points at a file that is not here: $rel"
    }
    $body = (Read-Text $full).TrimEnd("`n")
    $fence = Get-Fence $body
    [void]$expanded.Append("$fence text`n$body`n$fence")
    $pos = $m.Index + $m.Length
    $found++
}
[void]$expanded.Append($assets.Substring($pos))
if ($found -eq 0) { throw "licenses/bundled-assets.md has no <!-- verbatim: ... --> markers — the licence texts would be silently missing" }
[void]$out.Append($expanded.ToString().TrimEnd("`n"))
[void]$out.Append("`n`n---`n`n")

# `-o` rather than a pipe: cargo-about refuses to have its stdout redirected under
# PowerShell, and it is right to — PowerShell re-encodes a captured native stream.
$aboutFile = Join-Path ([IO.Path]::GetTempPath()) ("folio-about-" + [Guid]::NewGuid().ToString("N") + ".md")
Push-Location $repo
try {
    & cargo about generate --locked --workspace --fail -o $aboutFile about.hbs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo about failed (installed? ``cargo install cargo-about --locked --features cli``)"
    }
    $cratesText = (Read-Text $aboutFile).TrimEnd("`n")
} finally {
    Pop-Location
    Remove-Item -LiteralPath $aboutFile -ErrorAction SilentlyContinue
}

# about.hbs fences each licence in eight backticks, which handlebars cannot pick
# to fit its contents. Nothing in a licence text has ever contained a run that
# long — but "has ever" is not "cannot", and a fence that closes early would cut
# a legal document in half silently.
$tick = [regex]::Escape([string][char]0x60)
$lines = $cratesText -split "`n"
$fences = @($lines | Where-Object { $_ -match ("^" + $tick + "{8}") }).Count
$headings = @($lines | Where-Object { $_ -match "^### " }).Count
if ($fences -ne 2 * $headings) {
    throw "expected $(2 * $headings) fence lines for $headings licences, found $fences — a licence text contains an eight-backtick run and the fence in about.hbs no longer encloses it"
}

[void]$out.Append($cratesText)
[void]$out.Append("`n")

# Cargo.lock is a superset of any one build: it keeps entries for optional and
# platform-specific dependencies that no feature resolution in this workspace
# actually pulls in, and cargo-about walks the resolved graph rather than the
# lock file. The difference is small and it is not nothing, so it is stated
# rather than left as a discrepancy between two counts a reader might compare.
$attributed = [Collections.Generic.HashSet[string]]::new()
foreach ($m in [regex]::Matches($cratesText, "(?m)^- \*\*([A-Za-z0-9_.+-]+) ([^*]+)\*\*")) {
    [void]$attributed.Add($m.Groups[1].Value + " " + $m.Groups[2].Value.Trim())
}

$lock = Read-Text (Join-Path $repo "Cargo.lock")
$locked = @()
foreach ($block in ($lock -split "\[\[package\]\]")) {
    $n = [regex]::Match($block, "(?m)^name = ""([^""]+)""")
    $v = [regex]::Match($block, "(?m)^version = ""([^""]+)""")
    $s = [regex]::Match($block, "(?m)^source = ""registry\+")
    if ($n.Success -and $v.Success -and $s.Success) {
        $locked += , @($n.Groups[1].Value, $v.Groups[1].Value)
    }
}

$unresolved = @($locked | Where-Object { -not $attributed.Contains($_[0] + " " + $_[1]) })

Push-Location $repo
try {
    $metaJson = & cargo metadata --format-version 1 --locked 2>$null
    if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed" }
} finally {
    Pop-Location
}
$licenseOf = @{}
foreach ($p in (($metaJson -join "") | ConvertFrom-Json).packages) {
    $licenseOf[$p.name + " " + $p.version] = $p.license
}

$resolvedCount = $locked.Count - $unresolved.Count
$intro = @'
---

## In the lock file, not in any resolved build

`Cargo.lock` pins {TOTAL} packages from crates.io. {RESOLVED} of them are in the
dependency graph this workspace resolves, and are attributed above with their
licence texts. The {UNRESOLVED} below are optional or platform-specific entries
that no feature resolution here reaches: nothing links them, so nothing
distributes them. They are listed with the terms they declare, so that the two
counts a reader might compare are reconciled here rather than left as a gap.

| Package | Declared licence |
|---|---|
'@
$intro = $intro.Replace("{TOTAL}", "$($locked.Count)").Replace("{RESOLVED}", "$resolvedCount").Replace("{UNRESOLVED}", "$($unresolved.Count)")
[void]$out.Append("`n" + $intro.Replace("`r`n", "`n") + "`n")
$tick = [string][char]0x60
foreach ($p in ($unresolved | Sort-Object { $_[0] }, { $_[1] })) {
    $key = $p[0] + " " + $p[1]
    $lic = $licenseOf[$key]
    if (-not $lic) { $lic = "*not declared in its manifest*" }
    [void]$out.Append("| $tick$key$tick | $lic |`n")
}

$final = $out.ToString()
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[IO.File]::WriteAllBytes($OutFile, $utf8NoBom.GetBytes($final))
Write-Host "wrote $OutFile ($($final.Length) chars)"
