#!/bin/sh
#
# Print the Homebrew cask for a release, so that updating the tap is a copy
# rather than two fields somebody retypes.
#
# usage: cask.sh <version> <dmg sha256> [--file <folio.rb>] [--in-place]
#
# The tap is `lulu-loopp/homebrew-folio` and the file is `Casks/folio.rb`.
# `brew install --cask lulu-loopp/folio/folio` reads it, and the two fields that
# change at every release are the two this prints: `version`, which the download
# URL is built out of, and `sha256`, which Homebrew checks the downloaded image
# against before it unpacks anything.
#
# ## The hash is copied, never recomputed
#
# It is the `Folio-<version>-macos-arm64.dmg` line of `SHA256SUMS-macos.txt` —
# the file `checksums.sh` wrote over the image that was uploaded. A hash taken
# from a second download is a hash of whatever that download was; a hash taken
# from the file beside the image is the claim the release page already makes,
# and `brew fetch --cask` then either agrees with the release page or says so.
# `docs/RELEASING.md` says the same thing about winget's `InstallerSha256`, for
# the same reason.
#
# ## `--file`, and why it is the one to use
#
# With `--file` the cask that is already in the tap is read and only those two
# lines are replaced, so everything the tap has grown since — a `desc` somebody
# improved, a `zap` path, a `depends_on` — survives the release that had nothing
# to say about it. Both lines have to be there and there exactly once, or
# nothing is printed: a cask this script does not recognise is a cask a person
# should look at.
#
# Without `--file` the built-in text below is printed, which is the shape the
# tap carries today. It is the answer to "there is no tap yet" and to "what is
# this file meant to look like", and it is deliberately the second-choice path:
# it can only ever say what was true when it was written.
#
# The URL's `-preview` is part of the tag and not part of the version — every
# release so far has been tagged `v<version>-preview` over a manifest with no
# suffix (`docs/RELEASING.md`, "The tag"). A release tagged otherwise needs that
# line changed once, in the tap, by hand.

set -eu

usage() {
	echo "usage: cask.sh <version> <dmg sha256> [--file <folio.rb>] [--in-place]" >&2
	exit 2
}

version=""
sha=""
file=""
in_place=0

while [ $# -gt 0 ]; do
	case "$1" in
	--file)
		[ $# -ge 2 ] || usage
		file="$2"
		shift 2
		;;
	--in-place)
		in_place=1
		shift
		;;
	-h | --help) usage ;;
	-*)
		echo "cask.sh: unknown argument $1" >&2
		usage
		;;
	*)
		if [ -z "$version" ]; then
			version="$1"
		elif [ -z "$sha" ]; then
			sha="$1"
		else
			echo "cask.sh: unexpected argument $1" >&2
			usage
		fi
		shift
		;;
	esac
done

[ -n "$version" ] || usage
[ -n "$sha" ] || usage

# Both are read off a release page by a person, which is the one place in this
# lane where a typo has nobody to catch it: a version with the `v` still on it
# builds a URL naming `vv0.4.1`, and a hash that lost a character fails for
# every reader with a message about a download rather than about this file.
if ! printf '%s' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
	echo "cask.sh: '$version' is not a version — the manifest's, with no 'v' and no '-preview'" >&2
	exit 1
fi
if ! printf '%s' "$sha" | grep -Eq '^[0-9a-f]{64}$'; then
	echo "cask.sh: '$sha' is not a SHA-256 — 64 lower-case hexadecimal digits," >&2
	echo "         copied from the image's line in SHA256SUMS-macos.txt" >&2
	exit 1
fi

if [ "$in_place" = "1" ] && [ -z "$file" ]; then
	echo "cask.sh: --in-place needs --file, because it is that file it writes back" >&2
	exit 1
fi

# Everything that can refuse is asked before anything is written, so that a
# `--in-place` run either replaces the cask or leaves it exactly as it was.
if [ -n "$file" ]; then
	if [ ! -f "$file" ]; then
		echo "cask.sh: no cask at $file" >&2
		exit 1
	fi
	for field in version sha256; do
		found=$(grep -Ec "^[[:space:]]*$field \"[^\"]*\"" "$file" || true)
		if [ "$found" != "1" ]; then
			echo "cask.sh: $file has $found lines declaring $field, and this edit needs exactly one" >&2
			exit 1
		fi
	done
fi

render() {
	if [ -z "$file" ]; then
		cat <<CASK
cask "folio" do
  version "$version"
  sha256 "$sha"

  url "https://github.com/lulu-loopp/folio-terminal/releases/download/v#{version}-preview/Folio-#{version}-macos-arm64.dmg"
  name "Folio"
  desc "Terminal that typesets formulas where a command prints them, with files previewed beside the prompt"
  homepage "https://github.com/lulu-loopp/folio-terminal"

  depends_on arch: :arm64
  depends_on macos: ">= :sonoma"

  app "Folio.app"

  zap trash: [
    "~/Library/Application Support/Folio",
  ]
end
CASK
		return 0
	fi

	sed -E \
		-e "s|^([[:space:]]*version )\"[^\"]*\"|\1\"$version\"|" \
		-e "s|^([[:space:]]*sha256 )\"[^\"]*\"|\1\"$sha\"|" \
		"$file"
}

if [ "$in_place" = "1" ]; then
	updated="$file.new"
	render >"$updated"
	mv "$updated" "$file"
	echo "cask.sh: $file is now version $version" >&2
	cat "$file"
else
	render
fi
