#!/bin/sh
#
# Assemble `Folio.app` from a `cargo build --release` output, and put the
# debug information beside it rather than inside it.
#
# usage: bundle.sh --out <dir> [--binary <path>] [--icon <path>] [--no-dsym]
#        bundle.sh --icons-only <path.icns> [--icon <path>] [--icon-fallback]
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
# `--icons-only` writes the `.icns` at the given path and stops there — no
# binary, no plist, no `cargo`. It is how the icon can be proved on a machine
# that has not built anything, and with `--icon-fallback` it is how the `.ico`
# route stays exercised (see 3).
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
# The same render writes the two update keys (0.4.6 ticket U-9):
# `FolioUpdateProtocol` and `FolioMinUpdater`, from
# `bt_winres::release_manifest::{PROTOCOL, MIN_UPDATER}` — the constants the
# Windows build writes into the manifest `folio.exe` carries. They are sealed
# with the rest of `Info.plist` when the bundle is signed, which is all of that
# manifest a bundle needs: its seal already covers every file in it.
#
# 2. **Reproducible means the same commit gives the same plist and the same
# layout.** Everything this script writes other than the executable is a
# function of the checkout: the plist is the template plus one version string,
# the icon set is resampled from a file in `assets/`, `PkgInfo` is eight
# constant bytes. It does **not** claim a byte-identical executable — that is a
# property of the compiler and the profile, not of the packaging — so the tree
# and the sizes are printed at the end for a reader to compare two runs with.
#
# 3. **The icon.** macOS wants an `.icns`; this repository's mark is drawn by
# `assets/app-icon/make-folio-ico.py` out of geometry in code
# (`assets/app-icon/README.md`), and that script writes it twice: nine entries
# into `folio.ico`, which is what Windows links, and one square at 1024 into
# `folio-1024.png`, which is what this script reads. The tools a stock Mac has
# for the conversion are `sips` and `iconutil`, both in `/usr/bin`, and neither
# needs Xcode.
#
# 1024 is `icon_512x512@2x`, the largest slot an icon set has, so from that one
# source every slot `iconutil` names is generated and not one of them is an
# upscale. `build_icns` below is the whole of it, and it is a function so that
# `--icons-only` can run the icon and nothing else.
#
# The `.ico` remains a source this script can read, because the day the PNG is
# missing is not the day to discover the fallback stopped working;
# `--icon-fallback` takes it deliberately, which is how that route is kept
# exercised. What it produces is the reason the PNG exists, and both halves are
# invisible in the result:
#
#   * `sips` reads an `.ico` as **its largest entry only** — 256x256 for this
#     file. The hand-drawn 16, 20, 24, 32, 40, 48 and 64 entries inside it are
#     DIB payloads that `sips` will not address individually, so the small
#     iconset slots come out resampled from the 256 rather than taken from the
#     drawings made for them.
#   * That source has **no pixels above 256**, so `icon_256x256@2x`,
#     `icon_512x512` and `icon_512x512@2x` are not generated at all — the slot
#     loop skips a slot wider than its source rather than upscaling into it,
#     because a blurred 256 exactly where Finder's largest preview looks is
#     worse than an absent slot, which macOS fills by scaling the 256 itself.
#
# The default source is the largest PNG under `assets/app-icon/`, and the `.ico`
# only when there is none; `--icon` overrides both.
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
#
# 6. **The four documents travel inside the bundle.** `LICENSE-MIT`,
# `LICENSE-APACHE`, `THIRD-PARTY-NOTICES.md` and `TRADEMARK.md` — the same four
# the Windows archive carries, for the same reasons
# (`scripts/release/package.ps1`): MIT and Apache-2.0 both ask that the notice
# accompany the distribution, `THIRD-PARTY-NOTICES.md` is how `option-ext`'s
# MPL-2.0 §3.2 obligation is met, and `TRADEMARK.md` says what the two licences
# do not grant.
#
# On macOS there is nowhere else to put them. The disk image is a delivery van
# the reader throws away — `dmg.sh` carries no document of its own, because two
# copies of a licence in one download is a question nobody can answer — and what
# survives the drag to `/Applications` is `Folio.app`. So they are copied into
# `Contents/Resources/` from the repository root, where `check-notices.ps1` has
# already proved that the notices match `Cargo.lock`, and they are copied
# **before** any signing, so they are inside the seal: a licence a
# `codesign --verify` would not miss is a licence nobody can remove without the
# removal showing. Through 0.4.0 neither macOS download carried any of them.
#
# 7. **The binary is asked what it is, and a stale one is refused.** The plist
# is rendered from *this* checkout's manifest and the executable is copied from
# wherever `--binary` points; nothing but this check stands between the two
# being about different builds, and the manual route — build, bundle, sign,
# notarize, publish — never asked. It is the comparison
# `.github/workflows/release.yml` makes after its own build, moved to where both
# routes pass through it: `--version` must answer `Folio <version> (<commit>)`
# with the version the plist just received and the short hash of `HEAD`.
#
# The expected version is read back out of the rendered `Info.plist` rather than
# out of `Cargo.toml`, so 1 above still holds — there is no second reader of the
# manifest here, only a reader of what the renderer wrote.

