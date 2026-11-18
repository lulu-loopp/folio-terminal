//! Diagnostics for a pane's shell ending, for a window closing and for the run's last window
//! closing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bt_platform::close_road::{CloseArrival, CloseRoad, OwnCloseRoad};

/// The most error text a last-window diagnostic retains, including its trailing ellipsis.
const MAX_CONTROLLED_FAILURE_CHARS: usize = 240;

/// What the shell's exit made the reaper retire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShellExitDisposition {
    BirthDeathFellBack(&'static str),
    BirthDeathPaneKept,
    /// The system ended the shell with the session (`SYSTEM_ENDED_EXIT_CODE`, plan row 191): the
    /// pane is kept with its face and nothing is retired.
    SystemEndedPaneKept,
    /// A resumed agent ended by itself and the pane's shell is handed back in its place (plan row
    /// 195): the pane is kept.
    HandedBack,
    PaneRetired,
    TabKept,
    TabRetired,
}

impl ShellExitDisposition {
    fn words(self) -> String {
        match self {
            Self::BirthDeathFellBack(program) => {
                format!(
                    "birth death: fell back to {}",
                    program_basename(Some(Path::new(program)))
                )
            }
            Self::BirthDeathPaneKept => "birth death: pane kept".to_owned(),
            Self::SystemEndedPaneKept => "ended by the system: pane kept".to_owned(),
            Self::HandedBack => "handed back".to_owned(),
            Self::PaneRetired => "retired pane".to_owned(),
            Self::TabKept => "tab kept".to_owned(),
            Self::TabRetired => "tab retired".to_owned(),
        }
    }
}

/// The facts the reaper learned about one shell exit.
#[derive(Debug)]
pub(crate) struct ShellExit {
    pub(crate) seat: u64,
    pub(crate) code: Option<u32>,
    pub(crate) elapsed: Duration,
    pub(crate) program: Option<PathBuf>,
    pub(crate) tab: u64,
    pub(crate) disposition: ShellExitDisposition,
    /// **Whether the system said the session was ending when the reaper judged this exit**
    /// (`bt_platform::session_end::shutting_down`, plan row 199): a probe written on the exit's
    /// line as `shutting_down=0|1`, so that a reboot's log shows whether the system's sign-out
    /// is already set when an exit reaches the reaper before the question reaches the window.
    /// `None` off Windows, where there is no probe and the line has no field.
    ///
    /// **Private to this module** (row 199's review F1), so that no exit can be built outside it
    /// by a struct literal: every product exit goes through [`ShellExit::observed`], which reads
    /// the probe, and a test outside this module through [`ShellExit::unprobed`].
    shutting_down: Option<bool>,
}

impl ShellExit {
    /// **An exit the reaper is judging now**: its facts as given, and the session-end probe read
    /// at this instant — the one a later rule would read it at (plan row 199).
    pub(crate) fn observed(
        seat: u64,
        code: Option<u32>,
        elapsed: Duration,
        program: Option<PathBuf>,
        tab: u64,
        disposition: ShellExitDisposition,
    ) -> Self {
        Self {
            seat,
            code,
            elapsed,
            program,
            tab,
            disposition,
            shutting_down: bt_platform::session_end::shutting_down(),
        }
    }

    /// **An exit a test built, with no probe read**: the line says what it said before row 199.
    #[cfg(test)]
    pub(crate) fn unprobed(
        seat: u64,
        code: Option<u32>,
        elapsed: Duration,
        program: Option<PathBuf>,
        tab: u64,
        disposition: ShellExitDisposition,
    ) -> Self {
        Self {
            seat,
            code,
            elapsed,
            program,
            tab,
            disposition,
            shutting_down: None,
        }
    }
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
    // The probe goes last, so that every reader of the line's start reads it as before.
    let shutting_down = match exit.shutting_down {
        Some(true) => " shutting_down=1",
        Some(false) => " shutting_down=0",
        None => "",
    };
    one_line(&format!(
        "Folio: pane {} shell exited code={code} after {} ms ({}, tab {}, {}){shutting_down}",
        exit.seat,
        exit.elapsed.as_millis(),
        program_basename(exit.program.as_deref()),
        exit.tab,
        exit.disposition.words()
    ))
}

