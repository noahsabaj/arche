use std::fmt;
use std::io::{self, BufRead};

use crate::identifier::{Identifier, IdentifierInterner};
use crate::source_snapshot::SourcePosition;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenKind {
    Keyword(Keyword),
    Identifier(Identifier),
    Integer(String),
    Float(String),
    LeftBrace,
    RightBrace,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Colon,
    Comma,
    Dot,
    Equal,
    Bang,
    Ampersand,
    Pipe,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Tilde,
    Less,
    Greater,
    Caret,
    Eof,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Keyword {
    World,
    Component,
    Resource,
    Tag,
    Event,
    Relation,
    System,
    Schedule,
    Startup,
    Run,
    Flush,
    Spawn,
    Despawn,
    Insert,
    Exit,
    Query,
    Read,
    Mut,
    Entity,
    For,
    In,
    If,
    Else,
    While,
    Let,
    True,
    False,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LexError {
    pub span: SourceSpan,
    pub character: char,
}

#[derive(Debug)]
pub enum LexerFailure {
    Read(io::Error),
    Lex(LexError),
}

pub struct Lexer<R: BufRead> {
    reader: R,
    position: SourcePosition,
    pending: Option<Token>,
    identifiers: IdentifierInterner,
}

impl<R: BufRead> Lexer<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            position: SourcePosition {
                byte: 0,
                line: 1,
                column: 1,
            },
            pending: None,
            identifiers: IdentifierInterner::default(),
        }
    }

    #[cfg(test)]
    fn with_position(reader: R, position: SourcePosition) -> Self {
        Self {
            reader,
            position,
            pending: None,
            identifiers: IdentifierInterner::default(),
        }
    }

    pub fn next_token(&mut self) -> Result<Token, LexerFailure> {
        if let Some(token) = self.pending.take() {
            return Ok(token);
        }

        loop {
            let Some(byte) = self.peek_byte()? else {
                return Ok(Token {
                    kind: TokenKind::Eof,
                    span: SourceSpan {
                        start: self.position,
                        end: self.position,
                    },
                });
            };

            if byte.is_ascii_whitespace() {
                self.consume_ascii_whitespace_prefix()?;
                continue;
            }

            if !byte.is_ascii() {
                let (character, span) = self.consume_utf8_character()?;
                if character.is_whitespace() {
                    continue;
                }
                return Err(LexerFailure::Lex(LexError { span, character }));
            }

            if byte.is_ascii_alphabetic() || byte == b'_' {
                return self.identifier_token();
            }
            if byte.is_ascii_digit() {
                return self.number_token();
            }

            let start = self.position;
            self.consume_ascii_byte(byte)?;
            let span = SourceSpan {
                start,
                end: self.position,
            };
            return punctuation(byte).map_or_else(
                || {
                    Err(LexerFailure::Lex(LexError {
                        span,
                        character: char::from(byte),
                    }))
                },
                |kind| Ok(Token { kind, span }),
            );
        }
    }

    fn identifier_token(&mut self) -> Result<Token, LexerFailure> {
        let start = self.position;
        let mut bytes = Vec::new();
        while let Some(byte) = self.peek_byte()? {
            if !byte.is_ascii_alphanumeric() && byte != b'_' {
                break;
            }
            self.consume_ascii_byte(byte)?;
            push_token_byte(&mut bytes, byte)?;
        }

        let text = String::from_utf8(bytes).expect("identifier bytes are ASCII");
        let kind = match Keyword::from_text(&text) {
            Some(keyword) => TokenKind::Keyword(keyword),
            None => TokenKind::Identifier(self.identifiers.intern(text)?),
        };
        Ok(Token {
            kind,
            span: SourceSpan {
                start,
                end: self.position,
            },
        })
    }

    fn number_token(&mut self) -> Result<Token, LexerFailure> {
        let start = self.position;
        let mut bytes = Vec::new();
        while let Some(byte) = self.peek_byte()? {
            if !byte.is_ascii_digit() {
                break;
            }
            self.consume_ascii_byte(byte)?;
            push_token_byte(&mut bytes, byte)?;
        }

        let mut is_float = false;
        if self.peek_byte()? == Some(b'.') {
            let dot_start = self.position;
            self.consume_ascii_byte(b'.')?;
            if self.peek_byte()?.is_some_and(|byte| byte.is_ascii_digit()) {
                is_float = true;
                push_token_byte(&mut bytes, b'.')?;
                while let Some(byte) = self.peek_byte()? {
                    if !byte.is_ascii_digit() {
                        break;
                    }
                    self.consume_ascii_byte(byte)?;
                    push_token_byte(&mut bytes, byte)?;
                }
            } else {
                self.pending = Some(Token {
                    kind: TokenKind::Dot,
                    span: SourceSpan {
                        start: dot_start,
                        end: self.position,
                    },
                });
                let text = String::from_utf8(bytes).expect("integer bytes are ASCII");
                return Ok(Token {
                    kind: TokenKind::Integer(text),
                    span: SourceSpan {
                        start,
                        end: dot_start,
                    },
                });
            }
        }

        let text = String::from_utf8(bytes).expect("number bytes are ASCII");
        Ok(Token {
            kind: if is_float {
                TokenKind::Float(text)
            } else {
                TokenKind::Integer(text)
            },
            span: SourceSpan {
                start,
                end: self.position,
            },
        })
    }

    fn peek_byte(&mut self) -> Result<Option<u8>, LexerFailure> {
        self.reader
            .fill_buf()
            .map(|bytes| bytes.first().copied())
            .map_err(LexerFailure::Read)
    }

    fn consume_ascii_byte(&mut self, byte: u8) -> Result<(), LexerFailure> {
        self.reader.consume(1);
        advance_ascii(&mut self.position, byte)?;
        Ok(())
    }

    fn consume_raw_byte(&mut self) -> Result<u8, LexerFailure> {
        let byte = self.peek_byte()?.ok_or_else(|| {
            LexerFailure::Read(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "source ended inside a UTF-8 character",
            ))
        })?;
        self.reader.consume(1);
        advance_byte(&mut self.position)?;
        Ok(byte)
    }

    fn consume_ascii_whitespace_prefix(&mut self) -> Result<(), LexerFailure> {
        let consumed = {
            let reader = &mut self.reader;
            let position = &mut self.position;
            let bytes = reader.fill_buf().map_err(LexerFailure::Read)?;
            consume_ascii_whitespace(position, bytes)?
        };
        self.reader.consume(consumed);
        Ok(())
    }

    fn consume_utf8_character(&mut self) -> Result<(char, SourceSpan), LexerFailure> {
        let start = self.position;
        let first = self.peek_byte()?.expect("caller observed a source byte");
        let expected =
            utf8_width(first).ok_or_else(|| LexerFailure::Read(invalid_utf8(start.byte)))?;
        let mut bytes = [0u8; 4];
        bytes[0] = self.consume_raw_byte()?;
        for byte in &mut bytes[1..expected] {
            *byte = self.consume_raw_byte()?;
            if *byte & 0xc0 != 0x80 {
                return Err(LexerFailure::Read(invalid_utf8(start.byte)));
            }
        }
        let character = std::str::from_utf8(&bytes[..expected])
            .map_err(|_| LexerFailure::Read(invalid_utf8(start.byte)))?
            .chars()
            .next()
            .ok_or_else(|| LexerFailure::Read(invalid_utf8(start.byte)))?;
        advance_unicode_column(&mut self.position)?;
        Ok((
            character,
            SourceSpan {
                start,
                end: self.position,
            },
        ))
    }
}

