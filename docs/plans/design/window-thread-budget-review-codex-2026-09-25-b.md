**Verdict: adopt revision (b) with changes; do not dispatch A1, A3 and A5 unchanged as a three-ticket batch. A5 is ready after the 0.4.5 tag. Correct A1's admission contract before dispatch, then dispatch A3 against that agreed interface; A3 lands after A1.**

Reviewed on `docs/window-thread-budget`, HEAD `f566e89c`. I read the whole note and the first Codex review. This review judges only “Revision 2026-09-25 (b), after the Codex review”; its precedence paragraph governs the earlier text. References below identify paragraphs of (b), not demands to rewrite the superseded proposal. This is a design review, not an implementation acceptance or a performance measurement.

| Finding | Assessment | Paragraph of (b) that answers it | One change still needed |
|---|---|---|---|
| R1 — spellings versus execution | **Partly answered** | R1 items 1–4 replace spelling matching with a resolved lint, execution-role checks and a scoped token; §R-A, “The source guard, restated,” expands the universe and pins the doors. | Replace §R-A's syntactic door contract with an explicit, sealed execution-capability contract and a coverage matrix that tests its escapes. The required boundary is detailed below; the current checks can all be green while its claimed property fails. |
| R2 — targets versus aggregate admission | **Answered** | R2's corrections; §R-B, “The composition inequality,” “The numbers are provisional policy targets,” “Enforced in 0.4.6,” and “Disclaimed”; §R-G Q1. | — |
| R3 — accounting backstop | **Answered** | §R-C items 1–5 and its eight deterministic cases: whole-turn recording before the threshold, inclusive call measurements, interval union, persistent summaries, loss counters, unchanged in-progress watchdog responsibility and measured cost. | — |
| R4 — hand-off's partial conformance | **Answered** | §R-D, “Expected failures are data,” the hand-off table row, and “B3 does not repay D-33.” | — |
| R5 — D-41 evidence | **Answered** | §R-E's replacement paragraph, from “Its logs can classify and prioritise” through the matched/interleaved experiment and picture-resumption requirement. | — |
| R6 — incomplete lane obligations | **Answered** | R6's three instance corrections; §R-D, “The obligations,” its four adapter rows and its expected-failure harness paragraph. | — |
| R7 — registry/prose divergence | **Answered** | R7, “One registry is authoritative,” all three following bullets, and the final paragraph leaving substantive ruling approval to review. | — |
| R8 — ticket dependencies | **Answered** | R8's ledger alignments; §R-F's prerequisite/landing table and opening paragraph; §R-G's watcher-retention decision. | — |

“Answered” means the design now supplies the missing obligation; it does not assert that its future implementation passes. In particular, R3's 5,000 × 0.9 µs case requires preserving sub-microsecond precision internally before aggregation: converting each duration to integer microseconds would fail its own acceptance. The 40 ms unclassified case must cover named `Scope`/`Work` time as well as time outside stations. R8's “can proceed in parallel” permits preparation on dependent branches; it does not remove the table's A1 prerequisite for A3. Neither detail needs a new finding.

The unresolved cases below are continuations of R1, not additional R9-style findings. Each concerns a claimed admission property that can fail while the specified guard remains green.

**R1: what the Clippy check actually establishes.**

