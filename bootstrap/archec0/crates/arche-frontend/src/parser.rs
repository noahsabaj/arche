//! Recursive-descent parser for the frozen M27-C1 surface grammar.
//!
//! Parsing retains source order and exact spans.  It intentionally performs no
//! name, type, effect, ownership, or stable-identity checking.

use std::io::BufRead;
use std::sync::Arc;

use crate::ast::*;
use crate::lexer::{Keyword, Lexer, Punctuation, Token, TokenKind};
use crate::{Diagnostic, FileId, SourcePosition, Span, Symbol};

/// Parses one retained C1 source reader into its syntax-only AST.
pub fn parse_reader<R: BufRead>(file: FileId, reader: R) -> Result<AstFile, Diagnostic> {
    Parser::new(file, reader)?.parse_file()
}

struct Parser<R: BufRead> {
    lexer: Lexer<R>,
    current: Token,
    next: Token,
    previous: Option<Span>,
}

impl<R: BufRead> Parser<R> {
    fn new(file: FileId, reader: R) -> Result<Self, Diagnostic> {
        let mut lexer = Lexer::new(file, reader);
        let current = lexer.next_token()?;
        let next = lexer.next_token()?;
        Ok(Self {
            lexer,
            current,
            next,
            previous: None,
        })
    }

    fn parse_file(mut self) -> Result<AstFile, Diagnostic> {
        let mut items = Vec::new();
        while !self.at_eof() {
            items.push(self.parse_item()?);
        }
        Ok(AstFile {
            items,
            eof_span: self.current.span,
        })
    }

    fn parse_item(&mut self) -> Result<AstItem, Diagnostic> {
        let docs = self.parse_docs()?;
        if self.at_keyword(Keyword::Impl) {
            return self
                .parse_impl(docs)
                .map(|item| AstItem::Impl(Box::new(item)));
        }
        let visibility = self.parse_visibility()?;
        if self.at_keyword(Keyword::Mod) {
            return self.parse_module(docs, visibility).map(AstItem::Module);
        }
        if self.at_keyword(Keyword::Use) {
            return self.parse_import(docs, visibility).map(AstItem::Import);
        }
        self.parse_declaration(docs, visibility)
            .map(|item| AstItem::Declaration(Box::new(item)))
    }

    fn parse_docs(&mut self) -> Result<Vec<AstDocComment>, Diagnostic> {
        let mut docs = Vec::new();
        while let TokenKind::DocComment(text) = self.current.kind.clone() {
            let span = self.bump()?.span;
            docs.push(AstDocComment { text, span });
        }
        Ok(docs)
    }

    fn parse_visibility(&mut self) -> Result<AstVisibility, Diagnostic> {
        if !self.at_keyword(Keyword::Pub) {
            return Ok(AstVisibility {
                kind: AstVisibilityKind::Private,
                span: self.point_span(),
            });
        }
        let start = self.bump()?.span;
        if !self.consume_punct(Punctuation::LeftParen)? {
            return Ok(AstVisibility {
                kind: AstVisibilityKind::Public,
                span: start,
            });
        }
        let kind = if self.consume_keyword(Keyword::Package)? {
            AstVisibilityKind::Package
        } else if self.consume_keyword(Keyword::Super)? {
            AstVisibilityKind::Super
        } else if self.consume_keyword(Keyword::In)? {
            let path = self.parse_path(true)?;
            if !matches!(
                path.root,
                AstPathRoot::Package | AstPathRoot::SelfValue | AstPathRoot::Super(_)
            ) {
                return Err(Diagnostic::at(
                    "PARSE001",
                    path.span,
                    "visibility paths must begin with `package`, `self`, or `super`",
                ));
            }
            AstVisibilityKind::In(path)
        } else {
            return Err(self.error("expected `package`, `super`, or `in path` in visibility"));
        };
        let close =
            self.expect_punct(Punctuation::RightParen, "expected `)` to close visibility")?;
        Ok(AstVisibility {
            kind,
            span: start.join(close),
        })
    }

    fn parse_module(
        &mut self,
        docs: Vec<AstDocComment>,
        visibility: AstVisibility,
    ) -> Result<AstModule, Diagnostic> {
        let start = self.expect_keyword(Keyword::Mod, "expected `mod`")?;
        let (name, name_span) = self.identifier("expected module name after `mod`")?;
        let end = self.expect_punct(Punctuation::Semicolon, "expected `;` after module item")?;
        Ok(AstModule {
            docs,
            visibility,
            name,
            name_span,
            span: start.join(end),
        })
    }

    fn parse_import(
        &mut self,
        docs: Vec<AstDocComment>,
        visibility: AstVisibility,
    ) -> Result<AstImport, Diagnostic> {
        let start = self.expect_keyword(Keyword::Use, "expected `use`")?;
        let path = self.parse_path(false)?;
        if matches!(path.root, AstPathRoot::Bare | AstPathRoot::SelfType) {
            return Err(Diagnostic::at(
                "PARSE001",
                path.span,
                "an import requires a rooted item path",
            ));
        }
        let end = self.expect_punct(Punctuation::Semicolon, "expected `;` after import")?;
        Ok(AstImport {
            docs,
            visibility,
            path,
            span: start.join(end),
        })
    }

    fn parse_path(&mut self, allow_root_only: bool) -> Result<AstPath, Diagnostic> {
        let start = self.current.span;
        let mut segments = Vec::new();
        let root = match self.current.kind.clone() {
            TokenKind::Identifier(first) => {
                let first_span = self.bump()?.span;
                if self.at_punct(Punctuation::ColonColon) && !self.next_is_punct(Punctuation::Less)
                {
                    AstPathRoot::Identifier(first)
                } else {
                    segments.push(AstPathSegment {
                        name: first,
                        generic_arguments: None,
                        span: first_span,
                    });
                    AstPathRoot::Bare
                }
            }
            TokenKind::Keyword(Keyword::Package) => {
                self.bump()?;
                AstPathRoot::Package
            }
            TokenKind::Keyword(Keyword::SelfValue) => {
                self.bump()?;
                AstPathRoot::SelfValue
            }
            TokenKind::Keyword(Keyword::SelfType) => {
                self.bump()?;
                AstPathRoot::SelfType
            }
            TokenKind::Keyword(Keyword::Super) => {
                let mut depth = 0_u64;
                loop {
                    depth = depth.checked_add(1).ok_or_else(|| {
                        Diagnostic::at("PARSE004", self.current.span, "`super` depth overflow")
                    })?;
                    self.bump()?;
                    if self.at_punct(Punctuation::ColonColon)
                        && self.next_is_keyword(Keyword::Super)
                    {
                        self.bump()?;
                        continue;
                    }
                    break;
                }
                AstPathRoot::Super(depth)
            }
            _ => return Err(self.error("expected a path")),
        };

        while self.at_punct(Punctuation::ColonColon) && !self.next_is_punct(Punctuation::Less) {
            self.bump()?;
            let (name, span) = self.identifier("expected path segment after `::`")?;
            segments.push(AstPathSegment {
                name,
                generic_arguments: None,
                span,
            });
        }
        if segments.is_empty()
            && !allow_root_only
            && matches!(
                root,
                AstPathRoot::Package | AstPathRoot::SelfValue | AstPathRoot::Super(_)
            )
            && !(self.at_punct(Punctuation::ColonColon) && self.next_is_punct(Punctuation::Less))
        {
            return Err(self.error("expected `::` and a path segment after path root"));
        }
        Ok(AstPath {
            root,
            segments,
            generic_arguments: None,
            span: self.finish_span(start),
        })
    }

    fn parse_declaration(
        &mut self,
        docs: Vec<AstDocComment>,
        visibility: AstVisibility,
    ) -> Result<AstDeclaration, Diagnostic> {
        if self.at_keyword(Keyword::World) {
            return self.parse_world(docs, visibility);
        }
        if self.at_keyword(Keyword::Component) || self.at_keyword(Keyword::Resource) {
            return self.parse_record_declaration(docs, visibility);
        }
        if self.at_keyword(Keyword::Tag) {
            return self.parse_tag(docs, visibility);
        }
        if self.at_keyword(Keyword::Struct) {
            return self.parse_struct(docs, visibility);
        }
        if self.at_keyword(Keyword::Enum) {
            return self.parse_enum(docs, visibility);
        }
        if self.at_keyword(Keyword::Type) {
            return self.parse_type_alias(docs, visibility);
        }
        if self.at_keyword(Keyword::Const) {
            return self.parse_const_item(docs, visibility);
        }
        if self.at_keyword(Keyword::Static) {
            return self.parse_static_item(docs, visibility);
        }
        if self.at_keyword(Keyword::System) {
            return self.parse_system(docs, visibility);
        }
        if self.at_keyword(Keyword::Schedule) {
            return self.parse_schedule(docs, visibility);
        }
        if self.at_keyword(Keyword::Trait) {
            return self.parse_trait(docs, visibility);
        }
        if self.at_keyword(Keyword::Fn)
            || self.at_keyword(Keyword::Gen)
            || self.at_keyword(Keyword::Unsafe)
        {
            return self.parse_callable_declaration(docs, visibility);
        }
        Err(self.error("expected an M27-C1 item declaration"))
    }

    fn parse_world(
        &mut self,
        docs: Vec<AstDocComment>,
        visibility: AstVisibility,
    ) -> Result<AstDeclaration, Diagnostic> {
        let start = self.expect_keyword(Keyword::World, "expected `world`")?;
        let (name, name_span) = self.identifier("expected world name")?;
        self.expect_punct(Punctuation::LeftBrace, "expected `{` after world name")?;
        self.expect_keyword(Keyword::Init, "expected `init` in world")?;
        let initializer = self.parse_world_init_block()?;
        let end = self.expect_punct(Punctuation::RightBrace, "expected `}` to close world")?;
        Ok(AstDeclaration {
            docs,
            visibility,
            name,
            name_span,
            kind: AstDeclarationKind::World {
                initializer: Box::new(initializer),
            },
            span: start.join(end),
        })
    }

    fn parse_world_init_block(&mut self) -> Result<AstWorldInitBlock, Diagnostic> {
        let start = self.expect_punct(Punctuation::LeftBrace, "expected `{` after `init`")?;
        let mut entries = Vec::new();
        while !self.at_punct(Punctuation::RightBrace) {
            let entry_start = self.current.span;
            let kind = if self.consume_keyword(Keyword::Resource)? {
                let ty = self.parse_type()?;
                self.expect_punct(Punctuation::Equal, "expected `=` in resource initializer")?;
                let value = self.parse_expression()?;
                AstWorldInitKind::Resource {
                    ty: Box::new(ty),
                    value: Box::new(value),
                }
            } else if self.consume_keyword(Keyword::Spawn)? {
                self.expect_punct(Punctuation::LeftBrace, "expected `{` after `spawn`")?;
                let values = self.parse_expression_list(Punctuation::RightBrace)?;
                self.expect_punct(Punctuation::RightBrace, "expected `}` after spawn values")?;
                AstWorldInitKind::Spawn { values }
            } else {
                return Err(self.error("expected `resource` or `spawn` world initializer"));
            };
            let end = self.expect_punct(
                Punctuation::Semicolon,
                "expected `;` after world initializer",
            )?;
            entries.push(AstWorldInit {
                kind,
                span: entry_start.join(end),
            });
        }
        let end = self.expect_punct(
            Punctuation::RightBrace,
            "expected `}` to close world initializer block",
        )?;
        Ok(AstWorldInitBlock {
            entries,
            span: start.join(end),
        })
    }

    fn parse_record_declaration(
        &mut self,
        docs: Vec<AstDocComment>,
        visibility: AstVisibility,
    ) -> Result<AstDeclaration, Diagnostic> {
        let component = self.at_keyword(Keyword::Component);
        let start = self.bump()?.span;
        let (name, name_span) = self.identifier("expected component or resource name")?;
        let generics = self.parse_generic_parameters(false)?;
        let where_clause = self.parse_where_clause()?;
        let fields = self.parse_record_fields()?;
        let declaration = AstRecordDeclaration {
            generics,
            where_clause,
            fields,
        };
        Ok(AstDeclaration {
            docs,
            visibility,
            name,
            name_span,
            kind: if component {
                AstDeclarationKind::Component(Box::new(declaration))
            } else {
                AstDeclarationKind::Resource(Box::new(declaration))
            },
            span: self.finish_span(start),
        })
    }

    fn parse_tag(
        &mut self,
        docs: Vec<AstDocComment>,
        visibility: AstVisibility,
    ) -> Result<AstDeclaration, Diagnostic> {
        let start = self.expect_keyword(Keyword::Tag, "expected `tag`")?;
        let (name, name_span) = self.identifier("expected tag name")?;
        let end = self.expect_punct(Punctuation::Semicolon, "expected `;` after tag")?;
        Ok(AstDeclaration {
            docs,
            visibility,
            name,
            name_span,
            kind: AstDeclarationKind::Tag,
            span: start.join(end),
        })
    }

    fn parse_struct(
        &mut self,
        docs: Vec<AstDocComment>,
        visibility: AstVisibility,
    ) -> Result<AstDeclaration, Diagnostic> {
        let start = self.expect_keyword(Keyword::Struct, "expected `struct`")?;
        let (name, name_span) = self.identifier("expected struct name")?;
        let generics = self.parse_generic_parameters(false)?;
        let where_clause = self.parse_where_clause()?;
        let form = if self.at_punct(Punctuation::LeftBrace) {
            AstStructForm::Record(self.parse_record_fields()?)
        } else if self.consume_punct(Punctuation::LeftParen)? {
            let mut fields = Vec::new();
            if !self.at_punct(Punctuation::RightParen) {
                loop {
                    let field_start = self.current.span;
                    let visibility = self.parse_visibility()?;
                    let ty = self.parse_type()?;
                    fields.push(AstTupleField {
                        visibility,
                        ty,
                        span: self.finish_span(field_start),
                    });
                    if !self.consume_punct(Punctuation::Comma)? {
                        break;
                    }
                    if self.at_punct(Punctuation::RightParen) {
                        break;
                    }
                }
            }
            self.expect_punct(Punctuation::RightParen, "expected `)` after tuple fields")?;
            self.expect_punct(Punctuation::Semicolon, "expected `;` after tuple struct")?;
            AstStructForm::Tuple(fields)
        } else {
            self.expect_punct(Punctuation::Semicolon, "expected struct body or `;`")?;
            AstStructForm::Unit
        };
        Ok(AstDeclaration {
            docs,
            visibility,
            name,
            name_span,
            kind: AstDeclarationKind::Struct(Box::new(AstStructDeclaration {
                generics,
                where_clause,
                form,
            })),
            span: self.finish_span(start),
        })
    }

    fn parse_record_fields(&mut self) -> Result<Vec<AstRecordField>, Diagnostic> {
        self.expect_punct(Punctuation::LeftBrace, "expected `{` for record fields")?;
        let mut fields = Vec::new();
        if !self.at_punct(Punctuation::RightBrace) {
            loop {
                let start = self.current.span;
                let docs = self.parse_docs()?;
                let visibility = self.parse_visibility()?;
                let (name, _) = self.identifier("expected record field name")?;
                self.expect_punct(Punctuation::Colon, "expected `:` after field name")?;
                let ty = self.parse_type()?;
                fields.push(AstRecordField {
                    docs,
                    visibility,
                    name,
                    ty,
                    span: self.finish_span(start),
                });
                if !self.consume_punct(Punctuation::Comma)? {
                    break;
                }
                if self.at_punct(Punctuation::RightBrace) {
                    break;
                }
            }
        }
        self.expect_punct(Punctuation::RightBrace, "expected `}` after record fields")?;
        Ok(fields)
    }

