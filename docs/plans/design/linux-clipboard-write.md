# Linux clipboard write ownership repair

**Status:** accepted for implementation. The owner approved eight waiting operations, one four-second
deadline from admission through result publication, FIFO with no coalescing,
and no write byte cap. Existing read limits and the three-second outer quit
budget stay unchanged.

## Observed fact and decision

Linux copy gestures call `bt_platform::set_clipboard_text` on the window
thread. Wayland's existing owned-copy startup blocks on `ready.recv()` while
the owner thread connects and prepares a registry. `PreparedCopy` is `!Send`, so it
must stay on the thread that prepared the native connection. Its current
`flush_for_owner` only calls `queue.flush`; it does not prove the compositor
processed the selection claim.

X11 uses Arboard 3.6.1. `XContext::new` calls unbounded
`RustConnection::connect(None)`; `Inner::write` stores bytes, sends
`SetSelectionOwner`, flushes, and returns without a server reply. `Clipboard::drop`
can wait up to 100 ms for clipboard-manager handover, then destroys the owner
window, flushes, and joins the `wait_for_event` serving thread without a bound.
The existing first-party X11 read transport already has nonblocking display
connection, `DeadlineStream`, cancellation checks and deadline-bounded X11
replies. Moving Arboard's current call to an ordinary worker would leave these
startup, success, and retirement gaps.

Extend the existing process `ClipboardReadOwner` into one mixed Linux
`ClipboardLane`. One FIFO makes read→copy→read and Copy→Paste follow admission
order without a second order or a cross-lane barrier. Keep paste policy and
destination adoption in `bt-app`; keep X11 ICCCM/INCR and Wayland serving in
their existing native libraries.

## Owners and operation contract

| Fact | Current owner and paths | Proposed owner and paths |
|---|---|---|
| Linux operation order, admission, deadline, cancellation and outcome | `ClipboardReadOwner` orders reads; copy callers write separately on the window | One process `ClipboardLane` owns the mixed FIFO, eight waiting slots, one active operation, one held result and one absolute deadline per admission. |
| Text intended by a copy gesture | The window reads the source at the synchronous call | At admission the originating action moves one immutable `String` snapshot into its `WriteRequest`. The worker consumes that snapshot; it never re-reads a later selection. Do not make a second full queued copy. |
| Retained native writer resources | `linux_clipboard::WRITE_CLIPBOARD` retains Arboard's cached `Clipboard` or Wayland `CopyHandle` | The platform owner keeps an independent X11 owner candidate or Wayland `CopyHandle`, plus the previous owner only while a candidate is unresolved or being retired. The X server/compositor remains authoritative for the actual selection. Retirement joins each native worker after the lane is joined. |
| Paste destination and copy feedback | `Runtime` owns destination validity and visible selection/confirmation effects | Still `Runtime`. Read results keep `ClipboardTargetToken`; write outcomes carry request id and the originating effect token. The window alone adopts outcomes. |

Every accepted read or write retains its FIFO position and receives exactly
one terminal outcome; never coalesce or discard a command because another
result is available. The held-result slot stops the worker until the window
handles the earlier outcome and acknowledges its exact request id. The
acknowledgement occurs for success, native error, stale destination, missing
window, invalid effect target, and returned application error; a wrong or
duplicate id cannot advance the lane. Thus an earlier live Paste is adopted before a
later Copy starts, and a later Paste starts only after that Copy's
server-confirmed outcome. Keep existing destination-staleness cancellation
for reads and shutdown cancellation. A later copy does not invalidate an
earlier read.

Move the existing eight waiting slots across reads and writes; do not add a
second queue. The single deadline is `admitted_at + 4 s` and includes queue
delay, native startup/ownership confirmation and publication. Expired queued
work is answered without entering the backend. Admission refusal is immediate;
native refusal, timeout and cancellation are returned as write failures, not
as successful copies. Preserve read text/URI 8 MiB, PNG 256 MiB and 4,096-file
limits. Add no text-size limit to writes.

## Worker entrypoint and native acknowledgement

Route all Linux copy callers through one worker-only platform entrypoint,
`set_clipboard_text_on_worker(WorkerCtx, backend, owned_text, deadline,
cancelled)`. That is the only Linux app write entrance. The existing
synchronous `set_clipboard_text` remains for non-Linux platform paths and is
not callable from Linux runtime copy gestures. Keep the `WRITE_CLIPBOARD`
mutex out of native setup, server waits and joins.

**X11:** reuse the first-party nonblocking X11 connector and its
`DeadlineStream` for the Arboard-owned connection; factor the current control
and stream into one reusable Linux clipboard transport seam, rather than
adding another X11 connector. Give Arboard an independent candidate `Inner`,
outside its process-global cached `Inner`, that accepts the generic X11
connection/control, while retaining its existing
`Inner::write`, `handle_selection_request`, `TARGETS`, `UTF8_STRING` and INCR
logic. The candidate owns the admitted text before claiming selection. It
sends `SetSelectionOwner`, flushes, then waits on the same connection for
`GetSelectionOwner`; success requires the reply to name the candidate window
and arrive before the operation deadline. This reply is the server-ordering
barrier before the lane publishes success or starts a following read. Only
after confirmation does `WRITE_CLIPBOARD` swap to the candidate; failed
pre-claim setup leaves the old retained owner untouched. Candidate and old
owner serving threads remain joinable throughout replacement.

