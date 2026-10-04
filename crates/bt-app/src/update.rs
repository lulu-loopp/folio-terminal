//! **Whether a newer Folio exists** — asked once a day, answered by a mark on
//! the gear, and never acted on by the check itself (`docs/DESIGN.md` §7.52);
//! what a reader may then do about it is the update job's (`update_job`).
//!
//! # What this is, stated as a bound
//!
//! One `GET` of one fixed address, at most once every twenty-four hours across
//! every window and every process on the machine — at launch and again each day
//! while Folio stays open — on a thread of its own, carrying nothing about the
//! machine or the person at it, failing in complete silence, and downloading
//! nothing whatever it learns. That sentence is the whole feature, and every
//! part of it is load-bearing:
//!
//! * **One address.** [`RELEASES_HOST`] and [`RELEASES_PATH`] are constants; no
//!   part of the answer can redirect the next question, because there is no next
//!   question. A process started with `--update-feed <file-URL>` reads a local
//!   folder's list instead, and then never asks the address ([`Feed`], U-30b).
//! * **Once a day, across windows.** The stamp in `update-check.json` answers
//!   *is it time yet*; a claim file beside it answers *is another window already
//!   asking*. Two windows opened together make one request, and the second one
//!   does not queue behind the first — it simply does not ask. See
//!   [`OfferState::run_on_clock`].
//! * **Its own thread.** Nothing on the path from `main` to the first frame
//!   waits for this. The thread is started after the window exists and its
//!   answer arrives as an ordinary wake, exactly the way the PSReadLine probe's
//!   does.
//! * **Nothing about the machine.** The request carries a `User-Agent` of
//!   [`USER_AGENT`] and not one byte more — no version, no identifier, no
//!   cookie, no query string. See [`USER_AGENT`] for why it is not empty.
//! * **Silent.** Every failure — no network, DNS, a proxy, a rate limit, a
//!   response that is not JSON, a tag that is not a version — is the same
//!   outcome: the attempt is recorded, the last answer's stamp stays where it
//!   was, and nothing is said. A terminal that reported its update check's
//!   problems would be a terminal that talked about itself.
//! * **Downloads nothing.** The check has no installer, no replacement, no
//!   restart: the most it can do is put a dot on a gear and a sentence in a
//!   dialog. What it learned is the update job's evidence (`update_job`): on a
//!   build whose platform's gate is open (Windows since 0.4.6, U-31) the job
//!   may raise a card, and only a press on that card downloads anything. The
//!   check runs the same with the gate open or shut.
//!
//! # The ruling about in-place self-update, and what it cost here
//!
//! `docs/plans/port/macos-plan-2026-09-12.md` §M4 defers in-place self-update
//! out of 0.4 and says it "becomes *open the release page*" on a Mac. **That
//! cost this module nothing, because the swap it replaces was never built**:
//! the bullet above is the feature as it has always shipped, on Windows as much
//! as anywhere, and [`RELEASES_PAGE`] is the one press the settings row has
//! offered since §7.51 landed. A reader who comes here looking for the Windows
//! installer this ticket was going to gate should stop looking — there is no
//! `cfg` to write, because there is no second behaviour to choose between.
//! Since 0.4.6 the swap exists, and it is not here: `update_job` and the
//! Prepare, apply and rollback modules it drives, behind a gate per platform.
//!
//! What M4-10 actually changed is one layer down: `bt-platform`'s
//! `http::https_get` now has a macOS arm (`NSURLSession`, DESIGN §13.27), so
//! [`GitHubReleases::latest_tag`] stopped being two arms and this file stopped
//! naming a platform.
//!
//! # Why a failure counts as the day's attempt
//!
//! It is the whole of the no-retry-storm rule. A laptop on a train would
//! otherwise fail, find itself still due, and fail again — once per window, per
//! launch, per turn of a window left open, forever — and the machine that
//! suffers most is the one least able to answer. So a refusal writes
//! `attempted_at_ms`, and the check is due only when both that and the last
//! answer's `checked_at_ms` are a day old ([`owed`]): a week offline is seven
//! attempts across every window and process, not seven thousand. The answer's
//! stamp itself does not move on a refusal (T-UPDATE-DAILY), so About's "Last
//! checked" names the last answer rather than hiding a failure behind a fresh
//! time. A held cross-process claim is looked at again after its own stale
//! bound, which lets this process read the other one's answer without a second
//! request; and should the attempt not reach the disk, this process still
//! remembers it in memory and waits the same day.
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
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering as AtomicOrdering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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

/// **Whether this build may update itself** (0.4.6 ticket U-8).
///
/// A build fact, decided once by `build.rs` from `FOLIO_UPDATER` and carried
/// here as the cfg `folio_updater`: true only for a build whose invocation said
/// `FOLIO_UPDATER=on`, which the release pipeline does and nothing else does
/// (`build-release.yml` on a `v*` tag or its `updater` dispatch input, and the
/// macOS release build in `docs/RELEASING.md`). A `cargo build`, a CI build and
/// every candidate answer false. See `src/update_eligibility.rs` for the one
/// value the variable takes.
///
/// **Eligibility, not permission.** The owner's ruling of 2026-09-25 makes a
/// copy's self-update need a signed running build *and* this flag; the signature
/// is asked elsewhere, when there is an update to ask it about. And it is a
/// capability the bytes carry rather than a claim about where they came from:
/// a copied release binary is as eligible as the one that was downloaded.
///
/// Read by the `diagnostics.log` run header, which is where `smoke.ps1`
/// checks it, and by the update job's eligibility (U-18, `update_job`).
#[must_use]
pub const fn eligible() -> bool {
    cfg!(folio_updater)
}

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

/// The bound on one phase of the exchange — each of WinHTTP's four phase
/// timeouts, and `NSURLSession`'s `timeoutIntervalForRequest`.
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
    newest(
        serde_json::from_str::<Vec<Release>>(body)
            .ok()?
            .into_iter()
            .map(|release| release.tag_name),
    )
}

/// **The greatest of `tags` by precedence**, skipping any that is not a
/// version — [`newest_tag`]'s rule, shared with the release feed's list
/// ([`Feed`]), so the two sources cannot come to disagree about which tag is
/// the newest.
fn newest(tags: impl Iterator<Item = String>) -> Option<String> {
    tags.filter_map(|tag| Version::parse(&tag).map(|it| (it, tag)))
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

/// **The offer decision**: the tag this reader is offered, or `None` (0.4.6 ticket
/// U-6; `docs/plans/design/self-update-2026-09-16.md` §B).
///
/// Two conditions, both necessary. Automatic check is not one of them: it is
/// the daily schedule, and Off only stops the scheduled start (T-UPDATE-ON-ABOUT
/// round 2, R4).
///
/// * **The tag is newer than the running build** — [`newer_than`].
/// * **The tag is above the skipped one by precedence**, not merely different
///   from it. A tag at or below `skipped_tag` is never offered again, and a tag
///   above it is. Equality would re-offer a withdrawn release's predecessor: skip
///   `0.5.1`, the release is pulled, the list's newest becomes `0.5.0`, and
///   `0.5.0` is still newer than a running `0.4.x`.
///
/// A `skipped_tag` that does not parse as a version skips nothing, for
/// [`newest_tag`]'s reason: a tag that is not a version takes no part in ordering.
#[must_use]
pub fn should_offer<'state>(state: &'state UpdateCheckV1, running: &str) -> Option<&'state str> {
    let tag = newer_than(state.latest_tag.as_deref(), running)?;
    let skipped = state.skipped_tag.as_deref().and_then(Version::parse);
    match (Version::parse(tag), skipped) {
        (Some(offered), Some(skipped)) if offered <= skipped => None,
        _ => Some(tag),
    }
}

/// Whether the gear wears its mark.
///
/// Two conditions and both are necessary: there is an offer ([`should_offer`]),
/// **and** this reader has not been shown this one. The second is what stops a
/// dot that has been answered from coming back on the next launch, and it is
/// keyed by the tag rather than by a flag, so the next release lights it again
/// without anything having to clear anything.
#[must_use]
pub fn mark_is_lit(state: &UpdateCheckV1, running: &str) -> bool {
    should_offer(state, running).is_some_and(|tag| Some(tag) != state.seen_tag.as_deref())
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

/// Whether the automatic check is owed by `state`: [`due`] of the last answer
/// **and** of the last unanswered attempt, so a refusal counts as the day's
/// attempt without moving the answer's stamp (T-UPDATE-DAILY).
#[must_use]
pub fn owed(state: &UpdateCheckV1, now_ms: u64) -> bool {
    due(state.checked_at_ms, now_ms) && due(state.attempted_at_ms, now_ms)
}

/// How long until `stamp`'s day has passed by [`due`]; zero once it has.
fn wait_ms(stamp: u64, now_ms: u64) -> u64 {
    if due(stamp, now_ms) {
        0
    } else {
        CHECK_INTERVAL_MS - (now_ms - stamp)
    }
}

/// What the last question did, as the About page tells it, read from a
/// document: 0 never asked, 1 answered, 2 asked without an answer.
const fn last_answer_of(state: &UpdateCheckV1) -> u8 {
    if state.attempted_at_ms != 0 {
        2
    } else if state.checked_at_ms == 0 {
        0
    } else if state.latest_tag.is_some() {
        1
    } else {
        2
    }
}

// ── the check itself ────────────────────────────────────────────────────────

/// Where a tag comes from.
///
/// A trait with one method so the whole of [`OfferState::run_on_clock`] can be
/// tested without a network: the tests hand it a source that counts its calls
/// and answers from a string, and the product hands it [`GitHubReleases`].
/// Nothing else in this module knows that HTTP exists.
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
    /// **One arm, on every platform** (M4-10).
    ///
    /// This used to be two, gated on `cfg(windows)`, because WinHTTP was the
    /// only stack `bt-platform` had and the other arm's whole body was
    /// `Err("this build has no HTTP stack")`. It is one again now that
    /// `NSURLSession` answers the same door with the same signature and the
    /// same refusals, and the platform question has gone back where it belongs
    /// — which is why this file is no longer on
    /// `only_the_named_files_decide_what_platform_this_is`' list.
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
}

// ── the release feed: a local folder in place of the releases page ──────────

/// The file a release feed's folder holds its list in.
pub const FEED_LIST: &str = "releases.json";

/// **A release feed** (0.4.6 ticket U-30b): `--update-feed <file-URL>`, a
/// folder standing in for the releases page, so the first self-update can be
/// rehearsed on a clean machine before it ships (`docs/plans/release/
/// clean-vm.md` §4.4).
///
/// The folder holds [`FEED_LIST`] — the GitHub releases list's shape, each
/// release with at least `tag_name`, `name`, `draft`, `prerelease` and
/// `assets[]{name, browser_download_url, size}` — and the assets beside it,
/// each `browser_download_url` a `file:` URL. The check reads the list here
/// instead of asking [`RELEASES_HOST`], and a press copies the offer's two
/// files instead of downloading them (`update_job::FeedCopy`); **everything
/// after the fetch is the release page's road**: the checksum document, the
/// archive reader and the signer the running build requires. A feed can
/// therefore only deliver a build signed by the same signer, which is what
/// makes a visible flag safe to have.
///
/// **One process's input, never a fact.** Given on the command line and held
/// in [`FEED`] for this process only: no environment variable, no settings
/// key, nothing written. A start without the flag asks github.com again. The
/// processes an update starts (the trial, the applier, recovery) never need
/// it: the download happened at Prepare.
///
/// A drafted release is left out of the list, as the releases page leaves it
/// out of the unauthenticated list the check asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feed {
    /// The URL as the command line gave it — what the diagnostics line says.
    url: String,
    /// The folder it names; `None` when it names no local folder, which makes
    /// every read of the feed fail rather than fall back to the network.
    folder: Option<PathBuf>,
}

/// One release as the feed lists it. Every field is required: a list shaped
/// otherwise is a failed check, as a malformed answer from the page is.
#[derive(serde::Deserialize)]
struct FeedRelease {
    tag_name: String,
    /// Required of the feed's shape (the releases list's); the check does not read it, hence
    /// the leading underscore.
    #[serde(rename = "name")]
    _name: String,
    draft: bool,
    /// Required of the feed's shape; the check offers pre-releases as the page's list does, so
    /// it does not read it either.
    #[serde(rename = "prerelease")]
    _prerelease: bool,
    assets: Vec<FeedAsset>,
}

/// One file of a release, as the feed lists it.
#[derive(serde::Deserialize)]
struct FeedAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

impl Feed {
    /// The feed `url` names: a `file:` URL to a folder, with or without its
    /// trailing slash.
    #[must_use]
    pub fn at(url: &str) -> Self {
        Self {
            url: url.to_owned(),
            folder: local_path(url.strip_suffix('/').unwrap_or(url)),
        }
    }

