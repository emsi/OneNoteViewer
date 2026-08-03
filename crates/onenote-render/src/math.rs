use onenote_core::{Color, MathExpression, MathNode};

/// Input accepted by a replaceable mathematical layout backend.
#[derive(Clone, Copy, Debug)]
pub struct MathLayoutRequest<'a> {
    /// Canonical `OfficeMath` expression produced by `onenote-core`.
    pub expression: &'a MathExpression,
    /// Requested text size in typographic points.
    pub font_size: f32,
    /// Requested foreground color.
    pub color: Color,
    /// Whether the expression occupies a complete paragraph.
    pub display: bool,
    /// Raster pixels per typographic point.
    pub pixels_per_point: f32,
}

/// Toolkit-independent RGBA output from a mathematical layout backend.
#[derive(Clone, Debug)]
pub struct MathRaster {
    /// Raster width in pixels.
    pub width: u32,
    /// Raster height in pixels.
    pub height: u32,
    /// RGBA8 pixels with `width * 4` bytes per row.
    pub rgba: Vec<u8>,
    /// Width occupied in the host's logical coordinate system.
    pub logical_width: f32,
    /// Height occupied in the host's logical coordinate system.
    pub logical_height: f32,
    /// Distance from the top edge to the inline baseline.
    pub baseline: f32,
}

/// Replaceable native mathematical layout backend.
pub trait MathLayoutBackend: Send + Sync {
    /// Render one canonical expression without consulting external resources.
    ///
    /// # Errors
    ///
    /// Returns a bounded diagnostic when the backend cannot render the input.
    fn render(&self, request: MathLayoutRequest<'_>) -> Result<MathRaster, String>;
}

/// Convert the canonical math AST to a self-contained Typst math expression.
///
/// Literal notebook text is always emitted through Typst's `text` function, so
/// it cannot introduce code, package imports, or file access.
pub fn to_typst_math(expression: &MathExpression) -> String {
    let mut output = String::new();
    append_expression(&mut output, expression);
    if output.is_empty() {
        output.push_str("text(\" \" )");
    }
    output
}

fn append_expression(output: &mut String, expression: &MathExpression) {
    for node in &expression.nodes {
        append_node(output, node);
    }
}

#[allow(clippy::too_many_lines)]
fn append_node(output: &mut String, node: &MathNode) {
    match node {
        MathNode::Text { value } => push_text(output, value),
        MathNode::Accent { character, body } => {
            output.push_str("accent(");
            grouped(output, body);
            output.push(',');
            push_character(output, *character, '^');
            output.push(')');
        }
        MathNode::Box { body, .. } => grouped(output, body),
        MathNode::BoxedFormula { body, .. } => {
            output.push_str("rect(");
            grouped(output, body);
            output.push(')');
        }
        MathNode::Brackets {
            open, close, body, ..
        } => delimiters(output, *open, *close, std::slice::from_ref(body), None),
        MathNode::BracketsWithSeparators {
            open,
            close,
            separator,
            segments,
            ..
        } => delimiters(output, *open, *close, segments, *separator),
        MathNode::EquationArray { rows, .. } => {
            output.push_str("mat(");
            separated_expressions(output, rows, ";");
            output.push(')');
        }
        MathNode::Fraction {
            numerator,
            denominator,
            ..
        } => fraction(output, numerator, denominator),
        MathNode::FunctionApply { function, argument } => {
            grouped(output, function);
            grouped(output, argument);
        }
        MathNode::LeftSubSup {
            subscript,
            superscript,
            body,
        } => {
            output.push_str("attach(");
            grouped(output, body);
            output.push_str(", bl: ");
            grouped(output, subscript);
            output.push_str(", tl: ");
            grouped(output, superscript);
            output.push(')');
        }
        MathNode::LowerLimit { body, limit } => {
            output.push_str("limits(");
            grouped(output, body);
            output.push_str(")_(");
            append_expression(output, limit);
            output.push(')');
        }
        MathNode::Matrix {
            columns,
            bracket,
            items,
            ..
        } => {
            let columns = usize::from(columns.unwrap_or(1).max(1));
            let delimiters = matrix_delimiters(*bracket);
            if let Some((open, _)) = delimiters {
                output.push_str("lr(");
                push_text(output, open);
            }
            output.push_str("mat(");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push(if index % columns == 0 { ';' } else { ',' });
                }
                grouped(output, item);
            }
            output.push(')');
            if let Some((_, close)) = delimiters {
                push_text(output, close);
                output.push(')');
            }
        }
        MathNode::Nary {
            operator,
            subscript,
            superscript,
            body,
            ..
        } => {
            output.push_str("limits(");
            push_nary_operator(output, *operator);
            output.push_str(")_(");
            append_expression(output, subscript);
            output.push_str(")^(");
            append_expression(output, superscript);
            output.push(')');
            grouped(output, body);
        }
        MathNode::Operator { character } => push_character(output, *character, ' '),
        MathNode::Overbar { body } => unary(output, "overline", body),
        MathNode::Phantom { body, .. } => {
            output.push_str("#hide($");
            append_expression(output, body);
            output.push_str("$)");
        }
        MathNode::Radical { degree, body } => {
            if degree.nodes.is_empty() {
                unary(output, "sqrt", body);
            } else {
                output.push_str("root(");
                grouped(output, degree);
                output.push(',');
                grouped(output, body);
                output.push(')');
            }
        }
        MathNode::SlashedFraction {
            linear,
            numerator,
            denominator,
        } => {
            grouped(output, numerator);
            push_text(output, if *linear { "∕" } else { "/" });
            grouped(output, denominator);
        }
        MathNode::Stack { upper, lower } => {
            output.push_str("binom(");
            grouped(output, upper);
            output.push(',');
            grouped(output, lower);
            output.push(')');
        }
        MathNode::StretchStack {
            character, body, ..
        } => {
            output.push_str("accent(");
            grouped(output, body);
            output.push(',');
            push_character(output, *character, '¯');
            output.push(')');
        }
        MathNode::Subscript { body, subscript } => {
            grouped(output, body);
            output.push_str("_(");
            append_expression(output, subscript);
            output.push(')');
        }
        MathNode::SubSup {
            body,
            subscript,
            superscript,
            ..
        } => {
            grouped(output, body);
            output.push_str("_(");
            append_expression(output, subscript);
            output.push_str(")^(");
            append_expression(output, superscript);
            output.push(')');
        }
        MathNode::Superscript { body, superscript } => {
            grouped(output, body);
            output.push_str("^(");
            append_expression(output, superscript);
            output.push(')');
        }
        MathNode::Underbar { body } => unary(output, "underline", body),
        MathNode::UpperLimit { body, limit } => {
            output.push_str("limits(");
            grouped(output, body);
            output.push_str(")^(");
            append_expression(output, limit);
            output.push(')');
        }
        MathNode::Unsupported {
            character,
            arguments,
            ..
        } => {
            if let Some(character) = character {
                push_text(output, &character.to_string());
            }
            for argument in arguments {
                grouped(output, argument);
            }
        }
    }
}

