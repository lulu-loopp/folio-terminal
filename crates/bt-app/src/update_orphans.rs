//! **The update entrances a copy that is gone left behind: the carry pass's sweep** (0.4.10 plan
//! row 68b; R5 of 2026-11-04, R6).
//!
//! An update arms an entrance before it touches the copy — on Windows a value
//! `HKCU\…\Run\FolioUpdate-<txn8>` (`bt_platform::logon_hook`), on macOS a LaunchAgent
//! `io.github.lulu-loopp.folio.update-<txn8>.plist` (`bt_platform::launch_agent`) — so that an
//! update interrupted by a lost machine is finished or undone at the next login, and retires it at
//! the commit, at the rollback, or at that copy's next start. A copy deleted while its transaction
//! was in flight retires nothing: every login after it runs a rescue program that is not there,
//! until somebody runs `--uninstall-cleanup`.
//!
//! So each start's carry pass ([`crate::carry_pass`]) reads the entrances of this account and
//! removes **one whose rescue program is gone** — nothing at it, by the system's own `NotFound`,
//! on a volume that is there (row 67's R4, [`gone_by`]). An entrance whose program is there is a
//! transaction of a copy that is (this copy's own among them, armed and not yet retired), and is
//! left; so is one whose program the machine will not say about, a drive that is away among them:
//! it comes back with the drive. A value of another type, a command or a plist not of the door's
//! own shape, is never judged and left. Only names of the door's own shape are read; every other
//! `Run` value and every other plist stays byte for byte.
//!
//! **Held during an update's trial** ([`Writer::OrphanSweep`], asked once per run and only when a
//! removal is due): the trial's build may be rolled back, and the entrances are the transaction's
//! until its commit, which runs the sweep again.
//!
//! **One diagnostics line per entrance when its fate changes** (R10): `BT_UPDATE_ORPHANS <name>
//! <removed|left (live)|left (owner unknown)|left (unread)|held|refused (<stage>)>`, or
//! `BT_UPDATE_ORPHANS none`; the value's or the file's name only, never the program's path. No row
//! shows an entrance, so the pass's part is a bare answer.
//!
//! What the sweep does not touch (R8 and the owner's open R9): an update's `.rejected-` copies of
//! a document, and the storage worker's `.displaced-` copies — both are copies of the person's
//! bytes, not entrances.

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use bt_platform::admission::WorkerCtx;
use bt_platform::logon_hook::{Cleaned, Registry};

use crate::attention_ownership::{Look, gone_by, what_stands};
use crate::carry_pass::MemberAnswer;
use crate::update_trial::Writer;

/// **What the sweep did with one entrance.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OrphanFate {
    /// Its rescue program is gone, and so is it.
    Removed,
    /// Its rescue program is there: a transaction of a copy that is.
    LeftLive,
    /// The machine will not say about its rescue program (a drive that is away among them).
    LeftOwnerUnknown,
    /// Not of the door's own shape: never judged.
    LeftUnread,
    /// Due, and held for the update's commit.
    Held,
    /// Reading or removing it failed, at the stage named.
    Refused(String),
}

impl OrphanFate {
    fn fate_said(&self) -> String {
        match self {
            Self::Removed => "removed".to_owned(),
            Self::LeftLive => "left (live)".to_owned(),
            Self::LeftOwnerUnknown => "left (owner unknown)".to_owned(),
            Self::LeftUnread => "left (unread)".to_owned(),
            Self::Held => "held".to_owned(),
            Self::Refused(stage) => format!("refused ({stage})"),
        }
    }
}

/// **The judge both walkers hand their entrances to**: the machine's answer about a program, and
/// the trial's gate, asked once and only when a removal is due.
struct OrphanJudge<'a, L: Look + ?Sized, D: FnOnce(Writer) -> bool> {
    look: &'a L,
    gate: Option<D>,
    held: bool,
    judged: Vec<(String, OrphanFate)>,
}

impl<'a, L: Look + ?Sized, D: FnOnce(Writer) -> bool> OrphanJudge<'a, L, D> {
    fn new(look: &'a L, defer: D) -> Self {
        Self {
            look,
            gate: Some(defer),
            held: false,
            judged: Vec::new(),
        }
    }

