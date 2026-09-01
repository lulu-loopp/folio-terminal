//! **Whether a newer Folio exists** — asked once a day, answered by a mark on
//! the gear, and never acted on (`docs/DESIGN.md` §7.51).
//!
//! # What this is, stated as a bound
//!
//! One `GET` of one fixed address, at most once every twenty-four hours across
//! every window on the machine, on a thread of its own, carrying nothing about
//! the machine or the person at it, failing in complete silence, and downloading
//! nothing whatever it learns. That sentence is the whole feature, and every
//! part of it is load-bearing:
//!
//! * **One address.** [`RELEASES_HOST`] and [`RELEASES_PATH`] are constants; no
//!   part of the answer can redirect the next question, because there is no next
//!   question.
//! * **Once a day, across windows.** The stamp in `update-check.json` answers
//!   *is it time yet*; a claim file beside it answers *is another window already
//!   asking*. Two windows opened together make one request, and the second one
//!   does not queue behind the first — it simply does not ask. See
//!   [`run`].
//! * **Its own thread.** Nothing on the path from `main` to the first frame
//!   waits for this. The thread is started after the window exists and its
//!   answer arrives as an ordinary wake, exactly the way the PSReadLine probe's
//!   does.
//! * **Nothing about the machine.** The request carries a `User-Agent` of
//!   [`USER_AGENT`] and not one byte more — no version, no identifier, no
//!   cookie, no query string. See [`USER_AGENT`] for why it is not empty.
//! * **Silent.** Every failure — no network, DNS, a proxy, a rate limit, a
//!   response that is not JSON, a tag that is not a version — is the same
//!   outcome: the stamp advances and nothing is said. A terminal that reported
//!   its update check's problems would be a terminal that talked about itself.
//! * **Downloads nothing.** There is no installer, no replacement, no restart.
//!   The most this feature can do is put a dot on a gear and a sentence in a
//!   dialog, and the one press it offers hands an address to the browser.
//!
//! # Why the stamp advances on failure
//!
//! It is the whole of the no-retry-storm rule. A laptop on a train would
//! otherwise fail, find itself still due, and fail again — once per window, per
//! launch, forever — and the machine that suffers most is the one least able to
//! answer. The stamp records *when we asked*, not *when we were told*, so a
//! week offline is seven attempts and not seven thousand.
//!
//! # Why the tag is compared and not the date
//!
//! A release's date says when somebody pressed a button; its tag says what they
//! pressed it on. `v0.1.0-preview` and `0.1.0` are the same three numbers with
//! different standing, and semantic versioning already has the answer —
//! `0.1.0-preview < 0.1.0 < 0.1.1` — so [`Version`] implements it rather than
//! inventing a comparison. Two things fall out of that and both are tested: a
//! build's own `+hash` is metadata and **not** a version, so the same release
//! built twice is not an update; and the shipped `v0.1.0-preview` is *older*
//! than the `0.1.0` in the binary that shipped under it, so the first thing this
//! code ever did on a real machine was correctly say nothing.

use std::{
    cmp::Ordering,
    fs::OpenOptions,
    io::Write as _,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bt_persist::UpdateCheckV1;

/// The host the question goes to.
pub const RELEASES_HOST: &str = "api.github.com";

/// The path beside it — the release **list**.
///
/// **Not `/releases/latest`, and the reason was measured rather than reasoned
/// about.** That endpoint is the obvious one and it was the first thing this
/// code called; against this repository it answers `404`. GitHub's `latest`
/// deliberately excludes drafts *and pre-releases*, and every release Folio has
/// ever published is a pre-release — so the endpoint that looks right would have
/// left this feature silently dead on every machine, in the failure mode this
/// module is otherwise built to have: no error, no mark, nothing to notice.
///
/// The list has no such rule. It carries every published release, newest first,
/// and never carries a draft — a draft is visible only to somebody with push
/// access, and this request sends no credentials, so there is nothing here that
/// filters for one.
pub const RELEASES_PATH: &str = "/repos/lulu-loopp/folio-terminal/releases";

/// The page a press opens, which is for a person rather than for this code.
pub const RELEASES_PAGE: &str = "https://github.com/lulu-loopp/folio-terminal/releases";

/// The `User-Agent` the request travels under, and the only thing it says.
///
/// **Not empty, and the reason is not ours**: GitHub's API refuses a request
/// with no agent at all, with a `403` and a sentence about it. So the choice is
/// not *whether* to identify the program but *how much*, and this is the least
/// that works — the product's name, with no version, no build, no operating
/// system and no identifier of any kind. A version here would let a server count
/// installs per release, which is a thing this product does not collect and
/// therefore must not hand somebody else the ability to collect either.
pub const USER_AGENT: &str = "Folio";

/// `update-check.json`, beside `settings.json`.
pub const STATE_FILE_NAME: &str = "update-check.json";

/// The claim file — held for the length of one request and no longer.
pub const CLAIM_FILE_NAME: &str = "update-check.lock";

/// Twenty-four hours, in milliseconds.
pub const CHECK_INTERVAL_MS: u64 = 24 * 60 * 60 * 1_000;

/// How old a claim has to be before it is read as abandoned rather than held.
///
/// A claim is dropped by the thread that took it, including when that thread's
/// request fails — so a claim outliving this is a process that was killed
/// between taking one and finishing. Five minutes is far outside anything
/// [`BUDGET`] permits and far inside "the user has noticed the check stopped
/// working", which are the two edges this number has to sit between.
pub const CLAIM_STALE_MS: u64 = 5 * 60 * 1_000;

/// Each of WinHTTP's four phase timeouts.
const PHASE_TIMEOUT: Duration = Duration::from_secs(5);

/// The whole request's own deadline.
const BUDGET: Duration = Duration::from_secs(15);

/// The most response this will read.
///
/// Measured, not guessed: one release object with its notes and its four assets
/// is 18 KB against this repository today, and the list returns thirty of them
/// at a time — so a full page is about half a megabyte and this is twice that.
/// A body that outgrows it is a day with no answer and no mark, which is the
/// same silence every other failure here produces.
const BODY_CAP_BYTES: usize = 1_024 * 1_024;

// ── the version, and what makes one newer than another ──────────────────────

/// A semantic version, parsed from a release tag or from `CARGO_PKG_VERSION`.
///
/// Build metadata is deliberately **absent from the type** rather than parsed
/// and ignored: semantic versioning says it takes no part in precedence, and a
/// field nobody may compare is a field somebody eventually compares.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Version {
    core: [u64; 3],
    /// The dot-separated identifiers after the `-`. Empty is a release, and a
    /// release outranks every pre-release of the same three numbers.
    pre: Vec<PreIdent>,
}

