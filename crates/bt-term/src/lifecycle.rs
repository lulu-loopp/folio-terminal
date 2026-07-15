use crate::adapter::AdapterEvent;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleEventKind {
    RowsRemoved,
    ForcedFinalize,
    PrimaryParked,
    PrimaryRestored,
    HistoryCleared,
    QuotaEvicted,
    CandidatesInvalidated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleDirective {
    CaptureAndRebase,
    Finalize,
    ParkPrimary,
    RestorePrimary,
    DeleteHistoryAndStaging,
    DeleteHistory,
    InvalidateStaging,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LifecycleRule {
    pub event: LifecycleEventKind,
    pub directive: LifecycleDirective,
}

/// DESIGN.md §3.1 executable lifecycle table. Payload handling remains in the session actor.
pub const LIFECYCLE_RULES: [LifecycleRule; 7] = [
    LifecycleRule {
        event: LifecycleEventKind::RowsRemoved,
        directive: LifecycleDirective::CaptureAndRebase,
    },
    LifecycleRule {
        event: LifecycleEventKind::ForcedFinalize,
        directive: LifecycleDirective::Finalize,
    },
    LifecycleRule {
        event: LifecycleEventKind::PrimaryParked,
        directive: LifecycleDirective::ParkPrimary,
    },
    LifecycleRule {
        event: LifecycleEventKind::PrimaryRestored,
        directive: LifecycleDirective::RestorePrimary,
    },
    LifecycleRule {
        event: LifecycleEventKind::HistoryCleared,
        directive: LifecycleDirective::DeleteHistoryAndStaging,
    },
    LifecycleRule {
        event: LifecycleEventKind::QuotaEvicted,
        directive: LifecycleDirective::DeleteHistory,
    },
    LifecycleRule {
        event: LifecycleEventKind::CandidatesInvalidated,
        directive: LifecycleDirective::InvalidateStaging,
    },
];

fn event_kind(event: &AdapterEvent) -> LifecycleEventKind {
    match event {
        AdapterEvent::RowsRemoved(_) => LifecycleEventKind::RowsRemoved,
        AdapterEvent::ForcedFinalize(_) => LifecycleEventKind::ForcedFinalize,
        AdapterEvent::PrimaryParked => LifecycleEventKind::PrimaryParked,
        AdapterEvent::PrimaryRestored => LifecycleEventKind::PrimaryRestored,
        AdapterEvent::HistoryCleared(_) => LifecycleEventKind::HistoryCleared,
        AdapterEvent::QuotaEvicted(_) => LifecycleEventKind::QuotaEvicted,
        AdapterEvent::CandidatesInvalidated => LifecycleEventKind::CandidatesInvalidated,
    }
}

pub(crate) fn directive(event: &AdapterEvent) -> Option<LifecycleDirective> {
    let kind = event_kind(event);
    LIFECYCLE_RULES
        .iter()
        .find(|rule| rule.event == kind)
        .map(|rule| rule.directive)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_event_kind_occurs_exactly_once() {
        for kind in [
            LifecycleEventKind::RowsRemoved,
            LifecycleEventKind::ForcedFinalize,
            LifecycleEventKind::PrimaryParked,
            LifecycleEventKind::PrimaryRestored,
            LifecycleEventKind::HistoryCleared,
            LifecycleEventKind::QuotaEvicted,
            LifecycleEventKind::CandidatesInvalidated,
        ] {
            assert_eq!(
                LIFECYCLE_RULES
                    .iter()
                    .filter(|rule| rule.event == kind)
                    .count(),
                1
            );
        }
    }
}
