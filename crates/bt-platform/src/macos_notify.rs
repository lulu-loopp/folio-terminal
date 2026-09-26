//! **The desktop notification and the Dock tile, on macOS** — the twin of the
//! notification and taskbar half of `windows_impl` (ticket M4-6).
//!
//! # The four doors, and why they are one file
//!
//! `Notifier`, `Taskbar`, [`flash_window`] and [`taskbar_is_auto_hidden`] are
//! four names the portable module refused with `M4-6` in the sentence, and they
//! are one file because on this platform they are one object: **the Dock icon**.
//! A notification is filed under the application's bundle identifier, the badge
//! is drawn on the application's tile, the bounce is the application's, and the
//! preference that decides whether any of it is on screen is the Dock's own. On
//! Windows the same four are three different subsystems — WinRT's notification
//! platform, `ITaskbarList3`, `FlashWindowEx` and `SHAppBarMessage` — and the
//! Windows arm is spread across `windows_impl` accordingly.
//!
//! # What a window is here, and what it is not
//!
//! **`Taskbar::new` takes a window and does not use it, and [`flash_window`]
//! takes one and does not use it either.** That is not a stub: a taskbar button
//! belongs to an `HWND`, and a Dock tile belongs to a *process*. The signature
//! is kept because the door is one door on both platforms and `bt-app` calls it
//! per window on both; what changes is that two windows share one tile here, so
//! the last reading written is the one the reader sees. [`Taskbar`] says which
//! object wrote the badge it is showing, and clears it on the way out only if
//! it is still that one — without which a window that closed while a build was
//! running would leave `40%` on the Dock for the life of the process.
//!
//! # Authorization, and the one thing it costs
//!
//! `UNUserNotificationCenter` will not raise anything until the reader has said
//! yes, and asking is a system prompt. So the ask happens **on the first
//! notification this process actually raises** and never at launch — the same
//! promise the Windows arm keeps about its registry value (`docs/DESIGN.md`
//! §7.6): a reader who never runs anything that asks ends the day having been
//! asked nothing. The answer arrives on whatever thread the platform answers on
//! and is recorded; a *denial* is what turns every later `show` into one line,
//! which is `NotificationDesk`'s own contract on the other side of the door.
//!
//! **The first notification of a fresh install may be lost**, because the
//! prompt is in front of the reader when it is posted. That is the honest cost
//! of not asking at launch, it happens once per machine, and the alternative —
//! a permission prompt during the first frame of the first run — is the thing
//! the lazy shape exists to avoid.
//!
//! # No bundle, no notification centre
//!
//! `+[UNUserNotificationCenter currentNotificationCenter]` raises an
//! Objective-C exception in a process that has no bundle identifier, and an
//! exception through a Rust frame is not a `Result`, it is the end of the
//! process. So the identifier is read **first**, out of `NSBundle`, and a
//! process without one is refused with a sentence that names the reason — which
//! is the case of `cargo run` out of `target/debug/folio`, and is why §4.5 of
//! the plan builds the bundle from M1 rather than from M5.
//!
//! # The click, and where it is parked
//!
//! A click on a notification Folio raised arrives at
//! `userNotificationCenter:didReceiveNotificationResponse:`, on the main
//! thread, and is pushed into the same queue the Windows arm's `Activated`
//! handler pushes into. **It is not routed through M3-1's `AppDelegate`
//! buffer**, and that is a decision rather than an omission:
//!
//! * the buffer exists because a *cold* `application:openURLs:` arrives before
//!   `resumed` and names a path, which is a thing a launched application can
//!   still act on. A cold notification click names `w=…&t=…&s=…` — a window, a
//!   tab and a pane **of the process that has exited** — and there is nothing
//!   for it to land on. `notify::NotificationRoute` says so in its own words,
//!   and the Windows arm gives cold activation up for the same reason;
//! * this delegate is set at first need, which is after launch, so a response
//!   delivered to a cold launch reaches no delegate at all and is dropped by
//!   the platform. Nothing in this file tries to catch it.
//!
//! What remains is a click while the process is alive, and it is parked in the
//! notifier's own queue until [`Notifier::take_activations`] is called on the
//! event loop's turn — which is exactly where the Windows arm parks it.
//!
//! # The thread
//!
//! The delegate is set from the main thread and the tile is written from the
//! main thread; both doors prove it rather than assume it, on `macos_impl`'s
//! own reasoning about a gate that cannot be met in a libtest case. The two
//! callbacks are called *by* the platform and do not gate, for the reason the
//! four application-delegate selectors do not.

