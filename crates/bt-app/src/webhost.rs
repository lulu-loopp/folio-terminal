//! The web preview's engine, driven: **slice ①, the platform host and its
//! input, and nothing else** (`docs/plans/web-preview/plan.md` §5).
//!
//! # What is here and what is deliberately not
//!
//! Here: the process's single WebView2 environment, one controller per web seat,
//! the recovery state machine §4 specifies, the visual's place in the window's
//! composition tree, and the input contract — which mouse events go in, which
//! chords come back out, and what `Tab` does at the edge of a page.
//!
//! Not here, and each is another slice's whole subject: the address field, the
//! three buttons, find and zoom and developer tools (④); the URL policy and the
//! sanctioned `file:` door (②); the preview buffer, the switcher, Recent and the
//! session schema (③); PDF and the file watcher (⑤); thumbnails (⑥). This slice
//! reaches a page through `BT_WEB_DEV` and through nothing a person can press.
//!
//! # Why the state machine is a separate object from the engine
//!
//! WebView2's creation callbacks **cannot be cancelled**. A pane that was
//! closed, or whose browser process died and was rebuilt, will still receive the
//! environment and controller callbacks of the attempt it abandoned. Each one
//! arrives carrying the generation it was launched under, and a generation that
//! is no longer current is dropped on the floor — including its controller,
//! which is *closed* rather than adopted, because adopting it would leave a live
//! browser process nobody points at.
//!
//! [`WebMachine`] is that rule and the eight or so others §4 states, written as
//! a type with no engine in it, so the whole recovery model can be shot at by
//! `cargo test` on a machine with no runtime installed. It arrives here from the
//! W0′ probe (`spikes/webview2-w0/src/machine.rs`) with its contract tests
//! intact, plus the two amendments the second round of evidence forced:
//!
//! - **the user data folder's wait has a deadline, and on the graceful path the
//!   deadline is the only backstop** — eight measured shutdowns, one of which
//!   said nothing at all in ten seconds, and none of which raised
//!   `ProcessFailed` (`w0p-evidence.md` §4.2);
//! - **`ProcessFailed` under a closing pane is not a rebuild** — the same event
//!   means "your browser died, build another" under a live pane and "the folder
//!   is yours now" under a closing one, and the model that rebuilt on both
//!   brought back panes the person had shut.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use bt_persist::SearchEngineV1;
use bt_platform::{WebChord, WebEvent, WebHost, WebNavigationVerdict};
use winit::keyboard::{ModifiersState, NamedKey};

use crate::shortcuts::{Action, ChordKey, Focus, Shortcuts};

// ── The recovery state machine (plan §4, gate 10) ──────────────────────────

/// Where one web seat is in its life.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WebState {
    Uninitialized,
    EnvironmentPending,
    ControllerPending,
    /// Events and policies are all installed; navigation is permitted.
    Ready,
    Closing,
    Failed,
}

/// What the caller must do with a callback that arrived.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WebEffect {
    /// Nothing: a stale callback, dropped.
    Ignore,
    /// Create the controller for this generation.
    CreateController,
    /// Install every event and policy for this generation, then report back.
    InstallEvents,
    /// Navigate. Only ever produced after `InstallEvents` was acknowledged.
    Navigate(String),
    /// A controller from an abandoned generation arrived: close it, do not keep
    /// it. Leaking it would leave a live browser process nobody points at.
    CloseOrphanController,
    /// The browser process is gone; build the whole thing again.
    RebuildFromScratch,
    /// Only the renderer died; the controller is still good.
    Reload,
    /// Wait for the browser to go before the user data folder is anybody's.
    AwaitBrowserExitBeforeCleanup,
    /// **The browser has let go of the user data folder.**
    ///
    /// The probe called this `CleanupUserDataFolder` because its folder was a
    /// temporary directory it deleted. The product's folder is a *profile* —
    /// `%LOCALAPPDATA%\Folio\WebView2`, holding the cookie jar and the cache
    /// (`plan.md` §0) — and deleting it on the last page closing would be
    /// deleting the thing it exists to keep. So what this effect names is the
    /// **moment**, not the removal: the folder is unheld, the teardown is
    /// finished, and a new environment may be created over it (which is the one
    /// step [`WebEffect::RebuildForNewVersion`] cannot take before this).
    ///
    /// Three doors lead here and the plan named only one of them.
    /// `BrowserProcessExited` is the door it named;
    /// `ProcessFailed(BROWSER_PROCESS_EXITED)` is the second, and the wait
    /// running out is the third — which on the graceful path is the *only* one
    /// that opens (`w0p-evidence.md` §4.2).
    ReleaseUserDataFolder,
    /// Evergreen installed a new build under a running process. Nothing about
    /// the window is wrong — only the browser binary is — so the seat comes
    /// back on the same `HWND` and the same visual, at the last good URL.
    ///
    /// What the caller owes this effect, in order: close every controller over
    /// the old environment, wait for the browser to go by the same three doors
    /// as `close`, **release the cached environment**, then create a new one. A
    /// new environment made while the old browser still holds the folder does
    /// not fail loudly — it simply never calls back.
    RebuildForNewVersion,
}

/// Where a closing seat is in its wait for the browser to go.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Cleanup {
    /// Not closing.
    Idle,
    /// `close` was called and the folder is still the browser's.
    Awaiting,
    /// Somebody already got the folder; a second door opening changes nothing.
    Done,
}

/// The recovery model of `plan.md` §4, with the two amendments W0′ forced.
pub(crate) struct WebMachine {
    state: WebState,
    generation: u64,
    /// Last write wins. Storing a URL is not navigating to it.
    desired_url: Option<String>,
    /// The URL a session file may record. Only a *successful* top-level
    /// navigation writes here.
    recoverable_url: Option<String>,
    events_installed: bool,
    cleanup: Cleanup,
}

impl Default for WebMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl WebMachine {
    pub(crate) fn new() -> Self {
        Self {
            state: WebState::Uninitialized,
            generation: 0,
            desired_url: None,
            recoverable_url: None,
            events_installed: false,
            cleanup: Cleanup::Idle,
        }
    }

    pub(crate) fn state(&self) -> WebState {
        self.state
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// The URL a session file could record — the last one that actually
    /// loaded.
    ///
    /// Written here because the recovery model is the thing that knows it: a
    /// machine that only tracked it while somebody was listening would be a
    /// different machine, and a crash three seconds before slice ③ arrives is
    /// still a crash that has to come back to the right page. Read today by
    /// `a_failed_page_does_not_overwrite_a_recoverable_url` and by the two
    /// crash tests beside it.
    ///
    /// **Slice ③ is the consumer this was written for.** It is read by
    /// [`WebSeat::identity`], and through it by the preview pool, the switcher,
    /// `session.json` and the Recent vault — all four of them off this one field,
    /// because "the URL a session file may record" and "what the switcher calls
    /// this seat" are the same sentence (`plan.md` §3 与 §4).
    pub(crate) fn recoverable_url(&self) -> Option<&str> {
        self.recoverable_url.as_deref()
    }

    /// Somebody asked for a URL. If the engine is not up yet this only records
    /// the wish; if it is, it navigates.
    pub(crate) fn request(&mut self, url: &str) -> WebEffect {
        self.desired_url = Some(url.to_owned());
        match self.state {
            WebState::Ready if self.events_installed => WebEffect::Navigate(url.to_owned()),
            WebState::Uninitialized | WebState::Failed => {
                self.generation += 1;
                self.state = WebState::EnvironmentPending;
                self.events_installed = false;
                WebEffect::Ignore
            }
            _ => WebEffect::Ignore,
        }
    }

    /// The environment callback for `generation` came back.
    pub(crate) fn on_environment(&mut self, generation: u64, ok: bool) -> WebEffect {
        if generation != self.generation || self.state != WebState::EnvironmentPending {
            return WebEffect::Ignore;
        }
        if !ok {
            self.state = WebState::Failed;
            return WebEffect::Ignore;
        }
        self.state = WebState::ControllerPending;
        WebEffect::CreateController
    }

    /// The controller callback for `generation` came back.
    pub(crate) fn on_controller(&mut self, generation: u64, ok: bool) -> WebEffect {
        if generation != self.generation || self.state != WebState::ControllerPending {
            // The controller is real and running even though nobody wants it.
            return if ok {
                WebEffect::CloseOrphanController
            } else {
                WebEffect::Ignore
            };
        }
        if !ok {
            self.state = WebState::Failed;
            return WebEffect::Ignore;
        }
        WebEffect::InstallEvents
    }

    /// Every handler and policy for `generation` is attached.
    pub(crate) fn on_events_installed(&mut self, generation: u64) -> WebEffect {
        if generation != self.generation || self.state != WebState::ControllerPending {
            return WebEffect::Ignore;
        }
        self.state = WebState::Ready;
        self.events_installed = true;
        match self.desired_url.clone() {
            Some(url) => WebEffect::Navigate(url),
            None => WebEffect::Ignore,
        }
    }

    /// A top-level navigation finished. Only success moves the recoverable URL —
    /// an error page, a cancelled navigation and `about:blank` all leave the
    /// last good URL where it was, so a crash during a failed load still
    /// restores what the person was looking at.
    pub(crate) fn on_navigation_completed(
        &mut self,
        generation: u64,
        url: &str,
        success: bool,
    ) -> WebEffect {
        if generation != self.generation || self.state != WebState::Ready {
            return WebEffect::Ignore;
        }
        if success && !is_blank(url) {
            self.recoverable_url = Some(url.to_owned());
        }
        WebEffect::Ignore
    }

    /// The browser process died. Everything is invalid; a new generation starts.
    ///
    /// **Unless the seat was already closing.** The same event carries both
    /// meanings — a crash under a live pane, and the exit of a browser that was
    /// asked to go — and the state is the only thing that tells them apart. A
    /// model that rebuilt on both would resurrect a pane the person shut.
    pub(crate) fn on_browser_process_failed(&mut self) -> WebEffect {
        if self.state == WebState::Closing {
            return self.finish_cleanup();
        }
        self.generation += 1;
        self.state = WebState::EnvironmentPending;
        self.events_installed = false;
        // Whatever was last good is what comes back up.
        self.desired_url = self
            .recoverable_url
            .clone()
            .or_else(|| self.desired_url.clone());
        WebEffect::RebuildFromScratch
    }

    /// **Read this page again** (W2 slice 5, `preview_watch`).
    ///
    /// A door and not a second `request`, because they are two different
    /// sentences: `request` says *go here*, which starts a navigation and would
    /// truncate the page's own history; this says *the bytes behind where you
    /// already are have changed*, which is what a static file saved on disk is
    /// and is exactly what the engine's own `Reload` means. The plan's words for
    /// it are "网页座位刷新 = 一次正常 `Reload`(不是重新导航)".
    ///
    /// Nothing at all unless the engine is up and the events are installed - a
    /// seat still building has a `desired_url` waiting for it and needs no help
    /// remembering it, and a seat that is closing is not a seat to reload.
    pub(crate) fn reload(&mut self) -> WebEffect {
        if self.state == WebState::Ready && self.events_installed {
            WebEffect::Reload
        } else {
            WebEffect::Ignore
        }
    }

    /// Only the render process died. The controller survives.
    pub(crate) fn on_render_process_failed(&mut self) -> WebEffect {
        if self.state == WebState::Ready {
            WebEffect::Reload
        } else {
            WebEffect::Ignore
        }
    }

    /// The seat is going away.
    pub(crate) fn close(&mut self) -> WebEffect {
        self.generation += 1;
        self.state = WebState::Closing;
        self.events_installed = false;
        self.cleanup = Cleanup::Awaiting;
        WebEffect::AwaitBrowserExitBeforeCleanup
    }

    /// `BrowserProcessExited` arrived — the door the plan named.
    pub(crate) fn on_browser_process_exited(&mut self) -> WebEffect {
        self.finish_cleanup()
    }

    /// The wait ran out. The plan's literal wording has no such input, which is
    /// why it waits for ever on the paths where the event never comes.
    pub(crate) fn on_cleanup_deadline(&mut self) -> WebEffect {
        self.finish_cleanup()
    }

    /// One folder, one release, whichever door opened first.
    fn finish_cleanup(&mut self) -> WebEffect {
        match self.cleanup {
            Cleanup::Awaiting => {
                self.cleanup = Cleanup::Done;
                WebEffect::ReleaseUserDataFolder
            }
            _ => WebEffect::Ignore,
        }
    }

    /// **The controller was closed to get out of a half-finished rehost**
    /// (F1a's lossy branch).
    ///
    /// The same shape as a version change, and for the same reason: the browser
    /// process is *alive* — only the controller went — so a new environment
    /// cannot be built over the folder until it has let go. What is different is
    /// only what the caller does first, which is to move the seat's address, so
    /// that the page this returns comes back in the window the person moved it
    /// to and not the one it left.
    #[allow(dead_code, reason = "F1b's transfer transaction is the caller")]
    pub(crate) fn on_rehost_lost(&mut self) -> WebEffect {
        self.on_new_browser_version_available()
    }

    /// A newer runtime is installed and the running one is now the old one.
    pub(crate) fn on_new_browser_version_available(&mut self) -> WebEffect {
        if self.state == WebState::Closing {
            // The seat is going away; the version it goes away on is nobody's
            // business.
            return WebEffect::Ignore;
        }
        self.generation += 1;
        self.state = WebState::EnvironmentPending;
        self.events_installed = false;
        self.desired_url = self
            .recoverable_url
            .clone()
            .or_else(|| self.desired_url.clone());
        WebEffect::RebuildForNewVersion
    }
}

fn is_blank(url: &str) -> bool {
    url.eq_ignore_ascii_case(BLANK_PAGE) || url.is_empty()
}

// Slice ② landed on 2026-08-22, so the placeholder it was written against is
// gone and the rule below is the one the address bar asks. The shape did not
// move: `WebSeat` calls the same function with the same arguments it always
// did. What did move is why the development target is allowed — the stub
// admitted it by name, and §3’s loopback rule admits it now (DESIGN §7.8 ⑦).
use crate::webnav::{
    BLANK_PAGE, Decision, Mint, Origin, Refusal, address_bar, check, navigation_starting,
};

// ── The development entry ──────────────────────────────────────────────────

/// `BT_WEB_DEV=<url>` — **the only way to a page in this build**.
///
/// Registered in `docs/HANDOFF-2026-08-21.md` §2 beside the other diagnostic
/// switches. Read once: an environment variable does not change under a running
/// process, and the navigation gate is called from inside a COM callback where
/// a syscall per keystroke of a page's own redirects would be a cost for
/// nothing.
pub(crate) fn development_target() -> Option<&'static str> {
    use std::sync::OnceLock;
    static TARGET: OnceLock<Option<String>> = OnceLock::new();
    TARGET
        .get_or_init(|| {
            std::env::var("BT_WEB_DEV")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .as_deref()
}

// ── The user data folder ───────────────────────────────────────────────────

/// The profile every web preview in this process shares.
///
/// **`%LOCALAPPDATA%` and never `%APPDATA%`** (`plan.md` §0): the folder holds a
/// disk cache, a cookie jar and a crash-dump directory, and a roaming profile
/// would carry all three between machines. It is a profile and it is not
/// deleted — see [`WebEffect::ReleaseUserDataFolder`].
pub(crate) fn user_data_folder_in(local_appdata: &Path) -> PathBuf {
    local_appdata.join("Folio").join("WebView2")
}

/// The same, on this machine.
///
/// Asked of the environment rather than assembled from a user name, on this
/// repository's standing rule about machine facts: ask the thing itself. A test
/// or a taking of evidence isolates it by isolating `LOCALAPPDATA`, which is
/// what makes "an isolated user data folder" one variable rather than two.
pub(crate) fn user_data_folder() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(|base| user_data_folder_in(Path::new(&base)))
}

// ── The keyboard contract ──────────────────────────────────────────────────

/// One chord the window takes back from a focused page, and what it does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ClaimedChord {
    pub(crate) chord: WebChord,
    pub(crate) action: Action,
}

/// Every chord the effective shortcut table carries that is in force with the
/// window focused like this, translated into the only vocabulary
/// `AcceleratorKeyPressed` speaks.
///
/// # This is a derivation, not a transcription
///
/// The W0′ probe carried a hand-copied table because it lives outside the
/// workspace (`spikes/webview2-w0/src/bindings.rs`). Inside it, copying would be
/// the bug: `Shortcuts` is `BINDINGS` **with the user's `keybindings.json` laid
/// over it**, and a page that kept giving the window back the factory chords
/// after somebody rebound one would be a shortcut table with two answers.
///
/// # What cannot be claimed, and why nobody will notice until it is too late
///
/// A **bare printable key never enters `AcceleratorKeyPressed` at all**. The
/// probe pressed `K` with a page focused: the page received it and the callback
/// did not fire once (`w0p-evidence.md` §2.4). So a row bound to a bare letter
/// would work everywhere in this window except over a web seat, silently. The
/// table has no such row today and `no_shipped_chord_is_a_bare_printable_key`
/// is what says so tomorrow.
pub(crate) fn claimable_chords(shortcuts: &Shortcuts, focus: Focus) -> Vec<ClaimedChord> {
    shortcuts
        .rows()
        .iter()
        .filter(|row| row.scope.holds(focus))
        .filter_map(|row| {
            let chord = row.chord.as_ref()?;
            let virtual_key = chord_virtual_key(&chord.key)?;
            Some(ClaimedChord {
                chord: WebChord {
                    virtual_key,
                    ctrl: chord.modifiers.contains(ModifiersState::CONTROL),
                    shift: chord.modifiers.contains(ModifiersState::SHIFT),
                    alt: chord.modifiers.contains(ModifiersState::ALT),
                },
                action: row.action,
            })
        })
        .collect()
}

