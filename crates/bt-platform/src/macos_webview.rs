//! **A page on a Mac: one `WKWebView` per seat, one delegate object for the
//! whole policy, and a compiled rule list where the third door's callback does
//! not exist** (ticket M4-2, on probe X-2's measurements; `docs/DESIGN.md`
//! §13.29).
//!
//! # What this module is and is not
//!
//! The same sentence `webview.rs` opens with, and it is worth repeating because
//! this is the second arm of one contract: **nothing here decides anything.**
//! Which URL to go to, whether a chord belongs to the window, when to rebuild
//! after a crash and where the seat is on screen are all questions
//! `bt_app::webhost` answers. What crosses the boundary is plain data —
//! [`WebEvent`] out, [`WebChord`] and a minted target in — and every type in it
//! is declared in `webview.rs` and compiled on every platform, which is why
//! `bt-app` can name `WebHost` with no `cfg` at all.
//!
//! # The three doors, and the one that is not a callback
//!
//! `bt_app::webnav` has three: `address_bar`, `navigation_starting` and
//! `resource_request`. The first never reaches a host. The second is
//! `-[WKNavigationDelegate webView:decidePolicyForNavigationAction:decisionHandler:]`,
//! which covers the main frame, every subframe, every redirect hop and every
//! script-driven location change — where WebView2 needed two separate events,
//! this is one callback that has to tell the two questions apart itself.
//!
//! **The third has no callback on this platform at all.** X-2 measured a
//! picture, a stylesheet, a script and a `fetch` each reaching a second origin's
//! socket while no delegate method ever named them. WebView2 answers this with
//! `WebResourceRequested` over every context; WebKit answers it with a
//! `WKContentRuleList` — a list of URL patterns compiled before the document
//! loads — and `bt_app::webnav::content_rules` is the same rule written in that
//! language, out of the same constants `resource_request` reads, pinned to it by
//! `the_two_spellings_of_the_resource_rule_agree`. The seat hands the list over
//! through [`WebHost::set_request_rules`], which is a door on every arm and does
//! nothing on the one whose engine asks per request.
//!
//! Which of the disk a local page may read by markup is not a pattern at all.
//! It is `-[WKWebView loadFileURL:allowingReadAccessToURL:]` with the minted
//! file's own folder — the grant Safari itself gives a `file://` page, measured
//! on Safari 26.6.2 for ticket 0.4.4-13 (the page's folder and below load; `../`
//! and another folder do not) — which X-2 measured enforcing with no rule list
//! in the room and no callback fired for the refusal.
//!
//! # What this arm does not promise, stated rather than implied
//!
//! Six sentences, and they are the port's rather than this file's: they are
//! written out in `docs/DESIGN.md` §13.29 and M4-3 says them in the product.
//!
//! * **No per-request decision for a document's own contents.** A refusal is a
//!   pattern that matched, not a question this program answered, so a rule that
//!   cannot be written as a pattern cannot be enforced here.
//! * **A refused subresource is dropped by the engine, not answered.** The
//!   Windows arm mints an empty 403 and queues a [`WebEvent::RequestRefused`]
//!   for the trace; there is no such line here, because nothing tells us.
//! * **Requests a service worker or a shared worker makes were not measured.**
//!   The Windows arm filters them on purpose
//!   (`AddWebResourceRequestedFilterWithRequestSourceKinds`); here they are
//!   unknown rather than covered.
//! * **Permissions are refused one capability at a time**, so a capability a
//!   later WebKit adds arrives with Apple's default rather than with Folio's
//!   refusal already standing.
//! * **The engine's own context menu stays.** `AreDefaultContextMenusEnabled`
//!   has no counterpart in WKWebView's public API, so that row of
//!   [`WEB_SETTINGS`] is reported unapplied rather than quietly assumed.
//! * **A page's icon and its match count never arrive.** WebKit announces
//!   neither, in any public form — see [`WebHost::get_favicon`] and
//!   [`WebHost::find`].
//!
//! # One thread, no locks
//!
//! Every call here is WebKit or AppKit and both are the main thread's. The gate
//! is [`window_thread`], the same one M1-3's and M4-1's doors pass through, so
//! the queue is an `Rc<RefCell<_>>` rather than a channel — the same shape, and
//! for the same reason, as the Windows arm's.
//!
//! # Nothing here blocks, and a panic here does not end the process
//!
//! Creation is asynchronous for the Windows arm's reason and for one of its own:
//! compiling a rule list is a completion block, and a seat's first navigation
//! must not start before the list it is to be judged by is on the page. So
//! [`ThirdDoor`] parks a URL that arrives ahead of its rules, and the compile's
//! completion block is what performs the load and what answers the
//! [`WebEvent::Controller`] the seat is waiting for.
//!
//! And **every delegate entry is wrapped in [`guarded`]**. X-2's second
//! carry-forward: a panic inside a delegate callback unwinds into Objective-C
//! and takes the process with it. Here it becomes the refusing answer — cancel,
//! deny, nil — plus one line on the diagnostics stream, which is where a
//! bundle's stderr goes (§13.23).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{NSResponder, NSView};
use objc2_foundation::{
    NSBundle, NSError, NSHTTPURLResponse, NSObjectProtocol, NSRect, NSString, NSURL,
    NSURLAuthenticationChallenge, NSURLAuthenticationMethodServerTrust, NSURLCredential,
    NSURLRequest, NSURLResponse, NSURLSessionAuthChallengeDisposition,
};
use objc2_web_kit::{
    WKContentRuleList, WKContentRuleListStore, WKFindConfiguration, WKFindResult, WKFrameInfo,
    WKMediaCaptureType, WKNavigation, WKNavigationAction, WKNavigationActionPolicy,
    WKNavigationDelegate, WKNavigationResponse, WKNavigationResponsePolicy, WKPermissionDecision,
    WKSecurityOrigin, WKUIDelegate, WKUserContentController, WKWebView, WKWebViewConfiguration,
    WKWebsiteDataStore, WKWindowFeatures,
};

use super::{
    CloseStep, INSTALL_SEQUENCE, InstallStep, PageVisual, RehostCompensation, RehostOutcome,
    RehostSide, RehostStep, WEB_CLOSE_STEPS, WEB_SETTINGS, WebChord, WebDpiOwnership, WebEvent,
    WebGuards, WebInstallReport, WebMouseEvent, WebNavigationVerdict, WebRequestGate,
    WebRequestKind, WebRequestVerdict, WebSetting, install_rollback,
};
use crate::macos_impl::{window_for, window_thread};
use crate::{Compositor, NativeWindow};

// ── the panic that must not leave this file ────────────────────────────────

/// **Run a delegate body, and turn a panic in it into the refusing answer.**
///
/// X-2's second carry-forward, in one function. A `#[unsafe(method(…))]` body is
/// called by Objective-C, and a panic unwinding across that frame ends the
/// process — with no terminal to say so, because a bundle started by `open` has
/// none. So every entry point below is this, and `refusal` is the value that
/// means *no* for whichever door it is: cancel the navigation, deny the
/// permission, open no window.
///
/// **The refusal and never the permission**, without exception: a door that
/// failed to decide has not decided, and the only safe reading of "we do not
/// know" is the one that lets nothing through.
fn guarded<T>(what: &str, refusal: T, body: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(answer) => answer,
        Err(panic) => {
            let said = panic
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| String::from("a panic with no message"));
            eprintln!("BT_MAC_WEB a panic inside {what} became a refusal: {said}");
            refusal
        }
    }
}

/// **One message, sent to the object rather than checked against its class.**
///
/// X-2's *first* carry-forward, and the reason it exists is measured rather than
/// guessed: objc2 verifies every selector against the receiver's class while
/// `debug_assertions` are on, and WebKit hands the authentication challenge over
/// as `WKNSURLAuthenticationChallenge` — a forwarding wrapper whose
/// `protectionSpace` is not a method on the class. The send works and only the
/// verification refuses it, which cost X-2 three runs before the cause was
/// found.
///
/// The probe's answer was to turn the verification off, which this crate cannot
/// do locally: objc2's `disable-encoding-assertions` is a Cargo feature, Cargo
/// features unify across a build, and turning it on here would turn the checking
/// off for every `msg_send!` in this workspace — including the ones where it has
/// caught a real mistake. This is the local answer instead. `performSelector:`
/// **is** a method on every class descended from `NSObject`, so the verification
/// passes on *it*, and the forwarding happens inside Objective-C where it
/// belongs. Ask the object, not the class.
///
/// # Safety
///
/// `selector` must name a zero-argument method that answers an object or `nil`,
/// and the answer must be an object this process may retain. Both are true of
/// the two selectors this file sends — `protectionSpace` and
/// `authenticationMethod` — and of nothing else.
unsafe fn ask<T: Message>(object: &T, selector: Sel) -> Option<Retained<AnyObject>> {
    // SAFETY: the caller's contract, plus `performSelector:`'s own — it takes a
    // selector and answers `id`, which is what the return type says.
    unsafe { msg_send![object, performSelector: selector] }
}

// ── what every callback can reach ──────────────────────────────────────────

/// The queue the delegate pushes onto, the two policy closures it asks, and the
/// nudge that gets the event loop to come and read what it wrote.
///
/// The same four things the Windows arm's `Shared` holds, for the same reasons;
/// the field notes there are the contract and are not repeated.
struct Shared {
    events: RefCell<VecDeque<WebEvent>>,
    /// The chords the window takes back from a focused page.
    ///
    /// **Held and not read** (M1-7, §13.13). WebView2 hands the host every key a
    /// focused page sees through `AcceleratorKeyPressed`; WKWebView has no such
    /// callback, and on this platform the window's own key handling sees the
    /// press first — a Command chord is the application's before it is ever the
    /// page's. So the table is kept because the type says a host keeps one, and
    /// nothing here consults it.
    chords: RefCell<Vec<WebChord>>,
    /// Asked, synchronously, inside `decidePolicyForNavigationAction:` about
    /// where the **main frame** is going.
    gate: Box<dyn Fn(&str) -> WebNavigationVerdict>,
    /// Asked, synchronously, in the same callback about where a **subframe** is
    /// going — the door `FrameNavigationStarting` is on the other platform. The
    /// rest of what a document is built out of never reaches a callback here and
    /// is [`ThirdDoor`]'s.
    request_gate: WebRequestGate,
    /// The target of the rewrite currently in flight, if any — the Windows arm's
    /// belt against a normalisation that answered twice.
    rewriting_to: RefCell<Option<String>>,
    /// The status the last main-frame response carried, so that
    /// [`WebEvent::NavigationCompleted`] can report one: WebKit's finish
    /// callback carries no response.
    last_status: Cell<i32>,
    /// **The term the page is being searched for, and how.**
    ///
    /// `ICoreWebView2Find` keeps a session and `find_step` asks *it* for the
    /// next match; WebKit has no session and no "again" call — the only find is
    /// `findString:withConfiguration:`, which takes the term every time. So the
    /// term is kept here, which is what makes a step a step rather than a search
    /// for nothing.
    found: RefCell<String>,
    found_case: Cell<bool>,
    wake: Box<dyn Fn()>,
}

impl Shared {
    fn push(&self, event: WebEvent) {
        self.events.borrow_mut().push_back(event);
        (self.wake)();
    }

    /// The address, the title and the two history booleans, read off the view at
    /// the one moment WebKit has them settled.
    ///
    /// **The stated gap**: the history API changes all four without a
    /// navigation, and WebKit announces that through key-value observation
    /// rather than through a delegate. So a single-page application's address
    /// stands still here where the Windows arm's `SourceChanged` follows it.
    /// Observing the four properties is what closes it, and §13.29 records it.
    fn publish_page(&self, web_view: &WKWebView) {
        // SAFETY: a live view on the main thread; every one of these is a
        // property read.
        let uri = unsafe { web_view.URL() }
            .and_then(|url| url.absoluteString())
            .map(|text| text.to_string())
            .unwrap_or_default();
        self.push(WebEvent::SourceChanged { uri });
        let title = unsafe { web_view.title() }
            .map(|text| text.to_string())
            .unwrap_or_default();
        self.push(WebEvent::DocumentTitleChanged { title });
        self.push(WebEvent::HistoryChanged {
            can_go_back: unsafe { web_view.canGoBack() },
            can_go_forward: unsafe { web_view.canGoForward() },
        });
    }

