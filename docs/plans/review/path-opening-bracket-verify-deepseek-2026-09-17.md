STATUS: COMPLETE

## Verification of d661b34c (path before opening bracket)

Read-only, at d661b34c (branch `fix/path-before-opening-bracket`, one commit on main ecf98443).
Subject: the printed-path recogniser, `crates/bt-transcript/src/paths.rs`. The fix promotes the
opening half of every bracket pair from a seam to a terminator — new `is_opening_delimiter`
(:1471-1499), folded into `is_path_terminator_char` (:1515-1520) — and deletes `is_ascii_opening_bracket`
and its two call sites as dead. I read the full diff plus every live call site of
`is_path_terminator_char` and `token_end`.

### (1) Every caller, and whether a new terminator cuts a path main read correctly

`is_path_terminator_char` has two call sites: `token_end` (:1534-1539) and the stop-run trim at
:2677 (`is_sentence_stop(character) && !is_path_terminator_char(character)`). `token_end` has three
call sites, none a test helper: the absolute scan (:827), the relative scan (:940, cached via
`token_end_seen` :909/:941), and the multi-row rejoin walk (:2862, `raw_end = token_end(row.text,
start)`).

`token_end` stops the token *before* the opening bracket. That bracket is itself not a path-tail
character, so `candidate_start_boundary` (:1783-1789) — called by `absolute_candidate_opens_at`
(:1303-1313) and `foreign_candidate_opens_at` (:1328-1333) — still **opens** a path immediately
*after* it: ending the token before a bracket and opening the one after it are the same boundary.
Therefore the only names a new terminator can cut are names whose *closing* half did not already
cut them — an opening bracket with no matching closer on the line. Shape by shape:

- `photo(1).png` — CONFIRMED already cut on main. `)` is in `is_closing_delimiter` (:1408-1435) and
  was before this commit, so main's token ended at `)`. The new `(` only moves the cut earlier.
  Either way the balanced name is never read whole (and, as a single-segment bare name with no
  separator, it was never a relative reference at all). No regression.
- `a[1].txt`, `{}`-containing names, names with `<` — same class. The closer (`]`, `}`, `>`) was
  already a terminator on main, so a balanced bracket name was already cut at its closing half; the
  new opener moves the cut to the opening half. JSON `{"a":1}` and redirection `cmd < file.txt` are
  unaffected: the opener ends nothing before it, and the redirect target still opens after it via
  `candidate_start_boundary`. No regression.
- MSVC `main.cpp(12,34): error` — main offered the whole form `main.cpp(12,34` then the seam
  `main.cpp` (second frame); the `(12,34)` was never part of a filename. Now `(` terminates and
  `main.cpp` links on the first frame. Legitimate improvement; this is the "one because MSVC now
  links on the first frame" test change.
- rustc `--> src/main.rs:12:34` — no bracket; `token_end` runs to the end and
  `split_printed_location` (:1559-1586, unchanged) yields line 12 col 34. Unaffected.
- `grep -n` `docs/a.md:12: content` — no bracket; token ends at the whitespace after `12`; `:12`
  splits off. Unaffected.
- markdown `[text](docs/a.md)` and `(docs/a.md)` alone — the `(` now ends the token before it and
  still opens the one after it; `docs/a.md` opens after `(` and terminates at `)`. The new lib.rs
  test pins it. Unaffected.
- `docs/a.md【说明】` — `【` (U+3010, :1487) is now a terminator, so the token stops at `【`. On main
  the token ran past `【` to the closer `】`, `release_prose_tail` (:1609-1618) could not peel the
  trailing CJK (it is `is_path_tail_char`), and `prose_seam_ends` found no seam (`【` is not ASCII, so
  `is_seam_separator` :1638-1640 does not fire) — main linked **nothing**. Same bug as the headline
  `（commit` case; fixed, not a regression.

The only new cut is the stated cost: a name whose opening bracket is *unmatched* on the line
(`D:\x\a(1` with no `)`) now reads `D:\x\a` unquoted. Quoting still reads it whole — a quoted token
is a declaration of extent and no terminator applies inside it (:833-839). This holds for all four
ASCII brackets and their full-width counterparts; it is the symmetric cost of "no pair, no name",
not a hidden extra. The stop-run trim at :2677 changes consistently with the same fact: an opening
bracket is no longer counted in the "stops behind" run, exactly as a closing bracket already was not
(the doc at :2668-2672 states the closing-half rule), so a form ending at an opening bracket is no
longer pressed down at the line edge. That is the promotion's own logic read at one call site, not a
second defect.

