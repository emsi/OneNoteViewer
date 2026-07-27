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
   unreviewed binary corpus.
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

Dependency/license audit remains a release gate and cannot pass until the
project source license is selected.

The source-code license must be selected before external code contributions
are accepted.
