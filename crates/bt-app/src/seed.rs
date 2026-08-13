//! One vault, three doors — `docs/DESIGN.md` §7.1.4, mock-up 3971-3974.
//!
//! Pin ("always bring this back"), Recent ("any of these can come back") and
//! undo-close ("that one, now") are three ways to draw from **one** store. Built
//! apart they would be three copies of the same write path, and the copy that
//! drifts is always the one you use least.
//!
//! What the store holds is a [`Seed`]: profile + place + your name for it.
//! Never output. Reviving a seed produces a *new shell standing in the right
//! folder*, and it says so — that is 「不存输出历史」 made visible, not a
//! limitation being apologised for.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bt_persist::{RecentEntryV1, RecentSeedV1};

/// How many seeds the vault keeps. Mock-up 4056: `state.recent.slice(0, 8)`.
pub const RECENT_CAPACITY: usize = 8;

/// What survives a close.
///
/// The two shapes mirror [`RecentSeedV1`] because they mirror the two kinds of
/// leaf a tab can be made of — `docs/DESIGN.md` §7.1.4: "Recent 条目 = 终端
/// seed **或 files 场所**（关闭纯 files tab 同样可撤销）". A files-only tab that
/// could be restored by the shutdown prompt but not by Ctrl+Shift+T would be two
/// doors onto one store with one of them broken, which is the exact failure this
/// module exists to prevent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seed {
    /// A terminal: the profile that launched it, where it stood, what you called
    /// it. `profile_id` is a **stable id, not the title and not a display
    /// object** (§7.1.4) — a title is something the user can change and the
    /// program can overwrite, so keying identity on it would lose the tab.
    Term {
        profile_id: String,
        cwd: String,
        manual_name: Option<String>,
    },
    /// A place a files pane was rooted at.
    Files { root: String },
}

impl Seed {
    /// The dedup key — `docs/DESIGN.md` §7.1.4: "去重键 = profile_id+cwd+手动名
    /// （同位置不同名的 agent 保持独立条目）".
    ///
    /// Note this deliberately **includes the manual name**, where the mock-up's
    /// `recentKey` (4045) used `title @ cwd` and dropped it. The prototype had no
    /// stable profile id to key on, so its `title` was standing in for one; the
    /// spec that outranks it says two agents in the same folder under different
    /// names are two entries, and the checked-in fixture already encodes the
    /// pipe-joined three-slot form.
    #[must_use]
    pub fn recent_key(&self) -> String {
        match self {
            Self::Term {
                profile_id,
                cwd,
                manual_name,
            } => format!(
                "{profile_id}|{cwd}|{}",
                manual_name.as_deref().unwrap_or_default()
            ),
            // The files locus has no profile and no name of its own, so it takes
            // the same three slots with the two it cannot fill left empty.
            Self::Files { root } => format!("|{root}|"),
        }
    }

    /// Whether this is a thing that can carry a name the user typed — the guard
    /// `startRename` opens with (mock-up 5858-5859: `const s =
    /// tabIdentSession(w); if (!s) return;   /* a files-only tab has no session
    /// to name */`).
    ///
    /// The manual name is a slot on the *terminal* seed and on nothing else: a
    /// files place is identified by its root, which is a fact about the disk
    /// rather than something anyone chose. So a tab whose identity leaf is a
    /// files pane has no field for the editor to write to, and the editor must
    /// decline to open rather than open onto nothing.
    ///
    /// **Today this never answers `false` for a real tab.** Every tab in this
    /// build holds a terminal (`TabState::seed` constructs `Term`
    /// unconditionally), so the guard is satisfied by construction and this is
    /// the stub the ticket asks for: the *shape* of the mock-up's condition,
    /// carrying the one case that will exist once T5 gives a tab a files-only
    /// identity leaf. It is a real function and not a comment because the day
    /// that case arrives, the caller must already be asking.
    #[must_use]
    pub fn can_be_named(&self) -> bool {
        match self {
            Self::Term { .. } => true,
            Self::Files { .. } => false,
        }
    }
}

