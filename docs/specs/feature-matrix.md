# Persisted Feature Inventory

## How to Read This Matrix

This is the complete target inventory for content that can plausibly occur in
local OneNote notebook files. "Required" means the application must preserve or
visibly report it, not necessarily reproduce every editing behavior.

Evidence classes:

- **Normative:** explicitly defined by pinned Microsoft format documents.
- **Observed:** implemented or described by independent parser work but absent
  from `[MS-ONE]`.
- **Product:** user-facing OneNote behavior whose exact local persistence is
  variable or undocumented.

Delivery levels:

- **MVP:** needed for a useful, honest viewer.
- **Later:** model or placeholder in MVP; richer display follows.
- **Unsupported:** detected with an explicit limitation.

## Notebook and Navigation

| Feature | Evidence | MVP behavior | Level |
|---|---|---|---|
| Direct native `.one`/`.onetoc2` representation | Product requirement | Parse semantic objects and geometry; no HTML/Markdown/PDF conversion layer | MVP |
| Multiple open notebooks | Product requirement | One workspace tree and global search | MVP |
| `.onepkg` acquisition | Observed CAB package | One-time bounded extraction to durable native notebook tree | MVP |
| Notebook display name and color | Normative | Display and use as navigation accent | MVP |
| Ordered sections | Normative TOC | Preserve order | MVP |
| Nested section groups | Normative plus filesystem | Recursive navigation | MVP |
| Standalone section | Normative | Synthetic one-section notebook | MVP |
| Page order | Normative | Preserve series order | MVP |
| Subpages/page levels | Normative | Hierarchical page list | MVP |
| Cached/untitled page title | Normative | Correct title fallback | MVP |
| Displayed page number | Normative | Preserve as optional metadata | Later |
| Creation/modified timestamps | Normative | Display and index/filter | MVP |
| Authors and most recent author | Normative | Preserve; optional inspector | Later |
| Section/page identity GUIDs | Normative | Stable navigation and link targets | MVP |
| Read-only/deletable flags | Normative | Never enable edits; preserve state | MVP |
| Multiple selected roots with duplicate IDs | Application | Namespace by source identity; warn | MVP |

## Page and Freeform Canvas

| Feature | Evidence | MVP behavior | Level |
|---|---|---|---|
| Unbounded freeform canvas | Normative | Pan, scroll, zoom | MVP |
| Arbitrary outline position | Normative | Position in normalized page units | MVP |
| Overlapping outlines | Normative | Deterministic scene/source order | MVP |
| Direct page content outside outline | Normative | Render at page geometry | MVP |
| User-set width/height | Normative | Respect fixed bounds | MVP |
| Content-driven/minimum/max dimensions | Normative | Layout with preserved hints | MVP |
| Page size and portrait orientation | Normative | Set canvas/print boundary when present | MVP |
| Page margins and origin | Normative | Preserve and apply | MVP |
| Background image/object | Normative | Draw behind normal content | MVP |
| Alignment and tight layout | Normative | Best-effort layout, fixture tested | MVP |
| Collision priority/resolution hints | Normative | Preserve; approximate if necessary | Later |
| Boiler/title date/title time fields | Normative | Render title components | MVP |
| Custom page templates | Product | Render persisted result, not template rules | MVP |
| Page color/rule lines/grid lines | Product, persistence unclear | Detect if observed; otherwise limitation | Later |
| Section/page templates and stationery | Product | Render persisted result; no template engine | MVP |
| Zoom state from OneNote | Product | Viewer-owned state | Unsupported |

## Text and Paragraphs

