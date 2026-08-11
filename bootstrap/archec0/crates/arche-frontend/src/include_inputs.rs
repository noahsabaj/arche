//! Pure AST discovery for package-relative `include_bytes`/`include_str` inputs.
//!
//! Discovery is intentionally staged. [`find_include_input_candidates`] finds
//! only the direct, unqualified source spellings that could resolve to the
//! embedded prelude functions. It does not claim that a same-named local or
//! user definition is the builtin. The HIR integration must first resolve the
//! candidate's `callee_span` to the branded embedded-Core row and only then
//! call [`IncludeInputCandidate::validate`].
//!
//! After validation, integration must collect candidates from every parsed
//! module, sort them globally by canonical package-name bytes and then
//! [`PortablePath`], acquire each distinct path after all module sources with
//! `SourceRole::Include`, require exact UTF-8 for every `IncludeInputKind::Str`
//! view, and construct package source-tree commitments from those retained
//! bytes. It must never reopen the original include path.

use std::collections::BTreeSet;
use std::sync::Arc;

use arche_package::PortablePath;

use crate::ast::{
    AstBlock, AstCondition, AstConstExpression, AstConstExpressionKind, AstDeclaration,
    AstDeclarationKind, AstEffectSets, AstElseBranch, AstExpression, AstExpressionKind, AstFile,
    AstGenericArgumentKind, AstGenericArguments, AstGenericParameterKind, AstGenericParameters,
    AstImpl, AstItem, AstMethodParameter, AstMethodSignature, AstPath, AstPathRoot, AstPattern,
    AstPatternKind, AstPostfixKind, AstSchedule, AstSlicePatternPart, AstStatementKind,
    AstStructForm, AstSystemGenericArgument, AstSystemParameterKind, AstType, AstTypeBoundKind,
    AstTypeKind, AstVariantForm, AstVisibility, AstVisibilityKind, AstWhereClause,
    AstWherePredicateKind, AstWorldInitBlock, AstWorldInitKind,
};
use crate::{Diagnostic, Span};

/// Frozen diagnostic for an include builtin whose path cannot become a
/// retained package input.
pub const INVALID_INCLUDE_DIAGNOSTIC: &str = "CTFE005";

/// The two source-spellable include views. Both kinds share one retained input
/// and FileId when their portable path is equal.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum IncludeInputKind {
    Bytes,
    Str,
}

impl IncludeInputKind {
    pub const fn source_name(self) -> &'static str {
        match self {
            Self::Bytes => "include_bytes",
            Self::Str => "include_str",
        }
    }
}

/// A canonical semantic-view key. Integration deduplicates source acquisition
/// by `path` alone, while this key keeps bytes and string views distinct.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct IncludeInputKey {
    pub path: PortablePath,
    pub kind: IncludeInputKind,
}

/// A syntactically direct prelude candidate. This is deliberately not proof
/// that the callee is the embedded builtin; name resolution supplies that
/// proof before [`Self::validate`] is called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncludeInputCandidate {
    pub kind: IncludeInputKind,
    pub callee_span: Span,
    pub call_span: Span,
    argument: IncludeArgumentCandidate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum IncludeArgumentCandidate {
    Literal { decoded: Arc<str>, span: Span },
    Invalid { span: Span },
}

/// One validated include use. Repeated uses retain their independent source
/// spans; [`IncludeInputKey`] and [`canonical_acquisition_paths`] provide the
/// two canonical deduplication projections.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncludeInput {
    pub kind: IncludeInputKind,
    pub path: PortablePath,
    pub callee_span: Span,
    pub literal_span: Span,
    pub call_span: Span,
}

impl IncludeInput {
    pub fn key(&self) -> IncludeInputKey {
        IncludeInputKey {
            path: self.path.clone(),
            kind: self.kind,
        }
    }
}

