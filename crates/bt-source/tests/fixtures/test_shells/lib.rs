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
