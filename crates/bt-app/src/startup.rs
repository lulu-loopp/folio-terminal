//! **The order a start runs in** (T-STARTUP-ORDER-SHARED): the steps a launch takes before its
//! first window has a tab, written once, as two functions every start goes through.
//!
//! A start is split in two by the event loop, and so is this order. [`begin`] is the process's
//! half, which `fn main` calls before there is a loop: the removal verbs, the command line, the
//! hand-over. [`resume`] is the first window's half, which `Runtime::create` calls when the loop
//! first resumes: the startup migration and the script's preparation before the window, the
//! window, the PowerShell questions before the first tab, the first tab. The window's
//! half needs the running loop the process's half builds, so nothing can call it first.
//!
//! What each step does is the caller's, handed in as the step: the product's are the bodies
//! `main` and `Runtime::create` write inline, and a test's are stand-ins that record which step
//! ran (`headless::StartupStep`). What the order is, is this module's — the one thing a test of
//! the steps' order reads, and the one thing the product cannot do differently.

/// **The process's steps, in the fields' order**: what [`begin`] is handed.
pub(crate) struct FrontDoor<Removal, Parse, Claim> {
    /// A removal verb answered (`--remove-shell-integration`, `--remove-explorer-menu`): its
    /// process leaves here, having opened nothing, written no settings and handed nothing over.
    pub(crate) removal: Removal,
    /// The command line read: a rescue door's line answered, every other line parsed, and a
    /// refusal answered on the console it was typed at.
    pub(crate) parse: Parse,
    /// The hand-over: the data directory's claim taken, or the parsed request handed to the Folio
    /// that holds it and this process gone. What comes back is what it carries into its own run.
    pub(crate) claim: Claim,
}

/// **The process's half of a start**: the removal verbs, then the command line, then the
/// hand-over — and what the hand-over kept, for the run.
///
/// The removal comes before the parse because a verb that takes Folio back out of the machine is
/// owed nothing the parse leads to; the parse before the hand-over because what is handed over is
/// the parsed request.
pub(crate) fn begin<Request, Resident>(
    door: FrontDoor<impl FnOnce(), impl FnOnce() -> Request, impl FnOnce(Request) -> Resident>,
) -> Resident {
    (door.removal)();
    let request = (door.parse)();
    (door.claim)(request)
}

/// **The first window's steps, in the fields' order**: what [`resume`] is handed.
pub(crate) struct FirstWindow<Migration, Script, Window, Questions, FirstTab> {
    /// The startup's observation of an older Folio line in a `$PROFILE`
    /// (`shell_integration::begin_startup_migration`), started on a worker nobody waits for.
    pub(crate) migration: Migration,
    /// Folio's own `folio.ps1`, compared and repaired on a worker nobody waits for
    /// (`shell_integration::begin_powershell_script_preparation`).
    pub(crate) script: Script,
    /// The window, opened through the backend's door, with what it is painted with.
    pub(crate) window: Window,
    /// The parser questions about the PowerShell rows, put to the probe on a worker nobody waits
    /// for, about the program answers the first tab is made from.
    pub(crate) questions: Questions,
    /// The first window's tabs, their panes made from those answers in the opened window.
    pub(crate) first_tab: FirstTab,
}

/// **The first window's half of a start**: the startup migration and the script's preparation,
/// then the window, then the PowerShell questions, then the first tab.
///
/// The two workers start before the window because neither is the window's to wait for: what
/// they do is done by the time a PowerShell is born, or that birth prepares the script itself.
/// The questions are asked before the first tab because a PowerShell row's pane composes its
/// command line from the answer, and a question asked after the birth is one the birth could not
/// have found answered in time. `answers` is what the steps share: the program walk's answers, which the
/// window and the questions adopt and the first tab is born from.
pub(crate) fn resume<Answers, Window, Tabs, Error>(
    answers: &mut Answers,
    steps: FirstWindow<
        impl FnOnce(),
        impl FnOnce(),
        impl FnOnce(&mut Answers) -> Result<Window, Error>,
        impl FnOnce(&mut Answers),
        impl FnOnce(&mut Answers, &mut Window) -> Result<Tabs, Error>,
    >,
) -> Result<(Window, Tabs), Error> {
    (steps.migration)();
    (steps.script)();
    let mut window = (steps.window)(answers)?;
    (steps.questions)(answers);
    let tabs = (steps.first_tab)(answers, &mut window)?;
    Ok((window, tabs))
}

#[cfg(test)]
mod tests {
    use crate::headless::{self, StartupStep};

    /// The walk's answers as two looks land them: a PowerShell 7 row first, a Windows PowerShell
    /// row (its name in Chinese) once the window is up.
    fn walk() -> Vec<Vec<String>> {
        vec![
            vec!["pwsh 7".to_owned()],
            vec!["Windows PowerShell 中文".to_owned()],
        ]
    }

    /// RED (T-STARTUP-ORDER-SHARED) — **a start runs its eight steps in one order**: a removal
    /// verb, the command line, the hand-over, the startup migration, the script's preparation, the
    /// window, the PowerShell questions, the first tab — through `begin` and `resume`,
    /// the functions `main` and `Runtime::create` call, with the headless start's stand-ins. The
    /// hand-over is handed what the parse read.
    ///
    /// MUTATIONS: swap the removal and the parse in `begin` — the command line is read before a
    /// removal verb is answered; swap the script's preparation and the window in
    /// `resume` — the window opens before the script is being prepared.
    #[test]
    fn the_startup_steps_run_in_their_order() {
        let argv = r"folio --cwd D:\项目 --profile pwsh";
        let ran = headless::start(argv, walk());
        assert_eq!(
            ran.steps,
            vec![
                StartupStep::Removal,
                StartupStep::CommandLine,
                StartupStep::HandOver,
                StartupStep::StartupMigration,
                StartupStep::ScriptPreparation,
                StartupStep::WindowOpened,
                StartupStep::PowerShellQuestions,
                StartupStep::FirstTab,
            ]
        );
        assert_eq!(ran.handed_over.as_deref(), Some(argv));
    }

    /// RED (T-STARTUP-ORDER-SHARED) — **the PowerShell questions are asked before the first tab,
    /// about every program answer the first tab is made from.** The walk answers one row when the
    /// window is titled and another before the questions; the questions are asked about both, and
    /// the first tab, made after them, is made from exactly those.
    ///
    /// MUTATION: make the first tab before the questions in `resume` — the first tab
    /// comes first, and it is made from the one answer the window adopted, never asked about the
    /// Windows PowerShell row it would be born without an answer for.
    #[test]
    fn the_powershell_questions_are_asked_before_the_first_tab() {
        let ran = headless::start("folio", walk());
        let at = |wanted: StartupStep| {
            ran.steps
                .iter()
                .position(|step| *step == wanted)
                .unwrap_or_else(|| panic!("{wanted:?} never ran: {:?}", ran.steps))
        };
        assert!(
            at(StartupStep::PowerShellQuestions) < at(StartupStep::FirstTab),
            "{:?}",
            ran.steps
        );
        assert_eq!(ran.asked, ["pwsh 7", "Windows PowerShell 中文"]);
        assert_eq!(ran.born_from, ran.asked);
    }
}
