# X-4 — the AppKit application delegate bridge

*2026-09-12. Ticket X-4 of `docs/plans/port/macos-plan-2026-09-12.md`, branch
`probe/macos-x4`. Run on the Mac mini in `~/folio-port/wt/x4` at `origin/main`
`5453c83`, under `docs/plans/port/mac-mini-venue.md`. Crate beside this file
under `docs/plans/port/probe-x4/`.*

## The route, which is the finding

**winit 0.30.13's own documentation is wrong about the thing this ticket exists
to settle.** `src/platform/macos.rs:29` says "Winit guarantees that it will not
register an application delegate, so the solution is to register your own". It
does register one: `platform_impl/macos/event_loop.rs:240` calls
`app.setDelegate(...)` with a private `WinitApplicationDelegate`, and
`app_state.rs`'s `ApplicationDelegate::get` reads `NSApp.delegate` back, checks
`is_kind_of` and **panics** on anything else — from the CFRunLoop observers
(`observer.rs:62`, `:84`), every turn of the loop. The documented route takes
winit down on the first iteration, and a forwarding proxy fails the same check.

The route that works, used throughout:

1. **Add the selectors winit does not implement to the class winit already
   registered.** After `EventLoop::new`, look the class up by name
   (`objc2::runtime::AnyClass::get(c"WinitApplicationDelegate")`) and
   `class_addMethod` four implementations onto it:
   `applicationShouldHandleReopen:hasVisibleWindows:`,
   `applicationShouldTerminate:`,
   `applicationShouldTerminateAfterLastWindowClosed:` and
   `application:openURLs:`. For each, `class_getInstanceMethod` returned null and
   `class_addMethod` true: nothing of winit's was displaced, and `NSApp.delegate`
   stays winit's own object, so its assertion never fires.
2. **Hand winit's delegate back to `setDelegate:` once**, immediately after,
   because AppKit caches which delegate methods exist when it is called and winit
   called it before these four existed. Not needed on macOS 26.6 — run `r1`
   skipped it and every event still arrived — but it costs one message.
3. **Services never touch the delegate**: a provider object is registered with
   `-[NSApplication setServicesProvider:]`, and AppKit calls the method named by
   `NSMessage` on it.

Every `ApplicationHandler` event kept arriving: by the end of the full run
`about_to_wait` had fired 829 times, `window_event` 42 and `user_event` 9.

## The five events

| Event | Result | Evidence |
|---|---|---|
| **reopen**, a second `open` of the running app | **PASS** | one `app` pid per run, so a second `open` starts no second executable; four arrivals, each routed within 2 ms |
| **reopen with no window open** (Q10) | **PASS** | `hasVisibleWindows` NO, and the bridge opened window #2 |
| **Services**, `public.file-url` from a provider object | **PASS** | three folders in one delivery — spaces, CJK with an em dash, plain — all decoded intact, warm and cold |
| **termination**, Cancel and Later | **PASS**, with a rule below | Cancel returned and the app lived; Later returned and the deferred answer landed 1.35 s later |
| **last window closed** | **PASS** | answered NO; the app sat with zero windows and reopened on demand |
| **a Dock click with hidden windows** | **NOT-CHECKABLE**, covered | see below |

**The rule, and the only FAIL-shaped thing found.** `-[NSApplication
terminate:]` must not be called from inside a winit `ApplicationHandler`
callback. `NSTerminateLater` makes AppKit spin a nested run loop until
`replyToApplicationShouldTerminate:` arrives; winit's observers are in
`kCFRunLoopCommonModes` and do run inside it, but if the call came from a winit
callback its handler `RefCell` is already borrowed and `event_handler.rs:135`
answers re-entry with a panic, once per turn, forever — run `r1` was a live
process at 100% CPU, ended by its pid. From the main dispatch queue instead,
which is what Command-Q and the Dock's Quit do, it is clean. Second measurement,
for M3-1: **that deferred loop does not drain the main dispatch queue** (run
`r2`: neither the queued answer nor a later `dispatch_async` ran, and the app
hung). The answer must come from winit's handler, which *is* driven there; run
`r3` did that and it unwound in 1 ms. Any AppKit call spinning a nested loop —
`NSAlert`, `NSOpenPanel`, M2-3's row — is unsafe inside `ApplicationHandler`.

**`hasVisibleWindows` is not the signal it reads as.** Minimised: YES. Hidden
(`-[NSApplication hide:]`): YES. Only zero windows reported NO. The bridge must
decide from its own window list; the flag is advisory.