impl IncludeInputCandidate {
    /// Validates the exact builtin call contract after the caller has proved
    /// that `callee_span` resolves to the matching branded embedded-Core row.
    pub fn validate(&self) -> Result<IncludeInput, Diagnostic> {
        let IncludeArgumentCandidate::Literal { decoded, span } = &self.argument else {
            return Err(Diagnostic::at(
                INVALID_INCLUDE_DIAGNOSTIC,
                self.argument.span(),
                format!(
                    "`{}` requires exactly one string-literal portable path",
                    self.kind.source_name()
                ),
            ));
        };
        let path = PortablePath::new(decoded).map_err(|_| {
            Diagnostic::at(
                INVALID_INCLUDE_DIAGNOSTIC,
                *span,
                format!(
                    "`{}` path is not a safe NFC portable path relative to the declaring package root",
                    self.kind.source_name()
                ),
            )
        })?;
        Ok(IncludeInput {
            kind: self.kind,
            path,
            callee_span: self.callee_span,
            literal_span: *span,
            call_span: self.call_span,
        })
    }
}

impl IncludeArgumentCandidate {
    const fn span(&self) -> Span {
        match self {
            Self::Literal { span, .. } | Self::Invalid { span } => *span,
        }
    }
}

/// Finds every direct `include_bytes(...)`/`include_str(...)` candidate in one
/// complete parsed file, retaining source preorder. Qualified, grouped,
/// generic, method, and field calls are not prelude candidates.
pub fn find_include_input_candidates(file: &AstFile) -> Vec<IncludeInputCandidate> {
    let mut visitor = IncludeVisitor::default();
    visitor.file(file);
    visitor.candidates
}

