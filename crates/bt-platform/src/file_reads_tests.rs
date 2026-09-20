use super::*;

#[test]
fn file_reads_lane_totals_roll_over_without_losing_bytes() {
    let ledger = Ledger::new();
    ledger.add(
        Lane::InlineImage,
        17,
        1,
        Some(Path::new("/users/private/a.png")),
    );
    ledger.add(Lane::Preview, 23, 2, None);
    let first = ledger.rotate().unwrap();
    assert_eq!(
        first.lanes[Lane::InlineImage as usize],
        Totals {
            bytes: 17,
            reads: 1,
            ..Totals::default()
        }
    );
    assert_eq!(
        first.lanes[Lane::Preview as usize],
        Totals {
            bytes: 23,
            reads: 2,
            ..Totals::default()
        }
    );
    ledger.add(Lane::Peek, 31, 1, None);
    let second = ledger.rotate().unwrap();
    assert_eq!(second.total_bytes(), 31);
    assert_eq!(second.lanes[Lane::InlineImage as usize], Totals::default());
}

#[test]
fn file_reads_budget_is_strict_and_input_only_excuses_the_total() {
    let mut minute = Minute::default();
    minute.lanes[0].bytes = BUDGET;
    assert!(!over_budget(&minute, false));
    minute.lanes[1].bytes = 1;
    assert!(over_budget(&minute, false));
    assert!(!over_budget(&minute, true));
    minute.lanes[0].bytes += 1;
    assert!(over_budget(&minute, true));
    minute.elapsed_ms = 120_000;
    assert!(
        !over_budget(&minute, false),
        "a delayed collector reports a rate, not two minutes as one"
    );
}

#[test]
fn file_reads_repeat_and_closing_line_use_supplied_minutes() {
    let mut reporter = Reporter::default();
    let mut minute = Minute::default();
    minute.lanes[0].bytes = 60_000_000;
    let first = reporter.observe(1, &minute, false).unwrap();
    assert!(first.contains("with no input"));
    for end in 2..=10 {
        assert!(reporter.observe(end, &minute, false).is_none());
    }
    assert!(reporter.observe(11, &minute, false).is_some());
    let closed = reporter.observe(12, &Minute::default(), false).unwrap();
    assert!(closed.contains("ended after 11 min, 0.660 GB"), "{closed}");
    assert!(reporter.observe(13, &Minute::default(), false).is_none());
    assert!(
        reporter
            .observe(14, &minute, true)
            .unwrap()
            .contains("with input")
    );
}

#[test]
fn file_reads_top_three_never_format_directories_or_control_characters() {
    let ledger = Ledger::new();
    for (path, count) in [
        (r"C:\Users\alice\pictures\first.png", 9),
        ("/home/alice/second.png", 8),
        ("/home/alice/third.png", 7),
        ("/home/alice/fourth.png", 1),
    ] {
        for _ in 0..count {
            ledger.add(Lane::InlineImage, 3_000_000, 1, Some(Path::new(path)));
        }
    }
    let minute = ledger.rotate().unwrap();
    let line = minute.line(1, false);
    assert!(line.contains("first.png ×9"), "{line}");
    assert!(line.contains("second.png ×8"));
    assert!(line.contains("third.png ×7"));
    for secret in [
        "alice",
        "Users",
        "/home",
        "pictures",
        "fourth.png",
        "\\",
    ] {
        assert!(!line.contains(secret), "{line}");
    }
    ledger.add(
        Lane::InlineImage,
        1,
        1,
        Some(Path::new("/private/evil\nname.png")),
    );
    assert!(!ledger.rotate().unwrap().line(2, false).contains('\n'));
    assert!(ledger.rotate().unwrap().top[0].is_empty());
}

#[test]
fn file_reads_an_inflight_add_defers_collection_without_waiting() {
    let ledger = Ledger::new();
    ledger.banks[0].users.fetch_add(1, Ordering::Acquire);
    assert!(ledger.rotate().is_none());
    ledger.add(Lane::Pdf, 12, 1, None);
    ledger.banks[0].users.fetch_sub(1, Ordering::Release);
    assert_eq!(ledger.rotate().unwrap().total_bytes(), 0);
    assert_eq!(ledger.rotate().unwrap().total_bytes(), 12);
}

