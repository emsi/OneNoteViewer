# OneNote Input and Parsing Profile

## 1. Purpose and Normative Basis

This document specifies how OneNote Viewer discovers, parses, normalizes, and
reports local OneNote content. It is complete at the application-integration
level. The archived Microsoft specifications are normative for binary field
layout:

1. `[MS-CAB]` v20110304 defines the Cabinet container used by observed
   `.onepkg` exports. It does not define the paths or OneNote payload.
2. `[MS-ONESTORE]` v20250520 defines desktop revision-store structure for
   `.one` and `.onetoc2`, and the FSSHTTP persistence form.
3. `[MS-ONE]` v20221115 defines OneNote property sets, properties, and the
   semantic object graph.
4. `[MS-FSSHTTPB]` v20240820 defines the alternate packaging used by
   OneDrive-origin files.
5. `[MS-DOC]` v20260217 defines the character-position (`CP`) rules referenced
   by `[MS-ONE]`, and `[MS-LCID]` v20240423 defines language identifiers.
6. `[MS-OSHARED]` v20251113 and `[MS-DTYP]` v20241119 define referenced common
   Office and Windows types.
7. `[MS-OFFCRYPTO]` v20260217 defines Office encryption structures relevant to
   password-protected sections.
8. The Ink Serialized Format specification supplies a related integer encoding
   used by OneNote ink, but does not by itself define OneNote's ink object
   graph.

The exact files and checksums are in
[the reference manifest](../references/README.md). If this profile conflicts
with a normative `MUST` in the pinned Microsoft documents, the Microsoft
document wins and this profile must be corrected.

Microsoft does not specify exact visual layout behavior, OneNote's ink object
graph, or its OfficeMath serialization completely. The unofficial sources for
those gaps are clearly marked in the reference manifest and must be validated
against fixtures.

## 2. Supported Input Set

### 2.1 Required

- A notebook directory containing `Open Notebook.onetoc2` or another
  `.onetoc2` plus referenced `.one` section files.
- A manifest-free OneNote backup directory containing recursively grouped
  `.one` section snapshots.
- A selected `.onetoc2` notebook table-of-contents file.
- A standalone `.one` section.
- A `.onepkg` export, through the managed on-disk extraction in ADR 0002.
- Multiple independent inputs open in one application workspace.
- Desktop revision-store files produced by supported OneNote Desktop versions.
- FSSHTTP-packaged files that already exist locally. No network access is
  implied.

`[MS-FSSHTTP]`, the SOAP synchronization protocol referenced by
`[MS-ONESTORE]`, is intentionally not part of this profile. The viewer consumes
only a persistence stream already present on disk; it does not synchronize or
request one. `[MS-FSSHTTPB]` is therefore required, while `[MS-FSSHTTP]` is
not.

### 2.2 Explicitly Outside Initial Compatibility

- OneNote 2003/2007 legacy formats that do not conform to the current
  `[MS-ONESTORE]` profile.
- `.onecache`, `.onebin`, `.onetmp`, or partially synchronized cache fragments.
- Cloud notebook placeholders, Graph API resources, OneDrive URLs, or sync
  protocols.
- Password-protected/encrypted sections.
- Repairing or writing any OneNote file.

Unsupported inputs produce a specific diagnostic and never fall through to an
unrelated parser based only on the extension.

## 3. Notebook Filesystem Model

A local notebook is a directory tree:

```text
Notebook/
  Open Notebook.onetoc2
  Section A.one
  Section B.one
  Section Group/
    Section C.one
```

The `.onetoc2` establishes notebook metadata, section order, colors, display
names, and child filenames. Section groups can be represented by nested
directories and nested TOC entries. The filesystem is only a container; order
must come from the TOC when it is valid. Orphan `.one` files may be offered in
an "unlisted sections" group with a warning, never silently discarded.

