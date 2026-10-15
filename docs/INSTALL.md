# Install OneNote Viewer

Flatpak is the primary OneNote Viewer release channel. It supplies a consistent
GTK runtime across Linux distributions and includes the `7zz` tool required for
`.onepkg` import.

Releases provide both Flatpak and AppImage packages for these architectures:

| Processor / `uname -m` | Filename architecture | Status |
| --- | --- | --- |
| Intel/AMD 64-bit (`x86_64`) | `x86_64` | Existing support |
| ARM64 (`aarch64`) | `aarch64` | **Experimental/beta** |

ARM64 packages pass automated tests and packaged startup checks on native ARM64
CI runners. Testing on physical ARM desktops, graphics drivers, and different
distributions remains limited. Please report ARM64 problems with your device,
distribution, package format, and application version.

## Install

First install Flatpak and enable Flathub using the instructions for your Linux
distribution at [flatpak.org/setup](https://flatpak.org/setup/).

Then:

1. [Download the latest OneNote Viewer Flatpak](https://github.com/emsi/OneNoteViewer/releases/latest)
   matching your architecture in the table above.
2. Open the downloaded file with your software center and install it.
3. Start **OneNote Viewer** from the desktop application menu.

The equivalent terminal command, run from the folder containing the downloaded
file, is:

```bash
arch=$(flatpak --default-arch)
flatpak install --user --or-update ./OneNoteViewer-0.4.0-linux-${arch}.flatpak
```

The bundle is published directly through GitHub rather than a Flatpak
repository. It therefore does not appear in Flathub search results.

## Verify the Download

Checksum verification is optional but recommended. Download
the matching `OneNoteViewer-0.4.0-linux-<arch>.flatpak.sha256` from the
[latest release](https://github.com/emsi/OneNoteViewer/releases/latest) into
the same directory as the Flatpak, then run:

```bash
arch=$(flatpak --default-arch)
sha256sum --check OneNoteViewer-0.4.0-linux-${arch}.flatpak.sha256
```

Both files can also be downloaded from a terminal:

```bash
version=0.4.0
arch=$(flatpak --default-arch)
curl --fail --location --remote-name \
  "https://github.com/emsi/OneNoteViewer/releases/latest/download/OneNoteViewer-${version}-linux-${arch}.flatpak"
curl --fail --location --remote-name \
  "https://github.com/emsi/OneNoteViewer/releases/latest/download/OneNoteViewer-${version}-linux-${arch}.flatpak.sha256"
```

Or with GitHub CLI:

```bash
arch=$(flatpak --default-arch)
gh release download --repo emsi/OneNoteViewer \
  --pattern "OneNoteViewer-*-linux-${arch}.flatpak*"
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
arch=$(flatpak --default-arch)
flatpak install --user --or-update ./OneNoteViewer-0.4.0-linux-${arch}.flatpak
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
also provides an AppImage for each architecture. Download the one matching
`uname -m` (`x86_64` or `aarch64`), then run:

```bash
arch=$(uname -m)
chmod +x "OneNoteViewer-0.4.0-${arch}.AppImage"
"./OneNoteViewer-0.4.0-${arch}.AppImage"
```

If FUSE is unavailable or incompatible:

```bash
arch=$(uname -m)
"./OneNoteViewer-0.4.0-${arch}.AppImage" --appimage-extract-and-run
```

AppImages are built on Ubuntu 24.04 for their respective architectures and
require compatible host system libraries, including glibc. They are not
guaranteed to run on older distributions. ARM64 AppImages are experimental/beta,
just like the ARM64 Flatpak. Flatpak is the more portable option across
significantly different distributions.
