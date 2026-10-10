//! **`update_prepare_windows`, as the application drives it.** Tests whose first assertion is about
//! `update_prepare_windows`, written in the crate root's scope rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

// ── A1d: every owner-thread door takes a token (design note 2026-09-26, revision (e)2 as
//    corrected by (f)1) ──────────────────────────────────────────────────────────────────────

/// **One owner-thread door, asked from a worker and in every phase of the window thread.**
///
/// A thread the thread door started (A1b's real spawner) is refused with the door's name and
/// its work does not run; on a thread that entered as the window thread the door is admitted —
/// and its work runs — in exactly `phases`, and in every other phase it is refused, with that
/// phase named, and its work does not run.
fn a_door_answers_by_role_and_phase<D: bt_platform::admission::Door>(
    phases: &'static [bt_platform::admission::Phase],
) {
    use bt_platform::admission::{Phase, Refused, Role, admitted};
    const WORKER: &str = "bt-test-owner-door";
    let door = D::KEY.name();
    let on_a_worker =
        bt_platform::spawn_at_priority(WORKER, bt_platform::ThreadPriority::BelowNormal, |_ctx| {
            let mut ran = false;
            let answer = admitted::<D, _>(|_token| ran = true);
            (answer, ran)
        })
        .expect("a worker through the thread door")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    assert_eq!(
        on_a_worker,
        (
            Err(Refused {
                door,
                role: Role::Worker(WORKER),
                phase: None,
            }),
            false
        ),
        "`{door}` is refused on a worker, by its own name, and does nothing there"
    );
    std::thread::spawn(move || {
        assert!(bt_platform::admission::enter_window_thread());
        for phase in [Phase::Starting, Phase::Running, Phase::Exiting] {
            match phase {
                Phase::Starting => {}
                Phase::Running => assert!(bt_platform::admission::loop_running()),
                Phase::Exiting => assert!(bt_platform::admission::exiting()),
            }
            let mut ran = false;
            let answer = admitted::<D, _>(|_token| ran = true);
            if phases.contains(&phase) {
                assert_eq!(
                    (answer, ran),
                    (Ok(()), true),
                    "`{door}` is admitted, and runs, on the window thread in {phase:?}"
                );
            } else {
                assert_eq!(
                    (answer, ran),
                    (
                        Err(Refused {
                            door,
                            role: Role::Window,
                            phase: Some(phase),
                        }),
                        false
                    ),
                    "`{door}` is refused on the window thread in {phase:?}, and does nothing"
                );
            }
        }
    })
    .join()
    .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

/// RED (A1d) — **every owner-thread door is refused on a worker by its own name, and is admitted
/// on the window thread in exactly the phases the design note's door table gives it.**
///
/// The phases are written here as the table (revision (e)2, with (f)1's `Exiting` for
/// `WebController`) writes them, not read back from the types, so a door whose phase set drifts
/// from the table goes red by name. What this adds to A1a's and A1b's probes is the claim per
/// door, through the real thread door, for the doors the product now reaches only inside an
/// admission.
///
/// MUTATION: widen a door's phases in `bt_platform::admission::doors` (`CompositorBirth` to
/// `[Running, Exiting]`), or narrow one (`WebController` back to `[Running]`), and this names it.
#[test]
fn every_owner_door_is_refused_on_a_worker_and_admitted_only_in_its_phases() {
    use bt_platform::admission::Phase::{Exiting, Running, Starting};
    use bt_platform::admission::doors;
    a_door_answers_by_role_and_phase::<doors::FontFamilyLookup>(&[Running]);
    a_door_answers_by_role_and_phase::<doors::PresentFrame>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::CompositorCommit>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::CompositorBirth>(&[Running]);
    a_door_answers_by_role_and_phase::<doors::CompositorWindowSize>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::SurfaceBirth>(&[Running]);
    a_door_answers_by_role_and_phase::<doors::PtyResize>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::PlaceHidden>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::PlaceExposure>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::TitleFlush>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::PaneRetirementWait>(&[Exiting]);
    a_door_answers_by_role_and_phase::<doors::SessionWriteWait>(&[Exiting]);
    a_door_answers_by_role_and_phase::<doors::SessionWriterRetire>(&[Exiting]);
    a_door_answers_by_role_and_phase::<doors::TraceFlush>(&[Exiting]);
    a_door_answers_by_role_and_phase::<doors::UpdateLeave>(&[Exiting]);
    // Desktop retirement waits only after the event loop enters Exiting.
    a_door_answers_by_role_and_phase::<doors::DesktopRetire>(&[Exiting]);
    a_door_answers_by_role_and_phase::<doors::PreviewRecoveryCopies>(&[Starting, Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::LaunchHandOver>(&[Starting]);
    a_door_answers_by_role_and_phase::<doors::WebController>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::WebEnvironment>(&[Running]);
    a_door_answers_by_role_and_phase::<doors::WebRehost>(&[Running]);
    a_door_answers_by_role_and_phase::<doors::ImeCaretArea>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::GpuOpen>(&[Running]);
    a_door_answers_by_role_and_phase::<doors::FocusWindow>(&[Running]);
    a_door_answers_by_role_and_phase::<doors::SetVisible>(&[Running, Exiting]);
    a_door_answers_by_role_and_phase::<doors::SetCursor>(&[Running]);
    assert_eq!(
        doors::ALL.len(),
        26,
        "a door added to the registry is a door this list has to name"
    );
}
