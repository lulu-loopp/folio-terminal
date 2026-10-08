//! **What Claude Code prints about pasted pictures, printed by a real program and read by a
//! terminal session** (T-IMAGE-N-GAPS).
//!
//! The fixture is Claude Code's own byte shapes, written to a file and printed by a stand-in
//! program started through the test-shell door (`bt_pty::test_shell::Hygiene`); Claude Code
//! itself is never started. The session then asks the real disk about every `[Image #k]` the
//! screen shows, the way the app's worker does.
//!
//! These lived among `bt-term`'s own unit tests until CC-4: the test-shell door is `bt-pty`'s and
//! reaches the platform layer, and `bt-term` names neither, not even for a test.

#![allow(clippy::disallowed_methods)]

use std::{
    num::NonZeroU32,
    path::{Path, PathBuf},
};

use bt_term::{
    DualPlaneSession, InlineImageDecoder, MathLayoutOptions, SessionDecorationTask,
    scale_inline_image, verify_path,
};
use bt_viewport::{ViewportFrame, ViewportProjection};

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

/// A fresh folder under the temp directory holding `present` as small files, and the folder.
fn temporary_pictures(present: &[&str]) -> PathBuf {
    let directory = bt_testpath::temp_path("betterterminal-image-placeholder");
    std::fs::create_dir(&directory).unwrap();
    for name in present {
        std::fs::write(directory.join(name), b"not decoded here").unwrap();
    }
    directory
}

fn enable_path_detection(session: &mut DualPlaneSession) {
    session.set_math_layout_options(MathLayoutOptions {
        detect_image_paths: true,
        ..MathLayoutOptions::default()
    });
}

/// One frame of `session`, with the printed-path questions it raises answered against the real
/// disk and the frame taken again — which is exactly the two-frame rhythm the app has: a frame
/// asks, a worker answers, the next frame draws.
fn frame_after_path_verification(
    session: &mut DualPlaneSession,
    projection: &mut ViewportProjection,
) -> ViewportFrame {
    session.viewport_frame(projection).unwrap();
    session.absorb_printed_path_probes(projection);
    drain_image_decodes(session);
    session.viewport_frame(projection).unwrap()
}

/// Run every outstanding image task through the real decoder, and every printed-path question
/// through `verify_path` with the desktop build's own door-ready transform, exactly as the app's
/// workers do.
fn drain_image_decodes(session: &mut DualPlaneSession) {
    let mut decoder = InlineImageDecoder::default();
    while let Some(task) = session.take_decoration_worker_task() {
        match task {
            SessionDecorationTask::InlineImage(task) => {
                let result = decoder.decode(task.clone());
                session.complete_inline_image_result(task, result);
            }
            SessionDecorationTask::ScaleInlineImage(task) => {
                session.complete_inline_image_scale(scale_inline_image(&task));
            }
            SessionDecorationTask::VerifyPath(path) => {
                let verdict = verify_path(&path, &bt_platform::resolved_for_a_door);
                session.complete_path_verification(path, verdict);
            }
            SessionDecorationTask::Math(_) => panic!("the fixture contains no math"),
        }
    }
}

/// Columns of one frame row carrying the resting reference affordance, and the columns carrying
/// the solid hover upgrade.
fn underlined_columns(frame: &ViewportFrame, row: u32) -> (Vec<u32>, Vec<u32>) {
    let columns = frame.columns.get() as usize;
    let start = row as usize * columns;
    let mut dotted = Vec::new();
    let mut solid = Vec::new();
    for (column, cell) in frame.cells[start..start + columns].iter().enumerate() {
        let flags = cell.style.flags;
        if flags.contains(bt_transcript::CellFlags::UNDERLINE) {
            solid.push(column as u32);
        } else if flags.contains(bt_transcript::CellFlags::DOTTED_UNDERLINE) {
            dotted.push(column as u32);
        }
    }
    (dotted, solid)
}

/// One transcript row as Claude Code prints it for a picture of a sent message: Ink's OSC 8,
/// BEL-terminated (`ESC ]8;;<url> BEL <label> ESC ]8;; BEL`), over exactly the label, indented
/// under the echoed message (T-IMAGE-N-GAPS, the producer's own `As` row).
fn claude_code_image_row(number: u32, target: &Path) -> String {
    format!(
        "  \u{23bf}  \u{1b}]8;;{}\u{7}[Image #{number}]\u{1b}]8;;\u{7}",
        bt_transcript::paths::local_path_to_file_uri(target)
    )
}