A selected directory without a usable root `.onetoc2` follows a distinct
backup-folder profile. Its relative directories become synthetic section
groups, recognized dated filenames are grouped as snapshots of a logical
section, and selection/reconstructed ordering are reported as provenance
rather than attributed to the binary format. The evidence profiles, limits,
and implementation gates are defined in the
[backup-folder loader plan](../plans/backup-folder-loader.md).

Resolve referenced child paths relative to the TOC that owns them. Before any
open:

1. Parse as a Windows-compatible relative path independent of the host OS.
2. Reject absolute paths, drive prefixes, UNC/device prefixes, and `..`.
3. Normalize separators without changing case.
4. Resolve beneath the canonical notebook root.
5. Reject a symlink resolution that escapes that root.
6. Preserve the original name for display, but use a separate sanitized name
   for extraction.

The desktop header's `guidAncestor` may connect a section to a sibling or
parent TOC as described in `[MS-ONESTORE]` 2.3.1. It is corroborating metadata,
not permission to traverse outside the selected root.

### 3.1 ONEPKG Container

Observed `.onepkg` exports are Microsoft Cabinet archives, identified by the
four-byte `MSCF` signature. There is no known separate normative
`[MS-ONEPKG]` specification. `[MS-CAB]` defines the container while the
payload is validated as the same `.onetoc2` and `.one` formats used in an
ordinary notebook tree.

Package extraction is a source-acquisition step, not OneNote parsing and not
content conversion. The application uses an external 7-Zip process to write a
durable native notebook tree under the policy in
[ADR 0002](../decisions/0002-onepkg-extraction.md). It must not call
`onenote_parser::Parser::parse_package`, load the archive into a byte vector,
or collect expanded entries in memory.

Archive paths preserve nested directories. A shallowest valid `.onetoc2` is a
notebook root candidate; additional nested `.onetoc2` files are valid and may
represent section groups. The post-extraction discovery and containment rules
are identical to a user-selected notebook directory.

## 4. Format Identification

Extensions guide file selection but do not establish format.

For desktop revision stores, the file begins with the 16-byte
`Header.guidFileType`:

| Type | GUID | Little-endian on-disk bytes |
|---|---|---|
| `.one` | `7B5C52E4-D88C-4DA7-AEB1-5378D02996D3` | `E4 52 5C 7B 8C D8 A7 4D AE B1 53 78 D0 29 96 D3` |
| `.onetoc2` | `43FF2FA1-EFD9-4C76-9EE2-10EA5722765F` | `A1 2F FF 43 D9 EF 76 4C 9E E2 10 EA 57 22 76 5F` |

`Header.guidFileFormat` must be
`109ADD3F-911B-49F5-A5D0-1791EDC8AED8`. Validate the complete header as
specified by `[MS-ONESTORE]` 2.3.1, including expected file length and
reachable chunk bounds.

The adapter delegates header sniffing between desktop and FSSHTTP inputs to
the parser and records the detected persistence kind. A `.onepkg` is accepted
for extraction only when its bytes identify a valid CAB container; its
published result is accepted as a notebook only after the normal TOC and
section validations pass.

## 5. Binary Parsing Rules

### 5.1 General

- All desktop structures are byte-aligned.
- Integers are little-endian unless the normative specification says
  otherwise.
- Checked arithmetic is mandatory for every offset, length, count, and unit
  conversion.
- A reference is valid only when its complete range is within the file and
  permitted by the containing structure.
- Cycles, recursion depth, object counts, and cumulative payload bytes are
  bounded.
- Data not reachable from the validated header/root structures is ignored.
- Source data is immutable. "Rewrite unchanged" requirements in the Microsoft
  specifications apply to editors; this viewer retains unknown values only in
  diagnostics/source locators because it never writes.

### 5.2 Desktop Revision Store Pipeline

An implementation from bytes follows this order:

1. Parse the fixed header at offset zero.
2. Validate the transaction log and select the committed reachable state. Do
   not interpret free or abandoned chunks as live data.
3. Traverse the root file-node list from
   `Header.fcrFileNodeListRoot`, following fragments and validating list
   headers, sequence numbers, references, and footers.
