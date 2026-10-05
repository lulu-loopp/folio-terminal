# Shared scope for the tracked-text integrity gates. Keep exclusions here so a
# file cannot be text for one integrity check and invisible to the other.

$script:TrackedTextBinaryExtensions = @(
    '.avi', '.bin', '.btcr', '.dat', '.dll', '.exe', '.gif', '.icc', '.ico',
    '.jpeg', '.jpg', '.mkv', '.mov', '.mp4', '.nupkg', '.otf', '.packdump',
    '.pdf', '.pfb', '.png', '.recording', '.ttf', '.vte', '.webm', '.wmv',
    '.woff', '.woff2', '.zip'
)

function Test-VendoredThirdPartyNotice([string] $Relative) {
    $path = $Relative.Replace('\', '/')
    if ($path -eq 'THIRD-PARTY-NOTICES.md' -or $path.StartsWith('licenses/')) {
        return $true
    }
    if (-not $path.StartsWith('vendor/')) { return $false }
    if ($path -match '/licenses?/') { return $true }
    $name = [IO.Path]::GetFileName($path)
    return $name -match '^(?i:LICENSE|LICENCE|COPYING|NOTICE|AUTHORS|CONTRIBUTORS)(?:[.-].*)?$'
}

function Get-TrackedTextFiles([string] $Repo) {
    Push-Location $Repo
    try {
        $tracked = @(& git -c core.quotepath=false ls-files)
        if ($LASTEXITCODE -ne 0) { throw 'git ls-files failed' }
    } finally {
        Pop-Location
    }

    foreach ($relative in $tracked) {
        if (Test-VendoredThirdPartyNotice $relative) { continue }
        $extension = [IO.Path]::GetExtension($relative).ToLowerInvariant()
        if ($script:TrackedTextBinaryExtensions -contains $extension) { continue }
        $full = Join-Path $Repo $relative
        if (-not (Test-Path -LiteralPath $full -PathType Leaf)) {
            throw "$relative is tracked but cannot be read from the working tree"
        }
        [pscustomobject]@{ Relative = $relative; Full = $full }
    }
}
