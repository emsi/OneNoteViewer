# Historical OneNote Parser Compatibility Patches

The active application dependency is upstream
[`msiemens/onenote.rs`](https://github.com/msiemens/onenote.rs), pinned in the
workspace manifest to release `v2.0.0` at revision
`fa4d7a044324af3bfe68727704a9789a08b36a3c`. That release contains the
compatibility changes formerly maintained in the project fork.

This directory retains an MPL-2.0 source snapshot for audit history and
fallback inspection; Cargo does not build it. Upstream test samples and
snapshots are intentionally omitted here. The application exposes the parser
only through `onenote-core`.

## Former Compatibility Patches

1. `OutlineElementNode` accepts a missing creation timestamp. The timestamp is
   not consumed by the high-level parser model, and OneNote Desktop sections
   in the project corpus legitimately omit it.
2. `PropertyValue::to_u8_lossless` accepts unsigned integer encodings of
   different widths only when the value fits exactly in `u8`.
3. Page-size and paragraph-alignment parsing use that lossless conversion.
   This accepts native sections emitted with a wider property encoding without
   truncating malformed values.
4. Desktop attachment resolution classifies the `file_data_ref` before
   requiring an embedded `FileDataStore`. Explicitly invalid and missing
   payloads retain their metadata and a public availability status, allowing
   consumers to render a broken-content indicator instead of silently treating
   them as empty files. Internal `<ifndf>` references still require and resolve
   through the store.
5. `OutlineGroup` accepts a missing `LastModifiedTime` while emitting a
   page-scoped parser warning. `[MS-ONE]` requires that value, but the
   high-level outline-group model does not consume it; preserving the group's
   children is safer than rejecting the complete section and does not invent a
   timestamp.
6. `.onetoc2` global ID tables resolve `GlobalIdTableEntry3FNDX` range-copy
   records against their dependency revision. Source and destination ranges
   are validated before copying so malformed indices still produce an
   actionable parser error.
7. Notebook TOC entries are deduplicated before sorting, with the latest
   reference to a section or section-group filename taking precedence. This
   prevents stale ordering snapshots retained by OneNote Desktop from
   duplicating sections or interleaving section groups in an obsolete order.
8. Desktop revision materialization follows all dependency revisions from the
   active root before collecting objects. This prevents active pages from
   losing inherited rich-text or child objects after collaborative edits.
9. `RichText` exposes structured hyperlinks as exact targets plus UTF-16 source
   ranges. Hidden `HYPERLINK` marker runs are associated with adjacent visible
   hyperlink runs without changing the existing raw text or style APIs.

The supplied private corpus has one section requiring patch 1 and two sections
that reach a wider numeric property requiring patches 2 and 3. All 32 native
sections parse after these changes. In the separate manifest-free private
backup corpus, 25 of 83 physical snapshots require patch 4 to get past an
`<invfdo>` declaration when no file-data store exists, and two snapshots of one
logical section require patch 5. All 83 snapshots parse independently after
the patches; the corpus remains private and is tested only when
`ONENOTE_BACKUP_TEST_CORPUS` is set.

## Upstream History

The compatibility changes were accepted upstream through these pull requests:

- [msiemens/onenote.rs#36](https://github.com/msiemens/onenote.rs/pull/36)
  preserves unavailable attachment payloads and fixes
  [issue #35](https://github.com/msiemens/onenote.rs/issues/35).
- [msiemens/onenote.rs#37](https://github.com/msiemens/onenote.rs/pull/37)
  recovers outlines with omitted timestamps and fixes
  [issue #33](https://github.com/msiemens/onenote.rs/issues/33).
- [msiemens/onenote.rs#38](https://github.com/msiemens/onenote.rs/pull/38)
  accepts losslessly convertible enum property widths and fixes
  [issue #34](https://github.com/msiemens/onenote.rs/issues/34).

The TOC ordering fix was merged through
[msiemens/onenote.rs#40](https://github.com/msiemens/onenote.rs/pull/40) and
fixes [issue #5](https://github.com/msiemens/onenote.rs/issues/5). Active
revision materialization was merged through
[msiemens/onenote.rs#41](https://github.com/msiemens/onenote.rs/pull/41).
Structured rich-text hyperlinks were merged through
[msiemens/onenote.rs#42](https://github.com/msiemens/onenote.rs/pull/42).

Upstream release `v2.0.0` contains the complete resulting source tree used by
OneNoteViewer, including the `GlobalIdTableEntry3FNDX` range resolution.