Keep the native connection on its serving worker, with that worker's own
`WorkerCtx` alive; no borrowed worker capability crosses threads. The
candidate owns immutable text and only sends a ready/cancel handle across the
boundary. The controlled X11 owner has three modes on that same transport: admitted
setup/write (the request's deadline and cancellation), serving (no request
deadline, owner-lifecycle cancellation only), and retirement (the existing
retirement cutoff). The existing 10 ms cancellation poll in the first-party
transport lets serving stop without waiting for an X event. Do not put the
four-second request deadline on a successfully retained owner.

**Wayland:** use `copy_multi_owned_until` with the request deadline and a
platform-supplied priority-thread spawner. It returns a joinable handle without
waiting for setup; `PreparedCopy` and its connection stay on that spawned
serving thread. The thread uses the shared nonblocking connector and cancellable
registry/seat roundtrips. After claim flush, it sends a cancellable
`wl_display.sync` callback and reports success only after that callback; flush
alone is not success. Reuse the existing `roundtrip_cancellable` loop from the
vendor common path. An unresolved claim keeps its callback and candidate alive.
`reconcile_until` sends a later sync barrier on the same connection without
reclaiming selection. A confirmed candidate replaces the current owner; the
previous owner is retired within the operation cutoff, or retained joinably for
later retirement if the cutoff expires. Failure before claim keeps the previous
owner.

For both backends, distinguish three outcomes: `FailedBeforeClaim` keeps the
previous retained owner; `Confirmed` makes the candidate the retained owner;
`Unconfirmed` reports failure and keeps the candidate plus previous resources
needed for service or retirement. After a claim is sent, timeout or transport
failure cannot guarantee that the old OS selection remained unchanged. The OS
owns the actual selection; the process only owns retained serving resources.
Never publish success for an unconfirmed candidate, and keep at most one
unresolved candidate. Before any later read or write, reconcile that candidate
on its original connection, after its claim request. This server barrier keeps
Copy→Paste ordered across connections. If reconciliation confirms the
candidate, retire the previous resource; if it confirms another owner, retire
the candidate and keep the prior resource joinable as needed. If confirmation
remains unavailable, fail the operation without opening a later read or
sending another claim. Do not attempt an unconditional rollback or re-claim,
since another application may now own the selection. Serving resources use
lifecycle cancellation rather than the four-second request deadline. Process
retirement cancels the lane first, passes the application's single absolute
three-second cutoff through lane and owner reaping, and joins each finished
worker. If a worker has not finished by that cutoff, return the existing
continue-shutdown error and keep its join handle in the process owner book; do
not invoke a blocking destructor or start a second timer. Request cancellation
during replacement cleanup stops that wait and leaves the owner joinable for
desktop retirement.

For either backend, publish one typed result before waking
`AppEvent::LinuxClipboardReady`. Report asynchronous errors through the
existing recoverable clipboard path. Delay visible effects until the result:
do not clear terminal selection at admission, and do not show formula-copy or
git-copy success feedback before native ownership is confirmed. Clear a
terminal selection only if its original window/session and expected
`ViewSelection` are still current; do not compare copied text alone.

## Retirement, doors, debt and proof

Provide an explicit bounded owner-retirement entrypoint; do not rely on
Arboard's current unbounded `Drop`. It first tries clipboard-manager handover
for at most Arboard's existing 100 ms and never past the caller's cutoff,
then cancels the serving loop, destroys/flushed the owner window through the
controlled transport, and joins the serving thread. Every handler/transport
wait the thread can enter must observe the same cancellation/cutoff so the
join is bounded. The lane is canceled and reaped under the same cutoff; then
`release_clipboard_on_worker` retires retained X11 or Wayland owners with that
unchanged cutoff. Copy replacement uses the copy's remaining four-second
deadline and observes request cancellation while retiring a previous owner.
If desktop retirement reaches its cutoff, unfinished join handles remain in
the process owner book for the existing quick-exit path. Do not add a timer or
detached cleanup helper.

The window does bounded admission and result adoption only. Keep
`Station::ClipboardWrite` around admission. Register the worker write door and
lane worker effects in the clipboard rows of `docs/ARCHITECTURE.md` and
`crates/bt-app/src/window_waits.tsv`; update the clipboard chain and the Linux
partial-repayment text under D-50. D-50 remains open for cross-platform paste
convergence. Record the controlled Arboard X11 transport/lifecycle changes in
`vendor/arboard/CHANGES-FOLIO.md`, and the deadline/roundtrip-owned Wayland
copy change in `vendor/wl-clipboard-rs/CHANGES-FOLIO.md`. No new process or
separate structural-debt item is needed.

Permanent tests must prove: mixed FIFO and read→copy→read; fresh Copy→Paste
data only after server acknowledgement; every admitted command gets one
outcome; eight waiting slots and no coalescing; a queued request expires from
its admission deadline; delayed copy failure leaves selection and success
feedback unchanged; a changed selection is not cleared by an older success;
Wayland does not report ready on flush alone and failed startup retains the
old owner; X11's Arboard path confirms the exact owner window through the
first-party timed transport; manager handover, stalled serving, cancellation,
and owner-thread joins finish within the existing three-second retirement
policy. Use barrier-controlled transports and the existing protocol fixtures,
with no sleep as synchronization. Windows and macOS keep their existing
paths.
