# The "moved, not added" rule of the shrink-only lists (T-MOVED-NOT-ADDED-RULE, ruling R1 (a) of
# T-SPLIT-2B), dot-sourced by check-census-unknowns.ps1, check-pointer-debt.ps1,
# check-env-doors.ps1, check-window-seam-native.ps1 and check-embedded-spaces.ps1. It is
# check-dead-code.ps1's rule (U-44) said once for row shapes whose location is not one column.
#
# Each check first matches its tree's rows against the merge base's as it always has. What is left
# is handed here as two lists of rows, each row an object with
#
#   * Identity - the row without its location and without its measurement;
#   * Location - where the item is written (a module, a file, or both), $null for a row that
#                names no location this rule can read (it is never moved);
#   * Tail     - the item the row is about, inside its location; the same for every row of one
#                identity;
#   * Row      - the check's own row, handed back.
#
# and, for the location tests, every row of the tree and every row of the base as (Location, Tail).
# A tree row left over is MOVED when, ordinally:
#
#   (i)   the base rows left over with its identity all stand at one location L, and the tree rows
#         left over with its identity all stand at one other location L';
#   (ii)  no row of the tree has location L and its tail: the old place no longer LISTS it - the
#         list, which its generator's test holds equal to the tree's reads, not the tree's items;
#         a same-named item at another place that takes over a read the old item dropped therefore
#         passes as moved, held to the base row's measurement, as check-dead-code's U-44 rule does
#         (a list of literals, which name no item, passes -NoItem and the pairing alone says how
#         many left L);
#   (ii') no row of the base has location L' and its tail: a move lands where the tail was not
#         listed (with -NoItem, not asked);
#   (iii) it pairs one-to-one, in order, with those base rows; a tree row past their number is
#         MOVED AND GROWN.
#
# Anything else is ADDED (no base row has its identity, or L' is L), AMBIGUOUS ((i) fails), or
# STILL AT ITS OLD LOCATION ((ii) fails), and ADDED too when (ii') fails. A moved row comes back with the base row it paired with
# (From), so a check with a measurement judges it against that row: shrinking is allowed as for
# any row, growing is refused.

function Find-MovedRows {
    param(
        [object[]]$Unmatched,
        [object[]]$Leftover,
        [object[]]$Tree,
        [object[]]$Base,
        [switch]$NoItem
    )
    $ordinal = [StringComparer]::Ordinal
    $from = [Collections.Generic.Dictionary[string, Collections.Generic.List[object]]]::new($ordinal)
    foreach ($row in $Leftover) {
        if ($null -eq $row.Location) { continue }
        if (-not $from.ContainsKey($row.Identity)) { $from[$row.Identity] = [Collections.Generic.List[object]]::new() }
        $from[$row.Identity].Add($row)
    }
    $held = [Collections.Generic.HashSet[string]]::new($ordinal)
    foreach ($row in $Tree) {
        if ($null -ne $row.Location) { [void]$held.Add("$($row.Location)`n$($row.Tail)") }
    }
    $heldBefore = [Collections.Generic.HashSet[string]]::new($ordinal)
    foreach ($row in $Base) {
        if ($null -ne $row.Location) { [void]$heldBefore.Add("$($row.Location)`n$($row.Tail)") }
    }
    $groups = [Collections.Generic.Dictionary[string, Collections.Generic.List[object]]]::new($ordinal)
    $order = [Collections.Generic.List[string]]::new()
    $results = [Collections.Generic.List[object]]::new()
    foreach ($row in $Unmatched) {
        if ($null -eq $row.Location) {
            $results.Add([pscustomobject]@{ Row = $row.Row; Verdict = "added"; From = $null; Was = $null; Tail = $row.Tail; Location = $null })
            continue
        }
        if (-not $groups.ContainsKey($row.Identity)) {
            $groups[$row.Identity] = [Collections.Generic.List[object]]::new()
            $order.Add($row.Identity)
        }
        $groups[$row.Identity].Add($row)
    }
    foreach ($identity in $order) {
        $rows = $groups[$identity]
        $sources = if ($from.ContainsKey($identity)) { $from[$identity] } else { $null }
        $to = [Collections.Generic.HashSet[string]]::new($ordinal)
        foreach ($row in $rows) { [void]$to.Add($row.Location) }
        $at = [Collections.Generic.HashSet[string]]::new($ordinal)
        if ($sources) { foreach ($row in $sources) { [void]$at.Add($row.Location) } }
        $verdict = $null
        if (-not $sources) {
            $verdict = "added"
        } elseif ($to.Count -ne 1 -or $at.Count -ne 1) {
            $verdict = "ambiguous"
        } elseif (@($to)[0] -ceq @($at)[0]) {
            $verdict = "added"
        } elseif (-not $NoItem -and $held.Contains("$(@($at)[0])`n$($rows[0].Tail)")) {
            $verdict = "still"
        } elseif (-not $NoItem -and $heldBefore.Contains("$(@($to)[0])`n$($rows[0].Tail)")) {
            $verdict = "added"
        }
        $was = if ($sources) { (@($at) | Sort-Object) -join ", " } else { $null }
        for ($k = 0; $k -lt $rows.Count; $k++) {
            $one = $verdict
            $source = $null
            if (-not $one) {
                if ($k -lt $sources.Count) { $one = "moved"; $source = $sources[$k].Row } else { $one = "grown" }
            }
            $results.Add([pscustomobject]@{
                    Row = $rows[$k].Row; Verdict = $one; From = $source; Was = $was
                    Tail = $rows[$k].Tail; Location = $rows[$k].Location
                })
        }
    }
    return , $results
}

