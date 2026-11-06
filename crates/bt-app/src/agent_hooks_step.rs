//! **Agent hooks, said and repaired at start: the carry pass's first member** (0.4.10 plan row
//! 67b, on 67a's writer that keeps the person's bytes).
//!
//! The owner's machine lost resume after an update because the hooks named a copy of Folio that
//! was gone, the row read Off and nothing said why. So, on the pass's worker
//! ([`crate::carry_pass`]), each agent's configuration is classified
//! ([`HooksFound`], through the family's own `scan_resolved`), and exactly two states are written
//! without a question (R1): **a dead owner** — every other copy the file names is `NotFound` on a
//! volume that is there (R4) — and **this copy's own old shape** (R3). Both are written by the
//! switch's own writer (`apply_resolved` with [`Decision::Install`]), which asks every question
//! again before it writes, records the configuration's root in the marks and changes only Folio's
//! own entries (row 67a). Everything else is said on the Agents page and changes nothing: a living
//! other copy (its 30-second take-over is the press's, R5), an owner the machine will not say
//! about, a file this build cannot read, an entry that looks like Folio's in a shape it cannot
//! classify, somebody else's `notify` or hook file.
//!
//! **Held during an update's trial** ([`Writer::AgentHooksRepair`], asked once per run and only
//! when a repair is due): the classification is still published, and the commit runs the pass
//! again. **Never beside a press** (R11 keeps the press on the window thread): a press takes its
//! family's latch ([`AgentHooksStep::take_latch`]) for its write, and a repair that finds the latch
//! taken writes nothing. One toast per family whose gone copy's hooks were pointed here (R8), and
//! one diagnostics line per repair due, `BT_AGENT_HOOKS <family> <state> repaired=<from>`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use bt_platform::admission::WorkerCtx;

use crate::attention_copilot::Readiness;
use crate::attention_hooks::{Config, Unresolved};
use crate::attention_ownership::{self as ownership, Decision, HooksFound, Outcome};
use crate::i18n::Text;
use crate::update_trial::Writer;

/// A family's classification of a resolved file, against this copy's program, with the machine's
/// answer about a path handed in.
pub(crate) type HooksScanFn = fn(&Config, &Path, &dyn ownership::Look) -> HooksFound;

/// **One agent's configuration, as the step reads and repairs it** — the seam a family joins by
/// (row 69's Codex `hooks.json` replaces the `notify` member here).
pub(crate) struct HooksFamily {
    /// The family's name in `attention_map` (`claude-code`, `codex`, `copilot`): what the resume
    /// row asks `agent_resume` about.
    pub(crate) family: &'static str,
    /// The word the diagnostics line names it by.
    pub(crate) tag: &'static str,
    /// Where its file is on this machine, as the newest program walk read the agent's folders.
    pub(crate) path: fn() -> Option<PathBuf>,
    /// The family's classification of a resolved file ([`HooksFound`]).
    pub(crate) scan: HooksScanFn,
    /// The switch's own writer.
    pub(crate) apply: fn(&Config, Decision, &Path, &Path) -> Outcome,
    /// Whether a repair may write the family's hooks now: always, except where the agent's own
    /// version decides whether they mean what Folio reads them as (Copilot, R9), or which entry is
    /// Folio's (Codex, 0.4.10 plan row 69c).
    pub(crate) within_its_gate: fn() -> bool,
    /// The notice a repair of a gone copy's hooks raises (R8), by the file the repair wrote.
    pub(crate) repaired: fn(&Path) -> Text,
}

/// **The families, in the Agents page's row order** — the order of every per-agent array on the
/// window (`agent_config_refusals`, `agent_takeovers`).
pub(crate) static HOOKS_FAMILIES: [HooksFamily; 3] = [
    HooksFamily {
        family: crate::attention_map::CLAUDE_CODE,
        tag: "claude",
        path: crate::attention_hooks::settings_path,
        scan: crate::attention_hooks::scan_resolved,
        apply: crate::attention_hooks::apply_resolved,
        within_its_gate: no_gate,
        repaired: claude_repaired,
    },
    // Its file by its version (row 69c): `hooks.json` from 0.148, `config.toml`'s `notify` line
    // below; repaired only once the version has been asked and answered.
    HooksFamily {
        family: crate::attention_map::CODEX,
        tag: "codex",
        path: crate::attention_codex::entry_path,
        scan: crate::attention_codex::scan_resolved,
        apply: crate::attention_codex::apply_resolved,
        within_its_gate: crate::attention_codex::repair_within_the_gate,
        repaired: crate::attention_codex::repaired_notice,
    },
    // Repaired within its version gate (R9): only once the version question has been asked and
    // answered, and never for a copilot too old for the hooks (`repair_within_the_gate`). Never on
    // the resume row's line: this build resumes no Copilot session.
    HooksFamily {
        family: crate::attention_map::COPILOT,
        tag: "copilot",
        path: crate::attention_copilot::hooks_path,
        scan: crate::attention_copilot::scan_resolved,
        apply: crate::attention_copilot::apply_resolved,
        within_its_gate: crate::attention_copilot::repair_within_the_gate,
        repaired: copilot_repaired,
    },
];

/// A family whose hooks mean the same to every version of its agent.
fn no_gate() -> bool {
    true
}

/// The notices of the two families with one file each.
fn claude_repaired(_: &Path) -> Text {
    Text::AgentHooksRepairedClaude
}

fn copilot_repaired(_: &Path) -> Text {
    Text::AgentHooksRepairedCopilot
}

/// **One family's answer from one run of the step.**
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HooksScan {
    /// What the file holds now — after the repair, when there was one.
    pub(crate) found: HooksFound,
    /// What a repair this run wrote the hooks from.
    pub(crate) repaired_from: Option<HooksFound>,
    /// The notice that repair owes, when it pointed a gone copy's hooks here (R8) and the rescan
    /// confirmed it (review N6) — by the file it wrote, for codex's two (row 69c).
    pub(crate) notice: Option<Text>,
    /// Why a repair that was due did not happen, when the row has that to say (review N4).
    pub(crate) unrepaired: Option<Unrepaired>,
}

impl HooksScan {
    fn unrepaired(found: HooksFound) -> Self {
        Self {
            found,
            repaired_from: None,
            notice: None,
            unrepaired: None,
        }
    }

    fn left(found: HooksFound, why: Unrepaired) -> Self {
        Self {
            unrepaired: Some(why),
            ..Self::unrepaired(found)
        }
    }
}

/// **Why a due repair was not written, as the Agents row says it** (0.4.10 plan row 67b, review
/// N4). A repair outside its family's gate, or beside a press, has nothing to say: the version's
/// answer and the press each run the pass again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Unrepaired {
    /// Held while an update is on trial; the commit runs the pass again.
    Held,
    /// The writer refused, for its sentence.
    Refused(&'static str),
}

/// **What the window adopts**: the three answers, why a due repair was not written, Copilot's
/// readiness read beside them, and the notices the runs since the last adoption owe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HooksSnapshot {
    pub(crate) found: [HooksFound; 3],
    pub(crate) unrepaired: [Option<Unrepaired>; 3],
    pub(crate) copilot_readiness: Readiness,
    pub(crate) notices: Vec<Text>,
}

/// **A family's latch, held** — let go when this is dropped, on a return or as the thread unwinds,
/// so a panic in a write never leaves the family latched for the rest of the run.
pub(crate) struct HooksLatchHeld<'a>(&'a AtomicBool);

impl Drop for HooksLatchHeld<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// **The step's state between its runs**: one latch per family, shared with the presses, and the
/// snapshot waiting for the window.
pub(crate) struct AgentHooksStep {
    latches: [AtomicBool; 3],
    published: Mutex<Option<HooksSnapshot>>,
}

impl AgentHooksStep {
    pub(crate) const fn new() -> Self {
        Self {
            latches: [
                AtomicBool::new(false),
                AtomicBool::new(false),
                AtomicBool::new(false),
            ],
            published: Mutex::new(None),
        }
    }