/// Returns distinct include semantic-view keys in canonical path-then-kind
/// order. Call-site spans intentionally do not participate.
pub fn canonical_include_keys<'a>(
    inputs: impl IntoIterator<Item = &'a IncludeInput>,
) -> Vec<IncludeInputKey> {
    inputs
        .into_iter()
        .map(IncludeInput::key)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Returns distinct acquisition paths in canonical order. Bytes and string
/// views of one path therefore acquire one immutable snapshot and one FileId.
pub fn canonical_acquisition_paths<'a>(
    inputs: impl IntoIterator<Item = &'a IncludeInput>,
) -> Vec<PortablePath> {
    inputs
        .into_iter()
        .map(|input| input.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[derive(Default)]
struct IncludeVisitor {
    candidates: Vec<IncludeInputCandidate>,
}

impl IncludeVisitor {
    fn file(&mut self, file: &AstFile) {
        for item in &file.items {
            self.item(item);
        }
    }

    fn item(&mut self, item: &AstItem) {
        match item {
            AstItem::Module(module) => self.visibility(&module.visibility),
            AstItem::Import(import) => {
                self.visibility(&import.visibility);
                self.path(&import.path);
            }
            AstItem::Declaration(declaration) => self.declaration(declaration),
            AstItem::Impl(implementation) => self.implementation(implementation),
        }
    }

    fn declaration(&mut self, declaration: &AstDeclaration) {
        self.visibility(&declaration.visibility);
        match &declaration.kind {
            AstDeclarationKind::World { initializer } => self.world(initializer),
            AstDeclarationKind::Component(record) | AstDeclarationKind::Resource(record) => {
                self.generic_parameters(record.generics.as_ref());
                self.where_clause(record.where_clause.as_ref());
                for field in &record.fields {
                    self.visibility(&field.visibility);
                    self.ty(&field.ty);
                }
            }
            AstDeclarationKind::Tag => {}
            AstDeclarationKind::Struct(structure) => {
                self.generic_parameters(structure.generics.as_ref());
                self.where_clause(structure.where_clause.as_ref());
                match &structure.form {
                    AstStructForm::Unit => {}
                    AstStructForm::Tuple(fields) => {
                        for field in fields {
                            self.visibility(&field.visibility);
                            self.ty(&field.ty);
                        }
                    }
                    AstStructForm::Record(fields) => {
                        for field in fields {
                            self.visibility(&field.visibility);
                            self.ty(&field.ty);
                        }
                    }
                }
            }
            AstDeclarationKind::Enum(enumeration) => {
                self.generic_parameters(enumeration.generics.as_ref());
                self.where_clause(enumeration.where_clause.as_ref());
                for variant in &enumeration.variants {
                    match &variant.form {
                        AstVariantForm::Unit => {}
                        AstVariantForm::Tuple(fields) => {
                            for field in fields {
                                self.ty(field);
                            }
                        }
                        AstVariantForm::Record(fields) => {
                            for field in fields {
                                self.ty(&field.ty);
                            }
                        }
                    }
                }
            }
            AstDeclarationKind::TypeAlias(alias) => {
                self.generic_parameters(alias.generics.as_ref());
                self.ty(&alias.target);
                self.where_clause(alias.where_clause.as_ref());
            }
            AstDeclarationKind::Const(item) => {
                self.ty(&item.ty);
                self.expression(&item.value);
            }
            AstDeclarationKind::Static(item) => {
                self.ty(&item.ty);
                self.expression(&item.value);
            }
            AstDeclarationKind::Function(function) => {
                self.function_signature(&function.signature);
                self.block(&function.body);
            }
            AstDeclarationKind::Generator(generator) => {
                self.generic_parameters(generator.generics.as_ref());
                for parameter in &generator.parameters {
                    self.pattern(&parameter.pattern);
                    self.ty(&parameter.ty);
                }
                self.ty(&generator.resume);
                self.ty(&generator.yields);
                self.effect_sets(&generator.effects);
                if let Some(result) = &generator.result {
                    self.ty(result);
                }
                self.where_clause(generator.where_clause.as_ref());
                self.block(&generator.body);
            }
            AstDeclarationKind::System(system) => {
                self.generic_parameters(system.generics.as_ref());
                for parameter in &system.parameters {
                    match &parameter.kind {
                        AstSystemParameterKind::ResourceRead(ty)
                        | AstSystemParameterKind::ResourceWrite(ty)
                        | AstSystemParameterKind::Capability(ty) => self.ty(ty),
                        AstSystemParameterKind::Query(terms) => {
                            for term in terms {
                                self.ty(&term.ty);
                            }
                        }
                        AstSystemParameterKind::Commands => {}
                    }
                }
                self.effect_sets(&system.effects);
                self.where_clause(system.where_clause.as_ref());
                self.block(&system.body);
            }
            AstDeclarationKind::Schedule(schedule) => self.schedule(schedule),
            AstDeclarationKind::Trait(trait_) => {
                self.generic_parameters(trait_.generics.as_ref());
                self.where_clause(trait_.where_clause.as_ref());
                for method in &trait_.methods {
                    self.method_signature(&method.signature);
                }
            }
        }
    }

    fn implementation(&mut self, implementation: &AstImpl) {
        self.generic_parameters(implementation.generics.as_ref());
        if let Some(path) = &implementation.trait_path {
            self.path(path);
        }
        self.ty(&implementation.target);
        self.where_clause(implementation.where_clause.as_ref());
        for method in &implementation.methods {
            self.visibility(&method.visibility);
            self.method_signature(&method.signature);
            self.block(&method.body);
        }
    }

    fn function_signature(&mut self, signature: &crate::ast::AstFunctionSignature) {
        self.generic_parameters(signature.generics.as_ref());
        for parameter in &signature.parameters {
            self.pattern(&parameter.pattern);
            self.ty(&parameter.ty);
        }
        self.effect_sets(&signature.effects);
        if let Some(result) = &signature.result {
            self.ty(result);
        }
        self.where_clause(signature.where_clause.as_ref());
    }

    fn method_signature(&mut self, signature: &AstMethodSignature) {
        self.generic_parameters(signature.generics.as_ref());
        for parameter in &signature.parameters {
            match parameter {
                AstMethodParameter::Receiver(_) => {}
                AstMethodParameter::Parameter(parameter) => {
                    self.pattern(&parameter.pattern);
                    self.ty(&parameter.ty);
                }
            }
        }
        self.effect_sets(&signature.effects);
        if let Some(result) = &signature.result {
            self.ty(result);
        }
        self.where_clause(signature.where_clause.as_ref());
    }

    fn generic_parameters(&mut self, parameters: Option<&AstGenericParameters>) {
        let Some(parameters) = parameters else {
            return;
        };
        for parameter in &parameters.parameters {
            if let AstGenericParameterKind::Type { bounds, .. } = &parameter.kind {
                for bound in bounds {
                    self.type_bound(bound);
                }
            }
        }
    }

    fn where_clause(&mut self, clause: Option<&AstWhereClause>) {
        let Some(clause) = clause else {
            return;
        };
        for predicate in &clause.predicates {
            if let AstWherePredicateKind::Type { ty, bounds } = &predicate.kind {
                self.ty(ty);
                for bound in bounds {
                    self.type_bound(bound);
                }
            }
        }
    }

    fn type_bound(&mut self, bound: &crate::ast::AstTypeBound) {
        if let AstTypeBoundKind::Trait(path) = &bound.kind {
            self.path(path);
        }
    }

    fn effect_sets(&mut self, effects: &AstEffectSets) {
        if let Some(requires) = &effects.requires {
            for path in &requires.members {
                self.path(path);
            }
        }
        if let Some(throws) = &effects.throws {
            for ty in &throws.members {
                self.ty(ty);
            }
        }
    }

    fn ty(&mut self, ty: &AstType) {
        match &ty.kind {
            AstTypeKind::Path(path) => self.path(path),
            AstTypeKind::Tuple(types) => {
                for ty in types {
                    self.ty(ty);
                }
            }
            AstTypeKind::Array { element, length } => {
                self.ty(element);
                self.const_expression(length);
            }
            AstTypeKind::Slice(element) => self.ty(element),
            AstTypeKind::Reference { pointee, .. } | AstTypeKind::RawPointer { pointee, .. } => {
                self.ty(pointee);
            }
            AstTypeKind::FunctionPointer {
                parameters,
                effects,
                result,
                ..
            } => {
                for parameter in parameters {
                    self.ty(parameter);
                }
                self.effect_sets(effects);
                if let Some(result) = result {
                    self.ty(result);
                }
            }
            AstTypeKind::Scalar(_)
            | AstTypeKind::Never
            | AstTypeKind::Unit
            | AstTypeKind::Str
            | AstTypeKind::SelfType => {}
        }
    }

    fn path(&mut self, path: &AstPath) {
        if let Some(arguments) = &path.generic_arguments {
            self.generic_arguments(arguments);
        }
        for segment in &path.segments {
            if let Some(arguments) = &segment.generic_arguments {
                self.generic_arguments(arguments);
            }
        }
    }

    fn generic_arguments(&mut self, arguments: &AstGenericArguments) {
        for argument in &arguments.arguments {
            match &argument.kind {
                AstGenericArgumentKind::Type(ty) => self.ty(ty),
                AstGenericArgumentKind::IntegerConst(expression) => {
                    self.const_expression(expression);
                }
                AstGenericArgumentKind::Lifetime(_) => {}
            }
        }
    }

    fn const_expression(&mut self, expression: &AstConstExpression) {
        match &expression.kind {
            AstConstExpressionKind::Path(path) => self.path(path),
            AstConstExpressionKind::Group(child)
            | AstConstExpressionKind::Unary { operand: child, .. } => {
                self.const_expression(child);
            }
            AstConstExpressionKind::Binary { left, right, .. } => {
                self.const_expression(left);
                self.const_expression(right);
            }
            AstConstExpressionKind::Integer(_) => {}
        }
    }

    fn world(&mut self, initializer: &AstWorldInitBlock) {
        for entry in &initializer.entries {
            match &entry.kind {
                AstWorldInitKind::Resource { ty, value } => {
                    self.ty(ty);
                    self.expression(value);
                }
                AstWorldInitKind::Spawn { values } => {
                    for value in values {
                        self.expression(value);
                    }
                }
            }
        }
    }

    fn schedule(&mut self, schedule: &AstSchedule) {
        for run in &schedule.runs {
            self.path(&run.target);
            if let Some(arguments) = &run.arguments {
                for argument in &arguments.arguments {
                    match argument {
                        AstSystemGenericArgument::Type(ty) => self.ty(ty),
                        AstSystemGenericArgument::IntegerConst(value) => {
                            self.const_expression(value);
                        }
                    }
                }
            }
        }
    }

    fn block(&mut self, block: &AstBlock) {
        for statement in &block.statements {
            match &statement.kind {
                AstStatementKind::Let {
                    pattern,
                    ty,
                    value,
                    else_block,
                } => {
                    self.pattern(pattern);
                    if let Some(ty) = ty {
                        self.ty(ty);
                    }
                    self.expression(value);
                    if let Some(block) = else_block {
                        self.block(block);
                    }
                }
                AstStatementKind::For {
                    pattern,
                    iterator,
                    body,
                    ..
                } => {
                    self.pattern(pattern);
                    self.expression(iterator);
                    self.block(body);
                }
                AstStatementKind::Assignment { place, value, .. } => {
                    self.expression(place);
                    self.expression(value);
                }
                AstStatementKind::Expression { expression, .. } => self.expression(expression),
            }
        }
        if let Some(tail) = &block.tail {
            self.expression(tail);
        }
    }

    fn expression(&mut self, expression: &AstExpression) {
        match &expression.kind {
            AstExpressionKind::Path(path) => self.path(path),
            AstExpressionKind::Group(child)
            | AstExpressionKind::Unary { operand: child, .. }
            | AstExpressionKind::Yield(child) => self.expression(child),
            AstExpressionKind::Tuple(values) | AstExpressionKind::Array(values) => {
                for value in values {
                    self.expression(value);
                }
            }
            AstExpressionKind::ArrayRepeat { value, count } => {
                self.expression(value);
                self.const_expression(count);
            }
            AstExpressionKind::Record {
                constructor,
                fields,
            } => {
                self.path(constructor);
                for field in fields {
                    self.expression(&field.value);
                }
            }
            AstExpressionKind::Block(block)
            | AstExpressionKind::Loop(block)
            | AstExpressionKind::Unsafe(block) => self.block(block),
            AstExpressionKind::If(if_) => {
                self.condition(&if_.condition);
                self.block(&if_.then_block);
                if let Some(branch) = &if_.else_branch {
                    match branch {
                        AstElseBranch::Block(block) => self.block(block),
                        AstElseBranch::If(expression) => self.expression(expression),
                    }
                }
            }
            AstExpressionKind::While(while_) => {
                self.condition(&while_.condition);
                self.block(&while_.body);
            }
            AstExpressionKind::Match { operand, arms }
            | AstExpressionKind::Catch { operand, arms } => {
                self.expression(operand);
                for arm in arms {
                    self.pattern(&arm.pattern);
                    if let Some(guard) = &arm.guard {
                        self.expression(guard);
                    }
                    self.expression(&arm.value);
                }
            }
            AstExpressionKind::Closure(closure) => {
                for parameter in &closure.parameters {
                    self.pattern(&parameter.pattern);
                    if let Some(ty) = &parameter.ty {
                        self.ty(ty);
                    }
                }
                self.effect_sets(&closure.effects);
                if let Some(result) = &closure.result {
                    self.ty(result);
                }
                self.expression(&closure.body);
            }
            AstExpressionKind::GeneratorClosure(generator) => {
                for parameter in &generator.parameters {
                    self.pattern(&parameter.pattern);
                    if let Some(ty) = &parameter.ty {
                        self.ty(ty);
                    }
                }
                self.ty(&generator.resume);
                self.ty(&generator.yields);
                self.effect_sets(&generator.effects);
                if let Some(result) = &generator.result {
                    self.ty(result);
                }
                self.expression(&generator.body);
            }
            AstExpressionKind::Return(value)
            | AstExpressionKind::Break(value)
            | AstExpressionKind::Throw(value) => {
                if let Some(value) = value {
                    self.expression(value);
                }
            }
            AstExpressionKind::Binary { left, right, .. } => {
                self.expression(left);
                self.expression(right);
            }
            AstExpressionKind::Cast { value, ty } => {
                self.expression(value);
                self.ty(ty);
            }
            AstExpressionKind::Postfix { base, parts } => {
                if let Some(first) = parts.first() {
                    if let AstPostfixKind::Call(arguments) = &first.kind {
                        if let Some(kind) = direct_include_kind(base) {
                            let argument = match arguments.as_slice() {
                                [argument] => match &argument.kind {
                                    AstExpressionKind::Literal(crate::ast::AstLiteral::String(
                                        decoded,
                                    )) => IncludeArgumentCandidate::Literal {
                                        decoded: decoded.clone(),
                                        span: argument.span,
                                    },
                                    _ => IncludeArgumentCandidate::Invalid {
                                        span: argument.span,
                                    },
                                },
                                _ => IncludeArgumentCandidate::Invalid {
                                    span: base.span.join(first.span),
                                },
                            };
                            self.candidates.push(IncludeInputCandidate {
                                kind,
                                callee_span: base.span,
                                call_span: base.span.join(first.span),
                                argument,
                            });
                        }
                    }
                }
                self.expression(base);
                for part in parts {
                    match &part.kind {
                        AstPostfixKind::Call(arguments)
                        | AstPostfixKind::CommandSpawn(arguments) => {
                            for argument in arguments {
                                self.expression(argument);
                            }
                        }
                        AstPostfixKind::Index(index) | AstPostfixKind::Resume(index) => {
                            self.expression(index);
                        }
                        AstPostfixKind::Method {
                            generic_arguments,
                            arguments,
                            ..
                        } => {
                            if let Some(arguments_) = generic_arguments {
                                self.generic_arguments(arguments_);
                            }
                            for argument in arguments {
                                self.expression(argument);
                            }
                        }
                        AstPostfixKind::TurbofishCall {
                            generic_arguments,
                            arguments,
                        } => {
                            self.generic_arguments(generic_arguments);
                            for argument in arguments {
                                self.expression(argument);
                            }
                        }
                        AstPostfixKind::Field(_) | AstPostfixKind::TupleField(_) => {}
                    }
                }
            }
            AstExpressionKind::Literal(_)
            | AstExpressionKind::SelfValue
            | AstExpressionKind::Unit
            | AstExpressionKind::Continue => {}
        }
    }

    fn condition(&mut self, condition: &AstCondition) {
        match condition {
            AstCondition::Expression(expression) => self.expression(expression),
            AstCondition::Let { pattern, value } => {
                self.pattern(pattern);
                self.expression(value);
            }
        }
    }

    fn pattern(&mut self, pattern: &AstPattern) {
        match &pattern.kind {
            AstPatternKind::BarePathOrBinding(path) => self.path(path),
            AstPatternKind::Reference { pattern, .. } => self.pattern(pattern),
            AstPatternKind::Tuple(patterns) | AstPatternKind::Or(patterns) => {
                for pattern in patterns {
                    self.pattern(pattern);
                }
            }
            AstPatternKind::Slice(parts) => {
                for part in parts {
                    if let AstSlicePatternPart::Pattern(pattern) = part {
                        self.pattern(pattern);
                    }
                }
            }
            AstPatternKind::Constructor { path, payload } => {
                self.path(path);
                match payload {
                    crate::ast::AstConstructorPatternPayload::Unit => {}
                    crate::ast::AstConstructorPatternPayload::Tuple(patterns) => {
                        for pattern in patterns {
                            self.pattern(pattern);
                        }
                    }
                    crate::ast::AstConstructorPatternPayload::Record(fields) => {
                        for field in fields {
                            self.pattern(&field.pattern);
                        }
                    }
                }
            }
            AstPatternKind::Range { start, end, .. } => {
                for endpoint in [start, end] {
                    if let crate::ast::AstRangeEndpoint::Const(path) = endpoint {
                        self.path(path);
                    }
                }
            }
            AstPatternKind::At { binding, pattern } => {
                self.pattern(binding);
                self.pattern(pattern);
            }
            AstPatternKind::Wildcard
            | AstPatternKind::Unit
            | AstPatternKind::Literal(_)
            | AstPatternKind::Binding { .. } => {}
        }
    }

    fn visibility(&mut self, visibility: &AstVisibility) {
        if let AstVisibilityKind::In(path) = &visibility.kind {
            self.path(path);
        }
    }
}

fn direct_include_kind(base: &AstExpression) -> Option<IncludeInputKind> {
    let AstExpressionKind::Path(path) = &base.kind else {
        return None;
    };
    if !matches!(path.root, AstPathRoot::Bare)
        || path.generic_arguments.is_some()
        || path.segments.len() != 1
        || path.segments[0].generic_arguments.is_some()
    {
        return None;
    }
    match path.segments[0].name.as_str() {
        "include_bytes" => Some(IncludeInputKind::Bytes),
        "include_str" => Some(IncludeInputKind::Str),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Cursor;
    use std::path::{Path, PathBuf};

    use crate::{parse_reader, FileId};

    use super::*;

    fn parse(source: &str) -> AstFile {
        parse_reader(FileId(29), Cursor::new(source.as_bytes())).unwrap()
    }

    fn validated(source: &str) -> Result<Vec<IncludeInput>, Diagnostic> {
        find_include_input_candidates(&parse(source))
            .iter()
            .map(IncludeInputCandidate::validate)
            .collect()
    }

    #[test]
    fn direct_candidates_retain_kind_path_and_all_three_spans() {
        let inputs = validated(
            "pub const A: &'static [u8] = include_bytes(\"data/table.bin\");\n\
             pub const B: &'static str = include_str(\"data/message.txt\");",
        )
        .unwrap();
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].kind, IncludeInputKind::Bytes);
        assert_eq!(inputs[0].path.as_str(), "data/table.bin");
        assert_eq!(inputs[1].kind, IncludeInputKind::Str);
        assert_eq!(inputs[1].path.as_str(), "data/message.txt");
        for input in inputs {
            assert!(input.call_span.start.byte <= input.callee_span.start.byte);
            assert!(input.callee_span.end.byte <= input.literal_span.start.byte);
            assert!(input.literal_span.end.byte <= input.call_span.end.byte);
        }
    }

    #[test]
    fn validation_rejects_nonliteral_arity_and_malformed_paths_as_ctfe005() {
        for source in [
            "pub fn f(path: str) { include_bytes(path); }",
            "pub fn f() { include_str(); }",
            "pub fn f() { include_bytes(\"a\", \"b\"); }",
            "pub fn f() { include_str(\"../escape\"); }",
            "pub fn f() { include_bytes(\"/absolute\"); }",
            "pub fn f() { include_str(\"Cafe\\u{301}.txt\"); }",
        ] {
            let candidates = find_include_input_candidates(&parse(source));
            assert_eq!(candidates.len(), 1, "candidate missing: {source}");
            let error = candidates[0].validate().unwrap_err();
            assert_eq!(error.code, INVALID_INCLUDE_DIAGNOSTIC, "{source}");
            assert!(error.primary.span.is_some(), "{source}");
        }
    }

    #[test]
    fn qualified_grouped_generic_and_method_calls_are_not_direct_candidates() {
        let file = parse(
            "pub fn f() {\n\
                 package::include_bytes(\"a\");\n\
                 (include_bytes)(\"b\");\n\
                 object.include_str(\"c\");\n\
                 include_bytes::<u8>(\"d\");\n\
             }",
        );
        assert!(find_include_input_candidates(&file).is_empty());
    }

    #[test]
    fn discovery_is_semantic_neutral_until_the_callee_is_branded() {
        let candidates = find_include_input_candidates(&parse(
            "pub fn f(include_bytes: Callback, path: str) { include_bytes(path); }",
        ));
        assert_eq!(candidates.len(), 1);
        // Integration must not validate this row after HIR resolves the callee
        // to the local parameter instead of embedded Core.
        assert_eq!(candidates[0].kind, IncludeInputKind::Bytes);
    }

    #[test]
    fn complete_visitor_crosses_nested_declarations_callables_and_control_flow() {
        let file = parse(
            "pub world W { init {\n\
                 resource Message = include_str(\"world.txt\");\n\
                 spawn { include_bytes(\"spawn.bin\") };\n\
             } }\n\
             pub const C: &'static str = include_str(\"const.txt\");\n\
             pub static S: &'static [u8] = include_bytes(\"static.bin\");\n\
             pub fn nested(flag: bool, value: Value) {\n\
                 let closure = || include_str(\"closure.txt\");\n\
                 let generator = gen || resume () yields () include_bytes(\"generator.bin\");\n\
                 if flag { include_str(\"if.txt\"); } else { include_str(\"else.txt\"); }\n\
                 while flag { include_bytes(\"while.bin\"); }\n\
                 match value { _ => include_str(\"match.txt\") };\n\
                 catch value { _ => include_bytes(\"catch.bin\") };\n\
                 unsafe { include_str(\"unsafe.txt\"); };\n\
                 for item in [value] { include_bytes(\"for.bin\"); }\n\
             }\n\
             pub gen fn Named() resume () yields () { include_str(\"named-gen.txt\"); }\n\
             pub system System() { include_bytes(\"system.bin\"); }\n\
             impl Thing { pub fn method(&self) { include_str(\"method.txt\"); } }\n\
             pub type ConstTree<const N: usize> = [u8; N + 1usize];\n\
             pub schedule Scheduled { run package::System::<const 1usize>; }",
        );
        let inputs = find_include_input_candidates(&file)
            .iter()
            .map(IncludeInputCandidate::validate)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(inputs.len(), 16);
        assert!(inputs
            .iter()
            .any(|input| input.path.as_str() == "world.txt"));
        assert!(inputs
            .iter()
            .any(|input| input.path.as_str() == "generator.bin"));
        assert!(inputs
            .iter()
            .any(|input| input.path.as_str() == "method.txt"));
    }

    #[test]
    fn canonical_keys_preserve_views_while_acquisition_paths_deduplicate() {
        let inputs = validated(
            "pub fn f() {\n\
                 include_str(\"shared.data\");\n\
                 include_bytes(\"shared.data\");\n\
                 include_str(\"shared.data\");\n\
                 include_bytes(\"another.data\");\n\
             }",
        )
        .unwrap();
        let keys = canonical_include_keys(&inputs);
        assert_eq!(keys.len(), 3);
        assert_eq!(keys[0].path.as_str(), "another.data");
        assert_eq!(keys[1].path.as_str(), "shared.data");
        assert_eq!(keys[1].kind, IncludeInputKind::Bytes);
        assert_eq!(keys[2].kind, IncludeInputKind::Str);
        assert_eq!(
            canonical_acquisition_paths(&inputs)
                .iter()
                .map(PortablePath::as_str)
                .collect::<Vec<_>>(),
            ["another.data", "shared.data"]
        );
    }

    fn collect_arc_files(directory: &Path, output: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect_arc_files(&path, output);
            } else if path.extension().is_some_and(|extension| extension == "arc") {
                output.push(path);
            }
        }
    }

    #[test]
    fn both_real_corpora_are_covered_and_pin_the_two_environment_inputs() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../tests/m27c1");
        let mut discovered = Vec::new();
        for corpus in ["language-game", "language-environment"] {
            let mut files = Vec::new();
            collect_arc_files(&root.join(corpus), &mut files);
            files.sort();
            for (index, path) in files.into_iter().enumerate() {
                let bytes = fs::read(&path).unwrap();
                let file = parse_reader(FileId(u64::try_from(index).unwrap()), Cursor::new(bytes))
                    .unwrap();
                discovered.extend(
                    find_include_input_candidates(&file)
                        .iter()
                        .map(IncludeInputCandidate::validate)
                        .collect::<Result<Vec<_>, _>>()
                        .unwrap(),
                );
            }
        }
        assert_eq!(
            canonical_include_keys(&discovered)
                .iter()
                .map(|key| (key.path.as_str(), key.kind))
                .collect::<Vec<_>>(),
            [
                ("data/message.txt", IncludeInputKind::Str),
                ("data/table.bin", IncludeInputKind::Bytes),
            ]
        );
    }
}
