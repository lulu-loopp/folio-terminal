//! The providers' answers through their own door, without a client (T-UIA-SKELETON, 88a): the
//! real subclass procedure called in-process with the question UI Automation sends, and the
//! providers it hands back asked through their COM interfaces. No window: the null handle makes
//! every system call inside refuse, which the providers answer around (the screen origin is then
//! (0, 0), so a rectangle on the screen is the published one).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    IRawElementProviderFragment, IRawElementProviderFragmentRoot, IRawElementProviderSimple,
    ITextProvider, ITextProvider2, IValueProvider, NavigateDirection_FirstChild,
    NavigateDirection_LastChild, NavigateDirection_NextSibling, NavigateDirection_Parent,
    TextUnit_Word, UIA_ControlTypePropertyId, UIA_E_ELEMENTNOTAVAILABLE,
    UIA_HasKeyboardFocusPropertyId, UIA_NamePropertyId, UIA_TextPatternId, UIA_ValuePatternId,
    UiaRootObjectId,
};
use windows::Win32::UI::WindowsAndMessaging::{self, WM_DESTROY};
use windows::core::{BOOL, BSTR, Interface};

use super::super::{
    CellIn, CellPoint, ClientRect, PaneId, PaneText, RowPlace, RowText, Shape, UIA_ROOT_OBJECT_ID,
    WM_GETOBJECT, WindowText,
};
use super::{HookData, UIA_SUBCLASS_ID, hresult, uia_subclass};

/// PIN — **the two numbers are the ones Windows gives them.**
#[test]
fn the_root_question_is_the_numbers_windows_gives_it() {
    assert_eq!(WM_GETOBJECT, WindowsAndMessaging::WM_GETOBJECT);
    assert_eq!(UIA_ROOT_OBJECT_ID, UiaRootObjectId);
}

fn row(text: &str, index: u32) -> RowText {
    let cells: Vec<String> = text.chars().map(String::from).collect();
    RowText::from_cells(
        cells.iter().enumerate().map(|(column, text)| CellIn {
            text,
            spacer: false,
            word_start: text != " " && (column == 0 || cells[column - 1] == " "),
        }),
        RowPlace {
            left: 0.0,
            cell_width: 10.0,
            top: 20.0 * f64::from(index),
            height: 20.0,
        },
        false,
        0,
    )
}

fn pane(seat: u64, name: &str, left: f64, holds: bool, lines: &[&str]) -> PaneText {
    let rows = lines
        .iter()
        .zip(0u32..)
        .map(|(line, index)| row(line, index))
        .collect();
    PaneText::new(
        PaneId { tab: 1, seat },
        name.to_owned(),
        ClientRect {
            left,
            top: 0.0,
            width: 100.0,
            height: 100.0,
        },
        holds,
        rows,
        Some(CellPoint { row: 0, column: 4 }),
        None,
    )
}

/// A hook's data with a wake that counts, for `shape`.
fn hook(shape: Shape) -> (HookData, Arc<AtomicUsize>) {
    let woken = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&woken);
    let data = HookData::new(
        0,
        shape,
        Box::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
        }),
    );
    (data, woken)
}

/// The window procedure, called as the system would.
fn send(data: &HookData, message: u32, lparam: isize) -> isize {
    // SAFETY: the data outlives the call; the null window makes every system call inside refuse.
    unsafe {
        uia_subclass(
            HWND::default(),
            message,
            WPARAM(0),
            LPARAM(lparam),
            UIA_SUBCLASS_ID,
            (&raw const *data) as usize,
        )
    }
    .0
}

fn bstr(value: &VARIANT) -> String {
    BSTR::try_from(value)
        .map(|text| text.to_string())
        .unwrap_or_default()
}

/// RED (88a) — **nothing is built until a client asks**: the root is made on the first
/// `UiaRootObjectId` question and not before, the loop is woken once for it, and another object
/// id (MSAA's `OBJID_CLIENT`) makes nothing.
///
/// MUTATION: make the root in `HookData::new`, or drop the `wanted.swap` guard on the wake.
#[test]
fn the_root_is_made_on_the_first_question_only() {
    let (data, woken) = hook(Shape::Text);
    let _ = send(&data, WM_GETOBJECT, -4);
    let _ = send(&data, 0x000F, 0);
    assert!(!data.made(), "no question for the root yet");
    assert!(!data.shared().wanted.load(Ordering::SeqCst));
    assert_eq!(woken.load(Ordering::SeqCst), 0);
    let _ = send(&data, WM_GETOBJECT, -25);
    let _ = send(&data, WM_GETOBJECT, -25);
    assert!(data.made());
    assert!(data.shared().wanted.load(Ordering::SeqCst));
    assert_eq!(
        woken.load(Ordering::SeqCst),
        1,
        "one wake, at the first question"
    );
}