set -eu

usage() {
	echo "usage: bundle.sh --out <dir> [--binary <path>] [--icon <path>] [--no-dsym]" >&2
	echo "       bundle.sh --icons-only <path.icns> [--icon <path>] [--icon-fallback]" >&2
	exit 2
}

out=""
binary=""
icon=""
icons_only=""
icon_fallback=0
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
	--icons-only)
		[ $# -ge 2 ] || usage
		icons_only="$2"
		shift 2
		;;
	--icon-fallback)
		icon_fallback=1
		shift
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

if [ -n "$icons_only" ]; then
	if [ -n "$out" ]; then
		echo "bundle.sh: --icons-only writes one file and --out assembles a bundle; pick one" >&2
		usage
	fi
else
	[ -n "$out" ] || usage
fi

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

if [ -z "$icons_only" ]; then
	if [ -z "$binary" ]; then
		binary="${CARGO_TARGET_DIR:-$repo/target}/release/folio"
	fi
	if [ ! -f "$binary" ]; then
		echo "bundle.sh: no release executable at $binary" >&2
		echo "           build one first: cargo build --release --locked -p bt-app" >&2
		exit 1
	fi
fi

# The icon source: the largest PNG in the icon directory if there is one, and
# the `.ico` otherwise. `ls -S` sorts by size, which for a set of renderings of
# one drawing is the same order as by pixels, and this is a fall back to a
# single known file rather than a search. `--icon-fallback` skips the look, so
# that the `.ico` route can be run on a tree where the PNG is present — which is
# every tree, which is why it would otherwise never be run again.
if [ -z "$icon" ]; then
	if [ "$icon_fallback" = "0" ]; then
		icon=$(ls -S "$repo/assets/app-icon/"*.png 2>/dev/null | head -1 || true)
	fi
	[ -n "$icon" ] || icon="$repo/assets/app-icon/folio.ico"
fi
if [ ! -f "$icon" ]; then
	echo "bundle.sh: no icon source at $icon" >&2
	exit 1
fi

# The icon step, on its own. Everything it needs is one source file, `sips` and
# `iconutil`; nothing else this script does is an input to it, and a proof that
# a source fills every slot should not have to be bought with a release build.
build_icns() {
	icns_source=$1
	icns_destination=$2

	iconset=$(mktemp -d "${TMPDIR:-/tmp}/folio-iconset.XXXXXX")
	trap 'rm -rf "$iconset"' EXIT INT TERM
	mkdir -p "$iconset/Folio.iconset"

	native=$(sips -g pixelWidth "$icns_source" | awk '/pixelWidth:/ { print $2 }')
	if [ -z "$native" ]; then
		echo "bundle.sh: sips could not read a pixel width out of $icns_source" >&2
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
		sips -s format png -z "$pixels" "$pixels" "$icns_source" --out "$iconset/Folio.iconset/$name" >/dev/null
		echo "  $name (${pixels}px)"
	done

	iconutil -c icns "$iconset/Folio.iconset" -o "$icns_destination"
	rm -rf "$iconset"
	trap - EXIT INT TERM
}

if [ -n "$icons_only" ]; then
	echo "bundle.sh: the icon and nothing else"
	echo "  icon   $icon"
	mkdir -p "$(dirname "$icons_only")"
	build_icns "$icon" "$icons_only"
	printf '  %12s  %s
' "$(wc -c <"$icons_only" | tr -d ' ')" "$icons_only"
	exit 0
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
# `rust-toolchain.toml` pins the Windows triple (docs/DESIGN.md §13.5: the pin
# is the version, not the host), and rustup refuses a channel whose triple is
# not this machine's. Off Windows the same version on this host's own triple is
# the toolchain every Mac lane here already exports; derive it once when the
# caller has not.
if [ -z "${RUSTUP_TOOLCHAIN:-}" ] && [ "$(uname -s)" = Darwin ]; then
	version=$(sed -n 's/^channel = "\([0-9][0-9.]*\)-.*/\1/p' "$repo/rust-toolchain.toml")
	arch=$(uname -m)
	[ "$arch" = arm64 ] && arch=aarch64
	RUSTUP_TOOLCHAIN="$version-$arch-apple-darwin"
	export RUSTUP_TOOLCHAIN
	echo "bundle.sh: toolchain $RUSTUP_TOOLCHAIN (this host's triple at the pinned version)"
