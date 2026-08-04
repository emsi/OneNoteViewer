use onenote_core::OneNoteLoader;
use onenote_index::{SearchIndex, SearchQuery};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

#[test]
fn complete_private_notebook_is_indexed_and_searchable() {
    let Some(root) = notebook_root() else {
        return;
    };
    let loaded = OneNoteLoader::default()
        .load(root)
        .expect("private notebook must parse");
    let expected_pages = loaded.notebook.pages().count();
    let query_term = loaded
        .notebook
        .pages()
        .flat_map(|page| {
            page.visible_text()
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .map(|word| {
            word.trim_matches(|character: char| !character.is_alphanumeric())
                .to_owned()
        })
        .find(|word| word.chars().count() >= 6)
        .expect("corpus must contain a searchable word");

    let temporary = tempfile::tempdir().expect("temporary index directory");
    let mut index = SearchIndex::open(temporary.path().join("workspace.sqlite")).expect("index");
    let cancel = AtomicBool::new(false);
    let mut final_progress = None;
    index
        .replace_source(&loaded.notebook, &cancel, |progress| {
            final_progress = Some(progress);
        })
        .expect("full corpus indexing");

    let status = index.sources().expect("source status");
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].page_count, expected_pages);
    assert_eq!(status[0].fingerprint, loaded.notebook.fingerprint);
    assert_eq!(
        final_progress.expect("progress").pages_completed,
        expected_pages
    );
    index.verify_integrity().expect("index integrity");

    let hits = index
        .search(&SearchQuery::simple(query_term), &cancel)
        .expect("corpus search");
    assert!(!hits.is_empty());
    assert!(hits
        .iter()
        .all(|hit| hit.source_id == loaded.notebook.source_id));
    assert!(hits
        .iter()
        .all(|hit| hit.source_fingerprint == loaded.notebook.fingerprint));
}

fn notebook_root() -> Option<PathBuf> {
    let corpus = std::env::var_os("ONENOTE_TEST_CORPUS").map(PathBuf::from)?;
    let mut roots: Vec<_> = std::fs::read_dir(corpus)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("onetoc2"))
        })
        .collect();
    roots.sort();
    roots.into_iter().next()
}