/// One identifier of a pre-release string.
///
/// The split is the whole of semantic versioning's §11.4.1–2: identifiers that
/// are all digits compare as numbers, so `rc.2` precedes `rc.10`; anything else
/// compares as ASCII text; and a numeric identifier always precedes an
/// alphanumeric one.
#[derive(Clone, Debug, Eq, PartialEq)]
enum PreIdent {
    Numeric(u64),
    Text(String),
}

impl Version {
    /// Read a tag, or `None` if it is not a version this code can order.
    ///
    /// A leading `v` is accepted because that is how the tags in this repository
    /// are spelled, and stripping it here is what lets `VERSION` — which has no
    /// `v` — and a tag be handed to the same function.
    ///
    /// **Silence is the failure mode.** A tag that is not a version returns
    /// `None`, the check treats that exactly as it treats a refused connection,
    /// and nothing is drawn. A build that guessed at an unparseable tag would be
    /// a build that could invent an update.
    #[must_use]
    pub fn parse(tag: &str) -> Option<Self> {
        let tag = tag.trim();
        let tag = tag
            .strip_prefix('v')
            .or_else(|| tag.strip_prefix('V'))
            .unwrap_or(tag);
        // Build metadata is dropped here and not stored: `+` may not appear
        // anywhere else, so the first one ends the part that matters.
        let tag = match tag.split_once('+') {
            Some((before, build)) => {
                if build.is_empty() || !build.split('.').all(is_identifier) {
                    return None;
                }
                before
            }
            None => tag,
        };
        let (core, pre) = match tag.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (tag, None),
        };

        let mut fields = core.split('.');
        let mut numbers = [0u64; 3];
        for slot in &mut numbers {
            let field = fields.next()?;
            if field.is_empty() || !field.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            *slot = field.parse().ok()?;
        }
        if fields.next().is_some() {
            return None;
        }

        let pre = match pre {
            None => Vec::new(),
            Some(pre) => {
                let mut identifiers = Vec::new();
                for field in pre.split('.') {
                    if !is_identifier(field) {
                        return None;
                    }
                    identifiers.push(if field.bytes().all(|byte| byte.is_ascii_digit()) {
                        // A numeric identifier too long for a `u64` is not a
                        // number this can order, and pretending otherwise
                        // would put two different releases in the same place.
                        PreIdent::Numeric(field.parse().ok()?)
                    } else {
                        PreIdent::Text(field.to_owned())
                    });
                }
                identifiers
            }
        };

        Some(Self { core: numbers, pre })
    }
}

/// Whether one dot-separated field is a legal identifier: non-empty, and made of
/// digits, ASCII letters and hyphens.
fn is_identifier(field: &str) -> bool {
    !field.is_empty()
        && field
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.core.cmp(&other.core) {
            Ordering::Equal => {}
            decided => return decided,
        }
        // "A pre-release version has lower precedence than the associated
        // normal version" — the one rule that is not a list comparison, and the
        // one this product actually needed: `0.1.0-preview` is what shipped and
        // `0.1.0` is what is in the binary that shipped under it.
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => self.pre.cmp(&other.pre),
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PreIdent {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Numeric(left), Self::Numeric(right)) => left.cmp(right),
            (Self::Text(left), Self::Text(right)) => left.cmp(right),
            // "Numeric identifiers always have lower precedence than
            // alphanumeric identifiers."
            (Self::Numeric(_), Self::Text(_)) => Ordering::Less,
            (Self::Text(_), Self::Numeric(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for PreIdent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// **The highest version in a release list**, verbatim, or `None`.
///
/// Two decisions in four lines, and both are about not trusting the order the
/// server happened to send:
///
/// * **The maximum, not the first.** GitHub sorts this list by when each
///   release was *created*, and those are not the same order. A `0.1.4`
///   published today for a line somebody is still maintaining would stand ahead
///   of the `0.2.0` published last month, and a reader on `0.2.0` would be
///   offered a downgrade.
/// * **A tag that is not a version is skipped, not fatal.** One `nightly` in the
///   list must not take the whole answer with it, which is what a `?` here would
///   do.
///
/// A named field of a struct with one field in it rather than a walk through a
/// `Value`, so that a response shaped like anything else is a parse failure —
/// which is a silence — instead of a `None` that looks like an empty list.
#[must_use]
pub fn newest_tag(body: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: String,
    }
    serde_json::from_str::<Vec<Release>>(body)
        .ok()?
        .into_iter()
        .filter_map(|release| Version::parse(&release.tag_name).map(|it| (it, release.tag_name)))
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, tag)| tag)
}

