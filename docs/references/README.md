# Reference Manifest

## Policy

The repository stores primary specifications that are necessary to implement
the file reader and whose publishers permit implementation copies. Each file
is versioned and hashed. `SHA256SUMS` is the integrity authority.

Web pages and source repositories that change frequently are recorded with
exact commit pins where possible. Third-party wiki text is not copied into the
repository because its redistribution license is not explicit; this project's
own format profile independently summarizes the required implementation facts.

Last research refresh: **2026-07-26 UTC**.

## Archived Microsoft Specifications

| Local file | Version/release | Pages | Purpose | Upstream |
|---|---:|---:|---|---|
| `microsoft/MS-CAB-v20110304.pdf` | 2011-03-04 | 20 | Normative Cabinet container used by observed `.onepkg` exports | [Direct published PDF](https://download.microsoft.com/download/4/d/a/4da14f27-b4ef-4170-a6e6-5b1ef85b1baa/%5Bms-cab%5D.pdf) |
| `microsoft/MS-ONE-v20221115.pdf` | 3.4, 2022-11-15 | 108 | Normative OneNote semantic property sets and features | [Microsoft Learn landing page](https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-one/73d22548-a613-4350-8c23-07d15576be50) |
| `microsoft/MS-ONESTORE-v20250520.pdf` | 13.3, 2025-05-20 | 101 | Normative desktop revision store and OneStore persistence | [Microsoft Learn landing page](https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-onestore/ae670cd2-4b38-4b24-82d1-87cfb2cc3725) |
| `microsoft/MS-FSSHTTPB-v20240820.pdf` | 24.0, 2024-08-20 | 117 | Binary packaging for locally downloaded FSSHTTP files | [Microsoft Learn landing page](https://learn.microsoft.com/en-us/openspecs/sharepoint_protocols/ms-fsshttpb/f59fc37d-2232-4b14-baac-25f98e9e7b5a) |
| `microsoft/MS-DOC-v20260217.pdf` | 12.5, 2026-02-17 | 580 | Character-position (`CP`) rules referenced by `[MS-ONE]` | [Microsoft Learn landing page](https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-doc/ccd7b486-7881-484c-a137-51170af7cc22) |
| `microsoft/MS-LCID-v20240423.pdf` | 16.0, 2024-04-23 | 61 | Language-code identifiers referenced by `[MS-ONE]` | [Microsoft Learn landing page](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-lcid/70feba9f-294e-491e-b6eb-56532684c37f) |
| `microsoft/MS-OSHARED-v20251113.pdf` | 2025-11-13 | 199 | Referenced Office common data types | [Direct published PDF](https://officeprotocoldocs-f5hpbjgea6b8gneq.b02.azurefd.net/files/MS-OSHARED/%5BMS-OSHARED%5D.pdf) |
| `microsoft/MS-DTYP-v20241119.pdf` | 2024-11-19 | 155 | Referenced Windows GUID, FILETIME, and data types | [Direct published PDF](https://winprotocoldocs-bhdugrdyduf5h2e4.b02.azurefd.net/MS-DTYP/%5BMS-DTYP%5D.pdf) |
| `microsoft/MS-OFFCRYPTO-v20260217.pdf` | 2026-02-17 | 119 | Encryption research for the explicitly deferred protected-section feature | [Direct published PDF](https://officeprotocoldocs-f5hpbjgea6b8gneq.b02.azurefd.net/files/MS-OFFCRYPTO/%5BMS-OFFCRYPTO%5D.pdf) |
| `microsoft/Ink-Serialized-Format.pdf` | Point-in-time specification | 49 | Related signed multibyte/differential ink encoding | [Microsoft download](https://download.microsoft.com/download/0/B/E/0BE8BDD7-E5E8-422A-ABFD-4342ED7AD886/InkSerializedFormat%28ISF%29Specification.pdf) |

The Open Specifications notice inside the Microsoft documents permits copies
for developing implementations and distribution of relevant portions, subject
to the complete intellectual-property notice in each PDF. The ISF document
states that it is provided under the Microsoft Open Specification Promise.
These observations are not legal advice.

`[MS-FSSHTTP]` is not archived because it specifies the SOAP synchronization
exchange, which this offline viewer does not implement. `[MS-FSSHTTPB]` is
archived because its binary persistence structures are present in local
OneNote files. Stable RFCs referenced by the Microsoft specifications are
linked from those documents and are not duplicated here.

Verify the local archive:

```sh
sha256sum --check docs/references/SHA256SUMS
```

## Critical Gap-Filling Sources

### Current Rust Parser

- Repository: [msiemens/onenote.rs](https://github.com/msiemens/onenote.rs)
- Inspected commit:
  `f9cdc59f984bc1f7f096b54100cefaaebc892573`
  (2026-07-23)
- License: MPL-2.0
- Why it matters: current HEAD implements desktop and FSSHTTP OneStore paths,
  semantic projection, ink/math, lazy payloads, warnings, stored handwriting
  recognition, and an unreleased `.onepkg` reader.
- Caution: published version 1.1.1 does not contain all inspected `master`
  capabilities. Its package API buffers the archive and expanded entries in
  memory, so this application does not use that API; see ADR 0002.

### Informal Missing Specifications

- Wiki: [onenote.rs wiki](https://github.com/msiemens/onenote.rs/wiki)
- Inspected wiki commit:
  `4c208d40a328b5f7e12cfd0a893ed2029fefe00b`
- Pages: `Ink Object Format`, `Math Inline Object Format`
- Why it matters: Microsoft does not define these OneNote object graphs.
- Caution: unofficial, empirical, and no explicit wiki redistribution license
  was found; validate every implemented class against fixtures.

### Reference Renderer

- Repository: [msiemens/one2html](https://github.com/msiemens/one2html)
- Inspected commit:
  `59930ad309004030f812790c6749efe4265bb6bd`
  (2026-05-28)
- Release checked: 1.3.1; license MIT
- Why it matters: concrete layout, list, table, image, ink, and MathML/LaTeX
  rendering behavior.
- Caution: its README still states a stale OneDrive-only limitation while its
  unreleased changelog consumes desktop-capable parser 2.0 work.

### Independent Cross-Checks

Use these as corroboration and test oracles, not normative definitions:

- [Joplin](https://github.com/laurent22/joplin), inspected HEAD
  `51c6d4f809dbe79d18ace9934e05e5f9e416b305`, contains downstream OneNote
  converter work.
- [LibMsON](https://github.com/blu-base/libmson), inspected HEAD
  `37bc22d6c98f17eac451c4330aac494e60990a6c`.
- [Dropbox OneNote parser](https://github.com/dropbox/onenote-parser),
  inspected HEAD `f14e67ab0b69d1921a9c976830ef6f11e97f295f`.
- [Apache Tika](https://github.com/apache/tika), inspected HEAD
  `6f966a22d11a99657eaacdd2f9023e7874c612ab`.

## Platform Sources

- [GTK4 drawing model](https://docs.gtk.org/gtk4/drawing-model.html)
- [GTK4 accessibility](https://docs.gtk.org/gtk4/section-accessibility.html)
- [gtk-rs GTK4 API](https://gtk-rs.org/gtk4-rs/stable/latest/docs/gtk4/)
- [SQLite FTS5](https://sqlite.org/fts5.html)
- [Flatpak basic concepts and portals](https://docs.flatpak.org/en/latest/basic-concepts.html)
- [Qt LGPL obligations](https://www.qt.io/development/open-source-lgpl-obligations)
- [Tauri 2 architecture](https://v2.tauri.app/concept/architecture/)

## Product Feature Cross-Checks

These Microsoft Support pages describe user-visible features, not binary
encoding. They are used to ensure the feature inventory includes content that
may be flattened or represented by undocumented extensions:

- [Insert or attach files and printouts](https://support.microsoft.com/en-us/onenote/onenote-help-and-learning/insert-or-attach-files-to-notes)
- [Embed content in OneNote](https://support.microsoft.com/en-US/OneNote/embed-content-in-onenote)
- [Drawing tools](https://support.microsoft.com/en-US/OneNote/onenote-help-and-learning/learn-more-about-drawing-tools)
- [Image and printout OCR](https://support.microsoft.com/en-us/office/copy-text-from-pictures-and-file-printouts-using-ocr-in-onenote-93a70a2f-ebcd-42dc-9f0b-19b09fd775b4)
- [Advanced options, including Linked Notes, ink, tags, printouts, and passwords](https://support.microsoft.com/en-us/office/onenote-options-advanced-928d1b3f-f580-479b-aa0b-47ac512bd827)
- [Data protection, backup, recycle bin, and export matrix](https://support.microsoft.com/en-us/office/onenote-platform-data-protection-recovery-39b8cdbe-fa57-49de-a4ac-a38aac2af5c7)

## Refresh Procedure

1. Check each Microsoft landing page for a newer published version.
2. Download it under a new versioned filename; never overwrite historical
   evidence under an old name.
3. Verify title, version, release date, and page count from the PDF.
4. Update `SHA256SUMS`, this manifest, the format profile if semantics changed,
   and the ADR if platform evidence changed.
5. Inspect upstream parser changelog/releases and update commit pins.
6. Run documentation validation and the full fixture matrix.