impl From<&Seed> for RecentSeedV1 {
    fn from(seed: &Seed) -> Self {
        match seed {
            Seed::Term {
                profile_id,
                cwd,
                manual_name,
            } => Self::Term {
                profile_id: profile_id.clone(),
                cwd: cwd.clone(),
                manual_name: manual_name.clone(),
            },
            Seed::Files { root } => Self::Files { root: root.clone() },
        }
    }
}

impl From<&RecentSeedV1> for Seed {
    fn from(seed: &RecentSeedV1) -> Self {
        match seed {
            RecentSeedV1::Term {
                profile_id,
                cwd,
                manual_name,
            } => Self::Term {
                profile_id: profile_id.clone(),
                cwd: cwd.clone(),
                manual_name: manual_name.clone(),
            },
            RecentSeedV1::Files { root } => Self::Files { root: root.clone() },
        }
    }
}

/// One entry in the vault: a seed, whatever it was looking at, and when it was
/// put there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentEntry {
    pub seed: Seed,
    /// The files this tab's preview panes were showing, in tree order — 裁决 10
    /// (2026-08-12).
    ///
    /// **The same bug, caught the second time by name.** This module's own
    /// header records what happened when a leaf kind was left out of the vault:
    /// a files-only tab came back through the shutdown prompt and could not be
    /// reached by Ctrl+Shift+T, which is two doors onto one store with one of
    /// them broken. The session file now brings a preview pane back
    /// (`bt_persist::TabV1::preview`), so an entry that dropped it would be that
    /// asymmetry again, in this store, for this reason.
    ///
    /// Paths only, and no pins: Recent is a *launcher*, not a layout. It
    /// restores the places you were — the pool regrows from disk on demand, and
    /// nobody ever promised a closed tab's pane arrangement back.
    pub previews: Vec<String>,
    /// Absolute, not relative — "3 分钟前" is computed at the moment it is drawn,
    /// so a vault read back from disk a day later says "a day ago" rather than
    /// repeating whatever it said when it was written.
    pub at: SystemTime,
}

/// The store itself. Most-recent-first, deduped by [`Seed::recent_key`], capped
/// at [`RECENT_CAPACITY`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SeedVault {
    entries: Vec<RecentEntry>,
}

impl SeedVault {
    /// Put a seed in, newest first, with whatever its tab was previewing.
    ///
    /// Re-recording a place you already have **moves it to the front and keeps
    /// one copy** rather than growing a second: the vault is a list of places,
    /// and a place is somewhere you can be more than once (mock-up 4053-4054).
    /// The newest recording's previews win outright, for the same reason the
    /// newest timestamp does — the entry describes the last time you were there.
    ///
    /// `previews` is a parameter and not an `Option` with a default, so every
    /// door into the vault has to answer the question. A writer that could stay
    /// silent about a leaf kind is precisely how the files leaf went missing.
    pub fn record(&mut self, seed: Seed, previews: Vec<String>, at: SystemTime) {
        let key = seed.recent_key();
        self.entries.retain(|entry| entry.seed.recent_key() != key);
        self.entries.insert(0, RecentEntry { seed, previews, at });
        self.entries.truncate(RECENT_CAPACITY);
    }

