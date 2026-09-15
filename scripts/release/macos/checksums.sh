#!/bin/sh
#
# Hash what the release page is made of, in the format `shasum -c` reads — from
# *inside* the directory, so that every line names a bare file name.
#
# usage: checksums.sh --dir <dir> [--out <name>]
#
# ## Why this is a script and not a line in a launcher
#
# It was a line in a launcher, and the launcher passed a path:
#
#     shasum -a 256 "$PKG/Folio-$VERSION-macos-arm64.dmg"
#
# `shasum` writes back the name it was given, so 0.4.0's `SHA256SUMS-macos.txt`
# went up reading
#
#     <hash>  target/macos-package/Folio-0.4.0-macos-arm64.dmg
#
# and a reader who downloaded both files into one folder and ran
# `shasum -c SHA256SUMS-macos.txt` was answered
# `target/macos-package/...: No such file or directory`. The file was rewritten
# by hand before the page went up, which is a format nobody can rely on twice.
#
# `cd` first and hash bare names, and the line is one a reader can check where
# they are standing. It is also the line `scripts/release/package.ps1` writes on
# the Windows side — hash, two spaces, name — so both halves of a release
# publish one format and a reader learns it once.
#
# ## What goes in it
#
# Every regular file in the directory except the checksum file itself, which
# cannot carry its own hash. That is the Windows rule as well: the directory is
# the release page, so what is published is what is hashed, and because no list
# of names is typed anywhere no asset can be left out of one.
#
# ## Read back
#
# `shasum -c` is run over the file that was just written, from the directory it
# was written in. It is the only check that the format is the format this script
# claims — a hash nobody has verified is a string.

set -eu

usage() {
	echo "usage: checksums.sh --dir <dir> [--out <name>]" >&2
	exit 2
}

dir=""
out="SHA256SUMS-macos.txt"

while [ $# -gt 0 ]; do
	case "$1" in
	--dir)
		[ $# -ge 2 ] || usage
		dir="$2"
		shift 2
		;;
	--out)
		[ $# -ge 2 ] || usage
		out="$2"
		shift 2
		;;
	-h | --help) usage ;;
	*)
		echo "checksums.sh: unknown argument $1" >&2
		usage
		;;
	esac
done

[ -n "$dir" ] || usage

if [ ! -d "$dir" ]; then
	echo "checksums.sh: no directory at $dir" >&2
	exit 1
fi

# The output is a name and not a path, because the whole point of this script is
# that the file sits in the directory it is about and names its neighbours.
case "$out" in
*/* | "")
	echo "checksums.sh: --out is a name inside the directory, not a path: $out" >&2
	exit 1
	;;
esac

cd "$dir"

echo "checksums.sh: $(pwd)/$out"
echo

# Written to a second name and moved into place, so that a run interrupted
# halfway leaves the previous file rather than half of a new one — and so that
# the loop below cannot hash a file it is in the middle of writing.
tmp="$out.partial"
rm -f "$tmp"
for file in *; do
	[ -f "$file" ] || continue
	[ "$file" != "$out" ] || continue
	[ "$file" != "$tmp" ] || continue
	shasum -a 256 "$file" >>"$tmp"
done

if [ ! -s "$tmp" ]; then
	rm -f "$tmp"
	echo "checksums.sh: there is nothing in $dir to hash" >&2
	exit 1
fi

mv "$tmp" "$out"
cat "$out"

echo
echo "=== shasum -c $out"
shasum -c "$out"
