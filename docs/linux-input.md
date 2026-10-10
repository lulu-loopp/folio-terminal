# Linux input and text rendering checks

This guide describes the isolated Linux input probes. They use private display
servers, runtime directories and Wayland sockets; they do not send input to the
desktop session that launches them.

## Xorg keyboard, terminal and window gestures

[`scripts/ci/linux-input-smoke.py`](../scripts/ci/linux-input-smoke.py) starts a
private Xorg server with the Void core keyboard and pointer. Its configuration
disables automatic device and GPU discovery. `xdotool` sends XTest events only
to the Folio window on that private server.

The probe checks first-run Escape, PTY text, Kitty keyboard protocol,
modifyOtherKeys, Alt, window move and east-edge resize, `stty size` after resize,
and shell-child reaping on quit. It does not test a physical keyboard or an IME
engine.

```sh
python3 scripts/ci/linux-input-smoke.py \
    --exe target/debug/folio \
    --xorg /usr/bin/Xorg \
    --modulepath /usr/lib64/xorg/modules \
    --xdotool /usr/bin/xdotool \
    --openbox /usr/bin/openbox
```

The probe also accepts `--xorg-config` and `--artifacts`. Use an isolated Xorg
module path containing the dummy video and Void input drivers when the system
does not have those modules. The Xorg configuration must keep
`AutoAddDevices`, `AutoEnableDevices` and `AutoAddGPU` disabled.

## Native Wayland keyboard protocols and shortcuts

[`scripts/ci/linux-wayland-keyboard-smoke.py`](../scripts/ci/linux-wayland-keyboard-smoke.py)
starts Niri on a private Xvfb display, then runs Folio on Niri's private
Wayland socket. It reuses the PTY `KEY_READER` fixture from the Xorg smoke to
check ordinary shell input, Kitty Shift+Enter, xterm modifyOtherKeys Ctrl+Enter
and the Alt prefix. It then sends Ctrl+Shift+Q and checks that Folio exits and
its PTY shell child is reaped.

The test injects XTest keys into the nested Niri outer window. It does not run
an input-method peer during these raw-key checks, and it does not attach host
input devices or test a physical keyboard. It sets the XKB layout to US only
on the private Xvfb and nested Niri instances.

```sh
python3 scripts/ci/linux-wayland-keyboard-smoke.py \
    --exe target/debug/folio \
    --niri /usr/bin/niri \
    --xvfb /usr/bin/Xvfb \
    --xdotool /usr/bin/xdotool \
    --setxkbmap /usr/bin/setxkbmap
```

The helper accepts `--xdotool-libdir` for an extracted `xdotool` binary and
`--artifacts` to change its output directory. Niri, Xvfb, `xdotool` and
`setxkbmap` must be available.

## Native Wayland text input and popup placement

[`scripts/ci/linux-wayland-ime-smoke.py`](../scripts/ci/linux-wayland-ime-smoke.py)
runs Folio on a nested Niri compositor. A separate test peer speaks the real
`zwp_input_method_v2` protocol to Niri; Niri then sends composition events over
`zwp_text_input_v3` to Folio. XTest events enter Niri through the private outer
Xvfb window. No host `DISPLAY`, `WAYLAND_DISPLAY`, `/dev/input` device or
`uinput` device is used.

The test peer starts a `nihao` preedit after the synthetic `N` key and commits
`你好` after Space. It creates an input popup surface with a magenta marker.
The harness checks that preedit reaches Folio without changing PTY bytes, that
the terminal caret moves to the composition cursor, and that Niri's popup
rectangle matches Folio's latest caret rectangle. The saved screenshot shows
the marker under that caret. It then checks the UTF-8 commit, candidate-caret
withdrawal and IME deactivation on blur, a new composition after focus returns,
and PTY-child reaping when the window closes.

Build the protocol peer and run the probe with a Linux Folio binary:

```sh
cargo build --locked -j1 \
    --manifest-path scripts/ci/wayland-ime-driver/Cargo.toml \
    --target-dir target/linux-wayland-ime-driver

python3 scripts/ci/linux-wayland-ime-smoke.py \
    --exe target/debug/folio \
    --ime-driver target/linux-wayland-ime-driver/debug/wayland-ime-driver
```

