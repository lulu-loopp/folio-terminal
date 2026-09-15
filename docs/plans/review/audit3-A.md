STATUS COMPLETE

## A-1 — high — TMPDIR splits the single-writer lock

Location: `crates/bt-platform/src/instance.rs:501` at `6a414963`; runtime selection at `crates/bt-platform/src/instance.rs:332`.

```rust
let runtime = prepare_runtime_directory().ok()?;
let tag = directory_tag(directory);
let lock = std::fs::OpenOptions::new()
    .create(true)
    .truncate(false)
    .write(true)
    .mode(0o600)
    .open(lock_path_in(&runtime, &tag))
    .ok()?;
```

```rust
let base = std::env::var_os("TMPDIR")
    .filter(|value| !value.is_empty())
    .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
```

Trigger: launch two Folio processes as the same user against the same data directory, one with the normal macOS TMPDIR and another with TMPDIR=/tmp (or unset).

Consequence: the processes flock different files and both return `Some(DataDirectoryClaim)`. Both become storage writers, allowing their settings/session snapshots to overwrite each other. Their launch sockets also differ, so the second launch cannot hand over to the first. `crates/bt-app/src/persist.rs:223` uses this claim as writer authorization.

Smallest correct fix: put the ownership lock at a stable location derived from the canonical data directory, independent of TMPDIR. Endpoint discovery must also use a stable per-user location so launches with different environments reach the same owner.

## A-2 — medium — Foreign Carbon hotkeys are consumed

Location: `crates/bt-platform/src/hotkey.rs:1639` at `6a414963`; the parameter-error branch at line 1635 has the same defect.

```rust
if named.signature == SUMMON_SIGNATURE && live {
    super::wake_the_summon();
}
NO_ERR
```

Trigger: Carbon dispatches `kEventHotKeyPressed` for another handler's registration through Folio's application-target handler first. The event has a different signature or an ID outside the live Folio claims.

Consequence: Folio takes no action but terminates propagation by returning noErr, suppressing the other handler's hotkey. An unreadable direct-object parameter is also consumed. Carbon requires an unhandled event to return eventNotHandledErr so dispatch can continue. [Apple's Carbon Event Manager Programming Guide, archived copy](https://leopard-adc.pepas.com/documentation/Carbon/Conceptual/Carbon_Event_Manager/CarbonEvents.pdf).

Smallest correct fix: return `eventNotHandledErr` (-9874) for parameter-read failures and nonmatching/non-live registrations; return noErr only after handling a live Folio hotkey.

## A-3 — high — An obsolete WebKit callback certifies a new controller without resource rules

Location: `crates/bt-platform/src/macos_webview.rs:505` at `6a414963`; invalidation at line 440 and replacement at line 1208.

```rust
unsafe { controller.removeAllContentRuleLists() };
unsafe { controller.addContentRuleList(&list) };
*again.attached.borrow_mut() = Some((compiled_from.clone(), list));
*again.refused.borrow_mut() = None;
again.stands.set(true);
if again.settled() {
    again.answer_what_waited();
```

Trigger: controller generation 1 starts asynchronous rule compilation; compilation remains outstanding past the application's 10-second startup deadline; the user retries. `close()` calls `let_go()`, but neither invalidates the callback nor clears its in-flight status. Generation 2 creates a new WKUserContentController and updates `door.page`/`door.owed`; its `compile()` returns because `compiling` is still true. Generation 1's successful callback then arrives, with the same requested rules. The retry path is `crates/bt-app/src/webhost.rs:2641`; the deadline is at line 1424.

Consequence: the callback installs the list on its captured generation-1 controller, sets the shared `stands` flag, and reports success for the current generation-2 request. `install()` reports resource protection from that flag at line 1372, and the replacement view loads with no WKContentRuleList. For a local HTML preview, scripts and subresources can make network requests that the file-seat rules were supposed to block; the navigation delegate does not gate those requests.

Smallest correct fix: associate compilation and installed-rule state with a controller generation. Invalidate that generation on close/replacement, ignore obsolete completions without changing current state, and install/verify the rules on the current controller before reporting it ready. A replacement controller must not inherit an older controller's `attached`/`stands` state.

## A-4 — high — A symlink to an application bypasses the program-launch refusal

Location: `crates/bt-platform/src/handoff.rs:685` at `6a414963`; caller at line 752.

```rust
if metadata.is_dir() {
    return path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("app"));
}
```

```rust
let metadata = std::fs::metadata(path).map_err(|error| format!("{path:?}: {error}"))?;
if opening_it_would_run_it(path, &metadata) {
    return Err(PROGRAM_REFUSED.to_owned());
}
let url = file_url(path, metadata.is_dir())?;
hand_over(&url, &path.to_string_lossy())
```

Trigger: a local path named `notes` is a symbolic link to a runnable `Payload.app` directory, and the user invokes the files menu's Open With action on `notes`. That action passes the path to this function (`crates/bt-app/src/main.rs:76710`, then line 80723).

Consequence: `metadata()` follows the link and identifies a directory, but the extension check examines the link name, which has no `.app` suffix. The check passes and NSWorkspace opens the linked application, executing it under the user's account despite this entry point's explicit program refusal. No filesystem race is required.

Smallest correct fix: resolve the path first, obtain metadata and classify the resolved target, and pass that same resolved target to NSWorkspace. A symlink to a `.app` must receive `PROGRAM_REFUSED`, just like a direct application path.

## A-5 — medium — The AVFoundation worker never drains autoreleased objects

Location: `crates/bt-platform/src/macos_player.rs:251` at `6a414963`; unpooled pump at line 650.

```rust
.spawn(move || {
    run(&url, &shared, &commands, &inbox);
```

```rust
loop {
    let state = self.publish_state(shared);
```

Trigger: open a media preview and keep its `folio-video-engine` thread alive while playing or seeking. The Rust thread enters AVFoundation construction and repeatedly calls Cocoa from `publish_state`, `apply`, and `take_frame`, without an autorelease pool anywhere in the worker.

Consequence: autoreleased Foundation/AVFoundation temporaries on this thread have no periodically drained pool, so they accumulate during the preview's lifetime. The main thread's AppKit pool cannot drain another thread's objects, and retaining the returned player/item objects does not release framework-created temporaries. Apple's memory-management contract explicitly requires pools for secondary Cocoa threads and periodic draining for long-lived loops. [Apple: Using Autorelease Pool Blocks](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/MemoryMgmt/Articles/mmAutoreleasePools.html).

Smallest correct fix: create an autorelease pool around worker initialization/teardown and drain a nested pool on every pump iteration, keeping cross-iteration objects in `Retained` ownership. A single pool around the entire lifetime would still let playback temporaries accumulate.