/// The claim on that chord, if this list has one.
pub(crate) fn claim_for(claims: &[ClaimedChord], chord: WebChord) -> Option<&ClaimedChord> {
    claims.iter().find(|claim| claim.chord == chord)
}

/// Whether this list claims that key with those modifiers.
///
/// The window's own code path asks [`claim_for`], because it wants the verb and
/// not the verdict. This spelling exists for the reconciliation tests, whose
/// whole subject is which keys a page keeps — a question that has a yes and a no
/// and no verb at all.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn claims_chord(
    claims: &[ClaimedChord],
    virtual_key: u16,
    ctrl: bool,
    shift: bool,
    alt: bool,
) -> bool {
    claim_for(
        claims,
        WebChord {
            virtual_key,
            ctrl,
            shift,
            alt,
        },
    )
    .is_some()
}

/// The Win32 virtual key a chord's key half is pressed on **this layout**.
fn chord_virtual_key(key: &ChordKey) -> Option<u16> {
    match key {
        ChordKey::Character(text) => {
            // Asked of the layout rather than of a table: `Alt+Shift+-` is
            // `VK_OEM_MINUS` on a US keyboard and something else on a German
            // one, and the person pressing it is the one whose layout counts.
            bt_platform::virtual_key_for_character(text.chars().next()?)
        }
        ChordKey::Named(named) => named_key_virtual_key(*named),
    }
}

/// The Win32 virtual key behind a named key.
///
/// Layout-independent by construction — a named key *is* a virtual key wearing
/// winit's name for it — so this is a table and not a syscall.
pub(crate) fn named_key_virtual_key(key: NamedKey) -> Option<u16> {
    Some(match key {
        NamedKey::Tab => VK_TAB,
        NamedKey::Escape => VK_ESCAPE,
        NamedKey::Enter => 0x0d,
        NamedKey::Space => 0x20,
        NamedKey::Backspace => 0x08,
        NamedKey::Delete => 0x2e,
        NamedKey::Insert => 0x2d,
        NamedKey::Home => 0x24,
        NamedKey::End => 0x23,
        NamedKey::PageUp => 0x21,
        NamedKey::PageDown => 0x22,
        NamedKey::ArrowLeft => VK_LEFT,
        NamedKey::ArrowUp => 0x26,
        NamedKey::ArrowRight => VK_RIGHT,
        NamedKey::ArrowDown => 0x28,
        NamedKey::F1 => 0x70,
        NamedKey::F2 => 0x71,
        NamedKey::F3 => 0x72,
        NamedKey::F4 => 0x73,
        NamedKey::F5 => VK_F5,
        NamedKey::F6 => 0x75,
        NamedKey::F7 => 0x76,
        NamedKey::F8 => 0x77,
        NamedKey::F9 => 0x78,
        NamedKey::F10 => 0x79,
        NamedKey::F11 => 0x7a,
        NamedKey::F12 => VK_F12,
        _ => return None,
    })
}

pub(crate) const VK_TAB: u16 = 0x09;
pub(crate) const VK_ESCAPE: u16 = 0x1b;
pub(crate) const VK_LEFT: u16 = 0x25;
pub(crate) const VK_RIGHT: u16 = 0x27;
pub(crate) const VK_F5: u16 = 0x74;
pub(crate) const VK_F12: u16 = 0x7b;

// ── Where the page is, and whether it is anywhere ──────────────────────────

/// The rectangle a page occupies, in physical pixels of the window's client
/// area.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WebBounds {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// Whether the page is on the glass this frame, and where.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WebPresence {
    Hidden,
    Shown(WebBounds),
}

/// The one place that decides whether a page is visible.
///
/// `body` is the seat's preview body — `None` when the seat has no rectangle at
/// all, which is what a tab switched away, a seat the fit ladder dropped and a
/// pane parked off stage by focus mode all look like. `obstructed` is a modal:
/// the scrim is painted over the whole window **by wgpu**, and the page is
/// underneath wgpu, so a hole punched through a scrim would be a page read
/// clearly through a dimmed window.
///
/// Hiding is not decoration. A hidden WebView stops its timers and its
/// `requestAnimationFrame` entirely — 1 811 ms of CPU and 718 frames over six
/// seconds visible, **0 and 0** hidden (`w0p-evidence.md` §1 gate 8) — which is
/// the whole of how a page on a tab nobody is looking at costs nothing.
pub(crate) fn web_presence(body: Option<[f32; 4]>, obstructed: bool) -> WebPresence {
    let Some([left, top, right, bottom]) = body else {
        return WebPresence::Hidden;
    };
    if obstructed {
        return WebPresence::Hidden;
    }
    // Outwards on every edge: a page inset by the half pixel a solver's
    // rounding left behind would show a hairline of window along two of its
    // sides, and the hole `bt-render` punches is rounded the same way.
    let x = left.floor() as i32;
    let y = top.floor() as i32;
    let width = (right.ceil() as i32 - x).max(0) as u32;
    let height = (bottom.ceil() as i32 - y).max(0) as u32;
    if width == 0 || height == 0 {
        return WebPresence::Hidden;
    }
    WebPresence::Shown(WebBounds {
        x,
        y,
        width,
        height,
    })
}

impl WebPresence {
    /// The rectangle, when there is one on the glass.
    ///
    /// Read by the one caller that asks [`web_presence`] twice — once with
    /// nothing standing over the seat, to learn its *size*, and once honestly, to
    /// learn its *presence*. See [`WebSeat::apply_presence`] for why those are
    /// two questions.
    pub(crate) fn bounds(self) -> Option<WebBounds> {
        match self {
            Self::Shown(bounds) => Some(bounds),
            Self::Hidden => None,
        }
    }
}

impl WebBounds {
    /// The rectangle `bt_render::WindowRenderer::set_web_holes` is given, in the
    /// `[left, top, right, bottom]` every chrome rectangle in this program is
    /// spelled in.
    pub(crate) fn as_rect(self) -> [f32; 4] {
        [
            self.x as f32,
            self.y as f32,
            (self.x + self.width as i32) as f32,
            (self.y + self.height as i32) as f32,
        ]
    }
}

// ── The five failure cards, and where each one's reason comes from ─────────

/// What a web seat is showing instead of a page — or, for the one that stands
/// **over** a page, as well as it (§7.7 ④).
///
/// # One drawing, five rows, two placements
///
/// Every variant answers the same four questions the card asks: a sentence, at
/// most one line of fact, exactly one verb, and whether it takes the seat or
/// stands on a scrim over it. There is no sixth question and no second verb —
/// 「一排按钮是程序把自己的判断交还给读者」.
///
/// # Nothing here decides anything a machine already said
///
/// The reasons are not invented at the card. `RuntimeMissing` is the loader's
/// own answer (gate 7 proved the registry lies and the loader does not),
/// `DidNotLoad` is `NavigationCompleted`'s `WebErrorStatus`, `RenderProcessGone`
/// is `ProcessFailed` with the renderer's kind, `Blocked` carries the
/// [`Refusal`] the navigation gate produced, and `DownloadRefused` is what is
/// left when `webnav::address_bar` will not take a download's own URL. The card
/// spells them; it does not judge them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WebFault {
    /// There is no WebView2 runtime on this machine.
    RuntimeMissing {
        /// The call that failed and the code it failed with.
        detail: String,
    },
    /// A navigation never committed a document.
    DidNotLoad {
        /// The host that was asked — the half of a URL a connection failure is
        /// about.
        host: String,
        /// `WebErrorStatus` in the SDK's own spelling.
        detail: String,
    },
    /// The renderer under this page exited.
    RenderProcessGone,
    /// A URL this seat was **handed** does not open in a preview.
    ///
    /// Handed, not clicked: a link inside a page is inert and says so in the
    /// foot (§7.1.5g ⑤), and this card is for the case where there is no page to
    /// say it over — a stale pin, a restored session, the command palette.
    Blocked { url: String, refusal: Refusal },
    /// A download was cancelled and could not be handed to the machine's
    /// browser either.
    DownloadRefused { file_name: String },
}

/// The one thing a failure card's button does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WebFaultVerb {
    /// Open Microsoft's download page in the machine's browser.
    DownloadTheRuntime,
    /// Try the navigation again.
    Reload,
    /// Put the refused address on the clipboard.
    CopyAddress(String),
    /// Hand the **page** over, since the file could not be.
    OpenPageInBrowser,
}

/// Where Microsoft publishes the runtime, and the only address this product
/// hands out that is not one somebody asked for.
///
/// A constant rather than a search: the card's verb promises a specific page,
/// and a query typed into whatever engine is configured is not that page.
pub(crate) const RUNTIME_DOWNLOAD_PAGE: &str =
    "https://developer.microsoft.com/microsoft-edge/webview2/";

impl WebFault {
    /// The one sentence.
    pub(crate) fn say(&self) -> String {
        match self {
            Self::RuntimeMissing { .. } => crate::i18n::Text::WebFailRuntimeSay.text().to_owned(),
            Self::DidNotLoad { host, .. } => crate::i18n::web_fail_did_not_respond(host),
            Self::RenderProcessGone => crate::i18n::Text::WebFailCrashSay.text().to_owned(),
            // The scheme comes from `webnav::scheme_of` and from nowhere else
            // (§7.8 ③: 「不另起第二种解析」). An address that carries none — a
            // bare host, an empty string — gets the sentence that names no
            // scheme rather than a sentence with a hole in it.
            Self::Blocked { url, .. } => match crate::webnav::scheme_of(url) {
                Some(scheme) => crate::i18n::web_fail_blocked_scheme(&scheme),
                None => crate::i18n::Text::WebFailBlockedSay.text().to_owned(),
            },
            Self::DownloadRefused { .. } => crate::i18n::Text::WebFailDownloadSay.text().to_owned(),
        }
    }

    /// The one line of fact under it, or nothing when there is no fact worth
    /// quoting into a bug report.
    pub(crate) fn detail(&self) -> Option<&str> {
        match self {
            Self::RuntimeMissing { detail } | Self::DidNotLoad { detail, .. } => {
                (!detail.is_empty()).then_some(detail.as_str())
            }
            // The crash has none, and that is the mock-up's own answer: there is
            // no code a renderer's exit hands over that a reader could act on.
            Self::RenderProcessGone => None,
            Self::Blocked { url, .. } => Some(url.as_str()),
            Self::DownloadRefused { file_name } => {
                (!file_name.is_empty()).then_some(file_name.as_str())
            }
        }
    }

    /// The word on the button.
    pub(crate) fn verb_text(&self) -> crate::i18n::Text {
        match self {
            Self::RuntimeMissing { .. } => crate::i18n::Text::WebFailRuntimeVerb,
            Self::DidNotLoad { .. } | Self::RenderProcessGone => {
                crate::i18n::Text::PreviewWebReload
            }
            Self::Blocked { .. } => crate::i18n::Text::WebFailBlockedVerb,
            Self::DownloadRefused { .. } => crate::i18n::Text::WebFailDownloadVerb,
        }
    }

    /// What pressing it does.
    pub(crate) fn verb(&self) -> WebFaultVerb {
        match self {
            Self::RuntimeMissing { .. } => WebFaultVerb::DownloadTheRuntime,
            Self::DidNotLoad { .. } | Self::RenderProcessGone => WebFaultVerb::Reload,
            Self::Blocked { url, .. } => WebFaultVerb::CopyAddress(url.clone()),
            Self::DownloadRefused { .. } => WebFaultVerb::OpenPageInBrowser,
        }
    }

    /// The address a refused navigation was aimed at, when that is what this
    /// card is about.
    ///
    /// The head's name cell reads it when there is nothing else to put there: a
    /// seat whose one navigation was refused has no document title and no
    /// committed URL, and a blank cell over a card that names the address in
    /// full would be the head saying less than the body under it.
    pub(crate) fn refused_address(&self) -> Option<String> {
        match self {
            Self::Blocked { url, .. } => Some(url.clone()),
            _ => None,
        }
    }

    /// Whether this card stands on a scrim **over a page that is still there**,
    /// rather than being the whole of what the seat holds.
    ///
    /// One variant answers `true` and the ruling says why: what was cancelled is
    /// the download, and the page that asked for it is still standing and still
    /// scrolled where the reader left it — blanking it would throw away more
    /// than the failure did. The other four have nothing behind them to keep;
    /// take one of those away and what is left is the black hole a hidden
    /// WebView leaves (`w0-evidence.md` §2⑨), which is why they have no Escape.
    pub(crate) fn stands_over_the_page(&self) -> bool {
        matches!(self, Self::DownloadRefused { .. })
    }
}

/// `COREWEBVIEW2_WEB_ERROR_STATUS`, in the SDK's own spelling.
///
/// The names and not an invented sentence: the fact line under a card exists so
/// that it can be copied into a bug report, and the string somebody searching
/// for that failure will find is the one the API uses. The mock-up writes
/// `ERR_CONNECTION_REFUSED` in this slot, which is **Chromium's** vocabulary and
/// not this API's — a demo constant, not a ruling, and recorded as a mock-up
/// debt rather than transcribed into a lie.
fn web_error_status_name(status: i32) -> &'static str {
    match status {
        1 => "CertificateCommonNameIsIncorrect",
        2 => "CertificateExpired",
        3 => "ClientCertificateContainsErrors",
        4 => "CertificateRevoked",
        5 => "CertificateIsInvalid",
        6 => "ServerUnreachable",
        7 => "Timeout",
        8 => "ErrorHttpInvalidServerResponse",
        9 => "ConnectionAborted",
        10 => "ConnectionReset",
        11 => "Disconnected",
        12 => "CannotConnect",
        13 => "HostNameNotResolved",
        14 => "OperationCanceled",
        15 => "RedirectFailed",
        16 => "UnexpectedError",
        17 => "ValidAuthenticationCredentialsRequired",
        18 => "ValidProxyAuthenticationRequired",
        _ => "Unknown",
    }
}

/// What a finished navigation leaves on the seat: a card, or nothing.
///
/// A pure function so that the one distinction it makes can be shot at without
/// an engine — and the distinction is the whole of it. A **refused** navigation
/// completes with `IsSuccess == false` exactly as a connection failure does, and
/// the two mean opposite things: one is the policy working and already has a
/// card of its own, the other is the network. Without this, every `· blocked`
/// in the foot would also raise a 「did not respond」 over the seat.
pub(crate) fn load_fault(uri: &str, success: bool, status: i32) -> Option<WebFault> {
    if success || status == WEB_ERROR_OPERATION_CANCELED {
        return None;
    }
    Some(WebFault::DidNotLoad {
        host: crate::webnav::host_of(uri).unwrap_or_default(),
        detail: format!("WebErrorStatus · {}", web_error_status_name(status)),
    })
}

/// What to do about a download the engine has already been told to cancel.
///
/// 方案 §0: 「取消并外开可重放的 GET URL,不可重放者提示无法下载」. **What
/// 「可重放」 means is not guessed at** — it is the address bar's own answer,
/// because a `blob:` or a `data:` URL names memory inside a page rather than a
/// request anybody else can make, and those are exactly the ones that door
/// already refuses. One rule, one door, and no second opinion about what a plain
/// link can carry.
pub(crate) fn download_answer(uri: &str, file_name: &str) -> Result<String, WebFault> {
    match address_bar(uri) {
        Decision::Navigate(target) => Ok(target),
        Decision::Search(_) | Decision::Refuse(_) => Err(WebFault::DownloadRefused {
            file_name: file_name
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or_default()
                .to_owned(),
        }),
    }
}

/// The one status that is **not** a page that did not load.
///
/// A refused navigation completes with `IsSuccess == false` exactly as a
/// connection failure does, and the two mean opposite things: one is the policy
/// working and already has a card of its own, the other is the network. Without
/// this, every `· blocked` in the foot would also raise a 「did not respond」
/// over the seat.
const WEB_ERROR_OPERATION_CANCELED: i32 = 14;

// ── Where a non-address goes ───────────────────────────────────────────────

/// The three engines, as this build spells them.
///
/// Constants here and a *name* in `settings.json` (`bt_persist::SearchEngineV1`)
/// — see that type for why a template in a file is the one shape §3's URL policy
/// exists to refuse.
const fn search_prefix(engine: SearchEngineV1) -> &'static str {
    match engine {
        SearchEngineV1::DuckDuckGo => "https://duckduckgo.com/?q=",
        SearchEngineV1::Bing => "https://www.bing.com/search?q=",
        SearchEngineV1::Google => "https://www.google.com/search?q=",
    }
}

/// Where the address field sends something that is not an address (§7.7 ②,
/// 方案 §0's five extras).
///
/// **Form encoding, because a query string is a form.** Everything outside the
/// unreserved set is percent-encoded and a space becomes `+`, which is what
/// every search box on the web has sent since forms existed — and it is what
/// keeps `c++ std::string` and `a&b=c` from arriving as three parameters and a
/// syntax error. The `+` a person typed is `%2B` for the same reason.
///
/// The composed URL is **not** trusted for being composed here: `WebSeat::go_to`
/// puts it through `webnav::address_bar` exactly as it puts a typed one, which
/// is the whole of 「钉不是授权」 said about a string this build built itself.
pub(crate) fn search_url(engine: SearchEngineV1, query: &str) -> String {
    let mut url = String::from(search_prefix(engine));
    for byte in query.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                url.push(*byte as char);
            }
            b' ' => url.push('+'),
            other => {
                use std::fmt::Write as _;
                let _ = write!(url, "%{other:02X}");
            }
        }
    }
    url
}

// ── Zoom ───────────────────────────────────────────────────────────────────

