# X-2 — WKWebView policy enforcement, probed

**Verdict: PASS.** Every enforcement point the Windows host has maps to a public
WKWebView API, or is written down below as an unsupported guarantee. **20 rows:
12 mapped, 6 mapped with a difference, 2 unsupported guarantees.**

2026-09-12, on the Mac mini, against `origin/main` at `ffb0d3b2`. Worktree
`~/folio-port/wt/x2`, probe crate `probe-x2` (kept at
`docs/plans/port/probe-x2/`), shared target directory, one cargo at a time.
X-1's window shape: a winit window whose content view holds a real `WKWebView`,
on screen, driven by a self-running timeline — nobody was at the machine.

## What was measured, and why the servers are the ground truth

The probe runs **two HTTP servers on 127.0.0.1**, on two ports, so that a
document and its subresources are two origins. Every delegate callback is logged
with its arguments; every socket hit is logged with its `Sec-Fetch-Dest`,
`Referer` and `Origin`. A request that reaches origin B's socket while no
delegate callback ever named it is the gap, in bytes rather than in prose. Three
seats were run in one process: a browsing seat with **no** content rule list, the
same seat **with** a compiled `WKContentRuleList` blocking origin B, and a local
`file:` seat opened with `loadFileURL:allowingReadAccessToURL:` — first bare, then
with a second rule list. The pictures report their own outcome into
`document.title`, which the probe reads back through `WKWebView.title`.

## The requirement-to-API matrix

The requirements are `crates/bt-app/src/webnav.rs`'s three doors
(`address_bar`, `navigation_starting`, `resource_request`) and every filter and
refusing handler on `crates/bt-platform/src/webview.rs`.

| # | Requirement (Windows) | WKWebView public API | Verdict |
|---|---|---|---|
| 1 | Address bar / pin / session / palette — a string from outside (`webnav::address_bar`) | none needed: it is decided before `loadRequest:` | mapped |
| 2 | Top-level navigation re-asked (`NavigationStarting`) | `decidePolicyForNavigationAction:` + `targetFrame.isMainFrame` | mapped |
| 3 | Cancel **and navigate to** a rewritten URL | `.Cancel`, then the host starts the substitute load | mapped with a difference |
| 4 | Frames and iframes (`FrameNavigationStarting`) | same callback, `targetFrame.isMainFrame == false`, `sourceFrame` names the parent | mapped |
| 5 | Every redirect hop asked again | same callback once per hop + `didReceiveServerRedirectForProvisionalNavigation:` | mapped |
| 6 | Subresources — image, stylesheet, script, fetch (`WebResourceRequested`, `CONTEXT_ALL`) | **no callback exists**; `WKContentRuleList` blocks by URL pattern | mapped with a difference |
| 7 | A local seat reads its own folder and no other (`resource_request`, `Mint::File`) | `loadFileURL:allowingReadAccessToURL:` | mapped with a difference |
| 8 | A local seat reaches no server | `WKContentRuleList` blocking `^https?://` | mapped with a difference |
| 9 | `file:` refused as a location from a page | WebKit refuses it itself, before any callback | mapped with a difference |
| 10 | Downloads cancelled (`DownloadStarting`) | `decidePolicyForNavigationResponse:` → `.Cancel`; `navigationAction.shouldPerformDownload`; `WKDownloadDelegate` for the `.Download` route | mapped |
| 11 | Popups and `target=_blank` (`NewWindowRequested`) | `WKUIDelegate createWebViewWithConfiguration:…` → nil | mapped |
| 12 | A script changing `location` | `decidePolicyForNavigationAction:`, `navigationType = Other` | mapped |
| 13 | `data:` / `blob:` / `javascript:` as a location | same callback, full URL, `.Cancel` | mapped |
| 14 | External schemes never launched (`LaunchingExternalUriScheme`) | same callback with the `mailto:` URL; `.Cancel` stops the hand-off | mapped |
| 15 | Intercepting `http(s)` the way `WebResourceRequested` does | `WKURLSchemeHandler` — **refuses every scheme WebKit handles** | **unsupported guarantee** |
| 16 | HTTP authentication refused | `didReceiveAuthenticationChallenge:` → `RejectProtectionSpace` | mapped |
| 17 | Script dialogs answered, never shown (`ScriptDialogOpening`) | `WKUIDelegate`'s three panel methods; not implementing them is the refusal | mapped |
| 18 | Every permission denied (`PermissionRequested` → DENY) | per-capability `WKUIDelegate` methods; unimplemented means denied | mapped with a difference |
| 19 | The profile's lifetime (`%LOCALAPPDATA%\Folio\WebView2`) | `WKWebsiteDataStore.nonPersistentDataStore` | mapped |
| 20 | Requests a service or shared worker makes (`…FilterWithRequestSourceKinds`, `SOURCE_KINDS_ALL`) | no Rust-side door; not measured | **unsupported guarantee** |

