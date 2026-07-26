#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

sha256sum --check docs/references/SHA256SUMS

status=0

require_text() {
    file=$1
    text=$2
    if ! grep -Fq "$text" "$file"; then
        printf '%s: missing required documentation invariant: %s\n' \
            "$file" "$text" >&2
        status=1
    fi
}

reject_text() {
    text=$1
    if grep -R -F -n --include='*.md' "$text" README.md docs; then
        printf 'retired documentation wording found: %s\n' "$text" >&2
        status=1
    fi
}

require_text README.md 'docs/MASTER-PLAN.md'
require_text docs/README.md 'MASTER-PLAN.md'
require_text docs/MASTER-PLAN.md 'canonical entry point and master plan'
require_text docs/plans/roadmap.md '../MASTER-PLAN.md'

reject_text 'Specification complete; implementation not started.'
reject_text 'three production crates'
reject_text '`.onepkg` support can enter this milestone'
reject_text '`.onepkg` decompression currently occurs in memory'

for file in README.md docs/*.md docs/*/*.md; do
    while IFS= read -r target; do
        case "$target" in
            http://*|https://*|\#*|'') continue ;;
        esac
        target=${target%%\#*}
        base=$(dirname "$file")
        if [ ! -e "$base/$target" ]; then
            printf '%s: broken local link: %s\n' "$file" "$target" >&2
            status=1
        fi
    done <<EOF
$(sed -n 's/.*](\([^)]*\)).*/\1/p' "$file")
EOF
done

exit "$status"
