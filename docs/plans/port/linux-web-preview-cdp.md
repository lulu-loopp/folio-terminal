# Linux web preview over the private CDP pipe

Issue 14 requires the web seat to run in the existing preview pane, preserve
its navigation and resource rules, and survive cancellation, moves, and close.
Linux uses a headless Chromium child as a rendering engine, not a second
user-facing browser window. Its compositor frames go back into Folio's existing
`VideoLayer` texture path.

## Ownership and interface

`bt-platform::WebHost` stays the external seam. Its Linux adapter owns one page
target; a process-wide actor owns the Chromium child, the CDP pipe, target
sessions, and protocol IDs. One child serves every web seat for the profile, so
cookies and storage are shared and two children never claim the same profile.
The actor worker, not the window thread, starts and reaps the child. The
process-wide worker is retained with `linux_process::register_helper`; close
and desktop retirement send cancellation through its mailbox, and
`shutdown_helpers` joins it after owners have sent their cancellation
requests. Normal host methods enqueue commands and return without waiting for
CDP responses. Startup, navigation, and requests use the existing WebHost
state-machine deadlines; the platform adds no second timeout.

When the existing engine deadline expires, the WebSeat closes its Linux host
through the same actor mailbox used by ordinary retirement. A matching `Close`
or `CancelCreate` interrupts that host's in-flight setup response; a late
`Target.createTarget` reply or attachment is closed even if a newer generation
has already started. Cancellation does not add a second timer or interrupt
another host's setup. The actor keeps only one unresolved canceled page-create
disposition: while another live host needs the browser, a new page-create fails
promptly without sending another target request; when no other live host needs
it, the actor retires the browser epoch before allowing the retry.

The actor retains a protocol reply only while its command is actively awaited.
Late replies to unwound calls and replies to unawaited commands are dropped;
asynchronous worker setup replies remain owned by their setup record. Nested
calls keep each active response id until its own waiter returns.

The platform publishes only the newest immutable `WebFrame` per host:
`PageVisual`, the exact `WebMachine` controller generation, an increasing frame
sequence, capture-time bounds/visibility, pixel dimensions, and BGRA8 bytes in
an `Arc`. The app still owns current layout. `WebHost::take_frame` transfers
the newest frame; a late frame is discarded when its page or generation no
longer matches. The existing `WebPageSpoke` wake reaches
`Runtime::drive_web_page`, which reads the frame and places it through
`VideoLayer { stage: Seat, ... }`.

