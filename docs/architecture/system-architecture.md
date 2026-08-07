# System Architecture

## Context

The project delivers reusable native-access, rendering, and index/query
components plus a read-only local desktop application that composes them. It
directly parses native OneNote files and reconstructs their spatial page model;
conversion to HTML, Markdown, PDF, or another notes format is not part of the
runtime architecture. Notebook content is untrusted and potentially very
large. Parsing and indexing must remain responsive, failures must be isolated
to the smallest useful unit, and the original files must not be changed.

## Runtime Components

```text
`.onepkg` ──> managed external extraction ──> durable notebook directory
                                                    │
Local notebook paths <──────────────────────────────┘
        │
        ▼
Input discovery and fingerprinting
        │
        ▼
Parser adapter ── warnings ──> diagnostics model
        │
        ▼
Immutable notebook domain model
        ├───────────────┐
        ▼               ▼
Index/query API     UI-neutral page scene
        │               │
        │               ▼
        │         Embeddable GTK renderer
        ├───────┬───────┤
        ▼       ▼       ▼
 External    Desktop   External
 clients     viewer    host apps
```

## Input Lifecycle

1. At startup, the viewer scans the configurable default notebooks location
   under XDG Documents and opens every notebook folder it finds. The user can
   also choose an arbitrary notebook directory, `.onetoc2`, or `.one` through
   the desktop file chooser without moving it. Discovery canonicalizes each
   chosen root and records a source identity based on canonical path, file
   identities, sizes, and modification timestamps. A directory with a usable
   root `.onetoc2` follows the manifest hierarchy. A manifest-free backup
   directory follows the planned reusable
   [backup-folder loader](../plans/backup-folder-loader.md), which reconstructs
   one synthetic notebook and retains snapshot-selection provenance.
2. Selecting `.onepkg` starts the separate workflow in ADR 0002: an external
   7-Zip process extracts into an on-disk staging directory, the result is
   validated and atomically published, and the resulting directory returns to
   step 1. Package entries never become an in-memory notebook representation.
3. Discovery, parsing, and indexing run sequentially on one viewer-owned
   worker; scene construction and queries run in short independent jobs. GTK
   objects remain on the main thread. Package extraction has phase reporting
   and process cancellation; bounded scheduling and cancellation for the
   remaining long-running operations are still required before release.
4. The adapter converts parser objects to an immutable domain model and emits
   structured warnings for skipped or degraded content.
5. The indexer updates a transactionally isolated index generation after the
   parsed model becomes available to the UI.
6. A failed index replacement leaves its last good generation available. Full
   automatic/manual source refresh is not implemented yet.
7. Activating an attachment returns its stable resource action to the viewer.
   The viewer presents explicit Open and Save As choices. Both stream the lazy
   payload on a worker through the core resource API and a cancellable GIO
   replacement stream. Save As writes to the portal-selected `GFile`; Open
   writes to an XDG cache entry scoped by source identity, fingerprint, and
   resource identity, then uses `GtkFileLauncher`. No attachment is extracted
   during parsing, import, rendering, or indexing.

Source files are opened read-only. There is no write-back code path.
The immutable domain model preserves semantic objects and their source
geometry. It is not populated by parsing generated HTML, Markdown, PDF, or
converter output.

## Domain Model

The stable application model is:

```text
Workspace
  Notebook*
    SectionGroup*
      Section*
        Page* (ordered; may have a parent page)
          PageMetadata
          CanvasObject*
            Outline
              OutlineElement*
                RichText | Table | Image | Attachment | Ink | Unknown
            Image | Attachment | Ink | Unknown
```

Every object carries a stable source locator, optional geometry, semantic
content, and zero or more warnings. Unknown content remains represented so the
viewer can report that something was omitted instead of silently deleting it
from the mental model.

The domain model is a public application-independent contract owned by
`onenote-core`. It contains no GTK, viewer workspace, SQLite, or upstream
parser types. Source-scoped locators remain meaningful across the renderer and
index boundaries.

## Canvas

`onenote-render` creates a retained, UI-neutral page scene. The GTK adapter
renders that scene rather than creating one GTK child per text run:

- scene construction owns OneNote geometry, stacking, object bounds, hit
  regions, accessibility semantics, and explicit nested flow ordering;
- Pango lays out rich text with font fallback and bidirectional text in the GTK
  adapter;
- the GTK adapter resolves exact Pango and asynchronous math heights into one
  generation-cached geometry map, preserving source gaps within each flow;
- GSK snapshots draw backgrounds, text, images, table lines, tags, and ink.
- Interactive objects such as links and attachments expose hit regions and
  accessibility nodes.
- A spatial index identifies visible and clickable objects.
- Zoom changes the scene transform and invalidates adapter measurements. It does
  not mutate source geometry.
- Large pages render only objects intersecting the viewport plus an overscan
  margin.