use std::sync::atomic::{AtomicIsize, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, define_class, msg_send};
use objc2_app_kit::{NSApplication, NSRequestUserAttentionType};
use objc2_foundation::{
    NSBundle, NSDictionary, NSError, NSNumber, NSString, NSUserDefaults, ns_string,
};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationDefaultActionIdentifier, UNNotificationPresentationOptions,
    UNNotificationRequest, UNNotificationResponse, UNNotificationSound, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};

use crate::macos_impl::off_the_window_thread;
use crate::{NativeWindow, TaskbarProgress, dock_badge_label};

/// The main-thread gate the two doors that touch AppKit pass through.
///
/// `macos_impl::window_thread`'s twin, with its own sentence because what is
/// being asked for is not a window: the caller of these is the event loop, and
/// the thing that is main-thread-only is `NSApplication` and the notification
/// centre's delegate rather than any one window.
fn main_thread(what: &str) -> Result<MainThreadMarker, String> {
    MainThreadMarker::new().ok_or_else(|| off_the_window_thread(what))
}

// ── notifications ──────────────────────────────────────────────────────────

/// **The `userInfo` key one route travels under.**
///
/// A key of this program's own and not one of Apple's: `userInfo` is a
/// dictionary the application fills in and reads back, and the only thing in
/// Folio's is the launch string `notify::NotificationRoute` writes. Spelled
/// with the product in it so that a person reading a delivered notification's
/// payload can tell whose it is.
fn launch_key() -> &'static NSString {
    ns_string!("folio.launch")
}

/// What the authorization answer was, as a number two threads can share.
///
/// Three states and not a `bool`, because "not answered yet" is a real one:
/// the request is made in [`Notifier::new`] and the answer arrives on the
/// platform's own thread some time later, so every `show` between those two
/// moments is posted in the dark. It is posted anyway — the platform holds it
/// or drops it, and a notification withheld by this process on the strength of
/// an answer that has not arrived would be a notification lost for certain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Permission {
    /// The request has been made and nothing has come back.
    Unanswered,
    /// The reader said yes.
    Granted,
    /// The reader said no, or had already said no.
    Denied,
}

impl Permission {
    const UNANSWERED: u8 = 0;
    const GRANTED: u8 = 1;
    const DENIED: u8 = 2;

    fn from_code(code: u8) -> Self {
        match code {
            Self::GRANTED => Self::Granted,
            Self::DENIED => Self::Denied,
            _ => Self::Unanswered,
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::Unanswered => Self::UNANSWERED,
            Self::Granted => Self::GRANTED,
            Self::Denied => Self::DENIED,
        }
    }
}

/// What the two callbacks and the event loop share.
///
/// The Windows arm's `NotifierShared` with two fields more, and both of them
/// are there because this platform answers **later** where WinRT answered in
/// the call: the authorization answer and the failure of a posted request both
/// arrive on somebody else's thread, after the `show` that caused them has
/// already returned `Ok`.
struct NotifierShared {
    /// Launch strings from notifications that have been clicked and not yet
    /// routed.
    activations: Mutex<Vec<String>>,
    /// How the owning loop is told there is something in the queue.
    wake: Mutex<Box<dyn Fn() + Send>>,
    /// The authorization answer, as a [`Permission`] code.
    permission: AtomicU8,
    /// **The first thing the platform refused, waiting for somebody to be told
    /// about it.**
    ///
    /// `-[UNUserNotificationCenter addNotificationRequest:withCompletionHandler:]`
    /// reports its failure to a block rather than to the caller, so the `show`
    /// that failed has already answered `Ok` by the time the sentence exists.
    /// It is carried here and taken by the **next** `show`, which is one
    /// message late and is the whole of the difference: `NotificationDesk`
    /// still hears the sentence once, still latches its refusal once, and still
    /// costs one line.
    refusal: Mutex<Option<String>>,
}

