//! Deterministic C2 expression typing over already-resolved leaves.
//!
//! Name, declaration, call, method, and trait-candidate lookup belong to their
//! dedicated C2 authorities.  This module accepts a small resolved expression
//! algebra that a body adapter can construct after those questions are
//! answered.  Inference variables never cross this module's checked boundary.

use std::{fmt, sync::Arc};

use arche_frontend::{
    lexer::{FloatLiteral, FloatSuffix, IntegerLiteral, IntegerSuffix},
    GenericArgumentShape, GenericParameterKind, IntegerType, Mutability, SymbolicConstExpression,
    SymbolicConstNode, SymbolicLifetime, SymbolicType,
};

use crate::{
    check_float_literal, check_integer_literal, classify_coercion, generic_argument_kind,
    select_sealed_primitive_operator, validate_body_symbolic_type, validate_generic_argument,
    validate_generic_arguments, BinderStack, BinderValidationError, CheckedCoercion,
    FloatLiteralError, FloatType, GenericFormationError, IntegerLiteralError, LifetimeOutlives,
    PrimitiveOperatorTrait, SealedPrimitiveOperator,
};

/// The four C2 diagnostic classes emitted by this type-engine boundary.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum TypeDiagnosticCode {
    InvalidFormation,
    TypeMismatch,
    InvalidLiteralOrPrimitive,
    UnsatisfiedTraitSelection,
}

impl TypeDiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidFormation => "TYPE001",
            Self::TypeMismatch => "TYPE002",
            Self::InvalidLiteralOrPrimitive => "TYPE003",
            Self::UnsatisfiedTraitSelection => "TRAIT002",
        }
    }
}

/// A precise, span-independent type-engine failure.  The body adapter attaches
/// source spans and deterministic diagnostic prose.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeCheckError {
    kind: Box<TypeCheckErrorKind>,
}

impl TypeCheckError {
    pub fn code(&self) -> TypeDiagnosticCode {
        match self.kind.as_ref() {
            TypeCheckErrorKind::Binder(_)
            | TypeCheckErrorKind::GenericFormation(_)
            | TypeCheckErrorKind::UnresolvedInferenceVariable
            | TypeCheckErrorKind::RecursiveInferenceType => TypeDiagnosticCode::InvalidFormation,
            TypeCheckErrorKind::Mismatch { .. }
            | TypeCheckErrorKind::ArrayExpressionTypeMismatch { .. }
            | TypeCheckErrorKind::ExpectedBoolean { .. }
            | TypeCheckErrorKind::BreakOutsideLoop
            | TypeCheckErrorKind::ReturnOutsideCallable => TypeDiagnosticCode::TypeMismatch,
            TypeCheckErrorKind::IntegerLiteral(_) | TypeCheckErrorKind::FloatLiteral(_) => {
                TypeDiagnosticCode::InvalidLiteralOrPrimitive
            }
            TypeCheckErrorKind::UnsatisfiedPrimitiveOperator { .. } => {
                TypeDiagnosticCode::UnsatisfiedTraitSelection
            }
        }
    }

    pub fn kind(&self) -> &TypeCheckErrorKind {
        self.kind.as_ref()
    }

    /// The diagnostic prose without the separately stored taxonomy code.
    ///
    /// [`fmt::Display`] remains useful as a self-contained error rendering,
    /// while semantic diagnostics use this view so their `code` and `message`
    /// fields do not redundantly encode the same prefix.
    pub fn message(&self) -> impl fmt::Display + '_ {
        TypeCheckErrorMessage(self)
    }

    fn new(kind: TypeCheckErrorKind) -> Self {
        Self {
            kind: Box::new(kind),
        }
    }
}

impl fmt::Display for TypeCheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: ", self.code().as_str())?;
        self.fmt_message(formatter)
    }
}

impl TypeCheckError {
    fn fmt_message(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind.as_ref() {
            TypeCheckErrorKind::Binder(error) => write!(formatter, "invalid bound type: {error:?}"),
            TypeCheckErrorKind::GenericFormation(error) => {
                write!(formatter, "invalid generic formation: {error:?}")
            }
            TypeCheckErrorKind::UnresolvedInferenceVariable => {
                formatter.write_str("type inference left an unresolved variable")
            }
            TypeCheckErrorKind::RecursiveInferenceType => {
                formatter.write_str("type inference attempted to construct an infinite type")
            }
            TypeCheckErrorKind::Mismatch { expected, actual } => {
                write!(formatter, "expected {expected:?}, found {actual:?}")
            }
            TypeCheckErrorKind::ArrayExpressionTypeMismatch { .. } => {
                formatter.write_str("array expression cannot satisfy the expected non-array type")
            }
            TypeCheckErrorKind::ExpectedBoolean { actual } => {
                write!(formatter, "expected bool, found {actual:?}")
            }
            TypeCheckErrorKind::BreakOutsideLoop => {
                formatter.write_str("break used outside a loop")
            }
            TypeCheckErrorKind::ReturnOutsideCallable => {
                formatter.write_str("return used outside a callable")
            }
            TypeCheckErrorKind::IntegerLiteral(error) => {
                write!(formatter, "invalid integer literal: {error:?}")
            }
            TypeCheckErrorKind::FloatLiteral(error) => {
                write!(formatter, "invalid float literal: {error:?}")
            }
            TypeCheckErrorKind::UnsatisfiedPrimitiveOperator {
                operator,
                left,
                right,
                result,
            } => write!(
                formatter,
                "no primitive {operator:?} selection for left={left:?}, right={right:?}, result={result:?}"
            ),
        }
    }
}

struct TypeCheckErrorMessage<'a>(&'a TypeCheckError);

impl fmt::Display for TypeCheckErrorMessage<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt_message(formatter)
    }
}

impl std::error::Error for TypeCheckError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeCheckErrorKind {
    Binder(BinderValidationError),
    GenericFormation(GenericFormationError),
    UnresolvedInferenceVariable,
    RecursiveInferenceType,
    Mismatch {
        expected: SymbolicType,
        actual: SymbolicType,
    },
    ArrayExpressionTypeMismatch {
        expected: SymbolicType,
    },
    ExpectedBoolean {
        actual: SymbolicType,
    },
    BreakOutsideLoop,
    ReturnOutsideCallable,
    IntegerLiteral(IntegerLiteralError),
    FloatLiteral(FloatLiteralError),
    UnsatisfiedPrimitiveOperator {
        operator: PrimitiveExpressionOperator,
        left: SymbolicType,
        right: Option<SymbolicType>,
        result: SymbolicType,
    },
}

/// The resolved environment needed by expression typing.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TypingContext {
    binders: BinderStack,
    outlives: LifetimeOutlives,
    return_type: Option<SymbolicType>,
}

impl TypingContext {
    pub fn new(
        binders: BinderStack,
        outlives: LifetimeOutlives,
        return_type: Option<SymbolicType>,
    ) -> Self {
        Self {
            binders,
            outlives,
            return_type,
        }
    }

    pub const fn binders(&self) -> &BinderStack {
        &self.binders
    }

    pub const fn outlives(&self) -> &LifetimeOutlives {
        &self.outlives
    }

    pub const fn return_type(&self) -> Option<&SymbolicType> {
        self.return_type.as_ref()
    }
}

