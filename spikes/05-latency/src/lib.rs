use std::time::Instant;

use serde::Serialize;

pub const TARGET_RATES_HZ: [u32; 3] = [60, 120, 144];

#[derive(Clone, Copy, Debug)]
pub struct EchoEvent {
    pub sequence: u64,
    pub target_hz: u32,
    pub injected_at: Instant,
}

#[derive(Clone, Copy, Debug)]
pub struct SubmitReceipt {
    pub submitted_at: Instant,
    pub present_called_at: Instant,
}

/// Replaceable source boundary. It deliberately exposes no winit type so a TSF-backed
/// source can replace the spike adapter without changing the recorder.
pub trait EventSource {
    fn next_echo(&mut self) -> Result<Option<EchoEvent>, String>;
}

/// Replaceable presentation boundary. Production code can substitute another window
/// stack or renderer without changing the measurement definition.
pub trait PresentationBoundary {
    fn clear_and_submit(&mut self, event: &EchoEvent) -> Result<SubmitReceipt, String>;
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct LatencySample {
    pub sequence: u64,
    pub target_hz: u32,
    pub event_to_submit_us: u64,
    pub event_to_present_call_us: u64,
}

impl LatencySample {
    pub fn from_receipt(event: EchoEvent, receipt: SubmitReceipt) -> Result<Self, String> {
        let submit = receipt
            .submitted_at
            .checked_duration_since(event.injected_at)
            .ok_or_else(|| "submit timestamp precedes event injection".to_owned())?;
        let present = receipt
            .present_called_at
            .checked_duration_since(event.injected_at)
            .ok_or_else(|| "present-call timestamp precedes event injection".to_owned())?;
        if receipt.present_called_at < receipt.submitted_at {
            return Err("present-call timestamp precedes queue submit".to_owned());
        }
        Ok(Self {
            sequence: event.sequence,
            target_hz: event.target_hz,
            event_to_submit_us: submit.as_micros().try_into().unwrap_or(u64::MAX),
            event_to_present_call_us: present.as_micros().try_into().unwrap_or(u64::MAX),
        })
    }
}

pub fn drive<S: EventSource, P: PresentationBoundary>(
    source: &mut S,
    presenter: &mut P,
) -> Result<Vec<LatencySample>, String> {
    let mut samples = Vec::new();
    while let Some(event) = source.next_echo()? {
        let receipt = presenter.clear_and_submit(&event)?;
        samples.push(LatencySample::from_receipt(event, receipt)?);
    }
    Ok(samples)
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Distribution {
    pub samples: usize,
    pub min_us: u64,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
}

pub fn distribution(values: impl IntoIterator<Item = u64>) -> Option<Distribution> {
    let mut values = values.into_iter().collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(Distribution {
        samples: values.len(),
        min_us: values[0],
        p50_us: nearest_rank(&values, 50),
        p95_us: nearest_rank(&values, 95),
        p99_us: nearest_rank(&values, 99),
        max_us: values[values.len() - 1],
    })
}

fn nearest_rank(sorted: &[u64], percentile: usize) -> u64 {
    let rank = (percentile * sorted.len()).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[derive(Clone, Copy, Debug)]
pub struct ProposedThreshold {
    pub maximum_p95_submit_us: u64,
    pub maximum_p99_submit_us: u64,
}

impl ProposedThreshold {
    pub fn accepts(self, distribution: Distribution) -> bool {
        distribution.p95_us <= self.maximum_p95_submit_us
            && distribution.p99_us <= self.maximum_p99_submit_us
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::thread;
    use std::time::Duration;

    use super::*;

    struct ScriptedSource(VecDeque<EchoEvent>);

    impl EventSource for ScriptedSource {
        fn next_echo(&mut self) -> Result<Option<EchoEvent>, String> {
            Ok(self.0.pop_front())
        }
    }

    struct FakePresenter;

    impl PresentationBoundary for FakePresenter {
        fn clear_and_submit(&mut self, _event: &EchoEvent) -> Result<SubmitReceipt, String> {
            thread::sleep(Duration::from_millis(1));
            let submitted_at = Instant::now();
            Ok(SubmitReceipt {
                submitted_at,
                present_called_at: Instant::now(),
            })
        }
    }

    #[test]
    fn replaceable_boundaries_produce_samples_without_winit_types() {
        let now = Instant::now();
        let mut source = ScriptedSource(VecDeque::from([
            EchoEvent {
                sequence: 1,
                target_hz: 60,
                injected_at: now,
            },
            EchoEvent {
                sequence: 2,
                target_hz: 120,
                injected_at: now,
            },
        ]));
        let samples = drive(&mut source, &mut FakePresenter).unwrap();
        assert_eq!(samples.len(), 2);
        assert!(
            samples
                .iter()
                .all(|sample| sample.event_to_submit_us >= 1_000)
        );
    }

    #[test]
    fn timestamp_inversion_is_rejected() {
        let now = Instant::now();
        let event = EchoEvent {
            sequence: 1,
            target_hz: 60,
            injected_at: now,
        };
        let receipt = SubmitReceipt {
            submitted_at: now + Duration::from_millis(2),
            present_called_at: now + Duration::from_millis(1),
        };
        assert!(LatencySample::from_receipt(event, receipt).is_err());
    }

    #[test]
    fn nearest_rank_distribution_is_stable() {
        let stats = distribution(1..=100).unwrap();
        assert_eq!(stats.p50_us, 50);
        assert_eq!(stats.p95_us, 95);
        assert_eq!(stats.p99_us, 99);
    }

    #[test]
    fn a_deliberately_slow_distribution_fails_the_gate() {
        let stats = distribution([1_000, 2_000, 3_000, 50_000]).unwrap();
        let gate = ProposedThreshold {
            maximum_p95_submit_us: 10_000,
            maximum_p99_submit_us: 20_000,
        };
        assert!(!gate.accepts(stats));
    }

    #[test]
    fn target_rates_are_exactly_the_three_required_rates() {
        assert_eq!(TARGET_RATES_HZ, [60, 120, 144]);
    }
}