/// **The one question the rest of the window asks**: is the tag we last heard
/// about newer than the build that is running?
///
/// `Some(tag)` carries the tag verbatim, because the tag is what the sentence in
/// the dialog says and what the state file compares against. `None` covers every
/// other case in one: no answer yet, an answer that will not parse, a running
/// version that will not parse, the same version, and an older one.
#[must_use]
pub fn newer_than<'tag>(latest: Option<&'tag str>, running: &str) -> Option<&'tag str> {
    let latest = latest?;
    let found = Version::parse(latest)?;
    let running = Version::parse(running)?;
    (found > running).then_some(latest)
}

/// Whether the gear wears its mark.
///
/// Two conditions and both are necessary: there is a newer version, **and** this
/// reader has not been shown this one. The second is what stops a dot that has
/// been answered from coming back on the next launch, and it is keyed by the tag
/// rather than by a flag, so the next release lights it again without anything
/// having to clear anything.
#[must_use]
pub fn mark_is_lit(state: &UpdateCheckV1, running: &str) -> bool {
    newer_than(state.latest_tag.as_deref(), running).is_some()
        && state.latest_tag.as_deref() != state.seen_tag.as_deref()
}

/// Whether the releases page is owed a question.
///
/// **A clock that has gone backwards is due**, which is the one case worth
/// spelling out: a machine whose time was wrong and has been corrected holds a
/// stamp in its own future, and a plain subtraction would read that as "checked
/// recently" — forever, because the stamp can only be rewritten by a check that
/// the stamp is preventing.
#[must_use]
pub fn due(checked_at_ms: u64, now_ms: u64) -> bool {
    now_ms < checked_at_ms || now_ms - checked_at_ms >= CHECK_INTERVAL_MS
}

// ── the check itself ────────────────────────────────────────────────────────

/// Where a tag comes from.
///
/// A trait with one method so the whole of [`run`] can be tested without a
/// network: the tests hand it a source that counts its calls and answers from a
/// string, and the product hands it [`GitHubReleases`]. Nothing else in this
/// module knows that HTTP exists.
pub trait Releases {
    /// The latest release's tag, or a sentence nobody reads.
    ///
    /// # Errors
    ///
    /// Every failure, which the caller treats identically.
    fn latest_tag(&self) -> Result<String, String>;
}

/// The real one: one `GET`, over the operating system's own stack.
pub struct GitHubReleases;

impl Releases for GitHubReleases {
    #[cfg(windows)]
    fn latest_tag(&self) -> Result<String, String> {
        let body = bt_platform::http::https_get(&bt_platform::http::HttpsGet {
            host: RELEASES_HOST,
            path: RELEASES_PATH,
            user_agent: USER_AGENT,
            phase_timeout: PHASE_TIMEOUT,
            budget: BUDGET,
            cap: BODY_CAP_BYTES,
        })?;
        newest_tag(&body).ok_or_else(|| "the answer carries no version".to_owned())
    }

    #[cfg(not(windows))]
    fn latest_tag(&self) -> Result<String, String> {
        Err("this build has no HTTP stack".to_owned())
    }
}

/// What one call to [`run`] did, for the tests and for nobody else.
///
/// The product ignores it: every arm below the first two ends in the same place,
/// which is a state file on a disk and a window that may or may not draw a dot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The stamp says the last question was asked less than a day ago. No claim
    /// was taken and no request was made.
    TooSoon,
    /// Another window holds the claim. Same two negatives.
    Busy,
    /// A tag came back and is now in the file.
    Answered(String),
    /// The question was asked and did not come back. The stamp advanced anyway.
    Refused,
}

/// **The whole check, on the calling thread, against an injected world.**
///
/// The order of the first three steps is the two-window rule, and it is the
/// order rather than the steps that makes it true:
///
/// 1. **Take the claim first.** Not "decide, then claim" — two windows that both
///    read a stale stamp before either wrote one would both decide to ask.
///    Everything that reads or writes the stamp happens inside the claim.
/// 2. **Then read the stamp**, and let go if it is not time yet.
/// 3. **Then write the stamp**, before the request rather than after it, so a
///    window that starts while this one is waiting on a socket sees a fresh
///    stamp the moment this one lets go of the claim.
///
/// A window that finds the claim held does **not** wait: it does nothing at all
/// this launch, and its gear draws whatever the file said when it opened. The
/// alternative — blocking a thread until the other window's request finishes,
/// then reading the answer — would buy one dot one launch earlier at the price
/// of a thread that can be made to wait on somebody else's network.
pub fn run(dir: &Path, now_ms: u64, source: &dyn Releases) -> Outcome {
    let Some(_claim) = Claim::take(&dir.join(CLAIM_FILE_NAME), now_ms) else {
        return Outcome::Busy;
    };

    let path = dir.join(STATE_FILE_NAME);
    let (mut state, _) = bt_persist::read_update_check(&path);
    if !due(state.checked_at_ms, now_ms) {
        return Outcome::TooSoon;
    }

    state.checked_at_ms = now_ms;
    let _ = bt_persist::write_update_check_atomic(&path, &state);

    match source.latest_tag() {
        Ok(tag) => {
            state.latest_tag = Some(tag.clone());
            let _ = bt_persist::write_update_check_atomic(&path, &state);
            Outcome::Answered(tag)
        }
        Err(_) => Outcome::Refused,
    }
}

