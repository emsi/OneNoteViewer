#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

for command in flatpak flatpak-builder; do
    if ! command -v "$command" >/dev/null 2>&1; then
        printf 'missing build command: %s\n' "$command" >&2
        printf '%s\n' 'Ubuntu: sudo apt install flatpak flatpak-builder' >&2
        exit 1
    fi
done

if command -v unshare >/dev/null 2>&1 && ! unshare --user --map-root-user true 2>/dev/null; then
    cat >&2 <<'EOF'
Flatpak cannot build in this environment because user namespaces are blocked.
This commonly happens inside an unprivileged Docker or development container;
bubblewrap needs permission to create namespaces even when Flatpak is installed.

Run this script directly on the host, start the container with the privileges
required for nested namespaces, or use the GitHub Actions release workflow.
Changing PKG_CONFIG_PATH or reinstalling Flatpak will not fix this condition.
EOF
    exit 1
fi

version=$(sed -n '
    /^\[workspace\.package\]$/,/^\[/ {
        s/^version = "\([^"]*\)"/\1/p
    }
' Cargo.toml)
if [ -z "$version" ]; then
    printf 'could not determine workspace version\n' >&2
    exit 1
fi

arch=$(flatpak --default-arch)
manifest=packaging/flatpak/io.github.emsi.OneNoteViewer.yml
build_dir=target/flatpak/build
repo_dir=target/flatpak/repo
bundle="dist/OneNoteViewer-${version}-linux-${arch}.flatpak"

flatpak remote-add \
    --user \
    --if-not-exists \
    flathub \
    https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak-builder \
    --user \
    --force-clean \
    --disable-rofiles-fuse \
    --install-deps-from=flathub \
    --repo="$repo_dir" \
    "$build_dir" \
    "$manifest"
mkdir -p dist
flatpak build-bundle \
    "$repo_dir" \
    "$bundle" \
    io.github.emsi.OneNoteViewer \
    --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo
sha256sum "$bundle" >"$bundle.sha256"
printf 'Created %s\n' "$root/$bundle"