/// The zoom ladder, as a browser has them: the rungs `Ctrl`+wheel steps
/// through, with `1.0` a rung so that the way back to unzoomed is a detent and
/// not an aim.
///
/// A ladder and not a multiplier, because a multiplier has no bottom and no top
/// and never lands on a round number — and because the two ends are where a
/// reader finds out that the gesture has stopped rather than gone unnoticed.
const ZOOM_LADDER: [f64; 15] = [
    0.25, 0.33, 0.50, 0.67, 0.75, 0.80, 0.90, 1.00, 1.10, 1.25, 1.50, 1.75, 2.00, 2.50, 3.00,
];

/// The rung above or below `factor`, or `factor` itself at the end of the
/// ladder.
///
/// Written as "which rung is this nearest, then step from there" so that a zoom
/// the page arrived at by some other route — a restored one, a rung that was
/// removed — still lands on the ladder rather than stepping off a value that is
/// not on it.
pub(crate) fn zoom_step(factor: f64, up: bool) -> f64 {
    let mut nearest = 0;
    for (index, rung) in ZOOM_LADDER.iter().enumerate() {
        if (rung - factor).abs() < (ZOOM_LADDER[nearest] - factor).abs() {
            nearest = index;
        }
    }
    let stepped = if up {
        (nearest + 1).min(ZOOM_LADDER.len() - 1)
    } else {
        nearest.saturating_sub(1)
    };
    ZOOM_LADDER[stepped]
}

// ── What the head, the foot and the cards read ─────────────────────────────

/// Everything about a page that this window draws, and nothing it does not.
///
/// **Every field arrives on an event.** None of them is polled, and the W1
/// report is explicit about why for the two that matter most: 「`CanGoBack` /
/// `CanGoForward` 事件驱动 ... 不要照抄小样的 420ms 假节拍」. A getter read on
/// the window's frame clock would be sampling values that settle on the
/// engine's.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PageFacts {
    /// `DocumentTitleChanged`. Empty until a document says what it is called,
    /// which is what makes the head fall back to the address.
    pub(crate) title: String,
    /// `SourceChanged` — the committed address, including the ones the history
    /// API writes without completing a navigation.
    pub(crate) url: String,
    pub(crate) can_go_back: bool,
    pub(crate) can_go_forward: bool,
    /// Between `NavigationStarting` and `NavigationCompleted`. What turns the
    /// reload button into a stop.
    pub(crate) loading: bool,
    /// When the navigation in flight began — the phase the mark's own spinner is
    /// carried round on (§7.7 ②: 「导航在途时这一格自转」).
    ///
    /// An instant and not a counter, because the spin is a *rate* and the frames
    /// it is sampled on are whatever the loop happens to turn.
    pub(crate) loading_since: Option<Instant>,
    /// `StatusBarTextChanged` — where the pointer is pointing, resolved by the
    /// engine. Empty when it is over nothing.
    pub(crate) hover: String,
}

impl PageFacts {
    /// What the head's name cell shows.
    ///
    /// The title, and the address until there is one. A blank cell over a page
    /// that is loading would be the one moment the head says nothing at all,
    /// and the address is what the reader just typed.
    pub(crate) fn name(&self) -> &str {
        if self.title.is_empty() {
            &self.url
        } else {
            &self.title
        }
    }
}

// ── The driver ─────────────────────────────────────────────────────────────

/// How long anything waits for a browser process to say it is gone.
///
/// Ten seconds, which is the probe's own wait and the one the eight measured
/// shutdowns were read against: six answered in 271–390 ms, one in 6 588 ms and
/// one not at all (`w0p-evidence.md` §4.2). A number chosen under the slowest
/// one that *did* answer would turn that shutdown into a false deadline.
const BROWSER_EXIT_DEADLINE: Duration = Duration::from_secs(10);

/// How far apart two presses may land and still be one double click, in
/// physical pixels.
const DOUBLE_CLICK_SLOP: i32 = 6;

/// Which button a mouse event moves, and which way.
fn button_bit(event: bt_platform::WebMouseEvent) -> Option<(u32, bool)> {
    use bt_platform::WebMouseEvent as Event;
    use bt_platform::web_mouse_buttons as bit;
    Some(match event {
        Event::LeftDown | Event::LeftDoubleClick => (bit::LEFT, true),
        Event::LeftUp => (bit::LEFT, false),
        Event::RightDown => (bit::RIGHT, true),
        Event::RightUp => (bit::RIGHT, false),
        Event::MiddleDown => (bit::MIDDLE, true),
        Event::MiddleUp => (bit::MIDDLE, false),
        Event::XDown(1) => (bit::X1, true),
        Event::XUp(1) => (bit::X1, false),
        Event::XDown(_) => (bit::X2, true),
        Event::XUp(_) => (bit::X2, false),
        Event::Move | Event::Leave | Event::Wheel(_) | Event::HorizontalWheel(_) => return None,
    })
}

/// What the window has to do about something the engine said.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum WebOutcome {
    /// The window took a chord back from the page; run it.
    Run(Action),
    /// `Tab` walked off the end of the page's own controls.
    ///
    /// The Tab contract's second half: inside a page `Tab` is the page's, and at
    /// its edge the keyboard comes back to Folio. `forward` is which edge.
    FocusLeftThePage { forward: bool },
    /// The page wants a cursor. A Win32 `IDC_*` number.
    Cursor(u32),
    /// The page took (`true`) or lost (`false`) the keyboard.
    PageFocus(bool),
    /// **A top-level navigation actually loaded, and this seat's identity is now
    /// that URL** (W2 slice ③).
    ///
    /// Read straight off [`WebMachine::recoverable_url`] and not off the event,
    /// which is the whole point: `plan.md` §4 says only a
    /// `NavigationCompleted(IsSuccess)` writes that field, and `plan.md` §3 says
    /// the switcher's identity is the last successfully committed URL. Those are
    /// **one sentence**, so they are one field — the machine's — and this outcome
    /// is how the pool, the switcher, `session.json` and Recent read it. A second
    /// ledger kept beside it would be the two accounts disagreeing about which
    /// page a restart comes back to.
    ///
    /// Emitted only when the machine's answer actually changed, so a reload of
    /// the same address is not a new row.
    ///
    /// **It carries nothing**, and that is the point: the window asks
    /// [`WebSeat::identity`] for the URL, so the only string anybody reads is the
    /// machine's own field. An outcome that carried a copy would be a second
    /// account travelling beside the first, and the two would part company the
    /// first time one of them was dropped on the floor.
    Committed,
    /// The teardown is finished: the browser has let go and the seat may be
    /// forgotten.
    Gone,
    /// A navigation was refused, and this is where it wanted to go.
    ///
    /// Slice ④ draws this as the「导航被拦」card (`DESIGN.md` §7.7 ④). Until then
    /// it is said out loud on `stderr`, because the difference between "the
    /// policy stopped it" and "it went and came back" is not otherwise visible
    /// from outside the process — and one of those two is a security hole.
    Refused(String),
    /// Something the window should say out loud that **no card covers**.
    ///
    /// Slice ④ drew the five §7.7 ④ rules, and this is what is left over: an
    /// engine error in a state nobody has written a sentence for. It goes where
    /// `BT_DPI` goes and for the same reason — a fact with nowhere to be drawn
    /// is still a fact.
    Fault(String),
    /// A URL the window should hand to the machine's browser.
    ///
    /// Raised by a download the engine cancelled whose address a plain link can
    /// replay (方案 §0). External hand-off is the window's verb and always has
    /// been; the seat only says which address.
    HandOff(String),
    /// The find session's tally, on its way to the search capsule.
    FindMatches { count: i32, active: i32 },
    /// **A `CapturePreview` came back** (W2 slice ⑥) — the encoded PNG, or
    /// `None` if the engine refused, together with the viewport size it is a
    /// picture of.
    ///
    /// The size rides along rather than being read at the far end because the
    /// seat may be re-sized between the ask and the answer, and a picture
    /// labelled with the wrong shape is a picture that will be drawn stretched
    /// and never noticed.
    Captured {
        png: Option<Vec<u8>>,
        source: Option<(u32, u32)>,
    },
}

/// Why the driver is waiting for a browser process to go.
///
/// The distinction matters because the two waits end in opposite things: one
/// ends a seat and the other starts it again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrowserWait {
    /// The seat is closing.
    Teardown,
    /// Evergreen replaced the runtime under a running process, so a live
    /// browser was asked to go and a new environment cannot be built until it
    /// has: **a new environment over a folder the old browser still holds does
    /// not fail, it simply never calls back** (`w0p-evidence.md` §3.4).
    Rebuild,
}

/// **Where a web seat lives**: the window that owns it, and the key of the
/// visual inside that window's tree that its page composes into.
///
/// The two travel together because they are one fact. The window's compositor
/// holds one visual per page since W2 slice ③, and a controller is pointed at
/// one of them for its whole life (`SetRootVisualTarget`) under one parent HWND
/// (`put_ParentWindow`). Keeping them as one value rather than two fields is
/// what makes "this seat's page composes into this seat's box, in this seat's
/// window" true by construction: the attach, the placement, the detach and the
/// controller the next rebuild asks for cannot name four different places.
///
/// # Why the key is a whole [`bt_platform::PageVisual`] and not a seat number
///
/// F1a wrote this field as a bare `seat: u64` because that was what the
/// compositor's table was keyed by. It is a **platform-layer address**, and its
/// job is to be the one name under which this page is reachable in the window it
/// is standing in — so the moment that table stopped being nameable by a seat
/// number, this had to stop being one too. A seat number is unique only inside
/// its tab (`seats::Seats::lone_seat` starts every torn-out tab at one), so an
/// address that carried only the seat could name two different pages of one
/// window, and the four operations above would then be *guaranteed* to disagree
/// rather than merely able to. The tab travels with it for that reason and no
/// other: nothing here reads it, and everything here is filed under it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SeatAddress {
    pub(crate) page: bt_platform::PageVisual,
    pub(crate) hwnd: std::num::NonZeroIsize,
}

/// How one [`WebSeat::rehost`] ended, in the caller's own vocabulary.
///
/// Four answers and not two, because a tear-out has to be told three different
/// things: that it may move its tab and the page went with it whole, that it may
/// move its tab and the page will come back rebuilt, and that it may **not**
/// move its tab at all.
/// **No caller in the tree yet, on purpose.** F1a is the spike and this narrow
/// contract; F1b is the App-level transfer transaction that presses it. The
/// convention is `git.rs`'s: an item a named later slice will call carries the
/// allow rather than being held out of the build until then, because the thing
/// the slice has to get right is the contract and the contract is what a test
/// can hold today.
#[allow(dead_code, reason = "F1b's transfer transaction is the caller")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RehostReport {
    /// The live page moved: same document, same history, same heap, no
    /// navigation. The caller moves its tab.
    Moved,
    /// There was nothing to hand over — the seat is still coming up, is on its
    /// way out, or is already where it was asked to go. Its address is now the
    /// target's, so whatever it builds next builds there. The caller moves its
    /// tab.
    AddressOnly,
    /// **Nothing moved.** The page is still on the source window at the bounds
    /// it had, and the caller's tab stays where it is. The string is where the
    /// handoff refused.
    SourceKept(String),
    /// The handoff could not be undone, so the controller was closed. The
    /// address has moved and the seat is rebuilding at its last good URL **in
    /// the target window**; the page's in-document state is gone, and this
    /// answer says so rather than calling itself a move.
    Rebuilding(String),
}

/// One web seat: the state machine, the engine, and the mint they share.
pub(crate) struct WebSeat {
    /// Which window and which visual this page belongs to **right now** — the
    /// one thing [`WebSeat::rehost`] moves, and the one thing every rebuild
    /// reads.
    address: SeatAddress,
    folder: PathBuf,
    machine: WebMachine,
    host: WebHost,
    /// What this pane asked for. Shared with the navigation gate, which runs
    /// inside a COM callback and cannot reach `self`.
    mint: Rc<RefCell<Mint>>,
    /// The chords the window takes back, and what each one runs.
    claims: Vec<ClaimedChord>,
    /// What is waiting on a browser, and until when.
    waiting: Option<(BrowserWait, Instant)>,
    /// Where the window says the page belongs this frame.
    ///
    /// Recorded whether or not there is a controller to tell, because **the
    /// engine has to be given its size before it is given a URL**. The first
    /// run of this slice navigated first and sized a frame later: the page laid
    /// out against the controller's default zero-by-zero bounds, and what
    /// arrived on the glass was a document whose backgrounds had re-laid
    /// themselves to the real width while its text was still rastered for the
    /// size it loaded at — a page with bands and no words, unchanged by a click,
    /// and correct the instant the window was resized. So the wish is kept here
    /// from the moment the window has one, and [`WebSeat::apply_presence`] pays
    /// it the moment there is somebody to pay.
    wanted: WebPresence,
    /// What the engine has actually been told, so a frame that changed nothing
    /// issues no calls.
    presence: Option<WebPresence>,
    /// **How big this seat's rectangle is**, whether or not the page is on the
    /// glass — see [`WebSeat::apply_presence`] for why the two are separate
    /// questions since W2 slice ③.
    wanted_size: Option<(u32, u32)>,
    /// And what the engine has actually been told about it.
    sized: Option<(u32, u32)>,
    /// Which buttons the page believes are down.
    ///
    /// Kept here and nowhere else because it is derived from the very events
    /// this seat forwards: a mask assembled at the call site would be a second
    /// account of the same presses, and the two would part company the first
    /// time a button came up over another window.
    buttons: u32,
    /// The last left press, for the double click the engine cannot infer.
    ///
    /// `SendMouseInput` has a `LEFT_BUTTON_DOUBLE_CLICK` kind and no way to
    /// derive it: a host that only ever sent `LEFT_BUTTON_DOWN` twice would give
    /// a page a `click` and never a `dblclick`, so selecting a word by
    /// double-clicking it would silently not work.
    last_left_press: Option<(Instant, (i32, i32))>,
    /// **What the host minted for this seat**, carried from the call that asked
    /// for the navigation to the moment the navigation is issued (W2 slice 5).
    ///
    /// It is *carried* rather than derived from the URL at the point of issue,
    /// and the difference is the whole of `webnav`'s 2: a mint is a note the
    /// host wrote about its own intention, and a rule that read `file:` off the
    /// front of a string and concluded "the host must have meant this" would
    /// turn that note back into a property of the string. The recovery machine
    /// is why it has to survive the request: a browser that crashes is rebuilt
    /// and re-navigated to `desired_url`, and that second navigation is as much
    /// the host's own as the first was.
    ///
    /// Last write wins, mirroring section 4's `desired_url`, because it is the
    /// same fact seen from the policy's side.
    minted: Mint,
    /// Everything the head, the foot and the cards read (slice ④).
    page: PageFacts,
    /// What the seat is showing instead of — or over — its page.
    fault: Option<WebFault>,
    /// The refusal the navigation gate produced, kept by the URL it was about.
    ///
    /// Shared with the gate for [`WebSeat::mint`]'s reason: the gate runs inside
    /// a COM callback and cannot reach `self`. It is written there and read here
    /// when `NavigationStarting` reports the cancel, which is what lets the
    /// 「导航被拦」 card name a reason **the door actually gave** rather than one
    /// re-derived afterwards from a mint that may have moved on.
    refusal: Rc<RefCell<Option<(String, Refusal)>>>,
    /// Whether a find session has actually been started on this page.
    ///
    /// `ICoreWebView2::Find` is not free to ask for: reaching the session at all
    /// makes the engine take the keyboard (measured, 2026-08-22 — see
    /// [`WebSeat::find`]), so a capsule opened with an empty query must not
    /// touch it, and a capsule closed without one must not either.
    finding: bool,
    /// The term the session that is running was started with, so that a second
    /// ask on the same term is a walk rather than a fresh search.
    found: String,
    /// **Whether a `CapturePreview` is out and unanswered** (W2 slice ⑥).
    ///
    /// One at a time, and the reason is the measurement: the ask costs the
    /// asking thread 0.115 ms and the answer takes 33–85 ms to come back, so a
    /// second ask made before the first landed would be a queue growing at the
    /// frame rate against a drain running at twelve a second. It is kept here
    /// and not in the store because the engine is what is busy.
    capturing: bool,
    /// The page's zoom as this window last set it.
    ///
    /// Kept here as well as in the controller because `Ctrl`+wheel steps from
    /// the rung it is on, and a rung read back through COM on every notch would
    /// be a syscall inside a wheel gesture.
    zoom: f64,
}

impl WebSeat {
    /// Open a web seat on this pane and start the engine towards `url`.
    pub(crate) fn open(
        page: bt_platform::PageVisual,
        hwnd: std::num::NonZeroIsize,
        url: &str,
        minted: Mint,
        wake: Box<dyn Fn()>,
    ) -> Result<Self, String> {
        let folder = user_data_folder().ok_or_else(|| {
            String::from("LOCALAPPDATA is not set, so there is no profile to use")
        })?;
        let mint = Rc::new(RefCell::new(Mint::Nothing));
        let gate = Rc::clone(&mint);
        let refusal = Rc::new(RefCell::new(None));
        let refusal_sink = Rc::clone(&refusal);
        let host = WebHost::new(
            Box::new(move |candidate| {
                match navigation_starting(candidate, &gate.borrow()) {
                    Decision::Navigate(target) if target == candidate => {
                        WebNavigationVerdict::Proceed
                    }
                    Decision::Navigate(target) => WebNavigationVerdict::CancelAndNavigateTo(target),
                    // A search cannot come out of this door — slice ② has a test
                    // that pins that — and a candidate that is not an address is
                    // not one this seat is going to.
                    //
                    // **The reason is written down where it was decided.** The
                    // 「导航被拦」card needs it, and the only place it exists is
                    // inside this closure: `NavigationStarting` reports that a
                    // navigation was cancelled and never why, and asking the
                    // door a second time afterwards would be asking it about a
                    // mint that may have moved on between the two questions.
                    Decision::Refuse(why) => {
                        *refusal_sink.borrow_mut() = Some((candidate.to_owned(), why));
                        WebNavigationVerdict::Cancel
                    }
                    Decision::Search(_) => WebNavigationVerdict::Cancel,
                }
            }),
            wake,
        );
        let mut web = Self {
            address: SeatAddress { page, hwnd },
            folder,
            machine: WebMachine::new(),
            host,
            mint,
            claims: Vec::new(),
            waiting: None,
            wanted: WebPresence::Hidden,
            presence: None,
            wanted_size: None,
            sized: None,
            buttons: bt_platform::web_mouse_buttons::NONE,
            last_left_press: None,
            minted,
            page: PageFacts::default(),
            finding: false,
            found: String::new(),
            fault: None,
            refusal,
            capturing: false,
            zoom: 1.0,
        };
        let effect = web.machine.request(url);
        debug_assert_eq!(effect, WebEffect::Ignore, "an engine that is not up yet");
        web.start_environment()?;
        Ok(web)
    }

