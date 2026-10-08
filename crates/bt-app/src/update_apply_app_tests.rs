//! **`update_apply`, as the application drives it.** Tests whose first assertion is about
//! `update_apply`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use std::time::Duration;

/// PIN (ticket #62) — **the menu's `Paste` is the keyboard's paste, and
/// there is only one of it.**
///
/// What the row spends is `paste_from_clipboard_into`, which is what
/// `Ctrl+V` spends with the focused seat filled in — so the thing worth
/// pinning is the door itself, exercised here on the four promises every
/// caller inherits: the bytes are bracketed when the shell asked for
/// bracketing, `\r\n` is normalised the way a terminal delivers it, the
/// selection goes, and the view comes back to the bottom. A second paste
/// path would be a second place for the multi-line policy to be decided when
/// it lands (P2-6), which is the whole reason there is one.
///
/// MUTATION: give the menu its own writer and the bracketing promise is the
/// first thing to drift — a `Paste` that skipped `ESC [ 200 ~` hands `bash`
/// a multi-line paste it runs a line at a time.
#[test]
fn the_menus_paste_is_the_keyboards_paste_door_and_leaves_the_view_at_the_bottom() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
    session
        .feed(b"\x1b[?2004hone\r\ntwo\r\nthree\r\nfour")
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let selection = ViewSelection {
        start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
        end: frame.anchor_at(0, 2, Bias::After).unwrap().unwrap(),
    };
    session.set_view_selection(Some(selection.clone()));
    projection.set_selection(Some(selection));
    projection.scroll_by_subpixels(2 * projection.cell_height_subpixels().get());
    assert!(
        projection.is_scrolled(),
        "the fixture has to be reading history or the return to the bottom proves nothing"
    );

    let mut written = Vec::new();
    paste_text(&mut session, &mut projection, "a\r\nb\n", |chunk| {
        written.extend_from_slice(chunk);
        Ok(())
    })
    .unwrap();

    assert_eq!(
        written, b"\x1b[200~a\rb\r\x1b[201~",
        "bracketed because the shell asked, with the breaks a terminal delivers"
    );
    assert!(session.view_selection().is_none());
    assert!(projection.selection().is_none());
    assert!(
        !projection.is_scrolled(),
        "typing returns the view to the live bottom, and a paste is typing"
    );
}