    /// A navigation that did not arrive, from either of the two callbacks that
    /// can say so.
    fn navigation_failed(&self, web_view: &WKWebView, error: &NSError) {
        // SAFETY: a live view on the main thread.
        let uri = unsafe { web_view.URL() }
            .and_then(|url| url.absoluteString())
            .map(|text| text.to_string())
            .unwrap_or_default();
        let described = error.localizedDescription().to_string();
        eprintln!("BT_MAC_WEB a navigation failed: {described}");
        self.push(WebEvent::NavigationCompleted {
            uri,
            success: false,
            status: self.last_status.get(),
        });
    }

    /// **Point the page at an address** — and, for a local file, at the one
    /// folder it may read, which is the folder Safari grants a `file://` page.
    ///
    /// This is where the local seat's folder bound actually stands.
    /// `loadFileURL:allowingReadAccessToURL:` is the whole of it: X-2 measured a
    /// picture and a frame naming a sibling folder refused by this call with no
    /// rule list in the room, and no callback fired for either — which is why
    /// the folder half is not, and cannot be, a pattern in
    /// `webnav::content_rules`.
    fn load(&self, web_view: &WKWebView, url: &str) {
        let text = NSString::from_str(url);
        let Some(address) = NSURL::URLWithString(&text) else {
            eprintln!("BT_MAC_WEB an address Foundation would not parse: {url}");
            return;
        };
        if address.isFileURL() {
            // The *path* rather than the URL, so that Foundation's own decoding
            // is what undoes the percent escapes `Mint::file` wrote — and then a
            // file URL built back from that path, which is the form
            // `loadFileURL:` wants.
            let Some(path) = address.path() else {
                eprintln!("BT_MAC_WEB a file URL with no path: {url}");
                return;
            };
            let file = NSURL::fileURLWithPath(&path);
            let Some(folder) = file.URLByDeletingLastPathComponent() else {
                eprintln!("BT_MAC_WEB a file URL with no folder: {url}");
                return;
            };
            // SAFETY: a live view and two live file URLs, on the main thread.
            let _ = unsafe { web_view.loadFileURL_allowingReadAccessToURL(&file, &folder) };
            return;
        }
        let request = NSURLRequest::requestWithURL(&address);
        // SAFETY: a live view and a request this call just made, on the main
        // thread.
        let _ = unsafe { web_view.loadRequest(&request) };
    }
}

// ── the third door, and the one piece of state that makes it orderly ───────

/// **The seat's resource rule in its compiled form, and everything that waits on
/// it.**
///
/// `wanted` is what the seat last said its policy is; `attached` is the list
/// actually on the page, beside the JSON it was compiled from. Two things wait
/// for the two to agree: the [`WebEvent::Controller`] a freshly made page owes
/// its seat, and a navigation that arrived before its rules did. A document
/// loading against the previous mint's list is the previous seat's policy
/// enforced on this one's contents — and compiling is a completion block, so
/// there is a moment in which that would be possible. There is no such moment:
/// the URL is **parked** and the block performs the load.
///
/// It is an `Rc` of its own rather than fields on [`WebHost`] because the
/// completion block outlives the call that made it and cannot hold `&mut` of
/// anything.
struct ThirdDoor {
    shared: Rc<Shared>,
    /// Where compiled lists are cached — the folder `request_environment` was
    /// given. See `bt_app::webhost::web_engine_folder` for what that folder is
    /// and is not.
    store: RefCell<Option<Retained<WKContentRuleListStore>>>,
    /// The page the list goes onto and the parked load goes to.
    page: RefCell<Option<Retained<WKWebView>>>,
    /// **Which page this door is currently about**, moved on by every
    /// [`WebHost::request_controller`] and by every [`ThirdDoor::let_go`].
    ///
    /// A compiled rule list belongs to the controller it was compiled for, and
    /// compiling is a completion block — so a seat that asks for a second page
    /// while the first page's list is still in flight has two pages and one
    /// door. Before this number existed that block landed on the door it was
    /// started from whatever had happened since: it wrote `attached` and
    /// `stands` for a controller the host had already thrown away, and
    /// [`WebHost::attach_delegates`] then certified the *new* page's
    /// subresources as gated by a list that had never been put on it (RA-3).
    /// Every effect a completion has is therefore conditional on the mint it
    /// captured still being this one — see [`compile_is_stale`].
    ///
    /// It is the door's own count and not the seat's `generation`: the seat
    /// numbers the attempts it is making, this numbers the controllers that
    /// exist, and a close makes a new one of the second without making one of
    /// the first.
    mint: Cell<u64>,
    /// A [`WebEvent::Controller`] this door still owes, and the generation it is
    /// owed for.
    owed: Cell<Option<u64>>,
    wanted: RefCell<String>,
    /// **What is on the page and which page it went onto** — the decision, with
    /// no WebKit object in it, which is what lets the state machine below be
    /// asked by a `#[test]` on a machine that cannot make one.
    attached: RefCell<Option<Attached>>,
    /// The objects behind [`ThirdDoor::attached`]: the controller the list was
    /// added to, kept so that the claim can be **checked against the live page**
    /// rather than believed, and the list itself, kept so that this door holds
    /// its own strong reference to what the page is using.
    ///
    /// The two fields move together and [`ThirdDoor::list_is_on_this_page`]
    /// demands that they agree, so a drift between them reads as *not
    /// standing*, which is the direction a guard has to fail in.
    on_the_page: RefCell<
        Option<(
            Retained<WKUserContentController>,
            Retained<WKContentRuleList>,
        )>,
    >,
    /// **Where this seat is going, if it is not there yet** — and the only copy
    /// of that fact (audit 3 A-1).
    ///
    /// Written by [`ThirdDoor::destined_for`] on **both** of its branches, which
    /// is what makes it the seat's current destination rather than a record of
    /// one it was once given: an address that has been superseded by a later
    /// `navigate` is not somewhere this seat is going, and replaying it a
    /// compile round-trip later would take the reader off the document they
    /// asked for last.
    parked: RefCell<Option<String>>,
    compiling: Cell<bool>,
    /// Whether a list is on the page — which is the whole of what
    /// `WebGuards::resource_requests` means here.
    stands: Cell<bool>,
    /// **What the compiler said when it refused**, kept so that the seat's
    /// fault line can carry it.
    ///
    /// It is here rather than on a diagnostic stream because a bundle started
    /// by `open` has none: the one run that found this — M4-2's own `.app`
    /// proof, where `^(http|https)://` would not compile — spent a whole cycle
    /// establishing a fact WebKit had already said out loud.
    refused: RefCell<Option<String>>,
}

/// **A rule list that is on a page, said without naming a WebKit object.**
///
/// `json` is what it was compiled from — what [`ThirdDoor::settled`] compares
/// against `wanted` — and `mint` is the page it went onto. Both are needed:
/// the same JSON compiled for the controller before last is not on this one.
#[derive(Debug, Eq, PartialEq)]
struct Attached {
    json: String,
    mint: u64,
}

impl ThirdDoor {
    fn new(shared: Rc<Shared>) -> Self {
        Self {
            shared,
            store: RefCell::new(None),
            page: RefCell::new(None),
            mint: Cell::new(0),
            owed: Cell::new(None),
            wanted: RefCell::new(String::new()),
            attached: RefCell::new(None),
            on_the_page: RefCell::new(None),
            parked: RefCell::new(None),
            compiling: Cell::new(false),
            stands: Cell::new(false),
            refused: RefCell::new(None),
        }
    }

    /// Whether the list on the page is the list the seat asked for.
    fn settled(&self) -> bool {
        let wanted = self.wanted.borrow();
        match self.attached.borrow().as_ref() {
            Some(attached) => attached.json == *wanted,
            // A seat that has said nothing has nothing outstanding; a seat that
            // has said something and has no list has.
            None => wanted.is_empty(),
        }
    }

    /// **Take the seat's one destination**, and say whether it may be loaded
    /// now — the pure half of [`WebHost::navigate`] (audit 3 A-1).
    ///
    /// A seat goes to one address and it is the last one it was given, so this
    /// writes [`ThirdDoor::parked`] on both branches. The branch that was
    /// missing is the one that loads: a door that is settled loads straight
    /// away, and leaving a `parked` address behind it left the seat holding a
    /// destination it had already been taken off. The cost of that is one
    /// compile round-trip later, in [`ThirdDoor::answer_what_waited`], which
    /// finds the door standing and navigates a reader who is reading their
    /// second report to the address they typed before it.
    fn destined_for(&self, url: &str) -> bool {
        if self.settled() {
            *self.parked.borrow_mut() = None;
            return true;
        }
        *self.parked.borrow_mut() = Some(url.to_owned());
        false
    }

    /// **Whether this door's `stands` is a statement about the page it is
    /// standing on** — the pure half of [`ThirdDoor::list_is_on_this_page`].
    ///
    /// `stands` alone is a flag; this is the flag together with the fact it was
    /// set from. They can only disagree if a list is attached for a mint that
    /// has moved on, which is precisely RA-3's shape.
    fn stands_for_this_mint(&self) -> bool {
        self.stands.get()
            && self
                .attached
                .borrow()
                .as_ref()
                .is_some_and(|attached| attached.mint == self.mint.get())
    }

    /// **Whether the list this door would certify is on the page it is
    /// certifying** — the question `WebGuards::resource_requests` really asks.
    ///
    /// Two answers have to agree before this says yes: the door's own
    /// bookkeeping ([`ThirdDoor::stands_for_this_mint`]), and the **live
    /// object** — the controller this page carries now has to be the very one
    /// the list was added to. WebKit has no public way to ask a controller what
    /// rule lists are on it, so the identity of the controller is what stands in
    /// for that question, and it is a fact from outside this struct rather than
    /// a restatement of what is in it.
    fn list_is_on_this_page(&self) -> bool {
        if !self.stands_for_this_mint() {
            return false;
        }
        let Some(live) = self.controller() else {
            return false;
        };
        self.on_the_page
            .borrow()
            .as_ref()
            .is_some_and(|(on, _)| std::ptr::eq(&**on, &*live))
    }

    /// The content controller the list goes onto.
    ///
    /// `-[WKWebView configuration]` answers a **copy**, and the copy carries the
    /// same user content controller object — which is what makes swapping a rule
    /// list on a page that is already on the glass possible at all, and is what
    /// X-2 measured working.
    fn controller(&self) -> Option<Retained<WKUserContentController>> {
        let page = self.page.borrow();
        let page = page.as_ref()?;
        // SAFETY: a live view on the main thread.
        Some(unsafe { page.configuration().userContentController() })
    }

    /// **Answer whatever was waiting for the door to be decided** — either way.
    fn answer_what_waited(&self) {
        if let Some(generation) = self.owed.take() {
            // **Two ways a door does not stand, and they are different
            // sentences**: a seat that stated no rule at all has nothing that
            // could have failed, and a seat whose rule would not compile has.
            let error = if self.stands_for_this_mint() {
                None
            } else if self.wanted.borrow().is_empty() {
                Some(String::from(
                    "this seat stated no resource rule, so nothing gates what its pages are built out of",
                ))
            } else {
                Some(match self.refused.borrow().as_deref() {
                    Some(said) => format!("the page's resource rules would not compile: {said}"),
                    None => String::from("the page's resource rules would not compile"),
                })
            };
            self.shared.push(WebEvent::Controller { generation, error });
        }
        let parked = self.parked.borrow_mut().take();
        let Some(url) = parked else {
            return;
        };
        let page = self.page.borrow().clone();
        match (self.stands_for_this_mint(), page) {
            (true, Some(page)) => self.shared.load(&page, &url),
            // **Fail closed, and say so.** A navigation that cannot be judged is
            // a navigation that does not happen, and the seat hears it as the
            // refusal it is rather than as a page that never arrived.
            _ => self.shared.push(WebEvent::NavigationStarting {
                uri: url,
                cancelled: true,
            }),
        }
    }

