//! **A Carbon hot key, claimed and then pressed by a hand** —
//! T-MAC-SUMMON-DIAG's answer to the one station `tests/macos_hotkey.rs` could
//! not reach.
//!
//! # Why this exists beside a proof that already passed
//!
//! M4-8's proof sends the application a `kEventHotKeyPressed` it built itself
//! (`docs/DESIGN.md` §13.51 ④). That station is worth having and it measured
//! something real, but it shares this repository's own idea of what the event
//! looks like with the code under test — and when the two agreed on a *wrong*
//! four-character code for `kEventParamDirectObject`, the round trip was
//! perfect and the key on the desk did nothing. The one station in that proof
//! that used a real press (`CGEventPost` to `kCGHIDEventTap`) needed the
//! Accessibility grant and was skipped, and its absence is what let the defect
//! ship.
//!
//! So this is the opposite shape: **nothing here is Folio**. No winit, no
//! `bt_platform::hotkey`, no `EventLoop`, no window. It is the smallest program
//! that can hold Carbon's own contract — register, install, run, print — and
//! what it proves it proves about the *system*, which is exactly what is wanted
//! when the question is "does this Mac deliver this chord to this user at all".
//!
//! ```text
//! RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin \
//!   cargo run -p bt-platform --example hotkey_probe
//! ```
//!
//! It prints `registered` and then one line per press, and it runs until
//! `Ctrl+C`. Run it **in a window of the Mac's own screen** — a Terminal on the
//! machine, or one over screen sharing — because the thing being tested is a
//! keyboard.
//!
//! # Both positions of the key left of `1`
//!
//! Two claims are made, not one:
//!
//! * **`kVK_ANSI_Grave` (0x32)** — the key code the product registers, and the
//!   position backtick occupies on an ANSI keyboard;
//! * **`kVK_ISO_Section` (0x0A)** — the *extra* key an ISO keyboard has in that
//!   corner, which on those layouts is where `§`/`±` lives and which is a
//!   different key code for what a reader would call the same place.
//!
//! The press names which one it arrived on. That is the point: the product
//! claims 0x32 only, and whether that is right for the person holding the
//! keyboard is a fact about their keyboard that no amount of reading can
//! settle. `kVK_*` codes are **positions**, not characters — the general answer
//! for "which position produces backtick on this layout" is `UCKeyTranslate`
//! against the current input source, which §13.51 ⑤ leaves open — and this
//! probe is how that question gets an observation instead of an argument.
//!
//! # What a silent run means
//!
//! `registered` followed by nothing when the key is pressed is a real answer and
//! a narrow one: the claim was accepted by the hot key manager and the press is
//! not reaching this process. Another program holding the chord shows up as
//! `Err(-9878)` (`eventHotKeyExistsErr`) on the line above instead, so the two
//! are never confused.

#[cfg(not(target_os = "macos"))]
fn main() {
    println!(
        "hotkey_probe is macOS-only: it asks Carbon's hot key manager a question \
         no other system has. Nothing was registered."
    );
}