    fn parse_enum(
        &mut self,
        docs: Vec<AstDocComment>,
        visibility: AstVisibility,
    ) -> Result<AstDeclaration, Diagnostic> {
        let start = self.expect_keyword(Keyword::Enum, "expected `enum`")?;
        let (name, name_span) = self.identifier("expected enum name")?;
        let generics = self.parse_generic_parameters(false)?;
        let where_clause = self.parse_where_clause()?;
        self.expect_punct(Punctuation::LeftBrace, "expected `{` after enum header")?;
        let mut variants = Vec::new();
        if !self.at_punct(Punctuation::RightBrace) {
            loop {
                variants.push(self.parse_variant()?);
                if !self.consume_punct(Punctuation::Comma)? {
                    break;
                }
                if self.at_punct(Punctuation::RightBrace) {
                    break;
                }
            }
        }
        self.expect_punct(Punctuation::RightBrace, "expected `}` after enum variants")?;
        Ok(AstDeclaration {
            docs,
            visibility,
            name,
            name_span,
            kind: AstDeclarationKind::Enum(Box::new(AstEnumDeclaration {
                generics,
                where_clause,
                variants,
            })),
            span: self.finish_span(start),
        })
    }

    fn parse_variant(&mut self) -> Result<AstVariant, Diagnostic> {
        let start = self.current.span;
        let docs = self.parse_docs()?;
        let (name, _) = self.identifier("expected enum variant name")?;
        let form = if self.consume_punct(Punctuation::LeftParen)? {
            let values = self.parse_type_list(Punctuation::RightParen)?;
            self.expect_punct(Punctuation::RightParen, "expected `)` after variant fields")?;
            AstVariantForm::Tuple(values)
        } else if self.consume_punct(Punctuation::LeftBrace)? {
            let mut fields = Vec::new();
            if !self.at_punct(Punctuation::RightBrace) {
                loop {
                    let field_start = self.current.span;
                    let (field_name, _) = self.identifier("expected variant field name")?;
                    self.expect_punct(Punctuation::Colon, "expected `:` after variant field")?;
                    let ty = self.parse_type()?;
                    fields.push(AstVariantField {
                        name: field_name,
                        ty,
                        span: self.finish_span(field_start),
                    });
                    if !self.consume_punct(Punctuation::Comma)? {
                        break;
                    }
                    if self.at_punct(Punctuation::RightBrace) {
                        break;
                    }
                }
            }
            self.expect_punct(Punctuation::RightBrace, "expected `}` after variant fields")?;
            AstVariantForm::Record(fields)
        } else {
            AstVariantForm::Unit
        };
        Ok(AstVariant {
            docs,
            name,
            form,
            span: self.finish_span(start),
        })
    }

    fn parse_type_alias(
        &mut self,
        docs: Vec<AstDocComment>,
        visibility: AstVisibility,
    ) -> Result<AstDeclaration, Diagnostic> {
        let start = self.expect_keyword(Keyword::Type, "expected `type`")?;
        let (name, name_span) = self.identifier("expected type alias name")?;
        let generics = self.parse_generic_parameters(false)?;
        self.expect_punct(Punctuation::Equal, "expected `=` in type alias")?;
        let target = self.parse_type()?;
        let where_clause = self.parse_where_clause()?;
        self.expect_punct(Punctuation::Semicolon, "expected `;` after type alias")?;
        Ok(AstDeclaration {
            docs,
            visibility,
            name,
            name_span,
            kind: AstDeclarationKind::TypeAlias(Box::new(AstTypeAlias {
                generics,
                target,
                where_clause,
            })),
            span: self.finish_span(start),
        })
    }

    fn parse_const_item(
        &mut self,
        docs: Vec<AstDocComment>,
        visibility: AstVisibility,
    ) -> Result<AstDeclaration, Diagnostic> {
        let start = self.expect_keyword(Keyword::Const, "expected `const`")?;
        let (name, name_span) = self.identifier("expected const name")?;
        self.expect_punct(Punctuation::Colon, "expected `:` after const name")?;
        let ty = self.parse_type()?;
        self.expect_punct(Punctuation::Equal, "expected `=` in const item")?;
        let value = self.parse_expression()?;
        self.expect_punct(Punctuation::Semicolon, "expected `;` after const item")?;
        Ok(AstDeclaration {
            docs,
            visibility,
            name,
            name_span,
            kind: AstDeclarationKind::Const(Box::new(AstConstItem { ty, value })),
            span: self.finish_span(start),
        })
    }

    fn parse_static_item(
        &mut self,
        docs: Vec<AstDocComment>,
        visibility: AstVisibility,
    ) -> Result<AstDeclaration, Diagnostic> {
        let start = self.expect_keyword(Keyword::Static, "expected `static`")?;
        let mutable = self.consume_keyword(Keyword::Mut)?;
        let (name, name_span) = self.identifier("expected static name")?;
        self.expect_punct(Punctuation::Colon, "expected `:` after static name")?;
        let ty = self.parse_type()?;
        self.expect_punct(Punctuation::Equal, "expected `=` in static item")?;
        let value = self.parse_expression()?;
        self.expect_punct(Punctuation::Semicolon, "expected `;` after static item")?;
        Ok(AstDeclaration {
            docs,
            visibility,
            name,
            name_span,
            kind: AstDeclarationKind::Static(Box::new(AstStaticItem { mutable, ty, value })),
            span: self.finish_span(start),
        })
    }

    fn parse_callable_declaration(
        &mut self,
        docs: Vec<AstDocComment>,
        visibility: AstVisibility,
    ) -> Result<AstDeclaration, Diagnostic> {
        let start = self.current.span;
        let unsafe_ = self.consume_keyword(Keyword::Unsafe)?;
        let generator = self.consume_keyword(Keyword::Gen)?;
        self.expect_keyword(Keyword::Fn, "expected `fn` in callable declaration")?;
        let (name, name_span) = self.identifier("expected callable name")?;
        let generics = self.parse_generic_parameters(false)?;
        self.expect_punct(Punctuation::LeftParen, "expected `(` after callable name")?;
        let parameters = self.parse_parameters(Punctuation::RightParen)?;
        self.expect_punct(Punctuation::RightParen, "expected `)` after parameters")?;

        let kind = if generator {
            self.expect_keyword(Keyword::Resume, "expected `resume` in generator signature")?;
            let resume = self.parse_type()?;
            self.expect_keyword(Keyword::Yields, "expected `yields` in generator signature")?;
            let yields = self.parse_type()?;
            let effects = self.parse_effect_sets()?;
            let result = if self.consume_punct(Punctuation::Arrow)? {
                Some(self.parse_type()?)
            } else {
                None
            };
            let where_clause = self.parse_where_clause()?;
            let body = self.parse_block()?;
            AstDeclarationKind::Generator(Box::new(AstGenerator {
                unsafe_,
                generics,
                parameters,
                resume,
                yields,
                effects,
                result,
                where_clause,
                body,
            }))
        } else {
            let effects = self.parse_effect_sets()?;
            let result = if self.consume_punct(Punctuation::Arrow)? {
                Some(self.parse_type()?)
            } else {
                None
            };
            let where_clause = self.parse_where_clause()?;
            let signature_span = self.finish_span(start);
            let body = self.parse_block()?;
            AstDeclarationKind::Function(Box::new(AstFunction {
                signature: AstFunctionSignature {
                    unsafe_,
                    generics,
                    parameters,
                    effects,
                    result,
                    where_clause,
                    span: signature_span,
                },
                body,
            }))
        };
        Ok(AstDeclaration {
            docs,
            visibility,
            name,
            name_span,
            kind,
            span: self.finish_span(start),
        })
    }

    fn parse_parameters(&mut self, close: Punctuation) -> Result<Vec<AstParameter>, Diagnostic> {
        let mut parameters = Vec::new();
        if self.at_punct(close) {
            return Ok(parameters);
        }
        loop {
            let start = self.current.span;
            let pattern = self.parse_pattern()?;
            self.expect_punct(Punctuation::Colon, "expected `:` after parameter pattern")?;
            let ty = self.parse_type()?;
            parameters.push(AstParameter {
                pattern,
                ty,
                span: self.finish_span(start),
            });
            if !self.consume_punct(Punctuation::Comma)? {
                break;
            }
            if self.at_punct(close) {
                break;
            }
        }
        Ok(parameters)
    }

    fn parse_trait(
        &mut self,
        docs: Vec<AstDocComment>,
        visibility: AstVisibility,
    ) -> Result<AstDeclaration, Diagnostic> {
        let start = self.expect_keyword(Keyword::Trait, "expected `trait`")?;
        let (name, name_span) = self.identifier("expected trait name")?;
        let generics = self.parse_generic_parameters(false)?;
        let where_clause = self.parse_where_clause()?;
        self.expect_punct(Punctuation::LeftBrace, "expected `{` after trait header")?;
        let mut methods = Vec::new();
        while !self.at_punct(Punctuation::RightBrace) {
            let method_start = self.current.span;
            let method_docs = self.parse_docs()?;
            let (method_name, signature) = self.parse_method_signature()?;
            let end =
                self.expect_punct(Punctuation::Semicolon, "expected `;` after trait method")?;
            methods.push(AstTraitMethod {
                docs: method_docs,
                name: method_name,
                signature,
                span: method_start.join(end),
            });
        }
        self.expect_punct(Punctuation::RightBrace, "expected `}` after trait methods")?;
        Ok(AstDeclaration {
            docs,
            visibility,
            name,
            name_span,
            kind: AstDeclarationKind::Trait(Box::new(AstTrait {
                generics,
                where_clause,
                methods,
            })),
            span: self.finish_span(start),
        })
    }

    fn parse_impl(&mut self, docs: Vec<AstDocComment>) -> Result<AstImpl, Diagnostic> {
        let start = self.expect_keyword(Keyword::Impl, "expected `impl`")?;
        let is_default = self.consume_keyword(Keyword::Default)?;
        let generics = self.parse_generic_parameters(false)?;
        let first = self.parse_type()?;
        let (trait_path, target) = if self.consume_keyword(Keyword::For)? {
            let path = match first.kind {
                AstTypeKind::Path(path) => path,
                AstTypeKind::SelfType => AstPath {
                    root: AstPathRoot::SelfType,
                    segments: Vec::new(),
                    generic_arguments: None,
                    span: first.span,
                },
                _ => {
                    return Err(Diagnostic::at(
                        "PARSE001",
                        first.span,
                        "expected trait path before `for`",
                    ));
                }
            };
            (Some(path), self.parse_type()?)
        } else {
            (None, first)
        };
        let where_clause = self.parse_where_clause()?;
        self.expect_punct(Punctuation::LeftBrace, "expected `{` after impl header")?;
        let mut methods = Vec::new();
        while !self.at_punct(Punctuation::RightBrace) {
            let method_start = self.current.span;
            let method_docs = self.parse_docs()?;
            let visibility = self.parse_visibility()?;
            let (name, signature) = self.parse_method_signature()?;
            let body = self.parse_block()?;
            methods.push(AstImplMethod {
                docs: method_docs,
                visibility,
                name,
                signature,
                body,
                span: self.finish_span(method_start),
            });
        }
        self.expect_punct(Punctuation::RightBrace, "expected `}` after impl methods")?;
        Ok(AstImpl {
            docs,
            is_default,
            generics,
            trait_path,
            target,
            where_clause,
            methods,
            span: self.finish_span(start),
        })
    }

    fn parse_method_signature(
        &mut self,
    ) -> Result<(AstMethodName, AstMethodSignature), Diagnostic> {
        let start = self.current.span;
        let unsafe_ = self.consume_keyword(Keyword::Unsafe)?;
        self.expect_keyword(Keyword::Fn, "expected `fn` in method signature")?;
        let name = self.parse_method_name("expected method name")?;
        let generics = self.parse_generic_parameters(false)?;
        self.expect_punct(Punctuation::LeftParen, "expected `(` after method name")?;
        let parameters = self.parse_method_parameters()?;
        self.expect_punct(
            Punctuation::RightParen,
            "expected `)` after method parameters",
        )?;
        let effects = self.parse_effect_sets()?;
        let result = if self.consume_punct(Punctuation::Arrow)? {
            Some(self.parse_type()?)
        } else {
            None
        };
        let where_clause = self.parse_where_clause()?;
        Ok((
            name,
            AstMethodSignature {
                unsafe_,
                generics,
                parameters,
                effects,
                result,
                where_clause,
                span: self.finish_span(start),
            },
        ))
    }

    fn parse_method_parameters(&mut self) -> Result<Vec<AstMethodParameter>, Diagnostic> {
        let mut parameters = Vec::new();
        if self.at_punct(Punctuation::RightParen) {
            return Ok(parameters);
        }
        if self.at_keyword(Keyword::SelfValue)
            || (self.at_keyword(Keyword::Mut) && self.next_is_keyword(Keyword::SelfValue))
        {
            let start = self.current.span;
            let mutable = self.consume_keyword(Keyword::Mut)?;
            self.expect_keyword(Keyword::SelfValue, "expected `self` receiver")?;
            parameters.push(AstMethodParameter::Receiver(AstReceiver {
                kind: AstReceiverKind::Value { mutable },
                span: self.finish_span(start),
            }));
            if self.consume_punct(Punctuation::Comma)? {
                if self.at_punct(Punctuation::RightParen) {
                    return Ok(parameters);
                }
            } else {
                return Ok(parameters);
            }
        } else if self.at_punct(Punctuation::Ampersand) {
            let start = self.bump()?.span;
            let lifetime = self.take_lifetime()?;
            let mutable = self.consume_keyword(Keyword::Mut)?;
            if self.consume_keyword(Keyword::SelfValue)? {
                parameters.push(AstMethodParameter::Receiver(AstReceiver {
                    kind: AstReceiverKind::Reference { lifetime, mutable },
                    span: self.finish_span(start),
                }));
                if self.consume_punct(Punctuation::Comma)? {
                    if self.at_punct(Punctuation::RightParen) {
                        return Ok(parameters);
                    }
                } else {
                    return Ok(parameters);
                }
            } else {
                if lifetime.is_some() {
                    return Err(Diagnostic::at(
                        "PARSE001",
                        start,
                        "a lifetime after `&` is valid only on a method receiver",
                    ));
                }
                let inner = self.parse_pattern()?;
                let pattern = AstNode::new(
                    AstPatternKind::Reference {
                        mutable,
                        pattern: Box::new(inner),
                    },
                    self.finish_span(start),
                );
                self.expect_punct(Punctuation::Colon, "expected `:` after method parameter")?;
                let ty = self.parse_type()?;
                parameters.push(AstMethodParameter::Parameter(Box::new(AstParameter {
                    pattern,
                    ty,
                    span: self.finish_span(start),
                })));
                if self.consume_punct(Punctuation::Comma)? {
                    if self.at_punct(Punctuation::RightParen) {
                        return Ok(parameters);
                    }
                } else {
                    return Ok(parameters);
                }
            }
        }
        loop {
            let start = self.current.span;
            let pattern = self.parse_pattern()?;
            self.expect_punct(Punctuation::Colon, "expected `:` after method parameter")?;
            let ty = self.parse_type()?;
            parameters.push(AstMethodParameter::Parameter(Box::new(AstParameter {
                pattern,
                ty,
                span: self.finish_span(start),
            })));
            if !self.consume_punct(Punctuation::Comma)? {
                break;
            }
            if self.at_punct(Punctuation::RightParen) {
                break;
            }
        }
        Ok(parameters)
    }