    /// Everything this door holds, let go of.
    ///
    /// **Every `Cell` and `RefCell` on the struct is named in this body**, and
    /// `let_go_names_every_cell_of_the_third_door` in `webview.rs` keeps it that
    /// way. `compiling` is the one that was missed, and what that cost is RA-3's
    /// second half: a `close()` while a compile was in flight left the door
    /// latched shut — `compile` returns immediately while `compiling` is set —
    /// for the rest of the process's life, and the block that finally landed
    /// then wrote `attached` and `stands` for a page that no longer existed.
    fn let_go(&self) {
        *self.store.borrow_mut() = None;
        *self.page.borrow_mut() = None;
        *self.attached.borrow_mut() = None;
        *self.on_the_page.borrow_mut() = None;
        *self.parked.borrow_mut() = None;
        *self.refused.borrow_mut() = None;
        self.owed.set(None);
        self.compiling.set(false);
        self.stands.set(false);
        // **And the door moves on.** Clearing `compiling` un-latches it; moving
        // the mint is what makes the block still in flight for the page that
        // has just gone land on a mint that is not this one, so that it cannot
        // write the state this call has just cleared.
        self.mint.set(self.mint.get().wrapping_add(1));
        // `wanted` is deliberately **kept**: it is what the seat last said its
        // policy is, and letting go of a page does not un-say it — a rebuilt
        // page compiles the same rule again.
    }
}

/// **Compile the seat's resource rule and put it on the page**, then answer
/// whatever was waiting for it.
///
/// A free function because the completion block calls it again: a seat whose
/// mint moved while a compile was in flight has to be compiled for the mint it
/// moved to, and a block cannot reach the host.
///
/// Idempotent and cheap: a door that is already settled answers what waited and
/// returns, and a compile already in flight is left to finish rather than raced.
fn compile(door: &Rc<ThirdDoor>) {
    if door.compiling.get() {
        return;
    }
    if door.page.borrow().is_none() {
        // Nothing to put a list on yet. `request_controller` compiles as soon as
        // there is.
        return;
    }
    if door.settled() {
        door.answer_what_waited();
        return;
    }
    let Some(store) = door.store.borrow().clone() else {
        return;
    };
    let Some(controller) = door.controller() else {
        return;
    };
    let json = door.wanted.borrow().clone();
    let identifier = identifier_for(&json);
    // A copy for the block, which outlives this call and therefore cannot
    // borrow what the call is still about to hand to the compiler.
    let compiled_from = json.clone();
    // **And the page this compile is for**, captured beside the controller it
    // is for. See [`ThirdDoor::mint`].
    let mint = door.mint.get();

    door.compiling.set(true);
    let again = Rc::clone(door);
    let done = RcBlock::new(move |list: *mut WKContentRuleList, error: *mut NSError| {
        let outcome = if error.is_null() {
            NonNull::new(list)
                // SAFETY: a live rule list WebKit handed to this block; the
                // retain is what makes it outlive the call.
                .and_then(|list| unsafe { Retained::retain(list.as_ptr()) })
                .ok_or_else(|| String::from("the rule list compiler answered with nothing"))
        } else {
            // SAFETY: a live error WebKit handed to this block, and a property
            // read on it.
            let error = unsafe { &*error };
            Err(error.localizedDescription().to_string())
        };
        // **Is this still the page and the policy this compile was started
        // for?** Everything below writes the door, and a block that has been
        // overtaken on either count may write none of it — the two WebKit calls
        // in the arm below are the ones that put a list on a live page, so the
        // question is asked before them and not after. This is also what clears
        // `compiling` and starts the compile the current page is owed.
        if compile_is_stale(&again, mint, &compiled_from) {
            return;
        }
        match outcome {
            Ok(list) => {
                // SAFETY: a live content controller and a live rule list, on the
                // main thread — which is where WebKit answers this block.
                unsafe { controller.removeAllContentRuleLists() };
                unsafe { controller.addContentRuleList(&list) };
                *again.on_the_page.borrow_mut() = Some((controller.clone(), list));
                rules_compiled(&again, Ok(compiled_from.clone()));
            }
            Err(reason) => {
                // SAFETY: as above.
                unsafe { controller.removeAllContentRuleLists() };
                *again.on_the_page.borrow_mut() = None;
                rules_compiled(&again, Err(reason));
            }
        }
    });
    // SAFETY: a live store, two strings this function made, and a block
    // `RcBlock` keeps alive across the call.
    unsafe {
        store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
            Some(&NSString::from_str(&identifier)),
            Some(&NSString::from_str(&json)),
            Some(&done),
        );
    }
}

/// **Whether a finished compile is still about the page *and* the policy it was
/// started for** — and the whole of what one that is not does (RA-3; audit 3
/// A-1).
///
/// A rule list belongs to the controller it was compiled for. The door is
/// shared across every controller a seat makes, so a completion that lands
/// after [`WebHost::request_controller`] or [`ThirdDoor::let_go`] has moved the
/// mint on is holding a list for a page that is gone, and the only honest thing
/// it can do with it is nothing: it does not attach it, it does not say the
/// door stands, it does not record a refusal, and it answers nobody — the
/// [`WebEvent::Controller`] `owed` names belongs to the *current* attempt and
/// would be a false certification of it.
///
/// **And the second fact, for the same reason as the first** (audit 3 A-1). A
/// compile carries a *page* and a *policy*, and the mint moves only when the
/// page does: a seat re-minted in place — a reader clicking an `http://` link
/// printed in a local report, then opening a second report out of the files
/// column — moves `wanted` across a category boundary and back while one
/// compile is in flight, and the block that lands is holding the browsing
/// seat's list for a page showing a local document. The mint has not moved, so
/// asking about the page alone answers *this is current*, and the caller's very
/// next two lines put a list on the live page that says `http` and `https` are
/// allowed. So both facts are asked here, before either WebKit call, and a
/// compile that answers for a policy the seat has moved off is dropped exactly
/// as one for a page it has moved off is — never attached and then corrected,
/// because for the length of that correction the document on the glass is being
/// judged by a policy nobody asked for.
///
/// Two things it must still do, and they are why this is a function rather than
/// an early `return` in the block. `compiling` is cleared, because it is the
/// latch that stops a second compile from starting and this compile is over;
/// and [`compile`] is called, because the page that took over is owed a list of
/// its own and nothing else is going to start one — `request_controller`
/// already tried and found the latch set.
fn compile_is_stale(door: &Rc<ThirdDoor>, mint: u64, compiled_from: &str) -> bool {
    door.compiling.set(false);
    let page_moved = mint != door.mint.get();
    // The borrow ends here: `compile` below reads `wanted` itself.
    let policy_moved = *door.wanted.borrow() != compiled_from;
    if !page_moved && !policy_moved {
        return false;
    }
    if page_moved {
        eprintln!(
            "BT_MAC_WEB a rule list compiled for page {mint} arrived after page {} took over; \
             it is dropped and the current page is compiled for",
            door.mint.get()
        );
    } else {
        eprintln!(
            "BT_MAC_WEB a rule list compiled for a resource rule the seat has moved off arrived \
             for page {mint}; it is dropped and the rule the seat states now is compiled for"
        );
    }
    compile(door);
    true
}

/// **What a finished compile does to the door**, once [`compile_is_stale`] has
/// said it is this page's.
///
/// The whole of the state machine and **no WebKit in it**, which is what lets
/// `door_tests` below ask it the questions X-2's in-bundle target cannot: the
/// objects have already been put on the controller by the caller, and what is
/// left is the bookkeeping and who gets told.
///
/// `Ok` carries the JSON the list was compiled from; `Err` carries what the
/// compiler said.
fn rules_compiled(door: &Rc<ThirdDoor>, outcome: Result<String, String>) {
    match outcome {
        Ok(compiled_from) => {
            *door.attached.borrow_mut() = Some(Attached {
                json: compiled_from,
                mint: door.mint.get(),
            });
            *door.refused.borrow_mut() = None;
            door.stands.set(true);
            if door.settled() {
                door.answer_what_waited();
            } else {
                // The seat moved on while this was in flight, and what is on
                // the page is last mint's rule. Nothing is answered until
                // the list is this mint's.
                compile(door);
            }
        }
        Err(reason) => {
            // **Fail closed.** A page whose rule list would not compile has
            // no third door, so it keeps nothing of the old one — and it is
            // not retried, because a compile that failed on this JSON would
            // fail on it again. The next `set_request_rules` is what starts
            // one.
            *door.attached.borrow_mut() = None;
            door.stands.set(false);
            eprintln!("BT_MAC_WEB the page's resource rules would not compile: {reason}");
            *door.refused.borrow_mut() = Some(reason);
            door.answer_what_waited();
        }
    }
}

/// **The identifier a rule list is compiled under, which is its own contents.**
///
/// `WKContentRuleListStore` caches a compilation by identifier on disk, so an
/// identifier that named a *policy* — "the file seat's", say — would go on
/// answering with last week's compilation after the rule behind it changed. A
/// name derived from the bytes cannot: a changed rule is a changed name, and the
/// old compilation is simply never asked for again.
///
/// FNV-1a, because what is wanted is a short stable name and not a promise about
/// an adversary: both sides of this are written by this program.
fn identifier_for(json: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in json.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("folio-{hash:016x}")
}

// ── the one object that is the whole policy ────────────────────────────────

/// The URL a request names, as text.
fn url_of(request: &NSURLRequest) -> String {
    // SAFETY: a live request handed over by WebKit, on the main thread.
    request
        .URL()
        .and_then(|url| url.absoluteString())
        .map(|text| text.to_string())
        .unwrap_or_default()
}

