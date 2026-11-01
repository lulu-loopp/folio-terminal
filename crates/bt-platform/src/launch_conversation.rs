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

use std::{
    io,
    time::{Duration, Instant},
};

/// **The clock the launch wire's waits are read on, and the waits themselves**
/// (T-LAUNCH-HANDOVER-BUDGET-ON-A-CLOCK). A wait of the conversation is a wait for the other end
/// to act, at either end: the second launch's budget ([`crate::launch_pipe::HANDOVER_BUDGET`]:
/// the reply on Windows, every frame on Unix), the server's step deadline, and the Windows
/// listener's first word. Each arm reads its deadline on [`LaunchClock::now`] and hands each of
/// those waits to [`LaunchClock::wait`]; the Windows busy retry's own waits, made inside the
/// client's open, are counted through [`LaunchClock::spent`]. The Windows writes are not here:
/// each is `attention_pipe::write_bounded`'s own real 250 ms, which buffering decides, not the
/// peer (noted in `docs/plans/TIMING-BOUND-TESTS.tsv`). [`RealClock`]
/// in the product, through `hand_over` and `LaunchPipe::start`; a test hands a counted clock, so
/// "the other end acted in time" and "the budget ran out" are facts of the test, never a race
/// between a real window and a thread of the same process.
pub(crate) trait LaunchClock: Send + Sync {
    /// The monotonic reading a deadline is set and read on.
    fn now(&self) -> Instant;
    /// Wait up to `bound` for the other end. `ready(d)` makes the transport's own wait of `d` and
    /// answers whether the other end acted inside it; this answers the same.
    fn wait(&self, bound: Duration, ready: &mut dyn FnMut(Duration) -> bool) -> bool;
    /// A wait of `waited` the transport has already made on its own, outside [`Self::wait`] (the
    /// Windows client's open, past a busy endpoint): nothing on the real clock, which has moved by
    /// it already. Only the Windows transport makes such a wait.
    #[cfg(windows)]
    fn spent(&self, waited: Duration);
}

/// **The product's [`LaunchClock`]**: the monotonic clock, and each wait made once, as long as it
/// was asked to be.
pub(crate) struct RealClock;

impl LaunchClock for RealClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn wait(&self, bound: Duration, ready: &mut dyn FnMut(Duration) -> bool) -> bool {
        ready(bound)
    }

    #[cfg(windows)]
    fn spent(&self, _waited: Duration) {}
}

/// What a [`CountedClock`] does with one wait it is asked for.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Turn {
    /// Wait for the other end within the door suites' patience
    /// ([`crate::door_skipped::PATIENCE`]); a wait the other end does not end inside it is spent
    /// whole, on the count.
    Await,
    /// Do not wait: the whole bound is spent, on the count.
    Spend,
}

/// **A test's [`LaunchClock`]** (T-LAUNCH-HANDOVER-BUDGET-ON-A-CLOCK): its reading moves only by
/// the waits it spends, and `turn` decides, for each wait by its number from one and its bound,
/// whether the other end is awaited — within the suite's patience — or the bound is spent at once.
/// "Answered in time" is the other end's event; "the budget ran out" is the count; a wait of no
/// time at all is spent without being made. It writes down every bound it was asked for.
#[cfg(test)]
pub(crate) struct CountedClock {
    began: Instant,
    /// How far the spent waits moved the clock, and every bound asked for, in order.
    counted: std::sync::Mutex<(Duration, Vec<Duration>)>,
    turn: std::sync::Mutex<Box<dyn FnMut(usize, Duration) -> Turn + Send>>,
}

#[cfg(test)]
impl CountedClock {
    pub(crate) fn new(turn: impl FnMut(usize, Duration) -> Turn + Send + 'static) -> Self {
        Self {
            began: Instant::now(),
            counted: std::sync::Mutex::new((Duration::ZERO, Vec::new())),
            turn: std::sync::Mutex::new(Box::new(turn)),
        }
    }

    /// A clock whose every wait awaits the other end.
    pub(crate) fn answering() -> Self {
        Self::new(|_, _| Turn::Await)
    }

    /// How far the spent waits moved the clock.
    pub(crate) fn moved(&self) -> Duration {
        self.counted.lock().unwrap().0
    }

    /// Every bound a wait was asked for, in order.
    pub(crate) fn asked(&self) -> Vec<Duration> {
        self.counted.lock().unwrap().1.clone()
    }
}

#[cfg(test)]
impl LaunchClock for CountedClock {
    fn now(&self) -> Instant {
        self.began + self.moved()
    }

    fn wait(&self, bound: Duration, ready: &mut dyn FnMut(Duration) -> bool) -> bool {
        if self.turn_for(bound) == Turn::Await
            && !bound.is_zero()
            && ready(crate::door_skipped::PATIENCE)
        {
            return true;
        }
        self.moves_by(bound);
        false
    }

    /// A wait the transport made on its own (a busy endpoint's) is asked of `turn` like any
    /// other: `Spend` moves the clock by it; `Await` does not — the other end frees the endpoint
    /// in its own time, and the transport asks again.
    #[cfg(windows)]
    fn spent(&self, waited: Duration) {
        if self.turn_for(waited) == Turn::Spend {
            self.moves_by(waited);
        }
    }
}

#[cfg(test)]
impl CountedClock {
    /// Write `bound` down, and ask `turn` for this wait.
    fn turn_for(&self, bound: Duration) -> Turn {
        let number = {
            let mut counted = self.counted.lock().unwrap();
            counted.1.push(bound);
            counted.1.len()
        };
        (self.turn.lock().unwrap())(number, bound)
    }

    fn moves_by(&self, bound: Duration) {
        let mut counted = self.counted.lock().unwrap();
        counted.0 = counted.0.saturating_add(bound);
    }
}

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