**The Dock click** needs `System Events`, so Accessibility *and* Automation;
`osascript` from ssh returned `-1712`, an Apple Event timeout behind a TCC prompt
no agent could answer. **NOT-CHECKABLE**, and covered by reopen, which a Dock
click delivers identically — reached here through `open` with the app hidden and
with it minimised.

## What `bt-platform` should expose

**A channel, not a trait.** The delegate methods are C functions on a class
Folio does not own; they cannot hold a `&mut dyn` anything, and
`applicationShouldTerminate:` must return a value from a C stack. So:

```rust
pub struct AppEvent { pub origin: AppOrigin, pub kind: AppEventKind }
pub enum AppOrigin { Reopen, Services, Termination, LastWindowClosed }
pub enum AppEventKind {
    OpenWindow { had_visible_windows: bool },
    OpenPaths(Vec<PathBuf>),
    TerminationRequested,
    LastWindowClosed,
}
```

delivered on winit's own user-event channel —
`EventLoop::<AppEvent>::with_user_event()`, `EventLoopProxy::send_event` from the
C function, `ApplicationHandler::user_event` on the other side. Non-blocking,
same main thread, and it buffers: in the cold-Services run the Service arrived at
t=222 ms, *before* `resumed` at 247 ms and before any window existed, and reached
the handler at 551 ms. M4-9 and M3-5 both depend on that.

Termination needs a second, synchronous face, because AppKit wants its answer
before the event can be routed: a `TerminationAnswer` the bridge consults on the
delegate's stack (`Cancel` / `Now` / `Defer`) plus `resolve_deferred(bool)` from
`about_to_wait`. Folio's simplest correct shape is `Cancel`, then re-issue
`terminate:` when the save finishes.

winit 0.30.13 also offers `EventLoopBuilderExtMacOS::with_activation_policy`,
`with_default_menu`, `with_activate_ignoring_other_apps` and
`EventLoopExtMacOS::hide_application`, but **no** `set_dock_visibility`: Dock
presence is `LSUIElement` plus the activation policy.

## Services registration evidence

One `NSServices` entry in the generated `Info.plist`: `NSMenuItem/default`
"Open in Folio Probe", `NSMessage` `openInFolioProbe`, `NSPortName` `ProbeX4`,
`NSSendTypes` `public.file-url` and `NSFilenamesPboardType`. After
`lsregister -f` and `pbs -flush`, `pbs -dump_pboard` listed it under
`NSBundleIdentifier = io.github.lulu-loopp.folio.probe-x4`. Headless invocation
needs no `automator`: `NSPerformService` with a pasteboard of file URLs is
Finder's own path. Cold delivery needs a *sender with its own bundle identifier*;
sent from the probe's own executable, LaunchServices answered "never opened its
Services port before the timeout" — a self-collision, not a defect.

## Numbers, processes, and what the owner must grant

Clean build of the probe and its graph (winit 0.30.13, objc2 0.6.4,
objc2-app-kit 0.3.2), `nice -n 10 … -j 6`: **21.16 s wall, 65.12 s user, peak RSS
1.878 GB**, target 619 MB, binary 1,619,896 bytes. Rebuilding the binary alone:
1.14 s. The scratch target was deleted, the shared one untouched; free space
ended at 57 GiB.

**Created, and how each ended.** Four probe app processes (46369, 46575, 46746,
47043) and four senders (46408, 46784, 46828, 47041). 46746 and 47043 quit on
their own `NSTerminateNow` and every sender exited on its own; 46369 and 46575
were the two deadlocks above, ended by `kill` on the pid read from the probe's
own first log line. Nothing was matched by name or title. Two ad-hoc signed
bundles under `~/folio-port/wt/x4/out/`, `ProbeX4.app` and a `SenderX4.app` with
its own identifier, were registered with `lsregister -f` and both
**unregistered with `lsregister -u`**; `pbs -dump_pboard` and `lsregister -dump`
name neither now, and neither went near `~/Applications`. Logs:
`~/folio-port/logs/x4-r{1,3,5}.log`.

**For the owner.** A TCC prompt for Apple Events / Accessibility appeared during
the Dock-click attempt and timed out unanswered. Nothing here needs that grant,
but Q12's macOS `ui-probe` and Q2's global shortcut will — the second probe in a
row, after X-1's Screen Recording, to stop at a dialog an agent cannot reach.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VNAfpT6VRgLihW5Fp3EU74
