# Public Integration API

This specification refines the reusable-component deliverable in the
[project master plan](../MASTER-PLAN.md).

## Purpose

OneNote Viewer must make local OneNote content useful beyond its own desktop
shell. This specification defines supported integration boundaries for other
note-taking, knowledge-management, desktop-search, and archival applications.
It describes contracts. The first Rust implementations now exist, but remain
pre-1.0 and may change before the first published API version.

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

`onenote-core` is the shared foundation. `OneNoteLoader` exposes read-only
operations for a `.onetoc2` or standalone `.one`, returning immutable domain
objects plus diagnostics and a lazy `ResourceStore`. Directory root discovery
currently belongs to the viewer and should move behind a reusable core API. In
particular, manifest-free backup folders require a core-owned inspection and
aggregate-loading API that returns one source, a reconstructed section-group
tree, deterministic snapshot selection, and provenance. That API is planned,
not implemented; its contract and delivery gates are in the
[backup-folder loader plan](../plans/backup-folder-loader.md).
`OnePkgExtractor` is a separate optional operation and is never required to
consume an already extracted source.

The public model includes:

- source identity and fingerprint;
- notebook and section-group hierarchy;
- ordered sections, pages, and subpages;
- semantic page objects, including title/body roles, and source geometry;
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

Scene construction is deterministic for the same model and render options. It
supports cancellation and returns diagnostics for unsupported or approximated
content. `SceneOptions` lets an embedding host retain the complete native page
or omit native title-area objects when the host presents `Page.title` and
timestamps in its own chrome; content cropping is an independent option. The
OneNote Viewer shell uses both options, while the reusable renderer defaults
to the complete native page. Scene construction performs no file selection,
link launching, attachment execution, or application navigation.

The scene API is the portability boundary for a future non-GTK backend. It
must not contain `gtk`, `gdk`, `gio`, or viewer application types.

### Embeddable GTK Renderer

`onenote-render-gtk` adapts `PageScene` to Pango/GSK and exposes an embeddable
page widget/controller. A host application supplies the page, viewport,
theme/font context, automatic-text fallback color, and callbacks for link,
attachment, selection, and navigation actions. Explicit OneNote foreground
colors remain source-controlled.

The component owns pan, zoom, viewport culling, hit testing, bounded lazy image
decode, and retention of scene accessibility semantics. Mapping each scene
object to a keyboard-focusable GTK accessible child is not implemented. It
does not own notebook navigation, search UI, window creation, recent files, or
workspace persistence. The `standalone` example embeds it in a window without
linking `onenote-viewer`.

`PageView` exposes its effective bounded zoom and a change notification that
covers built-in gestures as well as host-initiated changes. Hosts can therefore
synchronize controls or persist a preference without duplicating zoom input
handling; persistence remains a host responsibility.

The first supported interface is a Rust crate. A GObject-introspectable wrapper
for use from other GTK languages is a pre-1.0 integration goal; it must be
versioned and tested rather than exposing unstable Rust symbols as a C ABI.

## Index and Query APIs

`onenote-index` is a headless library. It accepts an explicit `onenote-core`
`Notebook` and owns its private, versioned storage. Its current public
operations cover:

- create/open an index at a caller-selected location;
- replace and remove a complete source transactionally;
- report indexed source generations and verify index integrity;
- execute bounded structured queries across caller-selected source scopes;
- cancel replacement and queries;
- return source fingerprints with stored search hits so the caller can reject
  stale results.

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

The `onenote-query` command is an adapter over `onenote-core` and
`onenote-index`, not a second implementation. Its schema is versioned
independently from internal Rust types and has a process-level compatibility
test.

## Current Publication Status

The library boundaries, Rustdoc comments, standalone GTK example, independent
JSONL process client, structured errors, typed locators, resource limits, and
cancellation tests exist. They are usable implementation boundaries but are
not yet published/stable public artifacts. Missing quality-contract items are
tracked in [remaining work](../REMAINING-WORK.md), especially malformed-input
breadth, callback/thread documentation, golden protocol fixtures, semantic
versioning guidance, and GObject introspection. The GPL-3.0-or-later project
license permits reuse and redistribution but does not make the pre-1.0
interfaces stable.

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
9. the project license permits intended library reuse and redistribution
   (**satisfied by GPL-3.0-or-later**).

Pre-1.0 APIs may evolve from corpus evidence, but changes require release notes
and migration guidance. Internal modules remain private unless they satisfy
this contract.

## Non-Goals

- A plugin system inside OneNote Viewer.
- A network server or cloud search service.
- A writable OneNote object model.
- Direct access to the viewer's workspace database or private caches.
- A promise that every renderer backend can reproduce Pango/GSK identically.
