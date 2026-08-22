//! W0′ re-verification: the rows the first pass left empty.
//!
//! Everything here belongs to a gate that already exists; it is kept apart
//! because each function answers a question the first run of that gate recorded
//! as *not measured* rather than as *measured and fine*:
//!
//! * gate 2 asked nothing at all about protected media;
//! * gate 4 minted one pointer id and had no driver-generated contact to try;
//! * gate 5 measured only the chords **Folio claims** — never the ones the page
//!   needs, which is the half a browser in a pane lives or dies by;
//! * gate 7 recorded the self-update *trigger* as unforceable and then never
//!   tried the *recovery* the trigger would ask for;
//! * gate 9 never asked what the policy does to `about:blank`, which §4 uses.

use crate::bindings;
use crate::log::{emit, note};
use crate::probe::Probe;
use anyhow::Result;
use std::rc::Rc;
use std::time::{Duration, Instant};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_POINTER_EVENT_KIND_DOWN, COREWEBVIEW2_POINTER_EVENT_KIND_UP,
    COREWEBVIEW2_POINTER_EVENT_KIND_UPDATE,
};
use windows::Win32::Foundation::POINT;

// ── gate 2: protected media ────────────────────────────────────────────────

/// What the engine will say about EME without a licensed stream to play.
///
/// The row this replaces read "not tested" and gave a reason. The reason still
/// holds for the *surface* — no offline page can produce a protected frame — but
/// it never justified leaving the *pipeline* unasked. Whether a Widevine CDM
/// exists in this runtime at all, and whether a composition-hosted WebView will
/// hand out a `MediaKeys`, are both answerable offline and both decide whether
/// a preview pane can show a paid video at all.
pub fn gate2_protected_media(probe: &mut Probe) -> Result<()> {
    probe.drain_messages();
    let script = r#"(async () => {
        const config = [{
            initDataTypes: ['keyids', 'cenc'],
            videoCapabilities: [{ contentType: 'video/mp4; codecs="avc1.42E01E"' }],
            audioCapabilities: [{ contentType: 'audio/mp4; codecs="mp4a.40.2"' }],
        }];
        const ask = async (system) => {
            try {
                const access = await navigator.requestMediaKeySystemAccess(system, config);
                let keys = null;
                try { keys = await access.createMediaKeys(); } catch (error) { return { system, access: true, keys: false, error: String(error) }; }
                let attached = false;
                try {
                    const video = document.createElement('video');
                    await video.setMediaKeys(keys);
                    attached = true;
                } catch (error) { /* attaching is the row, not the run */ }
                return { system, access: true, keys: !!keys, attached,
                         keySystem: access.keySystem };
            } catch (error) {
                return { system, access: false, error: String(error) };
            }
        };
        const result = {
            secureContext: window.isSecureContext,
            hasEme: typeof navigator.requestMediaKeySystemAccess === 'function',
            hasMediaCapabilities: !!(navigator.mediaCapabilities && navigator.mediaCapabilities.decodingInfo),
            systems: [await ask('org.w3.clearkey'), await ask('com.widevine.alpha'),
                      await ask('com.microsoft.playready.recommendation')],
        };
        window.chrome.webview.postMessage({ kind: 'eme', result });
    })()"#;
    probe.host.execute_script(script, Duration::from_secs(5));
    probe.pump(Duration::from_secs(3));
    let answer = probe
        .drain_messages()
        .into_iter()
        .find(|message| message.get("kind").and_then(|kind| kind.as_str()) == Some("eme"));
    emit(
        2,
        "protected-media",
        serde_json::json!({
            "page": answer,
            "still_untested": "a protected *surface*: whether a licensed stream composites or comes back black through CapturePreview needs a licensed stream, and no offline page is one",
            "why_it_matters": "a key system that is absent here is a video that will not play in the pane at all, which is a product answer even without a stream",
        }),
    );
    Ok(())
}

// ── gate 4: the driver's own contact, and two of them ───────────────────────

