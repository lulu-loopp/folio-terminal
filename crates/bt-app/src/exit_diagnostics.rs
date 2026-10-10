//! Diagnostics for a pane's shell ending and for the run's last window closing.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// The most error text a last-window diagnostic retains, including its trailing ellipsis.
const MAX_CONTROLLED_FAILURE_CHARS: usize = 240;

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
    one_line(&format!(
        "Folio: pane {} shell exited code={code} after {} ms ({}, tab {}, {})",
        exit.seat,
        exit.elapsed.as_millis(),
        program_basename(exit.program.as_deref()),
        exit.tab,
        exit.disposition.words()
    ))
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

fn starts_absolute_path(text: &str, at: usize) -> bool {
    let bytes = text.as_bytes();
    let boundary = at == 0
        || bytes[at - 1].is_ascii_whitespace()
        || matches!(
            bytes[at - 1],
            b'(' | b'[' | b'{' | b'<' | b'"' | b'\'' | b'=' | b':'
        );
    if !boundary {
        return false;
    }
    bytes[at] == b'/'
        || (bytes[at].is_ascii_alphabetic()
            && bytes.get(at + 1) == Some(&b':')
            && matches!(bytes.get(at + 2), Some(b'/' | b'\\')))
}

fn path_end(text: &str, start: usize) -> usize {
    let quote = start
        .checked_sub(1)
        .and_then(|before| match text.as_bytes()[before] {
            b'"' => Some('"'),
            b'\'' => Some('\''),
            _ => None,
        });
    for (offset, character) in text[start..].char_indices() {
        if offset == 0 {
            continue;
        }
        let ends = quote.map_or_else(
            || {
                character.is_whitespace()
                    || matches!(character, '"' | '\'' | ')' | ']' | '}' | '>' | ',' | ';')
            },
            |quote| character == quote,
        );
        if ends {
            return start + offset;
        }
    }
    text.len()
}

/// Reduce drive-rooted and POSIX-rooted paths without asking the host OS to parse the other OS's
/// spelling. Quotes allow a path containing spaces to remain one lexical path.
fn reduce_absolute_paths(text: &str) -> String {
    let mut reduced = String::with_capacity(text.len());
    let mut cursor = 0;
    while cursor < text.len() {
        if starts_absolute_path(text, cursor) {
            let end = path_end(text, cursor);
            let path = &text[cursor..end];
            let basename = path
                .rfind(['/', '\\'])
                .map_or(path, |separator| &path[separator + 1..]);
            reduced.push_str(if basename.is_empty() {
                "<root>"
            } else {
                basename
            });
            cursor = end;
        } else {
            let character = text[cursor..]
                .chars()
                .next()
                .expect("cursor is before the end of the text");
            reduced.push(character);
            cursor += character.len_utf8();
        }
    }
    reduced
}

fn cap_controlled_failure(text: &str) -> String {
    let mut characters = text.chars();
    let mut capped: String = characters
        .by_ref()
        .take(MAX_CONTROLLED_FAILURE_CHARS)
        .collect();
    if characters.next().is_some() {
        capped.pop();
        capped.push('…');
    }
    capped
}

fn controlled_failure_text(text: &str) -> String {
    cap_controlled_failure(&reduce_absolute_paths(&one_line(text)))
}

/// Spend the one-shot cause attached to a native close request.
pub(crate) fn take_last_window_cause(
    shell_exit_close_requested: &mut bool,
) -> LastWindowCause<'static> {
    if std::mem::take(shell_exit_close_requested) {
        LastWindowCause::EveryPaneShellExited
    } else {
        LastWindowCause::PersonClosedIt
    }
}

fn last_window_closed_line(cause: LastWindowCause<'_>) -> String {
    let why = match cause {
        LastWindowCause::EveryPaneShellExited => "every pane's shell exited".to_owned(),
        LastWindowCause::PersonClosedIt => "the person closed it".to_owned(),
        LastWindowCause::Quit => "quit".to_owned(),
        LastWindowCause::LaunchWireQuit => "launch wire quit".to_owned(),
        LastWindowCause::UpdateRestart => "update restart".to_owned(),
        LastWindowCause::ControlledFailure(what) => {
            format!("controlled failure ({})", controlled_failure_text(what))
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

    /// MUTATION (observed RED): skip basename redaction and print the whole program path.
    #[test]
    fn a_shell_line_contains_no_environment_value_or_full_program_path() {
        let planted_environment_value = "EXIT_DIAGNOSTIC_SECRET_493";
        let folder = PathBuf::from(planted_environment_value)
            .join("private-folder-with-a-name-that-must-not-be-logged");
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

    /// MUTATION (observed RED): skip `one_line` around the completed shell-exit line.
    #[test]
    fn a_shell_basename_with_a_newline_still_makes_one_diagnostic_line() {
        let exit = ShellExit {
            seat: 13,
            code: Some(7),
            elapsed: Duration::from_millis(29),
            program: Some(PathBuf::from("外壳\n程序")),
            tab: 17,
            disposition: ShellExitDisposition::TabRetired,
        };
        let line = lines_for(&[exit]).pop().expect("the exit line");
        assert_eq!(
            line,
            "Folio: pane 13 shell exited code=7 after 29 ms (外壳 程序, tab 17, tab retired)"
        );
        assert!(!line.contains(['\r', '\n']), "{line:?}");
    }

    /// MUTATION (observed RED): leave the shell-exit close latch raised when taking its cause.
    #[test]
    fn a_summon_refused_close_spends_its_cause_before_a_later_person_close() {
        let mut shell_exit_close_requested = true;
        assert_eq!(
            take_last_window_cause(&mut shell_exit_close_requested),
            LastWindowCause::EveryPaneShellExited,
            "the refused attempt owns the shell-exit cause"
        );
        assert_eq!(
            take_last_window_cause(&mut shell_exit_close_requested),
            LastWindowCause::PersonClosedIt,
            "a later hand-close is not assigned the refused attempt's cause"
        );
    }

    /// MUTATION (observed RED): keep the flattened error without reducing its absolute paths.
    #[test]
    fn a_controlled_failure_reduces_every_absolute_path_and_caps_its_error() {
        let planted_windows_folder = "C:\\planted-private\\机密";
        let planted_posix_folder = "/planted-private/秘密";
        let long_tail = "界".repeat(MAX_CONTROLLED_FAILURE_CHARS);
        let error = format!(
            "open {planted_windows_folder}\\failure.bin and {planted_posix_folder}/other.log\n{long_tail}"
        );
        let reduced = controlled_failure_text(&error);
        let line = last_window_line(LastWindowCause::ControlledFailure(&error));

        assert!(line.contains("open failure.bin and other.log "), "{line}");
        assert!(!line.contains(planted_windows_folder), "{line}");
        assert!(!line.contains(planted_posix_folder), "{line}");
        assert!(!line.contains(['\r', '\n']), "{line:?}");
        assert_eq!(reduced.chars().count(), MAX_CONTROLLED_FAILURE_CHARS);
        assert!(reduced.ends_with('…'), "{reduced}");
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