impl From<io::Error> for LexerFailure {
    fn from(error: io::Error) -> Self {
        Self::Read(error)
    }
}

#[cfg(test)]
pub fn lex(source: &str) -> Result<Vec<Token>, LexError> {
    lex_reader(std::io::Cursor::new(source.as_bytes())).expect("in-memory source reads cannot fail")
}

#[cfg(test)]
pub fn lex_reader<R: BufRead>(reader: R) -> io::Result<Result<Vec<Token>, LexError>> {
    collect_tokens(Lexer::new(reader))
}

#[cfg(test)]
fn lex_reader_at<R: BufRead>(
    reader: R,
    initial_position: SourcePosition,
) -> io::Result<Result<Vec<Token>, LexError>> {
    collect_tokens(Lexer::with_position(reader, initial_position))
}

#[cfg(test)]
fn collect_tokens<R: BufRead>(mut lexer: Lexer<R>) -> io::Result<Result<Vec<Token>, LexError>> {
    let mut tokens = Vec::new();
    loop {
        match lexer.next_token() {
            Ok(token) => {
                let eof = token.kind == TokenKind::Eof;
                push_token(&mut tokens, token)?;
                if eof {
                    return Ok(Ok(tokens));
                }
            }
            Err(LexerFailure::Lex(error)) => return Ok(Err(error)),
            Err(LexerFailure::Read(error)) => return Err(error),
        }
    }
}

