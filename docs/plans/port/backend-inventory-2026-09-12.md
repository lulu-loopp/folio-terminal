# M1-2 — the `bt-platform` backend inventory

*2026-09-12. Ticket M1-2 of `docs/plans/port/macos-plan-2026-09-12.md` (§2 M1,
§4.3, §4.4, §7.1). Branch `docs/macos-backend-inventory`. Measured against
`main` at `0670be9`. Windows workstation; the one cargo run is
`cargo check -p bt-platform --target aarch64-apple-darwin -j 2`,
`CARGO_INCREMENTAL=0`, in a worktree of its own. No product code changed.*

This replaces the spike's Appendix B. That appendix counted **79 distinct items
missing from `bt_platform`** at `05bf018` by reading `bt-app`'s compiler errors
on a Mac; it was a reading of what the compiler happened to reach before it gave
up, and it is now eleven weeks and several features stale. This counts the
**public surface of `bt-platform` itself**, item by item, and then says which of
those items `bt-app` actually names. The two numbers are different on purpose and
the gap between them is the first result below.

---

## 1. Method, and what counts as one item

**One row per named public item at module level** — a function, a type, a
constant, a type alias, or a module. A type's methods are described inside its
own row rather than given rows of their own, because a `Compositor` with eleven
methods is one thing to port and eleven things to port it *into*; splitting it
would inflate the count without adding a decision. Items re-exported at the crate
root under the same name as the module item they come from (`handoff`'s nine, the
priority trio's portable arms) are counted **once**, at their definition.

`windows_impl` is a private module whose names are re-exported at the crate root,
so the crate root is where `bt-app` sees them and where they are listed here.
`webview` is likewise a private module with a public re-export list — §4.3 of the
plan calls it `#[cfg(windows)] pub mod webview`, which it is not; it is
`#[cfg(windows)] mod webview` plus a 27-name `pub use`. That matters for the
ticket, because the "module bodies become `mod win` / `mod mac`" shape §4.3
proposes has to be applied to a re-export list here, not to a module declaration.

Four columns need their vocabulary stated once.

**Windows type in the signature.** Yes only when a Windows type or a Win32 value
is visible to a caller: `HWND`, anything under `windows::`, a `HANDLE`, a Win32
virtual-key code, a `NonZeroIsize` that *is* an `HWND`, or a registry/COM string
shape. A `NonZeroIsize` window handle counts, because it is the leak §4.4 ② is
about even though the spelling is portable.

**Ownership / thread.** `window` = must be called on the thread that owns the
window handle it is given. `any` = free. `handle` = the value owns a native
object and releases it on `Drop`. `own thread` = it starts and joins a thread of
its own.

**Caller on failure.** `?` = propagated with `anyhow::Context` and usually fatal
to the startup it sits in; `toast` = surfaced to the reader; `log` = one
`eprintln!` and carry on; `ignore` = `let _ =` or `.ok()`; `fallback` = a named
substitute value; `—` = cannot fail.

**Class**, the five of §4.4 plus one the plan does not have:

| Code | Class | Meaning |
|---|---|---|
| **P** | portable policy | pure, compiles off Windows today, macOS calls it unchanged |
| **P°** | portable policy, unreachable | pure and compiled, but **no macOS caller exists** — the body is a Windows fact expressed in portable Rust |
| **R** | real macOS implementation | a macOS backend has to be written |
| **X** | invocation-time refusal | present everywhere, answers "no" with a reason |
| **N** | harmless lifecycle no-op | present everywhere, does nothing, and nothing downstream notices |
| **A** | compile-time absence | not present off Windows, deliberately |

**P° is this ticket's finding about §4.4 and is argued in §6.** The plan's five
classes are about what a *backend* does with an item. They have no name for an
item that is already portable, already green on `aarch64-apple-darwin`, and dead
the moment the product runs on a Mac — and there are thirty-eight of those, more
than the refusal and no-op buckets the plan does name, put together and tripled.

---

## 2. The inventory

### 2.1 Crate root — portable policy, no gate

Every row here compiles for `aarch64-apple-darwin` today. None of them carries a
dependency; `bt-platform`'s manifest opens with
`[target.'cfg(windows)'.dependencies]` and has no other table.

| Item | Signature as written | Win type | Thread | Caller on failure | Class | Reason |
|---|---|---|---|---|---|---|
| `WindowRect` | `struct { left, top, right, bottom: i32 }` | no | any | — | **P** | a rectangle; `NSRect` converts at the edge |
| `WheelScrollAmount` | `enum { Lines(u32), Page }` | no | any | — | **P** | the same two answers AppKit gives |
| `CustomFrameMetrics` | `struct { width, height, title_bar_height, tab_strip_right_px, caption_button_width, caption_button_count, resize_border: i32, resizable: bool }` | no | any | — | **P** | M3-3 feeds it traffic-light geometry instead |
| `CustomFrameGeometry` | `struct { title_bar_logical_px, caption_button_logical_px: u32 }` | no | any | — | **P** | two logical numbers handed in by `bt-render` |
| `CustomFrameHit` | `enum { Client, Caption, Left … BottomRight }` | no | any | — | **P** | deliberately written without Win32 constants |
| `custom_frame_hit_test` | `fn(CustomFrameMetrics, i32, i32) -> CustomFrameHit` | no | any | — | **P** | pure arithmetic, already passing off Windows |
| `PendingWindowPos` | `struct { x, y, cx, cy: i32, no_move, no_size: bool }` | no | any | — | **P°** | the pure half of `WM_WINDOWPOSCHANGING`; no AppKit message carries it |
| `hold_pending_pos_to` | `fn(WindowRect, PendingWindowPos) -> Option<PendingWindowPos>` | no | any | — | **P°** | same message, same absence |
| `logical_px_for_dpi` | `fn(u32, u32) -> i32` | no | any | — | **P** | DPI arithmetic; backing scale 2.0 is a value, not a different rule |
| `composition_visual_offset` | `fn(i32, i32) -> (f32, f32)` | no | any | — | **P** | an `i32`→`f32` pair; `CALayer` positions take the same |
| `GroundBand` | `struct { x, y, width, height: i32 }` + `is_empty` | no | any | — | **P** | the skirt's rectangle |
| `window_skirt` | `fn((u32,u32), (u32,u32)) -> [GroundBand; 2]` | no | any | — | **P** | pure; M4-1 places the two bands as sublayers |
| `PageVisual` | `struct { tab: u64, seat: u64 }` | no | any | — | **P** | an address in the tab tree |
| `VisualLayer` | `enum { Bottom, Top }` + `insert_above_with_null_reference` | no | any | — | **P°** | the enum survives; the `bool` it produces is `IDCompositionVisual::AddVisual`'s `insertAbove` and means nothing to `CALayer` |
| `INSERT_ABOVE_REFERENCE` | `const bool = true` | no | any | — | **A** | literally an `AddVisual` argument |
| `file_uri_to_path` | `fn(&str) -> Option<PathBuf>` | no | any | `fallback` | **P** | a `file:` URI parser; drive-letter handling is inert on a Unix path |
| `ContextMenuShape` | `struct { label, icon, command: String }` | shape | any | — | **P°** | a registry verb's three values |
| `CONTEXT_MENU_VERB_KEY` | `const &str = "Folio"` | shape | any | — | **A** | a registry key name |
| `CONTEXT_MENU_CLASSES` | `const &str = r"Software\Classes"` | shape | any | — | **A** | a registry path |
| `CONTEXT_MENU_TREES` | `const [&str; 2]` | shape | any | — | **A** | two registry subtrees |
| `context_menu_shape` | `fn(&Path, &str) -> ContextMenuShape` | shape | any | — | **P°** | composes the verb's command line |
| `changing_explorer_menu` | `fn(impl FnOnce() -> Result<T,String>, impl FnOnce()) -> Result<T,String>` | no | any | propagates | **P°** | write-then-announce; `NSServices` is declarative and has nothing to announce |
| `NOTIFICATION_AUMID` | `const &str = "Folio.Terminal"` | shape | any | — | **A** | an AppUserModelID |
| `notification_aumid_key` | `fn() -> String` | shape | any | — | **A** | `HKCU\Software\Classes\AppUserModelId\…` |
| `NOTIFICATION_DISPLAY_NAME` | `const &str = "Folio"` | no | any | — | **P°** | the name in that registration |
| `toast_xml` | `fn(&str, &str, &str) -> String` | shape | any | — | **P°** | a WinRT toast document; `UNMutableNotificationContent` takes fields |
| `ContextMenuState` | `enum { Absent, Current, Stale }` | no | any | — | **P°** | a verdict about a registry tree |
| `ContextMenuTree` | `enum { Absent, Broken, Written(ContextMenuShape) }` + `shape` | shape | any | — | **P°** | as above |
| `context_menu_verdict` | `fn(&[ContextMenuTree], &ContextMenuShape) -> ContextMenuState` | shape | any | — | **P°** | as above |
| `context_menu_command_exe` | `fn(&str) -> Option<&str>` | shape | any | `fallback` | **P°** | splits a registry command line |
| `RegisteredExe` | `struct { on_disk, ours: bool }` | no | any | — | **P°** | a fact about another build's registration |
| `explorer_reassert_wanted` | `fn(impl IntoIterator<Item = RegisteredExe>) -> bool` | no | any | — | **P°** | the shared rewrite rule |
| `context_menu_reassert_wanted` | `fn(&[ContextMenuTree], &ContextMenuShape, impl Fn(&Path) -> bool) -> bool` | shape | any | — | **P°** | as above |
| `IMAGE_FILE_EXTENSIONS` | `const [&str; 6]` | no | any | — | **P** | the picture formats the decoder honours |
| `image_file_filter_spec` | `fn() -> String` | no | any | — | **P** | a `*.png;*.jpg` string; `NSOpenPanel` takes types, so M2-3 reads the const instead |
| `MonospaceFamily` | `struct { name: String, files: Vec<PathBuf> }` | no | any | — | **P** | a family and its files; `CTFontCollection` fills the same shape |
| `FONT_SETTINGS_URI` | `const &str = "ms-settings:fonts"` | no | any | — | **A** | a Windows settings URI |
| `fonts_folder` | `fn() -> PathBuf` | no | any | `fallback` | **R** | `%WINDIR%\Fonts`; macOS wants `/System/Library/Fonts` and `~/Library/Fonts` |
| `quiet_command` | `fn(impl AsRef<OsStr>) -> Command` | no | any | — | **P** | already portable — `CREATE_NO_WINDOW` is applied under a gate inside. M2-2 re-verifies only |
| `DEFAULT_MONOSPACE_FAMILY` | `const &str = "Consolas"` | no | any | — | **R** | macOS's default is SF Mono or Menlo; M2-4 |
| `order_monospace_families` | `fn(Vec<MonospaceFamily>) -> Vec<MonospaceFamily>` | no | any | — | **P** | the ordering policy, already passing off Windows |
| `ThreadPriority` | `enum` (three bands) | no | any | — | **P** | the vocabulary the portable arm answers in |
| `TaskbarProgressState` | `enum { Normal, Error, Paused, … }` | no | any | — | **P** | the Dock tile can carry the same three |
| `TaskbarProgress` | `struct` + `const CLEARED` | no | any | — | **P** | as above |

### 2.2 Crate root — Windows-gated leaves

| Item | Signature | Win type | Thread | Caller on failure | Class | Reason |
|---|---|---|---|---|---|---|
| `process_image_path` | `#[cfg(windows)] fn(u32) -> Option<PathBuf>` | no | any | — | **R** | **no `bt-app` caller**; peer verification inside the crate. M3-5 needs the macOS twin for the launch socket's peer check |
| `quiet_command_named` | `#[cfg(windows)] fn(&Path) -> Option<Command>` | shape | any | `?`/`None` | **R** | resolves a name on `PATH` and applies the quiet flag; three `bt-app` callers, all already inside `#[cfg(windows)]` |

### 2.3 `windows_impl` — the 73 re-exported names

These are the port. Everything here is `#[cfg(windows)]` today and every one of
them is a name `bt-app` can write without a gate, which is §4.3's whole design.

**Window handles and geometry.**

| Item | Signature | Win type | Thread | Caller on failure | Class | Reason |
|---|---|---|---|---|---|---|
| `get_window_rect` | `fn(NonZeroIsize) -> Result<WindowRect, String>` | hwnd | window | `.ok()` / `?` | **R** | `NSWindow.frame` |
| `set_window_outer_rect` | `fn(NonZeroIsize, WindowRect) -> Result<(), String>` | hwnd | window | `?` at startup | **R** | `setFrame:display:`; M1-3 |
| `stand_window_at` | `fn(NonZeroIsize, WindowRect) -> Result<(), String>` | hwnd | window | `log` | **R** | the quake drop; M1-3 |
| `get_work_area` | `fn(NonZeroIsize) -> Result<WindowRect, String>` | hwnd | window | early return | **R** | `NSScreen.visibleFrame` |
| `work_area_at` | `fn(i32, i32) -> Result<WindowRect, String>` | no | any | `fallback` | **R** | the screen under a point |
| `virtual_screen_rect` | `fn() -> WindowRect` | no | any | — (fallback) | **R** | the union of `NSScreen.screens` |
| `dpi_at` | `fn(i32, i32) -> u32` | no | any | — | **R** | backing scale × 96 |
| `get_dpi_for_window` | `fn(NonZeroIsize) -> Result<u32, String>` | hwnd | window | `?` | **R** | `NSWindow.backingScaleFactor` |
| `monitor_id_at` | `fn(i32, i32) -> Option<String>` | no | any | early return | **R** | display identity; M3-4 and R5's case-folding argument |
| `pointer_position` | `fn() -> Option<(i32, i32)>` | no | any | `fallback` | **R** | `NSEvent.mouseLocation`, flipped |
| `top_level_window_at` | `fn(i32, i32) -> Option<NonZeroIsize>` | hwnd | any | early return | **R** | `CGWindowListCreateImage`'s neighbour, or refuse |
| `thread_mouse_capture` | `fn() -> Option<NonZeroIsize>` | hwnd | window | `None` | **X** | AppKit has no per-thread capture; the reader is a diagnostic field |
| `is_window_minimized` | `fn(NonZeroIsize) -> bool` | hwnd | any | — | **R** | `isMiniaturized` |
| `is_window_cloaked` | `fn(NonZeroIsize) -> bool` | hwnd | any | — | **X** | cloaking is a DWM notion; `isVisible`/occlusion is the nearest and is not the same fact |
| `cloaked_from_attribute` | `fn(Option<u32>) -> bool` | value | any | — | **P°** | pure reading of a DWM attribute word |
| `window_is_exposed` | `fn(NonZeroIsize) -> bool` | hwnd | window | — | **R** | `NSWindowOcclusionState`; the notification gate reads it |
| `exposure_probe_points` | `fn(WindowRect) -> [(i32,i32); 3]` | no | any | — | **P** | **no `bt-app` caller**; pure, reusable by M4-6 |
| `exposed_from_probe` | `fn(NonZeroIsize, Option<WindowRect>, impl FnMut(i32,i32) -> Option<NonZeroIsize>) -> bool` | hwnd | any | — | **P** | **no `bt-app` caller**; pure policy over an injected probe |
| `set_window_topmost` | `fn(NonZeroIsize, bool) -> Result<(), String>` | hwnd | window | `log` | **R** | `NSWindow.level` |
| `set_window_dark_mode` | `fn(NonZeroIsize, bool) -> Result<(), String>` | hwnd | window | `log` | **R** | `appearance = NSAppearanceNameDarkAqua` |
| `set_system_backdrop` | `fn(NonZeroIsize, bool) -> Result<(), String>` | hwnd | window | `log` | **R** | `NSVisualEffectView` |
| `system_backdrop_available` | `fn(NonZeroIsize) -> bool` | hwnd | window | — | **R** | answers the Acrylic row; on macOS the answer is a constant `true` |
| `install_window_class_background` | `fn(NonZeroIsize, Option<[u8;3]>) -> Result<(), String>` | hwnd | window | `?` at startup | **R** | the window's backing colour before the first present; `contentView.layer.backgroundColor` |
| `request_window_close` | `fn(NonZeroIsize) -> Result<(), String>` | hwnd | window | propagates | **R** | `performClose:` |
| `take_keyboard_focus` | `fn(NonZeroIsize) -> Result<(), String>` | hwnd | window | `ignore` / `log` | **R** | `makeKeyAndOrderFront:` |
| `flash_window` | `fn(NonZeroIsize)` | hwnd | window | — | **R** | `NSApp.requestUserAttention:`; M4-6 |
| `hide_every_window_of_this_process` | `fn() -> usize` | no | window | — | **R** | the panic path's last act; `NSApp.hide:` |
| `taskbar_is_auto_hidden` | `fn() -> bool` | no | any | — | **R** | the Dock's autohide preference |
| `taskbar_auto_hidden_from_state` | `fn(usize) -> bool` | value | any | — | **P°** | **no `bt-app` caller**; pure reading of an `APPBARDATA` state word |

**Handle-owning window services.**

| Item | Signature | Win type | Thread | Caller on failure | Class | Reason |
|---|---|---|---|---|---|---|
| `Compositor` | `struct`; `new(NonZeroIsize) -> Result<Self,String>`, `set_window_size`, `set_covered_size`, `skirt_covers_anything`, `gpu_visual_ptr -> *mut c_void`, `set_gpu_offset`, `attach_web_visual`, `set_page_ground_color`, `detach_web_visual`, `place_web_visual`, `hide_web_visual`, `commit` | hwnd, COM, raw ptr | window, handle | `?` at startup, `log` after | **R** | the `CALayer` tree; X-1 then M1-4/M4-1. `set_gpu_offset` has **no `bt-app` caller** |
| `CustomWindowFrame` | `struct`; `install(NonZeroIsize, CustomFrameGeometry) -> Result<Self,String>`, `in_size_move`, `set_tab_strip_right_px`, `set_min_client_size` | hwnd, subclass | window, handle | `?` at startup | **R** | `WM_NCCALCSIZE` subclass; M3-3 against the traffic lights |
| `Taskbar` | `struct`; `new(NonZeroIsize)`, `set_progress(TaskbarProgress)` | hwnd, COM | window, handle | `log`, then refuse once | **R** | Dock tile progress; M4-6 |
| `SystemSettingsWatch` | `struct`; `install(NonZeroIsize, Box<dyn Fn()>) -> Result<Self,String>` | hwnd | window, handle | `.ok()` — best effort | **R** | `NSDistributedNotificationCenter` / `NSWorkspace` notifications |
| `Notifier` | `struct`; `new(Box<dyn Fn() + Send>)`, `show(&mut, &str,&str,&str)`, `take_activations() -> Vec<String>` | WinRT | any, handle | `log`, refuse once | **R** | `UNUserNotificationCenter`; needs the bundle id, which is why §4.5 builds the bundle from M1 |
| `MathContextMenu` | `struct`; `new(NonZeroIsize)`, `request() -> Result<bool,String>`, `take_result() -> Option<Result<bool,String>>` | hwnd | window, handle | `?` at startup | **R** | a deferred `NSMenu` popup |
| `FolderPicker` | `struct`; `new(NonZeroIsize)`, `request(Option<&Path>)`, `take_result() -> Option<Result<Option<PathBuf>,String>>` | hwnd, COM | window, handle | `?` at startup | **R** | `NSOpenPanel`; M2-3 |
| `ImagePicker` | `struct`; `new(NonZeroIsize)`, `request(ShellPickKind, Option<&Path>)`, `take_result()` | hwnd, COM | window, handle | `?` at startup | **R** | `NSOpenPanel` with content types |
| `FilePickKind` | `pub type FilePickKind = ShellPickKind` (`Program` \| `Image`) | no | any | — | **P** | two cases; the alias is the public name |
| `ImeSystemCaret` | `struct`; `new(NonZeroIsize) -> Self`, `update(i32,i32) -> Result<(),String>`, `destroy(&mut)` | hwnd | window, handle | `ignore` | **R** | `NSTextInputClient.firstRectForCharacterRange`; M1-8 |
| `DirWatch` | `struct`; `start`, `start_shallow`, `start_shallow_named(&Path, impl Fn(DirChange<'_>) + Send + 'static) -> Result<Self, io::Error>` | overlapped I/O | own thread, handle | `log`, watch absent | **R** | FSEvents, **all three contracts**; M2-1 |
| `DirChange<'a>` | `enum { Named(&'a [OsString]), Unknown }` | no | watcher thread | — | **P** | the overflow/named distinction FSEvents also has |

**Clipboard, keyboard, input.**

| Item | Signature | Win type | Thread | Caller on failure | Class | Reason |
|---|---|---|---|---|---|---|
| `clipboard_text` | `fn(NonZeroIsize) -> Result<String, String>` | hwnd | window | `.ok()` / match | **R** | `NSPasteboard` needs **no window**; M1-9 drops the parameter |
| `set_clipboard_text` | `fn(NonZeroIsize, &str) -> Result<(), String>` | hwnd | window | `toast` | **R** | as above |
| `cancel_composition` | `fn(NonZeroIsize) -> bool` — *and the `#[cfg(not(windows))]` arm takes the same `NonZeroIsize`* | hwnd | window | `ignore` | **R** | `discardMarkedText`; the portable arm **already leaks the handle** (§6 ③) |
| `virtual_key_for_character` | `fn(char) -> Option<u16>` | **VK code** | any | `?` | **R** | returns a Win32 virtual key; the web chord's other half. M1-7 |
| `wheel_scroll_amount` | `fn() -> Result<WheelScrollAmount, String>` | no | any | `fallback` — 3 lines + `log` | **R** | `SystemParametersInfo` → the scroll preference |
| `client_area_animation_enabled` | `fn() -> Result<bool, String>` | no | any | `.ok()` → `Motion::from…` | **R** | Reduce Motion |
| `system_uses_light_apps` | `fn() -> Option<bool>` | registry | any | `None` | **R** | `AppleInterfaceStyle` |
| `os_ui_language` | `fn() -> String` | no | any | — | **R** | `NSLocale.preferredLanguages` |

**The console, the process, the streams.**

| Item | Signature | Win type | Thread | Caller on failure | Class | Reason |
|---|---|---|---|---|---|---|
| `adopt_parent_console` | `fn()` | console | main, early | — | **N** | a Unix process already has its parent's stdio; the call becomes nothing and nothing downstream notices |
| `detach_console` | `fn() -> bool` | console | main | `ignore` | **N** | as above |
| `std_error_is_console` | `fn() -> bool` | console | any | — | **R** | **no `bt-app` caller**; `isatty` is the Unix answer |
| `write_to_console` | `fn(&str) -> bool` | console | any | `&&` in a chain | **R** | plain `write` to fd 2 |
| `install_console_ctrl_handler` | `fn() -> bool` | console | main | `ignore` | **R** | a `SIGINT`/`SIGTERM` handler; M3-7 |
| `redirect_std_streams_to_file` | `fn(&Path) -> bool` | handles | main, once | branches the channel | **R** | `dup2`; **load-bearing** — `diagnostics.rs` picks `Log` or `Nowhere` off this `bool` |
| `silence_std_streams` | `fn()` | handles | main, once | — | **R** | `dup2` onto `/dev/null`; the other half of the same branch |
| `leave_process` | `fn(i32) -> !` | process | any | — | **R** | cannot refuse: the type has no empty answer. `_exit` after the same flush |
| `message_box` | `fn(&str, &str)` | no | any | — | **R** | `NSAlert`; M2-3. The last-resort fault reporter |
| `apartments_left` | `fn() -> u64` | COM | any | — | **A** | **no `bt-app` caller**; a COM apartment ledger, and macOS has no apartments |
| `recycle` | `fn(&Path) -> Result<bool, String>` | COM | any | `toast` | **R** | `NSFileManager.trashItemAtURL:`; M2-2 |
| `directory_folds_case` | `fn(&Path) -> bool` | no | any | — | **R** | the volume's own answer; on APFS it is per-volume and **this is R5's question**, asked here first |
| `documents_directory` | `fn() -> Option<PathBuf>` | shell | any | `None` | **A** | called only by `psreadline.rs`, which is absent |
| `file_product_version` | `fn(&Path) -> Option<String>` | version rsrc | any | `None` | **A** | reads a Win32 version block; `psreadline.rs` only |
| `monospace_font_families` | `fn() -> Vec<MonospaceFamily>` | DirectWrite | any | — | **R** | `CTFontCollection`; M2-4 |
| `current_user_registry_string` | `fn(&str, &str) -> Option<String>` | registry | any | `None` | **A** | `wsl.rs` only |
| `current_user_registry_subkeys` | `fn(&str) -> Vec<String>` | registry | any | empty | **A** | `wsl.rs` only |
| `install_context_menu` | `fn(&str, &ContextMenuShape) -> Result<(), String>` | registry | any | `log` | **A** | the classic Explorer verb |
| `remove_context_menu` | `fn(&str) -> Result<(), String>` | registry | any | `log` | **A** | as above |
| `read_context_menu` | `fn(&str) -> Vec<ContextMenuTree>` | registry | any | empty | **A** | as above |
| `announce_explorer_menu_change` | `fn()` | shell | any | — | **A** | `SHChangeNotify` |
| `set_current_thread_priority` | `fn(ThreadPriority) -> bool` | band | any | reads the `bool` | **X** | the portable arm exists and answers `false`; the macOS answer is a QoS class and is a decision, not a translation |
| `current_thread_priority` | `fn() -> Option<ThreadPriority>` | band | any | — | **X** | as above |
| `spawn_at_priority` | `fn<T>(&str, ThreadPriority, impl FnOnce() -> T + Send) -> io::Result<JoinHandle<T>>` | band | spawns | `?` | **P** | the portable arm already names the thread and runs the body; only the band is dropped |

### 2.4 `webview` — 27 re-exported names

All `#[cfg(windows)]`. `bt-app`'s `webhost.rs` is the only consumer and it names
seventeen of them.

| Item | Signature | Win type | Thread | Caller on failure | Class | Reason |
|---|---|---|---|---|---|---|
| `WebHost` | `struct`; `new(3×Box<dyn Fn>)`, `drain`, `set_claimed_chords(Vec<WebChord>)`, `has_controller`, `request_environment(&Path,u64)`, `request_controller(NonZeroIsize,u64)`, `install(&Compositor, PageVisual, u64)`, `rehost(&RehostSide,&RehostSide,(i32,i32,u32,u32),bool)`, `dpi_ownership`, `set_bounds`, `set_rasterization_scale`, `notify_parent_window_moved`, `set_visible`, `navigate`, `reload`, `stop`, `go_back`, `go_forward`, `open_dev_tools`, `zoom`, `set_zoom`, `find`, `find_step`, `find_stop`, `focus_page`, `browser_process_id`, `send_mouse`, `capture_preview`, `get_favicon`, `close_pending_controller`, `close` | hwnd, COM | window, handle | mixed — `toast`, `log`, fault state | **R** | `WKWebView`; M4-2. `dpi_ownership` and `browser_process_id` have **no `bt-app` caller** |
| `WebChord` | `struct { virtual_key: u16, ctrl, shift, alt: bool }` | **VK + no Command** | any | — | **R** | §4.4 ②: it has to change shape. M1-7 |
| `WebKey` | `struct { chord: WebChord, down: bool }` | VK | any | — | **R** | same shape change |
| `WebEvent` | `enum`, 20 variants (`Environment`, `Controller`, `NavigationStarting`, `RequestRefused`, `AcceleratorKey`, `Favicon`, …) | some carry `HRESULT`-shaped strings | any | matched | **R** | the engine's vocabulary; M4-2 maps `WKNavigationDelegate` onto it |
| `WebMouseEvent` | `enum` (`Move`, `LeftDown`, `LeftDoubleClick`, …) | Win32 message ids | any | — | **R** | `NSEvent` types |
| `web_mouse_buttons` | `mod` with `NONE/LEFT/RIGHT/MIDDLE/X1/X2: u32` | Win32 bits | any | — | **R** | the values are `MK_*` bits |
| `WebNavigationVerdict` | `enum { Proceed, Cancel, CancelAndNavigateTo(String) }` | no | any | — | **P** | `webnav.rs`'s pure decision; X-2 says the verdict survives and the *hook* does not |
| `WebRequestVerdict` | `enum { Allow, Refuse }` | no | any | — | **P** | as above |
| `WebGuards` | `struct` + `none()`, `all_stand()`, `missing()` | no | any | reported | **P** | the four switches as a value |
| `RehostSide<'a>` | `struct { compositor: &'a Compositor, page: PageVisual, hwnd: NonZeroIsize }` | **hwnd field** | window | — | **R** | §4.4 ②: the `hwnd` has to go or become a window token |
| `RehostOutcome` | `enum { Moved, KeptSource{..}, Lost{..} }` | no | any | matched | **P** | the three answers survive a `CALayer` reparent |
| `RehostStep` / `REHOST_SEQUENCE` | `enum` / `const [RehostStep; 9]` | no | any | — | **P°** | **no `bt-app` caller**; the WebView2 reparent walk |
| `RehostCompensation` / `rehost_compensation` | `struct` + `is_empty` / `fn(RehostStep) -> RehostCompensation` | no | any | — | **P°** | **no `bt-app` caller**; the undo for that walk |
| `InstallStep` / `INSTALL_SEQUENCE` / `InstallRollback` / `install_rollback` / `WebInstallReport` | five items | no | any | — | **P°** | **no `bt-app` caller**; the controller-install walk and its rollback |
| `WEB_CLOSE_STEPS` | `const [CloseStep; 7]` (the enum itself is not exported) | no | any | — | **P°** | **no `bt-app` caller** |
| `WebSetting` / `WebSettingRule` / `WEB_SETTINGS` | `enum` + `api()`, `interface()`, `rule()` / `const [(WebSetting,bool); 9]` | names WebView2 interfaces | any | — | **A** | nine `ICoreWebView2Settings` properties by name; **no `bt-app` caller**. X-2's matrix replaces them |
| `WebDpiOwnership` | `struct { detects_monitor_scale_changes: bool, rasterization_scale: f64, bounds_mode_is_raw_pixels: bool }` | WebView2 concepts | any | — | **A** | three WebView2-only questions; **no `bt-app` caller** |
| `forget_web_environment` | `fn()` | COM statics | any | — | **R** | drops the shared environment; `WKWebsiteDataStore` is M2-6/M4-2's question |
| `webview2_runtime_version` | `fn() -> Result<String, String>` | WebView2 | any | sets a fault | **X** | "is the engine installed" — on macOS WebKit always is, so the honest arm answers `Ok` |

### 2.5 `video` and `video::engine`

`#[cfg(windows)] pub mod video;` — 11 items in `mod.rs` (counting the `engine`
module itself) and 15 in `engine.rs`. `bt-app` names `first_frame`, `prewarm`,
`shutdown_media_session`, `SEEK_FRACTION`, `VideoFrame`, and from `engine`:
`Engine`, `EngineError`, `EngineState`, `engines_outstanding`,
`engines_shut_down`, `engines_started`.

| Item | Signature | Win type | Thread | Caller on failure | Class | Reason |
|---|---|---|---|---|---|---|
| `video` | `#[cfg(windows)] pub mod` | — | — | — | **R** | becomes a plain `pub mod` with `win`/`mac`/`neither` bodies (§4.3) |
| `VideoFrame` | `struct` (pixels + size) | no | any | — | **P** | the frame's shape |
| `FirstFrameCost` / `FIRST_FRAME_BUDGET` / `SEEK_FRACTION` | `struct` + `total()` / two consts | no | any | — | **P** | timing policy, unchanged |
| `first_frame` | `fn(&Path, u32, u32) -> Option<VideoFrame>` | MF | own thread | `None` → no hover card | **R** | `AVAssetImageGenerator`; M4-4 |
| `decode_first_frame` / `decode_first_frame_measured` | same, plus cost | MF | own thread | `None` | **R** | as above; **no `bt-app` caller** for either |
| `prewarm` | `fn()` | MF platform | any | — | **N** | `MFStartup` warm-up; AVFoundation needs none |
| `shutdown_media_session` | `fn()` | MF platform | main | — | **N** | `MFShutdown`; nothing to shut down |
| `media_session_starts` | `fn() -> u32` | ledger | any | — | **P°** | **no `bt-app` caller**; a test ledger |
| `engine` | `pub mod` | — | — | — | **R** | `AVPlayer`; M4-5 |
| `Engine` | `struct`; `open`, `open_on(Adapter)`, `source`, `state`, `frame`, `standing_frame`, `frame_cost`, `adapter_in_use`, `play`, `pause`, `seek`, `set_rate`, `set_muted`, `set_volume`, `wait_for_metadata`, `shutdown` | MF, D3D | own thread, handle | `EngineError` → banner | **R** | M4-5, including **audio**, which the spike never costed |
| `EngineError` / `EngineState` / `Frame` / `FrameCost` / `Adapter` | five types | `Adapter` names D3D11 vs software | any | matched | **R** | `Adapter` is the one that does not survive |
| `FRAME_POLL_INTERVAL` / `IDLE_POLL_INTERVAL` / `OPEN_BUDGET` / `SHUTDOWN_BUDGET` | four consts | no | any | — | **P** | timing policy |
| `engines_started` / `engines_shut_down` / `engines_outstanding` | `fn() -> u64` ×3 | no | any | — | **P** | the leak ledger; `main.rs`'s tests read it |
| `CanPlay` / `can_play_types` | `enum` / `fn(&[&str]) -> Vec<CanPlay>` | MF | any | — | **R** | **no `bt-app` caller**; the format probe |

### 2.6 `attention_pipe` — 10 items

| Item | Signature | Win type | Thread | Caller on failure | Class | Reason |
|---|---|---|---|---|---|---|
| `attention_pipe` | `#[cfg(windows)] pub mod` | — | — | — | **R** | a Unix socket; M4-7 |
| `AttentionPipe` | `struct`; `start(impl Fn(String) + Send + 'static) -> io::Result<Self>`, `name() -> &str`, `counts() -> PipeCounts`. `unsafe impl Sync`, `Drop` | `HANDLE` | own thread, handle | `log`, channel absent | **R** | M4-7 |
| `PipeCounts` | `struct` (accepted/refused/…) | no | any | — | **P** | the same counters |
| `MAX_MESSAGE_BYTES` / `MAX_FRAMES_PER_SECOND` | `const usize` / `const u32` | no | any | — | **P** | bounds, unchanged |
| `session_tag` | `fn(&str) -> String` | **logon SID** | any | — | **R** | R6: a Unix socket names a *user*, not a session |
| `endpoint_name` | `fn(&str, u32, u128) -> String` | pipe name | any | — | **R** | a socket path, with a length limit the pipe name did not have |
| `security_descriptor_sddl` | `fn(&str) -> String` | **SDDL** | any | — | **A** | SDDL has no Unix twin; M4-7 states the difference rather than substituting `0600` |
| `send_line` | `fn(&str, &str) -> io::Result<()>` | pipe | any | `?` | **R** | the client half |
| `names_an_endpoint` | `fn(&str) -> bool` | pipe name | any | — | **R** | **no `bt-app` caller**; the grammar changes with the name |
| `unguessable_bits` | `fn() -> u128` | `BCrypt` | any | — | **R** | `getrandom` |

### 2.7 `launch_pipe` — 7 items

| Item | Signature | Win type | Thread | Caller on failure | Class | Reason |
|---|---|---|---|---|---|---|
| `launch_pipe` | `#[cfg(windows)] pub mod` | — | — | — | **R** | M3-5 |
| `LaunchPipe` | `struct`; `start<T,D,C>(&Path, decide: D, commit: C) -> io::Result<Self>`, `name()`. `unsafe impl Sync`, `Drop` | `HANDLE` | own thread, handle | `log` | **R** | the `Decision`/`Admission`/client-`CONFIRM` semantics are preserved; the transport is not |
| `Decision<T>` | `struct` | no | any | — | **P** | the semantics §7.59 fixes |
| `endpoint_name` | `fn(&str, &str) -> String` | pipe name | any | — | **R** | socket path length limits; R5 |
| `endpoint_for` | `fn(&Path) -> Option<String>` | pipe name | any | `None` | **R** | as above |
| `hand_over` | `fn(&str, &str, impl FnOnce(u32,&str)) -> io::Result<()>` | pipe | any | `?` | **R** | the client half, with peer verification to add (DESIGN §7.59b) |
| `HANDOVER_BUDGET` / `CONFIRM` | `const Duration` / `const &str = "ok"` | no | any | — | **P** | wire policy, unchanged |

### 2.8 `hang` — 10 items

`pub mod hang;` is **ungated** and already carries four `#[cfg(not(windows))]`
arms, exactly as §4.4 says.

| Item | Signature | Win type | Thread | Caller on failure | Class | Reason |
|---|---|---|---|---|---|---|
| `hang` | `pub mod` | — | — | — | **P** | already present everywhere |
| `current_thread_id` | `fn() -> u32` | — | any | — | **R** | a `pthread` id, or `gettid`'s Mach twin; the arm exists and returns `0` |
| `Answer` | `enum` + `phrase()` | no | any | — | **P** | the liveness verdict |
| `ask_thread_to_answer` | `fn(u32, Duration) -> Answer` | — | any | matched | **R** | M4-11's **event-loop liveness handshake**; the portable arm refuses today |
| `StackSample` | `struct` + `refused(&str)` | no | any | — | **P** | the sample's shape, including its refusal |
| `capture_thread_stack` | `fn(u32, usize) -> StackSample` | `CONTEXT` | any | `StackSample::refused` | **X** | already refuses off Windows *and* off x86-64. Suspending a thread and reading its stack is not a macOS facility Folio should reach for; M4-11 collects the system's crash reports instead |
| `ModuleSite` / `ModuleRange` | two structs | no | any | — | **P** | address → module + offset |
| `module_map` | `fn() -> Vec<ModuleRange>` | PSAPI | any | empty | **R** | `_dyld_image_count`; the portable arm returns empty today |
| `resolve` | `fn(&[ModuleRange], u64, usize) -> Option<ModuleSite>` | no | any | — | **P** | ungated, pure binary search |
| `scan_frames` | `fn(&[ModuleRange], &[u8], usize) -> Vec<ModuleSite>` | no | any | — | **P** | ungated, pure |

### 2.9 `handoff` — 16 items

`pub mod handoff;` is ungated; nine of its items are gated inside and re-exported
at the crate root.

| Item | Signature | Win type | Thread | Caller on failure | Class | Reason |
|---|---|---|---|---|---|---|
| `handoff` | `pub mod` | — | — | — | **P** | present everywhere |
| `PROGRAM_REFUSED` | `const &str` | no | any | — | **P** | a sentinel `bt-app` matches with `.contains` |
| `effective_final_component` | `fn(&Path) -> String` | path grammar | any | — | **P°** | Windows path folding |
| `normalised_target` | `fn(&Path) -> Option<PathBuf>` | path grammar | any | `None` | **P°** | as above |
| `asks_windows_not_to_normalise` | `fn(&Path) -> bool` | `\\?\` | any | — | **A** | a `\\?\` prefix has no Unix meaning |
| `names_a_program` | `fn(&Path, &str) -> bool` | `PATHEXT` | any | — | **R** | on Unix the question is the execute bit, not an extension list |
| `validate_openable_path` | `fn(&Path) -> Result<(), String>` | mixed | any | propagates | **R** | the refusal survives; the grammar under it does not |
| `validate_local_image_path` | `fn(&Path) -> Result<(), String>` | mixed | any | propagates | **R** | as above |
| `reveal_arguments` | `fn(&Path) -> Option<OsString>` | `explorer /select` | any | `None` | **R** | `NSWorkspace.activateFileViewerSelectingURLs:` takes URLs, not an argument string |
| `reveal_argument_form` | `fn(&Path, bool) -> Option<OsString>` | as above | any | `None` | **P°** | **no `bt-app` caller** |
| `program_in_directories` | `fn(&Path, &[PathBuf], &str, &dyn Fn(&Path) -> bool) -> Option<PathBuf>` | `PATHEXT` | any | `None` | **R** | the `PATH` walk; the extension list becomes the execute bit |
| `program_on_path` | `#[cfg(windows)] fn(&Path) -> Option<PathBuf>` | `PATHEXT` | any | `?` | **R** | as above |
| `shell_execute` | `#[cfg(windows)] fn(NonZeroIsize, &str) -> Result<(),String>` | hwnd | window | `toast` | **R** | `NSWorkspace.openURL:` needs no window |
| `open_local_file` | `#[cfg(windows)] fn(NonZeroIsize, &Path) -> Result<(),String>` | hwnd | window | `toast` | **R** | as above |
| `open_local_path` | `#[cfg(windows)] fn(NonZeroIsize, &Path) -> Result<(),String>` | hwnd | window | `toast` | **R** | as above |
| `reveal_in_explorer` | `#[cfg(windows)] fn(NonZeroIsize, &Path) -> Result<(),String>` | hwnd | window | `toast` | **R** | Finder |
| `open_system_fonts_page` | `#[cfg(windows)] fn(NonZeroIsize) -> Result<(),String>` | hwnd | window | `toast` | **R** | Font Book, or the fonts folder |

### 2.10 `hotkey` — 17 items

`pub mod hotkey;` is ungated and carries **two** `#[cfg(not(windows))]` arms,
exactly as §4.4 says.

| Item | Signature | Win type | Thread | Caller on failure | Class | Reason |
|---|---|---|---|---|---|---|
| `hotkey` | `pub mod` | — | — | — | **P** | present everywhere |
| `Hotkey` | `struct { ctrl, alt, shift, win: bool, virtual_key: u16 }` | **VK + `win`** | any | — | **R** | needs a Command field and a macOS key code; the same shape change as `WebChord` |
| `registration_bits` | `fn(Hotkey) -> Option<(u32,u32)>` | `MOD_*` | any | `None` → refuse | **P°** | the `RegisterHotKey` argument pair |
| `holds_a_summon_modifier` | `const fn(Hotkey) -> bool` | no | any | — | **P** | R2-14's refusal, and it holds on any platform |
| `HotkeyFault` | `enum` + `is_already_registered()` | Win32 error | any | shown in settings | **R** | M4-8's *not authorized* state is a new variant |
| `is_our_hotkey` | `fn(u32, isize, usize, i32) -> bool` | `WM_HOTKEY` | any | — | **A** | a window-message predicate |
| `summon_should_act` | `fn(u32, isize, usize, i32, bool) -> bool` | `WM_HOTKEY` | any | — | **A** | as above |
| `registration_is_live` | `fn(i32) -> bool` | id ledger | any | — | **P°** | bookkeeping over Win32 hotkey ids |
| `GlobalHotkey` | `#[cfg(windows)] struct` + `id()`, `Drop` | hwnd/thread | window, handle | `HotkeyFault` | **R** | `CGEventTap`; M4-8, gated behind X-5 |
| `register` | `#[cfg(windows)] fn(i32, Hotkey) -> Result<GlobalHotkey, HotkeyFault>` | thread msg | window | `HotkeyFault` | **R** | as above |
| `summon_message_hook` | `#[cfg(windows)] fn(i32, impl Fn() + 'static) -> impl FnMut(*const c_void) -> bool` | `MSG` | window | — | **A** | a winit `with_msg_hook` callback; **the one `#[cfg(windows)]` in `main.rs`** |
| `foreground_window` | `fn() -> Option<NonZeroIsize>` (two arms) | hwnd | any | `None` | **R** | the frontmost application, not a window handle |
| `give_foreground_to` | `fn(NonZeroIsize) -> bool` (two arms) | hwnd | any | reads the `bool` | **R** | `NSRunningApplication.activate…` |
| `allow_foreground_for` | `#[cfg(windows)] fn(u32) -> bool` | `AllowSetForegroundWindow` | any | `ignore` | **N** | macOS has no foreground-lock to ask permission from; the launch handover simply activates |
| `another_round` | `fn(usize, Duration) -> bool` | no | any | — | **P** | the retry budget's policy |
| `FOREGROUND_BUDGET` | `const Duration` | no | any | — | **P** | 250 ms, unchanged |
| `HandoverStep` / `handover_step` | `enum` / `const fn(u32,u32,bool) -> HandoverStep` | `AttachThreadInput` | any | — | **A** | thread-input attachment is a Win32 notion |

### 2.11 `http`, `instance`, `msix`, `explorer_command`

| Item | Signature | Win type | Thread | Caller on failure | Class | Reason |
|---|---|---|---|---|---|---|
| `http` | `#[cfg(windows)] pub mod` | — | — | — | **R** | M4-10, `NSURLSession` |
| `http::HttpsGet<'a>` | `struct { host, path, … }` | no | any | — | **P** | one request, described |
| `http::https_get` | `fn(&HttpsGet) -> Result<String, String>` | WinHTTP | own thread | `Err` → settings row | **R** | M4-10 |
| `instance` | `pub mod` | — | — | — | **P** | ungated |
| `instance::DataDirectoryClaim` | `#[cfg(windows)] struct { handle: HANDLE }` / `#[cfg(not(windows))] struct;` | **HANDLE** | any, handle | — | **R** | §4.4 ①: the SDK type stays inside the gated body |
| `instance::claim_data_directory` | `fn(&Path) -> Option<DataDirectoryClaim>` | mutex | any | `None` → hand over | **R** | the non-Windows arm returns **`Some`** — §4.4 ④ is right, and M3-5 **builds** the guarantee |
| `instance::directory_tag` | `fn(&Path) -> String` | **case folding** | any | — | **R** | lowercases a lossy path; wrong on a case-sensitive APFS volume (R5) |
| `instance::claim_name` | `fn(&Path) -> String` | `Local\` prefix | any | — | **R** | a kernel object name; becomes a socket path |
| `msix` | `pub mod` — **ungated** | — | — | — | **P°** | its pure half compiles off Windows today |
| `msix::PACKAGE_NAME` / `PACKAGE_PUBLISHER` / `PACKAGE_FILE_NAME` / `PACKAGE_EXECUTABLE` / `EXPLORER_COMMAND_CLSID` / `PRIMARY_CONTEXT_MENU_BUILD` | six consts | package identity | any | — | **A** | MSIX identity; `PACKAGE_FILE_NAME` is the only one `bt-app` names |
| `msix::supports_primary_context_menu` | `fn(u32) -> bool` | build number | any | — | **A** | a Windows 11 build test |
| `msix::Rdn` / `distinguished_name` / `publisher_matches_subject` / `describe_name` | one alias, three fns | X.500 subject | any | — | **P°** | pure certificate-subject parsing; **no `bt-app` caller** |
| `msix::PackageRegistration` / `registered` / `register` / `remove` | `#[cfg(windows)]`, four items | WinRT | own thread | `toast` | **A** | sparse-MSIX deployment |
| `msix::windows_build` | `#[cfg(windows)] fn() -> u32` | `RtlGetVersion` | any | `0` | **A** | as above |
| `msix::explorer_command_clsid` | `#[cfg(windows)] fn() -> windows::core::GUID` | **`windows::core::GUID`** | any | — | **A** | §4.4 ①, exactly as named. **No `bt-app` caller** |
| `explorer_command` | `#[cfg(windows)] pub mod` | — | — | — | **A** | `IExplorerCommand`; §4.4 |
| `explorer_command::Verb` | `struct` | COM | main | — | **A** | as above |
| `explorer_command::serve` | `fn(Verb) -> Result<(), String>` | COM | main, blocks | `leave_process` | **A** | as above |
| `explorer_command::IDLE_LINGER` | `const Duration` | no | any | — | **A** | as above |

---

## 3. (a) The M1 startup path, in call order

M1's acceptance is "a fresh, Finder-launched Folio opens a window and a native
shell". Everything below stands between `main` and the first frame, and M1-1 has
to have an answer for each. `window_hwnd` is the hinge: **one function,
`main.rs:108160`, called at 50 sites**, and every row that says `hwnd` in the
tables above reaches the platform through it.

```
fn window_hwnd(window: &Window) -> Result<std::num::NonZeroIsize>
    window.window_handle()  →  RawWindowHandle::Win32  →  handle.hwnd
    else: Err("bt-app requires a Win32 window handle")
```

That `else` is the first line of M1-1: on macOS `window_handle()` answers
`RawWindowHandle::AppKit { ns_view }`, and the function as written refuses every
window on the platform. It is the **native handle abstraction** M1-1 is named
after, and until it exists nothing below it runs.

**The two window constructors.** `Runtime::create` (`main.rs:35581`) opens the
first window of the process; `Runtime::open_window` (`main.rs:36445`) opens every
later one. They are not the same code and they must be ported together — the
second one's own doc comment says it deliberately does none of the first's file
reading, and the *platform* prologue is what they share.

| # | `Runtime::create` | `Runtime::open_window` | Item |
|---:|---|---|---|
| 1 | `35862` | `36481` | `event_loop.create_window(attributes)` → `?` |
| 2 | `35864` | — | `install_theme_class_background(&window)?` → `install_window_class_background` (`108019`) |
| 3 | `35866` | `36485` | `window.set_ime_allowed(true)` — X-3 says it stays |
| 4 | `35867` | `36486` | **`window_hwnd(&window)?`** |
| 5 | `35873` | `36487` | `CustomWindowFrame::install(hwnd, CustomFrameGeometry{..})?` |
| 6 | `35882` | `36496` | `ImeSystemCaret::new(hwnd)` — infallible, returns `Self` |
| 7 | `35883` | `36497` | `MathContextMenu::new(hwnd)?` |
| 8 | `35886` | `36500` | `FolderPicker::new(hwnd)?` |
| 9 | `35889` | `36503` | `ImagePicker::new(hwnd)?` |
| 10 | `35898` | — | `system_backdrop_available(hwnd)` → the Acrylic row |
| 11 | — | `36547–36549` | `dpi_at`, `work_area_at`, `virtual_screen_rect` — where the new window stands |
| 12 | `35918` | `36578` | `set_window_outer_rect(hwnd, …)?` |
| 13 | `35935` | `36586` | **`Compositor::new(hwnd)?`** |
| 14 | `35942` | — | `install_page_ground_color(&compositor)` → `set_page_ground_color`, `log` on failure |
| 15 | `35943` | `36597` | `GpuContext::open(bt_render::WindowTarget::CompositionVisual(compositor.gpu_visual_ptr()), …)?` |
| 16 | `35094` | `35094` | `new_window_runtime` → `window_hwnd(&window).ok()` → `SystemSettingsWatch::install(hwnd, Box<dyn Fn()>)`, **`.ok()` — best effort** |

**Seven** of those sixteen are a `bt-platform` call propagated with `?` and
`anyhow::Context`, and are therefore fatal to the launch — steps 2, 5, 7, 8, 9,
12 and 13, with step 15's `GpuContext::open` an eighth belonging to `bt-render`.
`SystemSettingsWatch` is the only one already written as best-effort, and
`ImeSystemCaret::new` is the only one that cannot fail. That ratio is what
M1-1's "deferred-service construction made harmless" is about: on macOS four of
these constructors — the two pickers, the formula menu and the IME caret — have
nothing to construct at that moment, and each has to become either a real
AppKit call or a value that costs nothing, **not** a `?` that kills a launch.

**`Compositor::new` and `WindowTarget::CompositionVisual`.** They are one touch,
not two: `Compositor::new(hwnd)` builds the DirectComposition tree, and
`gpu_visual_ptr()` hands its GPU visual straight into
`bt_render::WindowTarget::CompositionVisual`. `bt-render` already gates that
variant `#[cfg(windows)]` (the spike's class-B fix) while `WindowTargetKind`
keeps both names everywhere, so the *kind* travels and the *variant* does not.
X-1's finding — wgpu-hal 30's Metal backend offers only `Opaque` and
`PostMultiplied`, and `required_alpha_mode` demands `PreMultiplied` for
`CompositionVisual` — lands exactly here: step 15 cannot be reached on macOS with
the contract as written, which is why M1-4 is a `WindowTargetKind` arm **with its
own alpha policy** rather than a third spelling of the same one.

**Device-loss reconstruction** is the same touch again, and it is the reason the
abstraction cannot be a one-off at startup.
`impl LostDevice for TheDeviceAndItsWindows` (`main.rs:101443`) walks every open
window, reads `window.compositor.gpu_visual_ptr()` **at the moment of use and
never stores it**, wraps each in `WindowTarget::CompositionVisual`, and calls
`gpu.rebuild_after_device_loss(rebuilt)`. Its doc comment states the contract in
so many words: the visual must be live at the instant the surface is made, and
the `Compositor` the caller holds is what makes it so. A macOS arm has to keep
that property for a `CAMetalLayer` — M1-1 and M1-4 own it jointly, and X-1's
fourth probe case is a device-loss reconstruction for precisely this reason.

**Deferred, but inside M1's acceptance line.** `Taskbar::new(hwnd)` is built
lazily on the first progress report (`main.rs:24729`) and `Notifier::new` on the
first toast (`main.rs:24798`); both remember a refusal so a machine that cannot
do it costs one line of stderr rather than one per message. That is the shape
every macOS refusal should copy, and it is already in the tree.

---

## 4. (b) `bt-app`'s platform gates today, and the allowlist

Measured at `0670be9`: `bt_platform::` appears at **528 occurrences across 36
files**, 255 of them in `main.rs`. The plan's §4.3 says 519 across 36 with 249 in
`main.rs`; the file count is exact and the occurrence counts have drifted by nine
in five days, which is the drift R2 predicts and the reason this section states
its own measurement date.

**The eleven files with a platform `cfg` — §4.3's list is exactly right.**

| File | Gate | What it gates |
|---|---|---|
| `psreadline.rs:426/452` | `windows` / `not(windows)` | `run_probe()` — the real PowerShell probe vs `Probe::default()` |
| `psreadline.rs:873/877` | both | `documents_directory()` vs `None` |
| `psreadline.rs:994/998` | both | `file_product_version(path)` vs `None` |
| `psreadline.rs:1625, 1670, 1708, 1781` | `windows` | four test helpers and tests |
| `attention_copilot.rs:657/687` | both | `run_probe()` — `program_on_path` + `quiet_command_named` vs `None` |
| `attention_copilot.rs:681, 715` | `windows` | `probe_command_tail` and its test |
| `files.rs:1081/1091` | both | `is_concealed(&Metadata)` — `FILE_ATTRIBUTE_HIDDEN` vs `false` |
| `files.rs:1909` | `windows` | the hidden/system-name test |
| `explorer_menu.rs:472/474` | both | `supported()` — `msix::supports_primary_context_menu(msix::windows_build())` vs `false` |
| `explorer_menu.rs:515/542` | both | `read_state()` vs `PackageState::Unsupported` |
| `wsl.rs:84/95` | both | `impl Registry for CurrentUser` — the two registry reads vs `None` |
| `update.rs:376/389` | both | `latest_tag()` — `http::https_get` vs `Err("this build has no HTTP stack")` |
| `shell_integration.rs:1104/1121` | both | `run_profile_probe(program)` vs `None` |
| `settings.rs:1281/1283` | both | `monospace_font_families()` vs `order_monospace_families(Vec::new())` |
| `palette_index.rs:882/884` | both | a test's `symlink_dir` vs `symlink` |
| `main.rs:109662` | `windows` | `EventLoopBuilderExtWindows` + `hotkey::summon_message_hook` — **the only one** |
| `git_panel.rs:6267` | `windows` | one drive-letter test |

**Which must become the named allowlist.** Six of these are *production* arms
that a macOS build has to enter and that nothing else may join:
`psreadline.rs`'s `run_probe`, `attention_copilot.rs`'s `run_probe`,
`files.rs`'s `is_concealed`, `explorer_menu.rs`'s `supported`/`read_state`,
`wsl.rs`'s `impl Registry`, `update.rs`'s `latest_tag`,
`shell_integration.rs`'s `run_profile_probe` and `settings.rs`'s font
enumeration. Three are tests (`palette_index.rs`, `git_panel.rs`, and
`psreadline.rs`'s four) and belong on the list as tests, not as product gates.
`main.rs:109662` is the one gate that will **grow** rather than shrink: M4-8
adds a macOS arm beside it.

The allowlist gate M1-10 builds should therefore hold a file-and-line list of
*production* gates and refuse a new one anywhere else, which is the only shape
that keeps §4.3's ratio — 528 platform calls behind 17 gates — from decaying.

**`cli.rs` is not on the list and has two ungated `std::os::windows` uses**, as
§4.3 says, and both are confirmed:

- **`cli.rs:445`**, inside `fn value_for(name, text, arg, args) -> Result<OsString, CliFault>`:
  `use std::os::windows::ffi::{OsStrExt, OsStringExt};` — it splits a `--cwd=x`
  argument at the `=` on the **encoded** `OsString` via `encode_wide` /
  `from_wide`, because the half after the sign is a path and a path is not
  required to be text.
- **`cli.rs:1240`**, inside the test
  **`an_argument_that_is_not_text_is_taken_as_the_path`**:
  `use std::os::windows::ffi::OsStringExt;` — it builds a lone high surrogate
  (`OsString::from_wide(&[0x0044, 0xD800, 0x005C])`) as a name Windows will hand
  over and `to_str` will refuse.

`scripts/check-portable-core.ps1` never sees either, because it scans the
thirteen portable crates and `bt-app` is not one of them. The plan's
recommendation — give `value_for` a portable implementation rather than admit
`cli.rs` to the list — is the right one and this inventory adds a detail for it:
the production use is **split-at-a-known-ASCII-offset**, which
`OsStr::as_encoded_bytes` / `OsStr::from_encoded_bytes_unchecked` express on both
platforms, and the *test* has no portable twin at all — a lone surrogate is not a
thing a Unix `OsString` can hold. So M1-10 makes the production function portable
**and keeps the test `#[cfg(windows)]`**, with a Unix sibling that uses an
invalid UTF-8 byte instead. Those are two decisions, not one.

---

## 5. (c) The count

**268 public items**, counted from the tables above — a row that names several
items (`msix`'s six consts, the five engine types, the five install-walk items)
expands to its own number.

| Group | Rows | Items |
|---|---:|---:|
| crate root, ungated (§2.1) | 44 | 44 |
| crate root, `#[cfg(windows)]` (§2.2) | 2 | 2 |
| `windows_impl` re-exports (§2.3) | 73 | 73 |
| `webview` re-exports (§2.4) | 19 | 27 |
| `video` + `video::engine`, both module rows included (§2.5) | 14 | 27 |
| `attention_pipe`, module + 10 (§2.6) | 10 | 11 |
| `launch_pipe`, module + 7 (§2.7) | 7 | 8 |
| `hang`, module + 10 (§2.8) | 10 | 11 |
| `handoff`, module + 16 (§2.9) | 17 | 17 |
| `hotkey`, module + 17 (§2.10) | 17 | 18 |
| `http`, `instance`, `msix`, `explorer_command` — 4 modules + 26 (§2.11) | 19 | 30 |
| **Total** | **232** | **268** |

**By classification.**

| Class | Items | Share |
|---|---:|---:|
| **R** — real macOS implementation | 114 | 43 % |
| **P** — portable policy, reached on macOS | 61 | 23 % |
| **A** — compile-time absence | 44 | 16 % |
| **P°** — portable policy, unreachable on macOS | 38 | 14 % |
| **X** — invocation-time refusal | 6 | 2 % |
| **N** — harmless lifecycle no-op | 5 | 2 % |

**Windows types in signatures.** Two numbers, because the ticket's list and the
honest reading are not the same size.

*The hard leaks — the ones the ticket names — are **58**:* 40 items carry an
`HWND` as a `NonZeroIsize` (a parameter in 38 of them, a **field** in
`RehostSide` and a constructor argument held by every window-scoped handle type);
3 carry a `HANDLE` (`DataDirectoryClaim`, `AttentionPipe`, `LaunchPipe`); 1
returns `windows::core::GUID` (`msix::explorer_command_clsid`); 10 carry a Win32
**virtual key, modifier bit or window-message value** (`WebChord`, `WebKey`,
`Hotkey`, `virtual_key_for_character`, `registration_bits`, `web_mouse_buttons`,
`WebMouseEvent`, `is_our_hotkey`, `summon_should_act`, `summon_message_hook`); 3
carry Win32 thread-input or foreground ids (`handover_step`, `HandoverStep`,
`allow_foreground_for`); and 1 is shaped by a `CONTEXT` capture
(`hang::capture_thread_stack`).

*Counting Windows **shapes** as well as Windows **types**, it is **164**.* The
other 106 carry a registry path, an SDDL descriptor, a named-pipe name, an
AppUserModelID, `PATHEXT`, a `\\?\` prefix, a console handle, a DWM or
`APPBARDATA` value word, or the identity of a COM / WinRT / Media Foundation
object. **That second number is the one that should size M1-1**, because a
signature that is portably *spelled* and Windows-*shaped* is exactly the defect
§4.4 ② is about, and it outnumbers the visible handles nearly three to one.

**What `bt-app` actually consumes: 173 of the 268 item names.** Measured by
matching every item name against `bt_platform::…` paths, `use bt_platform::…`
imports and module-qualified uses in `crates/bt-app/src`; it is a name match, so
it is generous rather than strict. **Ninety-five public items have no `bt-app`
reference at all** — read by `bt-platform`'s own tests, or by nothing. Twenty-two
of those are worth naming because they are whole design surfaces nobody upstairs
uses: `INSTALL_SEQUENCE`,
`REHOST_SEQUENCE`, `WEB_CLOSE_STEPS`, `WEB_SETTINGS`, `WebDpiOwnership`,
`WebSetting`, `WebSettingRule`, `InstallStep`, `InstallRollback`,
`install_rollback`, `WebInstallReport`, `RehostStep`, `RehostCompensation`,
`rehost_compensation`, `Compositor::set_gpu_offset`, `WebHost::dpi_ownership`,
`WebHost::browser_process_id`, `exposure_probe_points`, `exposed_from_probe`,
`taskbar_auto_hidden_from_state`, `apartments_left`, `std_error_is_console`,
`process_image_path`. **M4-2 should not port any of the first fourteen.** They
describe WebView2's install and rehost walks and have no WKWebView counterpart to
describe; carrying them across would be porting a shape rather than a behaviour.

That is the headline correction to the spike: the port is not 79 items and it is
not 268. It is **173 items `bt-app` names**, of which **114 across the whole
surface need a real macOS implementation**, and the rest is either already
portable or should be deleted rather than translated.

---

## 6. What §4.4 got wrong, and what it got right

**Right, and verified line by line.** `set_current_thread_priority`'s portable
arm answering `false` (`lib.rs:9053`). **Four** `#[cfg(not(windows))]` arms in
`hang.rs` — lines 140, 249, 362 and 433 — and **two** in `hotkey.rs` — 623 and
630. `msix::explorer_command_clsid()` returning `windows::core::GUID` behind a
Windows gate (`msix.rs:419`). `DataDirectoryClaim` holding a `HANDLE` in its
gated definition and being a unit struct in its non-Windows one
(`instance.rs:51` / `154`). `WebChord` carrying a virtual key and no Command
(`webview.rs:107`). `RehostSide.hwnd: NonZeroIsize` (`webview.rs:1006`).
`leave_process(code: i32) -> !`. `redirect_std_streams_to_file` and
`silence_std_streams` being load-bearing — `diagnostics.rs:263` picks
`Channel::Log` or `Channel::Nowhere` off the first one's `bool` and calls the
second in the `else`. And §4.4 ④: `claim_data_directory` off Windows returns
**`Some(DataDirectoryClaim)`**, with the comment saying so in as many words. The
single-writer guarantee is absent by construction, and M3-5 builds it.

**Six things to correct or add.**

**① The five classes need a sixth, and it is the third-largest bucket.**
Thirty-eight items are pure, compile for `aarch64-apple-darwin` today, and have
**no macOS caller**: the context-menu family (11 of §2.1's rows), the
notification-registration family (`NOTIFICATION_DISPLAY_NAME`, `toast_xml`),
`PendingWindowPos` / `hold_pending_pos_to`, `VisualLayer`, the ten WebView2
install- and rehost-walk descriptions, `cloaked_from_attribute`,
`taskbar_auto_hidden_from_state`, `registration_bits`, `registration_is_live`,
`media_session_starts`, `handoff`'s three path-grammar helpers, and `msix`'s
certificate-subject parser with its `Rdn` alias. None of the five classes fits:
they are not a refusal, not a no-op, not an absence, and calling them "portable
policy" implies a macOS caller that will never exist. M1-1 needs the distinction
because it decides what goes behind the `mac` body and what simply stays where it
is — compiled, green, and unreferenced.

**② §4.3's `#[cfg(windows)] pub mod webview` does not exist.** `webview` is
`#[cfg(windows)] mod webview` (private) plus a 27-name `pub use` at the crate
root; `bt-app` never writes `bt_platform::webview::` outside one doc link. The
`mod win` / `mod mac` / `not(any(...))` shape §4.3 prescribes has to be applied
to the **re-export list**, which is a different edit and a better one: the three
bodies can export different *sets* only if the list is unconditional, so M4-2
must decide the macOS name set before the module is split, not after.

**③ `cancel_composition`'s portable arm already leaks the window handle.**
`#[cfg(not(windows))] pub fn cancel_composition(hwnd: NonZeroIsize) -> bool`
(`lib.rs:9119`) takes an `HWND`-shaped parameter it discards. It is the exact
defect §4.4 ② names — a signature that encodes Windows without saying so — and it
is already in the *portable* half of the crate, which is where the plan assumes
the leaks are not. `clipboard_text` and `set_clipboard_text` are the same defect
with a live implementation behind them, and M1-9 already owns dropping the
parameter; `cancel_composition` should be dropped in the same ticket rather than
waiting for M1-8.

**④ `msix` is not absent off Windows and `explorer_command` is.** §4.4's closing
sentence puts `wsl.rs`, `psreadline.rs`, `msix.rs` and `explorer_command.rs` in
one bucket. Two of those four are `bt-app` modules and two are `bt-platform`
modules, and only one of the `bt-platform` pair is actually gated:
`pub mod explorer_command;` is `#[cfg(windows)]`, while `pub mod msix;` is
**ungated** — six consts, `supports_primary_context_menu`, and the whole
certificate-subject parser compile on the Apple target today. Only the four
deployment entry points and the CLSID are gated. That is the right shape and the
plan should say so, because "`msix.rs` is absent" would send a ticket to delete
something that already builds.

**⑤ `WatchDepth` is not public, and `FilePickKind` is a type alias.**
`windows_impl::WatchDepth` (`Tree` / `HereOnly`) is the value that carries M2-1's
three contracts, and it is **not** in the crate-root re-export list — `bt-app`
reaches the three contracts through three constructors (`DirWatch::start`,
`start_shallow`, `start_shallow_named`) instead. M2-1 should either export the
enum or keep the three doors; what it must not do is assume the enum is already
part of the interface. Symmetrically, `FilePickKind` is
`pub type FilePickKind = ShellPickKind`, so the public name and the defined name
differ and a mechanical rename would break the alias.

**⑥ The startup path's `?` count is the real M1-1 risk, not the handle type.**
Seven of the sixteen startup steps propagate with `anyhow::Context` and kill the
launch: `install_window_class_background`, `CustomWindowFrame::install`,
`MathContextMenu::new`, `FolderPicker::new`, `ImagePicker::new`,
`set_window_outer_rect` and `Compositor::new`. The plan's M1-1 title says
"deferred-service construction made harmless", which is right, but the inventory
shows the harm is concentrated: **seven** `?` between `create_window` and the
first frame, in **both** constructors, and every one of them is a Win32 bridge
that has no macOS work to do at that moment. The lazy shape `Taskbar` and
`Notifier` already use — build on first need, remember the refusal, cost one line
of stderr — is the pattern to copy, and it is already in this tree.

---

## Appendix — the one cargo run

```
cargo check -p bt-platform --target aarch64-apple-darwin -j 2
    Finished `dev` profile in 0.62s
    7 warnings, 0 errors
```

The warnings are the evidence, not the noise: `unused import: std::num::NonZeroIsize`,
`constant CREATE_NO_WINDOW is never used`, `constant DEFAULT_PATHEXT is never used`,
`constant STACK_SCAN_BYTES is never used`, `function note_claimed is never used`,
`function note_released is never used`. Six dead items on the Apple target is the
whole of what falls out of `bt-platform` when two thirds of its body is gated
away — which is X-6's point restated from the other end, and the reason **the
tables above are a reading of the source and not of the compiler**.
