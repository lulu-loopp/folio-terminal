//! **What crosses the endpoint, and what a pane is called on the other side of it.**
//!
//! `bt_platform::attention_pipe` owns the kernel object and knows nothing about grammar; this
//! module owns the grammar and knows nothing about the kernel. The split is deliberate — the unsafe
//! boundary should not have to be reopened to change a field name.
//!
//! # A capability, not an address
//!
//! `docs/plans/attention/plan.md` §10.6's first clause is that `FOLIO_PANE` is **diagnostic** and
//! routes nothing. That is kept literally: the variable exists, it says `<window>.<tab>.<seat>`,
//! it is what a user reads in `env` to see which pane they are in — and **it is not in the
//! message**. What is in the message is [`CAPABILITY_VARIABLE`]'s value: 128 unguessable bits
//! minted when the pane was born, held on the leaf, and gone the instant the leaf is.
//!
//! The difference is the whole of red line 13. Coordinates can be *guessed*, incremented, and
//! walked: a caller holding `1.2.0` can try `1.2.1`. A capability cannot be walked, so a hook that
//! inherited one pane's environment can raise a hand for **that** pane and has no way to name
//! another — not because it is refused, but because it cannot say the words.
//!
//! # The payload never crosses
//!
//! `folio attention <family>:<event> --json <payload>` takes the hook's own payload, and the only
//! things it ever sends onward are the **two fields the mapping table declared**: the identifier a
//! row names an `IdSource` for — today no row does — and the sentence a turn ended on, where a row
//! names a [`Words`](crate::attention_map::Words) source. Both are bounded and checked; everything
//! else in a payload stays in the hook process and dies with it. That is not an optimisation: a
//! channel that carried payloads would be a channel worth attacking.
//!
//! The words are the wider of the two doors and are worth stating exactly. What crosses is at most
//! eighty-one characters of prose with no control character in it, produced by
//! [`crate::attention_words`] from a source a row declared, and the far end refuses a line whose
//! `text` is anything else. What it is *for* is the whole of §11.7's wording clause: a program that
//! wrote a sentence has said this better than anything composed from a pane name. What it is not is
//! a general text channel — a family with nothing declared cannot put a byte through it.
//!
//! # Failure is silent, and counted
//!
//! There is no reply channel — the endpoint's pipe is inbound-only — so a malformed line cannot be
//! answered. It is dropped and counted. The counters are the only evidence such a line arrived, and
//! `BT_ATTENTION_TRACE` is where a line that *was* understood leaves its mark.

use std::{
    sync::{Mutex, OnceLock, PoisonError},
    time::{Duration, Instant},
};

use bt_platform::attention_pipe::AttentionPipe;

use crate::attention::{
    ClearClass, ClearReason, ClearSelector, Event, IdSource, WaitSlot, wait_key_is_well_formed,
};
use crate::attention_map::{self, TURN_END};

/// **Diagnostic only.** `<window>.<tab>.<seat>`, so a person in a pane can see which one they are
/// in; nothing reads it back.
///
/// It is deliberately the *stale-able* one. A pane torn out to another window, moved between
/// seats or merged has a different address afterwards, and a channel that routed by this would
/// route to whatever now sits at the old coordinates. The plan says so in as many words, and the
/// way to make sure a rule like that is kept is to make the thing it forbids impossible rather than
/// discouraged — which is why the message below has no field this could go in.
pub(crate) const PANE_VARIABLE: &str = "FOLIO_PANE";

/// The endpoint's name, for a child that wants to speak to its window.
pub(crate) const ENDPOINT_VARIABLE: &str = "FOLIO_ATTENTION_PIPE";

/// **The capability.** One pane, 128 bits, minted at birth and dead with the leaf.
pub(crate) const CAPABILITY_VARIABLE: &str = "FOLIO_ATTENTION";

/// How long a strong credential may stand with nothing having ended it.
///
/// Ten minutes, which is upstream's own synchronous timeout for a permission request — the longest
/// a well-behaved producer's wait can legitimately last. This is hygiene and not correctness: the
/// ledger's watermark already guarantees that the next genuine request is seen whether or not this
/// ever fires (§11.4.3). What it prevents is a badge outliving the thing it reports, which is the
/// 2026-08-21 defect stated in general form.
pub(crate) const WAIT_TTL: Duration = Duration::from_secs(600);

/// How many lines may wait for the window thread before the oldest are dropped.
///
/// A bound rather than a growing queue: the only way to reach it is a window thread that has
/// stopped answering, and in that situation an unbounded queue turns one stuck frame into memory
/// growth. Dropping the **oldest** because the newest is the one that is actually happening.
const INBOX_BOUND: usize = 256;

/// One arrival, in the words it crosses the pipe in.
///
/// Five fields and two of them are usually absent. `family` and `event` together name a row of the
/// mapping table; `capability` names the pane; `id` is present only when that row declared a field
/// path, which today no row does; `text` is present only when a turn-end row declared where its
/// words live **and** they were there to be found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Message {
    pub capability: String,
    pub family: String,
    pub event: String,
    pub id: Option<String>,
    /// **The sentence the turn ended on**, already cleaned and bounded by
    /// [`crate::attention_words`]. `None` is the ordinary answer and costs the reader only the
    /// wording the toast had before this field existed.
    pub text: Option<String>,
}

/// The wire's version, and the reason for it: a `folio.exe` on `PATH` may be a different build from
/// the one that spawned the shell — a user upgrading Folio while a long-lived agent is running is
/// the ordinary way that happens. A line from a version this build does not know is dropped rather
/// than half-understood.
const WIRE_VERSION: u64 = 1;

impl Message {
    /// The line this message crosses as.
    #[must_use]
    pub(crate) fn encode(&self) -> String {
        let mut value = serde_json::Map::new();
        value.insert("v".to_owned(), WIRE_VERSION.into());
        value.insert("cap".to_owned(), self.capability.clone().into());
        value.insert("family".to_owned(), self.family.clone().into());
        value.insert("event".to_owned(), self.event.clone().into());
        if let Some(id) = &self.id {
            value.insert("id".to_owned(), id.clone().into());
        }
        if let Some(text) = &self.text {
            value.insert("text".to_owned(), text.clone().into());
        }
        serde_json::Value::Object(value).to_string()
    }