impl NotifierShared {
    fn permission(&self) -> Permission {
        Permission::from_code(self.permission.load(Ordering::SeqCst))
    }

    fn answered(&self, permission: Permission) {
        self.permission.store(permission.code(), Ordering::SeqCst);
    }

    /// Record a refusal, keeping the first — the later ones are its
    /// consequences.
    ///
    /// Poisoned locks are ignored rather than unwrapped, for the Windows arm's
    /// reason: this runs on a platform thread, where a panic has nobody to
    /// catch it.
    fn refuse(&self, why: String) {
        if let Ok(mut refusal) = self.refusal.lock() {
            refusal.get_or_insert(why);
        }
    }

    fn take_refusal(&self) -> Option<String> {
        self.refusal.lock().ok().and_then(|mut held| held.take())
    }

    /// One click, on its way to the event loop.
    fn activated(&self, launch: String) {
        if let Ok(mut queue) = self.activations.lock() {
            queue.push(launch);
        }
        if let Ok(wake) = self.wake.lock() {
            wake();
        }
    }
}

define_class!(
    // SAFETY:
    // - `NSObject` has no subclassing requirements.
    // - This class does not implement `Drop`; its one ivar does, and the
    //   macro's generated `dealloc` runs it.
    #[unsafe(super(NSObject))]
    #[name = "FolioNotificationDelegate"]
    #[ivars = Arc<NotifierShared>]
    struct NotificationDelegate;

    unsafe impl NSObjectProtocol for NotificationDelegate {}

    unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
        /// **A notification arriving while Folio is the active application.**
        ///
        /// Without this method the platform's own answer is *show nothing*: a
        /// foreground application is assumed to be able to say it in its own
        /// window. Folio can — that is what the tab's own marks are — but the
        /// decision has already been taken before anything reaches this file.
        /// `bt_app::notify::desktop_reach` is the ladder, and the pane whose
        /// tab is on screen in a focused window answers `Reach::Nothing`, which
        /// never calls `show` at all. So **everything that gets here has
        /// already been ruled owed**, and the honest answer is to present it.
        ///
        /// A second gate here would be the thing red line 12 forbids: a
        /// decision about one moment re-taken at a later one, from facts this
        /// side of the door cannot see.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            handler: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            let _callback = crate::admission::enter_callback("notification-center");
            handler.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }

        /// **A click, or a dismissal, on one of ours.**
        ///
        /// Only the click is an activation. `UNNotificationDefaultActionIdentifier`
        /// is "the reader opened it"; `UNNotificationDismissActionIdentifier` is
        /// "the reader swept it away", and treating the second as the first
        /// would send somebody who cleared a banner to a pane they were not
        /// asking for. The Windows arm makes the same split — it listens to
        /// `Activated` and not to `Dismissed`.
        ///
        /// The completion handler is called on every path, including the ones
        /// that carry nothing: it is how the platform is told this process is
        /// finished with the response, and a response never completed is a
        /// delegate the platform stops waiting on.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive_response(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            handler: &DynBlock<dyn Fn()>,
        ) {
            let _callback = crate::admission::enter_callback("notification-center");
            // SAFETY: reading one of the framework's own constant strings,
            // which is initialised before any notification can be responded to.
            let opened = unsafe { UNNotificationDefaultActionIdentifier };
            if &*response.actionIdentifier() == opened
                && let Some(launch) = launch_string(&response.notification())
            {
                self.ivars().activated(launch);
            }
            handler.call(());
        }
    }
);

impl NotificationDelegate {
    fn new(shared: Arc<NotifierShared>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(shared);
        // SAFETY: `NSObject`'s designated initializer, called on a fresh
        // allocation whose ivars are set.
        unsafe { msg_send![super(this), init] }
    }
}

/// The route one delivered notification carries, or `None` if it carries none.
///
/// `None` is a real answer and not a failure: a notification under this bundle
/// identifier that Folio did not write — one left by an earlier build, or by a
/// developer's own probe — has no `folio.launch` in its `userInfo`, and a click
/// on it should do nothing rather than be decoded into a pane by guesswork.
/// `NotificationRoute::parse` refuses the same way on the other side.
fn launch_string(notification: &UNNotification) -> Option<String> {
    route_from(&notification.request().content().userInfo())
}

