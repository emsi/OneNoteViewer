use num_traits::ToPrimitive;
use onenote_core::{Color, MathExpression, MathSpan, TextStyle};
use onenote_render::{to_typst_math, MathLayoutBackend, MathLayoutRequest, MathRaster};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};
use typst::foundations::Bytes;
use typst::layout::{Frame, FrameItem};
use typst::text::Font;
use typst_as_lib::TypstTemplate;

const LOGICAL_PIXELS_PER_POINT: f32 = 96.0 / 72.0;
const MAX_MATH_DIMENSION: u32 = 16_384;
const MAX_MATH_RASTER_BYTES: usize = 64 * 1024 * 1024;
const MAX_GUARD_ATTEMPTS: usize = 4;
const LOGICAL_CROP_PADDING: f32 = 1.0;

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
        let equation = format!("#math.equation(block: {display}, ${expression}$)");
        let logical_scale = pixels_per_point / LOGICAL_PIXELS_PER_POINT;
        let crop_padding = (LOGICAL_CROP_PADDING * logical_scale)
            .ceil()
            .to_u32()
            .unwrap_or(1)
            .max(1);
        let mut guard = font_size;

        for attempt in 0..MAX_GUARD_ATTEMPTS {
            let source = format!(
                "#set page(width: auto, height: auto, margin: {guard}pt, fill: none)\n\
                 #set text(size: {font_size}pt, fill: rgb(\"#{:02x}{:02x}{:02x}\"))\n\
                 {equation}",
                color.red, color.green, color.blue,
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
            preflight_page(
                page.frame.width().to_pt(),
                page.frame.height().to_pt(),
                pixels_per_point,
            )?;
            let frame = baseline_frame(&page.frame, 0.0, 0.0)
                .unwrap_or_else(|| page_content_frame(&page.frame, guard, request.display));
            let pixmap = typst_render::render(page, pixels_per_point);
            let width = pixmap.width();
            let height = pixmap.height();
            let rgba = pixmap.take();
            validate_raster(width, height, rgba.len())?;
            let ink = alpha_bounds(&rgba, width, height);
            if ink.is_some_and(|bounds| touches_guard(bounds, width, height, crop_padding)) {
                if attempt + 1 == MAX_GUARD_ATTEMPTS {
                    return Err("math ink exceeds the guarded raster bounds".to_owned());
                }
                guard *= 2.0;
                continue;
            }
            return crop_raster(
                &rgba,
                width,
                height,
                frame,
                ink,
                crop_padding,
                pixels_per_point,
                logical_scale,
            );
        }
        unreachable!("the bounded guard loop returns or reports an error")
    }
}

#[derive(Clone, Copy, Debug)]
struct FrameMetrics {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    baseline: f64,
}

#[derive(Clone, Copy, Debug)]
struct PixelBounds {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

impl PixelBounds {
    fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    fn padded(self, padding: u32, width: u32, height: u32) -> Self {
        Self {
            left: self.left.saturating_sub(padding),
            top: self.top.saturating_sub(padding),
            right: self.right.saturating_add(padding).min(width),
            bottom: self.bottom.saturating_add(padding).min(height),
        }
    }

    fn width(self) -> u32 {
        self.right.saturating_sub(self.left)
    }

