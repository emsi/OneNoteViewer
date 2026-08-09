# ADR 0003: Reusable Renderer and Index Interfaces

- **Status:** Accepted
- **Date:** 2026-07-26
- **Decision owners:** Project maintainers

## Context

The project is more useful if native OneNote access is not confined to one
desktop executable. Other note-taking applications should be able to embed a
faithful `.one` page, and search or knowledge-management tools should be able
to index notebooks and resolve relevant pages without duplicating binary
parsing, canvas reconstruction, or SQLite internals.

The earlier three-crate layout placed canvas rendering inside
`onenote-viewer`. That makes the application shell an accidental integration
boundary and encourages reuse through conversion, UI automation, database
inspection, or copied code. Those outcomes conflict with source-native access
and long-term robustness.

## Decision

Make the desktop application a consumer of reusable components:

- `onenote-core` owns source access, parser adaptation, immutable native domain
  objects, diagnostics, and stable source-scoped locators.
- `onenote-render` owns UI-neutral page layout, retained scene construction,
  hit regions, and accessibility semantics.
- `onenote-render-gtk` owns the embeddable Pango/GSK page component.
- `onenote-index` owns rebuildable storage, ingestion, and a structured public
  query API.
- a versioned headless JSON Lines adapter makes index/query operations
  available to non-Rust software.
- `onenote-viewer` owns only application composition, windows, workspace
  persistence, navigation UI, and desktop integration.

Dependency direction is one-way. None of the reusable crates can depend on
`onenote-viewer`, its settings, or its workspace database. The viewer cannot
use private shortcuts unavailable to another consumer.

The public contract is specified in
[the integration API specification](../specs/public-api.md). Public means
documented, tested from a separate consumer, versioned, bounded, cancellable,
and supported. It does not mean exporting every internal type.

## Robustness and Flexibility

Robustness takes priority over a superficially broad API:

- hostile input produces typed errors and diagnostics, not process panics;
- callers provide explicit limits and cancellation;
- immutable values and source locators cross boundaries instead of parser or
  database implementation types;
- index updates are transactional and search results identify their source
  fingerprint;
- renderer actions are callbacks chosen by the host, never hidden side
  effects;
- optional integrations do not increase the core viewer's runtime privileges;
- compatibility fixtures cover both Rust APIs and serialized protocols.

Flexibility comes from the UI-neutral scene and semantic domain model, not from
weakly typed maps or generated markup. A future Qt, Skia, or off-screen backend
can consume the scene without changing OneNote parsing. A future search engine
can replace SQLite behind the query contract without changing consumers.

## Consequences

- The workspace grows from three to five production crates and requires public
  API documentation, examples, compatibility tests, and release discipline.
- A GTK-specific crate remains necessary because text shaping, accessibility,
  and rendering cannot be completely toolkit-neutral.
- The initial reusable API is Rust-native; non-Rust search clients use the
  versioned process protocol. A GObject-introspectable renderer wrapper is a
  separate pre-1.0 goal.
- GPL-3.0-or-later permits downstream library reuse while requiring distributed
  combined works and modifications to preserve the same software freedoms.
- Premature generalization is controlled by accepting abstractions only when
  exercised by the viewer and at least one independent example consumer.

## Implementation Status

All five crates and the one-way dependency graph now exist. The GTK standalone
example consumes only `onenote-core`, `onenote-render`, and
`onenote-render-gtk`; the JSON Lines process integration test consumes the
versioned query adapter independently. The viewer composes only public crate
exports.

These are pre-1.0 implementation boundaries, not published stable artifacts.
Generated documentation publication, fuller contract tests, GObject
introspection, and callback/thread compatibility guarantees remain in
[issue #45](https://github.com/emsi/OneNoteViewer/issues/45).

## Rejected Alternatives

- **Keep rendering inside the viewer:** prevents supported embedding and makes
  application state leak into layout code.
- **Expose SQLite directly:** couples consumers to migrations, FTS details,
  and private cache data.
- **Use HTML as the interchange format:** loses native spatial semantics and
  violates the core product requirement.
- **Start a local network service:** adds lifecycle, permissions, and attack
  surface without a demonstrated need.
- **Promise a stable Rust or C ABI immediately:** corpus and renderer spikes
  must shape the first sound interface before compatibility is frozen.
