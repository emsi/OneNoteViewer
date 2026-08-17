# Reusable OneNote Backup-Folder Loader Plan

- **Status:** Implemented for the issue #39 baseline; later extensions remain tracked separately
- **Owner:** `onenote-core`, integrated by `onenote-viewer`
- **Target milestone:** Milestone 2 for default loading; Milestone 3 for
  historical-version browsing
- **Last reconciled:** 2026-08-17 UTC

## Purpose

Provide a reusable, read-only loader for OneNote backup directories that
contain recursively arranged `.one` section files but no usable root
`.onetoc2` table of contents.

Such a directory must open as one synthetic notebook. Its subdirectories must
be represented as nested section groups, its logical sections must be
deduplicated across dated backup snapshots, and the selected sections must be
available through the same public domain, rendering, and indexing interfaces
as a manifest-backed notebook.

## Implemented Baseline

The issue #39 implementation provides:

- bounded, symlink-safe inspection through public `onenote-core` types;
- anchored recognition of the two observed dated-filename profiles;
- deterministic latest-per-section and explicit all-copies policies;
- one stable aggregate source with nested directory groups, native page order,
  source-scoped identities, lazy resource loaders, and structured diagnostics;
- typed workspace persistence, legacy path-only workspace migration, manual
  refresh, phase progress, and cancellation;
- staged viewer publication after transactional index replacement, preserving
  the last known-good visible generation when loading or indexing fails; and
- root-manifest precedence with an explicit, copyable fallback confirmation
  when a present manifest cannot be parsed.

As-of/exact-date views, changed-section parse reuse, automatic monitoring,
viewer-local ordering overlays, and a general stabilized source-classification
facade remain future work. They are extension points rather than part of the
implemented baseline.

The loader belongs in `onenote-core`. It must not depend on GTK, the viewer
workspace, SQLite, or the search index. OneNote Viewer is one consumer of the
loader, not its only usable host.

## Problem Statement

Before this loader, the application discovered every `.one` below a directory
when it could not find a root `.onetoc2`. It then loaded each file through the
standalone section path, so one backup appeared as dozens of one-section
notebooks. Directory-based section groups and repeated backup versions were
lost.

This behavior is correct for an explicitly opened standalone `.one` file but
is not an adequate interpretation of a backup directory.

## Evidence and Claim Boundary

The initial design is based on aggregate inspection of a private OneNote
backup corpus. That corpus contains:

- 83 `.one` files and no `.onetoc2`;
- 42 logical section paths after normalizing observed backup suffixes;
- 41 logical sections with more than one dated snapshot;
- 10 top-level directory groups;
- filename suffixes in both of these forms:
  - `<section>.one (On YYYY-MM-DD).one`
  - `<section> (On DD-MM-YYYY).one`

Observed file modification times generally agree with the dates in those
suffixes. These facts justify fixtures and an initial compatibility profile;
they do not prove that every OneNote version, locale, or backup configuration
uses these conventions. Detection and fallback behavior must therefore be
explicit, diagnostic, and extensible.

Private section or page names must not be committed to tests, logs, or
documentation. Synthetic or independently licensed fixtures remain required.

## Goals

1. Open a manifest-free backup directory as one notebook source.
2. Preserve recursive directories as nested section groups.
3. Group multiple dated files that represent snapshots of one logical section.
4. Select the newest snapshot candidate of each logical section by default and
   report a parse failure rather than silently substituting older content.
5. Retain metadata and provenance for unselected snapshots without eagerly
   loading their complete content.
6. Expose inspection and loading through a reusable `onenote-core` API.
7. Give the aggregate source and its contents stable, source-scoped identities.
8. Make the aggregate notebook usable unchanged by the renderer and indexer.
9. Support deterministic refresh, cancellation, progress, resource ceilings,
   and partial-success diagnostics.
10. Never modify, repair, rename, or reorganize the backup directory.

## Non-Goals

- Reconstruct metadata that is absent without a `.onetoc2`, including native
  section order, section color, closed/open state, or original group identity.
- Treat every directory containing a `.one` file as a backup automatically
  without reporting how it was classified.
- Merge sections across different relative directories based only on similar
  names.