    /// One line, read back — or `None`, which is the answer to every kind of nonsense.
    ///
    /// Deliberately total and deliberately unforgiving. There is no partial success here: a line
    /// that is not exactly one JSON object with a version this build knows and three non-empty
    /// strings is not a request that arrived slightly wrong, it is a line from something that is
    /// not `folio attention`.
    #[must_use]
    pub(crate) fn decode(line: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
        let object = value.as_object()?;
        if object.get("v")?.as_u64()? != WIRE_VERSION {
            return None;
        }
        let text = |key: &str| -> Option<String> {
            let text = object.get(key)?.as_str()?;
            (!text.is_empty() && text.len() <= 128).then(|| text.to_owned())
        };
        let id = match object.get("id") {
            None => None,
            Some(value) => {
                let id = value.as_str()?;
                // The same bound the association key has, checked on the way in as well as on the
                // way out: this end has no reason to trust that the far end applied it.
                if !wait_key_is_well_formed(id) {
                    return None;
                }
                Some(id.to_owned())
            }
        };
        let words = match object.get("text") {
            None => None,
            Some(value) => {
                let words = value.as_str()?;
                // The same bound and the same shape the verb minted it to, checked again here for
                // `id`'s reason: this end has no cause to trust that the far end applied either.
                if !words_are_quotable(words) {
                    return None;
                }
                Some(words.to_owned())
            }
        };
        Some(Self {
            capability: text("cap")?,
            family: text("family")?,
            event: text("event")?,
            id,
            text: words,
        })
    }
}

/// The most characters a quoted sentence may cross as: [`crate::attention_words::LIMIT`] of
/// somebody's own words plus the one mark this build adds when it shortens them.
const MAX_QUOTED_CHARACTERS: usize = crate::attention_words::LIMIT + 1;

/// **Whether a `text` field is a sentence this build could have written.**
///
/// Three clauses and each is a different attack surface. Non-empty, because an empty quotation
/// would raise a toast with a blank body — worse than the wording it replaced. Bounded in
/// *characters*, matching the bound the verb cut it to, so a sentence in Chinese is allowed the
/// same eighty words as one in English rather than a quarter of them. And no control characters at
/// all: the body of this string is written onto a desktop notification, and a newline, a `\r` or an
/// escape byte in one is a line of somebody else's making in a window Folio put up.
#[must_use]
pub(crate) fn words_are_quotable(words: &str) -> bool {
    !words.is_empty()
        && words.chars().count() <= MAX_QUOTED_CHARACTERS
        && !words.chars().any(char::is_control)
}

/// **The one place a `<family>:<event>` is split**, so the two ends cannot come to disagree.
///
/// The qualified spelling is what makes a family cost data alone: the hook's command line carries
/// its own family name, so this build never has to work out which upstream a bare `Stop` came from
/// — a question that has no answer, since two families spell two different events the same way.
#[must_use]
pub(crate) fn split_event(qualified: &str) -> Option<(&str, &str)> {
    let (family, event) = qualified.split_once(':')?;
    (!family.is_empty() && !event.is_empty()).then_some((family, event))
}

/// What one understood message asks of a pane — **on each of two lanes, independently.**
///
/// Not one answer or the other, and that is the correction the shipped tables force: `Stop` is a
/// boundary that ends every wait this pane was holding **and** the announcement that a turn is
/// over. Those are two different sentences about the same instant, they go to two different places,
/// and a lookup that returned the first and stopped would silently drop the second on the one
/// family the block was written for.
///
/// Both being `None` is not an error. It is a hook this build has no mapping for — which is what an
/// unrecognised upstream event ought to be, and the reason the endpoint answers nobody: there is
/// nothing to say back that would help.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Asks {
    /// The queue lane: an arrival for the ledger.
    pub ledger: Option<Event>,
    /// The event lane: a turn ended, and this is who said so.
    pub turn_end: Option<crate::attention::Via>,
}

impl Asks {
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.ledger.is_none() && self.turn_end.is_none()
    }
}

/// **Whether one arrived capability names the pane holding `held`.**
///
/// One comparison, written out because of what the two guards are worth. An empty `held` is a leaf
/// that was never given a capability — a shell-less fixture, or a pane spawned on a machine whose
/// endpoint would not open — and it must never match anything, or every such pane in the window
/// would answer to the same nothing. An empty `capability` cannot get this far ([`Message::decode`]
/// refuses it), and is refused again here: this is the one place a pane is chosen, and a door that
/// is checked in two places is a door that stays shut when one of them is edited.
#[must_use]
pub(crate) fn names_this_pane(capability: &str, held: &str) -> bool {
    !capability.is_empty() && capability == held
}

/// Read a decoded message against the rows this machine has installed.
#[must_use]
pub(crate) fn asks_of(installed: &[crate::attention::MappingRow], message: &Message) -> Asks {
    Asks {
        ledger: attention_map::row(attention_map::ROWS, &message.family, &message.event)
            .map(|row| attention_map::event_for(installed, row, message.id.as_deref())),
        turn_end: attention_map::turn_end(&message.family, &message.event),
    }
}

/// The identifier one row wants out of a hook payload, if it wants one.
///
/// Runs in the **verb**, in the hook's own process, which is what keeps the payload off the wire.
/// A path is a top-level key: nothing upstream has been quoted as putting its identifier deeper,
/// and a dotted path language invented before anything needs it is a language nobody has read.
#[must_use]
pub(crate) fn identifier(id: IdSource, payload: Option<&serde_json::Value>) -> Option<String> {
    let IdSource::Path(path) = id else {
        return None;
    };
    let found = payload?.get(path)?.as_str()?;
    wait_key_is_well_formed(found).then(|| found.to_owned())
}

