#!/bin/sh
# Install a staged Linux build in the current user's XDG locations.

set -eu

usage() {
	echo "usage: install-linux.sh [--from <artifact directory>] [--prefix <test/local prefix>]" >&2
	exit "${1:-2}"
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
artifacts="$repo_root/target/linux-release"
prefix=""

while [ "$#" -gt 0 ]; do
	case "$1" in
	--from)
		[ "$#" -ge 2 ] || usage
		artifacts=$2
		shift 2
		;;
	--prefix)
		[ "$#" -ge 2 ] || usage
		prefix=$2
		shift 2
		;;
	-h | --help)
		usage 0
		;;
	*)
		echo "install-linux.sh: unknown argument $1" >&2
		usage
		;;
	esac
done

case "$artifacts" in
	/*) ;;
	*) artifacts="$PWD/$artifacts" ;;
esac

if [ -n "$prefix" ]; then
	case "$prefix" in
		/*) ;;
		*) prefix="$PWD/$prefix" ;;
	esac
	bin_dir="$prefix/bin"
	data_home="$prefix/share"
else
	[ -n "${HOME:-}" ] || {
		echo "install-linux.sh: HOME is unset" >&2
		exit 1
	}
	bin_dir="$HOME/.local/bin"
	case "${XDG_DATA_HOME:-}" in
	/*) data_home=$XDG_DATA_HOME ;;
	*) data_home="$HOME/.local/share" ;;
	esac
fi

binary="$artifacts/bin/folio"
desktop_source="$artifacts/share/applications/io.github.lulu-loopp.folio.desktop"
icon_source="$artifacts/share/icons/hicolor/512x512@2/apps/io.github.lulu-loopp.folio.png"
desktop_dir="$data_home/applications"
icon_dir="$data_home/icons/hicolor/512x512@2/apps"
doc_dir="$data_home/doc/folio"
installed_binary="$bin_dir/folio"

for source in \
	"$binary" \
	"$desktop_source" \
	"$icon_source" \
	"$artifacts/share/doc/folio/LICENSE-MIT" \
	"$artifacts/share/doc/folio/LICENSE-APACHE" \
	"$artifacts/share/doc/folio/THIRD-PARTY-NOTICES.md" \
	"$artifacts/share/doc/folio/TRADEMARK.md"; do
	[ -f "$source" ] || {
		echo "install-linux.sh: required build artifact is missing: $source" >&2
		exit 1
	}
done
[ -x "$binary" ] || {
	echo "install-linux.sh: staged folio is not executable: $binary" >&2
	exit 1
}
version_line=$("$binary" --version)
case "$version_line" in
	"Folio "*) ;;
	*)
		echo "install-linux.sh: staged binary did not identify itself as Folio" >&2
		exit 1
		;;
esac

case "$installed_binary" in
	*=*)
		echo "install-linux.sh: the desktop executable path may not contain '=': $installed_binary" >&2
		exit 1
		;;
esac
if printf '%s' "$installed_binary" \
	| LC_ALL=C od -An -v -tu1 \
	| awk '{ for (i = 1; i <= NF; i++) if ($i < 32 || $i == 127) found = 1 } END { exit !found }'; then
	echo "install-linux.sh: the desktop Exec path may not contain control characters" >&2
	exit 1
fi

desktop_exec_argument() {
	escaped_exec=$(printf '%s' "$1" | sed \
		-e 's/\\/\\\\/g' \
		-e 's/"/\\"/g' \
		-e 's/[$]/\\$/g' \
		-e 's/`/\\`/g' \
		-e 's/%/%%/g')
	escaped=$(printf '%s' "$escaped_exec" | sed -e 's/\\/\\\\/g')
	printf '"%s"' "$escaped"
}

install -d "$bin_dir" "$desktop_dir" "$icon_dir" "$doc_dir"
install -m 0755 "$binary" "$installed_binary"
install -m 0644 "$icon_source" "$icon_dir/io.github.lulu-loopp.folio.png"
for notice in LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md TRADEMARK.md; do
	install -m 0644 "$artifacts/share/doc/folio/$notice" "$doc_dir/$notice"
done

desktop_tmp=$(mktemp "$desktop_dir/.io.github.lulu-loopp.folio.desktop.XXXXXX")
trap 'rm -f "$desktop_tmp"' 0 HUP INT TERM
exec_argument=$(desktop_exec_argument "$installed_binary")
while IFS= read -r line || [ -n "$line" ]; do
	case "$line" in
	'Exec=/usr/bin/env -- folio') printf 'Exec=/usr/bin/env -- %s\n' "$exec_argument" ;;
	*) printf '%s\n' "$line" ;;
	esac
done <"$desktop_source" >"$desktop_tmp"
install -m 0644 "$desktop_tmp" "$desktop_dir/io.github.lulu-loopp.folio.desktop"
rm -f "$desktop_tmp"
trap - 0 HUP INT TERM

echo "Installed $version_line"
echo "  binary: $installed_binary"
echo "  desktop: $desktop_dir/io.github.lulu-loopp.folio.desktop"
echo "  icon: $icon_dir/io.github.lulu-loopp.folio.png"
echo "  notices: $doc_dir"
