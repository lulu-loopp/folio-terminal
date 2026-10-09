//! The pump, driven over real sessions fed real bytes (design T-COMPOSE-CRATE §6.2, state tests
//! 5 and 6, and the declining executor of Disagreements 2).

use std::collections::BTreeSet;
use std::num::{NonZeroI64, NonZeroU32};
use std::path::{Path, PathBuf};

use bt_doc::{
    DecorationLifecycle, SUBPIXELS_PER_PX,
    math::{MathRaster, MathRenderError},
};
use bt_math::MathEngine;
use bt_term::{
    DecodedInlineImage, DualPlaneSession, InlineImageDecodeError, InlineImageScaleTask,
    InlineImageTask, LIVE_MATH_STABLE_INTERVAL, MathLayoutOptions, PathVerdict, ScaledInlineImage,
    SessionDecorationTask, SessionMathTask,
};
use bt_transcript::TranscriptId;
use bt_viewport::{ViewportFrame, ViewportProjection};
use web_time::Instant;

use super::*;
use crate::{Pane, project, schedule, typeset};

const INK: [u8; 3] = [0xd8, 0xdc, 0xe8];

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn subpixels(px: i64) -> NonZeroI64 {
    NonZeroI64::new(px * SUBPIXELS_PER_PX).unwrap()
}

/// A session and its view, with printed paths detected and the cell metrics an inline formula
/// can be placed in (a 24 px row, 19 px ASCII baseline, 12 px cells, 20 px em).
fn pane_of(columns: u32, rows: u32) -> (DualPlaneSession, ViewportProjection) {
    bt_term::install_test_host_names();
    let mut session = DualPlaneSession::new(nz(columns), nz(rows));
    session.set_math_layout_options(MathLayoutOptions {
        detect_image_paths: true,
        ..MathLayoutOptions::default()
    });
    session.set_font_size_subpixels(subpixels(20));
    session.set_cell_height_subpixels(subpixels(24));
    session.set_cell_width_subpixels(subpixels(12));
    session.set_ascii_baseline_subpixels(subpixels(19));
    let view = session.new_projection(session.layout_key());
    (session, view)
}

/// Project the pane and schedule what the frame shows, as a host turn does for a frame that is
/// not held.
fn compose(session: &mut DualPlaneSession, view: &mut ViewportProjection) -> ViewportFrame {
    let projected = project(Pane { session, view }).unwrap();
    schedule(session, &projected.frame);
    projected.frame
}

