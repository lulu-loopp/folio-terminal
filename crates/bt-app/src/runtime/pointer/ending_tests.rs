//! **Every lifecycle door ends a held capture, driven** (T-POINTER-CAPTURE cut 5, R-9): on a
//! headless window, a capture of each owner kind is latched, the door is run as the product runs
//! it, and the slot is read — empty, with the ending the door is the door of, for every owner the
//! door takes away; still held for every owner it does not.

use super::*;
use crate::headless::key_down;
use crate::headless::{Headless, HeadlessApplication, HeadlessFolio};
use crate::runtime::pointer::{CaptureOwner, PointerCapture};
use crate::test_support::{buffer_saying, cross_tab, host_spelling, seat_of, tab_with_a_preview};
use crate::{
    DividerDrag, DividerGrip, Drag, DragCarry, DragLatch, DragSource, FilePeekPress, FloatDrag,
    FloatHeadPress, ImageDrag, LeafSession, LocalImageActivation, PanePress, PreviewBlockDrag,
    PreviewBodyDrag, PreviewTextDrag, RowPress, SelectionDrag, SelectionDragMode, SpringGate,
    TabCarry, TabPress, TerminalColumnDrag, TerminalThumbDrag, VideoPicturePress, preview,
    settings,
};
use bt_doc::Bias;
use bt_layout::{Axis, Ratio, SplitId};
use bt_viewport::ViewSelection;
use std::num::NonZeroU32;
use winit::dpi::PhysicalPosition;
use winit::event::{MouseButton, WindowEvent};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey};

/// Where every fixture's press was taken.
fn origin() -> PhysicalPosition<f64> {
    PhysicalPosition::new(40.0, 40.0)
}

/// A selection's two anchors, read off a terminal of its own: an anchor names a place in the
/// content, which is all a held selection carries.
fn a_selection() -> ViewSelection {
    let mut session =
        crate::DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
    session.feed(b"held").unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    ViewSelection {
        start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
        end: frame.anchor_at(0, 3, Bias::After).unwrap().unwrap(),
    }
}

/// What a gesture is held on: the tab in front, one of its panes with that pane's shell, a
/// preview surface, a floating window.
#[derive(Clone, Copy)]
struct On {
    tab: TabId,
    shell: PasteTarget,
    surface: PreviewSurface,
    float: float::FloatId,
}

/// **A capture of every owner kind** (§1.2's rows), held on `on`.
fn every_owner(on: On) -> Vec<CaptureOwner> {
    let latch = DragLatch::new(origin());
    let seat = on.shell.seat;
    vec![
        CaptureOwner::Divider(DividerDrag {
            split: SplitId(1),
            dir: Axis::Row,
            grip: DividerGrip::Ratio(Ratio::HALF),
        }),
        CaptureOwner::TabPress(TabPress::armed(on.tab, origin(), Instant::now())),
        CaptureOwner::PanePress(on.tab, PanePress { seat, latch }),
        CaptureOwner::RowPress(RowPress {
            host: RowHost::Column(seat),
            key: "notes.md".to_owned(),
            rect: [0.0; 4],
            latch,
        }),
        CaptureOwner::Drag(Box::new(Drag {
            source: DragSource::Tab(on.tab),
            carry: DragCarry::Tab(TabCarry {
                grab: 3.0,
                origin: 0,
                offset: 0.0,
                moved: false,
                home: on.tab,
            }),
            pointer: origin(),
            landing: None,
            paste_offer: None,
            home: None,
            spring: SpringGate::default(),
            autoscroll_ticked_at: None,
            seam: None,
        })),
        CaptureOwner::FloatHeadPress(FloatHeadPress::armed(
            on.float,
            origin(),
            [0.0, 0.0, 200.0, 100.0],
        )),
        CaptureOwner::FloatDrag(FloatDrag {
            win: on.float,
            kind: crate::FloatDragKind::Move { grab: [3.0, 3.0] },
        }),
        CaptureOwner::GlanceHeadPress(FilePeekPress { latch }),
        CaptureOwner::GlanceThumb(3.0),
        CaptureOwner::VideoBar(on.surface),
        CaptureOwner::VideoPicture(VideoPicturePress::at(on.surface, [3.0, 3.0])),
        CaptureOwner::PreviewBodyThumb(
            on.tab,
            PreviewBodyDrag {
                surface: on.surface,
                axis: preview::ScrollAxis::Vertical,
                grab: 3.0,
            },
        ),
        CaptureOwner::BlockThumb(
            on.tab,
            PreviewBlockDrag {
                surface: on.surface,
                index: 0,
                grab: 3.0,
            },
        ),
        CaptureOwner::PicturePan(
            on.tab,
            ImageDrag {
                surface: on.surface,
                last: [3.0, 3.0],
            },
        ),
        CaptureOwner::EditSelection(on.tab, on.surface),
        CaptureOwner::RenderedSelection(
            on.tab,
            PreviewTextDrag {
                surface: on.surface,
                latch,
                link: None,
                control: false,
                pressed: None,
                seated: false,
                reached: None,
            },
        ),
        CaptureOwner::TerminalThumb(on.tab, TerminalThumbDrag { seat, grab: 3.0 }),
        CaptureOwner::TerminalFootMark(on.tab, TerminalColumnDrag { seat, grab: 3.0 }),
        CaptureOwner::Route(MouseRoute::Local(Box::new(SelectionDrag {
            mode: SelectionDragMode::Linear,
            owner: on.shell,
            origin_row: 0,
            origin_column: 0,
            origin: a_selection(),
            hyperlink: None,
            hyperlink_control: false,
            local_image_activation: LocalImageActivation::None,
        }))),
        CaptureOwner::Route(MouseRoute::Forward {
            button: input::MouseProtocolButton::Left,
            sgr: true,
            owner: on.shell,
        }),
        CaptureOwner::Route(MouseRoute::MathBlock),
        CaptureOwner::SettingsSlider(settings::SettingsRow::Theme),
        CaptureOwner::SettingsMenuBar(3.0),
    ]
}