/// Say a requested shell's spawn failure and the last-resort shell that replaced it.
pub(crate) fn say_spawn_fallback(
    seat: u64,
    fallback: &bt_pty::ShellFallback,
    mut note: impl FnMut(&str),
) {
    note(&one_line(&format!(
        "Folio: pane {seat} shell spawn failed: {} ({}); fell back to {}",
        program_basename(Some(Path::new(&fallback.requested))),
        controlled_failure_text(&fallback.error),
        program_basename(Some(Path::new(fallback.started)))
    )));
}

/// Say a pane birth whose program could not be spawned and had no working fallback.
pub(crate) fn say_spawn_failure(
    seat: u64,
    program: Option<&Path>,
    error: &str,
    mut note: impl FnMut(&str),
) {
    note(&one_line(&format!(
        "Folio: pane {seat} shell spawn failed: {} ({})",
        program_basename(program),
        controlled_failure_text(error)
    )));
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

/// Why the run's last ordinary window closed, on the roads that are not a window's own close
/// (that one says itself: [`say_window_closed`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LastWindowCause<'a> {
    Quit,
    LaunchWireQuit,
    UpdateRestart,
    /// The Restart Manager closed Folio (`ENDSESSION_CLOSEAPP`, T-RESTART-REGISTRATION).
    SystemClosedIt,
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

fn last_window_closed_line(cause: LastWindowCause<'_>) -> String {
    let why = match cause {
        LastWindowCause::Quit => "quit".to_owned(),
        LastWindowCause::LaunchWireQuit => "launch wire quit".to_owned(),
        LastWindowCause::UpdateRestart => "update restart".to_owned(),
        LastWindowCause::SystemClosedIt => "the system closed it".to_owned(),
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

/// **How a window's close was asked for** (0.4.10 plan row 103): what the platform saw of the
/// request, and where the pointer was, named by the window's own chrome. Taken when the request
/// arrives (`Runtime::note_close_arrival`) and kept through the close's re-runs — a gate's answer,
/// a wait, the summoned terminal's answer — until the close is performed; every new request
/// replaces it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CloseAsked {
    /// `None` when nothing was witnessed: an unmarked close request.
    pub(crate) arrival: Option<CloseArrival>,
    pub(crate) pointer: PointerAtClose,
}

/// **Where the pointer was when the close arrived**, in the window's own words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PointerAtClose {
    /// Over the window's close button: Folio's caption `×`, or AppKit's on macOS.
    CloseButton,
    /// Over another part of the window's chrome (the caption, the tabs, the rail).
    Chrome,
    /// Over the window, off its chrome.
    Window,
    /// Not over this window.
    Outside,
    /// Not read.
    Unread,
}

impl PointerAtClose {
    const fn pointer_word(self) -> &'static str {
        match self {
            Self::CloseButton => "close-button",
            Self::Chrome => "chrome",
            Self::Window => "window",
            Self::Outside => "outside",
            Self::Unread => "unread",
        }
    }
}

/// **How a window came to close.**
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClosedHow {
    /// A close request reached it.
    Asked(CloseAsked),
    /// Its last tab moved to another window, so nothing is left in it.
    TabMovedOut,
}

/// Which kind of window closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClosedWindow {
    Ordinary,
    /// The summoned terminal.
    Summon,
}

/// **What the window was when it closed**: its kind and id, what it held, and — when the run goes
/// on — how many ordinary windows are left.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ClosedFacts {
    pub(crate) window: ClosedWindow,
    pub(crate) id: u64,
    pub(crate) tabs: usize,
    pub(crate) panes: usize,
    /// `None` when this close ends the run.
    pub(crate) windows_left: Option<usize>,
}

