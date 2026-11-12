//! The sign-in entry's tests (0.4.10 plan row 22a): every one over [`SignInFake`], two keys held
//! in memory under names of the suite's own, and real files in a scratch folder for "there" and
//! "gone" — never the account's `Run` key.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bt_platform::logon_hook::{REG_SZ, Registry};
use bt_platform::sign_in_entry::{ENTRY_NAME, EntryKeys, REG_BINARY, command_for};

use super::*;
use crate::attention_ownership::PathStands;
use crate::test_support::{ScratchFolder, free_fn_body, squeezed};

/// The keys the fake is asked under: never the account's.
const SIGN_IN_TEST_KEYS: EntryKeys<'static> = EntryKeys {
    run: r"Software\Folio-Test\start-at-sign-in\Run",
    approved: r"Software\Folio-Test\start-at-sign-in\StartupApproved",
};

/// **Two keys held in memory**, every value read and every value written recorded by name.
#[derive(Default)]
struct SignInFake {
    values: BTreeMap<(String, String), (u32, Vec<u8>)>,
    read: Vec<String>,
    written: Vec<String>,
    /// Every write refused, in the system's words (review N2).
    refuse_set: bool,
    /// Every removal under this key refused (review N2).
    refuse_delete_under: Option<&'static str>,
}

/// The system's words a refusing fake answers with.
const SIGN_IN_DENIED: &str = "access is denied";

impl SignInFake {
    fn put_value(&mut self, key: &str, name: &str, kind: u32, data: Vec<u8>) {
        self.values
            .insert((key.to_owned(), name.to_owned()), (kind, data));
    }

    fn value_under(&self, key: &str, name: &str) -> Option<&(u32, Vec<u8>)> {
        self.values.get(&(key.to_owned(), name.to_owned()))
    }

    /// The entry, as the `Run` key holds it.
    fn sign_in_entry_value(&self) -> Option<&(u32, Vec<u8>)> {
        self.value_under(SIGN_IN_TEST_KEYS.run, ENTRY_NAME)
    }
}

impl Registry for SignInFake {
    fn set(&mut self, key: &str, name: &str, kind: u32, data: &[u8]) -> io::Result<()> {
        assert!(
            key == SIGN_IN_TEST_KEYS.run || key == SIGN_IN_TEST_KEYS.approved,
            "only the suite's keys"
        );
        if self.refuse_set {
            return Err(io::Error::other(SIGN_IN_DENIED));
        }
        self.written.push(format!("{key}|{name}"));
        self.put_value(key, name, kind, data.to_vec());
        Ok(())
    }

    fn flush(&mut self, _key: &str) -> io::Result<()> {
        Ok(())
    }

    fn get(&mut self, key: &str, name: &str) -> io::Result<Option<(u32, Vec<u8>)>> {
        assert!(
            key == SIGN_IN_TEST_KEYS.run || key == SIGN_IN_TEST_KEYS.approved,
            "only the suite's keys"
        );
        self.read.push(name.to_owned());
        Ok(self.value_under(key, name).cloned())
    }

    fn delete(&mut self, key: &str, name: &str) -> io::Result<bool> {
        if self.refuse_delete_under == Some(key) {
            return Err(io::Error::other(SIGN_IN_DENIED));
        }
        self.written.push(format!("{key}|{name}"));
        Ok(self
            .values
            .remove(&(key.to_owned(), name.to_owned()))
            .is_some())
    }

    fn names(&mut self, _key: &str) -> io::Result<Vec<String>> {
        panic!("the sign-in entry never lists a key")
    }
}

