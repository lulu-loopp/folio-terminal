# M1 ConPTY 端到端验收；宽度视觉正确性请改用 width-probe-input.vt。
# 在 bt-app 中运行（路径相对仓库根）：& .\scripts\dev\width-acceptance.ps1
# 此处的竖线错位/间隙可能来自 ConPTY 的独立列记账，不作为网格宽度 FAIL。

$e = [char]27
function CodePoints { param([int[]]$Points) ($Points | ForEach-Object { [char]::ConvertFromUtf32($_) }) -join "" }

$family = CodePoints 0x1F468,0x200D,0x1F469,0x200D,0x1F467,0x200D,0x1F466
$thumbs = CodePoints 0x1F44D,0x1F3FD
$flag = CodePoints 0x1F1FA,0x1F1F8
$eacute = "e" + [char]0x0301
$mixed = "A" + [char]0x2606 + [char]0x4E2D + [char]0x2502 + [char]0xFF22
$umbrellaVS15 = [char]0x2602 + [char]0xFE0E
$umbrellaVS16 = [char]0x2602 + [char]0xFE0F

function Show { param([string]$Label, [string]$Text, [int]$Cells)
  Write-Host ("|" + $Text + "|")
  Write-Host ("|" + ("#" * $Cells) + "| " + $Label + "=" + $Cells)
}
function Pause-Page { [void](Read-Host "按 Enter 继续") }

Write-Host "[1/4] legacy 经 ConPTY：只验不崩溃/不残留；间隙不判 FAIL"
Show "family" $family 8
Show "skin" $thumbs 4
Show "flag" $flag 2
Pause-Page

Write-Host "[2/4] 发送 CSI ? 2027 h；确认后续仍完整可读"
Write-Host "$e[?2027h" -NoNewline
Show "family" $family 2
Show "skin" $thumbs 2
Show "flag" $flag 2
Show "combining" $eacute 1
Show "mixed" $mixed 7
Show "VS15" $umbrellaVS15 1
Show "VS16" $umbrellaVS16 2
Pause-Page

Write-Host "[3/4] 2027 + DECAWM off + 行尾晚变宽 panic 回归"
Write-Host "$e[?7l$e[999G" -NoNewline
Write-Host ([char]0x2602) -NoNewline
Write-Host ([char]0xFE0F)
Write-Host "$e[?7h" -NoNewline
Write-Host "PANIC PASS：此行可见，且屏幕无持续错乱"
Pause-Page

Write-Host "[4/4] 发送 CSI ? 2027 l，恢复 legacy"
Write-Host "$e[?2027l" -NoNewline
Show "family restored" $family 8
Write-Host "DONE：只据存活/无残留/协商序列透传判定；宽度改验直灌 fixture。"
