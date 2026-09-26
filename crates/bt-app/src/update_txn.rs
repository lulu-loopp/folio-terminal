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
//! Only the [`Header`] (`{v, txn, rescue, class, outcome}`) and the [`Receipt`]
//! (`{v, txn, nonce, pid, version}`) are read by a build other than the one that
//! wrote them: an ordinary start of any later version reads the header, and the
//! rescue build (a copy of O) reads the receipt the new build wrote. They are
//! frozen at v1, carry their version, and refuse one they do not know by name.
//! The [`Body`] is written and read by the rescue build alone.
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
        }
    }
}

// ─────────────────────────────────── the receipt ───────────────────────────────────

/// **The trial's receipt, v1** — `{v, txn, nonce, pid, version}`, written by N
/// alone into `H\<txn>\health-<nonce>` once it has claimed the data directory,
/// read its settings and session and drawn its first text (§C.5, F-1, F-14).
///
/// It is evidence, not a decision: only the lock holder turns it into
/// `Committed`, and only while the journal says `Trial` ([`next`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Receipt {
    pub(crate) txn: TxnId,
    pub(crate) nonce: Nonce,
    pub(crate) pid: u32,
    pub(crate) version: String,
}

#[derive(Serialize, Deserialize)]
struct ReceiptWire {
    v: u64,
    txn: String,
    nonce: String,
    pid: u32,
    version: String,
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
        })
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(&ReceiptWire {
            v: RECEIPT_VERSION,
            txn: self.txn.to_string(),
            nonce: self.nonce.to_string(),
            pid: self.pid,
            version: self.version.clone(),
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
    Trial {
        nonce: Nonce,
        process: TrialProcess,
        /// Wall-clock milliseconds when P started N; the deadline counts from it.
        began_ms: u64,
    },
    Committed,
    RollbackIntent {
        trial: Option<TrialProcess>,
    },
    Stuck {
        trial: Option<TrialProcess>,
        last_error: String,
    },
    RolledBack,
    Abandoned,
    Retired {
        outcome: Outcome,
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
    Trial,
    Committed,
    RollbackIntent,
    Stuck,
    RolledBack,
    Abandoned,
    Retired,
}

impl PhaseKind {
    pub(crate) const ALL: [PhaseKind; 12] = [
        PhaseKind::Allocated,
        PhaseKind::Prepared,
        PhaseKind::Handoff,
        PhaseKind::Armed,
        PhaseKind::Moving,
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
            Phase::Trial { .. } => PhaseKind::Trial,
            Phase::Committed => PhaseKind::Committed,
            Phase::RollbackIntent { .. } => PhaseKind::RollbackIntent,
            Phase::Stuck { .. } => PhaseKind::Stuck,
            Phase::RolledBack => PhaseKind::RolledBack,
            Phase::Abandoned => PhaseKind::Abandoned,
            Phase::Retired { .. } => PhaseKind::Retired,
        }
    }

    pub(crate) fn class(&self) -> Class {
        self.kind().class()
    }

    /// **The header outcome of this phase** — what the lock holder writes into
    /// the header with it.
    pub(crate) fn outcome(&self) -> HeaderOutcome {
        match self {
            Phase::Retired {
                outcome: Outcome::Committed,
            } => HeaderOutcome::Committed,
            Phase::Retired {
                outcome: Outcome::RolledBack,
            } => HeaderOutcome::RolledBack,
            phase => phase.kind().outcome(),
        }
    }
}

// ───────────────────────────────────── the journal ─────────────────────────────────────

/// The journal's body, owned by the rescue build's version (F-8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Body {
    pub(crate) phase: Phase,
    pub(crate) layout: Layout,
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
            },
        }
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

    /// The journal after `event`, or why `event` cannot happen now.
    pub(crate) fn advance(&self, event: &Event) -> Result<Self, Refusal> {
        let phase = next(&self.txn, &self.body.phase, event)?;
        Ok(Self {
            body: Body {
                phase,
                layout: self.body.layout.clone(),
            },
            ..self.clone()
        })
    }
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
    /// The entrance is written, flushed and read back (F-2). The proof is
    /// `bt_platform::logon_hook::Armed`, which only `logon_hook::arm` makes, and
    /// only after the read-back matched — so `Armed` cannot be recorded before
    /// the entrance is on disk (U-22). It must name this journal's
    /// transaction ([`Refusal::EntranceForAnotherTransaction`]).
    Armed(bt_platform::logon_hook::Armed),
    /// The entrance could not be made durable, or its command is too long.
    EntranceFailed,
    /// The restart is put back to `Prepared`: admission refused (W3, W5), an
    /// entrance found from a dead attempt (W4), or a macOS swap not performed
    /// (M5).
    Reverted,
    /// Exclusive admission taken and no process runs from the install.
    Admitted,
    /// Every move is done and P started N with `nonce`.
    TrialBegan {
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
    Armed,
    EntranceFailed,
    Reverted,
    Admitted,
    TrialBegan,
    ReceiptAccepted,
    RollbackDeclared,
    RolledBack,
    RollbackFailed,
    Retired,
}

