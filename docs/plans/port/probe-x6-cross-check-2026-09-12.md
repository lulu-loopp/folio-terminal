# X-6 — what a Windows agent can actually check about macOS code

*2026-09-12. Ticket X-6 of `docs/plans/port/macos-plan-2026-09-12.md` (§3, §7.1).
Branch `probe/macos-cross-check`. Run on the Windows workstation, `-j 2`,
`CARGO_INCREMENTAL=0`, `cargo check` only, one cargo at a time.*

## Setup

`rust-toolchain.toml` pins `1.94.1-x86_64-pc-windows-msvc`; that toolchain had
only its own host target installed, so `rustup target add aarch64-apple-darwin`
was run against it. Nothing outside `~/.rustup` was touched. Every command below
ran with the repository's `.cargo/config.toml` in force, and `cargo check -v`
confirms the flag reaches rustc: `target-feature=+crt-static` appears on the
Apple-target command line. CI's `core-linux` job overrides `RUSTFLAGS` to cancel
it; `core-macos` does not, and neither did this probe.

## Probe A — the objc2 dependency graph

A scratch crate with its own `[workspace]` table, its dependencies under
`[target.'cfg(target_os = "macos")'.dependencies]` exactly as §4.6 requires, and
a `lib.rs` that names one class from each crate through `Retained<T>` so the
compiler has to look the type up rather than take the spelling on trust. Both
files are committed beside this one under `docs/plans/port/probe-x6/`.

| Crate | Version resolved | Result |
|---|---|---|
| `objc2` | 0.6.4 | checks |
| `block2` | 0.6.2 | checks |
| `objc2-foundation` | 0.3.2 | checks |
| `objc2-app-kit` | 0.3.2 | checks |
| `objc2-quartz-core` (`CAMetalLayer`) | 0.3.2 | checks |
| `objc2-web-kit` (`WKWebView`) | 0.3.2 | checks |
| `objc2-user-notifications` | 0.3.2 | checks |
| `objc2-core-graphics` (`CGEventTapLocation`) | 0.3.2 | checks |
| `core-graphics` (`CGEventTapLocation`) | 0.24.0 | checks |

**Probe A passes**, with `+crt-static` in force, with no errors and no warnings,
in about thirty seconds from cold. Two facts worth carrying into M1-2. First,
the `objc2-*` 0.3 crates gate every class behind a feature of its own name:
default features give you `std` and nothing else, so `NSWindow`, `CAMetalLayer`
and `WKWebView` each have to be asked for by name — a missing feature is an
unresolved import, which is a good failure mode and a noisy one. Second, the
lock file already carries `objc2` 0.6.4 and the 0.3.2 family through winit and
wgpu, so the backend adds features rather than a second copy of the graph.

## Probe B — a real app target

| Target | default | `--all-targets` | First error |
|---|---|---|---|
| `bt-platform` | passes | passes | — |
| 8 of the 13 portable crates | passes | passes | — |
| `bt-math`, `bt-term`, `bt-pty`, `bt-corpus` | fails | fails | `psm` build script |
| `bt-render` | passes | fails | `psm` build script |
| `bt-app` | fails | fails | `psm` build script |

The eight that pass everywhere are `bt-unicode`, `bt-doc`, `bt-detect`,
`bt-layout`, `bt-persist`, `bt-winres`, `bt-transcript` and `bt-viewport`.
**The first real error is not in this repository and has nothing to do with
`crt-static`:**

```
error: failed to run custom build command for `psm v0.1.31`

Caused by:
  process didn't exit successfully: `...\build\psm-63a257da37165045\build-script-build` (exit code: 1)
  --- stdout
  cargo:rustc-check-cfg=cfg(switchable_stack,asm,link_asm)
  cargo:rerun-if-env-changed=CC_aarch64-apple-darwin
  CC_aarch64-apple-darwin = None
  ...
  cargo:warning=Compiler family detection failed due to error: ToolNotFound:
  failed to find tool "cc": program not found
  --- stderr
  error occurred in cc-rs: failed to find tool "cc": program not found
```

