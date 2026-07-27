#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

if ! command -v cargo-about >/dev/null 2>&1; then
    printf '%s\n' 'missing build command: cargo-about' >&2
    printf '%s\n' \
        'Install: cargo +stable install cargo-about --version 0.9.1 --locked --features cli' >&2
    exit 1
fi

version=$(cargo about --version)
case "$version" in
    *"cargo-about 0.9.1"*) ;;
    *)
        printf 'cargo-about 0.9.1 is required, found: %s\n' "$version" >&2
        exit 1
        ;;
esac

raw_report=$(mktemp)
report=$(mktemp)
trap 'rm -f "$raw_report" "$report"' EXIT HUP INT TERM
cargo about generate \
    about.hbs \
    --workspace \
    --frozen \
    --fail \
    --output-file "$raw_report"
sed \
    -e 's/\r$//' \
    -e 's/[[:blank:]]*$//' \
    "$raw_report" >"$report"

if [ "${1:-}" = "--update" ]; then
    install -m 0644 "$report" THIRD-PARTY-LICENSES.html
    printf '%s\n' 'Updated THIRD-PARTY-LICENSES.html.'
    exit 0
fi
if [ "$#" -ne 0 ]; then
    printf 'usage: %s [--update]\n' "$0" >&2
    exit 2
fi

if ! cmp -s "$report" THIRD-PARTY-LICENSES.html; then
    printf '%s\n' \
        'THIRD-PARTY-LICENSES.html is stale; regenerate it with:' >&2
    printf '%s\n' './scripts/check-licenses.sh --update' >&2
    exit 1
fi

printf '%s\n' 'Dependency licenses and generated notices are current.'
