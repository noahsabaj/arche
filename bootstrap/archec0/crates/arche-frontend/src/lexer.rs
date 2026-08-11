//! Incremental, syntax-complete M27-C1 lexer.

use std::collections::VecDeque;
use std::io::{self, BufRead};
use std::sync::Arc;

use crate::source::{advance, Diagnostic, FileId, SourcePosition, Span};
use crate::symbol::{Symbol, SymbolInterner};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Keyword {
    As,
    Break,
    Catch,
    Commands,
    Component,
    Const,
    Continue,
    Default,
    Else,
    Enum,
    Entity,
    False,
    Fn,
    For,
    Gen,
    If,
    Impl,
    In,
    Init,
    Let,
    Loop,
    Match,
    Mod,
    Move,
    Mut,
    Package,
    Pub,
    Query,
    Read,
    Ref,
    Requires,
    Resource,
    Resume,
    Return,
    Run,
    Schedule,
    SelfType,
    SelfValue,
    Spawn,
    Static,
    Str,
    Struct,
    Super,
    System,
    Tag,
    Throw,
    Throws,
    Trait,
    True,
    Type,
    Unsafe,
    Use,
    Where,
    While,
    World,
    Yield,
    Yields,
    I8,
    I16,
    I32,
    I64,
    Isize,
    U8,
    U16,
    U32,
    U64,
    Usize,
    F32,
    F64,
    Bool,
    Char,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Punctuation {
    ColonColon,
    Arrow,
    FatArrow,
    RangeInclusive,
    Range,
    ShiftLeft,
    ShiftRight,
    LessEqual,
    GreaterEqual,
    EqualEqual,
    NotEqual,
    LogicalAnd,
    LogicalOr,
    AddAssign,
    LeftBrace,
    RightBrace,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Comma,
    Semicolon,
    Colon,
    Dot,
    At,
    Pipe,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Ampersand,
    Caret,
    Bang,
    Tilde,
    Equal,
    Less,
    Greater,
}

impl Punctuation {
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::ColonColon => "::",
            Self::Arrow => "->",
            Self::FatArrow => "=>",
            Self::RangeInclusive => "..=",
            Self::Range => "..",
            Self::ShiftLeft => "<<",
            Self::ShiftRight => ">>",
            Self::LessEqual => "<=",
            Self::GreaterEqual => ">=",
            Self::EqualEqual => "==",
            Self::NotEqual => "!=",
            Self::LogicalAnd => "&&",
            Self::LogicalOr => "||",
            Self::AddAssign => "+=",
            Self::LeftBrace => "{",
            Self::RightBrace => "}",
            Self::LeftParen => "(",
            Self::RightParen => ")",
            Self::LeftBracket => "[",
            Self::RightBracket => "]",
            Self::Comma => ",",
            Self::Semicolon => ";",
            Self::Colon => ":",
            Self::Dot => ".",
            Self::At => "@",
            Self::Pipe => "|",
            Self::Plus => "+",
            Self::Minus => "-",
            Self::Star => "*",
            Self::Slash => "/",
            Self::Percent => "%",
            Self::Ampersand => "&",
            Self::Caret => "^",
            Self::Bang => "!",
            Self::Tilde => "~",
            Self::Equal => "=",
            Self::Less => "<",
            Self::Greater => ">",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumericBase {
    Binary,
    Octal,
    Decimal,
    Hexadecimal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegerSuffix {
    I8,
    I16,
    I32,
    I64,
    Isize,
    U8,
    U16,
    U32,
    U64,
    Usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FloatSuffix {
    F32,
    F64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegerLiteral {
    pub base: NumericBase,
    /// Canonical digit sequence without base prefix, separators, or suffix.
    pub digits: Arc<str>,
    pub suffix: Option<IntegerSuffix>,
    pub raw: Arc<str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FloatLiteral {
    pub base: NumericBase,
    /// Exact validated source spelling. C2 converts this exact rational/dyadic
    /// value once the contextual floating type is known.
    pub raw: Arc<str>,
    pub suffix: Option<FloatSuffix>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenKind {
    Keyword(Keyword),
    Identifier(Symbol),
    Lifetime(Symbol),
    Wildcard,
    DocComment(Arc<str>),
    Integer(IntegerLiteral),
    Float(FloatLiteral),
    Character(char),
    String(Arc<str>),
    Punctuation(Punctuation),
    Eof,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    /// Exact UTF-8 token spelling. The EOF token owns an empty string.
    pub lexeme: Arc<str>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug)]
struct Character {
    value: char,
    start: SourcePosition,
    end: SourcePosition,
}

struct CharacterReader<R> {
    reader: R,
    position: SourcePosition,
    pending: VecDeque<Character>,
}

impl<R: BufRead> CharacterReader<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            position: SourcePosition::START,
            pending: VecDeque::new(),
        }
    }

    fn peek(&mut self, index: usize) -> io::Result<Option<Character>> {
        while self.pending.len() <= index {
            let Some(character) = self.decode()? else {
                break;
            };
            self.pending.push_back(character);
        }
        Ok(self.pending.get(index).copied())
    }

    fn next(&mut self) -> io::Result<Option<Character>> {
        if let Some(character) = self.pending.pop_front() {
            return Ok(Some(character));
        }
        self.decode()
    }

    fn decode(&mut self) -> io::Result<Option<Character>> {
        let mut bytes = [0_u8; 4];
        let read = self.reader.read(&mut bytes[..1])?;
        if read == 0 {
            return Ok(None);
        }
        let width = utf8_width(bytes[0]).ok_or_else(|| invalid_utf8(self.position.byte))?;
        if width > 1 {
            self.reader
                .read_exact(&mut bytes[1..width])
                .map_err(|error| match error.kind() {
                    io::ErrorKind::UnexpectedEof => invalid_utf8(self.position.byte),
                    _ => error,
                })?;
        }
        let value = std::str::from_utf8(&bytes[..width])
            .map_err(|_| invalid_utf8(self.position.byte))?
            .chars()
            .next()
            .ok_or_else(|| invalid_utf8(self.position.byte))?;
        let start = self.position;
        advance(&mut self.position, value)?;
        Ok(Some(Character {
            value,
            start,
            end: self.position,
        }))
    }
}

/// Incremental lexer over an immutable retained reader.
pub struct Lexer<R> {
    file: FileId,
    characters: CharacterReader<R>,
    symbols: SymbolInterner,
}

impl<R: BufRead> Lexer<R> {
    pub fn new(file: FileId, reader: R) -> Self {
        Self {
            file,
            characters: CharacterReader::new(reader),
            symbols: SymbolInterner::default(),
        }
    }

    pub fn next_token(&mut self) -> Result<Token, Diagnostic> {
        loop {
            let Some(first) = self.next_character()? else {
                let position = self.characters.position;
                return Ok(Token {
                    kind: TokenKind::Eof,
                    lexeme: Arc::from(""),
                    span: self.span(position, position),
                });
            };
            if first.start.byte == 0 && first.value == '\u{feff}' {
                return Err(Diagnostic::at(
                    "LEX001",
                    self.span(first.start, first.end),
                    "source must be UTF-8 without a byte-order mark",
                ));
            }
            if is_lexical_whitespace(first.value) {
                continue;
            }
            if first.value.is_whitespace() {
                return Err(Diagnostic::at(
                    "LEX001",
                    self.span(first.start, first.end),
                    "only ASCII space, tab, CR, and LF are lexical whitespace",
                ));
            }
            if first.value == '/' {
                if self.peek_value(0)? == Some('/') {
                    self.next_character()?.expect("peeked character");
                    match self.peek_value(0)? {
                        Some('/') => {
                            let third = self.next_character()?.expect("peeked character");
                            return self.doc_comment(first, third);
                        }
                        Some('!') => {
                            let bang = self.next_character()?.expect("peeked character");
                            return Err(Diagnostic::at(
                                "LEX001",
                                self.span(first.start, bang.end),
                                "inner doc comments do not exist in Arche 0.1",
                            ));
                        }
                        _ => {
                            self.line_comment()?;
                            continue;
                        }
                    }
                }
                if self.peek_value(0)? == Some('*') {
                    let star = self.next_character()?.expect("peeked character");
                    if self.peek_value(0)? == Some('*') {
                        let doc = self.next_character()?.expect("peeked character");
                        return Err(Diagnostic::at(
                            "LEX001",
                            self.span(first.start, doc.end),
                            "block doc comments do not exist in Arche 0.1",
                        ));
                    }
                    self.block_comment(first, star)?;
                    continue;
                }
            }
            return self.token_starting(first);
        }
    }

    fn token_starting(&mut self, first: Character) -> Result<Token, Diagnostic> {
        if first.value == '_' || unicode_ident::is_xid_start(first.value) {
            return self.identifier(first);
        }
        if first.value.is_ascii_digit() {
            return self.number(first);
        }
        if first.value == '"' {
            return self.quoted(first, '"');
        }
        if first.value == '\'' {
            return self.lifetime_or_character(first);
        }
        self.punctuation(first)
    }

    fn identifier(&mut self, first: Character) -> Result<Token, Diagnostic> {
        let mut text = String::new();
        text.push(first.value);
        let mut end = first.end;
        while self
            .peek_value(0)?
            .is_some_and(|value| value == '_' || unicode_ident::is_xid_continue(value))
        {
            let character = self.next_character()?.expect("peeked character");
            text.push(character.value);
            end = character.end;
        }
        let span = self.span(first.start, end);
        if text == "_" {
            return Ok(Token {
                kind: TokenKind::Wildcard,
                lexeme: Arc::from(text),
                span,
            });
        }
        let symbol = self
            .symbols
            .intern_identifier(&text)
            .map_err(|error| Diagnostic::at("LEX001", span, error.to_string()))?;
        let kind = keyword(symbol.as_str())
            .map(TokenKind::Keyword)
            .unwrap_or(TokenKind::Identifier(symbol));
        Ok(Token {
            kind,
            lexeme: Arc::from(text),
            span,
        })
    }

    fn lifetime_or_character(&mut self, first: Character) -> Result<Token, Diagnostic> {
        let starts_identifier = self
            .peek_value(0)?
            .is_some_and(|value| value == '_' || unicode_ident::is_xid_start(value));
        if starts_identifier {
            let mut length = 0_usize;
            while self
                .peek_value(length)?
                .is_some_and(|value| value == '_' || unicode_ident::is_xid_continue(value))
            {
                length = length.checked_add(1).ok_or_else(|| {
                    Diagnostic::at(
                        "LEX001",
                        self.span(first.start, self.characters.position),
                        "lifetime lookahead exceeds host address space",
                    )
                })?;
            }
            if self.peek_value(length)? != Some('\'') {
                let mut raw = String::from("'");
                let mut name = String::new();
                let mut end = first.end;
                for _ in 0..length {
                    let character = self.next_character()?.expect("peeked character");
                    raw.push(character.value);
                    name.push(character.value);
                    end = character.end;
                }
                let span = self.span(first.start, end);
                let symbol = self
                    .symbols
                    .intern_identifier(&name)
                    .map_err(|error| Diagnostic::at("LEX001", span, error.to_string()))?;
                return Ok(Token {
                    kind: TokenKind::Lifetime(symbol),
                    lexeme: Arc::from(raw),
                    span,
                });
            }
        }
        self.quoted(first, '\'')
    }

    fn quoted(&mut self, first: Character, delimiter: char) -> Result<Token, Diagnostic> {
        let mut raw = String::new();
        raw.push(delimiter);
        let mut decoded = String::new();
        loop {
            let Some(character) = self.next_character()? else {
                return Err(Diagnostic::at(
                    "LEX002",
                    self.span(first.start, self.characters.position),
                    "unterminated quoted literal",
                ));
            };
            raw.push(character.value);
            if character.value == delimiter {
                let span = self.span(first.start, character.end);
                let lexeme: Arc<str> = Arc::from(raw);
                if delimiter == '\'' {
                    let mut characters = decoded.chars();
                    let Some(value) = characters.next() else {
                        return Err(Diagnostic::at(
                            "LEX002",
                            span,
                            "character literal must contain exactly one Unicode scalar",
                        ));
                    };
                    if characters.next().is_some() {
                        return Err(Diagnostic::at(
                            "LEX002",
                            span,
                            "character literal must contain exactly one Unicode scalar",
                        ));
                    }
                    return Ok(Token {
                        kind: TokenKind::Character(value),
                        lexeme,
                        span,
                    });
                }
                return Ok(Token {
                    kind: TokenKind::String(Arc::from(decoded)),
                    lexeme,
                    span,
                });
            }
            if character.value == '\\' {
                let value = self.escape(first.start, &mut raw)?;
                decoded.push(value);
                continue;
            }
            if character.value.is_control() {
                return Err(Diagnostic::at(
                    "LEX002",
                    self.span(character.start, character.end),
                    "unescaped control character in quoted literal",
                ));
            }
            decoded.push(character.value);
        }
    }

    fn escape(&mut self, start: SourcePosition, raw: &mut String) -> Result<char, Diagnostic> {
        let Some(escape) = self.next_character()? else {
            return Err(Diagnostic::at(
                "LEX002",
                self.span(start, self.characters.position),
                "unterminated escape sequence",
            ));
        };
        raw.push(escape.value);
        Ok(match escape.value {
            '\\' => '\\',
            '"' => '"',
            '\'' => '\'',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '0' => '\0',
            'x' => {
                let mut value = 0_u32;
                for _ in 0..2 {
                    let Some(digit) = self.next_character()? else {
                        return Err(Diagnostic::at(
                            "LEX002",
                            self.span(start, self.characters.position),
                            "`\\x` escape requires exactly two hexadecimal digits",
                        ));
                    };
                    raw.push(digit.value);
                    let Some(digit) = digit.value.to_digit(16) else {
                        return Err(Diagnostic::at(
                            "LEX002",
                            self.span(digit.start, digit.end),
                            "`\\x` escape requires exactly two hexadecimal digits",
                        ));
                    };
                    value = value * 16 + digit;
                }
                if value > 0x7f {
                    return Err(Diagnostic::at(
                        "LEX002",
                        self.span(start, self.characters.position),
                        "`\\x` escape must be at most 0x7F",
                    ));
                }
                char::from_u32(value).expect("ASCII escape is a Unicode scalar")
            }
            'u' => {
                let Some(open) = self.next_character()? else {
                    return Err(Diagnostic::at(
                        "LEX002",
                        self.span(start, self.characters.position),
                        "Unicode escape requires `{`",
                    ));
                };
                raw.push(open.value);
                if open.value != '{' {
                    return Err(Diagnostic::at(
                        "LEX002",
                        self.span(open.start, open.end),
                        "Unicode escape requires `{`",
                    ));
                }
                let mut value = 0_u32;
                let mut digits = 0_u8;
                loop {
                    let Some(character) = self.next_character()? else {
                        return Err(Diagnostic::at(
                            "LEX002",
                            self.span(start, self.characters.position),
                            "unterminated Unicode escape",
                        ));
                    };
                    raw.push(character.value);
                    if character.value == '}' {
                        break;
                    }
                    let Some(digit) = character.value.to_digit(16) else {
                        return Err(Diagnostic::at(
                            "LEX002",
                            self.span(character.start, character.end),
                            "Unicode escape contains a non-hexadecimal digit",
                        ));
                    };
                    digits = digits.checked_add(1).ok_or_else(|| {
                        Diagnostic::at(
                            "LEX002",
                            self.span(start, character.end),
                            "Unicode escape has more than six digits",
                        )
                    })?;
                    if digits > 6 {
                        return Err(Diagnostic::at(
                            "LEX002",
                            self.span(start, character.end),
                            "Unicode escape has more than six digits",
                        ));
                    }
                    value = value * 16 + digit;
                }
                if digits == 0 {
                    return Err(Diagnostic::at(
                        "LEX002",
                        self.span(start, self.characters.position),
                        "Unicode escape requires one through six hexadecimal digits",
                    ));
                }
                char::from_u32(value).ok_or_else(|| {
                    Diagnostic::at(
                        "LEX002",
                        self.span(start, self.characters.position),
                        "Unicode escape is not a scalar value",
                    )
                })?
            }
            _ => {
                return Err(Diagnostic::at(
                    "LEX002",
                    self.span(escape.start, escape.end),
                    "unknown escape sequence",
                ));
            }
        })
    }

    fn number(&mut self, first: Character) -> Result<Token, Diagnostic> {
        let mut raw = String::new();
        raw.push(first.value);
        let mut end = first.end;
        let mut base = NumericBase::Decimal;
        let mut prefix_length = 0_usize;
        if first.value == '0' {
            base = match self.peek_value(0)? {
                Some('b') => NumericBase::Binary,
                Some('o') => NumericBase::Octal,
                Some('x') => NumericBase::Hexadecimal,
                _ => NumericBase::Decimal,
            };
            if base != NumericBase::Decimal {
                let prefix = self.next_character()?.expect("peeked character");
                raw.push(prefix.value);
                end = prefix.end;
                prefix_length = 2;
            }
        }

        self.consume_numeric_digits(base, &mut raw, &mut end)?;
        let mut is_float = false;
        if matches!(base, NumericBase::Decimal | NumericBase::Hexadecimal)
            && self.peek_value(0)? == Some('.')
            && self.peek_value(1)? != Some('.')
        {
            is_float = true;
            let point = self.next_character()?.expect("peeked character");
            raw.push('.');
            end = point.end;
            self.consume_numeric_digits(base, &mut raw, &mut end)?;
        }

        let exponent_marker = match base {
            NumericBase::Decimal => matches!(self.peek_value(0)?, Some('e' | 'E')),
            NumericBase::Hexadecimal => matches!(self.peek_value(0)?, Some('p' | 'P')),
            NumericBase::Binary | NumericBase::Octal => false,
        };
        if exponent_marker {
            is_float = true;
            let exponent = self.next_character()?.expect("peeked character");
            raw.push(exponent.value);
            end = exponent.end;
            if matches!(self.peek_value(0)?, Some('+' | '-')) {
                let sign = self.next_character()?.expect("peeked character");
                raw.push(sign.value);
                end = sign.end;
            }
            self.consume_numeric_digits(NumericBase::Decimal, &mut raw, &mut end)?;
        }

        while self.peek_value(0)?.is_some_and(|value| {
            value == '_' || value.is_ascii_alphanumeric() || unicode_ident::is_xid_continue(value)
        }) {
            let character = self.next_character()?.expect("peeked character");
            raw.push(character.value);
            end = character.end;
        }

        let span = self.span(first.start, end);
        let parsed = parse_numeric(&raw, base, prefix_length, is_float)
            .map_err(|message| Diagnostic::at("LEX002", span, message))?;
        Ok(match parsed {
            ParsedNumeric::Integer { digits, suffix } => Token {
                kind: TokenKind::Integer(IntegerLiteral {
                    base,
                    digits: Arc::from(digits),
                    suffix,
                    raw: Arc::from(raw.as_str()),
                }),
                lexeme: Arc::from(raw),
                span,
            },
            ParsedNumeric::Float { suffix } => Token {
                kind: TokenKind::Float(FloatLiteral {
                    base,
                    raw: Arc::from(raw.as_str()),
                    suffix,
                }),
                lexeme: Arc::from(raw),
                span,
            },
        })
    }

    fn consume_numeric_digits(
        &mut self,
        base: NumericBase,
        raw: &mut String,
        end: &mut SourcePosition,
    ) -> Result<(), Diagnostic> {
        while self.peek_value(0)?.is_some_and(|value| {
            value == '_'
                || match base {
                    NumericBase::Binary => value.is_ascii_digit(),
                    NumericBase::Octal => value.is_ascii_digit(),
                    NumericBase::Decimal => value.is_ascii_digit(),
                    NumericBase::Hexadecimal => value.is_ascii_hexdigit(),
                }
        }) {
            let character = self.next_character()?.expect("peeked character");
            raw.push(character.value);
            *end = character.end;
        }
        Ok(())
    }

    fn punctuation(&mut self, first: Character) -> Result<Token, Diagnostic> {
        let (kind, mut end) = match first.value {
            ':' if self.peek_value(0)? == Some(':') => {
                (Punctuation::ColonColon, self.consume_peeked()?.end)
            }
            '-' if self.peek_value(0)? == Some('>') => {
                (Punctuation::Arrow, self.consume_peeked()?.end)
            }
            '=' if self.peek_value(0)? == Some('>') => {
                (Punctuation::FatArrow, self.consume_peeked()?.end)
            }
            '.' if self.peek_value(0)? == Some('.') => {
                let second = self.consume_peeked()?;
                if self.peek_value(0)? == Some('=') {
                    (Punctuation::RangeInclusive, self.consume_peeked()?.end)
                } else {
                    (Punctuation::Range, second.end)
                }
            }
            '<' if self.peek_value(0)? == Some('<') => {
                (Punctuation::ShiftLeft, self.consume_peeked()?.end)
            }
            '>' if self.peek_value(0)? == Some('>') => {
                (Punctuation::ShiftRight, self.consume_peeked()?.end)
            }
            '<' if self.peek_value(0)? == Some('=') => {
                (Punctuation::LessEqual, self.consume_peeked()?.end)
            }
            '>' if self.peek_value(0)? == Some('=') => {
                (Punctuation::GreaterEqual, self.consume_peeked()?.end)
            }
            '=' if self.peek_value(0)? == Some('=') => {
                (Punctuation::EqualEqual, self.consume_peeked()?.end)
            }
            '!' if self.peek_value(0)? == Some('=') => {
                (Punctuation::NotEqual, self.consume_peeked()?.end)
            }
            '&' if self.peek_value(0)? == Some('&') => {
                (Punctuation::LogicalAnd, self.consume_peeked()?.end)
            }
            '|' if self.peek_value(0)? == Some('|') => {
                (Punctuation::LogicalOr, self.consume_peeked()?.end)
            }
            '+' if self.peek_value(0)? == Some('=') => {
                (Punctuation::AddAssign, self.consume_peeked()?.end)
            }
            '{' => (Punctuation::LeftBrace, first.end),
            '}' => (Punctuation::RightBrace, first.end),
            '(' => (Punctuation::LeftParen, first.end),
            ')' => (Punctuation::RightParen, first.end),
            '[' => (Punctuation::LeftBracket, first.end),
            ']' => (Punctuation::RightBracket, first.end),
            ',' => (Punctuation::Comma, first.end),
            ';' => (Punctuation::Semicolon, first.end),
            ':' => (Punctuation::Colon, first.end),
            '.' => (Punctuation::Dot, first.end),
            '@' => (Punctuation::At, first.end),
            '|' => (Punctuation::Pipe, first.end),
            '+' => (Punctuation::Plus, first.end),
            '-' => (Punctuation::Minus, first.end),
            '*' => (Punctuation::Star, first.end),
            '/' => (Punctuation::Slash, first.end),
            '%' => (Punctuation::Percent, first.end),
            '&' => (Punctuation::Ampersand, first.end),
            '^' => (Punctuation::Caret, first.end),
            '!' => (Punctuation::Bang, first.end),
            '~' => (Punctuation::Tilde, first.end),
            '=' => (Punctuation::Equal, first.end),
            '<' => (Punctuation::Less, first.end),
            '>' => (Punctuation::Greater, first.end),
            other => {
                return Err(Diagnostic::at(
                    "LEX001",
                    self.span(first.start, first.end),
                    format!("character {other:?} is not an Arche 0.1 token"),
                ));
            }
        };
        if end.byte < first.end.byte {
            end = first.end;
        }
        let spelling = kind.spelling();
        Ok(Token {
            kind: TokenKind::Punctuation(kind),
            lexeme: Arc::from(spelling),
            span: self.span(first.start, end),
        })
    }

    fn doc_comment(&mut self, first: Character, third: Character) -> Result<Token, Diagnostic> {
        let mut text = String::new();
        let mut lexeme = String::from("///");
        let mut end = third.end;
        while let Some(character) = self.characters.peek(0).map_err(source_read_error)? {
            if matches!(character.value, '\r' | '\n') {
                break;
            }
            let character = self.next_character()?.expect("peeked character");
            text.push(character.value);
            lexeme.push(character.value);
            end = character.end;
        }
        Ok(Token {
            kind: TokenKind::DocComment(Arc::from(text)),
            lexeme: Arc::from(lexeme),
            span: self.span(first.start, end),
        })
    }

    fn line_comment(&mut self) -> Result<(), Diagnostic> {
        while let Some(character) = self.characters.peek(0).map_err(source_read_error)? {
            if matches!(character.value, '\r' | '\n') {
                break;
            }
            self.next_character()?;
        }
        Ok(())
    }

    fn block_comment(&mut self, first: Character, star: Character) -> Result<(), Diagnostic> {
        let mut depth = 1_u64;
        let mut end = star.end;
        while depth != 0 {
            let Some(character) = self.next_character()? else {
                return Err(Diagnostic::at(
                    "LEX002",
                    self.span(first.start, end),
                    "unterminated block comment",
                ));
            };
            end = character.end;
            if character.value == '/' && self.peek_value(0)? == Some('*') {
                let nested = self.consume_peeked()?;
                end = nested.end;
                depth = depth.checked_add(1).ok_or_else(|| {
                    Diagnostic::at(
                        "LEX002",
                        self.span(first.start, end),
                        "nested block-comment depth exceeds u64",
                    )
                })?;
            } else if character.value == '*' && self.peek_value(0)? == Some('/') {
                let close = self.consume_peeked()?;
                end = close.end;
                depth -= 1;
            }
        }
        Ok(())
    }

    fn next_character(&mut self) -> Result<Option<Character>, Diagnostic> {
        self.characters.next().map_err(source_read_error)
    }

    fn peek_value(&mut self, index: usize) -> Result<Option<char>, Diagnostic> {
        self.characters
            .peek(index)
            .map(|character| character.map(|character| character.value))
            .map_err(source_read_error)
    }

    fn consume_peeked(&mut self) -> Result<Character, Diagnostic> {
        self.next_character()
            .map(|value| value.expect("peeked character"))
    }

    const fn span(&self, start: SourcePosition, end: SourcePosition) -> Span {
        Span {
            file: self.file,
            start,
            end,
        }
    }
}

enum ParsedNumeric {
    Integer {
        digits: String,
        suffix: Option<IntegerSuffix>,
    },
    Float {
        suffix: Option<FloatSuffix>,
    },
}

fn parse_numeric(
    raw: &str,
    base: NumericBase,
    prefix_length: usize,
    is_float: bool,
) -> Result<ParsedNumeric, &'static str> {
    if is_float {
        let (body, suffix) = strip_float_suffix(raw);
        validate_float(body, base)?;
        return Ok(ParsedNumeric::Float { suffix });
    }
    let (body, suffix) = strip_integer_suffix(raw);
    let digits = body.get(prefix_length..).ok_or("invalid numeric prefix")?;
    if !valid_digit_sequence(digits, base) {
        return Err("invalid integer digit or separator sequence");
    }
    Ok(ParsedNumeric::Integer {
        digits: digits
            .chars()
            .filter(|character| *character != '_')
            .collect(),
        suffix,
    })
}

fn validate_float(raw: &str, base: NumericBase) -> Result<(), &'static str> {
    let (marker_lower, marker_upper) = match base {
        NumericBase::Decimal => ('e', 'E'),
        NumericBase::Hexadecimal => ('p', 'P'),
        NumericBase::Binary | NumericBase::Octal => return Err("invalid floating-point base"),
    };
    let exponent = raw
        .char_indices()
        .find(|(_, character)| matches!(*character, value if value == marker_lower || value == marker_upper));
    let (significand, exponent_digits) = match exponent {
        Some((index, _)) => {
            let after = &raw[index + 1..];
            let after = after.strip_prefix(['+', '-']).unwrap_or(after);
            (&raw[..index], Some(after))
        }
        None => (raw, None),
    };
    let (whole, fraction) = match significand.split_once('.') {
        Some(parts) => parts,
        None if base == NumericBase::Decimal && exponent_digits.is_some() => (significand, ""),
        None => return Err("floating literal requires a decimal point or exponent"),
    };
    let whole = if base == NumericBase::Hexadecimal {
        whole
            .strip_prefix("0x")
            .ok_or("hexadecimal float requires `0x`")?
    } else {
        whole
    };
    if !valid_digit_sequence(whole, base) {
        return Err("invalid floating-point whole-digit sequence");
    }
    if !fraction.is_empty() && !valid_digit_sequence(fraction, base) {
        return Err("invalid floating-point fractional-digit sequence");
    }
    if base == NumericBase::Hexadecimal && exponent_digits.is_none() {
        return Err("hexadecimal float requires a binary exponent");
    }
    if let Some(exponent) = exponent_digits {
        if !valid_digit_sequence(exponent, NumericBase::Decimal) {
            return Err("invalid floating-point exponent");
        }
    }
    Ok(())
}

fn valid_digit_sequence(value: &str, base: NumericBase) -> bool {
    if value.is_empty() || value.starts_with('_') || value.ends_with('_') {
        return false;
    }
    let mut previous_separator = false;
    for character in value.chars() {
        if character == '_' {
            if previous_separator {
                return false;
            }
            previous_separator = true;
            continue;
        }
        let valid = match base {
            NumericBase::Binary => matches!(character, '0' | '1'),
            NumericBase::Octal => matches!(character, '0'..='7'),
            NumericBase::Decimal => character.is_ascii_digit(),
            NumericBase::Hexadecimal => character.is_ascii_hexdigit(),
        };
        if !valid {
            return false;
        }
        previous_separator = false;
    }
    !previous_separator
}

fn strip_integer_suffix(raw: &str) -> (&str, Option<IntegerSuffix>) {
    for (text, suffix) in [
        ("isize", IntegerSuffix::Isize),
        ("usize", IntegerSuffix::Usize),
        ("i16", IntegerSuffix::I16),
        ("i32", IntegerSuffix::I32),
        ("i64", IntegerSuffix::I64),
        ("u16", IntegerSuffix::U16),
        ("u32", IntegerSuffix::U32),
        ("u64", IntegerSuffix::U64),
        ("i8", IntegerSuffix::I8),
        ("u8", IntegerSuffix::U8),
    ] {
        if let Some(body) = raw.strip_suffix(text) {
            return (body, Some(suffix));
        }
    }
    (raw, None)
}

fn strip_float_suffix(raw: &str) -> (&str, Option<FloatSuffix>) {
    if let Some(body) = raw.strip_suffix("f32") {
        (body, Some(FloatSuffix::F32))
    } else if let Some(body) = raw.strip_suffix("f64") {
        (body, Some(FloatSuffix::F64))
    } else {
        (raw, None)
    }
}

fn keyword(value: &str) -> Option<Keyword> {
    Some(match value {
        "as" => Keyword::As,
        "break" => Keyword::Break,
        "catch" => Keyword::Catch,
        "commands" => Keyword::Commands,
        "component" => Keyword::Component,
        "const" => Keyword::Const,
        "continue" => Keyword::Continue,
        "default" => Keyword::Default,
        "else" => Keyword::Else,
        "enum" => Keyword::Enum,
        "entity" => Keyword::Entity,
        "false" => Keyword::False,
        "fn" => Keyword::Fn,
        "for" => Keyword::For,
        "gen" => Keyword::Gen,
        "if" => Keyword::If,
        "impl" => Keyword::Impl,
        "in" => Keyword::In,
        "init" => Keyword::Init,
        "let" => Keyword::Let,
        "loop" => Keyword::Loop,
        "match" => Keyword::Match,
        "mod" => Keyword::Mod,
        "move" => Keyword::Move,
        "mut" => Keyword::Mut,
        "package" => Keyword::Package,
        "pub" => Keyword::Pub,
        "query" => Keyword::Query,
        "read" => Keyword::Read,
        "ref" => Keyword::Ref,
        "requires" => Keyword::Requires,
        "resource" => Keyword::Resource,
        "resume" => Keyword::Resume,
        "return" => Keyword::Return,
        "run" => Keyword::Run,
        "schedule" => Keyword::Schedule,
        "Self" => Keyword::SelfType,
        "self" => Keyword::SelfValue,
        "spawn" => Keyword::Spawn,
        "static" => Keyword::Static,
        "str" => Keyword::Str,
        "struct" => Keyword::Struct,
        "super" => Keyword::Super,
        "system" => Keyword::System,
        "tag" => Keyword::Tag,
        "throw" => Keyword::Throw,
        "throws" => Keyword::Throws,
        "trait" => Keyword::Trait,
        "true" => Keyword::True,
        "type" => Keyword::Type,
        "unsafe" => Keyword::Unsafe,
        "use" => Keyword::Use,
        "where" => Keyword::Where,
        "while" => Keyword::While,
        "world" => Keyword::World,
        "yield" => Keyword::Yield,
        "yields" => Keyword::Yields,
        "i8" => Keyword::I8,
        "i16" => Keyword::I16,
        "i32" => Keyword::I32,
        "i64" => Keyword::I64,
        "isize" => Keyword::Isize,
        "u8" => Keyword::U8,
        "u16" => Keyword::U16,
        "u32" => Keyword::U32,
        "u64" => Keyword::U64,
        "usize" => Keyword::Usize,
        "f32" => Keyword::F32,
        "f64" => Keyword::F64,
        "bool" => Keyword::Bool,
        "char" => Keyword::Char,
        _ => return None,
    })
}

const fn is_lexical_whitespace(value: char) -> bool {
    matches!(value, ' ' | '\t' | '\r' | '\n')
}

fn source_read_error(error: io::Error) -> Diagnostic {
    Diagnostic::path(
        "SOURCE002",
        format!("could not read retained source: {error}"),
    )
}

fn utf8_width(first: u8) -> Option<usize> {
    match first {
        0x00..=0x7f => Some(1),
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

fn invalid_utf8(offset: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("source is not valid UTF-8 at byte {offset}"),
    )
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use sha2::{Digest, Sha256};

    use super::*;

    const ALL_KEYWORDS: &[(Keyword, &str)] = &[
        (Keyword::As, "as"),
        (Keyword::Break, "break"),
        (Keyword::Catch, "catch"),
        (Keyword::Commands, "commands"),
        (Keyword::Component, "component"),
        (Keyword::Const, "const"),
        (Keyword::Continue, "continue"),
        (Keyword::Default, "default"),
        (Keyword::Else, "else"),
        (Keyword::Enum, "enum"),
        (Keyword::Entity, "entity"),
        (Keyword::False, "false"),
        (Keyword::Fn, "fn"),
        (Keyword::For, "for"),
        (Keyword::Gen, "gen"),
        (Keyword::If, "if"),
        (Keyword::Impl, "impl"),
        (Keyword::In, "in"),
        (Keyword::Init, "init"),
        (Keyword::Let, "let"),
        (Keyword::Loop, "loop"),
        (Keyword::Match, "match"),
        (Keyword::Mod, "mod"),
        (Keyword::Move, "move"),
        (Keyword::Mut, "mut"),
        (Keyword::Package, "package"),
        (Keyword::Pub, "pub"),
        (Keyword::Query, "query"),
        (Keyword::Read, "read"),
        (Keyword::Ref, "ref"),
        (Keyword::Requires, "requires"),
        (Keyword::Resource, "resource"),
        (Keyword::Resume, "resume"),
        (Keyword::Return, "return"),
        (Keyword::Run, "run"),
        (Keyword::Schedule, "schedule"),
        (Keyword::SelfType, "Self"),
        (Keyword::SelfValue, "self"),
        (Keyword::Spawn, "spawn"),
        (Keyword::Static, "static"),
        (Keyword::Str, "str"),
        (Keyword::Struct, "struct"),
        (Keyword::Super, "super"),
        (Keyword::System, "system"),
        (Keyword::Tag, "tag"),
        (Keyword::Throw, "throw"),
        (Keyword::Throws, "throws"),
        (Keyword::Trait, "trait"),
        (Keyword::True, "true"),
        (Keyword::Type, "type"),
        (Keyword::Unsafe, "unsafe"),
        (Keyword::Use, "use"),
        (Keyword::Where, "where"),
        (Keyword::While, "while"),
        (Keyword::World, "world"),
        (Keyword::Yield, "yield"),
        (Keyword::Yields, "yields"),
        (Keyword::I8, "i8"),
        (Keyword::I16, "i16"),
        (Keyword::I32, "i32"),
        (Keyword::I64, "i64"),
        (Keyword::Isize, "isize"),
        (Keyword::U8, "u8"),
        (Keyword::U16, "u16"),
        (Keyword::U32, "u32"),
        (Keyword::U64, "u64"),
        (Keyword::Usize, "usize"),
        (Keyword::F32, "f32"),
        (Keyword::F64, "f64"),
        (Keyword::Bool, "bool"),
        (Keyword::Char, "char"),
    ];

    const ALL_PUNCTUATION: &[Punctuation] = &[
        Punctuation::ColonColon,
        Punctuation::Arrow,
        Punctuation::FatArrow,
        Punctuation::RangeInclusive,
        Punctuation::Range,
        Punctuation::ShiftLeft,
        Punctuation::ShiftRight,
        Punctuation::LessEqual,
        Punctuation::GreaterEqual,
        Punctuation::EqualEqual,
        Punctuation::NotEqual,
        Punctuation::LogicalAnd,
        Punctuation::LogicalOr,
        Punctuation::AddAssign,
        Punctuation::LeftBrace,
        Punctuation::RightBrace,
        Punctuation::LeftParen,
        Punctuation::RightParen,
        Punctuation::LeftBracket,
        Punctuation::RightBracket,
        Punctuation::Comma,
        Punctuation::Semicolon,
        Punctuation::Colon,
        Punctuation::Dot,
        Punctuation::At,
        Punctuation::Pipe,
        Punctuation::Plus,
        Punctuation::Minus,
        Punctuation::Star,
        Punctuation::Slash,
        Punctuation::Percent,
        Punctuation::Ampersand,
        Punctuation::Caret,
        Punctuation::Bang,
        Punctuation::Tilde,
        Punctuation::Equal,
        Punctuation::Less,
        Punctuation::Greater,
    ];

    fn lex(source: &[u8]) -> Result<Vec<Token>, Diagnostic> {
        let mut lexer = Lexer::new(FileId(7), Cursor::new(source));
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token()?;
            let eof = token.kind == TokenKind::Eof;
            tokens.push(token);
            if eof {
                return Ok(tokens);
            }
        }
    }

    const fn keyword_golden_tag(keyword: Keyword) -> u8 {
        match keyword {
            Keyword::As => 1,
            Keyword::Break => 2,
            Keyword::Catch => 3,
            Keyword::Commands => 4,
            Keyword::Component => 5,
            Keyword::Const => 6,
            Keyword::Continue => 7,
            Keyword::Default => 8,
            Keyword::Else => 9,
            Keyword::Enum => 10,
            Keyword::Entity => 11,
            Keyword::False => 12,
            Keyword::Fn => 13,
            Keyword::For => 14,
            Keyword::Gen => 15,
            Keyword::If => 16,
            Keyword::Impl => 17,
            Keyword::In => 18,
            Keyword::Init => 19,
            Keyword::Let => 20,
            Keyword::Loop => 21,
            Keyword::Match => 22,
            Keyword::Mod => 23,
            Keyword::Move => 24,
            Keyword::Mut => 25,
            Keyword::Package => 26,
            Keyword::Pub => 27,
            Keyword::Query => 28,
            Keyword::Read => 29,
            Keyword::Ref => 30,
            Keyword::Requires => 31,
            Keyword::Resource => 32,
            Keyword::Resume => 33,
            Keyword::Return => 34,
            Keyword::Run => 35,
            Keyword::Schedule => 36,
            Keyword::SelfType => 37,
            Keyword::SelfValue => 38,
            Keyword::Spawn => 39,
            Keyword::Static => 40,
            Keyword::Str => 41,
            Keyword::Struct => 42,
            Keyword::Super => 43,
            Keyword::System => 44,
            Keyword::Tag => 45,
            Keyword::Throw => 46,
            Keyword::Throws => 47,
            Keyword::Trait => 48,
            Keyword::True => 49,
            Keyword::Type => 50,
            Keyword::Unsafe => 51,
            Keyword::Use => 52,
            Keyword::Where => 53,
            Keyword::While => 54,
            Keyword::World => 55,
            Keyword::Yield => 56,
            Keyword::Yields => 57,
            Keyword::I8 => 58,
            Keyword::I16 => 59,
            Keyword::I32 => 60,
            Keyword::I64 => 61,
            Keyword::Isize => 62,
            Keyword::U8 => 63,
            Keyword::U16 => 64,
            Keyword::U32 => 65,
            Keyword::U64 => 66,
            Keyword::Usize => 67,
            Keyword::F32 => 68,
            Keyword::F64 => 69,
            Keyword::Bool => 70,
            Keyword::Char => 71,
        }
    }

    const fn punctuation_golden_tag(punctuation: Punctuation) -> u8 {
        match punctuation {
            Punctuation::ColonColon => 1,
            Punctuation::Arrow => 2,
            Punctuation::FatArrow => 3,
            Punctuation::RangeInclusive => 4,
            Punctuation::Range => 5,
            Punctuation::ShiftLeft => 6,
            Punctuation::ShiftRight => 7,
            Punctuation::LessEqual => 8,
            Punctuation::GreaterEqual => 9,
            Punctuation::EqualEqual => 10,
            Punctuation::NotEqual => 11,
            Punctuation::LogicalAnd => 12,
            Punctuation::LogicalOr => 13,
            Punctuation::AddAssign => 14,
            Punctuation::LeftBrace => 15,
            Punctuation::RightBrace => 16,
            Punctuation::LeftParen => 17,
            Punctuation::RightParen => 18,
            Punctuation::LeftBracket => 19,
            Punctuation::RightBracket => 20,
            Punctuation::Comma => 21,
            Punctuation::Semicolon => 22,
            Punctuation::Colon => 23,
            Punctuation::Dot => 24,
            Punctuation::At => 25,
            Punctuation::Pipe => 26,
            Punctuation::Plus => 27,
            Punctuation::Minus => 28,
            Punctuation::Star => 29,
            Punctuation::Slash => 30,
            Punctuation::Percent => 31,
            Punctuation::Ampersand => 32,
            Punctuation::Caret => 33,
            Punctuation::Bang => 34,
            Punctuation::Tilde => 35,
            Punctuation::Equal => 36,
            Punctuation::Less => 37,
            Punctuation::Greater => 38,
        }
    }

    const fn numeric_base_golden_tag(base: NumericBase) -> u8 {
        match base {
            NumericBase::Binary => 1,
            NumericBase::Octal => 2,
            NumericBase::Decimal => 3,
            NumericBase::Hexadecimal => 4,
        }
    }

    const fn integer_suffix_golden_tag(suffix: IntegerSuffix) -> u8 {
        match suffix {
            IntegerSuffix::I8 => 1,
            IntegerSuffix::I16 => 2,
            IntegerSuffix::I32 => 3,
            IntegerSuffix::I64 => 4,
            IntegerSuffix::Isize => 5,
            IntegerSuffix::U8 => 6,
            IntegerSuffix::U16 => 7,
            IntegerSuffix::U32 => 8,
            IntegerSuffix::U64 => 9,
            IntegerSuffix::Usize => 10,
        }
    }

    const fn float_suffix_golden_tag(suffix: FloatSuffix) -> u8 {
        match suffix {
            FloatSuffix::F32 => 1,
            FloatSuffix::F64 => 2,
        }
    }

    fn push_golden_bytes(output: &mut Vec<u8>, bytes: &[u8]) {
        output.extend_from_slice(&u64::try_from(bytes.len()).unwrap().to_le_bytes());
        output.extend_from_slice(bytes);
    }

    fn push_golden_span(output: &mut Vec<u8>, span: Option<Span>) {
        let Some(span) = span else {
            output.push(0);
            return;
        };
        output.push(1);
        for value in [
            span.file.0,
            span.start.byte,
            span.start.line,
            span.start.column,
            span.end.byte,
            span.end.line,
            span.end.column,
        ] {
            output.extend_from_slice(&value.to_le_bytes());
        }
    }

    fn push_golden_diagnostic(output: &mut Vec<u8>, source: &[u8], error: &Diagnostic) {
        push_golden_bytes(output, source);
        push_golden_bytes(output, error.code.as_bytes());
        push_golden_bytes(output, error.message.as_bytes());
        push_golden_bytes(output, error.primary.message.as_bytes());
        push_golden_span(output, error.primary.span);
        output.extend_from_slice(&u64::try_from(error.secondary.len()).unwrap().to_le_bytes());
        for label in &error.secondary {
            push_golden_bytes(output, label.message.as_bytes());
            push_golden_span(output, label.span);
        }
        output.extend_from_slice(&u64::try_from(error.notes.len()).unwrap().to_le_bytes());
        for note in &error.notes {
            push_golden_bytes(output, note.as_bytes());
        }
    }

    fn encode_complete_token_stream(tokens: &[Token]) -> Vec<u8> {
        let mut output = b"ARCHE-C1-TOKEN-GOLDEN\0".to_vec();
        output.extend_from_slice(&1_u32.to_le_bytes());
        output.extend_from_slice(&u64::try_from(tokens.len()).unwrap().to_le_bytes());
        for token in tokens {
            match &token.kind {
                TokenKind::Keyword(keyword) => {
                    output.push(1);
                    output.push(keyword_golden_tag(*keyword));
                }
                TokenKind::Identifier(identifier) => {
                    output.push(2);
                    push_golden_bytes(&mut output, identifier.as_str().as_bytes());
                }
                TokenKind::Lifetime(lifetime) => {
                    output.push(3);
                    push_golden_bytes(&mut output, lifetime.as_str().as_bytes());
                }
                TokenKind::Wildcard => output.push(4),
                TokenKind::DocComment(comment) => {
                    output.push(5);
                    push_golden_bytes(&mut output, comment.as_bytes());
                }
                TokenKind::Integer(integer) => {
                    output.push(6);
                    output.push(numeric_base_golden_tag(integer.base));
                    push_golden_bytes(&mut output, integer.digits.as_bytes());
                    output.push(integer.suffix.map_or(0, integer_suffix_golden_tag));
                    push_golden_bytes(&mut output, integer.raw.as_bytes());
                }
                TokenKind::Float(float) => {
                    output.push(7);
                    output.push(numeric_base_golden_tag(float.base));
                    output.push(float.suffix.map_or(0, float_suffix_golden_tag));
                    push_golden_bytes(&mut output, float.raw.as_bytes());
                }
                TokenKind::Character(character) => {
                    output.push(8);
                    output.extend_from_slice(&u32::from(*character).to_le_bytes());
                }
                TokenKind::String(string) => {
                    output.push(9);
                    push_golden_bytes(&mut output, string.as_bytes());
                }
                TokenKind::Punctuation(punctuation) => {
                    output.push(10);
                    output.push(punctuation_golden_tag(*punctuation));
                }
                TokenKind::Eof => output.push(11),
            }
            push_golden_bytes(&mut output, token.lexeme.as_bytes());
            for value in [
                token.span.file.0,
                token.span.start.byte,
                token.span.start.line,
                token.span.start.column,
                token.span.end.byte,
                token.span.end.line,
                token.span.end.column,
            ] {
                output.extend_from_slice(&value.to_le_bytes());
            }
        }
        output
    }

    fn lowercase_hex(bytes: &[u8]) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            output.push(char::from(DIGITS[usize::from(byte >> 4)]));
            output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
        }
        output
    }

    #[test]
    fn lexes_every_longest_match_operator() {
        let source = b":: -> => ..= .. << >> <= >= == != && || += { } ( ) [ ] , ; : . @ | + - * / % & ^ ! ~ = < >";
        let tokens = lex(source).unwrap();
        let spellings = tokens
            .iter()
            .filter_map(|token| match token.kind {
                TokenKind::Punctuation(value) => Some(value.spelling()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            spellings,
            [
                "::", "->", "=>", "..=", "..", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||",
                "+=", "{", "}", "(", ")", "[", "]", ",", ";", ":", ".", "@", "|", "+", "-", "*",
                "/", "%", "&", "^", "!", "~", "=", "<", ">"
            ]
        );
    }

    #[test]
    fn rejects_a_nonexistent_operator_with_an_exact_diagnostic() {
        let error = lex(b"?").unwrap_err();
        assert_eq!(error.code, "LEX001");
        assert_eq!(error.message, "character '?' is not an Arche 0.1 token");
        assert_eq!(error.primary.message, error.message);
        assert_eq!(
            error.primary.span,
            Some(Span {
                file: FileId(7),
                start: SourcePosition::START,
                end: SourcePosition {
                    byte: 1,
                    line: 1,
                    column: 2,
                },
            })
        );
        assert!(error.secondary.is_empty());
        assert!(error.notes.is_empty());
    }

    #[test]
    fn distinguishes_ranges_decimal_points_and_invalid_field_like_numbers() {
        let tokens = lex(b"1..2 1..=2 1. 0x1.fp2f32").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Integer(_)));
        assert_eq!(tokens[1].kind, TokenKind::Punctuation(Punctuation::Range));
        assert!(matches!(
            tokens[4].kind,
            TokenKind::Punctuation(Punctuation::RangeInclusive)
        ));
        assert!(matches!(tokens[6].kind, TokenKind::Float(_)));
        assert!(matches!(tokens[7].kind, TokenKind::Float(_)));
        assert_eq!(lex(b"1.foo").unwrap_err().code, "LEX002");
    }

    #[test]
    fn validates_exact_numeric_separator_and_base_grammar() {
        for valid in [
            &b"1_000u64"[..],
            &b"0b10_01"[..],
            &b"0o7_0"[..],
            &b"0xCA_FE"[..],
            &b"1e-2f64"[..],
            &b"0x1.8p+2"[..],
        ] {
            lex(valid).unwrap();
        }
        for invalid in [
            &b"1__0"[..],
            &b"0b2"[..],
            &b"0o8"[..],
            &b"0x"[..],
            &b"1e"[..],
            &b"0x1.0"[..],
            &b"12wat"[..],
        ] {
            assert_eq!(lex(invalid).unwrap_err().code, "LEX002");
        }
    }

    #[test]
    fn retains_doc_lifetime_identifier_and_decoded_literal_payloads() {
        let tokens =
            lex("/// exact Café\n'a 'z' \"a\\n\\u{1F642}\" _ Cafe\u{301}".as_bytes()).unwrap();
        assert_eq!(
            tokens[0].kind,
            TokenKind::DocComment(Arc::from(" exact Café"))
        );
        let TokenKind::Lifetime(name) = &tokens[1].kind else {
            panic!("lifetime expected")
        };
        assert_eq!(name.as_str(), "a");
        assert_eq!(tokens[2].kind, TokenKind::Character('z'));
        assert_eq!(tokens[3].kind, TokenKind::String(Arc::from("a\n🙂")));
        assert_eq!(tokens[4].kind, TokenKind::Wildcard);
        let TokenKind::Identifier(name) = &tokens[5].kind else {
            panic!("identifier expected")
        };
        assert_eq!(name.as_str(), "Café");
    }

    #[test]
    fn skips_nested_comments_and_rejects_nonexistent_doc_forms() {
        let tokens = lex(b"/* outer /* nested */ done */ component").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Keyword(Keyword::Component));
        assert_eq!(lex(b"//! no").unwrap_err().code, "LEX001");
        assert_eq!(lex(b"/** no */").unwrap_err().code, "LEX001");
        assert_eq!(lex(b"/* open").unwrap_err().code, "LEX002");
    }

    #[test]
    fn pins_multibyte_crlf_bare_cr_and_eof_spans() {
        let tokens = lex("é\t\r\n_\r_".as_bytes()).unwrap();
        assert_eq!(tokens[0].span.start, SourcePosition::START);
        assert_eq!(tokens[0].span.end.byte, 2);
        assert_eq!(tokens[1].span.start.line, 2);
        assert_eq!(tokens[1].span.start.column, 1);
        assert_eq!(tokens[2].span.start.line, 2);
        assert_eq!(tokens[2].span.start.column, 2);
        assert_eq!(tokens[3].kind, TokenKind::Eof);
        assert_eq!(tokens[3].span.start, tokens[3].span.end);
    }

    #[test]
    fn rejects_bom_malformed_utf8_and_literal_escapes() {
        assert_eq!(
            lex("\u{feff}component".as_bytes()).unwrap_err().code,
            "LEX001"
        );
        assert_eq!(lex(&[0xff]).unwrap_err().code, "SOURCE002");
        for source in [
            b"'ab'".as_slice(),
            b"\"\\x80\"",
            b"\"\\u{}\"",
            b"\"line\n\"",
        ] {
            assert_eq!(lex(source).unwrap_err().code, "LEX002");
        }
    }

    #[test]
    fn active_lexer_negative_matrix_has_exact_codes_spans_messages_and_note_order() {
        type ExpectedSpan = Option<(u64, u64, u64, u64, u64, u64)>;
        let cases: &[(&[u8], &str, &str, ExpectedSpan)] = &[
            (
                b"1.foo",
                "LEX002",
                "invalid floating-point fractional-digit sequence",
                Some((0, 1, 1, 5, 1, 6)),
            ),
            (
                b"1__0",
                "LEX002",
                "invalid integer digit or separator sequence",
                Some((0, 1, 1, 4, 1, 5)),
            ),
            (
                b"0b2",
                "LEX002",
                "invalid integer digit or separator sequence",
                Some((0, 1, 1, 3, 1, 4)),
            ),
            (
                b"0o8",
                "LEX002",
                "invalid integer digit or separator sequence",
                Some((0, 1, 1, 3, 1, 4)),
            ),
            (
                b"0x",
                "LEX002",
                "invalid integer digit or separator sequence",
                Some((0, 1, 1, 2, 1, 3)),
            ),
            (
                b"1e",
                "LEX002",
                "invalid floating-point exponent",
                Some((0, 1, 1, 2, 1, 3)),
            ),
            (
                b"0x1.0",
                "LEX002",
                "hexadecimal float requires a binary exponent",
                Some((0, 1, 1, 5, 1, 6)),
            ),
            (
                b"12wat",
                "LEX002",
                "invalid integer digit or separator sequence",
                Some((0, 1, 1, 5, 1, 6)),
            ),
            (
                b"//! no",
                "LEX001",
                "inner doc comments do not exist in Arche 0.1",
                Some((0, 1, 1, 3, 1, 4)),
            ),
            (
                b"/** no */",
                "LEX001",
                "block doc comments do not exist in Arche 0.1",
                Some((0, 1, 1, 3, 1, 4)),
            ),
            (
                b"/* open",
                "LEX002",
                "unterminated block comment",
                Some((0, 1, 1, 7, 1, 8)),
            ),
            (
                b"\xef\xbb\xbfcomponent",
                "LEX001",
                "source must be UTF-8 without a byte-order mark",
                Some((0, 1, 1, 3, 1, 2)),
            ),
            (
                b"\xff",
                "SOURCE002",
                "could not read retained source: source is not valid UTF-8 at byte 0",
                None,
            ),
            (
                b"'ab'",
                "LEX002",
                "character literal must contain exactly one Unicode scalar",
                Some((0, 1, 1, 4, 1, 5)),
            ),
            (
                b"\"\\x80\"",
                "LEX002",
                "`\\x` escape must be at most 0x7F",
                Some((0, 1, 1, 5, 1, 6)),
            ),
            (
                b"\"\\u{}\"",
                "LEX002",
                "Unicode escape requires one through six hexadecimal digits",
                Some((0, 1, 1, 5, 1, 6)),
            ),
            (
                b"\"line\n\"",
                "LEX002",
                "unescaped control character in quoted literal",
                Some((5, 1, 6, 6, 2, 1)),
            ),
            (
                b"?",
                "LEX001",
                "character '?' is not an Arche 0.1 token",
                Some((0, 1, 1, 1, 1, 2)),
            ),
        ];

        let mut golden = b"ARCHE-C1-ACTIVE-LEXER-NEGATIVE-MATRIX\0".to_vec();
        for (source, code, message, span) in cases {
            let error = lex(source).unwrap_err();
            assert_eq!(error.code, *code, "{}", String::from_utf8_lossy(source));
            assert_eq!(
                error.message,
                *message,
                "{}",
                String::from_utf8_lossy(source)
            );
            assert_eq!(error.primary.message, error.message);
            assert_eq!(
                error.primary.span,
                span.map(
                    |(start_byte, start_line, start_column, end_byte, end_line, end_column)| Span {
                        file: FileId(7),
                        start: SourcePosition {
                            byte: start_byte,
                            line: start_line,
                            column: start_column,
                        },
                        end: SourcePosition {
                            byte: end_byte,
                            line: end_line,
                            column: end_column,
                        },
                    }
                ),
                "{}",
                String::from_utf8_lossy(source)
            );
            assert!(error.secondary.is_empty());
            assert!(error.notes.is_empty());
            push_golden_diagnostic(&mut golden, source, &error);
        }

        let digest: [u8; 32] = Sha256::digest(&golden).into();
        assert_eq!(
            digest,
            [
                0xd0, 0xc8, 0x07, 0x12, 0x17, 0xde, 0x6e, 0xed, 0x49, 0x42, 0x66, 0xf3, 0x6a, 0x08,
                0x7c, 0x5b, 0x49, 0xfe, 0x01, 0xf7, 0xea, 0x9d, 0xc3, 0xe3, 0x1f, 0x85, 0xf0, 0x07,
                0x0b, 0xe8, 0x18, 0x61,
            ]
        );
    }

    #[test]
    fn complete_token_stream_is_byte_exact_and_covers_the_closed_lexer_surface() {
        let mut source = String::from(
            "/// complete token payload Caf\u{e9}\r\n/* outer /* nested */ done */\r\n",
        );
        source.push_str(
            &ALL_KEYWORDS
                .iter()
                .map(|(_, spelling)| *spelling)
                .collect::<Vec<_>>()
                .join(" "),
        );
        source.push_str("\r\n");
        source.push_str(
            &ALL_PUNCTUATION
                .iter()
                .map(|punctuation| punctuation.spelling())
                .collect::<Vec<_>>()
                .join(" "),
        );
        source.push_str("\r\nalpha Caf\u{e9} \u{039a}\u{03b1}\u{03c6}\u{03ad}\u{03c2} 'life _\r\n");
        source.push_str(concat!(
            "0b10 0o7 42 0xCA ",
            "1i8 2i16 3i32 4i64 5isize 6u8 7u16 8u32 9u64 10usize ",
            "1_000u64 0b10_01u8 0o7_0 0xCA_FE ",
            "1.25 2.5f32 3e2f64 0x1.fp2 0x1.fp2f32 0x1.fp2f64 1e-2f64\r\n",
        ));
        source.push_str(r#"'z' '\n' '\x41' '\u{e9}' "\\\"\'\n\r\t\0\x41\u{1f642}""#);
        source.push_str("\r\n");

        let tokens = lex(source.as_bytes()).unwrap();
        let keywords = tokens
            .iter()
            .filter_map(|token| match token.kind {
                TokenKind::Keyword(keyword) => Some(keyword),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            keywords,
            ALL_KEYWORDS
                .iter()
                .map(|(keyword, _)| *keyword)
                .collect::<Vec<_>>()
        );
        for (keyword, spelling) in ALL_KEYWORDS {
            assert_eq!(super::keyword(spelling), Some(*keyword));
        }
        let mut keyword_tags = ALL_KEYWORDS
            .iter()
            .map(|(keyword, _)| keyword_golden_tag(*keyword))
            .collect::<Vec<_>>();
        keyword_tags.sort_unstable();
        keyword_tags.dedup();
        assert_eq!(keyword_tags, (1_u8..=71).collect::<Vec<_>>());

        let punctuation = tokens
            .iter()
            .filter_map(|token| match token.kind {
                TokenKind::Punctuation(punctuation) => Some(punctuation),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(punctuation, ALL_PUNCTUATION);
        let mut punctuation_tags = ALL_PUNCTUATION
            .iter()
            .map(|punctuation| punctuation_golden_tag(*punctuation))
            .collect::<Vec<_>>();
        punctuation_tags.sort_unstable();
        punctuation_tags.dedup();
        assert_eq!(punctuation_tags, (1_u8..=38).collect::<Vec<_>>());

        let mut integer_bases = tokens
            .iter()
            .filter_map(|token| match &token.kind {
                TokenKind::Integer(integer) => Some(numeric_base_golden_tag(integer.base)),
                _ => None,
            })
            .collect::<Vec<_>>();
        integer_bases.sort_unstable();
        integer_bases.dedup();
        assert_eq!(integer_bases, [1, 2, 3, 4]);
        let mut integer_suffixes = tokens
            .iter()
            .filter_map(|token| match &token.kind {
                TokenKind::Integer(integer) => integer.suffix.map(integer_suffix_golden_tag),
                _ => None,
            })
            .collect::<Vec<_>>();
        integer_suffixes.sort_unstable();
        integer_suffixes.dedup();
        assert_eq!(integer_suffixes, (1_u8..=10).collect::<Vec<_>>());

        let mut float_bases = tokens
            .iter()
            .filter_map(|token| match &token.kind {
                TokenKind::Float(float) => Some(numeric_base_golden_tag(float.base)),
                _ => None,
            })
            .collect::<Vec<_>>();
        float_bases.sort_unstable();
        float_bases.dedup();
        assert_eq!(float_bases, [3, 4]);
        let mut float_suffixes = tokens
            .iter()
            .filter_map(|token| match &token.kind {
                TokenKind::Float(float) => float.suffix.map(float_suffix_golden_tag),
                _ => None,
            })
            .collect::<Vec<_>>();
        float_suffixes.sort_unstable();
        float_suffixes.dedup();
        assert_eq!(float_suffixes, [1, 2]);

        assert!(tokens
            .iter()
            .any(|token| matches!(token.kind, TokenKind::Identifier(_))));
        assert!(tokens
            .iter()
            .any(|token| matches!(token.kind, TokenKind::Lifetime(_))));
        assert!(tokens
            .iter()
            .any(|token| matches!(token.kind, TokenKind::Wildcard)));
        assert!(tokens
            .iter()
            .any(|token| matches!(token.kind, TokenKind::DocComment(_))));
        assert!(tokens
            .iter()
            .any(|token| matches!(token.kind, TokenKind::Character(_))));
        assert!(tokens
            .iter()
            .any(|token| matches!(token.kind, TokenKind::String(_))));
        assert_eq!(
            tokens
                .iter()
                .filter(|token| matches!(token.kind, TokenKind::Eof))
                .count(),
            1
        );

        let digest = Sha256::digest(encode_complete_token_stream(&tokens));
        assert_eq!(
            lowercase_hex(&digest),
            "234bcbbd327fbb2a27e34df896d1e0752a2c4f1235fc43aead43a37a6ae7e5ad"
        );
    }

    #[test]
    fn lexes_the_frozen_c1_positive_fixture_to_one_exact_eof() {
        let source = include_bytes!("../../../../../tests/m27c1/vectors/lexical-positive.arc");
        let tokens = lex(source).unwrap();
        assert_eq!(
            tokens
                .iter()
                .filter(|token| token.kind == TokenKind::Eof)
                .count(),
            1
        );
        assert!(matches!(tokens[0].kind, TokenKind::DocComment(_)));
        assert!(tokens
            .iter()
            .any(|token| matches!(token.kind, TokenKind::Float(ref value) if value.base == NumericBase::Hexadecimal)));
        assert!(tokens
            .iter()
            .any(|token| { token.kind == TokenKind::Punctuation(Punctuation::RangeInclusive) }));
        assert_eq!(
            tokens.last().unwrap().span.start,
            tokens.last().unwrap().span.end
        );
    }
}
