//! **The delegate selectors winit's own application delegate does not
//! implement** (ticket M3-1, probe X-4; the fifth is T-MAC-DOCKMENU's).
//!
//! The AppKit half of [`app_delegate`](crate::app_delegate): everything here is
//! the route into the delegate methods, and everything about what Folio *does*
//! with them is next door.
//!
//! **Four of them are M3-1's and the fifth is the Dock tile's menu**
//! (`applicationDockMenu:`, `docs/DESIGN.md` §13.50). It is added by the very
//! same route, on the same measurement — `class_getInstanceMethod` answers null
//! for it on winit 0.30.13, so nothing of winit's is displaced — and it is the
//! one of the five that **answers with an object** rather than with a flag or
//! with nothing. What it answers with is [`crate::macos_menu::dock_menu`]'s,
//! which is where the menu and its ownership rule live.
//!
//! # The finding, and winit's own documentation is wrong about it
//!
//! winit 0.30.13's `src/platform/macos.rs` says, in the crate's own words, that
//! *"Winit guarantees that it will not register an application delegate, so the
//! solution is to register your own"*. **It does register one.**
//! `platform_impl/macos/event_loop.rs:240` calls `-[NSApplication setDelegate:]`
//! with a private `WinitApplicationDelegate`, and `app_state.rs`'s
//! `ApplicationDelegate::get` reads `NSApp.delegate` back, checks `is_kind_of`
//! and **panics** on anything else — from the CFRunLoop observers
//! (`observer.rs:62`, `:84`), on every single turn of the loop. So the
//! documented route takes winit down on the first iteration, and a forwarding
//! proxy fails the identical check for the identical reason.
//!
//! The route that works, and that X-4 ran a full session through:
//!
//! 1. **After `EventLoop::new`** — which is what registers the class with the
//!    Objective-C runtime — look it up by name,
//!    `AnyClass::get(c"WinitApplicationDelegate")`;
//! 2. `class_addMethod` the selectors winit does not implement onto that
//!    class. X-4 measured `class_getInstanceMethod` as null and `class_addMethod`
//!    as true for every one of them: nothing of winit's is displaced, and
//!    `NSApp.delegate` stays winit's own object, so its assertion never fires;
//!    every `ApplicationHandler` callback kept arriving — 829 `about_to_wait`,
//!    42 `window_event`, 9 `user_event` by the end of the run;
//! 3. **Hand winit's delegate back to `setDelegate:` once**, immediately after.
//!    AppKit caches which delegate methods exist at the moment that is called,
//!    and winit called it before Folio's own existed. X-4 measured that macOS 26.6
//!    delivers every event even without it (run `r1` skipped it), so this is one
//!    message against a cache whose behaviour is not contracted anywhere.
//!
//! **It is not undone on drop.** The Objective-C runtime has no
//! `class_removeMethod`; a method added to a class is added for the life of the
//! process. That is why [`crate::AppDelegate::install`] is once per process and
//! why the door is held for as long as the program runs.
//!
//! # Services are M4-9's, and they never touch the delegate
//!
//! A Service is *not* a selector on this class at all. `-[NSApplication
//! setServicesProvider:]` takes an object of the application's own, AppKit calls
//! the method `NSServices`' `NSMessage` key names **on that object**, and the
//! delegate is not consulted at all — X-4 registered one and read
//! `NSApp.servicesProvider` back to confirm it. M4-9 owns that object and its
//! `Info.plist` entry; what it inherits from this file is the channel, because a
//! Service delivery is [`AppDelegateEventKind::OpenPaths`] with an origin of its
//! own, and the cold-delivery buffer next door is what makes a Service that
//! arrives before `resumed` reach a window. Nothing in this module needs to
//! change for it: the provider object is registered beside `add_the_delegate_selectors`,
//! and it posts into the same [`Outbox`].
//!
//! # The thread
//!
//! Every one of these is called by AppKit on the main thread, which is the only
//! thread `NSApplication` may be touched from. These implementations
//! therefore do not gate: they are *called* by the platform rather than asked by
//! this program, and a check that cannot fail is a check that proves nothing —
//! `macos_impl`'s own note about the two doors that touch no AppKit.
//! [`reply_to_should_terminate`] **does** gate, because it is the one call in
//! this file that `bt-app` makes rather than receives.

