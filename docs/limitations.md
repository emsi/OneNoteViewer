# Known Limitations

This document records stable user-facing boundaries. Current bugs, compatibility
gaps, and planned improvements are tracked in
[GitHub issues](https://github.com/emsi/OneNoteViewer/issues), which is the source
of truth for their status.

## 1.0 Blockers

- **Hand-drawn ink:** stored handwriting-recognition text can be searchable, but
  ink strokes are not rendered yet
  ([issue #6](https://github.com/emsi/OneNoteViewer/issues/6)).
- **Freeform text selection:** page titles, dates, and notebook paths can be
  copied, but text on the freeform page canvas cannot yet be selected or copied
  ([issue #8](https://github.com/emsi/OneNoteViewer/issues/8)).

## Product Boundaries

- OneNote Viewer opens local notebook data. It does not synchronize with
  OneDrive, edit notebooks, or reproduce live Microsoft 365 collaboration and
  cloud services.
- Password-protected sections and unconverted OneNote 2003/2007 files are not
  supported.
- Search uses text and metadata stored by OneNote. It does not inspect the body
  of attached documents or perform OCR on images that have no stored text.
- Stored attachment icons and preview images can be displayed, and original
  files can be saved or opened explicitly. Office documents, scripts, macros,
  and other embedded content are never executed inside the viewer.
- Proprietary Microsoft fonts are not bundled. Missing fonts are substituted by
  Linux and can change line wrapping and spacing.

## Compatibility

The application is used with notebooks created across OneNote 2010 through
modern Microsoft 365 and OneNote for the web, but no finite collection can cover
every producer, object type, or damaged file. Unsupported content should remain
visible as a placeholder or diagnostic where the available format information
allows it.

Please report reproducible compatibility problems through the
[issue tracker](https://github.com/emsi/OneNoteViewer/issues/new), including
the OneNote version or source when known and the application's copyable error
message. Do not attach private notebook data to a public issue.