4. Build the global identification table mapping compact IDs to
   `ExtendedGUID` values.
5. Discover object-space manifest lists. Exactly one root object space is
   expected.
6. Resolve revision manifests, their dependency revisions, contexts, and
   revision roles. Revisions are immutable; the active view normally uses role
   `0x00000001`.
7. Resolve object groups and declarations into object identity, JCID, object
   state, and reference-count metadata.
8. Resolve file-data-store references lazily. Do not materialize attachment or
   image payloads while building the object graph.
9. Expose the resolved object spaces to the `[MS-ONE]` semantic projector.

Malformed optional structures become warnings only when the remaining object
graph is unambiguous. Invalid roots, unbounded references, conflicting object
identities, or ambiguous active revisions are fatal for that section.

### 5.3 FSSHTTP Persistence Pipeline

For a locally stored FSSHTTP input:

1. Parse stream object headers and compact integers using `[MS-FSSHTTPB]`.
2. Read the OneStore file header and mapping tables described by
   `[MS-ONESTORE]` 2.7 and 2.8.
3. Reconstruct object groups, object spaces, revisions, and revision roles.
4. Present the same resolved `ObjectSpace` interface used by the desktop path.

No SOAP request, authentication, synchronization, or cloud endpoint is part
of this process.

### 5.4 Property Sets

An object property set consists of a JCID plus a set of properties. JCID
determines the semantic object class; `PropertyID.type` determines whether a
value is inline, length-prefixed data, an object reference, an object-space
reference, or an array. The implementation must use the structural decoder in
`[MS-ONESTORE]` 2.6 before applying `[MS-ONE]` meanings.

Properties:

- may appear in any order;
- may not appear more than once in one property set;
- use the absence/default behavior defined by the owning `[MS-ONE]` property
  set;
- must be bounds-checked before decoding arrays;
- may share a PropertyID across semantic contexts, so the owning JCID matters.

The special length rules for `RgOutlineIndentDistance`,
`TableColumnsLocked`, and `TableColumnWidths` in `[MS-ONE]` 2.1.12 must be
implemented or inherited from the parser.

## 6. Semantic Projection

### 6.1 Notebook and Section Roots

For `.onetoc2`, the root/default content object is
`jcidPersistablePropertyContainerForTOC`. Project its ordered child entries,
folder filenames, display names, colors, and file identities into notebook
navigation. Preserve nested section-group entries as hierarchy; filesystem
paths and slash-joined display labels are not substitutes for section groups.

For `.one`, the root object space is `SectionObjectSpace`:

- default content root: `jcidSectionNode`;
- metadata root: `jcidSectionMetaData`.

The section node contains ordered page-series references. A page series begins
with a top-level page and subsequent entries are its subpages. Preserve the
stored order and explicit page level; do not infer hierarchy from title text.

### 6.2 Page Object Spaces

Each page reference leads to a `PageObjectSpace`:

- default content root: `jcidPageManifestNode`;
- metadata root: `jcidPageMetaData`;
- optional version metadata root: `jcidRevisionMetaData`.

Project:

- stable page/link identity;
- title, displayed page number, creation and modification times;
- page level, size/orientation, margins, read-only/deleted/conflict state;
- ordered direct page content;
- optional version/conflict metadata;
- author information where available.

Keep title-area objects semantically distinct from ordinary body objects even
when both are represented as positioned outlines. This permits a renderer host
to reproduce the complete native page or present page title/time metadata once
in native application chrome without flattening or guessing from coordinates.

Do not merge conflict pages or historical versions into the current page.
Expose them as separate optional views when implemented.

### 6.3 Standard Property-Set Classes

The complete standard `[MS-ONE]` class set must be recognized even when the UI
does not yet render it:

