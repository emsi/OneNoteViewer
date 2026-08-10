use onenote_core::Rect;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

/// Options for literal text matching inside one rendered page.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FindOptions {
    /// Preserve letter case while matching.
    pub case_sensitive: bool,
    /// Accept matches only at Unicode word boundaries.
    pub whole_word: bool,
    /// Treat accents and other combining marks as significant.
    pub match_diacritics: bool,
}

/// UTF-8 byte range in the displayed text used to create a match.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FindTextRange {
    pub start_byte: usize,
    pub end_byte: usize,
}

/// One navigable occurrence and its resolved page geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct FindMatch {
    /// One or more rectangles when a text match crosses wrapped lines.
    pub bounds: Vec<Rect>,
}

impl FindMatch {
    pub(crate) fn new(bounds: Vec<Rect>) -> Option<Self> {
        (!bounds.is_empty()).then_some(Self { bounds })
    }

    pub(crate) fn primary_bounds(&self) -> Option<Rect> {
        self.bounds.first().copied()
    }
}

struct NormalizedUnit {
    normalized_start: usize,
    normalized_end: usize,
    source_start: usize,
    source_end: usize,
}

struct NormalizedText {
    text: String,
    units: Vec<NormalizedUnit>,
}

/// Find normalized literal matches and map them back to displayed UTF-8 offsets.
pub fn find_text_ranges(
    text: &str,
    query: &str,
    options: FindOptions,
    limit: usize,
) -> Vec<FindTextRange> {
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }
    let haystack = normalize(text, options);
    let needle = normalize(query, options).text;
    if needle.is_empty() {
        return Vec::new();
    }

    haystack
        .text
        .match_indices(&needle)
        .filter_map(|(start, _)| {
            let end = start + needle.len();
            let first = haystack
                .units
                .iter()
                .find(|unit| unit.normalized_start == start)?;
            let last = haystack
                .units
                .iter()
                .rev()
                .find(|unit| unit.normalized_end == end)?;
            let range = FindTextRange {
                start_byte: first.source_start,
                end_byte: last.source_end,
            };
            (!options.whole_word || has_word_boundaries(text, range)).then_some(range)
        })
        .take(limit)
        .collect()
}

fn normalize(text: &str, options: FindOptions) -> NormalizedText {
    let mut normalized = String::new();
    let mut units: Vec<NormalizedUnit> = Vec::new();
    let mut characters = text.char_indices().peekable();
    while let Some((source_start, character)) = characters.next() {
        let source_end = characters.peek().map_or(text.len(), |(offset, _)| *offset);
        for decomposed in character.to_string().nfd() {
            if !options.match_diacritics && is_combining_mark(decomposed) {
                if let Some(previous) = units.last_mut() {
                    previous.source_end = source_end;
                }
                continue;
            }
            if options.case_sensitive {
                append_normalized_unit(
                    &mut normalized,
                    &mut units,
                    decomposed,
                    source_start,
                    source_end,
                );
            } else {
                for folded in decomposed.to_lowercase() {
                    append_normalized_unit(
                        &mut normalized,
                        &mut units,
                        folded,
                        source_start,
                        source_end,
                    );
                }
            }
        }
    }
    NormalizedText {
        text: normalized,
        units,
    }
}

fn append_normalized_unit(
    normalized: &mut String,
    units: &mut Vec<NormalizedUnit>,
    character: char,
    source_start: usize,
    source_end: usize,
) {
    let normalized_start = normalized.len();
    normalized.push(character);
    units.push(NormalizedUnit {
        normalized_start,
        normalized_end: normalized.len(),
        source_start,
        source_end,
    });
}

fn has_word_boundaries(text: &str, range: FindTextRange) -> bool {
    let before = text[..range.start_byte].chars().next_back();
    let after = text[range.end_byte..].chars().next();
    before.is_none_or(|character| !is_word_character(character))
        && after.is_none_or(|character| !is_word_character(character))
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

#[cfg(test)]
mod tests {
    use super::{find_text_ranges, FindOptions, FindTextRange};

    #[test]
    fn matching_maps_normalized_unicode_back_to_utf8_offsets() {
        let text = "Cafe\u{301} CAFE café";
        assert_eq!(
            find_text_ranges(text, "café", FindOptions::default(), 10),
            vec![
                FindTextRange {
                    start_byte: 0,
                    end_byte: 6,
                },
                FindTextRange {
                    start_byte: 7,
                    end_byte: 11,
                },
                FindTextRange {
                    start_byte: 12,
                    end_byte: 17,
                },
            ]
        );
    }

    #[test]
    fn options_control_case_diacritics_and_whole_words() {
        let exact = FindOptions {
            case_sensitive: true,
            match_diacritics: true,
            whole_word: true,
        };
        assert_eq!(
            find_text_ranges("Résumé résumé résumés", "résumé", exact, 10),
            vec![FindTextRange {
                start_byte: 9,
                end_byte: 17,
            }]
        );
        assert!(find_text_ranges("résumé", "resume", exact, 10).is_empty());
    }

    #[test]
    fn empty_and_combining_mark_only_queries_do_not_match() {
        assert!(find_text_ranges("content", "", FindOptions::default(), 10).is_empty());
        assert!(find_text_ranges("content", "\u{301}", FindOptions::default(), 10).is_empty());
    }
}