use std::ffi::{CStr, c_char, c_void};
use std::sync::{Arc, OnceLock};

use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{MainThreadMarker, msg_send, sel};
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply};
use objc2_foundation::{NSArray, NSURL};

use crate::app_delegate::{
    AppDelegateEvent, AppDelegateEventKind, AppDelegateOrigin, Outbox, TerminationAnswer,
    path_from_file_url,
};
use crate::macos_impl::off_the_window_thread;

// ── the raw runtime, declared here because objc2 does not lend it out ───────

// `objc2` builds *new* classes with its own `ClassBuilder`; adding a method to a
// class somebody else registered is outside what it wraps, so the two entry
// points are declared against `libobjc` directly. This crate is the workspace's
// unsafe boundary and this is the eleventh thing it is a boundary against: the
// Objective-C runtime itself, rather than a framework written in it.
#[link(name = "objc", kind = "dylib")]
unsafe extern "C" {
    fn class_addMethod(
        cls: *mut AnyClass,
        name: Sel,
        imp: *const c_void,
        types: *const c_char,
    ) -> Bool;
    fn class_getInstanceMethod(cls: *const AnyClass, name: Sel) -> *const c_void;
}

/// The channel the C functions below post into.
///
/// A `static` because a method implementation is a C function with no `self` of
/// this program's — the whole reason [`crate::AppDelegate`]'s shape is a channel
/// and not a trait. Written once, by `add_the_delegate_selectors`, before any of
/// them can be called: AppKit cannot send a message to a selector that has not
/// been added yet.
static OUTBOX: OnceLock<Arc<Outbox>> = OnceLock::new();

/// Post, or drop it on the floor if the door was never opened.
///
/// The floor is unreachable — the selectors are added in the same call that
/// fills the cell — and it is written as an answer rather than an `expect`
/// because a panic inside an Objective-C frame unwinds into AppKit, which X-2
/// measured as a process that ends before any hook of this program's is
/// consulted.
///
/// `pub(crate)` for [`macos_services`](crate::macos_services), which is the
/// second AppKit object in this crate that is called with nothing of this
/// program's in its hand and has to find the channel the same way — a Service
/// posts into this very cell, which is what puts a cold delivery behind the same
/// buffer as a cold `application:openURLs:`.
pub(crate) fn post(origin: AppDelegateOrigin, kind: AppDelegateEventKind) {
    if let Some(outbox) = OUTBOX.get() {
        outbox.post(AppDelegateEvent { origin, kind });
    }
}

// ── the implementations ────────────────────────────────────────────────────

/// `applicationShouldHandleReopen:hasVisibleWindows:`
///
/// **Answers NO, always.** NO is AppKit's "this application has handled it", and
/// it has: the event is on its way to `bt-app`, which raises a window of its own
/// or opens one. YES would additionally let AppKit un-miniaturise whatever it
/// thinks the front window is, which is a second actor deciding the same
/// question from a flag X-4 measured as advisory — YES for a minimised window,
/// YES for one hidden with `-[NSApplication hide:]`, NO only for an application
/// with no windows at all. One answer, decided in one place, out of the window
/// list that knows what a window holds.
extern "C-unwind" fn should_handle_reopen(
    _this: &AnyObject,
    _cmd: Sel,
    _app: &AnyObject,
    has_visible_windows: Bool,
) -> Bool {
    post(
        AppDelegateOrigin::Reopen,
        AppDelegateEventKind::Reopen {
            had_visible_windows: has_visible_windows.as_bool(),
        },
    );
    Bool::NO
}

/// `applicationShouldTerminate:`
///
/// **Answers `NSTerminateLater`, always**, and returns on the same line it was
/// called on. Folio's quit is a transaction that spans turns of the event loop —
/// ask the reader about unsaved names, save, photograph every window, write the
/// session, retire the panes, release `session.lock` — and none of that can
/// happen on this stack. `NSTerminateNow` here would be a quit with the session
/// unwritten; `NSTerminateCancel` would be a Dock *Quit* that did nothing.
///
/// What completes it is [`TerminationAnswer::answer`], from winit's own handler.
/// See rule ② in [`app_delegate`](crate::app_delegate)'s header for why it
/// cannot be the main dispatch queue instead.
extern "C-unwind" fn should_terminate(
    _this: &AnyObject,
    _cmd: Sel,
    _app: &AnyObject,
) -> NSApplicationTerminateReply {
    post(
        AppDelegateOrigin::Termination,
        AppDelegateEventKind::TerminationRequested(TerminationAnswer::new()),
    );
    NSApplicationTerminateReply::TerminateLater
}