/// Every owner kind's name, in §1.2's order.
fn every_name() -> Vec<&'static str> {
    vec![
        "Divider",
        "TabPress",
        "PanePress",
        "RowPress",
        "Drag",
        "FloatHeadPress",
        "FloatDrag",
        "GlanceHeadPress",
        "GlanceThumb",
        "VideoBar",
        "VideoPicture",
        "PreviewBodyThumb",
        "BlockThumb",
        "PicturePan",
        "EditSelection",
        "RenderedSelection",
        "TerminalThumb",
        "TerminalFootMark",
        "Route::Local",
        "Route::Forward",
        "Route::MathBlock",
        "SettingsSlider",
        "SettingsMenuBar",
    ]
}

/// A window of two tabs: tab 1 in front with two shells, tab 2 with one.
fn two_tabs(tag: &str) -> HeadlessApplication {
    let folio = HeadlessFolio::with_tabs(
        tag,
        vec![cross_tab(1, &["A", "B"]), cross_tab(2, &["C"])],
        0,
    );
    let mut application = HeadlessApplication::of(folio);
    // Each tab's layout carried to its shells, as a window does for every tab it holds.
    let mut runtime = application.runtime();
    for index in [1, 0] {
        runtime.window.active_tab = index;
        runtime
            .settle_seat_set_change()
            .expect("the tab takes its layout");
    }
    application
}

/// The pane of the tab in front at `index` among its shells, as the shell a press is handed to.
fn shell(runtime: &Runtime<'_, Headless>, index: usize) -> PasteTarget {
    let (seat, leaf): (&SeatId, &LeafSession) = runtime
        .sessions
        .iter()
        .nth(index)
        .expect("the tab holds that many shells");
    PasteTarget {
        tab: runtime.id,
        seat: *seat,
        incarnation: leaf.incarnation,
    }
}

/// What the gestures of this window are held on: the tab in front, its pane `index`, that pane's
/// preview surface, and floating window 7.
fn on_pane(runtime: &Runtime<'_, Headless>, index: usize) -> On {
    let shell = shell(runtime, index);
    On {
        tab: runtime.id,
        shell,
        surface: seat_of(shell.tab, shell.seat),
        float: 7,
    }
}

/// Latch `owner` in this window, the left button holding it.
fn hold(runtime: &mut Runtime<'_, Headless>, owner: CaptureOwner) {
    runtime.latch_capture(owner, MouseButton::Left);
}

