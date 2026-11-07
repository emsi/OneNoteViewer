use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use onenote_core::{Color, OneNoteLoader, Page};
use onenote_render::{PageScene, SceneBuilder, ScenePrimitive};

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn color_to_hex(color: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", color.red, color.green, color.blue)
}

fn page_to_svg(page: &Page, scene: &PageScene) -> (String, usize) {
    let mut min_x = 0.0_f32;
    let mut min_y = 0.0_f32;
    let mut max_x = (scene.bounds.width + 40.0).max(800.0);
    let mut max_y = (scene.bounds.height + 40.0).max(600.0);

    for node in &scene.nodes {
        min_x = min_x.min(node.bounds.x);
        min_y = min_y.min(node.bounds.y);
        max_x = max_x.max(node.bounds.x + node.bounds.width);
        max_y = max_y.max(node.bounds.y + node.bounds.height);
    }

    let margin = 20.0_f32;
    let view_x = min_x - margin;
    let view_y = min_y - margin;
    let view_w = (max_x - min_x) + margin * 2.0;
    let view_h = (max_y - min_y) + margin * 2.0;

    let mut svg = String::new();
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{:.1} {:.1} {:.1} {:.1}\" width=\"{:.0}px\" height=\"{:.0}px\" style=\"background:#ffffff; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif;\">\n",
        view_x, view_y, view_w, view_h, view_w, view_h
    ));

    // Page background
    svg.push_str(&format!(
        "  <rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"#ffffff\" />\n",
        view_x, view_y, view_w, view_h
    ));

    // Title if available
    if !page.title.is_empty() {
        svg.push_str(&format!(
            "  <text x=\"{:.1}\" y=\"{:.1}\" font-size=\"24\" font-weight=\"600\" fill=\"#201f1e\">{}</text>\n",
            (min_x + 10.0).max(view_x + 10.0),
            (min_y + 30.0).max(view_y + 30.0),
            escape_xml(&page.title)
        ));
    }

    let mut stroke_count = 0_usize;

    for node in &scene.nodes {
        match &node.primitive {
            ScenePrimitive::Ink { strokes } => {
                for stroke in strokes {
                    if stroke.points.is_empty() {
                        continue;
                    }
                    stroke_count += 1;
                    let stroke_color = stroke.color.unwrap_or(Color {
                        red: 32,
                        green: 31,
                        blue: 30,
                        alpha: 255,
                    });
                    let hex = color_to_hex(stroke_color);
                    let opacity = (f64::from(stroke.opacity) / 255.0).clamp(0.0, 1.0);
                    let width = stroke.width.max(1.0);

                    if stroke.points.len() == 1 {
                        let pt = &stroke.points[0];
                        svg.push_str(&format!(
                            "  <circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\" fill=\"{}\" opacity=\"{:.2}\" />\n",
                            pt.x, pt.y, width / 2.0, hex, opacity
                        ));
                    } else {
                        let mut d = String::new();
                        let first = &stroke.points[0];
                        d.push_str(&format!("M {:.2} {:.2}", first.x, first.y));
                        for pt in &stroke.points[1..] {
                            d.push_str(&format!(" L {:.2} {:.2}", pt.x, pt.y));
                        }
                        svg.push_str(&format!(
                            "  <path d=\"{}\" stroke=\"{}\" stroke-width=\"{:.2}\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\" opacity=\"{:.2}\" />\n",
                            d, hex, width, opacity
                        ));
                    }
                }
            }
            ScenePrimitive::Text { block, marker } => {
                let start_x = node.bounds.x;
                let start_y = node.bounds.y + 14.0;
                if let Some(m) = marker {
                    svg.push_str(&format!(
                        "  <text x=\"{:.1}\" y=\"{:.1}\" font-size=\"14\" fill=\"#605e5c\">{}</text>\n",
                        start_x - 14.0, start_y, escape_xml(m)
                    ));
                }
                let visible = block.visible_text();
                for (line_idx, line) in visible.lines().enumerate() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let font_size = block.base_style.font_size.unwrap_or(11.0) * 1.33;
                    let color = block.base_style.foreground.unwrap_or(Color {
                        red: 32,
                        green: 31,
                        blue: 30,
                        alpha: 255,
                    });
                    let hex = color_to_hex(color);
                    let bold = if block.base_style.bold { " font-weight=\"bold\"" } else { "" };
                    let italic = if block.base_style.italic { " font-style=\"italic\"" } else { "" };
                    let y = start_y + (line_idx as f32) * (font_size * 1.3);

                    svg.push_str(&format!(
                        "  <text x=\"{:.1}\" y=\"{:.1}\" font-size=\"{:.1}\" fill=\"{}\"{}{}>{}</text>\n",
                        start_x, y, font_size, hex, bold, italic, escape_xml(trimmed)
                    ));
                }
            }
            ScenePrimitive::Line {
                color,
                width,
                to_x,
                to_y,
            } => {
                let hex = color_to_hex(*color);
                svg.push_str(&format!(
                    "  <line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"{:.1}\" />\n",
                    node.bounds.x, node.bounds.y, *to_x, *to_y, hex, width.max(1.0)
                ));
            }
            ScenePrimitive::Fill {
                color,
                corner_radius,
            } => {
                let hex = color_to_hex(*color);
                svg.push_str(&format!(
                    "  <rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\" />\n",
                    node.bounds.x, node.bounds.y, node.bounds.width, node.bounds.height, corner_radius, hex
                ));
            }
            ScenePrimitive::Attachment(attachment) => {
                svg.push_str(&format!(
                    "  <g transform=\"translate({:.1}, {:.1})\"><rect width=\"180\" height=\"36\" rx=\"4\" fill=\"#f3f2f1\" stroke=\"#d2d0ce\"/><text x=\"12\" y=\"22\" font-size=\"12\" fill=\"#323130\">📎 {}</text></g>\n",
                    node.bounds.x, node.bounds.y, escape_xml(&attachment.resource.name)
                ));
            }
            _ => {}
        }
    }

    svg.push_str("</svg>\n");
    (svg, stroke_count)
}