/// The two rows gate 4 recorded as limitations of the probe rather than of the
/// engine: one pointer id, and no driver-generated `WM_POINTER`.
pub fn gate4_real_contacts(probe: &mut Probe) -> Result<()> {
    // ── two contacts, each with its own id, in one frame ──────────────────
    probe.drain_messages();
    let frame = probe.host.next_pointer_frame();
    let contacts = [
        (1u32, probe.seat_point(0.35, 0.55)),
        (2u32, probe.seat_point(0.60, 0.55)),
    ];
    // POINTER_FLAG_DOWN | INRANGE | INCONTACT, then UPDATE, then UP.
    let down = 0x0001_0000 | 0x0000_0002 | 0x0000_0004;
    let update = 0x0002_0000 | 0x0000_0002 | 0x0000_0004;
    let up = 0x0004_0000;
    let mut errors = Vec::new();
    for (id, point) in contacts {
        if let Err(error) = probe.host.send_pointer(
            COREWEBVIEW2_POINTER_EVENT_KIND_DOWN,
            2,
            id,
            point,
            512,
            down,
        ) {
            errors.push(format!("down {id}: {error}"));
        }
    }
    probe.pump(Duration::from_millis(80));
    for (id, point) in contacts {
        let moved = POINT {
            x: point.x,
            y: point.y - 24,
        };
        if let Err(error) = probe.host.send_pointer(
            COREWEBVIEW2_POINTER_EVENT_KIND_UPDATE,
            2,
            id,
            moved,
            512,
            update,
        ) {
            errors.push(format!("update {id}: {error}"));
        }
    }
    probe.pump(Duration::from_millis(80));
    for (id, point) in contacts {
        if let Err(error) =
            probe
                .host
                .send_pointer(COREWEBVIEW2_POINTER_EVENT_KIND_UP, 2, id, point, 0, up)
        {
            errors.push(format!("up {id}: {error}"));
        }
    }
    probe.pump(Duration::from_millis(300));
    let seen = probe.drain_messages();
    let ids: Vec<u64> = seen
        .iter()
        .filter(|message| message.get("kind").and_then(|kind| kind.as_str()) == Some("pointer"))
        .filter_map(|message| message.get("pointerId").and_then(serde_json::Value::as_u64))
        .collect();
    let mut distinct = ids.clone();
    distinct.sort_unstable();
    distinct.dedup();
    let most_touches = seen
        .iter()
        .filter(|message| message.get("kind").and_then(|kind| kind.as_str()) == Some("touch"))
        .filter_map(|message| message.get("touches").and_then(serde_json::Value::as_u64))
        .max()
        .unwrap_or(0);
    emit(
        4,
        "two-contacts",
        serde_json::json!({
            "frame_id": frame,
            "send_errors": errors,
            "pointer_ids_the_page_saw": distinct,
            "most_simultaneous_touches": most_touches,
            "reading": "two ids and a TouchList of two is a host that can carry a pinch; one id is a mouse with a different name",
        }),
    );

    // ── a contact the driver made ─────────────────────────────────────────
    //
    // Injected touch is delivered to whichever window owns the pixels under the
    // point, so this refuses to fire unless those pixels are demonstrably ours.
    // The 2026-08 ui-probe rule — verify pixel ownership before pressing — is
    // the same rule here, and the cost of ignoring it is a stranger's window
    // being touched.
    let seat_point = probe.seat_point(0.5, 0.45);
    let screen = crate::win::client_to_screen(probe.window.hwnd, seat_point);
    let second = crate::win::client_to_screen(
        probe.window.hwnd,
        POINT {
            x: seat_point.x + 120,
            y: seat_point.y,
        },
    );
    let owner_first = crate::win::root_window_from_point(screen);
    let owner_second = crate::win::root_window_from_point(second);
    let ours = probe.window.hwnd.0 as isize;
    let safe = owner_first == ours && owner_second == ours;
    if !safe {
        emit(
            4,
            "injected-touch",
            serde_json::json!({
                "attempted": false,
                "reason": "the pixels under the contact points do not belong to this window; injecting there would touch somebody else's window",
                "owner_of_first_point": format!("{owner_first:#x}"),
                "owner_of_second_point": format!("{owner_second:#x}"),
                "host_hwnd": format!("{ours:#x}"),
            }),
        );
        return Ok(());
    }
    let initialized = crate::inject::touch::initialize(2);
    let mut injection_errors = Vec::new();
    let mut pointer_messages = Vec::new();
    if initialized.is_ok() {
        crate::win::clear_wndproc_keys();
        probe.drain_messages();
        injection_errors =
            crate::inject::touch::drag_together(&[screen, second], POINT { x: 0, y: -30 });
        let log = crate::win::pump_for(Duration::from_millis(500), |_| {});
        pointer_messages = log.pointers;
    }
    let mut injected_contacts: Vec<u32> =
        pointer_messages.iter().map(|row| row.pointer_id).collect();
    injected_contacts.sort_unstable();
    injected_contacts.dedup();
    emit(
        4,
        "injected-touch",
        serde_json::json!({
            "attempted": true,
            "InitializeTouchInjection": match &initialized {
                Ok(()) => serde_json::json!("ok"),
                Err(error) => serde_json::json!({ "error": error }),
            },
            "InjectTouchInput_errors": injection_errors,
            "wm_pointer_messages_at_the_host": pointer_messages,
            "distinct_contacts": injected_contacts,
            "reading": "a WM_POINTER row here is the driver path this machine has no digitizer for; the host still has to translate it into SendPointerInput, which the rows above measure separately",
        }),
    );
    Ok(())
}

