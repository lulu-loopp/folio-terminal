//! **The kept body is the body drawn directly, byte for byte** — 0.4.9 plan row 64a's gate
//! (`docs/DESIGN.md`, entry of 2026-11-03: the window's body is kept).
//!
//! A window keeps its body — the clear, the ground picture and every pane — as a texture, and on
//! a frame whose panes and window facts are the last frame's it puts that texture back instead of
//! drawing the panes again. The claim is that nothing on the glass can tell: every frame a kept
//! body is put back on is the frame the same window would have drawn without one. So two windows
//! over one device are handed the same inputs, frame after frame — `kept`, as the product makes
//! every window, and `direct`, which `bt_render::probe_surface::present_direct` sends down the
//! road every frame took before the layer — and their read-backs are compared byte for byte.
//!
//! The drawing is one function on both roads; what differs is the layer, its key and its copy.
//! So a byte that differs is the copy's (a blend, an sRGB view, a rounding), and a body that is
//! put back when it should have been drawn again is the key's (a stale pane, a stale caret).
//! `body_composes` says which road each frame of `kept` took, so a gate that passed because the
//! body was drawn on every frame is not mistaken for one that passed because it was kept.
//!
//! One test, not nine: the ground and the caret's shape are process-wide, and nine cases run in
//! parallel would race them.
//!
//! MUTATIONS (each red, on the committed tree only):
//! - P1: `BodyState` gains the window's `retained_revision` (any chrome change draws the body
//!   again) — the eight mark steps' `body_composes` assertion goes red.
//! - P2: `BodyKey::matches` stops comparing the panes' frames — the frame after a pane printed
//!   puts back the old rows, and the bytes differ.
//! - P3: `BodyKey::matches` stops comparing `cursor_blink_visible` (the state's own `==`
//!   replaced field by field without it) — the caret's blink puts back a lit caret, and the
//!   bytes differ.
//! - P4: the copy is drawn through the sRGB views with an alpha blend — the bytes differ under
//!   the translucent grounds.

#![allow(clippy::disallowed_methods)]

use std::sync::Arc;

use bt_render::glyph_probe::{GlyphFixture, first_difference};
use bt_render::{
    BackgroundFit, BackgroundImage, CellMetrics, ChromeLabel, ChromeLabelWeight, ChromeQuad,
    ChromeSurface, CursorStyle, FrameSource, FrameTrigger, GpuContext, PresentOutcome, SeatFrame,
    SeatViewport, WindowGround, WindowRenderer, current_cursor_style, set_cursor_style,
    set_window_ground, window_ground,
};
use bt_transcript::CapturedCell;
use bt_viewport::{SelectionSpan, ViewportFrame};

/// The format every window in this product picks.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

/// The window, in logical pixels; each case draws it at its own scale.
const WIDTH: u32 = 900;
const HEIGHT: u32 = 600;

/// How many panes stand side by side.
const PANES: u32 = 3;

/// **One admitted call of door `D` on this thread**, which is the window thread: a present is
/// an owner-thread door. The first call enters the window thread and runs its loop.
fn admit<D: bt_effects::admission::Door, R>(
    work: impl for<'scope> FnOnce(bt_effects::admission::WaitToken<'scope, D>) -> R,
) -> R {
    use bt_effects::admission::{Role, admitted, enter_window_thread, loop_running, role};
    if role() != Role::Window {
        assert!(
            enter_window_thread(),
            "this thread enters as the window thread"
        );
        assert!(loop_running(), "and its loop is running");
    }
    admitted::<D, R>(work).expect("admitted on the window thread")
}

