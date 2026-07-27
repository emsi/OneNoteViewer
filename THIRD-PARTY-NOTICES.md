# Third-Party Notices

This file defines the licensing boundary for the OneNote Viewer repository and
its release artifacts.

## OneNote Viewer

Copyright (c) 2026 Mariusz Woloszyn

Except for the material identified below, the source code and project-authored
documentation in this repository are licensed under the GNU General Public
License, version 3 or (at your option) any later version
(`GPL-3.0-or-later`). The complete license is in `LICENSE`.

## OneNote Parser

The application depends on `onenote_parser` and
`onenote_parser-macros` from:

- source: <https://github.com/emsi/onenote.rs>
- revision: `3cc4e985d842c76dc04055955b460713d6f6ea24`
- license: Mozilla Public License 2.0 (`MPL-2.0`)

That revision contains local compatibility changes described in
`third_party/onenote.rs/PATCHES.md`. A source snapshot and the complete MPL 2.0
text are retained under `third_party/onenote.rs/`.

Under section 3.3 of MPL 2.0, the MPL-covered parser source included in the
combined GPL executable is additionally made available under
`GPL-3.0-or-later`. Recipients may continue to use the covered parser source
under MPL 2.0. MPL notices, attribution, and source availability are not
removed or restricted.

## Rust Dependencies

`THIRD-PARTY-LICENSES.html` contains the license texts, versions, and source
links for the complete locked Cargo dependency graph. It is generated from
`Cargo.lock` with the pinned process documented in `scripts/check-licenses.sh`.

The application dynamically uses GTK, GLib, Pango, Cairo, Graphene, HarfBuzz,
and their platform dependencies. Native builds do not bundle those system
libraries. Flatpak obtains them from the separately distributed GNOME runtime.
The AppImage bundles the runtime libraries identified by the license and
package metadata under its `usr/share/doc/` directory.

SQLite is compiled through the `rusqlite` `bundled` feature. SQLite is in the
public domain: <https://www.sqlite.org/copyright.html>.

## Icons

The icons under `crates/onenote-viewer/resources/icons/` are from Lucide or are
derived from Feather. They remain under their ISC and MIT licenses,
respectively. The complete notices are in
`crates/onenote-viewer/resources/LUCIDE-LICENSE`.

## Microsoft Reference Documents

Files under `docs/references/microsoft/` are Microsoft specifications retained
as implementation references. They are not covered by the OneNote Viewer GPL
grant. Each document remains subject to its embedded Microsoft intellectual
property notice. Provenance, version pins, hashes, and upstream locations are
recorded in `docs/references/README.md` and
`docs/references/SHA256SUMS`.

Microsoft names and product names are trademarks of their respective owners.
OneNote Viewer is an independent project and is not affiliated with or endorsed
by Microsoft.

## Package Extractor

OneNote Viewer invokes `7zz` or `7z` as a separate process for one-time
`.onepkg` extraction. Native development builds use an installed executable.
The Flatpak and AppImage bundle the official 7-Zip 26.02 `7zzs` console
executable as `7zz`; it is not linked into OneNote Viewer.

7-Zip is copyright Igor Pavlov and is distributed under the GNU Lesser General
Public License 2.1 or later, with additional component notices described in its
`License.txt`. Portable artifacts include that exact license and the unmodified
`7z2602-src.tar.xz` corresponding source archive. Upstream source:
<https://github.com/ip7z/7zip/releases/tag/26.02>.