- Silently substitute an older section when the selected snapshot is corrupt.
- Parse all historical snapshots or materialize all resources in memory during
  normal opening.
- Modify or repair an incomplete backup.
- Provide cloud sync, OneDrive access, editing, HTML/Markdown conversion, or
  package extraction.
- Make `.onepkg` a runtime source. Package extraction remains the separate
  one-time, on-disk workflow defined by ADR 0002.

## Terminology

- **Manifest-backed notebook:** A notebook whose hierarchy and ordering come
  from a `.onetoc2`.
- **Backup folder:** A selected directory with recursive `.one` files and no
  usable root notebook manifest, classified by this loader.
- **Synthetic notebook:** The immutable `Notebook` projected from a backup
  folder. "Synthetic" describes its reconstructed container and hierarchy, not
  its native `.one` page content.
- **Logical section key:** The normalized relative parent path plus the
  normalized section basename after a recognized backup suffix is removed.
- **Snapshot:** One physical `.one` candidate for a logical section.
- **Latest-per-section view:** A composite notebook containing the newest
  selected snapshot of each logical section. It is not necessarily a
  historically coherent notebook state.
- **As-of view:** For each logical section, the newest snapshot whose effective
  date is no later than a caller-selected cutoff.
- **Provenance:** The physical path, parsed filename date, filesystem metadata,
  selection reason, and diagnostics connecting a projected section to its
  source snapshot.

## Source Classification and Dispatch

Classification must be a reusable core operation and return evidence, not only
a Boolean:

1. An explicitly selected `.one` remains a standalone section source.
2. An explicitly selected `.onetoc2` uses the existing manifest-backed loader.
3. A selected directory with a usable root `.onetoc2` uses the manifest-backed
   loader. The manifest is authoritative; the loader must not also synthesize a
   second notebook from adjacent `.one` files.
4. A selected directory with no usable root `.onetoc2` and at least one valid
   recursively discovered `.one` candidate may be classified as a backup
   folder.
5. A directory with neither a usable manifest nor a valid section returns a
   structured unsupported-source error.

The public API must provide both:

- an explicit `inspect_backup_folder` operation for callers that already know
  what source type they intend to load; and
- a general directory/source inspection operation that reports the selected
  classification and reasons.

Opening a directory must not depend on viewer-private filename scanning.

## Proposed Public API Shape

Exact Rust names may change during implementation, but the capability boundary
must remain equivalent to the following:

```rust
pub struct BackupFolderLoader {
    limits: BackupFolderLimits,
}

pub struct BackupFolderOptions {
    pub snapshot_selection: SnapshotSelection,
    pub recycle_bin: RecycleBinPolicy,
    pub parse_failures: ParseFailurePolicy,
}

pub enum SnapshotSelection {
    LatestPerSection,
    AsOf(SystemTime),
    ExactDate(NaiveDate),
}

pub enum RecycleBinPolicy {
    ExcludeFromPrimary,
    IncludeAsSectionGroup,
}

pub enum ParseFailurePolicy {
    Strict,
    FallBackWithDiagnostic,
}

pub struct BackupFolderInspection {
    pub source: SourceDescriptor,
    pub logical_tree: BackupTree,
    pub snapshots: Vec<BackupSnapshot>,
    pub selection: SnapshotSelectionReport,
    pub diagnostics: Vec<Diagnostic>,
}

impl BackupFolderLoader {
    pub fn inspect(
        &self,
        root: &Path,
        options: &BackupFolderOptions,
        cancel: &CancellationToken,
        progress: &dyn ProgressSink,
    ) -> Result<BackupFolderInspection, BackupFolderError>;

    pub fn load(
        &self,
        inspection: BackupFolderInspection,
        cancel: &CancellationToken,
        progress: &dyn ProgressSink,
    ) -> Result<LoadedNotebook, BackupFolderError>;
}
```

`inspect` must perform bounded metadata discovery and selection without parsing
every historical section. `load` parses selected candidates and returns the
existing public notebook model, lazy resource store, source fingerprint, and
diagnostics. Inspection data must not expose parser-internal types.

The general source API may later wrap this as `OneNoteSourceLoader` or
`DirectorySourceLoader`. That naming decision must not move backup aggregation
into `onenote-viewer`.

