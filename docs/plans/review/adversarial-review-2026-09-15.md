# Adversarial code review, 2026-09-15

Reviewed: everything merged to `main` since v0.3.0-preview (`9acd482`) and read at `6a414963` —
the macOS port, the formula band, the large-document viewport and the whole release lane — in
four slices.

A. **The macOS platform layer.** The single-writer claim, the global hot key, the WebKit host,
   the handoff to LaunchServices and the video engine: what the port does that no Windows
   reviewer has ever had to read.
B. **The `bt-app` surface.** The large-document viewport, the menu bar, settings, the card model
   and the formula marks — the four places 0.4.0 put new state on a window.
C. **The crates under it** — `bt-render`, `bt-math`, `bt-term`, `bt-pty`, `bt-persist`,
   `bt-transcript`, `bt-viewport`, `bt-winres`.
D. **The release lane** — the macOS bundle, signing, notarization and image scripts, the release
   and CI workflows, the check scripts, and whether the documents describe what the code does.

How: one Codex reviewer read each slice, without a build and without running the program. Every
finding was then re-verified against the code by a reader who owns the verdict and the severity,
opens each line rather than trusting the reviewer's anchor, folds duplicates and names the
ticket. **Where the two disagree the verifier wins, and the severities in this document are the
verifier's.** No build, no test and no compiler was run at any point, no window was launched, and
nothing in the repository was modified except this file.

Row ids here are `R<slice>-<n>`: `RA-3` is slice A's A-3, `RC-1` is slice C's C-1.

## The numbers

| Slice | Reported | Verified real | Critical | High | Medium | Low |
|---|---|---|---|---|---|---|
| A the macOS platform layer | 5 | 5 | 0 | 2 | 1 | 2 |
| B the `bt-app` surface | 4 | 4 | 0 | 2 | 2 | 0 |
| C the crates under it | 2 | 2 | 1 | 0 | 1 | 0 |
| D the release lane and the documents | 6 | 6 | 0 | 1 | 2 | 3 |
| **Total** | **17** | **17** | **1** | **5** | **6** | **5** |

All seventeen survived verification: ten as reported (REAL) and seven with a trigger or a
consequence the reviewer had not got right (REAL-BUT-DIFFERENT — RA-2, RA-3, RA-5, RB-1, RB-3,
RD-2, RD-4). Nothing was ruled not a defect, at the level of a finding or of a sub-claim. There
are no duplicates this round: seventeen reports, seventeen distinct defects, which is itself a
fact about the slices — they barely overlap.

Five severities moved. **RC-1 up to critical**: a 360-byte formula inside a Markdown file or a
line of terminal output walks past the macro budget and asks the pinned lexer for 10^8 tokens,
and an allocation abort is not caught by the conversion's `catch_unwind`. **RB-4 up to high**: on
macOS the zero-window state is the ordinary resting state the port deliberately built, and in it
`Cmd+Q` and the Quit row are both dead. **RA-1 down to medium** (two Folio processes must be
started with different `TMPDIR` values, which is an environment mismatch rather than a gesture),
**RA-2 down to low** (Carbon delivers a hot key event only to the process that registered it, so
the handler can consume nobody's chord but this process's own) and **RD-4 down to low** (what is
lost is an archived log, not a release).

Three REAL-BUT-DIFFERENT verdicts are *worse* than reported once traced. **RB-1** is not "wakes
without an idle wait": the deadline is permanently in the past, which `about_to_wait_inner`'s own
comment forty lines below calls "a process at 100% CPU with nothing on any screen". **RA-3**
needs no ten-second deadline — any second `request_controller` while a compile is outstanding
does it, and `let_go` does not clear `compiling`, so a close during a compile latches the door
shut. **RD-2**'s packaging README tells the reader that the failing step's "own exit code is the
one to read", and that exit code is always 1.

**Nothing in this round touches the settings-open stall or the card wheel path.** Both were in
the reviewed range and both were read; the seventeen defects are elsewhere.
`feature/settings-open-stall` and `fix/card-wheel-lag` carry no row of this review.

