# Linux

Folio has native X11 and Wayland windows and uses the Unix PTY for terminal
sessions. The release recipe currently targets `x86_64-unknown-linux-gnu`.

## Current support

- Terminal startup and presentation are checked on X11 and Wayland with a real
  display session and a PTY.
- Linux windows use Folio's own title bar and window buttons. Drag the title bar
  to move the window, or drag its edges to resize it.
- Clipboard reads run asynchronously and are bounded. X11 reads support a local
  UNIX display socket or a `DISPLAY` value with a literal IP address; remote
  display hostnames are not supported. Clipboard ownership and text writes use
  the native backend selected for the window.
- Wayland clipboard operations require the compositor to support
  `ext-data-control` or `wlr-data-control` version 1 or later. Without either
  protocol, clipboard operations report an error; terminal startup still works.
- Directory watches start and retire on workers. Desktop open/reveal, file
  chooser, notification, video poster extraction, and trash operations also
  run off the window thread. Final quit waits up to three seconds for
  desktop cleanup, using the session-save budget. If cleanup is still running at
  the deadline, Folio records a diagnostic and continues exiting. The retirement
  worker closes the trash queue to new requests and lets accepted requests drain.
  Folio does not cancel them to meet the deadline, so they may still be running
  when the process exits. Closing their source window does not cancel the
  filesystem action.
- Web preview requires a full Chromium build that supports unpacked Manifest V3
  extensions. Folio does not bundle Chromium. It checks `FOLIO_CHROMIUM_PATH`
  first, then looks on `PATH` for `chromium`, `chromium-browser`,
  `google-chrome-stable`, or `google-chrome`. A matching executable name is
  only a candidate: Folio verifies that its private request-policy extension
  loads before installing a page. Some branded Google Chrome builds disable
  unpacked extension loading. In that case startup reports that the selected
  browser did not load Folio's private MV3 policy extension and asks for a full
  Chromium build that supports unpacked MV3 extensions.
  Full Chrome 154.0.8037.92 from Chrome for Testing was used for local engine
  validation; that test browser is not bundled or installed by Folio. Set
  `FOLIO_CHROMIUM_PATH` to the full Chromium executable when it is not on
  `PATH`.
- Desktop notifications use `org.freedesktop.Notifications` on the session
  D-Bus. When the server supports actions, clicking a notification opens its
  Folio target. A missing server or a failed request reports an error
  asynchronously.
- Local video plays inside Folio's video pane. GStreamer, the system media
  library, decodes the file. Folio displays the frames and sends audio to
  `autoaudiosink`. Installed GStreamer plugins determine which video and audio
  formats Folio can play.
- The video bar supports play/pause, timeline seeking, volume, mute and four
  speeds: 1×, 1.25×, 1.5× and 2×. Press `Space` to play or pause, `←` or `→` to
  seek by five seconds, `↑` or `↓` to change volume, and `M` to toggle mute.
  Folio reports an error if it cannot open or decode a video.
- When `ffprobe` and `ffmpeg` are installed, local video can show one poster
  frame.

X11 can provide global pointer, window, monitor, and work-area coordinates.
Wayland does not expose those global coordinates. Operations that
need them, such as choosing a monitor from a global point or placing a window at
an absolute position, may report that the compositor does not expose the needed
capability. Wayland startup and terminal use do not depend on those operations.

| Operation | X11 | Wayland |
| --- | --- | --- |
| Minimize | Requests the window manager | Requests the compositor |
| Restore a minimized window from Folio | Supported | Unavailable through winit; restore through the compositor |
| Request focus | Requests the window manager | Refused without an activation token; an already focused window needs no request |
| Place a window at desktop coordinates | Supported | Unavailable |
| Global shortcut and drop-down summon | Supported | Reports the missing global shortcut or placement capability |

Window managers and compositors decide whether to accept each request.

## Runtime requirements

Run Folio inside an X11 or Wayland graphical session. Wayland needs a valid
`WAYLAND_DISPLAY` and `XDG_RUNTIME_DIR`; X11 needs a valid `DISPLAY`. Folio uses
wgpu's Vulkan and OpenGL ES backends, so the machine needs a working Vulkan
loader and driver or an EGL/OpenGL ES implementation. The binary is dynamically
linked for GNU/Linux.

