# XTVERSION review — 2026-09-17

Target: d8a59187, following c7b0914d on e0f682b8; both git show diffs reviewed.
Paths: adapter.rs and session.rs are in crates/bt-term/src; vte is registry 0.15.0.
Verdict: merge with must-fixes (sync debt must leave at its commit).

Verified: reply is ESC P > | Folio(0.4.2) ESC \, 18 bytes: adapter.rs:47,
crates/bt-term/Cargo.toml:3 inherits Cargo.toml:36 (the product version).
No-query state audit: adapter.rs:1564-1638 reads/writes parser_boundary,
parser_sequence_open, parser_tail, parser_tail_open_start, parser_dcs_active,
parser_sync_active; writes announced_focus and cursor_row_positioned_explicitly.
No Term/grid/processor read occurs there. Processor deadline is read only after
advance (1643-1653); focus consumer is set_keyboard_focus (706), cursor consumer
is session.rs:3871 via adapter.rs:1417, after parsing; no earlier-feed exposure.
No-query events retain transcript -> Title/ResetTitle -> Bell -> GridWrites
(adapter.rs:813-816,977-993). Marker early-return (905) occurs after that drain;
unconsumed actions are not boundary-scanned. No early exit can lose counted bells.
Cost: one processor advance per Bytes action without queries, plus the existing
boundary pass (two parsers, not literally one pass); no new buffer allocation,
but existing parser_tail.push can allocate. Zero extra CPU cost is not proven.
Sync caveat: registry vte-0.15.0/src/ansi.rs:375 checks buffer + entire slice size;
segmentation can change overflow timing. Its reverse ESU/BSU scan (405-415)
can commit an earlier block while retaining a later block's deadline.

P2: reply can outlive its committed block (adapter.rs:969-970,1695). Repro:
BSU XTVERSION ESU BSU in one feed returns no reply; the first block committed,
but the next block's deadline suppresses its debt. BSU XTVERSION ESU DA1 gives
[DA1, XTVERSION], so the limitation also includes replies OUTSIDE the block.
Must flush debt at its block's commit before later blocks/replies; add both tests.
Payload requirement mismatch (adapter.rs:560): scratch OSC/DCS/APC + ESC[>q
and CSI200~ ESC[>q CSI201~ each answer once. ESC ends string payload in vte
(lib.rs:323-328,419-422,444-446); paste brackets describe terminal INPUT,
not an output quoting mode. This is parser semantics, not substring matching.
Adapter suite: 68/68 pass. Initial lifecycle: 42/42 plus one scratch probe pass.
ESU, explicit deadline finish, overflow, marker commit, RIS then ESU: exactly once;
BSU/query/ESU in one feed, last-byte query and adjacent queries pass too.
Budgets measured: sparse 128,089,299 B / 107,372 allocations; full 35,604,923 B /
35,073 allocations. TUI cycles: 16 allocations at both heights (21,304/21,504 B).
1 MiB flood: 262,144 replies, 4,718,592 payload bytes; 28.10-39.13 ms live, 41.13-46.74 ms
with canonical fork; 14,156,232 charged B / 262,182 allocation calls each.
Queue storage is linear (approximately 12.5 MiB incl. 32-byte slots, excluding
allocator overhead); no cap. Bytes counter charges realloc growth, not peak RSS.
No quadratic path found: one segment/query, amortized queue growth, sync scans
new bytes plus seven overlap bytes. Canonical adds three discard locks/segment
(adapter.rs:308-319,977-982); query reply adds one lock (1698), plus empty final
advance when a query ends the slice (969). Flood timings are two noisy local samples.
Event qualification: BEL XTVERSION OSC-title now yields Bell before Title;
c7 yielded Title before Bell (adapter.rs:963,984). Query-free order is preserved.
Missing committed regression: BEL + OSC-title + printable in one Bytes action,
asserting Title/Bell/GridWrites, plus bells on both sides of a paused shell marker.
Term/listener callbacks access only their own state/queues (adapter.rs:263-305);
resize seed reads the tail only in begin_resize_transaction (1158). Cursor memory
is captured after feed settlement (session.rs:3068-3088); resize resets it (1106).
Canonical gets identical slices including the empty final slice (977-982), with
its output discarded; UTF-8 partial state persists in vte/lib.rs:68-69,113-114.
c7 comparison executed in disposable worktree with the five ordering tests copied
unchanged except public imports/helper name: BEFORE-DA1 (2962), interleaving
(2987), every split (3005), one-write probe (3045) FAIL with DA1/DSR first.
AFTER-DA1 (2975) PASSES: all five should not fail; it guards the reverse case.
Old lifecycle base also passes (45 passed, four expected failures incl. scratch).
The sync-debt defect reproduces on c7 too; d8 leaves this new feature's defect
unfixed. All four heap budgets and both TUI allocation counts match c7 exactly.
Final target lifecycle run: 42 base + three scratch tests pass (45/45). Scratch
confirms UTF-8 at every split with canonical reflow, inert payload text ignored,
and marker-paused bells delivered once. Query-event order is Bell then Title.
Overflow probe ends cleanly; no differing final state found, despite call-sensitive
vte overflow logic. Keep an explicit near-limit segmentation regression.
Only authorized cargo commands ran; no application launched or process stopped.
