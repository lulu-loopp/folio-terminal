//! **An activity this process holds, so that macOS does not nap it** (0.4.10 plan row 87, part
//! 87c; T-TRAY-87C-MACOS; the coordinator's R14 of 2026-11-12 and R6, R7 of 2026-11-20).
//!
//! App Nap slows an application none of whose windows is on the glass: its timers are coalesced,
//! its I/O and its threads lowered. A run whose last window is put away has nothing on the glass,
//! and its shells and agents are still writing into their PTYs; napped, the drains that empty
//! those PTYs run late, and a program that writes faster than a late drain reads blocks on a full
//! pipe. `bt-app` holds one [`ProcessActivity`] while a window is put away and work runs in it,
//! and lets it go when the window comes back or the work ends.
//!
//! **`NSActivityUserInitiatedAllowingIdleSystemSleep`** (R6): the options that keep App Nap off
//! without holding the Mac awake. An idle Mac may still sleep, as it may with a window shown; the
//! plain `UserInitiated` would also hold the system awake while an agent works, which a shown
//! window does not do.
//!
//! **Elsewhere there is nothing to hold**: App Nap is macOS's, and [`ProcessActivity::begin_holding`]
//! answers a token that holds nothing.

/// **The activity, held for as long as this value lives**: `-[NSProcessInfo
/// beginActivityWithOptions:reason:]`'s token on macOS, ended by `endActivity:` when it is dropped.
pub struct ProcessActivity {
    #[cfg(target_os = "macos")]
    token:
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>,
}

impl ProcessActivity {
    /// **Begin the activity**, `reason` the sentence the system's own tools show for it (Activity
    /// Monitor, `pmset -g assertions`); it is not shown in Folio.
    ///
    /// # Errors
    ///
    /// None today: `beginActivityWithOptions:reason:` answers a token and has no refusal. The
    /// `Result` is the door's shape, so that a platform whose activity can be refused says so.
    pub fn begin_holding(reason: &str) -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        {
            use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
            let token = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
                NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
                &NSString::from_str(reason),
            );
            Ok(Self { token })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = reason;
            Ok(Self {})
        }
    }
}

impl Drop for ProcessActivity {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        {
            // SAFETY: the token is the one `beginActivityWithOptions:reason:` answered, which is
            // the type `endActivity:` takes, and it is ended once: this value owns it.
            unsafe {
                objc2_foundation::NSProcessInfo::processInfo().endActivity(&self.token);
            }
        }
    }
}

impl std::fmt::Debug for ProcessActivity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProcessActivity")
    }
}

#[cfg(test)]
mod tests {
    use super::ProcessActivity;

    /// RED (0.4.10 plan row 87, part 87c) — **the activity begins and ends**: begun, it answers a
    /// token; dropped, it is ended (on macOS `endActivity:`, which would raise on a token it did
    /// not hand out); and a second one begins after the first has gone.
    #[test]
    fn the_activity_begins_and_ends() {
        let first = ProcessActivity::begin_holding("Folio's test of its activity").expect("begun");
        drop(first);
        let second =
            ProcessActivity::begin_holding("Folio's test of its activity").expect("begun again");
        drop(second);
    }
}