/// The cause's words: the person only for Folio's own caption and final-tab roads (R3).
const fn closed_cause(how: ClosedHow) -> &'static str {
    match how {
        ClosedHow::TabMovedOut => "the last tab moved to another window",
        ClosedHow::Asked(CloseAsked { arrival: None, .. }) => "an unmarked close request",
        ClosedHow::Asked(CloseAsked {
            arrival: Some(arrival),
            ..
        }) => match arrival.road {
            CloseRoad::Own(OwnCloseRoad::CaptionButton | OwnCloseRoad::FinalTab) => {
                "the person closed it"
            }
            CloseRoad::Own(OwnCloseRoad::ShellsExited) => "every pane's shell exited",
            CloseRoad::SystemClose(_) | CloseRoad::CloseButton | CloseRoad::Menu(_) => {
                "the system's close command"
            }
            CloseRoad::CloseMessage | CloseRoad::Outside => "a close message",
        },
    }
}

/// **The one line a window's close writes** (0.4.10 plan row 103): `Folio: last window closed —`
/// when it ends the run, `Folio: window closed —` with the windows left when it does not. Enum
/// words and counts only, on one physical line.
pub(crate) fn window_closed_line(how: ClosedHow, facts: ClosedFacts) -> String {
    let (road, delivery, foreground, pointer) = match how {
        ClosedHow::TabMovedOut => ("tab-moved-out", "unread", "unread", "unread"),
        ClosedHow::Asked(asked) => (
            asked
                .arrival
                .map_or("unmarked", |arrival| arrival.road.road_word()),
            asked
                .arrival
                .and_then(|arrival| arrival.delivery)
                .map_or("unread", |delivery| delivery.delivery_word()),
            match asked.arrival.and_then(|arrival| arrival.foreground) {
                Some(true) => "yes",
                Some(false) => "no",
                None => "unread",
            },
            asked.pointer.pointer_word(),
        ),
    };
    let window = match facts.window {
        ClosedWindow::Ordinary => "ordinary",
        ClosedWindow::Summon => "summon",
    };
    let head = if facts.windows_left.is_some() {
        "Folio: window closed"
    } else {
        "Folio: last window closed"
    };
    let cause = closed_cause(how);
    let (id, tabs, panes) = (facts.id, facts.tabs, facts.panes);
    let mut line = format!(
        "{head} — {cause}; window={window} {id}; road={road}; delivery={delivery}; \
         foreground={foreground}; pointer={pointer}; tabs={tabs}; panes={panes}"
    );
    if let Some(left) = facts.windows_left {
        line.push_str("; windows left=");
        line.push_str(&left.to_string());
    }
    line
}

/// Say a window's one close line through the road the window says its lines on.
pub(crate) fn say_window_closed(how: ClosedHow, facts: ClosedFacts, mut note: impl FnMut(&str)) {
    note(&window_closed_line(how, facts));
}

