# Linux trash transaction

**Status:** the coordinator approved the design in `docs/plans/review/linux-trash-review.md`. Application integration is in progress.

## Decision

Linux file-row and scheme deletion SHOULD submit one asynchronous trash transaction to an
application-owned worker lane. The worker performs the existing Linux `gio trash -- <path>`
operation for the captured path and publishes its result before waking the event loop. The event-loop
owner applies settings, listing, palette, and message changes from that result. Windows and macOS
keep their current synchronous recycle path and its native cancellation behavior.

Use a small `TrashLane` rather than `HandoffLane`. `HandoffLane` requires a `NativeWindow`, returns
only `Result<(), String>`, and stores acceptance duty in a window's `Pending`; closing that window
intentionally drops the result. Trash has an application-owned scheme-settings effect, must retain
the `Result<bool, String>` cancellation distinction, and can still affect a tab after that tab moves
to another window. The new lane SHOULD follow `HandoffLane`'s bounded FIFO, unique request ID,
publish-before-wake, and non-blocking admission pattern without sharing its window target.

## Owners and request

`TrashLane` and the pending transaction map belong to `App`. A request carries a unique
`TrashId`, the exact `PathBuf` chosen by the gesture, and one target:

- **File row:** `LeafId`, captured files root, row key, display name, and parent directory.
- **Scheme:** captured user scheme file name and path, plus the requesting window's pending UI
  duty. That duty belongs to the requesting `WindowRuntime` incarnation, not to a reusable
  `WindowId` alone.

`Runtime::delete_files_row` MUST keep its current live-tree row check before admission. It MUST
capture the path from that row and MUST NOT perform the recycle operation itself. `delete_scheme_at`
MUST keep resolving the user file from the catalogue at the press. Neither worker request may be
rebuilt later from a row index, active tab, or current scheme selection.

The lane SHOULD admit at most 32 outstanding requests, counting queued, running, and published but
undrained answers, execute one request at a time in FIFO order, and never coalesce two gestures.
Admission MUST return without waiting. A full or disconnected lane returns a refusal to the caller,
which uses the existing deletion error surface; it does not block the window thread. The worker MUST
enter through `spawn_at_priority` and call the Linux recycle effect with its `WorkerCtx`. The effect
MUST retain the current `openable_unix_path` rule and reuse the existing `gio trash -- <path>`
argument construction and error formatting, with the child-process call moved behind the worker
context. It MUST pass a directory as one path so the desktop trashes and restores the whole folder.
It MUST NOT walk the folder, invoke a shell, or fall back to permanent deletion.

The completion is the existing recycle result:

| Result | Meaning |
| --- | --- |
| `Ok(true)` | The desktop accepted the path into Trash. |
| `Ok(false)` | The user cancelled. Make no local change and show no message. Linux `gio trash` does not currently return this result, but the transaction keeps the shared API distinction. |
| `Err(reason)` | The recycle operation or lane refused. Apply the existing `not_deleted` error surface once. |

The lane publishes the completion under `TrashId` before sending `AppEvent::TrashAnswered` (or an
equivalent existing loop wake). `FolioApp` drains the shared answer channel once and routes each
answer to its transaction owner. A request without a terminal result MUST remain observable as a
lane failure; no window may poll or wait for it.

## File-row completion

On `Ok(true)`, the completion owner finds the tab by `LeafId.tab`, then checks that the addressed
seat still exists and its files root still equals the captured root. If it does, it asks the existing
files worker to re-read the captured path's parent for that `LeafId`, clears selection only when it
still equals the captured key, marks the session dirty, and refreshes that window. The re-read MUST
be addressed to the captured `LeafId`, not whichever tab is active when the answer arrives.

An error is shown in that files column only while its tab and seat still exist. A closed seat or tab
has no UI recipient. A transferred tab keeps its `TabId`, so its completion follows it into the
current owner window. Closing the originating window therefore retires only window-specific UI
duty; it does not cancel an admitted filesystem operation while the application remains alive.

The filesystem operation remains path-based, as it is today. A same-path edit while queued is part
of the path the user asked to recycle; the worker uses the captured path and `gio` acts on the path
when the request reaches the head of the lane. This design adds no inode, timestamp, or content
precondition. On success the current listing is re-read, so a newly created entry at that path is
shown from the filesystem rather than removed from UI state by a stale row snapshot.

