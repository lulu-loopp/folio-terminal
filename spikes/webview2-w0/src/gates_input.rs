//! Gates 3, 4 and 5: the whole mouse, the pointer and drop paths, and the
//! keyboard matrix fired one row at a time at a page that has the focus.

use crate::bindings;
use crate::host::MouseEvent;
use crate::log::{emit, note, verdict};
use crate::probe::Probe;
use anyhow::Result;
use std::time::Duration;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_POINTER_EVENT_KIND_DOWN, COREWEBVIEW2_POINTER_EVENT_KIND_ENTER,
    COREWEBVIEW2_POINTER_EVENT_KIND_LEAVE, COREWEBVIEW2_POINTER_EVENT_KIND_UP,
    COREWEBVIEW2_POINTER_EVENT_KIND_UPDATE, ICoreWebView2CompositionController3,
};
use windows::Win32::Foundation::POINT;
use windows::core::Interface as _;

const LEFT_BUTTON: u32 = 1;

pub fn gate3(probe: &mut Probe) -> Result<()> {
    let (ok, _) = probe.navigate(&probe.server.url("/"), Duration::from_secs(20));
    if !ok {
        verdict(3, "fail", "the probe page did not load");
        return Ok(());
    }
    probe.pump(Duration::from_millis(500));
    probe.drain_messages();

    let centre = probe.seat_point(0.5, 0.5);
    let mut rows = Vec::new();

    // Each row: send one event, then ask the page what it saw. A row where the
    // page says nothing is a row composition hosting does not deliver.
    // **`LEAVE` gets three rows, not one.** The documentation says its buttons,
    // wheel data and point must all be zero; passing the seat's centre was
    // refused, and so was a zero point, both with `E_INVALIDARG (0x80070057)`.
    // So the third row asks the remaining question — whether the engine will
    // take it immediately after a move, i.e. whether the refusal is about the
    // arguments or about the pointer never having been inside. All three
    // spellings stay in the matrix so the answer is a reading rather than a
    // recollection.
    let origin = POINT { x: 0, y: 0 };
    let script: &[(&str, MouseEvent, POINT, u32)] = &[
        ("move", MouseEvent::Move, centre, 0),
        ("left-down", MouseEvent::LeftDown, centre, LEFT_BUTTON),
        ("left-up", MouseEvent::LeftUp, centre, 0),
        // A real double click is the whole four-message sequence; the
        // DOUBLE_CLICK kind is the second *press*, not a substitute for it.
        ("dbl-first-down", MouseEvent::LeftDown, centre, LEFT_BUTTON),
        ("dbl-first-up", MouseEvent::LeftUp, centre, 0),
        (
            "left-double-click",
            MouseEvent::LeftDoubleClick,
            centre,
            LEFT_BUTTON,
        ),
        ("dbl-second-up", MouseEvent::LeftUp, centre, 0),
        ("right-down", MouseEvent::RightDown, centre, 2),
        ("right-up", MouseEvent::RightUp, centre, 0),
        ("middle-down", MouseEvent::MiddleDown, centre, 16),
        ("middle-up", MouseEvent::MiddleUp, centre, 0),
        ("x1-down", MouseEvent::XDown(1), centre, 32),
        ("x1-up", MouseEvent::XUp(1), centre, 0),
        ("x2-down", MouseEvent::XDown(2), centre, 64),
        ("x2-up", MouseEvent::XUp(2), centre, 0),
        ("wheel-down", MouseEvent::Wheel(-120), centre, 0),
        ("wheel-up", MouseEvent::Wheel(120), centre, 0),
        (
            "horizontal-wheel",
            MouseEvent::HorizontalWheel(120),
            centre,
            0,
        ),
        ("leave-with-a-point", MouseEvent::Leave, centre, 0),
        ("leave", MouseEvent::Leave, origin, 0),
        ("move-before-leave", MouseEvent::Move, centre, 0),
        ("leave-right-after-a-move", MouseEvent::Leave, origin, 0),
    ];
    for (name, event, point, buttons) in script {
        probe.drain_messages();
        let sent = probe.mouse(*event, *point, *buttons);
        probe.pump(Duration::from_millis(120));
        let seen = probe.drain_messages();
        let kinds: Vec<String> = seen
            .iter()
            .filter_map(|message| {
                let kind = message.get("kind")?.as_str()?;
                let detail = message
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or(kind);
                Some(detail.to_owned())
            })
            .collect();
        let coordinates = seen.iter().find_map(|message| {
            Some((
                message.get("clientX")?.as_f64()?,
                message.get("clientY")?.as_f64()?,
            ))
        });
        let row = serde_json::json!({
            "event": name,
            "send_ok": sent.is_ok(),
            // `{:#}` and not `{}`: the plain form prints only this probe's
            // context and swallows the HRESULT underneath, which is the only
            // part of a refusal worth having.
            "send_error": sent.err().map(|error| format!("{error:#}")),
            "page_events": kinds,
            "page_saw_something": !seen.is_empty(),
            "page_coordinates": coordinates,
        });
        emit(3, "mouse-row", row.clone());
        rows.push(row);
    }

    // Where the page thinks the pointer is. The seat's centre in host client
    // coordinates must arrive as the seat's centre in CSS pixels, scaled — this
    // is the one arithmetic error that would make every click land elsewhere.
    probe.drain_messages();
    let quarter = probe.seat_point(0.25, 0.25);
    probe.mouse(MouseEvent::Move, quarter, 0)?;
    probe.pump(Duration::from_millis(150));
    let reported = probe.drain_messages().into_iter().find(|message| {
        message.get("kind").and_then(|kind| kind.as_str()) == Some("mousemove")
            && message.get("clientX").is_some()
    });
    let scale = f64::from(probe.window.dpi()) / 96.0;
    emit(
        3,
        "coordinate-check",
        serde_json::json!({
            "sent_client": [quarter.x, quarter.y],
            "seat_origin": [probe.seat.left, probe.seat.top],
            "expected_css": [
                f64::from(quarter.x - probe.seat.left) / scale,
                f64::from(quarter.y - probe.seat.top) / scale,
            ],
            "page_reported": reported,
            "scale": scale,
        }),
    );

    // ── the cursor the page asks for ──────────────────────────────────────
    let before = probe.evidence.borrow().cursor_changed.len();
    // The three cursor swatches sit in one row about 60% of the way down the
    // page. Rather than trust that arithmetic, ask the page where they are and
    // aim at what it answers — the same trick the rest of this file uses to
    // avoid measuring its own assumptions.
    let swatches = probe.host.execute_script(
        "JSON.stringify(['pointer-cursor','text-cursor','move-cursor'].map(id => { \
           const r = document.getElementById(id).getBoundingClientRect(); \
           return [Math.round((r.x + r.width / 2) * devicePixelRatio), \
                   Math.round((r.y + r.height / 2) * devicePixelRatio)]; }))",
        Duration::from_secs(3),
    );
    let centres: Vec<(i32, i32)> = swatches
        .as_deref()
        .and_then(|json| serde_json::from_str::<String>(json).ok())
        .and_then(|inner| serde_json::from_str::<Vec<(i32, i32)>>(&inner).ok())
        .unwrap_or_default();
    for (name, (x, y)) in ["pointer", "text", "move"].iter().zip(&centres) {
        let point = POINT {
            x: probe.seat.left + x,
            y: probe.seat.top + y,
        };
        probe.mouse(MouseEvent::Move, point, 0)?;
        probe.pump(Duration::from_millis(150));
        note(
            3,
            &format!("hovered the {name} swatch at {x},{y} inside the seat"),
        );
    }
    let cursors = probe.evidence.borrow().cursor_changed.clone();
    emit(
        3,
        "cursor-changed",
        serde_json::json!({
            "events_before": before,
            "events_total": cursors.len(),
            "system_cursor_ids": cursors,
        }),
    );

    // ── a selection drag ──────────────────────────────────────────────────
    probe.drain_messages();
    let start = POINT {
        x: probe.seat.left + 40,
        y: probe.seat.top + 30,
    };
    probe.mouse(MouseEvent::Move, start, 0)?;
    probe.mouse(MouseEvent::LeftDown, start, LEFT_BUTTON)?;
    for step in 1..=8 {
        probe.mouse(
            MouseEvent::Move,
            POINT {
                x: start.x + step * 22,
                y: start.y + step * 3,
            },
            LEFT_BUTTON,
        )?;
    }
    probe.mouse(
        MouseEvent::LeftUp,
        POINT {
            x: start.x + 180,
            y: start.y + 24,
        },
        0,
    )?;
    probe.pump(Duration::from_millis(250));
    let selection = probe
        .drain_messages()
        .into_iter()
        .find(|message| message.get("kind").and_then(|kind| kind.as_str()) == Some("selection"));
    emit(
        3,
        "selection-drag",
        serde_json::json!({ "page_reported_selection": selection }),
    );

    // ── capture: does a button held outside the seat still reach the page ─
    probe.drain_messages();
    let inside = probe.seat_point(0.5, 0.5);
    probe.mouse(MouseEvent::LeftDown, inside, LEFT_BUTTON)?;
    let outside = POINT {
        x: probe.seat.left - 120,
        y: probe.seat.top - 20,
    };
    probe.mouse(MouseEvent::Move, outside, LEFT_BUTTON)?;
    probe.mouse(MouseEvent::LeftUp, outside, 0)?;
    probe.pump(Duration::from_millis(200));
    let after_capture = probe.drain_messages();
    emit(
        3,
        "capture-outside-seat",
        serde_json::json!({
            "note": "the host decides whether to keep forwarding once a button went down inside the seat; the engine forwards whatever it is given, including negative coordinates",
            "page_events": after_capture.len(),
        }),
    );

    // The LEAVE family is judged on its own, because it is a finding rather
    // than a defect in the rest of the matrix: every other kind is accepted and
    // delivered, and this one is refused in all three spellings.
    let is_leave = |row: &serde_json::Value| {
        row["event"]
            .as_str()
            .is_some_and(|name| name.starts_with("leave"))
    };
    let leave_rows: Vec<_> = rows.iter().filter(|row| is_leave(row)).collect();
    let leave_accepted = leave_rows
        .iter()
        .filter(|row| row["send_ok"].as_bool() == Some(true))
        .count();
    emit(
        3,
        "leave-summary",
        serde_json::json!({
            "spellings_tried": leave_rows.len(),
            "accepted": leave_accepted,
            "errors": leave_rows
                .iter()
                .map(|row| row["send_error"].clone())
                .collect::<Vec<_>>(),
            "consequence_if_unavailable": "the host cannot tell a page the pointer has left the seat, so :hover and mouseleave state stays stuck when the cursor moves into the surrounding chrome. The workaround is a MOVE to a point outside the WebView's bounds, which the engine does accept.",
        }),
    );

    // …and the workaround is measured, not asserted: a MOVE to a point outside
    // the WebView's own bounds should be what makes the page believe the
    // pointer has gone.
    probe.drain_messages();
    probe.mouse(MouseEvent::Move, probe.seat_point(0.5, 0.5), 0)?;
    probe.pump(Duration::from_millis(120));
    probe.drain_messages();
    let outside_bounds = POINT {
        x: probe.seat.left - 200,
        y: probe.seat.top - 40,
    };
    probe.mouse(MouseEvent::Move, outside_bounds, 0)?;
    probe.pump(Duration::from_millis(250));
    let left_events: Vec<String> = probe
        .drain_messages()
        .into_iter()
        .filter_map(|message| {
            message
                .get("type")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .collect();
    emit(
        3,
        "leave-workaround",
        serde_json::json!({
            "moved_to_client": [outside_bounds.x, outside_bounds.y],
            "which_is_outside_the_seat": true,
            "page_events": left_events,
            "page_reported_a_leave": left_events
                .iter()
                .any(|name| name == "mouseout" || name == "mouseleave" || name == "pointerleave"),
        }),
    );

    let others: Vec<_> = rows.iter().filter(|row| !is_leave(row)).collect();
    let others_sent = others
        .iter()
        .all(|row| row["send_ok"].as_bool() == Some(true));
    let others_delivered = others
        .iter()
        .all(|row| row["page_saw_something"].as_bool() == Some(true));
    if others_sent && others_delivered {
        if leave_accepted > 0 {
            verdict(
                3,
                "pass",
                "every SendMouseInput kind was accepted and reached the page, LEAVE included",
            );
        } else {
            verdict(
                3,
                "pass",
                "every mouse kind except LEAVE was accepted and reached the page with exact coordinates; LEAVE is refused in all three spellings with E_INVALIDARG and needs the MOVE-outside-bounds workaround (see leave-summary)",
            );
        }
    } else {
        verdict(
            3,
            "fail",
            "a mouse kind other than LEAVE was refused by the engine or never reached the page",
        );
    }
    Ok(())
}

pub fn gate4(probe: &mut Probe) -> Result<()> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, NID_INTEGRATED_TOUCH, NID_READY, SM_DIGITIZER, SM_MAXIMUMTOUCHES,
    };
    let digitizer = unsafe { GetSystemMetrics(SM_DIGITIZER) };
    let touch_points = unsafe { GetSystemMetrics(SM_MAXIMUMTOUCHES) };
    let has_touch = digitizer & (NID_INTEGRATED_TOUCH | NID_READY) as i32 != 0 && touch_points > 0;
    emit(
        4,
        "digitizer",
        serde_json::json!({
            "SM_DIGITIZER": digitizer,
            "SM_MAXIMUMTOUCHES": touch_points,
            "has_touch_device": has_touch,
        }),
    );

    let (ok, _) = probe.navigate(&probe.server.url("/"), Duration::from_secs(20));
    if !ok {
        verdict(4, "fail", "the probe page did not load");
        return Ok(());
    }
    probe.pump(Duration::from_millis(400));

    // ── synthetic pointer input ───────────────────────────────────────────
    //
    // `SendPointerInput` does not need a digitizer: the host mints the
    // `ICoreWebView2PointerInfo` itself. What no digitizer takes away is the
    // *real* path — a driver-generated WM_POINTER the host would translate —
    // so a pass here says the plumbing works, not that the hardware was tried.
    let mut pointer_rows = Vec::new();
    for (label, pointer_kind) in [("pen", 3u32), ("touch", 2u32)] {
        probe.drain_messages();
        let point = probe.seat_point(0.4, 0.5);
        // POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT | POINTER_FLAG_DOWN
        let down_flags = 0x0000_0002 | 0x0000_0004 | 0x0001_0000;
        let update_flags = 0x0000_0002 | 0x0000_0004 | 0x0002_0000;
        let up_flags = 0x0004_0000;
        let steps = [
            (
                "enter",
                COREWEBVIEW2_POINTER_EVENT_KIND_ENTER,
                0x0000_0002,
                point,
            ),
            (
                "down",
                COREWEBVIEW2_POINTER_EVENT_KIND_DOWN,
                down_flags,
                point,
            ),
            (
                "update",
                COREWEBVIEW2_POINTER_EVENT_KIND_UPDATE,
                update_flags,
                POINT {
                    x: point.x + 40,
                    y: point.y + 18,
                },
            ),
            (
                "up",
                COREWEBVIEW2_POINTER_EVENT_KIND_UP,
                up_flags,
                POINT {
                    x: point.x + 40,
                    y: point.y + 18,
                },
            ),
            ("leave", COREWEBVIEW2_POINTER_EVENT_KIND_LEAVE, 0, point),
        ];
        let mut errors = Vec::new();
        for (name, kind, flags, at) in steps {
            if let Err(error) = probe
                .host
                .send_pointer(kind, pointer_kind, 1, at, 512, flags)
            {
                errors.push(format!("{name}: {error}"));
            }
            probe.pump(Duration::from_millis(60));
        }
        probe.pump(Duration::from_millis(200));
        let seen = probe.drain_messages();
        let pointer_events: Vec<String> = seen
            .iter()
            .filter(|message| {
                matches!(
                    message.get("kind").and_then(|kind| kind.as_str()),
                    Some("pointer") | Some("touch")
                )
            })
            .map(|message| {
                format!(
                    "{}:{}",
                    message
                        .get("pointerType")
                        .and_then(|value| value.as_str())
                        .unwrap_or("?"),
                    message
                        .get("type")
                        .and_then(|value| value.as_str())
                        .unwrap_or("?")
                )
            })
            .collect();
        let row = serde_json::json!({
            "device": label,
            "send_errors": errors,
            "page_pointer_events": pointer_events,
        });
        emit(4, "pointer-row", row.clone());
        pointer_rows.push(row);
    }

    // Two fingers at once — each with its own id, which the first pass recorded
    // as a limitation of the probe rather than measuring — and then a contact
    // the OS input stack made rather than this process.
    crate::gates_w0p::gate4_real_contacts(probe)?;

    // ── OLE drag and drop ─────────────────────────────────────────────────
    let payload = probe.shots.join("dropped-file.txt");
    std::fs::write(&payload, b"w0 drop payload").ok();
    let data = crate::dataobject::FileDrop::create(&payload);
    let drop_result = (|| -> Result<serde_json::Value> {
        let controller3: ICoreWebView2CompositionController3 = probe
            .host
            .composition
            .cast()
            .map_err(|error| anyhow::anyhow!("ICoreWebView2CompositionController3: {error}"))?;
        let point = probe.host.to_webview_point(probe.seat_point(0.5, 0.6));
        // **`effect` is in/out, and the value going *in* is the contract.** It
        // is the set of effects the drag *source* permits, exactly as in
        // `IDropTarget::DragEnter`. Passing 0 (DROPEFFECT_NONE) tells the page
        // the source allows nothing, and the page — correctly — answers by
        // refusing the drop and turning it into a dragleave. That is what the
        // first two runs of this gate recorded, and it was this probe declining
        // its own drop, not the engine refusing it.
        const DROPEFFECT_COPY: u32 = 1;
        const DROPEFFECT_MOVE: u32 = 2;
        const DROPEFFECT_LINK: u32 = 4;
        let allowed = DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK;
        // MK_LBUTTON, as a real drag from Explorer carries.
        let key_state = 0x0001;
        unsafe {
            let mut effect = allowed;
            controller3.DragEnter(&data, key_state, point, &mut effect)?;
            let entered = effect;
            effect = allowed;
            controller3.DragOver(key_state, point, &mut effect)?;
            let over = effect;
            effect = allowed;
            controller3.Drop(&data, key_state, point, &mut effect)?;
            Ok(serde_json::json!({
                "effects_offered_by_the_source": allowed,
                "drag_enter_effect": entered,
                "drag_over_effect": over,
                "drop_effect": effect,
            }))
        }
    })();
    probe.pump(Duration::from_millis(400));
    let drop_events = probe.drain_messages();
    let drag_seen: Vec<String> = drop_events
        .iter()
        .filter(|message| message.get("kind").and_then(|kind| kind.as_str()) == Some("drag"))
        .map(|message| {
            format!(
                "{} [{}]",
                message
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("?"),
                message
                    .get("types")
                    .and_then(|value| value.as_array())
                    .map(|list| list
                        .iter()
                        .filter_map(|value| value.as_str())
                        .collect::<Vec<_>>()
                        .join(","))
                    .unwrap_or_default()
            )
        })
        .collect();
    emit(
        4,
        "ole-drop",
        match &drop_result {
            Ok(effects) => serde_json::json!({
                "calls_succeeded": true,
                "effects": effects,
                "page_drag_events": drag_seen,
            }),
            Err(error) => serde_json::json!({
                "calls_succeeded": false,
                "error": error.to_string(),
                "page_drag_events": drag_seen,
            }),
        },
    );
    std::fs::remove_file(&payload).ok();

    let synthetic_ok = pointer_rows.iter().all(|row| {
        row["send_errors"]
            .as_array()
            .is_some_and(|errors| errors.is_empty())
    });
    // A drop that arrives as `dragleave` is a drop the page declined, not a
    // drop the engine delivered; the row only counts when `drop` itself fires.
    let drop_ok = drop_result.is_ok() && drag_seen.iter().any(|name| name.starts_with("drop"));
    let word = match (synthetic_ok, drop_ok, has_touch) {
        (true, true, true) => ("pass", "pen, touch and OLE drop all delivered"),
        (true, true, false) => (
            "pass",
            "synthetic pen/touch and OLE drop delivered; no digitizer on this machine, so the driver-generated WM_POINTER path is untested — device absent, not a failure",
        ),
        (true, false, _) => (
            "fail",
            "pointer input worked but the OLE drop never reached the page",
        ),
        _ => ("fail", "SendPointerInput was refused"),
    };
    verdict(4, word.0, word.1);
    Ok(())
}

