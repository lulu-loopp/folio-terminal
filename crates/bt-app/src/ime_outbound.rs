//! Text-free, opt-in observations of the IME boundary. No routing decisions live here.
use super::*;
pub(crate) use bt_platform::ime_trace::{enabled, line};
use std::cell::Cell;

pub fn install() {
    bt_platform::ime_trace::install(|message| {
        // One file, one writer: the focus census (`ime_report`) owns the sink.
        ime_report::TRACE.line(|| format!("{:?} {message}", Instant::now()));
    });
}

#[derive(Default)]
pub struct State {
    owner: Cell<Option<(ImeOwner, &'static str)>>,
    draw: Cell<Option<&'static str>>,
}

/// A quiet answer emits nothing. None resets the next composition's first answer.
fn changed<T: Copy + PartialEq>(last: &Cell<Option<T>>, next: Option<T>) -> bool {
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

fn cancel_line(owner: ImeOwner, reason: &'static str, result: Option<bool>) -> String {
    format!("IME_OUT_CANCEL owner={owner:?} reason={reason} result={result:?}")
}

fn owner_line(old: Option<(ImeOwner, &'static str)>, new: ImeOwner, cause: &'static str) -> String {
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

fn ruling_line(
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
fn draw_reason(owner_matches: bool, visible: bool, nonzero: bool, written: bool) -> &'static str {
    if !owner_matches {
        "owner_mismatch"
    } else if !visible {
        "cursor_invisible"
    } else if !nonzero {
        "zero_size_rectangle"
    } else if !written {
        "no_visible_cells"
    } else {
        "drawn"
    }
}

#[derive(Clone, Copy)]
struct DrawFacts {
    bytes: usize,
    visible: bool,
    row: u32,
    column: u32,
    alt: bool,
    area: ImeCursorArea,
}

fn draw_line(reason: &'static str, facts: DrawFacts) -> String {
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

impl Runtime<'_> {
    pub(super) fn trace_ime_owner(&self, keyboard: KeyboardOwner) {
        if !enabled() {
            return;
        }
        let owner = ime_owner(keyboard);
        let cause = match owner {
            ImeOwner::Modal if self.app.quit.as_ref().is_some_and(quit::Quit::is_asking) => {
                "quit_dialog"
            }
            ImeOwner::Modal if self.window.dirty_gate.is_open() => "dirty_gate",
            ImeOwner::Modal if self.window.first_run.is_open() => "first_run",
            ImeOwner::Modal if self.window.psreadline_invite.is_open() => "psreadline_invite",
            ImeOwner::Modal if self.window.settings.is_open() => "settings",
            ImeOwner::Modal => match popup_takes_the_key(self.popups_up()) {
                Some(Popup::Profile) => "profile_popup",
                Some(Popup::Root) => "root_popup",
                Some(Popup::File) => "file_popup",
                Some(Popup::Pane) => "pane_popup",
                Some(Popup::GraphFilter) => "graph_filter_popup",
                Some(Popup::Preview) => "preview_popup",
                Some(Popup::GitMenu) => "git_popup",
                Some(Popup::TermMenu) => "terminal_popup",
                Some(Popup::Tab) => "tab_popup",
                Some(Popup::Palette) => "palette_popup",
                None => "modal",
            },
            ImeOwner::Rename => "rename",
            ImeOwner::GraphSearch => "graph_search",
            ImeOwner::GitPrompt => "git_prompt",
            ImeOwner::Preview => "preview",
            ImeOwner::Search => "search",
            ImeOwner::Palette => "palette",
            ImeOwner::FilesTree => "files_tree",
            ImeOwner::Shell => "shell",
        };
        let last = self.window.ime_outbound.owner.replace(Some((owner, cause)));
        if last.map(|v| v.0) != Some(owner) {
            self.trace_ime_line(|| owner_line(last, owner, cause));
        }
    }

    pub(super) fn trace_ime_line(&self, message: impl FnOnce() -> String) {
        line(|| {
            format!(
                "window={} {}",
                u64::from(self.window.window.id()),
                message()
            )
        });
    }

    pub(super) fn trace_ime_input(&self, event: &Ime) {
        if !enabled() {
            return;
        }
        if matches!(event, Ime::Commit(_) | Ime::Disabled)
            || matches!(event, Ime::Preedit(text, _) if text.is_empty())
        {
            self.window.ime_outbound.draw.set(None);
        }
        self.trace_ime_line(|| input_line(event));
    }

    pub(super) fn trace_ime_ruling(
        &self,
        event: &Ime,
        here: &CompositionOrigin,
        ruling: CompositionRuling,
    ) {
        self.trace_ime_line(|| ruling_line(event, &self.window.composing_in, here, ruling));
    }

    pub(super) fn trace_ime_cancel(
        &self,
        owner: ImeOwner,
        reason: &'static str,
        result: Option<bool>,
    ) {
        if !enabled() {
            return;
        }
        if result.is_none() {
            self.window.ime_outbound.draw.set(None);
        }
        self.trace_ime_line(|| cancel_line(owner, reason, result));
    }

    pub(super) fn trace_ime_area(&self, area: ImeCursorArea, action: &'static str) {
        self.trace_ime_line(|| area_line(area, action));
    }

    pub(super) fn trace_ime_frame(&self, frame: &ViewportFrame, written: bool) {
        // Called only on the enabled path, before the unchanged-frame early return.
        let Some(preedit) = self.window.preedit.as_ref().filter(|p| !p.text.is_empty()) else {
            self.window.ime_outbound.draw.set(None);
            return;
        };
        let area = window_ime_cursor_area(
            self.window.renderer.seat_viewport(),
            self.window.renderer.ime_cursor_area(frame),
        );
        let reason = draw_reason(
            ime_owner(self.keyboard_owner()) == ImeOwner::Shell,
            frame.cursor.visible,
            area.width != 0 && area.height != 0,
            written,
        );
        if changed(&self.window.ime_outbound.draw, Some(reason)) {
            self.trace_ime_line(|| {
                draw_line(
                    reason,
                    DrawFacts {
                        bytes: preedit.text.len(),
                        visible: frame.cursor.visible,
                        row: frame.cursor.row,
                        column: frame.cursor.column,
                        alt: frame_is_alternate_screen(frame),
                        area,
                    },
                )
            });
        }
    }

    pub(super) fn destroy_ime_caret(&mut self, reason: &'static str) {
        self.trace_ime_line(|| caret_line("destroy", reason, None));
        self.window.ime_system_caret.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── what this module asks the crate instead ───────────────────────────
    //
    // **P3's equivalence commit for this batch**
    // (`docs/plans/bt-app-split-prep.md` §6.3, and §6.0 rule 3). Nothing is
    // deleted here: each reading is taken twice — once from
    // `include_str!("main.rs")` or from this file's own text, once from
    // `bt-source` — and `agree`/`agreed` are where the two are required to be
    // the same answer. The deletion is the commit after this one.
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

    /// **How many of these occurrences a product build compiles.**
    ///
    /// Two grains, because this tree says "test" two ways and a reader that
    /// took only one of them would count its own assertions. A file reached
    /// through a `#[cfg(test)] mod x;` declaration is not compiled into the
    /// product at all, which is `FileRecord::permits_product` over
    /// `Index::file_at` (§2.3, the pilot's `in_product`). An inline
    /// `#[cfg(test)] mod` inside a file the product *does* compile is not a
    /// file, so §2.4's identity carries its predicate instead, which is
    /// `clipboard_path_tests`' `in_product_items`. This module needs both: the
    /// spellings counted here are written again in its own assertions — in
    /// this commit, by the older of the two readings standing beside the newer
    /// one — and again in whole test files elsewhere in the package.
    ///
    /// The owner is the smallest callable holding the match, which is
    /// `Found::owners`' own rule taken one occurrence at a time so that the
    /// file grain can stand beside it. An occurrence in no callable — an
    /// `impl` header, a `const` — is left in, because nothing about a `cfg`
    /// says otherwise and dropping it quietly is the failure this preparation
    /// is about.
    fn in_the_product(found: &bt_source::Found) -> usize {
        let index = source_index();
        found
            .occurrences()
            .iter()
            .filter(|occurrence| {
                index
                    .file_at(occurrence.span.start())
                    .is_some_and(bt_source::FileRecord::permits_product)
                    && index
                        .items()
                        .iter()
                        .filter(|record| occurrence.span.within(record.whole()))
                        .min_by_key(|record| record.whole().len())
                        .is_none_or(|record| {
                            !record
                                .variant()
                                .predicates()
                                .iter()
                                .any(|predicate| predicate == "test")
                        })
            })
            .count()
    }

    /// The same count of one raw needle — the view `include_str!` handed this
    /// module.
    fn in_the_product_raw(needle: bt_source::Needle) -> usize {
        in_the_product(&found(needle, bt_source::View::Raw))
    }

    /// **The file's answer and the crate's, compared**, handing the file's
    /// back so the assertion after it is the one that was always there.
    fn agree<T: std::fmt::Debug + PartialEq>(what: &str, file: T, crate_reading: T) -> T {
        assert_eq!(
            file, crate_reading,
            "{what}: this module's reading of the file and the crate's disagree"
        );
        file
    }

    /// **`body`'s answer and the crate's, compared as bytes.** `body` hands
    /// back everything after `    fn name(` and stops before the closing
    /// `\n    }\n`, so the crate's body minus that closing line has to stand in
    /// the slice with nothing but the rest of the declaration in front of it.
    fn agreed(old: &'static str, name: &str) -> &'static str {
        let new = method_body("Runtime", name);
        let trimmed = &new[..new.rfind('\n').expect("a method's body spans lines")];
        let at = old.find(trimmed).unwrap_or_else(|| {
            panic!(
                "`Runtime::{name}`: this file's slice and the crate's body are not the same bytes"
            )
        });
        assert!(
            !old[..at].contains('{'),
            "`Runtime::{name}`: the crate's body stands inside this file's slice rather than at \
             the head of it"
        );
        old
    }

    #[test]
    fn ime_outbound_sites_keep_their_trace_calls() {
        let source = include_str!("main.rs");
        let body = |name: &str| {
            agreed(
                source
                    .split(&format!("    fn {name}("))
                    .nth(1)
                    .unwrap()
                    .split("\n    }\n")
                    .next()
                    .unwrap(),
                name,
            )
        };
        assert_eq!(
            agree(
                "the allowed door",
                source.matches(".set_ime_allowed(").count(),
                in_the_product_raw(bt_source::needle!(bt_source::Pattern::text(
                    ".set_ime_allowed("
                ))),
            ),
            2
        );
        assert_eq!(
            agree(
                "the cursor-area door",
                source.matches(".set_ime_cursor_area(").count(),
                in_the_product_raw(bt_source::needle!(bt_source::Pattern::text(
                    ".set_ime_cursor_area("
                ))),
            ),
            1
        );
        assert_eq!(
            agree(
                "the system caret's update",
                source.matches(".ime_system_caret.update(").count(),
                in_the_product_raw(bt_source::needle!(bt_source::Pattern::text(
                    ".ime_system_caret.update("
                ))),
            ),
            1
        );
        assert_eq!(
            agree(
                "the allowed line",
                source.matches("ime_outbound::allowed_line(true,").count(),
                in_the_product_raw(bt_source::needle!(bt_source::Pattern::text(
                    "ime_outbound::allowed_line(true,"
                ))),
            ),
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
        assert!(agree(
            "the frame trace",
            source.contains("self.trace_ime_frame(&terminal_frame, written)"),
            in_the_product_raw(bt_source::needle!(bt_source::Pattern::text(
                "self.trace_ime_frame(&terminal_frame, written)"
            ))) > 0,
        ));
        assert_eq!(source.matches(".ime_system_caret.destroy()").count(), 0);
        for reason in [
            "ime_disabled",
            "cancel_composition",
            "window_teardown",
            "window_blur",
        ] {
            assert_eq!(
                agree(
                    "a reasoned destruction",
                    source
                        .matches(&format!("destroy_ime_caret(\"{reason}\")"))
                        .count(),
                    in_the_product_raw(bt_source::Needle::new(bt_source::Pattern::text(&format!(
                        "destroy_ime_caret(\"{reason}\")"
                    )))),
                ),
                1
            );
        }
        // This module owns the shared platform door (including the portable no-op).
        let helper = include_str!("ime_outbound.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(helper.contains("caret_line(\"destroy\", reason, None)"));
        assert!(helper.contains("self.window.ime_system_caret.destroy()"));
        // The same two facts, asked of the door rather than of this file's text
        // above its own tests, and joined to the count that used to be taken of
        // `main.rs` alone: the product destroys the caret exactly once, and the
        // method that does it is this module's own.
        let door = method_body("Runtime", "destroy_ime_caret");
        assert!(door.contains("caret_line(\"destroy\", reason, None)"));
        assert!(door.contains("self.window.ime_system_caret.destroy()"));
        assert_eq!(
            in_the_product_raw(bt_source::needle!(bt_source::Pattern::text(
                ".ime_system_caret.destroy()"
            ))),
            source.matches(".ime_system_caret.destroy()").count()
                + usize::from(helper.contains("self.window.ime_system_caret.destroy()")),
            "the package's product sites are `main.rs`'s none plus this module's own door"
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
        let draw = draw_line("cursor_invisible", facts);
        assert_eq!(
            draw,
            "IME_DRAW surface=terminal drawn=false reason=cursor_invisible bytes=25 cursor_visible=false row=3 column=7 alt=true x=-3 y=4 width=5 height=6"
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
        for (owner, visible, nonzero, written, expected) in [
            (false, true, true, true, "owner_mismatch"),
            (true, false, true, false, "cursor_invisible"),
            (true, true, false, true, "zero_size_rectangle"),
            (true, true, true, false, "no_visible_cells"),
            (true, true, true, true, "drawn"),
        ] {
            assert_eq!(draw_reason(owner, visible, nonzero, written), expected);
        }
        let last = Cell::new(None);
        assert!(changed(&last, Some("drawn")));
        assert!(!changed(&last, Some("drawn")));
        assert!(changed(&last, Some("cursor_invisible")));
        assert!(!changed(&last, Some("cursor_invisible")));
        assert!(!changed(&last, None));
        assert!(changed(&last, Some("cursor_invisible")));
    }
}