## Discovery Algorithm

1. Canonicalize the selected root once.
2. Traverse it recursively under configured entry and depth limits.
3. Skip symbolic links by default and report each skipped link in bounded
   aggregate diagnostics.
4. Reject any resolved path outside the canonical root.
5. Record only metadata required for classification, grouping, selection, and
   fingerprinting.
6. Identify candidates by extension and then validate them through the native
   section loader before accepting the final projection.
7. Keep traversal order irrelevant by sorting candidates with a documented,
   deterministic path comparator before grouping.
8. Check cancellation during traversal, grouping, selection, and parsing.

Discovery must not read all file bodies into memory. File reads performed by
the parser and lazy resource loaders remain bounded by core limits.

## Filename and Snapshot Normalization

Backup-suffix recognition must be an anchored parser, not broad string
replacement. For the initial profile it recognizes only a final `.one` and one
of the observed suffix families:

```text
<logical-name>.one (On YYYY-MM-DD).one
<logical-name> (On DD-MM-YYYY).one
```

Rules:

- The date must be a real calendar date.
- Date order is selected from the structural position of the four-digit year,
  not guessed from host locale.
- The word `On` is matched only as part of a complete supported suffix.
- An invalid or unrecognized suffix is preserved as part of the logical name
  and produces a classification diagnostic when it resembles a backup suffix.
- An undated file remains a distinct candidate unless native identity evidence
  safely relates it to dated snapshots.
- Display spelling and Unicode are preserved.
- Logical-key comparison uses one documented normalization policy and reports
  case or Unicode collisions rather than silently merging ambiguous paths.
- Native section metadata may supply the display name after parsing, but it
  does not retroactively merge ambiguous candidates without stable identity
  evidence.

The filename parser must be isolated behind a small profile interface so
additional producer or locale patterns can be added with fixtures rather than
rewriting traversal.

## Logical Tree Reconstruction

For a backup source:

- the selected root basename is the fallback notebook display name;
- each relative directory is a `SectionGroup`;
- each selected logical section is a `Section` under its relative parent;
- arbitrary nesting depth is preserved within configured limits;
- empty directories do not create groups unless future evidence shows they
  carry OneNote meaning;
- sections at the root remain direct notebook children.

Without a manifest, native sibling order is unavailable. The synthetic tree
uses a deterministic natural path/name ordering and emits a structured
`synthetic_order` diagnostic. The UI may explain that backup ordering was
reconstructed, but it must not imply that alphabetical order came from
OneNote.

If parsed internal section metadata and the filename disagree, use the parsed
name for display when it is valid, retain the logical path for identity, and
record both values in provenance. Do not guess that same-named sections in
different directories are the same section.

## Snapshot Grouping and Selection

Candidates are grouped by the logical section key:

```text
(normalized relative parent path, normalized logical section basename)
```

The default selection order is:

1. newest valid date parsed from a recognized backup filename;
2. filesystem modification time when no filename date is available or as a
   tie-breaker;
3. deterministic relative-path byte ordering as the final tie-breaker.

The selection report must state which evidence selected each snapshot. Equal
dates with differing size, metadata, or content fingerprint are conflicts,
not silently interchangeable duplicates.

### Latest Per Section

This is the default interactive view. Each logical section contributes its
newest selected snapshot. The report must describe it as a composite view
because different sections may have been backed up on different dates.

### As-Of

For each logical section, select the newest snapshot no later than the cutoff.
A section with no eligible snapshot is absent and reported. This provides a
useful approximate historical view without claiming an atomic notebook
checkpoint.

### Exact Date

Select only candidates whose recognized backup date exactly matches the
requested date. Return explicit coverage and incompleteness information. This
mode is primarily useful to inspect producer behavior and compare backups.

### Parse Failure

In strict mode, failure of a selected snapshot leaves that logical section
failed and visible in diagnostics.

In fallback mode, the loader may try older candidates in descending selection
order. A fallback must:

- be bounded;
- be recorded in section provenance and notebook diagnostics;
- state both the rejected and selected snapshot dates;
- never be presented as the latest valid backup without qualification.

