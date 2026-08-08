use onenote_core::{
    ElementContent, Error, MathSpan, NotebookEntry, ObjectKind, OneNoteLoader, OnePkgExtractor,
    OutlineElement, PageObjectRole, ResourceRef, ResourceStatus, TextBlock,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

#[test]
fn extracted_private_notebook_projects_all_native_sections() {
    let Some(corpus) = corpus_path() else {
        return;
    };
    let files = native_files(&corpus);
    let section_file_count = files
        .iter()
        .filter(|path| has_extension(path, "one"))
        .count();
    let notebook_root = files
        .iter()
        .find(|path| {
            has_extension(path, "onetoc2") && path.parent().is_some_and(|parent| parent == corpus)
        })
        .expect("the extracted corpus must have a root .onetoc2");

    let loaded = OneNoteLoader::default()
        .load(notebook_root)
        .expect("the supplied notebook must project");
    let sections: Vec<_> = loaded.notebook.sections().collect();
    let pages: Vec<_> = loaded.notebook.pages().collect();

    assert_eq!(section_file_count, 32, "unexpected private corpus shape");
    assert_eq!(
        sections.len(),
        section_file_count,
        "every native section must appear in the projected tree"
    );
    assert!(!pages.is_empty(), "the notebook must expose pages");
    assert!(
        pages.iter().any(|page| !page.visible_text().is_empty()),
        "projected pages must expose searchable visible text"
    );
    assert!(
        pages.iter().any(|page| !page.objects.is_empty()),
        "projected pages must expose positioned objects"
    );
    let mut text_blocks = Vec::new();
    for page in &pages {
        collect_page_text_blocks(page, &mut text_blocks);
    }
    assert!(
        text_blocks
            .iter()
            .all(|block| !block.text.contains('\u{000B}')),
        "projected text must not expose source vertical tabs"
    );
    assert!(
        text_blocks.iter().any(|block| block.text.contains('\n')),
        "private corpus must exercise semantic source line breaks"
    );
    assert!(
        pages
            .iter()
            .flat_map(|page| &page.objects)
            .any(|object| object.role == PageObjectRole::Title),
        "native title-area objects must retain their semantic role"
    );
    assert!(
        pages
            .iter()
            .flat_map(|page| &page.objects)
            .any(|object| object.role == PageObjectRole::Body),
        "native body objects must remain distinct from title-area objects"
    );

    let encoded = serde_json::to_vec(&loaded.notebook).expect("semantic model must serialize");
    assert!(
        encoded.len() < 64 * 1024 * 1024,
        "semantic projection unexpectedly materialized a huge representation"
    );

    let model_resource_count = resource_refs(&loaded.notebook.entries).len();
    assert_eq!(
        loaded.resources.len(),
        model_resource_count,
        "every resource reference must have one lazy loader"
    );
    let bounded_resource = loaded
        .resources
        .resource_ids()
        .find(|id| loaded.resources.declared_size(id).unwrap_or(0) > 0)
        .cloned();
    if let Some(id) = bounded_resource {
        assert!(matches!(
            loaded.resources.read_limited(&id, 0),
            Err(Error::ResourceTooLarge { .. })
        ));
    }
}

#[test]
fn private_toc_uses_latest_complete_ordering_snapshot() {
    let Some(corpus) = machine_learning_corpus_path() else {
        return;
    };
    let toc = native_files(&corpus)
        .into_iter()
        .find(|path| {
            has_extension(path, "onetoc2") && path.parent().is_some_and(|parent| parent == corpus)
        })
        .expect("private TOC corpus must have a root .onetoc2");
    let loaded = OneNoteLoader::default()
        .load(toc)
        .expect("the private TOC notebook must project");
    let root_entries = loaded
        .notebook
        .entries
        .iter()
        .map(|entry| match entry {
            NotebookEntry::Section(_) => false,
            NotebookEntry::Group(_) => true,
        })
        .collect::<Vec<_>>();
    let first_group = root_entries
        .iter()
        .position(|is_group| *is_group)
        .expect("private TOC corpus must contain a root section group");
    assert!(root_entries[..first_group].iter().all(|is_group| !is_group));
    assert!(root_entries[first_group..].iter().all(|is_group| *is_group));
    let native_section_count = native_files(&corpus)
        .iter()
        .filter(|path| has_extension(path, "one"))
        .count();
    assert_eq!(native_section_count, 40, "unexpected corpus shape");
    assert_eq!(
        loaded.notebook.sections().count(),
        native_section_count,
        "every native section must appear exactly once"
    );
    let nested = loaded
        .notebook
        .sections()
        .find(|section| {
            loaded
                .notebook
                .section_path(&section.id)
                .is_some_and(|path| path.len() > 1)
        })
        .expect("private TOC corpus must contain a nested section");
    let nested_path = loaded
        .notebook
        .section_path(&nested.id)
        .expect("nested section path");
    assert_eq!(nested_path.last(), Some(&nested.name.as_str()));
}

#[test]
fn private_revision_section_uses_active_revision_and_semantic_lists() {
    let Some(section) = documentation_section_path() else {
        return;
    };
    let loaded = OneNoteLoader::default()
        .load(section)
        .expect("private revision section must project");
    assert!(
        loaded
            .notebook
            .pages()
            .all(|page| !page.visible_text().contains('\u{000B}')),
        "projected private pages must not expose source vertical tabs"
    );
    let mut source_line_breaks = 0_usize;
    let mut consecutive_line_break_blocks = 0_usize;
    for page in loaded.notebook.pages() {
        let mut blocks = Vec::new();
        collect_page_text_blocks(page, &mut blocks);
        source_line_breaks += blocks
            .iter()
            .map(|block| block.text.matches('\n').count())
            .sum::<usize>();
        consecutive_line_break_blocks += blocks
            .iter()
            .filter(|block| block.text.contains("\n\n"))
            .count();
    }
    assert!(source_line_breaks > 0);
    assert!(consecutive_line_break_blocks > 0);

    let mut selected_lists = Vec::new();
    let list_page = loaded
        .notebook
        .pages()
        .find(|page| {
            let mut lists = Vec::new();
            collect_page_lists(page, &mut lists);
            let has_nested_levels = [1, 2, 3]
                .into_iter()
                .all(|level| lists.iter().any(|(candidate, _)| *candidate == level));
            if has_nested_levels {
                selected_lists = lists;
            }
            has_nested_levels
        })
        .expect("private revision corpus must contain a deeply nested list page");
    assert!(list_page.visible_text().lines().count() > 10);

    assert!(selected_lists.iter().any(|(level, marker)| {
        *level == 1
            && marker
                .template
                .contains(&onenote_core::ListMarkerPart::Number(
                    onenote_core::ListNumberFormat::Decimal,
                ))
    }));
    assert!(selected_lists.iter().any(|(level, marker)| {
        *level == 2
            && marker
                .template
                .contains(&onenote_core::ListMarkerPart::Number(
                    onenote_core::ListNumberFormat::LowerLetter,
                ))
    }));
    assert!(selected_lists.iter().any(|(level, marker)| {
        *level == 3
            && marker
                .template
                .contains(&onenote_core::ListMarkerPart::Number(
                    onenote_core::ListNumberFormat::LowerRoman,
                ))
    }));
    let encoded = serde_json::to_string(&list_page).expect("private page must serialize");
    assert!(!encoded.contains(['\0', '\u{000B}']));
}

fn collect_lists<'a>(
    elements: &'a [OutlineElement],
    lists: &mut Vec<(u8, &'a onenote_core::ListMarker)>,
) {
    for element in elements {
        if let Some(marker) = &element.list {
            lists.push((element.level, marker));
        }
        collect_lists(&element.children, lists);
    }
}

fn collect_text_blocks<'a>(elements: &'a [OutlineElement], blocks: &mut Vec<&'a TextBlock>) {
    for element in elements {
        for content in &element.content {
            if let ElementContent::Text(text) = content {
                blocks.push(text);
            }
        }
        collect_text_blocks(&element.children, blocks);
    }
}

