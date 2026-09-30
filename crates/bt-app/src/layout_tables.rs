//! The process-wide keyboard-layout Shift tables and their worker road.
//!
//! A key event may ask only the in-memory map. Registry reads and layout-DLL
//! loads belong to `folio-layout-tables`, which starts with every HKL returned
//! by `GetKeyboardLayoutList` and accepts a nonblocking request when a later
//! `GetKeyboardLayout(0)` names an HKL the map has not seen. Answers are
//! published before `AppEvent::LayoutTablesReady`; the next lookup drains too,
//! so a lost wake cannot strand a table.
//!
//! **One terminal policy.** Every HKL the map has seen ends in exactly one of
//! two answers, `Known(table)` or `Unavailable`. `Unavailable` is reached when
//! the platform door answers `None` (a layout without readable tables), when a
//! request is refused because [`REQUEST_CAPACITY`] requests are already
//! unanswered, or when the worker is gone (a request refused as
//! `Disconnected`, or the answer channel reporting `Disconnected`, which turns
//! every pending HKL `Unavailable` at once). An `Unavailable` HKL is never
//! asked again, and its chords encode the un-shifted character until the
//! process ends. The first time an HKL becomes `Unavailable` for a reason
//! other than the door's `None`, one diagnostics note names the layout and the
//! reason.
//!
//! **No layouts, no worker.** An empty startup list — always the answer off
//! Windows, where there are no Win32 keyboard layouts — makes an inert
//! `LayoutTables`: no thread, no channels, and every lookup `Unavailable`
//! without a request.

use crate::AppEvent;
use crate::input::ShiftedCharacter;
use bt_platform::{KeyboardLayout, KeyboardLayoutShiftTable};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use winit::event_loop::EventLoopProxy;

/// How many miss requests may be admitted and unanswered at once. The window
/// end counts them itself, so the request queue, of the same size, is never
/// the one that refuses.
const REQUEST_CAPACITY: usize = 8;

#[derive(Clone)]
struct Answer {
    handle: usize,
    table: Option<KeyboardLayoutShiftTable>,
}

/// What the map holds for one HKL.
enum Slot {
    /// The worker has the job and its answer is not adopted yet. `requested`
    /// says it was admitted as a miss request and so counts against
    /// [`REQUEST_CAPACITY`]; a startup job does not.
    Pending {
        layout: KeyboardLayout,
        requested: bool,
    },
    /// Boxed: a table is a kilobyte, the other two variants are words.
    Known(Box<KeyboardLayoutShiftTable>),
    /// Terminal: never asked again; the chord's `k` is the un-shifted character.
    Unavailable,
}

/// Why an HKL became `Unavailable` without the door having answered `None`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Refusal {
    /// [`REQUEST_CAPACITY`] requests were admitted and not yet answered.
    RequestsFull,
    /// The worker's end of either channel is gone.
    WorkerGone,
    /// The startup list was empty, so no worker was started.
    NoWorker,
}

impl Refusal {
    const fn reason(self) -> &'static str {
        match self {
            Self::RequestsFull => "the request queue is full",
            Self::WorkerGone => "the layout-table worker has stopped",
            Self::NoWorker => "no keyboard layout was loaded at startup, so no worker was started",
        }
    }
}

/// The window's two channel ends while the worker is there, and why nothing
/// is offered when it is gone or was never started.
enum Road {
    Open {
        requests: SyncSender<KeyboardLayout>,
        answers: Receiver<Answer>,
    },
    Closed(Refusal),
}

/// The window end: the per-HKL map, the count of admitted and unanswered
/// requests, and the two channel ends it alone uses. One instance lives on
/// `App`, so all windows ask one cache and no window can drain another
/// window's answer.
pub(crate) struct LayoutTables {
    road: Road,
    slots: HashMap<usize, Slot>,
    requests_in_flight: usize,
    note: Box<dyn Fn(&str)>,
}