impl EventKind {
    pub(crate) const ALL: [EventKind; 15] = [
        EventKind::Prepared,
        EventKind::PrepareFailed,
        EventKind::LaunchedWithoutResume,
        EventKind::Discarded,
        EventKind::HandedOff,
        EventKind::Armed,
        EventKind::EntranceFailed,
        EventKind::Reverted,
        EventKind::Admitted,
        EventKind::TrialBegan,
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
            | EventKind::HandedOff => &[Actor::Old],
            EventKind::Armed
            | EventKind::EntranceFailed
            | EventKind::Admitted
            | EventKind::TrialBegan => &[Actor::Applier],
            EventKind::Reverted
            | EventKind::ReceiptAccepted
            | EventKind::RollbackDeclared
            | EventKind::RolledBack
            | EventKind::RollbackFailed
            | EventKind::Retired => &[Actor::Applier, Actor::Recovery],
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
            Event::Armed(_) => EventKind::Armed,
            Event::EntranceFailed => EventKind::EntranceFailed,
            Event::Reverted => EventKind::Reverted,
            Event::Admitted => EventKind::Admitted,
            Event::TrialBegan { .. } => EventKind::TrialBegan,
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
    (PhaseKind::Moving, EventKind::TrialBegan, PhaseKind::Trial),
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
/// A mismatched receipt in `Trial` is refused by value and is not listed.
pub(crate) const NAMED_REFUSALS: &[(PhaseKind, EventKind, Refusal)] = &[
    (
        PhaseKind::RollbackIntent,
        EventKind::ReceiptAccepted,
        Refusal::ReceiptAfterRollbackIntent,
    ),
    (
        PhaseKind::Stuck,
        EventKind::ReceiptAccepted,
        Refusal::ReceiptAfterRollbackIntent,
    ),
];

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
        (Phase::Handoff { .. }, Event::EntranceFailed) => Ok(Phase::Abandoned),
        (Phase::Handoff { .. } | Phase::Armed | Phase::Moving, Event::Reverted) => {
            Ok(Phase::Prepared {
                deferred_launches: 0,
            })
        }
        (Phase::Armed, Event::Admitted) => Ok(Phase::Moving),
        (
            Phase::Moving,
            Event::TrialBegan {
                nonce,
                process,
                began_ms,
            },
        ) => Ok(Phase::Trial {
            nonce: *nonce,
            process: *process,
            began_ms: *began_ms,
        }),
        (Phase::Trial { .. }, Event::ReceiptAccepted(receipt)) if receipt.txn != *txn => {
            Err(Refusal::ReceiptForAnotherTransaction)
        }
        (Phase::Trial { nonce, .. }, Event::ReceiptAccepted(receipt))
            if receipt.nonce != *nonce =>
        {
            Err(Refusal::ReceiptForAnotherTrial)
        }
        (Phase::Trial { .. }, Event::ReceiptAccepted(_)) => Ok(Phase::Committed),
        (Phase::RollbackIntent { .. } | Phase::Stuck { .. }, Event::ReceiptAccepted(_)) => {
            Err(Refusal::ReceiptAfterRollbackIntent)
        }
        (Phase::Moving, Event::RollbackDeclared) => Ok(Phase::RollbackIntent { trial: None }),
        (Phase::Trial { process, .. }, Event::RollbackDeclared) => Ok(Phase::RollbackIntent {
            trial: Some(*process),
        }),
        (Phase::RollbackIntent { .. } | Phase::Stuck { .. }, Event::RolledBack) => {
            Ok(Phase::RolledBack)
        }
        (
            Phase::RollbackIntent { trial } | Phase::Stuck { trial, .. },
            Event::RollbackFailed { error },
        ) => Ok(Phase::Stuck {
            trial: *trial,
            last_error: error.clone(),
        }),
        (Phase::Committed, Event::Retired) => Ok(Phase::Retired {
            outcome: Outcome::Committed,
        }),
        (Phase::RolledBack, Event::Retired) => Ok(Phase::Retired {
            outcome: Outcome::RolledBack,
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
    /// A lock holder whose tenure began in any later phase: R. It never writes
    /// `Trial`, because it never starts N.
    Recovery,
    /// N: the new build started with the trial's nonce.
    Trial,
    /// Any ordinary start, reading only the header.
    Start,
}

/// **Who may write each phase into the journal** ((b).2, "Who may write
/// what"). N writes no phase at all: its only write is its receipt.
pub(crate) const JOURNAL_WRITERS: &[(PhaseKind, &[Actor])] = &[
    (PhaseKind::Allocated, &[Actor::Old]),
    (
        PhaseKind::Prepared,
        &[Actor::Old, Actor::Applier, Actor::Recovery],
    ),
    (PhaseKind::Handoff, &[Actor::Old]),
    (PhaseKind::Armed, &[Actor::Applier]),
    (PhaseKind::Moving, &[Actor::Applier]),
    (PhaseKind::Trial, &[Actor::Applier]),
    (PhaseKind::Committed, &[Actor::Applier, Actor::Recovery]),
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

/// The rights table. Anything it does not list, nobody may do.
pub(crate) const EFFECT_RIGHTS: &[Right] = &[
    // O, as the job owner, sweeps what a dead preparation left (W1, M1).
    Right {
        actor: Actor::Old,
        effect: Effect::DetachMount,
        during: &[PhaseKind::Allocated],
    },
    Right {
        actor: Actor::Old,
        effect: Effect::DeleteTxnDir,
        during: &[PhaseKind::Allocated],
    },
    Right {
        actor: Actor::Old,
        effect: Effect::DeleteJournal,
        during: &[PhaseKind::Allocated],
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
    // N's one write.
    Right {
        actor: Actor::Trial,
        effect: Effect::WriteReceipt,
        during: &[PhaseKind::Trial],
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
        effect: Effect::DeleteTxnDir,
        during: &[
            PhaseKind::Allocated,
            PhaseKind::Prepared,
            PhaseKind::Abandoned,
            PhaseKind::Retired,
        ],
    },
    Right {
        actor: Actor::Start,
        effect: Effect::DeleteJournal,
        during: &[
            PhaseKind::Allocated,
            PhaseKind::Prepared,
            PhaseKind::Abandoned,
            PhaseKind::Retired,
        ],
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
    /// The rescue build: P, or R from the entrance or a `--then-launch`.
    LockHolder,
}

impl Asker {
    /// The actor this asker is while the journal says `phase`: a lock holder
    /// that takes the lock in `Handoff` or `Armed` becomes the applier.
    pub(crate) fn actor(self, phase: PhaseKind) -> Actor {
        match (self, phase) {
            (Asker::JobOwner, _) => Actor::Old,
            (Asker::LockHolder, PhaseKind::Handoff | PhaseKind::Armed) => Actor::Applier,
            (Asker::LockHolder, _) => Actor::Recovery,
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
    /// W6, M6, W7: record [`Event::RollbackDeclared`].
    DeclareRollback,
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
    /// W11: remove the entrance if `remove_entrance`, relaunch the installed
    /// build with `--update-failed`, then record [`Event::Retired`].
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
            | Action::AwaitReceipt { .. }
            | Action::Commit
            | Action::DeclareRolledBack
            | Action::StayStuck { .. } => Vec::new(),
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
            _ => Action::DeclareRollback,
        },
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
        Phase::RollbackIntent { trial } | Phase::Stuck { trial, .. } => match trial {
            Some(process) if disk.trial_alive => Action::StopTrial(*process),
            _ => match restore(&journal.body.layout, &disk.located) {
                Err(reason) => Action::StayStuck { reason },
                Ok(None) => Action::DeclareRolledBack,
                Ok(Some(steps)) => Action::RollBack(steps),
            },
        },
        Phase::Committed => finish_commit(disk),
        Phase::RolledBack => Action::FinishRollback {
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
        let (install, backup) = seen
            .iter()
            .find(|entry| entry.name == name)
            .map_or((None, None), |entry| (entry.install, entry.backup));
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
    /// Not decided yet (`Trial`, or a phase before it), or not readable by this
    /// build: the trial keeps its writes pending and looks again.
    Undecided,
    /// `Committed` is durable (or the transaction retired after it): the trial's
    /// writes may land.
    Committed,
    /// Decided otherwise — `RollbackIntent`, `Stuck`, `RolledBack`,
    /// `Abandoned`, retired without a commit — or gone: the journal is absent,
    /// or names another transaction. Nothing it held back will ever be written.
    Ended,
}

/// **What the journal's header says about the trial of `txn`**; `None` is no
/// journal at all.
///
/// The frozen header alone (F-8): `outcome == committed` releases the trial's
/// writes; `outcome == rolled_back` or a `terminal` class ends them; anything
/// else — or a header this build cannot read — is not decided yet. A journal
/// naming another transaction, or none, means this trial's is gone.
pub(crate) fn trial_sight(journal: Option<&[u8]>, txn: &TxnId) -> TrialSight {
    let Some(bytes) = journal else {
        return TrialSight::Ended;
    };
    let Ok(header) = Header::parse(bytes) else {
        return TrialSight::Undecided;
    };
    if header.txn != *txn {
        return TrialSight::Ended;
    }
    match (header.outcome, header.class) {
        (HeaderOutcome::Committed, _) => TrialSight::Committed,
        (HeaderOutcome::RolledBack, _) | (_, Class::Terminal) => TrialSight::Ended,
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
/// image and of the rescue build the header names, and the transaction its
/// own `--update-trial` names, if it was started as a trial.
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

impl StartAction {
    /// The effects this action performs, for [`EFFECT_RIGHTS`].
    pub(crate) fn effects(self) -> &'static [Effect] {
        match self {
            StartAction::Continue | StartAction::HandToRescue | StartAction::RunAsTrial => &[],
            StartAction::Retire => &[
                Effect::RemoveEntrance,
                Effect::DeleteTxnDir,
                Effect::DeleteJournal,
            ],
            StartAction::Discard => &[Effect::DeleteTxnDir, Effect::DeleteJournal],
        }
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
    fn armed_for(txn: TxnId) -> bt_platform::logon_hook::Armed {
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
            body: Body { phase, layout },
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
        }
    }

    fn valid_receipt() -> Receipt {
        receipt(txn(), nonce(TRIAL_NONCE))
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
            Phase::RollbackIntent { trial: Some(_) } | Phase::Stuck { trial: Some(_), .. }
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
            trial_phase(),
            Phase::Committed,
            Phase::RollbackIntent { trial: None },
            Phase::RollbackIntent { trial: Some(TRIAL) },
            Phase::Stuck {
                trial: Some(TRIAL),
                last_error: "a file is held open".to_owned(),
            },
            Phase::RolledBack,
            Phase::Abandoned,
            Phase::Retired {
                outcome: Outcome::Committed,
            },
            Phase::Retired {
                outcome: Outcome::RolledBack,
            },
        ]
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
            armed(),
            Event::EntranceFailed,
            Event::Reverted,
            Event::Admitted,
            Event::TrialBegan {
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
            Layout::Bundle { .. } => {
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
        for asker in [Asker::JobOwner, Asker::LockHolder] {
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
                last_error: String::new(),
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
            } => HeaderOutcome::Committed,
            Phase::RollbackIntent { .. }
            | Phase::Stuck { .. }
            | Phase::RolledBack
            | Phase::Retired {
                outcome: Outcome::RolledBack,
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
        let intent = journal(Phase::RollbackIntent { trial: None }, members_layout());
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
    /// lock holder the rest and the revert; N no phase at all, only its
    /// receipt; R never `Trial`.
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
            assert!(!may_record(Actor::Trial, phase), "N records {phase:?}");
            assert!(
                !may_record(Actor::Start, phase),
                "a start records {phase:?}"
            );
        }
        assert!(!may_record(Actor::Recovery, PhaseKind::Trial));
        let trial_rights: Vec<(Effect, &[PhaseKind])> = EFFECT_RIGHTS
            .iter()
            .filter(|right| right.actor == Actor::Trial)
            .map(|right| (right.effect, right.during))
            .collect();
        assert_eq!(
            trial_rights,
            vec![(Effect::WriteReceipt, &[PhaseKind::Trial][..])],
            "N's only write is its receipt, and only during its trial"
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
                                may(Actor::Start, *effect, phase),
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
    /// version, and deleted the rollback source on that. Here the only road
    /// to `Committed` is `ReceiptAccepted` in `Trial`, and `decide` answers
    /// `Commit` only where `next` would accept the receipt.
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
            vec![&(
                PhaseKind::Trial,
                EventKind::ReceiptAccepted,
                PhaseKind::Committed
            )]
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
                let entitled = matches!(journal.body.phase, Phase::Trial { .. })
                    && disk.asker == Asker::LockHolder
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
            Phase::RollbackIntent { trial: Some(TRIAL) },
            Phase::Stuck {
                trial: None,
                last_error: String::new(),
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
                    last_error: "a file is held open".to_owned(),
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
        let stuck = journal(Phase::RollbackIntent { trial: None }, members_layout())
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
                Layout::Bundle { .. } => bundle(Some(old_bundle()), Some(new_bundle())),
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
                    outcome: Outcome::RolledBack
                },
                "after {done} moves"
            );
            assert!(
                disk.is_old_install(&inventories),
                "after {done} moves: {disk:?}"
            );
        }
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
            Ok(Phase::RollbackIntent { trial: Some(TRIAL) })
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
                outcome: Outcome::Committed
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
            Phase::RollbackIntent { trial: Some(TRIAL) },
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
        let intent = journal(Phase::RollbackIntent { trial: None }, members_layout());
        let stuck = journal(
            Phase::Stuck {
                trial: None,
                last_error: "a file was held open".to_owned(),
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
                outcome: Outcome::RolledBack
            }
        );
        assert!(disk.is_old_install(&inventories));
    }

    /// RED (U-10) — **W11: `RolledBack` removes the entrance, relaunches the old
    /// build and retires; a later start then deletes what is left.**
    #[test]
    fn w11_rolled_back_removes_the_entrance_relaunches_and_retires() {
        let journal = journal(Phase::RolledBack, members_layout());
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
            at_start(&start_on(Phase::RolledBack)),
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
        let journal = journal(Phase::RollbackIntent { trial: None }, members_layout());
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
                    outcome: Outcome::RolledBack
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

    /// RED (U-10) — **M6: an exchange performed — the new identity is live — is
    /// rolled back, decided by identity and never by the phase.**
    #[test]
    fn m6_an_exchange_performed_rolls_back() {
        let journal = journal(Phase::Moving, bundle_layout());
        let located = bundle(Some(new_bundle()), Some(old_bundle()));
        assert_eq!(decide(&holder(&journal, located)), Action::DeclareRollback);
        assert_eq!(
            journal
                .advance(&Event::RollbackDeclared)
                .map(|j| j.body.phase),
            Ok(Phase::RollbackIntent { trial: None })
        );
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
            Phase::RollbackIntent { trial: Some(TRIAL) },
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
                last_error: "the exchange was refused".to_owned(),
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

    /// RED (U-10) — **M11: `RolledBack`, `Abandoned` and `Committed` with debt
    /// finish as W11–W13 do; the plist goes only after the terminal state.**
    #[test]
    fn m11_rolled_back_abandoned_and_committed_with_debt_finish_as_on_windows() {
        let located = bundle(Some(new_bundle()), None);
        let rolled_back = journal(Phase::RolledBack, bundle_layout());
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
            PathBuf::from("/Users/a/Library/Application Support/Folio"),
            PathBuf::from("/Users/b/Library/Application Support/Folio"),
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
}
