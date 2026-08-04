# Contributing

OneNote files are untrusted binary input and often contain private data.
Changes should be narrow, testable, and explicit about degraded behavior.

Before submitting implementation work:

1. Read `docs/MASTER-PLAN.md`, `docs/specs/onenote-format.md`, and
   `docs/limitations.md`.
2. Add or identify a legal fixture for each format behavior.
3. Keep parser internals behind `onenote-core`.
4. Keep GTK out of `onenote-core`, `onenote-render`, and `onenote-index`, and
   keep SQLite schema details out of public query types.
5. Treat public API changes as compatibility work: update documentation,
   independent-consumer tests, versioned protocol fixtures, and migration
   notes.
6. Never commit a personal notebook, user text, author identity, or an
   unreviewed binary corpus. Private-corpus tests must use environment-provided
   paths and aggregate structural assertions; do not copy private notebook,
   section, page, author, or content strings into code, tests, comments,
   snapshots, diagnostics, or documentation.
7. Prefer a structured warning over silently dropping unknown content.
8. Add resource-limit and malformed-input tests for new decoders.
9. Update the feature matrix, limitation entry, and corpus matrix together.
10. After changing `Cargo.lock`, run `./scripts/update-flatpak-sources.sh` and
    commit the regenerated Flatpak source manifest.

Run:

```sh
./scripts/validate-docs.sh
```

Required implementation checks are:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo doc --workspace --no-deps
git diff --check
```

Contributions are accepted under the project's GPL-3.0-or-later license unless
a file is explicitly governed by a compatible third-party license. Preserve
all existing notices and update `THIRD-PARTY-NOTICES.md` when adding or
changing third-party material.

After changing `Cargo.lock` or dependency licensing, install the pinned audit
tool and refresh the checked-in report:

```sh
cargo +stable install cargo-about --version 0.9.1 --locked --features cli
./scripts/check-licenses.sh --update
./scripts/check-licenses.sh
```
