use gtk::pango;
use num_traits::ToPrimitive;
use onenote_core::{TextAlignment, TextBlock, TextStyle};

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
        display.push_str(marker);
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
            display.push(character);
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

fn style_at(block: &TextBlock, run: Option<usize>) -> &TextStyle {
    run.and_then(|index| block.runs.get(index))
        .map_or(&block.base_style, |run| &run.style)
}

fn apply_style(attributes: &pango::AttrList, style: &TextStyle, start: u32, end: u32) {
    if start >= end {
        return;
    }
    if let Some(font) = &style.font {
        insert(attributes, pango::AttrString::new_family(font), start, end);
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
    use super::display_segments;
    use onenote_core::{TextAlignment, TextBlock, TextRun, TextStyle};

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
}