fn collect_page_lists<'a>(
    page: &'a onenote_core::Page,
    lists: &mut Vec<(u8, &'a onenote_core::ListMarker)>,
) {
    for object in &page.objects {
        if let ObjectKind::Outline(outline) = &object.kind {
            collect_lists(&outline.elements, lists);
        }
    }
}

fn collect_page_text_blocks<'a>(page: &'a onenote_core::Page, blocks: &mut Vec<&'a TextBlock>) {
    for object in &page.objects {
        if let ObjectKind::Outline(outline) = &object.kind {
            collect_text_blocks(&outline.elements, blocks);
        }
    }
}

#[test]
fn every_private_backup_section_snapshot_opens_individually() {
    let Some(corpus) = backup_corpus_path() else {
        return;
    };
    let sections: Vec<_> = native_files(&corpus)
        .into_iter()
        .filter(|path| has_extension(path, "one"))
        .collect();
    assert!(
        !sections.is_empty(),
        "the private backup corpus must contain .one snapshots"
    );

    let mut failures = Vec::new();
    let mut unavailable_resources = 0;
    for path in &sections {
        match OneNoteLoader::default().load(path) {
            Ok(loaded) => {
                for resource in resource_refs(&loaded.notebook.entries) {
                    let status = loaded
                        .resources
                        .status(&resource.id)
                        .expect("projected resource must have a lazy loader");
                    assert_eq!(status, resource.status);
                    if status != ResourceStatus::Available {
                        unavailable_resources += 1;
                        assert!(matches!(
                            loaded.resources.read_limited(&resource.id, u64::MAX),
                            Err(Error::ResourceUnavailable {
                                status: read_status,
                                ..
                            }) if read_status == status
                        ));
                    }
                }
            }
            Err(error) => {
                let relative = path.strip_prefix(&corpus).unwrap_or(path);
                let case_id = blake3::hash(relative.to_string_lossy().as_bytes())
                    .to_hex()
                    .to_string();
                let redacted = error
                    .to_string()
                    .replace(path.to_string_lossy().as_ref(), "[private-source]");
                failures.push((case_id[..12].to_owned(), redacted));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} private backup snapshots failed: {failures:?}",
        failures.len(),
        sections.len()
    );
    assert!(
        unavailable_resources > 0,
        "the backup compatibility corpus must exercise unavailable payloads"
    );
}

#[test]
fn supplied_package_extracts_on_disk_to_a_complete_native_tree() {
    let Some(package) = package_path() else {
        return;
    };
    let Ok(extractor) = OnePkgExtractor::detect() else {
        return;
    };
    let temporary = tempfile::tempdir().expect("temporary extraction parent");
    let destination = temporary.path().join("notebook");
    let source_size = std::fs::metadata(&package).expect("package metadata").len();

    let report = extractor
        .extract(&package, &destination, &AtomicBool::new(false))
        .expect("the supplied package must extract");

    assert_eq!(report.section_files, 32);
    assert_eq!(report.table_of_contents_files, 5);
    assert_eq!(report.total_files, 37);
    assert_eq!(
        std::fs::metadata(&package).expect("package metadata").len(),
        source_size,
        "package source must remain unchanged"
    );
    assert!(report.destination.is_dir());
}

#[test]
fn private_math_package_projects_structured_office_math_without_private_markers() {
    let Some(package) = math_package_path() else {
        return;
    };
    let Ok(extractor) = OnePkgExtractor::detect() else {
        return;
    };
    let temporary = tempfile::tempdir().expect("temporary extraction parent");
    let destination = temporary.path().join("notebook");
    extractor
        .extract(&package, &destination, &AtomicBool::new(false))
        .expect("private math package must extract");
    let section = native_files(&destination)
        .into_iter()
        .find(|path| has_extension(path, "one"))
        .expect("private math package must contain a section");
    let loaded = OneNoteLoader::default()
        .load(section)
        .expect("the private math section must project");
    let mut spans = Vec::new();
    for page in loaded.notebook.pages() {
        let mut page_spans = Vec::new();
        collect_page_math(page, &mut page_spans);
        if page_spans.len() > spans.len() {
            spans = page_spans;
        }
    }

    assert_eq!(
        spans.len(),
        3,
        "private fixture must contain three equations"
    );
    assert!(spans.iter().all(|span| span.expression.is_some()));
    assert!(spans.iter().all(|span| span.diagnostic.is_none()));
    let debug = format!("{spans:#?}");
    assert!(debug.contains("Superscript"));
    assert!(debug.contains("Fraction"));
    assert!(debug.contains("Nary"));
    let visible = spans
        .iter()
        .map(|span| span.visible_text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(visible.contains('∑'));
    assert!(!visible.contains(['\u{fdd0}', '\u{fdee}', '\u{fdef}']));
}

fn collect_math<'a>(elements: &'a [OutlineElement], spans: &mut Vec<&'a MathSpan>) {
    for element in elements {
        for content in &element.content {
            if let ElementContent::Text(text) = content {
                spans.extend(&text.math);
            }
        }
        collect_math(&element.children, spans);
    }
}

fn collect_page_math<'a>(page: &'a onenote_core::Page, spans: &mut Vec<&'a MathSpan>) {
    for object in &page.objects {
        if let ObjectKind::Outline(outline) = &object.kind {
            collect_math(&outline.elements, spans);
        }
    }
}

