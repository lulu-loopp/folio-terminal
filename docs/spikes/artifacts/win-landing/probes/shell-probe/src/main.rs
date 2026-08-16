//! Folio Windows-landing probes #4 and #5: Explorer context-menu verbs, and elevation.
//!
//! Verbs:
//!   shell-probe register-menu     -- write the three classic-verb key trees under HKCU
//!   shell-probe unregister-menu   -- delete them
//!   shell-probe dump-menu         -- print what is currently registered
//!   shell-probe argv ...          -- append argv + cwd + elevation state to shell-probe.log.
//!                                    This is what the registered `command` runs, so the log
//!                                    proves exactly what %V expanded to at each menu level.
//!   shell-probe runas             -- ShellExecuteExW("runas") on ourselves -> UAC prompt;
//!                                    reports what the API returned (including the cancel path)
//!   shell-probe amiadmin          -- print whether this token is elevated

use std::io::Write as _;

use windows::Win32::Foundation::{ERROR_CANCELLED, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteTreeW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::{
    SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

/// The verb key name. This is the subkey Explorer reads, and also what shows in the menu when
/// no `MUIVerb`/default value is set.
const VERB: &str = "Folio";

fn log(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
    if let Ok(exe) = std::env::current_exe() {
        let path = exe.with_file_name("shell-probe.log");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "[pid {}] {line}", std::process::id());
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_string(subkey: &str, name: &str, value: &str) -> windows::core::Result<()> {
    let sub = wide(subkey);
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
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

fn read_string(subkey: &str, name: &str) -> Option<String> {
    unsafe {
        let sub = wide(subkey);
        let mut key = HKEY::default();
        RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), None, KEY_READ, &mut key)
            .ok()
            .ok()?;
        let name_w = wide(name);
        let name_ptr = if name.is_empty() { PCWSTR::null() } else { PCWSTR(name_w.as_ptr()) };
        let mut size: u32 = 0;
        RegQueryValueExW(key, name_ptr, None, None, None, Some(&mut size)).ok().ok()?;
        let mut buf = vec![0u8; size as usize];
        RegQueryValueExW(key, name_ptr, None, None, Some(buf.as_mut_ptr()), Some(&mut size))
            .ok()
            .ok()?;
        let _ = RegCloseKey(key);
        let u: Vec<u16> = buf
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&c| c != 0)
            .collect();
        Some(String::from_utf16_lossy(&u))
    }
}

/// The three places a "open a terminal here" verb has to be registered, and why each exists:
///   Directory\Background  -- right-click empty space *inside* a folder; the folder is %V
///   Directory             -- right-click the folder icon itself; the clicked folder is %V
///   Drive                 -- right-click a drive root (C:\), which is not a `Directory`
fn menu_roots() -> [String; 3] {
    [
        format!("Software\\Classes\\Directory\\Background\\shell\\{VERB}"),
        format!("Software\\Classes\\Directory\\shell\\{VERB}"),
        format!("Software\\Classes\\Drive\\shell\\{VERB}"),
    ]
}

fn do_register_menu() -> windows::core::Result<()> {
    let exe = std::env::current_exe().expect("current_exe");
    let exe = exe.to_string_lossy().to_string();
    for root in menu_roots() {
        // Default value = the menu label. `MUIVerb` would be used instead for a localised
        // string resource; a plain default value is the simplest honest label.
        set_string(&root, "", "Open in Folio here")?;
        // `Icon` accepts "path,index" or a bare exe (index 0 = the app icon).
        set_string(&root, "Icon", &format!("{exe},0"))?;
        // Present the verb in the *extended* menu only? No -- omitting `Extended` keeps it in
        // the normal menu. Setting `NoWorkingDirectory` stops Explorer from cd-ing for us.
        set_string(
            &format!("{root}\\command"),
            "",
            &format!("\"{exe}\" argv --cwd \"%V\""),
        )?;
    }
    log(&format!("registered 3 verb trees for exe={exe}"));
    Ok(())
}

fn do_unregister_menu() {
    for root in menu_roots() {
        let w = wide(&root);
        let r = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(w.as_ptr())) };
        log(&format!("deleted HKCU\\{root} -> {r:?}"));
    }
}

fn do_dump_menu() {
    for root in menu_roots() {
        log(&format!("HKCU\\{root} [] = {:?}", read_string(&root, "")));
        log(&format!("HKCU\\{root} [Icon] = {:?}", read_string(&root, "Icon")));
        log(&format!(
            "HKCU\\{root}\\command [] = {:?}",
            read_string(&format!("{root}\\command"), "")
        ));
    }
}

fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some((&raw mut elevation).cast()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret,
        )
        .is_ok();
        ok && elevation.TokenIsElevated != 0
    }
}

/// The whole-window elevation route: relaunch ourselves under the `runas` verb. There is no
/// way to make one *tab* elevated inside a non-elevated process -- a process has exactly one
/// token, granted at CreateProcess time, and UAC only ever elevates a whole new process.
fn do_runas() -> windows::core::Result<()> {
    let exe = std::env::current_exe().expect("current_exe");
    let exe_w = wide(&exe.to_string_lossy());
    let verb = wide("runas");
    let params = wide("argv --elevated-relaunch");
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(exe_w.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    log("calling ShellExecuteExW(runas) -- a UAC prompt should appear now");
    let result = unsafe { ShellExecuteExW(&mut info) };
    match result {
        Ok(()) => {
            log(&format!("ShellExecuteExW ok; child process handle={:?}", info.hProcess));
        }
        Err(e) => {
            // Declining the UAC prompt is not an exceptional condition -- it is the user
            // saying no, and it arrives as ERROR_CANCELLED (1223) wrapped in an HRESULT.
            let cancelled = e.code() == ERROR_CANCELLED.to_hresult();
            log(&format!(
                "ShellExecuteExW failed: {e:?} (user declined UAC = {cancelled})"
            ));
        }
    }
    Ok(())
}

/// Invoke our own registered verb on a directory through the shell, the same way Explorer
/// does when the menu item is clicked. This exercises the real `Directory\shell\Folio\command`
/// value and the shell's own `%V` substitution, with none of the coordinate-chasing that
/// driving the Win11 context menu by mouse required.
fn do_invoke_verb(path: &str) -> windows::core::Result<()> {
    let verb = wide(VERB);
    let file = wide(path);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    log(&format!("ShellExecuteExW verb={VERB:?} file={path:?}"));
    let r = unsafe { ShellExecuteExW(&mut info) };
    log(&format!("  -> {r:?}"));
    Ok(())
}

fn main() -> windows::core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str).unwrap_or("argv") {
        "register-menu" => do_register_menu()?,
        "unregister-menu" => do_unregister_menu(),
        "dump-menu" => do_dump_menu(),
        "runas" => do_runas()?,
        "invoke-verb" => do_invoke_verb(args.get(2).map(String::as_str).unwrap_or("C:\\"))?,
        "amiadmin" => log(&format!("elevated={}", is_elevated())),
        _ => {
            let cwd = std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|e| format!("<err {e}>"));
            log(&format!(
                "ARGV argv={args:?} cwd={cwd:?} elevated={} raw_cmdline={:?}",
                is_elevated(),
                std::env::args().collect::<Vec<_>>().join(" ")
            ));
        }
    }
    Ok(())
}
