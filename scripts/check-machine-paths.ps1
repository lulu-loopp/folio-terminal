# no_tracked_file_carries_a_machine_path
#
# A public repository must not be able to tell you whose laptop it was written
# on. This walks every tracked file and reads it as raw bytes, rather than asking
# `git grep`, which skips anything Git calls binary and so returns a clean bill of
# health for exactly the files most likely to be a verbatim recording. It refuses
# four things:
#
#   * an account directory named after a real person. Fixtures are allowed to
#     spell out a home directory; they are not allowed to spell out *a* home
#     directory. The placeholder list below is the whole permitted vocabulary.
#   * an e-mail address outside the reserved example domains (RFC 2606/6761).
#   * the checkout path this product was developed in.
#   * the author's account name, in any case — except on a line that is a
#     copyright notice naming them as the holder, which is the one place the
#     name is the attribution rather than a leak of it.
#
# Excluded, deliberately, and each for its own reason:
#   * `vendor/`, `licenses/` and `THIRD-PARTY-NOTICES.md` — third-party code and
#     licence texts carry their own authors' names and addresses. Those addresses
#     are the attribution; removing them is the breach, not the fix.
#   * `docs/` — the internal planning record. Gate 7 decides what of it ships.
#   * `corpus/` — terminal recordings, cleaned on their own line, with their own
#     gate.
#   * this file — it has to spell the forbidden strings in order to forbid them,
#     and a rule that cannot state its own subject is not a rule.
# Binary payloads are skipped by extension: a 10 MB font is a lottery of byte
# sequences that look like anything you care to search for.

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot
Push-Location $repo
try {
    $tracked = @(& git ls-files)
    if ($LASTEXITCODE -ne 0) { throw "git ls-files failed" }

    $skipPaths = @("corpus/", "docs/", "vendor/", "licenses/", "THIRD-PARTY-NOTICES.md",
                   "scripts/check-machine-paths.ps1")
    $skipExt = @(".ttf", ".otf", ".pfb", ".icc", ".nupkg", ".zip", ".png", ".jpg",
                 ".jpeg", ".ico", ".pdf", ".dll", ".exe", ".recording", ".btcr",
                 ".woff", ".woff2", ".mp4", ".webm")

    # The only account names a fixture may spell out.
    $placeholders = @("alice", "bob", "user", "users", "me", "dev", "you", "x", "a",
                      "test", "someone", "somebody", "example", "name", "public",
                      "default", "username", "folio", "...")

    # RFC 2606 and RFC 6761 reserved names: addresses here can never reach anyone.
    $exampleDomains = "^(example\.(com|org|net)|.*\.(example|invalid|test|localhost))$"
    # `hero@2x.png` is a retina asset, not a mailbox. A last label that is a file
    # extension is not a top-level domain.
    $notDomains = @("png", "jpg", "jpeg", "gif", "svg", "webp", "css", "js", "html",
                    "htm", "md", "rs", "txt", "json", "toml", "yml", "yaml", "exe",
                    "dll", "ps1", "sh", "bat", "ttf", "otf", "woff", "woff2", "log")

    # The `/home/` and `/Users/` spellings are case-sensitive on purpose: `/Home/End`
    # is a pair of key names, not a directory.
    $home_re = [regex]"(?:[A-Za-z]:[\\/]{1,2}[Uu]sers[\\/]{1,2}|/home/|/Users/)(%[A-Za-z_]+%|[A-Za-z0-9_.$-]{1,32})"
    $mail_re = [regex]"[A-Za-z0-9._%+-]+@([A-Za-z0-9.-]+\.([A-Za-z]{2,24}))"
    $checkout_re = [regex]"(?i)Developer[\\/]{1,2}BetterTerminal"
    $author_re = [regex]"(?i)weiyi|umich\.edu"
    # The one sentence the name above may appear in — see the loop below.
    $copyright_re = [regex]"(?i)copyright\b.*\bweiyi shi\b"

    $problems = New-Object System.Collections.Generic.List[string]

    foreach ($rel in $tracked) {
        if ($skipPaths | Where-Object { $rel.StartsWith($_) }) { continue }
        if ($skipExt -contains [IO.Path]::GetExtension($rel).ToLowerInvariant()) { continue }

        $full = Join-Path $repo $rel
        if (-not (Test-Path -LiteralPath $full)) { continue }
        $bytes = [IO.File]::ReadAllBytes($full)
        $text = [Text.Encoding]::Latin1.GetString($bytes)

        # **A copyright notice names its holder, and that is not leakage.** The
        # author's name is forbidden here as a trace of whose machine this was
        # written on, and required in `LICENSE-MIT`, in the appendix of
        # `LICENSE-APACHE` and in the binary's own `LegalCopyright` as the
        # attribution the two licences are granted by. The difference is the
        # sentence the name stands in, so that is what is asked: the name is
        # allowed on a line that is a copyright notice for it, and refused on
        # every other line in the tree. The address is never allowed — a mailbox
        # is not a holder, and no licence asks for one.
        foreach ($line in ($text -split "`n")) {
            $notice = $copyright_re.IsMatch($line)
            foreach ($m in $author_re.Matches($line)) {
                if ($notice -and $m.Value -notmatch "(?i)umich") { continue }
                $problems.Add("$rel names the author: $($m.Value)")
            }
        }
        foreach ($m in $checkout_re.Matches($text)) {
            $problems.Add("$rel names the development checkout: $($m.Value)")
        }
        foreach ($m in $home_re.Matches($text)) {
            $who = $m.Groups[1].Value
            $ok = ($placeholders -contains $who.ToLowerInvariant()) -or ($who -match "^%[A-Za-z_]+%$")
            if (-not $ok) {
                $problems.Add("$rel names a real account directory: $($m.Value) (use one of: $($placeholders -join ', '))")
            }
        }
        foreach ($m in $mail_re.Matches($text)) {
            $domain = $m.Groups[1].Value.ToLowerInvariant()
            if ($notDomains -contains $m.Groups[2].Value.ToLowerInvariant()) { continue }
            if ($domain -notmatch $exampleDomains) {
                $problems.Add("$rel carries a deliverable e-mail address: $($m.Value)")
            }
        }
    }

    if ($problems.Count -gt 0) {
        $unique = $problems | Select-Object -Unique
        throw ("tracked files carry machine-specific data:" +
            [Environment]::NewLine + ($unique -join [Environment]::NewLine))
    }

    Write-Host "$($tracked.Count) tracked files scanned; none names a machine, an account, or an address"
} finally {
    Pop-Location
}