/// RED (88a) — **the tree and the text, answered from the publication through the providers'
/// own interfaces**: the root's children are the published panes in order, each names itself,
/// says whether it holds the keyboard, and its Text pattern's document is its rows.
///
/// MUTATION: answer `Navigate(FirstChild)` with nothing, or read `GetText` from another pane.
#[test]
fn the_providers_answer_the_published_tree_and_text() {
    let (data, _) = hook(Shape::Text);
    let events = data.publish(WindowText {
        panes: vec![
            pane(4, "pwsh", 0.0, true, &["PS> ls", "a.txt"]),
            pane(9, "cmd", 100.0, false, &["C:>"]),
        ],
    });
    assert!(events.is_none(), "no root yet, so no event: nobody asked");
    let root: IRawElementProviderFragmentRoot = data.root().to_interface();
    let fragment: IRawElementProviderFragment = root.cast().unwrap();
    // SAFETY: in-process calls on this window's own providers.
    unsafe {
        let first = fragment.Navigate(NavigateDirection_FirstChild).unwrap();
        let last = fragment.Navigate(NavigateDirection_LastChild).unwrap();
        let simple: IRawElementProviderSimple = first.cast().unwrap();
        assert_eq!(
            bstr(&simple.GetPropertyValue(UIA_NamePropertyId).unwrap()),
            "pwsh"
        );
        assert_eq!(
            i32::try_from(&simple.GetPropertyValue(UIA_ControlTypePropertyId).unwrap()).unwrap(),
            Shape::Text.control_type_id()
        );
        assert!(
            bool::try_from(
                &simple
                    .GetPropertyValue(UIA_HasKeyboardFocusPropertyId)
                    .unwrap()
            )
            .unwrap()
        );
        let next = first.Navigate(NavigateDirection_NextSibling).unwrap();
        let next: IRawElementProviderSimple = next.cast().unwrap();
        let last: IRawElementProviderSimple = last.cast().unwrap();
        assert_eq!(
            bstr(&next.GetPropertyValue(UIA_NamePropertyId).unwrap()),
            "cmd"
        );
        assert_eq!(
            bstr(&last.GetPropertyValue(UIA_NamePropertyId).unwrap()),
            "cmd"
        );
        assert!(first.Navigate(NavigateDirection_Parent).is_ok());
        let pattern = simple.GetPatternProvider(UIA_TextPatternId).unwrap();
        let text: ITextProvider = pattern.cast().unwrap();
        let document = text.DocumentRange().unwrap();
        assert_eq!(document.GetText(-1).unwrap().to_string(), "PS> ls\r\na.txt");
        let focus = root.GetFocus().unwrap();
        let focus: IRawElementProviderSimple = focus.cast().unwrap();
        assert_eq!(
            bstr(&focus.GetPropertyValue(UIA_NamePropertyId).unwrap()),
            "pwsh"
        );
        let at = root.ElementProviderFromPoint(150.0, 10.0).unwrap();
        let at: IRawElementProviderSimple = at.cast().unwrap();
        assert_eq!(
            bstr(&at.GetPropertyValue(UIA_NamePropertyId).unwrap()),
            "cmd"
        );
        assert_eq!(first.BoundingRectangle().unwrap().width, 100.0);
    }
}

/// RED (88a) — **the caret range and word moves, through the Text pattern**.
#[test]
fn the_caret_range_is_the_published_caret() {
    let (data, _) = hook(Shape::Text);
    let _ = data.root();
    let _ = data.publish(WindowText {
        panes: vec![pane(4, "pwsh", 0.0, true, &["PS> ls -a"])],
    });
    let root: IRawElementProviderFragment = data.root().to_interface();
    // SAFETY: in-process calls on this window's own providers.
    unsafe {
        let pane: IRawElementProviderSimple = root
            .Navigate(NavigateDirection_FirstChild)
            .unwrap()
            .cast()
            .unwrap();
        let text: ITextProvider2 = pane
            .GetPatternProvider(UIA_TextPatternId)
            .unwrap()
            .cast()
            .unwrap();
        let mut active = BOOL::from(false);
        let caret = text.GetCaretRange(&raw mut active).unwrap();
        assert!(active.as_bool(), "the pane holds the keyboard");
        assert_eq!(
            caret.GetText(-1).unwrap().to_string(),
            "",
            "a caret is degenerate"
        );
        caret.ExpandToEnclosingUnit(TextUnit_Word).unwrap();
        assert_eq!(
            caret.GetText(-1).unwrap().to_string(),
            "ls ",
            "the word at column 4"
        );
        assert_eq!(caret.Move(TextUnit_Word, 1).unwrap(), 1);
        assert_eq!(caret.GetText(-1).unwrap().to_string(), "-a");
        assert_eq!(
            caret.Move(TextUnit_Word, 1).unwrap(),
            0,
            "no word after the last"
        );
        let rectangles = caret.GetBoundingRectangles().unwrap();
        assert!(!rectangles.is_null());
        let _ = windows::Win32::System::Ole::SafeArrayDestroy(rectangles);
    }
}