    fn parse_system(
        &mut self,
        docs: Vec<AstDocComment>,
        visibility: AstVisibility,
    ) -> Result<AstDeclaration, Diagnostic> {
        let start = self.expect_keyword(Keyword::System, "expected `system`")?;
        let (name, name_span) = self.identifier("expected system name")?;
        let generics = self.parse_generic_parameters(true)?;
        self.expect_punct(Punctuation::LeftParen, "expected `(` after system name")?;
        let mut parameters = Vec::new();
        if !self.at_punct(Punctuation::RightParen) {
            loop {
                parameters.push(self.parse_system_parameter()?);
                if !self.consume_punct(Punctuation::Comma)? {
                    break;
                }
                if self.at_punct(Punctuation::RightParen) {
                    break;
                }
            }
        }
        self.expect_punct(
            Punctuation::RightParen,
            "expected `)` after system parameters",
        )?;
        let effects = self.parse_effect_sets()?;
        let where_clause = self.parse_where_clause()?;
        let body = self.parse_block()?;
        Ok(AstDeclaration {
            docs,
            visibility,
            name,
            name_span,
            kind: AstDeclarationKind::System(Box::new(AstSystem {
                generics,
                parameters,
                effects,
                where_clause,
                body,
            })),
            span: self.finish_span(start),
        })
    }

    fn parse_system_parameter(&mut self) -> Result<AstSystemParameter, Diagnostic> {
        let start = self.current.span;
        let (name, _) = self.identifier("expected system parameter name")?;
        self.expect_punct(
            Punctuation::Colon,
            "expected `:` after system parameter name",
        )?;
        let kind = if self.consume_keyword(Keyword::Read)? {
            AstSystemParameterKind::ResourceRead(self.parse_type()?)
        } else if self.consume_keyword(Keyword::Mut)? {
            AstSystemParameterKind::ResourceWrite(self.parse_type()?)
        } else if self.consume_keyword(Keyword::Query)? {
            self.expect_punct(Punctuation::LeftBracket, "expected `[` after `query`")?;
            let mut terms = Vec::new();
            if !self.at_punct(Punctuation::RightBracket) {
                loop {
                    let term_start = self.current.span;
                    let term_kind = if self.at_punct(Punctuation::Bang)
                        && !matches!(
                            self.next.kind,
                            TokenKind::Punctuation(Punctuation::Comma)
                                | TokenKind::Punctuation(Punctuation::RightBracket)
                        ) {
                        self.bump()?;
                        AstQueryTermKind::Exclude
                    } else if self.consume_keyword(Keyword::Mut)? {
                        AstQueryTermKind::Write
                    } else {
                        AstQueryTermKind::Read
                    };
                    let ty = self.parse_type()?;
                    terms.push(AstQueryTerm {
                        kind: term_kind,
                        ty,
                        span: self.finish_span(term_start),
                    });
                    if !self.consume_punct(Punctuation::Comma)? {
                        break;
                    }
                    if self.at_punct(Punctuation::RightBracket) {
                        break;
                    }
                }
            }
            self.expect_punct(Punctuation::RightBracket, "expected `]` after query terms")?;
            AstSystemParameterKind::Query(terms)
        } else if self.consume_keyword(Keyword::Commands)? {
            AstSystemParameterKind::Commands
        } else {
            AstSystemParameterKind::Capability(self.parse_type()?)
        };
        Ok(AstSystemParameter {
            name,
            kind,
            span: self.finish_span(start),
        })
    }

    fn parse_schedule(
        &mut self,
        docs: Vec<AstDocComment>,
        visibility: AstVisibility,
    ) -> Result<AstDeclaration, Diagnostic> {
        let start = self.expect_keyword(Keyword::Schedule, "expected `schedule`")?;
        let (name, name_span) = self.identifier("expected schedule name")?;
        self.expect_punct(Punctuation::LeftBrace, "expected `{` after schedule name")?;
        let mut runs = Vec::new();
        while !self.at_punct(Punctuation::RightBrace) {
            let run_start = self.expect_keyword(Keyword::Run, "expected `run` in schedule")?;
            let target = self.parse_path(false)?;
            self.validate_rooted_or_bound_path(&target, "schedule target")?;
            let arguments = if self.at_punct(Punctuation::ColonColon)
                && self.next_is_punct(Punctuation::Less)
            {
                self.bump()?;
                Some(self.parse_system_generic_arguments()?)
            } else {
                None
            };
            let end =
                self.expect_punct(Punctuation::Semicolon, "expected `;` after schedule run")?;
            runs.push(AstScheduleRun {
                target,
                arguments,
                span: run_start.join(end),
            });
        }
        self.expect_punct(Punctuation::RightBrace, "expected `}` after schedule runs")?;
        Ok(AstDeclaration {
            docs,
            visibility,
            name,
            name_span,
            kind: AstDeclarationKind::Schedule(Box::new(AstSchedule { runs })),
            span: self.finish_span(start),
        })
    }

    fn parse_system_generic_arguments(&mut self) -> Result<AstSystemGenericArguments, Diagnostic> {
        let start = self.expect_punct(Punctuation::Less, "expected `<` for system arguments")?;
        let mut arguments = Vec::new();
        if self.at_generic_close() {
            return Err(self.error("system generic arguments cannot be empty"));
        }
        loop {
            if self.consume_keyword(Keyword::Const)? {
                arguments.push(AstSystemGenericArgument::IntegerConst(
                    self.parse_const_expression()?,
                ));
            } else {
                arguments.push(AstSystemGenericArgument::Type(self.parse_type()?));
            }
            if !self.consume_punct(Punctuation::Comma)? {
                break;
            }
            if self.at_generic_close() {
                break;
            }
        }
        let end = self.expect_generic_close()?;
        Ok(AstSystemGenericArguments {
            arguments,
            span: start.join(end),
        })
    }

    fn parse_generic_parameters(
        &mut self,
        system_only: bool,
    ) -> Result<Option<AstGenericParameters>, Diagnostic> {
        if !self.at_punct(Punctuation::Less) {
            return Ok(None);
        }
        let start = self.bump()?.span;
        if self.at_generic_close() {
            return Err(self.error("generic parameter lists cannot be empty"));
        }
        let mut parameters = Vec::new();
        loop {
            let parameter_start = self.current.span;
            let kind = if let Some(name) = self.take_lifetime()? {
                if system_only {
                    return Err(Diagnostic::at(
                        "PARSE001",
                        parameter_start,
                        "system generics cannot declare lifetimes",
                    ));
                }
                let outlives = if self.consume_punct(Punctuation::Colon)? {
                    Some(
                        self.take_lifetime()?
                            .ok_or_else(|| self.error("expected lifetime after `:`"))?,
                    )
                } else {
                    None
                };
                AstGenericParameterKind::Lifetime { name, outlives }
            } else if self.consume_keyword(Keyword::Const)? {
                let (name, _) = self.identifier("expected const generic name")?;
                self.expect_punct(Punctuation::Colon, "expected `:` after const generic name")?;
                let ty = self.parse_integer_type()?;
                AstGenericParameterKind::IntegerConst { name, ty }
            } else {
                let (name, _) = self.identifier("expected generic parameter")?;
                let bounds = if self.consume_punct(Punctuation::Colon)? {
                    self.parse_type_bounds()?
                } else {
                    Vec::new()
                };
                AstGenericParameterKind::Type { name, bounds }
            };
            parameters.push(AstGenericParameter {
                kind,
                span: self.finish_span(parameter_start),
            });
            if !self.consume_punct(Punctuation::Comma)? {
                break;
            }
            if self.at_generic_close() {
                break;
            }
        }
        let end = self.expect_generic_close()?;
        Ok(Some(AstGenericParameters {
            parameters,
            span: start.join(end),
        }))
    }

    fn parse_generic_arguments(
        &mut self,
        turbofish: bool,
    ) -> Result<AstGenericArguments, Diagnostic> {
        let start = self.expect_punct(Punctuation::Less, "expected `<` for generic arguments")?;
        if self.at_generic_close() {
            return Err(self.error("generic argument lists cannot be empty"));
        }
        let mut arguments = Vec::new();
        loop {
            let argument_start = self.current.span;
            let kind = if let Some(lifetime) = self.take_lifetime()? {
                AstGenericArgumentKind::Lifetime(lifetime)
            } else if self.consume_keyword(Keyword::Const)? {
                AstGenericArgumentKind::IntegerConst(self.parse_const_expression()?)
            } else {
                AstGenericArgumentKind::Type(self.parse_type()?)
            };
            arguments.push(AstGenericArgument {
                kind,
                span: self.finish_span(argument_start),
            });
            if !self.consume_punct(Punctuation::Comma)? {
                break;
            }
            if self.at_generic_close() {
                break;
            }
        }
        let end = self.expect_generic_close()?;
        Ok(AstGenericArguments {
            arguments,
            turbofish,
            span: start.join(end),
        })
    }

    fn parse_where_clause(&mut self) -> Result<Option<AstWhereClause>, Diagnostic> {
        if !self.at_keyword(Keyword::Where) {
            return Ok(None);
        }
        let start = self.bump()?.span;
        let mut predicates = Vec::new();
        loop {
            let predicate_start = self.current.span;
            let kind = if let Some(lifetime) = self.take_lifetime()? {
                self.expect_punct(Punctuation::Colon, "expected `:` in lifetime predicate")?;
                let outlives = self
                    .take_lifetime()?
                    .ok_or_else(|| self.error("expected lifetime after `:`"))?;
                AstWherePredicateKind::Lifetime { lifetime, outlives }
            } else {
                let ty = self.parse_type()?;
                self.expect_punct(Punctuation::Colon, "expected `:` in type predicate")?;
                let bounds = self.parse_type_bounds()?;
                AstWherePredicateKind::Type {
                    ty: Box::new(ty),
                    bounds,
                }
            };
            predicates.push(AstWherePredicate {
                kind,
                span: self.finish_span(predicate_start),
            });
            if !self.consume_punct(Punctuation::Comma)? {
                break;
            }
            if self.starts_where_terminator() {
                break;
            }
        }
        Ok(Some(AstWhereClause {
            predicates,
            span: self.finish_span(start),
        }))
    }

    fn starts_where_terminator(&self) -> bool {
        self.at_punct(Punctuation::LeftBrace)
            || self.at_punct(Punctuation::Semicolon)
            || self.at_punct(Punctuation::Equal)
    }

    fn parse_type_bounds(&mut self) -> Result<Vec<AstTypeBound>, Diagnostic> {
        let mut bounds = vec![self.parse_type_bound()?];
        while self.consume_punct(Punctuation::Plus)? {
            bounds.push(self.parse_type_bound()?);
        }
        Ok(bounds)
    }

    fn parse_type_bound(&mut self) -> Result<AstTypeBound, Diagnostic> {
        let start = self.current.span;
        let kind = if let Some(lifetime) = self.take_lifetime()? {
            AstTypeBoundKind::Lifetime(lifetime)
        } else {
            let mut path = self.parse_path(false)?;
            self.validate_rooted_or_bound_path(&path, "type bound")?;
            if self.at_punct(Punctuation::Less) {
                let arguments = self.parse_generic_arguments(false)?;
                path.span = path.span.join(arguments.span);
                path.generic_arguments = Some(arguments);
            }
            AstTypeBoundKind::Trait(path)
        };
        Ok(AstTypeBound {
            kind,
            span: self.finish_span(start),
        })
    }

    fn parse_effect_sets(&mut self) -> Result<AstEffectSets, Diagnostic> {
        let start = self.current.span;
        let requires = if self.consume_keyword(Keyword::Requires)? {
            let set_start = self.previous.expect("`requires` was consumed");
            self.expect_punct(Punctuation::LeftBrace, "expected `{` after `requires`")?;
            let mut members = Vec::new();
            if !self.at_punct(Punctuation::RightBrace) {
                loop {
                    let path = self.parse_path(false)?;
                    self.validate_rooted_or_bound_path(&path, "required capability")?;
                    members.push(path);
                    if !self.consume_punct(Punctuation::Comma)? {
                        break;
                    }
                    if self.at_punct(Punctuation::RightBrace) {
                        break;
                    }
                }
            }
            let end = self.expect_punct(
                Punctuation::RightBrace,
                "expected `}` after required capabilities",
            )?;
            Some(AstEffectSet {
                members,
                span: set_start.join(end),
            })
        } else {
            None
        };
        let throws = if self.consume_keyword(Keyword::Throws)? {
            let set_start = self.previous.expect("`throws` was consumed");
            self.expect_punct(Punctuation::LeftBrace, "expected `{` after `throws`")?;
            let members = self.parse_type_list(Punctuation::RightBrace)?;
            let end =
                self.expect_punct(Punctuation::RightBrace, "expected `}` after thrown types")?;
            Some(AstEffectSet {
                members,
                span: set_start.join(end),
            })
        } else {
            None
        };
        let span = if requires.is_some() || throws.is_some() {
            Some(self.finish_span(start))
        } else {
            None
        };
        Ok(AstEffectSets {
            requires,
            throws,
            span,
        })
    }

    fn parse_type_list(&mut self, close: Punctuation) -> Result<Vec<AstType>, Diagnostic> {
        let mut types = Vec::new();
        if self.at_punct(close) {
            return Ok(types);
        }
        loop {
            types.push(self.parse_type()?);
            if !self.consume_punct(Punctuation::Comma)? {
                break;
            }
            if self.at_punct(close) {
                break;
            }
        }
        Ok(types)
    }

    fn parse_type(&mut self) -> Result<AstType, Diagnostic> {
        let start = self.current.span;
        let kind = if let Some(scalar) = self.take_scalar_type()? {
            AstTypeKind::Scalar(scalar)
        } else if self.consume_punct(Punctuation::Bang)? {
            AstTypeKind::Never
        } else if self.consume_keyword(Keyword::Str)? {
            AstTypeKind::Str
        } else if self.at_keyword(Keyword::SelfType) {
            let mut path = self.parse_path(true)?;
            self.validate_rooted_or_bound_path(&path, "type")?;
            if self.at_punct(Punctuation::Less) {
                let arguments = self.parse_generic_arguments(false)?;
                path.span = path.span.join(arguments.span);
                path.generic_arguments = Some(arguments);
            }
            if path.segments.is_empty() && path.generic_arguments.is_none() {
                AstTypeKind::SelfType
            } else {
                AstTypeKind::Path(path)
            }
        } else if self.consume_punct(Punctuation::LeftParen)? {
            if self.consume_punct(Punctuation::RightParen)? {
                AstTypeKind::Unit
            } else {
                let first = self.parse_type()?;
                self.expect_punct(
                    Punctuation::Comma,
                    "a parenthesized type must be a tuple and include `,`",
                )?;
                let mut elements = vec![first];
                if self.at_punct(Punctuation::Comma) {
                    return Err(self.error("tuple type contains an empty element"));
                }
                if !self.at_punct(Punctuation::RightParen) {
                    loop {
                        elements.push(self.parse_type()?);
                        if !self.consume_punct(Punctuation::Comma)? {
                            break;
                        }
                        if self.at_punct(Punctuation::RightParen) {
                            break;
                        }
                        if self.at_punct(Punctuation::Comma) {
                            return Err(self.error("tuple type contains an empty element"));
                        }
                    }
                }
                self.expect_punct(Punctuation::RightParen, "expected `)` after tuple type")?;
                AstTypeKind::Tuple(elements)
            }
        } else if self.consume_punct(Punctuation::LeftBracket)? {
            let element = self.parse_type()?;
            if self.consume_punct(Punctuation::Semicolon)? {
                let length = self.parse_const_expression()?;
                self.expect_punct(Punctuation::RightBracket, "expected `]` after array type")?;
                AstTypeKind::Array {
                    element: Box::new(element),
                    length,
                }
            } else {
                self.expect_punct(Punctuation::RightBracket, "expected `]` after slice type")?;
                AstTypeKind::Slice(Box::new(element))
            }
        } else if self.consume_punct(Punctuation::Ampersand)? {
            let lifetime = self.take_lifetime()?;
            let mutable = self.consume_keyword(Keyword::Mut)?;
            let pointee = self.parse_type()?;
            AstTypeKind::Reference {
                lifetime,
                mutable,
                pointee: Box::new(pointee),
            }
        } else if self.consume_punct(Punctuation::Star)? {
            let mutable = if self.consume_keyword(Keyword::Const)? {
                false
            } else if self.consume_keyword(Keyword::Mut)? {
                true
            } else {
                return Err(self.error("expected `const` or `mut` after `*` in pointer type"));
            };
            let pointee = self.parse_type()?;
            AstTypeKind::RawPointer {
                mutable,
                pointee: Box::new(pointee),
            }
        } else if self.at_keyword(Keyword::Fn)
            || (self.at_keyword(Keyword::Unsafe) && self.next_is_keyword(Keyword::Fn))
        {
            let unsafe_ = self.consume_keyword(Keyword::Unsafe)?;
            self.expect_keyword(Keyword::Fn, "expected `fn` in function-pointer type")?;
            self.expect_punct(
                Punctuation::LeftParen,
                "expected `(` in function-pointer type",
            )?;
            let parameters = self.parse_type_list(Punctuation::RightParen)?;
            self.expect_punct(
                Punctuation::RightParen,
                "expected `)` after function-pointer parameters",
            )?;
            let effects = self.parse_effect_sets()?;
            let result = if self.consume_punct(Punctuation::Arrow)? {
                Some(Box::new(self.parse_type()?))
            } else {
                None
            };
            AstTypeKind::FunctionPointer {
                unsafe_,
                parameters,
                effects,
                result,
            }
        } else if self.starts_path() {
            let mut path = self.parse_path(false)?;
            self.validate_rooted_or_bound_path(&path, "type")?;
            if self.at_punct(Punctuation::Less) {
                let arguments = self.parse_generic_arguments(false)?;
                path.span = path.span.join(arguments.span);
                path.generic_arguments = Some(arguments);
            }
            AstTypeKind::Path(path)
        } else {
            return Err(self.error("expected a type"));
        };
        Ok(AstNode::new(kind, self.finish_span(start)))
    }