#[cfg(test)]
mod tests {
    use bt_platform::close_road::{CloseDelivery, ClosePointer, MenuFrom, SystemCloseFrom};

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
            shutting_down: None,
        };
        assert_eq!(
            lines_for(&[exit]),
            ["Folio: pane 7 shell exited code=23 after 41 ms (pwsh.exe, tab 11, retired pane)"]
        );
    }

    /// RED (plan row 199) — **an exit the probe read says whether the session was ending, at the
    /// end of its line; an exit with no probe says what it always said.** The field goes last so
    /// that every reader of the line's start (`code=<n> `) reads it as before.
    ///
    /// MUTATION: drop the `shutting_down` suffix from `shell_exit_line` — the two probed lines
    /// lose their field.
    #[test]
    fn a_shell_exit_says_whether_the_session_was_ending() {
        let exit = |shutting_down| ShellExit {
            seat: 3,
            code: Some(1),
            elapsed: Duration::from_millis(7_234_567),
            program: Some(PathBuf::from("agents").join("claude.exe")),
            tab: 5,
            disposition: ShellExitDisposition::TabRetired,
            shutting_down,
        };
        assert_eq!(
            lines_for(&[exit(Some(true)), exit(Some(false)), exit(None)]),
            [
                "Folio: pane 3 shell exited code=1 after 7234567 ms (claude.exe, tab 5, tab retired) shutting_down=1",
                "Folio: pane 3 shell exited code=1 after 7234567 ms (claude.exe, tab 5, tab retired) shutting_down=0",
                "Folio: pane 3 shell exited code=1 after 7234567 ms (claude.exe, tab 5, tab retired)",
            ]
        );
    }

    /// RED (plan row 199) — **an exit the reaper judges carries the probe read at that instant**:
    /// what `bt_platform::session_end::shutting_down` answers — `Some(false)` on Windows (a test
    /// run is not a sign-out), `None` off Windows, where the line keeps its old shape; which
    /// platform this is stays bt-platform's to say. Every exit the reaper decides is built
    /// through [`ShellExit::observed`].
    ///
    /// MUTATION: `observed` stores `None` — the Windows lines lose their field
    /// (red on Windows only).
    #[test]
    fn an_observed_exit_carries_the_session_end_probe() {
        let exit = ShellExit::observed(
            1,
            Some(0),
            Duration::from_millis(5),
            None,
            2,
            ShellExitDisposition::TabKept,
        );
        assert_eq!(
            exit.shutting_down,
            bt_platform::session_end::shutting_down()
        );
    }

    /// MUTATION (observed RED): omit the stored OS error, keep bt-pty's stderr-only line, or write
    /// the error without reducing its absolute paths.
    #[test]
    fn a_spawn_fallback_names_both_programs_and_the_error_in_resident_diagnostics() {
        let fallback = bt_pty::ShellFallback {
            requested: PathBuf::from("private").join("pwsh.exe").into_os_string(),
            started: bt_pty::LAST_RESORT_SHELL,
            error: "access denied at C:\\Users\\alice\\pwsh.exe\nby the package broker".to_owned(),
        };
        let mut lines = Vec::new();
        say_spawn_fallback(9, &fallback, |line| lines.push(line.to_owned()));
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0],
            format!(
                "Folio: pane 9 shell spawn failed: pwsh.exe (access denied at pwsh.exe by the package broker); fell back to {}",
                program_basename(Some(Path::new(bt_pty::LAST_RESORT_SHELL)))
            )
        );
        assert!(!lines[0].contains("alice"));
    }

    /// MUTATION (observed RED): omit the error from the resident spawn-failure line, or write it
    /// without reducing its absolute paths.
    #[test]
    fn a_nonrecoverable_spawn_failure_has_one_resident_line() {
        let mut lines = Vec::new();
        say_spawn_failure(
            12,
            Some(Path::new("/private/bin/坏-shell")),
            "operation refused: /private/bin/坏-shell\n(os error 5)",
            |line| lines.push(line.to_owned()),
        );
        assert_eq!(
            lines,
            [
                "Folio: pane 12 shell spawn failed: 坏-shell (operation refused: 坏-shell (os error 5))"
            ]
        );
    }

    /// MUTATION (observed RED): report a birth death as an ordinary kept tab.
    #[test]
    fn birth_death_diagnostics_name_the_fallback_and_the_kept_pane() {
        let exit = |disposition| ShellExit {
            seat: 2,
            code: Some(5),
            elapsed: Duration::from_millis(701),
            program: Some(PathBuf::from("pwsh.exe")),
            tab: 3,
            disposition,
            shutting_down: None,
        };
        let lines = lines_for(&[
            exit(ShellExitDisposition::BirthDeathFellBack(
                bt_pty::LAST_RESORT_SHELL,
            )),
            exit(ShellExitDisposition::BirthDeathPaneKept),
        ]);
        assert!(
            lines[0].contains("birth death: fell back to"),
            "{:?}",
            lines[0]
        );
        assert!(
            lines[1].ends_with("birth death: pane kept)"),
            "{:?}",
            lines[1]
        );
    }

    fn close_asked_by(road: Option<CloseRoad>) -> ClosedHow {
        ClosedHow::Asked(CloseAsked {
            arrival: road.map(|road| CloseArrival {
                road,
                delivery: Some(CloseDelivery::Posted),
                foreground: Some(true),
                pointer: ClosePointer::Unread,
            }),
            pointer: PointerAtClose::CloseButton,
        })
    }

    const LAST: ClosedFacts = ClosedFacts {
        window: ClosedWindow::Ordinary,
        id: 7,
        tabs: 9,
        panes: 11,
        windows_left: None,
    };

    /// MUTATION (observed RED): map `ShellsExited` to `the person closed it`.
    #[test]
    fn the_last_window_after_every_shell_exited_says_that_cause() {
        assert_eq!(
            window_closed_line(
                close_asked_by(Some(CloseRoad::Own(OwnCloseRoad::ShellsExited))),
                ClosedFacts {
                    tabs: 1,
                    panes: 1,
                    ..LAST
                }
            ),
            "Folio: last window closed — every pane's shell exited; window=ordinary 7; \
             road=shell-exit; delivery=posted; foreground=yes; pointer=close-button; tabs=1; panes=1"
        );
    }

    /// RED (mutation: `closed_cause` answering `the person closed it` for every road) — **each
    /// road of a window's close has its ruled words (R3)**, and only Folio's own caption and
    /// final-tab roads say the person closed it.
    #[test]
    fn every_close_road_has_its_ruled_words_on_one_line() {
        for (how, cause, road) in [
            (
                close_asked_by(Some(CloseRoad::Own(OwnCloseRoad::CaptionButton))),
                "the person closed it",
                "caption-button",
            ),
            (
                close_asked_by(Some(CloseRoad::Own(OwnCloseRoad::FinalTab))),
                "the person closed it",
                "final-tab",
            ),
            (
                close_asked_by(Some(CloseRoad::SystemClose(SystemCloseFrom::Accelerator))),
                "the system's close command",
                "system-close (accelerator)",
            ),
            (
                close_asked_by(Some(CloseRoad::SystemClose(SystemCloseFrom::Mnemonic))),
                "the system's close command",
                "system-close (mnemonic)",
            ),
            (
                close_asked_by(Some(CloseRoad::SystemClose(SystemCloseFrom::Pointer))),
                "the system's close command",
                "system-close (pointer)",
            ),
            (
                close_asked_by(Some(CloseRoad::CloseMessage)),
                "a close message",
                "close-message",
            ),
            (
                close_asked_by(Some(CloseRoad::CloseButton)),
                "the system's close command",
                "close-button",
            ),
            (
                close_asked_by(Some(CloseRoad::Menu(MenuFrom::Key))),
                "the system's close command",
                "menu (key)",
            ),
            (
                close_asked_by(Some(CloseRoad::Menu(MenuFrom::Pointer))),
                "the system's close command",
                "menu (pointer)",
            ),
            (
                close_asked_by(Some(CloseRoad::Outside)),
                "a close message",
                "outside",
            ),
            (
                close_asked_by(None),
                "an unmarked close request",
                "unmarked",
            ),
            (
                ClosedHow::TabMovedOut,
                "the last tab moved to another window",
                "tab-moved-out",
            ),
        ] {
            let line = window_closed_line(how, LAST);
            assert!(
                line.starts_with(&format!(
                    "Folio: last window closed — {cause}; window=ordinary 7; road={road}; "
                )),
                "{line}"
            );
            assert!(line.ends_with("; tabs=9; panes=11"), "{line}");
            assert!(!line.contains(['\r', '\n']), "{line:?}");
        }
    }

    /// RED (mutation: the `windows left` tail dropped) — **a close that does not end the run says
    /// so, and how many ordinary windows are left (R4)**; an unwitnessed request reads its three
    /// unknowns as such.
    #[test]
    fn a_close_that_does_not_end_the_run_says_the_windows_left() {
        let line = window_closed_line(
            ClosedHow::Asked(CloseAsked {
                arrival: None,
                pointer: PointerAtClose::Unread,
            }),
            ClosedFacts {
                window: ClosedWindow::Summon,
                windows_left: Some(1),
                ..LAST
            },
        );
        assert_eq!(
            line,
            "Folio: window closed — an unmarked close request; window=summon 7; road=unmarked; \
             delivery=unread; foreground=unread; pointer=unread; tabs=9; panes=11; windows left=1"
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
            shutting_down: None,
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
            shutting_down: None,
        };
        let line = lines_for(&[exit]).pop().expect("the exit line");
        assert_eq!(
            line,
            "Folio: pane 13 shell exited code=7 after 29 ms (外壳 程序, tab 17, tab retired)"
        );
        assert!(!line.contains(['\r', '\n']), "{line:?}");
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
            (LastWindowCause::LaunchWireQuit, "launch wire quit"),
            (LastWindowCause::UpdateRestart, "update restart"),
            (LastWindowCause::SystemClosedIt, "the system closed it"),
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
