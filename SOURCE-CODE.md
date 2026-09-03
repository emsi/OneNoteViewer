# Corresponding Source

OneNote Viewer is free software licensed under `GPL-3.0-or-later`.

The preferred form for modification, including build and packaging scripts, is
available at:

<https://github.com/emsi/OneNoteViewer>

For a released binary, use the tag named in the release or the source revision
reported by:

```sh
onenote-viewer --source
```

The exact active MPL-2.0 OneNote parser source is available at:

<https://github.com/emsi/onenote.rs/tree/77cf881df7579eb58972e3db8ce0ca34d25a7f62>

That revision is based on upstream `v2.0.0`:

<https://github.com/msiemens/onenote.rs/tree/v2.0.0>

Portable releases include the unmodified 7-Zip 26.02 `7zzs` executable for
`.onepkg` extraction. Its corresponding source archive is included inside each
Flatpak and AppImage at
`share/sources/io.github.emsi.OneNoteViewer/7z2602-src.tar.xz`. The pinned
upstream release is:

<https://github.com/ip7z/7zip/releases/tag/26.02>

Each portable artifact also includes `THIRD-PARTY-NOTICES.md`,
`THIRD-PARTY-LICENSES.html`, the GPL text, the MPL text, and the icon notices.
`Cargo.lock` and the scripts in the source repository pin the dependency graph
and control compilation and packaging.

If source access at either URL becomes unavailable while a binary is still
being distributed, contact the copyright holder through the repository issue
tracker. This notice does not limit any source-delivery right granted by the
GPL or MPL.
