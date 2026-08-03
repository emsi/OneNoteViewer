# Packaging and Releases

This is the developer and release-maintainer guide. End-user setup belongs in
the [installation guide](INSTALL.md) and the short installation section on the
repository landing page.

GitHub releases publish two runnable Linux preview artifacts:

- `OneNoteViewer-<version>-linux-x86_64.flatpak`, the primary release channel;
- `OneNoteViewer-<version>-x86_64.AppImage`, the secondary portable option.

Both include the private, pinned `7zz` extractor. Native `.bin` files and
archives remain local and CI-only products because they are dynamically linked
for Ubuntu 24.04 or compatible hosts.

## Automated Builds

`.github/workflows/release.yml`:

- manually through **Actions > Release builds > Run workflow**;
- automatically for tags matching `v*`;
- always builds and tests the native binary without uploading it;
- builds Flatpak and AppImage workflow artifacts;
- creates tagged GitHub Releases containing only Flatpak, AppImage, and their
  SHA-256 checksums.

A release tag must equal `v` plus `[workspace.package].version`; for example,
version `0.1.2` uses tag `v0.1.2`. The same version must be the newest release
in `packaging/flatpak/io.github.emsi.OneNoteViewer.metainfo.xml`.

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