| Feature | Evidence | MVP behavior | Level |
|---|---|---|---|
| Unicode rich text | Normative | UTF-16 decode and Pango shaping | MVP |
| Multiple text/style runs | Normative | Preserve boundaries | MVP |
| Font family and size | Normative | Render with fallback | MVP |
| Bold/italic/strike | Normative | Render | MVP |
| Underline types | Normative | Render closest supported style | MVP |
| Foreground/highlight colors | Normative | Render | MVP |
| Superscript/subscript | Normative | Render | MVP |
| Paragraph styles and next-style metadata | Normative | Preserve and render effective style | MVP |
| Alignment and exact line spacing | Normative | Render | MVP |
| Space before/after | Normative | Render | MVP |
| Language and charset | Normative | Preserve for shaping/search | MVP |
| RTL paragraph, outline, and reading order | Normative | Pango bidi plus navigation tests | MVP |
| Hidden text runs | Normative | Exclude from normal render; configurable index policy | MVP |
| Protected hyperlink formatting | Normative | Preserve; no edit implications | MVP |
| Embedded-object marker in text | Normative | Associate object or show placeholder | MVP |
| Unsupported/missing fonts | Platform | Use fontconfig fallback; disclose in diagnostics | MVP |

## Lists and Tables

| Feature | Evidence | MVP behavior | Level |
|---|---|---|---|
| Bulleted lists | Normative | Render symbol/font and indentation | MVP |
| Numbered/custom-format lists | Normative | Format, restart, nesting | MVP |
| List spacing and accessibility index | Normative | Render spacing; preserve index | MVP |
| Nested outline groups | Normative | Preserve levels independently of lists | MVP |
| Tables, rows, cells | Normative | Render rectangular structure | MVP |
| Column count/widths/locks | Normative | Apply widths; locks are metadata | MVP |
| Borders visible | Normative | Render | MVP |
| Cell shading | Normative | Render | MVP |
| Rich nested content in cells | Normative | Recursive rendering | MVP |
| Merged cells | Not defined by `[MS-ONE]` set | Detect observed representation or report | Later |
| Excel-style formulas inside OneNote table | Product not local table model | Display stored text only | Unsupported |

## Tags and Tasks

| Feature | Evidence | MVP behavior | Level |
|---|---|---|---|
| Built-in tag shapes | Normative | Icon or generic marker | MVP |
| Custom tag labels | Normative | Display and search | MVP |
| Tag text/highlight colors | Normative | Render | MVP |
| Tag per-object states | Normative | Preserve | MVP |
| Checkbox/action status | Normative | Read-only checked/unchecked display | MVP |
| Created/completed/due timestamps | Normative | Display and index filters | MVP |
| Outlook task/action type | Normative metadata | Display only | MVP |
| Outlook synchronization/reminders | External service | No synchronization | Unsupported |
| Tag summary pages | Product-generated page | Render persisted page normally | MVP |

## Images, Printouts, and Drawings

| Feature | Evidence | MVP behavior | Level |
|---|---|---|---|
| Embedded raster images | Normative | Sniff/decode and render | MVP |
| Image filename/type/dimensions | Normative | Preserve | MVP |
| Image alternative text | Normative | Accessibility and search | MVP |
| Image position/background flag | Normative | Apply to canvas | MVP |
| Web/remote picture URL | Normative | Offline placeholder; no fetch | MVP |
| Screen clipping/camera/scanned image | Product stored as image | Render as ordinary local image | MVP |
| PDF/document printout pages | Product stored as images | Render actual image payloads | MVP |
| Printout source attachment | Normative embedded file | Offer separate extraction | MVP |
| OCR text associated with images | Product, format uncertain | Index if exposed; never invent | Later |
| Ink strokes and nested ink | Observed | Render path, color, width, opacity | MVP |
| Ink highlights | Observed style | Best-effort blend | MVP |
| Pen pressure and smoothing | Product/observed dimensions | Preserve exposed dimensions; best effort | Later |
| Effect pens (rainbow/galaxy/lava) | Product, encoding undocumented | Static closest-style fallback and warning | Later |
| Stored handwriting recognition | Observed | Search/accessibility text and associations | MVP |
| Ink selection grouping/z-order | Observed/incomplete | Preserve known nesting/order | Later |
| Shape recognition and converted shapes | Product | Render stored ink/image representation | MVP |
| Ink-to-text converted content | Product | Render final rich text; source gesture irrelevant | MVP |
| Ink replay/timing | Product, not specified | Static rendering only | Unsupported |