    fn height(self) -> u32 {
        self.bottom.saturating_sub(self.top)
    }
}

fn baseline_frame(frame: &Frame, x: f64, y: f64) -> Option<FrameMetrics> {
    if frame.has_baseline() {
        return Some(FrameMetrics {
            x,
            y,
            width: frame.width().to_pt(),
            height: frame.height().to_pt(),
            baseline: frame.baseline().to_pt(),
        });
    }
    frame.items().find_map(|(point, item)| {
        let FrameItem::Group(group) = item else {
            return None;
        };
        baseline_frame(&group.frame, x + point.x.to_pt(), y + point.y.to_pt())
    })
}

fn page_content_frame(frame: &Frame, guard: f32, display: bool) -> FrameMetrics {
    let guard = f64::from(guard);
    let width = (frame.width().to_pt() - guard * 2.0).max(1.0);
    let height = (frame.height().to_pt() - guard * 2.0).max(1.0);
    FrameMetrics {
        x: guard,
        y: guard,
        width,
        height,
        // Typst may flatten a small soft line frame into the page and discard
        // its explicit baseline. An inline auto-sized line ends at its
        // baseline; display math does not participate in surrounding text.
        baseline: if display { height / 2.0 } else { height },
    }
}

fn preflight_page(
    width_points: f64,
    height_points: f64,
    pixels_per_point: f32,
) -> Result<(), String> {
    let scale = f64::from(pixels_per_point);
    let width = (width_points * scale)
        .ceil()
        .to_u32()
        .ok_or_else(|| "math raster dimensions overflow".to_owned())?;
    let height = (height_points * scale)
        .ceil()
        .to_u32()
        .ok_or_else(|| "math raster dimensions overflow".to_owned())?;
    validate_dimensions(width, height).map(|_| ())
}

fn validate_raster(width: u32, height: u32, actual_bytes: usize) -> Result<(), String> {
    let expected = validate_dimensions(width, height)?;
    if expected != actual_bytes {
        return Err("math raster byte count does not match its dimensions".to_owned());
    }
    Ok(())
}

fn validate_dimensions(width: u32, height: u32) -> Result<usize, String> {
    if width == 0 || height == 0 || width > MAX_MATH_DIMENSION || height > MAX_MATH_DIMENSION {
        return Err("math raster dimensions are outside supported limits".to_owned());
    }
    let bytes = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "math raster dimensions overflow".to_owned())?;
    if bytes > MAX_MATH_RASTER_BYTES {
        return Err("math raster exceeds the decoded-size limit".to_owned());
    }
    Ok(bytes)
}

fn alpha_bounds(rgba: &[u8], width: u32, height: u32) -> Option<PixelBounds> {
    let mut bounds = None;
    for y in 0..height {
        for x in 0..width {
            let alpha = usize::try_from((y * width + x) * 4 + 3)
                .ok()
                .and_then(|offset| rgba.get(offset))
                .copied()
                .unwrap_or(0);
            if alpha == 0 {
                continue;
            }
            let pixel = PixelBounds {
                left: x,
                top: y,
                right: x + 1,
                bottom: y + 1,
            };
            bounds = Some(bounds.map_or(pixel, |current: PixelBounds| current.union(pixel)));
        }
    }
    bounds
}

fn touches_guard(bounds: PixelBounds, width: u32, height: u32, padding: u32) -> bool {
    bounds.left <= padding
        || bounds.top <= padding
        || width.saturating_sub(bounds.right) <= padding
        || height.saturating_sub(bounds.bottom) <= padding
}

#[allow(clippy::too_many_arguments)]
fn crop_raster(
    rgba: &[u8],
    width: u32,
    height: u32,
    frame: FrameMetrics,
    ink: Option<PixelBounds>,
    padding: u32,
    pixels_per_point: f32,
    logical_scale: f32,
) -> Result<MathRaster, String> {
    let frame_bounds = frame_pixel_bounds(frame, pixels_per_point, width, height)?;
    let content = ink.map_or(frame_bounds, |ink| ink.union(frame_bounds));
    let crop = content.padded(padding, width, height);
    let cropped_width = crop.width();
    let cropped_height = crop.height();
    let expected = validate_dimensions(cropped_width, cropped_height)?;
    let source_stride = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| "math raster row dimensions overflow".to_owned())?;
    let row_bytes = usize::try_from(cropped_width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| "math raster row dimensions overflow".to_owned())?;
    let left_bytes = usize::try_from(crop.left)
        .ok()
        .and_then(|left| left.checked_mul(4))
        .ok_or_else(|| "math raster crop offset overflow".to_owned())?;
    let mut cropped = Vec::with_capacity(expected);
    for y in crop.top..crop.bottom {
        let row = usize::try_from(y)
            .ok()
            .and_then(|y| y.checked_mul(source_stride))
            .and_then(|row| row.checked_add(left_bytes))
            .ok_or_else(|| "math raster crop offset overflow".to_owned())?;
        let end = row
            .checked_add(row_bytes)
            .ok_or_else(|| "math raster crop offset overflow".to_owned())?;
        cropped.extend_from_slice(
            rgba.get(row..end)
                .ok_or_else(|| "math raster crop is outside the source image".to_owned())?,
        );
    }
    if cropped.len() != expected {
        return Err("cropped math raster byte count is inconsistent".to_owned());
    }
    let baseline_pixels = ((frame.y + frame.baseline) * f64::from(pixels_per_point))
        .to_f32()
        .ok_or_else(|| "math baseline is outside supported limits".to_owned())?
        - crop.top.to_f32().unwrap_or(0.0);
    let logical_width = cropped_width.to_f32().unwrap_or(0.0) / logical_scale;
    let logical_height = cropped_height.to_f32().unwrap_or(0.0) / logical_scale;
    let baseline = baseline_pixels / logical_scale;
    if !baseline.is_finite() || baseline <= 0.0 || baseline >= logical_height {
        return Err("Typst produced an invalid math baseline".to_owned());
    }
    Ok(MathRaster {
        width: cropped_width,
        height: cropped_height,
        rgba: cropped,
        logical_width,
        logical_height,
        baseline,
    })
}