| Group | Standard classes |
|---|---|
| TOC and authors | `jcidPersistablePropertyContainerForTOC`, `jcidPersistablePropertyContainerForTOCSection`, `jcidReadOnlyPersistablePropertyContainerForAuthor` |
| Structure | `jcidSectionNode`, `jcidPageSeriesNode`, `jcidPageNode`, `jcidPageManifestNode`, `jcidTitleNode` |
| Canvas and content | `jcidOutlineNode`, `jcidOutlineElementNode`, `jcidOutlineGroup`, `jcidRichTextOENode`, `jcidImageNode`, `jcidNumberListNode`, `jcidTableNode`, `jcidTableRowNode`, `jcidTableCellNode`, `jcidEmbeddedFileNode` |
| Metadata | `jcidPageMetaData`, `jcidSectionMetaData`, `jcidConflictPageMetaData`, `jcidRevisionMetaData`, `jcidVersionHistoryMetaData` |
| History | `jcidVersionHistoryContent`, `jcidVersionProxy` |
| Shared definitions | `jcidNoteTagSharedDefinitionContainer`, `jcidParagraphStyleObject`, `jcidParagraphStyleObjectForText` |
| File payloads | `jcidEmbeddedFileContainer`, `jcidPictureContainer14` |

JCIDs discovered outside the standard table, including ink, math support,
handwriting recognition, and web embeds, are `ObservedExtension` classes.
Store their raw JCID and a warning when the selected parser cannot project
them.

### 6.4 Page Content Tree

`jcidPageManifestNode` has direct child nodes. Outlines contain outline
elements, which may contain rich text, a table, image, embedded file, ink, or
an unknown object. Tables recursively contain rows, cells, and cell outline
elements.

Order within each child collection is semantic. Geometry determines visual
overlap but must not replace stored reading order. Keep both:

- `source_order` for search, accessibility, and deterministic fallback;
- `z_order`/geometry for visual composition where evidence supports it.

### 6.5 Geometry and Units

`OffsetFromParentHoriz`, `OffsetFromParentVert`, widths, heights, margins,
table column widths, and indent distances are normally expressed in half-inch
increments. Normalize to logical pixels at 96 dpi:

```text
logical_px = value * 48
```

Preserve the source float alongside normalized geometry to avoid compounding
rounding error. Font size uses half-point units where specified. Colors follow
the exact `Color`/`COLORREF` byte ordering in `[MS-ONE]`, not CSS integer
ordering.

The page is a freeform, potentially unbounded canvas:

- an outline may appear anywhere and overlap another outline;
- content can be direct page content or nested in an outline;
- absent width/height means content-driven sizing, not zero;
- user-set dimensions and minimum/maximum layout hints are distinct;
- page size, orientation, margins, origin, collision priority, and background
  flags must remain available to the layout engine.

Ink coordinates use a distinct observed conversion based on the ISF-derived
path and OneNote bounding/scaling properties. Treat the current upstream
conversion as provisional and fixture-tested.

### 6.6 Rich Text

Decode `RichEditTextUnicode` as UTF-16 according to the spec and map
`TextRunIndex`/`TextRunFormatting` boundaries to Unicode character positions,
not UTF-8 byte indexes. Validate monotonic run boundaries and clamp only with a
warning when a deterministic prefix remains.

Preserve:

- font family and size;
- bold, italic, underline type, strikeout, superscript, and subscript;
- foreground and highlight color;
- language/charset and paragraph style identity;
- paragraph alignment, spacing before/after, and exact line spacing;
- LTR/RTL flags and reading order;
- hidden, hyperlink, protected-link, and embedded-object flags;
- author/original author/most-recent author metadata;
- math formatting and inline-object association.

Font substitution is a rendering decision. It must not rewrite the stored font
name in the domain model.

### 6.7 Lists and Tags

Lists retain format strings, bullet/number font, restart value, indentation,
spacing, style, and child level. Nested outline structure and list structure
are related but not interchangeable.

The domain model retains malformed or unknown list-format characters for
diagnostics. UI adapters replace interior NUL characters with U+FFFD before
passing strings to C-based text APIs; this display normalization must not
rewrite the retained source value.

