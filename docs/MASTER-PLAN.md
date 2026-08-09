# OneNote Viewer Master Plan

- **Status:** Functional implementation baseline; not release-ready
- **Current phase:** Milestone 2 readable-viewer completion and evidence
- **Last reconciled:** 2026-07-28 UTC

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
do not cross its public boundary. Manifest-free OneNote backup directories
must be reconstructed as one synthetic notebook through the reusable
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

- Milestone 0 documentation and evidence baseline is complete.
- The five-crate Rust workspace and pinned Rust 1.85.1 toolchain are in place.
- `onenote-core` projects native `.one`/`.onetoc2` sources into a public
  semantic and geometry model, fingerprints source trees, lazily exposes
  bounded resources through cancellable streaming, and manages validated,
  atomic, on-disk `.onepkg` extraction through `7zz`/`7z`.
- `onenote-render` builds deterministic UI-neutral scenes, and
  `onenote-render-gtk` provides an independently runnable Pango/GSK page view
  with culling, pan, zoom, hit testing, bounded asynchronous image decoding,
  Cairo line primitives, and asynchronous native OfficeMath typesetting through
  a replaceable UI-neutral backend contract. End-to-end notebook ink rendering
  remains tracked in [issue #6](https://github.com/emsi/OneNoteViewer/issues/6).
  Math source remains a typed domain AST and its marker-free linear form is
  indexed.
- `onenote-index` provides transactional multi-source FTS5 indexing,
  structured result locators, snippets, filtering, integrity checks, and a
  versioned JSON Lines query adapter with an independent process test.
- `onenote-viewer` provides persistent multi-notebook discovery and
  a collapsible notebook/section-group/section tree plus page navigation,
  background parsing/indexing/scene construction, global search result
  navigation, a native freeform canvas, settings-backed default notebook
  location discovery, and package onboarding with destination confirmation,
  phase progress, and cancellation. Attachments can be saved or opened on
  explicit request through bounded background streaming, safe destination
  replacement, a private source-scoped cache, and desktop/portal delegation.
  Its compact single-row shell exposes native window controls, an
  application-command menu, and persisted System, Light, and Dark themes under
  the [desktop UI requirements](specs/desktop-ui.md).
  Page title and creation time appear once in viewer chrome while the reusable
  renderer can still render the complete native title area for other hosts.
- A manifest-free backup directory is not yet aggregated: the current
  discovery fallback loads each recursive `.one` file as a standalone source.
  The reusable core replacement, snapshot selection, hierarchy reconstruction,
  and workspace migration are specified in the
  [backup-folder loader plan](plans/backup-folder-loader.md).
- The supplied private `.onepkg` has passed unchanged-source extraction to 32
  `.one` and five `.onetoc2` files. All 32 sections parse, every page builds a
  finite scene, 637 pages index in the root notebook, and the complete viewer
  opens and indexes it under Xvfb. A separate two-source run proved simultaneous
  workspace and index behavior.
- A separate private manifest-free backup contains 83 physical snapshots for
  42 logical section paths. All 83 now parse independently after the documented
  compatibility patches. This proves per-file recovery only, not aggregate
  backup loading, unique-page counts, or complete rendering fidelity.
- The private math regression fixture projects and natively renders its
  three OfficeMath expressions, including fractions, scripts, function
  application, and an n-ary summation with limits. This does not yet prove all
  rare OfficeMath operators or exact OneNote geometry.
- Ten primary Microsoft reference PDFs are pinned and reproducibly verified.
- Workspace tests, private-corpus tests, strict Clippy, formatting, and index
  integrity checks pass.
- This evidence is deliberately narrow. It does not prove release-grade
  fidelity, accessibility, security, producer breadth, or distribution.
  The [roadmap](plans/roadmap.md) defines execution order, the
  [risk register](limitations.md) defines evidence gaps and accepted boundaries,
  and [GitHub issues](https://github.com/emsi/OneNoteViewer/issues) track
  actionable implementation work.

### Next Execution Order

1. Implement the reusable manifest-free backup-folder loader and integrate its
   single synthetic notebook, reconstructed section groups, snapshot policy,
   workspace migration, and aggregate index generation.
2. Complete remaining viewer workflows: package preflight/limits,
   diagnostics, source refresh, tags, and fuller workspace restoration.
   Pointer-activated inline link handling and safe on-demand attachment actions
   are implemented; general canvas keyboard/screen-reader access remains part
   of the accessibility work.
3. Establish visual oracles and measured tolerances for rich text, tables,
   images/printouts, ink, negative coordinates, overlap, and large pages.
4. Map scene semantics into GTK accessibility, add keyboard navigation, and
   pass Orca tests under GNOME/KDE and Wayland/X11.
5. Expand licensed producer/feature and malformed/fuzz corpora; close parser,
   allocation, path, and process-lifecycle risks with repeatable evidence.
6. Benchmark and optimize cold parse/index, warm search, pan/zoom, image cache,
   and large workspace memory.
7. Publish API documentation and complete portable-package integration,
   reproducibility, and signing. The parser compatibility patches are available
   in upstream v2.0.0; GPL-3.0-or-later and third-party/corresponding-source
   packaging are already established.

## Definition of Success

The first mature release requires all of the following:

- representative native freeform pages match measured OneNote Desktop layout
  and content behavior within documented tolerances;
- multiple notebook roots remain open, persist across launches, and participate
  in one search scope;
- package onboarding is bounded, cancellable, recoverable, and independent
  from normal viewing;
- standalone renderer and search clients use only supported public interfaces;
- malformed and oversized input cannot panic, hang, escape source/staging
  roots, or cause unbounded allocation;
- deleting indexes and caches loses no notebook information;
- source trees remain byte-for-byte unchanged after viewing and indexing;
- accessibility, responsiveness, dependency, license, and distribution gates
  pass;
- release notes state precisely which producers and features were tested.

Detailed acceptance criteria remain normative in the product, format, search,
public API, corpus, feature, limitation, and roadmap documents.

## Document Authority

Read the project documents in this order:

1. **This master plan:** overall scope, deliverables, current phase, and
   definition of success.
2. **[Product requirements](specs/product-requirements.md):** normative user and
   product behavior.
3. **[Roadmap](plans/roadmap.md):** implementation sequence, milestones, and
   exit gates.
4. **Architecture and accepted ADRs:** component ownership and reasons for
   cross-cutting decisions.
5. **Detailed specifications:** format, feature, search, public API, and corpus
   contracts.
6. **[Limitations](limitations.md):** open risks, accepted boundaries, and
   release blockers.
7. **[GitHub issues](https://github.com/emsi/OneNoteViewer/issues):** actionable
   implementation work and its acceptance criteria.
8. **[Completion audit](plans/completion-audit.md):** historical evidence that
   the documentation baseline was assembled; it is not the current plan.

If documents disagree, the more specific accepted specification or ADR governs
its subject, but this master plan must be updated immediately so the main entry
never presents stale scope or status.
