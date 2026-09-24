# Focus follows a dropped path: round 5, 2026-09-17

Reviewed `cf50e163` against `31dda7bb` and the round-4 review.
App references below are `crates/bt-app/src/main.rs`; backend references are
the locally inspected, Cargo.lock-pinned winit 0.30.13 `src/platform_impl/`.
**Verdict: merge with must-fixes (bounds documentation only) before 0.4.2.**
Bar (a), retained-address drop protection: passes; drop/paste routing unchanged.
Bar (b), keyboard/composition: supported traces pass; unconditional safety is
not established. No demonstrated native regression warrants holding the code.

**Round-4 P2: closed** (`:18471`, `:18474`, `:18478`; routing guard `:99364`).
Honoured cancel: In(A) -> move B -> empty preedit -> Retiring(A);
punctuation's empty preedit -> Idle -> Commit(",") delivered.
Same-field empty preedit ends immediately (`:18469`); lifecycle notices are
not composition events (`:18494`). Bound (2)'s no-event loss is now accurate.

**All three backend citations verified, with precise line corrections.**
Windows `event_loop.rs:1550-1557`: empty preedit at `:1552`, Commit at `:1556`.
Windows `:1592-1599`: empty preedit at `:1594`, Commit at `:1598`, both before
Disabled (`:1603-1607`). These are adjacent send calls, not adjacent Rust lines.
macOS `view.rs:412-413`: adjacent queue calls, gated by hasMarkedText at `:411`.
Its unmarkText clears marked text at `:340` and queues empty preedit at `:345`.
These are all Commit emission sites in the Windows/macOS backend directories.

**One attack: double-clear followed by the old result.**
Without an earlier clear: In(A) -> empty at B -> Retiring(A) -> Commit(old)
is discarded. With an earlier cancel-time clear: In(A) -> Retiring(A) ->
later empty at B -> Idle (`:18474`) -> Commit(old) is DELIVERED (`:18478`).
The code cannot distinguish this from honoured cancel plus fresh punctuation;
the existing honoured-cancel test (`:172859-172875`) exercises the same states.
Windows permits the message translation: zero lparam (`:1537-1542`) does not
disable composition, so a later GCS_RESULTSTR can emit the pair (`:1546-1557`).
This is backend representability, not evidence that a supported IME retains
and commits old text after that clear. No such IME sequence is established here.
[Microsoft defines zero GCS flags as cancellation](https://learn.microsoft.com/en-us/windows/win32/intl/wm-ime-composition),
and [CPS_CANCEL clears the composition](https://learn.microsoft.com/en-us/windows/win32/api/imm/nf-imm-immnotifyime); neither demonstrates the hypothetical refusal.
macOS unmarkText also removes the marked text required for a later Commit.
**Must fix:** state this indistinguishable double-clear/late-result order as
an additional bound in `docs/DESIGN.md:1493-1495` and `main.rs:18428-18452`.
Pair adjacency proves no intervening event, not absence of an earlier clear;
qualify the universal safety claim. No behavior fix required without native evidence.

Validation: `cargo test -p bt-app --bin folio <filter> -j 4`: `ime` 65, `focus` 114, `drop` 98; 277 executions, zero failures.
Product code read-only; no scratch tests, application launch or process termination.