Niri, Xvfb, `xdotool` and `grim` must be available. Their paths can be supplied
with `--niri`, `--xvfb` and `--xdotool`; use `--xdotool-libdir` for an extracted
`xdotool` binary. Pass `--artifacts` to select another output directory. The
test creates and retires its own Xvfb server, Niri process, Wayland sockets and
PTY child.

This is an end-to-end protocol test, not a unit test. It verifies the Wayland
composition, commit, caret and popup transport through the compositor. Its
test peer is not an IBus or Fcitx engine, so it does not validate Pinyin
conversion or a production candidate list. XTest input is synthetic; it does
not claim physical keyboard testing.

## Browser keyboard and IME fixture

[`scripts/ci/linux-web-input-fixture.py`](../scripts/ci/linux-web-input-fixture.py)
serves a local editable page at `127.0.0.1` and a distinct loopback site at
`127.0.0.2`. The top page has an input, textarea and contenteditable, followed
by a same-origin iframe with its own editable and a nested cross-site editable
frame. The separate site exercises Chromium's OOPIF path. Each frame records
`keydown`, `beforeinput`, `input`, composition and focus events with the
`isTrusted` flag, DOM key/code, modifiers, value, selection, caret range and
frame marker.
Records have a per-frame sequence number. The aggregate event list is available
at `/state`; `--events-file` also writes the records as JSONL.

Start the fixture in a separate process and navigate the Linux preview to the
printed URL:

```sh
python3 scripts/ci/linux-web-input-fixture.py \
    --events-file target/web-input-events.jsonl
```

The server binds only `127.0.0.1` and `127.0.0.2`. The fixture provides page
content and evidence readback; it does not drive a browser or by itself prove
keyboard or IME delivery.

## Browser DOM keyboard and IME end-to-end probes

[`scripts/ci/linux-web-input-xorg-smoke.py`](../scripts/ci/linux-web-input-xorg-smoke.py)
starts that fixture, an isolated Xorg/Openbox session and private IBus under
`dbus-run-session`. It uses XTest to type into the top-level input and the
cross-site OOPIF input, then uses IBus/XIM with libpinyin to commit `你好` into
the browser input. The report includes trusted DOM `keydown` and committed
`input` events, the IME caret area, a preedit screenshot and PTY-child reaping.
The probe requires a native Xorg blur/refocus in the app log and checks that
the flushed XIM area exactly matches the accepted browser caret rectangle
translated by the visible page bounds. The area may remain unchanged during
composition; it must already be correct after the refocused field receives the
click.
The Xorg config disables device discovery and binds only the private Void
keyboard and pointer. Folio and Chromium use private XDG/profile state; only
Xorg and Openbox inherit the extracted Xorg library path.

Run it under a new private D-Bus session. `--modulepath` must contain the
private dummy-video and Void input modules; `--desktop-root` is the extracted
Xorg/Openbox bundle used for the server and window manager.

```sh
dbus-run-session -- python3 scripts/ci/linux-web-input-xorg-smoke.py \
    --exe /path/to/folio \
    --chromium /path/to/full/chromium \
    --xorg /path/to/Xorg \
    --modulepath /path/to/xorg-module-overlay \
    --xdotool /path/to/xdotool \
    --openbox /path/to/openbox \
    --desktop-root /path/to/extracted-desktop-root
```

To compare Xorg navigation and DOM keyboard input without the input-method
stack, run the same command without `dbus-run-session` and add
`--without-ibus`. This mode uses the same private Xorg, fixture and browser
path, but only checks DOM keyboard delivery and PTY-child reaping; it does not
test XIM composition or candidate placement.

