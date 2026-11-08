//! Authentic binary templates for Microsoft OneNote .one and .onetoc2 files.

pub const TEMPLATE_SECTION_ONE: &[u8] = include_bytes!(
    "../../../third_party/onenote-parser-v2/crates/parser/tests/samples/joplin/new_section.one"
);

pub const TEMPLATE_ONETOC2: &[u8] = include_bytes!(
    "../../../third_party/onenote-parser-v2/crates/parser/tests/samples/Open Notebook.onetoc2"
);