/// RED (A1d, revision (c)8 item 9) — **every owner-thread door's function takes its own door's
/// token, by value, first.**
///
/// Checked by coercion, where the compiler reads the signature: each line below compiles only if
/// the function's first parameter (after `self`) is `WaitToken<'_, doors::<its door>>`. The two
/// generic doors (`present_frame_with_phases`, `launch_wire::hand_over`) are called from a
/// function whose own signature says the same. The session writer's two doors are private to
/// `persist` and are checked in its tests.
///
/// MUTATION: give any door another door's token type (`owner_door::set_title` taking
/// `WaitToken<'_, doors::SetCursor>`), or take the token off it, and this does not compile.
#[test]
#[expect(
    clippy::type_complexity,
    reason = "each coercion spells one door's whole signature, which is what it checks"
)]
fn every_owner_door_takes_its_own_token_by_value() {
    use bt_platform::admission::{WaitToken, doors};
    use bt_platform::{Compositor, NativeWindow, RehostOutcome, RehostSide, WebHost};
    use bt_render::{
        FrameTrigger, GpuContext, PresentOutcome, RenderError, SeatFrame, WindowRenderer,
        WindowTarget,
    };
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    let _: fn(
        WaitToken<'_, doors::PtyBirth>,
        OsString,
        &[OsString],
        bool,
        shell_integration::EnvironmentDerivation,
        &[(OsString, OsString)],
        &[(OsString, OsString)],
        PtySize,
        OutputWake,
        Option<PathBuf>,
    ) -> Result<PtySession, PtyError> = pty_door::spawn_shell;
    let _: fn(WaitToken<'_, doors::PtyResize>, &mut PtySession, PtySize) -> Result<(), PtyError> =
        pty_door::resize;
    let _: fn(WaitToken<'_, doors::PaneRetirementWait>, Duration) -> usize =
        pty_door::wait_for_retirements;
    let _: fn(
        WaitToken<'_, doors::GpuOpen>,
        WindowTarget,
        u32,
        u32,
        f64,
    ) -> Result<(GpuContext, WindowRenderer), RenderError> = gpu_door::open_first_window;
    let _: fn(WaitToken<'_, doors::TitleFlush>, &Window, &str) = owner_door::set_title;
    let _: fn(WaitToken<'_, doors::ImeCaretArea>, &Window, winit::dpi::Position, winit::dpi::Size) =
        owner_door::set_ime_cursor_area;
    let _: fn(WaitToken<'_, doors::FocusWindow>, &Window) = owner_door::focus_window;
    let _: fn(WaitToken<'_, doors::SetVisible>, &Window, bool) = owner_door::set_visible;
    let _: fn(WaitToken<'_, doors::SetCursor>, &Window, winit::window::Cursor) =
        owner_door::set_cursor;
    let _: fn(WaitToken<'_, doors::PlaceHidden>, &Window) -> bool = window_is_hidden;
    let _: fn(WaitToken<'_, doors::PlaceExposure>, &Window) -> bool = window_is_exposed;
    let _: fn(WaitToken<'_, doors::TraceFlush>) = trace_sink::flush;
    let _: fn(WaitToken<'_, doors::UpdateLeave>) -> Option<crate::update_apply::Left> =
        crate::update_handoff::leave_armed;
    let _: fn(&Compositor, WaitToken<'_, doors::CompositorCommit>) -> Result<(), String> =
        Compositor::commit;
    let _: fn(WaitToken<'_, doors::CompositorBirth>, NativeWindow) -> Result<Compositor, String> =
        Compositor::new;
    let _: fn(
        WaitToken<'_, doors::CompositorBirth>,
    ) -> Result<Option<bt_platform::SpareParent>, String> = bt_platform::spare_parent;
    let _: fn(
        &Compositor,
        WaitToken<'_, doors::CompositorWindowSize>,
        u32,
        u32,
    ) -> Result<(), String> = Compositor::set_window_size;
    let _: fn(
        &mut WebHost,
        WaitToken<'_, doors::WebController>,
        NativeWindow,
        u64,
    ) -> Result<(), String> = WebHost::request_controller;
    let _: fn(
        &mut WebHost,
        WaitToken<'_, doors::WebEnvironment>,
        &Path,
        u64,
    ) -> Result<(), String> = WebHost::request_environment;
    let _: fn(
        &mut WebHost,
        WaitToken<'_, doors::WebRehost>,
        &RehostSide<'_>,
        &RehostSide<'_>,
        (i32, i32, u32, u32),
        bool,
    ) -> RehostOutcome = WebHost::rehost;
    let _: fn(
        WaitToken<'_, doors::FontFamilyLookup>,
        &str,
    ) -> Option<bt_platform::MonospaceFamily> = bt_platform::monospace_family_named;
    let _: fn(
        WaitToken<'_, doors::SurfaceBirth>,
        &mut GpuContext,
        WindowTarget,
        u32,
        u32,
        f64,
    ) -> Result<WindowRenderer, RenderError> = WindowRenderer::new;
    fn presents(
        renderer: &mut WindowRenderer,
        token: WaitToken<'_, doors::PresentFrame>,
        gpu: &mut GpuContext,
        seats: &[SeatFrame<'_>],
        trigger: FrameTrigger,
    ) -> Result<PresentOutcome, RenderError> {
        renderer.present_frame_with_phases(token, gpu, seats, trigger, |_| {})
    }
    let _ = presents;
    fn hands_over(
        token: WaitToken<'_, doors::LaunchHandOver>,
        admitted: &update_startup::Admitted,
        directory: &Path,
        argv: &cli::CliRequest,
    ) -> Option<i32> {
        launch_wire::hand_over(token, admitted, directory, argv, |_| {})
    }
    let _ = hands_over;
}