OneNote layout values described as half-inch units normalize to logical pixels
at `96 dpi`: `logical_px = half_inches * 48`. Physical monitor DPI is handled
by GTK scaling. Ink has its own coordinate conversion and must use corpus-based
tests; it must not reuse the page-unit conversion blindly.

Neither scene construction nor the GTK renderer owns a window, workspace,
notebook tree, file chooser, URI launcher, or attachment policy. Host actions
are explicit callbacks. This makes the GTK component embeddable and leaves the
scene model available to future rendering backends.

## Search

SQLite FTS5 stores derived, normalized page documents. Indexing is per page so
results navigate directly to a page and, where geometry exists, the matching
canvas object. See [the search specification](../specs/search.md).

A workspace index includes every currently open notebook and namespaces
internal identities by source. Opening another notebook adds it to the
workspace instead of replacing the active set. Search result activation
selects the owning notebook, section, and page before scrolling the matching
canvas object into view.

The index is stored below the XDG cache directory and carries a schema version
plus per-source fingerprints. An incompatible schema currently returns a
structured rebuild-required error; automated rebuild and app/parser version
tagging remain open. Deleting the index never loses notebook data.

`onenote-index` exposes ingestion and structured query operations as a public
headless library. Search requests and results carry typed filters and stable
source/page/object locators; the SQLite schema and FTS query text remain
private. A versioned JSON Lines process adapter exposes the same contract to
non-Rust software without adding a network service.

## Public Integration Boundaries

The desktop viewer is a composition root, not the owner of privileged
functionality. Reusable boundaries are:

- `onenote-core`: source access, immutable domain model, diagnostics, locators;
- `onenote-render`: deterministic UI-neutral layout and `PageScene`;
- `onenote-render-gtk`: embeddable GTK page component;
- `onenote-index`: transactional ingestion and structured query API;
- headless query adapter: versioned protocol for non-Rust clients.

Each boundary is independently documented, testable without the viewer, free
of application-global state, cancellable, and resource-bounded. See
[the public API specification](../specs/public-api.md) and
[ADR 0003](../decisions/0003-reusable-components.md).

## Concurrency and Memory

- The viewer currently serializes discovery, parsing, and indexing on one
  worker and runs scene/search jobs separately. It requires bounded scheduling
  and operation cancellation before release.
- Package imports and attachment copies share one foreground file-operation
  coordinator. Operation identities reject stale worker progress/completion;
  only one destination-writing operation runs at a time, while scene and
  search jobs remain independent.
- Reusable index, scene, and package APIs accept caller cancellation at their
  documented checkpoints; parser projection cancellation is not yet public.
- Binary payloads are streamed and decoded lazily; attachments are not loaded
  while indexing unless a bounded extractor is explicitly enabled.
- Attachment copies use a fixed 64 KiB core buffer, an explicit per-file
  ceiling, cooperative cancellation, declared-size validation, and
  destination replacement rather than full-payload allocation.
- Images use encoded/decoded size and dimension limits plus a bounded texture
  cache.
- The index library reports progress through caller callbacks and the viewer
  adapts worker results to GLib polling. User-visible progress detail is not
  wired yet.
- Notebook failures are isolated in the viewer. Finer per-section/page parser
  recovery remains corpus-dependent.

Initial resource ceilings are defined in the limitations document and become
configuration only after real corpus measurements.

## Security Boundaries

- Reject absolute paths, `..`, device paths, and symlink escapes referenced by
  notebook metadata.
- Treat attachment display names as untrusted metadata. Save/cache leaf names
  are sanitized with portable Windows restrictions and bounded UTF-8 length;
  cache directories use hashed source/resource identities and private modes.
- Never execute attachments in-process. Opening first materializes a completed
  read-only cache file and delegates it to the desktop through `FileLauncher`.
- Sniff content from bytes; extensions are hints only.
- Bound every allocation derived from input lengths and dimensions.
- Treat text, URLs, filenames, and metadata as data, never markup.
- Never instantiate web content inside the process for an embedded URL.
- Never execute attachments. Extraction uses sanitized generated names in a
  user-approved destination, followed by an explicit open action.
- Open external links only after a user gesture; show and confirm local targets
  and unfamiliar custom schemes before handing them to the desktop.
- Package extraction runs out of process into a private staging directory and
  applies listing-size, entry-count, path, and file-type validation. Expanded
  byte and disk-space ceilings remain a release blocker.
- The Flatpak build requests no network permission for the core viewer.

## Packaging

The primary distributable target is Flatpak using a stable GNOME runtime,
document portals, and no network access. An AppImage provides an
installation-free alternative for current x86-64 distributions. Both portable
formats bundle a pinned external `7zz` process and its corresponding source;
the Flatpak runtime remains separate. Dynamically linked native products are
built and tested but not published. Development builds remain ordinary Cargo
builds so packaging does not hide dependency or runtime failures.

Reusable Rust crates, API documentation, examples, the headless query adapter,
and eventually the GObject-introspectable renderer wrapper are separate release
artifacts. Their versions and compatibility notes do not depend on the Flatpak
application version remaining identical.
