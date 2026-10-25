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
# and, for the old-location test, every row of the tree as (Location, Tail). A tree row left over
# is MOVED when, ordinally:
#
#   (i)   the base rows left over with its identity all stand at one location L, and the tree rows
#         left over with its identity all stand at one other location L';
#   (ii)  no row of the tree has location L and its tail: the item left L (a list of literals,
#         which name no item, passes -NoItem and the pairing alone says how many left L);
#   (iii) it pairs one-to-one, in order, with those base rows; a tree row past their number is
#         MOVED AND GROWN.
#
# Anything else is ADDED (no base row has its identity, or L' is L), AMBIGUOUS ((i) fails), or
# STILL AT ITS OLD LOCATION ((ii) fails). A moved row comes back with the base row it paired with
# (From), so a check with a measurement judges it against that row: shrinking is allowed as for
# any row, growing is refused.

function Find-MovedRows {
    param(
        [object[]]$Unmatched,
        [object[]]$Leftover,
        [object[]]$Tree,
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
        "still" { return "it moved from $($Result.Was), but $($Result.Was) still holds $($Result.Tail): an item stands in one place" }
        "grown" { return "it moved from $($Result.Was) with more rows than it had there: a move does not grow" }
        default { return "" }
    }
}