## Slice A — the macOS platform layer

| id | Severity | Defect | Trigger | Anchor | Verdict | Ticket |
|---|---|---|---|---|---|---|
| RA-1 | medium | The single-writer claim and both socket endpoints live under `$TMPDIR`, so two processes of one user with different `TMPDIR` values `flock` different files: both answer `Some(DataDirectoryClaim)`, both write settings and sessions, and neither can hand a launch to the other. The kernel name the Windows arm claims cannot diverge this way. | a second Folio started from an environment whose `TMPDIR` is unset — an ssh session, an `env -i`, a daemon — while the first was started by launchd | `crates/bt-platform/src/instance.rs:331`, `:501`, `crates/bt-app/src/persist.rs:223` | REAL (lowered from high) | T-AUDIT3-MAC-PLATFORM |
| RA-2 | low | The Carbon handler returns `noErr` for every `kEventHotKeyPressed` it is given, including one whose signature is not Folio's, one whose id holds no live claim, and one whose direct-object parameter would not read — which is Carbon's word for "handled" and ends dispatch. | a hot key event for a registration this process has already released | `crates/bt-platform/src/hotkey.rs:1635`, `:1639`, `:1675` | REAL-BUT-DIFFERENT (lowered from medium: the handler is installed on `GetApplicationEventTarget`, and Carbon delivers a hot key event only to the process that registered it, so the only chord this can swallow is one registered inside this process — no other application's) | T-AUDIT3-MAC-PLATFORM |
| RA-3 | high | The rule-list compile captures generation 1's `WKUserContentController` and the shared door. A second `request_controller` while that compile is out returns at once (`compiling` is still set), and the completion then installs the list on the dead controller, sets the door's `stands`, and — because `wanted` has not changed, so `settled()` is true — reports the *current* generation ready. `install` reads `guards.resource_requests` off that same flag, so the live page is certified as gated while its controller carries no `WKContentRuleList`. `let_go` clears `page`, `attached` and `stands` and never `compiling`, so a close during a compile latches the same state in. | any retry or rebuild of the page while a compile is outstanding, with the seat's rules unchanged | `crates/bt-platform/src/macos_webview.rs:440`, `:505`, `:1207`, `:1372` | REAL-BUT-DIFFERENT (much broader trigger than the ten-second deadline the reviewer gave) | T-AUDIT3-MAC-WEB-RULES |
| RA-4 | high | `opening_it_would_run_it` asks `std::fs::metadata`, which follows a symbolic link, and then asks the *link's* name for the `.app` extension — so a link named `notes` pointing at an application is a directory with no `.app` suffix, passes the refusal, and is handed to `NSWorkspace::openURL`, which resolves it and launches the program. No race is needed; the file branch's execute-bit test does not have the hole, because a mode is read off the resolved file. | Open (or double-click) a tree row that is a symlink to a `.app` | `crates/bt-platform/src/handoff.rs:685`, `:752`, `crates/bt-app/src/main.rs:80721` | REAL | T-AUDIT3-MAC-OPEN-REFUSAL |
| RA-5 | low | The `folio-video-engine` thread calls into AVFoundation and Cocoa for the whole life of a media preview with no autorelease pool anywhere on it, which is the one thing Apple's memory-management contract requires of a secondary Cocoa thread. | play or seek a video preview | `crates/bt-platform/src/macos_player.rs:251`, `:650`, `:753`, `:824` | REAL-BUT-DIFFERENT (lowered from medium: this file's own per-iteration calls return scalars and `CMTime`s — `item.error()` is the one object, and `copyPixelBuffer` is `+1` and released — so what accumulates is whatever the framework autoreleases internally, which is a hypothesis this review cannot measure; the missing pool is not) | T-AUDIT3-MAC-PLATFORM |

Read and found sound in this slice: the runtime directory is made `0700` at creation, refuses a
symlink and refuses a directory of another user's; the claim unlinks both stale endpoints only
under the lock; the hot key handler's two-fact test — this process's signature *and* a live
ledger entry — is the Windows arm's rule spelled correctly; the WebKit host fails closed on a
rule list that will not compile, and parks the navigation rather than letting it through; and the
image door's own gate (`openable_local_image`) keeps its extension list.

