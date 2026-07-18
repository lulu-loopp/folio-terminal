use super::WinChild;
use crate::cmdbuilder::CommandBuilder;
use crate::win::procthreadattr::ProcThreadAttributeList;
use anyhow::{bail, ensure, Error};
use filedescriptor::{FileDescriptor, OwnedHandle};
use lazy_static::lazy_static;
use shared_library::shared_library;
use std::ffi::OsString;
use std::io::Error as IoError;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::{mem, ptr};
use winapi::shared::minwindef::DWORD;
use winapi::shared::winerror::{HRESULT, S_OK};
use winapi::um::handleapi::*;
use winapi::um::processthreadsapi::*;
use winapi::um::winbase::{
    CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};
use winapi::um::wincon::COORD;
use winapi::um::winnt::HANDLE;

pub type HPCON = HANDLE;

pub const PSUEDOCONSOLE_INHERIT_CURSOR: DWORD = 0x1;
pub const PSEUDOCONSOLE_RESIZE_QUIRK: DWORD = 0x2;
pub const PSEUDOCONSOLE_WIN32_INPUT_MODE: DWORD = 0x4;
#[allow(dead_code)]
pub const PSEUDOCONSOLE_PASSTHROUGH_MODE: DWORD = 0x8;
pub const CONPTY_SIDECAR_VERSION: &str = "1.25.260710002-preview";

shared_library!(SystemConPtyFuncs,
    pub fn CreatePseudoConsole(
        size: COORD,
        hInput: HANDLE,
        hOutput: HANDLE,
        flags: DWORD,
        hpc: *mut HPCON
    ) -> HRESULT,
    pub fn ResizePseudoConsole(hpc: HPCON, size: COORD) -> HRESULT,
    pub fn ClosePseudoConsole(hpc: HPCON),
);

// The Microsoft.Windows.Console.ConPTY NuGet import library and conpty.h expose a deliberately
// prefixed ABI. The DLL also contains the system spellings, but those are compatibility exports;
// selecting them would report a sidecar while continuing through inbox behavior.
shared_library!(SidecarConPtyFuncs,
    pub fn ConptyCreatePseudoConsole(
        size: COORD,
        hInput: HANDLE,
        hOutput: HANDLE,
        flags: DWORD,
        hpc: *mut HPCON
    ) -> HRESULT,
    pub fn ConptyResizePseudoConsole(hpc: HPCON, size: COORD) -> HRESULT,
    pub fn ConptyReleasePseudoConsole(hpc: HPCON) -> HRESULT,
    pub fn ConptyClosePseudoConsole(hpc: HPCON),
);

/// The implementation selected for the process-wide native pseudoconsole API.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConPtySource {
    /// The Windows Terminal host loaded from the application's directory.
    Sidecar { dll: PathBuf },
    /// The ConPTY implementation exported by the operating system's kernel32.
    System,
}

impl std::fmt::Display for ConPtySource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sidecar { dll } => write!(
                formatter,
                "source=sidecar version={CONPTY_SIDECAR_VERSION} dll={}",
                dll.display()
            ),
            Self::System => formatter.write_str("source=system version=windows-inbox"),
        }
    }
}

struct LoadedConPty {
    funcs: ConPtyFuncs,
    source: ConPtySource,
}

enum ConPtyFuncs {
    Sidecar(SidecarConPtyFuncs),
    System(SystemConPtyFuncs),
}

impl ConPtyFuncs {
    fn creation_flags(&self) -> DWORD {
        match self {
            // The packaged ABI documents the standard CreatePseudoConsole contract. Microsoft's
            // node-pty integration likewise passes zero unless cursor inheritance was explicitly
            // requested; portable-pty's private 0x2/0x4 flags must not cross this ABI boundary.
            Self::Sidecar(_) => 0,
            Self::System(_) => {
                PSUEDOCONSOLE_INHERIT_CURSOR
                    | PSEUDOCONSOLE_RESIZE_QUIRK
                    | PSEUDOCONSOLE_WIN32_INPUT_MODE
            }
        }
    }

    fn create(
        &self,
        size: COORD,
        input: HANDLE,
        output: HANDLE,
        flags: DWORD,
        con: *mut HPCON,
    ) -> HRESULT {
        unsafe {
            match self {
                Self::Sidecar(funcs) => {
                    (funcs.ConptyCreatePseudoConsole)(size, input, output, flags, con)
                }
                Self::System(funcs) => {
                    (funcs.CreatePseudoConsole)(size, input, output, flags, con)
                }
            }
        }
    }

    fn resize(&self, con: HPCON, size: COORD) -> HRESULT {
        unsafe {
            match self {
                Self::Sidecar(funcs) => (funcs.ConptyResizePseudoConsole)(con, size),
                Self::System(funcs) => (funcs.ResizePseudoConsole)(con, size),
            }
        }
    }

    fn release(&self, con: HPCON) -> HRESULT {
        unsafe {
            match self {
                Self::Sidecar(funcs) => (funcs.ConptyReleasePseudoConsole)(con),
                Self::System(_) => S_OK,
            }
        }
    }

    fn close(&self, con: HPCON) {
        unsafe {
            match self {
                Self::Sidecar(funcs) => (funcs.ConptyClosePseudoConsole)(con),
                Self::System(funcs) => (funcs.ClosePseudoConsole)(con),
            }
        }
    }
}