/// The same reading, one layer down — **the half a `#[test]` can run.**
///
/// Split out because a `UNNotification` cannot be built by this program: it is
/// handed over by the platform, inside a bundle, after a reader has clicked
/// something. A `userInfo` dictionary can be built by anybody, and the thing
/// worth holding is that [`route_written_into`] and this agree about the key
/// and about the string — which is the whole of what a click decodes.
fn route_from(info: &NSDictionary) -> Option<String> {
    // The dictionary is `NSDictionary<AnyObject, AnyObject>` — what the
    // framework declares — so the key crosses as an object and the value is
    // asked whether it is a string rather than assumed to be one.
    let key: &AnyObject = launch_key();
    let value = info.objectForKey(key)?;
    value.downcast_ref::<NSString>().map(NSString::to_string)
}

/// **The `userInfo` one route is posted in**, and the one place it is spelled.
///
/// A function rather than two lines inside `show`, so that the writer and the
/// reader above are one pair a test can close: `a_route_survives_the_userinfo`
/// puts a launch string through both and gets it back.
fn route_written_into(launch: &str) -> Retained<NSDictionary> {
    let route = NSString::from_str(launch);
    let info = NSDictionary::from_slices(&[launch_key()], &[&*route]);
    // SAFETY: the cast is between two Rust spellings of the same Objective-C
    // object — the generic parameters are this side's claim about the contents,
    // and `setUserInfo:` is declared against the untyped dictionary. What the
    // method itself requires is that the values are property-list types, and
    // this dictionary holds one `NSString` under one `NSString`.
    unsafe { Retained::cast_unchecked(info) }
}

/// **How many notifications this process has posted**, which is where each
/// one's identifier comes from.
///
/// A counter and **not the launch string**, and that is the one decision in
/// this file that looks like the opposite of the obvious. An identifier is what
/// the platform replaces by: posting a second request under an identifier that
/// is already delivered *removes the first*. The launch string names a pane,
/// so two bells from one pane would carry one identifier — and a shell that
/// rang twice would leave one notification here and two on Windows, where
/// `toast_xml` writes no `tag` and no `group` and every toast is its own.
/// A difference that large should be a decision somebody took, and the decision
/// is that the two platforms say the same thing.
static NEXT_NOTIFICATION: AtomicU64 = AtomicU64::new(1);

/// This process's voice in the notification centre (M4-6).
///
/// See the module header for the authorization, the bundle identifier and the
/// click. What is left to say here is the memory: **nothing is held per
/// notification**. The Windows arm keeps the last thirty-two `ToastNotification`
/// objects alive because a click comes back through a handler registered on the
/// object that was shown; this platform delivers every response to one
/// delegate, so the object a notification was posted from can go the moment the
/// request is accepted, and a click on a notification from an hour ago is
/// routed by exactly the same path as a click on the newest one.
pub struct Notifier {
    /// The application's notification centre. One per process, and the
    /// framework's own singleton — held so that the delegate below is not the
    /// only thing keeping this side of the conversation alive.
    center: Retained<UNUserNotificationCenter>,
    /// **The delegate, held because the framework does not hold it.**
    /// `-[UNUserNotificationCenter delegate]` is a `weak` property: an object
    /// set there and then released takes itself back out, and the clicks stop
    /// arriving with nothing to say why. So the notifier owns it for its whole
    /// life, and dropping the notifier is what ends the subscription — no
    /// `Drop` of this type's own, because the weak property is what makes one
    /// unnecessary.
    _delegate: Retained<NotificationDelegate>,
    shared: Arc<NotifierShared>,
}

