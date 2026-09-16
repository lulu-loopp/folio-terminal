//! **The Services provider object — Finder's *Services ▸ Open in Folio***
//! (ticket M4-9, probe X-4, plan §8 Q3).
//!
//! The second AppKit door into [`app_delegate`](crate::app_delegate)'s channel,
//! after [`macos_menu`](crate::macos_menu)'s, and the one that is furthest from
//! the delegate: **a Service never touches the delegate at all**. AppKit takes
//! an object of this application's own through `-[NSApplication
//! setServicesProvider:]`, reads `NSServices` out of the bundle's `Info.plist`,
//! and sends the method that array's `NSMessage` key names **to that object**.
//! X-4 registered one and read `NSApp.servicesProvider` back to confirm it.
//!
//! # Why this is the whole of "Open Folio here" on this platform
//!
//! On Windows the same verb is a COM class Explorer creates inside this very
//! executable (`explorer_command`), and the plan's §8 Q3 was ruled on
//! 2026-09-12: **`NSServices` only, no Finder Sync extension**. Terminal.app,
//! iTerm2 and Ghostty all ship a Services entry and nothing else, so this is the
//! convention rather than a reduced version of one. The two halves that make it
//! work are not both code: the array in `packaging/macos/Info.plist.in` is what
//! puts the row in the menu, and this file is what is on the other end of it.
//! Either one alone is a row that does nothing or an object nobody calls.
//!
//! # What the method may do, and it is the same rule as the four selectors
//!
//! It is called by AppKit on the main thread with a framework frame underneath
//! it, exactly like the delegate methods next door, so it is under rule ① of
//! [`app_delegate`](crate::app_delegate)'s header word for word: **read the
//! pasteboard, post, return**. Everything the paths then cause — a window, a
//! tab, a shell — happens a turn later on the event loop's own stack.
//!
//! It posts [`AppDelegateEventKind::OpenPaths`] rather than a kind of its own,
//! and that was decided before this ticket: `AppDelegateOrigin`'s own
//! documentation says a Service delivers `OpenPaths` "with an origin of its
//! own", because what crosses is a list of paths either way. What the **origin**
//! buys is the one thing the two deliveries do not share, and it is a real
//! difference rather than bookkeeping — see [`AppDelegateOrigin::Services`].
//!
//! # The cold delivery is somebody else's rule, already written
//!
//! A Service can be the reason this process exists: LaunchServices starts the
//! application when nothing has its Services port open, and the delivery then
//! arrives **before `resumed`** — X-4 timed the cold case at t=222 ms against a
//! `resumed` at 247 ms, with no window in existence. Nothing here handles that.
//! It posts into the same [`Outbox`](crate::app_delegate) the four selectors
//! post into, which holds everything until `AppDelegate::ready`, and the buffer
//! that already exists for `application:openURLs:` is the buffer for this. That
//! is the whole reason this file registers no channel of its own.
//!
//! # Why the folder rule is not here
//!
//! The ticket asks whether a *file* in the selection is refused or opens the
//! folder that contains it. It opens the folder — the answer the Windows verb
//! has always given (`bt_app::explorer_menu::folder_for`: a folder is the
//! folder, a file is its folder, a name with nothing at it opens nothing) — and
//! **the rule is applied at the landing rather than here**, for three reasons
//! that are each about this file rather than about tidiness:
//!
//! * it is the *product's* rule and not the platform's. `folder_for` is read by
//!   Explorer's verb on the other machine and is tested there; a second copy of
//!   it in this crate would be a second answer to one question, which is the
//!   thing `ShellPickKind` exists to stop (`docs/DESIGN.md` §13.17 ⑦);
//! * answering it **needs the disk**, and this method is on AppKit's stack. A
//!   `stat` per selected file inside a Services callback is exactly the work
//!   rule ① says does not happen here;
//! * the disk can move between the gesture and the turn that lands it. A path
//!   judged here and opened three turns later would have been judged against a
//!   directory that no longer has to exist, so the judgement has to be taken
//!   where the tab is opened in any case.
//!
//! So what crosses this door is what the reader selected, decoded, in order.

