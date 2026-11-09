# every_broken_doc_link_is_a_row_and_every_row_a_broken_link (T-DOC-LINK-DEBT, 0.4.9 plan row 78)
#
# The measuring half of `docs/plans/DOC-LINK-DEBT.tsv`. It runs
#
#     cargo doc -p bt-app --no-deps --locked --message-format=json
#
# and keeps every `compiler-message` whose lint code (`message.code.code`) is
# `rustdoc::broken_intra_doc_links` - that code, never the message's text, is what a warning is
# kept by - writing each one's primary span (file, line, column) and message to
# `target/doc-links/measured.tsv`. The other rustdoc lints (`rustdoc::invalid_html_tags`,
# `rustdoc::bare_urls`, ...) are counted by code and printed, and gate nothing (ruling R6).
#
# Then it runs bt-source's `doc_link_debt` test target by name (`cargo test -p bt-source --locked
# --test doc_link_debt`), which keys each warning to the item whose documentation holds it,
# writes the rendering to `target/doc-link-debt.tsv`, and fails on any difference from the
# committed list: a broken link the list does not hold, or a row no broken link is. The grammar
# (which message forms, which positions, what is refused) is that target's module doc. This
# script's exit code is the target's.
#
# A `cargo doc` that fails, a warning with no single primary span, or a message with a tab or a
# line break in it ends the script before the target runs: a measurement that did not happen
# must not look like one that agreed. Not a `check-*` script: it compiles, so locally it runs
# only on a lane, and in CI in the `docs` job.
# `scripts/dev/generate-doc-link-debt.ps1` runs it and copies the rendering over the list.

$ErrorActionPreference = "Stop"

# cargo's exit code is read by hand below; a non-zero one must not end the script first.
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$folder = Join-Path $repo "target/doc-links"
$measured = Join-Path $folder "measured.tsv"
$lint = "rustdoc::broken_intra_doc_links"

New-Item -ItemType Directory -Force -Path $folder | Out-Null
if (Test-Path -LiteralPath $measured) { Remove-Item -LiteralPath $measured -Force }

Push-Location $repo
try {
    $started = Get-Date
    $stream = @(& cargo doc -p bt-app --no-deps --locked --message-format=json)
    $status = $LASTEXITCODE
    $took = (Get-Date) - $started
} finally {
    Pop-Location
}
if ($status -ne 0) {
    throw "cargo doc -p bt-app --no-deps exited $status - nothing was measured (its errors are above)"
}

$kept = [Collections.Generic.List[string]]::new()
$others = [ordered]@{}
foreach ($line in $stream) {
    if (-not $line.StartsWith("{")) { continue }
    $record = $line | ConvertFrom-Json -Depth 64
    if ($record.reason -ne "compiler-message") { continue }
    $message = $record.message
    $code = if ($null -ne $message.code) { $message.code.code } else { $null }
    if ($code -ceq $lint) {
        $primary = @($message.spans | Where-Object { $_.is_primary })
        if ($primary.Count -ne 1) {
            throw "a $lint warning with $($primary.Count) primary spans, not one: $($message.message)"
        }
        $text = [string]$message.message
        if ($text.Contains("`t") -or $text.Contains("`n") -or $text.Contains("`r")) {
            throw "a $lint message with a tab or a line break in it, which the measured file cannot hold: $text"
        }
        $span = $primary[0]
        $kept.Add("$($span.file_name)`t$($span.line_start)`t$($span.column_start)`t$text")
    } elseif ($null -ne $code -and $code.StartsWith("rustdoc::")) {
        if (-not $others.Contains($code)) { $others[$code] = 0 }
        $others[$code] += 1
    }
}

$utf8 = New-Object System.Text.UTF8Encoding($false)
$body = "file`tline`tcolumn`tmessage`n" + (($kept | ForEach-Object { "$_`n" }) -join "")
[IO.File]::WriteAllText($measured, $body, $utf8)

Write-Host ("cargo doc -p bt-app --no-deps: {0} {1} warning(s) in {2:N0} s, written to target/doc-links/measured.tsv" -f $kept.Count, $lint, $took.TotalSeconds)
if ($others.Count -eq 0) {
    Write-Host "other rustdoc lints: none"
} else {
    foreach ($entry in $others.GetEnumerator()) {
        Write-Host "other rustdoc lint, counted and not gated: $($entry.Key) x$($entry.Value)"
    }
}

Push-Location $repo
try {
    & cargo test -p bt-source --locked --test doc_link_debt -- --nocapture | Out-Host
    $keyed = $LASTEXITCODE
} finally {
    Pop-Location
}
exit $keyed
