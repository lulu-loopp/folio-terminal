# Adversarial code review, 2026-09-21

Reviewed: everything merged to `main` since v0.3.0-preview (tag `9acd482f`) — 929 commits, 187
files under `crates/`, about +142,000 / −64,000 lines, read at `09ffa417` and, for the last two
knives, at `cb58fbb0`. The whole macOS port, the path paste and drop road, large documents, output
coalescing, the metronome and the repaint settle had each been read only in the branch that
introduced it; none of them had been read whole. Cut into six areas by subject rather than by diff:

A. **The macOS platform layer.** The `macos_*` and `portable_impl` arms of `bt-platform`, the
   hotkey, the menu bar and the single-instance handover: unsafe and Objective-C lifetimes, thread
   ownership, the operating system's own permission prompts, and the navigation gate on the
   embedded web view.
B. **Foreign content entering the command line.** Copying and dropping a file to get its path,
   a clipboard picture becoming a temporary file, focus following a drop: quoting for a shell is
   the injection surface, and the temporary file's life and cleanup is the other half.
C. **The line a program's output travels.** `bt-term`'s session and adapter, `bt-detect`,
   coalescing, the version query, the repaint settle, the live plane: whether malformed or hostile
   output can wedge, grow without bound, put something on the glass the source never printed, or
   reorder the replies.
D. **Frames and the device.** Pacing, the journey ledger, the hang watchdog, device loss and
   rebuild in `bt-render`, the adapter preference, the font library: state kept on both sides of an
   asynchronous boundary, and whether a rebuild invalidates everything it should.
E. **Everything Folio writes outside its own application folder.** Settings, profiles, the agent
   configuration files it must leave as it found them, the first-run card, the Explorer entry and
   the new cleanup command, the launch pipe, the update check.
F. **Documents and editing.** The in-place Markdown editing changed since 0.3.0, the large-document
   work, the live preview and its viewport, the formula tools, the PowerShell syntax set: data
   safety, the 64 KiB and 8 MiB boundaries, undo.

How: one reviewer read each area against a written brief that fixed its scope by path and subject,
without a build and without running the program. Every finding was then re-derived from its trigger
by a second reviewer who owns the verdict and the severity, who walks each early return rather than
trusting the reviewer's anchor, and who opens every cited line before writing it down. A finding
counted only after that second derivation. What survived was then triaged by three thresholds:
introduced or made worse since the baseline; a hard requirement — data loss, a crash, a frozen
window, a picture that does not match its source, a privacy leak, execution nobody asked for — that
an ordinary gesture reaches; and whether it breaks the scenario the product is for. Everything else
went on the ledger. No build, no test and no compiler was run at any point, no window was launched,
and nothing in the repository was modified by a reviewer or a verifier.

Two departures from that method are worth stating, because the rows have to be read with them in
mind. **Area B and area C were each read twice**, by two different reviewers against two different
briefs — the first reviewer returned a single finding in area C, and the second brief was written
to answer that by naming the depth expected of each claim. The second pass's rows carry the ids
`B2-n` and `C2-n`. **Area E is the one place where the reviewer and the verifier were the same
reader**: the first reviewer's run was cut off with nothing delivered, the area was re-read from a
written sweep of scenario classes, and the verification was a separate read of the result rather
than a second person's. Its three rows are the weakest-method rows in this table.

## The numbers

| Area | Findings | Survived | Fixed in 0.4.3 | On the ledger | Struck or dropped by ruling |
|---|---|---|---|---|---|
| A the macOS platform layer | 4 | 4 | 4 (one of them by half) | 1 (the other half) | 0 |
| B foreign content, first pass | 2 | 2 | 0 | 0 | 2 |
| B2 foreign content, second pass | 4 | 3 | 1 | 1 | 1 needs a ruling |
| C the line output travels, first pass | 1 | 1 | 1 | 0 | 0 |
| C2 the line output travels, second pass | 4 | 4 | 3 | 0 | 1 |
| D2 frames and the device | 3 (+3 questions) | 2 (+3) | 0 | 5 | 0 |
| E writes outside the application folder | 4 | 4 | 3 | 1 | 0 |
| F documents and editing | 3 | 3 | 1 | 2 | 0 |
| **Total** | **25 (+3)** | **23 (+3)** | **13** | **10** | **4** |

Twenty-three of the twenty-five findings survived the second derivation; the three D2 questions are
carried in the table as well, because each of them ended with a disposition. Two findings were
refuted outright, and four more had their trigger or their consequence refuted while the mechanism
held — they stay in the table with the narrowed verdict, because the mechanism is still worth
writing down. That is the audit's own false-positive record and it is in its own section below.

Three severities are not this audit's to set at all. On 2026-09-21 the owner narrowed the hard
requirement that four of these rows rest on, and dropped or struck four dispositions with it. Those
rulings are stated in full after the table, and the table's disposition column already obeys them.

