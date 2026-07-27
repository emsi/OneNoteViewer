# Repository Layout

## Design Goals

The repository needs strong boundaries around an unusually complex,
untrusted binary format, but it does not need a service architecture. A
modular Cargo workspace separates public domain, layout, GTK rendering, and
index/query APIs from the desktop application. The viewer is one consumer of
those components rather than their owner.

## Current Tree

```text
.
├── .github/workflows/
│   └── release.yml
├── Cargo.toml
├── Cargo.lock
├── crates/
│   ├── onenote-core/
│   │   ├── src/
│   │   └── tests/
│   ├── onenote-index/
│   │   ├── src/
│   │   │   └── bin/
│   │   └── tests/
│   ├── onenote-render/
│   │   ├── src/
│   │   └── tests/
│   ├── onenote-render-gtk/
│   │   ├── src/
│   │   └── examples/
│   └── onenote-viewer/
│       └── src/
├── docs/
│   ├── architecture/
│   ├── decisions/
│   ├── plans/
│   ├── references/
│   └── specs/
├── packaging/
│   ├── flatpak/
│   └── native/
├── scripts/
│   ├── build-flatpak-release.sh
│   ├── check-system-deps.sh
│   ├── fetch-references.sh
│   ├── package-native-release.sh
│   ├── update-flatpak-sources.sh
│   └── validate-docs.sh
└── third_party/
    └── onenote.rs/
```

The source files are currently flat within these small crates. Directory
submodules should be introduced only when ownership becomes clearer, not to
imitate the earlier planned tree.

## Ownership Boundaries

### `onenote-core`

Owns input discovery, parser integration, stable public domain types, content
warnings, source fingerprints, internal-link identities, and attachment
streams. It must not depend on GTK or SQLite.

Its `package` module orchestrates the bounded external extraction defined by
ADR 0002 and then hands the durable native directory to normal discovery. It
does not parse CAB payloads or retain expanded entries in memory.

The upstream parser's types do not cross this crate's public boundary. That
keeps the application insulated from a pre-2.0 parser API and gives one place
to enforce resource limits and normalize units.

### `onenote-index`

Owns SQLite schema migrations, document extraction, FTS query construction,
ranking, snippets, and index lifecycle. It depends on `onenote-core`, never on
GTK. The database is always reconstructible from source notebooks.

Its public library interface owns structured ingestion, refresh, status,
query, cancellation, and result types. A small `onenote-query` binary adapts
that interface to the versioned JSON Lines protocol. SQLite tables and raw FTS
syntax are private implementation details.

### `onenote-render`

Owns OneNote page layout, normalized geometry, retained `PageScene` values,
stacking, hit regions, and accessibility semantics. It depends only on
`onenote-core` and UI-neutral support libraries. It must run in headless tests
and must not depend on GTK, GIO, SQLite, or viewer state.

### `onenote-render-gtk`

Owns Pango text layout, GSK snapshots, viewport culling, pan/zoom interaction,
and the embeddable GTK page widget/controller. It depends on `onenote-render`
and receives all host actions through explicit callbacks. It does not create
windows, choose files, navigate notebooks, or persist a workspace.

### `onenote-viewer`

Owns GTK application state, windows, navigation, virtualized lists, workspace
persistence, desktop integration, and user-visible warnings. It composes all
four library crates. It does not parse binary OneNote structures, construct
private page scenes, or query SQLite directly. Notebook, nested section-group,
and section rows form one typed tree; they are not reconstructed from flattened
display labels.

## Dependency Direction

```text
onenote-viewer ──> onenote-render-gtk ──> onenote-render ──> onenote-core
       ├─────────> onenote-index ─────────────────────────> onenote-core
       └──────────────────────────────────────────────────> onenote-core
```

No dependency points toward `onenote-viewer`. `onenote-core`,
`onenote-render`, and `onenote-index` tests run without a display server.
Canvas layout produces a testable scene description before the GTK adapter
creates render nodes. Public interfaces follow
[ADR 0003](../decisions/0003-reusable-components.md).

## Supporting Directories

- `docs/references/` contains redistributed primary specifications and a
  checksum manifest, not arbitrary web captures.
- A future `fixtures/` contains only small synthetic or explicitly licensed
  data. The current private corpus is supplied through an ignored
  environment-specific path.
- `third_party/onenote.rs/` is the retained MPL-2.0 parser audit snapshot. The
  active Cargo dependency is the public fork at the immutable revision recorded
  in the workspace manifest and lockfile.
- `scripts/` contains native dependency preflight plus reproducible reference
  retrieval, documentation validation, and release packaging entry points, not
  a second build system.
- `packaging/` contains the Flatpak manifest/assets and native
  executable/archive runtime guidance. Generated artifacts are written to
  ignored `dist/`.

## Deliberate Omissions

- No plugin system; reusable libraries and protocols are integration
  boundaries without an in-process viewer extension mechanism.
- No network service or background daemon.
- No database repository abstraction beyond the concrete SQLite index.
- No generated documentation site; Markdown remains reviewable in Git.
- No expansion of the `onenote.rs` fork beyond narrow, documented
  compatibility fixes submitted for upstream review.
