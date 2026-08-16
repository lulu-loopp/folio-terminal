//! Folio Windows-landing probe #1: unpackaged toast notifications.
//!
//! Verbs:
//!   toast-probe register     -- write the HKCU AUMID + CLSID/LocalServer32 keys
//!   toast-probe show [secs]  -- SetCurrentProcessExplicitAppUserModelID, register the COM
//!                               activator class object, raise a toast, pump messages so both
//!                               the in-process `Activated` event and the out-of-proc
//!                               INotificationActivationCallback can fire. Logs everything.
//!   toast-probe -Embedding   -- what COM launches when the toast is clicked and no instance
//!                               of the class object is registered (cold activation path).
//!   toast-probe unregister   -- delete every key `register` wrote
//!   toast-probe dumpkeys     -- print the current state of the keys
//!
//! Everything is logged both to stdout and to `toast-probe.log` next to the exe, because the
//! COM cold-activation path is launched by Windows with no console attached.

use std::io::Write as _;

use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{
    ToastActivatedEventArgs, ToastDismissalReason, ToastDismissedEventArgs, ToastFailedEventArgs,
    ToastNotification, ToastNotificationManager,
};
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoInitializeEx, CoRegisterClassObject,
    CoRevokeClassObject, IClassFactory, IClassFactory_Impl, REGCLS_MULTIPLEUSE,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteTreeW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::UI::Notifications::{
    INotificationActivationCallback, INotificationActivationCallback_Impl,
    NOTIFICATION_USER_INPUT_DATA,
};
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
};
use windows::core::{BOOL, GUID, HSTRING, IUnknown, Interface, PCWSTR};

/// The AUMID Folio would use. Reverse-DNS-ish, no spaces; this is the identity the platform
/// keys the notification on and what Action Center groups under.
const AUMID: &str = "Folio.Terminal";
/// The activator CLSID. Any stable GUID we mint; it only has to match between the
/// `CustomActivator` value under the AUMID key and the `HKCU\Software\Classes\CLSID` entry.
const ACTIVATOR_CLSID: GUID = GUID::from_u128(0x9f1b8d21_4c7e_4f0a_9d3b_6a2e5c81f704);
const ACTIVATOR_CLSID_STR: &str = "{9F1B8D21-4C7E-4F0A-9D3B-6A2E5C81F704}";

fn log(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
    if let Ok(exe) = std::env::current_exe() {
        let path = exe.with_file_name("toast-probe.log");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "[pid {}] {line}", std::process::id());
        }
    }
}

// --------------------------------------------------------------------------------------------
// registry helpers
// --------------------------------------------------------------------------------------------

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_string(root: HKEY, subkey: &str, name: &str, value: &str) -> windows::core::Result<()> {
    let sub = wide(subkey);
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            root,
            PCWSTR(sub.as_ptr()),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
        .ok()?;
        let data = wide(value);
        let bytes = std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2);
        let name_w = wide(name);
        let name_ptr = if name.is_empty() { PCWSTR::null() } else { PCWSTR(name_w.as_ptr()) };
        RegSetValueExW(key, name_ptr, None, REG_SZ, Some(bytes)).ok()?;
        let _ = RegCloseKey(key);
    }
    Ok(())
}

fn read_string(root: HKEY, subkey: &str, name: &str) -> Option<String> {
    unsafe {
        let sub = wide(subkey);
        let mut key = HKEY::default();
        RegOpenKeyExW(root, PCWSTR(sub.as_ptr()), None, KEY_READ, &mut key).ok().ok()?;
        let name_w = wide(name);
        let name_ptr = if name.is_empty() { PCWSTR::null() } else { PCWSTR(name_w.as_ptr()) };
        let mut size: u32 = 0;
        RegQueryValueExW(key, name_ptr, None, None, None, Some(&mut size)).ok().ok()?;
        let mut buf = vec![0u8; size as usize];
        RegQueryValueExW(key, name_ptr, None, None, Some(buf.as_mut_ptr()), Some(&mut size))
            .ok()
            .ok()?;
        let _ = RegCloseKey(key);
        let u16s: Vec<u16> = buf
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&c| c != 0)
            .collect();
        Some(String::from_utf16_lossy(&u16s))
    }
}

