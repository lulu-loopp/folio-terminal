//! **Every diagnostic door, read once, at the program's entry**
//! (T-DIAGNOSTIC-DOORS-AT-THE-ENTRY).
//!
//! The recording and trace switches — `BT_PTY_DUMP`, `BT_PTY_INPUT_DUMP`, every `BT_*_TRACE`
//! and the dumps beside them — are the person's shell speaking to this run. They used to be read
//! by the code that writes them, at the moment of use: `bt_pty`'s spawn, `trace`'s gates, the
//! renderer's frame. That code is library code, and a test binary runs it as readily as the
//! program, so a shell that had `BT_PTY_DUMP` set for a diagnosis made every test that started a
//! real pane record into the diagnosis's file. The doors are a system boundary, and the boundary
//! is here: [`EntryDoors::at_the_entry`] is called by `main`, once, before anything can reach a station,
//! and [`EntryDoors::install`] hands each library its answer. Nothing below reads the
//! environment again — the shell cannot change it under a running process anyway — and a process
//! whose entry is not `main`, every test binary, installs nothing and honours no door. A test that
//! wants a recording or a trace names a file of its own and hands it over directly.
//!
//! **Set-but-empty is off** ([`diagnostics::named_file`], [`diagnostics::switched_on`]): a shell
//! that writes `BT_PTY_DUMP=` has said "not this run", and that is the word every door here is
//! read with. Whitespace is not trimmed: `" "` is a filename.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::{diagnostics, trace};

/// The raw value of every door [`EntryDoors`] reads, as the environment gave it — the one input
/// [`EntryDoors::from_values`] turns into answers, so a test can make that decision without
/// touching the process's environment.
#[derive(Clone, Debug, Default)]
pub(crate) struct DoorValues {
    pub(crate) pty_dump: Option<OsString>,
    pub(crate) pty_input_dump: Option<OsString>,
    pub(crate) mouse_trace: Option<OsString>,
    pub(crate) card_trace: Option<OsString>,
    pub(crate) attention_trace: Option<OsString>,
    pub(crate) preview_trace: Option<OsString>,
    pub(crate) web_trace: Option<OsString>,
    pub(crate) glyph_census: Option<OsString>,
    pub(crate) ime_trace: Option<OsString>,
    pub(crate) focus_thumb_dump: Option<OsString>,
    pub(crate) chrome_dump: Option<OsString>,
    pub(crate) hotkey_trace: Option<OsString>,
    pub(crate) decor_trace: Option<OsString>,
    pub(crate) semantic_trace: Option<OsString>,
    pub(crate) perf_trace: Option<OsString>,
    pub(crate) startup_trace: Option<OsString>,
    pub(crate) resize_trace: Option<OsString>,
    pub(crate) layout_events: Option<OsString>,
    pub(crate) git_trace: Option<OsString>,
    /// Every variable of the environment whose name begins `BT_` (in any case), with its value —
    /// the two rules stated as a shape rather than a list read these
    /// ([`diagnostics::console_was_asked_for`], `trace_sink`'s sink).
    pub(crate) named: Vec<(OsString, OsString)>,
}

/// **What this run's entry was asked for**, one answer per diagnostic door.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct EntryDoors {
    /// `BT_PTY_DUMP` and `BT_PTY_INPUT_DUMP`: which streams of every pane this run makes are
    /// recorded. A pane born at any time takes this same answer (`create_leaf_session`).
    pub(crate) recording: bt_pty::PtyRecording,
    /// The files `trace`'s gates and dumps write.
    pub(crate) traces: trace::TraceFiles,
    /// `BT_CHROME_DUMP`.
    pub(crate) chrome_dump: Option<PathBuf>,
    /// `BT_HOTKEY_TRACE`, written by `bt_platform::hotkey`.
    pub(crate) hotkey_trace: Option<PathBuf>,
    /// `BT_SEMANTIC_TRACE` and `BT_DECOR_TRACE`, written by `bt-term`.
    pub(crate) term: bt_term::TermDiagnostics,
    /// `BT_PERF_TRACE`, here and in the three crates under this one (`bt_viewport::trace`).
    pub(crate) perf: bool,
    /// `BT_STARTUP_TRACE`.
    pub(crate) startup: bool,
    /// `BT_RESIZE_TRACE`.
    pub(crate) resize: bool,
    /// `BT_LAYOUT_EVENTS`.
    pub(crate) layout_events: bool,
    /// `BT_GIT_TRACE`.
    pub(crate) git: bool,
    /// The name of every `BT_` variable the environment held, whatever its value.
    pub(crate) named: Vec<OsString>,
    /// The name of every `BT_` variable the environment held with a value that is not empty.
    pub(crate) named_with_a_value: Vec<OsString>,
}

