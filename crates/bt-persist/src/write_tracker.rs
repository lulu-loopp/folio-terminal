//! Write-failure alert cadence — docs/M2-persistence-schema-v1.md §5.3.
//!
//! "按失败连续段(failure streak)告警一次,不按尝试次数告警…进入失败状态时告警
//! 一次;此后同一原因的连续失败不再重复告警;下一次写入成功后重置计数,若后续
//! 又失败则视为新的失败段,重新告警一次." This is pure bookkeeping over a
//! caller-supplied sequence of write outcomes — it does not retry writes, do
//! I/O, or decide *how* the alert is shown (that is the chrome slice's job);
//! it only answers "is this particular failure the start of a new streak".

/// What the caller should do about this write outcome, per §5.3's cadence
/// rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteAlertAction {
    /// Either the write succeeded, or it failed but is part of an
    /// already-alerted streak — do not show anything.
    None,
    /// This failure starts a new streak (the previous attempt succeeded, or
    /// this is the first attempt ever) — surface exactly one alert.
    AlertOnce,
}

/// Tracks whether the most recent write attempt failed, so repeated
/// failures during (say) a full disk collapse into a single alert instead
/// of one per debounce cycle.
#[derive(Debug, Clone, Copy, Default)]
pub struct WriteFailureTracker {
    in_failure_streak: bool,
}

impl WriteFailureTracker {
    pub fn new() -> Self {
        Self {
            in_failure_streak: false,
        }
    }

    /// Feed the outcome of one write attempt. Returns whether the caller
    /// should surface an alert for it.
    pub fn record(&mut self, succeeded: bool) -> WriteAlertAction {
        if succeeded {
            self.in_failure_streak = false;
            return WriteAlertAction::None;
        }
        if self.in_failure_streak {
            WriteAlertAction::None
        } else {
            self.in_failure_streak = true;
            WriteAlertAction::AlertOnce
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_failure_alerts_once() {
        let mut tracker = WriteFailureTracker::new();
        assert_eq!(tracker.record(false), WriteAlertAction::AlertOnce);
    }

    #[test]
    fn repeated_failures_in_the_same_streak_stay_silent() {
        let mut tracker = WriteFailureTracker::new();
        assert_eq!(tracker.record(false), WriteAlertAction::AlertOnce);
        assert_eq!(tracker.record(false), WriteAlertAction::None);
        assert_eq!(tracker.record(false), WriteAlertAction::None);
    }

    #[test]
    fn success_resets_the_streak_so_the_next_failure_alerts_again() {
        let mut tracker = WriteFailureTracker::new();
        assert_eq!(tracker.record(false), WriteAlertAction::AlertOnce);
        assert_eq!(tracker.record(true), WriteAlertAction::None);
        assert_eq!(tracker.record(false), WriteAlertAction::AlertOnce);
    }

    #[test]
    fn success_alone_never_alerts() {
        let mut tracker = WriteFailureTracker::new();
        assert_eq!(tracker.record(true), WriteAlertAction::None);
        assert_eq!(tracker.record(true), WriteAlertAction::None);
    }
}
