use onenote_parser::contents::{MathInlineObject, MathObjectType};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

const START: char = '\u{fdd0}';
const SEPARATOR: char = '\u{fdee}';
const END: char = '\u{fdef}';

/// One parsed `OfficeMath` expression.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct MathExpression {
    /// Ordered expression nodes.
    pub nodes: Vec<MathNode>,
}

impl MathExpression {
    /// Return a marker-free, readable Unicode representation for search,
    /// accessibility, diagnostics, and clipboard fallbacks.
    pub fn plain_text(&self) -> String {
        let mut output = String::new();
        self.append_plain_text(&mut output);
        output
    }

    /// Whether decoding retained an operator that has no faithful typed form.
    pub fn contains_unsupported(&self) -> bool {
        self.nodes.iter().any(MathNode::contains_unsupported)
    }

    fn append_plain_text(&self, output: &mut String) {
        for node in &self.nodes {
            node.append_plain_text(output);
        }
    }
}

/// A UI-neutral `OfficeMath` node.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MathNode {
    /// Literal mathematical text.
    Text { value: String },
    /// An accent over an expression.
    Accent {
        character: Option<char>,
        body: MathExpression,
    },
    /// A layout box with source display flags.
    Box {
        align: Option<u8>,
        body: MathExpression,
    },
    /// A visibly boxed expression.
    BoxedFormula {
        align: Option<u8>,
        body: MathExpression,
    },
    /// Paired delimiters around an expression.
    Brackets {
        open: Option<char>,
        close: Option<char>,
        align: Option<u8>,
        body: MathExpression,
    },
    /// Paired delimiters containing multiple separated segments.
    BracketsWithSeparators {
        open: Option<char>,
        close: Option<char>,
        separator: Option<char>,
        align: Option<u8>,
        segments: Vec<MathExpression>,
    },
    /// Rows of aligned equations.
    EquationArray {
        columns: Option<u8>,
        align: Option<u8>,
        rows: Vec<MathExpression>,
    },
    /// A numerator over a denominator.
    Fraction {
        small: bool,
        numerator: MathExpression,
        denominator: MathExpression,
    },
    /// Function application.
    FunctionApply {
        function: MathExpression,
        argument: MathExpression,
    },
    /// Scripts to the left of a base expression.
    LeftSubSup {
        subscript: MathExpression,
        superscript: MathExpression,
        body: MathExpression,
    },
    /// A limit below a base expression.
    LowerLimit {
        body: MathExpression,
        limit: MathExpression,
    },
    /// A row-major matrix.
    Matrix {
        columns: Option<u8>,
        bracket: Option<char>,
        align: Option<u8>,
        items: Vec<MathExpression>,
    },
    /// A summation, integral, product, or other n-ary operator.
    Nary {
        operator: Option<char>,
        align: Option<u8>,
        subscript: MathExpression,
        superscript: MathExpression,
        body: MathExpression,
    },
    /// An operator character that must not be built up.
    Operator { character: Option<char> },
    /// A bar above an expression.
    Overbar { body: MathExpression },
    /// Invisible content that retains layout space.
    Phantom {
        kind: Option<char>,
        align: Option<u8>,
        body: MathExpression,
    },
    /// A square root or indexed radical.
    Radical {
        degree: MathExpression,
        body: MathExpression,
    },
    /// A skewed or linear fraction.
    SlashedFraction {
        linear: bool,
        numerator: MathExpression,
        denominator: MathExpression,
    },
    /// Two expressions stacked without a fraction bar.
    Stack {
        upper: MathExpression,
        lower: MathExpression,
    },
    /// A horizontally stretched character over or under an expression.
    StretchStack {
        character: Option<char>,
        align: Option<u8>,
        body: MathExpression,
    },
    /// A subscript attached to a base expression.
    Subscript {
        body: MathExpression,
        subscript: MathExpression,
    },
    /// Subscript and superscript attached to a base expression.
    SubSup {
        align: Option<u8>,
        body: MathExpression,
        subscript: MathExpression,
        superscript: MathExpression,
    },
    /// A superscript attached to a base expression.
    Superscript {
        body: MathExpression,
        superscript: MathExpression,
    },
    /// A bar below an expression.
    Underbar { body: MathExpression },
    /// A limit above a base expression.
    UpperLimit {
        body: MathExpression,
        limit: MathExpression,
    },
    /// A recognized stream whose operator properties are not supported.
    Unsupported {
        object_type: String,
        arg_count: u32,
        column: Option<u8>,
        align: Option<u8>,
        character: Option<char>,
        character1: Option<char>,
        character2: Option<char>,
        arguments: Vec<MathExpression>,
    },
}