/// What the slot holds after a door ran, and the endings the doors ran since the last asking.
fn after(
    runtime: &mut Runtime<'_, Headless>,
) -> (Option<&'static str>, Vec<(&'static str, &'static str)>) {
    let held = runtime
        .app
        .pointer_capture
        .as_ref()
        .map(|capture: &PointerCapture| capture.owner.name());
    let ended = std::mem::take(&mut runtime.app.capture_endings);
    runtime.app.pointer_capture = None;
    (held, ended)
}

/// **One door, every owner kind**: each owner of `owners` is latched in turn and `door` is run;
/// an owner named in `ends` comes out of the slot with `ending`, every other one is still held.
fn each_owner_against(
    application: &mut HeadlessApplication,
    door: &str,
    ending: &'static str,
    ends: &[&str],
    owners: impl Fn(&Runtime<'_, Headless>) -> Vec<CaptureOwner>,
    run: impl FnMut(&mut HeadlessApplication),
) {
    each_owner_against_and_gone(application, door, ending, ends, &[], owners, run);
}

/// [`each_owner_against`], for a door that also takes down a surface of the window on its way,
/// whose owners in `gone_too` it ends with *owner gone* through that surface's own door.
fn each_owner_against_and_gone(
    application: &mut HeadlessApplication,
    door: &str,
    ending: &'static str,
    ends: &[&str],
    gone_too: &[&str],
    owners: impl Fn(&Runtime<'_, Headless>) -> Vec<CaptureOwner>,
    mut run: impl FnMut(&mut HeadlessApplication),
) {
    let count = owners(&application.runtime()).len();
    for which in 0..count {
        let owner = owners(&application.runtime()).swap_remove(which);
        let name = owner.name();
        hold(&mut application.runtime(), owner);
        run(application);
        let (held, ended) = after(&mut application.runtime());
        let expected = if ends.contains(&name) {
            (None, vec![(name, ending)])
        } else if gone_too.contains(&name) {
            (None, vec![(name, "owner gone")])
        } else {
            (Some(name), Vec::new())
        };
        assert_eq!((held, ended), expected, "{door} and a held {name}");
    }
}

/// The owner kinds whose payload names a tab-bound surface (R-9's *off the glass*).
const TAB_BOUND: [&str; 15] = [
    "Divider",
    "PanePress",
    "RowPress",
    "VideoBar",
    "VideoPicture",
    "PreviewBodyThumb",
    "BlockThumb",
    "PicturePan",
    "EditSelection",
    "RenderedSelection",
    "TerminalThumb",
    "TerminalFootMark",
    "Route::Local",
    "Route::Forward",
    "Route::MathBlock",
];

/// The gestures held on a preview surface's document.
const ON_A_DOCUMENT: [&str; 7] = [
    "VideoBar",
    "VideoPicture",
    "PreviewBodyThumb",
    "BlockThumb",
    "PicturePan",
    "EditSelection",
    "RenderedSelection",
];

/// RED (T-POINTER-CAPTURE cut 5, R-9) — **each lifecycle door ends a held capture**, with its
/// own ending, for every owner kind it takes away and for no other: §4.1 cut 5's matrix, door by
/// door, run on a headless window as the product runs it — the blur through the window event
/// dispatcher, the system taking the pointer away through the turn's resize door, the scale, the
/// summoned window's hide, the close, the tab switch, a tab and a pane closed, a zoom, a surface
/// swept, a buffer left, a document re-parsed, a floating window dismissed, the glance card
/// hidden, the settings sheet shut (by the gear and by `Esc`).
///
/// MUTATION: give `activate_tab` the owner-gone ending (`OwnerGone(Gone::Tab(self.id))`) instead
/// of its own — the tab switch's row reads `owner gone` where `off the glass` is owed, and a
/// press handed to a program is no longer told its button came up.
#[test]
fn each_door_ends_a_held_capture() {
    let all = every_name();
    let mut application = two_tabs("ending-doors");

    // The blur, through the dispatcher.
    each_owner_against(
        &mut application,
        "a blur",
        "cancel",
        &all,
        |runtime| every_owner(on_pane(runtime, 0)),
        |application| application.window_event(WindowEvent::Focused(false)),
    );

    // The system took the pointer: the capture's sample is a window that no longer holds it.
    each_owner_against(
        &mut application,
        "the resize door",
        "cancel",
        &all,
        |runtime| every_owner(on_pane(runtime, 0)),
        |application| {
            let mut runtime = application.runtime();
            if let Some(capture) = runtime.app.pointer_capture.as_mut() {
                capture.os = Some(bt_platform::NativeWindow::stand_in(0x1234));
            }
            runtime
                .flush_pending_pty_resize(Instant::now())
                .expect("the turn's resize door runs");
        },
    );
    // And a capture the system still holds is left alone by the same door.
    let owner = every_owner(on_pane(&application.runtime(), 0)).swap_remove(0);
    hold(&mut application.runtime(), owner);
    application
        .runtime()
        .flush_pending_pty_resize(Instant::now())
        .expect("the turn's resize door runs");
    assert_eq!(
        after(&mut application.runtime()),
        (Some("Divider"), Vec::new()),
        "a capture whose sample still holds is a hand still on it"
    );

    // A scale change: the owners holding a pixel quantity taken at the press.
    each_owner_against(
        &mut application,
        "a scale change",
        "scale change",
        &[
            "TabPress",
            "PanePress",
            "RowPress",
            "Drag",
            "FloatHeadPress",
            "FloatDrag",
            "GlanceHeadPress",
            "GlanceThumb",
            "PreviewBodyThumb",
            "BlockThumb",
            "PicturePan",
            "RenderedSelection",
            "TerminalThumb",
            "TerminalFootMark",
            "SettingsMenuBar",
            "VideoPicture",
        ],
        |runtime| every_owner(on_pane(runtime, 0)),
        |application| {
            let mut runtime = application.runtime();
            runtime
                .scale_factor_changed()
                .expect("the scale change is answered");
            // And the rectangle that follows a scale change, as the system sends it.
            let size = runtime.window.window.inner_size();
            runtime.resized(size).expect("the rectangle arrives");
        },
    );

    // The summoned window sent away.
    each_owner_against(
        &mut application,
        "the summoned window's hide",
        "cancel",
        &all,
        |runtime| every_owner(on_pane(runtime, 0)),
        |application| {
            application
                .runtime()
                .hide_quake_window()
                .expect("the window is sent away");
        },
    );

    // The tab switch: every tab-bound gesture of the tab in front, and nothing of the window's.
    // (The switch also takes the glance card down, whose own door lets go of a hand on it.)
    each_owner_against_and_gone(
        &mut application,
        "a tab switch",
        "off the glass",
        &TAB_BOUND,
        &["GlanceHeadPress", "GlanceThumb"],
        |runtime| every_owner(on_pane(runtime, 0)),
        |application| {
            let mut runtime = application.runtime();
            runtime
                .activate_tab(1, false)
                .expect("tab 2 comes in front");
            runtime.activate_tab(0, false).expect("and tab 1 again");
        },
    );

    // A zoom: the gestures held on the pane that goes out of view.
    each_owner_against(
        &mut application,
        "a zoom",
        "off the glass",
        &TAB_BOUND[..14],
        |runtime| every_owner(on_pane(runtime, 1)),
        |application| {
            let mut runtime = application.runtime();
            let kept = shell(&runtime, 0).seat;
            runtime
                .toggle_pane_zoom(kept)
                .expect("the first pane is zoomed");
            runtime
                .toggle_pane_zoom(kept)
                .expect("and let go of again, which takes nothing away");
        },
    );

    // A surface the sweep finds gone, a buffer left, a document re-parsed.
    let gone = |runtime: &Runtime<'_, Headless>| {
        let mut on = on_pane(runtime, 0);
        on.surface = seat_of(on.tab, SeatId(99));
        every_owner(on)
    };
    each_owner_against(
        &mut application,
        "the sweep",
        "owner gone",
        &ON_A_DOCUMENT,
        gone,
        |application| application.runtime().sweep_preview_panes(),
    );
    each_owner_against(
        &mut application,
        "a buffer left",
        "owner gone",
        &ON_A_DOCUMENT,
        |runtime| every_owner(on_pane(runtime, 0)),
        |application| {
            let mut runtime = application.runtime();
            let surface = on_pane(&runtime, 0).surface;
            runtime.leave_preview_buffer_in(0, surface);
        },
    );

    // A floating window dismissed, the glance card hidden, the settings sheet shut.
    let floating = |runtime: &Runtime<'_, Headless>| {
        let mut on = on_pane(runtime, 0);
        on.surface = PreviewSurface::Float(on.float);
        let mut owners = every_owner(on);
        owners[3] = CaptureOwner::RowPress(RowPress {
            host: RowHost::Float(on.float),
            key: "notes.md".to_owned(),
            rect: [0.0; 4],
            latch: DragLatch::new(origin()),
        });
        owners
    };
    each_owner_against(
        &mut application,
        "a floating window dismissed",
        "owner gone",
        &[
            "RowPress",
            "FloatHeadPress",
            "FloatDrag",
            "VideoBar",
            "VideoPicture",
            "PreviewBodyThumb",
            "BlockThumb",
            "PicturePan",
            "EditSelection",
            "RenderedSelection",
        ],
        floating,
        |application| {
            application
                .runtime()
                .dismiss_float(7)
                .expect("the window is dismissed");
        },
    );
    let on_the_card = |runtime: &Runtime<'_, Headless>| {
        let mut on = on_pane(runtime, 0);
        on.surface = PreviewSurface::Peek;
        every_owner(on)
    };
    each_owner_against(
        &mut application,
        "the glance card hidden",
        "owner gone",
        &[
            "GlanceHeadPress",
            "GlanceThumb",
            "VideoBar",
            "VideoPicture",
            "PreviewBodyThumb",
            "BlockThumb",
            "PicturePan",
            "EditSelection",
            "RenderedSelection",
        ],
        on_the_card,
        |application| {
            application.runtime().hide_file_peek();
        },
    );
    each_owner_against(
        &mut application,
        "the settings sheet shut by the gear",
        "owner gone",
        &["SettingsSlider", "SettingsMenuBar"],
        |runtime| every_owner(on_pane(runtime, 0)),
        |application| {
            let mut runtime = application.runtime();
            runtime
                .toggle_settings_panel()
                .expect("the gear opens the sheet");
            runtime.toggle_settings_panel().expect("and shuts it");
        },
    );
    each_owner_against(
        &mut two_tabs("ending-doors-esc"),
        "the settings sheet shut by Esc",
        "owner gone",
        &["SettingsSlider", "SettingsMenuBar"],
        // Under the sheet nothing but the sheet's own gestures can be held (and `Esc` cancels a
        // drag or a divider before it reaches a sheet).
        |runtime| {
            every_owner(on_pane(runtime, 0))
                .into_iter()
                .filter(|owner| owner.name().starts_with("Settings"))
                .collect()
        },
        |application| {
            let mut runtime = application.runtime();
            runtime
                .toggle_settings_panel()
                .expect("the gear opens the sheet");
            runtime
                .publish_frame(bt_render::FrameTrigger {
                    occurred_at: Instant::now(),
                    source: bt_render::FrameSource::Expose,
                })
                .expect("the sheet is drawn");
            runtime.redraw().expect("and reaches the glass");
            runtime
                .keyboard_input(
                    &key_down(Key::Named(NamedKey::Escape), KeyCode::Escape, None),
                    false,
                )
                .expect("Esc shuts it");
            assert!(
                !runtime.window.settings.is_open(),
                "Esc shut the sheet (drawn: {})",
                runtime.settings_layout().is_some()
            );
        },
    );

    // A pane closed: the gestures held on it, on a window of its own each time.
    let held_on_the_pane = [
        "PanePress",
        "RowPress",
        "VideoBar",
        "VideoPicture",
        "PreviewBodyThumb",
        "BlockThumb",
        "PicturePan",
        "EditSelection",
        "RenderedSelection",
        "TerminalThumb",
        "TerminalFootMark",
        "Route::Local",
        "Route::Forward",
    ];
    for which in 0..all.len() {
        let mut application = two_tabs("ending-doors-pane");
        let owner = every_owner(on_pane(&application.runtime(), 1)).swap_remove(which);
        let name = owner.name();
        hold(&mut application.runtime(), owner);
        let mut runtime = application.runtime();
        let closing = shell(&runtime, 1).seat;
        runtime.close_pane(closing).expect("the pane closes");
        let expected = if held_on_the_pane.contains(&name) {
            (None, vec![(name, "owner gone")])
        } else if name == "Divider" {
            // Not the pane's: the seat set's settle drops a divider whose layout changed, as it
            // always has (cell C1·f, unchanged).
            (None, Vec::new())
        } else {
            (Some(name), Vec::new())
        };
        assert_eq!(
            after(&mut runtime),
            expected,
            "a pane closed and a held {name}"
        );
    }

    // A tab closed: everything that names it or is held on it.
    let mut tabs = HeadlessApplication::of(HeadlessFolio::with_tabs(
        "ending-doors-tabs",
        (1..=all.len() as u64 + 1)
            .map(|id| cross_tab(id, &["A"]))
            .collect(),
        0,
    ));
    each_owner_against(
        &mut tabs,
        "a tab closed",
        "owner gone",
        &[
            "Divider",
            "TabPress",
            "PanePress",
            "RowPress",
            "Drag",
            "VideoBar",
            "VideoPicture",
            "PreviewBodyThumb",
            "BlockThumb",
            "PicturePan",
            "EditSelection",
            "RenderedSelection",
            "TerminalThumb",
            "TerminalFootMark",
            "Route::Local",
            "Route::Forward",
            "Route::MathBlock",
            // And the glance card, which the tab that comes in front takes down by its own door.
            "GlanceHeadPress",
            "GlanceThumb",
        ],
        |runtime| every_owner(on_pane(runtime, 0)),
        |application| {
            let mut runtime = application.runtime();
            let active = runtime.window.active_tab;
            runtime.close_tab(active).expect("the tab closes");
        },
    );

    // A window closed, on a window of its own each time.
    for which in 0..all.len() {
        let mut application = two_tabs("ending-doors-close");
        let owner = every_owner(on_pane(&application.runtime(), 0)).swap_remove(which);
        let name = owner.name();
        hold(&mut application.runtime(), owner);
        application
            .runtime()
            .close_window(false)
            .expect("the window closes");
        assert_eq!(
            after(&mut application.runtime()),
            (None, vec![(name, "cancel")]),
            "closing the window ends a held {name} with a cancel"
        );
    }
}