/// RED (88a) — **the Value pattern is the edit shape's only**.
#[test]
fn only_the_edit_shape_answers_the_value_pattern() {
    for (shape, answers) in [
        (Shape::Edit, true),
        (Shape::Document, false),
        (Shape::Text, false),
    ] {
        let (data, _) = hook(shape);
        let _ = data.publish(WindowText {
            panes: vec![pane(4, "pwsh", 0.0, true, &["ab"])],
        });
        let root: IRawElementProviderFragment = data.root().to_interface();
        // SAFETY: in-process calls on this window's own providers.
        unsafe {
            let pane: IRawElementProviderSimple = root
                .Navigate(NavigateDirection_FirstChild)
                .unwrap()
                .cast()
                .unwrap();
            let value = pane.GetPatternProvider(UIA_ValuePatternId);
            assert_eq!(value.is_ok(), answers, "{shape:?}");
            if let Ok(value) = value {
                let value: IValueProvider = value.cast().unwrap();
                assert_eq!(value.Value().unwrap().to_string(), "ab");
                assert!(!value.IsReadOnly().unwrap().as_bool());
            }
        }
    }
}

/// RED (88a) — **the snapshot door: a provider answers from the newest publication only**. A
/// range made before a shorter publication is clamped into it, a pane the newest publication
/// lacks is gone, and after `WM_DESTROY` every provider says the element is not available.
#[test]
fn providers_read_the_newest_publication_only() {
    let (data, _) = hook(Shape::Text);
    let _ = data.publish(WindowText {
        panes: vec![pane(4, "pwsh", 0.0, true, &["abcdef"])],
    });
    let root: IRawElementProviderFragment = data.root().to_interface();
    // SAFETY: in-process calls on this window's own providers.
    unsafe {
        let simple: IRawElementProviderSimple = root
            .Navigate(NavigateDirection_FirstChild)
            .unwrap()
            .cast()
            .unwrap();
        let text: ITextProvider = simple
            .GetPatternProvider(UIA_TextPatternId)
            .unwrap()
            .cast()
            .unwrap();
        let document = text.DocumentRange().unwrap();
        let _ = data.publish(WindowText {
            panes: vec![pane(4, "pwsh", 0.0, true, &["abc"])],
        });
        assert_eq!(document.GetText(-1).unwrap().to_string(), "abc", "clamped");
        let _ = data.publish(WindowText {
            panes: vec![pane(5, "other", 0.0, true, &["x"])],
        });
        let error = document.GetText(-1).unwrap_err();
        assert_eq!(
            error.code(),
            hresult(UIA_E_ELEMENTNOTAVAILABLE),
            "pane 4 is gone"
        );
        let _ = send(&data, WM_DESTROY, 0);
        let error = root.Navigate(NavigateDirection_FirstChild).unwrap_err();
        assert_eq!(
            error.code(),
            hresult(UIA_E_ELEMENTNOTAVAILABLE),
            "the window is gone"
        );
        assert_eq!(
            send(&data, WM_GETOBJECT, -25),
            0,
            "a released window answers nothing"
        );
    }
}

/// RED (88a) — **a focus event is owed only when the pane holding the keyboard moves**, and a
/// client's `SetFocus` is parked for the loop with one wake.
#[test]
fn focus_moves_owe_one_event_and_a_clients_focus_is_parked() {
    let (data, woken) = hook(Shape::Text);
    let _ = data.root();
    let two = |focused: u64| WindowText {
        panes: vec![
            pane(4, "a", 0.0, focused == 4, &["a"]),
            pane(9, "b", 100.0, focused == 9, &["b"]),
        ],
    };
    assert!(data.publish(two(4)).is_some(), "the first focus");
    assert!(data.publish(two(4)).is_none(), "unmoved");
    assert!(
        data.publish(two(0)).is_none(),
        "the window let the keyboard go"
    );
    assert!(data.publish(two(4)).is_some(), "and took it back");
    assert!(data.publish(two(9)).is_some(), "moved");
    let root: IRawElementProviderFragment = data.root().to_interface();
    // SAFETY: in-process calls on this window's own providers.
    unsafe {
        let first = root.Navigate(NavigateDirection_FirstChild).unwrap();
        first.SetFocus().unwrap();
    }
    assert_eq!(
        woken.load(Ordering::SeqCst),
        2,
        "the first question and the focus request"
    );
    assert_eq!(data.take_focus_request(), Some(PaneId { tab: 1, seat: 4 }));
    assert_eq!(data.take_focus_request(), None, "taken once");
}