fi
(cd "$repo" && cargo run -q --locked -p bt-winres --bin render-info-plist) >"$app/Contents/Info.plist"

# `Contents/MacOS/folio` — the name `CFBundleExecutable` in the template gives.
cp "$binary" "$app/Contents/MacOS/folio"
chmod 755 "$app/Contents/MacOS/folio"

# **Ask the executable what it is** — 7 above. Asked of the copy in the bundle,
# because that is the one that gets signed, notarized and published, and asked
# with the plist already rendered, because the answer it is compared against is
# the renderer's and not a second reading of `Cargo.toml`.
stamped=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")
# The rule `crates/bt-app/build.rs` follows, spelled the same way: the short
# hash of `HEAD`, and the word `unknown` where there is no git to ask — so a
# checkout without one compares `unknown` with `unknown` rather than failing
# over a fact neither side has.
commit=$(git -C "$repo" rev-parse --short=10 HEAD 2>/dev/null || echo unknown)
expected="Folio $stamped ($commit)"
answered=$("$app/Contents/MacOS/folio" --version)
echo "  says   $answered"
if [ "$answered" != "$expected" ]; then
	echo "bundle.sh: the binary says '$answered'; this checkout is '$expected'" >&2
	echo "           Build this tree before bundling it — the plist is written from" >&2
	echo "           the manifest, and a bundle whose program is older is a release" >&2
	echo "           wearing a version it was not built at." >&2
	exit 1
fi

# `Contents/PkgInfo` — eight bytes, no newline. Launch Services reads the
# package type out of `Info.plist`; this file is the older place it looked, it
# costs nothing, and its absence is the kind of thing a bundle inspector
# reports. `????` is the signature field for an application with no registered
# creator code, which since Mac OS X is every application.
printf 'APPL????' >"$app/Contents/PkgInfo"

# `Contents/Resources/Folio.icns` — 3 above, in a temporary iconset that is
# removed whether or not `iconutil` succeeds.
build_icns "$icon" "$app/Contents/Resources/Folio.icns"

# `Contents/Resources/` and the four documents — 6 above. Copied before anything
# is signed, so that they are inside the seal rather than beside it. A name that
# is not in the checkout stops the run here, with `cp` naming the file: these
# are the same four the Windows archive carries, and a release that quietly drops
# one is a release that distributes the crates without their notices.
for document in LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md TRADEMARK.md; do
	cp "$repo/$document" "$app/Contents/Resources/$document"
done

# **Read back what was assembled**, rather than trusting eight copies that each
# reported success. The bundle is the artifact; the commands above are not. This
# is the same refusal `scripts/release/package.ps1` makes of the Windows
# archive — every name on the list is there, and nothing else is — and it is
# what makes the list a list rather than a comment: a file that stops being
# copied, or one that arrives without being asked for (a `.DS_Store` a Finder
# window left behind is the ordinary one, and it would be signed along with the
# rest), is found here and not by a reader.
contents='Contents/Info.plist
Contents/MacOS/folio
Contents/PkgInfo
Contents/Resources/Folio.icns
Contents/Resources/LICENSE-APACHE
Contents/Resources/LICENSE-MIT
Contents/Resources/THIRD-PARTY-NOTICES.md
Contents/Resources/TRADEMARK.md'

assembled=$(cd "$app" && find . -type f | sed 's|^\./||' | LC_ALL=C sort)
listed=$(printf '%s\n' "$contents" | LC_ALL=C sort)
if [ "$assembled" != "$listed" ]; then
	echo "bundle.sh: Folio.app does not hold exactly the listed files" >&2
	echo "listed:" >&2
	printf '%s\n' "$listed" | sed 's|^|  |' >&2
	echo "assembled:" >&2
	printf '%s\n' "$assembled" | sed 's|^|  |' >&2
	exit 1
fi

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
echo "bundle.sh: update protocol $(/usr/libexec/PlistBuddy -c 'Print :FolioUpdateProtocol' "$app/Contents/Info.plist"), min updater $(/usr/libexec/PlistBuddy -c 'Print :FolioMinUpdater' "$app/Contents/Info.plist")"
