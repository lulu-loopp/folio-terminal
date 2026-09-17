# Cell-claim review, 2026-09-17

Target: `bbf6ce89` = `95d5bad6` + F1 `98fdd302` + F3 `1233720b` + F2 `bbf6ce89`. This reviews only those three findings, not the earlier review's recursion/crop work. Report created before investigation and updated during it.
References at target: S = `crates/bt-term/src/session.rs`; T = `vendor/alacritty_terminal/src/term/mod.rs`; A = `crates/bt-term/src/adapter.rs`; V = pinned registry `vte-0.15.0/src/ansi.rs`. In replays A/B/C/D mean OSC 133 markers terminated by BEL; ESC/CSI/CRLF name literal control bytes; spaces separating tokens are explanatory unless inside quotes.

## Verdicts

| Commit | Verdict for 0.4.2 | Reason |
| --- | --- | --- |
| F1 `98fdd302` | **hold** | Original defects fixed; cached-cluster reconstruction still launders prompt text. |
| F3 `1233720b` | **hold** | Per-screen swap works, but synchronized buffering crosses provenance phases. |
| F2 `bbf6ce89` | **merge with must-fixes** | Accept A/C/D compatibility; correct the contract and restore meaningful negative fixtures. |
| Three together | **hold** | Two independently reproduced product blockers below. |

## F1: original fixes pass; a seventeenth path survives

Both original reproductions now refuse the contaminated row live and frozen: the prompt's right-margin arrow widened by output FE0F, and output's tab retaining the prompt's combining acute on a trailing space. Independent replays checked the actual cells (`command_output_write=false`), zero Rendered inline blocks, and frozen Ineligible sites. The committed `re_cutting_and_partial_writes_carry_the_claim_of_the_text_they_keep` also passes its live/frozen output-only rendering controls.

**P1: `GraphemeState.cluster` can outlive the text it describes.** T:1852 checks cursor/wrap/screen and Unicode continuation, not whether the lead still contains that cluster. Erasure, tab and DECSC/DECRC can replace the lead and restore the expected cursor without retiring the cached cluster. T:1949 then reads the replacement cell's claim and T:1983 grants it to the old cached text reconstructed by `write_grapheme_at_cursor`.

Reproduction at 60x8: `CSI ?2027h A B C CRLF "o"*59 D A "↔" ESC 7 C CSI 2;60H CSI X TAB ESC 8 FE0F "energy $x^2$ here" CRLF D`. The arrow was the prompt's; ECH removes it and the output tab claims its empty replacement; DECRC restores the pending-wrap cursor. Result: `"↔\u{fe0f}"`, **claim=true, one Rendered inline block**, frozen site **CommandOutput**. The safety assertion failed before being changed to an observation assertion. A second probe uses prompt `⌚`, CUP 3;1 and FE0E: the shrinking/placeholder relocation branch also resurrects claimed prompt text and renders once. Adding B immediately before C still reproduces the wrong cell/frozen claim in the widening case, although the live input-region gate suppresses that variant.

Required: invalidate or correctly reanchor the grapheme cache across intervening cell mutations, and preserve the provenance of the cluster actually reconstructed. Reading an unrelated replacement cell's flag is insufficient. Add both relocation directions, live/frozen checks, and ownership controls; the current immediate-append regressions do not cover cached replay.

The sixteen-site argument is sound for immediate writes, but does not account for that retained text source. Replacement masks the template bit and replaces `c`/`extra`; wide fix-ups clear text; spacers carry no independent text; IRM/insert/delete/scroll/IL/DL/RI move entire cells; append/attach/emoji extension intersect provenance; DECALN defaults first; tab intersects when marks survive; erase/reset default flags; reflow/fork preserve cells. REP uses V:1563-1565 `Handler::input`. The adapter's generated image labels (A:892) also use ordinary parser input. No additional direct character assignment was found outside these paths; cached-cluster replay, rather than another `.c =`, is the missing path.

## F3: accept the swap design, reject its universal premise

