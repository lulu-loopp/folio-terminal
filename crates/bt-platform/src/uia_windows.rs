//! **The Windows half of [`super`]**: the subclass that answers `WM_GETOBJECT`, the COM providers,
//! and the focus event.
//!
//! # Threads
//!
//! The subclass runs on the window's thread, inside whatever `SendMessage` asked. Every provider
//! is a server-side provider *without* `ProviderOptions_UseComThreading`: UI Automation calls it on
//! its own threads, at any moment, and it answers from the last [`WindowText`] the loop published
//! and from nothing else. The publication is one `Arc` behind one mutex, held only to copy the
//! `Arc` or to swap it ([`Shared::snapshot`], [`UiaHook::publish`]): no provider ever reaches the
//! application's state, and the window thread never waits on a provider.
//!
//! # Laziness
//!
//! Nothing is built until a client asks: the subclass costs one comparison per message, the root
//! provider is made on the first `UiaRootObjectId` question, and only then does [`UiaHook::wanted`]
//! say yes and the loop start building publications.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use windows::Win32::Foundation::{
    E_INVALIDARG, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, SAFEARRAY};
use windows::Win32::System::Ole::{SafeArrayCreateVector, SafeArrayDestroy, SafeArrayPutElement};
use windows::Win32::System::Variant::{VARENUM, VARIANT, VT_I4, VT_R8, VT_UNKNOWN};
use windows::Win32::UI::Accessibility::{
    IRawElementProviderFragment, IRawElementProviderFragment_Impl,
    IRawElementProviderFragmentRoot, IRawElementProviderFragmentRoot_Impl,
    IRawElementProviderSimple, IRawElementProviderSimple_Impl, ITextProvider_Impl, ITextProvider2,
    ITextProvider2_Impl, ITextRangeProvider, ITextRangeProvider_Impl, IValueProvider,
    IValueProvider_Impl, NavigateDirection, NavigateDirection_FirstChild,
    NavigateDirection_LastChild, NavigateDirection_NextSibling, NavigateDirection_Parent,
    NavigateDirection_PreviousSibling, ProviderOptions, ProviderOptions_ServerSideProvider,
    SupportedTextSelection, SupportedTextSelection_Single, TextPatternRangeEndpoint,
    TextPatternRangeEndpoint_Start, TextUnit, UIA_AutomationFocusChangedEventId,
    UIA_AutomationIdPropertyId, UIA_ClassNamePropertyId, UIA_ControlTypePropertyId,
    UIA_E_ELEMENTNOTAVAILABLE, UIA_E_INVALIDOPERATION, UIA_HasKeyboardFocusPropertyId,
    UIA_IsContentElementPropertyId, UIA_IsControlElementPropertyId, UIA_IsEnabledPropertyId,
    UIA_IsKeyboardFocusablePropertyId, UIA_IsReadOnlyAttributeId, UIA_NamePropertyId,
    UIA_PATTERN_ID, UIA_PROPERTY_ID, UIA_TEXTATTRIBUTE_ID, UIA_TextPattern2Id, UIA_TextPatternId,
    UIA_ValuePatternId, UiaAppendRuntimeId, UiaClientsAreListening, UiaDisconnectProvider,
    UiaGetReservedNotSupportedValue, UiaHostProviderFromHwnd, UiaPoint, UiaRaiseAutomationEvent,
    UiaRect, UiaReturnRawElementProvider,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::WM_DESTROY;
use windows::core::{
    BOOL, BSTR, ComObject, Error, HRESULT, IUnknown, Interface, PCWSTR, Ref, Result, implement,
};

use super::{Boundary, ClientRect, PaneText, Shape, WindowText, asks_for_the_root};
use crate::NativeWindow;

/// "BTUA". Distinct from every other subclass id in this crate.
const UIA_SUBCLASS_ID: usize = 0x4254_5541;

/// No focus request parked.
const NO_SEAT: u64 = u64::MAX;

/// **What every provider of one window shares**: the window, the publication, and the two flags
/// the loop reads. Free-threaded: every field is either fixed at birth, atomic, or the one mutex.
pub(super) struct Shared {
    /// The window's `HWND`, as a number: the handle is the window thread's, the number is only
    /// ever handed back to the system's coordinate calls, which take it from any thread.
    hwnd: isize,
    shape: Shape,
    /// **The publication.** Held only to copy the `Arc` out or to swap a new one in.
    snapshot: Mutex<Arc<WindowText>>,
    /// A client has asked for the root: the loop builds publications from now on.
    wanted: AtomicBool,
    /// The window is alive: after `WM_DESTROY` every provider answers "element not available".
    alive: AtomicBool,
    /// A client's `SetFocus` on a pane, parked for the loop ([`NO_SEAT`] when none is).
    focus_asked: AtomicU64,
    /// Wakes the loop: on the first question, and on a parked focus request. It sends one
    /// event and waits for nothing.
    asked: Box<dyn Fn() + Send + Sync>,
}

impl Shared {
    /// **The last publication** — one lock, one `Arc` copy, nothing else under it.
    pub(super) fn snapshot(&self) -> Arc<WindowText> {
        Arc::clone(&self.snapshot.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Wake the loop.
    pub(super) fn wake(&self) {
        (self.asked)();
    }

    fn hwnd(&self) -> HWND {
        HWND(self.hwnd as *mut c_void)
    }

    fn alive(&self) -> Result<()> {
        if self.alive.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(gone())
        }
    }

    /// Where the client area's origin is on the screen, now (R5: client pixels in the
    /// publication, screen pixels at the question).
    fn origin(&self) -> (f64, f64) {
        let mut point = POINT { x: 0, y: 0 };
        // SAFETY: a live window's handle and a point on this stack; a refusal leaves (0, 0).
        let _ = unsafe { ClientToScreen(self.hwnd(), &raw mut point) };
        (f64::from(point.x), f64::from(point.y))
    }

    /// A screen point in client pixels.
    fn to_client(&self, x: f64, y: f64) -> (f64, f64) {
        let (left, top) = self.origin();
        (x - left, y - top)
    }

    fn to_screen(&self, rect: ClientRect) -> UiaRect {
        let (left, top) = self.origin();
        UiaRect {
            left: rect.left + left,
            top: rect.top + top,
            width: rect.width,
            height: rect.height,
        }
    }
}

/// The answer for a provider whose window, pane or text has gone.
fn gone() -> Error {
    Error::from_hresult(hresult(UIA_E_ELEMENTNOTAVAILABLE))
}

fn invalid_operation() -> Error {
    Error::from_hresult(hresult(UIA_E_INVALIDOPERATION))
}

/// A `u32` error number of `UIAutomationCoreApi.h` as the `HRESULT` it is.
fn hresult(code: u32) -> HRESULT {
    HRESULT(i32::from_ne_bytes(code.to_ne_bytes()))
}

/// A null interface out-parameter: the shims write nothing and answer `S_OK`.
fn none<T>() -> Result<T> {
    Err(Error::empty())
}

// ── the hook ──────────────────────────────────────────────────────────────

/// **One window's answer to UI Automation.** Dropping it releases the root and removes the
/// subclass.
pub struct UiaHook {
    hwnd: HWND,
    /// Held for as long as the subclass is installed: the subclass reaches it through the
    /// reference data and nothing may move it.
    data: Box<HookData>,
}

/// What the subclass reads, on the window's thread only.
pub(super) struct HookData {
    shared: Arc<Shared>,
    /// The root provider, made on the first question ([`HookData::root`]).
    root: RefCell<Option<ComObject<Root>>>,
    /// The pane the last focus event named, so a publication raises one only when it moved.
    raised: Cell<Option<u64>>,
    /// The root has been handed back (at `WM_DESTROY`, or at the hook's drop).
    released: Cell<bool>,
}

impl HookData {
    pub(super) fn new(hwnd: isize, shape: Shape, asked: Box<dyn Fn() + Send + Sync>) -> Self {
        Self {
            shared: Arc::new(Shared {
                hwnd,
                shape,
                snapshot: Mutex::new(Arc::new(WindowText::default())),
                wanted: AtomicBool::new(false),
                alive: AtomicBool::new(true),
                focus_asked: AtomicU64::new(NO_SEAT),
                asked,
            }),
            root: RefCell::new(None),
            raised: Cell::new(None),
            released: Cell::new(false),
        }
    }

    /// **The root provider, made on the first question.** The first one also says the window is
    /// wanted and wakes the loop, which publishes the first text on that turn.
    pub(super) fn root(&self) -> ComObject<Root> {
        if let Some(root) = self.root.borrow().as_ref() {
            return root.clone();
        }
        let root = ComObject::new(Root {
            shared: Arc::clone(&self.shared),
        });
        *self.root.borrow_mut() = Some(root.clone());
        if !self.shared.wanted.swap(true, Ordering::AcqRel) {
            self.shared.wake();
        }
        root
    }

    /// Whether the root has been made.
    pub(super) fn made(&self) -> bool {
        self.root.borrow().is_some()
    }

    pub(super) fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    /// **The window is going: every provider says so from now on, and the root is handed back.**
    fn release(&self, hwnd: HWND) {
        if self.released.replace(true) {
            return;
        }
        self.shared.alive.store(false, Ordering::Release);
        let Some(root) = self.root.borrow_mut().take() else {
            return;
        };
        let simple: IRawElementProviderSimple = root.to_interface();
        // SAFETY: the window's own thread and handle; a null provider tells UI Automation the
        // window answers no more (`UiaReturnRawElementProvider`'s documented release), and the
        // disconnect lets every client's proxy of the root go.
        unsafe {
            let _ = UiaReturnRawElementProvider(hwnd, WPARAM(0), LPARAM(0), None);
            let _ = UiaDisconnectProvider(&simple);
        }
    }

    /// **A new publication**, and the focus event it owes if the pane holding the keyboard moved.
    pub(super) fn publish(&self, text: WindowText) -> Option<UiaEvents> {
        let focused = text.focused();
        let old = {
            let mut held = self
                .shared
                .snapshot
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            std::mem::replace(&mut *held, Arc::new(text))
        };
        drop(old);
        let before = self.raised.replace(focused);
        let seat = focused.filter(|seat| before != Some(*seat))?;
        let root = self.root.borrow().clone()?;
        Some(UiaEvents { root, seat })
    }
}

impl UiaHook {
    /// **Subclass `window`.** `asked` wakes the loop: it is called on the window's thread at the
    /// first question, and on one of UI Automation's threads when a client sets the focus on a
    /// pane — so it may only send the wake.
    ///
    /// # Errors
    /// `SetWindowSubclass` refused, with the system's error number.
    pub fn install(
        window: NativeWindow,
        shape: Shape,
        asked: Box<dyn Fn() + Send + Sync>,
    ) -> std::result::Result<Self, String> {
        let hwnd = window.as_hwnd();
        let data = Box::new(HookData::new(hwnd.0 as isize, shape, asked));
        let reference_data = (&raw const *data) as usize;
        // SAFETY: called on the window's own thread with a live HWND, and the box above outlives
        // the subclass — `Drop` removes it first.
        let installed = unsafe {
            SetWindowSubclass(hwnd, Some(uia_subclass), UIA_SUBCLASS_ID, reference_data)
        };
        if !installed.as_bool() {
            return Err(format!(
                "SetWindowSubclass(ui automation) failed: {}",
                // SAFETY: reads this thread's last error; no preconditions.
                unsafe { GetLastError().0 }
            ));
        }
        Ok(Self { hwnd, data })
    }

    /// **Whether a client has asked** — until one has, the loop builds nothing.
    #[must_use]
    pub fn wanted(&self) -> bool {
        self.data.shared.wanted.load(Ordering::Acquire)
    }

    /// **The loop's publication**: swapped in under the one lock, and the focus event owed, if
    /// the pane holding the keyboard moved, for the events worker to raise (R6).
    pub fn publish(&self, text: WindowText) -> Option<UiaEvents> {
        self.data.publish(text)
    }

    /// **The pane a client asked to focus**, taken: once per request.
    #[must_use]
    pub fn take_focus_request(&self) -> Option<u64> {
        let seat = self.data.shared.focus_asked.swap(NO_SEAT, Ordering::AcqRel);
        (seat != NO_SEAT).then_some(seat)
    }
}

impl Drop for UiaHook {
    fn drop(&mut self) {
        self.data.release(self.hwnd);
        // SAFETY: dropped on the thread that installed it; the subclass goes before the box it
        // reads through.
        unsafe {
            let _ = RemoveWindowSubclass(self.hwnd, Some(uia_subclass), UIA_SUBCLASS_ID);
        }
    }
}

/// The subclass procedure: the root question is answered here and not forwarded (it is this
/// window's to answer); `WM_DESTROY` releases the root and is forwarded; everything else is
/// forwarded untouched.
pub(super) unsafe extern "system" fn uia_subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    reference_data: usize,
) -> LRESULT {
    let data = reference_data as *const HookData;
    if !data.is_null() {
        // SAFETY: the owning `UiaHook` holds this box for the whole installed interval and
        // removes the subclass before freeing it, on this same thread.
        let data = unsafe { &*data };
        if asks_for_the_root(message, lparam.0) && !data.released.get() {
            let simple: IRawElementProviderSimple = data.root().to_interface();
            // SAFETY: the message's own window and parameters, and a provider this window owns.
            return unsafe { UiaReturnRawElementProvider(hwnd, wparam, lparam, &simple) };
        }
        if message == WM_DESTROY {
            data.release(hwnd);
        }
    }
    // SAFETY: forwarding untouched messages is the required subclass contract.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

// ── the event road ────────────────────────────────────────────────────────

/// **A focus event owed** — made by [`UiaHook::publish`] on the window's thread, raised by
/// [`UiaEvents::raise`] on the events worker (R6: never on the window thread, because a client's
/// in-process handler runs inside the raise).
pub struct UiaEvents {
    root: ComObject<Root>,
    seat: u64,
}

impl UiaEvents {
    /// **Raise it**, on a worker: nothing when no client listens, or when the pane or the window
    /// is gone by now.
    pub fn raise(self, worker: &crate::admission::WorkerCtx) {
        let _ = worker;
        // SAFETY: joins this thread to the multithreaded apartment for the raise; balanced below
        // when it succeeded.
        let joined = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        let shared = &self.root.shared;
        // SAFETY: no preconditions.
        let listening = unsafe { UiaClientsAreListening() }.as_bool();
        if listening
            && shared.alive.load(Ordering::Acquire)
            && shared.snapshot().pane(self.seat).is_some()
        {
            let pane: IRawElementProviderSimple = self.root.pane(self.seat).to_interface();
            // SAFETY: a provider this window owns; a refusal only means no client heard.
            let _ = unsafe { UiaRaiseAutomationEvent(&pane, UIA_AutomationFocusChangedEventId) };
        }
        drop(self);
        if joined {
            // SAFETY: balances the successful `CoInitializeEx` above, on the same thread.
            unsafe { CoUninitialize() };
        }
    }
}

// ── the providers ─────────────────────────────────────────────────────────

/// **The window's root fragment**: hosted by the window (`UiaHostProviderFromHwnd` gives its
/// name, rectangle and runtime id), its children the active tab's terminal panes.
#[implement(IRawElementProviderSimple, IRawElementProviderFragment, IRawElementProviderFragmentRoot)]
pub(super) struct Root {
    shared: Arc<Shared>,
}

impl Root_Impl {
    /// A provider of one of this root's panes.
    fn pane(&self, seat: u64) -> ComObject<Pane> {
        ComObject::new(Pane {
            shared: Arc::clone(&self.shared),
            root: self.to_object(),
            seat,
        })
    }

    fn pane_fragment(&self, seat: u64) -> IRawElementProviderFragment {
        self.pane(seat).to_interface()
    }
}

impl IRawElementProviderSimple_Impl for Root_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, _patternid: UIA_PATTERN_ID) -> Result<IUnknown> {
        none()
    }

    fn GetPropertyValue(&self, _propertyid: UIA_PROPERTY_ID) -> Result<VARIANT> {
        Ok(VARIANT::default())
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        self.shared.alive()?;
        // SAFETY: the window's handle; the host provider is the system's own for it.
        unsafe { UiaHostProviderFromHwnd(self.shared.hwnd()) }
    }
}