impl Notifier {
    /// Claim the identity, open the channel and ask the reader, or say why not.
    ///
    /// `wake` is called from whatever thread the platform delivers a click on —
    /// which on this platform is the main one, and the contract is written for
    /// the general case anyway because it is the same contract on both. It must
    /// do nothing but nudge the event loop.
    ///
    /// # Errors
    ///
    /// If this process has no bundle identifier, or if it is asked from a
    /// thread that is not the main one.
    pub fn new(wake: Box<dyn Fn() + Send>) -> Result<Self, String> {
        // **Before the thread gate and before anything from the framework.**
        // The centre's own accessor raises an Objective-C exception in a
        // process with no bundle, and an exception is not a value this function
        // could turn into the `Err` below.
        if NSBundle::mainBundle().bundleIdentifier().is_none() {
            return Err(
                "this process has no bundle identifier, and the notification centre files every \
                 message under one; a Folio built into Folio.app has one and a binary run out of \
                 target/debug does not"
                    .to_owned(),
            );
        }
        let _ = main_thread("the notification centre's delegate")?;
        let shared = Arc::new(NotifierShared {
            activations: Mutex::new(Vec::new()),
            wake: Mutex::new(wake),
            permission: AtomicU8::new(Permission::UNANSWERED),
            refusal: Mutex::new(None),
        });
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let delegate = NotificationDelegate::new(Arc::clone(&shared));
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

        // Alert and sound, which is what a Windows toast is: a banner that
        // stays in the notification centre, and the system's own sound.
        // **`Badge` is deliberately not asked for** — the Dock badge this
        // product draws is the taskbar button's progress reading (see
        // [`Taskbar`]), which is not the notification platform's badge and does
        // not need its permission.
        let answering = Arc::clone(&shared);
        let block = RcBlock::new(move |granted: Bool, error: *mut NSError| {
            let _callback = crate::admission::enter_callback("notification-center");
            if !error.is_null() {
                // SAFETY: a non-null `NSError*` the framework handed this block
                // for the length of the call.
                let error = unsafe { &*error };
                answering.refuse(format!(
                    "UNUserNotificationCenter::requestAuthorization: {}",
                    error.localizedDescription()
                ));
            }
            answering.answered(if granted.as_bool() {
                Permission::Granted
            } else {
                Permission::Denied
            });
        });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &block,
        );
        Ok(Self {
            center,
            _delegate: delegate,
            shared,
        })
    }

    /// Raise one notification. `launch` comes back verbatim if it is clicked.
    ///
    /// # Errors
    ///
    /// If the reader has refused this application's notifications, or if the
    /// platform refused the *previous* request — see [`NotifierShared::refusal`]
    /// for why a failure can be one message late.
    pub fn show(&mut self, title: &str, body: &str, launch: &str) -> Result<(), String> {
        if let Some(refusal) = self.shared.take_refusal() {
            return Err(refusal);
        }
        if self.shared.permission() == Permission::Denied {
            return Err(
                "this application's notifications are turned off; System Settings ▸ \
                 Notifications is where they come back"
                    .to_owned(),
            );
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        // Omitted rather than set empty, for `toast_xml`'s reason: an empty
        // second line is a blank line the platform still lays out.
        if !body.is_empty() {
            content.setBody(&NSString::from_str(body));
        }
        content.setSound(Some(&UNNotificationSound::defaultSound()));
        // SAFETY: the dictionary [`route_written_into`] builds holds one
        // `NSString` under one `NSString`, which is the property-list
        // requirement this method carries.
        unsafe { content.setUserInfo(&route_written_into(launch)) };
        let identifier = format!(
            "folio.notification.{}",
            NEXT_NOTIFICATION.fetch_add(1, Ordering::Relaxed)
        );
        // No trigger: a request with none is delivered immediately, which is
        // the only kind this product raises. Everything Folio notifies about
        // has already happened.
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&identifier),
            &content,
            None,
        );
        let answering = Arc::clone(&self.shared);
        let block = RcBlock::new(move |error: *mut NSError| {
            let _callback = crate::admission::enter_callback("notification-center");
            if !error.is_null() {
                // SAFETY: a non-null `NSError*` the framework handed this block
                // for the length of the call.
                let error = unsafe { &*error };
                answering.refuse(format!(
                    "UNUserNotificationCenter::addNotificationRequest: {}",
                    error.localizedDescription()
                ));
            }
        });
        self.center
            .addNotificationRequest_withCompletionHandler(&request, Some(&block));
        Ok(())
    }

    /// Every clicked notification's launch string since the last call, oldest
    /// first.
    #[must_use]
    pub fn take_activations(&self) -> Vec<String> {
        self.shared
            .activations
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }
}

