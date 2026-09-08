$esc = [char]27
1..50 | ForEach-Object {
    Write-Host -NoNewline ($esc + '[H' + $esc + '[2J')
    Write-Host ('full-screen redraw generation ' + $_)
    1..6 | ForEach-Object { Write-Host ('row ' + $_ + ' ########################') }
    Start-Sleep -Milliseconds 25
}

