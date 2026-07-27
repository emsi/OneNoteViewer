#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

generator_commit=737c0085912f9f7dabf9341d4608e2a77a51a73a
generator_sha256=b373c8ab1a05378ec5d8ed0645c7b127bcec7d2f7a1798694fbc627d570d856c
generator=$(mktemp)
output=$(mktemp)
trap 'rm -f "$generator" "$output"' EXIT HUP INT TERM

curl \
    --fail \
    --location \
    --silent \
    --show-error \
    "https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/$generator_commit/cargo/flatpak-cargo-generator.py" \
    --output "$generator"
printf '%s  %s\n' "$generator_sha256" "$generator" | sha256sum --check --status
uv run \
    --with aiohttp \
    --with tomlkit \
    python "$generator" Cargo.lock -o "$output"
install -m 0644 "$output" packaging/flatpak/cargo-sources.json
printf '%s\n' 'Updated packaging/flatpak/cargo-sources.json'
