#!/bin/sh
#
# Assemble `Folio.app` from a `cargo build --release` output, and put the
# debug information beside it rather than inside it.
#
# usage: bundle.sh --out <dir> [--binary <path>] [--icon <path>] [--no-dsym]
#
# `--out` is the directory the bundle is written into; `Folio.app` and
# `Folio.app.dSYM` are created there and anything already at those two names is
# replaced. `--binary` is the release executable, and defaults to
# `${CARGO_TARGET_DIR:-<repo>/target}/release/folio` — the same rule cargo
# itself follows, so a caller that set `CARGO_TARGET_DIR` for the build does not
# have to repeat the path here. `--icon` overrides the icon source resolved
# below. `--no-dsym` skips `dsymutil` for a local run where the debug
# information is not wanted; a release must never pass it (see 4).
#
# 1. **The version is not read here, and that is the point.** It comes from
# `cargo run -p bt-winres --bin render-info-plist`, which fills
# `packaging/macos/Info.plist.in`'s `@VERSION@` from `bt-winres`'s own
# `CARGO_PKG_VERSION` — the `[workspace.package] version` line every crate in
# this tree inherits, and the one line
# `bt_app::version::tests::the_version_is_the_manifests_and_nothing_elses`
# already holds `folio --version`, the PE `VERSIONINFO` block and every
# diagnostic header to. A `grep` of `Cargo.toml` in this script would be a
# second reader of that line, agreeing with the first right up until a release
# moved it. `set -e` and the renderer's own exit code are what stand between an
# unfilled `@VERSION@` and a signed bundle carrying it.
#
# 2. **Reproducible means the same commit gives the same plist and the same
# layout.** Everything this script writes other than the executable is a
# function of the checkout: the plist is the template plus one version string,
# the icon set is resampled from a file in `assets/`, `PkgInfo` is eight
# constant bytes. It does **not** claim a byte-identical executable — that is a
# property of the compiler and the profile, not of the packaging — so the tree
# and the sizes are printed at the end for a reader to compare two runs with.
#
# 3. **The icon.** macOS wants an `.icns`; this repository's icon is
# `assets/app-icon/folio.ico`, drawn by `make-folio-ico.py` from geometry in
# code (`assets/app-icon/README.md`). The tools a stock Mac has for that
# conversion are `sips` and `iconutil`, both in `/usr/bin`, and neither needs
# Xcode. Two facts about `sips` and the `.ico` container decide what comes out,
# and both are recorded here because they are invisible in the result:
#
#   * `sips` reads an `.ico` as **its largest entry only** — 256x256 for this
#     file. The hand-drawn 16, 20, 24, 32, 40, 48 and 64 entries inside it are
#     DIB payloads that `sips` will not address individually, so the small
#     iconset slots are resampled from the 256 rather than taken from the
#     drawings made for them. On this mark — a fold, two papers and a graphite
#     tile, with no hairline a downscale could lose — that is a visible
#     difference nowhere, but it is a difference, and the honest place to fix it
#     is the icon source rather than this script.
#   * The source has **no pixels above 256**, so `icon_256x256@2x`,
#     `icon_512x512` and `icon_512x512@2x` are not generated. Upscaling to fill
#     them would put a blurred 256 where Finder's largest preview looks, which
#     is worse than the absence: with the slot missing, macOS scales the 256
#     itself and every reader gets the same result. A 1024 PNG drawn from the
#     same geometry would fill all three, and that is an icon-source ticket —
#     `make-folio-ico.py` can already render any size, but it needs Pillow,
#     which a stock Mac does not have.
#
# If a PNG is ever added under `assets/app-icon/`, the largest one wins over the
# `.ico` without this script changing: that is the `--icon` default's own rule
# below, not a special case.
#
# 4. **`dsymutil` runs, and its output does not go in the bundle.** The release
# profile sets `debug = "line-tables-only"`, and on Apple targets that debug
# information stays in the object files with a debug map in the linked image
# pointing at them (`docs/DESIGN.md` 13.31). The `.dSYM` `dsymutil` builds from
# the two is the only artifact that can turn a crash report from a shipped build
# back into file names and line numbers, it pairs with the image by UUID, and it
# exists only on the machine that linked the binary. So it is built here, beside
# the bundle, for the release lane to archive next to the notarization log — and
# it is **not** placed inside `Folio.app`, where it would be several times the
# download and signed for no reason.
#
# `dsymutil` warns on this workspace and the warning is expected: one object of
# the `psm` crate is hand-written assembly that rustc leaves in the temporary
# directory it links from, which is gone by the time this runs. An assembly stub
# carries no line tables, so what the warning names is debug information that
# never existed. Every Rust object is found, and `dwarfdump --uuid` below prints
# the UUID a crash report will be matched against.
#
# 5. **No home directory is assumed.** A GitHub macOS runner has none worth
# assuming and neither does anyone else's machine; every path here is either
# resolved from this script's own location or given on the command line.

set -eu

usage() {
	echo "usage: bundle.sh --out <dir> [--binary <path>] [--icon <path>] [--no-dsym]" >&2
	exit 2
}

out=""
binary=""
icon=""
dsym=1

while [ $# -gt 0 ]; do
	case "$1" in
	--out)
		[ $# -ge 2 ] || usage
		out="$2"
		shift 2
		;;
	--binary)
		[ $# -ge 2 ] || usage
		binary="$2"
		shift 2
		;;
	--icon)
		[ $# -ge 2 ] || usage
		icon="$2"
		shift 2
		;;
	--no-dsym)
		dsym=0
		shift
		;;
	-h | --help) usage ;;
	*)
		echo "bundle.sh: unknown argument $1" >&2
		usage
		;;
	esac
