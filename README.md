# OneNote Viewer for Linux

OneNote Viewer is a native Linux desktop application for opening, indexing, and
searching local Microsoft OneNote notebooks while preserving their freeform page
layout. It reads native OneNote data directly and imports `.onepkg` notebook
exports instead of converting pages to HTML, Markdown, PDF, or another linear
format.

## Install

Flatpak is the primary release channel.

1. [Download OneNote Viewer 0.1.5](https://github.com/emsi/OneNoteViewer/releases/latest/download/OneNoteViewer-0.1.5-linux-x86_64.flatpak).
2. Open the downloaded file with your software center and install it.

Alternatively, install it from a terminal opened in the download folder:

```bash
flatpak install --user --or-update ./OneNoteViewer-0.1.5-linux-x86_64.flatpak
```

Start **OneNote Viewer** from the desktop application menu. Flatpak and the
Flathub repository must already be configured; the
[installation guide](docs/INSTALL.md) covers that setup, checksum verification,
updates, removal, and the AppImage alternative.

## Screenshots

### Light Theme

![OneNote Viewer displaying a notebook in the light theme](docs/images/onenote-viewer-light.png)

### Dark Theme

![OneNote Viewer displaying a notebook in the dark theme](docs/images/onenote-viewer-dark.png)

## Highlights

- Reconstructs positioned and overlapping rich text, lists, tables, images,
  printouts, and OfficeMath equations on the freeform page canvas.
- Imports complete `.onepkg` notebook exports through a guided, validated,
  on-disk process with progress and cancellation.
- Keeps multiple notebooks open in one workspace with nested section groups,
  collapsible navigation, and one search across page titles and stored content.
- Searches OneNote's stored image alternative and recognition text.
- Preserves explicit OneNote links, optionally recognizes visible URLs and email
  addresses, and resolves internal page links across open notebooks.
- Displays stored attachment icons and previews, and can save or open the
  original files through the desktop with progress and cancellation.
- Restores the workspace and last viewed page, remembers the zoom level, and
  provides persistent light, dark, and system theme choices.
- The current development build adds Back and Forward history across notebooks,
  search results, and internal links through the application menu,
  `Alt+Left`/`Alt+Right`, and mouse navigation buttons. This will be included in
  the next release after 0.1.5.

## Open Notebooks

Use **Open Notebook Folder...** for a locally copied OneNote notebook directory,
or **Open OneNote File...** for a standalone `.one` section. Additional notebook
directories join the same searchable workspace without being moved. Notebook
folders placed under the configurable default notebooks location open
automatically on the next launch.

## Import a OneNote Package

A `.onepkg` file is a convenient single-file export of a complete notebook.
OneNote Viewer imports it once into a normal notebook folder, validates the
result, and then opens that folder like any other notebook. The original package
is not modified, and normal viewing does not depend on the extractor afterward.

### Export From OneNote

Full-notebook package export is available in OneNote desktop for Windows:

1. Open the notebook and allow OneNote to finish synchronizing it.
2. Choose **File > Export**.
3. Under **Export Current**, choose **Notebook**.
4. Choose **OneNote Package (`*.onepkg`)** as the format.
5. Select **Export** and save the package to a local folder.

Microsoft documents notebook export as a Windows desktop capability in its
[OneNote data-protection and recovery overview](https://support.microsoft.com/en-US/OneNote/onenote-platform-data-protection-recovery).
The OneNote for the web download workflow is different and is limited by
Microsoft to notebooks stored in personal OneDrive accounts.

### Import Into OneNote Viewer

1. Open the application menu and choose **Import OneNote Package...**.
2. Select the `.onepkg` file.
3. Review the destination notebook folder. It is created under the configured
   default notebooks location unless **Change Location** selects another parent.
4. Select **Import**. The progress display reports extraction and validation;
   the operation can be cancelled before publication completes.

Flatpak and AppImage releases include the required `7zz` extractor, so package
import does not depend on a separately installed host tool.

## Current Limitations

Two missing capabilities currently prevent a 1.0 release:

- rendering hand-drawn ink strokes
  ([issue #6](https://github.com/emsi/OneNoteViewer/issues/6));
- selecting and copying text from the freeform page body
  ([issue #8](https://github.com/emsi/OneNoteViewer/issues/8)).

See [known limitations](docs/limitations.md) for stable product boundaries.
Current bugs, compatibility gaps, and planned improvements are tracked in
[GitHub issues](https://github.com/emsi/OneNoteViewer/issues).

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
- [Known limitations](docs/limitations.md)
- [Installation guide](docs/INSTALL.md)
- [Packaging and release guide](docs/RELEASES.md)
- [Roadmap and acceptance gates](docs/plans/roadmap.md)
- [Reference provenance](docs/references/README.md)

## Status

**OneNote Viewer is practical for everyday use.** The project owner uses it to
access, search, and read 15 years of personal notes. Native notebook loading,
freeform rendering, multi-notebook navigation and search, `.onepkg` import,
attachments, links, workspace restoration, and portable Linux packages are all
working today.

Compatibility is exercised with private notebooks created across OneNote 2010
through modern Microsoft 365 and OneNote for the web. Those notebooks are not
committed to the repository. The remaining 1.0 blockers are hand-drawn ink
rendering and freeform page text selection/copy; other current work is tracked
in [GitHub issues](https://github.com/emsi/OneNoteViewer/issues).

## Repository Shape

Native parsing, UI-neutral page-scene construction, GTK rendering, and search
live behind documented component boundaries. Other note-taking and
knowledge-management applications can render `.one` sections or query relevant
pages without adopting the complete desktop application.

The code is a modular Cargo workspace:

```text
crates/
  onenote-core/        Read-only domain model and parser adapter
  onenote-render/      UI-neutral page layout and retained scene
  onenote-render-gtk/  Embeddable GTK4 OneNote page widget
  onenote-index/       Rebuildable index and public query API
  onenote-viewer/      GTK4 desktop application/composition root
docs/               Architecture, decisions, specifications, and plans
packaging/          Flatpak and distribution metadata
scripts/            Reproducible maintenance and validation commands
```

See the [repository layout](docs/architecture/repository-layout.md) for
ownership rules and dependency direction.

## Development

Ubuntu 24.04 development requires Rust 1.85.1 and the GTK 4.14 development
stack:

```bash
sudo apt update
sudo apt install build-essential pkg-config libgtk-4-dev
./scripts/check-system-deps.sh
cargo run -p onenote-viewer -- /path/to/notebook
cargo test --workspace --all-targets
```

`libgtk-4-dev` supplies the required Graphene, Pango, Cairo, GDK, and GSK
development metadata. An unset `PKG_CONFIG_PATH` is normal for distribution
packages. The standalone reusable renderer can be exercised with:

```bash
cargo run -p onenote-render-gtk --example standalone -- /path/to/section.one
```

See the [packaging and release guide](docs/RELEASES.md) for local artifact
builds and the tagged GitHub release workflow.

## Safety Baseline

Notebook files are untrusted input. Current code canonicalizes sources, applies
projection and payload limits, decodes images lazily with allocation ceilings,
validates package paths, stages extraction privately, and publishes packages
atomically. It never writes notebook sources. The SQLite index is disposable
derived data under the user's XDG cache directory. Remaining hostile-input and
external-action work is tracked explicitly rather than implied complete.

## Licensing

OneNote Viewer is free software licensed under the
[GNU General Public License, version 3 or later](LICENSE).

The pinned OneNote parser and retained parser source remain available
under MPL-2.0 and are additionally distributed under GPL-3.0-or-later as part
of the combined application under MPL 2.0 section 3.3. Lucide and
Feather-derived icons retain their ISC/MIT terms. Microsoft reference
documents are not covered by the project GPL and retain their embedded terms.
See [third-party notices](THIRD-PARTY-NOTICES.md), the generated
[dependency license report](THIRD-PARTY-LICENSES.html), and
[corresponding-source information](SOURCE-CODE.md).

## Trademark Notice

Microsoft, Microsoft 365, OneNote, and Windows are trademarks of the Microsoft
group of companies. OneNote Viewer is an independent project and is not
affiliated with, endorsed by, sponsored by, or otherwise associated with
Microsoft. See Microsoft's
[Trademark and Brand Guidelines](https://www.microsoft.com/en-us/legal/intellectualproperty/trademarks).
