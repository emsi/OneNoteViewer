#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
dest="$root/docs/references/microsoft"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

mkdir -p "$dest"
mkdir -p "$tmp/docs/references/microsoft"

fetch() {
    output=$1
    url=$2
    curl --fail --location --retry 3 \
        --output "$tmp/docs/references/microsoft/$output" "$url"
}

fetch MS-CAB-v20110304.pdf \
    'https://download.microsoft.com/download/4/d/a/4da14f27-b4ef-4170-a6e6-5b1ef85b1baa/%5Bms-cab%5D.pdf'
fetch MS-ONE-v20221115.pdf \
    'https://officeprotocoldocs-f5hpbjgea6b8gneq.b02.azurefd.net/files/MS-ONE/%5BMS-ONE%5D.pdf'
fetch MS-ONESTORE-v20250520.pdf \
    'https://officeprotocoldocs-f5hpbjgea6b8gneq.b02.azurefd.net/files/MS-ONESTORE/%5BMS-ONESTORE%5D.pdf'
fetch MS-FSSHTTPB-v20240820.pdf \
    'https://officeprotocoldocs-f5hpbjgea6b8gneq.b02.azurefd.net/files/MS-FSSHTTPB/%5BMS-FSSHTTPB%5D.pdf'
fetch MS-DOC-v20260217.pdf \
    'https://officeprotocoldocs-f5hpbjgea6b8gneq.b02.azurefd.net/files/MS-DOC/%5BMS-DOC%5D.pdf'
fetch MS-LCID-v20240423.pdf \
    'https://winprotocoldocs-bhdugrdyduf5h2e4.b02.azurefd.net/MS-LCID/%5BMS-LCID%5D.pdf'
fetch MS-OSHARED-v20251113.pdf \
    'https://officeprotocoldocs-f5hpbjgea6b8gneq.b02.azurefd.net/files/MS-OSHARED/%5BMS-OSHARED%5D.pdf'
fetch MS-DTYP-v20241119.pdf \
    'https://winprotocoldocs-bhdugrdyduf5h2e4.b02.azurefd.net/MS-DTYP/%5BMS-DTYP%5D.pdf'
fetch MS-OFFCRYPTO-v20260217.pdf \
    'https://officeprotocoldocs-f5hpbjgea6b8gneq.b02.azurefd.net/files/MS-OFFCRYPTO/%5BMS-OFFCRYPTO%5D.pdf'
fetch Ink-Serialized-Format.pdf \
    'https://download.microsoft.com/download/0/B/E/0BE8BDD7-E5E8-422A-ABFD-4342ED7AD886/InkSerializedFormat%28ISF%29Specification.pdf'

cd "$tmp"
sha256sum --check "$root/docs/references/SHA256SUMS"

# Publish only after every download matches the pinned archive. A newer
# upstream document therefore cannot overwrite a file under an old version.
cp docs/references/microsoft/*.pdf "$dest/"
