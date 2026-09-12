//! X-2 — WKWebView policy enforcement, probed.
//!
//! A scratch probe, outside Folio's workspace. It asks one question: every
//! place the Windows host decides whether a web request may happen — top-level
//! navigation, a frame, a subresource, a redirect, a `file:` URL, a download, a
//! popup, a script location change, a `data:` URL, an external scheme, an HTTP
//! authentication challenge — where is the *public* WKWebView API that decides
//! the same thing, and what does it not reach?
//!
//! The Windows host's broadest hook is `WebResourceRequested` with
//! `AddWebResourceRequestedFilter("*", ..._CONTEXT_ALL)`
//! (`crates/bt-platform/src/webview.rs`), which is asked about **every**
//! request in every context. WKWebView has no such event. The probe therefore
//! measures two things at once:
//!
//! * what each delegate callback is asked about, with its arguments, and
//! * what the two local HTTP servers actually receive — because a request that
//!   reaches the server is a request no policy stopped.
//!
//! The servers are the ground truth. A subresource that arrives at origin B's
//! socket while no delegate callback mentioned it is the gap, spelled out in
//! bytes.
//!
//! Everything is logged to `~/folio-port/logs/x2/probe-x2-run.log`; the first
//! line is the pid, which is the only pid the launcher will ever end.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};
use objc2_foundation::{
    NSError, NSObjectProtocol, NSString, NSURLAuthenticationChallenge, NSURLCredential,
    NSURLRequest, NSURLResponse, NSURLSessionAuthChallengeDisposition, NSURL,
};
use objc2_web_kit::{
    WKContentRuleList, WKContentRuleListStore, WKDownload, WKFrameInfo, WKNavigation,
    WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKNavigationResponse,
    WKNavigationResponsePolicy, WKNavigationType, WKUIDelegate, WKURLSchemeHandler, WKURLSchemeTask,
    WKWebView, WKWebViewConfiguration, WKWebsiteDataStore, WKWindowFeatures,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

// ---------------------------------------------------------------- logging ---

static LOG: OnceLock<Mutex<File>> = OnceLock::new();
static PHASE: AtomicUsize = AtomicUsize::new(0);
static PHASE_NAMES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn out_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned());
    let dir = PathBuf::from(home).join("folio-port/logs/x2");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn logfile() -> &'static Mutex<File> {
    LOG.get_or_init(|| {
        let path = out_dir().join("probe-x2-run.log");
        Mutex::new(File::create(path).expect("probe log"))
    })
}

fn phase_names() -> &'static Mutex<Vec<String>> {
    PHASE_NAMES.get_or_init(|| Mutex::new(vec!["boot".to_owned()]))
}

fn phase() -> String {
    let i = PHASE.load(Ordering::SeqCst);
    phase_names()
        .lock()
        .map(|names| names.get(i).cloned().unwrap_or_default())
        .unwrap_or_default()
}

fn enter_phase(name: &str) {
    let mut names = phase_names().lock().expect("phases");
    names.push(name.to_owned());
    PHASE.store(names.len() - 1, Ordering::SeqCst);
}