fn load_conpty() -> LoadedConPty {
    // If the kernel doesn't export these functions then their system is
    // too old and we cannot run.
    let kernel = SystemConPtyFuncs::open(Path::new("kernel32.dll")).expect(
        "this system does not support conpty.  Windows 10 October 2018 or newer is required",
    );

    // BetterTerminal's patch deliberately uses an absolute application-directory path. The
    // upstream bare `conpty.dll` name also searches PATH, which can silently select an unrelated
    // host when the packaged sidecar is absent. Requiring the paired OpenConsole executable keeps
    // the supported choices strict: the packaged pair or the operating-system implementation.
    if std::env::var_os("BT_CONPTY_FORCE_SYSTEM").is_none() {
        if let Ok(application) = std::env::current_exe() {
            if let Some(directory) = application.parent() {
                let dll = directory.join("conpty.dll");
                let host = directory.join("OpenConsole.exe");
                if dll.is_file() && host.is_file() {
                    if let Ok(funcs) = SidecarConPtyFuncs::open(&dll) {
                        return LoadedConPty {
                            funcs: ConPtyFuncs::Sidecar(funcs),
                            source: ConPtySource::Sidecar { dll },
                        };
                    }
                }
            }
        }
    }

    LoadedConPty {
        funcs: ConPtyFuncs::System(kernel),
        source: ConPtySource::System,
    }
}

lazy_static! {
    static ref CONPTY: LoadedConPty = load_conpty();
}

/// Report the implementation selected by the same process-wide loader used by `openpty`.
pub fn conpty_source() -> ConPtySource {
    CONPTY.source.clone()
}

pub struct PsuedoCon {
    con: HPCON,
}

unsafe impl Send for PsuedoCon {}
unsafe impl Sync for PsuedoCon {}

impl Drop for PsuedoCon {
    fn drop(&mut self) {
        CONPTY.funcs.close(self.con);
    }
}

impl PsuedoCon {
    pub fn new(size: COORD, input: FileDescriptor, output: FileDescriptor) -> Result<Self, Error> {
        let mut con: HPCON = INVALID_HANDLE_VALUE;
        let result = CONPTY.funcs.create(
            size,
            input.as_raw_handle() as _,
            output.as_raw_handle() as _,
            CONPTY.funcs.creation_flags(),
            &mut con,
        );
        ensure!(
            result == S_OK,
            "failed to create psuedo console: HRESULT {}",
            result
        );
        Ok(Self { con })
    }

    pub fn resize(&self, size: COORD) -> Result<(), Error> {
        let result = CONPTY.funcs.resize(self.con, size);
        ensure!(
            result == S_OK,
            "failed to resize console to {}x{}: HRESULT: {}",
            size.X,
            size.Y,
            result
        );
        Ok(())
    }

    pub fn spawn_command(&self, cmd: CommandBuilder) -> anyhow::Result<WinChild> {
        let mut si: STARTUPINFOEXW = unsafe { mem::zeroed() };
        si.StartupInfo.cb = mem::size_of::<STARTUPINFOEXW>() as u32;
        // Explicitly set the stdio handles as invalid handles otherwise
        // we can end up with a weird state where the spawned process can
        // inherit the explicitly redirected output handles from its parent.
        // For example, when daemonizing wezterm-mux-server, the stdio handles
        // are redirected to a log file and the spawned process would end up
        // writing its output there instead of to the pty we just created.
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
        si.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
        si.StartupInfo.hStdError = INVALID_HANDLE_VALUE;

        let mut attrs = ProcThreadAttributeList::with_capacity(1)?;
        attrs.set_pty(self.con)?;
        si.lpAttributeList = attrs.as_mut_ptr();

        let mut pi: PROCESS_INFORMATION = unsafe { mem::zeroed() };

        let (mut exe, mut cmdline) = cmd.cmdline()?;
        let cmd_os = OsString::from_wide(&cmdline);

        let cwd = cmd.current_directory();

        let res = unsafe {
            CreateProcessW(
                exe.as_mut_slice().as_mut_ptr(),
                cmdline.as_mut_slice().as_mut_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                cmd.environment_block().as_mut_slice().as_mut_ptr() as *mut _,
                cwd.as_ref()
                    .map(|c| c.as_slice().as_ptr())
                    .unwrap_or(ptr::null()),
                &mut si.StartupInfo,
                &mut pi,
            )
        };
        if res == 0 {
            let err = IoError::last_os_error();
            let msg = format!(
                "CreateProcessW `{:?}` in cwd `{:?}` failed: {}",
                cmd_os,
                cwd.as_ref().map(|c| OsString::from_wide(c)),
                err
            );
            log::error!("{}", msg);
            bail!("{}", msg);
        }

        let release = CONPTY.funcs.release(self.con);
        ensure!(
            release == S_OK,
            "failed to release sidecar pseudoconsole after process attachment: HRESULT {}",
            release
        );

        // Make sure we close out the thread handle so we don't leak it;
        // we do this simply by making it owned
        let _main_thread = unsafe { OwnedHandle::from_raw_handle(pi.hThread as _) };
        let proc = unsafe { OwnedHandle::from_raw_handle(pi.hProcess as _) };

        Ok(WinChild {
            proc: Mutex::new(proc),
        })
    }
}
