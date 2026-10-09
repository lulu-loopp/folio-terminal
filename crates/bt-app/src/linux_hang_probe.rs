//! A bounded check that the Linux window thread is processing winit user events.
//!
//! This measures user-event responsiveness only. It does not independently
//! observe native modal loops, so it is a weaker measurement than the native
//! Windows and macOS probes.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use bt_platform::hang::Answer;

type Wake = dyn Fn(u64) -> bool + Send + Sync + 'static;

static NEXT_QUESTION_ID: AtomicU64 = AtomicU64::new(1);

fn next_question_id() -> u64 {
    NEXT_QUESTION_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            next.checked_add(1)
        })
        .expect("Linux hang-probe question IDs exhausted")
}

fn registry() -> &'static Mutex<Option<Arc<Instance>>> {
    static REGISTRY: OnceLock<Mutex<Option<Arc<Instance>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(None))
}

fn registered_instance() -> Option<Arc<Instance>> {
    registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

struct Pending {
    id: u64,
    reply: SyncSender<()>,
}

#[derive(Default)]
struct State {
    wake: Option<Arc<Wake>>,
    pending: Option<Pending>,
}

struct Instance {
    state: Mutex<State>,
}

impl Instance {
    fn new(wake: impl Fn(u64) -> bool + Send + Sync + 'static) -> Self {
        Self {
            state: Mutex::new(State {
                wake: Some(Arc::new(wake)),
                pending: None,
            }),
        }
    }

    fn ask(&self, timeout: Duration) -> Answer {
        let started = Instant::now();
        let (id, wake, reply_rx) = {
            let mut state: MutexGuard<'_, State> = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(wake) = state.wake.as_ref().cloned() else {
                return Answer::NoWindow;
            };
            if state.pending.is_some() {
                return Answer::Silent;
            }

            let (reply_tx, reply_rx) = sync_channel(1);
            let id = next_question_id();
            state.pending = Some(Pending {
                id,
                reply: reply_tx,
            });
            (id, wake, reply_rx)
        };

        if !wake(id) {
            self.clear_pending(id);
            return Answer::NoWindow;
        }

        let remaining = timeout.saturating_sub(started.elapsed());
        match reply_rx.recv_timeout(remaining) {
            Ok(()) => Answer::Answered,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Answer::Silent,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Answer::NoWindow,
        }
    }

    fn answer(&self, id: u64) {
        let pending = {
            let mut state: MutexGuard<'_, State> = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.pending.as_ref().map(|pending| pending.id) == Some(id) {
                state.pending.take()
            } else {
                None
            }
        };
        if let Some(pending) = pending {
            let _ = pending.reply.try_send(());
        }
    }

    fn clear_pending(&self, id: u64) {
        let mut state: MutexGuard<'_, State> = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.pending.as_ref().map(|pending| pending.id) == Some(id) {
            state.pending.take();
        }
    }

    fn close(&self) {
        let pending = {
            let mut state: MutexGuard<'_, State> = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.wake = None;
            state.pending.take()
        };
        drop(pending);
    }
}

/// Keeps one Linux user-event responsiveness probe registered until dropped.
#[must_use]
pub struct Registration {
    instance: Arc<Instance>,
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.instance.close();
        let mut current = registry()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if current
            .as_ref()
            .is_some_and(|instance| Arc::ptr_eq(instance, &self.instance))
        {
            *current = None;
        }
    }
}

/// Registers the event-loop wake function used by [`ask`].
///
/// The function should enqueue the given numeric ID as a winit user event and
/// return whether the event was accepted.
pub fn install(wake: impl Fn(u64) -> bool + Send + Sync + 'static) -> Registration {
    let instance = Arc::new(Instance::new(wake));
    *registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::clone(&instance));
    Registration { instance }
}

/// Asks the registered Linux event loop to process a user event within `timeout`.
pub fn ask(timeout: Duration) -> Answer {
    let started = Instant::now();
    registered_instance().map_or(Answer::NoWindow, |instance| {
        instance.ask(timeout.saturating_sub(started.elapsed()))
    })
}