## Slice B — the `bt-app` surface

| id | Severity | Defect | Trigger | Anchor | Verdict | Ticket |
|---|---|---|---|---|---|---|
| RB-1 | high | `math_copied` is written when a formula copy lands and is never cleared by any production path, so from 1,300 ms after that copy every `turn` returns a deadline already in the past and the loop is given `ControlFlow::WaitUntil(<past>)` for the life of the window — the state `about_to_wait_inner` itself describes, forty lines below, as "a deadline already in the past, i.e. a process at 100% CPU with nothing on any screen". | copy one formula, then leave the window open | `crates/bt-app/src/main.rs:85238`, `:100278`, `:109519` | REAL-BUT-DIFFERENT (worse: a permanent busy loop, not extra wake-ups) | T-AUDIT3-FORMULA-TOOLS |
| RB-2 | medium | Every Markdown buffer reads whole (`reads_whole` is set at construction), so a Markdown file over 8 MiB is answered `TooLargeToEdit` every time. `accept` clears `stale` in its preamble and the over-limit arm installs nothing when a body already exists, so the head shown at first open can never be refreshed: the buffer forgets it is behind the disk, and `Reload from disk` spends a read that changes nothing, for ever. | a Markdown file over 8 MiB that changes on disk — or one that grows past the cap | `crates/bt-app/src/preview.rs:5054`, `:6000`, `:6092` | REAL | T-AUDIT3-PREVIEW-OVERSIZE |
| RB-3 | medium | `math_hover_anchor` is window-owned and is cleared only by a pointer event, or by the 500 ms grace a pointer event starts. Change the active tab — or the focused pane, or close the pane — with the pointer stationary and nothing clears it; `math_tool_placement` then finds no frame that knows the anchor, and `sync_math_tools`' last arm deliberately keeps the marks standing, so tab A's source and copy marks are drawn over tab B until the pointer moves. | hover a formula, then switch tab by keyboard | `crates/bt-app/src/main.rs:84908`, `:84964`, `:85026`, `:85180` | REAL-BUT-DIFFERENT (broader: any focus change with no pointer event, not the tab switch alone) | T-AUDIT3-FORMULA-TOOLS |
| RB-4 | high | Quit is a `VerbNamed` row, and `verb_entry` enables a row only when there is a window focus — so with the last window closed the row is disabled, and because `Scope::Window` also makes it print its chord, AppKit swallows `Cmd+Q` on a greyed item. `answer_a_menu_row` would not serve it either: a menu-bar origin returns before decoding the choice when `frontmost_window()` is `None`. The disabled state is pinned by a test, so it is a ruling to be changed rather than an oversight. | on macOS, close the last window, then press `Cmd+Q` or choose Quit Folio | `crates/bt-app/src/menubar.rs:137`, `:548`, `:1021`, `crates/bt-app/src/main.rs:107969` | REAL (raised from medium) | T-AUDIT3-MAC-QUIT |

Read and found sound in this slice: `land_read` does reconcile a landed read against the buffer's
incarnation and revision and keeps the reader's bytes when it must, so T-EDIT-DISK holds;
`copy_math_latex` writes `math_copied` only for a clipboard write that actually landed; the marks'
own deadline asks for the next animation frame and for nothing once both journeys have landed, so
a resting pointer costs no wake-ups; and the menu bar's Help and application rows are enabled with
no window on purpose, which is what makes the Quit row's absence from that list an oversight
rather than a policy.

## Slice C — the crates under it