#[cfg(test)]
fn legacy_lex_reader_at<R: BufRead>(
    mut reader: R,
    initial_position: SourcePosition,
) -> io::Result<Result<Vec<Token>, LexError>> {
    const READ_BUFFER_BYTES: usize = 1024 * 1024;

    let mut input = Vec::new();
    input
        .try_reserve_exact(READ_BUFFER_BYTES)
        .map_err(|error| allocation_error("lexer input buffer", error))?;
    input.resize(READ_BUFFER_BYTES, 0);

    let mut tokens = Vec::new();
    let mut identifiers = IdentifierInterner::default();
    let mut state = StreamingState::Idle;
    let mut position = initial_position;

    loop {
        let byte_count = reader.read(&mut input)?;
        if byte_count == 0 {
            break;
        }
        let bytes = &input[..byte_count];
        let mut index = 0usize;
        while index < bytes.len() {
            match &mut state {
                StreamingState::Idle => {
                    let byte = bytes[index];
                    if byte.is_ascii_whitespace() {
                        let consumed = consume_ascii_whitespace(&mut position, &bytes[index..])?;
                        index += consumed;
                    } else if byte.is_ascii_alphabetic() || byte == b'_' {
                        let mut token = Vec::new();
                        push_token_byte(&mut token, byte)?;
                        state = StreamingState::Identifier {
                            start: position,
                            bytes: token,
                        };
                        advance_ascii(&mut position, byte)?;
                        index += 1;
                    } else if byte.is_ascii_digit() {
                        let mut token = Vec::new();
                        push_token_byte(&mut token, byte)?;
                        state = StreamingState::Integer {
                            start: position,
                            bytes: token,
                        };
                        advance_ascii(&mut position, byte)?;
                        index += 1;
                    } else if byte.is_ascii() {
                        let start = position;
                        advance_ascii(&mut position, byte)?;
                        index += 1;
                        let Some(kind) = punctuation(byte) else {
                            return Ok(Err(LexError {
                                span: SourceSpan {
                                    start,
                                    end: position,
                                },
                                character: char::from(byte),
                            }));
                        };
                        push_token(
                            &mut tokens,
                            Token {
                                kind,
                                span: SourceSpan {
                                    start,
                                    end: position,
                                },
                            },
                        )?;
                    } else {
                        let expected =
                            utf8_width(byte).ok_or_else(|| invalid_utf8(position.byte))?;
                        let mut character_bytes = [0; 4];
                        character_bytes[0] = byte;
                        state = StreamingState::Utf8 {
                            start: position,
                            bytes: character_bytes,
                            len: 1,
                            expected,
                        };
                        advance_byte(&mut position)?;
                        index += 1;
                    }
                }
                StreamingState::Identifier { start, bytes: text } => {
                    let byte = bytes[index];
                    if byte.is_ascii_alphanumeric() || byte == b'_' {
                        push_token_byte(text, byte)?;
                        advance_ascii(&mut position, byte)?;
                        index += 1;
                    } else {
                        let start = *start;
                        let text = take_streaming_bytes(&mut state);
                        finish_identifier(&mut tokens, &mut identifiers, start, position, text)?;
                    }
                }
                StreamingState::Integer { start, bytes: text } => {
                    let byte = bytes[index];
                    if byte.is_ascii_digit() {
                        push_token_byte(text, byte)?;
                        advance_ascii(&mut position, byte)?;
                        index += 1;
                    } else if byte == b'.' {
                        let start = *start;
                        let text = take_streaming_bytes(&mut state);
                        state = StreamingState::PendingDecimalPoint {
                            start,
                            bytes: text,
                            dot_start: position,
                        };
                        advance_ascii(&mut position, byte)?;
                        index += 1;
                    } else {
                        let start = *start;
                        let text = take_streaming_bytes(&mut state);
                        finish_number(&mut tokens, start, position, text, false)?;
                    }
                }
                StreamingState::PendingDecimalPoint {
                    start,
                    bytes: text,
                    dot_start,
                } => {
                    let byte = bytes[index];
                    if byte.is_ascii_digit() {
                        push_token_byte(text, b'.')?;
                        push_token_byte(text, byte)?;
                        let start = *start;
                        let text = take_streaming_bytes(&mut state);
                        state = StreamingState::Fraction { start, bytes: text };
                        advance_ascii(&mut position, byte)?;
                        index += 1;
                    } else {
                        let start = *start;
                        let dot_start = *dot_start;
                        let text = take_streaming_bytes(&mut state);
                        finish_number(&mut tokens, start, dot_start, text, false)?;
                        push_token(
                            &mut tokens,
                            Token {
                                kind: TokenKind::Dot,
                                span: SourceSpan {
                                    start: dot_start,
                                    end: position,
                                },
                            },
                        )?;
                    }
                }
                StreamingState::Fraction { start, bytes: text } => {
                    let byte = bytes[index];
                    if byte.is_ascii_digit() {
                        push_token_byte(text, byte)?;
                        advance_ascii(&mut position, byte)?;
                        index += 1;
                    } else {
                        let start = *start;
                        let text = take_streaming_bytes(&mut state);
                        finish_number(&mut tokens, start, position, text, true)?;
                    }
                }
                StreamingState::Utf8 {
                    start,
                    bytes: character_bytes,
                    len,
                    expected,
                } => {
                    let byte = bytes[index];
                    if byte & 0xc0 != 0x80 {
                        return Err(invalid_utf8(start.byte));
                    }
                    character_bytes[*len] = byte;
                    *len += 1;
                    advance_byte(&mut position)?;
                    index += 1;
                    if *len == *expected {
                        let character = std::str::from_utf8(&character_bytes[..*len])
                            .map_err(|_| invalid_utf8(start.byte))?
                            .chars()
                            .next()
                            .ok_or_else(|| invalid_utf8(start.byte))?;
                        let start = *start;
                        state = StreamingState::Idle;
                        advance_unicode_column(&mut position)?;
                        if !character.is_whitespace() {
                            return Ok(Err(LexError {
                                span: SourceSpan {
                                    start,
                                    end: position,
                                },
                                character,
                            }));
                        }
                    }
                }
            }
        }
    }

    match state {
        StreamingState::Idle => {}
        StreamingState::Identifier { start, bytes } => {
            finish_identifier(&mut tokens, &mut identifiers, start, position, bytes)?;
        }
        StreamingState::Integer { start, bytes } => {
            finish_number(&mut tokens, start, position, bytes, false)?;
        }
        StreamingState::PendingDecimalPoint {
            start,
            bytes,
            dot_start,
        } => {
            finish_number(&mut tokens, start, dot_start, bytes, false)?;
            push_token(
                &mut tokens,
                Token {
                    kind: TokenKind::Dot,
                    span: SourceSpan {
                        start: dot_start,
                        end: position,
                    },
                },
            )?;
        }
        StreamingState::Fraction { start, bytes } => {
            finish_number(&mut tokens, start, position, bytes, true)?;
        }
        StreamingState::Utf8 { start, .. } => return Err(invalid_utf8(start.byte)),
    }

    push_token(
        &mut tokens,
        Token {
            kind: TokenKind::Eof,
            span: SourceSpan {
                start: position,
                end: position,
            },
        },
    )?;
    Ok(Ok(tokens))
}

