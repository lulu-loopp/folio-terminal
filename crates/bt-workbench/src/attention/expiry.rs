//! **The semantic expiry rule: how long a standing credential may stand, and the clock that
//! says when each one runs out** (`docs/plans/design/ownership-census-2026-09-25.md` §5.1).
//!
//! It lived in `bt-app`'s `attention_wire` until `bt-workbench` was born, and the ledger
//! depended on the adapter for it. The rule belongs with the ledger (`docs/ARCHITECTURE.md` §12.1),
//! so the dependency now points the other way: `attention_wire` imports [`WaitClock`] from here.
//! Where the clock is *armed*, *forgotten* and *spent* — `deliver_attention` and
//! `settle_attention` — stays in `bt-app` with the routing it belongs to.

use std::time::{Duration, Instant};

use super::{ClearClass, ClearReason, ClearSelector, Event, WaitSlot};

/// How long a strong credential may stand with nothing having ended it.
///
/// Ten minutes, which is upstream's own synchronous timeout for a permission request — the longest
/// a well-behaved producer's wait can legitimately last. This is hygiene and not correctness: the
/// ledger's watermark already guarantees that the next genuine request is seen whether or not this
/// ever fires (§11.4.3). What it prevents is a badge outliving the thing it reports, which is the
/// 2026-08-21 defect stated in general form.
pub const WAIT_TTL: Duration = Duration::from_secs(600);

/// **When each standing credential runs out**, and nothing else.
///
/// Beside the ledger rather than inside it, for the ledger's own stated reason: it is a pure
/// function of arrivals, the frame's facts are handed in, and a clock is one of those facts. This
/// holds no credential — only a deadline per slot — so the two cannot disagree about *whether* a
/// pane is asking; the worst a drifted entry can do is produce a clear for something that has
/// already gone, which the ledger answers with no change and no line.
#[derive(Clone, Debug, Default)]
pub struct WaitClock {
    entries: Vec<(WaitSlot, Instant)>,
}

impl WaitClock {
    /// Start (or restart) one slot's ten minutes.
    pub fn arm(&mut self, slot: &WaitSlot, now: Instant) {
        let deadline = now + WAIT_TTL;
        match self.entries.iter_mut().find(|(held, _)| held == slot) {
            Some(entry) => entry.1 = deadline,
            None => self.entries.push((slot.clone(), deadline)),
        }
    }

    /// Forget whatever a clear has just retired.
    pub fn forget(&mut self, selector: &ClearSelector) {
        self.entries.retain(|(slot, _)| match selector {
            ClearSelector::All => false,
            ClearSelector::Kind(kind) => slot.kind() != *kind,
            ClearSelector::Key { kind, key } => {
                slot != &WaitSlot::Keyed {
                    kind: *kind,
                    key: key.clone(),
                }
            }
        });
    }

    /// The next instant this pane owes the loop a wake-up, or `None`.
    ///
    /// `None` for a pane with nothing standing, which is every pane almost always — so an idle
    /// window asks for no wake-ups at all on this account.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.entries.iter().map(|(_, at)| *at).min()
    }

    /// The clears that have come due, removing them as it goes.
    ///
    /// `Boundary`, because a timer is not a receipt: it is not evidence that anything ended, it is
    /// this build giving up on being told. Giving up has to be unconditional or it would leave
    /// exactly the entries it exists to sweep.
    pub fn due(&mut self, now: Instant) -> Vec<Event> {
        let mut expired = Vec::new();
        self.entries.retain(|(slot, at)| {
            if *at > now {
                return true;
            }
            expired.push(Event::StrongClear {
                selector: match slot {
                    WaitSlot::Keyed { kind, key } => ClearSelector::Key {
                        kind: *kind,
                        key: key.clone(),
                    },
                    WaitSlot::Level(kind) => ClearSelector::Kind(*kind),
                },
                class: ClearClass::Boundary,
                reason: ClearReason::Ttl,
                begins_turn: false,
            });
            false
        });
        expired
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::WaitKind;

    /// **The clock is hygiene and it expires as a boundary.**
    #[test]
    fn a_standing_credential_runs_out_after_ten_minutes_and_not_before() {
        let start = Instant::now();
        let mut clock = WaitClock::default();
        let slot = WaitSlot::Level(WaitKind::Permission);
        clock.arm(&slot, start);
        assert_eq!(clock.deadline(), Some(start + WAIT_TTL));
        assert!(
            clock
                .due(start + WAIT_TTL - Duration::from_secs(1))
                .is_empty()
        );
        assert_eq!(
            clock.due(start + WAIT_TTL),
            [Event::StrongClear {
                selector: ClearSelector::Kind(WaitKind::Permission),
                class: ClearClass::Boundary,
                reason: ClearReason::Ttl,
                begins_turn: false,
            }],
            "a timer is not a receipt: it is this build giving up on being told, and giving up \
             conditionally would leave behind exactly the entry it exists to sweep"
        );
        assert_eq!(
            clock.deadline(),
            None,
            "a fired entry is gone, not repeated"
        );
    }

    /// Re-asserting a credential restarts its clock rather than adding a second one.
    #[test]
    fn a_restated_credential_keeps_one_deadline() {
        let start = Instant::now();
        let mut clock = WaitClock::default();
        let slot = WaitSlot::Level(WaitKind::Permission);
        clock.arm(&slot, start);
        clock.arm(&slot, start + Duration::from_secs(60));
        assert_eq!(
            clock.deadline(),
            Some(start + Duration::from_secs(60) + WAIT_TTL)
        );
        assert!(clock.due(start + WAIT_TTL).is_empty());
        clock.forget(&ClearSelector::Kind(WaitKind::Permission));
        assert_eq!(clock.deadline(), None);
    }

    /// An idle pane owes the loop nothing.
    #[test]
    fn a_pane_with_nothing_standing_asks_for_no_wake_ups() {
        assert_eq!(WaitClock::default().deadline(), None);
    }
}