macro_rules! say {
    ($($arg:tt)*) => {{
        let line = format!($($arg)*);
        eprintln!("{line}");
        if let Ok(mut f) = logfile().lock() {
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    }};
}

// ------------------------------------------------------------ the servers ---

struct Resp {
    status: u16,
    reason: &'static str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Resp {
    fn html(body: String) -> Self {
        Self {
            status: 200,
            reason: "OK",
            headers: vec![("Content-Type".into(), "text/html; charset=utf-8".into())],
            body: body.into_bytes(),
        }
    }
    fn bytes(kind: &str, body: Vec<u8>) -> Self {
        Self {
            status: 200,
            reason: "OK",
            headers: vec![("Content-Type".into(), kind.into())],
            body,
        }
    }
    fn not_found() -> Self {
        Self {
            status: 404,
            reason: "Not Found",
            headers: vec![("Content-Type".into(), "text/plain".into())],
            body: b"no".to_vec(),
        }
    }
}

/// The 1x1 PNG every picture fixture is; what matters is that the byte hits
/// the socket, not what it decodes to.
const PIXEL: [u8; 69] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

type Handler = Arc<dyn Fn(&str) -> Resp + Send + Sync>;

fn spawn_server(tag: &'static str, handler: Handler) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let handler = Arc::clone(&handler);
            std::thread::spawn(move || {
                let mut buffer = [0u8; 8192];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let text = String::from_utf8_lossy(&buffer[..read]).to_string();
                let mut lines = text.lines();
                let first = lines.next().unwrap_or("").to_owned();
                let path = first.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let mut referer = String::from("-");
                let mut authorization = String::from("-");
                let mut destination = String::from("-");
                let mut origin = String::from("-");
                for line in lines {
                    let lowered = line.to_ascii_lowercase();
                    if let Some(value) = lowered.strip_prefix("referer:") {
                        referer = value.trim().to_owned();
                    } else if lowered.starts_with("authorization:") {
                        authorization = String::from("present");
                    } else if let Some(value) = lowered.strip_prefix("sec-fetch-dest:") {
                        destination = value.trim().to_owned();
                    } else if let Some(value) = lowered.strip_prefix("origin:") {
                        origin = value.trim().to_owned();
                    }
                }
                say!(
                    "[{}] HIT {tag} {path} dest={destination} referer={referer} origin={origin} authorization={authorization}",
                    phase()
                );
                let response = handler(&path);
                let mut head = format!("HTTP/1.1 {} {}\r\n", response.status, response.reason);
                for (name, value) in &response.headers {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
                head.push_str(&format!("Content-Length: {}\r\n", response.body.len()));
                head.push_str("Connection: close\r\n\r\n");
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&response.body);
                let _ = stream.flush();
            });
        }
    });
    port
}

// ----------------------------------------------------------- the fixtures ---

static PORTS: OnceLock<(u16, u16)> = OnceLock::new();

fn ports() -> (u16, u16) {
    *PORTS.get().expect("ports")
}

fn origin_a() -> String {
    format!("http://127.0.0.1:{}", ports().0)
}

fn origin_b() -> String {
    format!("http://127.0.0.1:{}", ports().1)
}

/// The browsing-seat fixture: one document that names everything a page can
/// name. Each picture reports its own outcome into `document.title`, which is
/// the only channel a probe with no host bridge has — and `WKWebView.title` is
/// a public property this reads back.
fn index_page() -> String {
    let a = origin_a();
    let b = origin_b();
    format!(
        r##"<!doctype html><html><head><meta charset="utf-8"><title>x2</title>
<link rel="stylesheet" href="{b}/style.css">
<script src="{b}/third.js"></script>
<script>
function report(k, v) {{ document.title = document.title + " " + k + "=" + v; }}
function fetchB() {{ fetch("{b}/fetched.txt").then(function(){{report("fetch","ok")}}, function(){{report("fetch","blocked")}}); }}
function openWin() {{ var w = window.open("{a}/popup.html", "_blank"); report("windowopen", w ? "returned" : "null"); }}
function locChange() {{ location.href = "{a}/target.html"; }}
function goAuth() {{ location.href = "{a}/auth"; }}
function click(id) {{ document.getElementById(id).click(); }}
</script></head><body>
<h1>X-2 browsing seat</h1>
<img id="crossimg" src="{b}/img.png" onload="report('crossimg','loaded')" onerror="report('crossimg','blocked')">
<img id="fileimg" src="file:///etc/hosts" onload="report('fileimg','loaded')" onerror="report('fileimg','blocked')">
<iframe id="crossframe" src="{b}/frame.html" width="80" height="40"></iframe>
<p>
<a id="blank" href="{a}/target.html" target="_blank">blank</a>
<a id="fileLink" href="file:///etc/hosts">file</a>
<a id="dataLink" href="data:text/html,%3Cb%3Edata%3C/b%3E">data</a>
<a id="redir" href="{a}/redirect">redirect</a>
<a id="dl" href="{a}/download.bin">download</a>
<a id="mail" href="mailto:someone@example.com">mail</a>
</p>
</body></html>"##
    )
}

