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
and their platform dependencies. Native archives do not bundle those system
libraries. Flatpak obtains them from the separately distributed GNOME runtime.

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

## External Package Extractor

OneNote Viewer can invoke an installed `7zz` or `7z` executable as a separate
process for one-time `.onepkg` extraction. The extractor is not linked into or
bundled with OneNote Viewer. 7-Zip is separately distributed under its own
terms; see <https://www.7-zip.org/license.html>.