    /// **Go somewhere on this seat** — the one door every later navigation takes
    /// (W2 slice ③).
    ///
    /// The switcher's row, a pin, a restored session and the address field slice
    /// ④ will grow all arrive here, and each of them is *a request*, not a
    /// permission: the URL still passes `webnav::address_bar` at the call site
    /// and `webnav::navigation_starting` inside the engine's own callback, which
    /// is the two-gate rule `plan.md` §3 states and `webnav`'s ① records at
    /// length. This method's own contract is narrower and is the recovery
    /// machine's: last write wins, and nothing is navigated until the events are
    /// installed.
    ///
    /// It answers the effect rather than acting on it, because acting needs the
    /// window's compositor and this type is asked from places that do not hold
    /// one — see [`WebSeat::go`], which is this plus that.
    pub(crate) fn go(
        &mut self,
        url: &str,
        minted: Mint,
        compositor: &bt_platform::Compositor,
    ) -> Vec<WebOutcome> {
        self.minted = minted;
        let effect = self.machine.request(url);
        let mut outcomes = Vec::new();
        self.apply(effect, compositor, &mut outcomes);
        outcomes
    }

    /// **Read the page again** - the file behind it was saved (W2 slice 5).
    ///
    /// [`WebMachine::reload`] is the sentence; this is that plus the compositor
    /// the effect needs, exactly as [`Self::go`] is `request` plus it.
    pub(crate) fn reload(&mut self, compositor: &bt_platform::Compositor) -> Vec<WebOutcome> {
        let effect = self.machine.reload();
        let mut outcomes = Vec::new();
        self.apply(effect, compositor, &mut outcomes);
        outcomes
    }

    /// **What this seat is remembered as** — the last URL that actually loaded.
    ///
    /// One field, one reader: [`WebMachine::recoverable_url`]. See
    /// [`WebOutcome::Committed`] for why there is no second account of it.
    pub(crate) fn identity(&self) -> Option<&str> {
        self.machine.recoverable_url()
    }

    /// The chords the window takes back from a focused page.
    ///
    /// Recomputed whenever the effective table or the window's focus changes,
    /// because both change the answer and the COM callback has no way to ask.
    pub(crate) fn set_claims(&mut self, shortcuts: &Shortcuts, focus: Focus) {
        let claims = claimable_chords(shortcuts, focus);
        if claims == self.claims {
            return;
        }
        self.host
            .set_claimed_chords(claims.iter().map(|claim| claim.chord).collect());
        self.claims = claims;
    }

    /// Read everything the engine has said and act on it.
    pub(crate) fn drive(&mut self, compositor: &bt_platform::Compositor) -> Vec<WebOutcome> {
        let mut outcomes = Vec::new();
        for event in self.host.drain() {
            let effect = self.digest(&event, &mut outcomes);
            self.apply(effect, compositor, &mut outcomes);
        }
        outcomes
    }

    /// One event, turned into one effect — and into whatever the window has to
    /// hear about directly.
    fn digest(&mut self, event: &WebEvent, outcomes: &mut Vec<WebOutcome>) -> WebEffect {
        match event {
            WebEvent::Environment { generation, error } => {
                // **The runtime question is asked of the loader, never of this
                // error string.** Gate 7 watched the registry go on reporting a
                // version for a runtime that was not there while
                // `CreateCoreWebView2EnvironmentWithOptions` failed
                // synchronously; the loader is the oracle that did not lie. So
                // an environment that failed is two different cards depending on
                // one further fact, and that fact is a second call rather than a
                // pattern match on a message.
                //
                // **An environment that failed with a runtime installed draws
                // no card**, and that is a boundary of the ruling rather than a
                // gap in the code: §7.7 ④ names five states and this is not one
                // of them, so it goes where slice ① put every fact with nowhere
                // to be drawn — `stderr`. Recorded as an open item; a sixth card
                // is a sentence somebody has to rule.
                if let Some(error) = error {
                    if bt_platform::webview2_runtime_version().is_err() {
                        self.fault = Some(WebFault::RuntimeMissing {
                            detail: error.clone(),
                        });
                    }
                    outcomes.push(WebOutcome::Fault(error.clone()));
                }
                self.machine.on_environment(*generation, error.is_none())
            }
            WebEvent::Controller { generation, error } => {
                if let Some(error) = error {
                    outcomes.push(WebOutcome::Fault(error.clone()));
                }
                self.machine.on_controller(*generation, error.is_none())
            }
            // The state machine has no opinion about a navigation that has not
            // finished; what the window owes is the refusal, said out loud.
            WebEvent::NavigationStarting { uri, cancelled } => {
                if *cancelled {
                    // The card is for a URL the **seat** was handed, not for a
                    // link inside a page: §7.1.5g ⑤ says a link this window will
                    // not follow does nothing and says so in the foot, and there
                    // is a page standing there to say it over. What tells the two
                    // apart is whether anything ever committed here.
                    let refused = self.refusal.borrow_mut().take();
                    if let Some((url, why)) = refused
                        && self.page.url.is_empty()
                    {
                        self.fault = Some(WebFault::Blocked { url, refusal: why });
                    }
                    outcomes.push(WebOutcome::Refused(uri.clone()));
                } else {
                    if !self.page.loading {
                        self.page.loading_since = Some(Instant::now());
                    }
                    self.page.loading = true;
                }
                WebEffect::Ignore
            }
            WebEvent::NavigationCompleted {
                uri,
                success,
                status,
            } => {
                self.page.loading = false;
                self.page.loading_since = None;
                if *success {
                    self.fault = None;
                } else if let Some(fault) = load_fault(uri, *success, *status) {
                    self.fault = Some(fault);
                }
                // Asked *before* and *after*, and the answer is the machine's
                // both times: a failure page, an `about:blank` and a cancelled
                // navigation all reach here and none of them may move the
                // identity (`plan.md` §4). Comparing the machine's own field
                // across the call is what makes that true without this arm
                // knowing which of the three it is looking at.
                let was = self.machine.recoverable_url().map(str::to_owned);
                let effect =
                    self.machine
                        .on_navigation_completed(self.machine.generation(), uri, *success);
                if let Some(now) = self.machine.recoverable_url()
                    && was.as_deref() != Some(now)
                {
                    outcomes.push(WebOutcome::Committed);
                }
                effect
            }
            // 0 is the browser process and 1 the renderer: one name over two
            // entirely different events.
            WebEvent::ProcessFailed { kind, .. } if *kind == 0 => {
                self.browser_is_gone(WebEvent::ProcessFailed {
                    kind: 0,
                    description: String::new(),
                })
            }
            WebEvent::ProcessFailed { .. } => {
                self.page.loading = false;
                self.page.loading_since = None;
                self.fault = Some(WebFault::RenderProcessGone);
                self.machine.on_render_process_failed()
            }
            WebEvent::BrowserProcessExited { kind } => {
                self.browser_is_gone(WebEvent::BrowserProcessExited { kind: *kind })
            }
            WebEvent::NewBrowserVersionAvailable => self.machine.on_new_browser_version_available(),
            WebEvent::AcceleratorKey { key, handled } => {
                // The key-up half of a claimed chord is the same chord arriving
                // a second time; the verb runs once, on the way down.
                if *handled && key.down {
                    outcomes.extend(self.action_for(key.chord).map(WebOutcome::Run));
                }
                WebEffect::Ignore
            }
            WebEvent::MoveFocusRequested { reason } => {
                // `COREWEBVIEW2_MOVE_FOCUS_REASON`: 1 next, 2 previous.
                outcomes.push(WebOutcome::FocusLeftThePage {
                    forward: *reason != 2,
                });
                WebEffect::Ignore
            }
            WebEvent::GotFocus => {
                outcomes.push(WebOutcome::PageFocus(true));
                WebEffect::Ignore
            }
            WebEvent::LostFocus => {
                outcomes.push(WebOutcome::PageFocus(false));
                WebEffect::Ignore
            }
            WebEvent::CursorChanged { system_cursor_id } => {
                outcomes.push(WebOutcome::Cursor(*system_cursor_id));
                WebEffect::Ignore
            }
            WebEvent::HistoryChanged {
                can_go_back,
                can_go_forward,
            } => {
                self.page.can_go_back = *can_go_back;
                self.page.can_go_forward = *can_go_forward;
                WebEffect::Ignore
            }
            WebEvent::DocumentTitleChanged { title } => {
                self.page.title.clone_from(title);
                WebEffect::Ignore
            }
            WebEvent::SourceChanged { uri } => {
                // The blank page this host mints for itself is not an address a
                // person asked for, and putting it in the head would be the seat
                // announcing its own scaffolding.
                if uri.eq_ignore_ascii_case(BLANK_PAGE) {
                    self.page.url.clear();
                } else {
                    self.page.url.clone_from(uri);
                }
                WebEffect::Ignore
            }
            WebEvent::StatusBarTextChanged { text } => {
                self.page.hover.clone_from(text);
                WebEffect::Ignore
            }
            // **The download is already cancelled** — the engine could not be
            // asked later. What is decided here is what happens instead, and
            // the rule is 方案 §0's: hand over a URL that can be replayed, and
            // say so when it cannot. What「可重放」means is not guessed at —
            // it is the address bar's own answer, because a `blob:` or a
            // `data:` URL names memory inside a page rather than a request
            // anybody else can make, and those are exactly the ones that door
            // already refuses.
            WebEvent::DownloadStarting { uri, file_name } => {
                match download_answer(uri, file_name) {
                    Ok(target) => outcomes.push(WebOutcome::HandOff(target)),
                    Err(fault) => self.fault = Some(fault),
                }
                WebEffect::Ignore
            }
            WebEvent::FindMatches { count, active } => {
                outcomes.push(WebOutcome::FindMatches {
                    count: *count,
                    active: *active,
                });
                WebEffect::Ignore
            }
            // **The picture of this page a card asked for** (W2 slice ⑥). The
            // seat is the messenger and nothing else: what the bytes are decoded
            // to, at what size, and which card draws them are `web_thumb`'s, and
            // a failed capture travels as a `None` so that the store can let go
            // of the slot it was holding for it.
            WebEvent::Captured { png } => {
                self.capturing = false;
                outcomes.push(WebOutcome::Captured {
                    png: png.clone(),
                    // The size the engine was last told, which is the size the
                    // picture is of — asked here rather than at the far end,
                    // because by the time the bytes are decoded the seat may
                    // have been given another rectangle.
                    source: self.sized,
                });
                WebEffect::Ignore
            }
        }
    }

    /// A browser process died, by either of the two doors that say so.
    ///
    /// **A rebuild already in flight consumes the old browser's obituary.** The
    /// browser this seat is rebuilding *away from* was asked to go; the event
    /// saying it went is the end of that wait, not the news of a fresh crash,
    /// and feeding it to the state machine would start a second rebuild of the
    /// thing already being rebuilt.
    fn browser_is_gone(&mut self, event: WebEvent) -> WebEffect {
        if matches!(self.waiting, Some((BrowserWait::Rebuild, _))) {
            self.waiting = None;
            return WebEffect::RebuildForNewVersion;
        }
        match event {
            WebEvent::BrowserProcessExited { .. } => self.machine.on_browser_process_exited(),
            _ => self.machine.on_browser_process_failed(),
        }
    }

    /// Do what an effect says, and whatever the effect it produces says after
    /// that. The chain is at most two long — `InstallEvents` is acknowledged and
    /// yields the first navigation — but it is written as a loop so that a third
    /// link cannot be added by accident somewhere that only handles two.
    fn apply(
        &mut self,
        first: WebEffect,
        compositor: &bt_platform::Compositor,
        outcomes: &mut Vec<WebOutcome>,
    ) {
        let mut effect = first;
        loop {
            let next = match self.step(&effect, compositor, outcomes) {
                Ok(next) => next,
                Err(error) => {
                    outcomes.push(WebOutcome::Fault(error));
                    return;
                }
            };
            match next {
                Some(follow_up) => effect = follow_up,
                None => return,
            }
        }
    }

    fn step(
        &mut self,
        effect: &WebEffect,
        compositor: &bt_platform::Compositor,
        outcomes: &mut Vec<WebOutcome>,
    ) -> Result<Option<WebEffect>, String> {
        match effect {
            WebEffect::Ignore => Ok(None),
            WebEffect::CreateController => {
                self.host
                    .request_controller(self.address.hwnd, self.machine.generation())?;
                Ok(None)
            }
            WebEffect::InstallEvents => {
                // The visual first: the controller is told where to render
                // before it is told to do anything at all.
                compositor.attach_web_visual(self.address.page)?;
                self.host.install(compositor, self.address.page)?;
                // **Before the navigation, never after.** The next line's
                // acknowledgement produces the first `Navigate`, and a page that
                // begins loading against the controller's default zero-by-zero
                // bounds rasters its text for a viewport it will never be shown
                // in. See [`WebSeat::wanted`].
                self.apply_presence(compositor)?;
                let generation = self.machine.generation();
                Ok(Some(self.machine.on_events_installed(generation)))
            }
            WebEffect::Navigate(url) => {
                // **The mint before the navigation, always.**
                // `NavigationStarting` can fire before `Navigate` has returned,
                // and a gate asked about a target the pane has not yet admitted
                // to minting would cancel the pane's own navigation.
                //
                // Three answers and not two, since slice 5: the seat's own blank
                // page, the one `file:` URL a controlled file entry minted and
                // handed in with the request, and - for every ordinary address -
                // nothing at all. The carried mint is honoured only for the URL
                // it was made for, so a mint left over from a page that has since
                // been navigated away from admits nothing.
                let minted = if url.eq_ignore_ascii_case(BLANK_PAGE) {
                    Mint::Blank
                } else if self.minted.target() == Some(url.as_str()) {
                    self.minted.clone()
                } else {
                    Mint::Nothing
                };
                // **The third door** (`webnav` 6): what the host mints, the host
                // asks about before it issues it, so that "every navigation this
                // product starts has been through a gate" has no exception in it.
                // An ordinary address has no mint and was gated at its call site
                // by `webnav::address_bar`; there is nothing here for it to
                // answer.
                if minted != Mint::Nothing {
                    match check(url, Origin::HostMinted(&minted)) {
                        Decision::Navigate(target) if target == *url => {}
                        verdict => {
                            return Err(format!(
                                "the host declined to issue its own navigation to {url}: {verdict:?}"
                            ));
                        }
                    }
                }
                *self.mint.borrow_mut() = minted;
                self.host.navigate(url)?;
                Ok(None)
            }
            WebEffect::Reload => {
                self.host.reload()?;
                Ok(None)
            }
            WebEffect::CloseOrphanController => {
                self.host.close_pending_controller();
                Ok(None)
            }
            WebEffect::RebuildFromScratch => {
                // The browser is *already* gone — that is what the event said —
                // so there is nothing to wait for. Its folder is free the moment
                // its process tree ended.
                self.host.close();
                self.presence = None;
                bt_platform::forget_web_environment();
                self.start_environment()?;
                Ok(None)
            }
            WebEffect::RebuildForNewVersion => {
                // Here the browser is alive and has to be asked. Only when it
                // has gone may a new environment be made — see [`BrowserWait::Rebuild`].
                if self.waiting.is_none() {
                    self.host.close();
                    self.presence = None;
                    self.waiting =
                        Some((BrowserWait::Rebuild, Instant::now() + BROWSER_EXIT_DEADLINE));
                    return Ok(None);
                }
                self.waiting = None;
                bt_platform::forget_web_environment();
                self.start_environment()?;
                Ok(None)
            }
            WebEffect::AwaitBrowserExitBeforeCleanup => {
                self.host.close();
                self.presence = None;
                self.waiting = Some((
                    BrowserWait::Teardown,
                    Instant::now() + BROWSER_EXIT_DEADLINE,
                ));
                Ok(None)
            }
            WebEffect::ReleaseUserDataFolder => {
                self.waiting = None;
                let _ = compositor.detach_web_visual(self.address.page);
                outcomes.push(WebOutcome::Gone);
                Ok(None)
            }
        }
    }

    fn start_environment(&mut self) -> Result<(), String> {
        let generation = self.machine.generation();
        let folder = self.folder.clone();
        self.host.request_environment(&folder, generation)
    }

