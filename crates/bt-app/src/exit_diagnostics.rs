//! Diagnostics for a pane's shell ending and for the run's last window closing.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// What the shell's exit made the reaper retire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShellExitDisposition {
    PaneRetired,
    TabKept,
    TabRetired,
}

impl ShellExitDisposition {
    const fn words(self) -> &'static str {
        match self {
            Self::PaneRetired => "retired pane",
            Self::TabKept => "tab kept",
            Self::TabRetired => "tab retired",
        }
    }
}

/// The facts the reaper learned about one shell exit.
pub(crate) struct ShellExit {
    pub(crate) seat: u64,
    pub(crate) code: Option<u32>,
    pub(crate) elapsed: Duration,
    pub(crate) program: Option<PathBuf>,
    pub(crate) tab: u64,
    pub(crate) disposition: ShellExitDisposition,
}

fn program_basename(program: Option<&Path>) -> String {
    program
        .and_then(Path::file_name)
        .filter(|name| !name.is_empty())
        .map_or_else(
            || "<unknown>".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        )
}

fn shell_exit_line(exit: &ShellExit) -> String {
    let code = exit
        .code
        .map_or_else(|| "none".to_owned(), |code| code.to_string());
    format!(
        "Folio: pane {} shell exited code={code} after {} ms ({}, tab {}, {})",
        exit.seat,
        exit.elapsed.as_millis(),
        program_basename(exit.program.as_deref()),
        exit.tab,
        exit.disposition.words()
    )
}

/// Say each exit exactly once through the resident diagnostics road.
pub(crate) fn say_shell_exits<'a>(
    exits: impl IntoIterator<Item = &'a ShellExit>,
    mut note: impl FnMut(&str),
) {
    for exit in exits {
        note(&shell_exit_line(exit));
    }
}

/// Why the run's last ordinary window closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LastWindowCause<'a> {
    EveryPaneShellExited,
    PersonClosedIt,
    Quit,
    LaunchWireQuit,
    UpdateRestart,
    ControlledFailure(&'a str),
    UnknownRoad(&'static str),
}

fn one_line(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            '\r' | '\n' => ' ',
            other => other,
        })
        .collect()
}

fn last_window_closed_line(cause: LastWindowCause<'_>) -> String {
    let why = match cause {
        LastWindowCause::EveryPaneShellExited => "every pane's shell exited".to_owned(),
        LastWindowCause::PersonClosedIt => "the person closed it".to_owned(),
        LastWindowCause::Quit => "quit".to_owned(),
        LastWindowCause::LaunchWireQuit => "launch wire quit".to_owned(),
        LastWindowCause::UpdateRestart => "update restart".to_owned(),
        LastWindowCause::ControlledFailure(what) => {
            format!("controlled failure ({})", one_line(what))
        }
        LastWindowCause::UnknownRoad(function) => format!("<unknown road: {function}>"),
    };
    format!("Folio: last window closed — {why}")
}

/// Say the run's one last-window line through the resident diagnostics road.
pub(crate) fn say_last_window_closed(cause: LastWindowCause<'_>, mut note: impl FnMut(&str)) {
    note(&last_window_closed_line(cause));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_for(exits: &[ShellExit]) -> Vec<String> {
        let mut lines = Vec::new();
        say_shell_exits(exits, |line| lines.push(line.to_owned()));
        lines
    }

    fn last_window_line(cause: LastWindowCause<'_>) -> String {
        let mut lines = Vec::new();
        say_last_window_closed(cause, |line| lines.push(line.to_owned()));
        assert_eq!(lines.len(), 1, "one run end writes one line");
        lines.pop().expect("the one line")
    }

    /// MUTATION (observed RED): remove the `note` call from `say_shell_exits` (no line).
    #[test]
    fn a_shell_that_exits_says_its_code_basename_and_retirement() {
        let exit = ShellExit {
            seat: 7,
            code: Some(23),
            elapsed: Duration::from_millis(41),
            program: Some(PathBuf::from("shells").join("pwsh.exe")),
            tab: 11,
            disposition: ShellExitDisposition::PaneRetired,
        };
        assert_eq!(
            lines_for(&[exit]),
            ["Folio: pane 7 shell exited code=23 after 41 ms (pwsh.exe, tab 11, retired pane)"]
        );
    }

    /// MUTATION (observed RED): map `EveryPaneShellExited` to `the person closed it`.
    #[test]
    fn the_last_window_after_every_shell_exited_says_that_cause() {
        assert_eq!(
            last_window_line(LastWindowCause::EveryPaneShellExited),
            "Folio: last window closed — every pane's shell exited"
        );
    }

    /// MUTATION (observed RED): map `Quit` to `update restart`.
    #[test]
    fn a_plain_quit_says_quit() {
        assert_eq!(
            last_window_line(LastWindowCause::Quit),
            "Folio: last window closed — quit"
        );
    }

    /// MUTATION (observed RED): print `program.display()` instead of its file name.
    #[test]
    fn a_shell_line_contains_no_environment_value_or_full_program_path() {
        let planted_environment_value = "EXIT_DIAGNOSTIC_SECRET_493";
        let folder = PathBuf::from("private-folder-with-a-name-that-must-not-be-logged");
        let exit = ShellExit {
            seat: 3,
            code: None,
            elapsed: Duration::from_millis(9),
            program: Some(folder.join("shéll")),
            tab: 5,
            disposition: ShellExitDisposition::TabKept,
        };
        let line = lines_for(&[exit]).pop().expect("the exit line");
        assert!(line.contains("(shéll, tab 5, tab kept)"), "{line}");
        assert!(!line.contains(planted_environment_value), "{line}");
        assert!(!line.contains(folder.to_string_lossy().as_ref()), "{line}");
    }

    #[test]
    fn every_run_end_cause_has_the_ruled_words_and_stays_on_one_line() {
        for (cause, words) in [
            (LastWindowCause::PersonClosedIt, "the person closed it"),
            (LastWindowCause::LaunchWireQuit, "launch wire quit"),
            (LastWindowCause::UpdateRestart, "update restart"),
            (
                LastWindowCause::ControlledFailure("renderer failed\nwhile presenting"),
                "controlled failure (renderer failed while presenting)",
            ),
            (
                LastWindowCause::UnknownRoad("mystery"),
                "<unknown road: mystery>",
            ),
        ] {
            let line = last_window_line(cause);
            assert_eq!(line, format!("Folio: last window closed — {words}"));
            assert!(!line.contains(['\r', '\n']), "{line:?}");
        }
    }
}
