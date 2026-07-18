param(
    [Parameter(Mandatory = $true)]
    [string] $Package,

    [Parameter(Mandatory = $true)]
    [string] $Destination,

    [Parameter(Mandatory = $true)]
    [string] $TestDestination
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$expectedPackageHash = '05fe9b571ea4fb198f5012405cb39a132cf23eee50feaa496524c149b2502692'
$entries = @(
    @{
        Entry = 'runtimes/win-x64/native/conpty.dll'
        File = 'conpty.dll'
        Hash = 'e2fe87e2258c4e46ffc5157f727218cc25f34a174902f72eb8a5b49edd9a6458'
    },
    @{
        Entry = 'build/native/runtimes/x64/OpenConsole.exe'
        File = 'OpenConsole.exe'
        Hash = '2525c351aa136d555e5df9a3c9d6ce9be43f785e37e3c993b8f23b3f0a53c7fa'
    }
)

function Get-LowerSha256([string] $Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-StreamSha256([System.IO.Stream] $Stream) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $bytes = $sha.ComputeHash($Stream)
        return (($bytes | ForEach-Object { $_.ToString('x2') }) -join '')
    }
    finally {
        $sha.Dispose()
    }
}

if ((Get-LowerSha256 $Package) -ne $expectedPackageHash) {
    throw "ConPTY NuGet package SHA-256 does not match the pinned official asset: $Package"
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead((Resolve-Path -LiteralPath $Package))
try {
    foreach ($item in $entries) {
        $entry = $archive.GetEntry($item.Entry)
        if ($null -eq $entry) {
            throw "ConPTY NuGet package is missing $($item.Entry)"
        }

        $source = $entry.Open()
        try {
            $actualEntryHash = Get-StreamSha256 $source
        }
        finally {
            $source.Dispose()
        }
        if ($actualEntryHash -ne $item.Hash) {
            throw "ConPTY package entry SHA-256 does not match $($item.Entry)"
        }

        foreach ($directory in @($Destination, $TestDestination)) {
            $relativeTargets = @($item.File)
            if ($item.File -eq 'OpenConsole.exe') {
                # The native NuGet targets place architecture hosts below the DLL directory.
                # Keep the requested application-directory pair as well as the official x64 path.
                $relativeTargets += 'x64\OpenConsole.exe'
            }
            foreach ($relativeTarget in $relativeTargets) {
                $target = Join-Path $directory $relativeTarget
                [System.IO.Directory]::CreateDirectory(
                    [System.IO.Path]::GetDirectoryName($target)
                ) | Out-Null
                if ((Test-Path -LiteralPath $target -PathType Leaf) -and
                    (Get-LowerSha256 $target) -eq $item.Hash) {
                    continue
                }

                $temporary = "$target.bt-extract-$([System.Guid]::NewGuid().ToString('N')).tmp"
                try {
                    $source = $entry.Open()
                    try {
                        $output = [System.IO.File]::Open(
                            $temporary,
                            [System.IO.FileMode]::CreateNew,
                            [System.IO.FileAccess]::Write,
                            [System.IO.FileShare]::None
                        )
                        try {
                            $source.CopyTo($output)
                        }
                        finally {
                            $output.Dispose()
                        }
                    }
                    finally {
                        $source.Dispose()
                    }

                    if ((Get-LowerSha256 $temporary) -ne $item.Hash) {
                        throw "Extracted ConPTY sidecar SHA-256 does not match $($item.File)"
                    }
                    if (Test-Path -LiteralPath $target -PathType Leaf) {
                        $backup = "$target.bt-backup-$([System.Guid]::NewGuid().ToString('N')).tmp"
                        try {
                            [System.IO.File]::Replace($temporary, $target, $backup)
                        }
                        finally {
                            if (Test-Path -LiteralPath $backup) {
                                Remove-Item -LiteralPath $backup -Force
                            }
                        }
                    }
                    else {
                        [System.IO.File]::Move($temporary, $target)
                    }
                }
                finally {
                    if (Test-Path -LiteralPath $temporary) {
                        Remove-Item -LiteralPath $temporary -Force
                    }
                }
            }
        }
    }
}
finally {
    $archive.Dispose()
}
