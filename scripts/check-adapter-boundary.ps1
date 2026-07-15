$ErrorActionPreference = "Stop"

$files = @(
    "crates/bt-term/src/adapter.rs",
    "crates/bt-term/src/cell_capture.rs"
)
$forbidden = '\bbt_(doc|detect|viewport)\b'
$violations = Select-String -Path $files -Pattern $forbidden

if ($violations) {
    $details = ($violations | ForEach-Object {
        "$($_.Path):$($_.LineNumber): $($_.Line.Trim())"
    }) -join [Environment]::NewLine
    throw "adapter boundary imports a policy crate:$([Environment]::NewLine)$details"
}
