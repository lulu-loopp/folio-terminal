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

use bt_layout::SeatId;
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

// ── The seam with slice ② ──────────────────────────────────────────────────
//
// seam: 片② `webnav::navigation_starting` 落地后,删掉这一整段
// (`Refusal` / `Decision` / `Mint` / `BLANK_PAGE` / `navigation_starting` /
// `navigation_starting_against`)并改成
//
//     use crate::webnav::{BLANK_PAGE, Decision, Mint, Refusal, navigation_starting};
//
// 形状逐字相同,`WebSeat` 的调用处一行不改。

/// Why a candidate is not going anywhere. Slice ②'s vocabulary, carried here so
/// that the day it lands is a day of deletions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Refusal {
    ScriptOrInlineScheme,
    FileScheme,
    BrowserInternalScheme,
    ExternalScheme,
    UserInfo,
    NetworkPath,
    ControlOrWhitespace,
    NoHost,
    NotMinted,
    Empty,
}

/// What the policy says about one candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Decision {
    /// Go here. **The string is not always the candidate**: §3's loopback rule
    /// rewrites `0.0.0.0` to `127.0.0.1` keeping port, path, query and
    /// fragment, and that rewrite comes out of this door.
    Navigate(String),
    /// Not an address at all; the default search engine's business. It cannot
    /// come out of `NavigationStarting` — only out of the address field, which
    /// is slice ④'s — and slice ② has a test that pins that.
    Search(String),
    Refuse(Refusal),
}

/// What this pane minted for itself.
///
/// **One per pane, last write wins**, on the same discipline as §4's
/// `desired_url` and for the same reason: a mint is the answer to "did *we* ask
/// for this", and two of them is two answers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum Mint {
    /// The pane has asked for nothing.
    #[default]
    Nothing,
    /// The pane asked for its own blank page.
    ///
    /// This variant exists because of W0′ gate 9's one honest failure: the
    /// engine really does raise `NavigationStarting` for `about:blank`
    /// (`navigation_starting_fired: true`, `uri: "about:blank"`), while §3 as
    /// written refused `about:` outright — so a product that installed §3
    /// literally would cancel its own navigation and sit on the previous page.
    Blank,
    /// The pane asked for one sanctioned `file:` URL, minted from a canonical
    /// path. Slice ⑤'s door; slice ① never mints one.
    File(String),
}

pub(crate) const BLANK_PAGE: &str = "about:blank";

/// The narrowest policy that can exist: the URL this build was told to open,
/// and the blank page the host mints for itself.
///
/// Slice ②'s function of the same name is the real one — an allowlist, a
/// refusal matrix and a loopback rewrite, with eighteen rows of red tests behind
/// it. This one is a placeholder with the same signature and the same answers
/// for the two inputs slice ① can produce, so that the merge is a deletion.
///
/// The one difference to know about: here the development target is admitted
/// **by name**; there it will be admitted by §3's loopback rule, on its own
/// merits as an `http://127.0.0.1:port/` address. A `BT_WEB_DEV` pointed
/// somewhere that is not loopback therefore works today and will not after the
/// merge — which is the correct direction for a flag that exists to open a local
/// dev server.
pub(crate) fn navigation_starting(candidate: &str, mint: &Mint) -> Decision {
    navigation_starting_against(candidate, mint, development_target())
}

fn navigation_starting_against(candidate: &str, mint: &Mint, minted: Option<&str>) -> Decision {
    if candidate.is_empty() {
        return Decision::Refuse(Refusal::Empty);
    }
    if candidate.eq_ignore_ascii_case(BLANK_PAGE) {
        return if matches!(mint, Mint::Blank) {
            Decision::Navigate(BLANK_PAGE.to_owned())
        } else {
            Decision::Refuse(Refusal::NotMinted)
        };
    }
    if minted.is_some_and(|target| target == candidate) {
        return Decision::Navigate(candidate.to_owned());
    }
    if let Mint::File(sanctioned) = mint
        && sanctioned == candidate
    {
        return Decision::Navigate(candidate.to_owned());
    }
    // Everything else is refused, and named as precisely as a placeholder can
    // name it — the classification is slice ②'s to do properly.
    let scheme = candidate
        .split_once(':')
        .map(|(scheme, _)| scheme.to_ascii_lowercase())
        .unwrap_or_default();
    Decision::Refuse(match scheme.as_str() {
        "javascript" | "data" | "blob" | "vbscript" => Refusal::ScriptOrInlineScheme,
        "file" => Refusal::FileScheme,
        "about" | "edge" | "chrome" | "devtools" | "view-source" => Refusal::BrowserInternalScheme,
        "mailto" | "tel" | "ftp" | "ws" | "wss" => Refusal::ExternalScheme,
        _ => Refusal::NotMinted,
    })
}

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

