//! One line of evidence, one line of JSON.
//!
//! Everything this probe learns leaves through here so that a gate's verdict and
//! the reading behind it are the same artefact: `W0 {json}` on stdout, and the
//! identical line appended to `$BT_W0_LOG` when the caller names a file.

use std::cell::RefCell;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::time::Instant;

thread_local! {
    static START: RefCell<Option<Instant>> = const { RefCell::new(None) };
}

fn elapsed_ms() -> u128 {
    START.with(|cell| {
        let mut slot = cell.borrow_mut();
        let start = slot.get_or_insert_with(Instant::now);
        start.elapsed().as_millis()
    })
}

/// Emit one evidence record. `fields` must be a JSON object.
pub fn emit(gate: u32, event: &str, fields: serde_json::Value) {
    let mut object = serde_json::Map::new();
    object.insert("gate".into(), gate.into());
    object.insert("t_ms".into(), (elapsed_ms() as u64).into());
    object.insert("event".into(), event.into());
    if let serde_json::Value::Object(extra) = fields {
        for (key, value) in extra {
            object.insert(key, value);
        }
    }
    let line = format!("W0 {}", serde_json::Value::Object(object));
    println!("{line}");
    let _ = std::io::stdout().flush();
    if let Ok(path) = std::env::var("BT_W0_LOG")
        && let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// A gate's verdict. `pass`, `fail` and `blocked` are the only three words the
/// evidence document is allowed to copy out of this probe.
pub fn verdict(gate: u32, verdict: &str, note: &str) {
    emit(
        gate,
        "verdict",
        serde_json::json!({ "verdict": verdict, "note": note }),
    );
}

/// Free-form progress that is not itself evidence.
pub fn note(gate: u32, text: &str) {
    emit(gate, "note", serde_json::json!({ "text": text }));
}
