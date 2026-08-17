# OneNote Viewer Master Plan

- **Status:** Usable pre-1.0 application; two 1.0 blockers remain
- **Current phase:** 1.0 completion
- **Last reconciled:** 2026-08-09 UTC

## Role of This Document

This is the canonical entry point and master plan for the OneNote Viewer
project. It defines the overall outcome, non-negotiable boundaries, major
deliverables, current phase, and the authority of the supporting documents.

It does not duplicate binary-format tables, API schemas, or individual feature
rows. Those details live in the linked specifications. A change to product
scope, architectural direction, delivery order, or definition of success must
update this document and the affected detailed document together.

## Mission

Deliver a robust, mature, native Linux application that opens, renders,
indexes, and searches local Microsoft OneNote notebooks while preserving their
source-native freeform layout.

The same implementation must also make OneNote content useful outside this
application. Other software must be able to:

- parse native `.one` sections and notebook trees into a documented semantic
  and geometry model;
- embed the page renderer without adopting the OneNote Viewer application
  shell;
- index multiple notebooks and find relevant pages, objects, and source data
  through a supported query interface.

## Non-Negotiable Outcomes

1. **Native, spatial rendering:** `.one` and `.onetoc2` are parsed directly.
   HTML, Markdown, PDF, or another note model never becomes the viewer or
   indexer's canonical representation.
2. **Freeform fidelity:** coordinates, extents, overlap, stacking, ink,
   printouts, images, tables, rich text, and object relationships are
   preserved or visibly diagnosed.
3. **Multi-notebook workspace:** the configurable default notebooks location
   is scanned automatically, several notebooks remain open, arbitrary external
   sources can be added without relocation, and one local search spans all
   active sources.
4. **Read-only and offline:** source notebook trees are never modified, and
   viewing/indexing requires no Microsoft account or network service.
5. **Bounded package onboarding:** `.onepkg` is extracted once, on disk, by a
   managed external extractor into a durable native notebook tree. Complete
   archive contents are never accumulated in application memory.
6. **Reusable components:** parsing/domain, UI-neutral scene construction,
   GTK rendering, and index/query behavior are supported integration
   boundaries, not viewer-private code.
7. **Robust public contracts:** integrations use structured errors, stable
   source-scoped locators, explicit limits, cancellation, versioning, and
   independent consumer tests.
8. **Honest compatibility:** unsupported and unknown content remains visible
   through diagnostics. Compatibility claims name the tested producers and
   features.

The normative behavioral detail is in
[the product requirements](specs/product-requirements.md).

## Deliverables

### Native Access Foundation

`onenote-core` provides read-only source discovery, parser isolation,
immutable domain objects, geometry, diagnostics, lazy payload access, and
stable source-scoped identities. Upstream parser and revision-store internals
do not cross its public boundary. Manifest-free OneNote backup directories are
reconstructed as one synthetic notebook through the reusable
[backup-folder loader](plans/backup-folder-loader.md), not interpreted by the
viewer as unrelated standalone sections.

### Reusable Rendering

`onenote-render` converts domain pages into a deterministic, UI-neutral
`PageScene`. `onenote-render-gtk` supplies an embeddable Pango/GSK component
with pan, zoom, culling, hit testing, and accessibility. A standalone host
must render a `.one` page without linking `onenote-viewer`.

### Reusable Index and Query

`onenote-index` transactionally indexes multiple explicit sources and returns
structured matches with source, notebook, section, page, object, and geometry
locators. A versioned JSON Lines adapter provides the same query behavior to
non-Rust software without exposing SQLite or creating a network service.

### Desktop Viewer

`onenote-viewer` composes the public components into the OneNote-like
notebook/section-group/section/page/canvas experience. It owns windows,
workspace persistence, navigation UI, and desktop integration, but receives no
private parser, renderer, or index access unavailable to other consumers.
Its default notebooks location is the `OneNoteViewer` directory under the
user's XDG Documents directory. Notebook folders copied or extracted there are
discovered automatically at startup. The location is configurable, and
explicitly opened sources elsewhere remain read-only in place.