    /// **Take family `index`'s latch**, or `None` when somebody holds it. A press holds it across
    /// its write whatever this answers (the press wins ties: it is the person saying what they
    /// want); a repair that is answered `None` writes nothing.
    pub(crate) fn take_latch(&self, index: usize) -> Option<HooksLatchHeld<'_>> {
        let latch = &self.latches[index];
        (!latch.swap(true, Ordering::AcqRel)).then_some(HooksLatchHeld(latch))
    }

    /// **One run over the families**: each file at `paths` (the families' order) resolved once,
    /// classified against `exe` — this copy's program — with `look` answering what stands at a
    /// path, and repaired where [`HooksFound::wants_repair`] says, into the marks of `data`.
    /// `defer` is the trial gate's question, asked at most once, and only when a repair is due;
    /// `within_its_gate` is a family's own ([`HooksFamily::within_its_gate`]), asked only when a
    /// repair of that family is due.
    pub(crate) fn scan_and_repair(
        &self,
        paths: &[Option<PathBuf>; 3],
        exe: &Path,
        data: &Path,
        look: &dyn ownership::Look,
        mut defer: impl FnMut(Writer) -> bool,
        within_its_gate: &dyn Fn(&HooksFamily) -> bool,
    ) -> [HooksScan; 3] {
        let mut held = None;
        std::array::from_fn(|index| {
            let family = &HOOKS_FAMILIES[index];
            let config = match paths[index].as_deref().map(Config::resolve) {
                None => return HooksScan::unrepaired(HooksFound::Nothing),
                Some(Err(Unresolved::Refused(reason))) => {
                    return HooksScan::unrepaired(HooksFound::Refused(reason));
                }
                Some(Err(Unresolved::Unreadable)) => {
                    return HooksScan::unrepaired(HooksFound::Unreadable);
                }
                // One read for the run (`Config::with_one_read`): what is classified is what is
                // edited, and a press that wrote since makes the write refuse rather than undo it.
                Some(Ok(config)) => config.with_one_read(),
            };
            let found = (family.scan)(&config, exe, look);
            if !found.wants_repair() {
                return HooksScan::unrepaired(found);
            }
            let say = |how: &str| {
                crate::diagnostics::stderr_line(format!(
                    "BT_AGENT_HOOKS {} {} repaired={how}{}",
                    family.tag,
                    found.diagnostic_word(),
                    gone_programs(&found)
                ));
            };
            if *held.get_or_insert_with(|| defer(Writer::AgentHooksRepair)) {
                say("held");
                return HooksScan::left(found, Unrepaired::Held);
            }
            if !within_its_gate(family) {
                say("gated");
                return HooksScan::unrepaired(found);
            }
            let Some(_latch) = self.take_latch(index) else {
                say("pressed");
                return HooksScan::unrepaired(found);
            };
            match (family.apply)(&config, Decision::Install, exe, data) {
                Outcome::Installed => {
                    let config = config.read_afresh();
                    let now = (family.scan)(&config, exe, look);
                    // **A repair is said only where the rescan confirms it** (review N6): two copies
                    // starting together can both land, and the one whose write came first finds
                    // the other's hooks there now — its own write lost, nothing to announce.
                    let confirmed = now.is_this_copys();
                    let notice = (confirmed && matches!(found, HooksFound::Dead(_)))
                        .then(|| (family.repaired)(config.named()));
                    crate::diagnostics::stderr_line(format!(
                        "BT_AGENT_HOOKS {} {} repaired={}{}",
                        family.tag,
                        now.diagnostic_word(),
                        if confirmed {
                            found.diagnostic_word().to_owned()
                        } else {
                            format!("lost ({})", now.diagnostic_word())
                        },
                        gone_programs(&found)
                    ));
                    HooksScan {
                        repaired_from: confirmed.then_some(found),
                        notice,
                        ..HooksScan::unrepaired(now)
                    }
                }
                Outcome::Refused(reason) => {
                    say(&format!("refused ({reason})"));
                    HooksScan::left(found, Unrepaired::Refused(reason))
                }
                outcome => {
                    say(&format!("refused ({outcome:?})"));
                    HooksScan::unrepaired(found)
                }
            }
        })
    }

    /// Put one run's answers where the window takes them; a snapshot the window has not taken
    /// yet hands its notices on to this one.
    pub(crate) fn publish_scans(&self, scans: &[HooksScan; 3], copilot_readiness: Readiness) {
        let mut published = self
            .published
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut notices = published
            .take()
            .map(|untaken| untaken.notices)
            .unwrap_or_default();
        notices.extend(scans.iter().filter_map(|scan| scan.notice));
        *published = Some(HooksSnapshot {
            found: std::array::from_fn(|index| scans[index].found.clone()),
            unrepaired: std::array::from_fn(|index| scans[index].unrepaired),
            copilot_readiness,
            notices,
        });
    }

    /// **The newest snapshot, once** — the window's adoption.
    pub(crate) fn take_snapshot(&self) -> Option<HooksSnapshot> {
        self.published
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

/// `" (was: <programs>)"` for a dead owner's programs — the paths go to the local log only.
fn gone_programs(found: &HooksFound) -> String {
    match found {
        HooksFound::Dead(programs) => format!(
            " (was: {})",
            programs
                .iter()
                .map(|program| program.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => String::new(),
    }
}

/// This process's step.
pub(crate) static AGENT_HOOKS: AgentHooksStep = AgentHooksStep::new();

/// How a run asks for a frame — installed once, at startup ([`install_hooks_wake`]).
static HOOKS_WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Install the wake, once.
pub(crate) fn install_hooks_wake(wake: impl Fn() + Send + Sync + 'static) {
    let _ = HOOKS_WAKE.set(Box::new(wake));
}

/// **The step, on this machine**: the carry pass's member ([`crate::carry_pass::run_carry_steps`]).
///
/// **Not before the program walk has answered** (the pane-environment ruling of 2026-10-05: the
/// folders are the account's current ones, never the launch's): the walk's first answer runs the
/// pass, so a run asked earlier — an Agents page opened in the first second — has nothing to read
/// yet and waits for it.
pub(crate) fn run_agent_hooks_step(ctx: &WorkerCtx) {
    if !crate::attention_hooks::AGENT_HOMES.answered() {
        return;
    }
    // The one family whose file is named by a question of its agent: codex's version decides
    // whether Folio's entry is the hook or the `notify` line (row 69c, R17), so it is settled here,
    // on the pass's worker, before any family's file is named.
    let _ = crate::attention_codex::settle_version(ctx);
    let Some(exe) = ownership::this_copy() else {
        crate::diagnostics::stderr_line(
            "BT_AGENT_HOOKS not scanned: this copy's own program could not be named".to_owned(),
        );
        return;
    };
    let paths = HOOKS_FAMILIES.each_ref().map(family_path);
    let scans = AGENT_HOOKS.scan_and_repair(
        &paths,
        &exe,
        &crate::persist::storage_dir(),
        &ownership::what_stands,
        crate::update_trial::defer,
        &|family| (family.within_its_gate)(),
    );
    AGENT_HOOKS.publish_scans(&scans, crate::attention_copilot::readiness());
    if let Some(wake) = HOOKS_WAKE.get() {
        wake();
    }
}

/// **Where a family's file is**: the account's own, in the product.
///
/// **A test process's agents keep their files in a scratch folder of its own**, never in the
/// account's (`persist::storage_dir`'s rule for the data folder): a test that drives a walk's
/// answer, an Agents-page open or a trial's commit runs the pass, and the pass writes.
fn family_path(family: &HooksFamily) -> Option<PathBuf> {
    #[cfg(test)]
    {
        Some(test_process_agent_homes().join(family.tag).join("config"))
    }
    #[cfg(not(test))]
    {
        (family.path)()
    }
}

/// The scratch folder [`family_path`] keeps a test process's agents in: one per process, named
/// once, under the build's `target/tb-tests` with the step's own tests' folders.
#[cfg(test)]
fn test_process_agent_homes() -> &'static Path {
    static HOMES: OnceLock<PathBuf> = OnceLock::new();
    HOMES.get_or_init(|| {
        test_scratch_root().join(bt_testpath::unique_name("bt-app-test-process-agent-homes"))
    })
}

/// The build's own scratch folder for this module's tests.
#[cfg(test)]
fn test_scratch_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tb-tests")
}

/// **The Agents row's line over what the scan found**, when it has one to say; `None` keeps the
/// row's own sentence. A refused file says its reason through `agent_config_refusals`, which also
/// makes its switch inert, so it has no line here. A due repair that was held or refused says why
/// (review N4); this copy's entries no install put where they stand are named by their events
/// (ruling N1). Said in the language in force.
#[must_use]
pub(crate) fn hooks_row_line(
    found: &HooksFound,
    unrepaired: Option<Unrepaired>,
) -> Option<&'static str> {
    match (found, unrepaired) {
        (HooksFound::OtherCopy(programs), _) => {
            Some(crate::i18n::agent_hooks_other_copy_line(programs))
        }
        (HooksFound::OwnerUnknown, _) => Some(Text::AgentHooksOwnerUnknown.text()),
        (HooksFound::Unreadable, _) => Some(Text::AgentHooksUnreadable.text()),
        (HooksFound::Unclassified, _) => Some(Text::AgentHooksUnclassified.text()),
        (HooksFound::Declined(reason), _) => Some(reason.text()),
        (HooksFound::Placed(events), _) => Some(crate::i18n::agent_hooks_placed_line(events)),
        (HooksFound::Dead(_) | HooksFound::Drifted, Some(Unrepaired::Held)) => {
            Some(Text::AgentHooksRepairHeld.text())
        }
        (HooksFound::Dead(_) | HooksFound::Drifted, Some(Unrepaired::Refused(reason))) => {
            Some(crate::i18n::agent_hooks_repair_refused_line(reason))
        }
        (
            HooksFound::Nothing
            | HooksFound::Current
            | HooksFound::Drifted
            | HooksFound::Dead(_)
            | HooksFound::Refused(_),
            _,
        ) => None,
    }
}

/// **Whether family `index`'s file holds hooks of a living other copy, or of one the machine will
/// not say about** — the first-run card's question (0.4.10 plan row 67b, ruling N3): a row over
/// either arrives Off, because a take-over is never a first-run act. Read on the caller's thread,
/// beside the card's three `state()` reads, by the start-time scan's own classification.
#[must_use]
pub(crate) fn held_by_another(index: usize) -> bool {
    let family = &HOOKS_FAMILIES[index];
    let Some(path) = family_path(family) else {
        return false;
    };
    // This copy's program cannot be named, so the row's press would be refused
    // (`stable_executable`): the row arrives Off rather than switched on for a refusal.
    let Some(exe) = ownership::this_copy() else {
        return true;
    };
    held_at(family, &path, &exe, &ownership::what_stands)
}

/// [`held_by_another`] over a named file, with the machine's answer about a path handed in.
fn held_at(family: &HooksFamily, path: &Path, exe: &Path, look: &dyn ownership::Look) -> bool {
    Config::resolve(path).is_ok_and(|config| {
        matches!(
            (family.scan)(&config, exe, look),
            HooksFound::OtherCopy(_) | HooksFound::OwnerUnknown
        )
    })
}

/// **The resume row's line** (R5): while resume is on, the agents this build resumes whose hooks
/// are not this copy's — none there, another copy's, a file that cannot be read alike. `None`
/// keeps the row's own sentence, and so does a window whose first scan has not answered.
#[must_use]
pub(crate) fn resume_row_line(
    resume_on: bool,
    known: bool,
    this_copys: [bool; 3],
) -> Option<&'static str> {
    if !resume_on || !known {
        return None;
    }
    let missing: Vec<&str> = HOOKS_FAMILIES
        .iter()
        .zip(this_copys)
        .filter(|(_, ours)| !ours)
        .filter_map(|(family, _)| crate::agent_resume::vendor(family.family))
        .map(|vendor| vendor.title)
        .collect();
    (!missing.is_empty()).then(|| crate::i18n::resume_needs_hooks_line(&missing))
}

#[cfg(test)]
mod tests {
    //! The step's tests (0.4.10 plan row 67b, report §1.8). **Fixtures only**: every agent's file
    //! is a scratch file under the build directory, every copy of Folio a file this test made, and
    //! the machine's answer about a path is either the real one about those files or one the test
    //! hands in. No test here reads an agent's real configuration or a Folio data folder.
    use super::*;
    use crate::attention_ownership::{DeadWhen, PathStands, volume_root_of, what_stands};
    use serde_json::{Value, json};
    use std::fs;

    fn root(name: &str) -> crate::test_support::ScratchFolder {
        crate::test_support::ScratchFolder::create(
            test_scratch_root().join(bt_testpath::unique_name(name)),
        )
    }

