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

use bt_persist::{PreviewSourceV1, RecentEntryV1, RecentPreviewV1, RecentSeedV1};

/// How many seeds the vault keeps. Mock-up 4056: `state.recent.slice(0, 8)`.
pub const RECENT_CAPACITY: usize = 8;

/// What survives a close.
///
/// The shapes mirror [`RecentSeedV1`] because they mirror the kinds of leaf a
/// tab can be made of — `docs/DESIGN.md` §7.1.4: "Recent 条目 = 终端 seed **或
/// files 场所**（关闭纯 files tab 同样可撤销）". A files-only tab that could be
/// restored by the shutdown prompt but not by Ctrl+Shift+T would be two doors
/// onto one store with one of them broken, which is the exact failure this
/// module exists to prevent.
///
/// **The third shape arrived with §7.1.6h**, and it arrived by that same
/// sentence rather than beside it: the slice that made a lone preview pane a tab
/// would otherwise have created a tab shape the vault had no row for, which is
/// the identical asymmetry one file over. `session.json` carries it as
/// `RecentSeedV1::Preview` from schema v8.
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
    /// The file — or, from W2 slice ③, the **page** — a lone preview pane was
    /// on (§7.1.6h).
    ///
    /// One shape and not two, because a page and a file are one kind of tab: the
    /// same pane, the same close, the same row in this vault. `source` is which
    /// of the two the string is, and it is carried rather than inferred for the
    /// reason `bt_persist::PreviewSourceV1` states — a build that guessed would
    /// hand a URL to a filesystem or a path to a navigation.
    Preview {
        path: String,
        source: PreviewSourceV1,
    },
    /// **A whole window that was closed** (multiwindow slice D, ruling ②).
    ///
    /// Closing a tab has always filled this vault, and closing a window closes
    /// every tab in it — so without this shape the one gesture that throws away
    /// six tabs at once would be the only one with no way back, which is the
    /// asymmetry this module's header says it exists to prevent. One row rather
    /// than six, because what was lost was a window.
    ///
    /// The seeds of its tabs and nothing else: no rectangle, no rail, no tree.
    /// That is this module's standing sentence — "Recent restores the places you
    /// were, not a layout" — said about a bigger object, and it is already why a
    /// closed folder tab forgets its column width. A window drawn back out opens
    /// where a new window opens, holding the places its tabs stood in.
    Window { seeds: Vec<Seed> },
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
            // And the file takes the *third* slot, which is the one a place
            // leaves empty — so a preview tab on `D:\a\b` and a files tab
            // rooted at `D:\a\b` are two rows and not one. They are two
            // different tabs and a key that collapsed them would let opening a
            // folder evict the file you had open in it.
            // **A page takes a fourth spelling of the third slot** and a file
            // keeps the one it has always had, because every vault row on every
            // disk says `||{path}` and re-keying them would evict them all on
            // first launch. The two cannot collide and it is provable rather than
            // likely: `|` is not a legal character in a Windows path, so no file
            // seed can produce a third slot beginning `url|`; and a window's key
            // begins `|||`, which this never does.
            Self::Preview {
                path,
                source: PreviewSourceV1::File,
            } => format!("||{path}"),
            Self::Preview {
                path,
                source: PreviewSourceV1::Url,
            } => format!("||url|{path}"),
            // **A fourth slot, which no tab shape can reach.** The three above
            // are `a|b|c`; this one is `|||…`, so a window can never collide
            // with a tab however its children are spelled. Inside it, the
            // children's own keys joined by a newline — a character no Windows
            // path may contain, so the join is unambiguous without inventing an
            // escape.
            //
            // A window is therefore deduped **by what was in it**, on the same
            // rule every other shape follows: closing two windows holding the
            // same places twice is one row, exactly as closing the same folder
            // twice is.
            Self::Window { seeds } => {
                let mut key = String::from("|||");
                for (index, seed) in seeds.iter().enumerate() {
                    if index > 0 {
                        key.push('\n');
                    }
                    key.push_str(&seed.recent_key());
                }
                key
            }
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
    /// A file is turned away on the same footing as a place and for the same
    /// reason: what identifies it is a path on disk.
    ///
    /// **This used to say it never answered `false` for a real tab**, and stood
    /// as the stub for "the one case that will exist once T5 gives a tab a
    /// files-only identity leaf". §7.1.6h is that day: both `false` arms are now
    /// reachable, `TabState::seed` builds all three shapes, and `open_rename`'s
    /// guard is a live gate rather than a promise.
    #[must_use]
    pub fn can_be_named(&self) -> bool {
        match self {
            Self::Term { .. } => true,
            // A window is turned away on the two above's own footing and one of
            // its own: it is not a tab at all, so there is no tab head for an
            // editor to open on.
            Self::Files { .. } | Self::Preview { .. } | Self::Window { .. } => false,
        }
    }

    /// **Whether this seed can say what it is** — the vault's own door policy
    /// (2026-08-20).
    ///
    /// `TabState::seed`'s standing rule is that an unwritable row is not written
    /// rather than written as a guess. That rule was enforced only where a seed
    /// could not be *built* at all; a seed that was built out of nothing — a
    /// shell that never reported a folder, a column that was never pointed
    /// anywhere — went in and came out as a row with no caption. Which is the
    /// same failure one layer down: a line in `RECENTLY OPENED` offering to bring
    /// something back without saying what.
    ///
    /// So the rule is asked here, at the shape, and both doors into the store ask
    /// it: [`SeedVault::record`] for what this run closes, and
    /// [`SeedVault::from_persisted`] for what an older one left on disk. One
    /// sentence, two doors — this module's own header explains what happens when
    /// a store is reached by two paths that do not agree.
    ///
    /// **A window answers for its children**: it is captioned by the first tab
    /// that can name itself ([`Self::first_tab`]), so it is nameable exactly when
    /// one of them is. `any` and not `all`, on `recent_is_available`'s footing:
    /// dropping a whole window because one tab of six is anonymous would refuse
    /// the other five.
    #[must_use]
    pub fn names_itself(&self) -> bool {
        match self {
            // Your name for it, or the folder it stood in. Both empty is a row
            // that would draw nothing at all.
            Self::Term {
                cwd, manual_name, ..
            } => !cwd.is_empty() || manual_name.as_deref().is_some_and(|name| !name.is_empty()),
            // A place and a file are their own captions, so an empty one is not a
            // caption. There is no second field to fall through to.
            Self::Files { root } => !root.is_empty(),
            Self::Preview { path, .. } => !path.is_empty(),
            Self::Window { seeds } => seeds.iter().any(Self::names_itself),
        }
    }

    /// How many tabs a window seed stands for — the one number the row's tooltip
    /// says out loud, and `None` for every shape that is a tab rather than a
    /// window full of them.
    #[must_use]
    pub fn window_tabs(&self) -> Option<usize> {
        match self {
            Self::Window { seeds } => Some(seeds.len()),
            Self::Term { .. } | Self::Files { .. } | Self::Preview { .. } => None,
        }
    }

    /// The tab a window row is *recognised* by — the first one that can say what
    /// it is — and `None` for every shape that is a tab already.
    ///
    /// A window row wears the name of the tab it opened with, which is what a
    /// reader would name it by ("the window I had `alpha` in"). The word
    /// "window" is not in the label because a label is measured and a row is one
    /// line; it is in the tooltip, beside the count, where this list already
    /// puts its detail.
    ///
    /// **"First" means first nameable, and skipping is not the same as
    /// dropping** (2026-08-20). A window can hold a tab this build cannot name —
    /// a leaf a newer build wrote, a column never pointed anywhere — and a window
    /// captioned by that tab is a blank row. The tab is still in `seeds` and
    /// still comes back with the window; it simply is not what the window is
    /// called. Filtering it out of the list instead would refuse to reopen a tab
    /// merely because it has no caption, which is a much larger claim than the
    /// label was making.
    #[must_use]
    pub fn first_tab(&self) -> Option<&Self> {
        match self {
            Self::Window { seeds } => seeds.iter().find(|seed| seed.names_itself()),
            Self::Term { .. } | Self::Files { .. } | Self::Preview { .. } => None,
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
            Seed::Preview { path, source } => Self::Preview {
                path: path.clone(),
                source: *source,
            },
            Seed::Window { seeds } => Self::Window {
                seeds: seeds.iter().map(Self::from).collect(),
            },
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
            RecentSeedV1::Preview { path, source } => Self::Preview {
                path: path.clone(),
                source: *source,
            },
            RecentSeedV1::Window { seeds } => Self::Window {
                seeds: seeds.iter().map(Self::from).collect(),
            },
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
    /// Places only, and no pins: Recent is a *launcher*, not a layout. It
    /// restores the places you were — the pool regrows from disk on demand, and
    /// nobody ever promised a closed tab's pane arrangement back.
    ///
    /// **A page is one of those places** (W2 slice ③), which is why the element
    /// is the wire type rather than a bare `String`: it is the one list in this
    /// module whose rows are scalars, and `bt_persist::RecentPreviewV1` is where
    /// the rule about telling a path from a URL without guessing already lives.
    pub previews: Vec<RecentPreviewV1>,
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
    ///
    /// **A seed that cannot name itself is not recorded** ([`Seed::names_itself`],
    /// 2026-08-20). Not an error and not a log line: the caller asked the store
    /// to remember a place, and a place nobody can name is not one this list can
    /// offer to go back to. Refusing here rather than at the drawing is what
    /// keeps the row indices honest — `MenuRow::Recent` carries the vault's own
    /// index straight into [`Self::take`], so a menu that hid a row the vault
    /// still held would revive the entry beside the one that was clicked.
    pub fn record(&mut self, seed: Seed, previews: Vec<RecentPreviewV1>, at: SystemTime) {
        if !seed.names_itself() {
            return;
        }
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
    ///
    /// And entries that cannot say what they are, on the same footing and for a
    /// nearer reason: [`Self::record`] stopped writing them on 2026-08-20, and a
    /// file written before that has them. A row this store cannot caption is a
    /// row a reader cannot choose between, so it leaves by the door it came in
    /// rather than being drawn blank — the load is the second door onto the one
    /// rule, not a second rule.
    #[must_use]
    pub fn from_persisted(entries: &[RecentEntryV1]) -> Self {
        Self {
            entries: entries
                .iter()
                .filter_map(|entry| {
                    let seed = Seed::from(&entry.seed);
                    seed.names_itself().then_some(())?;
                    Some(RecentEntry {
                        seed,
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
        crate::i18n::ago_just_now()
    } else if minutes < 60 {
        crate::i18n::ago_minutes(minutes)
    } else {
        crate::i18n::ago_hours(minutes / 60)
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
///
/// Shared with [`crate::git`], which reads git's own timestamps the same way and
/// for the same reason: one Gregorian calendar in this crate, not two.
pub(crate) fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
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
///
/// Shared with [`crate::git::relative_time`], which needs the same calendar for
/// the same reason this one does — a date to show, from a number of seconds, in
/// a workspace with no date-time dependency. One implementation of the Gregorian
/// calendar, not two that can disagree about a leap year.
pub(crate) fn civil_from_days(days: i64) -> (i64, i64, i64) {
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

    /// PIN — **the vault does not hold a row it cannot caption** (2026-08-20).
    ///
    /// Red gate: every one of these went in, and `RECENTLY OPENED` drew each as
    /// a line with a mark, a timestamp and no words — an offer to bring
    /// something back without saying what. `Seed::Term`'s own doc calls the
    /// empty place "the honest answer to 'where was it?' when nobody said", and
    /// it is; honest is not the same as *offerable*, and the difference is this
    /// door.
    ///
    /// Both doors, because a vault has two: what this run closes, and what an
    /// older one left on disk.
    #[test]
    fn the_vault_turns_away_a_row_that_cannot_say_what_it_is() {
        let mut vault = SeedVault::default();
        // Nothing said and nothing named: the shape that used to draw blank.
        vault.record(term("", None), Vec::new(), at(1));
        vault.record(term("", Some("")), Vec::new(), at(2));
        vault.record(
            Seed::Files {
                root: String::new(),
            },
            Vec::new(),
            at(3),
        );
        vault.record(
            Seed::Preview {
                path: String::new(),
                source: PreviewSourceV1::File,
            },
            Vec::new(),
            at(4),
        );
        vault.record(
            Seed::Window {
                seeds: vec![term("", None)],
            },
            Vec::new(),
            at(5),
        );
        assert!(vault.is_empty(), "not one of those five is a row");

        // Your own name for it is a caption, even standing nowhere — which is
        // exactly the tab an agent gets renamed into.
        vault.record(term("", Some("build")), Vec::new(), at(6));
        // And so is a place, with no name.
        vault.record(term("C:\\repo", None), Vec::new(), at(7));
        assert_eq!(vault.len(), 2);

        // The disk door says the same sentence about a file written before this
        // rule existed.
        let legacy = SeedVault {
            entries: vec![
                RecentEntry {
                    seed: term("", None),
                    previews: Vec::new(),
                    at: at(1),
                },
                RecentEntry {
                    seed: term("C:\\repo", None),
                    previews: Vec::new(),
                    at: at(2),
                },
            ],
        };
        let reloaded = SeedVault::from_persisted(&legacy.to_persisted());
        assert_eq!(
            reloaded.entries().len(),
            1,
            "the blank row leaves by the door it came in"
        );
        assert_eq!(reloaded.entries()[0].seed, term("C:\\repo", None));
    }

    /// PIN — a window is recognised by the first tab that **can** say what it
    /// is, and the tabs it skips still come back with it (2026-08-20).
    ///
    /// Red gate: `seeds.first()` and nothing else, so a window whose first tab
    /// was an anonymous shell drew a blank row for the whole window — six tabs
    /// behind a caption of nothing. Dropping that tab from the list instead
    /// would be the opposite error: a tab is skipped for the *label*, not
    /// refused for the reopening, and `recent_is_available` already reads the
    /// list on exactly those terms.
    #[test]
    fn a_window_is_named_by_the_first_tab_that_can_name_itself() {
        let window = Seed::Window {
            seeds: vec![term("", None), term("C:\\repo", None)],
        };
        assert_eq!(window.first_tab(), Some(&term("C:\\repo", None)));
        assert!(window.names_itself());
        assert_eq!(
            window.window_tabs(),
            Some(2),
            "and the count is still both of them — the tooltip says how many \
             tabs come back, not how many have captions"
        );

        let mut vault = SeedVault::default();
        vault.record(window, Vec::new(), at(1));
        assert_eq!(vault.len(), 1);
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

    /// PIN (multiwindow slice D) — **a window's key is a fourth slot, so no
    /// window can ever collide with a tab.**
    ///
    /// The three tab shapes fill three pipe-separated slots; a window opens a
    /// fourth and puts its children's own keys inside it, joined by a newline —
    /// a character no Windows path may contain, so the join needs no escape and
    /// cannot be forged by a folder name.
    ///
    /// Red gate: key a window on its first tab and closing a window would evict
    /// the Recent row for the tab that was in it, so undo-close after closing a
    /// window would offer you the window instead of the tab.
    #[test]
    fn a_closed_window_can_never_take_a_tabs_row() {
        let window = Seed::Window {
            seeds: vec![
                term("C:\\repo", None),
                Seed::Files {
                    root: "C:\\docs".to_owned(),
                },
            ],
        };
        assert_eq!(window.recent_key(), "|||pwsh|C:\\repo|\n|C:\\docs|");
        assert_ne!(window.recent_key(), term("C:\\repo", None).recent_key());
        assert!(
            !window.can_be_named(),
            "a window has no tab head to type into"
        );
        assert_eq!(window.window_tabs(), Some(2));
        assert_eq!(window.first_tab(), Some(&term("C:\\repo", None)));

        // And two windows holding different places are two rows, on the same
        // dedup rule every other shape follows.
        let mut vault = SeedVault::default();
        vault.record(window.clone(), Vec::new(), at(0));
        vault.record(
            Seed::Window {
                seeds: vec![term("C:\\other", None)],
            },
            Vec::new(),
            at(60),
        );
        assert_eq!(vault.len(), 2);
        vault.record(window, Vec::new(), at(120));
        assert_eq!(vault.len(), 2, "the same window closed twice is one row");
    }

    /// PIN (multiwindow slice D) — **a closed window survives the disk, tabs and
    /// all.**
    ///
    /// The vault is written into `session.json`, so a shape the wire cannot carry
    /// is a row that disappears at the next restart — which for this shape means
    /// the one gesture that throws away six tabs at once quietly loses its way
    /// back overnight.
    #[test]
    fn a_window_row_round_trips_through_the_wire() {
        let mut vault = SeedVault::default();
        vault.record(
            Seed::Window {
                seeds: vec![
                    term("C:\\repo", Some("build")),
                    Seed::Preview {
                        path: "C:\\repo\\README.md".to_owned(),
                        source: PreviewSourceV1::File,
                    },
                ],
            },
            Vec::new(),
            at(0),
        );
        let persisted = vault.to_persisted();
        assert_eq!(
            persisted[0].seed,
            RecentSeedV1::Window {
                seeds: vec![
                    RecentSeedV1::Term {
                        profile_id: "pwsh".to_owned(),
                        cwd: "C:\\repo".to_owned(),
                        manual_name: Some("build".to_owned()),
                    },
                    RecentSeedV1::Preview {
                        path: "C:\\repo\\README.md".to_owned(),
                        source: PreviewSourceV1::File,
                    },
                ],
            }
        );
        assert_eq!(SeedVault::from_persisted(&persisted), vault);
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
            RecentPreviewV1::File(r"C:\repo\README.md".to_owned()),
            RecentPreviewV1::File(r"C:\repo\src\main.rs".to_owned()),
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
            vec![RecentPreviewV1::File(r"C:\repo\a.md".to_owned())],
            at(0),
        );
        vault.record(
            term(r"C:\repo", None),
            vec![RecentPreviewV1::File(r"C:\repo\b.md".to_owned())],
            at(60),
        );
        assert_eq!(vault.len(), 1, "still one place");
        assert_eq!(
            vault.entries()[0].previews,
            vec![RecentPreviewV1::File(r"C:\repo\b.md".to_owned())]
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

    /// **A closed page is a row in this vault, and it is not the row a file
    /// would be** (W2 slice ③, user ruling 2026-08-22).
    ///
    /// The reason the seed was extended at all is in the ruling's own sentence:
    /// without it, closing a web tab would be the one close in this window with
    /// no way back. So the shape has to survive the wire *and* keep its own
    /// dedup key — a page and a file spelled the same string are two rows, and
    /// the third slot is where that is said.
    ///
    /// Red gate: give the page arm of `recent_key` the file's `||{path}` and the
    /// two seeds collide, so pinning one evicts the other.
    #[test]
    fn a_closed_page_is_its_own_row_in_the_vault_and_survives_the_wire() {
        let page = Seed::Preview {
            path: "http://localhost:5173/app?tab=logs#top".to_owned(),
            source: PreviewSourceV1::Url,
        };
        let file = Seed::Preview {
            path: "http://localhost:5173/app?tab=logs#top".to_owned(),
            source: PreviewSourceV1::File,
        };
        assert_ne!(
            page.recent_key(),
            file.recent_key(),
            "a page and a file that happen to be spelled alike are two places"
        );
        assert_eq!(
            file.recent_key(),
            "||http://localhost:5173/app?tab=logs#top",
            "and the file's key is the one every vault on every disk already holds"
        );
        assert!(page.names_itself() && !page.can_be_named());
        assert_eq!(page.window_tabs(), None);

        let mut vault = SeedVault::default();
        vault.record(
            page.clone(),
            vec![RecentPreviewV1::Page {
                url: "http://localhost:5173/app?tab=logs#top".to_owned(),
            }],
            at(0),
        );
        vault.record(file.clone(), Vec::new(), at(60));
        assert_eq!(vault.len(), 2, "two rows, not one");
        let reloaded = SeedVault::from_persisted(&vault.to_persisted());
        assert_eq!(
            reloaded, vault,
            "and both come back through the wire unchanged, which is what the \
             schema v11 field bought"
        );
        assert_eq!(
            reloaded.entries()[1].previews,
            vec![RecentPreviewV1::Page {
                url: "http://localhost:5173/app?tab=logs#top".to_owned()
            }],
            "the page the tab was on rides with it, as a file's path always has"
        );
    }
}
