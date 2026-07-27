use gtk::pango;
use num_traits::ToPrimitive;
use onenote_core::{TextAlignment, TextBlock, TextStyle};
use std::borrow::Cow;

const REPLACEMENT_CHARACTER: char = '\u{fffd}';

pub(crate) fn layout(
    context: &pango::Context,
    block: &TextBlock,
    marker: Option<&str>,
    width: f32,
) -> pango::Layout {
    let (display, segments) = display_segments(block, marker);
    let layout = pango::Layout::new(context);
    layout.set_text(&display);
    layout.set_width(to_pango_units(width));
    layout.set_wrap(pango::WrapMode::WordChar);
    layout.set_alignment(match block.alignment {
        TextAlignment::Left | TextAlignment::Unknown => pango::Alignment::Left,
        TextAlignment::Center => pango::Alignment::Center,
        TextAlignment::Right => pango::Alignment::Right,
    });
    if let Some(spacing) = block.line_spacing {
        layout.set_line_spacing((spacing / 11.0).max(0.5));
    }
    let attributes = pango::AttrList::new();
    apply_style(
        &attributes,
        &block.base_style,
        0,
        u32::try_from(display.len()).unwrap_or(u32::MAX),
    );
    for segment in segments {
        apply_style(&attributes, &segment.style, segment.start, segment.end);
    }
    layout.set_attributes(Some(&attributes));
    layout
}

struct StyleSegment {
    start: u32,
    end: u32,
    style: TextStyle,
}

fn display_segments(block: &TextBlock, marker: Option<&str>) -> (String, Vec<StyleSegment>) {
    let mut display = String::new();
    if let Some(marker) = marker.filter(|marker| !marker.is_empty()) {
        for character in marker.chars() {
            push_display_character(&mut display, character);
        }
        display.push(' ');
    }
    let mut segments = Vec::new();
    let mut utf16_offset = 0_u32;
    let mut run_index = 0_usize;
    let mut active_run = None;
    let mut active_start = u32::try_from(display.len()).unwrap_or(u32::MAX);
    for character in block.text.chars() {
        while block
            .runs
            .get(run_index)
            .is_some_and(|run| utf16_offset >= run.end_utf16)
        {
            run_index += 1;
        }
        let run = block
            .runs
            .get(run_index)
            .filter(|run| utf16_offset >= run.start_utf16);
        let hidden = run.map_or(block.base_style.hidden, |run| run.style.hidden);
        if !hidden {
            let style_index = run.map(|_| run_index);
            if active_run != style_index {
                let end = u32::try_from(display.len()).unwrap_or(u32::MAX);
                if active_start < end {
                    segments.push(StyleSegment {
                        start: active_start,
                        end,
                        style: style_at(block, active_run).clone(),
                    });
                }
                active_run = style_index;
                active_start = end;
            }
            push_display_character(&mut display, character);
        }
        utf16_offset += if character.len_utf16() == 1 { 1 } else { 2 };
    }
    let end = u32::try_from(display.len()).unwrap_or(u32::MAX);
    if active_start < end {
        segments.push(StyleSegment {
            start: active_start,
            end,
            style: style_at(block, active_run).clone(),
        });
    }
    (display, segments)
}

pub(crate) fn glib_text(value: &str) -> Cow<'_, str> {
    if value.contains('\0') {
        Cow::Owned(value.replace('\0', "\u{fffd}"))
    } else {
        Cow::Borrowed(value)
    }
}

fn push_display_character(display: &mut String, character: char) {
    display.push(if character == '\0' {
        REPLACEMENT_CHARACTER
    } else {
        character
    });
}

fn style_at(block: &TextBlock, run: Option<usize>) -> &TextStyle {
    run.and_then(|index| block.runs.get(index))
        .map_or(&block.base_style, |run| &run.style)
}

fn apply_style(attributes: &pango::AttrList, style: &TextStyle, start: u32, end: u32) {
    if start >= end {
        return;
    }
    if let Some(font) = &style.font {
        let font = glib_text(font);
        insert(attributes, pango::AttrString::new_family(&font), start, end);
    }
    if let Some(size) = style.font_size {
        insert(
            attributes,
            pango::AttrSize::new_size_absolute(to_pango_units(size)),
            start,
            end,
        );
    }
    if style.bold {
        insert(
            attributes,
            pango::AttrInt::new_weight(pango::Weight::Bold),
            start,
            end,
        );
    }
    if style.italic {
        insert(
            attributes,
            pango::AttrInt::new_style(pango::Style::Italic),
            start,
            end,
        );
    }
    if style.underline {
        insert(
            attributes,
            pango::AttrInt::new_underline(pango::Underline::Single),
            start,
            end,
        );
    }
    if style.strikethrough {
        insert(
            attributes,
            pango::AttrInt::new_strikethrough(true),
            start,
            end,
        );
    }
    if style.superscript || style.subscript {
        let rise = if style.superscript { 5_000 } else { -3_000 };
        insert(attributes, pango::AttrInt::new_rise(rise), start, end);
    }
    if let Some(color) = style.foreground {
        insert(
            attributes,
            pango::AttrColor::new_foreground(
                color_channel(color.red),
                color_channel(color.green),
                color_channel(color.blue),
            ),
            start,
            end,
        );
    }
    if let Some(color) = style.highlight {
        insert(
            attributes,
            pango::AttrColor::new_background(
                color_channel(color.red),
                color_channel(color.green),
                color_channel(color.blue),
            ),
            start,
            end,
        );
    }
}