#[cfg(test)]
enum StreamingState {
    Idle,
    Identifier {
        start: SourcePosition,
        bytes: Vec<u8>,
    },
    Integer {
        start: SourcePosition,
        bytes: Vec<u8>,
    },
    PendingDecimalPoint {
        start: SourcePosition,
        bytes: Vec<u8>,
        dot_start: SourcePosition,
    },
    Fraction {
        start: SourcePosition,
        bytes: Vec<u8>,
    },
    Utf8 {
        start: SourcePosition,
        bytes: [u8; 4],
        len: usize,
        expected: usize,
    },
}

#[cfg(test)]
fn take_streaming_bytes(state: &mut StreamingState) -> Vec<u8> {
    match std::mem::replace(state, StreamingState::Idle) {
        StreamingState::Identifier { bytes, .. }
        | StreamingState::Integer { bytes, .. }
        | StreamingState::PendingDecimalPoint { bytes, .. }
        | StreamingState::Fraction { bytes, .. } => bytes,
        StreamingState::Idle | StreamingState::Utf8 { .. } => {
            unreachable!("only token-building states contain byte storage")
        }
    }
}

#[cfg(test)]
fn finish_identifier(
    tokens: &mut Vec<Token>,
    identifiers: &mut IdentifierInterner,
    start: SourcePosition,
    end: SourcePosition,
    bytes: Vec<u8>,
) -> io::Result<()> {
    let text = String::from_utf8(bytes).expect("identifier bytes are ASCII");
    let kind = match Keyword::from_text(&text) {
        Some(keyword) => TokenKind::Keyword(keyword),
        None => TokenKind::Identifier(identifiers.intern(text)?),
    };
    push_token(
        tokens,
        Token {
            kind,
            span: SourceSpan { start, end },
        },
    )
}

#[cfg(test)]
fn finish_number(
    tokens: &mut Vec<Token>,
    start: SourcePosition,
    end: SourcePosition,
    bytes: Vec<u8>,
    is_float: bool,
) -> io::Result<()> {
    let text = String::from_utf8(bytes).expect("number bytes are ASCII");
    let kind = if is_float {
        TokenKind::Float(text)
    } else {
        TokenKind::Integer(text)
    };
    push_token(
        tokens,
        Token {
            kind,
            span: SourceSpan { start, end },
        },
    )
}