// ---------------------------------------------------------------------------
// The clock beside the ledger
// ---------------------------------------------------------------------------

/// **When each standing credential runs out**, and nothing else.
///
/// Beside the ledger rather than inside it, for the ledger's own stated reason: it is a pure
/// function of arrivals, the frame's facts are handed in, and a clock is one of those facts. This
/// holds no credential — only a deadline per slot — so the two cannot disagree about *whether* a
/// pane is asking; the worst a drifted entry can do is produce a clear for something that has
/// already gone, which the ledger answers with no change and no line.
#[derive(Clone, Debug, Default)]
pub(crate) struct WaitClock {
    entries: Vec<(WaitSlot, Instant)>,
}

impl WaitClock {
    /// Start (or restart) one slot's ten minutes.
    pub(crate) fn arm(&mut self, slot: &WaitSlot, now: Instant) {
        let deadline = now + WAIT_TTL;
        match self.entries.iter_mut().find(|(held, _)| held == slot) {
            Some(entry) => entry.1 = deadline,
            None => self.entries.push((slot.clone(), deadline)),
        }
    }

    /// Forget whatever a clear has just retired.
    pub(crate) fn forget(&mut self, selector: &ClearSelector) {
        self.entries.retain(|(slot, _)| match selector {
            ClearSelector::All => false,
            ClearSelector::Kind(kind) => slot.kind() != *kind,
            ClearSelector::Key { kind, key } => {
                slot != &WaitSlot::Keyed {
                    kind: *kind,
                    key: key.clone(),
                }
            }
        });
    }

    /// The next instant this pane owes the loop a wake-up, or `None`.
    ///
    /// `None` for a pane with nothing standing, which is every pane almost always — so an idle
    /// window asks for no wake-ups at all on this account.
    #[must_use]
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.entries.iter().map(|(_, at)| *at).min()
    }

    /// The clears that have come due, removing them as it goes.
    ///
    /// `Boundary`, because a timer is not a receipt: it is not evidence that anything ended, it is
    /// this build giving up on being told. Giving up has to be unconditional or it would leave
    /// exactly the entries it exists to sweep.
    pub(crate) fn due(&mut self, now: Instant) -> Vec<Event> {
        let mut expired = Vec::new();
        self.entries.retain(|(slot, at)| {
            if *at > now {
                return true;
            }
            expired.push(Event::StrongClear {
                selector: match slot {
                    WaitSlot::Keyed { kind, key } => ClearSelector::Key {
                        kind: *kind,
                        key: key.clone(),
                    },
                    WaitSlot::Level(kind) => ClearSelector::Kind(*kind),
                },
                class: ClearClass::Boundary,
                reason: ClearReason::Ttl,
                begins_turn: false,
            });
            false
        });
        expired
    }
}

// ---------------------------------------------------------------------------
// The process's endpoint
// ---------------------------------------------------------------------------

static ENDPOINT: OnceLock<Option<AttentionPipe>> = OnceLock::new();
static INBOX: Mutex<Vec<String>> = Mutex::new(Vec::new());
static OVERFLOWED: Mutex<u64> = Mutex::new(0);

/// Open this process's endpoint, once, and start delivering into the inbox.
///
/// `wake` is called on the listener thread and must do nothing but nudge the loop.
///
/// A failure here is not fatal and is not reported to the user: an endpoint that would not open
/// means hooks cannot reach this window, which is the same situation as a machine where nobody has
/// installed any — the terminal works, and the attention queue stays as empty as it was before this
/// slice existed. The one thing it must never do is fall back to a weaker endpoint.
pub(crate) fn open(wake: impl Fn() + Send + Sync + 'static) -> Option<&'static AttentionPipe> {
    ENDPOINT
        .get_or_init(|| {
            AttentionPipe::start(move |line| {
                park(line);
                wake();
            })
            .ok()
        })
        .as_ref()
}

/// The endpoint's name, for the environment a shell is spawned with.
#[must_use]
pub(crate) fn endpoint_name() -> Option<&'static str> {
    ENDPOINT.get()?.as_ref().map(AttentionPipe::name)
}

fn park(line: String) {
    let mut inbox = INBOX.lock().unwrap_or_else(PoisonError::into_inner);
    if inbox.len() >= INBOX_BOUND {
        inbox.remove(0);
        *OVERFLOWED.lock().unwrap_or_else(PoisonError::into_inner) += 1;
    }
    inbox.push(line);
}

/// Everything that has arrived since the last time the window thread looked.
#[must_use]
pub(crate) fn take() -> Vec<String> {
    std::mem::take(&mut *INBOX.lock().unwrap_or_else(PoisonError::into_inner))
}

/// A fresh capability for one pane.
#[must_use]
pub(crate) fn mint_capability() -> String {
    format!("{:032x}", bt_platform::attention_pipe::unguessable_bits())
}

/// The names a `<family>:<event>` may be, for the verb's own refusal message.
///
/// Assembled from the two tables rather than written out, so a family added as data is a family the
/// help text already knows about.
#[must_use]
pub(crate) fn known_events() -> Vec<String> {
    let rows = attention_map::ROWS
        .iter()
        .map(|row| format!("{}:{}", row.family, row.event));
    let ends = TURN_END
        .iter()
        .map(|row| format!("{}:{}", row.family, row.event));
    let mut all = rows.chain(ends).collect::<Vec<_>>();
    all.sort_unstable();
    all.dedup();
    all
}

// ---------------------------------------------------------------------------
// The verb
// ---------------------------------------------------------------------------