fn insert(
    attributes: &pango::AttrList,
    attribute: impl Into<pango::Attribute>,
    start: u32,
    end: u32,
) {
    let mut attribute = attribute.into();
    attribute.set_start_index(start);
    attribute.set_end_index(end);
    attributes.insert(attribute);
}

fn color_channel(value: u8) -> u16 {
    u16::from(value) * 257
}

fn to_pango_units(value: f32) -> i32 {
    let scaled = value * 1_024.0;
    if scaled.is_nan() {
        0
    } else if scaled.is_sign_negative() {
        scaled.round().to_i32().unwrap_or(i32::MIN)
    } else {
        scaled.round().to_i32().unwrap_or(i32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::{display_segments, glib_text, layout};
    use onenote_core::{OneNoteLoader, TextAlignment, TextBlock, TextRun, TextStyle};
    use onenote_render::{SceneBuilder, ScenePrimitive};
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn display_mapping_removes_hidden_utf16_runs() {
        let visible = TextStyle::default();
        let hidden = TextStyle {
            hidden: true,
            ..TextStyle::default()
        };
        let block = TextBlock {
            text: "A😀secret Z".to_owned(),
            base_style: visible.clone(),
            runs: vec![
                TextRun {
                    start_utf16: 0,
                    end_utf16: 3,
                    style: visible.clone(),
                },
                TextRun {
                    start_utf16: 3,
                    end_utf16: 9,
                    style: hidden,
                },
                TextRun {
                    start_utf16: 9,
                    end_utf16: 11,
                    style: visible,
                },
            ],
            alignment: TextAlignment::Left,
            space_before: 0.0,
            space_after: 0.0,
            line_spacing: None,
        };
        let (display, _) = display_segments(&block, Some("•"));
        assert_eq!(display, "• A😀 Z");
    }

    #[test]
    fn display_mapping_replaces_interior_nuls() {
        let block = TextBlock {
            text: "A\0B".to_owned(),
            base_style: TextStyle::default(),
            runs: Vec::new(),
            alignment: TextAlignment::Left,
            space_before: 0.0,
            space_after: 0.0,
            line_spacing: None,
        };

        let (display, _) = display_segments(&block, Some("�\0."));

        assert_eq!(display, "��. A�B");
        assert!(!display.contains('\0'));
        assert_eq!(glib_text("A\0B"), "A�B");
    }

    #[test]
    fn pango_layout_accepts_sanitized_source_strings() {
        let block = TextBlock {
            text: "A\0B".to_owned(),
            base_style: TextStyle {
                font: Some("Bad\0Font".to_owned()),
                ..TextStyle::default()
            },
            runs: Vec::new(),
            alignment: TextAlignment::Left,
            space_before: 0.0,
            space_after: 0.0,
            line_spacing: None,
        };

        let layout = layout(&gtk::pango::Context::new(), &block, Some("�\0."), 200.0);

        assert_eq!(layout.text(), "��. A�B");
    }

    #[test]
    fn private_corpus_text_layouts_are_glib_safe() {
        let Some(root) = private_notebook_root() else {
            return;
        };
        let loaded = OneNoteLoader::default()
            .load(root)
            .expect("private notebook must parse");
        let builder = SceneBuilder::default();
        let cancel = AtomicBool::new(false);
        let context = gtk::pango::Context::new();
        let mut malformed_markers = 0_usize;

        for page in loaded.notebook.pages() {
            let scene = builder.build(page, &cancel).expect("page scene");
            for node in &scene.nodes {
                if let ScenePrimitive::Text { block, marker } = &node.primitive {
                    if marker
                        .as_deref()
                        .is_some_and(|marker| marker.contains('\0'))
                    {
                        malformed_markers += 1;
                    }
                    let layout = layout(&context, block, marker.as_deref(), node.bounds.width);
                    assert!(!layout.text().contains('\0'));
                }
            }
        }

        assert!(
            malformed_markers > 0,
            "private corpus must exercise malformed list-marker strings"
        );
    }

    fn private_notebook_root() -> Option<PathBuf> {
        let corpus = std::env::var_os("ONENOTE_TEST_CORPUS").map_or_else(
            || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../onepkg/Personal.extracted"),
            PathBuf::from,
        );
        let mut roots: Vec<_> = std::fs::read_dir(corpus)
            .ok()?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("onetoc2"))
            })
            .collect();
        roots.sort();
        roots.into_iter().next()
    }
}
