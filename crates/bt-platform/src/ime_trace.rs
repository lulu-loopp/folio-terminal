//! Opt-in IME diagnostics. The application owns the file and its writer queue.
use std::sync::OnceLock;

static ENABLED: OnceLock<bool> = OnceLock::new();
static SINK: OnceLock<fn(String)> = OnceLock::new();

pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var_os("BT_IME_TRACE").is_some_and(|v| !v.is_empty()))
}

/// Install before constructing windows. No platform code opens a second file.
pub fn install(sink: fn(String)) {
    if enabled() {
        let _ = SINK.set(sink);
    }
}

pub fn line(message: impl FnOnce() -> String) {
    if enabled()
        && let Some(sink) = SINK.get()
    {
        sink(message());
    }
}

/// All strings here are static call-site labels, never input-method text.
pub fn notify_line(reason: &'static str, phase: &'static str, result: Option<bool>) -> String {
    format!(
        "IME_OUT_NOTIFY notification=NI_COMPOSITIONSTR index=CPS_CANCEL value=0 reason={reason} phase={phase} result={result:?}"
    )
}

pub fn caret_line(
    action: &'static str,
    x: i32,
    y: i32,
    active: bool,
    result: &'static str,
) -> String {
    format!(
        "IME_OUT_NATIVE_CARET action={action} x={x} y={y} width=1 height=1 active={active} result={result}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ime_native_format_values_and_privacy() {
        let secret = "synthetic-private-preedit";
        let lines = [
            notify_line("owner_changed", "return", Some(false)),
            caret_line("update", -4, 9, true, "ok"),
        ];
        assert_eq!(
            lines[0],
            "IME_OUT_NOTIFY notification=NI_COMPOSITIONSTR index=CPS_CANCEL value=0 reason=owner_changed phase=return result=Some(false)"
        );
        assert_eq!(
            lines[1],
            "IME_OUT_NATIVE_CARET action=update x=-4 y=9 width=1 height=1 active=true result=ok"
        );
        for line in lines {
            assert!(!line.contains(secret));
        }
    }

    #[test]
    fn ime_native_outbound_sites_are_traced() {
        let source = include_str!("lib.rs");
        let cancel = source
            .split("pub fn cancel_composition(reason:")
            .nth(1)
            .unwrap()
            .split("fn active_layout_is_chinese")
            .next()
            .unwrap();
        assert!(cancel.contains("ime_trace::notify_line(reason, \"call\", None)"));
        assert!(cancel.contains("ime_trace::notify_line(reason, \"return\", Some(told))"));
        for action in ["update", "destroy"] {
            let method = source
                .split(&format!("pub fn {action}(&mut self"))
                .nth(1)
                .unwrap()
                .split("\n        }\n")
                .next()
                .unwrap();
            assert!(method.contains("ime_trace::caret_line"), "{action}");
        }
    }
}