impl LayoutTables {
    /// Start the worker on the layouts loaded at startup. `startup` is the
    /// platform's answer, and an empty one is the decision that there is no
    /// worker: off Windows it is always empty.
    pub(crate) fn spawn(
        proxy: EventLoopProxy<AppEvent>,
        startup: Vec<KeyboardLayout>,
    ) -> std::io::Result<Self> {
        Self::start(
            startup,
            Box::new(move || {
                let _ = proxy.send_event(AppEvent::LayoutTablesReady);
            }),
            Box::new(crate::diagnostics::note),
            |worker| {
                bt_platform::spawn_at_priority(
                    "folio-layout-tables",
                    bt_platform::ThreadPriority::BelowNormal,
                    move |worker_ctx| worker.run(worker_ctx),
                )
                .map(drop)
            },
        )
    }

    /// [`Self::spawn`] with the thread start as a seam: an empty startup list
    /// returns before the road exists and `start` is never called.
    fn start(
        startup: Vec<KeyboardLayout>,
        wake: Box<dyn Fn() + Send>,
        note: Box<dyn Fn(&str)>,
        start: impl FnOnce(WorkerRoad) -> std::io::Result<()>,
    ) -> std::io::Result<Self> {
        if startup.is_empty() {
            return Ok(Self {
                road: Road::Closed(Refusal::NoWorker),
                slots: HashMap::new(),
                requests_in_flight: 0,
                note,
            });
        }
        let (tables, worker) = road(startup, wake, note);
        start(worker)?;
        Ok(tables)
    }

    /// Adopt every answer already published. Called by the worker's event and
    /// before each lookup, which is the lane contract's lost-wake recovery.
    /// `Disconnected` after the last answer means the worker is gone: every HKL
    /// still pending becomes `Unavailable`.
    pub(crate) fn apply_answers(&mut self) -> usize {
        let mut applied = 0;
        loop {
            let Road::Open { answers, .. } = &self.road else {
                return applied;
            };
            match answers.try_recv() {
                Ok(answer) => {
                    self.adopt(answer);
                    applied += 1;
                }
                Err(TryRecvError::Empty) => return applied,
                Err(TryRecvError::Disconnected) => {
                    self.close(Refusal::WorkerGone);
                    return applied;
                }
            }
        }
    }

    /// The Shift character this HKL's table gives the virtual key. An HKL the
    /// map has not seen is offered to the worker without waiting and answers
    /// `Pending`, or `Unavailable` at once when the offer is refused.
    pub(crate) fn shifted_character(
        &mut self,
        layout: KeyboardLayout,
        virtual_key: u16,
    ) -> ShiftedCharacter {
        self.apply_answers();
        match self.slots.get(&layout.handle()) {
            Some(Slot::Known(table)) => ShiftedCharacter::Known(table.character(virtual_key)),
            Some(Slot::Pending { .. }) => ShiftedCharacter::Pending,
            Some(Slot::Unavailable) => ShiftedCharacter::Unavailable,
            None => self.request(layout),
        }
    }

    fn request(&mut self, layout: KeyboardLayout) -> ShiftedCharacter {
        let refusal = match &self.road {
            Road::Closed(refusal) => *refusal,
            Road::Open { .. } if self.requests_in_flight == REQUEST_CAPACITY => {
                Refusal::RequestsFull
            }
            Road::Open { requests, .. } => match requests.try_send(layout.clone()) {
                Ok(()) => {
                    self.requests_in_flight += 1;
                    self.slots.insert(
                        layout.handle(),
                        Slot::Pending {
                            layout,
                            requested: true,
                        },
                    );
                    return ShiftedCharacter::Pending;
                }
                // The count above keeps the queue below its size, so this is
                // the same refusal by another name.
                Err(TrySendError::Full(_)) => Refusal::RequestsFull,
                Err(TrySendError::Disconnected(_)) => {
                    self.close(Refusal::WorkerGone);
                    Refusal::WorkerGone
                }
            },
        };
        self.refuse(&layout, refusal);
        ShiftedCharacter::Unavailable
    }

