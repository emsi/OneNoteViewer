use crate::math::{decode_span, MathSegment};
use crate::model::{
    Attachment, Color, Diagnostic, DiagnosticSeverity, ElementContent, Image, Ink, InkPoint,
    InkStroke, ListMarker, ListMarkerPart, ListNumberFormat, Notebook, NotebookEntry, ObjectId,
    ObjectKind, Outline, OutlineElement, Page, PageId, PageObject, PageObjectRole, Rect,
    ResourceId, ResourceRef, Section, SectionGroup, SectionId, SourceFingerprint, SourceId, Table,
    TableCell, TextAlignment, TextBlock, TextLink, TextLinkOrigin, TextRun, TextStyle,
};
use crate::resource::{resource_status, ResourceLoader};
use crate::{Error, ResourceStore, Result, PIXELS_PER_HALF_INCH};
use linkify::{LinkFinder, LinkKind};
use onenote_parser::contents::{
    Content, EmbeddedFile, Image as ParserImage, Ink as ParserInk, List, Outline as ParserOutline,
    OutlineElement as ParserOutlineElement, OutlineItem, ParagraphStyling, RichText,
    Table as ParserTable,
};
use onenote_parser::notebook::Notebook as ParserNotebook;
use onenote_parser::page::{Page as ParserPage, PageContent};
use onenote_parser::property::common::{Color as ParserColor, ColorRef};
use onenote_parser::property::embedded_file::FileType;
use onenote_parser::property::rich_text::ParagraphAlignment;
use onenote_parser::section::{
    Section as ParserSection, SectionEntry as ParserSectionEntry,
    SectionGroup as ParserSectionGroup,
};
use onenote_parser::warn::Report;
use onenote_parser::Parser;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use typed_path::{PathType, TypedPath};
use uuid::Uuid;

/// Defensive projection limits applied after native format parsing.
#[derive(Clone, Copy, Debug)]
pub struct ParseLimits {
    /// Maximum sections in one notebook tree.
    pub max_sections: usize,
    /// Maximum pages across one source.
    pub max_pages: usize,
    /// Maximum top-level objects across one source.
    pub max_objects: usize,
    /// Maximum retained binary resources across one source.
    pub max_resources: usize,
    /// Maximum ink points retained in one ink object.
    pub max_ink_points_per_object: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_sections: 10_000,
            max_pages: 1_000_000,
            max_objects: 2_000_000,
            max_resources: 1_000_000,
            max_ink_points_per_object: 10_000_000,
        }
    }
}

/// A parsed semantic notebook plus separately owned lazy binary payloads.
#[derive(Clone, Debug)]
pub struct LoadedNotebook {
    /// Serializable, UI-neutral notebook model.
    pub notebook: Notebook,
    /// Lazy image and attachment payloads.
    pub resources: ResourceStore,
}

/// Read-only loader for native `.one` and `.onetoc2` sources.
#[derive(Clone, Copy, Debug, Default)]
pub struct OneNoteLoader {
    limits: ParseLimits,
}

impl OneNoteLoader {
    /// Construct a loader with explicit defensive projection limits.
    pub fn with_limits(limits: ParseLimits) -> Self {
        Self { limits }
    }

    /// Load either a complete `.onetoc2` notebook or one standalone `.one`
    /// section. The source is canonicalized but never modified.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the source cannot be opened, an unsupported
    /// source error for other file types, or a parse error for malformed data
    /// and exceeded defensive limits.
    pub fn load(&self, path: impl AsRef<Path>) -> Result<LoadedNotebook> {
        let requested = path.as_ref();
        let canonical = std::fs::canonicalize(requested).map_err(|source| Error::Io {
            path: requested.to_path_buf(),
            source,
        })?;
        let extension = canonical
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);

