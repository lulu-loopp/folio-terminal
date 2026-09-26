//! Process-wide file-content accounting. No locks, I/O, formatting or allocation
//! in `Ledger::add`. One collector rotates banks; producers never wait for it.
//! Bytes are bytes returned by a reader, including partial reads before errors.
//! Reads mean logical read passes (a head/tail probe also counts as a pass), not
//! kernel operations. Streams charge bytes as they arrive, and start a new pass
//! after a seek. Native loaders whose reads are opaque are reported separately.

use std::fs::{File, Metadata};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};

pub const BUDGET: u64 = 50_000_000;
const SLOTS: usize = 64;
const NAME_BYTES: usize = 96;
const BUSY: u64 = u64::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Lane {
    InlineImage,
    Peek,
    Animation,
    Preview,
    Pdf,
    GitPipe,
    Settings,
    Fonts,
    Attention,
    /// The install marker and the package manager's receipt beside the
    /// executable, read once at start (`install_channel`, ticket U-1).
    Install,
    /// The downloaded release archive the updater reads, and the manifest
    /// read out of the new `folio.exe` in it (`update_archive`, ticket U-14).
    Update,
    /// The update journal's header and phase, read again and again by a trial's
    /// watch until its transaction is decided (`update_trial`, ticket U-13).
    UpdateJournal,
    Other,
}

impl Lane {
    pub const ALL: [Self; 13] = [
        Self::InlineImage,
        Self::Peek,
        Self::Animation,
        Self::Preview,
        Self::Pdf,
        Self::GitPipe,
        Self::Settings,
        Self::Fonts,
        Self::Attention,
        Self::Install,
        Self::Update,
        Self::UpdateJournal,
        Self::Other,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::InlineImage => "inline_image",
            Self::Peek => "peek",
            Self::Animation => "animation",
            Self::Preview => "preview",
            Self::Pdf => "pdf",
            Self::GitPipe => "git_pipe",
            Self::Settings => "settings",
            Self::Fonts => "fonts",
            Self::Attention => "attention",
            Self::Install => "install",
            Self::Update => "update",
            Self::UpdateJournal => "update_journal",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub bytes: u64,
    pub reads: u64,
    pub opaque_loads: u64,
}

struct Slot {
    key: AtomicU64,
    reads: AtomicU64,
    name: [AtomicU8; NAME_BYTES],
}

impl Slot {
    const fn new() -> Self {
        Self {
            key: AtomicU64::new(0),
            reads: AtomicU64::new(0),
            name: [const { AtomicU8::new(0) }; NAME_BYTES],
        }
    }
}

struct LaneBank {
    bytes: AtomicU64,
    reads: AtomicU64,
    omitted: AtomicU64,
    opaque_loads: AtomicU64,
    slots: [Slot; SLOTS],
}

impl LaneBank {
    const fn new() -> Self {
        Self {
            bytes: AtomicU64::new(0),
            reads: AtomicU64::new(0),
            omitted: AtomicU64::new(0),
            opaque_loads: AtomicU64::new(0),
            slots: [const { Slot::new() }; SLOTS],
        }
    }

    fn path(&self, path: &Path, reads: u64) {
        let raw = path.as_os_str().as_encoded_bytes();
        let key = raw.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
        });
        let key = key.clamp(1, BUSY - 1);
        for offset in 0..SLOTS {
            let slot = &self.slots[(key as usize).wrapping_add(offset) % SLOTS];
            let held = slot.key.load(Ordering::Acquire);
            if held == key {
                slot.reads.fetch_add(reads, Ordering::Relaxed);
                return;
            }
            if held == 0
                && slot
                    .key
                    .compare_exchange(0, BUSY, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
            {
                // Split both spellings even when a Windows path is seen on Unix.
                // Only the basename is ever retained, and control bytes cannot
                // forge a diagnostics line. UTF-8 truncation is repaired at print.
                let basename = raw
                    .rsplit(|byte| *byte == b'/' || *byte == b'\\')
                    .next()
                    .unwrap_or(b"");
                for (cell, byte) in slot.name.iter().zip(basename.iter().copied()) {
                    cell.store(
                        if byte.is_ascii_control() { b'?' } else { byte },
                        Ordering::Relaxed,
                    );
                }
                slot.reads.store(reads, Ordering::Relaxed);
                slot.key.store(key, Ordering::Release);
                return;
            }
        }
        // Totals remain exact. A bounded table cannot name every distinct file;
        // report this explicitly, never imply the top three are exhaustive.
        self.omitted.fetch_add(reads, Ordering::Relaxed);
    }
}

