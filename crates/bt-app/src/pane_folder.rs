//! **The folder a pane is standing in, for the doors that open a folder from it**
//! (T-CWD-FROM-PROCESS; `docs/DESIGN.md`, 2026-10-17).
//!
//! One ladder, read on the ask — the folder button's card, `Ctrl+Shift+B`'s column, the files
//! column's root menu, the palette's place hint and the two folder choosers:
//!
//! 1. **The shell's own report** (OSC 7), when it has made one: the shell knows where it is.
//! 2. **The shell process's current directory**, read through `bt_platform::process_folder`, when
//!    the shell has said nothing — a shell without Folio's integration, a row whose arguments
//!    refused the injection, a program that is not a shell. The process read is of the shell
//!    the pane started (`bt_pty::PtySession::shell_process_id`), not of a program running under
//!    it: the shell's folder is what a person means by the pane's folder, and a program it runs
//!    started there.
//! 3. **The folder the pane was opened in**, only while there is no shell process to read (a
//!    pane being born, a pane with no shell): nothing has moved it yet.
//! 4. **Nothing** — [`PaneFolder::Unknown`] — when the process was asked and refused (no
//!    access, ended, a bridge to another machine, an answer that is no folder). The doors fall
//!    to the account's home and say "Folder unknown"; none of them shows a home folder as if it
//!    were the pane's.
//!
//! **The read is cached per pane until the shell has written something** ([`ShellMoment`]): a
//! shell that changes folder prints its next prompt, so two asks with no output between them read
//! the process once, and an ask after any output — or after the shell's exit was seen — reads it
//! again. Nothing reads it on a clock.

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

use bt_platform::process_folder::FolderRefusal;
use bt_term::DualPlaneSession;

/// The door the ladder's second rung asks: the product's is [`SystemProcessFolders`]; a test hands
/// in a scripted one.
pub(crate) trait ProcessFolders {
    /// The current directory of the shell process `shell`, or why it cannot be said.
    fn current_directory(&self, shell: bt_pty::ShellProcessId) -> Result<PathBuf, FolderRefusal>;
}

/// The operating system's answer, through `bt_platform::process_folder`.
pub(crate) struct SystemProcessFolders;

impl ProcessFolders for SystemProcessFolders {
    fn current_directory(&self, shell: bt_pty::ShellProcessId) -> Result<PathBuf, FolderRefusal> {
        bt_platform::process_folder::current_directory(shell.get())
    }
}

/// Where a pane is standing, and how that is known.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PaneFolder {
    /// The shell's last OSC 7 report.
    Reported(PathBuf),
    /// Read from the shell process, which reported nothing.
    Process(PathBuf),
    /// The folder the pane was opened in, while it has no shell process to read.
    Opened(PathBuf),
    /// No folder can be said: the process refused (its reason), or there was neither a process
    /// nor an opened-in folder (`None`).
    Unknown(Option<FolderRefusal>),
}

impl PaneFolder {
    /// The folder, when one is known.
    pub(crate) fn folder(&self) -> Option<&Path> {
        match self {
            Self::Reported(folder) | Self::Process(folder) | Self::Opened(folder) => Some(folder),
            Self::Unknown(_) => None,
        }
    }

    /// The folder, owned, when one is known.
    pub(crate) fn into_folder(self) -> Option<PathBuf> {
        match self {
            Self::Reported(folder) | Self::Process(folder) | Self::Opened(folder) => Some(folder),
            Self::Unknown(_) => None,
        }
    }
}

/// **The shell as it stands at one moment** — which process, how many bytes it had written, and
/// whether its exit had been seen. A cached read answers only the moment it was made at.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ShellMoment {
    pub(crate) shell: bt_pty::ShellProcessId,
    /// `bt_pty::PtySession::output_bytes_received` — every byte the shell has written.
    pub(crate) output: u64,
    pub(crate) exited: bool,
}

/// One pane's last process read and the moment it was made at.
///
/// A `RefCell` because the doors that ask are readers of the window (`&self`), and the read is
/// the same answer whoever asks first: the cache is a memo of the process's fact at one moment,
/// never a fact of its own.
#[derive(Debug, Default)]
pub(crate) struct ProcessFolderMemo(RefCell<Option<(ShellMoment, Result<PathBuf, FolderRefusal>)>>);

impl ProcessFolderMemo {
    /// The read for `moment`: the cached one when it was made at this moment, else a fresh one
    /// from `door`, kept.
    fn read_at(
        &self,
        moment: ShellMoment,
        door: &dyn ProcessFolders,
    ) -> Result<PathBuf, FolderRefusal> {
        let mut memo = self.0.borrow_mut();
        if let Some((at, read)) = memo.as_ref()
            && *at == moment
        {
            return read.clone();
        }
        let read = door.current_directory(moment.shell);
        *memo = Some((moment, read.clone()));
        read
    }
}