/// `applicationShouldTerminateAfterLastWindowClosed:`
///
/// **Answers NO** — the application stays in the Dock with no window open, and a
/// Dock or Finder click brings one back (plan §8 Q10, ruled 2026-09-12). It is
/// the macOS convention, and it is the shape `bt-app`'s own
/// `a_run_ends_with_its_last_visible_window` now reads off
/// [`crate::HostPlatform`] rather than assuming.
extern "C-unwind" fn should_terminate_after_last_window_closed(
    _this: &AnyObject,
    _cmd: Sel,
    _app: &AnyObject,
) -> Bool {
    post(
        AppDelegateOrigin::LastWindowClosed,
        AppDelegateEventKind::LastWindowClosed,
    );
    Bool::NO
}

/// `application:openURLs:`
///
/// The URLs arrive as an `NSArray<NSURL>` and leave as paths. A URL that is not
/// a local file is **dropped with a line on the diagnostic channel** rather than
/// refused to AppKit — this method returns nothing, so there is nobody to refuse
/// to, and a bundle that registers no `CFBundleURLTypes` is not sent one in the
/// first place.
extern "C-unwind" fn open_urls(
    _this: &AnyObject,
    _cmd: Sel,
    _app: &AnyObject,
    urls: &NSArray<NSURL>,
) {
    let mut paths = Vec::new();
    for url in urls.iter() {
        // `absoluteString` and not `path`: see `path_from_file_url`. The
        // escapes in the URL are the file system representation's own bytes,
        // and `-[NSURL path]` is an `NSString` round trip past them.
        let Some(spelling) = url.absoluteString() else {
            eprintln!("BT_MAC_APP application:openURLs: was handed a URL with no absolute form");
            continue;
        };
        match path_from_file_url(&spelling.to_string()) {
            Ok(path) => paths.push(path),
            Err(reason) => eprintln!("BT_MAC_APP application:openURLs: {reason}"),
        }
    }
    if paths.is_empty() {
        return;
    }
    post(
        AppDelegateOrigin::OpenUrls,
        AppDelegateEventKind::OpenPaths(paths),
    );
}

/// `applicationDockMenu:` (T-MAC-DOCKMENU, `docs/DESIGN.md` §13.50)
///
/// **The one of these that answers with an object**, and therefore the one with
/// an ownership rule: the method's name is not `alloc`, `new`, `copy` or
/// `mutableCopy`, so what it answers with is **not owned by AppKit** and is
/// handed over autoreleased. [`crate::macos_menu::dock_menu`] does that and this
/// carries the pointer out unchanged.
///
/// **It posts nothing.** Every other implementation in this file parks an event
/// and answers AppKit from the stack it was called on; this one is a *question*
/// about what is on a menu rather than news about something a reader did, and
/// the answer has to be given before the menu is drawn. What it reads is the
/// plan `bt-app` last refreshed onto the bar, which is a value this crate is
/// already holding — no window is asked, no lock outside this crate is taken,
/// and nothing of the application's runs. The press that follows *is* news, and
/// that goes through `folioDockChosen:` onto the one channel, a turn later.
extern "C-unwind" fn dock_menu(
    _this: &AnyObject,
    _cmd: Sel,
    _app: &AnyObject,
) -> *mut objc2_app_kit::NSMenu {
    crate::macos_menu::dock_menu()
}

// ── the injection ──────────────────────────────────────────────────────────

/// One selector, its implementation and the type encoding AppKit reads it by.
///
/// The encodings are the methods' own: `B` is `BOOL`, `Q` is the `NSUInteger`
/// an `NSApplicationTerminateReply` is, `v` is void, `@` an object and `:` a
/// selector. Getting one wrong is not a compile error, which is why they are
/// written beside the signature they describe and pinned by a test.
struct Injection {
    selector: Sel,
    imp: *const c_void,
    encoding: &'static CStr,
}

