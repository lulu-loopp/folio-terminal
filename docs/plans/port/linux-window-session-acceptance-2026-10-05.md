# Linux window, session, and preview acceptance

This records native UI acceptance in a private Xorg session. The tested executable logged `Folio 0.4.6 (9af334ab2f)`. Its launch, restore, Markdown, and image paths were live; it predates the Linux WebActor integration, so it does not establish Chromium or PDF behavior.

The tracked launcher and restore check is [`scripts/ci/linux-window-session-smoke.py`](../../../scripts/ci/linux-window-session-smoke.py). It starts a private Xorg server with the input driver disabled, a private Openbox process, temporary `HOME` and XDG directories, and its own runtime directory. It clears inherited display and `NIRI_SOCKET` variables before starting Folio. The test writes process snapshots containing PID, PPID, process group, session, TTY, and state; shell birth records include the owning app PID and TTY.

Run it with an app build and the local Xorg/Openbox tools:

```sh
python3 scripts/ci/linux-window-session-smoke.py \
  --exe /path/to/folio \
  --xorg /path/to/Xorg \
  --modulepath /path/to/xorg-module-overlay \
  --xdotool /path/to/xdotool \
  --openbox /path/to/openbox \
  --openbox-config /path/to/openbox/rc.xml \
  --library-dir /path/to/desktop-tools/lib64 \
  --xdg-data-dirs /path/to/desktop-tools/share:/usr/local/share:/usr/share \
  --xdg-config-dirs /path/to/desktop-tools/etc/xdg \
  --artifacts target/linux-window-session-smoke
```

## Launch, shell ownership, and restore

The same-profile `folio --new-window --cwd <folder>` control exited through the launch handover. The original PID retained exactly two native windows. The separate `folio --new-window fixture.md` path opened its own resident app process and native window; it did not change the primary window count. Closing that document window exited the secondary process and removed its shell process.

On the passing run, primary PID `3292045` owned these two live shell wrappers after handover and after the document window closed:

| Shell PID | PPID | PGID / SID | TTY | State |
| --- | --- | --- | --- | --- |
| `3292260` | `3292045` | `3292260` / `3292260` | `pts/9` | `Ss` |
| `3292302` | `3292045` | `3292302` / `3292302` | `pts/15` | `Ss` |

There were three cumulative primary PTY births. PID `3292293` was recorded with PPID `3292045` and TTY `pts/14`, but was already absent from the first post-handover process snapshot and remained absent after the document closed. The process snapshots therefore show two live primary shells, not three. The independent document process PID `3292312` owned shell PID `3292515` on `pts/14`; that PID was absent after its window closed. The same two primary shell PIDs remained live.

The primary clean-exit path wrote a `session.json` with two windows. On the next cold launch the UI trace contained both `Reopen your other tabs?` and the focused `Restore` action. Pressing Enter on that visible confirmation recreated two native windows and two PTY-ready shells. The card and action are shown in [the restore confirmation](linux-window-session-evidence/restore-confirm-question.png).

The original 10-second timeout used for a document CLI launch was not a valid success condition: an independent GUI launch stays alive while its window is open. The test now observes the primary and secondary windows and their shell processes, then closes the secondary window through its caption button.

## Unsaved Markdown close choices

Editing a Markdown preview changed the rendered buffer and showed its dirty marker while the disk file retained its original bytes. The close card displayed `Save all`, `Discard all`, and `Cancel` ([choice card](linux-window-session-evidence/dirty-close-choices.png)).

- Pressing Escape canceled the close. The app stayed open, the editor remained dirty, and the file bytes were unchanged ([after cancel](linux-window-session-evidence/dirty-cancelled.png)).
- Choosing `Discard all` closed the app with exit code 0 and left the fixture bytes exactly `b'# Dirty close probe\\n\\nThis line remains on disk until a save decision.\\n'`.
- In a separate fixture, choosing `Save all` closed the app with exit code 0 and wrote exactly `b'# Dirty save probe\\n\\nKeep this line for the save decisio\\nDIRTY_CLOSE_MARKERn.\\n'`. The close card is shown in [the save run](linux-window-session-evidence/dirty-save-choices.png).