Note tags can appear on rich text, table/cell content, images, attachments, and
other objects according to their property sets. Preserve:

- shared definition identity and label;
- action-item type/status/schema version;
- shape;
- checked/completed state;
- created, completed, and due timestamps;
- text and highlight colors;
- per-object state.

Unknown tag shapes receive a generic visible marker and searchable label.
Outlook task semantics are displayed as metadata only; no Outlook integration
or task execution occurs.

### 6.8 Images and Printouts

An image node may reference a picture file-data object and can carry:

- original/image filename and detected payload type;
- alternative text;
- display width/height and position;
- background status;
- upload/web-picture state or URL;
- timestamps and tags.

Sniff payload bytes before decode. Decode under pixel and memory limits.
OneNote printouts are commonly stored as one or more raster image objects even
when their filenames resemble the source PDF. Render the actual payload and
retain the source filename as metadata.

Remote/web picture URLs are never fetched automatically. If no local payload
exists, render a placeholder with the source URL.

### 6.9 Embedded Files and Media

File data is stored in a companion `onefiles` location or a
`FileDataStoreObject`, depending on format/version. Stream it lazily. Preserve
the embedded filename, source path as inert metadata, detected type, byte size,
position, dimensions, tags, and audio-recording GUID associations.

An attached Word, Excel, PowerPoint, PDF, archive, executable, audio, or video
file is an opaque payload to the core viewer. The viewer may extract the exact
bytes and ask GIO to open them after an explicit action. It does not provide
Office editing, OLE activation, macros, or in-process preview in the first
release.

### 6.10 Hyperlinks

Preserve visible text and the exact target string separately. Supported target
classes:

- `http` and `https`;
- `mailto`;
- file paths/URIs;
- OneNote internal links, including notebook/section/page identities and
  optional object anchors;
- unknown/custom schemes.

Internal links resolve against every open notebook by stable IDs first, then a
normalized local locator if IDs are absent. Ambiguous targets show a chooser.
Broken links remain visible. External or custom schemes require an explicit
user gesture and a confirmation appropriate to the scheme.

### 6.11 Ink and Handwriting Recognition

Microsoft's `[MS-ONE]` does not define the OneNote ink object graph. Current
upstream evidence uses:

```text
InkContainer
  -> InkDataNode
       -> InkStrokeNode*
            -> StrokePropertiesNode
```

Stroke paths use signed variable-length differential integers related to ISF,
organized by dimension, plus per-dimension scaling. Preserve stroke order,
points, pen tip, width/height, transparency, color, bounding box, and nested
ink containers.

When OneNote's stored handwriting recognition is present, preserve the
recognizer's line/word reading order, best text, alternatives, language ID,
stable word ID, and word-to-stroke association. This is stored recognition
output; the viewer does not run OCR.

### 6.12 OfficeMath

Math is embedded in normal rich-text runs. Observed data associates text
positions with inline math objects and uses private in-band start, separator,
and end markers to encode an operator tree. Supported operator families
include accents, boxes, brackets, equation arrays, fractions, functions,
limits, matrices, n-ary operators, radicals, stacks, sub/superscripts, and
under/overbars.

The domain model stores a typed math AST plus original text/run information.
Rendering may use Pango fallback text initially and MathML/LaTeX conversion
after validation. Unknown operators retain their arguments and show a warning;
they must not drop adjacent prose.

### 6.13 Web Embeds and Other Extensions

Modern OneNote files can contain observed, non-normative property sets such as
an iframe source URL and embed type. The offline viewer never instantiates an
iframe or fetches the URL. It renders a labeled, clickable placeholder.

Stickers, Loop/live components, online videos, add-in objects, meeting
services, transcription, and cloud-only AI output may appear as images,
attachments, URLs, unknown property sets, or not be persisted in local files
at all. Preserve known labels/URLs and always surface unknown-object warnings.
Screen clippings and scans normally project as images. Linked Notes, Outlook
meeting details, and email-to-OneNote content can combine ordinary rich text,
images, thumbnails, and links with undocumented metadata; the ordinary content
must remain usable even when that metadata is unknown.

