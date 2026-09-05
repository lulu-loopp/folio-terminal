//! **The class Explorer creates to draw and to run Folio's first-page menu
//! item** — `docs/DESIGN.md` §7.4a.
//!
//! A sixth unsafe boundary, against a sixth thing. `windows_impl` is Win32 for
//! this window's sake, [`crate::webview`] is WebView2, [`crate::hang`] is Win32
//! turned on this process, [`crate::attention_pipe`] is a channel other
//! processes speak into, [`crate::hotkey`] is the keyboard while somebody else
//! has it — and this is **this process answering questions another program
//! asks**. It is the only one of the six where the caller is in charge: Explorer
//! decides when the object is made, how long it lives, which thread it is called
//! on, and when the process may stop.
//!
//! # The shape, and why it is an executable and not a DLL
//!
//! Windows 11's first-page context menu is `IExplorerCommand` declared by a
//! package. The usual way to declare one is an in-process DLL loaded into a
//! surrogate; this declares an **out-of-process server**, which is `folio.exe`
//! itself started with `--explorer-command` (see
//! `packaging/msix/AppxManifest.xml`). The reason is that the alternative is a
//! second binary: a `.dll` that would have to carry its own copy of the menu's
//! words, its own version, its own signature and its own place in the archive,
//! and that would answer for a `folio.exe` it has no way to check it is beside.
//! One binary answers as itself.
//!
//! What it costs is a process launch per menu, which is the cost this module's
//! idle rule is about.
//!
//! # The life of the process
//!
//! COM starts it, so COM ends it. The server registers its class object,
//! resumes, and pumps messages until nothing has held one of its objects for
//! [`IDLE_LINGER`]. It does not exit the instant the last reference goes,
//! because a right-click is usually followed by another one and a menu that
//! costs a process launch every time is a menu that feels slow; it does not stay
//! either, because a terminal that leaves a process behind after somebody looked
//! at a menu is a terminal with a leak.
//!
//! **The apartment is single-threaded and there is a real message pump.** Shell
//! extensions are called on an apartment thread, and an STA with no pump is an
//! STA whose calls never arrive — the failure is not an error, it is a menu item
//! that never appears.
//!
//! # What this module does not decide
//!
//! Not the words, not the icon and not what happens on a click. Those are the
//! product's ([`bt_app::explorer_menu`]), handed in as [`Verb`], for the reason
//! §7.4 splits `ContextMenuShape` from the registry writes: the only things that
//! can be wrong here are apartment, lifetime and marshalling, and none of them
//! is easier to see with a string table in the middle of it.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicIsize, Ordering},
    },
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::{CLASS_E_NOAGGREGATION, E_NOTIMPL, E_OUTOFMEMORY, E_POINTER},
        System::Com::{
            CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoInitializeEx, CoRegisterClassObject,
            CoResumeClassObjects, CoRevokeClassObject, CoTaskMemAlloc, CoTaskMemFree,
            CoUninitialize, IBindCtx, IClassFactory, IClassFactory_Impl, REGCLS_MULTIPLEUSE,
            REGCLS_SUSPENDED,
        },
        UI::{
            Shell::{
                ECF_DEFAULT, ECS_ENABLED, IEnumExplorerCommand, IExplorerCommand,
                IExplorerCommand_Impl, IShellItemArray, SIGDN_FILESYSPATH,
            },
            WindowsAndMessaging::{
                DispatchMessageW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
                PeekMessageW, QS_ALLINPUT, TranslateMessage, WM_QUIT,
            },
        },
    },
    core::{BOOL, GUID, IUnknown, Interface, PWSTR, Ref, implement},
};

/// How long the server stays up after the last object it made was released.
///
/// Long enough that a second right-click reuses this process, short enough that
/// nobody finds a `folio.exe` in Task Manager and wonders what it is doing. It
/// is measured from the release and not from the launch, so a menu somebody
/// leaves open does not expire underneath them.
pub const IDLE_LINGER: Duration = Duration::from_secs(10);

