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
#
# ## The one assessment that has to pass
#
# `spctl` runs last, after the staple, and a refusal there is this script's
# failure. It is the only place in the lane where Gatekeeper can be expected to
# say yes: `sign.sh` asks the same question before notarization, where the
# answer is `source=Unnotarized Developer ID` by design, so that one prints its
# verdict and exits 0. `accepted` is not sufficient on its own either — the
# source has to say `Notarized Developer ID`, which is the claim the release
# page makes.

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

# **The Gatekeeper question, which is not the same question for the two
# artifacts.** `spctl -a -vvv` asks whether a bundle may *execute*;
# `-t open --context context:primary-signature` asks whether a document may be
# *opened*, which is what macOS actually asks when a downloaded disk image is
# double-clicked, and `-t exec` on an image answers a question nobody asks.
#
# It is asked here and not in `sign.sh`, because this is the first moment it can
# be passed: before the ticket is stapled even a real Developer ID signature is
# refused with `source=Unnotarized Developer ID`, so the assessment made there
# is informational and this one is the one that decides.
assess() {
	case "$path" in
	*.dmg)
		run spctl -a -vvv -t open --context context:primary-signature "$path"
		;;
	*)
		run spctl -a -vvv "$path"
		;;
	esac
}

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
		--key "$key" --key-id "$KEY_ID" --issuer "$ISSUER_ID" "$log.partial"
	echo "  # and only a fetch that succeeded is moved into place:"
	run mv "$log.partial" "$log"
	run xcrun stapler staple "$path"
	run xcrun stapler validate "$path"
	assess
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

# **A log that was not fetched is not a log that was kept.** Whatever is at
# `$log` goes first: an earlier submission's document left at that path is the
# one thing worse than no document, because the release lane archives it under
# this submission's name. Then the fetch writes a file of its own, and only a
# fetch that succeeded — and that answers about the submission this run made —
# is moved into place and announced.
rm -f "$log"
fetched="$log.partial"
rm -f "$fetched"
kept=0

if [ -n "$id" ]; then
	echo
	echo "=== notarytool log $id"
	if xcrun notarytool log "$id" \
		--key "$key" --key-id "$KEY_ID" --issuer "$ISSUER_ID" "$fetched"; then
		# The document names the submission it is about. `plutil` answers
		# nothing on a shape that has no `jobId`, and a log that does not say is
		# taken at its word rather than thrown away — what is refused is a log
		# that says it is about a different submission.
		about=$(/usr/bin/plutil -extract jobId raw -o - -- "$fetched" 2>/dev/null || true)
		if [ -n "$about" ] && [ "$about" != "$id" ]; then
			echo "notarize.sh: the log fetched is about submission $about, not $id" >&2
		else
			mv "$fetched" "$log"
			kept=1
			echo "notarize.sh: log kept at $log"
		fi
	else
		echo "notarize.sh: notarytool log $id failed; no log was kept" >&2
	fi
	rm -f "$fetched"
else
	echo "notarize.sh: the submission answered no id — no log to fetch" >&2
fi

[ -z "$zip" ] || rm -f "$zip"

# A refusal says where to read about itself, and it says it only if there is
# something there: the log is what names the binary the service objected to, and
# pointing at a path the fetch above did not write is how an hour goes missing.
where() {
	if [ "$kept" = "1" ]; then
		echo "see $log"
	else
		echo "and its log was not fetched, so there is nothing to read about it"
	fi
}

if [ "$submit_rc" != "0" ]; then
	echo "notarize.sh: the notary service did not accept this artifact; $(where)" >&2
	exit "$submit_rc"
fi

status=$(/usr/bin/plutil -extract status raw -o - -- "$submission" 2>/dev/null || true)
if [ "$status" != "Accepted" ]; then
	echo "notarize.sh: status is '$status', not 'Accepted'; $(where)" >&2
	exit 1
fi

# The log is part of what a release produces, and `docs/RELEASING.md` says it is
# archived once per submission. A submission that was accepted and whose log was
# not kept has lost the only record of what the service looked at — and Apple
# keeps it for a limited time, so it is lost for good. That is a failed release,
# and it fails here rather than three steps later where nobody connects the two.
# A *rejected* submission has already exited above with its own code, which is
# the failure worth reporting.
if [ "$kept" != "1" ]; then
	echo "notarize.sh: $path was accepted and its log was not kept; the submission is" >&2
	echo "             $id — fetch it with 'xcrun notarytool log' before it ages out." >&2
	exit 1
fi

echo
echo "=== stapler staple"
xcrun stapler staple "$path"

echo
echo "=== stapler validate"
xcrun stapler validate "$path"

# **This is the assessment that must pass**, and it is the last thing this
# script does: the ticket is stapled, so the answer here is the answer a
# reader's machine gives with the network off.
#
# `accepted` alone is not enough. Gatekeeper accepts for more than one reason,
# and the only one this release claims is a notarized Developer ID build — an
# `accepted` whose `source=` says anything else is a different claim, and
# `docs/RELEASING.md` has asked a person to read that line since M5. Reading it
# here means nobody has to.
echo
echo "=== spctl, after stapling"
set +e
verdict=$(assess 2>&1)
spctl_rc=$?
set -e
echo "$verdict"
if [ "$spctl_rc" != "0" ]; then
	echo "notarize.sh: Gatekeeper refused $path after it was stapled — this build does not ship" >&2
	exit 1
fi
case "$verdict" in
*"source=Notarized Developer ID"*) ;;
*)
	echo "notarize.sh: Gatekeeper accepted $path, but not as a notarized Developer ID build" >&2
	echo "             — the source above is not the claim this release makes." >&2
	exit 1
	;;
esac

echo
echo "notarize.sh: $path is notarized, stapled and accepted; the log is at $log"
