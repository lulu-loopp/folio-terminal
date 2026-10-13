//! **A second launch's side of the launch wire, in the one order both transports speak it** —
//! the client's half of `docs/DESIGN.md` §7.59b, written once over a [`Connection`] that each arm
//! of [`crate::launch_pipe`] supplies (a named pipe on Windows, a Unix socket elsewhere).
//!
//! The order is the safety, and its first step is the one this module exists to hold in one place:
//! **who answered is asked before a word is said**. The request names a folder somebody is
//! standing in, so it is not written to a process that is not this program; and on Darwin the
//! question has to be asked while the connection stands, because the kernel answers the peer's
//! process id from the live connection and says `ENOTCONN` once the peer has closed
//! (`crate::peer`'s header). A server that refuses this client therefore holds its end until the
//! client's first move (the Unix arm's `serve`), and the client makes that first move only after
//! it has its answer — so a refusal on either side is heard as the refusal it is.

use std::io;

/// **One connected end of the launch wire, at either end** — the three things a conversation does
/// with the transport, and nothing about how the transport does them. The client's conversation
/// ([`hand_over_on`]) needs these three; the Unix server's extends them with one more.
pub(crate) trait Connection {
    /// The process at the other end, if it is a Folio of this user — its id as the kernel names
    /// it, never a number out of a frame — or the refusal that says why it is not.
    fn vetted_peer(&mut self) -> io::Result<u32>;
    /// One frame out.
    fn say(&mut self, frame: &[u8]) -> io::Result<()>;
    /// One frame in.
    fn hear(&mut self) -> io::Result<Vec<u8>>;
}

/// **The client's whole conversation on a connection that has just been made**: ask who answered,
/// write the request, read the reply, let the caller act on it, confirm — and the connection is
/// closed when this returns, on every path.
///
/// `on_reply` is handed the server's process id from the first step and runs while the connection
/// is still open: the server commits only on this end's [`crate::launch_pipe::CONFIRM`], so what
/// the caller does with the answer has to be done inside the conversation.
pub(crate) fn hand_over_on(
    mut connection: impl Connection,
    request: &str,
    on_reply: impl FnOnce(u32, &str),
) -> io::Result<()> {
    // **Step 1: who answered?** First, before a byte of the command line is written.
    let server = connection.vetted_peer()?;
    connection.say(request.as_bytes())?;
    let reply = connection.hear()?;
    on_reply(server, &String::from_utf8_lossy(&reply));
    // **Step 4's second half, and the server's commit point**, written out so that the order of
    // these statements is the order of the protocol.
    connection.say(crate::launch_pipe::CONFIRM.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    /// What one recorded conversation did, in the order it did it.
    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Step {
        AskedWhoAnswered,
        Said(String),
        Heard,
        Replied(u32, String),
        Closed,
    }

    /// **A connection that writes down what is done to it** — a server whose peer answer and
    /// reply are given, and whose close is the moment it is dropped.
    struct Recording {
        steps: Rc<RefCell<Vec<Step>>>,
        peer: Option<u32>,
        reply: &'static str,
    }

    impl Connection for Recording {
        fn vetted_peer(&mut self) -> io::Result<u32> {
            self.steps.borrow_mut().push(Step::AskedWhoAnswered);
            self.peer.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "the launch endpoint is held by 另一个程序, which is not a Folio",
                )
            })
        }

        fn say(&mut self, frame: &[u8]) -> io::Result<()> {
            self.steps
                .borrow_mut()
                .push(Step::Said(String::from_utf8_lossy(frame).into_owned()));
            Ok(())
        }

        fn hear(&mut self) -> io::Result<Vec<u8>> {
            self.steps.borrow_mut().push(Step::Heard);
            Ok(self.reply.as_bytes().to_vec())
        }
    }

    impl Drop for Recording {
        fn drop(&mut self) {
            self.steps.borrow_mut().push(Step::Closed);
        }
    }

    /// One conversation over a [`Recording`] whose peer is `peer`, and what it did.
    fn converse(peer: Option<u32>) -> (io::Result<()>, Vec<Step>) {
        let steps = Rc::new(RefCell::new(Vec::new()));
        let connection = Recording {
            steps: Rc::clone(&steps),
            peer,
            reply: "已接手",
        };
        let ended = hand_over_on(connection, "--cwd 工作 folder", |server, reply| {
            steps
                .borrow_mut()
                .push(Step::Replied(server, reply.to_owned()));
        });
        let steps = steps.borrow().clone();
        (ended, steps)
    }

    /// **RED (T-LAUNCH-WIRE-PEER-READ) — the client asks who answered before it says a word, and
    /// a peer it refuses is told nothing.**
    ///
    /// The first step is the one whose place matters: on Darwin the peer's process id is answered
    /// only while the connection stands, so a client that spoke first would find a refusing server
    /// gone and say `NotConnected`, or `UnexpectedEof`, where the refusal belongs — and it would
    /// have written a folder to a program that is not a Folio. Both arms run this function, so
    /// this is the order on both.
    ///
    /// MUTATION: move `connection.vetted_peer()` after `connection.hear()` and both conversations
    /// go red — the first asks after it has said and heard, the second says the request to the
    /// peer it then refuses.
    #[test]
    fn the_client_asks_who_answered_before_it_says_a_word() {
        let (ended, steps) = converse(Some(4242));
        ended.expect("a Folio's answer is acted on and confirmed");
        assert_eq!(
            steps,
            vec![
                Step::AskedWhoAnswered,
                Step::Said("--cwd 工作 folder".to_owned()),
                Step::Heard,
                Step::Replied(4242, "已接手".to_owned()),
                Step::Said(crate::launch_pipe::CONFIRM.to_owned()),
                Step::Closed,
            ],
            "the peer is asked first, and the pid the reply is acted on with is its answer"
        );

        let (ended, steps) = converse(None);
        let refused = ended.expect_err("a peer that is not a Folio is refused");
        assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied);
        assert!(
            refused.to_string().contains("另一个程序"),
            "the refusal is the vetting's own, naming what holds the door: {refused}"
        );
        assert_eq!(
            steps,
            vec![Step::AskedWhoAnswered, Step::Closed],
            "nothing is said to a peer that is refused"
        );
    }
}
