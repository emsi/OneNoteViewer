# Install OneNote Viewer

Flatpak is the primary OneNote Viewer release channel. It supplies a consistent
GTK runtime across Linux distributions and includes the `7zz` tool required for
`.onepkg` import.

## Install

First install Flatpak and enable Flathub using the instructions for your Linux
distribution at [flatpak.org/setup](https://flatpak.org/setup/).

Then:

1. [Download the latest OneNote Viewer Flatpak](https://github.com/emsi/OneNoteViewer/releases/latest).
2. Open the downloaded file with your software center and install it.
3. Start **OneNote Viewer** from the desktop application menu.

The equivalent terminal command, run from the folder containing the downloaded
file, is:

```bash
flatpak install --user --or-update ./OneNoteViewer-0.1.3-linux-x86_64.flatpak
```

The bundle is published directly through GitHub rather than a Flatpak
repository. It therefore does not appear in Flathub search results.

## Verify the Download

Checksum verification is optional but recommended. Download
`OneNoteViewer-0.1.3-linux-x86_64.flatpak.sha256` from the
[latest release](https://github.com/emsi/OneNoteViewer/releases/latest) into
the same directory as the Flatpak, then run:

```bash
sha256sum --check OneNoteViewer-0.1.3-linux-x86_64.flatpak.sha256
```

Both files can also be downloaded from a terminal:

```bash
version=0.1.3
curl --fail --location --remote-name \
  "https://github.com/emsi/OneNoteViewer/releases/latest/download/OneNoteViewer-${version}-linux-x86_64.flatpak"
curl --fail --location --remote-name \
  "https://github.com/emsi/OneNoteViewer/releases/latest/download/OneNoteViewer-${version}-linux-x86_64.flatpak.sha256"
```

Or with GitHub CLI:

```bash
gh release download --repo emsi/OneNoteViewer \
  --pattern 'OneNoteViewer-*-linux-x86_64.flatpak*'
```

## Update

Version `0.1.0` was incorrectly published on the `master` Flatpak branch.
Remove that legacy branch once before installing `0.1.1`:

```bash
flatpak uninstall --user io.github.emsi.OneNoteViewer//master
```

This does not remove application data; Flatpak only removes it when explicitly
requested with `--delete-data`.

Then download the newer Flatpak and open it with the software center again, or
run:

```bash
flatpak install --user --or-update ./OneNoteViewer-0.1.3-linux-x86_64.flatpak
```

Because this is a directly distributed bundle, `flatpak update` cannot discover
new OneNote Viewer releases automatically.

## Remove

Remove OneNote Viewer through the software center or run:

```bash
flatpak uninstall --user io.github.emsi.OneNoteViewer
```

Removing the application does not delete notebooks.

## Notebook Access

Use the application's file and folder choosers to grant read-only access to
notebooks through the desktop portal. Host paths supplied as command-line
arguments are not automatically exposed inside the Flatpak sandbox.

The Flatpak has no network permission or unrestricted home-directory access.
Its bundled `7zz` extractor means `.onepkg` import does not depend on tools
installed on the host.

## AppImage Alternative

The [latest release](https://github.com/emsi/OneNoteViewer/releases/latest)
also provides an AppImage for running without installation:

```bash
chmod +x OneNoteViewer-*-x86_64.AppImage
./OneNoteViewer-*-x86_64.AppImage
```

If FUSE is unavailable or incompatible:

```bash
./OneNoteViewer-*-x86_64.AppImage --appimage-extract-and-run
```

The AppImage is built on Ubuntu 24.04 and requires a compatible x86-64 Linux
host. Flatpak is the more portable option across significantly different
distributions.