### ONEPKG Onboarding

The application detects `7zz`/`7z`, validates the CAB package, extracts to a
private on-disk staging directory, validates native files and paths, and
atomically publishes an unused destination. Missing-tool and sandbox behavior
are explicit and do not impair normal folder viewing.

## Architecture Baseline

```text
onenote-viewer ---> onenote-render-gtk ---> onenote-render ---> onenote-core
       |                                                    ^
       +----------> onenote-index --------------------------+

non-Rust clients ---> versioned query adapter ---> onenote-index
other GTK apps ----> onenote-render-gtk
future backends ---> onenote-render
```

No dependency points toward `onenote-viewer`. Detailed ownership and runtime
flow are defined by the
[repository layout](architecture/repository-layout.md) and
[system architecture](architecture/system-architecture.md).

## Delivery Plan

The detailed milestone sequence and exit gates are in the
[roadmap](plans/roadmap.md).

### Current Status

- OneNote Viewer is used daily to access, search, and read 15 years of personal
  notes created across OneNote 2010 through modern Microsoft 365 and OneNote for
  the web.
- The desktop application provides persistent multi-notebook navigation and
  search, native freeform rendering, `.onepkg` import, links, attachments,
  workspace restoration, navigation history, zoom, and light/dark/system themes.
- The reusable core, scene renderer, GTK widget, and index/query components are
  implemented as a five-crate Rust workspace. Flatpak is the primary release
  channel, with AppImage also published.
- The two remaining 1.0 blockers are hand-drawn ink rendering
  ([issue #6](https://github.com/emsi/OneNoteViewer/issues/6)) and freeform page
  text selection/copy
  ([issue #8](https://github.com/emsi/OneNoteViewer/issues/8)).
- [GitHub issues](https://github.com/emsi/OneNoteViewer/issues) are the source of
  truth for other bugs, compatibility gaps, and planned improvements. The
  [limitations document](limitations.md) records only stable product boundaries.

### Next Execution Order

1. Render hand-drawn ink from native notebook data.
2. Add spatial selection and clipboard support to the freeform page canvas.
3. Run the existing release checks and publish 1.0 when both blockers are
   complete.
4. Prioritize remaining work from the issue tracker according to user impact;
   it does not block 1.0 unless explicitly reclassified.

## Definition of 1.0 Success

The 1.0 release requires all of the following:

- multiple notebook roots remain open, persist across launches, and participate
  in one search scope;
- native freeform pages render text, tables, images, attachments, equations, and
  hand-drawn ink;
- page text can be spatially selected and copied;
- `.onepkg` import and published Flatpak/AppImage artifacts pass their release
  checks;
- source trees remain byte-for-byte unchanged after viewing and indexing;
- known remaining limitations are stated without presenting smaller open issues
  as release blockers.

Detailed behavior remains normative in the product, format, search, public API,
and feature specifications. Current implementation status belongs in GitHub
issues.

## Document Authority

Read the project documents in this order:

1. **This master plan:** overall scope, deliverables, current phase, and
   definition of success.
2. **[Product requirements](specs/product-requirements.md):** normative user and
   product behavior.
3. **[GitHub issues](https://github.com/emsi/OneNoteViewer/issues):** current
   implementation work, acceptance criteria, and status.
4. **[Roadmap](plans/roadmap.md):** long-term milestones and sequencing context.
5. **Architecture and accepted ADRs:** component ownership and reasons for
   cross-cutting decisions.
6. **Detailed specifications:** format, feature, search, public API, and corpus
   contracts.
7. **[Limitations](limitations.md):** stable user-facing product boundaries and
   the explicitly declared 1.0 blockers.
8. **[Completion audit](plans/completion-audit.md):** historical evidence that
   the documentation baseline was assembled; it is not the current plan.

If documents disagree, the more specific accepted specification or ADR governs
its subject. GitHub issues govern current implementation status, and this master
plan must be updated when project scope or release criteria change.