/// What a stand-in program that prints exactly `bytes` puts on its output, started through the
/// test-shell door (T-IMAGE-N-GAPS). The fixture is Claude Code's own byte shapes; Claude Code
/// itself is never started.
fn printed_by_a_stand_in(bytes: &[u8]) -> Vec<u8> {
    let hygiene = bt_pty::test_shell::Hygiene::new();
    let fixture = hygiene.root().join("claude-code-screen.bin");
    std::fs::write(&fixture, bytes).unwrap();
    #[cfg(windows)]
    let mut command = {
        let mut command = hygiene.command("cmd", bt_platform::quiet_command);
        command.arg("/C").arg("type").arg(&fixture);
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = hygiene.command("cat", bt_platform::quiet_command);
        command.arg(&fixture);
        command
    };
    let output = command
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .expect("the stand-in program runs");
    assert!(output.status.success(), "the stand-in printed its fixture");
    assert_eq!(
        output.stdout, bytes,
        "byte for byte, as Claude Code wrote it"
    );
    output.stdout
}

/// The `file:` target the frame carries over `(row, column)`, if any.
fn link_target(frame: &ViewportFrame, row: u32, column: u32) -> Option<String> {
    frame.hyperlink_at(row, column).map(|hit| hit.uri)
}

/// RED (T-IMAGE-N-GAPS, owner report 2026-10-08) — **every pasted picture on the input line is
/// a link, whichever extension Claude Code stored it under.**
///
/// The owner's line held `[Image #104]`, `[Image #105]` and `[Image #106]` in one sentence, with
/// only `#106` lit. Claude Code names each stored picture `<k>.<ext>` by its content, and
/// resizes a large one into JPEG first, so one folder holds `103.png`, `104.jpg`, `105.jpg` and
/// `106.png`. The newest learned picture is `103.png`; inferring only `<k>.png` lit `#106`
/// alone.
///
/// MUTATION: infer only the newest learned picture's own extension (the 2026-09-29 rule), and
/// `#104` and `#105` are text while `#106` lights — the screenshot.
#[test]
fn every_pasted_picture_on_the_input_line_lights_whichever_extension_it_was_stored_under() {
    let directory = temporary_pictures(&["103.png", "104.jpg", "105.jpg", "106.png"]);
    let mut session = DualPlaneSession::new(nz(80), nz(8));
    enable_path_detection(&mut session);
    let screen = format!(
        "{}\r\n> 你好世界[Image #104] 你好 [Image #105] [Image #106] 你好世",
        claude_code_image_row(103, &directory.join("103.png"))
    );
    session
        .feed(&printed_by_a_stand_in(screen.as_bytes()))
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    // The first pass learns 103 from the transcript row; the second asks about the rest.
    frame_after_path_verification(&mut session, &mut projection);
    let frame = frame_after_path_verification(&mut session, &mut projection);
    for (column, name) in [(10, "104.jpg"), (28, "105.jpg"), (41, "106.png")] {
        assert_eq!(
            link_target(&frame, 1, column),
            Some(bt_transcript::paths::local_path_to_file_uri(
                &directory.join(name)
            )),
            "the placeholder at column {column} is a link to {name}"
        );
    }
    let (dotted, _) = underlined_columns(&frame, 1);
    assert_eq!(
        dotted,
        (10..22).chain(28..40).chain(41..53).collect::<Vec<_>>(),
        "each wears the resting mark over exactly its own cells"
    );
    for name in ["103.png", "104.jpg", "105.jpg", "106.png"] {
        std::fs::remove_file(directory.join(name)).unwrap();
    }
    std::fs::remove_dir(&directory).unwrap();
}

