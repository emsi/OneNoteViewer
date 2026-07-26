use crate::contents::RichText;
use crate::errors::{ErrorKind, Result};
use crate::fsshttpb::data::exguid::ExGuid;
use crate::one::property::layout_alignment::LayoutAlignment;
use crate::one::property_set::{page_manifest_node, page_metadata, page_node, title_node};
use crate::onenote::ParserContext;
use crate::onenote::ink_recognition::{InkRecognition, parse_ink_recognition};
use crate::onenote::outline::{Outline, parse_outline};
use crate::onenote::page_content::{PageContent, parse_page_content};
use crate::onestore::ObjectSpace;
use time::macros::utc_datetime;

/// A page.
///
/// See [\[MS-ONE\] 1.3.2] and [\[MS-ONE\] 2.2.19].
///
/// [\[MS-ONE\] 1.3.2]: https://docs.microsoft.com/en-us/openspecs/office_file_formats/ms-one/2dd687ac-f36b-4723-b959-4d60c8a90ca9
/// [\[MS-ONE\] 2.2.19]: https://docs.microsoft.com/en-us/openspecs/office_file_formats/ms-one/e381b7c7-b434-43a2-ba23-0d08bafd281a
#[derive(Clone, Debug)]
pub struct Page {
    link_target_id: String,
    title: Option<Title>,
    title_text: Option<String>,
    level: i32,
    created_at: time::UtcDateTime,
    updated_at: Option<time::UtcDateTime>,
    author: Option<String>,
    height: Option<f32>,
    contents: Vec<PageContent>,
    ink_recognition: Option<InkRecognition>,
}

impl Page {
    /// The page's GUID. May be referenced by internal links.
    /// Ref: [ONESTORE 2.2.58](https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-one/34ea5601-f060-4a69-b5f9-5843a1f14098)
    pub fn link_target_id(&self) -> &str {
        &self.link_target_id
    }

    /// The page's title element.
    ///
    /// See [\[MS-ONE\] 2.2.64].
    ///
    /// [\[MS-ONE\] 2.2.64]: https://docs.microsoft.com/en-us/openspecs/office_file_formats/ms-one/00f0b68b-db49-4aea-9ad9-7c8e68e5c95d
    pub fn title(&self) -> Option<&Title> {
        self.title.as_ref()
    }

    /// The time at which the page was created.
    pub fn created_time(&self) -> time::UtcDateTime {
        self.created_at
    }

    /// The page's last-modified time, falling back to the creation time when
    /// the page has never been edited.
    pub fn updated_time(&self) -> time::UtcDateTime {
        self.updated_at.unwrap_or_else(|| self.created_time())
    }

    /// The page's level in the section page tree.
    ///
    /// See [\[MS-ONE\] 2.3.74].
    ///
    /// [\[MS-ONE\] 2.3.74]: https://docs.microsoft.com/en-us/openspecs/office_file_formats/ms-one/a8632c90-e74a-4ef6-8852-707d4c8817cd
    pub fn level(&self) -> i32 {
        self.level
    }

    /// The page's author.
    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    /// The page's height.
    ///
    /// See [\[MS-ONE\] 2.3.7].
    ///
    /// [\[MS-ONE\] 2.3.7]: https://docs.microsoft.com/en-us/openspecs/office_file_formats/ms-one/e5d0b5e0-0702-42af-8299-0e27c895ba7e
    pub fn height(&self) -> Option<f32> {
        self.height
    }

    /// The page contents.
    pub fn contents(&self) -> &[PageContent] {
        &self.contents
    }

    /// The page's handwriting recognition (OCR) result, if present.
    ///
    /// Returns `Some` only when the page's ink has been recognized by OneNote
    /// for Windows; pages without recognized handwriting return `None`.
    pub fn ink_recognition(&self) -> Option<&InkRecognition> {
        self.ink_recognition.as_ref()
    }

    /// The page's title text.
    ///
    /// This is calculated using a heuristic similar to the one OneNote uses.
    pub fn title_text(&self) -> Option<&str> {
        self.title_text.as_deref()
    }
}

/// A page title.
///
/// See [\[MS-ONE\] 2.2.29].
///
/// [\[MS-ONE\] 2.2.29]: https://docs.microsoft.com/en-us/openspecs/office_file_formats/ms-one/08bd4fd5-59fb-4568-9c82-d2d5280eced8

