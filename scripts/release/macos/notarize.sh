#!/bin/sh
#
# Submit a signed artifact to Apple's notary service, keep the log it answers
# with, and staple the ticket to the artifact.
#
# usage: notarize.sh --path <Folio.app|Folio.dmg> [--notary-dir <dir>]
#                    [--log <path>] [--dry-run]
#
# This is M5-2's script, written by M5-1 and **not run against Apple here**.
# `--dry-run` prints every command it would run, in order, with the real paths
# filled in, and touches nothing. The real run is the owner's, from a session
# where the signing already happened — see `packaging/macos/README.md`.
#
# ## The credential, and what is not in this repository
#
# Notarization authenticates with an App Store Connect API key: a `.p8` private
# key file, plus a key id and an issuer id that name it. The two ids are not
# secret — they are identifiers of a record in Apple's console — and they live
# in `<notary dir>/folio-notary.env` as `KEY_ID=` and `ISSUER_ID=`. The `.p8`
# **is** secret, it lives at `<notary dir>/private_keys/AuthKey_<KEY_ID>.p8`
# with mode 600, and nothing here opens it: `notarytool` is given the path and
# reads it itself. `packaging/macos/.gitignore` refuses `*.p8` at the place one
# would land in a checkout.
#
# The notary directory defaults to `$HOME/.appstoreconnect`, which is where
# Apple's own documentation puts it and the one home-directory assumption in
# this tree. `--notary-dir` moves it, which is how a CI runner that writes the
# key into a job-scoped directory calls this.
#
# Both files are read **only if present**. With either missing this script says
# which one and stops, before anything is uploaded and before a release lane
# believes a step happened.
#
# ## Why the app is zipped and the DMG is not
#
# `notarytool submit` takes one file, and the kinds it takes are `.dmg`, `.pkg`
# and `.zip`. A `.app` is a directory, so it goes up inside a zip made with
# `ditto -c -k --sequesterRsrc --keepParent` — `ditto` rather than `zip` because
# it preserves the extended attributes and symlink structure a signed bundle
# depends on, and `--keepParent` so that what unpacks is `Folio.app` and not its
# contents. **The zip is a transport and nothing else.** The ticket comes back
# stapled to the *bundle*, not to the zip, and the zip is thrown away.
#
# ## The log is an artifact, not a diagnostic
#
# `notarytool log` is the only place the service says *why* it accepted or
# rejected — which binaries it looked at, which of them lacked a time stamp or a
# hardened runtime. It is fetched on success as well as on failure, written
# beside the artifact as `<artifact>.notarylog.json`, and the release lane keeps
# it: the plan's M5 acceptance asks for the log of each submission to be retained
# beside what it is about, and a log fetched only when something went wrong is a
# log nobody has when a question is asked later.
#
# ## Stapling
#
# `stapler staple` writes the ticket into the artifact so that a machine with no
# network can still see that it was notarized. `stapler validate` then reads it
# back. A stapled `.app` and a stapled `.dmg` are two separate staples of two
# separate tickets, which is why this script is run once per artifact rather
# than once per release.

set -eu

usage() {
	echo "usage: notarize.sh --path <Folio.app|Folio.dmg> [--notary-dir <dir>] [--log <path>] [--dry-run]" >&2
	exit 2
}

path=""
notary_dir=""
log=""
dry_run=0

while [ $# -gt 0 ]; do
	case "$1" in
	--path)
		[ $# -ge 2 ] || usage
		path="$2"
		shift 2
		;;
	--notary-dir)
		[ $# -ge 2 ] || usage
		notary_dir="$2"
		shift 2
		;;
	--log)
		[ $# -ge 2 ] || usage
		log="$2"
		shift 2
		;;
	--dry-run)
		dry_run=1
		shift
		;;
	-h | --help) usage ;;
	*)
		echo "notarize.sh: unknown argument $1" >&2
		usage
		;;
	esac
done

[ -n "$path" ] || usage

case "$(uname -s)" in
Darwin) ;;
*)
	echo "notarize.sh: xcrun notarytool and stapler are macOS tools" >&2
	exit 1
	;;
esac

if [ ! -e "$path" ]; then
	# A dry run says what it would do; it is allowed to say it about an artifact
	# that has not been built yet, which is how `dmg.sh --dry-run` can print its
	# whole chain without writing an image first. A real run is not.
	if [ "$dry_run" = "1" ]; then
		echo "notarize.sh: nothing at $path yet — a dry run prints the commands anyway"
	else
		echo "notarize.sh: nothing at $path" >&2
		exit 1
	fi