fn frame_pixel_bounds(
    frame: FrameMetrics,
    pixels_per_point: f32,
    width: u32,
    height: u32,
) -> Result<PixelBounds, String> {
    let scale = f64::from(pixels_per_point);
    let left = positive_floor(frame.x * scale, width)?;
    let top = positive_floor(frame.y * scale, height)?;
    let right = positive_ceil((frame.x + frame.width) * scale, width)?;
    let bottom = positive_ceil((frame.y + frame.height) * scale, height)?;
    if left >= right || top >= bottom {
        return Err("Typst produced empty math frame bounds".to_owned());
    }
    Ok(PixelBounds {
        left,
        top,
        right,
        bottom,
    })
}

fn positive_floor(value: f64, maximum: u32) -> Result<u32, String> {
    if !value.is_finite() {
        return Err("math frame coordinates are not finite".to_owned());
    }
    Ok(value
        .floor()
        .max(0.0)
        .to_u32()
        .unwrap_or(maximum)
        .min(maximum))
}

fn positive_ceil(value: f64, maximum: u32) -> Result<u32, String> {
    if !value.is_finite() {
        return Err("math frame coordinates are not finite".to_owned());
    }
    Ok(value
        .ceil()
        .max(0.0)
        .to_u32()
        .unwrap_or(maximum)
        .min(maximum))
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
    use super::{
        validate_dimensions, TypstMathBackend, LOGICAL_PIXELS_PER_POINT, MAX_MATH_DIMENSION,
        MAX_MATH_RASTER_BYTES,
    };
    use onenote_core::{
        Color, ElementContent, MathExpression, MathNode, MathSpan, ObjectKind, OneNoteLoader,
        OnePkgExtractor, OutlineElement,
    };
    use onenote_render::{MathLayoutBackend, MathLayoutRequest, MathRaster};
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

        assert_safe_raster(&raster);
        assert!(raster.rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn inline_fraction_retains_overflowing_ink_and_baseline() {
        let text = |value: &str| MathExpression {
            nodes: vec![MathNode::Text {
                value: value.to_owned(),
            }],
        };
        let expression = MathExpression {
            nodes: vec![
                MathNode::Text {
                    value: "f(x) = ".to_owned(),
                },
                MathNode::Fraction {
                    small: false,
                    numerator: text("x + 1"),
                    denominator: text("x - 1"),
                },
            ],
        };

        let raster = render(&expression, 11.0, false, LOGICAL_PIXELS_PER_POINT);

        assert_safe_raster(&raster);
        assert!(raster.logical_height > 11.0);
    }

    #[test]
    fn nested_inline_math_is_safe_across_font_sizes_and_scales() {
        let text = |value: &str| MathExpression {
            nodes: vec![MathNode::Text {
                value: value.to_owned(),
            }],
        };
        let inner = MathExpression {
            nodes: vec![MathNode::Fraction {
                small: false,
                numerator: text("a"),
                denominator: text("b"),
            }],
        };
        let expression = MathExpression {
            nodes: vec![MathNode::Radical {
                degree: text("3"),
                body: MathExpression {
                    nodes: vec![MathNode::Fraction {
                        small: false,
                        numerator: inner,
                        denominator: text("c"),
                    }],
                },
            }],
        };

        for (font_size, pixels_per_point) in [
            (4.0, LOGICAL_PIXELS_PER_POINT),
            (11.0, LOGICAL_PIXELS_PER_POINT * 4.0),
            (144.0, LOGICAL_PIXELS_PER_POINT),
        ] {
            let raster = render(&expression, font_size, false, pixels_per_point);
            assert_safe_raster(&raster);
        }

        let first = render(&expression, 11.0, false, LOGICAL_PIXELS_PER_POINT);
        let second = render(&expression, 11.0, false, LOGICAL_PIXELS_PER_POINT);
        assert_eq!((first.width, first.height), (second.width, second.height));
        assert_eq!(first.baseline.to_bits(), second.baseline.to_bits());
        assert_eq!(first.rgba, second.rgba);
    }

    #[test]
    fn transparent_phantom_retains_logical_space() {
        let expression = MathExpression {
            nodes: vec![MathNode::Phantom {
                kind: None,
                align: None,
                body: MathExpression {
                    nodes: vec![MathNode::Text {
                        value: "reserved".to_owned(),
                    }],
                },
            }],
        };

        let raster = render(&expression, 11.0, false, LOGICAL_PIXELS_PER_POINT);

        assert_safe_raster(&raster);
        assert!(raster.logical_width > 1.0);
        assert!(raster.logical_height > 1.0);
    }

    #[test]
    fn raster_limits_are_enforced_before_allocation() {
        assert_eq!(validate_dimensions(1, 1), Ok(4));
        assert!(validate_dimensions(MAX_MATH_DIMENSION + 1, 1).is_err());
        let oversized_square = (MAX_MATH_RASTER_BYTES / 4).isqrt().saturating_add(1);
        let oversized_square = u32::try_from(oversized_square).unwrap();
        assert!(oversized_square <= MAX_MATH_DIMENSION);
        assert!(validate_dimensions(oversized_square, oversized_square).is_err());
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
            for display in [false, true] {
                let raster = TypstMathBackend
                    .render(MathLayoutRequest {
                        expression: &expression,
                        font_size: 16.0,
                        color: Color {
                            red: 0,
                            green: 0,
                            blue: 0,
                            alpha: 255,
                        },
                        display,
                        pixels_per_point: 1.5,
                    })
                    .unwrap_or_else(|error| panic!("{name} display={display}: {error}"));
                assert_safe_raster(&raster);
            }
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
            assert_safe_raster(&raster);
            assert!(raster.rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
        }
    }

    fn render(
        expression: &MathExpression,
        font_size: f32,
        display: bool,
        pixels_per_point: f32,
    ) -> MathRaster {
        TypstMathBackend
            .render(MathLayoutRequest {
                expression,
                font_size,
                color: Color {
                    red: 0,
                    green: 0,
                    blue: 0,
                    alpha: 255,
                },
                display,
                pixels_per_point,
            })
            .expect("math raster")
    }

    fn assert_safe_raster(raster: &MathRaster) {
        assert!(raster.width > 0);
        assert!(raster.height > 0);
        assert_eq!(
            raster.rgba.len(),
            usize::try_from(raster.width).unwrap() * usize::try_from(raster.height).unwrap() * 4
        );
        assert!(raster.logical_width.is_finite() && raster.logical_width > 0.0);
        assert!(raster.logical_height.is_finite() && raster.logical_height > 0.0);
        assert!(raster.baseline.is_finite());
        assert!(raster.baseline > 0.0 && raster.baseline < raster.logical_height);
        assert!(transparent_horizontal_edge(raster, 0));
        assert!(transparent_horizontal_edge(raster, raster.height - 1));
        assert!(transparent_vertical_edge(raster, 0));
        assert!(transparent_vertical_edge(raster, raster.width - 1));
    }

    fn transparent_horizontal_edge(raster: &MathRaster, y: u32) -> bool {
        (0..raster.width).all(|x| alpha(raster, x, y) == 0)
    }

    fn transparent_vertical_edge(raster: &MathRaster, x: u32) -> bool {
        (0..raster.height).all(|y| alpha(raster, x, y) == 0)
    }

    fn alpha(raster: &MathRaster, x: u32, y: u32) -> u8 {
        let offset = usize::try_from((y * raster.width + x) * 4 + 3).unwrap();
        raster.rgba[offset]
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