/// **The ladder** (the module header): `session`'s report, else the process read for `shell`,
/// else — with no process to read — the folder the pane was opened in.
pub(crate) fn pane_folder(
    session: &DualPlaneSession,
    shell: Option<ShellMoment>,
    memo: &ProcessFolderMemo,
    door: &dyn ProcessFolders,
) -> PaneFolder {
    if let Some(reported) = session.working_directory() {
        return PaneFolder::Reported(reported.to_path_buf());
    }
    match shell {
        Some(moment) => match memo.read_at(moment, door) {
            Ok(folder) => PaneFolder::Process(folder),
            Err(refusal) => PaneFolder::Unknown(Some(refusal)),
        },
        None => session
            .standing_folder()
            .map_or(PaneFolder::Unknown(None), |opened| {
                PaneFolder::Opened(opened.to_path_buf())
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{host_path, host_uri_path, leaf_saying};
    use std::cell::Cell;

    /// A door that answers `answer` and counts how often it was asked.
    struct Scripted {
        answer: Result<PathBuf, FolderRefusal>,
        asked: Cell<usize>,
    }

    impl Scripted {
        fn answering(answer: Result<PathBuf, FolderRefusal>) -> Self {
            Self {
                answer,
                asked: Cell::new(0),
            }
        }
    }

    impl ProcessFolders for Scripted {
        fn current_directory(
            &self,
            _shell: bt_pty::ShellProcessId,
        ) -> Result<PathBuf, FolderRefusal> {
            self.asked.set(self.asked.get() + 1);
            self.answer.clone()
        }
    }

    fn moment(output: u64, exited: bool) -> ShellMoment {
        ShellMoment {
            shell: bt_pty::ShellProcessId::for_test(4242),
            output,
            exited,
        }
    }

    /// RED — **a shell that reported nothing has its folder read from its process, once**, and the
    /// answer is the pane's folder.
    ///
    /// MUTATION (observed red): the second rung skipped (`shell` ignored) — the pane answers
    /// `Opened`, the folder it was opened in.
    #[test]
    fn a_silent_shell_is_read_from_its_process_once() {
        let leaf = leaf_saying("no report from this shell");
        let mut session = leaf.session;
        session.set_spawn_directory(Some(host_path(r"D:\opened")));
        let door = Scripted::answering(Ok(host_path(r"D:\opened\moved")));
        let memo = ProcessFolderMemo::default();
        assert_eq!(
            pane_folder(&session, Some(moment(10, false)), &memo, &door),
            PaneFolder::Process(host_path(r"D:\opened\moved"))
        );
        assert_eq!(door.asked.get(), 1);
    }

    /// RED — **a refused read is no folder**, never the folder the pane was opened in and never a
    /// home folder named as the pane's.
    ///
    /// MUTATION (observed red): a refusal answers `standing_folder` — `Opened`.
    #[test]
    fn a_refused_read_is_an_unknown_folder() {
        let leaf = leaf_saying("no report from this shell");
        let mut session = leaf.session;
        session.set_spawn_directory(Some(host_path(r"D:\opened")));
        for refusal in [
            FolderRefusal::NoAccess,
            FolderRefusal::Exited,
            FolderRefusal::FarSide,
            FolderRefusal::Unreadable,
            FolderRefusal::Unsupported,
        ] {
            let door = Scripted::answering(Err(refusal));
            let memo = ProcessFolderMemo::default();
            assert_eq!(
                pane_folder(&session, Some(moment(10, false)), &memo, &door),
                PaneFolder::Unknown(Some(refusal))
            );
        }
    }

    /// RED — **a shell's report is the answer and the process is never asked.**
    ///
    /// MUTATION (observed red): the report rung after the process rung — the door's answer wins.
    #[test]
    fn a_report_is_the_answer_and_the_door_is_never_asked() {
        let leaf = leaf_saying(&format!(
            "\u{1b}]7;file://localhost{}\u{7}",
            host_uri_path(r"D:\reported")
        ));
        let door = Scripted::answering(Ok(host_path(r"D:\elsewhere")));
        let memo = ProcessFolderMemo::default();
        assert_eq!(
            pane_folder(&leaf.session, Some(moment(10, false)), &memo, &door),
            PaneFolder::Reported(host_path(r"D:\reported"))
        );
        assert_eq!(door.asked.get(), 0);
    }

    /// RED — **with no shell process the pane stands where it was opened**, and with neither the
    /// folder is unknown.
    #[test]
    fn with_no_process_the_pane_stands_where_it_was_opened() {
        let leaf = leaf_saying("no shell behind this pane");
        let mut session = leaf.session;
        let door = Scripted::answering(Ok(host_path(r"D:\never")));
        let memo = ProcessFolderMemo::default();
        assert_eq!(
            pane_folder(&session, None, &memo, &door),
            PaneFolder::Unknown(None)
        );
        session.set_spawn_directory(Some(host_path(r"D:\opened")));
        assert_eq!(
            pane_folder(&session, None, &memo, &door),
            PaneFolder::Opened(host_path(r"D:\opened"))
        );
        assert_eq!(door.asked.get(), 0);
    }

    /// RED — **two asks with nothing written between them read the process once; output or an
    /// exit seen between them reads it again.**
    ///
    /// MUTATIONS (observed red): no memo (every ask reads) — the second ask counts 2; the memo
    /// keyed by the shell alone — the ask after output counts 1.
    #[test]
    fn two_asks_between_prompts_read_once() {
        let leaf = leaf_saying("no report from this shell");
        let door = Scripted::answering(Ok(host_path(r"D:\here")));
        let memo = ProcessFolderMemo::default();
        let ask = |moment| pane_folder(&leaf.session, Some(moment), &memo, &door);
        ask(moment(10, false));
        ask(moment(10, false));
        assert_eq!(door.asked.get(), 1, "nothing written: one read");
        ask(moment(11, false));
        assert_eq!(door.asked.get(), 2, "the shell wrote: read again");
        ask(moment(11, true));
        assert_eq!(door.asked.get(), 3, "the shell's exit was seen: read again");
        ask(moment(11, true));
        assert_eq!(door.asked.get(), 3);
    }
}
