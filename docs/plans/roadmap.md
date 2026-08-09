# Roadmap

This is the long-term implementation sequence beneath the
[master plan](../MASTER-PLAN.md). The master plan owns 1.0 criteria and overall
scope; GitHub issues own current work and status. Roadmap milestones are broader
engineering goals and are not additional 1.0 release gates.

## Milestone 0: Specification and Evidence

**Status: complete**

- Define product scope and read-only safety boundary.
- Select and compare the desktop technology stack.
- Archive the primary format specifications with checksums.
- Define the parsing pipeline, semantic model, feature inventory, limitations,
  search contract, and corpus requirements.
- Pin and inspect relevant current parser/renderer work.
- Establish direct source-native rendering and managed package extraction as
  product requirements.
- Establish reusable renderer and index/query interfaces as supported product
  boundaries.

Exit evidence is the documentation set governed by `docs/MASTER-PLAN.md`.
Implementation compatibility is explicitly not claimed by this milestone.

## Milestone 1: Feasibility Spikes

**Status: implementation spikes complete; evidence gates partially open**

All three workstreams produced retained product components rather than
throwaway code. Parser, scene, GTK, index, standalone-client, and private-corpus
tests pass. The original performance, visual-oracle, accessibility, and broad
corpus gates remain open and are tracked as
[GitHub issues](https://github.com/emsi/OneNoteViewer/issues); therefore this
milestone does not authorize a broad compatibility claim.

### Parser Spike

- [x] Create the Cargo workspace and `onenote-core`.
- [x] Pin Rust 1.85.1 and the reviewed parser revision.
- [x] Load the supplied desktop `.one` and `.onetoc2` corpus.
- [ ] Add an independently licensed FSSHTTP sample and broader producers.
- [x] Turn the proven external `7z` procedure into a bounded package-orchestration
  API and reproduce extraction through automated integration tests.
- [x] Parse the extracted private notebook tree and compare notebook-level
  discovery with its individual `.one` sections and nested TOCs.
- [x] Expose a serializable stable domain model and structured diagnostics.
- [ ] Establish recorded memory/time baselines and broad malformed-input behavior.
- [x] Identify upstream API gaps for unknown objects, history, and resource
  limits.

**Gate status:** private regression notebooks spanning OneNote versions from
2010 through modern Microsoft 365 load without panic and project page/title/text,
images, attachments, and tables. Redistributable producer breadth, tags, ink,
broader malformed inputs, and measurements remain open. Private equation-heavy
fixtures cover typed OfficeMath projection, structured native rendering,
fallback text, and search, but not every rare operator family.

### Canvas Spike

- [x] Build `onenote-render` as a headless scene builder and
  `onenote-render-gtk` as a GTK4 custom widget consuming its synthetic scene.
- [x] Render Pango rich text, images, tables, links, and attachments.
- [ ] Render hand-drawn ink from representative notebook sources
  ([issue #6](https://github.com/emsi/OneNoteViewer/issues/6)).
- [x] Implement viewport culling, pan/zoom, hit testing, and UI-neutral
  accessibility semantics.
- [x] Embed the GTK component in a minimal host that does not depend on
  `onenote-viewer`.
- [ ] Map semantic scene nodes to GTK accessibility nodes and capture
  GNOME/KDE, Wayland/X11, and Orca evidence.

**Gate status:** the GTK direction is retained after successful real-notebook
Xvfb rendering and bounded-image/culling implementation. Frame pacing,
mixed-script fixtures, accessibility, stable-memory measurement, and desktop
appearance remain unproven, so ADR 0001 is not fully closed.

### Search Spike

- [x] Implement the page document extractor and a disposable FTS5 database.
- [x] Verify structured multi-field extraction, safe queries, filters,
  snippets, geometry locators, and source removal.
- [x] Exercise the structured library API through tests and the
  versioned JSON Lines protocol from a non-Rust test process.
- [x] Prove transactional rollback on cancellation.
- [ ] Benchmark incremental refresh and warm/cold latency on recorded hardware.

**Gate status:** correctness, integrity, multi-source, cancellation, and
independent protocol fixtures pass. Recorded latency targets remain open.

## Milestone 2: Readable Notebook MVP

**Status: usable viewer delivered; remaining enhancements continue**

- [x] Open/close/reopen multiple notebook roots.
- [x] Virtualized, collapsible notebook/section-group/section tree and page
  navigation without flattening section-group paths.
- [ ] Load a manifest-free OneNote backup directory as one synthetic notebook
  through the reusable core loader, preserving directories as section groups
  and selecting dated section snapshots with explicit provenance. See the
  [backup-folder loader plan](backup-folder-loader.md).
- [x] Add settings-backed default notebook location discovery plus `.onepkg`
  destination confirmation, phase progress, and cancellation.
- [ ] Complete `.onepkg` resource limits and preflight: expanded-size and
  free-space checks, early missing-tool reporting, and destination naming.
- [x] Active-page freeform canvas with rich text, lists, tables, lazy images,
  attachments, placeholders, pan, and zoom.
- [x] Viewer title/date chrome without duplicate native title-area rendering;
  reusable scene construction retains a full-page option.
- [x] Preserve and pointer-activate inline web, mail, file, and OneNote page
  links through host-owned policy.
- [ ] Complete tags.
- [x] Add safe on-demand attachment Save As and Open actions with bounded
  streaming, portable filename sanitation, progress, cancellation, private
  cache materialization, and desktop/portal delegation.
- [x] Project, render, and index basic OfficeMath with structured fallback.
- [x] Global search with result navigation to matching object geometry.
- [x] Persistent all-open-notebooks workspace and default global search scope.
- [x] Standalone renderer and query consumers.
- [ ] Publish Rust API documentation and compatibility/migration policy.
- [ ] Add per-page/per-section compatibility warning surfaces.
- [ ] Add source-change detection and manual transactional refresh.

**Long-term milestone criteria:**

- every MVP feature-matrix row has a fixture or an explicit accepted
  limitation;
- no source-write path exists;
- malformed corpus produces no panic, hang, root escape, or unbounded
  allocation;
- keyboard navigation and Orca smoke tests pass;
- independent renderer and index clients use no viewer-private interfaces;
- public errors, cancellation, limits, locators, and protocol-version fixtures
  pass;
- `cargo fmt`, clippy, tests, and dependency/license audit pass.

Current implementation tasks, acceptance criteria, and status are tracked as
[GitHub issues](https://github.com/emsi/OneNoteViewer/issues). The
[limitations document](../limitations.md) records stable product boundaries.

## Milestone 3: Compatibility and Fidelity

- Fill producer and private corpus matrix.
- Improve title/list/table geometry and font fallback diagnostics.
- Complete tested OfficeMath operators.
- Improve nested ink and stored handwriting recognition.
- Complete internal section/object-anchor link resolution beyond the current
  cross-notebook page-ID path.
- History/conflict indicators and optional separate views.
- Incremental refresh at section granularity.

Compatibility documentation names passing producers and features; it does not
use an unqualified "supports OneNote."

## Milestone 4: Distribution

- [x] Apply GPL-3.0-or-later, preserve third-party license boundaries, and
  package corresponding-source and dependency notices.
- [x] Add publishable Flatpak and AppImage paths, plus unpublished native
  build/test products, with no Flatpak network or broad filesystem permission.
- [x] Add local and `v*` tag/dispatch workflows that publish only the portable
  preview artifacts.
- [ ] Flatpak installation, portal, no-network, notebook-tree, and package
  onboarding smoke tests.
- [x] Add versioned AppStream metadata.
- [ ] Complete MIME associations and reproducibility comparison.
- [ ] Performance, accessibility, security, and privacy review.
- [ ] Signed stable release artifacts and a documented update process.
- [ ] Versioned library artifacts, API documentation, integration examples, and
  migration notes for public interface changes.

Flatpak and AppImage package onboarding use a checksum-pinned bundled `7zz`
process with its license and corresponding source. Portal installation and
end-to-end package-import tests remain release work.

## Deferred Backlog

- Password-protected sections.
- Optional in-application previews for explicitly supported attachment types.
- Media playback with note timing.
- Old-version/conflict browsing.
- Additional distribution packages.
- Optional export to an open archival format, isolated from the native
  viewer/index path.

Editing, OneDrive synchronization, real-time collaboration, and embedded web
execution remain out of scope unless a later ADR changes the product.
