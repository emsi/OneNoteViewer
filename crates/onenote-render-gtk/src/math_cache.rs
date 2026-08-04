use num_traits::ToPrimitive;
use onenote_core::{Color, MathExpression, MathSpan, TextStyle};
use onenote_render::{to_typst_math, MathLayoutBackend, MathLayoutRequest, MathRaster};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};
use typst::foundations::Bytes;
use typst::text::Font;
use typst_as_lib::TypstTemplate;

const LOGICAL_PIXELS_PER_POINT: f32 = 96.0 / 72.0;
const MAX_MATH_DIMENSION: u32 = 16_384;
const MAX_MATH_RASTER_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Eq)]
pub(crate) struct MathKey {
    source: Arc<str>,
    font_size_bits: u32,
    color: u32,
    display: bool,
    pixels_per_point_bits: u32,
}

impl PartialEq for MathKey {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
            && self.font_size_bits == other.font_size_bits
            && self.color == other.color
            && self.display == other.display
            && self.pixels_per_point_bits == other.pixels_per_point_bits
    }
}

impl Hash for MathKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.source.hash(state);
        self.font_size_bits.hash(state);
        self.color.hash(state);
        self.display.hash(state);
        self.pixels_per_point_bits.hash(state);
    }
}

impl MathKey {
    pub(crate) fn new(
        span: &MathSpan,
        style: &TextStyle,
        default_color: Color,
        zoom: f32,
    ) -> Option<Self> {
        let expression = span.expression.as_ref()?;
        let font_size = style.font_size.unwrap_or(11.0).clamp(4.0, 144.0);
        let color = style.foreground.unwrap_or(default_color);
        let render_scale = zoom.clamp(1.0, 4.0);
        Some(Self {
            source: Arc::from(to_typst_math(expression)),
            font_size_bits: font_size.to_bits(),
            color: rgba_u32(color),
            display: span.display,
            pixels_per_point_bits: (LOGICAL_PIXELS_PER_POINT * render_scale).to_bits(),
        })
    }

    fn request<'a>(&self, expression: &'a MathExpression) -> MathLayoutRequest<'a> {
        MathLayoutRequest {
            expression,
            font_size: f32::from_bits(self.font_size_bits),
            color: color_from_u32(self.color),
            display: self.display,
            pixels_per_point: f32::from_bits(self.pixels_per_point_bits),
        }
    }

    pub(crate) fn estimated_size(&self) -> MathSize {
        let font_size = f32::from_bits(self.font_size_bits);
        let characters =
            f32::from(u16::try_from(self.source.chars().count().clamp(1, 200)).unwrap_or(200));
        MathSize {
            width: (characters.sqrt() * font_size * 2.2).max(font_size),
            height: if self.display {
                font_size * 2.8
            } else {
                font_size * 2.0
            },
            baseline: font_size * 1.45,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MathSize {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) baseline: f32,
}

/// Offline native math backend using Typst and fonts embedded in the binary.
#[derive(Clone, Copy, Debug, Default)]
pub struct TypstMathBackend;

impl TypstMathBackend {
    /// Construct the stateless default math backend.
    pub fn new() -> Self {
        Self
    }
}

impl MathLayoutBackend for TypstMathBackend {
    fn render(&self, request: MathLayoutRequest<'_>) -> Result<MathRaster, String> {
        let font_size = finite(request.font_size, 11.0).clamp(4.0, 144.0);
        let pixels_per_point = finite(request.pixels_per_point, 2.0).clamp(1.0, 8.0);
        let color = request.color;
        let expression = to_typst_math(request.expression);
        let display = if request.display { "true" } else { "false" };
        let source = format!(
            "#set page(width: auto, height: auto, margin: 1pt, fill: none)\n\
             #set text(size: {font_size}pt, fill: rgb(\"#{:02x}{:02x}{:02x}\"))\n\
             #math.equation(block: {display}, ${expression}$)",
            color.red, color.green, color.blue
        );
        let template = TypstTemplate::new(embedded_fonts().clone(), source);
        let compiled = template.compile();
        let document = compiled.output.map_err(|diagnostics| {
            bounded_diagnostic(format!("Typst rejected generated math: {diagnostics:?}"))
        })?;
        let page = document
            .pages
            .first()
            .ok_or_else(|| "Typst produced no page for the math expression".to_owned())?;
        let pixmap = typst_render::render(page, pixels_per_point);
        let width = pixmap.width();
        let height = pixmap.height();
        let rgba = pixmap.take();
        let expected = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .map(|height| width * height * 4)
            })
            .ok_or_else(|| "math raster dimensions overflow".to_owned())?;
        if width == 0 || height == 0 || width > MAX_MATH_DIMENSION || height > MAX_MATH_DIMENSION {
            return Err("math raster dimensions are outside supported limits".to_owned());
        }
        if expected != rgba.len() || rgba.len() > MAX_MATH_RASTER_BYTES {
            return Err("math raster exceeds the decoded-size limit".to_owned());
        }
        let logical_scale = pixels_per_point / LOGICAL_PIXELS_PER_POINT;
        let logical_width = width.to_f32().unwrap_or(0.0) / logical_scale;
        let logical_height = height.to_f32().unwrap_or(0.0) / logical_scale;
        Ok(MathRaster {
            width,
            height,
            rgba,
            logical_width,
            logical_height,
            baseline: logical_height * 0.78,
        })
    }
}