/// A `REG_SZ`'s bytes for `text`.
fn sz(text: &OsStr) -> Vec<u8> {
    text.to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// The entry naming `exe`, as this build writes it.
fn entry_of(exe: &Path) -> (u32, Vec<u8>) {
    (REG_SZ, sz(&command_for(exe)))
}

/// **The copies a test's entry can name**: this copy (`this`, its canonical spelling, and
/// `this_spelled_otherwise`, the same file by a path with `..` in it), another copy that is there,
/// and a copy that is gone from a volume that is there.
struct SignInCopies {
    _scratch: ScratchFolder,
    this: PathBuf,
    this_spelled_otherwise: PathBuf,
    other: PathBuf,
    gone: PathBuf,
}

fn sign_in_copies(tag: &str) -> SignInCopies {
    let raw = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tb-tests")
        .join(bt_testpath::unique_name(tag));
    let scratch = ScratchFolder::create(raw.clone());
    let made = |root: &Path, copy: &str| {
        let exe = root.join(copy).join("folio.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, copy.as_bytes()).unwrap();
        exe
    };
    let this = made(&scratch, "here");
    let other = made(&scratch, "other");
    let gone = scratch.join("gone").join("folio.exe");
    SignInCopies {
        this_spelled_otherwise: raw.join("here").join("folio.exe"),
        this,
        other,
        gone,
        _scratch: scratch,
    }
}

/// A gate that must not be asked: nothing is due.
fn sign_in_gate_never_asked(writer: Writer) -> bool {
    panic!("the trial's gate is asked only when a repair is due, not for {writer:?}")
}

/// **The machine with the drive a program was on unplugged.**
struct SignInAwayVolume;

impl Look for SignInAwayVolume {
    fn stands(&self, path: &Path) -> PathStands {
        what_stands(path)
    }

    fn volume_of(&self, _missing: &Path) -> Option<PathBuf> {
        None
    }
}

/// The member over `registry`, this copy `this`, with no trial and no press.
fn sign_in_carried(registry: &mut SignInFake, this: &Path) -> SignInRun {
    carry_sign_in_in(
        registry,
        SIGN_IN_TEST_KEYS,
        this,
        &what_stands,
        |_| false,
        &SignInPresses::new(),
        &Mutex::new(()),
    )
}

/// The press of `on` over `registry`, as the worker makes it.
fn pressed(registry: &mut SignInFake, this: &Path, on: bool) -> SignInPressed {
    let presses = SignInPresses::new();
    presses.note_sign_in_press(on);
    press_sign_in_in(
        registry,
        SIGN_IN_TEST_KEYS,
        Ok(this),
        &what_stands,
        &presses,
        &Mutex::new(()),
    )
}

/// RED (T-START-AT-LOGON-SWITCH, R6, R9) — **the switch's On writes the entry naming this copy and
/// takes the system's switch off its name; Off removes both; every other value of both keys stays
/// byte for byte.**
///
/// MUTATION (`plant-22-off-leaves-the-entry.py`): `Off` removes nothing and the entry stays.
#[test]
fn the_switch_writes_the_entry_and_off_removes_it() {
    let copies = sign_in_copies("sign-in-press");
    let mut registry = SignInFake::default();
    let entrance = (
        REG_SZ,
        sz(OsStr::new(r#""C:\x\rescue\folio.exe" --update-recover"#)),
    );
    registry.put_value(
        SIGN_IN_TEST_KEYS.run,
        "FolioUpdate-abcd0123",
        entrance.0,
        entrance.1.clone(),
    );
    registry.put_value(
        SIGN_IN_TEST_KEYS.run,
        "SomebodyElse",
        REG_SZ,
        sz(OsStr::new("tool")),
    );
    registry.put_value(
        SIGN_IN_TEST_KEYS.approved,
        "SomebodyElse",
        REG_BINARY,
        vec![3, 0, 0, 0],
    );
    registry.put_value(
        SIGN_IN_TEST_KEYS.approved,
        ENTRY_NAME,
        REG_BINARY,
        vec![3, 0, 0, 0],
    );
    let others = |registry: &SignInFake| {
        registry
            .values
            .iter()
            .filter(|((_, name), _)| name != ENTRY_NAME)
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Vec<_>>()
    };
    let before = others(&registry);

    let on = pressed(&mut registry, &copies.this, true);
    assert_eq!(on.outcome, SignInPressDid::Done);
    assert_eq!(on.answer.map(|answer| answer.row), Some(SignInRow::On));
    assert_eq!(on.press_line(), "BT_SIGN_IN press on written");
    assert_eq!(
        registry.sign_in_entry_value(),
        Some(&entry_of(&copies.this))
    );
    assert_eq!(
        registry.value_under(SIGN_IN_TEST_KEYS.approved, ENTRY_NAME),
        None,
        "On takes the system's switch off its name"
    );
    assert_eq!(others(&registry), before);

    let off = pressed(&mut registry, &copies.this, false);
    assert_eq!(off.outcome, SignInPressDid::Done);
    assert_eq!(off.answer.map(|answer| answer.row), Some(SignInRow::Off));
    assert_eq!(off.press_line(), "BT_SIGN_IN press off removed");
    assert_eq!(
        registry.sign_in_entry_value(),
        None,
        "Off removes the entry"
    );
    assert_eq!(others(&registry), before, "and nothing else");
    assert_eq!(
        off.press_card(),
        (true, Text::SignInOffToast.text().to_owned()),
        "the press is answered with its card"
    );
}

/// RED (T-START-AT-LOGON-SWITCH, review N2) — **the press's card is the sign-in switch's own**: a
/// program the press cannot name changes nothing in the switch's sentence, never the hooks'; a
/// step the registry refused is the step the entry could not take, in the system's words and
/// without the door's module name; and an On whose entry is written while its Startup-apps value
/// would not come off says that fact — not "Not changed".
#[test]
fn the_press_card_says_the_sign_in_switchs_own_sentences() {
    let copies = sign_in_copies("sign-in-cards");
    let presses = SignInPresses::new();
    presses.note_sign_in_press(true);
    let mut registry = SignInFake::default();
    let unnamed = press_sign_in_in(
        &mut registry,
        SIGN_IN_TEST_KEYS,
        Err(Text::AgentHooksExeUnknown.text()),
        &what_stands,
        &presses,
        &Mutex::new(()),
    );
    let (done, card) = unnamed.press_card();
    assert!(!done);
    assert_eq!(
        card,
        crate::i18n::sign_in_not_changed(Text::SignInProgramNotStartable.text())
    );
    assert!(registry.read.is_empty() && registry.written.is_empty());

    let mut registry = SignInFake {
        refuse_set: true,
        ..SignInFake::default()
    };
    let refused = pressed(&mut registry, &copies.this, true);
    let (done, card) = refused.press_card();
    assert!(!done);
    assert_eq!(
        card,
        crate::i18n::sign_in_entry_failed(
            bt_platform::sign_in_entry::EntryStage::Write,
            SIGN_IN_DENIED
        )
    );
    assert!(!card.contains("sign_in_entry"), "{card}");
    assert_eq!(refused.press_line(), "BT_SIGN_IN press on refused");

    let mut registry = SignInFake {
        refuse_delete_under: Some(SIGN_IN_TEST_KEYS.run),
        ..SignInFake::default()
    };
    let (kind, data) = entry_of(&copies.this);
    registry.put_value(SIGN_IN_TEST_KEYS.run, ENTRY_NAME, kind, data);
    let (done, card) = pressed(&mut registry, &copies.this, false).press_card();
    assert!(!done);
    assert_eq!(
        card,
        crate::i18n::sign_in_entry_failed(
            bt_platform::sign_in_entry::EntryStage::Remove,
            SIGN_IN_DENIED
        )
    );

    let mut registry = SignInFake {
        refuse_delete_under: Some(SIGN_IN_TEST_KEYS.approved),
        ..SignInFake::default()
    };
    registry.put_value(
        SIGN_IN_TEST_KEYS.approved,
        ENTRY_NAME,
        REG_BINARY,
        vec![3, 0, 0, 0],
    );
    let on = pressed(&mut registry, &copies.this, true);
    assert_eq!(
        on.outcome,
        SignInPressDid::WrittenStillTurnedOff(SIGN_IN_DENIED.to_owned())
    );
    assert_eq!(
        registry.sign_in_entry_value(),
        Some(&entry_of(&copies.this)),
        "the entry is written"
    );
    assert_eq!(
        on.answer.map(|answer| answer.row),
        Some(SignInRow::TurnedOffBySystem)
    );
    assert_eq!(on.press_line(), "BT_SIGN_IN press on written still-off");
    let (done, card) = on.press_card();
    assert!(!done);
    assert_eq!(card, crate::i18n::sign_in_still_turned_off(SIGN_IN_DENIED));
    assert!(!card.starts_with("Not changed"), "{card}");
}

/// RED (T-START-AT-LOGON-SWITCH, R5, R7) — **off by default: an account that never pressed the
/// switch has no entry, and the start's member creates none.** The row reads `Unknown` (drawn Off)
/// until an answer lands, and the restart registration keeps the ruled flags.
///
/// MUTATION (`plant-22-member-creates-an-absent-entry.py`): count an absent entry as a repair and
/// the member writes one.
#[test]
fn a_fresh_account_has_no_entry_until_the_switch_is_pressed() {
    let copies = sign_in_copies("sign-in-fresh");
    let mut registry = SignInFake::default();
    let run = carry_sign_in_in(
        &mut registry,
        SIGN_IN_TEST_KEYS,
        &copies.this,
        &what_stands,
        sign_in_gate_never_asked,
        &SignInPresses::new(),
        &Mutex::new(()),
    );
    assert_eq!(run.found, SignInFound::Absent);
    assert_eq!(run.repaired, None);
    assert_eq!(run.answer.row, SignInRow::Off);
    assert_eq!(run.sign_in_line(), "BT_SIGN_IN absent");
    assert!(
        registry.written.is_empty(),
        "nothing written: {:?}",
        registry.written
    );
    assert_eq!(SignInRow::default(), SignInRow::Unknown);
    assert!(!SignInRow::Unknown.switch_on());
    assert_eq!(
        restart_flags_for(SignInRow::Unknown),
        bt_platform::session_end::RESTART_FLAGS
    );
}

/// RED (T-START-AT-LOGON-SWITCH, R2) — **the entry names the running copy, links followed, and
/// carries nothing after the quoted path**: both writers take this copy from
/// `attention_ownership::this_copy` (the running image's own file, `bt_platform::running_image`)
/// and gate it with `stable_executable`; what the press writes is [`command_for`] of it; a path too
/// long for a `Run` value is refused before anything is written, with its sentence.
///
/// MUTATION (`plant-22-links-not-followed.py`): the press takes `std::env::current_exe()` and the
/// first assertion names it.
#[test]
fn the_entry_names_the_running_copy() {
    for door in ["begin_sign_in_press_with", "run_sign_in_step"] {
        let body = squeezed(free_fn_body(door));
        assert!(
            body.contains("crate::attention_ownership::this_copy()")
                && body.contains("crate::attention_ownership::stable_executable(this.as_deref())"),
            "{door} names this copy by its running image, gated: {body}"
        );
        assert!(!body.contains("current_exe"), "{door}: {body}");
    }
    let copies = sign_in_copies("sign-in-command");
    let mut registry = SignInFake::default();
    let _ = pressed(&mut registry, &copies.this, true);
    let mut quoted = std::ffi::OsString::from("\"");
    quoted.push(copies.this.as_os_str());
    quoted.push("\"");
    assert_eq!(registry.sign_in_entry_value(), Some(&(REG_SZ, sz(&quoted))));

    let long = copies
        .this
        .parent()
        .unwrap()
        .join("x".repeat(bt_platform::logon_hook::COMMAND_LIMIT))
        .join("folio.exe");
    let mut registry = SignInFake::default();
    let refused = pressed(&mut registry, &long, true);
    assert_eq!(
        refused.outcome,
        SignInPressDid::Refused(crate::i18n::sign_in_not_changed(
            Text::SignInPathTooLong.text()
        ))
    );
    assert_eq!(refused.press_line(), "BT_SIGN_IN press on refused");
    assert!(registry.written.is_empty(), "nothing written");
}

/// RED (T-START-AT-LOGON-SWITCH, R7; the member's `proved_by` in `CarryCheck::in_the_pass`) —
/// **the carry pass points the entry of a copy that is gone at this copy**, says so once, and a
/// round that finds what it said says nothing.
///
/// MUTATION (`plant-22-member-skipped.py`): the member is not called in `run_carry_steps`, and the
/// inventory's guard names the check with no call.
#[test]
fn the_carry_pass_points_the_sign_in_entry_at_this_copy() {
    let copies = sign_in_copies("sign-in-carry");
    let mut registry = SignInFake::default();
    let (kind, data) = entry_of(&copies.gone);
    registry.put_value(SIGN_IN_TEST_KEYS.run, ENTRY_NAME, kind, data);
    let said = LastSaid::new();
    let lines = RefCell::new(Vec::new());
    let say = |line: String| lines.borrow_mut().push(line);

    let first = sign_in_carried(&mut registry, &copies.this);
    assert_eq!(first.found, SignInFound::Dead);
    assert_eq!(first.repaired.as_deref(), Some("written"));
    assert_eq!(
        first.answer.row,
        SignInRow::On,
        "read again after the write"
    );
    assert_eq!(
        registry.sign_in_entry_value(),
        Some(&entry_of(&copies.this))
    );
    let answer = sign_in_member_answer(&first, &said, &say);
    assert_eq!((answer.checks, answer.changed), (1, 1));

    let second = sign_in_carried(&mut registry, &copies.this);
    assert_eq!(second.found, SignInFound::Current);
    let _ = sign_in_member_answer(&second, &said, &say);
    let third = sign_in_carried(&mut registry, &copies.this);
    let quiet = sign_in_member_answer(&third, &said, &say);
    assert_eq!(quiet.changed, 0, "an unchanged round says nothing");
    assert_eq!(
        *lines.borrow(),
        ["BT_SIGN_IN dead repaired=written", "BT_SIGN_IN current"]
    );
    let root = copies.this.parent().unwrap().parent().unwrap();
    assert!(
        lines
            .borrow()
            .iter()
            .all(|line| !line.contains(&*root.to_string_lossy())),
        "never a path"
    );
}

/// One case of [`a_moved_copys_entry_is_pointed_at_this_one`]: what it is, the entry, whether its
/// volume is away, what the member finds, what it repairs, and the row after.
type SignInMovedCase<'a> = (
    &'a str,
    (u32, Vec<u8>),
    bool,
    SignInFound,
    Option<&'a str>,
    SignInRow,
);

/// RED (T-START-AT-LOGON-SWITCH, R7, R8; the check's old-shape test) — **a moved copy's entry is
/// pointed at this one**: this copy by another spelling and a copy that is gone are rewritten to
/// this copy exactly; another copy that is there, a copy on a volume that is away and another
/// program's value under the name are left byte for byte, and each is said.
///
/// MUTATIONS: re-point only an exact spelling (`plant-22-dead-left-not-repointed.py`) and the
/// gone copy's entry stays; take another live copy's (`plant-22-live-copy-taken.py`); judge by
/// `exists` alone (`plant-22-volume-ignored.py`) and the away copy's is rewritten.
#[test]
fn a_moved_copys_entry_is_pointed_at_this_one() {
    let copies = sign_in_copies("sign-in-moved");
    let tool = (REG_SZ, sz(OsStr::new(r#""C:\Tools\sync.exe""#)));
    let cases: [SignInMovedCase<'_>; 5] = [
        (
            "this copy by another spelling",
            entry_of(&copies.this_spelled_otherwise),
            false,
            SignInFound::ThisCopyOtherSpelling,
            Some("written"),
            SignInRow::On,
        ),
        (
            "a copy that is gone",
            entry_of(&copies.gone),
            false,
            SignInFound::Dead,
            Some("written"),
            SignInRow::On,
        ),
        (
            "another copy that is there",
            entry_of(&copies.other),
            false,
            SignInFound::AnotherCopy,
            None,
            SignInRow::OnElsewhere,
        ),
        (
            "a copy on a volume that is away",
            entry_of(&copies.gone),
            true,
            SignInFound::OwnerUnknown,
            None,
            SignInRow::OnElsewhere,
        ),
        (
            "another program's value",
            tool,
            false,
            SignInFound::NotOurs,
            None,
            SignInRow::NotOurs,
        ),
    ];
    for (what, (kind, data), away, found, repaired, row) in cases {
        let mut registry = SignInFake::default();
        registry.put_value(SIGN_IN_TEST_KEYS.run, ENTRY_NAME, kind, data.clone());
        let run = if away {
            carry_sign_in_in(
                &mut registry,
                SIGN_IN_TEST_KEYS,
                &copies.this,
                &SignInAwayVolume,
                sign_in_gate_never_asked,
                &SignInPresses::new(),
                &Mutex::new(()),
            )
        } else {
            sign_in_carried(&mut registry, &copies.this)
        };
        assert_eq!(run.found, found, "{what}");
        assert_eq!(run.repaired.as_deref(), repaired, "{what}");
        assert_eq!(run.answer.row, row, "{what}");
        let expected = if repaired.is_some() {
            entry_of(&copies.this)
        } else {
            (kind, data)
        };
        assert_eq!(registry.sign_in_entry_value(), Some(&expected), "{what}");
        assert!(run.sign_in_line().starts_with("BT_SIGN_IN "), "{what}");
    }
}

/// RED (T-START-AT-LOGON-SWITCH, R2) — **no other value of the `Run` key is read or touched, and a
/// value under the name that is not of Folio's shape is never judged**: the member, both presses
/// and the uninstall ask only for the entry's own name; another program's value under it — a
/// `REG_EXPAND_SZ`, or a plain `REG_SZ` quoting another program, the shape a real one has
/// (review N4) — is never judged: the member says so and writes nothing, On refuses with its
/// sentence, Off and the uninstall leave it and the Startup-apps value of the name.
///
/// MUTATION (`plant-22-foreign-value-overwritten.py`): On writes over any value under the name;
/// (`plant-22-reads-every-value.py`): the door judges any text quoted at its start.
#[test]
fn no_other_value_of_the_run_key_is_read_or_touched() {
    let copies = sign_in_copies("sign-in-foreign");
    let mut expanded = command_for(&copies.gone);
    expanded.push(" --background");
    for (what, foreign) in [
        ("an expandable string", (2, sz(&command_for(&copies.gone)))),
        (
            "another program's command",
            (REG_SZ, sz(OsStr::new(r#""C:\Tools\sync.exe""#))),
        ),
    ] {
        let mut registry = SignInFake::default();
        registry.put_value(
            SIGN_IN_TEST_KEYS.run,
            ENTRY_NAME,
            foreign.0,
            foreign.1.clone(),
        );
        registry.put_value(
            SIGN_IN_TEST_KEYS.approved,
            ENTRY_NAME,
            REG_BINARY,
            vec![3, 0, 0, 0],
        );
        registry.put_value(
            SIGN_IN_TEST_KEYS.run,
            "FolioUpdate-abcd0123",
            REG_SZ,
            sz(OsStr::new("x")),
        );
        registry.put_value(SIGN_IN_TEST_KEYS.run, "SomebodyElse", REG_SZ, sz(&expanded));
        let before = registry.values.clone();

        let run = sign_in_carried(&mut registry, &copies.this);
        assert_eq!(run.found, SignInFound::NotOurs, "{what}");
        assert_eq!(run.answer.row, SignInRow::NotOurs, "{what}");
        let on = pressed(&mut registry, &copies.this, true);
        assert_eq!(
            on.outcome,
            SignInPressDid::Refused(crate::i18n::sign_in_not_changed(
                Text::SignInEntryNotOurs.text()
            )),
            "{what}"
        );
        let off = pressed(&mut registry, &copies.this, false);
        assert_eq!(off.outcome, SignInPressDid::Done, "{what}");
        assert_eq!(
            uninstall_sign_in_in(&mut registry, SIGN_IN_TEST_KEYS, &copies.this, &what_stands),
            SignInRemoval::LeftNotOurs,
            "{what}"
        );
        assert_eq!(registry.values, before, "{what}: every value byte for byte");
        assert!(
            registry.written.is_empty(),
            "{what}: {:?}",
            registry.written
        );
        assert!(
            registry.read.iter().all(|name| name == ENTRY_NAME),
            "{what}: only the entry's own name is read: {:?}",
            registry.read
        );
    }

    // A quoted command with anything after it is not of the shape either.
    let mut registry = SignInFake::default();
    registry.put_value(SIGN_IN_TEST_KEYS.run, ENTRY_NAME, REG_SZ, sz(&expanded));
    assert_eq!(
        sign_in_carried(&mut registry, &copies.this).found,
        SignInFound::NotOurs
    );
}

/// RED (T-START-AT-LOGON-SWITCH, R7, R11) — **the member's repair waits for the trial's commit**:
/// during an update's trial a repair that is due asks the gate once, is held and recorded as
/// pending ([`Writer::StartAtSignIn`]), and nothing is written; the commit's release asks the pass
/// for this member, whose round writes it.
///
/// MUTATION (`plant-22-unheld.py`): write without asking the gate and the trial's round writes.
#[test]
fn the_sign_in_entry_waits_for_the_trials_commit() {
    let copies = sign_in_copies("sign-in-trial");
    let mut registry = SignInFake::default();
    let (kind, data) = entry_of(&copies.gone);
    registry.put_value(SIGN_IN_TEST_KEYS.run, ENTRY_NAME, kind, data.clone());
    let gate = crate::update_trial::Gate::new();
    let asked = std::cell::Cell::new(0);
    let held = carry_sign_in_in(
        &mut registry,
        SIGN_IN_TEST_KEYS,
        &copies.this,
        &what_stands,
        |writer| {
            asked.set(asked.get() + 1);
            gate.defer(true, writer)
        },
        &SignInPresses::new(),
        &Mutex::new(()),
    );
    assert_eq!(asked.get(), 1, "the gate is asked once");
    assert_eq!(held.repaired.as_deref(), Some("held"));
    assert_eq!(
        registry.sign_in_entry_value(),
        Some(&(kind, data)),
        "nothing written"
    );
    assert_eq!(gate.pending(), vec![Writer::StartAtSignIn]);
    assert!(
        squeezed(free_fn_body("release_trial_writer")).contains(&squeezed(
            "Writer::StartAtSignIn => carry_pass::begin_carry_pass(CarryAsk::START_AT_SIGN_IN)"
        )),
        "the commit's release asks for this member"
    );
    let committed = sign_in_carried(&mut registry, &copies.this);
    assert_eq!(committed.repaired.as_deref(), Some("written"));
    assert_eq!(
        registry.sign_in_entry_value(),
        Some(&entry_of(&copies.this))
    );
}

/// RED (T-START-AT-LOGON-SWITCH, R6) — **a press during the carry wins**: while a press holds the
/// entry's latch the member writes nothing, and a press counted after the member's reading stops
/// its write too — `repaired=pressed` either way, the press's own answer being the newer.
///
/// MUTATION (`plant-22-press-loses.py`): the member waits for the latch instead of trying it, or
/// writes whatever the count says.
#[test]
fn a_press_during_the_carry_wins() {
    let copies = sign_in_copies("sign-in-press-wins");
    let dead = entry_of(&copies.gone);
    let writes = Mutex::new(());
    let presses = SignInPresses::new();

    let mut registry = SignInFake::default();
    registry.put_value(SIGN_IN_TEST_KEYS.run, ENTRY_NAME, dead.0, dead.1.clone());
    let latched = writes.lock().unwrap();
    let run = carry_sign_in_in(
        &mut registry,
        SIGN_IN_TEST_KEYS,
        &copies.this,
        &what_stands,
        |_| false,
        &presses,
        &writes,
    );
    drop(latched);
    assert_eq!(run.repaired.as_deref(), Some("pressed"));
    assert_eq!(
        registry.sign_in_entry_value(),
        Some(&dead),
        "nothing written under a press"
    );

    let pressing_look = |path: &Path| {
        presses.note_sign_in_press(false);
        what_stands(path)
    };
    let run = carry_sign_in_in(
        &mut registry,
        SIGN_IN_TEST_KEYS,
        &copies.this,
        &pressing_look,
        |_| false,
        &presses,
        &writes,
    );
    assert_eq!(run.repaired.as_deref(), Some("pressed"));
    assert_eq!(
        registry.sign_in_entry_value(),
        Some(&dead),
        "nor after a press counted since the reading"
    );
}

/// RED (T-START-AT-LOGON-SWITCH, R6; 68b's R4 rule) — **an answer read before the last press is
/// not adopted**: the press's own answer is newer, and an answer that changes nothing is no
/// change.
///
/// MUTATION (`plant-22-stale-answer-adopted.py`): adopt every answer.
#[test]
fn a_sign_in_answer_older_than_a_press_is_not_adopted() {
    let presses = SignInPresses::new();
    let mut row = SignInRow::Unknown;
    let answer = |row, presses_seen| SignInAnswer { row, presses_seen };
    assert!(adopt_sign_in_answer(
        &mut row,
        answer(SignInRow::Off, 0),
        &presses
    ));
    assert_eq!(row, SignInRow::Off);
    assert!(!adopt_sign_in_answer(
        &mut row,
        answer(SignInRow::Off, 0),
        &presses
    ));
    presses.note_sign_in_press(true);
    assert!(!adopt_sign_in_answer(
        &mut row,
        answer(SignInRow::On, 0),
        &presses
    ));
    assert_eq!(row, SignInRow::Off, "read before the press: dropped");
    assert!(adopt_sign_in_answer(
        &mut row,
        answer(SignInRow::On, 1),
        &presses
    ));
    assert_eq!(row, SignInRow::On);
}

/// RED (T-START-AT-LOGON-SWITCH, R6, review N1) — **a member's reading taken while a press is
/// counted but not yet answered is not adopted**: the press is counted on the window thread and
/// written on its worker, so a member that reads in between describes the machine before the
/// press; its answer carries the presses answered when it read, and the window drops it whichever
/// of the two lands last — before the press's worker has run, after it, and in either order with
/// the press's answer. A reading taken after the press's worker answered is adopted.
///
/// MUTATION (`plant-22-member-answer-carries-the-noted-count.py`): the member's answer carries the
/// presses counted, and the stale reading is taken over the press's answer.
#[test]
fn a_member_reading_taken_before_a_press_was_answered_is_not_adopted() {
    let copies = sign_in_copies("sign-in-unanswered");
    let mut registry = SignInFake::default();
    let (kind, data) = entry_of(&copies.this);
    registry.put_value(SIGN_IN_TEST_KEYS.run, ENTRY_NAME, kind, data);
    let presses = SignInPresses::new();
    let writes = Mutex::new(());
    let carried = |registry: &mut SignInFake| {
        carry_sign_in_in(
            registry,
            SIGN_IN_TEST_KEYS,
            &copies.this,
            &what_stands,
            sign_in_gate_never_asked,
            &presses,
            &writes,
        )
    };

    // Off is pressed; the member reads before the press's worker has run.
    presses.note_sign_in_press(false);
    let stale = carried(&mut registry);
    assert_eq!(stale.answer.row, SignInRow::On, "read before the removal");

    // The stale reading lands while the press's worker has not run: dropped.
    let mut row = SignInRow::Unknown;
    assert!(
        !adopt_sign_in_answer(&mut row, stale.answer, &presses),
        "a reading taken while the press is unanswered, landing before the press's answer"
    );
    assert_eq!(row, SignInRow::Unknown);

    let off = press_sign_in_in(
        &mut registry,
        SIGN_IN_TEST_KEYS,
        Ok(&copies.this),
        &what_stands,
        &presses,
        &writes,
    );
    assert_eq!(registry.sign_in_entry_value(), None, "the press removed it");

    // The press's answer lands first, then the stale reading: the row stays Off.
    let mut row = SignInRow::Unknown;
    assert!(adopt_sign_in_answer(
        &mut row,
        off.answer.unwrap(),
        &presses
    ));
    assert_eq!(row, SignInRow::Off);
    assert!(
        !adopt_sign_in_answer(&mut row, stale.answer, &presses),
        "a reading taken before the press was answered is older than the press"
    );
    assert_eq!(row, SignInRow::Off);

    // The stale reading lands first, then the press's answer: the stale one is dropped and the
    // press's taken.
    let mut row = SignInRow::Unknown;
    assert!(
        !adopt_sign_in_answer(&mut row, stale.answer, &presses),
        "the stale reading landing first is dropped as well"
    );
    assert_eq!(row, SignInRow::Unknown);
    assert!(adopt_sign_in_answer(
        &mut row,
        off.answer.unwrap(),
        &presses
    ));
    assert_eq!(row, SignInRow::Off);

    // A reading taken after the press answered is the machine's word.
    let fresh = carried(&mut registry);
    let mut row = SignInRow::Unknown;
    assert!(adopt_sign_in_answer(&mut row, fresh.answer, &presses));
    assert_eq!(row, SignInRow::Off);
}

/// RED (T-START-AT-LOGON-SWITCH, R9) — **an entry turned off in Startup apps reads so, and the
/// member writes nothing over it** (the person's choice); a press On takes the system's switch off
/// the entry's name and leaves the switch of every other name.
///
/// MUTATION (`plant-22-system-toggle-ignored.py`): judge the entry without the system's switch and
/// the member rewrites this copy's other spelling under it.
#[test]
fn an_entry_turned_off_in_startup_apps_reads_off_and_on_turns_it_back_on() {
    let copies = sign_in_copies("sign-in-approved");
    let mut registry = SignInFake::default();
    let (kind, data) = entry_of(&copies.this_spelled_otherwise);
    registry.put_value(SIGN_IN_TEST_KEYS.run, ENTRY_NAME, kind, data.clone());
    registry.put_value(
        SIGN_IN_TEST_KEYS.approved,
        ENTRY_NAME,
        REG_BINARY,
        vec![3, 0, 0, 0, 9, 9, 9, 9],
    );
    registry.put_value(
        SIGN_IN_TEST_KEYS.approved,
        "SomebodyElse",
        REG_BINARY,
        vec![3, 0, 0, 0],
    );

    let run = sign_in_carried(&mut registry, &copies.this);
    assert_eq!(run.found, SignInFound::TurnedOffBySystem);
    assert_eq!(run.repaired, None);
    assert_eq!(run.answer.row, SignInRow::TurnedOffBySystem);
    assert_eq!(run.sign_in_line(), "BT_SIGN_IN turned off in Startup apps");
    assert!(registry.written.is_empty(), "{:?}", registry.written);
    assert!(!SignInRow::TurnedOffBySystem.switch_on());

    let on = pressed(&mut registry, &copies.this, true);
    assert_eq!(on.answer.map(|answer| answer.row), Some(SignInRow::On));
    assert_eq!(
        registry.value_under(SIGN_IN_TEST_KEYS.approved, ENTRY_NAME),
        None
    );
    assert_eq!(
        registry.value_under(SIGN_IN_TEST_KEYS.approved, "SomebodyElse"),
        Some(&(REG_BINARY, vec![3, 0, 0, 0])),
        "another name's switch stays"
    );
}

/// RED (T-START-AT-LOGON-SWITCH, R8, review N9) — **On over another copy's entry says it took it
/// over**: the entry of another copy that is there, or of one on a volume that is away, is pointed
/// at this copy without asking (R8), and the card states that this copy now starts at sign-in
/// instead of the other; over this copy's own entry, or none, the card is the plain one.
#[test]
fn a_press_over_another_copys_entry_says_it_took_it_over() {
    let copies = sign_in_copies("sign-in-took-over");
    let away = copies.gone.with_file_name("away").join("folio.exe");
    let press_over = |program: &Path, look: &dyn Look| {
        let mut registry = SignInFake::default();
        let (kind, data) = entry_of(program);
        registry.put_value(SIGN_IN_TEST_KEYS.run, ENTRY_NAME, kind, data);
        let presses = SignInPresses::new();
        presses.note_sign_in_press(true);
        let on = press_sign_in_in(
            &mut registry,
            SIGN_IN_TEST_KEYS,
            Ok(&copies.this),
            look,
            &presses,
            &Mutex::new(()),
        );
        assert_eq!(
            registry.sign_in_entry_value(),
            Some(&entry_of(&copies.this)),
            "the entry names this copy"
        );
        on
    };
    for (what, program, look) in [
        (
            "another copy that is there",
            &copies.other,
            &what_stands as &dyn Look,
        ),
        ("a copy on a volume that is away", &away, &SignInAwayVolume),
    ] {
        let on = press_over(program, look);
        assert_eq!(on.outcome, SignInPressDid::TookOver, "{what}");
        assert_eq!(on.press_line(), "BT_SIGN_IN press on taken over", "{what}");
        assert_eq!(
            on.press_card(),
            (true, Text::SignInTookOverToast.text().to_owned()),
            "{what}"
        );
    }
    let own = press_over(&copies.this, &what_stands);
    assert_eq!(own.outcome, SignInPressDid::Done);
    assert_eq!(
        own.press_card(),
        (true, Text::SignInOnToast.text().to_owned())
    );
}

/// RED (T-START-AT-LOGON-SWITCH, focused re-review F1) — **a press that ends without an answer
/// marks the presses answered only when no other press's worker is in flight**. A start the thread
/// door refuses (the suite's gate refuses it here) is counted down like a worker that answered, and
/// the last one down marks every press counted before it as answered, so a refused press does not
/// keep member readings from being adopted until the next press; its answer is the press's
/// failure, with the presses counted. While an earlier press's worker is still to write, nothing is
/// marked: a member reading taken then describes the machine before that write and is dropped. A
/// worker that answered and is the last one down marks the presses refused since its answer; a
/// worker that runs counts itself down. Both ends go through the count and neither marks it
/// itself. (A panicking worker takes the same end as a refused start; the press's body has no
/// task for the suite's gate to make panic.)
///
/// MUTATION (`plant-22a-f1.py`): the end marks the count whatever is in flight.
#[test]
fn a_refused_press_marks_the_presses_answered_only_when_no_worker_is_left() {
    static SIGN_IN_ALONE_REFUSED: SignInPresses = SignInPresses::new();
    static SIGN_IN_REFUSED_BEHIND: SignInPresses = SignInPresses::new();
    static SIGN_IN_WORKER_RAN: SignInPresses = SignInPresses::new();
    let refusing = || {
        let gate = std::sync::Arc::new(crate::lane::Gate::default());
        gate.refuse_next_start();
        crate::lane::Executor::scripted(gate)
    };
    let body = squeezed(free_fn_body("begin_sign_in_press_with"));
    assert_eq!(
        body.matches("presses.sign_in_press_ended()").count(),
        2,
        "the worker and the end both count the press down: {body}"
    );
    assert!(!body.contains("note_sign_in_answered"), "{body}");

    // One press, its start refused: no worker is left, and member readings are the machine's word.
    let presses = &SIGN_IN_ALONE_REFUSED;
    begin_sign_in_press_with(&refusing(), presses, true);
    assert_eq!(presses.sign_in_presses_answered(), 1);
    let refused = take_sign_in_press().expect("the refused press is answered");
    assert_eq!(refused.presses_seen, 1);
    assert!(matches!(refused.outcome, SignInPressDid::Refused(_)));
    let mut row = SignInRow::Unknown;
    let read_now = SignInAnswer {
        row: SignInRow::Off,
        presses_seen: presses.sign_in_presses_answered(),
    };
    assert!(adopt_sign_in_answer(&mut row, read_now, presses));

    // Press 1 is counted and its worker is still to write when press 2's start is refused: nothing
    // is marked, and a member reading taken now is dropped once press 1's worker has answered both
    // presses.
    let copies = sign_in_copies("sign-in-in-flight");
    let mut registry = SignInFake::default();
    let writes = Mutex::new(());
    let presses = &SIGN_IN_REFUSED_BEHIND;
    presses.note_sign_in_press(true);
    begin_sign_in_press_with(&refusing(), presses, true);
    assert_eq!(
        presses.sign_in_presses_answered(),
        0,
        "press 1's worker is in flight: its write is still to come"
    );
    let refused = take_sign_in_press().expect("the refused press is answered");
    assert_eq!(refused.presses_seen, 2);
    let stale = carry_sign_in_in(
        &mut registry,
        SIGN_IN_TEST_KEYS,
        &copies.this,
        &what_stands,
        sign_in_gate_never_asked,
        presses,
        &writes,
    );
    assert_eq!(
        stale.answer.row,
        SignInRow::Off,
        "read before press 1's write"
    );
    let on = press_sign_in_in(
        &mut registry,
        SIGN_IN_TEST_KEYS,
        Ok(&copies.this),
        &what_stands,
        presses,
        &writes,
    );
    presses.sign_in_press_ended();
    assert_eq!(on.presses_seen, 2, "press 1's worker answers both presses");
    assert_eq!(presses.sign_in_presses_answered(), 2);
    let mut row = SignInRow::Unknown;
    assert!(adopt_sign_in_answer(&mut row, on.answer.unwrap(), presses));
    assert_eq!(row, SignInRow::On);
    assert!(
        !adopt_sign_in_answer(&mut row, stale.answer, presses),
        "a reading taken while press 1's worker was in flight is older than its write"
    );
    assert_eq!(row, SignInRow::On);

    // Press 1's worker has answered press 1 alone and not yet ended when press 2 is counted and
    // its start refused: the end leaves the mark to the worker, and the worker, last down, makes
    // it.
    let presses = SignInPresses::new();
    presses.note_sign_in_press(true);
    presses.note_sign_in_answered(1);
    presses.note_sign_in_press(false);
    presses.sign_in_press_ended();
    assert_eq!(presses.sign_in_presses_answered(), 1);
    presses.sign_in_press_ended();
    assert_eq!(
        presses.sign_in_presses_answered(),
        2,
        "no worker is left: press 2 is marked"
    );

    // A worker that runs counts itself down (in a test process it touches no registry).
    let presses = &SIGN_IN_WORKER_RAN;
    let gate = std::sync::Arc::new(crate::lane::Gate::default());
    begin_sign_in_press_with(&crate::lane::Executor::scripted(gate), presses, true);
    crate::lane::wait_until("the press's worker counted itself down", || {
        presses.sign_in_presses_answered() == 1
    });
}

/// RED (T-START-AT-LOGON-SWITCH, R10) — **while this copy's entry is on, the restart registration
/// leaves the reboot's restart to it**, and keeps the ruled flags in every other state.
///
/// MUTATION (`plant-22-restart-flags-unchanged.py`): the flags ignore the row.
#[test]
fn the_restart_flags_leave_sign_in_to_the_entry_while_it_is_on() {
    use bt_platform::session_end::{RESTART_FLAGS, RESTART_NO_REBOOT};
    assert_eq!(
        restart_flags_for(SignInRow::On),
        RESTART_FLAGS | RESTART_NO_REBOOT
    );
    for row in [
        SignInRow::Unknown,
        SignInRow::Off,
        SignInRow::OnElsewhere,
        SignInRow::TurnedOffBySystem,
        SignInRow::NotOurs,
    ] {
        assert_eq!(restart_flags_for(row), RESTART_FLAGS, "{row:?}");
    }
}
