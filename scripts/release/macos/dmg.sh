#!/bin/sh
#
# Build the disk image a reader downloads: `Folio.app` and a link to
# `/Applications` beside it, signed, notarized, stapled, and asked of
# Gatekeeper the way a download is asked.
#
# usage: dmg.sh --app <path to Folio.app> --out <dir> [--identity <id>]
#               [--notary-dir <dir>] [--volname <name>] [--skip-notarize]
#               [--dry-run]
#
# This is M5-3's script, written by M5-1 and **not run against Apple here**.
# `--dry-run` prints the commands and touches nothing.
#
# ## What is in it, and what is deliberately not
#
# Two entries: the application, and a symbolic link to `/Applications`. The
# drag-to-install gesture is the whole interface of a disk image, and the link is
# what makes it a gesture rather than an instruction. Nothing else goes in — no
# `README`, whose relative links resolve against a repository and not against a
# mounted volume (`scripts/release/package.ps1` makes the same refusal for the
# same reason), and no licence file, because the application carries all four in
# `Contents/Resources/`: `LICENSE-MIT`, `LICENSE-APACHE`,
# `THIRD-PARTY-NOTICES.md` and `TRADEMARK.md`, put there by `bundle.sh` and
# sealed by `sign.sh`. A second copy on a volume the reader throws away would be
# a second copy to keep in step with the first.
#
# **No background image and no window layout.** Those are set by mounting the
# image read-write and scripting the Finder through Apple events, which needs
# Automation permission, a window server and a logged-in session; on a release
# machine it is a step that fails silently into an image that looks unfinished.
# The default Finder view shows both entries and the gesture works. The plan does
# not ask for one; when it does, it is its own ticket and its own permission.
#
# ## `ditto`, not `cp`
#
# The application arriving in the staging folder is already signed, and a signed
# bundle is its contents *and* their extended attributes. `ditto` copies both;
# `cp -R` does not carry every one of them, and a signature that does not verify
# after the copy is a defect discovered at `spctl` with nothing to point at.
#
# ## `UDZO`, and why the image is read-only
#
# `hdiutil create -format UDZO` writes a compressed, read-only image. Read-only
# is what makes the signature mean anything: a read-write image can be modified
# after it is signed and the signature stays valid for the container it was made
# of. Compressed because it is a download.
#
# ## The disk image is signed, and it is not code
#
# `codesign` on a `.dmg` seals the container, and Gatekeeper checks that seal
# when the image is opened. It gets `--timestamp` for the same reason the
# application does, and it does **not** get `--options runtime` or an
# entitlements file: those describe how a process runs, and a disk image is not
# one. The application inside it keeps the signature `sign.sh` gave it; this
# signature is about the wrapper.
#
# ## The Gatekeeper question is a different question
#
# `spctl -a -vvv Folio.app` asks whether the application may execute.
# `spctl -a -vvv -t open --context context:primary-signature Folio.dmg` asks
# whether this *document* may be opened, which is the assessment macOS actually
# makes when a downloaded image is double-clicked, and `-t exec` on a disk image
# answers a question nobody asks. The plan's M5 acceptance names this exact line.
#
# **It is asked once, and after the staple.** `notarize.sh` makes it on whatever
# it staples — this image included — and a refusal there is fatal, so the
# ordinary run of this script reaches the end with the question already
# answered. Only `--skip-notarize` asks it here, and there it is informational:
# an image nobody notarized is refused by design.
#
# ## Two names for one set of bytes
#
# The image leaves here as `Folio.dmg` and as `Folio-macos-arm64.dmg`, and the
# second is a copy of the first. GitHub serves
# `/releases/latest/download/<asset>` and resolves it by asset *name*, so a
# download link on a page outside this repository can only be written against a
# name that is the same in every release — which the published
# `Folio-<version>-macos-arm64.dmg` is not. The long name is still given by
# whoever moves the image to the release page, for the reason above: a script
# that built it would be a second reader of the version line. The copy that has
# no version in it is made here, because here is after the ticket is stapled,
# and a copy taken any earlier would be the file most people click and the one
# Gatekeeper turns away offline.
#
# It is a copy and not a second image: `checksums.sh` hashes the directory the
# release page is made of, so both names arrive there under one hash, and a
# reader who fetched either can check what they have. The two are hashed against
# each other here, because a copy nobody read back is a copy.

set -eu

usage() {
	echo "usage: dmg.sh --app <path to Folio.app> --out <dir> [--identity <id>] [--notary-dir <dir>] [--volname <name>] [--skip-notarize] [--dry-run]" >&2
	exit 2
}

app=""
out=""
identity="-"
notary_dir=""
volname="Folio"
skip_notarize=0
dry_run=0