#[cfg(target_os = "macos")]
fn main() {
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;
    use std::io::Write as _;
    use std::ptr;

    // ── Carbon, declared by hand ────────────────────────────────────────────
    //
    // The same declarations `src/hotkey.rs` carries and deliberately **not**
    // shared with it: a probe that imported the product's idea of Carbon would
    // be the proof that already passed. What this file and that one have in
    // common has to be the framework, not a module.

    type EventHandlerRef = *mut c_void;
    type EventHandlerCallRef = *mut c_void;
    type EventRef = *mut c_void;
    type EventTargetRef = *mut c_void;
    type EventHotKeyRef = *mut c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct EventTypeSpec {
        event_class: u32,
        event_kind: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct EventHotKeyID {
        signature: u32,
        id: u32,
    }

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn GetApplicationEventTarget() -> EventTargetRef;
        fn InstallEventHandler(
            target: EventTargetRef,
            handler: unsafe extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> i32,
            number_of_types: u32,
            list: *const EventTypeSpec,
            user_data: *mut c_void,
            installed: *mut EventHandlerRef,
        ) -> i32;
        fn RegisterEventHotKey(
            key_code: u32,
            modifiers: u32,
            id: EventHotKeyID,
            target: EventTargetRef,
            options: u32,
            claimed: *mut EventHotKeyRef,
        ) -> i32;
        fn GetEventParameter(
            event: EventRef,
            name: u32,
            wanted_type: u32,
            actual_type: *mut u32,
            buffer_size: usize,
            actual_size: *mut usize,
            buffer: *mut c_void,
        ) -> i32;
        /// Deprecated since 10.9 and still the whole of what this probe needs: a
        /// main event loop that dispatches Carbon events on the main thread.
        /// `CFRunLoopRun` would turn the run loop without ever draining the
        /// Carbon event queue, which is a program that waits forever for a press
        /// it has already been sent.
        fn RunApplicationEventLoop();
    }

    const NO_ERR: i32 = 0;
    const K_EVENT_CLASS_KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
    const K_EVENT_HOT_KEY_PRESSED: u32 = 5;
    /// **Four hyphens**, and the whole of why this file exists — see the module
    /// note. `'obj '` is `typeObjectSpecifier`, from the Apple event namespace,
    /// and reading a hot key press with it answers `eventParameterNotFoundErr`.
    const K_EVENT_PARAM_DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
    const TYPE_EVENT_HOT_KEY_ID: u32 = u32::from_be_bytes(*b"hkid");
    const CONTROL_KEY: u32 = 0x1000;
    /// `kVK_ANSI_Grave` — the key the product claims.
    const ANSI_GRAVE: u32 = 0x32;
    /// `kVK_ISO_Section` — the key an ISO keyboard has in the same corner.
    const ISO_SECTION: u32 = 0x0A;
    const PROBE_SIGNATURE: u32 = u32::from_be_bytes(*b"prob");

    /// What each id was claimed for, read back by the handler so a press says
    /// which of the two positions it arrived on.
    fn position(id: u32) -> &'static str {
        match id {
            1 => "kVK_ANSI_Grave 0x32",
            2 => "kVK_ISO_Section 0x0A",
            _ => "a claim this probe did not make",
        }
    }

    /// **Every hot key press this process is offered**, printed rather than
    /// judged.
    ///
    /// A signature that is not this probe's is printed too: it means something
    /// else in this address space holds a chord, which is a thing worth seeing
    /// and is invisible from anywhere else.
    unsafe extern "C" fn printed(
        _call: EventHandlerCallRef,
        event: EventRef,
        _user_data: *mut c_void,
    ) -> i32 {
        let mut named = EventHotKeyID {
            signature: 0,
            id: 0,
        };
        // SAFETY: Carbon's contract is that `event` is live for this call; the
        // out-parameter is a local of exactly the declared size; the two nulls
        // are documented as "do not report the actual type / size".
        let status = unsafe {
            GetEventParameter(
                event,
                K_EVENT_PARAM_DIRECT_OBJECT,
                TYPE_EVENT_HOT_KEY_ID,
                ptr::null_mut(),
                size_of::<EventHotKeyID>(),
                ptr::null_mut(),
                (&raw mut named).cast::<c_void>(),
            )
        };
        if status == NO_ERR {
            println!(
                "pressed id={} ({}) signature={}",
                named.id,
                position(named.id),
                four_characters(named.signature)
            );
        } else {
            // The shipped defect, seen from the outside: the handler runs and
            // the press cannot be named.
            println!("pressed, but GetEventParameter answered OSStatus {status}");
        }
        let _ = std::io::stdout().flush();
        NO_ERR
    }

    fn four_characters(value: u32) -> String {
        value
            .to_be_bytes()
            .iter()
            .map(|byte| {
                if byte.is_ascii_graphic() || *byte == b' ' {
                    char::from(*byte)
                } else {
                    '.'
                }
            })
            .collect()
    }

    /// **Install first, then claim, then run** — the order Carbon's contract
    /// wants and the one an `EventHandlerRef` dropped by a guard breaks.
    ///
    /// The reference the install writes is deliberately let go: only
    /// `RemoveEventHandler` removes a handler, and this program's exit is the
    /// only moment it stops being wanted.
    pub fn run() {
        let wanted = EventTypeSpec {
            event_class: K_EVENT_CLASS_KEYBOARD,
            event_kind: K_EVENT_HOT_KEY_PRESSED,
        };
        let mut installed: EventHandlerRef = ptr::null_mut();
        // SAFETY: the target outlives the process; the handler is an `extern
        // "C"` function of this module with Carbon's signature; the list is one
        // live local read for the length of the call; no user data is passed.
        let status = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                printed,
                1,
                &raw const wanted,
                ptr::null_mut(),
                &raw mut installed,
            )
        };
        if status != NO_ERR {
            println!("InstallEventHandler -> Err({status}); nothing was claimed");
            return;
        }
        println!("handler installed");
        let mut claimed_any = false;
        for (id, key_code) in [(1_u32, ANSI_GRAVE), (2, ISO_SECTION)] {
            let mut claim: EventHotKeyRef = ptr::null_mut();
            // SAFETY: two integers by value, a plain struct by value, the
            // application's own target, and an out-parameter that is a local.
            // The claim is held for the life of the process on purpose.
            let status = unsafe {
                RegisterEventHotKey(
                    key_code,
                    CONTROL_KEY,
                    EventHotKeyID {
                        signature: PROBE_SIGNATURE,
                        id,
                    },
                    GetApplicationEventTarget(),
                    0,
                    &raw mut claim,
                )
            };
            // `-9878` is `eventHotKeyExistsErr`: somebody else — very possibly a
            // second copy of Folio — already holds this chord, and that is the
            // answer rather than a failure of the probe.
            let outcome = if status == NO_ERR && !claim.is_null() {
                claimed_any = true;
                "Ok".to_owned()
            } else {
                format!("Err({status})")
            };
            let where_it_is = position(id);
            println!(
                "registered id={id} keycode={key_code:#04x} ({where_it_is}) \
                 modifiers={CONTROL_KEY:#x} -> {outcome}"
            );
        }
        if !claimed_any {
            println!("no claim was accepted; there is nothing to press");
            return;
        }
        println!("press Control and the key left of 1; Ctrl+C to stop");
        let _ = std::io::stdout().flush();
        // SAFETY: takes nothing, answers nothing, and returns only when the
        // event loop is stopped. It must be called on the main thread, which is
        // where `main` put it.
        unsafe { RunApplicationEventLoop() };
    }
}