/// The grounds a window is measured over: opaque, translucent, and translucent with a picture.
fn grounds() -> [(&'static str, WindowGround); 3] {
    // A picture with a gradient in every channel and its own alpha, so a copy that rounds,
    // re-encodes or blends anywhere is a copy that moves some byte of it.
    let (width_px, height_px) = (64_u32, 48_u32);
    let rgba: Vec<u8> = (0..height_px)
        .flat_map(|y| {
            (0..width_px).flat_map(move |x| {
                [
                    (x * 4) as u8,
                    (y * 5) as u8,
                    ((x + y) * 3) as u8,
                    (128 + x + y) as u8,
                ]
            })
        })
        .collect();
    let picture = Arc::new(BackgroundImage {
        key: "row 64a gradient".to_owned(),
        rgba: Arc::from(rgba),
        width_px,
        height_px,
    });
    [
        ("opaque", WindowGround::opaque()),
        (
            "60 %",
            WindowGround {
                alpha: 0.6,
                ..WindowGround::opaque()
            },
        ),
        (
            "60 % and a picture",
            WindowGround {
                image: Some(picture),
                fit: BackgroundFit::Fill,
                image_opacity: 0.7,
                alpha: 0.6,
            },
        ),
    ]
}

/// The panes' boxes, side by side across the window.
fn pane_boxes(width: u32, height: u32) -> Vec<SeatViewport> {
    let each = width / PANES;
    (0..PANES)
        .map(|index| SeatViewport {
            x: index * each,
            y: 0,
            width: if index + 1 == PANES {
                width - index * each
            } else {
                each
            },
            height,
        })
        .collect()
}

/// The panes' frames: the glyph fixture's page in each, a selection in the first, search hits
/// in the second, and the first pane's caret shown — it is the one with the keyboard.
fn pane_frames(metrics: CellMetrics, width: u32, height: u32) -> Vec<ViewportFrame> {
    let fixture = GlyphFixture::new(width / PANES, height);
    (0..PANES)
        .map(|index| {
            let mut frame = fixture.frame(metrics);
            match index {
                0 => {
                    frame.cursor.row = 1;
                    frame.cursor.column = 3;
                    frame.cursor.visible = true;
                    frame.selection_spans = vec![SelectionSpan {
                        row: 2,
                        start_column: 1,
                        end_column: 9,
                    }];
                }
                1 => {
                    frame.search_spans = vec![SelectionSpan {
                        row: 0,
                        start_column: 0,
                        end_column: 4,
                    }];
                    frame.current_search_spans = vec![SelectionSpan {
                        row: 1,
                        start_column: 2,
                        end_column: 6,
                    }];
                }
                _ => {}
            }
            frame
        })
        .collect()
}

/// The chrome of mark step `step`: a strip across the top, its caption, and a mark whose colour
/// is the breath's — the thing a working tab changes on every step.
fn chrome(window: &mut WindowRenderer, width: u32, step: u8) {
    let strip = 24.0 * window.scale_factor() as f32;
    let breath = 120 + step * 15;
    let _ = window.set_chrome(
        vec![
            ChromeQuad {
                rect: [0.0, 0.0, width as f32, strip],
                color: [30, 34, 40],
                surface: ChromeSurface::Ground,
            },
            ChromeQuad {
                rect: [6.0, 6.0, strip - 6.0, strip - 6.0],
                color: [breath, 200, 255 - breath],
                surface: ChromeSurface::Ink,
            },
        ],
        vec![ChromeLabel {
            text: "row 64a".to_owned(),
            rect: [strip, 0.0, width as f32 / 2.0, strip],
            clip: None,
            font_size_px: 12.0 * window.scale_factor() as f32,
            color: [220, 220, 220],
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            mono: false,
        }],
        Vec::new(),
    );
}

/// Present `frames` in `boxes` on `window`, the first pane holding the keyboard.
fn present(
    gpu: &mut GpuContext,
    window: &mut WindowRenderer,
    boxes: &[SeatViewport],
    frames: &[ViewportFrame],
) {
    let metrics = window.base_metrics();
    let seats: Vec<SeatFrame<'_>> = boxes
        .iter()
        .zip(frames)
        .enumerate()
        .map(|(index, (seat, frame))| SeatFrame {
            seat: *seat,
            clip: *seat,
            frame,
            metrics,
            focused: index == 0,
        })
        .collect();
    let outcome = admit::<bt_effects::admission::doors::PresentFrame, _>(|token| {
        window.present_frame(
            token,
            gpu,
            &seats,
            FrameTrigger {
                occurred_at: web_time::Instant::now(),
                source: FrameSource::Expose,
            },
        )
    })
    .expect("the frame presents");
    assert!(
        matches!(outcome, PresentOutcome::Presented(_)),
        "every frame of this gate keeps all of its text: {outcome:?}"
    );
}

/// The two windows' pictures of one frame, compared byte for byte.
fn same_bytes(gpu: &GpuContext, kept: &WindowRenderer, direct: &WindowRenderer, at: &str) {
    let kept_pixels = bt_render_probe::read_back(kept, gpu).expect("the kept window reads back");
    let direct_pixels =
        bt_render_probe::read_back(direct, gpu).expect("the direct window reads back");
    assert_eq!(
        kept_pixels.len(),
        direct_pixels.len(),
        "{at}: the two windows are one size"
    );
    if let Some((index, left, right)) = first_difference(&kept_pixels, &direct_pixels) {
        let width = kept.presented_swapchain_size().0 as usize;
        panic!(
            "{at}: the kept body and the body drawn directly differ first at ({}, {}): \
             {left:?} against {right:?}",
            index % width,
            index / width
        );
    }
}

#[test]
fn the_kept_body_is_the_body_drawn_directly_byte_for_byte() {
    let restore_ground = window_ground();
    let restore_style = current_cursor_style();
    let mut gpu = pollster::block_on(GpuContext::headless(FORMAT)).expect("a device");
    for scale in [1.0_f64, 1.5, 2.0] {
        let all_grounds = grounds();
        for (index, (ground_name, ground)) in all_grounds.iter().enumerate() {
            let (_, next_ground) = &all_grounds[(index + 1) % all_grounds.len()];
            let _ = set_window_ground(ground.clone());
            let at = |frame: &str| format!("scale {scale}, ground {ground_name}, {frame}");
            let width = (f64::from(WIDTH) * scale).round() as u32;
            let height = (f64::from(HEIGHT) * scale).round() as u32;
            let mut kept = WindowRenderer::offscreen(&mut gpu, width, height, scale, FORMAT)
                .expect("the kept window");
            let mut direct = WindowRenderer::offscreen(&mut gpu, width, height, scale, FORMAT)
                .expect("the direct window");
            bt_render::probe_surface::present_direct(&mut direct);
            let boxes = pane_boxes(width, height);
            let mut frames = pane_frames(kept.base_metrics(), width, height);

            // Frame 0: the first frame draws the body.
            for window in [&mut kept, &mut direct] {
                chrome(window, width, 0);
                present(&mut gpu, window, &boxes, &frames);
            }
            same_bytes(&gpu, &kept, &direct, &at("frame 0"));
            assert_eq!(kept.body_composes(), 1, "{}", at("frame 0"));

            // Frames 1..=8: the mark steps, and nothing else moves. The body is put back.
            for step in 1..=8 {
                for window in [&mut kept, &mut direct] {
                    chrome(window, width, step);
                    present(&mut gpu, window, &boxes, &frames);
                }
                same_bytes(&gpu, &kept, &direct, &at(&format!("mark step {step}")));
                assert_eq!(
                    kept.body_composes(),
                    1,
                    "{}: a mark step over panes that said nothing draws no pane",
                    at(&format!("mark step {step}"))
                );
            }

            // Frame 9: the last pane prints. The body is drawn again, from the new rows.
            let last = frames.last_mut().expect("three panes");
            let row = last.rows.get() - 1;
            let at_cell = (row * last.columns.get()) as usize;
            for (offset, letter) in "PRINTED".chars().enumerate() {
                last.cells[at_cell + offset] = CapturedCell::plain(letter.to_string());
            }
            for window in [&mut kept, &mut direct] {
                present(&mut gpu, window, &boxes, &frames);
            }
            same_bytes(&gpu, &kept, &direct, &at("a pane printed"));
            assert_eq!(kept.body_composes(), 2, "{}", at("a pane printed"));

            // Frame 10: a mark step after it. The body this compose filed is put back.
            for window in [&mut kept, &mut direct] {
                chrome(window, width, 9);
                present(&mut gpu, window, &boxes, &frames);
            }
            same_bytes(&gpu, &kept, &direct, &at("a mark step after the print"));
            assert_eq!(
                kept.body_composes(),
                2,
                "{}: the body drawn from the printed rows is the one kept",
                at("a mark step after the print")
            );

            // Frame 11: the caret blinks off.
            for window in [&mut kept, &mut direct] {
                let _ = window.set_cursor_blink_visible(false);
                present(&mut gpu, window, &boxes, &frames);
            }
            same_bytes(&gpu, &kept, &direct, &at("the caret blinked off"));
            assert_eq!(kept.body_composes(), 3, "{}", at("the caret blinked off"));

            // Frame 12: the caret lights again, and nothing else moves.
            for window in [&mut kept, &mut direct] {
                let _ = window.set_cursor_blink_visible(true);
                present(&mut gpu, window, &boxes, &frames);
            }
            same_bytes(&gpu, &kept, &direct, &at("the caret blinked on"));
            assert_eq!(kept.body_composes(), 4, "{}", at("the caret blinked on"));

            // Frame 13: the caret changes shape, lit, and nothing else moves.
            let _ = set_cursor_style(CursorStyle::Underline);
            for window in [&mut kept, &mut direct] {
                present(&mut gpu, window, &boxes, &frames);
            }
            same_bytes(&gpu, &kept, &direct, &at("the caret's shape"));
            assert_eq!(kept.body_composes(), 5, "{}", at("the caret's shape"));

            // Frame 14: the window loses the keyboard; its caret goes hollow.
            for window in [&mut kept, &mut direct] {
                let _ = window.set_window_focused(false);
                present(&mut gpu, window, &boxes, &frames);
            }
            same_bytes(&gpu, &kept, &direct, &at("the window's focus"));
            assert_eq!(kept.body_composes(), 6, "{}", at("the window's focus"));

            // Frame 15: the ground changes under the live windows — the next case's ground.
            let _ = set_window_ground(next_ground.clone());
            for window in [&mut kept, &mut direct] {
                present(&mut gpu, window, &boxes, &frames);
            }
            same_bytes(&gpu, &kept, &direct, &at("the ground changed"));
            assert_eq!(kept.body_composes(), 7, "{}", at("the ground changed"));

            // Frame 16: the window is resized; the panes keep their boxes inside it.
            let (wider, taller) = (width + 37, height + 23);
            for window in [&mut kept, &mut direct] {
                window
                    .resize(&gpu, wider, taller)
                    .expect("the window resizes");
                present(&mut gpu, window, &boxes, &frames);
            }
            same_bytes(&gpu, &kept, &direct, &at("a resize"));
            assert_eq!(kept.body_composes(), 8, "{}", at("a resize"));

            // Frame 17: a mark step at the new size. The body filed into the remade layer is put
            // back.
            for window in [&mut kept, &mut direct] {
                chrome(window, wider, 0);
                present(&mut gpu, window, &boxes, &frames);
            }
            same_bytes(&gpu, &kept, &direct, &at("a mark step after the resize"));
            assert_eq!(
                kept.body_composes(),
                8,
                "{}: the body drawn at the new size is the one kept",
                at("a mark step after the resize")
            );
            let _ = set_cursor_style(restore_style);
        }
    }
    let _ = set_window_ground(restore_ground);
    let _ = set_cursor_style(restore_style);
}