impl MathNode {
    fn contains_unsupported(&self) -> bool {
        let expression = MathExpression::contains_unsupported;
        match self {
            Self::Unsupported { .. } => true,
            Self::Text { .. } | Self::Operator { .. } => false,
            Self::Accent { body, .. }
            | Self::Box { body, .. }
            | Self::BoxedFormula { body, .. }
            | Self::Brackets { body, .. }
            | Self::Overbar { body }
            | Self::Phantom { body, .. }
            | Self::StretchStack { body, .. }
            | Self::Underbar { body } => expression(body),
            Self::BracketsWithSeparators { segments, .. }
            | Self::EquationArray { rows: segments, .. }
            | Self::Matrix {
                items: segments, ..
            } => segments.iter().any(expression),
            Self::Fraction {
                numerator,
                denominator,
                ..
            }
            | Self::SlashedFraction {
                numerator,
                denominator,
                ..
            } => expression(numerator) || expression(denominator),
            Self::FunctionApply { function, argument } => {
                expression(function) || expression(argument)
            }
            Self::LeftSubSup {
                subscript,
                superscript,
                body,
            }
            | Self::Nary {
                subscript,
                superscript,
                body,
                ..
            }
            | Self::SubSup {
                subscript,
                superscript,
                body,
                ..
            } => expression(body) || expression(subscript) || expression(superscript),
            Self::LowerLimit { body, limit } | Self::UpperLimit { body, limit } => {
                expression(body) || expression(limit)
            }
            Self::Radical { degree, body } => expression(degree) || expression(body),
            Self::Stack { upper, lower } => expression(upper) || expression(lower),
            Self::Subscript { body, subscript } => expression(body) || expression(subscript),
            Self::Superscript { body, superscript } => expression(body) || expression(superscript),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn append_plain_text(&self, output: &mut String) {
        match self {
            Self::Text { value } => output.push_str(value),
            Self::Accent { character, body } => {
                body.append_plain_text(output);
                push_optional(output, *character);
            }
            Self::Box { body, .. } | Self::BoxedFormula { body, .. } => {
                body.append_plain_text(output);
            }
            Self::Brackets {
                open, close, body, ..
            } => {
                push_optional(output, *open);
                body.append_plain_text(output);
                push_optional(output, *close);
            }
            Self::BracketsWithSeparators {
                open,
                close,
                separator,
                segments,
                ..
            } => {
                push_optional(output, *open);
                append_joined(output, segments, separator.unwrap_or(','));
                push_optional(output, *close);
            }
            Self::EquationArray { rows, .. } => append_joined(output, rows, '\n'),
            Self::Fraction {
                numerator,
                denominator,
                ..
            }
            | Self::SlashedFraction {
                numerator,
                denominator,
                ..
            } => {
                output.push('(');
                numerator.append_plain_text(output);
                output.push_str(")/(");
                denominator.append_plain_text(output);
                output.push(')');
            }
            Self::FunctionApply { function, argument } => {
                function.append_plain_text(output);
                argument.append_plain_text(output);
            }
            Self::LeftSubSup {
                subscript,
                superscript,
                body,
            } => {
                output.push_str("_(");
                subscript.append_plain_text(output);
                output.push_str(")^(");
                superscript.append_plain_text(output);
                output.push(')');
                body.append_plain_text(output);
            }
            Self::LowerLimit { body, limit } => {
                body.append_plain_text(output);
                output.push_str("_(");
                limit.append_plain_text(output);
                output.push(')');
            }
            Self::Matrix { columns, items, .. } => {
                let columns = usize::from(columns.unwrap_or(1).max(1));
                output.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        output.push(if index % columns == 0 { ';' } else { ',' });
                    }
                    item.append_plain_text(output);
                }
                output.push(']');
            }
            Self::Nary {
                operator,
                subscript,
                superscript,
                body,
                ..
            } => {
                push_optional(output, *operator);
                output.push_str("_(");
                subscript.append_plain_text(output);
                output.push_str(")^(");
                superscript.append_plain_text(output);
                output.push(')');
                body.append_plain_text(output);
            }
            Self::Operator { character } => push_optional(output, *character),
            Self::Overbar { body }
            | Self::Phantom { body, .. }
            | Self::StretchStack { body, .. }
            | Self::Underbar { body } => body.append_plain_text(output),
            Self::Radical { degree, body } => {
                output.push('√');
                if !degree.nodes.is_empty() {
                    output.push('[');
                    degree.append_plain_text(output);
                    output.push(']');
                }
                output.push('(');
                body.append_plain_text(output);
                output.push(')');
            }
            Self::Stack { upper, lower } => {
                upper.append_plain_text(output);
                output.push('/');
                lower.append_plain_text(output);
            }
            Self::Subscript { body, subscript } => {
                body.append_plain_text(output);
                output.push_str("_(");
                subscript.append_plain_text(output);
                output.push(')');
            }
            Self::SubSup {
                body,
                subscript,
                superscript,
                ..
            } => {
                body.append_plain_text(output);
                output.push_str("_(");
                subscript.append_plain_text(output);
                output.push_str(")^(");
                superscript.append_plain_text(output);
                output.push(')');
            }
            Self::Superscript { body, superscript } => {
                body.append_plain_text(output);
                output.push_str("^(");
                superscript.append_plain_text(output);
                output.push(')');
            }
            Self::UpperLimit { body, limit } => {
                body.append_plain_text(output);
                output.push_str("^(");
                limit.append_plain_text(output);
                output.push(')');
            }
            Self::Unsupported {
                character,
                arguments,
                ..
            } => {
                push_optional(output, *character);
                append_joined(output, arguments, ',');
            }
        }
    }
}

