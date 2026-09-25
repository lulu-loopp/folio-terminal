//! **The page host, on a machine with neither engine** — see the note on the
//! `portable` module declaration at the end of `webview.rs`.
//!
//! The twelve data types the window and the engine speak in live in
//! `webview.rs` and are compiled on every platform; this is the thirteenth. It
//! stood in for macOS too until M4-2 wrote `macos_webview.rs`, and what is left
//! for it to say is the honest answer on a Linux server: there is no web
//! preview here.
//!
//! **Every door this file has, the other two arms have, spelled the same way**
//! — `web_host_contract_tests` in `lib.rs` reads the three as text and holds
//! them to one set, because `bt-app` names `WebHost` with no `cfg` and no
//! compiler on one machine can check more than one of them.

use std::cell::Cell;
use std::path::Path;

use super::{
    NativeWindow, PageVisual, RehostCompensation, RehostOutcome, RehostSide, RehostStep, WebChord,
    WebColorScheme, WebDpiOwnership, WebEvent, WebInstallReport, WebMouseEvent,
    WebNavigationVerdict, WebRequestVerdict,
};
use crate::{Compositor, EnvironmentAnswer, WebWarmUp};

/// What every door of this host answers with.
fn no_engine(what: &str) -> String {
    format!("{what} is not on this platform yet")
}

/// **A web seat's engine, before there is one.**
///
/// Constructible and empty: `bt_app::webhost::WebSeat::open` builds one, hands
/// it three closures and then asks it for an environment, and it is *that* ask
/// that refuses. The three closures are held rather than dropped because they
/// are the seat's own policy — the navigation gate, the request gate and the
/// wake — and M4-2 calls all three from a `WKNavigationDelegate`; a host that
/// threw them away would be a different type by then.
pub struct WebHost {
    #[expect(
        dead_code,
        reason = "the macOS arm calls it from -webView:decidePolicyForNavigationAction:"
    )]
    gate: Box<dyn Fn(&str) -> WebNavigationVerdict>,
    #[expect(
        dead_code,
        reason = "the macOS arm asks it about a subframe; the rest is a compiled rule list"
    )]
    request_gate: Box<dyn Fn(&str) -> WebRequestVerdict>,
    #[expect(
        dead_code,
        reason = "the macOS arm wakes the loop when a delegate answers"
    )]
    wake: Box<dyn Fn()>,
    /// The colour scheme the seat said its pages prefer — kept, like the other two arms keep
    /// it, so that the seat's own record of what it said reads the same on every machine.
    color_scheme: Cell<Option<WebColorScheme>>,
}

impl WebHost {
    /// Build the host. Cannot fail: the return type is `Self`, and the seat
    /// holds one whether or not a page ever opens in it.
    #[must_use]
    pub fn new(
        gate: Box<dyn Fn(&str) -> WebNavigationVerdict>,
        request_gate: Box<dyn Fn(&str) -> WebRequestVerdict>,
        wake: Box<dyn Fn()>,
    ) -> Self {
        Self {
            gate,
            request_gate,
            wake,
            color_scheme: Cell::new(None),
        }
    }

    /// Which colour scheme this seat's pages prefer. Remembered and told to nobody: there is no
    /// page here to prefer anything.
    pub fn set_color_scheme(&self, scheme: WebColorScheme) -> Result<(), String> {
        self.color_scheme.set(Some(scheme));
        Ok(())
    }

    /// The scheme this host was last told, `None` before it was told one.
    #[must_use]
    pub fn color_scheme(&self) -> Option<WebColorScheme> {
        self.color_scheme.get()
    }

    /// Everything the engine has said since the last drain. Nothing, ever —
    /// there is no engine to say anything, and the seat's state machine reads
    /// an empty drain as "no news", which is true.
    #[must_use]
    pub fn drain(&self) -> Vec<WebEvent> {
        Vec::new()
    }

    /// The chords this window keeps from a focused page. Remembered by nobody,
    /// because no page is ever focused.
    pub fn set_claimed_chords(&self, chords: Vec<WebChord>) {
        let _ = chords;
    }

    /// The caller's resource rule, compiled. Nothing to compile it for.
    pub fn set_request_rules(&self, rules: &str) -> Result<(), String> {
        let _ = rules;
        Ok(())
    }

