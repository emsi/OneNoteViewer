# Product Requirements

This is the normative behavioral contract beneath the
[project master plan](../MASTER-PLAN.md).

## Product Definition

OneNote Viewer is a mature, native Linux desktop viewer for local Microsoft
OneNote notebooks. It reconstructs the notebook and page experience directly
from `.onetoc2` and `.one` source data. It is not a migration utility,
converter, web wrapper, or editor.

The intended experience is comparable to the desktop OneNote application for
reading and finding existing notes:

- several notebooks can remain open in one workspace;
- notebook, section group, section, and page navigation remain visible and
  preserve source ordering;
- page context identifies the notebook and complete section-group ancestry,
  followed by the owning section;
- selecting a page displays its native freeform canvas;
- one search covers all open notebooks and navigates to the matching page and
  canvas object;
- the workspace and index persist locally between launches.

The UI can follow Linux desktop conventions and need not be a pixel copy of
Microsoft OneNote. Its information architecture and spatial page semantics
must remain recognizably OneNote.

## Non-Negotiable Invariants

### Source-Native Viewing

The viewer parses `.one` and `.onetoc2` files and constructs its domain and
scene models from their semantic objects and geometry. Source files remain the
authority. The SQLite search index, image thumbnails, scene caches, and
workspace metadata are disposable derived data.

HTML, Markdown, PDF, plain text, or another application's document model must
not sit between the OneNote parser and the renderer or indexer. Such formats
may be used only as independent test oracles. They are never the representation
shown to the user and never the source used to rebuild the index.

### Freeform Fidelity

OneNote pages are spatial canvases, not linear documents. The implementation
must preserve and render, when present:

- independent object positions and dimensions;
- overlapping objects and deterministic stacking order;
- unbounded or very large page extents, including negative coordinates;
- backgrounds, rule lines, images, printouts, ink, tables, and rich outlines;
- relationships between text markers and embedded objects;
- page units, object constraints, and layout hints without flattening.

Unsupported content must retain a visible placeholder or diagnostic. Silently
reflowing a page into reading order is not an acceptable fallback.

### Multi-Notebook Workspace and Search

Opening another notebook adds it to the current workspace; it does not replace
the existing notebook. Each source has a stable identity even when two
notebooks contain duplicate internal GUIDs. The navigation model supports
notebook switching and nested section groups without losing the active page in
other notebooks.

The application has a configurable **default notebooks location**. Its initial
value is the `OneNoteViewer` subdirectory of the user's XDG Documents
directory. Every notebook folder found there is opened automatically at
startup, including folders copied there outside the application. Sources
opened from arbitrary other locations remain in place and persist separately;
the application does not move them into the default location.

The local index covers every open notebook by default. Search results identify
the notebook, section, page, matching field, and canvas object when geometry is
known. Activating a result selects the correct notebook and page and brings the
matching object into view. Closing a notebook removes it from the active search
scope without modifying its files.

### Local and Read-Only Operation

Viewing and indexing require no Microsoft account, OneDrive connection, or
network service. The application never writes into a source notebook tree.
Attachments are extracted only after an explicit user action, and package
extraction writes only to a separate user-selected destination.

### Reusable Components and Public Access

The desktop viewer must not own the only usable path to OneNote content. Its
core, renderer, and indexer are product components with documented public
interfaces:

- another application can open a `.one` section through `onenote-core` and
  receive the same immutable semantic and geometry model as the viewer;
- a UI-neutral renderer builds a deterministic page scene without depending on
  the OneNote Viewer application or GTK;
- an embeddable GTK component renders and navigates that scene without
  depending on the viewer's windows, workspace, or settings;
- `onenote-index` accepts explicit sources and exposes structured queries and
  results containing stable notebook, section, page, and object locators;
- non-Rust software can query through a versioned, headless process protocol
  without scraping viewer output or reading the SQLite schema directly.

These are supported integration boundaries, not internal modules made public
accidentally. They require API documentation, examples, compatibility policy,
bounded resource behavior, structured errors, cancellation, and tests with a
consumer outside `onenote-viewer`. The public interfaces expose native OneNote
semantics; they do not convert pages to HTML, Markdown, or PDF.

The detailed contract is in the
[public integration API specification](public-api.md), and its ownership is
fixed by [ADR 0003](../decisions/0003-reusable-components.md).

## Supported Source Lifecycles

### Existing Notebook Tree

The user selects a notebook directory, `.onetoc2`, or standalone `.one` file.
The viewer fingerprints and parses it in place, read-only. External changes are
detected and incorporated through a transactional refresh.

### Manifest-Free Backup Folder