    #[must_use]
    pub fn entries(&self) -> &[RecentEntry] {
        &self.entries
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Draw an entry back out, removing it — mock-up 7366,
    /// `state.recent.splice(i, 1)`.
    ///
    /// Taking it out is what makes Recent a *launcher* rather than a history: the
    /// entry has become a tab, and leaving a copy behind would offer to open a
    /// place that is already open.
    ///
    /// The whole entry rather than its seed: the tab being rebuilt is the seed
    /// **and** what it was looking at, and a door that handed back only the seed
    /// is a door that reopens the shell without the page beside it.
    pub fn take(&mut self, index: usize) -> Option<RecentEntry> {
        (index < self.entries.len()).then(|| self.entries.remove(index))
    }

    /// Rebuild from what was on disk, newest-first order preserved. Entries whose
    /// timestamp cannot be read are dropped rather than guessed at: an entry
    /// claiming a time we invented would print a confident "just now" lie.
    #[must_use]
    pub fn from_persisted(entries: &[RecentEntryV1]) -> Self {
        Self {
            entries: entries
                .iter()
                .filter_map(|entry| {
                    Some(RecentEntry {
                        seed: Seed::from(&entry.seed),
                        previews: entry.previews.clone(),
                        at: parse_iso8601_utc(&entry.timestamp)?,
                    })
                })
                .take(RECENT_CAPACITY)
                .collect(),
        }
    }

    #[must_use]
    pub fn to_persisted(&self) -> Vec<RecentEntryV1> {
        self.entries
            .iter()
            .map(|entry| RecentEntryV1 {
                key: entry.seed.recent_key(),
                seed: RecentSeedV1::from(&entry.seed),
                timestamp: format_iso8601_utc(entry.at),
                previews: entry.previews.clone(),
            })
            .collect()
    }
}

/// `just now` / `Nm ago` / `Nh ago` — mock-up 7280-7285.
///
/// Anything older than an hour keeps counting in hours rather than graduating to
/// days, because the prototype's own ladder stops there and a vault of eight
/// entries rarely reaches back further.
#[must_use]
pub fn ago_label(at: SystemTime, now: SystemTime) -> String {
    // A seed stamped in the future (a clock that moved backwards) is not an
    // error worth a branch of its own — it reads as "just now", which is the
    // closest true thing we can say about it.
    let minutes = now.duration_since(at).unwrap_or(Duration::ZERO).as_secs() / 60;
    if minutes < 1 {
        "just now".to_owned()
    } else if minutes < 60 {
        format!("{minutes}m ago")
    } else {
        format!("{}h ago", minutes / 60)
    }
}

/// Pinned tabs lead — mock-up 4071-4073.
///
/// **Stable within each run**: pinning one tab must not shuffle the others, so
/// this is a stable partition and not a sort on some ordering key. Order has
/// exactly one home (the tab list itself) because drag-reorder writes to it;
/// re-deriving order at paint time would fight the drag and the drag would win
/// every other frame.
pub fn normalize_pins<T>(tabs: &mut [T], is_pinned: impl Fn(&T) -> bool) {
    // `sort_by_key` is documented stable, so `false < true` puts the pinned run
    // first while leaving both runs in their original relative order.
    tabs.sort_by_key(|tab| !is_pinned(tab));
}

/// Is the pinned-lead invariant currently true?
///
/// This is the partition F57 has to preserve, and it is now asked rather than
/// merely written down: `Runtime::tab_trailers` — the one function both the
/// strip and the rail build their rows from — asserts it every frame, so a write
/// path that breaks the partition is named on the frame it breaks it.
///
/// It was `#[allow(dead_code)]` while it waited for that call site, and the wait
/// was the cost: the path that actually broke the invariant (N160①'s "pin
/// follows content", which pins a tab where it stands) shipped and went
/// unnoticed, because the sentence it would have failed was never spoken.
#[must_use]
pub fn pins_are_normalized<T>(tabs: &[T], is_pinned: impl Fn(&T) -> bool) -> bool {
    let pinned = tabs.iter().filter(|tab| is_pinned(tab)).count();
    tabs.iter().take(pinned).all(is_pinned)
}

/// How a pin travels when panes move between tabs — `P1-11`, quoted into
/// `docs/DESIGN.md` §7.1.4 as three rules. T5 owns the gestures; the rules live
/// here so both tickets read the same sentence.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinMigration {
    /// A tab is merged into a target: the target inherits the pin.
    MergedInto,
    /// A pane is pushed back out to the strip: it keeps the origin tab's pin.
    PoppedOut,
    /// Explicitly dragged out into a brand-new tab: **not** inherited — nobody
    /// ever promised to bring this one back.
    DraggedOutToNewTab,
}

