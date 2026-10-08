//! **The installation's transaction, as a protocol and nothing else** — the
//! journal's frozen header, the phases a transaction passes through and who may
//! write each, the trial's receipt, the member inventories, and the decision an
//! actor makes from what it finds on disk
//! (`docs/plans/design/self-update-2026-09-16.md`, revision 2026-09-25 (b),
//! §(b).2; this module is (b).5's ticket U-10).
//!
//! # Pure, by contract
//!
//! Nothing here reads or writes a file, takes a lock, reads a clock, starts or
//! ends a process or touches the registry. The caller describes the disk
//! ([`Disk`], [`StartView`]), is handed an [`Action`] or a [`StartAction`], and
//! performs it through the doors that own the effects — U-11's journal writes
//! and locks, U-22 and U-26's entrances, U-23's moves. That is (b).1 F-14's
//! "`update_txn::decide` stays pure", and it is what lets every row of (b).2's
//! two recovery tables be a test that runs in microseconds.
//!
//! # The actors
//!
//! **O** is the running build and the in-app job owner; **P** (the applier) and
//! **R** (recovery) are both the rescue copy of O's own executable holding the
//! transaction lock (F-8); **N** is the trial, the new build P started with a
//! per-trial nonce; and any **ordinary start** reads only the header. [`Actor`]
//! names them, [`JOURNAL_WRITERS`] says who may write which phase and
//! [`EFFECT_RIGHTS`] who may touch which file in which phase.
//!
//! # What crosses versions
//!
//! Only the [`Header`] (`{v, txn, rescue, class, outcome}`, and since 0.4.8 the
//! writer's version `written_by`) and the [`Receipt`] (`{v, txn, nonce, pid,
//! version}`) are read by a build other than the one that wrote them: an
//! ordinary start of any later version reads the header, and the rescue build
//! (a copy of O) reads the receipt the new build wrote. They are frozen at v1,
//! carry their version, and refuse one they do not know by name. The [`Body`]
//! is written and read by the rescue build alone.
//!
//! # A journal this build cannot read (0.4.8 E1)
//!
//! Every production read of a journal goes through [`sight`], and of a
//! receipt through [`receipt_sight`]; each reader's answer to what it cannot
//! read whole is its row of one table ([`Role::beyond`]). **The envelope rule,
//! frozen for ever:** whatever a later header's `v`, `class` or `outcome` say,
//! its `txn`, `rescue` and `written_by` keep their names, types and meaning,
//! so a build that reads nothing else of a journal still finds the rescue
//! build that can. The `class` and `outcome` vocabularies are closed: a later
//! build adds no class and no outcome. A later body or receipt word that an
//! older rescue build would misread raises the release manifest's
//! `min_updater` to its first version.
//!
//! **Nothing in this file is called from the product yet, and that is the
//! ticket boundary**: U-11 gives it its door, U-12 its startup caller and
//! U-20…U-29 its drivers.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the protocol lands before its drivers: U-11, U-12 and U-20…U-29 call it ((b).5)"
    )
)]

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use bt_platform::HostPlatform;

use serde::{Deserialize, Serialize};

/// The one version of the journal header this build writes and reads.
pub(crate) const HEADER_VERSION: u64 = 1;

/// The one version of the trial's receipt this build writes and reads.
pub(crate) const RECEIPT_VERSION: u64 = 1;

/// **How long a trial has to prove itself** — §C.5's 90 s, counted from the
/// wall-clock instant recorded in [`Phase::Trial`] so that a recovery started
/// by another process (R) measures the same deadline P did.
pub(crate) const TRIAL_DEADLINE_MS: u64 = 90_000;

/// **A prepared transaction is discarded at its second launch without a
/// resume** — (b).1 F-17's "increments `deferred_launches` at each launch that
/// does not resume, discards at 2".
pub(crate) const DEFERRED_LAUNCH_LIMIT: u8 = 2;

/// **How many rollbacks a `Stuck` transaction gets** — the coordinator's
/// ruling 4 for U-29: W10/M10's "again at every logon and every start",
/// bounded. `Stuck` counts the failed attempts it has recorded
/// ([`Phase::Stuck`]'s `attempts`); at this many, a lock holder tries no more
/// and the transaction stays `Stuck`, its journal, rollback material and
/// entrance kept, for a person to finish from the folder the card names.
pub(crate) const STUCK_ATTEMPT_LIMIT: u8 = 3;

/// The receipt's file name is this prefix and the trial's nonce
/// (`H\<txn>\health-<nonce>`, (b).2's objects table).
pub(crate) const RECEIPT_FILE_PREFIX: &str = "health-";

// ───────────────────────────── fixed-length values ─────────────────────────────

/// Why bytes offered as a header, a receipt or a journal are not one.
///
/// Every refusal is named: a start that meets a journal it cannot read must be
/// able to say which of these it met, and a test must be able to tell a
/// truncated file from one written by a version this build predates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ParseRefusal {
    /// The bytes end inside the document: an empty file, or one cut short.
    Truncated,
    /// A complete document whose `v` is not the one version this build knows.
    UnknownVersion(u64),
    /// A fixed-length hex field of the wrong length, counted in hex digits.
    WrongLength {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    /// A fixed-length field holding something other than lowercase hex.
    NotHex { field: &'static str },
    /// A header `class` that is none of the four.
    UnknownClass(String),
    /// A journal whose header class is not the class of its body's phase.
    ClassDisagreesWithPhase { class: Class, phase: PhaseKind },
    /// A header `outcome` that is none of the three.
    UnknownOutcome(String),
    /// A journal whose header outcome is not the outcome of its body's phase.
    OutcomeDisagreesWithPhase {
        outcome: HeaderOutcome,
        phase: PhaseKind,
    },
    /// Anything else: not JSON, a field missing or of the wrong type.
    Malformed(String),
    /// **The bytes could not be read at all** (E1 round 2): the read failed
    /// with this operating-system error, which is not "no such file" — a
    /// sharing violation, a refused access, a folder where the file should
    /// be. Nothing is known of the document, and nothing is concluded from
    /// its absence ([`sight_of_read`]).
    Unread(String),
}

impl fmt::Display for ParseRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("the document is truncated"),
            Self::UnknownVersion(v) => write!(f, "version {v} is not one this build reads"),
            Self::WrongLength {
                field,
                expected,
                found,
            } => write!(f, "`{field}` has {found} hex digits, not {expected}"),
            Self::NotHex { field } => write!(f, "`{field}` is not lowercase hex"),
            Self::UnknownClass(class) => write!(f, "`{class}` is not a transaction class"),
            Self::ClassDisagreesWithPhase { class, phase } => {
                write!(f, "class {class:?} is not the class of phase {phase:?}")
            }
            Self::UnknownOutcome(outcome) => {
                write!(f, "`{outcome}` is not a transaction outcome")
            }
            Self::OutcomeDisagreesWithPhase { outcome, phase } => {
                write!(
                    f,
                    "outcome {outcome:?} is not the outcome of phase {phase:?}"
                )
            }
            Self::Malformed(why) => write!(f, "malformed: {why}"),
            Self::Unread(error) => write!(f, "it could not be read: {error}"),
        }
    }
}

fn nibble(field: &'static str, digit: u8) -> Result<u8, ParseRefusal> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        _ => Err(ParseRefusal::NotHex { field }),
    }
}

fn parse_hex<const N: usize>(field: &'static str, text: &str) -> Result<[u8; N], ParseRefusal> {
    if text.len() != 2 * N {
        return Err(ParseRefusal::WrongLength {
            field,
            expected: 2 * N,
            found: text.len(),
        });
    }
    let mut bytes = [0u8; N];
    for (byte, pair) in bytes.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        *byte = (nibble(field, pair[0])? << 4) | nibble(field, pair[1])?;
    }
    Ok(bytes)
}

/// **Which field a fixed-length value is**, named in its refusals.
pub(crate) trait Field {
    const NAME: &'static str;
}

/// A fixed-length byte string written as lowercase hex; `K` says which field
/// it is, so a transaction id is never mistaken for a nonce or a digest.
pub(crate) struct Fixed<K, const N: usize> {
    bytes: [u8; N],
    field: PhantomData<K>,
}

impl<K: Field, const N: usize> Fixed<K, N> {
    pub(crate) const fn new(bytes: [u8; N]) -> Self {
        Self {
            bytes,
            field: PhantomData,
        }
    }

    /// Reads the lowercase hex this value is written as.
    pub(crate) fn parse(text: &str) -> Result<Self, ParseRefusal> {
        parse_hex::<N>(K::NAME, text).map(Self::new)
    }

    /// The bytes themselves.
    pub(crate) const fn bytes(&self) -> &[u8; N] {
        &self.bytes
    }
}

impl<K, const N: usize> Clone for Fixed<K, N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K, const N: usize> Copy for Fixed<K, N> {}

impl<K, const N: usize> PartialEq for Fixed<K, N> {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl<K, const N: usize> Eq for Fixed<K, N> {}

impl<K, const N: usize> fmt::Display for Fixed<K, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.bytes
            .iter()
            .try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl<K: Field, const N: usize> fmt::Debug for Fixed<K, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({self})", K::NAME)
    }
}

impl<K, const N: usize> Serialize for Fixed<K, N> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de, K: Field, const N: usize> Deserialize<'de> for Fixed<K, N> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// The field a [`TxnId`] is.
pub(crate) enum TxnField {}
impl Field for TxnField {
    const NAME: &'static str = "txn";
}

/// The field a [`Nonce`] is.
pub(crate) enum NonceField {}
impl Field for NonceField {
    const NAME: &'static str = "nonce";
}

/// The field a [`Digest`] is.
pub(crate) enum DigestField {}
impl Field for DigestField {
    const NAME: &'static str = "digest";
}

/// The field a [`Cdhash`] is.
pub(crate) enum CdhashField {}
impl Field for CdhashField {
    const NAME: &'static str = "cdhash";
}

/// **A transaction's identity**: 16 random bytes minted by O at `Allocated`.
/// It names `H\<txn>`, and its first eight hex digits name the entrance
/// (`FolioUpdate-<txn8>`).
pub(crate) type TxnId = Fixed<TxnField, 16>;

/// **A per-actor secret**: 32 random bytes. The trial's nonce is handed to N
/// alone on its command line, and a receipt counts only if it carries it back;
/// the applier's nonce is what O hands P in `Handoff`.
pub(crate) type Nonce = Fixed<NonceField, 32>;

/// **A member's SHA-256.**
pub(crate) type Digest = Fixed<DigestField, 32>;

/// **A bundle's main executable's code-directory hash** (the 20-byte cdhash
/// macOS reports), which with the version is the bundle's identity.
pub(crate) type Cdhash = Fixed<CdhashField, 20>;

/// Reads a JSON document, telling a document cut short from one that is wrong.
fn json<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, ParseRefusal> {
    serde_json::from_slice(bytes).map_err(|error| {
        if error.is_eof() {
            ParseRefusal::Truncated
        } else {
            ParseRefusal::Malformed(error.to_string())
        }
    })
}

/// The first question asked of a versioned document: which version is it?
#[derive(Deserialize)]
struct Versioned {
    v: u64,
}

fn versioned(bytes: &[u8], known: u64) -> Result<(), ParseRefusal> {
    let Versioned { v } = json(bytes)?;
    if v == known {
        Ok(())
    } else {
        Err(ParseRefusal::UnknownVersion(v))
    }
}

// ─────────────────────────────────── the header ───────────────────────────────────

/// **What an ordinary start may conclude from the header alone** ((b).2, "An
/// ordinary start of any version reads only the frozen header").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Class {
    /// O is still acquiring: downloading, expanding, copying, verifying.
    Preparing,
    /// Verified and waiting: the job owner resumes or discards it.
    Deferred,
    /// The install may be mid-change, or a lock holder owns an outcome it has
    /// not retired: a start that is not the trial hands itself to the rescue.
    Destructive,
    /// Finished: whatever is left in `H\<txn>`, the journal and the entrance
    /// may be deleted by anyone holding the lock.
    Terminal,
}

impl Class {
    fn word(self) -> &'static str {
        match self {
            Class::Preparing => "preparing",
            Class::Deferred => "deferred",
            Class::Destructive => "destructive",
            Class::Terminal => "terminal",
        }
    }

    fn from_word(word: &str) -> Result<Self, ParseRefusal> {
        match word {
            "preparing" => Ok(Class::Preparing),
            "deferred" => Ok(Class::Deferred),
            "destructive" => Ok(Class::Destructive),
            "terminal" => Ok(Class::Terminal),
            _ => Err(ParseRefusal::UnknownClass(word.to_owned())),
        }
    }
}

/// **What the transaction has decided, as the header says it** — frozen with
/// the header at v1 (coordinator ruling, 2026-09-27): `none` until a decision,
/// `committed` from `Committed` on, `rolled_back` from `RollbackIntent` on.
///
/// The class cannot say it: `Trial`, `Committed` and `RollbackIntent` are all
/// `destructive`, and both retirements `terminal`. The trial N reads this, and
/// only the header, to learn whether it may write (`update_trial`, U-13); the
/// lock holder writes it together with the phase it records, as the class is
/// ([`PhaseKind::outcome`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum HeaderOutcome {
    /// Nothing decided: every phase before `Committed` or `RollbackIntent`,
    /// and `Abandoned`, which moved nothing.
    None,
    /// `Committed`, and its retirement.
    Committed,
    /// `RollbackIntent`, `Stuck`, `RolledBack`, and their retirement.
    RolledBack,
}

impl HeaderOutcome {
    fn word(self) -> &'static str {
        match self {
            HeaderOutcome::None => "none",
            HeaderOutcome::Committed => "committed",
            HeaderOutcome::RolledBack => "rolled_back",
        }
    }

    fn from_word(word: &str) -> Result<Self, ParseRefusal> {
        match word {
            "none" => Ok(HeaderOutcome::None),
            "committed" => Ok(HeaderOutcome::Committed),
            "rolled_back" => Ok(HeaderOutcome::RolledBack),
            _ => Err(ParseRefusal::UnknownOutcome(word.to_owned())),
        }
    }
}

/// **The journal's frozen header, v1** — `{v, txn, rescue, class, outcome}`
/// (F-8; `outcome` by the coordinator's ruling of 2026-09-27).
///
/// `rescue` is the path of the rescue build (`H\<txn>\rescue\folio.exe`, or
/// the rescue clone's bundle on macOS), which is what a start that finds a
/// destructive class hands itself to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Header {
    pub(crate) txn: TxnId,
    pub(crate) rescue: String,
    pub(crate) class: Class,
    pub(crate) outcome: HeaderOutcome,
}

#[derive(Serialize, Deserialize)]
struct HeaderWire {
    v: u64,
    txn: TxnId,
    rescue: String,
    class: String,
    outcome: String,
    /// **The version of the build that wrote these bytes** (0.4.8 E1): every
    /// writer stamps its own [`crate::version::VERSION`]; absent from what
    /// 0.4.6 and 0.4.7 wrote. Attribution only: it is read only when the
    /// journal does not parse whole ([`sight`]), to word the card and the log
    /// line, and it never changes what a reader does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    written_by: Option<String>,
}

impl Header {
    /// Reads the header out of a journal's bytes, ignoring its body: a start
    /// reads these five fields and nothing else, whatever version wrote the
    /// rest.
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, ParseRefusal> {
        versioned(bytes, HEADER_VERSION)?;
        let wire: HeaderWire = json(bytes)?;
        Ok(Self {
            txn: wire.txn,
            rescue: wire.rescue,
            class: Class::from_word(&wire.class)?,
            outcome: HeaderOutcome::from_word(&wire.outcome)?,
        })
    }

    /// The header alone, as v1 bytes.
    pub(crate) fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(&self.wire()).expect("a header always serialises")
    }

    fn wire(&self) -> HeaderWire {
        HeaderWire {
            v: HEADER_VERSION,
            txn: self.txn,
            rescue: self.rescue.clone(),
            class: self.class.word().to_owned(),
            outcome: self.outcome.word().to_owned(),
            written_by: Some(crate::version::VERSION.to_owned()),
        }
    }
}

// ─────────────────────────────────── the receipt ───────────────────────────────────

/// **The trial's receipt, v1** — `{v, txn, nonce, pid, version}` and, since
/// 0.4.7, the optional `started` — written by N alone into
/// `H\<txn>\health-<nonce>` once it has claimed the data directory, read its
/// settings and session and drawn its first text (§C.5, F-1, F-14).
///
/// It is evidence, not a decision: only the lock holder turns it into
/// `Committed`, and only while the journal says `Trial` ([`next`]).
///
/// **`started`** (0.4.7 ticket U-37, design revision (h) H.1): the trial's own
/// start instant, read by the trial about itself, in the units the process
/// list reports (`bt_platform::install_flip::Running::started`). It binds the
/// receipt to the exact process that wrote it: a lock holder records a running
/// trial the journal does not know only when some process runs with exactly
/// this `(pid, started)`. The version stays 1: a reader of v1 ignores a field
/// it does not know (no `deny_unknown_fields`, 0.4.6's reader included), and
/// the ordinary commit of a recorded trial never reads it — the nonce the
/// journal recorded binds that one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Receipt {
    pub(crate) txn: TxnId,
    pub(crate) nonce: Nonce,
    pub(crate) pid: u32,
    pub(crate) version: String,
    pub(crate) started: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct ReceiptWire {
    v: u64,
    txn: String,
    nonce: String,
    pid: u32,
    version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    started: Option<u64>,
}

impl Receipt {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, ParseRefusal> {
        versioned(bytes, RECEIPT_VERSION)?;
        let wire: ReceiptWire = json(bytes)?;
        Ok(Self {
            txn: TxnId::parse(&wire.txn)?,
            nonce: Nonce::parse(&wire.nonce)?,
            pid: wire.pid,
            version: wire.version,
            started: wire.started,
        })
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(&ReceiptWire {
            v: RECEIPT_VERSION,
            txn: self.txn.to_string(),
            nonce: self.nonce.to_string(),
            pid: self.pid,
            version: self.version.clone(),
            started: self.started,
        })
        .expect("a receipt always serialises")
    }

    /// The receipt's file name inside `H\<txn>`, for the trial holding `nonce`.
    pub(crate) fn file_name(nonce: &Nonce) -> String {
        format!("{RECEIPT_FILE_PREFIX}{nonce}")
    }
}

// ─────────────────────────────────── inventories ───────────────────────────────────

/// One regular file of an install set: its name in the install folder, its
/// SHA-256 and its size.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Member {
    pub(crate) name: String,
    pub(crate) digest: Digest,
    pub(crate) size: u64,
}

/// **The Windows member inventories, recorded by O under the transaction lock
/// before `Prepared`** (F-8) and never recomputed by P or R.
///
/// * `old_shipped` — the names the running build shipped.
/// * `old_present` — what is actually at every name of `old_shipped` or of
///   `new` when O measured it. This is the rollback material: every one of
///   these moves to `backup\` before the new set moves in, and every one comes
///   back on a rollback. A present name that `old_shipped` does not list is a
///   collision ([`Inventories::collisions`]); it is carried like any other old
///   file, so a rollback puts it back byte for byte.
/// * `new` — the successor's members, from its manifest (F-4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Inventories {
    pub(crate) old_shipped: Vec<String>,
    pub(crate) old_present: Vec<Member>,
    pub(crate) new: Vec<Member>,
}

/// The folders of `H\<txn>` a Windows member moves between, and the install.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Place {
    /// The install folder itself.
    Install,
    /// `H\<txn>\backup\` — the old files, moved out of the install.
    Backup,
    /// `H\<txn>\set\` — the verified new files, before they move in.
    Set,
    /// `H\<txn>\rolledout\` — the new files, moved out again by a rollback.
    RolledOut,
}

/// One rename of one member from one place to another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Move {
    pub(crate) name: String,
    pub(crate) from: Place,
    pub(crate) to: Place,
}

impl Inventories {
    /// Present names the running build did not ship: pre-existing files at
    /// names the new set will occupy.
    pub(crate) fn collisions(&self) -> impl Iterator<Item = &Member> {
        self.old_present
            .iter()
            .filter(|member| !self.old_shipped.contains(&member.name))
    }

    /// **The flip, in its one order**: every old file out to `backup\`, then
    /// every new file in from `set\`. Old goes first because a rename never
    /// replaces its target; I1′ holds after every prefix of this list, which is
    /// what makes a death anywhere inside `Moving` recoverable ((b).1 F-7).
    pub(crate) fn forward_moves(&self) -> Vec<Move> {
        let out = self.old_present.iter().map(|member| Move {
            name: member.name.clone(),
            from: Place::Install,
            to: Place::Backup,
        });
        let r#in = self.new.iter().map(|member| Move {
            name: member.name.clone(),
            from: Place::Set,
            to: Place::Install,
        });
        out.chain(r#in).collect()
    }

    fn old(&self, name: &str) -> Option<&Member> {
        self.old_present.iter().find(|member| member.name == name)
    }

    fn new_member(&self, name: &str) -> Option<&Member> {
        self.new.iter().find(|member| member.name == name)
    }

    /// Every name either inventory mentions, old first, each once.
    fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for member in self.old_present.iter().chain(&self.new) {
            if !names.contains(&member.name.as_str()) {
                names.push(&member.name);
            }
        }
        names
    }
}

/// **A macOS bundle's identity** — its main executable's cdhash and its
/// `CFBundleShortVersionString` (F-3). Recovery decides which side of the swap
/// is live by reading this, never from the phase.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BundleIdentity {
    pub(crate) cdhash: Cdhash,
    pub(crate) version: String,
}

/// **What the transaction replaces**, which is also which of (b).2's two
/// tables governs it: a Windows member set, moved file by file, or a macOS
/// bundle, exchanged in one call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "layout")]
pub(crate) enum Layout {
    Members(Inventories),
    Bundle {
        old: BundleIdentity,
        new: BundleIdentity,
    },
    /// **A macOS bundle transaction before its successor is verified** — what
    /// O records at `Allocated` (F-17: "records each resource intent"): the
    /// running bundle's identity and the version the offer names. The new
    /// bundle's identity is known only once it is verified in `stage/`, so
    /// [`Journal::prepare_with`] replaces this with [`Layout::Bundle`] as it
    /// records `Prepared`; no later phase carries it.
    BundleIntent {
        old: BundleIdentity,
        to_version: String,
    },
}

// ───────────────────────────────────── phases ─────────────────────────────────────

/// The process N runs as, recorded so that R — which is not N's parent — can
/// tell N from a stranger that reused its pid (F-7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TrialProcess {
    pub(crate) pid: u32,
    /// The process's creation time as the platform reports it, opaque here.
    pub(crate) started: u64,
}

/// **A trial the lock holder started over a `Stuck` transaction whose new
/// bundle is live** (U-29b, the coordinator's ruling 3): the new build is never
/// started plainly before `Committed`, so it is started as a trial, and its
/// receipt — this nonce's — commits the transaction forward (W8/M8). The
/// process it runs as is the phase's `trial`, stopped first by any later
/// rollback attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Retrial {
    pub(crate) nonce: Nonce,
    /// Wall-clock milliseconds when the holder started it; the trial's
    /// deadline counts from it.
    pub(crate) began_ms: u64,
}

/// How a transaction that reached a decided outcome ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Outcome {
    Committed,
    RolledBack,
}

/// **The durable phase of a transaction** ((b).2's states).
///
/// `Moving` is (b).2's `Moving`/`Exchanging`: the Windows moves, or the one
/// macOS exchange — the two tables share every other state, and the journal's
/// [`Layout`] says which is meant. `Retired` is the note's "marks the class
/// `terminal`" made a state, so that the class is always a function of the
/// phase ([`PhaseKind::class`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase")]
pub(crate) enum Phase {
    Allocated,
    Prepared {
        deferred_launches: u8,
    },
    Handoff {
        applier: Nonce,
    },
    Armed,
    Moving,
    /// **A last-resort trial reserved before its process is launched**
    /// (0.4.7 U-35, Windows): an exit guard found the new set live at
    /// `Moving`, and the operating system refused both the new build's start
    /// and the rescue copy's. The nonce is durable before the same new image
    /// is asked once more with `--update-trial`, and it is the only trial this
    /// transaction may run from here ([`Phase::reserved_trial`]). That trial
    /// commits itself on its own exact receipt ([`Event::LastTrialReady`]); a
    /// recovery that can run adopts that receipt into [`Phase::Trial`], ends a
    /// handed-back instance that never became ready, or rolls back. No process
    /// identity exists yet, which is why this is a distinct phase.
    TrialStarting {
        nonce: Nonce,
        began_ms: u64,
    },
    Trial {
        nonce: Nonce,
        process: TrialProcess,
        /// Wall-clock milliseconds when P started N; the deadline counts from it.
        began_ms: u64,
    },
    Committed,
    RollbackIntent {
        trial: Option<TrialProcess>,
        /// **A trial of the new build was begun although no process is
        /// recorded** (U-35 round 2): the rollback was declared over
        /// `TrialStarting`, whose reserved trial was asked to start and may
        /// have run. [`Phase::RolledBack::untried`] is then false, so the card
        /// says the new version did not start — never that the update was
        /// interrupted before it did. Absent when false, so every journal
        /// written before it reads as before.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        trial_started: bool,
    },
    Stuck {
        trial: Option<TrialProcess>,
        /// [`Phase::RollbackIntent`]'s `trial_started`, carried across failed
        /// rollbacks to the `RolledBack` that follows.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        trial_started: bool,
        last_error: String,
        /// The rollbacks that failed, this one included: 1 at the first
        /// `Stuck`, one more at each failed retry ([`STUCK_ATTEMPT_LIMIT`]).
        attempts: u8,
        /// The trial started over this `Stuck` with the new bundle live, if
        /// one was ([`Event::RetrialBegan`]); a failed retry clears it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retrial: Option<Retrial>,
    },
    RolledBack {
        /// **No trial of the new build was ever begun** (0.4.7 U-42a; 0.4.6's
        /// D-7): the rollback came from `Moving` — a power cut or a failed
        /// move — or from a `Stuck` no trial ran over, so the new version
        /// never ran. The card a start sent after it says the update was
        /// interrupted, not that the new version did not start. Absent from
        /// the journal when false, so a journal without it reads as before.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        untried: bool,
    },
    Abandoned,
    Retired {
        outcome: Outcome,
        /// `RolledBack`'s [`Phase::RolledBack::untried`], kept at the
        /// retirement: the start sent with `--update-failed` reads it here.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        untried: bool,
    },
}

/// A [`Phase`] without its data, for tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PhaseKind {
    Allocated,
    Prepared,
    Handoff,
    Armed,
    Moving,
    TrialStarting,
    Trial,
    Committed,
    RollbackIntent,
    Stuck,
    RolledBack,
    Abandoned,
    Retired,
}

impl PhaseKind {
    pub(crate) const ALL: [PhaseKind; 13] = [
        PhaseKind::Allocated,
        PhaseKind::Prepared,
        PhaseKind::Handoff,
        PhaseKind::Armed,
        PhaseKind::Moving,
        PhaseKind::TrialStarting,
        PhaseKind::Trial,
        PhaseKind::Committed,
        PhaseKind::RollbackIntent,
        PhaseKind::Stuck,
        PhaseKind::RolledBack,
        PhaseKind::Abandoned,
        PhaseKind::Retired,
    ];

    /// **The header class of each phase.** `Handoff` is destructive because an
    /// accepted restart belongs to its applier: a start that continued past it
    /// would take the admission the applier needs, and the owner ruled
    /// (2026-09-25, 3) that a start during an apply waits. `Committed` and
    /// `RolledBack` are destructive until their lock holder has retired the
    /// entrance and the rollback material, because "every retirement of
    /// rollback material or of the entrance is preceded by a durable terminal
    /// state" is the lock holder's to keep. `Stuck` is destructive for ever:
    /// its journal, backup and entrance are the only road back.
    pub(crate) fn class(self) -> Class {
        match self {
            PhaseKind::Allocated => Class::Preparing,
            PhaseKind::Prepared => Class::Deferred,
            PhaseKind::Handoff
            | PhaseKind::Armed
            | PhaseKind::Moving
            | PhaseKind::TrialStarting
            | PhaseKind::Trial
            | PhaseKind::Committed
            | PhaseKind::RollbackIntent
            | PhaseKind::Stuck
            | PhaseKind::RolledBack => Class::Destructive,
            PhaseKind::Abandoned | PhaseKind::Retired => Class::Terminal,
        }
    }

    /// **The header outcome of each phase but `Retired`**, whose outcome is
    /// its own ([`Phase::outcome`]).
    fn outcome(self) -> HeaderOutcome {
        match self {
            PhaseKind::Committed => HeaderOutcome::Committed,
            PhaseKind::RollbackIntent | PhaseKind::Stuck | PhaseKind::RolledBack => {
                HeaderOutcome::RolledBack
            }
            PhaseKind::Allocated
            | PhaseKind::Prepared
            | PhaseKind::Handoff
            | PhaseKind::Armed
            | PhaseKind::Moving
            | PhaseKind::TrialStarting
            | PhaseKind::Trial
            | PhaseKind::Abandoned
            | PhaseKind::Retired => HeaderOutcome::None,
        }
    }
}

impl Phase {
    pub(crate) fn kind(&self) -> PhaseKind {
        match self {
            Phase::Allocated => PhaseKind::Allocated,
            Phase::Prepared { .. } => PhaseKind::Prepared,
            Phase::Handoff { .. } => PhaseKind::Handoff,
            Phase::Armed => PhaseKind::Armed,
            Phase::Moving => PhaseKind::Moving,
            Phase::TrialStarting { .. } => PhaseKind::TrialStarting,
            Phase::Trial { .. } => PhaseKind::Trial,
            Phase::Committed => PhaseKind::Committed,
            Phase::RollbackIntent { .. } => PhaseKind::RollbackIntent,
            Phase::Stuck { .. } => PhaseKind::Stuck,
            Phase::RolledBack { .. } => PhaseKind::RolledBack,
            Phase::Abandoned => PhaseKind::Abandoned,
            Phase::Retired { .. } => PhaseKind::Retired,
        }
    }

    pub(crate) fn class(&self) -> Class {
        self.kind().class()
    }

    /// **The one trial a `TrialStarting` transaction runs** (U-35): its
    /// reserved nonce, or `None` in every other phase. The single owner of
    /// what is started at `TrialStarting` — the exit guards name it
    /// (`update_apply_windows::opens_now`), a start admits only it
    /// (`update_startup::run`), and only it commits itself
    /// (`update_apply::commit_last_trial`).
    pub(crate) fn reserved_trial(&self) -> Option<Nonce> {
        match self {
            Phase::TrialStarting { nonce, .. } => Some(*nonce),
            _ => None,
        }
    }

    /// **The header outcome of this phase** — what the lock holder writes into
    /// the header with it.
    pub(crate) fn outcome(&self) -> HeaderOutcome {
        match self {
            Phase::Retired {
                outcome: Outcome::Committed,
                ..
            } => HeaderOutcome::Committed,
            Phase::Retired {
                outcome: Outcome::RolledBack,
                ..
            } => HeaderOutcome::RolledBack,
            phase => phase.kind().outcome(),
        }
    }
}

// ───────────────────────────────────── the journal ─────────────────────────────────────

/// **Whose road the transaction takes** — the adapter the body names
/// (`docs/plans/design/managed-update-2026-09-29.md` §1.1 R1, R2; 0.4.7
/// ticket U-41a1): chosen once, at the press, from how the copy was installed
/// (`update_adapter::of_channel`), and read from the journal — never from the
/// channel — by the applier, the recovery and every later lock holder, which
/// call its `Prepare`, `Activate` and `Prove / Recover` through it.
///
/// [`Adapter::Ours`] is the only one this build's roads take: the others are
/// named so that a journal can record them, and their roads are off
/// (`update_adapter::built_on`), so a managed copy keeps its row's command.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Adapter {
    /// Folio's own road: the member set on Windows, the bundle on macOS.
    #[default]
    Ours,
    Homebrew,
    Scoop,
    Winget,
}

impl Adapter {
    /// Whether this is [`Adapter::Ours`] — which the journal does not write,
    /// so that an ordinary copy's journal is the bytes 0.4.6 wrote.
    #[must_use]
    pub(crate) fn is_ours(&self) -> bool {
        *self == Adapter::Ours
    }
}

/// The journal's body, owned by the rescue build's version (F-8).
///
/// **`adapter`** (0.4.7 ticket U-41a1) follows the receipt's rule for a field
/// added to a v1 document (U-37, H.1; `Receipt::started`): absent when it is
/// [`Adapter::Ours`], so an ordinary copy's journal is written byte for byte
/// as 0.4.6 wrote it; a body without it (0.4.6's) reads as `Ours`; and a
/// reader ignores a field it does not know (no `deny_unknown_fields`, 0.4.6's
/// reader included).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Body {
    pub(crate) phase: Phase,
    pub(crate) layout: Layout,
    #[serde(default, skip_serializing_if = "Adapter::is_ours")]
    pub(crate) adapter: Adapter,
}

/// **`H\journal.json`**: the frozen header's `txn` and `rescue`, and the body.
/// The header's `class` is not stored twice — it is written from the phase and
/// checked against it when read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Journal {
    pub(crate) txn: TxnId,
    pub(crate) rescue: String,
    pub(crate) body: Body,
}

#[derive(Serialize)]
struct JournalWire<'a> {
    #[serde(flatten)]
    header: HeaderWire,
    body: &'a Body,
}

#[derive(Deserialize)]
struct BodyOnly {
    body: Body,
}

impl Journal {
    /// **A new transaction, as O creates it** under the transaction lock
    /// before it acquires anything (F-17): every resource it goes on to take
    /// is then discoverable from a journal that already exists.
    pub(crate) fn allocate(txn: TxnId, rescue: String, layout: Layout) -> Self {
        Self {
            txn,
            rescue,
            body: Body {
                phase: Phase::Allocated,
                layout,
                adapter: Adapter::Ours,
            },
        }
    }

    /// **The journal naming `adapter`** — what the press records at
    /// `Allocated` (managed-update R2); every later phase carries it.
    #[must_use]
    pub(crate) fn naming(mut self, adapter: Adapter) -> Self {
        self.body.adapter = adapter;
        self
    }

    pub(crate) fn header(&self) -> Header {
        Header {
            txn: self.txn,
            rescue: self.rescue.clone(),
            class: self.body.phase.class(),
            outcome: self.body.phase.outcome(),
        }
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(&JournalWire {
            header: self.header().wire(),
            body: &self.body,
        })
        .expect("a journal always serialises")
    }

    /// Reads a whole journal: the header first, by the same rules a start
    /// reads it with, then the body, which must be in the phase the header's
    /// class says.
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, ParseRefusal> {
        let header = Header::parse(bytes)?;
        let BodyOnly { body } = json(bytes)?;
        let phase = body.phase.kind();
        if phase.class() != header.class {
            return Err(ParseRefusal::ClassDisagreesWithPhase {
                class: header.class,
                phase,
            });
        }
        if body.phase.outcome() != header.outcome {
            return Err(ParseRefusal::OutcomeDisagreesWithPhase {
                outcome: header.outcome,
                phase,
            });
        }
        Ok(Self {
            txn: header.txn,
            rescue: header.rescue,
            body,
        })
    }

    /// **`Prepared`, with what was verified**: the journal after
    /// [`Event::Prepared`], its layout replaced by `layout` — the members or
    /// the bundle identities O measured under the transaction lock before this
    /// write (F-8), in place of the intent it recorded at `Allocated`.
    ///
    /// # Errors
    /// [`Event::Prepared`]'s refusal: only an `Allocated` journal is prepared.
    pub(crate) fn prepare_with(&self, layout: Layout) -> Result<Self, Refusal> {
        let mut prepared = self.advance(&Event::Prepared)?;
        prepared.body.layout = layout;
        Ok(prepared)
    }