fn generate_html(title: &str, pages: &[(&str, &str, usize)]) -> String {
    let mut html = String::new();
    html.push_str(&format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{} - OneNote Export</title>
<style>
  * {{ box-sizing: border-box; margin: 0; padding: 0; }}
  body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; display: flex; height: 100vh; overflow: hidden; background: #f0f2f5; color: #1c1e21; }}
  #sidebar {{ width: 280px; min-width: 240px; background: #ffffff; border-right: 1px solid #e1e4e8; display: flex; flex-direction: column; overflow: hidden; }}
  #sidebar-header {{ padding: 16px; border-bottom: 1px solid #eaecef; background: #fafbfc; }}
  #sidebar-header h2 {{ font-size: 16px; font-weight: 700; color: #7719aa; margin-bottom: 4px; }}
  #sidebar-header p {{ font-size: 12px; color: #586069; }}
  #page-list {{ list-style: none; overflow-y: auto; flex: 1; }}
  .page-item {{ padding: 12px 16px; border-bottom: 1px solid #f1f1f1; cursor: pointer; transition: background 0.15s; }}
  .page-item:hover {{ background: #f6f8fa; }}
  .page-item.active {{ background: #f1e6f9; border-left: 4px solid #7719aa; font-weight: 600; }}
  .page-title {{ font-size: 13px; margin-bottom: 2px; }}
  .page-meta {{ font-size: 11px; color: #6a737d; }}
  #main {{ flex: 1; display: flex; flex-direction: column; overflow: hidden; }}
  #toolbar {{ height: 50px; background: #ffffff; border-bottom: 1px solid #e1e4e8; display: flex; align-items: center; padding: 0 16px; gap: 12px; }}
  .tool-btn {{ background: #ffffff; border: 1px solid #d1d5da; border-radius: 6px; padding: 6px 12px; font-size: 13px; font-weight: 500; cursor: pointer; transition: background 0.1s; display: inline-flex; align-items: center; gap: 6px; }}
  .tool-btn:hover {{ background: #f3f4f6; }}
  .tool-btn:active {{ background: #e5e7eb; }}
  #page-heading {{ font-size: 14px; font-weight: 600; margin-right: auto; }}
  #canvas-container {{ flex: 1; overflow: auto; padding: 24px; display: flex; justify-content: center; align-items: flex-start; }}
  #canvas-wrapper {{ background: #ffffff; box-shadow: 0 4px 12px rgba(0,0,0,0.08); border-radius: 8px; overflow: hidden; transition: transform 0.15s ease-out; transform-origin: top center; }}
  svg, img {{ display: block; }}
</style>
</head>
<body>
<div id="sidebar">
  <div id="sidebar-header">
    <h2>📓 {}</h2>
    <p>{} pages exported</p>
  </div>
  <ul id="page-list">
"#, escape_xml(title), escape_xml(title), pages.len()));

    for (idx, (page_title, filename, strokes)) in pages.iter().enumerate() {
        let active = if idx == 0 { " active" } else { "" };
        let strokes_label = if *strokes > 0 {
            format!(" • ✏️ {} strokes", strokes)
        } else {
            String::new()
        };
        html.push_str(&format!(
            r#"    <li class="page-item{}" onclick="selectPage({}, '{}', '{}')">
      <div class="page-title">{}</div>
      <div class="page-meta">Page {}{}</div>
    </li>
"#,
            active, idx, escape_xml(page_title), filename,
            escape_xml(page_title), idx + 1, strokes_label
        ));
    }

    html.push_str(&format!(r#"  </ul>
</div>
<div id="main">
  <div id="toolbar">
    <div id="page-heading">{}</div>
    <button class="tool-btn" onclick="zoomIn()">➕ Zoom In</button>
    <button class="tool-btn" onclick="zoomOut()">➖ Zoom Out</button>
    <button class="tool-btn" onclick="resetZoom()">↺ Reset</button>
    <a id="download-btn" class="tool-btn" href="{}" download>⬇️ Save SVG</a>
  </div>
  <div id="canvas-container">
    <div id="canvas-wrapper">
      <img id="page-img" src="{}" alt="OneNote Page" />
    </div>
  </div>
</div>
<script>
  let currentZoom = 1.0;
  function updateZoom() {{
    document.getElementById('canvas-wrapper').style.transform = `scale(${{currentZoom}})`;
  }}
  function zoomIn() {{ currentZoom = Math.min(3.0, currentZoom + 0.15); updateZoom(); }}
  function zoomOut() {{ currentZoom = Math.max(0.2, currentZoom - 0.15); updateZoom(); }}
  function resetZoom() {{ currentZoom = 1.0; updateZoom(); }}

  function selectPage(idx, title, filename) {{
    document.querySelectorAll('.page-item').forEach((item, i) => {{
      item.classList.toggle('active', i === idx);
    }});
    document.getElementById('page-heading').textContent = title;
    document.getElementById('page-img').src = filename;
    document.getElementById('download-btn').href = filename;
    document.getElementById('download-btn').setAttribute('download', filename);
  }}
</script>
</body>
</html>
"#, 
        escape_xml(pages.first().map(|p| p.0).unwrap_or("")),
        pages.first().map(|p| p.1).unwrap_or(""),
        pages.first().map(|p| p.1).unwrap_or("")
    ));

    html
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: onenote-export <file.one or notebook directory> [output_directory]");
        std::process::exit(1);
    }

    let input_path = PathBuf::from(&args[1]);
    let output_dir = if args.len() >= 3 {
        PathBuf::from(&args[2])
    } else {
        let stem = input_path.file_stem().and_then(|s| s.to_str()).unwrap_or("export");
        input_path.parent().unwrap_or(Path::new(".")).join(format!("{}_export", stem))
    };

    println!("Loading: {}", input_path.display());
    let loaded = OneNoteLoader::default().load(&input_path)?;
    fs::create_dir_all(&output_dir)?;

    let cancel = AtomicBool::new(false);
    let mut total_pages = 0;
    let mut total_strokes = 0;

    for section in loaded.notebook.sections() {
        println!("Section: {}", section.name);
        let mut export_pages = Vec::new();

        for (p_idx, page) in section.pages.iter().enumerate() {
            total_pages += 1;
            let display_title = if page.title.trim().is_empty() {
                format!("Untitled Page {}", p_idx + 1)
            } else {
                page.title.clone()
            };

            let scene = SceneBuilder::default().build(page, &cancel)?;
            let (svg_content, strokes) = page_to_svg(page, &scene);
            total_strokes += strokes;

            let safe_title = display_title
                .replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-', "_");
            let svg_filename = format!("page_{:02}_{}.svg", p_idx + 1, safe_title);
            let svg_file_path = output_dir.join(&svg_filename);

            let mut f = File::create(&svg_file_path)?;
            f.write_all(svg_content.as_bytes())?;

            println!("  Exported page {} ({} strokes): {}", p_idx + 1, strokes, svg_filename);
            export_pages.push((display_title, svg_filename, strokes));
        }

        let html = generate_html(&section.name, &export_pages.iter().map(|(t, f, s)| (t.as_str(), f.as_str(), *s)).collect::<Vec<_>>());
        let html_path = output_dir.join("index.html");
        let mut hf = File::create(&html_path)?;
        hf.write_all(html.as_bytes())?;
        println!("  Created interactive HTML viewer: {}", html_path.display());
    }

    println!("\nExport complete!");
    println!("Total pages: {}", total_pages);
    println!("Total strokes: {}", total_strokes);
    println!("Output folder: {}", output_dir.display());
    println!("Open in browser: xdg-open \"{}/index.html\"", output_dir.display());

    Ok(())
}
