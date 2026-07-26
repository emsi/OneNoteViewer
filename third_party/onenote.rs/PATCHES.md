# OneNote Parser Fork

This directory vendors `onenote.rs` revision
`f9cdc59f984bc1f7f096b54100cefaaebc892573` (version 1.1.1) under its original
MPL-2.0 license. It contains only the parser and parser-macro source required
at runtime; upstream test samples and snapshots are intentionally omitted.

The application depends on this audited snapshot instead of exposing the
upstream parser as a public API. Keep local changes narrow and suitable for
upstream submission.

## Local Compatibility Patches

1. `OutlineElementNode` accepts a missing creation timestamp. The timestamp is
   not consumed by the high-level parser model, and OneNote Desktop sections
   in the project corpus legitimately omit it.
2. `PropertyValue::to_u8_lossless` accepts unsigned integer encodings of
   different widths only when the value fits exactly in `u8`.
3. Page-size and paragraph-alignment parsing use that lossless conversion.
   This accepts native sections emitted with a wider property encoding without
   truncating malformed values.

The supplied private corpus has one section requiring patch 1 and two sections
that reach a wider numeric property requiring patches 2 and 3. All 32 native
sections parse after these changes.
