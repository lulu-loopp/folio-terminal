# Linux clipboard write design review

**Round 3 decision: ACCEPTED for implementation.** The amendment resolves the prior owner-ambiguity HOLD.

It names the originating action as owner of an immutable write snapshot captured at admission, keeps the retained native writer in `bt-platform`, and distinguishes that retained resource from the X server/compositor's actual selection. X11 has an independent candidate `Inner`, a deadline-bounded `SetSelectionOwner` → `GetSelectionOwner` confirmation, and a controlled owner-lifecycle/retirement path. Wayland waits for a cancellable `wl_display.sync` callback before reporting ready. Both backends define `FailedBeforeClaim`, `Confirmed`, and `Unconfirmed` states. An unconfirmed claim reports failure without success feedback and retains the candidate plus previous resources needed for service. It reconciles on the original connection before any later native read or write, so a following Paste observes the OS selection only after the earlier claim is ordered. The design does not automatically reclaim selection from another application.

The operation contract remains consistent with the approved limits: one mixed FIFO, eight waiting operations, one four-second deadline through publication, no write byte cap, existing read caps, and the existing three-second retirement cutoff. It preserves read→copy→read, does not cancel an earlier read when a copy arrives, keeps platform connections on their serving workers with their own `WorkerCtx`, and leaves Windows/macOS paths unchanged.

For implementation acceptance, add a product-level test for each backend where a claim takes effect but its acknowledgement is withheld, then prove the unconfirmed resources are retained and a later write reconciles before claiming. No code or tests were changed or run in this design review.

## Implementation follow-up (2026-10-05)

- The X11 fake-ack fixture now calls `set_clipboard_text_on_worker` for both
  writes. It holds the first owner reply until the first write is reported
  unconfirmed, releases the original and reconciliation replies, then verifies
  the next `SetSelectionOwner` arrives after reconciliation on the first
  connection.
- The Wayland fixture installs the first source while holding server flush,
  observes an unconfirmed result, reconciles on that source's connection, then
  performs a later owned copy and verifies Paste returns the later text. It
  also cancels a retirement wait without dropping the owner and retries an
  expired cutoff with the same join handle.
- The X11 fake-ack fixture retires the current candidate with an already-expired
  cutoff, then verifies the retained owner book can retry and reap it. The
  deadline-stream control test proves caller cancellation does not set owner
  lifecycle cancellation.
- X11 retirement retries a full one-slot command mailbox with the same caller
  cutoff and existing cancellation poll, and reports a disconnected worker
  separately. If a retry reaches an active handover, the owner reports the
  transient state and the caller queues retirement again. The deterministic
  fake-owner test covers a queued canceled retirement, the desktop retry, and
  the serving-thread join.
- The app lane fixture proves read→copy→read order, result-id backpressure,
  queue capacity, expiry, and terminal outcomes for expired and refused writes.
- The lane retirement fixture proves shutdown signals active cancellation before
  checking the absolute cutoff and reports the existing continue-shutdown path
  when a controlled reader has not finished.
