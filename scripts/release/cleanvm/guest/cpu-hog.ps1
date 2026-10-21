<#
.SYNOPSIS
    Guest side of the load row (clean-vm.md 4.7): load every processor of the guest at Normal
    priority for a while, the way three cargo builds load the owner's machine.

.DESCRIPTION
    Runs in the guest under Windows PowerShell 5.1. Starts -Threads spinners (default: twice
    the guest's logical processors), each its own powershell.exe at the Normal priority class
    spinning one thread until -Seconds have passed, and waits for all of them. A spinner ends
    by itself at its deadline, so nothing has to stop it; this script ends no process.

    Writes -Log: the start and the end, in UTC with milliseconds, the thread count and the pids.
#>
param(
  [int]$Threads = 0,
  [int]$Seconds = 60,
  [string]$Log = 'C:\folio-vm\cpu-hog.txt'
)
$ErrorActionPreference = 'Stop'
if ($Threads -le 0) { $Threads = 2 * [Environment]::ProcessorCount }
function Stamp { [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ') }
function Say($s) { Add-Content -LiteralPath $Log -Value "$(Stamp) $s" -Encoding UTF8 }

$spin = "`$end = [DateTime]::UtcNow.AddSeconds($Seconds); while ([DateTime]::UtcNow -lt `$end) { }"
$spinners = @()
for ($i = 0; $i -lt $Threads; $i++) {
  $spinners += Start-Process -FilePath 'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe' `
    -ArgumentList @('-NoProfile', '-Command', $spin) -WindowStyle Hidden -PassThru
}
foreach ($spinner in $spinners) { $spinner.PriorityClass = [Diagnostics.ProcessPriorityClass]::Normal }
Say ("hog start threads={0} processors={1} seconds={2} pids={3}" -f $Threads, [Environment]::ProcessorCount, $Seconds, (($spinners | ForEach-Object { $_.Id }) -join ','))
foreach ($spinner in $spinners) { $spinner.WaitForExit() }
Say 'hog end'