    /// The URL as it was given.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// **The list, read once per question** through `file_reads` on
    /// [`bt_platform::file_reads::Lane::Update`] — a file read, not a network
    /// one — without its drafts.
    fn releases(&self) -> Result<Vec<FeedRelease>, String> {
        let folder = self
            .folder
            .as_deref()
            .ok_or_else(|| format!("{} names no local folder", self.url))?;
        let list = folder.join(FEED_LIST);
        let text =
            bt_platform::file_reads::read_to_string(bt_platform::file_reads::Lane::Update, &list)
                .map_err(|error| format!("{}: {error}", list.display()))?;
        let releases: Vec<FeedRelease> =
            serde_json::from_str(&text).map_err(|error| format!("{}: {error}", list.display()))?;
        Ok(releases
            .into_iter()
            .filter(|release| !release.draft)
            .collect())
    }

    /// **Where the feed keeps `name` of release `tag`**, and the length the
    /// list gives it.
    ///
    /// # Errors
    /// The list cannot be read, names no such release or file, or gives the
    /// file an address that is not a local `file:` URL.
    pub fn asset(&self, tag: &str, name: &str) -> Result<(PathBuf, u64), String> {
        let release = self
            .releases()?
            .into_iter()
            .find(|release| release.tag_name == tag)
            .ok_or_else(|| format!("the feed lists no release {tag}"))?;
        let asset = release
            .assets
            .into_iter()
            .find(|asset| asset.name == name)
            .ok_or_else(|| format!("the feed's {tag} has no {name}"))?;
        let path = local_path(&asset.browser_download_url).ok_or_else(|| {
            format!(
                "the feed's {name} is at {}, which is not a local file",
                asset.browser_download_url
            )
        })?;
        Ok((path, asset.size))
    }
}

/// The local path a `file:` URL names, without a query or a fragment — the
/// web pane's one parser (`webnav::LocalFileUrl`).
fn local_path(url: &str) -> Option<PathBuf> {
    crate::webnav::LocalFileUrl::parse(url)
        .filter(|parsed| parsed.tail().is_empty())
        .map(|parsed| parsed.path().to_path_buf())
}

impl Releases for Feed {
    /// The newest tag the list names, by [`newest_tag`]'s rule.
    fn latest_tag(&self) -> Result<String, String> {
        newest(self.releases()?.into_iter().map(|release| release.tag_name))
            .ok_or_else(|| "the feed lists no version".to_owned())
    }
}

/// **Where the check asks**: the feed when this process was given one, the
/// releases page otherwise. Never both — a feed that cannot be read is a
/// failed check, not a reason to ask the network.
#[must_use]
pub fn check_source<'a>(feed: Option<&'a Feed>, page: &'a dyn Releases) -> &'a dyn Releases {
    match feed {
        Some(feed) => feed,
        None => page,
    }
}

/// What one call to [`OfferState::run_on_clock`] did.
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
    /// The question was asked and did not come back. It is the day's attempt
    /// (`attempted_at_ms`); the answer's stamp stays where it was.
    Refused,
}

/// What the automatic check's one application clock owes at `now_ms`.
///
/// The update job is deliberately absent: an offer, download or failure card
/// may be standing while the check follows its own cadence. `After` is a wall
/// duration translated to the event loop's `Instant` only at the scheduling
/// door, so sleep and wall-clock corrections are judged again by [`due`] on
/// the turn that follows them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Schedule {
    Off,
    /// An update's trial holds the check's writes (`update_trial`, F-7);
    /// [`release_trial`] starts the check when the trial is committed.
    Held,
    InFlight,
    Start,
    After(u64),
}

/// The check facts the About page renders. The persisted schema stays
/// `UpdateCheckV1`; the last-answer bit and in-flight bit last only for this
/// process because neither belongs in the compatibility file.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CheckView {
    pub checking: bool,
    pub checked_at_ms: u64,
    pub answered: Option<bool>,
}

/// **The update check's state, under one owner** (0.4.6 ticket U-6; fact 11 of
/// `docs/ARCHITECTURE.md` §4.2, structural debt D-53).
///
/// The only reader and writer of `update-check.json` and of this process's copy
/// of it, and the holder of the claim file beside it. The file's four fields —
/// `checked_at_ms`, `latest_tag`, `seen_tag`, `skipped_tag` — change only through
/// [`Self::transact`], which holds [`Self::file`] across the **whole**
/// read-modify-write: read the file, change it, write it, publish the result to
/// [`Self::known`].
///
/// # Why one lock, and not re-reading before writing
///
/// The check used to take its document before the request and, after R4-14,
/// re-read it just before writing the answer back. That narrowed the window and
/// did not close it: a Skip or an acknowledgement written between that re-read
/// and its write was still replaced by the older document. Atomic replacement
/// makes each write whole; it does not make read-modify-write atomic. The writers
/// are two threads of this process — the check's `bt-update-check` worker and the
/// window thread (the mark answered on About, and Skip) — so the lock
/// is a mutex in this process, and every writer takes it.
///
/// # Why a lock, and not the storage worker
///
/// This file never went through `persist`'s stores or a storage worker: the
/// check writes it from its own thread, and the window thread writes it on the
/// frame the reader acknowledges the mark, as `settings.json` is written on a
/// press. Routing it through a worker would add a channel and a wake for a file of
/// four fields; a lock over one read and one atomic write of a few hundred bytes
/// serialises the same writers with nothing new. The lock is **never** held
/// across the network request: what the request did — an answer with its stamp,
/// or a refusal's attempt — lands after it.
///
/// # What the lock does not cover
///
/// Other processes. Two processes on one data directory are kept from asking
/// together by the claim file ([`CLAIM_FILE_NAME`]), which is a don't-wait
/// exclusion and not a lock anybody waits on; a second process's write landing
/// inside the first one's transaction is not excluded by it.
pub struct OfferState {
    /// `update-check.json`.
    path: PathBuf,
    /// `update-check.lock`, the cross-process claim to ask.
    claim: PathBuf,
    /// **The one lock**: held across every read-modify-write of the file.
    file: Mutex<()>,
    /// What this process last read or wrote. Replaced only at the end of a
    /// transaction (under [`Self::file`]) or by [`Self::load`]; the frame reads
    /// it and never waits on the disk.
    known: Mutex<UpdateCheckV1>,
    /// The reader's switch (`SettingsV1::update_check`), as last told.
    enabled: AtomicBool,
    /// **A change an update's trial held back from the file** (`update_trial`,
    /// F-7): [`Self::known`] holds it, and a commit writes it
    /// ([`Self::release_trial`]).
    owed: AtomicBool,
    /// **This process's check reads a local release feed** (`--update-feed`,
    /// U-30b): its stamps and answers are marked
    /// [`UpdateCheckV1::local_stamp`] and [`UpdateCheckV1::local_tag`]; a
    /// process without the flag forgets such an answer (U-42e,
    /// [`forget_a_local_answer`]).
    local: bool,
    /// **This launch's check has settled** (U-18): it answered, was refused,
    /// found the stamp too fresh or the claim held, or will not run at all. The
    /// update job decides nothing before this ([`Self::job_evidence`]).
    settled: AtomicBool,
    /// Whether the shared check worker is currently asking. The About page
    /// reads this to disable its `Check` button; it is not a second request
    /// owner, only the visible state of this one.
    checking: AtomicBool,
    /// What the last question did: 0 before any known question, 1 when it got
    /// an answer, 2 when it got none. Kept in process because the persisted
    /// document deliberately records the time and last tag, not an error.
    last_answer: AtomicU8,
    /// The last automatic or manual request which got no answer, or found
    /// another process holding the claim. The refusal is also written to the
    /// document (`attempted_at_ms`); this copy is what bounds the retry when
    /// that write did not land, or for a feed's check, which never writes it.
    last_attempt_ms: AtomicU64,
    /// The in-process retry interval after [`Self::last_attempt_ms`]. A refused
    /// source keeps the ordinary daily cadence; a held claim is looked at again
    /// after the claim's own stale bound so this process can ingest the other
    /// process's answer. Zero means the persisted stamps own the next look.
    retry_after_ms: AtomicU64,
    /// A test's pause between a transaction's read and its write — the one place
    /// a racing writer can land. Fires once.
    #[cfg(test)]
    between: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl OfferState {
    /// The owner of `dir`'s state file, with the file read into memory — for
    /// a process that asks the releases page ([`Self::load_for`]). The tests'
    /// spelling; the product names the feed ([`load`]).
    #[cfg(test)]
    #[must_use]
    pub fn load(dir: &Path, enabled: bool) -> Self {
        Self::load_for(dir, enabled, false)
    }

    /// The owner of `dir`'s state file, with the file read into memory, for a
    /// process whose check reads a local release feed (`local`) or the
    /// releases page. A process of the page forgets what a feed answered
    /// ([`forget_a_local_answer`]; U-42e) — in memory here, and on the disk at
    /// its first write.
    ///
    /// On the window thread at startup: one small file beside `settings.json`,
    /// read so the first frame draws the right gear.
    #[must_use]
    pub fn load_for(dir: &Path, enabled: bool, local: bool) -> Self {
        let path = dir.join(STATE_FILE_NAME);
        let (mut state, report) =
            bt_persist::read_update_check_keeping(&path, crate::update_trial::keeping());
        crate::update_trial::owe_copy(&report, &path, |path| {
            let _ = bt_persist::read_update_check(path);
        });
        if !local {
            forget_a_local_answer(&mut state);
        }
        let last_answer = last_answer_of(&state);
        Self {
            claim: dir.join(CLAIM_FILE_NAME),
            path,
            file: Mutex::new(()),
            known: Mutex::new(state),
            enabled: AtomicBool::new(enabled),
            owed: AtomicBool::new(false),
            local,
            settled: AtomicBool::new(false),
            checking: AtomicBool::new(false),
            last_answer: AtomicU8::new(last_answer),
            last_attempt_ms: AtomicU64::new(0),
            retry_after_ms: AtomicU64::new(0),
            #[cfg(test)]
            between: Mutex::new(None),
        }
    }

    /// The state as this process last saw it.
    #[must_use]
    pub fn known(&self) -> UpdateCheckV1 {
        self.known
            .lock()
            .expect("the update state is not held across a panic")
            .clone()
    }

    /// Whether the switch is on.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled.load(AtomicOrdering::Acquire)
    }