| id | Severity | Defect | Trigger | Anchor | Verdict | Ticket |
|---|---|---|---|---|---|---|
| RC-1 | critical | The macro budget's DAG charges each definition its body bytes plus each child's cost once, and the second pass charges an argument its *source* bytes times the body's `#` count — so amplification through a chain of parameterised macros is never multiplied along the chain. Eight definitions, each passing its parameter on tenfold, cost about 4 KiB against a 32 KiB ceiling and expand to 10^8 tokens. `mitex-lexer 0.2.4` materialises every level (`expand_tokens` into `extend_inner`) with no runtime budget, so the formula worker allocates gigabytes; an allocation abort is not a panic and `convert_math`'s `catch_unwind` does not contain it. The process dies, with every session in every window. | render a 360-byte display formula — from a Markdown file, or from text printed into a terminal | `crates/bt-math/src/macro_budget.rs:189`, `:248`, `crates/bt-math/src/lib.rs:41`, `:588` | REAL (raised from high) | T-AUDIT3-MACRO-BUDGET |
| RC-2 | medium | `math_tool_boxes` clamps a band's geometry against `self.seat`, and outside `compose_frame` that field names the **focused** seat by design ("Back to the focused seat…", `lib.rs:8371`). The app asks it for whichever pane's frame knows the hovered anchor, so in a split with unequal panes the marks beside a formula in the unfocused pane are clamped to the focused pane's width and height — placed over the formula's own ink, or suppressed by a shorter pane's height. `math_hit_test` reads the same field, so the press geometry is wrong in the same direction. | drag a divider to make one pane much wider, then hover a formula in the pane that has not got the keyboard | `crates/bt-render/src/lib.rs:382`, `:7067`, `:8174`, `:8371`, `crates/bt-app/src/main.rs:85028` | REAL | T-AUDIT3-FORMULA-TOOLS |

Read and found sound in this slice: the macro budget's *own* stated door is shut — an argument
that names a defined macro is refused outright, so `\newcommand{\a}[1]{#1}\a{\a}` cannot smuggle
recursion past the graph — cycles are caught, `\newenvironment`, `\edef`, `\let` and
`\expandafter` are refused, and the plain lexer really does emit `Token::Hash`, so `uses` counts
what it claims to count. What is missing is only the multiplication.

## Slice D — the release lane and the documents

