**adopt with changes**

Static confirmation of revision (e), scoped to P3, P4 and P2's decoder wording. Read on `design/thread-door` at `e8290621`, against `78a3699a`; the branch changes only design/review documents. No build, tests, compiler probes or commit; only this file is written.

## P2 wording — met

Meeting sentence: “`Station::from_byte`'s fallback is never reached through `decode`, because the range check comes first.” The proposed decoder checks station, node, scope and reserved bits before constructing `Location::Resume`; today's `from_byte` returns `Station`, with a `Starting` fallback. The correction is explicit.

## P3 — not met

The inventory repair is substantive. `rg -a` finds eleven product commit statements: five in `bt-app`, plus `Compositor::new` and `set_window_size` (`bt-platform/src/lib.rs:3900,4031`), `WebHost::rehost` twice (`webview.rs:1920,1929`) and `compensate` twice (`1985,1994`). The latter has just one caller, `rehost`'s failure branch (`1948`). The three new enclosing batches account for all six internal statements, including compensation. The three direct test/example callers are correctly listed (`webview.rs:3987`, `portable_impl.rs:1822`, `examples/video-probe.rs:399`). `GpuOpen` correctly records `Runtime::create`'s `pollster::block_on` (`main.rs:41459`) as new pending row 23.

However, the specified private `Compositor::commit_now` cannot serve four of those six statements: `Compositor` is defined in `windows_impl`, while `webview` is its sibling (`lib.rs:3382,3398,3841`). A bare private method is inaccessible there. The brief must specify usable visibility and fence the tokenless helper's callers, rather than leave that contract to A1d.

Independent full-workspace `rg -a -n`, restricted to `*.rs`, spot-checked three rows:

- **SetCursor:** all 27 product `apply_pointer_cursor` calls match the table, including `chrome_mouse_input` ×3; its published pattern finds them.
- **WebEnvironment:** the three real `WebSeat::start_environment` calls are `open` ×1 and `step` ×2 (`webhost.rs:2188,2993,3008`); its pattern finds them. Same-named calls in `web_spare.rs` are test-model code.
- **CompositorBirth:** the table's callers are correct, but its published `Compositor::new\(`/`spare_parent\(` patterns do not find the minting-function callers `Runtime::create` in `resumed` (`main.rs:62729`), `Runtime::open_window` in `open_pending_window` (`60401`), or `make_spare_web_controller` in `warm_web_engine` (`runtime/web.rs:57`). Thus the patterns do not establish the claimed complete caller inventory. Likewise, `SessionWriteWait` omits a `wait_for_landing` search and `PaneRetirementWait` omits `settle_quit`.

Confirmed one NUL byte in `bt-platform/src/lib.rs`; ordinary ripgrep's binary handling is insufficient. All searches above used `-a`.

**One change:** amend (e)2's contract to specify `pub(crate) commit_now`, with exactly the six named internal call sites plus the token-taking `commit` forwarding call allowed, and a reproducible caller-search closure: search every named minting function and each additional caller level printed in the table, resolving homonyms and filtering tests. This completes the existing inventory without adding another door.

## P4 — not met

The eight added first-party edges match the base bodies: Windows `DirWatch::drop → close` ×3 (`lib.rs:10788–10790`), with `CloseHandle` ×1 in `close` (`10973`); `flush_sink → Queue::close` ×1 (`trace_sink.rs:228`); `OutputRing::close → state` and `InputRing::close → state` (`bt-pty/src/lib.rs:1355,1490`); and shutdown's two `PtyError::from` conversions (`2125,2141`, backed by `Io(#[from] std::io::Error)`). `Child::try_wait` is correctly ×2 (`2121,2137`), the second inside the `reap_within` closure.

Read all twelve rows and their named helper bodies. Their Windows/macOS chains, ordered first-party edges and stated vocabulary counts match, including both engine sleep/join pairs, trace's two sleeps, and PTY finish-before-shutdown. Those platform controls are green by inspection under the written vocabulary.

But the closed rule says: “Every callee must itself be a pinned body in the table”. On non-Windows/non-macOS, `VideoSeat::shutdown` resolves to `video::engine::no_player::Engine::shutdown` (`bt-platform/src/video_portable.rs:373,473`), not either pinned engine body. Its body is `match self._never {}`: no edges or effects, but it is still an unlisted first-party callee. `VideoSeat` and therefore `VideoSeats` are not green across the full source universe. The uninhabited engine prevents execution; the source rule grants no reachability exemption.

**One change:** pin that portable `Engine::shutdown` body with zero edges/effects and identify it as the third platform target of `VideoSeat::shutdown`. Keep the closed rule and five red mutations unchanged.

## Dispatch

- **A1a — no:** the stated P3 confirmation gate remains unmet; finish the narrow (e)2 correction first. No additional registry door was found.
- **A1d's table as its brief — no:** settle helper visibility/caller fencing and complete the reproducible search contract.
- **A1e — no:** add the portable pinned helper so every platform's today-chain is a green control.