fn corpus_path() -> Option<PathBuf> {
    let path = std::env::var_os("ONENOTE_TEST_CORPUS").map(PathBuf::from)?;
    path.is_dir().then_some(path)
}

fn package_path() -> Option<PathBuf> {
    let path = std::env::var_os("ONENOTE_TEST_PACKAGE").map(PathBuf::from)?;
    path.is_file().then_some(path)
}

fn math_package_path() -> Option<PathBuf> {
    let path = std::env::var_os("ONENOTE_MATH_TEST_PACKAGE").map(PathBuf::from)?;
    path.is_file().then_some(path)
}

fn machine_learning_corpus_path() -> Option<PathBuf> {
    let path = std::env::var_os("ONENOTE_TOC_TEST_CORPUS").map(PathBuf::from)?;
    path.is_dir().then_some(path)
}

fn documentation_section_path() -> Option<PathBuf> {
    let path = std::env::var_os("ONENOTE_REVISION_TEST_SECTION").map(PathBuf::from)?;
    path.is_file().then_some(path)
}

fn backup_corpus_path() -> Option<PathBuf> {
    std::env::var_os("ONENOTE_BACKUP_TEST_CORPUS")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

fn native_files(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(directory)
            .expect("read corpus directory")
            .map(|entry| entry.expect("read corpus entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                pending.push(path);
            } else if has_extension(&path, "one") || has_extension(&path, "onetoc2") {
                files.push(path);
            }
        }
    }
    files
}

