#!/bin/sh
set -eu

status=0

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'missing command: %s\n' "$1" >&2
        status=1
    fi
}

require_pkg_config() {
    module=$1
    version=$2
    package_hint=$3
    if ! pkg-config --exists "$module >= $version"; then
        printf 'missing system library: %s >= %s (install %s)\n' \
            "$module" "$version" "$package_hint" >&2
        status=1
    fi
}

require_command cargo
require_command pkg-config

if command -v pkg-config >/dev/null 2>&1; then
    require_pkg_config gtk4 4.14 libgtk-4-dev
    require_pkg_config graphene-gobject-1.0 1.10 libgraphene-1.0-dev
fi

if ! command -v 7zz >/dev/null 2>&1 &&
    ! command -v 7z >/dev/null 2>&1; then
    printf '%s\n' \
        'optional dependency missing: 7zz/7z (install 7zip for .onepkg import)' >&2
fi

if [ "$status" -eq 0 ]; then
    printf 'Required OneNote Viewer build dependencies are available.\n'
fi

exit "$status"