    fn parse_integer_type(&mut self) -> Result<AstIntegerType, Diagnostic> {
        let value = match self.current.kind {
            TokenKind::Keyword(Keyword::I8) => AstIntegerType::I8,
            TokenKind::Keyword(Keyword::I16) => AstIntegerType::I16,
            TokenKind::Keyword(Keyword::I32) => AstIntegerType::I32,
            TokenKind::Keyword(Keyword::I64) => AstIntegerType::I64,
            TokenKind::Keyword(Keyword::Isize) => AstIntegerType::Isize,
            TokenKind::Keyword(Keyword::U8) => AstIntegerType::U8,
            TokenKind::Keyword(Keyword::U16) => AstIntegerType::U16,
            TokenKind::Keyword(Keyword::U32) => AstIntegerType::U32,
            TokenKind::Keyword(Keyword::U64) => AstIntegerType::U64,
            TokenKind::Keyword(Keyword::Usize) => AstIntegerType::Usize,
            _ => return Err(self.error("expected an integer type")),
        };
        self.bump()?;
        Ok(value)
    }

    fn take_scalar_type(&mut self) -> Result<Option<AstScalarType>, Diagnostic> {
        let scalar = match self.current.kind {
            TokenKind::Keyword(Keyword::I8) => AstScalarType::Integer(AstIntegerType::I8),
            TokenKind::Keyword(Keyword::I16) => AstScalarType::Integer(AstIntegerType::I16),
            TokenKind::Keyword(Keyword::I32) => AstScalarType::Integer(AstIntegerType::I32),
            TokenKind::Keyword(Keyword::I64) => AstScalarType::Integer(AstIntegerType::I64),
            TokenKind::Keyword(Keyword::Isize) => AstScalarType::Integer(AstIntegerType::Isize),
            TokenKind::Keyword(Keyword::U8) => AstScalarType::Integer(AstIntegerType::U8),
            TokenKind::Keyword(Keyword::U16) => AstScalarType::Integer(AstIntegerType::U16),
            TokenKind::Keyword(Keyword::U32) => AstScalarType::Integer(AstIntegerType::U32),
            TokenKind::Keyword(Keyword::U64) => AstScalarType::Integer(AstIntegerType::U64),
            TokenKind::Keyword(Keyword::Usize) => AstScalarType::Integer(AstIntegerType::Usize),
            TokenKind::Keyword(Keyword::F32) => AstScalarType::F32,
            TokenKind::Keyword(Keyword::F64) => AstScalarType::F64,
            TokenKind::Keyword(Keyword::Bool) => AstScalarType::Bool,
            TokenKind::Keyword(Keyword::Char) => AstScalarType::Char,
            TokenKind::Keyword(Keyword::Entity) => AstScalarType::Entity,
            _ => return Ok(None),
        };
        self.bump()?;
        Ok(Some(scalar))
    }

    fn parse_block(&mut self) -> Result<AstBlock, Diagnostic> {
        let start = self.expect_punct(Punctuation::LeftBrace, "expected `{` to start block")?;
        let mut statements = Vec::new();
        let mut tail = None;
        while !self.at_punct(Punctuation::RightBrace) {
            if self.at_eof() {
                return Err(self.error("expected `}` to close block"));
            }
            if self.at_keyword(Keyword::Let) {
                statements.push(self.parse_let_statement()?);
                continue;
            }
            if self.at_keyword(Keyword::For) {
                statements.push(self.parse_for_statement()?);
                continue;
            }
            let expression = self.parse_expression()?;
            let expression_start = expression.span;
            if self.at_punct(Punctuation::Equal) || self.at_punct(Punctuation::AddAssign) {
                if !Self::is_place_expression(&expression) {
                    return Err(Diagnostic::at(
                        "PARSE001",
                        expression.span,
                        "assignment left side must be a place expression",
                    ));
                }
                let operator = if self.consume_punct(Punctuation::Equal)? {
                    AstAssignmentOperator::Assign
                } else {
                    self.expect_punct(Punctuation::AddAssign, "expected assignment operator")?;
                    AstAssignmentOperator::AddAssign
                };
                let value = self.parse_expression()?;
                let end = self.expect_punct(
                    Punctuation::Semicolon,
                    "expected `;` after assignment statement",
                )?;
                statements.push(AstNode::new(
                    AstStatementKind::Assignment {
                        place: Box::new(expression),
                        operator,
                        value: Box::new(value),
                    },
                    expression_start.join(end),
                ));
            } else if let Some(end) = self.take_punct(Punctuation::Semicolon)? {
                statements.push(AstNode::new(
                    AstStatementKind::Expression {
                        expression: Box::new(expression),
                        semicolon: true,
                    },
                    expression_start.join(end),
                ));
            } else if self.at_punct(Punctuation::RightBrace) {
                tail = Some(Box::new(expression));
                break;
            } else if Self::is_block_control_expression(&expression) {
                statements.push(AstNode::new(
                    AstStatementKind::Expression {
                        expression: Box::new(expression),
                        semicolon: false,
                    },
                    expression_start,
                ));
            } else {
                return Err(self.error("expected `;` after non-final expression"));
            }
        }
        let end = self.expect_punct(Punctuation::RightBrace, "expected `}` to close block")?;
        Ok(AstBlock {
            statements,
            tail,
            span: start.join(end),
        })
    }

    fn parse_let_statement(&mut self) -> Result<AstStatement, Diagnostic> {
        let start = self.expect_keyword(Keyword::Let, "expected `let`")?;
        let pattern = self.parse_pattern()?;
        let ty = if self.consume_punct(Punctuation::Colon)? {
            Some(Box::new(self.parse_type()?))
        } else {
            None
        };
        self.expect_punct(Punctuation::Equal, "expected `=` in let statement")?;
        let value = self.parse_expression()?;
        let else_block = if self.consume_keyword(Keyword::Else)? {
            Some(Box::new(self.parse_block()?))
        } else {
            None
        };
        let end = self.expect_punct(Punctuation::Semicolon, "expected `;` after let statement")?;
        Ok(AstNode::new(
            AstStatementKind::Let {
                pattern: Box::new(pattern),
                ty,
                value: Box::new(value),
                else_block,
            },
            start.join(end),
        ))
    }

    fn parse_for_statement(&mut self) -> Result<AstStatement, Diagnostic> {
        let start = self.expect_keyword(Keyword::For, "expected `for`")?;
        let pattern = self.parse_pattern()?;
        self.expect_keyword(Keyword::In, "expected `in` in for statement")?;
        let iterator = self.parse_expression_mode(false)?;
        let body = self.parse_block()?;
        let semicolon = self.consume_punct(Punctuation::Semicolon)?;
        Ok(AstNode::new(
            AstStatementKind::For {
                pattern: Box::new(pattern),
                iterator: Box::new(iterator),
                body: Box::new(body),
                semicolon,
            },
            self.finish_span(start),
        ))
    }

    fn is_place_expression(expression: &AstExpression) -> bool {
        match &expression.kind {
            AstExpressionKind::Path(_) | AstExpressionKind::SelfValue => true,
            AstExpressionKind::Postfix { .. } => true,
            AstExpressionKind::Unary {
                operator: AstUnaryOperator::Dereference,
                ..
            } => true,
            AstExpressionKind::Group(inner) => Self::is_place_expression(inner),
            _ => false,
        }
    }

    fn is_block_control_expression(expression: &AstExpression) -> bool {
        matches!(
            expression.kind,
            AstExpressionKind::Block(_)
                | AstExpressionKind::If(_)
                | AstExpressionKind::While(_)
                | AstExpressionKind::Loop(_)
                | AstExpressionKind::Match { .. }
                | AstExpressionKind::Catch { .. }
                | AstExpressionKind::Unsafe(_)
        )
    }

    fn parse_expression(&mut self) -> Result<AstExpression, Diagnostic> {
        self.parse_expression_mode(true)
    }

    fn parse_expression_mode(&mut self, allow_record: bool) -> Result<AstExpression, Diagnostic> {
        self.parse_logical_or(allow_record)
    }

    fn parse_logical_or(&mut self, allow_record: bool) -> Result<AstExpression, Diagnostic> {
        let mut left = self.parse_logical_and(allow_record)?;
        while self.consume_punct(Punctuation::LogicalOr)? {
            let right = self.parse_logical_and(allow_record)?;
            left = Self::binary(AstBinaryOperator::LogicalOr, left, right);
        }
        Ok(left)
    }

    fn parse_logical_and(&mut self, allow_record: bool) -> Result<AstExpression, Diagnostic> {
        let mut left = self.parse_bit_or(allow_record)?;
        while self.consume_punct(Punctuation::LogicalAnd)? {
            let right = self.parse_bit_or(allow_record)?;
            left = Self::binary(AstBinaryOperator::LogicalAnd, left, right);
        }
        Ok(left)
    }

    fn parse_bit_or(&mut self, allow_record: bool) -> Result<AstExpression, Diagnostic> {
        let mut left = self.parse_bit_xor(allow_record)?;
        while self.consume_punct(Punctuation::Pipe)? {
            let right = self.parse_bit_xor(allow_record)?;
            left = Self::binary(AstBinaryOperator::BitOr, left, right);
        }
        Ok(left)
    }

    fn parse_bit_xor(&mut self, allow_record: bool) -> Result<AstExpression, Diagnostic> {
        let mut left = self.parse_bit_and(allow_record)?;
        while self.consume_punct(Punctuation::Caret)? {
            let right = self.parse_bit_and(allow_record)?;
            left = Self::binary(AstBinaryOperator::BitXor, left, right);
        }
        Ok(left)
    }

    fn parse_bit_and(&mut self, allow_record: bool) -> Result<AstExpression, Diagnostic> {
        let mut left = self.parse_equality(allow_record)?;
        while self.consume_punct(Punctuation::Ampersand)? {
            let right = self.parse_equality(allow_record)?;
            left = Self::binary(AstBinaryOperator::BitAnd, left, right);
        }
        Ok(left)
    }

    fn parse_equality(&mut self, allow_record: bool) -> Result<AstExpression, Diagnostic> {
        let left = self.parse_relational(allow_record)?;
        let operator = if self.consume_punct(Punctuation::EqualEqual)? {
            Some(AstBinaryOperator::Equal)
        } else if self.consume_punct(Punctuation::NotEqual)? {
            Some(AstBinaryOperator::NotEqual)
        } else {
            None
        };
        if let Some(operator) = operator {
            let right = self.parse_relational(allow_record)?;
            Ok(Self::binary(operator, left, right))
        } else {
            Ok(left)
        }
    }

    fn parse_relational(&mut self, allow_record: bool) -> Result<AstExpression, Diagnostic> {
        let left = self.parse_shift(allow_record)?;
        let operator = if self.consume_punct(Punctuation::Less)? {
            Some(AstBinaryOperator::Less)
        } else if self.consume_punct(Punctuation::LessEqual)? {
            Some(AstBinaryOperator::LessEqual)
        } else if self.consume_punct(Punctuation::Greater)? {
            Some(AstBinaryOperator::Greater)
        } else if self.consume_punct(Punctuation::GreaterEqual)? {
            Some(AstBinaryOperator::GreaterEqual)
        } else {
            None
        };
        if let Some(operator) = operator {
            let right = self.parse_shift(allow_record)?;
            Ok(Self::binary(operator, left, right))
        } else {
            Ok(left)
        }
    }

    fn parse_shift(&mut self, allow_record: bool) -> Result<AstExpression, Diagnostic> {
        let mut left = self.parse_additive(allow_record)?;
        loop {
            let operator = if self.consume_punct(Punctuation::ShiftLeft)? {
                Some(AstBinaryOperator::ShiftLeft)
            } else if self.consume_punct(Punctuation::ShiftRight)? {
                Some(AstBinaryOperator::ShiftRight)
            } else {
                None
            };
            let Some(operator) = operator else { break };
            let right = self.parse_additive(allow_record)?;
            left = Self::binary(operator, left, right);
        }
        Ok(left)
    }