## The findings

Merge commits name the merge, not the branch tip. The entry titles are `docs/DESIGN.md`'s own,
dated 2026-09-20 or 2026-09-21. Tests are named, never located: `main.rs` is about to be split, so
a line number here would be a lie within the week.

| id | Severity | What it is | Verdict | Disposition | Pinned by |
|---|---|---|---|---|---|
| A-1 | high | A content rule list compiled for a policy the seat has since left is attached to the live page anyway, because the completion block asks only whether the page's mint moved and not whether the wanted rules did — so for one compile round trip a local document is judged by the browsing policy, which blocks its own pictures and permits the network. | **true**, two sub-claims corrected: the parked address is *not* refused closed — it loads, so the seat also navigates away from the document one gesture late; and reachability is narrower than reported, since both gestures must fall inside one compile. | fixed in 0.4.3, `98d04d7c`, entry *A page that may not fetch may not open a socket; a stale gate is never hung; only the writer binds* | `a_rule_list_compiled_for_a_superseded_rule_is_never_attached`, `a_seat_goes_to_the_last_address_it_was_given` |
| A-2 | high | The compiled rule list — the only thing standing between a previewed local document and a socket on macOS — names `http` and `https` and never `ws`/`wss`, although the per-request answer refuses them. A local page's script opens a WebSocket and streams the document out. | **true and stronger than reported.** The reviewer left open whether the engine routes a WebSocket handshake through the rule lists; it does, and has since a version below the product's floor. The reviewer also missed that the per-request answer refuses `ws` for *every* seat, so the naive repair would have broken the dev-server preview. | fixed in 0.4.3 on macOS, `98d04d7c`, same entry. The Windows half cannot be fixed — that engine raises no event for WebSocket traffic — and is stated instead. Reclassified as hygiene rather than a boundary by the 2026-09-21 ruling below. | `a_seat_that_may_not_fetch_may_not_open_a_socket` |
| A-3 | high | Both the launch endpoint and the attention endpoint are bound without asking whether this process holds the data directory's claim, and the comment above one of them asserts that it does. A second launch that lost the claim binds them first; the writer then loses both for the life of the process, and every tab opened in the loser is absent from the session. | **true**, severity re-cut: data loss yes, privacy leak no — the loser's panes are spawned with no attention endpoint at all, so no text goes anywhere. | fixed in 0.4.3, `98d04d7c`, same entry | `only_the_writer_binds_the_data_directorys_endpoints`, `a_second_claimant_is_not_the_writer_of_that_directory` |
| A-4 | latent | The delegate selectors are injected one at a time with no rollback, and `applicationShouldTerminate:` is second in the list, so a refusal further down leaves a live method whose answer can never be given: ⌘Q then hangs the application for good. | **partly true.** The mechanism is sound and traced. The live path is **refuted**: the check walks the superclass chain, and the pinned window library's own delegate implements neither colliding selector. It becomes real on a version bump, and the window widens every time a selector is appended. | the free half shipped in 0.4.3 — the terminate selector moved to the end of the list, `98d04d7c`. The two-pass rewrite (refuse before any mutation) is on the 0.4.4 ledger. | `the_terminate_selector_is_the_last_one_injected` |
| B-1 | medium | A path is quoted for the shell the pane *started*, which is captured once at spawn; in a nested shell typed into that same pane the quoting is inert and a filename carrying `&` or `$( )` becomes syntax. After the reader presses Enter, a command runs. | **partly true.** Two of the three chains hold as claimed, with one byte string corrected — a single backtick leaves the line unterminated where the reviewer said it executed. The third chain is **refuted as written** and holds only for a crafted name. Nothing at all executes before Enter on any chain, and the whole line is on the glass. | **struck by the 2026-09-21 ruling.** The proposed remedy — highlight the dangerous characters when the foreground program is not the row's own shell — is off the ledger. | — |
| B-2 | medium | The clipboard-picture store is created with `create_dir_all` and its files with default permissions: no symlink check, no owner check, no mode, at either level, on either platform — although the design requires all of it, on both, before every operation. | **partly true.** Confirmed as to code and **wider** than reported: the requirement is unconditional and unimplemented on both platforms, the Windows access-list clause included. **Refuted** as to consequence: the Windows and macOS temporary directories are per-user, so no shipped configuration reaches the leak; only Linux, which is not a shipped platform, or a redirected temporary directory does. | **struck by the 2026-09-21 ruling.** The owner check is off the ledger; if Linux is ever shipped it belongs to that port. | — |
| B2-1 | medium | A pasted newline arrives as Enter in any child that never enabled bracketed paste, so a clipboard whose content the reader never saw runs as commands with no confirmation anywhere. | **true as mechanism, partly true as a finding.** The dominant case — pasting a script into a shell — is what every terminal does and what the reader asked for. The defect is the residue: a trailing newline after a line meant to be read, text hidden on the page it was copied from, and a paste into a full-screen program in raw mode that did not ask for bracketing. Predates 0.3.0. | **ruled 2026-09-21, 0.4.4.** Measured on the owner's machine the same evening: neither `cmd.exe` nor Windows PowerShell enables bracketed paste, so three pasted lines run the first two at once — the ordinary case, not a residue. The ruling: the gesture was *paste*, not *run*; where a program brackets pastes the Enter that follows is the consent, and these two shells skip it. So (B) a multi-line paste into a program that has not enabled bracketed paste asks first, shows what is about to be pasted, and can be told not to ask again; and (C) at a PowerShell prompt Folio first tries to land the whole block on the input line unexecuted — by sending PSReadLine's add-a-line key in place of each newline, or by teaching the PSReadLine build Folio ships to accept bracketed paste — which is to be proven by a short experiment before the ticket is written. A terminal cannot switch bracketed paste on for a program that does not know the markers. | — |
| B2-2 | high | The macOS clipboard arm sniffs a TIFF's eight-byte header and never its dimensions, then hands the whole thing to the system image decoder: a 200-byte file declaring 65535 × 65535 asks for about 17 GB. The sibling PNG and bitmap arms both refuse the identical claim. | **true.** Whether the framework returns nothing or raises, the process ends — the binding is built without the catch-all — and the intermediate sizes a real scan reaches succeed and swap the machine to a stop. New in 0.4.1. | fixed in 0.4.3, `98d04d7c`, entry *A clipboard picture shows its shape before it is decoded* | `a_tiff_that_claims_a_huge_shape_is_refused_before_it_is_decoded`, `every_encoding_is_judged_by_the_one_ceiling`, `a_real_tiff_past_the_ceiling_is_refused_before_it_is_drawn`, `a_shape_appkit_does_not_know_is_zero_and_not_four_billion` |
| B2-3 | medium | Clipboard text and the clipboard's file list are read and copied on the window thread with no size and no count ceiling — four full-length copies of the text before it reaches the ring, and one round trip plus one allocation per file name. The picture rung is the only one with a ceiling. | **true**, with the reviewer's illustration corrected: an ordinary spreadsheet-sized copy is a fraction of a second, not seconds. The honest statement is that the ceiling is absent, so the cost is whatever the source chose, and a local process can choose gigabytes. Text predates 0.3.0; the file list is 0.4.1's. | 0.4.4 ledger — one rule for both rungs: a rung refuses what it will not carry into memory, before it carries it. | — |
| C-1 | high | When a synchronized update ends on its 150 ms deadline, the arm that ends it clears the entire replay tail — including the half-written sequence the boundary parser is still inside. A resize then arms the canonical fork from an empty tail, so the fork stands at ground while the displayed parser stands mid-sequence, and the rest of that sequence is printed to the grid as text and can scroll into the record. | **true.** Two citation slips, neither touching the trace, and a second entrance the reviewer missed: a shell-integration marker commits the block through the same function and clears the same tail. Predates 0.3.0. The two conditions correlate rather than compound — a window drag is exactly what starves the drain turns that would have committed the block. | fixed in 0.4.3, `4cac17b6`, entry *Ending a synchronized update keeps the sequence it interrupted* | `a_timed_out_synchronized_update_keeps_the_sequence_it_interrupted`, `a_marker_that_commits_a_synchronized_update_keeps_the_sequence_it_interrupted`, `a_timed_out_synchronized_update_keeps_a_dcs_introducer`, `a_timed_out_synchronized_update_with_nothing_open_gives_the_whole_tail_up`, `a_sequence_kept_across_a_timeout_stops_at_the_tail_ceiling` |
| C2-1 | high | The boundary parser opens a synchronized update only on the exact single-parameter spelling, while the vendored parser opens on any parameter equal to 2026 and closes only on the exact one. Three legal spellings therefore open a block Folio does not know is open: the replay tail is given up byte by byte, the resize fork is armed from nothing, and everything the program printed inside the block leaves both the glass and the record, unparsed. | **true**, and re-verified as still live on `main` after C-1's fix had merged: that fix repairs a different shape and leaves the parameter rule untouched, so the outcome was byte-identical. Predates 0.3.0. | fixed in 0.4.3, `cb58fbb0`, entry *The boundary parser opens a synchronized update where the terminal does* | `the_boundary_parser_opens_a_synchronized_update_where_the_vendored_parser_does`, `the_boundary_parser_ends_a_synchronized_update_where_the_vendored_parser_does`, `a_resize_inside_a_synchronized_update_keeps_the_block_whatever_spelling_wrote_it`, `a_block_the_replay_tail_could_not_carry_is_written_to_the_grid_before_the_fork` |
| C2-2 | high | Resting the pointer on a reference in terminal output resolves it three times per pointer-move event, and each resolution makes two blocking filesystem calls on the window thread over the whole path — with no drive-type test, no cache and no worker. A mapped drive whose share has stopped answering, or a junction to one anywhere in the middle of the path, freezes the window for as long as the redirector takes. | **true and understated.** Six blocking calls per motion event, not one; a further call on every modifier change and one per frame while a card is up; no cache exists anywhere; and the target of an OSC 8 link is taken verbatim, so the text is the attacker's. Every line number in the finding was wrong. Predates 0.3.0 — it is a latent defect of the 2026-09-08 audit's own fix, which added a call to a window-thread predicate while documenting it as free. | fixed in 0.4.3, `bc05ced5`, entries *The window thread never asks the disk about a path a program printed* and *A press asks what a pointer move asks, and a local path is never called another machine*, corrected the next day by *Correction: the component-by-component link walk is withdrawn* and *Correction: the worker produces the door's input with the door's own function*. The volume question the fix first added was withdrawn by the owner; what shipped is the move off the thread and nothing else. | `no_hover_door_makes_a_filesystem_call_of_its_own`, `the_routing_table_asks_a_filesystem_nothing`, `a_name_nobody_has_answered_for_is_not_a_link_and_touches_nothing`, `one_subject_is_resolved_once_however_many_readers_ask`, `the_pointer_memo_is_emptied_at_the_top_of_every_pointer_move`, `a_link_under_a_resting_pointer_is_asked_about_by_the_press` |
| C2-3 | medium-high | A forged `OSC 133;B` opens an input region from anywhere, and the next resize then writes the PowerShell repaint chord `ESC[24;8~` onto the stdin of whatever program holds the pane — a remote shell, a text editor, an agent. The product's own comment records what those bytes do to a line editor that does not know them: they land in the reader's command line. | **true and understated.** The forged mark is accepted from inside a running command as well as at a prompt, so it works *during* a real command; and no shell integration need be installed — only a pane whose program is named `pwsh` or `powershell`, which is the Windows default. Predates 0.3.0. | fixed in 0.4.3, `cb58fbb0`, entry *The prompt chord goes only to a prompt the shell opened in order*. The stronger gate the closure review proposed — a nonce minted in the script and carried in every mark — is **struck by the 2026-09-21 ruling**. | `a_prompt_mark_forged_inside_a_running_command_is_typed_at_by_nothing`, `a_prompt_with_a_command_running_inside_it_is_not_one_this_window_types_at` |
| C2-4 | high | On Windows, Ctrl+click on a path printed in terminal output hands interpreter scripts — `.py`, `.pyw`, `.ahk`, `.au3`, `.pl`, `.rb`, `.lua`, `.sh`, `.tcl`, `.wsb`, `.inf`, `.application` — to the system's open verb, which runs them. An OSC 8 link's label can read `notes.txt` while its target is the script, and a checked-out repository satisfies the existence test by itself. | **true on the mechanism, partly true as written.** The finding's own headline list is wrong in both directions — `.js` and `.vbs` *are* refused — and four of its five triggers are wrong, while the one real trigger is worse than it said. Predates 0.3.0. | **dropped by the owner's ruling of 2026-09-21.** Ctrl+click on a printed path keeps opening it; the modifier is the reader's consent. The reveal-only remedy never shipped and the program list is byte-for-byte what it was. | pinned the other way: `ctrl_on_a_printed_file_opens_it_whatever_the_extension`, `a_plain_click_on_a_printed_path_still_previews_it` |
| D2-1a | latent | A surface acquisition that keeps failing spins an unbounded, unpunished re-present loop on the window thread: the cheap gate is invalidated by the first absorbed failure, so every retry is a full compose plus a surface reconfigure, and the arm that decides whether to ask again says yes with no counter and no wait — beside a sibling arm that has exactly that counter and says in its own comment why. Nothing is written to the log, ever. | **partly true.** The mechanics are exact, line for line. The trigger is **refuted against the pinned graphics dependency**: neither shipping backend produces the states it needs — one never returns them at all, the other has the relevant timeout turned off — and the events the finding names as triggers reach either the already-fixed invisible arm or the fatal arm, which is a different road. | 0.4.4 ledger, as a precondition rather than a repair: the counter and a log line must be added **before** any upgrade of the graphics dependency, since the hole opens the day a backend starts returning those states. | — |
| D2-2 | high | The error exit of the whole event loop, and the panic hook beside it, both discard dirty Markdown editing buffers that an ordinary window close would have asked about. The session snapshot holds paths and names and says so in its own words; there is no document autosave, the undo log is not persisted, and the recovery copies that exist are for configuration files. | **true, and worse on two counts.** The panic road loses strictly more — it does not close windows at all, so the shells are not retired and no snapshot is taken for that instant. And the set of errors that reaches this exit is the *whole event loop's*, from twelve doors including every keyboard, resize, drop and mouse handler, not the render path's alone. | 0.4.4 ledger, at the top of it. The rule the code is missing is that no road out of this process may discard bytes the reader typed; today that rule is a property of one door rather than of the program. | — |
| D2-Q1 | — | Whether a frame-shape refusal — which stops the process — can be tripped from program output or a document. | **not found.** Four attempts, each ruled out by a named invariant; the live-formula family is closed by construction rather than by luck. One future door: the retired inline-image band pipeline overwrites a band's clip height, and its own comment calls the reversal "one character". | 0.4.4 ledger as a one-line note against that switch, so whoever flips it back reads this first. | — |
| D2-Q2 | — | Indeterminate progress and a standing attention ticket both hold the loop redrawing at the display's rate for ever — including after the program that set the progress has exited, and including while the window is minimised or invisible. Each tick is a full chrome rebuild, a full compose and a present, because the gate's cheap path cannot fire once any pixel has moved. | **partly true.** Two claims hold, one is wrong and one over-general: a *static determinate* reading does not hold the loop awake — its tween clears — and the pacing is exactly one tick per display interval, never faster. But the per-tick cost really is a full compose, there is no visibility short-circuit anywhere on the path, and progress really does survive the program's exit. Not new: this week's change was the look, not the scheduling. | **owner ruling, 2026-09-21: stop when not visible; keep the refresh rate when visible.** Scheduled for 0.4.5. Whether a progress report should outlive the program that made it is still a separate question nobody has ruled on. | — |
| D2-Q3 | low | The shell-integration scripts are installed with a truncating write, so a process killed mid-write leaves a partial `folio.ps1` that every PowerShell on the machine then dot-sources. | **true**, and the consequence is smaller than it sounds: a parse error at the start of every shell until the next Folio launch repairs it, with nothing executed, or — on an exact statement boundary — a prefix that runs and marks itself installed. Self-healing either way. | 0.4.4 ledger: one line each at two sites, swapping in the atomic writer this repository already ships and uses for its own documents. | — |
| E-1 | high | The PowerShell module install writes nine files into a version directory it did not write, because the only thing standing in front of the write is a version number probed once at process start; and the removal that follows deletes the whole version leaf, taking the gallery's own record, help and catalogue with it. | **true and sharper than reported.** One of the two reaching states needs no race at all — it is returned before the probe is consulted, so even a fresh probe does not save the directory — and a probe that cannot read anything lands in the other. Predates 0.3.0. | fixed in 0.4.3, `fbc7c2cc`, entry *Folio installs its module only where the place is empty or its own*; the file-by-file removal that spares a stranger's sidecars shipped with it. | `a_module_this_build_did_not_write_survives_an_install`, `an_absent_an_empty_and_folios_own_leaf_all_take_the_install`, `a_leaf_of_ours_with_somebody_elses_sidecars_updates_and_is_removed_file_by_file`, `uninstall_psreadline_leaves_every_file_folio_never_wrote` |
| E-2 | medium | All three agent-configuration installers write back a document they read before a delay long enough to contain two synchronizing writes, with nothing comparing the target to what they read — so a concurrent edit by the agent itself is silently reverted, and the second press of a day skips the dated copy, so neither the lost version nor an account of it survives. | **true** at the commit the area was read at. Predates 0.3.0. | fixed in 0.4.3 by a branch already in flight, `53a215fd`: the write moved onto one shared object that re-reads the resolved target immediately before writing and refuses on a change, and the structural test now demands that shared writer so a fourth installer cannot re-open it. The remaining window is one temporary-file synchronize wide and is narrower than the one the profile writer has accepted since its own ticket. | `a_landing_is_whole_or_it_is_nothing_at_all`, which pins that the shared writer is the only way out. **The arm that refuses on a changed file is pinned by nothing** — it is the one seam in this row's fix with no permanent test. |
| E-3a | medium | Every copy Folio keeps of somebody else's configuration file is created with this process's default permissions rather than the source's, so a file restricted to its owner — and two of the providers document writing a bearer token into exactly this file — gets a world-readable dated copy, which the purge deliberately keeps. Three sites share the shape. | **true.** Privacy leak on the POSIX platforms; on Windows the copy inherits the directory's list, so the damage there is the loss of an explicit restriction rather than a new exposure. Predates 0.3.0. | 0.4.4 ledger, as one structural fix over the three copy sites rather than three patches. The owner ruled on 2026-09-20 that the purge keeps the copy and names its path in the output. | — |
| E-3b | medium | The replacement itself — not the copy — is a fresh file object, so it loses the target's access list, mode and attributes. | **true** at the commit the area was read at. | fixed in 0.4.3 by the same branch, `53a215fd`: the preserving writer when a file was there, the plain one only when there was none. | the writer's own round trip, `preserving_replace_keeps_mode_and_ownership` and `a_preserving_replacement_carries_mode_and_extended_attributes`; the choice at this call site is pinned by nothing. | 
| F-1 | medium-high | A preview save writes a temporary sibling and renames it over the target, so the file the reader saves is a new object: alternate data streams (the mark a download carries among them), explicit access entries, owner, attributes, Unix mode, and every extended attribute — the quarantine flag, tags, comments — are gone, and a hard link is silently broken. The preserving writer exists in the tree and is used for one file. | **partly true.** The losses are real, unruled and correctly traced, but creation time **survives** on a local NTFS volume through the filesystem's own name tunnelling, so the finding is wrong about the common Windows case; and the fix it proposed repairs Windows only — the preserving writer's Unix arm carries owner and mode and not extended attributes — while turning today's silent link break into a save that fails. Predates 0.3.0; what is new is the asymmetry. | fixed in 0.4.3, `4bc8030a`, entries *A save replaces the content and keeps what the file carried* and *Content is guaranteed, what the file carried is best effort, and nothing is left behind*, with the extended attributes added to the Unix arm. The hard-linked file keeps today's behaviour by ruling — the save succeeds and the other name keeps the old bytes — with the notice and a true synchronise on the 0.4.4 ledger. | `a_save_is_written_by_the_writer_that_keeps_what_the_file_carried`, `a_save_keeps_the_stream_the_ace_and_the_attributes_the_file_carried`, `a_hard_linked_document_still_saves_and_the_other_name_keeps_its_bytes`, `a_preserving_replacement_carries_mode_and_extended_attributes`, `a_read_only_or_locked_file_is_refused_and_loses_nothing` |
| F-2 | medium | The composite bitmap for inline formulas on one line has no byte ceiling: its width is the distance between the first and last formula on the line, bounded only by the 8 KiB source cap, so one printed line with two valid formulas at opposite ends forces one allocation as wide as the line, retained with no budget. | **partly true.** The missing ceiling is exactly as traced; the arithmetic is about twice too high — tens of megabytes at the defaults, about 180 MB at the largest font on the most scaled display, not the figure reported — and "one line aborts the process" overstates a mechanism that needs repetition or real memory pressure. The harder consequence the finding missed: a composite over the texture budget is tiled, uploaded and then refused **every frame it is visible**, which is the frozen-window standard rather than the crash one. New after 0.3.0. | 0.4.4 ledger: the three-line ceiling, and the refusal latch as its own ticket, since a composite just under the budget evicts the whole texture cache every frame regardless. | — |
| F-3 | low | A GIF's declared loop count is never read, so a picture the file says should play once plays for ever — and keeps the decoding worker and the repaint awake while it does. Every browser stops it. | **partly true: confirmed as a gap, refuted as a hard-requirement defect.** Every frame drawn is a frame the file contains, composed by the specification's own algorithm and pinned frame for frame against an independent decoder; a loop count is playback policy, not content. What is lost is fidelity to the author and a small standing cost. Predates 0.3.0. | 0.4.4 ledger or later, with one dated line settling what the count means — the specification's own wording is ambiguous about whether it counts plays or repeats, and that is the difference between one play and two on the commonest value. | — |

