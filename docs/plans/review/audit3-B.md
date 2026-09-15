STATUS COMPLETE

1. **B-1 — high — Formula copy leaves a permanently expired event-loop deadline**

   **Location:** `crates/bt-app/src/main.rs:100278` at `6a414963`.

   ```rust
   self.window
       .math_copied
       .as_ref()
       .map(|(_, at)| *at + FOOT_REVEAL_FEEDBACK),
   ```

   **Trigger:** Successfully copy a terminal formula, then leave that window open for more than 1,300 ms. `copy_math_latex` stores `math_copied` at line 85238; no production path clears it.

   **Consequence:** Every subsequent `turn` returns the same expired deadline. `about_to_wait_inner` passes it to `ControlFlow::WaitUntil` at line 109559, so the application repeatedly wakes without an idle wait.

   **Smallest correct fix:** Consume `math_copied` when its deadline expires, refresh the overlay once to remove the checkmark, and return no further deadline for that copy.

2. **B-2 — medium — An oversized Markdown preview cannot refresh its displayed bytes**

   **Location:** `crates/bt-app/src/preview.rs:6096` at `6a414963`.

   ```rust
   if self.content.is_none() {
       self.accept(HeadOutcome::Read {
           text: head.text,
           truncated: true,
   ```

   **Trigger:** Open a Markdown file larger than 8 MiB, then change its first line on disk while keeping the file larger than 8 MiB. Let the disk watcher reload it.

   **Consequence:** The first load fills `content` with the 64 KiB fallback. Every later whole-file read returns a fresh fallback, but this condition discards it because `content` already exists. `accept` also clears `stale` at line 6000. The preview continues showing obsolete bytes after reload. A previously smaller Markdown file that grows beyond the cap has the same problem.

   **Smallest correct fix:** Capture whether this is a clean disk reload before clearing `stale`; install the returned fallback for that case as well as initial loading, updating the content, index, revision, truncation flag and modification time together. Preserve the existing body for an edit-upgrade refusal and keep the revision/incarnation check in `land_read` to protect concurrent edits.

3. **B-3 — medium — Formula controls remain over an unrelated tab**

   **Location:** `crates/bt-app/src/main.rs:84961` at `6a414963`.

   ```rust
   } else {
       // A picture that has not caught up has nothing to say about this
       // band, and §7.1.5p ⑥'s answer to that is unchanged: it says
       // nothing, rather than offering a neighbour. The marks stand where
       // they are until a picture that knows the band arrives.
       false
   };
   ```

   **Trigger:** Hover a formula in tab A until its controls are fully visible, keep the pointer stationary, and use a keyboard shortcut to activate tab B, which contains no formula. Start with no search or popup open.

   **Consequence:** `activate_tab` changes the active tab without clearing the window-owned `math_hover_anchor` or `math_tools` (lines 38463–38542). The hover deadline is still `None`. Placement lookup now searches tab B's sessions and returns `None`, so this branch retains the previous follow state indefinitely. `formula_tool_layers` continues drawing its cached boxes (lines 85128–85150), leaving tab A's source/copy controls over tab B until another pointer event clears the hover.

   **Smallest correct fix:** Before changing the active tab, clear the old session's formula hover and the window's formula anchor, clear deadline, pressed state and follow state. The new tab can then establish its own hover from its own content.

4. **B-4 — medium — Mac Quit stops working when the last window closes**

   **Location:** `crates/bt-app/src/menubar.rs:548` at `6a414963`.

   ```rust
   enabled: focus.is_some_and(|focus| binding.is_some_and(|row| row.scope.holds(focus))),
   ```

   **Trigger:** On macOS, close the last Folio window, leaving the application running, then select Quit Folio or press Command-Q.

   **Consequence:** Quit is a `VerbNamed("quit", ...)` row (line 137), so the absent window focus disables it. The native menu disables automatic enabling, leaving both the row and its key equivalent unavailable. Even enabling this row alone would not fix it: `answer_a_menu_row` returns before decoding the choice when `frontmost_window()` is absent (`main.rs:107969`). The running application cannot be quit through its application menu or normal quit accelerator in this state.

   **Smallest correct fix:** Enable Quit independently of window focus and dispatch it before looking up a window. Set the existing application-level `quit_requested` flag so the normal quit transaction runs, including when there are zero windows.