fn has_extension(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn resource_refs(entries: &[NotebookEntry]) -> Vec<&ResourceRef> {
    let mut resources = Vec::new();
    for entry in entries {
        match entry {
            NotebookEntry::Section(section) => {
                for page in &section.pages {
                    for object in &page.objects {
                        match &object.kind {
                            ObjectKind::Image(image) => {
                                resources.push(&image.resource);
                                resources.extend(image.web_fallback.iter());
                            }
                            ObjectKind::Attachment(attachment) => {
                                resources.push(&attachment.resource);
                                resources.extend(attachment.icon.iter());
                            }
                            ObjectKind::Outline(outline) => {
                                for element in &outline.elements {
                                    element_resource_refs(element, &mut resources);
                                }
                            }
                            ObjectKind::Ink(_) | ObjectKind::Unknown => {}
                        }
                    }
                }
            }
            NotebookEntry::Group(group) => resources.extend(resource_refs(&group.entries)),
        }
    }
    resources
}

fn element_resource_refs<'a>(
    element: &'a onenote_core::OutlineElement,
    output: &mut Vec<&'a ResourceRef>,
) {
    for content in &element.content {
        match content {
            ElementContent::Image(image) => {
                output.push(&image.resource);
                output.extend(image.web_fallback.iter());
            }
            ElementContent::Attachment(attachment) => {
                output.push(&attachment.resource);
                output.extend(attachment.icon.iter());
            }
            ElementContent::Table(table) => {
                for row in &table.rows {
                    for cell in row {
                        for nested in &cell.elements {
                            element_resource_refs(nested, output);
                        }
                    }
                }
            }
            ElementContent::Text(_) | ElementContent::Ink(_) | ElementContent::Unknown => {}
        }
    }
    for child in &element.children {
        element_resource_refs(child, output);
    }
}