Two more merges in this range came out of the closure reviews of these fixes rather than out of a
knife: `78cd4508`, *A removal that found nothing says nothing in the window*, and `1bc7cad5`,
*Folio's own writers wait their turn for the marks record* — both defects introduced by, or exposed
beside, the E-1 repair and caught before the release. They are recorded here because they are part
of the cost of the audit, not part of its yield.

## What did not survive

Two findings were refuted outright:

- **B2-4 — an external drop is aimed from a cursor position sampled at dispatch rather than at the
  release.** False. The claim was read off the *name* of a function rather than followed into it:
  on both backends the window library calls the application handler synchronously inside the
  platform's delivery of the drop, buffering only when the handler is already on the stack, which
  it is not during that delivery. The comment in the product that asserts this is accurate. What
  the finding leaves behind is worth having anyway — the synchronicity is an invariant the window
  library owns and Folio depends on silently, and nothing names it, so a future dependency bump or
  a modal that pumps messages would make the finding true without anything going red.
- **D2-1b — a surface configuration failure stops the whole process for one wedged window.** False
  at the premise: the function it rests on returns nothing at all, not a result, so the arm has no
  trigger. The road it describes is real and reaches the same exit from other errors, and those
  facts are kept under D2-2.

Four more had the trigger or the consequence refuted while the mechanism held, which is why they
are in the table with a narrowed verdict rather than here: **A-4** and **D2-1a** are both
unreachable with the dependency the lock file pins, and both become live on an upgrade; **B-2** is
real in code and unreachable in every configuration this product ships; **F-3** is a real gap that
does not reach any hard requirement.

