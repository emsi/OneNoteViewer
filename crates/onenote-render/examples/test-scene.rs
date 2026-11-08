use onenote_core::OneNoteLoader;
use onenote_render::{SceneBuilder, SceneOptions};
use std::env;
use std::path::Path;
use std::sync::atomic::AtomicBool;

fn main() {
    let path = env::args().nth(1).expect("missing path");
    let loaded = OneNoteLoader::default().load(Path::new(&path)).expect("failed to load");
    let options = SceneOptions::default();
    let cancel = AtomicBool::new(false);

    for (page_idx, page) in loaded.notebook.pages().enumerate().take(3) {
        println!("\n=== Building scene for Page {}: {} ===", page_idx, page.title);
        let scene = SceneBuilder::with_options(options.clone()).build(&page, &cancel).expect("failed to build scene");
        println!("Scene bounds: {:?}", scene.bounds);
        println!("Total scene nodes: {}", scene.nodes.len());
        
        let mut ink_nodes = 0;
        let mut other_nodes = 0;
        for node in &scene.nodes {
            match &node.primitive {
                onenote_render::ScenePrimitive::Ink { strokes } => {
                    ink_nodes += 1;
                    if ink_nodes <= 5 {
                        println!("  Ink node bounds: {:?}, strokes: {}", node.bounds, strokes.len());
                    }
                }
                _ => { other_nodes += 1; }
            }
        }
        println!("Summary: {} ink nodes, {} other nodes", ink_nodes, other_nodes);
    }
}