The viewer should initially use strict mode unless user testing demonstrates a
clear need for fallback. Libraries expose both policies.

## Historical Snapshot Retention

Normal opening stores lightweight metadata for unselected snapshots:

- relative physical path;
- logical section key;
- parsed backup date, if any;
- size and modification time;
- selection rank and exclusion reason;
- optional bounded content fingerprint;
- diagnostics.

The complete historical `.one` content and resources remain on disk and are
loaded only when a caller explicitly requests that snapshot. The initial
viewer need not expose history browsing, but the core model must not discard
the information required to add it later.

Snapshot metadata may live beside, rather than inside, the renderer-facing
`Notebook` model so render and index consumers are not forced to understand
backup policy.

## Recycle Bin and Special Directories

OneNote backup layouts may contain a recycle-bin subtree. The initial loader
must recognize only configured, evidence-backed well-known paths. It must not
classify arbitrary directories by substring.

Default behavior:

- exclude recycle-bin sections from the primary notebook tree and active
  index;
- retain them in inspection metadata;
- report their count without logging private filenames.

An explicit option may include the subtree as a clearly labeled synthetic
section group. No mode deletes or modifies it.

## Identity and Fingerprinting

### Source Identity

The aggregate `SourceId` is derived from the canonical backup root and source
kind, not from each physical `.one` file. It must remain stable when a new
backup snapshot appears under the same root.

### Content Identity

Section, page, object, and resource locators are namespaced by the aggregate
source. Section identity combines:

- the logical relative path;
- stable native section identity when available;
- an explicit version of the identity algorithm.

The loader must not reuse the standalone-section projection unchanged because
that path assigns an independent source identity to each file.

### Fingerprint

The source-generation fingerprint must cover enough metadata to detect:

- added or removed logical sections;
- new or removed snapshots;
- replacement of a selected snapshot;
- changes to a candidate at the same path;
- changes in loader policy or normalization profile that alter projection.

The initial implementation may combine normalized relative path, file size,
high-resolution modification time, and bounded native metadata. Full-file
hashing is optional when needed to resolve collisions or unreliable mtimes.
Fingerprint construction must be streaming and bounded.

## Aggregate Projection and Resources

Loading several selected parser sections into one source requires a projection
context shared across files:

- one aggregate source identity and fingerprint;
- collision-safe section/page/object identifiers;
- one notebook tree;
- one diagnostic collection;
- one lazy `ResourceStore`.

`ResourceStore` must gain an internal merge/extend operation that:

- preserves lazy resource loaders;
- rejects conflicting `ResourceId` entries;
- never materializes payload bytes merely to combine stores;
- associates resource failures with the owning section snapshot.

Projection failures must be isolated per selected section where possible. A
usable partial notebook and structured load report are preferable to losing
all readable sections, subject to caller-selected strictness.

## Errors and Diagnostics

Errors stop the operation when the source cannot be classified or safely
traversed. Diagnostics describe bounded partial incompatibility.

Required structured cases include:

- root missing, unreadable, or not a directory;
- no valid `.one` candidates;
- usable root manifest found, so backup classification was not selected;
- traversal entry/depth/metadata limit exceeded;
- symbolic link skipped or root escape rejected;
- malformed or unsupported backup suffix;
- logical-name normalization collision;
- duplicate or conflicting snapshot date;
- selected snapshot disappeared or changed during loading;
- selected section parse failure;
- older-snapshot fallback used;
- parsed name disagrees with backup filename;
- synthetic ordering used because no manifest exists;
- recycle-bin content excluded;
- cancellation;
- source changed between inspection and completed load.

Diagnostics must use stable codes, source-relative paths, and bounded details.
Application logs and progress events must omit private names by default.

## Resource and Security Limits

`BackupFolderLimits` must include at least:

- maximum traversal entries;
- maximum directory depth;
- maximum candidate sections;
- maximum snapshots per logical section;
- maximum aggregate path bytes retained;
- maximum diagnostics retained per code and in total;
- maximum concurrent section parses;
- existing per-section/page/object/resource limits from `onenote-core`.

The loader:

