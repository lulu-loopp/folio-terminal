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
//!   and the gid, read from the credentials the kernel recorded when the
//!   connection was made — and the process id with a `getsockopt` of
//!   `LOCAL_PEERPID` at the `SOL_LOCAL` level. Two calls, because the first one
//!   does not answer the second question, and **the two are not kept alike**:
//!   the user is the record and is still answered after the peer has closed its
//!   end; the process id is answered from the live connection, and once the
//!   peer has closed, `LOCAL_PEERPID` (and `LOCAL_PEEREPID`, and
//!   `LOCAL_PEERTOKEN`, which carries a pid too) says `ENOTCONN`.
//! * **Linux** has no `getpeereid` at all: the kernel hands all three over at
//!   once through `getsockopt(SOL_SOCKET, SO_PEERCRED)`, which fills in a
//!   `libc::ucred` — pid, uid and gid, recorded at `connect` and still answered
//!   after the peer has closed. So the user is read out of the answer to the
//!   wider question, and the narrow door is the wide one with two fields
//!   dropped.
//!
//! **So a pid is asked the moment a connection stands, and the end that refuses
//! does not close first.** The launch door's client asks right after `connect`
//! and before it writes a byte (`crate::launch_conversation`), and its server,
//! refusing, holds its end until the client's first move — which is what keeps
//! Darwin's answer from depending on which of the two ends got there first.
//! What the answers name is the peer as the connection knows it, not as a
//! frame says it: a number the peer wrote is not an answer to "who are you".
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

/// The peer of a connected Unix socket, as the kernel names it.
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
/// The kernel's own, for a descriptor that is not a connected socket — and,
/// here, `NotConnected` for one whose peer has already closed its end.
#[cfg(target_os = "macos")]
pub(crate) fn credentials(stream: &UnixStream) -> io::Result<Credentials> {
    /// `<sys/un.h>`: the option level for an `AF_UNIX` socket's own options.
    const SOL_LOCAL: libc::c_int = 0;
    /// `<sys/un.h>`: the pid of the peer, answered while the connection stands.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// What this kernel answers for the process id of a peer that has closed
    /// its end — the one place the two arms differ in what they keep: the pid
    /// of the process that connected (this one, in the test below), or that
    /// the connection is gone.
    #[cfg(target_os = "macos")]
    fn the_pid_once_the_peer_has_closed() -> Result<Option<u32>, io::ErrorKind> {
        Err(io::ErrorKind::NotConnected)
    }
    #[cfg(not(target_os = "macos"))]
    fn the_pid_once_the_peer_has_closed() -> Result<Option<u32>, io::ErrorKind> {
        Ok(Some(std::process::id()))
    }

    /// **PIN (T-LAUNCH-WIRE-PEER-READ) — what each kernel still answers about a
    /// peer that has closed its end.**
    ///
    /// The fact the launch wire's order rests on, measured rather than assumed:
    /// the user is the record made at connect on both kernels; the process id
    /// is too on Linux, and on Darwin it is answered only while the connection
    /// stands — which is why the launch door's client asks first and its
    /// refusing server does not close first. Here the peer closes before the
    /// question is asked, deliberately.
    #[test]
    fn what_the_kernel_still_answers_after_the_peer_has_closed() {
        let directory = bt_testpath::temp_path("peer-closed 对端");
        std::fs::create_dir_all(&directory).expect("make the scratch directory");
        let path = directory.join("s");
        let listener = UnixListener::bind(&path).expect("bind a socket");
        let client = UnixStream::connect(&path).expect("connect to it");
        let (server, _) = listener.accept().expect("accept the connection");
        // SAFETY: `geteuid` reads this process's own credentials and cannot fail.
        let me = unsafe { libc::geteuid() };
        assert_eq!(
            uid(&client).expect("the user is answered while connected"),
            me
        );
        drop(server);

        assert_eq!(
            uid(&client).expect("the user is the record made at connect"),
            me,
            "the peer's user is still named after it has closed"
        );
        assert_eq!(
            credentials(&client)
                .map(|peer| peer.pid)
                .map_err(|error| error.kind()),
            the_pid_once_the_peer_has_closed(),
            "the peer's process, asked after it has closed"
        );
        drop(client);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