pub(crate) fn spawn_render(
    backend: Arc<dyn MathLayoutBackend>,
    key: MathKey,
    expression: MathExpression,
    callback: impl FnOnce(MathKey, Result<MathRaster, String>) + Send + 'static,
) {
    std::thread::spawn(move || {
        let result = backend.render(key.request(&expression));
        callback(key, result);
    });
}

fn embedded_fonts() -> &'static Vec<Font> {
    static FONTS: OnceLock<Vec<Font>> = OnceLock::new();
    FONTS.get_or_init(|| {
        typst_assets::fonts()
            .flat_map(|bytes| Font::iter(Bytes::from_static(bytes)))
            .collect()
    })
}

fn rgba_u32(color: Color) -> u32 {
    u32::from_be_bytes([color.red, color.green, color.blue, color.alpha])
}

fn color_from_u32(color: u32) -> Color {
    let [red, green, blue, alpha] = color.to_be_bytes();
    Color {
        red,
        green,
        blue,
        alpha,
    }
}

fn finite(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

fn bounded_diagnostic(mut diagnostic: String) -> String {
    let boundary = diagnostic
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= 512)
        .last()
        .unwrap_or(0);
    if diagnostic.len() > 512 {
        diagnostic.truncate(boundary);
    }
    diagnostic
}

