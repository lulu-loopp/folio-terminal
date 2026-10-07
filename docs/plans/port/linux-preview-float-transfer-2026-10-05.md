# Linux floated preview tab transfer

This fixes the blank preview observed when a tab containing a floated preview moves to another native window.

## Ownership

DESIGN §7.1.3 keeps preview buffers and their view state on the owning tab. A floated preview is still a `PreviewSurface::Float(id)` in that tab; the `FloatHost` and its `FloatId` belong to one window. Therefore a tab transfer must move only the live preview floats whose `FloatPreview.tab` names that tab, assign IDs in the receiving host, and re-key the tab's preview and graph view entries. A page carried by such a float is a `LeafId` outside the pane tree, so it must join the same `WebSeat::rehost` transaction as the tab's docked pages before the source window retires.

## Regression and result

[`scripts/ci/linux-preview-float-transfer-smoke.py`](../../../scripts/ci/linux-preview-float-transfer-smoke.py) runs in a private Xorg display, with a private XDG profile and a test shell. It opens a Markdown preview, floats it, writes `C=kept` in the tab's shell, creates a second native window, then moves the tab through its context menu. The red check records the chrome trace size after the source window closes and requires a newly drawn `DockRight` control in the remaining target window; it also checks the shell cookie in the original PTY recording. It verifies the two live shell wrappers still have the Folio PID as PPID, distinct PTYs, and one live Bash child each.

Run it with a built Folio executable and the local Xorg/Openbox tools:

```sh
python3 scripts/ci/linux-preview-float-transfer-smoke.py \
  --exe /path/to/folio \
  --xorg /path/to/Xorg \
  --modulepath /path/to/xorg-module-overlay \
  --xdotool /path/to/xdotool \
  --openbox /path/to/openbox \
  --openbox-config /path/to/openbox/rc.xml \
  --library-dir /path/to/desktop-tools/lib64 \
  --xdg-data-dirs /path/to/desktop-tools/share:/usr/local/share:/usr/share \
  --xdg-config-dirs /path/to/desktop-tools/etc/xdg \
  --artifacts target/linux-float-transfer
```

On baseline `56bbff3c`, the shell cookie survived, but no target-window frame drew the float. The before and after captures are [the live source float](linux-preview-float-transfer-evidence/before-float-tab-transfer.png) and [the blank destination](linux-preview-float-transfer-evidence/after-float-tab-transfer-red.png). The test failed at `the moved tab kept its shell cookie but no frame after source close drew the floated preview or Dock control`.

The fix now moves the preview float and any live page before the source window closes. It assigns a target-host ID, scales the physical frame through logical coordinates, clamps it to the target viewport, re-keys the tab's `PreviewPane` and `GraphView`, carries a playing video seat, and transfers a page's keyboard receipt with its `LeafId`. It clears old-window pointer gestures and re-keys the preview edit focus.

The same private-Xorg command now passes. The [destination capture](linux-preview-float-transfer-evidence/after-float-tab-transfer-green.png) shows the formula, `DOCK`, and `FLOAT_MOVE_COOKIE=kept` in the moved tab. The source window is gone; the target owns the tab and its float. After clean exit, both shell wrappers and their Bash children were absent.

On the passing run, Folio PID `3425709` still owned both shells after the move:

| Shell wrapper PID | PPID | PGID / SID | TTY | Bash child PID |
| --- | --- | --- | --- | --- |
| `3425896` | `3425709` | `3425896` / `3425896` | `pts/9` | `3425912` |
| `3425954` | `3425709` | `3425954` / `3425954` | `pts/14` | `3425959` |

## Verification

The following focused tests pass:

- `float::tests::a_preview_float_transfers_with_its_tab_and_target_window_geometry`
- `tab_identity_tests::a_preview_float_rekey_keeps_its_tab_owned_view_state`
- `tab_identity_tests::a_tab_transfer_carries_its_floated_preview_surface`

The private-Xorg smoke passed with Folio built from `56bbff3c` plus this working-tree fix; its embedded version string is `Folio 0.4.6 (56bbff3c58)`. The two shell wrappers and Bash children listed above were absent after clean app exit. This original run covers Markdown rendering; the later Chromium/PDF UI results are recorded below.

