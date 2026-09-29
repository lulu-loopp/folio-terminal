<#
.SYNOPSIS
    Guest side of the updater rows (clean-vm.md 4.4): what the guest looks like
    now, as text, zipped for copyFileFromGuestToHost.

.DESCRIPTION
    Runs in the guest under Windows PowerShell 5.1. Collects into
    -GuestHome\updater-out-<Tag> and zips it to -GuestHome\updater-<Tag>.zip:
      install-listing.txt   every file of the install folder with its SHA-256
                            (folio.exe with its FileVersion); the two 79 MB
                            executables of H are listed, never copied (H-2)
      journal.json          read with every sharing flag and closed at once
      txn-*                 the small files of each transaction (owner, receipts)
      diagnostics.log, update-check.json, appdata-folio.txt
      run-key.reg           HKCU ...\Run
      processes.txt         folio.exe and OpenConsole.exe with their command lines
      updater-*.txt         this run's watcher, journal history, keys and marker
      shots\                the key driver's pictures
#>
param([string]$Tag = 'run', [string]$GuestHome = 'C:\folio-vm')
$ErrorActionPreference = 'Continue'
$install = Join-Path $GuestHome 'folio'
$H = Join-Path $install '.folio-update'
$out = Join-Path $GuestHome "updater-out-$Tag"
if (Test-Path -LiteralPath $out) { Remove-Item -LiteralPath $out -Recurse -Force }
[IO.Directory]::CreateDirectory($out) | Out-Null

"collected $((Get-Date).ToString('yyyy-MM-dd HH:mm:ss.fff zzz'))" | Set-Content (Join-Path $out 'when.txt')
Get-ChildItem -LiteralPath $install -Recurse -Force -ErrorAction SilentlyContinue | Sort-Object FullName | ForEach-Object {
  if ($_.PSIsContainer) { '{0,-64} {1,12} {2}' -f '<dir>', '', $_.FullName }
  else {
    $hash = try { (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256 -ErrorAction Stop).Hash.ToLowerInvariant() } catch { "<unreadable: $($_.Exception.Message)>" }
    $v = if ($_.Name -eq 'folio.exe') { " v=$($_.VersionInfo.FileVersion)" } else { '' }
    '{0,-64} {1,12} {2}{3}' -f $hash, $_.Length, $_.FullName, $v
  }
} | Set-Content -LiteralPath (Join-Path $out 'install-listing.txt') -Encoding UTF8

if (Test-Path -LiteralPath (Join-Path $H 'journal.json')) {
  try {
    $fs = [IO.File]::Open((Join-Path $H 'journal.json'), 'Open', 'Read', [IO.FileShare]'ReadWrite, Delete')
    try { [IO.File]::WriteAllText((Join-Path $out 'journal.json'), (New-Object IO.StreamReader($fs)).ReadToEnd()) } finally { $fs.Dispose() }
  } catch { "journal.json unreadable: $($_.Exception.Message)" | Set-Content (Join-Path $out 'journal.json.err') }
}
Get-ChildItem -LiteralPath $H -Directory -ErrorAction SilentlyContinue | ForEach-Object {
  Get-ChildItem -LiteralPath $_.FullName -File -ErrorAction SilentlyContinue | Where-Object { $_.Length -lt 1MB -and $_.Extension -notin '.exe', '.dll', '.zip', '.msix' } | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $out ('txn-' + $_.Name)) -ErrorAction SilentlyContinue
  }
}
$data = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'Folio'
foreach ($name in 'diagnostics.log', 'update-check.json') {
  $f = Join-Path $data $name
  if (Test-Path -LiteralPath $f) { Copy-Item -LiteralPath $f -Destination $out }
}
Get-ChildItem -LiteralPath $data -Force -ErrorAction SilentlyContinue | ForEach-Object { '{0,12} {1} {2}' -f $_.Length, $_.LastWriteTime.ToString('HH:mm:ss'), $_.Name } | Set-Content (Join-Path $out 'appdata-folio.txt')
& reg.exe export 'HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run' (Join-Path $out 'run-key.reg') /y 2>&1 | Out-Null
@(Get-CimInstance Win32_Process -Filter "Name='folio.exe' OR Name='OpenConsole.exe'" -ErrorAction SilentlyContinue | ForEach-Object { "$($_.ProcessId) parent=$($_.ParentProcessId) created=$($_.CreationDate) $($_.ExecutablePath) :: $($_.CommandLine)" }) | Set-Content (Join-Path $out 'processes.txt')
Get-ChildItem -LiteralPath $GuestHome -Filter 'updater-*.txt' -File | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $out }
if (Test-Path -LiteralPath (Join-Path $GuestHome 'shots')) { Copy-Item -LiteralPath (Join-Path $GuestHome 'shots') -Destination (Join-Path $out 'shots') -Recurse }
$zip = Join-Path $GuestHome "updater-$Tag.zip"
if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
Compress-Archive -Path "$out\*" -DestinationPath $zip -Force
