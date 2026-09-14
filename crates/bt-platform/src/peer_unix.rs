//! **Who is at the other end of a connected Unix socket, asked of the kernel**
//! — the one question both of this crate's Unix doors ask, in the one place the
//! two platforms spell it differently.
//!
//! [`crate::attention_pipe`] and [`crate::launch_pipe`] each bind a socket in
//! the same runtime directory and each refuses a peer whose uid is not this
//! process's own; the launch door asks one thing more, the peer's **process
//! id**, because it goes on to look that process's executable up. Neither of
//! them reads a credential out of a frame: a frame is something the peer writes,
//! and a number the peer chose is not an answer to "who are you".
//!
//! # Two doors rather than one, and the reason is a decision rather than a call
//!
//! [`uid`] is the peer's user and nothing else, and [`credentials`] is that
//! plus the process id. The doorbell takes the first **on purpose** — the
//! programs on the other end of it are other people's (`claude`, `codex`,
//! `node`, a shell), it never looks an executable up, and a door that asked the
//! kernel for a number it has no use for would be a door whose own header had
//! stopped describing it (`docs/DESIGN.md` §13.28 ⑤). The launch wire takes the
//! second, because the only thing that ever speaks it is a second Folio and the
//! image at the other end is exactly what it is checking (§13.28 ⑥).
//!
//! # The two arms, and why one call is not enough for both platforms
//!
//! * **Darwin** answers the user with `getpeereid` — two out-parameters, the uid
//!   and the gid — and the process id with a `getsockopt` of `LOCAL_PEERPID` at
//!   the `SOL_LOCAL` level, which is `<sys/un.h>`'s own name for the pid the
//!   kernel recorded when the peer connected. Two calls, because the first one
//!   does not answer the second question.
//! * **Linux** has no `getpeereid` at all: the kernel hands all three over at
//!   once through `getsockopt(SOL_SOCKET, SO_PEERCRED)`, which fills in a
//!   `libc::ucred` — pid, uid and gid, recorded at `connect` in the same way. So
//!   the user is read out of the answer to the wider question, and the narrow
//!   door is the wide one with two fields dropped.
//!
//! Both are the kernel's record of the peer *at the moment it connected*, which
//! is the property the two doors rest on: a process that later execs something
//! else does not change what is written on a connection that is already open.
//!
//! **A pid is `Option` rather than a number, because a kernel may have no
//! process to name.** `SO_PEERCRED` answers `0` for a peer the reader cannot see
//! — one in another pid namespace — and a caller that treated that as a process
//! id would look up the executable of whatever `0` happens to mean. The launch
//! door refuses a `None` in the same sentence it refuses a peer of another user.

use std::{
    io,
    os::unix::{io::AsRawFd, net::UnixStream},
};

/// The peer of a connected Unix socket, as the kernel recorded it at `connect`.
pub(crate) struct Credentials {
    /// The user the process at the other end is running as.
    pub(crate) uid: u32,
    /// The process at the other end, where this platform names one — and `None`
    /// where the kernel answered with a number that is not a process.
    pub(crate) pid: Option<u32>,
}

/// **The peer's user, and nothing else.**
///
/// # Errors
///
/// The kernel's own, for a descriptor that is not a connected socket.
#[cfg(target_os = "macos")]
pub(crate) fn uid(stream: &UnixStream) -> io::Result<u32> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: the descriptor is the borrowed stream's and both outputs are live
    // locals of this frame.
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &raw mut uid, &raw mut gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(uid)
}

/// The same question where the one call that answers it answers all three at
/// once — so the narrow door is the wide one with two fields dropped.
///
/// # Errors
///
/// The kernel's own, for a descriptor that is not a connected socket.
#[cfg(not(target_os = "macos"))]
pub(crate) fn uid(stream: &UnixStream) -> io::Result<u32> {
    Ok(credentials(stream)?.uid)
}

/// **The peer's user and its process id**, both from the kernel.
///
/// # Errors
///
/// The kernel's own, for a descriptor that is not a connected socket.
#[cfg(target_os = "macos")]
pub(crate) fn credentials(stream: &UnixStream) -> io::Result<Credentials> {
    /// `<sys/un.h>`: the option level for an `AF_UNIX` socket's own options.
    const SOL_LOCAL: libc::c_int = 0;
    /// `<sys/un.h>`: the pid of the peer, as the kernel recorded it at connect.
    const LOCAL_PEERPID: libc::c_int = 2;

    let uid = self::uid(stream)?;
    let mut pid: libc::pid_t = 0;
    let mut length = libc::socklen_t::try_from(std::mem::size_of::<libc::pid_t>()).unwrap_or(0);
    // SAFETY: `pid` is a live local and `length` describes it exactly.
    let read = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            SOL_LOCAL,
            LOCAL_PEERPID,
            (&raw mut pid).cast(),
            &raw mut length,
        )
    };
    if read != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Credentials {
        uid,
        pid: process_id(pid),
    })
}

/// The same two numbers where the kernel hands over all three at once.
///
/// # Errors
///
/// The kernel's own, for a descriptor that is not a connected socket.
#[cfg(not(target_os = "macos"))]
pub(crate) fn credentials(stream: &UnixStream) -> io::Result<Credentials> {
    let mut peer = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = libc::socklen_t::try_from(std::mem::size_of::<libc::ucred>()).unwrap_or(0);
    // SAFETY: `peer` is a live local and `length` describes it exactly.
    let read = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut peer).cast(),
            &raw mut length,
        )
    };
    if read != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Credentials {
        uid: peer.uid,
        pid: process_id(peer.pid),
    })
}

/// **A process the kernel named**, or `None` for a number that is not one.
///
/// Zero is the answer `SO_PEERCRED` gives for a peer in a pid namespace this
/// process cannot see into, and a negative one is not a pid at all; both are the
/// same thing to a caller — there is no process here to look anything up about.
fn process_id(pid: libc::pid_t) -> Option<u32> {
    u32::try_from(pid).ok().filter(|pid| *pid != 0)
}
