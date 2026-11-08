/// A pass-through value with a fixed `Debug` representation.
///
/// `f.debug_map().entry(k, v)` always formats `v` via `Debug`, which adds
/// quotes / escapes when `v` is a `String`. Use `DebugOutput` to substitute
/// already-formatted text without those decorations.
pub(crate) struct DebugOutput<'a>(&'a str);

impl<'a> From<&'a str> for DebugOutput<'a> {
    fn from(value: &'a str) -> Self {
        Self(value)
    }
}

impl std::fmt::Debug for DebugOutput<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
