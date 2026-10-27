//! Text-free, opt-in observations of the IME boundary. No routing decisions live here.
use super::*;
pub(crate) use bt_platform::ime_trace::{enabled, line};
use std::cell::Cell;

/// Turn the platform's IME trace on when the entry was asked for `BT_IME_TRACE`
/// (`entry_doors`); with no file named, nothing is installed and the trace is off.
pub fn install() {
    if crate::entry_doors::installed().traces.ime.is_none() {
        return;
    }
    bt_platform::ime_trace::install(|message| {
        // One file, one writer: the focus census (`ime_report`) owns the sink.
        ime_report::TRACE.line(|| format!("{:?} {message}", Instant::now()));
    });
}

#[derive(Default)]
pub struct State {
    pub(super) owner: Cell<Option<(ImeOwner, &'static str)>>,
    pub(super) draw: Cell<Option<&'static str>>,
}

/// A quiet answer emits nothing. None resets the next composition's first answer.
pub(super) fn changed<T: Copy + PartialEq>(last: &Cell<Option<T>>, next: Option<T>) -> bool {
    last.replace(next) != next && next.is_some()
}

pub fn input_line(event: &Ime) -> String {
    match event {
        Ime::Enabled => "IME_IN kind=Enabled bytes=0".to_owned(),
        Ime::Disabled => "IME_IN kind=Disabled bytes=0".to_owned(),
        Ime::Preedit(text, cursor) => {
            format!("IME_IN kind=Preedit bytes={} cursor={cursor:?}", text.len())
        }
        Ime::Commit(text) => format!("IME_IN kind=Commit bytes={}", text.len()),
    }
}

pub fn allowed_line(value: bool, reason: &'static str) -> String {
    format!("IME_OUT_ALLOWED value={value} reason={reason}")
}

pub fn area_line(area: ImeCursorArea, action: &'static str) -> String {
    format!(
        "IME_OUT_AREA x={} y={} width={} height={} action={action}",
        area.x, area.y, area.width, area.height
    )
}

pub(super) fn cancel_line(owner: ImeOwner, reason: &'static str, result: Option<bool>) -> String {
    format!("IME_OUT_CANCEL owner={owner:?} reason={reason} result={result:?}")
}

pub(super) fn owner_line(
    old: Option<(ImeOwner, &'static str)>,
    new: ImeOwner,
    cause: &'static str,
) -> String {
    format!(
        "IME_OWNER old={:?} new={new:?} previous_cause={} cause={cause}",
        old.map(|v| v.0),
        old.map_or("initial", |v| v.1)
    )
}

fn origin_kind(origin: &CompositionOrigin) -> ImeOwner {
    match origin {
        CompositionOrigin::Shell(_) => ImeOwner::Shell,
        CompositionOrigin::Preview(_) => ImeOwner::Preview,
        CompositionOrigin::GraphSearch(_) => ImeOwner::GraphSearch,
        CompositionOrigin::Search(_) => ImeOwner::Search,
        CompositionOrigin::FilesTree(_) => ImeOwner::FilesTree,
        CompositionOrigin::Rename(_) => ImeOwner::Rename,
        CompositionOrigin::GitPrompt => ImeOwner::GitPrompt,
        CompositionOrigin::Palette => ImeOwner::Palette,
        CompositionOrigin::Modal => ImeOwner::Modal,
    }
}

pub(super) fn ruling_line(
    event: &Ime,
    state: &Composing,
    here: &CompositionOrigin,
    ruling: CompositionRuling,
) -> String {
    let owner = origin_kind(here);
    let reason = if !ruling.deliver {
        "origin_mismatch"
    } else if matches!(owner, ImeOwner::Modal | ImeOwner::FilesTree) {
        "owner_swallows"
    } else if owner != ImeOwner::Shell {
        "field_route"
    } else {
        "shell_route"
    };
    format!(
        "IME_RULING {} origin={:?} destination={owner:?} same_origin={} deliver={} next={:?} reason={reason}",
        input_line(event),
        state.origin().map(origin_kind),
        state.origin().is_none_or(|origin| origin == here),
        ruling.deliver,
        ruling.next
    )
}

/// Reports the terminal projection only. Alternate-screen mode is context,
/// never a veto: the compositor can and does draw in a TUI's alternate screen.
/// `in_view` is the frame's `cursor_in_view`: a cursor the program hid is still where a
/// composition is drawn, so only a cursor whose cell the frame does not draw stops it.
pub(super) fn draw_reason(
    owner_matches: bool,
    in_view: bool,
    nonzero: bool,
    written: bool,
) -> &'static str {
    if !owner_matches {
        "owner_mismatch"
    } else if !in_view {
        "cursor_out_of_view"
    } else if !nonzero {
        "zero_size_rectangle"
    } else if !written {
        "no_visible_cells"
    } else {
        "drawn"
    }
}

#[derive(Clone, Copy)]
pub(super) struct DrawFacts {
    pub(super) bytes: usize,
    pub(super) visible: bool,
    pub(super) row: u32,
    pub(super) column: u32,
    pub(super) alt: bool,
    pub(super) area: ImeCursorArea,
}

pub(super) fn draw_line(reason: &'static str, facts: DrawFacts) -> String {
    let DrawFacts {
        bytes,
        visible,
        row,
        column,
        alt,
        area,
    } = facts;
    format!(
        "IME_DRAW surface=terminal drawn={} reason={reason} bytes={bytes} cursor_visible={visible} row={row} column={column} alt={alt} x={} y={} width={} height={}",
        reason == "drawn",
        area.x,
        area.y,
        area.width,
        area.height
    )
}

pub fn caret_line(
    action: &'static str,
    reason: &'static str,
    position: Option<(i32, i32)>,
) -> String {
    format!("IME_OUT_CARET action={action} reason={reason} position={position:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── what this module asks the crate ────────────────────
    //
    // **P3's deletion commit for this batch**
    // (`docs/plans/bt-app-split-prep.md` §6.3, and §6.0 rule 3). The commit
    // before this one took each reading twice — once from
    // the window's file or this file's own text, once from `bt-source`
    // — and asserted the two were the same answer; this one removes the older
    // of the two, because two implementations of one judgement do not vouch
    // for each other (`docs/CONVENTIONS.md` §十 rule 4).
    //
    // **The pattern is `main.rs::pty_drain_budget_tests`' and is not
    // re-derived**; that module's header carries the six points behind
    // `source_index`, `item_body` and `method_body`.
    //
    // One reading changes shape. The caret's destruction was counted as zero in
    // `main.rs` and then found by hand in this file's own product half — two
    // readings of one claim, "the door is here and nowhere else". Asked of the
    // package it is one number: **one** product site, and the body pin below
    // says which method it stands in. A package-wide zero would have inverted
    // the guard, because the door this module owns is itself a product site.
    fn source_index() -> &'static bt_source::Index {
        bt_source::Index::of_package("bt-app")
    }

    /// The body of `owner::name`, braces included — the identity of §2.4
    /// rather than a line of a file.
    fn item_body(query: &bt_source::ItemQuery) -> &'static str {
        source_index()
            .body_of(query)
            .unwrap_or_else(|failure| panic!("{failure}"))
    }