fn frame_text(frame: &ViewportFrame) -> String {
    let columns = frame.columns.get() as usize;
    (0..frame.drawable_rows())
        .map(|row| {
            frame.cells[row * columns..(row + 1) * columns]
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What a task is, for "was it taken twice": its kind and the identity its completion carries.
fn identity(task: &SessionDecorationTask) -> String {
    match task {
        SessionDecorationTask::Math(task) => match task.as_ref() {
            SessionMathTask::Frozen(task) => format!("frozen {}", task.candidate_id.0),
            SessionMathTask::Live(task) => {
                format!("live {:?} {}", task.screen, task.candidate_row)
            }
        },
        SessionDecorationTask::InlineImage(task) => format!("image {}", task.occurrence_id),
        SessionDecorationTask::ScaleInlineImage(task) => format!("scale {}", task.occurrence_id),
        SessionDecorationTask::VerifyPath(path) => format!("path {}", path.display()),
    }
}

/// **The host that does nothing**: every task declined, and no decoder, no engine, no disk — the
/// web executor's shape for the kinds it cannot do. It writes down what it was asked, so a test
/// can tell whether anything was asked twice.
#[derive(Default)]
struct DecliningExecutor {
    asked: Vec<String>,
    frozen: Vec<TranscriptId>,
}

impl Executor for DecliningExecutor {
    fn math(&mut self, task: &mut SessionMathTask) -> Outcome<Result<MathRaster, MathRenderError>> {
        if let SessionMathTask::Frozen(frozen) = task {
            self.frozen.push(frozen.candidate_id);
        }
        self.asked
            .push(identity(&SessionDecorationTask::Math(Box::new(
                task.clone(),
            ))));
        Outcome::Declined
    }

    fn table(&mut self, source: &str, _: f32) -> Outcome<Result<MathRaster, MathRenderError>> {
        self.asked.push(format!("table {source}"));
        Outcome::Declined
    }

    fn image(
        &mut self,
        task: &InlineImageTask,
    ) -> Outcome<Result<DecodedInlineImage, InlineImageDecodeError>> {
        self.asked
            .push(identity(&SessionDecorationTask::InlineImage(task.clone())));
        Outcome::Declined
    }

    fn scale(&mut self, task: &InlineImageScaleTask) -> Outcome<ScaledInlineImage> {
        self.asked
            .push(identity(&SessionDecorationTask::ScaleInlineImage(
                task.clone(),
            )));
        Outcome::Declined
    }

    fn verify(&mut self, path: &Path) -> Outcome<PathVerdict> {
        self.asked.push(identity(&SessionDecorationTask::VerifyPath(
            path.to_path_buf(),
        )));
        Outcome::Declined
    }
}

/// **The desktop's answers for the kinds that touch a disk**: a real decode and a real `metadata`
/// of a file that is not there, and formulas declined. The comparison a declined record must not
/// look like.
struct DiskExecutor;

impl Executor for DiskExecutor {
    fn math(&mut self, _: &mut SessionMathTask) -> Outcome<Result<MathRaster, MathRenderError>> {
        Outcome::Declined
    }

    fn table(&mut self, _: &str, _: f32) -> Outcome<Result<MathRaster, MathRenderError>> {
        Outcome::Declined
    }

    fn image(
        &mut self,
        task: &InlineImageTask,
    ) -> Outcome<Result<DecodedInlineImage, InlineImageDecodeError>> {
        Outcome::Done(bt_term::decode_inline_image(task.clone()))
    }

    fn scale(&mut self, task: &InlineImageScaleTask) -> Outcome<ScaledInlineImage> {
        Outcome::Done(bt_term::scale_inline_image(task))
    }

    fn verify(&mut self, path: &Path) -> Outcome<PathVerdict> {
        Outcome::Done(bt_term::verify_path(path, &|_| None))
    }
}

/// **The desktop lane's formulas, answered on this thread**: the engine, `typeset`, in [`INK`];
/// a proven table measured as nothing (the extent is the host's shaper's, which this has none of).
struct TypesettingExecutor {
    engine: MathEngine,
}

impl Executor for TypesettingExecutor {
    fn math(&mut self, task: &mut SessionMathTask) -> Outcome<Result<MathRaster, MathRenderError>> {
        Outcome::Done(typeset(&self.engine, task, INK))
    }

    fn table(&mut self, _: &str, _: f32) -> Outcome<Result<MathRaster, MathRenderError>> {
        Outcome::Done(Err(MathRenderError::NotDetected))
    }

    fn image(
        &mut self,
        task: &InlineImageTask,
    ) -> Outcome<Result<DecodedInlineImage, InlineImageDecodeError>> {
        Outcome::Done(bt_term::decode_inline_image(task.clone()))
    }

    fn scale(&mut self, task: &InlineImageScaleTask) -> Outcome<ScaledInlineImage> {
        Outcome::Done(bt_term::scale_inline_image(task))
    }

    fn verify(&mut self, path: &Path) -> Outcome<PathVerdict> {
        Outcome::Done(bt_term::verify_path(path, &|_| None))
    }
}

/// The names the world below prints: a picture and a note in a folder that does not exist.
struct Printed {
    picture: PathBuf,
    note: PathBuf,
}

fn printed() -> Printed {
    let folder = bt_testpath::temp_path("bt-compose-pump");
    Printed {
        picture: folder.join("\u{56fe}\u{7247}.png"),
        note: folder.join("\u{7b14}\u{8bb0}-notes.md"),
    }
}

/// **One screen with every kind of decoration work on it**: a display formula already scrolled
/// into history, a pasted picture (`OSC 1337`), a printed picture path, a printed note, and a
/// live display formula and a live inline one, in mixed script. Settled, composed at the bottom and
/// at the top, so every kind has been filed.
fn a_screen_of_work(
    session: &mut DualPlaneSession,
    view: &mut ViewportProjection,
    names: &Printed,
) {
    let start = Instant::now();
    let mut bytes = "$$a^2 + b^2$$\r\n".to_owned();
    for line in 0..10 {
        bytes.push_str(&format!("\u{586b}\u{5145} filler {line}\r\n"));
    }
    bytes.push_str("\x1b]1337;File=inline=1:AAAA\x07\r\n");
    bytes.push_str(&format!("{}\r\n", names.picture.display()));
    bytes.push_str(&format!("{}\r\n", names.note.display()));
    bytes.push_str("\u{516c}\u{5f0f}\r\n$$c^2$$\r\n");
    bytes.push_str("\x1b]133;A\x07> \x1b]133;B\x07show\x1b]133;C\x07\r\n");
    bytes.push_str("\u{80fd}\u{91cf} $E = mc^2$ here\r\n");
    bytes.push_str("\x1b]133;D;0\x07\x1b]133;A\x07> \x1b]133;B\x07");
    session.feed_at(bytes.as_bytes(), start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    compose(session, view);
    view.scroll_to_top();
    compose(session, view);
}

/// Pump with budget `budget` until the session owes nothing, bounded so a pump that never
/// finished would fail here rather than hang.
fn pump_dry(
    session: &mut DualPlaneSession,
    executor: &mut dyn Executor,
    budget: Budget,
) -> Vec<PumpReport> {
    let mut reports = Vec::new();
    for _ in 0..256 {
        let report = pump(session, executor, budget);
        reports.push(report);
        if !report.more_pending {
            return reports;
        }
    }
    panic!("the session was still owed work after 256 pumps: {reports:?}");
}

// ── test 5: a declining executor ─────────────────────────────────────────────────────────────

/// RED — **a pump with an executor that declines everything gives every task it takes a terminal
/// completion** (§6.2 test 5): the session is owed nothing afterwards, no task is taken twice —
/// not by the pump and not by the frames composed after it — the screen shows the formulas'
/// source text, and every record is in its declined state, which is not the state a real missing
/// file leaves.
///
/// MUTATIONS: drop the `decline_path_verification` call from `pump`'s `VerifyPath` arm — the path
/// stays in flight and `outstanding_decoration_work` stays above zero; in `pump`'s `InlineImage`
/// arm, answer `Declined` with `Err(InlineImageDecodeError::Io(..))` — the picture is not
/// `declined_by_host`; make `decline_path_verification` leave the name out of the declined ledger
/// — the composed frame asks about it again and it is taken twice.
#[test]
fn a_declining_executor_gives_every_task_it_takes_a_terminal_completion() {
    let names = printed();
    let (mut session, mut view) = pane_of(60, 8);
    a_screen_of_work(&mut session, &mut view, &names);
    assert!(session.outstanding_decoration_work() > 0, "work was filed");

    let mut executor = DecliningExecutor::default();
    let reports = pump_dry(&mut session, &mut executor, Budget { tasks: 2 });
    // The frames after the answers: whatever they would ask for again is asked here.
    for _ in 0..2 {
        view.scroll_to_bottom();
        compose(&mut session, &mut view);
        view.scroll_to_top();
        compose(&mut session, &mut view);
        pump_dry(&mut session, &mut executor, Budget { tasks: 2 });
    }

    assert_eq!(
        session.outstanding_decoration_work(),
        0,
        "every task taken was answered: {reports:?}"
    );
    let taken: Vec<&String> = executor
        .asked
        .iter()
        .filter(|asked| !asked.starts_with("table "))
        .collect();
    let distinct: BTreeSet<&String> = taken.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        taken.len(),
        "no task is taken twice: {taken:?}"
    );
    let declined: usize = reports.iter().map(|report| report.declined).sum();
    let completed: usize = reports.iter().map(|report| report.completed).sum();
    assert_eq!(completed, 0, "a declining executor completes nothing");
    assert!(declined > 0);
    for kind in ["frozen ", "live ", "image ", "path "] {
        assert!(
            taken.iter().any(|asked| asked.starts_with(kind)),
            "the screen filed `{kind}` work, or this proves nothing about it: {taken:?}"
        );
    }

    // The source text is what stands.
    view.scroll_to_top();
    let top = compose(&mut session, &mut view);
    assert!(top.math_blocks.is_empty(), "no picture over any formula");
    assert!(
        frame_text(&top).contains("$$a^2 + b^2$$"),
        "{}",
        frame_text(&top)
    );
    view.scroll_to_bottom();
    let bottom = compose(&mut session, &mut view);
    assert!(bottom.math_blocks.is_empty());
    let text = frame_text(&bottom);
    assert!(
        text.contains("$$c^2$$") && text.contains("$E = mc^2$"),
        "{text}"
    );

    // Each record in its declined state.
    for id in &executor.frozen {
        assert_eq!(
            session.decoration(*id).map(|record| record.decoration),
            Some(DecorationLifecycle::Suppressed),
            "a declined formula is set aside with no failure to report"
        );
    }
    let pictures = session.inline_image_records();
    assert_eq!(pictures.len(), 1, "the pasted picture: {pictures:?}");
    assert!(
        pictures[0].failed && pictures[0].declined_by_host,
        "{pictures:?}"
    );
    for name in [&names.picture, &names.note] {
        assert!(session.path_declined_by_host(name), "{name:?}");
        assert_eq!(session.path_verdict(name), None, "no verdict was written");
    }

    // A real missing file, for comparison: the same screen, answered by the disk.
    let (mut missing, mut missing_view) = pane_of(60, 8);
    a_screen_of_work(&mut missing, &mut missing_view, &names);
    pump_dry(&mut missing, &mut DiskExecutor, Budget { tasks: 2 });
    assert_eq!(
        missing.path_verdict(&names.note),
        Some(PathVerdict::absent())
    );
    assert!(!missing.path_declined_by_host(&names.note));
    let pictures = missing.inline_image_records();
    assert!(
        pictures[0].failed && !pictures[0].declined_by_host,
        "a payload that does not decode failed for itself: {pictures:?}"
    );
}

/// RED — **the declining executor holds no `InlineImageDecoder`** (design Disagreements 2: the web
/// executor declines a picture before a decoder exists, so none is ever constructed).
///
/// Asked of the type, which is where "holds" is decided: a value of `DecliningExecutor` is its
/// fields and nothing else, and a decoder field — whatever its name — would add the decoder's own
/// size to it. The decoder is not zero-sized, so the comparison cannot pass by accident.
///
/// MUTATION: add a `decoder: bt_term::InlineImageDecoder` field to `DecliningExecutor` (built by
/// its `Default`) — its size grows by the decoder's.
#[test]
fn the_declining_executor_holds_no_decoder() {
    assert!(size_of::<bt_term::InlineImageDecoder>() > 0);
    assert_eq!(
        size_of::<DecliningExecutor>(),
        size_of::<Vec<String>>() + size_of::<Vec<TranscriptId>>(),
        "the declining executor is what it writes down, and nothing that could decode"
    );
}

// ── test 6: the budget ───────────────────────────────────────────────────────────────────────

/// RED — **a pump takes at most `budget.tasks` tasks and says while work remains** (§6.2 test 6),
/// and **the executor's answer lands as the same completion the desktop's lane gives**: the same
/// screen answered task by task by `typeset` (the lane's arm) and answered through the pump draws
/// the same frames.
///
/// MUTATIONS: let `pump` take one task more than its budget (`0..=budget.tasks`) — the first
/// report counts two; report `more_pending: false` unconditionally — the first report says
/// nothing remains while it does; complete a frozen formula with `Err(HostDeclined)` on `Done`
/// — the pumped frame at the top of the history carries no picture.
#[test]
fn a_pump_takes_its_budget_and_lands_the_lanes_completion() {
    let names = printed();
    let (mut pumped, mut pumped_view) = pane_of(60, 8);
    a_screen_of_work(&mut pumped, &mut pumped_view, &names);
    let (mut lane, mut lane_view) = pane_of(60, 8);
    a_screen_of_work(&mut lane, &mut lane_view, &names);

    let mut executor = TypesettingExecutor {
        engine: MathEngine::new(),
    };
    let first = pump(&mut pumped, &mut executor, Budget::default());
    assert_eq!(
        first.completed + first.declined,
        1,
        "one task per call by default"
    );
    assert!(first.more_pending, "and more remains");
    let reports = pump_dry(&mut pumped, &mut executor, Budget { tasks: 3 });
    for report in &reports {
        assert!(report.completed + report.declined <= 3, "{report:?}");
    }
    assert!(!reports.last().unwrap().more_pending);
    assert_eq!(pumped.outstanding_decoration_work(), 0);

    // The desktop lane's way: every task taken, the formula typeset by the lane's own call, and
    // the answer handed to the session's completion for it.
    let engine = MathEngine::new();
    while let Some(task) = lane.take_decoration_worker_task() {
        match task {
            SessionDecorationTask::Math(mut task) => {
                let result = typeset(&engine, &mut task, INK);
                match *task {
                    SessionMathTask::Frozen(task) => lane.complete_worker_result(task, result),
                    SessionMathTask::Live(task) => lane.complete_live_worker_result(task, result),
                };
            }
            SessionDecorationTask::InlineImage(task) => {
                let result = bt_term::decode_inline_image(task.clone());
                lane.complete_inline_image_result(task, result);
            }
            SessionDecorationTask::ScaleInlineImage(task) => {
                lane.complete_inline_image_scale(bt_term::scale_inline_image(&task));
            }
            SessionDecorationTask::VerifyPath(path) => {
                let verdict = bt_term::verify_path(&path, &|_| None);
                lane.complete_path_verification(path, verdict);
            }
        }
    }

    for (pumped_frame, lane_frame) in [
        (
            compose(&mut pumped, &mut pumped_view),
            compose(&mut lane, &mut lane_view),
        ),
        {
            pumped_view.scroll_to_bottom();
            lane_view.scroll_to_bottom();
            (
                compose(&mut pumped, &mut pumped_view),
                compose(&mut lane, &mut lane_view),
            )
        },
    ] {
        assert!(
            !pumped_frame.math_blocks.is_empty(),
            "the frame carries pictures, or the comparison is of nothing"
        );
        assert!(pumped_frame == lane_frame, "the pumped frame is the lane's");
    }
}

// ── a table's extent ─────────────────────────────────────────────────────────────────────────

/// A host's table measure, standing in for `bt-app`'s shaper: the extent of a pipe table at
/// `font_size_px`, a function of the source and the em only (rows of 1.5 em, columns of 6 em),
/// with no pixels, as `bt-app`'s `table_raster` returns it.
fn table_extent(source: &str, font_size_px: f32) -> MathRaster {
    let lines: Vec<&str> = source
        .lines()
        .filter(|line| !line.contains("---"))
        .collect();
    let columns = lines
        .first()
        .map_or(0, |line| line.matches('|').count().saturating_sub(1));
    let width_px = (columns as f32 * 6.0 * font_size_px).round() as u32;
    let height_px = (lines.len() as f32 * 1.5 * font_size_px).round() as u32;
    MathRaster {
        rgba: Vec::new(),
        width_px,
        height_px,
        content_height_px: height_px,
        ascent_px: 0.0,
        descent_px: 0.0,
        baseline_px: 0.0,
        render_time: std::time::Duration::ZERO,
        inline_runs: Vec::new(),
    }
}

/// The typesetting executor with a table measure: what it was asked to measure, and at what em.
struct MeasuringExecutor {
    typesetter: TypesettingExecutor,
    measured: Vec<(String, f32)>,
}

impl Executor for MeasuringExecutor {
    fn math(&mut self, task: &mut SessionMathTask) -> Outcome<Result<MathRaster, MathRenderError>> {
        self.typesetter.math(task)
    }

    fn table(
        &mut self,
        source: &str,
        font_size_px: f32,
    ) -> Outcome<Result<MathRaster, MathRenderError>> {
        self.measured.push((source.to_owned(), font_size_px));
        Outcome::Done(Ok(table_extent(source, font_size_px)))
    }

    fn image(
        &mut self,
        task: &InlineImageTask,
    ) -> Outcome<Result<DecodedInlineImage, InlineImageDecodeError>> {
        self.typesetter.image(task)
    }

    fn scale(&mut self, task: &InlineImageScaleTask) -> Outcome<ScaledInlineImage> {
        self.typesetter.scale(task)
    }

    fn verify(&mut self, path: &Path) -> Outcome<PathVerdict> {
        self.typesetter.verify(path)
    }
}

/// RED — **a proven table is measured by the host at the em its task was laid out at, and the
/// extent the host answers is the block the session keeps** (the pump's table road: `math`
/// proves the table and returns no picture, `table` measures it, the session is completed with
/// the measure).
///
/// The pane's em (20 px) is not its row (24 px), so a measure taken at any other size of the
/// layout lands a block of another width.
///
/// MUTATION: in `table_of`, measure at the row instead of the em
/// (`layout.font_size_subpixels as f32 / SUBPIXELS_PER_PX as f32 * 1.2`) — the measure is asked
/// at 24 px and the "measured at the pane's em" assertion goes red.
#[test]
fn a_proven_table_is_measured_at_its_tasks_em_and_kept_at_that_extent() {
    let (mut session, mut view) = pane_of(60, 4);
    let table = "| \u{540d}\u{79f0} | value |\r\n|---|---|\r\n| a | 1 |\r\n| b | 2 |\r\n";
    // Into history, where a proven table is a block over its own rows.
    session
        .feed(
            format!("{table}\u{7ed3}\u{675f} one\r\ntail two\r\ntail three\r\ntail four\r\n")
                .as_bytes(),
        )
        .unwrap();
    compose(&mut session, &mut view);
    view.scroll_to_top();
    compose(&mut session, &mut view);

    let mut executor = MeasuringExecutor {
        typesetter: TypesettingExecutor {
            engine: MathEngine::new(),
        },
        measured: Vec::new(),
    };
    pump_dry(&mut session, &mut executor, Budget { tasks: 4 });
    assert_eq!(session.outstanding_decoration_work(), 0);
    assert!(
        !executor.measured.is_empty(),
        "the screen filed a table, or this proves nothing"
    );
    for (source, font_size_px) in &executor.measured {
        assert!(
            source.contains('\u{540d}'),
            "the table's own source: {source:?}"
        );
        assert_eq!(*font_size_px, 20.0, "measured at the pane's em");
    }

    view.scroll_to_top();
    let frame = compose(&mut session, &mut view);
    let tables: Vec<_> = frame
        .math_blocks
        .iter()
        .filter(|block| block.artifact.kind == bt_viewport::RgbaArtifactKind::Table)
        .collect();
    assert_eq!(
        tables.len(),
        1,
        "the table is a block: {:?}",
        frame.math_blocks.len()
    );
    // The table grows a row at a time, and each growth is measured again; the block is the last.
    let (source, _) = executor
        .measured
        .iter()
        .max_by_key(|(source, _)| source.len())
        .unwrap();
    let expected = table_extent(source, 20.0);
    assert_eq!(
        tables[0].artifact.width_px, expected.width_px,
        "the session keeps the width the host measured at the em"
    );
    assert!(
        tables[0].artifact.height_subpixels >= i64::from(expected.height_px) * SUBPIXELS_PER_PX,
        "and stands at least as tall as the measure"
    );
}