impl IRawElementProviderFragment_Impl for Root_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        self.shared.alive()?;
        let text = self.shared.snapshot();
        let seat = match direction {
            NavigateDirection_FirstChild => text.panes.first(),
            NavigateDirection_LastChild => text.panes.last(),
            _ => None,
        };
        match seat {
            Some(pane) => Ok(self.pane_fragment(pane.seat)),
            None => none(),
        }
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        // The host provider gives the root's runtime id.
        Ok(std::ptr::null_mut())
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        // The host provider gives the root's rectangle.
        Ok(UiaRect::default())
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> Result<()> {
        Ok(())
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        Ok(self.to_interface())
    }
}

impl IRawElementProviderFragmentRoot_Impl for Root_Impl {
    fn ElementProviderFromPoint(&self, x: f64, y: f64) -> Result<IRawElementProviderFragment> {
        self.shared.alive()?;
        let (x, y) = self.shared.to_client(x, y);
        let text = self.shared.snapshot();
        match text.panes.iter().find(|pane| pane.body.contains(x, y)) {
            Some(pane) => Ok(self.pane_fragment(pane.seat)),
            None => none(),
        }
    }

    fn GetFocus(&self) -> Result<IRawElementProviderFragment> {
        self.shared.alive()?;
        match self.shared.snapshot().focused() {
            Some(seat) => Ok(self.pane_fragment(seat)),
            None => none(),
        }
    }
}

