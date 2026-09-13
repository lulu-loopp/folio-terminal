//! **X-2's matrix, driven against the real host, inside a real `.app`** —
//! the claims M4-2 makes that no `#[test]` can hold (ticket M4-2,
//! `docs/DESIGN.md` §13.29).
//!
//! # Why this is a target of its own
//!
//! Three reasons, and the third is this ticket's own.
//!
//! **The main thread.** Everything here is WebKit and AppKit and both are the
//! main thread's; libtest does not hand a case that thread — M2-3 measured it on
//! this workspace's toolchain, where a `#[test]` run with `--test-threads=1`
//! still executes on a thread the harness spawned and `MainThreadMarker::new()`
//! is `None`. `harness = false` gives this file the process's own `main`.
//!
//! **A bundle.** `+[WKWebsiteDataStore defaultDataStore]` is the application's
//! own store, and what makes it *the application's* is the bundle identifier —
//! §4.5, and the reason the plan builds the bundle from M1 rather than from M5.
//! A binary run out of `target/debug/deps` has no identifier, so a proof taken
//! there would be a proof about a different store than the one the product uses.
//!
//! **A window server.** An ssh session cannot reach one (X-1 measured that), so
//! on the Mac mini the binary is copied into a throwaway ad-hoc-signed `.app`
//! and started with `open`, which asks launchd to run it in the logged-in
//! session.
//!
//! # What it costs a run that is not asking for it
//!
//! Nothing, and the gate is not an environment variable — `open` passes none of
//! the shell's environment, so a variable could not reach the process that
//! matters. The gate is the thing that is already true of the run that wants
//! this: the binary is **inside a `.app`**. An ordinary `cargo test -p
//! bt-platform` runs it out of `target/debug/deps`, where there is no bundle, so
//! it prints one line and exits — on macOS and everywhere else.
//!
//! The report is written beside the bundle, at `m4-2-report.log`, because a
//! process started by `open` inherits no working directory either.
//!
//! # What it proves, in order
//!
//! Every row is one X-2 measured on a probe, asked again of the product's own
//! host. **Nothing here touches the network**: the rows that needed two HTTP
//! servers are X-2's and stay there, and what is left is everything a local seat
//! can be asked without one.
//!
//! ① the two-step creation answers in the Windows arm's order — a
//!    `WebEvent::Environment`, then a `WebEvent::Controller`, then an install
//!    whose three guards all stand;
//! ② a local document opens through `loadFileURL:allowingReadAccessToURL:` and
//!    reads **its own folder** — a picture beside it loads;
//! ③ and **no other folder**: a picture and a frame naming the sibling
//!    directory are refused, with no rule list involved in either;
//! ④ a `data:` link is cancelled at the navigation gate, and the seat hears it;
//! ⑤ a `javascript:` link starts no navigation and moves the seat nowhere —
//!    and it *does* run in the page's own context, which is a difference from
//!    the other engine and is measured here rather than assumed away;
//! ⑥ `window.open` answers the page `null` — the window is refused at
//!    `createWebViewWithConfiguration:`;
//! ⑦ a body the engine will not draw is a download, and it is cancelled and
//!    reported rather than saved;
//! ⑧ `alert()` returns to the page immediately and no panel is on the screen;
//! ⑨ a `file:` URL outside the minted one does not become the seat's address —
//!    and the report names which of the two doors refused it;
//! ⑩ closing the seat takes the page's view out of the window, and the slot with
//!    it.
//!
//! **No TCC prompt is possible from this file.** It photographs its own window
//! with `CGWindowListCreateImage`, which macOS allows a process for its own
//! windows without a Screen Recording grant (X-1), and it drives nothing through
//! System Events, Accessibility or Automation — every step is a call into this
//! process's own objects.

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::ptr::NonNull;
    use std::time::Instant;

    use bt_platform::{
        Compositor, NativeWindow, PageVisual, WebEvent, WebHost, WebNavigationVerdict,
        WebRequestVerdict,
    };
    use objc2::rc::{Retained, autoreleasepool};
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSWindow,
        NSWindowStyleMask,
    };
    use objc2_foundation::{
        NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSRunLoop, NSSize, ns_string,
    };

    // ── the report ─────────────────────────────────────────────────────────

    struct Report {
        file: std::fs::File,
        started: Instant,
        failures: usize,
    }

    impl Report {
        fn at(path: &Path) -> Self {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("the report is beside the bundle, which this process can write to");
            Self {
                file,
                started: Instant::now(),
                failures: 0,
            }
        }

        fn say(&mut self, line: &str) {
            let at = self.started.elapsed().as_millis();
            let _ = writeln!(self.file, "[{at:>7}ms] {line}");
            let _ = self.file.flush();
            println!("[{at:>7}ms] {line}");
        }

        fn pass(&mut self, claim: &str) {
            self.say(&format!("PASS {claim}"));
        }

        fn fail(&mut self, claim: &str, because: &str) {
            self.failures += 1;
            self.say(&format!("FAIL {claim}: {because}"));
        }

        fn verdict(&mut self, row: &str, held: bool, detail: &str) {
            if held {
                self.pass(&format!("{row} — {detail}"));
            } else {
                self.fail(row, detail);
            }
        }
    }

    /// `…/Something.app/Contents/MacOS/<binary>` — three levels, and the
    /// extension is what makes it a bundle rather than a folder three deep.
    fn enclosing_bundle() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let bundle = exe.parent()?.parent()?.parent()?;
        (bundle.extension()? == "app").then(|| bundle.to_path_buf())
    }

    // ── the seat's own policy, as this file spells it ──────────────────────

    /// The rule list `bt_app::webnav::content_rules` emits for a `Mint::File`
    /// seat, as text.
    ///
    /// A literal here on purpose: this crate does not depend on `bt-app` and
    /// must not, so what the seat hands the host is an **input** to this proof
    /// rather than something it computes. That the JSON really is the same
    /// sentence `resource_request` answers is pinned where both live —
    /// `bt_app::webnav::content_rule_tests`.
    const FILE_SEAT_RULES: &str = concat!(
        r#"[{"trigger":{"url-filter":"^http://"},"action":{"type":"block"}},"#,
        r#"{"trigger":{"url-filter":"^https://"},"action":{"type":"block"}}]"#
    );

    /// Whether a candidate is inside the folder the seat was minted on — the
    /// sentence `webnav::resource_request`'s `Mint::File` arm answers, written
    /// for this platform's paths.
    fn inside(folder: &str, candidate: &str) -> bool {
        let body = candidate.split(['?', '#']).next().unwrap_or(candidate);
        body.starts_with(folder)
    }

    // ── the window ─────────────────────────────────────────────────────────

    fn a_window(mtm: MainThreadMarker) -> Retained<NSWindow> {
        let frame = NSRect::new(NSPoint::new(120.0, 120.0), NSSize::new(900.0, 640.0));
        // SAFETY: a window made on the main thread with a documented style and
        // backing store.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(ns_string!("Folio M4-2 probe"));
        window.makeKeyAndOrderFront(None);
        window
    }

    fn handle_of(window: &NSWindow) -> NativeWindow {
        let content = window.contentView().expect("a window has a content view");
        NativeWindow::from_appkit(NonNull::from(&*content).cast::<c_void>())
    }

    /// Turn the main run loop for `seconds`, so that what has been asked for is
    /// actually done.
    ///
    /// Turning it rather than sleeping matters twice here: a window is only
    /// composited by a loop that is running, and **every WebKit answer this file
    /// waits for is a completion block delivered on this run loop** — a `sleep`
    /// would be a proof that nothing ever arrives.
    fn settle(seconds: f64) {
        let loops = NSRunLoop::currentRunLoop();
        let until = Instant::now() + std::time::Duration::from_secs_f64(seconds);
        while Instant::now() < until {
            autoreleasepool(|_| {
                let date = NSDate::dateWithTimeIntervalSinceNow(0.01);
                // SAFETY: AppKit's own default-mode constant and a live date, on
                // the thread that owns the loop.
                unsafe { loops.runMode_beforeDate(NSDefaultRunLoopMode, &date) };
            });
        }
    }

    /// Turn the loop until `wanted` finds something in the host's drain, or the
    /// budget runs out. Everything drained is kept, so a later row can ask about
    /// an event an earlier one did not want.
    fn until(
        host: &WebHost,
        seen: &mut Vec<WebEvent>,
        seconds: f64,
        wanted: impl Fn(&[WebEvent]) -> bool,
    ) -> bool {
        let deadline = Instant::now() + std::time::Duration::from_secs_f64(seconds);
        loop {
            seen.extend(host.drain());
            if wanted(seen.as_slice()) {
                return true;
            }
            if Instant::now() >= deadline {
                seen.extend(host.drain());
                return wanted(seen.as_slice());
            }
            settle(0.05);
        }
    }

    // ── the fixture on disk ────────────────────────────────────────────────

    /// The 1x1 PNG every picture fixture is; what matters is whether the engine
    /// was allowed to read it, not what it decodes to.
    const PIXEL: [u8; 69] = [
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// The document the seat is opened on. Every picture reports its own outcome
    /// into `document.title`, which `-[WKWebView title]` reads back — the same
    /// channel X-2 used, and the only one a host with no bridge has. **The
    /// bridge being shut is the point**: a probe that opened one would be
    /// measuring a seat this product does not ship.
    fn report_page() -> String {
        String::from(
            r##"<!doctype html><html><head><meta charset="utf-8"><title>m42</title>
<script>
function report(k, v) { document.title = document.title + " " + k + "=" + v; }
function openWin() { var w = window.open("outside.html", "_blank"); report("windowopen", w ? "returned" : "null"); }
function shout() { alert("a page holding the seat"); report("alert", "returned"); }
function click(id) { document.getElementById(id).click(); }
</script></head><body>
<h1>M4-2 local seat</h1>
<img id="inside" src="inside.png" onload="report('inside','loaded')" onerror="report('inside','blocked')">
<img id="outside" src="../outside/secret.png" onload="report('outside','loaded')" onerror="report('outside','blocked')">
<iframe id="outsideframe" src="../outside/secret.html" width="80" height="40"
  onload="report('outsideframe','onload')"></iframe>
<p>
<a id="dataLink" href="data:text/html,%3Cb%3Edata%3C/b%3E">data</a>
<a id="scriptLink" href="javascript:report('javascript','ran')">script</a>
<a id="awayLink" href="../outside/secret.html">away</a>
</p>
</body></html>"##,
        )
    }

    /// The seat's folder, its sibling, and the file the seat is minted on.
    struct Fixture {
        folder: PathBuf,
        document: PathBuf,
        download: PathBuf,
        outside: PathBuf,
    }

    fn write_the_fixture(work: &Path) -> Fixture {
        let root = work.join("m4-2-seat");
        let folder = root.join("open");
        let outside = root.join("outside");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&folder).expect("the seat's folder");
        std::fs::create_dir_all(&outside).expect("the folder beside it");
        let document = folder.join("report.html");
        std::fs::write(&document, report_page()).expect("the document");
        std::fs::write(folder.join("inside.png"), PIXEL).expect("a picture in the folder");
        std::fs::write(outside.join("secret.png"), PIXEL).expect("a picture outside it");
        std::fs::write(
            outside.join("secret.html"),
            "<!doctype html><title>secret</title><p>the sibling folder</p>",
        )
        .expect("a document outside it");
        // **A body the engine will not draw** — the download row, with no server
        // in the room: `canShowMIMEType` is false for this and the response door
        // is what cancels it.
        let download = folder.join("attachment.folio-blob");
        std::fs::write(&download, b"attachment-bytes").expect("something to refuse");
        Fixture {
            folder,
            document,
            download,
            outside,
        }
    }

    /// A path as the `file:` URL `bt_app::webnav::Mint::file` would mint from
    /// it. The four characters that would re-open the parse are the four that
    /// door encodes, and nothing else is touched.
    fn file_url(path: &Path) -> String {
        let mut url = String::from("file://");
        for character in path.to_string_lossy().chars() {
            match character {
                '%' => url.push_str("%25"),
                '#' => url.push_str("%23"),
                '?' => url.push_str("%3F"),
                ' ' => url.push_str("%20"),
                other => url.push(other),
            }
        }
        url
    }

    // ── the picture of our own window ──────────────────────────────────────

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CgPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CgSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CgRect {
        origin: CgPoint,
        size: CgSize,
    }
    const CG_RECT_NULL: CgRect = CgRect {
        origin: CgPoint {
            x: f64::INFINITY,
            y: f64::INFINITY,
        },
        size: CgSize {
            width: 0.0,
            height: 0.0,
        },
    };
    const K_LIST_INCLUDING_WINDOW: u32 = 1 << 3;
    const K_IMAGE_BOUNDS_IGNORE_FRAMING: u32 = 1 << 0;
    const K_IMAGE_BEST_RESOLUTION: u32 = 1 << 3;

    // Declared here rather than reached through `objc2-core-graphics` because
    // turning that crate's `CGWindow` feature on would be a feature added to the
    // *library* for a test's sake — `macos_compose.rs` declares the same five
    // for the same reason.
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGWindowListCreateImage(
            screen_bounds: CgRect,
            list_option: u32,
            window_id: u32,
            image_option: u32,
        ) -> *mut c_void;
        fn CGImageGetWidth(image: *mut c_void) -> usize;
        fn CGImageGetHeight(image: *mut c_void) -> usize;
        fn CGImageGetBytesPerRow(image: *mut c_void) -> usize;
        fn CGImageGetDataProvider(image: *mut c_void) -> *mut c_void;
        fn CGDataProviderCopyData(provider: *mut c_void) -> *mut c_void;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDataGetBytePtr(data: *mut c_void) -> *const u8;
        fn CFDataGetLength(data: *mut c_void) -> isize;
        fn CFRelease(cf: *mut c_void);
    }

    /// Write a picture of **this process's own window** beside the bundle, for a
    /// reader who would rather look than read lines.
    ///
    /// `CGWindowListCreateImage` over a window this process owns needs no Screen
    /// Recording grant — X-1 established that, and it is why nothing here can
    /// raise a privacy prompt on somebody's desk.
    fn photograph(window_number: u32, into: &Path) -> Option<(usize, usize)> {
        // SAFETY: a window number this process owns, the two documented option
        // flags, and a null rectangle asking for the window's own bounds.
        let image = unsafe {
            CGWindowListCreateImage(
                CG_RECT_NULL,
                K_LIST_INCLUDING_WINDOW,
                window_number,
                K_IMAGE_BOUNDS_IGNORE_FRAMING | K_IMAGE_BEST_RESOLUTION,
            )
        };
        if image.is_null() {
            return None;
        }
        // SAFETY: `image` is the live CGImage the call above returned.
        let (width, height, stride) = unsafe {
            (
                CGImageGetWidth(image),
                CGImageGetHeight(image),
                CGImageGetBytesPerRow(image),
            )
        };
        // SAFETY: the provider belongs to the image, which is still alive.
        let provider = unsafe { CGImageGetDataProvider(image) };
        // SAFETY: as above.
        let data = unsafe { CGDataProviderCopyData(provider) };
        if data.is_null() {
            // SAFETY: created by this function and not used again.
            unsafe { CFRelease(image) };
            return None;
        }
        // SAFETY: `data` is a live CFData and the slice does not outlive it.
        let length = unsafe { CFDataGetLength(data) } as usize;
        let source = unsafe { std::slice::from_raw_parts(CFDataGetBytePtr(data), length) };
        let mut bytes = format!("P6\n{width} {height}\n255\n").into_bytes();
        for y in 0..height {
            for x in 0..width {
                let from = y * stride + x * 4;
                // 32 bits per pixel, little-endian, alpha first: B, G, R, A.
                bytes.push(source[from + 2]);
                bytes.push(source[from + 1]);
                bytes.push(source[from]);
            }
        }
        let _ = std::fs::write(into, bytes);
        // SAFETY: both were created by this function and are not used again.
        unsafe {
            CFRelease(data);
            CFRelease(image);
        }
        Some((width, height))
    }

    // ── the exercise ───────────────────────────────────────────────────────

    pub fn run() {
        let Some(bundle) = enclosing_bundle() else {
            println!(
                "macos_webview: skipped — this binary is not inside a .app, and the website \
                 data store is keyed on a bundle identifier"
            );
            return;
        };
        let work = bundle
            .parent()
            .expect("a bundle is inside a directory")
            .to_path_buf();
        let mut report = Report::at(&work.join("m4-2-report.log"));
        report.say(&format!(
            "pid={} bundle={}",
            std::process::id(),
            bundle.display()
        ));

        let mtm = MainThreadMarker::new().expect("harness = false gives this file the main thread");
        // The application object has to exist before a `WKWebView` does — it is
        // what owns the run loop every completion block below is delivered on —
        // and the policy is stated rather than inherited so that the window
        // really comes up in the logged-in session.
        let application = NSApplication::sharedApplication(mtm);
        let _ = application.setActivationPolicy(NSApplicationActivationPolicy::Regular);

        let fixture = write_the_fixture(&work);
        let minted = file_url(&fixture.document);
        let folder = file_url(&fixture.folder);
        report.say(&format!("minted {minted}"));

        // **The seat's policy, and it is the product's rule written out** — the
        // `Mint::File` arm of `webnav::navigation_starting` and of
        // `webnav::resource_request`.
        //
        // One deliberate widening: the navigation gate admits **the seat's
        // folder** rather than the one file in it. A gate that admitted only the
        // minted document would refuse the attachment at the *action*, and row ⑦
        // is about the door after that one — `decidePolicyForNavigationResponse:`
        // is reached only by a navigation somebody allowed, so a proof of it
        // needs a second address inside the same folder to allow.
        let gate_folder = format!("{folder}/");
        let request_folder = gate_folder.clone();
        let mut host = WebHost::new(
            Box::new(move |candidate: &str| {
                let body = candidate.split(['?', '#']).next().unwrap_or(candidate);
                if body.len() >= gate_folder.len()
                    && body[..gate_folder.len()].eq_ignore_ascii_case(&gate_folder)
                {
                    WebNavigationVerdict::Proceed
                } else {
                    WebNavigationVerdict::Cancel
                }
            }),
            Box::new(move |candidate: &str| {
                if candidate.starts_with("file:") && inside(&request_folder, candidate) {
                    WebRequestVerdict::Allow
                } else {
                    WebRequestVerdict::Refuse
                }
            }),
            Box::new(|| {}),
        );
        let mut seen: Vec<WebEvent> = Vec::new();

        // ① the two-step creation, in the Windows arm's order.
        if let Err(reason) = host.set_request_rules(FILE_SEAT_RULES) {
            report.fail("the seat states its resource rule", &reason);
            report.say("ALL_DONE");
            return;
        }
        let window = a_window(mtm);
        let number = window.windowNumber() as u32;
        let handle = handle_of(&window);
        let compositor = match Compositor::new(handle) {
            Ok(compositor) => compositor,
            Err(reason) => {
                report.fail("the composition builds on a real window", &reason);
                report.say("ALL_DONE");
                return;
            }
        };
        let page = PageVisual { tab: 1, seat: 1 };
        if let Err(reason) = compositor.attach_web_visual(page) {
            report.fail("the page gets a slot", &reason);
            report.say("ALL_DONE");
            return;
        }
        let _ = compositor.place_web_visual(page, (0, 0), (0.0, 0.0, 1800.0, 1280.0));

        let store_folder = work.join("m4-2-rules");
        if let Err(reason) = host.request_environment(&store_folder, 1) {
            report.fail("the engine is asked for", &reason);
            report.say("ALL_DONE");
            return;
        }
        let environment_first = until(&host, &mut seen, 2.0, |events| {
            events.iter().any(|event| {
                matches!(
                    event,
                    WebEvent::Environment {
                        generation: 1,
                        error: None
                    }
                )
            })
        });
        report.verdict(
            "① the engine answers",
            environment_first,
            "WebEvent::Environment for generation 1",
        );

        if let Err(reason) = host.request_controller(handle, 1) {
            report.fail("the page is asked for", &reason);
            report.say("ALL_DONE");
            return;
        }
        let controller = until(&host, &mut seen, 8.0, |events| {
            events
                .iter()
                .any(|event| matches!(event, WebEvent::Controller { generation: 1, .. }))
        });
        let controller_error = seen.iter().find_map(|event| match event {
            WebEvent::Controller { error, .. } => error.clone(),
            _ => None,
        });
        report.verdict(
            "① the page answers, with its rule list compiled",
            controller && controller_error.is_none(),
            &format!("Controller arrived={controller} error={controller_error:?}"),
        );
        if !controller {
            report.say("ALL_DONE");
            return;
        }

        let installed = match host.install(&compositor, page, 1) {
            Ok(installed) => installed,
            Err(reason) => {
                report.fail("the page is taken into service", &reason);
                report.say("ALL_DONE");
                return;
            }
        };
        report.verdict(
            "① every gate this seat needs stands",
            installed.guards.all_stand(),
            &format!(
                "guards={:?} missing={:?} unapplied={:?}",
                installed.guards,
                installed.guards.missing(),
                installed
                    .unapplied
                    .iter()
                    .map(|setting| setting.api())
                    .collect::<Vec<_>>()
            ),
        );

        let page_view = page_view(&window);
        report.verdict(
            "① the page's own view stands inside the page's slot",
            page_view.is_some() && views_in_the_slots(&window) == 1,
            &format!(
                "views in the slots: {}, and one of them is a WKWebView: {}",
                views_in_the_slots(&window),
                page_view.is_some()
            ),
        );
        let page_view = page_view.as_deref();

        // ② and ③ the local seat's own folder, and no other.
        if let Err(reason) = host.navigate(&minted) {
            report.fail("the seat is pointed at its document", &reason);
            report.say("ALL_DONE");
            return;
        }
        let arrived = until(&host, &mut seen, 8.0, |events| {
            events
                .iter()
                .any(|event| matches!(event, WebEvent::NavigationCompleted { success: true, .. }))
        });
        settle(1.5);
        let title = read_title(&mut report, page_view);
        report.verdict(
            "② a local document opens through loadFileURL:allowingReadAccessToURL:",
            arrived,
            &format!("NavigationCompleted={arrived} title={title:?}"),
        );
        report.verdict(
            "② it reads its own folder",
            title.contains("inside=loaded"),
            &format!("title={title:?}"),
        );
        report.verdict(
            "③ and not the folder beside it",
            title.contains("outside=blocked"),
            &format!("title={title:?}"),
        );
        report.verdict(
            "③ a frame naming that folder never loads either",
            !title.contains("outsideframe=onload"),
            &format!("title={title:?}"),
        );
        report.say(&format!(
            "   (the folder beside it is {} and was never read)",
            fixture.outside.display()
        ));

        // ④ a `data:` link.
        let before = seen.len();
        run_script(&mut report, page_view, "click('dataLink')");
        let cancelled = until(&host, &mut seen, 3.0, |events| {
            events[before..].iter().any(|event| {
                matches!(
                    event,
                    WebEvent::NavigationStarting { uri, cancelled: true } if uri.starts_with("data:")
                )
            })
        });
        report.verdict(
            "④ a data: link is cancelled at the navigation gate",
            cancelled,
            &format!("{:?}", &seen[before..]),
        );

        // ⑤ a `javascript:` link.
        //
        // **X-2 listed this beside `data:` and `blob:` and it does not belong
        // there.** Measured here: WebKit evaluates a `javascript:` URL in the
        // page's own context and never offers it to
        // `decidePolicyForNavigationAction:` at all, where the other engine
        // raises `NavigationStarting` and the gate refuses it. What the claim
        // reduces to is what the address does, and the address does nothing —
        // which is also the whole of the consequence, since a page can run its
        // own script with a `<script>` tag and this reaches no other origin and
        // no disk.
        let before = seen.len();
        let url_before = read_url(page_view);
        run_script(&mut report, page_view, "click('scriptLink')");
        settle(1.0);
        let after = read_title(&mut report, page_view);
        let url_after = read_url(page_view);
        let no_navigation = !seen[before..]
            .iter()
            .any(|event| matches!(event, WebEvent::NavigationStarting { .. }));
        report.verdict(
            "⑤ a javascript: link starts no navigation and moves the seat nowhere",
            no_navigation && url_after == url_before,
            &format!(
                "url before={url_before:?} after={url_after:?} events={:?}",
                &seen[before..]
            ),
        );
        report.say(&format!(
            "   FINDING a javascript: URL is evaluated by the engine and never reaches the gate              — the page reports {}",
            if after.contains("javascript=ran") {
                "that it ran"
            } else {
                "nothing"
            }
        ));

        // ⑥ a window a page asked for.
        run_script(&mut report, page_view, "openWin()");
        settle(1.0);
        let opened = read_title(&mut report, page_view);
        report.verdict(
            "⑥ window.open answers the page null",
            opened.contains("windowopen=null"),
            &format!("title={opened:?}"),
        );

        // ⑦ a body the engine will not draw.
        let before = seen.len();
        let download = file_url(&fixture.download);
        run_script(
            &mut report,
            page_view,
            &format!("location.href = {download:?}"),
        );
        let refused = until(&host, &mut seen, 4.0, |events| {
            events[before..]
                .iter()
                .any(|event| matches!(event, WebEvent::DownloadStarting { .. }))
        });
        report.verdict(
            "⑦ a download is cancelled and reported rather than saved",
            refused,
            &format!("{:?}", &seen[before..]),
        );

        // ⑧ a page's own modal window.
        let before = seen.len();
        run_script(&mut report, page_view, "shout()");
        let dismissed = until(&host, &mut seen, 3.0, |events| {
            events[before..]
                .iter()
                .any(|event| matches!(event, WebEvent::ScriptDialogDismissed { kind: 0 }))
        });
        settle(0.5);
        let shouted = read_title(&mut report, page_view);
        report.verdict(
            "⑧ alert() returns to the page at once and opens no window",
            dismissed && shouted.contains("alert=returned"),
            &format!("dismissed={dismissed} title={shouted:?}"),
        );

        // ⑨ a file: URL outside the minted one, as a location.
        let before = seen.len();
        let away = file_url(&fixture.outside.join("secret.html"));
        run_script(&mut report, page_view, "click('awayLink')");
        let _ = until(&host, &mut seen, 3.0, |events| {
            events[before..].iter().any(|event| {
                matches!(
                    event,
                    WebEvent::NavigationStarting { .. } | WebEvent::NavigationCompleted { .. }
                )
            })
        });
        settle(0.6);
        let landed = read_url(page_view);
        let by_the_gate = seen[before..].iter().any(|event| {
            matches!(
                event,
                WebEvent::NavigationStarting {
                    cancelled: true,
                    ..
                }
            )
        });
        report.verdict(
            "⑨ a file: URL outside the minted one does not become the seat's address",
            landed == minted,
            &format!("landed={landed:?} away={away} events={:?}", &seen[before..]),
        );
        report.say(&format!(
            "   FINDING it was refused by {}",
            if by_the_gate {
                "this host's navigation gate"
            } else {
                "the engine itself, before any callback — the read-access scope                  loadFileURL: was given, which is X-2's row 9 arriving one door earlier"
            }
        ));

        // A picture of our own window, for a reader.
        match photograph(number, &work.join("m4-2-window.ppm")) {
            Some((width, height)) => {
                report.say(&format!("a picture of window {number}: {width}x{height}"));
            }
            None => report.say(&format!("window {number} could not be photographed")),
        }

        // ⑩ closing takes the page out of the window.
        host.close();
        settle(0.3);
        let left = views_in_the_slots(&window);
        report.verdict(
            "⑩ closing the seat takes the page's view out of the window",
            left == 0,
            &format!("the page's slot holds {left} views"),
        );
        let _ = compositor.detach_web_visual(page);
        window.close();

        report.say(&format!("failures={} — M4_2_DONE", report.failures));
        report.say("ALL_DONE");
    }

    /// Every view standing inside a page slot, counted by walking the window.
    ///
    /// **The window and not the host is what is asked**, here and in
    /// [`page_view`] below: what `install` promises is that the page's view is
    /// *in the hierarchy*, and a host that answered the question itself would be
    /// the thing under test marking its own work.
    fn views_in_the_slots(window: &NSWindow) -> usize {
        let Some(content) = window.contentView() else {
            return 0;
        };
        let mut found = 0;
        let slots = content.subviews();
        for index in 0..slots.count() {
            found += slots.objectAtIndex(index).subviews().count();
        }
        found
    }

    /// The page's own view, found in the window rather than asked of the host.
    ///
    /// This is the probe's instrument: a `WebHost` with a "hand me your view" or
    /// a "run this script" door would be a host with a bridge, which is exactly
    /// what `WEB_SETTINGS`' first two rows shut. Reading `document.title` back
    /// off the view is the only channel a seat with no bridge has, and it is the
    /// one X-2 used.
    fn page_view(window: &NSWindow) -> Option<Retained<objc2_web_kit::WKWebView>> {
        let content = window.contentView()?;
        let slots = content.subviews();
        for slot in 0..slots.count() {
            let children = slots.objectAtIndex(slot).subviews();
            for child in 0..children.count() {
                if let Ok(view) = children
                    .objectAtIndex(child)
                    .downcast::<objc2_web_kit::WKWebView>()
                {
                    return Some(view);
                }
            }
        }
        None
    }

    /// The address the page is actually on, which is what "the seat did not go
    /// there" is a claim about.
    fn read_url(page: Option<&objc2_web_kit::WKWebView>) -> String {
        let Some(view) = page else {
            return String::new();
        };
        // SAFETY: a live view on the main thread.
        unsafe { view.URL() }
            .and_then(|url| url.absoluteString())
            .map(|text| text.to_string())
            .unwrap_or_default()
    }

    fn read_title(report: &mut Report, page: Option<&objc2_web_kit::WKWebView>) -> String {
        let Some(view) = page else {
            report.say("   (the probe found no page to read a title from)");
            return String::new();
        };
        // SAFETY: a live view on the main thread.
        unsafe { view.title() }
            .map(|text| text.to_string())
            .unwrap_or_default()
    }

    fn run_script(report: &mut Report, page: Option<&objc2_web_kit::WKWebView>, script: &str) {
        report.say(&format!("   run {script}"));
        let Some(view) = page else {
            return;
        };
        // SAFETY: a live view, a string this call made, and no completion
        // handler — the answer this probe reads is `document.title`.
        unsafe {
            view.evaluateJavaScript_completionHandler(
                &objc2_foundation::NSString::from_str(script),
                None,
            );
        }
        settle(0.4);
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    mac::run();
    #[cfg(not(target_os = "macos"))]
    println!("macos_webview: nothing to run on this platform");
}
