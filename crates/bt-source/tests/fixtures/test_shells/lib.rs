//! The `test_shells` guard's fixture: each shape it refuses, and each it allows.

use std::process::Command;

pub fn product_starts_a_pane() {
    let _ = PtySession::spawn(command(), size(), wake());
}

#[cfg(test)]
mod tests {
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
    fn allowed_through_the_helper_and_for_programs_that_are_not_shells() {
        let hygiene = Hygiene::new();
        let _ = TestShell::spawn(command(), size());
        let _ = hygiene.command("powershell.exe", Command::new).output();
        let _ = Command::new("node").arg("--version").output();
        let _ = Command::new(std::env::current_exe().unwrap()).output();
        let named = "powershell.exe";
    }
}
