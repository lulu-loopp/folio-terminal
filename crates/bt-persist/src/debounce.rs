//! Debounced-write state machine — docs/M2-persistence-schema-v1.md §5.1.
//!
//! "`session.json` 在有意义的变更后 debounce 约 1-2 秒落盘…不是每次事件立即
//! 写、也不是只在退出时写一次". The 1-2s figure itself is a call-site policy
//! (the chrome slice that wires this crate into `bt-app`), not something
//! this crate hardcodes — see [`Debouncer::should_flush`]'s `debounce`
//! parameter. This type holds no timer and never calls `Instant::now()`
//! itself: every method takes the current time as an explicit argument, so
//! tests can drive it with synthetic timestamps instead of real sleeps.

use std::time::{Duration, Instant};

/// Tracks "there is an unsaved change, and it happened at time T" and
/// answers "has the debounce window elapsed as of time T'?". Does not
/// perform any I/O itself — pair it with [`crate::atomic_write`] (or the
/// typed `write_settings`/`write_session` helpers) at the call site once
/// [`Debouncer::should_flush`] says it's time.
#[derive(Debug, Clone, Copy, Default)]
pub struct Debouncer {
    dirty_since: Option<Instant>,
}

impl Debouncer {
    pub fn new() -> Self {
        Self { dirty_since: None }
    }

    /// Record that a debounce-worthy change happened at `now`. Each call
    /// resets the window — this is "静默窗口" semantics (§5.1: "触发计时器
    /// 重置"), not a fixed deadline from the first change.
    pub fn mark_dirty(&mut self, now: Instant) {
        self.dirty_since = Some(now);
    }

    /// Whether there is an unsaved change at all (regardless of whether the
    /// debounce window has elapsed).
    pub fn is_dirty(&self) -> bool {
        self.dirty_since.is_some()
    }

    /// True when there is a pending change *and* at least `debounce` has
    /// elapsed since the most recent [`Self::mark_dirty`] call, as measured
    /// against the caller-supplied `now`. Never true with no pending change.
    pub fn should_flush(&self, now: Instant, debounce: Duration) -> bool {
        match self.dirty_since {
            Some(t) => now.saturating_duration_since(t) >= debounce,
            None => false,
        }
    }

    /// Call after a successful flush to clear the pending-change state.
    pub fn mark_flushed(&mut self) {
        self.dirty_since = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_pending_change_never_flushes() {
        let debouncer = Debouncer::new();
        assert!(!debouncer.is_dirty());
        assert!(!debouncer.should_flush(Instant::now(), Duration::from_secs(0)));
    }

    #[test]
    fn flushes_only_after_the_window_elapses() {
        let mut debouncer = Debouncer::new();
        let t0 = Instant::now();
        debouncer.mark_dirty(t0);
        assert!(debouncer.is_dirty());

        let window = Duration::from_millis(1500);
        assert!(!debouncer.should_flush(t0 + Duration::from_millis(500), window));
        assert!(!debouncer.should_flush(t0 + Duration::from_millis(1499), window));
        assert!(debouncer.should_flush(t0 + Duration::from_millis(1500), window));
        assert!(debouncer.should_flush(t0 + Duration::from_secs(10), window));
    }

    #[test]
    fn a_new_change_resets_the_window() {
        let mut debouncer = Debouncer::new();
        let t0 = Instant::now();
        let window = Duration::from_secs(1);
        debouncer.mark_dirty(t0);
        // Right before the window would have elapsed, another change lands.
        let t1 = t0 + Duration::from_millis(900);
        debouncer.mark_dirty(t1);
        // The old deadline (t0 + 1s) has passed, but the window restarted at t1.
        assert!(!debouncer.should_flush(t0 + Duration::from_millis(1000), window));
        assert!(debouncer.should_flush(t1 + Duration::from_secs(1), window));
    }

    #[test]
    fn mark_flushed_clears_pending_state() {
        let mut debouncer = Debouncer::new();
        let t0 = Instant::now();
        debouncer.mark_dirty(t0);
        debouncer.mark_flushed();
        assert!(!debouncer.is_dirty());
        assert!(!debouncer.should_flush(t0 + Duration::from_secs(100), Duration::from_secs(1)));
    }
}
