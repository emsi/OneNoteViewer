# Search Specification

## User Contract

Search operates across all currently open notebooks without network access.
Results appear incrementally, are ranked, show notebook/section/page context
and a highlighted snippet, and navigate to the matching page/object.

Source notebooks remain authoritative. The index is disposable derived data.

The same search capability is a public headless interface for other software;
the desktop result view is only one consumer. See
[the public integration API](public-api.md).

## Indexed Document

One FTS document represents one page with these logical fields:

| Field | Source | Indexed |
|---|---|---|
| `title` | Effective page title | Yes, high weight |
| `body` | Visible rich text in source reading order | Yes |
| `tags` | Labels, task state, and due-date text | Yes |
| `alt_text` | Image alternative text | Yes |
| `ink_text` | Stored handwriting recognition | Yes |
| `attachments` | Embedded filenames and detected type | Yes |
| `links` | Visible labels and normalized target text | Yes |
| `path` | Notebook, section group, section names | Yes |
| stable IDs, timestamps, geometry | Metadata columns | No |

Hidden text is excluded by default. Deleted and historical content are
excluded from the active index. A later history view can use a separate index
scope.

Attachment body text, remote image content, and newly run OCR are not part of
MVP search.

## Storage

Use an ordinary metadata/content table and an FTS5 external-content table in
one SQLite database. Database metadata includes:

- schema version;
- application build;
- parser version/commit;
- source canonical identity and fingerprint;
- completed index generation.

Index updates occur in a transaction and publish only when a complete
generation succeeds. Foreign keys and FTS consistency checks run in tests.

## Tokenization and Normalization

- Preserve original Unicode for display.
- Normalize line endings and replace format-only object markers with spaces.
- Use FTS5 `unicode61` initially, with diacritic behavior documented by tests.
- Do not lowercase, stem, or transliterate in application code.
- Keep language IDs so a future schema can choose language-aware tokenizers.
- Add two- and three-character prefix indexes only after size benchmarks.

CJK segmentation, stemming, and accent behavior require explicit multilingual
fixtures. Changing a tokenizer increments the index schema and rebuilds.

## Query Behavior

The simple search box treats ordinary user text as a safely quoted sequence,
not raw FTS syntax. It provides:

- all-term matching by default;
- quoted phrase support;
- trailing `*` prefix only through validated query construction;
- filters for notebook, section, tag, attachment, and date;
- relevance ranking weighted approximately:
  `title > tags > path > body/ink/alt/attachments/links`;
- deterministic tie-breaking by updated time then source order.

An optional advanced mode may expose boolean and `NEAR` syntax later. Invalid
input returns a user-readable parse error, never interpolated SQL.

Library and protocol callers submit the same structured query model. They do
not supply raw SQL or depend on FTS5 syntax unless a separately versioned
advanced-query field is introduced.

## Results and Navigation

Each indexed fragment keeps a source locator and optional canvas bounding box.
A result opens the notebook and section, selects the page, scrolls the match
into view, and highlights it temporarily. If geometry is unavailable, it opens
the page and selects the nearest semantic object.

Snippets are produced from stored original text with UI-native highlighting,
not inserted HTML.

Public results also include the source fingerprint and stable
notebook/section/page/object locators needed to resolve the match from another
process. Display strings are never the identity contract.

## Performance Gates

Measure using the corpus plus a generated large workspace:

- search keystroke to first result: target under 100 ms warm;
- complete top-100 results: target under 300 ms warm;
- cancellation of an obsolete query: under 50 ms;
- reindex only changed sections after fingerprint comparison;
- UI remains responsive during full rebuild.

These are targets pending baseline hardware selection, not current claims.