use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{AnyThread, MainThreadMarker, define_class, msg_send, sel};
use objc2_app_kit::{NSApplication, NSPasteboard, NSUpdateDynamicServices};
use objc2_foundation::NSString;

use crate::app_delegate::{AppDelegateEventKind, AppDelegateOrigin};
use crate::macos_impl::off_the_window_thread;

define_class!(
    // SAFETY:
    // - `NSObject` has no subclassing requirements.
    // - This class does not implement `Drop` and has no ivars: everything the
    //   method needs is the crate-wide channel `macos_app` owns, for the reason
    //   that channel is a `static` in the first place — the provider is
    //   reachable only through AppKit, which hands it nothing of this program's.
    #[unsafe(super(NSObject))]
    #[name = "FolioServicesProvider"]
    struct ServicesProvider;

    impl ServicesProvider {
        /// **`openInFolio:userData:error:`** — the method `NSServices`'
        /// `NSMessage` key names, with the two arguments AppKit appends to it.
        ///
        /// The shape is the framework's and not a choice: a Services method is
        /// `(NSPasteboard *, NSString *, NSString **)`, `NSMessage` carries only
        /// the first word of the selector, and a method with any other shape is
        /// one AppKit looks for, does not find, and reports to the reader as a
        /// Service that failed.
        ///
        /// **`userData` is unread**, because the `NSServices` entry sets no
        /// `NSUserData`: there is one verb here, and a key that distinguished
        /// two would be a key with one value.
        ///
        /// **`error` is left as AppKit set it**, and that is a decision rather
        /// than an omission. Writing through it puts a system alert in front of
        /// the reader, and the only state that could fill it — a pasteboard with
        /// no file URL on it — is one `NSSendTypes` says Finder does not build:
        /// the row is offered for a selection of files and folders and for
        /// nothing else. A selection this method could not read is said on the
        /// diagnostic channel, where `application:openURLs:` says the same
        /// thing, and the reader is not shown an alert about a gesture that
        /// cannot happen.
        #[unsafe(method(openInFolio:userData:error:))]
        fn open_in_folio(
            &self,
            pasteboard: &NSPasteboard,
            _user_data: *mut NSString,
            _error: *mut *mut NSString,
        ) {
            let paths = paths_on(pasteboard);
            if paths.is_empty() {
                eprintln!(
                    "BT_MAC_APP openInFolio:userData:error: was handed a selection with no local \
                     file in it"
                );
                return;
            }
            crate::macos_app::post(
                AppDelegateOrigin::Services,
                AppDelegateEventKind::OpenPaths(paths),
            );
        }
    }

    unsafe impl NSObjectProtocol for ServicesProvider {}
);

impl ServicesProvider {
    /// One provider, made once. No ivars, so `init` is `NSObject`'s own.
    fn new() -> Retained<Self> {
        // SAFETY: `init` on a freshly allocated instance of this class, which
        // inherits `NSObject`'s implementation of it.
        unsafe { msg_send![Self::alloc(), init] }
    }
}

/// **Every local path the selection names, decoded, in the reader's own
/// order.**
///
/// Order is load-bearing: a selection of three folders is three tabs, and the
/// reader chose which is first. `readObjectsForClasses:options:` preserves the
/// pasteboard's item order, which is the order Finder wrote the selection in.
///
/// A URL that is not a local file is **dropped with a line** rather than
/// refused: this is `application:openURLs:`' own rule at the second door, and it
/// is the right one here for a reason of its own — a selection of four folders
/// and one thing this program cannot open is four tabs and a line, not five
/// refusals.
///
/// `absoluteString` and not `-[NSURL path]`, for the reason
/// [`crate::path_from_file_url`] gives at length: a path is not required to be text,
/// the percent escapes in the URL *are* the file system representation's own
/// bytes, and an `NSString` round trip past them hands back a different file.
fn paths_on(pasteboard: &NSPasteboard) -> Vec<std::path::PathBuf> {
    let urls = match crate::macos_file_urls::urls_on(pasteboard) {
        Ok(urls) => urls,
        Err(reason) => {
            eprintln!("BT_MAC_APP openInFolio: {reason}");
            return Vec::new();
        }
    };
    // Services keep their existing partial-open rule and their own diagnostic channel; a clipboard
    // candidate instead fails as a whole when one advertised file cannot be acquired.
    let mut paths = Vec::new();
    for url in urls {
        match crate::clipboard::file_urls([url]) {
            crate::clipboard::Candidate::Present(mut selected) => paths.append(&mut selected),
            crate::clipboard::Candidate::Absent => {
                eprintln!("BT_MAC_APP selected item is not a local file")
            }
            crate::clipboard::Candidate::Unreadable(reason) => {
                eprintln!("BT_MAC_APP openInFolio: {reason}")
            }
        }
    }
    paths
}