/// **What `folio attention` does, from a process with no window and no console.**
///
/// The whole of it is: read two variables, look one row up, write one line, exit. There is no
/// retry, no queue, no file and no wait for an answer — the endpoint's pipe is inbound-only and
/// there is nothing to wait for. A hook that took a second to run would be a hook the user feels,
/// because the one that matters most fires while Claude Code is holding an approval open.
///
/// **Exit codes**, and they are for a person reading a hook log rather than for a program:
/// `0` said; `1` there was nowhere to say it (not in a Folio pane, or the window has gone);
/// `2` the call itself was wrong. None of them is ever a reason to block.
pub(crate) fn run_verb(call: Result<crate::cli::AttentionCall, crate::cli::AttentionFault>) -> i32 {
    let call = match call {
        Ok(call) => call,
        Err(fault) => {
            report(&refusal_text(&fault));
            return match fault {
                crate::cli::AttentionFault::NothingAsked => 0,
                _ => 2,
            };
        }
    };
    let Some((family, event)) = split_event(&call.event) else {
        report(&refusal_text(&crate::cli::AttentionFault::ExtraEvent(
            call.event.clone(),
        )));
        return 2;
    };
    let known = attention_map::row(attention_map::ROWS, family, event).is_some()
        || attention_map::turn_end(family, event).is_some();
    if !known {
        report(&format!(
            "folio attention: this build has no mapping for {}\n\n{}",
            call.event,
            usage_text()
        ));
        return 2;
    }
    let (Some(endpoint), Some(capability)) =
        (variable(ENDPOINT_VARIABLE), variable(CAPABILITY_VARIABLE))
    else {
        // Not a failure worth a sentence on a console nobody is looking at: it means the caller is
        // not running inside a Folio pane, which is the ordinary state of every other terminal on
        // the machine. The code says so and nothing is printed.
        return 1;
    };
    let payload = payload_of(call.payload.as_ref())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    let message = message_for(capability, family, event, payload.as_ref());
    match bt_platform::attention_pipe::send_line(&endpoint, &message.encode()) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

/// **The line one call sends**, from the pane it names and the payload it was handed.
///
/// Pure, and that is the point of its existing at all: every way of coming away with no words —
/// no payload, a row that declares no source, a transcript that has been deleted, a message that
/// ended on a tool call — has to leave a message that is still sent. A test can walk every one of
/// those, and none of them can be walked through [`run_verb`], which reads this process's own
/// environment.
#[must_use]
pub(crate) fn message_for(
    capability: String,
    family: &str,
    event: &str,
    payload: Option<&serde_json::Value>,
) -> Message {
    // The identifier, if — and only if — the row declared where one lives.
    let id = attention_map::row(attention_map::ROWS, family, event)
        .and_then(|row| identifier(row.id, payload));
    // And the sentence, if — and only if — a turn-end row declared where one lives, and it was
    // there. The payload itself stops here: this process exits in a moment and takes it with it.
    let text = attention_map::turn_end_row(family, event)
        .and_then(|row| quoted(row.words, payload))
        .filter(|words| words_are_quotable(words));
    Message {
        capability,
        family: family.to_owned(),
        event: event.to_owned(),
        id,
        text,
    }
}

/// The sentence a row's declared source holds, cleaned to what a toast may carry.
///
/// `None` at every step, and each `None` is a real state rather than a defensive one: a caller that
/// passed no payload, a field that is not there, a path that names nothing.
#[must_use]
fn quoted(words: attention_map::Words, payload: Option<&serde_json::Value>) -> Option<String> {
    let payload = payload?;
    let limit = crate::attention_words::LIMIT;
    match words {
        attention_map::Words::None => None,
        attention_map::Words::Field(key) => {
            crate::attention_words::lede(payload.get(key)?.as_str()?, limit)
        }
        attention_map::Words::Transcript(key) => crate::attention_words::transcript_lede(
            std::path::Path::new(payload.get(key)?.as_str()?),
            limit,
        ),
    }
}

/// The payload's text, from wherever the caller said it would be.
///
/// The two spellings are two families' doing rather than a choice: codex **appends** its payload to
/// the `notify` array, so it arrives as the value of `--json`; Claude Code writes its own onto the
/// hook's standard input, so its command line says `--json -` and this reads the handle.
#[must_use]
fn payload_of(source: Option<&crate::cli::AttentionPayload>) -> Option<String> {
    match source? {
        crate::cli::AttentionPayload::Inline(text) => Some(text.clone()),
        crate::cli::AttentionPayload::Stdin => payload_on_stdin(),
    }
}

/// The most of a hook payload this will read. Claude Code's `Stop` payload is a few hundred bytes;
/// this is two orders of magnitude of headroom and still a bound, so a handle that turned out to
/// be a firehose cannot hold the hook open forever.
const MAX_PAYLOAD_BYTES: u64 = 64 * 1024;

/// **How long this waits for a payload before going without one.**
///
/// A bound in time as well as in bytes, and it is not defensive: whether the write end of this
/// handle is ever closed is somebody else's decision. A runner that writes its object and holds the
/// pipe open for the hook's lifetime — an ordinary way to get this wrong, and one this build cannot
/// fix from here — would leave a `read_to_end` blocked forever, and the notification that this
/// whole verb exists to send would never be sent at all. Two hundred milliseconds is far longer
/// than a parent that has already written its object needs to close it, and short enough that the
/// hook is over before anybody notices it ran.
///
/// Running out is not a failure. It is one more way of arriving with no sentence, and it takes the
/// same road every other one takes: the line goes out saying a turn ended, and the toast says so in
/// its own voice.
const STDIN_BUDGET: Duration = Duration::from_millis(200);

/// The payload on this process's standard input, when there is one there to read.
///
/// **A console is not a payload**, and the check is what keeps that true: a person who types the
/// verb at a prompt has redirected nothing, and a read of their console handle would sit there
/// waiting for them to type a JSON object at a program that is supposed to have exited already.
///
/// The read itself happens on a thread of its own so that [`STDIN_BUDGET`] can be enforced over it.
/// Nothing is joined and nothing needs to be: this process exits within a moment of the send below,
/// and a thread still parked on a handle nobody wrote to goes with it.
#[must_use]
fn payload_on_stdin() -> Option<String> {
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        return None;
    }
    let (sender, payload) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut bytes = Vec::new();
        let read = std::io::stdin()
            .lock()
            .take(MAX_PAYLOAD_BYTES)
            .read_to_end(&mut bytes);
        let _ = sender.send(read.ok().map(|_| bytes));
    });
    let bytes = payload.recv_timeout(STDIN_BUDGET).ok()??;
    String::from_utf8(bytes).ok()
}

