#!/bin/sh
#
# Sign `Folio.app` with the hardened runtime, a secure time stamp and the
# entitlements file `packaging/macos/` keeps — then read the signature back and
# print what Gatekeeper says about it.
#
# usage: sign.sh --app <path to Folio.app> [--identity <id>]
#                [--entitlements <path>] [--no-spctl]
#
# `--identity` defaults to `-`, which is **ad-hoc**: a signature with no
# certificate behind it. That is the only identity an automated session on this
# project can use, and it is enough to prove everything about this script except
# the one thing only a certificate can prove. The owner passes the real one —
# `--identity "Developer ID Application: <name> (<TEAMID>)"` — from a session
# with the login keychain open; over ssh, `codesign` answers
# `errSecInternalComponent` because the key it needs is in a keychain that
# session never unlocked (`docs/plans/port/macos-plan-2026-09-12.md` section 5,
# item 3).
#
# ## The four flags, and why each is not optional
#
#   * `--options runtime` **is** the hardened runtime. It is a flag on the
#     signature, not a key in the entitlements file — which is why that file
#     contains nothing but `false` values: it is the list of *exceptions* to a
#     restriction this flag turns on, and Folio takes none.
#   * `--timestamp` asks Apple's time stamp server to countersign. Without it
#     the signature stops verifying when the certificate expires, which for a
#     Developer ID certificate is five years after a release nobody will rebuild.
#     Measured on an ad-hoc run: `codesign` accepts the flag and the result
#     carries `Signature=adhoc` with no time stamp in it — there is no
#     certificate for a time stamp to be *about* — so the flag is passed
#     unconditionally rather than switched on the identity, and the ad-hoc
#     signature simply has nothing to countersign.
#   * `--entitlements` is `packaging/macos/entitlements.plist` unless a caller
#     says otherwise. It is given to the **bundle**, which is where the main
#     executable's entitlements come from; see the nested-code rule below for
#     why it is not given to anything else.
#   * `--force` replaces a signature that is already there. A bundle assembled
#     by `bundle.sh` has none, but a re-sign of an already signed tree is the
#     ordinary case in a release lane that retries, and without this the second
#     run fails on a file the first one signed.
#
# ## The nested-code order, which matters before there is any nested code
#
# `codesign` seals a bundle by hashing what is inside it. So anything inside
# that carries its own signature must already carry its final one when the
# bundle is sealed, or the seal covers a signature that is about to be replaced
# and `--verify --deep` reports the bundle as modified. The order is therefore
# fixed, and it is inside out:
#
#   1. **Innermost first.** Nested code, deepest path first: frameworks, XPC
#      services, plug-ins, login items, helper applications, and any loose
#      dynamic library or Mach-O executable that is not the main one. A
#      framework that itself contains a framework is reached first by the same
#      rule, because its path is longer.
#   2. **Then the main executable.** On this bundle that step is not a separate
#      command: `Contents/MacOS/folio` is what `CFBundleExecutable` names, and
#      signing the bundle in 3 is what signs it. The step exists as a step for
#      the case that makes it one — a *second* binary in `Contents/MacOS/`,
#      which is nested code by 1 and must be signed before the bundle whatever
#      it is called.
#   3. **Then the bundle**, which seals everything above and receives the
#      entitlements.
#
# **Folio has no nested code today.** One executable with every Rust crate linked
# into it, an `.icns`, an `Info.plist` and a `PkgInfo`; `otool -L` on the
# executable names only `/System/Library/Frameworks` and `/usr/lib`, which are
# Apple's and are not this project's to sign. The walk below therefore finds
# nothing and says so — which is a fact worth printing at every release, because
# the day it stops being true is the day this order starts mattering and nobody
# would otherwise notice.
#
# Nested code is signed **without this bundle's entitlements**. Entitlements are
# a property of a process, and a framework or a library is not one; a helper
# application or an XPC service *is* one and brings its own file, which is a
# `--entitlements` this script would grow a way to name when such a helper
# appears. Guessing that the app's entitlements are also the helper's is how a
# helper ends up asking for something it was never reviewed for.
#
# `--deep` appears in the verification below and **never** in a signing command.
# Apple's own guidance is that `codesign --deep -s` is for repairing somebody
# else's broken bundle: it signs everything it finds with one set of options and
# one set of entitlements, which is precisely the guess the paragraph above
# refuses. For *verification* it is the right flag and the plan's acceptance
# line names it.
#
# ## What this script exits with
#
# `codesign --verify --deep --strict` failing is a failure on any identity.
# `spctl` is the one that depends: an ad-hoc signature is *correctly* rejected
# by Gatekeeper — measured, `spctl -a -vvv` prints `rejected` and exits 3 — so
# on `--identity -` that answer is printed and the script still exits 0. With a
# real identity the same answer is a release that must not ship, and the exit
# code says so. Before notarization even a real Developer ID signature is
# rejected with `source=Unnotarized Developer ID`; that is M5-2's step, and the
# message `spctl` prints names which of the two it is.

set -eu

usage() {
	echo "usage: sign.sh --app <path to Folio.app> [--identity <id>] [--entitlements <path>] [--no-spctl]" >&2
	exit 2
}

app=""
identity="-"
entitlements=""
run_spctl=1