while [ $# -gt 0 ]; do
	case "$1" in
	--app)
		[ $# -ge 2 ] || usage
		app="$2"
		shift 2
		;;
	--out)
		[ $# -ge 2 ] || usage
		out="$2"
		shift 2
		;;
	--identity)
		[ $# -ge 2 ] || usage
		identity="$2"
		shift 2
		;;
	--notary-dir)
		[ $# -ge 2 ] || usage
		notary_dir="$2"
		shift 2
		;;
	--volname)
		[ $# -ge 2 ] || usage
		volname="$2"
		shift 2
		;;
	--skip-notarize)
		skip_notarize=1
		shift
		;;
	--dry-run)
		dry_run=1
		shift
		;;
	-h | --help) usage ;;
	*)
		echo "dmg.sh: unknown argument $1" >&2
		usage
		;;
	esac
done

[ -n "$app" ] || usage
[ -n "$out" ] || usage

here=$(cd "$(dirname "$0")" && pwd)

case "$(uname -s)" in
Darwin) ;;
*)
	echo "dmg.sh: hdiutil is a macOS tool" >&2
	exit 1
	;;
esac

if [ ! -d "$app" ]; then
	echo "dmg.sh: no bundle at $app" >&2
	exit 1
fi
app=$(cd "$app" && pwd)

mkdir -p "$out"
out=$(cd "$out" && pwd)
dmg="$out/Folio.dmg"
stable="$out/Folio-macos-arm64.dmg"

run() {
	if [ "$dry_run" = "1" ]; then
		printf '  '
		for word in "$@"; do
			case "$word" in
			*" "*) printf '"%s" ' "$word" ;;
			*) printf '%s ' "$word" ;;
			esac
		done
		printf '\n'
		return 0
	fi
	"$@"
}

echo "dmg.sh: $dmg"
echo "  app      $app"
echo "  volume   $volname"
if [ "$identity" = "-" ]; then
	echo "  identity ad-hoc — Gatekeeper will reject the result, as it should"
else
	echo "  identity $identity"
fi
if [ "$dry_run" = "1" ]; then
	echo "  --dry-run: the commands below are printed and not run"
fi
echo

# The staging folder is what the image is made of, and it is thrown away after.
# It is created next to the output rather than in a temporary directory so that a
# failed run leaves the thing that failed where the reader can look at it.
staging="$out/dmg-staging"

echo "=== staging"
run rm -rf "$staging"
run mkdir -p "$staging"
run ditto "$app" "$staging/Folio.app"
run ln -s /Applications "$staging/Applications"

echo
echo "=== hdiutil create"
run rm -f "$dmg"
run hdiutil create -volname "$volname" -srcfolder "$staging" \
	-fs HFS+ -format UDZO -ov -quiet "$dmg"

echo
echo "=== codesign the image"
run codesign --force --timestamp -s "$identity" "$dmg"

echo
echo "=== codesign --verify --strict --verbose=2"
run codesign --verify --strict --verbose=2 "$dmg"

if [ "$skip_notarize" = "1" ]; then
	echo
	echo "dmg.sh: --skip-notarize; the image is signed and not notarized"
else
	echo
	echo "=== notarize and staple"
	# `notarize.sh` is the one script that talks to Apple, and it prints its own
	# commands under `--dry-run`, so the dry run hands the flag through rather
	# than printing a line that says a second script would have run.
	set -- --path "$dmg"
	[ -z "$notary_dir" ] || set -- "$@" --notary-dir "$notary_dir"
	[ "$dry_run" = "0" ] || set -- "$@" --dry-run
	"$here/notarize.sh" "$@"
fi

echo
if [ "$skip_notarize" = "0" ]; then
	# `notarize.sh` asked this exact question after it stapled the ticket, and a
	# refusal there stopped this script before it reached here. Asking it a
	# second time would print the same verdict about the same bytes.
	echo "=== spctl: asked and answered by notarize.sh, above, after the staple"
elif [ "$dry_run" = "1" ]; then
	echo "=== spctl -a -vvv -t open --context context:primary-signature"
	run spctl -a -vvv -t open --context context:primary-signature "$dmg"
else
	# **Informational, because nothing here was notarized.** Gatekeeper refuses
	# an image that has not been through the notary service however it was
	# signed, so under `--skip-notarize` this verdict is the expected one and
	# failing on it would be failing on the flag that was asked for.
	echo "=== spctl -a -vvv -t open --context context:primary-signature"
	set +e
	spctl -a -vvv -t open --context context:primary-signature "$dmg" 2>&1
	spctl_rc=$?
	set -e
	echo "spctl exit $spctl_rc — informational: --skip-notarize, so this image is not one that ships"
fi

run rm -rf "$staging"

echo
echo "=== the copy under the name that never changes"
run cp "$dmg" "$stable"
if [ "$dry_run" = "0" ]; then
	if [ "$(shasum -a 256 "$dmg" | cut -d " " -f 1)" != "$(shasum -a 256 "$stable" | cut -d " " -f 1)" ]; then
		echo "dmg.sh: $stable is not a copy of $dmg" >&2
		exit 1
	fi
	echo "$(basename "$stable"): the same bytes as $(basename "$dmg")"
fi

echo
if [ "$dry_run" = "1" ]; then
	echo "dmg.sh: dry run complete; nothing was written and nothing was uploaded."
else
	echo "dmg.sh: $dmg"
	echo "dmg.sh: $stable"
	ls -l "$dmg" "$stable"
fi