The native keyboard stack loads `libxkbcommon.so.0`; X11 also uses
`libxkbcommon-x11.so.0`. The Wayland backend dynamically loads
`libwayland-client.so.0`. Wgpu loads `libvulkan.so.1` for Vulkan, or `libEGL.so.1`
and the system's GLES driver for OpenGL ES. Package names vary by distribution;
the system must provide the libraries for the active display backend and
graphics driver. The build and installer do not bundle desktop services or GPU
drivers.

These command-line helpers are used only by the matching feature:

| Helper | Feature |
| --- | --- |
| `/usr/bin/env` | Passing the installed executable path from the desktop entry |
| `gio` | Desktop file opening and recycling |
| `xdg-open` | Fallback for opening a path when `gio` is absent |
| `gdbus` | Asking a FileManager1-compatible file manager to reveal a path; Folio can open the parent directory when that request is unavailable |
| `zenity` or `kdialog` | File and folder pickers; Folio tries them in that order |
| `ffprobe` and `ffmpeg` | One local-video poster frame |

Missing helpers affect their feature and are reported by that operation. They are
not installation prerequisites for terminal startup.

### Video playback requirements

Folio loads these GStreamer 1.x libraries when playback starts:
`libgstreamer-1.0.so.0`, `libgstapp-1.0.so.0`, `libgobject-2.0.so.0` and
`libglib-2.0.so.0`. The system also needs GStreamer plugins that provide
`playbin`, `appsink` and `autoaudiosink`, and support the video's formats.
Package names vary by distribution. Folio does not need GStreamer development
headers to build. Missing runtime libraries or plugins prevent video playback,
but they do not prevent terminal startup.

## Download a Linux workflow artifact

The Linux workflow stores a `.tar.gz` as a GitHub Actions workflow artifact. It
does not publish the archive as a GitHub Release asset. The archive preserves
executable modes and contains the staged build, install and uninstall scripts,
these Linux guides, `BUILD-INFO.txt` and `SHA256SUMS`.

Download an artifact from its workflow run, verify the archive, then verify its
contents after extraction:

```sh
gh run download <run-id> --name folio-<version>-linux-x86_64
sha256sum -c folio-<version>-linux-x86_64.tar.gz.sha256
tar -xzf folio-<version>-linux-x86_64.tar.gz
cd folio-<version>-linux-x86_64
sha256sum -c SHA256SUMS
./scripts/release/install-linux.sh --from "$PWD"
```

To uninstall, run `./scripts/release/uninstall-linux.sh` from the extracted
directory. Each release archive's `BUILD-INFO.txt` records the runner image, the
build host's glibc version, and the highest GLIBC symbol version required by its
binary. The Ubuntu artifact requires GLIBC 2.39; a local Fedora build requires
GLIBC 2.43. These values apply to those artifacts; Folio does not claim a
universal minimum across Linux distributions.

## System appearance

When the theme setting is `System`, Folio reads the desktop's
`org.freedesktop.appearance/color-scheme` value from the XDG Settings portal and
follows its change notifications while a window is open. If the portal, key or
value is unavailable, Folio uses its existing dark fallback; it does not infer
the theme from a desktop name or GTK setting.

## Build

The build script reads the pinned compiler version from `rust-toolchain.toml`,
uses the Linux host toolchain, overrides the workspace's Windows-only static
CRT flag, and builds from `Cargo.lock` with `--locked`:

```sh
scripts/release/build-linux.sh --out target/linux-release
```

This script currently supports x86_64 Linux. It stages the executable, desktop
entry, icon, MIT and Apache licenses, third-party notices, and trademark notice
in the directory passed to `--out`. It does not install system packages or
write to the user's XDG directories.

## Install for one user

After building, install the staged files:

```sh
scripts/release/install-linux.sh
```

The default locations are:

| File | Location |
| --- | --- |
| Executable | `~/.local/bin/folio` |
| Desktop entry | `$XDG_DATA_HOME/applications/io.github.lulu-loopp.folio.desktop` |
| Icon | `$XDG_DATA_HOME/icons/hicolor/512x512@2/apps/io.github.lulu-loopp.folio.png` |
| Notices | `$XDG_DATA_HOME/doc/folio/` |