# The sentence a refused verdict is said with, after the row's own spelling.
function Format-MoveRefusal([object]$Result) {
    switch ($Result.Verdict) {
        "ambiguous" { return "its identity left $($Result.Was) and arrived at more than one place, or left more than one: a move is paired one-to-one, and this one cannot be" }
        "still" { return "it moved from $($Result.Was), but $($Result.Was) still lists $($Result.Tail): an item stands in one place" }
        "grown" { return "it moved from $($Result.Was) with more rows than it had there: a move does not grow" }
        default { return "" }
    }
}

# THE RENAME ARM, stricter than a move, for the lists whose rows carry a reason of their own
# (check-env-doors.ps1, check-window-seam-native.ps1). A tree row still ADDED after the move
# pairing is RENAMED when, ordinally:
#
#   (i)   its Group (the row without its item and its measurement: what it is and where it stands)
#         is the Group of exactly one base row left over, and of no other added tree row - so the
#         base row's item is gone from the list at that same place and this is the one row that
#         arrived there;
#   (ii)  the check finds its measurement not grown against that base row;
#   (iii) its reason is dated and names the ticket: `YYYY-MM-DD: <TICKET-ID> <why>`, a real day,
#         and not the base row's reason. A move needs no reason; a rename does, because the item
#         changed.
#
# (i) failing with more than one row on a side is AMBIGUOUS and refused; (iii) failing is refused
# with the form it needs. The lists with no reason column of their own (the census unknowns, the
# pointer debt) and the one whose rows name no item (embedded spaces) have no rename arm: a
# renamed item there is an added row, as before.
function Find-RenamedRows {
    param(
        [object[]]$Added,
        [object[]]$Leftover
    )
    $ordinal = [StringComparer]::Ordinal
    $count = { param($rows) $by = [Collections.Generic.Dictionary[string, Collections.Generic.List[object]]]::new($ordinal)
        foreach ($row in $rows) {
            if (-not $by.ContainsKey($row.Group)) { $by[$row.Group] = [Collections.Generic.List[object]]::new() }
            $by[$row.Group].Add($row)
        }
        , $by }
    $bases = & $count $Leftover
    $news = & $count $Added
    $results = [Collections.Generic.List[object]]::new()
    foreach ($row in $Added) {
        $from = if ($bases.ContainsKey($row.Group)) { $bases[$row.Group] } else { $null }
        $verdict = if (-not $from) { "added" }
            elseif ($from.Count -ne 1 -or $news[$row.Group].Count -ne 1) { "ambiguous" }
            else { "renamed" }
        $results.Add([pscustomobject]@{
                Row = $row.Row; Verdict = $verdict
                From = if ($verdict -eq "renamed") { $from[0].Row } else { $null }
                Reason = $row.Reason; WasReason = if ($from) { $from[0].Reason } else { $null }
            })
    }
    return , $results
}

# $null when `reason` is a dated reason naming a ticket and new against `was`; otherwise what is
# wrong with it.
function Test-RenameReason([string]$Reason, [string]$Was) {
    if ($Reason -cnotmatch '^(?<date>\d{4}-\d{2}-\d{2}): (?<ticket>[A-Z][A-Z0-9]*(?:-[A-Z0-9]+)*)\s+\S') {
        return "a rename carries a dated reason naming its ticket ('YYYY-MM-DD: <TICKET-ID> <why>'), and this one says '$Reason'"
    }
    $day = [DateTime]::MinValue
    if (-not [DateTime]::TryParseExact($Matches['date'], 'yyyy-MM-dd', [Globalization.CultureInfo]::InvariantCulture,
            [Globalization.DateTimeStyles]::None, [ref]$day)) {
        return "a rename's reason is dated with a real day, and '$($Matches['date'])' is not one"
    }
    if ($Reason -ceq $Was) { return "a rename carries a new dated reason, and this one is the base row's" }
    return $null
}