    /// The journal after `event`, or why `event` cannot happen now.
    pub(crate) fn advance(&self, event: &Event) -> Result<Self, Refusal> {
        let phase = next(&self.txn, &self.body.phase, event)?;
        Ok(Self {
            body: Body {
                phase,
                layout: self.body.layout.clone(),
                adapter: self.body.adapter,
            },
            ..self.clone()
        })
    }
}

// ───────────────────────────── what a reader sees ─────────────────────────────

/// **What a reader sees in `H\journal.json`** (0.4.8 E1, the journal's escape
/// hatch): every production read of a journal another build may have written
/// goes through [`sight`], and every reader's answer to anything but
/// [`Sight::Known`] is its row of the one table, [`Role::beyond`].
///
/// **The rule.** A transaction this build cannot read whole is preserved
/// byte for byte, and only the rescue build its envelope names settles it.
/// The frozen header's class actions are the exceptions, taken on the
/// header's promise and not on the unknown body: a `terminal` class is
/// retired, a `preparing` or `deferred` one continued past or discarded when
/// the install was replaced by hand. A header this build cannot read is a
/// `destructive` transaction with nothing decided ([`Sight::acting_header`]).
///
/// **The envelope** — `txn`, `rescue` and `written_by` — keeps its names, its
/// types and its meaning in every header any later build writes, whatever
/// its `v`, `class` or `outcome` say. It is frozen for ever: it is how a
/// build that reads nothing else of a journal still finds the build that can.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Sight {
    /// Every word is one this build knows.
    Known(Journal),
    /// The frozen header reads and the body does not: a later build's phase,
    /// layout or adapter, or a body cut short.
    Header { header: Header, beyond: Beyond },
    /// The header does not read and its envelope does: a later header
    /// version, class or outcome, or a header damaged outside the envelope.
    Envelope { envelope: Envelope, beyond: Beyond },
    /// Not even the envelope reads: no transaction and no rescue build can be
    /// named.
    Unreadable(ParseRefusal),
}

/// **The header's envelope** — the three fields every header of every
/// version carries with the same names, types and meaning ([`Sight`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Envelope {
    pub(crate) txn: TxnId,
    pub(crate) rescue: String,
    pub(crate) written_by: Option<String>,
}

/// The envelope as it is read: whatever `v`, `class` and `outcome` say.
#[derive(Deserialize)]
struct EnvelopeWire {
    txn: TxnId,
    rescue: String,
    #[serde(default)]
    written_by: Option<String>,
}

/// **What lies beyond this build's grammar** — attribution only: it words
/// the card and the line, and no reader's action depends on it
/// ([`beyond_rule`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Beyond {
    /// The envelope's `written_by`, when it names a build later than this
    /// one. A later `written_by` says only that a later build last wrote the
    /// bytes, not that the rest of them is sound.
    pub(crate) newer: Option<String>,
    /// Why the journal did not read whole.
    pub(crate) refusal: ParseRefusal,
}

/// **The one reading of a journal's bytes** ([`Sight`]). Pure: the caller
/// read the bytes.
pub(crate) fn sight(bytes: &[u8]) -> Sight {
    sight_as(bytes, crate::version::VERSION)
}

/// **What a reader sees from one read of `H\journal.json`** (E1 round 2):
/// `None` only when there is no such file; a read that failed any other way
/// is [`Sight::Unreadable`] with the operating system's error as its refusal
/// ([`ParseRefusal::Unread`]) — a journal that could not be read is never
/// taken for no journal, and each reader takes its role's answer to it.
/// Pure: the caller made the read.
pub(crate) fn sight_of_read(read: std::io::Result<Vec<u8>>) -> Option<Sight> {
    match read {
        Ok(bytes) => Some(sight(&bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => Some(Sight::Unreadable(ParseRefusal::Unread(error.to_string()))),
    }
}

/// [`sight`] as the build of version `this_build` reads it.
fn sight_as(bytes: &[u8], this_build: &str) -> Sight {
    let refusal = match Journal::parse(bytes) {
        Ok(journal) => return Sight::Known(journal),
        Err(refusal) => refusal,
    };
    let envelope: Option<EnvelopeWire> = json(bytes).ok();
    let newer = envelope
        .as_ref()
        .and_then(|envelope| envelope.written_by.as_deref())
        .filter(|written_by| later_than(written_by, this_build))
        .map(str::to_owned);
    match Header::parse(bytes) {
        Ok(header) => Sight::Header {
            header,
            beyond: Beyond { newer, refusal },
        },
        Err(refusal) => match envelope {
            Some(EnvelopeWire {
                txn,
                rescue,
                written_by,
            }) => Sight::Envelope {
                envelope: Envelope {
                    txn,
                    rescue,
                    written_by,
                },
                beyond: Beyond { newer, refusal },
            },
            None => Sight::Unreadable(refusal),
        },
    }
}

/// Whether the version `written_by` is later than `this_build`; a version
/// either side cannot order is not.
fn later_than(written_by: &str, this_build: &str) -> bool {
    match (
        crate::update::Version::parse(written_by),
        crate::update::Version::parse(this_build),
    ) {
        (Some(written_by), Some(this_build)) => written_by > this_build,
        _ => false,
    }
}

impl Sight {
    /// **The header a reader acts on**: the journal's own, the header read
    /// alone, or for an envelope a `destructive` transaction with nothing
    /// decided — which hands every start to the envelope's rescue build and
    /// opens what an unknown live set opens. `None` when nothing reads.
    pub(crate) fn acting_header(&self) -> Option<Header> {
        match self {
            Sight::Known(journal) => Some(journal.header()),
            Sight::Header { header, .. } => Some(header.clone()),
            Sight::Envelope { envelope, .. } => Some(Header {
                txn: envelope.txn,
                rescue: envelope.rescue.clone(),
                class: Class::Destructive,
                outcome: HeaderOutcome::None,
            }),
            Sight::Unreadable(_) => None,
        }
    }

    /// The later build that wrote what this one cannot read, if one is named.
    pub(crate) fn newer(&self) -> Option<&str> {
        match self {
            Sight::Header { beyond, .. } | Sight::Envelope { beyond, .. } => {
                beyond.newer.as_deref()
            }
            Sight::Known(_) | Sight::Unreadable(_) => None,
        }
    }

    /// **The line a reader says** of what it saw and of what `role` does
    /// about it, from the table ([`beyond_rule`]).
    pub(crate) fn said(&self, role: Role) -> String {
        let seen = match self {
            Sight::Known(journal) => format!("transaction {} is read whole", journal.txn),
            Sight::Header { header, beyond } => format!(
                "transaction {} is {:?} and its body {}",
                header.txn,
                header.class,
                beyond.account()
            ),
            Sight::Envelope { envelope, beyond } => {
                format!("transaction {}'s header {}", envelope.txn, beyond.account())
            }
            Sight::Unreadable(refusal) => format!("the journal cannot be read ({refusal})"),
        };
        format!("{seen}; {}", beyond_rule(role, self.newer()).phrase())
    }
}

impl Beyond {
    fn account(&self) -> String {
        match &self.newer {
            Some(newer) => format!("was written by Folio {newer} ({})", self.refusal),
            None => format!("cannot be read ({})", self.refusal),
        }
    }
}

/// **What the trial's receipt reads as** — the receipt readers' one reading
/// ([`Role::ReceiptWrite`], [`Role::WindowsReceiptWatch`],
/// [`Role::MacReceiptWatch`]): a receipt this build cannot read is never
/// accepted and never written over ([`BeyondAction::NeverAccept`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ReceiptSight {
    Known(Receipt),
    Unknown(ParseRefusal),
}

/// **The one reading of a receipt's bytes.**
pub(crate) fn receipt_sight(bytes: &[u8]) -> ReceiptSight {
    match Receipt::parse(bytes) {
        Ok(receipt) => ReceiptSight::Known(receipt),
        Err(refusal) => ReceiptSight::Unknown(refusal),
    }
}

impl ReceiptSight {
    /// The receipt, or why it is not one: `role`'s row is never to accept
    /// it.
    pub(crate) fn known(self, role: Role) -> Result<Receipt, String> {
        match self {
            ReceiptSight::Known(receipt) => Ok(receipt),
            ReceiptSight::Unknown(refusal) => match beyond_rule(role, None) {
                BeyondAction::NeverAccept => Err(refusal.to_string()),
                other => Err(format!("{refusal}; {}", other.phrase())),
            },
        }
    }
}

/// **Who reads a journal or a receipt another build may have written**: the
/// sixteen reader and decision roles of the escape hatch (`docs/DESIGN.md`,
/// 2026-10-08), each with its row of [`Role::beyond`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Role {
    /// The ordinary start (`update_startup::run`) and its decisions.
    Start,
    /// The trial's watch ([`trial_sight`]).
    TrialWatch,
    /// The trial's watchdog (`update_trial::hand_back`).
    TrialHandBack,
    /// The trial's receipt writer (`update_trial::write_receipt`).
    ReceiptWrite,
    /// The Windows lock holder's receipt watch (`update_apply::read_receipt`).
    WindowsReceiptWatch,
    /// U-35's reservation (`update_apply::reserve_last_trial`).
    LastTrialReserve,
    /// U-35's self-commit (`update_apply::commit_last_trial_as`).
    LastTrialCommit,
    /// The applier's window election (`update_apply::read_window_phase`).
    WindowElection,
    /// The Windows exit guard (`update_apply_windows::opens_now`).
    WindowsExit,
    /// The Windows lock holder: the applier and the recovery
    /// (`update_apply_windows`).
    WindowsHolder,
    /// The macOS exit guard (`update_apply_macos::opens_now`).
    MacExit,
    /// The macOS lock holder (`update_apply_macos`, its `Txn::hold`).
    MacHolder,
    /// The macOS lock holder's receipt watch.
    MacReceiptWatch,
    /// The recovery door (`update_recover::run` and its leave).
    RecoveryDoor,
    /// The outgoing build's exit (`update_handoff`, its `OldLeave`).
    OutgoingExit,
    /// The job owner at a launch (`update_prepare::at_launch`) and the press
    /// (Prepare).
    JobOwner,
}

impl Role {
    /// Every role.
    #[cfg(test)]
    pub(crate) const ALL: [Role; 16] = [
        Role::Start,
        Role::TrialWatch,
        Role::TrialHandBack,
        Role::ReceiptWrite,
        Role::WindowsReceiptWatch,
        Role::LastTrialReserve,
        Role::LastTrialCommit,
        Role::WindowElection,
        Role::WindowsExit,
        Role::WindowsHolder,
        Role::MacExit,
        Role::MacHolder,
        Role::MacReceiptWatch,
        Role::RecoveryDoor,
        Role::OutgoingExit,
        Role::JobOwner,
    ];

    /// **The one reading of a journal's bytes, made by this role** —
    /// [`sight`], which reads the same for every role. The role is written
    /// where the product reads, so that the call-site registry of the
    /// journal's reads (this module's tests, `parse_sites`) can hold each
    /// read to the row of [`Role::beyond`] it answers by.
    pub(crate) fn sight(self, bytes: &[u8]) -> Sight {
        sight(bytes)
    }

    /// **The one reading of one read of `H\journal.json`, made by this
    /// role** — [`sight_of_read`], written where the product reads as
    /// [`Role::sight`] is.
    pub(crate) fn sight_of_read(self, read: std::io::Result<Vec<u8>>) -> Option<Sight> {
        sight_of_read(read)
    }

    /// **The table: what each role does with anything but what it reads
    /// whole.**
    pub(crate) const fn beyond(self) -> BeyondAction {
        match self {
            Role::Start | Role::TrialWatch | Role::TrialHandBack | Role::OutgoingExit => {
                BeyondAction::ActOnHeader
            }
            Role::ReceiptWrite | Role::WindowsReceiptWatch | Role::MacReceiptWatch => {
                BeyondAction::NeverAccept
            }
            Role::LastTrialReserve
            | Role::LastTrialCommit
            | Role::WindowElection
            | Role::WindowsHolder
            | Role::MacHolder => BeyondAction::StandAside,
            Role::WindowsExit | Role::MacExit | Role::RecoveryDoor => BeyondAction::UnknownLiveSet,
            Role::JobOwner => BeyondAction::LeaveToItsRescue,
        }
    }
}

/// **What a reader does with a journal or a receipt it cannot read whole.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BeyondAction {
    /// Act on [`Sight::acting_header`] by the role's own rules: a header
    /// that does not read is a `destructive` transaction with nothing
    /// decided; a journal of which nothing reads is left exactly as it is,
    /// and a start says so on its card.
    ActOnHeader,
    /// Never accept it and never write over it.
    NeverAccept,
    /// Record nothing, remove nothing, end nothing, and let the lock go.
    StandAside,
    /// Answer what a `destructive` transaction whose live set is unknown
    /// opens: the rescue copy on Windows, a held-writes trial on macOS —
    /// never the installed build plainly.
    UnknownLiveSet,
    /// Leave it to the build that wrote it: the offer still shows, and the
    /// press answers that another build's update is not finished.
    LeaveToItsRescue,
}

impl BeyondAction {
    fn phrase(self) -> &'static str {
        match self {
            BeyondAction::ActOnHeader => "this build acts on its header alone",
            BeyondAction::NeverAccept => "it is never accepted or written over",
            BeyondAction::StandAside => {
                "this holder stands aside: nothing is recorded, removed or ended"
            }
            BeyondAction::UnknownLiveSet => "which set is live is not known",
            BeyondAction::LeaveToItsRescue => "it is left to the build that wrote it",
        }
    }
}

/// **The action `role` takes on what it cannot read whole** — its row of
/// [`Role::beyond`], whatever later build `newer` names: the attribution
/// words the card and the line and never changes an action.
pub(crate) fn beyond_rule(role: Role, newer: Option<&str>) -> BeyondAction {
    let _attribution_only = newer;
    role.beyond()
}

/// **The later build every role test names** as the writer of what it cannot
/// read ([`beyond_inputs`]).
#[cfg(test)]
pub(crate) const LATER_BUILD: &str = "99.0.0";

/// **The three journals a reader role is fed** (E1), from the bytes of a
/// journal it reads whole, `known`: (i) an unknown header word — the class
/// `paused` — written by [`LATER_BUILD`], so only the envelope reads; (ii) a
/// known header over an unknown body word — the phase `FuturePhase`; (iii)
/// bytes of which nothing reads.
#[cfg(test)]
pub(crate) fn beyond_inputs(known: &[u8]) -> [(&'static str, Vec<u8>); 3] {
    let mut header_word: serde_json::Value =
        serde_json::from_slice(known).expect("a journal is JSON");
    header_word["class"] = serde_json::Value::from("paused");
    header_word["written_by"] = serde_json::Value::from(LATER_BUILD);
    let mut body_word: serde_json::Value =
        serde_json::from_slice(known).expect("a journal is JSON");
    body_word["body"]["phase"] = serde_json::Value::from("FuturePhase");
    [
        (
            "an unknown header word, by a later build",
            serde_json::to_vec(&header_word).expect("bytes"),
        ),
        (
            "an unknown body word",
            serde_json::to_vec(&body_word).expect("bytes"),
        ),
        ("bytes that are no journal", br#"{"x":1}"#.to_vec()),
    ]
}

/// **A journal whose file cannot be read at all** (E1 round 2): `journal`
/// replaced by a folder of that name, which every platform refuses to read
/// as a file with an error other than "no such file". Answers that error's
/// kind, for the test to show it is no absence.
#[cfg(test)]
pub(crate) fn a_journal_that_cannot_be_read(journal: &Path) -> std::io::ErrorKind {
    let _ = std::fs::remove_file(journal);
    std::fs::create_dir_all(journal).expect("a folder at the journal's name");
    let kind = std::fs::read(journal)
        .expect_err("a folder is no file")
        .kind();
    assert_ne!(kind, std::io::ErrorKind::NotFound);
    kind
}

/// **The three receipts a receipt reader is fed** (E1), from `known`, a
/// receipt it reads: (i) the same receipt as a later version, `v: 2`; (ii) a
/// v1 receipt whose `pid` is a word this build does not read; (iii) bytes
/// that are no receipt.
#[cfg(test)]
pub(crate) fn receipt_beyond_inputs(known: &Receipt) -> [(&'static str, Vec<u8>); 3] {
    let mut later: serde_json::Value =
        serde_json::from_slice(&known.encode()).expect("a receipt is JSON");
    later["v"] = serde_json::Value::from(2);
    let mut word: serde_json::Value =
        serde_json::from_slice(&known.encode()).expect("a receipt is JSON");
    word["pid"] = serde_json::Value::from("the trial");
    [
        (
            "a later receipt version",
            serde_json::to_vec(&later).expect("bytes"),
        ),
        (
            "an unknown receipt word",
            serde_json::to_vec(&word).expect("bytes"),
        ),
        ("bytes that are no receipt", br#"{"x":1}"#.to_vec()),
    ]
}

// ─────────────────────────────────── transitions ───────────────────────────────────

/// **Something that happened, which a writer records as the next phase.**
///
/// Not `Clone`: [`Event::Armed`] carries the entrance's proof, which only the
/// entrance door makes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Event {
    /// O verified and staged the successor (§C.2's last step).
    Prepared,
    /// O's preparation failed: nothing installed was touched.
    PrepareFailed,
    /// The job owner started while `Prepared` and has not resumed.
    LaunchedWithoutResume,
    /// The job owner discards the prepared successor: the reader's choice, a
    /// failed revalidation, or an install replaced by hand.
    Discarded,
    /// The quit landed and O hands the transaction to its applier (F-6).
    HandedOff { applier: Nonce },
    /// **O could not start the applier it had just handed the transaction
    /// to** (U-21, §C.3: "a spawn failure there journals `Failed` and exits
    /// anyway"). Nothing has moved: the transaction is abandoned — terminal,
    /// outcome `none` — and the next start retires it (W13).
    ApplierNotStarted,
    /// **The old build did not let go within the applier's wait** (U-28,
    /// §C.4: "If 60 s pass, P journals `Failed`, deletes staging, and exits
    /// without touching anything"): the applier held the transaction lock, but
    /// the data directory's claim was still held when its wait ran out.
    /// Nothing has moved: the transaction is abandoned — terminal, outcome
    /// `none` — and cleared.
    OldStayed,
    /// **The staged set is no longer what was verified** (U-23, (b).1 F-17):
    /// the Windows applier, holding the lock after the old build let go and
    /// before it writes the entrance, found an old file, a staged member or
    /// the rescue copy changed since `Prepared`
    /// (`update_prepare_windows::staged_as_verified`). Nothing has moved: the
    /// transaction is abandoned — terminal, outcome `none` — and the next
    /// ordinary start retires it (W13).
    Unverified,
    /// The entrance is written, flushed and read back (F-2, F-3). The proof is
    /// `bt_platform::install_txn::Armed`, one type for both platforms, which
    /// only the two entrance doors make, and only after their read-back
    /// matched: `logon_hook::arm` (the Windows `Run` value, U-22) and
    /// `launch_agent::arm` (the macOS LaunchAgent plist, U-26) — so `Armed`
    /// cannot be recorded before the entrance is on disk. It must name this
    /// journal's transaction ([`Refusal::EntranceForAnotherTransaction`]).
    Armed(bt_platform::install_txn::Armed),
    /// The entrance could not be made durable, or its command is too long.
    EntranceFailed,
    /// The restart is put back to `Prepared`: admission refused (W3, W5), an
    /// entrance found from a dead attempt (W4), or a macOS swap not performed
    /// (M5).
    Reverted,
    /// Exclusive admission taken and no process runs from the install.
    Admitted,
    /// **The exit guard reserved its last trial before launching it** (U-35).
    TrialPlanned { nonce: Nonce, began_ms: u64 },
    /// The reserved or directly launched trial is running with its exact
    /// process identity.
    TrialBegan {
        nonce: Nonce,
        process: TrialProcess,
        began_ms: u64,
    },
    /// **U-35's reserved last trial proved itself ready and commits its own
    /// transaction** — recorded by that trial ([`Actor::Trial`]) under the
    /// transaction lock, because the holders that would otherwise adopt it
    /// all run the rescue copy the operating system refused. The receipt is
    /// the ordinary U-37 readiness evidence, read back from disk; `process` is
    /// the recording process's own pid and start instant, which the receipt
    /// must name exactly. Only from [`Phase::TrialStarting`].
    LastTrialReady {
        receipt: Receipt,
        process: TrialProcess,
    },
    /// **The lock holder started the new build as a trial over a `Stuck`
    /// transaction whose new bundle is live** (U-29b, ruling 3): the nonce it
    /// gave, the process the list found, and when it began.
    RetrialBegan {
        nonce: Nonce,
        process: TrialProcess,
        began_ms: u64,
    },
    /// The lock holder holds a receipt.
    ReceiptAccepted(Receipt),
    /// The trial failed, timed out, or never began.
    RollbackDeclared,
    /// Every old member is back, verified by digest (or identity).
    RolledBack,
    /// A rollback step failed.
    RollbackFailed { error: String },
    /// The lock holder retired the entrance and the rollback material.
    Retired,
}

/// An [`Event`] without its data, for tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum EventKind {
    Prepared,
    PrepareFailed,
    LaunchedWithoutResume,
    Discarded,
    HandedOff,
    ApplierNotStarted,
    OldStayed,
    Unverified,
    Armed,
    EntranceFailed,
    Reverted,
    Admitted,
    TrialPlanned,
    TrialBegan,
    LastTrialReady,
    RetrialBegan,
    ReceiptAccepted,
    RollbackDeclared,
    RolledBack,
    RollbackFailed,
    Retired,
}

impl EventKind {
    pub(crate) const ALL: [EventKind; 21] = [
        EventKind::Prepared,
        EventKind::PrepareFailed,
        EventKind::LaunchedWithoutResume,
        EventKind::Discarded,
        EventKind::HandedOff,
        EventKind::ApplierNotStarted,
        EventKind::OldStayed,
        EventKind::Unverified,
        EventKind::Armed,
        EventKind::EntranceFailed,
        EventKind::Reverted,
        EventKind::Admitted,
        EventKind::TrialPlanned,
        EventKind::TrialBegan,
        EventKind::LastTrialReady,
        EventKind::RetrialBegan,
        EventKind::ReceiptAccepted,
        EventKind::RollbackDeclared,
        EventKind::RolledBack,
        EventKind::RollbackFailed,
        EventKind::Retired,
    ];

    /// **Who records this event** ((b).2, "Who may write what").
    pub(crate) fn authors(self) -> &'static [Actor] {
        match self {
            EventKind::Prepared
            | EventKind::PrepareFailed
            | EventKind::LaunchedWithoutResume
            | EventKind::Discarded
            | EventKind::HandedOff
            | EventKind::ApplierNotStarted => &[Actor::Old],
            EventKind::OldStayed
            | EventKind::Unverified
            | EventKind::Armed
            | EventKind::EntranceFailed
            | EventKind::Admitted => &[Actor::Applier],
            // R starts N too (U-29b): an exchange a dead applier left with the
            // new bundle live is decided by a trial, and a `Stuck` one with
            // the new bundle live is started only as one.
            EventKind::TrialPlanned
            | EventKind::TrialBegan
            | EventKind::RetrialBegan
            | EventKind::Reverted
            | EventKind::ReceiptAccepted
            | EventKind::RollbackDeclared
            | EventKind::RolledBack
            | EventKind::RollbackFailed
            | EventKind::Retired => &[Actor::Applier, Actor::Recovery],
            EventKind::LastTrialReady => &[Actor::Trial],
        }
    }
}

impl Event {
    pub(crate) fn kind(&self) -> EventKind {
        match self {
            Event::Prepared => EventKind::Prepared,
            Event::PrepareFailed => EventKind::PrepareFailed,
            Event::LaunchedWithoutResume => EventKind::LaunchedWithoutResume,
            Event::Discarded => EventKind::Discarded,
            Event::HandedOff { .. } => EventKind::HandedOff,
            Event::ApplierNotStarted => EventKind::ApplierNotStarted,
            Event::OldStayed => EventKind::OldStayed,
            Event::Unverified => EventKind::Unverified,
            Event::Armed(_) => EventKind::Armed,
            Event::EntranceFailed => EventKind::EntranceFailed,
            Event::Reverted => EventKind::Reverted,
            Event::Admitted => EventKind::Admitted,
            Event::TrialPlanned { .. } => EventKind::TrialPlanned,
            Event::TrialBegan { .. } => EventKind::TrialBegan,
            Event::LastTrialReady { .. } => EventKind::LastTrialReady,
            Event::RetrialBegan { .. } => EventKind::RetrialBegan,
            Event::ReceiptAccepted(_) => EventKind::ReceiptAccepted,
            Event::RollbackDeclared => EventKind::RollbackDeclared,
            Event::RolledBack => EventKind::RolledBack,
            Event::RollbackFailed { .. } => EventKind::RollbackFailed,
            Event::Retired => EventKind::Retired,
        }
    }
}

/// Why an event cannot be recorded in the phase it arrived in. A refusal is an
/// answer, never a panic: the writer records nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The event has no transition from this phase.
    Illegal { from: PhaseKind, event: EventKind },
    /// A receipt naming another transaction.
    ReceiptForAnotherTransaction,
    /// A receipt whose nonce is not this trial's.
    ReceiptForAnotherTrial,
    /// A last trial's receipt does not name that exact process.
    ReceiptForAnotherProcess,
    /// An entrance's proof made for another transaction (U-22).
    EntranceForAnotherTransaction,
    /// **A receipt that arrives once `RollbackIntent` is durable is ignored, by
    /// rule** (F-1, F-14): the rollback has been decided and a late health
    /// report does not overturn it.
    ReceiptAfterRollbackIntent,
}

/// **Every transition**, as `(from, event, to)`. [`next`] realises exactly
/// these; any other pair is [`Refusal::Illegal`] or one of [`NAMED_REFUSALS`].
pub(crate) const TRANSITIONS: &[(PhaseKind, EventKind, PhaseKind)] = &[
    (
        PhaseKind::Allocated,
        EventKind::Prepared,
        PhaseKind::Prepared,
    ),
    (
        PhaseKind::Allocated,
        EventKind::PrepareFailed,
        PhaseKind::Abandoned,
    ),
    (
        PhaseKind::Prepared,
        EventKind::LaunchedWithoutResume,
        PhaseKind::Prepared,
    ),
    (
        PhaseKind::Prepared,
        EventKind::LaunchedWithoutResume,
        PhaseKind::Abandoned,
    ),
    (
        PhaseKind::Prepared,
        EventKind::Discarded,
        PhaseKind::Abandoned,
    ),
    (
        PhaseKind::Prepared,
        EventKind::HandedOff,
        PhaseKind::Handoff,
    ),
    (
        PhaseKind::Handoff,
        EventKind::ApplierNotStarted,
        PhaseKind::Abandoned,
    ),
    (
        PhaseKind::Handoff,
        EventKind::OldStayed,
        PhaseKind::Abandoned,
    ),
    (
        PhaseKind::Handoff,
        EventKind::Unverified,
        PhaseKind::Abandoned,
    ),
    (PhaseKind::Handoff, EventKind::Armed, PhaseKind::Armed),
    (
        PhaseKind::Handoff,
        EventKind::EntranceFailed,
        PhaseKind::Abandoned,
    ),
    (PhaseKind::Handoff, EventKind::Reverted, PhaseKind::Prepared),
    (PhaseKind::Armed, EventKind::Reverted, PhaseKind::Prepared),
    (PhaseKind::Armed, EventKind::Admitted, PhaseKind::Moving),
    (PhaseKind::Moving, EventKind::Reverted, PhaseKind::Prepared),
    (
        PhaseKind::Moving,
        EventKind::TrialPlanned,
        PhaseKind::TrialStarting,
    ),
    (PhaseKind::Moving, EventKind::TrialBegan, PhaseKind::Trial),
    (
        PhaseKind::TrialStarting,
        EventKind::TrialBegan,
        PhaseKind::Trial,
    ),
    (
        PhaseKind::TrialStarting,
        EventKind::LastTrialReady,
        PhaseKind::Committed,
    ),
    (
        PhaseKind::TrialStarting,
        EventKind::RollbackDeclared,
        PhaseKind::RollbackIntent,
    ),
    (
        PhaseKind::Moving,
        EventKind::RollbackDeclared,
        PhaseKind::RollbackIntent,
    ),
    (
        PhaseKind::Trial,
        EventKind::ReceiptAccepted,
        PhaseKind::Committed,
    ),
    (
        PhaseKind::Stuck,
        EventKind::ReceiptAccepted,
        PhaseKind::Committed,
    ),
    (PhaseKind::Stuck, EventKind::RetrialBegan, PhaseKind::Stuck),
    (
        PhaseKind::Trial,
        EventKind::RollbackDeclared,
        PhaseKind::RollbackIntent,
    ),
    (
        PhaseKind::RollbackIntent,
        EventKind::RolledBack,
        PhaseKind::RolledBack,
    ),
    (
        PhaseKind::RollbackIntent,
        EventKind::RollbackFailed,
        PhaseKind::Stuck,
    ),
    (
        PhaseKind::Stuck,
        EventKind::RolledBack,
        PhaseKind::RolledBack,
    ),
    (
        PhaseKind::Stuck,
        EventKind::RollbackFailed,
        PhaseKind::Stuck,
    ),
    (PhaseKind::Committed, EventKind::Retired, PhaseKind::Retired),
    (
        PhaseKind::RolledBack,
        EventKind::Retired,
        PhaseKind::Retired,
    ),
];

/// The refusals that are not merely [`Refusal::Illegal`], by `(from, event)`.
/// A mismatched receipt in `Trial` or in a `Stuck` with a retrial, and any
/// receipt in a `Stuck` without one, are refused by value and are not listed.
pub(crate) const NAMED_REFUSALS: &[(PhaseKind, EventKind, Refusal)] = &[(
    PhaseKind::RollbackIntent,
    EventKind::ReceiptAccepted,
    Refusal::ReceiptAfterRollbackIntent,
)];

/// **The phase after `event`**, total over every `(phase, event)`: a pair
/// [`TRANSITIONS`] does not list is a refusal, never a panic. `txn` is the
/// journal's own, which a receipt must name.
pub(crate) fn next(txn: &TxnId, phase: &Phase, event: &Event) -> Result<Phase, Refusal> {
    match (phase, event) {
        (Phase::Allocated, Event::Prepared) => Ok(Phase::Prepared {
            deferred_launches: 0,
        }),
        (Phase::Allocated, Event::PrepareFailed) => Ok(Phase::Abandoned),
        (Phase::Prepared { deferred_launches }, Event::LaunchedWithoutResume) => {
            let launches = deferred_launches.saturating_add(1);
            Ok(if launches >= DEFERRED_LAUNCH_LIMIT {
                Phase::Abandoned
            } else {
                Phase::Prepared {
                    deferred_launches: launches,
                }
            })
        }
        (Phase::Prepared { .. }, Event::Discarded) => Ok(Phase::Abandoned),
        (Phase::Prepared { .. }, Event::HandedOff { applier }) => {
            Ok(Phase::Handoff { applier: *applier })
        }
        (Phase::Handoff { .. }, Event::Armed(proof)) => {
            if proof.transaction() == txn.bytes() {
                Ok(Phase::Armed)
            } else {
                Err(Refusal::EntranceForAnotherTransaction)
            }
        }
        (
            Phase::Handoff { .. },
            Event::EntranceFailed | Event::ApplierNotStarted | Event::OldStayed | Event::Unverified,
        ) => Ok(Phase::Abandoned),
        (Phase::Handoff { .. } | Phase::Armed | Phase::Moving, Event::Reverted) => {
            Ok(Phase::Prepared {
                deferred_launches: 0,
            })
        }
        (Phase::Armed, Event::Admitted) => Ok(Phase::Moving),
        (Phase::Moving, Event::TrialPlanned { nonce, began_ms }) => Ok(Phase::TrialStarting {
            nonce: *nonce,
            began_ms: *began_ms,
        }),
        (
            Phase::Moving | Phase::TrialStarting { .. },
            Event::TrialBegan {
                nonce,
                process,
                began_ms,
            },
        ) => {
            if let Phase::TrialStarting { nonce: planned, .. } = phase
                && planned != nonce
            {
                return Err(Refusal::ReceiptForAnotherTrial);
            }
            Ok(Phase::Trial {
                nonce: *nonce,
                process: *process,
                began_ms: *began_ms,
            })
        }
        // U-35's reserved trial commits itself on the same evidence a holder
        // adopts a handed-back trial by (U-37, H.3): a receipt of this
        // transaction, at the reserved nonce, naming this very process by
        // pid and start instant.
        (Phase::TrialStarting { .. }, Event::LastTrialReady { receipt, .. })
            if receipt.txn != *txn =>
        {
            Err(Refusal::ReceiptForAnotherTransaction)
        }
        (Phase::TrialStarting { nonce, .. }, Event::LastTrialReady { receipt, .. })
            if receipt.nonce != *nonce =>
        {
            Err(Refusal::ReceiptForAnotherTrial)
        }
        (Phase::TrialStarting { .. }, Event::LastTrialReady { receipt, process })
            if receipt.pid != process.pid || receipt.started != Some(process.started) =>
        {
            Err(Refusal::ReceiptForAnotherProcess)
        }
        (Phase::TrialStarting { .. }, Event::LastTrialReady { .. }) => Ok(Phase::Committed),
        (
            Phase::Trial { .. }
            | Phase::Stuck {
                retrial: Some(_), ..
            },
            Event::ReceiptAccepted(receipt),
        ) if receipt.txn != *txn => Err(Refusal::ReceiptForAnotherTransaction),
        (
            Phase::Trial { nonce, .. }
            | Phase::Stuck {
                retrial: Some(Retrial { nonce, .. }),
                ..
            },
            Event::ReceiptAccepted(receipt),
        ) if receipt.nonce != *nonce => Err(Refusal::ReceiptForAnotherTrial),
        // A `Stuck` whose new bundle is live recovers forward on the receipt
        // of the trial its holder started over it (U-29b, ruling 3); the
        // trial the rollback was declared on is never heard again (F-14).
        (
            Phase::Trial { .. }
            | Phase::Stuck {
                retrial: Some(_), ..
            },
            Event::ReceiptAccepted(_),
        ) => Ok(Phase::Committed),
        (Phase::RollbackIntent { .. } | Phase::Stuck { .. }, Event::ReceiptAccepted(_)) => {
            Err(Refusal::ReceiptAfterRollbackIntent)
        }
        (
            Phase::Stuck {
                trial_started,
                last_error,
                attempts,
                ..
            },
            Event::RetrialBegan {
                nonce,
                process,
                began_ms,
            },
        ) => Ok(Phase::Stuck {
            trial: Some(*process),
            trial_started: *trial_started,
            last_error: last_error.clone(),
            attempts: *attempts,
            retrial: Some(Retrial {
                nonce: *nonce,
                began_ms: *began_ms,
            }),
        }),
        (Phase::Moving, Event::RollbackDeclared) => Ok(Phase::RollbackIntent {
            trial: None,
            trial_started: false,
        }),
        (Phase::TrialStarting { .. }, Event::RollbackDeclared) => Ok(Phase::RollbackIntent {
            trial: None,
            trial_started: true,
        }),
        (Phase::Trial { process, .. }, Event::RollbackDeclared) => Ok(Phase::RollbackIntent {
            trial: Some(*process),
            trial_started: false,
        }),
        (
            Phase::RollbackIntent {
                trial,
                trial_started,
            }
            | Phase::Stuck {
                trial,
                trial_started,
                ..
            },
            Event::RolledBack,
        ) => Ok(Phase::RolledBack {
            untried: trial.is_none() && !trial_started,
        }),
        (
            Phase::RollbackIntent {
                trial,
                trial_started,
            },
            Event::RollbackFailed { error },
        ) => Ok(Phase::Stuck {
            trial: *trial,
            trial_started: *trial_started,
            last_error: error.clone(),
            attempts: 1,
            retrial: None,
        }),
        (
            Phase::Stuck {
                trial,
                trial_started,
                attempts,
                ..
            },
            Event::RollbackFailed { error },
        ) => Ok(Phase::Stuck {
            trial: *trial,
            trial_started: *trial_started,
            last_error: error.clone(),
            attempts: attempts.saturating_add(1),
            retrial: None,
        }),
        (Phase::Committed, Event::Retired) => Ok(Phase::Retired {
            outcome: Outcome::Committed,
            untried: false,
        }),
        (Phase::RolledBack { untried }, Event::Retired) => Ok(Phase::Retired {
            outcome: Outcome::RolledBack,
            untried: *untried,
        }),
        _ => Err(Refusal::Illegal {
            from: phase.kind(),
            event: event.kind(),
        }),
    }
}

