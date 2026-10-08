#!/bin/sh
#
# The cask's hooks, installed with Homebrew (0.4.6 ticket U-2).
#
# usage: check-cask-hooks.sh <Folio-<version>-macos-arm64.dmg>
#
# `packaging/homebrew/folio.rb` is rendered with a `file://` URL to the image
# given, put in a scratch tap, and taken through what the cask promises
# (docs/RELEASING.md, "Distribution manifests"):
#
#   1. `brew install --cask`: `postflight_steps` wrote the marker attribute
#      `io.github.lulu-loopp.folio.install` on the installed bundle, and a Folio
#      started from it says in `diagnostics.log` that it is managed by homebrew
#      (with no uninstall hook: E-10).
#   2. `brew upgrade --cask` to a bumped version, and `brew reinstall --cask`:
#      the marker is there after each (`postflight_steps` write it again), and
#      Folio's cleanup did not run (a planted update entrance is still there).
#   3. plain `brew uninstall --cask`: the app is gone and the cleanup did not
#      run — the consequence of `uninstall_hook: false`, recorded.
#   4. `brew uninstall --cask --zap` while that Folio runs: the app is taken
#      away anyway (Homebrew moves it back into the Caskroom before any zap
#      step), the door refuses with exit 2, and the zap goes on.
#   5. `brew uninstall --cask --zap` with Folio closed: the door ran, the
#      planted entrance is gone, and the data folder and a planted remover's
#      folder (`Folio-uninstall`) are trashed.
#
# **Nothing of this account is touched.** Every brew command and the Folio it
# starts run with HOME pointed at a scratch folder, so the cleanup door's
# account-wide rows (LaunchAgents, agent hooks, shell profiles) and the zap's
# `~/Library/Application Support/Folio` are the scratch folder's. The app is
# installed with `--appdir` into the scratch folder, never /Applications. The
# scratch tap is removed at the end. It refuses to start where a cask named
# `folio` is already installed: that is somebody's Folio, and a second one of
# the same token cannot be installed beside it.
#
# The planted entrance is a plist named as the update's entrance is
# (`io.github.lulu-loopp.folio.update-` and eight hex digits), in the scratch
# home's LaunchAgents; the door's sweep removes it (`launch_agent::sweep`).
#
# Homebrew refuses a cask from a file path unless HOMEBREW_DEVELOPER is set, so
# the cask goes into a tap made with `brew tap-new --no-git`. The image is
# quarantined by Homebrew as any download is; macOS may ask once before the
# first start.

set -eu

[ $# -eq 1 ] || { echo "usage: check-cask-hooks.sh <Folio-<version>-macos-arm64.dmg>" >&2; exit 2; }
dmg=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
[ -f "$dmg" ] || { echo "check-cask-hooks.sh: no image at $dmg" >&2; exit 2; }
version=$(basename "$dmg" | sed -nE 's/^Folio-([0-9]+\.[0-9]+\.[0-9]+)-macos-arm64\.dmg$/\1/p')
[ -n "$version" ] || { echo "check-cask-hooks.sh: $dmg is not named Folio-<version>-macos-arm64.dmg" >&2; exit 2; }
command -v brew >/dev/null || { echo "check-cask-hooks.sh: brew is not on PATH" >&2; exit 2; }
if brew list --cask folio >/dev/null 2>&1; then
	echo "check-cask-hooks.sh: a cask named folio is installed here; this check needs a machine without one" >&2
	exit 2
fi

repo=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/folio-cask-hooks.XXXXXX")
home="$work/home"
apps="$work/Applications"
mkdir -p "$home/Library/LaunchAgents" "$apps"
tap="folio-u2-check/hooks"
attribute="io.github.lulu-loopp.folio.install"
expected='{"v":1,"manager":"homebrew","uninstall_hook":false}'
planted="$home/Library/LaunchAgents/io.github.lulu-loopp.folio.update-00c0ffee.plist"
diagnostics="$home/Library/Application Support/Folio/diagnostics.log"
sha=$(shasum -a 256 "$dmg" | cut -d' ' -f1)
folio=""
failures=0

# Every brew command: the scratch home, no auto-update, no API (the tap is local).
b() {
	HOME="$home" HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_FROM_API=1 \
		HOMEBREW_NO_ANALYTICS=1 HOMEBREW_NO_ENV_HINTS=1 brew "$@"
}

# 0 when the command succeeds, 1 when it does not — without `set -e` ending
# the script at the first check that fails.
ok() {
	if "$@"; then echo 0; else echo 1; fi
}

check() {
	if [ "$1" = "0" ]; then echo "ok   $2"; else echo "FAIL $2"; failures=$((failures + 1)); fi
}

cleanup() {
	if [ -n "$folio" ] && kill -0 "$folio" 2>/dev/null; then kill "$folio"; fi
	if b list --cask "$tap/folio" >/dev/null 2>&1; then b uninstall --cask --force "$tap/folio" || true; fi
	b untap "$tap" >/dev/null 2>&1 || true
	rm -rf "$work"
}
trap cleanup EXIT

# The cask as this repository publishes it, at a version, from the local image.
render() {
	sed -E \
		-e "s|^([[:space:]]*version )\"[^\"]*\"|\1\"$1\"|" \
		-e "s|^([[:space:]]*sha256 )\"[^\"]*\"|\1\"$sha\"|" \
		-e "s|^([[:space:]]*url )\"[^\"]*\"|\1\"file://$dmg\"|" \
		"$repo/packaging/homebrew/folio.rb" >"$casks/folio.rb"
}

marker_is_there() {
	[ "$(xattr -p "$attribute" "$apps/Folio.app" 2>/dev/null)" = "$expected" ]
}

plant() {
	printf '<?xml version="1.0" encoding="UTF-8"?>\n<plist version="1.0"><dict><key>Label</key><string>io.github.lulu-loopp.folio.update-00c0ffee</string></dict></plist>\n' >"$planted"
}

# Started, and waited for until this run's install channel line is in the log;
# then a moment more, for the data folder's claim the cleanup door asks about.
start_folio() {
	seen=$(grep -c 'install channel' "$diagnostics" 2>/dev/null || true)
	seen=${seen:-0}
	HOME="$home" "$apps/Folio.app/Contents/MacOS/folio" >/dev/null 2>&1 &
	folio=$!
	echo "started Folio, pid $folio"
	tries=0
	until [ "$(grep -c 'install channel' "$diagnostics" 2>/dev/null || true)" -gt "$seen" ] 2>/dev/null ||
		[ $tries -ge 120 ]; do
		sleep 0.5
		tries=$((tries + 1))
	done
	sleep 2
}

stop_folio() {
	kill "$folio"
	wait "$folio" 2>/dev/null || true
	folio=""
}

b tap-new --no-git "$tap" >/dev/null
casks="$(b --repository "$tap")/Casks"
mkdir -p "$casks"

# ── 1. install ────────────────────────────────────────────────────────────────
render "$version"
b install --cask --appdir="$apps" "$tap/folio"
check "$(ok marker_is_there)" "install: the marker attribute is $(xattr -p "$attribute" "$apps/Folio.app" 2>/dev/null)"
start_folio
line=$(grep 'install channel' "$diagnostics" | tail -1 || true)
case "$line" in
*"install channel managed by homebrew — marker"*) check 0 "install: diagnostics.log says '$line'" ;;
*) check 1 "install: diagnostics.log says '$line'" ;;
esac
stop_folio
plant

