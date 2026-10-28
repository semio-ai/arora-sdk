//! The parser: tokens to a [`Document`], by recursive descent.
//!
//! A line that does not parse is reported and left out of the document; its
//! children are kept, under the node that holds the line. An argument that
//! does not parse is reported and left out of its call; the rest of the call
//! is kept.

use serde::{Deserialize, Serialize};

use super::ast::{
    Argument, Call, Comment, Document, Expr, ExprKind, FunctionName, Name, Node, Span,
};
use super::lexer::{tokens, Kind, Token};
use super::Diagnostic;

/// A parsed text: its document, and what is wrong with the text.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Parsed {
    pub document: Document,
    pub diagnostics: Vec<Diagnostic>,
}

/// Parse `text` into a [`Document`]. Every line that parses is in it, in
/// spite of the lines that do not, which the diagnostics report.
pub fn parse(text: &str) -> Parsed {
    let mut diagnostics = Vec::new();
    let tokens = tokens(text, &mut diagnostics);
    let mut parser = Parser {
        text,
        tokens,
        pos: 0,
        diagnostics,
        pending: Vec::new(),
        line_end: 0,
        previous: Span::default(),
    };
    let nodes = parser.block(true);
    parser.peek();
    let comments = std::mem::take(&mut parser.pending);
    let mut diagnostics = parser.diagnostics;
    diagnostics.sort_by_key(|diagnostic| diagnostic.span.start);
    Parsed {
        document: Document { nodes, comments },
        diagnostics,
    }
}

/// The element types an array literal can state: `f64[1, 2]`.
pub(crate) const ARRAY_TYPES: [&str; 12] = [
    "bool", "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32", "f64", "str",
];

/// The type suffixes of a number, longest first.
const SUFFIXES: [&str; 10] = [
    "u16", "u32", "u64", "i16", "i32", "i64", "f32", "f64", "u8", "i8",
];

struct Parser<'s> {
    text: &'s str,
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
    /// The comments read and not yet given to a node.
    pending: Vec<Comment>,
    /// Where the last line read ends, to tell a blank line before the next.
    line_end: usize,
    /// The last token read.
    previous: Span,
}

/// What a line gives its block: a node, or, when the line does not parse,
/// the children indented under it.
enum Line {
    Node(Box<Node>),
    Orphans(Vec<Node>),
}

/// The head of a node line: all but its children and comments.
struct Head {
    span: Span,
    name: Option<Name>,
    target: Option<Expr>,
    call: Call,
}

/// A failure already reported.
type Parse<T> = Result<T, ()>;

impl<'s> Parser<'s> {
    // Reading tokens.
    //================================================================

    /// The next token, the comments before it read into `pending`.
    #[inline]
    fn peek(&mut self) -> Token {
        while self.tokens[self.pos].kind == Kind::Comment {
            let span = self.tokens[self.pos].span;
            self.pending.push(Comment {
                span,
                text: self.text[span.start + 1..span.end].trim_end().to_string(),
            });
            self.pos += 1;
        }
        self.tokens[self.pos]
    }

    /// The kind of the token after the next one, comments aside.
    fn peek_second(&mut self) -> Kind {
        self.peek();
        self.tokens[self.pos + 1..]
            .iter()
            .find(|token| token.kind != Kind::Comment)
            .map_or(Kind::Eof, |token| token.kind)
    }

    #[inline]
    fn bump(&mut self) -> Token {
        let token = self.peek();
        if token.kind != Kind::Eof {
            self.pos += 1;
            self.previous = token.span;
        }
        token
    }

    #[inline]
    fn at(&mut self, kind: Kind) -> bool {
        self.peek().kind == kind
    }

    #[inline]
    fn eat(&mut self, kind: Kind) -> Option<Token> {
        self.at(kind).then(|| self.bump())
    }

    fn expect(&mut self, kind: Kind, message: &str) -> Parse<Token> {
        match self.eat(kind) {
            Some(token) => Ok(token),
            None => {
                let span = self.peek().span;
                self.fail(message.to_string(), span)
            }
        }
    }