    /// The clock the deadline door is hung on. **The only backstop on the
    /// graceful path**, where `ProcessFailed` does not come at all and
    /// `BrowserProcessExited` came late once in eight and not at all once in
    /// eight.
    pub(crate) fn tick(
        &mut self,
        now: Instant,
        compositor: &bt_platform::Compositor,
    ) -> Vec<WebOutcome> {
        let Some((kind, deadline)) = self.waiting else {
            return Vec::new();
        };
        if now < deadline {
            return Vec::new();
        }
        self.waiting = None;
        let mut outcomes = Vec::new();
        let effect = match kind {
            BrowserWait::Teardown => self.machine.on_cleanup_deadline(),
            BrowserWait::Rebuild => WebEffect::RebuildForNewVersion,
        };
        self.apply(effect, compositor, &mut outcomes);
        outcomes
    }

    /// When the window next has to come back and look at the clock.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.waiting.map(|(_, deadline)| deadline)
    }

    fn action_for(&self, chord: WebChord) -> Option<Action> {
        claim_for(&self.claims, chord).map(|claim| claim.action)
    }

    /// Put the page where the seat is, or take it off the glass.
    ///
    /// One call sets the engine's bounds, the visual's offset and its clip; the
    /// frame that follows publishes all three in the same `Commit`, which is the
    /// whole reason the visual path exists — the WebView2 spike measured zero
    /// seam here against 4–10 px of tearing on the child-window path.
    pub(crate) fn place(
        &mut self,
        compositor: &bt_platform::Compositor,
        presence: WebPresence,
        size: Option<(u32, u32)>,
    ) -> Result<(), String> {
        self.wanted = presence;
        // **The size is not the presence** (W2 slice ③). A seat's rectangle
        // exists whenever its pane does; whether the page is *on the glass* is a
        // second question, answered by a modal and by which tab is in front. They
        // were one answer while a window held one page, because the only page
        // there was was the one you were looking at.
        if let Some(size) = size {
            self.wanted_size = Some(size);
        }
        self.apply_presence(compositor)
    }

    /// Tell the engine where it is, if there is an engine and it does not know
    /// already.
    fn apply_presence(&mut self, compositor: &bt_platform::Compositor) -> Result<(), String> {
        if !self.host.has_controller() {
            return Ok(());
        }
        // **A page is given its size even while it is hidden**, and this is slice
        // ①'s own sentence — "the engine has to be given its size before it is
        // given a URL" — meeting the case slice ③ created. A page can now be born
        // on a tab nobody is looking at (a restored window with two of them) or
        // behind a modal (the restore prompt is up at launch), and a page whose
        // controller was never sized loads against zero by zero. Measured: such a
        // page never committed at all, so the seat had no identity, no pool row
        // and nothing in `session.json`.
        if let Some((width, height)) = self.wanted_size
            && self.sized != self.wanted_size
        {
            self.host.set_size(width, height)?;
            self.sized = Some((width, height));
        }
        if self.presence == Some(self.wanted) {
            return Ok(());
        }
        self.presence = Some(self.wanted);
        match self.wanted {
            WebPresence::Hidden => self.host.set_visible(false),
            WebPresence::Shown(bounds) => {
                compositor.place_web_visual(
                    self.address.page,
                    (bounds.x, bounds.y),
                    (0.0, 0.0, bounds.width as f32, bounds.height as f32),
                )?;
                self.host.set_visible(true)
            }
        }
    }

    // ── Moving one seat to another window (F1a) ────────────────────────────

    /// **Where this seat lives.** Read by everything that builds a controller.
    #[allow(dead_code, reason = "F1b's transfer transaction is the caller")]
    pub(crate) fn address(&self) -> SeatAddress {
        self.address
    }

    /// **Move this seat, and the live page on it, into another window.**
    ///
    /// The narrow contract `plan.md`'s v3 增补 names: `WebSeat` caches its own
    /// seat key and HWND, so moving only the window's map entry would leave the
    /// *next* rebuild — a browser crash, an Evergreen update, a failed handoff —
    /// asking for a controller on the window this page has left. This is the one
    /// door that writes that cache, and it writes it in the same call that moves
    /// the page.
    ///
    /// # Three phases, and only the middle one can half-happen
    ///
    /// **Prepare** builds the target window's visual: it can fail, and when it
    /// does nothing has been asked of the controller at all. **The platform
    /// handoff** is [`bt_platform::WebHost::rehost`], every step of which has a
    /// written compensation. **The model commit** is this function's own last
    /// few lines — the address, the presence caches and the source window's now
    /// empty visual — and by the time it runs there is nothing left that can
    /// fail.
    ///
    /// # What the old window is owed first
    ///
    /// A page that was mid-drag believes buttons are down and the pointer is
    /// inside it. Both are facts about a window it is leaving, so both are
    /// settled against the old host before the parent changes: every held button
    /// is released and the pointer is moved out of the page — which is how this
    /// product says "leave" at all, `SendMouseInput(LEAVE)` being refused by the
    /// engine in every spelling (`w0p-evidence.md` §1 gate 3).
    ///
    /// # Focus follows the tab
    ///
    /// `take_focus` puts the keyboard back into the page in its new window. The
    /// caller passes it when the moved tab is the one in front, because a person
    /// whose hand just carried this page somewhere has said where they are
    /// looking.
    #[allow(dead_code, reason = "F1b's transfer transaction is the caller")]
    pub(crate) fn rehost(
        &mut self,
        from: &bt_platform::Compositor,
        to: &bt_platform::Compositor,
        address: SeatAddress,
        take_focus: bool,
        outcomes: &mut Vec<WebOutcome>,
    ) -> RehostReport {
        if address == self.address {
            return RehostReport::AddressOnly;
        }
        // Prepare. The one resource that must exist before the controller is
        // touched, and the one whose failure costs the source window nothing.
        if let Err(error) = to.attach_web_visual(address.page) {
            return RehostReport::SourceKept(error);
        }
        // A seat whose controller has not arrived — or has already gone — has no
        // page to hand over and still has to follow its tab. Its next controller
        // is built on the window it is in now, which is exactly what moving the
        // address means.
        if !self.host.has_controller() {
            self.adopt(from, address);
            return RehostReport::AddressOnly;
        }
        self.settle_input_for_handoff();
        let size = self.sized.or(self.wanted_size).unwrap_or((0, 0));
        let visible = matches!(self.wanted, WebPresence::Shown(_));
        let outcome = self.host.rehost(
            &bt_platform::RehostSide {
                compositor: from,
                page: self.address.page,
                hwnd: self.address.hwnd,
            },
            &bt_platform::RehostSide {
                compositor: to,
                page: address.page,
                hwnd: address.hwnd,
            },
            size,
            visible,
        );
        match outcome {
            bt_platform::RehostOutcome::Moved => {
                self.adopt(from, address);
                if take_focus {
                    let _ = self.host.focus_page();
                }
                RehostReport::Moved
            }
            bt_platform::RehostOutcome::KeptSource {
                failed_at, error, ..
            } => {
                // The target's visual was built for a page that is not coming.
                let _ = to.detach_web_visual(address.page);
                RehostReport::SourceKept(format!("{failed_at:?}: {error}"))
            }
            bt_platform::RehostOutcome::Lost {
                failed_at,
                error,
                compensation_error,
            } => {
                // **The address moves first.** The rebuild that follows asks for
                // a controller, and the whole of this branch is that it must ask
                // for one on the window the person put the tab in.
                self.adopt(from, address);
                let effect = self.machine.on_rehost_lost();
                self.apply(effect, to, outcomes);
                RehostReport::Rebuilding(format!(
                    "{failed_at:?}: {error}; the compensation failed too: {compensation_error}"
                ))
            }
        }
    }

    /// The model commit, plus the source window's now empty visual.
    ///
    /// The detach is best-effort and deliberately last: an empty visual left in
    /// a tree costs nothing on screen, and a page that has already arrived
    /// somewhere else must not be reported as not having moved.
    fn adopt(&mut self, from: &bt_platform::Compositor, address: SeatAddress) {
        let _ = from.detach_web_visual(self.address.page);
        let _ = from.commit();
        self.take_address(address);
    }

    /// **This seat is now that seat, in that window** — the whole of the model
    /// commit, and the half that needs no compositor.
    ///
    /// The two presence caches go with the address, because they record what *an
    /// engine in the old window* had already been told. Left standing, the next
    /// frame would decide it had nothing to say and the page would sit at the
    /// old window's rectangle inside the new one.
    fn take_address(&mut self, address: SeatAddress) {
        self.address = address;
        self.presence = None;
        self.sized = None;
    }

    /// Give the window being left a page that believes nothing is pressed and
    /// the pointer is elsewhere.
    fn settle_input_for_handoff(&mut self) {
        use bt_platform::WebMouseEvent as Event;
        use bt_platform::web_mouse_buttons as bit;
        for (mask, up) in [
            (bit::LEFT, Event::LeftUp),
            (bit::RIGHT, Event::RightUp),
            (bit::MIDDLE, Event::MiddleUp),
            (bit::X1, Event::XUp(1)),
            (bit::X2, Event::XUp(2)),
        ] {
            if self.buttons & mask != 0 {
                self.buttons &= !mask;
                let _ = self.host.send_mouse(up, (-1, -1), self.buttons);
            }
        }
        // Outside the page on both axes, which is this product's only working
        // spelling of "the pointer left".
        let _ = self
            .host
            .send_mouse(Event::Move, (-1, -1), bt_platform::web_mouse_buttons::NONE);
        // A press that was interrupted by a window change is not half of a
        // double click.
        self.last_left_press = None;
    }

    /// Where the page is this frame, or `None` when it is not on the glass.
    ///
    /// What the engine has been *told*, not what the window last wished: a
    /// pointer is forwarded in the coordinates the page believes it occupies,
    /// and those are the ones it was given.
    pub(crate) fn shown_at(&self) -> Option<WebBounds> {
        match self.presence {
            Some(WebPresence::Shown(bounds)) => Some(bounds),
            Some(WebPresence::Hidden) | None => None,
        }
    }

    /// Forward one mouse event. `window_point` is in the window's client area,
    /// in physical pixels; the translation into the page's own space happens
    /// here and nowhere else.
    ///
    /// **A point outside the bounds is deliberately still forwarded.** That is
    /// how a page is told the pointer left it: `SendMouseInput(LEAVE)` is
    /// refused by the engine in all three spellings the API allows —
    /// `E_INVALIDARG` whatever coordinates and button mask it is given
    /// (`w0p-evidence.md` §1 gate 3) — and a move to a point outside the
    /// rectangle is the substitute the same gate measured working.
    pub(crate) fn send_mouse(
        &mut self,
        event: bt_platform::WebMouseEvent,
        window_point: (i32, i32),
        now: Instant,
    ) -> Result<(), String> {
        let Some(bounds) = self.shown_at() else {
            return Ok(());
        };
        let point = (window_point.0 - bounds.x, window_point.1 - bounds.y);
        let event = self.upgrade_to_double_click(event, window_point, now);
        // The mask the engine is handed describes the state **including** this
        // event, which is what a Win32 mouse message carries: a press arrives
        // with its own bit set and a release arrives with it already clear.
        if let Some((bit, down)) = button_bit(event) {
            if down {
                self.buttons |= bit;
            } else {
                self.buttons &= !bit;
            }
        }
        self.host.send_mouse(event, point, self.buttons)
    }

    /// A second left press in the same place, soon enough, is a double click.
    ///
    /// The interval is the window's own [`crate::MULTI_CLICK_INTERVAL`] rather
    /// than a number of this module's, so that a double click on a page and a
    /// double click on a tab are the same gesture to the same hand. The slop is
    /// six physical pixels: a hand that has moved further than that between two
    /// presses meant two presses.
    fn upgrade_to_double_click(
        &mut self,
        event: bt_platform::WebMouseEvent,
        window_point: (i32, i32),
        now: Instant,
    ) -> bt_platform::WebMouseEvent {
        if event != bt_platform::WebMouseEvent::LeftDown {
            return event;
        }
        let paired = self.last_left_press.is_some_and(|(at, was)| {
            now.saturating_duration_since(at) <= crate::MULTI_CLICK_INTERVAL
                && (was.0 - window_point.0).abs() <= DOUBLE_CLICK_SLOP
                && (was.1 - window_point.1).abs() <= DOUBLE_CLICK_SLOP
        });
        // A double click consumes its own history, exactly as the tab strip's
        // does: without this a third press would pair with the second.
        self.last_left_press = (!paired).then_some((now, window_point));
        if paired {
            bt_platform::WebMouseEvent::LeftDoubleClick
        } else {
            event
        }
    }

    // ── What the chrome reads, and the verbs it presses (slice ④) ──────────

    /// Everything the head, the foot and the cards draw from.
    pub(crate) fn page(&self) -> &PageFacts {
        &self.page
    }

    /// The card this seat is showing, if it is showing one.
    pub(crate) fn fault(&self) -> Option<&WebFault> {
        self.fault.as_ref()
    }

    /// Take the sheet away.
    ///
    /// **Only the sheet.** The four cards that *are* the seat have no Escape and
    /// no other dismissal, because taking one of them away leaves the black hole
    /// a hidden WebView draws — see [`WebFault::stands_over_the_page`].
    pub(crate) fn dismiss_sheet(&mut self) -> bool {
        if self
            .fault
            .as_ref()
            .is_some_and(WebFault::stands_over_the_page)
        {
            self.fault = None;
            return true;
        }
        false
    }

    /// Walk this page's own navigation stack.
    ///
    /// Guarded on what the head is drawing, which is what the reader pressed:
    /// the buttons are dimmed and inert when the stack has no more to give, and
    /// a call that went through anyway would be the window acting on a history
    /// the person is not looking at.
    ///
    /// **Not [`Self::go`]**, which is a different sentence that slice ③ named
    /// first: that one is the one door a *new* address takes into this seat, and
    /// this one asks the page for a place it has already been. Two verbs, two
    /// names.
    pub(crate) fn walk_history(&mut self, forwards: bool) -> Result<(), String> {
        if forwards {
            if !self.page.can_go_forward {
                return Ok(());
            }
            self.host.go_forward()
        } else {
            if !self.page.can_go_back {
                return Ok(());
            }
            self.host.go_back()
        }
    }

    /// The third button: reload, or stop while something is in flight.
    ///
    /// One verb and not two, because it is one button. 「同一秒里刷新钮变停止钮
    /// ,三个钮还是三个钮」.
    /// **The card stays up until something loads.** Clearing it on the press
    /// would blank the seat to the black hole a hidden WebView draws for as long
    /// as the retry takes, which is the one state §7.7 ④ exists to keep off the
    /// glass; the head's mark spins over the card meanwhile, so the retry is
    /// visible without the card having to leave.
    pub(crate) fn reload_or_stop(&mut self) -> Result<(), String> {
        if self.page.loading {
            return self.host.stop();
        }
        self.host.reload()
    }

    /// Open the engine's developer tools on this page.
    pub(crate) fn open_dev_tools(&self) -> Result<(), String> {
        self.host.open_dev_tools()
    }

    /// Go somewhere, because a person typed it or pressed a row that named it.
    ///
    /// **The address door and no other.** The string is judged by
    /// `webnav::address_bar` — the same function a pin, a restored session and
    /// the command palette are judged by — and a refusal is silent here on
    /// purpose: the field says it, in the field, by turning `--err` (§7.7 ④'s
    /// 「说在打字的地方」). A card would be telling a reader what they are
    /// already looking at.
    /// The bool is whether the address was taken; the outcomes are whatever the
    /// state machine had to say about starting it.
    pub(crate) fn go_to(
        &mut self,
        input: &str,
        engine: SearchEngineV1,
        compositor: &bt_platform::Compositor,
    ) -> (bool, Vec<WebOutcome>) {
        // **A non-address is a search, and the search is an address** (方案 §0).
        // The composed URL goes back through the same door a typed one does —
        // this build's own string gets no more trust than a person's, which is
        // 「钉不是授权」 said about the one URL this window writes itself.
        let target = match address_bar(input) {
            Decision::Navigate(target) => target,
            Decision::Search(query) => match address_bar(&search_url(engine, &query)) {
                Decision::Navigate(target) => target,
                Decision::Search(_) | Decision::Refuse(_) => return (false, Vec::new()),
            },
            Decision::Refuse(_) => return (false, Vec::new()),
        };
        // Through the machine and not straight at the engine: §4's `desired_url`
        // is what a seat that is still coming up remembers, and a navigation
        // issued around it would be one the recovery model never heard of.
        let effect = self.machine.request(&target);
        let mut outcomes = Vec::new();
        self.apply(effect, compositor, &mut outcomes);
        (true, outcomes)
    }

    /// Whether this text would be navigated to, for the field that has to say so
    /// while it is still being typed.
    ///
    /// The same door, asked without knocking. An empty field is not wrong — it
    /// is unfinished — so it does not light up red.
    pub(crate) fn would_go_to(input: &str) -> bool {
        input.trim().is_empty()
            || matches!(
                address_bar(input),
                Decision::Navigate(_) | Decision::Search(_)
            )
    }

    /// One notch of `Ctrl`+wheel.
    ///
    /// **`Ctrl`+wheel is empty everywhere else in this window** — there is no
    /// type-size zoom in this product and a picture zooms on the bare wheel — so
    /// nothing is being taken from anything by claiming it over a page.
    pub(crate) fn zoom_by(&mut self, up: bool) -> Result<(), String> {
        let next = zoom_step(self.zoom, up);
        if (next - self.zoom).abs() < f64::EPSILON {
            return Ok(());
        }
        self.zoom = next;
        self.host.set_zoom(next)
    }

    /// Search this page for `term`. The counts come back as
    /// [`WebOutcome::FindMatches`].
    /// **The engine takes the keyboard when this is called**, measured on the
    /// machine (2026-08-22): a capsule opened over a page and typed into
    /// received exactly one character, because the first keystroke started a
    /// find and the find moved the focus into the page. The caller takes it
    /// back — see `Runtime::refresh_search` — and this half's job is to not ask
    /// at all when there is nothing to ask about, which is every keystroke of an
    /// empty field and every close of a capsule nobody typed in.
    pub(crate) fn find(&mut self, term: &str, case_sensitive: bool) -> Result<(), String> {
        if term.is_empty() {
            self.found.clear();
            return self.find_stop();
        }
        self.finding = true;
        self.found = term.to_owned();
        self.host.find(term, case_sensitive)
    }

    /// The term the running find session was started with, or empty.
    ///
    /// What tells a keystroke that has moved the query from one that has not —
    /// and therefore whether the tally on the glass still belongs to what is in
    /// the field.
    pub(crate) fn found(&self) -> &str {
        &self.found
    }

    /// **Ask, or walk** — one door, because on a page they are one gesture.
    ///
    /// A terminal's capsule searches on every keystroke because the search is
    /// this window's own regex over its own transcript and touches nothing else.
    /// A page's cannot: `ICoreWebView2Find::Start` **moves the keyboard into the
    /// page**, measured on the machine (2026-08-22 — a live find over a page
    /// took exactly one character and then typed into the document), and there
    /// is no way to ask it not to. So on a page the find runs when the reader
    /// asks for it — `Enter`, the two walk buttons, `F3` — and what the ask
    /// means depends on whether the term has moved since the last one: a new
    /// term starts a session, the same term steps through it.
    pub(crate) fn find_or_step(
        &mut self,
        term: &str,
        case_sensitive: bool,
        forwards: bool,
    ) -> Result<(), String> {
        if term.is_empty() {
            self.found.clear();
            return self.find_stop();
        }
        if self.finding && self.found == term {
            // The walk itself, inlined rather than given a door of its own: on
            // a page there is no second caller — an ask on the same term *is*
            // the walk, which is what this function's own name says.
            return self.host.find_step(forwards);
        }
        self.find(term, case_sensitive)
    }

    /// End the session and take the page's highlights off.
    pub(crate) fn find_stop(&mut self) -> Result<(), String> {
        if !self.finding {
            return Ok(());
        }
        self.finding = false;
        self.host.find_stop()
    }

    /// Put the keyboard inside the page.
    pub(crate) fn focus_page(&self) -> Result<(), String> {
        self.host.focus_page()
    }

    /// **Whether this seat has been asked to go** (W2 slice 5).
    ///
    /// A controller that is closing has nothing on the glass - `host.close()`
    /// took its visual out of the tree - but the wait for its browser process to
    /// end runs for as long as ten seconds (`w0p-evidence.md` 4.2). The window
    /// asks this so that it stops cutting a hole in its own surface for a page
    /// that is not there: without it, replacing a page with a document leaves a
    /// transparent rectangle over the document until the browser exits, which is
    /// the desktop showing through a pane (found on the machine, W2 slice 5).
    pub(crate) fn is_closing(&self) -> bool {
        self.machine.state() == WebState::Closing
    }

    /// **Everything a card's capture decision is made of, read off the seat**
    /// (W2 slice ⑥).
    ///
    /// Assembled here rather than at the call site because every one of the five
    /// is this type's own state, and a caller that reached in for them one at a
    /// time would be four opportunities to ask a stale question. The *decision*
    /// is not here: it is `web_thumb::WebThumbs::due`, which is a pure function
    /// over these facts and is tested without an engine.
    pub(crate) fn capture_facts(&self) -> crate::web_thumb::SeatFacts {
        crate::web_thumb::SeatFacts {
            // **The engine's own answer and not a re-derivation.** A hidden
            // WebView never completes a capture — measured three times over two
            // re-verification runs — so this is the gate the whole lane stands
            // on, and the only honest source for it is what the controller was
            // last actually told.
            on_glass: matches!(self.presence, Some(WebPresence::Shown(_))),
            closing: self.is_closing(),
            // A seat showing a failure card has no page behind it to picture,
            // and a seat that has never committed a document has nothing on its
            // glass but the blank page this host minted for itself.
            committed: self.machine.recoverable_url().is_some() && self.fault.is_none(),
            capturing: self.capturing,
            size: self.sized,
        }
    }

    /// **Ask the engine for a picture of this page** (W2 slice ⑥).
    ///
    /// The mechanism only. Whether this seat should be asked at all was decided
    /// by [`crate::web_thumb::WebThumbs::due`] out of [`Self::capture_facts`];
    /// what comes back travels as [`WebOutcome::Captured`].
    pub(crate) fn capture_page(&mut self) -> Result<(), String> {
        debug_assert!(
            !self.capturing,
            "the store issues one capture per seat at a time"
        );
        self.capturing = true;
        match self.host.capture_preview() {
            Ok(()) => Ok(()),
            Err(error) => {
                // The engine never took the ask, so no completion is coming and
                // the flag must not be left standing — a seat stuck on "a
                // capture is out" is a seat whose card can never be refreshed
                // again.
                self.capturing = false;
                Err(error)
            }
        }
    }

    /// The seat is going away: close the controller and start the wait.
    pub(crate) fn close(&mut self, compositor: &bt_platform::Compositor) -> Vec<WebOutcome> {
        if self.machine.state() == WebState::Closing {
            return Vec::new();
        }
        let effect = self.machine.close();
        let mut outcomes = Vec::new();
        self.apply(effect, compositor, &mut outcomes);
        outcomes
    }
}