/// The local-file seat's document, written into a folder of its own. Its
/// siblings are the whole question: `inside.png` is in the folder the seat was
/// opened on, `../outside/secret.png` is not, and `{b}/img.png` is the network
/// a previewed local page must not reach (`webnav::resource_request`'s
/// `Mint::File` arm).
fn report_page() -> String {
    let b = origin_b();
    format!(
        r##"<!doctype html><html><head><meta charset="utf-8"><title>x2file</title>
<script>function report(k, v) {{ document.title = document.title + " " + k + "=" + v; }}</script>
</head><body>
<h1>X-2 local seat</h1>
<img id="inside" src="inside.png" onload="report('inside','loaded')" onerror="report('inside','blocked')">
<img id="outside" src="../outside/secret.png" onload="report('outside','loaded')" onerror="report('outside','blocked')">
<img id="net" src="{b}/img.png" onload="report('net','loaded')" onerror="report('net','blocked')">
<iframe id="outsideframe" src="../outside/secret.html" width="80" height="40"
  onload="report('outsideframe','onload')"></iframe>
<a id="away" href="{b}/frame.html">away</a>
</body></html>"##
    )
}

fn write_file_seat() -> (PathBuf, PathBuf) {
    let root = out_dir().join("seat");
    let inside = root.join("open");
    let outside = root.join("outside");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&inside).expect("seat dir");
    std::fs::create_dir_all(&outside).expect("outside dir");
    std::fs::write(inside.join("report.html"), report_page()).expect("report");
    std::fs::write(inside.join("inside.png"), PIXEL).expect("inside");
    std::fs::write(outside.join("secret.png"), PIXEL).expect("secret");
    std::fs::write(
        outside.join("secret.html"),
        "<!doctype html><title>secret</title><p>the sibling folder</p>",
    )
    .expect("secret html");
    (inside.join("report.html"), inside)
}

// ------------------------------------------------------ the two rule lists --

thread_local! {
    static RULE_LISTS: std::cell::RefCell<HashMap<String, Retained<WKContentRuleList>>> =
        std::cell::RefCell::new(HashMap::new());
}

fn compile_rule_list(mtm: MainThreadMarker, identifier: &str, json: &str) {
    let dir = out_dir().join("rulelists");
    let _ = std::fs::create_dir_all(&dir);
    let url = unsafe { NSURL::fileURLWithPath(&NSString::from_str(&dir.to_string_lossy())) };
    let Some(store) = (unsafe { WKContentRuleListStore::storeWithURL(Some(&url), mtm) }) else {
        say!("RULELIST {identifier}: no store");
        return;
    };
    let name = identifier.to_owned();
    let done = RcBlock::new(move |list: *mut WKContentRuleList, error: *mut NSError| {
        if !error.is_null() {
            let error = unsafe { &*error };
            say!(
                "RULELIST {name}: COMPILE FAILED {}",
                unsafe { error.localizedDescription() }.to_string()
            );
            return;
        }
        if list.is_null() {
            say!("RULELIST {name}: no list and no error");
            return;
        }
        let retained = unsafe { Retained::retain(list) }.expect("rule list");
        say!("RULELIST {name}: compiled");
        RULE_LISTS.with(|lists| {
            lists.borrow_mut().insert(name.clone(), retained);
        });
    });
    unsafe {
        store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
            Some(&NSString::from_str(identifier)),
            Some(&NSString::from_str(json)),
            Some(&done),
        );
    }
}

// ------------------------------------------------------------- the policy ---

/// The probe's stand-in for `bt_app::webnav`: the same shape of answer, from
/// the same inputs, so that "where can the rule be called" is asked about the
/// rule Folio actually has.
fn allowed(url: &str) -> bool {
    let phase = phase();
    let lowered = url.to_ascii_lowercase();
    if lowered.starts_with("data:")
        || lowered.starts_with("javascript:")
        || lowered.starts_with("blob:")
    {
        return false;
    }
    if lowered.starts_with("mailto:") || lowered.starts_with("ftp:") || lowered.starts_with("tel:")
    {
        return false;
    }
    if phase.starts_with("file") {
        // A minted local seat: only the file it was opened on, and nothing on
        // the network. `webnav::check`'s `Mint::File` arm, in one line.
        return lowered.starts_with("file:") && lowered.contains("/seat/open/");
    }
    if lowered.starts_with("file:") {
        return false;
    }
    if lowered.starts_with("about:blank") {
        return true;
    }
    lowered.starts_with("http://") || lowered.starts_with("https://")
}

// ----------------------------------------------------------- the delegate ---

fn url_of(request: Retained<NSURLRequest>) -> String {
    unsafe { request.URL() }
        .and_then(|url| unsafe { url.absoluteString() })
        .map(|text| text.to_string())
        .unwrap_or_else(|| String::from("<none>"))
}