    /// The body of one inherent method of `owner`.
    fn method_body(owner: &str, name: &str) -> &'static str {
        item_body(&bt_source::ItemQuery::method(owner, name))
    }

    /// One search over the whole package, refusing loudly rather than
    /// answering a smaller question.
    fn found(needle: bt_source::Needle, view: bt_source::View) -> bt_source::Found {
        source_index()
            .search(&bt_source::Search::new(needle, view))
            .unwrap_or_else(|failure| panic!("{failure}"))
    }

    /// **How many of these occurrences a product build compiles** —
    /// `bt_source::Found::in_the_product`, which owns that rule and both of the
    /// grains it takes: §2.3's file and §2.4's item.
    ///
    /// This module needs both, which is why it is asked here rather than left
    /// to the file grain: the spellings counted below are written again in this
    /// module's own assertions, and again in whole test files elsewhere in the
    /// package. The rule used to be written out here, in one of twelve copies
    /// of it this crate carried.
    fn in_the_product(found: &bt_source::Found) -> usize {
        found.in_the_product(source_index()).len()
    }

    /// The same count of one raw needle — the view `include_str!` handed this
    /// module.
    fn in_the_product_raw(needle: bt_source::Needle) -> usize {
        in_the_product(&found(needle, bt_source::View::Raw))
    }

    #[test]
    fn ime_outbound_sites_keep_their_trace_calls() {
        let body = |name: &str| method_body("Runtime", name);
        assert_eq!(
            in_the_product_raw(bt_source::needle!(bt_source::Pattern::text(
                ".set_ime_allowed("
            ))),
            2
        );
        // One winit call, inside its owner-thread door (A1d), and one road to the door.
        assert_eq!(
            in_the_product_raw(bt_source::needle!(bt_source::Pattern::text(
                ".set_ime_cursor_area("
            ))),
            1
        );
        assert_eq!(
            in_the_product_raw(bt_source::needle!(bt_source::Pattern::text(
                "owner_door::set_ime_cursor_area("
            ))),
            1
        );
        assert_eq!(
            in_the_product_raw(bt_source::needle!(bt_source::Pattern::text(
                ".ime_system_caret.update("
            ))),
            1
        );
        assert_eq!(
            in_the_product_raw(bt_source::needle!(bt_source::Pattern::text(
                "ime_outbound::allowed_line(true,"
            ))),
            2
        );
        for (method, trace) in [
            ("keyboard_owner", "self.trace_ime_owner(owner)"),
            ("ime_input", "self.trace_ime_input(&event)"),
            ("ime_input", "self.trace_ime_ruling(&event, &here, ruling)"),
            (
                "cancel_composition",
                "self.trace_ime_cancel(started_in, reason, None)",
            ),
            (
                "cancel_composition",
                "self.trace_ime_cancel(started_in, reason, Some(told))",
            ),
            (
                "cancel_composition",
                "bt_platform::cancel_composition(reason)",
            ),
            ("apply_ime_cursor_area", "self.trace_ime_area(area, action)"),
            ("apply_ime_cursor_area", "ime_outbound::caret_line("),
            ("offer_ime_caret", "self.trace_ime_area("),
            ("reoffer_ime_cursor_area", "self.trace_ime_area("),
            (
                "flush_ime_cursor_area",
                "self.apply_ime_cursor_area(area, \"flushed\")",
            ),
        ] {
            assert!(body(method).contains(trace), "{method}: {trace}");
        }
        let input = body("ime_input");
        assert!(input.find("trace_ime_ruling") < input.find("if !ruling.deliver"));
        assert!(
            in_the_product_raw(bt_source::needle!(bt_source::Pattern::text(
                "self.trace_ime_frame(&terminal_frame, written)"
            ))) > 0
        );
        for reason in [
            "ime_disabled",
            "cancel_composition",
            "window_teardown",
            "window_blur",
        ] {
            assert_eq!(
                in_the_product_raw(bt_source::Needle::new(bt_source::Pattern::text(&format!(
                    "destroy_ime_caret(\"{reason}\")"
                )))),
                1
            );
        }
        // This module owns the shared platform door (including the portable
        // no-op), and it owns it alone: the product destroys the caret exactly
        // once, and the body pin says which method that one site stands in.
        let door = method_body("Runtime", "destroy_ime_caret");
        assert!(door.contains("caret_line(\"destroy\", reason, None)"));
        assert!(door.contains("self.window.ime_system_caret.destroy()"));
        assert_eq!(
            in_the_product_raw(bt_source::needle!(bt_source::Pattern::text(
                ".ime_system_caret.destroy()"
            ))),
            1
        );
    }

    #[test]
    fn ime_ruling_reports_swallow_reroute_and_same_kind_mismatch() {
        let event = Ime::Preedit("private-preedit".to_owned(), None);
        for (here, reason) in [
            (CompositionOrigin::Modal, "owner_swallows"),
            (CompositionOrigin::FilesTree(None), "owner_swallows"),
            (CompositionOrigin::Palette, "field_route"),
            (CompositionOrigin::Shell(None), "shell_route"),
        ] {
            let ruling = composition_ruling(&Composing::Idle, &here, ComposingEvent::Opens);
            let line = ruling_line(&event, &Composing::Idle, &here, ruling);
            assert!(line.ends_with(reason));
            assert!(!line.contains("private-preedit"));
        }
        // No origin Debug output: RenameSubject can contain file names.
        let here = CompositionOrigin::Search(Some(SeatId(2)));
        let state = Composing::Retiring(CompositionOrigin::Search(Some(SeatId(1))));
        let event = Ime::Commit("private-commit".to_owned());
        let ruling = composition_ruling(&state, &here, ComposingEvent::Commits);
        assert_eq!(
            ruling_line(&event, &state, &here, ruling),
            "IME_RULING IME_IN kind=Commit bytes=14 origin=Some(Search) destination=Search same_origin=false deliver=false next=Idle reason=origin_mismatch"
        );
    }

    #[test]
    fn ime_format_values_never_include_input_text() {
        let text = "private-synthetic-preedit";
        let preedit = Ime::Preedit(text.to_owned(), Some((1, 2)));
        let state = Composing::In(CompositionOrigin::Shell(None));
        let here = CompositionOrigin::Modal;
        let ruling = composition_ruling(&state, &here, ComposingEvent::Commits);
        let area = ImeCursorArea {
            x: -3,
            y: 4,
            width: 5,
            height: 6,
        };
        let lines = [
            (
                input_line(&preedit),
                "IME_IN kind=Preedit bytes=25 cursor=Some((1, 2))",
            ),
            (
                input_line(&Ime::Commit(text.to_owned())),
                "IME_IN kind=Commit bytes=25",
            ),
            (input_line(&Ime::Enabled), "IME_IN kind=Enabled bytes=0"),
            (input_line(&Ime::Disabled), "IME_IN kind=Disabled bytes=0"),
            (
                allowed_line(true, "new_window"),
                "IME_OUT_ALLOWED value=true reason=new_window",
            ),
            (
                area_line(area, "flushed"),
                "IME_OUT_AREA x=-3 y=4 width=5 height=6 action=flushed",
            ),
            (
                cancel_line(ImeOwner::Shell, "owner_changed", Some(false)),
                "IME_OUT_CANCEL owner=Shell reason=owner_changed result=Some(false)",
            ),
            (
                owner_line(
                    Some((ImeOwner::Shell, "shell")),
                    ImeOwner::Modal,
                    "first_run",
                ),
                "IME_OWNER old=Some(Shell) new=Modal previous_cause=shell cause=first_run",
            ),
            (
                ruling_line(&Ime::Commit(text.to_owned()), &state, &here, ruling),
                "IME_RULING IME_IN kind=Commit bytes=25 origin=Some(Shell) destination=Modal same_origin=false deliver=false next=Idle reason=origin_mismatch",
            ),
        ];
        let facts = DrawFacts {
            bytes: text.len(),
            visible: false,
            row: 3,
            column: 7,
            alt: true,
            area,
        };
        let draw = draw_line("cursor_out_of_view", facts);
        assert_eq!(
            draw,
            "IME_DRAW surface=terminal drawn=false reason=cursor_out_of_view bytes=25 cursor_visible=false row=3 column=7 alt=true x=-3 y=4 width=5 height=6"
        );
        assert!(!draw.contains(text));
        assert_eq!(
            draw_line(
                "drawn",
                DrawFacts {
                    visible: true,
                    ..facts
                }
            ),
            "IME_DRAW surface=terminal drawn=true reason=drawn bytes=25 cursor_visible=true row=3 column=7 alt=true x=-3 y=4 width=5 height=6"
        );
        assert_eq!(
            caret_line("update", "cursor_area", Some((-3, 4))),
            "IME_OUT_CARET action=update reason=cursor_area position=Some((-3, 4))"
        );
        assert_eq!(
            caret_line("destroy", "window_blur", None),
            "IME_OUT_CARET action=destroy reason=window_blur position=None"
        );
        for (actual, expected) in lines {
            assert_eq!(actual, expected);
            assert!(!actual.contains(text));
        }
        let unicode = "synthetic-\u{e9}\n";
        assert_eq!(
            input_line(&Ime::Preedit(unicode.to_owned(), None)),
            "IME_IN kind=Preedit bytes=13 cursor=None"
        );
        assert!(!input_line(&Ime::Commit(unicode.to_owned())).contains(unicode));
    }

    #[test]
    fn ime_draw_answers_and_change_latch() {
        for (owner, in_view, nonzero, written, expected) in [
            (false, true, true, true, "owner_mismatch"),
            (true, false, true, false, "cursor_out_of_view"),
            (true, true, false, true, "zero_size_rectangle"),
            (true, true, true, false, "no_visible_cells"),
            (true, true, true, true, "drawn"),
        ] {
            assert_eq!(draw_reason(owner, in_view, nonzero, written), expected);
        }
        let last = Cell::new(None);
        assert!(changed(&last, Some("drawn")));
        assert!(!changed(&last, Some("drawn")));
        assert!(changed(&last, Some("cursor_out_of_view")));
        assert!(!changed(&last, Some("cursor_out_of_view")));
        assert!(!changed(&last, None));
        assert!(changed(&last, Some("cursor_out_of_view")));
    }
}