#[derive(Clone, Debug)]
pub struct Title {
    pub(crate) contents: Vec<Outline>,
    pub(crate) offset_horizontal: f32,
    pub(crate) offset_vertical: f32,
    pub(crate) layout_alignment_in_parent: Option<LayoutAlignment>,
    pub(crate) layout_alignment_self: Option<LayoutAlignment>,
}

impl Title {
    /// The title contents.
    pub fn contents(&self) -> &[Outline] {
        &self.contents
    }

    /// The horizontal offset from the page origin in half-inch increments.
    ///
    /// See [\[MS-ONE\] 2.3.18].
    ///
    /// [\[MS-ONE\] 2.3.18]: https://docs.microsoft.com/en-us/openspecs/office_file_formats/ms-one/5fb9e84a-c9e9-4537-ab14-e5512f24669a
    pub fn offset_horizontal(&self) -> f32 {
        self.offset_horizontal
    }

    /// The vertical offset from the page origin in half-inch increments.
    ///
    /// See [\[MS-ONE\] 2.3.19].
    ///
    /// [\[MS-ONE\] 2.3.19]: https://docs.microsoft.com/en-us/openspecs/office_file_formats/ms-one/5c4992ba-1db5-43e9-83dd-7299c562104d
    pub fn offset_vertical(&self) -> f32 {
        self.offset_vertical
    }

    /// The title's alignment relative to the containing outline element (if present).
    ///
    /// See [\[MS-ONE\] 2.3.27].
    ///
    /// [\[MS-ONE\] 2.3.27]: https://docs.microsoft.com/en-us/openspecs/office_file_formats/ms-one/61fa50be-c355-4b8d-ac01-761a2f7f66c0
    pub fn layout_alignment_in_parent(&self) -> Option<LayoutAlignment> {
        self.layout_alignment_in_parent
    }

    /// The title's alignment.
    ///
    /// See [\[MS-ONE\] 2.3.33].
    ///
    /// [\[MS-ONE\] 2.3.33]: https://docs.microsoft.com/en-us/openspecs/office_file_formats/ms-one/4e7fe9db-2fdb-4239-b291-dc4b909c94ad
    pub fn layout_alignment_self(&self) -> Option<LayoutAlignment> {
        self.layout_alignment_self
    }
}

pub(crate) fn parse_page(
    page_space: &(impl ObjectSpace + ?Sized),
    ctx: &mut ParserContext,
) -> Result<Page> {
    let metadata = parse_metadata(page_space)?;
    let manifest = parse_manifest(page_space)?;

    let data = parse_data(manifest, page_space)?;

    let title = data
        .title
        .map(|id| parse_title(id, page_space, ctx))
        .transpose()?;

    let level = metadata.page_level;

    let title_text = title
        .as_ref()
        .and_then(|title| title.contents.first())
        .and_then(outline_text);

    ctx.page = Some((
        metadata.entity_guid.0,
        title_text
            .clone()
            .unwrap_or_else(|| "<Unknown title>".to_owned()),
    ));

    // Parse recognition before content: it populates `ctx.recognized_words`,
    // which `parse_page_content` reads to attach each stroke's word.
    let ink_recognition = data
        .recognized_text
        .map(|root_id| parse_ink_recognition(root_id, page_space, ctx))
        .transpose()?
        .flatten();

    let contents: Vec<PageContent> = data
        .content
        .into_iter()
        .map(|content_id| parse_page_content(content_id, page_space, ctx))
        .collect::<Result<_>>()?;

    ctx.recognized_words.clear();

    let link_target_id = format!("{}", metadata.entity_guid);

    ctx.page = None;

    let created_at = metadata.created_at.try_into().unwrap_or_else(|err| {
        // FILETIME values outside the representable range fall back to the
        // UNIX epoch. `last_modified` is less precise on disk so this only
        // matters for `created_at`.
        warn!(ctx, "invalid page creation timestamp: {err}");
        utc_datetime!(1970-01-01 0:00)
    });
    let updated_at = data.last_modified.map(|time| time.into());

    Ok(Page {
        link_target_id,
        title,
        title_text: title_text.as_deref().map(remove_hyperlink).or_else(|| {
            contents
                .iter()
                .filter_map(|page_content| page_content.outline())
                .filter_map(outline_text)
                .next()
        }),
        level,
        created_at,
        updated_at,
        author: data.author.map(|author| author.into_value()),
        height: data.page_height,
        contents,
        ink_recognition,
    })
}

