//! Reporting half of the process ledger. The existing hang-watch worker calls
//! this after handling hang evidence. No timer, thread, window deadline or wake
//! is installed here. A quiet untraced run does only the minute/activity checks;
//! it neither collects a bank nor formats or writes a line.

use bt_platform::file_reads::{Ledger, Reporter};
use winit::event::{Ime, WindowEvent};

pub fn is_user_input(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::KeyboardInput {
            is_synthetic: false,
            ..
        } | WindowEvent::Ime(Ime::Preedit(..) | Ime::Commit(_))
            | WindowEvent::ModifiersChanged(_)
            | WindowEvent::CursorMoved { .. }
            | WindowEvent::CursorEntered { .. }
            | WindowEvent::CursorLeft { .. }
            | WindowEvent::MouseInput { .. }
            | WindowEvent::MouseWheel { .. }
            | WindowEvent::Touch(_)
            | WindowEvent::PinchGesture { .. }
            | WindowEvent::PanGesture { .. }
            | WindowEvent::DoubleTapGesture { .. }
            | WindowEvent::RotationGesture { .. }
            | WindowEvent::TouchpadPressure { .. }
            | WindowEvent::AxisMotion { .. }
            | WindowEvent::DroppedFile(_)
            | WindowEvent::HoveredFile(_)
            | WindowEvent::HoveredFileCancelled
            | WindowEvent::CloseRequested
    )
}

#[derive(Default)]
pub struct Clock {
    last_minute: u64,
    last_boundary_ms: u64,
    pending: Option<(u64, bool, u64)>,
    reporter: Reporter,
}

impl Clock {
    /// Time is supplied by the watchdog's existing session clock. Collection
    /// racing an add is deferred until that worker's next already-owned wake.
    pub fn tick(
        &mut self,
        now_ms: u64,
        trace: bool,
        ledger: &Ledger,
        mut input: impl FnMut() -> bool,
        mut diagnostic: impl FnMut(String),
        mut perf: impl FnMut(String),
    ) {
        let minute = now_ms / 60_000;
        if self.pending.is_none() {
            if minute <= self.last_minute {
                return;
            }
            self.last_minute = minute;
            let elapsed_ms = now_ms.saturating_sub(self.last_boundary_ms);
            self.last_boundary_ms = now_ms;
            let input_seen = input();
            if !ledger.changed() && !trace && !self.reporter.active() {
                return;
            }
            self.pending = Some((minute, input_seen, elapsed_ms));
        }
        let Some(mut totals) = ledger.rotate() else {
            return;
        };
        let (end, input_seen, elapsed_ms) = self.pending.take().expect("a collection has a minute");
        totals.elapsed_ms = elapsed_ms;
        if trace {
            perf(totals.perf_line(end, input_seen));
        }
        if let Some(line) = self.reporter.observe(end, &totals, input_seen) {
            diagnostic(line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_platform::file_reads::Lane;

    #[test]
    fn file_reads_clock_has_no_idle_collection_and_trace_includes_zero_minutes() {
        let ledger = Ledger::new();
        let mut clock = Clock::default();
        let mut diagnostics = Vec::new();
        let mut perf = Vec::new();
        for time in [0, 59_999, 60_000, 119_999] {
            clock.tick(
                time,
                false,
                &ledger,
                || false,
                |line| diagnostics.push(line),
                |line| perf.push(line),
            );
        }
        assert!(diagnostics.is_empty());
        assert!(perf.is_empty());
        ledger.add(Lane::Preview, 50_000_001, 1, None);
        clock.tick(
            120_000,
            true,
            &ledger,
            || true,
            |line| diagnostics.push(line),
            |line| perf.push(line),
        );
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("with input"));
        assert!(perf[0].contains("preview_bytes=50000001"));
        clock.tick(
            180_000,
            true,
            &ledger,
            || false,
            |line| diagnostics.push(line),
            |line| perf.push(line),
        );
        assert!(diagnostics[1].contains("ended after 1 min"));
        assert!(perf[1].contains("preview_bytes=0"));
    }

    #[test]
    fn file_reads_input_excludes_background_window_news() {
        assert!(!is_user_input(&WindowEvent::RedrawRequested));
        assert!(!is_user_input(&WindowEvent::Focused(true)));
        assert!(!is_user_input(&WindowEvent::Ime(Ime::Enabled)));
        assert!(is_user_input(&WindowEvent::Ime(Ime::Commit("text".into()))));
        assert!(is_user_input(&WindowEvent::DroppedFile(
            "example.txt".into()
        )));
    }
}