    /// The reader turned the automatic daily check on or off. It changes the
    /// next scheduled start only; a manual check and an answer already on the
    /// wire remain the same check through the same worker door.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, AtomicOrdering::Release);
    }

    /// **This launch's check has said all it will** (U-18): mark it, so the
    /// update job may decide on what this owner holds.
    pub fn settle(&self) {
        self.settled.store(true, AtomicOrdering::Release);
    }

    /// **What the update job decides on** (U-18): the state as this process
    /// holds it — once this launch's check has settled, and `None` before, so
    /// no offer is derived from a cache the check is about to replace. The
    /// Automatic check switch is not part of it: it is the daily schedule only.
    #[must_use]
    pub fn job_evidence(&self) -> Option<UpdateCheckV1> {
        self.settled
            .load(AtomicOrdering::Acquire)
            .then(|| self.known())
    }

    /// The tag this reader is offered now — [`should_offer`] over this owner's
    /// evidence. The schedule switch is deliberately not eligibility.
    #[must_use]
    pub fn offer(&self, running: &str) -> Option<String> {
        should_offer(&self.known(), running).map(str::to_owned)
    }

    /// Whether the gear wears its mark — [`mark_is_lit`] over this owner's
    /// evidence. The schedule switch is deliberately not visibility.
    #[must_use]
    pub fn mark_is_lit(&self, running: &str) -> bool {
        mark_is_lit(&self.known(), running)
    }

    /// The part of the check state the About page can show without reading the
    /// state file or inventing another owner.
    #[must_use]
    pub fn view(&self) -> CheckView {
        CheckView {
            checking: self.checking.load(AtomicOrdering::Acquire),
            checked_at_ms: self.known().checked_at_ms,
            answered: match self.last_answer.load(AtomicOrdering::Acquire) {
                1 => Some(true),
                2 => Some(false),
                _ => None,
            },
        }
    }

    /// The automatic schedule at `now_ms`, from the persisted answer and
    /// attempt stamps and this process's bounded retry state; `trial_holds` is
    /// whether an update's trial holds the check's writes. [`due`] remains the
    /// one owner of the daily decision ([`owed`], [`wait_ms`]).
    fn schedule(&self, now_ms: u64, trial_holds: bool) -> Schedule {
        if !self.enabled() {
            return Schedule::Off;
        }
        if trial_holds {
            return Schedule::Held;
        }
        if self.checking.load(AtomicOrdering::Acquire) {
            return Schedule::InFlight;
        }
        let (checked_at_ms, attempted_at_ms) = {
            let known = self
                .known
                .lock()
                .expect("the update state is not held across a panic");
            (known.checked_at_ms, known.attempted_at_ms)
        };
        let persisted = wait_ms(checked_at_ms, now_ms).max(wait_ms(attempted_at_ms, now_ms));
        let in_memory = self.retry_wait_ms(now_ms);
        match persisted.max(in_memory) {
            0 => Schedule::Start,
            wait => Schedule::After(wait),
        }
    }

    /// How long this process's own retry bound still holds the automatic
    /// check back; zero when it does not. A clock set back past the attempt
    /// releases it, as [`due`] releases a stamp in the future.
    fn retry_wait_ms(&self, now_ms: u64) -> u64 {
        let retry_after_ms = self.retry_after_ms.load(AtomicOrdering::Acquire);
        let attempted_at_ms = self.last_attempt_ms.load(AtomicOrdering::Acquire);
        if retry_after_ms == 0
            || now_ms < attempted_at_ms
            || now_ms - attempted_at_ms >= retry_after_ms
        {
            0
        } else {
            retry_after_ms - (now_ms - attempted_at_ms)
        }
    }

    fn retry_after(&self, now_ms: u64, interval_ms: u64) {
        self.last_attempt_ms.store(now_ms, AtomicOrdering::Release);
        self.retry_after_ms
            .store(interval_ms, AtomicOrdering::Release);
    }

    fn clear_retry(&self) {
        self.retry_after_ms.store(0, AtomicOrdering::Release);
    }

    /// **The one read-modify-write.** `change` answers whether it changed
    /// anything; an unchanged document is not written.
    ///
    /// The lock is held from before the read until after the write and the
    /// publication to [`Self::known`], so no other writer in this process can
    /// land between them. A failed write leaves [`Self::known`] as it was: what
    /// the process believes is what is on the disk.
    fn transact(
        &self,
        change: impl FnOnce(&mut UpdateCheckV1) -> bool,
    ) -> Result<(), bt_persist::WriteError> {
        let _file = self
            .file
            .lock()
            .expect("the update state file is not held across a panic");
        // **An update's trial reads and changes what it holds, and writes
        // nothing** (`update_trial`, F-7): the file is O's until the trial is
        // committed, and this process holds the data directory's claim, so no
        // other process writes it meanwhile.
        let deferred = crate::update_trial::writes_are_deferred();
        let mut state = if deferred {
            self.known()
        } else {
            bt_persist::read_update_check(&self.path).0
        };
        // A feed's answer never reaches an ordinary start's file (U-42e).
        let forgot = !self.local && forget_a_local_answer(&mut state);
        let changed = change(&mut state) || forgot;
        #[cfg(test)]
        {
            let pause = self
                .between
                .lock()
                .expect("the test pause is not held across a panic")
                .take();
            if let Some(pause) = pause {
                pause();
            }
        }
        if changed {
            if crate::update_trial::defer(crate::update_trial::Writer::UpdateCheck) {
                self.owed.store(true, AtomicOrdering::Release);
            } else {
                bt_persist::write_update_check_atomic(&self.path, &state)?;
            }
        }
        *self
            .known
            .lock()
            .expect("the update state is not held across a panic") = state;
        Ok(())
    }

    /// **An update's trial was committed: what it held back reaches the file**
    /// — the state as this process holds it, once, if a change was held back
    /// (`update_trial`, F-7).
    ///
    /// # Errors
    /// The write's refusal; the change stays owed.
    pub fn release_trial(&self) -> Result<(), bt_persist::WriteError> {
        let _file = self
            .file
            .lock()
            .expect("the update state file is not held across a panic");
        if self.owed.load(AtomicOrdering::Acquire) {
            bt_persist::write_update_check_atomic(&self.path, &self.known())?;
            self.owed.store(false, AtomicOrdering::Release);
        }
        Ok(())
    }

    /// **The whole check, on the calling thread, against an injected world.**
    ///
    /// The order of the first three steps is the two-window rule, and it is the
    /// order rather than the steps that makes it true:
    ///
    /// 1. **Take the claim first.** Not "decide, then claim" — two windows that both
    ///    read a stale stamp before either wrote one would both decide to ask.
    ///    Everything that reads or writes the stamp happens inside the claim.
    /// 2. **Then read the stamps**, and let go if it is not time yet ([`owed`]).
    /// 3. **Then write what the request did**: an answer with its stamp, or a
    ///    refusal as the day's attempt (`attempted_at_ms`), which leaves the
    ///    answer's stamp in place. The claim is held across the request, so a
    ///    window that starts meanwhile finds it held and does not ask.
    ///
    /// Step 2 is one transaction and step 3 a second, after the request. The
    /// lock is not held across the request, so the window thread is never made
    /// to wait on somebody else's network.
    ///
    /// A window that finds the claim held does **not** wait: it does nothing at all
    /// this launch, and its gear draws whatever the file said when it opened. The
    /// alternative — blocking a thread until the other window's request finishes,
    /// then reading the answer — would buy one dot one launch earlier at the price
    /// of a thread that can be made to wait on somebody else's network.
    ///
    /// Every outcome settles the check for the update job ([`Self::settle`]):
    /// whatever it learned is in [`Self::known`] by then, and this launch asks
    /// nothing more.
    #[cfg(test)]
    pub(crate) fn run(&self, now_ms: u64, source: &dyn Releases) -> Outcome {
        self.run_on_clock(now_ms, source, || now_ms)
    }

    fn run_on_clock(
        &self,
        now_ms: u64,
        source: &dyn Releases,
        completed_at: impl FnOnce() -> u64,
    ) -> Outcome {
        self.checking.store(true, AtomicOrdering::Release);
        let outcome = self.ask(now_ms, source, false, completed_at);
        self.finish(now_ms, &outcome);
        outcome
    }

    /// The About page's `Check`: the same question, claim, source and owner as
    /// [`Self::run`], with the daily timestamp gate deliberately bypassed.
    #[cfg(test)]
    pub(crate) fn run_now(&self, now_ms: u64, source: &dyn Releases) -> Outcome {
        self.run_now_on_clock(now_ms, source, || now_ms)
    }

    fn run_now_on_clock(
        &self,
        now_ms: u64,
        source: &dyn Releases,
        completed_at: impl FnOnce() -> u64,
    ) -> Outcome {
        self.checking.store(true, AtomicOrdering::Release);
        let outcome = self.ask(now_ms, source, true, completed_at);
        self.finish(now_ms, &outcome);
        outcome
    }

    fn finish(&self, now_ms: u64, outcome: &Outcome) {
        match outcome {
            Outcome::Answered(_) => {
                self.last_answer.store(1, AtomicOrdering::Release);
                self.clear_retry();
            }
            Outcome::Refused => {
                self.last_answer.store(2, AtomicOrdering::Release);
                self.retry_after(now_ms, CHECK_INTERVAL_MS);
            }
            Outcome::Busy => self.retry_after(now_ms, CLAIM_STALE_MS),
            Outcome::TooSoon => {
                // The document is fresher than this process: another process
                // answered, or was refused, since this one last looked. Its
                // facts are now [`Self::known`], and About tells them.
                self.last_answer
                    .store(last_answer_of(&self.known()), AtomicOrdering::Release);
                self.clear_retry();
            }
        }
        self.checking.store(false, AtomicOrdering::Release);
        self.settle();
    }

    /// [`Self::run_on_clock`]'s question, before it settles.
    fn ask(
        &self,
        now_ms: u64,
        source: &dyn Releases,
        now: bool,
        completed_at: impl FnOnce() -> u64,
    ) -> Outcome {
        let Some(_claim) = Claim::take(&self.claim, now_ms) else {
            return Outcome::Busy;
        };

        let mut too_soon = false;
        let local = self.local;
        let _ = self.transact(|state| {
            too_soon = !now && !owed(state, now_ms);
            false
        });
        if too_soon {
            return Outcome::TooSoon;
        }

        let answer = source.latest_tag();
        let completed_at_ms = completed_at();
        match answer {
            Ok(tag) => {
                // Only the fields this thread fetched are written; everything
                // else is whatever the file says under the lock — an
                // acknowledgement or a Skip made while the request was on the
                // wire included.
                let persisted = self.transact(|state| {
                    state.checked_at_ms = completed_at_ms;
                    state.attempted_at_ms = 0;
                    // Whose stamp this is: a start without the feed asks again
                    // at once over a feed's (U-42e).
                    state.local_stamp = local;
                    state.latest_tag = Some(tag.clone());
                    state.local_tag = local;
                    true
                });
                persisted.map_or(Outcome::Refused, |()| Outcome::Answered(tag))
            }
            Err(_) => {
                // **The day's attempt is spent**, across every window and
                // process (the no-retry-storm rule). A feed's refusal is not
                // written: a feed is a rehearsal, and nothing it does may hold
                // back an ordinary start's question to the page (U-42e).
                if !local {
                    let _ = self.transact(|state| {
                        state.attempted_at_ms = completed_at_ms;
                        true
                    });
                }
                Outcome::Refused
            }
        }
    }

    /// Write the tag this reader has now been shown.
    ///
    /// # Errors
    ///
    /// The write's.
    pub fn mark_seen(&self, tag: &str) -> Result<(), bt_persist::WriteError> {
        self.transact(|state| {
            if state.seen_tag.as_deref() == Some(tag) {
                return false;
            }
            state.seen_tag = Some(tag.to_owned());
            true
        })
    }

    /// **Skip this version** (U-6; design note §B): write `skipped_tag` and
    /// `seen_tag`.
    ///
    /// A new writer of the state (fact 11, (c′)). The skipped tag only rises: a
    /// Skip of a tag at or below the one already skipped keeps the higher one,
    /// which is the precedence [`should_offer`] compares by. `seen_tag` becomes
    /// the tag pressed on, so the mark goes out with the card.
    ///
    /// # Errors
    ///
    /// The write's. A Skip that did not reach the disk is not in [`Self::known`]
    /// either, so nothing reports it as kept.
    pub fn skip(&self, tag: &str) -> Result<(), bt_persist::WriteError> {
        self.transact(|state| {
            let higher = match (
                Version::parse(tag),
                state.skipped_tag.as_deref().and_then(Version::parse),
            ) {
                (Some(pressed), Some(held)) => pressed > held,
                _ => true,
            };
            let mut changed = false;
            if higher && state.skipped_tag.as_deref() != Some(tag) {
                state.skipped_tag = Some(tag.to_owned());
                changed = true;
            }
            if state.seen_tag.as_deref() != Some(tag) {
                state.seen_tag = Some(tag.to_owned());
                changed = true;
            }
            changed
        })
    }

    /// Arm a pause that runs once, inside the next transaction, between its
    /// read and its write.
    #[cfg(test)]
    fn pause_between_read_and_write(&self, pause: impl FnOnce() + Send + 'static) {
        *self
            .between
            .lock()
            .expect("the test pause is not held across a panic") = Some(Box::new(pause));
    }
}

/// **The mark on the gear is answered**: the reader has been shown the page the
/// row is on.
///
/// Idempotent and cheap to call every frame the page is up — it reads the
/// owner's memory, and touches the disk only on the one frame that actually
/// changes the answer.
///
/// **The gear does not redraw on that frame, and it does not need to.** The page
/// this is called from is a modal standing over the title bar with the scrim
/// dimming everything behind it, so nobody is looking at the mark while it goes
/// out; and closing the dialog rebuilds the whole of the chrome, which is the
/// next moment the gear is a thing anybody can see.
pub fn answer_mark() {
    let Some(owner) = OWNER.get() else {
        return;
    };
    let running = crate::version::VERSION;
    if !owner.mark_is_lit(running) {
        return;
    }
    if let Some(tag) = owner.offer(running) {
        let _ = owner.mark_seen(&tag);
    }
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
                let held_since = bt_platform::file_reads::read_to_string(
                    bt_platform::file_reads::Lane::Settings,
                    path,
                )
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

/// **This process's one owner of the state** — [`OfferState`], opened once at
/// startup by [`load`] on the data directory.
static OWNER: OnceLock<OfferState> = OnceLock::new();

/// How a finished check asks for a frame — [`install_wake`].
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// **This process's release feed**, when its command line gave one
/// ([`use_feed`], U-30b). Never written anywhere, so it ends with the process.
static FEED: OnceLock<Feed> = OnceLock::new();

/// **`--update-feed <url>`, taken**: the check and the download of this
/// process read the feed at `url` ([`Feed`]). Answers the diagnostics line
/// that says so, `update feed: <url>`.
///
/// Once per process, at start, before [`begin`]; a second call changes
/// nothing.
pub fn use_feed(url: &str) -> String {
    feed_line(FEED.get_or_init(|| Feed::at(url)).url())
}

/// The diagnostics line a start given the feed at `url` writes.
fn feed_line(url: &str) -> String {
    format!("update feed: {url}")
}

/// This process's release feed, if its command line gave one.
#[must_use]
pub fn feed() -> Option<&'static Feed> {
    FEED.get()
}

/// Install the repaint the answer needs, once per process.
///
/// The same shape as `psreadline::install_wake` and for the same reason: the
/// answer can land while the window is sitting on a modal with nothing else to
/// draw, and the mark it lights is on the title bar behind that modal.
pub fn install_wake<F: Fn() + Send + Sync + 'static>(wake: F) {
    let _ = WAKE.set(Box::new(wake));
}

/// Open this process's owner on `dir`, so the first frame draws the right gear.
///
/// On the window thread, at startup, before anything is measured. It is one
/// small file beside `settings.json`, which is read on the same thread a few
/// lines earlier; the thing that must not be on this thread is the *request*,
/// and that is [`begin`]'s.
pub fn load(dir: &Path, enabled: bool) {
    let _ = OWNER.set(OfferState::load_for(dir, enabled, feed().is_some()));
}