    /// Whether the entrance `name`, starting `rescue` (`None`: not of the door's shape), goes.
    fn removes(&mut self, name: &str, rescue: Option<&Path>) -> bool {
        let fate = match rescue.map(|rescue| gone_by(rescue, self.look)) {
            None => OrphanFate::LeftUnread,
            Some(Ok(false)) => OrphanFate::LeftLive,
            Some(Err(_)) => OrphanFate::LeftOwnerUnknown,
            Some(Ok(true)) => {
                if let Some(defer) = self.gate.take() {
                    self.held = defer(Writer::OrphanSweep);
                }
                if !self.held {
                    return true;
                }
                OrphanFate::Held
            }
        };
        self.judged.push((name.to_owned(), fate));
        false
    }

    /// The fates of the entrances the walk removed (or failed to), beside the ones judged left.
    fn fates(mut self, walked: Vec<(String, OrphanFate)>) -> Vec<(String, OrphanFate)> {
        self.judged.extend(walked);
        self.judged.sort_by(|(left, _), (right, _)| left.cmp(right));
        self.judged
    }
}

/// **The Windows sweep**: the `Run` values of `key` whose name is the door's
/// ([`bt_platform::logon_hook::VALUE_PREFIX`]), through the door's own walker
/// ([`bt_platform::logon_hook::clean_by`]). A walk the registry refused is one fate, named
/// `Run`.
pub(crate) fn sweep_run_values(
    registry: &mut impl Registry,
    key: &str,
    look: &(impl Look + ?Sized),
    defer: impl FnOnce(Writer) -> bool,
) -> Vec<(String, OrphanFate)> {
    let mut judge = OrphanJudge::new(look, defer);
    let walked = bt_platform::logon_hook::clean_by(registry, key, |name, rescue| {
        judge.removes(name, Some(rescue))
    });
    let walked = match walked {
        Err(refusal) => vec![("Run".to_owned(), OrphanFate::Refused(refusal.to_string()))],
        Ok(cleaned) => cleaned
            .into_iter()
            .filter_map(|(name, cleaned)| match cleaned {
                Cleaned::Removed => Some((name, OrphanFate::Removed)),
                Cleaned::Refused(refusal) => Some((name, OrphanFate::Refused(refusal.to_string()))),
                // Judged, and its fate is the judge's — or never judged: not of the door's shape.
                Cleaned::Left(_) => (!judge.judged.iter().any(|(judged, _)| *judged == name))
                    .then_some((name, OrphanFate::LeftUnread)),
            })
            .collect(),
    };
    judge.fates(walked)
}

/// **The macOS sweep**: the plists of `agents` whose name is the door's
/// ([`bt_platform::launch_agent::is_ours`]), each read for its rescue program with the door's own
/// reader ([`bt_platform::launch_agent::rescue_of_plist`]) and judged, through the door's own
/// walker ([`bt_platform::launch_agent::sweep_by`]). A walk the folder refused is one fate, named
/// `LaunchAgents`.
pub(crate) fn sweep_launch_agents(
    agents: &Path,
    look: &(impl Look + ?Sized),
    defer: impl FnOnce(Writer) -> bool,
) -> Vec<(String, OrphanFate)> {
    let mut judge = OrphanJudge::new(look, defer);
    let walked = bt_platform::launch_agent::sweep_by(agents, |plist| {
        let rescue = bt_platform::launch_agent::rescue_of_plist(plist);
        judge.removes(&launch_agent_name(plist), rescue.as_deref())
    });
    let walked = match walked {
        Err(refusal) => vec![(
            "LaunchAgents".to_owned(),
            OrphanFate::Refused(launch_agent_stage(&refusal).to_owned()),
        )],
        Ok(swept) => swept
            .into_iter()
            .map(|(plist, removed)| {
                let fate = match removed {
                    Ok(()) => OrphanFate::Removed,
                    Err(refusal) => OrphanFate::Refused(launch_agent_stage(&refusal).to_owned()),
                };
                (launch_agent_name(&plist), fate)
            })
            .collect(),
    };
    judge.fates(walked)
}