/// [`EntryDoors`] with every door shut — what a process whose entry installed nothing reads.
static SHUT: EntryDoors = EntryDoors {
    recording: bt_pty::PtyRecording::none(),
    traces: trace::TraceFiles {
        mouse: None,
        card: None,
        attention: None,
        preview: None,
        web: None,
        glyph_census: None,
        ime: None,
        focus_thumb: None,
    },
    chrome_dump: None,
    hotkey_trace: None,
    term: bt_term::TermDiagnostics {
        semantic_trace: false,
        decor_trace: None,
    },
    perf: false,
    startup: false,
    resize: false,
    layout_events: false,
    git: false,
    named: Vec::new(),
    named_with_a_value: Vec::new(),
};

static INSTALLED: OnceLock<EntryDoors> = OnceLock::new();

impl EntryDoors {
    /// **The one reader of the diagnostic doors.** Called by `main` and by nothing else.
    pub(crate) fn at_the_entry() -> Self {
        Self::from_values(DoorValues {
            pty_dump: std::env::var_os("BT_PTY_DUMP"),
            pty_input_dump: std::env::var_os("BT_PTY_INPUT_DUMP"),
            mouse_trace: std::env::var_os("BT_MOUSE_TRACE"),
            card_trace: std::env::var_os("BT_CARD_TRACE"),
            attention_trace: std::env::var_os("BT_ATTENTION_TRACE"),
            preview_trace: std::env::var_os("BT_PREVIEW_TRACE"),
            web_trace: std::env::var_os("BT_WEB_TRACE"),
            glyph_census: std::env::var_os("BT_GLYPH_CENSUS"),
            ime_trace: std::env::var_os("BT_IME_TRACE"),
            focus_thumb_dump: std::env::var_os("BT_FOCUS_THUMB_DUMP"),
            chrome_dump: std::env::var_os("BT_CHROME_DUMP"),
            hotkey_trace: std::env::var_os("BT_HOTKEY_TRACE"),
            decor_trace: std::env::var_os("BT_DECOR_TRACE"),
            semantic_trace: std::env::var_os("BT_SEMANTIC_TRACE"),
            perf_trace: std::env::var_os("BT_PERF_TRACE"),
            startup_trace: std::env::var_os("BT_STARTUP_TRACE"),
            resize_trace: std::env::var_os("BT_RESIZE_TRACE"),
            layout_events: std::env::var_os("BT_LAYOUT_EVENTS"),
            git_trace: std::env::var_os("BT_GIT_TRACE"),
            named: std::env::vars_os()
                .filter(|(name, _)| {
                    name.to_string_lossy()
                        .to_ascii_uppercase()
                        .starts_with("BT_")
                })
                .collect(),
        })
    }

    /// The answers `values` give, set-but-empty off for every door.
    pub(crate) fn from_values(values: DoorValues) -> Self {
        let file = diagnostics::named_file;
        let on = diagnostics::switched_on;
        Self {
            recording: bt_pty::PtyRecording::new(
                file(values.pty_dump),
                file(values.pty_input_dump),
            ),
            traces: trace::TraceFiles {
                mouse: file(values.mouse_trace),
                card: file(values.card_trace),
                attention: file(values.attention_trace),
                preview: file(values.preview_trace),
                web: file(values.web_trace),
                glyph_census: file(values.glyph_census),
                ime: file(values.ime_trace),
                focus_thumb: file(values.focus_thumb_dump),
            },
            chrome_dump: file(values.chrome_dump),
            hotkey_trace: file(values.hotkey_trace),
            term: bt_term::TermDiagnostics {
                semantic_trace: on(values.semantic_trace),
                decor_trace: file(values.decor_trace),
            },
            perf: on(values.perf_trace),
            startup: on(values.startup_trace),
            resize: on(values.resize_trace),
            layout_events: on(values.layout_events),
            git: on(values.git_trace),
            named_with_a_value: values
                .named
                .iter()
                .filter(|(_, value)| !value.is_empty())
                .map(|(name, _)| name.clone())
                .collect(),
            named: values.named.into_iter().map(|(name, _)| name).collect(),
        }
    }

    /// **Hand every library its answer**, and keep this one as the process's. Called once, by
    /// `main`, before any station can be reached; a second call changes nothing.
    pub(crate) fn install(self) -> &'static Self {
        trace::install(self.traces.clone());
        if let Some(path) = &self.hotkey_trace {
            bt_platform::hotkey::trace_into(path.clone());
        }
        bt_term::install_diagnostics(self.term.clone());
        bt_viewport::trace::install_perf(self.perf);
        INSTALLED.get_or_init(|| self)
    }
}