#[cfg(test)]
mod machine_tests {
    use super::*;

    fn boot(preview: &mut WebMachine, url: &str) {
        assert_eq!(preview.request(url), WebEffect::Ignore);
        let generation = preview.generation();
        assert_eq!(
            preview.on_environment(generation, true),
            WebEffect::CreateController
        );
        assert_eq!(
            preview.on_controller(generation, true),
            WebEffect::InstallEvents
        );
        assert_eq!(
            preview.on_events_installed(generation),
            WebEffect::Navigate(url.to_owned())
        );
        assert_eq!(preview.state(), WebState::Ready);
    }

    #[test]
    fn nothing_navigates_before_every_handler_is_on() {
        let mut preview = WebMachine::new();
        preview.request("https://example.com/");
        let generation = preview.generation();
        preview.on_environment(generation, true);
        assert_eq!(
            preview.on_controller(generation, true),
            WebEffect::InstallEvents
        );
        assert_eq!(preview.state(), WebState::ControllerPending);
        assert_eq!(
            preview.on_events_installed(generation),
            WebEffect::Navigate("https://example.com/".into())
        );
    }

    #[test]
    fn a_late_environment_callback_from_a_closed_pane_is_dropped() {
        let mut preview = WebMachine::new();
        preview.request("https://example.com/");
        let stale = preview.generation();
        preview.close();
        assert_eq!(preview.on_environment(stale, true), WebEffect::Ignore);
    }

    #[test]
    fn a_late_controller_from_an_abandoned_generation_is_closed_not_adopted() {
        let mut preview = WebMachine::new();
        preview.request("https://example.com/");
        let stale = preview.generation();
        preview.on_environment(stale, true);
        assert_eq!(
            preview.on_browser_process_failed(),
            WebEffect::RebuildFromScratch
        );
        assert_ne!(preview.generation(), stale);
        assert_eq!(
            preview.on_controller(stale, true),
            WebEffect::CloseOrphanController
        );
        let current = preview.generation();
        assert_eq!(
            preview.on_environment(current, true),
            WebEffect::CreateController
        );
    }

    /// PIN (W2 slice 5) - **a reload is not a navigation**, and it is nothing
    /// at all until the engine is up.
    ///
    /// The plan's sentence for what a saved file does to a page: "网页座位刷新 =
    /// 一次正常 `Reload`(不是重新导航)". A `request` would truncate the page's
    /// own history for an address that has not changed.
    #[test]
    fn a_reload_is_only_owed_by_a_page_that_is_already_up() {
        let mut preview = WebMachine::new();
        assert_eq!(
            preview.reload(),
            WebEffect::Ignore,
            "a seat with no engine has nothing to read again"
        );
        preview.request("https://example.com/page");
        assert_eq!(
            preview.reload(),
            WebEffect::Ignore,
            "and neither has one whose environment is still coming up"
        );
        boot(&mut preview, "https://example.com/page");
        assert_eq!(preview.reload(), WebEffect::Reload);
        assert_eq!(
            preview.recoverable_url(),
            None,
            "and it moves nothing: a reload is not a commit"
        );
        preview.close();
        assert_eq!(
            preview.reload(),
            WebEffect::Ignore,
            "a closing seat is not a seat to reload"
        );
    }

    /// PIN (W2 slice 5) - **every navigation this host issues has been through a
    /// door, and the mint it is issued under is one the host carried rather than
    /// one read off the URL.**
    ///
    /// A source pin, because the alternative is a live browser: the effect this
    /// is about is applied inside `WebSeat::step`, which needs a compositor and a
    /// controller. What a machine can hold is the shape of those eight lines -
    /// that the mint installed is the carried one, matched against the URL being
    /// issued, and that `Origin::HostMinted` is asked before `navigate` is
    /// called.
    ///
    /// RED GATE: derive the mint from the URL instead (`file:` prefix implies
    /// `Mint::File`) and the first needle disappears; drop the `check` and the
    /// second does.
    #[test]
    fn the_host_asks_its_own_gate_before_it_issues_its_own_navigation() {
        let source: String = include_str!("webhost.rs")
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        assert!(
            source.contains(concat!(
                "}elseifself.minted.target()==Some(url.as",
                "_str()){self.minted.clone()"
            )),
            "the mint installed is the one the caller carried, honoured only for \
             the URL it was made for"
        );
        let gate = concat!("check(url,Origin::Host", "Minted(&minted))");
        assert_eq!(
            source.matches(gate).count(),
            1,
            "exactly one host-minted gate"
        );
        let after_gate = &source[source.find(gate).expect("the gate just counted")..];
        let navigate = concat!("self.host.na", "vigate(url)?;");
        assert!(
            after_gate.contains(navigate),
            "and it stands in front of the navigation rather than behind it"
        );
        assert!(
            !source[..source.find(gate).expect("the gate just counted")].contains(navigate),
            "nothing navigates before the gate"
        );
    }

    #[test]
    fn desired_url_is_last_write_wins_and_only_one_navigation_results() {
        let mut preview = WebMachine::new();
        preview.request("https://first.example/");
        let generation = preview.generation();
        preview.request("https://second.example/");
        preview.request("https://third.example/");
        preview.on_environment(generation, true);
        preview.on_controller(generation, true);
        assert_eq!(
            preview.on_events_installed(generation),
            WebEffect::Navigate("https://third.example/".into())
        );
    }

    #[test]
    fn a_failed_page_does_not_overwrite_a_recoverable_url() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        let generation = preview.generation();
        preview.on_navigation_completed(generation, "https://good.example/", true);
        assert_eq!(preview.recoverable_url(), Some("https://good.example/"));
        preview.on_navigation_completed(generation, "https://dead.example/", false);
        assert_eq!(preview.recoverable_url(), Some("https://good.example/"));
        preview.on_navigation_completed(generation, "about:blank", true);
        assert_eq!(preview.recoverable_url(), Some("https://good.example/"));
    }

    #[test]
    fn a_browser_crash_comes_back_to_the_last_good_url() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        let generation = preview.generation();
        preview.on_navigation_completed(generation, "https://good.example/", true);
        preview.request("https://broken.example/");
        preview.on_navigation_completed(generation, "https://broken.example/", false);
        assert_eq!(
            preview.on_browser_process_failed(),
            WebEffect::RebuildFromScratch
        );
        let generation = preview.generation();
        preview.on_environment(generation, true);
        preview.on_controller(generation, true);
        assert_eq!(
            preview.on_events_installed(generation),
            WebEffect::Navigate("https://good.example/".into())
        );
    }

    #[test]
    fn a_renderer_crash_only_reloads() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        let before = preview.generation();
        assert_eq!(preview.on_render_process_failed(), WebEffect::Reload);
        assert_eq!(preview.generation(), before);
        assert_eq!(preview.state(), WebState::Ready);
    }

    #[test]
    fn closing_waits_for_the_browser_to_exit_before_the_udf_is_touched() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        assert_eq!(preview.close(), WebEffect::AwaitBrowserExitBeforeCleanup);
        assert_eq!(preview.state(), WebState::Closing);
        assert_eq!(
            preview.on_browser_process_exited(),
            WebEffect::ReleaseUserDataFolder
        );
    }

    /// W0′ revision ①. Eight measured shutdowns: six said `BrowserProcessExited`
    /// in 271–390 ms, one took 6 588 ms, and one never said anything at all
    /// (`w0p-evidence.md` §4.2). And on the graceful path `ProcessFailed` does
    /// **not** come — all eight read `process_failed_browser_exited: false` — so
    /// the second door is shut there and the deadline is the only backstop.
    #[test]
    fn a_browser_that_never_says_it_exited_is_cleaned_up_when_the_wait_runs_out() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        assert_eq!(preview.close(), WebEffect::AwaitBrowserExitBeforeCleanup);
        assert_eq!(
            preview.on_cleanup_deadline(),
            WebEffect::ReleaseUserDataFolder
        );
    }

    #[test]
    fn process_failed_is_the_second_door_to_cleanup() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        preview.close();
        assert_eq!(
            preview.on_browser_process_failed(),
            WebEffect::ReleaseUserDataFolder
        );
    }

    #[test]
    fn the_user_data_folder_is_cleaned_exactly_once() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        preview.close();
        assert_eq!(
            preview.on_browser_process_exited(),
            WebEffect::ReleaseUserDataFolder
        );
        assert_eq!(preview.on_browser_process_failed(), WebEffect::Ignore);
        assert_eq!(preview.on_cleanup_deadline(), WebEffect::Ignore);
        assert_eq!(preview.on_browser_process_exited(), WebEffect::Ignore);
    }

    /// W0′ revision ②. The same `ProcessFailed` means "rebuild" under a live
    /// pane and "the folder is yours" under a closing one, and only the state
    /// tells them apart. The plan's §4 as written rebuilt on both — which brings
    /// back a pane the person shut.
    #[test]
    fn a_dying_browser_does_not_resurrect_a_closed_pane() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        preview.close();
        let closed_at = preview.generation();
        assert_ne!(
            preview.on_browser_process_failed(),
            WebEffect::RebuildFromScratch
        );
        assert_eq!(preview.state(), WebState::Closing);
        assert_eq!(preview.generation(), closed_at);
    }

    #[test]
    fn a_new_runtime_version_rebuilds_the_seat_at_the_last_good_url() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        let generation = preview.generation();
        preview.on_navigation_completed(generation, "https://good.example/", true);
        assert_eq!(
            preview.on_new_browser_version_available(),
            WebEffect::RebuildForNewVersion
        );
        assert_ne!(preview.generation(), generation);
        let current = preview.generation();
        preview.on_environment(current, true);
        preview.on_controller(current, true);
        assert_eq!(
            preview.on_events_installed(current),
            WebEffect::Navigate("https://good.example/".into())
        );
    }

    #[test]
    fn a_controller_from_before_the_version_change_is_closed_not_adopted() {
        let mut preview = WebMachine::new();
        preview.request("https://good.example/");
        let stale = preview.generation();
        preview.on_environment(stale, true);
        assert_eq!(
            preview.on_new_browser_version_available(),
            WebEffect::RebuildForNewVersion
        );
        assert_eq!(
            preview.on_controller(stale, true),
            WebEffect::CloseOrphanController
        );
    }

    #[test]
    fn a_version_change_during_close_is_nobodys_business() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        preview.close();
        assert_eq!(
            preview.on_new_browser_version_available(),
            WebEffect::Ignore
        );
        assert_eq!(preview.state(), WebState::Closing);
    }

    #[test]
    fn a_navigation_completed_from_a_stale_generation_cannot_write_the_recoverable_url() {
        let mut preview = WebMachine::new();
        boot(&mut preview, "https://good.example/");
        let stale = preview.generation();
        preview.on_browser_process_failed();
        preview.on_navigation_completed(stale, "https://attacker.example/", true);
        assert_eq!(preview.recoverable_url(), None);
    }
}

#[cfg(test)]
mod keyboard_tests {
    use super::*;
    use crate::shortcuts::{BINDINGS, Chord, ChordKey, Focus, Shortcuts};
    use winit::keyboard::{ModifiersState, NamedKey};

    /// Bare `Alt`. Named here and not beside the table above because no code
    /// path in this window reaches it — the point of the row below is precisely
    /// that this key is the page's.
    const VK_MENU: u16 = 0x12;

