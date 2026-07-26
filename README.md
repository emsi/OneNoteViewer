# OneNote Viewer for Linux

OneNote Viewer is a planned mature, native Linux desktop application for
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

This repository currently contains the implementation specification,
architecture decisions, delivery plan, and a versioned copy of the primary
format references. Application code starts with the milestone 1 parser,
renderer, and search feasibility work.

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
- [Roadmap and acceptance gates](docs/plans/roadmap.md)
- [Reference provenance](docs/references/README.md)

## Status

**Planning baseline established; implementation not started.**

The supplied private `.onepkg` has been structurally extracted and validated,
but semantic parser and renderer compatibility is not yet proven. The first
implementation gate is a corpus-backed proof that the selected Rust parser can
read representative OneNote Desktop notebooks without data loss. The project
must not claim broad format compatibility until the fixture matrix in the
roadmap is exercised.

## Repository Shape

The intended code layout is a modular Cargo workspace:

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

Directories are added when they gain real contents. See the
[repository layout](docs/architecture/repository-layout.md) for ownership
rules and dependency direction.

## Safety Baseline

Notebook files are untrusted input. Parsing is bounded, source paths are
canonicalized, attachment names are sanitized, external links require user
activation, and embedded files are never executed or previewed in-process.
The SQLite index is disposable derived data under the user's XDG data/cache
directories.

## Licensing

No project source license has been selected yet. That decision must be made by
the copyright owner before application code is accepted. Downloaded reference
documents retain their original terms; see
[reference provenance](docs/references/README.md).
