# Local parser patches

This vendored parser is based on `emsi/onenote.rs` commit
`77cf881df7579eb58972e3db8ce0ca34d25a7f62`, the revision originally used by
the application. The root workspace patches that dependency to this local
copy so fixes are included in builds.

Local changes:

- Decode OneNote ink coordinates as cumulative signed deltas per dimension.
- Reject ink paths whose sample count cannot be divided across dimensions.
- Read stroke width and height from their matching properties.

The upstream license is in `LICENSE`.