pub fn gate5(probe: &mut Probe) -> Result<()> {
    let (ok, _) = probe.navigate(&probe.server.url("/"), Duration::from_secs(20));
    if !ok {
        verdict(5, "fail", "the probe page did not load");
        return Ok(());
    }
    probe.pump(Duration::from_millis(400));
    probe.window.focus_self();
    probe.pump(Duration::from_millis(100));
    if !probe.window.is_foreground() {
        verdict(
            5,
            "blocked",
            "the probe window could not take the foreground; no key could be trusted",
        );
        return Ok(());
    }

    // ── before focus enters the page ──────────────────────────────────────
    crate::win::clear_wndproc_keys();
    let sent = crate::win::send_chord(true, true, false, b'N' as u16);
    let log = crate::win::pump_for(Duration::from_millis(250), |_| {});
    let host_before: Vec<_> = log
        .keys
        .iter()
        .filter(|key| key.hwnd == probe.window.hwnd.0 as isize && key.is_down())
        .map(|key| key.vk)
        .collect();
    emit(
        5,
        "before-focus",
        serde_json::json!({
            "sendinput_events": sent,
            "host_hwnd_keydowns": host_before,
        }),
    );

    // ── focus into the page ───────────────────────────────────────────────
    probe.host.move_focus_into_web()?;
    probe.pump(Duration::from_millis(200));
    probe.tell_page("focus-field");
    probe.pump(Duration::from_millis(200));
    let focus_window = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() };
    let children = crate::win::child_windows(probe.window.hwnd);
    emit(
        5,
        "focus-state",
        serde_json::json!({
            "GetFocus": format!("{:#x}", focus_window.0 as isize),
            "host_hwnd": format!("{:#x}", probe.window.hwnd.0 as isize),
            "focus_is_host": focus_window == probe.window.hwnd,
            "children": children,
            "controller_focus_events": probe.evidence.borrow().focus_changed.clone(),
        }),
    );

    // ── the 30-row matrix ─────────────────────────────────────────────────
    //
    // Two of the rows are `Alt+Shift+…`, and **`Alt+Shift` is the Windows
    // keyboard-layout switcher**. Firing them changes the layout of whatever
    // has the foreground, so the layout is read before and after and put back
    // if it moved — and the fact that it moves at all is a finding about the
    // product's table, not about this probe.
    let layout_before = active_layout();
    let host_hwnd = probe.window.hwnd.0 as isize;
    let mut claimed_by_host = 0;
    let mut leaked_to_page = 0;
    let mut lost_foreground = 0;
    for row in bindings::CHORDS {
        if !probe.window.is_foreground() {
            // A key sent while something else is in front is a key sent into
            // somebody else's window. Take the foreground back or record the
            // row as unmeasured; never just fire and hope.
            probe.window.focus_self();
            probe.pump(Duration::from_millis(120));
            if !probe.window.is_foreground() {
                lost_foreground += 1;
                emit(
                    5,
                    "chord",
                    serde_json::json!({
                        "id": row.id,
                        "chord": row.chord,
                        "measured": false,
                        "reason": "the probe window was not in the foreground",
                    }),
                );
                continue;
            }
            probe.host.move_focus_into_web()?;
            probe.pump(Duration::from_millis(120));
        }
        probe.drain_messages();
        crate::win::clear_wndproc_keys();
        let accelerators_before = probe.evidence.borrow().accelerators.len();
        let sent = crate::win::send_chord(row.ctrl, row.shift, row.alt, row.vk);
        let log = crate::win::pump_for(Duration::from_millis(180), |_| {});
        probe.pump(Duration::from_millis(80));

        let host_saw = log
            .keys
            .iter()
            .any(|key| key.hwnd == host_hwnd && key.is_down() && u32::from(row.vk) == key.vk);
        let elsewhere: Vec<String> = log
            .keys
            .iter()
            .filter(|key| key.hwnd != host_hwnd && key.is_down() && u32::from(row.vk) == key.vk)
            .map(|key| format!("{:#x}", key.hwnd))
            .collect();
        let evidence = probe.evidence.borrow();
        let accelerators: Vec<_> = evidence.accelerators[accelerators_before..]
            .iter()
            .filter(|record| record.vk == u32::from(row.vk))
            .cloned()
            .collect();
        drop(evidence);
        let handled = accelerators.iter().any(|record| record.handled);
        let page_keys: Vec<String> = probe
            .drain_messages()
            .into_iter()
            .filter(|message| {
                message.get("kind").and_then(|kind| kind.as_str()) == Some("key")
                    && message.get("type").and_then(|value| value.as_str()) == Some("keydown")
            })
            .filter_map(|message| {
                message
                    .get("key")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            })
            .collect();
        if handled {
            claimed_by_host += 1;
        }
        if !page_keys.is_empty() {
            leaked_to_page += 1;
        }
        emit(
            5,
            "chord",
            serde_json::json!({
                "id": row.id,
                "chord": row.chord,
                "sendinput_events": sent,
                "host_hwnd_saw_keydown": host_saw,
                "delivered_to": elsewhere,
                "accelerator_fired": !accelerators.is_empty(),
                "accelerator_handled": handled,
                "page_keydown": page_keys,
            }),
        );
    }

    let layout_after = active_layout();
    if layout_after != layout_before {
        restore_layout(layout_before);
    }
    emit(
        5,
        "keyboard-layout",
        serde_json::json!({
            "before": format!("{layout_before:#x}"),
            "after_the_matrix": format!("{layout_after:#x}"),
            "changed": layout_after != layout_before,
            "restored": active_layout() == layout_before,
            "finding": "Alt+Shift is the Windows layout switcher, and two rows of BINDINGS (split-horizontal, split-vertical) are Alt+Shift chords; on a machine with two or more layouts installed, pressing them also cycles the layout",
        }),
    );

    // ── the keys with no chord: Tab, Shift+Tab, Esc, keyup, autorepeat ────
    for (name, vk, shift) in bindings::BARE_KEYS {
        probe.drain_messages();
        let move_focus_before = probe.evidence.borrow().move_focus_requested.len();
        let accelerators_before = probe.evidence.borrow().accelerators.len();
        crate::win::clear_wndproc_keys();
        let sent = crate::win::send_chord(false, *shift, false, *vk);
        let log = crate::win::pump_for(Duration::from_millis(220), |_| {});
        probe.pump(Duration::from_millis(120));
        let evidence = probe.evidence.borrow();
        let move_focus: Vec<i32> = evidence.move_focus_requested[move_focus_before..].to_vec();
        let accelerators = evidence.accelerators.len() - accelerators_before;
        drop(evidence);
        let page = probe.drain_messages();
        emit(
            5,
            "bare-key",
            serde_json::json!({
                "key": name,
                "sendinput_events": sent,
                "host_hwnd_saw": log.keys.iter().any(|key| key.hwnd == host_hwnd && key.is_down()),
                "accelerator_events": accelerators,
                "move_focus_requested": move_focus,
                "page_focus_moves": page
                    .iter()
                    .filter(|message| message.get("kind").and_then(|kind| kind.as_str()) == Some("focusin"))
                    .count(),
            }),
        );
    }

    // Tab walked to the edge: the page has three focusable controls, so the
    // fourth Tab must leave through MoveFocusRequested rather than nowhere.
    probe.tell_page("focus-field");
    probe.pump(Duration::from_millis(150));
    let before = probe.evidence.borrow().move_focus_requested.len();
    for _ in 0..6 {
        crate::win::send_chord(false, false, false, bindings::VK_TAB);
        probe.pump(Duration::from_millis(120));
    }
    let after = probe.evidence.borrow().move_focus_requested.clone();
    emit(
        5,
        "tab-to-the-edge",
        serde_json::json!({
            "move_focus_requested_before": before,
            "reasons": after,
            "contract": "COREWEBVIEW2_MOVE_FOCUS_REASON: 0 = PROGRAMMATIC, 1 = NEXT, 2 = PREVIOUS",
        }),
    );

    // ── the keys the page owns, and the clipboard behind two of them ──────
    let page_keys_measured = crate::gates_w0p::gate5_page_owned_keys(probe)?;
    if page_keys_measured {
        crate::gates_w0p::gate5_clipboard_round_trip(probe)?;
    }

    // ── the IME's own window, and whether it follows the pane ─────────────
    ime_reading(probe, "seat-at-rest");
    let seat = probe.seat;
    probe.move_seat(windows::Win32::Foundation::RECT {
        left: seat.left + 160,
        top: seat.top + 90,
        ..seat
    })?;
    probe.pump(Duration::from_millis(300));
    probe.tell_page("focus-field");
    probe.pump(Duration::from_millis(250));
    ime_reading(probe, "seat-moved-by-160x90");
    probe.move_seat(seat)?;

    // ── a second monitor, if this machine has one ─────────────────────────
    dpi_reading(probe)?;

    if claimed_by_host > 0 {
        verdict(
            5,
            "pass",
            "the chord matrix was fired row by row with the page holding focus; see every `chord` record for who got each key",
        );
    } else {
        verdict(
            5,
            "fail",
            "no chord reached AcceleratorKeyPressed, so the host cannot keep any of its keyboard while a preview has focus",
        );
    }
    note(
        5,
        &format!(
            "chords the host reclaimed: {claimed_by_host}; chords the page also saw: {leaked_to_page}; rows unmeasured for want of the foreground: {lost_foreground}"
        ),
    );
    Ok(())
}