## Follow-up transfer review

Review found two more transfer hazards. First, a rekey inside `debug_assert!` would disappear in optimized builds. The call now runs unconditionally into `let rekeyed`, and the assertion only observes its result. Second, one-at-a-time ID assignment could map source floats `1, 6` into target host epoch `5` as `6, 7`; the first rekey would evict the second surface's still-live tab view. `FloatHost::transfer_previews_to` validates the pinned set and raises the receiving host's watermark past the maximum source ID before assigning any new ID. The direct regression rekeys two distinct Markdown buffers to target IDs `7` and `8`; it passed as `tab_identity_tests::a_batch_float_transfer_keeps_two_distinct_documents_when_ids_overlap`.

The receiving-set collection explicitly filters pinned floats. `FloatHost::live_windows` also returns its transient peek, but the two production preview-float constructors (`runtime/peek.rs::promote_file_peek` and `runtime/preview.rs::pop_out_preview`) both use `FloatMode::Pinned`; hover peeks use `FloatTenant::Files`.

The `21d8db02a2` PDF run is a historical failure before the WebActor control-target attach and frame-owner fixes. It started Chromium but produced no `navigate` or `navigation_completed` event, so it did not test PDF rendering or transfer. Later WebActor startup and frame-ownership fixes are included in the runtime verified below.

## Native window and preview lifecycle

The tracked [`linux-wayland-web-preview-smoke.py`](../../../scripts/ci/linux-wayland-web-preview-smoke.py) harness runs Folio in a nested private Niri session hosted by a private Xorg display. It owns separate HOME/XDG directories, Wayland and Niri sockets, Chromium profile, shell, and fixture. It captures the full Niri output with `grim`, then crops the current WebActor bounds before checking the canary. Its cancellation mode serves a loopback document with headers sent and the body held until after `PaneClose`.

Folio `0.4.6 (8c84e8a3eb)` with Chromium 154 passed these native Wayland runs:

- [HTML after float, cross-window tab move, and DockRight](../../../target/linux-port-team/preview-final-evidence/native-wayland/html-dock/preview-tab-docked.png). Both source and receiving PTY shells remained Folio-owned on distinct TTYs, the shell cookie survived, and the docked HTML canary stayed visible.
- [PDF in its native Wayland pane](../../../target/linux-port-team/preview-final-evidence/native-wayland/pdf-dock/pdf-in-pane.png), [after float](../../../target/linux-port-team/preview-final-evidence/native-wayland/pdf-dock/preview-float.png), [after tab move](../../../target/linux-port-team/preview-final-evidence/native-wayland/pdf-dock/preview-tab-in-destination.png), and [after DockRight](../../../target/linux-port-team/preview-final-evidence/native-wayland/pdf-dock/preview-tab-docked.png). The PDF viewer and OOPIF frame-owner trace reached accepted screencast frames; the page stayed visible through every rehost.
- [Held HTTP response before cancellation](../../../target/linux-port-team/preview-final-evidence/native-wayland/midload-cancel/cancel-held-response.png) and [after cancellation and response release](../../../target/linux-port-team/preview-final-evidence/native-wayland/midload-cancel/cancel-after-response.png). The test waited until Chromium received the response headers and the actor logged a paused main-document fetch, then clicked `PaneClose` before the body was released. The page's post-load fetch never ran, no frame arrived after the hidden placement, and clean quit reaped the private Chromium profile processes and its PTY shell.

Native Xorg also passed HTML and PDF pane → float → cross-window move → DockRight → clean-exit runs. The screenshots for those passes are [HTML DockRight](../../../target/linux-port-team/preview-final-evidence/native-xorg/html-dock/html-after-dock.png) and [PDF DockRight](../../../target/linux-port-team/preview-final-evidence/native-xorg/pdf-dock/pdf-after-dock.png). The destination screenshots retain the moved shell cookie and visible canary.

The Wayland full-screen images and focused trace/log reports came from the private smoke runs stored under `target/linux-port-team/preview-final-evidence` in the integration checkout. These runs make no claim about screenshots from only the child window surface; the checked images include the Niri compositor output.