### (2) The removed seam logic

`token_may_carry_a_seam` (:1687-1689) was a cheap pre-filter; it still returns true on a trailing
`is_sentence_stop` or any non-ASCII byte. The deleted clause `|| is_ascii_opening_bracket(...)` is
now dead — no token passed to it contains an opening bracket, because `token_end` stops first.
`prose_seam_ends` (:1720-1746) still carries the two surviving seam clauses: the trailing
sentence-stop run (`offset >= stops_from`, :1734) and the ASCII-separator-followed-by-non-ASCII
transition (`is_seam_separator(character) && !next.is_ascii()`, :1734-1739). Nothing besides
brackets relied on the removed clause: the `docs/a.md,这里是操作顺序` seam is still produced by
`prose_seam_ends` and still admitted by the non-ASCII byte check in `token_may_carry_a_seam`
(:1688). The rule moved to `token_end`; it did not disappear (the doc at :1704-1708 says exactly
this).

### (3) `:line[:col]` after the change

`split_printed_location` is unchanged (:1559-1586). Real `:line[:col]` shapes contain no bracket, so
no change reaches them, and no reference changes which file it names. The one behavioural difference
is the MSVC `(12,34)` shape, which was never parsed as a location (the `(`/`)` prevented `:...`
from being a trailing run) — it was a whole-token reading main offered and the disk denied. Now it
is simply not offered.

### (4) The three updated tests

1. MSVC rows 7/8 (`main.cpp(12,34): error`): `with_position_syntax` → `named`, and `links_in` moves
   from "wait for a second frame" to "link on the first frame." Legitimate — the `(12,34)`
   whole-token reading is gone, so there is no longer a second reading to wait for; `main.cpp` is
   the file that exists.
2. `a_bracket_glued_to_a_cjk_word_seams_like_any_other_separator` → `a_mark_glued_to_a_cjk_word_cuts_the_name_it_follows`:
   `D:\x\a.md(说明)` and `D:\x\a.md{批}` now read `D:\x\a.md` only; main also offered the whole
   token `D:\x\a.md(说明`, which the disk denied. Legitimate — the seam reading was always the
   wanted one.
3. `an_opening_bracket_is_a_seam_whatever_follows_it` → `an_opening_bracket_ends_a_token_whatever_follows_it`:
   the drive-rooted `D:\dist\x.exe(0.1.0` and `D:\x\a(1` now read up to the bracket, and a NEW
   quoted assertion `"见 \"D:\x\a(1\""` → `D:\x\a(1` documents that quoting still admits the
   unmatched-bracket name. Legitimate and honest — it pins the stated cost and its escape hatch.

No test was weakened to hide a regression; each asserts the new, narrower reading and (in case 3)
that quoting preserves the old one.

### (5) The 2026-08-28 ruling reversal

The removed doc pinned: an ASCII opening bracket is a **seam**, never a terminator, because "a
bracket carries its own evidence and needs no witness behind it." Its practical effect was to keep
an *unmatched* opening bracket readable whole (`D:\x\a(1` offered whole plus the seam `D:\x\a`),
since a *matched* one (`D:\x\a(1).txt`) was already cut by its `)` — the ruling protected only the
unbalanced spelling.

Reversing it is right. The asymmetry it protected (unbalanced names readable, balanced names not)
was a bug, not a feature: the closing half already proved "a name that reaches a bracket has
ended," and reading only one half of the pair was the rule stated at one end (:1512-1514). The
reversal makes the behaviour uniform — every bracket-pair name is read to its opening half, and a
name that genuinely contains a bracket is reached by quoting — at the cost of the unbalanced name,
which is rare and fully covered by quoting (`"D:\x\a(1"`). The reported case is fixed:
`docs/…html（commit e0bfcfe）。` now reads `docs/…html` because `（` (U+FF08, :1477) is a terminator.

### Verdict

**CORRECT.** Every caller is accounted for; no name that main read whole loses its reading except
the stated unmatched-bracket cost, which quoting preserves; the ASCII-separator→non-ASCII seam is
intact; `:line[:col]` is untouched; all three test updates are legitimate consequences; and the
2026-08-28 ruling reversal removes a genuine asymmetry for a narrow, quoted-recoverable cost. No gap
found.

STATUS: COMPLETE