A selected directory containing recursive `.one` section files but no usable
root `.onetoc2` is treated as one backup source, not as many single-section
notebooks. The viewer reconstructs its relative directories as section groups
and selects one snapshot for each logical section according to an explicit,
deterministic policy. The default selects the newest snapshot candidate per
section and reports if that candidate cannot be parsed.

Because a manifest-free backup does not carry authoritative notebook ordering
and may contain section snapshots from different dates, the reconstructed
hierarchy, synthetic ordering, selection evidence, conflicts, and omissions
must be available through diagnostics and provenance. Older snapshots remain
discoverable without being parsed or indexed eagerly. Opening and refresh are
read-only, bounded, cancellable, and transactional.

This behavior is owned by the reusable core API and is not yet implemented.
Its design and delivery gates are defined by the
[backup-folder loader plan](../plans/backup-folder-loader.md).

### OneNote Package

`.onepkg` is an acquisition container, not a runtime notebook format. The
application performs a one-time, on-disk extraction into a durable directory
of native `.onetoc2` and `.one` files, then opens that directory through the
same path as any other notebook. Reopening and indexing use the extracted
native files; the archive is not decompressed again.

Package extraction proposes a new child directory under the default notebooks
location and displays the exact final path before starting. The user can
choose a different parent for an individual import without changing the
default. Existing destination directories are never merged or overwritten.

The package operation must never materialize the complete package or all
expanded entries in memory. Detailed behavior is fixed by
[ADR 0002](../decisions/0002-onepkg-extraction.md).

## Minimum Viewer Experience

The primary window provides:

- a workspace-level notebook switcher/tree;
- nested section-group and ordered section navigation;
- an ordered, hierarchical page list;
- a spatial canvas with pan and zoom;
- restoration of the last application-wide zoom between launches;
- a global search entry and result view spanning all open notebooks;
- a compact single-row application header with native window controls and a
  menu for infrequent application commands;
- persisted System, Light, and Dark application themes with readable active,
  inactive, selected, and disabled states;
- scoped compatibility and source-refresh diagnostics.
- explicit attachment details with Open and Save As actions, bounded
  background copying, progress/cancellation, and copyable failure diagnostics.

Navigation lists must be virtualized for large notebooks. Loading, indexing,
refresh, extraction, and cancellation expose progress without blocking the UI.
An error in one section or notebook must not remove other usable notebooks.
All error detail is selectable and has an explicit copy command. The normative
shell, theming, dialog, and visual-validation rules are specified in
[Desktop UI Requirements](desktop-ui.md).

## Acceptance Criteria

The MVP cannot be called a native OneNote viewer until:

1. two or more notebook roots can remain open and searchable together;
2. representative freeform pages retain measured position, extent, overlap,
   object order, rich content, and ink relative to OneNote Desktop references;
3. no HTML, Markdown, PDF, or converted note database is used in the runtime
   load/render/index path;
4. deleting the application index and caches loses no notebook information;
5. source trees remain byte-for-byte unchanged after viewing and indexing;
6. a `.onepkg` can be extracted on disk, validated, added to the workspace,
   and reopened without the original package or extractor;
7. missing extractor, malformed package, cancellation, and destination
   conflicts leave the source archive and existing destination unchanged;
8. a standalone example application renders a `.one` page using the public
   core and renderer APIs without depending on `onenote-viewer`;
9. a standalone client indexes at least two notebook sources and resolves a
   query result to source/page/object identity through the public query API;
10. public APIs return structured errors, honor cancellation and limits, and
    remain free of application-global state;
11. a manifest-free backup folder opens as one source with reconstructed
    section groups, deterministic snapshot selection, and explicit provenance.
12. available attachments can be saved byte-for-byte or opened through the
    desktop handler without changing the notebook source; cancellation,
    integrity failure, unsafe names, and concurrent destination changes do not
    publish partial output or overwrite a newer destination.

## Existing Linux Tools Are Not Substitutes

Current Linux-capable projects cover adjacent workflows:

- [Butterfly's OneNote support](https://butterfly.linwood.dev/docs/v2/onenote/)
  imports OneNote content into Butterfly's own document model.
- [Joplin's OneNote import](https://joplinapp.org/help/apps/import_export/#importing-from-onenote)
  converts content into Joplin's HTML/Markdown-oriented note model.
- [P3X OneNote](https://github.com/patrikx3/onenote) wraps Microsoft's online
  OneNote web application and does not open local notebook files.
- [one2html](https://github.com/msiemens/one2html) is a command-line HTML
  converter and is useful only as a development cross-check here.

None provides this project's combination of direct local-file viewing,
source-native freeform layout, simultaneous notebooks, and global local search.
This comparison should be refreshed before releases; it does not justify
ignoring new compatible viewers.