    /// A copy of Folio at `<root>/<folder>/folio.exe` (the name is what makes it one).
    fn copy_at(root: &Path, folder: &str) -> PathBuf {
        let exe = root.join(folder).join("folio.exe");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, folder.as_bytes()).unwrap();
        exe
    }

    /// The three families' files under `config`, in the step's order.
    fn files_in(config: &Path) -> [PathBuf; 3] {
        [
            config.join("claude").join("settings.json"),
            config.join("codex").join("config.toml"),
            config.join("copilot").join("hooks").join("folio.json"),
        ]
    }

    fn some(files: &[PathBuf; 3]) -> [Option<PathBuf>; 3] {
        files.clone().map(Some)
    }

    /// Each family installed by the switch's own writer, for `exe`.
    fn install_all(files: &[PathBuf; 3], exe: &Path, data: &Path) {
        type ApplyAt = fn(&Path, Decision, &Path, &Path) -> Outcome;
        let applies: [ApplyAt; 3] = [
            crate::attention_hooks::apply_at,
            crate::attention_codex::apply_at,
            crate::attention_copilot::apply_at,
        ];
        for (file, apply) in files.iter().zip(applies) {
            assert_eq!(
                apply(file, Decision::Install, exe, data),
                Outcome::Installed
            );
        }
    }

    fn bytes(files: &[PathBuf; 3]) -> Vec<Option<Vec<u8>>> {
        files.iter().map(|file| fs::read(file).ok()).collect()
    }

    fn found(scans: &[HooksScan; 3]) -> Vec<HooksFound> {
        scans.iter().map(|scan| scan.found.clone()).collect()
    }

    fn never_held(_: Writer) -> bool {
        false
    }

    /// **A machine a test writes** ([`ownership::Look`]): what stands, by `stands`, and the volume
    /// every missing path leads to — with the number of times that was asked for, which only the
    /// scan's rule does (re-review R2).
    struct MachineAsWritten<F> {
        stands: F,
        volume: Option<PathBuf>,
        volume_asked: std::cell::Cell<usize>,
    }

    impl<F> MachineAsWritten<F> {
        fn new(stands: F, volume: Option<PathBuf>) -> Self {
            Self {
                stands,
                volume,
                volume_asked: std::cell::Cell::new(0),
            }
        }
    }

    impl<F: Fn(&Path) -> PathStands> ownership::Look for MachineAsWritten<F> {
        fn stands(&self, path: &Path) -> PathStands {
            (self.stands)(path)
        }

        fn volume_of(&self, _missing: &Path) -> Option<PathBuf> {
            self.volume_asked.set(self.volume_asked.get() + 1);
            self.volume.clone()
        }
    }

    fn every_family(_: &HooksFamily) -> bool {
        true
    }

    /// T1 — **a dead owner is repaired at start, once, for all three families**: the files name
    /// this copy, the roots are in this copy's marks (the gone copy kept its own data folder), one
    /// notice per family is published, and a second run writes nothing and owes no notice.
    ///
    /// MUTATIONS: the liveness rule answers "alive" for a missing program
    /// (`plant-67b-dead-is-alive-r4.py`) and the scan reads another copy's hooks; the repair writes
    /// nothing (`plant-67b-repair-writes-nothing.py`).
    #[test]
    fn a_dead_owner_is_repaired_at_start_once() {
        let root = root("hooks-step-dead");
        let gone = copy_at(&root, "A gone");
        let here = copy_at(&root, "B here");
        let files = files_in(&root.join("config"));
        let data = root.join("data");
        install_all(&files, &gone, &root.join("gone data"));
        fs::remove_file(&gone).unwrap();
        let step = AgentHooksStep::new();
        let scans = step.scan_and_repair(
            &some(&files),
            &here,
            &data,
            &what_stands,
            never_held,
            &every_family,
        );
        for scan in &scans {
            assert_eq!(
                scan.repaired_from,
                Some(HooksFound::Dead(vec![gone.clone()]))
            );
            assert_eq!(scan.found, HooksFound::Current);
        }
        for file in &files {
            let text = fs::read_to_string(file).unwrap();
            assert!(
                !text.contains("A gone"),
                "the gone copy is still named in {}",
                file.display()
            );
        }
        let marks = crate::shell_integration::profile_marks::Marks::read(&data).unwrap();
        assert!(!marks.agent_config_roots.claude.is_empty());
        assert!(!marks.agent_config_roots.codex.is_empty());
        assert!(!marks.agent_config_roots.copilot.is_empty());
        step.publish_scans(&scans, Readiness::Unknown);
        let snapshot = step.take_snapshot().expect("a run publishes");
        assert_eq!(
            snapshot.notices,
            [
                Text::AgentHooksRepairedClaude,
                Text::AgentHooksRepairedCodex,
                Text::AgentHooksRepairedCopilot
            ]
        );
        assert_eq!(
            snapshot.found,
            [
                HooksFound::Current,
                HooksFound::Current,
                HooksFound::Current
            ]
        );
        let repaired = bytes(&files);
        let again = step.scan_and_repair(
            &some(&files),
            &here,
            &data,
            &what_stands,
            never_held,
            &every_family,
        );
        assert!(again.iter().all(|scan| scan.repaired_from.is_none()));
        assert_eq!(bytes(&files), repaired, "a second run wrote");
        step.publish_scans(&again, Readiness::Unknown);
        assert!(
            step.take_snapshot().unwrap().notices.is_empty(),
            "a notice was said twice"
        );
    }

    /// Row 69c — **on a codex that runs hooks the repair is the hook's**: a gone copy's hook in
    /// `hooks.json` and the `notify` line it left in `config.toml` are repaired in one write — the
    /// hook names this copy, the line is out — and the notice says codex asks to trust the new path.
    ///
    /// MUTATION: `plant-69c-hook-notice-is-the-lines.py` gives the hook's repair the line's notice.
    #[test]
    fn a_gone_copys_codex_hook_and_line_are_repaired_into_the_hook() {
        let root = root("hooks-step-codex-hook");
        let gone = copy_at(&root, "A gone");
        let here = copy_at(&root, "B here");
        let mut files = files_in(&root.join("config"));
        let config_toml = files[1].clone();
        files[1] = config_toml.with_file_name("hooks.json");
        let gone_data = root.join("gone data");
        install_all(&files, &gone, &gone_data);
        assert_eq!(
            crate::attention_codex::apply_at(&config_toml, Decision::Install, &gone, &gone_data),
            Outcome::Installed
        );
        fs::remove_file(&gone).unwrap();
        let step = AgentHooksStep::new();
        let scans = step.scan_and_repair(
            &some(&files),
            &here,
            &root.join("data"),
            &what_stands,
            never_held,
            &every_family,
        );
        assert_eq!(
            scans[1].repaired_from,
            Some(HooksFound::Dead(vec![gone.clone()]))
        );
        assert_eq!(scans[1].found, HooksFound::Current);
        let hook = fs::read_to_string(&files[1]).unwrap();
        assert!(
            hook.contains("B here") && !hook.contains("A gone"),
            "{hook}"
        );
        let line = fs::read_to_string(&config_toml).unwrap();
        assert!(!line.contains("notify"), "the line is out: {line}");
        step.publish_scans(&scans, Readiness::Unknown);
        assert_eq!(
            step.take_snapshot().unwrap().notices,
            [
                Text::AgentHooksRepairedClaude,
                Text::AgentHooksRepairedCodexHook,
                Text::AgentHooksRepairedCopilot
            ]
        );
    }

    /// **A press between the scan and the write is never undone**: the run classified the file
    /// and then the file changed (here, the person's press took Folio's hooks out while the trial's
    /// gate was being asked); the repair's write is of the bytes it classified, so it refuses, the
    /// file keeps the press's bytes, and the other families are repaired as before.
    ///
    /// MUTATION: the run reads the file again for its write (`plant-67b-read-twice.py`): the
    /// repair puts the hooks back over the press.
    #[test]
    fn a_press_between_the_scan_and_the_write_is_never_undone() {
        let root = root("hooks-step-press-between");
        let gone = copy_at(&root, "A gone");
        let here = copy_at(&root, "B here");
        let files = files_in(&root.join("config"));
        let data = root.join("data");
        install_all(&files, &gone, &root.join("gone data"));
        fs::remove_file(&gone).unwrap();
        let pressed = "{\n  \"model\": \"mine\"\n}\n";
        let step = AgentHooksStep::new();
        let scans = step.scan_and_repair(
            &some(&files),
            &here,
            &data,
            &what_stands,
            |_| {
                fs::write(&files[0], pressed).unwrap();
                false
            },
            &every_family,
        );
        assert_eq!(scans[0].repaired_from, None, "the press was written over");
        assert_eq!(fs::read_to_string(&files[0]).unwrap(), pressed);
        for scan in &scans[1..] {
            assert_eq!(
                scan.repaired_from,
                Some(HooksFound::Dead(vec![gone.clone()]))
            );
        }
        step.publish_scans(&scans, Readiness::Unknown);
        assert_eq!(
            step.take_snapshot().unwrap().notices,
            [
                Text::AgentHooksRepairedCodex,
                Text::AgentHooksRepairedCopilot
            ],
            "only what was repaired is said"
        );
    }

    /// R9 — **Copilot's hooks are repaired only within its version gate**: a family whose
    /// gate does not admit the repair is left as it was and said, and the others are repaired.
    #[test]
    fn a_family_outside_its_gate_is_left_for_the_answer() {
        let root = root("hooks-step-gate");
        let gone = copy_at(&root, "A gone");
        let here = copy_at(&root, "B here");
        let files = files_in(&root.join("config"));
        let data = root.join("data");
        install_all(&files, &gone, &root.join("gone data"));
        fs::remove_file(&gone).unwrap();
        let copilot_before = fs::read(&files[2]).unwrap();
        let scans = AgentHooksStep::new().scan_and_repair(
            &some(&files),
            &here,
            &data,
            &what_stands,
            never_held,
            &|family: &HooksFamily| family.tag != "copilot",
        );
        assert_eq!(
            scans[2],
            HooksScan::unrepaired(HooksFound::Dead(vec![gone.clone()]))
        );
        assert_eq!(fs::read(&files[2]).unwrap(), copilot_before);
        assert!(scans[..2].iter().all(|scan| scan.repaired_from.is_some()));
    }

    /// Row 69c (the 69c review's C4) — **codex's repair waits for its version**: before the version
    /// has been asked, a gone copy's `notify` line is said and left, and neither `config.toml` nor
    /// a `hooks.json` beside it is written — a repair then could write a hook a codex below 0.148
    /// skips, in place of the line that works there. Once the version is answered (here: no codex
    /// where the walk looked, unknown, which is current), the same scan repairs it.
    ///
    /// MUTATION: `plant-69c-repair-gate-open.py` opens codex's gate before the version is asked,
    /// and the line is repaired at once.
    #[test]
    fn a_codex_repair_waits_for_the_version() {
        let root = root("hooks-step-codex-gate");
        let gone = copy_at(&root, "A gone");
        let here = copy_at(&root, "B here");
        let files = files_in(&root.join("config"));
        let data = root.join("data");
        install_all(&files, &gone, &root.join("gone data"));
        fs::remove_file(&gone).unwrap();
        let hooks_json = files[1].with_file_name("hooks.json");
        let before = (fs::read(&files[1]).unwrap(), fs::read(&hooks_json).ok());
        let book = std::sync::Mutex::new(crate::attention_codex::VersionBook::new());
        let gate = |family: &HooksFamily| {
            family.tag != "codex" || crate::attention_codex::repair_within_the_gate_in(&book)
        };
        let step = AgentHooksStep::new();
        let scans =
            step.scan_and_repair(&some(&files), &here, &data, &what_stands, never_held, &gate);
        assert_eq!(
            scans[1],
            HooksScan::unrepaired(HooksFound::Dead(vec![gone.clone()]))
        );
        assert_eq!(
            (fs::read(&files[1]).unwrap(), fs::read(&hooks_json).ok()),
            before,
            "both of codex's files byte for byte"
        );
        assert!(scans[0].repaired_from.is_some() && scans[2].repaired_from.is_some());

        assert!(crate::attention_codex::adopt_walk_program_in(
            &book,
            Some(None)
        ));
        let again =
            step.scan_and_repair(&some(&files), &here, &data, &what_stands, never_held, &gate);
        assert_eq!(
            again[1].repaired_from,
            Some(HooksFound::Dead(vec![gone.clone()]))
        );
        let line = fs::read_to_string(&files[1]).unwrap();
        assert!(
            line.contains("B here") && !line.contains("A gone"),
            "{line}"
        );
    }

    /// T2 — **a living other copy's hooks are left, and said by its path** (R5): nothing is
    /// written, the row names the other program, and the resume row counts both resumed agents as
    /// missing.
    #[test]
    fn a_living_other_owner_is_left_and_said() {
        let root = root("hooks-step-living");
        let other = copy_at(&root, "A living");
        let here = copy_at(&root, "B here");
        let files = files_in(&root.join("config"));
        let data = root.join("data");
        install_all(&files, &other, &data);
        let before = bytes(&files);
        let scans = AgentHooksStep::new().scan_and_repair(
            &some(&files),
            &here,
            &data,
            &what_stands,
            never_held,
            &every_family,
        );
        assert_eq!(
            found(&scans),
            vec![HooksFound::OtherCopy(vec![other.clone()]); 3]
        );
        assert_eq!(bytes(&files), before);
        let line = hooks_row_line(&scans[0].found, scans[0].unrepaired)
            .expect("the row says whose hooks these are");
        assert!(line.contains(&other.display().to_string()), "{line}");
        let resume = resume_row_line(
            true,
            true,
            scans.each_ref().map(|scan| scan.found.is_this_copys()),
        )
        .expect("resume cannot work for either agent");
        assert!(
            resume.contains("Claude Code") && resume.contains("Codex"),
            "{resume}"
        );
        assert!(!resume.contains("Copilot"), "{resume}");
    }

    /// The Claude Code entries an install writes, re-spelt in one of the old string templates:
    /// `quote` wraps the program, and `Stop` keeps its `--json -` only when `stop_payload`.
    fn claude_in_old_shape(file: &Path, quote: fn(&Path) -> String, stop_payload: bool) {
        let mut settings: Value = serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap();
        for groups in settings["hooks"].as_object_mut().unwrap().values_mut() {
            for group in groups.as_array_mut().unwrap() {
                for hook in group["hooks"].as_array_mut().unwrap() {
                    let program = PathBuf::from(hook["command"].as_str().unwrap());
                    let args: Vec<String> = hook["args"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|word| word.as_str().unwrap().to_owned())
                        .collect();
                    let payload =
                        args.len() == 4 && (stop_payload || args[1] != "claude-code:Stop");
                    let line = format!(
                        "{} attention {}{}",
                        quote(&program),
                        args[1],
                        if payload { " --json -" } else { "" }
                    );
                    *hook = json!({ "type": "command", "command": line });
                }
            }
        }
        fs::write(file, serde_json::to_string_pretty(&settings).unwrap()).unwrap();
    }

    /// Copilot's entries, re-spelt in one of the old shell columns (`column`, `quote`).
    fn copilot_in_old_shape(file: &Path, column: &str, quote: fn(&Path) -> String) {
        let mut document: Value = serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap();
        for entries in document["hooks"].as_object_mut().unwrap().values_mut() {
            for entry in entries.as_array_mut().unwrap() {
                let program = PathBuf::from(entry["exec"].as_str().unwrap());
                let event = entry["args"][1].as_str().unwrap().to_owned();
                let object = entry.as_object_mut().unwrap();
                object.remove("exec");
                object.remove("args");
                object.insert(
                    column.to_owned(),
                    format!("{} attention {event}", quote(&program)).into(),
                );
            }
        }
        fs::write(file, serde_json::to_string_pretty(&document).unwrap()).unwrap();
    }

    fn double_quoted(program: &Path) -> String {
        format!("\"{}\"", program.display())
    }

    fn single_quoted(program: &Path) -> String {
        format!("'{}'", program.display())
    }

    fn powershell_quoted(program: &Path) -> String {
        format!("& '{}'", program.display())
    }

    /// T3 — **every old shape of this copy's hooks is brought to today's, in place, and today's
    /// shape is left alone**: C1 (whose `Stop` carried no payload, so resume got nothing), C2, C3
    /// with sh quoting, P1 and P2; C4, X1 and P3 are not written.
    ///
    /// MUTATIONS: the drift check answers "current" (`plant-67b-shape-check-skipped-r3.py`); the
    /// repair writes nothing (`plant-67b-repair-writes-nothing.py`).
    #[test]
    fn every_historical_shape_of_this_copy_is_brought_to_the_current_one() {
        type Respell = Box<dyn Fn(&[PathBuf; 3])>;
        let shapes: [(&str, Respell); 5] = [
            (
                "C1",
                Box::new(|files| claude_in_old_shape(&files[0], double_quoted, false)),
            ),
            (
                "C2",
                Box::new(|files| claude_in_old_shape(&files[0], double_quoted, true)),
            ),
            (
                "C3",
                Box::new(|files| claude_in_old_shape(&files[0], single_quoted, true)),
            ),
            (
                "P1",
                Box::new(|files| copilot_in_old_shape(&files[2], "powershell", powershell_quoted)),
            ),
            (
                "P2",
                Box::new(|files| copilot_in_old_shape(&files[2], "bash", single_quoted)),
            ),
        ];
        for (shape, respell) in shapes {
            let root = root(&format!("hooks-step-shape-{shape}"));
            let here = copy_at(&root, "here");
            let files = files_in(&root.join("config"));
            let data = root.join("data");
            install_all(&files, &here, &data);
            let today = bytes(&files);
            respell(&files);
            let step = AgentHooksStep::new();
            let scans = step.scan_and_repair(
                &some(&files),
                &here,
                &data,
                &what_stands,
                never_held,
                &every_family,
            );
            let family = usize::from(shape.starts_with('P')) * 2;
            assert_eq!(
                scans[family].repaired_from,
                Some(HooksFound::Drifted),
                "{shape}"
            );
            assert_eq!(scans[family].found, HooksFound::Current, "{shape}");
            for (index, scan) in scans.iter().enumerate() {
                if index != family {
                    assert_eq!(scan, &HooksScan::unrepaired(HooksFound::Current), "{shape}");
                }
            }
            let settings: Value =
                serde_json::from_slice(&fs::read(&files[family]).unwrap()).unwrap();
            if family == 0 {
                // `Stop` carries its payload again: the words and the session record reach the wire.
                assert_eq!(
                    settings["hooks"]["Stop"][0]["hooks"][0]["args"],
                    json!(["attention", "claude-code:Stop", "--json", "-"]),
                    "{shape}"
                );
            } else {
                assert_eq!(
                    settings,
                    crate::attention_copilot::document_for(&here),
                    "{shape}"
                );
            }
            assert_eq!(
                bytes(&files)[1..2],
                today[1..2],
                "{shape}: Codex's current notify was written"
            );
        }
    }

    /// T3's other half (R3) — **a missing `Stop` is drift, a hand-removed event that carries no
    /// resume is not.**
    #[test]
    fn a_missing_stop_is_drift_and_another_missing_event_is_the_persons() {
        let root = root("hooks-step-missing-stop");
        let here = copy_at(&root, "here");
        let files = files_in(&root.join("config"));
        let data = root.join("data");
        install_all(&files, &here, &data);
        let remove = |event_of: &dyn Fn(&str) -> bool| {
            let mut settings: Value =
                serde_json::from_slice(&fs::read(&files[0]).unwrap()).unwrap();
            let hooks = settings["hooks"].as_object_mut().unwrap();
            let event = hooks
                .keys()
                .find(|event| event_of(event))
                .expect("an event to take out")
                .clone();
            hooks.remove(&event);
            fs::write(&files[0], serde_json::to_string_pretty(&settings).unwrap()).unwrap();
        };
        remove(&|event| event != "Stop");
        let theirs = fs::read(&files[0]).unwrap();
        let step = AgentHooksStep::new();
        let left = step.scan_and_repair(
            &some(&files),
            &here,
            &data,
            &what_stands,
            never_held,
            &every_family,
        );
        assert_eq!(left[0], HooksScan::unrepaired(HooksFound::Current));
        assert_eq!(
            fs::read(&files[0]).unwrap(),
            theirs,
            "a person's own removal was undone"
        );
        remove(&|event| event == "Stop");
        let put_back = step.scan_and_repair(
            &some(&files),
            &here,
            &data,
            &what_stands,
            never_held,
            &every_family,
        );
        assert_eq!(put_back[0].repaired_from, Some(HooksFound::Drifted));
        let settings: Value = serde_json::from_slice(&fs::read(&files[0]).unwrap()).unwrap();
        assert!(
            settings["hooks"].get("Stop").is_some(),
            "resume's event was not put back"
        );
    }

    /// T4 — **with no hooks and resume on, the resume row names the agents it cannot bring back**;
    /// with resume off, before the first scan, or with both resumed agents hooked, it says its
    /// own sentence; Copilot is never named (R9).
    ///
    /// MUTATION: the resume row's line is dropped (`plant-67b-resume-line-dropped.py`).
    #[test]
    fn no_hooks_with_resume_on_is_said_on_the_resume_row() {
        let root = root("hooks-step-none");
        let files = files_in(&root.join("config"));
        let scans = AgentHooksStep::new().scan_and_repair(
            &some(&files),
            &copy_at(&root, "here"),
            &root.join("data"),
            &what_stands,
            never_held,
            &every_family,
        );
        assert_eq!(found(&scans), vec![HooksFound::Nothing; 3]);
        assert!(
            files.iter().all(|file| !file.exists()),
            "a scan created a file"
        );
        let hooked = scans.each_ref().map(|scan| scan.found.is_this_copys());
        let line = resume_row_line(true, true, hooked).expect("resume cannot work");
        assert_eq!(
            line,
            crate::i18n::resume_needs_hooks_line(&["Claude Code", "Codex"])
        );
        assert_eq!(resume_row_line(false, true, hooked), None);
        assert_eq!(resume_row_line(true, false, hooked), None);
        assert_eq!(resume_row_line(true, true, [true, true, false]), None);
        assert_eq!(
            resume_row_line(true, true, [true, false, true]),
            Some(crate::i18n::resume_needs_hooks_line(&["Codex"]))
        );
    }

    /// T5 — **a file Folio cannot read is never written, and is said** (D2: the never-written
    /// byte-order-mark case is not one of these: 67a's writer reads past a mark and keeps it).
    #[test]
    fn a_file_folio_cannot_parse_is_never_written_and_is_said() {
        let cases: [(usize, &str); 6] = [
            (0, "{\"hooks\": {\"Stop\": [\n"),
            (0, "{\n  // a comment\n  \"model\": \"opus\"\n}\n"),
            (0, "{\"hooks\": [\"a future shape\"]}\n"),
            (1, "notify = { program = \"x\" }\n"),
            (1, "notify = [\n"),
            (2, "{\"version\": 999, \"hooks\": {}}\n"),
        ];
        for (number, (family, text)) in cases.into_iter().enumerate() {
            let root = root(&format!("hooks-step-unreadable-{number}"));
            let files = files_in(&root.join("config"));
            fs::create_dir_all(files[family].parent().unwrap()).unwrap();
            fs::write(&files[family], text).unwrap();
            let step = AgentHooksStep::new();
            let scans = step.scan_and_repair(
                &some(&files),
                &copy_at(&root, "here"),
                &root.join("data"),
                &what_stands,
                never_held,
                &every_family,
            );
            assert_eq!(scans[family].found, HooksFound::Unreadable, "{text}");
            assert_eq!(fs::read_to_string(&files[family]).unwrap(), text);
            assert_eq!(
                hooks_row_line(&scans[family].found, scans[family].unrepaired),
                Some(Text::AgentHooksUnreadable.text())
            );
            step.publish_scans(&scans, Readiness::Unknown);
            assert!(step.take_snapshot().unwrap().notices.is_empty());
        }
    }

    /// Settings with Folio's entries — any command naming a `folio.exe` — and the groups and
    /// events they leave empty taken out: what the person's own configuration is.
    fn without_folio(settings: &Value) -> Value {
        let mut settings = settings.clone();
        if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
            for groups in hooks.values_mut() {
                let groups = groups.as_array_mut().unwrap();
                for group in groups.iter_mut() {
                    group["hooks"].as_array_mut().unwrap().retain(|hook| {
                        !hook["command"]
                            .as_str()
                            .is_some_and(|command| command.contains("folio.exe"))
                    });
                }
                groups.retain(|group| !group["hooks"].as_array().unwrap().is_empty());
            }
            hooks.retain(|_, groups| !groups.as_array().unwrap().is_empty());
        }
        settings
    }

    /// T6 (the hard rule) — **an unattended repair changes Folio's own entries and nothing else**:
    /// a file with the person's hooks before and after Folio's on one event, one in the same group
    /// as Folio's, root keys in no order, four-space indentation, CRLF, no newline at the end, an
    /// escaped character and a number spelled `1.50`. Every line of the person's is still there, in
    /// its order, and the file read as values without Folio's entries is what it was.
    #[test]
    fn a_repair_touches_only_folios_own_entries() {
        let root = root("hooks-step-hard-rule");
        let gone = root.join("A gone").join("folio.exe");
        let here = copy_at(&root, "B here");
        let files = files_in(&root.join("config"));
        let gone_json = serde_json::to_string(&gone.display().to_string()).unwrap();
        let text = [
            "{",
            "    \"zeta\": 1.50,",
            "    \"hooks\": {",
            "        \"Stop\": [",
            "            {\"hooks\": [{\"type\": \"command\", \"command\": \"my-before \\u00e9\"}]},",
            &format!(
                "            {{\"hooks\": [{{\"type\": \"command\", \"command\": \"mine\"}}, {{\"type\": \"command\", \"command\": {gone_json}, \"args\": [\"attention\", \"claude-code:Stop\", \"--json\", \"-\"], \"async\": true}}]}},"
            ),
            "            {\"hooks\": [{\"type\": \"command\", \"command\": \"my-after\"}]}",
            "        ]",
            "    },",
            "    \"alpha\": {\"keep\": [3, 2, 1]}",
            "}",
        ]
        .join("\r\n");
        fs::create_dir_all(files[0].parent().unwrap()).unwrap();
        fs::write(&files[0], &text).unwrap();
        let scans = AgentHooksStep::new().scan_and_repair(
            &some(&files),
            &here,
            &root.join("data"),
            &what_stands,
            never_held,
            &every_family,
        );
        assert_eq!(
            scans[0].repaired_from,
            Some(HooksFound::Dead(vec![gone.clone()]))
        );
        let written = fs::read_to_string(&files[0]).unwrap();
        assert!(!written.ends_with('\n'), "a newline was added at the end");
        assert!(
            !written.replace("\r\n", "").contains('\n'),
            "a line ending was changed"
        );
        // In order, and each as it was — but for the separator an entry appended after it needs.
        let mut lines = written.split("\r\n");
        for theirs in text
            .split("\r\n")
            .filter(|line| !line.contains("folio.exe"))
        {
            assert!(
                lines.any(|line| line == theirs || line.strip_suffix(',') == Some(theirs)),
                "the person's line {theirs:?} moved or changed:\n{written}"
            );
        }
        // The line Folio's entry shared: the person's entry, and the group's bytes before it, as
        // they were.
        let shared = "            {\"hooks\": [{\"type\": \"command\", \"command\": \"mine\"}";
        assert!(
            written.split("\r\n").any(|line| line.starts_with(shared)),
            "the person's entry beside Folio's moved or changed:\n{written}"
        );
        let before: Value = serde_json::from_str(&text).unwrap();
        let after: Value = serde_json::from_str(&written).unwrap();
        assert_eq!(without_folio(&after), without_folio(&before));
    }

    /// T7 — **an owner the machine will not say about is not dead**: nothing is written, and the
    /// row says the owner could not be verified.
    #[test]
    fn an_owner_that_cannot_be_asked_about_is_not_dead() {
        let root = root("hooks-step-unknown");
        let asked = copy_at(&root, "A asked");
        let here = copy_at(&root, "B here");
        let files = files_in(&root.join("config"));
        let data = root.join("data");
        install_all(&files, &asked, &data);
        let before = bytes(&files);
        let look = |path: &Path| {
            if path == asked {
                PathStands::Unknown
            } else {
                what_stands(path)
            }
        };
        let scans = AgentHooksStep::new().scan_and_repair(
            &some(&files),
            &here,
            &data,
            &look,
            never_held,
            &every_family,
        );
        assert_eq!(found(&scans), vec![HooksFound::OwnerUnknown; 3]);
        assert_eq!(bytes(&files), before);
        assert_eq!(
            hooks_row_line(&scans[0].found, scans[0].unrepaired),
            Some(Text::AgentHooksOwnerUnknown.text())
        );
    }

    /// T8 (R4) — **a missing owner on a volume that is not there is not dead**: an unplugged
    /// drive's copy keeps its hooks.
    ///
    /// MUTATION: the volume is not asked about (`plant-67b-no-volume-check-r4.py`).
    #[test]
    fn an_owner_on_an_absent_volume_is_not_dead() {
        let root = root("hooks-step-volume");
        let away = copy_at(&root, "A away");
        let here = copy_at(&root, "B here");
        let files = files_in(&root.join("config"));
        let data = root.join("data");
        install_all(&files, &away, &data);
        let before = bytes(&files);
        let volume = volume_root_of(&away).expect("a fixture path names its volume");
        let look = MachineAsWritten::new(
            |path: &Path| {
                if path == away || path == volume {
                    PathStands::Missing
                } else {
                    what_stands(path)
                }
            },
            Some(volume.clone()),
        );
        let scans = AgentHooksStep::new().scan_and_repair(
            &some(&files),
            &here,
            &data,
            &look,
            never_held,
            &every_family,
        );
        assert_eq!(found(&scans), vec![HooksFound::OwnerUnknown; 3]);
        assert_eq!(bytes(&files), before);
        // The same owner on a volume that answers is dead, by the same rule.
        let answers = MachineAsWritten::new(
            |path: &Path| {
                if path == away {
                    PathStands::Missing
                } else {
                    what_stands(path)
                }
            },
            Some(volume),
        );
        assert_eq!(
            ownership::other_live_by(
                &away,
                &here,
                &answers,
                DeadWhen::MissingOnAVolumeThatIsThere
            ),
            Ok(false)
        );
    }

    /// **R4 is the scan's rule, not the press's**: a person's press and the uninstall decide on
    /// the machine as it stands (an owner that is not there is gone, design §6.2), so an unplugged
    /// drive's copy never keeps a person from turning hooks off or from uninstalling; the scan,
    /// which writes with nobody asking, calls the same owner unknown. And an operand that names no
    /// file — a bare `folio.exe` an old build wrote, a placeholder — is gone to a press and cannot
    /// be asked about by the scan, which leaves it and says so. A press never walks the way to a
    /// missing owner's volume: only the scan's rule asks for it (re-review R2).
    ///
    /// MUTATIONS: the scan's rule calls an operand that names no file dead
    /// (`plant-67b-unaskable-operand-dead.py`); the volume is walked whoever asks
    /// (`plant-67b-volume-walked-eagerly.py`).
    #[test]
    fn the_volume_rule_is_the_scans_and_a_press_decides_on_the_machine_as_it_stands() {
        let root = root("hooks-step-rules");
        let here = copy_at(&root, "B here");
        let away = root.join("A away").join("folio.exe");
        let volume = volume_root_of(&away).expect("a fixture path names its volume");
        let unplugged = MachineAsWritten::new(
            |path: &Path| {
                if path == away || path == volume {
                    PathStands::Missing
                } else {
                    what_stands(path)
                }
            },
            Some(volume.clone()),
        );
        assert_eq!(
            ownership::other_live_by(&away, &here, &unplugged, DeadWhen::Missing),
            Ok(false),
            "a press takes an unplugged drive's copy for gone"
        );
        // Re-review R2: a press never walks the way to the volume; only the scan's rule asks.
        assert_eq!(
            unplugged.volume_asked.get(),
            0,
            "a press asked for the volume"
        );
        assert_eq!(
            ownership::other_live_by(
                &away,
                &here,
                &unplugged,
                DeadWhen::MissingOnAVolumeThatIsThere
            ),
            Err(Text::AgentHooksOwnerUnknown.text()),
            "the scan does not"
        );
        assert_eq!(unplugged.volume_asked.get(), 1);
        for operand in ["folio.exe", "${HOME}/folio.exe"] {
            let operand = Path::new(operand);
            assert_eq!(
                ownership::other_live_by(operand, &here, &what_stands, DeadWhen::Missing),
                Ok(false),
                "a press removes {}",
                operand.display()
            );
            assert_eq!(
                ownership::other_live_by(
                    operand,
                    &here,
                    &what_stands,
                    DeadWhen::MissingOnAVolumeThatIsThere
                ),
                Err(Text::AgentHooksOwnerUnknown.text()),
                "the scan leaves {}",
                operand.display()
            );
        }
    }

    /// The volume rule's spellings, read off the path alone.
    #[test]
    fn a_volume_is_the_drive_the_share_or_the_mount() {
        // The rooted spellings read the same on every host: a Windows path compares by its
        // components, so `/Volumes\Stick` is `/Volumes/Stick` there.
        assert_eq!(
            volume_root_of(Path::new("/Volumes/Stick/Folio.app/Contents/MacOS/folio")),
            Some(PathBuf::from("/Volumes/Stick"))
        );
        assert_eq!(
            volume_root_of(Path::new("/Applications/Folio.app/Contents/MacOS/folio")),
            Some(PathBuf::from("/"))
        );
        assert_eq!(volume_root_of(Path::new("Folio/folio")), None);
        // A drive and a share are prefixes only where the host's paths have them.
        if bt_platform::host_platform() == bt_platform::HostPlatform::Windows {
            assert_eq!(
                volume_root_of(Path::new(r"D:\Apps\Folio\folio.exe")),
                Some(PathBuf::from(r"D:\"))
            );
            assert_eq!(
                volume_root_of(Path::new(r"\\server\share\Folio\folio.exe")),
                Some(PathBuf::from(r"\\server\share\"))
            );
            assert_eq!(volume_root_of(Path::new(r"D:Folio\folio.exe")), None);
        }
    }

    /// T9 — **a repair waits for the trial to be committed**: during a trial the gate is asked
    /// once, records `AgentHooksRepair`, and nothing is written while the classification is still
    /// answered; the commit releases the writer, and the run it starts repairs.
    ///
    /// MUTATION: the gate is not asked (`plant-67b-trial-not-asked.py`).
    #[test]
    fn a_repair_waits_for_the_trial_to_be_committed() {
        use crate::update_trial::{Gate, watch};
        use crate::update_txn::{Inventories, Journal, Layout, Phase, TxnId};

        let root = root("hooks-step-trial");
        let gone = copy_at(&root, "A gone");
        let here = copy_at(&root, "B here");
        let files = files_in(&root.join("config"));
        let data = root.join("data");
        install_all(&files, &gone, &data);
        fs::remove_file(&gone).unwrap();
        let before = bytes(&files);
        let gate = Gate::new();
        let mut asked = 0;
        let step = AgentHooksStep::new();
        let held = step.scan_and_repair(
            &some(&files),
            &here,
            &data,
            &what_stands,
            |writer| {
                asked += 1;
                gate.defer(true, writer)
            },
            &every_family,
        );
        assert_eq!(asked, 1, "the gate is asked once per run");
        assert_eq!(gate.pending(), vec![Writer::AgentHooksRepair]);
        assert_eq!(found(&held), vec![HooksFound::Dead(vec![gone.clone()]); 3]);
        assert!(held.iter().all(|scan| scan.repaired_from.is_none()));
        assert_eq!(
            bytes(&files),
            before,
            "a trial wrote another program's file"
        );

        let txn = TxnId::new([0x67; 16]);
        let mut journal = Journal::allocate(
            txn,
            "rescue".to_owned(),
            Layout::Members(Inventories {
                old_shipped: Vec::new(),
                old_present: Vec::new(),
                new: Vec::new(),
            }),
        );
        journal.body.phase = Phase::Committed;
        let path = root.join("journal.json");
        fs::write(&path, journal.encode()).unwrap();
        watch(
            &gate,
            (
                &path,
                crate::update_trial::Mark::on_disk(
                    &path.with_file_name(crate::update_txn::UNKEPT_FILE),
                ),
                &path.with_file_name("health-test"),
            ),
            txn,
            crate::update_trial::Cadence::every(std::time::Duration::from_millis(5)),
            crate::update_trial::Told::to_the_log(&|| {}),
            None,
            &mut crate::update_trial::watchdog_asleep(),
        );
        assert_eq!(gate.take_released(), vec![Writer::AgentHooksRepair]);
        let released = step.scan_and_repair(
            &some(&files),
            &here,
            &data,
            &what_stands,
            |writer| gate.defer(true, writer),
            &every_family,
        );
        assert!(
            released
                .iter()
                .all(|scan| scan.repaired_from == Some(HooksFound::Dead(vec![gone.clone()])))
        );
    }

    /// T10 — **a repair never writes while a press holds its family's latch**, and the latch is
    /// let go after the repair that took it, so a press after it is not kept out.
    ///
    /// MUTATION: the repair does not take the latch (`plant-67b-latch-not-taken.py`).
    #[test]
    fn a_repair_never_starts_while_a_press_holds_the_latch() {
        let root = root("hooks-step-latch");
        let gone = copy_at(&root, "A gone");
        let here = copy_at(&root, "B here");
        let files = files_in(&root.join("config"));
        let data = root.join("data");
        install_all(&files, &gone, &data);
        fs::remove_file(&gone).unwrap();
        let claude = fs::read(&files[0]).unwrap();
        let step = AgentHooksStep::new();
        let press = step.take_latch(0).expect("nobody holds it");
        let scans = step.scan_and_repair(
            &some(&files),
            &here,
            &data,
            &what_stands,
            never_held,
            &every_family,
        );
        assert_eq!(
            scans[0],
            HooksScan::unrepaired(HooksFound::Dead(vec![gone.clone()]))
        );
        assert_eq!(
            fs::read(&files[0]).unwrap(),
            claude,
            "a repair wrote beside a press"
        );
        assert!(scans[1..].iter().all(|scan| scan.repaired_from.is_some()));
        drop(press);
        assert!(
            step.take_latch(1).is_some(),
            "the repair kept Codex's latch"
        );
        let after = step.scan_and_repair(
            &some(&files),
            &here,
            &data,
            &what_stands,
            never_held,
            &every_family,
        );
        assert!(
            after[0].repaired_from.is_some(),
            "the press let go and the repair still waits"
        );
    }

    /// T11 — **an entry that looks like Folio's in a shape this build cannot read is left byte for
    /// byte and said**, and no repair writes beside it; a person's own program that speaks the
    /// same verb is theirs, not one of these.
    #[test]
    fn an_unclassifiable_folio_like_entry_is_left_and_said() {
        let root = root("hooks-step-unclassified");
        let gone = root.join("A gone").join("folio.exe");
        let gone_json = serde_json::to_string(&gone.display().to_string()).unwrap();
        let files = files_in(&root.join("config"));
        let texts = [
            format!(
                "{{\"hooks\": {{\"Stop\": [{{\"hooks\": [{{\"type\": \"command\", \"command\": {gone_json}, \"args\": [\"attention\", \"claude-code:Stop\"], \"timeout\": 5}}]}}]}}}}\n"
            ),
            format!("notify = [{gone_json}, \"attention\", \"codex:agent-turn-complete\"]\n"),
            format!(
                "{{\"version\": 1, \"hooks\": {{\"sessionStart\": [{{\"exec\": {gone_json}, \"args\": [\"attention\", \"copilot:sessionStart\"], \"retries\": 2}}]}}}}\n"
            ),
        ];
        for (file, text) in files.iter().zip(&texts) {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        }
        let scans = AgentHooksStep::new().scan_and_repair(
            &some(&files),
            &copy_at(&root, "B here"),
            &root.join("data"),
            &what_stands,
            never_held,
            &every_family,
        );
        assert_eq!(found(&scans), vec![HooksFound::Unclassified; 3]);
        for (file, text) in files.iter().zip(&texts) {
            assert_eq!(&fs::read_to_string(file).unwrap(), text);
        }
        assert_eq!(
            hooks_row_line(&scans[0].found, scans[0].unrepaired),
            Some(Text::AgentHooksUnclassified.text())
        );
        fs::write(
            &files[0],
            "{\"hooks\": {\"Stop\": [{\"hooks\": [{\"type\": \"command\", \"command\": \"mytool attention claude-code:Stop\", \"timeout\": 5}]}]}}\n",
        )
        .unwrap();
        let theirs = AgentHooksStep::new().scan_and_repair(
            &some(&files),
            &copy_at(&root, "B here"),
            &root.join("data"),
            &what_stands,
            never_held,
            &every_family,
        );
        assert_eq!(theirs[0].found, HooksFound::Nothing);
    }

    /// Somebody else's `notify`, somebody else's hook file under Folio's name, and a file that is a
    /// link out of the agent's folder: none is written, and each row says why — the first two
    /// with a switch that still presses, the third with an inert one.
    #[test]
    fn what_is_not_folios_to_write_is_said_and_left() {
        let root = root("hooks-step-declined");
        let files = files_in(&root.join("config"));
        let codex = "notify = [\"my-notifier\", \"--loud\"]\n";
        let copilot = "{\"version\": 1, \"hooks\": {\"sessionStart\": [{\"exec\": \"mine\"}]}}\n";
        for (file, text) in files[1..].iter().zip([codex, copilot]) {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        }
        let scans = AgentHooksStep::new().scan_and_repair(
            &some(&files),
            &copy_at(&root, "here"),
            &root.join("data"),
            &what_stands,
            never_held,
            &every_family,
        );
        assert_eq!(
            found(&scans),
            [
                HooksFound::Nothing,
                HooksFound::Declined(Text::AgentCodexNotifyTaken.into()),
                HooksFound::Declined(Text::AgentCopilotFileTaken.into())
            ]
        );
        assert_eq!(fs::read_to_string(&files[1]).unwrap(), codex);
        assert_eq!(fs::read_to_string(&files[2]).unwrap(), copilot);
        assert_eq!(
            hooks_row_line(&scans[1].found, scans[1].unrepaired),
            Some(Text::AgentCodexNotifyTaken.text())
        );
    }

    /// T12 — **the window's creation reads no agent's file**: `Runtime::create` asks no row state
    /// and no readiness, and the adoption reads none. What the window thread still reads is the
    /// presses' (R11) and the first-run card's three `state()` questions and three
    /// `held_by_another` ones (ruling N3), asked once on a first run (report §2.2).
    #[test]
    fn the_window_thread_reads_no_agent_file_at_start() {
        let create = crate::test_support::method_body("Runtime", "create");
        for read in [
            "row_state(",
            "attention_copilot::readiness(",
            "attention_hooks::state(",
            "attention_codex::state(",
            "attention_copilot::state(",
        ] {
            assert!(
                !create.contains(read),
                "the launch reads an agent's file: {read}"
            );
        }
        let adopt = crate::test_support::free_fn_body("adopt_agent_hooks_snapshot");
        assert!(!adopt.contains("row_state(") && !adopt.contains("readiness()"));
    }

    /// **The pass runs at the moments the rulings name** (R7): the walk's answer (its first
    /// included), the Agents page's open edge, a press, the trial's commit, and a new language.
    /// And a press never writes beside the repair (review N2).
    ///
    /// MUTATION: the press ignores the latch's answer (`plant-67b-press-beside-repair.py`).
    #[test]
    fn the_pass_runs_at_the_moments_the_rulings_name() {
        use crate::test_support::{free_fn_body, method_body};
        let begins = "carry_pass::begin_carry_pass()";
        let walk = free_fn_body("adopt_program_answers");
        assert!(walk.contains(begins) && walk.contains("AGENT_HOMES.answered()"));
        assert!(method_body("Runtime", "refresh_agent_rows").contains(begins));
        for press in [
            "apply_claude_hooks",
            "apply_codex_notify",
            "apply_copilot_hooks",
        ] {
            let body = method_body("Runtime", press);
            assert!(
                body.contains("crate::agent_row_asked_again(self.app,"),
                "{press} does not ask for a scan"
            );
            // Review N2: a press that finds the repair holding the latch writes nothing.
            assert!(
                body.contains("let Some(press) = agent_hooks_step::AGENT_HOOKS.take_latch(")
                    && body.contains("return self.press_beside_a_repair(")
                    && body.contains("drop(press);"),
                "{press} writes beside a repair"
            );
        }
        let beside = method_body("Runtime", "press_beside_a_repair");
        assert!(
            beside.contains("crate::agent_row_asked_again(self.app, family)")
                && beside.contains("Text::AgentHooksBeingRepaired"),
            "a press beside a repair does not say so"
        );
        assert!(free_fn_body("agent_row_asked_again").contains(begins));
        // A row's refused repair carries the writer's sentence: a new language says it again.
        assert!(method_body("Runtime", "adopt_new_language").contains(begins));
        assert!(
            free_fn_body("release_trial_writer")
                .contains("Writer::AgentHooksRepair => carry_pass::begin_carry_pass()")
        );
    }

    /// **The rows say what the scan found, and a refusal still wins**: a line leaves the switch
    /// pressable, a refused file's reason makes it inert; the resume row says its line while it
    /// has one.
    #[test]
    fn the_agents_rows_say_the_scan_and_keep_their_switches() {
        use crate::settings::{SettingsRow, SettingsValues};
        let line = Text::AgentHooksUnclassified.text();
        let said = SettingsValues {
            agent_row_lines: [Some(line), Some(line), Some(line)],
            resume_agents_line: Some(crate::i18n::resume_needs_hooks_line(&["Codex"])),
            ..SettingsValues::sample()
        };
        for row in [
            SettingsRow::ClaudeHooks,
            SettingsRow::CodexNotify,
            SettingsRow::CopilotHooks,
        ] {
            assert_eq!(row.description(&said), line);
            assert!(row.option_enabled(0, &said) && row.option_enabled(1, &said));
        }
        assert_eq!(
            SettingsRow::ResumeAgents.description(&said),
            crate::i18n::resume_needs_hooks_line(&["Codex"])
        );
        let refused = SettingsValues {
            agent_config_refusals: [Some(Text::AgentConfigLink.text()), None, None],
            ..said
        };
        assert_eq!(
            SettingsRow::ClaudeHooks.description(&refused),
            Text::AgentConfigLink.text()
        );
        assert_eq!(
            SettingsRow::ResumeAgents.description(&SettingsValues::sample()),
            Text::DescResumeAgents.text()
        );
    }

    /// The product's families read the agents' folders through the walk's cell, and a test
    /// process's step never does (`family_path`).
    #[test]
    fn a_test_process_never_hands_the_step_an_accounts_own_file() {
        assert_eq!(
            (HOOKS_FAMILIES[0].path)(),
            crate::attention_hooks::settings_path()
        );
        let scratch = test_process_agent_homes();
        assert!(scratch.starts_with(test_scratch_root()));
        for family in &HOOKS_FAMILIES {
            assert!(family_path(family).is_some_and(|path| path.starts_with(scratch)));
        }
    }

    /// The settings file as a value, and written back.
    fn edit_json(file: &Path, edit: impl FnOnce(&mut Value)) {
        let mut value: Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
        edit(&mut value);
        fs::write(file, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    }

    /// Folio's `Notification.agent_needs_input` entry, read out of the group it was written in.
    fn needs_input(settings: &Value) -> Option<(usize, Value)> {
        settings["hooks"]["Notification"]
            .as_array()?
            .iter()
            .enumerate()
            .find(|(_, group)| group["matcher"] == "agent_needs_input")
            .map(|(at, group)| (at, group["hooks"][0].clone()))
    }

    /// N1 (ruling 2026-11-05) — **a person's edit of this copy's own entries is theirs**: `async`
    /// set false on `Stop`, a copy of `Stop` put under `SubagentStop`, Folio's
    /// `Notification.agent_needs_input` entry moved into the person's own `Notification` group,
    /// and Copilot's `timeoutSec` changed on one entry. The scan calls none of them drift, writes
    /// nothing, and the row names the event Folio did not put its entry under. When a real old
    /// shape is repaired beside them (`Stop` without its payload; one Copilot entry in the old
    /// `powershell` column), only that is rewritten, and every edit of the person's stays.
    ///
    /// MUTATION: the drift check compares the whole entry again (`plant-67b-whole-entry-drift.py`).
    #[test]
    fn a_persons_edit_of_this_copys_own_entry_is_left() {
        let root = root("hooks-step-persons-edit");
        let here = copy_at(&root, "here");
        let files = files_in(&root.join("config"));
        let data = root.join("data");
        install_all(&files, &here, &data);
        edit_json(&files[0], |settings| {
            let stop = &mut settings["hooks"]["Stop"][0]["hooks"][0];
            stop["async"] = json!(false);
            let copy = stop.clone();
            settings["hooks"]["SubagentStop"] = json!([{ "hooks": [copy] }]);
            let (at, entry) = needs_input(settings).expect("the entry an install writes");
            let groups = settings["hooks"]["Notification"].as_array_mut().unwrap();
            groups.remove(at);
            groups.push(json!({ "hooks": [{ "type": "command", "command": "say mine" }, entry] }));
        });
        edit_json(&files[2], |document| {
            let events = document["hooks"].as_object_mut().unwrap();
            let first = events.values_mut().next().unwrap();
            first[0]["timeoutSec"] = json!(30);
        });
        let edited = bytes(&files);
        let step = AgentHooksStep::new();
        let scans = step.scan_and_repair(
            &some(&files),
            &here,
            &data,
            &what_stands,
            never_held,
            &every_family,
        );
        let placed = HooksFound::Placed(vec!["SubagentStop".to_owned()]);
        assert_eq!(scans[0], HooksScan::unrepaired(placed.clone()));
        assert_eq!(scans[2], HooksScan::unrepaired(HooksFound::Current));
        assert_eq!(bytes(&files), edited, "a person's edit was written over");
        let line = hooks_row_line(&scans[0].found, scans[0].unrepaired).expect("said");
        assert_eq!(
            line,
            crate::i18n::agent_hooks_placed_line(&["SubagentStop".to_owned()])
        );
        assert!(scans.iter().all(|scan| scan.found.is_this_copys()));

        // A real old shape beside the edits: only it is rewritten.
        edit_json(&files[0], |settings| {
            settings["hooks"]["Stop"][0]["hooks"][0]["args"] =
                json!(["attention", "claude-code:Stop"]);
        });
        edit_json(&files[2], |document| {
            let events = document["hooks"].as_object_mut().unwrap();
            let last = events.values_mut().last().unwrap();
            let entry = last[0].as_object_mut().unwrap();
            let program = entry.remove("exec").unwrap();
            let request = entry.remove("args").unwrap()[1].clone();
            entry.insert(
                "powershell".to_owned(),
                json!(format!(
                    "& '{}' attention {}",
                    program.as_str().unwrap(),
                    request.as_str().unwrap()
                )),
            );
        });
        let repaired = step.scan_and_repair(
            &some(&files),
            &here,
            &data,
            &what_stands,
            never_held,
            &every_family,
        );
        assert_eq!(repaired[0].repaired_from, Some(HooksFound::Drifted));
        assert_eq!(repaired[0].found, placed);
        assert_eq!(repaired[2].repaired_from, Some(HooksFound::Drifted));
        assert_eq!(repaired[2].found, HooksFound::Current);
        let settings: Value = serde_json::from_slice(&fs::read(&files[0]).unwrap()).unwrap();
        let stop = &settings["hooks"]["Stop"][0]["hooks"][0];
        assert_eq!(
            stop["args"],
            json!(["attention", "claude-code:Stop", "--json", "-"])
        );
        assert_eq!(
            stop["async"],
            json!(false),
            "the person's async was taken back"
        );
        assert!(
            settings["hooks"]["SubagentStop"].is_array(),
            "the person's copy under another event was taken out"
        );
        assert!(
            needs_input(&settings).is_none(),
            "the moved entry was put back in a group of Folio's"
        );
        assert!(
            settings["hooks"]["Notification"]
                .as_array()
                .unwrap()
                .iter()
                .any(|group| group.get("matcher").is_none()
                    && group["hooks"].as_array().unwrap().len() == 2),
            "the moved entry left the person's group"
        );
        let document: Value = serde_json::from_slice(&fs::read(&files[2]).unwrap()).unwrap();
        let first = document["hooks"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap();
        assert_eq!(
            first[0]["timeoutSec"],
            json!(30),
            "the person's timeout was taken back"
        );
        assert!(
            !fs::read_to_string(&files[2])
                .unwrap()
                .contains("powershell")
        );
    }

    /// Re-review R1 (2026-11-05) — **a gone copy's entry the person placed is pointed at this copy
    /// where it stands**: copy A's `Stop` entry copied under `SubagentStop` with `async` set false,
    /// and A's Copilot `agentStop` entry copied under `subagentStop` with its own `timeoutSec`. A is
    /// gone, and the start-time repair claims the wanted entries; the two placed ones are not
    /// deleted but name this copy, keep the person's keys, and the rows say they are the person's.
    /// Whether a person's placing survives does not hang on whether Folio moved.
    ///
    /// MUTATIONS: an install deletes another copy's unclaimed entry again, Claude Code's
    /// (`plant-67b-placed-deleted.py`) or Copilot's (`plant-67b-placed-deleted-copilot.py`).
    #[test]
    fn a_gone_copys_placed_entry_is_pointed_here_where_it_stands() {
        let root = root("hooks-step-gone-placed");
        let gone = copy_at(&root, "A gone");
        let here = copy_at(&root, "B here");
        let files = files_in(&root.join("config"));
        let data = root.join("data");
        install_all(&files, &gone, &root.join("gone data"));
        edit_json(&files[0], |settings| {
            let mut copy = settings["hooks"]["Stop"][0]["hooks"][0].clone();
            copy["async"] = json!(false);
            settings["hooks"]["SubagentStop"] = json!([{ "hooks": [copy] }]);
        });
        edit_json(&files[2], |document| {
            let mut copy = document["hooks"]["agentStop"][0].clone();
            copy["timeoutSec"] = json!(45);
            document["hooks"]["subagentStop"] = json!([copy]);
        });
        fs::remove_file(&gone).unwrap();
        let scans = AgentHooksStep::new().scan_and_repair(
            &some(&files),
            &here,
            &data,
            &what_stands,
            never_held,
            &every_family,
        );
        let dead = Some(HooksFound::Dead(vec![gone.clone()]));
        assert_eq!(scans[0].repaired_from, dead);
        assert_eq!(
            scans[0].found,
            HooksFound::Placed(vec!["SubagentStop".to_owned()])
        );
        assert_eq!(scans[2].repaired_from, dead);
        assert_eq!(
            scans[2].found,
            HooksFound::Placed(vec!["subagentStop".to_owned()])
        );
        let program = json!(here.to_string_lossy());
        let settings: Value = serde_json::from_slice(&fs::read(&files[0]).unwrap()).unwrap();
        let placed = &settings["hooks"]["SubagentStop"][0]["hooks"][0];
        assert_eq!(
            placed["command"], program,
            "the placed entry was not pointed here"
        );
        assert_eq!(
            placed["args"],
            settings["hooks"]["Stop"][0]["hooks"][0]["args"]
        );
        assert_eq!(
            placed["async"],
            json!(false),
            "the person's async was taken back"
        );
        let document: Value = serde_json::from_slice(&fs::read(&files[2]).unwrap()).unwrap();
        let placed = &document["hooks"]["subagentStop"][0];
        assert_eq!(
            placed["exec"], program,
            "the placed entry was not pointed here"
        );
        assert_eq!(
            placed["timeoutSec"],
            json!(45),
            "the person's timeout was taken back"
        );
        for file in &files {
            let text = fs::read_to_string(file).unwrap();
            assert!(
                !text.contains("A gone"),
                "{} names the gone copy",
                file.display()
            );
        }
        assert_eq!(
            hooks_row_line(&scans[0].found, scans[0].unrepaired),
            Some(crate::i18n::agent_hooks_placed_line(&[
                "SubagentStop".to_owned()
            ]))
        );
    }

    /// N4 — **a due repair that did not happen is said on the row, and resume counts the hooks as
    /// missing**: this copy's C1 hooks (whose `Stop` carries no payload) in a folder where the
    /// dated copy cannot be kept are not written, the row says the writer's reason, the switch
    /// reads Off, and the resume row names Claude Code; held during an update's trial, the row
    /// says the repair waits for the update to be confirmed.
    ///
    /// MUTATION: a refused repair is published as nothing to say (`plant-67b-refusal-unsaid.py`).
    #[test]
    fn a_due_repair_that_did_not_happen_is_said_and_counts_as_missing() {
        let root = root("hooks-step-refused");
        let here = copy_at(&root, "here");
        let files = files_in(&root.join("config"));
        let data = root.join("data");
        install_all(&files, &here, &data);
        claude_in_old_shape(&files[0], double_quoted, false);
        let old = fs::read(&files[0]).unwrap();
        let held = AgentHooksStep::new().scan_and_repair(
            &some(&files),
            &here,
            &data,
            &what_stands,
            |_| true,
            &every_family,
        );
        assert_eq!(
            held[0],
            HooksScan::left(HooksFound::Drifted, Unrepaired::Held)
        );
        assert_eq!(
            hooks_row_line(&held[0].found, held[0].unrepaired),
            Some(Text::AgentHooksRepairHeld.text())
        );
        // A folder standing where today's copy of the person's file goes.
        let backup =
            files[0].with_extension(format!("json.bak-{}", crate::attention_hooks::today()));
        fs::create_dir_all(&backup).unwrap();
        let scans = AgentHooksStep::new().scan_and_repair(
            &some(&files),
            &here,
            &data,
            &what_stands,
            never_held,
            &every_family,
        );
        let refusal = Text::AgentConfigNoBackup.text();
        assert_eq!(
            scans[0],
            HooksScan::left(HooksFound::Drifted, Unrepaired::Refused(refusal))
        );
        assert_eq!(fs::read(&files[0]).unwrap(), old);
        assert_eq!(
            hooks_row_line(&scans[0].found, scans[0].unrepaired),
            Some(crate::i18n::agent_hooks_repair_refused_line(refusal))
        );
        let hooked = scans.each_ref().map(|scan| scan.found.is_this_copys());
        assert_eq!(
            hooked,
            [false, true, true],
            "an old shape counted as this copy's"
        );
        assert_eq!(
            resume_row_line(true, true, hooked),
            Some(crate::i18n::resume_needs_hooks_line(&["Claude Code"]))
        );
    }

    /// Makes `link` lead to `target`: a directory junction on Windows (no privilege needed), a
    /// symbolic link elsewhere.
    fn link_to(link: &Path, target: &Path) {
        let hygiene = bt_pty::test_shell::Hygiene::new();
        let mut command = if bt_platform::host_platform() == bt_platform::HostPlatform::Windows {
            let cmd = PathBuf::from(std::env::var_os("SystemRoot").expect("Windows names it"))
                .join("System32")
                .join("cmd.exe");
            let mut command = hygiene.command(cmd, bt_platform::quiet_command);
            command.args(["/C", "mklink", "/J"]).arg(link).arg(target);
            command
        } else {
            let mut command = hygiene.command("/bin/ln", bt_platform::quiet_command);
            command.arg("-s").arg(target).arg(link);
            command
        };
        let made = command.output().expect("the link command runs");
        assert!(
            made.status.success(),
            "the link was not made: {}",
            String::from_utf8_lossy(&made.stderr)
        );
    }

    /// N5 — **a missing owner behind a link is judged by the volume the link leads to**: the
    /// owner is spelled under a junction (a symbolic link off Windows) whose own volume is there.
    /// With the link's target there, the copy is gone; with the target gone — the drive it was on
    /// unplugged — the scan cannot follow the way and calls the owner unknown, though the volume
    /// the path is spelled on answers. A press decides on the machine as it stands.
    ///
    /// MUTATION: the volume is read off the path's spelling again
    /// (`plant-67b-volume-by-spelling-r2.py`).
    #[test]
    fn a_missing_owner_behind_a_link_is_judged_where_the_link_leads() {
        let root = root("hooks-step-link");
        let here = copy_at(&root, "B here");
        let target = root.join("elsewhere");
        fs::create_dir_all(&target).unwrap();
        let link = root.join("linked");
        link_to(&link, &target);
        let owner = link.join("Folio").join("folio.exe");
        let scan = DeadWhen::MissingOnAVolumeThatIsThere;
        assert_eq!(
            ownership::other_live_by(&owner, &here, &what_stands, scan),
            Ok(false),
            "a copy gone from where the link leads is gone"
        );
        fs::remove_dir_all(&target).unwrap();
        assert_eq!(
            what_stands(&volume_root_of(&owner).expect("a volume")),
            PathStands::Folder,
            "the volume the path is spelled on is there"
        );
        assert_eq!(what_stands(&owner), PathStands::Missing);
        assert_eq!(
            ownership::volume_of_a_missing_path(&owner),
            None,
            "the way through the link was followed by its spelling"
        );
        assert_eq!(
            ownership::other_live_by(&owner, &here, &what_stands, scan),
            Err(Text::AgentHooksOwnerUnknown.text()),
            "a copy behind a link to a volume that is away was called gone"
        );
        assert_eq!(
            ownership::other_live_by(&owner, &here, &what_stands, DeadWhen::Missing),
            Ok(false)
        );
    }

    /// N3 — **the first-run card's question**: a file a living other copy's hooks are in, and one
    /// whose owner the machine will not say about, is held by another; one with no Folio entry, or
    /// only a gone copy's, is not.
    #[test]
    fn a_file_another_living_copy_holds_is_said_to_the_first_run_card() {
        let root = root("hooks-step-first-run");
        let other = copy_at(&root, "A living");
        let here = copy_at(&root, "B here");
        let files = files_in(&root.join("config"));
        let data = root.join("data");
        for (family, file) in HOOKS_FAMILIES.iter().zip(&files) {
            assert!(
                !held_at(family, file, &here, &what_stands),
                "no file is held"
            );
        }
        install_all(&files, &other, &data);
        for (family, file) in HOOKS_FAMILIES.iter().zip(&files) {
            assert!(held_at(family, file, &here, &what_stands), "{}", family.tag);
            let unknown = |path: &Path| {
                if path == other {
                    PathStands::Unknown
                } else {
                    what_stands(path)
                }
            };
            assert!(held_at(family, file, &here, &unknown), "{}", family.tag);
        }
        fs::remove_file(&other).unwrap();
        for (family, file) in HOOKS_FAMILIES.iter().zip(&files) {
            assert!(
                !held_at(family, file, &here, &what_stands),
                "{}",
                family.tag
            );
        }
    }
}