fn variable(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// The verb's own usage block, with the names the tables know.
#[must_use]
pub(crate) fn usage_text() -> String {
    let names = known_events().join("\n  ");
    format!(
        "usage: folio attention <family>:<event> [--json <payload>|-]\n\n\
         Ring this pane's doorbell. Meant to be called from an agent CLI's hook; it says one \
         thing and exits.\n\n\
         --json -  reads the payload from standard input, which is where most hook runners \
         write it.\n\n\
         events this build knows:\n  {names}\n"
    )
}

fn refusal_text(fault: &crate::cli::AttentionFault) -> String {
    use crate::cli::AttentionFault;
    let notice = match fault {
        AttentionFault::NothingAsked => None,
        AttentionFault::MissingValue(flag) => {
            Some(format!("folio attention: {flag} needs a value"))
        }
        AttentionFault::UnknownFlag(flag) => {
            Some(format!("folio attention: unknown option {flag}"))
        }
        AttentionFault::ExtraEvent(event) => Some(format!(
            "folio attention: one call says one thing, and {event} is a second"
        )),
    };
    match notice {
        Some(notice) => format!("{notice}\n\n{}", usage_text()),
        None => usage_text(),
    }
}

/// Onto whatever console started this process, and nowhere else if there is none.
fn report(text: &str) {
    bt_platform::write_to_console(text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::{MappingRow, Via, WaitKind};
    use crate::attention_map::{CLAUDE_CODE, CODEX, PI, installed_rows};

    fn installed(family: &str) -> Vec<MappingRow> {
        installed_rows(attention_map::ROWS, family, |_| true)
    }

    /// A line goes out and comes back the same.
    #[test]
    fn a_message_survives_the_wire() {
        let message = Message {
            capability: "0123456789abcdef0123456789abcdef".to_owned(),
            family: CLAUDE_CODE.to_owned(),
            event: "PermissionRequest".to_owned(),
            id: None,
            text: None,
        };
        assert_eq!(Message::decode(&message.encode()), Some(message));
    }

    /// **Nothing in the message names a pane by where it is.**
    ///
    /// The red form of red line 13: a wire that carried a tab or a seat would let a caller holding
    /// one pane's capability walk to the next, and no amount of checking at the far end could take
    /// that back. So the assertion is on the bytes.
    #[test]
    fn the_wire_carries_no_coordinates_to_walk() {
        let line = Message {
            capability: "cap".to_owned(),
            family: CLAUDE_CODE.to_owned(),
            event: "Stop".to_owned(),
            id: None,
            text: None,
        }
        .encode();
        for coordinate in ["tab", "seat", "window", "pane", "leaf", "index"] {
            assert!(
                !line.contains(coordinate),
                "the wire has a `{coordinate}` field, which is an address: {line}"
            );
        }
    }

    /// Nonsense is dropped whole, never half-read.
    #[test]
    fn a_line_that_is_not_this_grammar_is_refused_entirely() {
        for rubbish in [
            "",
            "   ",
            "not json",
            "[1,2,3]",
            "{}",
            r#"{"v":2,"cap":"c","family":"f","event":"e"}"#,
            r#"{"v":1,"cap":"","family":"f","event":"e"}"#,
            r#"{"v":1,"cap":"c","family":"f"}"#,
            r#"{"v":1,"cap":"c","family":"f","event":"e","id":"has a space"}"#,
            r#"{"v":1,"cap":"c","family":"f","event":"e","id":12}"#,
        ] {
            assert_eq!(
                Message::decode(rubbish),
                None,
                "this was accepted and should not have been: {rubbish}"
            );
        }
        // A capability longer than any this build mints is refused before it can be compared
        // against anything, which keeps the comparison itself bounded.
        let long = "a".repeat(200);
        assert_eq!(
            Message::decode(&format!(
                r#"{{"v":1,"cap":"{long}","family":"f","event":"e"}}"#
            )),
            None
        );
    }

    /// **The sentence crosses, and it crosses whole.**
    #[test]
    fn the_words_a_turn_ended_on_survive_the_wire() {
        let message = Message {
            capability: "0123456789abcdef0123456789abcdef".to_owned(),
            family: CLAUDE_CODE.to_owned(),
            event: "Stop".to_owned(),
            id: None,
            text: Some("收官了。".to_owned()),
        };
        assert_eq!(Message::decode(&message.encode()), Some(message));
    }

    /// **A `text` this build could not have written is not read at all.**
    ///
    /// Refusing the whole line rather than the field, which is this grammar's rule everywhere else
    /// and is the right one here too: the verb never mints any of these, so a line carrying one did
    /// not come from `folio attention`, and half-reading it would be admitting a message from
    /// something that is pretending to be one. The three clauses are three different problems — a
    /// blank body, an unbounded body, and a body carrying line breaks or escapes into a desktop
    /// notification.
    #[test]
    fn a_line_whose_words_are_not_ones_this_build_wrote_is_refused() {
        let line = |text: &str| {
            format!(
                r#"{{"v":1,"cap":"c","family":"f","event":"e","text":{}}}"#,
                serde_json::Value::from(text)
            )
        };
        for bad in [
            String::new(),
            "远".repeat(MAX_QUOTED_CHARACTERS + 1),
            "two\nlines".to_owned(),
            "a\rb".to_owned(),
            "\u{1b}]9;spoof\u{7}".to_owned(),
        ] {
            assert!(
                !words_are_quotable(&bad),
                "this would be quoted and should not be: {bad:?}"
            );
            assert_eq!(
                Message::decode(&line(&bad)),
                None,
                "the line carrying it was accepted: {bad:?}"
            );
        }
        // A `text` that is not a string at all is not a sentence either.
        assert_eq!(
            Message::decode(r#"{"v":1,"cap":"c","family":"f","event":"e","text":12}"#),
            None
        );
        // And the longest sentence the verb can mint — eighty of theirs and one of ours — crosses.
        let longest = format!("{}…", "远".repeat(crate::attention_words::LIMIT));
        assert!(words_are_quotable(&longest));
        assert!(Message::decode(&line(&longest)).is_some());
    }

    /// **Where each family's words come from, and that they arrive cleaned.**
    #[test]
    fn a_row_that_declares_a_source_is_the_only_row_that_quotes() {
        // codex hands the sentence over in the payload it already sends.
        let codex = serde_json::json!({
            "type": "agent-turn-complete",
            "last-assistant-message": "**Done.** The rest is detail.",
        });
        assert_eq!(
            message_for("c".to_owned(), CODEX, "agent-turn-complete", Some(&codex)).text,
            Some("Done.".to_owned()),
            "the marks around the words are not the words"
        );
        // A family with no declared source quotes nothing, whatever is in its payload — the pin on
        // "declared, never inferred". `stop` is codex's other turn-end row and it is one of these.
        assert_eq!(
            message_for("c".to_owned(), CODEX, "stop", Some(&codex)).text,
            None
        );
        assert_eq!(
            message_for(
                "c".to_owned(),
                PI,
                "agent_settled",
                Some(&serde_json::json!({ "message": "hello" }))
            )
            .text,
            None
        );
        // And an event that is not a turn's end never carries words at all, whatever it is handed.
        assert_eq!(
            message_for(
                "c".to_owned(),
                CLAUDE_CODE,
                "PermissionRequest",
                Some(&serde_json::json!({ "last-assistant-message": "hello", "prompt": "secret" }))
            )
            .text,
            None
        );
    }

    /// **Nothing to quote never costs the notification.**
    ///
    /// The contract the whole of this slice hangs on, said as a test on the line that goes out: a
    /// payload that is missing, empty, pointing at a file that is not there, or pointing at one
    /// that holds nothing quotable still produces the same `Stop` message it produced before any of
    /// this existed. A regression here is not a missing sentence — it is a missing notification.
    #[test]
    fn a_sentence_that_could_not_be_found_still_sends_the_line() {
        let bare = Message {
            capability: "c".to_owned(),
            family: CLAUDE_CODE.to_owned(),
            event: "Stop".to_owned(),
            id: None,
            text: None,
        };
        let stop = |payload: Option<&serde_json::Value>| {
            message_for("c".to_owned(), CLAUDE_CODE, "Stop", payload)
        };
        // No payload at all — the hook runner wrote nothing, or the handle was a console.
        assert_eq!(stop(None), bare);
        // A payload without the field the row names.
        assert_eq!(
            stop(Some(&serde_json::json!({ "session_id": "abc" }))),
            bare
        );
        // The field, naming a file that is not there.
        let missing = std::env::temp_dir().join("folio-no-such-transcript-4c71.jsonl");
        assert_eq!(
            stop(Some(&serde_json::json!({
                "transcript_path": missing.to_string_lossy(),
            }))),
            bare
        );
        // The field, naming something that is not a file.
        assert_eq!(
            stop(Some(&serde_json::json!({
                "transcript_path": std::env::temp_dir().to_string_lossy(),
            }))),
            bare
        );
        // The field, holding something that is not a path.
        assert_eq!(
            stop(Some(&serde_json::json!({ "transcript_path": 7 }))),
            bare
        );
        // A payload that is not an object.
        assert_eq!(stop(Some(&serde_json::json!([1, 2, 3]))), bare);
        // And in every one of them the line is still a line this build will read back.
        assert_eq!(Message::decode(&bare.encode()), Some(bare));
    }

    /// A qualified name splits once, and an unqualified one is not a name.
    #[test]
    fn an_event_carries_the_family_that_spells_it() {
        assert_eq!(
            split_event("claude-code:PermissionRequest"),
            Some(("claude-code", "PermissionRequest"))
        );
        for bad in ["Stop", ":Stop", "claude-code:", ""] {
            assert_eq!(split_event(bad), None, "{bad}");
        }
    }

    /// The two lanes stay apart: a wait becomes a ledger event, a turn end becomes an announcement.
    #[test]
    fn each_name_reaches_exactly_one_lane() {
        let claude = installed(CLAUDE_CODE);
        let message = |event: &str| Message {
            capability: "c".to_owned(),
            family: CLAUDE_CODE.to_owned(),
            event: event.to_owned(),
            id: None,
            text: None,
        };
        assert_eq!(
            asks_of(&claude, &message("PermissionRequest")),
            Asks {
                ledger: Some(Event::StrongWait(WaitSlot::Level(WaitKind::Permission))),
                turn_end: None,
            }
        );
        // **`Stop` reaches both lanes, and this is the assertion that keeps it doing so.** A lookup
        // that answered once would drop whichever of the two it did not return, and the one it
        // would drop is the one nothing else produces.
        let stop = asks_of(&claude, &message("Stop"));
        assert!(matches!(stop.ledger, Some(Event::StrongClear { .. })));
        assert_eq!(stop.turn_end, Some(Via::Stop));
        // pi is announcement-only, so its one event reaches only the other lane.
        let settled = Message {
            capability: "c".to_owned(),
            family: PI.to_owned(),
            event: "agent_settled".to_owned(),
            id: None,
            text: None,
        };
        assert_eq!(
            asks_of(&installed(PI), &settled),
            Asks {
                ledger: None,
                turn_end: Some(Via::AgentSettled),
            }
        );
        // And a name from nowhere asks nothing at all.
        assert!(
            asks_of(
                &claude,
                &Message {
                    capability: "c".to_owned(),
                    family: "nobody".to_owned(),
                    event: "Whatever".to_owned(),
                    id: None,
                    text: None,
                }
            )
            .is_empty()
        );
    }

    /// The identifier is taken only where a row declared one — today, nowhere.
    #[test]
    fn no_payload_field_is_read_that_a_row_did_not_declare() {
        let payload = serde_json::json!({ "tool_use_id": "toolu_01", "prompt": "a secret" });
        assert_eq!(identifier(IdSource::None, Some(&payload)), None);
        assert_eq!(
            identifier(IdSource::Path("tool_use_id"), Some(&payload)),
            Some("toolu_01".to_owned())
        );
        // A declared path whose value is not a well-formed key is not a key.
        let messy = serde_json::json!({ "tool_use_id": "has a space" });
        assert_eq!(
            identifier(IdSource::Path("tool_use_id"), Some(&messy)),
            None
        );
        // Every shipped row declares nothing, so nothing is ever extracted today.
        for row in attention_map::ROWS {
            assert_eq!(
                identifier(row.id, Some(&payload)),
                None,
                "{}:{} would put a payload field on the wire",
                row.family,
                row.event
            );
        }
    }

    /// **The clock is hygiene and it expires as a boundary.**
    #[test]
    fn a_standing_credential_runs_out_after_ten_minutes_and_not_before() {
        let start = Instant::now();
        let mut clock = WaitClock::default();
        let slot = WaitSlot::Level(WaitKind::Permission);
        clock.arm(&slot, start);
        assert_eq!(clock.deadline(), Some(start + WAIT_TTL));
        assert!(
            clock
                .due(start + WAIT_TTL - Duration::from_secs(1))
                .is_empty()
        );
        assert_eq!(
            clock.due(start + WAIT_TTL),
            [Event::StrongClear {
                selector: ClearSelector::Kind(WaitKind::Permission),
                class: ClearClass::Boundary,
                reason: ClearReason::Ttl,
                begins_turn: false,
            }],
            "a timer is not a receipt: it is this build giving up on being told, and giving up \
             conditionally would leave behind exactly the entry it exists to sweep"
        );
        assert_eq!(
            clock.deadline(),
            None,
            "a fired entry is gone, not repeated"
        );
    }

    /// Re-asserting a credential restarts its clock rather than adding a second one.
    #[test]
    fn a_restated_credential_keeps_one_deadline() {
        let start = Instant::now();
        let mut clock = WaitClock::default();
        let slot = WaitSlot::Level(WaitKind::Permission);
        clock.arm(&slot, start);
        clock.arm(&slot, start + Duration::from_secs(60));
        assert_eq!(
            clock.deadline(),
            Some(start + Duration::from_secs(60) + WAIT_TTL)
        );
        assert!(clock.due(start + WAIT_TTL).is_empty());
        clock.forget(&ClearSelector::Kind(WaitKind::Permission));
        assert_eq!(clock.deadline(), None);
    }

    /// An idle pane owes the loop nothing.
    #[test]
    fn a_pane_with_nothing_standing_asks_for_no_wake_ups() {
        assert_eq!(WaitClock::default().deadline(), None);
    }

    /// **A capability names one pane, and a pane that was never given one names nothing.**
    #[test]
    fn a_pane_with_no_capability_answers_to_nothing() {
        let one = mint_capability();
        let two = mint_capability();
        assert_ne!(one, two, "two panes must not be the same pane");
        assert_eq!(one.len(), 32, "128 bits, spelled: {one}");
        assert!(names_this_pane(&one, &one));
        assert!(!names_this_pane(&one, &two));
        // The fixture case, and the machine-without-an-endpoint case: a leaf that holds nothing.
        assert!(
            !names_this_pane(&one, ""),
            "a pane that was never told a capability must not answer to one"
        );
        assert!(
            !names_this_pane("", ""),
            "and two panes holding nothing are not the same pane"
        );
    }

    /// **The whole chain, over a real kernel object: bytes in, ledger lines out.**
    ///
    /// Every link except the process spawn, which only a real machine can exercise: the endpoint's
    /// pipe, the wire grammar, the family lookup, the installed rows, the mode, the slot, the
    /// ledger and the trace's own words. It exists because each of those has its own tests and
    /// **none of them can fail in the way an integration fails** — a table that spells an event one
    /// way and an installer that spells it another are both correct on their own.
    ///
    /// The sequence is the one §11.4.3 walks: a permission request arrives, a place is taken, the
    /// user answers, the turn ends, and the next request is a **new** episode rather than a
    /// restatement of the answered one.
    #[test]
    fn a_permission_request_crosses_the_endpoint_and_mints_an_episode() {
        use crate::attention::{AnswerKind, AttentionLedger, Reach, Site, State};
        use bt_layout::SeatId;
        use bt_platform::attention_pipe::{AttentionPipe, send_line};
        use std::sync::mpsc;

        let (sender, lines) = mpsc::channel();
        let endpoint = AttentionPipe::start(move |line| {
            let _ = sender.send(line);
        })
        .expect("open an endpoint");
        let capability = mint_capability();
        let installed = installed(CLAUDE_CODE);
        let at = Site {
            tab: 0,
            seat: SeatId(0),
        };
        let mut ledger = AttentionLedger::default();
        let mut place = 0;
        let mut trace = Vec::new();
        // The ledger reads no clock of its own; every arrival below lands at one instant, which is
        // the truth for a sequence a test walks in a few microseconds.
        let now = Instant::now();

        // What a hook would run: `folio attention claude-code:<event>`, whose whole effect is one
        // line on this pipe.
        let say = |event: &str| {
            let line = Message {
                capability: capability.clone(),
                family: CLAUDE_CODE.to_owned(),
                event: event.to_owned(),
                id: None,
                text: None,
            }
            .encode();
            send_line(endpoint.name(), &line).expect("the endpoint took the line");
            let arrived = lines
                .recv_timeout(Duration::from_secs(5))
                .expect("the endpoint delivered nothing");
            let message = Message::decode(&arrived).expect("a line this build wrote, read back");
            assert!(
                names_this_pane(&message.capability, &capability),
                "the line names the pane that sent it"
            );
            asks_of(&installed, &message)
        };

        // ① The prompt appears.
        let asks = say("PermissionRequest");
        trace.extend(
            ledger
                .apply(
                    at,
                    Reach::Flash,
                    asks.ledger.expect("a wait"),
                    &mut place,
                    now,
                )
                .lines,
        );
        assert_eq!(ledger.state(), State::Requested(1));
        // ② The pass sees a pane nobody is looking at, and it takes a place.
        trace.extend(
            ledger
                .apply(
                    at,
                    Reach::Flash,
                    Event::Settle {
                        active: false,
                        focused: false,
                    },
                    &mut place,
                    now,
                )
                .lines,
        );
        assert_eq!(
            ledger.state(),
            State::Queued {
                episode: 1,
                ticket: 0
            }
        );
        // ③ The user types `2`.
        trace.extend(
            ledger
                .apply(
                    at,
                    Reach::Flash,
                    Event::Answer(AnswerKind::Keyboard),
                    &mut place,
                    now,
                )
                .lines,
        );
        assert_eq!(ledger.state(), State::Acknowledged(1));
        // ④ The turn ends. Both lanes, from one arrival — the clear that empties the ledger and the
        // announcement that says a turn is over.
        let stop = say("Stop");
        trace.extend(
            ledger
                .apply(
                    at,
                    Reach::Flash,
                    stop.ledger.expect("a clear"),
                    &mut place,
                    now,
                )
                .lines,
        );
        trace.extend(
            ledger
                .announce_turn_end(
                    at,
                    Reach::Flash,
                    crate::attention::NotificationSwitches {
                        turn_end: true,
                        desktop_messages: true,
                    },
                    attention_map::PIPE_TRANSPORT,
                    stop.turn_end.expect("an announcement"),
                    None,
                )
                .lines,
        );
        assert_eq!(ledger.state(), State::Idle);
        // ⑤ **And the second request is a second request.** This is the cell the whole watermark
        // design exists for: a level-mode kind with no identifier, asked again after an answer,
        // must not be swallowed as a restatement of the one that was answered.
        let asks = say("PermissionRequest");
        trace.extend(
            ledger
                .apply(
                    at,
                    Reach::Flash,
                    asks.ledger.expect("a wait"),
                    &mut place,
                    now,
                )
                .lines,
        );
        assert_eq!(ledger.state(), State::Requested(2));

        assert_eq!(
            trace,
            [
                "mint tab=0 seat=SeatId(0) episode=1 src=pipe gen=1 grounds=awaiting prev=-",
                "admit tab=0 seat=SeatId(0) ticket=0 episode=1 grounds=awaiting active=0 focused=0",
                "toast tab=0 seat=SeatId(0) why=awaiting ticket=0 episode=1 reach=flash",
                "answer tab=0 seat=SeatId(0) ticket=0 episode=1 by=keyboard weak=0 strong=1",
                "clear tab=0 seat=SeatId(0) episode=1 src=pipe gen=1 reason=hook",
                "drop tab=0 seat=SeatId(0) episode=1 reason=hook",
                "toast tab=0 seat=SeatId(0) why=turn-end episode=- reach=flash src=pipe via=stop",
                "mint tab=0 seat=SeatId(0) episode=2 src=pipe gen=2 grounds=awaiting prev=1",
            ],
            "the trace is the evidence this chain works, so its exact words are the assertion"
        );
    }

    /// **The whole of the new lane, from a file on disk to the body of a toast.**
    ///
    /// Every link between the two ends has its own test above and **none of them can fail the way
    /// an integration fails**: a row that declares `transcript_path`, a reader that looks for
    /// `transcript_path`, a wire that carries what it found and a ledger that lends it to the
    /// announcement are four correct things that could still be spelling four different words.
    /// What this asserts is the sentence a reader would actually see.
    #[test]
    fn a_turn_that_ended_with_words_says_them_on_the_desktop() {
        use crate::attention::{AttentionLedger, NotificationSwitches, Reach, Site, Why};
        use bt_layout::SeatId;

        let path = std::env::temp_dir().join(format!(
            "folio-attention-wire-{}-transcript.jsonl",
            std::process::id()
        ));
        std::fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::json!({
                    "type": "assistant",
                    "message": { "role": "assistant", "content": [{
                        "type": "text",
                        "text": "**收官了。** 剩下的写在交接里。",
                    }]},
                })
            ),
        )
        .expect("a scratch transcript");

        // What the hook hands the verb, and what the verb decides to say.
        let payload = serde_json::json!({
            "session_id": "abc",
            "hook_event_name": "Stop",
            "transcript_path": path.to_string_lossy(),
        });
        let sent = message_for("cap".to_owned(), CLAUDE_CODE, "Stop", Some(&payload));
        let _ = std::fs::remove_file(&path);

        // The line, read back at the window's end exactly as it arrived.
        let arrived = Message::decode(&sent.encode()).expect("a line this build wrote");
        assert_eq!(arrived.text.as_deref(), Some("收官了。"));

        // And the announcement it becomes.
        let asks = asks_of(&installed(CLAUDE_CODE), &arrived);
        let raised = AttentionLedger::default()
            .announce_turn_end(
                Site {
                    tab: 0,
                    seat: SeatId(0),
                },
                Reach::Toast,
                NotificationSwitches {
                    turn_end: true,
                    desktop_messages: true,
                },
                attention_map::PIPE_TRANSPORT,
                asks.turn_end.expect("an announcement"),
                arrived.text.as_deref(),
            )
            .raised
            .expect("an interruption");
        assert_eq!(raised.why, Why::TurnEnd);
        assert_eq!(
            raised.body.as_deref(),
            Some("收官了。"),
            "the body is the agent's own sentence, with no label in front of it"
        );
    }

    /// The verb's help lists every family the tables know, and gains one when a family is data.
    #[test]
    fn the_known_names_come_from_the_tables_themselves() {
        let names = known_events();
        assert!(names.contains(&"claude-code:PermissionRequest".to_owned()));
        assert!(names.contains(&format!("{CODEX}:permission-request")));
        assert!(names.contains(&format!("{PI}:agent_settled")));
        assert!(
            names.windows(2).all(|pair| pair[0] < pair[1]),
            "sorted and unique"
        );
    }
}
