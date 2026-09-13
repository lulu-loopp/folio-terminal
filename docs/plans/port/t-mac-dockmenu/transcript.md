# T-MAC-DOCKMENU — the trip

macOS 26.6.2 (25G83), Apple M4, 2026-09-13. A debug build of
`feature/macos-dock-menu` at `5cc88d1b` (this branch merged with `main` at
`e1acfa19`), in a worktree of its own at `~/folio-port/wt/dockmenu` so that no
other ticket's checkout under `~/folio-port` was touched;
`CARGO_TARGET_DIR=~/folio-port/target-dock`, `-j 2`, `nice -n 10`.

Run by `dock-menu-proof.sh` beside this file. Nothing outside `~/folio-port` was
written apart from the `~/Library` shells a bundle identifier leaves behind,
which the script removes; no running Folio was looked at or signalled, and the
two `.app` bundles the owner keeps at `~/folio-port/` were not touched.

## What was driven, and what could not be

The Dock tile itself cannot be right-clicked by an agent: it needs `System
Events`, and therefore Accessibility *and* Automation, behind a TCC prompt an
ssh session cannot reach (X-4 measured it, DESIGN §13.21). What the script
drives instead is the whole of what the Dock does when a tile is right-clicked —
send the application delegate `applicationDockMenu:`, then send the chosen
item's own action through `-[NSApplication sendAction:to:from:]` — asked of the
probe's own delegate and its own items, with no window server gesture at all.
What a human should look for is in DESIGN §13.50 ⑥.

## `cargo test --locked -p bt-platform -j 2`

Green on the second run. The first run had one failure, and it is this ticket's
own test rather than its code:

```text
---- macos_menu::tests::the_dock_menu_is_handed_over_autoreleased stdout ----
panicked at crates/bt-platform/src/macos_menu.rs:624:9:
a menu whose retain count is still this module's leaks once per right-click
test result: FAILED. 247 passed; 1 failed
```

The case reads this module's source text for the spelling that hands the menu
over to AppKit, and its two needles were written as plain literals — which are
in the case itself, one of them twice over because the `MUTATION:` line above it
named the wrong spelling in full. So the first assertion passed on its own text
however the menu was actually returned, and the second failed on its own text
however careful the code was. Both needles are spelled through `concat!` now and
the mutation line no longer spells its subject in full, which is `main.rs`'
`launch_landing_tests`' own device for a module that reads the file it is
written in.

## The bundle proof

A throwaway ad-hoc-signed `FolioDockMenu.app` with an identifier of its own
(`io.github.lulu-loopp.folio.dockmenu`) and an isolated `HOME` exported by a
`CFBundleExecutable` wrapper script, started with `open` because an ssh session
cannot reach the window server a winit event loop needs.

```text
[      0ms] pid=98769 bundle=/Users/…/out-dockmenu/FolioDockMenu.app
[      0ms] MEASURED class_getInstanceMethod(WinitApplicationDelegate, applicationDockMenu:) before the event loop: None
[    140ms] MEASURED the same reading after EventLoop::new: Some(false)
[    140ms] PASS winit does not implement applicationDockMenu: itself
[    140ms] PASS the application delegate installed onto winit's own class
[    424ms] PASS the delegate AppKit holds answers applicationDockMenu:
[    424ms] PASS and it still answers M3-1's four
[    426ms] PASS the menu bar installed
[    426ms] MEASURED the Dock menu carries ["New window", "New tab"]
[    426ms] PASS the Dock menu is the plan's rows, in the plan's order
[    426ms] PASS the Dock menu decides its own rows rather than asking a responder chain
[    426ms] PASS every Dock row is in force
[    426ms] PASS no Dock row prints a key equivalent
[    461ms] MEASURED ⌘N on the bar: [(Bar, Verb("new-window"))]
[    461ms] PASS the bar's own row sends one choice, from the bar
[    461ms] MEASURED the first Dock row: [(Dock, Verb("new-window"))]
[    461ms] PASS the first Dock row sends the same verb, from the Dock
[    461ms] MEASURED the second Dock row: [(Dock, Verb("new-tab"))]
[    461ms] PASS the second Dock row is the new tab
[    461ms] PASS the bar took the second language
[    461ms] MEASURED after the language switch: ["新建窗口", "新建标签"]
[    461ms] PASS a refresh reaches the Dock tile with no door of its own
[    470ms] PASS a plan with no Dock rows answers nil, and AppKit's own menu is untouched
[    471ms] 0 failed
[    471ms] ALL_DONE
[    484ms] run_app returned Ok
```

The two readings worth keeping out of that list:

* `None` then `Some(false)` — `WinitApplicationDelegate` is not in the runtime
  at all until `EventLoop::new` builds the loop, and when it is, it does **not**
  implement `applicationDockMenu:`. X-4's measurement for its four, made again
  for the fifth: nothing of winit's is displaced and `NSApp.delegate` stays
  winit's own object;
* `[(Bar, Verb("new-window"))]` beside `[(Dock, Verb("new-window"))]` — one
  verb, one inbox, one field apart. That field is what the landing reads to
  decide which window the press is about.
