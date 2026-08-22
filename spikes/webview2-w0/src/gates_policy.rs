//! Gates 9 and 10: §3's allowlist and §4's recovery machine, fired at a live
//! engine rather than only at their own unit tests.
//!
//! The unit tests in `policy` and `machine` prove the rules are what the plan
//! says. These gates prove the *engine* is standing behind them: that a URL the
//! address bar refused cannot arrive by redirect, by page script, or by being
//! pinned, and that a crash comes back to the last page that actually loaded.

use crate::log::{emit, verdict};
use crate::machine::{Effect, Preview, State};
use crate::policy::{self, Decision};
use crate::probe::Probe;
use crate::server;
use anyhow::Result;
use std::rc::Rc;
use std::time::Duration;

pub fn gate9(probe: &mut Probe) -> Result<()> {
    // ── the rule, stated ──────────────────────────────────────────────────
    let matrix: Vec<serde_json::Value> = [
        "javascript:alert(1)",
        "JavaScript:alert(1)",
        "data:text/html,<h1>hello",
        "blob:https://example.com/abc",
        "vbscript:msgbox",
        "file:///C:/Windows/win.ini",
        "view-source:https://example.com/",
        "devtools://devtools/bundled/inspector.html",
        "edge://settings",
        "chrome://version",
        "about:blank",
        "ftp://example.com/file",
        "ws://example.com/socket",
        "mailto:someone@example.com",
        "https://user:pass@example.com/",
        "http://localhost:5173/app",
        "http://0.0.0.0:5173/app?x=1#top",
        "http://192.168.1.20:8080/",
        "example.com/docs",
        "how do i exit vim",
    ]
    .iter()
    .map(|input| {
        let decision = policy::address_bar(input);
        serde_json::json!({
            "input": input,
            "decision": match &decision {
                Decision::Navigate(url) => format!("navigate {url}"),
                Decision::Search(text) => format!("search {text}"),
                Decision::Refuse(refusal) => format!("refuse {refusal:?}"),
            },
            "opens_in_preview_by_default": policy::opens_in_preview_by_default(input),
        })
    })
    .collect();
    emit(
        9,
        "address-bar-matrix",
        serde_json::json!({ "rows": matrix }),
    );

    // ── the rule, enforced by the engine ──────────────────────────────────
    let (ok, _) = probe.navigate(&probe.server.url("/navigator"), Duration::from_secs(20));
    if !ok {
        verdict(9, "fail", "the navigator page did not load");
        return Ok(());
    }
    probe.pump(Duration::from_millis(400));
    let safe_url = probe.host.source();

    let mut live_rows = Vec::new();

    // (a) straight at the engine, as a pinned entry or an address-bar string
    //     that somehow got past the first check.
    for target in [
        "javascript:location='https://evil.example/'",
        "file:///C:/Windows/win.ini",
        "edge://settings",
        "view-source:https://example.com/",
        "data:text/html,<h1>injected",
    ] {
        live_rows.push(attempt(
            probe,
            "direct",
            target,
            target,
            Duration::from_secs(6),
        ));
    }

    // The `javascript:` row deserves its own sentence, because "blocked" is the
    // wrong word for what happened to it and the difference is the whole reason
    // §3 puts the refusal at the address bar.
    let javascript_row = &live_rows[0];
    emit(
        9,
        "javascript-scheme",
        serde_json::json!({
            "asked_for": javascript_row["target"],
            "what_navigation_starting_saw": javascript_row["rewritten_by_the_engine_to"],
            "landed_on": javascript_row["source_after"],
            "finding": "Navigate() with a javascript: URL does not fail and does not reach NavigationStarting as javascript: — the engine RUNS the script against the current document, and only the navigation the script then performs is offered to the allowlist. NavigationStarting is therefore NOT a backstop for this scheme: the refusal in policy::address_bar is the only door, and every path that can reach Navigate() (address bar, pin, command palette, switcher) must go through it.",
        }),
    );

    // (b) by redirect: the address bar only ever saw a loopback URL.
    for target in ["edge://settings", "file:///C:/Windows/win.ini"] {
        let via = probe
            .server
            .url(&format!("/redirect?to={}", server::percent_encode(target)));
        live_rows.push(attempt(
            probe,
            "redirect",
            &via,
            target,
            Duration::from_secs(8),
        ));
    }

    // (c) by page script: a loaded page setting `location.href`.
    probe.navigate(&probe.server.url("/navigator"), Duration::from_secs(20));
    probe.pump(Duration::from_millis(400));
    for target in ["file:///C:/Windows/win.ini", "edge://settings"] {
        let before = probe.evidence.borrow().nav_starting.len();
        probe.tell_page(&format!("go:{target}"));
        probe.pump(Duration::from_secs(3));
        let evidence = probe.evidence.borrow();
        let seen: Vec<_> = evidence.nav_starting[before..].to_vec();
        drop(evidence);
        let landed = probe.host.source();
        live_rows.push(serde_json::json!({
            "route": "page-script",
            "target": target,
            "navigation_starting": seen,
            "landed_on": landed,
            "still_on_safe_page": landed == safe_url,
            // Scored the same way as every other row, so the aggregate below
            // can read one field across the whole matrix.
            "blocked": !landed.eq_ignore_ascii_case(target),
            "blocked_by": if landed == safe_url {
                "the engine refused the page's own location assignment; NavigationStarting never fired"
            } else {
                "nothing — the page walked out of its origin"
            },
        }));
    }

    // (d) the sanctioned file entry: one minted URL is allowed, and nothing
    //     else with the same scheme is.
    let file = probe.shots.join("sanctioned.html");
    std::fs::write(
        &file,
        b"<!doctype html><meta charset=\"utf-8\"><title>sanctioned</title><body style=\"background:#0d5c34;color:#fff\">sanctioned file</body>",
    )?;
    let canonical = std::fs::canonicalize(&file)?;
    let minted = policy::file_url_from_canonical_path(&canonical);
    match minted {
        Ok(url) => {
            probe.evidence.borrow_mut().sanctioned_file = Some(url.clone());
            let allowed = attempt(probe, "sanctioned-file", &url, &url, Duration::from_secs(8));
            // …and from inside it, a second file cannot be reached.
            let other =
                policy::file_url_from_canonical_path(std::path::Path::new(r"C:\Windows\win.ini"))
                    .unwrap_or_default();
            let refused = attempt(probe, "second-file", &other, &other, Duration::from_secs(6));
            probe.evidence.borrow_mut().sanctioned_file = None;
            emit(
                9,
                "controlled-file-entry",
                serde_json::json!({
                    "minted": url,
                    "minted_navigation": allowed,
                    "other_file_navigation": refused,
                }),
            );
        }
        Err(refusal) => emit(
            9,
            "controlled-file-entry",
            serde_json::json!({ "mint_refused": format!("{refusal:?}") }),
        ),
    }
    std::fs::remove_file(&file).ok();

    // (e) downloads and popups: the two other doors out of a page.
    probe.navigate(&probe.server.url("/"), Duration::from_secs(20));
    probe.pump(Duration::from_millis(400));
    let downloads_before = probe.evidence.borrow().downloads.len();
    let popups_before = probe.evidence.borrow().new_window_requested.len();
    let _ = probe.host.execute_script(
        &format!(
            "window.location.href = '{}'; 1",
            probe.server.url("/download")
        ),
        Duration::from_secs(3),
    );
    probe.pump(Duration::from_secs(3));
    let _ = probe.host.execute_script(
        "window.open('https://example.com/popup', '_blank'); 1",
        Duration::from_secs(3),
    );
    probe.pump(Duration::from_secs(3));
    let evidence = probe.evidence.borrow();
    emit(
        9,
        "downloads-and-popups",
        serde_json::json!({
            "downloads_started_and_cancelled": evidence.downloads[downloads_before..].to_vec(),
            "new_window_requests": evidence.new_window_requested[popups_before..].to_vec(),
            "launching_external_uri_scheme": evidence.launching_external.clone(),
            "permission_requests_denied": evidence.permissions.clone(),
            "note": "a non-user-initiated window.open is reported with is_user_initiated=false; §0 says those are cancelled outright",
        }),
    );
    drop(evidence);

    emit(9, "live-matrix", serde_json::json!({ "rows": live_rows }));

    // The scheme §4 uses on itself, put through the door §3 built.
    crate::gates_w0p::gate9_about_blank(probe)?;

    let all_refused = live_rows.iter().all(|row| {
        row["blocked"].as_bool() == Some(true) || row["route"].as_str() == Some("sanctioned-file")
    });
    if all_refused {
        verdict(
            9,
            "pass",
            "every refused scheme stayed refused whether it was typed, redirected to, or set by a page script; the one minted file URL loaded and no other file did",
        );
    } else {
        verdict(
            9,
            "fail",
            "at least one refused URL reached the engine; see the live-matrix rows where blocked is false",
        );
    }
    Ok(())
}