// ─────────────────────────────────── writer rights ───────────────────────────────────

/// **Who acts on a transaction** ((b).2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Actor {
    /// O: the running build, and the in-app job owner of a later launch.
    Old,
    /// A lock holder whose tenure began in `Handoff` or `Armed`, and so
    /// performs the apply: P, or the rescue build taking P's place.
    Applier,
    /// A lock holder whose tenure began in any later phase: R. It writes
    /// `Trial` when it starts N over an exchange a dead applier left with the
    /// new bundle live (U-29b).
    Recovery,
    /// N: the new build started with the trial's nonce.
    Trial,
    /// Any ordinary start, reading only the header.
    Start,
}

/// **Who may write each phase into the journal** ((b).2, "Who may write
/// what"). N writes its receipt and, in one case only, one phase: U-35's
/// reserved trial records `Committed` over `TrialStarting` from its own
/// receipt ([`EventKind::LastTrialReady`], whose one author it is — the
/// writer of the journal checks [`EventKind::authors`] too, so N can record
/// no other event that ends in `Committed`).
pub(crate) const JOURNAL_WRITERS: &[(PhaseKind, &[Actor])] = &[
    (PhaseKind::Allocated, &[Actor::Old]),
    (
        PhaseKind::Prepared,
        &[Actor::Old, Actor::Applier, Actor::Recovery],
    ),
    (PhaseKind::Handoff, &[Actor::Old]),
    (PhaseKind::Armed, &[Actor::Applier]),
    (PhaseKind::Moving, &[Actor::Applier]),
    (PhaseKind::TrialStarting, &[Actor::Applier, Actor::Recovery]),
    (PhaseKind::Trial, &[Actor::Applier, Actor::Recovery]),
    (
        PhaseKind::Committed,
        &[Actor::Applier, Actor::Recovery, Actor::Trial],
    ),
    (
        PhaseKind::RollbackIntent,
        &[Actor::Applier, Actor::Recovery],
    ),
    (PhaseKind::Stuck, &[Actor::Applier, Actor::Recovery]),
    (PhaseKind::RolledBack, &[Actor::Applier, Actor::Recovery]),
    (PhaseKind::Abandoned, &[Actor::Old, Actor::Applier]),
    (PhaseKind::Retired, &[Actor::Applier, Actor::Recovery]),
];

/// Whether `actor` may record `phase`.
pub(crate) fn may_record(actor: Actor, phase: PhaseKind) -> bool {
    JOURNAL_WRITERS
        .iter()
        .any(|(kind, writers)| *kind == phase && writers.contains(&actor))
}

/// **Everything done to a file, a folder, the entrance or a process** other
/// than recording a phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Effect {
    /// N writes `H\<txn>\health-<nonce>`.
    WriteReceipt,
    /// The Run value or LaunchAgent plist, written, flushed and read back.
    WriteEntrance,
    RemoveEntrance,
    /// Windows: an old file, install → `backup\`.
    MoveOldOut,
    /// Windows: a new file, `set\` → install.
    MoveNewIn,
    /// Windows: a new file, install → `rolledout\`.
    MoveNewOut,
    /// Windows: an old file, `backup\` → install.
    MoveOldBack,
    /// macOS: the one `RENAME_SWAP` exchange, forward or back.
    Swap,
    /// Asking the trial process to quit and, after its grace, ending it.
    EndTrial,
    /// Deleting recorded rollback material: `backup\`'s old files, or the old
    /// bundle in `stage/`.
    DeleteRollbackMaterial,
    /// Detaching a disk image mounted under `H`.
    DetachMount,
    /// Deleting `H\<txn>`.
    DeleteTxnDir,
    /// Deleting `H\journal.json`.
    DeleteJournal,
}

/// **Who may do which effect, in which durable phase** ((b).2's two tables).
pub(crate) struct Right {
    pub(crate) actor: Actor,
    pub(crate) effect: Effect,
    pub(crate) during: &'static [PhaseKind],
}

const HOLDERS_ROLLING_BACK: &[PhaseKind] = &[PhaseKind::RollbackIntent, PhaseKind::Stuck];
const TERMINAL: &[PhaseKind] = &[PhaseKind::Abandoned, PhaseKind::Retired];
/// Where O clears a transaction away: `Allocated` (M1's sweep) and the
/// `Abandoned` it recorded itself before `Handoff` (U-27).
const OLD_CLEARS: &[PhaseKind] = &[PhaseKind::Allocated, PhaseKind::Abandoned];
/// Where an ordinary start deletes `H\<txn>` (a retirement, or a discard of a
/// folder replaced by hand) — and so where it first detaches whatever image is
/// still mounted under it (the coordinator's ruling, U-27).
const START_DELETES: &[PhaseKind] = &[
    PhaseKind::Allocated,
    PhaseKind::Prepared,
    PhaseKind::Abandoned,
    PhaseKind::Retired,
];

/// The rights table. Anything it does not list, nobody may do.
pub(crate) const EFFECT_RIGHTS: &[Right] = &[
    // O, as the job owner, sweeps what a dead preparation left (W1, M1), and
    // clears away a transaction it abandoned itself — a Prepare that failed, a
    // deferred one discarded at its second launch or on a failed revalidation
    // (U-27): the image detached first, then `H/<txn>`, then the journal.
    Right {
        actor: Actor::Old,
        effect: Effect::DetachMount,
        during: OLD_CLEARS,
    },
    Right {
        actor: Actor::Old,
        effect: Effect::DeleteTxnDir,
        during: OLD_CLEARS,
    },
    Right {
        actor: Actor::Old,
        effect: Effect::DeleteJournal,
        during: OLD_CLEARS,
    },
    // The applier arms, admits and moves (W3–W6, M3–M6).
    Right {
        actor: Actor::Applier,
        effect: Effect::WriteEntrance,
        during: &[PhaseKind::Handoff],
    },
    Right {
        actor: Actor::Applier,
        effect: Effect::RemoveEntrance,
        during: &[
            PhaseKind::Handoff,
            PhaseKind::Armed,
            PhaseKind::Moving,
            PhaseKind::Committed,
            PhaseKind::RolledBack,
            PhaseKind::Abandoned,
            PhaseKind::Retired,
        ],
    },
    Right {
        actor: Actor::Applier,
        effect: Effect::MoveOldOut,
        during: &[PhaseKind::Moving],
    },
    Right {
        actor: Actor::Applier,
        effect: Effect::MoveNewIn,
        during: &[PhaseKind::Moving],
    },
    Right {
        actor: Actor::Applier,
        effect: Effect::Swap,
        during: &[
            PhaseKind::Moving,
            PhaseKind::RollbackIntent,
            PhaseKind::Stuck,
        ],
    },
    // A lock holder that launched the trial over `Moving` and could not
    // record `TrialBegan` ends the trial it just launched (U-34): the
    // journal never knew it, so no recorded fact changes, and `decide` never
    // hands out `StopTrial` at `Moving` (it records no trial there).
    Right {
        actor: Actor::Applier,
        effect: Effect::EndTrial,
        during: &[PhaseKind::Moving],
    },
    // R ends a handed-back process of the new build that never became ready
    // (U-37, H.3 step 2) over `Moving`, and over U-35's `TrialStarting`,
    // whose reserved trial is exactly such a process when it is not ready. An
    // applier never holds `TrialStarting`: only an exit guard writes it, as
    // its road's last act, and an applier's entry refuses it.
    Right {
        actor: Actor::Recovery,
        effect: Effect::EndTrial,
        during: &[PhaseKind::Moving, PhaseKind::TrialStarting],
    },
    // Both lock holders roll back, commit and retire (W7–W13, M7–M11).
    Right {
        actor: Actor::Applier,
        effect: Effect::EndTrial,
        during: HOLDERS_ROLLING_BACK,
    },
    Right {
        actor: Actor::Recovery,
        effect: Effect::EndTrial,
        during: HOLDERS_ROLLING_BACK,
    },
    Right {
        actor: Actor::Applier,
        effect: Effect::MoveNewOut,
        during: HOLDERS_ROLLING_BACK,
    },
    Right {
        actor: Actor::Recovery,
        effect: Effect::MoveNewOut,
        during: HOLDERS_ROLLING_BACK,
    },
    Right {
        actor: Actor::Applier,
        effect: Effect::MoveOldBack,
        during: HOLDERS_ROLLING_BACK,
    },
    Right {
        actor: Actor::Recovery,
        effect: Effect::MoveOldBack,
        during: HOLDERS_ROLLING_BACK,
    },
    Right {
        actor: Actor::Recovery,
        effect: Effect::Swap,
        during: HOLDERS_ROLLING_BACK,
    },
    Right {
        actor: Actor::Applier,
        effect: Effect::DeleteRollbackMaterial,
        during: &[PhaseKind::Committed],
    },
    Right {
        actor: Actor::Recovery,
        effect: Effect::DeleteRollbackMaterial,
        during: &[PhaseKind::Committed],
    },
    // R reverts an exchange it finds not performed (M5), and retires the
    // entrance once an outcome is durable.
    Right {
        actor: Actor::Recovery,
        effect: Effect::RemoveEntrance,
        during: &[
            PhaseKind::Moving,
            PhaseKind::Committed,
            PhaseKind::RolledBack,
            PhaseKind::Abandoned,
            PhaseKind::Retired,
        ],
    },
    Right {
        actor: Actor::Applier,
        effect: Effect::DeleteTxnDir,
        during: TERMINAL,
    },
    Right {
        actor: Actor::Recovery,
        effect: Effect::DeleteTxnDir,
        during: TERMINAL,
    },
    Right {
        actor: Actor::Applier,
        effect: Effect::DeleteJournal,
        during: TERMINAL,
    },
    Right {
        actor: Actor::Recovery,
        effect: Effect::DeleteJournal,
        during: TERMINAL,
    },
    // N's receipt, in its recorded `Trial` and in U-35's `TrialStarting`,
    // which records no process yet. N's one journal write, U-35's
    // `LastTrialReady`, is in `JOURNAL_WRITERS` and `EventKind::authors`.
    Right {
        actor: Actor::Trial,
        effect: Effect::WriteReceipt,
        during: &[PhaseKind::TrialStarting, PhaseKind::Trial],
    },
    // An ordinary start retires a terminal transaction, and discards one whose
    // install was replaced by hand while it was preparing or deferred.
    Right {
        actor: Actor::Start,
        effect: Effect::RemoveEntrance,
        during: TERMINAL,
    },
    Right {
        actor: Actor::Start,
        effect: Effect::DetachMount,
        during: START_DELETES,
    },
    Right {
        actor: Actor::Start,
        effect: Effect::DeleteTxnDir,
        during: START_DELETES,
    },
    Right {
        actor: Actor::Start,
        effect: Effect::DeleteJournal,
        during: START_DELETES,
    },
];

/// Whether `actor` may do `effect` while the journal durably says `phase`.
pub(crate) fn may(actor: Actor, effect: Effect, phase: PhaseKind) -> bool {
    EFFECT_RIGHTS.iter().any(|right| {
        right.actor == actor && right.effect == effect && right.during.contains(&phase)
    })
}

// ───────────────────────────── the decision from disk ─────────────────────────────

/// Who is asking [`decide`]. Both hold the transaction lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asker {
    /// The in-app job owner of a start that continued past a preparing or
    /// deferred header.
    JobOwner,
    /// The applier P, or a lock holder that performs the apply in its place
    /// (U-24's Windows rescue).
    LockHolder,
    /// **The rescue build as recovery** (`--update-recover`, U-29b): R, from
    /// the entrance at login or from an ordinary start. It finishes whatever
    /// phase a dead applier left and applies nothing: a `Handoff` or an
    /// `Armed` it finds — nothing exchanged — goes back to `Prepared` (the
    /// coordinator's ruling 1); every later phase is decided as a lock
    /// holder's.
    Rescue,
}

impl Asker {
    /// The actor this asker is while the journal says `phase`: a lock holder
    /// that takes the lock in `Handoff` or `Armed` becomes the applier.
    pub(crate) fn actor(self, phase: PhaseKind) -> Actor {
        match (self, phase) {
            (Asker::JobOwner, _) => Actor::Old,
            (Asker::LockHolder | Asker::Rescue, PhaseKind::Handoff | PhaseKind::Armed) => {
                Actor::Applier
            }
            (Asker::LockHolder | Asker::Rescue, _) => Actor::Recovery,
        }
    }
}

/// **What is at one member's name, located by digest** — the file in the
/// install folder and the file in `backup\`, each as its SHA-256, `None` where
/// there is no file. The caller hashes what it finds; it never says what a file
/// *is*, only what it hashes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Seen {
    pub(crate) name: String,
    pub(crate) install: Option<Digest>,
    pub(crate) backup: Option<Digest>,
    /// **A file that is there and could not be read**, by place, and why —
    /// never absent (0.4.7 U-42b; 0.4.6's D-9: the new `folio.msix` held open
    /// with no sharing hashed to nothing, the rollback took it for absent,
    /// and the old file's move back collided with it). Empty where every file
    /// there was read.
    pub(crate) unread: Vec<(Place, String)>,
}

/// **The replaced thing as found on disk**: by digest for a Windows member
/// set, by identity for a macOS bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Located {
    /// One entry per name of either inventory; a name not listed is absent
    /// from both places.
    Members(Vec<Seen>),
    /// The identity at the launch path and at `H/<txn>/stage/`, `None` where
    /// there is no readable bundle.
    Bundle {
        live: Option<BundleIdentity>,
        stage: Option<BundleIdentity>,
    },
}

/// **A description of the disk, built by the caller** under the transaction
/// lock. Everything [`decide`] knows is here; it reads nothing else.
#[derive(Clone, Debug)]
pub(crate) struct Disk<'a> {
    pub(crate) journal: &'a Journal,
    pub(crate) asker: Asker,
    /// Whether the entrance (Run value or LaunchAgent plist) exists.
    pub(crate) entrance: bool,
    pub(crate) located: Located,
    /// The receipt found at [`Receipt::file_name`] of the trial's nonce, if
    /// it parsed.
    pub(crate) receipt: Option<Receipt>,
    /// Whether the process recorded in the journal (pid *and* start time) is
    /// still running.
    pub(crate) trial_alive: bool,
    /// Wall-clock milliseconds, for the trial's deadline.
    pub(crate) now_ms: u64,
}

/// **What the asker does next.** It performs the action and, where the action
/// ends in an [`Event`], records it through [`Journal::advance`]; then it asks
/// again. Every action is safe to repeat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Nothing is this asker's to do.
    Leave,
    /// W1, M1: detach any mount under `H`, delete `H\<txn>`, then the journal.
    Sweep,
    /// W2, M2: record [`Event::LaunchedWithoutResume`]; the in-app job may
    /// still resume (revalidating) in this launch.
    CountDeferredLaunch,
    /// W3, M3: write the entrance, flush and read it back, then record
    /// [`Event::Armed`] (or [`Event::EntranceFailed`]).
    Apply,
    /// W4, M5: remove the entrance if `remove_entrance`, then record
    /// [`Event::Reverted`].
    Revert { remove_entrance: bool },
    /// W5, M4: take exclusive admission and look for processes running from
    /// the install; record [`Event::Admitted`], or on a refusal remove the
    /// entrance and record [`Event::Reverted`].
    Admit,
    /// W6, W7: record [`Event::RollbackDeclared`].
    DeclareRollback,
    /// **M6 as U-29b rules it**: the exchange was performed — the new
    /// identity live, the old one in `stage/` — and nobody started the trial.
    /// Start the new build as the trial and record [`Event::TrialBegan`]; the
    /// trial is then waited for as M7. A trial that cannot be started records
    /// [`Event::RollbackDeclared`] (M9).
    BeginTrial,
    /// W7: the trial lives and its deadline has not passed; look again.
    AwaitReceipt { until_ms: u64 },
    /// W8, M8: record [`Event::ReceiptAccepted`] with the receipt in hand.
    Commit,
    /// W9, M9: ask the recorded trial process to quit, end it after its grace,
    /// and wait for it to be gone.
    StopTrial(TrialProcess),
    /// W9, M9: take exclusive admission and perform these steps, in order; a
    /// failure records [`Event::RollbackFailed`].
    RollBack(Restore),
    /// W9, M9: the old install is verified by digest (or identity); record
    /// [`Event::RolledBack`].
    DeclareRolledBack,
    /// W9, W10, M9, M10: the rollback cannot be completed from what is on
    /// disk; record [`Event::RollbackFailed`] with `reason` (or, already
    /// `Stuck`, keep it). The journal, the rollback material and the entrance
    /// all stay.
    StayStuck { reason: String },
    /// W10, M10 at the bound: `Stuck` has failed [`STUCK_ATTEMPT_LIMIT`]
    /// rollbacks, and this holder tries no more — it records nothing, keeps
    /// everything, and says `last_error` with the journal's folder.
    GiveUp { last_error: String },
    /// W11: remove the entrance if `remove_entrance`, record
    /// [`Event::Retired`], then relaunch the installed build with
    /// `--update-failed` — after the retirement, so that the start it makes
    /// finds a terminal journal and not one it would hand back (U-29).
    FinishRollback { remove_entrance: bool },
    /// W8 after the commit, W12, M8: delete exactly these recorded old files
    /// from `backup\` (or the old bundle from `stage/`), remove the entrance if
    /// `remove_entrance`, then record [`Event::Retired`]. Never a rollback.
    FinishCommit {
        delete_backup: Vec<String>,
        delete_stage: bool,
        remove_entrance: bool,
    },
    /// W13, a retired transaction: remove the entrance if `remove_entrance`,
    /// delete `H\<txn>`, and delete the journal only once `H\<txn>` is gone (a
    /// running rescue cannot delete itself, so the next start finishes it).
    Retire { remove_entrance: bool },
}

/// How the old install comes back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Restore {
    /// Windows: the new files out to `rolledout\` first, then the old files
    /// back from `backup\` — the new set is never destroyed before the old is
    /// restored (F-7).
    Moves(Vec<Move>),
    /// macOS: exchange the live new bundle with the old one in `stage/`.
    SwapBack,
}

impl Action {
    /// **The effects this action performs**, for [`EFFECT_RIGHTS`].
    pub(crate) fn effects(&self) -> Vec<Effect> {
        match self {
            Action::Leave
            | Action::CountDeferredLaunch
            | Action::Admit
            | Action::DeclareRollback
            | Action::BeginTrial
            | Action::AwaitReceipt { .. }
            | Action::Commit
            | Action::DeclareRolledBack
            | Action::StayStuck { .. }
            | Action::GiveUp { .. } => Vec::new(),
            Action::Sweep => vec![
                Effect::DetachMount,
                Effect::DeleteTxnDir,
                Effect::DeleteJournal,
            ],
            Action::Apply => vec![Effect::WriteEntrance],
            Action::Revert { remove_entrance } | Action::FinishRollback { remove_entrance } => {
                entrance_removal(*remove_entrance).collect()
            }
            Action::StopTrial(_) => vec![Effect::EndTrial],
            Action::RollBack(Restore::SwapBack) => vec![Effect::Swap],
            Action::RollBack(Restore::Moves(moves)) => moves
                .iter()
                .map(|step| match step.to {
                    Place::RolledOut => Effect::MoveNewOut,
                    _ => Effect::MoveOldBack,
                })
                .collect(),
            Action::FinishCommit {
                delete_backup,
                delete_stage,
                remove_entrance,
            } => (!delete_backup.is_empty() || *delete_stage)
                .then_some(Effect::DeleteRollbackMaterial)
                .into_iter()
                .chain(entrance_removal(*remove_entrance))
                .collect(),
            Action::Retire { remove_entrance } => entrance_removal(*remove_entrance)
                .chain([Effect::DeleteTxnDir, Effect::DeleteJournal])
                .collect(),
        }
    }
}

fn entrance_removal(remove: bool) -> impl Iterator<Item = Effect> {
    remove.then_some(Effect::RemoveEntrance).into_iter()
}

/// **The recovery decision, from what is on disk and nothing remembered**
/// ((b).2's W-rows and M-rows). The asker holds the transaction lock; the
/// answer depends only on `disk`, so recovery interrupted during recovery is
/// recovery again.
pub(crate) fn decide(disk: &Disk<'_>) -> Action {
    let journal = disk.journal;
    let phase = &journal.body.phase;
    if disk.asker == Asker::JobOwner {
        return match phase {
            Phase::Allocated => Action::Sweep,
            Phase::Prepared { .. } => Action::CountDeferredLaunch,
            _ => Action::Leave,
        };
    }
    match phase {
        Phase::Allocated | Phase::Prepared { .. } => Action::Leave,
        // Nothing was exchanged: `Admitted` is recorded before the exchange.
        Phase::Handoff { .. } | Phase::Armed if disk.asker == Asker::Rescue => Action::Revert {
            remove_entrance: disk.entrance,
        },
        Phase::Handoff { .. } if disk.entrance => Action::Revert {
            remove_entrance: true,
        },
        Phase::Handoff { .. } => Action::Apply,
        Phase::Armed => Action::Admit,
        Phase::Moving => match (&journal.body.layout, &disk.located) {
            (Layout::Bundle { old, .. }, Located::Bundle { live, .. })
                if live.as_ref() == Some(old) =>
            {
                Action::Revert {
                    remove_entrance: disk.entrance,
                }
            }
            (Layout::Bundle { old, new }, Located::Bundle { live, stage })
                if live.as_ref() == Some(new) && stage.as_ref() == Some(old) =>
            {
                Action::BeginTrial
            }
            _ => Action::DeclareRollback,
        },
        // The U-35 launch was refused or died before a receipt could identify
        // it. A live/ready process is adopted by `survey` before `decide`;
        // reaching this arm means there is no trial to wait for.
        Phase::TrialStarting { .. } => Action::DeclareRollback,
        Phase::Trial { began_ms, .. } => {
            let answered = disk.receipt.as_ref().is_some_and(|receipt| {
                next(
                    &journal.txn,
                    phase,
                    &Event::ReceiptAccepted(receipt.clone()),
                )
                .is_ok()
            });
            let until_ms = began_ms.saturating_add(TRIAL_DEADLINE_MS);
            if answered {
                Action::Commit
            } else if disk.trial_alive && disk.now_ms < until_ms {
                Action::AwaitReceipt { until_ms }
            } else {
                Action::DeclareRollback
            }
        }
        // The trial started over this `Stuck` (U-29b, ruling 3): its receipt
        // commits forward, and while it lives within its deadline it is
        // waited for, as `Trial` is (W7, W8).
        Phase::Stuck {
            retrial: Some(_), ..
        } if disk.receipt.as_ref().is_some_and(|receipt| {
            next(
                &journal.txn,
                phase,
                &Event::ReceiptAccepted(receipt.clone()),
            )
            .is_ok()
        }) =>
        {
            Action::Commit
        }
        Phase::Stuck {
            retrial: Some(retrial),
            ..
        } if disk.trial_alive
            && disk.now_ms < retrial.began_ms.saturating_add(TRIAL_DEADLINE_MS) =>
        {
            Action::AwaitReceipt {
                until_ms: retrial.began_ms.saturating_add(TRIAL_DEADLINE_MS),
            }
        }
        Phase::Stuck {
            last_error,
            attempts,
            ..
        } if *attempts >= STUCK_ATTEMPT_LIMIT && !disk.trial_alive => Action::GiveUp {
            last_error: last_error.clone(),
        },
        Phase::RollbackIntent { trial, .. } | Phase::Stuck { trial, .. } => match trial {
            Some(process) if disk.trial_alive => Action::StopTrial(*process),
            _ => match restore(&journal.body.layout, &disk.located) {
                Err(reason) => Action::StayStuck { reason },
                Ok(None) => Action::DeclareRolledBack,
                Ok(Some(steps)) => Action::RollBack(steps),
            },
        },
        Phase::Committed => finish_commit(disk),
        Phase::RolledBack { .. } => Action::FinishRollback {
            remove_entrance: disk.entrance,
        },
        Phase::Abandoned | Phase::Retired { .. } => Action::Retire {
            remove_entrance: disk.entrance,
        },
    }
}

/// The steps that bring the old install back, `None` when it already is, or
/// why it cannot be done from what is on disk.
fn restore(layout: &Layout, located: &Located) -> Result<Option<Restore>, String> {
    match (layout, located) {
        (Layout::Members(inventories), Located::Members(seen)) => rollback_moves(inventories, seen)
            .map(|moves| (!moves.is_empty()).then_some(Restore::Moves(moves))),
        (Layout::Bundle { old, new }, Located::Bundle { live, stage }) => {
            if live.as_ref() == Some(old) {
                Ok(None)
            } else if live.as_ref() == Some(new) && stage.as_ref() == Some(old) {
                Ok(Some(Restore::SwapBack))
            } else {
                Err("neither bundle identity is where a swap back would need it".to_owned())
            }
        }
        _ => Err("the disk was described for another layout".to_owned()),
    }
}

/// **The Windows rollback, reconciled by digest against the recorded
/// inventories** (F-7): never by inverting the last phase. A name whose
/// install file is neither its old nor its new digest is somebody else's file,
/// and is never moved: the rollback stops there instead.
pub(crate) fn rollback_moves(
    inventories: &Inventories,
    seen: &[Seen],
) -> Result<Vec<Move>, String> {
    let mut out = Vec::new();
    let mut back = Vec::new();
    for name in inventories.names() {
        let entry = seen.iter().find(|entry| entry.name == name);
        // A file that is there and could not be read is neither absent nor
        // any digest: the rollback cannot say what it is, cannot move it out,
        // and cannot put the old file where it stands (U-42b).
        if let Some((place, why)) = entry.and_then(|entry| entry.unread.first()) {
            return Err(match place {
                Place::Install => {
                    format!("`{name}` in the install could not be read or moved out: {why}")
                }
                _ => format!("the old `{name}` in the backup could not be read: {why}"),
            });
        }
        let (install, backup) = entry.map_or((None, None), |entry| (entry.install, entry.backup));
        let old = inventories.old(name).map(|member| member.digest);
        let new = inventories.new_member(name).map(|member| member.digest);
        if install.is_some() && install == old {
            continue;
        }
        if install.is_some() && install == new {
            out.push(Move {
                name: name.to_owned(),
                from: Place::Install,
                to: Place::RolledOut,
            });
        } else if install.is_some() {
            return Err(format!(
                "`{name}` in the install is neither the old file nor the new one"
            ));
        }
        if let Some(old) = old {
            if backup != Some(old) {
                return Err(format!(
                    "the old `{name}` is in neither the install nor the backup"
                ));
            }
            back.push(Move {
                name: name.to_owned(),
                from: Place::Backup,
                to: Place::Install,
            });
        }
    }
    out.extend(back);
    Ok(out)
}

fn finish_commit(disk: &Disk<'_>) -> Action {
    let (delete_backup, delete_stage) = match (&disk.journal.body.layout, &disk.located) {
        (Layout::Members(inventories), Located::Members(seen)) => (
            inventories
                .old_present
                .iter()
                .filter(|member| {
                    seen.iter().any(|entry| {
                        entry.name == member.name && entry.backup == Some(member.digest)
                    })
                })
                .map(|member| member.name.clone())
                .collect(),
            false,
        ),
        (Layout::Bundle { old, .. }, Located::Bundle { stage, .. }) => {
            (Vec::new(), stage.as_ref() == Some(old))
        }
        _ => (Vec::new(), false),
    };
    Action::FinishCommit {
        delete_backup,
        delete_stage,
        remove_entrance: disk.entrance,
    }
}

// ───────────────────────────── the installation home ─────────────────────────────

/// The name of the Windows installation home inside the install folder.
pub(crate) const WINDOWS_HOME: &str = ".folio-update";

/// What the macOS home's name is made of: `.` + the bundle's own name + this
/// (`/Applications/Folio.app` → `/Applications/.Folio.app.folio-update`, F-3).
pub(crate) const MACOS_HOME_SUFFIX: &str = ".folio-update";

/// Where a Folio bundle keeps its main executable — `packaging/macos/Info.plist.in`'s
/// `CFBundleExecutable` under `Contents/MacOS` — for a home found from the
/// bundle alone ([`Home::for_bundle`]). A home found from the running
/// executable ([`Home::of`]) uses that executable's own place instead.
pub(crate) const MACOS_EXECUTABLE_INSIDE: &str = "Contents/MacOS/folio";

/// **The installation home `H` and the objects in it** ((b).2's objects
/// table): `<install>\.folio-update\` on Windows, and on macOS the fixed
/// sibling of the bundle, `<parent>/.<BundleName>.folio-update/` (F-3), so the
/// running copy finds it from its own executable and nothing else.
///
/// A path, not a claim that anything is there: a copy started before any
/// transaction finds no home at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Home {
    root: PathBuf,
    rescue: RescueShape,
}

/// **What the header's `rescue` names**, which differs by platform: the
/// rescue executable itself on Windows (`H\<txn>\rescue\folio.exe`), the
/// rescue clone's bundle on macOS, whose executable sits where this copy's own
/// sits inside its bundle — the clone is a copy of this very bundle (F-8).
#[derive(Clone, Debug, PartialEq, Eq)]
enum RescueShape {
    Executable,
    /// `name` is the bundle's own file name (`Folio.app`), which the staged
    /// and the rescue bundle keep; `inside` is the executable's path inside it.
    Bundle {
        name: OsString,
        inside: PathBuf,
    },
}

impl Home {
    /// The home of the copy whose executable is `exe`, or `None` where this
    /// build has no updater home: an executable outside a bundle on macOS, and
    /// every other platform.
    pub(crate) fn of(platform: HostPlatform, exe: &Path) -> Option<Self> {
        match platform {
            HostPlatform::Windows => Some(Self {
                root: exe.parent()?.join(WINDOWS_HOME),
                rescue: RescueShape::Executable,
            }),
            HostPlatform::MacOs => {
                let (bundle, inside) = bundle_of(exe)?;
                let mut home = Self::for_bundle(bundle)?;
                if let RescueShape::Bundle { inside: at, .. } = &mut home.rescue {
                    *at = inside.to_path_buf();
                }
                Some(home)
            }
            HostPlatform::OtherUnix => None,
        }
    }

    /// **The home of the rescue build whose executable is `exe`, and the
    /// installed program it starts again**: `H\<txn>\rescue\<name>` gives `H`
    /// and `<install>\<name>` — the entrance's command names only the rescue
    /// program, and the journal's place follows from it (F-2). `None` when
    /// `exe` does not sit in a `rescue` folder of an installation home, and on
    /// every platform but Windows (the macOS rescue clone is U-26's).
    pub(crate) fn of_rescue(platform: HostPlatform, exe: &Path) -> Option<(Self, PathBuf)> {
        if platform != HostPlatform::Windows {
            return None;
        }
        let rescue = exe.parent()?;
        let root = rescue.parent()?.parent()?;
        if rescue.file_name()? != "rescue" || root.file_name()? != WINDOWS_HOME {
            return None;
        }
        let installed = root.parent()?.join(exe.file_name()?);
        Some((
            Self {
                root: root.to_path_buf(),
                rescue: RescueShape::Executable,
            },
            installed,
        ))
    }

    /// **The home the entrance named, and the installed program the rescue
    /// build at `exe` starts again** — `--update-recover <home>` (F-3).
    ///
    /// macOS: `home` must be a locator's home, `<parent>/.<Bundle>.folio-update`;
    /// the installed bundle is `<parent>/<Bundle>`, and its program sits where
    /// the rescue clone's own executable sits inside the clone (the clone is a
    /// copy of that bundle). Windows: the home the rescue build's own path
    /// gives ([`Home::of_rescue`]), which must be the one named. `None` for a
    /// name that is not a home's, and on every other platform.
    pub(crate) fn of_rescue_named(
        platform: HostPlatform,
        exe: &Path,
        home: &Path,
    ) -> Option<(Self, PathBuf)> {
        match platform {
            HostPlatform::Windows => {
                Self::of_rescue(platform, exe).filter(|(found, _)| found.root == home)
            }
            HostPlatform::MacOs => {
                let name = home.file_name()?.to_str()?;
                let bundle_name = name.strip_prefix('.')?.strip_suffix(MACOS_HOME_SUFFIX)?;
                let bundle = home.parent()?.join(bundle_name);
                let (_, inside) = bundle_of(exe)?;
                // The locator of that bundle is `home` itself, by construction.
                let mut found = Self::for_bundle(&bundle)?;
                if let RescueShape::Bundle { inside: at, .. } = &mut found.rescue {
                    *at = inside.to_path_buf();
                }
                Some((found, bundle.join(inside)))
            }
            HostPlatform::OtherUnix => None,
        }
    }

    /// **The locator (F-3): the macOS home of the bundle at `bundle`**, a fixed
    /// sibling `<parent>/.<BundleName>.folio-update/`, or `None` for a path
    /// that is not an `.app` with a parent.
    ///
    /// A function of the bundle's path and nothing else — not the data
    /// directory, not the account — so every data root and every account that
    /// runs this bundle finds the same home, and the home sits outside both
    /// bundles an exchange swaps, on the bundle's own volume. Pure: nothing is
    /// read, and nothing need exist.
    pub(crate) fn for_bundle(bundle: &Path) -> Option<Self> {
        if bundle.extension()? != "app" {
            return None;
        }
        let bundle_name = bundle.file_name()?;
        let mut name = OsString::from(".");
        name.push(bundle_name);
        name.push(MACOS_HOME_SUFFIX);
        Some(Self {
            root: bundle.parent()?.join(name),
            rescue: RescueShape::Bundle {
                name: bundle_name.to_os_string(),
                inside: PathBuf::from(MACOS_EXECUTABLE_INSIDE),
            },
        })
    }

    /// `H` itself: the folder `--uninstall-cleanup` removes, and the argument
    /// the entrance hands the rescue build.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// A Windows-shaped home at `root` itself, for a test that builds one in a
    /// temporary folder.
    #[cfg(test)]
    pub(crate) fn at(root: PathBuf) -> Self {
        Self {
            root,
            rescue: RescueShape::Executable,
        }
    }

    /// **The program a start hands itself to**, from the header's `rescue`.
    pub(crate) fn rescue_program(&self, rescue: &str) -> PathBuf {
        match &self.rescue {
            RescueShape::Executable => PathBuf::from(rescue),
            RescueShape::Bundle { inside, .. } => Path::new(rescue).join(inside),
        }
    }

    /// `H\admission`: shared by every running copy, exclusive by the mover.
    pub(crate) fn admission(&self) -> PathBuf {
        self.root.join("admission")
    }