For immediately parsed bytes, per-screen provenance is an appropriate alternative to splitting at every swap. S:3052 states primary/alternate separately; T:627 maps them using the actual active mode; T:1207 and T:2904 exchange them with the grids. The resize canonical terminal is restated too (A:1237). A marker pauses the adapter, events are applied, and both answers are restated before resume (S:2976-2993); this is an assignment by screen identity, not a second swap.

The stock pager test passes every byte split. Independent swap-back/RIS sequences, each followed by D/A/B/C in the same feed, pass every byte split too: primary output claimed, prompt unclaimed, subsequent output claimed. The named plain-canvas and crashed-canvas prompt tests pass. V:899-917 maps 1049 and falls through to Unknown for 47/1047; the terminal handles only its recognized swap mode. Repeated 1049 set/unset therefore stays idempotent, as the stock test confirms.

**P1: synchronized updates cross the phase boundaries that the adapter enforces.** A:854 calls `Processor::advance`; V:303-305 buffers bytes while DEC 2026 is active, and V:335-338 later parses all retained bytes against the current handler. The adapter has already consumed and applied intervening shell markers, so homogeneous adapter segments do not imply homogeneous terminal writes.

Reproduction: `A CSI ?2026h "energy $x^2$ here" CRLF B C CSI ?2026l D`. Text arrives during Prompt, is flushed during Output, and gets **claim=true, one Rendered inline block**, then an eligible frozen site. The safety assertion fails. Reverse it: `A B C CSI ?2026h "energy $x^2$ here" CRLF D A CSI ?2026l`; genuine output gets **claim=false**. Both wrong answers reproduce at every byte split. Neither depends on screen switching or on accepting C without B. A screen swap buffered ahead of a marker likewise cannot be assumed to have happened when the adapter assigns that marker's screen.

Required: keep text, screen transitions and phase markers ordered through deferred parsing, including explicit sync termination, timeout, and the canonical resize parser. Preserve provenance at the actual ordered write boundary; merely storing two flush-time flags cannot do it. Add sync-wrapped prompt/output controls alongside the ordinary screen-swap tests.

**P2 contract mismatch:** the alternate answer is not always false. `A B C ESC[?1049h A B C "Z"` claims Z on the canvas. After swap-back, D/A/B/C and re-entry, an unmarked `Q` is claimed too: `retire_alternate_semantic_regions` (S:4574) deletes regions but leaves the alternate Output phase/authority. These are alternate claims, not primary claims leaking through a swap. Inline eligibility still uses the separate AltScreenContent policy. Either enforce the promised no-claim invariant or document and test this retained alternate state; the existing canvas test covers only an unmarked alternate screen.

## F2: compatibility is justified; state is not authentication

S:4254-4263 returns before working/status/region/phase mutations for None/Finished. Prompt, Input and Output are accepted; repeated C closes/reopens output as before. Accepting A then C without B is reasonable under the explicit in-band trust contract: B would not authenticate the producer, and A/C/D fixtures are supported. All three shipped integrations normally emit B; fixtures establish compatibility, not evidence of a separately authenticated shell. A forged cycle and echo arriving inside C..D remain eligible by design.

The edited `tab_status` fixtures preserve their tests: A/B do not clear progress/failure, so the final C still performs the asserted clearing. The 65/130 repaint fixtures retain a partly full initial queue, a changing head, full-capacity handoffs, tail service within one sweep, and latest-head `y_{8}` service. Added A/B do not weaken those assertions; both pass. **Repair an unedited regression consequence:** S:29332-29340's two combining-mark arms start with bare C. F2 now refuses their supposed output before any accent arrives, so those negatives no longer prove the append rule. Give each a valid initial cycle and an output-only control; the old green `no_road` result cannot establish those arms' sensitivity.

Source audit of the requested ordinary shell paths (no shell was launched):

