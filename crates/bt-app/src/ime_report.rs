//! Instrumentation only. The decision consumes booleans and native facts; it
//! cannot retain text, route a key, or change IME/focus state.

pub use bt_platform::ime_observation::NativeFacts;
use std::sync::OnceLock;
use std::time::Instant;

pub static TRACE: crate::trace::Dump = crate::trace::Dump::new("BT_IME_TRACE");

pub fn now_ms() -> u64 {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed().as_millis() as u64
}

#[derive(Clone, Copy, Debug)]
struct Stamp {
    order: u32,
    ms: u64,
}

#[derive(Clone, Copy)]
pub enum ImeKind {
    Enabled,
    Preedit,
    Commit,
    Disabled,
}

#[derive(Default)]
pub struct Report {
    order: u32,
    created: Option<Stamp>,
    shown: Option<Stamp>,
    allowed: Option<(bool, Stamp)>,
    first_focus: Option<Stamp>,
    first_enabled: Option<Stamp>,
    first_key: Option<Stamp>,
    focus_at: Option<Stamp>,
    focused: bool,
    enabled_since_focus: bool,
    ime_since_focus: bool,
    latin_keys: u8,
    reported: bool,
    probed_at: Option<u64>,
}

/// The least time between two native readings inside one focus.
pub const PROBE_MIN_INTERVAL_MS: u64 = 10_000;

impl Report {
    fn stamp(&mut self, ms: u64) -> Stamp {
        self.order += 1;
        Stamp {
            order: self.order,
            ms,
        }
    }
    pub fn created(&mut self, ms: u64) {
        self.created = Some(self.stamp(ms));
    }
    pub fn shown(&mut self, ms: u64) {
        if self.shown.is_none() {
            self.shown = Some(self.stamp(ms));
        }
    }
    pub fn allowed(&mut self, value: bool, ms: u64) {
        self.allowed = Some((value, self.stamp(ms)));
    }
    pub fn first_key(&mut self, ms: u64) {
        if self.first_key.is_none() {
            self.first_key = Some(self.stamp(ms));
        }
    }
    pub fn watching_keys(&self) -> bool {
        self.focused && !self.ime_since_focus && !self.reported
    }
    pub fn trace_order(&self, window: impl std::fmt::Debug, station: &str) {
        TRACE.line(|| format!("IME startup window={window:?} station={station} created={:?} shown={:?} allowed={:?} first_focus={:?} first_enabled={:?} first_key={:?}",
            self.created, self.shown, self.allowed, self.first_focus, self.first_enabled, self.first_key));
    }
    pub fn has_first_key(&self) -> bool {
        self.first_key.is_some()
    }
    pub fn focus(&mut self, focused: bool, ms: u64) {
        if self.focused == focused {
            return;
        }
        self.focused = focused;
        self.latin_keys = 0;
        if focused {
            self.enabled_since_focus = false;
            self.ime_since_focus = false;
            self.reported = false;
            self.probed_at = None;
            let stamp = self.stamp(ms);
            self.first_focus.get_or_insert(stamp);
            self.focus_at = Some(stamp);
        }
    }
    pub fn ime(&mut self, kind: ImeKind, ms: u64) {
        if matches!(kind, ImeKind::Enabled) {
            if self.first_enabled.is_none() {
                self.first_enabled = Some(self.stamp(ms));
            }
            self.enabled_since_focus |= self.focused;
        }
        self.ime_since_focus |= self.focused;
        self.latin_keys = 0;
    }
    /// True exactly at the third consecutive qualifying press. Saturating at
    /// four avoids native reads for every subsequent key in the same streak.
    pub fn key(&mut self, terminal: bool, latin: bool) -> bool {
        if !self.focused || !terminal || !latin || self.ime_since_focus || self.reported {
            self.latin_keys = 0;
            return false;
        }
        self.latin_keys = self.latin_keys.saturating_add(1).min(4);
        self.latin_keys == 3
    }
    /// Whether the native reading may be taken now, and if so, that it was.
    ///
    /// **The budget on the probe itself** (closure review, 2026-09-20). The
    /// streak alone bounds nothing: a reading that does not confirm leaves the
    /// report unlatched, so every later word reaches the threshold again, and a
    /// machine with no input method at all would pay a COM activation per word
    /// for the life of the focus. The first streak of a focus is always read;
    /// after that, one reading per [`PROBE_MIN_INTERVAL_MS`].
    pub fn may_probe(&mut self, ms: u64) -> bool {
        if self
            .probed_at
            .is_some_and(|at| ms.saturating_sub(at) < PROBE_MIN_INTERVAL_MS)
        {
            return false;
        }
        self.probed_at = Some(ms);
        true
    }
    pub fn confirm(&mut self, facts: NativeFacts) -> bool {
        let report = self.focused
            && !self.ime_since_focus
            && !self.reported
            && self.latin_keys == 3
            && facts.composing_mode();
        self.reported |= report;
        report
    }
    pub fn line(
        &self,
        reason: &str,
        ms: u64,
        terminal: bool,
        web_host: bool,
        facts: NativeFacts,
    ) -> String {
        let headline = if reason == "plain-text" {
            "Folio: keys are arriving as plain text while an input method is active and has not engaged since focus"
        } else {
            "Folio: IME observation"
        };
        let stamp = |value: Option<Stamp>| {
            value.map_or_else(
                || "unknown".to_owned(),
                |s| format!("{}@{}ms", s.order, s.ms),
            )
        };
        let allowed = self.allowed.map_or_else(
            || "unknown".to_owned(),
            |(v, s)| format!("{v}@{}", stamp(Some(s))),
        );
        format!(
            "{headline} — reason={reason} at_ms={ms} focused={} terminal={terminal} web_host={web_host} allowed={allowed} enabled_since_focus={} ime_since_focus={} latin_keys={} created={} shown={} first_focus={} focus={} first_enabled={} first_key={} native={facts:?} native_none=unknown mode_source=imm-compat",
            self.focused,
            self.enabled_since_focus,
            self.ime_since_focus,
            self.latin_keys,
            stamp(self.created),
            stamp(self.shown),
            stamp(self.first_focus),
            stamp(self.focus_at),
            stamp(self.first_enabled),
            stamp(self.first_key)
        )
    }
}

/// ASCII Latin letters (possibly with printable ASCII punctuation/spaces).
/// No text leaves this classifier. Controls and numeric-only packets do not count.
pub fn printable_latin(text: Option<&str>) -> bool {
    text.is_some_and(|text| {
        text.bytes().any(|b| b.is_ascii_alphabetic())
            && text.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
    })
}

#[cfg(test)]
#[path = "ime_report_tests.rs"]
mod tests;