Converted ink-to-text is ordinary rich text. Ink-to-shape output may remain ink
or use an extension object. Pressure/effect-pen dimensions beyond the
documented/observed stroke model are best-effort and must generate a warning
rather than flattening all strokes to an indistinguishable default.

## 7. History, Conflicts, and Deleted Content

The revision store can retain immutable revisions; `[MS-ONE]` defines version
history pages, proxies, metadata, conflict pages/objects, and deleted graph
space markers.

Initial behavior:

- render only the active revision in normal navigation;
- report the presence/count of historical, conflict, and deleted content;
- do not index deleted content;
- do not merge conflict objects into the active canvas;
- add explicit history/conflict views only after fixtures establish semantics.

Selecting the newest timestamp is not a substitute for resolving revision role
and dependencies correctly.

## 8. Encryption

An `ObjectDataEncryptionKeyV2FNDX` marks an encrypted object space.
`[MS-ONESTORE]` says its encryption-data bytes are opaque at that layer;
`[MS-OFFCRYPTO]` supplies the broader cryptographic structures.

The initial viewer detects encryption and reports "password-protected section
is unsupported." It must not attempt partial plaintext projection, log key
material, or ask for a password until a separately reviewed design supports
secure key derivation, memory handling, and test vectors.

Sensitivity-label or rights-management protection is a distinct product and
authorization concern. The viewer does not bypass it. Protected content that
cannot be represented from the local file is rejected with a separate
diagnostic from password encryption.

## 9. Error and Warning Contract

Every parsed input returns either:

- `Loaded(model, report)`;
- `PartiallyLoaded(model, report)` when deterministic content remains; or
- `Rejected(fatal_diagnostic)`.

A diagnostic contains source path, section/page/object locator when known,
layer (`discovery`, `package`, `revision-store`, `property-set`, `semantic`,
`render`, `index`), severity, stable code, bounded human detail, and upstream
error chain without raw notebook text.

Required non-fatal examples:

- unknown property/JCID;
- optional image payload missing;
- unknown tag shape;
- a page skipped while other pages remain valid;
- remote image not fetched;
- unsupported history or extension object.

Required fatal examples:

- invalid root/header for the selected file;
- path escape;
- truncated required structure;
- arithmetic overflow or limit exceeded;
- ambiguous/cyclic mandatory object graph;
- archive policy violation.

## 10. Parser Dependency Profile

Use `onenote_parser` only through `onenote-core`; parser types and revision
internals must not spread into the application. The public fork is pinned to an
immutable revision because the private desktop and backup corpora need
unreleased support and five narrow compatibility patches. Those patches and
their upstream pull requests are documented in
`third_party/onenote.rs/PATCHES.md`; the local source tree is retained for
audit history but is not the Cargo dependency.

Before beta, the chosen upstream release/commit must demonstrate:

- desktop and FSSHTTP header sniffing;
- `.one` and `.onetoc2` parsing;
- path-traversal protections;
- bounds-checked reads without panics on the malformed corpus;
- newest active revision resolution;
- lazy attachment/image access;
- per-page/section warnings;
- ink and math projection;

The current implementation pin is
`f9cdc59f984bc1f7f096b54100cefaaebc892573` from 2026-07-23. It includes
unreleased desktop work plus the documented local patches. It also contains an
in-memory package reader that this application explicitly does not use.
Production must move to a reviewed tagged release or explicitly accept and
maintain the documented fork.

## 11. Implementation Acceptance

Parsing support for a feature is complete only when:

1. a legal fixture contains it;
2. the binary input class is identified;
3. the domain model preserves its semantic data and geometry;
4. malformed variants fail without panic or unbounded allocation;
5. renderer and index behavior match the feature matrix;
6. a OneNote reference screenshot/export or independent parser result provides
   an oracle;
7. known differences are recorded in `docs/limitations.md`.

Passing a single notebook is not evidence of general compatibility.