/// A plist's file name, as the sweep's line says it.
fn launch_agent_name(plist: &Path) -> String {
    plist
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The stage a LaunchAgent refusal stopped at — its own words carry the folder's path.
fn launch_agent_stage(refusal: &bt_platform::launch_agent::Refusal) -> &'static str {
    match refusal {
        bt_platform::launch_agent::Refusal::List { .. } => "list",
        bt_platform::launch_agent::Refusal::Remove(_) => "remove",
        _ => "sweep",
    }
}

/// **The sweep's last lines**, each said again only when it differs from every line said last
/// time (R-D's change rule).
pub(crate) struct OrphansSaid(Mutex<Vec<String>>);

impl OrphansSaid {
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }

    /// The fates as the pass's part: one check, changed when any line was said.
    pub(crate) fn orphans_answer(
        &self,
        fates: &[(String, OrphanFate)],
        say: &dyn Fn(String),
    ) -> MemberAnswer<()> {
        let lines = if fates.is_empty() {
            vec!["BT_UPDATE_ORPHANS none".to_owned()]
        } else {
            fates
                .iter()
                .map(|(name, fate)| format!("BT_UPDATE_ORPHANS {name} {}", fate.fate_said()))
                .collect()
        };
        let mut last = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let mut changed = 0;
        for line in &lines {
            if !last.contains(line) {
                say(line.clone());
                changed = 1;
            }
        }
        *last = lines;
        MemberAnswer {
            part: (),
            checks: 1,
            changed,
        }
    }
}

/// This process's.
static ORPHANS_SAID: OrphansSaid = OrphansSaid::new();