fn aumid_key() -> String {
    format!("Software\\Classes\\AppUserModelId\\{AUMID}")
}
fn clsid_key() -> String {
    format!("Software\\Classes\\CLSID\\{ACTIVATOR_CLSID_STR}")
}
fn localserver_key() -> String {
    format!("{}\\LocalServer32", clsid_key())
}

fn do_register() -> windows::core::Result<()> {
    let exe = std::env::current_exe().expect("current_exe");
    let exe = exe.to_string_lossy().to_string();

    // (a) The AUMID identity. DisplayName is what Action Center shows as the sender.
    //     IconUri must be an absolute path on disk (no URL, no resource id).
    //     CustomActivator wires the click back to our COM class.
    set_string(HKEY_CURRENT_USER, &aumid_key(), "DisplayName", "Folio")?;
    set_string(
        HKEY_CURRENT_USER,
        &aumid_key(),
        "IconUri",
        "C:\\Windows\\System32\\@WLOGO_96x96.png",
    )?;
    set_string(HKEY_CURRENT_USER, &aumid_key(), "CustomActivator", ACTIVATOR_CLSID_STR)?;

    // (b) The COM activator: a per-user local server pointing at this exe.
    set_string(HKEY_CURRENT_USER, &clsid_key(), "", "Folio Toast Activator")?;
    set_string(HKEY_CURRENT_USER, &localserver_key(), "", &format!("\"{exe}\""))?;

    log(&format!("registered AUMID={AUMID} activator={ACTIVATOR_CLSID_STR} exe={exe}"));
    Ok(())
}

fn do_unregister() {
    unsafe {
        let a = wide(&aumid_key());
        let c = wide(&clsid_key());
        let ra = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(a.as_ptr()));
        let rc = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(c.as_ptr()));
        log(&format!("unregister: aumid_key={ra:?} clsid_key={rc:?}"));
    }
}

fn do_dumpkeys() {
    for (k, v) in [
        (aumid_key(), "DisplayName"),
        (aumid_key(), "IconUri"),
        (aumid_key(), "CustomActivator"),
        (localserver_key(), ""),
    ] {
        log(&format!("HKCU\\{k} [{v}] = {:?}", read_string(HKEY_CURRENT_USER, &k, v)));
    }
}

// --------------------------------------------------------------------------------------------
// COM activator
// --------------------------------------------------------------------------------------------

#[windows::core::implement(INotificationActivationCallback)]
struct Activator;

impl INotificationActivationCallback_Impl for Activator_Impl {
    fn Activate(
        &self,
        appusermodelid: &PCWSTR,
        invokedargs: &PCWSTR,
        data: *const NOTIFICATION_USER_INPUT_DATA,
        count: u32,
    ) -> windows::core::Result<()> {
        let aumid = unsafe { appusermodelid.to_string() }.unwrap_or_default();
        let args = unsafe { invokedargs.to_string() }.unwrap_or_default();
        log(&format!(
            "*** COM ACTIVATE *** aumid={aumid:?} invokedArgs={args:?} inputCount={count} data_null={}",
            data.is_null()
        ));
        Ok(())
    }
}

#[windows::core::implement(IClassFactory)]
struct ActivatorFactory;

impl IClassFactory_Impl for ActivatorFactory_Impl {
    fn CreateInstance(
        &self,
        outer: windows::core::Ref<'_, IUnknown>,
        iid: *const GUID,
        object: *mut *mut core::ffi::c_void,
    ) -> windows::core::Result<()> {
        log("class factory: CreateInstance called");
        if !outer.is_null() {
            return Err(windows::Win32::Foundation::CLASS_E_NOAGGREGATION.into());
        }
        let unknown: IUnknown = Activator.into();
        unsafe { unknown.query(iid, object).ok() }
    }
    fn LockServer(&self, _lock: BOOL) -> windows::core::Result<()> {
        Ok(())
    }
}

// --------------------------------------------------------------------------------------------
// toast
// --------------------------------------------------------------------------------------------