    /// Every chord the shipped table carries, spelled the way a person presses
    /// it — the product-side twin of the transcription the W0′ probe fired at a
    /// focused page and got 30/30 back from (`w0p-evidence.md` §2.1).
    ///
    /// **If `BINDINGS` changes this list must change with it**, which is the
    /// whole reason it is written out: the reconciliation is only worth anything
    /// while somebody is forced to look at it.
    const EXPECTED_CHORDS: &[(&str, &str)] = &[
        ("new-tab", "Ctrl+Shift+n"),
        ("new-window", "Ctrl+Shift+m"),
        // **One arrived on 2026-08-23** (multiwindow slice E2): the whole
        // application leaving. It is claimed back off a focused page like every
        // other window verb — a page that swallowed it would be a page a reader
        // cannot quit out of.
        ("quit", "Ctrl+Shift+q"),
        ("close-pane", "Ctrl+Shift+w"),
        ("next-tab", "Ctrl+Tab"),
        ("prev-tab", "Ctrl+Shift+Tab"),
        ("goto-tab-1", "Ctrl+Shift+1"),
        ("goto-tab-2", "Ctrl+Shift+2"),
        ("goto-tab-3", "Ctrl+Shift+3"),
        ("goto-tab-4", "Ctrl+Shift+4"),
        ("goto-tab-5", "Ctrl+Shift+5"),
        ("goto-tab-6", "Ctrl+Shift+6"),
        ("goto-tab-7", "Ctrl+Shift+7"),
        ("goto-tab-8", "Ctrl+Shift+8"),
        ("goto-tab-9", "Ctrl+Shift+9"),
        ("reopen-closed", "Ctrl+Shift+t"),
        ("jump-attention", "Ctrl+Shift+a"),
        ("command-palette", "Ctrl+Shift+p"),
        ("focus-mode", "Ctrl+Shift+z"),
        ("split-horizontal", "Alt+Shift+-"),
        ("split-vertical", "Alt+Shift+="),
        ("duplicate-pane-split", "Ctrl+Shift+d"),
        ("files-pane", "Ctrl+Shift+b"),
        ("git-page", "Ctrl+Shift+g"),
        ("open-settings", "Ctrl+,"),
        ("save-preview", "Ctrl+s"),
        ("prev-command-mark", "Ctrl+Shift+ArrowUp"),
        ("next-command-mark", "Ctrl+Shift+ArrowDown"),
        ("open-search", "Ctrl+f"),
        ("next-match", "F3"),
        ("prev-match", "Shift+F3"),
        // **Three arrived on 2026-08-22** (§7.7, W2 slice ④): the capsule's own
        // Escape, and the two rows the user ruled in for a page's address field
        // and its developer tools.
        ("close-search", "Escape"),
        ("web-address", "Ctrl+l"),
        ("web-devtools", "F12"),
    ];

    fn spell(chord: &Chord) -> String {
        let mut out = String::new();
        if chord.modifiers.contains(ModifiersState::CONTROL) {
            out.push_str("Ctrl+");
        }
        if chord.modifiers.contains(ModifiersState::ALT) {
            out.push_str("Alt+");
        }
        if chord.modifiers.contains(ModifiersState::SHIFT) {
            out.push_str("Shift+");
        }
        match &chord.key {
            ChordKey::Character(text) => out.push_str(text),
            ChordKey::Named(named) => out.push_str(&format!("{named:?}")),
        }
        out
    }

    /// RED — the reconciliation. The table the window dispatches on and the list
    /// the web host takes back from a focused page are the same rows.
    #[test]
    fn the_chord_table_the_web_seat_claims_is_the_table_the_window_ships() {
        let spelled: Vec<(&str, String)> = BINDINGS
            .iter()
            .filter_map(|row| row.chord.as_ref().map(|chord| (row.id, spell(chord))))
            .collect();
        let expected: Vec<(&str, String)> = EXPECTED_CHORDS
            .iter()
            .map(|(id, chord)| (*id, (*chord).to_owned()))
            .collect();
        assert_eq!(spelled, expected);
        assert_eq!(spelled.len(), 34);
    }

    /// RED — and every one of them reaches a virtual key, because
    /// `AcceleratorKeyPressed` speaks Win32 and nothing else.
    #[test]
    fn every_shipped_chord_resolves_to_a_virtual_key_on_this_layout() {
        let claims = claimable_chords(&Shortcuts::defaults(), every_focus());
        assert_eq!(
            claims.len(),
            34,
            "a chord this window owns that the web host cannot name in Win32 is \
             a chord that silently stops working while a page has the focus"
        );
        for claim in &claims {
            assert!(claim.chord.virtual_key != 0, "{:?}", claim.action);
        }
    }

    /// RED — W0′'s accidental finding, written down where the next person to add
    /// a row will trip over it (`w0p-evidence.md` §2.4).
    ///
    /// A bare printable key never enters `AcceleratorKeyPressed` at all: the
    /// probe pressed `K` with the page focused, the page received it and the
    /// callback did not fire once. So a bare letter in `BINDINGS` would be a
    /// shortcut that works everywhere except over a web seat, silently. The
    /// table has none today; this test is what says so tomorrow.
    #[test]
    fn no_shipped_chord_is_a_bare_printable_key() {
        for row in BINDINGS {
            let Some(chord) = row.chord.as_ref() else {
                continue;
            };
            let bare = chord.modifiers.is_empty();
            let printable = matches!(&chord.key, ChordKey::Character(_));
            assert!(
                !(bare && printable),
                "{}: a bare printable key never reaches AcceleratorKeyPressed, so \
                 this row would never fire while a page has the focus",
                row.id
            );
        }
    }

    /// RED — the other half of the matrix (`w0p-evidence.md` §2.2): the keys the
    /// page needs are the keys this window does not claim.
    #[test]
    fn the_page_keeps_every_key_the_window_does_not_claim() {
        let claims = claimable_chords(&Shortcuts::defaults(), every_focus());
        let ctrl = |vk: u16| claims_chord(&claims, vk, true, false, false);
        for letter in ['C', 'V', 'X', 'A', 'Z', 'Y', 'R', 'P'] {
            assert!(
                !ctrl(letter as u16),
                "Ctrl+{letter} belongs to the page (clipboard, undo, reload, print)"
            );
        }
        // F5 is still the page's: reload has a button of its own on the head,
        // and the key the engine already answers with the same verb is not one
        // this table has any reason to take.
        assert!(!claims_chord(&claims, VK_F5, false, false, false));
        // **F12 is the window's since 2026-08-22** (user ruling). It is a door
        // that did not otherwise exist from the keyboard — the developer-tools
        // tool is invisible until the pointer arrives — and the verb behind it
        // is this window's `web-devtools` row.
        assert!(claims_chord(&claims, VK_F12, false, false, false));
        // And `Ctrl+L`, which is the address field's second door.
        assert!(claims_chord(&claims, b'L' as u16, true, false, false));
        // Bare Alt walks in as a system key and walks straight out to the page.
        assert!(!claims_chord(&claims, VK_MENU, false, false, true));
        // The control row: this one is the window's, and the page must not see it.
        assert!(claims_chord(&claims, b'Z' as u16, true, true, false));
    }

    /// RED — `Alt+Left` / `Alt+Right` are the engine's back and forward.
    ///
    /// Measured: the callback sees them, the host does not claim them, and the
    /// page never receives them — the engine eats them itself and navigates.
    /// Slice ① leaves that as it stands; taking them back is a product ruling
    /// slice ④ makes, and it would be made *here*, by adding two rows to the
    /// claim, which is why this test names the fact rather than the code.
    #[test]
    fn alt_left_and_alt_right_stay_with_the_engine() {
        let claims = claimable_chords(&Shortcuts::defaults(), every_focus());
        assert!(!claims_chord(&claims, VK_LEFT, false, false, true));
        assert!(!claims_chord(&claims, VK_RIGHT, false, false, true));
        // **And the ruling that kept them there** (§7.7, W2 slice ④,
        // 2026-08-22). Not a measurement this time but a decision, and this is
        // the nail in it: no row of the shipped table may claim those two
        // chords under *any* focus the window can be in, so taking them back is
        // a change somebody makes to the ruling and never one that arrives as a
        // side effect of adding a row.
        for row in BINDINGS {
            let Some(chord) = row.chord.as_ref() else {
                continue;
            };
            let alt_only = chord.modifiers == ModifiersState::ALT;
            let arrow = matches!(
                &chord.key,
                ChordKey::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight)
            );
            assert!(
                !(alt_only && arrow),
                "{}: Alt+Left and Alt+Right are the engine's back and forward (DESIGN §7.7 W2 ④); claiming one is a ruling, not a row",
                row.id
            );
        }
    }

    /// PIN (§7.7 ②, W2 slice ④) — **`Ctrl+F` is claimed over a page, and that
    /// is what keeps the engine's own find bar out of the seat.**
    ///
    /// This build's bindings do not carry `AreBrowserAcceleratorKeysEnabled`, so
    /// a key this table does not take is a key the engine keeps — and the key
    /// the engine keeps here opens a second search box inside a window whose
    /// whole search story is that there is one.
    ///
    /// MUTATION: put `open-search` back on `Scope::TerminalPrimary` and this
    /// goes red, which is the second host losing its capsule.
    #[test]
    fn the_page_gives_the_search_chord_back_to_the_window() {
        let on_a_page = Focus {
            preview: true,
            terminal_primary: false,
            search_open: false,
            web_page: true,
        };
        let claims = claimable_chords(&Shortcuts::defaults(), on_a_page);
        assert!(claims_chord(&claims, b'F' as u16, true, false, false));
        // And Escape is *not* claimed until there is a capsule to put away: a
        // page owns every key this table does not, and with nothing open there
        // is nothing for the window to do with it.
        assert!(!claims_chord(&claims, VK_ESCAPE, false, false, false));
        let searching = Focus {
            search_open: true,
            ..on_a_page
        };
        let claims = claimable_chords(&Shortcuts::defaults(), searching);
        assert!(claims_chord(&claims, VK_ESCAPE, false, false, false));
    }

    /// RED — a row out of scope is not claimed, because a key the window will
    /// not act on must not be taken away from the page.
    #[test]
    fn a_row_out_of_scope_is_left_to_the_page() {
        let nothing_focused = Focus::default();
        let claims = claimable_chords(&Shortcuts::defaults(), nothing_focused);
        // `save-preview` is Ctrl+S and is scoped to a focused preview seat.
        assert!(!claims_chord(&claims, b'S' as u16, true, false, false));
        let on_a_preview = Focus {
            preview: true,
            ..Focus::default()
        };
        let claims = claimable_chords(&Shortcuts::defaults(), on_a_preview);
        assert!(claims_chord(&claims, b'S' as u16, true, false, false));
    }

    #[test]
    fn the_named_keys_the_table_uses_all_have_a_virtual_key() {
        for named in [
            NamedKey::Tab,
            NamedKey::ArrowUp,
            NamedKey::ArrowDown,
            NamedKey::F3,
        ] {
            assert!(named_key_virtual_key(named).is_some(), "{named:?}");
        }
    }

    fn every_focus() -> Focus {
        Focus {
            preview: true,
            terminal_primary: true,
            search_open: true,
            web_page: true,
        }
    }
}

// Slice ② landed, and with it the stub these tests were written against.
// They pinned a placeholder that admitted nothing but the two targets this
// build mints for itself; the rule that replaced it is the one the address
// bar asks, so a page following a link to another http(s) site is ordinary
// browsing rather than a refusal. What each of them said is said again, of
// the real rule, by `webnav`’s own twenty-five contract tests — the mint
// pair by `about_blank_passes_only_through_the_mint` and
// `a_mint_admits_one_target_and_no_relatives`, the file door by
// `only_the_minted_file_url_passes`. Nothing moved to a second home.

#[cfg(test)]
mod folder_tests {
    use super::*;
    use std::path::Path;

    /// RED — `%LOCALAPPDATA%`, never `%APPDATA%`: the folder holds a cache, a
    /// cookie jar and a crash-dump directory, none of which may roam
    /// (`plan.md` §0).
    #[test]
    fn the_user_data_folder_is_the_products_own_under_local_appdata() {
        let folder = user_data_folder_in(Path::new(r"C:\Users\x\AppData\Local"));
        assert_eq!(
            folder,
            Path::new(r"C:\Users\x\AppData\Local\Folio\WebView2")
        );
    }
}

/// **Where a moved seat comes back** (F1a, `plan.md` v3 增补).
///
/// The narrow contract's whole reason: `WebSeat` caches its own seat key and
/// HWND, so a transfer that moved only the window's map entry would leave the
/// *next* controller — the one a browser crash asks for — being built on the
/// window this page has left. None of these needs a browser, because the fact
/// under test is which address the seat names.
#[cfg(test)]
mod rehost_address_tests {
    use super::*;

    fn hwnd(value: isize) -> std::num::NonZeroIsize {
        std::num::NonZeroIsize::new(value).expect("a non-zero window handle")
    }

    fn page(tab: u64, seat: u64) -> bt_platform::PageVisual {
        bt_platform::PageVisual { tab, seat }
    }

    /// A seat with an address, a machine and an engine that has never been
    /// started. **No environment is requested**, which is what keeps these
    /// tests browserless.
    fn detached(address: SeatAddress) -> WebSeat {
        WebSeat {
            address,
            folder: PathBuf::from(r"C:\nowhere"),
            machine: WebMachine::new(),
            host: bt_platform::WebHost::new(
                Box::new(|_| bt_platform::WebNavigationVerdict::Proceed),
                Box::new(|| {}),
            ),
            mint: Rc::new(RefCell::new(Mint::Nothing)),
            claims: Vec::new(),
            waiting: None,
            wanted: WebPresence::Hidden,
            presence: None,
            wanted_size: None,
            sized: None,
            buttons: bt_platform::web_mouse_buttons::NONE,
            last_left_press: None,
            minted: Mint::Nothing,
            page: PageFacts::default(),
            finding: false,
            found: String::new(),
            fault: None,
            refusal: Rc::new(RefCell::new(None)),
            capturing: false,
            zoom: 1.0,
        }
    }

    /// PIN (F1b′) — **two panes of one window that both call themselves seat 1
    /// are two addresses.**
    ///
    /// The F1a self-check, made into an assertion. [`SeatAddress`] is a
    /// platform-layer address and its one job is to name this page's visual in
    /// the window it stands in; a seat number cannot do that job, because every
    /// tab a pane is torn out into numbers its seats from one again. Two tabs of
    /// one window would then hold one address between them, and the four
    /// operations it exists to keep in agreement — attach, install, place,
    /// detach — would agree with each other about the wrong page.
    ///
    /// MUTATION: drop the tab from [`bt_platform::PageVisual`] and the two
    /// addresses below are equal, which is `rehost` reporting `AddressOnly` for a
    /// move that has to happen and a live page composing into another tab's box.
    #[test]
    fn one_window_two_tabs_one_seat_number_is_two_addresses() {
        let same_window = hwnd(0x1111);
        let first = SeatAddress {
            page: page(1, 1),
            hwnd: same_window,
        };
        let second = SeatAddress {
            page: page(2, 1),
            hwnd: same_window,
        };
        assert_ne!(
            first, second,
            "the window is the same and the seat number is the same, so the tab \
             is the only thing that can tell the two pages apart"
        );
        assert_eq!(detached(second).address(), second);
    }

    /// RED — **a seat that moved window rebuilds in the window it moved to.**
    ///
    /// The acceptance gate `plan.md` v3 增补 F1a names in one line: 「红测 = 迁移
    /// 后注入 browser process failure,页面在**目标**窗原位重建」. The rebuild asks
    /// for a controller on [`WebSeat::address`] and attaches to its seat key, so
    /// this is the fact the whole contract rests on.
    ///
    /// MUTATIONS:
    /// ① let `rehost` move the window's map entry and not this cache — the
    ///    second assertion goes red and the page rebuilds in the old window;
    /// ② move only the HWND and leave the seat key — the rebuild would build a
    ///    controller on the right window pointed at a visual in the wrong one.
    #[test]
    fn a_rehosted_seat_rebuilds_in_the_window_it_moved_to() {
        let source = SeatAddress {
            page: page(1, 3),
            hwnd: hwnd(0x1111),
        };
        let target = SeatAddress {
            page: page(4, 9),
            hwnd: hwnd(0x2222),
        };
        let mut seat = detached(source);
        seat.machine.request("https://example.com/");
        let generation = seat.machine.generation();
        seat.machine.on_environment(generation, true);
        seat.machine.on_controller(generation, true);
        seat.machine.on_events_installed(generation);
        seat.machine
            .on_navigation_completed(generation, "https://example.com/", true);
        assert_eq!(seat.address(), source);

        seat.take_address(target);

        // The browser dies under the moved seat.
        assert_eq!(
            seat.machine.on_browser_process_failed(),
            WebEffect::RebuildFromScratch
        );
        assert_eq!(
            seat.address(),
            target,
            "the rebuild is asked for on the window the seat moved to"
        );
        assert_eq!(seat.machine.recoverable_url(), Some("https://example.com/"));
    }

    /// RED — and a compensation that could not run leaves the seat rebuilding
    /// **in the target**, not in the window it half left.
    ///
    /// The lossy branch is allowed to lose the document; it is not allowed to
    /// lose the window.
    #[test]
    fn a_handoff_that_could_not_be_undone_rebuilds_in_the_target_window() {
        let target = SeatAddress {
            page: page(4, 9),
            hwnd: hwnd(0x2222),
        };
        let mut seat = detached(SeatAddress {
            page: page(1, 3),
            hwnd: hwnd(0x1111),
        });
        seat.machine.request("https://example.com/");
        let generation = seat.machine.generation();
        seat.machine.on_environment(generation, true);
        seat.machine.on_controller(generation, true);
        seat.machine.on_events_installed(generation);
        seat.machine
            .on_navigation_completed(generation, "https://example.com/", true);

        seat.take_address(target);
        assert_eq!(
            seat.machine.on_rehost_lost(),
            WebEffect::RebuildForNewVersion
        );
        assert_eq!(seat.address(), target);
        assert_eq!(seat.machine.recoverable_url(), Some("https://example.com/"));
    }

    /// RED — **the new window is told the rectangle again.**
    ///
    /// `apply_presence` says nothing when what it wants is what it last said,
    /// and what it last said was said to an engine in another window. A seat
    /// that carried those two caches across would sit at the old window's
    /// rectangle, correct only after the next resize.
    #[test]
    fn a_moved_seat_forgets_what_the_old_window_was_told() {
        let mut seat = detached(SeatAddress {
            page: page(1, 3),
            hwnd: hwnd(0x1111),
        });
        seat.wanted_size = Some((800, 600));
        seat.sized = Some((800, 600));
        seat.presence = Some(WebPresence::Shown(WebBounds {
            x: 10,
            y: 20,
            width: 800,
            height: 600,
        }));

        seat.take_address(SeatAddress {
            page: page(4, 9),
            hwnd: hwnd(0x2222),
        });

        assert_eq!(seat.sized, None, "the new engine has been told no size");
        assert_eq!(
            seat.presence, None,
            "the new engine has been told no presence"
        );
        assert_eq!(
            seat.wanted_size,
            Some((800, 600)),
            "what the window wants is unchanged; only what was said is forgotten"
        );
    }