- follows no symlinks by default;
- writes nothing in the source tree;
- opens files read-only;
- performs no archive extraction;
- performs no external process execution;
- does not load complete resource payloads during discovery;
- checks source metadata before and after parsing to detect concurrent change;
- returns cancellation promptly at documented checkpoints.

## Progress and Concurrency

Progress uses stable phases:

1. classifying source;
2. discovering candidates;
3. grouping snapshots;
4. selecting snapshots;
5. parsing selected sections;
6. assembling notebook and resources;
7. final fingerprint verification.

Counts may be reported, but private filenames are opt-in diagnostic details.
Parsing may use bounded worker concurrency after single-threaded behavior is
correct. Aggregation and output ordering must remain deterministic regardless
of task completion order.

## Viewer Integration

`onenote-viewer` must:

1. replace its manifest-free "every `.one` is a source" discovery fallback
   with the core source-classification API;
2. enqueue one load job for a backup root;
3. display the returned synthetic notebook and nested section groups through
   the existing navigation model;
4. persist the backup root, source kind, normalization profile, and snapshot
   policy instead of every selected physical file;
5. show concise compatibility/selection diagnostics;
6. index the aggregate notebook as one source generation;
7. keep the previously loaded notebook and index generation if refresh fails.

The renderer requires no backup-specific behavior. It receives ordinary
projected pages and their native freeform geometry.

## Workspace Migration

Existing workspaces may already contain many standalone source entries created
by opening a backup directory with the current fallback.

Migration must not silently collapse arbitrary standalone `.one` sources.
When a user explicitly reopens or refreshes a recognized backup root, the
viewer may replace persisted standalone entries only when all of these hold:

- each candidate is a descendant of that canonical root;
- each was created by directory discovery rather than an explicit standalone
  file open, where provenance is available;
- the aggregate backup load succeeds;
- index replacement commits successfully.

Otherwise, preserve existing entries and report a deduplication opportunity.
The migration must be transactional so failure leaves the prior workspace and
index usable.

## Refresh Semantics

Refresh is an inspection/load/index transaction:

1. inspect the root with the persisted policy;
2. compute and compare the candidate/selection fingerprint;
3. parse only newly selected or changed logical sections where the projection
   API permits;
4. assemble and validate the complete next notebook generation;
5. transactionally replace the indexed source generation;
6. publish the new workspace generation;
7. retain the old generation if any step fails or is cancelled.

A source mutation during inspection or load returns a retryable
`source_changed` result. The loader must not combine metadata and content from
an unbounded moving target.

## Index and Query Integration

`onenote-index` continues to accept one public `Notebook` plus its source
fingerprint. It must not parse backup filenames or traverse directories.

For a backup source:

- latest-per-section content is indexed by default;
- excluded historical and recycle-bin snapshots are not indexed;
- hits resolve through the aggregate source and stable logical section
  locator;
- a refresh replaces one aggregate source generation transactionally;
- future historical indexing, if added, must use explicit snapshot/version
  locators and must not mix historical pages into default search silently.

## Implementation Work Packages

### Phase 0: Contract and Fixtures

- Finalize source-kind, inspection, selection, provenance, and diagnostic
  types.
- Add synthetic filename and directory fixtures for both observed suffix
  families.
- Record only aggregate expectations for the private corpus.
- Decide and document path comparison and Unicode normalization behavior.

**Gate:** API review confirms that a non-viewer Rust caller can inspect and
load a backup folder without GTK or SQLite.

### Phase 1: Inspector and Classifier

- Move bounded directory discovery from `onenote-viewer` into `onenote-core`.
- Implement root-manifest precedence and explicit backup classification.
- Implement anchored filename parsing, logical grouping, selection reports,
  special-directory policy, limits, cancellation, and deterministic ordering.

**Gate:** metadata-only tests cover shuffled traversal, invalid dates,
collisions, symlinks, limits, cancellation, and all selection modes.

### Phase 2: Aggregate Loader

- Refactor projection to load multiple parser sections under one source.
- Add stable aggregate identities and fingerprinting.
- Merge lazy resource stores with collision detection.
- Implement strict partial failure and diagnostic fallback policies.
- Return one `LoadedNotebook` plus snapshot provenance.