/// Acknowledge a matching question at the start of its winit user-event handler.
pub fn answer(id: u64) {
    if let Some(instance) = registered_instance() {
        instance.answer(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::{Receiver, TryRecvError};
    use std::thread;

    const ANSWER_PATIENCE: Duration = Duration::from_secs(60);
    static GLOBAL_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn global_test_lock() -> MutexGuard<'static, ()> {
        GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn channel_instance() -> (Arc<Instance>, Receiver<u64>) {
        let (wake_tx, wake_rx) = std::sync::mpsc::sync_channel(1);
        let instance = Arc::new(Instance::new(move |id| wake_tx.try_send(id).is_ok()));
        (instance, wake_rx)
    }

    #[test]
    fn a_matching_user_event_answers_an_active_question() {
        let (instance, wake_rx) = channel_instance();
        let asking = Arc::clone(&instance);
        let waiter = thread::spawn(move || asking.ask(ANSWER_PATIENCE));

        let id = wake_rx.recv().expect("question was not sent");
        instance.answer(id);

        assert_eq!(waiter.join().expect("asker panicked"), Answer::Answered);
    }

    #[test]
    fn zero_timeout_keeps_one_tombstone_until_its_event_arrives() {
        let (instance, wake_rx) = channel_instance();

        assert_eq!(instance.ask(Duration::ZERO), Answer::Silent);
        let first_id = wake_rx.recv().expect("zero-timeout question was not sent");
        assert_eq!(instance.ask(Duration::ZERO), Answer::Silent);
        assert!(matches!(wake_rx.try_recv(), Err(TryRecvError::Empty)));

        instance.answer(first_id);
        assert_eq!(instance.ask(Duration::ZERO), Answer::Silent);
        let second_id = wake_rx.recv().expect("next question was not sent");
        assert!(second_id > first_id);
        instance.answer(second_id);
    }

    #[test]
    fn an_ack_for_an_older_question_does_not_answer_the_current_one() {
        let (instance, wake_rx) = channel_instance();
        let asking = Arc::clone(&instance);
        let first_waiter = thread::spawn(move || asking.ask(ANSWER_PATIENCE));
        let first_id = wake_rx.recv().expect("first question was not sent");
        instance.answer(first_id);
        assert_eq!(
            first_waiter.join().expect("first asker panicked"),
            Answer::Answered
        );

        let asking = Arc::clone(&instance);
        let second_waiter = thread::spawn(move || asking.ask(ANSWER_PATIENCE));
        let second_id = wake_rx.recv().expect("second question was not sent");
        assert!(second_id > first_id);
        instance.answer(first_id);
        let pending_id = instance
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending
            .as_ref()
            .map(|pending| pending.id);
        instance.answer(second_id);

        assert_eq!(pending_id, Some(second_id));
        assert_eq!(
            second_waiter.join().expect("second asker panicked"),
            Answer::Answered
        );
    }

    #[test]
    fn a_failed_send_returns_no_window_and_is_not_retried() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let counted_attempts = Arc::clone(&attempts);
        let instance = Instance::new(move |_| {
            counted_attempts.fetch_add(1, Ordering::Relaxed);
            false
        });

        assert_eq!(instance.ask(ANSWER_PATIENCE), Answer::NoWindow);
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
        assert!(
            instance
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pending
                .is_none()
        );
    }

    #[test]
    fn dropping_registration_releases_an_active_waiter() {
        let _lock = global_test_lock();
        let (wake_tx, wake_rx) = std::sync::mpsc::sync_channel(1);
        let registration = install(move |id| wake_tx.try_send(id).is_ok());
        let waiter = thread::spawn(|| ask(ANSWER_PATIENCE));

        let _id = wake_rx.recv().expect("registered question was not sent");
        drop(registration);

        assert_eq!(waiter.join().expect("asker panicked"), Answer::NoWindow);
        assert_eq!(ask(Duration::ZERO), Answer::NoWindow);
    }

    #[test]
    fn dropping_a_stale_registration_keeps_the_new_registration() {
        let _lock = global_test_lock();
        let (old_wake_tx, old_wake_rx) = std::sync::mpsc::sync_channel(1);
        let old_registration = install(move |id| old_wake_tx.try_send(id).is_ok());
        let (new_wake_tx, new_wake_rx) = std::sync::mpsc::sync_channel(1);
        let new_registration = install(move |id| new_wake_tx.try_send(id).is_ok());

        drop(old_registration);
        assert_eq!(ask(Duration::ZERO), Answer::Silent);
        let id = new_wake_rx.recv().expect("new registration was not used");
        assert!(old_wake_rx.try_recv().is_err());
        answer(id);
        drop(new_registration);
    }
}
