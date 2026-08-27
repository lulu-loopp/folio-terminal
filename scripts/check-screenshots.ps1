# every_screenshot_the_readme_points_at_is_listed_and_is_the_right_size
#
# `docs/screenshots/README.md` is the list of shots: one table row per file, with
# the size the file is supposed to be and the hash it was committed with. This
# reads that table and refuses three things:
#
#   * a row naming a file that is not there. A README that links a picture nobody
#     took shows a broken image on the front page of the repository.
#   * a file that is not the size its row claims. The shots are 3200 x 2000
#     physical pixels - 1600 x 1000 logical at 200%, two device pixels to the
#     logical one - and the one failure mode that cannot be seen by looking at
#     the picture is a resampled one: a screenshot of a terminal scaled by a
#     resampler is a screenshot of the resampler. A file that measures 1600 x
#     1000 is one of the earlier 100% pass, not a retake.
#   * a picture either README references that the table does not list. The table
#     is what says how a shot was taken and what it is of; an image that appears
#     on the page without a row in it is one nobody can retake.
#
# The size is read out of the PNG header rather than by decoding the file: the
# first chunk of a PNG is `IHDR`, and its width and height are the two big-endian
# 32-bit words at offset 16. That is eight bytes of arithmetic instead of a
# dependency on an imaging stack, and it cannot be fooled by metadata.
#
# What this gate does NOT check is what is IN the picture. No script reads
# pixels for a user name in a path or a real repository in a files column;
# `docs/screenshots/README.md` says so in its own words, and that one is on
# whoever takes the shot.

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot
$shotDir = Join-Path $repo "docs\screenshots"
$manifest = Join-Path $shotDir "README.md"

function Get-PngSize([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    try {
        $head = New-Object byte[] 24
        if ($stream.Read($head, 0, 24) -ne 24) { return $null }
        $signature = [byte[]](0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A)
        for ($i = 0; $i -lt 8; $i++) { if ($head[$i] -ne $signature[$i]) { return $null } }
        if ([Text.Encoding]::ASCII.GetString($head, 12, 4) -ne "IHDR") { return $null }
        $w = ([int]$head[16] -shl 24) -bor ([int]$head[17] -shl 16) -bor ([int]$head[18] -shl 8) -bor [int]$head[19]
        $h = ([int]$head[20] -shl 24) -bor ([int]$head[21] -shl 16) -bor ([int]$head[22] -shl 8) -bor [int]$head[23]
        return [pscustomobject]@{ Width = $w; Height = $h }
    } finally {
        $stream.Dispose()
    }
}

$problems = New-Object System.Collections.Generic.List[string]

if (-not (Test-Path -LiteralPath $manifest)) {
    throw "docs/screenshots/README.md is missing - it is the list this gate reads"
}

# Table rows naming a file, and table rows carrying a size, read separately and
# joined by name. The document says the same fourteen things twice on purpose -
# once as what each shot is of, once as what was taken and when - so a file's
# name and the size it should be do not have to arrive in the same row.
$listed = New-Object System.Collections.Generic.List[string]
$sizes = @{}
foreach ($line in [IO.File]::ReadAllLines($manifest)) {
    if ($line -notmatch '^\s*\|') { continue }
    $m = [regex]::Match($line, '`([A-Za-z0-9._@-]+\.png)`')
    if (-not $m.Success) { continue }
    $name = $m.Groups[1].Value
    if (-not $listed.Contains($name)) { $listed.Add($name) }
    $size = [regex]::Match($line, '(\d{3,5})\s*[x×]\s*(\d{3,5})')
    if ($size.Success) {
        $sizes[$name] = [pscustomobject]@{
            Width  = [int]$size.Groups[1].Value
            Height = [int]$size.Groups[2].Value
        }
    }
}

if ($listed.Count -eq 0) {
    throw "docs/screenshots/README.md lists no shots - the gate has nothing to check"
}

foreach ($name in $listed) {
    $path = Join-Path $shotDir $name
    if (-not (Test-Path -LiteralPath $path)) {
        $problems.Add("$name is listed but is not in docs/screenshots/")
        continue
    }
    if (-not $sizes.ContainsKey($name)) {
        $problems.Add("$name is listed without a size anywhere - the list has to say what the file should measure")
        continue
    }
    $size = Get-PngSize $path
    if ($null -eq $size) {
        $problems.Add("$name is not a PNG - the list says PNG and a screenshot of text is exactly the picture JPEG is worst at")
        continue
    }
    $want = $sizes[$name]
    if ($size.Width -ne $want.Width -or $size.Height -ne $want.Height) {
        $problems.Add("$name is $($size.Width)x$($size.Height); the list says $($want.Width)x$($want.Height)")
    }
}

# What the two READMEs actually put on the page.
foreach ($readme in @("README.md", "README.zh-CN.md")) {
    $path = Join-Path $repo $readme
    if (-not (Test-Path -LiteralPath $path)) { continue }
    $text = [IO.File]::ReadAllText($path)
    foreach ($m in [regex]::Matches($text, 'docs/screenshots/([A-Za-z0-9._@-]+\.png)')) {
        $name = $m.Groups[1].Value
        if ($listed -notcontains $name) {
            $problems.Add("$readme shows $name, which docs/screenshots/README.md does not list")
        }
    }
}

if ($problems.Count -gt 0) {
    $unique = $problems | Select-Object -Unique
    throw ("the screenshots and their list disagree:" +
        [Environment]::NewLine + ($unique -join [Environment]::NewLine))
}

Write-Host "$($listed.Count) screenshots listed, all present at the size their row claims; both READMEs link only listed files"
