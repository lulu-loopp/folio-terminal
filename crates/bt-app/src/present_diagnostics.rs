//! Observation only. No deadlines, rendering decisions, native reads, or user text.
use std::cell::Cell;
use std::sync::LazyLock;
use std::time::Instant;

use bt_render::{FrameSource, PresentConfiguration, PresentPhase};

static ORIGIN: LazyLock<Instant> = LazyLock::new(Instant::now);

pub fn timestamp(now: Instant) -> u64 {
    now.saturating_duration_since(*ORIGIN).as_micros() as u64
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Progress {
    pub events: u64,
    pub turns: u64,
}

thread_local! { static PROGRESS: Cell<Progress> = const { Cell::new(Progress { events: 0, turns: 0 }) }; }

pub fn progress() -> Progress {
    PROGRESS.get()
}
pub fn event() {
    let p = PROGRESS.get();
    PROGRESS.set(Progress {
        events: p.events + 1,
        ..p
    });
}
pub fn turn() {
    let p = PROGRESS.get();
    PROGRESS.set(Progress {
        turns: p.turns + 1,
        ..p
    });
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Outcome {
    #[default]
    NoPicture,
    Presented,
    Unchanged,
    WithoutText,
    Skipped,
    NotVisible,
    Reconfigure,
    FailedRender,
    FailedCommit,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::NoPicture => "no_picture",
            Self::Presented => "presented",
            Self::Unchanged => "unchanged",
            Self::WithoutText => "without_text",
            Self::Skipped => "skipped",
            Self::NotVisible => "not_visible",
            Self::Reconfigure => "reconfigure",
            Self::FailedRender => "failed:render",
            Self::FailedCommit => "failed:commit",
        }
    }
    fn from_index(index: usize) -> Self {
        [
            Self::NoPicture,
            Self::Presented,
            Self::Unchanged,
            Self::WithoutText,
            Self::Skipped,
            Self::NotVisible,
            Self::Reconfigure,
            Self::FailedRender,
            Self::FailedCommit,
        ][index]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    Stale,
    Landed,
}

fn decade(age_us: u64) -> u32 {
    let mut threshold = 1_000_000u64;
    let mut level = 0;
    while age_us >= threshold {
        level += 1;
        let Some(next) = threshold.checked_mul(10) else {
            break;
        };
        threshold = next;
    }
    level
}

/// One second, then powers of ten. Time and visibility are values, not reads.
pub fn decision(
    owed: bool,
    shown: bool,
    minimized: bool,
    age_us: u64,
    reported: u32,
    landed: bool,
) -> Option<Line> {
    if landed && reported != 0 {
        return Some(Line::Landed);
    }
    (owed && shown && !minimized && decade(age_us) > reported).then_some(Line::Stale)
}

#[derive(Default)]
pub struct State {
    pub sequence: u64,
    pub pending_since: Option<u64>,
    pub last_present: Option<u64>,
    pub last_landed: (u64, u64),
    pub last_attempt: (u64, u64, Outcome),
    reported: u32,
    counts: [u64; 9],
    baseline: Progress,
}

impl State {
    pub fn observe(&mut self, owed: bool, now: u64, progress: Progress) {
        if owed && self.pending_since.is_none() {
            self.pending_since = Some(now);
            self.baseline = progress;
            self.counts = [0; 9];
        } else if !owed {
            self.pending_since = None;
            self.reported = 0;
        }
    }
    pub fn age(&self, now: u64) -> u64 {
        self.pending_since.map_or(0, |at| now.saturating_sub(at))
    }
    pub fn check(
        &mut self,
        owed: bool,
        shown: bool,
        minimized: bool,
        now: u64,
        landed: bool,
    ) -> Option<Line> {
        let age = self.age(now);
        let line = decision(owed, shown, minimized, age, self.reported, landed);
        if line == Some(Line::Stale) {
            self.reported = decade(age);
        }
        if line == Some(Line::Landed) {
            self.reported = 0;
        }
        line
    }
    pub fn attempted(&mut self, attempt: &Attempt) {
        self.last_attempt = (attempt.generation, attempt.sequence, attempt.outcome);
        self.counts[attempt.outcome as usize] += 1;
    }
    pub fn line(&self, window: u64, now: u64, progress: Progress, line: Line) -> String {
        let (reason, count) = self
            .counts
            .iter()
            .enumerate()
            .max_by_key(|(_, count)| *count)
            .unwrap();
        let (generation, sequence, outcome) = self.last_attempt;
        let last = optional_number(self.last_present.map(|at| now.saturating_sub(at) / 1000));
        let landing = if line == Line::Landed {
            "; a picture landed"
        } else {
            ""
        };
        format!(
            "Folio: window {window} has shown no new picture for {} ms — last present {last} ms ago (gen {generation}, seq {sequence}, outcome {}); {} attempts since, {count} of them {}; the window thread dispatched {} events and turned {} times in that span{landing}; last_landed_gen={} last_landed_seq={}",
            self.age(now) / 1000,
            outcome.label(),
            self.counts.iter().sum::<u64>(),
            if *count == 0 {
                "none"
            } else {
                Outcome::from_index(reason).label()
            },
            progress.events.saturating_sub(self.baseline.events),
            progress.turns.saturating_sub(self.baseline.turns),
            self.last_landed.0,
            self.last_landed.1
        )
    }
}

pub struct Attempt {
    pub window: u64,
    pub generation: u64,
    pub sequence: u64,
    source: FrameSource,
    retained: bool,
    pub outcome: Outcome,
    pub landed_at: Option<Instant>,
    pub timings: [u64; 6],
    timed: bool,
    phase: Option<usize>,
    phase_at: u64,
}

impl Attempt {
    pub fn new(
        window: u64,
        generation: u64,
        sequence: u64,
        source: FrameSource,
        retained: bool,
        timed: bool,
        now: u64,
    ) -> Self {
        Self {
            window,
            generation,
            sequence,
            source,
            retained,
            outcome: Outcome::NoPicture,
            landed_at: None,
            timings: [0; 6],
            timed,
            phase: None,
            phase_at: now,
        }
    }
    pub fn phase_at(&mut self, phase: Option<usize>, now: u64) {
        if let Some(old) = self.phase {
            self.timings[old] += now.saturating_sub(self.phase_at);
        }
        self.phase = phase;
        self.phase_at = now;
    }
    pub fn phase(&mut self, phase: Option<usize>) {
        if self.timed {
            self.phase_at(phase, timestamp(Instant::now()));
        }
    }
    pub fn render_phase(&mut self, phase: PresentPhase) {
        let index = match phase {
            PresentPhase::SurfaceConfigure(generation) => {
                self.generation = generation;
                crate::hang_watch::present_generation(generation);
                Some(0)
            }
            PresentPhase::SurfaceAcquire => Some(1),
            PresentPhase::QueueSubmit => Some(3),
            PresentPhase::Present => Some(4),
            PresentPhase::Complete => None,
            _ => Some(2),
        };
        self.phase(index);
    }
    pub fn line(
        &self,
        native: bt_platform::NativePresentFacts,
        shown: bool,
        attention: (bool, Option<u64>),
        configuration: PresentConfiguration,
        since_last: u64,
        pending_age: u64,
    ) -> String {
        let (exposed, attention_age) = attention;
        let client = native
            .client
            .map_or_else(|| "unknown".into(), |(w, h)| format!("{w}x{h}"));
        format!(
            "BT_PERF_TRACE attempt win={} gen={} seq={} src={:?} retained={} outcome={} mode={:?} latency={} wait={} native_iconic={} native_cloaked={} native_client={} native_style_visible={} folio_shown={} attention_exposed={} attention_age_us={} configure_us={} acquire_us={} encode_us={} submit_us={} present_us={} commit_us={} since_last_present_us={} pending_age_us={}",
            self.window,
            configuration.generation,
            self.sequence,
            self.source,
            u8::from(self.retained),
            self.outcome.label(),
            configuration.mode,
            configuration.latency,
            configuration.wait,
            boolean(native.iconic),
            boolean(native.cloaked),
            client,
            boolean(native.style_visible),
            u8::from(shown),
            u8::from(exposed),
            optional_number(attention_age),
            self.timings[0],
            self.timings[1],
            self.timings[2],
            self.timings[3],
            self.timings[4],
            self.timings[5],
            since_last,
            pending_age
        )
    }
}

pub fn native_fields(native: bt_platform::NativePresentFacts) -> String {
    let client = native
        .client
        .map_or_else(|| "unknown".into(), |(w, h)| format!("{w}x{h}"));
    format!(
        "native_iconic={} native_cloaked={} native_client={} native_style_visible={}",
        boolean(native.iconic),
        boolean(native.cloaked),
        client,
        boolean(native.style_visible)
    )
}

pub fn boolean(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "1",
        Some(false) => "0",
        None => "unknown",
    }
}
fn optional_number(value: Option<u64>) -> String {
    value.map_or_else(|| "unknown".into(), |n| n.to_string())
}

#[cfg(test)]
#[path = "present_diagnostics_tests.rs"]
mod tests;