`limitsNavigationsToAppBoundDomains` is not a row: it reads back `true` with no
`WKAppBoundDomains` key in the bundle, it caps at ten declared domains, and a
browsing seat has no domain list. It is not a policy hook for this product.

## The fixture run

| Case | Delegate callback fired | Could the policy have blocked it, and where | Measured |
|---|---|---|---|
| Top-level load of the fixture | `ACTION` (`type=Other`, main frame) | yes — `decidePolicyForNavigationAction:` | allowed, `RESPONSE` 200 |
| Cross-origin `<img>` | **none** | only `WKContentRuleList` | hit B `dest=image`; with the rule list, no hit, `crossimg=blocked` |
| Cross-origin `<link rel=stylesheet>` | **none** | only `WKContentRuleList` | hit B `dest=style`; blocked with the rule list |
| Third-party `<script src>` | **none** | only `WKContentRuleList` | hit B `dest=script`; blocked with the rule list |
| `fetch()` to the second origin | **none** | only `WKContentRuleList` | hit B `dest=empty`, `Origin:` present |
| Cross-origin `<iframe>` | `ACTION` (`target={main=false}`, `source=` the parent) | yes — the same callback | allowed; with the rule list the callback still fires and the fetch never happens |
| 302 → 302 → page | `ACTION` **per hop** + two `SERVER-REDIRECT` | yes — each hop | all three hops asked and allowed |
| `<a target=_blank>` | `ACTION` with `targetFrame=nil`, then `CREATE-WEBVIEW` | yes — either | refused by returning nil |
| `window.open(...)` | **`CREATE-WEBVIEW` only** (no prior `ACTION`) | yes — `WKUIDelegate` only | refused by returning nil |
| `location.href = …` | `ACTION` (`type=Other`) | yes | allowed, page changed |
| `<a href="file:…">` from an http page | **none** | WebKit refuses it first | navigation never started |
| `<img src="file:…">` from an http page | **none** | WebKit refuses it first | `fileimg=blocked` |
| `<a href="data:…">` | `ACTION`, full data URL | yes | cancelled |
| `<a href="mailto:…">` | `ACTION`, `type=LinkActivated` | yes | cancelled, nothing handed to the machine |
| `Content-Disposition: attachment` | `ACTION`, then `RESPONSE` `canShowMIMEType=false`, `suggested=folio-x2.bin` | yes — `decidePolicyForNavigationResponse:` | cancelled; `FAIL-PROVISIONAL Frame load interrupted` |
| 401 with `WWW-Authenticate: Basic` | `AUTH`, `realm=folio-x2`, `NSURLAuthenticationMethodHTTPBasic` | yes | rejected; the 401 body rendered |
| Local seat: `<img>` in its own folder | **none** | not needed | `inside=loaded` |
| Local seat: `<img src="../outside/…">` | **none** | `allowingReadAccessToURL:` | `outside=blocked` **without any rule list** |
| Local seat: `<iframe src="../outside/…">` | **none** | `allowingReadAccessToURL:` | never loaded, and no callback either |
| Local seat: `<img>` from a server | **none** | only `WKContentRuleList` | `net=loaded` bare; `net=blocked` with `^https?://` blocked |
| Local seat: link to a server | `ACTION` (`source=` the `file:` page) | yes | cancelled |
| `setURLSchemeHandler:` for `https` / `file` | — | — | `NSInvalidArgumentException`: *is a URL scheme that WKWebView handles natively* |
| `WKWebView.handlesURLScheme` | — | — | true for `http`, `https`, `file`, `about`, `data`, `blob`; false for a private scheme |

Two side measurements: `nonPersistentDataStore` reports `isPersistent=false`, and
one object conforming to both `WKNavigationDelegate` and `WKUIDelegate` serves
the whole matrix.