/// **One terminal pane**: a fragment of the root, its own Text pattern, and for the edit shape
/// the Value pattern. It names its pane by seat and reads the pane from the publication at each
/// question: a pane the publication no longer has is gone.
#[implement(IRawElementProviderSimple, IRawElementProviderFragment, ITextProvider2, IValueProvider)]
pub(super) struct Pane {
    shared: Arc<Shared>,
    root: ComObject<Root>,
    seat: u64,
}

impl Pane {
    /// The pane's publication now, and its answer computed from it.
    fn with<T>(&self, answer: impl FnOnce(&PaneText) -> Result<T>) -> Result<T> {
        self.shared.alive()?;
        let text = self.shared.snapshot();
        let pane = text.pane(self.seat).ok_or_else(gone)?;
        answer(pane)
    }
}

impl Pane_Impl {
    fn range(&self, start: u32, end: u32) -> ITextRangeProvider {
        ComObject::new(Range {
            pane: self.to_object(),
            ends: AtomicU64::new(pack(start, end)),
        })
        .to_interface()
    }

    fn sibling(&self, step: isize) -> Result<IRawElementProviderFragment> {
        let text = self.shared.snapshot();
        let Some(index) = text.panes.iter().position(|pane| pane.seat == self.seat) else {
            return Err(gone());
        };
        match index
            .checked_add_signed(step)
            .and_then(|next| text.panes.get(next))
        {
            Some(pane) => Ok(self.root.pane_fragment(pane.seat)),
            None => none(),
        }
    }
}