#[allow(dead_code)]
impl PinMigration {
    /// Whether the resulting tab is pinned, given the origin tab's pin.
    #[must_use]
    pub fn resulting_pin(self, origin_pinned: bool) -> bool {
        match self {
            Self::MergedInto | Self::PoppedOut => origin_pinned,
            Self::DraggedOutToNewTab => false,
        }
    }
}

// ── ISO-8601 UTC ──
//
// `RecentEntryV1::timestamp` is an opaque ISO-8601 string and bt-persist has no
// reason to parse it; the age-based label is a call-site concern, so the call
// site is where the conversion lives. The workspace has no date-time dependency,
// and these two functions are the whole of what a `Nm ago` label needs.
//
// The civil/day conversions are Howard Hinnant's `days_from_civil` /
// `civil_from_days` — exact integer arithmetic over the proleptic Gregorian
// calendar, valid for any year, not a lookup table with a range.

const SECONDS_PER_DAY: i64 = 86_400;

/// `YYYY-MM-DDTHH:MM:SSZ`, matching the checked-in fixtures.
#[must_use]
pub fn format_iso8601_utc(at: SystemTime) -> String {
    let secs = match at.duration_since(UNIX_EPOCH) {
        Ok(delta) => i64::try_from(delta.as_secs()).unwrap_or(i64::MAX),
        // Pre-epoch: `duration_since` reports how far back, so negate it.
        Err(err) => -i64::try_from(err.duration().as_secs()).unwrap_or(i64::MAX),
    };
    // Floor-divide so pre-epoch instants land on the right day rather than
    // truncating toward zero into the next one.
    let days = secs.div_euclid(SECONDS_PER_DAY);
    let rem = secs.rem_euclid(SECONDS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// The inverse, tolerant of a fractional-second part and of `+00:00` for `Z`,
/// because a timestamp is data we did not necessarily write ourselves.
#[must_use]
pub fn parse_iso8601_utc(text: &str) -> Option<SystemTime> {
    let bytes = text.as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if bytes[10] != b'T' && bytes[10] != b' ' {
        return None;
    }
    if bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let num = |from: usize, to: usize| text.get(from..to)?.parse::<i64>().ok();
    let year = num(0, 4)?;
    let month = num(5, 7)?;
    let day = num(8, 10)?;
    let hour = num(11, 13)?;
    let minute = num(14, 16)?;
    let second = num(17, 19)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let secs =
        days_from_civil(year, month, day) * SECONDS_PER_DAY + hour * 3600 + minute * 60 + second;
    if secs >= 0 {
        UNIX_EPOCH.checked_add(Duration::from_secs(secs as u64))
    } else {
        UNIX_EPOCH.checked_sub(Duration::from_secs(secs.unsigned_abs()))
    }
}

/// Days since 1970-01-01 for a proleptic-Gregorian civil date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    // Shift the year so it starts in March, which puts the leap day last and
    // makes the day-of-year a single linear expression.
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(cwd: &str, name: Option<&str>) -> Seed {
        Seed::Term {
            profile_id: "pwsh".to_owned(),
            cwd: cwd.to_owned(),
            manual_name: name.map(str::to_owned),
        }
    }

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    /// PIN — the vault's ruler. `docs/DESIGN.md` §7.1.4 and mock-up 4106.
    #[test]
    fn the_vault_measures_what_the_spec_says_it_measures() {
        assert_eq!(RECENT_CAPACITY, 8, "state.recent.slice(0, 8)");
    }

    /// J104 (mock-up 5854-5859) — the stub the ticket asks for.
    ///
    /// `startRename` refuses to open on a tab with no session to name. The
    /// condition is real and is written down here; what makes this a *stub* is
    /// that nothing in this build can currently fail it, because every tab seeds
    /// as a `Term` (`TabState::seed`). The `Files` half is the case T5 will
    /// create, and it is asserted now so the guard is not invented later by
    /// somebody who has forgotten why it exists.
    #[test]
    fn only_a_thing_with_a_session_can_be_given_a_name() {
        assert!(
            term("C:\\notes", None).can_be_named(),
            "a terminal has a manual-name slot, so the editor may open on it"
        );
        assert!(
            !Seed::Files {
                root: "C:\\docs".to_owned()
            }
            .can_be_named(),
            "a files place is identified by its root — there is nothing to type into"
        );
    }

    /// The key is the spec's three slots, pipe-joined, with `None` as empty —
    /// the shape already checked into the canonical fixture.
    #[test]
    fn the_dedup_key_is_profile_and_place_and_your_name_for_it() {
        assert_eq!(term("C:\\notes", None).recent_key(), "pwsh|C:\\notes|");
        assert_eq!(
            term("C:\\notes", Some("build")).recent_key(),
            "pwsh|C:\\notes|build"
        );
        assert_eq!(
            Seed::Files {
                root: "C:\\docs".to_owned()
            }
            .recent_key(),
            "|C:\\docs|"
        );
    }

    /// §7.1.4: "同位置不同名的 agent 保持独立条目". This is the clause that
    /// parts ways with the mock-up's `title @ cwd`, so it gets its own gate.
    #[test]
    fn two_agents_in_one_folder_under_different_names_stay_two_entries() {
        let mut vault = SeedVault::default();
        vault.record(term("C:\\repo", Some("claude")), Vec::new(), at(0));
        vault.record(term("C:\\repo", Some("codex")), Vec::new(), at(60));
        assert_eq!(vault.len(), 2, "the name is part of the identity");

        vault.record(term("C:\\repo", Some("claude")), Vec::new(), at(120));
        assert_eq!(vault.len(), 2, "the same agent is still one entry");
        assert_eq!(vault.entries()[0].seed, term("C:\\repo", Some("claude")));
        assert_eq!(vault.entries()[0].at, at(120), "re-recording restamps it");
    }

    #[test]
    fn the_vault_holds_eight_and_forgets_from_the_bottom() {
        let mut vault = SeedVault::default();
        for i in 0..12 {
            vault.record(term(&format!("C:\\p{i}"), None), Vec::new(), at(i * 60));
        }
        assert_eq!(vault.len(), RECENT_CAPACITY);
        assert_eq!(
            vault.entries()[0].seed,
            term("C:\\p11", None),
            "newest first"
        );
        assert_eq!(
            vault.entries()[RECENT_CAPACITY - 1].seed,
            term("C:\\p4", None),
            "the oldest survivor"
        );
    }

    /// Mock-up 7366: reopening splices the entry out. Recent is a launcher, not
    /// a history — an entry that has become a tab must stop offering itself.
    #[test]
    fn drawing_a_seed_out_of_the_vault_removes_it() {
        let mut vault = SeedVault::default();
        vault.record(term("C:\\a", None), Vec::new(), at(0));
        vault.record(term("C:\\b", None), Vec::new(), at(60));

        assert_eq!(
            vault.take(0).map(|entry| entry.seed),
            Some(term("C:\\b", None)),
            "undo-close = 0"
        );
        assert_eq!(vault.len(), 1);
        assert!(vault.take(5).is_none(), "out of range is not a panic");
    }

    /// Mock-up 7280-7285.
    #[test]
    fn the_ago_label_counts_in_minutes_then_hours() {
        let now = at(100_000);
        assert_eq!(ago_label(now, now), "just now");
        assert_eq!(
            ago_label(at(100_000 - 59), now),
            "just now",
            "under a minute"
        );
        assert_eq!(ago_label(at(100_000 - 60), now), "1m ago");
        assert_eq!(ago_label(at(100_000 - 59 * 60), now), "59m ago");
        assert_eq!(ago_label(at(100_000 - 60 * 60), now), "1h ago");
        assert_eq!(ago_label(at(100_000 - 300 * 60), now), "5h ago");
        assert_eq!(ago_label(at(200_000), now), "just now", "a backwards clock");
    }

    /// 裁决 10 (2026-08-12) — **a closed tab's preview travels in the vault with
    /// it**, so undo-close and the restore prompt bring the same tab back.
    ///
    /// The red gate is the asymmetry itself: drop `previews` anywhere on the
    /// path — the recorder, the wire form, the read back — and the page the tab
    /// was showing survives a shutdown but not a Ctrl+Shift+T, which is exactly
    /// the shape of the files-leaf bug this module's header records.
    #[test]
    fn a_closed_tabs_preview_comes_back_out_of_the_vault_with_it() {
        let mut vault = SeedVault::default();
        let pages = vec![
            r"C:\repo\README.md".to_owned(),
            r"C:\repo\src\main.rs".to_owned(),
        ];
        vault.record(term(r"C:\repo", None), pages.clone(), at(0));

        // Through the wire and back: the paths are on the entry, not on the seed,
        // and a `to_persisted` that forgot them would still round-trip the seed.
        let persisted = vault.to_persisted();
        assert_eq!(persisted[0].previews, pages);
        let reloaded = SeedVault::from_persisted(&persisted);
        assert_eq!(reloaded, vault, "a full round trip through the wire form");

        let drawn = reloaded.entries()[0].clone();
        assert_eq!(drawn.previews, pages, "in tree order, both of them");

        // A tab that was previewing nothing says so, rather than inheriting the
        // last tab's pages: the field is per entry.
        vault.record(term(r"C:\other", None), Vec::new(), at(60));
        assert!(vault.entries()[0].previews.is_empty());
        assert_eq!(vault.entries()[1].previews, pages);
    }

    /// Re-recording a place you already have replaces its pages too. The entry
    /// describes *the last time you were there*, and half-updating it would
    /// offer to bring back a page you had already closed.
    #[test]
    fn the_newest_recording_of_a_place_brings_its_own_pages() {
        let mut vault = SeedVault::default();
        vault.record(
            term(r"C:\repo", None),
            vec![r"C:\repo\a.md".to_owned()],
            at(0),
        );
        vault.record(
            term(r"C:\repo", None),
            vec![r"C:\repo\b.md".to_owned()],
            at(60),
        );
        assert_eq!(vault.len(), 1, "still one place");
        assert_eq!(
            vault.entries()[0].previews,
            vec![r"C:\repo\b.md".to_owned()]
        );
    }

    /// The absolute stamp is what makes the label honest across a restart: a
    /// vault written yesterday must say "20h ago" today, not "just now".
    #[test]
    fn the_stamp_survives_the_disk_and_the_label_is_computed_fresh() {
        let mut vault = SeedVault::default();
        let written = at(1_780_000_000);
        vault.record(term("C:\\notes", Some("build")), Vec::new(), written);

        let persisted = vault.to_persisted();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].key, "pwsh|C:\\notes|build");
        assert_eq!(
            persisted[0].seed,
            RecentSeedV1::Term {
                profile_id: "pwsh".to_owned(),
                cwd: "C:\\notes".to_owned(),
                manual_name: Some("build".to_owned()),
            }
        );

        let reloaded = SeedVault::from_persisted(&persisted);
        assert_eq!(reloaded, vault, "a full round trip through the wire form");
        assert_eq!(
            ago_label(
                reloaded.entries()[0].at,
                written + Duration::from_secs(20 * 3600)
            ),
            "20h ago"
        );
    }

    /// An entry whose stamp we cannot read is dropped, not given an invented one
    /// — a guessed timestamp prints a confident lie in the menu.
    #[test]
    fn an_unreadable_stamp_drops_its_entry_rather_than_inventing_a_time() {
        let entry = |cwd: &str, timestamp: &str| RecentEntryV1 {
            key: format!("pwsh|{cwd}|"),
            seed: RecentSeedV1::Term {
                profile_id: "pwsh".to_owned(),
                cwd: cwd.to_owned(),
                manual_name: None,
            },
            timestamp: timestamp.to_owned(),
            previews: Vec::new(),
        };
        let vault = SeedVault::from_persisted(&[
            entry("C:\\a", "not a time"),
            entry("C:\\b", "2026-08-02T21:14:00Z"),
        ]);
        assert_eq!(vault.len(), 1);
        assert_eq!(vault.entries()[0].seed, term("C:\\b", None));
    }

    #[test]
    fn iso8601_round_trips_and_matches_the_fixture_spelling() {
        let parsed = parse_iso8601_utc("2026-08-02T21:14:00Z").expect("fixture stamp parses");
        assert_eq!(format_iso8601_utc(parsed), "2026-08-02T21:14:00Z");

        // Leap day, epoch, and a pre-epoch instant: the civil conversion is
        // arithmetic over the whole calendar, not a table with a range.
        for text in [
            "1970-01-01T00:00:00Z",
            "2024-02-29T12:34:56Z",
            "1969-12-31T23:59:59Z",
            "2000-02-29T00:00:00Z",
            "2100-03-01T00:00:00Z",
        ] {
            let at = parse_iso8601_utc(text).expect("parses");
            assert_eq!(format_iso8601_utc(at), text, "round trip {text}");
        }

        assert_eq!(parse_iso8601_utc("2026-08-02"), None, "too short");
        assert_eq!(parse_iso8601_utc("2026-13-02T00:00:00Z"), None, "month 13");
        assert_eq!(parse_iso8601_utc(""), None);
    }

    /// Mock-up 4066-4073: pinned lead, and pinning one tab does not shuffle the
    /// others.
    #[test]
    fn pinned_tabs_lead_and_the_rest_keep_their_order() {
        let mut tabs = vec![("a", false), ("b", true), ("c", false), ("d", true)];
        normalize_pins(&mut tabs, |tab| tab.1);
        assert_eq!(
            tabs,
            vec![("b", true), ("d", true), ("a", false), ("c", false)],
            "stable within each run"
        );
        assert!(pins_are_normalized(&tabs, |tab| tab.1));

        let before = tabs.clone();
        normalize_pins(&mut tabs, |tab| tab.1);
        assert_eq!(tabs, before, "idempotent");
    }

    #[test]
    fn the_invariant_notices_an_unpinned_tab_inside_the_pinned_run() {
        assert!(pins_are_normalized(&[true, true, false], |t| *t));
        assert!(pins_are_normalized::<bool>(&[], |t| *t));
        assert!(pins_are_normalized(&[false, false], |t| *t));
        assert!(
            !pins_are_normalized(&[true, false, true], |t| *t),
            "a pinned tab stranded behind an unpinned one"
        );
    }

    /// P1-11, quoted into §7.1.4. T5 owns the gestures; the rules are stated
    /// once, here, so the drag ticket consumes them instead of restating them.
    #[test]
    fn a_pin_travels_with_content_except_when_it_was_never_promised() {
        assert!(PinMigration::MergedInto.resulting_pin(true));
        assert!(PinMigration::PoppedOut.resulting_pin(true));
        assert!(
            !PinMigration::DraggedOutToNewTab.resulting_pin(true),
            "显式拖出成新标签 = 不继承"
        );
        for rule in [
            PinMigration::MergedInto,
            PinMigration::PoppedOut,
            PinMigration::DraggedOutToNewTab,
        ] {
            assert!(!rule.resulting_pin(false), "nothing invents a pin");
        }
    }
}
