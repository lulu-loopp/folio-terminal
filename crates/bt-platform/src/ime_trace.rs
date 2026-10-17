//! Opt-in IME diagnostics. The application owns the file and its writer queue.
//!
//! **On only when the program's entry installs a sink** (T-DIAGNOSTIC-DOORS-AT-THE-ENTRY): the
//! entry reads `BT_IME_TRACE` once and installs one when it names a file; this crate reads no
//! environment variable, so a test binary that installs nothing traces nothing.
use std::sync::OnceLock;

static SINK: OnceLock<fn(String)> = OnceLock::new();

/// Whether the trace is on: whether a sink was installed.
pub fn enabled() -> bool {
    SINK.get().is_some()
}

/// Turn the trace on, into `sink`. Called by the entry when `BT_IME_TRACE` names a file, before
/// constructing windows, and by nothing else. No platform code opens a second file.
pub fn install(sink: fn(String)) {
    let _ = SINK.set(sink);
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
    use bt_source::ItemQuery;

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
        let cancel = crate::source_test::body_in_reaching_variant(
            &ItemQuery::function("cancel_composition"),
            3,
            "windows",
        );
        assert!(cancel.contains("ime_trace::notify_line(reason, \"call\", None)"));
        assert!(cancel.contains("ime_trace::notify_line(reason, \"return\", Some(told))"));
        for action in ["update", "destroy"] {
            let method = crate::source_test::body(
                &ItemQuery::method("ImeSystemCaret", action).in_module("crate::windows_impl"),
            );
            assert!(method.contains("ime_trace::caret_line"), "{action}");
        }
    }
}