impl IRawElementProviderSimple_Impl for Pane_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, patternid: UIA_PATTERN_ID) -> Result<IUnknown> {
        self.shared.alive()?;
        let text = patternid == UIA_TextPatternId || patternid == UIA_TextPattern2Id;
        let value = patternid == UIA_ValuePatternId && self.shared.shape.has_value_pattern();
        if text || value {
            Ok(self.to_interface())
        } else {
            none()
        }
    }

    fn GetPropertyValue(&self, propertyid: UIA_PROPERTY_ID) -> Result<VARIANT> {
        self.with(|pane| {
            Ok(match propertyid {
                UIA_ControlTypePropertyId => VARIANT::from(self.shared.shape.control_type_id()),
                UIA_NamePropertyId => VARIANT::from(pane.name.as_str()),
                UIA_AutomationIdPropertyId => VARIANT::from(format!("pane-{}", pane.seat).as_str()),
                UIA_ClassNamePropertyId => VARIANT::from("FolioTerminalPane"),
                UIA_HasKeyboardFocusPropertyId => VARIANT::from(pane.holds_keyboard),
                UIA_IsKeyboardFocusablePropertyId
                | UIA_IsEnabledPropertyId
                | UIA_IsControlElementPropertyId
                | UIA_IsContentElementPropertyId => VARIANT::from(true),
                _ => VARIANT::default(),
            })
        })
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        none()
    }
}

