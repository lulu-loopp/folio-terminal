#!/bin/sh
# Package a staged Linux build for a workflow artifact.

set -eu
LC_ALL=C
export LC_ALL

usage() {
	echo "usage: package-linux.sh --from <stage> --out <directory> --version <version> --commit <sha> --runner <label>" >&2
	exit "${1:-2}"
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
from=""
out=""
version=""
commit=""
runner=""

while [ "$#" -gt 0 ]; do
	case "$1" in
	--from)
		[ "$#" -ge 2 ] || usage
		from=$2
		shift 2
		;;
	--out)
		[ "$#" -ge 2 ] || usage
		out=$2
		shift 2
		;;
	--version)
		[ "$#" -ge 2 ] || usage
		version=$2
		shift 2
		;;
	--commit)
		[ "$#" -ge 2 ] || usage
		commit=$2
		shift 2
		;;
	--runner)
		[ "$#" -ge 2 ] || usage
		runner=$2
		shift 2
		;;
	-h | --help)
		usage 0
		;;
	*)
		echo "package-linux.sh: unknown argument $1" >&2
		usage
		;;
	esac
done

[ -n "$from" ] && [ -n "$out" ] && [ -n "$version" ] && [ -n "$commit" ] && [ -n "$runner" ] || usage
case "$from" in /*) ;; *) from="$PWD/$from" ;; esac
case "$out" in /*) ;; *) out="$PWD/$out" ;; esac

binary="$from/bin/folio"
for source in \
	"$binary" \
	"$from/share/applications/io.github.lulu-loopp.folio.desktop" \
	"$from/share/icons/hicolor/512x512@2/apps/io.github.lulu-loopp.folio.png" \
	"$from/share/doc/folio/LICENSE-MIT" \
	"$from/share/doc/folio/LICENSE-APACHE" \
	"$from/share/doc/folio/THIRD-PARTY-NOTICES.md" \
	"$from/share/doc/folio/TRADEMARK.md"; do
	[ -f "$source" ] || {
		echo "package-linux.sh: required staged file is missing: $source" >&2
		exit 1
	}
done
[ -x "$binary" ] || {
	echo "package-linux.sh: staged folio is not executable: $binary" >&2
	exit 1
}

version_line=$("$binary" --version)
case "$version_line" in
	"Folio $version ("*) ;;
	*)
		echo "package-linux.sh: staged version does not match $version: $version_line" >&2
		exit 1
		;;
esac

build_host=$(awk -F= '$1 == "PRETTY_NAME" { sub(/^"/, "", $2); sub(/"$/, "", $2); print $2; exit }' /etc/os-release)
host_glibc=$(ldd --version 2>&1 | sed -n '1p')
highest_glibc=$(readelf --version-info "$binary" | grep -oE 'GLIBC_[0-9]+(\.[0-9]+)+' | sort -V | tail -n 1)
needed=$(
	readelf --dynamic "$binary" \
		| sed -n 's/.*Shared library: \[\(.*\)\].*/\1/p' \
		| awk '
			BEGIN { separator = "" }
			{ printf "%s%s", separator, $0; separator = ", " }
			END { if (NR) printf "\n" }
		'
)
source_epoch=$(git -C "$repo_root" show -s --format=%ct "$commit")
[ -n "$build_host" ] && [ -n "$host_glibc" ] && [ -n "$highest_glibc" ] || {
	echo "package-linux.sh: could not measure the build host or ELF ABI" >&2
	exit 1
}

archive_root="folio-$version-linux-x86_64"
archive_name="$archive_root.tar.gz"
archive_dir="$out"
archive="$archive_dir/$archive_name"
archive_checksum="$archive.sha256"
[ ! -e "$archive" ] || {
	echo "package-linux.sh: archive already exists: $archive" >&2
	exit 1
}
[ ! -e "$archive_checksum" ] || {
	echo "package-linux.sh: checksum already exists: $archive_checksum" >&2
	exit 1
}

mkdir -p "$archive_dir"
work=$(mktemp -d "$archive_dir/.folio-linux-package.XXXXXX")
trap 'rm -rf "$work"' 0 HUP INT TERM
package_root="$work/$archive_root"
install -d "$package_root"
cp -a "$from/." "$package_root/"
install -d "$package_root/scripts/release" "$package_root/docs"
install -m 0755 "$repo_root/scripts/release/install-linux.sh" "$package_root/scripts/release/install-linux.sh"
install -m 0755 "$repo_root/scripts/release/uninstall-linux.sh" "$package_root/scripts/release/uninstall-linux.sh"
install -m 0644 "$repo_root/docs/linux.md" "$package_root/docs/linux.md"
install -m 0644 "$repo_root/docs/linux.zh-CN.md" "$package_root/docs/linux.zh-CN.md"

{
	printf 'Folio version: %s\n' "$version"
	printf 'Source commit: %s\n' "$commit"
	printf 'Binary version line: %s\n' "$version_line"
	printf 'Build runner label: %s\n' "$runner"
	printf 'Build host: %s\n' "$build_host"
	printf 'Build host glibc: %s\n' "$host_glibc"
	printf 'Highest GLIBC symbol version referenced by this ELF: %s\n' "$highest_glibc"
	printf 'ELF DT_NEEDED libraries: %s\n' "$needed"
} >"$package_root/BUILD-INFO.txt"

(
	cd "$package_root"
	find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum >SHA256SUMS
)
tar --sort=name --mtime="@$source_epoch" --owner=0 --group=0 --numeric-owner \
	-cf "$work/$archive_root.tar" -C "$work" "$archive_root"
gzip -n -c "$work/$archive_root.tar" >"$work/$archive_name"
(
	cd "$work"
	sha256sum "$archive_name" >"$archive_name.sha256"
)
mv "$work/$archive_name" "$archive"
mv "$work/$archive_name.sha256" "$archive_checksum"

archive_size=$(wc -c <"$archive" | tr -d '[:space:]')
echo "Packaged $version_line"
echo "Archive: $archive ($archive_size bytes)"
cat "$archive_checksum"