done

[ -n "$out" ] || usage

# This script is `<repo>/scripts/release/macos/bundle.sh`; the repository is
# three directories up from it, however the caller spelled the invocation.
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)

case "$(uname -s)" in
Darwin) ;;
*)
	echo "bundle.sh: a macOS bundle is assembled on macOS — sips, iconutil and dsymutil are its tools" >&2
	exit 1
	;;
esac

if [ -z "$binary" ]; then
	binary="${CARGO_TARGET_DIR:-$repo/target}/release/folio"
fi
if [ ! -f "$binary" ]; then
	echo "bundle.sh: no release executable at $binary" >&2
	echo "           build one first: cargo build --release --locked -p bt-app" >&2
	exit 1
fi

# The icon source: the largest PNG in the icon directory if there is one, and
# the `.ico` otherwise. `ls -S` sorts by size, which for a set of renderings of
# one drawing is the same order as by pixels, and this is a fall back to a
# single known file rather than a search.
if [ -z "$icon" ]; then
	icon=$(ls -S "$repo/assets/app-icon/"*.png 2>/dev/null | head -1 || true)
	[ -n "$icon" ] || icon="$repo/assets/app-icon/folio.ico"
fi
if [ ! -f "$icon" ]; then
	echo "bundle.sh: no icon source at $icon" >&2
	exit 1
fi

mkdir -p "$out"
out=$(cd "$out" && pwd)
app="$out/Folio.app"
dsym_path="$out/Folio.app.dSYM"

rm -rf "$app" "$dsym_path"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

echo "bundle.sh: assembling $app"
echo "  binary $binary"
echo "  icon   $icon"

# `Contents/Info.plist` — 1 above. `-q` so that cargo's progress does not reach
# the file; the renderer prints the document on stdout and nothing else.
(cd "$repo" && cargo run -q --locked -p bt-winres --bin render-info-plist) >"$app/Contents/Info.plist"

# `Contents/MacOS/folio` — the name `CFBundleExecutable` in the template gives.
cp "$binary" "$app/Contents/MacOS/folio"
chmod 755 "$app/Contents/MacOS/folio"

# `Contents/PkgInfo` — eight bytes, no newline. Launch Services reads the
# package type out of `Info.plist`; this file is the older place it looked, it
# costs nothing, and its absence is the kind of thing a bundle inspector
# reports. `????` is the signature field for an application with no registered
# creator code, which since Mac OS X is every application.
printf 'APPL????' >"$app/Contents/PkgInfo"

# `Contents/Resources/Folio.icns` — 3 above. Built in a temporary iconset that
# is removed whether or not `iconutil` succeeds.
iconset=$(mktemp -d "${TMPDIR:-/tmp}/folio-iconset.XXXXXX")
trap 'rm -rf "$iconset"' EXIT INT TERM
mkdir -p "$iconset/Folio.iconset"

native=$(sips -g pixelWidth "$icon" | awk '/pixelWidth:/ { print $2 }')
if [ -z "$native" ]; then
	echo "bundle.sh: sips could not read a pixel width out of $icon" >&2
	exit 1
fi
echo "  icon source is ${native}px wide"

# Every slot Apple's iconset names, as `<point size>:<scale>:<pixels>`. A slot
# wider than the source is skipped rather than upscaled.
for slot in 16:1:16 16:2:32 32:1:32 32:2:64 128:1:128 128:2:256 256:1:256 256:2:512 512:1:512 512:2:1024; do
	points=${slot%%:*}
	rest=${slot#*:}
	scale=${rest%%:*}
	pixels=${rest##*:}
	if [ "$pixels" -gt "$native" ]; then
		echo "  no ${points}x${points}@${scale}x (${pixels}px): the source has ${native}px"
		continue
	fi
	if [ "$scale" = "1" ]; then
		name="icon_${points}x${points}.png"
	else
		name="icon_${points}x${points}@${scale}x.png"
	fi
	sips -s format png -z "$pixels" "$pixels" "$icon" --out "$iconset/Folio.iconset/$name" >/dev/null
done

iconutil -c icns "$iconset/Folio.iconset" -o "$app/Contents/Resources/Folio.icns"
rm -rf "$iconset"
trap - EXIT INT TERM

# `Folio.app.dSYM` — 4 above. Read off the copy that ships, so that the UUID in
# the archived debug information is the UUID of the image a reader's crash
# report will name.
if [ "$dsym" = "1" ]; then
	dsymutil "$app/Contents/MacOS/folio" -o "$dsym_path"
	echo "  dSYM $dsym_path"
	dwarfdump --uuid "$dsym_path" || true
else
	echo "  dSYM skipped (--no-dsym); a release must not do this"
fi

echo
echo "bundle.sh: the tree, with sizes in bytes"
(cd "$out" && find Folio.app -print | sort | while IFS= read -r entry; do
	if [ -d "$entry" ]; then
		printf '  %12s  %s/\n' "-" "$entry"
	else
		printf '  %12s  %s\n' "$(wc -c <"$entry" | tr -d ' ')" "$entry"
	fi
done)
echo
printf '  %12s  %s\n' "$(du -sk "$app" | awk '{ print $1 * 1024 }')" "Folio.app (total)"
if [ "$dsym" = "1" ]; then
	printf '  %12s  %s\n' "$(du -sk "$dsym_path" | awk '{ print $1 * 1024 }')" "Folio.app.dSYM (total, beside the bundle)"
fi
echo
echo "bundle.sh: $(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist") $(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")"