impl IRawElementProviderFragment_Impl for Pane_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        self.shared.alive()?;
        match direction {
            NavigateDirection_Parent => Ok(self.root.to_interface()),
            NavigateDirection_NextSibling => self.sibling(1),
            NavigateDirection_PreviousSibling => self.sibling(-1),
            _ => none(),
        }
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        let low = i32::from_ne_bytes(((self.seat & 0xFFFF_FFFF) as u32).to_ne_bytes());
        let high = i32::from_ne_bytes(((self.seat >> 32) as u32).to_ne_bytes());
        let append = i32::try_from(UiaAppendRuntimeId).unwrap_or(3);
        safe_array_i4(&[append, low, high])
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        self.with(|pane| Ok(self.shared.to_screen(pane.body)))
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> Result<()> {
        self.with(|_| Ok(()))?;
        self.shared.focus_asked.store(self.seat, Ordering::Release);
        self.shared.wake();
        Ok(())
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        Ok(self.root.to_interface())
    }
}

impl ITextProvider_Impl for Pane_Impl {
    fn GetSelection(&self) -> Result<*mut SAFEARRAY> {
        let ends = self.with(|pane| Ok(pane.selection().or(pane.caret().map(|at| (at, at)))))?;
        let ranges: Vec<ITextRangeProvider> = ends
            .into_iter()
            .map(|(start, end)| self.range(start, end))
            .collect();
        safe_array_unknown(&ranges)
    }