/// Write the tag this reader has now been shown into the state file.
///
/// Read-modify-write rather than a store of what the caller had, because the
/// caller's copy is as old as the frame it came from and the field beside this
/// one is written by a thread.
pub fn mark_seen(dir: &Path, tag: &str) {
    let path = dir.join(STATE_FILE_NAME);
    let (mut state, _) = bt_persist::read_update_check(&path);
    if state.seen_tag.as_deref() == Some(tag) {
        return;
    }
    state.seen_tag = Some(tag.to_owned());
    let _ = bt_persist::write_update_check_atomic(&path, &state);
}

/// **The reader has been shown the page the row is on**: put the mark out.
///
/// Idempotent and cheap to call every frame the page is up — it reads
/// [`known`], which is in memory, and touches the disk only on the one frame
/// that actually changes the answer.
///
/// **The gear does not redraw on that frame, and it does not need to.** The page
/// this is called from is a modal standing over the title bar with the scrim
/// dimming everything behind it, so nobody is looking at the mark while it goes
/// out; and closing the dialog rebuilds the whole of the chrome, which is the
/// next moment the gear is a thing anybody can see.
pub fn answer_mark(dir: &Path) {
    let state = known();
    if !mark_is_lit(&state, crate::version::VERSION) {
        return;
    }
    let Some(tag) = state.latest_tag.as_deref() else {
        return;
    };
    mark_seen(dir, tag);
    load(dir);
}

/// The right to be the window that asks, held for one request.
///
/// A file created with `create_new`, which is one atomic kernel operation and
/// therefore an actual mutex rather than a read followed by a hopeful write. It
/// carries the millisecond it was taken at so that a claim left behind by a
/// process that was killed can be told from one that is being used — see
/// [`CLAIM_STALE_MS`].
struct Claim(PathBuf);

impl Claim {
    fn take(path: &Path, now_ms: u64) -> Option<Self> {
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(mut file) => {
                let _ = write!(file, "{now_ms}");
                Some(Self(path.to_owned()))
            }
            Err(_) => {
                // Either another window is asking, or a process died holding
                // this. The stamp inside says which, and a claim whose stamp
                // cannot be read at all is one whose writer did not survive
                // writing it.
                let held_since = std::fs::read_to_string(path)
                    .ok()
                    .and_then(|text| text.trim().parse::<u64>().ok());
                let abandoned = match held_since {
                    None => true,
                    Some(stamp) => now_ms < stamp || now_ms - stamp > CLAIM_STALE_MS,
                };
                if !abandoned {
                    return None;
                }
                // **Taking over is deliberately not atomic**, and the cost of
                // that is bounded: two windows recovering the same abandoned
                // claim in the same instant make two requests, once, after a
                // crash. Closing it would need a second mutex whose own
                // abandonment would need a third.
                let mut file = std::fs::File::create(path).ok()?;
                let _ = write!(file, "{now_ms}");
                Some(Self(path.to_owned()))
            }
        }
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        // A claim that cannot be removed becomes an abandoned one, which the
        // next window recovers. There is nothing better to do and nobody to
        // tell.
        let _ = std::fs::remove_file(&self.0);
    }
}

// ── what the window reads, and how the answer gets back to it ───────────────

/// The state as this process last saw it.
///
/// A `Mutex` and not a `OnceLock`, because it is written twice: once on the
/// window thread at startup from the file, and once more from the checking
/// thread when an answer lands.
static KNOWN: Mutex<Option<UpdateCheckV1>> = Mutex::new(None);

/// How a finished check asks for a frame — [`install_wake`].
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Install the repaint the answer needs, once per process.
///
/// The same shape as `psreadline::install_wake` and for the same reason: the
/// answer can land while the window is sitting on a modal with nothing else to
/// draw, and the mark it lights is on the title bar behind that modal.
pub fn install_wake<F: Fn() + Send + Sync + 'static>(wake: F) {
    let _ = WAKE.set(Box::new(wake));
}

/// Read the file into [`KNOWN`], so the first frame draws the right gear.
///
/// On the window thread, at startup, before anything is measured. It is one
/// small file beside `settings.json`, which is read on the same thread a few
/// lines earlier; the thing that must not be on this thread is the *request*,
/// and that is [`begin`]'s.
pub fn load(dir: &Path) {
    let (state, _) = bt_persist::read_update_check(&dir.join(STATE_FILE_NAME));
    *KNOWN
        .lock()
        .expect("the update state is not held across a panic") = Some(state);
}

/// What the chrome and the dialog read.
#[must_use]
pub fn known() -> UpdateCheckV1 {
    KNOWN
        .lock()
        .expect("the update state is not held across a panic")
        .clone()
        .unwrap_or_default()
}

/// **The settings row's own sentence.**
///
/// A function on this module rather than a literal in the table, on
/// `context_menu::row_description`'s footing: what the row says depends on a
/// fact about the world, and the module that owns the fact is the one that
/// should be asked. The base sentence is a table entry like every other row's;
/// only the one that names a version is composed.
#[must_use]
pub fn row_description() -> &'static str {
    row_description_in(crate::i18n::current())
}