Sub-claims that fell inside surviving findings, recorded because each of them would have been
believed: B-1's third injection chain does not execute as written; C2-4's own headline list of
dangerous extensions contains two that are refused, and four of its five triggers are wrong;
F-1's creation-time loss does not happen on a local NTFS volume; F-2's arithmetic is twice the
truth; B2-3's illustration of an ordinary copy freezing for seconds is not true of an ordinary
copy; D2-Q2's claim that any progress report holds the loop awake is not true of a static
determinate one; and, in the enumeration of what Folio writes outside its own folder, the row
saying macOS gets no preference file of its own is false — the uninstaller's own table removes one,
written by the frameworks under Folio's identity — while the claim that nothing left behind still
runs is true only where the manual cleanup door was run.

One correction runs the other way, and it is the reason the verification step keeps paying for
itself: **the reviewers' line numbers are not evidence.** One area's citations were wrong by
thousands of lines throughout while the reasoning was correct, and the report claimed every link
had been opened firsthand. A verifier who trusted them would have found nothing at those lines and
might have called a true finding refuted.

## The owner's rulings of 2026-09-21

These were made while the fixes were in flight, and they changed dispositions rather than
priorities. They are stated here as rulings because that is what they are.

**Ctrl+click on a printed path keeps opening it.** The remedy C2-4 argued for — route a `file:`
target that came out of terminal text to the preview seat, so that a printed path is revealed and
never run — was implemented, reviewed, and **withdrawn before it shipped**. Ctrl+click on a file
printed in the terminal hands it to this machine's registered handler exactly as it always has; a
folder is still revealed; the program list is byte-for-byte what it was, and the additions the
branch had made to it were taken back out. The reason is the whole of it: *the modifier is the
user's consent*. It is a gesture the owner makes every day, one more step is a step backwards, and
what happens after a deliberate gesture is the user's business. The volume question the same branch
had added — refusing to treat a drive letter that stands for a share as local — went with it: a
drive letter standing for a network volume is where readers keep their work, and the check was
removed entirely. What shipped from that branch is the half that changes no behaviour at all: the
window thread stops asking the disk, and a name the background ledger says is gone says so.