**Gate:** synthetic and licensed/native fixtures load as one notebook with the
expected group tree, sections, pages, resources, and stable identities.

### Phase 3: Viewer, Workspace, and Index

- Replace viewer-private fallback discovery.
- Persist one backup source descriptor and policy.
- Implement safe migration from directory-discovered standalone entries.
- Show reconstructed-order, conflict, failure, and fallback diagnostics.
- Replace the index as one aggregate generation.

**Gate:** opening the representative private backup folder yields one notebook,
all selected logical sections are reachable, and global search returns locators
under that one source.

### Phase 4: Refresh and Historical Views

- Implement transactional source refresh with changed-section reuse.
- Expose snapshot inventory and provenance in a restrained viewer surface.
- Add as-of and exact-date opening without changing the default active source
  silently.
- Define historical indexing policy before indexing any old snapshot.

**Gate:** adding a newer section snapshot updates only the projected generation,
cancellation preserves the old view/index, and historical selection is
reproducible.

### Phase 5: Public API Hardening

- Publish Rustdoc examples for inspection, default load, and as-of load.
- Add an independent consumer integration test.
- Freeze diagnostic codes and serialization forms needed by non-Rust adapters.
- Document thread, callback, cancellation, and compatibility behavior.

**Gate:** the loader satisfies the public API quality contract and can be used
by another note-taking or desktop-search application without viewer-private
types.

## Test Strategy

### Unit Tests

- Both observed suffix families and undated filenames.
- Leap days, invalid dates, Unicode, case collisions, embedded `(On ...)`
  text, and multiple `.one` substrings.
- Logical-key construction at root and nested depths.
- Latest, as-of, and exact-date selection.
- Modification-time and lexical tie-breakers.
- Deterministic output from randomized input order.
- Limit accounting and diagnostic truncation.

### Filesystem Tests

- Root manifest precedence.
- Manifest-free recursive tree reconstruction.
- Empty, unreadable, changing, and deeply nested directories.
- Symlink files/directories and attempted path escape.
- Candidate removal/replacement between inspection and load.
- Recycle-bin exclusion and explicit inclusion.
- Cancellation at every progress phase.

### Native Section Tests

- Multiple valid `.one` sections aggregated under one source.
- Duplicate internal GUIDs in different logical paths.
- Parsed-name/filename disagreement.
- Corrupt newest snapshot in strict and fallback modes.
- Lazy images/attachments retained after resource-store merge.
- Resource identifier collision reported without eager payload reads.
- Partial section failure does not panic or erase unrelated sections.

### Integration Tests

- Viewer opens a backup root as one notebook with expandable section groups.
- Page navigation, rendering, and global search work without backup-specific
  renderer or index code.
- Workspace close/reopen preserves source kind and selection policy.
- Migration does not collapse explicitly opened standalone files.
- Refresh and index replacement are transactional.
- Independent core consumer loads and enumerates the synthetic notebook.

### Private Corpus Check

An opt-in local test may verify the observed aggregate counts of 83 physical
files, 42 logical section paths, 41 multi-snapshot sections, and 10 top-level
groups. It must accept the corpus path from the environment, skip when absent,
and never print or snapshot private names.

## Acceptance Criteria

The default loader is complete when:

1. a manifest-free backup root is represented by exactly one `SourceId` and
   one notebook;
2. relative directories become expandable nested section groups;
3. repeated dated files produce one selected logical section by default;
4. selection is deterministic and fully represented in provenance;
5. older snapshots remain discoverable without eager parsing;
6. native page geometry and resources reach existing render/index APIs
   unchanged;
7. resource use, traversal, concurrency, diagnostics, and cancellation are
   bounded;
8. symlinks and source mutation cannot escape or corrupt the read-only load;
9. corrupt/ambiguous data is visible through stable diagnostics and never
   silently merged;
10. source refresh and workspace/index replacement are transactional;
11. an independent non-viewer consumer test uses only public core APIs;
12. the representative private corpus passes aggregate checks without leaking
    private names.

Historical browsing is complete only after as-of/exact-date UI, provenance,
and locator behavior have separate acceptance evidence.

## Risks and Recommended Defaults