define_class!(
    // SAFETY:
    // - `NSObject` has no subclassing requirements beyond the main thread, which
    //   `MainThreadOnly` states and which every WebKit delegate callback
    //   satisfies.
    // - This class implements no `Drop` of its own; the `Rc<Shared>` in its
    //   ivars is dropped by the `dealloc` `define_class!` generates.
    #[unsafe(super(objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FolioWebPolicyGate"]
    #[ivars = Rc<Shared>]
    struct Gate;

    unsafe impl NSObjectProtocol for Gate {}

    unsafe impl WKNavigationDelegate for Gate {
        /// **Where anything is going** — the main frame, a subframe, every
        /// redirect hop, a script setting `location`, a `data:` link, a
        /// `mailto:` and a window a page wants opened. One callback, where the
        /// other engine has four events.
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide_action(
            &self,
            web_view: &WKWebView,
            action: &WKNavigationAction,
            handler: &block2::DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            let mut renavigate_to = None;
            let policy = guarded(
                "decidePolicyForNavigationAction:",
                WKNavigationActionPolicy::Cancel,
                || self.decide(action, &mut renavigate_to),
            );
            handler.call((policy,));
            // **After the handler and not inside it.** A cancel-and-renavigate
            // is two decisions — this navigation stops, that one starts — and
            // asking the engine to start the second before the first has been
            // answered is asking it to run this callback inside itself.
            if let Some(target) = renavigate_to {
                guarded("the substitute navigation", (), || {
                    self.shared_state().load(web_view, &target);
                });
            }
        }

        /// **What came back** — the download refusal lives here, because this is
        /// the one callback that has seen the response's headers.
        #[unsafe(method(webView:decidePolicyForNavigationResponse:decisionHandler:))]
        fn decide_response(
            &self,
            _web_view: &WKWebView,
            response: &WKNavigationResponse,
            handler: &block2::DynBlock<dyn Fn(WKNavigationResponsePolicy)>,
        ) {
            let policy = guarded(
                "decidePolicyForNavigationResponse:",
                WKNavigationResponsePolicy::Cancel,
                || {
                    // SAFETY: a live response from WebKit on the main thread.
                    let raw: Retained<NSURLResponse> = unsafe { response.response() };
                    if unsafe { response.isForMainFrame() } {
                        let status = raw
                            .clone()
                            .downcast::<NSHTTPURLResponse>()
                            .map_or(0, |http| http.statusCode() as i32);
                        self.shared_state().last_status.set(status);
                    }
                    // **`canShowMIMEType == false` is the download**, which is
                    // what `DownloadStarting` is on the other platform: a body
                    // the engine will not draw is a body it would save. The
                    // cancel is unconditional for `SetCancel`'s reason — the
                    // decision cannot be taken later — and what happens
                    // *instead* is the caller's, on its own turn.
                    if unsafe { response.canShowMIMEType() } {
                        return WKNavigationResponsePolicy::Allow;
                    }
                    let uri = raw.URL()
                        .and_then(|url| url.absoluteString())
                        .map(|text| text.to_string())
                        .unwrap_or_default();
                    let file_name = raw.suggestedFilename()
                        .map(|text| text.to_string())
                        .unwrap_or_default();
                    self.shared_state()
                        .push(WebEvent::DownloadStarting { uri, file_name });
                    WKNavigationResponsePolicy::Cancel
                },
            );
            handler.call((policy,));
        }

        /// The address committed. **Three facts leave here at once** — the URL,
        /// the title and the two history booleans — because WebKit publishes
        /// them as properties rather than as events, and this is the moment they
        /// are settled for a navigation that really happened.
        #[unsafe(method(webView:didCommitNavigation:))]
        fn did_commit(&self, web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            guarded("didCommitNavigation:", (), || {
                self.shared_state().publish_page(web_view);
            });
        }

        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            guarded("didFinishNavigation:", (), || {
                let shared = self.shared_state();
                shared.publish_page(web_view);
                // SAFETY: the view WebKit is calling this on.
                let uri = unsafe { web_view.URL() }
                    .and_then(|url| url.absoluteString())
                    .map(|text| text.to_string())
                    .unwrap_or_default();
                shared.push(WebEvent::NavigationCompleted {
                    uri,
                    success: true,
                    status: shared.last_status.get(),
                });
            });
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional(
            &self,
            web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
            error: &NSError,
        ) {
            guarded("didFailProvisionalNavigation:", (), || {
                self.shared_state().navigation_failed(web_view, error);
            });
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(
            &self,
            web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
            error: &NSError,
        ) {
            guarded("didFailNavigation:", (), || {
                self.shared_state().navigation_failed(web_view, error);
            });
        }

        /// **Every password box refused, and the machine's own certificate
        /// evaluation left alone.**
        ///
        /// One callback carries both questions. A server-trust challenge is the
        /// TLS handshake asking whether this certificate is acceptable, and the
        /// only answer this product has is *the one the operating system would
        /// give* — the same certificate store, revocation and managed profile
        /// WinHTTP uses on the other platform. Everything else is a site asking
        /// a reader for a password inside a pane of a terminal, and the answer to
        /// that is no.
        ///
        /// Telling the two apart means reading the challenge, which is where
        /// [`ask`] and its long note come in.
        #[unsafe(method(webView:didReceiveAuthenticationChallenge:completionHandler:))]
        fn did_challenge(
            &self,
            _web_view: &WKWebView,
            challenge: &NSURLAuthenticationChallenge,
            handler: &block2::DynBlock<
                dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential),
            >,
        ) {
            let disposition = guarded(
                "didReceiveAuthenticationChallenge:",
                // The floor: exactly what a host that never implemented this
                // method at all would do. Never worse than not being here.
                NSURLSessionAuthChallengeDisposition::PerformDefaultHandling,
                || {
                    // SAFETY: two zero-argument selectors that answer objects —
                    // see `ask`.
                    let method = unsafe { ask(challenge, sel!(protectionSpace)) }
                        .and_then(|space| unsafe { ask(&*space, sel!(authenticationMethod)) })
                        .and_then(|name| name.downcast::<NSString>().ok())
                        .map(|name| name.to_string());
                    // SAFETY: Foundation's own constant.
                    let server_trust = unsafe { NSURLAuthenticationMethodServerTrust }.to_string();
                    match method {
                        Some(named) if named == server_trust => {
                            NSURLSessionAuthChallengeDisposition::PerformDefaultHandling
                        }
                        Some(_) => NSURLSessionAuthChallengeDisposition::RejectProtectionSpace,
                        // A challenge that would not say what it is gets the
                        // floor rather than a guess in either direction.
                        None => NSURLSessionAuthChallengeDisposition::PerformDefaultHandling,
                    }
                },
            );
            handler.call((disposition, std::ptr::null_mut()));
        }

        /// **The page's process died** — WebKit's only process notification, and
        /// it names the one that draws.
        ///
        /// Reported as [`WebEvent::ProcessFailed`] with the renderer's kind and
        /// not the browser's, because that is what it is: the `WKWebView` is
        /// still a live object with its delegates on it, and the recovery Apple
        /// documents for this is a reload — which is exactly what
        /// `bt_app::webhost::WebMachine::on_render_process_failed` answers a
        /// renderer death with. The browser-process kind would start a rebuild
        /// from an environment this platform does not have.
        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn content_process_died(&self, _web_view: &WKWebView) {
            guarded("webViewWebContentProcessDidTerminate:", (), || {
                self.shared_state().push(WebEvent::ProcessFailed {
                    kind: 1,
                    description: String::from("the web content process ended"),
                });
            });
        }
    }

    unsafe impl WKUIDelegate for Gate {
        /// **No page opens a window.** X-2 measured this as the only door
        /// `window.open` reaches — it fires with no navigation action decided
        /// first — so it is shut here as well as at the action above.
        #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
        fn create_web_view(
            &self,
            _web_view: &WKWebView,
            _configuration: &WKWebViewConfiguration,
            action: &WKNavigationAction,
            _features: &WKWindowFeatures,
        ) -> Option<Retained<WKWebView>> {
            guarded("createWebViewWithConfiguration:", None, || {
                // Routed through the gate before it is refused, so that the
                // caller's trace carries the address a page tried to open in a
                // window of its own and not merely the fact that one did.
                // SAFETY: a live action from WebKit on the main thread.
                let request = unsafe { action.request() };
                let uri = url_of(&request);
                let _ = (self.shared_state().gate)(&uri);
                None
            })
        }

        /// **`alert`, answered without a window.** The engine shows a panel only
        /// if this method is here; being here and returning immediately is what
        /// `AreDefaultScriptDialogsEnabled(false)` plus `ScriptDialogOpening`
        /// buys on the other platform — a page in a loop gets its `alert` back
        /// on the spot every time and never holds the seat.
        #[unsafe(method(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:))]
        fn run_alert(
            &self,
            _web_view: &WKWebView,
            _message: &NSString,
            _frame: &WKFrameInfo,
            handler: &block2::DynBlock<dyn Fn()>,
        ) {
            guarded("runJavaScriptAlertPanel", (), || {
                // 0, 1 and 2 are `COREWEBVIEW2_SCRIPT_DIALOG_KIND`'s alert,
                // confirm and prompt — the vocabulary `WebEvent` is written in.
                self.shared_state()
                    .push(WebEvent::ScriptDialogDismissed { kind: 0 });
            });
            handler.call(());
        }

        #[unsafe(method(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:))]
        fn run_confirm(
            &self,
            _web_view: &WKWebView,
            _message: &NSString,
            _frame: &WKFrameInfo,
            handler: &block2::DynBlock<dyn Fn(Bool)>,
        ) {
            guarded("runJavaScriptConfirmPanel", (), || {
                self.shared_state()
                    .push(WebEvent::ScriptDialogDismissed { kind: 1 });
            });
            handler.call((Bool::NO,));
        }

        #[unsafe(method(webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:))]
        fn run_prompt(
            &self,
            _web_view: &WKWebView,
            _prompt: &NSString,
            _default_text: Option<&NSString>,
            _frame: &WKFrameInfo,
            handler: &block2::DynBlock<dyn Fn(*mut NSString)>,
        ) {
            guarded("runJavaScriptTextInputPanel", (), || {
                self.shared_state()
                    .push(WebEvent::ScriptDialogDismissed { kind: 2 });
            });
            handler.call((std::ptr::null_mut(),));
        }

        /// **The camera and the microphone, refused by name.**
        ///
        /// Apple documents the unimplemented case as *prompt*, not as *deny*, so
        /// this is one of the places where not writing the method would have
        /// been a permission dialog on somebody's screen. The Windows arm denies
        /// every capability through one `PermissionRequested` event; here each
        /// capability is its own method, which is the port's fourth stated
        /// difference.
        #[unsafe(method(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:))]
        fn media_capture(
            &self,
            _web_view: &WKWebView,
            _origin: &WKSecurityOrigin,
            _frame: &WKFrameInfo,
            _kind: WKMediaCaptureType,
            handler: &block2::DynBlock<dyn Fn(WKPermissionDecision)>,
        ) {
            handler.call((WKPermissionDecision::Deny,));
        }

        #[unsafe(method(webView:requestDeviceOrientationAndMotionPermissionForOrigin:initiatedByFrame:decisionHandler:))]
        fn device_orientation(
            &self,
            _web_view: &WKWebView,
            _origin: &WKSecurityOrigin,
            _frame: &WKFrameInfo,
            handler: &block2::DynBlock<dyn Fn(WKPermissionDecision)>,
        ) {
            handler.call((WKPermissionDecision::Deny,));
        }
    }
);

impl Gate {
    fn new(mtm: MainThreadMarker, shared: Rc<Shared>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(shared);
        // SAFETY: `NSObject`'s designated initialiser on a freshly allocated
        // instance of this class.
        unsafe { msg_send![super(this), init] }
    }

    fn shared_state(&self) -> &Rc<Shared> {
        self.ivars()
    }

    /// **The whole of the navigation policy**, separated from the callback so
    /// that the callback is three lines and this is one sentence per door.
    fn decide(
        &self,
        action: &WKNavigationAction,
        renavigate_to: &mut Option<String>,
    ) -> WKNavigationActionPolicy {
        let shared = self.shared_state();
        // SAFETY: a live action from WebKit, on the main thread.
        let request = unsafe { action.request() };
        let uri = url_of(&request);

        // **A window, and this host opens none.** `targetFrame` is nil when the
        // action would land in a window that does not exist yet —
        // `target=_blank` and `window.open`. The Windows arm answers
        // `NewWindowRequested` with `SetHandled(true)` and queues nothing; this
        // is that, at the earlier of the two doors, and
        // `createWebViewWithConfiguration:` shuts the other.
        let Some(target) = (unsafe { action.targetFrame() }) else {
            return WKNavigationActionPolicy::Cancel;
        };

        // **A download, refused before it starts.** The engine says so on the
        // action when the reader asked for one — a `download` attribute, the
        // Option key, the context menu — and the response callback catches the
        // other half, where the *server* said so.
        // SAFETY: as above.
        if unsafe { action.shouldPerformDownload() } {
            shared.push(WebEvent::DownloadStarting {
                uri,
                file_name: String::new(),
            });
            return WKNavigationActionPolicy::Cancel;
        }

        // SAFETY: a live frame from WebKit.
        if !unsafe { target.isMainFrame() } {
            // **A frame is not the seat going anywhere**, so it is the third
            // door's question and not the first's — the same split
            // `FrameNavigationStarting` makes on the other platform, arriving
            // here as one callback that has to tell them apart itself.
            if matches!(
                (shared.request_gate)(&uri, WebRequestKind::Document),
                WebRequestVerdict::Allow
            ) {
                return WKNavigationActionPolicy::Allow;
            }
            shared.push(WebEvent::RequestRefused { uri });
            return WKNavigationActionPolicy::Cancel;
        }

        // The rewrite already in flight arrives here as an ordinary candidate. It
        // is not offered to the policy a second time: normalisation is
        // idempotent, so a second answer could only be the same one, and asking
        // anyway is what a loop looks like from the inside.
        let in_flight = shared.rewriting_to.borrow().as_deref() == Some(uri.as_str());
        let verdict = if in_flight {
            *shared.rewriting_to.borrow_mut() = None;
            WebNavigationVerdict::Proceed
        } else {
            (shared.gate)(&uri)
        };
        let cancelled = match &verdict {
            WebNavigationVerdict::Proceed => false,
            WebNavigationVerdict::Cancel => {
                *shared.rewriting_to.borrow_mut() = None;
                true
            }
            WebNavigationVerdict::CancelAndNavigateTo(substitute) => {
                *shared.rewriting_to.borrow_mut() = Some(substitute.clone());
                *renavigate_to = Some(substitute.clone());
                true
            }
        };
        shared.push(WebEvent::NavigationStarting { uri, cancelled });
        if cancelled {
            WKNavigationActionPolicy::Cancel
        } else {
            WKNavigationActionPolicy::Allow
        }
    }
}

