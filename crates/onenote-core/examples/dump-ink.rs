use onenote_core::{OneNoteLoader, ObjectKind, ElementContent};
use std::env;
use std::path::Path;

fn main() {
    let path = env::args().nth(1).expect("missing path");
    let loaded = OneNoteLoader::default().load(Path::new(&path)).expect("failed to load");
    for (page_idx, page) in loaded.notebook.pages().enumerate().take(8) {
        println!("\n=== Page {}: {} (id: {}) ===", page_idx, page.title, page.id);
        for (obj_idx, obj) in page.objects.iter().enumerate() {
            match &obj.kind {
                ObjectKind::Outline(outline) => {
                    println!("  [Obj {}] Outline at {:?}: {} elements", obj_idx, obj.bounds, outline.elements.len());
                    for (el_idx, el) in outline.elements.iter().enumerate() {
                        for c in &el.content {
                            match c {
                                ElementContent::Text(t) => {
                                    if !t.text.is_empty() {
                                        println!("    el {}: Text ({} chars): {:?}", el_idx, t.text.len(), t.text.chars().take(40).collect::<String>());
                                    } else {
                                        println!("    el {}: EMPTY Text", el_idx);
                                    }
                                }
                                ElementContent::Image(img) => println!("    el {}: Image ({:?})", el_idx, img.resource.id),
                                ElementContent::Ink(ink) => println!("    el {}: Ink ({} strokes)", el_idx, ink.strokes.len()),
                                ElementContent::Attachment(_) => println!("    el {}: Attachment", el_idx),
                                ElementContent::Unknown => println!("    el {}: Unknown element", el_idx),
                                _ => println!("    el {}: Other element", el_idx),
                            }
                        }
                    }
                }
                ObjectKind::Image(img) => println!("  [Obj {}] Image at {:?}: {:?}", obj_idx, obj.bounds, img.resource.id),
                ObjectKind::Ink(ink) => println!("  [Obj {}] Ink at {:?}: {} strokes", obj_idx, obj.bounds, ink.strokes.len()),
                ObjectKind::Attachment(_) => println!("  [Obj {}] Attachment at {:?}", obj_idx, obj.bounds),
                ObjectKind::Unknown => println!("  [Obj {}] UNKNOWN OBJECT at {:?}", obj_idx, obj.bounds),
            }
        }
    }
}