    /// Whether a controller is in service. Never.
    #[must_use]
    pub fn has_controller(&self) -> bool {
        false
    }

    /// **The refusal, and the one place it is said.**
    ///
    /// The first thing a seat asks for and the last thing that happens to it:
    /// `bt_app::webhost` turns the `Err` into the seat's fault state, which is
    /// a banner in the pane naming the reason rather than an empty rectangle.
    pub fn request_environment(&mut self, folder: &Path, generation: u64) -> Result<(), String> {
        let _ = (folder, generation);
        Err(no_engine("the web preview"))
    }

    /// Ask for a controller on this window. Unreachable: no environment.
    pub fn request_controller(
        &mut self,
        window: NativeWindow,
        generation: u64,
    ) -> Result<(), String> {
        let _ = (window, generation);
        Err(no_engine("the web preview"))
    }

    /// Take the controller into service. Unreachable: no controller.
    pub fn install(
        &mut self,
        compositor: &Compositor,
        page: PageVisual,
        generation: u64,
    ) -> Result<WebInstallReport, String> {
        let _ = (compositor, page, generation);
        Err(no_engine("the web preview"))
    }

    /// Hand a live page from one window to another. Unreachable: no page.
    ///
    /// `KeptSource` and not `Lost`, because the two mean different things to
    /// the tear-out that asked: `Lost` says the page is gone and the tab may
    /// move anyway, and `KeptSource` says nothing moved — which is the truth.
    pub fn rehost(
        &mut self,
        from: &RehostSide<'_>,
        to: &RehostSide<'_>,
        rect: (i32, i32, u32, u32),
        visible: bool,
    ) -> RehostOutcome {
        let _ = (from, to, rect, visible);
        RehostOutcome::KeptSource {
            failed_at: RehostStep::Hide,
            error: no_engine("the web preview"),
            compensation: RehostCompensation::default(),
        }
    }

    /// Where the seat is. Nothing to tell.
    pub fn set_bounds(&self, x: i32, y: i32, width: u32, height: u32) -> Result<(), String> {
        let _ = (x, y, width, height);
        Ok(())
    }

    /// The scale a page rasterizes at. Nothing to tell.
    pub fn set_rasterization_scale(&self, scale: f64) -> Result<(), String> {
        let _ = scale;
        Ok(())
    }

    /// The window moved. Nothing to tell.
    pub fn notify_parent_window_moved(&self) -> Result<(), String> {
        Ok(())
    }

    /// On the glass, or off it. Nothing to show either way.
    pub fn set_visible(&self, visible: bool) -> Result<(), String> {
        let _ = visible;
        Ok(())
    }

    /// Go to an address. Unreachable: the seat has no engine and says so.
    pub fn navigate(&self, url: &str) -> Result<(), String> {
        let _ = url;
        Err(no_engine("the web preview"))
    }

    /// Read the page again. Unreachable.
    pub fn reload(&self) -> Result<(), String> {
        Err(no_engine("the web preview"))
    }

    /// Stop reading. There is nothing in flight.
    pub fn stop(&self) -> Result<(), String> {
        Ok(())
    }

    /// Back through the history. Unreachable.
    pub fn go_back(&self) -> Result<(), String> {
        Err(no_engine("the web preview"))
    }

    /// Forward through the history. Unreachable.
    pub fn go_forward(&self) -> Result<(), String> {
        Err(no_engine("the web preview"))
    }

    /// The engine's own inspector. Unreachable.
    pub fn open_dev_tools(&self) -> Result<(), String> {
        Err(no_engine("the web preview"))
    }

    /// The zoom the page is at. One, which is what a page nobody has zoomed is
    /// at — the row reads *100%* rather than blank.
    #[must_use]
    pub fn zoom(&self) -> f64 {
        1.0
    }

    /// Set the zoom. Unreachable.
    pub fn set_zoom(&self, factor: f64) -> Result<(), String> {
        let _ = factor;
        Err(no_engine("the web preview"))
    }

    /// Find in page. Unreachable.
    pub fn find(&self, term: &str, case_sensitive: bool) -> Result<(), String> {
        let _ = (term, case_sensitive);
        Err(no_engine("the web preview"))
    }

    /// The next match. Unreachable.
    pub fn find_step(&self, forwards: bool) -> Result<(), String> {
        let _ = forwards;
        Err(no_engine("the web preview"))
    }