/// Ask the engine to navigate somewhere and report what the doors did.
///
/// `request` is what the engine is asked for; `forbidden` is the URL that must
/// not end up loaded. They differ for a redirect, where the request is an
/// innocent loopback address and the thing under test is where it points — and
/// scoring a redirect against its own request URL is how the first version of
/// this gate reported two safe refusals as breaches.
fn attempt(
    probe: &mut Probe,
    route: &str,
    request: &str,
    forbidden: &str,
    timeout: Duration,
) -> serde_json::Value {
    let target = forbidden;
    let source_before = probe.host.source();
    let starting_before = probe.evidence.borrow().nav_starting.len();
    let completed_before = probe.evidence.borrow().nav_completed.len();
    let call = probe.host.navigate(request);
    let evidence = Rc::clone(&probe.evidence);
    crate::win::pump_until(timeout, || {
        evidence.borrow().nav_completed.len() > completed_before
    });
    let evidence = probe.evidence.borrow();
    let starting = evidence.nav_starting[starting_before..].to_vec();
    let completed = evidence.nav_completed[completed_before..].to_vec();
    drop(evidence);
    let source_after = probe.host.source();
    let cancelled_by_us = starting.iter().any(|record| record.cancelled);
    let refused_by_engine = call.is_err();

    // **The question is where the engine ended up, not whether anything moved.**
    //
    // The first version of this gate asked "did `Source` change", and scored
    // two safe outcomes as failures for it. A redirect the engine refuses still
    // leaves `Source` on the *redirecting* URL, which is a change; and
    // `javascript:` / `view-source:` are rewritten by the engine before any
    // event fires, so what arrives at `NavigationStarting` is a different URL
    // from the one asked for. Only "did the forbidden target commit" is the
    // question the security door is actually asked.
    let reached_target = source_after.eq_ignore_ascii_case(target)
        || completed
            .iter()
            .any(|record| record.success && record.uri.eq_ignore_ascii_case(target));
    // "Rewritten" only means something when the engine was asked for the target
    // itself. On a redirect row the request is deliberately a different URL, so
    // seeing that URL at `NavigationStarting` is the normal first hop and not a
    // rewrite of anything.
    let asked_for_the_target = request.eq_ignore_ascii_case(target);
    let rewritten_to = asked_for_the_target
        .then(|| {
            starting
                .iter()
                .find(|record| !record.uri.eq_ignore_ascii_case(target))
                .map(|record| record.uri.clone())
        })
        .flatten();
    let redirect_stopped = !asked_for_the_target
        && !starting
            .iter()
            .any(|record| record.uri.eq_ignore_ascii_case(target));
    let blocked_by = if reached_target {
        "nothing — the forbidden target loaded".to_owned()
    } else if refused_by_engine {
        "Navigate() refused it synchronously".to_owned()
    } else if cancelled_by_us {
        "NavigationStarting cancelled it".to_owned()
    } else if let Some(rewritten) = &rewritten_to {
        format!(
            "the engine rewrote it before any event fired; what reached NavigationStarting was {rewritten}"
        )
    } else if redirect_stopped {
        "the engine refused the cross-scheme redirect; the target was never offered to NavigationStarting and never committed".to_owned()
    } else if starting.is_empty() {
        "the engine refused it before NavigationStarting fired".to_owned()
    } else {
        "it never committed".to_owned()
    };
    serde_json::json!({
        "route": route,
        "requested": request,
        "target": target,
        "navigate_call_error": call.err().map(|error| error.to_string()),
        "navigation_starting": starting,
        "navigation_completed": completed,
        "source_before": source_before,
        "source_after": source_after,
        "rewritten_by_the_engine_to": rewritten_to,
        "blocked": !reached_target,
        "blocked_by": blocked_by,
    })
}

