<#
.SYNOPSIS
    Who signed a release's two signed files, compared as names. Dot-sourced by
    `smoke.ps1` (the package's Publisher against its signer, and the package's
    signer against the executable's) and `release-statement.ps1` (one
    certificate on both files, naming the package's Publisher, before a
    statement says `authenticode true`); it runs nothing on its own.

.DESCRIPTION
    Moved out of `smoke.ps1` so that the two scripts ask the one question one
    way (0.4.10 plan row 102, the 1b+2 review's P2-7). It is copied into the
    clean guest beside `smoke.ps1`, so it is read by Windows PowerShell 5.1
    there: it carries a UTF-8 byte-order mark and speaks that dialect.
#>

# **A distinguished name is compared as a name and not as a string.**
# `CN=Weiyi Shi, O=Weiyi Shi` and `CN=Weiyi Shi,O=Weiyi Shi` are the same name
# written twice: the space after a separating comma is spelling and not content,
# and which of the two a certificate authority, a Windows API and a person
# editing XML each produce is not something this check gets to decide. Compared
# as text they differ, and the failure sends somebody to edit a file that was
# already correct — which is worse than not checking at all, because it teaches
# people to distrust the check.
#
# So each side is cut into its relative names at the commas that separate them,
# and only at those: a comma inside a value is written `\,` or sits inside a
# quoted value, and neither of those separates anything. Attribute types are
# compared without case, because `cn` and `CN` are one attribute. Values are
# compared with it, because `CN=Weiyi Shi` and `CN=WEIYI SHI` are two things a
# certificate authority issued on purpose and this is not the place to decide
# they are one person. Order is kept, because the order of the relative names is
# part of the name.
function Split-DistinguishedName {
    param([string] $Name)

    $parts = @()
    $current = New-Object System.Text.StringBuilder
    $quoted = $false
    for ($i = 0; $i -lt $Name.Length; $i++) {
        $character = $Name[$i]
        # A backslash spells the next character literally. It is how a comma, a
        # plus or a quotation mark appears inside a value, and the character it
        # protects is kept while the backslash itself is not — the two sides are
        # then comparable however each of them chose to write it.
        if ($character -eq '\' -and $i + 1 -lt $Name.Length) {
            [void] $current.Append($Name[$i + 1])
            $i++
            continue
        }
        if ($character -eq '"') { $quoted = -not $quoted; continue }
        if ($character -eq ',' -and -not $quoted) {
            $parts += $current.ToString()
            [void] $current.Clear()
            continue
        }
        [void] $current.Append($character)
    }
    $parts += $current.ToString()

    return @($parts | ForEach-Object {
        $rdn = $_.Trim()
        $equals = $rdn.IndexOf('=')
        if ($equals -lt 0) { return $rdn }
        '{0}={1}' -f $rdn.Substring(0, $equals).Trim().ToUpperInvariant(), $rdn.Substring($equals + 1).Trim()
    })
}

function Test-SameDistinguishedName {
    param([string] $Left, [string] $Right)

    # Named apart from the parameters on purpose: PowerShell's variable names do
    # not distinguish case, so a `$left` here would be the `$Left` above, and the
    # second line would be splitting an array it had already replaced.
    # As arrays: a name of one relative name comes back from the function as a
    # bare string, which has no `Count` under strict mode.
    $first = @(Split-DistinguishedName -Name $Left)
    $second = @(Split-DistinguishedName -Name $Right)
    if ($first.Count -ne $second.Count) { return $false }
    for ($i = 0; $i -lt $first.Count; $i++) {
        if ($first[$i] -cne $second[$i]) { return $false }
    }
    return $true
}

# **The `Publisher` a package declares**, out of its `AppxManifest.xml`. An msix
# is a zip and `MakeAppx` leaves the manifest in it as plain XML, so it is read
# that way: one attribute of one file needs no package reader.
function Get-PackagePublisher {
    param([Parameter(Mandatory)] [string] $Msix)

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $package = [System.IO.Compression.ZipFile]::OpenRead($Msix)
    try {
        $entry = $package.GetEntry('AppxManifest.xml')
        if (-not $entry) { throw "$Msix holds no AppxManifest.xml; it is not a package" }
        $reader = New-Object System.IO.StreamReader($entry.Open())
        try { $manifestText = $reader.ReadToEnd() } finally { $reader.Dispose() }
    }
    finally { $package.Dispose() }
    return ([xml] $manifestText).Package.Identity.Publisher
}

# **The `Publisher` Folio's packages are built with**: the one this checkout's
# `packaging/msix/AppxManifest.xml` declares (the part 3 review's N20).
function Get-CheckoutPublisher {
    param([Parameter(Mandatory)] [string] $Root)

    $manifestPath = Join-Path $Root 'packaging\msix\AppxManifest.xml'
    return ([xml] (Get-Content -LiteralPath $manifestPath -Raw)).Package.Identity.Publisher
}

# **Why a release's two signed files are not one signer's**, or `$null` when
# they are (0.4.10 plan row 102, the 1b+2 review's P2-7): `folio.exe` and
# `folio.msix` carry one certificate (the same thumbprint), and its subject is
# the `Publisher` the package declares, compared as a name; and that Publisher
# is `$Ours`, the one Folio's own manifest declares (N20), so a package that
# is only consistent with itself is not taken for ours. This is the half
# of the updater's own reading of a signature (`service_signature`: our
# service's subject and its profile) that a script can ask without Rust, and
# what a statement's `authenticode true` says.
function Get-OneSignerRefusal {
    param(
        [System.Security.Cryptography.X509Certificates.X509Certificate2] $Executable,
        [System.Security.Cryptography.X509Certificates.X509Certificate2] $Package,
        [Parameter(Mandatory)] [string] $Publisher,
        [Parameter(Mandatory)] [string] $Ours
    )

    if (-not $Executable) { return 'folio.exe carries no signer certificate' }
    if (-not $Package) { return 'folio.msix carries no signer certificate' }
    if ($Executable.Thumbprint -ne $Package.Thumbprint) {
        return ("folio.exe is signed by ""$($Executable.Subject)"" ($($Executable.Thumbprint)) and " +
                "folio.msix by ""$($Package.Subject)"" ($($Package.Thumbprint)): one release is signed with one certificate")
    }
    if (-not (Test-SameDistinguishedName -Left $Publisher -Right $Executable.Subject)) {
        return ("both files are signed by ""$($Executable.Subject)"" and the package declares " +
                "Publisher=""$Publisher"": the signature is not the one the package names")
    }
    if (-not (Test-SameDistinguishedName -Left $Ours -Right $Publisher)) {
        return ("the package declares Publisher=""$Publisher"" and Folio's own " +
                "packaging/msix/AppxManifest.xml declares ""$Ours"": the package is not ours")
    }
    return $null
}