        match extension.as_deref() {
            Some("onetoc2") => self.load_notebook(&canonical),
            Some("one") => self.load_section(&canonical),
            _ => Err(Error::UnsupportedSource { path: canonical }),
        }
    }

    fn load_notebook(&self, path: &Path) -> Result<LoadedNotebook> {
        let fingerprint = source_fingerprint(path)?;
        let parsed = Parser::new()
            .parse_notebook(host_typed_path(path))
            .map_err(|error| Error::Parse {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?;
        ensure_source_unchanged(path, &fingerprint)?;
        Projector::new(path, fingerprint, self.limits).notebook(&parsed)
    }

    fn load_section(&self, path: &Path) -> Result<LoadedNotebook> {
        let fingerprint = source_fingerprint(path)?;
        let parsed = Parser::new()
            .parse_section(host_typed_path(path))
            .map_err(|error| Error::Parse {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?;
        ensure_source_unchanged(path, &fingerprint)?;
        Projector::new(path, fingerprint, self.limits).standalone_section(&parsed)
    }
}

struct Projector {
    source_id: SourceId,
    fingerprint: SourceFingerprint,
    source_path: PathBuf,
    limits: ParseLimits,
    section_count: usize,
    page_count: usize,
    object_count: usize,
    resources: ResourceStore,
}

impl Projector {
    fn new(path: &Path, fingerprint: SourceFingerprint, limits: ParseLimits) -> Self {
        let path_bytes = path_identity(path);
        let source_id = SourceId::new(stable_id(&[b"source", &path_bytes]));
        Self {
            source_id,
            fingerprint,
            source_path: path.to_path_buf(),
            limits,
            section_count: 0,
            page_count: 0,
            object_count: 0,
            resources: ResourceStore::default(),
        }
    }

    fn notebook(mut self, parsed: &ParserNotebook) -> Result<LoadedNotebook> {
        let mut diagnostics = report_diagnostics(parsed.report(), &self.source_id);
        let entries = self.entries(parsed.entries(), "root", &mut diagnostics)?;
        let notebook = Notebook {
            source_id: self.source_id.clone(),
            fingerprint: self.fingerprint.clone(),
            name: notebook_name(&self.source_path),
            color: parsed.color().map(project_color),
            entries,
            diagnostics,
        };
        Ok(LoadedNotebook {
            notebook,
            resources: self.resources,
        })
    }

    fn standalone_section(mut self, parsed: &ParserSection) -> Result<LoadedNotebook> {
        let name = parsed.display_name().to_owned();
        let section = self.section(parsed, "standalone")?;
        Ok(LoadedNotebook {
            notebook: Notebook {
                source_id: self.source_id.clone(),
                fingerprint: self.fingerprint.clone(),
                name,
                color: section.color,
                entries: vec![NotebookEntry::Section(section)],
                diagnostics: Vec::new(),
            },
            resources: self.resources,
        })
    }

    fn entries(
        &mut self,
        entries: &[ParserSectionEntry],
        parent_key: &str,
        notebook_diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Vec<NotebookEntry>> {
        entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let key = format!("{parent_key}/{index}");
                match entry {
                    ParserSectionEntry::Section(section) => {
                        self.section(section, &key).map(NotebookEntry::Section)
                    }
                    ParserSectionEntry::SectionGroup(group) => self
                        .group(group, &key, notebook_diagnostics)
                        .map(NotebookEntry::Group),
                }
            })
            .collect()
    }

    fn group(
        &mut self,
        group: &ParserSectionGroup,
        key: &str,
        notebook_diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<SectionGroup> {
        Ok(SectionGroup {
            id: SectionId::new(self.id("group", key)),
            name: group.display_name().to_owned(),
            entries: self.entries(group.entries(), key, notebook_diagnostics)?,
        })
    }

    fn section(&mut self, section: &ParserSection, key: &str) -> Result<Section> {
        self.section_count += 1;
        self.enforce(
            self.section_count <= self.limits.max_sections,
            "section limit exceeded",
        )?;
        let id = SectionId::new(self.id("section", key));
        let mut pages = Vec::new();
        for (series_index, series) in section.page_series().iter().enumerate() {
            for (page_index, page) in series.pages().iter().enumerate() {
                let page_key = format!("{key}/{series_index}/{page_index}");
                pages.push(self.page(page, &page_key)?);
            }
        }
        Ok(Section {
            id,
            name: section.display_name().to_owned(),
            color: section.color().map(project_color),
            pages,
            diagnostics: report_diagnostics(section.report(), &self.source_id),
        })
    }

    fn page(&mut self, page: &ParserPage, key: &str) -> Result<Page> {
        self.page_count += 1;
        self.enforce(
            self.page_count <= self.limits.max_pages,
            "page limit exceeded",
        )?;
        let page_id = PageId::new(self.id("page", page.link_target_id()));
        let mut objects = Vec::new();

        if let Some(title) = page.title() {
            for (index, outline) in title.contents().iter().enumerate() {
                let object_key = format!("{key}/title/{index}");
                let fallback = (
                    half_inches(title.offset_horizontal()),
                    half_inches(title.offset_vertical()),
                );
                let mut object = self.outline_object(
                    outline,
                    &page_id,
                    &object_key,
                    objects.len(),
                    Some(fallback),
                )?;
                object.role = PageObjectRole::Title;
                objects.push(object);
            }
        }

        for (index, content) in page.contents().iter().enumerate() {
            let object_key = format!("{key}/content/{index}");
            let object = match content {
                PageContent::Outline(outline) => {
                    self.outline_object(outline, &page_id, &object_key, objects.len(), None)?
                }
                PageContent::Image(image) => {
                    self.image_object(image, &page_id, &object_key, objects.len())?
                }
                PageContent::EmbeddedFile(file) => {
                    self.attachment_object(file, &page_id, &object_key, objects.len())?
                }
                PageContent::Ink(ink) => {
                    self.ink_object(ink, &page_id, &object_key, objects.len())?
                }
                PageContent::Unknown => {
                    self.unknown_object(&page_id, &object_key, objects.len())?
                }
            };
            objects.push(object);
        }

        Ok(Page {
            id: page_id,
            native_id: page.link_target_id().to_owned(),
            title: page.title_text().unwrap_or("Untitled page").to_owned(),
            level: page.level(),
            created_at: page.created_time().to_string(),
            updated_at: page.updated_time().to_string(),
            author: page.author().map(str::to_owned),
            height: page.height().map(half_inches),
            objects,
        })
    }

    fn outline_object(
        &mut self,
        outline: &ParserOutline,
        page_id: &PageId,
        key: &str,
        z_index: usize,
        fallback_origin: Option<(f32, f32)>,
    ) -> Result<PageObject> {
        self.bump_object()?;
        let x = outline
            .offset_horizontal()
            .map(half_inches)
            .or_else(|| fallback_origin.map(|origin| origin.0))
            .unwrap_or(0.0);
        let y = outline
            .offset_vertical()
            .map(half_inches)
            .or_else(|| fallback_origin.map(|origin| origin.1))
            .unwrap_or(0.0);
        let width = outline
            .layout_max_width()
            .or_else(|| outline.layout_reserved_width())
            .or_else(|| outline.layout_minimum_outline_width())
            .map_or(480.0, half_inches)
            .max(1.0);
        let estimated_lines = f32::from(
            u16::try_from(count_outline_elements(outline.items()).max(1)).unwrap_or(u16::MAX),
        );
        let height = outline
            .layout_max_height()
            .map_or(estimated_lines * 24.0, half_inches)
            .max(1.0);
        Ok(PageObject {
            id: ObjectId::new(self.id("object", &format!("{page_id}/{key}"))),
            role: PageObjectRole::Body,
            bounds: Rect {
                x,
                y,
                width,
                height,
            },
            z_index: u32::try_from(z_index).unwrap_or(u32::MAX),
            kind: ObjectKind::Outline(self.outline(outline, key)?),
        })
    }

    fn outline(&mut self, outline: &ParserOutline, key: &str) -> Result<Outline> {
        Ok(Outline {
            child_level: outline.child_level(),
            indents: outline.indents().iter().copied().map(half_inches).collect(),
            user_sized: outline.is_layout_size_set_by_user(),
            elements: self.outline_items(outline.items(), key, outline.child_level())?,
        })
    }

    fn outline_items(
        &mut self,
        items: &[OutlineItem],
        key: &str,
        level: u8,
    ) -> Result<Vec<OutlineElement>> {
        let mut elements = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let item_key = format!("{key}/{index}");
            match item {
                OutlineItem::Element(element) => {
                    elements.push(self.outline_element(element, &item_key, level)?);
                }
                OutlineItem::Group(group) => {
                    elements.extend(self.outline_items(
                        group.outlines(),
                        &item_key,
                        level.saturating_add(group.child_level()),
                    )?);
                }
            }
        }
        Ok(elements)
    }

    fn outline_element(
        &mut self,
        element: &ParserOutlineElement,
        key: &str,
        level: u8,
    ) -> Result<OutlineElement> {
        let content = element
            .contents()
            .iter()
            .enumerate()
            .map(|(index, content)| self.element_content(content, &format!("{key}/{index}")))
            .collect::<Result<Vec<_>>>()?;
        let list = element.list_contents().first().map(project_list);
        Ok(OutlineElement {
            level,
            list,
            content,
            children: self.outline_items(
                element.children(),
                &format!("{key}/children"),
                level.saturating_add(element.child_level()),
            )?,
        })
    }

    fn element_content(&mut self, content: &Content, key: &str) -> Result<ElementContent> {
        match content {
            Content::RichText(text) => Ok(ElementContent::Text(project_text(text))),
            Content::Table(table) => Ok(ElementContent::Table(self.table(table, key)?)),
            Content::Image(image) => Ok(ElementContent::Image(self.image(image, key)?)),
            Content::EmbeddedFile(file) => {
                Ok(ElementContent::Attachment(self.attachment(file, key)?))
            }
            Content::Ink(ink) => Ok(ElementContent::Ink(self.ink(ink)?)),
            Content::Unknown => Ok(ElementContent::Unknown),
        }
    }

    fn table(&mut self, table: &ParserTable, key: &str) -> Result<Table> {
        let rows = table
            .contents()
            .iter()
            .enumerate()
            .map(|(row_index, row)| {
                row.contents()
                    .iter()
                    .enumerate()
                    .map(|(cell_index, cell)| {
                        let cell_key = format!("{key}/{row_index}/{cell_index}");
                        let elements = cell
                            .contents()
                            .iter()
                            .enumerate()
                            .map(|(index, element)| {
                                self.outline_element(element, &format!("{cell_key}/{index}"), 0)
                            })
                            .collect::<Result<Vec<_>>>()?;
                        Ok(TableCell {
                            background: cell.background_color().map(project_color),
                            max_width: cell.layout_max_width().map(half_inches),
                            elements,
                        })
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?;
        let locked_columns = (0..table.cols())
            .map(|column| {
                let byte_index = usize::try_from(column / 8).unwrap_or(usize::MAX);
                let mask = 1_u8 << (column % 8);
                table
                    .cols_locked()
                    .get(byte_index)
                    .is_some_and(|byte| byte & mask != 0)
            })
            .collect();
        Ok(Table {
            rows,
            column_widths: table
                .col_widths()
                .iter()
                .copied()
                .map(half_inches)
                .collect(),
            locked_columns,
            borders_visible: table.borders_visible(),
        })
    }

    fn image_object(
        &mut self,
        image: &ParserImage,
        page_id: &PageId,
        key: &str,
        z_index: usize,
    ) -> Result<PageObject> {
        self.bump_object()?;
        let projected = self.image(image, key)?;
        Ok(PageObject {
            id: ObjectId::new(self.id("object", &format!("{page_id}/{key}"))),
            role: PageObjectRole::Body,
            bounds: Rect {
                x: image.offset_horizontal().map_or(0.0, half_inches),
                y: image.offset_vertical().map_or(0.0, half_inches),
                width: projected.width.unwrap_or(240.0).max(1.0),
                height: projected.height.unwrap_or(180.0).max(1.0),
            },
            z_index: u32::try_from(z_index).unwrap_or(u32::MAX),
            kind: ObjectKind::Image(projected),
        })
    }

    fn image(&mut self, image: &ParserImage, key: &str) -> Result<Image> {
        self.enforce(
            self.resources.len() < self.limits.max_resources,
            "resource limit exceeded",
        )?;
        let id = ResourceId::new(self.id("resource", key));
        let extension = image.extension().unwrap_or("bin").trim_start_matches('.');
        let name = image
            .image_filename()
            .map_or_else(|| format!("image.{extension}"), str::to_owned);
        let resource = ResourceRef {
            id: id.clone(),
            name,
            media_type: image_media_type(extension).to_owned(),
            size: image.size().unwrap_or(0),
            status: resource_status(image.data_status()),
        };
        self.resources
            .insert(id, ResourceLoader::Image(image.clone()));
        Ok(Image {
            resource,
            width: image
                .layout_max_width()
                .or_else(|| image.picture_width())
                .map(half_inches),
            height: image
                .layout_max_height()
                .or_else(|| image.picture_height())
                .map(half_inches),
            alt_text: image.alt_text().map(str::to_owned),
            search_text: image.text().map(str::to_owned),
            hyperlink: image.hyperlink_url().map(str::to_owned),
            is_background: image.is_background(),
        })
    }

    fn attachment_object(
        &mut self,
        file: &EmbeddedFile,
        page_id: &PageId,
        key: &str,
        z_index: usize,
    ) -> Result<PageObject> {
        self.bump_object()?;
        let projected = self.attachment(file, key)?;
        Ok(PageObject {
            id: ObjectId::new(self.id("object", &format!("{page_id}/{key}"))),
            role: PageObjectRole::Body,
            bounds: Rect {
                x: file.offset_horizontal().map_or(0.0, half_inches),
                y: file.offset_vertical().map_or(0.0, half_inches),
                width: projected.width.unwrap_or(180.0).max(1.0),
                height: projected.height.unwrap_or(48.0).max(1.0),
            },
            z_index: u32::try_from(z_index).unwrap_or(u32::MAX),
            kind: ObjectKind::Attachment(projected),
        })
    }

    fn attachment(&mut self, file: &EmbeddedFile, key: &str) -> Result<Attachment> {
        self.enforce(
            self.resources.len() < self.limits.max_resources,
            "resource limit exceeded",
        )?;
        let id = ResourceId::new(self.id("resource", key));
        let media_type = match file.file_type() {
            FileType::Audio => "audio/*",
            FileType::Video => "video/*",
            FileType::Unknown => "application/octet-stream",
        };
        let resource = ResourceRef {
            id: id.clone(),
            name: file.filename().to_owned(),
            media_type: media_type.to_owned(),
            size: file.size(),
            status: resource_status(file.data_status()),
        };
        self.resources
            .insert(id, ResourceLoader::Attachment(file.clone()));
        Ok(Attachment {
            resource,
            width: file.layout_max_width().map(half_inches),
            height: file.layout_max_height().map(half_inches),
        })
    }

    fn ink_object(
        &mut self,
        ink: &ParserInk,
        page_id: &PageId,
        key: &str,
        z_index: usize,
    ) -> Result<PageObject> {
        self.bump_object()?;
        let bounds = ink.bounding_box().map_or(
            Rect {
                x: ink.offset_horizontal().map_or(0.0, half_inches),
                y: ink.offset_vertical().map_or(0.0, half_inches),
                width: 1.0,
                height: 1.0,
            },
            |bounds| Rect {
                x: ink
                    .offset_horizontal()
                    .map_or_else(|| finite(bounds.x()), half_inches),
                y: ink
                    .offset_vertical()
                    .map_or_else(|| finite(bounds.y()), half_inches),
                width: finite(bounds.width()).max(1.0),
                height: finite(bounds.height()).max(1.0),
            },
        );
        Ok(PageObject {
            id: ObjectId::new(self.id("object", &format!("{page_id}/{key}"))),
            role: PageObjectRole::Body,
            bounds,
            z_index: u32::try_from(z_index).unwrap_or(u32::MAX),
            kind: ObjectKind::Ink(self.ink(ink)?),
        })
    }

    fn ink(&self, ink: &ParserInk) -> Result<Ink> {
        let mut strokes = Vec::new();
        let mut recognized = Vec::new();
        let mut point_count = 0_usize;
        append_ink(
            ink,
            &mut strokes,
            &mut recognized,
            &mut point_count,
            self.limits.max_ink_points_per_object,
            &self.source_path,
        )?;
        recognized.sort();
        recognized.dedup();
        Ok(Ink {
            strokes,
            recognized_text: (!recognized.is_empty()).then(|| recognized.join(" ")),
        })
    }

    fn unknown_object(
        &mut self,
        page_id: &PageId,
        key: &str,
        z_index: usize,
    ) -> Result<PageObject> {
        self.bump_object()?;
        Ok(PageObject {
            id: ObjectId::new(self.id("object", &format!("{page_id}/{key}"))),
            role: PageObjectRole::Body,
            bounds: Rect {
                width: 160.0,
                height: 48.0,
                ..Rect::default()
            },
            z_index: u32::try_from(z_index).unwrap_or(u32::MAX),
            kind: ObjectKind::Unknown,
        })
    }

    fn bump_object(&mut self) -> Result<()> {
        self.object_count += 1;
        self.enforce(
            self.object_count <= self.limits.max_objects,
            "page object limit exceeded",
        )
    }

    fn enforce(&self, condition: bool, message: &str) -> Result<()> {
        if condition {
            Ok(())
        } else {
            Err(Error::Parse {
                path: self.source_path.clone(),
                message: message.to_owned(),
            })
        }
    }

    fn id(&self, kind: &str, key: &str) -> String {
        stable_id(&[
            self.source_id.as_str().as_bytes(),
            kind.as_bytes(),
            key.as_bytes(),
        ])
    }
}

fn project_text(text: &RichText) -> TextBlock {
    let base_style = project_text_style(text.paragraph_style());
    let total = u32::try_from(text.text().encode_utf16().count()).unwrap_or(u32::MAX);
    let projected_text = normalize_rich_text_controls(text.text());
    debug_assert_eq!(
        text.text().encode_utf16().count(),
        projected_text.encode_utf16().count(),
        "rich-text normalization must preserve UTF-16 run offsets"
    );
    let source_runs = source_text_runs(text, total);
    let runs = source_runs
        .iter()
        .filter(|run| run.start_utf16 < run.end_utf16)
        .map(|run| TextRun {
            start_utf16: run.start_utf16,
            end_utf16: run.end_utf16,
            style: project_text_style(run.style),
        })
        .collect();
    let mut links = text
        .hyperlinks()
        .into_iter()
        .map(|link| TextLink {
            start_utf16: link.start(),
            end_utf16: link.end(),
            target: link.target().to_owned(),
            origin: TextLinkOrigin::OneNote,
        })
        .collect::<Vec<_>>();
    links.extend(detect_plain_links(text, &source_runs, &links));
    links.sort_by_key(|link| (link.start_utf16, link.end_utf16));
    let mut math_objects = text.math_inline_objects().iter().copied();
    let associated = source_runs
        .iter()
        .map(|run| {
            let object = run
                .style
                .math_formatting()
                .then(|| math_objects.next())
                .flatten();
            (run, object)
        })
        .collect::<Vec<_>>();
    let mut math = Vec::new();
    let mut index = 0;
    while index < associated.len() {
        if !associated[index].0.style.math_formatting() {
            index += 1;
            continue;
        }
        let start = index;
        while index < associated.len() && associated[index].0.style.math_formatting() {
            index += 1;
        }
        let segments = associated[start..index]
            .iter()
            .map(|(run, object)| MathSegment {
                start_utf16: run.start_utf16,
                end_utf16: run.end_utf16,
                text: run.text,
                object: object.unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        let display = segments
            .first()
            .is_some_and(|segment| segment.start_utf16 == 0)
            && segments
                .last()
                .is_some_and(|segment| segment.end_utf16 == total);
        math.push(decode_span(&segments, display));
    }
    TextBlock {
        text: projected_text.into_owned(),
        base_style,
        runs,
        math,
        links,
        alignment: match text.paragraph_alignment() {
            ParagraphAlignment::Left => TextAlignment::Left,
            ParagraphAlignment::Center => TextAlignment::Center,
            ParagraphAlignment::Right => TextAlignment::Right,
            ParagraphAlignment::Unknown => TextAlignment::Unknown,
        },
        space_before: half_inches(text.paragraph_space_before()),
        space_after: half_inches(text.paragraph_space_after()),
        line_spacing: text.paragraph_line_spacing_exact().map(half_inches),
    }
}

fn detect_plain_links(
    text: &RichText,
    source_runs: &[SourceTextRun<'_>],
    explicit: &[TextLink],
) -> Vec<TextLink> {
    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url, LinkKind::Email]);
    finder.url_must_have_scheme(false);
    if source_runs.is_empty() {
        let style = text.paragraph_style();
        return if style.hidden() || style.hyperlink() || style.math_formatting() {
            Vec::new()
        } else {
            detect_links_in_run(&finder, text.text(), 0, explicit)
        };
    }
    let mut links = Vec::new();
    for run in source_runs {
        if run.style.hidden() || run.style.hyperlink() || run.style.math_formatting() {
            continue;
        }
        links.extend(detect_links_in_run(
            &finder,
            run.text,
            run.start_utf16,
            explicit,
        ));
    }
    links
}

fn detect_links_in_run(
    finder: &LinkFinder,
    text: &str,
    run_start_utf16: u32,
    explicit: &[TextLink],
) -> Vec<TextLink> {
    finder
        .links(text)
        .filter_map(|found| {
            let start_utf16 = run_start_utf16.saturating_add(byte_to_utf16(text, found.start()));
            let end_utf16 = run_start_utf16.saturating_add(byte_to_utf16(text, found.end()));
            if start_utf16 >= end_utf16
                || explicit
                    .iter()
                    .any(|link| start_utf16 < link.end_utf16 && end_utf16 > link.start_utf16)
            {
                return None;
            }
            let target = match found.kind() {
                LinkKind::Email => format!("mailto:{}", found.as_str()),
                LinkKind::Url if has_uri_scheme(found.as_str()) => found.as_str().to_owned(),
                LinkKind::Url => format!("https://{}", found.as_str()),
                _ => return None,
            };
            Some(TextLink {
                start_utf16,
                end_utf16,
                target,
                origin: TextLinkOrigin::Detected,
            })
        })
        .collect()
}

fn byte_to_utf16(text: &str, byte: usize) -> u32 {
    u32::try_from(text[..byte].encode_utf16().count()).unwrap_or(u32::MAX)
}

fn has_uri_scheme(value: &str) -> bool {
    let Some((scheme, _)) = value.split_once(':') else {
        return false;
    };
    let mut characters = scheme.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
}

fn normalize_rich_text_controls(text: &str) -> Cow<'_, str> {
    if !text.contains(['\0', '\u{000B}']) {
        return Cow::Borrowed(text);
    }

    Cow::Owned(
        text.chars()
            .map(|character| match character {
                '\0' => '\u{fffd}',
                '\u{000B}' => '\n',
                _ => character,
            })
            .collect(),
    )
}

struct SourceTextRun<'a> {
    start_utf16: u32,
    end_utf16: u32,
    text: &'a str,
    style: &'a ParagraphStyling,
}

fn source_text_runs(text: &RichText, total: u32) -> Vec<SourceTextRun<'_>> {
    let mut runs = Vec::new();
    let mut start = 0_u32;
    for (index, style) in text.text_run_formatting().iter().enumerate() {
        let end = text
            .text_run_indices()
            .get(index)
            .copied()
            .unwrap_or(total)
            .min(total)
            .max(start);
        let start_byte = utf16_to_byte(text.text(), start);
        let end_byte = utf16_to_byte(text.text(), end);
        runs.push(SourceTextRun {
            start_utf16: start,
            end_utf16: end,
            text: &text.text()[start_byte..end_byte],
            style,
        });
        start = end;
    }
    runs
}

fn utf16_to_byte(text: &str, target: u32) -> usize {
    let mut utf16 = 0_u32;
    for (byte, character) in text.char_indices() {
        if utf16 >= target {
            return byte;
        }
        utf16 = utf16.saturating_add(u32::try_from(character.len_utf16()).unwrap_or(2));
    }
    text.len()
}

fn project_text_style(style: &ParagraphStyling) -> TextStyle {
    TextStyle {
        font: style.font().map(str::to_owned),
        font_size: style.font_size().map(|size| f32::from(size) / 2.0),
        foreground: style.font_color().and_then(project_color_ref),
        highlight: style.highlight().and_then(project_color_ref),
        bold: style.bold(),
        italic: style.italic(),
        underline: style.underline(),
        strikethrough: style.strikethrough(),
        superscript: style.superscript(),
        subscript: style.subscript(),
        hidden: style.hidden(),
        hyperlink: style.hyperlink(),
        hyperlink_protected: style.hyperlink_protected(),
        language_code: style.language_code(),
    }
}

fn project_list(list: &List) -> ListMarker {
    ListMarker {
        template: project_list_template(list.list_format()),
        restart: list.list_restart(),
        font: list.list_font().or_else(|| list.font()).map(str::to_owned),
    }
}

fn project_list_template(format: &[char]) -> Vec<ListMarkerPart> {
    const AUTOMATIC_NUMBER: char = '\u{fffd}';

    let mut template = Vec::new();
    let mut literal = String::new();
    let mut characters = format.iter().copied();
    while let Some(character) = characters.next() {
        if character == AUTOMATIC_NUMBER {
            if !literal.is_empty() {
                template.push(ListMarkerPart::Literal(std::mem::take(&mut literal)));
            }
            if let Some(format_code) = characters.next() {
                template.push(ListMarkerPart::Number(project_number_format(format_code)));
            } else {
                template.push(ListMarkerPart::Number(ListNumberFormat::Unsupported(
                    u32::MAX,
                )));
            }
        } else {
            literal.push(character);
        }
    }
    if !literal.is_empty() {
        template.push(ListMarkerPart::Literal(literal));
    }
    template
}

fn project_number_format(format: char) -> ListNumberFormat {
    match u32::from(format) {
        0 => ListNumberFormat::Decimal,
        1 => ListNumberFormat::UpperRoman,
        2 => ListNumberFormat::LowerRoman,
        3 => ListNumberFormat::UpperLetter,
        4 => ListNumberFormat::LowerLetter,
        other => ListNumberFormat::Unsupported(other),
    }
}

fn append_ink(
    ink: &ParserInk,
    output: &mut Vec<InkStroke>,
    recognized: &mut Vec<String>,
    point_count: &mut usize,
    point_limit: usize,
    source_path: &Path,
) -> Result<()> {
    for stroke in ink.ink_strokes() {
        *point_count = point_count.saturating_add(stroke.path().len());
        if *point_count > point_limit {
            return Err(Error::Parse {
                path: source_path.to_path_buf(),
                message: "ink point limit exceeded".to_owned(),
            });
        }
        let word = stroke
            .recognized_word()
            .and_then(|word| word.text())
            .map(str::to_owned);
        if let Some(word) = &word {
            recognized.push(word.clone());
        }
        output.push(InkStroke {
            points: stroke
                .path()
                .iter()
                .map(|point| InkPoint {
                    x: finite(point.x()),
                    y: finite(point.y()),
                })
                .collect(),
            width: finite(stroke.width()),
            height: finite(stroke.height()),
            color: stroke.color().map(project_ink_color),
            opacity: stroke.transparency().unwrap_or(u8::MAX),
            recognized_word: word,
        });
    }
    for child in ink.child_groups() {
        append_ink(
            child,
            output,
            recognized,
            point_count,
            point_limit,
            source_path,
        )?;
    }
    Ok(())
}

fn project_color(color: ParserColor) -> Color {
    Color {
        red: color.r(),
        green: color.g(),
        blue: color.b(),
        alpha: color.alpha(),
    }
}

fn project_color_ref(color: ColorRef) -> Option<Color> {
    match color {
        ColorRef::Auto => None,
        ColorRef::Manual { r, g, b } => Some(Color {
            red: r,
            green: g,
            blue: b,
            alpha: u8::MAX,
        }),
    }
}

fn project_ink_color(value: u32) -> Color {
    let bytes = value.to_le_bytes();
    Color {
        red: bytes[0],
        green: bytes[1],
        blue: bytes[2],
        alpha: u8::MAX,
    }
}

fn report_diagnostics(report: &Report, source_id: &SourceId) -> Vec<Diagnostic> {
    report
        .warnings()
        .iter()
        .map(|warning| Diagnostic {
            severity: DiagnosticSeverity::Warning,
            code: "parser_warning".to_owned(),
            message: warning.message().to_owned(),
            page_id: warning.page().map(|(page_id, _)| {
                PageId::new(stable_id(&[
                    source_id.as_str().as_bytes(),
                    b"page",
                    page_id.to_string().as_bytes(),
                ]))
            }),
        })
        .collect()
}

fn count_outline_elements(items: &[OutlineItem]) -> usize {
    items
        .iter()
        .map(|item| match item {
            OutlineItem::Element(element) => 1 + count_outline_elements(element.children()),
            OutlineItem::Group(group) => count_outline_elements(group.outlines()),
        })
        .sum()
}

fn notebook_name(path: &Path) -> String {
    path.parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "OneNote notebook".to_owned())
}

fn half_inches(value: f32) -> f32 {
    finite(value) * PIXELS_PER_HALF_INCH
}

fn finite(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

fn image_media_type(extension: &str) -> &'static str {
    match extension.to_ascii_lowercase().as_str() {
        "bmp" => "image/bmp",
        "gif" => "image/gif",
        "jpeg" | "jpg" => "image/jpeg",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "tif" | "tiff" => "image/tiff",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

fn stable_id(parts: &[&[u8]]) -> String {
    let mut bytes = Vec::new();
    for part in parts {
        bytes.extend_from_slice(&(part.len() as u64).to_le_bytes());
        bytes.extend_from_slice(part);
    }
    Uuid::new_v5(&Uuid::NAMESPACE_URL, &bytes).to_string()
}

fn source_fingerprint(path: &Path) -> Result<SourceFingerprint> {
    let root = if has_extension(path, "onetoc2") {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    let mut files = if root.is_dir() {
        native_source_files(root)?
    } else {
        vec![path.to_path_buf()]
    };
    files.sort();

    let mut hasher = blake3::Hasher::new();
    for file in files {
        let metadata = fs_metadata(&file)?;
        if !metadata.is_file() {
            return Err(Error::Parse {
                path: file,
                message: "source manifest contains a non-regular file".to_owned(),
            });
        }
        let relative = file.strip_prefix(root).unwrap_or(&file);
        let relative_bytes = path_identity(relative);
        hasher.update(&(relative_bytes.len() as u64).to_le_bytes());
        hasher.update(&relative_bytes);
        hasher.update(&metadata.len().to_le_bytes());
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok());
        hasher.update(
            &modified
                .map_or(0, |duration| duration.as_secs())
                .to_le_bytes(),
        );
        hasher.update(
            &modified
                .map_or(0, |duration| duration.subsec_nanos())
                .to_le_bytes(),
        );
    }
    Ok(SourceFingerprint::new(
        hasher.finalize().to_hex().to_string(),
    ))
}

fn ensure_source_unchanged(path: &Path, before: &SourceFingerprint) -> Result<()> {
    let after = source_fingerprint(path)?;
    if &after == before {
        Ok(())
    } else {
        Err(Error::Parse {
            path: path.to_path_buf(),
            message: "source files changed while parsing".to_owned(),
        })
    }
}

fn native_source_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).map_err(|source| Error::Io {
            path: directory.clone(),
            source,
        })? {
            let entry = entry.map_err(|source| Error::Io {
                path: directory.clone(),
                source,
            })?;
            let file_type = entry.file_type().map_err(|source| Error::Io {
                path: entry.path(),
                source,
            })?;
            let path = entry.path();
            if file_type.is_symlink() {
                return Err(Error::Parse {
                    path,
                    message: "source tree contains a symbolic link".to_owned(),
                });
            }
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file()
                && (has_extension(&path, "one") || has_extension(&path, "onetoc2"))
            {
                files.push(path);
            }
        }
    }
    Ok(files)
}

fn fs_metadata(path: &Path) -> Result<std::fs::Metadata> {
    std::fs::metadata(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn has_extension(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

#[cfg(unix)]
fn path_identity(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(windows)]
fn path_identity(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

#[cfg(unix)]
fn host_typed_path(path: &Path) -> TypedPath<'_> {
    use std::os::unix::ffi::OsStrExt;
    TypedPath::new(path.as_os_str().as_bytes(), PathType::Unix)
}

#[cfg(windows)]
fn host_typed_path(path: &Path) -> TypedPath<'_> {
    TypedPath::new(
        path.to_str().expect("Windows path must be valid Unicode"),
        PathType::Windows,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        detect_links_in_run, normalize_rich_text_controls, project_list_template, stable_id,
        OneNoteLoader,
    };
    use crate::{Error, ListMarkerPart, ListNumberFormat, TextLink, TextLinkOrigin};
    use linkify::{LinkFinder, LinkKind};
    use std::fs;

    #[test]
    fn stable_ids_are_deterministic_and_typed() {
        let first = stable_id(&[b"source", b"path"]);
        let again = stable_id(&[b"source", b"path"]);
        let other = stable_id(&[b"page", b"path"]);
        assert_eq!(first, again);
        assert_ne!(first, other);
    }

    #[test]
    fn rejects_non_onenote_files_before_parsing() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("notes.txt");
        fs::write(&path, b"not OneNote").expect("write fixture");
        let error = OneNoteLoader::default()
            .load(&path)
            .expect_err("must reject extension");
        assert!(matches!(error, Error::UnsupportedSource { .. }));
    }

    #[test]
    fn decodes_automatic_number_templates_without_exposing_control_characters() {
        assert_eq!(
            project_list_template(&['\u{fffd}', '\0', '.']),
            vec![
                ListMarkerPart::Number(ListNumberFormat::Decimal),
                ListMarkerPart::Literal(".".to_owned()),
            ]
        );
        assert_eq!(
            project_list_template(&['(', '\u{fffd}', '\u{2}', ')']),
            vec![
                ListMarkerPart::Literal("(".to_owned()),
                ListMarkerPart::Number(ListNumberFormat::LowerRoman),
                ListMarkerPart::Literal(")".to_owned()),
            ]
        );
    }

    #[test]
    fn preserves_literal_bullet_templates() {
        assert_eq!(
            project_list_template(&['•']),
            vec![ListMarkerPart::Literal("•".to_owned())]
        );
    }

    #[test]
    fn normalizes_rich_text_controls_without_changing_utf16_offsets() {
        let source = "A😀\u{000B}\u{000B}B\0C";
        let normalized = normalize_rich_text_controls(source);

        assert_eq!(normalized, "A😀\n\nB�C");
        assert_eq!(
            source.encode_utf16().count(),
            normalized.encode_utf16().count()
        );
    }

    #[test]
    fn detects_plain_urls_and_emails_with_utf16_offsets() {
        let mut finder = LinkFinder::new();
        finder.kinds(&[LinkKind::Url, LinkKind::Email]);
        finder.url_must_have_scheme(false);
        let text = "😀 example.com and user@example.com";

        let links = detect_links_in_run(&finder, text, 7, &[]);

        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, "https://example.com");
        assert_eq!(links[0].origin, TextLinkOrigin::Detected);
        assert_eq!(links[0].start_utf16, 10);
        assert_eq!(links[1].target, "mailto:user@example.com");
    }

    #[test]
    fn explicit_link_ranges_take_precedence_over_detection() {
        let mut finder = LinkFinder::new();
        finder.kinds(&[LinkKind::Url, LinkKind::Email]);
        finder.url_must_have_scheme(false);
        let explicit = TextLink {
            start_utf16: 0,
            end_utf16: 12,
            target: "onenote:#page-id={abc}".to_owned(),
            origin: TextLinkOrigin::OneNote,
        };

        assert!(detect_links_in_run(&finder, "example.com", 0, &[explicit]).is_empty());
    }
}