The chain is `bt-math` → `typst-as-lib` → `typst` → `typst-eval` → `stacker` →
`psm`, and `bt-term`, `bt-pty` and `bt-corpus` inherit it through `bt-math`;
`bt-render` inherits it only through dev-dependencies, which is why its library
checks and its tests do not. `psm`'s build script calls
`cc::Build::get_compiler()` before it decides anything, and for `aarch64` off
Windows it assembles `src/arch/aarch_aapcs64.s`. This machine has no `cc`, no
`clang` and no `gcc` on `PATH` at all.

So **Probe B fails**, and X-6's stated pass condition — "both check" — is not
met. The failure is the second of the two kinds the ticket distinguishes: a
native tool, not a type error in portable code. A Windows agent cannot iterate
on it, and it stops before `bt-app`'s first line is read, so the interesting
question — where `bt-app` fails on Windows-only code — was never reached and
remains unanswered.

It is probably removable. The assembly `psm` needs is preprocessed and assembled
out of files in its own package, with no Apple SDK header anywhere in it, so a
clang on the Windows host, or `CC_aarch64_apple_darwin` pointed at one, should
satisfy it. That is an inference from reading the build script, not a measured
result: nothing was installed on this machine to test it.

`bt-platform` passing is the load-bearing positive result, and it deserves the
plan's own scepticism. Its source is 25,744 lines; `webview.rs`, `video/`,
`attention_pipe.rs`, `launch_pipe.rs`, `explorer_command.rs`, `http.rs` and the
`windows_impl` module inside `lib.rs` are 17,571 of them, and none of that
reaches rustc on the Apple target. Two thirds of the crate is gated out, and
the check is green. The plan's "compiles an empty crate" is right in substance:
what is verified is the portable third.

## Probe C — `--all-targets` versus the default

For every crate that passes, the two verdicts agree: `--all-targets` compiles
the lib test target and the integration tests as well, and all of them compile
for `aarch64-apple-darwin`.

| Crate | Test targets | `--all-targets` verdict |
|---|---|---|
| `bt-unicode`, `bt-doc`, `bt-winres`, `bt-transcript` | inline only | compiles |
| `bt-detect`, `bt-layout`, `bt-persist`, `bt-viewport` | 9 integration files, inline | compiles |
| `bt-platform` | inline, in 10 source files | compiles |
| `bt-render` | inline, in 9 source files | **not reached** — dev-dep on `bt-term` |
| `bt-math`, `bt-term`, `bt-pty`, `bt-corpus` | 22 integration files | **not reached** |
| `bt-app` | inline, in 84 source files | **not reached** |

**No test module in any portable crate fails to compile for the Apple target.**
The one crate whose verdict changes under `--all-targets` is `bt-render`, and
the change is its dev-dependency graph rather than its test code. Everything
else is unmeasured, not failing — and an unmeasured test module is exactly where
`core-macos`'s own comment says five of seven earlier platform assumptions hid.

## The conclusion

**Check is an authoring and compile venue, never acceptance**, and this probe
supplies evidence rather than assertion. `bt-platform` compiles clean for
`aarch64-apple-darwin` today while two thirds of its body is invisible to the
compiler; Probe A compiles nine Apple frameworks' bindings on a machine that has
never linked against a framework, because `cargo check` does not link. Selector
existence, clipboard, stdio, the link step and every native behaviour stay on
the Mac.

For §7.1's `check` column the recommendation is to split it in two, because one
word is currently doing two jobs:

- **`check` — compiles, and is verified on Windows today.** P-2 (no Rust at
  all), M1-2 (an inventory document), M1-9 (`NSPasteboard` lives in
  `bt-platform`, which cross-checks for the Apple target), M5-4 (a workflow
  file), M5-5 and M5-6 (the version gate and the documents both run against the
  Windows target, where they always did).
- **`author-only` — can be written on Windows, cannot be compiled there.**
  M1-10 and M2-6, both of which are `bt-app` work, and any later ticket whose
  code lands in `bt-math`, `bt-term`, `bt-pty`, `bt-corpus` or `bt-app`. A
  Windows agent may write those; it must not report them as checked.

One preflight ticket is worth adding ahead of M1-2: put a clang on the Windows
agent host, re-run Probe B, and move whatever then compiles from the second list
to the first. Until that is measured, `bt-app` on the Apple target is a claim
nobody on Windows has tested.