    /// RED — **the window being left gets a page with nothing pressed.**
    ///
    /// A page torn out mid-drag believes a button is down and the pointer is
    /// inside it. Both are facts about a window it is leaving, and a page that
    /// arrived in a new window still holding them would be a page whose next
    /// click is a drag-select from wherever the last one started.
    #[test]
    fn the_old_window_is_paid_its_buttons_before_the_parent_changes() {
        let mut seat = detached(SeatAddress {
            page: page(1, 3),
            hwnd: hwnd(0x1111),
        });
        seat.buttons = bt_platform::web_mouse_buttons::LEFT | bt_platform::web_mouse_buttons::X1;
        seat.last_left_press = Some((Instant::now(), (40, 40)));

        seat.settle_input_for_handoff();

        assert_eq!(seat.buttons, bt_platform::web_mouse_buttons::NONE);
        assert_eq!(seat.last_left_press, None);
    }
}

#[cfg(test)]
mod presence_tests {
    use super::*;

    /// RED — a seat with no rectangle has no page on it.
    ///
    /// Which is the same door three separate facts come through: the tab was
    /// switched away (the seat is not in the active tab's layout at all), the
    /// fit ladder dropped the seat, or focus mode parked it off stage. All three
    /// end as `device_rect: None`, and all three mean the same thing to a
    /// WebView — hide it, because a hidden WebView stops its timers and its
    /// requestAnimationFrame entirely (`w0p-evidence.md` §1 gate 8: 1 811 ms of
    /// CPU and 718 frames visible, 0 and 0 hidden).
    #[test]
    fn a_seat_with_no_rectangle_hides_the_page() {
        assert_eq!(web_presence(None, false), WebPresence::Hidden);
    }

    /// RED — and so does a modal, because the scrim is painted over the whole
    /// window by wgpu and the page is underneath wgpu. A hole punched through a
    /// scrim is a page the reader can see through a dimmed window.
    #[test]
    fn a_modal_hides_the_page_rather_than_being_shown_through() {
        let body = [10.0, 20.0, 210.0, 120.0];
        assert_eq!(web_presence(Some(body), true), WebPresence::Hidden);
    }

    /// RED — and the rectangle it is shown at is the seat's body in physical
    /// pixels, rounded outwards so no hairline of window shows at the seam.
    #[test]
    fn a_presented_seat_shows_the_page_at_its_body_rectangle() {
        let body = [10.4, 20.6, 210.2, 120.9];
        assert_eq!(
            web_presence(Some(body), false),
            WebPresence::Shown(WebBounds {
                x: 10,
                y: 20,
                width: 201,
                height: 101,
            })
        );
    }

    /// RED — a body that has collapsed to nothing is not a one-pixel page.
    #[test]
    fn a_body_with_no_area_hides_the_page() {
        assert_eq!(
            web_presence(Some([10.0, 20.0, 10.0, 120.0]), false),
            WebPresence::Hidden
        );
        assert_eq!(
            web_presence(Some([10.0, 20.0, 210.0, 20.0]), false),
            WebPresence::Hidden
        );
    }
}

/// **The five failure cards, and the rules that pick one** (§7.7 ④, W2 slice ④).
///
/// Every reason a card carries comes from a machine — the loader, the engine's
/// `WebErrorStatus`, the navigation gate's own [`Refusal`], the address door's
/// answer about a download's URL — and these are the tests of the picking rather
/// than of the drawing. Not one of them needs a browser.
#[cfg(test)]
mod fault_tests {
    use super::*;
    use crate::i18n::Text;

    /// The five, so that a sixth cannot arrive without somebody writing its
    /// sentence here.
    fn every_fault() -> Vec<WebFault> {
        vec![
            WebFault::RuntimeMissing {
                detail: "CreateCoreWebView2Environment failed (0x80070002)".to_owned(),
            },
            WebFault::DidNotLoad {
                host: "127.0.0.1".to_owned(),
                detail: "WebErrorStatus · CannotConnect".to_owned(),
            },
            WebFault::RenderProcessGone,
            WebFault::Blocked {
                url: "mailto:someone@example.com".to_owned(),
                refusal: Refusal::ExternalScheme,
            },
            WebFault::DownloadRefused {
                file_name: "report.pdf".to_owned(),
            },
        ]
    }

    /// PIN (§7.7 ④) — **one sentence, at most one fact, exactly one verb.**
    ///
    /// 「一图五行、一句话一事实一动词、无旁白」. The shape is the ruling: a row
    /// of buttons is the program handing its own decision back to the reader,
    /// and a second sentence of prose under the first is the aside the ruling
    /// forbids.
    ///
    /// MUTATIONS:
    /// ① give any card a second verb — there is nowhere to put it, which is the
    ///    point of `verb()` being one value;
    /// ② let a detail carry a sentence — the assertion on the full stop goes red.
    #[test]
    fn every_failure_says_one_sentence_one_fact_and_one_verb() {
        for fault in every_fault() {
            let say = fault.say();
            assert!(!say.trim().is_empty(), "{fault:?} says nothing");
            assert!(
                say.ends_with('.') || say.ends_with('。'),
                "{fault:?}'s sentence is a sentence: {say:?}"
            );
            let verb = fault.verb_text().text();
            assert!(!verb.trim().is_empty(), "{fault:?} offers no way out");
            // A fact is a thing you can copy into a bug report, never a second
            // sentence: no card's detail ends in a full stop.
            if let Some(detail) = fault.detail() {
                assert!(
                    !detail.ends_with('.') && !detail.ends_with('。'),
                    "{fault:?}'s fact reads as prose: {detail:?}"
                );
            }
        }
        // The crash is the one with no fact at all, and that is the mock-up's
        // own answer: a renderer's exit hands over no code a reader could act on.
        assert_eq!(WebFault::RenderProcessGone.detail(), None);
        assert_eq!(
            WebFault::RenderProcessGone.verb_text(),
            Text::PreviewWebReload,
            "and the way out is the button the head already carries"
        );
    }

    /// PIN (§7.7 ④, W2 slice ④) — **one of the five stands over a page and four
    /// replace one.**
    ///
    /// The download is the one that keeps what is behind it, because what was
    /// cancelled is the download and the page is still standing where the reader
    /// left it. The other four have nothing behind them to keep: take one away
    /// and what is left is the black hole a hidden WebView draws
    /// (`w0-evidence.md` §2⑨), which is why they have no Escape.
    ///
    /// MUTATION: make `stands_over_the_page` answer `true` for the crash — the
    /// seat stops hiding its page (`Runtime::sync_web_page`), the hole is
    /// punched over the card, and the card is invisible.
    #[test]
    fn only_the_download_stands_over_a_page() {
        for fault in every_fault() {
            let expected = matches!(fault, WebFault::DownloadRefused { .. });
            assert_eq!(fault.stands_over_the_page(), expected, "{fault:?}");
        }
    }

    /// PIN (§7.8 ③) — **the blocked card names the scheme the door named, and
    /// through the door's own parser.**
    ///
    /// `webnav::scheme_of` is the one reader; 「不另起第二种解析」. An address
    /// that carries no scheme gets the sentence that names none rather than a
    /// sentence with a hole in it.
    #[test]
    fn the_blocked_card_names_the_scheme_the_door_named() {
        let blocked = |url: &str| {
            WebFault::Blocked {
                url: url.to_owned(),
                refusal: Refusal::ExternalScheme,
            }
            .say()
        };
        assert!(blocked("mailto:a@b.c").starts_with("mailto:"));
        assert!(blocked("javascript:alert(1)").starts_with("javascript:"));
        assert_eq!(
            blocked("not an address"),
            Text::WebFailBlockedSay.text(),
            "an address with no scheme gets the sentence that names none"
        );
        // The address itself is the fact under the sentence, in full — that is
        // what the `Copy address` verb is for.
        assert_eq!(
            WebFault::Blocked {
                url: "mailto:a@b.c".to_owned(),
                refusal: Refusal::ExternalScheme,
            }
            .detail(),
            Some("mailto:a@b.c")
        );
    }

    /// PIN (§7.7 ④) — **a refused navigation is not a page that did not load.**
    ///
    /// Both complete with `IsSuccess == false`. One is the policy working and
    /// already has a card of its own; the other is the network. Without the
    /// distinction, every `· blocked` in the foot would also raise a
    /// 「did not respond」 over the seat.
    ///
    /// MUTATION: drop the `OPERATION_CANCELED` arm and the first assertion goes
    /// red — which is exactly the double card the ruling forbids.
    #[test]
    fn a_cancelled_navigation_is_not_a_page_that_did_not_load() {
        assert_eq!(load_fault("http://127.0.0.1:9134/x", false, 14), None);
        assert_eq!(load_fault("http://127.0.0.1:9134/x", true, 0), None);
        let fault = load_fault("http://127.0.0.1:9134/x", false, 12).expect("a card");
        assert_eq!(
            fault,
            WebFault::DidNotLoad {
                host: "127.0.0.1".to_owned(),
                detail: "WebErrorStatus · CannotConnect".to_owned(),
            }
        );
        // The sentence names the host that was asked and nothing else: the URL
        // is on the head, in full, three centimetres above this card.
        assert!(fault.say().starts_with("127.0.0.1"));
    }

    /// PIN (方案 §0) — **a download is handed over exactly when the address door
    /// would take its URL.**
    ///
    /// 「取消并外开可重放的 GET URL,不可重放者提示无法下载」, and what
    /// 「可重放」 means is that door's answer rather than a guess: a `blob:` or a
    /// `data:` URL names memory inside a page rather than a request anybody else
    /// can make, and those are the ones it already refuses.
    ///
    /// MUTATION: accept every scheme and a `blob:` download is handed to the
    /// machine's browser, which opens nothing at all.
    #[test]
    fn a_download_is_handed_over_when_a_plain_link_could_replay_it() {
        assert_eq!(
            download_answer("http://127.0.0.1:9134/report.pdf", r"C:\Users\a\report.pdf"),
            Ok("http://127.0.0.1:9134/report.pdf".to_owned())
        );
        // The card names the file and not the path it would have been written
        // to: a reader is looking for the thing they asked for.
        assert_eq!(
            download_answer("blob:http://127.0.0.1/9f2", r"C:\Users\a\report.pdf"),
            Err(WebFault::DownloadRefused {
                file_name: "report.pdf".to_owned(),
            })
        );
        assert!(matches!(
            download_answer("data:text/csv,a%2Cb", "table.csv"),
            Err(WebFault::DownloadRefused { .. })
        ));
    }

    /// PIN (方案 §0's five extras) — **`Ctrl`+wheel walks a ladder, and the
    /// ladder has two ends.**
    ///
    /// A ladder and not a multiplier: a multiplier has no bottom and no top and
    /// never lands on a round number, and `1.0` has to be a rung so that the way
    /// back to unzoomed is a detent rather than an aim.
    #[test]
    fn the_zoom_ladder_steps_and_stops() {
        assert!((zoom_step(1.0, true) - 1.10).abs() < 1e-9);
        assert!((zoom_step(1.0, false) - 0.90).abs() < 1e-9);
        // Both ends hold.
        assert!((zoom_step(3.0, true) - 3.0).abs() < 1e-9);
        assert!((zoom_step(0.25, false) - 0.25).abs() < 1e-9);
        // A factor that is not on the ladder joins it at the nearest rung rather
        // than stepping off a value the ladder does not have.
        assert!((zoom_step(1.03, true) - 1.10).abs() < 1e-9);
        assert!((zoom_step(1.03, false) - 0.90).abs() < 1e-9);
        // And a walk out and back lands exactly where it started, which is the
        // whole reason the rungs are named numbers.
        let mut factor = 1.0;
        for _ in 0..4 {
            factor = zoom_step(factor, true);
        }
        for _ in 0..4 {
            factor = zoom_step(factor, false);
        }
        assert!((factor - 1.0).abs() < 1e-9);
    }

    /// PIN — **every refusal the door can give has a card that can say it.**
    ///
    /// `webnav::Refusal` is ten variants and the blocked card is what a reader
    /// sees for any of them; this is the sweep that keeps an eleventh from
    /// arriving with nothing to draw.
    #[test]
    fn every_refusal_can_be_drawn() {
        for refusal in [
            Refusal::ScriptOrInlineScheme,
            Refusal::FileScheme,
            Refusal::BrowserInternalScheme,
            Refusal::ExternalScheme,
            Refusal::UserInfo,
            Refusal::NetworkPath,
            Refusal::ControlOrWhitespace,
            Refusal::NoHost,
            Refusal::NotMinted,
            Refusal::Empty,
        ] {
            let fault = WebFault::Blocked {
                url: "ftp://example.com/x".to_owned(),
                refusal,
            };
            assert!(!fault.say().is_empty(), "{refusal:?}");
            assert!(matches!(fault.verb(), WebFaultVerb::CopyAddress(_)));
            assert!(!fault.stands_over_the_page());
        }
    }
}

/// **Where a non-address goes** (§7.7 ②, 方案 §0's five extras).
#[cfg(test)]
mod search_tests {
    use super::*;

    /// PIN — **a non-address becomes an address, and the address goes back
    /// through the door.**
    ///
    /// `webnav::Decision::Search` hands over the *intent*; what this slice owns
    /// is the URL, the encoding and which engine. The composed URL gets no more
    /// trust for having been built here than a typed one does — 「钉不是授权」
    /// said about the one URL this window writes itself — so the test asserts
    /// the address door takes it, not merely that the string looks right.
    ///
    /// MUTATION: skip the second `address_bar` and the rule stops being 「所有
    /// 顶层导航都过门」 for exactly one caller.
    #[test]
    fn a_search_is_composed_into_an_address_the_door_takes() {
        for engine in [
            SearchEngineV1::DuckDuckGo,
            SearchEngineV1::Bing,
            SearchEngineV1::Google,
        ] {
            let url = search_url(engine, "ripgrep");
            assert!(
                url.starts_with("https://"),
                "{engine:?} composes an https address: {url}"
            );
            assert!(matches!(
                crate::webnav::address_bar(&url),
                crate::webnav::Decision::Navigate(_)
            ));
        }
        // Three engines, three addresses — a name in the file and a constant in
        // this build, never a template a person can put anything into.
        assert_eq!(
            search_url(SearchEngineV1::DuckDuckGo, "ripgrep"),
            "https://duckduckgo.com/?q=ripgrep"
        );
        assert_eq!(
            search_url(SearchEngineV1::Bing, "ripgrep"),
            "https://www.bing.com/search?q=ripgrep"
        );
        assert_eq!(
            search_url(SearchEngineV1::Google, "ripgrep"),
            "https://www.google.com/search?q=ripgrep"
        );
    }

    /// PIN — **form encoding, because a query string is a form.**
    ///
    /// A space is `+` and everything outside the unreserved set is
    /// percent-encoded, which is what every search box on the web has sent since
    /// forms existed. Without it `c++ std::string` arrives as a different query
    /// and `a&b=c` arrives as three parameters.
    ///
    /// MUTATION: pass the query through untouched and the door itself refuses it
    /// — a space inside something that has already named a scheme is
    /// `Refusal::ControlOrWhitespace`, which is this rule and the URL rule
    /// agreeing rather than two rules.
    #[test]
    fn a_query_is_form_encoded_on_its_way_into_the_address() {
        let url = search_url(SearchEngineV1::DuckDuckGo, "c++ std::string");
        assert_eq!(url, "https://duckduckgo.com/?q=c%2B%2B+std%3A%3Astring");
        assert!(matches!(
            crate::webnav::address_bar(&url),
            crate::webnav::Decision::Navigate(_)
        ));
        // An ampersand would otherwise start a second parameter, and a `#` would
        // cut the query in half.
        assert_eq!(
            search_url(SearchEngineV1::Bing, "a&b=c#d"),
            "https://www.bing.com/search?q=a%26b%3Dc%23d"
        );
        // Non-ASCII goes out as UTF-8 bytes, percent by percent.
        assert_eq!(
            search_url(SearchEngineV1::Google, "中文"),
            "https://www.google.com/search?q=%E4%B8%AD%E6%96%87"
        );
        // And the unreserved set is left alone, because encoding a character
        // that never needed it makes an address nobody can read back.
        assert_eq!(
            search_url(SearchEngineV1::DuckDuckGo, "a-b_c.d~e9"),
            "https://duckduckgo.com/?q=a-b_c.d~e9"
        );
    }

    /// PIN (§7.7 ④) — **the address field lights up red only for what will not
    /// be navigated to, and a word is not one of those.**
    ///
    /// 「说在打字的地方」 is about a refusal; a non-address is not refused, it is
    /// searched. An empty field is unfinished rather than wrong.
    #[test]
    fn the_field_reddens_for_a_refusal_and_not_for_a_word() {
        assert!(WebSeat::would_go_to(""));
        assert!(WebSeat::would_go_to("   "));
        assert!(WebSeat::would_go_to("localhost:5173/app"));
        assert!(
            WebSeat::would_go_to("how do i pin a pane"),
            "a word is a search, not a refusal"
        );
        assert!(!WebSeat::would_go_to("javascript:alert(1)"));
        assert!(!WebSeat::would_go_to("file:///C:/Windows/win.ini"));
        assert!(!WebSeat::would_go_to("mailto:a@b.c"));
    }
}