// ── the host ───────────────────────────────────────────────────────────────

/// One web preview's engine, on this platform.
///
/// Owns the page's view and, through it, the page's own process. Everything it
/// does is a step the caller's state machine told it to take.
pub struct WebHost {
    shared: Rc<Shared>,
    door: Rc<ThirdDoor>,
    /// The page, once [`WebHost::install`] has taken it into service.
    view: Option<Retained<WKWebView>>,
    /// The delegate object, held for as long as the view is: WebKit's delegate
    /// properties are **weak**, so a gate nobody kept would be deallocated and
    /// the page would have no policy at all.
    gate: Option<Retained<Gate>>,
    /// The view a [`WebHost::request_controller`] made, and the generation it was
    /// made for (R2-13).
    ///
    /// Named by its generation for the Windows arm's reason, which is not about
    /// callbacks here but is about the same defect: a seat closed and asked
    /// again has two of these, and an `install` for the second generation taking
    /// the first one's page is a live page pointed at a window that has already
    /// let go of it.
    pending_view: Option<(u64, Retained<WKWebView>, Retained<Gate>)>,
    /// The window the page's view belongs to, kept so that
    /// [`WebHost::focus_page`] can reach the responder chain.
    window: Option<NativeWindow>,
}

/// What every door of this host answers with when the page is not there.
fn no_page(what: &str) -> String {
    format!("{what}: this seat has no page")
}

impl WebHost {
    /// A host that has not started anything yet.
    ///
    /// The three closures are the seat's own policy and are held for the life of
    /// the host: `gate` is asked where the main frame is going, `request_gate`
    /// where a subframe is, and `wake` gets the event loop to come and
    /// [`WebHost::drain`].
    #[must_use]
    pub fn new(
        gate: Box<dyn Fn(&str) -> WebNavigationVerdict>,
        request_gate: WebRequestGate,
        wake: Box<dyn Fn()>,
    ) -> Self {
        let shared = Rc::new(Shared {
            events: RefCell::new(VecDeque::new()),
            chords: RefCell::new(Vec::new()),
            gate,
            request_gate,
            rewriting_to: RefCell::new(None),
            last_status: Cell::new(0),
            found: RefCell::new(String::new()),
            found_case: Cell::new(false),
            wake,
        });
        Self {
            door: Rc::new(ThirdDoor::new(Rc::clone(&shared))),
            shared,
            view: None,
            gate: None,
            pending_view: None,
            window: None,
        }
    }

    /// Everything the engine has said since the last time it was asked.
    #[must_use]
    pub fn drain(&self) -> Vec<WebEvent> {
        self.shared.events.borrow_mut().drain(..).collect()
    }

    /// The chords the window takes back from a focused page. Kept and not read —
    /// see [`Shared::chords`].
    pub fn set_claimed_chords(&self, chords: Vec<WebChord>) {
        *self.shared.chords.borrow_mut() = chords;
    }

    /// **The caller's resource rule in its compiled spelling** (M4-2,
    /// `docs/DESIGN.md` §13.29).
    ///
    /// Said by the seat once when it opens and again before every navigation,
    /// because the rule moves when the mint does. What happens next depends on
    /// how far the seat has got: before the page exists this is remembered, and
    /// once it exists the list is compiled and swapped — which X-2 measured
    /// working on a page that was already on the glass.
    ///
    /// It cannot refuse for a compilation that has not happened yet: the answer
    /// arrives in a completion block, and what depends on it — the first
    /// navigation, and `WebGuards::resource_requests` — waits for it rather than
    /// being told something now that may not be true.
    pub fn set_request_rules(&self, rules: &str) -> Result<(), String> {
        if *self.door.wanted.borrow() == rules {
            return Ok(());
        }
        *self.door.wanted.borrow_mut() = rules.to_owned();
        if self.door.page.borrow().is_some() {
            window_thread("the page's resource rules")?;
            compile(&self.door);
        }
        Ok(())
    }

    /// Whether a page is in service.
    #[must_use]
    pub fn has_controller(&self) -> bool {
        self.view.is_some()
    }

