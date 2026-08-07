#!/bin/sh
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
revision=${ONENOTE_VIEWER_SOURCE_REVISION:-}

if [ -z "$revision" ]; then
    revision=$(git -C "$root" rev-parse --verify HEAD)
    if ! git -C "$root" diff --quiet || ! git -C "$root" diff --cached --quiet; then
        revision="${revision}-dirty"
    fi
fi

case "$revision" in
    *-dirty) object_id=${revision%-dirty} ;;
    *) object_id=$revision ;;
esac
case "${#object_id}" in
    40|64) ;;
    *)
        printf 'invalid source revision: %s\n' "$revision" >&2
        exit 1
        ;;
esac
case "$object_id" in
    ''|*[!0-9a-f]*)
        printf 'invalid source revision: %s\n' "$revision" >&2
        exit 1
        ;;
esac

printf '%s\n' "$revision"