fn punctuation(byte: u8) -> Option<TokenKind> {
    match byte {
        b'{' => Some(TokenKind::LeftBrace),
        b'}' => Some(TokenKind::RightBrace),
        b'(' => Some(TokenKind::LeftParen),
        b')' => Some(TokenKind::RightParen),
        b'[' => Some(TokenKind::LeftBracket),
        b']' => Some(TokenKind::RightBracket),
        b':' => Some(TokenKind::Colon),
        b',' => Some(TokenKind::Comma),
        b'.' => Some(TokenKind::Dot),
        b'=' => Some(TokenKind::Equal),
        b'!' => Some(TokenKind::Bang),
        b'&' => Some(TokenKind::Ampersand),
        b'|' => Some(TokenKind::Pipe),
        b'+' => Some(TokenKind::Plus),
        b'-' => Some(TokenKind::Minus),
        b'*' => Some(TokenKind::Star),
        b'/' => Some(TokenKind::Slash),
        b'%' => Some(TokenKind::Percent),
        b'~' => Some(TokenKind::Tilde),
        b'<' => Some(TokenKind::Less),
        b'>' => Some(TokenKind::Greater),
        b'^' => Some(TokenKind::Caret),
        _ => None,
    }
}

fn utf8_width(first: u8) -> Option<usize> {
    match first {
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

fn push_token_byte(bytes: &mut Vec<u8>, byte: u8) -> io::Result<()> {
    if bytes.len() == bytes.capacity() {
        bytes
            .try_reserve(1)
            .map_err(|error| allocation_error("token text", error))?;
    }
    bytes.push(byte);
    Ok(())
}

#[cfg(test)]
fn push_token(tokens: &mut Vec<Token>, token: Token) -> io::Result<()> {
    if tokens.len() == tokens.capacity() {
        tokens
            .try_reserve(1)
            .map_err(|error| allocation_error("token list", error))?;
    }
    tokens.push(token);
    Ok(())
}

fn consume_ascii_whitespace(position: &mut SourcePosition, bytes: &[u8]) -> io::Result<usize> {
    let mut consumed = 0usize;
    let mut line_feed_count = 0u64;
    let mut non_cr_since_line_feed = 0u64;
    let mut non_cr_total = 0u64;
    for &byte in bytes {
        if !byte.is_ascii_whitespace() {
            break;
        }
        consumed += 1;
        if byte == b'\n' {
            line_feed_count = line_feed_count.checked_add(1).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "source line number overflows u64",
                )
            })?;
            non_cr_since_line_feed = 0;
        } else if byte != b'\r' {
            non_cr_total = non_cr_total.checked_add(1).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "source column width overflows u64",
                )
            })?;
            non_cr_since_line_feed = non_cr_since_line_feed.checked_add(1).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "source column width overflows u64",
                )
            })?;
        }
    }

    let byte_count = u64::try_from(consumed).map_err(|_| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            "lexer buffer length exceeds u64",
        )
    })?;
    position.byte = position.byte.checked_add(byte_count).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "source byte offset overflows u64",
        )
    })?;

    if line_feed_count == 0 {
        position.column = position.column.checked_add(non_cr_total).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "source column number overflows u64",
            )
        })?;
    } else {
        position.line = position.line.checked_add(line_feed_count).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "source line number overflows u64",
            )
        })?;
        position.column = 1u64.checked_add(non_cr_since_line_feed).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "source column number overflows u64",
            )
        })?;
    }
    Ok(consumed)
}

fn advance_ascii(position: &mut SourcePosition, byte: u8) -> io::Result<()> {
    advance_byte(position)?;
    match byte {
        b'\n' => {
            position.line = position.line.checked_add(1).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "source line number overflows u64",
                )
            })?;
            position.column = 1;
        }
        b'\r' => {}
        _ => advance_unicode_column(position)?,
    }
    Ok(())
}

fn advance_byte(position: &mut SourcePosition) -> io::Result<()> {
    position.byte = position.byte.checked_add(1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "source byte offset overflows u64",
        )
    })?;
    Ok(())
}

fn advance_unicode_column(position: &mut SourcePosition) -> io::Result<()> {
    position.column = position.column.checked_add(1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "source column number overflows u64",
        )
    })?;
    Ok(())
}

fn allocation_error(context: &'static str, error: impl fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::OutOfMemory,
        format!("could not allocate {context}: {error}"),
    )
}

fn invalid_utf8(offset: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("source is not valid UTF-8 at byte {offset}"),
    )
}