    /// Adopt one answer for a pending HKL. The door's `None` is `Unavailable`
    /// with no note: the layout has no table to read, and nothing failed.
    fn adopt(&mut self, answer: Answer) {
        let Some(Slot::Pending { requested, .. }) = self.slots.get(&answer.handle) else {
            // A second startup job for the same HKL answers after the first.
            return;
        };
        if *requested {
            self.requests_in_flight -= 1;
        }
        let slot = answer
            .table
            .map_or(Slot::Unavailable, |table| Slot::Known(Box::new(table)));
        self.slots.insert(answer.handle, slot);
    }

    /// The worker is gone: nothing more is offered, and every pending HKL is
    /// `Unavailable` now, each with its note.
    fn close(&mut self, refusal: Refusal) {
        self.road = Road::Closed(refusal);
        self.requests_in_flight = 0;
        let stranded: Vec<KeyboardLayout> = self
            .slots
            .values()
            .filter_map(|slot| match slot {
                Slot::Pending { layout, .. } => Some(layout.clone()),
                Slot::Known(_) | Slot::Unavailable => None,
            })
            .collect();
        for layout in stranded {
            self.refuse(&layout, refusal);
        }
    }

    /// Make the HKL `Unavailable` for good and say so once, through the
    /// diagnostics log. Every caller reaches it only for an HKL that is not
    /// yet terminal, so the note is once per HKL.
    fn refuse(&mut self, layout: &KeyboardLayout, refusal: Refusal) {
        self.slots.insert(layout.handle(), Slot::Unavailable);
        (self.note)(&format!(
            "keyboard layout {} (HKL {:#x}): Shift table unavailable, {}; Ctrl+Shift+Alt chords \
             under modifyOtherKeys use the un-shifted character",
            layout.name(),
            layout.handle(),
            refusal.reason(),
        ));
    }
}

/// The worker end and the deterministic seam its tests drive one job at a
/// time. Startup jobs precede later requests; a duplicate HKL reuses the first
/// copied result and never reopens the registry or DLL.
struct WorkerRoad {
    startup: VecDeque<KeyboardLayout>,
    incoming: Receiver<KeyboardLayout>,
    outgoing: SyncSender<Answer>,
    wake: Box<dyn Fn() + Send>,
    built: HashMap<usize, Option<KeyboardLayoutShiftTable>>,
}

impl WorkerRoad {
    fn run(mut self, worker_ctx: &bt_platform::admission::WorkerCtx) {
        while self.step_startup(|layout| build_on_system(worker_ctx, layout)) {}
        while let Ok(layout) = self.incoming.recv() {
            self.build(layout, |layout| build_on_system(worker_ctx, layout));
        }
    }

    fn step_startup(
        &mut self,
        build: impl FnOnce(&KeyboardLayout) -> Option<KeyboardLayoutShiftTable>,
    ) -> bool {
        let Some(layout) = self.startup.pop_front() else {
            return false;
        };
        self.build(layout, build);
        true
    }

    #[cfg(test)]
    fn step_request(
        &mut self,
        build: impl FnOnce(&KeyboardLayout) -> Option<KeyboardLayoutShiftTable>,
    ) -> bool {
        let layout = match self.incoming.try_recv() {
            Ok(layout) => layout,
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => return false,
        };
        self.build(layout, build);
        true
    }

    fn build(
        &mut self,
        layout: KeyboardLayout,
        build: impl FnOnce(&KeyboardLayout) -> Option<KeyboardLayoutShiftTable>,
    ) {
        let handle = layout.handle();
        let table = self
            .built
            .entry(handle)
            .or_insert_with(|| build(&layout))
            .clone();
        if self.outgoing.send(Answer { handle, table }).is_ok() {
            (self.wake)();
        }
    }
}