/// RED (cut 5, R-9) — **a re-parse retires the block thumb, and only it**: a new parse renumbers
/// the blocks, so a thumb held on one is let go of, while a selection drawn across the same page
/// holds its span through it (owner's ruling 2026-09-23).
///
/// MUTATION: end the capture with `Gone::Surface(surface)` in `rebuild_preview_document` — the
/// selection is let go of too.
#[test]
fn a_new_parse_lets_go_of_the_block_thumb_and_keeps_the_selection() {
    const PAGE: &str = "# Title\n\nfirst paragraph here.\n";
    let path = host_spelling(r"C:\w\notes.md");
    let a_page = || tab_with_a_preview(1, vec![buffer_saying(&path, "notes.md", PAGE)]);
    let surface = seat_of(TabId(1), a_page().1);
    for (owner, ended) in [
        (
            CaptureOwner::BlockThumb(
                TabId(1),
                PreviewBlockDrag {
                    surface,
                    index: 0,
                    grab: 3.0,
                },
            ),
            true,
        ),
        (
            CaptureOwner::RenderedSelection(
                TabId(1),
                PreviewTextDrag {
                    surface,
                    latch: DragLatch::new(origin()),
                    link: None,
                    control: false,
                    pressed: None,
                    seated: false,
                    reached: None,
                },
            ),
            false,
        ),
    ] {
        let mut folio = HeadlessFolio::with_tabs("ending-reparse", vec![a_page().0], 0);
        folio.draw();
        let mut runtime = folio.runtime();
        let name = owner.name();
        hold(&mut runtime, owner);
        // Whatever made the document's key differ — a byte typed, a face flipped — is a new
        // parse; the key forgotten is the shortest road to one.
        runtime.preview_pane_mut(surface).key_document(None);
        runtime.rebuild_preview_document(surface, [0.0, 0.0, 600.0, 400.0], 1.0);
        let expected = if ended {
            (None, vec![(name, "owner gone")])
        } else {
            (Some(name), Vec::new())
        };
        assert_eq!(
            after(&mut runtime),
            expected,
            "a new parse and a held {name}"
        );
    }
}