/// **Forget what a local release feed wrote** (0.4.7 ticket U-42e; 0.4.6's
/// D-10): its tag, so a start without `--update-feed` neither offers a
/// rehearsal's tag for a day nor sends a press to a releases page that has no
/// such release; and its stamp, so that start's check asks the page at once.
/// Each by its own mark: a feed check that got no answer leaves the page's
/// tag, which is kept (review finding 4). The reader's own marks (`seen_tag`,
/// `skipped_tag`) are theirs and stay. Answers whether anything was forgotten.
fn forget_a_local_answer(state: &mut UpdateCheckV1) -> bool {
    let forgot = state.local_stamp || state.local_tag;
    if state.local_stamp {
        state.checked_at_ms = 0;
        state.local_stamp = false;
    }
    if state.local_tag {
        state.latest_tag = None;
        state.local_tag = false;
    }
    forgot
}

/// Whether the gear wears its mark — the chrome's one question, asked of the
/// owner.
#[must_use]
pub fn gear_mark_is_lit<W: Copy + Eq>(job: &crate::update_job::Job<W>) -> bool {
    job.last_failure().is_some()
        || OWNER
            .get()
            .is_some_and(|owner| owner.mark_is_lit(crate::version::VERSION))
}

/// The tag named by the lit gear. A rollback reported by a later launch has no
/// captured job offer, so it uses the check owner's current offered tag when
/// there is one, then the running version as the last honest fallback.
#[must_use]
pub fn gear_mark_tag<W: Copy + Eq>(job: &crate::update_job::Job<W>) -> Option<String> {
    if let Some((failed_offer, _)) = job.last_failure() {
        return Some(
            failed_offer
                .as_ref()
                .map(|offer| offer.tag().to_owned())
                .or_else(offer)
                .unwrap_or_else(|| crate::version::VERSION.to_owned()),
        );
    }
    gear_mark_is_lit(job).then(offer).flatten()
}

/// **What the update job decides on** — [`OfferState::job_evidence`] of this
/// process's owner; `None` before the owner is opened or its check settles.
#[must_use]
pub fn job_evidence() -> Option<UpdateCheckV1> {
    OWNER.get().and_then(OfferState::job_evidence)
}

/// **The tag this process's check offers**, if any — [`OfferState::offer`] of
/// the owner, for About → Version's state line (U-19, T-UPDATE-ON-ABOUT).
#[must_use]
pub fn offer() -> Option<String> {
    OWNER
        .get()
        .and_then(|owner| owner.offer(crate::version::VERSION))
}

/// The check facts displayed on About, from the one owner.
#[must_use]
pub fn check_view() -> CheckView {
    OWNER
        .get()
        .map_or_else(CheckView::default, OfferState::view)
}

/// **Skip, pressed on the update card** (U-19): [`OfferState::skip`] of this
/// process's owner — the card hands the job's `Effect::RecordSkip` here and
/// writes nothing itself.
///
/// # Errors
///
/// The write's; `Ok` when no owner was opened (a test process), where there is
/// no file to write.
pub fn skip(tag: &str) -> Result<(), bt_persist::WriteError> {
    OWNER.get().map_or(Ok(()), |owner| owner.skip(tag))
}

/// The reader turned the automatic check on or off (Settings > About).
/// On and Off take effect on the application clock's next turn; neither hides
/// an offer nor cancels a job, and the About page's manual Check remains
/// available.
pub fn set_enabled(enabled: bool) {
    if let Some(owner) = OWNER.get() {
        owner.set_enabled(enabled);
    }
}

/// **An update's trial was committed** (`update_trial`): the change it held
/// back reaches `update-check.json`, and the check it did not start starts.
pub fn release_trial() {
    let Some(owner) = OWNER.get() else {
        return;
    };
    if let Err(error) = owner.release_trial() {
        eprintln!("BT_UPDATE_TRIAL update-check.json was not written: {error}");
    }
    begin();
}

/// Start the scheduled check.
///
/// A no-op when the switch is off, and that is the whole of the switch: no
/// thread, no claim, no file. Off is not a quieter check.
///
/// **Every road out settles the check for the update job** (U-18) and wakes
/// the loop to consider an offer: the thread's answer, a check that will not
/// run (the switch, a trial), and a kernel that would not give out a thread.
pub fn begin() {
    spawn_check(false, unix_epoch_ms());
}

/// Ask now from About. Answers whether this press started the shared worker;
/// a second press while it is in flight is refused by the same in-flight bit
/// that disables the button.
#[must_use]
pub fn begin_now() -> bool {
    spawn_check(true, unix_epoch_ms())
}

/// Whether this entry may start the shared check worker. Automatic check gates
/// only the scheduled entry; a reader's Check is independent of it.
#[must_use]
const fn check_entry_allowed(now: bool, automatic: bool) -> bool {
    now || automatic
}

