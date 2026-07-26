use onenote_core::OneNoteLoader;
use onenote_render::{SceneBuilder, ScenePrimitive};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

#[test]
fn every_private_corpus_page_builds_a_finite_scene() {
    let Some(root) = notebook_root() else {
        return;
    };
    let loaded = OneNoteLoader::default()
        .load(root)
        .expect("private notebook must parse");
    let pages: Vec<_> = loaded.notebook.pages().collect();
    let builder = SceneBuilder::default();
    let cancel = AtomicBool::new(false);
    let mut node_count = 0_usize;
    let mut text_count = 0_usize;
    let mut image_count = 0_usize;
    let mut attachment_count = 0_usize;
    let mut ink_count = 0_usize;

    for page in &pages {
        let scene = builder.build(page, &cancel).expect("page scene");
        assert_eq!(scene.page_id, page.id);
        assert!(finite_rect(scene.bounds));
        for node in &scene.nodes {
            assert!(finite_rect(node.bounds));
            node_count += 1;
            match &node.primitive {
                ScenePrimitive::Text { .. } => text_count += 1,
                ScenePrimitive::Image(_) => image_count += 1,
                ScenePrimitive::Attachment(_) => attachment_count += 1,
                ScenePrimitive::Ink { .. } => ink_count += 1,
                ScenePrimitive::Fill { .. }
                | ScenePrimitive::Line { .. }
                | ScenePrimitive::Placeholder { .. } => {}
            }
        }
    }

    assert!(!pages.is_empty());
    assert!(node_count > pages.len());
    assert!(text_count > 0);
    assert!(image_count + attachment_count + ink_count > 0);
}

fn finite_rect(rect: onenote_core::Rect) -> bool {
    [rect.x, rect.y, rect.width, rect.height]
        .into_iter()
        .all(f32::is_finite)
        && rect.width >= 0.0
        && rect.height >= 0.0
}

fn notebook_root() -> Option<PathBuf> {
    let corpus = std::env::var_os("ONENOTE_TEST_CORPUS").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../onepkg/Personal.extracted"),
        PathBuf::from,
    );
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