    fn text(&self, span: Span) -> &'s str {
        &self.text[span.start..span.end]
    }

    fn name(&self, token: Token) -> Name {
        Name {
            span: token.span,
            text: self.text(token.span).to_string(),
        }
    }

    fn error(&mut self, message: String, span: Span) {
        self.diagnostics.push(Diagnostic::error(message, span));
    }

    /// Report `message` at `span`, and fail.
    fn fail<T>(&mut self, message: String, span: Span) -> Parse<T> {
        self.error(message, span);
        Err(())
    }

    #[inline]
    fn at_line_end(&mut self) -> bool {
        matches!(
            self.peek().kind,
            Kind::Newline | Kind::Eof | Kind::Indent | Kind::Dedent
        )
    }

    /// Skip the rest of the line.
    fn skip_line(&mut self) {
        while !self.at_line_end() {
            self.bump();
        }
    }

    /// Skip to the `,` or the closing bracket that ends the current element,
    /// past any bracket it opens.
    fn skip_element(&mut self) {
        let mut depth = 0usize;
        while !self.at_line_end() {
            match self.peek().kind {
                Kind::LParen | Kind::LBracket => depth += 1,
                Kind::Comma | Kind::RParen | Kind::RBracket if depth == 0 => return,
                Kind::RParen | Kind::RBracket => depth -= 1,
                _ => {}
            }
            self.bump();
        }
    }

    // Lines and blocks.
    //================================================================

    /// The nodes of a block, up to its end.
    fn block(&mut self, top: bool) -> Vec<Node> {
        let mut nodes = Vec::new();
        loop {
            let token = self.peek();
            match token.kind {
                Kind::Eof => break,
                Kind::Dedent if top => {
                    self.bump();
                }
                Kind::Dedent => break,
                Kind::Newline => {
                    self.bump();
                }
                Kind::Indent => {
                    self.bump();
                    let span = self.peek().span;
                    self.error("this line is indented under no node".to_string(), span);
                    nodes.extend(self.children());
                }
                _ => match self.line() {
                    Line::Node(node) => nodes.push(*node),
                    Line::Orphans(orphans) => nodes.extend(orphans),
                },
            }
        }
        nodes
    }

    /// The block after an `Indent`, and its `Dedent`.
    fn children(&mut self) -> Vec<Node> {
        let children = self.block(false);
        self.eat(Kind::Dedent);
        children
    }

    fn line(&mut self) -> Line {
        let mut comments = std::mem::take(&mut self.pending);
        let start = comments
            .first()
            .map_or_else(|| self.peek().span.start, |comment| comment.span.start);
        let blank_before = self.text[self.line_end.min(start)..start]
            .matches('\n')
            .count()
            >= 2;
        let head = self.head();
        if head.is_ok() && !self.at_line_end() {
            let span = self.peek().span;
            let message = format!("`{}` is past the end of the node", self.text(span));
            self.error(message, span);
        }
        self.skip_line();
        let end = self.peek().span.start;
        self.eat(Kind::Newline);
        self.line_end = end;
        let head = match head {
            Ok(head) => head,
            Err(()) => {
                comments.append(&mut self.pending);
                self.pending = comments;
                let orphans = if self.eat(Kind::Indent).is_some() {
                    self.children()
                } else {
                    Vec::new()
                };
                return Line::Orphans(orphans);
            }
        };
        // A comment inside a call broken over lines goes above the node; the
        // comment after its last token stays at the end of its line.
        let mut comment = None;
        for found in std::mem::take(&mut self.pending) {
            if found.span.start >= head.span.end && comment.is_none() {
                comment = Some(found);
            } else {
                comments.push(found);
            }
        }
        let children = if self.eat(Kind::Indent).is_some() {
            self.children()
        } else {
            Vec::new()
        };
        Line::Node(Box::new(Node {
            span: head.span,
            name: head.name,
            target: head.target,
            call: head.call,
            children,
            comments,
            comment,
            blank_before,
        }))
    }

    /// `[name ':'] [target '='] function ['(' arguments ')']`.
    fn head(&mut self) -> Parse<Head> {
        let start = self.peek().span;
        let name = if matches!(self.peek().kind, Kind::Ident | Kind::Uuid)
            && self.peek_second() == Kind::Colon
        {
            let token = self.bump();
            self.bump();
            Some(self.name(token))
        } else {
            None
        };
        let target = if matches!(self.peek().kind, Kind::Variable | Kind::At) {
            let target = self.expression()?;
            self.expect(
                Kind::Equals,
                "expected `=` after the target the node's return value is written to",
            )?;
            Some(target)
        } else {
            None
        };
        let call = self.call(true)?;
        Ok(Head {
            span: start.to(call.span),
            name,
            target,
            call,
        })
    }

    // Calls.
    //================================================================

    /// `function(arguments)`; the parentheses may be left out when
    /// `bare` and there is no argument.
    fn call(&mut self, bare: bool) -> Parse<Call> {
        let token = self.peek();
        match token.kind {
            Kind::Ident | Kind::Uuid => {}
            Kind::LParen => {
                return self.fail(
                    "a computed function is shown by the display only: name the function"
                        .to_string(),
                    token.span,
                )
            }
            _ => {
                let message = format!("expected a function, found `{}`", self.text(token.span));
                return self.fail(message, token.span);
            }
        }
        let function = self.function_name()?;
        let (arguments, end) = if let Some(open) = self.eat(Kind::LParen) {
            self.arguments(open)
        } else if bare {
            (Vec::new(), function.span)
        } else {
            return self.fail(
                format!(
                    "expected `(`: a call in an argument is written `{}()`",
                    self.text(function.span)
                ),
                function.span,
            );
        };
        Ok(Call {
            span: function.span.to(end),
            function,
            arguments,
        })
    }

    /// `name`, or `module::name`.
    fn function_name(&mut self) -> Parse<FunctionName> {
        let first = self.bump();
        let first = self.name(first);
        if self.eat(Kind::PathSep).is_none() {
            return Ok(FunctionName {
                span: first.span,
                module: None,
                name: first,
            });
        }
        let token = self.peek();
        if !matches!(token.kind, Kind::Ident | Kind::Uuid) {
            return self.fail(
                "expected a function name after `::`".to_string(),
                token.span,
            );
        }
        self.bump();
        let name = self.name(token);
        Ok(FunctionName {
            span: first.span.to(name.span),
            module: Some(first),
            name,
        })
    }

    /// The arguments after `open`, up to and with the closing `)`: the
    /// arguments that parse, and the span of the last token read.
    fn arguments(&mut self, open: Token) -> (Vec<Argument>, Span) {
        let mut arguments = Vec::new();
        loop {
            if let Some(close) = self.eat(Kind::RParen) {
                return (arguments, close.span);
            }
            if self.at_line_end() {
                // The layout reports the unclosed `(`.
                let end = self.previous.max_end(open.span);
                return (arguments, end);
            }
            let failed = match self.argument() {
                Ok(argument) => {
                    arguments.push(argument);
                    false
                }
                Err(()) => {
                    self.skip_element();
                    true
                }
            };
            let token = self.peek();
            match token.kind {
                Kind::Comma => {
                    self.bump();
                }
                Kind::RParen => {}
                _ if self.at_line_end() => {}
                // A bracket that closes nothing here: past it, and on.
                _ if failed => {
                    self.bump();
                }
                _ => {
                    let message = format!("expected `,` or `)`, found `{}`", self.text(token.span));
                    self.error(message, token.span);
                    self.bump();
                    self.skip_element();
                    self.eat(Kind::Comma);
                }
            }
        }
    }

    /// `parameter: value`.
    fn argument(&mut self) -> Parse<Argument> {
        let token = self.peek();
        let name = match token.kind {
            Kind::Ident | Kind::Uuid => {
                self.bump();
                self.name(token)
            }
            Kind::LParen => {
                return self.fail(
                    "a computed parameter is shown by the display only: name the parameter"
                        .to_string(),
                    token.span,
                )
            }
            _ => {
                let message = format!(
                    "expected a parameter name, found `{}`",
                    self.text(token.span)
                );
                return self.fail(message, token.span);
            }
        };
        self.expect(Kind::Colon, "expected `:` after the parameter name")?;
        let value = self.expression()?;
        Ok(Argument {
            span: name.span.to(value.span),
            name,
            value,
        })
    }

    // Expressions.
    //================================================================

    /// A value, then any selections out of it: `source[path]`.
    fn expression(&mut self) -> Parse<Expr> {
        let mut expression = self.primary()?;
        while let Some(open) = self.eat(Kind::LBracket) {
            let mut depth = 0usize;
            let close = loop {
                if self.at_line_end() {
                    // The layout reports the unclosed `[`.
                    return Err(());
                }
                let token = self.bump();
                match token.kind {
                    Kind::LBracket => depth += 1,
                    Kind::RBracket if depth == 0 => break token,
                    Kind::RBracket => depth -= 1,
                    _ => {}
                }
            };
            let path = self.text[open.span.end..close.span.start]
                .trim()
                .to_string();
            if path.is_empty() {
                self.error(
                    "a selection names the path it reads: `source[path]`".to_string(),
                    open.span.to(close.span),
                );
            }
            expression = Expr {
                span: expression.span.to(close.span),
                kind: ExprKind::Select {
                    source: Box::new(expression),
                    path,
                },
            };
        }
        Ok(expression)
    }

    fn primary(&mut self) -> Parse<Expr> {
        let token = self.peek();
        let expr = |kind, span| Ok(Expr { span, kind });
        match token.kind {
            Kind::Number => {
                self.bump();
                let text = self.text(token.span);
                let suffix = SUFFIXES.iter().find(|suffix| text.ends_with(*suffix));
                let kind = ExprKind::Number {
                    text: text[..text.len() - suffix.map_or(0, |suffix| suffix.len())].to_string(),
                    suffix: suffix.map(|suffix| suffix.to_string()),
                };
                expr(kind, token.span)
            }
            Kind::String => {
                self.bump();
                let value = self.string(token)?;
                expr(ExprKind::String { value }, token.span)
            }
            Kind::Variable => {
                self.bump();
                let text = self.text(token.span);
                let name = text[1..text.len() - 1].trim();
                if name.starts_with('=') {
                    return self.fail(
                        "`{= …}` is an anonymous cell, which the display shows with the value \
                         it holds: a text binds a variable `{key}` or a literal"
                            .to_string(),
                        token.span,
                    );
                }
                if name.is_empty() || name.contains(char::is_whitespace) {
                    return self.fail(
                        "a variable is a store key `{path/key}` or a tree-local `{.name}`"
                            .to_string(),
                        token.span,
                    );
                }
                let name = name.to_string();
                expr(ExprKind::Variable { name }, token.span)
            }
            Kind::At => {
                self.bump();
                let node = self.reference("expected a node name after `@`")?;
                let parameter = if self.eat(Kind::Dot).is_some() {
                    Some(self.reference("expected a parameter name after `.`")?)
                } else {
                    None
                };
                let end = parameter.as_ref().map_or(node.span, |name| name.span);
                expr(ExprKind::Port { node, parameter }, token.span.to(end))
            }
            Kind::LParen => {
                self.bump();
                let close = self.expect(Kind::RParen, "expected `)`: `()` is the unit value")?;
                expr(ExprKind::Unit, token.span.to(close.span))
            }
            Kind::LBracket => {
                self.bump();
                let (elements, close) = self.elements()?;
                expr(
                    ExprKind::Array {
                        element: None,
                        elements,
                    },
                    token.span.to(close),
                )
            }
            Kind::Ident => {
                let second = self.peek_second();
                match (self.text(token.span), second) {
                    ("true", _) | ("false", _) => {
                        self.bump();
                        let value = self.text(token.span) == "true";
                        expr(ExprKind::Bool { value }, token.span)
                    }
                    ("none", _) => {
                        self.bump();
                        expr(ExprKind::None, token.span)
                    }
                    ("some", Kind::LParen) => {
                        self.bump();
                        self.bump();
                        let value = Box::new(self.expression()?);
                        let close = self.expect(Kind::RParen, "expected `)` after `some(…`")?;
                        expr(ExprKind::Some { value }, token.span.to(close.span))
                    }
                    ("uuid", Kind::LParen) => {
                        self.bump();
                        self.bump();
                        let id = self.expect(Kind::Uuid, "expected an id in `uuid(…)`")?;
                        let value =
                            uuid::Uuid::parse_str(self.text(id.span)).expect("the lexer reads ids");
                        let close = self.expect(Kind::RParen, "expected `)` after `uuid(…`")?;
                        expr(ExprKind::Uuid { value }, token.span.to(close.span))
                    }
                    ("json", Kind::LParen) => {
                        self.bump();
                        self.bump();
                        let string = self.expect(Kind::String, "expected a string in `json(…)`")?;
                        let text = self.string(string)?;
                        let close = self.expect(Kind::RParen, "expected `)` after `json(…`")?;
                        expr(ExprKind::Json { text }, token.span.to(close.span))
                    }
                    (element, Kind::LBracket) if ARRAY_TYPES.contains(&element) => {
                        let element = element.to_string();
                        self.bump();
                        self.bump();
                        let (elements, close) = self.elements()?;
                        expr(
                            ExprKind::Array {
                                element: Some(element),
                                elements,
                            },
                            token.span.to(close),
                        )
                    }
                    _ => {
                        let call = self.call(false)?;
                        let span = call.span;
                        expr(
                            ExprKind::Call {
                                call: Box::new(call),
                            },
                            span,
                        )
                    }
                }
            }
            Kind::Uuid => {
                let call = self.call(false)?;
                let span = call.span;
                expr(
                    ExprKind::Call {
                        call: Box::new(call),
                    },
                    span,
                )
            }
            Kind::Error => {
                self.bump();
                let message = format!("`{}` is not part of the language", self.text(token.span));
                self.fail(message, token.span)
            }
            _ => {
                let message = format!("expected a value, found `{}`", self.text(token.span));
                self.fail(message, token.span)
            }
        }
    }

    /// The name or id after `@` or `.`.
    fn reference(&mut self, message: &str) -> Parse<Name> {
        let token = self.peek();
        if !matches!(token.kind, Kind::Ident | Kind::Uuid) {
            return self.fail(message.to_string(), token.span);
        }
        self.bump();
        Ok(self.name(token))
    }

    /// The elements of an array after its `[`, and the span of its `]`.
    fn elements(&mut self) -> Parse<(Vec<Expr>, Span)> {
        let mut elements = Vec::new();
        loop {
            if let Some(close) = self.eat(Kind::RBracket) {
                return Ok((elements, close.span));
            }
            if self.at_line_end() {
                // The layout reports the unclosed `[`.
                return Err(());
            }
            let failed = match self.expression() {
                Ok(element) => {
                    elements.push(element);
                    false
                }
                Err(()) => {
                    self.skip_element();
                    true
                }
            };
            let token = self.peek();
            match token.kind {
                Kind::Comma => {
                    self.bump();
                }
                Kind::RBracket => {}
                _ if self.at_line_end() => {}
                // A bracket that closes nothing here: past it, and on.
                _ if failed => {
                    self.bump();
                }
                _ => {
                    let message = format!("expected `,` or `]`, found `{}`", self.text(token.span));
                    self.error(message, token.span);
                    self.bump();
                    self.skip_element();
                    self.eat(Kind::Comma);
                }
            }
        }
    }

    /// A string token's value: its quotes removed, its escapes read.
    fn string(&mut self, token: Token) -> Parse<String> {
        let quoted = self.text(token.span);
        match unescape(&quoted[1..quoted.len() - 1]) {
            Ok(value) => Ok(value),
            Err((offset, length, message)) => {
                let at = token.span.start + 1 + offset;
                self.fail(message, Span::new(at, at + length))
            }
        }
    }
}

