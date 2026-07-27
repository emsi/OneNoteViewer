# OneNote Viewer for Linux

OneNote Viewer is a native Linux desktop application for
opening, viewing, indexing, and searching local Microsoft OneNote notebooks.
It reads the native `.one` and `.onetoc2` data directly and reconstructs the
OneNote freeform page canvas; HTML, Markdown, PDF, or another linear document
format is never its canonical representation. It is deliberately read-only:
source notebook files are never modified.

Multiple notebook directories remain open together in one workspace, with the
familiar notebook/section-group/section/page/canvas hierarchy and one search
scope across all of them. This is a desktop viewer, not a converter or an
import bridge to another notes application.

The viewer is also a reference consumer of reusable components. Native
OneNote parsing, page layout/scene construction, GTK rendering, and
index/query behavior live behind documented library boundaries so other
note-taking and knowledge-management software can render `.one` sections or
find relevant OneNote pages without adopting the complete desktop application.

This repository contains a working Rust/GTK implementation, its specifications
and architecture decisions, and a versioned copy of the primary format
references. The implementation is a functional pre-release baseline; it is not
yet a broad format-compatibility or visual-fidelity claim.

## Scope

The initial product:

- opens local notebook directories containing `.onetoc2` and `.one` files;
- opens standalone `.one` sections;
- accepts `.onepkg` export packages through a one-time, managed extraction to
  a normal directory tree of `.onetoc2` and `.one` files;
- reads both desktop revision-store files and locally downloaded FSSHTTP
  packaged files, without contacting OneDrive;
- renders source-native OneNote pages as a spatial, freeform canvas, preserving
  coordinates, overlap, sizes, backgrounds, ink, and object relationships;
- searches titles, text, tags, image alternative text, handwriting recognition
  text, link targets, and attachment names across all open notebooks;
- extracts attachments only after an explicit user action and opens them with
  the desktop's registered application.

Cloud synchronization, editing, collaboration, and executing embedded content
are outside the initial scope. Export or conversion to HTML, Markdown, or PDF
is also outside the viewer's load/render/index pipeline because it would
discard or flatten the layout this project exists to preserve.

## Documentation

Start with the **[master plan](docs/MASTER-PLAN.md)**. It is the canonical
entry point for project scope, deliverables, current status, execution order,
and document authority. The supporting documents are:

- [Documentation map](docs/README.md)
- [Technology decision](docs/decisions/0001-technology-stack.md)
- [Product requirements](docs/specs/product-requirements.md)
- [System architecture](docs/architecture/system-architecture.md)
- [ONEPKG extraction decision](docs/decisions/0002-onepkg-extraction.md)
- [Reusable component decision](docs/decisions/0003-reusable-components.md)
- [OneNote parsing profile](docs/specs/onenote-format.md)
- [Public integration API](docs/specs/public-api.md)
- [Persisted feature inventory](docs/specs/feature-matrix.md)
- [Known limitations and risks](docs/limitations.md)
- [Remaining release work](docs/REMAINING-WORK.md)
- [Release builds](docs/RELEASES.md)
- [Roadmap and acceptance gates](docs/plans/roadmap.md)
- [Reference provenance](docs/references/README.md)

## Status

**Functional implementation baseline; compatibility and fidelity hardening are
still in progress.**

The five-crate workspace now parses native notebook trees, performs bounded
on-disk `.onepkg` extraction, builds UI-neutral page scenes, renders them in an
embeddable GTK widget, transactionally indexes multiple sources, exposes a
versioned JSON Lines query process, and composes those components into a
persistent multi-notebook GTK viewer. The supplied private package passes
extraction, all-section parse, all-page scene, indexing, search, standalone
renderer, and full viewer Xvfb tests.

The tested corpus is still one private desktop package. Accessibility, measured
layout fidelity, hostile-input coverage, several user actions, refresh,
packaging, and licensing remain release blockers. See
[remaining work](docs/REMAINING-WORK.md) for the exact gaps and completion
evidence required.

## Build and Run

Ubuntu 24.04 development requires Rust 1.85.1 and the GTK 4.14 development
stack. Install the native build dependencies with:

```bash
sudo apt update
sudo apt install build-essential pkg-config libgtk-4-dev
```

`libgtk-4-dev` pulls in the required Graphene, Pango, Cairo, GDK, and GSK
development metadata. `7zz` or `7z` is optional for package import and is not
needed to open an extracted notebook; on Ubuntu 24.04 it can be installed with
`sudo apt install 7zip`.

Check the host before compiling:

```bash
./scripts/check-system-deps.sh
```

```bash
cargo run -p onenote-viewer -- /path/to/notebook
cargo run -p onenote-render-gtk --example standalone -- /path/to/section.one
cargo test --workspace --all-targets
```

If Cargo reports that `gtk4.pc` or `graphene-gobject-1.0.pc` is missing, the
development packages are not installed on that host. An unset
`PKG_CONFIG_PATH` is normal for distribution packages; do not set it manually
unless GTK was deliberately installed under a nonstandard prefix.

## Portable Release Builds

GitHub Actions and local scripts produce a Flatpak bundle and an optimized
native Linux archive. The Flatpak is the recommended artifact for testing on
different distributions because it supplies a consistent GTK runtime:

```bash
flatpak install --user ./OneNoteViewer-linux-x86_64.flatpak
flatpak run io.github.emsi.OneNoteViewer
```

See [release builds](docs/RELEASES.md) for artifact selection, local build
commands, checksums, sandbox constraints, and tag-based GitHub releases.

The first command accepts a `.one`, `.onetoc2`, or directory. Additional paths
add sources to the same workspace. `.onepkg` files are imported through the
viewer so a durable destination can be selected.

## Repository Shape

The code is a modular Cargo workspace:

```text
crates/
  onenote-core/        Read-only domain model and parser adapter
  onenote-render/      UI-neutral page layout and retained scene
  onenote-render-gtk/  Embeddable GTK4 OneNote page widget
  onenote-index/       Rebuildable index and public query API
  onenote-viewer/      GTK4 desktop application/composition root
docs/               Architecture, decisions, specifications, and plans
fixtures/           Small redistributable synthetic notebooks
packaging/          Flatpak and distribution metadata
scripts/            Reproducible maintenance and validation commands
```

See the [repository layout](docs/architecture/repository-layout.md) for
ownership rules and dependency direction.

## Safety Baseline

Notebook files are untrusted input. Current code canonicalizes sources, applies
projection and payload limits, decodes images lazily with allocation ceilings,
validates package paths, stages extraction privately, and publishes packages
atomically. It never writes notebook sources. The SQLite index is disposable
derived data under the user's XDG cache directory. Remaining hostile-input and
external-action work is tracked explicitly rather than implied complete.

## Licensing

No project source license has been selected yet. That decision must be made
before redistribution or external contributions. The vendored parser retains
MPL-2.0; downloaded reference documents retain their original terms. See
[reference provenance](docs/references/README.md).
