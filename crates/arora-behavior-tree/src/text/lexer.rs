//! Tokens, and the layout that turns indentation into structure.
//!
//! `logos` cuts the text into raw tokens. A selection's path, `[…]` right
//! after a value, is one token: it runs to the first `]` on its line, so it
//! may hold any other character. The layout pass then reads the lines: a
//! line's indentation opens a block (`Indent`) or closes blocks (`Dedent`),
//! and its end is a `Newline`. A line that is empty or holds only a comment
//! makes neither. Inside brackets a line continues the one that opened them,
//! so a call broken over several lines is one node; a bracket still open when
//! a line comes back to the indentation of the line that opened it, and does
//! not close it, is unclosed, and the line starts afresh.
//!
//! A line ends at `\n`, `\r\n` or `\r`. A byte order mark opening the text is
//! no part of it.

use logos::Logos;

use super::ast::Span;
use super::Diagnostic;

/// The byte order mark.
const BOM: char = '\u{FEFF}';

#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
#[logos(skip r"[ \t\f\u{FEFF}]+")]
enum Raw {
    #[regex(r"\r\n|\r|\n")]
    Newline,
    #[regex(r"#[^\r\n]*", allow_greedy = true)]
    Comment,
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
    Ident,
    #[regex(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")]
    Uuid,
    #[regex(
        r"-?([0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?|NaN|inf)(u8|u16|u32|u64|i8|i16|i32|i64|f32|f64)?",
        priority = 4
    )]
    Number,
    #[regex(r#""([^"\\\r\n]|\\[^\r\n])*""#)]
    String,
    #[regex(r#"\{[^}"\r\n]*\}"#, allow_greedy = true)]
    Variable,
    #[regex(r#"\{"([^"\\\r\n]|\\[^\r\n])*"\}"#)]
    QuotedVariable,
    #[token("@")]
    At,
    #[token(".")]
    Dot,
    #[token("::")]
    PathSep,
    #[token(":")]
    Colon,
    #[token("=")]
    Equals,
    #[token(",")]
    Comma,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
}

/// A token the parser reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Comment,
    Ident,
    Uuid,
    Number,
    String,
    /// `{key}`.
    Variable,
    /// `{"key"}`.
    QuotedVariable,
    /// A selection's `[path]`.
    Path,
    At,
    Dot,
    PathSep,
    Colon,
    Equals,
    Comma,
    LParen,
    RParen,
    LBracket,
    RBracket,
    /// The end of a line that holds a node.
    Newline,
    /// A line more indented than the block it follows.
    Indent,
    /// The end of a block.
    Dedent,
    /// Text that is no token.
    Error,
    Eof,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Token {
    pub(crate) kind: Kind,
    pub(crate) span: Span,
}

impl From<Raw> for Kind {
    fn from(raw: Raw) -> Self {
        match raw {
            Raw::Newline => Kind::Newline,
            Raw::Comment => Kind::Comment,
            Raw::Ident => Kind::Ident,
            Raw::Uuid => Kind::Uuid,
            Raw::Number => Kind::Number,
            Raw::String => Kind::String,
            Raw::Variable => Kind::Variable,
            Raw::QuotedVariable => Kind::QuotedVariable,
            Raw::At => Kind::At,
            Raw::Dot => Kind::Dot,
            Raw::PathSep => Kind::PathSep,
            Raw::Colon => Kind::Colon,
            Raw::Equals => Kind::Equals,
            Raw::Comma => Kind::Comma,
            Raw::LParen => Kind::LParen,
            Raw::RParen => Kind::RParen,
            Raw::LBracket => Kind::LBracket,
            Raw::RBracket => Kind::RBracket,
        }
    }
}

/// Words that are literals, not names: `true`, `none`, `some(…)`, …
const RESERVED: [&str; 6] = ["true", "false", "none", "some", "uuid", "json"];

/// Whether `text` is written as a name as it is: one identifier, and no
/// literal.
pub(crate) fn is_name(text: &str) -> bool {
    let mut lexer = Raw::lexer(text);
    matches!(lexer.next(), Some(Ok(Raw::Ident)))
        && lexer.span() == (0..text.len())
        && lexer.next().is_none()
        && !RESERVED.contains(&text)
}

/// Whether `text` is written as a parameter's name as it is: names joined by
/// dots (`param.half_life`).
pub(crate) fn is_parameter_name(text: &str) -> bool {
    text.split('.').all(is_name)
}

/// The tokens of `text`, laid out in lines and blocks, ending with `Eof`.
pub(crate) fn tokens(text: &str, diagnostics: &mut Vec<Diagnostic>) -> Vec<Token> {
    let origin = if text.starts_with(BOM) {
        BOM.len_utf8()
    } else {
        0
    };
    let mut raw = Vec::with_capacity(text.len() / 4);
    let mut lexer = Raw::lexer(text);
    // The two tokens before the current one, comments aside: a `[` after a
    // value opens a selection's path.
    let mut before: [Option<Kind>; 2] = [None, None];
    while let Some(lexed) = lexer.next() {
        let span = lexer.span();
        let mut token = Token {
            kind: lexed.map_or(Kind::Error, Kind::from),
            span: Span::new(span.start, span.end),
        };
        if token.kind == Kind::LBracket && selects(before) {
            let rest = &text[span.end..];
            let end = rest.find([']', '\r', '\n']).unwrap_or(rest.len());
            let length = if rest[end..].starts_with(']') {
                end + 1
            } else {
                diagnostics.push(Diagnostic::error(
                    "a selection's path ends with `]` on its line".to_string(),
                    Span::new(span.start, span.end + end),
                ));
                end
            };
            lexer.bump(length);
            token = Token {
                kind: Kind::Path,
                span: Span::new(span.start, span.end + length),
            };
        }
        if token.kind != Kind::Comment {
            before = [before[1], Some(token.kind)];
        }
        raw.push(token);
    }
    Layout {
        text,
        origin,
        out: Vec::with_capacity(raw.len() + raw.len() / 4),
        indents: vec![0],
        depth: 0,
        open: None,
        line_has_content: false,
        diagnostics,
    }
    .run(&raw)
}

