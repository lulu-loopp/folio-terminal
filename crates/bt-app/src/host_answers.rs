//! **What this program answers for `bt-term`, said once, before the first session**
//! (`docs/ARCHITECTURE.md` §3.2).
//!
//! `bt-term` builds without the platform layer, so the two facts about the machine a terminal
//! session needs are installed into it by the host: the names this machine answers to, which a
//! `file://<host>/` working-directory report is compared against, and what every thread of its
//! image resample pool runs first. [`install`] gives both, and `main` calls it before the event
//! loop exists — every session this process makes is made by that loop. The third answer, the
//! name a hand-off door would open, is handed to `bt_term::verify_path` by the path-verification
//! lane, its one product caller.

/// Install this machine's answers into `bt-term`. Called once, by `main`, before the event loop.
pub(crate) fn install() {
    bt_term::install_host_names(this_machines_names());
    bt_term::install_pool_thread_start(enter_the_band_below_normal);
}

/// Every spelling of this machine's name a shell on it may put in a `file://<host>/` report —
/// the operating system's answer (`bt_platform::host_names`), never an environment variable.
pub(crate) fn this_machines_names() -> Vec<String> {
    bt_platform::host_names()
}

/// What a resample-pool thread runs first: the band below normal, the band every worker of this
/// process runs in, so a wallpaper-sized pass cannot take the window thread's cores.
fn enter_the_band_below_normal() {
    bt_platform::set_current_thread_priority(bt_platform::ThreadPriority::BelowNormal);
}

#[cfg(test)]
mod tests {
    use crate::test_support::{free_fn_body, squeezed};

    /// RED (B-AUDIT-046 TRM-3, moved here by CC-4) — **the names this program installs are the
    /// machine's own, and a working-directory report naming the machine is this machine's
    /// directory.**
    ///
    /// `bt-term`'s own pin
    /// (`session::tests::a_cwd_message_naming_this_host_is_accepted_and_a_foreign_one_ignored`)
    /// runs on the names its tests install; this is the half that is the host's: the operating
    /// system's name — the real producer, not a name handed in by the test — installed and then
    /// accepted from fish's own report.
    ///
    /// MUTATION: make `this_machines_names` answer the `COMPUTERNAME` variable — red on macOS and
    /// Linux, where it is unset; make it answer nothing — red everywhere.
    #[test]
    fn the_names_installed_are_the_machines_and_a_report_naming_it_is_accepted() {
        let names = super::this_machines_names();
        let host = bt_platform::host_names()
            .into_iter()
            .next()
            .expect("this machine has a name to ask for");
        assert!(
            names.iter().any(|name| name.eq_ignore_ascii_case(&host)),
            "the installed names hold the operating system's {host:?}: {names:?}"
        );
        bt_term::install_host_names(names);
        let directory = std::env::temp_dir();
        let bare = bt_transcript::paths::local_path_to_file_uri(&directory);
        let (_, path) = bare
            .split_once("://")
            .expect("a local file URI opens with the scheme and an empty authority");
        let mut session = bt_term::DualPlaneSession::new(
            std::num::NonZeroU32::new(80).unwrap(),
            std::num::NonZeroU32::new(24).unwrap(),
        );
        session
            .feed(format!("\x1b]7;file://{host}{path}\x07").as_bytes())
            .unwrap();
        assert_eq!(
            session.working_directory(),
            bt_term::file_uri_to_local_path(&bare, &[]).as_deref(),
            "fish's report naming this host as {host:?} is this machine's directory"
        );
    }

    /// Whether `main`'s body installs the host answers before anything in it could make a session:
    /// before the event loop is built (every session is made by that loop's turns) and before any
    /// session is named. `Err` names the violation.
    ///
    /// GUARD: its subject is how `main` is written (CONVENTIONS, "A test pins behaviour by running
    /// it"); `main` cannot run in a test, and the order of its statements is the fact.
    fn installs_before_the_first_session(main_body: &str) -> Result<(), String> {
        let body = squeezed(main_body);
        let install = body
            .find("host_answers::install();")
            .ok_or("`main` never installs the host answers")?;
        let first_session = ["EventLoop::", "DualPlaneSession", "create_leaf_session("]
            .iter()
            .filter_map(|spelling| body.find(spelling).map(|at| (at, *spelling)))
            .min()
            .ok_or("`main` builds no event loop, so this guard reads the wrong function")?;
        if first_session.0 < install {
            return Err(format!(
                "`main` reaches `{}` before it installs the host answers",
                first_session.1
            ));
        }
        Ok(())
    }

    /// GUARD (CC-4) — **`main` installs the host answers before the first session can exist.**
    ///
    /// A session constructed before the install reads no host names and panics on the first
    /// working-directory report it is sent.
    ///
    /// MUTATION: move `host_answers::install();` below `EventLoop::<AppEvent>::with_user_event()`
    /// in `main`, and this goes red.
    #[test]
    fn the_host_answers_are_installed_before_the_first_session_can_exist() {
        installs_before_the_first_session(free_fn_body("main")).unwrap();
    }

    /// GUARD (CC-4) — **the guard above reads each violation it exists for**: an install after the
    /// loop, an install after a session is named, and no install at all; and it passes the order
    /// `main` has.
    ///
    /// MUTATION: drop `DualPlaneSession` from the guard's list and the second planted body passes.
    #[test]
    fn the_install_guard_names_an_install_after_the_first_session() {
        let ordered = "{ enter(); host_answers::install(); let b = EventLoop::<E>::new(); }";
        assert_eq!(installs_before_the_first_session(ordered), Ok(()));
        for (planted, reached) in [
            (
                "{ let b = EventLoop::<E>::new(); host_answers::install(); }",
                "EventLoop::",
            ),
            (
                "{ let s = DualPlaneSession::new(c, r); host_answers::install(); \
                 let b = EventLoop::<E>::new(); }",
                "DualPlaneSession",
            ),
        ] {
            assert_eq!(
                installs_before_the_first_session(planted),
                Err(format!(
                    "`main` reaches `{reached}` before it installs the host answers"
                )),
                "{planted}"
            );
        }
        assert_eq!(
            installs_before_the_first_session("{ let b = EventLoop::<E>::new(); }"),
            Err("`main` never installs the host answers".to_owned())
        );
    }
}
