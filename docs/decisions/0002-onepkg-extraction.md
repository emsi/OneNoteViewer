# ADR 0002: Extract ONEPKG to a Durable Native Notebook Tree

- **Status:** Accepted
- **Date:** 2026-07-26
- **Decision owners:** Project maintainers

## Context

A `.onepkg` export is a one-time transport container. Observed exports,
including the project owner's package, are Microsoft Cabinet archives whose
payload is an ordinary hierarchy of `.onetoc2` and `.one` files. Microsoft
documents the CAB container in `[MS-CAB]` and the payload files in
`[MS-ONESTORE]` and `[MS-ONE]`; no separate normative `[MS-ONEPKG]`
specification was found. The mapping from archive paths to notebook/section
group layout is therefore corpus-backed behavior.

The inspected `onenote.rs` package API reads the archive and expanded entries
into memory. That API is unsuitable for this application even though its
OneNote parser remains the selected dependency. Packages can be hundreds of
megabytes compressed and larger when expanded, and extraction does not benefit
from sharing the viewer's address space.

## Decision

Treat `.onepkg` as a managed, one-time on-disk extraction operation:

1. Detect an external 7-Zip command, preferring `7zz` and then `7z`.
2. Verify the source begins with the CAB `MSCF` signature.
3. List the archive before extraction and reject unsafe paths, excessive entry
   counts, excessive declared sizes, unsupported entry types, and policy
   violations.
4. Create a new mode-`0700` staging directory on the same filesystem as the
   chosen final destination.
5. Invoke 7-Zip directly with an argument vector, never through a shell, using
   noninteractive overwrite and progress settings and a stable `C` locale.
6. Extract only into the staging directory. Never buffer the complete archive
   or all expanded entries in application memory.
7. Walk the resulting tree without following symlinks. Accept only regular
   files and directories contained beneath staging.
8. Require at least one valid `.onetoc2` and `.one` file. The shallowest TOC is
   the notebook root candidate; nested TOCs are valid for section groups.
9. Confirm the source fingerprint is unchanged, then atomically rename staging
   to the unused final directory.
10. Open the final directory through the normal native notebook discovery
    path. Future launches do not need the archive or extractor.

The package source is never modified or deleted. Failure or cancellation
removes only the new staging directory. An existing destination is never
merged, overwritten, or partially replaced.

The initial extractor contract is deliberately narrow: known CAB-based
`.onepkg` exports through 7-Zip. A later libarchive subprocess/helper can be
added behind the same contract if needed. `cabextract` may be documented as a
manual fallback, but supporting multiple command outputs in the application
before fixtures require them would add avoidable ambiguity.

## Tool Availability and Packaging

When neither `7zz` nor `7z` is available, the application:

- disables only "Add from OneNote Package";
- explains which executable is missing and how to install the distribution's
  7-Zip package;
- offers "Open extracted notebook folder" so a manually unpacked package
  remains usable;
- continues to open, render, and index existing notebook folders and sections.

A native package may depend on or recommend 7-Zip. A sandboxed Flatpak cannot
assume arbitrary host executables are visible. Its release must either bundle
a reviewed extractor with compatible licensing or disable package extraction
with the same clear explanation. Escaping the sandbox to run a host command is
not an accepted design.

The current native viewer attempts detection after the package and destination
are selected and reports a structured error. Proactive button disablement and
installation guidance are still UI work.

## Security and Resource Policy

Archive listing is advisory until the extracted tree is independently checked.
The implementation must also apply:

- maximum entry, single-file, and total expanded-byte limits;
- canonical containment checks for Windows and POSIX path forms;
- rejection of absolute, drive, UNC/device, empty-component, and `..` paths;
- cancellation by terminating and reaping the extractor process;
- disk-space preflight plus clear handling of out-of-space failure;
- bounded capture of diagnostics without notebook filenames or content in
  telemetry;
- source hash or stable fingerprint checks around extraction.

The current implementation bounds listing capture to 16 MiB and extracted
entries to 1,000,000. It does not yet enforce single/total expanded bytes,
preflight disk space, force a locale, directly verify `MSCF`, or re-fingerprint
the source after extraction. These remain release blockers; the decision above
is not evidence that every step is already implemented.

## Implementation Status

`OnePkgExtractor` detects `7zz`/`7z`, validates a bounded structured listing,
tests the archive, rejects unsafe paths, extracts without a shell into a private
sibling staging directory, reaps/kills on cancellation, rejects symlinks and
special files, checks canonical containment and native file counts, rejects an
existing destination, and atomically publishes the durable tree. It never
loads archive contents through the parser package API.

The current viewer invokes that API off the GTK thread, reports each durable
pipeline phase in a persistent activity surface, supports cancellation through
the core process-reaping path, and opens the resulting directory through
normal discovery. The resource checks listed immediately above and
byte-accurate extractor progress are not implemented. The exact completion
work is tracked in
[issue #41](https://github.com/emsi/OneNoteViewer/issues/41).

## Consequences

- Package extraction consumes disk proportional to the expanded notebook, but
  avoids package-sized application memory growth.
- The durable result is inspectable, movable, and usable by this or another
  native `.one` reader.
- The viewing, rendering, and indexing architecture does not acquire a second
  package-specific code path.
- Package support depends on extractor availability outside distributions that
  bundle it.
- There is no conversion to HTML, Markdown, PDF, or a proprietary application
  database. Extraction changes the container only.

## Evidence

On 2026-07-26, an ignored private `.onepkg` fixture was identified as a CAB archive and
passed `7z 23.01`'s complete archive test: 37 entries and 306,633,759 declared
expanded bytes. Its paths passed the initial traversal check. The source
package was then extracted on disk in 1.465 seconds into
an ignored private directory: 32 `.one` files, one root `.onetoc2`, and four
nested `.onetoc2` files, with no other files or non-regular entries. All files
had the expected native OneNote header, the expanded total matched the listing,
the destination root retained mode `0700`, and the source SHA-256 remained
`cd872cf5b06630ce99f3254a8fbddafd05e4e21238c459725fc3823facb6bd95`.
The source package and extracted tree are private and ignored by Git;
filenames and content are not recorded in this ADR.

Relevant implementations and specifications:

- [`[MS-CAB]` Cabinet File Format](https://download.microsoft.com/download/4/d/a/4da14f27-b4ef-4170-a6e6-5b1ef85b1baa/%5Bms-cab%5D.pdf)
- [7-Zip](https://www.7-zip.org/)
- [libarchive](https://github.com/libarchive/libarchive)
- [Rust `cab` crate](https://docs.rs/cab/latest/cab/)
- [`onenote.rs` package implementation](https://github.com/msiemens/onenote.rs/blob/f9cdc59f984bc1f7f096b54100cefaaebc892573/crates/parser/src/onepkg.rs)