/// The elements of a one-dimensional `SAFEARRAY` of `VT_I4`, and the array destroyed.
fn runtime_id(array: *mut windows::Win32::System::Com::SAFEARRAY) -> Vec<i32> {
    use windows::Win32::System::Ole::{SafeArrayDestroy, SafeArrayGetElement, SafeArrayGetUBound};
    // SAFETY: an array the provider made and handed to the caller, read inside its bounds and
    // destroyed once.
    unsafe {
        let upper = SafeArrayGetUBound(array, 1).unwrap();
        let elements = (0..=upper)
            .map(|index| {
                let mut value = 0i32;
                SafeArrayGetElement(array, &raw const index, (&raw mut value).cast()).unwrap();
                value
            })
            .collect();
        let _ = SafeArrayDestroy(array);
        elements
    }
}

/// The first pane of `data`'s published tree.
fn first_pane(data: &HookData) -> IRawElementProviderFragment {
    let root: IRawElementProviderFragment = data.root().to_interface();
    // SAFETY: an in-process call on this window's own provider.
    unsafe { root.Navigate(NavigateDirection_FirstChild).unwrap() }
}

/// RED (the review's F1) — **a pane is named by its tab and its seat**: two tabs each hold a seat
/// 1, and their panes are two elements — two runtime ids, a focus event owed when the active tab
/// changes, and a provider a client still holds for the first tab's pane answers "element not
/// available" once that tab is not the published one, never with the second tab's text.
///
/// MUTATION: build `GetRuntimeId` from the seat alone, match `Pane::published` (or the focus
/// event's `raised`) on the seat alone.
#[test]
fn a_pane_is_named_by_its_tab_and_seat() {
    let (data, _) = hook(Shape::Text);
    let _ = data.root();
    let in_tab = |tab: u64, line: &str| {
        let mut one = pane(1, line, 0.0, true, &[line]);
        one.id = PaneId { tab, seat: 1 };
        WindowText { panes: vec![one] }
    };
    assert!(
        data.publish(in_tab(1, "first")).is_some(),
        "the first focus"
    );
    let held = first_pane(&data);
    // SAFETY: in-process calls on this window's own providers.
    let first_id = runtime_id(unsafe { held.GetRuntimeId() }.unwrap());
    assert!(
        data.publish(in_tab(2, "second")).is_some(),
        "the keyboard moved to another tab's seat 1: a focus event is owed"
    );
    let now = first_pane(&data);
    // SAFETY: in-process calls on this window's own providers.
    unsafe {
        let second_id = runtime_id(now.GetRuntimeId().unwrap());
        assert_ne!(first_id, second_id, "two panes, two runtime ids");
        assert_eq!(
            second_id[1..],
            [2, 0, 1, 0],
            "the tab, then the seat, low halves first"
        );
        let held: IRawElementProviderSimple = held.cast().unwrap();
        let error = held.GetPropertyValue(UIA_NamePropertyId).unwrap_err();
        assert_eq!(
            error.code(),
            hresult(UIA_E_ELEMENTNOTAVAILABLE),
            "the first tab's pane is not the second's"
        );
        let now: IRawElementProviderSimple = now.cast().unwrap();
        assert_eq!(
            bstr(&now.GetPropertyValue(UIA_NamePropertyId).unwrap()),
            "second"
        );
    }
}

/// RED (the review's F3) — **every provider says COM's threading rules, and the first client
/// call's thread is recorded** beside the window's, for the E-1 run's one line.
///
/// MUTATION: answer `ProviderOptions_ServerSideProvider` alone, or drop the record in
/// `Shared::alive`.
#[test]
fn providers_use_com_threading_and_the_first_calls_thread_is_kept() {
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Accessibility::{
        ProviderOptions_ServerSideProvider, ProviderOptions_UseComThreading,
    };
    let (data, _) = hook(Shape::Text);
    let _ = data.publish(WindowText {
        panes: vec![pane(4, "pwsh", 0.0, true, &["ab"])],
    });
    assert_eq!(data.threads(), None, "no client has called");
    let root: IRawElementProviderSimple = data.root().to_interface();
    let wanted = ProviderOptions_ServerSideProvider.0 | ProviderOptions_UseComThreading.0;
    // SAFETY: in-process calls on this window's own providers.
    unsafe {
        assert_eq!(root.ProviderOptions().unwrap().0, wanted);
        let pane: IRawElementProviderSimple = first_pane(&data).cast().unwrap();
        assert_eq!(pane.ProviderOptions().unwrap().0, wanted);
    }
    let threads = data.threads().expect("a client has called");
    // SAFETY: reads this thread's id; no preconditions.
    let here = unsafe { GetCurrentThreadId() };
    assert_eq!((threads.window, threads.first_call), (here, here));
}
