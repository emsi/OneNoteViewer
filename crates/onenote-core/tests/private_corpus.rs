use onenote_core::{
    ElementContent, Error, NotebookEntry, ObjectKind, OneNoteLoader, OnePkgExtractor,
    PageObjectRole, ResourceRef, ResourceStatus,
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
fn machine_learning_toc_uses_latest_complete_ordering_snapshot() {
    let Some(corpus) = machine_learning_corpus_path() else {
        return;
    };
    let loaded = OneNoteLoader::default()
        .load(corpus.join("Open Notebook.onetoc2"))
        .expect("the MachineLearning notebook must project");
    let root_entries = loaded
        .notebook
        .entries
        .iter()
        .map(|entry| match entry {
            NotebookEntry::Section(section) => ("section", section.name.as_str()),
            NotebookEntry::Group(group) => ("group", group.name.as_str()),
        })
        .collect::<Vec<_>>();

    assert_eq!(
        root_entries,
        [
            ("section", "Notes"),
            ("section", "Coursera"),
            ("section", "Biometric Behavioral"),
            ("section", "Courses links and tutorials"),
            ("section", "Reinforcement Learning"),
            ("section", "Tips"),
            ("section", "Datasets"),
            ("section", "LLMs"),
            ("section", "Papers"),
            ("group", "_Tensorflow"),
            ("group", "Experiments"),
            ("group", "fastai Part 1 v3"),
            ("group", "md893 RL Nanodegree"),
            ("group", "nd101 Deep Learning Fundation"),
            ("group", "nd188 pytorch"),
            ("group", "nd892 NLP Nanodegree"),
            ("group", "ud012 deepracer"),
            ("group", "Udacity"),
        ]
    );
    assert_eq!(
        loaded.notebook.sections().count(),
        39,
        "every section referenced by the latest TOC snapshot must appear exactly once"
    );
    let deep_learning = loaded
        .notebook
        .sections()
        .find(|section| section.name == "Deep Larning (Udacity)")
        .expect("nested Udacity section");
    assert_eq!(
        loaded.notebook.section_path(&deep_learning.id),
        Some(vec!["Udacity", "Deep Larning (Udacity)"])
    );
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

fn corpus_path() -> Option<PathBuf> {
    let path = std::env::var_os("ONENOTE_TEST_CORPUS").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../onepkg/Personal.extracted"),
        PathBuf::from,
    );
    path.is_dir().then_some(path)
}

fn package_path() -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../onepkg/Personal.onepkg");
    path.is_file().then_some(path)
}

fn machine_learning_corpus_path() -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../onepkg/MachineLearning");
    path.is_dir().then_some(path)
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
