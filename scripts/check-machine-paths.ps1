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
#   * the checkout path this product was developed in, and the owner's
#     synced-documents folder that his other projects live under.
#   * the author's account name, in any case — except on a line that names them
#     as the holder of something: a copyright notice, or, since 0.2.0, the
#     sentence that says who signed the release. Both are the attribution rather
#     than a leak of it, and a reader who is asked to check a publisher's name in
#     a SmartScreen dialog has to be told it somewhere. Every other line in the
#     tree still refuses the name, and the e-mail address is refused everywhere.
#
# Excluded, deliberately, and each for its own reason:
#   * `vendor/`, `licenses/` and `THIRD-PARTY-NOTICES.md` — third-party code and
#     licence texts carry their own authors' names and addresses. Those addresses
#     are the attribution; removing them is the breach, not the fix.
#   * the dated record inside `docs/` — the folders and files listed in
#     `$docsNotScanned` below, each with its reason (and, inside `docs/plans/`,
#     the date it was last changed). They are verbatim history
#     (transcripts, evidence logs, quoted prompts, handoffs), and a
#     record rewritten to pass a scan is no longer the record. One more file,
#     listed in `$authorNameAllowed`, is scanned in full except for the
#     author's name, which it spells as the publisher. Everything else in
#     `docs/` is scanned: the rules, the guards and the front page's prose are
#     living documents, and since 2026-10-31 (0.4.9 row 129) a new file there is
#     covered by default instead of by being remembered on an include list.
#   * nothing in the append-only log, `docs/DESIGN.md`, except the hits that were
#     already there (2026-11-01, 0.4.9 row 156). Its old entries cannot be
#     rewritten, and every row appends a new one, so the file is scanned like any
#     other against `$keyedAllowList` below: a list of the hits it held on that
#     day, each keyed by its content rather than its line. A hit not on the list
#     is refused with its line number; a row that matches no hit is refused as
#     stale, so the list cannot rot. `-PrintKeys` prints the row a hit would take.
#   * `tests/corpus/` — terminal recordings, cleaned on their own line, with
#     their own gate.
#   * this file — it has to spell the forbidden strings in order to forbid them,
#     and a rule that cannot state its own subject is not a rule.
# Binary payloads are skipped by extension: a 10 MB font is a lottery of byte
# sequences that look like anything you care to search for.

