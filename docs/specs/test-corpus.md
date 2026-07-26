# Test Corpus Specification

## Purpose

OneNote compatibility cannot be established from the format PDFs alone.
Rendering behavior is partly undocumented and producer versions differ. This
matrix defines the evidence required before compatibility claims.

## Corpus Classes

### Redistributable

Small purpose-built notebooks committed under `fixtures/generated/` with a
README recording:

- producer application/version and operating system;
- exact creation steps;
- expected semantic object counts;
- OneNote screenshots or exports used as visual oracle;
- fixture license and whether personal metadata was removed.

Third-party fixtures may be included only with explicit compatible terms and
provenance.

### Private Compatibility Corpus

Real notebooks supplied locally through `ONENOTE_TEST_CORPUS`; never committed.
Tests report hashed case IDs and feature counts, not notebook text, filenames,
authors, or screenshots.

Current private evidence is one desktop `.onepkg`: extraction yields 32
sections and five TOCs; the root notebook projects all sections, every page
builds a finite scene, and 637 pages index and search. This is an implementation
smoke corpus, not fulfillment of the producer or feature matrices below.

### Malformed/Fuzz Corpus

Minimal generated truncations, length/count mutations, path escapes, archive
bombs, cycles, invalid UTF-16, decompression failures, and arbitrary fuzzer
findings. These contain no private source data.

## Producer Matrix

At minimum:

- OneNote Desktop 2016 local notebook;
- current Microsoft 365 OneNote Desktop local notebook;
- desktop backup copy;
- `.one` export from modern OneNote;
- locally downloaded OneDrive/FSSHTTP notebook;
- `.onepkg` export with a root TOC, nested section groups/TOCs, and Unicode
  paths;
- non-English and RTL notebook.

Legacy OneNote 2003/2007 examples are negative tests unless scope changes.

## Feature Cases

Each row should be isolated where practical:

- notebook color, reordered sections, nested section groups;
- top-level pages and multiple subpage levels;
- blank/untitled and very long titles;
- overlapping outlines, negative/large positions, fixed and automatic widths;
- every rich-text style, mixed scripts, bidi, emoji, combining marks;
- nested numbered/bulleted lists and restart values;
- tables with widths, shading, hidden borders, and nested content;
- all available built-in tag shapes plus custom tags and task dates;
- local images, remote-image placeholder, alternative text, background image;
- multi-page PDF and document printouts;
- attachments for Office, PDF, text, archive, audio/video, and executable types;
- internal, cross-section, cross-notebook, web, mail, file, and broken links;
- ink colors, highlighter, widths, nested groups, negative coordinates;
- recognized handwriting in multiple languages;
- simple and complex OfficeMath operator families;
- page versions, conflict pages, and deleted content;
- password-protected section as a negative case;
- unknown/modern embed object;
- one very large page and one very large notebook.

## Package Extraction Cases

The `.onepkg` acquisition suite is required for MVP and remains separate from
semantic parser fixtures:

- valid CAB packages with a root TOC, nested TOCs, Unicode names, and several
  sections;
- the project owner's private package, reported only by source hash, aggregate
  counts, and timings;
- missing `7zz`/`7z`, unsupported extractor version, and non-CAB input;
- corrupt and truncated CAB streams;
- absolute, drive-prefixed, UNC/device, `..`, separator-confused, and
  case-collision paths;
- symlink or non-regular output, if an extractor can produce it;
- too many entries, oversized single entry, excessive total expansion, and
  insufficient disk space;
- cancellation during listing and extraction;
- existing destination and source changes during extraction;
- extractor failure after partial output.

Every failure must leave the source and any pre-existing destination unchanged
and remove only the newly created staging directory. Tests monitor peak
application memory to prove package contents are not accumulated in-process.

## Public Integration Cases

Reusable components require consumers outside the desktop viewer:

- a headless Rust client opens a standalone `.one` through `onenote-core` and
  inspects semantic content, geometry, diagnostics, and lazy payload handles;
- a minimal GTK host embeds `onenote-render-gtk`, renders pages from the
  private and redistributable corpus, and handles link/attachment/navigation
  callbacks without linking `onenote-viewer`;
- `onenote-render` scene snapshots run without GTK or a display server;
- an independent Rust client indexes two notebook roots, queries across both,
  and resolves every result through source/page/object locators;
- a non-Rust test client negotiates the JSON Lines protocol, streams indexing
  progress/results, exercises cancellation, and verifies structured errors for
  malformed and unsupported requests;
- protocol-version and serialization golden fixtures detect accidental
  breaking changes;
- when the GObject wrapper exists, at least one non-Rust GTK-language example
  embeds it through generated introspection metadata.

These tests use only documented public interfaces. Importing private crate
modules, inspecting SQLite tables, or linking the viewer to make a test pass is
an API failure.

## Oracles

Use more than one where possible:

1. OneNote Desktop screenshots at 100% zoom.
2. OneNote PDF/HTML export, noting that export itself can differ.
3. Parsed object dump from the selected `onenote_parser` revision.
4. `one2html` output at its pinned revision.
5. An independent implementation such as Joplin's converter, Apache Tika, or
   LibMsON for overlapping features.
6. Hand-authored expected domain-model JSON that contains no binary payload.

Visual regression tests use scene/layout snapshots plus cropped PNGs with
tolerances for font rasterization. Semantic tests are primary; a screenshot
alone cannot prove search or accessibility.

## Acceptance

A corpus run records producer coverage, parsed/skipped pages, unknown JCIDs,
warning codes, fatal errors, peak memory, elapsed time, and rendered/indexed
feature counts. Release notes state exactly which producer rows passed.

No compatibility percentage is published unless the denominator is this
versioned matrix.