// ── the Dock tile ──────────────────────────────────────────────────────────

/// Which [`Taskbar`] wrote the badge the Dock is showing, or `0` for none.
///
/// The tile is the application's and the type is the window's, so "clear what
/// you put there" needs a way to ask whether what is there is still yours. A
/// number rather than a pointer because the answer has to survive the object
/// that wrote it being dropped.
static DOCK_BADGE_OWNER: AtomicU64 = AtomicU64::new(0);

/// The next identity handed to a [`Taskbar`]. Never `0`, which is the value
/// [`DOCK_BADGE_OWNER`] uses for "nobody".
static NEXT_DOCK_TILE: AtomicU64 = AtomicU64::new(1);

/// One window's place to put a progress reading, where that place is the Dock
/// tile (M4-6).
///
/// **What is lost against the Windows arm, said out loud.** `ITaskbarList3`
/// paints a bar in one of three colours; the Dock tile's only ink is the badge,
/// which is text. So the number survives the crossing and the colour does not:
/// a build that fails at 40% and a build that is paused at 40% both read `40%`
/// here, and the tab's own marks are where that difference is still drawn. The
/// mapping is [`crate::dock_badge_label`], which is pure and is where the rule
/// is written down.
pub struct Taskbar {
    /// This object's identity in [`DOCK_BADGE_OWNER`].
    id: u64,
}

impl Taskbar {
    /// Take a place on the Dock tile, or say why not.
    ///
    /// Nothing is created and nothing is claimed: the tile exists whether or
    /// not anybody writes to it, which is why — unlike the Windows arm, which
    /// takes a COM apartment and an interface here — this constructor's only
    /// work is to prove its thread and take an identity.
    ///
    /// # Errors
    ///
    /// If it is asked from a thread that is not the main one.
    pub fn new(window: NativeWindow) -> Result<Self, String> {
        // The Dock tile is the application's; see the module header.
        let _ = window;
        let _ = main_thread("the Dock tile")?;
        Ok(Self {
            id: NEXT_DOCK_TILE.fetch_add(1, Ordering::Relaxed),
        })
    }

    /// Put `progress` on the Dock tile.
    ///
    /// There is nothing to restore later and therefore nothing remembered: the
    /// Windows arm keeps its reading because the shell hands the button back
    /// blank whenever `explorer.exe` restarts, and the Dock tile is not handed
    /// back by anybody.
    ///
    /// # Errors
    ///
    /// If it is asked from a thread that is not the main one.
    pub fn set_progress(&self, progress: TaskbarProgress) -> Result<(), String> {
        let mtm = main_thread("the Dock tile's badge")?;
        let tile = NSApplication::sharedApplication(mtm).dockTile();
        match dock_badge_label(progress) {
            Some(label) => {
                tile.setBadgeLabel(Some(&NSString::from_str(&label)));
                DOCK_BADGE_OWNER.store(self.id, Ordering::SeqCst);
            }
            None => {
                tile.setBadgeLabel(None);
                // Only if it was ours. A window that has just finished must not
                // disown a badge another window is in the middle of showing.
                let _ = DOCK_BADGE_OWNER.compare_exchange(
                    self.id,
                    0,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
            }
        }
        Ok(())
    }
}

impl Drop for Taskbar {
    /// **Take the reading down if it is still this window's.**
    ///
    /// The Windows arm needs no such thing: a taskbar button dies with its
    /// window. One tile shared by every window does not, so a window closed
    /// mid-build would otherwise leave its last percentage on the Dock forever.
    fn drop(&mut self) {
        if DOCK_BADGE_OWNER
            .compare_exchange(self.id, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        // Nothing to do off the main thread, and this is a state the type
        // cannot be in: it is constructed on the event loop and dropped there.
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm)
                .dockTile()
                .setBadgeLabel(None);
        }
    }
}

