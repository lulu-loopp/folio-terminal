# Linux clipboard read design review

Reviewer: native Luna agent `linux_desktop_helpers`; coordinator: root.
Design: `docs/plans/design/linux-clipboard-read.md`.

The first review found six implementation blockers. The design was revised before
dispatch. The user approved the request and payload limits on 2026-10-03.

| Finding | Decision |
|---|---|
| Unnamed queue, result and transport owners | The process-wide reader owns its queue and one held result. Each request owns one transport interval. The runtime owns destination eligibility. Reads share no Arboard write lock. |
| Destination category is not an instance identity | Capture a live shell target or editor/surface instance. Recheck modal state and identity at adoption. Derive current recipient, caret and EOL from their owners. |
| Idle timeout does not bound a transfer | Carry one absolute deadline through queue wait, setup, fallback and transfer. Poll transport and cancellation. Shutdown cancels and joins the reader. |
| Undefined held-result and overflow behavior | Stop new reads while the held result awaits consumption. Over-limit data is `Unreadable`; absence alone falls through. |
| Undefined X11 text encoding | Prefer UTF-8, inspect the returned `TEXT` type, and decode `STRING` as Latin-1. Do not label arbitrary bytes UTF-8. |
| Undefined snapshot time | The offer is locked when reading starts, as approved by the user. One source identity covers all MIME rungs. |

Approved budget: eight waiting requests; four seconds from admission; 8 MiB each
for URI lists and text; 256 MiB for PNG; 4,096 local files. MIME discovery uses a
64 KiB streaming working buffer and retains supported candidates. This is not an
additional total-format rejection.

Wayland needs an opaque offer/session API around the pinned crate's existing
implementation. A one-shot ordered MIME request cannot preserve fallback after
an empty URI list. The patch keeps one offer, its event queue and cancellation
for the whole transaction.

Source inspection found that Arboard 3.6.1 does not provide X11 `TIMESTAMP`.
Requiring that target would reject Folio's own copied text. The coordinator
accepts XFixes selection tracking before acquisition, an operation-local event
generation, and a final server barrier and event drain. Any later selection
event invalidates the result, including replacement by the same owner window.
An available `TIMESTAMP` is checked too. Missing both mechanisms remains an
unreadable source. A deadline-aware x11rb stream bounds protocol waits without
adding a monitor thread.

Implementation and runtime acceptance remain open. This review approves the
design direction; it does not claim that transfers or application integration
have passed tests.