The marker splits the word at the insertion point; the check compared the complete saved bytes rather than searching only for the marker.

## Floating previews and tab moves

The real UI showed a file tree peek on hover, dismissed it after the pointer left, and kept it visible after clicking the same trigger to pin it. See [open](linux-window-session-evidence/file-peek-open.png), [dismissed](linux-window-session-evidence/file-peek-dismissed.png), and [pinned](linux-window-session-evidence/file-peek-pinned.png).

A Markdown preview containing a formula and checker image moved from its pane into a floating surface, docked back, and closed while the terminal remained. See [float](linux-window-session-evidence/preview-popped-out.png), [dock](linux-window-session-evidence/preview-docked.png), and [closed pane](linux-window-session-evidence/preview-closed.png).

Moving a tab with a docked Markdown preview to a second native window succeeded. The source window closed after its last tab moved; the destination showed the preview and the original shell still printed `P=kept` ([destination](linux-window-session-evidence/preview-tab-in-destination.png)).

The additional floated-preview transfer check exposed a gap in the original build. After popping the Markdown preview out, moving its tab to another native window preserved the tab's shell (`C=kept`) but the destination showed a blank preview area after the source window closed. The screenshots capture the [baseline live float](linux-window-session-evidence/float-before-tab-transfer.png) and the [baseline blank destination](linux-window-session-evidence/float-after-tab-transfer.png). The red run used `56bbff3c`; its shell cookie survived, but no destination frame drew the floated preview after source close.

The transfer fix is a separate change in [PR #27](https://github.com/lulu-loopp/folio-terminal/pull/27). `FolioApp::transfer_tab` carries each live pinned preview and its page, rehosts the page before the source window retires, rekeys tab-owned preview and graph state, and converts/clamps the float frame for the receiving window's scale and viewport. The transfer rekeys the complete batch after the source host's largest `FloatId`, so target IDs cannot collide with source surfaces that have not been rekeyed yet. The rekey call executes in release builds; the debug assertion checks its result afterward. The separate [transfer report](https://github.com/Frank0415/folio-terminal-linux-port/blob/d75b817e01e44b5a385f216090f7daca23e1dcc4/docs/plans/port/linux-preview-float-transfer-2026-10-05.md) includes the retained red/green captures. Its destination capture shows the formula, `DOCK`, and `FLOAT_MOVE_COOKIE=kept` after the source window closes.

The private-Xorg smoke passed on a binary built from `56bbff3c` plus the initial transfer fix, and passed again on Folio `0.4.6 (21d8db02a2)` after integration. That run kept the shell cookie, showed `DockRight` in the destination frame, and reaped both Folio-owned shell wrappers and their Bash children after clean quit. These are historical snapshot results. The raw local artifacts were not committed to the repository. A model regression covers two distinct preview buffers with colliding source/target ID ranges (`source 1,6; target epoch 5`); the batch maps them to `7,8` without replacing either buffer.

## WebActor and PDF status

The original screenshots used the pre-WebActor executable named above, so they establish only the native Markdown/image paths. A separate private-Xorg attempt with Folio `0.4.6 (21d8db02a2)`, full Chromium 154, a generated one-page PDF, and a private XDG/TMPDIR profile started the Chromium process but timed out waiting for `navigation_completed success=1`; `web.trace` contained only `place` records and no `navigate` record. The app reported that the web engine did not answer, so that run provides no PDF float, move, close, or resource-reaping result. The subsequent WebActor investigation identified an extension control-target attach race. Later browser verification is documented in [PR #24](https://github.com/lulu-loopp/folio-terminal/pull/24), and the separate transfer report records the later PDF lifecycle runs.