fn navigation_type_name(kind: WKNavigationType) -> &'static str {
    match kind {
        WKNavigationType::LinkActivated => "LinkActivated",
        WKNavigationType::FormSubmitted => "FormSubmitted",
        WKNavigationType::BackForward => "BackForward",
        WKNavigationType::Reload => "Reload",
        WKNavigationType::FormResubmitted => "FormResubmitted",
        WKNavigationType::Other => "Other",
        _ => "Unknown",
    }
}

fn frame_note(label: &str, frame: Option<Retained<WKFrameInfo>>) -> String {
    match frame {
        None => format!("{label}=nil"),
        Some(frame) => {
            let main = unsafe { frame.isMainFrame() };
            let url = url_of(unsafe { frame.request() });
            format!("{label}={{main={main}, url={url}}}")
        }
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and Gate implements no
    // Drop. Every delegate method here is called on the main thread, which is
    // what MainThreadOnly states.
    #[unsafe(super(objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FolioProbeX2Gate"]
    #[ivars = ()]
    struct Gate;

    unsafe impl NSObjectProtocol for Gate {}

    unsafe impl WKNavigationDelegate for Gate {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide_action(
            &self,
            _web_view: &WKWebView,
            action: &WKNavigationAction,
            handler: &block2::DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            let url = url_of(unsafe { action.request() });
            let kind = navigation_type_name(unsafe { action.navigationType() });
            let target = frame_note("target", unsafe { action.targetFrame() });
            let source = frame_note("source", unsafe { action.sourceFrame() });
            let download = unsafe { action.shouldPerformDownload() };
            let verdict = allowed(&url);
            say!(
                "[{}] ACTION url={url} type={kind} {target} {source} shouldPerformDownload={download} -> {}",
                phase(),
                if verdict { "Allow" } else { "CANCEL" }
            );
            handler.call((if verdict {
                WKNavigationActionPolicy::Allow
            } else {
                WKNavigationActionPolicy::Cancel
            },));
        }

        #[unsafe(method(webView:decidePolicyForNavigationResponse:decisionHandler:))]
        fn decide_response(
            &self,
            _web_view: &WKWebView,
            response: &WKNavigationResponse,
            handler: &block2::DynBlock<dyn Fn(WKNavigationResponsePolicy)>,
        ) {
            let raw: Retained<NSURLResponse> = unsafe { response.response() };
            let url = unsafe { raw.URL() }
                .and_then(|url| unsafe { url.absoluteString() })
                .map(|text| text.to_string())
                .unwrap_or_else(|| String::from("<none>"));
            let mime = unsafe { raw.MIMEType() }
                .map(|text| text.to_string())
                .unwrap_or_else(|| String::from("-"));
            let suggested = unsafe { raw.suggestedFilename() }
                .map(|text| text.to_string())
                .unwrap_or_else(|| String::from("-"));
            let main = unsafe { response.isForMainFrame() };
            let showable = unsafe { response.canShowMIMEType() };
            let status: i64 = if unsafe { msg_send![&*raw, respondsToSelector: sel!(statusCode)] } {
                unsafe { msg_send![&*raw, statusCode] }
            } else {
                -1
            };
            // An attachment is the download door: this is the one callback that
            // sees `Content-Disposition`, and cancelling here is what the
            // Windows host does in `DownloadStarting`.
            let attachment = !showable || suggested.ends_with(".bin");
            say!(
                "[{}] RESPONSE url={url} mainFrame={main} status={status} mime={mime} suggested={suggested} canShowMIMEType={showable} -> {}",
                phase(),
                if attachment { "CANCEL (download)" } else { "Allow" }
            );
            handler.call((if attachment {
                WKNavigationResponsePolicy::Cancel
            } else {
                WKNavigationResponsePolicy::Allow
            },));
        }

        #[unsafe(method(webView:didStartProvisionalNavigation:))]
        fn did_start(&self, web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            let url = unsafe { web_view.URL() }
                .and_then(|url| unsafe { url.absoluteString() })
                .map(|text| text.to_string())
                .unwrap_or_default();
            say!("[{}] START url={url}", phase());
        }

        #[unsafe(method(webView:didReceiveServerRedirectForProvisionalNavigation:))]
        fn did_redirect(&self, web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            let url = unsafe { web_view.URL() }
                .and_then(|url| unsafe { url.absoluteString() })
                .map(|text| text.to_string())
                .unwrap_or_default();
            say!("[{}] SERVER-REDIRECT url={url}", phase());
        }

        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            let url = unsafe { web_view.URL() }
                .and_then(|url| unsafe { url.absoluteString() })
                .map(|text| text.to_string())
                .unwrap_or_default();
            say!("[{}] FINISH url={url}", phase());
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional(
            &self,
            _web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
            error: &NSError,
        ) {
            say!(
                "[{}] FAIL-PROVISIONAL {}",
                phase(),
                unsafe { error.localizedDescription() }.to_string()
            );
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(
            &self,
            _web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
            error: &NSError,
        ) {
            say!(
                "[{}] FAIL {}",
                phase(),
                unsafe { error.localizedDescription() }.to_string()
            );
        }

        #[unsafe(method(webView:didReceiveAuthenticationChallenge:completionHandler:))]
        fn did_challenge(
            &self,
            _web_view: &WKWebView,
            challenge: &NSURLAuthenticationChallenge,
            handler: &block2::DynBlock<
                dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential),
            >,
        ) {
            say!("[{}] AUTH callback entered", phase());
            let space = unsafe { challenge.protectionSpace() };
            say!("[{}] AUTH protectionSpace read", phase());
            let host = unsafe { space.host() }.to_string();
            say!("[{}] AUTH host={host}", phase());
            let method = unsafe { space.authenticationMethod() }.to_string();
            say!("[{}] AUTH method={method}", phase());
            let realm = unsafe { space.realm() }
                .map(|text| text.to_string())
                .unwrap_or_else(|| String::from("-"));
            say!(
                "[{}] AUTH host={host} realm={realm} method={method} -> RejectProtectionSpace",
                phase()
            );
            handler.call((
                NSURLSessionAuthChallengeDisposition::RejectProtectionSpace,
                std::ptr::null_mut(),
            ));
        }

        #[unsafe(method(webView:navigationAction:didBecomeDownload:))]
        fn action_became_download(
            &self,
            _web_view: &WKWebView,
            action: &WKNavigationAction,
            _download: &WKDownload,
        ) {
            say!(
                "[{}] ACTION-BECAME-DOWNLOAD url={}",
                phase(),
                url_of(unsafe { action.request() })
            );
        }

        #[unsafe(method(webView:navigationResponse:didBecomeDownload:))]
        fn response_became_download(
            &self,
            _web_view: &WKWebView,
            _response: &WKNavigationResponse,
            _download: &WKDownload,
        ) {
            say!("[{}] RESPONSE-BECAME-DOWNLOAD", phase());
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn content_process_died(&self, _web_view: &WKWebView) {
            say!("[{}] CONTENT-PROCESS-TERMINATED", phase());
        }
    }

    unsafe impl WKUIDelegate for Gate {
        #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
        fn create_web_view(
            &self,
            _web_view: &WKWebView,
            _configuration: &WKWebViewConfiguration,
            action: &WKNavigationAction,
            _features: &WKWindowFeatures,
        ) -> Option<Retained<WKWebView>> {
            let url = url_of(unsafe { action.request() });
            let kind = navigation_type_name(unsafe { action.navigationType() });
            let target = frame_note("target", unsafe { action.targetFrame() });
            say!(
                "[{}] CREATE-WEBVIEW url={url} type={kind} {target} -> nil (refused)",
                phase()
            );
            None
        }

        #[unsafe(method(webViewDidClose:))]
        fn did_close(&self, _web_view: &WKWebView) {
            say!("[{}] UI-DID-CLOSE", phase());
        }
    }
);

define_class!(
    // SAFETY: as above. The one method WebKit would call if it ever handed a
    // task to this object; it never does for a scheme WebKit handles itself,
    // which is the point of registering it.
    #[unsafe(super(objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FolioProbeX2SchemeHandler"]
    #[ivars = ()]
    struct SchemeHandler;

    unsafe impl NSObjectProtocol for SchemeHandler {}

    unsafe impl WKURLSchemeHandler for SchemeHandler {
        #[unsafe(method(webView:startURLSchemeTask:))]
        fn start_task(
            &self,
            _web_view: &WKWebView,
            _task: &ProtocolObject<dyn WKURLSchemeTask>,
        ) {
            say!("[{}] SCHEME-TASK-START", phase());
        }

        #[unsafe(method(webView:stopURLSchemeTask:))]
        fn stop_task(
            &self,
            _web_view: &WKWebView,
            _task: &ProtocolObject<dyn WKURLSchemeTask>,
        ) {
            say!("[{}] SCHEME-TASK-STOP", phase());
        }
    }
);

impl SchemeHandler {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

impl Gate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

// ------------------------------------------------------------- the driver ---

/// The autonomous timeline. Nobody is at the machine, so the probe drives
/// itself, and every step names the requirement it is asking about.
const STEPS: &[(f32, &str)] = &[
    (0.8, "phase:net"),
    (1.0, "load-index"),
    (4.5, "title"),
    (5.0, "js:click('blank')"),
    (7.0, "js:openWin()"),
    (9.0, "js:fetchB()"),
    (11.0, "js:click('dataLink')"),
    (13.0, "js:click('fileLink')"),
    (15.0, "js:click('mail')"),
    (17.0, "js:click('redir')"),
    (20.0, "load-index"),
    (23.0, "js:click('dl')"),
    (25.5, "js:locChange()"),
    (28.5, "phase:crl"),
    (28.7, "rule:blockB"),
    (29.0, "load-index"),
    (33.0, "title"),
    (34.0, "phase:file"),
    (34.2, "rule:none"),
    (34.5, "load-file"),
    (38.5, "title"),
    (39.0, "js:document.getElementById('away').click()"),
    (41.0, "phase:fileguard"),
    (41.2, "rule:fileguard"),
    (41.5, "load-file"),
    (45.5, "title"),
    // Last, because a 401 is the one step that has ended this process before.
    (46.5, "phase:auth"),
    (46.7, "rule:none"),
    (47.0, "load-index"),
    (50.0, "js:goAuth()"),
    (54.0, "title"),
    (55.0, "finish"),
];
struct App {
    window: Option<Window>,
    webview: Option<Retained<WKWebView>>,
    gate: Option<Retained<Gate>>,
    seat_file: Option<(PathBuf, PathBuf)>,
    t0: Instant,
    step: usize,
    beat: u32,
}

fn main() {
    say!("probe-x2 start, pid {}", std::process::id());
    // A panic inside a delegate callback unwinds into Objective-C and the
    // process is gone before stderr reaches anybody — and an `open`ed bundle
    // has no terminal to print to. The hook puts it in the log instead.
    std::panic::set_hook(Box::new(|info| {
        say!("PANIC {info}");
    }));

    let index = Arc::new(|path: &str| -> Resp {
        match path {
            "/" | "/index.html" => Resp::html(index_page()),
            "/target.html" => Resp::html(
                "<!doctype html><title>target</title><h1>target</h1>".to_owned(),
            ),
            "/popup.html" => {
                Resp::html("<!doctype html><title>popup</title><h1>popup</h1>".to_owned())
            }
            "/redirect" => Resp {
                status: 302,
                reason: "Found",
                headers: vec![("Location".into(), "/redirect2".into())],
                body: Vec::new(),
            },
            "/redirect2" => Resp {
                status: 302,
                reason: "Found",
                headers: vec![("Location".into(), "/target.html".into())],
                body: Vec::new(),
            },
            "/download.bin" => Resp {
                status: 200,
                reason: "OK",
                headers: vec![
                    ("Content-Type".into(), "application/octet-stream".into()),
                    (
                        "Content-Disposition".into(),
                        "attachment; filename=\"folio-x2.bin\"".into(),
                    ),
                ],
                body: b"attachment-bytes".to_vec(),
            },
            "/auth" => Resp {
                status: 401,
                reason: "Unauthorized",
                headers: vec![
                    (
                        "WWW-Authenticate".into(),
                        "Basic realm=\"folio-x2\"".into(),
                    ),
                    ("Content-Type".into(), "text/plain".into()),
                ],
                body: b"need auth".to_vec(),
            },
            _ => Resp::not_found(),
        }
    }) as Handler;

    let second = Arc::new(|path: &str| -> Resp {
        match path {
            "/img.png" => Resp::bytes("image/png", PIXEL.to_vec()),
            "/third.js" => Resp::bytes(
                "application/javascript",
                b"window.__thirdParty = true;".to_vec(),
            ),
            "/style.css" => Resp::bytes("text/css", b"h1 { color: teal; }".to_vec()),
            "/frame.html" => {
                Resp::html("<!doctype html><title>frame</title><p>frame</p>".to_owned())
            }
            "/fetched.txt" => Resp::bytes("text/plain", b"fetched".to_vec()),
            _ => Resp::not_found(),
        }
    }) as Handler;

    let port_a = spawn_server("A", index);
    let port_b = spawn_server("B", second);
    PORTS.set((port_a, port_b)).expect("ports once");
    say!("server A on {port_a} (the page's own origin)");
    say!("server B on {port_b} (the second origin: subresources, script, frame)");

    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        window: None,
        webview: None,
        gate: None,
        seat_file: None,
        t0: Instant::now(),
        step: 0,
        beat: u32::MAX,
    };
    event_loop.run_app(&mut app).expect("run");
    say!("PROBE_X2_DONE");
}

impl App {
    fn build(&mut self, el: &ActiveEventLoop) {
        let mtm = MainThreadMarker::new().expect("main thread");

        let attrs = Window::default_attributes()
            .with_title("Folio X-2 probe")
            .with_inner_size(PhysicalSize::new(1200u32, 900u32));
        let window = el.create_window(attrs).expect("window");
        window.focus_window();
        say!(
            "window: scale {}, inner {:?}",
            window.scale_factor(),
            window.inner_size()
        );

        let handle = window.window_handle().expect("handle").as_raw();
        let RawWindowHandle::AppKit(appkit) = handle else {
            panic!("not an AppKit window handle");
        };
        let content: &NSView = unsafe { &*(appkit.ns_view.as_ptr().cast::<NSView>()) };
        let bounds = content.bounds();

        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
        // §4.5's question, asked here rather than assumed: a non-persistent
        // store is the whole of "no cookies survive the seat".
        let store = unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) };
        unsafe { configuration.setWebsiteDataStore(&store) };
        say!(
            "dataStore isPersistent={}",
            unsafe { store.isPersistent() }
        );
        let preferences = unsafe { configuration.preferences() };
        // Without this, `window.open` from a script is refused by WebKit itself
        // and the popup door is never exercised.
        unsafe { preferences.setJavaScriptCanOpenWindowsAutomatically(true) };

        // `limitsNavigationsToAppBoundDomains` wants an `WKAppBoundDomains` key
        // in Info.plist. Set it and read it back, so the report can say what it
        // does in a bundle that does not declare one.
        unsafe { configuration.setLimitsNavigationsToAppBoundDomains(true) };
        say!(
            "limitsNavigationsToAppBoundDomains reads back as {}",
            unsafe { configuration.limitsNavigationsToAppBoundDomains() }
        );

        // **The `WKURLSchemeHandler` question, answered by the runtime rather
        // than by the documentation**: it is not an equivalent of
        // `WebResourceRequested`, because WebKit refuses to hand it a scheme it
        // already handles. Two measurements — the public predicate, and the
        // registration itself inside an Objective-C exception catch.
        for scheme in ["http", "https", "file", "about", "data", "blob", "folio-probe"] {
            say!(
                "handlesURLScheme({scheme}) = {}",
                unsafe { WKWebView::handlesURLScheme(&NSString::from_str(scheme), mtm) }
            );
        }
        let scheme_handler = SchemeHandler::new(mtm);
        for scheme in ["https", "file", "folio-probe"] {
            let configuration = configuration.clone();
            let scheme_handler = scheme_handler.clone();
            let attempted = objc2::exception::catch(std::panic::AssertUnwindSafe(move || unsafe {
                configuration.setURLSchemeHandler_forURLScheme(
                    Some(ProtocolObject::from_ref(&*scheme_handler)),
                    &NSString::from_str(scheme),
                );
            }));
            match attempted {
                Ok(()) => say!("SCHEME-HANDLER {scheme}: accepted (no exception)"),
                Err(exception) => say!(
                    "SCHEME-HANDLER {scheme}: raised {}",
                    exception
                        .map(|e| format!("{e:?}"))
                        .unwrap_or_else(|| String::from("<nil exception>"))
                ),
            }
        }

        let webview = unsafe {
            WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), bounds, &configuration)
        };
        webview.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        content.addSubview(&webview);

        let gate = Gate::new(mtm);
        unsafe {
            webview.setNavigationDelegate(Some(ProtocolObject::from_ref(&*gate)));
            webview.setUIDelegate(Some(ProtocolObject::from_ref(&*gate)));
        }
        say!("delegates attached: WKNavigationDelegate + WKUIDelegate on one object");

        compile_rule_list(
            mtm,
            "blockB",
            &format!(
                r#"[{{"trigger":{{"url-filter":"^http://127\\.0\\.0\\.1:{}/"}},"action":{{"type":"block"}}}}]"#,
                ports().1
            ),
        );
        compile_rule_list(
            mtm,
            "fileguard",
            r#"[{"trigger":{"url-filter":"outside"},"action":{"type":"block"}},{"trigger":{"url-filter":"^https?://"},"action":{"type":"block"}}]"#,
        );

        self.seat_file = Some(write_file_seat());
        self.webview = Some(webview);
        self.gate = Some(gate);
        self.window = Some(window);
    }

    fn act(&mut self, what: &str, el: &ActiveEventLoop) {
        let Some(webview) = self.webview.clone() else {
            return;
        };
        let mtm = MainThreadMarker::new().expect("main thread");
        say!("--- step {what}");
        if let Some(name) = what.strip_prefix("phase:") {
            enter_phase(name);
            return;
        }
        if let Some(name) = what.strip_prefix("rule:") {
            let controller = unsafe { webview.configuration().userContentController() };
            unsafe { controller.removeAllContentRuleLists() };
            if name != "none" {
                RULE_LISTS.with(|lists| match lists.borrow().get(name) {
                    Some(list) => {
                        unsafe { controller.addContentRuleList(list) };
                        say!("rule list {name} attached");
                    }
                    None => say!("rule list {name} MISSING — it never compiled"),
                });
            } else {
                say!("rule lists cleared");
            }
            return;
        }
        if what == "load-index" {
            let url = format!("{}/index.html", origin_a());
            let ns = unsafe { NSURL::URLWithString(&NSString::from_str(&url)) }.expect("url");
            let request = unsafe { NSURLRequest::requestWithURL(&ns) };
            let _ = unsafe { webview.loadRequest(&request) };
            return;
        }
        if what == "load-file" {
            let (file, folder) = self.seat_file.clone().expect("seat");
            let url =
                unsafe { NSURL::fileURLWithPath(&NSString::from_str(&file.to_string_lossy())) };
            let read =
                unsafe { NSURL::fileURLWithPath(&NSString::from_str(&folder.to_string_lossy())) };
            say!(
                "loadFileURL {} allowingReadAccessToURL {}",
                file.display(),
                folder.display()
            );
            let _ = unsafe { webview.loadFileURL_allowingReadAccessToURL(&url, &read) };
            return;
        }
        if what == "title" {
            let title = unsafe { webview.title() }
                .map(|text| text.to_string())
                .unwrap_or_default();
            let url = unsafe { webview.URL() }
                .and_then(|url| unsafe { url.absoluteString() })
                .map(|text| text.to_string())
                .unwrap_or_default();
            say!("[{}] TITLE-REPORT url={url} title={title:?}", phase());
            return;
        }
        if let Some(code) = what.strip_prefix("js:") {
            let done = RcBlock::new(move |_value: *mut AnyObject, error: *mut NSError| {
                if !error.is_null() {
                    let error = unsafe { &*error };
                    say!(
                        "js error: {}",
                        unsafe { error.localizedDescription() }.to_string()
                    );
                }
            });
            unsafe {
                webview.evaluateJavaScript_completionHandler(
                    &NSString::from_str(code),
                    Some(&done),
                );
            }
            let _ = mtm;
            return;
        }
        if what == "finish" {
            say!("all steps done");
            el.exit();
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_none() {
            self.build(el);
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::CloseRequested) {
            el.exit();
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let elapsed = self.t0.elapsed().as_secs_f32();
        let beat = elapsed as u32;
        if beat != self.beat {
            self.beat = beat;
            say!("[{}] heartbeat t={beat}", phase());
        }
        while self.step < STEPS.len() && STEPS[self.step].0 <= elapsed {
            let what = STEPS[self.step].1;
            self.step += 1;
            self.act(what, el);
        }
        if self.step >= STEPS.len() {
            el.exit();
        }
    }
}