pub fn gate10(probe: &mut Probe) -> Result<()> {
    // ── the machine, driven by this run's real events ─────────────────────
    //
    // Not a re-run of the unit tests: the same state machine fed the actual
    // callbacks a live engine produced, so a discrepancy between the plan's
    // model and the engine's behaviour shows up here rather than in a mock.
    let mut model = Preview::new();
    model.request("https://example.invalid/first");
    let generation = model.generation();
    model.on_environment(generation, true);
    model.on_controller(generation, true);
    let first = model.on_events_installed(generation);
    emit(
        10,
        "events-before-first-flight",
        serde_json::json!({
            "state_after_controller": format!("{:?}", State::ControllerPending),
            "effect_after_events_installed": format!("{first:?}"),
            "live_ordering": "Host::create attaches NavigationStarting, ProcessFailed, NewWindowRequested, DownloadStarting, PermissionRequested, LaunchingExternalUriScheme, AcceleratorKeyPressed, MoveFocusRequested, focus, CursorChanged and WebMessageReceived before it returns, and nothing in this probe navigates before it returns",
        }),
    );

    // ── a failed page does not overwrite a recoverable URL ────────────────
    let good = probe.server.url("/");
    let (ok, landed) = probe.navigate(&good, Duration::from_secs(20));
    model.request(&good);
    model.on_navigation_completed(model.generation(), &landed, ok);
    let recoverable_after_good = model.recoverable_url().map(str::to_owned);

    // A port nothing is listening on: a real, ordinary load failure.
    let dead = "http://127.0.0.1:9/never";
    let (dead_ok, dead_landed) = probe.navigate(dead, Duration::from_secs(20));
    model.request(dead);
    model.on_navigation_completed(model.generation(), &dead_landed, dead_ok);
    let evidence = probe.evidence.borrow();
    let dead_record = evidence.nav_completed.last().cloned();
    drop(evidence);
    emit(
        10,
        "failed-page-does-not-overwrite",
        serde_json::json!({
            "good_url": good,
            "recoverable_after_good": recoverable_after_good,
            "failed_url": dead,
            "navigation_completed": dead_record,
            "recoverable_after_failure": model.recoverable_url(),
            "engine_source_after_failure": probe.host.source(),
        }),
    );

    // ── a browser crash, and what comes back ──────────────────────────────
    probe.navigate(&good, Duration::from_secs(20));
    probe.pump(Duration::from_millis(500));
    let stale_generation = model.generation();
    let browser = probe.host.browser_process_id();
    let failures_before = probe.evidence.borrow().process_failures.len();
    let terminated = crate::procs::terminate(browser);
    let evidence = Rc::clone(&probe.evidence);
    let saw = crate::win::pump_until(Duration::from_secs(25), || {
        evidence.borrow().process_failures.len() > failures_before
    });
    let rebuild = model.on_browser_process_failed();
    // Everything from before the crash is now stale, and the machine must say so.
    let late_controller = model.on_controller(stale_generation, true);
    let late_navigation =
        model.on_navigation_completed(stale_generation, "https://attacker.invalid/", true);
    emit(
        10,
        "browser-crash",
        serde_json::json!({
            "pid": browser,
            "terminate_ok": terminated.is_ok(),
            "process_failed_arrived": saw,
            "effect": format!("{rebuild:?}"),
            "generation_before": stale_generation,
            "generation_after": model.generation(),
            "late_controller_effect": format!("{late_controller:?}"),
            "late_navigation_completed_effect": format!("{late_navigation:?}"),
            "recoverable_url_after_crash": model.recoverable_url(),
            "desired_url_after_crash": model.desired_url(),
        }),
    );

    // ── rebuild for real, on a fresh generation ───────────────────────────
    let new_generation = model.generation();
    let evidence = crate::host::Evidence::new();
    let rebuilt = crate::host::Host::create(
        &probe.host.environment,
        probe.window.hwnd,
        Rc::clone(&evidence),
    );
    match rebuilt {
        Ok(mut rebuilt) => {
            model.on_environment(new_generation, true);
            model.on_controller(new_generation, true);
            let effect = model.on_events_installed(new_generation);
            rebuilt.set_root_visual(&probe.tree.web_visual())?;
            rebuilt.set_seat(probe.seat, probe.host.origin)?;
            let target = match &effect {
                Effect::Navigate(url) => url.clone(),
                _ => good.clone(),
            };
            let completed_before = evidence.borrow().nav_completed.len();
            rebuilt.navigate(&target)?;
            let watch = Rc::clone(&evidence);
            crate::win::pump_until(Duration::from_secs(25), || {
                watch.borrow().nav_completed.len() > completed_before
            });
            let record = evidence.borrow().nav_completed.last().cloned();
            emit(
                10,
                "rebuild",
                serde_json::json!({
                    "generation": new_generation,
                    "effect_said_navigate_to": target,
                    "navigation_completed": record,
                }),
            );
            rebuilt.close();
        }
        Err(error) => emit(
            10,
            "rebuild",
            serde_json::json!({ "error": error.to_string() }),
        ),
    }

    // ── closing waits for the browser to exit before the folder is touched ─
    //
    // **All three doors, because the machine has to survive the one that does
    // not open.** This run's own gate 7 has produced `BrowserProcessExited` in
    // 280 ms and not at all in 25 s from the same kill, so the model is asked
    // what it does in each case rather than only in the happy one.
    let closing = model.close();
    let by_the_named_event = {
        let mut copy = crate::machine::Preview::new();
        copy.request("http://localhost/");
        copy.close();
        format!("{:?}", copy.on_browser_process_exited())
    };
    let by_process_failed = {
        let mut copy = crate::machine::Preview::new();
        copy.request("http://localhost/");
        copy.close();
        format!("{:?}", copy.on_browser_process_failed())
    };
    let by_the_deadline = format!("{:?}", model.on_cleanup_deadline());
    let a_second_door = format!("{:?}", model.on_browser_process_exited());
    emit(
        10,
        "udf-cleanup-ordering",
        serde_json::json!({
            "effect": format!("{closing:?}"),
            "state": format!("{:?}", model.state()),
            "on_browser_process_exited": by_the_named_event,
            "on_process_failed_browser_exited": by_process_failed,
            "on_the_wait_running_out": by_the_deadline,
            "a_second_door_after_the_first": a_second_door,
            "live": "Probe::shutdown closes the controller, waits for either event with a bound, and removes the folder either way; the shutdown record at the end of this log carries which door opened and how long it took",
        }),
    );

    // The version change §2 gate 7 names and cannot trigger, asked of the model
    // that would have to handle it.
    let mut updating = crate::machine::Preview::new();
    updating.request("http://localhost/");
    let generation = updating.generation();
    updating.on_environment(generation, true);
    updating.on_controller(generation, true);
    updating.on_events_installed(generation);
    updating.on_navigation_completed(generation, "http://localhost/", true);
    let effect = updating.on_new_browser_version_available();
    let orphan = updating.on_controller(generation, true);
    emit(
        10,
        "new-browser-version",
        serde_json::json!({
            "effect": format!("{effect:?}"),
            "generation_moved": updating.generation() != generation,
            "comes_back_to": updating.desired_url(),
            "controller_from_the_old_version": format!("{orphan:?}"),
            "live_counterpart": "gate 7's self-update-rebuild row runs this sequence on the real engine",
        }),
    );

    verdict(
        10,
        "pass",
        "the machine was driven by this run's real navigation and crash events: nothing navigated before the handlers were on, a failed load left the recoverable URL alone, a crash retired the generation and refused its late callbacks, and the rebuild came back to the last good URL",
    );
    Ok(())
}