/// How long the pump sleeps between askings of whether it is finished.
///
/// It is the deadline on a **message** wait rather than a plain sleep, so the
/// thread stays able to answer a cross-apartment call the instant one arrives:
/// what this number bounds is only how late the process notices that nobody is
/// asking any more.
const IDLE_TICK_MS: u32 = 500;

/// The one thing the product tells this module.
///
/// The words and the picture are read **per call**, because Explorer asks for
/// them per call and the answer depends on the language the user is in.
pub struct Verb {
    /// The menu's words.
    pub title: String,
    /// `"<path to an executable>,<index>"`, the shell's own spelling.
    pub icon: String,
    /// What a click means. Called on the apartment thread, with the folder that
    /// was right-clicked.
    pub invoke: Box<dyn Fn(&Path) + Send + Sync>,
}

/// Live objects this server has handed out, plus explicit `LockServer` holds.
///
/// One counter for both, because the question it answers is one question: is
/// anybody still holding this process open. It is a process-wide static because
/// COM's own accounting is process-wide — a second server object in this process
/// would be a second answer to the same question.
static OUTSTANDING: AtomicIsize = AtomicIsize::new(0);

/// Register the class and answer Explorer until nobody is asking.
///
/// Returns when the pump has stopped, which is the point at which the process
/// should exit. Every failure is fatal to the run and is reported as a sentence:
/// there is no window to put it on and no useful half-working state — a server
/// that could not register its class is a menu item Explorer will start another
/// process for in a moment.
pub fn serve(verb: Verb) -> Result<(), String> {
    // SAFETY: the first COM call on this thread, and the thread is this
    // process's only one. The apartment lasts until `CoUninitialize` below.
    let apartment = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if apartment.is_err() {
        return Err(format!("CoInitializeEx: 0x{:08X}", apartment.0));
    }
    let served = register_and_pump(verb);
    // SAFETY: balances the `CoInitializeEx` above, on the same thread, after
    // every interface this function created has been dropped.
    unsafe { CoUninitialize() };
    served
}

fn register_and_pump(verb: Verb) -> Result<(), String> {
    let factory: IClassFactory = Factory {
        verb: Arc::new(verb),
    }
    .into();

    // **`REGCLS_SUSPENDED`, then resume.** Registering and resuming in one step
    // leaves a window in which a client can create the class before this thread
    // reaches its pump, and an STA object created by a thread that is not
    // pumping is a call that waits for a message loop that has not started.
    //
    // SAFETY: `factory` outlives the registration — it is dropped after
    // `CoRevokeClassObject` below — and the CLSID is the constant the manifest
    // declares.
    let cookie = unsafe {
        CoRegisterClassObject(
            &crate::msix::explorer_command_clsid(),
            &factory,
            CLSCTX_LOCAL_SERVER,
            REGCLS_MULTIPLEUSE | REGCLS_SUSPENDED,
        )
    }
    .map_err(|error| format!("CoRegisterClassObject: {}", error.message()))?;

    // SAFETY: called once, after the registration above, on the thread that owns
    // the apartment.
    let resumed = unsafe { CoResumeClassObjects() };
    if let Err(error) = resumed {
        // SAFETY: `cookie` is the one this function was just given and has not
        // been revoked.
        let _ = unsafe { CoRevokeClassObject(cookie) };
        return Err(format!("CoResumeClassObjects: {}", error.message()));
    }

    pump();

    // SAFETY: the cookie from the registration above, revoked once, before the
    // factory is dropped.
    let _ = unsafe { CoRevokeClassObject(cookie) };
    Ok(())
}