/// The doors this process's entry installed — every door shut in a process whose entry installed
/// none, which is every test binary.
pub(crate) fn installed() -> &'static EntryDoors {
    INSTALLED.get().unwrap_or(&SHUT)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RED (mutation: hand `values.pty_dump` on as a path without `diagnostics::named_file`, or read
    /// a switch by presence) — **set-but-empty is off for every door the entry reads**: an emptied
    /// variable records nothing, traces nothing and switches nothing on, and a named file is still
    /// that file, byte for byte.
    #[test]
    fn every_door_the_entry_reads_is_off_when_set_but_empty() {
        let empty = || Some(OsString::new());
        let emptied = EntryDoors::from_values(DoorValues {
            pty_dump: empty(),
            pty_input_dump: empty(),
            mouse_trace: empty(),
            card_trace: empty(),
            attention_trace: empty(),
            preview_trace: empty(),
            web_trace: empty(),
            glyph_census: empty(),
            ime_trace: empty(),
            focus_thumb_dump: empty(),
            chrome_dump: empty(),
            hotkey_trace: empty(),
            decor_trace: empty(),
            semantic_trace: empty(),
            perf_trace: empty(),
            startup_trace: empty(),
            resize_trace: empty(),
            layout_events: empty(),
            git_trace: empty(),
            named: vec![(OsString::from("BT_PTY_DUMP"), OsString::new())],
        });
        assert_eq!(
            EntryDoors {
                named: vec![OsString::from("BT_PTY_DUMP")],
                ..SHUT.clone()
            },
            emptied
        );
        assert_eq!(EntryDoors::from_values(DoorValues::default()), SHUT);

        let named = EntryDoors::from_values(DoorValues {
            pty_dump: Some(OsString::from(" ")),
            card_trace: Some(OsString::from("card.log")),
            perf_trace: Some(OsString::from("0")),
            named: vec![(OsString::from("bt_card_trace"), OsString::from("card.log"))],
            ..DoorValues::default()
        });
        assert_eq!(
            named.recording,
            bt_pty::PtyRecording::new(Some(PathBuf::from(" ")), None),
            "whitespace is a filename"
        );
        assert_eq!(named.traces.card, Some(PathBuf::from("card.log")));
        assert!(named.perf, "any value is on");
        assert_eq!(named.named_with_a_value, [OsString::from("bt_card_trace")]);
    }

    /// The marker the two child-process tests below set on the copy of this test binary they run
    /// their child half in: the folder the test owns. Only those tests' child halves read it.
    const CHILD: &str = "BT_ENTRY_DOORS_TEST_CHILD";

    /// The doors that name a file, each set to a file of its own in a child's environment.
    const FILE_DOORS: [&str; 13] = [
        "BT_PTY_DUMP",
        "BT_PTY_INPUT_DUMP",
        "BT_MOUSE_TRACE",
        "BT_CARD_TRACE",
        "BT_ATTENTION_TRACE",
        "BT_PREVIEW_TRACE",
        "BT_WEB_TRACE",
        "BT_GLYPH_CENSUS",
        "BT_IME_TRACE",
        "BT_FOCUS_THUMB_DUMP",
        "BT_CHROME_DUMP",
        "BT_HOTKEY_TRACE",
        "BT_DECOR_TRACE",
    ];

    /// The switches, each set to `1` in a child's environment.
    const SWITCHES: [&str; 6] = [
        "BT_SEMANTIC_TRACE",
        "BT_PERF_TRACE",
        "BT_STARTUP_TRACE",
        "BT_RESIZE_TRACE",
        "BT_LAYOUT_EVENTS",
        "BT_GIT_TRACE",
    ];

    /// Run `selector` — the calling test — again in a copy of this binary of its own, with every
    /// diagnostic door **set**: each file door to a file of `folder`, each switch to `1`, and
    /// [`CHILD`] to `folder`. A process of its own because the environment is the process's and a
    /// test may not write its own (`set_var` races every other test of the binary).
    fn in_a_child_with_every_door_set(selector: &str, folder: &std::path::Path) {
        let listing = bt_platform::quiet_command(std::env::current_exe().unwrap())
            .args(["--exact", selector, "--list"])
            .output()
            .expect("the harness can list its own tests");
        let listed = String::from_utf8_lossy(&listing.stdout);
        let named: Vec<&str> = listed
            .lines()
            .filter_map(|line| line.trim_end().strip_suffix(": test"))
            .collect();
        assert_eq!(
            named,
            [selector],
            "the child selector names one test: {listed}"
        );
        let mut command = bt_platform::quiet_command(std::env::current_exe().unwrap());
        command
            .args(["--exact", selector, "--nocapture", "--test-threads=1"])
            .env(CHILD, folder);
        for door in FILE_DOORS {
            command.env(door, folder.join(format!("{door}.log")));
        }
        for switch in SWITCHES {
            command.env(switch, "1");
        }
        let child = command.output().expect("the child half starts");
        assert!(
            child.status.success(),
            "the child half failed: {}\n{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
    }

    /// Every file in `folder`, by name, sorted.
    fn files_in(folder: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// One line through every trace station this child can reach without a window: the six
    /// gates, the IME dump, the summon trace and the platform's IME trace.
    fn walk_every_station() {
        let line = || String::from("entry-doors station");
        trace::emit(crate::mouse_trace::global(), line);
        trace::emit(crate::card_trace::global(), line);
        trace::emit(crate::attention_trace::global(), line);
        trace::emit(crate::preview_trace::global(), line);
        trace::emit(crate::web_trace::global(), line);
        trace::emit(crate::glyph_trace::global(), line);
        crate::ime_report::TRACE.line(line);
        bt_platform::hotkey::trace(line);
        bt_platform::ime_trace::line(line);
    }

    /// RED (T-DIAGNOSTIC-DOORS-AT-THE-ENTRY: before it, each gate read its variable at its first
    /// station, and this child left `BT_CARD_TRACE.log`, `BT_MOUSE_TRACE.log`, … and
    /// `BT_HOTKEY_TRACE.log` in the folder) — **a process whose entry installed nothing honours no
    /// door, though every one of them is set**: no gate opens, no switch is on, and the folder
    /// stays empty.
    #[test]
    fn a_process_whose_entry_installed_nothing_honours_no_door_though_every_one_is_set() {
        const SELECTOR: &str = "entry_doors::tests::a_process_whose_entry_installed_nothing_honours_no_door_though_every_one_is_set";
        if std::env::var_os(CHILD).is_some() {
            walk_every_station();
            assert!(crate::card_trace::global().is_none());
            assert!(!bt_platform::ime_trace::enabled());
            assert!(!bt_viewport::trace::perf());
            assert_eq!(installed(), &SHUT);
            return;
        }
        let folder = bt_testpath::temp_path("bt-entry-doors-unread");
        std::fs::create_dir_all(&folder).unwrap();
        in_a_child_with_every_door_set(SELECTOR, &folder);
        assert_eq!(files_in(&folder), Vec::<String>::new());
        std::fs::remove_dir_all(&folder).unwrap();
    }

    /// RED (mutation: drop any one door from [`EntryDoors::at_the_entry`], or the trace files from
    /// [`EntryDoors::install`] — the child's assertion or the folder's listing names it) — **the
    /// entry reads every door and installs its answer**: after `at_the_entry().install()` the gates open
    /// the files the variables name, the switches are on, and the recording a pane is born with
    /// is the one the two dump variables name.
    #[test]
    fn the_entry_reads_every_door_and_hands_each_library_its_answer() {
        const SELECTOR: &str =
            "entry_doors::tests::the_entry_reads_every_door_and_hands_each_library_its_answer";
        if let Some(folder) = std::env::var_os(CHILD) {
            let folder = PathBuf::from(folder);
            let doors = EntryDoors::at_the_entry().install();
            crate::ime_outbound::install();
            walk_every_station();
            let at = |door: &str| Some(folder.join(format!("{door}.log")));
            assert_eq!(
                doors.recording,
                bt_pty::PtyRecording::new(at("BT_PTY_DUMP"), at("BT_PTY_INPUT_DUMP"))
            );
            assert_eq!(doors.chrome_dump, at("BT_CHROME_DUMP"));
            assert_eq!(doors.traces.focus_thumb, at("BT_FOCUS_THUMB_DUMP"));
            assert_eq!(
                doors.term,
                bt_term::TermDiagnostics {
                    semantic_trace: true,
                    decor_trace: at("BT_DECOR_TRACE"),
                }
            );
            assert!(
                doors.perf && doors.startup && doors.resize && doors.layout_events && doors.git
            );
            assert!(bt_viewport::trace::perf() && bt_platform::ime_trace::enabled());
            assert!(
                FILE_DOORS
                    .into_iter()
                    .chain(SWITCHES)
                    .all(|door| doors.named.iter().any(|named| named == door)),
                "{:?}",
                doors.named
            );
            assert!(std::ptr::eq(doors, installed()));
            return;
        }
        let folder = bt_testpath::temp_path("bt-entry-doors-read");
        std::fs::create_dir_all(&folder).unwrap();
        in_a_child_with_every_door_set(SELECTOR, &folder);
        assert_eq!(
            files_in(&folder),
            [
                "BT_ATTENTION_TRACE.log",
                "BT_CARD_TRACE.log",
                "BT_GLYPH_CENSUS.log",
                "BT_HOTKEY_TRACE.log",
                "BT_IME_TRACE.log",
                "BT_MOUSE_TRACE.log",
                "BT_PREVIEW_TRACE.log",
                "BT_WEB_TRACE.log",
            ]
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