// ── gate 5: the keys the page owns ─────────────────────────────────────────

/// One row of the page-owned matrix.
struct PageKey {
    label: &'static str,
    ctrl: bool,
    shift: bool,
    alt: bool,
    vk: u16,
    /// What `KeyboardEvent.key` the page should report if it got the key
    /// itself, as opposed to only the modifiers around it.
    expect: &'static str,
}

const fn page_key(
    label: &'static str,
    ctrl: bool,
    shift: bool,
    alt: bool,
    vk: u16,
    expect: &'static str,
) -> PageKey {
    PageKey {
        label,
        ctrl,
        shift,
        alt,
        vk,
        expect,
    }
}

/// The other half of gate 5.
///
/// The first pass fired all thirty rows of `BINDINGS` and proved the host can
/// take any of them back. It never fired a key `BINDINGS` does **not** claim —
/// and those are the keys a browser in a pane is for. `Ctrl+C` that never
/// reaches the page is a preview nobody can copy a line out of.
pub fn gate5_page_owned_keys(probe: &mut Probe) -> Result<bool> {
    if !probe.window.is_foreground() {
        probe.window.focus_self();
        probe.pump(Duration::from_millis(150));
    }
    if !probe.window.is_foreground() {
        emit(
            5,
            "page-owned-keys",
            serde_json::json!({
                "measured": false,
                "reason": "the probe window was not in the foreground; SendInput would have gone to somebody else's window",
            }),
        );
        return Ok(false);
    }
    probe.host.move_focus_into_web()?;
    probe.pump(Duration::from_millis(150));
    probe.tell_page("focus-field");
    probe.pump(Duration::from_millis(150));

    let rows = [
        page_key("Ctrl+C", true, false, false, b'C' as u16, "c"),
        page_key("Ctrl+V", true, false, false, b'V' as u16, "v"),
        page_key("Ctrl+X", true, false, false, b'X' as u16, "x"),
        page_key("Ctrl+A", true, false, false, b'A' as u16, "a"),
        page_key("Ctrl+Z", true, false, false, b'Z' as u16, "z"),
        page_key("Ctrl+Y", true, false, false, b'Y' as u16, "y"),
        // The contrast row: this one *is* in BINDINGS, fired in the same block
        // under the same conditions, so "the page did not get it" cannot be
        // blamed on the measurement.
        page_key("Ctrl+Shift+Z", true, true, false, b'Z' as u16, "Z"),
        page_key("Alt (bare)", false, false, true, 0x12, "Alt"),
        page_key("F5", false, false, false, 0x74, "F5"),
        page_key("Ctrl+R", true, false, false, b'R' as u16, "r"),
        page_key("F12", false, false, false, 0x7B, "F12"),
        page_key("Ctrl+P", true, false, false, b'P' as u16, "p"),
        page_key("Alt+Left", false, false, true, 0x25, "ArrowLeft"),
        page_key("Alt+Right", false, false, true, 0x27, "ArrowRight"),
    ];

    let host_hwnd = probe.window.hwnd.0 as isize;
    let mut measured = Vec::new();
    for row in &rows {
        if !probe.window.is_foreground() {
            probe.window.focus_self();
            probe.pump(Duration::from_millis(120));
            probe.host.move_focus_into_web()?;
            probe.pump(Duration::from_millis(120));
        }
        probe.drain_messages();
        crate::win::clear_wndproc_keys();
        let before = probe.evidence.borrow().accelerators.len();
        let sent = crate::win::send_chord(row.ctrl, row.shift, row.alt, row.vk);
        let log = crate::win::pump_for(Duration::from_millis(200), |_| {});
        probe.pump(Duration::from_millis(120));

        let evidence = probe.evidence.borrow();
        let records: Vec<_> = evidence.accelerators[before..]
            .iter()
            .filter(|record| record.vk == u32::from(row.vk))
            .cloned()
            .collect();
        drop(evidence);
        let claimed_by_host = records.iter().any(|record| record.handled);
        let kinds: Vec<i32> = records.iter().map(|record| record.kind).collect();
        let page: Vec<serde_json::Value> = probe
            .drain_messages()
            .into_iter()
            .filter(|message| message.get("kind").and_then(|kind| kind.as_str()) == Some("key"))
            .collect();
        let page_keys: Vec<String> = page
            .iter()
            .filter_map(|message| {
                let name = message.get("key")?.as_str()?;
                let kind = message.get("type")?.as_str()?;
                Some(format!("{kind}:{name}"))
            })
            .collect();
        // The distinction the first pass's counter blurred: a page that reports
        // `Control` saw a modifier, not the chord.
        let page_saw_the_key = page
            .iter()
            .any(|message| message.get("key").and_then(|value| value.as_str()) == Some(row.expect));
        let row = serde_json::json!({
            "chord": row.label,
            "in_bindings": bindings::claims(u32::from(row.vk), row.ctrl, row.shift, row.alt),
            "sendinput_events": sent,
            "accelerator_fired": !records.is_empty(),
            "accelerator_kinds": kinds,
            "host_claimed_it": claimed_by_host,
            "page_saw_the_key_itself": page_saw_the_key,
            "page_key_events": page_keys,
            "host_hwnd_saw_a_keydown": log
                .keys
                .iter()
                .any(|key| key.hwnd == host_hwnd && key.is_down()),
            "who_got_it": if claimed_by_host {
                "host"
            } else if page_saw_the_key {
                "page"
            } else {
                "neither"
            },
        });
        emit(5, "page-owned-key", row.clone());
        measured.push(row);
    }
    emit(
        5,
        "page-owned-keys",
        serde_json::json!({
            "measured": true,
            "rows": measured.len(),
            "kind_legend": "COREWEBVIEW2_KEY_EVENT_KIND: 0 = KEY_DOWN, 1 = KEY_UP, 2 = SYSTEM_KEY_DOWN, 3 = SYSTEM_KEY_UP",
            "contract": "a row whose `in_bindings` is false and whose `who_got_it` is not `page` is a key the preview block cannot offer",
        }),
    );
    Ok(true)
}

