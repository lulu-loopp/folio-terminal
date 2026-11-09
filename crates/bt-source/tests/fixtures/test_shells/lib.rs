//! The `test_shells` guard's fixture: each shape it refuses, and each it allows.

use std::process::Command;

pub fn product_starts_a_pane() {
    let _ = PtySession::spawn(command(), size(), wake());
}

#[cfg(test)]
mod tests {
    use bt_platform::quiet_command as hush;
    use bt_pty::PtySession as Session;
    use std::process::Command as ProcessCommand;

    #[test]
    fn a_raw_pseudoconsole_spawn() {
        let _ = PtySession::spawn(command(), size(), wake());
    }

    #[test]
    fn a_door_handed_on_as_a_value() {
        let _ = Some(size()).map(PtySession::spawn_default);
    }

    #[test]
    fn a_powershell_off_a_pseudoconsole() {
        let _ = Command::new("powershell.exe").arg("-Command").output();
    }

    #[test]
    fn a_command_interpreter_by_its_variable() {
        let _ = Command::new(std::env::var_os("ComSpec").unwrap()).output();
    }

    #[test]
    fn a_git_bash_through_the_quiet_door() {
        let _ = bt_platform::quiet_command(&git_bash()).output();
    }

    #[test]
    fn aliases_do_not_hide_shell_starts() {
        use bt_platform as platform;
        use bt_pty as pty;
        use std::process as proc;
        let _ = ProcessCommand::new("pwsh").output();
        let _ = hush(&git_bash()).output();
        let _ = Session::spawn(command(), size(), wake());
        let _ = proc::Command::new("zsh").output();
        let _ = platform::quiet_command(&git_bash()).output();
        let _ = pty::PtySession::spawn(command(), size(), wake());
    }

    #[test]
    fn allowed_through_the_helper_and_for_programs_that_are_not_shells() {
        let hygiene = Hygiene::new();
        let _ = TestShell::spawn(command(), size());
        let _ = hygiene.command("powershell.exe", Command::new).output();
        let _ = Command::new("node").arg("--version").output();
        let _ = Command::new(std::env::current_exe().unwrap()).output();
        let named = "powershell.exe";
    }
}

/// The method door, defined where the fixture says its home is, and called from product code.
impl BirthFallback {
    pub fn spawn_last_resort(self) -> Session {
        self.spawn_through()
    }
}

pub fn the_reaper_starts_a_fallback(fallback: BirthFallback) {
    let _ = fallback.spawn_last_resort();
}

/// A second function of the door's name: its calls would read as the door's.
impl Elsewhere {
    fn spawn_last_resort(&self) {}
}

#[cfg(test)]
mod birth_fallbacks {
    #[test]
    fn a_birth_fallback_through_the_product_road() {
        let _ = fallback().spawn_last_resort();
        let _ = BirthFallback::spawn_last_resort(fallback());
        let _ = fallback()
            .spawn_last_resort();
    }

    #[test]
    fn allowed_through_the_door_and_for_other_spawns() {
        let _ = TestShell::spawn_birth_fallback(fallback(), columns(), rows(), wake(), why());
        let _ = std::thread::spawn(|| ());
        let _ = fallback().spawn();
    }
}

/// A free function of the door's name: its calls would read as the door's too.
pub fn spawn_last_resort() {}

/// The product's shell birth, its retry the person's (plan row 182): product code names both.
pub fn create_leaf_session(spec: ShellSpec) {
    let _ = pty_door::request_shell(spec, wake());
    let _ = bear(ctx(), turns(), spec, Retry::ThePersons, wake());
}

#[cfg(test)]
mod product_retries {
    #[test]
    fn a_birth_with_the_persons_retry() {
        let _ = crate::pty_door::request_shell(spec(), wake());
        let _ = bear(ctx(), turns(), spec(), Retry::ThePersons, wake());
    }

    #[test]
    fn allowed_through_the_tests_retry() {
        let _ = crate::pty_door::request_shell_in(turns(), spec(), wake());
        let _ = bear(ctx(), turns(), spec(), Retry::OneShotForATest, wake());
    }

    #[test]
    fn a_birth_through_a_grouped_import() {
        use crate::pty_door::{ShellSpec, request_shell};
        let _ = request_shell(spec(), wake());
    }

    #[test]
    fn a_birth_through_the_parent_module() {
        let _ = super::request_shell(spec(), wake());
    }

    #[test]
    fn the_persons_retry_through_a_grouped_import() {
        use super::Retry::{OneShotForATest, ThePersons};
        let _ = bear(ctx(), turns(), spec(), ThePersons, wake());
    }

    #[test]
    fn the_persons_retry_through_a_glob_import() {
        use super::Retry::*;
        let _ = bear(ctx(), turns(), spec(), ThePersons, wake());
    }
}

/// A test-build door that gives a test's birth the person's retry: test code too.
#[cfg(test)]
pub(crate) fn request_shell_in(spec: ShellSpec) {
    let _ = bear(ctx(), turns(), spec, Retry::ThePersons, wake());
}