fn fraction(output: &mut String, numerator: &MathExpression, denominator: &MathExpression) {
    output.push_str("frac(");
    grouped(output, numerator);
    output.push(',');
    grouped(output, denominator);
    output.push(')');
}

fn unary(output: &mut String, function: &str, body: &MathExpression) {
    output.push_str(function);
    output.push('(');
    grouped(output, body);
    output.push(')');
}

fn grouped(output: &mut String, expression: &MathExpression) {
    output.push_str("attach(");
    append_expression(output, expression);
    output.push(')');
}

fn delimiters(
    output: &mut String,
    open: Option<char>,
    close: Option<char>,
    segments: &[MathExpression],
    separator: Option<char>,
) {
    output.push_str("lr(");
    push_character(output, open, '(');
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            push_character(output, separator, ',');
        }
        grouped(output, segment);
    }
    push_character(output, close, ')');
    output.push(')');
}

fn separated_expressions(output: &mut String, expressions: &[MathExpression], separator: &str) {
    for (index, expression) in expressions.iter().enumerate() {
        if index > 0 {
            output.push_str(separator);
        }
        grouped(output, expression);
    }
}

fn push_character(output: &mut String, character: Option<char>, fallback: char) {
    push_text(output, &character.unwrap_or(fallback).to_string());
}

fn push_nary_operator(output: &mut String, character: Option<char>) {
    match character.unwrap_or('∑') {
        '∑' => output.push_str("sum"),
        '∏' => output.push_str("product"),
        '∐' => output.push_str("product.co"),
        '∫' => output.push_str("integral"),
        '∬' => output.push_str("integral.double"),
        '∭' => output.push_str("integral.triple"),
        character => push_text(output, &character.to_string()),
    }
}

fn matrix_delimiters(character: Option<char>) -> Option<(&'static str, &'static str)> {
    match character {
        Some('\u{24a8}' | '(') => Some(("(", ")")),
        Some('\u{24b1}' | '|') => Some(("|", "|")),
        Some('\u{24a9}' | '‖') => Some(("‖", "‖")),
        _ => None,
    }
}

fn push_text(output: &mut String, value: &str) {
    output.push_str("text(\"");
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => output.push('�'),
            character => output.push(character),
        }
    }
    output.push_str("\")");
}

#[cfg(test)]
mod tests {
    use super::to_typst_math;
    use onenote_core::{MathExpression, MathNode};

    #[test]
    fn literal_text_cannot_escape_into_typst_code() {
        let expression = MathExpression {
            nodes: vec![MathNode::Text {
                value: "x\") #import \"secret\" \\ $".to_owned(),
            }],
        };

        let source = to_typst_math(&expression);

        assert_eq!(source, "text(\"x\\\") #import \\\"secret\\\" \\\\ $\")");
    }

    #[test]
    fn fraction_and_scripts_are_structural() {
        let text = |value: &str| MathExpression {
            nodes: vec![MathNode::Text {
                value: value.to_owned(),
            }],
        };
        let expression = MathExpression {
            nodes: vec![MathNode::Fraction {
                small: false,
                numerator: text("x"),
                denominator: MathExpression {
                    nodes: vec![MathNode::Superscript {
                        body: text("r"),
                        superscript: text("2"),
                    }],
                },
            }],
        };

        assert_eq!(
            to_typst_math(&expression),
            "frac(attach(text(\"x\")),attach(attach(text(\"r\"))^(text(\"2\"))))"
        );
    }
}