    fn GetVisibleRanges(&self) -> Result<*mut SAFEARRAY> {
        let len = self.with(|pane| Ok(pane.len()))?;
        safe_array_unknown(&[self.range(0, len)])
    }

    fn RangeFromChild(
        &self,
        _childelement: Ref<IRawElementProviderSimple>,
    ) -> Result<ITextRangeProvider> {
        Err(Error::from_hresult(E_INVALIDARG))
    }

    fn RangeFromPoint(&self, point: &UiaPoint) -> Result<ITextRangeProvider> {
        let (x, y) = self.shared.to_client(point.x, point.y);
        let at = self.with(|pane| Ok(pane.offset_at(x, y)))?;
        Ok(self.range(at, at))
    }

    fn DocumentRange(&self) -> Result<ITextRangeProvider> {
        let len = self.with(|pane| Ok(pane.len()))?;
        Ok(self.range(0, len))
    }

    fn SupportedTextSelection(&self) -> Result<SupportedTextSelection> {
        Ok(SupportedTextSelection_Single)
    }
}

impl ITextProvider2_Impl for Pane_Impl {
    fn RangeFromAnnotation(
        &self,
        _annotationelement: Ref<IRawElementProviderSimple>,
    ) -> Result<ITextRangeProvider> {
        Err(Error::from_hresult(E_INVALIDARG))
    }

    fn GetCaretRange(&self, isactive: *mut BOOL) -> Result<ITextRangeProvider> {
        let (active, at) =
            self.with(|pane| Ok((pane.holds_keyboard, pane.caret().unwrap_or(0))))?;
        if !isactive.is_null() {
            // SAFETY: UI Automation's out-parameter, checked non-null.
            unsafe { *isactive = BOOL::from(active) };
        }
        Ok(self.range(at, at))
    }
}

impl IValueProvider_Impl for Pane_Impl {
    fn SetValue(&self, _val: &PCWSTR) -> Result<()> {
        Err(invalid_operation())
    }

    fn Value(&self) -> Result<BSTR> {
        self.with(|pane| Ok(BSTR::from(pane.text(0, pane.len(), -1))))
    }

    fn IsReadOnly(&self) -> Result<BOOL> {
        Ok(BOOL::from(false))
    }
}

/// Two offsets in one word, so a range moves atomically under a client on any thread.
fn pack(start: u32, end: u32) -> u64 {
    (u64::from(start) << 32) | u64::from(end)
}

fn unpack(ends: u64) -> (u32, u32) {
    // The two halves of `pack`'s word.
    #[allow(clippy::cast_possible_truncation)]
    ((ends >> 32) as u32, ends as u32)
}

/// **A text range of one pane**: two offsets into the publication, clamped into whichever
/// publication answers the next question.
#[implement(ITextRangeProvider)]
pub(super) struct Range {
    pane: ComObject<Pane>,
    ends: AtomicU64,
}

impl Range {
    fn ends(&self, pane: &PaneText) -> (u32, u32) {
        let (start, end) = unpack(self.ends.load(Ordering::Acquire));
        let (start, end) = (pane.clamp(start), pane.clamp(end));
        (start.min(end), end.max(start))
    }

    fn set(&self, start: u32, end: u32) {
        self.ends.store(pack(start, end), Ordering::Release);
    }

    /// The other range, when it is one of this window's ranges of the same pane.
    fn peer<'a>(&self, other: &'a Ref<ITextRangeProvider>) -> Result<&'a Range_Impl> {
        let other = other.ok()?;
        let other = other.cast_object_ref::<Range>()?;
        if other.pane.seat == self.pane.seat && Arc::ptr_eq(&other.pane.shared, &self.pane.shared)
        {
            Ok(other)
        } else {
            Err(Error::from_hresult(E_INVALIDARG))
        }
    }
}