/// Call the reader's eye to this application.
///
/// **`NSCriticalRequest` and not `NSInformationalRequest`**, because the
/// Windows arm is `FLASHW_TRAY | FLASHW_TIMERNOFG`: flash the button *until the
/// window comes to the foreground*. Critical is the request type that bounces
/// the Dock icon until the application is activated; informational bounces once
/// and stops, which is a different promise about a reader who is not at the
/// machine.
///
/// **The window is not read, for the reason in the module header** — the icon
/// is the application's, and two windows asking for attention ask for the same
/// icon. It is also a no-op while Folio is the active application, which is
/// what `FlashWindowEx` is on the foreground window and what
/// `bt_app::notify::desktop_reach`'s second row is written under.
///
/// The request's own number is kept in [`ATTENTION_REQUEST`]: it is the handle
/// `cancelUserAttentionRequest:` takes, and [`stop_flashing_window`] is the one
/// door that uses it (ticket 62). Being activated is still what ends the bounce
/// in the ordinary case, which is exactly what `FLASHW_TIMERNOFG` says on the
/// other platform. The Windows arm answers nothing and this one answers nothing
/// too.
pub fn flash_window(window: NativeWindow) {
    let _ = window;
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let request = NSApplication::sharedApplication(mtm)
        .requestUserAttention(NSRequestUserAttentionType::CriticalRequest);
    ATTENTION_REQUEST.store(request, Ordering::Relaxed);
}

/// The number of the last bounce [`flash_window`] asked for; `0` when none is
/// held. Main thread only in practice — both doors check the marker first.
static ATTENTION_REQUEST: AtomicIsize = AtomicIsize::new(0);

/// **Take back the bounce [`flash_window`] started** (ticket 62) — the Windows
/// arm's `FLASHW_STOP`. The icon is the application's, so the window is not
/// read, for [`flash_window`]'s reason. A request that has already ended (the
/// application was activated) is cancelled again harmlessly.
pub fn stop_flashing_window(window: NativeWindow) {
    let _ = window;
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let request = ATTENTION_REQUEST.swap(0, Ordering::Relaxed);
    if request != 0 {
        NSApplication::sharedApplication(mtm).cancelUserAttentionRequest(request);
    }
}

/// **Whether the Dock hides itself** — `SHAppBarMessage(ABM_GETSTATE)`'s twin,
/// and the one fact that decides whether the middle tier of `attention`'s three
/// exists on this desktop at all (`docs/DESIGN.md` §7.6, user ruling
/// 2026-08-28).
///
/// A bounce is "a mark you can glance at without being interrupted", and that
/// sentence has a premise: the icon is on screen. With the Dock set to hide
/// itself it is not — the bounce happens behind the edge of the display and the
/// reader sees nothing at all — so the tier collapses and the desktop is what
/// is left. That is `desktop_reach`'s fourth row, and this is the fact it reads.
///
/// **Asked of the reader's own preferences and not of the Dock.** There is no
/// public API for "is the Dock hidden"; the preference is
/// `com.apple.dock`'s `autohide`, which is where the Dock itself keeps the
/// answer and where System Settings writes it. Read through
/// `-[NSUserDefaults persistentDomainForName:]`, which is the supported way to
/// read another domain of the same user, and **read on `bt_app`'s taskbar
/// lane, never on the window thread** — the Windows arm's rule word for word
/// (ticket 62). `NSUserDefaults` is documented as safe to use from any thread.
///
/// **A domain that cannot be read, or a key that is not there, answers
/// `false`** — the direction `taskbar_auto_hidden_from_state` argues for and
/// the Dock's own default: of the two wrong answers, under-stating costs a
/// bounce nobody sees and over-stating puts a notification in front of somebody
/// who is looking at the pane.
#[must_use]
pub fn taskbar_is_auto_hidden() -> bool {
    let defaults = NSUserDefaults::standardUserDefaults();
    defaults
        .persistentDomainForName(ns_string!("com.apple.dock"))
        .and_then(|domain| domain.objectForKey(ns_string!("autohide")))
        .as_deref()
        .and_then(AnyObject::downcast_ref::<NSNumber>)
        .is_some_and(NSNumber::boolValue)
}

