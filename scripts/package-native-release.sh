#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

case "$(uname -m)" in
    x86_64) arch=x86_64 ;;
    aarch64|arm64) arch=aarch64 ;;
    *)
        printf 'unsupported release architecture: %s\n' "$(uname -m)" >&2
        exit 1
        ;;
esac

version=$(sed -n '
    /^\[workspace\.package\]$/,/^\[/ {
        s/^version = "\([^"]*\)"/\1/p
    }
' Cargo.toml)
if [ -z "$version" ]; then
    printf 'could not determine workspace version\n' >&2
    exit 1
fi

cargo build --locked --release -p onenote-viewer

archive="OneNoteViewer-${version}-linux-${arch}.tar.gz"
executable="OneNoteViewer-${version}-linux-${arch}.bin"
staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT HUP INT TERM
directory="$staging/OneNoteViewer-${version}-linux-${arch}"
mkdir -p "$directory"
install -m 0755 target/release/onenote-viewer "$directory/onenote-viewer"
install -m 0644 packaging/native/README.txt "$directory/README.txt"
install -m 0644 crates/onenote-viewer/resources/LUCIDE-LICENSE \
    "$directory/LUCIDE-LICENSE"
ldd target/release/onenote-viewer |
    sed -E 's/ \(0x[[:xdigit:]]+\)//g' >"$directory/RUNTIME-LIBRARIES.txt"

mkdir -p dist
uncompressed="$staging/$archive.tar"
tar \
    --sort=name \
    --mtime="@${SOURCE_DATE_EPOCH:-0}" \
    --owner=0 \
    --group=0 \
    --numeric-owner \
    -C "$staging" \
    -cf "$uncompressed" \
    "$(basename "$directory")"
gzip -n -c "$uncompressed" >"dist/$archive"
install -m 0755 target/release/onenote-viewer "dist/$executable"
sha256sum "dist/$archive" >"dist/$archive.sha256"
sha256sum "dist/$executable" >"dist/$executable.sha256"
printf 'Created %s\n' "$root/dist/$archive"
printf 'Created %s\n' "$root/dist/$executable"