/// The same in a named language — the entry point for a test that reads both
/// columns.
#[must_use]
pub fn row_description_in(lang: crate::i18n::Lang) -> &'static str {
    let state = known();
    match newer_than(state.latest_tag.as_deref(), crate::version::VERSION) {
        None => crate::i18n::Text::DescUpdateCheck.in_lang(lang),
        Some(tag) => intern(crate::i18n::update_row_available_in(lang, tag)),
    }
}

/// **A composed sentence that has to outlive the frame that composed it.**
///
/// `SettingsRow::description` answers `&'static str` — every row in the dialog
/// hands its answer straight to a `ChromeLabel`, and the whole table costs zero
/// allocations because of it. `psreadline::row_description` meets the same
/// signature with a `OnceLock` per language, which is sound there because its
/// probe is a one-shot; it is **not** sound here, because a process can be told
/// about `0.1.1` by the file it read at startup and about `0.1.2` by the check
/// that finished a second later, and a `OnceLock` would go on drawing the first
/// one under a mark lit for the second.
///
/// So the sentences are interned instead, and the pool is bounded by the thing
/// that generates them: one entry per language per distinct version this process
/// is told about, which is two languages and at most two versions — the one on
/// disk at startup and the one a single check can replace it with. It does not
/// grow with frames, with dialog opens, or with time.
fn intern(text: String) -> &'static str {
    static POOL: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut pool = POOL
        .lock()
        .expect("the sentence pool is not held across a panic");
    if let Some(held) = pool.iter().find(|held| **held == text) {
        return held;
    }
    let held: &'static str = Box::leak(text.into_boxed_str());
    pool.push(held);
    held
}

/// Start the one check this process makes.
///
/// A no-op when the switch is off, and that is the whole of the switch: no
/// thread, no claim, no file. Off is not a quieter check.
pub fn begin(dir: PathBuf, enabled: bool) {
    if !enabled {
        return;
    }
    // **In the background band.** A thread starts at normal priority whatever
    // the thread that spawned it was running at, and this one would otherwise
    // stand beside the window's loop for the length of a DNS lookup — see
    // `git::drain`, which is where this crate first wrote that down. A kernel
    // that will not give out a thread is a launch that simply has no update
    // check, which is where every launch before this slice was.
    let _ = bt_platform::spawn_at_priority(
        "bt-update-check",
        bt_platform::ThreadPriority::BelowNormal,
        move || {
            let now_ms = unix_epoch_ms();
            let outcome = run(&dir, now_ms, &GitHubReleases);
            if matches!(outcome, Outcome::Answered(_)) {
                load(&dir);
                if let Some(wake) = WAKE.get() {
                    wake();
                }
            }
        },
    );
}