/// **Add Folio's selectors to the class winit already registered.**
///
/// # Errors
///
/// If winit's delegate class is not in the runtime — which means this was called
/// before the event loop was built — or if the runtime refuses a method, which
/// on this route means winit has grown an implementation of its own and the
/// two would now be fighting over the same selector.
pub(crate) fn add_the_delegate_selectors(outbox: Arc<Outbox>) -> Result<(), String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| off_the_window_thread("installing the application delegate"))?;
    let Some(class) = AnyClass::get(c"WinitApplicationDelegate") else {
        return Err(
            "there is no WinitApplicationDelegate in this process, so the event loop has not been \
             built yet: the delegate is installed after `EventLoop::new` and not before it"
                .to_owned(),
        );
    };
    let selectors = [
        Injection {
            selector: sel!(applicationShouldHandleReopen:hasVisibleWindows:),
            imp: should_handle_reopen as *const c_void,
            encoding: c"B@:@B",
        },
        Injection {
            selector: sel!(applicationShouldTerminate:),
            imp: should_terminate as *const c_void,
            encoding: c"Q@:@",
        },
        Injection {
            selector: sel!(applicationShouldTerminateAfterLastWindowClosed:),
            imp: should_terminate_after_last_window_closed as *const c_void,
            encoding: c"B@:@",
        },
        Injection {
            selector: sel!(application:openURLs:),
            imp: open_urls as *const c_void,
            encoding: c"v@:@@",
        },
        // T-MAC-DOCKMENU's, and the only one whose return is an object: `@` for
        // the `NSMenu *` it answers with, then the object and the selector every
        // method is called with, then the `NSApplication *` it is handed.
        Injection {
            selector: sel!(applicationDockMenu:),
            imp: dock_menu as *const c_void,
            encoding: c"@@:@",
        },
    ];
    // The channel before the methods, or a delivery could land in the gap.
    let _ = OUTBOX.set(outbox);
    for injection in selectors {
        // SAFETY: the class is a live registered class, and reading whether it
        // already answers a selector mutates nothing.
        let already = unsafe { class_getInstanceMethod(class, injection.selector) };
        if !already.is_null() {
            return Err(format!(
                "winit now implements {:?} itself, and two implementations of one selector is \
                 one of them silently winning",
                injection.selector
            ));
        }
        // SAFETY: the implementation's Rust signature matches the encoding
        // beside it — `self`, `_cmd` and the method's own arguments, in AppKit's
        // own order — and `extern "C-unwind"` is the calling convention the
        // runtime dispatches through. The class pointer is winit's own live
        // class; adding a selector it does not answer displaces nothing.
        let added = unsafe {
            class_addMethod(
                std::ptr::from_ref(class).cast_mut(),
                injection.selector,
                injection.imp,
                injection.encoding.as_ptr(),
            )
        };
        if !added.as_bool() {
            return Err(format!(
                "the Objective-C runtime refused {:?} on winit's delegate class",
                injection.selector
            ));
        }
    }
    recompute_the_delegate_mask(mtm);
    Ok(())
}

/// **Hand winit's own delegate back to `setDelegate:`.**
///
/// `-[NSApplication setDelegate:]` caches which delegate methods exist at the
/// moment it is called, and winit called it before these four did. The object
/// handed back is the very one that is already there — still a
/// `WinitApplicationDelegate`, so winit's `is_kind_of` assertion is untouched —
/// and what the round trip buys is a recomputed mask.
///
/// X-4 measured that macOS 26.6 delivers every one of the four **without** this
/// (run `r1`), so it is one message against an undocumented cache rather than a
/// fix for a symptom that was seen.
fn recompute_the_delegate_mask(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    let Some(delegate) = app.delegate() else {
        return;
    };
    app.setDelegate(None);
    app.setDelegate(Some(&delegate));
}

/// **Complete a deferred termination** — `-[NSApplication
/// replyToApplicationShouldTerminate:]`.
///
/// The one call in this file `bt-app` makes rather than receives, so it is the
/// one that gates: `NSApplication` is the main thread's, and the whole reason
/// the answer is deferred is that it has to be given from the event loop's own
/// handler after AppKit's delegate stack has unwound.
///
/// # Errors
///
/// If it is called from any other thread.
pub(crate) fn reply_to_should_terminate(proceed: bool) -> Result<(), String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| off_the_window_thread("answering a termination request"))?;
    NSApplication::sharedApplication(mtm).replyToApplicationShouldTerminate(proceed);
    Ok(())
}

