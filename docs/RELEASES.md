# Release Builds

The project produces two Linux preview artifacts. Neither requires Rust,
Cargo, compiler headers, or `pkg-config` on the machine used to run it.

## Artifact Choice

### Flatpak Bundle

`OneNoteViewer-linux-x86_64.flatpak` is the recommended cross-distribution
artifact. It uses the GNOME 50 runtime, so the GTK version is independent of
the host distribution.

Install and run:

```bash
flatpak install --user ./OneNoteViewer-linux-x86_64.flatpak
flatpak run io.github.emsi.OneNoteViewer
```

Use the application's folder/file chooser to grant read-only access to
notebooks through the desktop portal. Host paths passed as command-line
arguments are not exported into the sandbox automatically.

The preview Flatpak intentionally has no network permission or broad home
directory permission. `.onepkg` extraction is disabled in practice because
the sandbox does not include `7zz`/`7z`; select an already extracted notebook
tree. A reviewed bundled extractor remains release work.

### Native Archive

`OneNoteViewer-<version>-linux-x86_64.tar.gz` is smaller and can be unpacked
anywhere. It is dynamically linked and intended for Ubuntu 24.04 or compatible
hosts with GTK 4.14 or newer:

```bash
sudo apt install libgtk-4-1 libgraphene-1.0-0
tar -xzf OneNoteViewer-*-linux-x86_64.tar.gz
./OneNoteViewer-*-linux-x86_64/onenote-viewer /path/to/notebook
```

The archive includes the exact `ldd` runtime-library inventory from its build
host. Prefer Flatpak when testing on a different distribution.

## Automated Builds

`.github/workflows/release.yml` builds both artifacts:

- manually through **Actions > Release builds > Run workflow**;
- automatically for tags matching `v*`;
- tagged builds create a GitHub Release and attach both artifacts plus SHA-256
  checksums.

A release tag must equal `v` plus `[workspace.package].version`; for example,
version `0.1.0` uses tag `v0.1.0`.

The workflow pins third-party actions by commit. Flatpak Cargo inputs are
downloaded by checksum from `packaging/flatpak/cargo-sources.json`, generated
from `Cargo.lock`.

## Local Builds

Native preview:

```bash
./scripts/package-native-release.sh
```

Flatpak preview:

```bash
sudo apt install flatpak flatpak-builder
./scripts/build-flatpak-release.sh
```

Both scripts write ignored artifacts and checksum files under `dist/`.

After changing `Cargo.lock`, regenerate the Flatpak source list:

```bash
./scripts/update-flatpak-sources.sh
```

The updater pins and verifies the official Flatpak Cargo generator before
running it through `uv`.

## Release Boundaries

These are unsigned preview/test artifacts, not a claim of stable OneNote
compatibility or store readiness. A project source license, signed artifacts,
AppStream metadata, bundled/sandboxed package extraction, portal testing, and
the remaining security/accessibility/fidelity gates are still required for a
public stable release.