/// The apartment's message loop, and the clock that ends it.
///
/// `PeekMessageW` in a loop rather than `GetMessageW`, because `GetMessageW`
/// sleeps until a message arrives and the thing this loop is waiting for is
/// **nothing arriving**. What it sleeps on instead is
/// [`wait_for_a_message`] — a wait with a deadline, so the reference count is
/// looked at every [`IDLE_TICK_MS`] whether or not anybody has spoken.
///
/// **One clock and not two.** A `WM_TIMER` would also wake this thread, and it
/// would be a second mechanism answering the same question; what decides is this
/// function's own reading of [`Instant`], so a wait that returns early or late
/// costs a little lateness rather than a process that never exits.
fn pump() {
    let mut idle_since = Some(Instant::now());
    loop {
        let mut message = MSG::default();
        // SAFETY: `message` is a live `MSG` this call fills in. `PM_REMOVE`
        // takes the message off this thread's own queue.
        while unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            if message.message == WM_QUIT {
                break;
            }
            // SAFETY: `message` was just filled in by `PeekMessageW` and is not
            // read again before it is refilled.
            unsafe {
                let _ = TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }
        if message.message == WM_QUIT {
            break;
        }
        if OUTSTANDING.load(Ordering::Acquire) > 0 {
            idle_since = None;
        } else {
            let since = *idle_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= IDLE_LINGER {
                break;
            }
        }
        // Between ticks the thread must not spin. A message wait with a deadline
        // is what an apartment sleeps on: it stays able to answer a cross-
        // apartment call the instant one arrives, which a `sleep` would not.
        wait_for_a_message();
    }
}