struct Bank {
    users: AtomicUsize,
    lanes: [LaneBank; Lane::ALL.len()],
}

impl Bank {
    const fn new() -> Self {
        Self {
            users: AtomicUsize::new(0),
            lanes: [const { LaneBank::new() }; Lane::ALL.len()],
        }
    }
}

pub struct Ledger {
    epoch: AtomicU64,
    pending: AtomicU64,
    changed: AtomicBool,
    banks: [Bank; 2],
}

impl Default for Ledger {
    fn default() -> Self {
        Self::new()
    }
}

impl Ledger {
    pub const fn new() -> Self {
        Self {
            epoch: AtomicU64::new(0),
            pending: AtomicU64::new(BUSY),
            changed: AtomicBool::new(false),
            banks: [const { Bank::new() }; 2],
        }
    }

    /// Two lane additions, two bank-reference additions, two epoch loads and
    /// one activity store. Path accounting is bounded by SLOTS probes, hashes
    /// the supplied path and copies at most NAME_BYTES only when claiming a slot.
    /// A producer racing the once-per-minute rotation retries the bank choice;
    /// it never waits for another producer, the collector, or a lock.
    pub fn add(&self, lane: Lane, bytes: u64, reads: u64, path: Option<&Path>) {
        self.add_event(lane, bytes, reads, 0, path);
    }

    fn add_event(&self, lane: Lane, bytes: u64, reads: u64, opaque: u64, path: Option<&Path>) {
        if bytes == 0 && reads == 0 && opaque == 0 {
            return;
        }
        loop {
            // SeqCst closes the enter/rotate store-load race: either the
            // collector sees our reference or we see its new epoch.
            let epoch = self.epoch.load(Ordering::SeqCst);
            let bank = &self.banks[epoch as usize % 2];
            bank.users.fetch_add(1, Ordering::SeqCst);
            if self.epoch.load(Ordering::SeqCst) != epoch {
                bank.users.fetch_sub(1, Ordering::SeqCst);
                continue;
            }
            let lane = &bank.lanes[lane as usize];
            lane.bytes.fetch_add(bytes, Ordering::Relaxed);
            lane.reads.fetch_add(reads, Ordering::Relaxed);
            if opaque > 0 {
                lane.opaque_loads.fetch_add(opaque, Ordering::Relaxed);
            }
            if reads > 0
                && let Some(path) = path
            {
                lane.path(path, reads);
            }
            self.changed.store(true, Ordering::Release);
            bank.users.fetch_sub(1, Ordering::SeqCst);
            return;
        }
    }

    pub fn changed(&self) -> bool {
        self.changed.load(Ordering::Acquire)
    }

    /// Single collector only. A reader in flight delays collection, never the
    /// rotation or any producer. The collector retries on its existing wake.
    pub fn rotate(&self) -> Option<Minute> {
        let mut previous = self.pending.load(Ordering::Relaxed);
        if previous == BUSY {
            self.changed.store(false, Ordering::Release);
            previous = self.epoch.fetch_add(1, Ordering::SeqCst);
            self.pending.store(previous, Ordering::Relaxed);
        }
        let bank = &self.banks[previous as usize % 2];
        if bank.users.load(Ordering::SeqCst) != 0 {
            return None;
        }
        let mut minute = Minute::default();
        for (index, lane) in bank.lanes.iter().enumerate() {
            minute.lanes[index] = Totals {
                bytes: lane.bytes.swap(0, Ordering::Relaxed),
                reads: lane.reads.swap(0, Ordering::Relaxed),
                opaque_loads: lane.opaque_loads.swap(0, Ordering::Relaxed),
            };
            minute.omitted[index] = lane.omitted.swap(0, Ordering::Relaxed);
            let mut paths: Vec<(u64, String, u64)> = Vec::new();
            for slot in &lane.slots {
                let key = slot.key.swap(0, Ordering::Relaxed);
                if key == 0 {
                    continue;
                }
                let bytes = slot
                    .name
                    .each_ref()
                    .map(|byte| byte.swap(0, Ordering::Relaxed));
                let end = bytes
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(NAME_BYTES);
                let count = slot.reads.swap(0, Ordering::Relaxed);
                if let Some(held) = paths.iter_mut().find(|held| held.0 == key) {
                    held.2 += count;
                } else {
                    paths.push((
                        key,
                        String::from_utf8_lossy(&bytes[..end]).into_owned(),
                        count,
                    ));
                }
            }
            paths.sort_unstable_by(|a, b| b.2.cmp(&a.2).then_with(|| a.1.cmp(&b.1)));
            minute.top[index] = paths
                .into_iter()
                .take(3)
                .map(|(_, name, count)| (name, count))
                .collect();
        }
        self.pending.store(BUSY, Ordering::Relaxed);
        Some(minute)
    }
}