For installation, an unset, empty or relative `XDG_DATA_HOME` selects `~/.local/share`. The
desktop entry launches Folio without file or URL arguments. The command line
accepts one positional path (a file or folder):

```text
folio [--cwd <folder>] [--profile <id>] [--new-window | --tab] [--] [<path>]
folio --help | --version
```

The desktop launcher passes the installed executable path through `env`; that
path cannot contain `=` or control characters. The installer checks this before
writing files. Spaces, `$`, quotes, backticks, backslashes, and `%` are supported.

For a disposable local prefix, pass the same `--prefix` to install and
uninstall. `--prefix` does not redirect Folio's configuration or session data.

```sh
scripts/release/install-linux.sh \
    --from target/linux-release \
    --prefix /tmp/folio-test-prefix
scripts/release/uninstall-linux.sh --prefix /tmp/folio-test-prefix
```

The installer and uninstaller do not use `sudo`. Uninstall removes only Folio's
binary, desktop entry, icon, and four bundled notices. It leaves
`$XDG_DATA_HOME/Folio`, shell files, and other user data in place.

## Configuration and data directories

| Content | Location |
| --- | --- |
| Settings and keybindings | `$XDG_CONFIG_HOME/Folio/<data-tag>/` |
| Sessions, profiles and other persistent data | `$XDG_DATA_HOME/Folio/` |
| Web profile | `$XDG_DATA_HOME/Folio/Chromium/` |
| Web cache | `$XDG_CACHE_HOME/Folio/<data-tag>/Chromium/` |
| Web temporary files | Private `$XDG_RUNTIME_DIR/Folio/`, or Folio's private per-user temporary directory |

Unset config, data and cache variables use `~/.config`, `~/.local/share` and
`~/.cache`. `<data-tag>` is the existing instance tag for the data directory;
different data directories keep separate configurations. The instance claim
and local IPC retain their existing private per-user temporary directory.

The application keeps its existing relative or empty `XDG_DATA_HOME` rule: those
values place data under the launch directory. Changing that rule requires a
[migration that preserves existing data](plans/design/linux-xdg-directories.md).
Config, cache and browser temporary directories ignore relative XDG overrides.

Folio reads an existing settings or keybindings file in the data directory when
the new config file is absent. A worker copies the old file without replacing
an existing config file or removing the original. Update trials defer migration
until writes are released. Normal uninstall preserves both locations; explicit
application data purge removes only the selected data namespace.

## Display smoke check

The smoke script uses a real X11 or Wayland session, starts a shell on a PTY,
and checks that nonzero terminal dimensions and text are presented. Its files
go under `target/linux-smoke`; it checks display and PTY output, not keyboard
input or IME.

```sh
python3 scripts/ci/linux-smoke.py wayland
python3 scripts/ci/linux-smoke.py x11
```

Wayland runs require the current session's `WAYLAND_DISPLAY` and
`XDG_RUNTIME_DIR`. X11 runs require `DISPLAY`.

## Private Xorg keyboard and PTY smoke check

`linux-input-smoke.py` starts a separate Xorg server with a Void core keyboard
and pointer. Its config disables automatic device and GPU discovery, so host
physical input devices are not attached. `xdotool` sends XTest events to the
Folio window. The check covers text input, Kitty keyboard protocol,
modifyOtherKeys, Alt, resize delivery to `stty size`, and shell-child reaping
after the application quits. It also drags the window from its caption and east
edge and checks the resulting X window geometry. It does not test a physical
keyboard or IME.

The test needs a Linux Folio binary, Xorg with the dummy video and Void input
drivers, `xdotool`, and Openbox:

```sh
python3 scripts/ci/linux-input-smoke.py \
    --exe target/debug/folio \
    --xorg /usr/bin/Xorg \
    --modulepath /usr/lib64/xorg/modules \
    --xdotool /usr/bin/xdotool \
    --openbox /usr/bin/openbox
```

Artifacts go under `target/linux-input-smoke` by default. The script creates a
private display and removes its server, window manager, and application on
exit.