fn pump(secs: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    let mut msg = MSG::default();
    while std::time::Instant::now() < deadline {
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn show_toast() -> windows::core::Result<()> {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };

    // Without this the platform cannot tell which AUMID the process speaks for.
    unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(AUMID))? };
    log(&format!("SetCurrentProcessExplicitAppUserModelID({AUMID}) ok"));

    // Register the activator class object so a click on a *live* toast is delivered to
    // THIS running process rather than launching a second copy of the exe.
    let factory: IClassFactory = ActivatorFactory.into();
    let cookie = unsafe {
        CoRegisterClassObject(&ACTIVATOR_CLSID, &factory, CLSCTX_LOCAL_SERVER, REGCLS_MULTIPLEUSE)
    };
    match &cookie {
        Ok(c) => log(&format!("CoRegisterClassObject ok cookie={c}")),
        Err(e) => log(&format!("CoRegisterClassObject FAILED {e:?}")),
    }

    // `launch` is what comes back as `invokedArgs` -- this is where Folio would put the
    // window id + tab id so the click can be routed to the exact tab.
    let xml_text = r#"<toast launch="action=focusTab&amp;window=3&amp;tab=7" activationType="foreground">
  <visual>
    <binding template="ToastGeneric">
      <text>cargo build finished</text>
      <text>bt-app in 41.2s - exit 0</text>
    </binding>
  </visual>
  <actions>
    <action content="Show tab" arguments="action=focusTab&amp;window=3&amp;tab=7" activationType="foreground"/>
    <action content="Dismiss" arguments="action=dismiss" activationType="foreground"/>
  </actions>
</toast>"#;

    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml_text))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;

    toast.Activated(&TypedEventHandler::<ToastNotification, windows::core::IInspectable>::new(
        |_sender, args| {
            let detail = args
                .as_ref()
                .and_then(|a| a.cast::<ToastActivatedEventArgs>().ok())
                .and_then(|a| a.Arguments().ok())
                .map(|s| s.to_string_lossy())
                .unwrap_or_else(|| "<no args>".into());
            log(&format!("*** IN-PROCESS Activated *** arguments={detail:?}"));
            Ok(())
        },
    ))?;
    toast.Dismissed(&TypedEventHandler::<ToastNotification, ToastDismissedEventArgs>::new(
        |_s, args| {
            let reason = args.as_ref().and_then(|a| a.Reason().ok());
            let name = match reason {
                Some(ToastDismissalReason::UserCanceled) => "UserCanceled",
                Some(ToastDismissalReason::ApplicationHidden) => "ApplicationHidden",
                Some(ToastDismissalReason::TimedOut) => "TimedOut",
                _ => "?",
            };
            log(&format!("Dismissed reason={name}"));
            Ok(())
        },
    ))?;
    toast.Failed(&TypedEventHandler::<ToastNotification, ToastFailedEventArgs>::new(|_s, args| {
        let err = args.as_ref().and_then(|a| a.ErrorCode().ok());
        log(&format!("*** Failed *** errorCode={err:?}"));
        Ok(())
    }))?;

    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?;
    log(&format!("notifier setting={:?}", notifier.Setting()));
    match notifier.Show(&toast) {
        Ok(()) => log("Show() ok -- toast requested"),
        Err(e) => log(&format!("Show() FAILED {e:?}")),
    }

    let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(45);
    log(&format!("pumping messages for {secs}s -- click the toast now"));
    pump(secs);
    log("pump finished");

    if let Ok(c) = cookie {
        unsafe { CoRevokeClassObject(c)? };
    }
    Ok(())
}

/// The cold path: Windows launched us with `-Embedding` because the toast was clicked and no
/// class object was registered. We must register the class object and wait to be called.
fn embedding() -> windows::core::Result<()> {
    log("launched with -Embedding (COM cold activation)");
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
    let factory: IClassFactory = ActivatorFactory.into();
    let cookie = unsafe {
        CoRegisterClassObject(&ACTIVATOR_CLSID, &factory, CLSCTX_LOCAL_SERVER, REGCLS_MULTIPLEUSE)?
    };
    log(&format!("-Embedding class object registered cookie={cookie}"));
    pump(20);
    unsafe { CoRevokeClassObject(cookie)? };
    log("-Embedding exiting");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let verb = args.get(1).map(String::as_str).unwrap_or("show");
    log(&format!("=== toast-probe start argv={args:?} ==="));
    let result = match verb {
        "register" => do_register(),
        "unregister" => {
            do_unregister();
            Ok(())
        }
        "dumpkeys" => {
            do_dumpkeys();
            Ok(())
        }
        "-Embedding" | "/Embedding" | "-embedding" => embedding(),
        _ => show_toast(),
    };
    if let Err(e) = result {
        log(&format!("ERROR: {e:?}"));
        std::process::exit(1);
    }
}