pub static LEDGER: Ledger = Ledger::new();
/// Name a delegated loader without claiming its file length was read. The
/// library/native API exposes neither byte counts nor its private cache hits.
pub fn opaque<T>(lane: Lane, work: impl FnOnce() -> T) -> T {
    LEDGER.add_event(lane, 0, 0, 1, None);
    work()
}
/// Output collected by std's child-process implementation, attributed here
/// without changing its concurrent pipe draining or wait behavior.
pub fn pipe_output(lane: Lane, output: &std::process::Output) {
    LEDGER.add(
        lane,
        (output.stdout.len() + output.stderr.len()) as u64,
        2,
        None,
    );
}
static INPUT: AtomicBool = AtomicBool::new(false);
pub fn input() {
    INPUT.store(true, Ordering::Release);
}
pub fn take_input() -> bool {
    INPUT.swap(false, Ordering::AcqRel)
}

pub struct Minute {
    pub lanes: [Totals; Lane::ALL.len()],
    pub top: [Vec<(String, u64)>; Lane::ALL.len()],
    pub omitted: [u64; Lane::ALL.len()],
    pub elapsed_ms: u64,
}

impl Default for Minute {
    fn default() -> Self {
        Self {
            lanes: [Totals::default(); Lane::ALL.len()],
            top: std::array::from_fn(|_| Vec::new()),
            omitted: [0; Lane::ALL.len()],
            elapsed_ms: 60_000,
        }
    }
}

impl Minute {
    pub fn total_bytes(&self) -> u64 {
        self.lanes.iter().map(|lane| lane.bytes).sum()
    }

    pub fn line(&self, age_minutes: u64, input_seen: bool) -> String {
        use std::fmt::Write;
        let top_lane = self
            .lanes
            .iter()
            .enumerate()
            .max_by_key(|(_, lane)| lane.bytes)
            .map(|(i, _)| i);
        let mut line = format!(
            "Folio: file reads {:.3} MB/min {} — ",
            self.total_bytes() as f64 / 1_000_000.0 * 60_000.0 / self.elapsed_ms.max(1) as f64,
            if input_seen {
                "with input"
            } else {
                "with no input"
            }
        );
        for (index, lane) in Lane::ALL.iter().enumerate() {
            if index > 0 {
                line.push_str(", ");
            }
            let totals = self.lanes[index];
            let _ = write!(
                line,
                "{} {:.3} MB in {} reads",
                lane.label(),
                totals.bytes as f64 / 1_000_000.0,
                totals.reads
            );
            if totals.opaque_loads > 0 {
                let _ = write!(
                    line,
                    " + {} opaque loads (bytes unknown)",
                    totals.opaque_loads
                );
            }
            if top_lane == Some(index) && !self.top[index].is_empty() {
                line.push_str(if self.omitted[index] > 0 {
                    " (top tracked: "
                } else {
                    " (top: "
                });
                for (i, (name, count)) in self.top[index].iter().enumerate() {
                    if i > 0 {
                        line.push_str(", ");
                    }
                    let _ = write!(line, "{name} ×{count}");
                }
                line.push(')');
            }
            if self.omitted[index] > 0 {
                let _ = write!(line, " [untracked paths: {} reads]", self.omitted[index]);
            }
        }
        let _ = write!(line, " · session age {age_minutes} min");
        if self.elapsed_ms != 60_000 {
            let _ = write!(line, " · window {} ms", self.elapsed_ms);
        }
        line
    }

    pub fn perf_line(&self, age_minutes: u64, input_seen: bool) -> String {
        use std::fmt::Write;
        let mut line = format!(
            "BT_PERF_TRACE file_reads minute={age_minutes} window_ms={} input_seen={input_seen}",
            self.elapsed_ms
        );
        for (lane, totals) in Lane::ALL.iter().zip(self.lanes) {
            let _ = write!(
                line,
                " {}_bytes={} {}_reads={}",
                lane.label(),
                totals.bytes,
                lane.label(),
                totals.reads
            );
            let _ = write!(
                line,
                " {}_opaque_loads={}",
                lane.label(),
                totals.opaque_loads
            );
        }
        line
    }
}