while [ $# -gt 0 ]; do
	case "$1" in
	--app)
		[ $# -ge 2 ] || usage
		app="$2"
		shift 2
		;;
	--identity)
		[ $# -ge 2 ] || usage
		identity="$2"
		shift 2
		;;
	--entitlements)
		[ $# -ge 2 ] || usage
		entitlements="$2"
		shift 2
		;;
	--no-spctl)
		run_spctl=0
		shift
		;;
	-h | --help) usage ;;
	*)
		echo "sign.sh: unknown argument $1" >&2
		usage
		;;
	esac
done

[ -n "$app" ] || usage

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)

case "$(uname -s)" in
Darwin) ;;
*)
	echo "sign.sh: codesign and spctl are macOS tools" >&2
	exit 1
	;;
esac

if [ ! -d "$app" ]; then
	echo "sign.sh: no bundle at $app" >&2
	exit 1
fi
app=$(cd "$app" && pwd)

[ -n "$entitlements" ] || entitlements="$repo/packaging/macos/entitlements.plist"
if [ ! -f "$entitlements" ]; then
	echo "sign.sh: no entitlements file at $entitlements" >&2
	exit 1
fi

if [ "$identity" = "-" ]; then
	echo "sign.sh: ad-hoc identity — this proves the shape of the signature and not its publisher"
else
	echo "sign.sh: identity $identity"
fi
echo "sign.sh: entitlements $entitlements"
echo

# The main executable, from the bundle's own declaration rather than from the
# name this project happens to use.
main=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")
echo "sign.sh: CFBundleExecutable is $main"

# ---------------------------------------------------------------------------
# 1. Nested code, deepest path first.
# ---------------------------------------------------------------------------
#
# Two kinds, found by two questions because they answer to two different ones. A
# nested *bundle* is known by its extension, and signing it signs what is inside
# it — so the walk `-prune`s at one rather than descending, and what is inside it
# never reaches this list. A loose Mach-O is known by asking `file`, because a
# helper binary has no extension to be known by.
nested=$(mktemp "${TMPDIR:-/tmp}/folio-nested.XXXXXX")
trap 'rm -f "$nested"' EXIT INT TERM

kinds='-name *.framework -o -name *.xpc -o -name *.appex -o -name *.bundle -o -name *.app -o -name *.plugin'

# shellcheck disable=SC2086 # `$kinds` is this script's own literal, split on purpose.
find "$app/Contents" \( $kinds \) -prune -print >"$nested"

# shellcheck disable=SC2086
find "$app/Contents" \( $kinds \) -prune -o -type f -print | while IFS= read -r file; do
	if [ "$file" = "$app/Contents/MacOS/$main" ]; then
		continue
	fi
	case "$(file -b "$file")" in
	*Mach-O*) echo "$file" ;;
	esac
done >>"$nested"

# Deepest first: count the separators, sort descending, drop the count.
ordered=$(awk -F/ '{ print NF, $0 }' "$nested" | sort -rn | cut -d' ' -f2- || true)

if [ -z "$ordered" ]; then
	echo "sign.sh: no nested code in this bundle — one executable, and the bundle signs it"
else
	echo "sign.sh: nested code, innermost first:"
	echo "$ordered" | sed 's|^|  |'
	echo "$ordered" | while IFS= read -r item; do
		[ -n "$item" ] || continue
		echo "sign.sh: signing nested $item"
		codesign --force --options runtime --timestamp -s "$identity" "$item"
	done
fi

rm -f "$nested"
trap - EXIT INT TERM

# ---------------------------------------------------------------------------
# 2 and 3. The bundle, which seals the tree and carries the entitlements.
# ---------------------------------------------------------------------------
echo
echo "sign.sh: signing the bundle"
codesign --force --options runtime --timestamp \
	--entitlements "$entitlements" \
	-s "$identity" "$app"

# ---------------------------------------------------------------------------
# Read it back.
# ---------------------------------------------------------------------------
echo
echo "=== codesign --verify --deep --strict --verbose=2"
codesign --verify --deep --strict --verbose=2 "$app"

echo
echo "=== codesign -dv --verbose=4"
codesign -dv --verbose=4 "$app" 2>&1

echo
# The plan's acceptance line spells this `--entitlements :-`; on Xcode 26 that
# spelling still works and prints `warning: Specifying ':' in the path is
# deprecated and will not work in a future release`. `--entitlements - --xml`
# is the same document with no warning and no colon, so that is what runs.
echo "=== codesign -d --entitlements - --xml"
codesign -d --entitlements - --xml "$app" 2>&1

if [ "$run_spctl" = "0" ]; then
	echo
	echo "sign.sh: spctl skipped (--no-spctl)"
	exit 0
fi

echo
echo "=== spctl -a -vvv"
set +e
spctl -a -vvv "$app" 2>&1
spctl_rc=$?
set -e
echo "spctl exit $spctl_rc"

if [ "$spctl_rc" = "0" ]; then
	echo "sign.sh: Gatekeeper accepts this bundle"
	exit 0
fi

if [ "$identity" = "-" ]; then
	echo "sign.sh: rejected, and that is the expected answer for an ad-hoc signature —"
	echo "         Gatekeeper asks who signed it and an ad-hoc signature has no answer."
	exit 0
fi

echo "sign.sh: Gatekeeper refused a signature made with a real identity." >&2
echo "         Before notarization this is expected and the line above says" >&2
echo "         'source=Unnotarized Developer ID' — run notarize.sh and try again." >&2
exit 1