/// A shell still being born, which keeps what it is sent (`PaneBirth::typed`).
fn a_shell_being_born() -> LeafSession {
    let mut leaf = crate::test_support::leaf_saying("SHELL");
    leaf.birth = Some(crate::PaneBirth {
        seed: crate::LeafSeed {
            profile: crate::profiles::DEFAULT_IDENTITY.to_owned(),
            ..crate::LeafSeed::default()
        },
        probe_input: None,
        typed: std::cell::RefCell::default(),
        waiting: crate::BirthWait::Programs {
            _waits: crate::programs_lane::pane_waits(),
        },
    });
    leaf
}

/// RED (cut 5, R-9, Q2's recommendation) — **a press handed to a program is told its button came
/// up when its gesture ends without the release**: at the last cell it was told about, in its
/// press's encoding, with the modifiers of that report — a release a `Ctrl+Tab` caused does not
/// say `Ctrl`. A tab switch and a blur both tell it, before the tab's seats change. (A shell that
/// is gone is owed nothing: that ending sends nothing by its type, [`OwnerGone`].)
///
/// MUTATION: send the release with `self.window.modifiers` in
/// `tell_the_program_its_button_came_up` — the release says `Ctrl` (`ESC [<16;3;2m`).
#[test]
fn a_forwarded_press_is_told_its_button_came_up_from_its_last_report() {
    for door in ["a tab switch", "a blur"] {
        let mut application = HeadlessApplication::of(HeadlessFolio::with_tabs(
            "ending-forward",
            vec![
                crate::test_support::tab_holding(a_shell_being_born()),
                cross_tab(2, &["C"]),
            ],
            0,
        ));
        let mut runtime = application.runtime();
        let owner = shell(&runtime, 0);
        hold(
            &mut runtime,
            CaptureOwner::Route(MouseRoute::Forward {
                button: input::MouseProtocolButton::Left,
                sgr: true,
                owner,
            }),
        );
        runtime.note_the_last_report_sent(
            bt_render::GridHit { row: 1, column: 2 },
            ModifiersState::empty(),
        );
        runtime.window.modifiers = ModifiersState::CONTROL;
        if door == "a tab switch" {
            runtime
                .activate_tab(1, false)
                .expect("tab 2 comes in front");
        } else {
            application.window_event(WindowEvent::Focused(false));
        }
        let runtime = application.runtime();
        let leaf = runtime.window.tabs[0]
            .sessions
            .values()
            .next()
            .expect("the shell");
        let typed = String::from_utf8(
            leaf.birth
                .as_ref()
                .expect("in birth")
                .typed
                .borrow()
                .clone(),
        )
        .expect("UTF-8 was sent");
        assert_eq!(
            typed, "\x1b[<0;3;2m",
            "{door}: the program hears its button come up"
        );
    }
}

