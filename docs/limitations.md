# Potential Limitations and Risk Register

## Status Labels

- **Open:** no proven mitigation yet.
- **Planned:** mitigation and acceptance gate are defined.
- **Accepted:** intentional product boundary.
- **Resolved:** evidence and tests demonstrate the mitigation.

This file tracks format and product limitations even when the feature matrix
contains the user-facing behavior.

## Critical Feasibility Risks

### L-001: Incomplete representative notebook corpus

- **Status:** Open
- **Impact:** Compatibility and visual fidelity claims cannot yet be verified
  across producers and the full feature matrix.
- **Evidence:** One private user package is structurally extracted, all 32
  sections parse, every page builds a scene, and the root's 637 pages index.
  It is not redistributable and does not establish producer or feature breadth.
- **Mitigation:** Build the producer/feature matrix in
  [the corpus specification](specs/test-corpus.md) before beta.
- **Release gate:** All MVP rows have at least one legal positive fixture;
  desktop inputs include both OneNote 2016 and current Microsoft 365.

### L-002: Desktop parser support is not in a stable release

- **Status:** Open
- **Impact:** The current crates.io `onenote_parser` 1.1.1 targets FSSHTTP
  content. Desktop revision-store support exists at upstream commit
  `f9cdc59...` and is described for the next major release.
- **Current mitigation:** The public `emsi/onenote.rs` fork is pinned to
  signed revision `3cc4e98...` and isolated behind `onenote-core`.
  `third_party/onenote.rs/PATCHES.md` records the five narrow compatibility
  patches, their upstream issues, and draft pull requests.
- **Remaining mitigation:** Complete upstream review and move to a tagged
  upstream release when it contains the required fixes.
- **Package boundary:** Its unreleased `.onepkg` API is not used because it
  expands package contents in memory; ADR 0002 owns package extraction.
- **Release gate:** Tagged parser version passes the complete input and
  malformed corpus without panics.

### L-003: Exact OneNote layout behavior is undocumented

- **Status:** Open
- **Impact:** Overlaps, nested lists, collision resolution, automatic outline
  sizing, title offsets, and rare table/math layouts may differ visually.
- **Evidence:** `[MS-ONE]` specifies stored properties, not Microsoft's
  rendering algorithm.
- **Mitigation:** Keep layout as a separate scene-building module, preserve all
  hints, compare to OneNote screenshots/exports, and document tolerances.
- **Release gate:** MVP visual fixtures meet agreed geometry/text tolerances;
  deviations remain listed per feature.

### L-004: Ink and OfficeMath are unofficially specified

- **Status:** Open
- **Impact:** Rare stroke dimensions, pen styles, nested ink, and exotic math
  operators can render incorrectly.
- **Mitigation:** Use the pinned `onenote.rs` informal specifications and
  implementation as evidence, preserve unknown operators, and create hostile
  and visual fixtures.
- **Release gate:** Every operator/stroke class claimed in release notes has an
  oracle-backed case.

## Parser and Format Risks

### L-005: Unknown objects are too opaque in the current parser API

- **Status:** Open
- **Impact:** Public `PageContent::Unknown`/`Content::Unknown` variants do not
  expose raw JCID or safe metadata, so diagnostics cannot identify dropped
  extensions.
- **Mitigation:** Upstream an API that exposes object type, source identity,
  and safe raw-property inventory without exposing internal lifetimes.
- **Release gate:** Every unknown object produces a stable diagnostic with
  JCID and page locator.

### L-006: History, conflict, and deleted content are not full high-level models

- **Status:** Planned
- **Impact:** The active page can be shown, but old versions and conflicts
  cannot initially be browsed with confidence.
- **Mitigation:** Detect and count these structures in MVP; add high-level
  projection only with targeted fixtures.
- **MVP behavior:** Clear indicator; no silent merge; excluded from active
  search.

### L-007: Password-protected sections

- **Status:** Accepted for MVP
- **Impact:** Encrypted object spaces cannot be viewed.
- **Difficulty:** Correct Office password derivation/decryption, secure secret
  handling, and comprehensive test vectors are a substantial independent
  feature. `[MS-ONESTORE]` treats its encryption payload as opaque.
- **MVP behavior:** Detect and reject the section with a specific message.
- **Future gate:** Separate threat model and `[MS-OFFCRYPTO]` implementation
  review.

### L-008: Legacy OneNote 2003/2007 formats

- **Status:** Accepted
- **Impact:** Old files that were never converted cannot be opened.
- **Mitigation:** Ask users to convert/export with OneNote on Windows; never
  misidentify them as corrupt current files.

### L-009: `.onepkg` depends on an external extractor

- **Status:** Planned
- **Impact:** Package onboarding is unavailable when compatible `7zz`/`7z`
  is absent, and invoking a tool introduces version, output-parsing, and
  process-lifecycle dependencies.
