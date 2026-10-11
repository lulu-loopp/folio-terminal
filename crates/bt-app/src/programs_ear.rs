//! The shallow directory ear for the program walk's exact `PATH` snapshot.
//!
//! Each eligible directory is a steward member. Watcher threads keep only a timestamp after the
//! pure `PATHEXT` name filter, and the application turns those timestamps through `WatchClock`.
//! No directory is read here and silence schedules no work.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use bt_platform::DirChange;
use winit::event_loop::EventLoopProxy;

use crate::AppEvent;
use crate::programs_lane::ProgramEnvironmentSnapshot;
use crate::watch_clock::WatchClock;
use crate::watch_steward::{Depth, EarHandle, Listener, MemberReport};

struct Watched {
    heard: bool,
    clock: WatchClock,
}

/// Every directory in the last completed program walk's eligible PATH set.
#[derive(Default)]
pub struct ProgramsEar {
    news: Arc<Mutex<BTreeMap<PathBuf, Instant>>>,
    extensions: Arc<Mutex<Vec<OsString>>>,
    watched: BTreeMap<PathBuf, Watched>,
    ear: Option<EarHandle>,
}

impl ProgramsEar {
    /// Bring the steward's wanted set level with a completed walk and retry members whose last
    /// start failed. The steward owns every start and stop; this method only posts its request.
    pub fn sync(
        &mut self,
        snapshot: &ProgramEnvironmentSnapshot,
        proxy: &EventLoopProxy<AppEvent>,
    ) {
        let news = Arc::clone(&self.news);
        let extensions = Arc::clone(&self.extensions);
        let proxy = proxy.clone();
        self.sync_with(snapshot, Instant::now(), move || {
            crate::watch_steward::open_ear(
                Depth::ShallowNamed,
                listener(&news, &extensions, &proxy),
            )
        });
    }

    fn sync_with(
        &mut self,
        snapshot: &ProgramEnvironmentSnapshot,
        now: Instant,
        make_ear: impl FnOnce() -> EarHandle,
    ) {
        *lock(&self.extensions) = extensions(snapshot.pathext.as_deref());
        let wanted: BTreeSet<PathBuf> = snapshot.directories.iter().cloned().collect();
        let rearm = self
            .watched
            .iter()
            .filter(|(_, watched)| !watched.heard)
            .map(|(path, _)| path.clone())
            .collect();
        self.watched.retain(|path, _| wanted.contains(path));
        for path in &wanted {
            self.watched.entry(path.clone()).or_insert(Watched {
                heard: false,
                clock: WatchClock::default(),
            });
        }
        lock(&self.news).retain(|path, _| wanted.contains(path));
        if !wanted.is_empty() && self.ear.is_none() {
            self.ear = Some(make_ear());
        }
        if let Some(ear) = self.ear.as_mut() {
            ear.want(&wanted, rearm);
        }
        self.fold_reports(now);
    }

    /// Fold watcher/steward news and answer whether one coalesced program walk is due now.
    pub fn advance(&mut self, now: Instant) -> bool {
        self.fold_reports(now);
        for (path, at) in std::mem::take(&mut *lock(&self.news)) {
            if let Some(watched) = self.watched.get_mut(&path) {
                watched.clock.note_event(at);
            }
        }
        self.watched
            .values_mut()
            .any(|watched| watched.clock.take_due(now))
    }

    /// The next wake owed by event-driven PATH news, or none in silence.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.watched
            .values()
            .filter_map(|watched| watched.clock.due_at())
            .min()
    }

    fn fold_reports(&mut self, now: Instant) {
        let Some(ear) = self.ear.as_ref() else {
            return;
        };
        for (path, report) in ear.take_reports() {
            let Some(watched) = self.watched.get_mut(&path) else {
                continue;
            };
            match report.report {
                MemberReport::Armed => {
                    if !watched.heard {
                        watched.heard = true;
                        watched.clock.note_event(now);
                    }
                }
                MemberReport::Failed(_) => watched.heard = false,
            }
        }
    }
}