| id | Severity | Defect | Trigger | Anchor | Verdict | Ticket |
|---|---|---|---|---|---|---|
| RD-1 | high | The bundle's `Contents/Resources` is given one file, `Folio.icns`, and the image is given the bundle and a link to `/Applications` — so neither macOS download carries `LICENSE-MIT`, `LICENSE-APACHE`, `THIRD-PARTY-NOTICES.md` or `TRADEMARK.md`, and nothing embeds them. `dmg.sh`'s own comment refuses to add a licence file "because the application carries its own", which is untrue; the Windows archive carries all four deliberately, as "what a recipient is owed". | make any macOS release with these scripts — including the one already published | `scripts/release/macos/bundle.sh:296`, `scripts/release/macos/dmg.sh:21`, `:178`, `scripts/release/package.ps1:254` | REAL | T-AUDIT3-MAC-RELEASE-ASSETS |
| RD-2 | medium | All three manual recipes sign without `--no-spctl` and then notarize, and `sign.sh` asks Gatekeeper about a Developer ID bundle Apple has not seen yet and `exit 1`s on the refusal it correctly predicts. The workflow passes `--no-spctl` for exactly this reason. | follow the documented sequence with a real identity | `scripts/release/macos/sign.sh:259`, `:277`, `docs/RELEASING.md:716`, `packaging/macos/README.md:140`, `docs/DESIGN.md:10001` | REAL-BUT-DIFFERENT (worse: `packaging/macos/README.md` tells the reader this step's "own exit code is the one to read rather than that line", and that exit code is always 1) | T-AUDIT3-RELEASE-RECIPE |
| RD-3 | medium | `bundle.sh` validates the executable only by existence, renders the plist from the checkout's version, and copies whatever is at `target/release/folio` into it. Nothing downstream compares the binary's version or commit with the tree, so the manual route can sign, notarize and ship a stale program wearing the new version. The workflow's `--version` comparison guards only the workflow. | a release built in a checkout with an older `target/release/folio` still in it | `scripts/release/macos/bundle.sh:173`, `:281`, `:284`, `.github/workflows/release.yml:511` | REAL | T-AUDIT3-MAC-RELEASE-ASSETS |
| RD-4 | low | The notarization log is fetched with `|| true` and the next line prints "log kept at $log" whatever happened — so a transient failure loses the one artifact `docs/RELEASING.md` requires be archived per submission, and if an earlier run left a file at that path the script presents *that* submission's log as this one's. | a nonzero `notarytool log` after an accepted submission | `scripts/release/macos/notarize.sh:229`, `docs/RELEASING.md:772`, `.github/workflows/release.yml:423` | REAL-BUT-DIFFERENT (lowered from medium; the stale-file case is the reviewer's claim made worse) | T-AUDIT3-RELEASE-RECIPE |
| RD-5 | low | The allowlist is extracted with `\[(?<body>[^\]]*)\]`, which stops at the first `]` inside the array — and comments are stripped only after the match succeeds. An ordinary comment naming `#[cfg(windows)]` or `[&str; 11]` inside the constant makes the gate throw "no longer declares FILES_THAT_MAY_NAME_A_PLATFORM" over a source its Rust twin accepts. No comment in the constant carries a `]` today, so the gate works and is one comment away from not working. | add a comment containing `]` to the constant | `scripts/check-portable-core.ps1:227`, `:236`, `crates/bt-app/src/main.rs:163034` | REAL | T-AUDIT3-DOC-CLAIMS |
| RD-6 | low | "English and Chinese — every string in both" is written in four documents, and the Finder Services row is fixed English in the plist — where the comment beside it states that a localized Services item needs bundle localization, which is its own ticket and has not happened. | read the claim; open Finder's Services menu with Folio set to Chinese | `README.md:83`, `README.zh-CN.md:73`, `docs/features.md:347`, `packaging/macos/Info.plist.in:150`, `scripts/release/macos/bundle.sh:260` | REAL | T-AUDIT3-DOC-CLAIMS |

Read and found sound in this slice: the release workflow does pass `--no-spctl` before
notarization and does assert Gatekeeper in both directions afterwards; it compares the built
bundle's `--version` against the workspace version and the checked-out commit; `notarize.sh`
fetches the log for a rejection as well as for an acceptance, and refuses a status that is not
`Accepted`; the Windows archive's contents are asserted against a list, and that list is where
the four documents a recipient is owed are named; and `check-portable-core.ps1` reads the
allowlist out of `main.rs` rather than keeping a second copy, which is the right shape with the
wrong reader.

## Tickets

Ten tickets. Seven are **before 0.4.1 ships**; three follow it. Three belong on branches already
in flight and say so. Every brief is in `scratchpad/review3/tickets/`, one file each,
self-contained for an implementing agent. No row of this review is left open.

### Before 0.4.1 ships

#### T-AUDIT3-MACRO-BUDGET — the macro bound counts definitions, and the cost is per call

RC-1. The one critical row of this round: a formula in any document this window renders can end
the process. The budget's cost model has to carry an argument-dependent multiplier along the
reference DAG — or, as the reviewer proposes and this verifier accepts as the smaller and safer
change for 0.4.1, refuse a call to a parameterised user macro from *inside* a macro definition,
which is the one shape that compounds. Direct parameterised calls with literal arguments keep
working. **Its own branch**: this is `bt-math` and its tests, not the formula band's UI.

#### T-AUDIT3-FORMULA-TOOLS — the band's marks: one clock, one anchor, one pane (`feature/math-block-polish`)

RB-1, RB-3, RC-2. All three are the formula marks, and the branch for polishing them is open and
still empty. Why together: the copy tick is a clock nobody stops, the hover anchor is a fact
nobody clears when the window moves on, and the geometry is measured against a pane that is not
the one being drawn. One pass over `math_copied`, `math_hover_anchor` and the seat the boxes are
clamped to closes all three, and RB-1 makes this a must-fix on its own.

#### T-AUDIT3-MAC-WEB-RULES — a rule list belongs to the controller it was compiled for

RA-3. A security guard reported as standing when it is not. The compile and the installed-rule
state have to be generation-stamped, an obsolete completion has to change nothing, `let_go` has
to end the in-flight state it currently leaves latched, and `guards.resource_requests` has to be
a statement about the controller now on the glass.

#### T-AUDIT3-MAC-OPEN-REFUSAL — the refusal is about the file that will actually be opened

RA-4. Resolve the path first, classify the resolved target, and hand LaunchServices that same
resolved target. A symlink to an application must get `PROGRAM_REFUSED`.

#### T-AUDIT3-MAC-QUIT — an application with no windows can still be quit

RB-4. Quit is enabled independently of window focus and dispatched before a window is looked up;
`menubar.rs:1021`'s assertion is rewritten to say so.

#### T-AUDIT3-MAC-RELEASE-ASSETS — what goes in the bundle, and whether it is this build (`chore/release-script-fixes`)

RD-1, RD-3. Why together: both are `bundle.sh` assembling a container out of inputs it does not
check. The four documents go into `Contents/Resources` before signing and are named in the bundle
verification; the executable is asked its `--version` and refused when it is not this checkout's.
RD-1 is a licence-compliance gap in a download that is already published, which is what makes
this a must-fix rather than a tidy-up.

#### T-AUDIT3-RELEASE-RECIPE — the documented sequence runs, and the log it promises is kept (`chore/release-script-fixes`)

RD-2, RD-4. Why together: both are the release lane telling its operator something that is not
true — a step whose exit code is guaranteed to be 1 and described as the thing to trust, and a
log claimed to be kept when the fetch failed. `--no-spctl` goes into all three manual recipes;
the log is fetched to a fresh path, required to succeed and to name this submission, and only
then moved into place.

### After 0.4.1

#### T-AUDIT3-PREVIEW-OVERSIZE — an over-cap buffer is still behind its file

RB-2. Separate "this answer replaces the body" from "this answer closes the question": a clean
disk reload installs the fresh fallback head with its index, revision, truncation flag and mtime,
while an edit-upgrade refusal keeps the reader's body — and a buffer that installed nothing does
not clear `stale`.

#### T-AUDIT3-MAC-PLATFORM — three Apple contracts the port does not keep

RA-1, RA-2, RA-5. Why together: one crate, three small independent contracts, none of them worth
a ticket alone — the ownership lock and the endpoint names derived from the canonical data
directory rather than from `$TMPDIR`, `eventNotHandledErr` for an event this process did not
handle, and an autorelease pool around the video worker's loop.

#### T-AUDIT3-DOC-CLAIMS — the gate reads what the source says, and the claims say what the code does

RD-5, RD-6. Why together: both are a statement about the source that the source does not support
— a reader that treats a bracket in a comment as the end of an array, and four documents that
promise Chinese for a row the plist fixes in English and says so.

## Method

Four slices, one reviewer and one verifier each, over a range that included a whole new platform.
The verification step earned its cost again and in a different way from September 11th: fewer
severities moved (five of seventeen), but two of the three worst rows of the round turned out
worse than the report said, and one of them — RC-1 — moved a grade because the verifier read the
pinned third-party lexer rather than the wrapper around it. A budget is only a budget against the
thing it is a budget of.

The recurring root cause of this round is **the answer that outlives its question**. A rule list
compiled for a controller that is gone certifies its successor (RA-3); a copy tick that has
expired keeps asking for the frame that would retire it (RB-1); a hover anchor keeps naming a
block in a tab nobody is looking at (RB-3); a read that replaced nothing clears the staleness that
would have asked again (RB-2); a cost computed once per definition stands in for a cost that
multiplies per call (RC-1); a seat restored for the focused pane answers for a pane that is not it
(RC-2); a stale executable wears the version of the checkout that did not build it (RD-3); and a
log line printed after `|| true` reports a file that was never written (RD-4). Last round's class
was *identity* — something named by where it is drawn. This round's is *lifetime*: something that
was true when it was computed and is consulted after it stopped being true.
