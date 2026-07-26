//! Handwriting recognition (OCR) property sets.
//!
//! When OneNote for Windows recognizes a page's handwriting, it persists the
//! recognizer's output as a four-level tree of property sets hanging off the
//! page node (via the undocumented `PageRecognizedTextContainer` reference):
//!
//! ```text
//! RecognizedTextRoot  (jcid 0x00020054)   one per page
//! └── RecognizedTextLine  (jcid 0x00020055)   one per recognized line
//!     └── RecognizedTextBlock (jcid 0x00020056)   one per word block
//!         └── RecognizedTextWord  (jcid 0x00020057)   one per word
//! ```
//!
//! The three upper levels are containers that reference their children through
//! `RecognizedTextChildNodes`. The leaf word node carries the actual recognized
//! text and its language.
//!
//! None of these identifiers appear in the published `[MS-ONE]` specification;
//! they were reverse-engineered from sample files.

use crate::errors::Result;
use crate::fsshttpb::data::exguid::ExGuid;
use crate::one::property::object_reference::ObjectReference;
use crate::one::property::{PropertyType, simple};
use crate::onestore::Object;

/// A recognized word leaf node ([`PropertySetId::RecognizedTextWord`]).
///
/// [`PropertySetId::RecognizedTextWord`]: super::PropertySetId::RecognizedTextWord
#[allow(dead_code)]
pub(crate) struct Word {
    /// The recognized text candidates, best guess first.
    pub(crate) alternatives: Vec<String>,
    /// The locale identifier (LCID) the recognizer used, if recorded.
    pub(crate) language_id: Option<u16>,
}

/// Parse the child references of a recognition container node.
///
/// Works for the root, line, and block levels, which all reference their
/// children through `RecognizedTextChildNodes`.
pub(crate) fn parse_children(object: &Object) -> Result<Vec<ExGuid>> {
    Ok(
        ObjectReference::parse_vec(PropertyType::RecognizedTextChildNodes, object)?
            .unwrap_or_default(),
    )
}

/// Read the ExGuid indices of the ink objects a recognized word was derived
/// from.
///
/// The `0x35df` value is a packed array of 20-byte records, each a constant
/// recognizer-batch GUID followed by a little-endian `u32`. That `u32` is the
/// allocation index, within the recognition node's own namespace, of an
/// `InkContainer` or `InkStrokeNode` that contributed to the word (the records
/// alternate container/stroke). Callers resolve the indices against the
/// namespace and keep the ones they need.
pub(crate) fn parse_stroke_reference_indices(object: &Object) -> Result<Vec<u32>> {
    let Some(data) = simple::parse_vec(PropertyType::RecognizedTextStrokeReferences, object)?
    else {
        return Ok(vec![]);
    };

    Ok(data
        .chunks_exact(20)
        .map(|record| u32::from_le_bytes([record[16], record[17], record[18], record[19]]))
        .collect())
}

/// Parse a [`Word`] leaf node.
pub(crate) fn parse_word(object: &Object) -> Result<Word> {
    let alternatives = simple::parse_vec(PropertyType::RecognizedText, object)?
        .map(|data| decode_alternatives(&data))
        .unwrap_or_default();
    let language_id = simple::parse_u16(PropertyType::RecognizedTextLanguageId, object)?;

    Ok(Word {
        alternatives,
        language_id,
    })
}

/// Decode the recognized-text property value.
///
/// The value is a UTF-16 LE, null-separated multi-string holding the recognized
/// candidates (best guess first) terminated by an empty string (`\0\0`). Empty
/// candidates are dropped.
fn decode_alternatives(data: &[u8]) -> Vec<String> {
    data.chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect::<Vec<_>>()
        .split(|&unit| unit == 0)
        .filter(|chunk| !chunk.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::decode_alternatives;

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
    }

    #[test]
    fn decodes_candidates_best_first() {
        let mut data = utf16("World");
        data.extend(utf16("\0world\0Worlds\0Word\0Would\0\0"));

        assert_eq!(
            decode_alternatives(&data),
            vec!["World", "world", "Worlds", "Word", "Would"]
        );
    }

    #[test]
    fn drops_trailing_terminator_and_empties() {
        let data = utf16("Hello\0\0");
        assert_eq!(decode_alternatives(&data), vec!["Hello"]);
    }

    #[test]
    fn empty_value_yields_no_candidates() {
        assert!(decode_alternatives(&[]).is_empty());
    }
}