# ── 2. upgrade and reinstall ─────────────────────────────────────────────────
render "$version.1"
b upgrade --cask --appdir="$apps" "$tap/folio"
check "$(ok marker_is_there)" "upgrade: the marker attribute is there after the upgrade"
check "$(ok [ -f "$planted" ])" "upgrade: the cleanup did not run"
b reinstall --cask --appdir="$apps" "$tap/folio"
check "$(ok marker_is_there)" "reinstall: the marker attribute is there after the reinstall"
check "$(ok [ -f "$planted" ])" "reinstall: the cleanup did not run"

# ── 3. plain uninstall ────────────────────────────────────────────────────────
b uninstall --cask "$tap/folio"
check "$(ok [ ! -e "$apps/Folio.app" ])" "uninstall: the app is gone"
check "$(ok [ -f "$planted" ])" "uninstall: the cleanup did not run (uninstall_hook false; its marks stay)"

# ── 4. zap while Folio runs ───────────────────────────────────────────────────
b install --cask --appdir="$apps" "$tap/folio"
start_folio
said=$(b uninstall --cask --zap "$tap/folio" 2>&1 || true)
echo "$said"
check "$(ok [ ! -e "$apps/Folio.app" ])" "zap while Folio runs: the app is taken away anyway (recorded)"
case "$said" in
*"A Folio instance is running"*) check 0 "zap while Folio runs: the door refused, and said why" ;;
*) check 1 "zap while Folio runs: the door refused, and said why" ;;
esac
check "$(ok [ -f "$planted" ])" "zap while Folio runs: the door changed nothing"
stop_folio

# ── 5. zap ────────────────────────────────────────────────────────────────────
b install --cask --appdir="$apps" "$tap/folio"
# What a remover ended before it could retire leaves behind (REMOVER_HOME).
mkdir -p "$home/Library/Application Support/Folio-uninstall/uninstall-0123"
said=$(b uninstall --cask --zap "$tap/folio" 2>&1)
echo "$said"
case "$said" in
*"Update entrances (LaunchAgents)"*) check 0 "zap: the door ran and brew printed its lines" ;;
*) check 1 "zap: the door ran and brew printed its lines" ;;
esac
check "$(ok [ ! -f "$planted" ])" "zap: the planted entrance is gone"
check "$(ok [ ! -e "$home/Library/Application Support/Folio" ])" "zap: the data folder is trashed"
check "$(ok [ ! -e "$home/Library/Application Support/Folio-uninstall" ])" "zap: the remover's folder is trashed"

echo
if [ "$failures" -gt 0 ]; then
	echo "$failures check(s) failed"
	exit 1
fi
echo "the cask's hooks hold, installed"
