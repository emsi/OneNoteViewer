# Roadmap

This is the detailed implementation sequence beneath the
[master plan](../MASTER-PLAN.md). The master plan owns overall scope, current
status, deliverables, and document authority; this roadmap owns milestone order
and exit gates.

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

Run three tested feasibility workstreams. Their code may be discarded, but
their fixtures, measurements, public-interface findings, and decisions remain
project evidence.

### Parser Spike

- Create the Cargo workspace and `onenote-core`.
- Pin a Rust toolchain meeting the reviewed parser's minimum version.
- Pin reviewed `onenote_parser` desktop-support commit.
- Load desktop `.one`, desktop `.onetoc2`, and FSSHTTP samples.
- Turn the proven external `7z` procedure into a bounded package-orchestration
  API and reproduce extraction through automated integration tests.
- Parse the already extracted private notebook tree and compare notebook-level
  discovery with its individual `.one` sections and nested TOCs.
- Dump stable domain-model JSON and structured warnings.
- Establish memory/time/error baselines and malformed-input behavior.
- Identify upstream API gaps for unknown objects, history, and resource limits.

**Gate:** representative desktop notebook loads without panic; page/title/text,
images, attachments, tables, tags, ink, and math survive projection.

### Canvas Spike

- Build `onenote-render` as a headless scene builder and
  `onenote-render-gtk` as a GTK4 custom widget consuming its synthetic scene.
- Render Pango mixed-script rich text, an image, table, link, and ink.
- Implement viewport culling, pan/zoom, hit testing, and accessibility nodes.
- Embed the GTK component in a minimal host that does not depend on
  `onenote-viewer`.
- Capture GNOME/KDE, Wayland/X11 screenshots and Orca behavior.

**Gate:** all five fallback triggers in ADR 0001 pass. Otherwise prototype the
same scene in Qt 6 and revisit the ADR before product code grows.

### Search Spike

- Implement the page document extractor and a disposable FTS5 database.
- Verify title/body/tag/alt/ink/attachment/link queries and snippets.
- Exercise the structured library API from a separate Rust client and the
  versioned JSON Lines protocol from a non-Rust test process.
- Benchmark incremental index and cancellation.

**Gate:** correctness fixtures pass and warm latency meets the search targets
on recorded baseline hardware.

## Milestone 2: Readable Notebook MVP

- Open/close/reopen multiple notebook roots.
- Notebook/section-group/section/page navigation.
- One-time `.onepkg` extraction with missing-tool, progress, cancellation,
  staging cleanup, and destination-conflict handling.
- Active-page freeform canvas with text, lists, tables, images, and printouts.
- Read-only tags, links, attachment metadata/extraction, ink, and basic math.
- Global search with result navigation.
- Persistent all-open-notebooks workspace and default global search scope.
- Published Rust API documentation and standalone renderer/query examples.
- Per-page and per-section compatibility warnings.
- Source-change detection and manual refresh.

**Exit criteria:**

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

## Milestone 3: Compatibility and Fidelity

- Fill producer and private corpus matrix.
- Improve title/list/table geometry and font fallback diagnostics.
- Complete tested OfficeMath operators.
- Improve nested ink and stored handwriting recognition.
- Internal cross-notebook/object link resolution.
- History/conflict indicators and optional separate views.
- Incremental refresh at section granularity.

Compatibility documentation names passing producers and features; it does not
use an unqualified "supports OneNote."

## Milestone 4: Distribution

- Choose and apply the project source license.
- Flatpak manifest with no network permission and portal-based access.
- Flatpak installation, portal, no-network, notebook-tree, and package
  onboarding smoke tests.
- Desktop metadata, icons, MIME associations, and reproducible builds.
- Performance, accessibility, security, and privacy review.
- Signed release artifacts and a documented update process.
- Versioned library artifacts, API documentation, integration examples, and
  migration notes for public interface changes.

Flatpak package onboarding ships only if a reviewed extractor is bundled; the
viewer remains fully functional for already extracted notebook trees if it is
not.

## Deferred Backlog

- Password-protected sections.
- Sandboxed attachment body extraction.
- Media playback with note timing.
- Old-version/conflict browsing.
- Additional distribution packages.
- Optional export to an open archival format, isolated from the native
  viewer/index path.

Editing, OneDrive synchronization, real-time collaboration, and embedded web
execution remain out of scope unless a later ADR changes the product.