#[cfg(test)]
mod tests {
    use super::TypstMathBackend;
    use onenote_core::{
        Color, ElementContent, MathExpression, MathNode, MathSpan, ObjectKind, OneNoteLoader,
        OnePkgExtractor, OutlineElement,
    };
    use onenote_render::{MathLayoutBackend, MathLayoutRequest};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn embedded_backend_renders_structured_fraction() {
        let text = |value: &str| MathExpression {
            nodes: vec![MathNode::Text {
                value: value.to_owned(),
            }],
        };
        let expression = MathExpression {
            nodes: vec![MathNode::Fraction {
                small: false,
                numerator: text("𝑛𝑥"),
                denominator: text("1!"),
            }],
        };

        let raster = TypstMathBackend
            .render(MathLayoutRequest {
                expression: &expression,
                font_size: 18.0,
                color: Color {
                    red: 0,
                    green: 0,
                    blue: 0,
                    alpha: 255,
                },
                display: true,
                pixels_per_point: 2.0,
            })
            .expect("math raster");

        assert!(raster.width > 8);
        assert!(raster.height > 8);
        assert!(raster.rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn embedded_backend_accepts_every_supported_node_family() {
        let text = |value: &str| MathExpression {
            nodes: vec![MathNode::Text {
                value: value.to_owned(),
            }],
        };
        let x = text("𝑥");
        let y = text("𝑦");
        let cases = vec![
            (
                "accent",
                MathNode::Accent {
                    character: Some('^'),
                    body: x.clone(),
                },
            ),
            (
                "box",
                MathNode::Box {
                    align: None,
                    body: x.clone(),
                },
            ),
            (
                "boxed",
                MathNode::BoxedFormula {
                    align: None,
                    body: x.clone(),
                },
            ),
            (
                "brackets",
                MathNode::Brackets {
                    open: Some('('),
                    close: Some(')'),
                    align: None,
                    body: x.clone(),
                },
            ),
            (
                "separators",
                MathNode::BracketsWithSeparators {
                    open: Some('('),
                    close: Some(')'),
                    separator: Some(','),
                    align: None,
                    segments: vec![x.clone(), y.clone()],
                },
            ),
            (
                "array",
                MathNode::EquationArray {
                    columns: Some(1),
                    align: None,
                    rows: vec![x.clone(), y.clone()],
                },
            ),
            (
                "fraction",
                MathNode::Fraction {
                    small: false,
                    numerator: x.clone(),
                    denominator: y.clone(),
                },
            ),
            (
                "function",
                MathNode::FunctionApply {
                    function: text("sin"),
                    argument: x.clone(),
                },
            ),
            (
                "left_scripts",
                MathNode::LeftSubSup {
                    subscript: text("1"),
                    superscript: text("2"),
                    body: x.clone(),
                },
            ),
            (
                "lower_limit",
                MathNode::LowerLimit {
                    body: text("lim"),
                    limit: text("0"),
                },
            ),
            (
                "matrix",
                MathNode::Matrix {
                    columns: Some(2),
                    bracket: Some('('),
                    align: None,
                    items: vec![x.clone(), y.clone()],
                },
            ),
            (
                "nary",
                MathNode::Nary {
                    operator: Some('∑'),
                    align: None,
                    subscript: text("1"),
                    superscript: text("∞"),
                    body: x.clone(),
                },
            ),
            (
                "operator",
                MathNode::Operator {
                    character: Some('±'),
                },
            ),
            ("overbar", MathNode::Overbar { body: x.clone() }),
            (
                "phantom",
                MathNode::Phantom {
                    kind: None,
                    align: None,
                    body: x.clone(),
                },
            ),
            (
                "radical",
                MathNode::Radical {
                    degree: text("3"),
                    body: x.clone(),
                },
            ),
            (
                "slashed",
                MathNode::SlashedFraction {
                    linear: true,
                    numerator: x.clone(),
                    denominator: y.clone(),
                },
            ),
            (
                "stack",
                MathNode::Stack {
                    upper: x.clone(),
                    lower: y.clone(),
                },
            ),
            (
                "stretch",
                MathNode::StretchStack {
                    character: Some('¯'),
                    align: None,
                    body: x.clone(),
                },
            ),
            (
                "subscript",
                MathNode::Subscript {
                    body: x.clone(),
                    subscript: text("1"),
                },
            ),
            (
                "sub_sup",
                MathNode::SubSup {
                    align: None,
                    body: x.clone(),
                    subscript: text("1"),
                    superscript: text("2"),
                },
            ),
            (
                "superscript",
                MathNode::Superscript {
                    body: x.clone(),
                    superscript: text("2"),
                },
            ),
            ("underbar", MathNode::Underbar { body: x.clone() }),
            (
                "upper_limit",
                MathNode::UpperLimit {
                    body: text("lim"),
                    limit: text("∞"),
                },
            ),
        ];

        for (name, node) in cases {
            let expression = MathExpression { nodes: vec![node] };
            TypstMathBackend
                .render(MathLayoutRequest {
                    expression: &expression,
                    font_size: 16.0,
                    color: Color {
                        red: 0,
                        green: 0,
                        blue: 0,
                        alpha: 255,
                    },
                    display: true,
                    pixels_per_point: 1.5,
                })
                .unwrap_or_else(|error| panic!("{name}: {error}"));
        }
    }

    #[test]
    fn private_math_corpus_renders_every_equation() {
        let Some(package) = std::env::var_os("ONENOTE_MATH_TEST_PACKAGE").map(PathBuf::from) else {
            return;
        };
        if !package.is_file() {
            return;
        }
        let Ok(extractor) = OnePkgExtractor::detect() else {
            return;
        };
        let temporary = tempfile::tempdir().expect("temporary extraction parent");
        let destination = temporary.path().join("notebook");
        extractor
            .extract(&package, &destination, &AtomicBool::new(false))
            .expect("private math package must extract");
        let section = first_section(&destination).expect("private math section");
        let loaded = OneNoteLoader::default()
            .load(section)
            .expect("private math section must project");
        let mut spans = Vec::new();
        for page in loaded.notebook.pages() {
            let mut page_spans = Vec::new();
            for object in &page.objects {
                if let ObjectKind::Outline(outline) = &object.kind {
                    collect_math(&outline.elements, &mut page_spans);
                }
            }
            if page_spans.len() > spans.len() {
                spans = page_spans;
            }
        }
        assert_eq!(spans.len(), 3);
        for span in spans {
            let expression = span.expression.as_ref().expect("decoded expression");
            let raster = TypstMathBackend
                .render(MathLayoutRequest {
                    expression,
                    font_size: 18.0,
                    color: Color {
                        red: 245,
                        green: 245,
                        blue: 245,
                        alpha: 255,
                    },
                    display: span.display,
                    pixels_per_point: 2.0,
                })
                .unwrap_or_else(|error| panic!("{}: {error}", span.visible_text()));
            assert!(raster.width > 8);
            assert!(raster.height > 8);
            assert!(raster.rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
        }
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

    fn first_section(root: &Path) -> Option<PathBuf> {
        std::fs::read_dir(root)
            .ok()?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .find(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("one"))
            })
    }
}
