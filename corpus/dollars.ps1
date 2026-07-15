Write-Output '$PATH $HOME ${env:USERPROFILE}'
Write-Output "awk '{print `$1, `$2}'"
1..80 | ForEach-Object { Write-Output ('price=$' + $_ + ' literal=$$ prompt=$ ') }

