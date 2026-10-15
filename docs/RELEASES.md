# Packaging and Releases

This is the developer and release-maintainer guide. End-user setup belongs in
the [installation guide](INSTALL.md) and the short installation section on the
repository landing page.

GitHub releases publish runnable Linux preview artifacts for both `x86_64`
and `aarch64`:

- `OneNoteViewer-<version>-linux-<arch>.flatpak`, the primary release channel;
- `OneNoteViewer-<version>-<arch>.AppImage`, the secondary portable option.

**ARM64 (`aarch64`) support is experimental/beta.** Native ARM64 CI runs the
same test suite and packaged startup/icon/extractor checks as x86-64. Physical
ARM desktop, graphics-driver, notebook fidelity, and cross-distribution
validation remain limited. Keep this status visible in download instructions
and release notes until that coverage is established.

Both include the private, pinned `7zz` extractor. Native `.bin` files and
archives remain local and CI-only products because they are dynamically linked
for Ubuntu 24.04 or compatible hosts.

## Automated Builds

`.github/workflows/release.yml`:

- manually through **Actions > Release builds > Run workflow**;
- automatically for tags matching `v*`;
- builds and tests native binaries on `ubuntu-24.04` and `ubuntu-24.04-arm`
  without uploading them;
- builds Flatpak and AppImage workflow artifacts on each native architecture;
- creates tagged GitHub Releases containing only Flatpak, AppImage, and their
  SHA-256 checksums.

All six build jobs must pass before publication, including the experimental
ARM64 jobs. The publication job verifies the presence and checksum of all four
packages. To test a feature branch without publishing, dispatch the workflow
with that branch selected; only a `v*` tag publishes a release.

A release tag must equal `v` plus `[workspace.package].version`; for example,
version `0.4.0` uses tag `v0.4.0`. The same version must be the newest release
in `packaging/flatpak/io.github.emsi.OneNoteViewer.metainfo.xml`.

The workflow pins third-party actions by commit. Flatpak Cargo inputs,
linuxdeploy, appimagetool, the AppImage runtime, and the bundled 7-Zip binary
and source are all verified against committed SHA-256 values.
The Flatpak action receives an explicit architecture, and the manifest selects
the matching 7-Zip archive. AppImage tools and the extractor are selected for
the native build host; these scripts do not perform cross-compilation.

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
./dist/OneNoteViewer-*-$(uname -m).AppImage
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
compatibility or store readiness. AppStream metadata is included. Signed
artifacts, MIME associations, cross-distribution and portal test coverage,
reproducibility comparison, and the remaining security/accessibility/fidelity
gates are still required for a public stable release.