fn spawn_check(now: bool, now_ms: u64) -> bool {
    let Some(owner) = OWNER.get() else {
        return false;
    };
    let settled_without_asking = || {
        owner.settle();
        crate::update_job::evidence_landed();
    };
    if !check_entry_allowed(now, owner.enabled()) {
        settled_without_asking();
        return false;
    }
    // **Not in an update's trial** (`update_trial`, F-7): the check writes its
    // stamp and its claim file into O's folder. It is asked again when the
    // trial is committed ([`release_trial`]).
    if crate::update_trial::defer(crate::update_trial::Writer::UpdateCheck) {
        settled_without_asking();
        return false;
    }
    // **This process's own retry bound holds every automatic entry**, the
    // launch's included: a launch entry the job starts after its pass lands
    // must not ask again a request the application clock just saw refused.
    if !now && owner.retry_wait_ms(now_ms) > 0 {
        settled_without_asking();
        return false;
    }
    if owner
        .checking
        .compare_exchange(false, true, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
        .is_err()
    {
        return false;
    }
    // **In the background band.** A thread starts at normal priority whatever
    // the thread that spawned it was running at, and this one would otherwise
    // stand beside the window's loop for the length of a DNS lookup — see
    // `git::drain`, which is where this crate first wrote that down. A kernel
    // that will not give out a thread is a launch that simply has no update
    // check, which is where every launch before this slice was.
    let spawned = bt_platform::spawn_at_priority(
        "bt-update-check",
        bt_platform::ThreadPriority::BelowNormal,
        move |_ctx| {
            let source = check_source(feed(), &GitHubReleases);
            let outcome = if now {
                owner.run_now_on_clock(now_ms, source, unix_epoch_ms)
            } else {
                owner.run_on_clock(now_ms, source, unix_epoch_ms)
            };
            if let Some(wake) = WAKE.get() {
                wake();
            }
            crate::update_job::evidence_landed();
            let _ = outcome;
        },
    );
    if spawned.is_err() {
        owner.checking.store(false, AtomicOrdering::Release);
        owner.last_answer.store(2, AtomicOrdering::Release);
        owner.retry_after(now_ms, CHECK_INTERVAL_MS);
        settled_without_asking();
        return false;
    }
    true
}

/// Turn the automatic schedule once and return the event-loop deadline for its
/// next look. The caller is the first open window's application-clock turn.
/// The request itself remains on [`spawn_check`]'s existing worker.
///
/// **Not during an update's trial.** The check's writes are held then, so
/// [`spawn_check`] would settle without asking, wake the loop, and be asked
/// again on the turn that wake makes — a loop. [`release_trial`] starts the
/// check once the trial is committed.
pub(crate) fn advance_schedule(now: Instant, now_ms: u64) -> Option<Instant> {
    let owner = OWNER.get()?;
    let trial_holds = crate::update_trial::writes_are_deferred();
    if owner.schedule(now_ms, trial_holds) == Schedule::Start {
        let _ = spawn_check(false, now_ms);
    }
    match owner.schedule(now_ms, trial_holds) {
        Schedule::After(milliseconds) => now.checked_add(Duration::from_millis(milliseconds)),
        Schedule::Off | Schedule::Held | Schedule::InFlight | Schedule::Start => None,
    }
}

/// The next instant About's relative failed-check text changes, while that page
/// is open. The language owner defines the displayed buckets; this translates
/// its wall duration to the event loop's clock.
pub(crate) fn last_checked_deadline(now: Instant, now_ms: u64) -> Option<Instant> {
    let view = check_view();
    (view.answered == Some(false))
        .then(|| crate::i18n::LastChecked::next_change_in_ms(view.checked_at_ms, now_ms))
        .flatten()
        .and_then(|milliseconds| now.checked_add(Duration::from_millis(milliseconds)))
}

/// The wall clock, in milliseconds since the Unix epoch.
///
/// The wall clock and not a monotonic one, because the number has to survive the
/// process that wrote it: "a day since the last check" is a question about two
/// different runs of the program, and a monotonic instant means nothing to the
/// second one. A clock that moves under this is what [`due`]'s backwards case is
/// for.
pub(crate) fn unix_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{
        CHECK_INTERVAL_MS, CLAIM_STALE_MS, OfferState, Outcome, Releases, STATE_FILE_NAME,
        Schedule, Version, check_entry_allowed, due, eligible, mark_is_lit, newer_than, newest_tag,
    };
    use bt_persist::UpdateCheckV1;

    /// RED (U-8) — **a build without the flag is never eligible, and a build
    /// with `FOLIO_UPDATER=on` always is.**
    ///
    /// Stated against the environment this very test binary was compiled in,
    /// read here with `option_env!` independently of `build.rs`: the build script
    /// is the real producer and [`eligible`] the real reader, and this holds the
    /// pair to the input. An ordinary `cargo test` — CI's, and every developer's
    /// — is the first branch, which is the half that matters: nothing but the
    /// release invocation may make a copy that updates itself. The other branch
    /// is what `FOLIO_UPDATER=on cargo test` checks. A build with any other value
    /// never gets this far; `build.rs` refuses it
    /// (`update_eligibility::tests::the_flag_accepts_only_its_exact_value`).
    ///
    /// MUTATION: make `eligible` answer `true`, or have `build.rs` emit the cfg
    /// whatever `decide` says, and the first branch goes red.
    #[test]
    fn a_build_without_the_flag_is_never_eligible() {
        match option_env!("FOLIO_UPDATER") {
            None | Some("") => assert!(
                !eligible(),
                "this test binary was built without FOLIO_UPDATER and says it may update itself"
            ),
            Some(value) => {
                assert_eq!(value, "on", "build.rs refuses every other value");
                assert!(
                    eligible(),
                    "this test binary was built with FOLIO_UPDATER=on and says it may not update itself"
                );
            }
        }
    }
    use std::{
        cell::RefCell,
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicU32, Ordering},
        },
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

        OfferState::load(&root, true)
            .mark_seen("v0.1.1")
            .expect("the acknowledgement is written");
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
        let owner = OfferState::load(&root, true);
        let source = Counting::ok("v0.1.1");

        let start = 1_756_000_000_000u64;
        assert_eq!(
            owner.run(start, &source),
            Outcome::Answered("v0.1.1".to_owned())
        );
        assert_eq!(source.calls(), 1);
        assert_eq!(state_of(&root).checked_at_ms, start);
        assert_eq!(state_of(&root).latest_tag.as_deref(), Some("v0.1.1"));

        // Every launch inside the day — a second window, a restart, a hundred
        // restarts — asks nothing.
        for offset in [1, 1_000, 60_000, CHECK_INTERVAL_MS - 1] {
            assert_eq!(owner.run(start + offset, &source), Outcome::TooSoon);
        }
        assert_eq!(source.calls(), 1, "no launch inside the day asks again");

        // And the day after, exactly one more.
        assert_eq!(
            owner.run(start + CHECK_INTERVAL_MS, &source),
            Outcome::Answered("v0.1.1".to_owned())
        );
        assert_eq!(source.calls(), 2);

        // A stamp from the future is a clock that moved, not a check that
        // happened: it does not lock the check out forever.
        assert!(due(start + CHECK_INTERVAL_MS, start));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-UPDATE-DAILY) — **the interval begins when the request
    /// completes, not when its worker starts.** A slow request must not spend
    /// part of the next interval while it is still in flight.
    ///
    /// MUTATION: write `now_ms` instead of `completed_at_ms` in `ask`'s answer
    /// transaction; the persisted stamp is the worker's start and this goes
    /// red.
    #[test]
    fn a_completed_check_stamps_its_completion_clock() {
        let root = dir("completion-clock");
        let owner = OfferState::load(&root, true);
        let started = 1_756_000_000_000u64;
        let completed = started + 12_345;

        assert_eq!(
            owner.run_on_clock(started, &Counting::ok("v0.4.7"), || completed),
            Outcome::Answered("v0.4.7".to_owned())
        );
        assert_eq!(state_of(&root).checked_at_ms, completed);
        assert_eq!(
            owner.schedule(completed, false),
            Schedule::After(CHECK_INTERVAL_MS)
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-UPDATE-DAILY) — **the automatic schedule is a table over the
    /// persisted age and switch, and never over the update job's state.** An
    /// offer card, download or failure card therefore cannot hold the daily
    /// check off; only the schedule switch and the check's own in-flight bit
    /// can.
    ///
    /// MUTATION: return `Schedule::Off` from the due arm of `schedule`; every
    /// due job-state row goes red.
    #[test]
    fn automatic_schedule_table_is_independent_of_the_update_job() {
        let now = 1_756_000_000_000u64;
        for (case, age, enabled, expected) in [
            ("fresh", CHECK_INTERVAL_MS - 1, true, Schedule::After(1)),
            ("due", CHECK_INTERVAL_MS, true, Schedule::Start),
            ("overdue", 2 * CHECK_INTERVAL_MS, true, Schedule::Start),
            ("off", 2 * CHECK_INTERVAL_MS, false, Schedule::Off),
        ] {
            let root = dir(&format!("schedule-{case}"));
            bt_persist::write_update_check_atomic(
                &root.join(STATE_FILE_NAME),
                &UpdateCheckV1 {
                    checked_at_ms: now - age,
                    latest_tag: Some("v0.4.6".to_owned()),
                    ..UpdateCheckV1::default()
                },
            )
            .expect("a completed check");
            let owner = OfferState::load(&root, enabled);
            for job_state in ["idle", "offer open", "downloading", "failure open"] {
                assert_eq!(
                    owner.schedule(now, false),
                    expected,
                    "{case}, job={job_state}"
                );
            }
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// RED (T-UPDATE-DAILY) — **another process's completed answer defers this
    /// one and is adopted without a second request.** The local schedule can be
    /// stale, so the existing claim + transaction re-read remains authoritative.
    ///
    /// MUTATION: decide `too_soon` from `self.known()` instead of the document
    /// read inside `transact`; the stale process makes the second source call.
    /// Or leave `last_answer` alone on `TooSoon`; About keeps saying it never
    /// asked.
    #[test]
    fn another_processs_fresh_completed_check_is_adopted_without_a_request() {
        let root = dir("schedule-other-process");
        let first = OfferState::load(&root, true);
        let now = 1_756_000_000_000u64;
        assert_eq!(first.schedule(now, false), Schedule::Start);

        let second = OfferState::load(&root, true);
        assert_eq!(
            second.run(now, &Counting::ok("v0.4.8")),
            Outcome::Answered("v0.4.8".to_owned())
        );

        let source = Counting::ok("v9.9.9");
        assert_eq!(first.run(now, &source), Outcome::TooSoon);
        assert_eq!(source.calls(), 0, "the other process already asked");
        assert_eq!(first.known().latest_tag.as_deref(), Some("v0.4.8"));
        assert_eq!(
            first.view().answered,
            Some(true),
            "About tells the other process's answer"
        );
        assert_eq!(
            first.schedule(now, false),
            Schedule::After(CHECK_INTERVAL_MS)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-UPDATE-DAILY) — **a 48-hour wall-clock jump starts exactly one
    /// check, and a backwards correction starts one rather than suppressing the
    /// schedule forever.** A completed answer moves the stamp to the sampled
    /// wall time, so neither jump can storm.
    ///
    /// MUTATION: make `due` use `saturating_sub`, or leave the successful stamp
    /// unchanged; the backwards row or the exactly-once assertions go red.
    #[test]
    fn sleep_and_clock_corrections_start_one_check_without_a_storm() {
        let root = dir("schedule-clock-jumps");
        let day = CHECK_INTERVAL_MS;
        let start = 1_756_000_000_000u64;
        let owner = OfferState::load(&root, true);
        assert!(matches!(
            owner.run(start, &Counting::ok("v0.4.7")),
            Outcome::Answered(_)
        ));

        let woke = start + 2 * day;
        assert_eq!(owner.schedule(woke, false), Schedule::Start);
        let source = Counting::ok("v0.4.8");
        assert!(matches!(owner.run(woke, &source), Outcome::Answered(_)));
        assert_eq!(source.calls(), 1);
        assert_eq!(owner.schedule(woke, false), Schedule::After(day));

        let corrected_back = start - day;
        assert_eq!(owner.schedule(corrected_back, false), Schedule::Start);
        assert!(matches!(
            owner.run(corrected_back, &source),
            Outcome::Answered(_)
        ));
        assert_eq!(source.calls(), 2);
        assert_eq!(owner.schedule(corrected_back, false), Schedule::After(day));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-UPDATE-DAILY; the no-retry-storm PIN it replaces) — **a machine
    /// that cannot reach the network makes one attempt a day, across this
    /// process's clock, later launches and other processes alike, and the
    /// failure is not hidden behind a fresh "Last checked".**
    ///
    /// The failure is silent: `latest_tag` and the answer's stamp do not move;
    /// the attempt is written as `attempted_at_ms`, and a process opened on
    /// the file tells About that the last question got no answer.
    ///
    /// MUTATION: drop the refusal's `attempted_at_ms` transaction in `ask`; a
    /// later launch asks again at once and the call count goes to eleven.
    #[test]
    fn a_refused_question_is_the_days_attempt_for_every_process() {
        let root = dir("refused");
        let owner = OfferState::load(&root, true);
        let source = Counting::refusing();

        let start = 1_756_000_000_000u64;
        assert_eq!(owner.run(start, &source), Outcome::Refused);
        assert_eq!(
            owner.schedule(start, false),
            Schedule::After(CHECK_INTERVAL_MS)
        );
        assert_eq!(
            owner.schedule(start + CHECK_INTERVAL_MS - 1, false),
            Schedule::After(1)
        );
        assert_eq!(
            owner.schedule(start + CHECK_INTERVAL_MS, false),
            Schedule::Start
        );
        for offset in 1..10u64 {
            let launch = OfferState::load(&root, true);
            assert_eq!(
                launch.schedule(start + offset * 60_000, false),
                Schedule::After(CHECK_INTERVAL_MS - offset * 60_000)
            );
            assert_eq!(
                launch.run(start + offset * 60_000, &source),
                Outcome::TooSoon
            );
        }
        assert_eq!(source.calls(), 1, "ten launches offline, one attempt");

        let state = state_of(&root);
        assert_eq!(state.checked_at_ms, 0, "no answer, no answer's stamp");
        assert_eq!(state.attempted_at_ms, start, "the day's attempt is spent");
        assert_eq!(state.latest_tag, None, "and nothing was invented to draw");
        assert!(!mark_is_lit(&state, "0.1.0"));
        assert_eq!(
            OfferState::load(&root, true).view().answered,
            Some(false),
            "a later launch still says the last question got no answer"
        );

        let next_day = start + CHECK_INTERVAL_MS;
        let answering = Counting::ok("v0.4.7");
        assert_eq!(
            OfferState::load(&root, true).run(next_day, &answering),
            Outcome::Answered("v0.4.7".to_owned())
        );
        let answered = state_of(&root);
        assert_eq!(answered.checked_at_ms, next_day);
        assert_eq!(answered.attempted_at_ms, 0, "an answer clears the attempt");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-UPDATE-DAILY) — **the schedule waits while an update's trial
    /// holds the check's writes.** The trial's `spawn_check` settles without
    /// asking and wakes the loop; a schedule still saying `Start` would be
    /// asked again on the turn that wake makes, for as long as the trial.
    ///
    /// MUTATION: drop the `trial_holds` arm from `OfferState::schedule`; the
    /// held row answers `Start` and this goes red.
    #[test]
    fn a_trial_holds_the_schedule_until_it_is_committed() {
        let root = dir("schedule-trial");
        let owner = OfferState::load(&root, true);
        let now = 1_756_000_000_000u64;
        assert_eq!(owner.schedule(now, true), Schedule::Held);
        assert_eq!(owner.schedule(now, false), Schedule::Start);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-UPDATE-DAILY) — **an answer that cannot land holds the check back a
    /// day as a source refusal**, held by this process's memory when neither the
    /// answer nor the attempt reaches the disk. Otherwise both stamps remain
    /// due and the application clock asks again on every turn.
    ///
    /// MUTATION: ignore the result of the answer transaction and return
    /// `Outcome::Answered(tag)`; the schedule becomes `Start` and this goes
    /// red.
    #[test]
    fn an_answer_that_cannot_be_persisted_does_not_make_a_retry_loop() {
        let root = dir("answer-write-refused");
        std::fs::create_dir(root.join(STATE_FILE_NAME)).expect("an unwritable state-file name");
        let owner = OfferState::load(&root, true);
        let source = Counting::ok("v0.4.7");
        let start = 1_756_000_000_000u64;

        assert_eq!(owner.run(start, &source), Outcome::Refused);
        assert_eq!(source.calls(), 1);
        assert_eq!(owner.known().checked_at_ms, 0);
        assert_eq!(owner.known().latest_tag, None);
        assert_eq!(
            owner.schedule(start, false),
            Schedule::After(CHECK_INTERVAL_MS)
        );

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
                // The second window, opened while this request is in flight: a
                // second process, so an owner of its own on the same directory.
                let second =
                    OfferState::load(&self.dir, true).run(self.now_ms, &Counting::ok("v9.9.9"));
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
            OfferState::load(&root, true).run(1_756_000_000_000, &source),
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

    /// RED (review row R4-14) — **an acknowledgement made while the request was
    /// in flight is not undone by it.**
    ///
    /// The claim keeps other *processes* out; it does not keep this process's
    /// own window thread out, and `OfferState::mark_seen` runs there — a reader opening
    /// the About page while the daily check is on the wire. `run` took its whole
    /// document before the request and wrote that document back after it, so the
    /// `seen_tag` written in between was replaced by the one from before it
    /// existed: the dot came back, on a version they had just dismissed, and the
    /// only way out was to dismiss it again after every check.
    ///
    /// The source below is the reader, in the one place the race is
    /// deterministic: inside the request.
    ///
    /// Red gate: make the answer's transaction write the document the stamp's
    /// transaction read, instead of reading under the lock, and `seen_tag` below
    /// is `None`.
    #[test]
    fn a_mark_answered_while_the_request_was_running_survives_it() {
        struct AnswersMidFlight<'owner> {
            owner: &'owner OfferState,
        }
        impl Releases for AnswersMidFlight<'_> {
            fn latest_tag(&self) -> Result<String, String> {
                // The reader opens the About page and the mark goes out, on the
                // window thread, while this request is still on the wire.
                self.owner
                    .mark_seen("v0.2.3")
                    .expect("the acknowledgement is written");
                Ok("v0.2.4".to_owned())
            }
        }

        let root = dir("seen-mid-flight");
        let owner = OfferState::load(&root, true);
        let source = AnswersMidFlight { owner: &owner };
        assert_eq!(
            owner.run(1_756_000_000_000, &source),
            Outcome::Answered("v0.2.4".to_owned())
        );

        let state = state_of(&root);
        assert_eq!(
            state.seen_tag.as_deref(),
            Some("v0.2.3"),
            "the acknowledgement the reader made is still in the file"
        );
        assert_eq!(
            state.latest_tag.as_deref(),
            Some("v0.2.4"),
            "and this check's own answer is in it too"
        );
        assert_eq!(
            state.checked_at_ms, 1_756_000_000_000,
            "and so is the stamp this thread advanced"
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

        let owner = OfferState::load(&root, true);
        let source = Counting::ok("v0.1.1");
        assert_eq!(owner.run(start + 1_000, &source), Outcome::Busy);
        assert_eq!(source.calls(), 0, "a fresh claim is another window's");

        assert_eq!(
            owner.run(start + CLAIM_STALE_MS + 1, &source),
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

    /// RED — **one transport, named without a `cfg`** (M4-10).
    ///
    /// Until `bt-platform` grew a macOS arm this file had two
    /// `latest_tag`s — WinHTTP on one side, `Err("this build has no HTTP
    /// stack")` on the other — and it was on
    /// `only_the_named_files_decide_what_platform_this_is`' list because of
    /// them. It is off that list now, and this is the claim that keeps it off
    /// from this side: the module that knows which stack a machine has is
    /// `bt-platform`, and this one asks it the same question everywhere.
    ///
    /// MUTATION: put either arm back and this names it — and the gate in
    /// `main.rs` names the file a second time, from the other direction.
    #[test]
    fn the_check_asks_one_stack_on_every_platform() {
        const SOURCE: &str = include_str!("update.rs");
        // **The module above these pins**, because a pin that searched the
        // whole file would find its own assertion — the needle below is spelled
        // out twice in this function.
        let above = SOURCE
            .split_once("\n#[cfg(test)]\nmod tests {")
            .expect("these pins are in this file")
            .0;
        let code = above
            .lines()
            .map(|line| line.find("//").map_or(line, |at| &line[..at]))
            .collect::<Vec<_>>()
            .join("\n");
        for word in ["windows", "macos", "target_os", "target_family"] {
            for opener in ["cfg(", "cfg!(", "cfg_attr("] {
                for line in code.lines().filter(|line| line.contains(opener)) {
                    assert!(
                        !line.contains(word),
                        "this file decides what platform it is on again: {line}"
                    );
                }
            }
        }
        assert!(
            code.contains("bt_platform::http::https_get(&bt_platform::http::HttpsGet {"),
            "the one call to the platform's HTTP stack has moved or gone"
        );
    }

    /// RED — **the four states the row can be in, and the one action it offers
    /// in every one of them, on every machine this build can be** (M4-10, §M4
    /// acceptance ⑦).
    ///
    /// The plan defers in-place self-update out of 0.4 and rules that it
    /// "becomes *open the release page*" on a Mac. The honest reading of that
    /// ruling, once the code is in front of you, is that **there is nothing to
    /// branch on**: the row has offered exactly one press since §7.51 landed,
    /// that press is [`super::RELEASES_PAGE`], and none of the four states
    /// changes which press it is. So this walks the state machine end to end
    /// and then asserts the verb against each of the three platforms this
    /// build can be — not because the answer could differ, but because "it does
    /// not differ" is the claim the ruling turns into.
    ///
    /// MUTATIONS: make `menu_action` answer `None` for one platform; give the
    /// row a second verb; let a refusal clear the tag that was already known
    /// and state ④ stops naming it.
    #[test]
    fn the_row_offers_the_release_page_whatever_machine_this_is() {
        let running = crate::version::VERSION;
        let newer = "v999.0.0";
        let root = dir("row-states");
        let owner = OfferState::load(&root, true);

        // ① Not asked yet — there is no file, and nothing to say about a
        //    version.
        let fresh = state_of(&root);
        assert_eq!(fresh.latest_tag, None);
        assert_eq!(newer_than(fresh.latest_tag.as_deref(), running), None);
        assert!(!mark_is_lit(&fresh, running));

        // ② Asked, and this build is the newest there is.
        assert_eq!(
            owner.run(CHECK_INTERVAL_MS, &Counting::ok(running)),
            Outcome::Answered(running.to_owned())
        );
        let current = state_of(&root);
        assert_eq!(newer_than(current.latest_tag.as_deref(), running), None);
        assert!(!mark_is_lit(&current, running));

        // ③ A newer release. This is the only state whose sentence names a
        //    version, and it names the verb beside it.
        assert_eq!(
            owner.run(2 * CHECK_INTERVAL_MS, &Counting::ok(newer)),
            Outcome::Answered(newer.to_owned())
        );
        let available = state_of(&root);
        assert_eq!(
            newer_than(available.latest_tag.as_deref(), running),
            Some(newer)
        );
        assert!(mark_is_lit(&available, running));

        // ④ Could not ask. The last answer's stamp and the tag this machine
        //    already knew about stay known — a failed check is a silence, not
        //    an erasure — and the attempt is the day's.
        assert_eq!(
            owner.run(3 * CHECK_INTERVAL_MS, &Counting::refusing()),
            Outcome::Refused
        );
        let refused = state_of(&root);
        assert_eq!(refused.latest_tag.as_deref(), Some(newer));
        assert_eq!(refused.checked_at_ms, 2 * CHECK_INTERVAL_MS);
        assert_eq!(refused.attempted_at_ms, 3 * CHECK_INTERVAL_MS);

        // And the address that press opens is a page for a person, over TLS.
        assert!(super::RELEASES_PAGE.starts_with("https://github.com/"));
        assert!(super::RELEASES_PAGE.ends_with("/releases"));

        let _ = std::fs::remove_dir_all(&root);
    }

    // ── U-6: one owner, Skip, precedence, the switch ───────────────────────

    /// RED (U-6) — **a Skip landing between the check's read and its write is not
    /// lost, and neither is anything the check wrote: all four fields survive.**
    ///
    /// The race R4-14's re-read narrowed and did not close: the check read the file,
    /// and a Skip on the window thread wrote `skipped_tag` and `seen_tag` before
    /// the check wrote its answer back over them. Here the two writers are two
    /// threads driving the one owner. The check's answer transaction is paused
    /// between its read and its write; the Skip is released at that instant and
    /// given 300 ms to land. Under the owner's lock it cannot land until the
    /// check's write is done, so it lands after it and both survive; without the
    /// lock it lands inside the window and the check's write erases it.
    ///
    /// MUTATION: in `OfferState::transact`, release the lock after the read (the
    /// re-read-before-write shape the file had on BASE) and `skipped_tag` and
    /// `seen_tag` below are `None`.
    #[test]
    fn skip_racing_check_and_seen_keeps_all_fields() {
        use std::sync::{Arc, Barrier, mpsc};
        use std::time::Duration;

        struct ArmsThePause {
            owner: Arc<OfferState>,
            barrier: Arc<Barrier>,
            skipped: std::sync::Mutex<Option<mpsc::Receiver<()>>>,
        }
        impl Releases for ArmsThePause {
            fn latest_tag(&self) -> Result<String, String> {
                // The answer's transaction is the next one: pause it between its
                // read and its write, release the Skip there, and wait for it.
                let barrier = Arc::clone(&self.barrier);
                let skipped = self
                    .skipped
                    .lock()
                    .expect("one pause")
                    .take()
                    .expect("armed once");
                self.owner.pause_between_read_and_write(move || {
                    barrier.wait();
                    let _ = skipped.recv_timeout(Duration::from_millis(300));
                });
                Ok("v0.2.4".to_owned())
            }
        }

        let root = dir("skip-race");
        let owner = Arc::new(OfferState::load(&root, true));
        let barrier = Arc::new(Barrier::new(2));
        let (done, skipped) = mpsc::channel();

        let skipper = {
            let owner = Arc::clone(&owner);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                owner.skip("v0.2.3").expect("the Skip is written");
                let _ = done.send(());
            })
        };
        let source = ArmsThePause {
            owner: Arc::clone(&owner),
            barrier,
            skipped: std::sync::Mutex::new(Some(skipped)),
        };
        assert_eq!(
            owner.run(1_756_000_000_000, &source),
            Outcome::Answered("v0.2.4".to_owned())
        );
        skipper.join().expect("the Skip thread finished");

        let state = state_of(&root);
        assert_eq!(state.checked_at_ms, 1_756_000_000_000, "the stamp survives");
        assert_eq!(
            state.latest_tag.as_deref(),
            Some("v0.2.4"),
            "the answer survives"
        );
        assert_eq!(
            state.skipped_tag.as_deref(),
            Some("v0.2.3"),
            "the Skip survives"
        );
        assert_eq!(
            state.seen_tag.as_deref(),
            Some("v0.2.3"),
            "and its seen tag"
        );
        assert_eq!(owner.known(), state, "and the owner's memory is the file");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-6) — **a Skip that did not reach the disk is not reported as kept.**
    ///
    /// The owner's memory is what the offer is decided from; a Skip adopted into
    /// memory and lost on the disk would hide the card for this launch and bring
    /// it back on the next, with nothing said.
    ///
    /// MUTATION: publish to `known` before the write in `OfferState::transact`
    /// (or ignore the write's error) and the memory claims the Skip.
    #[test]
    fn skip_failure_is_not_reported_as_persistent() {
        let root = dir("skip-fails");
        // A directory that does not exist: the read finds nothing and the write
        // cannot be made.
        let owner = OfferState::load(&root.join("gone"), true);
        assert!(
            owner.skip("v0.2.3").is_err(),
            "the write failed and says so"
        );
        assert_eq!(
            owner.known().skipped_tag,
            None,
            "and memory does not claim it"
        );
        assert_eq!(owner.known().seen_tag, None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-6) — **a skipped tag stays hidden across launches inside the day,
    /// with no check to re-learn it.**
    ///
    /// The check runs once a day; a launch inside the day reads the cached answer.
    /// The Skip is in the file, so the next launch's owner opens with it and the
    /// cached tag is neither offered nor marked.
    ///
    /// MUTATION: drop the `skipped_tag` clause from `should_offer` and the second
    /// launch offers `v0.5.0` again.
    #[test]
    fn cached_skipped_tag_stays_hidden_inside_daily_cadence() {
        let root = dir("skip-cached");
        let start = 1_756_000_000_000u64;
        let first = OfferState::load(&root, true);
        assert_eq!(
            first.run(start, &Counting::ok("v0.5.0")),
            Outcome::Answered("v0.5.0".to_owned())
        );
        assert_eq!(first.offer("0.4.6").as_deref(), Some("v0.5.0"));
        first.skip("v0.5.0").expect("the Skip is written");
        assert_eq!(first.offer("0.4.6"), None, "skipped, at once");

        // The next launch, an hour later: no question is asked, and the cached
        // tag stays skipped.
        let second = OfferState::load(&root, true);
        let source = Counting::ok("v0.5.0");
        assert_eq!(second.run(start + 3_600_000, &source), Outcome::TooSoon);
        assert_eq!(source.calls(), 0);
        assert_eq!(second.offer("0.4.6"), None, "not offered inside the day");
        assert!(!second.mark_is_lit("0.4.6"), "and the gear wears no mark");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-6) — **a newer tag is offered after a Skip, and an older one never
    /// is — by precedence, not inequality.**
    ///
    /// The withdrawn-release case is the one inequality gets wrong: skip `v0.5.1`,
    /// the release is pulled, and the list's newest is `v0.5.0` — different from
    /// the skipped tag, still newer than the running `0.4.6`, and below what the
    /// reader said no to.
    ///
    /// MUTATION: compare `offered != skipped` instead of `offered <= skipped` in
    /// `should_offer` and the withdrawn case is offered; let `skip` lower the
    /// skipped tag and the stale-Skip assertion goes red.
    #[test]
    fn newer_tag_is_offered_after_skip() {
        let root = dir("skip-newer");
        let owner = OfferState::load(&root, true);
        let day = CHECK_INTERVAL_MS;

        owner.run(day, &Counting::ok("v0.5.0"));
        owner.skip("v0.5.0").expect("written");
        assert_eq!(owner.offer("0.4.6"), None);

        // The next release is offered, and lights the mark.
        owner.run(2 * day, &Counting::ok("v0.5.1"));
        assert_eq!(owner.offer("0.4.6").as_deref(), Some("v0.5.1"));
        assert!(owner.mark_is_lit("0.4.6"));

        // Skipped too, then withdrawn: the older tag comes back as the newest.
        owner.skip("v0.5.1").expect("written");
        owner.run(3 * day, &Counting::ok("v0.5.0"));
        assert_eq!(owner.known().latest_tag.as_deref(), Some("v0.5.0"));
        assert_eq!(
            owner.offer("0.4.6"),
            None,
            "below the skipped tag: never again"
        );
        assert!(!owner.mark_is_lit("0.4.6"));

        // A stale Skip of an older tag does not lower the bar.
        owner.skip("v0.5.0").expect("written");
        assert_eq!(owner.known().skipped_tag.as_deref(), Some("v0.5.1"));

        // The pure decision, table-wise.
        let state = |latest: &str, skipped: Option<&str>| UpdateCheckV1 {
            latest_tag: Some(latest.to_owned()),
            skipped_tag: skipped.map(str::to_owned),
            ..UpdateCheckV1::default()
        };
        for (latest, skipped, offered) in [
            ("v0.5.0", None, true),
            ("v0.5.0", Some("v0.5.0"), false),
            ("v0.5.0", Some("0.5.0+other"), false),
            ("v0.5.0", Some("v0.5.1"), false),
            ("v0.5.1", Some("v0.5.0"), true),
            ("v0.5.0", Some("v0.5.0-rc.1"), true),
            ("v0.5.0", Some("nightly"), true),
        ] {
            assert_eq!(
                super::should_offer(&state(latest, skipped), "0.4.6").is_some(),
                offered,
                "{latest} with {skipped:?} skipped"
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-UPDATE-ON-ABOUT round 2, R4) — **Off stops the daily timer and
    /// nothing else**: the scheduled entry (`begin`) is refused while Off, and
    /// a reader's Check (`begin_now`) is still admitted.
    ///
    /// MUTATION: change `now || automatic` to `automatic`; manual Check is
    /// refused while Off and this goes red.
    #[test]
    fn automatic_check_off_stops_only_the_scheduled_entry() {
        assert!(!check_entry_allowed(false, false));
        assert!(check_entry_allowed(false, true));
        assert!(check_entry_allowed(true, false));
        assert!(check_entry_allowed(true, true));
    }

    /// RED (T-UPDATE-DAILY) — **a due check runs while an older offer is open
    /// and while its transaction is downloading, without replacing or
    /// cancelling either.** The newer evidence becomes the job's current
    /// decision; the running transaction completes for the immutable offer it
    /// already owns, and the newer tag can be offered afterwards.
    ///
    /// MUTATION: gate `OfferState::schedule` on `job.state().kind() == Idle`
    /// (represented by returning `Off` in the due arm); both due assertions go
    /// red before either newer answer can land.
    #[test]
    fn periodic_answers_leave_an_open_offer_and_running_transaction_frozen() {
        use std::sync::Mutex;

        use crate::install_channel::Channel;
        use crate::update_job::{
            Driver, Gathered, Job, NoDownloadDoor, Offer, Poster, Presenters, Refused,
            SharedTransport, State, Verb,
        };
        use crate::update_txn::TxnId;

        #[derive(Default)]
        struct Starts(Mutex<Option<Poster>>);
        impl Driver for Starts {
            fn prepare(
                &self,
                _: &Offer,
                _: &SharedTransport,
                post: &Poster,
            ) -> Result<(), Refused> {
                *self.0.lock().expect("the test driver") = Some(post.clone());
                Ok(())
            }
        }

        let root = dir("periodic-frozen-transaction");
        let owner = OfferState::load(&root, true);
        let day = CHECK_INTERVAL_MS;
        assert!(matches!(
            owner.run(day, &Counting::ok("v0.4.7")),
            Outcome::Answered(_)
        ));
        let gathered = |owner: &OfferState| Gathered {
            check: owner.job_evidence(),
            channel: Some(Channel::Ours),
            running: "0.4.6",
            capable: true,
            trial: false,
            platform: bt_platform::HostPlatform::Windows,
        };
        let presenters = Presenters {
            visited: &[1],
            open: &[1],
            quake: None,
        };
        let mut job = Job::with_offers(true);
        job.consider(gathered(&owner), &presenters, || TxnId::new([1; 16]));
        let first = job.state().offer().cloned().expect("the first offer");

        assert_eq!(owner.schedule(2 * day, false), Schedule::Start);
        assert!(matches!(
            owner.run(2 * day, &Counting::ok("v0.4.8")),
            Outcome::Answered(_)
        ));
        job.consider(gathered(&owner), &presenters, || TxnId::new([2; 16]));
        assert_eq!(job.state().kind(), crate::update_job::Kind::Available);
        assert_eq!(
            job.state().offer(),
            Some(&first),
            "the open offer is frozen"
        );

        let driver = Starts::default();
        let transport: SharedTransport = std::sync::Arc::new(NoDownloadDoor);
        job.answer_verb(Verb::Press, &driver, &transport)
            .expect("the first offer starts");
        let poster = driver
            .0
            .lock()
            .expect("the test driver")
            .take()
            .expect("a running transaction");
        assert!(matches!(job.state(), State::Downloading(..)));

        assert_eq!(owner.schedule(3 * day, false), Schedule::Start);
        assert!(matches!(
            owner.run(3 * day, &Counting::ok("v0.4.9")),
            Outcome::Answered(_)
        ));
        job.consider(gathered(&owner), &presenters, || TxnId::new([3; 16]));
        assert!(matches!(job.state(), State::Downloading(..)));
        assert_eq!(job.state().offer(), Some(&first));
        assert!(!poster.cancelled(), "the running transaction is untouched");
        assert_eq!(owner.offer("0.4.6").as_deref(), Some("v0.4.9"));
        assert_eq!(
            job.asked_offer(owner.offer("0.4.6").as_deref()),
            Some("v0.4.9"),
            "the newer tag is the one About offers once the transaction ends"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-UPDATE-ON-ABOUT round 2, R4) — **a known offer and its mark
    /// stay when Automatic check is Off**, loaded Off and switched Off alike.
    ///
    /// MUTATION: make `OfferState::offer` answer `None` while
    /// `!self.enabled()` (the pre-ticket suppression); the offer assertions go
    /// red (the same guard in `OfferState::mark_is_lit` reddens the mark's).
    #[test]
    fn automatic_check_off_keeps_a_known_offer_and_its_mark() {
        let root = dir("switch-off-known");
        bt_persist::write_update_check_atomic(
            &root.join(STATE_FILE_NAME),
            &UpdateCheckV1 {
                checked_at_ms: 1,
                latest_tag: Some("v9.0.0".to_owned()),
                ..UpdateCheckV1::default()
            },
        )
        .expect("a cached answer");
        let owner = OfferState::load(&root, false);
        assert_eq!(owner.offer("0.4.6").as_deref(), Some("v9.0.0"));
        assert!(owner.mark_is_lit("0.4.6"));
        owner.set_enabled(true);
        owner.set_enabled(false);
        assert_eq!(owner.offer("0.4.6").as_deref(), Some("v9.0.0"));
        assert!(owner.mark_is_lit("0.4.6"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-UPDATE-ON-ABOUT round 2, R4) — **an answer already on the wire
    /// is recorded** and offered even if Automatic check is turned Off before
    /// it lands.
    ///
    /// MUTATION: in `OfferState::ask`'s answered branch, return
    /// `Outcome::Refused` without the write while `!self.enabled()` (the
    /// pre-ticket `SwitchedOff`); the outcome and both later assertions go red.
    #[test]
    fn an_answer_already_on_the_wire_is_kept_after_automatic_check_goes_off() {
        struct TurnsOff<'owner> {
            owner: &'owner OfferState,
        }
        impl Releases for TurnsOff<'_> {
            fn latest_tag(&self) -> Result<String, String> {
                self.owner.set_enabled(false);
                Ok("v9.1.0".to_owned())
            }
        }

        let fresh = dir("switch-off-inflight");
        let inflight = OfferState::load(&fresh, true);
        assert_eq!(
            inflight.run_now(CHECK_INTERVAL_MS, &TurnsOff { owner: &inflight }),
            Outcome::Answered("v9.1.0".to_owned())
        );
        assert_eq!(inflight.offer("0.4.6").as_deref(), Some("v9.1.0"));
        assert_eq!(
            state_of(&fresh).latest_tag.as_deref(),
            Some("v9.1.0"),
            "the manual answer that landed after Off is still written"
        );

        let _ = std::fs::remove_dir_all(&fresh);
    }

    /// RED (T-UPDATE-ON-ABOUT round 2, R4) — **turning Automatic check Off
    /// does not cancel a download the reader started.** The setting's road
    /// tells the check owner only (`update::set_enabled`); the owner's
    /// evidence carries no switch, and the evidence landing again after Off
    /// leaves the job downloading: its driver is not told to stop, and its
    /// next report still moves it.
    ///
    /// MUTATION: in `Job::consider`'s `offered_this_launch` branch, stop the
    /// driver and return the job to Idle when the decision is refreshed (a
    /// re-decision of a job in flight); the job leaves Downloading and the
    /// cancel flag is set.
    #[test]
    fn automatic_check_off_does_not_cancel_a_started_download() {
        use crate::install_channel::Channel;
        use crate::update_job::{
            Bytes, Driver, Gathered, Job, NoDownloadDoor, Offer, Poster, Presenters, Refused,
            SharedTransport, State, Step, Verb,
        };
        use crate::update_txn::TxnId;

        #[derive(Default)]
        struct Starts(RefCell<Option<Poster>>);
        impl Driver for Starts {
            fn prepare(
                &self,
                _: &Offer,
                _: &SharedTransport,
                post: &Poster,
            ) -> Result<(), Refused> {
                *self.0.borrow_mut() = Some(post.clone());
                Ok(())
            }
        }

        let root = dir("switch-off-download-更新");
        bt_persist::write_update_check_atomic(
            &root.join(STATE_FILE_NAME),
            &UpdateCheckV1 {
                latest_tag: Some("v0.4.7".to_owned()),
                ..UpdateCheckV1::default()
            },
        )
        .expect("a cached answer");
        let owner = OfferState::load(&root, true);
        owner.settle();
        let gathered = |owner: &OfferState| Gathered {
            check: owner.job_evidence(),
            channel: Some(Channel::Ours),
            running: "0.4.6",
            capable: true,
            trial: false,
            platform: bt_platform::HostPlatform::Windows,
        };
        let presenters = Presenters {
            visited: &[1],
            open: &[1],
            quake: None,
        };
        let mut job = Job::with_offers(true);
        job.consider(gathered(&owner), &presenters, || TxnId::new([3; 16]));
        let driver = Starts::default();
        let transport: SharedTransport = Arc::new(NoDownloadDoor);
        job.answer_verb(Verb::Press, &driver, &transport)
            .expect("the download starts");
        let poster = driver.0.borrow_mut().take().expect("the running driver");

        owner.set_enabled(false);
        job.consider(gathered(&owner), &presenters, || TxnId::new([4; 16]));
        assert!(matches!(job.state(), State::Downloading(..)));
        assert!(!poster.cancelled(), "the driver is not told to stop");
        let bytes = Bytes {
            received: 5_000_000,
            total: Some(40_000_000),
        };
        poster.post(Step::Received(bytes));
        assert_eq!(job.drain_progress(), 0, "its report still moves the job");
        assert!(matches!(job.state(), State::Downloading(_, now) if *now == bytes));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-18) — **the update job decides only once this launch's check has
    /// said all it will, whatever it said.**
    ///
    /// The job's typed pending state waits for two facts, and this is the
    /// check's: before the check settles, the owner hands the job nothing, so a
    /// cached tag the check is about to replace is never offered on; after an
    /// answer, a refusal, a fresh stamp or a held claim, it hands over the state.
    /// Each outcome is a real `run` over a real directory.
    ///
    /// MUTATION: drop `self.settle()` from `OfferState::run` and every owner
    /// below still answers `None`.
    #[test]
    fn the_check_settles_for_the_update_job_on_every_outcome() {
        let start = 1_756_000_000_000u64;

        let root = dir("settles-answered");
        let owner = OfferState::load(&root, true);
        assert_eq!(
            owner.job_evidence(),
            None,
            "nothing is decided before the check"
        );
        assert_eq!(
            owner.run(start, &Counting::ok("v0.1.1")),
            Outcome::Answered("v0.1.1".to_owned())
        );
        let state = owner.job_evidence().expect("an answer settles the check");
        assert_eq!(state.latest_tag.as_deref(), Some("v0.1.1"));
        // A later launch inside the day: the stamp is fresh and the cache is the answer.
        let again = OfferState::load(&root, true);
        assert_eq!(again.job_evidence(), None);
        assert_eq!(
            again.run(start + 1, &Counting::ok("v0.1.2")),
            Outcome::TooSoon
        );
        assert_eq!(
            again.job_evidence().map(|state| state.latest_tag),
            Some(Some("v0.1.1".to_owned())),
            "a fresh stamp settles the check on the cached answer"
        );
        let _ = std::fs::remove_dir_all(&root);

        let root = dir("settles-refused");
        let owner = OfferState::load(&root, true);
        assert_eq!(owner.run(start, &Counting::refusing()), Outcome::Refused);
        assert!(
            owner.job_evidence().is_some(),
            "a refusal settles the check"
        );
        let _ = std::fs::remove_dir_all(&root);

        let root = dir("settles-busy");
        std::fs::write(root.join(super::CLAIM_FILE_NAME), start.to_string()).unwrap();
        let owner = OfferState::load(&root, true);
        assert_eq!(owner.run(start, &Counting::ok("v0.1.1")), Outcome::Busy);
        assert!(
            owner.job_evidence().is_some(),
            "a held claim settles the check"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN (U-15) — **the updater's trust door opens no system certificate
    /// store and installs nothing**: in `bt-platform`'s product code
    /// `CertOpenStore` is called only by `trust`'s `Store::memory` (a memory
    /// store), a certificate or revocation list is added only to the memory
    /// stores `trust` makes for its own use, and nothing names a system,
    /// registry, file or physical store provider, `CertOpenSystemStore`,
    /// `PFXImportCertStore` or `CertSaveStore`.
    ///
    /// The exclusive-root engine of the tests (E-13) is the reason a test
    /// certificate can validate at all; this is what keeps "without touching
    /// the machine store" true by construction rather than by care.
    /// `bt_platform::trust`'s `the_machine_store_is_never_written` checks the
    /// same thing from the other side, on the machine.
    ///
    /// MUTATION: open `CERT_STORE_PROV_SYSTEM_W` in `trust::arm::Store` and the
    /// forbidden list names it.
    #[test]
    fn the_trust_door_opens_no_system_certificate_store() {
        use bt_source::{Index, Pattern, Search, View, needle};
        let platform = Index::of_package("bt-platform");
        let owners_of = |name: &str| -> Vec<String> {
            let found = platform
                .search(&Search::new(
                    needle!(Pattern::path(name)),
                    View::Identifiers,
                ))
                .unwrap_or_else(|failure| panic!("{failure}"))
                .in_the_product(platform);
            let mut owners: Vec<String> = found
                .owners(platform)
                .into_keys()
                .map(|identity| {
                    let owner = identity.type_owner.map(|owner| format!("{owner}::"));
                    format!(
                        "{}::{}{}",
                        identity.module_path,
                        owner.unwrap_or_default(),
                        identity.name
                    )
                })
                .collect();
            owners.sort();
            owners
        };
        assert_eq!(
            owners_of("CertOpenStore"),
            vec!["crate::trust::arm::Store::memory".to_owned()],
            "a certificate store is opened only as a memory store"
        );
        assert_eq!(
            owners_of("CertAddEncodedCertificateToStore"),
            vec!["crate::trust::arm::Store::of_certificates".to_owned()],
        );
        assert_eq!(
            owners_of("CertAddEncodedCRLToStore"),
            vec!["crate::trust::arm::Engine::of".to_owned()],
        );
        for forbidden in [
            "CertOpenSystemStoreW",
            "CertOpenSystemStoreA",
            "CERT_STORE_PROV_SYSTEM",
            "CERT_STORE_PROV_SYSTEM_W",
            "CERT_STORE_PROV_SYSTEM_A",
            "CERT_STORE_PROV_SYSTEM_REGISTRY_W",
            "CERT_STORE_PROV_REG",
            "CERT_STORE_PROV_FILENAME_W",
            "CERT_STORE_PROV_FILE",
            "CERT_STORE_PROV_PHYSICAL_W",
            "CertAddCertificateContextToStore",
            "CertAddEncodedCertificateToSystemStoreW",
            "PFXImportCertStore",
            "CertSaveStore",
        ] {
            assert_eq!(owners_of(forbidden), Vec::<String>::new(), "{forbidden}");
        }
    }

    // ── the release feed (U-30b) ────────────────────────────────────────────

    use super::{FEED_LIST, Feed, check_source};

    /// The `file:` URL of `path`, as the product mints one (`webnav`).
    fn url_of(path: &Path) -> String {
        crate::webnav::file_url_of_local_path(&path.to_string_lossy()).expect("an absolute path")
    }

    /// **A feed folder at `folder`** listing `releases` — each a tag and
    /// whether it is a draft, with one asset beside the list — in the GitHub
    /// releases list's shape; answers the feed its folder's URL names, as the
    /// command line gives it (with the trailing slash).
    fn feed_in(folder: &Path, releases: &[(&str, bool)]) -> Feed {
        std::fs::create_dir_all(folder).expect("the feed's folder");
        let list: Vec<_> = releases
            .iter()
            .map(|(tag, draft)| {
                let name = format!("folio-{tag}.zip");
                std::fs::write(folder.join(&name), tag.as_bytes()).expect("an asset");
                serde_json::json!({
                    "tag_name": tag,
                    "name": format!("Folio {tag}"),
                    "draft": draft,
                    "prerelease": false,
                    "assets": [{
                        "name": name,
                        "browser_download_url": url_of(&folder.join(&name)),
                        "size": tag.len(),
                    }],
                })
            })
            .collect();
        std::fs::write(
            folder.join(FEED_LIST),
            serde_json::Value::Array(list).to_string(),
        )
        .expect("the list");
        Feed::at(&format!("{}/", url_of(folder)))
    }

    /// RED (U-30b) — **a process given `--update-feed` asks the feed's list,
    /// and github.com is never contacted.**
    ///
    /// The check's whole road runs — the claim, the stamp, the answer written
    /// to `update-check.json` — with the feed as its source, and the page's
    /// stand-in counts zero calls. The newest tag wins by precedence, as it
    /// does on the page's list, and a draft is not listed (the page's
    /// unauthenticated list never carries one). The URL is the command line's,
    /// through the real parser, and the diagnostics line names it.
    ///
    /// MUTATION: make `check_source` answer `page` whatever `feed` is (or call
    /// `owner.run` with `&GitHubReleases` in `begin`): the page is asked.
    #[test]
    fn a_feed_flag_makes_the_check_read_the_local_list() {
        let home = dir("feed-check");
        let feed = feed_in(
            &home.join("feed"),
            &[("v0.4.6", false), ("v0.4.7", false), ("v9.0.0", true)],
        );
        let request =
            crate::cli::parse(["--update-feed", feed.url()].map(std::ffi::OsString::from))
                .expect("an ordinary start");
        let given = request.update_feed.as_deref().expect("the flag's value");
        assert_eq!(Feed::at(given), feed);
        assert_eq!(super::feed_line(given), format!("update feed: {given}"));

        let page = Counting::ok("v0.0.1");
        let owner = OfferState::load(&home, true);
        assert_eq!(
            owner.run(CHECK_INTERVAL_MS, check_source(Some(&feed), &page)),
            Outcome::Answered("v0.4.7".to_owned())
        );
        assert_eq!(page.calls(), 0, "github.com was contacted");
        assert_eq!(state_of(&home).latest_tag.as_deref(), Some("v0.4.7"));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// RED (U-30b) — **the flag holds for its process only: nothing on the
    /// disk names the feed, and the next start without it asks github.com.**
    ///
    /// What the feed answered is the check's answer like any other (the tag
    /// in `update-check.json`); the feed itself — its URL, its folder — is
    /// written nowhere, so a start without the flag has no way to find it.
    ///
    /// MUTATION: keep the last feed in a static that `check_source` answers
    /// when it is given none — the next start reads the feed and the page is
    /// never asked.
    #[test]
    fn the_flag_is_not_persisted_and_the_next_start_uses_github() {
        let home = dir("feed-not-kept");
        let folder = home.join("feed");
        let feed = feed_in(&folder, &[("v0.4.7", false)]);
        let page = Counting::ok("v0.4.8");
        assert_eq!(
            OfferState::load(&home, true).run(CHECK_INTERVAL_MS, check_source(Some(&feed), &page)),
            Outcome::Answered("v0.4.7".to_owned())
        );
        let folder_text = folder.to_string_lossy().into_owned();
        for entry in std::fs::read_dir(&home).expect("the data directory") {
            let path = entry.expect("an entry").path();
            if path == folder {
                continue;
            }
            let text = String::from_utf8_lossy(&std::fs::read(&path).expect("a file")).into_owned();
            assert!(
                !text.contains(feed.url())
                    && !text.contains(&folder_text)
                    && !text.contains("feed"),
                "{} names the feed: {text}",
                path.display()
            );
        }

        let next = crate::cli::parse(Vec::<std::ffi::OsString>::new()).expect("a plain start");
        assert_eq!(next.update_feed, None);
        let given = next.update_feed.as_deref().map(Feed::at);
        assert_eq!(
            OfferState::load(&home, true)
                .run(2 * CHECK_INTERVAL_MS, check_source(given.as_ref(), &page)),
            Outcome::Answered("v0.4.8".to_owned())
        );
        assert_eq!(page.calls(), 1, "the next start asks github.com");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// RED (U-42e) — **a start without `--update-feed` forgets what a feed
    /// answered: the feed's tag is not offered, and its check asks
    /// github.com at once, not a day later.**
    ///
    /// 0.4.6's D-10: after a feed run, `update-check.json` kept the feed's
    /// `latest_tag v0.4.7` and its stamp, so every plain start offered v0.4.7
    /// for 24 hours and a press asked github.com for a release it did not
    /// have. The feed run here is the real check over the real file; the
    /// plain start is a second owner over the same folder.
    ///
    /// MUTATION: in `OfferState::load_for`, skip `forget_a_local_answer` —
    /// the plain start offers v0.4.7.
    #[test]
    fn a_start_without_the_feed_forgets_what_the_feed_answered() {
        let home = dir("feed-forgotten");
        let feed = feed_in(&home.join("feed"), &[("v0.4.7", false)]);
        let page = Counting::ok("v0.4.6");
        let rehearsal = OfferState::load_for(&home, true, true);
        assert_eq!(
            rehearsal.run(CHECK_INTERVAL_MS, check_source(Some(&feed), &page)),
            Outcome::Answered("v0.4.7".to_owned())
        );
        assert_eq!(rehearsal.offer("0.4.6").as_deref(), Some("v0.4.7"));
        let feeds = state_of(&home);
        assert!(
            feeds.local_stamp && feeds.local_tag,
            "the file says whose stamp and answer they are"
        );

        let plain = OfferState::load_for(&home, true, false);
        assert_eq!(plain.offer("0.4.6"), None, "the feed's tag is not offered");
        assert_eq!(
            plain.run(CHECK_INTERVAL_MS + 1, check_source(None, &page)),
            Outcome::Answered("v0.4.6".to_owned()),
            "the stamp is the feed's, so the page is asked now"
        );
        assert_eq!(page.calls(), 1);
        let after = state_of(&home);
        assert_eq!(after.latest_tag.as_deref(), Some("v0.4.6"));
        assert!(!after.local_stamp && !after.local_tag);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// RED (U-42e, review finding 4) — **a feed check that gets no answer
    /// leaves the page's tag the page's: the next plain start still offers
    /// it, and asks the page again at once over the feed's stamp.**
    ///
    /// Codex's sequence: a legitimate answer from github.com, its day passed;
    /// a start with an unreadable `--update-feed` gets no answer; the next plain
    /// start keeps the page's completed stamp and tag, sees that their day has
    /// passed, and asks the page.
    ///
    /// MUTATION: in `forget_a_local_answer`, forget the tag whenever anything
    /// is forgotten (`if forgot`): the plain start offers nothing.
    #[test]
    fn a_feed_check_with_no_answer_keeps_the_pages_tag() {
        let home = dir("feed-refused");
        let page = Counting::ok("v0.4.7");
        assert_eq!(
            OfferState::load_for(&home, true, false).run(CHECK_INTERVAL_MS, &page),
            Outcome::Answered("v0.4.7".to_owned())
        );
        let unreadable = Feed::at(&format!("{}/", url_of(&home.join("no-such-feed"))));
        assert_eq!(
            OfferState::load_for(&home, true, true).run(
                2 * CHECK_INTERVAL_MS,
                check_source(Some(&unreadable), &page)
            ),
            Outcome::Refused
        );
        let mixed = state_of(&home);
        assert!(!mixed.local_stamp && !mixed.local_tag, "{mixed:?}");

        let plain = OfferState::load_for(&home, true, false);
        assert_eq!(
            plain.offer("0.4.6").as_deref(),
            Some("v0.4.7"),
            "the page's tag is still offered"
        );
        assert_eq!(
            plain.run(2 * CHECK_INTERVAL_MS + 1, check_source(None, &page)),
            Outcome::Answered("v0.4.7".to_owned()),
            "the page's completed stamp is due, so the page is asked now"
        );
        assert_eq!(page.calls(), 2);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// RED (U-30b) — **a feed that cannot be read, or whose list is not the
    /// releases list's shape, is a failed check — the completed stamp stays,
    /// nothing is offered, nothing panics — and never a reason to ask github.com.**
    ///
    /// Each case: a URL that names no local folder, a folder that is not
    /// there, a list that is not JSON, an object where the list goes, a
    /// release without `assets`, an asset without `size`, a list of drafts
    /// only, and tags that are not versions. The same feeds give the download
    /// nothing: a file the list does not name, or names at an address that is
    /// not a local file, is the copy's refusal, with nothing left behind.
    ///
    /// MUTATION: `unwrap` the list's parse in `Feed::releases` (a malformed
    /// list panics), or let `check_source` fall back to the page when the feed
    /// fails (the page is asked).
    #[test]
    fn an_unreadable_or_malformed_feed_is_a_failed_check_not_a_panic() {
        let home = dir("feed-malformed");
        let page = Counting::ok("v0.4.8");
        let release = |extra: &str| {
            format!(
                r#"[{{"tag_name":"v0.4.7","name":"Folio","draft":false,"prerelease":false{extra}}}]"#
            )
        };
        let lists = [
            ("not-json", "this is not a list".to_owned()),
            ("object", r#"{"tag_name":"v0.4.7"}"#.to_owned()),
            ("no-assets", release("")),
            (
                "no-size",
                release(r#","assets":[{"name":"a.zip","browser_download_url":"file:///C:/a.zip"}]"#),
            ),
            (
                "drafts-only",
                r#"[{"tag_name":"v0.4.7","name":"F","draft":true,"prerelease":false,"assets":[]}]"#
                    .to_owned(),
            ),
            (
                "not-versions",
                r#"[{"tag_name":"nightly","name":"F","draft":false,"prerelease":false,"assets":[]}]"#
                    .to_owned(),
            ),
        ];
        let mut feeds = vec![
            Feed::at("https://example.invalid/feed/"),
            Feed::at(&format!("{}/", url_of(&home.join("absent")))),
        ];
        for (case, list) in lists {
            let folder = home.join(case);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join(FEED_LIST), list).unwrap();
            feeds.push(Feed::at(&url_of(&folder)));
        }
        for (at, feed) in feeds.iter().enumerate() {
            let now = (u64::try_from(at).unwrap() + 1) * CHECK_INTERVAL_MS;
            let owner = OfferState::load(&home, true);
            assert_eq!(
                owner.run(now, check_source(Some(feed), &page)),
                Outcome::Refused,
                "{}",
                feed.url()
            );
            assert_eq!(
                state_of(&home).checked_at_ms,
                0,
                "no answer leaves no completed stamp"
            );
            assert!(feed.asset("v0.4.7", "a.zip").is_err(), "{}", feed.url());
        }
        assert_eq!(page.calls(), 0, "github.com was contacted");

        // The copy: a name the list does not give, and an address that is not
        // a local file.
        let folder = home.join("remote-asset");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join(FEED_LIST),
            release(
                r#","assets":[{"name":"a.zip","browser_download_url":"https://example.invalid/a.zip","size":1}]"#,
            ),
        )
        .unwrap();
        let feed = Feed::at(&url_of(&folder));
        let into = home.join("into");
        std::fs::create_dir_all(&into).unwrap();
        let transport = crate::update_job::transport_for(
            Some(&feed),
            std::sync::Arc::new(crate::update_job::NoDownloadDoor),
        );
        let fetching = crate::update_job::Fetching {
            report: std::sync::Arc::new(|_| {}),
            cancelled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        for name in ["a.zip", "b.zip"] {
            let request = crate::update_job::Request {
                host: crate::update_job::RELEASE_HOST,
                path: String::new(),
                tag: "v0.4.7".to_owned(),
                file_name: name.to_owned(),
            };
            assert!(
                transport.fetch(&request, &into, &fetching).is_err(),
                "{name}"
            );
            assert!(!into.join(name).exists(), "{name}: nothing is left");
        }
        let _ = std::fs::remove_dir_all(&home);
    }
}