#[cfg(test)]
mod tests {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSDictionary, NSNumber, NSString, ns_string};

    use super::{Notifier, Permission, launch_key, route_from, route_written_into};

    /// RED — **a process with no bundle is refused, and it is refused before
    /// anything from the notification framework is touched.**
    ///
    /// This case runs out of `target/debug/deps`, which is exactly the shape
    /// the refusal is for. If the order in [`Notifier::new`] were the other way
    /// round — the thread gate first, or the centre fetched first — this would
    /// either report the wrong reason or take the whole test binary down with
    /// an Objective-C exception, which is the failure mode the order exists to
    /// prevent.
    ///
    /// MUTATION: move the bundle check below the thread gate and this names the
    /// wrong sentence; delete it and the case ends the process.
    #[test]
    fn a_process_with_no_bundle_is_refused_by_name() {
        let refusal = Notifier::new(Box::new(|| {}))
            .err()
            .expect("a test binary is not inside a bundle");
        assert!(
            refusal.contains("bundle identifier"),
            "the refusal names the reason: {refusal}"
        );
    }

    /// RED — the three permission codes survive the trip through the atomic.
    #[test]
    fn a_permission_is_the_same_one_after_it_has_been_a_number() {
        for permission in [
            Permission::Unanswered,
            Permission::Granted,
            Permission::Denied,
        ] {
            assert_eq!(Permission::from_code(permission.code()), permission);
        }
    }

    /// RED — **an unknown code reads as unanswered**, which is the only one of
    /// the three that is safe to guess: it posts, and lets the platform decide.
    #[test]
    fn a_code_from_nowhere_is_unanswered() {
        assert_eq!(Permission::from_code(0xff), Permission::Unanswered);
    }

    /// RED — **a route written into a notification is the route read back out
    /// of it**, through the same key, with nothing lost on the way.
    ///
    /// The pair is what a click is: `NotificationRoute::launch` writes
    /// `w=…&t=…&s=…` on one side of the platform and `NotificationRoute::parse`
    /// reads it on the other, and everything between them is these two
    /// functions. The `&` is in the string on purpose — it is what makes the
    /// Windows arm's XML escaping load-bearing, and it is a character a
    /// dictionary must carry untouched.
    ///
    /// MUTATION: change the key in either function without the other and this
    /// answers `None`.
    #[test]
    fn a_route_survives_the_userinfo_it_travels_in() {
        for launch in ["w=1&t=2&s=3", "w=18446744073709551615&t=0&s=0", ""] {
            let info = route_written_into(launch);
            assert_eq!(route_from(&info).as_deref(), Some(launch));
        }
    }

    /// RED — **a notification that carries no route of ours decodes to
    /// nothing**, which is the honest answer for a notification this build did
    /// not write: one left by an earlier version, or by a probe, under the same
    /// bundle identifier.
    #[test]
    fn a_notification_with_nothing_of_ours_in_it_names_no_pane() {
        let empty: Retained<NSDictionary> = NSDictionary::new();
        assert_eq!(route_from(&empty), None);
        let other = NSDictionary::from_slices(
            &[ns_string!("somebody.else")],
            &[&*NSString::from_str("w=1&t=2&s=3")],
        );
        // SAFETY: as in `route_written_into` — one Rust spelling of the same
        // object, holding property-list types.
        let other: Retained<NSDictionary> = unsafe { Retained::cast_unchecked(other) };
        assert_eq!(route_from(&other), None);
    }

    /// RED — **a value that is not a string is not a route.**
    ///
    /// `userInfo` is a dictionary anybody may put anything in, and reading a
    /// number as a string is the shape of an undefined-behaviour bug rather
    /// than of a wrong answer, which is why the reader downcasts rather than
    /// assumes.
    #[test]
    fn a_route_that_is_not_a_string_is_not_read_as_one() {
        let seven = NSNumber::new_i64(7);
        let numbered = NSDictionary::from_slices(&[launch_key()], &[seven.as_ref() as &AnyObject]);
        // SAFETY: as in `route_written_into` — one Rust spelling of the same
        // object. A number is a property-list type too; what this case is about
        // is that the reader does not take it for a string.
        let numbered: Retained<NSDictionary> = unsafe { Retained::cast_unchecked(numbered) };
        assert_eq!(route_from(&numbered), None);
    }
}