/// A resolved expression algebra.  `Known` is the adapter seam for locals,
/// paths, calls, fields, indexing, records, closures, generators, and other
/// constructs whose declaration-specific questions are answered elsewhere.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypedExpressionInput {
    Known(SymbolicType),
    Integer(IntegerLiteral),
    Float(FloatLiteral),
    Character(char),
    String(Box<str>),
    Boolean(bool),
    Unit,
    Tuple(Vec<Self>),
    Array(Vec<Self>),
    ArrayRepeat {
        value: Box<Self>,
        length: SymbolicConstExpression,
    },
    Borrow {
        mutability: Mutability,
        value: Box<Self>,
    },
    Block {
        statements: Vec<Self>,
        tail: Option<Box<Self>>,
    },
    If {
        condition: Box<Self>,
        then_branch: Box<Self>,
        else_branch: Option<Box<Self>>,
    },
    While {
        condition: Box<Self>,
        body: Box<Self>,
    },
    Loop {
        body: Box<Self>,
    },
    Return(Option<Box<Self>>),
    Break(Option<Box<Self>>),
    Continue,
    Unary {
        operator: UnaryTypeOperator,
        operand: Box<Self>,
    },
    Binary {
        operator: BinaryTypeOperator,
        left: Box<Self>,
        right: Box<Self>,
    },
    Assignment {
        place_type: SymbolicType,
        value: Box<Self>,
    },
    AddAssignment {
        place_type: SymbolicType,
        value: Box<Self>,
    },
    Coerce {
        value: Box<Self>,
        target: SymbolicType,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum UnaryTypeOperator {
    Negate,
    LogicalNot,
    BitNot,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum BinaryTypeOperator {
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PrimitiveExpressionOperator {
    Unary(UnaryTypeOperator),
    Binary(BinaryTypeOperator),
    AddAssignment,
}

/// A fully substituted immutable expression fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedExpression {
    natural_type: SymbolicType,
    ty: SymbolicType,
    coercion: Option<CheckedCoercion>,
    kind: CheckedExpressionKind,
}

impl CheckedExpression {
    pub const fn natural_type(&self) -> &SymbolicType {
        &self.natural_type
    }

    pub const fn ty(&self) -> &SymbolicType {
        &self.ty
    }

    pub const fn coercion(&self) -> Option<CheckedCoercion> {
        self.coercion
    }

    pub const fn kind(&self) -> &CheckedExpressionKind {
        &self.kind
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckedExpressionKind {
    Known,
    IntegerLiteral {
        unary_negative: bool,
        little_endian_bits: Box<[u8]>,
    },
    FloatLiteral {
        unary_negative: bool,
        raw_bits: u64,
        little_endian_bits: Box<[u8]>,
    },
    Character(char),
    String(Box<str>),
    Boolean(bool),
    Unit,
    Tuple(Vec<CheckedExpression>),
    Array(Vec<CheckedExpression>),
    ArrayRepeat {
        value: Box<CheckedExpression>,
        length: SymbolicConstExpression,
    },
    Borrow {
        mutability: Mutability,
        lifetime: SymbolicLifetime,
        value: Box<CheckedExpression>,
    },
    Block {
        statements: Vec<CheckedExpression>,
        tail: Option<Box<CheckedExpression>>,
    },
    If {
        condition: Box<CheckedExpression>,
        then_branch: Box<CheckedExpression>,
        else_branch: Option<Box<CheckedExpression>>,
    },
    While {
        condition: Box<CheckedExpression>,
        body: Box<CheckedExpression>,
    },
    Loop {
        body: Box<CheckedExpression>,
        break_count: usize,
    },
    Return(Option<Box<CheckedExpression>>),
    Break(Option<Box<CheckedExpression>>),
    Continue,
    Unary {
        operator: UnaryTypeOperator,
        operand: Box<CheckedExpression>,
        selection: CheckedPrimitiveSelection,
    },
    Binary {
        operator: BinaryTypeOperator,
        left: Box<CheckedExpression>,
        right: Box<CheckedExpression>,
        selection: CheckedPrimitiveSelection,
    },
    Assignment {
        value: Box<CheckedExpression>,
    },
    AddAssignment {
        value: Box<CheckedExpression>,
        selection: SealedPrimitiveOperator,
    },
    Coerce {
        value: Box<CheckedExpression>,
        target: SymbolicType,
        coercion: CheckedCoercion,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckedPrimitiveSelection {
    Sealed(SealedPrimitiveOperator),
    BooleanLogical,
    FloatComparison(FloatType),
}

/// Checks one expression and refuses to construct a result until every dense
/// inference variable has a concrete `SymbolicType` substitution.
pub fn check_typed_expression(
    input: &TypedExpressionInput,
    expected: Option<&SymbolicType>,
    context: &TypingContext,
) -> Result<CheckedExpression, TypeCheckError> {
    if let Some(return_type) = context.return_type() {
        validate_type(return_type, context.binders())?;
    }
    if let Some(expected) = expected {
        validate_type(expected, context.binders())?;
    }

    let mut checker = TypeChecker::new(context);
    let expected = expected.cloned().map(InferType::Symbolic);
    let raw = checker.infer(input, expected.as_ref())?;
    checker.default_numeric_variables()?;
    checker.reject_unresolved_variables()?;
    checker.materialize(raw)
}

/// Uses the shared generic-formation and binder authorities at an adapter
/// boundary before a generic type/call use enters expression inference.
pub fn check_generic_instantiation(
    formals: &[GenericParameterKind],
    actuals: &[GenericArgumentShape],
    binders: &BinderStack,
) -> Result<(), TypeCheckError> {
    validate_generic_arguments(formals, actuals)
        .map_err(|error| TypeCheckError::new(TypeCheckErrorKind::GenericFormation(error)))?;
    for (expected, actual) in formals.iter().zip(actuals) {
        debug_assert_eq!(&generic_argument_kind(actual), expected);
        validate_generic_argument(actual, binders)
            .map_err(|error| TypeCheckError::new(TypeCheckErrorKind::Binder(error)))?;
    }
    Ok(())
}

fn validate_type(ty: &SymbolicType, binders: &BinderStack) -> Result<(), TypeCheckError> {
    validate_body_symbolic_type(ty, binders)
        .map_err(|error| TypeCheckError::new(TypeCheckErrorKind::Binder(error)))
}

#[derive(Clone, Debug)]
struct InferenceOwner;

#[derive(Clone, Debug)]
struct InferenceVariable {
    owner: Arc<InferenceOwner>,
    index: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VariableClass {
    Any,
    Integer,
    Float,
}

#[derive(Clone, Debug)]
struct VariableNode {
    parent: u32,
    class: VariableClass,
    binding: Option<InferType>,
}

#[derive(Clone, Debug)]
enum InferType {
    Variable(InferenceVariable),
    Symbolic(SymbolicType),
    Tuple(Vec<Self>),
    Array {
        element: Box<Self>,
        length: SymbolicConstExpression,
    },
    Reference {
        mutability: Mutability,
        lifetime: SymbolicLifetime,
        pointee: Box<Self>,
    },
}

#[derive(Clone, Debug)]
struct RawExpression {
    natural_type: InferType,
    coerced_type: Option<InferType>,
    kind: RawExpressionKind,
}

#[derive(Clone, Debug)]
enum RawExpressionKind {
    Known,
    IntegerLiteral {
        literal: IntegerLiteral,
        unary_negative: bool,
    },
    FloatLiteral {
        literal: FloatLiteral,
        unary_negative: bool,
    },
    Character(char),
    String(Box<str>),
    Boolean(bool),
    Unit,
    Tuple(Vec<RawExpression>),
    Array(Vec<RawExpression>),
    ArrayRepeat {
        value: Box<RawExpression>,
        length: SymbolicConstExpression,
    },
    Borrow {
        mutability: Mutability,
        lifetime: SymbolicLifetime,
        value: Box<RawExpression>,
    },
    Block {
        statements: Vec<RawExpression>,
        tail: Option<Box<RawExpression>>,
    },
    If {
        condition: Box<RawExpression>,
        then_branch: Box<RawExpression>,
        else_branch: Option<Box<RawExpression>>,
    },
    While {
        condition: Box<RawExpression>,
        body: Box<RawExpression>,
    },
    Loop {
        body: Box<RawExpression>,
        break_count: usize,
    },
    Return(Option<Box<RawExpression>>),
    Break(Option<Box<RawExpression>>),
    Continue,
    Unary {
        operator: UnaryTypeOperator,
        operand: Box<RawExpression>,
    },
    Binary {
        operator: BinaryTypeOperator,
        left: Box<RawExpression>,
        right: Box<RawExpression>,
    },
    Assignment {
        value: Box<RawExpression>,
    },
    AddAssignment {
        place_type: SymbolicType,
        value: Box<RawExpression>,
    },
    Coerce {
        value: Box<RawExpression>,
        target: SymbolicType,
    },
}

#[derive(Clone, Debug)]
struct LoopFrame {
    join_type: InferType,
    break_count: usize,
}

struct TypeChecker<'a> {
    context: &'a TypingContext,
    owner: Arc<InferenceOwner>,
    variables: Vec<VariableNode>,
    loops: Vec<LoopFrame>,
}

impl<'a> TypeChecker<'a> {
    fn new(context: &'a TypingContext) -> Self {
        Self {
            context,
            owner: Arc::new(InferenceOwner),
            variables: Vec::new(),
            loops: Vec::new(),
        }
    }

    fn infer(
        &mut self,
        input: &TypedExpressionInput,
        expected: Option<&InferType>,
    ) -> Result<RawExpression, TypeCheckError> {
        let mut raw = match input {
            TypedExpressionInput::Known(ty) => {
                validate_type(ty, self.context.binders())?;
                RawExpression {
                    natural_type: InferType::Symbolic(ty.clone()),
                    coerced_type: None,
                    kind: RawExpressionKind::Known,
                }
            }
            TypedExpressionInput::Integer(literal) => RawExpression {
                natural_type: literal.suffix.map(integer_suffix_type).map_or_else(
                    || self.variable(VariableClass::Integer),
                    |ty| Ok(InferType::Symbolic(ty)),
                )?,
                coerced_type: None,
                kind: RawExpressionKind::IntegerLiteral {
                    literal: literal.clone(),
                    unary_negative: false,
                },
            },
            TypedExpressionInput::Float(literal) => RawExpression {
                natural_type: literal.suffix.map(float_suffix_type).map_or_else(
                    || self.variable(VariableClass::Float),
                    |ty| Ok(InferType::Symbolic(ty)),
                )?,
                coerced_type: None,
                kind: RawExpressionKind::FloatLiteral {
                    literal: literal.clone(),
                    unary_negative: false,
                },
            },
            TypedExpressionInput::Character(value) => RawExpression {
                natural_type: InferType::Symbolic(SymbolicType::Char),
                coerced_type: None,
                kind: RawExpressionKind::Character(*value),
            },
            TypedExpressionInput::String(value) => RawExpression {
                natural_type: InferType::Symbolic(SymbolicType::Reference {
                    mutability: Mutability::Shared,
                    lifetime: SymbolicLifetime::Static,
                    pointee: Box::new(SymbolicType::Str),
                }),
                coerced_type: None,
                kind: RawExpressionKind::String(value.clone()),
            },
            TypedExpressionInput::Boolean(value) => RawExpression {
                natural_type: InferType::Symbolic(SymbolicType::Bool),
                coerced_type: None,
                kind: RawExpressionKind::Boolean(*value),
            },
            TypedExpressionInput::Unit => unit_raw(),
            TypedExpressionInput::Tuple(elements) => self.infer_tuple(elements, expected)?,
            TypedExpressionInput::Array(elements) => self.infer_array(elements, expected)?,
            TypedExpressionInput::ArrayRepeat { value, length } => {
                self.infer_array_repeat(value, length, expected)?
            }
            TypedExpressionInput::Borrow { mutability, value } => {
                self.infer_borrow(*mutability, value, expected)?
            }
            TypedExpressionInput::Block { statements, tail } => {
                self.infer_block(statements, tail.as_deref(), expected)?
            }
            TypedExpressionInput::If {
                condition,
                then_branch,
                else_branch,
            } => self.infer_if(condition, then_branch, else_branch.as_deref(), expected)?,
            TypedExpressionInput::While { condition, body } => {
                let condition = self.infer(condition, Some(&bool_type()))?;
                let body = self.infer(body, None)?;
                RawExpression {
                    natural_type: InferType::Symbolic(SymbolicType::Unit),
                    coerced_type: None,
                    kind: RawExpressionKind::While {
                        condition: Box::new(condition),
                        body: Box::new(body),
                    },
                }
            }
            TypedExpressionInput::Loop { body } => self.infer_loop(body)?,
            TypedExpressionInput::Return(value) => self.infer_return(value.as_deref())?,
            TypedExpressionInput::Break(value) => self.infer_break(value.as_deref())?,
            TypedExpressionInput::Continue => RawExpression {
                natural_type: InferType::Symbolic(SymbolicType::Never),
                coerced_type: None,
                kind: RawExpressionKind::Continue,
            },
            TypedExpressionInput::Unary { operator, operand } => {
                self.infer_unary(*operator, operand, expected)?
            }
            TypedExpressionInput::Binary {
                operator,
                left,
                right,
            } => self.infer_binary(*operator, left, right, expected)?,
            TypedExpressionInput::Assignment { place_type, value } => {
                validate_type(place_type, self.context.binders())?;
                let value = self.infer(value, Some(&InferType::Symbolic(place_type.clone())))?;
                RawExpression {
                    natural_type: InferType::Symbolic(SymbolicType::Unit),
                    coerced_type: None,
                    kind: RawExpressionKind::Assignment {
                        value: Box::new(value),
                    },
                }
            }
            TypedExpressionInput::AddAssignment { place_type, value } => {
                validate_type(place_type, self.context.binders())?;
                let place = InferType::Symbolic(place_type.clone());
                let mut value = self.infer(value, None)?;
                if self.contains_variable(value.result_type())? {
                    self.constrain_expected(&mut value, &place)?;
                }
                RawExpression {
                    natural_type: InferType::Symbolic(SymbolicType::Unit),
                    coerced_type: None,
                    kind: RawExpressionKind::AddAssignment {
                        place_type: place_type.clone(),
                        value: Box::new(value),
                    },
                }
            }
            TypedExpressionInput::Coerce { value, target } => {
                validate_type(target, self.context.binders())?;
                let value = self.infer(value, Some(&InferType::Symbolic(target.clone())))?;
                RawExpression {
                    natural_type: InferType::Symbolic(target.clone()),
                    coerced_type: None,
                    kind: RawExpressionKind::Coerce {
                        value: Box::new(value),
                        target: target.clone(),
                    },
                }
            }
        };

        if let Some(expected) = expected {
            self.constrain_expected(&mut raw, expected)?;
        }
        Ok(raw)
    }

    fn infer_tuple(
        &mut self,
        elements: &[TypedExpressionInput],
        expected: Option<&InferType>,
    ) -> Result<RawExpression, TypeCheckError> {
        let expected_elements = expected.and_then(tuple_elements);
        let mut checked_elements = Vec::with_capacity(elements.len());
        let mut element_types = Vec::with_capacity(elements.len());
        for (index, element) in elements.iter().enumerate() {
            let expected_element = expected_elements
                .as_deref()
                .and_then(|values| values.get(index));
            let checked = self.infer(element, expected_element)?;
            element_types.push(checked.result_type().clone());
            checked_elements.push(checked);
        }
        Ok(RawExpression {
            natural_type: InferType::Tuple(element_types),
            coerced_type: None,
            kind: RawExpressionKind::Tuple(checked_elements),
        })
    }

    fn infer_array(
        &mut self,
        elements: &[TypedExpressionInput],
        expected: Option<&InferType>,
    ) -> Result<RawExpression, TypeCheckError> {
        if let Some(expected) = expected {
            if array_element(expected).is_none() && !self.contains_variable(expected)? {
                return Err(TypeCheckError::new(
                    TypeCheckErrorKind::ArrayExpressionTypeMismatch {
                        expected: self.resolve_type(expected)?,
                    },
                ));
            }
        }
        let expected_element = expected.and_then(array_element);
        let element_type =
            expected_element.map_or_else(|| self.variable(VariableClass::Any), Ok)?;
        let mut checked_elements = Vec::with_capacity(elements.len());
        for element in elements {
            let checked = self.infer(element, Some(&element_type))?;
            self.unify(&checked.result_type().clone(), &element_type)?;
            checked_elements.push(checked);
        }
        Ok(RawExpression {
            natural_type: InferType::Array {
                element: Box::new(element_type),
                length: array_length(elements.len())?,
            },
            coerced_type: None,
            kind: RawExpressionKind::Array(checked_elements),
        })
    }

    fn infer_array_repeat(
        &mut self,
        value: &TypedExpressionInput,
        length: &SymbolicConstExpression,
        expected: Option<&InferType>,
    ) -> Result<RawExpression, TypeCheckError> {
        crate::validate_symbolic_const(length, self.context.binders())
            .map_err(|error| TypeCheckError::new(TypeCheckErrorKind::Binder(error)))?;
        let expected_element = expected.and_then(array_element);
        let value = self.infer(value, expected_element.as_ref())?;
        Ok(RawExpression {
            natural_type: InferType::Array {
                element: Box::new(value.result_type().clone()),
                length: length.clone(),
            },
            coerced_type: None,
            kind: RawExpressionKind::ArrayRepeat {
                value: Box::new(value),
                length: length.clone(),
            },
        })
    }

    fn infer_borrow(
        &mut self,
        mutability: Mutability,
        value: &TypedExpressionInput,
        expected: Option<&InferType>,
    ) -> Result<RawExpression, TypeCheckError> {
        // Source borrow expressions never carry a declaration lifetime. Their
        // region origin is retained by later RegionFact rows, while the C2
        // body-local type shape uses the frozen erased-local lifetime marker.
        let lifetime = SymbolicLifetime::ErasedLocal;
        let pointee_expected =
            expected.and_then(|expected| reference_pointee(expected, mutability));
        let value = self.infer(value, pointee_expected.as_ref())?;
        Ok(RawExpression {
            natural_type: InferType::Reference {
                mutability,
                lifetime: lifetime.clone(),
                pointee: Box::new(value.result_type().clone()),
            },
            coerced_type: None,
            kind: RawExpressionKind::Borrow {
                mutability,
                lifetime,
                value: Box::new(value),
            },
        })
    }

    fn infer_block(
        &mut self,
        statements: &[TypedExpressionInput],
        tail: Option<&TypedExpressionInput>,
        expected: Option<&InferType>,
    ) -> Result<RawExpression, TypeCheckError> {
        let statements = statements
            .iter()
            .map(|statement| self.infer(statement, None))
            .collect::<Result<Vec<_>, _>>()?;
        let tail = tail
            .map(|tail| self.infer(tail, expected).map(Box::new))
            .transpose()?;
        let natural_type = tail.as_deref().map_or_else(
            || InferType::Symbolic(SymbolicType::Unit),
            |tail| tail.result_type().clone(),
        );
        Ok(RawExpression {
            natural_type,
            coerced_type: None,
            kind: RawExpressionKind::Block { statements, tail },
        })
    }

    fn infer_if(
        &mut self,
        condition: &TypedExpressionInput,
        then_branch: &TypedExpressionInput,
        else_branch: Option<&TypedExpressionInput>,
        expected: Option<&InferType>,
    ) -> Result<RawExpression, TypeCheckError> {
        let condition = self.infer(condition, Some(&bool_type()))?;
        let branch_expected = if else_branch.is_none() {
            Some(InferType::Symbolic(SymbolicType::Unit))
        } else {
            expected.cloned()
        };
        let mut then_branch = self.infer(then_branch, branch_expected.as_ref())?;
        let mut else_branch = else_branch
            .map(|branch| self.infer(branch, branch_expected.as_ref()).map(Box::new))
            .transpose()?;

        let join_type = if let Some(expected) = branch_expected {
            expected
        } else {
            let right = else_branch
                .as_deref_mut()
                .expect("an if without else receives a unit expected type");
            self.join(&mut then_branch, right)?
        };
        Ok(RawExpression {
            natural_type: join_type,
            coerced_type: None,
            kind: RawExpressionKind::If {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch),
                else_branch,
            },
        })
    }

    fn infer_loop(&mut self, body: &TypedExpressionInput) -> Result<RawExpression, TypeCheckError> {
        let join_type = self.variable(VariableClass::Any)?;
        self.loops.push(LoopFrame {
            join_type: join_type.clone(),
            break_count: 0,
        });
        let body = self.infer(body, None)?;
        let frame = self.loops.pop().expect("loop frame was pushed");
        let natural_type = if frame.break_count == 0 {
            self.unify(&frame.join_type, &InferType::Symbolic(SymbolicType::Never))?;
            InferType::Symbolic(SymbolicType::Never)
        } else {
            frame.join_type
        };
        Ok(RawExpression {
            natural_type,
            coerced_type: None,
            kind: RawExpressionKind::Loop {
                body: Box::new(body),
                break_count: frame.break_count,
            },
        })
    }

    fn infer_return(
        &mut self,
        value: Option<&TypedExpressionInput>,
    ) -> Result<RawExpression, TypeCheckError> {
        let Some(return_type) = self.context.return_type() else {
            return Err(TypeCheckError::new(
                TypeCheckErrorKind::ReturnOutsideCallable,
            ));
        };
        let return_type = InferType::Symbolic(return_type.clone());
        let value = value
            .map(|value| self.infer(value, Some(&return_type)).map(Box::new))
            .transpose()?;
        if value.is_none() {
            let mut unit = unit_raw();
            self.constrain_expected(&mut unit, &return_type)?;
        }
        Ok(RawExpression {
            natural_type: InferType::Symbolic(SymbolicType::Never),
            coerced_type: None,
            kind: RawExpressionKind::Return(value),
        })
    }

    fn infer_break(
        &mut self,
        value: Option<&TypedExpressionInput>,
    ) -> Result<RawExpression, TypeCheckError> {
        let Some(frame) = self.loops.last() else {
            return Err(TypeCheckError::new(TypeCheckErrorKind::BreakOutsideLoop));
        };
        let join_type = frame.join_type.clone();
        let value = match value {
            Some(value) => Some(Box::new(self.infer(value, Some(&join_type))?)),
            None => {
                let mut unit = unit_raw();
                self.constrain_expected(&mut unit, &join_type)?;
                None
            }
        };
        self.loops
            .last_mut()
            .expect("loop frame remains active")
            .break_count += 1;
        Ok(RawExpression {
            natural_type: InferType::Symbolic(SymbolicType::Never),
            coerced_type: None,
            kind: RawExpressionKind::Break(value),
        })
    }

    fn infer_unary(
        &mut self,
        operator: UnaryTypeOperator,
        operand: &TypedExpressionInput,
        expected: Option<&InferType>,
    ) -> Result<RawExpression, TypeCheckError> {
        if operator == UnaryTypeOperator::Negate {
            match operand {
                TypedExpressionInput::Integer(literal) => {
                    let natural_type = literal.suffix.map(integer_suffix_type).map_or_else(
                        || self.variable(VariableClass::Integer),
                        |ty| Ok(InferType::Symbolic(ty)),
                    )?;
                    let mut raw = RawExpression {
                        natural_type,
                        coerced_type: None,
                        kind: RawExpressionKind::IntegerLiteral {
                            literal: literal.clone(),
                            unary_negative: true,
                        },
                    };
                    if let Some(expected) = expected {
                        self.constrain_expected(&mut raw, expected)?;
                    }
                    return Ok(raw);
                }
                TypedExpressionInput::Float(literal) => {
                    let natural_type = literal.suffix.map(float_suffix_type).map_or_else(
                        || self.variable(VariableClass::Float),
                        |ty| Ok(InferType::Symbolic(ty)),
                    )?;
                    let mut raw = RawExpression {
                        natural_type,
                        coerced_type: None,
                        kind: RawExpressionKind::FloatLiteral {
                            literal: literal.clone(),
                            unary_negative: true,
                        },
                    };
                    if let Some(expected) = expected {
                        self.constrain_expected(&mut raw, expected)?;
                    }
                    return Ok(raw);
                }
                _ => {}
            }
        }

        let operand = self.infer(operand, expected)?;
        let natural_type = operand.result_type().clone();
        Ok(RawExpression {
            natural_type,
            coerced_type: None,
            kind: RawExpressionKind::Unary {
                operator,
                operand: Box::new(operand),
            },
        })
    }

    fn infer_binary(
        &mut self,
        operator: BinaryTypeOperator,
        left: &TypedExpressionInput,
        right: &TypedExpressionInput,
        expected: Option<&InferType>,
    ) -> Result<RawExpression, TypeCheckError> {
        if matches!(
            operator,
            BinaryTypeOperator::LogicalAnd | BinaryTypeOperator::LogicalOr
        ) {
            let left = self.infer(left, Some(&bool_type()))?;
            let right = self.infer(right, Some(&bool_type()))?;
            return Ok(RawExpression {
                natural_type: InferType::Symbolic(SymbolicType::Bool),
                coerced_type: None,
                kind: RawExpressionKind::Binary {
                    operator,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            });
        }

        let comparison = is_comparison(operator);
        let left = self.infer(left, None)?;
        let right = self.infer(right, None)?;
        self.unify_operator_types(left.result_type(), right.result_type())?;
        let natural_type = if comparison {
            InferType::Symbolic(SymbolicType::Bool)
        } else {
            let result = expected
                .cloned()
                .map_or_else(|| self.variable(VariableClass::Any), Ok)?;
            self.unify_operator_types(&result, left.result_type())?;
            self.unify_operator_types(&result, right.result_type())?;
            result
        };
        Ok(RawExpression {
            natural_type,
            coerced_type: None,
            kind: RawExpressionKind::Binary {
                operator,
                left: Box::new(left),
                right: Box::new(right),
            },
        })
    }

    fn constrain_expected(
        &mut self,
        expression: &mut RawExpression,
        expected: &InferType,
    ) -> Result<(), TypeCheckError> {
        let natural = expression.natural_type.clone();
        if self.contains_variable(&natural)? || self.contains_variable(expected)? {
            self.unify(&natural, expected)?;
            expression.coerced_type = Some(expected.clone());
            return Ok(());
        }
        let source = self.resolve_type(&natural)?;
        let target = self.resolve_type(expected)?;
        if classify_coercion(&source, &target, self.context.outlives()).is_none() {
            match &expression.kind {
                RawExpressionKind::IntegerLiteral {
                    literal,
                    unary_negative,
                } => {
                    if let Some(contextual_type) = as_integer_type(&target) {
                        check_integer_literal(literal, Some(contextual_type), *unary_negative)
                            .map_err(|error| {
                                TypeCheckError::new(TypeCheckErrorKind::IntegerLiteral(error))
                            })?;
                    }
                }
                RawExpressionKind::FloatLiteral {
                    literal,
                    unary_negative,
                } => {
                    if let Some(contextual_type) = as_float_type(&target) {
                        check_float_literal(literal, Some(contextual_type), *unary_negative)
                            .map_err(|error| {
                                TypeCheckError::new(TypeCheckErrorKind::FloatLiteral(error))
                            })?;
                    }
                }
                _ => {}
            }
            return Err(mismatch(target, source));
        }
        expression.coerced_type = Some(expected.clone());
        Ok(())
    }

    fn join(
        &mut self,
        left: &mut RawExpression,
        right: &mut RawExpression,
    ) -> Result<InferType, TypeCheckError> {
        let left_type = left.result_type().clone();
        let right_type = right.result_type().clone();
        if is_never_term(&left_type) {
            left.coerced_type = Some(right_type.clone());
            return Ok(right_type);
        }
        if is_never_term(&right_type) {
            right.coerced_type = Some(left_type.clone());
            return Ok(left_type);
        }
        if self.contains_variable(&left_type)? || self.contains_variable(&right_type)? {
            self.unify(&left_type, &right_type)?;
            left.coerced_type = Some(left_type.clone());
            right.coerced_type = Some(left_type.clone());
            return Ok(left_type);
        }
        let left_resolved = self.resolve_type(&left_type)?;
        let right_resolved = self.resolve_type(&right_type)?;
        let left_to_right =
            classify_coercion(&left_resolved, &right_resolved, self.context.outlives());
        let right_to_left =
            classify_coercion(&right_resolved, &left_resolved, self.context.outlives());
        match (left_to_right, right_to_left) {
            (Some(_), Some(_)) if left_resolved <= right_resolved => {
                if left_resolved != right_resolved {
                    right.coerced_type = Some(InferType::Symbolic(left_resolved.clone()));
                }
                Ok(InferType::Symbolic(left_resolved))
            }
            (Some(_), Some(_)) | (Some(_), None) => {
                left.coerced_type = Some(InferType::Symbolic(right_resolved.clone()));
                Ok(InferType::Symbolic(right_resolved))
            }
            (None, Some(_)) => {
                right.coerced_type = Some(InferType::Symbolic(left_resolved.clone()));
                Ok(InferType::Symbolic(left_resolved))
            }
            (None, None) => Err(mismatch(left_resolved, right_resolved)),
        }
    }

    fn variable(&mut self, class: VariableClass) -> Result<InferType, TypeCheckError> {
        let index = u32::try_from(self.variables.len())
            .map_err(|_| TypeCheckError::new(TypeCheckErrorKind::UnresolvedInferenceVariable))?;
        self.variables.push(VariableNode {
            parent: index,
            class,
            binding: None,
        });
        Ok(InferType::Variable(InferenceVariable {
            owner: Arc::clone(&self.owner),
            index,
        }))
    }

    fn root(&mut self, variable: &InferenceVariable) -> Result<u32, TypeCheckError> {
        if !Arc::ptr_eq(&self.owner, &variable.owner) {
            return Err(TypeCheckError::new(
                TypeCheckErrorKind::UnresolvedInferenceVariable,
            ));
        }
        let index = usize::try_from(variable.index)
            .ok()
            .filter(|index| *index < self.variables.len())
            .ok_or_else(|| TypeCheckError::new(TypeCheckErrorKind::UnresolvedInferenceVariable))?;
        let parent = self.variables[index].parent;
        if parent == variable.index {
            return Ok(parent);
        }
        let parent_variable = InferenceVariable {
            owner: Arc::clone(&self.owner),
            index: parent,
        };
        let root = self.root(&parent_variable)?;
        self.variables[index].parent = root;
        Ok(root)
    }

    fn root_readonly(&self, variable: &InferenceVariable) -> Result<u32, TypeCheckError> {
        if !Arc::ptr_eq(&self.owner, &variable.owner) {
            return Err(TypeCheckError::new(
                TypeCheckErrorKind::UnresolvedInferenceVariable,
            ));
        }
        let mut index = variable.index;
        loop {
            let node = usize::try_from(index)
                .ok()
                .and_then(|index| self.variables.get(index))
                .ok_or_else(|| {
                    TypeCheckError::new(TypeCheckErrorKind::UnresolvedInferenceVariable)
                })?;
            if node.parent == index {
                return Ok(index);
            }
            index = node.parent;
        }
    }

    fn unify(&mut self, left: &InferType, right: &InferType) -> Result<(), TypeCheckError> {
        match (left, right) {
            (InferType::Variable(left), InferType::Variable(right)) => {
                self.unify_variables(left, right)
            }
            (InferType::Variable(variable), other) | (other, InferType::Variable(variable)) => {
                self.bind_variable(variable, other)
            }
            (InferType::Symbolic(left), InferType::Symbolic(right)) => {
                if exact_symbolic_type_match(left, right) {
                    Ok(())
                } else {
                    Err(mismatch(left.clone(), right.clone()))
                }
            }
            (InferType::Tuple(left), InferType::Tuple(right)) => self.unify_lists(left, right),
            (InferType::Tuple(left), InferType::Symbolic(SymbolicType::Tuple(right)))
            | (InferType::Symbolic(SymbolicType::Tuple(right)), InferType::Tuple(left)) => {
                if left.len() != right.len() {
                    return Err(mismatch(
                        SymbolicType::Tuple(self.resolve_types(left)?),
                        SymbolicType::Tuple(right.clone()),
                    ));
                }
                for (left, right) in left.iter().zip(right) {
                    self.unify(left, &InferType::Symbolic(right.clone()))?;
                }
                Ok(())
            }
            (
                InferType::Array {
                    element: left_element,
                    length: left_length,
                },
                InferType::Array {
                    element: right_element,
                    length: right_length,
                },
            ) => {
                if left_length != right_length {
                    return Err(mismatch(
                        self.resolve_type(left)?,
                        self.resolve_type(right)?,
                    ));
                }
                self.unify(left_element, right_element)
            }
            (
                InferType::Array { element, length },
                InferType::Symbolic(SymbolicType::Array {
                    element: expected_element,
                    length: expected_length,
                }),
            )
            | (
                InferType::Symbolic(SymbolicType::Array {
                    element: expected_element,
                    length: expected_length,
                }),
                InferType::Array { element, length },
            ) => {
                if length != expected_length {
                    return Err(mismatch(
                        self.resolve_type(left)?,
                        self.resolve_type(right)?,
                    ));
                }
                self.unify(element, &InferType::Symbolic((**expected_element).clone()))
            }
            (
                InferType::Reference {
                    mutability: left_mutability,
                    lifetime: left_lifetime,
                    pointee: left_pointee,
                },
                InferType::Reference {
                    mutability: right_mutability,
                    lifetime: right_lifetime,
                    pointee: right_pointee,
                },
            ) if left_mutability == right_mutability && left_lifetime == right_lifetime => {
                self.unify(left_pointee, right_pointee)
            }
            (
                InferType::Reference {
                    mutability,
                    lifetime,
                    pointee,
                },
                InferType::Symbolic(SymbolicType::Reference {
                    mutability: expected_mutability,
                    lifetime: expected_lifetime,
                    pointee: expected_pointee,
                }),
            )
            | (
                InferType::Symbolic(SymbolicType::Reference {
                    mutability: expected_mutability,
                    lifetime: expected_lifetime,
                    pointee: expected_pointee,
                }),
                InferType::Reference {
                    mutability,
                    lifetime,
                    pointee,
                },
            ) if mutability == expected_mutability && lifetime == expected_lifetime => {
                self.unify(pointee, &InferType::Symbolic((**expected_pointee).clone()))
            }
            _ => Err(mismatch(
                self.resolve_type(left)?,
                self.resolve_type(right)?,
            )),
        }
    }

    fn unify_lists(
        &mut self,
        left: &[InferType],
        right: &[InferType],
    ) -> Result<(), TypeCheckError> {
        if left.len() != right.len() {
            return Err(mismatch(
                SymbolicType::Tuple(self.resolve_types(left)?),
                SymbolicType::Tuple(self.resolve_types(right)?),
            ));
        }
        for (left, right) in left.iter().zip(right) {
            self.unify(left, right)?;
        }
        Ok(())
    }

    fn unify_operator_types(
        &mut self,
        left: &InferType,
        right: &InferType,
    ) -> Result<(), TypeCheckError> {
        if self.contains_variable(left)? || self.contains_variable(right)? {
            self.unify(left, right)?;
        }
        Ok(())
    }

    fn unify_variables(
        &mut self,
        left: &InferenceVariable,
        right: &InferenceVariable,
    ) -> Result<(), TypeCheckError> {
        let left_root = self.root(left)?;
        let right_root = self.root(right)?;
        if left_root == right_root {
            return Ok(());
        }
        let (root, child) = if left_root < right_root {
            (left_root, right_root)
        } else {
            (right_root, left_root)
        };
        let root_index = usize::try_from(root).expect("u32 fits usize");
        let child_index = usize::try_from(child).expect("u32 fits usize");
        let merged_class = merge_classes(
            self.variables[root_index].class,
            self.variables[child_index].class,
        )?;
        let root_binding = self.variables[root_index].binding.clone();
        let child_binding = self.variables[child_index].binding.clone();
        let root_reaches_child = root_binding
            .as_ref()
            .map(|binding| self.type_contains_root(binding, child))
            .transpose()?
            .unwrap_or(false);
        let child_reaches_root = child_binding
            .as_ref()
            .map(|binding| self.type_contains_root(binding, root))
            .transpose()?
            .unwrap_or(false);
        if root_reaches_child || child_reaches_root {
            return Err(TypeCheckError::new(
                TypeCheckErrorKind::RecursiveInferenceType,
            ));
        }
        self.variables[child_index].parent = root;
        self.variables[root_index].class = merged_class;
        self.variables[root_index].binding = root_binding.or_else(|| child_binding.clone());
        if let (Some(left), Some(right)) =
            (child_binding, self.variables[root_index].binding.clone())
        {
            self.unify(&left, &right)?;
        }
        Ok(())
    }

    fn bind_variable(
        &mut self,
        variable: &InferenceVariable,
        ty: &InferType,
    ) -> Result<(), TypeCheckError> {
        let root = self.root(variable)?;
        if self.type_contains_root(ty, root)? {
            return Err(TypeCheckError::new(
                TypeCheckErrorKind::RecursiveInferenceType,
            ));
        }
        let index = usize::try_from(root).expect("u32 fits usize");
        if let Some(binding) = self.variables[index].binding.clone() {
            return self.unify(&binding, ty);
        }
        if let Some(resolved) = self.try_resolve_type(ty)? {
            ensure_variable_class(self.variables[index].class, &resolved)?;
        }
        self.variables[index].binding = Some(ty.clone());
        Ok(())
    }

    fn type_contains_root(&self, ty: &InferType, target: u32) -> Result<bool, TypeCheckError> {
        Ok(match ty {
            InferType::Variable(variable) => self.root_readonly(variable)? == target,
            InferType::Tuple(elements) => elements
                .iter()
                .map(|element| self.type_contains_root(element, target))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .any(|contains| contains),
            InferType::Array { element, .. } => self.type_contains_root(element, target)?,
            InferType::Reference { pointee, .. } => self.type_contains_root(pointee, target)?,
            InferType::Symbolic(_) => false,
        })
    }

    fn contains_variable(&self, ty: &InferType) -> Result<bool, TypeCheckError> {
        Ok(match ty {
            InferType::Variable(variable) => {
                let root = self.root_readonly(variable)?;
                let node = &self.variables[usize::try_from(root).expect("u32 fits usize")];
                match &node.binding {
                    Some(binding) => self.contains_variable(binding)?,
                    None => true,
                }
            }
            InferType::Tuple(elements) => {
                let mut found = false;
                for element in elements {
                    found |= self.contains_variable(element)?;
                }
                found
            }
            InferType::Array { element, .. } => self.contains_variable(element)?,
            InferType::Reference { pointee, .. } => self.contains_variable(pointee)?,
            InferType::Symbolic(_) => false,
        })
    }

    fn try_resolve_type(&self, ty: &InferType) -> Result<Option<SymbolicType>, TypeCheckError> {
        if self.contains_variable(ty)? {
            Ok(None)
        } else {
            self.resolve_type(ty).map(Some)
        }
    }

    fn resolve_type(&self, ty: &InferType) -> Result<SymbolicType, TypeCheckError> {
        Ok(match ty {
            InferType::Variable(variable) => {
                let root = self.root_readonly(variable)?;
                let node = &self.variables[usize::try_from(root).expect("u32 fits usize")];
                let binding = node.binding.as_ref().ok_or_else(|| {
                    TypeCheckError::new(TypeCheckErrorKind::UnresolvedInferenceVariable)
                })?;
                self.resolve_type(binding)?
            }
            InferType::Symbolic(ty) => ty.clone(),
            InferType::Tuple(elements) => SymbolicType::Tuple(self.resolve_types(elements)?),
            InferType::Array { element, length } => SymbolicType::Array {
                element: Box::new(self.resolve_type(element)?),
                length: length.clone(),
            },
            InferType::Reference {
                mutability,
                lifetime,
                pointee,
            } => SymbolicType::Reference {
                mutability: *mutability,
                lifetime: lifetime.clone(),
                pointee: Box::new(self.resolve_type(pointee)?),
            },
        })
    }

    fn resolve_types(&self, types: &[InferType]) -> Result<Vec<SymbolicType>, TypeCheckError> {
        types.iter().map(|ty| self.resolve_type(ty)).collect()
    }

    fn default_numeric_variables(&mut self) -> Result<(), TypeCheckError> {
        for index in 0..self.variables.len() {
            let index = u32::try_from(index).expect("variable count fits u32");
            if self.variables[usize::try_from(index).expect("u32 fits usize")].parent != index {
                continue;
            }
            if self.variables[usize::try_from(index).expect("u32 fits usize")]
                .binding
                .is_some()
            {
                continue;
            }
            self.variables[usize::try_from(index).expect("u32 fits usize")].binding =
                match self.variables[usize::try_from(index).expect("u32 fits usize")].class {
                    VariableClass::Integer => Some(InferType::Symbolic(SymbolicType::I32)),
                    VariableClass::Float => Some(InferType::Symbolic(SymbolicType::F64)),
                    VariableClass::Any => None,
                };
        }
        for index in 0..self.variables.len() {
            let variable = InferenceVariable {
                owner: Arc::clone(&self.owner),
                index: u32::try_from(index).expect("variable count fits u32"),
            };
            let root = self.root(&variable)?;
            let node = &self.variables[usize::try_from(root).expect("u32 fits usize")];
            if let Some(binding) = &node.binding {
                if let Some(resolved) = self.try_resolve_type(binding)? {
                    ensure_variable_class(node.class, &resolved)?;
                }
            }
        }
        Ok(())
    }

    fn reject_unresolved_variables(&self) -> Result<(), TypeCheckError> {
        for (index, node) in self.variables.iter().enumerate() {
            if node.parent == u32::try_from(index).expect("variable count fits u32")
                && node.binding.is_none()
            {
                return Err(TypeCheckError::new(
                    TypeCheckErrorKind::UnresolvedInferenceVariable,
                ));
            }
        }
        Ok(())
    }

    fn materialize(&self, raw: RawExpression) -> Result<CheckedExpression, TypeCheckError> {
        let natural_type = self.resolve_type(&raw.natural_type)?;
        validate_type(&natural_type, self.context.binders())?;
        let ty = raw
            .coerced_type
            .as_ref()
            .map_or_else(|| Ok(natural_type.clone()), |ty| self.resolve_type(ty))?;
        validate_type(&ty, self.context.binders())?;
        let coercion = if natural_type == ty {
            None
        } else {
            classify_coercion(&natural_type, &ty, self.context.outlives())
                .ok_or_else(|| mismatch(ty.clone(), natural_type.clone()))?
                .into()
        };
        let kind = self.materialize_kind(raw.kind, &natural_type)?;
        Ok(CheckedExpression {
            natural_type,
            ty,
            coercion,
            kind,
        })
    }

    fn materialize_kind(
        &self,
        kind: RawExpressionKind,
        natural_type: &SymbolicType,
    ) -> Result<CheckedExpressionKind, TypeCheckError> {
        Ok(match kind {
            RawExpressionKind::Known => CheckedExpressionKind::Known,
            RawExpressionKind::IntegerLiteral {
                literal,
                unary_negative,
            } => {
                let integer_type = as_integer_type(natural_type)
                    .ok_or_else(|| mismatch(natural_type.clone(), SymbolicType::I32))?;
                let literal = check_integer_literal(&literal, Some(integer_type), unary_negative)
                    .map_err(|error| {
                    TypeCheckError::new(TypeCheckErrorKind::IntegerLiteral(error))
                })?;
                CheckedExpressionKind::IntegerLiteral {
                    unary_negative,
                    little_endian_bits: literal.little_endian_bits().into(),
                }
            }
            RawExpressionKind::FloatLiteral {
                literal,
                unary_negative,
            } => {
                let float_type = as_float_type(natural_type)
                    .ok_or_else(|| mismatch(natural_type.clone(), SymbolicType::F64))?;
                let literal = check_float_literal(&literal, Some(float_type), unary_negative)
                    .map_err(|error| {
                        TypeCheckError::new(TypeCheckErrorKind::FloatLiteral(error))
                    })?;
                let raw_bits = literal.raw_bits();
                let width = match float_type {
                    FloatType::F32 => 4,
                    FloatType::F64 => 8,
                };
                CheckedExpressionKind::FloatLiteral {
                    unary_negative,
                    raw_bits,
                    little_endian_bits: raw_bits.to_le_bytes()[..width].into(),
                }
            }
            RawExpressionKind::Character(value) => CheckedExpressionKind::Character(value),
            RawExpressionKind::String(value) => CheckedExpressionKind::String(value),
            RawExpressionKind::Boolean(value) => CheckedExpressionKind::Boolean(value),
            RawExpressionKind::Unit => CheckedExpressionKind::Unit,
            RawExpressionKind::Tuple(elements) => CheckedExpressionKind::Tuple(
                elements
                    .into_iter()
                    .map(|element| self.materialize(element))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            RawExpressionKind::Array(elements) => CheckedExpressionKind::Array(
                elements
                    .into_iter()
                    .map(|element| self.materialize(element))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            RawExpressionKind::ArrayRepeat { value, length } => {
                CheckedExpressionKind::ArrayRepeat {
                    value: Box::new(self.materialize(*value)?),
                    length,
                }
            }
            RawExpressionKind::Borrow {
                mutability,
                lifetime,
                value,
            } => CheckedExpressionKind::Borrow {
                mutability,
                lifetime,
                value: Box::new(self.materialize(*value)?),
            },
            RawExpressionKind::Block { statements, tail } => CheckedExpressionKind::Block {
                statements: statements
                    .into_iter()
                    .map(|statement| self.materialize(statement))
                    .collect::<Result<Vec<_>, _>>()?,
                tail: tail
                    .map(|tail| self.materialize(*tail).map(Box::new))
                    .transpose()?,
            },
            RawExpressionKind::If {
                condition,
                then_branch,
                else_branch,
            } => CheckedExpressionKind::If {
                condition: Box::new(self.materialize(*condition)?),
                then_branch: Box::new(self.materialize(*then_branch)?),
                else_branch: else_branch
                    .map(|branch| self.materialize(*branch).map(Box::new))
                    .transpose()?,
            },
            RawExpressionKind::While { condition, body } => CheckedExpressionKind::While {
                condition: Box::new(self.materialize(*condition)?),
                body: Box::new(self.materialize(*body)?),
            },
            RawExpressionKind::Loop { body, break_count } => CheckedExpressionKind::Loop {
                body: Box::new(self.materialize(*body)?),
                break_count,
            },
            RawExpressionKind::Return(value) => CheckedExpressionKind::Return(
                value
                    .map(|value| self.materialize(*value).map(Box::new))
                    .transpose()?,
            ),
            RawExpressionKind::Break(value) => CheckedExpressionKind::Break(
                value
                    .map(|value| self.materialize(*value).map(Box::new))
                    .transpose()?,
            ),
            RawExpressionKind::Continue => CheckedExpressionKind::Continue,
            RawExpressionKind::Unary { operator, operand } => {
                let operand = self.materialize(*operand)?;
                let selection = select_unary(operator, operand.ty())?;
                CheckedExpressionKind::Unary {
                    operator,
                    operand: Box::new(operand),
                    selection,
                }
            }
            RawExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                let left = self.materialize(*left)?;
                let right = self.materialize(*right)?;
                let selection = select_binary(operator, left.ty(), right.ty(), natural_type)?;
                CheckedExpressionKind::Binary {
                    operator,
                    left: Box::new(left),
                    right: Box::new(right),
                    selection,
                }
            }
            RawExpressionKind::Assignment { value } => CheckedExpressionKind::Assignment {
                value: Box::new(self.materialize(*value)?),
            },
            RawExpressionKind::AddAssignment { place_type, value } => {
                let value = self.materialize(*value)?;
                let selection = select_sealed(
                    PrimitiveExpressionOperator::AddAssignment,
                    PrimitiveOperatorTrait::Add,
                    &place_type,
                    value.ty(),
                    &place_type,
                )?;
                CheckedExpressionKind::AddAssignment {
                    value: Box::new(value),
                    selection,
                }
            }
            RawExpressionKind::Coerce { value, target } => {
                let value = self.materialize(*value)?;
                let coercion =
                    classify_coercion(value.natural_type(), &target, self.context.outlives())
                        .ok_or_else(|| mismatch(target.clone(), value.natural_type().clone()))?;
                CheckedExpressionKind::Coerce {
                    value: Box::new(value),
                    target,
                    coercion,
                }
            }
        })
    }
}

impl RawExpression {
    fn result_type(&self) -> &InferType {
        self.coerced_type.as_ref().unwrap_or(&self.natural_type)
    }
}

fn unit_raw() -> RawExpression {
    RawExpression {
        natural_type: InferType::Symbolic(SymbolicType::Unit),
        coerced_type: None,
        kind: RawExpressionKind::Unit,
    }
}

fn bool_type() -> InferType {
    InferType::Symbolic(SymbolicType::Bool)
}

fn tuple_elements(ty: &InferType) -> Option<Vec<InferType>> {
    match ty {
        InferType::Tuple(elements) => Some(elements.clone()),
        InferType::Symbolic(SymbolicType::Tuple(elements)) => {
            Some(elements.iter().cloned().map(InferType::Symbolic).collect())
        }
        _ => None,
    }
}

fn array_element(ty: &InferType) -> Option<InferType> {
    match ty {
        InferType::Array { element, .. } => Some((**element).clone()),
        InferType::Symbolic(SymbolicType::Array { element, .. }) => {
            Some(InferType::Symbolic((**element).clone()))
        }
        _ => None,
    }
}

fn reference_pointee(ty: &InferType, mutability: Mutability) -> Option<InferType> {
    match ty {
        InferType::Reference {
            mutability: expected,
            pointee,
            ..
        } if reference_mutability_can_coerce(mutability, *expected) => Some((**pointee).clone()),
        InferType::Symbolic(SymbolicType::Reference {
            mutability: expected,
            pointee,
            ..
        }) if reference_mutability_can_coerce(mutability, *expected) => {
            Some(InferType::Symbolic((**pointee).clone()))
        }
        _ => None,
    }
}

const fn reference_mutability_can_coerce(source: Mutability, target: Mutability) -> bool {
    matches!(
        (source, target),
        (Mutability::Shared, Mutability::Shared)
            | (
                Mutability::Mutable,
                Mutability::Mutable | Mutability::Shared
            )
    )
}

fn is_never_term(ty: &InferType) -> bool {
    matches!(ty, InferType::Symbolic(SymbolicType::Never))
}

fn array_length(length: usize) -> Result<SymbolicConstExpression, TypeCheckError> {
    let length = u64::try_from(length)
        .map_err(|_| TypeCheckError::new(TypeCheckErrorKind::UnresolvedInferenceVariable))?;
    Ok(SymbolicConstExpression {
        integer_type: IntegerType::Usize,
        node: SymbolicConstNode::IntegerLiteral(length.to_le_bytes().to_vec()),
    })
}

fn merge_classes(
    left: VariableClass,
    right: VariableClass,
) -> Result<VariableClass, TypeCheckError> {
    match (left, right) {
        (VariableClass::Any, class) | (class, VariableClass::Any) => Ok(class),
        (VariableClass::Integer, VariableClass::Integer) => Ok(VariableClass::Integer),
        (VariableClass::Float, VariableClass::Float) => Ok(VariableClass::Float),
        (VariableClass::Integer, VariableClass::Float)
        | (VariableClass::Float, VariableClass::Integer) => {
            Err(mismatch(SymbolicType::I32, SymbolicType::F64))
        }
    }
}

fn ensure_variable_class(class: VariableClass, ty: &SymbolicType) -> Result<(), TypeCheckError> {
    let matches = match class {
        VariableClass::Any => true,
        VariableClass::Integer => as_integer_type(ty).is_some(),
        VariableClass::Float => as_float_type(ty).is_some(),
    };
    if matches {
        Ok(())
    } else {
        Err(mismatch(
            match class {
                VariableClass::Any | VariableClass::Integer => SymbolicType::I32,
                VariableClass::Float => SymbolicType::F64,
            },
            ty.clone(),
        ))
    }
}

fn as_integer_type(ty: &SymbolicType) -> Option<IntegerType> {
    Some(match ty {
        SymbolicType::I8 => IntegerType::I8,
        SymbolicType::I16 => IntegerType::I16,
        SymbolicType::I32 => IntegerType::I32,
        SymbolicType::I64 => IntegerType::I64,
        SymbolicType::Isize => IntegerType::Isize,
        SymbolicType::U8 => IntegerType::U8,
        SymbolicType::U16 => IntegerType::U16,
        SymbolicType::U32 => IntegerType::U32,
        SymbolicType::U64 => IntegerType::U64,
        SymbolicType::Usize => IntegerType::Usize,
        _ => return None,
    })
}

const fn integer_suffix_type(suffix: IntegerSuffix) -> SymbolicType {
    match suffix {
        IntegerSuffix::I8 => SymbolicType::I8,
        IntegerSuffix::I16 => SymbolicType::I16,
        IntegerSuffix::I32 => SymbolicType::I32,
        IntegerSuffix::I64 => SymbolicType::I64,
        IntegerSuffix::Isize => SymbolicType::Isize,
        IntegerSuffix::U8 => SymbolicType::U8,
        IntegerSuffix::U16 => SymbolicType::U16,
        IntegerSuffix::U32 => SymbolicType::U32,
        IntegerSuffix::U64 => SymbolicType::U64,
        IntegerSuffix::Usize => SymbolicType::Usize,
    }
}

fn as_float_type(ty: &SymbolicType) -> Option<FloatType> {
    match ty {
        SymbolicType::F32 => Some(FloatType::F32),
        SymbolicType::F64 => Some(FloatType::F64),
        _ => None,
    }
}

const fn float_suffix_type(suffix: FloatSuffix) -> SymbolicType {
    match suffix {
        FloatSuffix::F32 => SymbolicType::F32,
        FloatSuffix::F64 => SymbolicType::F64,
    }
}

fn mismatch(expected: SymbolicType, actual: SymbolicType) -> TypeCheckError {
    TypeCheckError::new(TypeCheckErrorKind::Mismatch { expected, actual })
}

fn is_comparison(operator: BinaryTypeOperator) -> bool {
    matches!(
        operator,
        BinaryTypeOperator::Equal
            | BinaryTypeOperator::NotEqual
            | BinaryTypeOperator::Less
            | BinaryTypeOperator::LessEqual
            | BinaryTypeOperator::Greater
            | BinaryTypeOperator::GreaterEqual
    )
}

fn select_unary(
    operator: UnaryTypeOperator,
    operand: &SymbolicType,
) -> Result<CheckedPrimitiveSelection, TypeCheckError> {
    if operator == UnaryTypeOperator::LogicalNot && operand == &SymbolicType::Bool {
        return Ok(CheckedPrimitiveSelection::BooleanLogical);
    }
    let trait_kind = match operator {
        UnaryTypeOperator::Negate => PrimitiveOperatorTrait::Neg,
        UnaryTypeOperator::LogicalNot => PrimitiveOperatorTrait::LogicalNot,
        UnaryTypeOperator::BitNot => PrimitiveOperatorTrait::BitNot,
    };
    let arguments = vec![
        GenericArgumentShape::Type(operand.clone()),
        GenericArgumentShape::Type(operand.clone()),
    ];
    select_sealed_primitive_operator(trait_kind, operand, &arguments)
        .map(CheckedPrimitiveSelection::Sealed)
        .ok_or_else(|| {
            TypeCheckError::new(TypeCheckErrorKind::UnsatisfiedPrimitiveOperator {
                operator: PrimitiveExpressionOperator::Unary(operator),
                left: operand.clone(),
                right: None,
                result: operand.clone(),
            })
        })
}

fn select_binary(
    operator: BinaryTypeOperator,
    left: &SymbolicType,
    right: &SymbolicType,
    result: &SymbolicType,
) -> Result<CheckedPrimitiveSelection, TypeCheckError> {
    if matches!(
        operator,
        BinaryTypeOperator::LogicalAnd | BinaryTypeOperator::LogicalOr
    ) {
        if left == &SymbolicType::Bool && right == &SymbolicType::Bool {
            return Ok(CheckedPrimitiveSelection::BooleanLogical);
        }
        let actual = if left != &SymbolicType::Bool {
            left.clone()
        } else {
            right.clone()
        };
        return Err(TypeCheckError::new(TypeCheckErrorKind::ExpectedBoolean {
            actual,
        }));
    }
    if is_comparison(operator) && (as_float_type(left).is_some() || as_float_type(right).is_some())
    {
        return match (as_float_type(left), as_float_type(right)) {
            (Some(left_float), Some(right_float)) if left_float == right_float => {
                Ok(CheckedPrimitiveSelection::FloatComparison(left_float))
            }
            _ => Err(mismatch(left.clone(), right.clone())),
        };
    }
    let trait_kind = match operator {
        BinaryTypeOperator::BitOr => PrimitiveOperatorTrait::BitOr,
        BinaryTypeOperator::BitXor => PrimitiveOperatorTrait::BitXor,
        BinaryTypeOperator::BitAnd => PrimitiveOperatorTrait::BitAnd,
        BinaryTypeOperator::Equal | BinaryTypeOperator::NotEqual => PrimitiveOperatorTrait::Eq,
        BinaryTypeOperator::Less
        | BinaryTypeOperator::LessEqual
        | BinaryTypeOperator::Greater
        | BinaryTypeOperator::GreaterEqual => PrimitiveOperatorTrait::Ord,
        BinaryTypeOperator::ShiftLeft => PrimitiveOperatorTrait::ShiftLeft,
        BinaryTypeOperator::ShiftRight => PrimitiveOperatorTrait::ShiftRight,
        BinaryTypeOperator::Add => PrimitiveOperatorTrait::Add,
        BinaryTypeOperator::Subtract => PrimitiveOperatorTrait::Sub,
        BinaryTypeOperator::Multiply => PrimitiveOperatorTrait::Mul,
        BinaryTypeOperator::Divide => PrimitiveOperatorTrait::Div,
        BinaryTypeOperator::Remainder => PrimitiveOperatorTrait::Rem,
        BinaryTypeOperator::LogicalOr | BinaryTypeOperator::LogicalAnd => unreachable!(),
    };
    let output = if is_comparison(operator) {
        None
    } else {
        Some(result)
    };
    let arguments = match output {
        Some(output) => vec![
            GenericArgumentShape::Type(left.clone()),
            GenericArgumentShape::Type(right.clone()),
            GenericArgumentShape::Type(output.clone()),
        ],
        None => vec![
            GenericArgumentShape::Type(left.clone()),
            GenericArgumentShape::Type(right.clone()),
        ],
    };
    select_sealed_primitive_operator(trait_kind, left, &arguments)
        .map(CheckedPrimitiveSelection::Sealed)
        .ok_or_else(|| {
            TypeCheckError::new(TypeCheckErrorKind::UnsatisfiedPrimitiveOperator {
                operator: PrimitiveExpressionOperator::Binary(operator),
                left: left.clone(),
                right: Some(right.clone()),
                result: result.clone(),
            })
        })
}

fn select_sealed(
    operator: PrimitiveExpressionOperator,
    trait_kind: PrimitiveOperatorTrait,
    left: &SymbolicType,
    right: &SymbolicType,
    result: &SymbolicType,
) -> Result<SealedPrimitiveOperator, TypeCheckError> {
    let arguments = vec![
        GenericArgumentShape::Type(left.clone()),
        GenericArgumentShape::Type(right.clone()),
        GenericArgumentShape::Type(result.clone()),
    ];
    select_sealed_primitive_operator(trait_kind, left, &arguments).ok_or_else(|| {
        TypeCheckError::new(TypeCheckErrorKind::UnsatisfiedPrimitiveOperator {
            operator,
            left: left.clone(),
            right: Some(right.clone()),
            result: result.clone(),
        })
    })
}

/// Byte-exact structural equality.  In particular, generic argument variants,
/// binder coordinates, lifetime kinds, const integer types, and complete const
/// expression trees must match; there is no numeric or generic-kind coercion.
fn exact_symbolic_type_match(left: &SymbolicType, right: &SymbolicType) -> bool {
    match (left, right) {
        (
            SymbolicType::Array {
                element: left_element,
                length: left_length,
            },
            SymbolicType::Array {
                element: right_element,
                length: right_length,
            },
        ) => exact_symbolic_type_match(left_element, right_element) && left_length == right_length,
        (SymbolicType::Slice(left), SymbolicType::Slice(right)) => {
            exact_symbolic_type_match(left, right)
        }
        (SymbolicType::Tuple(left), SymbolicType::Tuple(right)) => {
            exact_type_lists_match(left, right)
        }
        (
            SymbolicType::Reference {
                mutability: left_mutability,
                lifetime: left_lifetime,
                pointee: left_pointee,
            },
            SymbolicType::Reference {
                mutability: right_mutability,
                lifetime: right_lifetime,
                pointee: right_pointee,
            },
        ) => {
            left_mutability == right_mutability
                && left_lifetime == right_lifetime
                && exact_symbolic_type_match(left_pointee, right_pointee)
        }
        (
            SymbolicType::RawPointer {
                mutability: left_mutability,
                pointee: left_pointee,
            },
            SymbolicType::RawPointer {
                mutability: right_mutability,
                pointee: right_pointee,
            },
        ) => {
            left_mutability == right_mutability
                && exact_symbolic_type_match(left_pointee, right_pointee)
        }
        (
            SymbolicType::NominalPath {
                declaration: left_declaration,
                arguments: left_arguments,
            },
            SymbolicType::NominalPath {
                declaration: right_declaration,
                arguments: right_arguments,
            },
        ) => {
            left_declaration == right_declaration
                && exact_argument_lists_match(left_arguments, right_arguments)
        }
        (
            SymbolicType::FunctionPointer {
                unsafe_: left_unsafe,
                parameters: left_parameters,
                result: left_result,
                requires: left_requires,
                throws: left_throws,
            },
            SymbolicType::FunctionPointer {
                unsafe_: right_unsafe,
                parameters: right_parameters,
                result: right_result,
                requires: right_requires,
                throws: right_throws,
            },
        ) => {
            left_unsafe == right_unsafe
                && exact_type_lists_match(left_parameters, right_parameters)
                && exact_symbolic_type_match(left_result, right_result)
                && left_requires == right_requires
                && left_throws == right_throws
        }
        _ => left == right,
    }
}

fn exact_type_lists_match(left: &[SymbolicType], right: &[SymbolicType]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| exact_symbolic_type_match(left, right))
}

fn exact_argument_lists_match(
    left: &[GenericArgumentShape],
    right: &[GenericArgumentShape],
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| match (left, right) {
                (GenericArgumentShape::Type(left), GenericArgumentShape::Type(right)) => {
                    exact_symbolic_type_match(left, right)
                }
                (GenericArgumentShape::Lifetime(left), GenericArgumentShape::Lifetime(right)) => {
                    left == right
                }
                (
                    GenericArgumentShape::IntegerConst(left),
                    GenericArgumentShape::IntegerConst(right),
                ) => left == right,
                _ => false,
            })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arche_frontend::{
        lexer::{FloatSuffix, IntegerSuffix, NumericBase},
        DeclarationKind, SemanticDeclarationPath, TargetRoot,
    };

    use crate::CoercionKind;

    use super::*;

    fn context() -> TypingContext {
        TypingContext::default()
    }

    fn integer(digits: &str, suffix: Option<IntegerSuffix>) -> TypedExpressionInput {
        TypedExpressionInput::Integer(IntegerLiteral {
            base: NumericBase::Decimal,
            digits: Arc::from(digits),
            suffix,
            raw: Arc::from(digits),
        })
    }

    fn float(raw: &str, suffix: Option<FloatSuffix>) -> TypedExpressionInput {
        let spelling = match suffix {
            Some(FloatSuffix::F32) => format!("{raw}f32"),
            Some(FloatSuffix::F64) => format!("{raw}f64"),
            None => raw.to_owned(),
        };
        TypedExpressionInput::Float(FloatLiteral {
            base: NumericBase::Decimal,
            raw: Arc::from(spelling),
            suffix,
        })
    }

    fn check(input: &TypedExpressionInput) -> Result<CheckedExpression, TypeCheckError> {
        check_typed_expression(input, None, &context())
    }

    fn const_usize(value: u64) -> SymbolicConstExpression {
        SymbolicConstExpression {
            integer_type: IntegerType::Usize,
            node: SymbolicConstNode::IntegerLiteral(value.to_le_bytes().to_vec()),
        }
    }

    #[test]
    fn defaults_unsuffixed_integer_and_float_exactly() {
        assert_eq!(check(&integer("7", None)).unwrap().ty(), &SymbolicType::I32);
        assert_eq!(check(&float("1.5", None)).unwrap().ty(), &SymbolicType::F64);
    }

    #[test]
    fn expected_types_propagate_to_every_scalar_literal_boundary() {
        let rows = [
            (SymbolicType::I8, integer("127", None)),
            (SymbolicType::I16, integer("32767", None)),
            (SymbolicType::I32, integer("2147483647", None)),
            (SymbolicType::I64, integer("9", None)),
            (SymbolicType::Isize, integer("9", None)),
            (SymbolicType::U8, integer("255", None)),
            (SymbolicType::U16, integer("65535", None)),
            (SymbolicType::U32, integer("9", None)),
            (SymbolicType::U64, integer("9", None)),
            (SymbolicType::Usize, integer("9", None)),
            (SymbolicType::F32, float("1.25", None)),
            (SymbolicType::F64, float("1.25", None)),
        ];
        for (expected, input) in rows {
            let checked = check_typed_expression(&input, Some(&expected), &context()).unwrap();
            assert_eq!(checked.ty(), &expected);
        }
        assert_eq!(
            check(&TypedExpressionInput::Boolean(false)).unwrap().ty(),
            &SymbolicType::Bool
        );
        assert_eq!(
            check(&TypedExpressionInput::Character('􏿿')).unwrap().ty(),
            &SymbolicType::Char
        );
        assert_eq!(
            check(&TypedExpressionInput::Unit).unwrap().ty(),
            &SymbolicType::Unit
        );
        assert_eq!(
            check(&TypedExpressionInput::String("hello".into()))
                .unwrap()
                .ty(),
            &SymbolicType::Reference {
                mutability: Mutability::Shared,
                lifetime: SymbolicLifetime::Static,
                pointee: Box::new(SymbolicType::Str),
            }
        );

        for scalar in [
            SymbolicType::I8,
            SymbolicType::I16,
            SymbolicType::I32,
            SymbolicType::I64,
            SymbolicType::Isize,
            SymbolicType::U8,
            SymbolicType::U16,
            SymbolicType::U32,
            SymbolicType::U64,
            SymbolicType::Usize,
            SymbolicType::F32,
            SymbolicType::F64,
            SymbolicType::Bool,
            SymbolicType::Char,
            SymbolicType::Entity,
            SymbolicType::Unit,
            SymbolicType::Never,
        ] {
            assert_eq!(
                check_typed_expression(
                    &TypedExpressionInput::Known(scalar.clone()),
                    Some(&scalar),
                    &context(),
                )
                .unwrap()
                .ty(),
                &scalar
            );
        }
    }

    #[test]
    fn unary_negative_checks_the_signed_minimum_after_negation() {
        let input = TypedExpressionInput::Unary {
            operator: UnaryTypeOperator::Negate,
            operand: Box::new(integer("128", None)),
        };
        let checked = check_typed_expression(&input, Some(&SymbolicType::I8), &context()).unwrap();
        assert_eq!(checked.ty(), &SymbolicType::I8);
        let CheckedExpressionKind::IntegerLiteral {
            unary_negative,
            little_endian_bits,
        } = checked.kind()
        else {
            panic!("negative literal must remain exact literal evidence");
        };
        assert!(*unary_negative);
        assert_eq!(little_endian_bits.as_ref(), &[0x80]);
    }

    #[test]
    fn literal_overflow_is_type003() {
        let error =
            check_typed_expression(&integer("256", None), Some(&SymbolicType::U8), &context())
                .unwrap_err();
        assert_eq!(error.code().as_str(), "TYPE003");
    }

    #[test]
    fn empty_array_requires_an_expected_element_type() {
        let error = check(&TypedExpressionInput::Array(Vec::new())).unwrap_err();
        assert_eq!(
            error.kind(),
            &TypeCheckErrorKind::UnresolvedInferenceVariable
        );
        assert_eq!(error.code().as_str(), "TYPE001");

        let expected = SymbolicType::Array {
            element: Box::new(SymbolicType::U16),
            length: const_usize(0),
        };
        assert_eq!(
            check_typed_expression(
                &TypedExpressionInput::Array(Vec::new()),
                Some(&expected),
                &context(),
            )
            .unwrap()
            .ty(),
            &expected
        );

        let mismatch = check_typed_expression(
            &TypedExpressionInput::Array(Vec::new()),
            Some(&SymbolicType::Bool),
            &context(),
        )
        .unwrap_err();
        assert_eq!(mismatch.code().as_str(), "TYPE002");
        assert_eq!(
            mismatch.kind(),
            &TypeCheckErrorKind::ArrayExpressionTypeMismatch {
                expected: SymbolicType::Bool,
            }
        );
        assert_eq!(
            mismatch.message().to_string(),
            "array expression cannot satisfy the expected non-array type"
        );
    }

    #[test]
    fn tuple_and_array_contexts_flow_to_nested_literals() {
        let expected_tuple = SymbolicType::Tuple(vec![SymbolicType::U8, SymbolicType::F32]);
        let tuple = TypedExpressionInput::Tuple(vec![integer("5", None), float("2.5", None)]);
        assert_eq!(
            check_typed_expression(&tuple, Some(&expected_tuple), &context())
                .unwrap()
                .ty(),
            &expected_tuple
        );

        let expected_array = SymbolicType::Array {
            element: Box::new(SymbolicType::I16),
            length: const_usize(2),
        };
        let array = TypedExpressionInput::Array(vec![integer("1", None), integer("2", None)]);
        assert_eq!(
            check_typed_expression(&array, Some(&expected_array), &context())
                .unwrap()
                .ty(),
            &expected_array
        );
    }

    #[test]
    fn array_inference_is_deterministic_when_element_order_is_reversed() {
        for elements in [
            vec![integer("1", None), integer("2", Some(IntegerSuffix::U8))],
            vec![integer("2", Some(IntegerSuffix::U8)), integer("1", None)],
        ] {
            let checked = check(&TypedExpressionInput::Array(elements)).unwrap();
            assert_eq!(
                checked.ty(),
                &SymbolicType::Array {
                    element: Box::new(SymbolicType::U8),
                    length: const_usize(2),
                }
            );
        }
    }

    #[test]
    fn borrow_constructs_an_exact_reference_and_can_reborrow_to_shared() {
        let input = TypedExpressionInput::Borrow {
            mutability: Mutability::Mutable,
            value: Box::new(integer("4", None)),
        };
        let expected = SymbolicType::Reference {
            mutability: Mutability::Shared,
            lifetime: SymbolicLifetime::ErasedLocal,
            pointee: Box::new(SymbolicType::I16),
        };
        let checked = check_typed_expression(&input, Some(&expected), &context()).unwrap();
        assert_eq!(checked.ty(), &expected);
        assert_eq!(
            checked.natural_type(),
            &SymbolicType::Reference {
                mutability: Mutability::Mutable,
                lifetime: SymbolicLifetime::ErasedLocal,
                pointee: Box::new(SymbolicType::I16),
            }
        );
        assert_eq!(
            checked.coercion().map(CheckedCoercion::kind),
            Some(CoercionKind::MutableReborrowToShared)
        );
    }

    #[test]
    fn if_join_and_never_coercion_are_order_independent() {
        let return_context = TypingContext::new(
            BinderStack::default(),
            LifetimeOutlives::default(),
            Some(SymbolicType::I32),
        );
        let diverges = TypedExpressionInput::Return(Some(Box::new(integer("1", None))));
        for (then_branch, else_branch) in [
            (integer("2", None), diverges.clone()),
            (diverges.clone(), integer("2", None)),
        ] {
            let input = TypedExpressionInput::If {
                condition: Box::new(TypedExpressionInput::Boolean(true)),
                then_branch: Box::new(then_branch),
                else_branch: Some(Box::new(else_branch)),
            };
            assert_eq!(
                check_typed_expression(&input, None, &return_context)
                    .unwrap()
                    .ty(),
                &SymbolicType::I32
            );
        }
    }

    #[test]
    fn loop_break_values_form_one_deterministic_join() {
        let input = TypedExpressionInput::Loop {
            body: Box::new(TypedExpressionInput::Block {
                statements: vec![TypedExpressionInput::Break(Some(Box::new(integer(
                    "7", None,
                ))))],
                tail: None,
            }),
        };
        let checked = check(&input).unwrap();
        assert_eq!(checked.ty(), &SymbolicType::I32);
        let CheckedExpressionKind::Loop { break_count, .. } = checked.kind() else {
            panic!("expected loop fact");
        };
        assert_eq!(*break_count, 1);
    }

    #[test]
    fn a_loop_without_break_is_never_without_a_leaked_join_variable() {
        let input = TypedExpressionInput::Loop {
            body: Box::new(TypedExpressionInput::Continue),
        };
        assert_eq!(check(&input).unwrap().ty(), &SymbolicType::Never);
    }

    #[test]
    fn inference_variables_are_owner_branded_and_recursive_unions_fail_closed() {
        let context = context();
        let mut first = TypeChecker::new(&context);
        let mut second = TypeChecker::new(&context);
        let local = first.variable(VariableClass::Any).unwrap();
        let foreign = second.variable(VariableClass::Any).unwrap();
        assert_eq!(
            first.unify(&local, &foreign).unwrap_err().kind(),
            &TypeCheckErrorKind::UnresolvedInferenceVariable
        );

        let outer = first.variable(VariableClass::Any).unwrap();
        let inner = first.variable(VariableClass::Any).unwrap();
        first
            .unify(&outer, &InferType::Tuple(vec![inner.clone()]))
            .unwrap();
        assert_eq!(
            first.unify(&outer, &inner).unwrap_err().kind(),
            &TypeCheckErrorKind::RecursiveInferenceType
        );
    }

    #[test]
    fn primitive_matrix_and_near_misses_are_classified() {
        let add = TypedExpressionInput::Binary {
            operator: BinaryTypeOperator::Add,
            left: Box::new(integer("1", Some(IntegerSuffix::U16))),
            right: Box::new(integer("2", Some(IntegerSuffix::U16))),
        };
        let checked = check(&add).unwrap();
        assert_eq!(checked.ty(), &SymbolicType::U16);

        let contextual_add = TypedExpressionInput::Binary {
            operator: BinaryTypeOperator::Add,
            left: Box::new(integer("1", None)),
            right: Box::new(integer("2", None)),
        };
        assert_eq!(
            check_typed_expression(&contextual_add, Some(&SymbolicType::U16), &context())
                .unwrap()
                .ty(),
            &SymbolicType::U16
        );

        let unsigned_neg = TypedExpressionInput::Unary {
            operator: UnaryTypeOperator::Negate,
            operand: Box::new(TypedExpressionInput::Known(SymbolicType::U16)),
        };
        assert_eq!(
            check(&unsigned_neg).unwrap_err().code().as_str(),
            "TRAIT002"
        );

        let float_rem = TypedExpressionInput::Binary {
            operator: BinaryTypeOperator::Remainder,
            left: Box::new(float("1.0", Some(FloatSuffix::F32))),
            right: Box::new(float("1.0", Some(FloatSuffix::F32))),
        };
        assert_eq!(check(&float_rem).unwrap_err().code().as_str(), "TRAIT002");

        let mixed_add = TypedExpressionInput::Binary {
            operator: BinaryTypeOperator::Add,
            left: Box::new(TypedExpressionInput::Known(SymbolicType::I64)),
            right: Box::new(TypedExpressionInput::Known(SymbolicType::U32)),
        };
        assert_eq!(
            check_typed_expression(&mixed_add, Some(&SymbolicType::I64), &context())
                .unwrap_err()
                .code()
                .as_str(),
            "TRAIT002"
        );

        let logical_not_integer = TypedExpressionInput::Unary {
            operator: UnaryTypeOperator::LogicalNot,
            operand: Box::new(integer("1", None)),
        };
        assert_eq!(
            check(&logical_not_integer).unwrap_err().code().as_str(),
            "TRAIT002"
        );

        let error = check(&logical_not_integer).unwrap_err();
        assert_eq!(
            error.message().to_string(),
            "no primitive Unary(LogicalNot) selection for left=I32, right=None, result=I32"
        );
        assert_eq!(
            error.to_string(),
            "TRAIT002: no primitive Unary(LogicalNot) selection for left=I32, right=None, result=I32"
        );

        let typed_logical_not = TypedExpressionInput::Unary {
            operator: UnaryTypeOperator::LogicalNot,
            operand: Box::new(integer("0", Some(IntegerSuffix::U32))),
        };
        let error =
            check_typed_expression(&typed_logical_not, Some(&SymbolicType::U32), &context())
                .unwrap_err();
        assert_eq!(error.code().as_str(), "TRAIT002");
    }

    #[test]
    fn different_float_comparison_types_are_type002() {
        let input = TypedExpressionInput::Binary {
            operator: BinaryTypeOperator::Equal,
            left: Box::new(float("1.0", Some(FloatSuffix::F32))),
            right: Box::new(float("1.0", Some(FloatSuffix::F64))),
        };
        assert_eq!(check(&input).unwrap_err().code().as_str(), "TYPE002");
    }

    #[test]
    fn assignment_accepts_only_the_closed_coercion_relation() {
        let source = SymbolicType::Reference {
            mutability: Mutability::Mutable,
            lifetime: SymbolicLifetime::Static,
            pointee: Box::new(SymbolicType::I32),
        };
        let target = SymbolicType::Reference {
            mutability: Mutability::Shared,
            lifetime: SymbolicLifetime::Static,
            pointee: Box::new(SymbolicType::I32),
        };
        let input = TypedExpressionInput::Assignment {
            place_type: target,
            value: Box::new(TypedExpressionInput::Known(source)),
        };
        assert_eq!(check(&input).unwrap().ty(), &SymbolicType::Unit);

        let mismatch = TypedExpressionInput::Assignment {
            place_type: SymbolicType::U32,
            value: Box::new(TypedExpressionInput::Known(SymbolicType::I32)),
        };
        assert_eq!(check(&mismatch).unwrap_err().code().as_str(), "TYPE002");
    }

    #[test]
    fn add_assignment_contextualizes_untyped_literals_but_selects_typed_operands() {
        let contextual = TypedExpressionInput::AddAssignment {
            place_type: SymbolicType::I64,
            value: Box::new(integer("1", None)),
        };
        assert_eq!(check(&contextual).unwrap().ty(), &SymbolicType::Unit);

        let mixed = TypedExpressionInput::AddAssignment {
            place_type: SymbolicType::I64,
            value: Box::new(TypedExpressionInput::Known(SymbolicType::U32)),
        };
        let error = check(&mixed).unwrap_err();
        assert_eq!(error.code().as_str(), "TRAIT002");
        assert!(matches!(
            error.kind(),
            TypeCheckErrorKind::UnsatisfiedPrimitiveOperator {
                operator: PrimitiveExpressionOperator::AddAssignment,
                left: SymbolicType::I64,
                right: Some(SymbolicType::U32),
                result: SymbolicType::I64,
            }
        ));
    }

    #[test]
    fn structural_matching_covers_raw_pointers_function_pointers_and_const_trees() {
        let raw = SymbolicType::RawPointer {
            mutability: Mutability::Shared,
            pointee: Box::new(SymbolicType::Tuple(vec![
                SymbolicType::I8,
                SymbolicType::Bool,
            ])),
        };
        assert_eq!(
            check_typed_expression(
                &TypedExpressionInput::Known(raw.clone()),
                Some(&raw),
                &context(),
            )
            .unwrap()
            .ty(),
            &raw
        );

        let function = SymbolicType::FunctionPointer {
            unsafe_: false,
            parameters: vec![raw],
            result: Box::new(SymbolicType::Never),
            requires: Default::default(),
            throws: Default::default(),
        };
        assert_eq!(
            check_typed_expression(
                &TypedExpressionInput::Known(function.clone()),
                Some(&function),
                &context(),
            )
            .unwrap()
            .ty(),
            &function
        );

        let left = SymbolicType::Array {
            element: Box::new(SymbolicType::U8),
            length: SymbolicConstExpression {
                integer_type: IntegerType::Usize,
                node: SymbolicConstNode::WrappingAdd(
                    Box::new(const_usize(1)),
                    Box::new(const_usize(2)),
                ),
            },
        };
        let right = SymbolicType::Array {
            element: Box::new(SymbolicType::U8),
            length: SymbolicConstExpression {
                integer_type: IntegerType::Usize,
                node: SymbolicConstNode::WrappingAdd(
                    Box::new(const_usize(2)),
                    Box::new(const_usize(1)),
                ),
            },
        };
        assert_eq!(
            check_typed_expression(&TypedExpressionInput::Known(left), Some(&right), &context(),)
                .unwrap_err()
                .code()
                .as_str(),
            "TYPE002"
        );
    }

    #[test]
    fn generic_kind_validation_uses_exact_mixed_argument_kinds() {
        let declaration = SemanticDeclarationPath {
            registry_origin: "registry+https://packages.arche-lang.org".to_owned(),
            package_name: "typing-test".to_owned(),
            target: TargetRoot::Library,
            modules: Vec::new(),
            kind: DeclarationKind::Struct,
            name: "N".to_owned(),
        };
        let actuals = vec![
            GenericArgumentShape::Type(SymbolicType::I32),
            GenericArgumentShape::Lifetime(SymbolicLifetime::Static),
            GenericArgumentShape::IntegerConst(SymbolicConstExpression {
                integer_type: IntegerType::U16,
                node: SymbolicConstNode::ConstDefinitionPath(declaration),
            }),
        ];
        let formals = vec![
            GenericParameterKind::Type,
            GenericParameterKind::Lifetime,
            GenericParameterKind::IntegerConst(IntegerType::U16),
        ];
        check_generic_instantiation(&formals, &actuals, &BinderStack::default()).unwrap();

        let mut reversed = actuals;
        reversed.reverse();
        assert_eq!(
            check_generic_instantiation(&formals, &reversed, &BinderStack::default())
                .unwrap_err()
                .code()
                .as_str(),
            "TYPE001"
        );
    }
}