    /// `H\lock`: the transaction lock, held exclusive by whoever advances or
    /// retires the journal.
    pub(crate) fn lock(&self) -> PathBuf {
        self.root.join("lock")
    }

    /// `H\journal.json`.
    pub(crate) fn journal(&self) -> PathBuf {
        self.root.join("journal.json")
    }

    /// `H\<txn>`: everything one transaction owns.
    pub(crate) fn transaction(&self, txn: TxnId) -> PathBuf {
        self.root.join(txn.to_string())
    }

    /// `H\<txn>\health-<nonce>`: the trial's receipt ((b).2's objects table).
    pub(crate) fn receipt_path(&self, txn: TxnId, nonce: &Nonce) -> PathBuf {
        self.transaction(txn).join(Receipt::file_name(nonce))
    }

    /// **The installed bundle this macOS home belongs to**:
    /// `<parent>/<Bundle>.app`, the home's sibling — what the applier
    /// exchanges with `stage/` (U-28). `None` for a Windows home.
    pub(crate) fn installed_bundle(&self) -> Option<PathBuf> {
        let name = self.bundle_name()?;
        Some(self.root.parent()?.join(name))
    }

    /// **The installed bundle's main executable**, where this home's rescue
    /// clone keeps its own inside it — the image a process of the installed
    /// build runs (U-28). `None` for a Windows home.
    pub(crate) fn installed_program(&self) -> Option<PathBuf> {
        let RescueShape::Bundle { inside, .. } = &self.rescue else {
            return None;
        };
        Some(self.installed_bundle()?.join(inside))
    }

    /// **Windows `H\<txn>\<place>\`**: `set\` (the verified new files,
    /// before they move in), `backup\` (the old files, moved out) and
    /// `rolledout\` ((b).2's objects table, [`Place`]). `None` for
    /// [`Place::Install`], which is the install folder itself, and for a macOS
    /// home, whose transaction is a bundle.
    pub(crate) fn members_folder(&self, txn: TxnId, place: Place) -> Option<PathBuf> {
        if self.bundle_name().is_some() {
            return None;
        }
        let name = match place {
            Place::Install => return None,
            Place::Set => "set",
            Place::Backup => "backup",
            Place::RolledOut => "rolledout",
        };
        Some(self.transaction(txn).join(name))
    }

    /// **Windows `H\<txn>\rescue\<name>`**: the copy of the running
    /// executable the applier and recovery run from (F-8), and what the
    /// header's `rescue` names — [`Home::of_rescue`]'s inverse. `None` for a
    /// macOS home, whose rescue is a bundle ([`Home::rescue_bundle`]).
    pub(crate) fn rescue_copy(&self, txn: TxnId, name: &OsStr) -> Option<PathBuf> {
        if self.bundle_name().is_some() {
            return None;
        }
        Some(self.transaction(txn).join("rescue").join(name))
    }

    /// The bundle's own name, for the members a macOS home keeps under it.
    fn bundle_name(&self) -> Option<&OsStr> {
        match &self.rescue {
            RescueShape::Executable => None,
            RescueShape::Bundle { name, .. } => Some(name),
        }
    }

    /// macOS `H/<txn>/stage/<Bundle>.app`: the verified new bundle until the
    /// exchange, the old bundle after it — the rollback source (F-3). `None`
    /// for a Windows home, whose staged set is not a bundle.
    pub(crate) fn stage_bundle(&self, txn: TxnId) -> Option<PathBuf> {
        let name = self.bundle_name()?;
        Some(self.transaction(txn).join("stage").join(name))
    }

    /// macOS `H/<txn>/rescue/<Bundle>.app`: the clone of the old bundle the
    /// applier and recovery run from (F-3, F-8), and what the header's
    /// `rescue` names. `None` for a Windows home.
    pub(crate) fn rescue_bundle(&self, txn: TxnId) -> Option<PathBuf> {
        let name = self.bundle_name()?;
        Some(self.transaction(txn).join("rescue").join(name))
    }

    /// macOS `H/<txn>/rescue/<Bundle>.app/<inside>`: the program the
    /// LaunchAgent entrance runs. `None` for a Windows home.
    pub(crate) fn rescue_executable(&self, txn: TxnId) -> Option<PathBuf> {
        let RescueShape::Bundle { name, inside } = &self.rescue else {
            return None;
        };
        Some(self.transaction(txn).join("rescue").join(name).join(inside))
    }

    /// macOS `H/<txn>/mnt`: where the downloaded image is attached, found
    /// again from the mount table (M1, U-17). `None` for a Windows home.
    pub(crate) fn mount_point(&self, txn: TxnId) -> Option<PathBuf> {
        self.bundle_name()?;
        Some(self.transaction(txn).join("mnt"))
    }
}

/// A macOS executable's bundle and its path inside it:
/// `<X>.app/Contents/MacOS/<name>` → (`<X>.app`, `Contents/MacOS/<name>`).
fn bundle_of(exe: &Path) -> Option<(&Path, &Path)> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let shaped = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app";
    if !shaped {
        return None;
    }
    Some((bundle, exe.strip_prefix(bundle).ok()?))
}

// ───────────────────────────── what the trial sees ─────────────────────────────

/// **What the trial N reads of its own transaction while it waits to be
/// committed** — F-7's "it releases them only when it reads `Committed` in the
/// journal" (U-13, `update_trial`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrialSight {
    /// Not decided yet (`Trial`, or a phase before it), a rollback not
    /// finished (`RollbackIntent`, `Stuck`, `RolledBack` — a `Stuck` whose new
    /// bundle is live still recovers forward on its retrial's receipt, U-29b),
    /// or not readable by this build: the trial keeps its writes pending and
    /// looks again.
    Undecided,
    /// `Committed` is durable (or the transaction retired after it): the trial's
    /// writes may land.
    Committed,
    /// Decided otherwise and retired — `Abandoned`, retired without a commit —
    /// or gone: the journal is absent, or names another transaction. Nothing it
    /// held back will ever be written.
    Ended,
}

/// **What the journal's header says about the trial of `txn`**; `None` is no
/// journal at all.
///
/// The frozen header alone (F-8): `outcome == committed` releases the trial's
/// writes; a `terminal` class without it ends them; anything else — a
/// rollback still `destructive` included (U-29b: a `Stuck` whose new bundle is
/// live commits forward on the receipt of the trial started over it, so that
/// trial must be able to write one), or a header this build cannot read — is
/// not decided yet. A journal naming another transaction, or none, means this
/// trial's is gone. A header this build cannot read is a `destructive`
/// transaction with nothing decided ([`Sight::acting_header`],
/// [`Role::TrialWatch`]).
pub(crate) fn trial_sight(journal: Option<&[u8]>, txn: &TxnId) -> TrialSight {
    let Some(bytes) = journal else {
        return TrialSight::Ended;
    };
    let Some(header) = Role::TrialWatch.sight(bytes).acting_header() else {
        return TrialSight::Undecided;
    };
    if header.txn != *txn {
        return TrialSight::Ended;
    }
    match (header.outcome, header.class) {
        (HeaderOutcome::Committed, _) => TrialSight::Committed,
        (_, Class::Terminal) => TrialSight::Ended,
        _ => TrialSight::Undecided,
    }
}

// ───────────────────────────── the ordinary start ─────────────────────────────

/// What an ordinary start found at `H\journal.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum JournalRead {
    Absent,
    Unreadable(ParseRefusal),
    Read(Header),
}

/// **A description of what an ordinary start sees**, built by the caller:
/// the header, whether the transaction lock was free, the digest of its own
/// image and of the rescue build the header names, the transaction its own
/// `--update-trial` names, if it was started as a trial, and whether it
/// carries `--update-failed`.
///
/// `own_image` is `None` when the start did not or could not measure itself:
/// it then cannot tell a replaced install from its own, and replaces nothing.
#[derive(Clone, Debug)]
pub(crate) struct StartView {
    pub(crate) journal: JournalRead,
    pub(crate) lock_free: bool,
    pub(crate) own_image: Option<Digest>,
    pub(crate) rescue_image: Option<Digest>,
    pub(crate) trial_of: Option<TxnId>,
    /// **This start was sent by a lock holder after a rollback** — it carries
    /// `--update-failed <journal>` (U-29): it raises the card at `Failed`, and
    /// past an unfinished rollback it continues instead of handing itself
    /// back. The word decides, not the spelling of the path after it: a start
    /// that the rescue build sent and then handed back would come straight
    /// back with the word twice, which the command line refuses.
    pub(crate) sent_by_rollback: bool,
}

/// What an ordinary start does about the transaction before anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartAction {
    /// Start as usual.
    Continue,
    /// Terminal: remove the entrance if present, delete `H\<txn>`, then the
    /// journal once `H\<txn>` is gone; then start as usual.
    Retire,
    /// Preparing or deferred, and this start's own image is not the rescue
    /// copy of O: the install was replaced by hand. Delete `H\<txn>` and the
    /// journal — nothing in the install — then start as usual.
    Discard,
    /// Destructive: start the rescue build with `--then-launch <argv>` and exit.
    HandToRescue,
    /// This start is the trial the header's transaction started: run, writing
    /// nothing durable until the journal says `Committed` (U-13).
    RunAsTrial,
}

/// **What a start's retirement or discard removes, after the entrance** —
/// the closed list of everything an ordinary start may do to a transaction's
/// files ([`StartAction::steps`]). Closed so that the start that performs it
/// matches it whole: a step added here does not compile until every place
/// that performs one says what it does (0.4.8 E1-a2, F10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Clearing {
    /// Detach every image mounted under `H/<txn>`.
    DetachMount,
    /// Remove one thing, durably.
    Delete(Removal),
}

/// What a [`Clearing::Delete`] removes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Removal {
    /// `H\<txn>`.
    TxnDir,
    /// `H\journal.json`, once `H\<txn>` is gone.
    Journal,
}

impl Clearing {
    /// The effect of [`EFFECT_RIGHTS`] this step is.
    pub(crate) const fn effect(self) -> Effect {
        match self {
            Clearing::DetachMount => Effect::DetachMount,
            Clearing::Delete(Removal::TxnDir) => Effect::DeleteTxnDir,
            Clearing::Delete(Removal::Journal) => Effect::DeleteJournal,
        }
    }
}

/// **The steps of a start's action, in the protocol's order**: the entrance
/// first when it goes, then the clearing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StartSteps {
    /// The Run value or LaunchAgent plist is removed, if it is still there.
    pub(crate) remove_entrance: bool,
    pub(crate) clearing: &'static [Clearing],
}

/// A mount under `H/<txn>` is detached before the folder is deleted (U-17's
/// debt 7, the coordinator's ruling in U-27): a read-only volume inside it
/// would stop the deletion halfway. The journal goes last.
const CLEARING: &[Clearing] = &[
    Clearing::DetachMount,
    Clearing::Delete(Removal::TxnDir),
    Clearing::Delete(Removal::Journal),
];

impl StartAction {
    /// What this action removes, and in which order.
    pub(crate) const fn steps(self) -> StartSteps {
        match self {
            StartAction::Continue | StartAction::HandToRescue | StartAction::RunAsTrial => {
                StartSteps {
                    remove_entrance: false,
                    clearing: &[],
                }
            }
            StartAction::Retire => StartSteps {
                remove_entrance: true,
                clearing: CLEARING,
            },
            StartAction::Discard => StartSteps {
                remove_entrance: false,
                clearing: CLEARING,
            },
        }
    }

    /// The effects this action performs, for [`EFFECT_RIGHTS`]: its
    /// [`steps`](Self::steps), each as its effect.
    pub(crate) fn effects(self) -> Vec<Effect> {
        let steps = self.steps();
        steps
            .remove_entrance
            .then_some(Effect::RemoveEntrance)
            .into_iter()
            .chain(steps.clearing.iter().map(|step| step.effect()))
            .collect()
    }
}

/// **The ordinary start's rule, from the header alone** ((b).2). A journal
/// this build cannot read is left exactly as it is: nothing is deleted on the
/// strength of bytes nobody understood.
pub(crate) fn at_start(view: &StartView) -> StartAction {
    let JournalRead::Read(header) = &view.journal else {
        return StartAction::Continue;
    };
    match header.class {
        Class::Terminal if view.lock_free => StartAction::Retire,
        Class::Terminal => StartAction::Continue,
        Class::Destructive if view.trial_of == Some(header.txn) => StartAction::RunAsTrial,
        // A lock holder sent this start after a rollback it could not finish,
        // or after its own recovery failed (U-29b, the coordinator's ruling
        // 2): handing it back would only send it here again. The next start
        // without the word hands over as usual (M10).
        Class::Destructive if view.sent_by_rollback => StartAction::Continue,
        Class::Destructive => StartAction::HandToRescue,
        Class::Preparing | Class::Deferred => {
            let replaced = matches!(
                (view.own_image, view.rescue_image),
                (Some(own), Some(rescue)) if own != rescue
            );
            if view.lock_free && replaced {
                StartAction::Discard
            } else {
                StartAction::Continue
            }
        }
    }
}

/// **What a start sent with `--update-failed` tells the reader**, from the
/// frozen header alone (U-29, U-29b).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AfterRollback {
    /// `rolled_back` and `terminal`: the old build is back and the
    /// transaction retired — *Previous version restored.*
    Restored,
    /// Still `destructive`, whatever its outcome: the rollback did not finish
    /// (`Stuck`, or not begun), or recovery itself failed and started the
    /// live build this way (U-29b, the coordinator's ruling 2) — *Update
    /// incomplete.* and the journal's folder.
    Incomplete,
}

/// **Whether a retired rollback's new version never ran** (U-42a): the
/// body's `Retired { untried }`, read from the whole journal. `false` for any
/// other journal, and for one this build cannot read whole.
pub(crate) fn rolled_back_untried(sight: &Sight) -> bool {
    matches!(
        sight,
        Sight::Known(Journal {
            body: Body {
                phase: Phase::Retired {
                    outcome: Outcome::RolledBack,
                    untried: true,
                },
                ..
            },
            ..
        })
    )
}