/// The provider AppKit holds, kept alive for the life of the process.
///
/// `-[NSApplication setServicesProvider:]` does **not** retain its argument —
/// the header says so in one line — so the object has to be owned by somebody,
/// and the only lifetime that outlives every possible invocation is the
/// process's. A `static` rather than a field on `AppDelegate` for the reason
/// `macos_app`'s `OUTBOX` is one: what reaches this object is AppKit, which
/// holds nothing of this program's and can be asked at any moment.
static PROVIDER: OnceLock<ProviderCell> = OnceLock::new();

/// The `Retained` above, made `Send`/`Sync` by never being handed out.
///
/// A `Retained<ServicesProvider>` is neither, because messaging an AppKit object
/// from another thread is not safe in general — and nothing here ever does: the
/// cell is written once from the main thread and read by nobody. What it exists
/// for is the retain, not the pointer.
struct ProviderCell(
    #[allow(dead_code, reason = "the retain is the whole of what this holds")]
    Retained<ServicesProvider>,
);

// SAFETY: the value is written once, from the main thread, and never read,
// returned or messaged afterwards. The only thing that happens to it after the
// `set` is its release, and that never happens: a `static` is not dropped.
unsafe impl Send for ProviderCell {}
// SAFETY: as above — no `&ProviderCell` is ever turned back into a message.
unsafe impl Sync for ProviderCell {}

/// **Register the provider and make a fresh bundle's Services visible**
/// (M4-9).
///
/// Called once, from the main thread, after
/// [`AppDelegate::install`](crate::AppDelegate::install) — after, because the
/// channel this object posts into is filled by that call and a Service arriving
/// into an empty cell would be a delivery on the floor.
///
/// `NSUpdateDynamicServices` is the second line and it is not cosmetic.
/// LaunchServices caches the Services table; a bundle that has just been built,
/// copied or moved is one the cache has never read, and without this the row
/// appears after a logout rather than now. It is cheap, it is what Apple
/// documents for an application that registers services outside the ordinary
/// install path, and it is what makes a first run of a freshly downloaded Folio
/// have the row.
///
/// # Errors
///
/// If it is called from any thread but the main one, which is the only thread
/// `NSApplication` may be touched from.
pub(crate) fn install() -> Result<(), String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| off_the_window_thread("registering the Services provider"))?;
    let provider = ServicesProvider::new();
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: the object is an instance of this file's own class, which answers
    // the selector `NSServices`' `NSMessage` names. It is kept alive below.
    unsafe { app.setServicesProvider(Some(&provider)) };
    let _ = PROVIDER.set(ProviderCell(provider));
    NSUpdateDynamicServices();
    Ok(())
}

/// **Whether AppKit is holding a Services provider of this program's** (M4-9).
///
/// The `.app` test's own question, asked of `NSApp` rather than of this module's
/// bookkeeping — the same reading
/// [`delegate_answers_the_four_selectors`](crate::delegate_answers_the_four_selectors)
/// takes of the delegate, and for the same reason: what decides whether a
/// Service is delivered is the object AppKit has, not the one this file
/// remembers handing over.
#[must_use]
pub fn services_provider_answers_open_in_folio() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(provider) = NSApplication::sharedApplication(mtm).servicesProvider() else {
        return false;
    };
    // SAFETY: `respondsToSelector:` is `NSObject`'s and takes a selector.
    let answers: bool =
        unsafe { msg_send![&*provider, respondsToSelector: sel!(openInFolio:userData:error:)] };
    answers
}