/// Whether this list claims that key with those modifiers.
pub(crate) fn claims_chord(
    claims: &[ClaimedChord],
    virtual_key: u16,
    ctrl: bool,
    shift: bool,
    alt: bool,
) -> bool {
    claims.iter().any(|claim| {
        claim.chord
            == WebChord {
                virtual_key,
                ctrl,
                shift,
                alt,
            }
    })
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
pub(crate) const VK_MENU: u16 = 0x12;
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

// ── The driver ─────────────────────────────────────────────────────────────

/// How long anything waits for a browser process to say it is gone.
///
/// Ten seconds, which is the probe's own wait and the one the eight measured
/// shutdowns were read against: six answered in 271–390 ms, one in 6 588 ms and
/// one not at all (`w0p-evidence.md` §4.2). A number chosen under the slowest
/// one that *did* answer would turn that shutdown into a false deadline.
const BROWSER_EXIT_DEADLINE: Duration = Duration::from_secs(10);

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
    /// The teardown is finished: the browser has let go and the seat may be
    /// forgotten.
    Gone,
    /// Something the window should say out loud. Slice ④ owns the five failure
    /// cards (`DESIGN.md` §7.7 ④); until it exists this goes to `stderr`, which
    /// is the same place `BT_DPI` writes and for the same reason — a fact with
    /// nowhere yet to be drawn is still a fact.
    Fault(String),
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

/// One web seat: the state machine, the engine, and the mint they share.
pub(crate) struct WebSeat {
    pub(crate) seat: SeatId,
    hwnd: std::num::NonZeroIsize,
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
    /// What is on the glass now, so a frame that changed nothing issues no
    /// calls.
    presence: WebPresence,
}

impl WebSeat {
    /// Open a web seat on this pane and start the engine towards `url`.
    pub(crate) fn open(
        seat: SeatId,
        hwnd: std::num::NonZeroIsize,
        url: &str,
        wake: Box<dyn Fn()>,
    ) -> Result<Self, String> {
        let folder = user_data_folder().ok_or_else(|| {
            String::from("LOCALAPPDATA is not set, so there is no profile to use")
        })?;
        let mint = Rc::new(RefCell::new(Mint::Nothing));
        let gate = Rc::clone(&mint);
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
                    Decision::Search(_) | Decision::Refuse(_) => WebNavigationVerdict::Cancel,
                }
            }),
            wake,
        );
        let mut web = Self {
            seat,
            hwnd,
            folder,
            machine: WebMachine::new(),
            host,
            mint,
            claims: Vec::new(),
            waiting: None,
            presence: WebPresence::Hidden,
        };
        let effect = web.machine.request(url);
        debug_assert_eq!(effect, WebEffect::Ignore, "an engine that is not up yet");
        web.start_environment()?;
        Ok(web)
    }

    pub(crate) fn state(&self) -> WebState {
        self.machine.state()
    }

    /// The URL a session file could record — the last one that actually loaded.
    ///
    /// Slice ③'s to write down; slice ① keeps it because the recovery model is
    /// the thing that knows it, and a machine that tracked it only when
    /// somebody was listening would be a different machine.
    pub(crate) fn recoverable_url(&self) -> Option<&str> {
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
                if let Some(error) = error {
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
            // Recorded by the engine and acted on there; the state machine has
            // no opinion about a navigation that has not finished.
            WebEvent::NavigationStarting { .. } => WebEffect::Ignore,
            WebEvent::NavigationCompleted { uri, success, .. } => self
                .machine
                .on_navigation_completed(self.machine.generation(), uri, *success),
            // 0 is the browser process and 1 the renderer: one name over two
            // entirely different events.
            WebEvent::ProcessFailed { kind, .. } if *kind == 0 => {
                self.browser_is_gone(WebEvent::ProcessFailed {
                    kind: 0,
                    description: String::new(),
                })
            }
            WebEvent::ProcessFailed { .. } => self.machine.on_render_process_failed(),
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
                    .request_controller(self.hwnd, self.machine.generation())?;
                Ok(None)
            }
            WebEffect::InstallEvents => {
                // The visual first: the controller is told where to render
                // before it is told to do anything at all.
                compositor.attach_web_visual()?;
                self.host.install(compositor)?;
                let generation = self.machine.generation();
                Ok(Some(self.machine.on_events_installed(generation)))
            }
            WebEffect::Navigate(url) => {
                // **The mint before the navigation, always.**
                // `NavigationStarting` can fire before `Navigate` has returned,
                // and a gate asked about a target the pane has not yet admitted
                // to minting would cancel the pane's own navigation.
                *self.mint.borrow_mut() = if url.eq_ignore_ascii_case(BLANK_PAGE) {
                    Mint::Blank
                } else {
                    Mint::Nothing
                };
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
                self.presence = WebPresence::Hidden;
                bt_platform::forget_web_environment();
                self.start_environment()?;
                Ok(None)
            }
            WebEffect::RebuildForNewVersion => {
                // Here the browser is alive and has to be asked. Only when it
                // has gone may a new environment be made — see [`BrowserWait::Rebuild`].
                if self.waiting.is_none() {
                    self.host.close();
                    self.presence = WebPresence::Hidden;
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
                self.presence = WebPresence::Hidden;
                self.waiting = Some((
                    BrowserWait::Teardown,
                    Instant::now() + BROWSER_EXIT_DEADLINE,
                ));
                Ok(None)
            }
            WebEffect::ReleaseUserDataFolder => {
                self.waiting = None;
                let _ = compositor.detach_web_visual();
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
        self.claims
            .iter()
            .find(|claim| claim.chord == chord)
            .map(|claim| claim.action)
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
    ) -> Result<(), String> {
        if presence == self.presence {
            return Ok(());
        }
        self.presence = presence;
        match presence {
            WebPresence::Hidden => self.host.set_visible(false),
            WebPresence::Shown(bounds) => {
                self.host.set_size(bounds.width, bounds.height)?;
                compositor.place_web_visual(
                    (bounds.x, bounds.y),
                    (0.0, 0.0, bounds.width as f32, bounds.height as f32),
                )?;
                self.host.set_visible(true)
            }
        }
    }

    /// Where the page is this frame, or `None` when it is not on the glass.
    pub(crate) fn shown_at(&self) -> Option<WebBounds> {
        match self.presence {
            WebPresence::Shown(bounds) => Some(bounds),
            WebPresence::Hidden => None,
        }
    }

    /// Forward one mouse event. `window_point` is in the window's client area,
    /// in physical pixels; the translation into the page's own space happens
    /// here and nowhere else.
    pub(crate) fn send_mouse(
        &self,
        event: bt_platform::WebMouseEvent,
        window_point: (i32, i32),
        buttons_down: u32,
    ) -> Result<(), String> {
        let Some(bounds) = self.shown_at() else {
            return Ok(());
        };
        self.host.send_mouse(
            event,
            (window_point.0 - bounds.x, window_point.1 - bounds.y),
            buttons_down,
        )
    }

    /// Put the keyboard inside the page.
    pub(crate) fn focus_page(&self) -> Result<(), String> {
        self.host.focus_page()
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

    /// The browser process this seat is talking to, for evidence taking. `0`
    /// before there is one.
    pub(crate) fn browser_process_id(&self) -> u32 {
        self.host.browser_process_id()
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
    /// the web host takes back from a focused page are the same 30 rows.
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
        assert_eq!(spelled.len(), 30);
    }

    /// RED — and every one of them reaches a virtual key, because
    /// `AcceleratorKeyPressed` speaks Win32 and nothing else.
    #[test]
    fn every_shipped_chord_resolves_to_a_virtual_key_on_this_layout() {
        let claims = claimable_chords(&Shortcuts::defaults(), every_focus());
        assert_eq!(
            claims.len(),
            30,
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
        // F5 and F12 are the page's too, and stay so until somebody rules
        // otherwise — see `w0p-evidence.md` §8, slice ④.
        assert!(!claims_chord(&claims, VK_F5, false, false, false));
        assert!(!claims_chord(&claims, VK_F12, false, false, false));
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
        }
    }
}

#[cfg(test)]
mod navigation_stub_tests {
    use super::*;

    const DEV: &str = "http://127.0.0.1:9134/";

    fn verdict(candidate: &str, mint: &Mint) -> Decision {
        navigation_starting_against(candidate, mint, Some(DEV))
    }

    /// RED - slice one's policy is the narrowest one that can exist: the URL
    /// this build was told to open, and the blank page the host mints itself.
    #[test]
    fn only_the_hosts_own_two_targets_are_permitted() {
        assert_eq!(
            verdict(DEV, &Mint::Nothing),
            Decision::Navigate(DEV.to_owned())
        );
        assert_eq!(
            verdict(BLANK_PAGE, &Mint::Blank),
            Decision::Navigate(BLANK_PAGE.to_owned())
        );
        assert_eq!(
            verdict("ABOUT:BLANK", &Mint::Blank),
            Decision::Navigate(BLANK_PAGE.to_owned())
        );
    }

    /// RED - W0-prime gate 9's one honest failure, in its slice-one shape.
    ///
    /// The engine really does raise `NavigationStarting` for `about:blank`
    /// (`navigation_starting_fired: true`, `uri: "about:blank"`), so a door that
    /// refuses `about:` outright cancels the product's own navigation and leaves
    /// the seat on the page before. The door below lets through the blank page
    /// **this pane minted** and nothing else that spells itself `about:`.
    #[test]
    fn a_blank_page_nobody_minted_is_refused_and_a_minted_one_is_not() {
        assert_eq!(
            verdict(BLANK_PAGE, &Mint::Nothing),
            Decision::Refuse(Refusal::NotMinted)
        );
        for uri in ["about:config", "about:srcdoc", "about:blank#evil"] {
            assert!(
                matches!(verdict(uri, &Mint::Blank), Decision::Refuse(_)),
                "{uri}"
            );
        }
    }

    /// RED - a page that navigates itself somewhere else does not get there,
    /// whatever it spells the somewhere else as.
    #[test]
    fn a_page_that_navigates_itself_elsewhere_is_refused_by_family() {
        for (uri, expected) in [
            ("javascript:alert(1)", Refusal::ScriptOrInlineScheme),
            ("data:text/html,<b>x", Refusal::ScriptOrInlineScheme),
            (
                "blob:http://127.0.0.1:9134/x",
                Refusal::ScriptOrInlineScheme,
            ),
            ("file:///C:/Windows/win.ini", Refusal::FileScheme),
            ("edge://settings", Refusal::BrowserInternalScheme),
            (
                "view-source:http://127.0.0.1:9134/",
                Refusal::BrowserInternalScheme,
            ),
            ("mailto:a@b.c", Refusal::ExternalScheme),
            ("https://example.com/", Refusal::NotMinted),
            ("http://127.0.0.1:9134/other", Refusal::NotMinted),
            ("", Refusal::Empty),
        ] {
            assert_eq!(
                verdict(uri, &Mint::Nothing),
                Decision::Refuse(expected),
                "{uri}"
            );
        }
    }

    /// RED - the sanctioned `file:` door is slice five's, and it opens through
    /// the mint like every other door: one URL, the one that was minted.
    #[test]
    fn a_sanctioned_file_url_passes_and_its_neighbours_do_not() {
        let minted = "file:///D:/site/index.html";
        let mint = Mint::File(minted.to_owned());
        assert_eq!(
            navigation_starting_against(minted, &mint, None),
            Decision::Navigate(minted.to_owned())
        );
        assert_eq!(
            navigation_starting_against("file:///D:/site/secret.html", &mint, None),
            Decision::Refuse(Refusal::FileScheme)
        );
    }

    /// RED - with nothing minted and no development target, nothing goes
    /// anywhere. This is what the build a person installs does.
    #[test]
    fn with_nothing_minted_nothing_goes_anywhere() {
        assert_eq!(
            navigation_starting_against(DEV, &Mint::Nothing, None),
            Decision::Refuse(Refusal::NotMinted)
        );
        assert_eq!(
            navigation_starting_against(BLANK_PAGE, &Mint::Nothing, None),
            Decision::Refuse(Refusal::NotMinted)
        );
    }
}

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