fn listener(
    news: &Arc<Mutex<BTreeMap<PathBuf, Instant>>>,
    extensions: &Arc<Mutex<Vec<OsString>>>,
    proxy: &EventLoopProxy<AppEvent>,
) -> Listener {
    let news = Arc::clone(news);
    let extensions = Arc::clone(extensions);
    let proxy = proxy.clone();
    Arc::new(move |directory, change| {
        if executable_change(change, &lock(&extensions)) {
            lock(&news).insert(directory.to_path_buf(), Instant::now());
            let _ = proxy.send_event(AppEvent::PathDirectoryChanged);
        }
    })
}

fn extensions(value: Option<&OsStr>) -> Vec<OsString> {
    value
        .map(|value| value.to_string_lossy())
        .unwrap_or_default()
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(|extension| {
            if extension.starts_with('.') {
                OsString::from(extension)
            } else {
                OsString::from(format!(".{extension}"))
            }
        })
        .collect()
}

/// Whether a named shallow change could alter the executable search result. Unknown is always
/// admitted: it means the kernel overflowed precisely when dropping it would be unsafe.
fn executable_change(change: DirChange<'_>, extensions: &[OsString]) -> bool {
    match change {
        DirChange::Unknown => true,
        DirChange::Named(names) => names.iter().any(|name| {
            let extension = Path::new(name).extension();
            extensions.iter().any(|wanted| {
                extension.is_some_and(|extension| {
                    let wanted = wanted.to_string_lossy();
                    extension
                        .to_string_lossy()
                        .eq_ignore_ascii_case(wanted.trim_start_matches('.'))
                })
            })
        }),
        DirChange::Ended => false,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lane::wait_for_a_wake;
    use crate::watch_clock::WATCH_QUIET;
    use crate::watch_steward::Steward;
    use crate::watch_steward::fake::FakeDoor;
    use std::sync::mpsc;

    fn snapshot(directories: &[&str]) -> ProgramEnvironmentSnapshot {
        ProgramEnvironmentSnapshot {
            path: Some(std::env::join_paths(directories).unwrap()),
            pathext: Some(".COM;.EXE;.CMD".into()),
            directories: directories.iter().map(PathBuf::from).collect(),
        }
    }

    struct Rig {
        ear: ProgramsEar,
        steward: &'static Steward,
        wakes: mpsc::Receiver<()>,
    }

    impl Rig {
        fn over(door: FakeDoor) -> Self {
            let (wake, wakes) = mpsc::channel();
            let steward = Box::leak(Box::new(Steward::new(door, |_| {})));
            steward.install_wake(move || {
                let _ = wake.send(());
            });
            Self {
                ear: ProgramsEar::default(),
                steward,
                wakes,
            }
        }

        fn sync(&mut self, snapshot: &ProgramEnvironmentSnapshot, now: Instant) {
            let steward = self.steward;
            self.ear.sync_with(snapshot, now, || {
                steward.open_ear(Depth::ShallowNamed, Arc::new(|_, _| {}))
            });
        }

        fn after_wake(&mut self, now: Instant, what: &str) {
            wait_for_a_wake(&self.wakes, what);
            let _ = self.ear.advance(now);
        }
    }

    /// RED (T-OS-SIGNALS-WIN) — executable suffixes use PATHEXT without case, siblings and bare
    /// names do not wake, while overflow is conservatively news.
    ///
    /// MUTATIONS: compare case-sensitively; admit every named entry; reject `Unknown`.
    #[test]
    fn only_pathext_names_and_unknown_are_news() {
        let wanted = extensions(Some(OsStr::new(".EXE;.Cmd")));
        assert!(executable_change(
            DirChange::Named(&[OsString::from("foo.exe")]),
            &wanted
        ));
        assert!(executable_change(
            DirChange::Named(&[OsString::from("foo.CMD")]),
            &wanted
        ));
        assert!(!executable_change(
            DirChange::Named(&[OsString::from("foo.txt")]),
            &wanted
        ));
        assert!(!executable_change(
            DirChange::Named(&[OsString::from("foo")]),
            &wanted
        ));
        assert!(executable_change(DirChange::Unknown, &wanted));
    }

    /// RED (T-OS-SIGNALS-WIN) — add/remove changes the steward set exactly.
    /// MUTATION: retain an old member or omit a new one; `live` differs.
    #[test]
    fn the_watched_set_follows_each_walk_snapshot() {
        let door = FakeDoor::default();
        let mut rig = Rig::over(door.clone());
        let now = Instant::now();
        let first = snapshot(&[r"C:\Tools", r"C:\Users\alice\bin"]);
        rig.sync(&first, now);
        rig.after_wake(now, "the first PATH set");
        assert_eq!(door.live(), first.directories);

        let second = snapshot(&[r"C:\Users\alice\bin", r"D:\SDK\bin"]);
        rig.sync(&second, now);
        rig.after_wake(now, "the changed PATH set");
        door.wait_for_drops(1);
        assert_eq!(door.live(), second.directories);
    }

    /// RED (T-OS-SIGNALS-WIN) — a failed member is retried by the next completed walk.
    /// MUTATION: send an empty `rearm` set; the attempt count stays one.
    #[test]
    fn a_failed_directory_is_rearmed_by_the_next_walk_landing() {
        let door = FakeDoor::default();
        let mut rig = Rig::over(door.clone());
        let now = Instant::now();
        let path = PathBuf::from(r"C:\Tools");
        let wanted = snapshot(&[r"C:\Tools"]);
        door.refuse(&path);
        rig.sync(&wanted, now);
        rig.after_wake(now, "the refusal");
        assert_eq!(door.attempts(&path), 1);

        door.admit(&path);
        rig.sync(&wanted, now);
        rig.after_wake(now, "the rearm");
        assert_eq!(door.attempts(&path), 2);
        assert_eq!(door.live(), vec![path]);
    }

    /// RED (T-OS-SIGNALS-WIN) — a held start cannot hold the caller; it is in progress only on
    /// the steward worker. MUTATION: start through the door in `sync_with`; this call blocks.
    #[test]
    fn a_held_start_never_runs_on_the_window_thread() {
        let door = FakeDoor::default();
        door.hold_starts();
        let mut rig = Rig::over(door.clone());
        rig.sync(&snapshot(&[r"C:\Tools"]), Instant::now());
        door.wait_for_a_held_start();
        assert_eq!(door.in_progress(), (1, 0));
        assert_eq!(door.threads().0, vec!["watch-steward"]);
        door.release();
    }

    /// RED (T-OS-SIGNALS-WIN) — two hundred notifications are one due walk after quiet.
    /// MUTATION: count notifications rather than feeding one `WatchClock`; this becomes 200.
    #[test]
    fn two_hundred_events_coalesce_to_one_walk() {
        let start = Instant::now();
        let path = PathBuf::from(r"C:\Tools");
        let mut ear = ProgramsEar::default();
        ear.watched.insert(
            path.clone(),
            Watched {
                heard: true,
                clock: WatchClock::default(),
            },
        );
        for step in 0..200 {
            lock(&ear.news).insert(path.clone(), start + std::time::Duration::from_micros(step));
            assert!(!ear.advance(start + std::time::Duration::from_micros(step)));
        }
        let due = ear.deadline().expect("the burst owes one walk");
        assert_eq!(
            due,
            start + std::time::Duration::from_micros(199) + WATCH_QUIET
        );
        assert!(ear.advance(due));
        assert!(!ear.advance(due));
        assert_eq!(ear.deadline(), None);
    }
}
