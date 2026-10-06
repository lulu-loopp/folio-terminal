#!/bin/sh
# Remove Folio's per-user installation files and leave its data untouched.

set -eu

usage() {
	echo "usage: uninstall-linux.sh [--prefix <test/local prefix>]" >&2
	exit "${1:-2}"
}

prefix=""
while [ "$#" -gt 0 ]; do
	case "$1" in
	--prefix)
		[ "$#" -ge 2 ] || usage
		prefix=$2
		shift 2
		;;
	-h | --help)
		usage 0
		;;
	*)
		echo "uninstall-linux.sh: unknown argument $1" >&2
		usage
		;;
	esac
done

if [ -n "$prefix" ]; then
	case "$prefix" in
		/*) ;;
		*) prefix="$PWD/$prefix" ;;
	esac
	bin_dir="$prefix/bin"
	data_home="$prefix/share"
else
	[ -n "${HOME:-}" ] || {
		echo "uninstall-linux.sh: HOME is unset" >&2
		exit 1
	}
	bin_dir="$HOME/.local/bin"
	case "${XDG_DATA_HOME:-}" in
	/*) data_home=$XDG_DATA_HOME ;;
	*) data_home="$HOME/.local/share" ;;
	esac
fi

desktop_dir="$data_home/applications"
icon_dir="$data_home/icons/hicolor/512x512@2/apps"
doc_dir="$data_home/doc/folio"

rm -f "$bin_dir/folio"
rm -f "$desktop_dir/io.github.lulu-loopp.folio.desktop"
rm -f "$icon_dir/io.github.lulu-loopp.folio.png"
rm -f \
	"$doc_dir/LICENSE-MIT" \
	"$doc_dir/LICENSE-APACHE" \
	"$doc_dir/THIRD-PARTY-NOTICES.md" \
	"$doc_dir/TRADEMARK.md"

# Remove only directories that became empty after the owned files were removed.
rmdir "$doc_dir" 2>/dev/null || true
rmdir "$icon_dir" 2>/dev/null || true
rmdir "$(dirname -- "$icon_dir")" 2>/dev/null || true
rmdir "$desktop_dir" 2>/dev/null || true

echo "Removed Folio's binary, desktop entry, icon and bundled notices."
echo "Folio settings, sessions and other user data were left in place."
