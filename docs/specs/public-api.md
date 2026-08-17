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
objects plus diagnostics and a lazy `ResourceStore`. `BackupFolderLoader`
provides bounded, cancellable inspection and aggregate loading for
manifest-free desktop backup folders. It returns a reconstructed section-group
tree, deterministic snapshot selection and provenance through the same
`LoadedNotebook` model, without depending on GTK or SQLite. Persistable
`SourceDescriptor` values distinguish native files from backup roots and retain
the selected latest/all-copies policy. The compatibility contract and future
extension points are documented in the
[backup-folder loader plan](../plans/backup-folder-loader.md).
`OnePkgExtractor` is a separate optional operation and is never required to
consume an already extracted source.

`ResourceStore::copy_to` is the reusable attachment/image payload boundary.
It performs blocking, bounded streaming into a caller-owned `Write`, reports
progress on the calling thread, accepts a cloneable cooperative cancellation
handle, enforces an explicit byte ceiling, and validates reliable declared
payload lengths. It returns typed read, write, cancellation, size-limit, and
size-mismatch errors. Callers choose their worker/thread model and own durable
destination publication; the core library never selects paths, creates a
cache, or launches an attachment. This additive contract is exposed by core
API version 6. API version 7 adds optional lazy resource handles for OneNote's
browser-compatible image representation and embedded-file icon; primary
payload handles remain unchanged.

Core API version 8 adds the reusable backup-folder inspection, snapshot
selection, aggregate loading, progress/cancellation, and typed source
descriptor contracts described above.

The public model includes:

- source identity and fingerprint;
- notebook and section-group hierarchy;
- ordered sections, pages, and subpages;
- semantic page objects, including title/body roles, and source geometry;
- structured rich-text hyperlinks with exact targets, UTF-16 source ranges,
  and explicit-versus-detected provenance;
- lazy handles for bounded image and attachment payload access;
- unknown-object placeholders and stable diagnostic codes;
- stable locators that namespace OneNote GUIDs by source identity.

Upstream parser types, filesystem implementations, and internal revision-store
objects do not cross this boundary. Callers must be able to use the core API
without linking GTK or SQLite.

`OneNoteLoader` preserves source-authored hyperlinks by default. Its
`LoadOptions::detect_plain_text_links` enrichment is explicitly opt-in for
library consumers; when enabled, visible URL and email text may additionally
produce `TextLinkOrigin::Detected` ranges. This heuristic never replaces or
overlaps explicit OneNote metadata.

## Rendering APIs

### UI-Neutral Scene

`onenote-render` consumes a page from `onenote-core` and produces an immutable
`PageScene`. The scene contains normalized bounds, draw primitives, text
layout inputs, stacking order, hit regions, accessibility semantics, resource
handles, and source locators.

Each scene node can also carry an outer-to-inner `flow_path`. Every
`SceneFlowPosition` identifies the node's authored order in one independently
reflowable sequence. Separate freeform outlines use separate groups, and nested
content such as table cells adds a child group without losing its parent
position. Measuring backends use this metadata to preserve authored gaps while
moving only later nodes in the affected flow; they must not infer reading order
from stacking order or source object identity.

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

Text layout maps source UTF-16 link ranges through hidden-run suppression,
list-marker insertion, math replacement, and UTF-8 display layout. Pointer hit
testing returns `HitAction::OpenLink` only over the resulting linked glyphs.
The component underlines links and provides pointer affordance, but never
launches a URI; the embedding host owns internal navigation, scheme policy,
confirmation, and failure UI.

Pango's exact text and math measurements are resolved into adapter-local node
bounds. The resulting geometry is cached by layout generation and invalidated
when the scene, font context, zoom, or asynchronous math measurements change.
Drawing, canvas sizing, viewport culling, hit testing, and reveal operations all
consume the same resolved bounds. The immutable `PageScene` continues to retain
the source-authored anchors and approximate fallback geometry for other
backends.

The component owns pan, zoom, viewport culling, hit testing, bounded lazy image
decode, and retention of scene accessibility semantics. Mapping each scene
object to a keyboard-focusable GTK accessible child is not implemented. It
does not own notebook navigation, search UI, window creation, recent files, or
workspace persistence. The `standalone` example embeds it in a window without
linking `onenote-viewer`.

Attachment hit regions provide theme-aware visuals, pointer/tooltips, bounded
focus traversal, and Enter/Space activation. The renderer still returns only
`HitAction::OpenAttachment`; the embedding host owns availability diagnostics,
destination selection, persistence, desktop launching, and execution policy.
This keyboard action support is not a claim that the custom canvas exposes a
complete virtual accessibility tree.

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
- reuse a published source when its identity, fingerprint, schema, document
  projection version, and caller-defined model configuration match;
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

`IndexProfile` records the library's document-projection version together with
a stable caller-defined description of loader/model options that affect the
indexed document independently of source bytes. `ensure_source` reports
whether the generation was reused or rebuilt and performs no writes on an
exact match. SQLite schema and FTS syntax are implementation details. Raw SQL
is not a public interface, and callers cannot rely on the database remaining
compatible across versions.

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
tracked in [issue #45](https://github.com/emsi/OneNoteViewer/issues/45),
especially malformed-input
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
