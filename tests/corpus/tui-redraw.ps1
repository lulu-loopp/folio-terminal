$esc = [char]27
1..35 | ForEach-Object {
    Write-Host -NoNewline ($esc + '[H' + $esc + '[2J')
    Write-Host 'Agent-compatible TUI substitute'
    Write-Host ('stream item ' + $_)
    Write-Host '----------------------------------------'
    Write-Host -NoNewline ('bottom status: running ' + $_ + '/35')
    Start-Sleep -Milliseconds 20
}
Write-Host "`ncomplete"