R1 item 1 is substantially right about direct calls. On the pinned toolchain (`rustc 1.94.1`, Clippy 0.1.94, commit `e408947bf`), `DisallowedMethods::check_expr` checks resolved path expressions and type-resolved method expressions. It can catch a listed function taken as a value, and a listed `rx.recv()` without lexical spelling matching. Its path resolver visits inherent implementations, including nonlocal implementations. Thus **inherent methods on foreign Rust types are not categorically invisible**: `File::sync_all`, and resolvable listed wgpu/windows-rs methods, are legitimate candidates. What it does not do is infer blocking behavior from a method body, recursively inspect a foreign library, or establish when a returned value performs its effects. See the pinned [lint implementation](https://github.com/rust-lang/rust/blob/e408947bf/src/tools/clippy/clippy_lints/src/disallowed_methods.rs) and [path resolver](https://github.com/rust-lang/rust/blob/e408947bf/src/tools/clippy/clippy_utils/src/paths.rs).

There are three missing enforcement boundaries:

1. **Suppression is broader than the counted `expect`.** Add `#[allow(clippy::disallowed_methods)]` to an unregistered first-party helper containing a listed `fs::metadata`, and call it from the window thread. The four §R-A assertions still pass: no registered `expect` moved, every registered door still has its expected signature/check, and the vocabulary is unchanged. `-D warnings` does not make a locally allowed lint fire. Crate/module allowances, `cfg_attr` and allowances of containing lint groups need the same policy. Also, one `#[expect]` can suppress multiple effects; counting attributes is not counting the effects they authorize. Its fulfillment proves that a lint occurred, not that precisely the registered effects occurred. [Rust's lint-level rules](https://doc.rust-lang.org/reference/attributes/diagnostics.html#lint-check-attributes) describe these semantics.
2. **Macro expansion and FFI need explicit coverage.** A local macro expanding to a listed ordinary call is not automatically invisible. But external-macro diagnostics can be suppressed by the compiler, and linting every crate does not type-check every unexpanded exported macro body as an executable function. A first-party exported macro can emit a listed call in another crate while neither crate has a pinned door for the executing effect. Rust's [external-macro diagnostic handling](https://github.com/rust-lang/rust/blob/e408947bf/compiler/rustc_middle/src/lint.rs) makes this a separate case to prove, not something M1 establishes. For windows-rs, distinguish a direct listed binding/method from COM-vtable function-pointer calls or a newly declared `extern` entry point. For objc2, distinguish a generated Rust method from `msg_send!` selector dispatch. A selector is not itself the configured Rust method path. Listing `ShellExecuteW` or an AppKit wrapper does not fence those alternate routes. The blanket “foreign APIs that wait” sentence specifies no check for them.
3. **Only compiled configurations receive the resolved lint.** A file-set diff is not proof that every platform/feature branch was linted. The existing CI has workspace Clippy on Windows, but the macOS Clippy step names `bt-platform`; checking the remaining crates is not linting them. A2 must name the supported product configurations and plant platform-specific violations in those jobs. A shared vocabulary must distinguish a genuinely unavailable target-specific API from a misspelled or unresolvable entry; silently skipping entries is not coverage.

The existing “honest limit” permits an **unlisted** third-party blocking function as debt. It does not cover a **listed** effect escaping the checker through one of these mechanisms. Nor does whole-turn reporting establish admission: a short unregistered call violates admission even if the turn stays below every reporting threshold. State the invariant as “the enumerated effects, under the verified configurations and escape restrictions, pass through the checked doors,” and keep remaining effects explicitly outside that invariant.

**R1: a lifetime is useful, but the token needs authority as well as scope.**

The stated `std::thread::scope` pattern is a sound direction. A genuinely fresh lifetime bound by `for<'scope>`, with a result type independent of that lifetime, prevents returning the token or a closure/future that retains it. Invariance alone does not make a lifetime fresh: a caller-selected `'scope` in the constructor's signature would not establish this. A sealed type with private fields and no alternative safe constructors is not forgeable in safe Rust merely because callers can name it. An unsafe fabrication is a separate escape to fence, not a reason to reject capability types. The actual higher-ranked pattern is visible in [`std::thread::scope`](https://doc.rust-lang.org/std/thread/fn.scope.html).

However, even granting correct generativity and privacy, two safe sequences remain permitted by (b):

- A worker calls `hang_watch::admitted(..., |token| owner_door(token, ...))`. Nothing was sent across threads or leaked. §R-A checks only that a Window door takes `WaitToken`; it does not require an owner capability to mint that token. `!Send` constrains transfer, not creation. The Window-only property can fail with all four assertions satisfied.
- A caller obtains a token for one row/station and gives it to another Window door. The proposed unparameterized `WaitToken<'scope>` satisfies both signatures, and neither §R-A nor M4 requires the door to validate its identity. The effect can be charged to the wrong row or threshold, or run in an unauthorized lifecycle phase. R7's accurate generated table does not link the runtime token to the actual effect.

A1 therefore needs to specify the constructor and door signatures, their crate ownership, and their invariants. Require owner authority at `admitted`, reject an uninitialized thread role, seal role-setting/minting to the actual thread-entry paths, make the capability both non-Send and non-Sync, and bind the actual door to its registry identity and phase. A typed door identity or identity supplied and validated by the door can do this; a caller-selected `(row, station)` pair alone cannot. Lower-level doors in `bt-platform` and `bt-render` cannot import a concrete type defined only in the application binary: choose a shared, sealed representation without adding a public alternative mint or an externally implementable “token” trait.

M4 must test returning the token, storing it outside the scope, returning a capturing closure/future, transferring it by value and by reference, constructing it from an unrelated module, minting it on a worker/unregistered thread, and using the wrong door identity. The ordinary no-token call is only the simplest case. Keep the measurement guard owned by the synchronous admission/door invocation so `mem::forget(token)` cannot suppress recording; a leaked allocation is not permission to extend a Rust lifetime.

**R1: checking the caller before returning work does not check the executor.**

Consider a registered worker door with this shape:

```rust
#[expect(clippy::disallowed_methods, reason = "registered-door")]
fn door() -> impl FnOnce() {
    thread_role::expect_worker();
    || std::thread::sleep(delay())
}
```

The worker calls `door()` and sends its returned closure to the window thread. The role check passed on the worker. The effect is lexically under the registered `expect`; the owner/count are unchanged. The window invokes the closure and sleeps. No check runs at the effect. Replacing the returned closure with an `async` block has the same problem: creation and polling are different execution points. A role check at the start of an `async fn` runs on its first poll, which is better, but it does not authorize a later poll after migration to another thread. Async blocks [produce futures](https://doc.rust-lang.org/reference/expressions/block-expr.html#async-blocks); they do not synchronously execute their bodies when constructed.

A Window door can similarly take and consume/drop its token, then return a `'static` closure or future containing the raw effect. That value carries no scope lifetime, so even a perfectly generative `admitted` can return it. Only construction was measured. There is no requirement in the four source assertions that the token participate in the operation that eventually blocks. A returned raw function pointer, iterator, resource with a blocking destructor, or registered callback can cross the same boundary.

For this first milestone, specify synchronous effect doors: checks and per-call measurement immediately enclose the raw execution; the door cannot export an unchecked effect-bearing value. An async/callback/streaming operation needs its own declared contract, with the check at each executing/polling entry and accounting for the actual synchronous occupation, rather than treating construction as completion. Add mutations that move the effect into a returned closure/future and across a callback boundary. This closes the original first review's returned-closure counterexample, which M4 currently omits.

**R1: the worker-only check is diagnostic in release.**

R1 item 3 explicitly allows a wrong-role call to continue in release after incrementing a counter. That is useful evidence and respects the no-crash requirement. It does **not** establish that a worker-only effect cannot execute on the window thread. Add a caller on an unexercised UI branch: the lint permits calls to the door, the source guard finds its leading `expect_worker()`, normal tests need not reach that branch, and release executes the wait. This is not the same guarantee as compile-time exclusion.

Choose the guarantee before A1 is dispatched. To retain an enforced worker-only boundary without a release panic or fallback, require a sealed worker execution capability created inside `spawn_at_priority`'s running closure and usable only on that thread, including all supported worker-entry variants. Keep the assertion as a backstop. Otherwise explicitly describe wrong-thread detection as debug/test enforcement plus release diagnostics, carry the enforcement gap as debt, and withdraw “Contract 1 ... enforceable” for this property. Unknown must not silently mean Worker. Native callbacks and pre-loop/exit exceptions need named roles/phases rather than exemptions inferred from function names. Violations must remain visible in persistent summaries even when detail delivery drops records.

**R1: what M1–M9 prove, and what they do not.**

| Mutation | Assessment of the specified red result |
|---|---|
| M1 — cross-crate metadata helper | Good positive coverage for a resolved, compiled ordinary call. It does not establish macro, FFI or other-target coverage. |
| M2 — move a wait before spawn | Red only if the moved effect leaves every permitted suppression scope. Also test movement within one suppressed lexical owner and into/out of a nested closure. Attribute-owner/count equality alone cannot distinguish those cases. |
| M3 — direct window caller of worker body | Must execute that exact path and fail at the role check before the effect. Merely compiling/running unrelated tests is green. In release, as written, the expected observation is a violation counter, not prevention. |
| M4 — empty admission followed by door | Correct minimum compile-fail case; insufficient for generativity, worker-side minting, identity, or delayed-effect escape. Assert the intended diagnostic and compile the valid control. |
| M5 — `dyn Fn` and function pointer | Indirection itself is legal. Specify the mutation as a wrong-role or missing-capability invocation and actually execute it. A correctly admitted indirect call is a necessary passing control. |
| M6 — receiver method syntax | Appropriate positive test of type resolution; also prove listed foreign inherent methods on the pinned toolchain, including relevant platform bindings. |
| M7 — move an `expect` | Good owner/multiplicity test. Add an unregistered `allow`, a wider ancestor/group suppression, and an extra effect beneath an unchanged `expect`. |
| M8 — delete a vocabulary entry | Good, including coordinated deletion from registry/config without a ruling. Add misspelled/unresolved entries and platform-specific coverage controls. |
| M9 — add an unruly row | Good policy-delta test. Retain R7's other widening checks; a citation is still subject to substantive review. |

For each case, specify whether rejection is a compiler/lint diagnostic, source/registry failure, or executed runtime assertion, and verify that reason. An unrelated compile error or zero exercised requests cannot count. The two `gates-can-fail` plants are useful ongoing checks, but neither exercises role/capability enforcement. Put the added boundary cases into acceptance of A1/A2, not into an informal promise that a later reviewer will inspect every caller.

The source evidence above is from the pinned compiler/Clippy revision and Rust's language documentation. I did not run product mutations, native experiments or a workspace build. An attempted isolated compiler-probe command was blocked by automatic approval review; no probe results are claimed, and the conclusions above rest on source inspection. Only this review file is written; no commit is made.

**Dispatch verdict: A5 — go after the 0.4.5 tag. A1 — hold until R1's execution-capability boundary and its acceptance cases are concrete. A3 — go after that A1 interface is agreed, with landing dependent on A1. Do not dispatch all three unchanged or claim enforced admission before corrected A2 acceptance.**