## Files, Audio, Video, and Embeds

| Feature | Evidence | MVP behavior | Level |
|---|---|---|---|
| Arbitrary attached file | Normative | Metadata, explicit safe extraction | MVP |
| Original filename/source path | Normative | Display inert metadata | MVP |
| Attachment position/tags | Normative | Render attachment object | MVP |
| Word/Excel/PowerPoint files | Normative opaque file | Extract/open externally | MVP |
| Embedded Excel live sheet/OLE surface | Product/opaque | No in-app editing or activation | Unsupported |
| PDF attachment | Normative opaque file | Extract/open externally | MVP |
| Executable/script/archive | Normative opaque file | Warn; never execute automatically | MVP |
| Audio/video recording payload | Normative file object and GUIDs | Extract/open externally | MVP |
| Audio recording duration/link association | Normative | Display and link related notes | Later |
| Synchronized audio note highlighting | Product timing behavior | Static association only | Unsupported |
| Online video/web iframe | Observed extension | Offline URL placeholder | MVP |
| Forms, Stream, Power BI, Spotify, and other service cards | Product web embed | Offline label/URL placeholder | MVP |
| Live webpage preview | Network/runtime | Never embed browser content | Unsupported |
| Loop/live component | Product/cloud | Placeholder if detectable | Unsupported |
| Linked Notes document snippet/thumbnail | Product, persistence undocumented | Render stored image/text/link; warn on unknown object | Later |
| Outlook meeting details/email sent to OneNote | Product | Render persisted rich text, images, and links | MVP |

## Links and Math

| Feature | Evidence | MVP behavior | Level |
|---|---|---|---|
| Web URL | Normative | Styled link, explicit external open | MVP |
| `mailto` link | Normative | Explicit external open | MVP |
| File link | Normative | Show target; confirmation before open | MVP |
| Internal page/section link | Normative target string/product | Resolve across open notebooks | MVP |
| Object/paragraph anchor | Product | Resolve when stable object ID is available | Later |
| Broken/ambiguous link | Application | Visible diagnostic/chooser | MVP |
| Inline OfficeMath | Observed | Preserve AST; readable rendering | MVP |
| Fractions, scripts, roots, brackets | Observed | Render/test | MVP |
| Matrices, n-ary ops, limits, accents | Observed | Render/test | MVP |
| Rare OfficeMath operators | Observed/incomplete | Fallback text plus warning | Later |
| Math calculation/solver | Product behavior | View persisted expression/result only | Unsupported |

## History, Collaboration, and Protection

| Feature | Evidence | MVP behavior | Level |
|---|---|---|---|
| Active revision resolution | Normative | Required parse behavior | MVP |
| Page version-history presence | Normative | Show indicator/count | MVP |
| Browse old page versions | Normative objects | Separate read-only view | Later |
| Conflict pages/objects | Normative | Show warning and preserve separate | Later |
| Deleted graph-space content | Normative | Exclude; report presence | MVP |
| Author/revision metadata | Normative | Preserve | MVP |
| Password-protected object space | Normative marker + OFFCRYPTO | Detect and reject clearly | Unsupported |
| Sensitivity label/IRM-protected content | Product/security service | Detect where possible; never bypass protection | Unsupported |
| Real-time collaboration state | Cloud/runtime | None | Unsupported |
| OneDrive sync/offline hydration | Cloud/runtime | None | Unsupported |

## Searchable Fields

MVP indexing includes page title, visible rich text, tag labels/states, image
alternative text, stored handwriting-recognition text, attachment filenames,
link visible text/target, notebook/section names, and relevant timestamps.
Image OCR that is not exposed by the parser and body text inside attachments
are not silently indexed.

## Completeness Rule

A OneNote feature not listed here is a specification defect. Add it with an
evidence class, fixture, delivery level, and explicit rendering/index behavior
before implementation. Unknown binary objects still remain visible through
diagnostics; "not listed" never means "silently ignore."
