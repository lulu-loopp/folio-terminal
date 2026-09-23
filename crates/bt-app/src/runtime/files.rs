//! `files` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{
    ColumnKeyboard, FOOT_REVEAL_FEEDBACK, FileMenuTarget, FileMenuTreeRow, FilesEditPlace,
    FilesEditRow, FilesFocusArrival, FolderPick, FootSaying, LeafId, PointerTarget, ReferenceCard,
    RenameSubject, RevealedFoot, RowActivation, RowHost, Runtime, SplitSeed, TabClick, TabRename,
    answers_for, column_keyboard, columns_wanting_git, dropped_files_seat_at, files,
    files_key_under, files_key_within, files_keyboard_seat_of, files_open_chain,
    files_row_activation, files_row_counts_clicks, files_row_entry, files_row_menu_subject, float,
    float_dock_label, folder_pick_outcome, foot_revealed_label, git_panel, hang_watch,
    home_shortened_path, i18n, marks, name_is_writable, palette_index, press_files_node, preview,
    profiles, reroot_files_state, same_path_ignoring_case, seats, settings, toast,
};
use anyhow::Result;
use bt_layout::SeatId;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};
use winit::dpi::PhysicalPosition;
use winit::event::{KeyEvent, MouseScrollDelta};

impl Runtime<'_> {
    /// `Choose a folder…` in the starting-directory picker.
    pub(crate) fn browse_for_start_folder(&mut self) {
        let Some(index) = self.window.settings.editor().map(|editor| editor.index) else {
            return;
        };
        let start = match profiles::start_at(index) {
            profiles::StartAt::Fixed(path) => Some(path),
            _ => None,
        };
        match self.window.folder_picker.request(start.as_deref()) {
            Ok(true) => self.window.folder_pick = Some(FolderPick::ProfileStart(index)),
            Ok(false) => {}
            Err(error) => eprintln!("recoverable folder chooser failure: {error}"),
        }
    }

    /// **Put a box on one row of a files column** (B5, user ruling 2026-08-25).
    ///
    /// [`Self::open_preview_rename`] one surface over, and the same editor: the
    /// whole name in the box, the stem selected, the suffix left standing. A
    /// folder is renamed by the same door as a file, because on the disk they
    /// are the same act.
    ///
    /// **Only a docked column**, and that is geometry rather than policy — a
    /// [`RowHost::Git`] draws a repository's report and not a tree, and a
    /// [`RowHost::Float`] draws its rows through `float`'s own body, which has
    /// no box for an editor to be measured into. Neither refusal is a guard
    /// against a caller: the row is offered from `file_menu`, and a host that
    /// cannot draw the box declines here rather than opening an editor nobody
    /// can see.
    pub(crate) fn open_files_row_rename(&mut self, host: RowHost, key: &str) -> Result<()> {
        let RowHost::Column(seat) = host else {
            return Ok(());
        };
        let now = Instant::now();
        // Asked of the live tree rather than of the painted list, on
        // [`Self::fold_files_row`]'s reason: the menu can stand open while a
        // directory lands underneath it, and a key that no longer names a row
        // renames nothing.
        let Some(name) = self
            .files_trees(now)
            .get(&seat)
            .and_then(|tree| tree.rows.iter().find(|row| row.key == key))
            .map(|row| row.name.clone())
        else {
            return Ok(());
        };
        let leaf = self.leaf_here(seat);
        self.window.rename = Some(TabRename::open_files_row(leaf, key, &name));
        // A caret that arrives mid-blink arrives invisible half the time.
        self.window.rename_blink.reset(now, self.app.motion);
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// **Commit a tree row's draft to the filesystem** (B5, user ruling
    /// 2026-08-25).
    ///
    /// [`Self::rename_preview_file`]'s four silent refusals and its one card,
    /// for that function's own stated reasons — an empty name, an unchanged
    /// name, a name Windows will not take and a name a *different* entry already
    /// has all leave the row as it was and say nothing, because the reader can
    /// see whether it worked; a handle another program is holding is a fact
    /// about the machine that no box can show, so it raises a card.
    ///
    /// **The collision check is this window's and not `MoveFileExW`'s**, again
    /// for that function's reason: `std::fs::rename` carries
    /// `MOVEFILE_REPLACE_EXISTING` on this platform, and a rename that eats
    /// somebody's file is not a rename. Case is ignored against the old path so
    /// that `notes.md` → `Notes.md` is the legitimate rename of one entry into
    /// its own name rather than a collision with itself.
    ///
    /// What follows the row is the *folder*, not a buffer: the tree re-reads the
    /// directory the entry lives in, which is the same call a rename made in the
    /// preview head already makes and the same one an Explorer rename would be
    /// noticed by if the column had a watcher. A document open on that path is
    /// re-pointed too, because a file may be open in a preview and listed in a
    /// column at the same time and a window that renamed it in one place and not
    /// the other would be holding two names for one file.
    pub(crate) fn rename_files_row(&mut self, leaf: LeafId, key: &str, draft: &str) -> Result<()> {
        let Some(tab) = self.tab_slot_of(leaf.tab) else {
            return Ok(());
        };
        let root = self.window.tabs[tab].files_state(leaf.seat).root;
        if root.is_empty() {
            return Ok(());
        }
        let old = files::full_path(&root, key);
        let Some(directory) = old.parent().map(std::path::Path::to_path_buf) else {
            return Ok(());
        };
        let name = draft.trim();
        let was = old
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        if name.is_empty() || name == was || !name_is_writable(name) {
            return Ok(());
        }
        let new = directory.join(name);
        if new.exists() && !same_path_ignoring_case(&old, &new) {
            return Ok(());
        }
        if let Err(error) = hang_watch::during(hang_watch::Station::RenameDisk, || {
            std::fs::rename(&old, &new)
        }) {
            return self.toast(
                toast::ToastKind::Error,
                toast::ToastAnchor::FilesColumn(leaf.seat),
                Some(was),
                i18n::not_renamed(&error.to_string()),
            );
        }
        // A file that is also open in a preview keeps one identity across the
        // window, which is the one-buffer-per-file law read from the other end:
        // the column moved it, and the pool has to be told by whoever moved it.
        let source = preview::PreviewSource::file(&old);
        if self.preview_pool.get(&source).is_some() {
            self.follow_renamed_preview(&source, &new, name);
        }
        self.refresh_files_dirs_at(&directory);
        self.mark_session_dirty(Instant::now());
        Ok(())
    }

    /// **Put a box where a new file or folder is going to be** (0.3,
    /// `New file…` / `New folder…`).
    ///
    /// [`Self::open_files_row_rename`] one row down: the same editor, seeded
    /// empty, standing in the place the finished row will occupy. Which place
    /// that is, is `files::insert_pending_row`'s answer and not this function's.
    ///
    /// **The folder is opened first**, and that is the verb rather than a
    /// convenience: a row typed into a folded folder would be a field with
    /// nowhere to stand, and `Expand` is a row of the very menu this one was
    /// raised from. A folder already open is left exactly as it is — this is
    /// `open_files_path_to`'s reasoning about the difference between opening a
    /// folder and toggling one.
    ///
    /// **Only a docked column**, for [`Self::open_files_row_rename`]'s stated
    /// reason: a float draws its rows through `float`'s own body, which has no
    /// box for an editor to be measured into, and a `RowHost::Git` draws a
    /// report rather than a tree.
    pub(crate) fn open_files_row_new(
        &mut self,
        host: RowHost,
        parent: &str,
        folder: bool,
    ) -> Result<()> {
        let RowHost::Column(seat) = host else {
            return Ok(());
        };
        let now = Instant::now();
        let active = self.window.active_tab;
        if !parent.is_empty() {
            let Some(state) = self.window.tabs[active].files.get_mut(&seat) else {
                return Ok(());
            };
            if state.open.insert(parent.to_owned()) {
                self.window.tabs[active]
                    .file_trees
                    .entry(seat)
                    .or_default()
                    .settle_row(parent);
                self.refresh_files_dir(seat, parent);
            }
        }
        let leaf = self.leaf_here(seat);
        // **The folder is asked how it tells names apart, once, here** (D8(a)).
        // The box's advisory runs on every frame the caret blinks and may not
        // touch the disk; this is the one moment it can be asked, and the folder
        // the name is going into is exactly what has to answer — a tree under
        // WSL's case-sensitive flag and an ordinary volume disagree about
        // whether `notes.md` is `Notes.md`, and the window must not guess.
        let root = self.window.tabs[active].files_state(seat).root;
        let folds_case = bt_platform::directory_folds_case(&files::full_path(&root, parent));
        self.window.rename = Some(TabRename::open_files_new(leaf, parent, folder, folds_case));
        // A caret that arrives mid-blink arrives invisible half the time.
        self.window.rename_blink.reset(now, self.app.motion);
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// **Make the file or the folder the open box was naming** (0.3), and answer
    /// **which refusal stopped it** — `None` when the box is finished with,
    /// whether that is because the entry was made or because nothing here can
    /// help.
    ///
    /// A refusal keeps the field open on the draft that raised it —
    /// [`seats::FilesRowEdit::refused`]'s channel, which is the address bar's
    /// and not the rename's. A rename refuses in silence because there is a name
    /// underneath to fall back to and the reader can see it; a new entry has no
    /// name underneath, so Enter doing nothing at all would be the window
    /// refusing without saying which of its refusals it meant.
    ///
    /// **Which is why it is the refusal and not a `bool`** (D8(a) of the
    /// 2026-09-11 review). The box's own advisory is a prediction of this
    /// answer, and a prediction that is wrong used to be invisible: the field
    /// looked valid and Enter did nothing. The caller hands this back to the
    /// editor ([`TabRename::refuse`]) so that the refusal a reader sees is the
    /// refusal that stopped the commit.
    ///
    /// **`create_new` and not `create`** for the file: `File::create` truncates
    /// an existing file, and the folder can gain an entry between the judgement
    /// above and the call — a `New file…` that emptied somebody's notes because
    /// the two raced is not a refusal, it is a loss. The directory call has the
    /// same property already, which is why the two arms are not symmetrical in
    /// their spelling.
    ///
    /// **The listing is checked as well as the call**, for `rename_files_row`'s
    /// own reason turned round: the platform's answer to "it is already there"
    /// is an error that arrives *after* the field has closed on a name that did
    /// nothing, and the point of this refusal is that it is said in the box the
    /// name was typed in.
    ///
    /// A failure the disk itself raises — a folder that is read-only, a name the
    /// volume will not take — is a fact about the machine that no box can show,
    /// so it raises a card and closes the field, which is the same division
    /// `rename_files_row` draws between its silent refusals and its one card.
    pub(crate) fn create_files_row(
        &mut self,
        leaf: LeafId,
        parent: &str,
        folder: bool,
        draft: &str,
    ) -> Result<Option<files::NewNameRefusal>> {
        let Some(tab) = self.tab_slot_of(leaf.tab) else {
            return Ok(None);
        };
        let root = self.window.tabs[tab].files_state(leaf.seat).root;
        if root.is_empty() {
            return Ok(None);
        }
        let name = draft.trim();
        let directory = files::full_path(&root, parent);
        let path = directory.join(name);
        // Every refusal in one sentence, and the collision is decided **here**
        // rather than left to the platform for `rename_files_row`'s own reason
        // turned round: `File::create` truncates, and a `New file…` that emptied
        // somebody's notes because the name was already taken is not a refusal,
        // it is a loss. `create_new` below is the race-tight half of the same
        // decision — this one is what lets the box say so.
        let refusal = files::judge_new_name(draft)
            .or_else(|| path.exists().then_some(files::NewNameRefusal::Taken));
        if refusal.is_some() {
            return Ok(refusal);
        }
        let made = if folder {
            std::fs::create_dir(&path)
        } else {
            std::fs::File::create_new(&path).map(drop)
        };
        if let Err(error) = made {
            self.toast(
                toast::ToastKind::Error,
                toast::ToastAnchor::FilesColumn(leaf.seat),
                Some(name.to_owned()),
                i18n::not_created(&error.to_string()),
            )?;
            return Ok(None);
        }
        // Ahead of the watcher, which is `refresh_files_dirs_at`'s own sentence:
        // this window made the change, so it knows the truth a whole quiet
        // window before the kernel's account of it arrives — and the row has to
        // be in the tree before the selection below can land on it.
        self.refresh_files_dirs_at(&directory);
        // **The new row is selected, and it is not opened.** `New file…` says it
        // makes a file; opening one is the next click, and a menu row that did
        // more than its own name says is a row a reader cannot learn. Selecting
        // it is not more — it is where the press left the reader standing.
        self.open_files_path_to(leaf.seat, &files::child_key(parent, name))?;
        self.mark_session_dirty(Instant::now());
        Ok(None)
    }

    /// **Send one tree row to the Recycle Bin** (0.3, `Delete`).
    ///
    /// `bt_platform::recycle`, which is the same door `Delete scheme` goes
    /// through and never `remove_file` or `remove_dir_all`: the row goes
    /// somewhere it can be fetched back from, and that is what makes the row
    /// safe to offer with no question in front of it.
    ///
    /// **A folder goes whole**, in one call naming the folder — the shell moves
    /// the tree and puts it back the same way, and a walk that recycled the
    /// children one at a time would leave a reader restoring a folder file by
    /// file.
    ///
    /// Three answers, and the three of them are the whole of what this verb can
    /// do:
    ///
    /// * the bin took it — the folder above it is re-read, ahead of the watcher;
    /// * **the shell asked and the answer was no** (`Ok(false)`) — the one case
    ///   the bin cannot take raises Windows' own "this will be deleted
    ///   permanently" prompt, and a reader who declines it has just decided
    ///   something they are still looking at. Nothing happened and nothing is
    ///   said, which is `delete_scheme_file`'s own answer to the same prompt;
    /// * the call failed — a handle another program is holding, a folder this
    ///   account may not write. That is a fact about the machine that no row can
    ///   show, so it is the one refusal here that speaks.
    ///
    /// **The key is resolved against the live tree first** (adversarial review
    /// 2026-09-11, note N2), which is [`Self::open_files_row_rename`]'s own
    /// guard and it is owed here twice over: the menu can stand open while a
    /// directory lands underneath it, and a key that no longer names a row names
    /// nothing to recycle. The two verbs that act on a row now ask the same
    /// question before they act, so neither can be the one that resolves a stale
    /// key against a root that has since changed.
    pub(crate) fn delete_files_row(&mut self, host: RowHost, key: &str) -> Result<()> {
        let RowHost::Column(seat) = host else {
            return Ok(());
        };
        let active = self.window.active_tab;
        let root = self.window.tabs[active].files_state(seat).root;
        if root.is_empty() || key.is_empty() {
            return Ok(());
        }
        if !self
            .files_trees(Instant::now())
            .get(&seat)
            .is_some_and(|tree| tree.rows.iter().any(|row| row.key == key))
        {
            return Ok(());
        }
        let path = files::full_path(&root, key);
        let name = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        match bt_platform::recycle(&path) {
            Ok(false) => return Ok(()),
            Ok(true) => {}
            Err(error) => {
                return self.toast(
                    toast::ToastKind::Error,
                    toast::ToastAnchor::FilesColumn(seat),
                    Some(name),
                    i18n::not_deleted(&error),
                );
            }
        }
        if let Some(directory) = path.parent() {
            let directory = directory.to_path_buf();
            self.refresh_files_dirs_at(&directory);
        }
        // The selection cannot stay on a row that has gone. It is dropped rather
        // than moved to a neighbour: which neighbour is the next row *after the
        // re-read*, and the re-read has not happened on this frame — a guess made
        // now would put the accent on whichever row happened to be there before.
        if let Some(state) = self.window.tabs[active].files.get_mut(&seat)
            && state.sel.as_deref() == Some(key)
        {
            state.sel = None;
        }
        self.mark_session_dirty(Instant::now());
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// Ask every files column that is showing this directory to read it again.
    ///
    /// **Kept now that [`files_watch`] exists**, and not as a belt beside its
    /// braces: this window is the one that made the change, so it knows the
    /// truth a whole quiet window before the kernel's account of it arrives, and
    /// a row that follows its file on the next frame is not the same thing as a
    /// row that follows it in three hundred milliseconds. The watcher's own
    /// reading lands afterwards and finds the listing already correct, which
    /// [`files::DirCache::accept`] answers with "not a change" and no second
    /// repaint. A folder on a share this process cannot watch has only this.
    pub(crate) fn refresh_files_dirs_at(&mut self, directory: &std::path::Path) {
        let active = self.window.active_tab;
        let asks: Vec<(SeatId, String)> = self.window.tabs[active]
            .files
            .iter()
            .filter_map(|(seat, state)| Some((*seat, files_key_under(&state.root, directory)?)))
            .collect();
        for (seat, key) in asks {
            self.refresh_files_dir(seat, &key);
        }
    }

    /// **`addFilesPane` and its undo, as one verb** — the `˅` menu's `Files pane`
    /// row and `Ctrl+Shift+B` both land here.
    ///
    /// A toggle rather than an adder, which is both the mock-up's reading (6126:
    /// a tab that already has a files pane closes it) and VS Code's `Ctrl+B`.
    /// The two entries share the verb rather than the shape, so the menu row and
    /// the chord can never come to mean two different things.
    ///
    /// **The root is captured here and never again** (A7). It is the focused
    /// terminal's working directory at this instant, falling back to `HOME` when
    /// that shell has never said where it stands. `follow-focus` was ruled out
    /// on 2026-07-16 — a tree that re-roots itself as you `cd` is the single most
    /// startling thing an independent view can do — so this is the one moment the
    /// column ever looks at a terminal's cwd.
    ///
    /// Keyboard focus deliberately does not move, and `focused_leaf` is not
    /// touched: it names a *shell*, and a `focused_leaf` naming a seat with no
    /// session is exactly the crash I106 reports. The column takes *layout*
    /// focus, which is a different thing and is what makes it the last pane to
    /// be given up when the window runs out of room.
    ///
    /// (This paragraph used to say there was no `InputOwner::FilesTree` yet.
    /// There is — [`WindowRuntime::files_focus`] is it, and `files_tree_key` is its
    /// rung of §7.1.5's ladder. What stayed true is the sentence around it:
    /// giving a column the *keyboard* is a separate act from opening one, and
    /// opening one from a chord does not perform it.)
    pub(crate) fn toggle_files_pane(&mut self) -> Result<()> {
        if let Some(open) = self.seats.files_seat() {
            // Closing a files pane is closing a pane: the same verb, so the
            // state teardown, the tab-closes-with-its-last-pane rule and the
            // re-solve are the ones every other pane already gets.
            return self.close_pane(open);
        }
        let root = self.files_root_for_new_pane();
        self.seat_a_files_column(root)?;
        Ok(())
    }

    /// Seat a files column in this tab, rooted where the caller says.
    ///
    /// Split out of [`Self::toggle_files_pane`] rather than written a second
    /// time, because "where does a column arrive and how wide is it" is one
    /// answer and not one per door: the root rim, leading side, and the kind's
    /// own opening width (F62's 240), because a column arriving with no history
    /// has no width to bring.
    ///
    /// The *root* is the only thing a caller gets to say, and that is the whole
    /// difference between the two doors: the chord captures the focused shell's
    /// cwd ([`Self::files_root_for_new_pane`]), and a click on a printed folder
    /// captures the folder it was printed on.
    ///
    /// `None` is the solver refusing — this window has no room for another pane —
    /// and it leaves the tree untouched, so the caller has nothing to undo and
    /// only something to report.
    pub(crate) fn seat_a_files_column(&mut self, root: String) -> Result<Option<SeatId>> {
        let metrics = self.seat_metrics();
        let Some(seat) = self.seats.add_files_pane(&metrics, None) else {
            return Ok(None);
        };
        self.files.insert(
            seat,
            seats::FilesLeafState {
                root,
                ..seats::FilesLeafState::default()
            },
        );
        debug_assert!(
            self.files_match_files_seats(),
            "A3: opening a files pane files a state under the seat it minted"
        );
        // The tree changed, so everything downstream of its shape follows —
        // through the one door every seat-set change goes through.
        self.settle_seat_set_change()?;
        Ok(Some(seat))
    }

    /// **Show this folder in the files column** (§7.1.5g, user ruling
    /// 2026-08-21) — the plain half of the folder row.
    ///
    /// Two sentences, and each is a verb this window already has. A tab that has
    /// a column **re-roots it** ([`Self::reroot_files_column`]), which is the
    /// same door the root menu's rows, `Browse…` and a folder dropped on a column
    /// all commit; a tab with no column **gets one**
    /// ([`Self::seat_a_files_column`]), arriving where and as wide as
    /// `Ctrl+Shift+B` puts one. Neither is re-implemented here, so "clicking a
    /// printed folder is the same as pointing the column at it" is true rather
    /// than nearly true — the expansion set is cleared, the width is kept, and
    /// the read caches keyed on the old root are dropped without any of that
    /// being remembered a second time.
    ///
    /// **Re-root rather than add a second column**, and the reason is the one
    /// [`Seats::files_seat`] has always encoded: the chord toggles *the* column,
    /// so a window that answered a click by stacking a second one would hand the
    /// user a tab whose `Ctrl+Shift+B` closes a column they were not looking at.
    /// A tab can still hold two — a restored layout does, and so does a split —
    /// and the one that answers is the one first in seat order, which is the same
    /// one every other door in this file means by "the files column".
    ///
    /// **The keyboard does not move**, for [`Self::toggle_files_pane`]'s own
    /// reason: giving a column the keyboard is a separate act from opening one,
    /// and a click that was reading a printed line has not asked to stop reading
    /// it.
    ///
    /// Both roads and the refusal write a `BT_MOUSE_TRACE` line, for
    /// [`Self::open_preview_file`]'s reason: the whole of what this verb does is
    /// somewhere else on the screen, so "the click did nothing" and "the click
    /// re-rooted the column you were not looking at" are the same picture from
    /// the outside.
    /// **Stand the files column *on* this folder, moving the tree as little as
    /// it can** (user ruling 2026-08-25) — what every "locate" in this window
    /// means.
    ///
    /// The soft half of [`Self::show_folder_in_files_column`], and the ruling is
    /// about the difference. That verb re-roots, always, which is right for the
    /// door it was written for — a folder printed in a shell's output is very
    /// often somewhere else entirely — and wrong for every door that has since
    /// started calling it: `Show in files column`, a press on a breadcrumb
    /// segment, and now a row of the `…` chip's list. The report was a screenshot
    /// of a column that had been showing a whole repository and was suddenly
    /// showing one folder of it, with no way back but the root menu:
    /// 「打开文件不许重根文件树」.
    ///
    /// **The rule is a question about range, not about which door asked.** If
    /// the folder is *inside the tree the column is already rooted at* — the root
    /// itself included — then it is reachable, so the column keeps its root and
    /// **opens the way down to it, scrolls it into view, and selects it**. Only a
    /// folder outside that tree re-roots, because for that folder there is
    /// nothing to open the way down *to*.
    ///
    /// **Selecting rather than merely scrolling**, because the tree already has
    /// exactly one way of saying "this row is the one you asked about" and it is
    /// the selection every other arrival uses ([`Self::reveal_files_row`] is the
    /// scroll half of the keyboard's own move). A second kind of highlight would
    /// be a second thing for the reader to learn.
    ///
    /// **A column that has not read those folders yet** answers the same way it
    /// answers a keyboard walking into them: the expansions are recorded and the
    /// reads are asked for, and the row is revealed when the disk comes back —
    /// see [`WindowRuntime::files_locate`].
    pub(crate) fn locate_folder_in_files_column(&mut self, folder: &Path) -> Result<()> {
        let existing = self.seats.files_seat();
        self.mouse_trace(|| {
            format!(
                "locate_folder_in_files_column enter path={} column={existing:?}",
                folder.display()
            )
        });
        let Some(seat) = existing else {
            // No column at all: there is no tree to move gently, so the only
            // thing "locate" can mean is the one `show_folder_in_files_column`
            // already means — open one, rooted here.
            self.mouse_trace(|| "locate_folder_in_files_column leave=no-column".to_owned());
            return self.show_folder_in_files_column(folder);
        };
        let root = self.window.tabs[self.window.active_tab]
            .files_state(seat)
            .root;
        let Some(key) = files_key_within(&root, folder) else {
            self.mouse_trace(|| "locate_folder_in_files_column leave=outside-the-tree".to_owned());
            return self.show_folder_in_files_column(folder);
        };
        self.mouse_trace(|| format!("locate_folder_in_files_column leave=inside key={key}"));
        self.open_files_path_to(seat, &key)
    }

    /// Open every folder between a column's root and one node of it, select that
    /// node, and bring it into view.
    ///
    /// The three steps [`Self::locate_folder_in_files_column`]'s inside arm is,
    /// written once because the pending half calls it again when the disk lands.
    ///
    /// **Every folder on the way down is opened and re-asked**, the target
    /// included — which is exactly what a press on each of their triangles would
    /// have done, and it is the target's own press that makes this the same verb
    /// a press on a *visible* breadcrumb segment is. The re-ask is not
    /// bookkeeping: unfolding *is* this product's refresh gesture (Q3 is
    /// deferred), so a locate that recorded the expansions without asking would
    /// show a folder's children as of whenever it was last looked at.
    ///
    /// The toggle is deliberately **not** [`press_files_node`]: that one is a
    /// toggle, and half of these folders are already open — running it would
    /// shut them on the way past.
    ///
    /// **And the triangles do not turn, they arrive turned**
    /// ([`files::DirCache::settle_row`]). A turn is a picture of your hand doing
    /// something, and this is not your hand on these triangles: it is a run of
    /// folders being put into a state so that one row below them can be shown
    /// to you. Found on a real window (2026-08-25): an easing tween here left
    /// `.android` drawn with a *shut* triangle over its own open children,
    /// because nothing woke the loop again for a turn nobody had clicked.
    ///
    /// The intent is filed **before** the reveal rather than after: the row is
    /// very often not in the tree yet — its parent's listing is a worker answer
    /// away — and a reveal that found nothing would otherwise be the whole of
    /// what this verb did.
    fn open_files_path_to(&mut self, seat: SeatId, key: &str) -> Result<()> {
        let now = Instant::now();
        let active = self.window.active_tab;
        // Root first, so a tree that has read nothing asks for its folders in
        // the order it will draw them.
        for ancestor in files_open_chain(key) {
            let Some(state) = self.window.tabs[active].files.get_mut(&seat) else {
                return Ok(());
            };
            state.open.insert(ancestor.clone());
            self.window.tabs[active]
                .file_trees
                .entry(seat)
                .or_default()
                .settle_row(&ancestor);
            self.refresh_files_dir(seat, &ancestor);
        }
        // **The root has no row of its own**, and asking for one is what a tree
        // rooted here already answers: the whole list is that folder's contents.
        // So the column's own root scrolls nothing and selects nothing — it is
        // already the thing you asked to see — and filing an intent for a row
        // that will never exist would leave one owed for the rest of the
        // session.
        if !key.is_empty() {
            if let Some(state) = self.window.tabs[active].files.get_mut(&seat) {
                state.sel = Some(key.to_owned());
            }
            self.window.files_locate.insert(seat, key.to_owned());
            self.settle_files_locate();
        }
        self.mark_session_dirty(now);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Spend a filed locate as soon as the row it names is in the tree.
    ///
    /// [`Runtime::settle_preview_goto`]'s shape and its reason: the intent is
    /// recorded where it is formed and spent where there is something to measure,
    /// which is this frame when the directories were already read and a later one
    /// when they were not. Called from the locate itself and from the worker's
    /// own door, so no frame goes by with a row on screen and a reveal owed to
    /// it.
    ///
    /// **Spent on the first frame the row exists**, and dropped with it: a locate
    /// that has scrolled is finished, and one whose folder turned out not to be
    /// there is dropped by the same reader that drops a dead selection
    /// ([`Self::heal_files_selection`]), because it names the same key.
    fn settle_files_locate(&mut self) {
        let owed: Vec<(SeatId, String)> = self
            .window
            .files_locate
            .iter()
            .map(|(seat, key)| (*seat, key.clone()))
            .collect();
        if owed.is_empty() {
            return;
        }
        // One walk for every column that is owed one, on `files_tree_walk`'s own
        // cost argument: deriving a tree's rows is not free, and this is asked
        // once per worker answer.
        let trees = self.files_tree_contents();
        for (seat, key) in owed {
            let has_row = trees
                .get(&seat)
                .is_some_and(|tree| tree.rows.iter().any(|row| row.key == key));
            if !has_row {
                continue;
            }
            self.reveal_files_row(seat, &key);
            self.window.files_locate.remove(&seat);
        }
    }

    fn show_folder_in_files_column(&mut self, folder: &Path) -> Result<()> {
        let existing = self.seats.files_seat();
        self.mouse_trace(|| {
            format!(
                "show_folder_in_files_column enter path={} column={existing:?}",
                folder.display()
            )
        });
        let root = folder.display().to_string();
        match existing {
            Some(seat) => {
                self.reroot_files_column(seat, &root)?;
                self.mouse_trace(|| format!("show_folder_in_files_column leave=rerooted {seat:?}"));
            }
            None => match self.seat_a_files_column(root.clone())? {
                Some(seat) => {
                    // The other door of the two, and the same gesture behind it:
                    // a reader pointed at a folder and got it. The re-rooting
                    // half is recorded inside `reroot_files_column`; this half
                    // has to say so itself, because `seat_a_files_column`'s
                    // other caller is the chord, and the chord's folder is
                    // whichever one a shell happens to have `cd`'d into.
                    self.note_folder_opened(&root);
                    self.mouse_trace(|| {
                        format!("show_folder_in_files_column leave=seated {seat:?}")
                    });
                }
                None => self.mouse_trace(|| {
                    "show_folder_in_files_column leave=no-room-for-a-pane".to_owned()
                }),
            },
        }
        Ok(())
    }

    /// Where a files pane opened right now would be rooted (H115).
    ///
    /// The focused shell's own folder, because that is the place the user is
    /// standing when they ask for the tree; `HOME` when it has never named one,
    /// which is the same answer `cwd_for_spawn` gives a new tab in the same
    /// situation. Read through `sessions` by id rather than through the `Deref`,
    /// so this is answerable on a tab whose keyboard is somewhere unexpected.
    fn files_root_for_new_pane(&self) -> String {
        self.sessions
            .get(&self.focused_leaf)
            .and_then(|leaf| leaf.session.working_directory())
            .map(|cwd| cwd.display().to_string())
            .or_else(|| {
                profiles::home_directory(&bt_pty::SystemShellEnvironment)
                    .map(|home| home.display().to_string())
            })
            .unwrap_or_default()
    }

    pub(crate) fn disable_files_worker(&mut self) -> bool {
        files::disable_files_worker_state(
            &mut self.app.files_worker_running,
            &mut self.app.files_worker_notice_pending,
        )
    }

    /// Every files column of the active tab, walked into the rows it draws this
    /// frame, with whatever the walk could not answer put to the worker.
    ///
    /// **Why the active tab only.** Laziness is the whole design (§7.1.3): a
    /// column reads the directories it is showing and no others. A column in a
    /// tab nobody is looking at is showing none, so it asks for none — and asks
    /// the moment the tab is switched to, which is the same frame it first has a
    /// rectangle to draw into.
    ///
    /// **Why asking happens here.** The walk is the only thing that knows what
    /// is missing, because "missing" means *visibly* missing: an unopened folder
    /// is not missing, it is folded. Deriving the request list anywhere else
    /// would mean a second walk that could disagree with the one on screen.
    pub(crate) fn files_trees(
        &mut self,
        now: Instant,
    ) -> BTreeMap<SeatId, seats::FilesTreeContent> {
        let active = self.window.active_tab;
        let tab_id = self.window.tabs[active].id;
        let motion = self.app.motion;
        // The *ring's* seat, not the keyboard's: `:focus-visible`, so a column
        // that took the keyboard from a press shows its selection without one.
        let ringed = self.files_ring_seat();
        let walked = self.files_tree_walk(self.files_edit_place());
        // **R32's one input, gathered once for the whole tab**: which page each
        // column is standing on, and what it knows about the repository under it.
        // A column with no cache is not in it and cannot be — there is no entry
        // until a Git page has asked something, which is the gate saying "no
        // extra read" in the only way that cannot be forgotten. Nothing here asks
        // git anything; every answer in it was paid for by an open page.
        let pages = self.window.tabs[active].git_badge_sources();
        let badges: BTreeMap<SeatId, git_panel::GitTreeBadges> = walked
            .keys()
            .map(|seat| {
                let root = self.window.tabs[active]
                    .files
                    .get(seat)
                    .map(|state| state.root.clone())
                    .unwrap_or_default();
                (
                    *seat,
                    git_panel::GitTreeBadges::of(&pages, std::path::Path::new(&root)),
                )
            })
            .collect();
        let mut views = BTreeMap::new();
        let window = self.window_id();
        let mut asks = Vec::new();
        for (seat, (mut content, wanted)) in walked {
            content.focus_ring = Some(seat) == ringed;
            content.badges = badges.get(&seat).cloned().unwrap_or_default();
            let root = self.window.tabs[active]
                .files
                .get(&seat)
                .map(|state| state.root.clone())
                .unwrap_or_default();
            let cache = self.window.tabs[active].file_trees.entry(seat).or_default();
            for key in wanted {
                // Marked before the send so that the next walk — which may
                // happen on this very frame, from the hit test — sees a question
                // already asked rather than asking it a second time.
                cache.mark_pending(&key);
                asks.push(files::DirRequest {
                    window,
                    host: files::FilesHost::Docked(LeafId { tab: tab_id, seat }),
                    path: files::full_path(&root, &key),
                    key,
                });
            }
            // C33, sampled on this frame's own `now` beside the chevron's turn.
            content.turns = content
                .rows
                .iter()
                .filter_map(|row| match row.kind {
                    files::RowKind::Directory { open } => Some((
                        row.key.clone(),
                        cache.row_turn(&row.key, open, now, motion).0,
                    )),
                    _ => None,
                })
                .collect();
            views.insert(seat, content);
        }
        for ask in asks {
            if !self.app.files_worker.request(ask) {
                self.disable_files_worker();
                break;
            }
        }
        // The Git page's questions ride the same walk, for the same reason the
        // directory ones do: this is the only place that knows which columns are
        // on screen and where each of them is rooted.
        //
        // **The gate is R31's, and it is two conditions and not one.** A
        // repository is read when the master switch is on *and* a column is
        // actually showing its Git page — never because a folder happens to be
        // open, never on a timer, and never for a page nobody switched to. A
        // column on its tree costs exactly what it cost before this slice: no
        // process at all.
        let on_screen: Vec<(SeatId, seats::FilesView, String)> = self.window.tabs[active]
            .files
            .iter()
            .filter(|(seat, _)| views.contains_key(seat))
            .map(|(seat, state)| (*seat, state.view, state.root.clone()))
            .collect();
        for (seat, root) in columns_wanting_git(self.git_panel_on(), &on_screen) {
            self.ask_git_for_column(seat, &root);
        }
        views
    }

    /// Turn a column to one of its two pages.
    ///
    /// **Idempotent on purpose**: pressing the half you are already on is a
    /// press with nothing to do, and answering it with a repaint would make a
    /// segmented control flicker under a double click. It also does nothing at
    /// all while the master switch is off — not because this checks it, but
    /// because nothing can reach here: there is no strip to press and the chord
    /// asks the same question first.
    pub(crate) fn set_files_view(&mut self, seat: SeatId, view: seats::FilesView) -> Result<()> {
        let active = self.window.active_tab;
        let Some(state) = self.window.tabs[active].files.get_mut(&seat) else {
            return Ok(());
        };
        if state.view == view {
            return Ok(());
        }
        state.view = view;
        // The page is durable (R1), so turning it is a change to the session —
        // and the save is debounced exactly as every other layout change is.
        self.mark_session_dirty(Instant::now());
        // **And the picture is chrome**, so it is asked for the way every other
        // chrome-changing verb in this file asks: `refresh_chrome` rebuilds the
        // seat layer and `present_chrome_change` puts it on screen.
        // `publish_frame` is the *content* path — it re-composes what the shells
        // have printed and never touches the chrome build — so a page turned
        // through it changed the state and left the old page on screen. Caught on
        // the real machine, where the chord set the view and drew nothing (the
        // mouse worked all along, because the press dispatcher refreshes the
        // chrome on its way out).
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// One key, with a files column holding the keyboard — **answered by the
    /// page that column is showing** (焦点跟随可见视图, 2026-08-19).
    ///
    /// # The bug this exists to close
    ///
    /// D47 lends the keyboard to a *column*, and the router under it asked one
    /// question — is this seat a files column — and then handed every key to the
    /// **tree**. A column standing on its Git page therefore answered its arrows
    /// with a list nobody could see: the tree's selection and its scroll moved
    /// in the dark, and `Enter` opened a preview of a file the reader had never
    /// chosen and could not have pointed at. Nothing reached a shell, which is
    /// why it was silent; state changed under the user, which is why it was a
    /// bug.
    ///
    /// The judgement is made **here**, once, at the routing decision — not as a
    /// condition repeated on each key — and it is [`column_keyboard`]'s, which
    /// is answerable without a window.
    pub(crate) fn files_column_key(&mut self, seat: SeatId, event: &KeyEvent) -> Result<bool> {
        match column_keyboard(seat, &self.window.git_pages_shown) {
            ColumnKeyboard::GitPage => self.git_page_key(seat, event),
            ColumnKeyboard::Tree => self.files_tree_key(seat, event),
        }
    }

    /// Which page each docked Files column is on, and how wide its switch's two
    /// words are drawn.
    ///
    /// **Empty when the master switch is off**, and that emptiness is the whole
    /// mechanism: the geometry, the painter and the hit test all key off a
    /// column's presence here, so there is exactly one place the switch is
    /// consulted and no way for one of them to miss it.
    pub(crate) fn files_views(&mut self, scale: f32) -> BTreeMap<SeatId, seats::FilesViewContent> {
        if !self.git_panel_on() {
            return BTreeMap::new();
        }
        let font = seats::FILES_SEG_FONT_LOGICAL_PX * scale;
        // Two strings, measured once per frame rather than once per column: they
        // are the same two words in every column in the window, and the only
        // thing that could make them differ is the font, which is one font.
        let widths = seats::FilesView::ALL.map(|view| {
            self.window
                .renderer
                .measure_chrome_text(&mut self.app.gpu, view.label(), font)
        });
        let active = self.window.active_tab;
        self.seats
            .files()
            .into_iter()
            .map(|seat| {
                let view = self.window.tabs[active]
                    .files
                    .get(&seat)
                    .map_or(seats::FilesView::Files, |state| state.view);
                (seat, seats::FilesViewContent { view, widths })
            })
            .collect()
    }

    /// **Where the open name editor stands in a files column**, or nothing when
    /// none is open in one (0.3).
    ///
    /// The one reader of [`RenameSubject`] the tree's walk needs, asked here
    /// because this is where the window's open editor and the tab's rows meet.
    /// A box on a tab, a preview head, a breadcrumb or an address answers `None`
    /// — those are not rows of any tree.
    fn files_edit_place(&self) -> Option<FilesEditPlace<'_>> {
        let active = self.window.tabs.get(self.window.active_tab)?.id;
        match self.window.rename.as_ref()?.subject {
            RenameSubject::FilesRow { leaf, ref key } if leaf.tab == active => {
                Some(FilesEditPlace {
                    seat: leaf.seat,
                    at: FilesEditRow::Existing(key),
                })
            }
            RenameSubject::FilesNew {
                leaf,
                ref parent,
                folder,
                ..
            } if leaf.tab == active => Some(FilesEditPlace {
                seat: leaf.seat,
                at: FilesEditRow::New { parent, folder },
            }),
            _ => None,
        }
    }

    /// The rows as the hit test sees them: walked, never asked for.
    pub(crate) fn files_tree_contents(&self) -> BTreeMap<SeatId, seats::FilesTreeContent> {
        let ringed = self.files_ring_seat();
        self.files_tree_walk(self.files_edit_place())
            .into_iter()
            .map(|(seat, (mut content, _))| {
                content.focus_ring = Some(seat) == ringed;
                (seat, content)
            })
            .collect()
    }

    /// Fill in each column's `.files-foot` — what it says, and whether it is
    /// saying it in the accent.
    ///
    /// **The cut lives here** because the strip's width and the font are both
    /// here, and it is `settings::ellipsized_left` — the same cut the float's
    /// foot takes, from the *front*, so `…ers\Alice\Developer\folio-terminal`
    /// keeps the segment that answers "where am I" (B23). The mock-up reaches it
    /// with `direction: rtl` and a `<bdi>` around the path; we draw our own text,
    /// so we simply cut the string and lay it out left to right, and the bidi
    /// reordering bug that workaround exists for cannot occur.
    pub(crate) fn dress_files_feet(
        &mut self,
        scale: f32,
        now: Instant,
        trees: &mut BTreeMap<SeatId, seats::FilesTreeContent>,
    ) {
        let font = seats::FILES_FOOT_FONT_LOGICAL_PX * scale;
        let segmented = self.git_panel_on();
        let active = self.window.active_tab;
        for (seat, tree) in trees.iter_mut() {
            let revealed = self.window.revealed_foot.is_some_and(|(shown, at)| {
                shown == RevealedFoot::Column(*seat)
                    && now.saturating_duration_since(at) < FOOT_REVEAL_FEEDBACK
            });
            let root = self.window.tabs[active].files_state(*seat).root;
            // An unrooted column has no path and nothing to reveal, so its strip
            // is a hairline and nothing else — the mock-up's foot with an empty
            // `${files.root}` in it.
            if root.is_empty() {
                tree.foot_path = String::new();
                continue;
            }
            let Some(rect) = seats::files_pane_rect(&self.seat_layout, *seat) else {
                tree.foot_path = String::new();
                continue;
            };
            let path_box = seats::files_pane_geometry(rect, scale, segmented).foot_path;
            let room = path_box[2] - path_box[0];
            // **The strip trades one phrase for the other over ninety
            // milliseconds** (the animation slice's second half). The receipt's
            // 1300ms hold is untouched — `revealed` above is the same clock it
            // always was; what this decides is only which of the two phrases is
            // on the glass while they are changing places.
            let (said, dissolved) = self.foot_saying(
                FootSaying::Column(*seat),
                revealed.then(foot_revealed_label),
                now,
            );
            tree.foot_revealed = said.is_some();
            tree.foot_dissolved = dissolved;
            // A receipt is a sentence, not a path; only the root is spelled the
            // way the rail above it spells one (§13.40).
            let text = match said.as_deref() {
                Some(said) => std::borrow::Cow::Borrowed(said),
                None => home_shortened_path(&root),
            };
            let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
            let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
            tree.foot_path = settings::ellipsized_left(&text, room, font, &mut measure);
        }
    }

    /// Ask for a folder again because it has just been opened.
    ///
    /// Unfolding a folder is a refresh as well as a disclosure, and it stays one
    /// now that [`files_watch`] exists: the kernel stops speaking about a folder
    /// the moment its handle is dropped, so what happened while it was folded
    /// reached nobody, and the unfold is the only thing that can cover it. The
    /// cache is kept across a fold so that re-opening is instant, and re-asking
    /// is what keeps it from being instant *and wrong*. The kept rows stay on
    /// screen until the new answer lands, so a refresh never blinks.
    fn refresh_files_dir(&mut self, seat: SeatId, key: &str) {
        let tab = self.window.tabs[self.window.active_tab].id;
        self.ask_files_dir(files::FilesHost::Docked(LeafId { tab, seat }), key);
    }

    /// Ask for one folder of one file tree again, wherever that tree lives.
    ///
    /// [`Self::refresh_files_dir`]'s body, addressed the way the worker's own
    /// traffic is: a docked column is a [`LeafId`] and a float is an epoch, and
    /// the watcher speaks to both because both draw folders that can go out of
    /// date. A tree that has gone while the ask was being assembled resolves to
    /// no root and asks nothing, which is the same cancellation a landed answer
    /// with nowhere to go already is.
    ///
    /// **Nothing is marked pending here.** A refresh is not a first reading: the
    /// rows already on screen are the truth until a better one lands, and a
    /// `Pending` node would replace a listed folder with "Loading …" for as long
    /// as the disk took.
    fn ask_files_dir(&mut self, host: files::FilesHost, key: &str) {
        let root = match host {
            files::FilesHost::Docked(leaf) => self
                .window
                .tabs
                .iter()
                .find(|tab| tab.id == leaf.tab)
                .and_then(|tab| tab.files.get(&leaf.seat))
                .map(|state| state.root.clone()),
            files::FilesHost::Float(epoch) => self
                .window
                .float
                .live(epoch)
                .and_then(float::FloatWin::files)
                .map(|files| files.files.root.clone()),
        };
        let Some(root) = root else {
            return;
        };
        let request = files::DirRequest {
            window: self.window_id(),
            host,
            key: key.to_owned(),
            path: files::full_path(&root, key),
        };
        if !self.app.files_worker.request(request) {
            self.disable_files_worker();
        }
    }

    /// **Bring the file trees' subscriptions level and answer whatever the
    /// kernel has said** (`files_watch`, user report 2026-08-27).
    ///
    /// [`Self::advance_preview_watch`]'s shape one subject over, and the two
    /// halves are one call for that function's reason: the set a window is
    /// watching changes on the same events that produce news about it, and
    /// syncing after answering would act on a folder no column is showing any
    /// more.
    pub(crate) fn advance_files_watch(&mut self, now: Instant) -> Result<()> {
        let showing = self.watched_files_dirs();
        let wanted: BTreeSet<PathBuf> = showing.keys().cloned().collect();
        let armed = self.window.files_watch.sync(&wanted, &self.app.event_proxy);
        let due = self.window.files_watch.due(now);
        if armed.is_empty() && due.is_empty() {
            return Ok(());
        }
        let mut asks: Vec<(files::FilesHost, String)> = Vec::new();
        // What moved: every tree showing that folder, which is not one tree —
        // two columns and a float can be looking at one directory, and all of
        // them are about to be out of date together.
        for directory in &due {
            let Some(trees) = showing.get(directory) else {
                continue;
            };
            asks.extend(trees.iter().cloned());
        }
        // And `files_watch`'s rule 3: a folder whose watch has only just opened
        // was not being listened to a moment ago, so an answer this tree already
        // holds for it was taken while nobody was watching. A folder nothing has
        // asked about yet is left alone — the walk that put it on screen is
        // about to ask for it for the first time, and that read is on the far
        // side of this arming.
        for directory in &armed {
            let Some(trees) = showing.get(directory) else {
                continue;
            };
            asks.extend(
                trees
                    .iter()
                    .filter(|(host, key)| self.files_dir_answered(*host, key))
                    .cloned(),
            );
        }
        for (host, key) in asks {
            self.ask_files_dir(host, &key);
        }
        // **And the palette's index of every root those folders are under is
        // now out of date** (DESIGN.md §7.55 ④).
        //
        // Marked from `due` rather than from [`files::DirCache::revision`],
        // which was the other candidate and is the wrong instrument twice
        // over: a revision is bumped by `mark_pending` as well, so it says
        // "this column asked a question" as loudly as it says "the disk
        // moved" — and it is this same news read one layer further downstream,
        // so reading it here would be a second mechanism for one fact.
        //
        // It marks and does not rebuild. A repository whose files move all day
        // would otherwise be walked all day for a box nobody has opened; the
        // walk is asked for by the next `ask_for_file_indexes`, which is to
        // say by the next time somebody actually raises the palette.
        for directory in &due {
            for root in self.palette_files_roots() {
                if directory.starts_with(&root) {
                    self.app.file_indexes.mark_dirty(&root);
                }
            }
        }
        Ok(())
    }

    /// **Every folder this window has the contents of on the glass, and which
    /// trees are showing it.**
    ///
    /// The gate `files_watch` follows, and the whole of it. Two kinds of tree
    /// answer and they answer with the same thing — a folder on a disk: a docked
    /// column of the tab on screen, and a live float.
    ///
    /// **The tab on screen and not every tab**, which is the opposite of
    /// [`Self::watched_preview_files`] and for a reason about the subject. A
    /// preview buffer is *content* that is not re-read when you come back to its
    /// tab, so gating it on visibility would make returning to a tab the one
    /// place this window knowingly shows something it has been told is stale. A
    /// directory listing has the other property: `files_watch` reports the
    /// folders it newly armed, and coming back to a tab arms its folders again
    /// and re-asks for every one of them the column already had an answer for.
    /// So the visible gate here costs nothing in freshness and keeps a window of
    /// twenty tabs from holding a kernel handle and a blocked thread for every
    /// folder any of them ever unfolded.
    ///
    /// The value is a list rather than one address because two columns rooted at
    /// the same place are one subscription and two trees to tell.
    fn watched_files_dirs(&self) -> BTreeMap<PathBuf, Vec<(files::FilesHost, String)>> {
        let mut showing: BTreeMap<PathBuf, Vec<(files::FilesHost, String)>> = BTreeMap::new();
        // **Indexed by `get`**, because this is asked on every turn of the loop
        // including the ones a window takes on its way out: a window whose last
        // tab has exited has no tab on screen and is showing no folders, which
        // is an answer and not a case to guard against.
        if let Some(tab) = self.window.tabs.get(self.window.active_tab) {
            for (seat, state) in &tab.files {
                for key in files::visible_dirs(state) {
                    let host = files::FilesHost::Docked(LeafId {
                        tab: tab.id,
                        seat: *seat,
                    });
                    showing
                        .entry(files::full_path(&state.root, &key))
                        .or_default()
                        .push((host, key));
                }
            }
        }
        for win in self.window.float.live_windows() {
            let Some(tree) = win.files() else {
                continue;
            };
            for key in files::visible_dirs(&tree.files) {
                showing
                    .entry(files::full_path(&tree.files.root, &key))
                    .or_default()
                    .push((files::FilesHost::Float(win.id()), key));
            }
        }
        showing
    }

    /// Whether one tree already holds an answer about one of its folders.
    ///
    /// `files_watch`'s rule 3 asks this and nothing else: *unheard of* is the
    /// one state in which a newly armed folder is not owed a fresh read, because
    /// it is the one state in which a read is coming anyway.
    fn files_dir_answered(&self, host: files::FilesHost, key: &str) -> bool {
        match host {
            files::FilesHost::Docked(leaf) => self
                .window
                .tabs
                .iter()
                .find(|tab| tab.id == leaf.tab)
                .and_then(|tab| tab.file_trees.get(&leaf.seat))
                .is_some_and(|cache| cache.get(key).is_some()),
            files::FilesHost::Float(epoch) => self
                .window
                .float
                .live(epoch)
                .and_then(float::FloatWin::files)
                .is_some_and(|tree| tree.cache.get(key).is_some()),
        }
    }

    /// Take every directory the worker has finished.
    pub(crate) fn apply_files_results(
        &mut self,
        batch: &mut Vec<files::DirResponse>,
        lane_gone: bool,
    ) -> Result<()> {
        let mut changed = lane_gone;
        for response in answers_for(batch, |response| self.owns(response.owner())) {
            match response.host {
                // A tab or a column that closed while its read was in flight
                // has nowhere to put the answer, and that is not a failure —
                // it is the cancellation, arriving as a dropped result.
                files::FilesHost::Docked(leaf) => {
                    let Some(index) = self.window.tabs.iter().position(|tab| tab.id == leaf.tab)
                    else {
                        continue;
                    };
                    // The card's claim on a frame, for `apply_preview_results`'
                    // reason exactly: a listing landing in a background tab
                    // is what turns that tab's card from `Loading…` into its
                    // tree, and only a frame somebody asks for will project
                    // it.
                    let carded = self.window.focus_thumbs.seats(leaf.tab).is_some();
                    let tab = &mut self.window.tabs[index];
                    if !tab.files.contains_key(&leaf.seat) {
                        continue;
                    }
                    // **And whether it said anything new.** Every reading that
                    // reaches here used to be a change by definition, because
                    // the only ones there were had been asked for by a walk that
                    // had nothing. A watcher makes re-reads ordinary — a folder
                    // speaks for every write to every file in it — so a listing
                    // identical to the one already held must not heal
                    // selections, settle a locate or ask for a frame.
                    let told = tab
                        .file_trees
                        .entry(leaf.seat)
                        .or_default()
                        .accept(&response.key, response.outcome);
                    changed |= told && (index == self.window.active_tab || carded);
                }
                // The float's version of the same cancellation: the window is
                // gone, or it is showing a *different* view than the one that
                // asked. The epoch is what tells those apart — without it a
                // peek replaced by another trigger's would be handed the old
                // root's directories under keys that now mean somewhere else.
                //
                // With several floats on screen (2026-08-12) it is also what
                // *addresses* the answer: `live_mut` searches every window for
                // the one that asked, so two trees never fill in with each
                // other's contents.
                files::FilesHost::Float(epoch) => {
                    let Some(files) = self
                        .window
                        .float
                        .live_mut(epoch)
                        .and_then(float::FloatWin::files_mut)
                    else {
                        continue;
                    };
                    changed |= files.cache.accept(&response.key, response.outcome);
                }
            }
        }
        if changed {
            self.heal_files_selections();
            // A directory that has just landed is very often the one a locate
            // was waiting on — see [`WindowRuntime::files_locate`]. Asked before
            // the repaint, so the frame that first draws the row is the frame it
            // is already scrolled into.
            self.settle_files_locate();
            if self.refresh_chrome() {
                self.present_chrome_change()?;
            }
        }
        Ok(())
    }

    /// A press on one row of one files column (C155).
    ///
    /// **The rows are re-derived here rather than remembered from the frame the
    /// pointer was over.** An index into a list is only meaningful beside the
    /// list it indexes, and between the paint and the press a directory may have
    /// landed and made the tree longer. Re-walking costs a walk over the visible
    /// rows and removes the whole class of bug where a click lands on the row
    /// above the one that was clicked.
    ///
    /// Both kinds of node take the selection, which is what keeps the mouse and
    /// the keyboard telling one story about where you are; only a directory also
    /// opens or shuts.
    pub(crate) fn press_files_row(&mut self, seat: SeatId, index: usize) -> Result<()> {
        use crate::files::RowKind;
        let now = Instant::now();
        let motion = self.app.motion;
        let trees = self.files_trees(now);
        let Some(row) = trees.get(&seat).and_then(|tree| tree.rows.get(index)) else {
            self.window.files_row_clicks.interrupt();
            return Ok(());
        };
        if !row.is_node() {
            self.window.files_row_clicks.interrupt();
            return Ok(());
        }
        let key = row.key.clone();
        let kind = row.kind;
        // K156, and the 2026-08-19 ruling beside it: the second press on a row
        // is a verb, and which verb is the row's own. A file row opens in the
        // preview; a **folder row becomes this column's root**. The mock-up's
        // `dblclick` handler returned at once for `dir === "1"` (7847) and that
        // is the sentence the ruling overturns — a tree whose only way in was
        // the root menu was a tree you could only walk downwards by unfolding.
        let counted = files_row_counts_clicks(kind);
        let doubled = counted
            && self
                .window
                .files_row_clicks
                .register(RowHost::Column(seat), &key, now)
                == TabClick::Double;
        if !counted {
            self.window.files_row_clicks.interrupt();
        }
        let active = self.window.active_tab;
        // **The way in is taken before the fold, and instead of it.** The fold
        // is what the *first* press meant and it has already happened; running
        // it again here would turn a triangle on a tree that is about to be
        // thrown away, and owe the animation to a key the new root has never
        // heard of. The door is [`Self::reroot_files_column`] — the root menu's
        // own and the folder drop's own, so the expansion reset, the two caches
        // and the session write all come along without being remembered again.
        if doubled
            && let Some(entered) =
                files_row_entry(&self.window.tabs[active].files_state(seat).root, &key, kind)
        {
            return self.reroot_files_column(seat, &entered);
        }
        let activating = doubled && matches!(kind, RowKind::File);
        let Some(state) = self.window.tabs[active].files.get_mut(&seat) else {
            return Ok(());
        };
        let opening = press_files_node(state, &key, kind);
        if matches!(kind, RowKind::Directory { .. }) {
            // The triangle starts turning from wherever it actually is, so a
            // double-click reverses mid-flight instead of snapping.
            self.window.tabs[active]
                .file_trees
                .entry(seat)
                .or_default()
                .turn_row(&key, opening, now, motion);
        }
        if opening {
            self.refresh_files_dir(seat, &key);
        }
        if activating {
            self.activate_files_row(seat, &key)?;
        }
        self.mark_session_dirty(now);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// How wide each files head's name is drawn (B15).
    ///
    /// Measured with the renderer's own font at the head's own size, because a
    /// button that hugs its label has to be told how long the label is and this
    /// is the only place in the window that can answer. The same numbers reach
    /// the painter and [`seats::hit_files_root`], so the two cannot disagree
    /// about where the button ends.
    ///
    /// **In the face the caption is drawn in** (user report, 2026-08-25), which
    /// is [`seats::seat_title_face`]'s whole subject: the head that holds the
    /// keyboard draws its caption `Medium`, and this used to ask
    /// `measure_chrome_text` — the face's regular weight by definition — for
    /// every head alike. The shortfall is about 2%, it lands on the last glyph,
    /// and the report was a column headed `BetterTermina|`. So the focus is asked
    /// per seat rather than once: it is the same question the paint asks
    /// (`placement.id == seats.focus()`), and the two must give one answer.
    pub(crate) fn measure_files_names(
        &mut self,
        names: &BTreeMap<SeatId, String>,
    ) -> BTreeMap<SeatId, f32> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let focus = self.seats.focus();
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let mut measure = |text: &str, size: f32, face: git_panel::MeasureFace| {
            renderer.measure_chrome_label(
                gpu,
                text,
                size,
                face.weight,
                face.letter_spacing_em,
                face.tabular_numerals,
            )
        };
        names
            .iter()
            .map(|(seat, name)| {
                (
                    *seat,
                    seats::files_root_name_width(name, scale, *seat == focus, &mut measure),
                )
            })
            .collect()
    }

    /// Point a column somewhere else (E56).
    ///
    /// **The expansion set and the selection go; the width stays.** They are
    /// facts about a *place*, and this is a different place: a `/src` left open
    /// under the old root is not the `/src` of the new one, and honouring it
    /// would unfold whatever happened to be called that. The width is not about
    /// the place at all — it is how much room you gave this column on your
    /// screen — so it is the one thing that survives, which is also why it lives
    /// on the seat rather than in the state being cleared here.
    ///
    /// The cache goes too, and it has to: it is keyed by root-relative id, so
    /// every key in it would silently mean somewhere else the moment the root
    /// moved. Dropping it is what turns the next walk into a fresh set of
    /// questions.
    pub(crate) fn reroot_files_column(&mut self, seat: SeatId, root: &str) -> Result<()> {
        let active = self.window.active_tab;
        let Some(state) = self.window.tabs[active].files.get_mut(&seat) else {
            return Ok(());
        };
        let moved = reroot_files_state(state, root);
        // **Before the early return, because choosing is the event** (user
        // ruling 2026-09-05). Picking the folder the column is already rooted at
        // moves nothing, and it is still a reader saying "this one" — the list
        // is a list of gestures, so it hears that one and re-dates the row.
        self.note_folder_opened(root);
        if !moved {
            return Ok(());
        }
        self.window.tabs[active].file_trees.remove(&seat);
        // A column pointed somewhere else is a column that may be in another
        // repository — or in none. Everything it knew was about the old root.
        self.window.tabs[active].git_trees.remove(&seat);
        self.mark_session_dirty(Instant::now());
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **A reader pointed a files column at a folder** (§7.5, user ruling
    /// 2026-09-05) — the one door into [`recent_folders::RecentFolders`].
    ///
    /// One door and a handful of callers, rather than one caller derived from
    /// the column's root changing, because the difference this list is *about*
    /// is invisible from there: a column rooted somewhere new because a hand
    /// chose it and a column rooted somewhere new because a chord read a shell's
    /// folder are the same assignment and two different events. Deriving would
    /// record both, and the list would fill with wherever a build script last
    /// stepped.
    ///
    /// The write goes through the session's own debounce, so the folder is on
    /// the disk within a second or two of being opened and every window's menu
    /// sees it on the next frame.
    fn note_folder_opened(&mut self, folder: &str) {
        if !self.app.recent_folders.record(folder, SystemTime::now()) {
            return;
        }
        self.mark_session_dirty(Instant::now());
    }

    /// Collect the folder chooser's answer, once the dialog has shut.
    ///
    /// **Two verbs behind one bridge** (2026-08-16). The chooser is a single
    /// posted-message channel to Windows — it has to be, because two nested
    /// modal loops on one thread is not a thing a window survives — so which
    /// verb asked is remembered on this side, in [`WindowRuntime::folder_pick`], and
    /// spent here.
    pub(crate) fn apply_folder_pick_result(&mut self) -> Result<()> {
        let Some(result) = self.window.folder_picker.take_result() else {
            return Ok(());
        };
        let asked = self.window.folder_pick.take();
        let Some((asked, path)) = folder_pick_outcome(asked, result) else {
            return Ok(());
        };
        match asked {
            // A folder was chosen for a column. Re-rooting is the same verb the
            // quick list's rows commit, down to clearing the expansion set and
            // keeping the width (E56) — a folder reached by a different route is
            // not a different kind of destination.
            FolderPick::Reroot(seat) => {
                let path = path.to_string_lossy().into_owned();
                self.reroot_files_column(seat, &path)?;
            }
            // And for a pane: the ordinary split, in the ordinary direction,
            // with the folder as the arriving shell's own. The pane is asked for
            // again because a chooser can stand open for a minute and the shell
            // behind it can exit in that minute.
            // **The row is asked for again**, because a chooser can stand open
            // for a minute and the table can move in that minute.
            FolderPick::ProfileStart(index) => {
                if index < profiles::count() {
                    profiles::set_start_at(index, profiles::StartAt::Fixed(path));
                    self.store_profiles()?;
                }
                return Ok(());
            }
            FolderPick::SplitInto(seat) => {
                if self.sessions.contains_key(&seat) {
                    self.split_seat(
                        seat,
                        self.settings_split_axis(seat),
                        false,
                        SplitSeed::Folder(path),
                    )?;
                }
            }
            // And for the whole window: the `+`'s own verb, told where to stand.
            // The default profile, because that is what "new tab" means
            // everywhere else in this build (`Runtime::new_tab`, `Ctrl+Shift+N`,
            // the `+`'s tooltip) — this row chose a *place*, and choosing a shell
            // as well is what the four rows above it are for.
            FolderPick::NewTabIn => {
                let profile = self.default_profile_id();
                self.new_tab_with_profile(&profile, Some(path))?;
            }
        }
        Ok(())
    }

    /// Keep this folder, or stop keeping it (user ruling 2026-08-19).
    ///
    /// The write is immediate and undebounced, for `PinsStore`'s own reason: the
    /// press is the whole gesture, and a quiet window is a window in which it can
    /// be lost. What follows is a frame — the root menu's PINNED section is
    /// derived from the store every time the menu is built, never held.
    pub(crate) fn toggle_folder_pin(&mut self, path: &str) -> Result<()> {
        self.app
            .pins_store
            .toggle(bt_persist::PinKind::Folder, path);
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The tree row under a point, in either host, resolved to everything the
    /// menu about it needs — or, since the user ruling of 2026-09-10, **the
    /// column's own ground**, and nothing when the point is neither.
    ///
    /// The float is asked first because the float is drawn over the columns, so
    /// a row of a peek standing across a docked column is the row the pointer is
    /// really on. Both hosts re-derive their rows from the state rather than
    /// remembering the painted list, on [`Runtime::press_files_row`]'s reasoning.
    ///
    /// **The ground is asked last**, which is the same smallest-target-first
    /// order the whole hit-test chain reads in: a row is a rectangle inside the
    /// body, and a fallback that answered before it would put the folder's menu
    /// on every file in the tree.
    ///
    /// **A float's claim is terminal** (adversarial review 2026-09-11, row D1),
    /// which is [`Self::popover_trigger_at`]'s rule said about this door. A
    /// float is opaque: when the topmost window answers the point at all, the
    /// answer is that window's own applicable target or nothing, and never the
    /// docked chrome behind it. It used to be terminal for one part only —
    /// [`float::FloatPart::Row`] — and every other part fell through to the
    /// docked chrome. So a right press on a preview float's text, or on the
    /// empty space below a floating tree's last row, raised the menu of the
    /// *covered* row: a face of verbs with no file name on it, drawn on top of
    /// the float, with `Delete` among them. The float is deliberately left
    /// standing when this menu opens ([`Self::open_file_menu`]'s
    /// `close_popups_except`), so there was nothing on screen that could have
    /// said which file it meant.
    ///
    /// **And the rule is no longer this door's** (user report 2026-09-12). It
    /// was written here, at one caller, and so it held for the press and not for
    /// the hover — which asks the same question through
    /// [`Self::chrome_target_at`] and never came past this function at all. It
    /// now lives in [`Self::pointer_target_at`], the one router both gestures
    /// go through; what is left here is the part of the answer only a menu
    /// knows, which is which float parts have a file behind them.
    pub(crate) fn file_row_under(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Option<FileMenuTarget> {
        let (seat, index) = match self.pointer_target_at(position) {
            // A row of a floating tree raises that window's own menu, about the
            // file that window is showing.
            Some(PointerTarget::Float(id, float::FloatPart::Row(index))) => {
                let files = self.window.float.live(id)?.files()?;
                let rows = files::tree_view(&files.files, &files.cache).rows;
                let row = rows.get(index)?;
                let subject = files_row_menu_subject(row.kind)?;
                return Some(FileMenuTarget {
                    row: Some(FileMenuTreeRow {
                        host: RowHost::Float(id),
                        key: row.key.clone(),
                    }),
                    activation: files_row_activation(&files.files.root, &row.key),
                    subject,
                    crumbs: Vec::new(),
                    trigger: None,
                });
            }
            // Every other part of a float — its head, its foot, its rail, its
            // body, and a body its tenant declined — is this window's own and
            // raises no file menu. Silence, and not the chrome underneath, and
            // not the covered column's ground either.
            Some(PointerTarget::Float(..)) => return None,
            Some(PointerTarget::Chrome(seats::ChromeTarget::FilesRow { seat, index })) => {
                (seat, index)
            }
            Some(PointerTarget::Chrome(_)) | None => {
                return self.files_ground_under(position);
            }
        };
        let now = Instant::now();
        let trees = self.files_trees(now);
        let tree = trees.get(&seat)?;
        // **The row a new entry is being named in raises nothing** (0.3). It is
        // in the list because the box has to stand somewhere and every row under
        // it has to move down, but it names no file: there is nothing on the
        // disk to open, copy the path of, or send to the bin until Enter is
        // pressed. This is the same silence a `Cycle` and a `Notice` get and for
        // the same reason — [`files_row_menu_subject`], which is where the other
        // two are refused.
        //
        // **A row being *renamed* still raises its menu**, and the difference is
        // not a nicety: that row is a real entry with a real path, and the draft
        // over it is a name it may yet not take.
        let pending = matches!(
            self.files_edit_place(),
            Some(place) if place.seat == seat && matches!(place.at, FilesEditRow::New { .. })
        );
        if pending && tree.edit.as_ref().is_some_and(|edit| edit.at == index) {
            return None;
        }
        let row = tree.rows.get(index)?;
        let subject = files_row_menu_subject(row.kind)?;
        let key = row.key.clone();
        Some(FileMenuTarget {
            activation: files_row_activation(
                &self.window.tabs[self.window.active_tab]
                    .files_state(seat)
                    .root,
                &key,
            ),
            row: Some(FileMenuTreeRow {
                host: RowHost::Column(seat),
                key,
            }),
            subject,
            crumbs: Vec::new(),
            trigger: None,
        })
    }

    /// **The column's ground, resolved to the root folder's menu** (user ruling
    /// 2026-09-10).
    ///
    /// The root is addressed the way every other node of this tree is — by its
    /// key, and the root's key is the empty string. That is not a convention
    /// invented here: [`files::full_path`] already returns the root itself for
    /// it, and [`Runtime::open_files_row_new`] already reads an empty parent as
    /// "at the top level", which is why `New file…` and `New folder…` need no
    /// new door to place their box on the ground.
    ///
    /// **A column and never a float.** The verbs this face offers are the
    /// column's — `open_files_row_new` answers only [`RowHost::Column`], and a
    /// peek is a look at a folder rather than a place to make things in — so a
    /// float's own ground stays what it was: the window, and no menu. Since the
    /// review of 2026-09-11 that is enforced *above* this function rather than
    /// hoped for inside it: [`Runtime::file_row_under`] consumes a float's claim
    /// and never reaches here with a point that is inside one.
    ///
    /// A column with no root hands back [`RowActivation::Nowhere`] and
    /// [`Runtime::open_file_menu`] refuses it there, which is the same refusal a
    /// row of a rootless column already gets and is why there is no guard here.
    ///
    /// **An empty folder answers** (review row D3). The ground is resolved
    /// against the column's body rather than against its row geometry, so a
    /// column whose whole tree is the sentence *this folder is empty* is still
    /// standing on a folder and still raises its menu — which is the one case
    /// `New file…` was invented for and the one case it could not be reached in.
    /// Which sentences still decline is [`seats::files_ground_at`]'s answer.
    fn files_ground_under(&self, position: PhysicalPosition<f64>) -> Option<FileMenuTarget> {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let seat = seats::files_ground_at(
            &self.seat_layout,
            &self.files_tree_contents(),
            &self.window.git_pages_shown,
            scale,
            self.git_panel_on(),
            position.x,
            position.y,
        )?;
        let root = self.window.tabs[self.window.active_tab]
            .files_state(seat)
            .root;
        Some(FileMenuTarget {
            row: Some(FileMenuTreeRow {
                host: RowHost::Column(seat),
                key: String::new(),
            }),
            activation: files_row_activation(&root, ""),
            subject: profiles::FileMenuSubject::Root,
            crumbs: Vec::new(),
            trigger: None,
        })
    }

    /// The trigger a folder printed under the pointer speaks for, when there is
    /// one.
    ///
    /// [`Self::terminal_reference_cell`]'s opposite number, and the pair is
    /// exhaustive by construction: one resolution
    /// ([`Self::terminal_reference_at`]) answers both, each takes the arm of
    /// [`ReferenceCard`] that is its own, and a reference is therefore never
    /// both a glance and a flyout nor — which is the failure that matters —
    /// neither because two readings of "is this a folder" disagreed.
    pub(crate) fn folder_reference_trigger(&self) -> Option<float::FloatTrigger> {
        if self.window.mouse_route.is_some() || self.math_hit().is_some() {
            return None;
        }
        let (seat, hit) = self.pane_frame_hit()?;
        let cell = self.reference_cell_index(seat, hit)?;
        matches!(
            self.pointer_reference_at(seat, cell)?.card,
            ReferenceCard::Folder(_)
        )
        .then(|| float::FloatTrigger::Reference {
            leaf: LeafId {
                tab: self.window.tabs[self.window.active_tab].id,
                seat,
            },
            cell,
        })
    }

    /// **`Expand` / `Collapse`** — the folder menu's first row (user ruling
    /// 2026-08-25).
    ///
    /// The same toggle a press on the row is, through the same
    /// [`press_files_node`] both hosts already call, because a menu row and a
    /// click are one verb asked for two ways. What it deliberately does *not* go
    /// through is either host's press handler: those two also count the row
    /// toward the double-click chain, and a fold committed from a menu is not
    /// half of a double click.
    ///
    /// **The row is asked for again**, on the live tree, because the menu can
    /// stand open while a directory lands and the tree behind it grows. A key
    /// that no longer names a row folds nothing, which is the honest answer.
    pub(crate) fn fold_files_row(&mut self, host: RowHost, key: &str) -> Result<()> {
        let now = Instant::now();
        let motion = self.app.motion;
        match host {
            // A Git page draws no folders, so it has no fold to run and no
            // expansion set to run it in. It cannot reach this row —
            // `file_row_under` only answers for the two tree hosts — and
            // answering it here rather than reaching for a tree it has not got
            // is what keeps that true if a third caller ever appears.
            RowHost::Git(_) | RowHost::Terminal(_) => return Ok(()),
            RowHost::Column(seat) => {
                let trees = self.files_trees(now);
                let Some(kind) = trees
                    .get(&seat)
                    .and_then(|tree| tree.rows.iter().find(|row| row.key == key))
                    .map(|row| row.kind)
                else {
                    return Ok(());
                };
                let active = self.window.active_tab;
                let Some(state) = self.window.tabs[active].files.get_mut(&seat) else {
                    return Ok(());
                };
                let opening = press_files_node(state, key, kind);
                self.window.tabs[active]
                    .file_trees
                    .entry(seat)
                    .or_default()
                    .turn_row(key, opening, now, motion);
                if opening {
                    self.refresh_files_dir(seat, key);
                }
                self.mark_session_dirty(now);
            }
            RowHost::Float(id) => {
                let Some(win) = self.window.float.live_mut(id) else {
                    return Ok(());
                };
                let epoch = win.epoch;
                let Some(files) = win.files_mut() else {
                    return Ok(());
                };
                let rows = files::tree_view(&files.files, &files.cache).rows;
                let Some(kind) = rows.iter().find(|row| row.key == key).map(|row| row.kind) else {
                    return Ok(());
                };
                let root = files.files.root.clone();
                let opening = press_files_node(&mut files.files, key, kind);
                files.cache.turn_row(key, opening, now, motion);
                if opening {
                    // Unfolding is this product's refresh gesture, so an opened
                    // directory is re-asked exactly as a press on it re-asks.
                    let request = files::DirRequest {
                        window: self.window_id(),
                        host: files::FilesHost::Float(epoch),
                        path: files::full_path(&root, key),
                        key: key.to_owned(),
                    };
                    if !self.app.files_worker.request(request) {
                        self.disable_files_worker();
                    }
                }
            }
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// **`New terminal here`** — a folder row's own shell (user ruling
    /// 2026-08-25).
    ///
    /// **A tab, and that is the ruling this row makes.** The verb it reuses has
    /// two containers already — `New terminal in folder…` splits the pane when
    /// it is raised from a pane's `⌄` and opens a tab when it is raised from the
    /// new-tab menu (user ruling 2026-08-20) — and a menu has to pick one.
    /// A tab, for two reasons. It is the container **both hosts can always
    /// give**: a floating tree is not a pane, so there is nothing there to split,
    /// and a row that meant one thing in a column and another in a float would be
    /// the two surfaces disagreeing about one word. And a files column is very
    /// often the whole point of the tab it stands in — halving it to push a shell
    /// in beside it is a bigger change to the window than a row named
    /// `New terminal here` promises.
    ///
    /// **No chooser, and that is why the row has no ellipsis.** The two rows that
    /// spell this verb with three dots ask *which folder* before anything
    /// happens; this one already knows, because you right-clicked it. It is
    /// therefore [`Runtime::apply_folder_pick_result`]'s `NewTabIn` arm without
    /// the dialog in front of it — the default profile, told where to stand,
    /// because "the default profile" is what *new tab* means everywhere else in
    /// this build.
    pub(crate) fn new_terminal_in_folder(&mut self, path: &Path) -> Result<()> {
        let profile = self.default_profile_id();
        self.new_tab_with_profile(&profile, Some(path.to_path_buf()))
    }

    /// Show a docked column's root in File Explorer — `.files-pane .files-foot`
    /// (mock-up 527-537), the same verb the float's foot has and the same
    /// confirmation after it.
    ///
    /// **Through `open_local_path`**, which is this window's one door out to the
    /// shell for a path the user pointed at: it refuses the executable list
    /// whatever `PATHEXT` says, and a directory is nothing on that list — so
    /// "the tree never runs a program" costs this button nothing and is enforced
    /// by the same gate that enforces it for a double-clicked row, rather than by
    /// this call site being careful.
    pub(crate) fn reveal_files_root(&mut self, seat: SeatId) -> Result<()> {
        let root = self.window.tabs[self.window.active_tab]
            .files_state(seat)
            .root;
        if root.is_empty() {
            return Ok(());
        }
        // A root is a *place*, so it is opened rather than selected — see
        // `bt_platform::reveal_arguments` for why `/select` on a folder is one
        // level too far out. A failure to reach Explorer withholds the
        // confirmation rather than claiming one, which is the honest report.
        if !self.reveal_in_explorer(Path::new(&root)) {
            return Ok(());
        }
        self.window.revealed_foot = Some((RevealedFoot::Column(seat), Instant::now()));
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// The tree tenant, drawn — **on whichever of its two pages it is standing**.
    ///
    /// R2 used to answer this with one page and no question asked: a float drew
    /// the file tree whatever `FilesLeafState::view` said, so a column popped out
    /// of its Git page landed the reader on the tree without a word. The ruling
    /// that closed the report (2026-08-19) is the plain one — a pop-out is a
    /// *move*, and a move that changes what you were looking at is a different
    /// verb — so the page comes with the window and works there.
    pub(crate) fn files_float_layer(
        &mut self,
        id: float::FloatId,
        now: Instant,
    ) -> Option<marks::OverlayLayer> {
        let (geometry, fade) = self.float_geometry_of(id)?;
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let motion = self.app.motion;
        let (mode, root) = {
            let win = self.window.float.drawn().find(|win| win.epoch == id)?;
            (win.mode, win.files()?.files.root.clone())
        };
        let palette = bt_render::chrome_palette();
        let mut quads = Vec::new();
        let mut labels = Vec::new();
        let mut sprites = Vec::new();
        // **This window's hover, or nobody's.** One pointer means one hovered
        // window, so a row index that forgot which window it belonged to would
        // light the same row in every float on screen.
        let hover = self
            .window
            .float_hover
            .filter(|(hovered, _)| *hovered == id)
            .map(|(_, part)| part);
        if self.float_shows_git_page(id) {
            self.push_float_git_page(
                id,
                geometry.body,
                hover,
                scale,
                &palette,
                (&mut quads, &mut labels, &mut sprites),
            );
        } else {
            // Everything the drawing needs, taken in one borrow so the measuring
            // below — which wants the renderer — is not fighting the host for
            // `self`.
            let content = {
                let win = self.window.float.drawn().find(|win| win.epoch == id)?;
                let files = win.files()?;
                let view = files::tree_view(&files.files, &files.cache);
                let turns = view
                    .rows
                    .iter()
                    .filter_map(|row| match row.kind {
                        files::RowKind::Directory { open } => Some((
                            row.key.clone(),
                            files.cache.row_turn(&row.key, open, now, motion).0,
                        )),
                        _ => None,
                    })
                    .collect();
                seats::FilesTreeContent {
                    foot_dissolved: 0.0,
                    rows: view.rows,
                    // **None, and R2's surviving half is why.** A badge is
                    // subordinate to a page: it shows what an *open* Git page is
                    // already keeping true, and this window is standing on its
                    // tree — the one on its Git page is not drawing a tree to put
                    // badges in. The old second reason ("`FloatFiles` names no
                    // tab, so this tab's open Git page is a question this window
                    // cannot ask") is gone with the ruling that gave the window a
                    // repository of its own.
                    badges: crate::git_panel::GitTreeBadges::default(),
                    scroll_px: files.cache.scroll_px,
                    selected: files.files.sel.clone(),
                    turns,
                    // **A float never holds the name editor**, which is the same
                    // sentence `focus_ring` makes one line down and for the same
                    // reason: no key is routed to a float's page yet, and
                    // `open_files_row_rename` declines a host that is not a
                    // docked column — a box measured into a body this module does
                    // not lay out would be a box nobody can see.
                    edit: None,
                    // Never, for either mode — ruled 2026-08-12. A transient
                    // peek is never keyboard-driven (G90). A pinned float holds
                    // `win.focused` from the moment a *click* tore it off, which
                    // is `:focus`; the ring is `:focus-visible`, earned only by
                    // keys — and no key is routed to a float's *either* page yet
                    // (the float-keyboard ledger item). When that lands, this
                    // becomes the float's own keyboard-visible bit, exactly as
                    // the docked column's — and which page answers it is already
                    // decided, by [`Self::float_shows_git_page`] above.
                    focus_ring: false,
                    // A float wears its own foot, drawn by `float::build` from
                    // the chassis geometry — these two are the *docked* strip's
                    // and nothing here reads them.
                    foot_path: String::new(),
                    foot_revealed: false,
                }
            };
            let hovered_row = match hover {
                Some(float::FloatPart::Row(index)) => Some(index),
                _ => None,
            };
            // The tree, drawn by the very function the docked column uses (C39)
            // — handed a body rect, a hovered row and its own ground's inks, and
            // asking nothing about which host it is in.
            seats::push_files_tree(
                geometry.body,
                &content,
                hovered_row,
                seats::FilesRowInk::on_float(&palette),
                scale,
                &palette,
                (&mut quads, &mut labels, &mut sprites),
            );
        }
        let body = float::FloatBody {
            // Chrome fills are opaque by construction, so the lift into the
            // overlay's vocabulary is the rail's own: alpha 1.0, and the layer's
            // opacity carries the fade for all three channels at once.
            quads: quads
                .into_iter()
                .map(|quad| bt_render::OverlayQuad {
                    rect: quad.rect,
                    color: quad.color,
                    alpha: 1.0,
                })
                .collect(),
            labels,
            sprites,
        };
        let head_font = float::FLOAT_HEAD_FONT_LOGICAL_PX * scale;
        let foot_font = float::FLOAT_FOOT_FONT_LOGICAL_PX * scale;
        // `#files-flyout .fly-head { text-transform: uppercase }` — the head
        // shouts its ROOT, and the rule is the files head's alone because a
        // *filename* keeps its case (mock-up 698-699).
        let name = profiles::cwd_leaf(&root).to_uppercase();
        let revealed = self.foot_reveal_is_fresh(RevealedFoot::Float(id), now);
        // The docked column's own sentence, in a window of its own.
        let (said, foot_dissolved) = self.foot_saying(
            FootSaying::Float(id),
            revealed.then(foot_revealed_label),
            now,
        );
        let revealed = said.is_some();
        // The torn-out tree's foot is the docked column's foot in a window of
        // its own, so it spells a root the same way (§13.40).
        let root = match said.as_deref() {
            Some(said) => std::borrow::Cow::Borrowed(said),
            None => home_shortened_path(&root),
        };
        let (name, path) = {
            let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
            let mut measure = |text: &str, size: f32| renderer.measure_chrome_text(gpu, text, size);
            (
                settings::ellipsized(
                    &name,
                    geometry.head_title[2] - geometry.head_title[0],
                    head_font,
                    &mut measure,
                ),
                // B23: the foot is cut from the front, because the end of a path
                // is the part that answers "where am I".
                settings::ellipsized_left(
                    &root,
                    geometry.foot_path[2] - geometry.foot_path[0],
                    foot_font,
                    &mut measure,
                ),
            )
        };
        Some(float::build(
            &geometry,
            &float::FloatChrome {
                dissolved: foot_dissolved,
                mode,
                // `#i-folder` and a left-hand dock panel: this window holds a
                // tree, and P54's side-honesty puts the filled half where the
                // pane will land.
                mark: marks::ChromeMark::Folder,
                title: &name,
                path: &path,
                // A folder is not a buffer: there is no head read to be
                // truncated and no save to refuse, so this foot's right hand is
                // empty by construction rather than by this frame's luck.
                notice: "",
                notice_width: 0.0,
                dock_label: float_dock_label(),
                dock_mark: marks::ChromeMark::DockLeft,
                hover,
                revealed,
                // A tree has no buffer, so it has neither the dot nor the two
                // verbs the dot and the flip belong to.
                dirty: false,
                flip_to_source: false,
            },
            body,
            scale,
            &palette,
            fade,
        ))
    }

    /// `.pane-float` — a docked column pops back out into a pinned window (G95).
    ///
    /// It carries root, expansion, selection, width **and scroll position**
    /// (G97; the cache holds the last of those, which is the half the mock-up
    /// never managed) — the column is not being closed and reopened, it is being
    /// moved, and a move that lost half its state would be a different verb.
    ///
    /// **Which page it is standing on is part of that state** (user ruling,
    /// 2026-08-19). It always was — `FilesLeafState::view` travels in the clone
    /// below — but the window it arrived in drew the tree unconditionally, so a
    /// column popped out of its Git page landed on the file tree without a word.
    /// The page comes with it now, and so does everything the page is drawn
    /// from: the repository cache, its scroll and its selection. A move that
    /// re-read the repository would be a different verb too.
    pub(crate) fn undock_files_column(&mut self, seat: SeatId) -> Result<()> {
        let active = self.window.active_tab;
        let Some(state) = self.window.tabs[active].files.get(&seat).cloned() else {
            return Ok(());
        };
        let cache = self.window.tabs[active]
            .file_trees
            .get(&seat)
            .cloned()
            .unwrap_or_default();
        let git = self.window.tabs[active]
            .git_trees
            .get(&seat)
            .cloned()
            .unwrap_or_default();
        let git_scroll = self.window.tabs[active]
            .git_scroll
            .get(&seat)
            .copied()
            .unwrap_or(0.0);
        let width = self
            .seats
            .fixed_extent_of(seat)
            .unwrap_or(bt_layout::FILES_W);
        // **The button's own box, and not the column's** (user report,
        // 2026-08-16). The window hangs off the control that summoned it, which
        // is every other float's rule and `pop_out_preview`'s exactly. Anchored
        // to the whole column instead, `float_placement` was asked to fit the
        // window above or below a trigger that already spans the viewport from
        // top to bottom, found no room on either side, and did the only thing
        // its last resort allows: a window the height of its own strip — the
        // bare bar at the foot of the window the report shows.
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let anchor = seats::full_pane_rect(&self.seat_layout, seat).and_then(|rect| {
            seats::pane_head_geometry(
                rect,
                bt_layout::SeatKind::Files,
                self.seat_layout.seat_is_on_stage(seat),
                scale,
            )
            .float
        });
        // Taking the pane out first, so the window is never both docked and
        // floating at once — the mock-up's own note (3828-3833) records that
        // exact duplication, and the case it could not handle: the last pane of
        // the last tab, where closing refuses to empty the strip. `close_pane`
        // already answers that by keeping the tab alive, so there is no special
        // case left here to write.
        self.close_pane(seat)?;
        self.place_float(
            float::FloatMode::Pinned,
            None,
            float::FloatFiles {
                // The page it is standing on, its expansion and its row all
                // travel inside this one value — they are the column's state and
                // this *is* that column, moved.
                files: state,
                cache,
                git,
                git_scroll,
                width,
            },
            anchor,
        )
    }

    /// The files column holding the keyboard *right now*, or `None`.
    ///
    /// Every way the stored [`LeafId`] can go stale is answered here rather than
    /// by writing `None` back from each of the places one of them happens: the
    /// tab was switched, the column was closed, the pane it named became
    /// something else. Reading it is therefore total and self-healing, and the
    /// keyboard falls back to the shell the instant the column it was lent to
    /// stops being on screen.
    pub(crate) fn files_keyboard_seat(&self) -> Option<SeatId> {
        files_keyboard_seat_of(
            self.window.files_focus.owner,
            &self.window.tabs[self.window.active_tab],
        )
    }

    /// Whether the column holding the keyboard is *showing* that it does —
    /// `.files-tree:focus-visible`, healed through the same read as the owner.
    fn files_ring_seat(&self) -> Option<SeatId> {
        self.window
            .files_focus
            .visible
            .then(|| self.files_keyboard_seat())
            .flatten()
    }

    /// Lend the keyboard to a column, or take it back for the shells.
    ///
    /// `arrival` is what decides the ring, and every caller knows it because
    /// every caller *is* one gesture or the other: a press passes
    /// [`FilesFocusArrival::Pointer`] and a key passes
    /// [`FilesFocusArrival::Keyboard`]. That is `:focus-visible` and not
    /// `:focus`, and the difference is the whole of the 2026-08-12 ruling — a
    /// click on a row used to light the accent ring, which the mock-up's own
    /// rule never does.
    ///
    /// Returns whether anything changed, because the ring is drawn from this and
    /// a frame is owed exactly when it moves — including when the *owner* stands
    /// still and only the ring goes out, which is what a press on the column that
    /// already had the keyboard does.
    pub(crate) fn set_files_keyboard(
        &mut self,
        seat: Option<SeatId>,
        arrival: FilesFocusArrival,
    ) -> bool {
        let tab = self.window.tabs[self.window.active_tab].id;
        let owner = seat.map(|seat| LeafId { tab, seat });
        self.window.files_focus.arrive(owner, arrival)
    }

    /// Enter, Space or a double click on a file row (K156/D44).
    ///
    /// **One door, one gesture.** Every file goes to the preview seat — a
    /// picture down the decode lane it always used, so a `.png` opens in the
    /// same place however you reached it, and everything else through the tab's
    /// buffer pool, which shows it as text or says in its own body why it
    /// cannot. `DESIGN.md` §7.1.3 asked for exactly that; until this slice the
    /// rest went to the system's own handler, and that interim is over.
    ///
    /// **The keyboard does not move.** §7.1.3 asks for browsing continuity in as
    /// many words ("打开时键盘焦点不离开树"): opening a file is something you do
    /// on the way past, and a tree that hands the keyboard away every time you
    /// look at something can only be walked once.
    fn activate_files_row(&mut self, seat: SeatId, key: &str) -> Result<()> {
        let root = self.window.tabs[self.window.active_tab]
            .files_state(seat)
            .root;
        match files_row_activation(&root, key) {
            RowActivation::Preview(path) => self.open_preview(path),
            // `files_row_activation` mints one of two answers and neither of
            // them is the rail's — a tree row is always "open this here". The
            // arm is written rather than wildcarded so that the day a third door
            // reaches this function, it is a compiler error and not a silence.
            RowActivation::DefaultApp(path) => {
                self.open_local_path(&path);
                Ok(())
            }
            RowActivation::Nowhere => Ok(()),
        }
    }

    /// One key, with a files column holding the keyboard (D44/D47).
    ///
    /// Returns whether the key was the tree's at all. Everything that *is* the
    /// tree's is consumed here whether or not it moved anything, which is D49
    /// stated as code: with a column focused there is nothing to type into, and
    /// a letter that fell through to the encoder would land in a shell the user
    /// is not looking at.
    fn files_tree_key(&mut self, seat: SeatId, event: &KeyEvent) -> Result<bool> {
        let Some(command) = files::tree_command(&event.logical_key, self.window.modifiers) else {
            // Still the tree's key: it owns the keyboard, so nothing here
            // reaches a shell. It simply has nothing to do with this one.
            //
            // And it does *not* light the ring. Rule ② is about the keys this
            // column answers — the arrows, Home/End, Enter, Escape — not about
            // every byte that happens to arrive while it holds the keyboard: a
            // stray letter is the user typing at a list that has nothing to type
            // into, and answering it with a focus ring would be the column
            // claiming a gesture it did not receive.
            return Ok(true);
        };
        // **Rule ②** (`:focus-visible`, 2026-08-12): the column already had the
        // keyboard, and it is now being *used* as a keyboard. Nothing about who
        // owns it changed — only that the ring has been earned, which is the one
        // transition `set_files_keyboard` cannot express. Set before the command
        // is carried out, so the very keypress that moves the selection is the
        // one that lights the ring around it, in the same frame.
        if self.window.files_focus.navigated() {
            self.refresh_chrome();
        }
        if event.repeat
            && matches!(
                command,
                files::TreeCommand::Activate | files::TreeCommand::ContextMenu
            )
        {
            // Holding Enter down on a file would open it once per repeat.
            // Travel repeats happily; opening is a verb you mean once — and so
            // is asking a row what can be done with it.
            return Ok(true);
        }
        if command == files::TreeCommand::Release {
            // The arrival is immaterial with no owner to arrive at — rule ④: the
            // bit stops meaning anything the moment the keyboard leaves, and the
            // next entry recomputes it from *how* that entry happened.
            if self.set_files_keyboard(None, FilesFocusArrival::Keyboard) && self.refresh_chrome() {
                self.present_chrome_change()?;
            }
            return Ok(true);
        }
        let now = Instant::now();
        let motion = self.app.motion;
        let rows = self
            .files_trees(now)
            .get(&seat)
            .map(|tree| tree.rows.clone())
            .unwrap_or_default();
        let active = self.window.active_tab;
        let Some(state) = self.window.tabs[active].files.get_mut(&seat) else {
            return Ok(true);
        };
        let action = files::apply_tree_command(state, &rows, command);
        let selected = match &action {
            files::TreeAction::None | files::TreeAction::Release => None,
            files::TreeAction::Select(key)
            | files::TreeAction::Opened(key)
            | files::TreeAction::Closed(key)
            | files::TreeAction::Activate(key)
            // Scrolled into view like any other answered key: the menu is about
            // to be hung off this row's rectangle, and a rectangle that is
            // scrolled out of the body is not somewhere a menu can hang from.
            | files::TreeAction::ContextMenu(key) => Some(key.clone()),
        };
        match &action {
            files::TreeAction::Opened(key) | files::TreeAction::Closed(key) => {
                let opened = matches!(action, files::TreeAction::Opened(_));
                self.window.tabs[active]
                    .file_trees
                    .entry(seat)
                    .or_default()
                    .turn_row(key, opened, now, motion);
                if opened {
                    self.refresh_files_dir(seat, key);
                }
            }
            files::TreeAction::Activate(key) => {
                let key = key.clone();
                self.activate_files_row(seat, &key)?;
            }
            files::TreeAction::ContextMenu(_) => {}
            files::TreeAction::None | files::TreeAction::Select(_) | files::TreeAction::Release => {
            }
        }
        if let Some(key) = selected {
            self.reveal_files_row(seat, &key);
        }
        // After the scroll, not before: the anchor is the row's rectangle, and
        // `reveal_files_row` may have just moved it.
        if let files::TreeAction::ContextMenu(key) = &action {
            let key = key.clone();
            self.raise_file_menu_on_row(seat, &key)?;
        }
        if !matches!(action, files::TreeAction::None) {
            self.mark_session_dirty(now);
        }
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(true)
    }

    /// Scroll a column just far enough that the row it is standing on is on
    /// screen.
    ///
    /// A browser does this for nothing — a focused element is scrolled into view
    /// by the engine, which is why the mock-up's `filesTreeKey` never mentions
    /// it. Here it is ours, and without it ↓ walks the selection off the bottom
    /// edge and the tree looks like it stopped responding.
    ///
    /// Minimal travel and never re-centring: a row already in view moves
    /// nothing, and a row just past an edge comes exactly to that edge. The
    /// alternative — always putting the selection in the middle — makes every
    /// keypress scroll the whole list, which is the same list moving under your
    /// eyes for no reason.
    fn reveal_files_row(&mut self, seat: SeatId, key: &str) {
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let trees = self.files_tree_contents();
        let Some(tree) = trees.get(&seat) else {
            return;
        };
        let Some(index) = tree.rows.iter().position(|row| row.key == key) else {
            return;
        };
        let Some(body) =
            seats::files_body_rect(&self.seat_layout, seat, scale, self.git_panel_on())
        else {
            return;
        };
        let geometry = seats::files_tree_geometry(body, tree.rows.len(), tree.scroll_px, scale);
        let row = geometry.row_rect(index);
        let above = geometry.viewport[1] - row[1];
        let below = row[3] - geometry.viewport[3];
        let travel = if above > 0.0 {
            -above
        } else if below > 0.0 {
            below
        } else {
            return;
        };
        let rows = tree.rows.len();
        let cache = self.window.tabs[self.window.active_tab]
            .file_trees
            .entry(seat)
            .or_default();
        cache.scroll_px = seats::clamp_files_scroll(body, rows, geometry.scroll_px + travel, scale);
    }

    /// Wheel over a files column's body (C28's `overflow-y: auto`).
    pub(crate) fn scroll_files_tree(
        &mut self,
        seat: SeatId,
        body: [f32; 4],
        delta: MouseScrollDelta,
    ) -> Result<()> {
        let travel = self.vertical_wheel_travel(delta, body[3] - body[1]);
        let scale = self.window.renderer.metrics().scale_factor as f32;
        let rows = self
            .files_tree_contents()
            .get(&seat)
            .map(|tree| tree.rows.len())
            .unwrap_or_default();
        let active = self.window.active_tab;
        let cache = self.window.tabs[active].file_trees.entry(seat).or_default();
        // **Both ends, here** (R2 乙案). There is no scrolling backwards past the
        // top and none forwards past the last row, and this side knows both —
        // it has the body the painter will use and the rows that are in it. The
        // upper half used to be left to the painter, which meant a wheel at the
        // end of a list went on adding to a number nothing was reading: the
        // stored offset ran away from the picture, and coming back up cost one
        // notch per notch spent, with the list frozen until the debt was paid.
        let scrolled = seats::clamp_files_scroll(body, rows, cache.scroll_px - travel, scale);
        if scrolled == cache.scroll_px {
            return Ok(());
        }
        cache.scroll_px = scrolled;
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Pull every column's stored scroll back inside what its list can still
    /// show, in the cache **and** in the frame's own copy of it.
    ///
    /// The third writer, and the one that answers the half of R2 乙案 the other
    /// two cannot: a scroll that was legal when it was written stops being legal
    /// when the *list* changes under it — a folder folded, a directory re-read
    /// with fewer children in it, a divider dragged to make the pane taller. The
    /// wheel is not involved in any of those and has nothing to clamp.
    ///
    /// It is handed the trees this frame is about to draw rather than walking
    /// its own, and it writes the healed number into both: one walk, and the
    /// picture and the cache cannot disagree about the offset even for the frame
    /// the heal happens on. `refresh_chrome` is the caller, which is the one
    /// funnel every path that rebuilds the picture already goes through, so
    /// "what is stored is what is drawn" holds for every frame rather than for
    /// the frames somebody remembered to heal.
    pub(crate) fn heal_files_scroll(
        &mut self,
        scale: f32,
        trees: &mut BTreeMap<SeatId, seats::FilesTreeContent>,
    ) {
        let active = self.window.active_tab;
        let segmented = self.git_panel_on();
        for (seat, tree) in trees.iter_mut() {
            let Some(body) = seats::files_body_rect(&self.seat_layout, *seat, scale, segmented)
            else {
                continue;
            };
            let healed = seats::clamp_files_scroll(body, tree.rows.len(), tree.scroll_px, scale);
            tree.scroll_px = healed;
            if let Some(cache) = self.window.tabs[active].file_trees.get_mut(seat) {
                cache.scroll_px = healed;
            }
        }
    }

    /// **Measure the open name editor into the tree row it is drawn in** (0.3).
    ///
    /// `dress_preview_name_editor`'s job one surface over, and the same
    /// division: the walk decided *which row* the box is on, and this fills in
    /// what is drawn in it — which only something holding a font can say. The
    /// row's own name box is asked of [`seats::files_row_name_box`], the very
    /// function the painter places the label with, so the caret and the letters
    /// it stands between are measured against one rectangle.
    ///
    /// **The caret line is published here too**, for the reason every other
    /// surface publishes its own: the IME's candidate list has to stand under
    /// what is being typed, and this is the one place that rectangle exists.
    pub(crate) fn dress_files_tree_editor(
        &mut self,
        scale: f32,
        trees: &mut BTreeMap<SeatId, seats::FilesTreeContent>,
    ) {
        let Some(place) = self.files_edit_place() else {
            return;
        };
        let seat = place.seat;
        // **Only a new entry can be refused in the box**, and only once
        // something has been typed: an empty draft is a field nobody has
        // finished, not a name the folder said no to. A rename's refusals are
        // silent by a ruling of their own (2026-08-19) — there is a name
        // underneath to fall back to, and the reader can see it.
        //
        // **The collision is asked of the rows and not of the disk.** This runs
        // on every frame the caret blinks, and a `stat` per frame is exactly the
        // shape red line R-i forbids; the tree's own listing is what the reader
        // is looking at, so it is also the honest thing to answer from. The
        // authoritative check is at the commit, where it happens once.
        //
        // **But it is asked the way the folder itself would ask it** (D8(a) of
        // the 2026-09-11 review). The rows were compared by exact bytes while
        // the commit compared with `path.exists()`, which on an ordinary Windows
        // volume folds case — so with `Notes.md` in the folder, `notes.md` drew
        // no red, Enter hit `Taken`, and the field sat there looking valid and
        // doing nothing for ever. The folder was asked which it is when the box
        // opened ([`RenameSubject::FilesNew`]); one answer, read here.
        let folds_case = matches!(
            self.window.rename.as_ref().map(|editor| &editor.subject),
            Some(RenameSubject::FilesNew {
                folds_case: true,
                ..
            })
        );
        // **And the refusal the commit really raised outranks the prediction**
        // (D8(a) again). Everything above is an advisory; a commit-only refusal
        // — a file that arrived since the last listing, a volume that refused
        // the name for a reason no listing shows — is what
        // [`TabRename::refused`] carries, and it is drawn in the same red. The
        // empty draft stays outside both: an empty field is a field nobody has
        // finished, not a name the folder said no to.
        let refused = match (place.at, self.window.rename.as_ref()) {
            (FilesEditRow::New { parent, .. }, Some(editor)) if !editor.text().is_empty() => {
                let name = editor.text().trim();
                editor.refusal().is_some()
                    || files::judge_new_name(editor.text()).is_some()
                    || trees.get(&seat).is_some_and(|tree| {
                        let taken = files::child_key(parent, name);
                        tree.rows
                            .iter()
                            .any(|row| files::names_are_one(&row.key, &taken, folds_case))
                    })
            }
            _ => false,
        };
        let Some(tree) = trees.get_mut(&seat) else {
            return;
        };
        let Some(edit) = tree.edit.as_mut() else {
            return;
        };
        let Some(row) = tree.rows.get(edit.at) else {
            return;
        };
        let segmented = self.git_panel_on();
        let Some(body) = seats::files_body_rect(&self.seat_layout, seat, scale, segmented) else {
            return;
        };
        let geometry = seats::files_tree_geometry(body, tree.rows.len(), tree.scroll_px, scale);
        let name_box = seats::files_row_name_box(geometry.row_rect(edit.at), row.depth, scale);
        let box_width = name_box[2] - name_box[0];
        let caret_width = (seats::TAB_RENAME_CARET_LOGICAL_PX * scale)
            .round()
            .max(1.0);
        let font = seats::FILES_TREE_FONT_LOGICAL_PX * scale;
        // Disjoint fields, split by hand, for `measure_open_rename`'s reason:
        // the editor owns where its window starts and the renderer owns how wide
        // a string is, and this is the one place the two have to meet.
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let Some(editor) = self.window.rename.as_mut() else {
            return;
        };
        let mut shape = |text: &str| renderer.chrome_text_advances(gpu, text, font);
        let fitted = editor.fit(box_width, caret_width, true, &mut shape);
        edit.text = fitted.text;
        edit.caret_px = fitted.caret_px;
        edit.selection = fitted.selection;
        edit.caret_lit = self.window.rename_blink.visible();
        edit.refused = refused;
        let x = (name_box[0] + edit.caret_px).min(name_box[2] - caret_width);
        self.window.rename_caret_line = Some([x, name_box[1], x + caret_width, name_box[3]]);
    }

    /// C35, applied only where a landed directory has proved a selection gone.
    fn heal_files_selections(&mut self) {
        let active = self.window.active_tab;
        let tab = &mut self.window.tabs[active];
        for seat in tab.seats.files() {
            let Some(cache) = tab.file_trees.get(&seat) else {
                continue;
            };
            let Some(state) = tab.files.get(&seat) else {
                continue;
            };
            if files::selection_is_dead(state, cache) {
                tab.files.entry(seat).and_modify(|state| state.sel = None);
            }
        }
    }

    /// How wide each column's switch is, for the hit test.
    ///
    /// Derived from the same map the paint was handed, so the two halves of "the
    /// button you can press is the button you can see" are one number. Empty when
    /// the master switch is off, which is what makes the strip unpressable
    /// without a second condition anywhere.
    pub(crate) fn files_seg_widths(&self) -> BTreeMap<SeatId, [f32; 2]> {
        self.window.files_view_widths.clone()
    }

    /// The roots the `Files` section is drawn from: every files column standing
    /// on the tab in front.
    ///
    /// **The tab in front and not the window**, on `watched_files_dirs`'s own
    /// division: a palette raised on a tab is asking about the place that tab
    /// is standing in, and walking the roots of five other tabs would be five
    /// walks nobody asked for to answer a question about one folder.
    pub(crate) fn palette_files_roots(&self) -> BTreeSet<PathBuf> {
        self.files
            .values()
            .filter(|state| files::root_is_addressable(&state.root))
            .map(|state| PathBuf::from(&state.root))
            .collect()
    }

    /// Ask for any index the `Files` section needs and does not have.
    pub(crate) fn ask_for_file_indexes(&mut self) {
        // **It asks and it does not forget.** Forgetting is `App`'s
        // (`retain_file_indexes`), because the register is the app's: a window
        // that pruned it would be one window deciding, out of its own list of
        // roots, that another window's repository is not worth keeping — §2.4's
        // rule 2 with a register in place of a queue.
        for root in self.palette_files_roots() {
            if let Some(request) = self.app.file_indexes.claim(&root) {
                // A lane that has gone is a lane that answers nothing; the
                // section then simply has no rows, which is the honest picture
                // of a walk that cannot happen.
                let _ = self.app.file_index_worker.request(request);
            }
        }
    }

    /// **The note the `Files` section carries when it has no rows to show.**
    ///
    /// Two things can be true of a section that came back empty, and they are
    /// not the same news. The walk may still be running — say so, or the box
    /// teaches a reader that this product cannot find their files. Or it may
    /// have finished *and stopped on its own cap* — say that too, because the
    /// file the reader is looking for may be perfectly real and simply past
    /// the thirty-thousandth entry, and `Nothing matches` would be this window
    /// stating as a fact something it does not know.
    ///
    /// Building wins when both are true: a walk that is still going may yet
    /// turn the second answer into rows.
    pub(crate) fn palette_files_note(&self) -> Option<i18n::Text> {
        let roots = self.palette_files_roots();
        if roots
            .iter()
            .any(|root| self.app.file_indexes.state(root) == palette_index::IndexState::Building)
        {
            return Some(i18n::Text::PaletteIndexing);
        }
        roots
            .iter()
            .filter_map(|root| self.app.file_indexes.get(root))
            .any(palette_index::FileIndex::truncated)
            .then_some(i18n::Text::PaletteFilesTruncated)
    }

    /// Unfold every files column of this tab that has the path under its root.
    pub(crate) fn locate_path_in_files_columns(&mut self, path: &Path) -> Result<()> {
        let targets: Vec<(bt_layout::SeatId, String)> = self
            .files
            .iter()
            .filter_map(|(seat, state)| {
                files::key_under_root(&state.root, path).map(|key| (*seat, key))
            })
            .collect();
        for (seat, key) in targets {
            self.open_files_path_to(seat, &key)?;
        }
        Ok(())
    }

    /// **Spend whatever one drop put on this window** (GitHub issue #1 ②).
    ///
    /// [`Self::flush_wheel`]'s twin, and called from the same two doors for the
    /// same reason: the top of `window_event` for every event that is not
    /// another file of this drop, and the top of a turn. Free — one `Option`
    /// read — on every turn in which nobody dropped anything.
    ///
    /// **One paste, however many files.** The whole batch goes to
    /// [`Self::paste_paths_into`] as one list, which is what puts three files on
    /// one command line rather than running the first two.
    ///
    /// **And it asks nothing about where the pointer is** (release review 0.4.2
    /// X-10). The point travelled here inside the batch, taken when the drop
    /// opened; a reading made at this line would be a reading made *after* the
    /// release, which on a busy window is long enough for the hand to have
    /// reached another pane. See [`DropBatch`].
    ///
    /// The station is entered only when there is a drop to spend and is handed
    /// back on the way out, on [`hang_watch::enter`]'s own rule: this door
    /// stands inside two other functions, and a name it kept would be charged to
    /// the keystroke or the turn that came after it.
    pub(crate) fn flush_dropped_files(&mut self) -> Result<()> {
        let Some(batch) = self.window.dropped_files.take() else {
            return Ok(());
        };
        let leaving = hang_watch::enter(hang_watch::Station::FileDrop);
        // **Nothing is resolved here.** Both halves of the address travelled
        // inside the batch from the moment the drop opened: the point, because a
        // reading taken at this line is taken after the release (X-10), and the
        // shell, because the tab on top and the program in that seat can both
        // have changed by now (X-1). A batch aimed at nothing is spent on
        // nobody.
        // **Admission is asked again here** (review 2026-09-17 P1-b). The
        // address travelled with the batch precisely because the world moves
        // between the release and the turn that spends it — and what can move is
        // not only the tab and the shell. A quit card, a gate or a settings page
        // that came up in between is a window now waiting for a keystroke, and a
        // path written under it would be followed by an `Enter` that answers the
        // card. Refused the same way an unaimed drop is: nothing typed, nothing
        // focused, nothing raised, and the point said out loud for the report
        // somebody will make.
        let target = batch.target.filter(|_| !self.a_modal_holds_the_window());
        let pasted = match target {
            Some(target) => self
                .paste_paths_into(target, batch.paths, "write dropped paths to PTY")
                .and_then(|written| {
                    // **The keyboard follows the path, and the window comes to
                    // the front with it** (owner's ruling 2026-09-17) — once for
                    // the whole batch, because a batch is one drop however many
                    // files it carried. Behind the write's own answer: a drop
                    // that reached no shell raises nothing and focuses nothing.
                    if written {
                        self.focus_the_pane_a_path_landed_in(target.seat)?;
                        self.bring_this_window_forward();
                    }
                    Ok(())
                }),
            // A drop aimed at chrome, at a files column, or at a pane with no
            // shell behind it: nothing is typed, which is the answer
            // `paste_paths_into` gave for those before the address existed. The
            // point is said out loud because "I dropped a file and nothing
            // happened" is a report somebody will make, and where the hand was
            // is the whole of what answers it. A coordinate and never a path:
            // the names in a drop are the reader's files.
            None => {
                eprintln!(
                    "dropped files landed on no shell; opened at {:?}",
                    batch.point
                );
                Ok(())
            }
        };
        hang_watch::at(leaving);
        pasted
    }

    /// **Which pane a dropped path is typed into** (GitHub issue #1 ②).
    ///
    /// The pane under the pointer, asked of the same router a press is asked of
    /// — a float's claim is terminal, an open rail or focus column covers what
    /// is behind it, and what is left is [`seats::pane_at`]. A point that is
    /// none of those is chrome or no pane at all, and the answer there is the
    /// pane holding the keyboard: a path is going onto a command line, and the
    /// command line the reader is typing on is the only one this window can
    /// honestly mean.
    ///
    /// **A pane that is not a terminal is still that pane's drop.** The files
    /// column is a leaf of the layout tree like any other, and so is a preview
    /// pane; [`Self::paste_paths_into`] finds no shell on either and does
    /// nothing, which is the honest answer rather than sending the path
    /// somewhere the hand was not.
    ///
    /// **It is also the line between the two roads, and that line moved on
    /// 2026-09-16 without moving here.** §7.1.1 now lets an *internal* drag put
    /// a path on a command line as well — but only over a terminal's middle, and
    /// only with `Paste path` written on the box first, which is the whole of
    /// what the revision turns on. This road has no box to write on: a drag from
    /// Explorer or the Finder is another application's, and winit answers
    /// neither `HoveredFile` nor a position while it is in flight. So the
    /// external drop keeps the industry's convention — the pane under the cursor,
    /// whatever kind it is, with no zone inside it — and the two roads meet only
    /// at [`Self::paste_paths_into`], which is where they should.
    ///
    /// **Where the point comes from, given that winit throws it away.** winit
    /// 0.30 reports a drop as a path and nothing else: the Windows backend is
    /// handed `POINTL` in `IDropTarget::Drop` and discards it, and the macOS
    /// backend never reads the dragging location out of
    /// `performDragOperation:`. Neither platform sends a pointer event while
    /// another application's drag is over the window either, so a drag that
    /// began in Explorer or the Finder arrives at a window whose pointer has
    /// already left it and `pointer_position` is `None` — which is *most*
    /// drops. [`Self::platform_pointer_now`] is what closes that: the cursor is
    /// asked of the platform, once, **as the drop arrives** (release review
    /// 0.4.2 X-10), and travels here inside the batch. The keyboard's pane is
    /// what is left when even that answers nothing, which is a window on a
    /// session with no desktop to read.
    pub(crate) fn dropped_files_seat(
        &mut self,
        position: Option<PhysicalPosition<f64>>,
    ) -> Option<SeatId> {
        let covered = position.is_some_and(|position| {
            matches!(
                self.pointer_target_at(position),
                Some(PointerTarget::Float(..))
            ) || self.panel_covers(position)
        });
        dropped_files_seat_at(&self.seat_layout, position, covered)
    }
}