The browser receives only the existing profile directory at the
`request_environment` door: `$XDG_DATA_HOME/Folio/Chromium`. The Linux path
resolver supplies `$XDG_CACHE_HOME/Folio/Chromium` for the disk cache and a
verified private `$XDG_RUNTIME_DIR/Folio` for Chromium temporary files (with
that resolver's per-user fallback). `WebHost::request_environment` stays
nonblocking: its worker prepares and verifies these directories before
launching the child, then sends `WebEvent::Environment`. The actor passes
`TMPDIR` only to the Chromium child. The unpacked policy extension is under
`$XDG_RUNTIME_DIR/Folio/WebPolicyExtension/<directory_tag>` and is removed after
that Chromium child exits. The tag is the existing instance data-directory tag,
so two Folio data namespaces do not overwrite each other's extension files. It
does not change Folio's environment or kernel instance claim and IPC directory.

## Request policy

The actor enables CDP `Fetch` before it resumes a page target. A paused request
carries `frameId` and `resourceType`. `Page.getFrameTree` supplies the main
frame ID; root `Page.frameNavigated` events replace it after a top-level
navigation. The actor compares IDs directly:

| Request | Folio rule |
| --- | --- |
| `Document` in the current main frame | `WebHost` navigation gate |
| Subframe document or any other resource | `WebHost` resource gate |

The actor pauses the request and posts its URI, frame classification, CDP
request ID, page address, and controller generation to the host. On
`WebHost::drain`, the original app-owned gate closure answers it. `Proceed`
continues that request; `Cancel` fails it as aborted; `CancelAndNavigateTo`
fails it and issues the returned URI in the same page target. The request ID
is consumed once. A close or a superseding generation fails any paused request
belonging to the old page.

The root page attaches to out-of-process iframes, workers, shared workers, and
service workers before resuming them, then enables Fetch in each session.
Fetch gates page requests, OOPIF requests, dedicated worker requests, and
service worker requests. Service workers use page-target auto-attach, so the target's parent
session identifies the WebHost that registered a worker, including Fetch
requests made by its `install` handler before it has controlled clients.
`ServiceWorker.enable` later supplies exact `controlledClients` target IDs;
requests from a worker shared by more than one live page wait for each mapped
WebHost resource gate and continue only if every current client allows them.
The request is refused when there is no live client or registration owner.
`Fetch.requestPaused` is asynchronous: the browser remains paused
until `Fetch.continueRequest`, `Fetch.failRequest`, or a redirect command is
sent back, and the command's result is another protocol response. The window
thread therefore never waits for a callback; it drains the actor's pending
decisions, calls the original app-owned closures with the current `Mint`, and
queues the verdict back to the actor. The install report marks frame
navigation and resource requests available only after the root session and
automatic target attachment have both acknowledged Fetch interception. Script
dialogs are dismissed through `Page.javascriptDialogOpening` before the guard
is reported as available.

Full Chrome 154 does not emit `Fetch.requestPaused` for HTTP fetches made by a
SharedWorker, even after `Fetch.enable` on that worker session. The reachable
SharedWorker source classes were measured instead: remote-origin workers can
only use network requests under `Mint::Nothing` and `Mint::File`, both of which
allow those schemes; a file-origin page cannot create a SharedWorker, and a
remote-origin worker's file fetch fails inside Chromium without a file request.
A `Mint::Blank` `about:blank` document has an opaque origin and its Blob-backed
SharedWorker constructor fails with `SecurityError`. When one client navigates
to that blank page, its port closes; any surviving worker still has the other
live page as a client and that page's mint continues to govern what it can
reach. This is a measured source-class equivalence, not a Fetch gate. DNR still
gates the `WebSocket` class separately.

A newly attached out-of-process iframe begins paused. `Page.getFrameTree`
can wait for that target to resume, so the actor enables its page/runtime/resource
interception and IME hooks while paused, sends `Runtime.runIfWaitingForDebugger`,
then reads and indexes the frame tree. This keeps the resource hook in place
without synchronously waiting on a paused target.

The actor attaches worker targets with the same resource policy. `Fetch.enable`
and `Target.setAutoAttach` are written to the CDP pipe before
`Runtime.runIfWaitingForDebugger`; their response IDs remain tracked while the
actor handles other hosts. Successful acknowledgements complete setup. An error
from either required command closes that worker target and posts `ProcessFailed`
to its still-current owner generation. The actor does not treat an ignored or
unacknowledged setup as a working guard.

The browser target also auto-attaches new page targets with
`waitForDebuggerOnStart`. The actor defers page-attach events during
`Target.createTarget`, then matches the returned target ID to the pending seat
before it configures and resumes that exact page. Duplicate sessions on an
already-owned page are detached. A browser-created page with no pending seat is
closed while paused. For `Page.windowOpen`, a `userGesture=true` event routes
its URL to the current page with `Page.navigate`, so the normal navigation gate
sees it; a false gesture is ignored, and any resulting popup target is still
closed. The Chrome 154 probe observed a paused popup with the root page's exact
`openerId`; routing the trusted URL caused one request in the existing page,
while a script popup caused no request.

Full Chrome's `Fetch` domain does not pause WebSocket handshakes or top-level
non-network navigations such as `about:blank`. The policy extension handles
WebSockets through per-tab Declarative Net Request session rules; it gets the
tab ID by mapping the exact CDP target ID with `chrome.debugger.getTargets()`.
For non-network main-frame navigation, a document-start Navigation API hook
captures browser intrinsics before page scripts run, synchronously cancels the
navigation, and sends the URL through a nonce-authenticated CDP binding to the
same app navigation gate. Its replay permit is one-use. A Chrome 154 hostile
fixture changed the destination getter, `isTrusted` prototype getter,
`preventDefault`, `addEventListener`, `Object.getOwnPropertyDescriptor`,
`Object.prototype.toJSON`, `JSON.stringify`, and `Reflect.apply`, and called a
forged binding; the real `about:blank` click was still canceled and the loaded
root frame stayed in place.

The private control page is opened at a fixed extension ID and checked for its
expected ID and ready promise. If the selected executable does not load the
unpacked MV3 extension, startup fails with that requirement named; an executable
name on `PATH` alone is not treated as proof of extension support. Linux reports
`resource_requests` available only after the policy page is ready, the browser
root has page auto-attach enabled, the page Fetch session is active, and its
per-tab WebSocket rule has been installed.

The built-in PDF viewer is an engine-owned child frame at the exact committed
URL `chrome-extension://mhjfbmdgcfjbbpaeojofohoefgiehjai/index.html`. Chrome
loads its viewer UI assets from `chrome://resources/`, a scheme the shared
`resource_request` gate correctly refuses for page content. Linux continues
those assets only when CDP reports that exact viewer document, its current
session belongs to this host, and its parent is the host's current main frame.
Other `chrome://` requests and the viewer's `file://` content still go through
the normal app gate. The Chrome 154 product-flags probe rendered the checked-in
PDF fixture and recorded the viewer-frame navigation before its resource
requests; the focused actor regression rejects the same request from an
ordinary page frame, a mismatched session, a different parent, or another
`chrome://` path.

## Rendering and input

`Page.startScreencast` sends the live page viewport as PNG frames. The actor
acknowledges each frame, decodes it to BGRA8, increments the host sequence,
stores only the latest immutable frame, and wakes `WebPageSpoke`. Hidden pages
stop screencasting; making them visible starts it again. `set_bounds` and
`set_rasterization_scale` update the CSS viewport and device scale. The
capture-time bounds travel with the frame as metadata, not as layout authority.

Pointer events use CDP `Input.dispatchMouseEvent` after converting the page
seat's physical-pixel coordinates to CSS pixels. Shared `WebKeyEvent` and
`WebImeEvent` values map to CDP keyboard and IME commands. The app sends them
only after its existing shortcut and input-owner ladder selects the page.
Windows and macOS keep native key delivery. The shared
`WebEvent::ImeCursorChanged` carries browser-derived caret geometry to the
existing IME candidate-window owner; the Linux UA-shadow text-node Range probe
measured the actual caret inside a native input at selection offset 3, and a
DOM Range measured a contenteditable caret. The event's page, controller
generation, and rasterization scale allow late geometry to be rejected.

## Process lifecycle and packaged engine

`--remote-debugging-pipe` uses inherited file descriptors 3 and 4; the child
does not open a debugging port. Chromium keeps its sandbox enabled. The actor
closes one target on seat retirement. A move retains that target and document,
then changes its `PageVisual` owner on the actor; queued frames still carrying
the source page key are discarded. The host and app both validate the
controller generation before accepting a late frame or request result. After
the last page closes, the actor sends `Browser.close`, watches `Child::try_wait`
while continuing to read its mailbox, and reaps the process and output readers.
Desktop shutdown sets the actor's existing cancellation flag; that interrupts a
stalled close and uses the shared kill/reap helper on Chromium's owned process
group. `linux_process::shutdown_helpers` joins registered actors during the
existing desktop-retirement sequence. The
profile and XDG cache persist; only the engine's transient runtime directory
is eligible for cleanup, after the child has exited.

The local CFT browser probes use the official Chrome for Testing full Chrome
154.0.8037.92 archive, SHA-256
`ff43322f335e436b2f4dcdfeeec5db032299e335a7e8c1c618b326e100ce8732`, stored
only under the ignored target directory. HeadlessShell is not used because it
does not load the policy extension. No Rust CDP dependency is added:
`serde_json` and PNG decoding reuse workspace dependencies. The shipped
Linux runtime requires a user-provided full Chromium executable with unpacked
MV3 extension support. Folio does not bundle or install the browser. Set
`FOLIO_CHROMIUM_PATH` when it is not available under one of the searched `PATH`
names; each candidate must still load the private policy extension at startup.

## Upstream API references

- [Chrome DevTools Protocol Fetch](https://chromedevtools.github.io/devtools-protocol/tot/Fetch/)
- [Chrome DevTools Protocol Page](https://chromedevtools.github.io/devtools-protocol/tot/Page/)
- [Chrome DevTools Protocol Input](https://chromedevtools.github.io/devtools-protocol/tot/Input/)
- [Chromium remote-debugging-pipe descriptors](https://chromium.googlesource.com/chromium/src/+/main/content/public/browser/devtools_agent_host.h)
- [Chromium user-data and cache paths](https://chromium.googlesource.com/chromium/src/+/HEAD/docs/user_data_dir.md)
- [Chrome for Testing downloads](https://googlechromelabs.github.io/chrome-for-testing/)
- [Chrome Declarative Net Request API](https://developer.chrome.com/docs/extensions/reference/api/declarativeNetRequest)
- [Chrome native messaging](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)

## Folio Xorg acceptance on integration snapshot `8c84e8a3`

The actual Folio UI matrix used the immutable integration commit
`8c84e8a3ebf1d9d7d916caf23e62eeacfdd2a276`, full Chrome for Testing
154.0.8037.92, and private Xorg/XDG/profile directories. This is an engineering
validation snapshot, not a release. The reproducible harness is
[`scripts/ci/linux-web-policy-e2e.py`](../../../scripts/ci/linux-web-policy-e2e.py);
raw logs, screenshots, the tested executable, and private browser/display tools
were local artifacts under `target/linux-port-team/web-final-evidence/`. They
were not committed to the repository.

The Nothing-mint page rendered in the preview pane and loaded its permitted
HTTP redirect/final script, same-page fetch, OOPIF document/script/image,
ServiceWorker install fetch and controlled fetch, SharedWorker network fetch,
and WebSocket handshake. A visible trusted click to `about:blank` was refused
as `BrowserInternalScheme` and left the current page in place. The timer-driven
script popup produced no request; the trusted button reported `isTrusted=true`
and its popup URL was loaded in the existing pane. The page's file and browser
internal destinations did not load.

A File-mint local HTML page loaded its sibling SVG and HTTP script. Its
`file://example.invalid/share/pixel.svg` request was refused as `NetworkPath`.
The local one-page PDF rendered in Chromium's built-in viewer, with the visible
`1/1` page and `FOLIO PDF FLOAT TRANSFER` marker. `chrome://resources/` requests
were permitted only for the exact committed built-in viewer document and its
owned parent frame.

When the browser path did not name a file, Folio displayed the generic “The web
engine did not start” card with the path error. When Chromium could not load the
private unpacked MV3 policy extension, the card named that requirement and
requested a compatible full Chromium build. In both cases the app and its
terminal PTY remained live.

The retained service-worker notes are deliberately scoped: Fetch interception
was observed for service-worker install/runtime requests owned by the page. It
was not observed for SharedWorker network fetches. Chromium refused network
worker construction from opaque `about:blank` and refused a network worker's
file fetch; reachable Nothing/File source classes keep the product's existing
network permission equivalent, as recorded above. The separate CFT probes and
actor tests cover paused child setup, worker-setup failure retirement, browser
popup target ownership, PDF viewer resource ownership, and cancellation.