- **Mitigation:** Use the narrow contract in ADR 0002, preflight and
  independently verify the on-disk result, test supported extractor versions,
  and provide installation guidance plus an "Open extracted notebook folder"
  fallback.
- **Boundary:** The upstream in-memory package API is prohibited. Normal
  viewing and indexing never depend on the extractor.
- **Current evidence:** Listing/path validation, entry count/type checks,
  private staging, cancellation, destination conflicts, and atomic publication
  are implemented. Tests cover unsafe paths, bounded listing capture,
  pre-launch cancellation, unchanged-source real extraction, and corpus
  counts. The viewer imports packages but does not yet expose progress or
  cancellation.
- **Release gate:** Missing-tool, corrupt/truncated, path, limit, cancellation,
  partial-output, and source-unchanged cases pass.

### L-010: Producer/version variation

- **Status:** Open
- **Impact:** Desktop local files, backups, modern exports, OneDrive downloads,
  and Mac backups can use different persistence paths and extensions.
- **Mitigation:** Identify by bytes, record producer coverage, retain warnings,
  and never equate one passing source with all `.one` files. Manifest-free
  backup aggregation uses explicit evidence-backed filename profiles,
  provenance, and conservative fallbacks as specified in the
  [backup-folder loader plan](plans/backup-folder-loader.md).

### L-011: Corrupt or partially copied notebooks

- **Status:** Planned
- **Impact:** Copying while OneNote writes may yield inconsistent transaction
  state or missing `onefiles` payloads.
- **Mitigation:** Respect transaction logs, isolate failure per section/page,
  identify missing payloads, and recommend recopy/export rather than repair.
- **Boundary:** Viewer does not modify or repair source files.

## Rendering Limitations

### L-012: Font substitution

- **Status:** Accepted
- **Impact:** Calibri, Aptos, and user fonts may be absent on Linux, changing
  line breaks and geometry.
- **Mitigation:** fontconfig/Pango fallback, optional font diagnostics, and
  tests with redistributable metrically compatible fonts.
- **Boundary:** Proprietary Microsoft fonts are not bundled.

### L-013: Office attachments and embedded Excel

- **Status:** Accepted for MVP
- **Impact:** A spreadsheet, Word document, PowerPoint, or OLE-like object
  cannot be edited or faithfully previewed in the viewer.
- **Mitigation:** Preserve and safely extract the original payload; show
  persisted printout/preview images if available; open externally on request.
- **Security boundary:** Never activate macros, OLE, scripts, or executables
  in-process. External applications assume responsibility after confirmation.

### L-014: PDF and document printouts

- **Status:** Planned
- **Impact:** OneNote may store printout pages as raster images with misleading
  `.pdf` filenames. Searchable OCR/source-page relationships may be absent.
- **Mitigation:** Sniff bytes, render actual images, keep original filenames,
  and index only text explicitly available in the notebook.

### L-015: Audio/video synchronization

- **Status:** Accepted for MVP
- **Impact:** Media payloads may be extracted/opened, but note highlighting
  synchronized to playback and recording controls are not reproduced.
- **Mitigation:** Preserve GUID/duration associations for a later media view.

### L-016: Web embeds and remote images

- **Status:** Accepted
- **Impact:** Online video, iframe, or remote images show an offline
  placeholder.
- **Reason:** The product is local/offline and hostile URLs must not execute in
  an embedded browser.
- **Mitigation:** Display sanitized label/URL and allow explicit external open.

### L-017: Cloud-only and live features

- **Status:** Accepted
- **Impact:** Loop components, collaboration cursors, sync state, Math
  Assistant solving, transcription services, reminders, and Copilot results
are not recreated unless their final output is ordinary persisted content.

Effect pens, pen-pressure nuance, ink smoothing, Linked Notes metadata, and
ink-to-shape extension objects can also exceed the documented static model.
The viewer renders their persisted basic ink/image/text/link form and reports
lost effects.

### L-018: Accessibility of a custom canvas

- **Status:** Open
- **Impact:** A purely drawn page would be invisible or unusable to screen
  readers and keyboard users.
- **Mitigation:** Scene objects expose accessible text/link/image roles,
  reading order, focus actions, and bounds; use standard GTK controls where
  possible.
- **Current evidence:** UI-neutral scene semantics exist and navigation lists
  use standard virtualized GTK controls. The custom canvas does not yet expose
  scene nodes as accessible GTK children.
- **Release gate:** Orca keyboard/screen-reader tests for text, links,
  attachments, navigation, zoom, and search results.

### L-019: Extreme page coordinates and sizes

- **Status:** Open
- **Impact:** A malformed or legitimately huge canvas can overflow transforms,
  allocate enormous surfaces, or degrade pan/zoom.
- **Mitigation:** finite-number validation, coordinate ceilings, viewport
  culling, tiled image decode, and visible warnings for clamped content.
- **Current evidence:** Scene tests require finite bounds, viewport culling is
  active, and numeric GTK conversions saturate. Coordinate ceilings, tiled
  image decode, and user-visible clamp warnings are not implemented.