/// RED (cut 5, R-9, cell C1·g; round 2) — **a divider held when the tab switches puts its ratio
/// back first**: off the glass is a cancel for a divider, read off the tab whose seats it moved
/// before the tab in front changes, so the split comes back to the ratio it had when the press
/// began.
///
/// MUTATION: drop the divider from `leaves_the_glass` — the switch leaves the ratio the hand
/// dragged it to.
#[test]
fn a_tab_switch_puts_a_held_dividers_ratio_back() {
    let mut application = two_tabs("ending-divider-ratio");
    let mut runtime = application.runtime();
    let (split, dragged) = runtime.seats.tree().ratios()[0];
    let at_the_press = Ratio::from_ppm(if dragged == Ratio::HALF {
        300_000
    } else {
        500_000
    })
    .expect("a legal ratio");
    hold(
        &mut runtime,
        CaptureOwner::Divider(DividerDrag {
            split,
            dir: Axis::Row,
            grip: DividerGrip::Ratio(at_the_press),
        }),
    );
    runtime
        .activate_tab(1, false)
        .expect("tab 2 comes in front");
    runtime.activate_tab(0, false).expect("and tab 1 again");
    let ratio = runtime
        .seats
        .tree()
        .ratios()
        .into_iter()
        .find_map(|(id, ratio)| (id == split).then_some(ratio));
    assert_eq!(
        ratio,
        Some(at_the_press),
        "the split is back at the ratio it had when the press began"
    );
}