impl Span {
    /// `self`, or the span of `fallback` when `self` ends before it.
    fn max_end(self, fallback: Span) -> Span {
        if self.end >= fallback.end {
            self
        } else {
            fallback
        }
    }
}

/// A string's escapes read: those Rust's `Debug` writes, `\n`, `\r`, `\t`,
/// `\\`, `\"`, `\'`, `\0` and `\u{…}`. An error gives the offset and the
/// length of the escape that does not read.
fn unescape(text: &str) -> Result<String, (usize, usize, String)> {
    let mut value = String::with_capacity(text.len());
    let mut chars = text.char_indices();
    while let Some((at, char)) = chars.next() {
        if char != '\\' {
            value.push(char);
            continue;
        }
        let escaped = match chars.next() {
            Some((_, 'n')) => '\n',
            Some((_, 'r')) => '\r',
            Some((_, 't')) => '\t',
            Some((_, '\\')) => '\\',
            Some((_, '"')) => '"',
            Some((_, '\'')) => '\'',
            Some((_, '0')) => '\0',
            Some((_, 'u')) => {
                let rest = &text[at + 2..];
                let code = rest
                    .strip_prefix('{')
                    .and_then(|rest| rest.split_once('}'))
                    .and_then(|(hex, _)| u32::from_str_radix(hex, 16).ok().map(|code| (hex, code)));
                match code.and_then(|(hex, code)| char::from_u32(code).map(|char| (hex, char))) {
                    Some((hex, char)) => {
                        // `{`, the digits and `}`.
                        for _ in 0..hex.len() + 2 {
                            chars.next();
                        }
                        char
                    }
                    None => {
                        return Err((
                            at,
                            2,
                            "`\\u` is written `\\u{…}` with a code point in hex".to_string(),
                        ))
                    }
                }
            }
            other => {
                let length = 1 + other.map_or(0, |(_, char)| char.len_utf8());
                return Err((at, length, "unknown escape".to_string()));
            }
        };
        value.push(escaped);
    }
    Ok(value)
}
