# Release Builds

GitHub releases publish two runnable Linux preview artifacts: Flatpak and
AppImage. Neither requires Rust, Cargo, compiler headers, or `pkg-config` on
the machine used to run it. Native binaries remain local and CI-only build
products.

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
directory permission. It includes a private, pinned `7zz` executable, its
license, and corresponding source, so `.onepkg` import does not depend on a
host-installed extractor. Package input and the durable destination must be
selected through the application's portal-backed choosers.

### AppImage

`OneNoteViewer-<version>-x86_64.AppImage` is the installation-free portable
artifact. It bundles GTK and the other non-base runtime libraries, GTK data,
image loaders, and the private `7zz` extractor:

```bash
chmod +x OneNoteViewer-*-x86_64.AppImage
./OneNoteViewer-*-x86_64.AppImage /path/to/notebook
```

AppImages normally mount themselves through FUSE. On a host without compatible
FUSE support, use the built-in extraction fallback:

```bash
./OneNoteViewer-*-x86_64.AppImage --appimage-extract-and-run
```

The AppImage is built on Ubuntu 24.04. It is intended for current x86-64 Linux
desktop distributions with a compatible glibc, X11 or Wayland, graphics
drivers, fonts, and desktop services. Flatpak remains the stronger option when
the host distribution is older or materially different.

### Unpublished Native Build

`scripts/package-native-release.sh` still creates a quick-run `.bin`, native
archive, and corresponding-source archive under `dist/`. They are useful on
the development host and for CI smoke tests, but the workflow does not upload
or attach them to GitHub releases. They are dynamically linked and intended
for Ubuntu 24.04 or compatible hosts with GTK 4.14 or newer:

```bash
sudo apt install libgtk-4-1 libgraphene-1.0-0
./scripts/package-native-release.sh
./dist/OneNoteViewer-*-linux-x86_64.bin /path/to/notebook
```

The public source repository and parser fork are the preferred modification
forms. Runnable binaries report their source revision and license locations:

```bash
./onenote-viewer --source
./onenote-viewer --license
./onenote-viewer --third-party-notices
```

## Automated Builds

`.github/workflows/release.yml`:

- manually through **Actions > Release builds > Run workflow**;
- automatically for tags matching `v*`;
- always builds and tests the native binary without uploading it;
- builds Flatpak and AppImage workflow artifacts;
- creates tagged GitHub Releases containing only Flatpak, AppImage, and their
  SHA-256 checksums.

A release tag must equal `v` plus `[workspace.package].version`; for example,
version `0.1.0` uses tag `v0.1.0`.

The workflow pins third-party actions by commit. Flatpak Cargo inputs,
linuxdeploy, appimagetool, the AppImage runtime, and the bundled 7-Zip binary
and source are all verified against committed SHA-256 values.

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

Flatpak Builder uses bubblewrap and therefore requires user namespaces. It
cannot run inside an unprivileged Docker/dev container that blocks namespace
creation, even when Flatpak and its runtimes are installed. In that situation,
run the script directly on the host or use the GitHub Actions workflow.

AppImage preview:

```bash
sudo apt install \
  adwaita-icon-theme curl file libgtk-4-dev librsvg2-common patchelf \
  shared-mime-info
./scripts/build-appimage-release.sh
./dist/OneNoteViewer-*-x86_64.AppImage
```

All three scripts write ignored artifacts and checksum files under `dist/`.
Native packaging also creates a corresponding-source archive and refuses
tracked or staged changes so that its contents match the executable. CI
launches the native and AppImage executables under Xvfb. It also installs and
launches the generated Flatpak under Xvfb, and verifies from inside the
installed sandbox that its bundled `7zz` supports CAB archives. The AppImage
payload receives the same extractor check before upload. Each executable also
renders every application symbolic icon to pixels with its packaged GTK
runtime; missing, empty, or implausibly filled icons fail the workflow. The
same packaged-runtime check measures the collapsed navigation control and fails
if theme sizing would make it wider than its allocated band.

After changing `Cargo.lock`, regenerate the Flatpak source list:

```bash
./scripts/update-flatpak-sources.sh
```

The updater pins and verifies the official Flatpak Cargo generator before
running it through `uv`.

## Release Boundaries

These are unsigned preview/test artifacts, not a claim of stable OneNote
compatibility or store readiness. Signed artifacts, AppStream metadata,
cross-distribution and portal test coverage, and the remaining
security/accessibility/fidelity gates are still required for a public stable
release.
