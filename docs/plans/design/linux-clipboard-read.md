# Linux clipboard reads — ownership repair design, 2026-10-03

**Status:** implemented for reads and extended to mixed reads/writes by
[`linux-clipboard-write.md`](linux-clipboard-write.md). The write design is
the current ownership contract where these notes differ.

## Scenario and scope

Terminal, field, rename/palette, and preview paste must not wait on an X11 selection owner or a Wayland clipboard source. Each admitted read produces one bounded answer from one source interval or a terminal outcome; the answer is applied only if its original destination instance is still live.

- **Owner:** Folio owner. **Coordinator:** `/root`. The review is recorded at `docs/plans/review/linux-clipboard-read-review-codex-2026-10-03.md`.
- **Baseline:** `5dae85fa63fb119132b123204245606656187306`; candidate: none; worktree: `linux-port`.
- **In:** Linux clipboard reads and their app paste callers. **Out:** clipboard writes, Windows/macOS read contracts, paste policy, picture encoding. Reads become asynchronous, changing daily gestures.
- **Owner-approved budget:** eight waiting requests; four seconds from admission for the whole operation; URI-list 8 MiB; text 8 MiB; PNG 256 MiB; at most 4,096 parsed local files. Destination identity is captured at the gesture; clipboard source snapshot is taken when the worker begins the read.

## Observed fact and authority

Static inspection: `paste_from_clipboard_into`, `clipboard_line`, `paste_into_field`, and `paste_into_preview` read synchronously. Linux `clipboard_payload` holds the process-wide Arboard mutex during raw reads. File URI and text transfers are unbounded; X11 INCR appends without a cap. X11 does not validate selection ownership across MIME requests. Wayland MIME enumeration and `get_contents` select offers independently. Linux `begin`/`finish` do not pin a source.

The native X11 owner plus XFixes selection generation and an optional selection `TIMESTAMP`, or one Wayland offer on one seat, is authoritative for source bytes. The live runtime destination instance is authoritative for application. In the original synchronous path, `live_paste_target` checks the terminal target only; fields, rename/palette and preview need completion-time instance checks.

## Fact ownership (CONVENTIONS §十 rule 11)

| | Current | Proposed |
|---|---|---|
| Fact and owner | Window runtime reads and consumes one gesture synchronously; `linux_clipboard` owns request details but no stable source identity | One process-wide `ClipboardLane` owns mixed read/write order and request lifecycle; the window runtime alone owns destination acceptance and delivery |
| Writers / readers | External selection owner and Folio `set_clipboard_text` / Linux raw reader for URI/PNG, Arboard for text, runtime for consumption | X11/Wayland owner candidates serve writes and `LinuxClipboardReader` reads one pinned source interval; the window runtime consumes tagged results |
| Invalidators | Source replacement between rungs is unchecked; synchronous call prevents destination changes until it returns | X11 owner/timestamp mismatch, pinned Wayland offer failure, transfer failure/over-cap, deadline, cancellation, or stale target rejects the result |
| Retirement | X11 requestor is destroyed on drop; Wayland reader drops; Arboard context lasts to process exit; Linux `finish` is a no-op | Every success/error/cancel/timeout releases requestor/offer/pipe before terminal publication; shutdown cancels, drains and joins before dropping the retained write owner |

## Process-wide lane and lifecycle

The original read implementation used one `ClipboardReadOwner`; the accepted
write extension replaces it with one `ClipboardLane` per Folio process (not per
`Runtime`/window). Its mixed FIFO has **eight waiting** operations, one active
operation, and one result slot. Read requests carry an id, `TextOnly` or
`Payload`, `ClipboardTargetToken`, cancellation state, and an absolute
deadline. Write requests own the immutable text snapshot captured at admission
and an effect token. After publishing any result, the worker waits for the
window to handle or discard it and acknowledge the exact id before starting
the next operation. A queued request whose deadline expires is terminally
timed out without opening the clipboard; the queue records that status for the
next app drain, without retaining payload bytes.

At read gesture time the window snapshots the request id and destination token. When the worker begins, `LinuxClipboardReader` snapshots one source interval: X11 owner plus XFixes selection generation and an optional `TIMESTAMP`, or one Wayland offer and seat. All fallback rungs use that exact source; a later Wayland offer is never spliced into the transaction. If the pinned offer becomes unusable, return `Unreadable`. At copy gesture time the window instead moves the selected text into the write request, so later UI changes cannot alter the copy. The four-second deadline starts at admission, so queue delay counts; it covers setup, transfer/ownership confirmation and publication. Publish before waking the app event loop. On shutdown, close admission, cancel queued and active work, discard a held result, wake and join the lane, then retire the retained native write candidates.