**A preview is not a security boundary.** The network rules on a previewed local document are
hygiene, not a wall, and nothing is promised about them. An ordinary browser opening a local page
lets it reach the network freely; the rule that a local document may not is stricter than any
browser and cannot be honoured without turning scripts off, which would end the interactive
preview. So the existing refusals stay as best effort, no new mechanism is built for them, and the
closure review's verdict that the Windows WebSocket hole was a must-fix is withdrawn. The other
side of that ruling is a feature ticket: **opening a local HTML preview to the network is first on
the 0.4.4 ledger** — today a previewed page cannot fetch a stylesheet, a chart library or a font,
which readers meet as a broken preview rather than as a protection.

**Three ledger items are struck.** The paste warning for nested shells (B-1), the owner check on
the clipboard-picture store (B-2), and the nonce on prompt marks (the stronger gate proposed beside
C2-3's fix) are off the ledger and will not be implemented.

**And the hard requirement is narrowed, for every audit after this one.** A security finding counts
when something *executes, writes a file, types into the terminal or sends a credential with no user
gesture at all*. What follows a deliberate gesture — Ctrl+click, "Open" in a menu, a paste, a drop,
a double click — is the user's business and is recorded as "not fixing" rather than triaged. Two
exceptions stand, both the owner's: the files column still does not open an executable directly,
and the repaint chord is still only sent when the marks arrive as a complete set, because nobody
made a gesture there and Folio is the one typing. Implementation tickets must also say in writing
that the implementer may not add refusals, warnings or checks of their own without asking first —
this audit's fixes grew three such additions, none of them requested.

The tracing that produced that narrowing is worth recording too: none of the recent security
measures originated with the owner. A brief named unrequested execution and privacy as hard
requirements, the triage promoted them to must-fix, the remedies were chosen by the triage, and the
implementers each added a further layer. The root was the brief and the triage bar, not the
reviewers.

## Two process lessons

**A fix that re-derives instead of moving costs eight rounds.** The C2-2 repair — take the disk
question off the window thread — went through eight review rounds before it merged. Every round
found the same class of difference: the branch had written a *new* answer to a question `main`
already answered, so a path under a link the resolver could not decode went dead, an intermediate
junction into a live share went dead, a printed folder spelled with a `..` step went dead, and a
bundle behind a link not named like one became openable. None of those was the defect being fixed;
each was a new answer disagreeing with the old one. The shape that finally merged is the one the
owner's own words describe: *the same question, asked on another thread*. The existing function was
moved to the worker verbatim and both the old door and the new one call it. Two rules come out of
it. **Reuse the function rather than re-deriving its answer** — if the fix is "somewhere else asks
this", the fix is a move, and any behaviour difference at all is a finding against the branch.
**And at least one test per seam must run through the real producer, not a fixture**: three of the
eight rounds' findings were invisible to the branch's own tests because those tests handed the door
a hand-made answer that the real resolver could no longer produce, so a test stayed green while
pinning a rule that had been withdrawn.

**A whole audit before a patch release turns the patch into a large release.** 0.4.3 was meant to
be a small version. It absorbed six knives, ten must-fix findings, two to four review rounds each,
and a full continuous-integration run per round, and it shipped days late with a changelog the size
of a minor version's. The rule the owner drew from it: **audits belong to minor versions.** A patch
release takes the defects its readers actually hit; a full adversarial pass is scheduled once per
minor version, triaged strictly, with most of what it finds going to the ledger by default. Reviews
of an ordinary small fix stop at two rounds.

## The 0.4.4 ledger, as this audit leaves it

- **D2-2 — unsaved edits die on the failure road and on panic.** Top of the list. Both roads run
  through one window-closing function; that function should write every dirty buffer's bytes to a
  recovery file and record its path, and the panic hook should do the same before it leaves, since
  a panic loses strictly more. The question the door asks today stays where it is and becomes the
  question rather than the only protection.
- **Window-thread file work behind the Settings presses, and the lock waits with it** — including
  the system open call itself, which is synchronous on that thread and is the measured cause of a
  1.4 s stall on a Ctrl+click.
- **The folder flyout's rows still ask the disk on the window thread**, from a folder a program
  named. The links branch moved the hover and the press off that thread and left this one.
- **The preserving replacement is a sequence, not an atomic rename**, so a crash or a power cut
  inside it can leave the document under neither its own name nor the temporary one. Open by
  declaration in the 2026-09-20 entry rather than by oversight; it wants a reading, not a patch.
- **E-3a — the copies Folio keeps of other people's configuration files** take this process's
  permissions instead of the source's, at three sites of one shape. One shared function, not three
  repairs.
- **F-2 — the inline-formula composite has no byte ceiling**, and the texture refusal that follows
  a composite over the budget re-uploads it every frame. Two tickets, the second the more valuable.
- **F-3 — a GIF's declared loop count**, with one dated line deciding what the count means.
- **B2-3 — the clipboard's text and file rungs have no ceiling**, on the window thread. The file
  count also closes the drop cap the design ruled and the shipped road never received.
- **A-4 — the delegate selectors are injected one at a time with no rollback.** The free mitigation
  shipped; the two-pass rewrite is owed the next time a selector is appended, or at a version bump.
- **F-1's remainder** — the notice for a hard-linked save, the true synchronise the owner ruled for
  0.4.4, and the agent-configuration writer, which loses the same metadata the editor used to.
- **D2-Q3 — the shell-integration scripts are written non-atomically**, two lines.
- **D2-Q1 — one note against the retired inline-image band switch**, whose reversal would make a
  frame-shape refusal reachable from program output.
- **The standing decoration animation** — indeterminate progress and a waiting ticket redraw at the
  display's rate while nothing on the glass changes, including while the window is invisible. Ruled
  **stop when not visible**, keep the rate when visible; scheduled for **0.4.5**, with the
  before-and-after measured on the first frame after the window becomes visible again.
- **The graphics-dependency precondition from D2-1a** — the two swapchain arms must be given a
  consecutive-failure counter and a log line **before** the graphics library is upgraded, because
  the loop is unreachable only for as long as the pinned backends refuse to produce those states.

## Unresolved

- **Whether a progress report should outlive the program that made it.** An indeterminate progress
  set by a program that exits without clearing it keeps the ring turning until the next command, or
  for the whole session on a pane with no shell integration. The visibility half is ruled; this
  half is not, and the existing entry on those states does not address it.
- **What the E-1 row should say about its own method.** Area E's reviewer and verifier were the
  same reader. The row is stated as confirmed because the trace was re-derived at two commits and
  the fix is pinned by four tests that are red on the baseline, but it has not had the second pair
  of eyes every other row has had.
- **Two of the knives' own open questions were never settled**, and both were left open
  deliberately rather than answered from memory: whether the console host forwards a multi-parameter
  synchronized-update sequence verbatim (it only gates one of C2-1's three spellings, and the other
  two and the non-Windows arm need no such assumption), and what a common modal text editor does
  with the repaint chord (the line-editor consequence is attested in the product's own comment, so
  C2-3's triage did not depend on it).
