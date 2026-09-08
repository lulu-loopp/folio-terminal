use crate::{Child, ChildKiller, ExitStatus};
use anyhow::Context as _;
use std::io::{Error as IoError, Result as IoResult};
use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};
use std::{mem, ptr};
use winapi::shared::minwindef::DWORD;
use winapi::um::jobapi2::{
    AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
};
use winapi::um::minwinbase::STILL_ACTIVE;
use winapi::um::processthreadsapi::*;
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::winbase::INFINITE;
use winapi::um::winnt::{
    JobObjectExtendedLimitInformation, HANDLE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

pub mod conpty;
mod procthreadattr;
mod psuedocon;

pub use psuedocon::{CONPTY_SIDECAR_VERSION, ConPtySource, conpty_source};

use filedescriptor::OwnedHandle;

/// **A job object holding one pane's child and everything that child starts**
/// (BetterTerminal, review row R2-6).
///
/// A pseudoconsole ends a *session*: closing it tells the console host to go,
/// and the host's client — the shell — is killed by the terminal beside it. What
/// neither of those reaches is what the shell itself started. A build left
/// running, a server, a watcher: each is a grandchild of this process with no
/// console of its own, and before this it went on running with nothing left to
/// show it, until the machine was restarted or somebody found it in a task list.
///
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is the documented way to say that: the
/// job's processes are terminated when the last handle to it closes, and the
/// only handle is the one this holds. So the job's life is the pane's, and
/// closing the pane ends what the pane started.
///
/// **It does not fight the console host.** `OpenConsole.exe` is created by the
/// pseudoconsole implementation and not by this call, so it is never in this
/// job; what is in it is the client this function launched and its descendants.
/// A job cannot be joined twice in the same nesting chain, so a machine where
/// this process is itself inside a job (a packaged app, a debugger, a CI runner)
/// nests one more level, which Windows 8 and later allow.
///
/// **A job that cannot be made or joined is not an error.** The pane is exactly
/// as good as it was before this existed — the child runs, the terminal works —
/// so the failure is logged and the field is `None`.
#[derive(Debug)]
pub struct Job {
    /// Held, never read: the job's whole effect is what closing this handle
    /// does to the processes inside it.
    _handle: Option<OwnedHandle>,
}

impl Job {
    /// Make a job that kills what it holds when it closes, and put `process` in
    /// it. `process` must be a handle with `PROCESS_SET_QUOTA` and
    /// `PROCESS_TERMINATE` — which is what `CreateProcessW` hands back.
    pub fn holding(process: HANDLE) -> Self {
        // SAFETY: an unnamed job object with default security, whose handle is
        // owned below and closed exactly once.
        let handle = unsafe { CreateJobObjectW(ptr::null_mut(), ptr::null()) };
        if handle.is_null() {
            log::warn!(
                "could not create a job object for the child: {}",
                IoError::last_os_error()
            );
            return Self { _handle: None };
        }
        // SAFETY: the handle above, owned from here on.
        let handle = unsafe { OwnedHandle::from_raw_handle(handle as _) };

        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the documented information class for this structure, whose
        // size is passed as the same structure's own.
        let set = unsafe {
            SetInformationJobObject(
                handle.as_raw_handle() as _,
                JobObjectExtendedLimitInformation,
                &mut limits as *mut _ as *mut _,
                mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as DWORD,
            )
        };
        if set == 0 {
            log::warn!(
                "could not set the job object's limits: {}",
                IoError::last_os_error()
            );
            return Self { _handle: None };
        }

        // SAFETY: both handles are this function's own.
        let assigned =
            unsafe { AssignProcessToJobObject(handle.as_raw_handle() as _, process as _) };
        if assigned == 0 {
            log::warn!(
                "could not put the child in its job object: {}",
                IoError::last_os_error()
            );
            return Self { _handle: None };
        }

        Self {
            _handle: Some(handle),
        }
    }
}

#[derive(Debug)]
pub struct WinChild {
    proc: Mutex<OwnedHandle>,
    /// Closed when this child object is dropped, which kills whatever the child
    /// started and has not ended. See [`Job`].
    pub(crate) _job: Job,
}

impl WinChild {
    fn is_complete(&mut self) -> IoResult<Option<ExitStatus>> {
        let mut status: DWORD = 0;
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
        let res = unsafe { GetExitCodeProcess(proc.as_raw_handle() as _, &mut status) };
        if res != 0 {
            if status == STILL_ACTIVE {
                Ok(None)
            } else {
                Ok(Some(ExitStatus::with_exit_code(status)))
            }
        } else {
            Ok(None)
        }
    }

    fn do_kill(&mut self) -> IoResult<()> {
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
        let res = unsafe { TerminateProcess(proc.as_raw_handle() as _, 1) };
        let err = IoError::last_os_error();
        if res != 0 {
            Err(err)
        } else {
            Ok(())
        }
    }
}

impl ChildKiller for WinChild {
    fn kill(&mut self) -> IoResult<()> {
        self.do_kill().ok();
        Ok(())
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
        Box::new(WinChildKiller { proc })
    }
}

#[derive(Debug)]
pub struct WinChildKiller {
    proc: OwnedHandle,
}

impl ChildKiller for WinChildKiller {
    fn kill(&mut self) -> IoResult<()> {
        let res = unsafe { TerminateProcess(self.proc.as_raw_handle() as _, 1) };
        let err = IoError::last_os_error();
        if res != 0 {
            Err(err)
        } else {
            Ok(())
        }
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        let proc = self.proc.try_clone().unwrap();
        Box::new(WinChildKiller { proc })
    }
}

impl Child for WinChild {
    fn try_wait(&mut self) -> IoResult<Option<ExitStatus>> {
        self.is_complete()
    }

    fn wait(&mut self) -> IoResult<ExitStatus> {
        if let Ok(Some(status)) = self.try_wait() {
            return Ok(status);
        }
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
        unsafe {
            WaitForSingleObject(proc.as_raw_handle() as _, INFINITE);
        }
        let mut status: DWORD = 0;
        let res = unsafe { GetExitCodeProcess(proc.as_raw_handle() as _, &mut status) };
        if res != 0 {
            Ok(ExitStatus::with_exit_code(status))
        } else {
            Err(IoError::last_os_error())
        }
    }

    fn process_id(&self) -> Option<u32> {
        let res = unsafe { GetProcessId(self.proc.lock().unwrap().as_raw_handle() as _) };
        if res == 0 {
            None
        } else {
            Some(res)
        }
    }

    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        let proc = self.proc.lock().unwrap();
        Some(proc.as_raw_handle())
    }
}

impl std::future::Future for WinChild {
    type Output = anyhow::Result<ExitStatus>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<anyhow::Result<ExitStatus>> {
        match self.is_complete() {
            Ok(Some(status)) => Poll::Ready(Ok(status)),
            Err(err) => Poll::Ready(Err(err).context("Failed to retrieve process exit status")),
            Ok(None) => {
                struct PassRawHandleToWaiterThread(pub RawHandle);
                unsafe impl Send for PassRawHandleToWaiterThread {}

                let proc = self.proc.lock().unwrap().try_clone()?;
                let handle = PassRawHandleToWaiterThread(proc.as_raw_handle());

                let waker = cx.waker().clone();
                std::thread::spawn(move || {
                    unsafe {
                        WaitForSingleObject(handle.0 as _, INFINITE);
                    }
                    waker.wake();
                });
                Poll::Pending
            }
        }
    }
}