[`scripts/ci/linux-wayland-web-input-smoke.py`](../scripts/ci/linux-wayland-web-input-smoke.py)
starts its own loopback fixture, Xvfb, nested Niri and the existing
`zwp_input_method_v2` test peer. It checks XTest keyboard delivery to the top
input and OOPIF input, sends a `nihao` preedit and `你好` commit to the top
browser input, then blurs and restores the window and repeats composition in
the nested OOPIF input. The fixture records
`compositionstart`, `compositionupdate`, `compositionend`, `beforeinput` and
`input` with each frame marker. The probe also requires a nonempty caret area
and an input-popup rectangle matching each caret before and after refocus.
Preedit and committed browser text must stay out of the terminal PTY.

Build the private protocol peer and run the Wayland probe with a full Chromium
build:

```sh
cargo build --locked -j1 \
    --manifest-path scripts/ci/wayland-ime-driver/Cargo.toml \
    --target-dir target/linux-wayland-ime-driver

python3 scripts/ci/linux-wayland-web-input-smoke.py \
    --exe /path/to/folio \
    --chromium /path/to/full/chromium \
    --ime-driver target/linux-wayland-ime-driver/debug/wayland-ime-driver
```

The JSONL records are saved in each run's private artifact directory. A passing
top-level key route has `frame="top"`, `id="input"`, trusted `keydown` events
and an `input.value` containing `FOLIO_XORG_KEY_OK`. The OOPIF route has
`frame="oopif-127.0.0.2"`, `id="oopif-input"` and
`FOLIO_OOPIF_KEY_OK`. The first composition records use `top/input`; after
blur and refocus the second uses `oopif-127.0.0.2/oopif-input`. Both committed
input values contain `你好`. Wayland XTest and the IME-v2 peer
are synthetic protocol input, not a claim that a physical keyboard or a
production Wayland input method was tested. The Xorg path uses the installed
libpinyin engine in a private IBus session.

## Chromium navigation bootstrap events

[`scripts/ci/linux-navigation-correlation-smoke.py`](../scripts/ci/linux-navigation-correlation-smoke.py)
starts full Chromium headless with a private profile and XDG directories,
loads the production `navigation_gate.js` bootstrap, and clicks two top-level
links through CDP. It records the real `Runtime.bindingCalled` payloads and
checks their request order, URL, policy token and cancelable status. Both
navigations remain on the fixture's file URL. The script does not start Folio
or call the Linux actor; it validates the production browser bootstrap and
the browser-to-host event payloads.

The Rust test
`a_reversed_navigation_verdict_cannot_replace_the_latest_main_frame_request`
checks the actor's deterministic correlation predicate for the latest request,
root session/context, controller generation and policy token. It rejects an
older request after a newer request becomes current. Together, these are a real
Chromium bootstrap probe and a deterministic actor predicate test, not a live
actor/CDP reversed-reply integration test.

```sh
python3 scripts/ci/linux-navigation-correlation-smoke.py \
    --chromium /path/to/full/chromium
```

The probe is separate from the default Cargo test run because it needs a full
Chromium build. It uses a private directory under `target` for profile, cache,
HOME and XDG state, plus a short private runtime directory so Chromium's Unix
socket paths fit the platform limit. It clears inherited display and session
bus variables and does not open a window or use desktop input.

## Shell integration and color glyphs

`python3 scripts/shell-integration/tests/linux-pty.py` exercises Bash, Zsh,
`sh`, Dash and Fish in real Unix PTYs, including working directory and exit
status integration.

The focused renderer gate is:

```sh
cargo test --locked -p bt-render --lib \
    linux_color_emoji_uses_bundled_face_with_nonempty_color_rasters -j1
```

It asks the production font system for color rasters for 😀, 👍🏽 and a family
ZWJ sequence at 1×, 1.5× and 2×. Private Niri runs show CJK and these emoji
while one output changes between these scales. DECSET 2027 enables
grapheme-cluster handling for the ZWJ and skin-tone samples.

A separate private Sway 1.11 run uses two headless outputs at the same time:
1600×1000 at 1× and 2400×1500 at 2×. Moving the same Folio window from 1× to
2× and back produces fresh matching renderer DPI records and visible CJK and
emoji. Shell input works after the compositor focuses the moved window; an
xdg close request exits cleanly. That run uses Intel graphics with Mesa 26.1.5
and Vulkan. Its compositor config, HOME, XDG directories and sockets are
private; it does not change the host desktop.