fn parse_title(
    title_id: ExGuid,
    space: &(impl ObjectSpace + ?Sized),
    ctx: &mut ParserContext,
) -> Result<Title> {
    let title_object = space
        .get_object(title_id)
        .ok_or_else(|| ErrorKind::MalformedOneNoteData("title object is missing".into()))?;
    let title = title_node::parse(title_object)?;
    let contents = title
        .children
        .into_iter()
        .map(|outline_id| parse_outline(outline_id, space, ctx))
        .collect::<Result<_>>()?;

    Ok(Title {
        contents,
        offset_horizontal: title.offset_horizontal,
        offset_vertical: title.offset_vertical,
        layout_alignment_in_parent: title.layout_alignment_in_parent,
        layout_alignment_self: title.layout_alignment_self,
    })
}

fn parse_data(
    manifest: page_manifest_node::Data,
    space: &(impl ObjectSpace + ?Sized),
) -> Result<page_node::Data> {
    let page_id = manifest.page;
    let page_object = space
        .get_object(page_id)
        .ok_or_else(|| ErrorKind::MalformedOneNoteData("page object is missing".into()))?;

    page_node::parse(page_object)
}

fn parse_manifest(space: &(impl ObjectSpace + ?Sized)) -> Result<page_manifest_node::Data> {
    let page_manifest_id = space
        .content_root()
        .ok_or_else(|| ErrorKind::MalformedOneNoteData("page content id is missing".into()))?;
    let page_manifest_object = space
        .get_object(page_manifest_id)
        .ok_or_else(|| ErrorKind::MalformedOneNoteData("page object is missing".into()))?;

    page_manifest_node::parse(page_manifest_object)
}

fn parse_metadata(space: &(impl ObjectSpace + ?Sized)) -> Result<page_metadata::Data> {
    let metadata_id = space
        .metadata_root()
        .ok_or_else(|| ErrorKind::MalformedOneNoteData("page metadata id is missing".into()))?;
    let metadata_object = space
        .get_object(metadata_id)
        .ok_or_else(|| ErrorKind::MalformedOneNoteData("page metadata object is missing".into()))?;

    page_metadata::parse(metadata_object)
}

fn outline_text(outline: &Outline) -> Option<String> {
    let text = outline
        .items
        .first()?
        .element()?
        .contents
        .first()?
        .rich_text()?;

    let visible = visible_text(text);
    Some(visible).filter(|s| !s.is_empty())
}

/// Concatenate text runs that are not Hidden (per [MS-ONE] §2.3.76).
/// `text_run_indices` are UTF-16 code unit positions where each run ends;
/// the last run extends to the end of the paragraph text.
fn visible_text(rt: &RichText) -> String {
    let formatting = rt.text_run_formatting();
    if formatting.is_empty() {
        return rt.text().to_owned();
    }

    let utf16: Vec<u16> = rt.text().encode_utf16().collect();
    let indices = rt.text_run_indices();

    // Build run boundaries: [0, idx_0, idx_1, ..., utf16.len()]
    let mut bounds: Vec<usize> = std::iter::once(0)
        .chain(indices.iter().map(|&i| i as usize))
        .collect();
    if bounds.len() == formatting.len() {
        bounds.push(utf16.len());
    }

    let mut out: Vec<u16> = Vec::with_capacity(utf16.len());
    for (i, style) in formatting.iter().enumerate() {
        if style.hidden() {
            continue;
        }
        let start = bounds[i].min(utf16.len());
        let end = bounds
            .get(i + 1)
            .copied()
            .unwrap_or(utf16.len())
            .min(utf16.len());
        if start < end {
            out.extend_from_slice(&utf16[start..end]);
        }
    }

    String::from_utf16_lossy(&out)
}

fn remove_hyperlink(title: &str) -> String {
    const HYPERLINK_MARKER: &str = "\u{fddf}HYPERLINK \"";

    let mut clean_title = title.to_string();

    // Find the first hyperlink mark
    while let Some(marker_start) = clean_title.find(HYPERLINK_MARKER) {
        let hyperlink_part = &clean_title[marker_start + HYPERLINK_MARKER.len()..];

        // Find the closing double quote of the hyperlink
        if let Some(quote_end) = hyperlink_part.find('"') {
            let before_hyperlink = &clean_title[..marker_start];
            let after_hyperlink = &hyperlink_part[quote_end + 1..];
            clean_title = format!("{}{}", before_hyperlink, after_hyperlink);
        } else {
            // Sometimes links are broken, in these cases we only consider what is before the mark
            clean_title = title[..marker_start].to_string();
        }
    }

    clean_title
}