    fn parse_additive(&mut self, allow_record: bool) -> Result<AstExpression, Diagnostic> {
        let mut left = self.parse_multiplicative(allow_record)?;
        loop {
            let operator = if self.consume_punct(Punctuation::Plus)? {
                Some(AstBinaryOperator::Add)
            } else if self.consume_punct(Punctuation::Minus)? {
                Some(AstBinaryOperator::Subtract)
            } else {
                None
            };
            let Some(operator) = operator else { break };
            let right = self.parse_multiplicative(allow_record)?;
            left = Self::binary(operator, left, right);
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self, allow_record: bool) -> Result<AstExpression, Diagnostic> {
        let mut left = self.parse_cast(allow_record)?;
        loop {
            let operator = if self.consume_punct(Punctuation::Star)? {
                Some(AstBinaryOperator::Multiply)
            } else if self.consume_punct(Punctuation::Slash)? {
                Some(AstBinaryOperator::Divide)
            } else if self.consume_punct(Punctuation::Percent)? {
                Some(AstBinaryOperator::Remainder)
            } else {
                None
            };
            let Some(operator) = operator else { break };
            let right = self.parse_cast(allow_record)?;
            left = Self::binary(operator, left, right);
        }
        Ok(left)
    }

    fn parse_cast(&mut self, allow_record: bool) -> Result<AstExpression, Diagnostic> {
        let mut value = self.parse_unary(allow_record)?;
        while self.consume_keyword(Keyword::As)? {
            let ty = self.parse_type()?;
            let span = value.span.join(ty.span);
            value = AstNode::new(
                AstExpressionKind::Cast {
                    value: Box::new(value),
                    ty,
                },
                span,
            );
        }
        Ok(value)
    }

    fn parse_unary(&mut self, allow_record: bool) -> Result<AstExpression, Diagnostic> {
        let start = self.current.span;
        let operator = if self.consume_punct(Punctuation::Minus)? {
            Some(AstUnaryOperator::Negate)
        } else if self.consume_punct(Punctuation::Bang)? {
            Some(AstUnaryOperator::LogicalNot)
        } else if self.consume_punct(Punctuation::Tilde)? {
            Some(AstUnaryOperator::BitNot)
        } else if self.consume_punct(Punctuation::Star)? {
            Some(AstUnaryOperator::Dereference)
        } else if self.consume_punct(Punctuation::Ampersand)? {
            if self.consume_keyword(Keyword::Mut)? {
                Some(AstUnaryOperator::BorrowMutable)
            } else {
                Some(AstUnaryOperator::BorrowShared)
            }
        } else {
            None
        };
        if let Some(operator) = operator {
            let operand = self.parse_unary(allow_record)?;
            let span = start.join(operand.span);
            Ok(AstNode::new(
                AstExpressionKind::Unary {
                    operator,
                    operand: Box::new(operand),
                },
                span,
            ))
        } else {
            self.parse_postfix(allow_record)
        }
    }

    fn parse_postfix(&mut self, allow_record: bool) -> Result<AstExpression, Diagnostic> {
        let mut expression = self.parse_primary(allow_record)?;
        let mut parts = Vec::new();
        loop {
            let part_start = self.current.span;
            let kind = if self.consume_punct(Punctuation::LeftParen)? {
                let arguments = self.parse_expression_list(Punctuation::RightParen)?;
                self.expect_punct(Punctuation::RightParen, "expected `)` after arguments")?;
                AstPostfixKind::Call(arguments)
            } else if self.consume_punct(Punctuation::LeftBracket)? {
                let index = self.parse_expression()?;
                self.expect_punct(Punctuation::RightBracket, "expected `]` after index")?;
                AstPostfixKind::Index(index)
            } else if self.consume_punct(Punctuation::Dot)? {
                if self.at_keyword(Keyword::Spawn) && self.next_is_punct(Punctuation::LeftBrace) {
                    self.bump()?;
                    self.expect_punct(Punctuation::LeftBrace, "expected `{` after `.spawn`")?;
                    let values = self.parse_expression_list(Punctuation::RightBrace)?;
                    self.expect_punct(Punctuation::RightBrace, "expected `}` after spawn payload")?;
                    AstPostfixKind::CommandSpawn(values)
                } else if self.at_keyword(Keyword::Resume)
                    && self.next_is_punct(Punctuation::LeftParen)
                {
                    self.bump()?;
                    self.expect_punct(Punctuation::LeftParen, "expected `(` after `.resume`")?;
                    let value = self.parse_expression()?;
                    self.expect_punct(Punctuation::RightParen, "expected `)` after resume value")?;
                    AstPostfixKind::Resume(value)
                } else if matches!(self.current.kind, TokenKind::Integer(_)) {
                    let TokenKind::Integer(literal) = self.bump()?.kind else {
                        unreachable!()
                    };
                    AstPostfixKind::TupleField(literal)
                } else {
                    let method_or_field = self.parse_method_name("expected member after `.`")?;
                    let has_turbofish = self.at_punct(Punctuation::ColonColon)
                        && self.next_is_punct(Punctuation::Less);
                    if has_turbofish || self.at_punct(Punctuation::LeftParen) {
                        let generic_arguments = if has_turbofish {
                            self.bump()?;
                            Some(self.parse_generic_arguments(true)?)
                        } else {
                            None
                        };
                        self.expect_punct(
                            Punctuation::LeftParen,
                            "expected `(` after method name",
                        )?;
                        let arguments = self.parse_expression_list(Punctuation::RightParen)?;
                        self.expect_punct(
                            Punctuation::RightParen,
                            "expected `)` after method call",
                        )?;
                        AstPostfixKind::Method {
                            name: method_or_field,
                            generic_arguments,
                            arguments,
                        }
                    } else if let AstMethodName::Identifier(name) = method_or_field {
                        AstPostfixKind::Field(name)
                    } else {
                        return Err(self.error("contextual method keyword must be followed by `(`"));
                    }
                }
            } else if self.at_punct(Punctuation::ColonColon)
                && self.next_is_punct(Punctuation::Less)
            {
                self.bump()?;
                let generic_arguments = self.parse_generic_arguments(true)?;
                if self.at_punct(Punctuation::ColonColon)
                    && matches!(self.next.kind, TokenKind::Identifier(_))
                {
                    if !parts.is_empty() {
                        expression = Self::wrap_postfix(expression, std::mem::take(&mut parts));
                    }
                    let AstExpressionKind::Path(mut path) = expression.kind else {
                        return Err(Diagnostic::at(
                            "PARSE001",
                            expression.span,
                            "generic path continuation requires a value path",
                        ));
                    };
                    if let Some(segment) = path.segments.last_mut() {
                        segment.generic_arguments = Some(generic_arguments);
                    } else {
                        path.generic_arguments = Some(generic_arguments);
                    }
                    self.bump()?;
                    let (name, span) =
                        self.identifier("expected path segment after generic arguments")?;
                    path.segments.push(AstPathSegment {
                        name,
                        generic_arguments: None,
                        span,
                    });
                    while self.at_punct(Punctuation::ColonColon)
                        && matches!(self.next.kind, TokenKind::Identifier(_))
                    {
                        self.bump()?;
                        let (name, span) = self.identifier("expected path segment after `::`")?;
                        path.segments.push(AstPathSegment {
                            name,
                            generic_arguments: None,
                            span,
                        });
                    }
                    path.span = path
                        .span
                        .join(self.previous.expect("path segment consumed"));
                    let full_span = path.span;
                    if allow_record && self.at_punct(Punctuation::LeftBrace) {
                        let fields = self.parse_record_expression_fields()?;
                        expression = AstNode::new(
                            AstExpressionKind::Record {
                                constructor: path,
                                fields,
                            },
                            full_span.join(self.previous.expect("record closed")),
                        );
                    } else {
                        expression = AstNode::new(AstExpressionKind::Path(path), full_span);
                    }
                    continue;
                }
                if self.at_punct(Punctuation::LeftBrace) && allow_record {
                    if !parts.is_empty() {
                        expression = Self::wrap_postfix(expression, std::mem::take(&mut parts));
                    }
                    let AstExpressionKind::Path(mut constructor) = expression.kind else {
                        return Err(Diagnostic::at(
                            "PARSE001",
                            expression.span,
                            "record constructor must be a value path",
                        ));
                    };
                    constructor.span = constructor.span.join(generic_arguments.span);
                    constructor.generic_arguments = Some(generic_arguments);
                    let fields = self.parse_record_expression_fields()?;
                    expression = AstNode::new(
                        AstExpressionKind::Record {
                            constructor,
                            fields,
                        },
                        expression.span.join(self.previous.expect("record closed")),
                    );
                    continue;
                }
                self.expect_punct(
                    Punctuation::LeftParen,
                    "expected `(` after turbofish generic arguments",
                )?;
                let arguments = self.parse_expression_list(Punctuation::RightParen)?;
                self.expect_punct(Punctuation::RightParen, "expected `)` after arguments")?;
                AstPostfixKind::TurbofishCall {
                    generic_arguments,
                    arguments,
                }
            } else {
                break;
            };
            parts.push(AstPostfix {
                kind,
                span: self.finish_span(part_start),
            });
        }
        if parts.is_empty() {
            Ok(expression)
        } else {
            Ok(Self::wrap_postfix(expression, parts))
        }
    }

    fn wrap_postfix(base: AstExpression, parts: Vec<AstPostfix>) -> AstExpression {
        let end = parts.last().map_or(base.span, |part| part.span);
        let span = base.span.join(end);
        AstNode::new(
            AstExpressionKind::Postfix {
                base: Box::new(base),
                parts,
            },
            span,
        )
    }

    fn binary(
        operator: AstBinaryOperator,
        left: AstExpression,
        right: AstExpression,
    ) -> AstExpression {
        let span = left.span.join(right.span);
        AstNode::new(
            AstExpressionKind::Binary {
                operator,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        )
    }

    fn parse_primary(&mut self, allow_record: bool) -> Result<AstExpression, Diagnostic> {
        let start = self.current.span;
        let kind = match self.current.kind.clone() {
            TokenKind::Integer(literal) => {
                self.bump()?;
                AstExpressionKind::Literal(AstLiteral::Integer(literal))
            }
            TokenKind::Float(literal) => {
                self.bump()?;
                AstExpressionKind::Literal(AstLiteral::Float(literal))
            }
            TokenKind::Character(value) => {
                self.bump()?;
                AstExpressionKind::Literal(AstLiteral::Character(value))
            }
            TokenKind::String(value) => {
                self.bump()?;
                AstExpressionKind::Literal(AstLiteral::String(value))
            }
            TokenKind::Keyword(Keyword::True) => {
                self.bump()?;
                AstExpressionKind::Literal(AstLiteral::Boolean(true))
            }
            TokenKind::Keyword(Keyword::False) => {
                self.bump()?;
                AstExpressionKind::Literal(AstLiteral::Boolean(false))
            }
            TokenKind::Punctuation(Punctuation::LeftParen) => {
                self.bump()?;
                if self.consume_punct(Punctuation::RightParen)? {
                    AstExpressionKind::Unit
                } else {
                    let first = self.parse_expression()?;
                    if self.consume_punct(Punctuation::Comma)? {
                        let mut elements = vec![first];
                        if self.at_punct(Punctuation::Comma) {
                            return Err(self.error("tuple expression contains an empty element"));
                        }
                        if !self.at_punct(Punctuation::RightParen) {
                            loop {
                                elements.push(self.parse_expression()?);
                                if !self.consume_punct(Punctuation::Comma)? {
                                    break;
                                }
                                if self.at_punct(Punctuation::RightParen) {
                                    break;
                                }
                                if self.at_punct(Punctuation::Comma) {
                                    return Err(
                                        self.error("tuple expression contains an empty element")
                                    );
                                }
                            }
                        }
                        self.expect_punct(
                            Punctuation::RightParen,
                            "expected `)` after tuple expression",
                        )?;
                        AstExpressionKind::Tuple(elements)
                    } else {
                        self.expect_punct(
                            Punctuation::RightParen,
                            "expected `)` after grouped expression",
                        )?;
                        AstExpressionKind::Group(Box::new(first))
                    }
                }
            }
            TokenKind::Punctuation(Punctuation::LeftBracket) => {
                self.bump()?;
                if self.consume_punct(Punctuation::RightBracket)? {
                    AstExpressionKind::Array(Vec::new())
                } else {
                    let first = self.parse_expression()?;
                    if self.consume_punct(Punctuation::Semicolon)? {
                        let count = self.parse_const_expression()?;
                        self.expect_punct(
                            Punctuation::RightBracket,
                            "expected `]` after array repeat",
                        )?;
                        AstExpressionKind::ArrayRepeat {
                            value: Box::new(first),
                            count,
                        }
                    } else {
                        let mut values = vec![first];
                        while self.consume_punct(Punctuation::Comma)? {
                            if self.at_punct(Punctuation::RightBracket) {
                                break;
                            }
                            values.push(self.parse_expression()?);
                        }
                        self.expect_punct(
                            Punctuation::RightBracket,
                            "expected `]` after array expression",
                        )?;
                        AstExpressionKind::Array(values)
                    }
                }
            }
            TokenKind::Punctuation(Punctuation::LeftBrace) => {
                AstExpressionKind::Block(self.parse_block()?)
            }
            TokenKind::Keyword(Keyword::If) => return self.parse_if_expression(),
            TokenKind::Keyword(Keyword::While) => return self.parse_while_expression(),
            TokenKind::Keyword(Keyword::Loop) => {
                self.bump()?;
                AstExpressionKind::Loop(self.parse_block()?)
            }
            TokenKind::Keyword(Keyword::Match) => return self.parse_match_expression(false),
            TokenKind::Keyword(Keyword::Catch) => return self.parse_match_expression(true),
            TokenKind::Keyword(Keyword::Unsafe) => {
                self.bump()?;
                AstExpressionKind::Unsafe(self.parse_block()?)
            }
            TokenKind::Keyword(Keyword::Move)
            | TokenKind::Punctuation(Punctuation::Pipe)
            | TokenKind::Punctuation(Punctuation::LogicalOr) => return self.parse_closure(false),
            TokenKind::Keyword(Keyword::Gen) => return self.parse_closure(true),
            TokenKind::Keyword(Keyword::Return) => {
                self.bump()?;
                let value = if self.starts_expression() {
                    Some(Box::new(self.parse_expression()?))
                } else {
                    None
                };
                AstExpressionKind::Return(value)
            }
            TokenKind::Keyword(Keyword::Break) => {
                self.bump()?;
                let value = if self.starts_expression() {
                    Some(Box::new(self.parse_expression()?))
                } else {
                    None
                };
                AstExpressionKind::Break(value)
            }
            TokenKind::Keyword(Keyword::Continue) => {
                self.bump()?;
                AstExpressionKind::Continue
            }
            TokenKind::Keyword(Keyword::Throw) => {
                self.bump()?;
                let value = if self.starts_expression() {
                    Some(Box::new(self.parse_expression()?))
                } else {
                    None
                };
                AstExpressionKind::Throw(value)
            }
            TokenKind::Keyword(Keyword::Yield) => {
                self.bump()?;
                AstExpressionKind::Yield(Box::new(self.parse_expression()?))
            }
            TokenKind::Keyword(Keyword::SelfValue)
                if !self.next_is_punct(Punctuation::ColonColon) =>
            {
                self.bump()?;
                AstExpressionKind::SelfValue
            }
            _ if self.starts_path() => {
                let path = self.parse_path(false)?;
                if allow_record && self.at_punct(Punctuation::LeftBrace) {
                    let fields = self.parse_record_expression_fields()?;
                    AstExpressionKind::Record {
                        constructor: path,
                        fields,
                    }
                } else {
                    AstExpressionKind::Path(path)
                }
            }
            _ => return Err(self.error("expected an expression")),
        };
        Ok(AstNode::new(kind, self.finish_span(start)))
    }

    fn parse_expression_list(
        &mut self,
        close: Punctuation,
    ) -> Result<Vec<AstExpression>, Diagnostic> {
        let mut values = Vec::new();
        if self.at_punct(close) {
            return Ok(values);
        }
        loop {
            values.push(self.parse_expression()?);
            if !self.consume_punct(Punctuation::Comma)? {
                break;
            }
            if self.at_punct(close) {
                break;
            }
        }
        Ok(values)
    }

    fn parse_record_expression_fields(
        &mut self,
    ) -> Result<Vec<AstRecordExpressionField>, Diagnostic> {
        self.expect_punct(Punctuation::LeftBrace, "expected `{` in record expression")?;
        let mut fields = Vec::new();
        if !self.at_punct(Punctuation::RightBrace) {
            loop {
                let start = self.current.span;
                let (name, _) = self.identifier("expected record expression field")?;
                self.expect_punct(Punctuation::Colon, "expected `:` after record field")?;
                let value = self.parse_expression()?;
                fields.push(AstRecordExpressionField {
                    name,
                    value,
                    span: self.finish_span(start),
                });
                if !self.consume_punct(Punctuation::Comma)? {
                    break;
                }
                if self.at_punct(Punctuation::RightBrace) {
                    break;
                }
            }
        }
        self.expect_punct(
            Punctuation::RightBrace,
            "expected `}` after record expression",
        )?;
        Ok(fields)
    }

    fn parse_if_expression(&mut self) -> Result<AstExpression, Diagnostic> {
        let start = self.expect_keyword(Keyword::If, "expected `if`")?;
        let condition = self.parse_condition()?;
        let then_block = self.parse_block()?;
        let else_branch = if self.consume_keyword(Keyword::Else)? {
            if self.at_keyword(Keyword::If) {
                Some(AstElseBranch::If(Box::new(self.parse_if_expression()?)))
            } else {
                Some(AstElseBranch::Block(self.parse_block()?))
            }
        } else {
            None
        };
        Ok(AstNode::new(
            AstExpressionKind::If(Box::new(AstIfExpression {
                condition,
                then_block,
                else_branch,
            })),
            self.finish_span(start),
        ))
    }

    fn parse_condition(&mut self) -> Result<AstCondition, Diagnostic> {
        if self.consume_keyword(Keyword::Let)? {
            let pattern = self.parse_pattern()?;
            self.expect_punct(Punctuation::Equal, "expected `=` in let condition")?;
            let value = self.parse_expression_mode(false)?;
            Ok(AstCondition::Let {
                pattern: Box::new(pattern),
                value: Box::new(value),
            })
        } else {
            Ok(AstCondition::Expression(Box::new(
                self.parse_expression_mode(false)?,
            )))
        }
    }

    fn parse_while_expression(&mut self) -> Result<AstExpression, Diagnostic> {
        let start = self.expect_keyword(Keyword::While, "expected `while`")?;
        let condition = self.parse_condition()?;
        let body = self.parse_block()?;
        Ok(AstNode::new(
            AstExpressionKind::While(Box::new(AstWhileExpression { condition, body })),
            self.finish_span(start),
        ))
    }

    fn parse_match_expression(&mut self, catch: bool) -> Result<AstExpression, Diagnostic> {
        let start = if catch {
            self.expect_keyword(Keyword::Catch, "expected `catch`")?
        } else {
            self.expect_keyword(Keyword::Match, "expected `match`")?
        };
        let operand = self.parse_expression_mode(false)?;
        self.expect_punct(Punctuation::LeftBrace, "expected `{` before arms")?;
        let mut arms = Vec::new();
        if !self.at_punct(Punctuation::RightBrace) {
            loop {
                let arm_start = self.current.span;
                let pattern = self.parse_pattern()?;
                let guard = if self.consume_keyword(Keyword::If)? {
                    Some(self.parse_expression()?)
                } else {
                    None
                };
                self.expect_punct(Punctuation::FatArrow, "expected `=>` in arm")?;
                let value = self.parse_expression()?;
                arms.push(AstMatchArm {
                    pattern,
                    guard,
                    value,
                    span: self.finish_span(arm_start),
                });
                if !self.consume_punct(Punctuation::Comma)? {
                    break;
                }
                if self.at_punct(Punctuation::RightBrace) {
                    break;
                }
            }
        }
        self.expect_punct(Punctuation::RightBrace, "expected `}` after arms")?;
        let kind = if catch {
            AstExpressionKind::Catch {
                operand: Box::new(operand),
                arms,
            }
        } else {
            AstExpressionKind::Match {
                operand: Box::new(operand),
                arms,
            }
        };
        Ok(AstNode::new(kind, self.finish_span(start)))
    }

    fn parse_closure(&mut self, generator: bool) -> Result<AstExpression, Diagnostic> {
        let start = self.current.span;
        if generator {
            self.expect_keyword(Keyword::Gen, "expected `gen`")?;
        }
        let move_ = self.consume_keyword(Keyword::Move)?;
        self.expect_pipe("expected `|` to start closure parameters")?;
        let mut parameters = Vec::new();
        if !self.at_punct(Punctuation::Pipe) {
            loop {
                let parameter_start = self.current.span;
                let pattern = self.parse_at_pattern_mode(true)?;
                let ty = if self.consume_punct(Punctuation::Colon)? {
                    Some(self.parse_type()?)
                } else {
                    None
                };
                parameters.push(AstClosureParameter {
                    pattern,
                    ty,
                    span: self.finish_span(parameter_start),
                });
                if !self.consume_punct(Punctuation::Comma)? {
                    break;
                }
                if self.at_punct(Punctuation::Pipe) {
                    break;
                }
            }
        }
        self.expect_pipe("expected `|` after closure parameters")?;
        let kind = if generator {
            self.expect_keyword(Keyword::Resume, "expected `resume` in generator closure")?;
            let resume = self.parse_type()?;
            self.expect_keyword(Keyword::Yields, "expected `yields` in generator closure")?;
            let yields = self.parse_type()?;
            let effects = self.parse_effect_sets()?;
            let result = if self.consume_punct(Punctuation::Arrow)? {
                Some(self.parse_type()?)
            } else {
                None
            };
            let body = self.parse_expression()?;
            AstExpressionKind::GeneratorClosure(Box::new(AstGeneratorClosure {
                move_,
                parameters,
                resume,
                yields,
                effects,
                result,
                body: Box::new(body),
            }))
        } else {
            let effects = self.parse_effect_sets()?;
            let result = if self.consume_punct(Punctuation::Arrow)? {
                Some(self.parse_type()?)
            } else {
                None
            };
            let body = self.parse_expression()?;
            AstExpressionKind::Closure(Box::new(AstClosure {
                move_,
                parameters,
                effects,
                result,
                body: Box::new(body),
            }))
        };
        Ok(AstNode::new(kind, self.finish_span(start)))
    }

    fn parse_pattern(&mut self) -> Result<AstPattern, Diagnostic> {
        let start = self.current.span;
        let first = self.parse_at_pattern()?;
        if !self.consume_punct(Punctuation::Pipe)? {
            return Ok(first);
        }
        let mut alternatives = vec![first];
        loop {
            alternatives.push(self.parse_at_pattern()?);
            if !self.consume_punct(Punctuation::Pipe)? {
                break;
            }
        }
        Ok(AstNode::new(
            AstPatternKind::Or(alternatives),
            self.finish_span(start),
        ))
    }

    fn parse_at_pattern(&mut self) -> Result<AstPattern, Diagnostic> {
        self.parse_at_pattern_mode(false)
    }

    fn parse_at_pattern_mode(&mut self, no_top_alt: bool) -> Result<AstPattern, Diagnostic> {
        let start = self.current.span;
        let binding = self.parse_structural_pattern(no_top_alt)?;
        if self.consume_punct(Punctuation::At)? {
            if !Self::is_binding_pattern(&binding) {
                return Err(Diagnostic::at(
                    "PARSE001",
                    binding.span,
                    "left side of `@` must be a binding pattern",
                ));
            }
            let pattern = self.parse_at_pattern_mode(no_top_alt)?;
            Ok(AstNode::new(
                AstPatternKind::At {
                    binding: Box::new(binding),
                    pattern: Box::new(pattern),
                },
                self.finish_span(start),
            ))
        } else {
            Ok(binding)
        }
    }

    fn is_binding_pattern(pattern: &AstPattern) -> bool {
        match &pattern.kind {
            AstPatternKind::Binding { .. } => true,
            AstPatternKind::BarePathOrBinding(path) => {
                matches!(path.root, AstPathRoot::Bare) && path.segments.len() == 1
            }
            _ => false,
        }
    }

    fn parse_structural_pattern(&mut self, no_top_alt: bool) -> Result<AstPattern, Diagnostic> {
        let start = self.current.span;
        if self.consume_keyword(Keyword::Mut)? {
            let (name, _) = self.identifier("expected binding name after `mut`")?;
            return Ok(AstNode::new(
                AstPatternKind::Binding {
                    name,
                    mutable: true,
                    by_reference: false,
                    reference_mutable: false,
                },
                self.finish_span(start),
            ));
        }
        if self.consume_keyword(Keyword::Ref)? {
            let reference_mutable = self.consume_keyword(Keyword::Mut)?;
            let (name, _) = self.identifier("expected binding name after `ref`")?;
            return Ok(AstNode::new(
                AstPatternKind::Binding {
                    name,
                    mutable: false,
                    by_reference: true,
                    reference_mutable,
                },
                self.finish_span(start),
            ));
        }
        if matches!(self.current.kind, TokenKind::Wildcard) {
            self.bump()?;
            return Ok(AstNode::new(AstPatternKind::Wildcard, start));
        }
        if self.consume_punct(Punctuation::Ampersand)? {
            let mutable = self.consume_keyword(Keyword::Mut)?;
            let pattern = if no_top_alt {
                self.parse_at_pattern_mode(true)?
            } else {
                self.parse_pattern()?
            };
            return Ok(AstNode::new(
                AstPatternKind::Reference {
                    mutable,
                    pattern: Box::new(pattern),
                },
                self.finish_span(start),
            ));
        }
        if self.consume_punct(Punctuation::LeftParen)? {
            if self.consume_punct(Punctuation::RightParen)? {
                return Ok(AstNode::new(AstPatternKind::Unit, self.finish_span(start)));
            }
            let first = self.parse_pattern()?;
            self.expect_punct(
                Punctuation::Comma,
                "a parenthesized pattern must be a tuple and include `,`",
            )?;
            let mut values = vec![first];
            if self.at_punct(Punctuation::Comma) {
                return Err(self.error("tuple pattern contains an empty element"));
            }
            if !self.at_punct(Punctuation::RightParen) {
                loop {
                    values.push(self.parse_pattern()?);
                    if !self.consume_punct(Punctuation::Comma)? {
                        break;
                    }
                    if self.at_punct(Punctuation::RightParen) {
                        break;
                    }
                    if self.at_punct(Punctuation::Comma) {
                        return Err(self.error("tuple pattern contains an empty element"));
                    }
                }
            }
            self.expect_punct(Punctuation::RightParen, "expected `)` after tuple pattern")?;
            return Ok(AstNode::new(
                AstPatternKind::Tuple(values),
                self.finish_span(start),
            ));
        }
        if self.consume_punct(Punctuation::LeftBracket)? {
            let mut parts = Vec::new();
            if !self.at_punct(Punctuation::RightBracket) {
                loop {
                    if let Some(span) = self.take_punct(Punctuation::Range)? {
                        parts.push(AstSlicePatternPart::Rest(span));
                    } else {
                        parts.push(AstSlicePatternPart::Pattern(Box::new(
                            self.parse_pattern()?,
                        )));
                    }
                    if !self.consume_punct(Punctuation::Comma)? {
                        break;
                    }
                    if self.at_punct(Punctuation::RightBracket) {
                        break;
                    }
                }
            }
            self.expect_punct(
                Punctuation::RightBracket,
                "expected `]` after slice pattern",
            )?;
            return Ok(AstNode::new(
                AstPatternKind::Slice(parts),
                self.finish_span(start),
            ));
        }

        let atom = self.parse_pattern_atom()?;
        if self.at_punct(Punctuation::Range) || self.at_punct(Punctuation::RangeInclusive) {
            let Some(range_start) = Self::pattern_range_endpoint(&atom) else {
                return Err(Diagnostic::at(
                    "PARSE001",
                    atom.span,
                    "invalid range-pattern endpoint",
                ));
            };
            let inclusive = self.consume_punct(Punctuation::RangeInclusive)?;
            if !inclusive {
                self.expect_punct(Punctuation::Range, "expected range operator")?;
            }
            let range_end = self.parse_range_endpoint()?;
            return Ok(AstNode::new(
                AstPatternKind::Range {
                    inclusive,
                    start: range_start,
                    end: range_end,
                },
                self.finish_span(start),
            ));
        }
        Ok(atom)
    }

    fn parse_pattern_atom(&mut self) -> Result<AstPattern, Diagnostic> {
        let start = self.current.span;
        let kind = if self.consume_punct(Punctuation::Minus)? {
            let TokenKind::Integer(literal) = self.current.kind.clone() else {
                return Err(self.error("expected integer literal after `-` in pattern"));
            };
            self.bump()?;
            AstPatternKind::Literal(AstPatternLiteral::Integer {
                negative: true,
                literal,
            })
        } else {
            match self.current.kind.clone() {
                TokenKind::Integer(literal) => {
                    self.bump()?;
                    AstPatternKind::Literal(AstPatternLiteral::Integer {
                        negative: false,
                        literal,
                    })
                }
                TokenKind::Character(value) => {
                    self.bump()?;
                    AstPatternKind::Literal(AstPatternLiteral::Character(value))
                }
                TokenKind::String(value) => {
                    self.bump()?;
                    AstPatternKind::Literal(AstPatternLiteral::String(value))
                }
                TokenKind::Keyword(Keyword::True) => {
                    self.bump()?;
                    AstPatternKind::Literal(AstPatternLiteral::Boolean(true))
                }
                TokenKind::Keyword(Keyword::False) => {
                    self.bump()?;
                    AstPatternKind::Literal(AstPatternLiteral::Boolean(false))
                }
                _ if self.starts_path() => {
                    let path = self.parse_pattern_value_path()?;
                    if self.consume_punct(Punctuation::LeftParen)? {
                        let mut patterns = Vec::new();
                        if !self.at_punct(Punctuation::RightParen) {
                            loop {
                                patterns.push(self.parse_pattern()?);
                                if !self.consume_punct(Punctuation::Comma)? {
                                    break;
                                }
                                if self.at_punct(Punctuation::RightParen) {
                                    break;
                                }
                            }
                        }
                        self.expect_punct(
                            Punctuation::RightParen,
                            "expected `)` after constructor pattern",
                        )?;
                        AstPatternKind::Constructor {
                            path,
                            payload: AstConstructorPatternPayload::Tuple(patterns),
                        }
                    } else if self.consume_punct(Punctuation::LeftBrace)? {
                        let mut fields = Vec::new();
                        if !self.at_punct(Punctuation::RightBrace) {
                            loop {
                                let field_start = self.current.span;
                                let (name, _) =
                                    self.identifier("expected constructor-pattern field")?;
                                self.expect_punct(
                                    Punctuation::Colon,
                                    "expected `:` after constructor-pattern field",
                                )?;
                                let pattern = self.parse_pattern()?;
                                fields.push(AstRecordPatternField {
                                    name,
                                    pattern,
                                    span: self.finish_span(field_start),
                                });
                                if !self.consume_punct(Punctuation::Comma)? {
                                    break;
                                }
                                if self.at_punct(Punctuation::RightBrace) {
                                    break;
                                }
                            }
                        }
                        self.expect_punct(
                            Punctuation::RightBrace,
                            "expected `}` after constructor pattern",
                        )?;
                        AstPatternKind::Constructor {
                            path,
                            payload: AstConstructorPatternPayload::Record(fields),
                        }
                    } else if path.segments.len() > 1 || !matches!(path.root, AstPathRoot::Bare) {
                        AstPatternKind::Constructor {
                            path,
                            payload: AstConstructorPatternPayload::Unit,
                        }
                    } else {
                        AstPatternKind::BarePathOrBinding(path)
                    }
                }
                _ => return Err(self.error("expected a pattern")),
            }
        };
        Ok(AstNode::new(kind, self.finish_span(start)))
    }

    fn pattern_range_endpoint(pattern: &AstPattern) -> Option<AstRangeEndpoint> {
        match &pattern.kind {
            AstPatternKind::Literal(AstPatternLiteral::Integer { negative, literal }) => {
                Some(AstRangeEndpoint::Integer {
                    negative: *negative,
                    literal: literal.clone(),
                    span: pattern.span,
                })
            }
            AstPatternKind::Literal(AstPatternLiteral::Character(value)) => {
                Some(AstRangeEndpoint::Character {
                    value: *value,
                    span: pattern.span,
                })
            }
            AstPatternKind::BarePathOrBinding(path) => Some(AstRangeEndpoint::Const(path.clone())),
            AstPatternKind::Constructor {
                path,
                payload: AstConstructorPatternPayload::Unit,
            } => Some(AstRangeEndpoint::Const(path.clone())),
            _ => None,
        }
    }

    fn parse_pattern_value_path(&mut self) -> Result<AstPath, Diagnostic> {
        let mut path = self.parse_path(false)?;
        while self.at_punct(Punctuation::ColonColon) && self.next_is_punct(Punctuation::Less) {
            self.bump()?;
            let arguments = self.parse_generic_arguments(true)?;
            if let Some(segment) = path.segments.last_mut() {
                segment.generic_arguments = Some(arguments);
            } else {
                path.generic_arguments = Some(arguments);
            }
            self.expect_punct(
                Punctuation::ColonColon,
                "expected `::` after generic value-path arguments",
            )?;
            let (name, span) =
                self.identifier("expected value-path segment after generic arguments")?;
            path.segments.push(AstPathSegment {
                name,
                generic_arguments: None,
                span,
            });
            while self.at_punct(Punctuation::ColonColon) && !self.next_is_punct(Punctuation::Less) {
                self.bump()?;
                let (name, span) = self.identifier("expected value-path segment after `::`")?;
                path.segments.push(AstPathSegment {
                    name,
                    generic_arguments: None,
                    span,
                });
            }
            path.span = path
                .span
                .join(self.previous.expect("value-path segment consumed"));
        }
        Ok(path)
    }

    fn parse_range_endpoint(&mut self) -> Result<AstRangeEndpoint, Diagnostic> {
        let start = self.current.span;
        if self.consume_punct(Punctuation::Minus)? {
            let TokenKind::Integer(literal) = self.current.kind.clone() else {
                return Err(self.error("expected integer after `-` in range endpoint"));
            };
            self.bump()?;
            return Ok(AstRangeEndpoint::Integer {
                negative: true,
                literal,
                span: self.finish_span(start),
            });
        }
        match self.current.kind.clone() {
            TokenKind::Integer(literal) => {
                self.bump()?;
                Ok(AstRangeEndpoint::Integer {
                    negative: false,
                    literal,
                    span: self.finish_span(start),
                })
            }
            TokenKind::Character(value) => {
                self.bump()?;
                Ok(AstRangeEndpoint::Character {
                    value,
                    span: self.finish_span(start),
                })
            }
            _ if self.starts_path() => {
                Ok(AstRangeEndpoint::Const(self.parse_pattern_value_path()?))
            }
            _ => Err(self.error("expected range-pattern endpoint")),
        }
    }

    fn parse_const_expression(&mut self) -> Result<AstConstExpression, Diagnostic> {
        self.parse_const_bit_or()
    }

    fn parse_const_bit_or(&mut self) -> Result<AstConstExpression, Diagnostic> {
        let mut left = self.parse_const_bit_xor()?;
        while self.consume_punct(Punctuation::Pipe)? {
            let right = self.parse_const_bit_xor()?;
            left = Self::const_binary(AstConstBinaryOperator::BitOr, left, right);
        }
        Ok(left)
    }

    fn parse_const_bit_xor(&mut self) -> Result<AstConstExpression, Diagnostic> {
        let mut left = self.parse_const_bit_and()?;
        while self.consume_punct(Punctuation::Caret)? {
            let right = self.parse_const_bit_and()?;
            left = Self::const_binary(AstConstBinaryOperator::BitXor, left, right);
        }
        Ok(left)
    }

    fn parse_const_bit_and(&mut self) -> Result<AstConstExpression, Diagnostic> {
        let mut left = self.parse_const_shift()?;
        while self.consume_punct(Punctuation::Ampersand)? {
            let right = self.parse_const_shift()?;
            left = Self::const_binary(AstConstBinaryOperator::BitAnd, left, right);
        }
        Ok(left)
    }

    fn parse_const_shift(&mut self) -> Result<AstConstExpression, Diagnostic> {
        let mut left = self.parse_const_additive()?;
        loop {
            let operator = if self.consume_punct(Punctuation::ShiftLeft)? {
                Some(AstConstBinaryOperator::ShiftLeft)
            } else if self.at_punct(Punctuation::ShiftRight)
                && Self::token_starts_const_expression(&self.next.kind)
            {
                self.bump()?;
                Some(AstConstBinaryOperator::ShiftRight)
            } else {
                None
            };
            let Some(operator) = operator else { break };
            let right = self.parse_const_additive()?;
            left = Self::const_binary(operator, left, right);
        }
        Ok(left)
    }

    fn parse_const_additive(&mut self) -> Result<AstConstExpression, Diagnostic> {
        let mut left = self.parse_const_multiplicative()?;
        loop {
            let operator = if self.consume_punct(Punctuation::Plus)? {
                Some(AstConstBinaryOperator::Add)
            } else if self.consume_punct(Punctuation::Minus)? {
                Some(AstConstBinaryOperator::Subtract)
            } else {
                None
            };
            let Some(operator) = operator else { break };
            let right = self.parse_const_multiplicative()?;
            left = Self::const_binary(operator, left, right);
        }
        Ok(left)
    }

    fn parse_const_multiplicative(&mut self) -> Result<AstConstExpression, Diagnostic> {
        let mut left = self.parse_const_unary()?;
        loop {
            let operator = if self.consume_punct(Punctuation::Star)? {
                Some(AstConstBinaryOperator::Multiply)
            } else if self.consume_punct(Punctuation::Slash)? {
                Some(AstConstBinaryOperator::Divide)
            } else if self.consume_punct(Punctuation::Percent)? {
                Some(AstConstBinaryOperator::Remainder)
            } else {
                None
            };
            let Some(operator) = operator else { break };
            let right = self.parse_const_unary()?;
            left = Self::const_binary(operator, left, right);
        }
        Ok(left)
    }

    fn parse_const_unary(&mut self) -> Result<AstConstExpression, Diagnostic> {
        let start = self.current.span;
        let operator = if self.consume_punct(Punctuation::Minus)? {
            Some(AstConstUnaryOperator::Negate)
        } else if self.consume_punct(Punctuation::Tilde)? {
            Some(AstConstUnaryOperator::BitNot)
        } else {
            None
        };
        if let Some(operator) = operator {
            let operand = self.parse_const_unary()?;
            let span = start.join(operand.span);
            Ok(AstNode::new(
                AstConstExpressionKind::Unary {
                    operator,
                    operand: Box::new(operand),
                },
                span,
            ))
        } else {
            self.parse_const_primary()
        }
    }

    fn parse_const_primary(&mut self) -> Result<AstConstExpression, Diagnostic> {
        let start = self.current.span;
        let kind = match self.current.kind.clone() {
            TokenKind::Integer(literal) => {
                self.bump()?;
                AstConstExpressionKind::Integer(literal)
            }
            TokenKind::Punctuation(Punctuation::LeftParen) => {
                self.bump()?;
                let inner = self.parse_const_expression()?;
                self.expect_punct(
                    Punctuation::RightParen,
                    "expected `)` after const expression",
                )?;
                AstConstExpressionKind::Group(Box::new(inner))
            }
            _ if self.starts_path() => {
                let path = self.parse_path(false)?;
                self.validate_rooted_or_bound_path(&path, "const expression")?;
                AstConstExpressionKind::Path(path)
            }
            _ => return Err(self.error("expected a const expression")),
        };
        Ok(AstNode::new(kind, self.finish_span(start)))
    }

    fn const_binary(
        operator: AstConstBinaryOperator,
        left: AstConstExpression,
        right: AstConstExpression,
    ) -> AstConstExpression {
        let span = left.span.join(right.span);
        AstNode::new(
            AstConstExpressionKind::Binary {
                operator,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        )
    }

    fn parse_method_name(&mut self, message: &'static str) -> Result<AstMethodName, Diagnostic> {
        let name = match self.current.kind.clone() {
            TokenKind::Identifier(name) => AstMethodName::Identifier(name),
            TokenKind::Keyword(Keyword::Read) => AstMethodName::Read,
            TokenKind::Keyword(Keyword::Resource) => AstMethodName::Resource,
            TokenKind::Keyword(Keyword::Run) => AstMethodName::Run,
            TokenKind::Keyword(Keyword::Spawn) => AstMethodName::Spawn,
            _ => return Err(self.error(message)),
        };
        self.bump()?;
        Ok(name)
    }

    fn identifier(&mut self, message: &'static str) -> Result<(Symbol, Span), Diagnostic> {
        let TokenKind::Identifier(name) = self.current.kind.clone() else {
            return Err(self.error(message));
        };
        let span = self.bump()?.span;
        Ok((name, span))
    }

    fn take_lifetime(&mut self) -> Result<Option<Symbol>, Diagnostic> {
        let TokenKind::Lifetime(name) = self.current.kind.clone() else {
            return Ok(None);
        };
        self.bump()?;
        Ok(Some(name))
    }

    fn validate_rooted_or_bound_path(
        &self,
        path: &AstPath,
        context: &'static str,
    ) -> Result<(), Diagnostic> {
        let valid = match path.root {
            AstPathRoot::Bare => path.segments.len() == 1,
            AstPathRoot::SelfType => path.segments.is_empty(),
            AstPathRoot::Package | AstPathRoot::SelfValue | AstPathRoot::Super(_) => {
                !path.segments.is_empty()
            }
            AstPathRoot::Identifier(_) => !path.segments.is_empty(),
        };
        if valid {
            Ok(())
        } else {
            Err(Diagnostic::at(
                "PARSE001",
                path.span,
                format!("{context} must be a rooted item path or a single bound path"),
            ))
        }
    }

    fn starts_path(&self) -> bool {
        matches!(
            self.current.kind,
            TokenKind::Identifier(_)
                | TokenKind::Keyword(Keyword::Package)
                | TokenKind::Keyword(Keyword::SelfValue)
                | TokenKind::Keyword(Keyword::SelfType)
                | TokenKind::Keyword(Keyword::Super)
        )
    }

    fn starts_expression(&self) -> bool {
        matches!(
            self.current.kind,
            TokenKind::Identifier(_)
                | TokenKind::Integer(_)
                | TokenKind::Float(_)
                | TokenKind::Character(_)
                | TokenKind::String(_)
                | TokenKind::Keyword(Keyword::True)
                | TokenKind::Keyword(Keyword::False)
                | TokenKind::Keyword(Keyword::Package)
                | TokenKind::Keyword(Keyword::SelfValue)
                | TokenKind::Keyword(Keyword::SelfType)
                | TokenKind::Keyword(Keyword::Super)
                | TokenKind::Keyword(Keyword::If)
                | TokenKind::Keyword(Keyword::While)
                | TokenKind::Keyword(Keyword::Loop)
                | TokenKind::Keyword(Keyword::Match)
                | TokenKind::Keyword(Keyword::Catch)
                | TokenKind::Keyword(Keyword::Unsafe)
                | TokenKind::Keyword(Keyword::Move)
                | TokenKind::Keyword(Keyword::Gen)
                | TokenKind::Keyword(Keyword::Return)
                | TokenKind::Keyword(Keyword::Break)
                | TokenKind::Keyword(Keyword::Continue)
                | TokenKind::Keyword(Keyword::Throw)
                | TokenKind::Keyword(Keyword::Yield)
                | TokenKind::Punctuation(Punctuation::LeftParen)
                | TokenKind::Punctuation(Punctuation::LeftBracket)
                | TokenKind::Punctuation(Punctuation::LeftBrace)
                | TokenKind::Punctuation(Punctuation::Pipe)
                | TokenKind::Punctuation(Punctuation::LogicalOr)
                | TokenKind::Punctuation(Punctuation::Minus)
                | TokenKind::Punctuation(Punctuation::Bang)
                | TokenKind::Punctuation(Punctuation::Tilde)
                | TokenKind::Punctuation(Punctuation::Star)
                | TokenKind::Punctuation(Punctuation::Ampersand)
        )
    }

    fn token_starts_const_expression(kind: &TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::Identifier(_)
                | TokenKind::Integer(_)
                | TokenKind::Keyword(Keyword::Package)
                | TokenKind::Keyword(Keyword::SelfValue)
                | TokenKind::Keyword(Keyword::SelfType)
                | TokenKind::Keyword(Keyword::Super)
                | TokenKind::Punctuation(Punctuation::LeftParen)
                | TokenKind::Punctuation(Punctuation::Minus)
                | TokenKind::Punctuation(Punctuation::Tilde)
        )
    }

    fn at_eof(&self) -> bool {
        matches!(self.current.kind, TokenKind::Eof)
    }

    fn at_keyword(&self, keyword: Keyword) -> bool {
        self.current.kind == TokenKind::Keyword(keyword)
    }

    fn next_is_keyword(&self, keyword: Keyword) -> bool {
        self.next.kind == TokenKind::Keyword(keyword)
    }

    fn at_punct(&self, punctuation: Punctuation) -> bool {
        self.current.kind == TokenKind::Punctuation(punctuation)
    }

    fn next_is_punct(&self, punctuation: Punctuation) -> bool {
        self.next.kind == TokenKind::Punctuation(punctuation)
    }

    fn at_generic_close(&self) -> bool {
        self.at_punct(Punctuation::Greater) || self.at_punct(Punctuation::ShiftRight)
    }

    fn bump(&mut self) -> Result<Token, Diagnostic> {
        let consumed = self.current.clone();
        self.previous = Some(consumed.span);
        self.current = self.next.clone();
        self.next = self.lexer.next_token()?;
        Ok(consumed)
    }

    fn consume_keyword(&mut self, keyword: Keyword) -> Result<bool, Diagnostic> {
        if !self.at_keyword(keyword) {
            return Ok(false);
        }
        self.bump()?;
        Ok(true)
    }

    fn expect_keyword(
        &mut self,
        keyword: Keyword,
        message: &'static str,
    ) -> Result<Span, Diagnostic> {
        if !self.at_keyword(keyword) {
            return Err(self.error(message));
        }
        Ok(self.bump()?.span)
    }

    fn consume_punct(&mut self, punctuation: Punctuation) -> Result<bool, Diagnostic> {
        if !self.at_punct(punctuation) {
            return Ok(false);
        }
        self.bump()?;
        Ok(true)
    }

    fn take_punct(&mut self, punctuation: Punctuation) -> Result<Option<Span>, Diagnostic> {
        if !self.at_punct(punctuation) {
            return Ok(None);
        }
        Ok(Some(self.bump()?.span))
    }

    fn expect_punct(
        &mut self,
        punctuation: Punctuation,
        message: &'static str,
    ) -> Result<Span, Diagnostic> {
        if !self.at_punct(punctuation) {
            return Err(self.error(message));
        }
        Ok(self.bump()?.span)
    }

    fn expect_generic_close(&mut self) -> Result<Span, Diagnostic> {
        if self.at_punct(Punctuation::Greater) {
            return Ok(self.bump()?.span);
        }
        if self.at_punct(Punctuation::ShiftRight) {
            return self.split_current_pair(Punctuation::Greater, ">");
        }
        Err(self.error("expected `>` to close generic arguments"))
    }

    fn expect_pipe(&mut self, message: &'static str) -> Result<Span, Diagnostic> {
        if self.at_punct(Punctuation::Pipe) {
            return Ok(self.bump()?.span);
        }
        if self.at_punct(Punctuation::LogicalOr) {
            return self.split_current_pair(Punctuation::Pipe, "|");
        }
        Err(self.error(message))
    }

    fn split_current_pair(
        &mut self,
        punctuation: Punctuation,
        spelling: &'static str,
    ) -> Result<Span, Diagnostic> {
        let span = self.current.span;
        let middle = SourcePosition {
            byte: span.start.byte.checked_add(1).ok_or_else(|| {
                Diagnostic::at(
                    "PARSE004",
                    span,
                    "source position overflow while splitting token",
                )
            })?,
            line: span.start.line,
            column: span.start.column.checked_add(1).ok_or_else(|| {
                Diagnostic::at(
                    "PARSE004",
                    span,
                    "source position overflow while splitting token",
                )
            })?,
        };
        let first = Span {
            file: span.file,
            start: span.start,
            end: middle,
        };
        self.current = Token {
            kind: TokenKind::Punctuation(punctuation),
            lexeme: Arc::from(spelling),
            span: Span {
                file: span.file,
                start: middle,
                end: span.end,
            },
        };
        self.previous = Some(first);
        Ok(first)
    }

    fn finish_span(&self, start: Span) -> Span {
        self.previous.map_or(start, |end| start.join(end))
    }

    fn point_span(&self) -> Span {
        Span {
            file: self.current.span.file,
            start: self.current.span.start,
            end: self.current.span.start,
        }
    }

    fn error(&self, message: impl Into<String>) -> Diagnostic {
        let code = if self.at_eof() {
            "PARSE003"
        } else {
            "PARSE001"
        };
        Diagnostic::at(code, self.current.span, message)
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::fs;
    use std::io::Cursor;
    use std::path::{Path, PathBuf};

    use sha2::{Digest, Sha256};

    use super::*;
    use crate::ast::dump_ast;

    fn parse(source: &str) -> Result<AstFile, Diagnostic> {
        parse_reader(FileId(41), Cursor::new(source.as_bytes()))
    }

    fn collect_arc_files(directory: &Path, output: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("fixture directory must be readable") {
            let path = entry.expect("fixture entry must be readable").path();
            if path.is_dir() {
                collect_arc_files(&path, output);
            } else if path.extension().is_some_and(|extension| extension == "arc") {
                output.push(path);
            }
        }
    }

    fn fixture_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../tests/m27c1")
    }

    #[test]
    fn parses_both_complete_frozen_workspace_corpora() {
        let root = fixture_root();
        let mut files = Vec::new();
        collect_arc_files(&root.join("language-game"), &mut files);
        collect_arc_files(&root.join("language-environment"), &mut files);
        files.sort();
        assert!(!files.is_empty());
        for (index, path) in files.into_iter().enumerate() {
            let bytes = fs::read(&path).expect("fixture source must be readable");
            let result = parse_reader(
                FileId(u64::try_from(index).expect("fixture count fits u64")),
                Cursor::new(bytes),
            );
            if let Err(error) = result {
                panic!("{}: {error} at {:?}", path.display(), error.primary.span);
            }
        }
    }

    #[test]
    fn parses_positive_vectors_and_contextual_token_splits() {
        let root = fixture_root().join("vectors");
        for name in ["empty-forms.arc", "lexical-positive.arc"] {
            let bytes = fs::read(root.join(name)).expect("vector must be readable");
            parse_reader(FileId(7), Cursor::new(bytes))
                .unwrap_or_else(|error| panic!("{name}: {error} at {:?}", error.primary.span));
        }
        parse(
            "pub type Deep = Vec<Map<K, Option<Box<V>>>>;\n\
             pub fn values() {\n\
                 let closure = || requires {} throws {} ();\n\
                 let record = Wrapper::<Vec<Vec<i32>>> { value: [] };\n\
                 identity::<Vec<Vec<i32>>>(record.value);\n\
                 Type::<i32>::associated(record.value);\n\
             }\n\
             pub trait ReferencePatterns {\n\
                 fn shared(&value: &i32);\n\
                 fn mutable(&mut value: &mut i32);\n\
             }\n\
             impl Self for Wrapper<i32> {}\n\
             pub schedule Generic {\n\
                 run package::systems::Run::<Vec<Vec<i32>>, const 8 >> 1>;\n\
             }",
        )
        .unwrap();
    }

    #[test]
    fn parses_authoritative_method_turbofish_and_relational_field_access() {
        let file = parse(
            "pub fn compare(value: Value, rhs: Value) {\n\
                 value.method::<T>();\n\
                 value.field < rhs;\n\
                 (value as T) < rhs;\n\
             }",
        )
        .unwrap();
        let AstItem::Declaration(declaration) = &file.items[0] else {
            panic!("function declaration expected")
        };
        let AstDeclarationKind::Function(function) = &declaration.kind else {
            panic!("function expected")
        };
        let AstStatementKind::Expression { expression, .. } = &function.body.statements[0].kind
        else {
            panic!("method-call statement expected")
        };
        let AstExpressionKind::Postfix { parts, .. } = &expression.kind else {
            panic!("postfix expression expected")
        };
        assert!(matches!(
            parts.as_slice(),
            [AstPostfix {
                kind: AstPostfixKind::Method {
                    generic_arguments: Some(AstGenericArguments {
                        turbofish: true,
                        ..
                    }),
                    ..
                },
                ..
            }]
        ));
        let AstStatementKind::Expression { expression, .. } = &function.body.statements[1].kind
        else {
            panic!("comparison statement expected")
        };
        assert!(matches!(
            expression.kind,
            AstExpressionKind::Binary {
                operator: AstBinaryOperator::Less,
                ..
            }
        ));
        assert!(parse("pub fn f(v: V) { v.method<T>(); }").is_err());
        assert!(parse("pub fn f(value: T, rhs: T) { value as T < rhs; }").is_err());
    }

    #[test]
    fn parses_generic_value_path_tails_in_expressions_and_patterns() {
        parse(
            "pub fn paths(value: X) {\n\
                 package::<T>::make();\n\
                 self::<T>::make();\n\
                 super::<T>::make();\n\
                 let Type::<T>::Tuple(inner) = value;\n\
                 let Type::<T>::Record { field: inner } = value;\n\
                 let Type::<T>::Unit = value;\n\
                 let record = Type::<T>::Record { field: value };\n\
                 match value {\n\
                     Type::<T>::LOW..=Type::<T>::HIGH => record,\n\
                     _ => record,\n\
                 };\n\
             }",
        )
        .unwrap();
    }

    #[test]
    fn preserves_never_query_terms_and_reserved_commands_authority() {
        let file =
            parse("pub system S(cmd: commands, q: query [!, !!]) { cmd.spawn {}; }").unwrap();
        let AstItem::Declaration(declaration) = &file.items[0] else {
            panic!("system declaration expected")
        };
        let AstDeclarationKind::System(system) = &declaration.kind else {
            panic!("system expected")
        };
        let AstSystemParameterKind::Query(terms) = &system.parameters[1].kind else {
            panic!("query parameter expected")
        };
        assert!(matches!(terms[0].kind, AstQueryTermKind::Read));
        assert!(matches!(terms[0].ty.kind, AstTypeKind::Never));
        assert!(matches!(terms[1].kind, AstQueryTermKind::Exclude));
        assert!(matches!(terms[1].ty.kind, AstTypeKind::Never));
        for source in [
            "pub system S(commands: commands) {}",
            "pub fn commands() {}",
            "pub fn f() { let commands = 1i32; }",
            "pub type T = commands;",
            "use commands::item;",
            "pub const C: i32 = commands;",
        ] {
            assert!(
                parse(source).is_err(),
                "reserved `commands` parsed: {source}"
            );
        }
    }

    #[test]
    fn visibility_paths_are_restricted_to_lexical_ancestors() {
        for source in [
            "pub(in dependency) struct S;",
            "pub(in dependency::module) struct S;",
            "pub(in Self) struct S;",
        ] {
            assert!(
                parse(source).is_err(),
                "invalid visibility parsed: {source}"
            );
        }
        parse(
            "pub(in package) struct P;\n\
             pub(in self) struct S;\n\
             pub(in super::super::module) struct A;",
        )
        .unwrap();
    }

    #[test]
    fn non_value_path_contexts_reject_self_associated_paths() {
        for source in [
            "pub type T = Self::Assoc;",
            "pub struct S<T> where T: Self::Assoc {}",
            "pub fn f() requires { Self::Assoc } {}",
            "pub schedule S { run Self::Assoc; }",
            "pub type A = [u8; Self::Assoc];",
        ] {
            assert!(parse(source).is_err(), "invalid path mode parsed: {source}");
        }
        parse("pub fn f() { Self::Assoc(); }").unwrap();
    }

    #[test]
    fn closure_delimiters_use_at_patterns_and_leave_body_bit_or_unambiguous() {
        parse(
            "pub fn f(a: i32, b: i32, c: i32) {\n\
                 let first = |a| b | c;\n\
                 let same_tokens = |a | b| c;\n\
                 let nested_or = |(a | b,)| a;\n\
                 let nested_reference_or = |&(a | b,)| a;\n\
             }",
        )
        .unwrap();
        assert!(parse("pub fn f() { let invalid = |a | b: T| a; }").is_err());
        assert!(parse("pub fn f() { let invalid = |&a | b: T| a; }").is_err());
        assert!(
            parse("pub fn f() { let invalid = gen |a | b: T| resume () yields () a; }").is_err()
        );
    }

    #[test]
    fn function_pointer_omitted_and_explicit_unit_results_remain_distinct() {
        let file = parse("pub type Omitted = fn(); pub type Unit = fn() -> ();").unwrap();
        let AstItem::Declaration(omitted) = &file.items[0] else {
            panic!("type alias expected")
        };
        let AstDeclarationKind::TypeAlias(omitted) = &omitted.kind else {
            panic!("type alias expected")
        };
        let AstTypeKind::FunctionPointer {
            result: omitted_result,
            ..
        } = &omitted.target.kind
        else {
            panic!("function pointer expected")
        };
        assert!(omitted_result.is_none());

        let AstItem::Declaration(unit) = &file.items[1] else {
            panic!("type alias expected")
        };
        let AstDeclarationKind::TypeAlias(unit) = &unit.kind else {
            panic!("type alias expected")
        };
        let AstTypeKind::FunctionPointer {
            result: Some(result),
            ..
        } = &unit.target.kind
        else {
            panic!("explicit function-pointer result expected")
        };
        assert!(matches!(result.kind, AstTypeKind::Unit));
        let dump = dump_ast(&file);
        assert!(dump.contains("(result none)"));
        assert!(dump.contains("(result (type (kind (unit))"));
    }

    #[test]
    fn rejects_all_frozen_empty_comma_list_forms() {
        let cases = [
            ("generic parameters", "pub struct S<,>;"),
            ("generic arguments", "pub type T = Vec<,>;"),
            ("where predicates", "pub struct S<T> where , {}"),
            ("function parameters", "pub fn f(,) {}"),
            ("method parameters", "pub trait T { fn f(,); }"),
            ("system generics", "pub system S<,>() {}"),
            ("system parameters", "pub system S(,) {}"),
            (
                "system generic arguments",
                "pub schedule S { run package::m::f::<,>; }",
            ),
            ("requires set", "pub fn f() requires {,} {}"),
            ("throws set", "pub fn f() throws {,} {}"),
            ("component fields", "pub component C {,}"),
            ("resource fields", "pub resource R {,}"),
            ("struct record fields", "pub struct S {,}"),
            ("tuple fields", "pub struct S(,);"),
            ("enum variants", "pub enum E {,}"),
            ("enum tuple fields", "pub enum E { V(,) }"),
            ("enum record fields", "pub enum E { V {,} }"),
            ("function pointer types", "pub type F = fn(,);"),
            ("tuple types", "pub type T = (,);"),
            ("world initializers", "pub world W { init {,} }"),
            ("world spawn values", "pub world W { init { spawn {,}; } }"),
            ("query terms", "pub system S(q: query [,]) {}"),
            ("call arguments", "pub fn f() { call(,); }"),
            ("command spawn values", "pub fn f(c: C) { c.spawn {,}; }"),
            ("record expression fields", "pub fn f() { R {,}; }"),
            ("array values", "pub fn f() { [,]; }"),
            ("tuple expressions", "pub fn f() { (,); }"),
            ("closure parameters", "pub fn f() { |,| (); }"),
            ("match arms", "pub fn f(x: X) { match x {,} }"),
            ("catch arms", "pub fn f(x: X) { catch x {,} }"),
            ("tuple patterns", "pub fn f(x: X) { let (,) = x; }"),
            (
                "constructor tuple patterns",
                "pub fn f(x: X) { let C(,) = x; }",
            ),
            (
                "constructor record patterns",
                "pub fn f(x: X) { let C {,} = x; }",
            ),
            ("slice patterns", "pub fn f(x: X) { let [,] = x; }"),
        ];
        for (name, source) in cases {
            assert!(
                parse(source).is_err(),
                "{name} accepted an empty comma list"
            );
        }
    }

    #[test]
    fn rejects_singleton_and_multi_element_tuple_double_commas() {
        let cases = [
            "pub type T = (i32,,);",
            "pub type T = (i32, u32,,);",
            "pub fn f() { let x = (1i32,,); }",
            "pub fn f() { let x = (1i32, 2i32,,); }",
            "pub fn f(x: X) { let (a,,) = x; }",
            "pub fn f(x: X) { let (a, b,,) = x; }",
        ];
        for source in cases {
            assert!(
                parse(source).is_err(),
                "accepted tuple double comma: {source}"
            );
        }
    }

    #[test]
    fn rejects_required_semicolon_omissions() {
        let cases = [
            "mod child",
            "pub fn f() { let x = 1i32 let y = 2i32; }",
            "pub fn f() { first() second(); }",
            "pub world W { init { spawn {} } }",
            "pub schedule S { run package::m::system }",
        ];
        for source in cases {
            assert!(
                parse(source).is_err(),
                "accepted missing semicolon: {source}"
            );
        }
    }

    #[test]
    fn frozen_negative_vectors_fail_and_eof_delimiter_is_zero_width() {
        let root = fixture_root().join("vectors");
        for name in [
            "invalid-empty-comma.arc",
            "invalid-number-member.arc",
            "invalid-reserved-identifier.arc",
            "invalid-tuple-double-comma.arc",
        ] {
            let bytes = fs::read(root.join(name)).expect("negative vector must be readable");
            assert!(
                parse_reader(FileId(11), Cursor::new(bytes)).is_err(),
                "negative vector parsed: {name}"
            );
        }

        let bytes =
            fs::read(root.join("missing-delimiter-eof.arc")).expect("EOF vector must be readable");
        let length = u64::try_from(bytes.len()).expect("fixture length fits u64");
        let error = parse_reader(FileId(12), Cursor::new(bytes)).unwrap_err();
        assert_eq!(error.code, "PARSE003");
        let span = error.primary.span.expect("parse error has a source span");
        assert_eq!(span.start, span.end);
        assert_eq!(span.start.byte, length);
    }

    #[test]
    fn ast_dump_is_deterministic_and_field_complete_for_parsed_source() {
        let file = parse(
            "/// doc\n\
             pub fn f<T>(mut x: T) requires { package::caps::Io } throws { Error } -> T\n\
             where T: Clone { if true { x } else { return x; } }",
        )
        .unwrap();
        let first = dump_ast(&file);
        let second = dump_ast(&file);
        assert_eq!(first, second);
        assert!(first.starts_with("ARCHE-AST-TEXT 1\n"));
        for field in [
            "docs",
            "visibility",
            "generics",
            "effects",
            "body",
            "eof-span",
        ] {
            assert!(first.contains(field), "dump omitted `{field}`: {first}");
        }
    }

    #[test]
    fn complete_workspace_ast_text_is_byte_exact() {
        let root = fixture_root();
        let mut files = Vec::new();
        collect_arc_files(&root.join("language-game"), &mut files);
        collect_arc_files(&root.join("language-environment"), &mut files);
        for name in ["empty-forms.arc", "lexical-positive.arc"] {
            files.push(root.join("vectors").join(name));
        }
        files.sort();

        let mut digest = Sha256::new();
        for (index, path) in files.into_iter().enumerate() {
            let bytes = fs::read(&path).expect("golden source must be readable");
            // Freeze one canonical checkout representation so an autocrlf host
            // cannot change the positional golden while reading the same Git
            // source blobs. Bare CR remains covered by the source/lexer tests.
            let bytes = String::from_utf8(bytes)
                .expect("C1 fixtures are exact UTF-8")
                .replace("\r\n", "\n")
                .into_bytes();
            let file = parse_reader(
                FileId(u64::try_from(index).expect("golden file count fits u64")),
                Cursor::new(bytes),
            )
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let dump = dump_ast(&file);
            digest.update(
                u64::try_from(dump.len())
                    .expect("AST dump length fits u64")
                    .to_le_bytes(),
            );
            digest.update(dump.as_bytes());
        }

        let mut actual = String::with_capacity(64);
        for byte in digest.finalize() {
            write!(actual, "{byte:02x}").expect("writing to String cannot fail");
        }
        assert_eq!(
            actual,
            "7517d9049b62b7e6b1ca79ef3c169a0a9f61374fdc68da03472f3584a81b40d0"
        );
    }
}
