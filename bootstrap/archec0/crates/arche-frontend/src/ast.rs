//! Complete, syntax-preserving M27-C1 AST.
//!
//! These nodes contain session-local source structure only. They deliberately
//! carry no stable definition/type identity and perform no semantic checking.

use std::fmt::Write as _;
use std::sync::Arc;

use crate::lexer::{FloatLiteral, FloatSuffix, IntegerLiteral, IntegerSuffix, NumericBase};
use crate::{Span, Symbol};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstNode<T> {
    pub kind: T,
    pub span: Span,
}

impl<T> AstNode<T> {
    pub const fn new(kind: T, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstFile {
    pub items: Vec<AstItem>,
    pub eof_span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstDocComment {
    /// Exact UTF-8 bytes after `///` and before CR/LF.
    pub text: Arc<str>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstItem {
    Module(AstModule),
    Import(AstImport),
    Declaration(Box<AstDeclaration>),
    Impl(Box<AstImpl>),
}

impl AstItem {
    pub const fn span(&self) -> Span {
        match self {
            Self::Module(item) => item.span,
            Self::Import(item) => item.span,
            Self::Declaration(item) => item.span,
            Self::Impl(item) => item.span,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstVisibility {
    pub kind: AstVisibilityKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstVisibilityKind {
    Private,
    Public,
    Package,
    Super,
    In(AstPath),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstPathRoot {
    Bare,
    Package,
    SelfValue,
    SelfType,
    Super(u64),
    /// A neutral first segment. Resolution determines whether this is a
    /// dependency alias, type, generic, local, or module binding.
    Identifier(Symbol),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstPath {
    pub root: AstPathRoot,
    pub segments: Vec<AstPathSegment>,
    pub generic_arguments: Option<AstGenericArguments>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstPathSegment {
    pub name: Symbol,
    pub generic_arguments: Option<AstGenericArguments>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstModule {
    pub docs: Vec<AstDocComment>,
    pub visibility: AstVisibility,
    pub name: Symbol,
    pub name_span: Span,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstImport {
    pub docs: Vec<AstDocComment>,
    pub visibility: AstVisibility,
    pub path: AstPath,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstDeclaration {
    pub docs: Vec<AstDocComment>,
    pub visibility: AstVisibility,
    pub name: Symbol,
    pub name_span: Span,
    pub kind: AstDeclarationKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstDeclarationKind {
    World { initializer: Box<AstWorldInitBlock> },
    Component(Box<AstRecordDeclaration>),
    Resource(Box<AstRecordDeclaration>),
    Tag,
    Struct(Box<AstStructDeclaration>),
    Enum(Box<AstEnumDeclaration>),
    TypeAlias(Box<AstTypeAlias>),
    Const(Box<AstConstItem>),
    Static(Box<AstStaticItem>),
    Function(Box<AstFunction>),
    Generator(Box<AstGenerator>),
    System(Box<AstSystem>),
    Schedule(Box<AstSchedule>),
    Trait(Box<AstTrait>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstGenericParameters {
    pub parameters: Vec<AstGenericParameter>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstGenericParameter {
    pub kind: AstGenericParameterKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstGenericParameterKind {
    Lifetime {
        name: Symbol,
        outlives: Option<Symbol>,
    },
    Type {
        name: Symbol,
        bounds: Vec<AstTypeBound>,
    },
    IntegerConst {
        name: Symbol,
        ty: AstIntegerType,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstGenericArguments {
    pub arguments: Vec<AstGenericArgument>,
    /// True only for the explicit value-position `::<...>` spelling.
    pub turbofish: bool,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstGenericArgument {
    pub kind: AstGenericArgumentKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstGenericArgumentKind {
    Type(AstType),
    Lifetime(Symbol),
    IntegerConst(AstConstExpression),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstWhereClause {
    pub predicates: Vec<AstWherePredicate>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstWherePredicate {
    pub kind: AstWherePredicateKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstWherePredicateKind {
    Type {
        ty: Box<AstType>,
        bounds: Vec<AstTypeBound>,
    },
    Lifetime {
        lifetime: Symbol,
        outlives: Symbol,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstTypeBound {
    pub kind: AstTypeBoundKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstTypeBoundKind {
    Trait(AstPath),
    Lifetime(Symbol),
}

pub type AstType = AstNode<AstTypeKind>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstTypeKind {
    Scalar(AstScalarType),
    Never,
    Unit,
    Str,
    SelfType,
    Path(AstPath),
    Tuple(Vec<AstType>),
    Array {
        element: Box<AstType>,
        length: AstConstExpression,
    },
    Slice(Box<AstType>),
    Reference {
        lifetime: Option<Symbol>,
        mutable: bool,
        pointee: Box<AstType>,
    },
    RawPointer {
        mutable: bool,
        pointee: Box<AstType>,
    },
    FunctionPointer {
        unsafe_: bool,
        parameters: Vec<AstType>,
        effects: AstEffectSets,
        result: Option<Box<AstType>>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AstIntegerType {
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
pub enum AstScalarType {
    Integer(AstIntegerType),
    F32,
    F64,
    Bool,
    Char,
    Entity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstEffectSets {
    /// `None` preserves omission; `Some(empty)` preserves an explicitly empty
    /// boundary, which differs for closure/generator inference.
    pub requires: Option<AstEffectSet<AstPath>>,
    pub throws: Option<AstEffectSet<AstType>>,
    pub span: Option<Span>,
}

impl AstEffectSets {
    pub const fn omitted() -> Self {
        Self {
            requires: None,
            throws: None,
            span: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstEffectSet<T> {
    pub members: Vec<T>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstRecordDeclaration {
    pub generics: Option<AstGenericParameters>,
    pub where_clause: Option<AstWhereClause>,
    pub fields: Vec<AstRecordField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstRecordField {
    pub docs: Vec<AstDocComment>,
    pub visibility: AstVisibility,
    pub name: Symbol,
    pub ty: AstType,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstTupleField {
    pub visibility: AstVisibility,
    pub ty: AstType,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstStructDeclaration {
    pub generics: Option<AstGenericParameters>,
    pub where_clause: Option<AstWhereClause>,
    pub form: AstStructForm,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstStructForm {
    Unit,
    Tuple(Vec<AstTupleField>),
    Record(Vec<AstRecordField>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstEnumDeclaration {
    pub generics: Option<AstGenericParameters>,
    pub where_clause: Option<AstWhereClause>,
    pub variants: Vec<AstVariant>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstVariant {
    pub docs: Vec<AstDocComment>,
    pub name: Symbol,
    pub form: AstVariantForm,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstVariantForm {
    Unit,
    Tuple(Vec<AstType>),
    Record(Vec<AstVariantField>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstVariantField {
    pub name: Symbol,
    pub ty: AstType,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstTypeAlias {
    pub generics: Option<AstGenericParameters>,
    pub target: AstType,
    pub where_clause: Option<AstWhereClause>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstConstItem {
    pub ty: AstType,
    pub value: AstExpression,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstStaticItem {
    pub mutable: bool,
    pub ty: AstType,
    pub value: AstExpression,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstFunction {
    pub signature: AstFunctionSignature,
    pub body: AstBlock,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstFunctionSignature {
    pub unsafe_: bool,
    pub generics: Option<AstGenericParameters>,
    pub parameters: Vec<AstParameter>,
    pub effects: AstEffectSets,
    pub result: Option<AstType>,
    pub where_clause: Option<AstWhereClause>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstGenerator {
    pub unsafe_: bool,
    pub generics: Option<AstGenericParameters>,
    pub parameters: Vec<AstParameter>,
    pub resume: AstType,
    pub yields: AstType,
    pub effects: AstEffectSets,
    pub result: Option<AstType>,
    pub where_clause: Option<AstWhereClause>,
    pub body: AstBlock,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstParameter {
    pub pattern: AstPattern,
    pub ty: AstType,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstTrait {
    pub generics: Option<AstGenericParameters>,
    pub where_clause: Option<AstWhereClause>,
    pub methods: Vec<AstTraitMethod>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstTraitMethod {
    pub docs: Vec<AstDocComment>,
    pub name: AstMethodName,
    pub signature: AstMethodSignature,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstMethodSignature {
    pub unsafe_: bool,
    pub generics: Option<AstGenericParameters>,
    pub parameters: Vec<AstMethodParameter>,
    pub effects: AstEffectSets,
    pub result: Option<AstType>,
    pub where_clause: Option<AstWhereClause>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstMethodParameter {
    Receiver(AstReceiver),
    Parameter(Box<AstParameter>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstReceiver {
    pub kind: AstReceiverKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstReceiverKind {
    Value {
        mutable: bool,
    },
    Reference {
        lifetime: Option<Symbol>,
        mutable: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstImpl {
    pub docs: Vec<AstDocComment>,
    pub is_default: bool,
    pub generics: Option<AstGenericParameters>,
    pub trait_path: Option<AstPath>,
    pub target: AstType,
    pub where_clause: Option<AstWhereClause>,
    pub methods: Vec<AstImplMethod>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstImplMethod {
    pub docs: Vec<AstDocComment>,
    pub visibility: AstVisibility,
    pub name: AstMethodName,
    pub signature: AstMethodSignature,
    pub body: AstBlock,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstSystem {
    pub generics: Option<AstGenericParameters>,
    pub parameters: Vec<AstSystemParameter>,
    pub effects: AstEffectSets,
    pub where_clause: Option<AstWhereClause>,
    pub body: AstBlock,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstSystemParameter {
    pub name: Symbol,
    pub kind: AstSystemParameterKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstSystemParameterKind {
    ResourceRead(AstType),
    ResourceWrite(AstType),
    Query(Vec<AstQueryTerm>),
    Commands,
    Capability(AstType),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstQueryTerm {
    pub kind: AstQueryTermKind,
    pub ty: AstType,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AstQueryTermKind {
    Read,
    Write,
    Exclude,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstSchedule {
    pub runs: Vec<AstScheduleRun>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstScheduleRun {
    pub target: AstPath,
    pub arguments: Option<AstSystemGenericArguments>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstSystemGenericArguments {
    pub arguments: Vec<AstSystemGenericArgument>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstSystemGenericArgument {
    Type(AstType),
    IntegerConst(AstConstExpression),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstWorldInitBlock {
    pub entries: Vec<AstWorldInit>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstWorldInit {
    pub kind: AstWorldInitKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstWorldInitKind {
    Resource {
        ty: Box<AstType>,
        value: Box<AstExpression>,
    },
    Spawn {
        values: Vec<AstExpression>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstBlock {
    pub statements: Vec<AstStatement>,
    pub tail: Option<Box<AstExpression>>,
    pub span: Span,
}

pub type AstStatement = AstNode<AstStatementKind>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstStatementKind {
    Let {
        pattern: Box<AstPattern>,
        ty: Option<Box<AstType>>,
        value: Box<AstExpression>,
        else_block: Option<Box<AstBlock>>,
    },
    For {
        pattern: Box<AstPattern>,
        iterator: Box<AstExpression>,
        body: Box<AstBlock>,
        semicolon: bool,
    },
    Assignment {
        place: Box<AstExpression>,
        operator: AstAssignmentOperator,
        value: Box<AstExpression>,
    },
    Expression {
        expression: Box<AstExpression>,
        semicolon: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AstAssignmentOperator {
    Assign,
    AddAssign,
}

pub type AstExpression = AstNode<AstExpressionKind>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstExpressionKind {
    Literal(AstLiteral),
    Path(AstPath),
    SelfValue,
    Unit,
    Group(Box<AstExpression>),
    Tuple(Vec<AstExpression>),
    Array(Vec<AstExpression>),
    ArrayRepeat {
        value: Box<AstExpression>,
        count: AstConstExpression,
    },
    Record {
        constructor: AstPath,
        fields: Vec<AstRecordExpressionField>,
    },
    Block(AstBlock),
    If(Box<AstIfExpression>),
    While(Box<AstWhileExpression>),
    Loop(AstBlock),
    Match {
        operand: Box<AstExpression>,
        arms: Vec<AstMatchArm>,
    },
    Catch {
        operand: Box<AstExpression>,
        arms: Vec<AstMatchArm>,
    },
    Unsafe(AstBlock),
    Closure(Box<AstClosure>),
    GeneratorClosure(Box<AstGeneratorClosure>),
    Return(Option<Box<AstExpression>>),
    Break(Option<Box<AstExpression>>),
    Continue,
    Throw(Option<Box<AstExpression>>),
    Yield(Box<AstExpression>),
    Unary {
        operator: AstUnaryOperator,
        operand: Box<AstExpression>,
    },
    Binary {
        operator: AstBinaryOperator,
        left: Box<AstExpression>,
        right: Box<AstExpression>,
    },
    Cast {
        value: Box<AstExpression>,
        ty: AstType,
    },
    Postfix {
        base: Box<AstExpression>,
        parts: Vec<AstPostfix>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstLiteral {
    Integer(IntegerLiteral),
    Float(FloatLiteral),
    Character(char),
    String(Arc<str>),
    Boolean(bool),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstRecordExpressionField {
    pub name: Symbol,
    pub value: AstExpression,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstIfExpression {
    pub condition: AstCondition,
    pub then_block: AstBlock,
    pub else_branch: Option<AstElseBranch>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstWhileExpression {
    pub condition: AstCondition,
    pub body: AstBlock,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstCondition {
    Expression(Box<AstExpression>),
    Let {
        pattern: Box<AstPattern>,
        value: Box<AstExpression>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstElseBranch {
    Block(AstBlock),
    If(Box<AstExpression>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstMatchArm {
    pub pattern: AstPattern,
    pub guard: Option<AstExpression>,
    pub value: AstExpression,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstClosure {
    pub move_: bool,
    pub parameters: Vec<AstClosureParameter>,
    pub effects: AstEffectSets,
    pub result: Option<AstType>,
    pub body: Box<AstExpression>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstGeneratorClosure {
    pub move_: bool,
    pub parameters: Vec<AstClosureParameter>,
    pub resume: AstType,
    pub yields: AstType,
    pub effects: AstEffectSets,
    pub result: Option<AstType>,
    pub body: Box<AstExpression>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstClosureParameter {
    pub pattern: AstPattern,
    pub ty: Option<AstType>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstPostfix {
    pub kind: AstPostfixKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstPostfixKind {
    Call(Vec<AstExpression>),
    Index(AstExpression),
    Method {
        name: AstMethodName,
        generic_arguments: Option<AstGenericArguments>,
        arguments: Vec<AstExpression>,
    },
    Field(Symbol),
    TupleField(IntegerLiteral),
    CommandSpawn(Vec<AstExpression>),
    Resume(AstExpression),
    TurbofishCall {
        generic_arguments: AstGenericArguments,
        arguments: Vec<AstExpression>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstMethodName {
    Identifier(Symbol),
    Read,
    Resource,
    Run,
    Spawn,
}

impl AstMethodName {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Identifier(name) => name.as_str(),
            Self::Read => "read",
            Self::Resource => "resource",
            Self::Run => "run",
            Self::Spawn => "spawn",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AstUnaryOperator {
    Negate,
    LogicalNot,
    BitNot,
    Dereference,
    BorrowShared,
    BorrowMutable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AstBinaryOperator {
    LogicalOr,
    LogicalAnd,
    BitOr,
    BitXor,
    BitAnd,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    ShiftLeft,
    ShiftRight,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

pub type AstPattern = AstNode<AstPatternKind>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstPatternKind {
    Wildcard,
    Unit,
    Literal(AstPatternLiteral),
    /// Resolution decides whether an unqualified identifier is a binding,
    /// const, or unit variant. Explicit `mut`/`ref` modes remain separate.
    BarePathOrBinding(AstPath),
    Binding {
        name: Symbol,
        mutable: bool,
        by_reference: bool,
        reference_mutable: bool,
    },
    Reference {
        mutable: bool,
        pattern: Box<AstPattern>,
    },
    Tuple(Vec<AstPattern>),
    Slice(Vec<AstSlicePatternPart>),
    Constructor {
        path: AstPath,
        payload: AstConstructorPatternPayload,
    },
    Range {
        inclusive: bool,
        start: AstRangeEndpoint,
        end: AstRangeEndpoint,
    },
    At {
        binding: Box<AstPattern>,
        pattern: Box<AstPattern>,
    },
    Or(Vec<AstPattern>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstPatternLiteral {
    Integer {
        negative: bool,
        literal: IntegerLiteral,
    },
    Character(char),
    String(Arc<str>),
    Boolean(bool),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstSlicePatternPart {
    Pattern(Box<AstPattern>),
    Rest(Span),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstConstructorPatternPayload {
    Unit,
    Tuple(Vec<AstPattern>),
    Record(Vec<AstRecordPatternField>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AstRecordPatternField {
    pub name: Symbol,
    pub pattern: AstPattern,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstRangeEndpoint {
    Integer {
        negative: bool,
        literal: IntegerLiteral,
        span: Span,
    },
    Character {
        value: char,
        span: Span,
    },
    Const(AstPath),
}

pub type AstConstExpression = AstNode<AstConstExpressionKind>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AstConstExpressionKind {
    Integer(IntegerLiteral),
    Path(AstPath),
    Group(Box<AstConstExpression>),
    Unary {
        operator: AstConstUnaryOperator,
        operand: Box<AstConstExpression>,
    },
    Binary {
        operator: AstConstBinaryOperator,
        left: Box<AstConstExpression>,
        right: Box<AstConstExpression>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AstConstUnaryOperator {
    Negate,
    BitNot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AstConstBinaryOperator {
    BitOr,
    BitXor,
    BitAnd,
    ShiftLeft,
    ShiftRight,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

/// Canonical C1 AST debug/golden envelope.
pub fn dump_ast(file: &AstFile) -> String {
    let mut printer = AstPrinter::new();
    printer.output.push_str("ARCHE-AST-TEXT 1\n");
    printer.file(file);
    printer.output.push('\n');
    printer.output
}

struct AstPrinter {
    output: String,
}

impl AstPrinter {
    fn new() -> Self {
        Self {
            output: String::new(),
        }
    }

    fn form(&mut self, name: &str, body: impl FnOnce(&mut Self)) {
        self.output.push('(');
        self.output.push_str(name);
        body(self);
        self.output.push(')');
    }

    fn nested_form(&mut self, name: &str, body: impl FnOnce(&mut Self)) {
        self.output.push(' ');
        self.form(name, body);
    }

    fn field(&mut self, name: &str, body: impl FnOnce(&mut Self)) {
        self.nested_form(name, body);
    }

    fn atom(&mut self, value: &str) {
        self.output.push(' ');
        self.output.push_str(value);
    }

    fn boolean(&mut self, value: bool) {
        self.atom(if value { "true" } else { "false" });
    }

    fn unsigned(&mut self, value: u64) {
        self.output.push(' ');
        let _ = write!(self.output, "{value}");
    }

    fn string_value(&mut self, value: &str) {
        self.output.push(' ');
        self.string(value);
    }

    fn file(&mut self, file: &AstFile) {
        self.form("file", |printer| {
            printer.field("items", |printer| {
                for item in &file.items {
                    printer.item(item);
                }
            });
            printer.field("eof-span", |printer| printer.span(file.eof_span));
        });
    }

    fn item(&mut self, item: &AstItem) {
        match item {
            AstItem::Module(module) => {
                self.nested_form("module", |printer| {
                    printer.docs(&module.docs);
                    printer.field("visibility", |printer| {
                        printer.visibility(&module.visibility);
                    });
                    printer.field("name", |printer| {
                        printer.string_value(module.name.as_str());
                    });
                    printer.field("name-span", |printer| printer.span(module.name_span));
                    printer.field("span", |printer| printer.span(module.span));
                });
            }
            AstItem::Import(import) => {
                self.nested_form("import", |printer| {
                    printer.docs(&import.docs);
                    printer.field("visibility", |printer| {
                        printer.visibility(&import.visibility);
                    });
                    printer.field("path", |printer| printer.path(&import.path));
                    printer.field("span", |printer| printer.span(import.span));
                });
            }
            AstItem::Declaration(declaration) => self.declaration(declaration),
            AstItem::Impl(item) => self.impl_item(item),
        }
    }

    fn declaration(&mut self, declaration: &AstDeclaration) {
        self.nested_form("declaration", |printer| {
            printer.docs(&declaration.docs);
            printer.field("visibility", |printer| {
                printer.visibility(&declaration.visibility);
            });
            printer.field("name", |printer| {
                printer.string_value(declaration.name.as_str());
            });
            printer.field("name-span", |printer| printer.span(declaration.name_span));
            printer.field("kind", |printer| {
                printer.declaration_kind(&declaration.kind);
            });
            printer.field("span", |printer| printer.span(declaration.span));
        });
    }

    fn declaration_kind(&mut self, kind: &AstDeclarationKind) {
        match kind {
            AstDeclarationKind::World { initializer } => {
                self.nested_form("world", |printer| {
                    printer.field("initializer", |printer| {
                        printer.world_init_block(initializer);
                    });
                });
            }
            AstDeclarationKind::Component(record) => self.record("component", record),
            AstDeclarationKind::Resource(record) => self.record("resource", record),
            AstDeclarationKind::Tag => self.nested_form("tag", |_| {}),
            AstDeclarationKind::Struct(item) => self.struct_declaration(item),
            AstDeclarationKind::Enum(item) => self.enum_declaration(item),
            AstDeclarationKind::TypeAlias(item) => self.type_alias(item),
            AstDeclarationKind::Const(item) => self.const_item(item),
            AstDeclarationKind::Static(item) => self.static_item(item),
            AstDeclarationKind::Function(item) => self.function(item),
            AstDeclarationKind::Generator(item) => self.generator(item),
            AstDeclarationKind::System(item) => self.system(item),
            AstDeclarationKind::Schedule(item) => self.schedule(item),
            AstDeclarationKind::Trait(item) => self.trait_item(item),
        }
    }

    fn record(&mut self, name: &str, record: &AstRecordDeclaration) {
        self.nested_form(name, |printer| {
            printer.optional_generic_parameters("generics", record.generics.as_ref());
            printer.optional_where_clause(record.where_clause.as_ref());
            printer.field("fields", |printer| {
                for field in &record.fields {
                    printer.record_field(field);
                }
            });
        });
    }

    fn docs(&mut self, docs: &[AstDocComment]) {
        self.field("docs", |printer| {
            for doc in docs {
                printer.nested_form("doc-comment", |printer| {
                    printer.field("text", |printer| printer.string_value(&doc.text));
                    printer.field("span", |printer| printer.span(doc.span));
                });
            }
        });
    }

    fn visibility(&mut self, visibility: &AstVisibility) {
        self.nested_form("visibility", |printer| {
            printer.field("kind", |printer| match &visibility.kind {
                AstVisibilityKind::Private => printer.nested_form("private", |_| {}),
                AstVisibilityKind::Public => printer.nested_form("public", |_| {}),
                AstVisibilityKind::Package => printer.nested_form("package", |_| {}),
                AstVisibilityKind::Super => printer.nested_form("super", |_| {}),
                AstVisibilityKind::In(path) => printer.nested_form("in", |printer| {
                    printer.field("path", |printer| printer.path(path));
                }),
            });
            printer.field("span", |printer| printer.span(visibility.span));
        });
    }

    fn path(&mut self, path: &AstPath) {
        self.nested_form("path", |printer| {
            printer.field("root", |printer| printer.path_root(&path.root));
            printer.field("segments", |printer| {
                for segment in &path.segments {
                    printer.path_segment(segment);
                }
            });
            printer
                .optional_generic_arguments("generic-arguments", path.generic_arguments.as_ref());
            printer.field("span", |printer| printer.span(path.span));
        });
    }

    fn path_root(&mut self, root: &AstPathRoot) {
        match root {
            AstPathRoot::Bare => self.nested_form("bare", |_| {}),
            AstPathRoot::Package => self.nested_form("package", |_| {}),
            AstPathRoot::SelfValue => self.nested_form("self-value", |_| {}),
            AstPathRoot::SelfType => self.nested_form("self-type", |_| {}),
            AstPathRoot::Super(depth) => self.nested_form("super", |printer| {
                printer.field("depth", |printer| printer.unsigned(*depth));
            }),
            AstPathRoot::Identifier(name) => self.nested_form("identifier", |printer| {
                printer.field("name", |printer| printer.string_value(name.as_str()));
            }),
        }
    }

    fn path_segment(&mut self, segment: &AstPathSegment) {
        self.nested_form("path-segment", |printer| {
            printer.field("name", |printer| {
                printer.string_value(segment.name.as_str());
            });
            printer.optional_generic_arguments(
                "generic-arguments",
                segment.generic_arguments.as_ref(),
            );
            printer.field("span", |printer| printer.span(segment.span));
        });
    }

    fn optional_generic_arguments(&mut self, name: &str, value: Option<&AstGenericArguments>) {
        self.field(name, |printer| match value {
            Some(arguments) => printer.generic_arguments(arguments),
            None => printer.atom("none"),
        });
    }

    fn generic_arguments(&mut self, arguments: &AstGenericArguments) {
        self.nested_form("generic-arguments", |printer| {
            printer.field("arguments", |printer| {
                for argument in &arguments.arguments {
                    printer.generic_argument(argument);
                }
            });
            printer.field("turbofish", |printer| printer.boolean(arguments.turbofish));
            printer.field("span", |printer| printer.span(arguments.span));
        });
    }

    fn generic_argument(&mut self, argument: &AstGenericArgument) {
        self.nested_form("generic-argument", |printer| {
            printer.field("kind", |printer| match &argument.kind {
                AstGenericArgumentKind::Type(ty) => printer.nested_form("type", |printer| {
                    printer.field("value", |printer| printer.ty(ty));
                }),
                AstGenericArgumentKind::Lifetime(lifetime) => {
                    printer.nested_form("lifetime", |printer| {
                        printer.field("name", |printer| {
                            printer.string_value(lifetime.as_str());
                        });
                    });
                }
                AstGenericArgumentKind::IntegerConst(expression) => {
                    printer.nested_form("integer-const", |printer| {
                        printer.field("value", |printer| {
                            printer.const_expression(expression);
                        });
                    });
                }
            });
            printer.field("span", |printer| printer.span(argument.span));
        });
    }

    fn ty(&mut self, ty: &AstType) {
        self.nested_form("type", |printer| {
            printer.field("kind", |printer| match &ty.kind {
                AstTypeKind::Scalar(value) => printer.nested_form("scalar", |printer| {
                    printer.field("value", |printer| printer.atom(scalar_type_name(*value)));
                }),
                AstTypeKind::Never => printer.nested_form("never", |_| {}),
                AstTypeKind::Unit => printer.nested_form("unit", |_| {}),
                AstTypeKind::Str => printer.nested_form("str", |_| {}),
                AstTypeKind::SelfType => printer.nested_form("self-type", |_| {}),
                AstTypeKind::Path(path) => printer.nested_form("path", |printer| {
                    printer.field("value", |printer| printer.path(path));
                }),
                AstTypeKind::Tuple(elements) => printer.nested_form("tuple", |printer| {
                    printer.field("elements", |printer| {
                        for element in elements {
                            printer.ty(element);
                        }
                    });
                }),
                AstTypeKind::Array { element, length } => {
                    printer.nested_form("array", |printer| {
                        printer.field("element", |printer| printer.ty(element));
                        printer.field("length", |printer| {
                            printer.const_expression(length);
                        });
                    });
                }
                AstTypeKind::Slice(element) => printer.nested_form("slice", |printer| {
                    printer.field("element", |printer| printer.ty(element));
                }),
                AstTypeKind::Reference {
                    lifetime,
                    mutable,
                    pointee,
                } => printer.nested_form("reference", |printer| {
                    printer.optional_symbol("lifetime", lifetime.as_ref());
                    printer.field("mutable", |printer| printer.boolean(*mutable));
                    printer.field("pointee", |printer| printer.ty(pointee));
                }),
                AstTypeKind::RawPointer { mutable, pointee } => {
                    printer.nested_form("raw-pointer", |printer| {
                        printer.field("mutable", |printer| printer.boolean(*mutable));
                        printer.field("pointee", |printer| printer.ty(pointee));
                    });
                }
                AstTypeKind::FunctionPointer {
                    unsafe_,
                    parameters,
                    effects,
                    result,
                } => printer.nested_form("function-pointer", |printer| {
                    printer.field("unsafe", |printer| printer.boolean(*unsafe_));
                    printer.field("parameters", |printer| {
                        for parameter in parameters {
                            printer.ty(parameter);
                        }
                    });
                    printer.field("effects", |printer| printer.effect_sets(effects));
                    printer.optional_type("result", result.as_deref());
                }),
            });
            printer.field("span", |printer| printer.span(ty.span));
        });
    }

    fn optional_symbol(&mut self, name: &str, value: Option<&Symbol>) {
        self.field(name, |printer| match value {
            Some(value) => printer.string_value(value.as_str()),
            None => printer.atom("none"),
        });
    }

    fn optional_generic_parameters(&mut self, name: &str, value: Option<&AstGenericParameters>) {
        self.field(name, |printer| match value {
            Some(parameters) => printer.generic_parameters(parameters),
            None => printer.atom("none"),
        });
    }

    fn generic_parameters(&mut self, parameters: &AstGenericParameters) {
        self.nested_form("generic-parameters", |printer| {
            printer.field("parameters", |printer| {
                for parameter in &parameters.parameters {
                    printer.generic_parameter(parameter);
                }
            });
            printer.field("span", |printer| printer.span(parameters.span));
        });
    }

    fn generic_parameter(&mut self, parameter: &AstGenericParameter) {
        self.nested_form("generic-parameter", |printer| {
            printer.field("kind", |printer| match &parameter.kind {
                AstGenericParameterKind::Lifetime { name, outlives } => {
                    printer.nested_form("lifetime", |printer| {
                        printer.field("name", |printer| {
                            printer.string_value(name.as_str());
                        });
                        printer.optional_symbol("outlives", outlives.as_ref());
                    });
                }
                AstGenericParameterKind::Type { name, bounds } => {
                    printer.nested_form("type", |printer| {
                        printer.field("name", |printer| {
                            printer.string_value(name.as_str());
                        });
                        printer.type_bounds(bounds);
                    });
                }
                AstGenericParameterKind::IntegerConst { name, ty } => {
                    printer.nested_form("integer-const", |printer| {
                        printer.field("name", |printer| {
                            printer.string_value(name.as_str());
                        });
                        printer.field("type", |printer| printer.atom(integer_type_name(*ty)));
                    });
                }
            });
            printer.field("span", |printer| printer.span(parameter.span));
        });
    }

    fn optional_where_clause(&mut self, value: Option<&AstWhereClause>) {
        self.field("where-clause", |printer| match value {
            Some(clause) => printer.where_clause(clause),
            None => printer.atom("none"),
        });
    }

    fn where_clause(&mut self, clause: &AstWhereClause) {
        self.nested_form("where-clause", |printer| {
            printer.field("predicates", |printer| {
                for predicate in &clause.predicates {
                    printer.where_predicate(predicate);
                }
            });
            printer.field("span", |printer| printer.span(clause.span));
        });
    }

    fn where_predicate(&mut self, predicate: &AstWherePredicate) {
        self.nested_form("where-predicate", |printer| {
            printer.field("kind", |printer| match &predicate.kind {
                AstWherePredicateKind::Type { ty, bounds } => {
                    printer.nested_form("type", |printer| {
                        printer.field("type", |printer| printer.ty(ty));
                        printer.type_bounds(bounds);
                    });
                }
                AstWherePredicateKind::Lifetime { lifetime, outlives } => {
                    printer.nested_form("lifetime", |printer| {
                        printer.field("lifetime", |printer| {
                            printer.string_value(lifetime.as_str());
                        });
                        printer.field("outlives", |printer| {
                            printer.string_value(outlives.as_str());
                        });
                    })
                }
            });
            printer.field("span", |printer| printer.span(predicate.span));
        });
    }

    fn type_bounds(&mut self, bounds: &[AstTypeBound]) {
        self.field("bounds", |printer| {
            for bound in bounds {
                printer.type_bound(bound);
            }
        });
    }

    fn type_bound(&mut self, bound: &AstTypeBound) {
        self.nested_form("type-bound", |printer| {
            printer.field("kind", |printer| match &bound.kind {
                AstTypeBoundKind::Trait(path) => printer.nested_form("trait", |printer| {
                    printer.field("path", |printer| printer.path(path));
                }),
                AstTypeBoundKind::Lifetime(lifetime) => {
                    printer.nested_form("lifetime", |printer| {
                        printer.field("name", |printer| {
                            printer.string_value(lifetime.as_str());
                        });
                    });
                }
            });
            printer.field("span", |printer| printer.span(bound.span));
        });
    }

    fn effect_sets(&mut self, effects: &AstEffectSets) {
        self.nested_form("effect-sets", |printer| {
            printer.field("requires", |printer| match &effects.requires {
                Some(effect_set) => printer.path_effect_set(effect_set),
                None => printer.atom("none"),
            });
            printer.field("throws", |printer| match &effects.throws {
                Some(effect_set) => printer.type_effect_set(effect_set),
                None => printer.atom("none"),
            });
            printer.field("span", |printer| match effects.span {
                Some(span) => printer.span(span),
                None => printer.atom("none"),
            });
        });
    }

    fn path_effect_set(&mut self, effect_set: &AstEffectSet<AstPath>) {
        self.nested_form("effect-set", |printer| {
            printer.field("members", |printer| {
                for member in &effect_set.members {
                    printer.path(member);
                }
            });
            printer.field("span", |printer| printer.span(effect_set.span));
        });
    }

    fn type_effect_set(&mut self, effect_set: &AstEffectSet<AstType>) {
        self.nested_form("effect-set", |printer| {
            printer.field("members", |printer| {
                for member in &effect_set.members {
                    printer.ty(member);
                }
            });
            printer.field("span", |printer| printer.span(effect_set.span));
        });
    }

    fn record_field(&mut self, field: &AstRecordField) {
        self.nested_form("record-field", |printer| {
            printer.docs(&field.docs);
            printer.field("visibility", |printer| {
                printer.visibility(&field.visibility)
            });
            printer.field("name", |printer| {
                printer.string_value(field.name.as_str());
            });
            printer.field("type", |printer| printer.ty(&field.ty));
            printer.field("span", |printer| printer.span(field.span));
        });
    }

    fn tuple_field(&mut self, field: &AstTupleField) {
        self.nested_form("tuple-field", |printer| {
            printer.field("visibility", |printer| {
                printer.visibility(&field.visibility)
            });
            printer.field("type", |printer| printer.ty(&field.ty));
            printer.field("span", |printer| printer.span(field.span));
        });
    }

    fn struct_declaration(&mut self, item: &AstStructDeclaration) {
        self.nested_form("struct", |printer| {
            printer.optional_generic_parameters("generics", item.generics.as_ref());
            printer.optional_where_clause(item.where_clause.as_ref());
            printer.field("form", |printer| printer.struct_form(&item.form));
        });
    }

    fn struct_form(&mut self, form: &AstStructForm) {
        match form {
            AstStructForm::Unit => self.nested_form("unit", |_| {}),
            AstStructForm::Tuple(fields) => self.nested_form("tuple", |printer| {
                printer.field("fields", |printer| {
                    for field in fields {
                        printer.tuple_field(field);
                    }
                });
            }),
            AstStructForm::Record(fields) => self.nested_form("record", |printer| {
                printer.field("fields", |printer| {
                    for field in fields {
                        printer.record_field(field);
                    }
                });
            }),
        }
    }

    fn enum_declaration(&mut self, item: &AstEnumDeclaration) {
        self.nested_form("enum", |printer| {
            printer.optional_generic_parameters("generics", item.generics.as_ref());
            printer.optional_where_clause(item.where_clause.as_ref());
            printer.field("variants", |printer| {
                for variant in &item.variants {
                    printer.variant(variant);
                }
            });
        });
    }

    fn variant(&mut self, variant: &AstVariant) {
        self.nested_form("variant", |printer| {
            printer.docs(&variant.docs);
            printer.field("name", |printer| {
                printer.string_value(variant.name.as_str());
            });
            printer.field("form", |printer| printer.variant_form(&variant.form));
            printer.field("span", |printer| printer.span(variant.span));
        });
    }

    fn variant_form(&mut self, form: &AstVariantForm) {
        match form {
            AstVariantForm::Unit => self.nested_form("unit", |_| {}),
            AstVariantForm::Tuple(types) => self.nested_form("tuple", |printer| {
                printer.field("types", |printer| {
                    for ty in types {
                        printer.ty(ty);
                    }
                });
            }),
            AstVariantForm::Record(fields) => self.nested_form("record", |printer| {
                printer.field("fields", |printer| {
                    for field in fields {
                        printer.variant_field(field);
                    }
                });
            }),
        }
    }

    fn variant_field(&mut self, field: &AstVariantField) {
        self.nested_form("variant-field", |printer| {
            printer.field("name", |printer| {
                printer.string_value(field.name.as_str());
            });
            printer.field("type", |printer| printer.ty(&field.ty));
            printer.field("span", |printer| printer.span(field.span));
        });
    }

    fn type_alias(&mut self, item: &AstTypeAlias) {
        self.nested_form("type-alias", |printer| {
            printer.optional_generic_parameters("generics", item.generics.as_ref());
            printer.field("target", |printer| printer.ty(&item.target));
            printer.optional_where_clause(item.where_clause.as_ref());
        });
    }

    fn const_item(&mut self, item: &AstConstItem) {
        self.nested_form("const", |printer| {
            printer.field("type", |printer| printer.ty(&item.ty));
            printer.field("value", |printer| printer.expression(&item.value));
        });
    }

    fn static_item(&mut self, item: &AstStaticItem) {
        self.nested_form("static", |printer| {
            printer.field("mutable", |printer| printer.boolean(item.mutable));
            printer.field("type", |printer| printer.ty(&item.ty));
            printer.field("value", |printer| printer.expression(&item.value));
        });
    }

    fn function(&mut self, item: &AstFunction) {
        self.nested_form("function", |printer| {
            printer.field("signature", |printer| {
                printer.function_signature(&item.signature);
            });
            printer.field("body", |printer| printer.block(&item.body));
        });
    }

    fn function_signature(&mut self, signature: &AstFunctionSignature) {
        self.nested_form("function-signature", |printer| {
            printer.field("unsafe", |printer| printer.boolean(signature.unsafe_));
            printer.optional_generic_parameters("generics", signature.generics.as_ref());
            printer.field("parameters", |printer| {
                for parameter in &signature.parameters {
                    printer.parameter(parameter);
                }
            });
            printer.field("effects", |printer| printer.effect_sets(&signature.effects));
            printer.optional_type("result", signature.result.as_ref());
            printer.optional_where_clause(signature.where_clause.as_ref());
            printer.field("span", |printer| printer.span(signature.span));
        });
    }

    fn generator(&mut self, item: &AstGenerator) {
        self.nested_form("generator", |printer| {
            printer.field("unsafe", |printer| printer.boolean(item.unsafe_));
            printer.optional_generic_parameters("generics", item.generics.as_ref());
            printer.field("parameters", |printer| {
                for parameter in &item.parameters {
                    printer.parameter(parameter);
                }
            });
            printer.field("resume", |printer| printer.ty(&item.resume));
            printer.field("yields", |printer| printer.ty(&item.yields));
            printer.field("effects", |printer| printer.effect_sets(&item.effects));
            printer.optional_type("result", item.result.as_ref());
            printer.optional_where_clause(item.where_clause.as_ref());
            printer.field("body", |printer| printer.block(&item.body));
        });
    }

    fn optional_type(&mut self, name: &str, value: Option<&AstType>) {
        self.field(name, |printer| match value {
            Some(ty) => printer.ty(ty),
            None => printer.atom("none"),
        });
    }

    fn parameter(&mut self, parameter: &AstParameter) {
        self.nested_form("parameter", |printer| {
            printer.field("pattern", |printer| printer.pattern(&parameter.pattern));
            printer.field("type", |printer| printer.ty(&parameter.ty));
            printer.field("span", |printer| printer.span(parameter.span));
        });
    }

    fn trait_item(&mut self, item: &AstTrait) {
        self.nested_form("trait", |printer| {
            printer.optional_generic_parameters("generics", item.generics.as_ref());
            printer.optional_where_clause(item.where_clause.as_ref());
            printer.field("methods", |printer| {
                for method in &item.methods {
                    printer.trait_method(method);
                }
            });
        });
    }

    fn trait_method(&mut self, method: &AstTraitMethod) {
        self.nested_form("trait-method", |printer| {
            printer.docs(&method.docs);
            printer.field("name", |printer| printer.method_name(&method.name));
            printer.field("signature", |printer| {
                printer.method_signature(&method.signature);
            });
            printer.field("span", |printer| printer.span(method.span));
        });
    }

    fn method_signature(&mut self, signature: &AstMethodSignature) {
        self.nested_form("method-signature", |printer| {
            printer.field("unsafe", |printer| printer.boolean(signature.unsafe_));
            printer.optional_generic_parameters("generics", signature.generics.as_ref());
            printer.field("parameters", |printer| {
                for parameter in &signature.parameters {
                    printer.method_parameter(parameter);
                }
            });
            printer.field("effects", |printer| printer.effect_sets(&signature.effects));
            printer.optional_type("result", signature.result.as_ref());
            printer.optional_where_clause(signature.where_clause.as_ref());
            printer.field("span", |printer| printer.span(signature.span));
        });
    }

    fn method_parameter(&mut self, parameter: &AstMethodParameter) {
        match parameter {
            AstMethodParameter::Receiver(receiver) => {
                self.nested_form("receiver", |printer| {
                    printer.field("value", |printer| printer.receiver(receiver));
                });
            }
            AstMethodParameter::Parameter(parameter) => {
                self.nested_form("parameter", |printer| {
                    printer.field("value", |printer| printer.parameter(parameter));
                });
            }
        }
    }

    fn receiver(&mut self, receiver: &AstReceiver) {
        self.nested_form("receiver", |printer| {
            printer.field("kind", |printer| match &receiver.kind {
                AstReceiverKind::Value { mutable } => {
                    printer.nested_form("value", |printer| {
                        printer.field("mutable", |printer| printer.boolean(*mutable));
                    });
                }
                AstReceiverKind::Reference { lifetime, mutable } => {
                    printer.nested_form("reference", |printer| {
                        printer.optional_symbol("lifetime", lifetime.as_ref());
                        printer.field("mutable", |printer| printer.boolean(*mutable));
                    });
                }
            });
            printer.field("span", |printer| printer.span(receiver.span));
        });
    }

    fn impl_item(&mut self, item: &AstImpl) {
        self.nested_form("impl", |printer| {
            printer.docs(&item.docs);
            printer.field("is-default", |printer| printer.boolean(item.is_default));
            printer.optional_generic_parameters("generics", item.generics.as_ref());
            printer.field("trait-path", |printer| match &item.trait_path {
                Some(path) => printer.path(path),
                None => printer.atom("none"),
            });
            printer.field("target", |printer| printer.ty(&item.target));
            printer.optional_where_clause(item.where_clause.as_ref());
            printer.field("methods", |printer| {
                for method in &item.methods {
                    printer.impl_method(method);
                }
            });
            printer.field("span", |printer| printer.span(item.span));
        });
    }

    fn impl_method(&mut self, method: &AstImplMethod) {
        self.nested_form("impl-method", |printer| {
            printer.docs(&method.docs);
            printer.field("visibility", |printer| {
                printer.visibility(&method.visibility);
            });
            printer.field("name", |printer| printer.method_name(&method.name));
            printer.field("signature", |printer| {
                printer.method_signature(&method.signature);
            });
            printer.field("body", |printer| printer.block(&method.body));
            printer.field("span", |printer| printer.span(method.span));
        });
    }

    fn method_name(&mut self, name: &AstMethodName) {
        match name {
            AstMethodName::Identifier(name) => self.nested_form("identifier", |printer| {
                printer.field("name", |printer| printer.string_value(name.as_str()));
            }),
            AstMethodName::Read => self.nested_form("read", |_| {}),
            AstMethodName::Resource => self.nested_form("resource", |_| {}),
            AstMethodName::Run => self.nested_form("run", |_| {}),
            AstMethodName::Spawn => self.nested_form("spawn", |_| {}),
        }
    }

    fn system(&mut self, item: &AstSystem) {
        self.nested_form("system", |printer| {
            printer.optional_generic_parameters("generics", item.generics.as_ref());
            printer.field("parameters", |printer| {
                for parameter in &item.parameters {
                    printer.system_parameter(parameter);
                }
            });
            printer.field("effects", |printer| printer.effect_sets(&item.effects));
            printer.optional_where_clause(item.where_clause.as_ref());
            printer.field("body", |printer| printer.block(&item.body));
        });
    }

    fn system_parameter(&mut self, parameter: &AstSystemParameter) {
        self.nested_form("system-parameter", |printer| {
            printer.field("name", |printer| {
                printer.string_value(parameter.name.as_str());
            });
            printer.field("kind", |printer| match &parameter.kind {
                AstSystemParameterKind::ResourceRead(ty) => {
                    printer.nested_form("resource-read", |printer| {
                        printer.field("type", |printer| printer.ty(ty));
                    });
                }
                AstSystemParameterKind::ResourceWrite(ty) => {
                    printer.nested_form("resource-write", |printer| {
                        printer.field("type", |printer| printer.ty(ty));
                    });
                }
                AstSystemParameterKind::Query(terms) => {
                    printer.nested_form("query", |printer| {
                        printer.field("terms", |printer| {
                            for term in terms {
                                printer.query_term(term);
                            }
                        });
                    });
                }
                AstSystemParameterKind::Commands => {
                    printer.nested_form("commands", |_| {});
                }
                AstSystemParameterKind::Capability(ty) => {
                    printer.nested_form("capability", |printer| {
                        printer.field("type", |printer| printer.ty(ty));
                    });
                }
            });
            printer.field("span", |printer| printer.span(parameter.span));
        });
    }

    fn query_term(&mut self, term: &AstQueryTerm) {
        self.nested_form("query-term", |printer| {
            printer.field("kind", |printer| {
                printer.atom(match term.kind {
                    AstQueryTermKind::Read => "read",
                    AstQueryTermKind::Write => "write",
                    AstQueryTermKind::Exclude => "exclude",
                });
            });
            printer.field("type", |printer| printer.ty(&term.ty));
            printer.field("span", |printer| printer.span(term.span));
        });
    }

    fn schedule(&mut self, item: &AstSchedule) {
        self.nested_form("schedule", |printer| {
            printer.field("runs", |printer| {
                for run in &item.runs {
                    printer.schedule_run(run);
                }
            });
        });
    }

    fn schedule_run(&mut self, run: &AstScheduleRun) {
        self.nested_form("schedule-run", |printer| {
            printer.field("target", |printer| printer.path(&run.target));
            printer.field("arguments", |printer| match &run.arguments {
                Some(arguments) => printer.system_generic_arguments(arguments),
                None => printer.atom("none"),
            });
            printer.field("span", |printer| printer.span(run.span));
        });
    }

    fn system_generic_arguments(&mut self, arguments: &AstSystemGenericArguments) {
        self.nested_form("system-generic-arguments", |printer| {
            printer.field("arguments", |printer| {
                for argument in &arguments.arguments {
                    printer.system_generic_argument(argument);
                }
            });
            printer.field("span", |printer| printer.span(arguments.span));
        });
    }

    fn system_generic_argument(&mut self, argument: &AstSystemGenericArgument) {
        match argument {
            AstSystemGenericArgument::Type(ty) => self.nested_form("type", |printer| {
                printer.field("value", |printer| printer.ty(ty));
            }),
            AstSystemGenericArgument::IntegerConst(expression) => {
                self.nested_form("integer-const", |printer| {
                    printer.field("value", |printer| printer.const_expression(expression));
                });
            }
        }
    }

    fn world_init_block(&mut self, block: &AstWorldInitBlock) {
        self.nested_form("world-init-block", |printer| {
            printer.field("entries", |printer| {
                for entry in &block.entries {
                    printer.world_init(entry);
                }
            });
            printer.field("span", |printer| printer.span(block.span));
        });
    }

    fn world_init(&mut self, entry: &AstWorldInit) {
        self.nested_form("world-init", |printer| {
            printer.field("kind", |printer| match &entry.kind {
                AstWorldInitKind::Resource { ty, value } => {
                    printer.nested_form("resource", |printer| {
                        printer.field("type", |printer| printer.ty(ty));
                        printer.field("value", |printer| printer.expression(value));
                    });
                }
                AstWorldInitKind::Spawn { values } => {
                    printer.nested_form("spawn", |printer| {
                        printer.field("values", |printer| {
                            for value in values {
                                printer.expression(value);
                            }
                        });
                    });
                }
            });
            printer.field("span", |printer| printer.span(entry.span));
        });
    }

    fn block(&mut self, block: &AstBlock) {
        self.nested_form("block", |printer| {
            printer.field("statements", |printer| {
                for statement in &block.statements {
                    printer.statement(statement);
                }
            });
            printer.field("tail", |printer| match &block.tail {
                Some(expression) => printer.expression(expression),
                None => printer.atom("none"),
            });
            printer.field("span", |printer| printer.span(block.span));
        });
    }

    fn statement(&mut self, statement: &AstStatement) {
        self.nested_form("statement", |printer| {
            printer.field("kind", |printer| match &statement.kind {
                AstStatementKind::Let {
                    pattern,
                    ty,
                    value,
                    else_block,
                } => printer.nested_form("let", |printer| {
                    printer.field("pattern", |printer| printer.pattern(pattern));
                    printer.field("type", |printer| match ty {
                        Some(ty) => printer.ty(ty),
                        None => printer.atom("none"),
                    });
                    printer.field("value", |printer| printer.expression(value));
                    printer.field("else-block", |printer| match else_block {
                        Some(block) => printer.block(block),
                        None => printer.atom("none"),
                    });
                }),
                AstStatementKind::For {
                    pattern,
                    iterator,
                    body,
                    semicolon,
                } => printer.nested_form("for", |printer| {
                    printer.field("pattern", |printer| printer.pattern(pattern));
                    printer.field("iterator", |printer| printer.expression(iterator));
                    printer.field("body", |printer| printer.block(body));
                    printer.field("semicolon", |printer| printer.boolean(*semicolon));
                }),
                AstStatementKind::Assignment {
                    place,
                    operator,
                    value,
                } => printer.nested_form("assignment", |printer| {
                    printer.field("place", |printer| printer.expression(place));
                    printer.field("operator", |printer| {
                        printer.atom(match operator {
                            AstAssignmentOperator::Assign => "assign",
                            AstAssignmentOperator::AddAssign => "add-assign",
                        });
                    });
                    printer.field("value", |printer| printer.expression(value));
                }),
                AstStatementKind::Expression {
                    expression,
                    semicolon,
                } => printer.nested_form("expression", |printer| {
                    printer.field("value", |printer| printer.expression(expression));
                    printer.field("semicolon", |printer| printer.boolean(*semicolon));
                }),
            });
            printer.field("span", |printer| printer.span(statement.span));
        });
    }

    fn expression(&mut self, expression: &AstExpression) {
        self.nested_form("expression", |printer| {
            printer.field("kind", |printer| {
                printer.expression_kind(&expression.kind);
            });
            printer.field("span", |printer| printer.span(expression.span));
        });
    }

    fn expression_kind(&mut self, kind: &AstExpressionKind) {
        match kind {
            AstExpressionKind::Literal(literal) => {
                self.nested_form("literal", |printer| {
                    printer.field("value", |printer| printer.literal(literal));
                });
            }
            AstExpressionKind::Path(path) => self.nested_form("path", |printer| {
                printer.field("value", |printer| printer.path(path));
            }),
            AstExpressionKind::SelfValue => self.nested_form("self-value", |_| {}),
            AstExpressionKind::Unit => self.nested_form("unit", |_| {}),
            AstExpressionKind::Group(expression) => {
                self.nested_form("group", |printer| {
                    printer.field("expression", |printer| printer.expression(expression));
                });
            }
            AstExpressionKind::Tuple(elements) => self.nested_form("tuple", |printer| {
                printer.field("elements", |printer| {
                    for element in elements {
                        printer.expression(element);
                    }
                });
            }),
            AstExpressionKind::Array(elements) => self.nested_form("array", |printer| {
                printer.field("elements", |printer| {
                    for element in elements {
                        printer.expression(element);
                    }
                });
            }),
            AstExpressionKind::ArrayRepeat { value, count } => {
                self.nested_form("array-repeat", |printer| {
                    printer.field("value", |printer| printer.expression(value));
                    printer.field("count", |printer| printer.const_expression(count));
                });
            }
            AstExpressionKind::Record {
                constructor,
                fields,
            } => self.nested_form("record", |printer| {
                printer.field("constructor", |printer| printer.path(constructor));
                printer.field("fields", |printer| {
                    for field in fields {
                        printer.record_expression_field(field);
                    }
                });
            }),
            AstExpressionKind::Block(block) => self.nested_form("block", |printer| {
                printer.field("value", |printer| printer.block(block));
            }),
            AstExpressionKind::If(expression) => self.nested_form("if", |printer| {
                printer.field("value", |printer| printer.if_expression(expression));
            }),
            AstExpressionKind::While(expression) => self.nested_form("while", |printer| {
                printer.field("value", |printer| printer.while_expression(expression));
            }),
            AstExpressionKind::Loop(block) => self.nested_form("loop", |printer| {
                printer.field("body", |printer| printer.block(block));
            }),
            AstExpressionKind::Match { operand, arms } => {
                self.nested_form("match", |printer| {
                    printer.field("operand", |printer| printer.expression(operand));
                    printer.field("arms", |printer| {
                        for arm in arms {
                            printer.match_arm(arm);
                        }
                    });
                });
            }
            AstExpressionKind::Catch { operand, arms } => {
                self.nested_form("catch", |printer| {
                    printer.field("operand", |printer| printer.expression(operand));
                    printer.field("arms", |printer| {
                        for arm in arms {
                            printer.match_arm(arm);
                        }
                    });
                });
            }
            AstExpressionKind::Unsafe(block) => self.nested_form("unsafe", |printer| {
                printer.field("body", |printer| printer.block(block));
            }),
            AstExpressionKind::Closure(closure) => {
                self.nested_form("closure", |printer| {
                    printer.field("value", |printer| printer.closure(closure));
                });
            }
            AstExpressionKind::GeneratorClosure(closure) => {
                self.nested_form("generator-closure", |printer| {
                    printer.field("value", |printer| printer.generator_closure(closure));
                });
            }
            AstExpressionKind::Return(value) => self.nested_form("return", |printer| {
                printer.optional_expression("value", value.as_deref());
            }),
            AstExpressionKind::Break(value) => self.nested_form("break", |printer| {
                printer.optional_expression("value", value.as_deref());
            }),
            AstExpressionKind::Continue => self.nested_form("continue", |_| {}),
            AstExpressionKind::Throw(value) => self.nested_form("throw", |printer| {
                printer.optional_expression("value", value.as_deref());
            }),
            AstExpressionKind::Yield(value) => self.nested_form("yield", |printer| {
                printer.field("value", |printer| printer.expression(value));
            }),
            AstExpressionKind::Unary { operator, operand } => {
                self.nested_form("unary", |printer| {
                    printer.field("operator", |printer| {
                        printer.atom(unary_operator_name(*operator));
                    });
                    printer.field("operand", |printer| printer.expression(operand));
                });
            }
            AstExpressionKind::Binary {
                operator,
                left,
                right,
            } => self.nested_form("binary", |printer| {
                printer.field("operator", |printer| {
                    printer.atom(binary_operator_name(*operator));
                });
                printer.field("left", |printer| printer.expression(left));
                printer.field("right", |printer| printer.expression(right));
            }),
            AstExpressionKind::Cast { value, ty } => self.nested_form("cast", |printer| {
                printer.field("value", |printer| printer.expression(value));
                printer.field("type", |printer| printer.ty(ty));
            }),
            AstExpressionKind::Postfix { base, parts } => {
                self.nested_form("postfix", |printer| {
                    printer.field("base", |printer| printer.expression(base));
                    printer.field("parts", |printer| {
                        for part in parts {
                            printer.postfix(part);
                        }
                    });
                });
            }
        }
    }

    fn optional_expression(&mut self, name: &str, value: Option<&AstExpression>) {
        self.field(name, |printer| match value {
            Some(value) => printer.expression(value),
            None => printer.atom("none"),
        });
    }

    fn record_expression_field(&mut self, field: &AstRecordExpressionField) {
        self.nested_form("record-expression-field", |printer| {
            printer.field("name", |printer| {
                printer.string_value(field.name.as_str());
            });
            printer.field("value", |printer| printer.expression(&field.value));
            printer.field("span", |printer| printer.span(field.span));
        });
    }

    fn if_expression(&mut self, expression: &AstIfExpression) {
        self.nested_form("if-expression", |printer| {
            printer.field("condition", |printer| {
                printer.condition(&expression.condition);
            });
            printer.field("then-block", |printer| {
                printer.block(&expression.then_block);
            });
            printer.field("else-branch", |printer| match &expression.else_branch {
                Some(branch) => printer.else_branch(branch),
                None => printer.atom("none"),
            });
        });
    }

    fn while_expression(&mut self, expression: &AstWhileExpression) {
        self.nested_form("while-expression", |printer| {
            printer.field("condition", |printer| {
                printer.condition(&expression.condition);
            });
            printer.field("body", |printer| printer.block(&expression.body));
        });
    }

    fn condition(&mut self, condition: &AstCondition) {
        match condition {
            AstCondition::Expression(expression) => {
                self.nested_form("expression", |printer| {
                    printer.field("value", |printer| printer.expression(expression));
                });
            }
            AstCondition::Let { pattern, value } => self.nested_form("let", |printer| {
                printer.field("pattern", |printer| printer.pattern(pattern));
                printer.field("value", |printer| printer.expression(value));
            }),
        }
    }

    fn else_branch(&mut self, branch: &AstElseBranch) {
        match branch {
            AstElseBranch::Block(block) => self.nested_form("block", |printer| {
                printer.field("value", |printer| printer.block(block));
            }),
            AstElseBranch::If(expression) => self.nested_form("if", |printer| {
                printer.field("value", |printer| printer.expression(expression));
            }),
        }
    }

    fn match_arm(&mut self, arm: &AstMatchArm) {
        self.nested_form("match-arm", |printer| {
            printer.field("pattern", |printer| printer.pattern(&arm.pattern));
            printer.field("guard", |printer| match &arm.guard {
                Some(guard) => printer.expression(guard),
                None => printer.atom("none"),
            });
            printer.field("value", |printer| printer.expression(&arm.value));
            printer.field("span", |printer| printer.span(arm.span));
        });
    }

    fn closure(&mut self, closure: &AstClosure) {
        self.nested_form("closure", |printer| {
            printer.field("move", |printer| printer.boolean(closure.move_));
            printer.field("parameters", |printer| {
                for parameter in &closure.parameters {
                    printer.closure_parameter(parameter);
                }
            });
            printer.field("effects", |printer| printer.effect_sets(&closure.effects));
            printer.optional_type("result", closure.result.as_ref());
            printer.field("body", |printer| printer.expression(&closure.body));
        });
    }

    fn generator_closure(&mut self, closure: &AstGeneratorClosure) {
        self.nested_form("generator-closure", |printer| {
            printer.field("move", |printer| printer.boolean(closure.move_));
            printer.field("parameters", |printer| {
                for parameter in &closure.parameters {
                    printer.closure_parameter(parameter);
                }
            });
            printer.field("resume", |printer| printer.ty(&closure.resume));
            printer.field("yields", |printer| printer.ty(&closure.yields));
            printer.field("effects", |printer| printer.effect_sets(&closure.effects));
            printer.optional_type("result", closure.result.as_ref());
            printer.field("body", |printer| printer.expression(&closure.body));
        });
    }

    fn closure_parameter(&mut self, parameter: &AstClosureParameter) {
        self.nested_form("closure-parameter", |printer| {
            printer.field("pattern", |printer| printer.pattern(&parameter.pattern));
            printer.optional_type("type", parameter.ty.as_ref());
            printer.field("span", |printer| printer.span(parameter.span));
        });
    }

    fn postfix(&mut self, postfix: &AstPostfix) {
        self.nested_form("postfix", |printer| {
            printer.field("kind", |printer| match &postfix.kind {
                AstPostfixKind::Call(arguments) => {
                    printer.nested_form("call", |printer| {
                        printer.expression_list("arguments", arguments);
                    });
                }
                AstPostfixKind::Index(index) => {
                    printer.nested_form("index", |printer| {
                        printer.field("value", |printer| printer.expression(index));
                    });
                }
                AstPostfixKind::Method {
                    name,
                    generic_arguments,
                    arguments,
                } => printer.nested_form("method", |printer| {
                    printer.field("name", |printer| printer.method_name(name));
                    printer.optional_generic_arguments(
                        "generic-arguments",
                        generic_arguments.as_ref(),
                    );
                    printer.expression_list("arguments", arguments);
                }),
                AstPostfixKind::Field(name) => printer.nested_form("field", |printer| {
                    printer.field("name", |printer| printer.string_value(name.as_str()));
                }),
                AstPostfixKind::TupleField(index) => {
                    printer.nested_form("tuple-field", |printer| {
                        printer.field("index", |printer| printer.integer_literal(index));
                    });
                }
                AstPostfixKind::CommandSpawn(arguments) => {
                    printer.nested_form("command-spawn", |printer| {
                        printer.expression_list("arguments", arguments);
                    });
                }
                AstPostfixKind::Resume(value) => {
                    printer.nested_form("resume", |printer| {
                        printer.field("value", |printer| printer.expression(value));
                    });
                }
                AstPostfixKind::TurbofishCall {
                    generic_arguments,
                    arguments,
                } => printer.nested_form("turbofish-call", |printer| {
                    printer.field("generic-arguments", |printer| {
                        printer.generic_arguments(generic_arguments);
                    });
                    printer.expression_list("arguments", arguments);
                }),
            });
            printer.field("span", |printer| printer.span(postfix.span));
        });
    }

    fn expression_list(&mut self, name: &str, expressions: &[AstExpression]) {
        self.field(name, |printer| {
            for expression in expressions {
                printer.expression(expression);
            }
        });
    }

    fn pattern(&mut self, pattern: &AstPattern) {
        self.nested_form("pattern", |printer| {
            printer.field("kind", |printer| printer.pattern_kind(&pattern.kind));
            printer.field("span", |printer| printer.span(pattern.span));
        });
    }

    fn pattern_kind(&mut self, kind: &AstPatternKind) {
        match kind {
            AstPatternKind::Wildcard => self.nested_form("wildcard", |_| {}),
            AstPatternKind::Unit => self.nested_form("unit", |_| {}),
            AstPatternKind::Literal(literal) => self.nested_form("literal", |printer| {
                printer.field("value", |printer| printer.pattern_literal(literal));
            }),
            AstPatternKind::BarePathOrBinding(path) => {
                self.nested_form("bare-path-or-binding", |printer| {
                    printer.field("path", |printer| printer.path(path));
                });
            }
            AstPatternKind::Binding {
                name,
                mutable,
                by_reference,
                reference_mutable,
            } => self.nested_form("binding", |printer| {
                printer.field("name", |printer| printer.string_value(name.as_str()));
                printer.field("mutable", |printer| printer.boolean(*mutable));
                printer.field("by-reference", |printer| printer.boolean(*by_reference));
                printer.field("reference-mutable", |printer| {
                    printer.boolean(*reference_mutable);
                });
            }),
            AstPatternKind::Reference { mutable, pattern } => {
                self.nested_form("reference", |printer| {
                    printer.field("mutable", |printer| printer.boolean(*mutable));
                    printer.field("pattern", |printer| printer.pattern(pattern));
                });
            }
            AstPatternKind::Tuple(patterns) => self.nested_form("tuple", |printer| {
                printer.field("patterns", |printer| {
                    for pattern in patterns {
                        printer.pattern(pattern);
                    }
                });
            }),
            AstPatternKind::Slice(parts) => self.nested_form("slice", |printer| {
                printer.field("parts", |printer| {
                    for part in parts {
                        printer.slice_pattern_part(part);
                    }
                });
            }),
            AstPatternKind::Constructor { path, payload } => {
                self.nested_form("constructor", |printer| {
                    printer.field("path", |printer| printer.path(path));
                    printer.field("payload", |printer| {
                        printer.constructor_pattern_payload(payload);
                    });
                });
            }
            AstPatternKind::Range {
                inclusive,
                start,
                end,
            } => self.nested_form("range", |printer| {
                printer.field("inclusive", |printer| printer.boolean(*inclusive));
                printer.field("start", |printer| printer.range_endpoint(start));
                printer.field("end", |printer| printer.range_endpoint(end));
            }),
            AstPatternKind::At { binding, pattern } => self.nested_form("at", |printer| {
                printer.field("binding", |printer| printer.pattern(binding));
                printer.field("pattern", |printer| printer.pattern(pattern));
            }),
            AstPatternKind::Or(patterns) => self.nested_form("or", |printer| {
                printer.field("patterns", |printer| {
                    for pattern in patterns {
                        printer.pattern(pattern);
                    }
                });
            }),
        }
    }

    fn pattern_literal(&mut self, literal: &AstPatternLiteral) {
        match literal {
            AstPatternLiteral::Integer { negative, literal } => {
                self.nested_form("integer", |printer| {
                    printer.field("negative", |printer| printer.boolean(*negative));
                    printer.field("literal", |printer| printer.integer_literal(literal));
                });
            }
            AstPatternLiteral::Character(value) => {
                self.nested_form("character", |printer| {
                    printer.field("value", |printer| printer.character(*value));
                });
            }
            AstPatternLiteral::String(value) => self.nested_form("string", |printer| {
                printer.field("value", |printer| printer.string_value(value));
            }),
            AstPatternLiteral::Boolean(value) => self.nested_form("boolean", |printer| {
                printer.field("value", |printer| printer.boolean(*value));
            }),
        }
    }

    fn slice_pattern_part(&mut self, part: &AstSlicePatternPart) {
        match part {
            AstSlicePatternPart::Pattern(pattern) => {
                self.nested_form("pattern", |printer| {
                    printer.field("value", |printer| printer.pattern(pattern));
                });
            }
            AstSlicePatternPart::Rest(span) => self.nested_form("rest", |printer| {
                printer.field("span", |printer| printer.span(*span));
            }),
        }
    }

    fn constructor_pattern_payload(&mut self, payload: &AstConstructorPatternPayload) {
        match payload {
            AstConstructorPatternPayload::Unit => self.nested_form("unit", |_| {}),
            AstConstructorPatternPayload::Tuple(patterns) => {
                self.nested_form("tuple", |printer| {
                    printer.field("patterns", |printer| {
                        for pattern in patterns {
                            printer.pattern(pattern);
                        }
                    });
                });
            }
            AstConstructorPatternPayload::Record(fields) => {
                self.nested_form("record", |printer| {
                    printer.field("fields", |printer| {
                        for field in fields {
                            printer.record_pattern_field(field);
                        }
                    });
                });
            }
        }
    }

    fn record_pattern_field(&mut self, field: &AstRecordPatternField) {
        self.nested_form("record-pattern-field", |printer| {
            printer.field("name", |printer| {
                printer.string_value(field.name.as_str());
            });
            printer.field("pattern", |printer| printer.pattern(&field.pattern));
            printer.field("span", |printer| printer.span(field.span));
        });
    }

    fn range_endpoint(&mut self, endpoint: &AstRangeEndpoint) {
        match endpoint {
            AstRangeEndpoint::Integer {
                negative,
                literal,
                span,
            } => self.nested_form("integer", |printer| {
                printer.field("negative", |printer| printer.boolean(*negative));
                printer.field("literal", |printer| printer.integer_literal(literal));
                printer.field("span", |printer| printer.span(*span));
            }),
            AstRangeEndpoint::Character { value, span } => {
                self.nested_form("character", |printer| {
                    printer.field("value", |printer| printer.character(*value));
                    printer.field("span", |printer| printer.span(*span));
                });
            }
            AstRangeEndpoint::Const(path) => self.nested_form("const", |printer| {
                printer.field("path", |printer| printer.path(path));
            }),
        }
    }

    fn const_expression(&mut self, expression: &AstConstExpression) {
        self.nested_form("const-expression", |printer| {
            printer.field("kind", |printer| match &expression.kind {
                AstConstExpressionKind::Integer(literal) => {
                    printer.nested_form("integer", |printer| {
                        printer.field("literal", |printer| {
                            printer.integer_literal(literal);
                        });
                    });
                }
                AstConstExpressionKind::Path(path) => {
                    printer.nested_form("path", |printer| {
                        printer.field("value", |printer| printer.path(path));
                    });
                }
                AstConstExpressionKind::Group(expression) => {
                    printer.nested_form("group", |printer| {
                        printer.field("expression", |printer| {
                            printer.const_expression(expression);
                        });
                    });
                }
                AstConstExpressionKind::Unary { operator, operand } => {
                    printer.nested_form("unary", |printer| {
                        printer.field("operator", |printer| {
                            printer.atom(match operator {
                                AstConstUnaryOperator::Negate => "negate",
                                AstConstUnaryOperator::BitNot => "bit-not",
                            });
                        });
                        printer.field("operand", |printer| {
                            printer.const_expression(operand);
                        });
                    });
                }
                AstConstExpressionKind::Binary {
                    operator,
                    left,
                    right,
                } => printer.nested_form("binary", |printer| {
                    printer.field("operator", |printer| {
                        printer.atom(const_binary_operator_name(*operator));
                    });
                    printer.field("left", |printer| printer.const_expression(left));
                    printer.field("right", |printer| printer.const_expression(right));
                }),
            });
            printer.field("span", |printer| printer.span(expression.span));
        });
    }

    fn literal(&mut self, literal: &AstLiteral) {
        match literal {
            AstLiteral::Integer(literal) => self.integer_literal(literal),
            AstLiteral::Float(literal) => self.float_literal(literal),
            AstLiteral::Character(value) => self.nested_form("character", |printer| {
                printer.field("value", |printer| printer.character(*value));
            }),
            AstLiteral::String(value) => self.nested_form("string", |printer| {
                printer.field("value", |printer| printer.string_value(value));
            }),
            AstLiteral::Boolean(value) => self.nested_form("boolean", |printer| {
                printer.field("value", |printer| printer.boolean(*value));
            }),
        }
    }

    fn integer_literal(&mut self, literal: &IntegerLiteral) {
        self.nested_form("integer", |printer| {
            printer.field("base", |printer| {
                printer.atom(numeric_base_name(literal.base))
            });
            printer.field("digits", |printer| printer.string_value(&literal.digits));
            printer.field("suffix", |printer| match literal.suffix {
                Some(suffix) => printer.atom(integer_suffix_name(suffix)),
                None => printer.atom("none"),
            });
            printer.field("raw", |printer| printer.string_value(&literal.raw));
        });
    }

    fn float_literal(&mut self, literal: &FloatLiteral) {
        self.nested_form("float", |printer| {
            printer.field("base", |printer| {
                printer.atom(numeric_base_name(literal.base))
            });
            printer.field("raw", |printer| printer.string_value(&literal.raw));
            printer.field("suffix", |printer| match literal.suffix {
                Some(suffix) => printer.atom(float_suffix_name(suffix)),
                None => printer.atom("none"),
            });
        });
    }

    fn character(&mut self, value: char) {
        let mut bytes = [0; 4];
        self.string_value(value.encode_utf8(&mut bytes));
    }

    fn span(&mut self, span: Span) {
        self.unsigned(span.file.0);
        self.unsigned(span.start.byte);
        self.unsigned(span.start.line);
        self.unsigned(span.start.column);
        self.unsigned(span.end.byte);
        self.unsigned(span.end.line);
        self.unsigned(span.end.column);
    }

    fn string(&mut self, value: &str) {
        self.output.push('"');
        for character in value.chars() {
            match character {
                '"' => self.output.push_str("\\\""),
                '\\' => self.output.push_str("\\\\"),
                '\u{0008}' => self.output.push_str("\\b"),
                '\u{000c}' => self.output.push_str("\\f"),
                '\n' => self.output.push_str("\\n"),
                '\r' => self.output.push_str("\\r"),
                '\t' => self.output.push_str("\\t"),
                control if control <= '\u{001f}' => {
                    let _ = write!(self.output, "\\u{:04X}", u32::from(control));
                }
                other => self.output.push(other),
            }
        }
        self.output.push('"');
    }
}

fn scalar_type_name(value: AstScalarType) -> &'static str {
    match value {
        AstScalarType::Integer(value) => integer_type_name(value),
        AstScalarType::F32 => "f32",
        AstScalarType::F64 => "f64",
        AstScalarType::Bool => "bool",
        AstScalarType::Char => "char",
        AstScalarType::Entity => "entity",
    }
}

fn integer_type_name(value: AstIntegerType) -> &'static str {
    match value {
        AstIntegerType::I8 => "i8",
        AstIntegerType::I16 => "i16",
        AstIntegerType::I32 => "i32",
        AstIntegerType::I64 => "i64",
        AstIntegerType::Isize => "isize",
        AstIntegerType::U8 => "u8",
        AstIntegerType::U16 => "u16",
        AstIntegerType::U32 => "u32",
        AstIntegerType::U64 => "u64",
        AstIntegerType::Usize => "usize",
    }
}

fn numeric_base_name(value: NumericBase) -> &'static str {
    match value {
        NumericBase::Binary => "binary",
        NumericBase::Octal => "octal",
        NumericBase::Decimal => "decimal",
        NumericBase::Hexadecimal => "hexadecimal",
    }
}

fn integer_suffix_name(value: IntegerSuffix) -> &'static str {
    match value {
        IntegerSuffix::I8 => "i8",
        IntegerSuffix::I16 => "i16",
        IntegerSuffix::I32 => "i32",
        IntegerSuffix::I64 => "i64",
        IntegerSuffix::Isize => "isize",
        IntegerSuffix::U8 => "u8",
        IntegerSuffix::U16 => "u16",
        IntegerSuffix::U32 => "u32",
        IntegerSuffix::U64 => "u64",
        IntegerSuffix::Usize => "usize",
    }
}

fn float_suffix_name(value: FloatSuffix) -> &'static str {
    match value {
        FloatSuffix::F32 => "f32",
        FloatSuffix::F64 => "f64",
    }
}

fn unary_operator_name(value: AstUnaryOperator) -> &'static str {
    match value {
        AstUnaryOperator::Negate => "negate",
        AstUnaryOperator::LogicalNot => "logical-not",
        AstUnaryOperator::BitNot => "bit-not",
        AstUnaryOperator::Dereference => "dereference",
        AstUnaryOperator::BorrowShared => "borrow-shared",
        AstUnaryOperator::BorrowMutable => "borrow-mutable",
    }
}

fn binary_operator_name(value: AstBinaryOperator) -> &'static str {
    match value {
        AstBinaryOperator::LogicalOr => "logical-or",
        AstBinaryOperator::LogicalAnd => "logical-and",
        AstBinaryOperator::BitOr => "bit-or",
        AstBinaryOperator::BitXor => "bit-xor",
        AstBinaryOperator::BitAnd => "bit-and",
        AstBinaryOperator::Equal => "equal",
        AstBinaryOperator::NotEqual => "not-equal",
        AstBinaryOperator::Less => "less",
        AstBinaryOperator::LessEqual => "less-equal",
        AstBinaryOperator::Greater => "greater",
        AstBinaryOperator::GreaterEqual => "greater-equal",
        AstBinaryOperator::ShiftLeft => "shift-left",
        AstBinaryOperator::ShiftRight => "shift-right",
        AstBinaryOperator::Add => "add",
        AstBinaryOperator::Subtract => "subtract",
        AstBinaryOperator::Multiply => "multiply",
        AstBinaryOperator::Divide => "divide",
        AstBinaryOperator::Remainder => "remainder",
    }
}

fn const_binary_operator_name(value: AstConstBinaryOperator) -> &'static str {
    match value {
        AstConstBinaryOperator::BitOr => "bit-or",
        AstConstBinaryOperator::BitXor => "bit-xor",
        AstConstBinaryOperator::BitAnd => "bit-and",
        AstConstBinaryOperator::ShiftLeft => "shift-left",
        AstConstBinaryOperator::ShiftRight => "shift-right",
        AstConstBinaryOperator::Add => "add",
        AstConstBinaryOperator::Subtract => "subtract",
        AstConstBinaryOperator::Multiply => "multiply",
        AstConstBinaryOperator::Divide => "divide",
        AstConstBinaryOperator::Remainder => "remainder",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileId, SourcePosition};

    fn span() -> Span {
        Span {
            file: FileId(7),
            start: SourcePosition {
                byte: 11,
                line: 2,
                column: 3,
            },
            end: SourcePosition {
                byte: 19,
                line: 4,
                column: 5,
            },
        }
    }

    #[test]
    fn empty_file_uses_the_frozen_envelope_and_span_order() {
        let file = AstFile {
            items: Vec::new(),
            eof_span: span(),
        };

        assert_eq!(
            dump_ast(&file),
            "ARCHE-AST-TEXT 1\n(file (items) (eof-span 7 11 2 3 19 4 5))\n"
        );
    }

    #[test]
    fn strings_use_json_quoting_without_escaping_non_ascii() {
        let mut printer = AstPrinter::new();
        printer.string("\0\u{0008}\u{000c}\n\r\t\\\"é\u{001f}");

        assert_eq!(printer.output, "\"\\u0000\\b\\f\\n\\r\\t\\\\\\\"é\\u001F\"");
    }

    #[test]
    fn integer_literal_prints_every_retained_component() {
        let expression = AstNode::new(
            AstExpressionKind::Literal(AstLiteral::Integer(IntegerLiteral {
                base: NumericBase::Hexadecimal,
                digits: Arc::from("DEAD"),
                suffix: Some(IntegerSuffix::U64),
                raw: Arc::from("0xDE_ADu64"),
            })),
            span(),
        );
        let mut printer = AstPrinter::new();
        printer.expression(&expression);

        assert_eq!(
            printer.output,
            " (expression (kind (literal (value (integer (base hexadecimal) (digits \"DEAD\") (suffix u64) (raw \"0xDE_ADu64\"))))) (span 7 11 2 3 19 4 5))"
        );
    }
}
