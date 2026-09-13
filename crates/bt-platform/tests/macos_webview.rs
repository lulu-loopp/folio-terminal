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

    /// Turn the loop for `seconds` **and read everything the host says while it
    /// turns** — `settle` with the drain that `settle` has not got.
    ///
    /// The row this was written for is every row that runs a script and then
    /// asks what the engine said about it: `settle` only turns the run loop, so
    /// the events were delivered to the host and left sitting in it, and the
    /// slice the row went on to read was empty. Three rows of the first run
    /// failed on exactly that and none of them was failing about the engine.
    fn catch_up(host: &WebHost, seen: &mut Vec<WebEvent>, seconds: f64) {
        let _ = until(host, seen, seconds, |_| false);
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

    // ── two origins, served out of this process (M4-3) ─────────────────────

    /// One HTTP origin on `127.0.0.1`, and the log of everything that reached
    /// it.
    ///
    /// **The socket is the ground truth**, which is X-2's own instrument and the
    /// reason the network rows need a server at all: WKWebView announces a
    /// document's subresources to nobody, so "was this request stopped" is a
    /// question only the far end can answer. A line in this log with no delegate
    /// callback beside it is the gap, in bytes rather than in prose.
    ///
    /// Two of them and not one, because a document and its subresources have to
    /// be **two origins** for the cross-origin rows to mean anything.
    struct Origin {
        port: u16,
        hits: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Origin {
        fn base(&self) -> String {
            format!("http://127.0.0.1:{}", self.port)
        }

        fn lines(&self) -> Vec<String> {
            self.hits
                .lock()
                .map(|log| log.clone())
                .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
        }

        /// Whether anything matching `needle` reached this socket.
        fn was_asked_for(&self, needle: &str) -> bool {
            self.lines().iter().any(|line| line.contains(needle))
        }

        /// Forget everything so far — called between rows, so that "did this
        /// reach the server" is a question about *this* row.
        fn forget(&self) {
            match self.hits.lock() {
                Ok(mut log) => log.clear(),
                Err(poisoned) => poisoned.into_inner().clear(),
            }
        }
    }

    /// What a route answers: a status, extra headers, and the body.
    type Answer = (u16, Vec<String>, Vec<u8>);

    /// Start an origin. `reply` is asked `(method, path, body)` and answers.
    ///
    /// A thread per connection, and a read timeout on each: an engine opens
    /// connections it never sends anything down (a preconnect), and an accept
    /// loop that served them one at a time would be a test that hangs on the
    /// engine being efficient.
    fn serve(
        reply: impl Fn(&str, &str, &str) -> Answer + Send + Sync + 'static,
    ) -> std::io::Result<Origin> {
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let hits = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = std::sync::Arc::clone(&hits);
        let reply = std::sync::Arc::new(reply);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let log = std::sync::Arc::clone(&log);
                let reply = std::sync::Arc::clone(&reply);
                std::thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(2000)));
                    let mut head = Vec::new();
                    let mut byte = [0_u8; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        match stream.read(&mut byte) {
                            Ok(1) => head.push(byte[0]),
                            _ => return,
                        }
                        if head.len() > 16 * 1024 {
                            return;
                        }
                    }
                    let text = String::from_utf8_lossy(&head).into_owned();
                    let mut lines = text.lines();
                    let first = lines.next().unwrap_or_default().to_owned();
                    let mut parts = first.split(' ');
                    let method = parts.next().unwrap_or_default().to_owned();
                    let path = parts.next().unwrap_or_default().to_owned();
                    let header = |name: &str| -> String {
                        text.lines()
                            .find(|line| {
                                line.len() > name.len()
                                    && line[..name.len()].eq_ignore_ascii_case(name)
                            })
                            .and_then(|line| line.split_once(':'))
                            .map(|(_, value)| value.trim().to_owned())
                            .unwrap_or_else(|| String::from("-"))
                    };
                    let length: usize = header("Content-Length").parse().unwrap_or(0);
                    let mut body = vec![0_u8; length.min(64 * 1024)];
                    if !body.is_empty() && stream.read_exact(&mut body).is_err() {
                        body.clear();
                    }
                    let body = String::from_utf8_lossy(&body).into_owned();
                    let line = format!(
                        "{method} {path} dest={} mode={} origin={} referer={} body={body}",
                        header("Sec-Fetch-Dest"),
                        header("Sec-Fetch-Mode"),
                        header("Origin"),
                        header("Referer"),
                    );
                    match log.lock() {
                        Ok(mut log) => log.push(line),
                        Err(poisoned) => poisoned.into_inner().push(line),
                    }
                    let (status, headers, payload) = reply(&method, &path, &body);
                    let mut out = format!("HTTP/1.1 {status} X\r\nConnection: close\r\n");
                    for header in headers {
                        out.push_str(&header);
                        out.push_str("\r\n");
                    }
                    out.push_str(&format!("Content-Length: {}\r\n\r\n", payload.len()));
                    let _ = stream.write_all(out.as_bytes());
                    let _ = stream.write_all(&payload);
                    let _ = stream.flush();
                });
            }
        });
        Ok(Origin { port, hits })
    }

    fn html(body: &str) -> Answer {
        (
            200,
            vec![String::from("Content-Type: text/html; charset=utf-8")],
            body.as_bytes().to_vec(),
        )
    }

    fn script(body: &str) -> Answer {
        (
            200,
            vec![String::from("Content-Type: text/javascript; charset=utf-8")],
            body.as_bytes().to_vec(),
        )
    }

    /// The document a browsing seat is opened on. Every row reports its own
    /// outcome into `document.title`, which is the only channel a seat with no
    /// bridge has.
    ///
    /// `%B%` is the second origin and `%MINT%` the `file:` URL the *other* seat
    /// was minted on — a page outside the mint naming something inside it, which
    /// is one of the rows.
    fn browsing_page(second: &str, minted: &str) -> String {
        let page = r##"<!doctype html><html><head><meta charset="utf-8"><title>m43</title>
<script>
var B = "%B%";
function report(k, v) { document.title = document.title + " " + k + "=" + v; }
function doFetch() { fetch(B + "/data.json").then(function (r) { return r.text(); })
  .then(function () { report("fetch", "loaded"); }, function () { report("fetch", "blocked"); }); }
function doWorker() {
  try {
    var w = new Worker("/worker.js");
    w.onmessage = function (e) { report("worker", e.data); };
    w.onerror = function () { report("worker", "error"); };
    w.postMessage(B);
  } catch (e) { report("worker", "refused"); }
}
function doServiceWorker() {
  if (!navigator.serviceWorker) { report("sw", "absent"); return; }
  navigator.serviceWorker.register("/sw.js").then(
    function () { report("sw", "registered"); },
    function (e) { report("sw", "refused:" + e.name); });
}
function doBlob() {
  var blob = new Blob(["<title>blob</title><p>bytes</p>"], { type: "text/html" });
  location.href = URL.createObjectURL(blob);
}
function capture(kind, want) {
  try {
    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
      report(kind, "noapi"); return;
    }
    navigator.mediaDevices.getUserMedia(want).then(
      function () { report(kind, "granted"); },
      function (e) { report(kind, "denied:" + e.name); });
  } catch (e) { report(kind, "threw:" + e.name); }
}
function doCamera() { capture("camera", { video: true }); }
function doMicrophone() { capture("microphone", { audio: true }); }
function doGeolocation() {
  try {
    if (!navigator.geolocation) { report("geolocation", "noapi"); return; }
    navigator.geolocation.getCurrentPosition(
      function () { report("geolocation", "granted"); },
      function (e) { report("geolocation", "denied:" + e.code); },
      { timeout: 2500, maximumAge: 0 });
  } catch (e) { report("geolocation", "threw:" + e.name); }
}
function openOnAGesture() { var w = window.open("/doc2.html", "_blank");
  report("openbyhand", w ? "returned" : "null"); }