## Scheme completion and watcher ordering

The worker never reads or writes `SettingsStore`, changes a palette, rescans the catalogue, or raises
a toast. Those remain event-loop-owner work.

While one or more local scheme-trash requests are pending, `SchemeWatch` MUST retain one
`schemes_rescan_owed` bit instead of running `reread_schemes`. An accepted request sets this bit,
and a watcher event sets it as usual. This keeps the selected-file fallback verdict from
running ahead of the local transaction and also coalesces watcher news without polling. Unrelated
scheme-file changes are read when the outstanding local scheme requests settle.

For each `Ok(true)` scheme result, the owner:

1. Reads the current stored light and dark scheme names and the current `scheme_source` values.
2. Falls back to the default only for a row whose current stored name still matches its recorded
   source name and whose source file is the request's file. A later user selection of another
   scheme is left alone. The write goes through the App-owned `SettingsStore` and existing scheme
   application path.
3. Raises one success card for this request in the original window if that window incarnation is
   still open. If it has retired, the settings effect still lands and no card is routed elsewhere.

The request also owes a catalogue rescan. After the last pending scheme-trash request completes,
the owner rescans and refreshes `scheme_source` before processing any deferred watcher verdict.
Because a successful local deletion has already cleared the matching stored name, this rescan
cannot raise the watcher’s duplicate “scheme gone” fallback card. A failed or cancelled request
does not change settings; the deferred rescan then applies the ordinary external-change verdict.
Multiple requests for one file remain separate FIFO transactions and each gets its own terminal
result; they are not merged by file name.

## Retirement and exit

Closing a source window drops only its presentation duty. The App-level transaction still settles
while Folio remains alive, including a successful scheme fallback. The file target is resolved by
`LeafId` at completion; a retired tab or seat receives no row mutation or toast. A scheme success
never raises a card in a different window merely because that window remains open.

Final-window exit and application quit wait asynchronously for admitted trash transactions to settle.
The event loop continues to consume answers. A successful scheme fallback updates settings before
the final settings and session write. Once no trash transaction remains, the existing quit path
continues. After application owners are dropped, the desktop-retirement worker joins the idle trash
lane. The window thread does not join or wait for a trash answer. This decision is recorded in
`docs/plans/review/linux-trash-review.md`.

## Acceptance evidence

- A stale files key is rejected before lane admission; an admitted file request reaches one worker
  call with the captured path, including a directory as a single operand.
- `Ok(false)` leaves listing, selection, settings, and messages unchanged. An effect error raises
  the existing deletion error only in a still-live target. Success refreshes only the live addressed
  tree and clears only its matching selection.
- Tab transfer between submission and completion routes through the tab's new owner. Closing its
  seat or tab drops the UI effect. Closing the requester does not route a scheme card to a sibling
  window.
- A successful selected-scheme delete stores the default before the deferred rescan and produces
  exactly one local success card. A failed delete leaves settings alone, then allows the ordinary
  watcher fallback. A scheme selection changed while queued is not overwritten.
- A same-path content edit while queued is not mistaken for a different row identity; the captured
  path executes once and the receipt re-reads the live tree.
- Admission and answer routing do not call `recv`, `join`, sleep, or `Command::output` on the window
  thread. Worker start, worker exit, full admission, lane disconnection, and pending-request behavior
  have tests through the lane's production adapter.

## Structural impact

- **Facts:** `App` owns lane identity and pending transactions; `SettingsStore`, `scheme_source`,
  and scheme palette remain App-owned; each files tree remains owned by its `LeafId`'s tab; each
  window owns only its presentation duty.
- **Effects:** Linux `gio trash` stays behind `bt_platform::linux_files`; its `Child::wait`
  runs only on the transaction worker through the worker context. Result delivery stays on the app
  event-loop owner. The new worker and any effect door must be listed in the execution registry.
- **Structure:** this adds one bounded transaction lane and one completion route. It does not move
  settings ownership, change Windows/macOS recycling, or create a general storage-lane abstraction.
