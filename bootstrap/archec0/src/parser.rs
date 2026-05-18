use std::fmt;

use crate::lexer::{Keyword, Span, Token, TokenKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Program {
    pub world: WorldDecl,
    pub components: Vec<ComponentDecl>,
    pub resources: Vec<ResourceDecl>,
    pub systems: Vec<SystemDecl>,
    pub startup: Option<StartupBlock>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldDecl {
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentDecl {
    pub name: String,
    pub fields: Vec<ComponentField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentField {
    pub name: String,
    pub type_name: TypeName,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceDecl {
    pub name: String,
    pub fields: Vec<ResourceField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceField {
    pub name: String,
    pub type_name: TypeName,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemDecl {
    pub name: String,
    pub params: Vec<SystemParam>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemParam {
    pub name: String,
    pub kind: SystemParamKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SystemParamKind {
    ReadResource { resource_name: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupBlock {
    pub statements: Vec<Statement>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Statement {
    Let(LetStatement),
    Spawn(SpawnStatement),
    Resource(ResourceStatement),
    Exit(ExitStatement),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LetStatement {
    pub name: String,
    pub type_name: TypeName,
    pub initializer: Expression,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpawnStatement {
    pub components: Vec<SpawnComponentLiteral>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpawnComponentLiteral {
    pub name: String,
    pub fields: Vec<SpawnComponentField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpawnComponentField {
    pub name: String,
    pub value: ComponentLiteralValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceStatement {
    pub name: String,
    pub fields: Vec<ResourceLiteralField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceLiteralField {
    pub name: String,
    pub value: ComponentLiteralValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComponentLiteralValue {
    Float { text: String, span: Span },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeName {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitStatement {
    pub expression: Expression,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Expression {
    Integer(IntegerLiteral),
    Identifier { name: String, span: Span },
    Binary(BinaryExpression),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BinaryExpression {
    pub operator: BinaryOperator,
    pub left: Box<Expression>,
    pub right: Box<Expression>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
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

pub fn parse_program(tokens: &[Token]) -> Result<Program, ParseError> {
    let mut parser = Parser { tokens, current: 0 };
    let world = parser.parse_world_declaration()?;
    let mut components = Vec::new();
    let mut resources = Vec::new();
    let mut systems = Vec::new();
    loop {
        if parser.match_keyword(Keyword::Component) {
            components.push(parser.parse_component_declaration()?);
            continue;
        }

        if parser.match_keyword(Keyword::Resource) {
            resources.push(parser.parse_resource_declaration()?);
            continue;
        }

        if parser.match_keyword(Keyword::System) {
            systems.push(parser.parse_system_declaration()?);
            continue;
        }

        break;
    }
    let startup = if parser.match_keyword(Keyword::Startup) {
        Some(parser.parse_startup_block()?)
    } else {
        None
    };
    parser.expect_eof()?;

    Ok(Program {
        world,
        components,
        resources,
        systems,
        startup,
    })
}

struct Parser<'a> {
    tokens: &'a [Token],
    current: usize,
}

impl Parser<'_> {
    fn parse_world_declaration(&mut self) -> Result<WorldDecl, ParseError> {
        let world_token = self.peek();

        if world_token.kind != TokenKind::Keyword(Keyword::World) {
            return Err(ParseError {
                span: world_token.span,
                message: "expected `world` declaration".to_string(),
            });
        }
        self.advance();

        let name_token = self.peek();
        let name = match &name_token.kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(ParseError {
                    span: name_token.span,
                    message: "expected world name after `world`".to_string(),
                })
            }
        };
        self.advance();

        Ok(WorldDecl { name })
    }

    fn parse_component_declaration(&mut self) -> Result<ComponentDecl, ParseError> {
        let name = self.parse_identifier("expected component name after `component`")?;
        self.expect(TokenKind::LeftBrace, "expected `{` after component name")?;

        let mut fields = Vec::new();
        while self.peek().kind != TokenKind::RightBrace {
            if self.peek().kind == TokenKind::Eof {
                return Err(ParseError {
                    span: self.peek().span,
                    message: "expected `}` to close component declaration".to_string(),
                });
            }

            let name = self.parse_identifier("expected component field name")?;
            self.expect(TokenKind::Colon, "expected `:` after component field name")?;
            let type_name = self.parse_type_name("expected component field type after `:`")?;
            fields.push(ComponentField { name, type_name });
        }

        self.expect(
            TokenKind::RightBrace,
            "expected `}` to close component declaration",
        )?;
        Ok(ComponentDecl { name, fields })
    }

    fn parse_resource_declaration(&mut self) -> Result<ResourceDecl, ParseError> {
        let name = self.parse_identifier("expected resource name after `resource`")?;
        self.expect(TokenKind::LeftBrace, "expected `{` after resource name")?;

        let mut fields = Vec::new();
        while self.peek().kind != TokenKind::RightBrace {
            if self.peek().kind == TokenKind::Eof {
                return Err(ParseError {
                    span: self.peek().span,
                    message: "expected `}` to close resource declaration".to_string(),
                });
            }

            let name = self.parse_identifier("expected resource field name")?;
            self.expect(TokenKind::Colon, "expected `:` after resource field name")?;
            let type_name = self.parse_type_name("expected resource field type after `:`")?;
            fields.push(ResourceField { name, type_name });
        }

        self.expect(
            TokenKind::RightBrace,
            "expected `}` to close resource declaration",
        )?;
        Ok(ResourceDecl { name, fields })
    }

    fn parse_system_declaration(&mut self) -> Result<SystemDecl, ParseError> {
        let name = self.parse_identifier("expected system name after `system`")?;
        self.expect(TokenKind::LeftParen, "expected `(` after system name")?;

        let mut params = Vec::new();
        if self.peek().kind != TokenKind::RightParen {
            loop {
                params.push(self.parse_system_param()?);

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
        self.expect(TokenKind::LeftBrace, "expected `{` after system signature")?;
        self.expect(
            TokenKind::RightBrace,
            "expected `}` after empty system body",
        )?;

        Ok(SystemDecl { name, params })
    }

    fn parse_system_param(&mut self) -> Result<SystemParam, ParseError> {
        let name = self.parse_identifier("expected system parameter name")?;
        self.expect(TokenKind::Colon, "expected `:` after system parameter name")?;

        if !self.match_keyword(Keyword::Read) {
            return Err(ParseError {
                span: self.peek().span,
                message: "expected `read` system parameter access".to_string(),
            });
        }

        let resource_name = self.parse_identifier("expected resource name after `read`")?;

        Ok(SystemParam {
            name,
            kind: SystemParamKind::ReadResource { resource_name },
        })
    }

    fn parse_startup_block(&mut self) -> Result<StartupBlock, ParseError> {
        self.expect(TokenKind::LeftBrace, "expected `{` after `startup`")?;

        let mut statements = Vec::new();
        while self.peek().kind != TokenKind::RightBrace {
            if self.peek().kind == TokenKind::Eof {
                return Err(ParseError {
                    span: self.peek().span,
                    message: "expected `}` to close startup block".to_string(),
                });
            }
            statements.push(self.parse_statement()?);
        }

        self.expect(TokenKind::RightBrace, "expected `}` to close startup block")?;
        Ok(StartupBlock { statements })
    }

    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        if self.match_keyword(Keyword::Let) {
            return self.parse_let_statement();
        }

        if self.match_keyword(Keyword::Exit) {
            return self.parse_exit_statement();
        }

        if self.match_keyword(Keyword::Spawn) {
            return self.parse_spawn_statement();
        }

        if self.match_keyword(Keyword::Resource) {
            return self.parse_resource_statement();
        }

        Err(ParseError {
            span: self.peek().span,
            message: "expected statement".to_string(),
        })
    }

    fn parse_let_statement(&mut self) -> Result<Statement, ParseError> {
        let name = self.parse_identifier("expected binding name after `let`")?;
        self.expect(TokenKind::Colon, "expected `:` after let binding name")?;
        let type_name = self.parse_type_name("expected type name after `:`")?;
        self.expect(TokenKind::Equal, "expected `=` after let binding type")?;
        let initializer = self.parse_expression()?;

        Ok(Statement::Let(LetStatement {
            name,
            type_name,
            initializer,
        }))
    }

    fn parse_exit_statement(&mut self) -> Result<Statement, ParseError> {
        let expression = self.parse_expression_with_message("expected expression after `exit`")?;

        Ok(Statement::Exit(ExitStatement { expression }))
    }

    fn parse_spawn_statement(&mut self) -> Result<Statement, ParseError> {
        self.expect(TokenKind::LeftBrace, "expected `{` after `spawn`")?;

        let mut components = Vec::new();
        while self.peek().kind != TokenKind::RightBrace {
            if self.peek().kind == TokenKind::Eof {
                return Err(ParseError {
                    span: self.peek().span,
                    message: "expected `}` to close spawn block".to_string(),
                });
            }

            let name = self.parse_identifier("expected component literal in spawn block")?;
            self.expect(
                TokenKind::LeftBrace,
                "expected `{` after component literal name",
            )?;

            let mut fields = Vec::new();
            if self.peek().kind == TokenKind::RightBrace {
                return Err(ParseError {
                    span: self.peek().span,
                    message: "expected component literal field".to_string(),
                });
            }

            loop {
                fields.push(self.parse_spawn_component_field()?);

                if self.peek().kind != TokenKind::Comma {
                    break;
                }

                self.advance();
            }

            self.expect(
                TokenKind::RightBrace,
                "expected `}` after component literal fields",
            )?;
            components.push(SpawnComponentLiteral { name, fields });
        }

        self.expect(TokenKind::RightBrace, "expected `}` to close spawn block")?;
        Ok(Statement::Spawn(SpawnStatement { components }))
    }

    fn parse_resource_statement(&mut self) -> Result<Statement, ParseError> {
        let name = self.parse_identifier("expected resource literal name after `resource`")?;
        self.expect(
            TokenKind::LeftBrace,
            "expected `{` after resource literal name",
        )?;

        let mut fields = Vec::new();
        if self.peek().kind == TokenKind::RightBrace {
            return Err(ParseError {
                span: self.peek().span,
                message: "expected resource literal field".to_string(),
            });
        }

        loop {
            fields.push(self.parse_resource_literal_field()?);

            if self.peek().kind != TokenKind::Comma {
                break;
            }

            self.advance();
        }

        self.expect(
            TokenKind::RightBrace,
            "expected `}` after resource literal fields",
        )?;
        Ok(Statement::Resource(ResourceStatement { name, fields }))
    }

    fn parse_resource_literal_field(&mut self) -> Result<ResourceLiteralField, ParseError> {
        let name = self.parse_identifier("expected resource literal field name")?;
        self.expect(
            TokenKind::Colon,
            "expected `:` after resource literal field name",
        )?;
        let value = self.parse_component_literal_value()?;

        Ok(ResourceLiteralField { name, value })
    }

    fn parse_spawn_component_field(&mut self) -> Result<SpawnComponentField, ParseError> {
        let name = self.parse_identifier("expected component literal field name")?;
        self.expect(
            TokenKind::Colon,
            "expected `:` after component literal field name",
        )?;
        let value = self.parse_component_literal_value()?;

        Ok(SpawnComponentField { name, value })
    }

    fn parse_component_literal_value(&mut self) -> Result<ComponentLiteralValue, ParseError> {
        let token = self.peek();
        let span = token.span;
        let text = match &token.kind {
            TokenKind::Float(text) => text.clone(),
            _ => {
                return Err(ParseError {
                    span,
                    message: "expected float literal for component field value".to_string(),
                });
            }
        };
        self.advance();

        Ok(ComponentLiteralValue::Float { text, span })
    }

    fn parse_identifier(&mut self, message: &str) -> Result<String, ParseError> {
        let token = self.peek();
        let name = match &token.kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(ParseError {
                    span: token.span,
                    message: message.to_string(),
                })
            }
        };
        self.advance();

        Ok(name)
    }

    fn parse_type_name(&mut self, message: &str) -> Result<TypeName, ParseError> {
        let token = self.peek();
        let span = token.span;
        let name = match &token.kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(ParseError {
                    span,
                    message: message.to_string(),
                })
            }
        };
        self.advance();

        Ok(TypeName { name, span })
    }

    fn parse_expression(&mut self) -> Result<Expression, ParseError> {
        self.parse_expression_with_message("expected expression")
    }

    fn parse_expression_with_message(&mut self, message: &str) -> Result<Expression, ParseError> {
        let left = self.parse_primary_expression(message)?;

        if let Some(operator) = self.match_binary_operator() {
            let message = format!("expected expression after `{operator}`");
            let right = self.parse_primary_expression(&message)?;
            return Ok(Expression::Binary(BinaryExpression {
                operator,
                left: Box::new(left),
                right: Box::new(right),
            }));
        }

        Ok(left)
    }

    fn parse_primary_expression(&mut self, message: &str) -> Result<Expression, ParseError> {
        let token = self.peek();
        match &token.kind {
            TokenKind::Integer(_) => self.parse_integer_literal(message).map(Expression::Integer),
            TokenKind::Identifier(name) => {
                let expression = Expression::Identifier {
                    name: name.clone(),
                    span: token.span,
                };
                self.advance();
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

    fn match_binary_operator(&mut self) -> Option<BinaryOperator> {
        let operator = if self.peek().kind == TokenKind::Plus {
            BinaryOperator::Add
        } else if self.peek().kind == TokenKind::Minus {
            BinaryOperator::Subtract
        } else if self.peek().kind == TokenKind::Star {
            BinaryOperator::Multiply
        } else {
            return None;
        };

        self.advance();
        Some(operator)
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
        self.tokens
            .get(self.current)
            .or_else(|| self.tokens.last())
            .expect("lexer always emits EOF token")
    }

    fn advance(&mut self) {
        if self.current < self.tokens.len() {
            self.current += 1;
        }
    }
}

impl fmt::Display for Program {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Program")?;
        write!(formatter, "  world {}", self.world.name)?;

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
                    writeln!(formatter, "    {}", format_system_param(param))?;
                }
            }
            write!(formatter, "    body empty")?;
        }

        if let Some(startup) = &self.startup {
            writeln!(formatter)?;
            write!(formatter, "  startup")?;

            for statement in &startup.statements {
                match statement {
                    Statement::Let(let_statement) => {
                        writeln!(formatter)?;
                        writeln!(
                            formatter,
                            "    let {}: {}",
                            let_statement.name, let_statement.type_name.name
                        )?;
                        write_expression(formatter, &let_statement.initializer, "      ")?;
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

impl fmt::Display for Statement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Let(let_statement) => write!(
                formatter,
                "let {}: {} = {}",
                let_statement.name, let_statement.type_name.name, let_statement.initializer
            ),
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
            Self::Identifier { name, .. } => formatter.write_str(name),
            Self::Binary(binary) => write!(
                formatter,
                "{} {} {}",
                binary.left, binary.operator, binary.right
            ),
        }
    }
}

impl fmt::Display for BinaryOperator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Add => formatter.write_str("+"),
            Self::Subtract => formatter.write_str("-"),
            Self::Multiply => formatter.write_str("*"),
        }
    }
}

impl fmt::Display for ComponentLiteralValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Float { text, .. } => formatter.write_str(text),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} at byte {}", self.message, self.span.start)
    }
}

fn write_component_literal_value(
    formatter: &mut fmt::Formatter<'_>,
    value: &ComponentLiteralValue,
    indent: &str,
) -> fmt::Result {
    match value {
        ComponentLiteralValue::Float { text, .. } => write!(formatter, "{indent}float {text}"),
    }
}

fn format_system_param(param: &SystemParam) -> String {
    match &param.kind {
        SystemParamKind::ReadResource { resource_name } => {
            format!("param {}: read {}", param.name, resource_name)
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
        Expression::Identifier { name, .. } => write!(formatter, "{indent}identifier {name}"),
        Expression::Binary(binary) => {
            writeln!(formatter, "{indent}binary {}", binary.operator)?;
            write_expression(formatter, &binary.left, &format!("{indent}  "))?;
            writeln!(formatter)?;
            write_expression(formatter, &binary.right, &format!("{indent}  "))
        }
    }
}
