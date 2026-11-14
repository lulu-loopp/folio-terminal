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
//! taken writes nothing. One toast per family whose gone copy's hooks were pointed here (R8).
//!
//! **One diagnostics line per family when its answer changes** (row 68a, R-D of 2026-11-07):
//! `BT_AGENT_HOOKS <family> <state>`, with ` repaired=<from>` when a repair was due — written,
//! held, gated, pressed, refused or lost — and the gone programs for a dead owner. A start says
//! each family once; a run that finds what the last line said says nothing more
//! ([`AgentHooksStep::say_changed`]). The pass's own line counts the round.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use bt_platform::admission::WorkerCtx;

use crate::attention_copilot::Readiness;
use crate::attention_hooks::{Config, Unresolved};
use crate::attention_ownership::{self as ownership, Decision, HooksFound, Outcome};
use crate::carry_pass::MemberAnswer;
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
    /// Where its file is on this machine, as the newest program walk read the agent's folders, at
    /// the codex version the round settled: only codex's file depends on it (`hooks.json` or
    /// `config.toml`, row 69c), and it is handed in so the round names that file from the answer
    /// it holds (0.4.10 plan row 73, the re-review's F3).
    pub(crate) path: fn(&crate::attention_codex::CodexVersion) -> Option<PathBuf>,
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
        path: claude_path_at,
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
        path: crate::attention_codex::entry_path_at,
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
        path: copilot_path_at,
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

/// Claude Code's settings file, which no codex version moves.
fn claude_path_at(_: &crate::attention_codex::CodexVersion) -> Option<PathBuf> {
    crate::attention_hooks::settings_path()
}

/// Copilot's hooks file, which no codex version moves.
fn copilot_path_at(_: &crate::attention_codex::CodexVersion) -> Option<PathBuf> {
    crate::attention_copilot::hooks_path()
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
    /// The run's diagnostics line for the family (row 68a): said by [`AgentHooksStep::say_changed`]
    /// when it differs from the last one said.
    pub(crate) line: String,
}

impl HooksScan {
    fn unrepaired(found: HooksFound, line: String) -> Self {
        Self {
            found,
            repaired_from: None,
            notice: None,
            unrepaired: None,
            line,
        }
    }

    fn left(found: HooksFound, why: Unrepaired, line: String) -> Self {
        Self {
            unrepaired: Some(why),
            ..Self::unrepaired(found, line)
        }
    }
}

