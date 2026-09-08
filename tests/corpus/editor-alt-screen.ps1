$esc = [char]27
Write-Host -NoNewline ($esc + '[?1049h' + $esc + '[H' + $esc + '[2J')
Write-Host 'editor substitute (vim unavailable)'
Write-Host '~'
Write-Host '~'
Write-Host -NoNewline '-- INSERT --'
Start-Sleep -Milliseconds 80
Write-Host -NoNewline ($esc + '[HBetterTerminal M-1')
Start-Sleep -Milliseconds 80
Write-Host -NoNewline ($esc + '[?1049l')
Write-Host 'editor exited'