/// Sleep until this thread has a message or the tick expires.
fn wait_for_a_message() {
    // SAFETY: no handles are waited on, so the array is empty and the call is a
    // timed wait on this thread's own message queue.
    unsafe {
        MsgWaitForMultipleObjectsEx(None, IDLE_TICK_MS, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
    }
}

/// A live handle on this server, counted for the idle rule.
///
/// A guard rather than a pair of calls, so that the decrement cannot be lost on
/// an early return or a panic — which would be a `folio.exe` that never exits.
struct Outstanding;

impl Outstanding {
    fn take() -> Self {
        OUTSTANDING.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for Outstanding {
    fn drop(&mut self) {
        OUTSTANDING.fetch_sub(1, Ordering::AcqRel);
    }
}

// ── the class factory ───────────────────────────────────────────────────────

#[implement(IClassFactory)]
struct Factory {
    verb: Arc<Verb>,
}

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        interface: *const GUID,
        object: *mut *mut core::ffi::c_void,
    ) -> windows::core::Result<()> {
        if object.is_null() {
            return Err(E_POINTER.into());
        }
        // SAFETY: a non-null out parameter the caller owns; COM requires it be
        // cleared before anything else can fail.
        unsafe { *object = std::ptr::null_mut() };
        // Aggregation is a thing this class does not support, and saying so is
        // the documented answer rather than a silent one.
        if !outer.is_null() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let command: IExplorerCommand = Command {
            verb: Arc::clone(&self.verb),
            _outstanding: Outstanding::take(),
        }
        .into();
        // SAFETY: `interface` is the caller's IID and `object` its out
        // parameter; `query` writes one reference into it and takes none.
        unsafe { command.query(&*interface, object).ok() }
    }

    fn LockServer(&self, lock: BOOL) -> windows::core::Result<()> {
        // The count is the same count the objects use, because it answers the
        // same question. `LockServer(TRUE)` without a matching `FALSE` holds this
        // process open for as long as the client lives, which is what a client
        // that calls it is asking for.
        if lock.as_bool() {
            OUTSTANDING.fetch_add(1, Ordering::AcqRel);
        } else {
            OUTSTANDING.fetch_sub(1, Ordering::AcqRel);
        }
        Ok(())
    }
}

// ── the command itself ──────────────────────────────────────────────────────

#[implement(IExplorerCommand)]
struct Command {
    verb: Arc<Verb>,
    _outstanding: Outstanding,
}

impl IExplorerCommand_Impl for Command_Impl {
    fn GetTitle(&self, _items: Ref<IShellItemArray>) -> windows::core::Result<PWSTR> {
        co_task_string(&self.verb.title)
    }

    fn GetIcon(&self, _items: Ref<IShellItemArray>) -> windows::core::Result<PWSTR> {
        co_task_string(&self.verb.icon)
    }

    fn GetToolTip(&self, _items: Ref<IShellItemArray>) -> windows::core::Result<PWSTR> {
        // **Not an empty string.** `E_NOTIMPL` means "there is no tooltip" and
        // leaves the shell's own; an empty one means "the tooltip is nothing",
        // which draws an empty box.
        Err(E_NOTIMPL.into())
    }

    fn GetCanonicalName(&self) -> windows::core::Result<GUID> {
        // A canonical name is for a verb something else invokes by name. Nothing
        // invokes this one but the menu it is on.
        Ok(GUID::zeroed())
    }

    fn GetState(
        &self,
        _items: Ref<IShellItemArray>,
        _may_be_slow: BOOL,
    ) -> windows::core::Result<u32> {
        // Always enabled, and deliberately not conditional on the folder being
        // one Folio can open: every folder is. A state that had to look at the
        // disk would be a menu that waits for a disk, which is what
        // `may_be_slow` exists to warn about.
        Ok(ECS_ENABLED.0 as u32)
    }

    fn Invoke(
        &self,
        items: Ref<IShellItemArray>,
        _context: Ref<IBindCtx>,
    ) -> windows::core::Result<()> {
        let Some(folder) = first_file_system_path(items) else {
            // Nothing that names a place on this disk was handed over. There is
            // no honest guess to make — a window opened somewhere the user did
            // not click is worse than no window — so the click is answered and
            // nothing happens.
            return Ok(());
        };
        (self.verb.invoke)(&folder);
        Ok(())
    }

    fn GetFlags(&self) -> windows::core::Result<u32> {
        Ok(ECF_DEFAULT.0 as u32)
    }

    fn EnumSubCommands(&self) -> windows::core::Result<IEnumExplorerCommand> {
        // One item, no flyout.
        Err(E_NOTIMPL.into())
    }
}

/// The first thing in the array that has a path on this machine.
///
/// **The array is where the folder comes from for both item types.** A verb on
/// `Directory` is handed the folder that was clicked; a verb on
/// `Directory\Background` is handed the folder whose window was clicked. That
/// they arrive the same way is the whole reason one CLSID answers both.
///
/// The first entry and not all of them: the menu item is singular, and a
/// selection of four folders that opened four windows would be a verb doing
/// something nobody asked for.
fn first_file_system_path(items: Ref<IShellItemArray>) -> Option<PathBuf> {
    let items = items.ok().ok()?;
    // SAFETY: `items` is a live `IShellItemArray` borrowed for this call. Every
    // call below is a read on it, and `GetDisplayName`'s buffer is freed here.
    unsafe {
        if items.GetCount().ok()? == 0 {
            return None;
        }
        let item = items.GetItemAt(0).ok()?;
        // `SIGDN_FILESYSPATH` fails for anything that is not on a file system —
        // a library, a search result, *This PC*. That failure is the answer.
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(name.0.cast()));
        path
    }
}

/// A string in the allocator COM's caller will free it with.
///
/// `CoTaskMemAlloc` and not a Rust allocation: the caller of `GetTitle` frees
/// what it is given with `CoTaskMemFree`, and handing it memory from another
/// allocator is a heap corruption in Explorer rather than an error anybody sees.
fn co_task_string(text: &str) -> windows::core::Result<PWSTR> {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let bytes = std::mem::size_of_val(wide.as_slice());
    // SAFETY: a task allocation of exactly the length written below, checked for
    // null before it is written to.
    let buffer = unsafe { CoTaskMemAlloc(bytes) }.cast::<u16>();
    if buffer.is_null() {
        return Err(E_OUTOFMEMORY.into());
    }
    // SAFETY: `buffer` is a fresh allocation of `wide.len()` `u16`s and the two
    // regions cannot overlap.
    unsafe { std::ptr::copy_nonoverlapping(wide.as_ptr(), buffer, wide.len()) };
    Ok(PWSTR(buffer))
}