/// Whether a `[` after the tokens `before` opens a selection's path: it
/// follows a value that ends with a token of its own, a variable, a call, an
/// array, a path or a port.
fn selects(before: [Option<Kind>; 2]) -> bool {
    matches!(
        before,
        [
            _,
            Some(
                Kind::Variable | Kind::QuotedVariable | Kind::RParen | Kind::RBracket | Kind::Path
            )
        ] | [Some(Kind::At | Kind::Dot), Some(Kind::Ident | Kind::Uuid)]
    )
}

struct Layout<'a> {
    text: &'a str,
    /// Where the text starts, past a byte order mark.
    origin: usize,
    out: Vec<Token>,
    /// The indentation of each open block, the outermost first.
    indents: Vec<usize>,
    /// How many brackets are open.
    depth: usize,
    /// The outermost open bracket, and the indentation of its line.
    open: Option<(Span, usize)>,
    line_has_content: bool,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl Layout<'_> {
    fn run(mut self, raw: &[Token]) -> Vec<Token> {
        let mut at_line_start = true;
        for (index, token) in raw.iter().enumerate() {
            match token.kind {
                Kind::Newline => {
                    if self.depth > 0 && self.continues(&raw[index + 1..]) {
                        continue;
                    }
                    self.end_line(token.span);
                    at_line_start = true;
                }
                Kind::Comment => self.out.push(*token),
                kind => {
                    if at_line_start {
                        self.indent(token.span.start);
                        at_line_start = false;
                        self.line_has_content = true;
                    }
                    match kind {
                        Kind::LParen | Kind::LBracket => {
                            if self.depth == 0 {
                                self.open = Some((token.span, *self.indents.last().unwrap_or(&0)));
                            }
                            self.depth += 1;
                        }
                        Kind::RParen | Kind::RBracket => {
                            self.depth = self.depth.saturating_sub(1);
                        }
                        _ => {}
                    }
                    self.out.push(*token);
                }
            }
        }
        let end = Span::new(self.text.len(), self.text.len());
        self.end_line(end);
        for _ in 1..self.indents.len() {
            self.out.push(Token {
                kind: Kind::Dedent,
                span: end,
            });
        }
        self.out.push(Token {
            kind: Kind::Eof,
            span: end,
        });
        self.out
    }

    /// Whether the line after a newline inside brackets continues the line
    /// that opened them: it closes a bracket, or it is more indented than
    /// that line.
    fn continues(&mut self, rest: &[Token]) -> bool {
        let next = rest
            .iter()
            .find(|token| !matches!(token.kind, Kind::Newline | Kind::Comment));
        let (open, line_indent) = self.open.expect("a bracket is open");
        let continues = match next {
            None => false,
            Some(token) if matches!(token.kind, Kind::RParen | Kind::RBracket) => true,
            Some(token) => self.column(token.span.start) > line_indent,
        };
        if !continues {
            self.diagnostics.push(Diagnostic::error(
                format!("`{}` is not closed", &self.text[open.start..open.end]),
                open,
            ));
            self.depth = 0;
            self.open = None;
        }
        continues
    }

    fn end_line(&mut self, span: Span) {
        if self.depth > 0 {
            if let Some((open, _)) = self.open.take() {
                self.diagnostics.push(Diagnostic::error(
                    format!("`{}` is not closed", &self.text[open.start..open.end]),
                    open,
                ));
            }
            self.depth = 0;
        }
        if self.line_has_content {
            self.out.push(Token {
                kind: Kind::Newline,
                span,
            });
            self.line_has_content = false;
        }
    }

    /// The start of the line holding `offset`.
    fn line_start(&self, offset: usize) -> usize {
        self.text[..offset]
            .rfind(['\n', '\r'])
            .map_or(self.origin, |at| at + 1)
    }

    /// The indentation of the line whose first token starts at `offset`.
    fn column(&self, offset: usize) -> usize {
        offset - self.line_start(offset)
    }

    /// Open or close blocks for a line whose first token starts at `offset`.
    fn indent(&mut self, offset: usize) {
        let start = self.line_start(offset);
        let indentation = &self.text[start..offset];
        if indentation.contains('\t') {
            self.diagnostics.push(Diagnostic::error(
                "indentation is spaces: a tab indents nothing".to_string(),
                Span::new(start, offset),
            ));
        }
        let column = offset - start;
        let span = Span::new(offset, offset);
        let top = *self.indents.last().unwrap_or(&0);
        if column > top {
            self.indents.push(column);
            self.out.push(Token {
                kind: Kind::Indent,
                span,
            });
            return;
        }
        while column < *self.indents.last().unwrap_or(&0) {
            let enclosing = self.indents[..self.indents.len() - 1]
                .last()
                .copied()
                .unwrap_or(0);
            if column > enclosing {
                // Between two blocks' indentations: read as the inner
                // block's.
                self.diagnostics.push(Diagnostic::error(
                    "this line is indented as no enclosing block is".to_string(),
                    Span::new(start, offset),
                ));
                return;
            }
            self.indents.pop();
            self.out.push(Token {
                kind: Kind::Dedent,
                span,
            });
        }
    }
}