impl Keyword {
    fn from_text(text: &str) -> Option<Self> {
        match text {
            "world" => Some(Self::World),
            "component" => Some(Self::Component),
            "resource" => Some(Self::Resource),
            "tag" => Some(Self::Tag),
            "event" => Some(Self::Event),
            "relation" => Some(Self::Relation),
            "system" => Some(Self::System),
            "schedule" => Some(Self::Schedule),
            "startup" => Some(Self::Startup),
            "run" => Some(Self::Run),
            "flush" => Some(Self::Flush),
            "spawn" => Some(Self::Spawn),
            "despawn" => Some(Self::Despawn),
            "insert" => Some(Self::Insert),
            "exit" => Some(Self::Exit),
            "query" => Some(Self::Query),
            "read" => Some(Self::Read),
            "mut" => Some(Self::Mut),
            "entity" => Some(Self::Entity),
            "for" => Some(Self::For),
            "in" => Some(Self::In),
            "if" => Some(Self::If),
            "else" => Some(Self::Else),
            "while" => Some(Self::While),
            "let" => Some(Self::Let),
            "true" => Some(Self::True),
            "false" => Some(Self::False),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::World => "world",
            Self::Component => "component",
            Self::Resource => "resource",
            Self::Tag => "tag",
            Self::Event => "event",
            Self::Relation => "relation",
            Self::System => "system",
            Self::Schedule => "schedule",
            Self::Startup => "startup",
            Self::Run => "run",
            Self::Flush => "flush",
            Self::Spawn => "spawn",
            Self::Despawn => "despawn",
            Self::Insert => "insert",
            Self::Exit => "exit",
            Self::Query => "query",
            Self::Read => "read",
            Self::Mut => "mut",
            Self::Entity => "entity",
            Self::For => "for",
            Self::In => "in",
            Self::If => "if",
            Self::Else => "else",
            Self::While => "while",
            Self::Let => "let",
            Self::True => "true",
            Self::False => "false",
        }
    }
}

impl fmt::Display for TokenKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keyword(keyword) => write!(formatter, "Keyword({})", keyword.as_str()),
            Self::Identifier(text) => write!(formatter, "Identifier({text})"),
            Self::Integer(text) => write!(formatter, "Integer({text})"),
            Self::Float(text) => write!(formatter, "Float({text})"),
            Self::LeftBrace => formatter.write_str("LeftBrace"),
            Self::RightBrace => formatter.write_str("RightBrace"),
            Self::LeftParen => formatter.write_str("LeftParen"),
            Self::RightParen => formatter.write_str("RightParen"),
            Self::LeftBracket => formatter.write_str("LeftBracket"),
            Self::RightBracket => formatter.write_str("RightBracket"),
            Self::Colon => formatter.write_str("Colon"),
            Self::Comma => formatter.write_str("Comma"),
            Self::Dot => formatter.write_str("Dot"),
            Self::Equal => formatter.write_str("Equal"),
            Self::Bang => formatter.write_str("Bang"),
            Self::Ampersand => formatter.write_str("Ampersand"),
            Self::Pipe => formatter.write_str("Pipe"),
            Self::Plus => formatter.write_str("Plus"),
            Self::Minus => formatter.write_str("Minus"),
            Self::Star => formatter.write_str("Star"),
            Self::Slash => formatter.write_str("Slash"),
            Self::Percent => formatter.write_str("Percent"),
            Self::Tilde => formatter.write_str("Tilde"),
            Self::Less => formatter.write_str("Less"),
            Self::Greater => formatter.write_str("Greater"),
            Self::Caret => formatter.write_str("Caret"),
            Self::Eof => formatter.write_str("Eof"),
        }
    }
}

pub fn write_token(output: &mut impl io::Write, token: &Token) -> io::Result<()> {
    output.write_fmt(format_args!("{}\n", token.kind))
}