    /// **Ask for the engine**, reporting the answer as a
    /// [`WebEvent::Environment`] for this generation.
    ///
    /// There is no process-wide environment on this platform — WebKit is part of
    /// the operating system and a `WKWebView` is made on the spot — so what this
    /// step really establishes is the one thing that *is* shared and does live in
    /// a folder: the [`WKContentRuleListStore`] the third door is compiled into.
    /// A seat whose store could not be made has no third door, and the caller's
    /// own rule is that a local file is not opened without one, so the refusal
    /// belongs here rather than three steps later.
    ///
    /// The event is queued on the spot rather than from a callback, which is the
    /// same shape the Windows arm takes when the environment is already cached.
    pub fn request_environment(&mut self, folder: &Path, generation: u64) -> Result<(), String> {
        let what = "the page's engine";
        let mtm = window_thread(what)?;
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("{what}: {} could not be made: {error}", folder.display()))?;
        let path = NSString::from_str(&folder.to_string_lossy());
        // SAFETY: a string this function made, on the main thread.
        let url = NSURL::fileURLWithPath(&path);
        // SAFETY: a live file URL naming a directory this call has just made.
        let store = unsafe { WKContentRuleListStore::storeWithURL(Some(&url), mtm) }
            .ok_or_else(|| format!("{what}: no rule list store at {}", folder.display()))?;
        *self.door.store.borrow_mut() = Some(store);
        self.shared.push(WebEvent::Environment {
            generation,
            error: None,
        });
        Ok(())
    }

    /// **Make the page**, reporting the answer as a [`WebEvent::Controller`] for
    /// this generation.
    ///
    /// Two steps and not one, exactly as on Windows, and the second half is
    /// genuinely asynchronous here too — for a different reason. Making a
    /// `WKWebView` is a call that returns; compiling the seat's rule list is a
    /// completion block, and a page whose third door is not yet on it is not a
    /// page this host will let anybody navigate. So the view is made now and the
    /// event is queued when the list is on.
    pub fn request_controller(
        &mut self,
        window: NativeWindow,
        generation: u64,
    ) -> Result<(), String> {
        let what = "the page";
        let mtm = window_thread(what)?;
        if self.door.store.borrow().is_none() {
            return Err(format!("{what}: no engine has been asked for yet"));
        }
        self.window = Some(window);

        // SAFETY: every call in this block is a WebKit constructor or setter on
        // the main thread, against objects this function has just made.
        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };

        // **The store, and it is the persistent one.**
        //
        // `+[WKWebsiteDataStore defaultDataStore]` is the application's own,
        // inside the container keyed on the bundle identifier — which is why the
        // plan builds the bundle from M1 rather than from M5 (§4.5). That is the
        // promise `%LOCALAPPDATA%\Folio\WebView2` makes on the other platform: a
        // page a reader signed into is a page still signed in tomorrow, and
        // `SECURITY.md`'s web-preview paragraph records the cost of that on
        // both. X-2 recommended the non-persistent store instead; that is a
        // change to what the *product* promises rather than a question about how
        // this platform is spelled, so it is not taken here — the port keeps the
        // behaviour and M4-3 states it.
        let store = unsafe { WKWebsiteDataStore::defaultDataStore(mtm) };
        unsafe { configuration.setWebsiteDataStore(&store) };

        // **The bridge, shut by construction.** `IsWebMessageEnabled(false)` and
        // `AreHostObjectsAllowed(false)` are two calls on the other engine; here
        // a page has no way out and the host no way in unless a script message
        // handler or a user script is added to this controller, and none is.
        let controller = unsafe { WKUserContentController::new(mtm) };
        unsafe { configuration.setUserContentController(&controller) };

        // **Script on, said rather than inherited** — [`WEB_SETTINGS`]'s row and
        // its reason: a value that matches a default by accident is a value that
        // moves when the default does.
        let pages = unsafe { configuration.defaultWebpagePreferences() };
        unsafe { pages.setAllowsContentJavaScript(true) };
        // A page may not open a window, and the refusal is written twice: here,
        // where the engine is told not to let script try, and at the two delegate
        // doors that answer if it does anyway.
        let preferences = unsafe { configuration.preferences() };
        unsafe { preferences.setJavaScriptCanOpenWindowsAutomatically(false) };

        let view = unsafe {
            WKWebView::initWithFrame_configuration(
                WKWebView::alloc(mtm),
                NSRect::ZERO,
                &configuration,
            )
        };
        let gate = Gate::new(mtm, Rc::clone(&self.shared));

        // **The slot a page arrives into belongs to the generation it was asked
        // for** (R2-13). One standing here belongs to an attempt this one
        // supersedes, and is closed rather than dropped.
        self.close_pending_controller();

        // **A new page is a new mint, and it inherits nothing** (RA-3). The
        // list this door was reporting went onto the controller that is being
        // replaced; it is not on this one, so `attached`, the objects behind it
        // and `stands` are cleared rather than carried over — otherwise
        // `settled()` would answer true off the last page's work and this page
        // would be certified as gated without a list ever being put on it.
        // `refused` goes with them: it is what a compiler said about a page
        // that is gone.
        //
        // **`compiling` is deliberately not cleared.** A compile really is
        // still in flight, and clearing the latch here would start a second one
        // beside it. Moving the mint is what makes that block harmless:
        // `compile_is_stale` drops what it is holding, un-latches the door and
        // compiles again, for this page.
        self.door.mint.set(self.door.mint.get().wrapping_add(1));
        *self.door.attached.borrow_mut() = None;
        *self.door.on_the_page.borrow_mut() = None;
        *self.door.refused.borrow_mut() = None;
        self.door.stands.set(false);

        *self.door.page.borrow_mut() = Some(view.clone());
        self.door.owed.set(Some(generation));
        self.pending_view = Some((generation, view, gate));

        // And the third door, whose answer is what the event waits for.
        compile(&self.door);
        Ok(())
    }

    /// **Take the page into service**: the same five-step walk the Windows arm
    /// makes, with the same undo.
    ///
    /// Nothing navigates here, and that is the point — the delegates go on
    /// before the first load, because a load started a moment earlier would run
    /// before there was a policy to check it.
    pub fn install(
        &mut self,
        compositor: &Compositor,
        page: PageVisual,
        generation: u64,
    ) -> Result<WebInstallReport, String> {
        let mut guards = WebGuards::none();
        let mut unapplied = Vec::new();
        let mut failure_at = None;
        for step in INSTALL_SEQUENCE {
            let done = match step {
                InstallStep::TakeController => self.take_the_page(generation),
                InstallStep::Configure => self.configure().map(|could_not| unapplied = could_not),
                InstallStep::AttachEvents => self.attach_delegates(&mut guards),
                // **There is no process-wide engine to subscribe to.** WebView2
                // has one environment per user data folder and two events on it;
                // WebKit has neither, so this step is honestly empty and
                // `install_rollback`'s `environment_events` has nothing to take
                // back.
                InstallStep::AttachEnvironmentEvents => Ok(()),
                InstallStep::PointAtVisual => self.stand_in_the_slot(compositor, page),
            };
            if let Err(error) = done {
                failure_at = Some((step, error));
                break;
            }
        }
        let Some((failed_at, error)) = failure_at else {
            // **The handler is only half of that guard**, which is the Windows
            // arm's line and is true here for a different reason: the panel
            // methods *are* the switch, so a build in which that row did not
            // stand would be a build whose pages open modal windows of their
            // own however well the delegate went on.
            guards.script_dialogs &= !unapplied.contains(&WebSetting::DefaultScriptDialogs);
            return Ok(WebInstallReport { guards, unapplied });
        };
        if install_rollback(failed_at).controller {
            self.close_the_page();
        }
        Err(error)
    }

    /// Take the view [`Self::request_controller`] made for `generation`.
    fn take_the_page(&mut self, generation: u64) -> Result<(), String> {
        let (opened_for, view, gate) = self
            .pending_view
            .take()
            .ok_or_else(|| String::from("no page has been made for this seat"))?;
        if opened_for != generation {
            // The page belongs to an attempt this seat has moved on from. It is
            // closed rather than adopted, for the reason it would have been
            // closed had the caller's own machine caught it.
            self.pending_view = Some((opened_for, view, gate));
            self.close_pending_controller();
            return Err(format!(
                "the page that answered was asked for by generation {opened_for}, not {generation}"
            ));
        }
        self.view = Some(view);
        self.gate = Some(gate);
        Ok(())
    }

    /// **Walk [`WEB_SETTINGS`] and answer for every row of it.**
    ///
    /// A match and not a run of calls, for the table's own reason: what has to be
    /// right is the *set*, and a switch added to that table has to be answered on
    /// both platforms or this will not compile.
    ///
    /// Five of the nine are true here **by construction** rather than by a call,
    /// and writing them out is the point: WKWebView has no message bridge unless
    /// one is added, no host object mechanism at all, no status bar, and neither
    /// autofill nor a password store — those are Safari's and not the
    /// framework's.
    fn configure(&self) -> Result<Vec<WebSetting>, String> {
        let view = self
            .view
            .as_ref()
            .ok_or_else(|| no_page("configuring the page"))?;
        // SAFETY: a live view on the main thread. The configuration is a copy,
        // and the preferences objects inside it are the page's own.
        let configuration = unsafe { view.configuration() };
        let mut unapplied = Vec::new();
        for (setting, wanted) in WEB_SETTINGS {
            let stands = match setting {
                WebSetting::WebMessage
                | WebSetting::HostObjects
                | WebSetting::StatusBar
                | WebSetting::GeneralAutofill
                | WebSetting::PasswordAutosave => !wanted,
                // The page's own modal windows are shown only if the UI delegate
                // implements the three panel methods, and this host's does —
                // answering each immediately, which is what makes "off" true.
                WebSetting::DefaultScriptDialogs => !wanted,
                WebSetting::DevTools => {
                    // SAFETY: a live view on the main thread — asked as well as
                    // told, for the Windows arm's reason: a switch a build
                    // would not take is a switch this seat has to report.
                    unsafe { view.setInspectable(wanted) };
                    let took = unsafe { view.isInspectable() };
                    took == wanted
                }
                WebSetting::Script => {
                    // SAFETY: a live preferences object belonging to this page.
                    let allows = unsafe {
                        configuration
                            .defaultWebpagePreferences()
                            .allowsContentJavaScript()
                    };
                    allows == wanted
                }
                // **The one row this engine has no switch for.** A WKWebView
                // builds its context menu itself and no public property turns it
                // off, so the row is reported rather than quietly assumed. It is
                // a `Preference`, so nothing refuses a page over it, and the one
                // line it costs lands on the diagnostics stream where a reader
                // can find it.
                WebSetting::DefaultContextMenus => wanted,
            };
            if !stands {
                unapplied.push(setting);
            }
        }
        Ok(unapplied)
    }

    /// Put the policy on the page. **One object for both protocols**, which X-2
    /// measured serving the whole matrix.
    fn attach_delegates(&self, guards: &mut WebGuards) -> Result<(), String> {
        let what = "attaching the page's policy";
        let view = self.view.as_ref().ok_or_else(|| no_page(what))?;
        let gate = self.gate.as_ref().ok_or_else(|| no_page(what))?;
        let gate: &Gate = gate;
        // SAFETY: a live view and a live delegate object, on the main thread. The
        // delegate properties are weak and this host is what keeps the object
        // alive — see the field's own note.
        unsafe {
            view.setNavigationDelegate(Some(ProtocolObject::from_ref(gate)));
            view.setUIDelegate(Some(ProtocolObject::from_ref(gate)));
        }
        // `decidePolicyForNavigationAction:` is asked about a subframe as well as
        // the main frame — X-2's row 4 — so the frame gate is this one
        // subscription rather than a second event.
        guards.frame_navigation = true;
        // The three panel methods are on the class above, so they are on every
        // instance of it.
        guards.script_dialogs = true;
        // And the third door is the compiled list, which is on **this** page or
        // is not — asked rather than remembered, because this line is what puts
        // the word *guarded* in front of a reader and in `WebInstallReport`
        // (RA-3). See [`ThirdDoor::list_is_on_this_page`] for the two answers it
        // makes agree.
        guards.resource_requests = self.door.list_is_on_this_page();
        Ok(())
    }

    /// **Hand the page's own view to the window's composition.**
    ///
    /// The opposite direction from the other platform, and `attach_page_view`'s
    /// own note says why: WebView2 renders into a visual the host made, and
    /// WebKit makes its own view for the host to take. The slot is where it goes
    /// and the slot is what sizes it (M4-1, §13.24).
    fn stand_in_the_slot(&self, compositor: &Compositor, page: PageVisual) -> Result<(), String> {
        let view = self
            .view
            .as_ref()
            .ok_or_else(|| no_page("hosting the page"))?;
        compositor.attach_page_view(page, native_window_of(view))
    }

    /// **Move this live page to another window.**
    ///
    /// Nothing navigates, nothing reloads and nothing is rebuilt: the same view,
    /// the same process and the same document come out the other side, which is
    /// the difference between a page that was moved and a page that was opened
    /// again at the same address.
    ///
    /// Where the Windows arm walks nine compensable steps, this is one call. A
    /// view has exactly one superview, and `addSubview:` on the target's slot is
    /// what takes it out of the source's — so there is no moment in which the
    /// page belongs to both windows, and therefore no half-moved state for a
    /// compensation to undo. A failure leaves the page where it was, which is
    /// [`RehostOutcome::KeptSource`] with nothing compensated.
    pub fn rehost(
        &mut self,
        from: &RehostSide<'_>,
        to: &RehostSide<'_>,
        rect: (i32, i32, u32, u32),
        visible: bool,
    ) -> RehostOutcome {
        let keep = |error: String| RehostOutcome::KeptSource {
            failed_at: RehostStep::Hide,
            error,
            compensation: RehostCompensation::default(),
        };
        let Some(view) = self.view.clone() else {
            return keep(no_page("moving the page to another window"));
        };
        if let Err(error) = to
            .compositor
            .attach_page_view(to.page, native_window_of(&view))
        {
            return keep(error);
        }
        let clip = (0.0, 0.0, rect.2 as f32, rect.3 as f32);
        if let Err(error) = to
            .compositor
            .place_web_visual(to.page, (rect.0, rect.1), clip)
        {
            return keep(error);
        }
        if let Err(error) = self.set_visible(visible) {
            return keep(error);
        }
        // The source's slot comes down last, and only once the page is really
        // standing in the target's: a slot taken down first would be a frame in
        // which the page is in no window at all.
        let _ = from.compositor.detach_web_visual(from.page);
        self.window = Some(to.window);
        RehostOutcome::Moved
    }

    /// Where the seat is. **Nothing to tell**, and that is a fact about this
    /// platform rather than a door nobody wrote: the page's view is a subview of
    /// the page's slot with both autoresizing masks on, so the rectangle it
    /// occupies is the one `Compositor::place_web_visual` gave the slot. A second
    /// writer of one rectangle is how two clocks come to disagree.
    pub fn set_bounds(&self, x: i32, y: i32, width: u32, height: u32) -> Result<(), String> {
        let _ = (x, y, width, height);
        Ok(())
    }

    /// The scale a page rasterizes at. Nothing to tell: a view in a window
    /// rasterizes at that window's backing scale, and AppKit moves it itself when
    /// the window changes screens.
    pub fn set_rasterization_scale(&self, scale: f64) -> Result<(), String> {
        let _ = scale;
        Ok(())
    }

    /// The window moved. Nothing to tell, for the reason above.
    pub fn notify_parent_window_moved(&self) -> Result<(), String> {
        Ok(())
    }

    /// On the glass, or off it.
    pub fn set_visible(&self, visible: bool) -> Result<(), String> {
        let what = "showing the page";
        let view = self.view.as_ref().ok_or_else(|| no_page(what))?;
        window_thread(what)?;
        // SAFETY: a live view on the main thread.
        view.setHidden(!visible);
        Ok(())
    }

    /// **Go to an address**, or park it until the door it will be judged by is
    /// standing.
    pub fn navigate(&self, url: &str) -> Result<(), String> {
        let what = "going to an address";
        let view = self.view.as_ref().ok_or_else(|| no_page(what))?;
        window_thread(what)?;
        // **One destination, and this is it** — whether it is loaded now or
        // waited for. See [`ThirdDoor::destined_for`].
        if self.door.destined_for(url) {
            self.shared.load(view, url);
            return Ok(());
        }
        compile(&self.door);
        Ok(())
    }

    /// Read the page again.
    pub fn reload(&self) -> Result<(), String> {
        let view = self.view.as_ref().ok_or_else(|| no_page("reloading"))?;
        window_thread("reloading")?;
        // SAFETY: a live view on the main thread.
        let _ = unsafe { view.reload() };
        Ok(())
    }

    /// Stop reading. There may be nothing in flight, and that is not a failure.
    pub fn stop(&self) -> Result<(), String> {
        let Some(view) = self.view.as_ref() else {
            return Ok(());
        };
        window_thread("stopping")?;
        // SAFETY: a live view on the main thread.
        unsafe { view.stopLoading() };
        Ok(())
    }

    /// Back through the history.
    pub fn go_back(&self) -> Result<(), String> {
        let view = self.view.as_ref().ok_or_else(|| no_page("going back"))?;
        window_thread("going back")?;
        // SAFETY: a live view on the main thread.
        let _ = unsafe { view.goBack() };
        Ok(())
    }

    /// Forward through the history.
    pub fn go_forward(&self) -> Result<(), String> {
        let view = self.view.as_ref().ok_or_else(|| no_page("going forward"))?;
        window_thread("going forward")?;
        // SAFETY: a live view on the main thread.
        let _ = unsafe { view.goForward() };
        Ok(())
    }

    /// **The engine's own inspector, which no call opens.**
    ///
    /// `setInspectable(true)` is what [`WEB_SETTINGS`]'s `DevTools` row becomes
    /// here, and it is the whole of the public API: an inspectable page is opened
    /// from Safari's *Develop* menu or from the page's own context menu, and
    /// there is no message that opens one. So the verb refuses in a sentence a
    /// reader can act on rather than doing nothing quietly.
    pub fn open_dev_tools(&self) -> Result<(), String> {
        Err(String::from(
            "this page is inspectable; the inspector is opened from Safari's Develop menu",
        ))
    }

    /// The zoom the page is at. One for a seat with no page, which is what a page
    /// nobody has zoomed is at — the row reads *100%* rather than blank.
    #[must_use]
    pub fn zoom(&self) -> f64 {
        let Some(view) = self.view.as_ref() else {
            return 1.0;
        };
        if MainThreadMarker::new().is_none() {
            return 1.0;
        }
        // SAFETY: a live view on the main thread.
        unsafe { view.pageZoom() }
    }

    /// Set the zoom.
    pub fn set_zoom(&self, factor: f64) -> Result<(), String> {
        let view = self.view.as_ref().ok_or_else(|| no_page("zooming"))?;
        window_thread("zooming")?;
        // SAFETY: a live view on the main thread.
        unsafe { view.setPageZoom(factor) };
        Ok(())
    }

    /// **Find in page** — the search happens and the tally does not.
    ///
    /// `-[WKWebView findString:withConfiguration:completionHandler:]` scrolls to
    /// a match and highlights it, and answers one boolean: whether there was one.
    /// It counts nothing. So no [`WebEvent::FindMatches`] leaves this arm, and
    /// the seat's capsule has no number to show rather than a wrong one — the
    /// port's sixth stated difference.
    pub fn find(&self, term: &str, case_sensitive: bool) -> Result<(), String> {
        self.search(term, case_sensitive, true)
    }

    /// The next match, in either direction.
    ///
    /// WebKit keeps the term for itself and has no "again" call, so stepping is
    /// the same search asked once more — and a seat that has searched for nothing
    /// has nothing to step through, which is an `Ok` and not a failure.
    pub fn find_step(&self, forwards: bool) -> Result<(), String> {
        let term = self.shared.found.borrow().clone();
        if term.is_empty() {
            return Ok(());
        }
        self.search(&term, self.shared.found_case.get(), forwards)
    }

    /// Put the find away. WebKit's find keeps no session to end; what this does
    /// is forget the term, so that a later step is not a search nobody asked for.
    pub fn find_stop(&self) -> Result<(), String> {
        self.shared.found.borrow_mut().clear();
        Ok(())
    }

    fn search(&self, term: &str, case_sensitive: bool, forwards: bool) -> Result<(), String> {
        let view = self.view.as_ref().ok_or_else(|| no_page("searching"))?;
        let mtm = window_thread("searching")?;
        *self.shared.found.borrow_mut() = term.to_owned();
        self.shared.found_case.set(case_sensitive);
        // SAFETY: WebKit's own configuration object and its setters, on the main
        // thread.
        let configuration = unsafe { WKFindConfiguration::new(mtm) };
        unsafe { configuration.setCaseSensitive(case_sensitive) };
        unsafe { configuration.setBackwards(!forwards) };
        unsafe { configuration.setWraps(true) };
        // The answer says only *whether* a match was found — see the note on
        // `find` — so there is nothing to report from it.
        let done = RcBlock::new(move |_result: NonNull<WKFindResult>| {});
        // SAFETY: a live view, a string this call made, and a block `RcBlock`
        // keeps alive across the call.
        unsafe {
            view.findString_withConfiguration_completionHandler(
                &NSString::from_str(term),
                Some(&configuration),
                &done,
            );
        }
        Ok(())
    }

    /// Give the page the keyboard.
    pub fn focus_page(&self) -> Result<(), String> {
        let what = "giving the page the keyboard";
        let view = self.view.as_ref().ok_or_else(|| no_page(what))?;
        let window = self.window.ok_or_else(|| no_page(what))?;
        let (_mtm, ns_window) = window_for(window, what)?;
        let responder: &NSResponder = view;
        // SAFETY: a live window and a live view in it, on the main thread.
        if ns_window.makeFirstResponder(Some(responder)) {
            self.shared.push(WebEvent::GotFocus);
            return Ok(());
        }
        Err(format!("{what}: the window would not hand it over"))
    }

    /// **The page's own process, which has no public name.**
    ///
    /// WebView2 answers this with `BrowserProcessId`; WebKit's equivalent is
    /// private. Zero is what a host with no page answers on the other platform
    /// too, and the backend inventory records that no caller in `bt-app` reads
    /// it.
    #[must_use]
    pub fn browser_process_id(&self) -> u32 {
        0
    }

    /// **The pointer, and why nothing is forwarded.**
    ///
    /// WebView2 in composition hosting receives no input of its own, so the
    /// Windows arm hands it every press and move. A `WKWebView` is a real
    /// `NSView` in the window's own hierarchy and AppKit delivers to it directly
    /// — there is nothing for this door to send.
    ///
    /// **What that leaves open is not this door's**: the page's slot stands
    /// *under* Folio's own surface view (§13.24), so whether an event reaches the
    /// page at all is a question about that view's hit testing, which neither
    /// M4-1 nor this ticket settles. §13.29 writes it down and carries it
    /// forward rather than answering it here with a synthesized event.
    pub fn send_mouse(
        &self,
        event: WebMouseEvent,
        point: (i32, i32),
        buttons_down: u32,
    ) -> Result<(), String> {
        let _ = (event, point, buttons_down);
        Ok(())
    }

    /// **A picture of the page, for the focus card.**
    ///
    /// `-[WKWebView takeSnapshotWithConfiguration:completionHandler:]` is the
    /// call and it answers an `NSImage`; turning one into the PNG bytes
    /// [`WebEvent::Captured`] carries is an AppKit encode this ticket does not
    /// take on. The refusal is a supported outcome — `Captured { png: None }`
    /// already means "the seat goes on showing the last picture it had" — and it
    /// is a sentence rather than silence so that the gap is visible.
    pub fn capture_preview(&self) -> Result<(), String> {
        Err(String::from(
            "a picture of the page is not taken on this platform yet",
        ))
    }

    /// **The site's own icon, which this engine never mentions.**
    ///
    /// WebView2 announces `FaviconChanged` and answers `GetFavicon`; WKWebView
    /// has neither, in any public form. So no [`WebEvent::FaviconChanged`] ever
    /// leaves this arm and this verb refuses — the seat wears the product's own
    /// mark rather than a site's.
    pub fn get_favicon(&self) -> Result<(), String> {
        Err(String::from(
            "this engine does not tell a host what icon a page wears",
        ))
    }

    /// Close a page nobody came for (R2-13).
    pub fn close_pending_controller(&mut self) {
        let Some((_, view, _)) = self.pending_view.take() else {
            return;
        };
        // The borrow is taken and let go of before the write: a `RefCell`
        // borrowed inside an `if` condition is still borrowed in the body.
        let was_the_door_s = {
            let page = self.door.page.borrow();
            page.as_ref()
                .is_some_and(|page| std::ptr::eq(&**page, &*view))
        };
        if was_the_door_s {
            *self.door.page.borrow_mut() = None;
            self.door.owed.set(None);
        }
        release_the_view(&view);
    }

    /// **Close the host**, walking [`WEB_CLOSE_STEPS`].
    ///
    /// A match over the table and not a run of statements, for the table's own
    /// reason: what has to be right is the set, and a row added to it has to be
    /// answered here or this will not compile.
    ///
    /// It must not refuse: this runs on the way out of a seat and on the way out
    /// of the process.
    pub fn close(&mut self) {
        for step in WEB_CLOSE_STEPS {
            match step {
                // One object is all three on this platform: the view is the
                // controller, the composition and the page.
                CloseStep::Controller | CloseStep::Composition | CloseStep::Webview => {
                    self.close_the_page();
                }
                // There is no process-wide engine here, so nothing was ever
                // subscribed to one.
                CloseStep::EnvironmentEvents => {}
                // The rule list store this host cached, and everything the third
                // door was holding. Letting go of it is what makes a rebuild ask
                // for its own rather than adopt the one belonging to the seat it
                // is rebuilding away from (R2-12).
                CloseStep::CachedEnvironment => self.door.let_go(),
                CloseStep::PendingController => self.close_pending_controller(),
                // WebKit's find keeps no session, so there is no latch that could
                // be a statement about a page that is gone — see `find`.
                CloseStep::FindLatch => self.shared.found.borrow_mut().clear(),
            }
        }
    }

    /// Let go of the page in service: off the glass, out of the window, and with
    /// nothing left pointing at a delegate that is about to be dropped.
    fn close_the_page(&mut self) {
        if let Some(view) = self.view.take() {
            release_the_view(&view);
        }
        self.gate = None;
    }

    /// **What the engine says about who owns this page's device scale.**
    ///
    /// A fact rather than a setting: AppKit rasterizes a view at the backing
    /// scale of the window it is in and changes it itself when the window changes
    /// screens. So the engine detects the change, the scale is the window's, and
    /// the bounds this host sets are not raw pixels because this host sets none.
    #[must_use]
    pub fn dpi_ownership(&self) -> Option<WebDpiOwnership> {
        self.view.as_ref()?;
        let window = self.window?;
        let (_mtm, ns_window) = window_for(window, "the page's scale").ok()?;
        Some(WebDpiOwnership {
            detects_monitor_scale_changes: true,
            // SAFETY: a live window on the main thread.
            rasterization_scale: ns_window.backingScaleFactor(),
            bounds_mode_is_raw_pixels: false,
        })
    }
}