| Path | Marker order and F2 consequence |
| --- | --- |
| First prompt | PowerShell outer prompt and bash/zsh PS1 wrappers emit A/B before first submitted C; accepted. |
| Empty Enter / prompt Ctrl+C | PowerShell emits no C for empty/whitespace/comment input or cancellation; next A/B recovers. Zsh either redraws the marked prompt or runs precmd without a submitted command. Bash's next top-level prompt DEBUG trap may emit C/D even for an empty cycle; prior A/B makes it acceptable. No persistent refusal. |
| Command fails to start | PowerShell emits C for executable statements and parse errors; prompt emits D. Bash/zsh preexec precedes execution failure; a parse failure that skips preexec is followed by a fresh marked prompt. Next C remains accepted. |
| `clear` / editor clear | A clear command starts under C; clears do not erase shell phase. D/A/B opens the next cycle. An editor-only repaint leaves/reopens Input. |
| Nested shells | Integrated child A/B opens its cycle while parent is Output; child C is accepted. Parent's next marked prompt reopens after exit. Startup guards are not exported as child-shell authority. |
| ssh | Local ssh command already entered Output. An integrated remote shell's A/B/C is accepted; an unintegrated remote emits no C to refuse and its prompt/echo stays inside the outer command's claim. That is the stated authentication limit, not a new F2 refusal. |

Evidence: `folio.ps1:552-604,606-735` (ReadLine wrapper, parser, CommandStarted and outer PromptDepth); `folio.bash:187-199,265-348` (preexec/DEBUG and scalar/array prompt chains); `folio.zsh:195-239` (preexec/precmd/PS1); `betterterminal.bash` forwards to folio.bash. The listed standard orders are accepted in synthetic replays. This is source/trace validation, not interactive confirmation of arbitrary custom hooks or remote startup files. Shell hook semantics: [Bash interactive behavior](https://www.gnu.org/software/bash/manual/html_node/Interactive-Shell-Behavior), [zsh hook functions](https://zsh.sourceforge.io/Doc/Release/Functions.html).

DESIGN:2069-2073 is **not yet accurate and complete**. Replace “the shell that owns this pane said” with “the accepted in-band marker state says”; state explicitly that B/C without A also works (B creates Input), alongside A/C without B. Qualify the no-prompt/no-claim canvas statements: the committed crashed-canvas test itself has a shell prompt there. The arrival-time guarantee is false until the sync defect is fixed, and the all-text conjunction is false for stale cached replay. The unauthenticated-forgery, in-phase echo, separate alternate eligibility, unchanged copied source and non-executing math-rendering scope are otherwise appropriate. “Only a formula drawn” must be scoped to this inline gate: markers also affect working/progress/failure and command navigation metadata.

## Validation

All cargo invocations used the allowed package/target forms and `-j 4`; no heavier command or interactive application was launched, and no existing process was ended. Product code remained unchanged. Scratch tests lived only in lifecycle_matrix and were removed by restoring its original bytes (Git blob identity verified).

| Suite/filter | Stock result |
| --- | --- |
| bt-term --lib claim / output / inline / site / command / prompt / live_queue / the_canvas / no_road | 5 / 19 / 81 / 14 / 39 / 21 / 2 / 2 / 1 passed; overlapping selections |
| bt-term --test lifecycle_matrix | 42 passed before scratch and after byte-for-byte restoration |
| bt-term --test tab_status | 7 passed |
| alacritty_terminal | 146 unit + 45 reference + 1 doc passed |
| bt-transcript | 149 passed, 2 ignored; no doc tests |

Seven scratch tests completed: original live/frozen reproductions, both stale-cache relocation directions, ordinary swap/RIS cuts, alternate own/stale claims, sync phase crossings, and shell-order matrix. Safety assertions first exposed the cache, sync and canvas-contract failures; final observation assertions passed and do not establish safety. The original-tab scratch column was corrected from 17 to 18 before its passing run.

Lifecycle budgets (final restored stock run): resize 200 frames sparse **128,089,299 B / 107,372 allocations**, full **35,604,923 B / 35,073 allocations**; per-frame limits 737,280 B / 600 and 221,184 B / 192. Shrink medians **613.0 / 155.8 us**, paired ratio **3.92 <= 6**, ceilings 3/1 ms. Explicit repaint **16 allocations / 21,304 B** per cycle at 120x40 and **16 / 21,504 B** at 120x80; limits 18 / 28,672 B, same allocation count and at most 640 B growth (observed 200 B). Both budgets pass.