## The unsupported guarantees, for Q5

Four things the owner is being asked to accept, in plain words. **First**, there
is no per-request door for a page's own contents: a picture, a stylesheet, a
script or a `fetch` is never announced to Folio at all, so the only way to stop
one is a list of URL patterns compiled in advance, and a rule that cannot be
written as a pattern cannot be enforced. **Second**, a request stopped that way
is dropped by the engine rather than answered with the empty 403 the Windows host
mints, and Folio never learns it happened — there is no line for the trace and no
reason for a card to show, where today every refusal carries one. **Third**,
requests a service worker or a shared worker makes on a page's behalf, which the
Windows host filters on purpose, have no Rust-side door here and were not
measured, so they are unknown rather than covered. **Fourth**, permissions are
refused one capability at a time instead of by a single deny-everything event, so
a capability a future WebKit adds arrives with Apple's default rather than with
Folio's refusal already standing. None of these changes what the product's stated
policy *is* today: every rule `webnav.rs` actually holds — a local page reads its
own folder and reaches no server, a browsing page's own origins are its business —
is enforceable on macOS. What is reduced is the generality of the mechanism.

## The recommended design for M4-2 / M4-3

(The ticket called this M4-1; §7.1 gives that id to the `CALayer` composition, so
the policy work is M4-2's host and M4-3's enforcement.)

**Delegates decide what a seat may *go to*; a content rule list decides what a
document may be *built out of*, and both are compiled from the same `webnav`
answer.** One Objective-C class in `bt-platform` conforms to
`WKNavigationDelegate` and `WKUIDelegate` and calls the existing
`navigation_gate` closure from `decidePolicyForNavigationAction:` — main frame
and subframe in one place, where Windows needed two — with
`decidePolicyForNavigationResponse:` carrying the download refusal and
`createWebViewWithConfiguration:` the popup refusal. `Decision::Navigate(target)`
where `target != candidate` becomes cancel-then-load, exactly as
`CancelAndNavigateTo` already does.

For the third door, `webnav.rs` grows **one function and no new policy**: a
`content_rules(mint: &Mint) -> String` that emits the Safari content-blocker JSON
standing for the same sentence `resource_request` answers — for `Mint::File`, a
`block` rule on `^https?://` and nothing else, because the folder half is
already carried by the load itself; for `Mint::Nothing`, an empty list. It must be generated from the same constants `resource_request` reads, and
a test must assert the pair agree case by case, so the two spellings cannot
drift. The seat compiles it through `WKContentRuleListStore` at mint time (it
compiled in well under a step here) and swaps it on the
`WKUserContentController` — measured working, live, on an already-created seat.
The folder half of the local rule is carried by
`loadFileURL:allowingReadAccessToURL:` with the minted file's folder, which
enforced it here with no rule list at all.

Two smaller consequences. `SECURITY.md`'s web-preview paragraph is already the
right shape and gains the four sentences above. And `WKWebsiteDataStore` should
be non-persistent, which closes the persistent-profile hole that paragraph
currently records on Windows.

## Build and process ledger

`cargo check -j 6` green, 0 errors, 23 warnings (all `unnecessary unsafe`).
`cargo build -j 6` **rc=0, 22.6 s wall, 61.8 s user, peak RSS 1.70 GB**, binary
1,385,032 bytes, wrapped in an ad-hoc signed `ProbeX2.app` with bundle id
`io.github.lulu-loopp.folio.probe-x2` and `LSMinimumSystemVersion 14.0`. Shared
target directory 3.9 GB, volume 55 GiB free. Every run went in through `open`
from a launcher under `~/folio-port/launchers`, logged to `~/folio-port/logs`,
and ended only the pid the probe printed as its own first line; no process
outside `~/folio-port` was touched and nothing was installed. Five runs: the
first three died at the authentication challenge, which turned out to be objc2's
debug-time selector verification refusing `protectionSpace` on WebKit's
forwarding proxy `WKNSURLAuthenticationChallenge` — the send itself is fine, and
the probe's profile turns that verification off with the reason written beside
it. Worth carrying to M4-2: a panic inside a delegate callback unwinds into
Objective-C and takes the process with it before anything reaches stderr, and an
`open`ed bundle has no terminal, so the backend needs its own panic hook.