/// One math-formatted source range inside a rich-text paragraph.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MathSpan {
    /// Inclusive source offset in UTF-16 code units.
    pub start_utf16: u32,
    /// Exclusive source offset in UTF-16 code units.
    pub end_utf16: u32,
    /// Parsed expression. `None` retains a readable fallback after malformed input.
    pub expression: Option<MathExpression>,
    /// Marker-free fallback text.
    pub fallback_text: String,
    /// Whether this range occupies the complete paragraph.
    pub display: bool,
    /// Non-fatal decoding diagnostic.
    pub diagnostic: Option<String>,
}

impl MathSpan {
    /// Text exposed to search, accessibility, and non-math renderers.
    pub fn visible_text(&self) -> String {
        self.expression
            .as_ref()
            .map_or_else(|| self.fallback_text.clone(), MathExpression::plain_text)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct MathSegment<'a> {
    pub(crate) start_utf16: u32,
    pub(crate) end_utf16: u32,
    pub(crate) text: &'a str,
    pub(crate) object: MathInlineObject,
}

pub(crate) fn decode_span(segments: &[MathSegment<'_>], display: bool) -> MathSpan {
    let start_utf16 = segments.first().map_or(0, |segment| segment.start_utf16);
    let end_utf16 = segments
        .last()
        .map_or(start_utf16, |segment| segment.end_utf16);
    let fallback_text = segments
        .iter()
        .flat_map(|segment| segment.text.chars())
        .filter(|character| !matches!(*character, START | SEPARATOR | END))
        .collect();
    match Lexer::new(segments).and_then(|mut lexer| Parser::new(&mut lexer).parse()) {
        Ok(expression) => {
            let diagnostic = expression
                .contains_unsupported()
                .then(|| "math expression contains an unsupported operator".to_owned());
            MathSpan {
                start_utf16,
                end_utf16,
                expression: Some(expression),
                fallback_text,
                display,
                diagnostic,
            }
        }
        Err(message) => MathSpan {
            start_utf16,
            end_utf16,
            expression: None,
            fallback_text,
            display,
            diagnostic: Some(message),
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ObjectKind {
    SimpleText,
    Accent,
    Box,
    BoxedFormula,
    Brackets,
    BracketsWithSeparators,
    EquationArray,
    Fraction,
    FunctionApply,
    LeftSubSup,
    LowerLimit,
    Matrix,
    Nary,
    Operator,
    Overbar,
    Phantom,
    Radical,
    SlashedFraction,
    Stack,
    StretchStack,
    Subscript,
    SubSup,
    Superscript,
    Underbar,
    UpperLimit,
    PlainText,
}

impl From<MathObjectType> for ObjectKind {
    fn from(value: MathObjectType) -> Self {
        match value {
            MathObjectType::SimpleText => Self::SimpleText,
            MathObjectType::Accent => Self::Accent,
            MathObjectType::Box => Self::Box,
            MathObjectType::BoxedFormula => Self::BoxedFormula,
            MathObjectType::Brackets => Self::Brackets,
            MathObjectType::BracketsWithSeps => Self::BracketsWithSeparators,
            MathObjectType::EquationArray => Self::EquationArray,
            MathObjectType::Fraction => Self::Fraction,
            MathObjectType::FunctionApply => Self::FunctionApply,
            MathObjectType::LeftSubSup => Self::LeftSubSup,
            MathObjectType::LowerLimit => Self::LowerLimit,
            MathObjectType::Matrix => Self::Matrix,
            MathObjectType::Nary => Self::Nary,
            MathObjectType::OpChar => Self::Operator,
            MathObjectType::Overbar => Self::Overbar,
            MathObjectType::Phantom => Self::Phantom,
            MathObjectType::Radical => Self::Radical,
            MathObjectType::SlashedFraction => Self::SlashedFraction,
            MathObjectType::Stack => Self::Stack,
            MathObjectType::StretchStack => Self::StretchStack,
            MathObjectType::Subscript => Self::Subscript,
            MathObjectType::SubSup => Self::SubSup,
            MathObjectType::Superscript => Self::Superscript,
            MathObjectType::Underbar => Self::Underbar,
            MathObjectType::UpperLimit => Self::UpperLimit,
            MathObjectType::PlainText => Self::PlainText,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Object {
    kind: ObjectKind,
    arg_count: u32,
    column: Option<u8>,
    align: Option<u8>,
    character: Option<char>,
    character1: Option<char>,
    character2: Option<char>,
}

impl From<MathInlineObject> for Object {
    fn from(value: MathInlineObject) -> Self {
        Self {
            kind: value.object_type().into(),
            arg_count: value.arg_count(),
            column: value.column(),
            align: value.align(),
            character: value.char(),
            character1: value.char1(),
            character2: value.char2(),
        }
    }
}

#[derive(Debug)]
enum Token {
    Text(String),
    Start(Object),
    Separator(ObjectKind),
    End(ObjectKind),
}

struct Lexer {
    tokens: VecDeque<Token>,
}

impl Lexer {
    fn new(segments: &[MathSegment<'_>]) -> Result<Self, String> {
        let mut tokens = VecDeque::new();
        for segment in segments {
            let object = Object::from(segment.object);
            let text = segment.text;
            let markers = text
                .chars()
                .filter(|character| matches!(*character, START | SEPARATOR | END))
                .count();
            if markers > 1 {
                return Err("math text run contains multiple structural markers".to_owned());
            }
            if let Some(remainder) = text.strip_prefix(START) {
                tokens.push_back(Token::Start(object));
                if !remainder.is_empty() {
                    tokens.push_back(Token::Text(remainder.to_owned()));
                }
            } else if let Some(content) = text.strip_suffix(SEPARATOR) {
                if !content.is_empty() {
                    tokens.push_back(Token::Text(content.to_owned()));
                }
                tokens.push_back(Token::Separator(object.kind));
            } else if let Some(content) = text.strip_suffix(END) {
                if !content.is_empty() {
                    tokens.push_back(Token::Text(content.to_owned()));
                }
                tokens.push_back(Token::End(object.kind));
            } else if markers == 0 {
                tokens.push_back(Token::Text(text.to_owned()));
            } else {
                return Err("math structural marker is not at a run boundary".to_owned());
            }
        }
        Ok(Self { tokens })
    }

    fn pop(&mut self) -> Option<Token> {
        self.tokens.pop_front()
    }

    fn front(&self) -> Option<&Token> {
        self.tokens.front()
    }
}

struct Parser<'a> {
    lexer: &'a mut Lexer,
    depth: usize,
    nodes: usize,
}

impl<'a> Parser<'a> {
    const MAX_DEPTH: usize = 256;
    const MAX_NODES: usize = 100_000;

    fn new(lexer: &'a mut Lexer) -> Self {
        Self {
            lexer,
            depth: 0,
            nodes: 0,
        }
    }

    fn parse(&mut self) -> Result<MathExpression, String> {
        let expression = self.parse_until(None)?;
        if let Some(token) = self.lexer.front() {
            return Err(format!("unexpected trailing math token: {token:?}"));
        }
        Ok(expression)
    }

    fn parse_until(&mut self, stop: Option<ObjectKind>) -> Result<MathExpression, String> {
        let mut nodes = Vec::new();
        loop {
            match self.lexer.front() {
                None => {
                    if stop.is_some() {
                        return Err("math object is missing its end marker".to_owned());
                    }
                    break;
                }
                Some(Token::Separator(kind) | Token::End(kind)) if Some(*kind) == stop => break,
                Some(Token::Separator(_) | Token::End(_)) => {
                    return Err("mismatched math separator or end marker".to_owned());
                }
                Some(Token::Text(_)) => {
                    let Some(Token::Text(value)) = self.lexer.pop() else {
                        unreachable!();
                    };
                    self.bump_node()?;
                    nodes.push(MathNode::Text { value });
                }
                Some(Token::Start(_)) => nodes.push(self.parse_object()?),
            }
        }
        Ok(MathExpression { nodes })
    }

    fn parse_object(&mut self) -> Result<MathNode, String> {
        let Some(Token::Start(object)) = self.lexer.pop() else {
            return Err("expected math object start".to_owned());
        };
        self.depth += 1;
        if self.depth > Self::MAX_DEPTH {
            return Err("math nesting limit exceeded".to_owned());
        }
        self.bump_node()?;
        let mut arguments = Vec::new();
        loop {
            arguments.push(self.parse_until(Some(object.kind))?);
            match self.lexer.pop() {
                Some(Token::Separator(kind)) if kind == object.kind => continue,
                Some(Token::End(kind)) if kind == object.kind => break,
                _ => return Err("math object has an invalid argument boundary".to_owned()),
            }
        }
        self.depth -= 1;
        Ok(build_node(object, arguments))
    }

    fn bump_node(&mut self) -> Result<(), String> {
        self.nodes += 1;
        if self.nodes > Self::MAX_NODES {
            Err("math node limit exceeded".to_owned())
        } else {
            Ok(())
        }
    }
}

#[allow(clippy::too_many_lines)]
fn build_node(object: Object, mut arguments: Vec<MathExpression>) -> MathNode {
    let unsupported = |arguments| MathNode::Unsupported {
        object_type: format!("{:?}", object.kind),
        arg_count: object.arg_count,
        column: object.column,
        align: object.align,
        character: object.character,
        character1: object.character1,
        character2: object.character2,
        arguments,
    };
    if usize::try_from(object.arg_count).ok() != Some(arguments.len()) {
        return unsupported(arguments);
    }
    match object.kind {
        ObjectKind::Accent if arguments.len() == 1 => MathNode::Accent {
            character: object.character,
            body: arguments.remove(0),
        },
        ObjectKind::Box if arguments.len() == 1 => MathNode::Box {
            align: object.align,
            body: arguments.remove(0),
        },
        ObjectKind::BoxedFormula if arguments.len() == 1 => MathNode::BoxedFormula {
            align: object.align,
            body: arguments.remove(0),
        },
        ObjectKind::Brackets if arguments.len() == 1 => MathNode::Brackets {
            open: object.character,
            close: object.character1,
            align: object.align,
            body: arguments.remove(0),
        },
        ObjectKind::BracketsWithSeparators => MathNode::BracketsWithSeparators {
            open: object.character,
            close: object.character1,
            separator: object.character2,
            align: object.align,
            segments: arguments,
        },
        ObjectKind::EquationArray => MathNode::EquationArray {
            columns: object.column,
            align: object.align,
            rows: arguments,
        },
        ObjectKind::Fraction if arguments.len() == 2 => MathNode::Fraction {
            small: object.character == Some('\u{2298}'),
            numerator: arguments.remove(0),
            denominator: arguments.remove(0),
        },
        ObjectKind::FunctionApply if arguments.len() == 2 => MathNode::FunctionApply {
            function: arguments.remove(0),
            argument: arguments.remove(0),
        },
        ObjectKind::LeftSubSup if arguments.len() == 3 => MathNode::LeftSubSup {
            subscript: arguments.remove(0),
            superscript: arguments.remove(0),
            body: arguments.remove(0),
        },
        ObjectKind::LowerLimit if arguments.len() == 2 => MathNode::LowerLimit {
            body: arguments.remove(0),
            limit: arguments.remove(0),
        },
        ObjectKind::Matrix => MathNode::Matrix {
            columns: object.column,
            bracket: object.character,
            align: object.align,
            items: arguments,
        },
        ObjectKind::Nary if arguments.len() == 3 => MathNode::Nary {
            operator: object.character,
            align: object.align,
            subscript: arguments.remove(0),
            superscript: arguments.remove(0),
            body: arguments.remove(0),
        },
        ObjectKind::Operator if arguments.is_empty() => MathNode::Operator {
            character: object.character,
        },
        ObjectKind::Overbar if arguments.len() == 1 => MathNode::Overbar {
            body: arguments.remove(0),
        },
        ObjectKind::Phantom if arguments.len() == 1 => MathNode::Phantom {
            kind: object.character,
            align: object.align,
            body: arguments.remove(0),
        },
        ObjectKind::Radical if arguments.len() == 2 => MathNode::Radical {
            degree: arguments.remove(0),
            body: arguments.remove(0),
        },
        ObjectKind::SlashedFraction if arguments.len() == 2 => MathNode::SlashedFraction {
            linear: object.character == Some('\u{2215}'),
            numerator: arguments.remove(0),
            denominator: arguments.remove(0),
        },
        ObjectKind::Stack if arguments.len() == 2 => MathNode::Stack {
            upper: arguments.remove(0),
            lower: arguments.remove(0),
        },
        ObjectKind::StretchStack if arguments.len() == 1 => MathNode::StretchStack {
            character: object.character,
            align: object.align,
            body: arguments.remove(0),
        },
        ObjectKind::Subscript if arguments.len() == 2 => MathNode::Subscript {
            body: arguments.remove(0),
            subscript: arguments.remove(0),
        },
        ObjectKind::SubSup if arguments.len() == 3 => MathNode::SubSup {
            align: object.align,
            body: arguments.remove(0),
            subscript: arguments.remove(0),
            superscript: arguments.remove(0),
        },
        ObjectKind::Superscript if arguments.len() == 2 => MathNode::Superscript {
            body: arguments.remove(0),
            superscript: arguments.remove(0),
        },
        ObjectKind::Underbar if arguments.len() == 1 => MathNode::Underbar {
            body: arguments.remove(0),
        },
        ObjectKind::UpperLimit if arguments.len() == 2 => MathNode::UpperLimit {
            body: arguments.remove(0),
            limit: arguments.remove(0),
        },
        _ => unsupported(arguments),
    }
}

fn append_joined(output: &mut String, values: &[MathExpression], separator: char) {
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            output.push(separator);
        }
        value.append_plain_text(output);
    }
}

fn push_optional(output: &mut String, character: Option<char>) {
    if let Some(character) = character {
        output.push(character);
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_span, MathExpression, MathNode, MathSegment};
    use onenote_parser::contents::MathInlineObject;

    #[test]
    fn plain_text_is_marker_free() {
        let expression = MathExpression {
            nodes: vec![MathNode::Superscript {
                body: MathExpression {
                    nodes: vec![MathNode::Text {
                        value: "r".to_owned(),
                    }],
                },
                superscript: MathExpression {
                    nodes: vec![MathNode::Text {
                        value: "2".to_owned(),
                    }],
                },
            }],
        };
        assert_eq!(expression.plain_text(), "r^(2)");
    }

    #[test]
    fn malformed_stream_retains_readable_text_and_diagnostic() {
        let span = decode_span(
            &[MathSegment {
                start_utf16: 0,
                end_utf16: 2,
                text: "\u{fdd0}x",
                object: MathInlineObject::default(),
            }],
            true,
        );

        assert!(span.expression.is_none());
        assert_eq!(span.visible_text(), "x");
        assert!(span.diagnostic.is_some());
    }

    #[test]
    fn unsupported_nodes_are_detectable_by_renderer_hosts() {
        let expression = MathExpression {
            nodes: vec![MathNode::Unsupported {
                object_type: "FutureOperator".to_owned(),
                arg_count: 0,
                column: None,
                align: None,
                character: None,
                character1: None,
                character2: None,
                arguments: Vec::new(),
            }],
        };

        assert!(expression.contains_unsupported());
    }
}