- **Filename conventions vary by producer and locale.** Start with the two
  observed anchored profiles and preserve unknown names; do not generalize by
  guesswork.
- **Newest-per-section is not an atomic backup.** Use it for practical default
  viewing but label the projection accurately in diagnostics and provenance.
- **Modification times can be copied or rounded.** Prefer validated filename
  dates, use mtimes as fallback/tie-break evidence, and surface conflicts.
- **Directory paths may reflect backup organization rather than native group
  identity.** Preserve them as the best available synthetic hierarchy and
  report reconstructed ordering.
- **Fallback can hide corruption.** Default the viewer to strict selection;
  expose fallback as an explicit library policy until UX evidence supports it.
- **Fingerprinting every historical file can be expensive.** Begin with
  streaming metadata/native-header evidence and hash only ambiguous or changed
  candidates.
- **Aggregate projection may expose identifier collisions.** Namespace by
  source and logical section path, retain native IDs as evidence, and test
  collision behavior before incremental refresh.

## Open Decisions Requiring Evidence

These choices remain provisional and must be closed with fixtures or measured
behavior before the public API is stabilized:

1. **Path-key normalization:** byte-preserving comparison is safest on Linux,
   while case-insensitive or Unicode-normalized matching may better reflect the
   Windows producer. Recommendation: preserve exact paths and report possible
   collisions until cross-platform fixtures justify merging.
2. **Native identity across renamed/moved sections:** a stable internal section
   GUID may relate snapshots whose paths differ, but could also collide after a
   copy. Recommendation: use path as the logical key initially and expose
   native identity as provenance, not an automatic cross-directory merge key.
3. **Fingerprint strength:** metadata fingerprints are fast but can miss
   replaced files with preserved timestamps. Recommendation: hash bounded
   native headers during inspection and complete files only for ambiguity or
   change verification, then benchmark.
4. **Viewer fallback policy:** falling back to an older parsable snapshot
   increases readable coverage but can conceal a broken newest backup.
   Recommendation: strict by default, with explicit user action for fallback.
5. **Workspace migration UX:** automatic cleanup is convenient but risks
   collapsing intentionally opened files. Recommendation: migrate
   automatically only with persisted directory-discovery provenance;
   otherwise present a non-destructive diagnostic.
6. **Historical locator lifetime:** an as-of snapshot could share page GUIDs
   with the active generation. Recommendation: add an explicit snapshot
   version component before exposing history to query clients.

## Expected Code Areas

- `crates/onenote-core/src/`: source classification, backup inspection,
  filename profiles, selection, aggregate projection, provenance, limits,
  fingerprinting, and resource-store merging.
- `crates/onenote-viewer/src/workspace.rs`: replace recursive fallback and
  persist one backup source descriptor.
- `crates/onenote-viewer/src/worker.rs`: load and refresh one aggregate source
  with progress/cancellation.
- `crates/onenote-viewer/src/ui/`: diagnostics and future snapshot selection;
  no parsing logic.
- `crates/onenote-index/`: no backup discovery; only any locator/generation
  changes required by the aggregate core model.
- `fixtures/`: synthetic/licensed directory layouts and native section cases;
  never the private backup.

## Documentation Updates Required During Implementation

- Mark the corresponding roadmap item complete only after its gate passes.
- Update `specs/public-api.md` with the final type names and compatibility
  contract.
- Update `specs/onenote-format.md` with evidence-backed backup profiles while
  keeping filename conventions distinct from binary format claims.
- Add corpus cases and feature-matrix rows for classification, hierarchy,
  snapshot selection, and diagnostics.
- Update the tracking issue with tested producers/locales and unresolved
  conventions.
- Update [issue #39](https://github.com/emsi/OneNoteViewer/issues/39) and the
  master plan's current evidence.
- Add migration notes before changing persisted workspace source descriptors.

## Definition of Done

This plan is done when the reusable core loader, viewer integration, aggregate
index behavior, refresh transaction, public documentation, independent
consumer test, licensed fixtures, and private aggregate corpus check all pass;
no viewer-private fallback interprets a backup directory as independent
standalone notebooks; and remaining producer/version uncertainty is stated
precisely rather than hidden by broad compatibility claims.