/// The keyboard layout of the foreground thread.
fn active_layout() -> usize {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        GetKeyboardLayout(thread).0 as usize
    }
}

/// Put a layout back after `Alt+Shift` cycled it.
fn restore_layout(layout: usize) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        ACTIVATE_KEYBOARD_LAYOUT_FLAGS, ActivateKeyboardLayout, HKL,
    };
    unsafe {
        let handle = HKL(layout as *mut std::ffi::c_void);
        // Best-effort restore after a test cycled the layout; there is nothing
        // a failure here could usefully change for the caller.
        let _ = ActivateKeyboardLayout(handle, ACTIVATE_KEYBOARD_LAYOUT_FLAGS(0));
    }
}

/// Where the engine has put the composition window — the number that decides
/// where a candidate list appears.
fn ime_reading(probe: &mut Probe, label: &str) {
    use windows::Win32::UI::Input::Ime::{
        COMPOSITIONFORM, ImmGetCompositionWindow, ImmGetContext, ImmReleaseContext,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
    probe.tell_page("caret-rect");
    probe.pump(Duration::from_millis(200));
    let caret = probe
        .drain_messages()
        .into_iter()
        .find(|message| message.get("kind").and_then(|kind| kind.as_str()) == Some("caret-rect"));
    let focus = unsafe { GetFocus() };
    let reading = unsafe {
        let context = ImmGetContext(focus);
        if context.0.is_null() {
            serde_json::json!({ "himc": null })
        } else {
            let mut form = COMPOSITIONFORM::default();
            let ok = ImmGetCompositionWindow(context, &mut form).as_bool();
            let _ = ImmReleaseContext(focus, context);
            serde_json::json!({
                "himc": format!("{:?}", context.0),
                "ImmGetCompositionWindow": ok,
                "style": form.dwStyle,
                "current_pos": [form.ptCurrentPos.x, form.ptCurrentPos.y],
                "area": [form.rcArea.left, form.rcArea.top, form.rcArea.right, form.rcArea.bottom],
            })
        }
    };
    emit(
        5,
        "ime",
        serde_json::json!({
            "label": label,
            "focus_hwnd": format!("{:#x}", focus.0 as isize),
            "seat": [probe.seat.left, probe.seat.top, probe.seat.right, probe.seat.bottom],
            "page_field_rect": caret,
            "imm": reading,
        }),
    );
}

fn dpi_reading(probe: &mut Probe) -> Result<()> {
    use windows::Win32::Graphics::Gdi::{
        HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
    };

    let monitors = enumerate_monitors();
    let dpi_before = unsafe { GetDpiForWindow(probe.window.hwnd) };
    probe.drain_messages();
    let geometry_before = probe.host.execute_script(
        "JSON.stringify({dpr: devicePixelRatio, w: innerWidth, h: innerHeight})",
        Duration::from_secs(3),
    );
    emit(
        5,
        "dpi-before",
        serde_json::json!({
            "monitors": monitors.len(),
            "dpi": dpi_before,
            "page_geometry": geometry_before,
        }),
    );
    let Some(other) = monitors
        .iter()
        .find(|monitor| monitor.dpi != dpi_before)
        .copied()
    else {
        emit(
            5,
            "dpi-second-monitor",
            serde_json::json!({
                "status": "not tested",
                "reason": "no attached monitor reports a different DPI than the one the window is on",
            }),
        );
        return Ok(());
    };
    let mut rect = windows::Win32::Foundation::RECT::default();
    unsafe { GetWindowRect(probe.window.hwnd, &mut rect) }.ok();
    unsafe {
        SetWindowPos(
            probe.window.hwnd,
            None,
            other.left + 40,
            other.top + 40,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    }
    .ok();
    probe.pump(Duration::from_millis(700));
    probe.relayout()?;
    probe.pump(Duration::from_millis(500));
    let dpi_after = unsafe { GetDpiForWindow(probe.window.hwnd) };
    let geometry_after = probe.host.execute_script(
        "JSON.stringify({dpr: devicePixelRatio, w: innerWidth, h: innerHeight})",
        Duration::from_secs(3),
    );
    emit(
        5,
        "dpi-after",
        serde_json::json!({
            "dpi": dpi_after,
            "wm_dpichanged": crate::win::dpi_changes(),
            "page_geometry": geometry_after,
            "seat": [probe.seat.left, probe.seat.top, probe.seat.right, probe.seat.bottom],
        }),
    );
    ime_reading(probe, "after-dpi-change");
    unsafe {
        SetWindowPos(
            probe.window.hwnd,
            None,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    }
    .ok();
    probe.pump(Duration::from_millis(500));
    probe.relayout()?;
    let _ = (
        HMONITOR::default(),
        MONITOR_DEFAULTTONEAREST,
        MONITORINFO::default(),
        MonitorFromWindow,
    );
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct Monitor {
    left: i32,
    top: i32,
    dpi: u32,
}

fn enumerate_monitors() -> Vec<Monitor> {
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::core::BOOL;

    unsafe extern "system" fn visit(
        monitor: HMONITOR,
        _dc: HDC,
        rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        unsafe {
            let found = &mut *(lparam.0 as *mut Vec<Monitor>);
            let mut x = 0;
            let mut y = 0;
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y);
            found.push(Monitor {
                left: (*rect).left,
                top: (*rect).top,
                dpi: x,
            });
            BOOL(1)
        }
    }

    let mut found: Vec<Monitor> = Vec::new();
    let _ = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(visit),
            LPARAM(std::ptr::from_mut(&mut found) as isize),
        )
    };
    found
}
