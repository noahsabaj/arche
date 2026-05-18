use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenKind {
    Keyword(Keyword),
    Identifier(String),
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
    Plus,
    Minus,
    Star,
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
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LexError {
    pub offset: usize,
    pub character: char,
}

pub fn lex(source: &str) -> Result<Vec<Token>, LexError> {
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < source.len() {
        let character = source[index..].chars().next().expect("valid char boundary");

        if character.is_whitespace() {
            index += character.len_utf8();
            continue;
        }

        if is_identifier_start(character) {
            let start = index;
            index += character.len_utf8();

            while index < source.len() {
                let next = source[index..].chars().next().expect("valid char boundary");
                if !is_identifier_continue(next) {
                    break;
                }

                index += next.len_utf8();
            }

            let text = &source[start..index];
            let kind = match Keyword::from_text(text) {
                Some(keyword) => TokenKind::Keyword(keyword),
                None => TokenKind::Identifier(text.to_string()),
            };
            tokens.push(Token {
                kind,
                span: Span { start, end: index },
            });
            continue;
        }

        if character.is_ascii_digit() {
            let start = index;
            index += character.len_utf8();

            while index < source.len() {
                let next = source[index..].chars().next().expect("valid char boundary");
                if !next.is_ascii_digit() {
                    break;
                }

                index += next.len_utf8();
            }

            let mut is_float = false;
            if index < source.len() && source.as_bytes()[index] == b'.' {
                let dot_index = index;
                let digit_index = dot_index + 1;
                if digit_index < source.len() {
                    let next = source[digit_index..]
                        .chars()
                        .next()
                        .expect("valid char boundary");
                    if next.is_ascii_digit() {
                        is_float = true;
                        index = digit_index + next.len_utf8();

                        while index < source.len() {
                            let next = source[index..].chars().next().expect("valid char boundary");
                            if !next.is_ascii_digit() {
                                break;
                            }

                            index += next.len_utf8();
                        }
                    }
                }
            }

            let kind = if is_float {
                TokenKind::Float(source[start..index].to_string())
            } else {
                TokenKind::Integer(source[start..index].to_string())
            };
            tokens.push(Token {
                kind,
                span: Span { start, end: index },
            });
            continue;
        }

        let start = index;
        match character {
            '{' => {
                index += 1;
                tokens.push(Token {
                    kind: TokenKind::LeftBrace,
                    span: Span { start, end: index },
                });
            }
            '}' => {
                index += 1;
                tokens.push(Token {
                    kind: TokenKind::RightBrace,
                    span: Span { start, end: index },
                });
            }
            '(' => {
                index += 1;
                tokens.push(Token {
                    kind: TokenKind::LeftParen,
                    span: Span { start, end: index },
                });
            }
            ')' => {
                index += 1;
                tokens.push(Token {
                    kind: TokenKind::RightParen,
                    span: Span { start, end: index },
                });
            }
            '[' => {
                index += 1;
                tokens.push(Token {
                    kind: TokenKind::LeftBracket,
                    span: Span { start, end: index },
                });
            }
            ']' => {
                index += 1;
                tokens.push(Token {
                    kind: TokenKind::RightBracket,
                    span: Span { start, end: index },
                });
            }
            ':' => {
                index += 1;
                tokens.push(Token {
                    kind: TokenKind::Colon,
                    span: Span { start, end: index },
                });
            }
            ',' => {
                index += 1;
                tokens.push(Token {
                    kind: TokenKind::Comma,
                    span: Span { start, end: index },
                });
            }
            '.' => {
                index += 1;
                tokens.push(Token {
                    kind: TokenKind::Dot,
                    span: Span { start, end: index },
                });
            }
            '=' => {
                index += 1;
                tokens.push(Token {
                    kind: TokenKind::Equal,
                    span: Span { start, end: index },
                });
            }
            '+' => {
                index += 1;
                tokens.push(Token {
                    kind: TokenKind::Plus,
                    span: Span { start, end: index },
                });
            }
            '-' => {
                index += 1;
                tokens.push(Token {
                    kind: TokenKind::Minus,
                    span: Span { start, end: index },
                });
            }
            '*' => {
                index += 1;
                tokens.push(Token {
                    kind: TokenKind::Star,
                    span: Span { start, end: index },
                });
            }
            _ => {
                return Err(LexError {
                    offset: start,
                    character,
                });
            }
        }
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span {
            start: source.len(),
            end: source.len(),
        },
    });

    Ok(tokens)
}

fn is_identifier_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_'
}

fn is_identifier_continue(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
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
            Self::Plus => formatter.write_str("Plus"),
            Self::Minus => formatter.write_str("Minus"),
            Self::Star => formatter.write_str("Star"),
            Self::Eof => formatter.write_str("Eof"),
        }
    }
}

impl fmt::Display for LexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unexpected character '{}' at byte {}",
            self.character, self.offset
        )
    }
}