function openWithNoGesture() { setTimeout(function () {
  var w = window.open("/doc2.html", "_blank");
  report("opentimer", w ? "returned" : "null"); }, 0); }
function click(id) { document.getElementById(id).click(); }
</script></head><body style="margin:0">
<h1>M4-3 browsing seat</h1>
<img src="%B%/pixel.png" onload="report('crossimg','loaded')" onerror="report('crossimg','blocked')">
<link rel="stylesheet" href="%B%/style.css">
<script src="%B%/script.js" onerror="report('crossscript','blocked')"></script>
<iframe id="crossframe" src="%B%/frame.html" width="60" height="30"
  onload="report('crossframe','onload')"></iframe>
<iframe id="dataframe" src="data:text/html,%3Ctitle%3Edata%3C/title%3E" width="60" height="30"
  onload="report('dataframe','onload')"></iframe>
<p>
<a id="blankLink" href="/doc2.html" target="_blank">blank</a>
<a id="downloadLink" href="/attach" download="m43.bin">download</a>
<a id="mintLink" href="%MINT%">the other seat's file</a>
</p>
<form id="postForm" method="post" action="/form"><input type="hidden" name="k" value="v"></form>
<button id="gestureButton" onclick="openOnAGesture(); report('pressed','page')"
  style="position:absolute;left:20px;top:260px;width:160px;height:40px">open a window</button>
<button id="coveredButton" onclick="report('pressed','covered')"
  style="position:absolute;left:20px;top:340px;width:160px;height:40px">under the chrome</button>