/// A page's view, taken off the glass and left pointing at nothing.
///
/// The delegate properties are cleared **first**: WebKit holds them weakly, but a
/// view that is still loading when its delegate is dropped would call into a
/// deallocated object, and the order here is what makes that impossible.
fn release_the_view(view: &WKWebView) {
    if MainThreadMarker::new().is_none() {
        // Nothing here is safe off the window's thread, and a close that refused
        // would be a close nobody could perform on the way out of a process. The
        // view is dropped by the caller either way.
        eprintln!("BT_MAC_WEB a page was let go of off the window's thread");
        return;
    }
    // SAFETY: a live view on the main thread.
    unsafe {
        view.stopLoading();
        view.setNavigationDelegate(None);
        view.setUIDelegate(None);
    }
    let as_view: &NSView = view;
    as_view.removeFromSuperview();
}

/// The page's view as the handle `Compositor::attach_page_view` takes.
///
/// A `NativeWindow` on this platform *is* an `NSView` pointer — see the type's
/// own note — so this is a cast and not a conversion.
fn native_window_of(view: &WKWebView) -> NativeWindow {
    let as_view: &NSView = view;
    NativeWindow::from_appkit(NonNull::from(as_view).cast())
}

/// Drop the process-wide environment. **There is none**: WebKit is part of the
/// operating system, a `WKWebView` is made on the spot, and what a seat caches
/// for itself — the content rule list store — is let go of by
/// [`CloseStep::CachedEnvironment`].
pub fn forget_web_environment() {}

/// **Which engine is installed.**
///
/// On Windows this asks the loader whether the Evergreen runtime is here at all,
/// and a failure is the card that tells a reader to install it. On a platform
/// whose web engine ships with the operating system the honest answer is always
/// `Ok`, and the version is WebKit's own — asked of the framework rather than
/// derived from the system version, on this repository's standing rule about
/// machine facts.
pub fn webview2_runtime_version() -> Result<String, String> {
    let identifier = NSString::from_str("com.apple.WebKit");
    let key = NSString::from_str("CFBundleShortVersionString");
    let version = NSBundle::bundleWithIdentifier(&identifier)
        .and_then(|bundle| bundle.objectForInfoDictionaryKey(&key))
        .and_then(|value| value.downcast::<NSString>().ok())
        .map(|text| text.to_string());
    Ok(version.unwrap_or_else(|| String::from("WebKit")))
}

// ── the third door's arithmetic, asked without WebKit ──────────────────────

/// **Which completion belongs to which page, and who is told what** (RA-3).
///
/// Everything WebKit is in `tests/macos_webview.rs`, which needs a bundle, a
/// window server and the process's own `main` thread and therefore runs on one
/// machine on purpose. What is left over is a small state machine —
/// [`ThirdDoor`], [`compile_is_stale`] and [`rules_compiled`] — and none of it
/// touches an Objective-C object, so it is asked here, where an ordinary
/// `cargo test -p bt-platform` on a Mac will run it.
#[cfg(test)]
mod door_tests {
    use super::*;

    /// A door with no store, no page and no WebKit near it, holding the rule the
    /// seat has stated.
    fn a_door(wanted: &str) -> Rc<ThirdDoor> {
        let shared = Rc::new(Shared {
            events: RefCell::new(VecDeque::new()),
            chords: RefCell::new(Vec::new()),
            gate: Box::new(|_: &str| WebNavigationVerdict::Proceed),
            request_gate: Box::new(|_: &str, _| WebRequestVerdict::Allow),
            rewriting_to: RefCell::new(None),
            last_status: Cell::new(0),
            found: RefCell::new(String::new()),
            found_case: Cell::new(false),
            wake: Box::new(|| {}),
        });
        let door = Rc::new(ThirdDoor::new(shared));
        *door.wanted.borrow_mut() = wanted.to_owned();
        door
    }