pub fn over_budget(minute: &Minute, input_seen: bool) -> bool {
    let exceeds = |bytes| {
        u128::from(bytes) * 60_000 > u128::from(BUDGET) * u128::from(minute.elapsed_ms.max(1))
    };
    (!input_seen && exceeds(minute.total_bytes()))
        || minute.lanes.iter().any(|lane| exceeds(lane.bytes))
}

#[derive(Default)]
pub struct Reporter {
    episode: Option<Episode>,
}
struct Episode {
    elapsed_ms: u64,
    last: u64,
    bytes: u64,
}
impl Reporter {
    pub fn active(&self) -> bool {
        self.episode.is_some()
    }

    pub fn observe(
        &mut self,
        end_minute: u64,
        minute: &Minute,
        input_seen: bool,
    ) -> Option<String> {
        if over_budget(minute, input_seen) {
            let first = self.episode.is_none();
            let episode = self.episode.get_or_insert(Episode {
                elapsed_ms: 0,
                last: end_minute.saturating_sub(10),
                bytes: 0,
            });
            episode.elapsed_ms += minute.elapsed_ms;
            episode.bytes += minute.total_bytes();
            if first || end_minute.saturating_sub(episode.last) >= 10 {
                episode.last = end_minute;
                return Some(minute.line(end_minute, input_seen));
            }
        } else if let Some(episode) = self.episode.take() {
            return Some(format!(
                "Folio: file reads ended after {} min, {:.3} GB · session age {end_minute} min",
                episode.elapsed_ms / 60_000,
                episode.bytes as f64 / 1_000_000_000.0
            ));
        }
        None
    }
}

/// A transparent reader adapter. It forwards the original Read implementation
/// (including read_to_end's allocation strategy), counts partial-error bytes,
/// and never holds a ledger bank across I/O. One open or seek starts one pass.
pub struct Reader<'a, R> {
    inner: R,
    lane: Lane,
    path: Option<&'a Path>,
    ledger: &'a Ledger,
    started: bool,
}
impl<'a, R> Reader<'a, R> {
    pub fn new(inner: R, lane: Lane, path: Option<&'a Path>) -> Self {
        Self::with_ledger(inner, lane, path, &LEDGER)
    }
    fn with_ledger(inner: R, lane: Lane, path: Option<&'a Path>, ledger: &'a Ledger) -> Self {
        Self {
            inner,
            lane,
            path,
            ledger,
            started: false,
        }
    }
    fn counted(&mut self, bytes: usize) {
        self.ledger
            .add(self.lane, bytes as u64, u64::from(!self.started), self.path);
        self.started = true;
    }
}
impl<R: Read> Read for Reader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let result = self.inner.read(buffer);
        self.counted(result.as_ref().copied().unwrap_or(0));
        result
    }
    fn read_to_end(&mut self, buffer: &mut Vec<u8>) -> io::Result<usize> {
        let before = buffer.len();
        let result = self.inner.read_to_end(buffer);
        self.counted(buffer.len() - before);
        result
    }
}
impl<R: Seek> Seek for Reader<'_, R> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let result = self.inner.seek(from);
        if result.is_ok() {
            self.started = false;
        }
        result
    }
}
impl Reader<'_, File> {
    pub fn metadata(&self) -> io::Result<Metadata> {
        self.inner.metadata()
    }
}

pub fn open(lane: Lane, path: &Path) -> io::Result<Reader<'_, File>> {
    File::open(path).map(|file| Reader::new(file, lane, Some(path)))
}

/// Keep std's whole-file read and its allocation strategy unchanged.
pub fn read(lane: Lane, path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    let path = path.as_ref();
    let result = std::fs::read(path);
    if let Ok(bytes) = &result {
        LEDGER.add(lane, bytes.len() as u64, 1, Some(path));
    }
    result
}
pub fn read_to_string(lane: Lane, path: impl AsRef<Path>) -> io::Result<String> {
    let path = path.as_ref();
    // Read bytes so invalid UTF-8 still contributes its consumed bytes.
    let bytes = read(lane, path)?;
    String::from_utf8(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        )
    })
}

#[cfg(test)]
#[path = "file_reads_tests.rs"]
mod tests;
