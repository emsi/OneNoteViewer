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

if ! git diff --quiet || ! git diff --cached --quiet; then
    printf '%s\n' \
        'native releases require a committed source tree so the source archive matches the binary' >&2
    exit 1
fi
revision=$(git rev-parse --verify HEAD)

./scripts/check-licenses.sh
cargo build --locked --release -p onenote-viewer

archive="OneNoteViewer-${version}-linux-${arch}.tar.gz"
executable="OneNoteViewer-${version}-linux-${arch}.bin"
source_archive="OneNoteViewer-${version}-source.tar.gz"
staging=$(mktemp -d)
source_tar="$staging/OneNoteViewer-${version}-source.tar"
trap 'rm -rf "$staging"' EXIT HUP INT TERM
directory="$staging/OneNoteViewer-${version}-linux-${arch}"
mkdir -p "$directory"
install -m 0755 target/release/onenote-viewer "$directory/onenote-viewer"
install -m 0644 packaging/native/README.txt "$directory/README.txt"
install -m 0644 LICENSE "$directory/LICENSE"
install -m 0644 SOURCE-CODE.md "$directory/SOURCE-CODE.md"
install -m 0644 THIRD-PARTY-NOTICES.md "$directory/THIRD-PARTY-NOTICES.md"
install -m 0644 THIRD-PARTY-LICENSES.html "$directory/THIRD-PARTY-LICENSES.html"
install -m 0644 crates/onenote-viewer/resources/LUCIDE-LICENSE \
    "$directory/LUCIDE-LICENSE"
install -m 0644 third_party/onenote.rs/LICENSE \
    "$directory/ONENOTE-PARSER-MPL-2.0.txt"
printf '%s\n' "$revision" >"$directory/BUILD-REVISION"
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
git archive \
    --format=tar \
    --prefix="OneNoteViewer-${version}-source/" \
    HEAD >"$source_tar"
gzip -n -c "$source_tar" >"dist/$source_archive"
(
    cd dist
    sha256sum "$archive" >"$archive.sha256"
    sha256sum "$executable" >"$executable.sha256"
    sha256sum "$source_archive" >"$source_archive.sha256"
)
printf 'Created %s\n' "$root/dist/$archive"
printf 'Created %s\n' "$root/dist/$executable"
printf 'Created %s\n' "$root/dist/$source_archive"