/// Whether `Ctrl+C` and `Ctrl+V` did what they say, measured on the clipboard
/// rather than on the keydown.
pub fn gate5_clipboard_round_trip(probe: &mut Probe) -> Result<()> {
    // Whatever the person had is put back at the end of this function.
    let theirs = crate::inject::clipboard::text();

    const TYPED: &str = "w0p clipboard round trip";
    probe.tell_page(&format!("set-field:{TYPED}"));
    probe.pump(Duration::from_millis(250));
    probe.drain_messages();

    crate::win::send_chord(true, false, false, b'A' as u16);
    probe.pump(Duration::from_millis(200));
    probe.tell_page("report-field");
    probe.pump(Duration::from_millis(200));
    let after_select_all = probe
        .drain_messages()
        .into_iter()
        .find(|message| message.get("kind").and_then(|kind| kind.as_str()) == Some("field"));

    crate::inject::clipboard::set_text("w0p clipboard was not written");
    crate::win::send_chord(true, false, false, b'C' as u16);
    probe.pump(Duration::from_millis(400));
    let after_copy = crate::inject::clipboard::text();

    const PASTED: &str = "w0p pasted by the host";
    crate::inject::clipboard::set_text(PASTED);
    probe.tell_page("set-field:");
    probe.pump(Duration::from_millis(200));
    probe.drain_messages();
    crate::win::send_chord(true, false, false, b'V' as u16);
    probe.pump(Duration::from_millis(400));
    probe.tell_page("report-field");
    probe.pump(Duration::from_millis(200));
    let after_paste = probe
        .drain_messages()
        .into_iter()
        .find(|message| message.get("kind").and_then(|kind| kind.as_str()) == Some("field"));

    let pasted_value = after_paste
        .as_ref()
        .and_then(|message| message.get("value"))
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_owned();
    emit(
        5,
        "clipboard-round-trip",
        serde_json::json!({
            "typed_into_the_page": TYPED,
            "after_ctrl_a": after_select_all,
            "clipboard_after_ctrl_c": after_copy,
            "copy_worked": after_copy.as_deref() == Some(TYPED),
            "host_put_on_the_clipboard": PASTED,
            "field_after_ctrl_v": pasted_value,
            "paste_worked": pasted_value == PASTED,
            "why_the_clipboard_and_not_the_keydown": "a keydown the page receives and does nothing with looks identical to one it acts on; the clipboard is the only place the difference shows",
        }),
    );

    match theirs {
        Some(text) => {
            crate::inject::clipboard::set_text(&text);
        }
        None => {
            // Leaving this probe's string on an otherwise empty clipboard would
            // be leaving litter in somebody's session.
            crate::inject::clipboard::set_text("");
        }
    }
    Ok(())
}

