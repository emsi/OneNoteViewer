# Remaining Work

- **Status:** Active release-gap register
- **Last reconciled:** 2026-08-03 UTC

This file records what the current implementation does **not** yet prove or
complete. It is the dedicated bridge between the implemented baseline, the
[master plan](MASTER-PLAN.md), the [roadmap](plans/roadmap.md), and the detailed
[risk register](limitations.md). A target feature is not complete merely
because its model type or first rendering path exists.

## Release Blockers

### 1. Measured Freeform Fidelity

**Why open:** The scene builder preserves top-level geometry and stacking, but
uses conservative text-height estimates and approximate internal outline/table
flow. Ink is fitted to projected bounds. No OneNote Desktop screenshot oracle,
geometry tolerance, mixed-font baseline, or printout comparison is committed.

**Completion:** Add licensed pages and matching OneNote Desktop captures for
negative coordinates, overlap, rich runs, RTL, nested lists/tables, images,
printouts, ink, and math. Record object/text baselines and accepted tolerances;
make visual regressions testable on fixed fonts and renderers.

### 2. Accessibility and Keyboard Canvas Navigation

**Why open:** `PageScene` carries semantic labels and roles, and GTK navigation
uses standard virtualized controls, but scene nodes are not exposed as
focusable GTK accessible children. Attachment hit regions have focus traversal
and keyboard activation; inline text links and general scene nodes do not.
Orca has not been tested.

**Completion:** Implement a virtual accessible object/focus model synchronized
with viewport and hit regions. Add keyboard reading order and activation, then
record Orca tests under GNOME and KDE on Wayland and X11.

### 3. Complete Viewer Actions and Operations

**Why open:** The viewer resolves search hits, opens pointer-activated inline
links, safely saves/opens attachments, and imports packages. Attachment and
package writes share cancellable progress UI, but per-source diagnostics,
manual refresh, automatic source-change refresh, and restoration of active
page/pane state remain incomplete.

**Completion:** Add source fingerprint monitoring and transactional refresh,
diagnostics surfaces, and versioned workspace state with corruption recovery
tests. Extend the operation coordinator only when another concrete long-running
workflow requires it.

### 4. Manifest-Free Backup Folder Aggregation

**Why open:** A directory with recursive `.one` files but no root `.onetoc2`
is currently discovered as many standalone sources. Directory section groups,
dated snapshot relationships, one aggregate source identity, and provenance
are not reconstructed.

**Completion:** Implement the reusable core inspector and aggregate loader in
the [backup-folder loader plan](plans/backup-folder-loader.md), integrate one
synthetic notebook with the viewer and index, migrate only proven
directory-discovered workspace entries, and pass synthetic/licensed plus
private aggregate corpus gates.

### 5. Parser and Corpus Breadth

**Why open:** All 32 sections in one private desktop package and all 83
physical snapshots in one private backup folder parse only after documented
patches in the pinned public parser fork. This does not prove
OneNote 2016, Microsoft 365, Mac backup, FSSHTTP download, encrypted/corrupt,
or feature-matrix breadth. The private sources cannot be redistributed.

**Completion:** Add legal synthetic and producer fixtures for every MVP matrix
row, upstream the compatibility patches, move to a reviewed tagged parser (or
document a maintained fork), and publish producer-specific results without a
blanket “supports OneNote” claim.

### 6. Hostile-Input and Resource Proof

**Why open:** Projection counts, archive listing/entry counts, resource reads,
image dimensions/allocations, scene nodes, results, snippets, and texture cache
are bounded. Package paths and staging are validated. Missing are parser fuzz
coverage, total model-memory/section-byte ceilings, expanded-package byte and
disk-space ceilings, extraction process-group termination, coordinate clamps,
tiled oversized images, and crash/recovery testing.

**Completion:** Add fuzzers and malformed corpora for every binary boundary;
checked total-byte/depth/coordinate budgets; extraction expansion/disk checks;
whole-process cancellation tests; peak RSS/time assertions; and source/durable
destination unchanged checks for every failure mode.

### 7. Performance and Scale Evidence

**Why open:** The full private root opens and indexes 637 pages, and viewport
culling plus lazy image decode are implemented, but no baseline hardware,
latency, frame pacing, peak memory, or large multi-source measurements are
recorded. The viewer uses one sequential load/index worker and a clear-all
texture-cache eviction policy.

**Completion:** Benchmark cold parse/index, incremental refresh, warm queries,
page switching, pan/zoom, and image-heavy pages. Record hardware and thresholds,
add bounded scheduling/cancellation, and replace coarse cache eviction or
layout work only where measurements justify it.

### 8. Public API Publication

**Why open:** The Rust component boundaries, standalone GTK example, and JSONL
process client exist and are tested. The APIs are still pre-1.0, generated docs
are not published, thread/callback guarantees need fuller prose, and the
renderer has no GObject-introspection wrapper. GPL-3.0-or-later now provides a
clear reuse and redistribution license.

**Completion:** Close the quality checklist in `specs/public-api.md`, publish
rustdoc and protocol fixtures, define semantic-versioning/migration policy,
and add a non-Rust renderer binding if retained as a goal.

### 9. Distribution and Desktop Integration

**Why open:** Automated unsigned preview Flatpak and AppImage release paths now
exist, with a desktop file, icon, checksums, and tag-based GitHub publishing.
The dynamically linked native executable remains an unpublished CI/local test
product. Both portable formats bundle a checksum-pinned `7zz` plus its license
and corresponding source. There is still no AppStream metadata, MIME
association, portal/package-import test suite, complete AppImage runtime
license audit, signing, or stable release artifact.

**Completion:** Add no-network portal and packaged `.onepkg` tests, supply
AppStream/MIME metadata, audit bundled/runtime dependencies, test the AppImage
on supported distributions, and produce reproducible signed artifacts.

## Implemented Evidence

The following are complete enough to build on, but remain pre-1.0:

- source-native semantic projection with typed identities, hierarchy,
  geometry, diagnostics, lazy bounded resources, and source fingerprints;
- validated on-disk `.onepkg` extraction with private staging and atomic
  publication through an external `7zz`/`7z`, with UI phase reporting and
  cancellation;
- UI-neutral retained scenes with placeholders, culling, hit regions, and
  semantics;
- typed OfficeMath projection, marker-free search/accessibility text, a public
  replaceable math-layout contract, and bounded asynchronous native Typst
  rendering with embedded fonts and visible fallback diagnostics;
- embeddable GTK `PageView` with Pango/GSK/Cairo rendering, pan/zoom, and
  bounded asynchronous raster decode;
- bounded on-demand attachment Save As/Open with portable name sanitation,
  GIO replacement, cancellation/progress, source-scoped private cache, desktop
  delegation, and unchanged-destination failure tests;
- transactional multi-source FTS5 indexing and structured Rust/JSONL queries;
- native GTK multi-notebook shell with virtualized navigation, persistence,
  configurable XDG Documents library discovery,
  background work, global search, and result-to-canvas navigation;
- private-corpus extraction, parse, scene, index/search, standalone renderer,
  full-viewer, and two-source Xvfb evidence;
- workspace-wide tests and strict Clippy passing on the pinned toolchain.

Closing this file requires evidence, not deleting or weakening the associated
acceptance criteria.