/// RED (T-IMAGE-N-GAPS) — **a picture the disk did not hold yet when the input line was first
/// drawn lights when Claude Code draws the line again.**
///
/// Claude Code puts `[Image #k]` into the input line at once and writes the file afterwards,
/// asynchronously; the first frame can ask before the file lands and hear "no". A still screen
/// asks nothing more — the rule every printed name follows — but the program drawing the row
/// again (the reader typing on) is the program naming the picture again, and the "no" is asked
/// once more.
///
/// MUTATION: leave the placeholders out of the re-ask pass
/// (`paths_named_on_freshly_printed_rows`), and `#104` stays text after the row is redrawn
/// over a file that is now there.
#[test]
fn a_picture_written_after_the_input_line_was_drawn_lights_when_the_line_is_drawn_again() {
    let directory = temporary_pictures(&["103.png"]);
    let mut session = DualPlaneSession::new(nz(80), nz(8));
    enable_path_detection(&mut session);
    let screen = format!(
        "{}\r\n> 你好世界[Image #104]",
        claude_code_image_row(103, &directory.join("103.png"))
    );
    session
        .feed(&printed_by_a_stand_in(screen.as_bytes()))
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    // The first pass learns 103 from the transcript row; the second asks about 104 beside it.
    frame_after_path_verification(&mut session, &mut projection);
    let frame = frame_after_path_verification(&mut session, &mut projection);
    assert_eq!(
        link_target(&frame, 1, 10),
        None,
        "asked before the file landed: text"
    );
    assert!(
        session
            .path_verdict(&directory.join("104.png"))
            .is_some_and(|verdict| !verdict.exists),
        "and the disk's answer for it was no"
    );

    std::fs::write(directory.join("104.png"), b"written after the paste").unwrap();
    let frame = frame_after_path_verification(&mut session, &mut projection);
    assert_eq!(
        link_target(&frame, 1, 10),
        None,
        "a still screen asks nothing again"
    );

    session
        .feed(&printed_by_a_stand_in(
            "\r> 你好世界[Image #104] 你好".as_bytes(),
        ))
        .unwrap();
    let frame = frame_after_path_verification(&mut session, &mut projection);
    assert_eq!(
        link_target(&frame, 1, 10),
        Some(bt_transcript::paths::local_path_to_file_uri(
            &directory.join("104.png")
        )),
        "the program drew the placeholder again, the question was asked again, and the file \
         is there"
    );
    for name in ["103.png", "104.png"] {
        std::fs::remove_file(directory.join(name)).unwrap();
    }
    std::fs::remove_dir(&directory).unwrap();
}

/// RED (T-IMAGE-N-GAPS) — **a placeholder Claude Code's word wrap split across two rows of its
/// input box is one link over both halves.**
///
/// Claude Code wraps its input itself (word wrap, `hard`, no trim) and draws each wrapped row as
/// a line of its own, so the space inside `[Image #104]` is where a row can end: `…[Image ` on
/// one row, `#104] …` on the next, indented under the prompt. No terminal wrap flag joins them.
///
/// MUTATION: drop the seam pass for placeholders from `implicit_hyperlinks`, and neither half is
/// a link.
#[test]
fn a_placeholder_split_by_the_input_boxs_word_wrap_is_one_link_over_both_halves() {
    let directory = temporary_pictures(&["103.png", "104.png"]);
    let mut session = DualPlaneSession::new(nz(30), nz(8));
    enable_path_detection(&mut session);
    let screen = format!(
        "{}\r\n> 你好世界你好世界你[Image \r\n  #104] 你好",
        claude_code_image_row(103, &directory.join("103.png"))
    );
    session
        .feed(&printed_by_a_stand_in(screen.as_bytes()))
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    frame_after_path_verification(&mut session, &mut projection);
    let frame = frame_after_path_verification(&mut session, &mut projection);
    let picture = Some(bt_transcript::paths::local_path_to_file_uri(
        &directory.join("104.png"),
    ));
    assert_eq!(link_target(&frame, 1, 20), picture, "the upper half");
    assert_eq!(link_target(&frame, 2, 2), picture, "and the lower half");
    assert_eq!(
        underlined_columns(&frame, 1).0,
        (20..26).collect::<Vec<_>>(),
        "the mark covers `[Image` and not the blank after it"
    );
    assert_eq!(
        underlined_columns(&frame, 2).0,
        (2..7).collect::<Vec<_>>(),
        "and `#104]`, not the indent before it"
    );
    for name in ["103.png", "104.png"] {
        std::fs::remove_file(directory.join(name)).unwrap();
    }
    std::fs::remove_dir(&directory).unwrap();
}