    /// **The completion block's body with its two WebKit calls taken out** —
    /// the staleness question, and then the bookkeeping. It is written here in
    /// the same order the block writes it so that the thing under test is the
    /// thing that ships; the calls it leaves out (`removeAllContentRuleLists`,
    /// `addContentRuleList`) change the page, not the door.
    ///
    /// `compiled_from` is the rule the compile went out with, which is the
    /// block's own captured copy: on the success path it is what the list was
    /// built from, and on the failure path it is what would not build.
    fn land(door: &Rc<ThirdDoor>, mint: u64, compiled_from: &str, outcome: Result<String, String>) {
        if compile_is_stale(door, mint, compiled_from) {
            return;
        }
        rules_compiled(door, outcome);
    }

    /// Everything the door has told the seat since it was last asked.
    fn said(door: &Rc<ThirdDoor>) -> Vec<WebEvent> {
        door.shared.events.borrow_mut().drain(..).collect()
    }

    /// RED — **a rule list that arrives after its page was replaced changes
    /// nothing**, which is the whole of RA-3.
    ///
    /// The shape that makes this invisible without a mint: the seat's rule has
    /// **not** changed, so the list page 1 compiled is byte-identical to the one
    /// page 2 wants. Every test of `settled()` passes, the JSON matches, and the
    /// list is nevertheless sitting on a controller that has been thrown away.
    ///
    /// MUTATION: drop the mint comparison in [`compile_is_stale`] and this goes
    /// red on four counts at once — `stands`, `attached`, the answer page 2 is
    /// still owed, and the `WebEvent::Controller { error: None }` that would
    /// certify page 2 as gated by a list nothing ever put on it.
    #[test]
    fn a_rule_list_that_arrives_after_its_page_was_replaced_changes_nothing() {
        let rules = r#"[{"trigger":{"url-filter":".*"},"action":{"type":"block"}}]"#;
        let door = a_door(rules);

        // Page 1 asks, and its compile goes out.
        door.owed.set(Some(1));
        door.compiling.set(true);
        let first = door.mint.get();

        // Page 2 takes over — `request_controller`'s effect on the door, with
        // the seat's rule unchanged.
        door.mint.set(first + 1);
        *door.attached.borrow_mut() = None;
        *door.refused.borrow_mut() = None;
        door.stands.set(false);
        door.owed.set(Some(2));

        // Page 1's list lands, successfully, compiled from exactly what page 2
        // wants.
        land(&door, first, rules, Ok(rules.to_owned()));

        assert!(
            !door.stands.get(),
            "a list on a controller that is gone is not a door standing"
        );
        assert!(!door.stands_for_this_mint());
        assert!(
            door.attached.borrow().is_none(),
            "nothing is attached to page 2 by page 1's compile"
        );
        assert!(
            door.refused.borrow().is_none(),
            "nothing was refused either"
        );
        assert_eq!(
            door.owed.get(),
            Some(2),
            "page 2 is still owed its own answer"
        );
        assert!(
            said(&door).is_empty(),
            "an obsolete completion answers nobody"
        );
        assert!(
            !door.compiling.get(),
            "and it still un-latches the door, or the page that took over never \
             gets a list at all"
        );
    }

    /// RED — **a rule list compiled for a resource rule the seat has moved off
    /// is never put on the page** (audit 3 A-1).
    ///
    /// The shape that makes this invisible without the policy: the *page* has
    /// not changed, so every question about the mint answers "this is current".
    /// One seat, one controller, and the seat's rule flipped category and back
    /// inside one compile — a reader clicking an `http://` link printed in a
    /// local report, then opening a second report out of the files column. The
    /// list that lands is the browsing seat's, and the document on the glass is
    /// a local one; attaching it opens `http` and `https` to that document for
    /// as long as the corrective compile takes.
    ///
    /// MUTATION: drop the `wanted` comparison in [`compile_is_stale`] and this
    /// goes red on `attached`, on `stands` and on the recompile — and on the
    /// real page the browsing list has by then already been added.
    #[test]
    fn a_rule_list_compiled_for_a_superseded_rule_is_never_attached() {
        const FILE_RULES: &str =
            r#"[{"trigger":{"url-filter":"^http://"},"action":{"type":"block"}}]"#;
        const BROWSING_RULES: &str =
            r#"[{"trigger":{"url-filter":"^file:"},"action":{"type":"block"}}]"#;

        let door = a_door(FILE_RULES);
        let mint = door.mint.get();
        *door.attached.borrow_mut() = Some(Attached {
            json: FILE_RULES.to_owned(),
            mint,
        });
        door.stands.set(true);
        assert!(
            door.settled(),
            "the first report is settled on its own rule"
        );

        // Gesture one: an `http://` link. The rule flips and a compile goes out
        // for the browsing seat's list.
        *door.wanted.borrow_mut() = BROWSING_RULES.to_owned();
        door.compiling.set(true);

        // Gesture two: a second report out of the files column. The rule flips
        // back before the first compile has landed, so the door is settled
        // again and the report loads under the rule it is owed.
        *door.wanted.borrow_mut() = FILE_RULES.to_owned();

        // And now gesture one's compile lands, for this very page.
        land(&door, mint, BROWSING_RULES, Ok(BROWSING_RULES.to_owned()));

        assert_eq!(
            door.attached.borrow().as_ref().map(|it| it.json.as_str()),
            Some(FILE_RULES),
            "the local report is still judged by the local report's rule"
        );
        assert!(
            door.stands.get(),
            "and the list it had is still on the page"
        );
        assert!(door.settled());
        assert!(
            said(&door).is_empty(),
            "an overtaken completion certifies nobody"
        );
        assert!(
            !door.compiling.get(),
            "the latch is cleared, or the seat never compiles again"
        );
    }

    /// RED — **a seat goes to the last address it was given, and to no other**
    /// (audit 3 A-1, the second surprise on the same path).
    ///
    /// An address handed to [`WebHost::navigate`] while the door is unsettled
    /// waits for the door. One handed over after it has settled is loaded on
    /// the spot — and that is the moment the waiting one stops being a
    /// destination. Left behind, it is loaded a compile round-trip later by
    /// [`ThirdDoor::answer_what_waited`], which takes the reader off the report
    /// they opened last and onto the address they typed before it.
    ///
    /// MUTATION: take the `parked` clear out of [`ThirdDoor::destined_for`]'s
    /// settled branch and the last two assertions go red — the seat is still
    /// holding `http://example.com/` and answers it.
    #[test]
    fn a_seat_goes_to_the_last_address_it_was_given() {
        let door = a_door("[]");
        *door.attached.borrow_mut() = Some(Attached {
            json: String::from("[]"),
            mint: door.mint.get(),
        });
        door.stands.set(true);

        // Unsettled: the address waits.
        *door.wanted.borrow_mut() = String::from("[\"other\"]");
        assert!(!door.destined_for("http://example.com/"));
        assert_eq!(
            door.parked.borrow().as_deref(),
            Some("http://example.com/"),
            "an address the door cannot judge yet is where the seat is going"
        );

        // Settled again, and a second address arrives: it loads now, and it is
        // the only place this seat is going.
        *door.wanted.borrow_mut() = String::from("[]");
        assert!(door.destined_for("file:///D:/seat/open/report2.html"));
        assert!(
            door.parked.borrow().is_none(),
            "a superseded address is not still a destination"
        );

        door.answer_what_waited();
        assert!(
            said(&door).is_empty(),
            "nothing waited, so nothing is loaded and nothing is refused"
        );
    }

    /// RED — **a close during an outstanding compile ends the in-flight state
    /// rather than latching it** (RA-3, the second half).
    ///
    /// `compiling` is the latch that makes [`compile`] return immediately.
    /// Leaving it set through a `close()` means no later `set_request_rules`,
    /// `navigate` or `request_controller` can ever start a compile again for the
    /// life of the host — and the block that eventually lands re-sets `attached`
    /// and `stands` for a page that no longer exists.
    ///
    /// MUTATION: take `compiling.set(false)` out of [`ThirdDoor::let_go`] and the
    /// first assertion goes red; take the mint bump out and the rest do.
    #[test]
    fn a_close_during_a_compile_un_latches_the_door_and_its_completion_finds_nothing() {
        let rules = "[]";
        let door = a_door(rules);
        door.owed.set(Some(1));
        door.compiling.set(true);
        let first = door.mint.get();

        door.let_go();
        assert!(!door.compiling.get(), "a close un-latches the door");

        land(&door, first, rules, Ok(rules.to_owned()));
        assert!(!door.compiling.get());
        assert!(
            !door.stands.get(),
            "a door with no page does not stand because a block landed"
        );
        assert!(door.attached.borrow().is_none());
        assert!(said(&door).is_empty(), "there is nobody left to answer");
    }

    /// The path that must not regress: **one page, one compile, one answer.**
    #[test]
    fn a_list_compiled_for_the_page_it_is_on_answers_the_seat_and_stands() {
        let rules = "[]";
        let door = a_door(rules);
        door.owed.set(Some(7));
        door.compiling.set(true);
        let mint = door.mint.get();

        land(&door, mint, rules, Ok(rules.to_owned()));

        assert!(door.stands.get());
        assert!(door.stands_for_this_mint());
        assert!(door.settled());
        assert_eq!(
            *door.attached.borrow(),
            Some(Attached {
                json: rules.to_owned(),
                mint
            })
        );
        assert_eq!(
            said(&door),
            vec![WebEvent::Controller {
                generation: 7,
                error: None
            }]
        );
        assert_eq!(door.owed.get(), None, "the debt is paid once");
        assert!(!door.compiling.get());
    }

    /// **Fail closed**: a rule the compiler will not take leaves no door, and
    /// the navigation that was waiting for it does not happen.
    #[test]
    fn a_rule_that_will_not_compile_cancels_what_was_parked_and_says_why() {
        let door = a_door("[this is not a rule list]");
        door.owed.set(Some(9));
        door.compiling.set(true);
        *door.parked.borrow_mut() = Some(String::from("file:///tmp/a.html"));
        let mint = door.mint.get();

        land(
            &door,
            mint,
            "[this is not a rule list]",
            Err(String::from("rule list parse error")),
        );

        assert!(!door.stands.get());
        assert!(!door.stands_for_this_mint());
        assert!(door.attached.borrow().is_none());
        let events = said(&door);
        assert_eq!(events.len(), 2, "{events:?}");
        match &events[0] {
            WebEvent::Controller {
                generation: 9,
                error: Some(reason),
            } => assert!(
                reason.contains("rule list parse error"),
                "the seat's fault line carries what the compiler said: {reason:?}"
            ),
            other => panic!("the seat is told its page has no third door: {other:?}"),
        }
        assert_eq!(
            events[1],
            WebEvent::NavigationStarting {
                uri: String::from("file:///tmp/a.html"),
                cancelled: true
            },
            "a navigation that cannot be judged does not happen, and the seat hears so"
        );
    }

    /// RED — **a certification is about the page it was made for**, which is
    /// why `attach_delegates` may not read `stands` on its own.
    ///
    /// `stands` is a flag and a flag remembers; the pair `stands` +
    /// `attached.mint` is the flag together with the fact it was set from, and
    /// that is what [`ThirdDoor::list_is_on_this_page`] asks before
    /// `WebGuards::resource_requests` is written.
    #[test]
    fn a_certification_is_about_the_page_it_was_made_for() {
        let rules = "[]";
        let door = a_door(rules);
        let mint = door.mint.get();
        land(&door, mint, rules, Ok(rules.to_owned()));
        assert!(
            door.stands_for_this_mint(),
            "the list is on the page it was made for"
        );

        door.mint.set(mint + 1);
        assert!(
            door.stands.get(),
            "the flag is untouched by the page moving — that is exactly the point"
        );
        assert!(
            !door.stands_for_this_mint(),
            "and the pair is not: this list is not on this page"
        );
    }
}