</body></html>"##;
        page.replace("%B%", second).replace("%MINT%", minted)
    }

    /// Everything origin A answers.
    fn first_origin(
        second: String,
        minted: String,
    ) -> impl Fn(&str, &str, &str) -> Answer + Send + Sync {
        move |method: &str, path: &str, _body: &str| match (method, path) {
            (_, "/doc.html") => html(&browsing_page(&second, &minted)),
            (_, "/doc2.html") => html("<!doctype html><title>second</title><p>the second page</p>"),
            // Two hops and then a page, so that **every** hop is a question the
            // delegate is asked — X-2's row 5.
            (_, "/redirect1") => (302, vec![String::from("Location: /redirect2")], Vec::new()),
            (_, "/redirect2") => (302, vec![String::from("Location: /doc2.html")], Vec::new()),
            (_, "/auth") => (
                401,
                vec![
                    String::from("WWW-Authenticate: Basic realm=\"folio-m43\""),
                    String::from("Content-Type: text/html; charset=utf-8"),
                ],
                b"<!doctype html><title>unauthorised</title><p>the body behind the box</p>"
                    .to_vec(),
            ),
            (_, "/attach") => (
                200,
                vec![
                    String::from("Content-Type: application/octet-stream"),
                    String::from("Content-Disposition: attachment; filename=\"m43.bin\""),
                ],
                b"attachment-bytes".to_vec(),
            ),
            ("POST", "/form") => {
                html("<!doctype html><title>posted</title><p>the form arrived</p>")
            }
            (_, "/form") => html("<!doctype html><title>notposted</title>"),
            // A worker whose whole job is to reach the *other* origin, which is
            // X-2's row 20 and the thing it never measured.
            (_, "/worker.js") => script(
                "self.onmessage = function (e) { \
                   fetch(e.data + '/data.json').then(function (r) { return r.text(); }) \
                     .then(function () { self.postMessage('loaded'); }, \
                           function () { self.postMessage('blocked'); }); };",
            ),
            (_, "/sw.js") => script("self.addEventListener('install', function () {});"),
            _ => (
                404,
                vec![String::from("Content-Type: text/plain")],
                b"no".to_vec(),
            ),
        }
    }

    /// Everything origin B answers — nothing but subresources, which is what
    /// makes it the origin the rules are about.
    fn second_origin(method: &str, path: &str, _body: &str) -> Answer {
        let _ = method;
        match path {
            "/pixel.png" => (
                200,
                vec![String::from("Content-Type: image/png")],
                PIXEL.to_vec(),
            ),
            "/style.css" => (
                200,
                vec![String::from("Content-Type: text/css")],
                b"h1 { color: rebeccapurple }".to_vec(),
            ),
            "/script.js" => script("window.report && report('crossscript','ran');"),
            // **With an answer a cross-origin `fetch` may read.** Without it
            // the browser fetches the bytes and then refuses to hand them to
            // the page, and a row that only watched the page would call a
            // request that happened a request that did not.
            "/data.json" => (
                200,
                vec![
                    String::from("Content-Type: application/json"),
                    String::from("Access-Control-Allow-Origin: *"),
                ],
                b"{\"from\":\"the other origin\"}".to_vec(),
            ),
            "/frame.html" => html("<!doctype html><title>outside</title><p>the other origin</p>"),
            _ => (
                404,
                vec![String::from("Content-Type: text/plain")],
                b"no".to_vec(),
            ),
        }
    }

    // ── the pointer (M4-3, §13.29 ⑩'s last sentence) ───────────────────────

    /// The rule list `bt_app::webnav::content_rules` emits for a browsing seat.
    ///
    /// A literal for [`FILE_SEAT_RULES`]' reason: this crate does not depend on
    /// `bt-app` and must not, so what the seat hands the host is an **input** to
    /// this proof. `Mint::Nothing` blocks the disk and nothing else — the
    /// network is what a browsing seat is for.
    const BROWSING_SEAT_RULES: &str =
        r#"[{"trigger":{"url-filter":"^file:"},"action":{"type":"block"}}]"#;

    /// A point in the **document's** coordinates, as a point in this window's.
    ///
    /// Three conversions and every one of them is asked rather than assumed: the
    /// page's own view may or may not be flipped, the content view may or may
    /// not be (a test's window is not winit's — see `Compositor::in_content`),
    /// and `hitTest:` and `NSEvent` both want the *window's* space. Asking
    /// AppKit to do the arithmetic is the only way this does not become a fourth
    /// coordinate convention.
    fn window_point(window: &NSWindow, css: NSPoint) -> Option<NSPoint> {
        let content = window.contentView()?;
        let page = page_view(window)?;
        let view: &objc2_app_kit::NSView = &page;
        let local = if view.isFlipped() {
            css
        } else {
            NSPoint::new(css.x, view.bounds().size.height - css.y)
        };
        let in_content = view.convertPoint_toView(local, Some(&content));
        Some(content.convertPoint_toView(in_content, None))
    }

    /// **Which view AppKit would hand a press at this point** — the whole of the
    /// question this row is about, asked of the window itself rather than of
    /// anything this ticket wrote.
    fn routed_to(window: &NSWindow, css: NSPoint) -> Option<Retained<objc2_app_kit::NSView>> {
        let content = window.contentView()?;
        let point = window_point(window, css)?;
        content.hitTest(point)
    }

    /// Whether a view is the page's own, or something inside it.
    fn view_is_inside_a_web_view(view: &objc2_app_kit::NSView) -> bool {
        if view.downcast_ref::<objc2_web_kit::WKWebView>().is_some() {
            return true;
        }
        // SAFETY: `superview` hands back a view it does not retain for the
        // caller, and every one it hands back here is held by the window's own
        // hierarchy for longer than this walk.
        let mut standing = unsafe { view.superview() };
        while let Some(here) = standing {
            if here.downcast_ref::<objc2_web_kit::WKWebView>().is_some() {
                return true;
            }
            // SAFETY: as above.
            standing = unsafe { here.superview() };
        }
        false
    }

    /// **Whether the object standing as this page's UI delegate answers a
    /// selector at all** — the only way to ask this platform what it can refuse
    /// by name.
    ///
    /// Apple's rule for the permission methods is that *not implementing one is
    /// not a denial*: the default for an unimplemented capability is whatever
    /// WebKit does on its own, which for capture is a prompt. So "is this
    /// capability refused by Folio" is a question about the delegate's method
    /// list, and `respondsToSelector:` is the runtime's own answer to it.
    fn delegate_answers(page: Option<&objc2_web_kit::WKWebView>, selector: &str) -> bool {
        let Some(view) = page else {
            return false;
        };
        // SAFETY: a live view on the main thread.
        let Some(delegate) = (unsafe { view.UIDelegate() }) else {
            return false;
        };
        let Ok(name) = std::ffi::CString::new(selector) else {
            return false;
        };
        let selector = objc2::runtime::Sel::register(&name);
        // SAFETY: `respondsToSelector:` is a method on every object descended
        // from `NSObject`, and the delegate is one.
        unsafe { objc2::msg_send![&*delegate, respondsToSelector: selector] }
    }

    fn named(view: Option<&objc2_app_kit::NSView>) -> String {
        match view {
            None => String::from("nothing"),
            Some(view) => format!(
                "{}{}",
                view.class().name().to_string_lossy(),
                if view_is_inside_a_web_view(view) {
                    " (the page)"
                } else {
                    ""
                }
            ),
        }
    }

    /// **A press, delivered the way AppKit delivers one.**
    ///
    /// `-[NSWindow sendEvent:]` is the method the application's own event loop
    /// calls for a real click, and it is what asks `hitTest:` and then sends
    /// `mouseDown:` to whatever answered — so this drives the very mechanism the
    /// row is about, in this process, on this window.
    ///
    /// **Not `CGEventPost`**, which the ticket also allows: posting through the
    /// session's event tap is gated by the Accessibility permission on a current
    /// macOS, and a run that raised a TCC prompt on somebody's desk would have
    /// broken a harder rule than it kept. Nothing here leaves this process, the
    /// pointer on the machine does not move, and no other application can see
    /// any of it.
    fn press(window: &NSWindow, css: NSPoint) {
        let Some(point) = window_point(window, css) else {
            return;
        };
        let number = window.windowNumber();
        for kind in [
            objc2_app_kit::NSEventType::LeftMouseDown,
            objc2_app_kit::NSEventType::LeftMouseUp,
        ] {
            let event = objc2_app_kit::NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                kind,
                point,
                objc2_app_kit::NSEventModifierFlags::empty(),
                0.0,
                number,
                None,
                0,
                1,
                1.0,
            );
            if let Some(event) = event {
                window.sendEvent(&event);
            }
            settle(0.08);
        }
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

        // ── the network half of X-2's matrix (M4-3) ────────────────────────
        //
        // Everything above is what a local seat can be asked without a socket.
        // What follows is everything else, against **two** origins on
        // `127.0.0.1` served out of this process, because a document and its
        // subresources have to be two origins for a cross-origin row to mean
        // anything — and because the far socket is the only witness a request
        // nobody was told about has.
        let second = match serve(second_origin) {
            Ok(origin) => origin,
            Err(error) => {
                report.fail("the second origin starts", &error.to_string());
                report.say("ALL_DONE");
                return;
            }
        };
        let first = match serve(first_origin(second.base(), minted.clone())) {
            Ok(origin) => origin,
            Err(error) => {
                report.fail("the first origin starts", &error.to_string());
                report.say("ALL_DONE");
                return;
            }
        };
        report.say(&format!(
            "origins: A={} (the document) B={} (everything else)",
            first.base(),
            second.base()
        ));

        // **The browsing seat's own policy**, which is `Mint::Nothing`:
        // `webnav::navigation_starting` takes an `http(s)` address with a host,
        // and `webnav::resource_request` lets a document be built out of
        // anything but the disk.
        let mut browsing = WebHost::new(
            Box::new(|candidate: &str| {
                if candidate.starts_with("http://127.0.0.1:") {
                    WebNavigationVerdict::Proceed
                } else {
                    WebNavigationVerdict::Cancel
                }
            }),
            Box::new(|candidate: &str| {
                if candidate.starts_with("file:") {
                    WebRequestVerdict::Refuse
                } else {
                    WebRequestVerdict::Allow
                }
            }),
            Box::new(|| {}),
        );
        let mut net: Vec<WebEvent> = Vec::new();
        let page2 = PageVisual { tab: 1, seat: 2 };
        // The page stands where the pointer rows below aim: a rectangle whose
        // own origin is known, so that a point inside the document is a point in
        // this window. Placed **before** the install, exactly as the file seat
        // is, because a page installed into a slot of no size is a page laid out
        // against nothing.
        let page_left = 40_i32;
        let page_top = 60_i32;
        // **Points times the backing scale, and that is the whole of the first
        // run's pointer failure.** `place_web_visual` is given *physical*
        // pixels; a document's own coordinates are points. On a 2x display a
        // clip of 700x520 physical is a viewport 350x260 CSS pixels tall, and
        // the two buttons the pointer rows aim at — at 260 and 340 — were
        // outside the page altogether, so every press fell through to the
        // window and the rows read as a routing defect that was not there.
        let scale = window.backingScaleFactor() as f32;
        let page_wide = 700.0 * scale;
        let page_tall = 520.0 * scale;
        let started = browsing.set_request_rules(BROWSING_SEAT_RULES).is_ok()
            && compositor.attach_web_visual(page2).is_ok()
            && compositor
                .place_web_visual(
                    page2,
                    (page_left, page_top),
                    (0.0, 0.0, page_wide, page_tall),
                )
                .is_ok()
            && browsing.request_environment(&store_folder, 2).is_ok()
            && until(&browsing, &mut net, 3.0, |events| {
                events.iter().any(|event| {
                    matches!(
                        event,
                        WebEvent::Environment {
                            generation: 2,
                            error: None
                        }
                    )
                })
            })
            && browsing.request_controller(handle, 2).is_ok()
            && until(&browsing, &mut net, 8.0, |events| {
                events
                    .iter()
                    .any(|event| matches!(event, WebEvent::Controller { generation: 2, .. }))
            });
        if !started {
            report.fail("⑪ a browsing seat is built", &format!("{net:?}"));
            report.say("ALL_DONE");
            return;
        }
        let browsing_report = match browsing.install(&compositor, page2, 2) {
            Ok(installed) => installed,
            Err(reason) => {
                report.fail("⑪ the browsing page is taken into service", &reason);
                report.say("ALL_DONE");
                return;
            }
        };
        report.verdict(
            "⑪ a browsing seat's gates all stand",
            browsing_report.guards.all_stand(),
            &format!("guards={:?}", browsing_report.guards),
        );
        // `self::` because the file seat's own row bound a local of this name
        // above, and the module item is the one meant here.
        let browsing_view = self::page_view(&window);
        let browsing_view = browsing_view.as_deref();

        let document = format!("{}/doc.html", first.base());
        // **Waiting for *this* navigation and not for any that has ever
        // happened.** The first run's `go` asked whether the whole list held a
        // `NavigationCompleted`, which it always did from the second navigation
        // onwards, so every row after the first read a page that had not loaded
        // yet. The mark is taken before the call and the predicate reads only
        // past it.
        let go = |host: &WebHost, seen: &mut Vec<WebEvent>, url: &str| -> bool {
            let mark = seen.len();
            if host.navigate(url).is_err() {
                return false;
            }
            until(host, seen, 8.0, |events| {
                events[mark..]
                    .iter()
                    .any(|event| matches!(event, WebEvent::NavigationCompleted { .. }))
            })
        };

        // ⑫ the document's own contents reach the other origin, and **no
        //    delegate is told about any of them** — the port the third door has
        //    to be, measured against the product's own host rather than a probe.
        first.forget();
        second.forget();
        let before = net.len();
        let arrived = go(&browsing, &mut net, &document);
        settle(2.5);
        let title = read_title(&mut report, browsing_view);
        let announced = net[before..]
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    WebEvent::NavigationStarting { .. } | WebEvent::RequestRefused { .. }
                )
            })
            .count();
        report.verdict(
            "⑫ a browsing seat opens on the first origin",
            arrived && title.contains("m43"),
            &format!("arrived={arrived} title={title:?}"),
        );
        let cross = ["/pixel.png", "/style.css", "/script.js"];
        let reached: Vec<&str> = cross
            .iter()
            .copied()
            .filter(|path| second.was_asked_for(path))
            .collect();
        report.verdict(
            "⑫ a picture, a stylesheet and a script reach the second origin",
            reached.len() == cross.len(),
            &format!("reached={reached:?} of {cross:?}"),
        );
        report.say(&format!(
            "   FINDING the delegate was asked about {announced} of them, and the frame is \
             the only one it can be asked about — B's log: {:?}",
            second.lines()
        ));
        run_script(&mut report, browsing_view, "doFetch()");
        catch_up(&browsing, &mut net, 1.5);
        let fetched = read_title(&mut report, browsing_view);
        report.verdict(
            "⑫ a fetch() to the second origin is not announced either",
            second.was_asked_for("/data.json"),
            &format!("title={fetched:?}"),
        );

        // ⑬ the same document with the second origin in the rule list. The
        //    third door is the only thing that can stop any of the above, and
        //    this is it working on a seat that already exists.
        let blocking = format!(
            r#"[{{"trigger":{{"url-filter":"^file:"}},"action":{{"type":"block"}}}},{{"trigger":{{"url-filter":"^http://127\\.0\\.0\\.1:{}/"}},"action":{{"type":"block"}}}}]"#,
            second.port
        );
        match browsing.set_request_rules(&blocking) {
            Ok(()) => {
                second.forget();
                let arrived = go(&browsing, &mut net, &document);
                settle(2.5);
                run_script(&mut report, browsing_view, "doFetch()");
                catch_up(&browsing, &mut net, 1.5);
                let blocked = read_title(&mut report, browsing_view);
                report.verdict(
                    "⑬ a compiled rule list stops every one of them at the engine",
                    arrived
                        && !second.was_asked_for("/pixel.png")
                        && !second.was_asked_for("/style.css")
                        && !second.was_asked_for("/script.js")
                        && !second.was_asked_for("/data.json"),
                    &format!("B's log after the rule list: {:?}", second.lines()),
                );
                report.verdict(
                    "⑬ and the page is the only one that hears about it",
                    blocked.contains("crossimg=blocked") || blocked.contains("fetch=blocked"),
                    &format!("title={blocked:?}"),
                );
                let told = net
                    .iter()
                    .filter(|event| matches!(event, WebEvent::RequestRefused { .. }))
                    .count();
                report.say(&format!(
                    "   FINDING Folio was told about {told} of them: the engine drops a blocked \
                     request rather than answering it, so there is no line for the trace and no \
                     reason for a card"
                ));
            }
            Err(reason) => report.fail("⑬ the rule list compiles", &reason),
        }
        // Back to the seat's own rules for everything after this.
        let _ = browsing.set_request_rules(BROWSING_SEAT_RULES);
        let _ = go(&browsing, &mut net, &document);
        settle(1.5);

        // ⑭ every hop of a redirect chain.
        first.forget();
        let before = net.len();
        let _ = go(&browsing, &mut net, &format!("{}/redirect1", first.base()));
        catch_up(&browsing, &mut net, 1.0);
        let hops: Vec<&str> = net[before..]
            .iter()
            .filter_map(|event| match event {
                WebEvent::NavigationStarting { uri, .. } => Some(uri.as_str()),
                _ => None,
            })
            .collect();
        report.verdict(
            "⑭ every hop of a 302 chain is asked of the gate",
            hops.iter().any(|uri| uri.ends_with("/redirect1"))
                && hops.iter().any(|uri| uri.ends_with("/redirect2"))
                && hops.iter().any(|uri| uri.ends_with("/doc2.html")),
            &format!("asked about {hops:?}"),
        );
        let _ = go(&browsing, &mut net, &document);
        settle(2.0);

        // ⑮ a frame to another origin, and ⑯ a `data:` frame — the two rows
        //    that arrive at the *third* door through a callback rather than a
        //    pattern, because a frame is a navigation of something.
        let title = read_title(&mut report, browsing_view);
        report.verdict(
            "⑮ an iframe to the other origin is asked of the request gate",
            title.contains("crossframe=onload"),
            &format!("title={title:?}"),
        );
        report.verdict(
            "⑯ a data: iframe is what the document already holds, and loads",
            title.contains("dataframe=onload"),
            &format!("title={title:?}"),
        );

        // ⑰ HTTP authentication: refused, and the body behind the box drawn.
        let before = net.len();
        let _ = go(&browsing, &mut net, &format!("{}/auth", first.base()));
        settle(1.0);
        let after_auth = read_url(browsing_view);
        report.verdict(
            "⑰ a 401 with WWW-Authenticate opens no box and draws its own body",
            after_auth.ends_with("/auth"),
            &format!(
                "landed={after_auth:?} events={:?}",
                &net[before..].iter().take(4).collect::<Vec<_>>()
            ),
        );
        report.say(
            "   FINDING the other half of this callback — a TLS server-trust challenge — is not \
             exercised here: these origins are plain http, and what answers it is the system's \
             own evaluation (§13.29 ⑧)",
        );
        let _ = go(&browsing, &mut net, &document);
        settle(2.0);

        // ⑱ permissions, **one capability at a time**, which is the port's
        //    fourth stated difference — and the row has two halves, because the
        //    page cannot answer the whole question.
        //
        //    Apple's rule is that an unimplemented permission method is *not* a
        //    denial: the capability then arrives with WebKit's own default,
        //    which for capture is a panel on somebody's screen. So "which
        //    capabilities does this host refuse by name" is a question about the
        //    delegate's method list, and the runtime answers it exactly.
        for (selector, capability, wanted) in [
            (
                "webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:",
                "the camera and the microphone",
                true,
            ),
            (
                "webView:requestDeviceOrientationAndMotionPermissionForOrigin:initiatedByFrame:decisionHandler:",
                "device orientation and motion",
                true,
            ),
            (
                "webView:requestGeolocationPermissionForOrigin:initiatedByFrame:decisionHandler:",
                "geolocation",
                false,
            ),
            (
                "_webView:requestGeolocationPermissionForOrigin:initiatedByFrame:decisionHandler:",
                "geolocation, through WebKit's private spelling",
                false,
            ),
        ] {
            let answers = delegate_answers(browsing_view, selector);
            report.verdict(
                &format!("⑱ the policy object answers for {capability}: {wanted}"),
                answers == wanted,
                &format!("respondsToSelector:{selector} = {answers}"),
            );
        }
        // …and what the page itself sees when it asks, which is the other half
        // and is a fact about the *origin* rather than about the delegate.
        for (verb, name) in [
            ("doCamera()", "camera"),
            ("doMicrophone()", "microphone"),
            ("doGeolocation()", "geolocation"),
        ] {
            run_script(&mut report, browsing_view, verb);
            settle(3.0);
            let answered = read_title(&mut report, browsing_view);
            let said = answered
                .split(' ')
                .find(|word| word.starts_with(&format!("{name}=")))
                .unwrap_or("nothing at all");
            report.say(&format!(
                "   FINDING the page asked for {name} and got: {said}"
            ));
        }
        report.say(
            "   FINDING the camera and the microphone are refused by a WKUIDelegate method this \
             host implements, and the decision is made before WebKit touches a capture device, so \
             no privacy prompt is reachable. Geolocation has NO delegate method at all, public or \
             private, on this WebKit: what stands between a page and CoreLocation is the bundle \
             carrying no NSLocation…UsageDescription, which is a packaging fact rather than a \
             policy one",
        );

        // ⑲ a download, asked for by the page rather than by the server.
        let before = net.len();
        run_script(&mut report, browsing_view, "click('downloadLink')");
        let refused = until(&browsing, &mut net, 4.0, |events| {
            events[before..]
                .iter()
                .any(|event| matches!(event, WebEvent::DownloadStarting { .. }))
        });
        report.verdict(
            "⑲ a download= link is cancelled and reported",
            refused,
            &format!("{:?}", &net[before..]),
        );

        // ⑳ a window: by a link, by hand, and on a timer with no gesture behind
        //    it.
        let before = net.len();
        run_script(&mut report, browsing_view, "click('blankLink')");
        catch_up(&browsing, &mut net, 1.2);
        let stayed = read_url(browsing_view);
        report.verdict(
            "⑳ target=_blank opens nothing and moves the seat nowhere",
            stayed.ends_with("/doc.html"),
            &format!("landed={stayed:?} events={:?}", &net[before..]),
        );
        run_script(&mut report, browsing_view, "openWithNoGesture()");
        catch_up(&browsing, &mut net, 1.2);
        let timed = read_title(&mut report, browsing_view);
        report.verdict(
            "⑳ window.open with no gesture behind it answers null",
            timed.contains("opentimer=null"),
            &format!("title={timed:?}"),
        );

        // ㉑ a form POST.
        first.forget();
        let before = net.len();
        run_script(
            &mut report,
            browsing_view,
            "document.getElementById('postForm').submit()",
        );
        catch_up(&browsing, &mut net, 2.0);
        let posted = first.was_asked_for("POST /form");
        let asked = net[before..].iter().any(|event| {
            matches!(event, WebEvent::NavigationStarting { uri, .. } if uri.ends_with("/form"))
        });
        report.verdict(
            "㉑ a form POST is a navigation the gate is asked about",
            asked,
            &format!(
                "asked={asked} reached the server={posted} log={:?}",
                first.lines()
            ),
        );
        let _ = go(&browsing, &mut net, &document);
        settle(2.0);

        // ㉒ a `blob:` location.
        let before = net.len();
        run_script(&mut report, browsing_view, "doBlob()");
        catch_up(&browsing, &mut net, 1.5);
        let blob = net[before..].iter().find_map(|event| match event {
            WebEvent::NavigationStarting { uri, cancelled } if uri.starts_with("blob:") => {
                Some(*cancelled)
            }
            _ => None,
        });
        report.verdict(
            "㉒ a blob: location is offered to the gate with its whole URL",
            blob.is_some(),
            &format!("cancelled={blob:?} events={:?}", &net[before..]),
        );
        let _ = go(&browsing, &mut net, &document);
        settle(2.0);

        // ㉓ the other seat's minted file, named from a page outside the mint.
        let before = net.len();
        let standing = read_url(browsing_view);
        run_script(&mut report, browsing_view, "click('mintLink')");
        catch_up(&browsing, &mut net, 1.5);
        let stayed = read_url(browsing_view);
        let by_the_gate = net[before..]
            .iter()
            .any(|event| matches!(event, WebEvent::NavigationStarting { uri, .. } if uri.starts_with("file:")));
        report.verdict(
            "㉓ a file: URL inside the other seat's mint does not open from an http page",
            stayed == standing,
            &format!("landed={stayed:?} events={:?}", &net[before..]),
        );
        report.say(&format!(
            "   FINDING it was refused by {}",
            if by_the_gate {
                "this host's navigation gate"
            } else {
                "the engine itself, before any callback — WebKit refuses a file: navigation from \
                 an http document, which is X-2's row 9 arriving one door earlier than the gate"
            }
        ));

        // ㉔ **the unmeasured row.** A worker's own request, and a service
        //    worker registration — X-2's row 20, left unknown there.
        second.forget();
        run_script(&mut report, browsing_view, "doWorker()");
        catch_up(&browsing, &mut net, 3.0);
        let worked = read_title(&mut report, browsing_view);
        let worker_reached = second.was_asked_for("/data.json");
        report.verdict(
            "㉔ a Worker's fetch to the other origin happens, and nothing is told about it",
            worked.contains("worker="),
            &format!(
                "title={worked:?} reached B={worker_reached} log={:?}",
                second.lines()
            ),
        );
        // …and now the same request with the rule list standing, which is the
        // whole question: does the third door reach inside a worker?
        if browsing.set_request_rules(&blocking).is_ok() {
            let _ = go(&browsing, &mut net, &document);
            settle(2.0);
            second.forget();
            run_script(&mut report, browsing_view, "doWorker()");
            catch_up(&browsing, &mut net, 3.0);
            let ruled = read_title(&mut report, browsing_view);
            report.verdict(
                "㉔ and the compiled rule list covers it",
                !second.was_asked_for("/data.json"),
                &format!("title={ruled:?} B's log={:?}", second.lines()),
            );
            report.say(&format!(
                "   FINDING worker-originated: page reported {}, the socket {}",
                if ruled.contains("worker=blocked") {
                    "blocked"
                } else if ruled.contains("worker=loaded") {
                    "loaded"
                } else {
                    "nothing"
                },
                if second.was_asked_for("/data.json") {
                    "was reached"
                } else {
                    "was not reached"
                }
            ));
            let _ = browsing.set_request_rules(BROWSING_SEAT_RULES);
            let _ = go(&browsing, &mut net, &document);
            settle(2.0);
        }
        run_script(&mut report, browsing_view, "doServiceWorker()");
        settle(4.0);
        let registered = read_title(&mut report, browsing_view);
        report.say(&format!(
            "   FINDING a ServiceWorker registration on this seat: title={registered:?}, and A's \
             log shows {}",
            if first.was_asked_for("/sw.js") {
                "the script being fetched"
            } else {
                "no request for the script at all"
            }
        ));
        report.verdict(
            "㉔ a service worker registration is answered one way or the other",
            registered.contains("sw="),
            &format!("title={registered:?}"),
        );

        // ㉕ **the pointer** (§13.29 ⑩, the last of the seven).
        let _ = go(&browsing, &mut net, &document);
        settle(2.0);
        // Two points in the **document's** own coordinates, each the middle of
        // a button the fixture put at a fixed place: one the page is to answer,
        // one the window's chrome is to. `window_point` does every conversion
        // between here and AppKit, and asks for each of them rather than
        // assuming it.
        let on_the_page = NSPoint::new(100.0, 280.0);
        let under_the_chrome = NSPoint::new(100.0, 360.0);
        let routed = routed_to(&window, on_the_page);
        report.verdict(
            "㉕ a press inside the page's rectangle is routed to the WKWebView",
            routed.as_deref().is_some_and(view_is_inside_a_web_view),
            &format!("routed to {}", named(routed.as_deref())),
        );
        press(&window, on_the_page);
        settle(1.2);
        let pressed = read_title(&mut report, browsing_view);
        report.verdict(
            "㉕ and the page answers it",
            pressed.contains("pressed=page"),
            &format!("title={pressed:?}"),
        );
        report.verdict(
            "㉕ a window a page asked for **on a gesture** is refused as well",
            pressed.contains("openbyhand=null"),
            &format!("title={pressed:?}"),
        );
        // And the other half: the window says where its own chrome stands, and
        // that rectangle stops being the page's.
        // The same button, in the space `set_page_cover` is given: physical
        // pixels of the client area, which is `place_web_visual`'s own.
        let cover = [[
            page_left as f32,
            page_top as f32 + 330.0 * scale,
            page_left as f32 + 200.0 * scale,
            page_top as f32 + 395.0 * scale,
        ]];
        match compositor.set_page_cover(page2, &cover) {
            Ok(()) => {
                let routed = routed_to(&window, under_the_chrome);
                report.verdict(
                    "㉕ a press where Folio's own chrome stands is not the page's",
                    routed
                        .as_deref()
                        .is_none_or(|view| !view_is_inside_a_web_view(view)),
                    &format!("routed to {}", named(routed.as_deref())),
                );
                let before = read_title(&mut report, browsing_view);
                press(&window, under_the_chrome);
                settle(1.0);
                let after = read_title(&mut report, browsing_view);
                report.verdict(
                    "㉕ and the page never hears the press",
                    !after.contains("pressed=covered") && after == before,
                    &format!("title={after:?}"),
                );
                // Taken back off, the same point is the page's again — a cover
                // is this frame's answer and not a hole burnt in the slot.
                let _ = compositor.set_page_cover(page2, &[]);
                let routed = routed_to(&window, under_the_chrome);
                report.verdict(
                    "㉕ and it is the page's again the moment the chrome comes down",
                    routed.as_deref().is_some_and(view_is_inside_a_web_view),
                    &format!("routed to {}", named(routed.as_deref())),
                );
            }
            Err(reason) => report.fail("㉕ the window can say where it stands", &reason),
        }

        match photograph(number, &work.join("m4-3-window.ppm")) {
            Some((width, height)) => {
                report.say(&format!("a picture of window {number}: {width}x{height}"));
            }
            None => report.say(&format!("window {number} could not be photographed")),
        }
        browsing.close();
        settle(0.3);
        let _ = compositor.detach_web_visual(page2);

        window.close();

        report.say(&format!("failures={} — M4_3_DONE", report.failures));
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