#[test]
fn file_reads_stream_counts_partial_errors_and_new_passes() {
    struct FailsAfterBytes(bool);
    impl Read for FailsAfterBytes {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if self.0 {
                return Err(io::Error::other("fixture"));
            }
            self.0 = true;
            bytes[..3].copy_from_slice(b"abc");
            Ok(3)
        }
    }
    let ledger = Ledger::new();
    let mut input = Reader::with_ledger(FailsAfterBytes(false), Lane::Other, None, &ledger);
    let mut bytes = Vec::new();
    assert!(input.read_to_end(&mut bytes).is_err());
    assert_eq!(bytes, b"abc");
    assert_eq!(
        ledger.rotate().unwrap().lanes[Lane::Other as usize],
        Totals {
            bytes: 3,
            reads: 1,
            ..Totals::default()
        }
    );
}

#[test]
fn file_reads_parallel_adds_survive_rotation_and_table_overflow() {
    let ledger = std::sync::Arc::new(Ledger::new());
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let ledger = std::sync::Arc::clone(&ledger);
            std::thread::spawn(move || {
                for _ in 0..10_000 {
                    ledger.add(Lane::Animation, 7, 1, Some(Path::new("/private/clip.gif")));
                }
            })
        })
        .collect();
    let mut bytes = 0;
    let mut reads = 0;
    while workers.iter().any(|worker| !worker.is_finished()) {
        if let Some(minute) = ledger.rotate() {
            bytes += minute.total_bytes();
            reads += minute.lanes[Lane::Animation as usize].reads;
        }
        std::thread::yield_now();
    }
    for worker in workers {
        worker.join().unwrap();
    }
    // One bank may be pending, the other may have accepted the final adds.
    for _ in 0..2 {
        let minute = ledger.rotate().unwrap();
        bytes += minute.total_bytes();
        reads += minute.lanes[Lane::Animation as usize].reads;
    }
    assert_eq!(bytes, 280_000);
    assert_eq!(reads, 40_000);
    for i in 0..SLOTS + 3 {
        ledger.add(
            Lane::Other,
            1,
            1,
            Some(Path::new(&format!("/private/{i}.txt"))),
        );
    }
    let minute = ledger.rotate().unwrap();
    assert_eq!(minute.total_bytes(), (SLOTS + 3) as u64);
    assert_eq!(minute.omitted[Lane::Other as usize], 3);
    assert!(minute.line(1, false).contains("top tracked"));
}

#[test]
fn file_reads_seek_starts_a_new_pass_and_empty_reads_are_counted() {
    let ledger = Ledger::new();
    let mut reader = Reader::with_ledger(io::Cursor::new(b"hello"), Lane::Peek, None, &ledger);
    let mut head = [0; 2];
    reader.read_exact(&mut head).unwrap();
    reader.seek(SeekFrom::Start(0)).unwrap();
    let mut all = Vec::new();
    reader.read_to_end(&mut all).unwrap();
    assert_eq!(all, b"hello");
    assert_eq!(
        ledger.rotate().unwrap().lanes[Lane::Peek as usize],
        Totals {
            bytes: 7,
            reads: 2,
            ..Totals::default()
        }
    );
    let mut reader = Reader::with_ledger(io::empty(), Lane::Settings, None, &ledger);
    reader.read_to_end(&mut all).unwrap();
    assert_eq!(
        ledger.rotate().unwrap().lanes[Lane::Settings as usize].reads,
        1
    );
}

#[test]
fn file_reads_opaque_loads_never_invent_file_bytes() {
    let ledger = Ledger::new();
    ledger.add_event(Lane::Fonts, 0, 0, 3, None);
    let minute = ledger.rotate().unwrap();
    assert_eq!(minute.total_bytes(), 0);
    assert_eq!(minute.lanes[Lane::Fonts as usize].reads, 0);
    assert!(
        minute
            .line(1, false)
            .contains("3 opaque loads (bytes unknown)")
    );
    assert!(minute.perf_line(1, false).contains("fonts_opaque_loads=3"));
    assert!(!over_budget(&minute, false));
}