param(
    # Print, for every hit in a file on `$keyedAllowList`, the key its row takes
    # and the hit masked, then exit without judging anything.
    [switch]$PrintKeys
)

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot
Push-Location $repo
try {
    # **Every tracked name as itself** (2026-11-01, row 129's review N4). Git
    # C-quotes a name that is not ASCII unless `core.quotepath` is off, and the
    # quoted spelling names no file: `Test-Path` below then said no and the file
    # was skipped without a word. The names arrive as UTF-8, so they are read as
    # UTF-8 whatever the console's own code page is.
    $consoleEncoding = [Console]::OutputEncoding
    try {
        [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
        $tracked = @(& git -c core.quotepath=off ls-files)
        if ($LASTEXITCODE -ne 0) { throw "git ls-files failed" }
    } finally {
        [Console]::OutputEncoding = $consoleEncoding
    }

    $skipPaths = @("tests/corpus/", "vendor/", "licenses/", "THIRD-PARTY-NOTICES.md",
                   "scripts/check-machine-paths.ps1")

    # **The dated record inside `docs/`** (2026-10-31, 0.4.9 row 129). Until then
    # the whole of `docs/` was skipped and four front-page files were scanned by
    # name, so a machine path landed in `docs/ARCHITECTURE-GUARDS.md` unseen (row
    # 113's review, N3). Each entry is a prefix and says why it may hold what the
    # rest of the tree may not.
    #
    # **`docs/plans/` is scanned, all but its frozen record** (2026-11-01, 0.4.9
    # row 158; until then the whole folder sat here). It is the one dated folder
    # still written to: the generated lists, the release notes and their template,
    # the runbooks and the design notes code rows keep editing. Those are scanned
    # like any other file, so a generator that writes a machine path is caught at
    # its output and fixed at the generator. What stays out is the record that has
    # not changed since the date its entry gives and carries the hits it was
    # written with: a folder when the whole folder is a closed campaign, a file by
    # name when its folder is still living. A closed folder could still take a
    # new file unseen; a new campaign gets a new folder, and that is scanned.
    #
    # An entry ending in `/` is a folder; any other is one file, matched whole.
    # An entry that names no tracked file is refused as stale, so the table
    # cannot keep excusing a path that has moved. (An entry whose files have
    # lost their hits is not caught: that would mean scanning what is excluded.
    # Only a path with a hit belongs here, and a folder only when nothing in it
    # is read by the product or by another check.)
    $docsNotScanned = [ordered]@{
        # Closed campaigns, whole: reviews and their answers, evidence runs (gate
        # logs and `.jsonl` recorded verbatim), agent transcripts.
        "docs/plans/review/"              = "frozen since 2026-09-24: review reports and answers quoting the reviewed tree"
        "docs/plans/web-preview/"         = "frozen since 2026-09-07: web-preview reviews and verbatim gate evidence"
        "docs/plans/horizontal-scroll/"   = "frozen since 2026-09-07: horizontal-scroll plan and its reviews"
        "docs/plans/underline-hardening/" = "frozen since 2026-08-23: underline scenarios as run"
        "docs/plans/agents/"              = "frozen since 2026-09-20: agent survey"
        # The Mac port: its plan is living, its transcripts and probes are not.
        "docs/plans/port/m3-5/"           = "frozen since 2026-09-12: agent transcript, verbatim"
        "docs/plans/port/m4-7/"           = "frozen since 2026-09-12: agent transcript, verbatim"
        "docs/plans/port/m4-9/"           = "frozen since 2026-09-12: agent transcript, verbatim"
        "docs/plans/port/probe-x3-keyboard-ime-2026-09-12.md" = "frozen since 2026-09-12: probe record, as run"
        "docs/plans/port/probe-x4-app-delegate-2026-09-12.md" = "frozen since 2026-09-12: probe record, as run"
        # Attention: the evidence and the plan are record; the hook fixtures
        # beside them are the product's `include_str!` expectations, and scanned.
        "docs/plans/attention/evidence-2026-08-25.md"            = "frozen since 2026-08-25: verbatim evidence"
        "docs/plans/attention/evidence-cli-survey-2026-08-25.md" = "frozen since 2026-08-26: verbatim CLI survey"
        "docs/plans/attention/evidence-turn-end-words-2026-09-01.md" = "frozen since 2026-09-07: verbatim evidence"
        "docs/plans/attention/plan.md"                           = "frozen since 2026-09-15: the attention plan"
        # UI style: the audit is record; the copy guide beside it is a living rulebook.
        "docs/plans/ui-style/audit-codex-2026-08-25.md"          = "frozen since 2026-08-25: review answer, verbatim"
        # Dated plans in folders that are still written to.
        "docs/plans/bt-app-split-inventory-2026-09-15.md" = "frozen since 2026-09-15: dated split inventory"
        "docs/plans/paste-paths-design.md"                = "frozen since 2026-09-15: paste-paths design and its worked paths"
        "docs/plans/shell-matrix-2026-09-07.md"           = "frozen since 2026-09-08: shell matrix as measured"
        "docs/plans/design/agent-workbench-0.5-2026-09-20.md"           = "frozen since 2026-09-27: dated design note"
        "docs/plans/design/spare-web-controller-review-codex-2026-09-25.md" = "frozen since 2026-09-24: review answer, verbatim"
        "docs/plans/design/present-never-blocks-input-2026-09-20.md"    = "frozen since 2026-09-20: dated design note"
        "docs/plans/design/render-handoff-2026-09-16.md"                = "frozen since 2026-09-15: dated design note"
        "docs/plans/release/readiness-audit-2026-08-27.md"       = "frozen since 2026-09-15: dated readiness audit"
        "docs/plans/release/readiness-review-codex-2026-08-27.md" = "frozen since 2026-09-07: review answer, verbatim"
        "docs/plans/release/readiness-gaps-2026-08-27.md"        = "frozen since 2026-08-27: dated readiness gaps"
        "docs/plans/release/license-review-codex-2026-08-27.md"  = "frozen since 2026-08-27: review answer, verbatim"
        "docs/plans/release/plan.md"                             = "frozen since 2026-09-07: the 0.1 release plan"
        "docs/plans/release/winget.md"                           = "frozen since 2026-09-20: winget submission record"
        # Review reports quote the paths of the tree they reviewed.
        "docs/reviews/"  = "dated review reports quoting the reviewed tree"
        # The prompts handed to agents, kept as sent, trailers included.
        "docs/prompts/"  = "dated agent prompts kept as sent"
        # Spike write-ups and the probe scripts they ran, as run.
        "docs/spikes/"   = "dated spike records and their probes, as run"
        # Session handoffs, each a snapshot of one day.
        "docs/handoff/"  = "dated session handoffs"
    }
    # **Files scanned against a list of the hits they already hold** (2026-11-01,
    # 0.4.9 row 156; until then DESIGN sat in the table above). DESIGN is
    # append-only (`docs/ARCHITECTURE.md`, "append-only and it is evidence"): an
    # old entry cannot be rewritten, but a new one is written by nearly every
    # row, and a machine path in it would land unseen. The list file's own
    # header gives its grammar; it is scanned like any other file, which it can
    # pass because a row carries a hash and a masked shape, never the hit.
    $keyedAllowList = [ordered]@{
        "docs/DESIGN.md" = "scripts/check-machine-paths-design-allowed.txt"
    }
    # **Files scanned in full except for the author's name** (2026-11-01, row
    # 129's review F3). The home-directory, checkout and e-mail checks still run
    # on them, and so does the `umich.edu` half of the author check: a mailbox is
    # never attribution.
    $authorNameAllowed = [ordered]@{
        # The winget manifest folder, `manifests/w/<publisher-id>/Folio`, and the
        # winget-pkgs branch are named after the publisher ID, which is
        # attribution (as `$publisher_re` below), not a machine. The runbook's
        # pasted commands are exactly where a home or checkout path could land,
        # so only the name is excused.
        "docs/RELEASING.md" = "winget publisher identity"
    }
    $skipExt = @(".ttf", ".otf", ".pfb", ".icc", ".nupkg", ".zip", ".png", ".jpg",
                 ".jpeg", ".ico", ".pdf", ".dll", ".exe", ".recording", ".btcr",
                 ".woff", ".woff2", ".mp4", ".webm", ".packdump")

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
    # The checkout this product was developed in, before and after the 2026-10-23
    # disk move: `Developer\BetterTerminal`, and the three folders under
    # `Developer\Folio\` this project's work has run from since — the checkout,
    # its worktrees and the trace tools (`Folio\BetterTerminal`, `Folio\wt`,
    # `Folio\trace`). That folder holds other projects as well; they are not
    # named here, because a list of them would drift with every new one.
    # **The pre-move layout too** (2026-11-01, 0.4.9 row 157): before the move
    # the worktrees, the trace tools, the launchers and the build checkouts sat
    # directly under `Developer\` (`Developer\wt`, `Developer\trace`,
    # `Developer\launchers`, `Developer\build`), and the last two still do on
    # this machine. Each is named whole, so a neighbour such as `wtx` passes.
    # **And the layout before the product's rename** (2026-11-01, 0.4.9 row
    # 159): the checkout sat in `Developer\folio-terminal`, named after the
    # repository, and its worktrees in `Developer\bt-wt`. The repository keeps
    # that name, so the bare word is its address in every release URL and is
    # not refused; only the folder under `Developer\` is. Its worktree folder is
    # refused with its hyphenated siblings (`bt-wt-build`, …, all real); the one
    # the append-only log spells passes only by its keyed row in the DESIGN list.
    # Never a bare drive-letter `\Developer\`: a product fixture stands in a
    # drive-letter `\Developer\<folder>\…` that no machine of this project's
    # has used (`notes`, `Ledger`, `elsewhere`, `lunar-workshop`, …), and such
    # a path names nobody's machine. A fixture that replaces a recorded folder
    # takes a neutral name of the same length, so the recording's widths hold.
    # Nor `Developer\Folio\` alone: a fixture standing in a folder named after
    # the product (`…\Developer\folio\notes.md`, `…\Developer\Folio\docs`) is a
    # placeholder like `folio` in the account list below, not this checkout.
    $checkout_re = [regex]"(?i)Developer[\\/]{1,2}(?:BetterTerminal|Folio[\\/]{1,2}(?:BetterTerminal|trace|wt)\b|(?:wt|trace|launchers|build)\b|folio-terminal\b|bt-wt\b)"
    # **The owner's synced-documents folder** (2026-11-01, 0.4.9 row 159; row
    # 157's review F3). Projects outside this one live under it, and recorded
    # captions and prompts from them reached product fixtures, where neither
    # pattern above could see them: there is no `Users\` in front of it and no
    # listed folder behind `Developer\`. It is refused under the checkout check,
    # in its drive-letter, `file:///` and WSL spellings. A redirected Documents
    # folder on another drive (`X:\Documents\PowerShell\…`) is a Windows layout
    # the product has to handle, not this machine's, and stays allowed.
    $synced_re = [regex]"(?i)(?:[A-Za-z]:|/mnt/[a-z])[\\/]{1,2}Documents[\\/]{1,2}SyncFolder\b"
    $author_re = [regex]"(?i)weiyi|umich\.edu"
    # The three sentences the name above may appear in — see the loop below.
    $copyright_re = [regex]"(?i)copyright\b.*\bweiyi shi\b"
    # **A Chinese word, written the way this scan will meet it.** Every file is
    # read as Latin1 so that a recording of arbitrary bytes cannot throw on a
    # sequence that is not valid UTF-8; a Chinese word therefore arrives as the
    # Latin1 view of its UTF-8 bytes, and is searched for in exactly that form
    # rather than pasted here as the mojibake nobody could check.
    $asScanned = {
        param([string] $word)
        [regex]::Escape([Text.Encoding]::Latin1.GetString([Text.Encoding]::UTF8.GetBytes($word)))
    }
    $signWords = (@("签名", "发布者", "署名") | ForEach-Object { & $asScanned $_ }) -join "|"
    $signWords = "\b(?:sign|signed|signer|signing|signature|publisher)\b|$signWords"
    # The name and a word about signing, in either order and on one line: "signed
    # by Weiyi Shi", and "names Weiyi Shi as the publisher".
    $signed_re = [regex]("(?i)(?:\bweiyi shi\b.*(?:$signWords)|(?:$signWords).*\bweiyi shi\b)")
    # **A signed package's identity, which the sentence above does not reach**
    # (§7.4a): the `Publisher` of `packaging/msix/AppxManifest.xml`, the
    # publisher-scoped package name, and the two relative distinguished names the
    # holder appears as inside a certificate subject. Each is missed by
    # `$signed_re` for its own reason and none of them is an oversight there:
    # `PublisherDisplayName` gives `publisher` no word boundary to end on,
    # `WeiyiShi.Folio` has no space in it for `weiyi shi` to match, and a test
    # that names a *different* holder beside this one to prove the comparison
    # refuses it carries no word about signing at all.
    # **The winget manifest folder too** (2026-11-01, 0.4.9 row 158):
    # `manifests/w/WeiyiShi/Folio` is the same publisher-scoped identity spelt
    # as winget-pkgs files it, and a runbook that pastes the folder (the clean-VM
    # one copies it into the guest) says who published the package, not whose
    # machine it was built on.
    $publisher_re = [regex]"(?i)\b(?:CN|O)=Weiyi Shi\b|PublisherDisplayName|\bWeiyiShi\.Folio|\bmanifests[\\/]{1,2}w[\\/]{1,2}WeiyiShi[\\/]{1,2}Folio\b"

    $problems = New-Object System.Collections.Generic.List[string]

    # **The keyed lists** (row 156). A row is
    #     <key> <check> <date> <reason class>: <shape>
    # and its key is the first 16 hex digits of the SHA-256 of the check's name,
    # the matched text and the whole line, joined by NUL and taken as the bytes
    # the scan read (the line without its trailing whitespace). The line number
    # is no part of it: an entry inserted above a listed hit leaves its row
    # valid, and any edit to the hit's own line makes the row stale and the hit
    # new. Two identical hits on one line take two rows.
    $hitKey = {
        param([string] $check, [string] $value, [string] $line)
        $bytes = [Text.Encoding]::Latin1.GetBytes("$check`0$value`0$($line.TrimEnd())")
        $hash = [Security.Cryptography.SHA256]::HashData($bytes)
        ([Convert]::ToHexString($hash).Substring(0, 16)).ToLowerInvariant()
    }
    $row_re = [regex]"^(?<key>[0-9a-f]{16}) (?<check>author|checkout|home|mail) (?<date>\d{4}-\d{2}-\d{2}) (?<reason>old-checkout example|synced-folder example|author|runner account|false positive): (?<shape>\S.*)$"
    $keyedRows = @{}
    if (-not $PrintKeys) {
        foreach ($listed in $keyedAllowList.Keys) {
            $listFile = $keyedAllowList[$listed]
            $rows = [Collections.Generic.Dictionary[string, Collections.Generic.List[object]]]::new()
            $number = 0
            foreach ($row in [IO.File]::ReadAllLines((Join-Path $repo $listFile), [Text.UTF8Encoding]::new($false))) {
                $number++
                if ($row -match "^\s*(#|$)") { continue }
                $m = $row_re.Match($row)
                if (-not $m.Success) {
                    $problems.Add("${listFile}:$number is not a row: <key> <check> <date> <reason class>: <shape>")
                    continue
                }
                $key = "$($m.Groups['check'].Value) $($m.Groups['key'].Value)"
                if (-not $rows.ContainsKey($key)) { $rows[$key] = [Collections.Generic.List[object]]::new() }
                $rows[$key].Add([pscustomobject]@{ Number = $number; Text = $row })
            }
            $keyedRows[$listed] = $rows
        }
    }

    # Every hit in a text — a whole file, or one line of a keyed one: its check,
    # its matched text and its problem. The author check asks line by line,
    # because what excuses the name is the sentence it stands in; no pattern of
    # the other three can match across a line break, so they ask the text whole.
    $hitsIn = {
        param([string] $text, [bool] $nameAllowed)
        # Only a text that names the author is split into lines, and only a line
        # that names them is asked what sentence it is (row 158: the scan of
        # `docs/plans/` must not cost the licensing job seconds).
        $lines = if ($author_re.IsMatch($text)) { $text -split "`n" } else { @() }
        foreach ($line in $lines) {
            if (-not $author_re.IsMatch($line)) { continue }
            $notice = $nameAllowed -or $copyright_re.IsMatch($line) -or
                      $signed_re.IsMatch($line) -or $publisher_re.IsMatch($line)
            foreach ($m in $author_re.Matches($line)) {
                if ($notice -and $m.Value -notmatch "(?i)umich") { continue }
                [pscustomobject]@{ Check = "author"; Value = $m.Value; Problem = "names the author: $($m.Value)" }
            }
        }
        foreach ($m in $checkout_re.Matches($text)) {
            [pscustomobject]@{ Check = "checkout"; Value = $m.Value; Problem = "names the development checkout: $($m.Value)" }
        }
        foreach ($m in $synced_re.Matches($text)) {
            [pscustomobject]@{ Check = "checkout"; Value = $m.Value; Problem = "names the owner's synced-documents folder: $($m.Value)" }
        }
        foreach ($m in $home_re.Matches($text)) {
            $who = $m.Groups[1].Value
            $ok = ($placeholders -contains $who.ToLowerInvariant()) -or ($who -match "^%[A-Za-z_]+%$")
            if (-not $ok) {
                [pscustomobject]@{ Check = "home"; Value = $m.Value; Problem = "names a real account directory: $($m.Value) (use one of: $($placeholders -join ', '))" }
            }
        }
        foreach ($m in $mail_re.Matches($text)) {
            $domain = $m.Groups[1].Value.ToLowerInvariant()
            if ($notDomains -contains $m.Groups[2].Value.ToLowerInvariant()) { continue }
            if ($domain -notmatch $exampleDomains) {
                [pscustomobject]@{ Check = "mail"; Value = $m.Value; Problem = "carries a deliverable e-mail address: $($m.Value)" }
            }
        }
    }
    # A hit as `-PrintKeys` shows it: the account and the drive masked, the
    # checkout, the address and the author check's hit (the name, or the
    # mailbox domain its second half matches) not spelt at all.
    $masked = {
        param([string] $check, [string] $value)
        switch ($check) {
            "author" { "<author>" }
            "checkout" { "<checkout>" }
            "mail" { "<address>" }
            default {
                $v = [regex]::Replace($value, "(?i)weiyi[A-Za-z0-9_.-]*", "<author>")
                $v = [regex]::Replace($v, "[A-Za-z]:([\\/])", 'X:$1')
                [regex]::Replace($v, "((?:[Uu]sers|home)[\\/]{1,2})[A-Za-z0-9_.$-]+", '$1<acct>')
            }
        }
    }

    # Every entry of `$docsNotScanned` that excused at least one tracked file.
    $excusing = @{}
    $excludedFolders = @($docsNotScanned.Keys | Where-Object { $_.EndsWith("/") })
    $excludedFiles = [Collections.Generic.HashSet[string]]::new(
        [string[]]@($docsNotScanned.Keys | Where-Object { -not $_.EndsWith("/") }), [StringComparer]::Ordinal)
    foreach ($rel in $tracked) {
        if ($skipPaths | Where-Object { $rel.StartsWith($_) }) { continue }
        if ($excludedFiles.Contains($rel)) { $excusing[$rel] = $true; continue }
        $folder = $null
        if ($rel.StartsWith("docs/")) {
            foreach ($prefix in $excludedFolders) {
                if ($rel.StartsWith($prefix, [StringComparison]::Ordinal)) { $folder = $prefix; break }
            }
        }
        if ($folder) { $excusing[$folder] = $true; continue }
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
        # allowed on a line that is a copyright notice for it, or on one that
        # says who signed the release — a reader told to check the publisher a
        # SmartScreen dialog names has to be able to read that name here — and
        # refused on every other line in the tree. The address is never allowed —
        # a mailbox is not a holder, and no licence asks for one.
        #
        # **A package's publisher is a third sentence of the same kind** (§7.4a).
        # Windows compares the `Publisher` in `packaging/msix/AppxManifest.xml`
        # against the subject of the certificate that signed the package and
        # refuses a mismatch, so that string is not this project's to choose or to
        # abbreviate: it is the certificate authority's rendering of the holder,
        # character for character, and `bt_platform::msix` has to carry the same
        # one in order to check it. Like a copyright notice and like the line that
        # names who signed a release, it says who published this and nothing
        # whatever about the machine it was built on.
        $nameAllowed = $authorNameAllowed.Contains($rel)
        $keyed = $keyedAllowList.Contains($rel)
        if ($PrintKeys -and -not $keyed) { continue }
        if (-not $keyed) {
            foreach ($hit in & $hitsIn $text $nameAllowed) { $problems.Add("$rel $($hit.Problem)") }
            continue
        }
        $number = 0
        foreach ($line in ($text -split "`n")) {
            $number++
            foreach ($hit in & $hitsIn $line $nameAllowed) {
                $check, $value, $problem = $hit.Check, $hit.Value, $hit.Problem
                $key = & $hitKey $check $value $line
                if ($PrintKeys) {
                    Write-Output "$rel`:$number`t$key $check <date> <reason class>: $(& $masked $check $value)"
                    continue
                }
                $rows = $keyedRows[$rel]
                if ($rows.ContainsKey("$check $key") -and $rows["$check $key"].Count -gt 0) {
                    $rows["$check $key"].RemoveAt(0)
                    continue
                }
                $problems.Add("${rel}:$number $problem (not on $($keyedAllowList[$rel]), which lists only the hits the file held on 2026-11-01)")
            }
        }
    }

    if ($PrintKeys) { return }

    # An exclusion that excused nothing names a path that has moved or gone.
    foreach ($entry in $docsNotScanned.Keys) {
        if (-not $excusing.ContainsKey($entry)) {
            $problems.Add("`$docsNotScanned entry $entry is stale - no tracked file is under it any more (remove the entry, or point it at where the record moved)")
        }
    }

    # A row left over matched no hit: the line it was taken from has changed.
    foreach ($listed in $keyedRows.Keys) {
        foreach ($left in $keyedRows[$listed].Values) {
            foreach ($row in $left) {
                $problems.Add("$($keyedAllowList[$listed]):$($row.Number) is stale - no line of $listed carries that hit any more: $($row.Text) (the file is append-only, so an old entry was edited: restore it, or remove the row if the edit was ruled)")
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
