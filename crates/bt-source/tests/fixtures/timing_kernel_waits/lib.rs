// The timing scan's kernel-wait fixture (plan row 163): each shape the widened
// shape 1 reads as a real wait, each spelling it refuses, and the calls it must
// leave alone. Read by bt-source's `timing` test; never compiled.

use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};

const MS: u32 = 250;

fn handle() -> HANDLE {
    HANDLE::default()
}

// F1: imported, called bare, with a literal bound.
#[test]
fn an_imported_bare_object_wait() {
    let _ = unsafe { WaitForSingleObject(handle(), 5_000) };
}

// F2: called by its whole path, with a constant bound.
#[test]
fn a_whole_path_wait_on_a_constant() {
    let handles = [handle()];
    let _ = unsafe {
        windows::Win32::System::Threading::WaitForMultipleObjects(&handles, false, MS)
    };
}

// F3: the bound argument is not read — an unbounded wait and a look both count.
#[test]
fn an_unbounded_wait() {
    let _ = unsafe { WaitForSingleObject(handle(), INFINITE) };
}

#[test]
fn a_look_at_once() {
    let _ = unsafe { WaitForSingleObject(handle(), 0) };
}

// F4: a socket's timeouts, a method and an associated function.
#[test]
fn a_socket_read_timeout() {
    let stream = TcpStream::connect("127.0.0.1:1").unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(u64::from(MS))))
        .unwrap();
}

#[test]
fn a_connect_with_a_timeout() {
    let address: SocketAddr = "127.0.0.1:1".parse().unwrap();
    let _ = TcpStream::connect_timeout(&address, Duration::from_secs(1));
}

// F5: a libc wait by its path, inside a closure of the body.
#[test]
fn a_libc_poll_in_a_closure() {
    let mut waiting: [libc::pollfd; 0] = [];
    let wait = || unsafe { libc::poll(waiting.as_mut_ptr(), 0, 250) };
    let _ = wait();
}

// F6: first-party functions and methods named like a libc wait are not waits.
mod control {
    struct Watch;

    impl Watch {
        fn poll(&mut self, _now: u64) {}
    }

    fn poll(_now: u64) {}

    #[test]
    fn first_party_polls_are_not_waits() {
        poll(1);
        Watch.poll(2);
    }
}

// F7: the spellings that would hide a wait from the scan, each refused.
mod imported {
    use libc::poll;
    use windows::Win32::System::Threading::WaitForMultipleObjectsEx as wait_many;
}

// F8: a wait only in a helper the test calls is outside shape 1 (its own body).
#[cfg(test)]
fn a_helper_that_waits() {
    let _ = unsafe { WaitForSingleObject(handle(), 10_000) };
}

#[test]
fn a_wait_only_in_a_helper() {
    a_helper_that_waits();
}

// F9: a first-party function declared by a kernel wait's name.
#[allow(non_snake_case)]
fn WaitNamedPipeW(_name: &str, _milliseconds: u32) -> bool {
    true
}

// F10: a libc wait by a rooted path and by a longer path ending so — found.
#[test]
fn a_rooted_libc_path() {
    let _ = unsafe { ::libc::poll(std::ptr::null_mut(), 0, 250) };
}

#[test]
fn a_longer_libc_path() {
    let _ = unsafe { crate::ffi::libc::poll(std::ptr::null_mut(), 0, 250) };
}

// F11: the other kernel waits of the closed list — found.
#[test]
fn the_other_kernel_waits() {
    let _ = unsafe { WaitForInputIdle(handle(), 250) };
    let _ = unsafe { WaitForDebugEvent(std::ptr::null_mut(), 250) };
    let _ = unsafe { WSAPoll(&mut [], 250) };
    let _ = unsafe { windows::Win32::Networking::WinSock::select(0, None, None, None, None) };
    CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, 0.25, false);
    let _ = unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.25, 0) };
}

// F12: `libc::kevent` is a struct as well as a wait; built, it is neither found
// nor refused.
#[test]
fn a_kevent_built_not_waited() {
    let _events: [libc::kevent; 0] = [];
}

// F13: the other spellings that would hide a wait, each refused.
#[test]
fn a_libc_wait_as_a_value() {
    let _wait = libc::poll;
}

#[test]
fn a_kernel_wait_as_a_value() {
    let _wait = WaitForSingleObject;
}

mod grouped {
    use libc::{c_int, poll};
}

mod renamed_crate {
    use libc as c;
}

mod kqueue {
    use libc::kevent;
}

mod winsock {
    use windows::Win32::Networking::WinSock::select;
}

// F14: a glob of libc on the file's last line, with no newline after it.
use libc::*;