/// **Whether winit's own class already implements `applicationDockMenu:`** —
/// X-4's measurement, made again for the fifth selector (T-MAC-DOCKMENU).
///
/// `None` when `WinitApplicationDelegate` is not in the runtime yet, which is
/// every moment before `EventLoop::new`. `Some(false)` is the answer this
/// ticket's route depends on and the one the `.app` proof prints before it
/// installs anything: winit does not implement it, so adding it displaces
/// nothing and `NSApp.delegate` stays winit's own object.
///
/// It reads the runtime and changes nothing. Nothing in the product calls it;
/// [`add_the_delegate_selectors`] makes the same reading for itself, and refuses
/// rather than reports.
#[doc(hidden)]
#[must_use]
pub fn winit_delegate_already_answers_the_dock_menu() -> Option<bool> {
    let class = AnyClass::get(c"WinitApplicationDelegate")?;
    // SAFETY: the class is a live registered class, and reading whether it
    // already answers a selector mutates nothing.
    let already = unsafe { class_getInstanceMethod(class, sel!(applicationDockMenu:)) };
    Some(!already.is_null())
}

/// Whether the delegate object AppKit holds answers `applicationDockMenu:`
/// (T-MAC-DOCKMENU).
///
/// Beside [`delegate_answers_the_four_selectors`] rather than folded into it,
/// and the split is M3-1's own: those four are one ticket's claim about one
/// lifecycle channel, and this is a second ticket adding a fifth method for a
/// second reason. A caller that wants both asks both.
#[must_use]
pub fn delegate_answers_the_dock_menu() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() else {
        return false;
    };
    // SAFETY: `respondsToSelector:` is `NSObject`'s and takes a selector.
    let answers: bool =
        unsafe { msg_send![&*delegate, respondsToSelector: sel!(applicationDockMenu:)] };
    answers
}

/// Whether the delegate object AppKit holds answers all four selectors.
///
/// The `.app` test's own question, asked of AppKit rather than of this module's
/// bookkeeping: `respondsToSelector:` is what AppKit itself consults before it
/// sends any of them.
#[must_use]
pub fn delegate_answers_the_four_selectors() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() else {
        return false;
    };
    [
        sel!(applicationShouldHandleReopen:hasVisibleWindows:),
        sel!(applicationShouldTerminate:),
        sel!(applicationShouldTerminateAfterLastWindowClosed:),
        sel!(application:openURLs:),
    ]
    .into_iter()
    .all(|selector| {
        // SAFETY: `respondsToSelector:` is `NSObject`'s and takes a selector.
        let answers: bool = unsafe { msg_send![&*delegate, respondsToSelector: selector] };
        answers
    })
}

#[cfg(test)]
mod tests {
    /// RED — **the four encodings say what the four implementations are.**
    ///
    /// A type encoding is a string AppKit reads at run time; a wrong one is not
    /// a compile error, it is an argument read off the wrong register on the day
    /// somebody right-clicks a folder. So the pairing is pinned in the source
    /// text, which is the only place the two halves are next to each other.
    ///
    /// MUTATION: give `applicationShouldTerminate:` `B@:@` — the encoding of the
    /// selector above it, which is the plausible mistake — and this fails.
    #[test]
    fn each_selector_carries_the_encoding_of_its_own_implementation() {
        const SOURCE: &str = include_str!("macos_app.rs");
        for (selector, encoding) in [
            ("applicationShouldHandleReopen:hasVisibleWindows:", "B@:@B"),
            ("applicationShouldTerminate:", "Q@:@"),
            ("applicationShouldTerminateAfterLastWindowClosed:", "B@:@"),
            ("application:openURLs:", "v@:@@"),
            // T-MAC-DOCKMENU's, and the plausible mistake here is the other
            // way round from the one above: `v@:@`, the encoding of a method
            // that answers nothing, on the one method that answers an object.
            ("applicationDockMenu:", "@@:@"),
        ] {
            let at = SOURCE
                .find(&format!("selector: sel!({selector})"))
                .unwrap_or_else(|| panic!("{selector} is one of the four"));
            let entry = &SOURCE[at..];
            let entry = &entry[..entry.find("},").unwrap_or(entry.len())];
            assert!(
                entry.contains(&format!("c\"{encoding}\"")),
                "{selector} is added with the wrong type encoding:\n{entry}"
            );
        }
    }
}