Cancellation and stale-result rejection are separate. When a destination instance retires, the window-side owner marks matching queued reads canceled and sets the active read's cancellation flag; the transport polls that flag and closes descriptors. The request then has a terminal `Canceled` outcome. If completion won the race and a result is already held, the window still validates `ClipboardTargetToken` and drops a stale result silently, then acknowledges it. A clipboard source change during transfer is `Unreadable`, not destination cancellation. Writes are not canceled by a later copy or read. Their X11/Wayland candidates own independent retained serving resources; uncertain claims remain with the previous resource until same-connection reconciliation.

## Source and MIME semantics

X11 uses one requestor/property transaction. Subscribe to XFixes selection changes before acquiring the source, synchronize with the server, and drain tracking events to establish the source boundary. Every later matching event invalidates the result, including a replacement by the same owner window. Use a final server barrier and drain before publishing. Validate `TIMESTAMP` too when the owner provides it; Arboard 3.6.1 does not provide this target. If neither XFixes tracking nor `TIMESTAMP` is available, return `Unreadable`. The protocol is specified in [XFixes section 6](https://xorg.freedesktop.org/archive/current/doc/fixesproto/fixesproto.txt).

For text, request `UTF8_STRING`, then `TEXT`, then `STRING`. `TEXT` is a conversion request, not an encoding: inspect the returned property type. Decode returned `UTF8_STRING` as strict UTF-8 and returned `STRING` as ISO-8859-1/Latin-1; a direct `STRING` request is also Latin-1. A returned `TEXT` type, `COMPOUND_TEXT` (until implemented), or any other unsupported type is `Unreadable`. Wayland text preference is `text/plain;charset=utf-8`, then `text/plain`; validate the returned MIME and decode as UTF-8. File URI-list, text, and PNG fallback all remain within the same X11 interval or Wayland offer/seat.

Use a deadline-aware x11rb stream for setup, queries, property chunks and flushes. Native Unix sockets and literal-IP TCP connections share the absolute deadline and cancellation flag. Remote hostnames are refused because synchronous DNS cannot meet this contract.

The Wayland API question: adding ordered MIME priority to one `wl-clipboard-rs::get_contents` call can reuse its internal `get_offer` and avoids copying its offer/event-queue code. It chooses one advertised MIME, though, while Folio must parse URI bytes and may fall through from a valid empty/non-file list to text or PNG. Prefer a small public offer/session handle backed by existing `get_offer`, with bounded reads for successive rungs against that same offer; use a one-call priority helper only when one MIME is sufficient.

Shared rung outcomes stay precise: an unoffered MIME, or a valid URI-list containing no local files, is `Absent` and permits fallback. Empty text is `Absent`. Invalid encoding/format, malformed URI-list, X11 owner/timestamp mismatch, loss of the pinned Wayland offer, transport error, truncation, timeout, more than 4,096 local files, or a rung over its approved byte cap is `Unreadable` and stops fallback; do not truncate. Read X11 `TARGETS` in 64 KiB working chunks and retain supported atoms. This is a streaming buffer, not a limit on the whole advertised-type list. Do not translate errors or over-cap reads into `Absent`.

## Destination adoption, doors, and debt

`ClipboardTargetToken` carries a `WindowInstanceToken` plus a destination-instance token: terminal uses existing `PasteTarget { tab, seat, incarnation }`; field/rename/palette and preview use the concrete editor/field or preview-document instance, not just a caller enum. On completion re-check the same window and destination, plus `a_surface_above_the_clipboard_rung_holds_the_keyboard`; a new modal or changed keyboard owner drops the answer. Then derive mutable facts from the live object: terminal `paste_recipient` and leading-space state, field caret, and preview EOL. Do not snapshot these at request time. A valid picture continues through the existing `ClipboardPictureMailbox` unchanged.

The app owns queue/result publication and must satisfy §5.1: identity in every outcome, publish before wake, bounded queue/result, FIFO request handling, and terminal outcomes on worker failure. The platform read uses `spawn_at_priority`/`WorkerCtx` through a registered Linux clipboard effect door; `bt-platform` does not send `AppEvent`. The native window backend chooses X11 or Wayland. Update §5.1 (lane), §5.2 (Linux clipboard contract), §5.3/`window_waits.tsv` and `#effects` (worker door), §6 (OS effect), and §7.2 (paste chain).

This partially repays **D-50** (Linux's window-thread clipboard read). The
accepted write extension puts Linux copy in the same process FIFO and makes
the copy effect follow server confirmation; D-50 remains open for Windows/macOS
and cross-platform paste convergence. Existing row 26 remains the Windows
clipboard-open exception. No separate debt item is needed because the lane and
effect inventories are updated with the write implementation.

The review accepts one process-wide reader, concrete destination-instance tokens, streaming X11 `TARGETS`, and a small pinned-offer API in `wl-clipboard-rs` 0.9.4. Validate with controlled transports: X11 owner change, Wayland offer replacement (all fallback stays on the original offer), each MIME/overflow classification, cancellation versus publication, one-result backpressure, queue expiry, shutdown, stale/modal destination drops, and picture-mailbox handoff. Use no clock sleeps as synchronization. Desktop probes must use isolated displays and clipboard owners.
