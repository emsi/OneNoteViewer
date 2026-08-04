use onenote_core::{
    ElementContent, Error, MathSpan, NotebookEntry, ObjectKind, OneNoteLoader, OnePkgExtractor,
    OutlineElement, PageObjectRole, ResourceRef, ResourceStatus,
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
fn documentation_section_uses_active_revision_and_semantic_lists() {
    let Some(section) = documentation_section_path() else {
        return;
    };
    let loaded = OneNoteLoader::default()
        .load(section)
        .expect("Documentation.one must project");
    let java = loaded
        .notebook
        .pages()
        .find(|page| page.title == "Java")
        .expect("Java regression page");
    let java_text = java.visible_text();
    for expected in [
        "Prepare java sources:",
        "Extract function lines information:",
        "Run jtest analysis",
        "Build corpus:",
        "Update CVE dataset",
        "Train model:",
    ] {
        assert!(
            java_text.contains(expected),
            "active Java revision must retain {expected:?}"
        );
    }

    let gentoo = loaded
        .notebook
        .pages()
        .find(|page| page.title == "Gentoo build system")
        .expect("Gentoo regression page");
    let gentoo_text = gentoo.visible_text();
    assert!(gentoo_text.contains("Downloading and building a package:"));
    assert!(gentoo_text.contains("To create ccptestcli.sh script call"));
    assert!(gentoo_text.contains("-NA cpptest"));

    let mut lists = Vec::new();
    for object in &java.objects {
        if let ObjectKind::Outline(outline) = &object.kind {
            collect_lists(&outline.elements, &mut lists);
        }
    }
    assert!(lists.iter().any(|(level, marker)| {
        *level == 1
            && marker
                .template
                .contains(&onenote_core::ListMarkerPart::Number(
                    onenote_core::ListNumberFormat::Decimal,
                ))
    }));
    assert!(lists.iter().any(|(level, marker)| {
        *level == 2
            && marker
                .template
                .contains(&onenote_core::ListMarkerPart::Number(
                    onenote_core::ListNumberFormat::LowerLetter,
                ))
    }));
    assert!(lists.iter().any(|(level, marker)| {
        *level == 3
            && marker
                .template
                .contains(&onenote_core::ListMarkerPart::Number(
                    onenote_core::ListNumberFormat::LowerRoman,
                ))
    }));
    let encoded = serde_json::to_string(&java).expect("Java page must serialize");
    assert!(!encoded.contains(['\0', '\u{fffd}']));
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
fn maths_package_projects_structured_office_math_without_private_markers() {
    let Some(package) = maths_package_path() else {
        return;
    };
    let Ok(extractor) = OnePkgExtractor::detect() else {
        return;
    };
    let temporary = tempfile::tempdir().expect("temporary extraction parent");
    let destination = temporary.path().join("maths");
    extractor
        .extract(&package, &destination, &AtomicBool::new(false))
        .expect("Maths.onepkg must extract");
    let section = native_files(&destination)
        .into_iter()
        .find(|path| has_extension(path, "one"))
        .expect("Maths.onepkg must contain a section");
    let loaded = OneNoteLoader::default()
        .load(section)
        .expect("the Maths section must project");
    let page = loaded
        .notebook
        .pages()
        .find(|page| page.title.eq_ignore_ascii_case("mathx"))
        .expect("the mathx regression page");
    let mut spans = Vec::new();
    for object in &page.objects {
        if let ObjectKind::Outline(outline) = &object.kind {
            collect_math(&outline.elements, &mut spans);
        }
    }

    assert_eq!(spans.len(), 3, "mathx must contain all three equations");
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

fn maths_package_path() -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../onepkg/Maths.onepkg");
    path.is_file().then_some(path)
}

fn machine_learning_corpus_path() -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../onepkg/MachineLearning");
    path.is_dir().then_some(path)
}

fn documentation_section_path() -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../onepkg/ML@Parasoft/Documentation.one");
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
