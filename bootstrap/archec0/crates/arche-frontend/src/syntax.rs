use std::collections::VecDeque;
use std::io::{self, BufRead};

use crate::source::{advance, Diagnostic, FileId, SourcePosition, Span};
use crate::symbol::{Symbol, SymbolInterner};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Keyword {
    Mod,
    Use,
    Pub,
    Package,
    SelfValue,
    Super,
    In,
    As,
    World,
    Component,
    Resource,
    Tag,
    System,
    Schedule,
    Fn,
    Struct,
    Enum,
    Trait,
    Type,
    Const,
    Static,
    Init,
    Startup,
    Exit,
    Reserved,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TokenKind {
    Keyword(Keyword),
    Identifier(Symbol),
    Literal,
    LeftBrace,
    RightBrace,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Semicolon,
    ColonColon,
    Colon,
    Comma,
    Other(char),
    Eof,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Token {
    pub(crate) kind: TokenKind,
    pub(crate) span: Span,
}

#[derive(Debug)]
pub(crate) enum LexFailure {
    Io(io::Error),
    Diagnostic(Box<Diagnostic>),
}

impl From<io::Error> for LexFailure {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
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
    pending: Option<Character>,
}

impl<R: BufRead> CharacterReader<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            position: SourcePosition::START,
            pending: None,
        }
    }

    fn peek(&mut self) -> Result<Option<Character>, io::Error> {
        if self.pending.is_none() {
            self.pending = self.decode()?;
        }
        Ok(self.pending)
    }

    fn next(&mut self) -> Result<Option<Character>, io::Error> {
        if let Some(character) = self.pending.take() {
            return Ok(Some(character));
        }
        self.decode()
    }

    fn decode(&mut self) -> Result<Option<Character>, io::Error> {
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

pub(crate) struct Lexer<R> {
    file: FileId,
    characters: CharacterReader<R>,
    symbols: SymbolInterner,
}

impl<R: BufRead> Lexer<R> {
    pub(crate) fn new(file: FileId, reader: R) -> Self {
        Self {
            file,
            characters: CharacterReader::new(reader),
            symbols: SymbolInterner::default(),
        }
    }

    pub(crate) fn next_token(&mut self) -> Result<Token, LexFailure> {
        loop {
            let Some(character) = self.characters.next()? else {
                let position = self.characters.position;
                return Ok(Token {
                    kind: TokenKind::Eof,
                    span: Span {
                        file: self.file,
                        start: position,
                        end: position,
                    },
                });
            };
            if character.value.is_whitespace() {
                continue;
            }
            if character.value == '/'
                && self
                    .characters
                    .peek()?
                    .is_some_and(|next| next.value == '/')
            {
                self.characters.next()?;
                while let Some(next) = self.characters.next()? {
                    if next.value == '\n' {
                        break;
                    }
                }
                continue;
            }
            return self.token_starting(character);
        }
    }

    fn token_starting(&mut self, first: Character) -> Result<Token, LexFailure> {
        if first.value == '_' || unicode_ident::is_xid_start(first.value) {
            return self.identifier(first);
        }
        if first.value.is_ascii_digit() {
            return self.number(first);
        }
        if first.value == '"' || first.value == '\'' {
            return self.quoted(first);
        }

        let mut end = first.end;
        let kind = match first.value {
            '{' => TokenKind::LeftBrace,
            '}' => TokenKind::RightBrace,
            '(' => TokenKind::LeftParen,
            ')' => TokenKind::RightParen,
            '[' => TokenKind::LeftBracket,
            ']' => TokenKind::RightBracket,
            ';' => TokenKind::Semicolon,
            ',' => TokenKind::Comma,
            ':' => {
                if self
                    .characters
                    .peek()?
                    .is_some_and(|next| next.value == ':')
                {
                    end = self.characters.next()?.expect("peeked character").end;
                    TokenKind::ColonColon
                } else {
                    TokenKind::Colon
                }
            }
            other => TokenKind::Other(other),
        };
        Ok(Token {
            kind,
            span: Span {
                file: self.file,
                start: first.start,
                end,
            },
        })
    }

    fn identifier(&mut self, first: Character) -> Result<Token, LexFailure> {
        let mut text = String::new();
        text.push(first.value);
        let mut end = first.end;
        while let Some(character) = self.characters.peek()? {
            if character.value != '_' && !unicode_ident::is_xid_continue(character.value) {
                break;
            }
            let character = self.characters.next()?.expect("peeked character");
            text.push(character.value);
            end = character.end;
        }
        let symbol = self.symbols.intern_identifier(&text).map_err(|error| {
            LexFailure::Diagnostic(Box::new(Diagnostic::at(
                "LEX001",
                Span {
                    file: self.file,
                    start: first.start,
                    end,
                },
                error.to_string(),
            )))
        })?;
        let kind = keyword(symbol.as_str())
            .map(TokenKind::Keyword)
            .unwrap_or(TokenKind::Identifier(symbol));
        Ok(Token {
            kind,
            span: Span {
                file: self.file,
                start: first.start,
                end,
            },
        })
    }

    fn number(&mut self, first: Character) -> Result<Token, LexFailure> {
        let mut end = first.end;
        while let Some(character) = self.characters.peek()? {
            if !character.value.is_ascii_alphanumeric()
                && character.value != '_'
                && character.value != '.'
            {
                break;
            }
            let character = self.characters.next()?.expect("peeked character");
            end = character.end;
        }
        Ok(Token {
            kind: TokenKind::Literal,
            span: Span {
                file: self.file,
                start: first.start,
                end,
            },
        })
    }

    fn quoted(&mut self, first: Character) -> Result<Token, LexFailure> {
        let delimiter = first.value;
        let mut escaped = false;
        loop {
            let Some(character) = self.characters.next()? else {
                return Err(LexFailure::Diagnostic(Box::new(Diagnostic::at(
                    "LEX002",
                    Span {
                        file: self.file,
                        start: first.start,
                        end: self.characters.position,
                    },
                    "unterminated quoted literal",
                ))));
            };
            if !escaped && character.value == delimiter {
                return Ok(Token {
                    kind: TokenKind::Literal,
                    span: Span {
                        file: self.file,
                        start: first.start,
                        end: character.end,
                    },
                });
            }
            escaped = !escaped && character.value == '\\';
        }
    }
}

fn keyword(text: &str) -> Option<Keyword> {
    Some(match text {
        "mod" => Keyword::Mod,
        "use" => Keyword::Use,
        "pub" => Keyword::Pub,
        "package" => Keyword::Package,
        "self" => Keyword::SelfValue,
        "super" => Keyword::Super,
        "in" => Keyword::In,
        "as" => Keyword::As,
        "world" => Keyword::World,
        "component" => Keyword::Component,
        "resource" => Keyword::Resource,
        "tag" => Keyword::Tag,
        "system" => Keyword::System,
        "schedule" => Keyword::Schedule,
        "fn" => Keyword::Fn,
        "struct" => Keyword::Struct,
        "enum" => Keyword::Enum,
        "trait" => Keyword::Trait,
        "type" => Keyword::Type,
        "const" => Keyword::Const,
        "static" => Keyword::Static,
        "init" => Keyword::Init,
        "startup" => Keyword::Startup,
        "exit" => Keyword::Exit,
        "bool" | "catch" | "else" | "false" | "for" | "if" | "impl" | "let" | "match" | "mut"
        | "query" | "requires" | "return" | "spawn" | "throw" | "throws" | "true" | "unsafe"
        | "while" | "yield" => Keyword::Reserved,
        _ => return None,
    })
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AstVisibility {
    Private,
    Public,
    Package,
    Super,
    In(AstPath),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AstPathRoot {
    Bare,
    Package,
    SelfValue,
    Super(u64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AstPath {
    pub(crate) root: AstPathRoot,
    pub(crate) segments: Vec<Symbol>,
    pub(crate) span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AstDefinitionKind {
    World,
    Component,
    Resource,
    Tag,
    System,
    Schedule,
    Function,
    Struct,
    Enum,
    Trait,
    TypeAlias,
    Const,
    Static,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AstDefinition {
    pub(crate) kind: AstDefinitionKind,
    pub(crate) visibility: AstVisibility,
    pub(crate) name: Symbol,
    pub(crate) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AstMod {
    pub(crate) visibility: AstVisibility,
    pub(crate) name: Symbol,
    pub(crate) name_span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AstUse {
    pub(crate) visibility: AstVisibility,
    pub(crate) path: AstPath,
    pub(crate) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AstItem {
    Mod(AstMod),
    Use(AstUse),
    Definition(AstDefinition),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AstFile {
    pub(crate) items: Vec<AstItem>,
    pub(crate) eof_span: Span,
}

pub(crate) struct Parser<R: BufRead> {
    lexer: Lexer<R>,
    current: Token,
    next: Token,
}

pub(crate) fn parse_reader<R: BufRead>(file: FileId, reader: R) -> Result<AstFile, Diagnostic> {
    Parser::new(Lexer::new(file, reader))
        .map_err(lex_diagnostic)?
        .parse()
}

impl<R: BufRead> Parser<R> {
    pub(crate) fn new(mut lexer: Lexer<R>) -> Result<Self, LexFailure> {
        let current = lexer.next_token()?;
        let next = lexer.next_token()?;
        Ok(Self {
            lexer,
            current,
            next,
        })
    }

    pub(crate) fn parse(mut self) -> Result<AstFile, Diagnostic> {
        let mut items = Vec::new();
        while self.current.kind != TokenKind::Eof {
            items.push(self.parse_item()?);
        }
        Ok(AstFile {
            items,
            eof_span: self.current.span,
        })
    }

    fn parse_item(&mut self) -> Result<AstItem, Diagnostic> {
        if self.current.kind == TokenKind::Keyword(Keyword::Startup) {
            return Err(Diagnostic::at(
                "MIGRATE001",
                self.current.span,
                "M26 `startup` is not accepted by M27 targets; move data initialization into `world Name { init { ... } }` and process control into `fn main`",
            ));
        }
        if self.current.kind == TokenKind::Keyword(Keyword::Exit) {
            return Err(exit_migration(self.current.span));
        }
        if self.current.kind == TokenKind::Other('#') {
            return Err(Diagnostic::at(
                "MODULE006",
                self.current.span,
                "module path attributes are not supported; `mod name;` has one deterministic file mapping",
            ));
        }

        let visibility = self.parse_visibility()?;
        match self.current.kind.clone() {
            TokenKind::Keyword(Keyword::Mod) => self.parse_mod(visibility).map(AstItem::Mod),
            TokenKind::Keyword(Keyword::Use) => self.parse_use(visibility).map(AstItem::Use),
            TokenKind::Keyword(keyword) => self
                .definition_kind(keyword)
                .ok_or_else(|| {
                    Diagnostic::at("PARSE001", self.current.span, "expected an M27 item")
                })
                .and_then(|kind| self.parse_definition(visibility, kind))
                .map(AstItem::Definition),
            _ => Err(Diagnostic::at(
                "PARSE001",
                self.current.span,
                "expected `mod`, `use`, or an item declaration",
            )),
        }
    }

    fn parse_visibility(&mut self) -> Result<AstVisibility, Diagnostic> {
        if self.current.kind != TokenKind::Keyword(Keyword::Pub) {
            return Ok(AstVisibility::Private);
        }
        self.advance()?;
        if self.current.kind != TokenKind::LeftParen {
            return Ok(AstVisibility::Public);
        }
        self.advance()?;
        let visibility = match self.current.kind {
            TokenKind::Keyword(Keyword::Package) => {
                self.advance()?;
                AstVisibility::Package
            }
            TokenKind::Keyword(Keyword::Super) => {
                self.advance()?;
                AstVisibility::Super
            }
            TokenKind::Keyword(Keyword::In) => {
                self.advance()?;
                AstVisibility::In(self.parse_path()?)
            }
            _ => {
                return Err(Diagnostic::at(
                    "PARSE002",
                    self.current.span,
                    "expected `package`, `super`, or `in path` in visibility",
                ));
            }
        };
        self.expect(TokenKind::RightParen, "expected `)` to close visibility")?;
        Ok(visibility)
    }

    fn parse_mod(&mut self, visibility: AstVisibility) -> Result<AstMod, Diagnostic> {
        self.advance()?;
        let (name, name_span) = self.identifier("expected module name after `mod`")?;
        if self.current.kind == TokenKind::LeftBrace {
            return Err(Diagnostic::at(
                "MODULE004",
                self.current.span,
                "inline modules are not supported; declare `mod name;` and place the child in its deterministic `.arc` file",
            ));
        }
        self.expect(TokenKind::Semicolon, "expected `;` after `mod name`")?;
        Ok(AstMod {
            visibility,
            name,
            name_span,
        })
    }

    fn parse_use(&mut self, visibility: AstVisibility) -> Result<AstUse, Diagnostic> {
        let start = self.current.span;
        self.advance()?;
        let path = self.parse_path()?;
        if matches!(
            self.current.kind,
            TokenKind::Other('*') | TokenKind::LeftBrace
        ) {
            return Err(Diagnostic::at(
                "NAME004",
                self.current.span,
                "glob and grouped imports are not supported in M27-B",
            ));
        }
        if self.current.kind == TokenKind::Keyword(Keyword::As) {
            return Err(Diagnostic::at(
                "NAME005",
                self.current.span,
                "renamed imports are not supported in M27-B",
            ));
        }
        let end = self.expect(TokenKind::Semicolon, "expected `;` after `use` path")?;
        Ok(AstUse {
            visibility,
            path,
            span: start.join(end),
        })
    }

    fn parse_path(&mut self) -> Result<AstPath, Diagnostic> {
        let start = self.current.span;
        let mut root = AstPathRoot::Bare;
        let mut segments = Vec::new();
        match self.current.kind {
            TokenKind::Keyword(Keyword::Package) => {
                root = AstPathRoot::Package;
                self.advance()?;
                self.expect(TokenKind::ColonColon, "expected `::` after `package`")?;
            }
            TokenKind::Keyword(Keyword::SelfValue) => {
                root = AstPathRoot::SelfValue;
                self.advance()?;
                self.expect(TokenKind::ColonColon, "expected `::` after `self`")?;
            }
            TokenKind::Keyword(Keyword::Super) => {
                let mut count = 0_u64;
                loop {
                    count = count.checked_add(1).ok_or_else(|| {
                        Diagnostic::at("NAME006", self.current.span, "`super` depth overflow")
                    })?;
                    self.advance()?;
                    self.expect(TokenKind::ColonColon, "expected `::` after `super`")?;
                    if self.current.kind != TokenKind::Keyword(Keyword::Super) {
                        break;
                    }
                }
                root = AstPathRoot::Super(count);
            }
            _ => {}
        }

        let (first, mut end) = self.identifier("expected path segment")?;
        segments.push(first);
        while self.current.kind == TokenKind::ColonColon {
            self.advance()?;
            if matches!(
                self.current.kind,
                TokenKind::Other('*') | TokenKind::LeftBrace
            ) {
                return Err(Diagnostic::at(
                    "NAME004",
                    self.current.span,
                    "glob and grouped imports are not supported in M27-B",
                ));
            }
            let (segment, span) = self.identifier("expected path segment after `::`")?;
            segments.push(segment);
            end = span;
        }
        Ok(AstPath {
            root,
            segments,
            span: start.join(end),
        })
    }

    fn definition_kind(&self, keyword: Keyword) -> Option<AstDefinitionKind> {
        Some(match keyword {
            Keyword::World => AstDefinitionKind::World,
            Keyword::Component => AstDefinitionKind::Component,
            Keyword::Resource => AstDefinitionKind::Resource,
            Keyword::Tag => AstDefinitionKind::Tag,
            Keyword::System => AstDefinitionKind::System,
            Keyword::Schedule => AstDefinitionKind::Schedule,
            Keyword::Fn => AstDefinitionKind::Function,
            Keyword::Struct => AstDefinitionKind::Struct,
            Keyword::Enum => AstDefinitionKind::Enum,
            Keyword::Trait => AstDefinitionKind::Trait,
            Keyword::Type => AstDefinitionKind::TypeAlias,
            Keyword::Const => AstDefinitionKind::Const,
            Keyword::Static => AstDefinitionKind::Static,
            _ => return None,
        })
    }

    fn parse_definition(
        &mut self,
        visibility: AstVisibility,
        kind: AstDefinitionKind,
    ) -> Result<AstDefinition, Diagnostic> {
        let start = self.current.span;
        self.advance()?;
        let (name, name_span) = self.identifier("expected declaration name")?;
        if kind == AstDefinitionKind::World {
            return self.parse_world(visibility, name, start);
        }

        let mut end = name_span;
        let mut delimiters = VecDeque::new();
        let mut saw_body = false;
        loop {
            match self.current.kind {
                TokenKind::Keyword(Keyword::Exit) => return Err(exit_migration(self.current.span)),
                TokenKind::Keyword(Keyword::Startup) => {
                    return Err(Diagnostic::at(
                        "MIGRATE001",
                        self.current.span,
                        "M26 `startup` is not accepted by M27 targets",
                    ));
                }
                TokenKind::Eof => {
                    if delimiters.is_empty() {
                        break;
                    }
                    return Err(Diagnostic::at(
                        "PARSE003",
                        self.current.span,
                        "source ended inside a declaration",
                    ));
                }
                TokenKind::Semicolon if delimiters.is_empty() => {
                    end = self.current.span;
                    self.advance()?;
                    break;
                }
                TokenKind::LeftBrace => {
                    saw_body = true;
                    delimiters.push_back(TokenKind::RightBrace);
                }
                TokenKind::LeftParen => delimiters.push_back(TokenKind::RightParen),
                TokenKind::LeftBracket => delimiters.push_back(TokenKind::RightBracket),
                TokenKind::RightBrace | TokenKind::RightParen | TokenKind::RightBracket => {
                    let expected = delimiters.pop_back().ok_or_else(|| {
                        Diagnostic::at("PARSE004", self.current.span, "unmatched closing delimiter")
                    })?;
                    if self.current.kind != expected {
                        return Err(Diagnostic::at(
                            "PARSE004",
                            self.current.span,
                            "mismatched closing delimiter",
                        ));
                    }
                    end = self.current.span;
                    self.advance()?;
                    if delimiters.is_empty() && saw_body {
                        if self.current.kind == TokenKind::Semicolon {
                            end = self.current.span;
                            self.advance()?;
                        }
                        break;
                    }
                    continue;
                }
                _ => {}
            }
            end = self.current.span;
            self.advance()?;
        }
        Ok(AstDefinition {
            kind,
            visibility,
            name,
            span: start.join(end),
        })
    }

    fn parse_world(
        &mut self,
        visibility: AstVisibility,
        name: Symbol,
        start: Span,
    ) -> Result<AstDefinition, Diagnostic> {
        if self.current.kind != TokenKind::LeftBrace {
            return Err(Diagnostic::at(
                "MIGRATE001",
                start,
                "M26 `world Name` headers are not accepted by M27 targets; rewrite the root world as `world Name { init { ... } }` and move process control into `fn main`",
            ));
        }
        self.expect(TokenKind::LeftBrace, "expected `{` after world name")?;
        self.expect_keyword(Keyword::Init, "expected `init` block in world")?;
        self.expect(TokenKind::LeftBrace, "expected `{` after `init`")?;
        let mut depth = 1_u64;
        while depth != 0 {
            match self.current.kind {
                TokenKind::Keyword(Keyword::Exit) => return Err(exit_migration(self.current.span)),
                TokenKind::Keyword(Keyword::Startup) => {
                    return Err(Diagnostic::at(
                        "MIGRATE001",
                        self.current.span,
                        "M26 `startup` is not accepted by M27 targets",
                    ));
                }
                TokenKind::LeftBrace => {
                    depth = depth.checked_add(1).ok_or_else(|| {
                        Diagnostic::at(
                            "PARSE005",
                            self.current.span,
                            "world initializer depth overflow",
                        )
                    })?
                }
                TokenKind::RightBrace => depth -= 1,
                TokenKind::Eof => {
                    return Err(Diagnostic::at(
                        "PARSE005",
                        self.current.span,
                        "expected `}` to close world initializer",
                    ));
                }
                _ => {}
            }
            self.advance()?;
        }
        let close = self.expect(TokenKind::RightBrace, "expected `}` to close world")?;
        Ok(AstDefinition {
            kind: AstDefinitionKind::World,
            visibility,
            name,
            span: start.join(close),
        })
    }

    fn identifier(&mut self, message: &'static str) -> Result<(Symbol, Span), Diagnostic> {
        let TokenKind::Identifier(name) = self.current.kind.clone() else {
            return Err(Diagnostic::at("PARSE001", self.current.span, message));
        };
        let span = self.current.span;
        self.advance()?;
        Ok((name, span))
    }

    fn expect_keyword(
        &mut self,
        keyword: Keyword,
        message: &'static str,
    ) -> Result<Span, Diagnostic> {
        self.expect(TokenKind::Keyword(keyword), message)
    }

    fn expect(&mut self, kind: TokenKind, message: &'static str) -> Result<Span, Diagnostic> {
        if self.current.kind != kind {
            return Err(Diagnostic::at("PARSE001", self.current.span, message));
        }
        let span = self.current.span;
        self.advance()?;
        Ok(span)
    }

    fn advance(&mut self) -> Result<(), Diagnostic> {
        self.current = self.next.clone();
        self.next = self.lexer.next_token().map_err(lex_diagnostic)?;
        Ok(())
    }
}

fn lex_diagnostic(error: LexFailure) -> Diagnostic {
    match error {
        LexFailure::Diagnostic(diagnostic) => *diagnostic,
        LexFailure::Io(error) => {
            Diagnostic::path("SOURCE002", format!("could not read source: {error}"))
        }
    }
}

fn exit_migration(span: Span) -> Diagnostic {
    Diagnostic::at(
        "MIGRATE002",
        span,
        "M26 source `exit` is not accepted by M27 targets; return an `i32` from binary `fn main`",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(source: &str) -> Result<AstFile, Diagnostic> {
        let lexer = Lexer::new(FileId(0), Cursor::new(source.as_bytes()));
        Parser::new(lexer).map_err(lex_diagnostic)?.parse()
    }

    #[test]
    fn lexes_nfc_unicode_identifiers_incrementally() {
        let parsed = parse("component Cafe\u{301} { value: i32 }").unwrap();
        let AstItem::Definition(definition) = &parsed.items[0] else {
            panic!("definition expected")
        };
        assert_eq!(definition.name.as_str(), "Caf\u{e9}");
    }

    #[test]
    fn parses_explicit_module_use_visibility_and_world() {
        let parsed = parse(
            "pub(package) mod physics;\n\
             pub(in package::physics) use package::shared::Position;\n\
             pub world Game { init { spawn { } } }\n\
             pub fn main() { }",
        )
        .unwrap();
        assert_eq!(parsed.items.len(), 4);
        assert!(matches!(parsed.items[0], AstItem::Mod(_)));
        assert!(matches!(parsed.items[1], AstItem::Use(_)));
    }

    #[test]
    fn hard_rejects_m26_entry_syntax() {
        let legacy_world = parse("world Legacy\n\nstartup { exit 0 }").unwrap_err();
        assert_eq!(legacy_world.code, "MIGRATE001");
        assert!(legacy_world.message.contains("M26 `world Name`"));
        let startup = parse("startup { exit 0 }").unwrap_err();
        assert_eq!(startup.code, "MIGRATE001");
        let exit = parse("pub fn main() { exit 0 }").unwrap_err();
        assert_eq!(exit.code, "MIGRATE002");
    }

    #[test]
    fn rejects_non_explicit_module_forms() {
        assert_eq!(parse("mod physics { }").unwrap_err().code, "MODULE004");
        assert_eq!(
            parse("#[path='x'] mod physics;").unwrap_err().code,
            "MODULE006"
        );
        assert_eq!(
            parse("use package::physics::*;").unwrap_err().code,
            "NAME004"
        );
    }

    #[test]
    fn reserved_language_words_are_never_declaration_or_module_identifiers() {
        for reserved in [
            "bool", "catch", "else", "false", "for", "if", "impl", "let", "match", "mut", "query",
            "requires", "return", "spawn", "throw", "throws", "true", "unsafe", "while", "yield",
        ] {
            assert_eq!(
                parse(&format!("component {reserved} {{ }}"))
                    .unwrap_err()
                    .code,
                "PARSE001",
                "`{reserved}` must remain reserved"
            );
        }
        assert_eq!(parse("mod if;").unwrap_err().code, "PARSE001");
    }
}
