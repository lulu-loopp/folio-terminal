//! The process-wide keyboard-layout Shift tables and their worker road.
//!
//! A key event may ask only the in-memory map. Registry reads and layout-DLL
//! loads belong to `folio-layout-tables`, which starts with every HKL returned
//! by `GetKeyboardLayoutList` and accepts a nonblocking request when a later
//! `GetKeyboardLayout(0)` names an HKL the map has not seen. Answers are
//! published before `AppEvent::LayoutTablesReady`; the next lookup drains too,
//! so a lost wake cannot strand a table.

use crate::AppEvent;
use bt_platform::{KeyboardLayout, KeyboardLayoutShiftTable};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use winit::event_loop::EventLoopProxy;

const REQUEST_CAPACITY: usize = 8;

#[derive(Clone)]
struct Answer {
    handle: usize,
    table: Option<KeyboardLayoutShiftTable>,
}

/// The window end: copied tables, requests already in flight, and the two
/// channel ends it alone uses. One instance lives on `App`, so all windows ask
/// one cache and no window can drain another window's answer.
pub(crate) struct LayoutTables {
    requests: SyncSender<KeyboardLayout>,
    answers: Receiver<Answer>,
    tables: HashMap<usize, Option<KeyboardLayoutShiftTable>>,
    pending: HashSet<usize>,
}

impl LayoutTables {
    pub(crate) fn spawn(
        proxy: EventLoopProxy<AppEvent>,
        startup: Vec<KeyboardLayout>,
    ) -> std::io::Result<Self> {
        let (tables, worker) = road(
            startup,
            Box::new(move || {
                let _ = proxy.send_event(AppEvent::LayoutTablesReady);
            }),
        );
        let _worker = bt_platform::spawn_at_priority(
            "folio-layout-tables",
            bt_platform::ThreadPriority::BelowNormal,
            move |worker_ctx| worker.run(worker_ctx),
        )?;
        Ok(tables)
    }

    /// Adopt every answer already published. Called by the worker's event and
    /// before each lookup, which is the lane contract's lost-wake recovery.
    pub(crate) fn apply_answers(&mut self) -> usize {
        let mut applied = 0;
        while let Ok(answer) = self.answers.try_recv() {
            self.pending.remove(&answer.handle);
            self.tables.insert(answer.handle, answer.table);
            applied += 1;
        }
        applied
    }

    /// The table's answer for this HKL and virtual key. Outer `None` means the
    /// table is not here yet and a request is now pending if the bounded queue
    /// admitted it; inner `None` is a table cell that deliberately types no one
    /// character with Shift.
    pub(crate) fn shifted_character(
        &mut self,
        layout: KeyboardLayout,
        virtual_key: u16,
    ) -> Option<Option<char>> {
        self.apply_answers();
        let handle = layout.handle();
        if let Some(table) = self.tables.get(&handle) {
            return Some(
                table
                    .as_ref()
                    .and_then(|table| table.character(virtual_key)),
            );
        }
        if !self.pending.contains(&handle) {
            match self.requests.try_send(layout) {
                Ok(()) => {
                    self.pending.insert(handle);
                }
                Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {}
            }
        }
        None
    }
}

/// The worker end and the deterministic seam its tests drive one job at a
/// time. Startup jobs precede later requests; a duplicate HKL reuses the first
/// copied result and never reopens the registry or DLL.
struct WorkerRoad {
    startup: VecDeque<KeyboardLayout>,
    incoming: Receiver<KeyboardLayout>,
    outgoing: mpsc::Sender<Answer>,
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

fn road(startup: Vec<KeyboardLayout>, wake: Box<dyn Fn() + Send>) -> (LayoutTables, WorkerRoad) {
    let pending = startup.iter().map(KeyboardLayout::handle).collect();
    let (requests, incoming) = mpsc::sync_channel(REQUEST_CAPACITY);
    let (outgoing, answers) = mpsc::channel();
    (
        LayoutTables {
            requests,
            answers,
            tables: HashMap::new(),
            pending,
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
    use crate::input::{self, KeyOrigin, ShiftedCharacter};
    use bt_platform::HostPlatform;
    use bt_pty::ConPtyKind;
    use bt_term::{KeyboardProtocol, ModifyOtherKeys};
    use winit::keyboard::{Key, KeyCode, KeyLocation, ModifiersState, NativeKey, PhysicalKey};

    const VK_E: u16 = 0x45;

    fn layout(handle: usize) -> KeyboardLayout {
        KeyboardLayout::new(handle, format!("{handle:08X}"))
    }

    fn table(virtual_key: u16, character: char) -> KeyboardLayoutShiftTable {
        let mut characters = [None; 256];
        characters[usize::from(virtual_key)] = Some(character);
        KeyboardLayoutShiftTable::new(characters)
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
        let (mut tables, mut worker) = road(vec![current.clone()], Box::new(|| {}));
        let mut built = Vec::new();

        assert!(worker.step_startup(|layout| {
            built.push(layout.handle());
            Some(table(VK_E, 'E'))
        }));
        assert_eq!(built, [current.handle()]);
        assert_eq!(tables.apply_answers(), 1);
        assert_eq!(tables.shifted_character(current, VK_E), Some(Some('E')));
    }

    fn no_virtual_key(_: u16) -> Option<u16> {
        None
    }

    fn no_dead_keys(_: u16) -> bool {
        false
    }

    fn ctrl_shift_alt_e(shifted_character: ShiftedCharacter) -> Vec<u8> {
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
        .expect("modifyOtherKeys answers the chord")
    }

    /// RED (T-KEYBOARD-CTRLALT, round 4) — **a chord whose HKL has no table
    /// uses the unshifted character once; the worker answer is used afterwards.**
    ///
    /// No sleep and no product thread: the test makes the chord, advances the
    /// worker by one queued request, adopts the published answer, and makes the
    /// chord again. xterm's default `formatOtherKeys=0` fallback is `e` (101);
    /// the delivered US Shift table changes the next chord to `E` (69).
    ///
    /// MUTATION: remove the miss request in `LayoutTables::shifted_character`:
    /// the second chord remains `CSI 27;8;101~` forever.
    #[test]
    fn a_missing_layout_falls_back_for_one_chord_then_uses_the_worker_table() {
        let new_layout = layout(0x0409_0409);
        let (mut tables, mut worker) = road(Vec::new(), Box::new(|| {}));

        let first = tables
            .shifted_character(new_layout.clone(), VK_E)
            .map_or(ShiftedCharacter::Pending, ShiftedCharacter::Known);
        assert_eq!(ctrl_shift_alt_e(first), b"\x1b[27;8;101~");

        assert!(worker.step_request(|_| Some(table(VK_E, 'E'))));
        assert_eq!(tables.apply_answers(), 1);
        let second = tables
            .shifted_character(new_layout, VK_E)
            .map_or(ShiftedCharacter::Pending, ShiftedCharacter::Known);
        assert_eq!(ctrl_shift_alt_e(second), b"\x1b[27;8;69~");
    }
}
