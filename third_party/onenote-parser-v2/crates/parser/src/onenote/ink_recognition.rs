use crate::errors::Result;
use crate::fsshttpb::data::exguid::ExGuid;
use crate::one::property_set::PropertySetId;
use crate::one::property_set::recognized_text_node;
use crate::onenote::ParserContext;
use crate::onestore::ObjectSpace;

/// The handwriting recognition (OCR) result for a [`Page`].
///
/// When handwriting is recognized by OneNote for Windows, the recognizer's
/// output is stored in the file as lines ([`InkRecognizedLine`]) of words.
///
/// Lines, and the words within them, are returned in the order OneNote's
/// recognizer recorded — its own reading-order analysis (roughly top-to-bottom,
/// and within a line following the word's language, e.g. left-to-right for
/// `1033`). That order is passed through as-is; this crate does not re-derive or
/// verify it.
///
/// Recognition data is only present for ink that OneNote for Windows has
/// actually recognized; handwriting created or only ever opened in other
/// OneNote clients has no recognition data and [`Page::ink_recognition`]
/// returns `None`.
///
/// [`Page`]: crate::page::Page
/// [`Page::ink_recognition`]: crate::page::Page::ink_recognition
#[derive(Clone, Debug)]
pub struct InkRecognition {
    pub(crate) lines: Vec<InkRecognizedLine>,
}

impl InkRecognition {
    /// The recognized lines, in the recognizer's order (see [`InkRecognition`]).
    pub fn lines(&self) -> &[InkRecognizedLine] {
        &self.lines
    }

    /// The recognized text as a single string.
    ///
    /// Words are joined with spaces and lines with newlines — in the
    /// recognizer's order (see [`InkRecognition`]) — using each word's best
    /// (first) candidate. This is a convenience over walking [`lines`]; callers
    /// that need the alternative candidates should use that instead.
    ///
    /// [`lines`]: InkRecognition::lines
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|line| {
                line.words
                    .iter()
                    .filter_map(|word| word.text())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// A single recognized line of handwriting.
#[derive(Clone, Debug)]
pub struct InkRecognizedLine {
    pub(crate) words: Vec<InkRecognizedWord>,
}

impl InkRecognizedLine {
    /// The recognized words, in the recognizer's order (see [`InkRecognition`]).
    pub fn words(&self) -> &[InkRecognizedWord] {
        &self.words
    }
}

/// A single recognized word together with the recognizer's alternative guesses.
#[derive(Clone, Debug)]
pub struct InkRecognizedWord {
    pub(crate) id: u32,
    pub(crate) alternatives: Vec<String>,
    pub(crate) language_id: Option<u16>,
}

impl InkRecognizedWord {
    /// A stable identity for this recognized word, unique within the page's
    /// recognition tree.
    ///
    /// Each stroke that contributed to a recognized word exposes that word via
    /// [`InkStroke::recognized_word`]. A word spans multiple strokes, so
    /// iterating strokes encounters the same word once per contributing
    /// stroke; `id()` is the deduplication key. Every stroke that belongs to
    /// the same word reports an equal id, matching the id on the page-level
    /// [`Page::ink_recognition`] entry for that word. The value is unique
    /// within a single page's recognition tree; it is opaque otherwise and
    /// **must not** be compared across pages.
    ///
    /// [`InkStroke::recognized_word`]: crate::contents::InkStroke::recognized_word
    /// [`Page::ink_recognition`]: crate::page::Page::ink_recognition
    pub fn id(&self) -> u32 {
        self.id
    }

    /// The recognizer's best guess for this word, if any.
    ///
    /// This is the first of [`alternatives`](InkRecognizedWord::alternatives).
    pub fn text(&self) -> Option<&str> {
        self.alternatives.first().map(String::as_str)
    }

    /// All recognition candidates for this word, best guess first.
    pub fn alternatives(&self) -> &[String] {
        &self.alternatives
    }

    /// The locale identifier (LCID) the recognizer used for this word.
    ///
    /// For example `1031` is German (de-DE) and `1033` is English (en-US).
    pub fn language_id(&self) -> Option<u16> {
        self.language_id
    }
}

// Guard against cycles in a malformed recognition tree (e.g. a node declared to
// contain itself). The real tree is only four levels deep, so this cap is
// generous while still turning a cycle into a bounded walk instead of a stack
// overflow.
const MAX_RECOGNITION_DEPTH: u32 = 8;

pub(crate) fn parse_ink_recognition(
    root_id: ExGuid,
    space: &(impl ObjectSpace + ?Sized),
    ctx: &mut ParserContext,
) -> Result<Option<InkRecognition>> {
    let Some(root) = space.get_object(root_id) else {
        warn!(ctx, "recognized text root {:?} is missing", root_id);
        return Ok(None);
    };

    if root.id() != PropertySetId::RecognizedTextRoot.as_jcid() {
        warn!(
            ctx,
            "page recognized-text reference {:?} is not a recognition root (got {:?})",
            root_id,
            root.id()
        );
        return Ok(None);
    }

    let mut lines = Vec::new();
    for line_id in recognized_text_node::parse_children(root)? {
        let mut words = Vec::new();
        collect_words(line_id, space, ctx, &mut words, 0)?;
        if !words.is_empty() {
            lines.push(InkRecognizedLine { words });
        }
    }

    if lines.is_empty() {
        return Ok(None);
    }

    Ok(Some(InkRecognition { lines }))
}

/// Collect the recognized words beneath `id` into `out`.
///
/// A line groups its words through one or more intermediate block nodes, so we
/// descend through any container level until we reach the word leaves.
fn collect_words(
    id: ExGuid,
    space: &(impl ObjectSpace + ?Sized),
    ctx: &mut ParserContext,
    out: &mut Vec<InkRecognizedWord>,
    depth: u32,
) -> Result<()> {
    if depth > MAX_RECOGNITION_DEPTH {
        warn!(ctx, "maximum recognized-text nesting depth exceeded");
        return Ok(());
    }

    let Some(object) = space.get_object(id) else {
        warn!(ctx, "recognized text node {:?} is missing", id);
        return Ok(());
    };

    if object.id() == PropertySetId::RecognizedTextWord.as_jcid() {
        let word = recognized_text_node::parse_word(object)?;
        if !word.alternatives.is_empty() {
            let recognized = InkRecognizedWord {
                id: id.value,
                alternatives: word.alternatives,
                language_id: word.language_id,
            };

            // Record the stroke→word link so `InkStroke::recognized_word` can
            // be filled when page content is parsed.
            for index in recognized_text_node::parse_stroke_reference_indices(object)? {
                let stroke_id = ExGuid::from_guid(id.guid, index);
                let is_stroke = space
                    .get_object(stroke_id)
                    .is_some_and(|o| o.id() == PropertySetId::InkStrokeNode.as_jcid());
                if is_stroke {
                    ctx.recognized_words.insert(stroke_id, recognized.clone());
                }
            }

            out.push(recognized);
        }
        return Ok(());
    }

    for child_id in recognized_text_node::parse_children(object)? {
        collect_words(child_id, space, ctx, out, depth + 1)?;
    }

    Ok(())
}