fn is_start(endpoint: TextPatternRangeEndpoint) -> bool {
    endpoint == TextPatternRangeEndpoint_Start
}

impl ITextRangeProvider_Impl for Range_Impl {
    fn Clone(&self) -> Result<ITextRangeProvider> {
        Ok(ComObject::new(Range {
            pane: self.pane.clone(),
            ends: AtomicU64::new(self.ends.load(Ordering::Acquire)),
        })
        .to_interface())
    }

    fn Compare(&self, range: Ref<ITextRangeProvider>) -> Result<BOOL> {
        let Ok(other) = self.peer(&range) else {
            return Ok(BOOL::from(false));
        };
        self.pane
            .with(|pane| Ok(BOOL::from(self.ends(pane) == other.ends(pane))))
    }

    fn CompareEndpoints(
        &self,
        endpoint: TextPatternRangeEndpoint,
        targetrange: Ref<ITextRangeProvider>,
        targetendpoint: TextPatternRangeEndpoint,
    ) -> Result<i32> {
        let other = self.peer(&targetrange)?;
        self.pane.with(|pane| {
            let (start, end) = self.ends(pane);
            let (other_start, other_end) = other.ends(pane);
            let mine = if is_start(endpoint) { start } else { end };
            let theirs = if is_start(targetendpoint) {
                other_start
            } else {
                other_end
            };
            Ok(match mine.cmp(&theirs) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        })
    }

    fn ExpandToEnclosingUnit(&self, unit: TextUnit) -> Result<()> {
        self.pane.with(|pane| {
            let (start, _) = self.ends(pane);
            let (start, end) = pane.enclosing(start, Boundary::of_text_unit(unit.0));
            self.set(start, end);
            Ok(())
        })
    }

    fn FindAttribute(
        &self,
        _attributeid: UIA_TEXTATTRIBUTE_ID,
        _val: &VARIANT,
        _backward: BOOL,
    ) -> Result<ITextRangeProvider> {
        none()
    }

    fn FindText(
        &self,
        text: &BSTR,
        backward: BOOL,
        ignorecase: BOOL,
    ) -> Result<ITextRangeProvider> {
        let needle = text.to_string();
        let found = self.pane.with(|pane| {
            let (start, end) = self.ends(pane);
            Ok(pane.find(start, end, &needle, backward.as_bool(), ignorecase.as_bool()))
        })?;
        match found {
            Some((start, end)) => Ok(self.pane.range(start, end)),
            None => none(),
        }
    }

    fn GetAttributeValue(&self, attributeid: UIA_TEXTATTRIBUTE_ID) -> Result<VARIANT> {
        self.pane.shared.alive()?;
        if attributeid == UIA_IsReadOnlyAttributeId {
            return Ok(VARIANT::from(false));
        }
        // SAFETY: no preconditions; the system's own "not supported" sentinel.
        let unsupported = unsafe { UiaGetReservedNotSupportedValue() }?;
        Ok(VARIANT::from(unsupported))
    }

    fn GetBoundingRectangles(&self) -> Result<*mut SAFEARRAY> {
        let rectangles = self.pane.with(|pane| {
            let (start, end) = self.ends(pane);
            Ok(pane.rectangles(start, end))
        })?;
        let mut numbers = Vec::with_capacity(rectangles.len() * 4);
        for rect in rectangles {
            let screen = self.pane.shared.to_screen(rect);
            numbers.extend([screen.left, screen.top, screen.width, screen.height]);
        }
        safe_array_r8(&numbers)
    }

    fn GetEnclosingElement(&self) -> Result<IRawElementProviderSimple> {
        self.pane.shared.alive()?;
        Ok(self.pane.to_interface())
    }

    fn GetText(&self, maxlength: i32) -> Result<BSTR> {
        self.pane.with(|pane| {
            let (start, end) = self.ends(pane);
            Ok(BSTR::from(pane.text(start, end, maxlength)))
        })
    }

    fn Move(&self, unit: TextUnit, count: i32) -> Result<i32> {
        self.pane.with(|pane| {
            let (start, end) = self.ends(pane);
            let (start, end, moved) =
                pane.moved_range(start, end, Boundary::of_text_unit(unit.0), count);
            self.set(start, end);
            Ok(moved)
        })
    }

    fn MoveEndpointByUnit(
        &self,
        endpoint: TextPatternRangeEndpoint,
        unit: TextUnit,
        count: i32,
    ) -> Result<i32> {
        self.pane.with(|pane| {
            let (start, end) = self.ends(pane);
            let unit = Boundary::of_text_unit(unit.0);
            let moved = if is_start(endpoint) {
                let (at, moved) = pane.moved(start, unit, count);
                self.set(at, end.max(at));
                moved
            } else {
                let (at, moved) = pane.moved(end, unit, count);
                self.set(start.min(at), at);
                moved
            };
            Ok(moved)
        })
    }

    fn MoveEndpointByRange(
        &self,
        endpoint: TextPatternRangeEndpoint,
        targetrange: Ref<ITextRangeProvider>,
        targetendpoint: TextPatternRangeEndpoint,
    ) -> Result<()> {
        let other = self.peer(&targetrange)?;
        self.pane.with(|pane| {
            let (start, end) = self.ends(pane);
            let (other_start, other_end) = other.ends(pane);
            let at = if is_start(targetendpoint) {
                other_start
            } else {
                other_end
            };
            if is_start(endpoint) {
                self.set(at, end.max(at));
            } else {
                self.set(start.min(at), at);
            }
            Ok(())
        })
    }

    fn Select(&self) -> Result<()> {
        // Selecting from a client is 88b's (R1).
        Err(invalid_operation())
    }

    fn AddToSelection(&self) -> Result<()> {
        Err(invalid_operation())
    }

    fn RemoveFromSelection(&self) -> Result<()> {
        Err(invalid_operation())
    }

    fn ScrollIntoView(&self, _aligntotop: BOOL) -> Result<()> {
        // A pane's text is what it shows (scrollback is 88c's), so every range is in view.
        self.pane.shared.alive()
    }

    fn GetChildren(&self) -> Result<*mut SAFEARRAY> {
        safe_array_unknown::<ITextRangeProvider>(&[])
    }
}

// ── SAFEARRAYs ────────────────────────────────────────────────────────────

/// A one-dimensional `SAFEARRAY` of `vt`, each element put by `put`; the caller owns it.
fn safe_array(
    vt: VARENUM,
    count: usize,
    put: impl Fn(*mut SAFEARRAY, i32) -> Result<()>,
) -> Result<*mut SAFEARRAY> {
    let length = u32::try_from(count).map_err(|_| Error::from_hresult(E_INVALIDARG))?;
    // SAFETY: a vector of a fixed element type; null is the allocation's refusal.
    let array = unsafe { SafeArrayCreateVector(vt, 0, length) };
    if array.is_null() {
        return Err(Error::from_hresult(hresult(0x8007_000E)));
    }
    for index in 0..length {
        let index = i32::try_from(index).unwrap_or(i32::MAX);
        if let Err(error) = put(array, index) {
            // SAFETY: the array made above, not handed to anybody.
            let _ = unsafe { SafeArrayDestroy(array) };
            return Err(error);
        }
    }
    Ok(array)
}

fn safe_array_i4(values: &[i32]) -> Result<*mut SAFEARRAY> {
    safe_array(VT_I4, values.len(), |array, index| {
        let value = values[usize::try_from(index).unwrap_or(0)];
        // SAFETY: an index inside the vector; the element is copied in.
        unsafe { SafeArrayPutElement(array, &raw const index, (&raw const value).cast()) }
    })
}

fn safe_array_r8(values: &[f64]) -> Result<*mut SAFEARRAY> {
    safe_array(VT_R8, values.len(), |array, index| {
        let value = values[usize::try_from(index).unwrap_or(0)];
        // SAFETY: an index inside the vector; the element is copied in.
        unsafe { SafeArrayPutElement(array, &raw const index, (&raw const value).cast()) }
    })
}

fn safe_array_unknown<T: Interface>(values: &[T]) -> Result<*mut SAFEARRAY> {
    safe_array(VT_UNKNOWN, values.len(), |array, index| {
        let value = &values[usize::try_from(index).unwrap_or(0)];
        // SAFETY: an index inside the vector; for `VT_UNKNOWN` the element is the interface
        // pointer itself, which the array AddRefs.
        unsafe { SafeArrayPutElement(array, &raw const index, value.as_raw()) }
    })
}

#[cfg(test)]
#[path = "uia_windows_tests.rs"]
mod tests;
