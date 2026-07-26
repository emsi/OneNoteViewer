# Public Integration API

This specification refines the reusable-component deliverable in the
[project master plan](../MASTER-PLAN.md).

## Purpose

OneNote Viewer must make local OneNote content useful beyond its own desktop
shell. This specification defines supported integration boundaries for other
note-taking, knowledge-management, desktop-search, and archival applications.
It describes contracts; exact Rust names may change during milestone 1 before
the first published API version.

The APIs preserve native OneNote semantics and freeform geometry. They do not
provide HTML, Markdown, or PDF conversion.

## Design Principles

- **Library first:** `onenote-viewer` composes public components and receives no
  privileged access to parser, renderer, or index internals.
- **UI independence:** parsing, domain modeling, page layout, and search do not
  depend on GTK, a display server, or viewer-global state.
- **Explicit ownership:** callers choose sources, cache/index locations,
  limits, cancellation, and lifetimes. Libraries do not silently create global
  singletons or background daemons.
- **Stable identities:** results use source-scoped notebook, section, page, and
  object locators that can be stored and resolved after a refresh.
- **Robust failure:** untrusted files cannot panic the host process. Partial
  compatibility is returned through structured diagnostics.
- **Replaceable internals:** SQLite, the upstream parser, GTK, and cache formats
  do not leak into interfaces that do not require them.

## Common Domain API

`onenote-core` is the shared foundation. It exposes read-only operations for a
notebook directory, `.onetoc2`, or standalone `.one`, returning immutable
domain objects plus diagnostics. Package extraction is a separate optional
operation and is never required to consume an already extracted source.

The public model includes:

- source identity and fingerprint;
- notebook and section-group hierarchy;
- ordered sections, pages, and subpages;
- semantic page objects and source geometry;
- lazy handles for bounded image and attachment payload access;
- unknown-object placeholders and stable diagnostic codes;
- stable locators that namespace OneNote GUIDs by source identity.

Upstream parser types, filesystem implementations, and internal revision-store
objects do not cross this boundary. Callers must be able to use the core API
without linking GTK or SQLite.

## Rendering APIs

### UI-Neutral Scene

`onenote-render` consumes a page from `onenote-core` and produces an immutable
`PageScene`. The scene contains normalized bounds, draw primitives, text
layout inputs, stacking order, hit regions, accessibility semantics, resource
handles, and source locators.

Scene construction must be deterministic for the same model, render options,
font environment, and viewport constraints. It supports cancellation and
returns diagnostics for unsupported or approximated content. It performs no
file selection, link launching, attachment execution, or application
navigation.

The scene API is the portability boundary for a future non-GTK backend. It
must not contain `gtk`, `gdk`, `gio`, or viewer application types.

### Embeddable GTK Renderer

`onenote-render-gtk` adapts `PageScene` to Pango/GSK and exposes an embeddable
page widget/controller. A host application supplies the page, viewport,
theme/font context, and callbacks for link, attachment, selection, and
navigation actions.

The component owns pan, zoom, viewport culling, hit testing, and accessible
page-object presentation. It does not own notebook navigation, search UI,
window creation, recent files, or workspace persistence. A minimal example
must embed it in a window that does not link `onenote-viewer`.

The first supported interface is a Rust crate. A GObject-introspectable wrapper
for use from other GTK languages is a pre-1.0 integration goal; it must be
versioned and tested rather than exposing unstable Rust symbols as a C ABI.

## Index and Query APIs

`onenote-index` is a headless library. It accepts explicit `onenote-core`
sources or normalized page documents and owns its private, versioned storage.
Its public operations cover:

- create/open/rebuild an index at a caller-selected location;
- add, replace, refresh, and remove a source transactionally;
- report source/index status and structured compatibility diagnostics;
- execute bounded structured queries across caller-selected source scopes;
- cancel ingestion and queries;
- resolve stored search hits against the current source fingerprint.

A search request contains query text, source scope, field filters, result
limit, and optional ranking/snippet options. A search hit contains rank,
matched field, bounded source-text snippet, source fingerprint, notebook,
section, page and object locators, and geometry when known. It never requires a
caller to inspect SQLite tables or parse display strings.

SQLite schema and FTS syntax are implementation details. Raw SQL is not a
public interface, and callers cannot rely on the database remaining compatible
across versions.

### Non-Rust Query Protocol

A small headless command exposes the same operations to non-Rust software over
versioned JSON Lines on standard input/output. The protocol:

- begins with an explicit protocol-version negotiation;
- uses request IDs and structured success/error responses;
- streams bounded progress and result batches;
- supports cancellation;
- writes diagnostics to protocol messages, not human prose mixed into stdout;
- never becomes an always-running network service.

The command is an adapter over `onenote-core` and `onenote-index`, not a second
implementation. Its schema is versioned independently from internal Rust types
and includes compatibility fixtures.

## Compatibility and Quality Contract

Before an interface is called public:

1. its supported types and error cases have generated API documentation;
2. an example or integration test consumes it outside `onenote-viewer`;
3. malformed-input tests prove no panic crosses the boundary;
4. resource limits and cancellation behavior are specified and tested;
5. thread-safety and callback execution context are documented;
6. semantic-versioning rules identify breaking and additive changes;
7. serialization protocols have golden fixtures and explicit version fields;
8. logs and diagnostics avoid notebook content and private filenames by
   default;
9. the project license permits intended library reuse and redistribution.

Pre-1.0 APIs may evolve from corpus evidence, but changes require release notes
and migration guidance. Internal modules remain private unless they satisfy
this contract.

## Non-Goals

- A plugin system inside OneNote Viewer.
- A network server or cloud search service.
- A writable OneNote object model.
- Direct access to the viewer's workspace database or private caches.
- A promise that every renderer backend can reproduce Pango/GSK identically.