/// **The sweep, as a member of the start's carry pass** ([`crate::carry_pass::run_carry_steps`]):
/// this account's `Run` key on Windows, its LaunchAgents folder on macOS, this machine's disk and
/// the trial's gate.
///
/// Nothing elsewhere, where there are no entrances; nothing in a test process, whose pass must
/// never read or write the account's logon entries — the tests drive the two walkers over a
/// registry and a folder of their own.
pub(crate) fn run_orphans_step(_ctx: &WorkerCtx) -> Option<MemberAnswer<()>> {
    if cfg!(test) {
        return None;
    }
    let fates = match bt_platform::host_platform() {
        bt_platform::HostPlatform::Windows => sweep_run_values(
            &mut bt_platform::logon_hook::CurrentUser,
            bt_platform::logon_hook::RUN_KEY,
            &what_stands,
            crate::update_trial::defer,
        ),
        bt_platform::HostPlatform::MacOs => sweep_launch_agents(
            &crate::update_apply_macos::launch_agents()?,
            &what_stands,
            crate::update_trial::defer,
        ),
        bt_platform::HostPlatform::OtherUnix => return None,
    };
    Some(ORPHANS_SAID.orphans_answer(&fates, &crate::diagnostics::stderr_line))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io;
    use std::path::PathBuf;

    use bt_platform::logon_hook::{self, REG_SZ, VALUE_PREFIX};

    use super::*;
    use crate::attention_ownership::PathStands;

    /// The key the in-memory registry is asked under: never `RUN_KEY`.
    const KEY: &str = r"Software\Folio-Test\update-orphans";

    /// **A registry held in memory**, recording every value it was asked to read and every
    /// flush.
    #[derive(Default)]
    struct OrphanRunValues {
        values: BTreeMap<String, (u32, Vec<u8>)>,
        read: Vec<String>,
        flushes: usize,
    }

    impl Registry for OrphanRunValues {
        fn set(&mut self, key: &str, name: &str, kind: u32, data: &[u8]) -> io::Result<()> {
            assert_eq!(key, KEY, "only the test's own key");
            self.values.insert(name.to_owned(), (kind, data.to_vec()));
            Ok(())
        }

        fn flush(&mut self, _key: &str) -> io::Result<()> {
            self.flushes += 1;
            Ok(())
        }

        fn get(&mut self, _key: &str, name: &str) -> io::Result<Option<(u32, Vec<u8>)>> {
            self.read.push(name.to_owned());
            Ok(self.values.get(name).cloned())
        }

        fn delete(&mut self, _key: &str, name: &str) -> io::Result<bool> {
            Ok(self.values.remove(name).is_some())
        }

        fn names(&mut self, _key: &str) -> io::Result<Vec<String>> {
            Ok(self.values.keys().cloned().collect())
        }
    }

    /// A `REG_SZ`'s bytes, as the registry holds one.
    fn reg_sz(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    /// The transaction whose first byte is `first`.
    fn txn(first: u8) -> [u8; 16] {
        let mut txn = [0; 16];
        txn[0] = first;
        txn
    }

    /// A scratch folder under the build directory, removed with the test.
    fn scratch(tag: &str) -> crate::test_support::ScratchFolder {
        crate::test_support::ScratchFolder::create(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/tb-tests")
                .join(bt_testpath::unique_name(tag)),
        )
    }

    /// A rescue program at `<root>/<copy>/.folio-update/rescue/folio`, made when `there`.
    fn rescue(root: &Path, copy: &str, there: bool) -> PathBuf {
        let program = root
            .join(copy)
            .join(".folio-update")
            .join("rescue")
            .join("folio");
        if there {
            std::fs::create_dir_all(program.parent().unwrap()).unwrap();
            std::fs::write(&program, copy.as_bytes()).unwrap();
        }
        program
    }

    /// A gate that must not be asked: nothing is due.
    fn never_asked(writer: Writer) -> bool {
        panic!("the trial's gate is asked only when a removal is due, not for {writer:?}")
    }

    /// The registry the Windows tests sweep: a dead copy's entrance, this copy's and another live
    /// copy's armed ones, a value of another type and a command of another shape under the
    /// door's prefix, and somebody else's value beside them.
    fn run_key(root: &Path) -> (OrphanRunValues, String) {
        let mut registry = OrphanRunValues::default();
        for (first, copy, there) in [(1, "gone", false), (2, "here", true), (3, "other", true)] {
            let _armed =
                logon_hook::arm_in(&mut registry, KEY, &txn(first), &rescue(root, copy, there))
                    .expect("the test's own key takes the entrance");
        }
        registry
            .set(KEY, &format!("{VALUE_PREFIX}0a000000"), 4, &[1, 0, 0, 0])
            .unwrap();
        let unquoted = format!("{} --update-recover", rescue(root, "gone", false).display());
        registry
            .set(
                KEY,
                &format!("{VALUE_PREFIX}0b000000"),
                REG_SZ,
                &reg_sz(&unquoted),
            )
            .unwrap();
        let foreign = format!(
            "\"{}\" /background",
            root.join("Elsewhere").join("sync.exe").display()
        );
        registry
            .set(KEY, "SomebodyElse", REG_SZ, &reg_sz(&foreign))
            .unwrap();
        registry.read.clear();
        registry.flushes = 0;
        (registry, foreign)
    }

    /// RED (0.4.10 plan row 68b; R-C's old-shape test for the update entrance, one shape since
    /// 0.4.6) — **a dead update entrance is swept, and a live one is left.**
    ///
    /// Over a registry of the test's own: the entrance whose rescue program is gone from a volume
    /// that is there is removed, and the key flushed once; this copy's armed entrance and another
    /// live copy's are left (their programs are there); a value of another type and a command not
    /// of the door's shape are never judged and left; somebody else's value is never read and
    /// stays byte for byte.
    ///
    /// MUTATIONS: remove every entrance of the door's name
    /// (`plant-68b-sweep-takes-a-live-entrance.py`) and the live ones go; read every value of the
    /// key (`plant-68b-sweep-reads-a-foreign-value.py`) and somebody else's value is read — and,
    /// its program missing, removed.
    #[test]
    fn a_dead_update_entrance_is_swept_and_a_live_one_is_left() {
        let root = scratch("orphans-run");
        let (mut registry, foreign) = run_key(&root);
        let fates = sweep_run_values(&mut registry, KEY, &what_stands, |_| false);
        let name = |first| logon_hook::value_name(&txn(first));
        assert_eq!(
            fates,
            [
                (name(1), OrphanFate::Removed),
                (name(2), OrphanFate::LeftLive),
                (name(3), OrphanFate::LeftLive),
                (format!("{VALUE_PREFIX}0a000000"), OrphanFate::LeftUnread),
                (format!("{VALUE_PREFIX}0b000000"), OrphanFate::LeftUnread),
            ]
        );
        assert!(
            !registry.values.contains_key(&name(1)),
            "the dead one is gone"
        );
        assert_eq!(registry.flushes, 1, "the key flushed once");
        assert!(
            registry
                .read
                .iter()
                .all(|read| read.starts_with(VALUE_PREFIX)),
            "only the door's names are read: {:?}",
            registry.read
        );
        assert_eq!(
            registry.values.get("SomebodyElse"),
            Some(&(REG_SZ, reg_sz(&foreign))),
            "somebody else's value stays byte for byte"
        );

        let said = OrphansSaid::new();
        let lines = std::cell::RefCell::new(Vec::new());
        let say = |line: String| lines.borrow_mut().push(line);
        let first = said.orphans_answer(&fates, &say);
        assert_eq!((first.checks, first.changed), (1, 1));
        assert!(
            lines
                .borrow()
                .contains(&format!("BT_UPDATE_ORPHANS {} removed", name(1))),
            "said by name: {:?}",
            lines.borrow()
        );
        assert!(
            lines
                .borrow()
                .iter()
                .all(|line| !line.contains(&*root.to_string_lossy())),
            "and never by path"
        );
        let again = sweep_run_values(&mut registry, KEY, &what_stands, never_asked);
        lines.borrow_mut().clear();
        let second = said.orphans_answer(&again, &say);
        assert_eq!(
            (second.checks, second.changed),
            (1, 0),
            "a round that finds what was said says nothing"
        );
        assert!(
            lines.borrow().is_empty(),
            "the lines that stand are not said again"
        );
    }

    /// **The machine with the drive a program was on unplugged**: what stands is this machine's,
    /// and the way to a missing program's volume cannot be followed.
    struct OrphanAwayVolume;

    impl Look for OrphanAwayVolume {
        fn stands(&self, path: &Path) -> PathStands {
            what_stands(path)
        }

        fn volume_of(&self, _missing: &Path) -> Option<PathBuf> {
            None
        }
    }

    /// RED (0.4.10 plan row 68b; row 67's R4) — **an entrance whose rescue program is on a volume
    /// that is away is left, and said**: the program is not gone, only unplugged, and the
    /// transaction comes back with the drive.
    ///
    /// MUTATION: judge by `exists` alone (`plant-68b-sweep-ignores-the-volume.py`) and the
    /// entrance is removed.
    #[test]
    fn an_entrance_on_a_volume_that_is_away_is_left_and_said() {
        let root = scratch("orphans-away");
        let mut registry = OrphanRunValues::default();
        let _armed =
            logon_hook::arm_in(&mut registry, KEY, &txn(1), &rescue(&root, "gone", false)).unwrap();
        let fates = sweep_run_values(&mut registry, KEY, &OrphanAwayVolume, never_asked);
        let name = logon_hook::value_name(&txn(1));
        assert_eq!(fates, [(name.clone(), OrphanFate::LeftOwnerUnknown)]);
        assert!(registry.values.contains_key(&name), "left");
        let lines = std::cell::RefCell::new(Vec::new());
        let _ = OrphansSaid::new().orphans_answer(&fates, &|line| lines.borrow_mut().push(line));
        assert_eq!(
            *lines.borrow(),
            [format!("BT_UPDATE_ORPHANS {name} left (owner unknown)")]
        );
    }

    /// RED (0.4.10 plan row 68b) — **a dead LaunchAgent entrance is swept, and every other plist
    /// is left.**
    ///
    /// In a LaunchAgents folder of the test's own: the plist whose rescue program is gone is
    /// removed; the one whose program is there is left; one of the door's name whose bytes are
    /// not the door's plist is never judged and left; another program's plist, naming a program
    /// that is gone, and a folder of the door's name are never handed to the judge.
    ///
    /// MUTATION: hand every plist to the judge (`plant-68b-sweep-takes-a-foreign-plist.py`) and
    /// another program's plist is removed.
    #[test]
    fn a_dead_launch_agent_is_swept_and_every_other_plist_is_left() {
        use bt_platform::launch_agent::{file_name, plist};

        let root = scratch("orphans-agents");
        let agents = root.join("LaunchAgents");
        std::fs::create_dir_all(&agents).unwrap();
        let home = root.join("home");
        let gone = rescue(&root, "gone", false);
        let live = rescue(&root, "here", true);
        std::fs::write(
            agents.join(file_name(&txn(1))),
            plist(&txn(1), &gone, &home).unwrap(),
        )
        .unwrap();
        std::fs::write(
            agents.join(file_name(&txn(2))),
            plist(&txn(2), &live, &home).unwrap(),
        )
        .unwrap();
        let edited = plist(&txn(3), &gone, &home)
            .unwrap()
            .replace("<true/>", "<false/>");
        std::fs::write(agents.join(file_name(&txn(3))), &edited).unwrap();
        let foreign = plist(&txn(4), &gone, &home).unwrap();
        std::fs::write(agents.join("com.example.sync.plist"), &foreign).unwrap();
        std::fs::create_dir_all(agents.join(file_name(&txn(5)))).unwrap();

        let fates = sweep_launch_agents(&agents, &what_stands, |_| false);
        assert_eq!(
            fates,
            [
                (file_name(&txn(1)), OrphanFate::Removed),
                (file_name(&txn(2)), OrphanFate::LeftLive),
                (file_name(&txn(3)), OrphanFate::LeftUnread),
            ]
        );
        assert!(
            !agents.join(file_name(&txn(1))).exists(),
            "the dead one is gone"
        );
        assert!(agents.join(file_name(&txn(2))).is_file());
        assert_eq!(
            std::fs::read_to_string(agents.join(file_name(&txn(3)))).unwrap(),
            edited
        );
        assert_eq!(
            std::fs::read_to_string(agents.join("com.example.sync.plist")).unwrap(),
            foreign,
            "another program's plist stays byte for byte"
        );
        assert!(
            agents.join(file_name(&txn(5))).is_dir(),
            "and the folder stays"
        );
    }

    /// RED (0.4.10 plan row 68b, R6) — **the sweep waits for the trial's commit**: during an
    /// update's trial a removal that is due asks the gate once, is held and recorded as pending
    /// ([`Writer::OrphanSweep`]), and nothing is removed; the commit's round removes it.
    ///
    /// MUTATION: remove without asking the gate (`plant-68b-sweep-unheld.py`) and the trial's
    /// round removes the entrance.
    #[test]
    fn the_sweep_waits_for_the_trials_commit() {
        let root = scratch("orphans-trial");
        let mut registry = OrphanRunValues::default();
        for first in [1, 2] {
            let _armed = logon_hook::arm_in(
                &mut registry,
                KEY,
                &txn(first),
                &rescue(&root, "gone", false),
            )
            .unwrap();
        }
        let gate = crate::update_trial::Gate::new();
        let asked = std::cell::Cell::new(0);
        let held = sweep_run_values(&mut registry, KEY, &what_stands, |writer| {
            asked.set(asked.get() + 1);
            gate.defer(true, writer)
        });
        assert_eq!(asked.get(), 1, "the gate is asked once");
        assert_eq!(
            held.iter().map(|(_, fate)| fate).collect::<Vec<_>>(),
            [&OrphanFate::Held, &OrphanFate::Held]
        );
        assert_eq!(registry.values.len(), 2, "nothing removed during the trial");
        assert_eq!(gate.pending(), vec![Writer::OrphanSweep]);

        let committed = sweep_run_values(&mut registry, KEY, &what_stands, |_| false);
        assert_eq!(
            committed.iter().map(|(_, fate)| fate).collect::<Vec<_>>(),
            [&OrphanFate::Removed, &OrphanFate::Removed]
        );
        assert!(
            registry.values.is_empty(),
            "the commit's round removes them"
        );
    }
}