    /// Put the find away. Nothing to put away.
    pub fn find_stop(&self) -> Result<(), String> {
        Ok(())
    }

    /// Give the page the keyboard. Unreachable.
    pub fn focus_page(&self) -> Result<(), String> {
        Err(no_engine("the web preview"))
    }

    /// The pointer, over a page that is not there.
    pub fn send_mouse(
        &self,
        event: WebMouseEvent,
        point: (i32, i32),
        buttons_down: u32,
    ) -> Result<(), String> {
        let _ = (event, point, buttons_down);
        Ok(())
    }

    /// A picture of the page, for the focus card. Unreachable.
    pub fn capture_preview(&self) -> Result<(), String> {
        Err(no_engine("the web preview"))
    }

    /// The page's icon. Unreachable.
    pub fn get_favicon(&self) -> Result<(), String> {
        Err(no_engine("the web preview"))
    }

    /// Close a controller nobody came for. There is none.
    pub fn close_pending_controller(&mut self) {}

    /// Whether a creation call nobody will adopt has still not answered. Never here: this host
    /// makes no controller that can outlive the one who asked for it.
    #[must_use]
    pub fn has_orphans(&self) -> bool {
        false
    }

    /// Whether the engine has said something nobody has read yet.
    #[must_use]
    pub fn has_events(&self) -> bool {
        false
    }

    /// Close the host. Nothing to close, and it must not refuse: this runs on
    /// the way out of a seat and on the way out of the process.
    pub fn close(&mut self) {}

    /// The page's own process. There is none, and the number a host with no page
    /// answers on the other platforms is this one.
    #[must_use]
    pub fn browser_process_id(&self) -> u32 {
        0
    }

    /// Who owns this page's device scale. Nobody: there is no page.
    #[must_use]
    pub fn dpi_ownership(&self) -> Option<WebDpiOwnership> {
        None
    }
}

/// Drop the process-wide environment. There is none to drop.
///
/// `WKWebsiteDataStore`'s lifecycle is a different question with a different
/// answer, and M4-2 answered it in the macOS arm — see §4.5 of the plan and
/// `docs/DESIGN.md` §13.29.
pub fn forget_web_environment() {}

/// **Which environment the process has** (0.4.5 ticket 60). There is no process-wide environment
/// on this platform, so there is nothing to move: always `0`.
#[must_use]
pub fn web_environment_epoch() -> u64 {
    0
}

/// **The spare web controller's parent** (0.4.5 ticket 60), which this platform never makes: its
/// engine is made on the spot, so there is nothing to warm and no parent to hold it. Uninhabited —
/// [`spare_parent`] answers `None` — and declared so that `bt-app` names one type everywhere.
pub struct SpareParent(std::convert::Infallible);

impl SpareParent {
    /// The window. Unreachable: there is no parent.
    #[must_use]
    pub fn window(&self) -> NativeWindow {
        match self.0 {}
    }

    /// The parent's composition tree. Unreachable: there is no parent.
    #[must_use]
    pub fn compositor(&self) -> &Compositor {
        match self.0 {}
    }

    /// Whether the window exists. Unreachable: there is no parent.
    #[must_use]
    pub fn is_window(&self) -> bool {
        match self.0 {}
    }
}

/// **Make the spare's parent**: nothing to make on this platform.
pub fn spare_parent() -> Result<Option<SpareParent>, String> {
    Ok(None)
}

/// **The warm-up's door** (ticket 54): nothing to warm on a platform with no
/// engine. The answer is dropped unheard, and the page's own ask is still the
/// one that says there is no web preview here.
pub fn warm_web_environment(
    folder: &Path,
    answered: EnvironmentAnswer,
) -> Result<WebWarmUp, String> {
    let _ = (folder, answered);
    Ok(WebWarmUp::NothingToWarm)
}

/// **Which engine is installed.**
///
/// On Windows this is *is the Evergreen runtime here at all*, and the honest
/// answer on a platform whose web engine ships with the operating system would
/// be `Ok`. It is an `Err` today because the question a caller asks with it is
/// not really "is WebKit present" but "can this build show a page", and this
/// build cannot. The macOS arm answers `Ok` and has the pages to go with it.
pub fn webview2_runtime_version() -> Result<String, String> {
    Err(no_engine("the web preview"))
}