/// **The card a start sent with `--update-failed` raises**, or `None` for a
/// header that is neither a retired rollback nor still `destructive`.
pub(crate) fn after_rollback(header: &Header) -> Option<AfterRollback> {
    match (header.outcome, header.class) {
        (HeaderOutcome::RolledBack, Class::Terminal) => Some(AfterRollback::Restored),
        (_, Class::Destructive) => Some(AfterRollback::Incomplete),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! The recovery contract of (b).2, one test per row of each table, plus the
    //! four rules the contract turns on, the two frozen formats, and the
    //! enumerations that keep the transition and rights tables honest.
    //!
    //! Every disk here is synthetic: members are named after the shape of a
    //! Folio ZIP (an executable, two ConPTY sidecars, a command file) and their
    //! digests are single repeated bytes. `FakeDisk` renames the way a file
    //! system does — a rename never replaces its target — so a plan that would
    //! overwrite a file fails the test rather than passing it.

    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn digest(tag: u8) -> Digest {
        Digest::new([tag; 32])
    }

    fn nonce(tag: u8) -> Nonce {
        Nonce::new([tag; 32])
    }

    fn txn() -> TxnId {
        TxnId::new([0x7a; 16])
    }

    /// **A registry in memory**, for the entrance's proof: the proof is made
    /// only by `logon_hook::arm_in`, after a write, a flush and a read-back,
    /// and these tests make it that way too.
    #[derive(Default)]
    struct MemoryRegistry(BTreeMap<String, (u32, Vec<u8>)>);

    impl bt_platform::logon_hook::Registry for MemoryRegistry {
        fn set(&mut self, _: &str, name: &str, kind: u32, data: &[u8]) -> std::io::Result<()> {
            self.0.insert(name.to_owned(), (kind, data.to_vec()));
            Ok(())
        }

        fn flush(&mut self, _: &str) -> std::io::Result<()> {
            Ok(())
        }

        fn get(&mut self, _: &str, name: &str) -> std::io::Result<Option<(u32, Vec<u8>)>> {
            Ok(self.0.get(name).cloned())
        }

        fn delete(&mut self, _: &str, name: &str) -> std::io::Result<bool> {
            Ok(self.0.remove(name).is_some())
        }

        fn names(&mut self, _: &str) -> std::io::Result<Vec<String>> {
            Ok(self.0.keys().cloned().collect())
        }
    }

    /// The entrance's proof for `txn`, made through the door.
    fn armed_for(txn: TxnId) -> bt_platform::install_txn::Armed {
        bt_platform::logon_hook::arm_in(
            &mut MemoryRegistry::default(),
            "test",
            txn.bytes(),
            Path::new(r"C:\Folio\.folio-update\7a7a\rescue\folio.exe"),
        )
        .expect("the in-memory entrance reads back")
    }

    /// The entrance's proof for this module's transaction.
    fn armed() -> Event {
        Event::Armed(armed_for(txn()))
    }

    const TRIAL: TrialProcess = TrialProcess {
        pid: 4242,
        started: 1_000,
    };
    const BEGAN: u64 = 1_000_000;
    const TRIAL_NONCE: u8 = 0x33;

    fn member(name: &str, tag: u8) -> Member {
        Member {
            name: name.to_owned(),
            digest: digest(tag),
            size: u64::from(tag) * 100,
        }
    }

    /// The old build shipped four files, and a fifth, `helper.dll`, was already
    /// in the folder under a name the new build ships (a collision). The new
    /// build drops `uninstall.cmd`, keeps `conpty.dll` byte for byte, and
    /// changes the rest.
    fn inventories() -> Inventories {
        Inventories {
            old_shipped: [
                "folio.exe",
                "conpty.dll",
                "OpenConsole.exe",
                "uninstall.cmd",
            ]
            .map(str::to_owned)
            .to_vec(),
            old_present: vec![
                member("folio.exe", 0x01),
                member("conpty.dll", 0x02),
                member("OpenConsole.exe", 0x03),
                member("uninstall.cmd", 0x04),
                member("helper.dll", 0x05),
            ],
            new: vec![
                member("folio.exe", 0x11),
                member("conpty.dll", 0x02),
                member("OpenConsole.exe", 0x13),
                member("helper.dll", 0x15),
            ],
        }
    }

    fn old_bundle() -> BundleIdentity {
        BundleIdentity {
            cdhash: Cdhash::new([0x01; 20]),
            version: "0.4.6".to_owned(),
        }
    }

    fn new_bundle() -> BundleIdentity {
        BundleIdentity {
            cdhash: Cdhash::new([0x02; 20]),
            version: "0.4.7".to_owned(),
        }
    }

    fn members_layout() -> Layout {
        Layout::Members(inventories())
    }

    fn bundle_layout() -> Layout {
        Layout::Bundle {
            old: old_bundle(),
            new: new_bundle(),
        }
    }

    fn journal(phase: Phase, layout: Layout) -> Journal {
        Journal {
            txn: txn(),
            rescue: r"C:\Folio\.folio-update\7a7a\rescue\folio.exe".to_owned(),
            body: Body {
                phase,
                layout,
                adapter: Adapter::Ours,
            },
        }
    }

    fn trial_phase() -> Phase {
        Phase::Trial {
            nonce: nonce(TRIAL_NONCE),
            process: TRIAL,
            began_ms: BEGAN,
        }
    }

    fn receipt(txn: TxnId, nonce: Nonce) -> Receipt {
        Receipt {
            txn,
            nonce,
            pid: TRIAL.pid,
            version: "0.4.7".to_owned(),
            started: None,
        }
    }

    fn valid_receipt() -> Receipt {
        receipt(txn(), nonce(TRIAL_NONCE))
    }

    /// PIN (U-37, design revision (h) H.1 R2 and the rollout contract) — **a
    /// 0.4.7 receipt, `started` and all, is read by 0.4.6's receipt reader
    /// exactly as it always read one, and it commits a recorded trial by its
    /// nonce whatever `started` says; a receipt without `started` is written
    /// byte for byte as 0.4.6 wrote it.**
    ///
    /// In every update from 0.4.6 to any later version, the lock holder is the
    /// 0.4.6 rescue build: it must still commit the new trial it recorded. The
    /// fixture is `v0.4.6-preview`'s `ReceiptWire`, verbatim, with its reader's
    /// two steps (the version, then the fields); that road has no adoption and
    /// no `--from-trial` (a 0.4.7 trial hands nothing back to it:
    /// `update_apply_windows::tests::a_trial_hands_back_only_to_a_rescue_build_that_knows_the_word`).
    ///
    /// MUTATION: serialise `started` as `null` when it is `None` (drop
    /// `skip_serializing_if`), or version the receipt 2.
    #[test]
    fn a_0_4_6_reader_takes_a_0_4_7_receipt_as_it_always_did() {
        /// `v0.4.6-preview:crates/bt-app/src/update_txn.rs`, `ReceiptWire`.
        #[derive(Deserialize)]
        struct ReceiptWire046 {
            v: u64,
            txn: String,
            nonce: String,
            pid: u32,
            version: String,
        }
        let new = Receipt {
            started: Some(133_000_000_000_000_000),
            ..valid_receipt()
        };
        let bytes = new.encode();
        let old: ReceiptWire046 = serde_json::from_slice(&bytes).expect("0.4.6 reads it");
        assert_eq!(old.v, RECEIPT_VERSION);
        assert_eq!(RECEIPT_VERSION, 1);
        assert_eq!(
            (old.txn, old.nonce, old.pid, old.version),
            (
                new.txn.to_string(),
                new.nonce.to_string(),
                new.pid,
                new.version.clone()
            )
        );
        assert_eq!(
            next(&txn(), &trial_phase(), &Event::ReceiptAccepted(new.clone())),
            Ok(Phase::Committed),
            "a recorded trial commits by its nonce"
        );
        assert_eq!(Receipt::parse(&bytes), Ok(new));
        let without = valid_receipt();
        assert!(
            !String::from_utf8(without.encode())
                .unwrap()
                .contains("started")
        );
        assert_eq!(Receipt::parse(&without.encode()), Ok(without));
    }

    /// `v0.4.6-preview:crates/bt-app/src/update_txn.rs`, `Body`: the phase
    /// and the layout, and nothing else. Its two fields are read here with
    /// today's `Phase` and `Layout`, whose own additions since (U-42a's
    /// `untried`) follow the same rule and are pinned by their own tests.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Body046 {
        phase: Phase,
        layout: Layout,
    }

    /// The whole journal as 0.4.6 wrote it: the header's five fields and
    /// the body.
    #[derive(Serialize)]
    struct JournalWire046<'a> {
        #[serde(flatten)]
        header: HeaderWire046,
        body: &'a Body046,
    }

    /// The body as 0.4.6 read it (`BodyOnly`, verbatim).
    #[derive(Deserialize)]
    struct BodyOnly046 {
        body: Body046,
    }

    /// PIN (U-41a1, managed-update §1.3, U-37's H.1 rule for a v1
    /// document; E1) — **a journal 0.4.6 wrote, which names no adapter, reads
    /// as `Ours`, and 0.4.6 and 0.4.7 read an ordinary copy's journal as they
    /// always did**: its body is 0.4.6's bytes, and its header is 0.4.6's
    /// with the writer's version beside it (`written_by`, E1), a key neither
    /// of them knows.
    ///
    /// A 0.4.6 copy that is updated to 0.4.7 leaves a journal no adapter was
    /// recorded in, and every lock holder after it reads the adapter from the
    /// journal (R2): an absent field must be the road every existing
    /// transaction is on. And the body an ordinary copy writes stays 0.4.6's
    /// bytes, so nothing an ordinary update leaves on the disk changes with
    /// this field — the receipt's rule (`started`, U-37). Until E1 this pin
    /// said the whole journal was 0.4.6's bytes; the header now names its
    /// writer, and what holds is that 0.4.6 and 0.4.7 read it unchanged.
    ///
    /// MUTATION: drop `#[serde(default)]` from `Body::adapter` (the 0.4.6
    /// journal is refused as malformed), or drop its `skip_serializing_if`
    /// (an ordinary journal gains `"adapter":"Ours"`).
    #[test]
    fn a_journal_that_names_no_adapter_reads_as_ours_and_0_4_6_and_0_4_7_read_ours_as_ever() {
        for (phase, layout) in [
            (Phase::Allocated, members_layout()),
            (
                Phase::Prepared {
                    deferred_launches: 1,
                },
                bundle_layout(),
            ),
            (Phase::Moving, members_layout()),
            (
                Phase::Stuck {
                    trial: None,
                    trial_started: false,
                    last_error: "the move of `folio.exe` failed".to_owned(),
                    attempts: 2,
                    retrial: None,
                },
                bundle_layout(),
            ),
        ] {
            let ours = journal(phase.clone(), layout.clone());
            let old = Body046 { phase, layout };
            let header = ours.header();
            let header_046 = HeaderWire046 {
                v: 1,
                txn: header.txn,
                rescue: header.rescue,
                class: header.class.word().to_owned(),
                outcome: header.outcome.word().to_owned(),
            };
            let written_by_046 = serde_json::to_vec(&JournalWire046 {
                header: header_046,
                body: &old,
            })
            .unwrap();
            let read = Journal::parse(&written_by_046).expect("a 0.4.6 journal is read");
            assert_eq!(read.body.adapter, Adapter::Ours, "no adapter is ours");
            assert_eq!(read, ours);
            let ours_bytes = ours.encode();
            let BodyOnly046 { body } =
                serde_json::from_slice(&ours_bytes).expect("0.4.6 and 0.4.7 read the body");
            assert_eq!(body, old);
            assert_eq!(
                header_as_0_4_6_and_0_4_7_read_it(&ours_bytes),
                header_as_0_4_6_and_0_4_7_read_it(&written_by_046),
                "0.4.6 and 0.4.7 read the header as they always did"
            );
            let mut written: serde_json::Value = serde_json::from_slice(&ours_bytes).unwrap();
            let named = written
                .as_object_mut()
                .unwrap()
                .remove("written_by")
                .expect("the writer is named");
            assert_eq!(named, crate::version::VERSION);
            assert_eq!(
                written,
                serde_json::from_slice::<serde_json::Value>(&written_by_046).unwrap(),
                "beside the writer's name, an ordinary journal is 0.4.6's"
            );
        }
    }

    /// PIN (U-41a1, the same rule) — **a journal that names another adapter
    /// is read by 0.4.6's body reader exactly as it always read one, and by
    /// this build with its adapter; a body field this build does not know is
    /// ignored the same way.**
    ///
    /// The body is written and read by the rescue build's own version (F-8),
    /// so a 0.4.6 build never has to act on a journal a 0.4.7 road wrote; but
    /// the rule that lets fields be added to a v1 document without a version
    /// — every reader ignores what it does not know — is what keeps the
    /// header's readers (every later start) and the body's apart, and it has
    /// to hold in both directions: 0.4.6 reading this build's body, and this
    /// build reading a later one's.
    ///
    /// MUTATION: give `Body` `#[serde(deny_unknown_fields)]` — the body a
    /// later build wrote is refused.
    #[test]
    fn a_body_reader_ignores_an_adapter_or_any_field_it_does_not_know() {
        for adapter in [Adapter::Homebrew, Adapter::Scoop, Adapter::Winget] {
            let named = journal(
                Phase::Prepared {
                    deferred_launches: 0,
                },
                bundle_layout(),
            )
            .naming(adapter);
            let bytes = named.encode();
            let BodyOnly046 { body } = serde_json::from_slice(&bytes).expect("0.4.6 reads it");
            assert_eq!(
                body,
                Body046 {
                    phase: Phase::Prepared {
                        deferred_launches: 0
                    },
                    layout: bundle_layout(),
                },
                "{adapter:?}: 0.4.6 reads the phase and the layout as they are"
            );
            assert_eq!(Journal::parse(&bytes), Ok(named.clone()), "{adapter:?}");
            let advanced = named.advance(&Event::Discarded).unwrap();
            assert_eq!(
                advanced.body.adapter, adapter,
                "every later phase carries it"
            );
        }

        let ours = journal(Phase::Moving, members_layout());
        let mut later: serde_json::Value = serde_json::from_slice(&ours.encode()).unwrap();
        later["body"]["recorded_by_a_later_build"] = serde_json::json!({"说明": "未知字段"});
        let bytes = serde_json::to_vec(&later).unwrap();
        assert_eq!(
            Journal::parse(&bytes),
            Ok(ours),
            "a field this build does not know is not a refusal"
        );
    }

    /// The four folders a Windows member lives in, as the file system keeps
    /// them.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    struct FakeDisk {
        install: BTreeMap<String, Digest>,
        backup: BTreeMap<String, Digest>,
        set: BTreeMap<String, Digest>,
        rolledout: BTreeMap<String, Digest>,
    }

    fn files(members: &[Member]) -> BTreeMap<String, Digest> {
        members
            .iter()
            .map(|member| (member.name.clone(), member.digest))
            .collect()
    }

    impl FakeDisk {
        /// `Prepared`: the old install in place and the verified set staged.
        fn prepared(inventories: &Inventories) -> Self {
            Self {
                install: files(&inventories.old_present),
                set: files(&inventories.new),
                ..Self::default()
            }
        }

        fn place(&mut self, place: Place) -> &mut BTreeMap<String, Digest> {
            match place {
                Place::Install => &mut self.install,
                Place::Backup => &mut self.backup,
                Place::Set => &mut self.set,
                Place::RolledOut => &mut self.rolledout,
            }
        }

        /// A rename: the source must exist and the target must not.
        fn apply(&mut self, step: &Move) {
            let file = self
                .place(step.from)
                .remove(&step.name)
                .unwrap_or_else(|| panic!("no {:?} in {:?}", step.name, step.from));
            let previous = self.place(step.to).insert(step.name.clone(), file);
            assert!(previous.is_none(), "{step:?} would replace a file");
        }

        fn located(&self, inventories: &Inventories) -> Located {
            Located::Members(
                inventories
                    .names()
                    .into_iter()
                    .map(|name| Seen {
                        name: name.to_owned(),
                        install: self.install.get(name).copied(),
                        backup: self.backup.get(name).copied(),
                        unread: Vec::new(),
                    })
                    .collect(),
            )
        }

        /// The install holds exactly the old files, by digest.
        fn is_old_install(&self, inventories: &Inventories) -> bool {
            self.install == files(&inventories.old_present)
        }
    }

    /// A lock holder's disk: no entrance, no receipt, the trial gone, the
    /// clock at the trial's start.
    fn holder(journal: &Journal, located: Located) -> Disk<'_> {
        Disk {
            journal,
            asker: Asker::LockHolder,
            entrance: false,
            located,
            receipt: None,
            trial_alive: false,
            now_ms: BEGAN,
        }
    }

    fn job_owner(journal: &Journal, located: Located) -> Disk<'_> {
        Disk {
            asker: Asker::JobOwner,
            ..holder(journal, located)
        }
    }

    fn prepared_located() -> Located {
        FakeDisk::prepared(&inventories()).located(&inventories())
    }

    fn flipped() -> FakeDisk {
        let inventories = inventories();
        let mut disk = FakeDisk::prepared(&inventories);
        for step in inventories.forward_moves() {
            disk.apply(&step);
        }
        disk
    }

    fn bundle(live: Option<BundleIdentity>, stage: Option<BundleIdentity>) -> Located {
        Located::Bundle { live, stage }
    }

    fn start(journal: JournalRead, lock_free: bool) -> StartView {
        StartView {
            journal,
            lock_free,
            own_image: Some(digest(0x01)),
            rescue_image: Some(digest(0x01)),
            trial_of: None,
            sent_by_rollback: false,
        }
    }

    fn start_on(phase: Phase) -> StartView {
        start(
            JournalRead::Read(journal(phase, members_layout()).header()),
            true,
        )
    }

    /// **The rescue build driving a Windows rollback to its end**, the way
    /// U-24 will: ask, perform, record, ask again. The trial ends when asked
    /// to; a rollback that cannot finish stops the drive at `Stuck`.
    fn drive(mut journal: Journal, disk: &mut FakeDisk, mut entrance: bool) -> Journal {
        let inventories = inventories();
        let mut alive = matches!(
            journal.body.phase,
            Phase::RollbackIntent { trial: Some(_), .. } | Phase::Stuck { trial: Some(_), .. }
        );
        for _ in 0..64 {
            let view = Disk {
                entrance,
                trial_alive: alive,
                ..holder(&journal, disk.located(&inventories))
            };
            let event = match decide(&view) {
                Action::DeclareRollback => Event::RollbackDeclared,
                Action::StopTrial(_) => {
                    alive = false;
                    continue;
                }
                Action::RollBack(Restore::Moves(moves)) => {
                    moves.iter().for_each(|step| disk.apply(step));
                    continue;
                }
                Action::DeclareRolledBack => Event::RolledBack,
                Action::StayStuck { reason } => {
                    return journal
                        .advance(&Event::RollbackFailed { error: reason })
                        .expect("a failed rollback is recorded");
                }
                Action::FinishRollback { .. } => {
                    entrance = false;
                    Event::Retired
                }
                Action::Retire { .. } => return journal,
                other => panic!("the drive met {other:?} in {:?}", journal.body.phase),
            };
            journal = journal.advance(&event).expect("the decided event is legal");
        }
        panic!("the drive did not settle");
    }

    fn phase_samples() -> Vec<Phase> {
        vec![
            Phase::Allocated,
            Phase::Prepared {
                deferred_launches: 0,
            },
            Phase::Prepared {
                deferred_launches: 1,
            },
            Phase::Handoff {
                applier: nonce(0x44),
            },
            Phase::Armed,
            Phase::Moving,
            Phase::TrialStarting {
                nonce: nonce(TRIAL_NONCE),
                began_ms: BEGAN,
            },
            trial_phase(),
            Phase::Committed,
            Phase::RollbackIntent {
                trial: None,
                trial_started: false,
            },
            Phase::RollbackIntent {
                trial: Some(TRIAL),
                trial_started: false,
            },
            Phase::Stuck {
                trial: Some(TRIAL),
                trial_started: false,
                last_error: "a file is held open".to_owned(),
                attempts: 1,
                retrial: Some(Retrial {
                    nonce: nonce(TRIAL_NONCE),
                    began_ms: BEGAN,
                }),
            },
            Phase::RolledBack { untried: false },
            Phase::RolledBack { untried: true },
            Phase::Abandoned,
            Phase::Retired {
                outcome: Outcome::Committed,
                untried: false,
            },
            Phase::Retired {
                outcome: Outcome::RolledBack,
                untried: false,
            },
            Phase::Retired {
                outcome: Outcome::RolledBack,
                untried: true,
            },
        ]
    }

    /// RED (U-42a) — **a rollback no trial ever began is `RolledBack` and
    /// then `Retired` with `untried`; one after a trial, or a retrial over
    /// `Stuck`, is not; and a journal without the word reads as before.**
    ///
    /// The flag is what a start sent with `--update-failed` reads to say the
    /// update was interrupted rather than that the new version did not start
    /// (0.4.6's D-7). It is written only when true, so every journal an
    /// earlier build wrote — and every one after a trial — has the bytes it
    /// had.
    ///
    /// MUTATION: in `next`, answer `RolledBack { untried: false }` for
    /// `RollbackIntent` — the rollback from `Moving` is not untried.
    #[test]
    fn a_rollback_no_trial_began_is_retired_untried() {
        let retire = |from: Phase| {
            [Event::RollbackDeclared, Event::RolledBack, Event::Retired]
                .iter()
                .try_fold(journal(from, members_layout()), |journal, event| {
                    journal.advance(event)
                })
                .expect("the rollback's transitions")
        };
        assert_eq!(
            retire(Phase::Moving).body.phase,
            Phase::Retired {
                outcome: Outcome::RolledBack,
                untried: true,
            }
        );
        let after_trial = retire(trial_phase());
        assert_eq!(
            after_trial.body.phase,
            Phase::Retired {
                outcome: Outcome::RolledBack,
                untried: false,
            }
        );
        assert!(
            !String::from_utf8_lossy(&after_trial.encode()).contains("untried"),
            "a flag that is false is not written"
        );
        let stuck_retried = journal(
            Phase::Stuck {
                trial: None,
                trial_started: false,
                last_error: "held".to_owned(),
                attempts: 1,
                retrial: None,
            },
            members_layout(),
        )
        .advance(&Event::RetrialBegan {
            nonce: nonce(TRIAL_NONCE),
            process: TRIAL,
            began_ms: BEGAN,
        })
        .and_then(|journal| journal.advance(&Event::RolledBack))
        .expect("a retrial, then its rollback");
        assert_eq!(
            stuck_retried.body.phase,
            Phase::RolledBack { untried: false },
            "a retrial ran the new version"
        );
        let untried = retire(Phase::Moving).encode();
        let reread = Journal::parse(&untried).expect("the flag reads back");
        assert!(rolled_back_untried(&sight(&untried)));
        assert_eq!(reread.body.phase, retire(Phase::Moving).body.phase);
        let older = String::from_utf8(untried)
            .unwrap()
            .replace(",\"untried\":true", "");
        assert!(
            !rolled_back_untried(&sight(older.as_bytes())),
            "an earlier build's journal reads as a rollback after a trial: {older}"
        );
    }

    fn event_samples() -> Vec<Event> {
        vec![
            Event::Prepared,
            Event::PrepareFailed,
            Event::LaunchedWithoutResume,
            Event::Discarded,
            Event::HandedOff {
                applier: nonce(0x44),
            },
            Event::ApplierNotStarted,
            Event::OldStayed,
            Event::Unverified,
            armed(),
            Event::EntranceFailed,
            Event::Reverted,
            Event::Admitted,
            Event::TrialPlanned {
                nonce: nonce(TRIAL_NONCE),
                began_ms: BEGAN,
            },
            Event::TrialBegan {
                nonce: nonce(TRIAL_NONCE),
                process: TRIAL,
                began_ms: BEGAN,
            },
            Event::LastTrialReady {
                receipt: Receipt {
                    started: Some(TRIAL.started),
                    ..valid_receipt()
                },
                process: TRIAL,
            },
            Event::RetrialBegan {
                nonce: nonce(TRIAL_NONCE),
                process: TRIAL,
                began_ms: BEGAN,
            },
            Event::ReceiptAccepted(valid_receipt()),
            Event::RollbackDeclared,
            Event::RolledBack,
            Event::RollbackFailed {
                error: "a file is held open".to_owned(),
            },
            Event::Retired,
        ]
    }

    /// Every disk a holder or a job owner could be handed for `journal`: each
    /// instant of the flip, an install somebody else wrote into, both sides of
    /// the swap, with and without an entrance, each kind of receipt, the trial
    /// alive or gone, before and at its deadline.
    fn disks_for(journal: &Journal) -> Vec<Disk<'_>> {
        let locations: Vec<Located> = match &journal.body.layout {
            Layout::Members(inventories) => {
                let mut disk = FakeDisk::prepared(inventories);
                let mut all = vec![disk.located(inventories)];
                for step in inventories.forward_moves() {
                    disk.apply(&step);
                    all.push(disk.located(inventories));
                }
                disk.install.insert("folio.exe".to_owned(), digest(0xee));
                all.push(disk.located(inventories));
                all
            }
            Layout::Bundle { .. } | Layout::BundleIntent { .. } => {
                let sides = [None, Some(old_bundle()), Some(new_bundle())];
                let mut all = Vec::new();
                for live in &sides {
                    for stage in &sides {
                        all.push(bundle(live.clone(), stage.clone()));
                    }
                }
                all
            }
        };
        let receipts = [
            None,
            Some(valid_receipt()),
            Some(receipt(txn(), nonce(0x99))),
            Some(receipt(TxnId::new([0x01; 16]), nonce(TRIAL_NONCE))),
        ];
        let mut disks = Vec::new();
        for asker in [Asker::JobOwner, Asker::LockHolder, Asker::Rescue] {
            for located in &locations {
                for entrance in [false, true] {
                    for receipt in &receipts {
                        for trial_alive in [false, true] {
                            for now_ms in [BEGAN, BEGAN + TRIAL_DEADLINE_MS] {
                                disks.push(Disk {
                                    journal,
                                    asker,
                                    entrance,
                                    located: located.clone(),
                                    receipt: receipt.clone(),
                                    trial_alive,
                                    now_ms,
                                });
                            }
                        }
                    }
                }
            }
        }
        disks
    }

    fn journals() -> Vec<Journal> {
        phase_samples()
            .into_iter()
            .flat_map(|phase| {
                [
                    journal(phase.clone(), members_layout()),
                    journal(phase, bundle_layout()),
                ]
            })
            .collect()
    }

    // ───────────────────────────── the frozen formats ─────────────────────────────

    /// RED (U-10) — **header v1 round-trips, alone and inside a journal, and a
    /// start reads it without understanding the body.**
    ///
    /// The header is the one part of the journal every later version reads
    /// (F-8), so it has to survive its own encoding, the journal's, and a body
    /// written by a build this one has never met.
    #[test]
    fn header_v1_round_trips_alone_and_inside_a_journal() {
        for journal in journals() {
            let header = journal.header();
            assert_eq!(Header::parse(&header.encode()), Ok(header.clone()));
            assert_eq!(Header::parse(&journal.encode()), Ok(header));
            assert_eq!(Journal::parse(&journal.encode()), Ok(journal));
        }
        let mut future =
            serde_json::to_value(journal(Phase::Armed, members_layout()).header().wire())
                .expect("a header is a value");
        future["body"] = serde_json::json!({ "shape": "no build has written yet" });
        let bytes = serde_json::to_vec(&future).expect("bytes");
        assert_eq!(
            Header::parse(&bytes).map(|header| header.class),
            Ok(Class::Destructive)
        );
    }

    /// RED (U-10) — **every truncation of a journal is refused as a truncation,
    /// never read as a shorter header.**
    ///
    /// A journal is replaced by an atomic rename, so a short one is damage, and
    /// a start has to be able to say so rather than misread it.
    ///
    /// MUTATION: map every JSON error to `Malformed` in `json`, and each prefix
    /// is refused under the wrong name.
    #[test]
    fn a_truncated_header_is_refused_as_truncated() {
        let bytes = journal(trial_phase(), members_layout()).encode();
        for cut in 0..bytes.len() {
            assert_eq!(
                Header::parse(&bytes[..cut]),
                Err(ParseRefusal::Truncated),
                "cut at {cut} of {}",
                bytes.len()
            );
        }
    }

    /// RED (U-10) — **a header of another version is refused by that version,
    /// whatever else it holds.**
    ///
    /// A v2 header need not have v1's fields at all; the version is read
    /// first, so the refusal names the version rather than a missing field.
    #[test]
    fn a_header_of_an_unknown_version_is_refused_by_its_version() {
        assert_eq!(
            Header::parse(br#"{"v":2,"something":"else"}"#),
            Err(ParseRefusal::UnknownVersion(2))
        );
        let mut bytes = journal(Phase::Armed, members_layout()).encode();
        let at = bytes
            .windows(5)
            .position(|window| window == br#""v":1"#)
            .expect("the version");
        bytes[at + 4] = b'0';
        assert_eq!(Header::parse(&bytes), Err(ParseRefusal::UnknownVersion(0)));
    }

    /// RED (U-10) — **a class that is none of the four, and a class that is
    /// not its phase's, are refused by name.**
    #[test]
    fn a_header_class_that_is_unknown_or_disagrees_with_its_phase_is_refused() {
        let unknown = format!(
            r#"{{"v":1,"txn":"{}","rescue":"r","class":"paused","outcome":"none"}}"#,
            txn()
        );
        assert_eq!(
            Header::parse(unknown.as_bytes()),
            Err(ParseRefusal::UnknownClass("paused".to_owned()))
        );
        let stuck = journal(
            Phase::Stuck {
                trial: None,
                trial_started: false,
                last_error: String::new(),
                attempts: 1,
                retrial: None,
            },
            members_layout(),
        );
        let mut value: serde_json::Value = serde_json::from_slice(&stuck.encode()).expect("json");
        value["class"] = serde_json::json!("terminal");
        assert_eq!(
            Journal::parse(&serde_json::to_vec(&value).expect("bytes")),
            Err(ParseRefusal::ClassDisagreesWithPhase {
                class: Class::Terminal,
                phase: PhaseKind::Stuck,
            })
        );
    }

    /// RED (U-13) — **the header's outcome is the phase's decision**: `none`
    /// until one, `committed` from `Committed` on (its retirement included),
    /// `rolled_back` from `RollbackIntent` on; an unknown word, or one that is
    /// not its phase's, is refused by name.
    ///
    /// The class cannot carry this, and the trial reads only the header
    /// (coordinator ruling, 2026-09-27), so the lock holder writes it with the
    /// phase and a start checks it against the body it can read.
    ///
    /// MUTATION: in `PhaseKind::outcome`, answer `None` for `RollbackIntent`.
    #[test]
    fn a_header_outcome_follows_its_phase_and_is_refused_by_name_otherwise() {
        let expected = |phase: &Phase| match phase {
            Phase::Committed
            | Phase::Retired {
                outcome: Outcome::Committed,
                ..
            } => HeaderOutcome::Committed,
            Phase::RollbackIntent { .. }
            | Phase::Stuck { .. }
            | Phase::RolledBack { .. }
            | Phase::Retired {
                outcome: Outcome::RolledBack,
                ..
            } => HeaderOutcome::RolledBack,
            _ => HeaderOutcome::None,
        };
        for journal in journals() {
            let header = Header::parse(&journal.encode()).expect("a header");
            assert_eq!(
                header.outcome,
                expected(&journal.body.phase),
                "{:?}",
                journal.body.phase
            );
        }
        let unknown = format!(
            r#"{{"v":1,"txn":"{}","rescue":"r","class":"destructive","outcome":"maybe"}}"#,
            txn()
        );
        assert_eq!(
            Header::parse(unknown.as_bytes()),
            Err(ParseRefusal::UnknownOutcome("maybe".to_owned()))
        );
        let intent = journal(
            Phase::RollbackIntent {
                trial: None,
                trial_started: false,
            },
            members_layout(),
        );
        let mut value: serde_json::Value = serde_json::from_slice(&intent.encode()).expect("json");
        value["outcome"] = serde_json::json!("committed");
        assert_eq!(
            Journal::parse(&serde_json::to_vec(&value).expect("bytes")),
            Err(ParseRefusal::OutcomeDisagreesWithPhase {
                outcome: HeaderOutcome::Committed,
                phase: PhaseKind::RollbackIntent,
            })
        );
    }

    /// RED (U-10) — **receipt v1 round-trips, and its file name is the trial's
    /// nonce.**
    #[test]
    fn receipt_v1_round_trips_under_its_nonce() {
        let receipt = valid_receipt();
        assert_eq!(Receipt::parse(&receipt.encode()), Ok(receipt.clone()));
        assert_eq!(
            Receipt::file_name(&receipt.nonce),
            format!("health-{}", "33".repeat(32))
        );
    }

    /// RED (U-10) — **a receipt cut short, of another version, or with a
    /// fixed-length field of the wrong length or alphabet is refused by name.**
    ///
    /// The brief's "wrong digest length" is the nonce here: (b).1 F-8 freezes
    /// the receipt as `{v, txn, nonce, pid, version}`, and the nonce is its
    /// digest-length field.
    #[test]
    fn a_receipt_truncated_versioned_or_of_the_wrong_length_is_refused() {
        let bytes = valid_receipt().encode();
        for cut in 0..bytes.len() {
            assert_eq!(Receipt::parse(&bytes[..cut]), Err(ParseRefusal::Truncated));
        }
        let with = |txn: &str, nonce: &str, v: u64| {
            format!(r#"{{"v":{v},"txn":"{txn}","nonce":"{nonce}","pid":1,"version":"0.4.7"}}"#)
        };
        let good_txn = txn().to_string();
        let good_nonce = nonce(TRIAL_NONCE).to_string();
        assert_eq!(
            Receipt::parse(with(&good_txn, &good_nonce, 2).as_bytes()),
            Err(ParseRefusal::UnknownVersion(2))
        );
        assert_eq!(
            Receipt::parse(with(&good_txn, &good_nonce[2..], 1).as_bytes()),
            Err(ParseRefusal::WrongLength {
                field: "nonce",
                expected: 64,
                found: 62,
            })
        );
        assert_eq!(
            Receipt::parse(with(&good_txn[..30], &good_nonce, 1).as_bytes()),
            Err(ParseRefusal::WrongLength {
                field: "txn",
                expected: 32,
                found: 30,
            })
        );
        assert_eq!(
            Receipt::parse(with(&good_txn.to_uppercase(), &good_nonce, 1).as_bytes()),
            Err(ParseRefusal::NotHex { field: "txn" })
        );
    }

    // ───────────────────────────── transitions and rights ─────────────────────────────

    /// RED (U-10) — **every (phase, event) pair is a listed transition or a
    /// listed refusal, and every listed transition is reached.**
    ///
    /// `next` is a `match` and `TRANSITIONS` is a table; this walks every pair
    /// through the first and holds it to the second, so neither can grow an
    /// outcome the other does not know.
    #[test]
    fn every_phase_and_event_pair_is_a_listed_transition_or_a_listed_refusal() {
        let phases = phase_samples();
        let events = event_samples();
        let phase_kinds: BTreeSet<String> =
            phases.iter().map(|p| format!("{:?}", p.kind())).collect();
        assert_eq!(
            phase_kinds.len(),
            PhaseKind::ALL.len(),
            "a sample of every phase"
        );
        let event_kinds: BTreeSet<String> =
            events.iter().map(|e| format!("{:?}", e.kind())).collect();
        assert_eq!(
            event_kinds.len(),
            EventKind::ALL.len(),
            "a sample of every event"
        );
        let mut reached = Vec::new();
        for phase in &phases {
            for event in &events {
                let (from, kind) = (phase.kind(), event.kind());
                match next(&txn(), phase, event) {
                    Ok(to) => {
                        assert!(
                            TRANSITIONS.contains(&(from, kind, to.kind())),
                            "{from:?} + {kind:?} → {:?} is not listed",
                            to.kind()
                        );
                        reached.push((from, kind, to.kind()));
                    }
                    Err(refusal) => {
                        assert!(
                            !TRANSITIONS.iter().any(|(f, e, _)| (*f, *e) == (from, kind)),
                            "{from:?} + {kind:?} is listed yet refused: {refusal:?}"
                        );
                        let named = NAMED_REFUSALS
                            .iter()
                            .find(|(f, e, _)| (*f, *e) == (from, kind))
                            .map(|(_, _, refusal)| refusal.clone());
                        assert_eq!(
                            refusal,
                            named.unwrap_or(Refusal::Illegal { from, event: kind })
                        );
                    }
                }
            }
        }
        for listed in TRANSITIONS {
            assert!(reached.contains(listed), "{listed:?} is never reached");
        }
    }

    /// RED (U-10) — **every phase is recorded only by an actor the writers
    /// table names for it, and every named writer records something.**
    ///
    /// (b).2: O writes `Allocated`, `Prepared`, `Handoff` and `Abandoned`; the
    /// lock holder the rest and the revert; N writes no phase except U-35's
    /// exact reserved trial writing `Committed` from its own readiness
    /// receipt. R writes `Trial` since U-29b: it starts N over an exchange a
    /// dead applier left with the new bundle live (the coordinator's ruling
    /// 1).
    #[test]
    fn every_phase_is_recorded_only_by_the_writers_the_table_names() {
        // The one phase no transition writes is the first: O creates the
        // journal at `Allocated` (`Journal::allocate`).
        let mut used = BTreeSet::from([format!("{:?} {:?}", Actor::Old, PhaseKind::Allocated)]);
        for (_, event, to) in TRANSITIONS {
            for actor in event.authors() {
                assert!(
                    may_record(*actor, *to),
                    "{actor:?} records {to:?} by {event:?}"
                );
                used.insert(format!("{actor:?} {to:?}"));
            }
        }
        for (phase, writers) in JOURNAL_WRITERS {
            for actor in *writers {
                assert!(
                    used.contains(&format!("{actor:?} {phase:?}")),
                    "{actor:?} never records {phase:?}"
                );
            }
        }
        for phase in PhaseKind::ALL {
            assert_eq!(
                may_record(Actor::Trial, phase),
                phase == PhaseKind::Committed,
                "N's only journal row is U-35's commit: {phase:?}"
            );
            assert!(
                !may_record(Actor::Start, phase),
                "a start records {phase:?}"
            );
        }
        assert!(may_record(Actor::Recovery, PhaseKind::Trial));
        let trial_rights: Vec<(Effect, &[PhaseKind])> = EFFECT_RIGHTS
            .iter()
            .filter(|right| right.actor == Actor::Trial)
            .map(|right| (right.effect, right.during))
            .collect();
        assert_eq!(
            trial_rights,
            vec![(
                Effect::WriteReceipt,
                &[PhaseKind::TrialStarting, PhaseKind::Trial][..]
            )],
            "N writes its receipt in the reserved or recorded trial phase"
        );
    }

    /// RED (U-35 round 2, the review's B2) — **`TrialStarting`'s whole effect
    /// table, every actor × every effect**: its trial writes its receipt, and
    /// the recovery ends a handed-back instance of it that never became ready
    /// (H.3 step 2). Nothing else — no move, swap, entrance, deletion or
    /// cleanup before the phase becomes `Trial`, `Committed` or
    /// `RollbackIntent`, whose own rows govern from there; and no applier
    /// right, since no applier ever holds the phase.
    ///
    /// MUTATION: drop `TrialStarting` from Recovery's `EndTrial` row (the
    /// review's B2: an unready last trial is never ended); or give the applier
    /// that row back, or any other cell.
    #[test]
    fn u35_trial_starting_effect_rights_are_exhaustive() {
        let mut actual = Vec::new();
        for actor in [
            Actor::Old,
            Actor::Applier,
            Actor::Recovery,
            Actor::Trial,
            Actor::Start,
        ] {
            for effect in [
                Effect::WriteReceipt,
                Effect::WriteEntrance,
                Effect::RemoveEntrance,
                Effect::MoveOldOut,
                Effect::MoveNewIn,
                Effect::MoveNewOut,
                Effect::MoveOldBack,
                Effect::Swap,
                Effect::EndTrial,
                Effect::DeleteRollbackMaterial,
                Effect::DetachMount,
                Effect::DeleteTxnDir,
                Effect::DeleteJournal,
            ] {
                if may(actor, effect, PhaseKind::TrialStarting) {
                    actual.push((actor, effect));
                }
            }
        }
        assert_eq!(
            actual,
            vec![
                (Actor::Recovery, Effect::EndTrial),
                (Actor::Trial, Effect::WriteReceipt),
            ]
        );
    }

    /// RED (U-10) — **every action `decide` hands out, and every action a
    /// start takes, is within its actor's rights in that phase.**
    ///
    /// The rights table is data only if something is held to it: this holds
    /// every answer over every disk `disks_for` can build.
    #[test]
    fn every_decided_action_is_within_its_actors_rights() {
        for journal in journals() {
            let phase = journal.body.phase.kind();
            for disk in disks_for(&journal) {
                let action = decide(&disk);
                let actor = disk.asker.actor(phase);
                for effect in action.effects() {
                    assert!(
                        may(actor, effect, phase),
                        "{actor:?} may not {effect:?} in {phase:?} ({action:?})"
                    );
                }
            }
            for lock_free in [false, true] {
                for rescue_image in [None, Some(digest(0x01)), Some(digest(0x02))] {
                    for trial_of in [None, Some(txn())] {
                        let view = StartView {
                            rescue_image,
                            trial_of,
                            ..start(JournalRead::Read(journal.header()), lock_free)
                        };
                        for effect in at_start(&view).effects() {
                            assert!(
                                may(Actor::Start, effect, phase),
                                "a start may not {effect:?} in {phase:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    // ───────────────────────────── the four rules ─────────────────────────────

    /// RED (U-10) — **nothing but a receipt naming this transaction and this
    /// trial's nonce, held by the lock holder while the journal says `Trial`,
    /// ever becomes `Committed`.**
    ///
    /// F-1: rev 2 let recovery "write health" because it merely *was* the new
    /// version, and deleted the rollback source on that. Here the only roads
    /// to `Committed` are `ReceiptAccepted` in `Trial`, and in a `Stuck` whose
    /// holder started a trial over it (U-29b, ruling 3) — each on a receipt
    /// with that trial's own nonce — and, since U-35, `LastTrialReady` in
    /// `TrialStarting`, recorded by that reserved trial on its own receipt;
    /// and `decide` answers `Commit` only where `next` would accept the
    /// receipt.
    ///
    /// MUTATION: delete the `receipt.nonce != *nonce` arm of `next`, and a
    /// receipt from another trial commits.
    #[test]
    fn recovery_never_writes_committed_without_a_receipt() {
        let into_committed: Vec<_> = TRANSITIONS
            .iter()
            .filter(|(_, _, to)| *to == PhaseKind::Committed)
            .collect();
        assert_eq!(
            into_committed,
            vec![
                &(
                    PhaseKind::TrialStarting,
                    EventKind::LastTrialReady,
                    PhaseKind::Committed
                ),
                &(
                    PhaseKind::Trial,
                    EventKind::ReceiptAccepted,
                    PhaseKind::Committed
                ),
                &(
                    PhaseKind::Stuck,
                    EventKind::ReceiptAccepted,
                    PhaseKind::Committed
                ),
            ]
        );
        assert_eq!(
            next(
                &txn(),
                &trial_phase(),
                &Event::ReceiptAccepted(receipt(txn(), nonce(0x99)))
            ),
            Err(Refusal::ReceiptForAnotherTrial)
        );
        assert_eq!(
            next(
                &txn(),
                &trial_phase(),
                &Event::ReceiptAccepted(receipt(TxnId::new([0x01; 16]), nonce(TRIAL_NONCE)))
            ),
            Err(Refusal::ReceiptForAnotherTransaction)
        );
        for journal in journals() {
            for disk in disks_for(&journal) {
                let committed = decide(&disk) == Action::Commit;
                let entitled = matches!(
                    journal.body.phase,
                    Phase::Trial { .. }
                        | Phase::Stuck {
                            retrial: Some(_),
                            ..
                        }
                ) && disk.asker != Asker::JobOwner
                    && disk.receipt == Some(valid_receipt());
                assert_eq!(
                    committed, entitled,
                    "{:?} with {:?} from {:?}",
                    journal.body.phase, disk.receipt, disk.asker
                );
            }
        }
    }

    /// RED (U-10) — **once `RollbackIntent` is durable, a receipt changes
    /// nothing: the transition refuses it and the holder keeps rolling back.**
    ///
    /// F-1 and F-14: N's health report and P's deadline race, and the one lock
    /// holder settles the race by the order its own writes landed in.
    ///
    /// MUTATION: let the `RollbackIntent | Stuck` receipt arm of `next` answer
    /// `Ok(Phase::Committed)`, and a late receipt overturns the rollback.
    #[test]
    fn a_receipt_after_rollback_intent_is_ignored() {
        for phase in [
            Phase::RollbackIntent {
                trial: Some(TRIAL),
                trial_started: false,
            },
            Phase::Stuck {
                trial: None,
                trial_started: false,
                last_error: String::new(),
                attempts: 1,
                retrial: None,
            },
        ] {
            assert_eq!(
                next(&txn(), &phase, &Event::ReceiptAccepted(valid_receipt())),
                Err(Refusal::ReceiptAfterRollbackIntent)
            );
            let journal = journal(phase, members_layout());
            let disk = Disk {
                receipt: Some(valid_receipt()),
                ..holder(&journal, flipped().located(&inventories()))
            };
            let action = decide(&disk);
            assert!(matches!(action, Action::RollBack(_)), "{action:?}");
        }
    }

    /// RED (U-10) — **`Stuck` keeps its journal, its rollback material and its
    /// entrance: no answer to it deletes any of them, and no start retires it.**
    ///
    /// (b).2 W10: a rollback that could not finish is retried at every logon
    /// and every start, which needs all three. Rev 2 removed the journal of a
    /// failed rollback (F-1).
    ///
    /// MUTATION: class `Stuck` as `Terminal` in `PhaseKind::class`, and a start
    /// retires it — journal, backup and entrance.
    #[test]
    fn stuck_keeps_journal_backup_and_entrance() {
        let forbidden = [
            Effect::DeleteJournal,
            Effect::DeleteTxnDir,
            Effect::DeleteRollbackMaterial,
            Effect::RemoveEntrance,
        ];
        for layout in [members_layout(), bundle_layout()] {
            let journal = journal(
                Phase::Stuck {
                    trial: Some(TRIAL),
                    trial_started: false,
                    last_error: "a file is held open".to_owned(),
                    attempts: 1,
                    retrial: None,
                },
                layout,
            );
            for disk in disks_for(&journal) {
                let action = decide(&disk);
                for effect in action.effects() {
                    assert!(!forbidden.contains(&effect), "{action:?} in Stuck");
                }
            }
            let view = start(JournalRead::Read(journal.header()), true);
            assert_eq!(at_start(&view), StartAction::HandToRescue);
        }
        let stuck = journal(
            Phase::RollbackIntent {
                trial: None,
                trial_started: false,
            },
            members_layout(),
        )
        .advance(&Event::RollbackFailed {
            error: "a file is held open".to_owned(),
        })
        .expect("recorded");
        assert_eq!(stuck.body.phase.kind(), PhaseKind::Stuck);
    }

    /// RED (U-10) — **a transaction O has handed off is applied by whoever next
    /// holds the lock, and swept by nobody.**
    ///
    /// F-6: after O exits, a manual start could win the lock, read what looked
    /// like disposable preparation and delete P's payload. `Handoff` is
    /// destructive to a start, left alone by a job owner, and applied by a lock
    /// holder.
    ///
    /// MUTATION: add `| Phase::Handoff { .. }` to the job owner's
    /// `Phase::Allocated` arm in `decide`, and a job owner sweeps the payload.
    #[test]
    fn handoff_is_applied_never_swept() {
        for layout in [members_layout(), bundle_layout()] {
            let journal = journal(
                Phase::Handoff {
                    applier: nonce(0x44),
                },
                layout,
            );
            let located = match &journal.body.layout {
                Layout::Members(_) => prepared_located(),
                Layout::Bundle { .. } | Layout::BundleIntent { .. } => {
                    bundle(Some(old_bundle()), Some(new_bundle()))
                }
            };
            assert_eq!(decide(&holder(&journal, located.clone())), Action::Apply);
            assert_eq!(decide(&job_owner(&journal, located)), Action::Leave);
            for disk in disks_for(&journal) {
                let effects = decide(&disk).effects();
                assert!(!effects.contains(&Effect::DeleteTxnDir), "{:?}", disk.asker);
                assert!(
                    !effects.contains(&Effect::DeleteJournal),
                    "{:?}",
                    disk.asker
                );
            }
            let view = start(JournalRead::Read(journal.header()), true);
            assert_eq!(at_start(&view), StartAction::HandToRescue);
        }
    }

    // ───────────────────────────── (b).2, the Windows table ─────────────────────────────

    /// RED (U-10) — **W1: a preparation that died is swept by the next job
    /// owner holding the lock; the rescue leaves it, and a start continues.**
    #[test]
    fn w1_an_allocated_transaction_is_swept_by_its_job_owner() {
        let journal = Journal::allocate(txn(), "rescue".to_owned(), members_layout());
        assert_eq!(journal.body.phase, Phase::Allocated);
        assert_eq!(
            decide(&job_owner(&journal, prepared_located())),
            Action::Sweep
        );
        assert_eq!(decide(&holder(&journal, prepared_located())), Action::Leave);
        assert_eq!(at_start(&start_on(Phase::Allocated)), StartAction::Continue);
    }

    /// RED (U-10) — **W2: a prepared transaction is nobody's at start but its
    /// job owner's; it survives its first launch without a resume and is
    /// discarded at the second.**
    #[test]
    fn w2_a_prepared_transaction_survives_one_launch_and_is_discarded_at_the_second() {
        let first = journal(
            Phase::Prepared {
                deferred_launches: 0,
            },
            members_layout(),
        );
        assert_eq!(
            at_start(&start_on(first.body.phase.clone())),
            StartAction::Continue
        );
        assert_eq!(decide(&holder(&first, prepared_located())), Action::Leave);
        assert_eq!(
            decide(&job_owner(&first, prepared_located())),
            Action::CountDeferredLaunch
        );
        let second = first
            .advance(&Event::LaunchedWithoutResume)
            .expect("counted");
        assert_eq!(
            second.body.phase,
            Phase::Prepared {
                deferred_launches: 1
            }
        );
        let third = second
            .advance(&Event::LaunchedWithoutResume)
            .expect("counted");
        assert_eq!(third.body.phase, Phase::Abandoned);
    }

    /// RED (U-10) — **W3: a handoff is applied by the first lock holder, and a
    /// refused admission puts it back to `Prepared`.**
    #[test]
    fn w3_a_handoff_is_applied_or_reverted_on_a_refused_admission() {
        let journal = journal(
            Phase::Handoff {
                applier: nonce(0x44),
            },
            members_layout(),
        );
        assert_eq!(decide(&holder(&journal, prepared_located())), Action::Apply);
        assert_eq!(Asker::LockHolder.actor(PhaseKind::Handoff), Actor::Applier);
        assert_eq!(
            journal.advance(&armed()).map(|j| j.body.phase),
            Ok(Phase::Armed)
        );
        assert_eq!(
            journal.advance(&Event::Reverted).map(|j| j.body.phase),
            Ok(Phase::Prepared {
                deferred_launches: 0
            })
        );
    }

    /// RED (U-22) — **`Armed` is recorded only with the entrance's proof, and
    /// only the proof of this journal's own transaction.**
    ///
    /// F-2: the entrance is written, flushed and read back **before** the
    /// journal records `Armed`. [`Event::Armed`] carries
    /// `bt_platform::logon_hook::Armed`, which only `logon_hook::arm` makes
    /// after its read-back; a proof made for another transaction is refused,
    /// so an entrance another transaction left cannot stand in for this one's.
    ///
    /// MUTATION: in `next`, answer `Ok(Phase::Armed)` for any proof.
    #[test]
    fn armed_is_recorded_only_with_the_entrance_of_its_own_transaction() {
        let journal = journal(
            Phase::Handoff {
                applier: nonce(0x44),
            },
            members_layout(),
        );
        assert_eq!(
            journal.advance(&armed()).map(|j| j.body.phase),
            Ok(Phase::Armed)
        );
        let another = Event::Armed(armed_for(TxnId::new([0x11; 16])));
        assert_eq!(
            journal.advance(&another).map(|j| j.body.phase),
            Err(Refusal::EntranceForAnotherTransaction)
        );
        assert_eq!(
            journal
                .advance(&Event::EntranceFailed)
                .map(|j| j.body.phase),
            Ok(Phase::Abandoned),
            "an entrance that could not be made abandons the transaction"
        );
    }

    /// RED (U-26) — **the macOS entrance's proof arms the journal exactly as
    /// the Windows one does: one proof type, bound to its transaction.**
    ///
    /// F-3: the LaunchAgent plist is `F_FULLFSYNC`'d (file and folder) before
    /// the journal records `Armed`. `launch_agent::arm` answers the same
    /// `install_txn::Armed` that `logon_hook::arm` does, so the one event
    /// carries either and the transaction binding holds for both. The plist is
    /// written into a temporary folder standing in for `~/Library/LaunchAgents`.
    ///
    /// MUTATION: in `next`, answer `Ok(Phase::Armed)` for any proof.
    #[test]
    fn a_launch_agent_proof_arms_the_journal_too() {
        if bt_platform::host_platform() != HostPlatform::MacOs {
            return;
        }
        let agents = bt_testpath::temp_path("bt-u26-armed");
        let _ = std::fs::remove_dir_all(&agents);
        std::fs::create_dir_all(&agents).unwrap();
        let home = Path::new("/Applications/.Folio.app.folio-update");
        let rescue = home.join("rescue/Folio.app/Contents/MacOS/folio");
        let arm = |id: TxnId| bt_platform::launch_agent::arm(&agents, id.bytes(), &rescue, home);
        let journal = journal(
            Phase::Handoff {
                applier: nonce(0x44),
            },
            bundle_layout(),
        );
        assert_eq!(
            journal
                .advance(&Event::Armed(arm(txn()).unwrap()))
                .map(|j| j.body.phase),
            Ok(Phase::Armed)
        );
        assert_eq!(
            journal
                .advance(&Event::Armed(arm(TxnId::new([0x11; 16])).unwrap()))
                .map(|j| j.body.phase),
            Err(Refusal::EntranceForAnotherTransaction)
        );
        let _ = std::fs::remove_dir_all(&agents);
    }

    /// PIN (U-26) — **the proof has exactly two makers: the Windows entrance's
    /// `logon_hook::arm_in` and the macOS entrance's `launch_agent::arm_with`**,
    /// each after its read-back. `install_txn::Armed::proved` is crate-visible
    /// in `bt-platform`, so this is what keeps a third door from minting one.
    #[test]
    fn only_the_two_entrance_doors_make_the_armed_proof() {
        use bt_source::{Index, Pattern, Search, View, needle};
        let platform = Index::of_package("bt-platform");
        let made = platform
            .search(&Search::new(
                needle!(Pattern::text("Armed::proved(")),
                View::CodeKeepingLiterals,
            ))
            .unwrap_or_else(|failure| panic!("{failure}"))
            .in_the_product(platform);
        let mut owners: Vec<String> = made
            .owners(platform)
            .into_keys()
            .map(|identity| identity.name)
            .collect();
        owners.sort();
        assert_eq!(
            owners,
            vec![String::from("arm_in"), String::from("arm_with")],
            "{}",
            made.report(platform)
        );
    }

    /// RED (U-10) — **W4: an entrance found beside a `Handoff` is a dead
    /// attempt: it is removed and the restart reverts, with nothing moved.**
    #[test]
    fn w4_an_entrance_left_by_a_dead_handoff_is_removed_and_the_restart_reverts() {
        let journal = journal(
            Phase::Handoff {
                applier: nonce(0x44),
            },
            members_layout(),
        );
        let disk = Disk {
            entrance: true,
            ..holder(&journal, prepared_located())
        };
        let action = decide(&disk);
        assert_eq!(
            action,
            Action::Revert {
                remove_entrance: true
            }
        );
        assert_eq!(action.effects(), vec![Effect::RemoveEntrance]);
    }

    /// RED (U-10) — **W5: `Armed` is admitted into `Moving`, or reverted to
    /// `Prepared` when admission or the process check refuses.**
    #[test]
    fn w5_armed_is_admitted_into_moving_or_reverted() {
        let journal = journal(Phase::Armed, members_layout());
        assert_eq!(decide(&holder(&journal, prepared_located())), Action::Admit);
        assert_eq!(
            journal.advance(&Event::Admitted).map(|j| j.body.phase),
            Ok(Phase::Moving)
        );
        assert_eq!(
            journal.advance(&Event::Reverted).map(|j| j.body.phase),
            Ok(Phase::Prepared {
                deferred_launches: 0
            })
        );
    }

    /// RED (U-10) — **W6: a death anywhere inside `Moving` is rolled back, never
    /// rolled forward, and the rollback restores the old install exactly.**
    ///
    /// One case per instant of the flip: after each of `forward_moves`'
    /// renames, including none and all.
    #[test]
    fn w6_a_death_anywhere_inside_moving_rolls_back_to_the_old_install() {
        let inventories = inventories();
        let moves = inventories.forward_moves();
        for done in 0..=moves.len() {
            let mut disk = FakeDisk::prepared(&inventories);
            moves[..done].iter().for_each(|step| disk.apply(step));
            let journal = journal(Phase::Moving, members_layout());
            let view = holder(&journal, disk.located(&inventories));
            assert_eq!(decide(&view), Action::DeclareRollback, "after {done} moves");
            let end = drive(journal.clone(), &mut disk, true);
            assert_eq!(
                end.body.phase,
                Phase::Retired {
                    outcome: Outcome::RolledBack,
                    untried: true,
                },
                "after {done} moves"
            );
            assert!(
                disk.is_old_install(&inventories),
                "after {done} moves: {disk:?}"
            );
        }
    }

    /// RED (U-35, round 2) — **the last trial is reserved durably before
    /// launch, and leaves `TrialStarting` three ways only**: a holder adopts
    /// the reserved nonce's process into `Trial`; the reserved trial commits
    /// itself on a receipt of this transaction, its nonce and its own process
    /// (B3); or recovery declares a rollback, which is never read as
    /// "untried" (m1). This is one pre-launch state, not another launcher or
    /// a retry loop.
    ///
    /// MUTATION: leave the reservation in `Moving`, accept another nonce,
    /// begin a fresh trial when recovery finds `TrialStarting`, drop the
    /// process check from `LastTrialReady`, or record `trial_started: false`
    /// over `TrialStarting`.
    #[test]
    fn u35_the_last_trial_is_reserved_then_adopted_or_rolled_back() {
        let moving = journal(Phase::Moving, members_layout());
        let planned = moving
            .advance(&Event::TrialPlanned {
                nonce: nonce(TRIAL_NONCE),
                began_ms: BEGAN,
            })
            .expect("the reservation");
        assert_eq!(
            planned.body.phase,
            Phase::TrialStarting {
                nonce: nonce(TRIAL_NONCE),
                began_ms: BEGAN,
            }
        );
        assert_eq!(
            planned.advance(&Event::TrialBegan {
                nonce: nonce(0x99),
                process: TRIAL,
                began_ms: BEGAN + 1,
            }),
            Err(Refusal::ReceiptForAnotherTrial)
        );
        let trial = planned
            .advance(&Event::TrialBegan {
                nonce: nonce(TRIAL_NONCE),
                process: TRIAL,
                began_ms: BEGAN + 1,
            })
            .expect("the exact reserved trial");
        assert_eq!(
            trial.body.phase,
            Phase::Trial {
                nonce: nonce(TRIAL_NONCE),
                process: TRIAL,
                began_ms: BEGAN + 1,
            }
        );
        assert_eq!(
            trial
                .advance(&Event::ReceiptAccepted(valid_receipt()))
                .map(|journal| journal.body.phase),
            Ok(Phase::Committed)
        );
        // The receipt as the trial writes it (H.1): its pid and start instant.
        let exact = Receipt {
            started: Some(TRIAL.started),
            ..valid_receipt()
        };
        assert_eq!(
            planned
                .advance(&Event::LastTrialReady {
                    receipt: exact.clone(),
                    process: TRIAL,
                })
                .map(|journal| journal.body.phase),
            Ok(Phase::Committed),
            "the exact ready last trial needs no rescue-copy holder"
        );
        for (other, refusal) in [
            (
                Receipt {
                    started: Some(TRIAL.started.wrapping_add(1)),
                    ..exact.clone()
                },
                Refusal::ReceiptForAnotherProcess,
            ),
            (
                Receipt {
                    pid: TRIAL.pid + 1,
                    ..exact.clone()
                },
                Refusal::ReceiptForAnotherProcess,
            ),
            (valid_receipt(), Refusal::ReceiptForAnotherProcess),
            (
                Receipt {
                    nonce: nonce(0x99),
                    ..exact.clone()
                },
                Refusal::ReceiptForAnotherTrial,
            ),
            (
                Receipt {
                    txn: TxnId::new([0x01; 16]),
                    ..exact.clone()
                },
                Refusal::ReceiptForAnotherTransaction,
            ),
        ] {
            assert_eq!(
                planned.advance(&Event::LastTrialReady {
                    receipt: other.clone(),
                    process: TRIAL,
                }),
                Err(refusal),
                "{other:?}"
            );
        }
        assert_eq!(
            trial
                .advance(&Event::LastTrialReady {
                    receipt: exact.clone(),
                    process: TRIAL,
                })
                .map(|journal| journal.body.phase),
            Err(Refusal::Illegal {
                from: PhaseKind::Trial,
                event: EventKind::LastTrialReady,
            }),
            "once a holder adopted it, the holder commits it"
        );
        let disk = holder(&planned, flipped().located(&inventories()));
        assert_eq!(decide(&disk), Action::DeclareRollback);
        let intent = planned
            .advance(&Event::RollbackDeclared)
            .expect("the rollback over the reservation");
        assert_eq!(
            intent.body.phase,
            Phase::RollbackIntent {
                trial: None,
                trial_started: true,
            }
        );
        // The review's m1: the reserved trial was asked to start, so its
        // rollback is never "interrupted before the new version started" —
        // through `Stuck` too — while one from `Moving` still is.
        let failed = intent
            .advance(&Event::RollbackFailed {
                error: "held".to_owned(),
            })
            .expect("a failed rollback");
        for back in [&intent, &failed] {
            assert_eq!(
                back.advance(&Event::RolledBack)
                    .map(|journal| journal.body.phase),
                Ok(Phase::RolledBack { untried: false }),
                "{:?}",
                back.body.phase
            );
        }
        assert_eq!(
            moving
                .advance(&Event::RollbackDeclared)
                .and_then(|journal| journal.advance(&Event::RolledBack))
                .map(|journal| journal.body.phase),
            Ok(Phase::RolledBack { untried: true })
        );
    }

    /// RED (U-10) — **W7: a trial without its receipt is waited for only while
    /// its recorded process lives and its deadline holds; otherwise the
    /// rollback is declared, carrying the process to stop.**
    #[test]
    fn w7_a_trial_without_its_receipt_waits_only_while_it_lives_and_its_deadline_holds() {
        let journal = journal(trial_phase(), members_layout());
        let located = flipped().located(&inventories());
        let early_alive = Disk {
            trial_alive: true,
            now_ms: BEGAN + 1,
            ..holder(&journal, located.clone())
        };
        assert_eq!(
            decide(&early_alive),
            Action::AwaitReceipt {
                until_ms: BEGAN + TRIAL_DEADLINE_MS
            }
        );
        let late_alive = Disk {
            trial_alive: true,
            now_ms: BEGAN + TRIAL_DEADLINE_MS,
            ..holder(&journal, located.clone())
        };
        assert_eq!(decide(&late_alive), Action::DeclareRollback);
        let early_gone = Disk {
            now_ms: BEGAN + 1,
            ..holder(&journal, located)
        };
        assert_eq!(decide(&early_gone), Action::DeclareRollback);
        assert_eq!(
            journal
                .advance(&Event::RollbackDeclared)
                .map(|j| j.body.phase),
            Ok(Phase::RollbackIntent {
                trial: Some(TRIAL),
                trial_started: false
            })
        );
    }

    /// RED (U-10) — **W8: a trial holding its receipt is committed — even past
    /// its deadline — and `Committed` is durable before anything is deleted.**
    ///
    /// `Commit` itself deletes nothing; only the answer to `Committed` does.
    #[test]
    fn w8_a_trial_with_its_receipt_is_committed_before_anything_is_deleted() {
        let journal = journal(trial_phase(), members_layout());
        let disk = Disk {
            receipt: Some(valid_receipt()),
            entrance: true,
            now_ms: BEGAN + 10 * TRIAL_DEADLINE_MS,
            ..holder(&journal, flipped().located(&inventories()))
        };
        assert_eq!(decide(&disk), Action::Commit);
        assert!(Action::Commit.effects().is_empty());
        let committed = journal
            .advance(&Event::ReceiptAccepted(valid_receipt()))
            .expect("committed");
        assert_eq!(committed.body.phase, Phase::Committed);
        let after = Disk {
            entrance: true,
            ..holder(&committed, flipped().located(&inventories()))
        };
        assert_eq!(
            decide(&after),
            Action::FinishCommit {
                delete_backup: inventories()
                    .old_present
                    .iter()
                    .map(|member| member.name.clone())
                    .collect(),
                delete_stage: false,
                remove_entrance: true,
            }
        );
        assert_eq!(
            committed.advance(&Event::Retired).map(|j| j.body.phase),
            Ok(Phase::Retired {
                outcome: Outcome::Committed,
                untried: false,
            })
        );
    }

    /// RED (U-10) — **W9: a rollback stops the trial first, moves every new
    /// file out before any old file back, verifies the old install by digest,
    /// and stops at `Stuck` rather than move a file nobody recorded.**
    #[test]
    fn w9_rollback_stops_the_trial_moves_new_out_before_old_back_and_verifies() {
        let inventories = inventories();
        let journal = journal(
            Phase::RollbackIntent {
                trial: Some(TRIAL),
                trial_started: false,
            },
            members_layout(),
        );
        let mut disk = flipped();
        let alive = Disk {
            trial_alive: true,
            ..holder(&journal, disk.located(&inventories))
        };
        assert_eq!(decide(&alive), Action::StopTrial(TRIAL));
        let Action::RollBack(Restore::Moves(moves)) =
            decide(&holder(&journal, disk.located(&inventories)))
        else {
            panic!("a rollback")
        };
        let first_back = moves
            .iter()
            .position(|step| step.to == Place::Install)
            .expect("old files come back");
        assert!(
            moves[..first_back]
                .iter()
                .all(|step| step.to == Place::RolledOut)
        );
        assert!(
            moves[first_back..]
                .iter()
                .all(|step| step.from == Place::Backup)
        );
        assert!(
            !moves.iter().any(|step| step.name == "conpty.dll"),
            "a member that did not change is already the old file"
        );
        moves.iter().for_each(|step| disk.apply(step));
        assert!(disk.is_old_install(&inventories));
        assert_eq!(
            decide(&holder(&journal, disk.located(&inventories))),
            Action::DeclareRolledBack
        );

        let mut foreign = flipped();
        foreign.install.insert("folio.exe".to_owned(), digest(0xee));
        let untouched = foreign.clone();
        assert!(matches!(
            decide(&holder(&journal, foreign.located(&inventories))),
            Action::StayStuck { .. }
        ));
        let stuck = drive(journal.clone(), &mut foreign, true);
        assert!(matches!(stuck.body.phase, Phase::Stuck { .. }));
        assert_eq!(
            foreign, untouched,
            "nothing moved around a file nobody recorded"
        );
    }

    /// RED (U-10) — **W10: `Stuck` retries the same rollback at every holder,
    /// and a retry that can finish does.**
    #[test]
    fn w10_stuck_retries_the_rollback_at_every_holder() {
        let inventories = inventories();
        let intent = journal(
            Phase::RollbackIntent {
                trial: None,
                trial_started: false,
            },
            members_layout(),
        );
        let stuck = journal(
            Phase::Stuck {
                trial: None,
                trial_started: false,
                last_error: "a file was held open".to_owned(),
                attempts: 1,
                retrial: None,
            },
            members_layout(),
        );
        let mut disk = flipped();
        assert_eq!(
            decide(&holder(&stuck, disk.located(&inventories))),
            decide(&holder(&intent, disk.located(&inventories)))
        );
        let end = drive(stuck, &mut disk, true);
        assert_eq!(
            end.body.phase,
            Phase::Retired {
                outcome: Outcome::RolledBack,
                untried: true,
            }
        );
        assert!(disk.is_old_install(&inventories));
    }

    /// RED (U-10) — **W11: `RolledBack` removes the entrance, relaunches the old
    /// build and retires; a later start then deletes what is left.**
    #[test]
    fn w11_rolled_back_removes_the_entrance_relaunches_and_retires() {
        let journal = journal(Phase::RolledBack { untried: false }, members_layout());
        let disk = Disk {
            entrance: true,
            ..holder(&journal, prepared_located())
        };
        assert_eq!(
            decide(&disk),
            Action::FinishRollback {
                remove_entrance: true
            }
        );
        assert_eq!(
            at_start(&start_on(Phase::RolledBack { untried: false })),
            StartAction::HandToRescue
        );
        let retired = journal.advance(&Event::Retired).expect("retired");
        assert_eq!(at_start(&start_on(retired.body.phase)), StartAction::Retire);
    }

    /// RED (U-10) — **W12: cleanup after `Committed` deletes only the recorded
    /// old files still in `backup\`, and never becomes a rollback, whatever the
    /// disk looks like.**
    #[test]
    fn w12_cleanup_after_commit_is_finished_and_never_becomes_a_rollback() {
        let inventories = inventories();
        let committed = journal(Phase::Committed, members_layout());
        let mut disk = flipped();
        disk.backup.remove("folio.exe");
        disk.backup.remove("conpty.dll");
        disk.backup
            .insert("OpenConsole.exe".to_owned(), digest(0xee));
        assert_eq!(
            decide(&holder(&committed, disk.located(&inventories))),
            Action::FinishCommit {
                delete_backup: vec!["uninstall.cmd".to_owned(), "helper.dll".to_owned()],
                delete_stage: false,
                remove_entrance: false,
            }
        );
        for view in disks_for(&committed) {
            let action = decide(&view);
            assert!(
                matches!(action, Action::FinishCommit { .. } | Action::Leave),
                "{action:?}"
            );
        }
        assert_eq!(
            committed.advance(&Event::RollbackDeclared),
            Err(Refusal::Illegal {
                from: PhaseKind::Committed,
                event: EventKind::RollbackDeclared,
            })
        );
    }

    /// RED (U-10) — **W13: an abandoned transaction is retired — by its lock
    /// holder, or by any start that can take the lock.**
    #[test]
    fn w13_an_abandoned_transaction_is_retired() {
        let journal = journal(Phase::Abandoned, members_layout());
        let disk = Disk {
            entrance: true,
            ..holder(&journal, prepared_located())
        };
        assert_eq!(
            decide(&disk),
            Action::Retire {
                remove_entrance: true
            }
        );
        assert_eq!(at_start(&start_on(Phase::Abandoned)), StartAction::Retire);
        let held = start(JournalRead::Read(journal.header()), false);
        assert_eq!(at_start(&held), StartAction::Continue);
    }

    /// RED (U-10) — **a rollback cut after any of its moves is finished by the
    /// next holder**, which reads the disk again rather than remembering where
    /// it was: recovery interrupted during recovery is recovery again.
    #[test]
    fn a_rollback_cut_after_any_move_is_finished_by_the_next_holder() {
        let inventories = inventories();
        let journal = journal(
            Phase::RollbackIntent {
                trial: None,
                trial_started: false,
            },
            members_layout(),
        );
        let Action::RollBack(Restore::Moves(moves)) =
            decide(&holder(&journal, flipped().located(&inventories)))
        else {
            panic!("a rollback")
        };
        for done in 0..=moves.len() {
            let mut disk = flipped();
            moves[..done].iter().for_each(|step| disk.apply(step));
            let end = drive(journal.clone(), &mut disk, true);
            assert_eq!(
                end.body.phase,
                Phase::Retired {
                    outcome: Outcome::RolledBack,
                    untried: true,
                },
                "cut after {done}"
            );
            assert!(disk.is_old_install(&inventories), "cut after {done}");
        }
    }

    // ───────────────────────────── (b).2, the macOS table ─────────────────────────────

    /// RED (U-10) — **M1: a dead macOS preparation is swept, mounts under `H`
    /// included.**
    #[test]
    fn m1_an_allocated_transaction_is_swept_with_its_mounts() {
        let journal = journal(Phase::Allocated, bundle_layout());
        let located = bundle(Some(old_bundle()), None);
        assert_eq!(decide(&job_owner(&journal, located.clone())), Action::Sweep);
        assert!(Action::Sweep.effects().contains(&Effect::DetachMount));
        assert_eq!(decide(&holder(&journal, located)), Action::Leave);
    }

    /// RED (U-10) — **M2: as W2 — the job owner's, discarded at its second
    /// launch without a resume.**
    #[test]
    fn m2_a_prepared_bundle_is_left_to_its_job_owner() {
        let journal = journal(
            Phase::Prepared {
                deferred_launches: 1,
            },
            bundle_layout(),
        );
        let located = bundle(Some(old_bundle()), Some(new_bundle()));
        assert_eq!(decide(&holder(&journal, located.clone())), Action::Leave);
        assert_eq!(
            decide(&job_owner(&journal, located)),
            Action::CountDeferredLaunch
        );
        assert_eq!(
            journal
                .advance(&Event::LaunchedWithoutResume)
                .map(|j| j.body.phase),
            Ok(Phase::Abandoned)
        );
    }

    /// RED (U-10) — **M3: as W3 — a handoff is applied.**
    #[test]
    fn m3_a_bundle_handoff_is_applied() {
        let journal = journal(
            Phase::Handoff {
                applier: nonce(0x44),
            },
            bundle_layout(),
        );
        let located = bundle(Some(old_bundle()), Some(new_bundle()));
        assert_eq!(decide(&holder(&journal, located)), Action::Apply);
    }

    /// RED (U-10) — **M4: as W5, into the exchange.**
    #[test]
    fn m4_armed_is_admitted_into_the_exchange() {
        let journal = journal(Phase::Armed, bundle_layout());
        let located = bundle(Some(old_bundle()), Some(new_bundle()));
        assert_eq!(decide(&holder(&journal, located)), Action::Admit);
        assert_eq!(
            journal.advance(&Event::Admitted).map(|j| j.body.phase),
            Ok(Phase::Moving)
        );
    }

    /// RED (U-10) — **M5: an exchange not performed — the old identity is live
    /// — removes the plist and reverts to `Prepared`.**
    #[test]
    fn m5_an_exchange_not_performed_reverts_to_prepared() {
        let journal = journal(Phase::Moving, bundle_layout());
        let disk = Disk {
            entrance: true,
            ..holder(&journal, bundle(Some(old_bundle()), Some(new_bundle())))
        };
        assert_eq!(
            decide(&disk),
            Action::Revert {
                remove_entrance: true
            }
        );
        assert_eq!(Asker::LockHolder.actor(PhaseKind::Moving), Actor::Recovery);
        assert_eq!(
            journal.advance(&Event::Reverted).map(|j| j.body.phase),
            Ok(Phase::Prepared {
                deferred_launches: 0
            })
        );
    }

    /// RED (U-10, U-29b) — **M6: an exchange performed — the new identity is
    /// live, the old one in `stage/` — is decided by a trial the holder
    /// starts, by identity and never by the phase; a trial that cannot start
    /// is rolled back, and so is an exchange whose bundles are anywhere
    /// else.**
    ///
    /// The coordinator's ruling 1 (U-29b): "`Exchanging` → decided by the
    /// live identity (… new live: → `Trial` road, or `RollbackIntent` if the
    /// trial cannot be started)". The recovery writes `Trial` for it.
    ///
    /// MUTATION: drop the `BeginTrial` arm of `decide`'s `Moving`.
    #[test]
    fn m6_an_exchange_performed_is_decided_by_a_trial() {
        let journal = journal(Phase::Moving, bundle_layout());
        let located = bundle(Some(new_bundle()), Some(old_bundle()));
        for asker in [Asker::LockHolder, Asker::Rescue] {
            let disk = Disk {
                asker,
                ..holder(&journal, located.clone())
            };
            assert_eq!(decide(&disk), Action::BeginTrial);
            assert_eq!(asker.actor(PhaseKind::Moving), Actor::Recovery);
        }
        assert!(may_record(Actor::Recovery, PhaseKind::Trial));
        let trial = journal
            .advance(&Event::TrialBegan {
                nonce: nonce(TRIAL_NONCE),
                process: TRIAL,
                began_ms: BEGAN,
            })
            .expect("R starts N");
        assert_eq!(trial.body.phase, trial_phase());
        assert_eq!(
            journal
                .advance(&Event::RollbackDeclared)
                .map(|j| j.body.phase),
            Ok(Phase::RollbackIntent {
                trial: None,
                trial_started: false
            })
        );
        let nowhere = bundle(Some(new_bundle()), None);
        assert_eq!(decide(&holder(&journal, nowhere)), Action::DeclareRollback);
    }

    /// RED (U-29b) — **the rescue build puts back what a dead applier never
    /// exchanged — `Handoff` and `Armed` go to `Prepared`, the entrance
    /// removed — and waits for, then commits on, the trial it started over a
    /// `Stuck` whose new bundle is live; the bound still holds for everything
    /// else.**
    ///
    /// The coordinator's rulings 1 and 3: "`Handoff`/`Armed` (nothing
    /// exchanged) → remove the entrance, → `Prepared`"; "`Stuck` with the new
    /// bundle live starts the new build as a trial … if that trial then
    /// produces a receipt, the lock holder writes `Committed`". The applier
    /// itself still applies a `Handoff` (W3).
    ///
    /// MUTATION: drop the `Asker::Rescue` arm of `decide`'s `Handoff | Armed`.
    #[test]
    fn the_rescue_reverts_what_was_never_exchanged_and_waits_for_its_retrial() {
        let located = bundle(Some(old_bundle()), Some(new_bundle()));
        for phase in [
            Phase::Handoff {
                applier: nonce(0x44),
            },
            Phase::Armed,
        ] {
            let journal = journal(phase, bundle_layout());
            for entrance in [false, true] {
                let disk = Disk {
                    asker: Asker::Rescue,
                    entrance,
                    ..holder(&journal, located.clone())
                };
                assert_eq!(
                    decide(&disk),
                    Action::Revert {
                        remove_entrance: entrance
                    }
                );
            }
            assert_eq!(
                journal.advance(&Event::Reverted).map(|j| j.body.phase),
                Ok(Phase::Prepared {
                    deferred_launches: 0
                })
            );
        }
        let swapped = bundle(Some(new_bundle()), Some(old_bundle()));
        let stuck = journal(
            Phase::Stuck {
                trial: None,
                trial_started: false,
                last_error: "the exchange was refused".to_owned(),
                attempts: STUCK_ATTEMPT_LIMIT,
                retrial: None,
            },
            bundle_layout(),
        );
        let retried = stuck
            .advance(&Event::RetrialBegan {
                nonce: nonce(TRIAL_NONCE),
                process: TRIAL,
                began_ms: BEGAN,
            })
            .expect("the holder records the trial it started");
        let Phase::Stuck {
            trial, attempts, ..
        } = &retried.body.phase
        else {
            panic!("{:?}", retried.body.phase);
        };
        assert_eq!((*trial, *attempts), (Some(TRIAL), STUCK_ATTEMPT_LIMIT));
        assert_eq!(retried.header().class, Class::Destructive);
        let waiting = Disk {
            asker: Asker::Rescue,
            trial_alive: true,
            ..holder(&retried, swapped.clone())
        };
        assert_eq!(
            decide(&waiting),
            Action::AwaitReceipt {
                until_ms: BEGAN + TRIAL_DEADLINE_MS
            }
        );
        let answered = Disk {
            receipt: Some(valid_receipt()),
            ..waiting.clone()
        };
        assert_eq!(decide(&answered), Action::Commit);
        assert_eq!(
            retried
                .advance(&Event::ReceiptAccepted(valid_receipt()))
                .map(|j| j.body.phase),
            Ok(Phase::Committed)
        );
        assert_eq!(
            retried.advance(&Event::ReceiptAccepted(receipt(txn(), nonce(0x99)))),
            Err(Refusal::ReceiptForAnotherTrial)
        );
        let late = Disk {
            now_ms: BEGAN + TRIAL_DEADLINE_MS,
            ..waiting
        };
        assert_eq!(decide(&late), Action::StopTrial(TRIAL));
        let gone = Disk {
            trial_alive: false,
            ..late
        };
        assert!(matches!(decide(&gone), Action::GiveUp { .. }));
        let failed_again = retried
            .advance(&Event::RollbackFailed {
                error: "refused".to_owned(),
            })
            .expect("recorded");
        assert!(matches!(
            failed_again.body.phase,
            Phase::Stuck { retrial: None, .. }
        ));
    }

    /// RED (U-10) — **M7: as W7 — waited for while alive, rolled back when not.**
    #[test]
    fn m7_a_bundle_trial_without_its_receipt_waits_then_rolls_back() {
        let journal = journal(trial_phase(), bundle_layout());
        let located = bundle(Some(new_bundle()), Some(old_bundle()));
        let alive = Disk {
            trial_alive: true,
            ..holder(&journal, located.clone())
        };
        assert!(matches!(decide(&alive), Action::AwaitReceipt { .. }));
        assert_eq!(decide(&holder(&journal, located)), Action::DeclareRollback);
    }

    /// RED (U-10) — **M8: a receipt commits; then the plist and the old bundle
    /// in `stage/` are retired.**
    #[test]
    fn m8_a_bundle_receipt_commits_then_the_old_bundle_is_retired() {
        let journal = journal(trial_phase(), bundle_layout());
        let located = bundle(Some(new_bundle()), Some(old_bundle()));
        let disk = Disk {
            receipt: Some(valid_receipt()),
            ..holder(&journal, located.clone())
        };
        assert_eq!(decide(&disk), Action::Commit);
        let committed = journal
            .advance(&Event::ReceiptAccepted(valid_receipt()))
            .expect("committed");
        let after = Disk {
            entrance: true,
            ..holder(&committed, located)
        };
        assert_eq!(
            decide(&after),
            Action::FinishCommit {
                delete_backup: Vec::new(),
                delete_stage: true,
                remove_entrance: true,
            }
        );
    }

    /// RED (U-10) — **M9: the rollback stops the trial, swaps back only while
    /// the new bundle is live, never swaps a restored old bundle away again,
    /// and is stuck when neither identity is where a swap needs it.**
    #[test]
    fn m9_rollback_swaps_back_only_while_the_new_bundle_is_live() {
        let journal = journal(
            Phase::RollbackIntent {
                trial: Some(TRIAL),
                trial_started: false,
            },
            bundle_layout(),
        );
        let swapped = bundle(Some(new_bundle()), Some(old_bundle()));
        let alive = Disk {
            trial_alive: true,
            ..holder(&journal, swapped.clone())
        };
        assert_eq!(decide(&alive), Action::StopTrial(TRIAL));
        assert_eq!(
            decide(&holder(&journal, swapped)),
            Action::RollBack(Restore::SwapBack)
        );
        let restored = bundle(Some(old_bundle()), Some(new_bundle()));
        assert_eq!(
            decide(&holder(&journal, restored)),
            Action::DeclareRolledBack
        );
        for lost in [
            bundle(None, Some(old_bundle())),
            bundle(Some(new_bundle()), None),
        ] {
            assert!(matches!(
                decide(&holder(&journal, lost)),
                Action::StayStuck { .. }
            ));
        }
    }

    /// RED (U-10) — **M10: `Stuck` is M9 again, and a start hands itself to the
    /// rescue.**
    #[test]
    fn m10_a_stuck_bundle_rollback_is_retried() {
        let journal = journal(
            Phase::Stuck {
                trial: None,
                trial_started: false,
                last_error: "the exchange was refused".to_owned(),
                attempts: 1,
                retrial: None,
            },
            bundle_layout(),
        );
        let swapped = bundle(Some(new_bundle()), Some(old_bundle()));
        assert_eq!(
            decide(&holder(&journal, swapped)),
            Action::RollBack(Restore::SwapBack)
        );
        let view = start(JournalRead::Read(journal.header()), true);
        assert_eq!(at_start(&view), StartAction::HandToRescue);
    }

    /// RED (U-29) — **`Stuck` counts its failed rollbacks: each failure adds
    /// one, a holder retries below [`STUCK_ATTEMPT_LIMIT`] and gives up at it
    /// — recording nothing, keeping everything — while a trial that still
    /// lives is stopped first all the same.**
    ///
    /// The coordinator's ruling 4 (U-29): "add `Stuck{attempts}` and stop after
    /// 3 with the sentence naming the folder".
    ///
    /// MUTATION: drop the `attempts >= STUCK_ATTEMPT_LIMIT` arm of `decide`.
    #[test]
    fn stuck_counts_its_attempts_and_gives_up_at_the_bound() {
        let swapped = bundle(Some(new_bundle()), Some(old_bundle()));
        let mut journal = journal(
            Phase::RollbackIntent {
                trial: None,
                trial_started: false,
            },
            bundle_layout(),
        );
        for attempt in 1..=STUCK_ATTEMPT_LIMIT {
            assert!(matches!(
                decide(&holder(&journal, swapped.clone())),
                Action::RollBack(Restore::SwapBack)
            ));
            journal = journal
                .advance(&Event::RollbackFailed {
                    error: format!("refused {attempt}"),
                })
                .expect("a failed rollback is recorded");
            let Phase::Stuck { attempts, .. } = &journal.body.phase else {
                panic!("{:?}", journal.body.phase);
            };
            assert_eq!(*attempts, attempt);
        }
        let given_up = decide(&holder(&journal, swapped.clone()));
        assert_eq!(
            given_up,
            Action::GiveUp {
                last_error: format!("refused {STUCK_ATTEMPT_LIMIT}")
            }
        );
        assert!(given_up.effects().is_empty());
        let mut living = journal.clone();
        if let Phase::Stuck { trial, .. } = &mut living.body.phase {
            *trial = Some(TRIAL);
        }
        let alive = Disk {
            trial_alive: true,
            ..holder(&living, swapped)
        };
        assert_eq!(decide(&alive), Action::StopTrial(TRIAL));
        let view = start(JournalRead::Read(journal.header()), true);
        assert_eq!(at_start(&view), StartAction::HandToRescue);
    }

    /// RED (U-29, U-29b) — **a start sent with `--update-failed` continues
    /// past any transaction that is not retired — and only such a start; the
    /// header alone says which card it raises.**
    ///
    /// The rescue build that could not finish a rollback starts the installed
    /// build with `--update-failed <journal>` (U-29): handing that start back
    /// to the rescue would send it straight here again, and Folio would never
    /// open ("an app that never opens again is not an answer", U-12's
    /// ruling). Since U-29b a recovery that fails on any road starts the live
    /// build the same way (the coordinator's ruling 2), whatever the phase:
    /// the word decides, and the card is *Update incomplete.*
    ///
    /// MUTATION: drop the `sent_by_rollback` arm of `at_start`.
    #[test]
    fn a_start_sent_with_update_failed_continues_past_an_unfinished_rollback() {
        let sent = |phase: Phase| StartView {
            sent_by_rollback: true,
            ..start_on(phase)
        };
        let stuck = Phase::Stuck {
            trial: None,
            trial_started: false,
            last_error: "the exchange was refused".to_owned(),
            attempts: 2,
            retrial: None,
        };
        for phase in [
            stuck.clone(),
            Phase::RollbackIntent {
                trial: None,
                trial_started: false,
            },
            Phase::RolledBack { untried: false },
            Phase::Handoff {
                applier: nonce(0x44),
            },
            Phase::Moving,
            trial_phase(),
            Phase::Committed,
        ] {
            assert_eq!(at_start(&sent(phase.clone())), StartAction::Continue);
            assert_eq!(at_start(&start_on(phase)), StartAction::HandToRescue);
        }
        let retired = Phase::Retired {
            outcome: Outcome::RolledBack,
            untried: false,
        };
        assert_eq!(at_start(&sent(retired.clone())), StartAction::Retire);

        let header = |phase: Phase| journal(phase, bundle_layout()).header();
        assert_eq!(
            after_rollback(&header(retired)),
            Some(AfterRollback::Restored)
        );
        assert_eq!(
            after_rollback(&header(stuck)),
            Some(AfterRollback::Incomplete)
        );
        assert_eq!(
            after_rollback(&header(Phase::RolledBack { untried: false })),
            Some(AfterRollback::Incomplete)
        );
        for phase in [Phase::Armed, trial_phase(), Phase::Committed] {
            assert_eq!(
                after_rollback(&header(phase)),
                Some(AfterRollback::Incomplete)
            );
        }
        for phase in [
            Phase::Abandoned,
            Phase::Retired {
                outcome: Outcome::Committed,
                untried: false,
            },
            Phase::Prepared {
                deferred_launches: 0,
            },
        ] {
            assert_eq!(after_rollback(&header(phase)), None);
        }
    }

    /// RED (U-10) — **M11: `RolledBack`, `Abandoned` and `Committed` with debt
    /// finish as W11–W13 do; the plist goes only after the terminal state.**
    #[test]
    fn m11_rolled_back_abandoned_and_committed_with_debt_finish_as_on_windows() {
        let located = bundle(Some(new_bundle()), None);
        let rolled_back = journal(Phase::RolledBack { untried: false }, bundle_layout());
        let abandoned = journal(Phase::Abandoned, bundle_layout());
        let committed = journal(Phase::Committed, bundle_layout());
        let rolled_back_disk = Disk {
            entrance: true,
            ..holder(&rolled_back, located.clone())
        };
        assert_eq!(
            decide(&rolled_back_disk),
            Action::FinishRollback {
                remove_entrance: true
            }
        );
        let abandoned_disk = Disk {
            entrance: true,
            ..holder(&abandoned, located.clone())
        };
        assert_eq!(
            decide(&abandoned_disk),
            Action::Retire {
                remove_entrance: true
            }
        );
        let committed_disk = Disk {
            entrance: true,
            ..holder(&committed, located)
        };
        assert_eq!(
            decide(&committed_disk),
            Action::FinishCommit {
                delete_backup: Vec::new(),
                delete_stage: false,
                remove_entrance: true,
            }
        );
    }

    // ───────────────────────────── the ordinary start ─────────────────────────────

    /// RED (U-10) — **a start that is the header's own trial runs as the trial;
    /// any other start during a destructive transaction hands itself to the
    /// rescue.**
    #[test]
    fn a_destructive_journal_hands_every_start_but_its_trial_to_the_rescue() {
        let header = journal(trial_phase(), members_layout()).header();
        let mut view = start(JournalRead::Read(header), false);
        assert_eq!(at_start(&view), StartAction::HandToRescue);
        view.trial_of = Some(TxnId::new([0x01; 16]));
        assert_eq!(at_start(&view), StartAction::HandToRescue);
        view.trial_of = Some(txn());
        assert_eq!(at_start(&view), StartAction::RunAsTrial);
    }

    /// RED (U-10) — **a journal a start cannot read is left exactly as it is**,
    /// and so is a terminal one whose lock another process holds.
    #[test]
    fn an_unreadable_or_locked_journal_is_left_alone() {
        for read in [
            JournalRead::Absent,
            JournalRead::Unreadable(ParseRefusal::Truncated),
            JournalRead::Unreadable(ParseRefusal::UnknownVersion(2)),
        ] {
            assert_eq!(at_start(&start(read, true)), StartAction::Continue);
        }
        let header = journal(
            Phase::Retired {
                outcome: Outcome::Committed,
                untried: false,
            },
            members_layout(),
        )
        .header();
        assert_eq!(
            at_start(&start(JournalRead::Read(header), false)),
            StartAction::Continue
        );
    }

    /// RED (U-10) — **a start whose own image is not the rescue copy of O,
    /// while the transaction is preparing or deferred, discards the transaction
    /// — and only when it can take the lock.**
    #[test]
    fn an_install_replaced_by_hand_discards_a_waiting_transaction() {
        for phase in [
            Phase::Allocated,
            Phase::Prepared {
                deferred_launches: 0,
            },
        ] {
            let mut view = start_on(phase);
            view.rescue_image = Some(digest(0x02));
            assert_eq!(at_start(&view), StartAction::Discard);
            view.lock_free = false;
            assert_eq!(at_start(&view), StartAction::Continue);
            view.lock_free = true;
            view.rescue_image = None;
            assert_eq!(at_start(&view), StartAction::Continue);
            view.rescue_image = Some(digest(0x02));
            view.own_image = None;
            assert_eq!(at_start(&view), StartAction::Continue);
        }
    }

    /// RED (U-12) — **the installation home is found from the running
    /// executable alone: inside the install folder on Windows, beside the
    /// bundle on macOS, and nowhere on any other platform or outside a
    /// bundle.**
    ///
    /// (b).2's objects table, and F-3's reason for the macOS shape: the home
    /// must outlive the swap of the bundle it serves, so it cannot be inside
    /// it. The rescue build's program follows the same layout: the Windows
    /// header names the executable, the macOS header the clone's bundle, whose
    /// executable is where this copy's own is inside its bundle.
    ///
    /// MUTATION: in `Home::of`'s macOS arm, join the name onto `bundle`
    /// instead of `bundle.parent()?` (a home inside the bundle).
    #[test]
    fn the_home_is_found_from_the_running_executable() {
        let install = PathBuf::from("Folio");
        let home = Home::of(HostPlatform::Windows, &install.join("folio.exe")).unwrap();
        let root = install.join(WINDOWS_HOME);
        assert_eq!(home.admission(), root.join("admission"));
        assert_eq!(home.lock(), root.join("lock"));
        assert_eq!(home.journal(), root.join("journal.json"));
        assert_eq!(
            home.transaction(txn()),
            root.join("7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a")
        );
        let rescue = root
            .join("7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a")
            .join("rescue")
            .join("folio.exe");
        assert_eq!(home.rescue_program(&rescue.to_string_lossy()), rescue);

        let applications = PathBuf::from("Applications");
        let exe = applications
            .join("Folio.app")
            .join("Contents")
            .join("MacOS")
            .join("folio");
        let home = Home::of(HostPlatform::MacOs, &exe).unwrap();
        assert_eq!(
            home.journal(),
            applications
                .join(".Folio.app.folio-update")
                .join("journal.json")
        );
        let clone = applications
            .join(".Folio.app.folio-update")
            .join("7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a")
            .join("rescue")
            .join("Folio.app");
        assert_eq!(
            home.rescue_program(&clone.to_string_lossy()),
            clone.join("Contents").join("MacOS").join("folio")
        );

        let loose = PathBuf::from("target").join("debug").join("folio");
        assert_eq!(Home::of(HostPlatform::MacOs, &loose), None);
        assert_eq!(Home::of(HostPlatform::OtherUnix, &exe), None);
    }

    /// RED (U-26) — **the macOS home is found from the bundle's path alone, so
    /// every data root and every account that runs the bundle finds the same
    /// one**: the fixed sibling `<parent>/.<BundleName>.folio-update/`, outside
    /// the bundle, holding `lock`, `admission`, `journal.json` and each
    /// transaction's `stage/`, `rescue/` and `mnt/`.
    ///
    /// F-3: the lock, the journal and the rollback source must survive the
    /// exchange of the bundle, and a copy running under another data root (a
    /// second account, a moved data directory) must find the transaction the
    /// first one began — "that is the installation-to-transaction locator". A
    /// home derived from anything but the bundle's path would split one
    /// installation's transaction in two.
    ///
    /// MUTATION: in `Home::for_bundle`, join the name onto `bundle` instead of
    /// `bundle.parent()?` (a home inside the bundle, swapped away with it).
    #[test]
    fn the_home_is_found_from_any_data_root() {
        let applications = PathBuf::from("/Applications");
        let bundle = applications.join("Folio.app");
        let root = applications.join(".Folio.app.folio-update");
        let exe = bundle.join("Contents").join("MacOS").join("folio");
        // Two accounts, each with its own data root; neither enters the answer.
        let data_roots = [
            PathBuf::from("/Users/alice/Library/Application Support/Folio"),
            PathBuf::from("/Users/bob/Library/Application Support/Folio"),
        ];
        let mut found = Vec::new();
        for data in &data_roots {
            let home = Home::for_bundle(&bundle).unwrap();
            assert!(!home.root().starts_with(data), "{data:?}");
            assert_eq!(Home::of(HostPlatform::MacOs, &exe), Some(home.clone()));
            found.push(home);
        }
        assert_eq!(found[0], found[1], "one home for every data root");
        let home = &found[0];
        assert_eq!(home.root(), root);
        assert!(!home.root().starts_with(&bundle), "outside the bundle");
        assert_eq!(
            home.root().parent(),
            bundle.parent(),
            "on the bundle's volume"
        );
        assert_eq!(home.lock(), root.join("lock"));
        assert_eq!(home.admission(), root.join("admission"));
        assert_eq!(home.journal(), root.join("journal.json"));
        let txn_dir = root.join("7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a");
        assert_eq!(home.transaction(txn()), txn_dir);
        assert_eq!(
            home.stage_bundle(txn()),
            Some(txn_dir.join("stage").join("Folio.app"))
        );
        assert_eq!(
            home.rescue_bundle(txn()),
            Some(txn_dir.join("rescue").join("Folio.app"))
        );
        assert_eq!(
            home.rescue_executable(txn()),
            Some(
                txn_dir
                    .join("rescue")
                    .join("Folio.app")
                    .join("Contents")
                    .join("MacOS")
                    .join("folio")
            )
        );
        assert_eq!(home.mount_point(txn()), Some(txn_dir.join("mnt")));

        // A renamed bundle has a home of its own, and its members keep its name.
        let renamed = Home::for_bundle(&applications.join("Folio Beta.app")).unwrap();
        assert_eq!(
            renamed.root(),
            applications.join(".Folio Beta.app.folio-update")
        );
        assert_eq!(
            renamed.rescue_bundle(txn()),
            Some(
                applications
                    .join(".Folio Beta.app.folio-update")
                    .join("7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a")
                    .join("rescue")
                    .join("Folio Beta.app")
            )
        );

        assert_eq!(Home::for_bundle(&applications.join("folio")), None);
        let windows = Home::of(HostPlatform::Windows, Path::new("Folio/folio.exe")).unwrap();
        assert_eq!(windows.stage_bundle(txn()), None);
        assert_eq!(windows.rescue_executable(txn()), None);
        assert_eq!(windows.mount_point(txn()), None);
    }

    /// RED (U-26) — **a home named by the entrance gives the recovery its
    /// journal and the installed program: on macOS from the home's name and
    /// the clone's own place, on Windows only when it is the home the rescue
    /// build's path gives.**
    ///
    /// F-3's plist passes `--update-recover <home>`; F-2's `Run` value passes
    /// no home. A name that is not a locator's home is no home at all.
    ///
    /// MUTATION: in `Home::of_rescue_named`'s macOS arm, answer the rescue
    /// clone's own executable (`exe.to_path_buf()`) as the installed program.
    #[test]
    fn a_named_home_gives_the_journal_and_the_installed_program() {
        let applications = PathBuf::from("/Applications");
        let home = applications.join(".Folio.app.folio-update");
        let clone = home
            .join("7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a")
            .join("rescue")
            .join("Folio.app")
            .join("Contents")
            .join("MacOS")
            .join("folio");
        let (found, installed) = Home::of_rescue_named(HostPlatform::MacOs, &clone, &home).unwrap();
        assert_eq!(found.journal(), home.join("journal.json"));
        assert_eq!(
            installed,
            applications
                .join("Folio.app")
                .join("Contents")
                .join("MacOS")
                .join("folio")
        );
        for not_a_home in [
            applications.join("Folio.app"),
            applications.join(".Folio.app"),
            applications.join(".Folio.folio-update"),
            PathBuf::from("/"),
        ] {
            assert_eq!(
                Home::of_rescue_named(HostPlatform::MacOs, &clone, &not_a_home),
                None,
                "{not_a_home:?}"
            );
        }

        let install = PathBuf::from(r"C:\Folio");
        let rescue = install
            .join(WINDOWS_HOME)
            .join("7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a")
            .join("rescue")
            .join("folio.exe");
        let derived = Home::of_rescue(HostPlatform::Windows, &rescue).unwrap();
        assert_eq!(
            Home::of_rescue_named(HostPlatform::Windows, &rescue, &install.join(WINDOWS_HOME)),
            Some(derived)
        );
        assert_eq!(
            Home::of_rescue_named(HostPlatform::Windows, &rescue, &install.join("elsewhere")),
            None
        );
        assert_eq!(
            Home::of_rescue_named(HostPlatform::OtherUnix, &clone, &home),
            None
        );
    }

    /// PIN (U-26) — **the LaunchAgent entrance starts the rescue build with the
    /// word the rescue build's door answers.** `bt-platform` sits below
    /// `bt-app`'s argv grammar and spells the word itself; this holds the two
    /// spellings to one.
    #[test]
    fn the_entrance_starts_the_rescue_with_the_recovery_word() {
        assert_eq!(
            bt_platform::launch_agent::RECOVER_FLAG,
            crate::cli::UPDATE_RECOVER_FLAG
        );
    }

    /// RED (U-10) — **a file already present at a name the new set ships is a
    /// collision, and is carried out to `backup\` like any old file** (F-8), so
    /// a rollback puts it back byte for byte.
    #[test]
    fn a_collision_is_rollback_material_like_any_old_file() {
        let inventories = inventories();
        let collisions: Vec<&str> = inventories
            .collisions()
            .map(|member| member.name.as_str())
            .collect();
        assert_eq!(collisions, vec!["helper.dll"]);
        assert!(inventories.forward_moves().contains(&Move {
            name: "helper.dll".to_owned(),
            from: Place::Install,
            to: Place::Backup,
        }));
    }

    // ───────────────────────── the escape hatch (0.4.8 E1) ─────────────────────────

    /// A journal of this module's transaction, as this build writes it.
    fn written(phase: Phase) -> Vec<u8> {
        journal(phase, members_layout()).encode()
    }

    /// The JSON of `bytes`, with `key` set to `value` at the top level.
    fn with_top(bytes: &[u8], key: &str, value: serde_json::Value) -> Vec<u8> {
        let mut document: serde_json::Value = serde_json::from_slice(bytes).expect("JSON");
        document[key] = value;
        serde_json::to_vec(&document).expect("bytes")
    }

    /// RED (E1) — **`sight` names what reads — the whole journal, the header
    /// alone, the envelope alone, or nothing — and the build the envelope
    /// names as its writer words the card but changes no reader's action.**
    ///
    /// Every sight that is not `Known`, with and without a later `written_by`,
    /// is put to every role of the table: the action is the role's row and
    /// nothing else. `written_by` counts as later only when it orders after
    /// this build; an equal, earlier, absent or unreadable one names nobody.
    /// The envelope reads whatever `v`, `class` and `outcome` say, and it is
    /// what a header that does not read acts as: `destructive`, nothing
    /// decided, its own transaction and rescue build.
    ///
    /// MUTATION: in `beyond_rule`, answer `BeyondAction::StandAside` when
    /// `newer` names a build (a later writer would change what a reader does).
    #[test]
    fn a_sight_names_what_reads_and_its_writer_changes_no_action() {
        let known = journal(Phase::Moving, members_layout());
        let bytes = known.encode();
        assert_eq!(sight(&bytes), Sight::Known(known.clone()));
        let [(_, header_word), (_, body_word), (_, nothing)] = beyond_inputs(&bytes);

        let envelope_newer = sight(&header_word);
        let Sight::Envelope { envelope, beyond } = &envelope_newer else {
            panic!("an unknown class reads as its envelope: {envelope_newer:?}");
        };
        assert_eq!(
            envelope,
            &Envelope {
                txn: txn(),
                rescue: known.rescue.clone(),
                written_by: Some(LATER_BUILD.to_owned()),
            }
        );
        assert_eq!(beyond.newer.as_deref(), Some(LATER_BUILD));
        assert_eq!(
            beyond.refusal,
            ParseRefusal::UnknownClass("paused".to_owned())
        );
        assert_eq!(
            envelope_newer.acting_header(),
            Some(Header {
                txn: txn(),
                rescue: known.rescue.clone(),
                class: Class::Destructive,
                outcome: HeaderOutcome::None,
            }),
            "an envelope acts as destructive with nothing decided"
        );

        // A later header version and outcome read as the envelope too.
        for (key, value) in [
            ("v", serde_json::Value::from(2)),
            ("outcome", serde_json::Value::from("paused")),
        ] {
            let later = with_top(&bytes, key, value);
            assert!(
                matches!(sight(&later), Sight::Envelope { .. }),
                "{key}: {:?}",
                sight(&later)
            );
        }

        // Only a build that orders after this one is named.
        for (written_by, named) in [
            (serde_json::Value::from(crate::version::VERSION), false),
            (serde_json::Value::from("0.4.6"), false),
            (serde_json::Value::from("no version"), false),
            (serde_json::Value::Null, false),
            (serde_json::Value::from("v99.1.0-rc.1"), true),
        ] {
            let mut document: serde_json::Value = serde_json::from_slice(&header_word).unwrap();
            document["written_by"] = written_by.clone();
            let seen = sight(&serde_json::to_vec(&document).unwrap());
            assert!(
                matches!(seen, Sight::Envelope { .. }),
                "{written_by}: {seen:?}"
            );
            assert_eq!(seen.newer().is_some(), named, "{written_by}");
        }
        let envelope_unnamed = sight(&with_top(
            &header_word,
            "written_by",
            serde_json::Value::from(crate::version::VERSION),
        ));

        let header_unnamed = sight(&body_word);
        let Sight::Header { header, beyond } = &header_unnamed else {
            panic!("an unknown phase reads as its header: {header_unnamed:?}");
        };
        assert_eq!(header, &known.header());
        assert_eq!(beyond.newer, None, "this build wrote it");
        assert!(matches!(beyond.refusal, ParseRefusal::Malformed(_)));
        let header_newer = sight(&with_top(
            &body_word,
            "written_by",
            serde_json::Value::from(LATER_BUILD),
        ));
        assert_eq!(header_newer.newer(), Some(LATER_BUILD));
        assert_eq!(header_newer.acting_header(), Some(known.header()));

        let unreadable = sight(&nothing);
        assert!(matches!(unreadable, Sight::Unreadable(_)), "{unreadable:?}");
        assert_eq!(unreadable.acting_header(), None);
        // A transaction id that does not read leaves no envelope either.
        let torn = with_top(&header_word, "txn", serde_json::Value::from("7a"));
        assert!(matches!(sight(&torn), Sight::Unreadable(_)));
        assert!(matches!(
            sight(&bytes[..bytes.len() / 2]),
            Sight::Unreadable(ParseRefusal::Truncated)
        ));

        // The table, row by row (design §3(a)).
        let table = [
            (Role::Start, BeyondAction::ActOnHeader),
            (Role::TrialWatch, BeyondAction::ActOnHeader),
            (Role::TrialHandBack, BeyondAction::ActOnHeader),
            (Role::ReceiptWrite, BeyondAction::NeverAccept),
            (Role::WindowsReceiptWatch, BeyondAction::NeverAccept),
            (Role::LastTrialReserve, BeyondAction::StandAside),
            (Role::LastTrialCommit, BeyondAction::StandAside),
            (Role::WindowElection, BeyondAction::StandAside),
            (Role::WindowsExit, BeyondAction::UnknownLiveSet),
            (Role::WindowsHolder, BeyondAction::StandAside),
            (Role::MacExit, BeyondAction::UnknownLiveSet),
            (Role::MacHolder, BeyondAction::StandAside),
            (Role::MacReceiptWatch, BeyondAction::NeverAccept),
            (Role::RecoveryDoor, BeyondAction::UnknownLiveSet),
            (Role::OutgoingExit, BeyondAction::ActOnHeader),
            (Role::JobOwner, BeyondAction::LeaveToItsRescue),
        ];
        assert_eq!(table.map(|(role, _)| role), Role::ALL);
        let beyond = [
            &envelope_newer,
            &envelope_unnamed,
            &header_newer,
            &header_unnamed,
            &unreadable,
        ];
        for (role, action) in table {
            assert_eq!(role.beyond(), action, "{role:?}");
            for seen in beyond {
                assert_eq!(
                    beyond_rule(role, seen.newer()),
                    action,
                    "{role:?} over {seen:?}"
                );
            }
        }
    }

    /// RED (E1; role #2, the trial's watch, site H2 `trial_sight`) — **the
    /// trial reads a journal it cannot read whole by the header it acts on**:
    /// an unknown header word is its envelope's transaction, `destructive`
    /// and undecided — its writes stay held, and a journal naming another
    /// transaction is this trial's end; an unknown body word is decided by the
    /// frozen header as ever (a retired commit releases the writes); nothing
    /// that reads stays undecided. Pure: nothing is written.
    ///
    /// MUTATION: in `trial_sight`, read the header alone again
    /// (`Header::parse(bytes).ok()`): another transaction's envelope keeps the
    /// trial waiting for ever, its writes neither released nor dropped.
    #[test]
    fn the_trial_watch_reads_what_it_cannot_read_whole_by_its_header() {
        let other = TxnId::new([0x11; 16]);
        let trial = written(Phase::Trial {
            nonce: nonce(TRIAL_NONCE),
            process: TRIAL,
            began_ms: BEGAN,
        });
        let [
            (header_word, header_bytes),
            (body_word, body_bytes),
            (nothing, no_bytes),
        ] = beyond_inputs(&trial);
        for (what, bytes, own, another) in [
            (
                header_word,
                &header_bytes,
                TrialSight::Undecided,
                TrialSight::Ended,
            ),
            (
                body_word,
                &body_bytes,
                TrialSight::Undecided,
                TrialSight::Ended,
            ),
            (
                nothing,
                &no_bytes,
                TrialSight::Undecided,
                TrialSight::Undecided,
            ),
        ] {
            assert_eq!(trial_sight(Some(bytes), &txn()), own, "{what}");
            assert_eq!(trial_sight(Some(bytes), &other), another, "{what}");
        }
        let retired = written(Phase::Retired {
            outcome: Outcome::Committed,
            untried: false,
        });
        let [_, (body_word, body_bytes), _] = beyond_inputs(&retired);
        assert_eq!(
            trial_sight(Some(&body_bytes), &txn()),
            TrialSight::Committed,
            "{body_word}: the frozen header's outcome"
        );
    }

    /// `v0.4.6-preview` and `v0.4.7-preview`: `HeaderWire`, verbatim — the
    /// two are the same.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct HeaderWire046 {
        v: u64,
        txn: TxnId,
        rescue: String,
        class: String,
        outcome: String,
    }

    /// The header as 0.4.6 and 0.4.7 read it: the version first
    /// (`versioned`), then the five fields.
    fn header_as_0_4_6_and_0_4_7_read_it(bytes: &[u8]) -> Option<HeaderWire046> {
        #[derive(Deserialize)]
        struct Versioned046 {
            v: u64,
        }
        let Versioned046 { v } = serde_json::from_slice(bytes).ok()?;
        (v == 1).then_some(())?;
        serde_json::from_slice(bytes).ok()
    }

    /// PIN (E1, the rollout contract; the shape of
    /// `a_0_4_6_reader_takes_a_0_4_7_receipt_as_it_always_did`) — **0.4.6 and
    /// 0.4.7 read a header that names its writer exactly as they always read
    /// one, and this build reads 0.4.7's bytes, which name no writer, whole.**
    ///
    /// `written_by` is a key 0.4.6 and 0.4.7 do not know, in a v1 document:
    /// they ignore it, as they ignored `adapter` and the receipt's `started`
    /// (no `deny_unknown_fields`). Every start of 0.4.6 and 0.4.7 that meets a
    /// journal a later build wrote reads its header this way, and a downgrade
    /// passes no manifest, so this is the one check there is.
    ///
    /// MUTATION: make `HeaderWire::written_by` a required `String` (0.4.7's
    /// bytes no longer read whole), or write the header as `v: 2` (0.4.6 and
    /// 0.4.7 refuse every later journal).
    #[test]
    fn a_0_4_6_or_0_4_7_reader_reads_a_header_that_names_its_writer_as_it_always_did() {
        for phase in [
            Phase::Allocated,
            Phase::Moving,
            Phase::RollbackIntent {
                trial: Some(TRIAL),
                trial_started: false,
            },
            Phase::Retired {
                outcome: Outcome::Committed,
                untried: false,
            },
        ] {
            let ours = journal(phase.clone(), members_layout());
            let bytes = ours.encode();
            let named: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(named["written_by"], crate::version::VERSION);
            let old = header_as_0_4_6_and_0_4_7_read_it(&bytes)
                .unwrap_or_else(|| panic!("{phase:?}: 0.4.6 and 0.4.7 read it"));
            let header = ours.header();
            assert_eq!(
                old,
                HeaderWire046 {
                    v: 1,
                    txn: header.txn,
                    rescue: header.rescue.clone(),
                    class: header.class.word().to_owned(),
                    outcome: header.outcome.word().to_owned(),
                }
            );
            assert_eq!(Class::from_word(&old.class), Ok(header.class));
            assert_eq!(HeaderOutcome::from_word(&old.outcome), Ok(header.outcome));

            // 0.4.7's bytes: the same journal, no writer named.
            let unnamed = serde_json::to_vec(&JournalWire046 {
                header: HeaderWire046 {
                    v: 1,
                    txn: header.txn,
                    rescue: header.rescue.clone(),
                    class: header.class.word().to_owned(),
                    outcome: header.outcome.word().to_owned(),
                },
                body: &Body046 {
                    phase: phase.clone(),
                    layout: members_layout(),
                },
            })
            .unwrap();
            assert_eq!(sight(&unnamed), Sight::Known(ours), "{phase:?}");
        }
        assert_eq!(HEADER_VERSION, 1);
    }

    /// PIN (E5's grammar half; design §3(c) A2) — **the reserved trial's own
    /// commit re-encodes the body it read losslessly**: N is the one writer
    /// whose image is not O's, so the `Committed` it records over
    /// `TrialStarting` — from the bytes it read, as `commit_last_trial_as`
    /// reads them — is those bytes with three things changed and nothing
    /// else: the phase, the header's outcome, and the writer's version. Every
    /// other byte, the layout and an adapter that is not `Ours` included, is
    /// the byte O wrote, and N adds no field an O-image reader does not know.
    ///
    /// MUTATION: give `Phase::Committed` a field written by default, or record
    /// `LastTrialReady` with a fresh layout or the default adapter: the bytes
    /// N writes differ from O's outside the three.
    #[test]
    fn the_reserved_trials_commit_re_encodes_the_body_it_read_losslessly() {
        const OLDER: &str = "0.4.6";
        let receipt = Receipt {
            pid: TRIAL.pid,
            started: Some(TRIAL.started),
            ..valid_receipt()
        };
        let starting = Phase::TrialStarting {
            nonce: nonce(TRIAL_NONCE),
            began_ms: BEGAN,
        };
        let phase_bytes = |phase: &Phase| String::from_utf8(serde_json::to_vec(phase).unwrap());
        for (layout, adapter) in [
            (members_layout(), Adapter::Ours),
            (members_layout(), Adapter::Scoop),
            (bundle_layout(), Adapter::Homebrew),
        ] {
            let ours = journal(starting.clone(), layout).naming(adapter);
            // What O wrote: this build's bytes, as an older build signs them.
            let written_by = |version: &str| format!("\"written_by\":\"{version}\"");
            let o_bytes = String::from_utf8(ours.encode()).unwrap().replacen(
                &written_by(crate::version::VERSION),
                &written_by(OLDER),
                1,
            );
            let Sight::Known(read) = Role::LastTrialCommit.sight(o_bytes.as_bytes()) else {
                panic!("N reads O's journal whole: {o_bytes}");
            };
            let committed = read
                .advance(&Event::LastTrialReady {
                    receipt: receipt.clone(),
                    process: TRIAL,
                })
                .expect("its own exact receipt commits it");
            let n_bytes = String::from_utf8(committed.encode()).unwrap();
            let expected = o_bytes
                .replacen(
                    &phase_bytes(&starting).unwrap(),
                    &phase_bytes(&Phase::Committed).unwrap(),
                    1,
                )
                .replacen("\"outcome\":\"none\"", "\"outcome\":\"committed\"", 1)
                .replacen(&written_by(OLDER), &written_by(crate::version::VERSION), 1);
            assert_ne!(o_bytes, expected, "the three changes are made");
            assert_eq!(n_bytes, expected, "{adapter:?}: N changes nothing else");
        }
    }

    /// Which document a word is written in.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Document {
        /// The journal's frozen header, which every build reads.
        Header,
        /// The journal's body, which O's image reads.
        Body,
        /// The trial's receipt.
        Receipt,
    }

    /// Which closed vocabulary of the journal and the receipt a word is in.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Vocabulary {
        HeaderVersion,
        Envelope,
        Class,
        HeaderOutcome,
        Phase,
        RetiredOutcome,
        Layout,
        Adapter,
        ReceiptVersion,
    }

    impl Vocabulary {
        const fn document(self) -> Document {
            match self {
                Vocabulary::HeaderVersion
                | Vocabulary::Envelope
                | Vocabulary::Class
                | Vocabulary::HeaderOutcome => Document::Header,
                Vocabulary::Phase
                | Vocabulary::RetiredOutcome
                | Vocabulary::Layout
                | Vocabulary::Adapter => Document::Body,
                Vocabulary::ReceiptVersion => Document::Receipt,
            }
        }
    }

    /// Design §3(c): additive, or breaking for some reader that exists.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Kind {
        Additive,
        Breaking,
    }

    /// **Who writes a word** — O, the rescue copy of O's own image (P and
    /// R), and N — design §1.2's three images.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    struct Writers {
        old: bool,
        rescue: bool,
        new: bool,
    }

    impl Writers {
        const O: Self = Self {
            old: true,
            rescue: false,
            new: false,
        };
        const RESCUE: Self = Self {
            old: false,
            rescue: true,
            new: false,
        };
        const N: Self = Self {
            old: false,
            rescue: false,
            new: true,
        };

        const fn and(self, other: Self) -> Self {
            Self {
                old: self.old || other.old,
                rescue: self.rescue || other.rescue,
                new: self.new || other.new,
            }
        }

        /// The image `actor` runs: a lock holder is the rescue copy.
        const fn of(actor: Actor) -> Self {
            match actor {
                Actor::Old => Writers::O,
                Actor::Applier | Actor::Recovery => Writers::RESCUE,
                Actor::Trial => Writers::N,
                Actor::Start => Writers {
                    old: false,
                    rescue: false,
                    new: false,
                },
            }
        }
    }

    const O_RESCUE: Writers = Writers::O.and(Writers::RESCUE);
    const RESCUE_N: Writers = Writers::RESCUE.and(Writers::N);
    const EVERY: Writers = O_RESCUE.and(Writers::N);

    /// The words of [`Layout`], which carries data: an exhaustive projection,
    /// so a layout added there has no word until it has one here.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum LayoutWord {
        Members,
        Bundle,
        BundleIntent,
    }

    impl LayoutWord {
        fn of(layout: &Layout) -> Self {
            match layout {
                Layout::Members(_) => LayoutWord::Members,
                Layout::Bundle { .. } => LayoutWord::Bundle,
                Layout::BundleIntent { .. } => LayoutWord::BundleIntent,
            }
        }
    }

    /// **One word of the grammar, as its type's value.**
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Of {
        HeaderVersion,
        WrittenBy,
        Class(Class),
        Outcome(HeaderOutcome),
        Phase(PhaseKind),
        Retired(Outcome),
        Layout(LayoutWord),
        Adapter(Adapter),
        ReceiptVersion,
    }

    impl Of {
        const fn vocabulary(self) -> Vocabulary {
            match self {
                Of::HeaderVersion => Vocabulary::HeaderVersion,
                Of::WrittenBy => Vocabulary::Envelope,
                Of::Class(_) => Vocabulary::Class,
                Of::Outcome(_) => Vocabulary::HeaderOutcome,
                Of::Phase(_) => Vocabulary::Phase,
                Of::Retired(_) => Vocabulary::RetiredOutcome,
                Of::Layout(_) => Vocabulary::Layout,
                Of::Adapter(_) => Vocabulary::Adapter,
                Of::ReceiptVersion => Vocabulary::ReceiptVersion,
            }
        }
    }

    /// One row of the grammar.
    #[derive(Debug)]
    struct Word {
        of: Of,
        /// The word on the wire.
        word: &'static str,
        /// The release it arrived in.
        since: &'static str,
        kind: Kind,
        writers: Writers,
    }

    const fn additive(of: Of, word: &'static str, since: &'static str, writers: Writers) -> Word {
        Word {
            of,
            word,
            since,
            kind: Kind::Additive,
            writers,
        }
    }

    const fn breaking(of: Of, word: &'static str, since: &'static str, writers: Writers) -> Word {
        Word {
            of,
            word,
            since,
            kind: Kind::Breaking,
            writers,
        }
    }

    /// **The row of each word** — one exhaustive match over every vocabulary
    /// the journal and the receipt are read in (the header's class and
    /// outcome, the body's phase, retirement, layout and adapter, and the two
    /// versions). A word added to any of them does not compile until it has
    /// its row here, classified by design §3(c) of the escape hatch
    /// (`docs/DESIGN.md`, 2026-10-08) before it is written: a new class,
    /// outcome, header version or envelope change is forbidden; a breaking
    /// body or receipt word raises the release manifest's `min_updater`.
    const fn row(of: Of) -> Word {
        match of {
            Of::HeaderVersion => additive(of, "1", "0.4.6", EVERY),
            Of::WrittenBy => additive(of, "written_by", "0.4.8", EVERY),
            Of::Class(Class::Preparing) => additive(of, "preparing", "0.4.6", Writers::O),
            Of::Class(Class::Deferred) => additive(of, "deferred", "0.4.6", O_RESCUE),
            Of::Class(Class::Destructive) => additive(of, "destructive", "0.4.6", EVERY),
            Of::Class(Class::Terminal) => additive(of, "terminal", "0.4.6", O_RESCUE),
            Of::Outcome(HeaderOutcome::None) => additive(of, "none", "0.4.6", O_RESCUE),
            Of::Outcome(HeaderOutcome::Committed) => additive(of, "committed", "0.4.6", RESCUE_N),
            Of::Outcome(HeaderOutcome::RolledBack) => {
                additive(of, "rolled_back", "0.4.6", Writers::RESCUE)
            }
            Of::Phase(PhaseKind::Allocated) => additive(of, "Allocated", "0.4.6", Writers::O),
            Of::Phase(PhaseKind::Prepared) => additive(of, "Prepared", "0.4.6", O_RESCUE),
            Of::Phase(PhaseKind::Handoff) => additive(of, "Handoff", "0.4.6", Writers::O),
            Of::Phase(PhaseKind::Armed) => additive(of, "Armed", "0.4.6", Writers::RESCUE),
            Of::Phase(PhaseKind::Moving) => additive(of, "Moving", "0.4.6", Writers::RESCUE),
            Of::Phase(PhaseKind::TrialStarting) => {
                additive(of, "TrialStarting", "0.4.7", Writers::RESCUE)
            }
            Of::Phase(PhaseKind::Trial) => additive(of, "Trial", "0.4.6", Writers::RESCUE),
            Of::Phase(PhaseKind::Committed) => additive(of, "Committed", "0.4.6", RESCUE_N),
            Of::Phase(PhaseKind::RollbackIntent) => {
                additive(of, "RollbackIntent", "0.4.6", Writers::RESCUE)
            }
            Of::Phase(PhaseKind::Stuck) => additive(of, "Stuck", "0.4.6", Writers::RESCUE),
            Of::Phase(PhaseKind::RolledBack) => {
                additive(of, "RolledBack", "0.4.6", Writers::RESCUE)
            }
            Of::Phase(PhaseKind::Abandoned) => additive(of, "Abandoned", "0.4.6", O_RESCUE),
            Of::Phase(PhaseKind::Retired) => additive(of, "Retired", "0.4.6", Writers::RESCUE),
            Of::Retired(Outcome::Committed) => additive(of, "Committed", "0.4.6", Writers::RESCUE),
            Of::Retired(Outcome::RolledBack) => {
                additive(of, "RolledBack", "0.4.6", Writers::RESCUE)
            }
            // N re-encodes the layout it read when it commits itself (U-35).
            Of::Layout(LayoutWord::Members) => additive(of, "Members", "0.4.6", EVERY),
            Of::Layout(LayoutWord::Bundle) => additive(of, "Bundle", "0.4.6", EVERY),
            Of::Layout(LayoutWord::BundleIntent) => {
                additive(of, "BundleIntent", "0.4.6", Writers::O)
            }
            // Absent on the wire: what a 0.4.6 body, which names none, reads as.
            Of::Adapter(Adapter::Ours) => additive(of, "Ours", "0.4.7", EVERY),
            // A 0.4.6 reader takes these for `Ours` (design §3(d)): breaking
            // for it alone, met only by a downgrade to 0.4.6 with the macOS
            // rescue clone missing. Their roads are off, so nobody writes them
            // yet; the press would record one (O) and every later writer
            // carry it.
            Of::Adapter(Adapter::Homebrew) => breaking(of, "Homebrew", "0.4.7", EVERY),
            Of::Adapter(Adapter::Scoop) => breaking(of, "Scoop", "0.4.7", EVERY),
            Of::Adapter(Adapter::Winget) => breaking(of, "Winget", "0.4.7", EVERY),
            Of::ReceiptVersion => additive(of, "1", "0.4.6", Writers::N),
        }
    }

    /// The classes, the outcomes, the retirements and the adapters, each
    /// listed once; [`PhaseKind::ALL`] lists the phases.
    const CLASSES: [Class; 4] = [
        Class::Preparing,
        Class::Deferred,
        Class::Destructive,
        Class::Terminal,
    ];
    const OUTCOMES: [HeaderOutcome; 3] = [
        HeaderOutcome::None,
        HeaderOutcome::Committed,
        HeaderOutcome::RolledBack,
    ];
    const RETIREMENTS: [Outcome; 2] = [Outcome::Committed, Outcome::RolledBack];
    const LAYOUTS: [LayoutWord; 3] = [
        LayoutWord::Members,
        LayoutWord::Bundle,
        LayoutWord::BundleIntent,
    ];
    const ADAPTERS: [Adapter; 4] = [
        Adapter::Ours,
        Adapter::Homebrew,
        Adapter::Scoop,
        Adapter::Winget,
    ];

    /// **The journal's and the receipt's grammar** — every word's row
    /// ([`row`]): its document and vocabulary, the word, the release it
    /// arrived in, additive or breaking, and its writers.
    fn grammar() -> Vec<Word> {
        let mut words = vec![row(Of::HeaderVersion), row(Of::WrittenBy)];
        words.extend(CLASSES.map(|class| row(Of::Class(class))));
        words.extend(OUTCOMES.map(|outcome| row(Of::Outcome(outcome))));
        words.extend(PhaseKind::ALL.map(|phase| row(Of::Phase(phase))));
        words.extend(RETIREMENTS.map(|outcome| row(Of::Retired(outcome))));
        words.extend(LAYOUTS.map(|layout| row(Of::Layout(layout))));
        words.extend(ADAPTERS.map(|adapter| row(Of::Adapter(adapter))));
        words.push(row(Of::ReceiptVersion));
        words
    }

    /// RED (E1, E1-a2) — **every word of the journal's and the receipt's
    /// grammar has its row** (document and vocabulary, word, the release it
    /// arrived in, additive or breaking, its writers), **its writers are the
    /// ones the writer table allows, and the class and outcome vocabularies
    /// are closed at four words and three.**
    ///
    /// Each row comes from one exhaustive match ([`row`]), so a variant added
    /// to `Class`, `HeaderOutcome`, `PhaseKind` (and so `Phase`), `Outcome`,
    /// `Layout` or `Adapter` does not compile until it has its row; and each
    /// vocabulary's count is pinned here — 4 classes, 3 outcomes (design
    /// §3(c): a fifth class or a fourth outcome is read by every 0.4.6 and
    /// 0.4.7 start as a journal it cannot read, for ever), 13 phases, 2
    /// retirements, 3 layouts, 4 adapters: design §1.1's 29 words. Each row's
    /// word is the word this build's own serialiser and parser use. The
    /// writers of a phase are [`JOURNAL_WRITERS`]'s, and a class's or an
    /// outcome's are those of the phases that project to it.
    ///
    /// MUTATION: add a variant `Class::Paused` (or `Adapter::Nix`): the build
    /// fails at `row` (and at `Class::word`); add `LastTrialReady`'s writer N
    /// to `Prepared` in `JOURNAL_WRITERS`: the `Prepared` row's writers
    /// differ.
    #[test]
    fn every_grammar_word_has_its_row_and_the_header_vocabularies_are_closed() {
        let grammar = grammar();
        let words = |vocabulary: Vocabulary| -> Vec<&Word> {
            grammar
                .iter()
                .filter(|row| row.of.vocabulary() == vocabulary)
                .collect()
        };
        for (vocabulary, count) in [
            (Vocabulary::HeaderVersion, 1),
            (Vocabulary::Envelope, 1),
            (Vocabulary::Class, 4),
            (Vocabulary::HeaderOutcome, 3),
            (Vocabulary::Phase, 13),
            (Vocabulary::RetiredOutcome, 2),
            (Vocabulary::Layout, 3),
            (Vocabulary::Adapter, 4),
            (Vocabulary::ReceiptVersion, 1),
        ] {
            assert_eq!(words(vocabulary).len(), count, "{vocabulary:?}");
        }
        assert_eq!(
            [
                Vocabulary::Class,
                Vocabulary::HeaderOutcome,
                Vocabulary::Phase,
                Vocabulary::RetiredOutcome,
                Vocabulary::Layout,
                Vocabulary::Adapter,
            ]
            .map(|vocabulary| words(vocabulary).len())
            .iter()
            .sum::<usize>(),
            29,
            "design §1.1's 29 words"
        );
        for (at, row) in grammar.iter().enumerate() {
            assert!(
                grammar[at + 1..].iter().all(|other| other.of != row.of),
                "{:?} has two rows",
                row.of
            );
            assert!(
                crate::update::Version::parse(row.since).is_some(),
                "{:?}: {}",
                row.of,
                row.since
            );
            assert_ne!(row.writers, Writers::default(), "{:?}", row.of);
            if row.kind == Kind::Breaking {
                assert_eq!(
                    row.of.vocabulary(),
                    Vocabulary::Adapter,
                    "{}: a breaking word outside the adapters needs its own ruling",
                    row.word
                );
            }
        }
        assert!(
            words(Vocabulary::Class)
                .iter()
                .chain(words(Vocabulary::HeaderOutcome).iter())
                .chain(words(Vocabulary::HeaderVersion).iter())
                .chain(words(Vocabulary::Envelope).iter())
                .all(|row| row.kind == Kind::Additive
                    && row.of.vocabulary().document() == Document::Header),
            "the header's words are closed and additive"
        );

        // The words on the wire, as this build writes and reads them.
        for class in CLASSES {
            let row = row(Of::Class(class));
            assert_eq!(class.word(), row.word);
            assert_eq!(Class::from_word(row.word), Ok(class));
        }
        for outcome in OUTCOMES {
            let row = row(Of::Outcome(outcome));
            assert_eq!(outcome.word(), row.word);
            assert_eq!(HeaderOutcome::from_word(row.word), Ok(outcome));
        }
        for phase in phase_samples() {
            let wire = serde_json::to_value(&phase).unwrap();
            assert_eq!(
                wire["phase"],
                row(Of::Phase(phase.kind())).word,
                "{phase:?}"
            );
        }
        for outcome in RETIREMENTS {
            assert_eq!(
                serde_json::to_value(outcome).unwrap(),
                row(Of::Retired(outcome)).word
            );
        }
        for layout in [
            members_layout(),
            bundle_layout(),
            Layout::BundleIntent {
                old: old_bundle(),
                to_version: "0.4.7".to_owned(),
            },
        ] {
            assert_eq!(
                serde_json::to_value(&layout).unwrap()["layout"],
                row(Of::Layout(LayoutWord::of(&layout))).word
            );
        }
        for adapter in ADAPTERS {
            assert_eq!(
                serde_json::to_value(adapter).unwrap(),
                row(Of::Adapter(adapter)).word
            );
        }
        assert_eq!(HEADER_VERSION.to_string(), row(Of::HeaderVersion).word);
        assert_eq!(RECEIPT_VERSION.to_string(), row(Of::ReceiptVersion).word);
        let header: serde_json::Value =
            serde_json::from_slice(&journal(Phase::Moving, members_layout()).header().encode())
                .unwrap();
        assert!(header.get(row(Of::WrittenBy).word).is_some());

        // The writers, from the table that enforces them.
        let phase_writers = |phase: PhaseKind| -> Writers {
            JOURNAL_WRITERS
                .iter()
                .filter(|(kind, _)| *kind == phase)
                .flat_map(|(_, actors)| actors.iter())
                .fold(Writers::default(), |writers, actor| {
                    writers.and(Writers::of(*actor))
                })
        };
        for phase in PhaseKind::ALL {
            assert_eq!(
                row(Of::Phase(phase)).writers,
                phase_writers(phase),
                "{phase:?}"
            );
        }
        for class in CLASSES {
            let writers = PhaseKind::ALL
                .into_iter()
                .filter(|phase| phase.class() == class)
                .fold(Writers::default(), |writers, phase| {
                    writers.and(phase_writers(phase))
                });
            assert_eq!(row(Of::Class(class)).writers, writers, "{class:?}");
        }
        for outcome in OUTCOMES {
            let writers = phase_samples()
                .into_iter()
                .filter(|phase| phase.outcome() == outcome)
                .fold(Writers::default(), |writers, phase| {
                    writers.and(phase_writers(phase.kind()))
                });
            assert_eq!(row(Of::Outcome(outcome)).writers, writers, "{outcome:?}");
        }
        for outcome in RETIREMENTS {
            assert_eq!(
                row(Of::Retired(outcome)).writers,
                phase_writers(PhaseKind::Retired)
            );
        }
        let journal_writers = PhaseKind::ALL
            .into_iter()
            .fold(Writers::default(), |writers, phase| {
                writers.and(phase_writers(phase))
            });
        assert_eq!(row(Of::HeaderVersion).writers, journal_writers);
        assert_eq!(row(Of::WrittenBy).writers, journal_writers);
        for row in &grammar {
            if row.of.vocabulary().document() == Document::Body {
                assert_eq!(
                    row.writers.and(journal_writers),
                    journal_writers,
                    "{:?}: only a journal writer writes a body word",
                    row.of
                );
            }
        }
        assert_eq!(row(Of::ReceiptVersion).writers, Writers::N);
    }
}

/// **The call-site registry of the journal's and the receipt's reads**
/// (0.4.8 E1-a2; design note §1.3a, rev 2 R2-1, R2-15).
///
/// Guard (source-reading by design): its subject is where the product reads
/// a journal or a receipt, so it reads the product's code through
/// `bt-source`'s index — the product view, test items excluded — and binds to
/// no file.
///
/// # What it holds the product to
///
/// [`SITES`] is the one table of every product call, outside `#[cfg(test)]`,
/// of `Header::parse`, `Journal::parse`, `Receipt::parse`, [`sight`] and
/// [`Role::sight`] (one call shape, `sight(`), [`sight_of_read`] and
/// [`Role::sight_of_read`] (another), [`receipt_sight`] and this
/// module's own `json` — and of any `serde_json::from_*` in an updater module
/// (one whose path has a segment beginning `update`). Each row names the item
/// the calls stand in, how many there are, what the item is — a reader by its
/// [`Role`], one of this module's own parsers, the trial reading the receipt
/// it has just made itself, or a reading of something that is not a journal —
/// and the test that pins it. The guard fails, naming the table, when:
///
/// * a call stands in an item with no row, or a row's count is not the
///   code's (a new site, a removed one);
/// * a reader's item does not name exactly its row's role in its code
///   (`Role::<role>`, comments aside) — a site mapped to the wrong role, or a
///   reader that reads by one role and answers by another;
/// * a row's pinning test is not a `#[test]` function at the path it names.
///
/// The twenty sites of §1.3a are rows here by their ids; H4+J6 and H5+J8 are
/// one read each since E1-a1, J1 and J2 are the start's one read (H1), and
/// `P1` is `update_prepare::journal_there`, E1-a1's read for `Stop::Newer`.
/// X1, the trial's parse of the receipt it has just made, stays excluded from
/// the roles as the note says, with its own row.
///
/// # What it does not see
///
/// A parser renamed by `use … as …` is not the path it searches for, and a
/// raw `serde_json` read outside the updater's modules is outside its scope.
/// Review owns both.
#[cfg(test)]
mod parse_sites {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;

    use bt_source::{
        DiskScope, Index, ItemIdentity, ItemQuery, Pattern, Search, TargetId, TargetKind,
        TargetRoot, Universe, Vendor, View, needle, report,
    };

    use super::Role;

    /// What a registered call reads with.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub(super) enum Reads {
        HeaderParse,
        JournalParse,
        ReceiptParse,
        /// `sight(` — the free function and [`Role::sight`].
        Sight,
        /// `sight_of_read(` — the free function and [`Role::sight_of_read`]
        /// (E1 round 2: one read of the file, its error included).
        SightOfRead,
        ReceiptSight,
        /// This module's own `json`, which every parser goes through.
        Json,
        /// A `serde_json::from_*` in an updater module.
        SerdeJson,
    }

    /// What an item that reads is.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Is {
        /// A reader of bytes another build may have written, by its role.
        Reader(Role),
        /// One of `update_txn`'s own parsers: the grammar itself.
        Owner,
        /// The trial parsing the receipt it has just made (the note's X1).
        OwnBytes,
        /// A `serde_json` read of something that is not a journal or a
        /// receipt, said in words.
        NotJournal(&'static str),
    }

    /// One row of the registry.
    pub(super) struct Site {
        /// The note's id (§1.3a), or the owner's own name for the rest.
        pub(super) id: &'static str,
        /// `crate::module::Type::item`, as the index names it.
        pub(super) item: &'static str,
        pub(super) reads: Reads,
        pub(super) count: usize,
        pub(super) is: Is,
        /// `crate::module::tests::name` — a `#[test]` function.
        pub(super) pinned_by: &'static str,
    }

    const fn site(
        id: &'static str,
        item: &'static str,
        reads: Reads,
        count: usize,
        is: Is,
        pinned_by: &'static str,
    ) -> Site {
        Site {
            id,
            item,
            reads,
            count,
            is,
            pinned_by,
        }
    }

    /// **The registry.** A new read gets its row here, with its role and the
    /// test that feeds it an unknown header word, an unknown body word and
    /// bytes that are no journal (design note §5, the acceptance paragraph).
    pub(super) const SITES: &[Site] = &[
        site(
            "H1+J1+J2",
            "crate::update_startup::run",
            Reads::SightOfRead,
            1,
            Is::Reader(Role::Start),
            "crate::update_startup::tests::on_disk::the_start_acts_on_the_header_of_what_it_cannot_read_whole",
        ),
        site(
            "H2",
            "crate::update_txn::trial_sight",
            Reads::Sight,
            1,
            Is::Reader(Role::TrialWatch),
            "crate::update_txn::tests::the_trial_watch_reads_what_it_cannot_read_whole_by_its_header",
        ),
        site(
            "H3",
            "crate::update_trial::hand_back",
            Reads::SightOfRead,
            1,
            Is::Reader(Role::TrialHandBack),
            "crate::update_apply_windows::tests::a_trial_hands_back_what_it_cannot_read_whole_to_the_rescue_its_header_names",
        ),
        site(
            "H4+J6",
            "crate::update_apply_windows::opens_now",
            Reads::SightOfRead,
            1,
            Is::Reader(Role::WindowsExit),
            "crate::update_apply_windows::tests::the_windows_exit_opens_the_rescue_over_what_it_cannot_read_whole",
        ),
        site(
            "H5+J8",
            "crate::update_apply_macos::opens_now_with",
            Reads::SightOfRead,
            1,
            Is::Reader(Role::MacExit),
            "crate::update_apply_macos::tests::the_macos_exit_never_opens_the_installed_build_plainly_over_what_it_cannot_read",
        ),
        site(
            "H6",
            "crate::update_recover::header_of",
            Reads::SightOfRead,
            1,
            Is::Reader(Role::RecoveryDoor),
            "crate::update_recover::tests::the_door_never_opens_the_installed_build_plainly_over_what_it_cannot_read",
        ),
        site(
            "H7",
            "crate::update_handoff::OldLeave::opening",
            Reads::SightOfRead,
            1,
            Is::Reader(Role::OutgoingExit),
            "crate::update_handoff::tests::the_old_build_leaves_what_it_cannot_read_whole_by_its_header",
        ),
        site(
            "J10",
            "crate::update_handoff::OldLeave::fallback",
            Reads::SightOfRead,
            1,
            Is::Reader(Role::OutgoingExit),
            "crate::update_handoff::tests::the_old_build_leaves_what_it_cannot_read_whole_by_its_header",
        ),
        site(
            "J3",
            "crate::update_apply::reserve_last_trial",
            Reads::SightOfRead,
            1,
            Is::Reader(Role::LastTrialReserve),
            "crate::update_apply::beyond_tests::the_reservation_stands_aside_from_what_it_cannot_read_whole",
        ),
        site(
            "J4",
            "crate::update_apply::commit_last_trial_as",
            Reads::SightOfRead,
            1,
            Is::Reader(Role::LastTrialCommit),
            "crate::update_apply::beyond_tests::the_reserved_trial_stands_aside_from_what_it_cannot_read_whole",
        ),
        site(
            "J5",
            "crate::update_apply::read_window_phase",
            Reads::SightOfRead,
            1,
            Is::Reader(Role::WindowElection),
            "crate::update_apply::beyond_tests::the_window_election_stands_aside_from_what_it_cannot_read_whole",
        ),
        site(
            "J7",
            "crate::update_apply_windows::read_journal",
            Reads::SightOfRead,
            1,
            Is::Reader(Role::WindowsHolder),
            "crate::update_apply_windows::tests::the_windows_lock_holder_stands_aside_from_what_it_cannot_read_whole",
        ),
        site(
            "J9",
            "crate::update_apply_macos::Txn::hold",
            Reads::SightOfRead,
            1,
            Is::Reader(Role::MacHolder),
            "crate::update_apply_macos::tests::the_macos_lock_holder_stands_aside_from_what_it_cannot_read_whole",
        ),
        site(
            "J11",
            "crate::update_prepare::at_launch",
            Reads::SightOfRead,
            1,
            Is::Reader(Role::JobOwner),
            "crate::update_prepare::tests::the_job_owner_leaves_what_it_cannot_read_whole_and_the_press_says_why",
        ),
        site(
            "P1",
            "crate::update_prepare::journal_there",
            Reads::SightOfRead,
            1,
            Is::Reader(Role::JobOwner),
            "crate::update_prepare::tests::the_job_owner_leaves_what_it_cannot_read_whole_and_the_press_says_why",
        ),
        site(
            "R1",
            "crate::update_apply::read_receipt",
            Reads::ReceiptSight,
            1,
            Is::Reader(Role::WindowsReceiptWatch),
            "crate::update_apply::beyond_tests::the_receipt_watch_never_accepts_what_it_cannot_read",
        ),
        site(
            "R2",
            "crate::update_apply_macos::read_receipt",
            Reads::ReceiptSight,
            1,
            Is::Reader(Role::MacReceiptWatch),
            "crate::update_apply_macos::tests::the_macos_receipt_watch_never_accepts_what_it_cannot_read",
        ),
        site(
            "X1",
            "crate::update_trial::write_receipt",
            Reads::ReceiptParse,
            1,
            Is::OwnBytes,
            "crate::update_trial::tests::a_receipt_this_build_cannot_read_is_never_written_over",
        ),
        site(
            "sight",
            "crate::update_txn::Role::sight",
            Reads::Sight,
            1,
            Is::Owner,
            "crate::update_txn::tests::a_sight_names_what_reads_and_its_writer_changes_no_action",
        ),
        site(
            "sight",
            "crate::update_txn::sight_as",
            Reads::JournalParse,
            1,
            Is::Owner,
            "crate::update_txn::tests::a_sight_names_what_reads_and_its_writer_changes_no_action",
        ),
        site(
            "sight",
            "crate::update_txn::sight_as",
            Reads::HeaderParse,
            1,
            Is::Owner,
            "crate::update_txn::tests::a_sight_names_what_reads_and_its_writer_changes_no_action",
        ),
        site(
            "sight",
            "crate::update_txn::sight_as",
            Reads::Json,
            1,
            Is::Owner,
            "crate::update_txn::tests::a_sight_names_what_reads_and_its_writer_changes_no_action",
        ),
        site(
            "receipt_sight",
            "crate::update_txn::receipt_sight",
            Reads::ReceiptParse,
            1,
            Is::Owner,
            "crate::update_apply::beyond_tests::the_receipt_watch_never_accepts_what_it_cannot_read",
        ),
        site(
            "Journal::parse",
            "crate::update_txn::Journal::parse",
            Reads::HeaderParse,
            1,
            Is::Owner,
            "crate::update_txn::tests::a_header_class_that_is_unknown_or_disagrees_with_its_phase_is_refused",
        ),
        site(
            "Journal::parse",
            "crate::update_txn::Journal::parse",
            Reads::Json,
            1,
            Is::Owner,
            "crate::update_txn::tests::a_header_class_that_is_unknown_or_disagrees_with_its_phase_is_refused",
        ),
        site(
            "Header::parse",
            "crate::update_txn::Header::parse",
            Reads::Json,
            1,
            Is::Owner,
            "crate::update_txn::tests::a_header_of_an_unknown_version_is_refused_by_its_version",
        ),
        site(
            "Receipt::parse",
            "crate::update_txn::Receipt::parse",
            Reads::Json,
            1,
            Is::Owner,
            "crate::update_txn::tests::a_receipt_truncated_versioned_or_of_the_wrong_length_is_refused",
        ),
        site(
            "versioned",
            "crate::update_txn::versioned",
            Reads::Json,
            1,
            Is::Owner,
            "crate::update_txn::tests::a_header_of_an_unknown_version_is_refused_by_its_version",
        ),
        site(
            "json",
            "crate::update_txn::json",
            Reads::SerdeJson,
            1,
            Is::Owner,
            "crate::update_txn::tests::a_truncated_header_is_refused_as_truncated",
        ),
        site(
            "newest_tag",
            "crate::update::newest_tag",
            Reads::SerdeJson,
            1,
            Is::NotJournal("the release list a feed answers"),
            "crate::update::tests::a_release_list_yields_its_highest_version_or_nothing",
        ),
        site(
            "Feed::releases",
            "crate::update::Feed::releases",
            Reads::SerdeJson,
            1,
            Is::NotJournal("a local feed's release list"),
            "crate::update::tests::an_unreadable_or_malformed_feed_is_a_failed_check_not_a_panic",
        ),
        site(
            "sight_of_read",
            "crate::update_txn::Role::sight_of_read",
            Reads::SightOfRead,
            1,
            Is::Owner,
            "crate::update_txn::tests::a_sight_names_what_reads_and_its_writer_changes_no_action",
        ),
        site(
            "sight_of_read",
            "crate::update_txn::sight_of_read",
            Reads::Sight,
            1,
            Is::Owner,
            "crate::update_txn::tests::a_sight_names_what_reads_and_its_writer_changes_no_action",
        ),
    ];

    /// What the guard says every refusal is about.
    const TABLE: &str = "the call-site registry `update_txn::parse_sites::SITES`";

    /// The module `json`, and every `serde_json` read, are looked for in.
    const OWNER: &str = "crate::update_txn";

    /// `crate::module::Type::item` for `identity`.
    fn key(identity: &ItemIdentity) -> String {
        match &identity.type_owner {
            Some(owner) => format!("{}::{owner}::{}", identity.module_path, identity.name),
            None => format!("{}::{}", identity.module_path, identity.name),
        }
    }

    /// Whether `module_path` is one of the updater's modules.
    fn updater(module_path: &str) -> bool {
        module_path
            .split("::")
            .any(|segment| segment.starts_with("update"))
    }

    /// **What the product calls, item by item**: every product occurrence of
    /// `pattern`, counted by the item it stands in (a closure's is its
    /// function's), with `exempt` declarations taken out. An occurrence in no
    /// function at all is a failure of its own when `failures` is given.
    fn product_calls(
        index: &Index,
        pattern: Pattern,
        exempt: &[ItemQuery],
        failures: Option<&mut Vec<String>>,
    ) -> BTreeMap<String, usize> {
        let mut search = Search::new(needle!(pattern), View::Identifiers);
        for item in exempt {
            search = search.exempting_declarations_of(item.clone());
        }
        let found = index
            .search(&search)
            .unwrap_or_else(|failure| panic!("{failure}"))
            .in_the_product(index);
        let outside = found.outside_items(index);
        if let Some(failures) = failures
            && outside > 0
        {
            failures.push(format!(
                "{} product occurrence(s) of {} stand in no function: a read in a `const` or \
                 a `static` has no row in {TABLE}",
                outside,
                found.report(index)
            ));
        }
        let mut calls = BTreeMap::new();
        for (identity, count) in found.owners(index) {
            *calls.entry(key(&identity)).or_insert(0) += count;
        }
        calls
    }

    /// Every `(item, reads)` the product has, with its count.
    fn observed(index: &Index, failures: &mut Vec<String>) -> BTreeMap<(String, Reads), usize> {
        let in_owner = |name: &str| ItemQuery::function(name).in_module(OWNER);
        let shapes: Vec<(Reads, Pattern, Vec<ItemQuery>)> = vec![
            (Reads::HeaderParse, Pattern::path("Header::parse"), vec![]),
            (Reads::JournalParse, Pattern::path("Journal::parse"), vec![]),
            (Reads::ReceiptParse, Pattern::path("Receipt::parse"), vec![]),
            (
                Reads::Sight,
                Pattern::call("sight"),
                vec![
                    in_owner("sight"),
                    ItemQuery::method("Role", "sight").in_module(OWNER),
                ],
            ),
            (
                Reads::SightOfRead,
                Pattern::call("sight_of_read"),
                vec![
                    in_owner("sight_of_read"),
                    ItemQuery::method("Role", "sight_of_read").in_module(OWNER),
                ],
            ),
            (
                Reads::ReceiptSight,
                Pattern::call("receipt_sight"),
                vec![in_owner("receipt_sight")],
            ),
            (Reads::Json, Pattern::call("json"), vec![in_owner("json")]),
        ];
        let mut seen = BTreeMap::new();
        for (reads, pattern, exempt) in shapes {
            for (item, count) in product_calls(index, pattern, &exempt, Some(failures)) {
                // `json` is this module's: the same name elsewhere is another
                // function.
                if reads != Reads::Json || item.starts_with(&format!("{OWNER}::")) {
                    seen.insert((item, reads), count);
                }
            }
        }
        for name in ["from_slice", "from_str", "from_reader", "from_value"] {
            let pattern = Pattern::path(&format!("serde_json::{name}"));
            for (item, count) in product_calls(index, pattern, &[], Some(failures)) {
                if updater(item.rsplit_once("::").map_or(&*item, |(module, _)| module)) {
                    *seen.entry((item, Reads::SerdeJson)).or_insert(0) += count;
                }
            }
        }
        seen
    }

    /// The journal roles each function names in its product code — a table
    /// that lists them, such as `Role::ALL`, is no reader.
    fn roles_named(index: &Index) -> BTreeMap<String, BTreeSet<String>> {
        let mut named: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for role in Role::ALL {
            let role = format!("{role:?}");
            let pattern = Pattern::path(&format!("Role::{role}"));
            for item in product_calls(index, pattern, &[], None).into_keys() {
                named.entry(item).or_default().insert(role.clone());
            }
        }
        named
    }

    /// Whether `path` names one `#[test]` function.
    fn a_test_at(index: &Index, path: &str) -> Result<(), String> {
        let (module, name) = path
            .rsplit_once("::")
            .ok_or_else(|| format!("`{path}` is not a path"))?;
        let record = index
            .one(&ItemQuery::function(name).in_module(module))
            .map_err(|failure| failure.to_string())?;
        let declaration = index.text(record.declaration());
        if declaration.contains("#[test]") {
            Ok(())
        } else {
            Err(format!("`{path}` is a function and not a `#[test]`"))
        }
    }

    /// **The guard**: every difference between the product's reads in
    /// `index` and `table`, each naming the table.
    pub(super) fn judge(index: &Index, table: &[Site]) -> Vec<String> {
        let mut failures = Vec::new();
        let seen = observed(index, &mut failures);
        let mut rows: BTreeMap<(String, Reads), &Site> = BTreeMap::new();
        for row in table {
            if rows.insert((row.item.to_owned(), row.reads), row).is_some() {
                failures.push(format!(
                    "{} {:?} has two rows in {TABLE}",
                    row.item, row.reads
                ));
            }
        }
        for ((item, reads), count) in &seen {
            match rows.get(&(item.clone(), *reads)) {
                Some(row) if row.count == *count => {}
                Some(row) => failures.push(format!(
                    "{item} reads with {reads:?} {count} time(s) in the code and {} in {TABLE} \
                     (row {})",
                    row.count, row.id
                )),
                None => failures.push(format!(
                    "{item} reads with {reads:?} {count} time(s) and has no row in {TABLE}: a new \
                     read of a journal or a receipt takes its role in `update_txn::Role` and \
                     its row, with the test that pins it"
                )),
            }
        }
        for ((item, reads), row) in &rows {
            if !seen.contains_key(&(item.clone(), *reads)) {
                failures.push(format!(
                    "row {} of {TABLE}, {item} reading with {reads:?}, is no longer in the code: \
                     delete the row",
                    row.id
                ));
            }
        }
        let named = roles_named(index);
        for row in table {
            let names: Vec<&String> = named.get(row.item).into_iter().flatten().collect();
            match row.is {
                Is::Reader(role) => {
                    let role = format!("{role:?}");
                    if names != [&role] {
                        failures.push(format!(
                            "row {} of {TABLE} maps {} to `Role::{role}`, and its code names \
                             {names:?}: a reader reads by its one role (`Role::{role}.sight(..)`, \
                             `receipt_sight(..).known(Role::{role})`)",
                            row.id, row.item
                        ));
                    }
                }
                Is::Owner | Is::OwnBytes | Is::NotJournal(_) => {
                    if !names.is_empty() {
                        failures.push(format!(
                            "row {} of {TABLE} says {} is no reader, and its code names \
                             {names:?}",
                            row.id, row.item
                        ));
                    }
                }
            }
            if let Err(why) = a_test_at(index, row.pinned_by) {
                failures.push(format!(
                    "row {} of {TABLE} is pinned by `{}`, and {why}",
                    row.id, row.pinned_by
                ));
            }
        }
        failures
    }

    /// RED (E1-a2) — **every product read of a journal or a receipt is a row
    /// of the registry, with its count, its role and a test that exists**
    /// (design note §5, the acceptance paragraph: "adding a parse call outside
    /// the inventory fails the guard").
    ///
    /// MUTATION (observed in the report): add `let _ = Journal::parse(&bytes);`
    /// to `update_prepare::at_launch`; map row J9 to `Role::MacExit`; rename
    /// `the_door_never_opens_the_installed_build_plainly_over_what_it_cannot_read`
    /// without its row. Each names the table.
    #[test]
    fn every_read_of_a_journal_or_a_receipt_is_a_row_of_the_registry() {
        let failures = judge(Index::of_package("bt-app"), SITES);
        assert!(
            failures.is_empty(),
            "the product's reads of the journal and the receipt and {TABLE} differ:\n  {}",
            failures.join("\n  ")
        );
        let readers = SITES
            .iter()
            .filter(|row| matches!(row.is, Is::Reader(_)))
            .count();
        println!(
            "the call-site registry: {} rows, {readers} readers",
            SITES.len()
        );
    }

    /// A small crate with one read of each kind the guard judges, planted
    /// where a mutation of the product would put it.
    const PLANTED: &str = r#"
mod update_txn {
    pub enum Role { Start, WindowsExit }
    pub struct Sight;
    pub struct Header;
    pub struct Journal;
    impl Header {
        pub fn parse(bytes: &[u8]) -> Result<Header, ()> { json(bytes) }
    }
    impl Journal {
        pub fn parse(bytes: &[u8]) -> Result<Journal, ()> {
            Header::parse(bytes)?;
            json(bytes)
        }
    }
    fn json<T>(bytes: &[u8]) -> Result<T, ()> { serde_json::from_slice(bytes).map_err(drop) }
    pub fn sight(bytes: &[u8]) -> Sight {
        let _ = Journal::parse(bytes);
        Sight
    }
    impl Role {
        pub fn sight(self, bytes: &[u8]) -> Sight { sight(bytes) }
    }
    pub fn receipt_sight(_bytes: &[u8]) -> Sight { Sight }
    pub fn sight_of_read(read: Result<Vec<u8>, ()>) -> Option<Sight> {
        read.ok().map(|bytes| sight(&bytes))
    }
    impl Role {
        pub fn sight_of_read(self, read: Result<Vec<u8>, ()>) -> Option<Sight> { sight_of_read(read) }
    }
}

mod update_reader {
    use crate::update_txn::{Journal, Role};

    pub fn registered(bytes: &[u8]) {
        let _ = Role::Start.sight(bytes);
    }

    pub fn by_another_role(bytes: &[u8]) {
        let _ = Role::WindowsExit.sight(bytes);
    }

    pub fn planted(bytes: &[u8]) {
        let _ = Journal::parse(bytes);
    }

    pub fn raw(bytes: &[u8]) {
        let _: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    }

    #[cfg(test)]
    fn a_test_helper(bytes: &[u8]) {
        let _ = Journal::parse(bytes);
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn pins_it() {}

        fn not_a_test() {}
    }
}
"#;

    /// The rows that describe [`PLANTED`] whole, but for what each test
    /// plants.
    fn planted_rows() -> Vec<Site> {
        let pin = "crate::update_reader::tests::pins_it";
        vec![
            site(
                "own",
                "crate::update_txn::Header::parse",
                Reads::Json,
                1,
                Is::Owner,
                pin,
            ),
            site(
                "own",
                "crate::update_txn::Journal::parse",
                Reads::HeaderParse,
                1,
                Is::Owner,
                pin,
            ),
            site(
                "own",
                "crate::update_txn::Journal::parse",
                Reads::Json,
                1,
                Is::Owner,
                pin,
            ),
            site(
                "own",
                "crate::update_txn::json",
                Reads::SerdeJson,
                1,
                Is::Owner,
                pin,
            ),
            site(
                "own",
                "crate::update_txn::sight",
                Reads::JournalParse,
                1,
                Is::Owner,
                pin,
            ),
            site(
                "own",
                "crate::update_txn::Role::sight",
                Reads::Sight,
                1,
                Is::Owner,
                pin,
            ),
            site(
                "own",
                "crate::update_txn::sight_of_read",
                Reads::Sight,
                1,
                Is::Owner,
                pin,
            ),
            site(
                "own",
                "crate::update_txn::Role::sight_of_read",
                Reads::SightOfRead,
                1,
                Is::Owner,
                pin,
            ),
            site(
                "S1",
                "crate::update_reader::registered",
                Reads::Sight,
                1,
                Is::Reader(Role::Start),
                pin,
            ),
            site(
                "S2",
                "crate::update_reader::by_another_role",
                Reads::Sight,
                1,
                Is::Reader(Role::WindowsExit),
                pin,
            ),
            site(
                "S3",
                "crate::update_reader::planted",
                Reads::JournalParse,
                1,
                Is::NotJournal("planted"),
                pin,
            ),
            site(
                "S4",
                "crate::update_reader::raw",
                Reads::SerdeJson,
                1,
                Is::NotJournal("planted"),
                pin,
            ),
        ]
    }

    /// The index of [`PLANTED`], written into a scratch folder of its own.
    fn planted_index() -> Index {
        let directory = bt_testpath::temp_path("e1a2-parse-sites");
        std::fs::create_dir_all(&directory).expect("a scratch folder");
        let root = directory.join("lib.rs");
        std::fs::write(&root, PLANTED).expect("the planted crate is written");
        let universe = Universe::declare(
            "the planted reads",
            vec![TargetRoot {
                id: TargetId {
                    package: "planted".to_owned(),
                    kind: TargetKind::Library,
                    name: "planted".to_owned(),
                },
                file: root,
            }],
            vec![DiskScope::under(&directory)],
            Vendor::Excluded,
        )
        .expect("the planted crate is where it was written");
        let index =
            Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
        let _ = std::fs::remove_dir_all(Path::new(&directory));
        index
    }

    /// RED (E1-a2) — **the guard sees a read the table lacks, a read by
    /// another role than its row's, a raw `serde_json` read in an updater
    /// module, and a pinning test that is gone; and passes the same crate
    /// whole.**
    ///
    /// Each violation is planted in a small crate ([`PLANTED`]), and each
    /// table below leaves out one fact; a read in a `#[cfg(test)]` helper is
    /// never one. The control is the table that describes the crate whole.
    ///
    /// MUTATION: drop `.in_the_product(index)` in `product_calls` (the test
    /// helper's read is counted); make `judge` skip the role check (the
    /// wrong role passes); make `a_test_at` answer `Ok` (the renamed test
    /// passes).
    #[test]
    fn the_registry_refuses_each_planted_read_and_passes_the_crate_whole() {
        let index = planted_index();
        assert_eq!(judge(&index, &planted_rows()), Vec::<String>::new());

        let without = |id: &str| -> Vec<Site> {
            planted_rows()
                .into_iter()
                .filter(|row| row.id != id)
                .collect()
        };
        let failures = judge(&index, &without("S3"));
        assert!(
            failures.len() == 1
                && failures[0].contains("crate::update_reader::planted reads with JournalParse")
                && failures[0].contains("has no row in")
                && failures[0].contains(TABLE),
            "{failures:#?}"
        );
        let failures = judge(&index, &without("S4"));
        assert!(
            failures.len() == 1
                && failures[0].contains("crate::update_reader::raw reads with SerdeJson"),
            "{failures:#?}"
        );

        let mut wrong = planted_rows();
        wrong.iter_mut().find(|row| row.id == "S2").expect("S2").is = Is::Reader(Role::Start);
        let failures = judge(&index, &wrong);
        assert!(
            failures.len() == 1
                && failures[0].contains("row S2")
                && failures[0].contains("`Role::Start`")
                && failures[0].contains("[\"WindowsExit\"]"),
            "{failures:#?}"
        );

        for gone in [
            "crate::update_reader::tests::pins_it_renamed",
            "crate::update_reader::tests::not_a_test",
        ] {
            let mut renamed = planted_rows();
            renamed
                .iter_mut()
                .find(|row| row.id == "S1")
                .expect("S1")
                .pinned_by = gone;
            let failures = judge(&index, &renamed);
            assert!(
                failures.len() == 1 && failures[0].contains("row S1") && failures[0].contains(gone),
                "{gone}: {failures:#?}"
            );
        }

        let mut counted = planted_rows();
        counted
            .iter_mut()
            .find(|row| row.id == "S1")
            .expect("S1")
            .count = 2;
        let failures = judge(&index, &counted);
        assert!(
            failures.len() == 1 && failures[0].contains("1 time(s) in the code and 2"),
            "{failures:#?}"
        );
    }
}
