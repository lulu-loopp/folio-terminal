#!/bin/sh
# Build and stage the Linux user-install artifacts from this checkout.

set -eu

usage() {
	echo "usage: build-linux.sh [--out <artifact directory>]" >&2
	exit "${1:-2}"
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
out="$repo_root/target/linux-release"

while [ "$#" -gt 0 ]; do
	case "$1" in
	--out)
		[ "$#" -ge 2 ] || usage
		out=$2
		shift 2
		;;
	-h | --help)
		usage 0
		;;
	*)
		echo "build-linux.sh: unknown argument $1" >&2
		usage
		;;
	esac
done

case "$out" in
	/*) ;;
	*) out="$PWD/$out" ;;
esac

[ "$(uname -s)" = Linux ] || {
	echo "build-linux.sh: build on Linux so the artifact links to the Linux host" >&2
	exit 1
}
[ "$(uname -m)" = x86_64 ] || {
	echo "build-linux.sh: this release recipe targets x86_64-unknown-linux-gnu" >&2
	exit 1
}

target=x86_64-unknown-linux-gnu
channel=$(sed -n 's/^channel = "\([^"]*\)".*/\1/p' "$repo_root/rust-toolchain.toml")
[ -n "$channel" ] || {
	echo "build-linux.sh: rust-toolchain.toml has no channel pin" >&2
	exit 1
}
version=${channel%%-*}
toolchain="$version-$target"

for source in \
	"$repo_root/assets/linux/folio.desktop" \
	"$repo_root/assets/app-icon/folio-1024.png" \
	"$repo_root/LICENSE-MIT" \
	"$repo_root/LICENSE-APACHE" \
	"$repo_root/THIRD-PARTY-NOTICES.md" \
	"$repo_root/TRADEMARK.md"; do
	[ -f "$source" ] || {
		echo "build-linux.sh: required source is missing: $source" >&2
		exit 1
	}
done

rustup toolchain install "$toolchain" --profile minimal --no-self-update
target_dir=${CARGO_TARGET_DIR:-"$repo_root/target"}
case "$target_dir" in
	/*) ;;
	*) target_dir="$repo_root/$target_dir" ;;
esac

(
	cd "$repo_root"
	RUSTUP_TOOLCHAIN="$toolchain" \
		RUSTFLAGS='-C target-feature=-crt-static' \
		CARGO_INCREMENTAL=0 \
		CARGO_TARGET_DIR="$target_dir" \
		cargo build --release --locked --target "$target" -j 2 -p bt-app
)

binary="$target_dir/$target/release/folio"
[ -x "$binary" ] || {
	echo "build-linux.sh: cargo did not produce $binary" >&2
	exit 1
}
version_line=$("$binary" --version)
case "$version_line" in
	"Folio "*) ;;
	*)
		echo "build-linux.sh: unexpected --version answer: $version_line" >&2
		exit 1
		;;
esac

install -d \
	"$out/bin" \
	"$out/share/applications" \
	"$out/share/icons/hicolor/512x512@2/apps" \
	"$out/share/doc/folio"
install -m 0755 "$binary" "$out/bin/folio"
install -m 0644 \
	"$repo_root/assets/linux/folio.desktop" \
	"$out/share/applications/io.github.lulu-loopp.folio.desktop"
install -m 0644 \
	"$repo_root/assets/app-icon/folio-1024.png" \
	"$out/share/icons/hicolor/512x512@2/apps/io.github.lulu-loopp.folio.png"
for notice in LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md TRADEMARK.md; do
	install -m 0644 "$repo_root/$notice" "$out/share/doc/folio/$notice"
done

echo "Built $version_line"
echo "Staged Linux user-install files in $out"
find "$out" -type f -print | sort
