//! **Where this crate's `stderr` lines go** (T-TRACE-SINK-OFF-THE-CALLING-THREAD, 0.4.9 plan
//! row 51).
//!
//! Every line this crate would have written to standard error is handed to the road the
//! application installs ([`install`]): `bt-app`'s `main` installs `diagnostics::stderr_line`
//! before anything else it does, and that function hands the line to whichever writer owns the
//! run's `stderr` — `diagnostics.log`'s, or the console's — so no thread here writes a stream, and
//! a request door that says something performs no write of its own (the side-effects contract's
//! C1). A process that installs nothing — a test binary, a tool — writes nothing.
//!
//! The installed-road shape of [`crate::ime_trace`] and [`crate::door_skipped`]: this crate
//! cannot name `bt-app`, which depends on it.

use std::sync::OnceLock;

/// The road [`install`] set, read by [`say`].
static ROAD: OnceLock<fn(String)> = OnceLock::new();

/// **Where this crate's lines go from here on.** Called once, by the program's entry; a second
/// call is ignored, as the process has one answer.
pub fn install(road: fn(String)) {
    let _ = ROAD.set(road);
}

/// One line for this run's `stderr`, handed to the installed road — or, in a process that
/// installed none, to nobody. Never a write on this thread.
pub fn say(line: String) {
    if let Some(road) = ROAD.get() {
        road(line);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// RED (mutation: [`say`] writing the line itself, the `eprintln!` its callers had, in place
    /// of handing it on) — **a line goes to the installed road, in order.** The only test that
    /// touches [`ROAD`]: it is process-wide and set once.
    #[test]
    fn a_line_goes_to_the_installed_road() {
        static SAID: Mutex<Vec<String>> = Mutex::new(Vec::new());
        install(|line| {
            SAID.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(line);
        });
        say("first".to_owned());
        say("second".to_owned());
        let said = SAID
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(said, ["first", "second"]);
    }
}
