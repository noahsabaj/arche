use std::fmt;
use std::io::{self, BufRead};

use crate::identifier::Identifier;
use crate::lexer::{Keyword, LexError, Lexer, LexerFailure, SourceSpan as Span, Token, TokenKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Program {
    pub world: WorldDecl,
    pub tags: Vec<TagDecl>,
    pub components: Vec<ComponentDecl>,
    pub resources: Vec<ResourceDecl>,
    pub systems: Vec<SystemDecl>,
    pub schedules: Vec<ScheduleDecl>,
    pub startups: Vec<StartupBlock>,
    pub eof_span: Span,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TagDecl {
    pub name: Identifier,
    pub name_span: Span,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldDecl {
    pub name: Identifier,
    pub name_span: Span,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentDecl {
    pub name: Identifier,
    pub name_span: Span,
    pub fields: Vec<ComponentField>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentField {
    pub name: Identifier,
    pub name_span: Span,
    pub type_name: TypeName,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceDecl {
    pub name: Identifier,
    pub name_span: Span,
    pub fields: Vec<ResourceField>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceField {
    pub name: Identifier,
    pub name_span: Span,
    pub type_name: TypeName,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemDecl {
    pub name: Identifier,
    pub name_span: Span,
    pub params: Vec<SystemParam>,
    pub body: SystemBody,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemParam {
    pub name: Identifier,
    pub name_span: Span,
    pub kind: SystemParamKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SystemParamKind {
    ReadResource {
        resource_name: Identifier,
        resource_span: Span,
    },
    MutResource {
        resource_name: Identifier,
        resource_span: Span,
    },
    Query {
        terms: Vec<QueryTerm>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryTerm {
    pub access: QueryAccess,
    pub component_name: Identifier,
    pub component_span: Span,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryAccess {
    Read,
    Mut,
    Exclude,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemBody {
    pub statements: Vec<SystemBodyStatement>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SystemBodyStatement {
    Expression(Expression),
    QueryLoop(SystemQueryLoopStatement),
    Let(LetStatement),
    Assign(AssignmentStatement),
    AddAssign(AddAssignStatement),
    Block(SystemBlockStatement),
    If(SystemIfStatement),
    While(SystemWhileStatement),
}

impl SystemBodyStatement {
    pub fn span(&self) -> Span {
        match self {
            Self::Expression(expression) => expression.span(),
            Self::QueryLoop(statement) => statement.span,
            Self::Let(statement) => statement.span,
            Self::Assign(statement) => statement.span,
            Self::AddAssign(statement) => statement.span,
            Self::Block(statement) => statement.span,
            Self::If(statement) => statement.span,
            Self::While(statement) => statement.span,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemBlockStatement {
    pub statements: Vec<SystemBodyStatement>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemIfStatement {
    pub condition: Expression,
    pub then_block: SystemBlockStatement,
    pub else_block: Option<SystemBlockStatement>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemWhileStatement {
    pub condition: Expression,
    pub body: SystemBlockStatement,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemQueryLoopStatement {
    pub query_param: Identifier,
    pub query_span: Span,
    pub bindings: Vec<SystemQueryLoopBinding>,
    pub body: Vec<SystemBodyStatement>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemQueryLoopBinding {
    pub name: Identifier,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddAssignStatement {
    pub target: Expression,
    pub value: Expression,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduleDecl {
    pub name: Identifier,
    pub name_span: Span,
    pub items: Vec<ScheduleItem>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScheduleItem {
    Run {
        system_name: Identifier,
        system_span: Span,
        span: Span,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupBlock {
    pub statements: Vec<Statement>,
    pub keyword_span: Span,
    pub close_span: Span,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Statement {
    Let(LetStatement),
    Assign(AssignmentStatement),
    AddAssign(AddAssignStatement),
    Run(RunStatement),
    Spawn(SpawnStatement),
    Resource(ResourceStatement),
    Exit(ExitStatement),
}

impl Statement {
    pub fn span(&self) -> Span {
        match self {
            Self::Let(statement) => statement.span,
            Self::Assign(statement) => statement.span,
            Self::AddAssign(statement) => statement.span,
            Self::Run(statement) => statement.span,
            Self::Spawn(statement) => statement.span,
            Self::Resource(statement) => statement.span,
            Self::Exit(statement) => statement.span,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LetStatement {
    pub name: Identifier,
    pub name_span: Span,
    pub mutable: bool,
    pub type_name: TypeName,
    pub initializer: Expression,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssignmentStatement {
    pub target: Expression,
    pub value: Expression,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunStatement {
    pub schedule_name: Identifier,
    pub schedule_span: Span,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpawnStatement {
    pub components: Vec<SpawnComponentLiteral>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpawnComponentLiteral {
    pub name: Identifier,
    pub name_span: Span,
    pub fields: Vec<SpawnComponentField>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpawnComponentField {
    pub name: Identifier,
    pub name_span: Span,
    pub value: ComponentLiteralValue,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceStatement {
    pub name: Identifier,
    pub name_span: Span,
    pub fields: Vec<ResourceLiteralField>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceLiteralField {
    pub name: Identifier,
    pub name_span: Span,
    pub value: ComponentLiteralValue,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComponentLiteralValue {
    Float {
        text: String,
        span: Span,
    },
    Integer {
        value: u64,
        span: Span,
    },
    Bool {
        value: bool,
        span: Span,
    },
    Expression {
        expression: Box<Expression>,
        span: Span,
    },
}

impl ComponentLiteralValue {
    pub fn span(&self) -> Span {
        match self {
            Self::Float { span, .. }
            | Self::Integer { span, .. }
            | Self::Bool { span, .. }
            | Self::Expression { span, .. } => *span,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeName {
    pub name: Identifier,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitStatement {
    pub expression: Expression,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Expression {
    Integer(IntegerLiteral),
    Float {
        text: String,
        span: Span,
    },
    Bool {
        value: bool,
        span: Span,
    },
    Identifier {
        name: Identifier,
        span: Span,
    },
    FieldAccess {
        target: Box<Expression>,
        field_name: Identifier,
        field_span: Span,
        span: Span,
    },
    Unary(UnaryExpression),
    Binary(BinaryExpression),
    Parenthesized {
        expression: Box<Expression>,
        span: Span,
    },
}

impl Expression {
    pub fn span(&self) -> Span {
        match self {
            Self::Integer(integer) => integer.span,
            Self::Float { span, .. }
            | Self::Bool { span, .. }
            | Self::Identifier { span, .. }
            | Self::FieldAccess { span, .. }
            | Self::Parenthesized { span, .. } => *span,
            Self::Unary(unary) => unary.span,
            Self::Binary(binary) => binary.span,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnaryExpression {
    pub operator: UnaryOperator,
    pub operator_span: Span,
    pub operand: Box<Expression>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnaryOperator {
    Not,
    Negate,
    BitNot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BinaryExpression {
    pub operator: BinaryOperator,
    pub operator_span: Span,
    pub left: Box<Expression>,
    pub right: Box<Expression>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    ShiftLeft,
    ShiftRight,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
    BitAnd,
    BitXor,
    BitOr,
    LogicalAnd,
    LogicalOr,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegerLiteral {
    pub value: u64,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub span: Span,
    pub message: String,
}

#[derive(Debug)]
pub enum ParseStreamError {
    Read(io::Error),
    Lex(LexError),
    Parse(ParseError),
}

impl From<LexerFailure> for ParseStreamError {
    fn from(error: LexerFailure) -> Self {
        match error {
            LexerFailure::Read(error) => Self::Read(error),
            LexerFailure::Lex(error) => Self::Lex(error),
        }
    }
}

pub fn parse_lexer<R: BufRead>(lexer: Lexer<R>) -> Result<Program, ParseStreamError> {
    parse_token_source(lexer)
}

#[cfg(test)]
pub fn parse_program(tokens: &[Token]) -> Result<Program, ParseError> {
    match parse_token_source(SliceTokenSource { tokens, current: 0 }) {
        Ok(program) => Ok(program),
        Err(ParseStreamError::Parse(error)) => Err(error),
        Err(ParseStreamError::Read(error)) => panic!("slice token source read failed: {error}"),
        Err(ParseStreamError::Lex(error)) => panic!("slice token source lex failed: {error}"),
    }
}

fn parse_token_source<S: TokenSource>(mut source: S) -> Result<Program, ParseStreamError> {
    let current = source.next_token().map_err(ParseStreamError::from)?;
    let next = source.next_token().map_err(ParseStreamError::from)?;
    let previous_span = current.span;
    let mut parser = Parser {
        source,
        current,
        next,
        previous_span,
        source_error: None,
    };
    let result = parse_program_inner(&mut parser);
    if let Some(error) = parser.source_error {
        return Err(error.into());
    }
    result.map_err(ParseStreamError::Parse)
}

fn parse_program_inner<S: TokenSource>(parser: &mut Parser<S>) -> Result<Program, ParseError> {
    let world = parser.parse_world_declaration()?;
    let mut tags = Vec::new();
    let mut components = Vec::new();
    let mut resources = Vec::new();
    let mut systems = Vec::new();
    let mut schedules = Vec::new();
    let mut startups = Vec::new();
    loop {
        if parser.match_keyword(Keyword::Tag) {
            let declaration = parser.parse_tag_declaration()?;
            push_ast(
                &mut tags,
                declaration.name_span,
                declaration,
                "tag declarations",
            )?;
            continue;
        }

        if parser.match_keyword(Keyword::Component) {
            let declaration = parser.parse_component_declaration()?;
            push_ast(
                &mut components,
                declaration.name_span,
                declaration,
                "component declarations",
            )?;
            continue;
        }

        if parser.match_keyword(Keyword::Resource) {
            let declaration = parser.parse_resource_declaration()?;
            push_ast(
                &mut resources,
                declaration.name_span,
                declaration,
                "resource declarations",
            )?;
            continue;
        }

        if parser.match_keyword(Keyword::System) {
            let declaration = parser.parse_system_declaration()?;
            push_ast(
                &mut systems,
                declaration.name_span,
                declaration,
                "system declarations",
            )?;
            continue;
        }

        if parser.match_keyword(Keyword::Schedule) {
            let declaration = parser.parse_schedule_declaration()?;
            push_ast(
                &mut schedules,
                declaration.name_span,
                declaration,
                "schedule declarations",
            )?;
            continue;
        }

        if parser.match_keyword(Keyword::Startup) {
            let declaration = parser.parse_startup_block()?;
            push_ast(
                &mut startups,
                declaration.keyword_span,
                declaration,
                "startup declarations",
            )?;
            continue;
        }

        break;
    }
    let eof_span = parser.peek().span;
    parser.expect_eof()?;

    Ok(Program {
        span: Span {
            start: world.span.start,
            end: eof_span.end,
        },
        world,
        tags,
        components,
        resources,
        systems,
        schedules,
        startups,
        eof_span,
    })
}

trait TokenSource {
    fn next_token(&mut self) -> Result<Token, LexerFailure>;
}

impl<R: BufRead> TokenSource for Lexer<R> {
    fn next_token(&mut self) -> Result<Token, LexerFailure> {
        Lexer::next_token(self)
    }
}

#[cfg(test)]
struct SliceTokenSource<'a> {
    tokens: &'a [Token],
    current: usize,
}

#[cfg(test)]
impl TokenSource for SliceTokenSource<'_> {
    fn next_token(&mut self) -> Result<Token, LexerFailure> {
        let token = self
            .tokens
            .get(self.current)
            .or_else(|| self.tokens.last())
            .expect("lexer test helper always receives an EOF token")
            .clone();
        self.current = self.current.saturating_add(1);
        Ok(token)
    }
}

struct Parser<S: TokenSource> {
    source: S,
    current: Token,
    next: Token,
    previous_span: Span,
    source_error: Option<LexerFailure>,
}

impl<S: TokenSource> Parser<S> {
    fn parse_world_declaration(&mut self) -> Result<WorldDecl, ParseError> {
        let world_token = self.peek().clone();

        if world_token.kind != TokenKind::Keyword(Keyword::World) {
            return Err(ParseError {
                span: world_token.span,
                message: "expected `world` declaration".to_string(),
            });
        }
        self.advance();

        let (name, name_span) =
            self.parse_identifier_with_span("expected world name after `world`")?;

        Ok(WorldDecl {
            name,
            name_span,
            span: join_spans(world_token.span, name_span),
        })
    }

    fn parse_component_declaration(&mut self) -> Result<ComponentDecl, ParseError> {
        let start = self.previous_span;
        let (name, name_span) =
            self.parse_identifier_with_span("expected component name after `component`")?;
        self.expect(TokenKind::LeftBrace, "expected `{` after component name")?;

        let mut fields = Vec::new();
        while self.peek().kind != TokenKind::RightBrace {
            if self.peek().kind == TokenKind::Eof {
                return Err(ParseError {
                    span: self.peek().span,
                    message: "expected `}` to close component declaration".to_string(),
                });
            }

            let (name, name_span) =
                self.parse_identifier_with_span("expected component field name")?;
            self.expect(TokenKind::Colon, "expected `:` after component field name")?;
            let type_name = self.parse_type_name("expected component field type after `:`")?;
            let field = ComponentField {
                name,
                name_span,
                span: join_spans(name_span, type_name.span),
                type_name,
            };
            push_ast(&mut fields, field.name_span, field, "component fields")?;
        }

        let close_span = self.peek().span;
        self.expect(
            TokenKind::RightBrace,
            "expected `}` to close component declaration",
        )?;
        Ok(ComponentDecl {
            name,
            name_span,
            fields,
            span: join_spans(start, close_span),
        })
    }

    fn parse_tag_declaration(&mut self) -> Result<TagDecl, ParseError> {
        let start = self.previous_span;
        let (name, name_span) = self.parse_identifier_with_span("expected tag name after `tag`")?;
        Ok(TagDecl {
            name,
            name_span,
            span: join_spans(start, name_span),
        })
    }

    fn parse_resource_declaration(&mut self) -> Result<ResourceDecl, ParseError> {
        let start = self.previous_span;
        let (name, name_span) =
            self.parse_identifier_with_span("expected resource name after `resource`")?;
        self.expect(TokenKind::LeftBrace, "expected `{` after resource name")?;

        let mut fields = Vec::new();
        while self.peek().kind != TokenKind::RightBrace {
            if self.peek().kind == TokenKind::Eof {
                return Err(ParseError {
                    span: self.peek().span,
                    message: "expected `}` to close resource declaration".to_string(),
                });
            }

            let (name, name_span) =
                self.parse_identifier_with_span("expected resource field name")?;
            self.expect(TokenKind::Colon, "expected `:` after resource field name")?;
            let type_name = self.parse_type_name("expected resource field type after `:`")?;
            let field = ResourceField {
                name,
                name_span,
                span: join_spans(name_span, type_name.span),
                type_name,
            };
            push_ast(&mut fields, field.name_span, field, "resource fields")?;
        }

        let close_span = self.peek().span;
        self.expect(
            TokenKind::RightBrace,
            "expected `}` to close resource declaration",
        )?;
        Ok(ResourceDecl {
            name,
            name_span,
            fields,
            span: join_spans(start, close_span),
        })
    }

    fn parse_system_declaration(&mut self) -> Result<SystemDecl, ParseError> {
        let start = self.previous_span;
        let (name, name_span) =
            self.parse_identifier_with_span("expected system name after `system`")?;
        self.expect(TokenKind::LeftParen, "expected `(` after system name")?;

        let mut params = Vec::new();
        if self.peek().kind != TokenKind::RightParen {
            loop {
                let param = self.parse_system_param()?;
                push_ast(&mut params, param.name_span, param, "system parameters")?;

                if self.peek().kind != TokenKind::Comma {
                    break;
                }

                self.advance();
            }
        }

        self.expect(
            TokenKind::RightParen,
            "expected `)` after system parameters",
        )?;
        let body = self.parse_system_body()?;

        Ok(SystemDecl {
            name,
            name_span,
            params,
            span: join_spans(start, body.span),
            body,
        })
    }

    fn parse_system_body(&mut self) -> Result<SystemBody, ParseError> {
        let open_span = self.peek().span;
        self.expect(TokenKind::LeftBrace, "expected `{` after system signature")?;

        let mut statements = Vec::new();
        while self.peek().kind != TokenKind::RightBrace {
            if self.peek().kind == TokenKind::Eof {
                return Err(ParseError {
                    span: self.peek().span,
                    message: "expected `}` to close system body".to_string(),
                });
            }

            let statement = self.parse_system_body_statement()?;
            push_ast(
                &mut statements,
                statement.span(),
                statement,
                "system statements",
            )?;
        }

        let close_span = self.peek().span;
        self.expect(TokenKind::RightBrace, "expected `}` to close system body")?;
        Ok(SystemBody {
            statements,
            span: join_spans(open_span, close_span),
        })
    }

    fn parse_system_body_statement(&mut self) -> Result<SystemBodyStatement, ParseError> {
        if self.match_keyword(Keyword::For) {
            return self
                .parse_system_query_loop_statement()
                .map(SystemBodyStatement::QueryLoop);
        }

        if self.match_keyword(Keyword::Let) {
            return self.parse_let().map(SystemBodyStatement::Let);
        }

        if self.match_keyword(Keyword::If) {
            return self
                .parse_system_if_statement()
                .map(SystemBodyStatement::If);
        }

        if self.match_keyword(Keyword::While) {
            return self
                .parse_system_while_statement()
                .map(SystemBodyStatement::While);
        }

        if self.peek().kind == TokenKind::LeftBrace {
            return self.parse_system_block().map(SystemBodyStatement::Block);
        }

        let expression = self.parse_expression_with_message("expected system body expression")?;
        if self.peek().kind == TokenKind::Plus && self.peek_next().kind == TokenKind::Equal {
            let start = expression.span();
            self.advance();
            self.advance();
            let value = self.parse_expression_with_message("expected expression after `+=`")?;
            let span = join_spans(start, value.span());
            return Ok(SystemBodyStatement::AddAssign(AddAssignStatement {
                target: expression,
                value,
                span,
            }));
        }

        if self.peek().kind == TokenKind::Equal {
            let start = expression.span();
            self.advance();
            let value = self.parse_expression_with_message("expected expression after `=`")?;
            let span = join_spans(start, value.span());
            return Ok(SystemBodyStatement::Assign(AssignmentStatement {
                target: expression,
                value,
                span,
            }));
        }

        Ok(SystemBodyStatement::Expression(expression))
    }

    fn parse_system_block(&mut self) -> Result<SystemBlockStatement, ParseError> {
        let open_span = self.peek().span;
        self.expect(TokenKind::LeftBrace, "expected `{` to open system block")?;
        let mut statements = Vec::new();
        while self.peek().kind != TokenKind::RightBrace {
            if self.peek().kind == TokenKind::Eof {
                return Err(ParseError {
                    span: self.peek().span,
                    message: "expected `}` to close system block".to_string(),
                });
            }
            let statement = self.parse_system_body_statement()?;
            push_ast(
                &mut statements,
                statement.span(),
                statement,
                "system statements",
            )?;
        }
        let close_span = self.peek().span;
        self.advance();
        Ok(SystemBlockStatement {
            statements,
            span: join_spans(open_span, close_span),
        })
    }

    fn parse_system_if_statement(&mut self) -> Result<SystemIfStatement, ParseError> {
        let start = self.previous_span;
        let condition = self.parse_expression_with_message("expected condition after `if`")?;
        let then_block = self.parse_system_block()?;
        let else_block = if self.match_keyword(Keyword::Else) {
            Some(self.parse_system_block()?)
        } else {
            None
        };
        let end = else_block
            .as_ref()
            .map_or(then_block.span, |block| block.span);
        Ok(SystemIfStatement {
            condition,
            then_block,
            else_block,
            span: join_spans(start, end),
        })
    }

    fn parse_system_while_statement(&mut self) -> Result<SystemWhileStatement, ParseError> {
        let start = self.previous_span;
        let condition = self.parse_expression_with_message("expected condition after `while`")?;
        let body = self.parse_system_block()?;
        let span = join_spans(start, body.span);
        Ok(SystemWhileStatement {
            condition,
            body,
            span,
        })
    }

    fn parse_system_query_loop_statement(
        &mut self,
    ) -> Result<SystemQueryLoopStatement, ParseError> {
        let start = self.previous_span;
        self.expect(TokenKind::LeftParen, "expected `(` after `for`")?;

        let mut bindings = Vec::new();
        if self.peek().kind != TokenKind::RightParen {
            loop {
                let (name, span) =
                    self.parse_identifier_with_span("expected query loop binding name")?;
                push_ast(
                    &mut bindings,
                    span,
                    SystemQueryLoopBinding { name, span },
                    "query bindings",
                )?;

                if self.peek().kind != TokenKind::Comma {
                    break;
                }

                self.advance();
            }
        }

        self.expect(
            TokenKind::RightParen,
            "expected `)` after query loop bindings",
        )?;
        self.expect(
            TokenKind::Keyword(Keyword::In),
            "expected `in` after query loop bindings",
        )?;
        let (query_param, query_span) =
            self.parse_identifier_with_span("expected query parameter name after `in`")?;
        let block = self.parse_system_block()?;
        Ok(SystemQueryLoopStatement {
            query_param,
            query_span,
            bindings,
            body: block.statements,
            span: join_spans(start, block.span),
        })
    }

    fn parse_schedule_declaration(&mut self) -> Result<ScheduleDecl, ParseError> {
        let start = self.previous_span;
        let (name, name_span) =
            self.parse_identifier_with_span("expected schedule name after `schedule`")?;
        self.expect(TokenKind::LeftBrace, "expected `{` after schedule name")?;

        let mut items = Vec::new();
        while self.peek().kind != TokenKind::RightBrace {
            if self.peek().kind == TokenKind::Eof {
                return Err(ParseError {
                    span: self.peek().span,
                    message: "expected `}` to close schedule declaration".to_string(),
                });
            }

            let item = self.parse_schedule_item()?;
            let span = match &item {
                ScheduleItem::Run { span, .. } => *span,
            };
            push_ast(&mut items, span, item, "schedule items")?;
        }

        let close_span = self.peek().span;
        self.expect(
            TokenKind::RightBrace,
            "expected `}` to close schedule declaration",
        )?;
        Ok(ScheduleDecl {
            name,
            name_span,
            items,
            span: join_spans(start, close_span),
        })
    }

    fn parse_schedule_item(&mut self) -> Result<ScheduleItem, ParseError> {
        if self.match_keyword(Keyword::Run) {
            let start = self.previous_span;
            let (system_name, system_span) =
                self.parse_identifier_with_span("expected system name after `run`")?;
            return Ok(ScheduleItem::Run {
                system_name,
                system_span,
                span: join_spans(start, system_span),
            });
        }

        Err(ParseError {
            span: self.peek().span,
            message: "expected `run` schedule item".to_string(),
        })
    }

    fn parse_system_param(&mut self) -> Result<SystemParam, ParseError> {
        let start = self.peek().span;
        let (name, name_span) =
            self.parse_identifier_with_span("expected system parameter name")?;
        self.expect(TokenKind::Colon, "expected `:` after system parameter name")?;

        if self.match_keyword(Keyword::Read) {
            let (resource_name, resource_span) =
                self.parse_identifier_with_span("expected resource name after `read`")?;

            return Ok(SystemParam {
                name,
                name_span,
                span: join_spans(start, resource_span),
                kind: SystemParamKind::ReadResource {
                    resource_name,
                    resource_span,
                },
            });
        }

        if self.match_keyword(Keyword::Mut) {
            let (resource_name, resource_span) =
                self.parse_identifier_with_span("expected resource name after `mut`")?;
            return Ok(SystemParam {
                name,
                name_span,
                span: join_spans(start, resource_span),
                kind: SystemParamKind::MutResource {
                    resource_name,
                    resource_span,
                },
            });
        }

        if self.match_keyword(Keyword::Query) {
            let terms = self.parse_query_terms()?;
            let end = self.previous_span;

            return Ok(SystemParam {
                name,
                name_span,
                span: join_spans(start, end),
                kind: SystemParamKind::Query { terms },
            });
        }

        Err(ParseError {
            span: self.peek().span,
            message: "expected `read`, `mut`, or `query` system parameter access".to_string(),
        })
    }

    fn parse_query_terms(&mut self) -> Result<Vec<QueryTerm>, ParseError> {
        self.expect(TokenKind::LeftBracket, "expected `[` after `query`")?;

        let mut terms = Vec::new();
        if self.peek().kind == TokenKind::RightBracket {
            return Err(ParseError {
                span: self.peek().span,
                message: "expected query component term".to_string(),
            });
        }

        loop {
            let term = self.parse_query_term()?;
            push_ast(&mut terms, term.component_span, term, "query terms")?;

            if self.peek().kind != TokenKind::Comma {
                break;
            }

            self.advance();
        }

        self.expect(
            TokenKind::RightBracket,
            "expected `]` after query component terms",
        )?;
        Ok(terms)
    }

    fn parse_query_term(&mut self) -> Result<QueryTerm, ParseError> {
        let start = self.peek().span;
        let access = if self.peek().kind == TokenKind::Bang {
            self.advance();
            QueryAccess::Exclude
        } else if self.match_keyword(Keyword::Mut) {
            QueryAccess::Mut
        } else {
            QueryAccess::Read
        };
        let (component_name, component_span) =
            self.parse_identifier_with_span("expected query component name")?;

        Ok(QueryTerm {
            access,
            component_name,
            component_span,
            span: join_spans(start, component_span),
        })
    }

    fn parse_startup_block(&mut self) -> Result<StartupBlock, ParseError> {
        let start = self.previous_span;
        self.expect(TokenKind::LeftBrace, "expected `{` after `startup`")?;

        let mut statements = Vec::new();
        while self.peek().kind != TokenKind::RightBrace {
            if self.peek().kind == TokenKind::Eof {
                return Err(ParseError {
                    span: self.peek().span,
                    message: "expected `}` to close startup block".to_string(),
                });
            }
            let statement = self.parse_statement()?;
            push_ast(
                &mut statements,
                statement.span(),
                statement,
                "startup statements",
            )?;
        }

        let close_span = self.peek().span;
        self.expect(TokenKind::RightBrace, "expected `}` to close startup block")?;
        Ok(StartupBlock {
            statements,
            keyword_span: start,
            close_span,
            span: join_spans(start, close_span),
        })
    }

    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        if self.match_keyword(Keyword::Let) {
            return self.parse_let_statement();
        }

        if self.match_keyword(Keyword::Exit) {
            return self.parse_exit_statement();
        }

        if self.match_keyword(Keyword::Run) {
            return self.parse_run_statement();
        }

        if self.match_keyword(Keyword::Spawn) {
            return self.parse_spawn_statement();
        }

        if self.match_keyword(Keyword::Resource) {
            return self.parse_resource_statement();
        }

        if matches!(self.peek().kind, TokenKind::Identifier(_)) {
            let target = self.parse_expression()?;
            if self.peek().kind == TokenKind::Plus && self.peek_next().kind == TokenKind::Equal {
                self.advance();
                self.advance();
                let value = self.parse_expression_with_message("expected expression after `+=`")?;
                let span = join_spans(target.span(), value.span());
                return Ok(Statement::AddAssign(AddAssignStatement {
                    target,
                    value,
                    span,
                }));
            }
            self.expect(TokenKind::Equal, "expected `=` after assignment target")?;
            let value = self.parse_expression_with_message("expected expression after `=`")?;
            let span = join_spans(target.span(), value.span());
            return Ok(Statement::Assign(AssignmentStatement {
                target,
                value,
                span,
            }));
        }

        Err(ParseError {
            span: self.peek().span,
            message: "expected statement".to_string(),
        })
    }

    fn parse_let_statement(&mut self) -> Result<Statement, ParseError> {
        self.parse_let().map(Statement::Let)
    }

    fn parse_let(&mut self) -> Result<LetStatement, ParseError> {
        let start = self.previous_span;
        let mutable = self.match_keyword(Keyword::Mut);
        let (name, name_span) =
            self.parse_identifier_with_span("expected binding name after `let`")?;
        self.expect(TokenKind::Colon, "expected `:` after let binding name")?;
        let type_name = self.parse_type_name("expected type name after `:`")?;
        self.expect(TokenKind::Equal, "expected `=` after let binding type")?;
        let initializer = self.parse_expression()?;
        let span = join_spans(start, initializer.span());

        Ok(LetStatement {
            name,
            name_span,
            mutable,
            type_name,
            initializer,
            span,
        })
    }

    fn parse_run_statement(&mut self) -> Result<Statement, ParseError> {
        let start = self.previous_span;
        let (schedule_name, schedule_span) =
            self.parse_identifier_with_span("expected schedule name after `run`")?;

        Ok(Statement::Run(RunStatement {
            schedule_name,
            schedule_span,
            span: join_spans(start, schedule_span),
        }))
    }

    fn parse_exit_statement(&mut self) -> Result<Statement, ParseError> {
        let start = self.previous_span;
        let expression = self.parse_expression_with_message("expected expression after `exit`")?;
        let span = join_spans(start, expression.span());

        Ok(Statement::Exit(ExitStatement { expression, span }))
    }

    fn parse_spawn_statement(&mut self) -> Result<Statement, ParseError> {
        let start = self.previous_span;
        self.expect(TokenKind::LeftBrace, "expected `{` after `spawn`")?;

        let mut components = Vec::new();
        while self.peek().kind != TokenKind::RightBrace {
            if self.peek().kind == TokenKind::Eof {
                return Err(ParseError {
                    span: self.peek().span,
                    message: "expected `}` to close spawn block".to_string(),
                });
            }

            let (name, name_span) =
                self.parse_identifier_with_span("expected component literal in spawn block")?;
            self.expect(
                TokenKind::LeftBrace,
                "expected `{` after component literal name",
            )?;

            let mut fields = Vec::new();
            if self.peek().kind != TokenKind::RightBrace {
                loop {
                    let field = self.parse_spawn_component_field()?;
                    push_ast(
                        &mut fields,
                        field.name_span,
                        field,
                        "component literal fields",
                    )?;

                    if self.peek().kind != TokenKind::Comma {
                        break;
                    }

                    self.advance();
                }
            }

            let close_span = self.peek().span;
            self.expect(
                TokenKind::RightBrace,
                "expected `}` after component literal fields",
            )?;
            let component = SpawnComponentLiteral {
                name,
                name_span,
                fields,
                span: join_spans(name_span, close_span),
            };
            push_ast(
                &mut components,
                component.name_span,
                component,
                "spawn component literals",
            )?;
        }

        let close_span = self.peek().span;
        self.expect(TokenKind::RightBrace, "expected `}` to close spawn block")?;
        Ok(Statement::Spawn(SpawnStatement {
            components,
            span: join_spans(start, close_span),
        }))
    }

    fn parse_resource_statement(&mut self) -> Result<Statement, ParseError> {
        let start = self.previous_span;
        let (name, name_span) =
            self.parse_identifier_with_span("expected resource literal name after `resource`")?;
        self.expect(
            TokenKind::LeftBrace,
            "expected `{` after resource literal name",
        )?;

        let mut fields = Vec::new();
        if self.peek().kind != TokenKind::RightBrace {
            loop {
                let field = self.parse_resource_literal_field()?;
                push_ast(
                    &mut fields,
                    field.name_span,
                    field,
                    "resource literal fields",
                )?;

                if self.peek().kind != TokenKind::Comma {
                    break;
                }

                self.advance();
            }
        }

        let close_span = self.peek().span;
        self.expect(
            TokenKind::RightBrace,
            "expected `}` after resource literal fields",
        )?;
        Ok(Statement::Resource(ResourceStatement {
            name,
            name_span,
            fields,
            span: join_spans(start, close_span),
        }))
    }

    fn parse_resource_literal_field(&mut self) -> Result<ResourceLiteralField, ParseError> {
        let (name, name_span) =
            self.parse_identifier_with_span("expected resource literal field name")?;
        self.expect(
            TokenKind::Colon,
            "expected `:` after resource literal field name",
        )?;
        let value = self.parse_component_literal_value()?;
        let span = join_spans(name_span, value.span());

        Ok(ResourceLiteralField {
            name,
            name_span,
            value,
            span,
        })
    }

    fn parse_spawn_component_field(&mut self) -> Result<SpawnComponentField, ParseError> {
        let (name, name_span) =
            self.parse_identifier_with_span("expected component literal field name")?;
        self.expect(
            TokenKind::Colon,
            "expected `:` after component literal field name",
        )?;
        let value = self.parse_component_literal_value()?;
        let span = join_spans(name_span, value.span());

        Ok(SpawnComponentField {
            name,
            name_span,
            value,
            span,
        })
    }

    fn parse_component_literal_value(&mut self) -> Result<ComponentLiteralValue, ParseError> {
        let expression = self
            .parse_expression_with_message("expected scalar expression for startup field value")?;
        match expression {
            Expression::Float { text, span } => Ok(ComponentLiteralValue::Float { text, span }),
            Expression::Integer(integer) => Ok(ComponentLiteralValue::Integer {
                value: integer.value,
                span: integer.span,
            }),
            Expression::Bool { value, span } => Ok(ComponentLiteralValue::Bool { value, span }),
            expression => {
                let span = expression.span();
                Ok(ComponentLiteralValue::Expression {
                    expression: Box::new(expression),
                    span,
                })
            }
        }
    }

    fn parse_identifier_with_span(
        &mut self,
        message: &str,
    ) -> Result<(Identifier, Span), ParseError> {
        if !matches!(self.peek().kind, TokenKind::Identifier(_)) {
            return Err(ParseError {
                span: self.peek().span,
                message: message.to_string(),
            });
        }
        let token = self.take_current();
        let TokenKind::Identifier(name) = token.kind else {
            unreachable!("matched identifier token")
        };
        Ok((name, token.span))
    }

    fn parse_type_name(&mut self, message: &str) -> Result<TypeName, ParseError> {
        let (name, span) = self.parse_identifier_with_span(message)?;
        Ok(TypeName { name, span })
    }

    fn parse_expression(&mut self) -> Result<Expression, ParseError> {
        self.parse_expression_with_message("expected expression")
    }

    fn parse_expression_with_message(&mut self, message: &str) -> Result<Expression, ParseError> {
        self.parse_logical_or_expression(message)
    }

    fn parse_logical_or_expression(&mut self, message: &str) -> Result<Expression, ParseError> {
        let left = self.parse_logical_and_expression(message)?;
        self.parse_logical_or_tail(left)
    }

    fn parse_logical_or_tail(&mut self, mut left: Expression) -> Result<Expression, ParseError> {
        while self.peek().kind == TokenKind::Pipe && self.peek_next().kind == TokenKind::Pipe {
            let operator_span = join_spans(self.peek().span, self.peek_next().span);
            self.advance();
            self.advance();
            let right = self.parse_logical_and_expression("expected expression after `||`")?;
            left = make_binary(BinaryOperator::LogicalOr, operator_span, left, right);
        }
        Ok(left)
    }

    fn parse_logical_and_expression(&mut self, message: &str) -> Result<Expression, ParseError> {
        let left = self.parse_bitwise_or_expression(message)?;
        self.parse_logical_and_tail(left)
    }

    fn parse_logical_and_tail(&mut self, mut left: Expression) -> Result<Expression, ParseError> {
        while self.peek().kind == TokenKind::Ampersand
            && self.peek_next().kind == TokenKind::Ampersand
        {
            let operator_span = join_spans(self.peek().span, self.peek_next().span);
            self.advance();
            self.advance();
            let right = self.parse_bitwise_or_expression("expected expression after `&&`")?;
            left = make_binary(BinaryOperator::LogicalAnd, operator_span, left, right);
        }
        Ok(left)
    }

    fn parse_bitwise_or_expression(&mut self, message: &str) -> Result<Expression, ParseError> {
        let mut left = self.parse_bitwise_xor_expression(message)?;
        while self.peek().kind == TokenKind::Pipe && self.peek_next().kind != TokenKind::Pipe {
            let operator_span = self.peek().span;
            self.advance();
            let right = self.parse_bitwise_xor_expression("expected expression after `|`")?;
            left = make_binary(BinaryOperator::BitOr, operator_span, left, right);
        }
        Ok(left)
    }

    fn parse_bitwise_xor_expression(&mut self, message: &str) -> Result<Expression, ParseError> {
        let mut left = self.parse_bitwise_and_expression(message)?;
        while self.peek().kind == TokenKind::Caret {
            let operator_span = self.peek().span;
            self.advance();
            let right = self.parse_bitwise_and_expression("expected expression after `^`")?;
            left = make_binary(BinaryOperator::BitXor, operator_span, left, right);
        }
        Ok(left)
    }

    fn parse_bitwise_and_expression(&mut self, message: &str) -> Result<Expression, ParseError> {
        let mut left = self.parse_equality_expression(message)?;
        while self.peek().kind == TokenKind::Ampersand
            && self.peek_next().kind != TokenKind::Ampersand
        {
            let operator_span = self.peek().span;
            self.advance();
            let right = self.parse_equality_expression("expected expression after `&`")?;
            left = make_binary(BinaryOperator::BitAnd, operator_span, left, right);
        }
        Ok(left)
    }

    fn parse_equality_expression(&mut self, message: &str) -> Result<Expression, ParseError> {
        let left = self.parse_relational_expression(message)?;
        self.parse_equality_tail(left)
    }

    fn parse_equality_tail(&mut self, mut left: Expression) -> Result<Expression, ParseError> {
        loop {
            let operator = if self.peek().kind == TokenKind::Equal
                && self.peek_next().kind == TokenKind::Equal
            {
                BinaryOperator::Equal
            } else if self.peek().kind == TokenKind::Bang
                && self.peek_next().kind == TokenKind::Equal
            {
                BinaryOperator::NotEqual
            } else {
                break;
            };
            let operator_span = join_spans(self.peek().span, self.peek_next().span);
            self.advance();
            self.advance();
            let right = self
                .parse_relational_expression(&format!("expected expression after `{operator}`"))?;
            left = make_binary(operator, operator_span, left, right);
        }
        Ok(left)
    }

    fn parse_relational_expression(&mut self, message: &str) -> Result<Expression, ParseError> {
        let mut left = self.parse_shift_expression(message)?;
        loop {
            let (operator, operator_span, count) = match (&self.peek().kind, &self.peek_next().kind)
            {
                (TokenKind::Less, TokenKind::Equal) => (
                    BinaryOperator::LessEqual,
                    join_spans(self.peek().span, self.peek_next().span),
                    2,
                ),
                (TokenKind::Greater, TokenKind::Equal) => (
                    BinaryOperator::GreaterEqual,
                    join_spans(self.peek().span, self.peek_next().span),
                    2,
                ),
                (TokenKind::Less, _) => (BinaryOperator::Less, self.peek().span, 1),
                (TokenKind::Greater, _) => (BinaryOperator::Greater, self.peek().span, 1),
                _ => break,
            };
            for _ in 0..count {
                self.advance();
            }
            let right =
                self.parse_shift_expression(&format!("expected expression after `{operator}`"))?;
            left = make_binary(operator, operator_span, left, right);
        }
        Ok(left)
    }

    fn parse_shift_expression(&mut self, message: &str) -> Result<Expression, ParseError> {
        let mut left = self.parse_additive_expression(message)?;
        loop {
            let operator = if self.peek().kind == TokenKind::Less
                && self.peek_next().kind == TokenKind::Less
            {
                BinaryOperator::ShiftLeft
            } else if self.peek().kind == TokenKind::Greater
                && self.peek_next().kind == TokenKind::Greater
            {
                BinaryOperator::ShiftRight
            } else {
                break;
            };
            let operator_span = join_spans(self.peek().span, self.peek_next().span);
            self.advance();
            self.advance();
            let right =
                self.parse_additive_expression(&format!("expected expression after `{operator}`"))?;
            left = make_binary(operator, operator_span, left, right);
        }
        Ok(left)
    }

    fn parse_additive_expression(&mut self, message: &str) -> Result<Expression, ParseError> {
        let left = self.parse_multiplicative_expression(message)?;
        self.parse_additive_tail(left)
    }

    fn parse_additive_tail(&mut self, mut left: Expression) -> Result<Expression, ParseError> {
        loop {
            let operator = if self.peek().kind == TokenKind::Plus
                && self.peek_next().kind != TokenKind::Equal
            {
                BinaryOperator::Add
            } else if self.peek().kind == TokenKind::Minus {
                BinaryOperator::Subtract
            } else {
                break;
            };
            let operator_span = self.peek().span;
            self.advance();

            let message = format!("expected expression after `{operator}`");
            let right = self.parse_multiplicative_expression(&message)?;
            left = make_binary(operator, operator_span, left, right);
        }

        Ok(left)
    }

    fn parse_multiplicative_expression(&mut self, message: &str) -> Result<Expression, ParseError> {
        let left = self.parse_unary_expression(message)?;
        self.parse_multiplicative_tail(left)
    }

    fn parse_multiplicative_tail(
        &mut self,
        mut left: Expression,
    ) -> Result<Expression, ParseError> {
        while matches!(
            self.peek().kind,
            TokenKind::Star | TokenKind::Slash | TokenKind::Percent
        ) {
            let operator = match self.peek().kind {
                TokenKind::Star => BinaryOperator::Multiply,
                TokenKind::Slash => BinaryOperator::Divide,
                TokenKind::Percent => BinaryOperator::Remainder,
                _ => unreachable!(),
            };
            let operator_span = self.peek().span;
            self.advance();
            let right =
                self.parse_unary_expression(&format!("expected expression after `{operator}`"))?;
            left = make_binary(operator, operator_span, left, right);
        }

        Ok(left)
    }

    fn parse_field_access_expression(&mut self, message: &str) -> Result<Expression, ParseError> {
        let mut expression = self.parse_primary_expression(message)?;

        while self.peek().kind == TokenKind::Dot {
            self.advance();
            let (field_name, field_span) =
                self.parse_identifier_with_span("expected field name after `.`")?;
            let span = join_spans(expression.span(), field_span);

            expression = Expression::FieldAccess {
                target: Box::new(expression),
                field_name,
                field_span,
                span,
            };
        }

        Ok(expression)
    }

    fn parse_unary_expression(&mut self, message: &str) -> Result<Expression, ParseError> {
        let operator =
            if self.peek().kind == TokenKind::Bang && self.peek_next().kind != TokenKind::Equal {
                Some(UnaryOperator::Not)
            } else if self.peek().kind == TokenKind::Minus {
                Some(UnaryOperator::Negate)
            } else if self.peek().kind == TokenKind::Tilde {
                Some(UnaryOperator::BitNot)
            } else {
                None
            };
        if let Some(operator) = operator {
            let operator_span = self.peek().span;
            self.advance();
            let operand =
                self.parse_unary_expression(&format!("expected expression after `{operator}`"))?;
            let span = join_spans(operator_span, operand.span());
            return Ok(Expression::Unary(UnaryExpression {
                operator,
                operator_span,
                operand: Box::new(operand),
                span,
            }));
        }

        self.parse_field_access_expression(message)
    }

    fn parse_primary_expression(&mut self, message: &str) -> Result<Expression, ParseError> {
        let token = self.peek();
        match &token.kind {
            TokenKind::Integer(_) => self.parse_integer_literal(message).map(Expression::Integer),
            TokenKind::Float(_) => {
                let token = self.take_current();
                let TokenKind::Float(text) = token.kind else {
                    unreachable!("matched float token")
                };
                Ok(Expression::Float {
                    text,
                    span: token.span,
                })
            }
            TokenKind::Keyword(Keyword::True) => {
                let span = token.span;
                self.advance();
                Ok(Expression::Bool { value: true, span })
            }
            TokenKind::Keyword(Keyword::False) => {
                let span = token.span;
                self.advance();
                Ok(Expression::Bool { value: false, span })
            }
            TokenKind::LeftParen => {
                let open_span = token.span;
                self.advance();
                let expression =
                    self.parse_expression_with_message("expected expression after `(`")?;
                let close_span = self.peek().span;
                self.expect(
                    TokenKind::RightParen,
                    "expected `)` after parenthesized expression",
                )?;
                Ok(Expression::Parenthesized {
                    expression: Box::new(expression),
                    span: join_spans(open_span, close_span),
                })
            }
            TokenKind::Identifier(_) => {
                let (name, span) = self.parse_identifier_with_span(message)?;
                let expression = Expression::Identifier { name, span };
                Ok(expression)
            }
            _ => Err(ParseError {
                span: token.span,
                message: message.to_string(),
            }),
        }
    }

    fn parse_integer_literal(&mut self, message: &str) -> Result<IntegerLiteral, ParseError> {
        let token = self.peek();
        let span = token.span;
        let text = match &token.kind {
            TokenKind::Integer(text) => text,
            _ => {
                return Err(ParseError {
                    span,
                    message: message.to_string(),
                })
            }
        };

        let value = match text.parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                return Err(ParseError {
                    span,
                    message: "integer literal is too large".to_string(),
                })
            }
        };
        self.advance();

        Ok(IntegerLiteral { value, span })
    }

    fn match_keyword(&mut self, keyword: Keyword) -> bool {
        if self.peek().kind == TokenKind::Keyword(keyword) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: TokenKind, message: &str) -> Result<(), ParseError> {
        if self.peek().kind == expected {
            self.advance();
            Ok(())
        } else {
            Err(ParseError {
                span: self.peek().span,
                message: message.to_string(),
            })
        }
    }

    fn expect_eof(&mut self) -> Result<(), ParseError> {
        if self.peek().kind == TokenKind::Eof {
            Ok(())
        } else {
            Err(ParseError {
                span: self.peek().span,
                message: "expected end of file".to_string(),
            })
        }
    }

    fn peek(&self) -> &Token {
        &self.current
    }

    fn peek_next(&self) -> &Token {
        &self.next
    }

    fn advance(&mut self) {
        drop(self.take_current());
    }

    fn take_current(&mut self) -> Token {
        let eof_position = self.next.span.end;
        let placeholder = eof_token(eof_position);
        let next = std::mem::replace(&mut self.next, placeholder);
        let current = std::mem::replace(&mut self.current, next);
        self.previous_span = current.span;

        if self.source_error.is_none() {
            match self.source.next_token() {
                Ok(token) => self.next = token,
                Err(error) => {
                    let position = match &error {
                        LexerFailure::Lex(error) => error.span.start,
                        LexerFailure::Read(_) => eof_position,
                    };
                    self.next = eof_token(position);
                    self.source_error = Some(error);
                }
            }
        }

        current
    }
}

fn eof_token(position: crate::source_snapshot::SourcePosition) -> Token {
    Token {
        kind: TokenKind::Eof,
        span: Span {
            start: position,
            end: position,
        },
    }
}

fn join_spans(start: Span, end: Span) -> Span {
    Span {
        start: start.start,
        end: end.end,
    }
}

fn make_binary(
    operator: BinaryOperator,
    operator_span: Span,
    left: Expression,
    right: Expression,
) -> Expression {
    let span = join_spans(left.span(), right.span());
    Expression::Binary(BinaryExpression {
        operator,
        operator_span,
        left: Box::new(left),
        right: Box::new(right),
        span,
    })
}

fn push_ast<T>(
    values: &mut Vec<T>,
    span: Span,
    value: T,
    context: &'static str,
) -> Result<(), ParseError> {
    values.try_reserve(1).map_err(|error| ParseError {
        span,
        message: format!("could not allocate {context}: {error}"),
    })?;
    values.push(value);
    Ok(())
}

impl fmt::Display for Program {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Program")?;
        write!(formatter, "  world {}", self.world.name)?;

        for tag in &self.tags {
            writeln!(formatter)?;
            write!(formatter, "  tag {}", tag.name)?;
        }

        for component in &self.components {
            writeln!(formatter)?;
            write!(formatter, "  component {}", component.name)?;

            for field in &component.fields {
                writeln!(formatter)?;
                write!(
                    formatter,
                    "    field {}: {}",
                    field.name, field.type_name.name
                )?;
            }
        }

        for resource in &self.resources {
            writeln!(formatter)?;
            write!(formatter, "  resource {}", resource.name)?;

            for field in &resource.fields {
                writeln!(formatter)?;
                write!(
                    formatter,
                    "    field {}: {}",
                    field.name, field.type_name.name
                )?;
            }
        }

        for system in &self.systems {
            writeln!(formatter)?;
            writeln!(formatter, "  system {}", system.name)?;
            if system.params.is_empty() {
                writeln!(formatter, "    params 0")?;
            } else {
                for param in &system.params {
                    write_system_param(formatter, param)?;
                    writeln!(formatter)?;
                }
            }
            if system.body.statements.is_empty() {
                write!(formatter, "    body empty")?;
            } else {
                write!(formatter, "    body")?;
                for statement in &system.body.statements {
                    write_system_body_statement(formatter, statement, "      ")?;
                }
            }
        }

        for schedule in &self.schedules {
            writeln!(formatter)?;
            write!(formatter, "  schedule {}", schedule.name)?;

            for item in &schedule.items {
                match item {
                    ScheduleItem::Run { system_name, .. } => {
                        writeln!(formatter)?;
                        write!(formatter, "    run {system_name}")?;
                    }
                }
            }
        }

        for startup in &self.startups {
            writeln!(formatter)?;
            write!(formatter, "  startup")?;

            for statement in &startup.statements {
                match statement {
                    Statement::Let(let_statement) => {
                        writeln!(formatter)?;
                        writeln!(
                            formatter,
                            "    let {}{}: {}",
                            if let_statement.mutable { "mut " } else { "" },
                            let_statement.name,
                            let_statement.type_name.name
                        )?;
                        write_expression(formatter, &let_statement.initializer, "      ")?;
                    }
                    Statement::Assign(assignment) => {
                        writeln!(formatter)?;
                        writeln!(formatter, "    assign")?;
                        writeln!(formatter, "      target")?;
                        write_expression(formatter, &assignment.target, "        ")?;
                        writeln!(formatter)?;
                        writeln!(formatter, "      value")?;
                        write_expression(formatter, &assignment.value, "        ")?;
                    }
                    Statement::AddAssign(add_assign) => {
                        writeln!(formatter)?;
                        writeln!(formatter, "    add_assign")?;
                        writeln!(formatter, "      target")?;
                        write_expression(formatter, &add_assign.target, "        ")?;
                        writeln!(formatter)?;
                        writeln!(formatter, "      value")?;
                        write_expression(formatter, &add_assign.value, "        ")?;
                    }
                    Statement::Run(run) => {
                        writeln!(formatter)?;
                        write!(formatter, "    run {}", run.schedule_name)?;
                    }
                    Statement::Spawn(spawn) => {
                        writeln!(formatter)?;
                        write!(formatter, "    spawn")?;

                        for component in &spawn.components {
                            writeln!(formatter)?;
                            write!(formatter, "      component {}", component.name)?;

                            for field in &component.fields {
                                writeln!(formatter)?;
                                writeln!(formatter, "        field {}", field.name)?;
                                write_component_literal_value(
                                    formatter,
                                    &field.value,
                                    "          ",
                                )?;
                            }
                        }
                    }
                    Statement::Resource(resource) => {
                        writeln!(formatter)?;
                        write!(formatter, "    resource {}", resource.name)?;

                        for field in &resource.fields {
                            writeln!(formatter)?;
                            writeln!(formatter, "      field {}", field.name)?;
                            write_component_literal_value(formatter, &field.value, "        ")?;
                        }
                    }
                    Statement::Exit(exit) => {
                        writeln!(formatter)?;
                        writeln!(formatter, "    exit")?;
                        write_expression(formatter, &exit.expression, "      ")?;
                    }
                }
            }
        }

        Ok(())
    }
}

pub fn write_program(output: &mut impl io::Write, program: &Program) -> io::Result<()> {
    output.write_fmt(format_args!("{program}"))
}

impl fmt::Display for Statement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Let(let_statement) => write!(
                formatter,
                "let {}{}: {} = {}",
                if let_statement.mutable { "mut " } else { "" },
                let_statement.name,
                let_statement.type_name.name,
                let_statement.initializer
            ),
            Self::Assign(assignment) => {
                write!(formatter, "{} = {}", assignment.target, assignment.value)
            }
            Self::AddAssign(add_assign) => {
                write!(formatter, "{} += {}", add_assign.target, add_assign.value)
            }
            Self::Run(run) => write!(formatter, "run {}", run.schedule_name),
            Self::Spawn(spawn) => {
                write!(formatter, "spawn {{")?;
                for component in &spawn.components {
                    write!(formatter, " {} {{", component.name)?;
                    for (index, field) in component.fields.iter().enumerate() {
                        if index > 0 {
                            formatter.write_str(",")?;
                        }
                        write!(formatter, " {}: {}", field.name, field.value)?;
                    }
                    formatter.write_str(" }")?;
                }
                formatter.write_str(" }")
            }
            Self::Resource(resource) => {
                write!(formatter, "resource {} {{", resource.name)?;
                for (index, field) in resource.fields.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(",")?;
                    }
                    write!(formatter, " {}: {}", field.name, field.value)?;
                }
                formatter.write_str(" }")
            }
            Self::Exit(exit) => write!(formatter, "exit {}", exit.expression),
        }
    }
}

impl fmt::Display for Expression {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(integer) => write!(formatter, "{}", integer.value),
            Self::Float { text, .. } => formatter.write_str(text),
            Self::Bool { value, .. } => write!(formatter, "{value}"),
            Self::Identifier { name, .. } => formatter.write_str(name),
            Self::FieldAccess {
                target, field_name, ..
            } => {
                write!(formatter, "{target}.{field_name}")
            }
            Self::Unary(unary) => write!(formatter, "{}{}", unary.operator, unary.operand),
            Self::Binary(binary) => write!(
                formatter,
                "{} {} {}",
                binary.left, binary.operator, binary.right
            ),
            Self::Parenthesized { expression, .. } => write!(formatter, "({expression})"),
        }
    }
}

impl fmt::Display for BinaryOperator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Add => formatter.write_str("+"),
            Self::Subtract => formatter.write_str("-"),
            Self::Multiply => formatter.write_str("*"),
            Self::Divide => formatter.write_str("/"),
            Self::Remainder => formatter.write_str("%"),
            Self::ShiftLeft => formatter.write_str("<<"),
            Self::ShiftRight => formatter.write_str(">>"),
            Self::Less => formatter.write_str("<"),
            Self::LessEqual => formatter.write_str("<="),
            Self::Greater => formatter.write_str(">"),
            Self::GreaterEqual => formatter.write_str(">="),
            Self::Equal => formatter.write_str("=="),
            Self::NotEqual => formatter.write_str("!="),
            Self::BitAnd => formatter.write_str("&"),
            Self::BitXor => formatter.write_str("^"),
            Self::BitOr => formatter.write_str("|"),
            Self::LogicalAnd => formatter.write_str("&&"),
            Self::LogicalOr => formatter.write_str("||"),
        }
    }
}

impl fmt::Display for UnaryOperator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Not => formatter.write_str("!"),
            Self::Negate => formatter.write_str("-"),
            Self::BitNot => formatter.write_str("~"),
        }
    }
}

impl fmt::Display for ComponentLiteralValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Float { text, .. } => formatter.write_str(text),
            Self::Integer { value, .. } => write!(formatter, "{value}"),
            Self::Bool { value, .. } => write!(formatter, "{value}"),
            Self::Expression { expression, .. } => write!(formatter, "{expression}"),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at byte {}",
            self.message, self.span.start.byte
        )
    }
}

fn write_component_literal_value(
    formatter: &mut fmt::Formatter<'_>,
    value: &ComponentLiteralValue,
    indent: &str,
) -> fmt::Result {
    match value {
        ComponentLiteralValue::Float { text, .. } => write!(formatter, "{indent}float {text}"),
        ComponentLiteralValue::Integer { value, .. } => {
            write!(formatter, "{indent}integer {value}")
        }
        ComponentLiteralValue::Bool { value, .. } => {
            write!(formatter, "{indent}bool {value}")
        }
        ComponentLiteralValue::Expression { expression, .. } => {
            write_expression(formatter, expression, indent)
        }
    }
}

fn write_system_param(formatter: &mut fmt::Formatter<'_>, param: &SystemParam) -> fmt::Result {
    match &param.kind {
        SystemParamKind::ReadResource { resource_name, .. } => {
            write!(
                formatter,
                "    param {}: read {}",
                param.name, resource_name
            )
        }
        SystemParamKind::MutResource { resource_name, .. } => {
            write!(formatter, "    param {}: mut {}", param.name, resource_name)
        }
        SystemParamKind::Query { terms } => {
            writeln!(formatter, "    param {}: query", param.name)?;
            for (index, term) in terms.iter().enumerate() {
                if index > 0 {
                    writeln!(formatter)?;
                }
                write!(
                    formatter,
                    "      {} {}",
                    format_query_access(term.access),
                    term.component_name
                )?;
            }
            Ok(())
        }
    }
}

fn format_query_access(access: QueryAccess) -> &'static str {
    match access {
        QueryAccess::Read => "read",
        QueryAccess::Mut => "mut",
        QueryAccess::Exclude => "exclude",
    }
}

fn write_system_body_statement(
    formatter: &mut fmt::Formatter<'_>,
    statement: &SystemBodyStatement,
    indent: &str,
) -> fmt::Result {
    match statement {
        SystemBodyStatement::Expression(expression) => {
            writeln!(formatter)?;
            writeln!(formatter, "{indent}expr")?;
            write_expression(formatter, expression, &format!("{indent}  "))
        }
        SystemBodyStatement::QueryLoop(query_loop) => {
            writeln!(formatter)?;
            writeln!(formatter, "{indent}for")?;
            writeln!(formatter, "{indent}  bindings")?;
            for binding in &query_loop.bindings {
                writeln!(formatter, "{indent}    binding {}", binding.name)?;
            }
            writeln!(formatter, "{indent}  in {}", query_loop.query_param)?;

            if query_loop.body.is_empty() {
                write!(formatter, "{indent}  body empty")
            } else {
                write!(formatter, "{indent}  body")?;
                for statement in &query_loop.body {
                    write_system_body_statement(formatter, statement, &format!("{indent}    "))?;
                }
                Ok(())
            }
        }
        SystemBodyStatement::Let(let_statement) => {
            writeln!(formatter)?;
            writeln!(
                formatter,
                "{indent}let {}{}: {}",
                if let_statement.mutable { "mut " } else { "" },
                let_statement.name,
                let_statement.type_name.name
            )?;
            write_expression(
                formatter,
                &let_statement.initializer,
                &format!("{indent}  "),
            )
        }
        SystemBodyStatement::Assign(assignment) => {
            writeln!(formatter)?;
            writeln!(formatter, "{indent}assign")?;
            writeln!(formatter, "{indent}  target")?;
            write_expression(formatter, &assignment.target, &format!("{indent}    "))?;
            writeln!(formatter)?;
            writeln!(formatter, "{indent}  value")?;
            write_expression(formatter, &assignment.value, &format!("{indent}    "))
        }
        SystemBodyStatement::AddAssign(add_assign) => {
            writeln!(formatter)?;
            writeln!(formatter, "{indent}add_assign")?;
            writeln!(formatter, "{indent}  target")?;
            write_expression(formatter, &add_assign.target, &format!("{indent}    "))?;
            writeln!(formatter)?;
            writeln!(formatter, "{indent}  value")?;
            write_expression(formatter, &add_assign.value, &format!("{indent}    "))
        }
        SystemBodyStatement::Block(block) => {
            writeln!(formatter)?;
            write!(formatter, "{indent}block")?;
            for statement in &block.statements {
                write_system_body_statement(formatter, statement, &format!("{indent}  "))?;
            }
            Ok(())
        }
        SystemBodyStatement::If(statement) => {
            writeln!(formatter)?;
            writeln!(formatter, "{indent}if")?;
            write_expression(formatter, &statement.condition, &format!("{indent}  "))?;
            for child in &statement.then_block.statements {
                write_system_body_statement(formatter, child, &format!("{indent}  "))?;
            }
            if let Some(block) = &statement.else_block {
                writeln!(formatter)?;
                write!(formatter, "{indent}else")?;
                for child in &block.statements {
                    write_system_body_statement(formatter, child, &format!("{indent}  "))?;
                }
            }
            Ok(())
        }
        SystemBodyStatement::While(statement) => {
            writeln!(formatter)?;
            writeln!(formatter, "{indent}while")?;
            write_expression(formatter, &statement.condition, &format!("{indent}  "))?;
            for child in &statement.body.statements {
                write_system_body_statement(formatter, child, &format!("{indent}  "))?;
            }
            Ok(())
        }
    }
}

fn write_expression(
    formatter: &mut fmt::Formatter<'_>,
    expression: &Expression,
    indent: &str,
) -> fmt::Result {
    match expression {
        Expression::Integer(integer) => write!(formatter, "{indent}integer {}", integer.value),
        Expression::Float { text, .. } => write!(formatter, "{indent}float {text}"),
        Expression::Bool { value, .. } => write!(formatter, "{indent}bool {value}"),
        Expression::Identifier { name, .. } => write!(formatter, "{indent}identifier {name}"),
        Expression::FieldAccess {
            target, field_name, ..
        } => {
            writeln!(formatter, "{indent}field {field_name}")?;
            write_expression(formatter, target, &format!("{indent}  "))
        }
        Expression::Unary(unary) => {
            writeln!(formatter, "{indent}unary {}", unary.operator)?;
            write_expression(formatter, &unary.operand, &format!("{indent}  "))
        }
        Expression::Binary(binary) => {
            writeln!(formatter, "{indent}binary {}", binary.operator)?;
            write_expression(formatter, &binary.left, &format!("{indent}  "))?;
            writeln!(formatter)?;
            write_expression(formatter, &binary.right, &format!("{indent}  "))
        }
        Expression::Parenthesized { expression, .. } => {
            writeln!(formatter, "{indent}parenthesized")?;
            write_expression(formatter, expression, &format!("{indent}  "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;

    struct FailingWriter;

    impl io::Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "injected AST-output failure",
            ))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn ast_formatter_propagates_output_failures() {
        let tokens = lexer::lex("world Main startup { exit 0 }").expect("fixture lexes");
        let program = parse_program(&tokens).expect("fixture parses");
        let error = write_program(&mut FailingWriter, &program).expect_err("writer must fail");

        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    fn parse_startup_initializer(source: &str) -> Expression {
        let tokens = lexer::lex(source).expect("fixture lexes");
        let program = parse_program(&tokens).expect("fixture parses");
        let startup = program
            .startups
            .into_iter()
            .next()
            .expect("fixture has startup");
        let Statement::Let(statement) = &startup.statements[0] else {
            panic!("first statement should be a let");
        };
        statement.initializer.clone()
    }

    fn integer(expression: &Expression) -> u64 {
        let Expression::Integer(integer) = expression else {
            panic!("expected integer expression, got {expression:?}");
        };
        integer.value
    }

    #[test]
    fn boolean_precedence_preserves_equality_not_and_or_order() {
        let expression = parse_startup_initializer(
            "world Main startup { let ready: bool = !false == true && false || true exit 0 }",
        );

        let Expression::Binary(or) = expression else {
            panic!("outer expression should be logical or");
        };
        assert_eq!(or.operator, BinaryOperator::LogicalOr);
        let Expression::Binary(and) = &*or.left else {
            panic!("left side should be logical and");
        };
        assert_eq!(and.operator, BinaryOperator::LogicalAnd);
        let Expression::Binary(equal) = &*and.left else {
            panic!("logical and should contain equality");
        };
        assert_eq!(equal.operator, BinaryOperator::Equal);
        assert!(matches!(&*equal.left, Expression::Unary(_)));
    }

    #[test]
    fn mutable_let_and_direct_assignment_parse_in_startup_and_systems() {
        let source = "world Main
system Toggle() { let mut ready: bool = true ready = !ready }
startup { let mut ready: bool = false ready = true exit 0 }";
        let tokens = lexer::lex(source).expect("fixture lexes");
        let program = parse_program(&tokens).expect("fixture parses");

        let Statement::Let(startup_let) = &program.startups[0].statements[0] else {
            panic!("expected startup let");
        };
        assert!(startup_let.mutable);
        assert!(matches!(
            program.startups[0].statements[1],
            Statement::Assign(_)
        ));
        assert!(matches!(
            program.systems[0].body.statements.as_slice(),
            [SystemBodyStatement::Let(_), SystemBodyStatement::Assign(_)]
        ));
    }

    #[test]
    fn startup_ast_formatter_preserves_mutable_local_qualifier() {
        let source = "world Main startup { let mut value: i32 = 1 exit value }";
        let tokens = lexer::lex(source).expect("fixture lexes");
        let program = parse_program(&tokens).expect("fixture parses");

        assert_eq!(
            program.to_string(),
            "Program\n  world Main\n  startup\n    let mut value: i32\n      integer 1\n    exit\n      identifier value"
        );
    }

    #[test]
    fn add_assign_parses_structurally_in_startup_and_systems() {
        let source = "world Main
system Accumulate() { let mut value: f32 = 0.5 value += 0.25 }
startup { let mut value: i32 = 40 value += 2 exit value }";
        let tokens = lexer::lex(source).expect("fixture lexes");
        let program = parse_program(&tokens).expect("fixture parses");

        let Statement::AddAssign(startup_add) = &program.startups[0].statements[1] else {
            panic!("expected startup add-assign");
        };
        assert!(matches!(
            &startup_add.target,
            Expression::Identifier { name, .. } if name == "value"
        ));
        assert!(matches!(&startup_add.value, Expression::Integer(_)));
        assert_eq!(program.startups[0].statements[1].to_string(), "value += 2");

        let SystemBodyStatement::AddAssign(system_add) = &program.systems[0].body.statements[1]
        else {
            panic!("expected system add-assign");
        };
        assert!(matches!(&system_add.value, Expression::Float { .. }));
    }

    #[test]
    fn arithmetic_precedence_and_associativity_are_structural() {
        let expression = parse_startup_initializer(
            "world Main startup { let x: i32 = 20 - 5 - 3 + 2 * 4 exit x }",
        );

        let Expression::Binary(add) = expression else {
            panic!("outer expression should be addition");
        };
        assert_eq!(add.operator, BinaryOperator::Add);

        let Expression::Binary(second_subtract) = &*add.left else {
            panic!("left side should preserve left-associative subtraction");
        };
        assert_eq!(second_subtract.operator, BinaryOperator::Subtract);
        assert_eq!(integer(&second_subtract.right), 3);

        let Expression::Binary(first_subtract) = &*second_subtract.left else {
            panic!("first subtraction should be nested on the left");
        };
        assert_eq!(first_subtract.operator, BinaryOperator::Subtract);
        assert_eq!(integer(&first_subtract.left), 20);
        assert_eq!(integer(&first_subtract.right), 5);

        let Expression::Binary(multiply) = &*add.right else {
            panic!("multiplication should bind tighter than addition");
        };
        assert_eq!(multiply.operator, BinaryOperator::Multiply);
        assert_eq!(integer(&multiply.left), 2);
        assert_eq!(integer(&multiply.right), 4);
    }

    #[test]
    fn parentheses_override_arithmetic_precedence() {
        let expression =
            parse_startup_initializer("world Main startup { let x: i32 = (1 + 2) * 3 exit x }");

        let Expression::Binary(multiply) = expression else {
            panic!("outer expression should be multiplication");
        };
        assert_eq!(multiply.operator, BinaryOperator::Multiply);
        assert_eq!(integer(&multiply.right), 3);

        let Expression::Parenthesized {
            expression: parenthesized,
            ..
        } = &*multiply.left
        else {
            panic!("explicit parentheses should remain in the AST");
        };
        let Expression::Binary(add) = &**parenthesized else {
            panic!("parenthesized addition should remain nested");
        };
        assert_eq!(add.operator, BinaryOperator::Add);
        assert_eq!(integer(&add.left), 1);
        assert_eq!(integer(&add.right), 2);
    }

    #[test]
    fn reports_a_missing_parenthesis_at_the_stable_following_token() {
        let source = "world Main startup { let x: i32 = (1 + 2 exit x }";
        let tokens = lexer::lex(source).expect("fixture lexes");
        let error = parse_program(&tokens).expect_err("fixture should fail");

        assert_eq!(error.message, "expected `)` after parenthesized expression");
        let start = usize::try_from(error.span.start.byte).unwrap();
        let end = usize::try_from(error.span.end.byte).unwrap();
        assert_eq!(&source[start..end], "exit");
        assert_eq!((error.span.start.line, error.span.start.column), (1, 42));
    }

    #[test]
    fn reports_a_missing_closing_brace_at_captured_eof() {
        let source = "world Main\r\nstartup {\r\n  exit 0";
        let tokens = lexer::lex(source).expect("fixture lexes");
        let error = parse_program(&tokens).expect_err("missing brace must fail");

        assert_eq!(error.message, "expected `}` to close startup block");
        assert_eq!(error.span.start, error.span.end);
        assert_eq!(error.span.start.byte, u64::try_from(source.len()).unwrap());
        assert_eq!((error.span.start.line, error.span.start.column), (3, 9));
    }

    #[test]
    fn reports_an_incomplete_startup_literal_at_captured_eof() {
        let source = "world Main\ncomponent Value { x: i32 }\nstartup { spawn { Value { x:";
        let tokens = lexer::lex(source).expect("fixture lexes");
        let error = parse_program(&tokens).expect_err("incomplete literal must fail");

        assert_eq!(
            error.message,
            "expected scalar expression for startup field value"
        );
        assert_eq!(error.span.start, error.span.end);
        assert_eq!(error.span.start.byte, u64::try_from(source.len()).unwrap());
        assert_eq!((error.span.start.line, error.span.start.column), (3, 29));
    }

    #[test]
    fn startup_literal_spans_include_their_actual_closing_braces() {
        let source = "world Main
component Value { x: i32 }
resource State { x: i32 }
startup {
  resource State {
    x: 1
  }
  spawn {
    Value {
      x: 2
    }
  }
  exit 0
}";
        let tokens = lexer::lex(source).expect("fixture lexes");
        let program = parse_program(&tokens).expect("fixture parses");
        let startup = program.startups.into_iter().next().expect("startup exists");
        let Statement::Resource(resource) = &startup.statements[0] else {
            panic!("first statement is the resource literal");
        };
        let Statement::Spawn(spawn) = &startup.statements[1] else {
            panic!("second statement is the spawn literal");
        };

        let resource_start = source.find("resource State {\n").unwrap();
        let resource_end =
            source[resource_start..].find("\n  }").unwrap() + resource_start + "\n  }".len();
        assert_eq!(
            (resource.span.start.byte, resource.span.end.byte),
            (
                u64::try_from(resource_start).unwrap(),
                u64::try_from(resource_end).unwrap(),
            )
        );

        let component_start = source.find("Value {\n      x: 2").unwrap();
        let component_end =
            source[component_start..].find("\n    }").unwrap() + component_start + "\n    }".len();
        assert_eq!(
            (
                spawn.components[0].span.start.byte,
                spawn.components[0].span.end.byte,
            ),
            (
                u64::try_from(component_start).unwrap(),
                u64::try_from(component_end).unwrap(),
            )
        );
    }

    #[test]
    fn retains_multiple_startups_and_items_after_startup_for_semantic_modes() {
        let source = "world Main startup { exit 0 }\ncomponent Later {}\nstartup { exit 1 }";
        let tokens = lexer::lex(source).expect("fixture lexes");
        let program = parse_program(&tokens).expect("syntax-only parsing retains every item");

        let second = source
            .rfind("startup")
            .expect("fixture has two startup blocks");
        assert_eq!(program.startups.len(), 2);
        assert_eq!(program.components[0].name, "Later");
        assert_eq!(
            program.startups[1].keyword_span.start.byte,
            u64::try_from(second).unwrap()
        );
        assert_eq!(
            program.startups[1].keyword_span.end.byte,
            u64::try_from(second + "startup".len()).unwrap()
        );
        assert_eq!(
            (
                program.startups[1].keyword_span.start.line,
                program.startups[1].keyword_span.start.column
            ),
            (3, 1)
        );

        let ast = program.to_string();
        assert_eq!(ast.matches("\n  startup").count(), 2);
        assert!(ast.contains("component Later"));
    }

    #[test]
    fn streaming_parser_uses_lexer_tokens_without_a_collected_vector() {
        let lexer = lexer::Lexer::new(std::io::BufReader::with_capacity(
            2,
            std::io::Cursor::new(b"world Main startup { exit 0 }"),
        ));
        let program = parse_lexer(lexer).expect("streamed fixture parses");

        assert_eq!(program.world.name, "Main");
        assert!(matches!(
            program.startups[0].statements.as_slice(),
            [Statement::Exit(_)]
        ));
    }

    #[test]
    fn ast_identifier_occurrences_share_lexer_storage() {
        let source = "world Demo
component Position { x: i32 }
system Move(items: query[mut Position]) {
  for (pos) in items { pos.x += 1 }
}
startup { spawn { Position { x: 0 } } exit 0 }";
        let tokens = lexer::lex(source).expect("fixture lexes");
        let program = parse_program(&tokens).expect("fixture parses");

        let component = &program.components[0];
        let SystemParamKind::Query { terms } = &program.systems[0].params[0].kind else {
            panic!("parameter is a query");
        };
        let SystemBodyStatement::QueryLoop(query_loop) = &program.systems[0].body.statements[0]
        else {
            panic!("system body starts with a query loop");
        };
        let SystemBodyStatement::AddAssign(add) = &query_loop.body[0] else {
            panic!("query body contains +=");
        };
        let Expression::FieldAccess {
            target, field_name, ..
        } = &add.target
        else {
            panic!("+= target is a field");
        };
        let Expression::Identifier {
            name: target_name, ..
        } = &**target
        else {
            panic!("field target is a binding");
        };
        let startup = &program.startups[0];
        let Statement::Spawn(spawn) = &startup.statements[0] else {
            panic!("startup begins with spawn");
        };

        assert!(component.name.shares_storage_with(&terms[0].component_name));
        assert!(component
            .name
            .shares_storage_with(&spawn.components[0].name));
        assert!(component.fields[0].name.shares_storage_with(field_name));
        assert!(component.fields[0]
            .name
            .shares_storage_with(&spawn.components[0].fields[0].name));
        assert!(program.systems[0].params[0]
            .name
            .shares_storage_with(&query_loop.query_param));
        assert!(query_loop.bindings[0].name.shares_storage_with(target_name));
    }

    #[test]
    fn parses_exclusion_only_query_loop_without_bindings() {
        let source = "world Demo
component Hidden {}
system Visit(items: query[!Hidden]) { for () in items {} }
startup { exit 0 }";
        let tokens = lexer::lex(source).expect("exclusion-only query fixture lexes");
        let program = parse_program(&tokens).expect("an empty query binding list is valid");
        let SystemBodyStatement::QueryLoop(query_loop) = &program.systems[0].body.statements[0]
        else {
            panic!("system body starts with a query loop");
        };

        assert!(query_loop.bindings.is_empty());
        assert_eq!(query_loop.query_param, "items");
    }

    #[test]
    fn parses_tags_and_zero_data_literals() {
        let source = "world Demo
tag Enemy
component Empty {}
resource Ready {}
startup {
  resource Ready {}
  spawn {}
  spawn { Enemy {} Empty {} }
  exit 0
}";
        let tokens = lexer::lex(source).expect("zero-data fixture lexes");
        let program = parse_program(&tokens).expect("tags and zero-data literals should parse");
        assert_eq!(program.tags.len(), 1);
        assert_eq!(program.tags[0].name, "Enemy");
        assert!(program.components[0].fields.is_empty());
        assert!(program.resources[0].fields.is_empty());
        let startup = program.startups.into_iter().next().expect("startup parses");
        assert!(matches!(
            startup.statements[1],
            Statement::Spawn(SpawnStatement {
                ref components,
                ..
            }) if components.is_empty()
        ));
    }
}
