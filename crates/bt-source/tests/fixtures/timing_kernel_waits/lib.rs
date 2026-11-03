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