fi

[ -n "$notary_dir" ] || notary_dir="$HOME/.appstoreconnect"
env_file="$notary_dir/folio-notary.env"

if [ ! -f "$env_file" ]; then
	echo "notarize.sh: no notary credentials at $env_file" >&2
	echo "             it holds two lines, KEY_ID= and ISSUER_ID=, naming an App Store" >&2
	echo "             Connect API key whose .p8 sits in $notary_dir/private_keys/." >&2
	exit 1
fi

KEY_ID=""
ISSUER_ID=""
# shellcheck disable=SC1090 # the path is the caller's, by design.
. "$env_file"

if [ -z "$KEY_ID" ] || [ -z "$ISSUER_ID" ]; then
	echo "notarize.sh: $env_file must set both KEY_ID and ISSUER_ID" >&2
	exit 1
fi

key="$notary_dir/private_keys/AuthKey_$KEY_ID.p8"
if [ ! -f "$key" ]; then
	echo "notarize.sh: no API key at $key" >&2
	echo "             that is the name Apple's console gives the file it downloads;" >&2
	echo "             a key stored under another name is a key notarytool cannot find." >&2
	exit 1
fi

[ -n "$log" ] || log="$path.notarylog.json"

# Every command this script runs goes through `run`, so that `--dry-run` prints
# exactly what a real run would do rather than an approximation of it.
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

echo "notarize.sh: $path"
echo "  credentials $env_file"
echo "  key         $key"
echo "  log         $log"
if [ "$dry_run" = "1" ]; then
	echo "  --dry-run: the commands below are printed and not run"
fi
echo

upload="$path"
zip=""
case "$path" in
*.app)
	zip="${path%.app}-notarize.zip"
	upload="$zip"
	echo "=== the bundle goes up inside a zip"
	run rm -f "$zip"
	run ditto -c -k --sequesterRsrc --keepParent "$path" "$zip"
	;;
esac

echo "=== submit and wait"
if [ "$dry_run" = "1" ]; then
	run xcrun notarytool submit "$upload" \
		--key "$key" --key-id "$KEY_ID" --issuer "$ISSUER_ID" \
		--wait --output-format json
	echo "  # the submission id is read out of that JSON, and then:"
	run xcrun notarytool log "<submission-id>" \
		--key "$key" --key-id "$KEY_ID" --issuer "$ISSUER_ID" "$log"
	run xcrun stapler staple "$path"
	run xcrun stapler validate "$path"
	[ -z "$zip" ] || run rm -f "$zip"
	echo
	echo "notarize.sh: dry run complete; nothing was uploaded."
	exit 0
fi

submission=$(mktemp "${TMPDIR:-/tmp}/folio-notary.XXXXXX")
trap 'rm -f "$submission"' EXIT INT TERM

set +e
xcrun notarytool submit "$upload" \
	--key "$key" --key-id "$KEY_ID" --issuer "$ISSUER_ID" \
	--wait --output-format json >"$submission"
submit_rc=$?
set -e
cat "$submission"

# The id is in the JSON whether the submission was accepted or rejected, and the
# log is worth having in both cases — a rejection's log is the only statement of
# what was wrong with it.
# `plutil` reads JSON as well as it reads a plist and is in `/usr/bin` on every
# macOS, which a JSON parser of this script's own would not be.
id=$(/usr/bin/plutil -extract id raw -o - -- "$submission" 2>/dev/null || true)
if [ -n "$id" ]; then
	echo
	echo "=== notarytool log $id"
	xcrun notarytool log "$id" \
		--key "$key" --key-id "$KEY_ID" --issuer "$ISSUER_ID" "$log" || true
	echo "notarize.sh: log kept at $log"
else
	echo "notarize.sh: the submission answered no id — no log to fetch" >&2
fi

[ -z "$zip" ] || rm -f "$zip"

if [ "$submit_rc" != "0" ]; then
	echo "notarize.sh: the notary service did not accept this artifact; see $log" >&2
	exit "$submit_rc"
fi

status=$(/usr/bin/plutil -extract status raw -o - -- "$submission" 2>/dev/null || true)
if [ "$status" != "Accepted" ]; then
	echo "notarize.sh: status is '$status', not 'Accepted'; see $log" >&2
	exit 1
fi

echo
echo "=== stapler staple"
xcrun stapler staple "$path"

echo
echo "=== stapler validate"
xcrun stapler validate "$path"

echo
echo "notarize.sh: $path is notarized and stapled; the log is at $log"
