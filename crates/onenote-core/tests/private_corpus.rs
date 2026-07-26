use onenote_core::{ElementContent, Error, NotebookEntry, ObjectKind, OneNoteLoader, ResourceRef};
use std::path::{Path, PathBuf};

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

fn corpus_path() -> Option<PathBuf> {
    let path = std::env::var_os("ONENOTE_TEST_CORPUS").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../onepkg/Personal.extracted"),
        PathBuf::from,
    );
    path.is_dir().then_some(path)
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
                            ObjectKind::Image(image) => resources.push(&image.resource),
                            ObjectKind::Attachment(attachment) => {
                                resources.push(&attachment.resource);
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
            ElementContent::Image(image) => output.push(&image.resource),
            ElementContent::Attachment(attachment) => output.push(&attachment.resource),
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