/// The worker-only door body. The capability is made only by the thread door;
/// the registry read and loader call are inside the platform entrance this
/// statement names (`window_waits.tsv`'s worker-door effect row).
fn build_on_system(
    worker: &bt_platform::admission::WorkerCtx,
    layout: &KeyboardLayout,
) -> Option<KeyboardLayoutShiftTable> {
    bt_platform::keyboard_layout_shift_table(worker, layout)
}

/// The channel pair and both ends. **The answer channel's capacity is the
/// startup job count plus [`REQUEST_CAPACITY`], and the worker's `send` never
/// waits for room:** the worker sends one answer per job, the window adopts an
/// answer only for a pending HKL, and the HKLs pending at once are at most the
/// startup jobs plus the admitted and unanswered requests, which the window
/// caps at [`REQUEST_CAPACITY`]. The answers not yet drained therefore never
/// exceed the capacity.
fn road(
    startup: Vec<KeyboardLayout>,
    wake: Box<dyn Fn() + Send>,
    note: Box<dyn Fn(&str)>,
) -> (LayoutTables, WorkerRoad) {
    let slots = startup
        .iter()
        .map(|layout| {
            (
                layout.handle(),
                Slot::Pending {
                    layout: layout.clone(),
                    requested: false,
                },
            )
        })
        .collect();
    let (requests, incoming) = mpsc::sync_channel(REQUEST_CAPACITY);
    let (outgoing, answers) = mpsc::sync_channel(startup.len() + REQUEST_CAPACITY);
    (
        LayoutTables {
            road: Road::Open { requests, answers },
            slots,
            requests_in_flight: 0,
            note,
        },
        WorkerRoad {
            startup: startup.into(),
            incoming,
            outgoing,
            wake,
            built: HashMap::new(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{self, KeyOrigin};
    use bt_platform::HostPlatform;
    use bt_pty::ConPtyKind;
    use bt_term::{KeyboardProtocol, ModifyOtherKeys};
    use std::cell::RefCell;
    use std::rc::Rc;
    use winit::keyboard::{Key, KeyCode, KeyLocation, ModifiersState, NativeKey, PhysicalKey};

    const VK_E: u16 = 0x45;
    const UNSHIFTED: &[u8] = b"\x1b[27;8;101~";
    const SHIFTED: &[u8] = b"\x1b[27;8;69~";

    fn layout(handle: usize) -> KeyboardLayout {
        KeyboardLayout::new(handle, format!("{handle:08X}"))
    }

    fn table(virtual_key: u16, character: char) -> KeyboardLayoutShiftTable {
        let mut characters = [None; 256];
        characters[usize::from(virtual_key)] = Some(character);
        KeyboardLayoutShiftTable::new(characters)
    }

    /// The product's road with its notes recorded instead of logged.
    fn noted_road(
        startup: Vec<KeyboardLayout>,
    ) -> (LayoutTables, WorkerRoad, Rc<RefCell<Vec<String>>>) {
        let notes = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&notes);
        let (tables, worker) = road(
            startup,
            Box::new(|| {}),
            Box::new(move |line: &str| sink.borrow_mut().push(line.to_owned())),
        );
        (tables, worker, notes)
    }

    /// RED (T-KEYBOARD-CTRLALT, round 7) — **an empty startup list starts no
    /// worker, and every lookup is `Unavailable` without a request.**
    ///
    /// The empty list is the platform's answer off Windows (and on Windows
    /// when no layout is loaded), so there is no thread and no channel there.
    /// The thread start is `LayoutTables::start`'s seam; a one-layout list is
    /// the control that the seam is the one the product calls.
    ///
    /// MUTATION: remove the `startup.is_empty()` return in
    /// `LayoutTables::start`: the empty list starts a worker.
    #[test]
    fn an_empty_startup_list_starts_no_worker_and_every_lookup_is_unavailable() {
        let notes = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&notes);
        let started = RefCell::new(0);
        let mut tables = LayoutTables::start(
            Vec::new(),
            Box::new(|| {}),
            Box::new(move |line: &str| sink.borrow_mut().push(line.to_owned())),
            |_| {
                *started.borrow_mut() += 1;
                Ok(())
            },
        )
        .expect("an empty list cannot fail to start");
        assert_eq!(*started.borrow(), 0, "an empty list started a worker");

        let active = layout(0x0409_0409);
        assert_eq!(
            tables.shifted_character(active.clone(), VK_E),
            ShiftedCharacter::Unavailable
        );
        assert_eq!(chord(&mut tables, &active).as_deref(), Some(UNSHIFTED));
        assert_eq!(notes.borrow().len(), 1, "{:?}", notes.borrow());

        let _control = LayoutTables::start(
            vec![active],
            Box::new(|| {}),
            Box::new(|_: &str| {}),
            |_| {
                *started.borrow_mut() += 1;
                Ok(())
            },
        )
        .expect("the seam starts");
        assert_eq!(*started.borrow(), 1);
    }

    /// RED (T-KEYBOARD-CTRLALT, round 4) — **the worker builds the current
    /// layout during its startup pass, before any miss request is needed.**
    ///
    /// This drives the same worker seam as the product one job at a time. The
    /// current layout stands for the active HKL included in
    /// `GetKeyboardLayoutList`; its answer is published and adopted through the
    /// channel pair the product owns.
    ///
    /// MUTATION: make `step_startup` pop the job without calling `self.build`:
    /// the lookup stays pending and this test reads no `E`.
    #[test]
    fn the_worker_builds_the_current_layout_at_startup() {
        let current = layout(0x0409_0409);
        let (mut tables, mut worker, notes) = noted_road(vec![current.clone()]);
        let mut built = Vec::new();

        assert!(worker.step_startup(|layout| {
            built.push(layout.handle());
            Some(table(VK_E, 'E'))
        }));
        assert_eq!(built, [current.handle()]);
        assert_eq!(tables.apply_answers(), 1);
        assert_eq!(
            tables.shifted_character(current, VK_E),
            ShiftedCharacter::Known(Some('E'))
        );
        assert!(notes.borrow().is_empty());
    }

    fn no_virtual_key(_: u16) -> Option<u16> {
        None
    }

    fn no_dead_keys(_: u16) -> bool {
        false
    }

    fn ctrl_shift_alt_e(shifted_character: ShiftedCharacter) -> Option<Vec<u8>> {
        input::keyboard_bytes(
            &Key::Unidentified(NativeKey::Windows(VK_E)),
            &Key::Character("e".into()),
            KeyLocation::Standard,
            ModifiersState::CONTROL | ModifiersState::SHIFT | ModifiersState::ALT,
            false,
            KeyboardProtocol {
                kitty: 0,
                modify_other_keys: ModifyOtherKeys::Two,
                win32_input_mode: true,
            },
            KeyOrigin {
                platform: HostPlatform::Windows,
                physical_key: PhysicalKey::Code(KeyCode::KeyE),
                text_with_all_modifiers: None,
                virtual_key_of_scan_code: no_virtual_key,
                virtual_key_is_dead: no_dead_keys,
                shifted_character,
                conpty: ConPtyKind::Shipped,
            },
        )
    }

    /// The chord as the product makes it: the lookup, then the encoder.
    fn chord(tables: &mut LayoutTables, layout: &KeyboardLayout) -> Option<Vec<u8>> {
        ctrl_shift_alt_e(tables.shifted_character(layout.clone(), VK_E))
    }

    /// RED (T-KEYBOARD-CTRLALT, round 4) — **a chord whose HKL has no table
    /// yet is un-shifted while the request is pending; the worker's answer is
    /// used afterwards.**
    ///
    /// No sleep and no product thread: the test makes the chord, advances the
    /// worker by one queued request, adopts the published answer, and makes the
    /// chord again. While the table is not known the chord's `k` is the
    /// un-shifted `e` (101), Folio's own rule; the delivered US Shift table
    /// changes the next chord to `E` (69). Every chord made before the answer
    /// is adopted is un-shifted, not only the first.
    ///
    /// MUTATION: remove the miss request in `LayoutTables::request` (admit
    /// the HKL as `Pending` without `try_send`): the worker has no request and
    /// the chord remains `CSI 27;8;101~` forever.
    #[test]
    fn a_chord_before_its_layouts_table_lands_is_unshifted_then_uses_the_worker_table() {
        let new_layout = layout(0x0409_0409);
        let (mut tables, mut worker, notes) = noted_road(Vec::new());

        assert_eq!(chord(&mut tables, &new_layout).as_deref(), Some(UNSHIFTED));
        assert_eq!(chord(&mut tables, &new_layout).as_deref(), Some(UNSHIFTED));

        assert!(worker.step_request(|_| Some(table(VK_E, 'E'))));
        assert!(!worker.step_request(|_| unreachable!("the pending HKL is not asked twice")));
        assert_eq!(tables.apply_answers(), 1);
        assert_eq!(chord(&mut tables, &new_layout).as_deref(), Some(SHIFTED));
        assert!(notes.borrow().is_empty());
    }

    /// RED (T-KEYBOARD-CTRLALT, round 7) — **a layout refused because the
    /// request capacity is spent is `Unavailable` for good: it is never
    /// offered again, its chords stay un-shifted, and it is noted once.**
    ///
    /// Eight layouts are admitted and unanswered; the ninth is refused. After
    /// the worker answers all eight and the window adopts them, the ninth is
    /// asked about twice more and the worker receives nothing for it.
    ///
    /// MUTATION: in `LayoutTables::request`, make the spent-capacity arm
    /// `return ShiftedCharacter::Unavailable` without `refuse` (the round-4
    /// road, which left the HKL unmarked): the ninth layout is offered again
    /// once there is room, and no note is written.
    #[test]
    fn a_layout_refused_because_the_queue_is_full_is_unshifted_for_good_and_noted_once() {
        let (mut tables, mut worker, notes) = noted_road(Vec::new());
        for handle in 1..=REQUEST_CAPACITY {
            assert_eq!(
                tables.shifted_character(layout(handle), VK_E),
                ShiftedCharacter::Pending
            );
        }
        let ninth = layout(0x0409_0409);
        assert_eq!(chord(&mut tables, &ninth).as_deref(), Some(UNSHIFTED));
        assert_eq!(
            tables.shifted_character(ninth.clone(), VK_E),
            ShiftedCharacter::Unavailable
        );

        let mut answered = 0;
        while worker.step_request(|_| Some(table(VK_E, 'E'))) {
            answered += 1;
        }
        assert_eq!(answered, REQUEST_CAPACITY);
        assert_eq!(tables.apply_answers(), REQUEST_CAPACITY);

        assert_eq!(chord(&mut tables, &ninth).as_deref(), Some(UNSHIFTED));
        assert_eq!(chord(&mut tables, &ninth).as_deref(), Some(UNSHIFTED));
        assert!(!worker.step_request(|_| unreachable!("an Unavailable HKL is not asked again")));
        let notes = notes.borrow();
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("04090409"), "{notes:?}");
        assert!(notes[0].contains("the request queue is full"), "{notes:?}");
    }

    /// RED (T-KEYBOARD-CTRLALT, round 7) — **a worker gone before it answers
    /// a startup layout makes that layout `Unavailable` at the next lookup,
    /// with one note, and no later lookup offers anything.**
    ///
    /// Dropping the worker end drops both of its channel ends, which is what a
    /// worker thread that ended leaves behind. The answer channel then reports
    /// `Disconnected`.
    ///
    /// MUTATION: in `LayoutTables::apply_answers`, treat
    /// `TryRecvError::Disconnected` like `Empty`: the startup layout stays
    /// `Pending` forever and no note is written.
    #[test]
    fn a_worker_gone_before_a_startup_answer_leaves_that_layout_unavailable_and_noted() {
        let startup = layout(0x040C_040C);
        let (mut tables, worker, notes) = noted_road(vec![startup.clone()]);
        drop(worker);

        assert_eq!(chord(&mut tables, &startup).as_deref(), Some(UNSHIFTED));
        assert_eq!(
            tables.shifted_character(startup.clone(), VK_E),
            ShiftedCharacter::Unavailable
        );
        assert_eq!(chord(&mut tables, &startup).as_deref(), Some(UNSHIFTED));
        let notes = notes.borrow();
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("040C040C"), "{notes:?}");
        assert!(notes[0].contains("worker has stopped"), "{notes:?}");
    }

    /// RED (T-KEYBOARD-CTRLALT, round 7) — **a worker gone after admitting a
    /// miss request and before answering it turns that pending layout
    /// `Unavailable` at once, with one note; a layout first seen afterwards is
    /// `Unavailable` without a request.**
    ///
    /// MUTATION: in `LayoutTables::close`, leave the pending slots as they are
    /// (only close the road): the admitted layout stays `Pending` forever and
    /// its note is never written.
    #[test]
    fn a_worker_gone_before_a_miss_answer_leaves_the_pending_layout_unavailable_and_noted() {
        let missed = layout(0x0409_0409);
        let (mut tables, worker, notes) = noted_road(Vec::new());
        assert_eq!(
            tables.shifted_character(missed.clone(), VK_E),
            ShiftedCharacter::Pending
        );
        drop(worker);

        assert_eq!(
            tables.shifted_character(missed.clone(), VK_E),
            ShiftedCharacter::Unavailable
        );
        assert_eq!(chord(&mut tables, &missed).as_deref(), Some(UNSHIFTED));
        let later = layout(0x0411_0411);
        assert_eq!(
            tables.shifted_character(later.clone(), VK_E),
            ShiftedCharacter::Unavailable
        );
        assert_eq!(
            tables.shifted_character(later, VK_E),
            ShiftedCharacter::Unavailable
        );
        let notes = notes.borrow();
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes[0].contains("04090409"), "{notes:?}");
        assert!(notes[1].contains("04110411"), "{notes:?}");
        assert!(
            notes.iter().all(|note| note.contains("worker has stopped")),
            "{notes:?}"
        );
    }

    /// RED (T-KEYBOARD-CTRLALT, round 7) — **a layout the door cannot read a
    /// table from is `Unavailable`: its chords are un-shifted, it is never
    /// asked again, and nothing is noted.**
    ///
    /// Before round 7 the door's `None` was kept as a table with no cells, and
    /// every such chord sent nothing at all for the rest of the process.
    ///
    /// MUTATION: in `LayoutTables::adopt`, keep the door's `None` as a table
    /// with no cells (`KeyboardLayoutShiftTable::new([None; 256])`): the chord
    /// after the answer sends nothing.
    #[test]
    fn a_layout_the_door_cannot_read_is_unshifted_for_good_without_a_note() {
        let unreadable = layout(0x0409_0409);
        let (mut tables, mut worker, notes) = noted_road(Vec::new());
        assert_eq!(chord(&mut tables, &unreadable).as_deref(), Some(UNSHIFTED));

        assert!(worker.step_request(|_| None));
        assert_eq!(tables.apply_answers(), 1);
        assert_eq!(
            tables.shifted_character(unreadable.clone(), VK_E),
            ShiftedCharacter::Unavailable
        );
        assert_eq!(chord(&mut tables, &unreadable).as_deref(), Some(UNSHIFTED));
        assert!(!worker.step_request(|_| unreachable!("an Unavailable HKL is not asked again")));
        assert!(notes.borrow().is_empty(), "{:?}", notes.borrow());
    }
}