## Search Limitations

### L-020: Multilingual tokenization

- **Status:** Open
- **Impact:** SQLite `unicode61` is robust baseline tokenization but does not
  segment/stem every language ideally.
- **Mitigation:** multilingual fixtures, preserved language IDs, versioned
  tokenizer choice, and later CJK/language-aware extensions if benchmarks
  justify them.

### L-021: Attachment and image body search

- **Status:** Accepted for MVP
- **Impact:** Text inside attached Office/PDF files and unstored image text is
  not indexed.
- **Reason:** Extraction adds parsers for multiple hostile formats and can
  produce misleading or privacy-sensitive results.
- **Mitigation:** Index filenames, alt text, stored handwriting recognition,
  and persisted printout text. Design sandboxed extractors separately.

### L-022: Stale derived index

- **Status:** Planned
- **Impact:** Files changed externally can make search disagree with the view.
- **Mitigation:** source fingerprint per section, transactional generations,
  refresh notification, and automatic rebuild on parser/schema mismatch.
- **Current evidence:** Sources are fingerprinted and index replacement is
  transactional. The viewer does not yet monitor, notify, or refresh changed
  source trees and does not automatically rebuild an incompatible schema.

## Security and Packaging Risks

### L-023: Hostile notebook content

- **Status:** Open
- **Impact:** Integer overflows, decompression bombs, path traversal, image
  bombs, malicious links, and attachment filenames are expected attack inputs.
- **Mitigation:** checked/bounded parsing, canonical root containment,
  payload streaming, content sniffing, no embedded web runtime, and fuzzing.
- **Current evidence:** Canonical source access, projection/resource limits,
  package containment checks, bounded image decode, inert URLs/attachments,
  and no web runtime are implemented. Parser fuzzing, aggregate memory limits,
  attachment extraction policy, and package expansion/disk ceilings remain.

### L-024: Flatpak access to notebook directory trees

- **Status:** Planned
- **Impact:** A portal grant for only the `.onetoc2` file may not expose
  sibling sections and nested groups.
- **Mitigation:** select/grant the notebook directory, test persistent document
  portal permissions, and clearly request the minimal necessary tree.
- **Package constraint:** Flatpak cannot assume the host `7z` is visible. The
  current manifest bundles a checksum-pinned `7zz` and corresponding source;
  destination portal grants and end-to-end packaged import still need tests.
  Do not escape the sandbox to invoke a host executable.

### L-025: Source-code licensing and third-party boundaries

- **Status:** Mitigated
- **Impact:** Project and dependency terms must remain available when source or
  binaries are redistributed.
- **Mitigation:** Project-owned code and documentation use
  GPL-3.0-or-later. The parser remains MPL-2.0 and is additionally distributed
  under GPL-3.0-or-later in the combined work; icon and Microsoft-document
  terms remain separate. Release artifacts carry the complete notices,
  corresponding-source instructions, and a generated locked-dependency report.
- **Residual risk:** Every dependency update still requires a license audit,
  and bundled AppImage/native runtime components require release-specific
  inventories.

### L-026: Sensitivity labels and information-rights management

- **Status:** Accepted
- **Impact:** Locally present metadata can identify protected content while
  decryption/authorization may require Microsoft identity and policy services.
- **Mitigation:** Never bypass policy or treat this as ordinary password
  encryption. Show a distinct unsupported-protection diagnostic.

### L-027: Premature public API stability

- **Status:** Open
- **Impact:** Freezing parser, scene, GTK, or query interfaces before corpus
  evidence can preserve poor abstractions; leaving them informal makes
  downstream reuse unreliable.
- **Mitigation:** Keep pre-1.0 evolution explicit, expose stable domain and
  locator values instead of dependency internals, require semantic versioning
  and migration notes, and test every public boundary from an independent
  consumer.
- **Release gate:** The public integration API quality contract passes,
  including structured errors, cancellation, limits, thread/callback
  documentation and protocol golden fixtures. The reuse-compatible license
  requirement is satisfied by GPL-3.0-or-later.

## Implemented Resource Ceilings

Current defensive defaults are:

- 10,000 sections, 1,000,000 pages/resources, 2,000,000 objects, and
  10,000,000 ink points per projected object;
- 1,000,000 extracted entries and a 16 MiB bounded archive listing;
- 32 MiB encoded and 64 MiB decoded per image, maximum dimension 16,384, and a
  128 MiB GTK texture cache;
- 1,000,000 generated scene nodes per page;
- 1,000 search results and 2,048 snippet characters per query.

These ceilings are code defaults, not corpus-tuned release guarantees.
Section bytes, aggregate model memory, attachment extraction, expanded package
bytes/disk, coordinates, graph depth, and concurrency queues still need
measured limits. Until those and hostile-input tests pass, release builds must
not claim arbitrary untrusted notebook/package safety.