/// **A family's diagnostics line**: its state, and, for a run in which a repair was due, how it
/// went and the gone programs of a dead owner.
fn hooks_line(
    family: &HooksFamily,
    state: &HooksFound,
    repair: Option<(&str, &HooksFound)>,
) -> String {
    match repair {
        None => format!("BT_AGENT_HOOKS {} {}", family.tag, state.diagnostic_word()),
        Some((how, found)) => format!(
            "BT_AGENT_HOOKS {} {} repaired={how}{}",
            family.tag,
            state.diagnostic_word(),
            gone_programs(found)
        ),
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
/// readiness read beside them, and the notices the runs since the last adoption owe — and the two
/// facts the first-run card needs beside them (0.4.10 plan row 73), so that it reads no agent's
/// file on the window thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HooksSnapshot {
    pub(crate) found: [HooksFound; 3],
    pub(crate) unrepaired: [Option<Unrepaired>; 3],
    pub(crate) copilot_readiness: Readiness,
    /// **Whether `copilot_readiness` was read after Copilot's version probe had come back**: a
    /// readiness of `Unknown` read before it is "not asked yet", one read after it is "asked, and
    /// nothing to say" — and the first-run card offers Copilot's row only on an answer.
    pub(crate) copilot_read_after_probe: bool,
    /// **Whether an install of this copy's codex entry would be refused** — codex's own install
    /// refusals (`attention_codex::install_refused_at`), asked on the worker at the version the
    /// round settled, of the file the round scanned. Asked only where the scan's state would let a
    /// press write ([`press_would_write`]); `false` where it was not asked, which nothing reads on
    /// its own (`first_run::agent_row_offer`).
    pub(crate) codex_install_refused: bool,
    pub(crate) notices: Vec<Text>,
}

impl HooksSnapshot {
    /// **One run's answers, as the window adopts them**: the families' states, why a due repair
    /// was not written, Copilot's readiness and whether it was read after the probe, whether
    /// codex's install would be refused, and the notices the run's repairs owe.
    pub(crate) fn of(
        scans: &[HooksScan; 3],
        copilot_readiness: Readiness,
        copilot_read_after_probe: bool,
        codex_install_refused: bool,
    ) -> Self {
        Self {
            found: std::array::from_fn(|index| scans[index].found.clone()),
            unrepaired: std::array::from_fn(|index| scans[index].unrepaired),
            copilot_readiness,
            copilot_read_after_probe,
            codex_install_refused,
            notices: scans.iter().filter_map(|scan| scan.notice).collect(),
        }
    }

    /// **This run's answers in place of a part the window has not taken**: the untaken part's
    /// notices are still owed, so they are handed on, before this run's own.
    #[must_use]
    pub(crate) fn keeping_untaken(mut self, untaken: Option<Self>) -> Self {
        if let Some(untaken) = untaken {
            let mine = std::mem::replace(&mut self.notices, untaken.notices);
            self.notices.extend(mine);
        }
        self
    }
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
/// last diagnostics line said for each family.
pub(crate) struct AgentHooksStep {
    latches: [AtomicBool; 3],
    said: Mutex<[Option<String>; 3]>,
    /// Whether this process has said that this copy's program could not be named, since the
    /// last scan (the review's N8: said once, and again only after a scan in between).
    unnamed_said: AtomicBool,
}

impl AgentHooksStep {
    pub(crate) const fn new() -> Self {
        Self {
            latches: [
                AtomicBool::new(false),
                AtomicBool::new(false),
                AtomicBool::new(false),
            ],
            said: Mutex::new([None, None, None]),
            unnamed_said: AtomicBool::new(false),
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
            let as_found = |found: HooksFound| {
                let line = hooks_line(family, &found, None);
                HooksScan::unrepaired(found, line)
            };
            let config = match paths[index].as_deref().map(Config::resolve) {
                None => return as_found(HooksFound::Nothing),
                Some(Err(Unresolved::Refused(reason))) => {
                    return as_found(HooksFound::Refused(reason));
                }
                Some(Err(Unresolved::Unreadable)) => {
                    return as_found(HooksFound::Unreadable);
                }
                // One read for the run (`Config::with_one_read`): what is classified is what is
                // edited, and a press that wrote since makes the write refuse rather than undo it.
                Some(Ok(config)) => config.with_one_read(),
            };
            let found = (family.scan)(&config, exe, look);
            if !found.wants_repair() {
                return as_found(found);
            }
            let said = |how: &str| hooks_line(family, &found, Some((how, &found)));
            if *held.get_or_insert_with(|| defer(Writer::AgentHooksRepair)) {
                let line = said("held");
                return HooksScan::left(found, Unrepaired::Held, line);
            }
            if !within_its_gate(family) {
                let line = said("gated");
                return HooksScan::unrepaired(found, line);
            }
            let Some(_latch) = self.take_latch(index) else {
                let line = said("pressed");
                return HooksScan::unrepaired(found, line);
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
                    let how = if confirmed {
                        found.diagnostic_word().to_owned()
                    } else {
                        format!("lost ({})", now.diagnostic_word())
                    };
                    let line = hooks_line(family, &now, Some((&how, &found)));
                    HooksScan {
                        repaired_from: confirmed.then_some(found),
                        notice,
                        ..HooksScan::unrepaired(now, line)
                    }
                }
                Outcome::Refused(reason) => {
                    let line = said(&format!("refused ({reason})"));
                    HooksScan::left(found, Unrepaired::Refused(reason), line)
                }
                outcome => {
                    let line = said(&format!("refused ({outcome:?})"));
                    HooksScan::unrepaired(found, line)
                }
            }
        })
    }

    /// **Say each family's line when it differs from the last one said for that family**
    /// (row 68a, R-D of 2026-11-07), by `say`; answers how many it said.
    pub(crate) fn say_changed(&self, scans: &[HooksScan; 3], say: &dyn Fn(String)) -> usize {
        self.unnamed_said.store(false, Ordering::Relaxed);
        let mut said = self.said.lock().unwrap_or_else(PoisonError::into_inner);
        let mut changed = 0;
        for (last, scan) in said.iter_mut().zip(scans) {
            if last.as_deref() != Some(scan.line.as_str()) {
                say(scan.line.clone());
                *last = Some(scan.line.clone());
                changed += 1;
            }
        }
        changed
    }

    /// **Say that no family was scanned, because this copy's program could not be named** — once,
    /// and again only after a scan has run in between (the review's N8, under R-D's change rule).
    pub(crate) fn say_unnamed(&self, say: &dyn Fn(String)) {
        if !self.unnamed_said.swap(true, Ordering::Relaxed) {
            say(
                "BT_AGENT_HOOKS not scanned: this copy's own program could not be named".to_owned(),
            );
        }
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

/// **The step, on this machine**: the carry pass's member ([`crate::carry_pass::run_carry_steps`]),
/// answering its part of the round — three checks, one per family — or nothing.
///
/// **Not before the program walk has answered** (the pane-environment ruling of 2026-10-05: the
/// folders are the account's current ones, never the launch's): the walk's first answer runs the
/// pass, so a run asked earlier — the start's own, an Agents page opened in the first second — has
/// nothing to read yet and answers nothing. **A walk that ends with no answer, or a request no
/// walk stands for, settles the folders on the launch's** (`AgentHomesCell::homes_settled`, row
/// 68c's N1): that is a real answer, and this step reads them there until a walk answers.
pub(crate) fn run_agent_hooks_step(ctx: &WorkerCtx) -> Option<MemberAnswer<HooksSnapshot>> {
    if !crate::attention_hooks::AGENT_HOMES.homes_settled() {
        return None;
    }
    // The one family whose file is named by a question of its agent: codex's version decides
    // whether Folio's entry is the hook or the `notify` line (row 69c, R17), so it is settled here,
    // on the pass's worker, before any family's file is named, and codex's file is named from this
    // answer, not from the book a walk adopted since could have moved (the re-review's F3). The
    // first-run card's refusal is asked at this answer below (the review's N2).
    let version = crate::attention_codex::settle_version(ctx);
    let Some(exe) = ownership::this_copy() else {
        AGENT_HOOKS.say_unnamed(&crate::diagnostics::stderr_line);
        return None;
    };
    let paths = HOOKS_FAMILIES
        .each_ref()
        .map(|family| family_path(family, &version));
    let scans = AGENT_HOOKS.scan_and_repair(
        &paths,
        &exe,
        &crate::persist::storage_dir(),
        &ownership::what_stands,
        crate::update_trial::defer,
        &|family| (family.within_its_gate)(),
    );
    let changed = AGENT_HOOKS.say_changed(&scans, &crate::diagnostics::stderr_line);
    // The first-run card's two facts (0.4.10 plan row 73), on this worker. Codex's refusal is asked
    // at the version settled above, of the file just scanned. The probe's state is read before the
    // readiness, so a readiness said to follow the probe does.
    let codex_install_refused =
        codex_install_refused_at(&scans[1], paths[1].as_deref(), &exe, &version);
    let copilot_read_after_probe = crate::attention_copilot::probe_settled();
    let copilot_readiness = crate::attention_copilot::readiness();
    Some(MemberAnswer {
        part: HooksSnapshot::of(
            &scans,
            copilot_readiness,
            copilot_read_after_probe,
            codex_install_refused,
        ),
        checks: scans.len(),
        changed,
    })
}

/// **Whether a press of a family's switch would write, by what the scan found** (0.4.10 plan row
/// 73, ruling R1): the switch reads Off — no Folio entry, a gone copy's, a living other copy's
/// (whose press is the take-over), one the machine will not say about, or this copy's in an older
/// shape a repair did not write — and the writer has not already refused this file. A file this
/// build will not read or edit, an entry it cannot classify, one it declines, and this copy's
/// hooks in today's form are not.
#[must_use]
pub(crate) fn press_would_write(found: &HooksFound, unrepaired: Option<Unrepaired>) -> bool {
    let switch_off_and_writable = matches!(
        found,
        HooksFound::Nothing
            | HooksFound::Dead(_)
            | HooksFound::Drifted
            | HooksFound::OtherCopy(_)
            | HooksFound::OwnerUnknown
    );
    switch_off_and_writable && !matches!(unrepaired, Some(Unrepaired::Refused(_)))
}

/// **Whether codex's install would be refused**, asked of the file `path` the round scanned for
/// `exe` at the `version` the round settled — only where the scan's answer would let a press
/// write; `true` where no file could be named (the card then offers no row, as a press would have
/// nothing to write).
fn codex_install_refused_at(
    scan: &HooksScan,
    path: Option<&Path>,
    exe: &Path,
    version: &crate::attention_codex::CodexVersion,
) -> bool {
    if !press_would_write(&scan.found, scan.unrepaired) {
        return false;
    }
    path.is_none_or(|path| crate::attention_codex::install_refused_at(path, exe, version))
}

/// **Where a family's file is**: the account's own, in the product.
///
/// **A test process's agents keep their files in a scratch folder of its own**, never in the
/// account's (`persist::storage_dir`'s rule for the data folder): a test that drives a walk's
/// answer, an Agents-page open or a trial's commit runs the pass, and the pass writes.
fn family_path(
    family: &HooksFamily,
    settled: &crate::attention_codex::CodexVersion,
) -> Option<PathBuf> {
    #[cfg(test)]
    {
        let _ = settled;
        Some(test_process_agent_homes().join(family.tag).join("config"))
    }
    #[cfg(not(test))]
    {
        (family.path)(settled)
    }
}

/// The scratch folder [`family_path`] keeps a test process's agents in: one per process, named
/// once, under the build's `target/tb-tests` with the step's own tests' folders.
#[cfg(test)]
fn test_process_agent_homes() -> &'static Path {
    static HOMES: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
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

    /// A run's answer for a family without its diagnostics line, which
    /// `each_family_says_one_line_when_its_answer_changes` pins on its own.
    fn sans_line(scan: &HooksScan) -> HooksScan {
        HooksScan {
            line: String::new(),
            ..scan.clone()
        }
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
        let snapshot = HooksSnapshot::of(&scans, Readiness::Unknown, false, false);
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
        assert!(
            HooksSnapshot::of(&again, Readiness::Unknown, false, false)
                .notices
                .is_empty(),
            "a notice was said twice"
        );
    }

    /// RED (0.4.10 plan row 68a, R-D of 2026-11-07) — **each family says one line when its answer
    /// changes, and nothing when it does not**: the run that repairs a gone copy's hooks says how,
    /// for all three; the next says each is current now; the one after says nothing; a family whose
    /// file goes says that, alone.
    ///
    /// MUTATION: every run says every line (`plant-68a-line-said-every-round.py`).
    #[test]
    fn each_family_says_one_line_when_its_answer_changes() {
        let root = root("hooks-step-lines");
        let gone = copy_at(&root, "A gone");
        let here = copy_at(&root, "B here");
        let files = files_in(&root.join("config"));
        let data = root.join("data");
        install_all(&files, &gone, &root.join("gone data"));
        fs::remove_file(&gone).unwrap();
        let step = AgentHooksStep::new();
        let run = || {
            step.scan_and_repair(
                &some(&files),
                &here,
                &data,
                &what_stands,
                never_held,
                &every_family,
            )
        };
        let heard = std::cell::RefCell::new(Vec::<String>::new());
        let say = |line: String| heard.borrow_mut().push(line);
        let said = |scans: &[HooksScan; 3]| {
            let changed = step.say_changed(scans, &say);
            assert_eq!(changed, heard.borrow().len(), "the count is the lines said");
            heard.take()
        };
        let was = format!(" (was: {})", gone.display());
        assert_eq!(
            said(&run()),
            ["claude", "codex", "copilot"]
                .map(|tag| format!("BT_AGENT_HOOKS {tag} current repaired=dead{was}"))
        );
        assert_eq!(
            said(&run()),
            ["claude", "codex", "copilot"].map(|tag| format!("BT_AGENT_HOOKS {tag} current"))
        );
        assert!(said(&run()).is_empty(), "an unchanged answer says nothing");
        fs::remove_file(&files[0]).unwrap();
        let after = said(&run());
        assert_eq!(after.len(), 1, "{after:?}");
        assert!(after[0].starts_with("BT_AGENT_HOOKS claude "), "{after:?}");
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
        assert_eq!(
            HooksSnapshot::of(&scans, Readiness::Unknown, false, false).notices,
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
        assert_eq!(
            HooksSnapshot::of(&scans, Readiness::Unknown, false, false).notices,
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
            sans_line(&scans[2]),
            HooksScan::unrepaired(HooksFound::Dead(vec![gone.clone()]), String::new())
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
            sans_line(&scans[1]),
            HooksScan::unrepaired(HooksFound::Dead(vec![gone.clone()]), String::new())
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
                    assert_eq!(
                        sans_line(scan),
                        HooksScan::unrepaired(HooksFound::Current, String::new()),
                        "{shape}"
                    );
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
        assert_eq!(
            sans_line(&left[0]),
            HooksScan::unrepaired(HooksFound::Current, String::new())
        );
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
            assert!(
                HooksSnapshot::of(&scans, Readiness::Unknown, false, false)
                    .notices
                    .is_empty()
            );
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
            sans_line(&scans[0]),
            HooksScan::unrepaired(HooksFound::Dead(vec![gone.clone()]), String::new())
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
    /// presses' (R11).
    ///
    /// The start's carry step (0.4.10 plan row 68a, `startup::FirstWindow::carry`) is written
    /// inside `Runtime::create`, so it is held to the same: it begins the pass and reads nothing,
    /// and so do the step's own function (`carry_pass::begin_carry_at_start`) and the window's
    /// adoption of the round (`FolioApp::adopt_carry_snapshot`), both on the window thread (the
    /// review's N5).
    ///
    /// **And the first-run card reads none either** (0.4.10 plan row 73): its agent rows are said
    /// from the adopted snapshot by `first_run::card_machine` over `first_run::agent_row_offer` and
    /// it waits for that snapshot by `first_run::card_waits_for_the_agents`, so
    /// `Runtime::raise_first_run_if_due` and those three
    /// are held here too; so is the version probe's answer (`AppEvent::CopilotProbed`, ruling R9),
    /// whose readiness the pass it starts carries.
    ///
    /// **Nor the sign-in entry** (0.4.10 plan row 22a, R6): the round's adoption, the press's
    /// answer (`FolioApp::answer_sign_in_press`) and the press itself
    /// (`Runtime::apply_start_at_sign_in`) take what the pass's worker or the press's read.
    ///
    /// MUTATIONS: the step reads a row first (`plant-68a-carry-step-reads-on-the-window-thread.py`);
    /// the card asks Claude Code's state again (`plant-73-card-reads-claude-state.py`); the press's
    /// answer reads the entry again (`plant-22-read-on-the-window-thread.py`).
    #[test]
    fn the_window_thread_reads_no_agent_file_at_start() {
        use crate::test_support::{free_fn_body, item_body, method_body, squeezed};
        let create = method_body("Runtime", "create");
        assert!(
            squeezed(create).contains(&squeezed("carry: carry_pass::begin_carry_at_start,")),
            "the start's carry step is not where this test reads"
        );
        let events = item_body(
            &bt_source::ItemQuery::method("FolioApp", "user_event").of_trait("ApplicationHandler"),
        );
        let probed = events
            .find("AppEvent::CopilotProbed =>")
            .expect("the version probe's answer is handled");
        let probed = &events[probed..];
        let probed = &probed[..probed[1..]
            .find("AppEvent::")
            .map_or(probed.len(), |at| at + 1)];
        assert!(
            probed.contains("carry_pass::begin_carry_pass(CarryAsk::HOOKS)"),
            "the probe's answer no longer asks the pass, which carries the readiness"
        );
        let window_thread = [
            ("Runtime::create", create),
            ("begin_carry_at_start", free_fn_body("begin_carry_at_start")),
            (
                "FolioApp::adopt_carry_snapshot",
                method_body("FolioApp", "adopt_carry_snapshot"),
            ),
            (
                "adopt_agent_hooks_snapshot",
                free_fn_body("adopt_agent_hooks_snapshot"),
            ),
            (
                "Runtime::raise_first_run_if_due",
                method_body("Runtime", "raise_first_run_if_due"),
            ),
            ("agent_row_offer", free_fn_body("agent_row_offer")),
            ("card_machine", free_fn_body("card_machine")),
            (
                "card_waits_for_the_agents",
                free_fn_body("card_waits_for_the_agents"),
            ),
            ("press_would_write", free_fn_body("press_would_write")),
            ("AppEvent::CopilotProbed", probed),
            (
                "FolioApp::answer_sign_in_press",
                method_body("FolioApp", "answer_sign_in_press"),
            ),
            (
                "Runtime::apply_start_at_sign_in",
                method_body("Runtime", "apply_start_at_sign_in"),
            ),
        ];
        for (name, body) in window_thread {
            let body = squeezed(body);
            for read in [
                "row_state(",
                "attention_copilot::readiness(",
                "attention_hooks::state(",
                "attention_codex::state(",
                "attention_copilot::state(",
                "first_run_installable(",
                "install_refusal",
                "install_refused_at(",
                "scan_resolved(",
                "Config::resolve(",
                "hooks_are_switched_off(",
                // Explorer's registrations (row 68b, G6): read and written on the pass's worker.
                "context_menu::reassert(",
                "read_context_menu(",
                "explorer_menu::begin_probe(",
                "msix::registered(",
                // The sign-in entry (row 22a): read and written on the pass's worker and on the
                // press's own.
                "read_entry_in(",
                "write_entry_in(",
                "carry_sign_in_in(",
                "press_sign_in_in(",
                "CurrentUser",
            ] {
                assert!(
                    !body.contains(read),
                    "{name} reads an agent's file, an Explorer registration or the sign-in entry \
                     on the window thread: {read}"
                );
            }
            // The adoptions take the readiness the snapshot carries, and never ask it.
            assert!(
                name == "Runtime::create" || !body.contains("readiness()"),
                "{name} asks copilot's readiness on the window thread"
            );
        }
    }

    /// **The pass runs at the moments the rulings name** (R7): the walk's answer (its first
    /// included), the Agents page's open edge, a press, the trial's commit, and a new language.
    /// And a press never writes beside the repair (review N2).
    ///
    /// MUTATION: the press ignores the latch's answer (`plant-67b-press-beside-repair.py`).
    /// MUTATION: a walk with no answer settles nothing (`plant-68c-no-walk-never-settles.py`).
    #[test]
    fn the_pass_runs_at_the_moments_the_rulings_name() {
        use crate::test_support::{free_fn_body, method_body};
        // Each moment asks the pass for the agents' member (row 68b, R2).
        let begins =
            |body: &str| body.contains("begin_carry_pass(") && body.contains("CarryAsk::HOOKS");
        let walk = free_fn_body("adopt_program_answers");
        assert!(begins(walk) && walk.contains("AGENT_HOMES.answered()"));
        // Row 68c's N1: a walk that ends with no answer, or a request no walk stands for, settles
        // the folders on the launch's and runs the pass — the hooks' first real answer — and so
        // does the walk's first answer; both ask for the `$PROFILE` lines too (the coordinator's
        // ruling at row 68b's merge).
        let squeezed_walk = crate::test_support::squeezed(walk);
        assert!(
            walk.contains("answers.ended_with_no_walk_to_answer()")
                && squeezed_walk.contains(&crate::test_support::squeezed(
                    "if no_walk_answers && attention_hooks::AGENT_HOMES.settle_on_the_launch_homes() \
                     { carry_pass::begin_carry_pass(CarryAsk::HOMES_SETTLED); }"
                )),
            "a walk with no answer leaves the pass waiting for one: {walk}"
        );
        assert!(begins(method_body("Runtime", "refresh_agent_rows")));
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
        assert!(begins(free_fn_body("agent_row_asked_again")));
        // A row's refused repair carries the writer's sentence: a new language says it again.
        assert!(begins(method_body("Runtime", "adopt_new_language")));
        assert!(
            free_fn_body("release_trial_writer").contains(
                "Writer::AgentHooksRepair => carry_pass::begin_carry_pass(CarryAsk::HOOKS)"
            )
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
        let unasked = crate::attention_codex::CodexVersion::Unasked;
        assert_eq!(
            (HOOKS_FAMILIES[0].path)(&unasked),
            crate::attention_hooks::settings_path()
        );
        let scratch = test_process_agent_homes();
        assert!(scratch.starts_with(test_scratch_root()));
        for family in &HOOKS_FAMILIES {
            assert!(family_path(family, &unasked).is_some_and(|path| path.starts_with(scratch)));
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
        assert_eq!(
            sans_line(&scans[0]),
            HooksScan::unrepaired(placed.clone(), String::new())
        );
        assert_eq!(
            sans_line(&scans[2]),
            HooksScan::unrepaired(HooksFound::Current, String::new())
        );
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
            sans_line(&held[0]),
            HooksScan::left(HooksFound::Drifted, Unrepaired::Held, String::new())
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
            sans_line(&scans[0]),
            HooksScan::left(
                HooksFound::Drifted,
                Unrepaired::Refused(refusal),
                String::new()
            )
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
        crate::test_support::link_folder_to(&link, &target);
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

    /// N3 (0.4.10 plan row 67b; row 73's R7 moves it onto the scan and the card's mapping) — **a
    /// file a living other copy's hooks are in, and one whose owner the machine will not say about,
    /// is offered on the first-run card and arrives Off**; one with no Folio entry, or only a gone
    /// copy's whose repair has not been written, is offered and is nobody else's.
    #[test]
    fn a_file_another_living_copy_holds_arrives_off_on_the_first_run_card() {
        use crate::first_run::{AgentRowOffer, agent_row_offer};
        let root = root("hooks-step-first-run");
        let other = copy_at(&root, "A living");
        let here = copy_at(&root, "B here");
        let files = files_in(&root.join("config"));
        let data = root.join("data");
        // Gated, so a gone copy's hooks stay as found and nothing here writes but `install_all`.
        let gated = |_: &HooksFamily| false;
        let offers = |look: &dyn ownership::Look| {
            AgentHooksStep::new()
                .scan_and_repair(&some(&files), &here, &data, look, never_held, &gated)
                .map(|scan| agent_row_offer(&scan.found, scan.unrepaired, false))
        };
        let nobody_elses = AgentRowOffer {
            offered: true,
            over_another: false,
        };
        let another_copys = AgentRowOffer {
            offered: true,
            over_another: true,
        };
        assert_eq!(offers(&what_stands), [nobody_elses; 3], "no file is held");
        install_all(&files, &other, &data);
        assert_eq!(offers(&what_stands), [another_copys; 3], "a living copy's");
        let unknown = |path: &Path| {
            if path == other {
                PathStands::Unknown
            } else {
                what_stands(path)
            }
        };
        assert_eq!(offers(&unknown), [another_copys; 3], "an unknown owner's");
        fs::remove_file(&other).unwrap();
        assert_eq!(offers(&what_stands), [nobody_elses; 3], "a gone copy's");
    }

    /// RED (0.4.10 plan row 73, rulings R1 and R2) — **the snapshot carries codex's install
    /// refusal, asked of the file the round scanned at the version the round settled**: a clean
    /// folder is an install that writes, a `config.toml` this build cannot read refuses it, a family
    /// with no file named has nothing a press could write, and a state no press would write is not
    /// asked at all. On a codex below 0.148 the file is `config.toml` and the road the `notify`
    /// line's (the review's N2): a file with no `notify` key writes, and somebody else's program on
    /// the key refuses — which the hook road, at a version that runs hooks, does not ask.
    ///
    /// MUTATION: the mapping drops the refusal (`plant-73-codex-refusal-ignored.py`, red in
    /// `first_run::tests::each_found_state_offers_the_row_a_press_would_write`).
    #[test]
    fn the_snapshot_carries_codex_s_install_refusal() {
        let root = root("hooks-step-codex-refusal");
        let here = copy_at(&root, "B here");
        let folder = root.join("config").join("codex");
        fs::create_dir_all(&folder).unwrap();
        let hooks = folder.join("hooks.json");
        use crate::attention_codex::CodexVersion;
        let config = folder.join("config.toml");
        let nothing = HooksScan::unrepaired(HooksFound::Nothing, String::new());
        let current_codex = CodexVersion::of_version_line("codex-cli 0.150.0");
        let old_codex = CodexVersion::of_version_line("codex-cli 0.147.0");
        assert!(current_codex.runs_hooks() && !old_codex.runs_hooks());
        assert!(
            !codex_install_refused_at(&nothing, Some(&hooks), &here, &current_codex),
            "a clean folder: an install writes"
        );
        fs::write(&config, "this is not = = toml [[[").unwrap();
        assert!(
            codex_install_refused_at(&nothing, Some(&hooks), &here, &current_codex),
            "a config.toml this build cannot read: refused"
        );
        assert!(
            codex_install_refused_at(&nothing, None, &here, &current_codex),
            "no file named: nothing a press could write"
        );
        let current = HooksScan::unrepaired(HooksFound::Current, String::new());
        assert!(
            !codex_install_refused_at(&current, Some(&hooks), &here, &current_codex),
            "not asked where no press would write"
        );
        fs::remove_file(&config).unwrap();
        assert!(
            !codex_install_refused_at(&nothing, Some(&config), &here, &old_codex),
            "below 0.148, a config.toml with no notify line: the notify line is written"
        );
        fs::write(&config, "notify = [\"someone-else\", \"--quiet\"]\n").unwrap();
        assert!(
            codex_install_refused_at(&nothing, Some(&config), &here, &old_codex),
            "below 0.148, somebody else's program on the notify key: refused"
        );
        assert!(
            !codex_install_refused_at(&nothing, Some(&hooks), &here, &current_codex),
            "at a version that runs hooks, the hook is written beside somebody else's notify line"
        );
    }

    /// RED (0.4.10 plan row 73) — **the card's two facts are asked after what they depend on**:
    /// codex's refusal after the round settled codex's version and at that answer, handed in (an
    /// install's file and its road are the version's; the review's N2), of the file the round
    /// named from that same answer, so a walk adopted mid-round does not change the file the round
    /// scans and asks about (the re-review's F3), and Copilot's probe state before its readiness
    /// (a readiness said to follow the probe does).
    ///
    /// MUTATIONS: the refusal is asked at the version read back out of the book, not the one the
    /// round settled (`plant-73-refusal-at-a-reread-version.py`); the round names codex's file
    /// from the book (`plant-73-file-at-a-reread-version.py`); codex's file rule reads the book
    /// (`plant-73-entry-path-reads-the-book.py`).
    #[test]
    fn the_step_asks_the_card_s_facts_after_the_version_and_the_probe() {
        let body = crate::test_support::free_fn_body("run_agent_hooks_step");
        let at = |needle: &str| {
            body.find(needle)
                .unwrap_or_else(|| panic!("`{needle}` is not in the step"))
        };
        assert!(
            at("let version = crate::attention_codex::settle_version(ctx);")
                < at("codex_install_refused_at(")
        );
        assert!(at("let paths =") < at("codex_install_refused_at("));
        assert!(
            at("let version = crate::attention_codex::settle_version(ctx);") < at("let paths ="),
            "a family's file is named before the round settled codex's version"
        );
        let paths = &body[at("let paths =")..];
        let paths =
            crate::test_support::squeezed(&paths[..paths.find(';').expect("one statement")]);
        assert!(
            paths.ends_with(".map(|family|family_path(family,&version))"),
            "codex's file is not named from the version the round settled: {paths}"
        );
        assert!(
            crate::test_support::squeezed(crate::test_support::free_fn_body("family_path"))
                .contains("(family.path)(settled)"),
            "the product's family path is not asked at the version handed in"
        );
        let entry =
            crate::test_support::squeezed(crate::test_support::free_fn_body("entry_path_at"));
        assert!(
            entry.contains("settled.runs_hooks()") && !entry.contains("version()"),
            "codex's file rule reads the book, not the version handed in: {entry}"
        );
        let call = &body[at("codex_install_refused_at(")..];
        let call = crate::test_support::squeezed(&call[..call.find(';').expect("one statement")]);
        assert!(
            call.ends_with("&version)") || call.ends_with("&version,)"),
            "the refusal is not asked at the version the round settled: {call}"
        );
        assert!(
            at("attention_copilot::probe_settled()") < at("attention_copilot::readiness()"),
            "the probe's state is read after the readiness"
        );
    }

    /// RED (0.4.10 plan row 68a, the review's N8) — **a copy that cannot be named is said once**,
    /// under R-D's change rule: the line is said again only after a scan has run in between.
    ///
    /// MUTATION: the line is said every round (`plant-68a-unnamed-said-every-round.py`).
    #[test]
    fn an_unnamed_copy_is_said_once_until_a_scan_runs() {
        let step = AgentHooksStep::new();
        let heard = std::cell::RefCell::new(Vec::<String>::new());
        let say = |line: String| heard.borrow_mut().push(line);
        step.say_unnamed(&say);
        step.say_unnamed(&say);
        assert_eq!(
            heard.take(),
            ["BT_AGENT_HOOKS not scanned: this copy's own program could not be named"],
            "an unchanged answer says nothing"
        );
        let scans = [0, 1, 2].map(|_| HooksScan::unrepaired(HooksFound::Nothing, String::new()));
        step.say_changed(&scans, &say);
        heard.take();
        step.say_unnamed(&say);
        assert_eq!(
            heard.take().len(),
            1,
            "a scan in between makes it news again"
        );
    }
}