// ── gate 7: the rebuild a runtime update would ask for ──────────────────────

/// The self-update *recovery*, run for real without the trigger.
///
/// `NewBrowserVersionAvailable` cannot be provoked: it needs Evergreen to
/// install a build while this process runs, and installing one to order would
/// be a change to the machine. What the handler would then have to **do** is
/// entirely provokable, and it is the part that can be got wrong: close every
/// controller, wait for the browser to go, release the cached environment, and
/// build the seat again on the same `HWND` — no window torn down, no pane
/// closed, the same last good URL underneath.
pub fn gate7_self_update_rebuild(probe: &mut Probe) -> Result<()> {
    let url = probe.server.url("/");
    let (loaded, _) = probe.navigate(&url, Duration::from_secs(20));
    let version_before = crate::host::runtime_version();
    let pid_before = probe.host.browser_process_id();
    let hwnd_before = probe.window.hwnd.0 as isize;
    let started = Instant::now();

    let rebuild = probe.rebuild_host();
    probe.pump(Duration::from_millis(400));
    let (reloaded, landed) = match &rebuild {
        Ok(_) => probe.navigate(&url, Duration::from_secs(20)),
        Err(_) => (false, String::new()),
    };
    let pid_after = if rebuild.is_ok() {
        probe.host.browser_process_id()
    } else {
        0
    };
    emit(
        7,
        "self-update-rebuild",
        serde_json::json!({
            "trigger": "not forced — NewBrowserVersionAvailable needs Evergreen to install a build under a running process",
            "recovery_exercised": rebuild.is_ok(),
            "error": rebuild.as_ref().err().map(|error| format!("{error:#}")),
            "page_loaded_before": loaded,
            "runtime_version": version_before.as_ref().ok(),
            "browser_pid_before": pid_before,
            "browser_pid_after": pid_after,
            "browser_process_was_replaced": pid_before != pid_after && pid_after != 0,
            "host_hwnd_before": format!("{hwnd_before:#x}"),
            "host_hwnd_after": format!("{:#x}", probe.window.hwnd.0 as isize),
            "window_survived": hwnd_before == probe.window.hwnd.0 as isize,
            "reloaded_the_last_good_url": reloaded,
            "landed_on": landed,
            "elapsed_ms": started.elapsed().as_millis(),
            "reading": "the strategy §2 asks for — rebuild without restarting the window — is what this row executes; only the event that would start it is out of reach",
        }),
    );
    Ok(())
}

// ── gate 9: the scheme §4 uses on itself ───────────────────────────────────

/// `about:blank` through the same door as everything else.
///
/// §3's allowlist refuses `about:`, and §4 names `about:blank` as a thing the
/// preview navigates to and must not record as recoverable. Both cannot be
/// true of the same door, and which one gives is a decision somebody has to
/// make with a reading in front of them rather than at the keyboard.
pub fn gate9_about_blank(probe: &mut Probe) -> Result<()> {
    let policy = crate::policy::navigation_starting("about:blank", None);
    let before = probe.evidence.borrow().nav_starting.len();
    // Enforcement off for this one navigation: the question is what the engine
    // does with it, which a cancel from our own handler would hide.
    probe.evidence.borrow_mut().enforce_policy = false;
    let _ = probe.host.navigate("about:blank");
    let evidence = Rc::clone(&probe.evidence);
    crate::win::pump_until(Duration::from_secs(6), || {
        evidence.borrow().nav_starting.len() > before
    });
    probe.pump(Duration::from_millis(400));
    let starting: Vec<_> = probe.evidence.borrow().nav_starting[before..].to_vec();
    probe.evidence.borrow_mut().enforce_policy = true;
    let source = probe.host.source();
    emit(
        9,
        "about-blank",
        serde_json::json!({
            "policy_says": format!("{policy:?}"),
            "navigation_starting_fired": !starting.is_empty(),
            "navigation_starting": starting,
            "source_after": source,
            "the_conflict": "policy::navigation_starting refuses about: as a browser-internal scheme, and §4 navigates to about:blank and expects it to load without becoming the recoverable URL. If NavigationStarting fires for it, the enforcing handler cancels a navigation the product itself asked for",
        }),
    );
    note(
        9,
        "about:blank went through the same door §3 built for typed addresses; the row above says whether that door fires on it",
    );
    Ok(())
}