/// The wall clock, in milliseconds since the Unix epoch.
///
/// The wall clock and not a monotonic one, because the number has to survive the
/// process that wrote it: "a day since the last check" is a question about two
/// different runs of the program, and a monotonic instant means nothing to the
/// second one. A clock that moves under this is what [`due`]'s backwards case is
/// for.
fn unix_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{
        CHECK_INTERVAL_MS, CLAIM_STALE_MS, Outcome, Releases, STATE_FILE_NAME, Version, due,
        mark_is_lit, mark_seen, newer_than, newest_tag, run,
    };
    use bt_persist::UpdateCheckV1;
    use std::{
        cell::RefCell,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU32, Ordering},
    };

    /// A private directory for one test, cleaned on the way in as well as out —
    /// `persist::tests::appdata`'s rule, for its reason.
    fn dir(case: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bt-update-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private directory for this test");
        root
    }

    /// A source that counts, and answers the same thing every time.
    struct Counting {
        answer: Result<String, String>,
        calls: AtomicU32,
    }

    impl Counting {
        fn ok(tag: &str) -> Self {
            Self {
                answer: Ok(tag.to_owned()),
                calls: AtomicU32::new(0),
            }
        }
        fn refusing() -> Self {
            Self {
                answer: Err("no network".to_owned()),
                calls: AtomicU32::new(0),
            }
        }
        fn calls(&self) -> u32 {
            self.calls.load(Ordering::Relaxed)
        }
    }

    impl Releases for Counting {
        fn latest_tag(&self) -> Result<String, String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.answer.clone()
        }
    }

    fn state_of(dir: &Path) -> UpdateCheckV1 {
        bt_persist::read_update_check(&dir.join(STATE_FILE_NAME)).0
    }

    /// PIN — **the order semantic versioning defines, including the two cases
    /// this product actually shipped into.**
    ///
    /// The list is not decoration. `0.1.0-preview < 0.1.0` is the tag that is on
    /// the releases page against the version that is inside the binary it
    /// carries: read the other way round, every existing install would announce
    /// an update to itself on first launch. `0.1.0+abc == 0.1.0` is the same
    /// release built twice, which the ticket names in as many words.
    ///
    /// MUTATION: make the pre-release arm of `Version::cmp` answer `Equal`
    /// instead of `Greater`/`Less` and the `0.1.0-preview` pair goes red; drop
    /// the `+` split in `parse` and the build-metadata pair goes red; compare
    /// `PreIdent`s as text only and `rc.2 < rc.10` goes red.
    #[test]
    fn a_tag_is_ordered_the_way_semantic_versioning_says() {
        let ascending = [
            "v0.1.0-alpha",
            "v0.1.0-alpha.1",
            "v0.1.0-alpha.beta",
            "v0.1.0-beta",
            "v0.1.0-preview",
            "v0.1.0-rc.2",
            "v0.1.0-rc.10",
            "v0.1.0",
            "v0.1.1",
            "v0.2.0",
            "v1.0.0",
            "v1.0.10",
            "v1.2.0",
            "v10.0.0",
        ];
        for (index, lower) in ascending.iter().enumerate() {
            let lower_v = Version::parse(lower).unwrap_or_else(|| panic!("{lower} parses"));
            for higher in &ascending[index + 1..] {
                let higher_v = Version::parse(higher).expect("parses");
                assert!(lower_v < higher_v, "{lower} must come before {higher}");
            }
        }

        // Build metadata takes no part in precedence: the same release built
        // from two commits is one release.
        assert_eq!(
            Version::parse("0.1.0+abc").expect("parses"),
            Version::parse("0.1.0").expect("parses")
        );
        assert_eq!(
            Version::parse("v0.1.0+abc").expect("parses"),
            Version::parse("0.1.0+def").expect("parses")
        );

        // The `v` is a spelling of the tag and not part of the version.
        assert_eq!(
            Version::parse("v1.2.3").expect("parses"),
            Version::parse("1.2.3").expect("parses")
        );

        // And what is not a version is nothing at all, silently.
        for junk in [
            "",
            "v",
            "1",
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "1.-2.3",
            "1.2.3-",
            "1.2.3-+",
            "1.2.3+",
            "latest",
            "v1.2.3 (rc)",
            "1.2.3-rc!",
            "01.2.3-α",
        ] {
            assert!(
                Version::parse(junk).is_none(),
                "{junk:?} is not a version this code may order"
            );
        }
    }

    /// PIN — **what the window is told, out of a tag and a running build.**
    ///
    /// MUTATION: relax `>` to `>=` in `newer_than` and the same-version case
    /// goes red — which is the ticket's "同版不同 hash 不算新版" said as an
    /// assertion.
    #[test]
    fn only_a_strictly_newer_tag_is_an_update() {
        assert_eq!(newer_than(Some("v0.1.1"), "0.1.0"), Some("v0.1.1"));
        assert_eq!(newer_than(Some("v0.1.0"), "0.1.0"), None);
        assert_eq!(newer_than(Some("v0.1.0+deadbee"), "0.1.0"), None);
        assert_eq!(newer_than(Some("v0.1.0-preview"), "0.1.0"), None);
        assert_eq!(newer_than(Some("v0.0.9"), "0.1.0"), None);
        assert_eq!(newer_than(None, "0.1.0"), None);
        assert_eq!(newer_than(Some("nightly"), "0.1.0"), None);

        // The tag comes back verbatim, because the tag is what the state file
        // compares and what the sentence in the dialog says.
        assert_eq!(newer_than(Some("v0.2.0"), "0.1.0"), Some("v0.2.0"));
    }

    /// PIN — **a version whose mark has been answered does not light again, and
    /// the next one does.**
    ///
    /// MUTATION: drop the `latest != seen` clause from `mark_is_lit` and the
    /// second assertion goes red; key the seen mark on a `bool` instead of the
    /// tag and the fourth does.
    #[test]
    fn a_mark_that_has_been_answered_stays_out_until_the_next_release() {
        let root = dir("seen");
        let mut state = UpdateCheckV1 {
            latest_tag: Some("v0.1.1".to_owned()),
            ..UpdateCheckV1::default()
        };
        assert!(mark_is_lit(&state, "0.1.0"), "a newer tag lights the mark");

        mark_seen(&root, "v0.1.1");
        state.seen_tag = state_of(&root).seen_tag;
        assert!(
            !mark_is_lit(&state, "0.1.0"),
            "the mark goes out once this reader has been shown this version"
        );

        // And the next release lights it again with nothing having to clear
        // anything.
        state.latest_tag = Some("v0.1.2".to_owned());
        assert!(
            mark_is_lit(&state, "0.1.0"),
            "a version that has not been seen lights the mark whatever was seen before"
        );

        // A mark is never lit for a version this build is already past, however
        // long ago it was seen.
        let old = UpdateCheckV1 {
            latest_tag: Some("v0.0.9".to_owned()),
            ..UpdateCheckV1::default()
        };
        assert!(!mark_is_lit(&old, "0.1.0"));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — **one question a day, and the answer to "when" is the stamp rather
    /// than the launch.**
    ///
    /// MUTATION: write the stamp only on the success arm of `run` and the
    /// refusing half goes red; drop the `due` guard and every half does.
    #[test]
    fn the_releases_page_is_asked_at_most_once_a_day() {
        let root = dir("throttle");
        let source = Counting::ok("v0.1.1");

        let start = 1_756_000_000_000u64;
        assert_eq!(
            run(&root, start, &source),
            Outcome::Answered("v0.1.1".to_owned())
        );
        assert_eq!(source.calls(), 1);
        assert_eq!(state_of(&root).checked_at_ms, start);
        assert_eq!(state_of(&root).latest_tag.as_deref(), Some("v0.1.1"));

        // Every launch inside the day — a second window, a restart, a hundred
        // restarts — asks nothing.
        for offset in [1, 1_000, 60_000, CHECK_INTERVAL_MS - 1] {
            assert_eq!(run(&root, start + offset, &source), Outcome::TooSoon);
        }
        assert_eq!(source.calls(), 1, "no launch inside the day asks again");

        // And the day after, exactly one more.
        assert_eq!(
            run(&root, start + CHECK_INTERVAL_MS, &source),
            Outcome::Answered("v0.1.1".to_owned())
        );
        assert_eq!(source.calls(), 2);

        // A stamp from the future is a clock that moved, not a check that
        // happened: it does not lock the check out forever.
        assert!(due(start + CHECK_INTERVAL_MS, start));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — **a machine that cannot reach the network asks once, not once per
    /// launch.**
    ///
    /// This is the no-retry-storm rule, and it is the reason the stamp is
    /// written *before* the request rather than after it. The failure is
    /// complete and silent: nothing is written into `latest_tag`, so nothing is
    /// drawn, and the state file is otherwise exactly what a successful check
    /// would have left.
    ///
    /// MUTATION: move the stamp write below `source.latest_tag()` — or into the
    /// `Ok` arm — and the call count goes to eleven.
    #[test]
    fn a_refused_question_is_silent_and_is_not_asked_again_until_tomorrow() {
        let root = dir("refused");
        let source = Counting::refusing();

        let start = 1_756_000_000_000u64;
        assert_eq!(run(&root, start, &source), Outcome::Refused);
        for offset in 1..10u64 {
            assert_eq!(
                run(&root, start + offset * 60_000, &source),
                Outcome::TooSoon
            );
        }
        assert_eq!(source.calls(), 1, "ten launches offline, one attempt");

        let state = state_of(&root);
        assert_eq!(
            state.checked_at_ms, start,
            "the stamp advanced on the refusal"
        );
        assert_eq!(state.latest_tag, None, "and nothing was invented to draw");
        assert!(!mark_is_lit(&state, "0.1.0"));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — **two windows opened together ask once.**
    ///
    /// The second window is simulated from *inside* the first one's request,
    /// which is the only moment the two can actually collide: the source's
    /// answer is produced while the first window holds the claim, and it calls
    /// [`run`] again from there. Anything short of a real mutex — a stamp read
    /// then written, a flag in this process — lets that inner call through.
    ///
    /// MUTATION (both run): make `Claim::take` answer `Some` without touching a
    /// file, and the inner call comes back `TooSoon` — the stamp, not a mutex,
    /// doing the excluding, which is exactly the arrangement that loses when the
    /// two windows are a millisecond apart instead of nested. Move `Claim::take`
    /// below the `due` check in `run` and it goes red the same way.
    #[test]
    fn a_second_window_asking_at_the_same_instant_does_not_ask() {
        struct Reentrant {
            dir: PathBuf,
            now_ms: u64,
            inner: RefCell<Option<Outcome>>,
            calls: AtomicU32,
        }
        impl Releases for Reentrant {
            fn latest_tag(&self) -> Result<String, String> {
                self.calls.fetch_add(1, Ordering::Relaxed);
                // The second window, opened while this request is in flight.
                let second = run(&self.dir, self.now_ms, &Counting::ok("v9.9.9"));
                *self.inner.borrow_mut() = Some(second);
                Ok("v0.1.1".to_owned())
            }
        }

        let root = dir("two-windows");
        let source = Reentrant {
            dir: root.clone(),
            now_ms: 1_756_000_000_000,
            inner: RefCell::new(None),
            calls: AtomicU32::new(0),
        };
        assert_eq!(
            run(&root, 1_756_000_000_000, &source),
            Outcome::Answered("v0.1.1".to_owned())
        );
        assert_eq!(source.calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            source.inner.into_inner(),
            Some(Outcome::Busy),
            "the second window found the claim held and asked nothing"
        );
        assert_eq!(
            state_of(&root).latest_tag.as_deref(),
            Some("v0.1.1"),
            "and the answer in the file is the one window that asked"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — **a claim left behind by a killed process does not stop the check
    /// forever.**
    ///
    /// The one failure mode a lock file has that a stamp does not, and the
    /// reason the claim carries the millisecond it was taken at.
    ///
    /// MUTATION: return `None` unconditionally from `Claim::take`'s error arm
    /// and this goes red.
    #[test]
    fn an_abandoned_claim_is_recovered_rather_than_waited_on() {
        let root = dir("abandoned");
        let start = 1_756_000_000_000u64;
        std::fs::write(root.join(super::CLAIM_FILE_NAME), start.to_string())
            .expect("a claim left behind");

        let source = Counting::ok("v0.1.1");
        assert_eq!(run(&root, start + 1_000, &source), Outcome::Busy);
        assert_eq!(source.calls(), 0, "a fresh claim is another window's");

        assert_eq!(
            run(&root, start + CLAIM_STALE_MS + 1, &source),
            Outcome::Answered("v0.1.1".to_owned()),
            "a claim older than any request could be is a dead process's"
        );
        assert_eq!(source.calls(), 1);
        assert!(
            !root.join(super::CLAIM_FILE_NAME).exists(),
            "and the claim is let go when the request that took it is done"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — **the row says the number, in both columns.**
    ///
    /// A row that only said "a newer version is out" would send the reader to
    /// the page to find out which one, which is the whole of what this feature
    /// exists to save them. The base sentence is a table entry and is checked by
    /// `i18n`'s own gates; what is checked here is that the *composed* one
    /// carries the version and that the two columns are not one column said
    /// twice.
    ///
    /// MUTATION: drop `{version}` from either arm of `update_row_available_in`
    /// and that column goes red.
    #[test]
    fn the_rows_sentence_names_the_version_in_both_languages() {
        for lang in crate::i18n::Lang::ALL {
            let sentence = crate::i18n::update_row_available_in(lang, "0.1.1");
            assert!(sentence.contains("0.1.1"), "{lang:?}: {sentence}");
            // The copy guide's Chinese rule: a full stop after a Han character
            // is the full-width one.
            if lang == crate::i18n::Lang::Chinese {
                assert!(sentence.contains('。'), "{sentence}");
                assert!(!sentence.contains(". "), "{sentence}");
            }
        }
        assert_ne!(
            crate::i18n::update_row_available_in(crate::i18n::Lang::English, "0.1.1"),
            crate::i18n::update_row_available_in(crate::i18n::Lang::Chinese, "0.1.1"),
        );

        // And with nothing newer to say, the row wears the table's own sentence
        // — which is what every machine reads on the day its build is the
        // newest. `KNOWN` is untouched by every test in this module, so this is
        // the unloaded state and not a leftover.
        for lang in crate::i18n::Lang::ALL {
            assert_eq!(
                super::row_description_in(lang),
                crate::i18n::Text::DescUpdateCheck.in_lang(lang)
            );
        }
    }

    /// PIN — **the request carries nothing about the machine, and the address it
    /// goes to is the one the documents name.**
    ///
    /// `docs/PRIVACY.md` and both READMEs state four facts about this feature —
    /// the host, the path, the agent, and that the agent is all that is sent —
    /// and a document is not a gate. This is: the four constants are the four
    /// sentences, so a build that quietly started sending its version would have
    /// to edit this test to ship.
    ///
    /// MUTATION: put the version in [`USER_AGENT`], or a query string on
    /// [`RELEASES_PATH`], and this goes red naming which.
    #[test]
    fn the_question_carries_nothing_about_the_machine() {
        assert_eq!(super::RELEASES_HOST, "api.github.com");
        assert_eq!(
            super::RELEASES_PATH,
            "/repos/lulu-loopp/folio-terminal/releases"
        );
        assert!(
            !super::RELEASES_PATH.contains('?'),
            "a query string is somewhere to put a fact about the machine"
        );
        assert_eq!(super::USER_AGENT, "Folio");
        assert!(
            !super::USER_AGENT.contains(crate::version::VERSION)
                && !super::USER_AGENT.contains(crate::version::COMMIT),
            "the agent names the product and not this build"
        );

        // And the documents say so, in both languages. The paths are relative to
        // the crate, which is where every other document gate in this tree
        // reaches from.
        const PRIVACY: &str = include_str!("../../../docs/PRIVACY.md");
        const README: &str = include_str!("../../../README.md");
        const README_ZH: &str = include_str!("../../../README.zh-CN.md");
        for (name, text) in [
            ("docs/PRIVACY.md", PRIVACY),
            ("README.md", README),
            ("README.zh-CN.md", README_ZH),
        ] {
            assert!(
                text.contains(super::RELEASES_HOST),
                "{name} names the host this build asks"
            );
            assert!(
                text.contains("update_check"),
                "{name} names the key that switches it off"
            );
        }
        assert!(
            PRIVACY.contains(super::RELEASES_PATH),
            "docs/PRIVACY.md names the whole address, not only its host"
        );
    }

    /// PIN — **the highest version in the list wins, whatever order the list
    /// arrived in, and a list shaped like anything else is a silence.**
    ///
    /// The ordering half is not hypothetical: GitHub sorts these by creation
    /// date, so the first fixture below is exactly what a patch published for an
    /// older line after a newer minor looks like on the wire.
    ///
    /// MUTATION: take `.next()` instead of `.max_by(...)` and the first case
    /// answers `v0.1.4`; put `#[serde(default)]` on `tag_name` and `[{}]` comes
    /// back as a release tagged with the empty string, which draws nothing and
    /// would have been indistinguishable from working; turn the `filter_map`
    /// into a `map` with a `?` and the `nightly` case takes the whole answer
    /// with it.
    #[test]
    fn a_release_list_yields_its_highest_version_or_nothing() {
        assert_eq!(
            newest_tag(r#"[{"tag_name":"v0.1.4"},{"tag_name":"v0.2.0"},{"tag_name":"v0.1.3"}]"#),
            Some("v0.2.0".to_owned()),
            "newest by date is not newest by version"
        );
        assert_eq!(
            newest_tag(r#"[{"tag_name":"v0.1.0-preview","name":"0.1.0 preview"}]"#),
            Some("v0.1.0-preview".to_owned()),
            "the tag comes back verbatim, pre-release suffix and all"
        );
        assert_eq!(
            newest_tag(r#"[{"tag_name":"nightly"},{"tag_name":"v0.1.1"}]"#),
            Some("v0.1.1".to_owned()),
            "one tag that is not a version does not take the answer with it"
        );
        for junk in [
            "",
            "not json",
            "[]",
            "{}",
            r#"{"tag_name":"v0.1.1"}"#,
            r#"[{"tag":"v0.1.1"}]"#,
            r#"[{"tag_name":3}]"#,
            r#"[{"tag_name":"nightly"}]"#,
            // A rate limit answers 403 with a body of its own; the transport
            // refuses it first, and this is the second line.
            r#"{"message":"API rate limit exceeded","documentation_url":"…"}"#,
        ] {
            assert_eq!(newest_tag(junk), None, "{junk:?}");
        }
    }
}