impl fmt::Display for LexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unexpected character '{}' at byte {}",
            self.character, self.span.start.byte
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, BufRead, Cursor, Read};

    struct FailingWriter;

    impl io::Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "injected token-output failure",
            ))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn token_formatter_propagates_output_failures() {
        let token = lex("world").expect("fixture lexes").remove(0);
        let error = write_token(&mut FailingWriter, &token).expect_err("writer must fail");

        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn lexes_complete_m26_operator_surface() {
        let tokens = lex("/ % ~ < > ^").expect("M26 operators lex");
        let kinds = tokens
            .iter()
            .map(|token| token.kind.to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            ["Slash", "Percent", "Tilde", "Less", "Greater", "Caret", "Eof"]
        );
    }

    #[test]
    fn incremental_lexer_preserves_tokens_after_u32_boundary() {
        let initial_offset = u64::from(u32::MAX) - 4;
        let padding = 17;
        let reader =
            VirtualWhitespaceReader::new(padding, b"world Late startup { exit 47 }".to_vec());

        let tokens = lex_reader_at(
            reader,
            SourcePosition {
                byte: initial_offset,
                line: 1,
                column: 1,
            },
        )
        .expect("virtual source reads")
        .expect("virtual source lexes");
        let reference = legacy_lex_reader_at(
            VirtualWhitespaceReader::new(padding, b"world Late startup { exit 47 }".to_vec()),
            SourcePosition {
                byte: initial_offset,
                line: 1,
                column: 1,
            },
        )
        .expect("reference source reads")
        .expect("reference source lexes");

        assert_eq!(tokens[0].kind, TokenKind::Keyword(Keyword::World));
        assert_eq!(tokens, reference);
        assert_eq!(
            tokens[0].span.start.byte,
            initial_offset + padding,
            "the first real token remains beyond u32::MAX"
        );
        assert_eq!(tokens[0].span.start.line, 1);
        assert_eq!(tokens[0].span.start.column, padding + 1);
        assert_eq!(tokens.last().unwrap().kind, TokenKind::Eof);
        assert!(tokens.last().unwrap().span.start.byte > u64::from(u32::MAX));
    }

    #[test]
    fn repeated_identifier_storage_is_shared_across_u32_boundary() {
        let initial_offset = u64::from(u32::MAX) - 3;
        let tokens = lex_reader_at(
            TinyChunkReader::new(b"Same Same", 1),
            SourcePosition {
                byte: initial_offset,
                line: 1,
                column: 1,
            },
        )
        .expect("tiny streamed source reads")
        .expect("tiny streamed source lexes");

        let TokenKind::Identifier(first) = &tokens[0].kind else {
            panic!("first token is an identifier");
        };
        let TokenKind::Identifier(second) = &tokens[1].kind else {
            panic!("second token is an identifier");
        };
        assert!(first.shares_storage_with(second));
        assert!(tokens[0].span.start.byte < u64::from(u32::MAX));
        assert!(tokens[1].span.start.byte > u64::from(u32::MAX));
        assert_eq!(tokens[1].span.start.line, 1);
        assert_eq!(tokens[1].span.start.column, 6);
    }

    #[test]
    fn captures_unicode_crlf_and_chunk_boundary_positions() {
        let source = "world Main\r\n\u{2003}startup {\nexit 0\r\n}";
        let reader = TinyChunkReader::new(source.as_bytes(), 2);
        let tokens = lex_reader(reader)
            .expect("chunked source reads")
            .expect("chunked UTF-8 source lexes");

        let startup = tokens
            .iter()
            .find(|token| token.kind == TokenKind::Keyword(Keyword::Startup))
            .expect("startup token");
        assert_eq!(
            startup.span,
            SourceSpan {
                start: SourcePosition {
                    byte: 15,
                    line: 2,
                    column: 2,
                },
                end: SourcePosition {
                    byte: 22,
                    line: 2,
                    column: 9,
                },
            }
        );

        let close = tokens
            .iter()
            .find(|token| token.kind == TokenKind::RightBrace)
            .expect("closing brace");
        assert_eq!(close.span.start.line, 4);
        assert_eq!(close.span.start.column, 1);
        assert_eq!(close.span.end.line, 4);
        assert_eq!(close.span.end.column, 2);

        let eof = tokens.last().expect("EOF token");
        assert_eq!(eof.kind, TokenKind::Eof);
        assert_eq!(eof.span.start, eof.span.end);
        assert_eq!(eof.span.start.line, 4);
        assert_eq!(eof.span.start.column, 2);
    }

    #[test]
    fn lexical_errors_retain_the_unexpected_character_span() {
        let error = lex("world Main\n  @").expect_err("unexpected punctuation must fail");

        assert_eq!(error.character, '@');
        assert_eq!(
            error.span,
            SourceSpan {
                start: SourcePosition {
                    byte: 13,
                    line: 2,
                    column: 3,
                },
                end: SourcePosition {
                    byte: 14,
                    line: 2,
                    column: 4,
                },
            }
        );

        let error =
            lex("world Main\n \u{1f642}").expect_err("unexpected multibyte character must fail");
        assert_eq!(error.character, '\u{1f642}');
        assert_eq!(
            error.span,
            SourceSpan {
                start: SourcePosition {
                    byte: 12,
                    line: 2,
                    column: 2,
                },
                end: SourcePosition {
                    byte: 16,
                    line: 2,
                    column: 3,
                },
            }
        );
    }

    #[test]
    fn checked_positions_reject_u64_overflow() {
        for (initial, source, expected) in [
            (
                SourcePosition {
                    byte: u64::MAX,
                    line: 1,
                    column: 1,
                },
                "x",
                "source byte offset overflows u64",
            ),
            (
                SourcePosition {
                    byte: 0,
                    line: u64::MAX,
                    column: 1,
                },
                "\n",
                "source line number overflows u64",
            ),
            (
                SourcePosition {
                    byte: 0,
                    line: 1,
                    column: u64::MAX,
                },
                " ",
                "source column number overflows u64",
            ),
        ] {
            let error = lex_reader_at(std::io::Cursor::new(source.as_bytes()), initial)
                .expect_err("source position overflow must be explicit");
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(error.to_string().contains(expected));
        }
    }

    #[test]
    fn lexer_yields_tokens_incrementally_from_tiny_chunks() {
        let reader = TinyChunkReader::new(b"world Main startup { exit 0 }", 1);
        let mut lexer = Lexer::new(reader);

        let world = lexer.next_token().expect("first token");
        assert_eq!(world.kind, TokenKind::Keyword(Keyword::World));
        assert_eq!((world.span.start.byte, world.span.end.byte), (0, 5));

        let name = lexer.next_token().expect("second token");
        assert_eq!(name.kind, TokenKind::Identifier("Main".into()));
        assert_eq!((name.span.start.byte, name.span.end.byte), (6, 10));
    }

    #[test]
    fn lexer_surfaces_reader_failure_only_when_the_stream_reaches_it() {
        let mut lexer = Lexer::new(FailingAfterPrefixReader::new(b"world "));

        let world = lexer.next_token().expect("prefix token is available");
        assert_eq!(world.kind, TokenKind::Keyword(Keyword::World));

        let error = lexer
            .next_token()
            .expect_err("reader failure follows the prefix token");
        let LexerFailure::Read(error) = error else {
            panic!("expected reader failure");
        };
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(error.to_string(), "injected source read failure");
    }

    #[test]
    fn boolean_operator_punctuation_is_not_lost_or_split_into_identifiers() {
        let tokens = lex("true != false && !false || true == true").expect("fixture lexes");
        let kinds = tokens
            .into_iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>();

        assert_eq!(
            kinds,
            vec![
                TokenKind::Keyword(Keyword::True),
                TokenKind::Bang,
                TokenKind::Equal,
                TokenKind::Keyword(Keyword::False),
                TokenKind::Ampersand,
                TokenKind::Ampersand,
                TokenKind::Bang,
                TokenKind::Keyword(Keyword::False),
                TokenKind::Pipe,
                TokenKind::Pipe,
                TokenKind::Keyword(Keyword::True),
                TokenKind::Equal,
                TokenKind::Equal,
                TokenKind::Keyword(Keyword::True),
                TokenKind::Eof,
            ]
        );
    }

    struct TinyChunkReader<'a> {
        remaining: &'a [u8],
        chunk_size: usize,
    }

    struct FailingAfterPrefixReader<'a> {
        prefix: &'a [u8],
    }

    impl<'a> FailingAfterPrefixReader<'a> {
        fn new(prefix: &'a [u8]) -> Self {
            Self { prefix }
        }
    }

    impl Read for FailingAfterPrefixReader<'_> {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let available = self.fill_buf()?;
            let copied = available.len().min(output.len());
            output[..copied].copy_from_slice(&available[..copied]);
            self.consume(copied);
            Ok(copied)
        }
    }

    impl BufRead for FailingAfterPrefixReader<'_> {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            if self.prefix.is_empty() {
                Err(io::Error::other("injected source read failure"))
            } else {
                Ok(self.prefix)
            }
        }

        fn consume(&mut self, amount: usize) {
            self.prefix = &self.prefix[amount..];
        }
    }

    impl<'a> TinyChunkReader<'a> {
        fn new(remaining: &'a [u8], chunk_size: usize) -> Self {
            Self {
                remaining,
                chunk_size,
            }
        }
    }

    impl Read for TinyChunkReader<'_> {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let copied = self.remaining.len().min(self.chunk_size).min(output.len());
            output[..copied].copy_from_slice(&self.remaining[..copied]);
            self.remaining = &self.remaining[copied..];
            Ok(copied)
        }
    }

    impl BufRead for TinyChunkReader<'_> {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            Ok(&self.remaining[..self.remaining.len().min(self.chunk_size)])
        }

        fn consume(&mut self, amount: usize) {
            self.remaining = &self.remaining[amount..];
        }
    }

    struct VirtualWhitespaceReader {
        whitespace_remaining: u64,
        whitespace: Box<[u8]>,
        suffix: Cursor<Vec<u8>>,
    }

    impl VirtualWhitespaceReader {
        fn new(whitespace_remaining: u64, suffix: Vec<u8>) -> Self {
            Self {
                whitespace_remaining,
                whitespace: vec![b' '; 1024 * 1024].into_boxed_slice(),
                suffix: Cursor::new(suffix),
            }
        }
    }

    impl Read for VirtualWhitespaceReader {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let available = self.fill_buf()?;
            let copied = available.len().min(output.len());
            output[..copied].copy_from_slice(&available[..copied]);
            self.consume(copied);
            Ok(copied)
        }
    }

    impl BufRead for VirtualWhitespaceReader {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            if self.whitespace_remaining == 0 {
                return self.suffix.fill_buf();
            }
            let available = usize::try_from(
                self.whitespace_remaining
                    .min(u64::try_from(self.whitespace.len()).unwrap()),
            )
            .unwrap();
            Ok(&self.whitespace[..available])
        }

        fn consume(&mut self, amount: usize) {
            if self.whitespace_remaining == 0 {
                self.suffix.consume(amount);
            } else {
                self.whitespace_remaining -= u64::try_from(amount).unwrap();
            }
        }
    }
}
