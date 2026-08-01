use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::hash::Hash;
use std::io::{self, Read, Seek, Write};

use archec0::execution_package_v2::{
    decode_package_from, decode_package_from_with_code_range, CodeImageRange, ExecutionPackage,
    ExecutionPackageV2Error, FunctionLinkRecord, FunctionTarget, QueryAccess, SourceSpanRecord,
};
use archec0::ids_v2::{DeclId, PrimitiveType, SchemaId};
use archec0::observation_v2;
use archec0::runtime_v2::{
    QueryIndex, ResourceState, RuntimeV2Error, RuntimeWorldV2, ScheduleIndex, SchemaPayload,
    StartupOperationView, SystemParameterKind,
};
use archec0::scalar_v2::{self, ComparisonOp, F32BinaryOp, I32BinaryOp, ScalarValue, TrapKind};
use archec0::trap_v2::{self, TrapSite, TRAP_EXIT_STATUS};

use crate::core::{
    BlockId, CoreBinaryOp, CoreComparisonOp, CoreFunction, CoreInstruction, CoreQueryLoop,
    CoreSourceSubject, CoreSystem, CoreSystemBinaryOp, CoreSystemExpression, CoreSystemPlace,
    CoreSystemStatement, CoreSystemUnaryOp, CoreTerminator, CoreType, CoreUnaryOp, ValueId,
};
use crate::core_verify::VerifiedExecutableCore;
use crate::execution_package_build::{
    canonical_core_ids, validate_execution_package_link, ExecutionPackageBuildError,
};
use crate::lexer::SourceSpan;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceExecutionOutcome {
    Exited { status: i32 },
    Trapped { kind: TrapKind },
}

impl ReferenceExecutionOutcome {
    pub const fn process_status(self) -> i32 {
        match self {
            Self::Exited { status } => status,
            Self::Trapped { .. } => TRAP_EXIT_STATUS,
        }
    }
}

#[derive(Debug)]
pub enum ReferenceExecutionError {
    Package(ExecutionPackageV2Error),
    Link(ExecutionPackageBuildError),
    Runtime(RuntimeV2Error),
    InvalidVerifiedCore(String),
    AddressSpaceOverflow(&'static str),
    Allocation(&'static str),
    Observation(io::Error),
}

impl fmt::Display for ReferenceExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Package(error) => write!(formatter, "{error}"),
            Self::Link(error) => write!(formatter, "{error}"),
            Self::Runtime(error) => write!(formatter, "{error}"),
            Self::InvalidVerifiedCore(message) => formatter.write_str(message),
            Self::AddressSpaceOverflow(context) => {
                write!(formatter, "{context} does not fit the host address space")
            }
            Self::Allocation(context) => write!(formatter, "allocation failed for {context}"),
            Self::Observation(error) => write!(formatter, "observation output failed: {error}"),
        }
    }
}

impl Error for ReferenceExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Package(error) => Some(error),
            Self::Link(error) => Some(error),
            Self::Runtime(error) => Some(error),
            Self::Observation(error) => Some(error),
            Self::InvalidVerifiedCore(_) | Self::AddressSpaceOverflow(_) | Self::Allocation(_) => {
                None
            }
        }
    }
}

impl From<ExecutionPackageV2Error> for ReferenceExecutionError {
    fn from(error: ExecutionPackageV2Error) -> Self {
        Self::Package(error)
    }
}

impl From<ExecutionPackageBuildError> for ReferenceExecutionError {
    fn from(error: ExecutionPackageBuildError) -> Self {
        Self::Link(error)
    }
}

impl From<RuntimeV2Error> for ReferenceExecutionError {
    fn from(error: RuntimeV2Error) -> Self {
        Self::Runtime(error)
    }
}

/// Decode, link, and execute an ARCHEECS v2 package through verified Core.
///
/// Package and Core linkage is completed before `RuntimeWorldV2` can mutate.
/// A normal source exit streams ARCHEOBS2 and returns its low-eight-bit status;
/// a semantic integer trap streams the committed snapshot, writes the exact
/// source diagnostic, and returns status 70 through the outcome.
pub fn execute_from<R: Read + Seek, WOut: Write, WErr: Write>(
    core: &VerifiedExecutableCore,
    metadata: &mut R,
    stdout: &mut WOut,
    stderr: &mut WErr,
) -> Result<ReferenceExecutionOutcome, ReferenceExecutionError> {
    let package = decode_package_from(metadata)?;
    execute_decoded(core, package, None, stdout, stderr)
}

pub fn execute_from_with_code_range<R: Read + Seek, WOut: Write, WErr: Write>(
    core: &VerifiedExecutableCore,
    metadata: &mut R,
    code_range: CodeImageRange,
    stdout: &mut WOut,
    stderr: &mut WErr,
) -> Result<ReferenceExecutionOutcome, ReferenceExecutionError> {
    let package = decode_package_from_with_code_range(metadata, code_range)?;
    execute_decoded(core, package, Some(code_range), stdout, stderr)
}

pub fn execute_decoded<WOut: Write, WErr: Write>(
    core: &VerifiedExecutableCore,
    package: ExecutionPackage,
    code_range: Option<CodeImageRange>,
    stdout: &mut WOut,
    stderr: &mut WErr,
) -> Result<ReferenceExecutionOutcome, ReferenceExecutionError> {
    scalar_v2::initialize_floating_point_environment();

    let link = InterpreterLink::build(core, &package, code_range)?;
    let mut world = RuntimeWorldV2::from_package(package)?;
    let startup_operations = own_startup_operations(&world)?;
    if startup_operations.len() != core.startup_operations().count() {
        return Err(invalid_core(
            "metadata startup operation count does not match verified Core",
        ));
    }
    let interpreter = Interpreter {
        core,
        link,
        startup_operations,
    };

    match interpreter.execute_startup(&mut world) {
        Ok(status) => {
            observation_v2::write_observation(&world, stdout)
                .map_err(ReferenceExecutionError::Observation)?;
            stdout
                .flush()
                .map_err(ReferenceExecutionError::Observation)?;
            Ok(ReferenceExecutionOutcome::Exited { status })
        }
        Err(StepError::Trap(event)) => {
            trap_v2::emit_trap(&world, stdout, stderr, event.kind, event.site.borrowed())
                .map_err(ReferenceExecutionError::Observation)?;
            Ok(ReferenceExecutionOutcome::Trapped { kind: event.kind })
        }
        Err(StepError::Ordinary(error)) => Err(error),
    }
}

#[derive(Clone, Debug)]
struct OwnedTrapSite {
    basename: String,
    line: u64,
    column: u64,
    start_byte: u64,
    end_byte: u64,
}

impl OwnedTrapSite {
    fn borrowed(&self) -> TrapSite<'_> {
        TrapSite {
            basename: &self.basename,
            line: self.line,
            column: self.column,
            start_byte: self.start_byte,
            end_byte: self.end_byte,
        }
    }
}

#[derive(Clone, Debug)]
struct TrapEvent {
    kind: TrapKind,
    site: OwnedTrapSite,
}

#[derive(Debug)]
enum StepError {
    Ordinary(ReferenceExecutionError),
    Trap(TrapEvent),
}

impl From<ReferenceExecutionError> for StepError {
    fn from(error: ReferenceExecutionError) -> Self {
        Self::Ordinary(error)
    }
}

impl From<RuntimeV2Error> for StepError {
    fn from(error: RuntimeV2Error) -> Self {
        Self::Ordinary(ReferenceExecutionError::Runtime(error))
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum TrapPoint {
    Startup {
        block: BlockId,
        instruction_index: u64,
    },
    System {
        system_id: u64,
        expression_ordinal: u64,
    },
}

struct InterpreterLink {
    core_system_by_id: HashMap<DeclId, usize>,
    expression_ordinals: HashMap<usize, u64>,
    trap_sites: HashMap<TrapPoint, OwnedTrapSite>,
}

impl InterpreterLink {
    fn build(
        core: &VerifiedExecutableCore,
        package: &ExecutionPackage,
        code_range: Option<CodeImageRange>,
    ) -> Result<Self, ReferenceExecutionError> {
        validate_execution_package_link(core, package, code_range)?;
        let ids = canonical_core_ids(core)?;
        let mut core_system_by_id = HashMap::new();
        reserve_map(
            &mut core_system_by_id,
            core.program().systems.len(),
            "Core system link map",
        )?;
        for (index, system) in core.program().systems.iter().enumerate() {
            let id = ids.system(system.id).ok_or_else(|| {
                invalid_core(format!(
                    "verified Core system `{}` has no canonical identifier",
                    system.name
                ))
            })?;
            if core_system_by_id.insert(id, index).is_some() {
                return Err(invalid_core("duplicate canonical Core system identifier"));
            }
        }

        let expression_count = core
            .program()
            .source_map
            .entries
            .iter()
            .filter(|entry| matches!(entry.subject, CoreSourceSubject::SystemExpression { .. }))
            .count();
        let mut expression_ordinals = HashMap::new();
        reserve_map(
            &mut expression_ordinals,
            expression_count,
            "system expression ordinal map",
        )?;
        let mut trap_sites = HashMap::new();
        reserve_map(&mut trap_sites, expression_count, "semantic trap-site map")?;

        index_startup_traps(core, package, &mut trap_sites)?;
        for system in &core.program().systems {
            let canonical_id = ids.system(system.id).ok_or_else(|| {
                invalid_core(format!(
                    "verified Core system `{}` has no canonical identifier",
                    system.name
                ))
            })?;
            let system_index = package
                .systems
                .binary_search_by_key(&canonical_id, |record| record.id)
                .map_err(|_| {
                    invalid_core(format!("metadata has no linked system `{}`", system.name))
                })?;
            let function_link = package
                .function_links
                .get(system_index + 1)
                .ok_or_else(|| {
                    invalid_core(format!(
                        "metadata has no function link for system `{}`",
                        system.name
                    ))
                })?;
            index_system_expressions(
                core,
                package,
                system,
                function_link,
                &mut expression_ordinals,
                &mut trap_sites,
            )?;
        }

        Ok(Self {
            core_system_by_id,
            expression_ordinals,
            trap_sites,
        })
    }

    fn trap(&self, point: TrapPoint, kind: TrapKind) -> StepError {
        match self.trap_sites.get(&point) {
            Some(site) => StepError::Trap(TrapEvent {
                kind,
                site: site.clone(),
            }),
            None => StepError::Ordinary(invalid_core(format!(
                "verified Core trap site {point:?} has no linked v2 source span"
            ))),
        }
    }
}

fn index_startup_traps(
    core: &VerifiedExecutableCore,
    package: &ExecutionPackage,
    trap_sites: &mut HashMap<TrapPoint, OwnedTrapSite>,
) -> Result<(), ReferenceExecutionError> {
    let startup = startup_function(core)?;
    let function_link = package
        .function_links
        .first()
        .filter(|link| matches!(link.target, FunctionTarget::Startup))
        .ok_or_else(|| invalid_core("metadata has no startup function link"))?;
    for block in &startup.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let CoreInstruction::I32Binary { op, .. } = instruction else {
                continue;
            };
            if !matches!(op, CoreBinaryOp::Divide | CoreBinaryOp::Remainder) {
                continue;
            }
            let instruction_index = as_u64(instruction_index, "startup instruction index")?;
            let subject = CoreSourceSubject::StartupInstruction {
                block: block.id,
                instruction_index,
            };
            let span = required_core_span(core, &subject)?;
            let site = linked_trap_site(package, function_link, span)?;
            trap_sites.insert(
                TrapPoint::Startup {
                    block: block.id,
                    instruction_index,
                },
                site,
            );
        }
    }
    Ok(())
}

fn index_system_expressions(
    core: &VerifiedExecutableCore,
    package: &ExecutionPackage,
    system: &CoreSystem,
    function_link: &FunctionLinkRecord,
    expression_ordinals: &mut HashMap<usize, u64>,
    trap_sites: &mut HashMap<TrapPoint, OwnedTrapSite>,
) -> Result<(), ReferenceExecutionError> {
    let mut ordinal = 0;
    let mut context = ExpressionIndexContext {
        core,
        package,
        system,
        function_link,
        expression_ordinals,
        trap_sites,
    };
    index_statements(&mut context, &system.body.statements, &mut ordinal)?;
    let mapped_count = core
        .program()
        .source_map
        .entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.subject,
                CoreSourceSubject::SystemExpression { system_id, .. }
                    if system_id == system.id
            )
        })
        .count();
    if ordinal != as_u64(mapped_count, "system expression source-map count")? {
        return Err(invalid_core(format!(
            "system `{}` expression source map does not match Core structure",
            system.name
        )));
    }
    Ok(())
}

struct ExpressionIndexContext<'a> {
    core: &'a VerifiedExecutableCore,
    package: &'a ExecutionPackage,
    system: &'a CoreSystem,
    function_link: &'a FunctionLinkRecord,
    expression_ordinals: &'a mut HashMap<usize, u64>,
    trap_sites: &'a mut HashMap<TrapPoint, OwnedTrapSite>,
}

fn index_statements(
    context: &mut ExpressionIndexContext<'_>,
    statements: &[CoreSystemStatement],
    ordinal: &mut u64,
) -> Result<(), ReferenceExecutionError> {
    for statement in statements {
        match statement {
            CoreSystemStatement::Expression(expression) => {
                index_expression(context, expression, ordinal)?;
            }
            CoreSystemStatement::Let { value, .. }
            | CoreSystemStatement::Assign { value, .. }
            | CoreSystemStatement::AddAssign { value, .. } => {
                index_expression(context, value, ordinal)?;
            }
            CoreSystemStatement::QueryLoop(query) => {
                index_statements(context, &query.body, ordinal)?;
            }
            CoreSystemStatement::Block(body) => {
                index_statements(context, body, ordinal)?;
            }
            CoreSystemStatement::If {
                condition,
                then_body,
                else_body,
            } => {
                index_expression(context, condition, ordinal)?;
                index_statements(context, then_body, ordinal)?;
                index_statements(context, else_body, ordinal)?;
            }
            CoreSystemStatement::While { condition, body } => {
                index_expression(context, condition, ordinal)?;
                index_statements(context, body, ordinal)?;
            }
        }
    }
    Ok(())
}

fn index_expression(
    context: &mut ExpressionIndexContext<'_>,
    expression: &CoreSystemExpression,
    ordinal: &mut u64,
) -> Result<(), ReferenceExecutionError> {
    let current = *ordinal;
    *ordinal = ordinal
        .checked_add(1)
        .ok_or(ReferenceExecutionError::AddressSpaceOverflow(
            "system expression ordinal",
        ))?;
    let key = expression as *const CoreSystemExpression as usize;
    if context.expression_ordinals.insert(key, current).is_some() {
        return Err(invalid_core(
            "one Core system expression occurs at multiple source-map positions",
        ));
    }
    if matches!(
        expression,
        CoreSystemExpression::Binary {
            op: CoreSystemBinaryOp::I32Divide | CoreSystemBinaryOp::I32Remainder,
            ..
        }
    ) {
        let subject = CoreSourceSubject::SystemExpression {
            system_id: context.system.id,
            expression_ordinal: current,
        };
        let span = required_core_span(context.core, &subject)?;
        let site = linked_trap_site(context.package, context.function_link, span)?;
        context.trap_sites.insert(
            TrapPoint::System {
                system_id: context.system.id,
                expression_ordinal: current,
            },
            site,
        );
    }
    match expression {
        CoreSystemExpression::ResourceField { .. }
        | CoreSystemExpression::ComponentField { .. } => {}
        CoreSystemExpression::BoolNot(operand) | CoreSystemExpression::Unary { operand, .. } => {
            index_expression(context, operand, ordinal)?;
        }
        CoreSystemExpression::Binary { left, right, .. } => {
            index_expression(context, left, ordinal)?;
            index_expression(context, right, ordinal)?;
        }
        CoreSystemExpression::I32Const(_)
        | CoreSystemExpression::F32Const(_)
        | CoreSystemExpression::BoolConst(_)
        | CoreSystemExpression::Local { .. } => {}
    }
    Ok(())
}

fn required_core_span(
    core: &VerifiedExecutableCore,
    subject: &CoreSourceSubject,
) -> Result<SourceSpan, ReferenceExecutionError> {
    core.program().source_map.span(subject).ok_or_else(|| {
        invalid_core(format!(
            "verified Core source map has no entry for {subject:?}"
        ))
    })
}

fn linked_trap_site(
    package: &ExecutionPackage,
    function_link: &FunctionLinkRecord,
    span: SourceSpan,
) -> Result<OwnedTrapSite, ReferenceExecutionError> {
    let first = function_link.first_body_span.ok_or_else(|| {
        invalid_core("linked function with a semantic trap has no v2 body-span slice")
    })?;
    let end = first
        .index()
        .checked_add(function_link.body_span_count)
        .ok_or(ReferenceExecutionError::AddressSpaceOverflow(
            "function body-span range",
        ))?;
    let first_index = as_usize(first.index(), "function body-span index")?;
    let end_index = as_usize(end, "function body-span end")?;
    let body_spans = package
        .source_spans
        .get(first_index..end_index)
        .ok_or_else(|| invalid_core("linked function body-span slice is out of range"))?;
    let record = body_spans
        .iter()
        .find(|record| source_span_matches(record, span))
        .ok_or_else(|| {
            invalid_core(format!(
                "Core trap span bytes {}..{} is absent from its linked v2 function slice",
                span.start.byte, span.end.byte
            ))
        })?;
    let file_index = as_usize(record.file_name.index(), "source file-name index")?;
    let basename = package
        .strings
        .get(file_index)
        .ok_or_else(|| invalid_core("trap source file-name reference is out of range"))?;
    Ok(OwnedTrapSite {
        basename: try_clone_string(basename, "trap source basename")?,
        line: record.start_line,
        column: record.start_column,
        start_byte: record.start_byte,
        end_byte: record.end_byte,
    })
}

fn source_span_matches(record: &SourceSpanRecord, span: SourceSpan) -> bool {
    record.start_byte == span.start.byte
        && record.end_byte == span.end.byte
        && record.start_line == span.start.line
        && record.start_column == span.start.column
        && record.end_line == span.end.line
        && record.end_column == span.end.column
}

#[derive(Clone, Debug)]
enum OwnedStartupOperation {
    InitializeResource { schema: SchemaId, bytes: Vec<u8> },
    Spawn(Vec<OwnedSchemaPayload>),
    RunSchedule(ScheduleIndex),
}

#[derive(Clone, Debug)]
struct OwnedSchemaPayload {
    schema: SchemaId,
    bytes: Vec<u8>,
}

fn own_startup_operations(
    world: &RuntimeWorldV2,
) -> Result<Vec<OwnedStartupOperation>, ReferenceExecutionError> {
    let mut operations = Vec::new();
    let operation_count = world.startup_operations().len();
    reserve_vec(&mut operations, operation_count, "startup operation cursor")?;
    for operation in world.startup_operations() {
        match operation {
            StartupOperationView::InitializeResource(payload) => {
                operations.push(OwnedStartupOperation::InitializeResource {
                    schema: payload.schema_id,
                    bytes: try_copy_bytes(payload.bytes, "startup resource payload")?,
                });
            }
            StartupOperationView::Spawn(payloads) => {
                let mut owned = Vec::new();
                reserve_vec(&mut owned, payloads.len(), "startup spawn payloads")?;
                for payload in payloads {
                    owned.push(OwnedSchemaPayload {
                        schema: payload.schema_id(),
                        bytes: try_copy_bytes(payload.bytes(), "startup spawn payload")?,
                    });
                }
                operations.push(OwnedStartupOperation::Spawn(owned));
            }
            StartupOperationView::RunSchedule(schedule) => {
                operations.push(OwnedStartupOperation::RunSchedule(schedule));
            }
        }
    }
    Ok(operations)
}

struct Interpreter<'a> {
    core: &'a VerifiedExecutableCore,
    link: InterpreterLink,
    startup_operations: Vec<OwnedStartupOperation>,
}

impl Interpreter<'_> {
    fn execute_startup(&self, world: &mut RuntimeWorldV2) -> Result<i32, StepError> {
        let startup = startup_function(self.core)?;
        let instruction_count = startup.blocks.iter().try_fold(0usize, |count, block| {
            count.checked_add(block.instructions.len()).ok_or(
                ReferenceExecutionError::AddressSpaceOverflow("startup instruction count"),
            )
        })?;
        let mut values = HashMap::new();
        reserve_map(&mut values, instruction_count, "startup value map")?;
        let mut locals = HashMap::new();
        reserve_map(&mut locals, startup.locals.len(), "startup local map")?;
        let mut operation_cursor = 0usize;
        let mut block_id = startup.entry;

        loop {
            let block = startup
                .blocks
                .iter()
                .find(|block| block.id == block_id)
                .ok_or_else(|| {
                    invalid_step(format!(
                        "verified startup references absent block {}",
                        block_id.0
                    ))
                })?;
            for (instruction_index, instruction) in block.instructions.iter().enumerate() {
                let instruction_index = as_u64(instruction_index, "startup instruction index")?;
                match instruction {
                    CoreInstruction::InitializeResource { .. }
                    | CoreInstruction::Spawn { .. }
                    | CoreInstruction::RunSchedule { .. } => {
                        let operation =
                            self.startup_operations
                                .get(operation_cursor)
                                .ok_or_else(|| {
                                    invalid_step(
                                "verified startup has more side-effect slots than v2 metadata",
                            )
                                })?;
                        self.execute_metadata_operation(world, operation)?;
                        operation_cursor = operation_cursor.checked_add(1).ok_or(
                            ReferenceExecutionError::AddressSpaceOverflow(
                                "startup operation cursor",
                            ),
                        )?;
                    }
                    CoreInstruction::I32Const { result, value } => {
                        insert_value(&mut values, *result, ScalarValue::I32(*value))?;
                    }
                    CoreInstruction::I32Binary {
                        result,
                        op,
                        left,
                        right,
                    } => {
                        let left = expect_i32(value(&values, *left)?)?;
                        let right = expect_i32(value(&values, *right)?)?;
                        let scalar_op = i32_binary_op(*op);
                        let result_value =
                            scalar_v2::i32_binary(scalar_op, left, right).map_err(|kind| {
                                self.link.trap(
                                    TrapPoint::Startup {
                                        block: block.id,
                                        instruction_index,
                                    },
                                    kind,
                                )
                            })?;
                        insert_value(&mut values, *result, ScalarValue::I32(result_value))?;
                    }
                    CoreInstruction::I32Unary {
                        result,
                        op,
                        operand,
                    } => {
                        let operand = expect_i32(value(&values, *operand)?)?;
                        let result_value = match op {
                            CoreUnaryOp::Negate => scalar_v2::i32_negate(operand),
                            CoreUnaryOp::BitNot => scalar_v2::i32_bit_not(operand),
                        };
                        insert_value(&mut values, *result, ScalarValue::I32(result_value))?;
                    }
                    CoreInstruction::F32Const { result, bits } => {
                        insert_value(&mut values, *result, ScalarValue::F32Bits(*bits))?;
                    }
                    CoreInstruction::F32Unary {
                        result,
                        op,
                        operand,
                    } => {
                        let operand = expect_f32(value(&values, *operand)?)?;
                        let result_value = match op {
                            CoreUnaryOp::Negate => scalar_v2::f32_negate(operand),
                            CoreUnaryOp::BitNot => {
                                return Err(invalid_step(
                                    "verified Core applies bitwise not to f32",
                                ));
                            }
                        };
                        insert_value(&mut values, *result, ScalarValue::F32Bits(result_value))?;
                    }
                    CoreInstruction::F32Binary {
                        result,
                        op,
                        left,
                        right,
                    } => {
                        let left = expect_f32(value(&values, *left)?)?;
                        let right = expect_f32(value(&values, *right)?)?;
                        let op = f32_binary_op(*op)?;
                        insert_value(
                            &mut values,
                            *result,
                            ScalarValue::F32Bits(scalar_v2::f32_binary(op, left, right)),
                        )?;
                    }
                    CoreInstruction::Compare {
                        result,
                        op,
                        left,
                        right,
                        operand_type,
                    } => {
                        let comparison = comparison_op(*op);
                        let result_value = match operand_type {
                            CoreType::I32 => scalar_v2::i32_compare(
                                comparison,
                                expect_i32(value(&values, *left)?)?,
                                expect_i32(value(&values, *right)?)?,
                            ),
                            CoreType::F32 => scalar_v2::f32_compare(
                                comparison,
                                expect_f32(value(&values, *left)?)?,
                                expect_f32(value(&values, *right)?)?,
                            ),
                            CoreType::Bool => {
                                return Err(invalid_step(
                                    "verified Core applies ordered comparison to bool",
                                ));
                            }
                        };
                        insert_value(&mut values, *result, ScalarValue::Bool(result_value))?;
                    }
                    CoreInstruction::BoolConst { result, value } => {
                        insert_value(&mut values, *result, ScalarValue::Bool(*value))?;
                    }
                    CoreInstruction::BoolNot { result, operand } => {
                        let operand = expect_bool(value(&values, *operand)?)?;
                        insert_value(&mut values, *result, ScalarValue::Bool(!operand))?;
                    }
                    CoreInstruction::Equal {
                        result,
                        left,
                        right,
                        operand_type,
                        negate,
                    } => {
                        let equal = equal_values(
                            *operand_type,
                            value(&values, *left)?,
                            value(&values, *right)?,
                        )?;
                        insert_value(
                            &mut values,
                            *result,
                            ScalarValue::Bool(if *negate { !equal } else { equal }),
                        )?;
                    }
                    CoreInstruction::LocalStore {
                        local,
                        value: source,
                    } => {
                        let stored = value(&values, *source)?;
                        locals.insert(*local, stored);
                    }
                    CoreInstruction::LocalLoad { result, local } => {
                        let loaded = locals.get(local).copied().ok_or_else(|| {
                            invalid_step(format!(
                                "verified startup reads uninitialized local {}",
                                local.0
                            ))
                        })?;
                        insert_value(&mut values, *result, loaded)?;
                    }
                }
            }

            match block.terminator {
                CoreTerminator::Exit { value: exit_value } => {
                    if operation_cursor != self.startup_operations.len() {
                        return Err(invalid_step(
                            "v2 metadata has startup operations with no verified Core slot",
                        ));
                    }
                    let value = expect_i32(value(&values, exit_value)?)?;
                    return Ok((value as u32 & 0xff) as i32);
                }
                CoreTerminator::Jump { target } => block_id = target,
                CoreTerminator::Branch {
                    condition,
                    then_block,
                    else_block,
                } => {
                    block_id = if expect_bool(value(&values, condition)?)? {
                        then_block
                    } else {
                        else_block
                    };
                }
            }
        }
    }

    fn execute_metadata_operation(
        &self,
        world: &mut RuntimeWorldV2,
        operation: &OwnedStartupOperation,
    ) -> Result<(), StepError> {
        match operation {
            OwnedStartupOperation::InitializeResource { schema, bytes } => {
                world.initialize_resource(*schema, bytes)?;
            }
            OwnedStartupOperation::Spawn(payloads) => {
                let mut borrowed = Vec::new();
                reserve_vec(&mut borrowed, payloads.len(), "borrowed spawn payloads")?;
                for payload in payloads {
                    borrowed.push(SchemaPayload {
                        schema_id: payload.schema,
                        bytes: &payload.bytes,
                    });
                }
                world.spawn(&borrowed)?;
            }
            OwnedStartupOperation::RunSchedule(schedule) => {
                self.execute_schedule(world, *schedule)?;
            }
        }
        Ok(())
    }

    fn execute_schedule(
        &self,
        world: &mut RuntimeWorldV2,
        schedule: ScheduleIndex,
    ) -> Result<(), StepError> {
        let systems = {
            let descriptor = world.schedule(schedule)?;
            let mut systems = Vec::new();
            reserve_vec(
                &mut systems,
                descriptor.systems().len(),
                "schedule dispatch list",
            )?;
            systems.extend_from_slice(descriptor.systems());
            systems
        };
        for system_index in systems {
            let system_id = world.system(system_index)?.id();
            self.execute_system(world, system_id)?;
        }
        Ok(())
    }

    fn execute_system(
        &self,
        world: &mut RuntimeWorldV2,
        system_id: DeclId,
    ) -> Result<(), StepError> {
        let core_index = self
            .link
            .core_system_by_id
            .get(&system_id)
            .copied()
            .ok_or_else(|| invalid_step(format!("metadata selected unknown system {system_id}")))?;
        let system = self.core.program().systems.get(core_index).ok_or_else(|| {
            invalid_step("linked Core system index is outside the verified program")
        })?;
        let runtime_index = world
            .system_index(system_id)
            .ok_or_else(|| invalid_step(format!("runtime has no selected system {system_id}")))?;
        let parameters = own_invocation_parameters(world, runtime_index)?;
        let mut frame = SystemFrame::new(system, parameters)?;
        self.execute_scoped_statements(world, &mut frame, &system.body.statements)
    }

    fn execute_scoped_statements<'core>(
        &self,
        world: &mut RuntimeWorldV2,
        frame: &mut SystemFrame<'core>,
        statements: &'core [CoreSystemStatement],
    ) -> Result<(), StepError> {
        frame.push_scope(statements.len())?;
        let result = self.execute_statements(world, frame, statements);
        frame.pop_scope();
        result
    }

    fn execute_statements<'core>(
        &self,
        world: &mut RuntimeWorldV2,
        frame: &mut SystemFrame<'core>,
        statements: &'core [CoreSystemStatement],
    ) -> Result<(), StepError> {
        for statement in statements {
            match statement {
                CoreSystemStatement::Expression(expression) => {
                    self.evaluate_expression(world, frame, expression)?;
                }
                CoreSystemStatement::Let {
                    name,
                    mutable,
                    value,
                    ..
                } => {
                    let value = self.evaluate_expression(world, frame, value)?;
                    frame.bind_local(name, *mutable, value)?;
                }
                CoreSystemStatement::Assign { target, value } => {
                    let value = self.evaluate_expression(world, frame, value)?;
                    write_place(world, frame, target, value)?;
                }
                CoreSystemStatement::AddAssign { target, value } => {
                    let left = read_place(world, frame, target)?;
                    let right = self.evaluate_expression(world, frame, value)?;
                    let result = add_values(left, right)?;
                    write_place(world, frame, target, result)?;
                }
                CoreSystemStatement::QueryLoop(query) => {
                    self.execute_query_loop(world, frame, query)?;
                }
                CoreSystemStatement::Block(body) => {
                    self.execute_scoped_statements(world, frame, body)?;
                }
                CoreSystemStatement::If {
                    condition,
                    then_body,
                    else_body,
                } => {
                    if expect_bool(self.evaluate_expression(world, frame, condition)?)? {
                        self.execute_scoped_statements(world, frame, then_body)?;
                    } else {
                        self.execute_scoped_statements(world, frame, else_body)?;
                    }
                }
                CoreSystemStatement::While { condition, body } => {
                    while expect_bool(self.evaluate_expression(world, frame, condition)?)? {
                        self.execute_scoped_statements(world, frame, body)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn execute_query_loop<'core>(
        &self,
        world: &mut RuntimeWorldV2,
        frame: &mut SystemFrame<'core>,
        query_loop: &'core CoreQueryLoop,
    ) -> Result<(), StepError> {
        let query_index = frame.query_parameter(&query_loop.query_param)?;
        let terms = own_query_terms(world, query_index)?;
        let required_term_count = terms
            .iter()
            .filter(|term| term.access != QueryAccess::Exclude)
            .count();
        if required_term_count != query_loop.bindings.len() {
            return Err(invalid_step(format!(
                "query `{}` binding count does not match linked metadata",
                query_loop.query_param
            )));
        }

        let mut spawn_ordinals = Vec::new();
        for table in world.matching_tables(query_index)? {
            spawn_ordinals
                .try_reserve(table.rows().len())
                .map_err(|_| ReferenceExecutionError::Allocation("query row ordinals"))?;
            spawn_ordinals.extend(table.rows().iter().map(|row| row.spawn_ordinal()));
        }

        for spawn_ordinal in spawn_ordinals {
            let mut bindings = Vec::new();
            reserve_vec(
                &mut bindings,
                query_loop.bindings.len(),
                "query loop bindings",
            )?;
            let mut core_bindings = query_loop.bindings.iter();
            for term in &terms {
                if term.access == QueryAccess::Exclude {
                    continue;
                }
                let binding = core_bindings.next().ok_or_else(|| {
                    invalid_step("linked query has more required terms than Core bindings")
                })?;
                if binding.name != "_" {
                    bindings.push(RowBinding {
                        name: &binding.name,
                        schema: term.schema,
                        access: term.access,
                        spawn_ordinal,
                    });
                }
            }
            frame.query_bindings = bindings;
            let result = self.execute_scoped_statements(world, frame, &query_loop.body);
            frame.query_bindings.clear();
            result?;
        }
        Ok(())
    }

    fn evaluate_expression(
        &self,
        world: &RuntimeWorldV2,
        frame: &SystemFrame<'_>,
        expression: &CoreSystemExpression,
    ) -> Result<ScalarValue, StepError> {
        match expression {
            CoreSystemExpression::I32Const(value) => Ok(ScalarValue::I32(*value)),
            CoreSystemExpression::F32Const(bits) => Ok(ScalarValue::F32Bits(*bits)),
            CoreSystemExpression::BoolConst(value) => Ok(ScalarValue::Bool(*value)),
            CoreSystemExpression::Local { name, .. } => frame.local(name),
            CoreSystemExpression::ResourceField {
                param, field_name, ..
            } => {
                let schema = frame.resource_parameter(param)?.schema;
                read_resource_field(world, schema, field_name).map_err(Into::into)
            }
            CoreSystemExpression::ComponentField {
                binding,
                field_name,
                ..
            } => {
                let binding = frame.row_binding(binding)?;
                read_row_field(world, binding.spawn_ordinal, binding.schema, field_name)
                    .map_err(Into::into)
            }
            CoreSystemExpression::BoolNot(operand) => Ok(ScalarValue::Bool(!expect_bool(
                self.evaluate_expression(world, frame, operand)?,
            )?)),
            CoreSystemExpression::Unary { op, operand } => {
                let operand = self.evaluate_expression(world, frame, operand)?;
                evaluate_unary(*op, operand).map_err(Into::into)
            }
            CoreSystemExpression::Binary { op, left, right } => {
                if *op == CoreSystemBinaryOp::LogicalAnd {
                    let left = expect_bool(self.evaluate_expression(world, frame, left)?)?;
                    return if left {
                        Ok(ScalarValue::Bool(expect_bool(
                            self.evaluate_expression(world, frame, right)?,
                        )?))
                    } else {
                        Ok(ScalarValue::Bool(false))
                    };
                }
                if *op == CoreSystemBinaryOp::LogicalOr {
                    let left = expect_bool(self.evaluate_expression(world, frame, left)?)?;
                    return if left {
                        Ok(ScalarValue::Bool(true))
                    } else {
                        Ok(ScalarValue::Bool(expect_bool(
                            self.evaluate_expression(world, frame, right)?,
                        )?))
                    };
                }
                let left_value = self.evaluate_expression(world, frame, left)?;
                let right_value = self.evaluate_expression(world, frame, right)?;
                match evaluate_binary(*op, left_value, right_value) {
                    Ok(value) => Ok(value),
                    Err(BinaryEvaluationError::Ordinary(error)) => Err(error.into()),
                    Err(BinaryEvaluationError::Trap(kind)) => {
                        let key = expression as *const CoreSystemExpression as usize;
                        let ordinal = self
                            .link
                            .expression_ordinals
                            .get(&key)
                            .copied()
                            .ok_or_else(|| {
                                invalid_step("executed Core expression has no source-map ordinal")
                            })?;
                        Err(self.link.trap(
                            TrapPoint::System {
                                system_id: frame.system.id,
                                expression_ordinal: ordinal,
                            },
                            kind,
                        ))
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum InvocationParameterKind {
    ReadResource { schema: SchemaId },
    MutResource { schema: SchemaId },
    Query { query: QueryIndex },
}

struct InvocationParameter {
    name: String,
    kind: InvocationParameterKind,
}

fn own_invocation_parameters(
    world: &RuntimeWorldV2,
    system_index: archec0::runtime_v2::SystemIndex,
) -> Result<Vec<InvocationParameter>, ReferenceExecutionError> {
    let system = world.system(system_index)?;
    let mut parameters = Vec::new();
    reserve_vec(
        &mut parameters,
        system.parameters().len(),
        "system invocation parameters",
    )?;
    for parameter in system.parameters() {
        let kind = match parameter.kind() {
            SystemParameterKind::ReadResource { resource } => {
                InvocationParameterKind::ReadResource {
                    schema: world.schema(resource)?.id(),
                }
            }
            SystemParameterKind::MutResource { resource } => InvocationParameterKind::MutResource {
                schema: world.schema(resource)?.id(),
            },
            SystemParameterKind::Query { query } => InvocationParameterKind::Query { query },
        };
        parameters.push(InvocationParameter {
            name: try_clone_string(parameter.name(), "system parameter name")?,
            kind,
        });
    }
    Ok(parameters)
}

#[derive(Clone, Copy)]
struct OwnedQueryTerm {
    access: QueryAccess,
    schema: SchemaId,
}

fn own_query_terms(
    world: &RuntimeWorldV2,
    query: QueryIndex,
) -> Result<Vec<OwnedQueryTerm>, ReferenceExecutionError> {
    let query = world.query(query)?;
    let mut terms = Vec::new();
    reserve_vec(&mut terms, query.terms().len(), "query term bindings")?;
    for term in query.terms() {
        terms.push(OwnedQueryTerm {
            access: term.access(),
            schema: world.schema(term.schema())?.id(),
        });
    }
    Ok(terms)
}

struct LocalBinding<'a> {
    name: &'a str,
    mutable: bool,
    value: ScalarValue,
}

struct LocalScope<'a> {
    bindings: Vec<LocalBinding<'a>>,
}

#[derive(Clone, Copy)]
struct ResourceBinding {
    schema: SchemaId,
    mutable: bool,
}

#[derive(Clone, Copy)]
struct RowBinding<'a> {
    name: &'a str,
    schema: SchemaId,
    access: QueryAccess,
    spawn_ordinal: u64,
}

struct SystemFrame<'a> {
    system: &'a CoreSystem,
    parameters: Vec<InvocationParameter>,
    scopes: Vec<LocalScope<'a>>,
    query_bindings: Vec<RowBinding<'a>>,
}

impl<'a> SystemFrame<'a> {
    fn new(
        system: &'a CoreSystem,
        parameters: Vec<InvocationParameter>,
    ) -> Result<Self, ReferenceExecutionError> {
        let mut scopes = Vec::new();
        reserve_vec(&mut scopes, 8, "system lexical scopes")?;
        Ok(Self {
            system,
            parameters,
            scopes,
            query_bindings: Vec::new(),
        })
    }

    fn push_scope(&mut self, binding_capacity: usize) -> Result<(), ReferenceExecutionError> {
        if self.scopes.len() == self.scopes.capacity() {
            self.scopes
                .try_reserve(1)
                .map_err(|_| ReferenceExecutionError::Allocation("system lexical scopes"))?;
        }
        let mut bindings = Vec::new();
        reserve_vec(&mut bindings, binding_capacity, "system lexical bindings")?;
        self.scopes.push(LocalScope { bindings });
        Ok(())
    }

    fn pop_scope(&mut self) {
        self.scopes
            .pop()
            .expect("system executor balances verified lexical scopes");
    }

    fn bind_local(
        &mut self,
        name: &'a str,
        mutable: bool,
        value: ScalarValue,
    ) -> Result<(), ReferenceExecutionError> {
        let scope = self
            .scopes
            .last_mut()
            .ok_or_else(|| invalid_core("system local declaration has no lexical scope"))?;
        if scope.bindings.len() == scope.bindings.capacity() {
            scope
                .bindings
                .try_reserve(1)
                .map_err(|_| ReferenceExecutionError::Allocation("system lexical bindings"))?;
        }
        scope.bindings.push(LocalBinding {
            name,
            mutable,
            value,
        });
        Ok(())
    }

    fn local(&self, name: &str) -> Result<ScalarValue, StepError> {
        self.local_binding(name)
            .map(|binding| binding.value)
            .ok_or_else(|| invalid_step(format!("verified system reads unknown local `{name}`")))
    }

    fn local_binding(&self, name: &str) -> Option<&LocalBinding<'a>> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|scope| scope.bindings.iter().rev())
            .find(|binding| binding.name == name)
    }

    fn local_binding_mut(&mut self, name: &str) -> Option<&mut LocalBinding<'a>> {
        self.scopes
            .iter_mut()
            .rev()
            .flat_map(|scope| scope.bindings.iter_mut().rev())
            .find(|binding| binding.name == name)
    }

    fn parameter(&self, name: &str) -> Result<&InvocationParameter, StepError> {
        self.parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .ok_or_else(|| {
                invalid_step(format!(
                    "verified system references unknown parameter `{name}`"
                ))
            })
    }

    fn resource_parameter(&self, name: &str) -> Result<ResourceBinding, StepError> {
        match self.parameter(name)?.kind {
            InvocationParameterKind::ReadResource { schema } => Ok(ResourceBinding {
                schema,
                mutable: false,
            }),
            InvocationParameterKind::MutResource { schema } => Ok(ResourceBinding {
                schema,
                mutable: true,
            }),
            InvocationParameterKind::Query { .. } => Err(invalid_step(format!(
                "query parameter `{name}` is used as a resource"
            ))),
        }
    }

    fn query_parameter(&self, name: &str) -> Result<QueryIndex, StepError> {
        match self.parameter(name)?.kind {
            InvocationParameterKind::Query { query } => Ok(query),
            InvocationParameterKind::ReadResource { .. }
            | InvocationParameterKind::MutResource { .. } => Err(invalid_step(format!(
                "resource parameter `{name}` is used as a query"
            ))),
        }
    }

    fn row_binding(&self, name: &str) -> Result<RowBinding<'a>, StepError> {
        self.query_bindings
            .iter()
            .find(|binding| binding.name == name)
            .copied()
            .ok_or_else(|| invalid_step(format!("unknown active query binding `{name}`")))
    }
}

fn read_place(
    world: &RuntimeWorldV2,
    frame: &SystemFrame<'_>,
    place: &CoreSystemPlace,
) -> Result<ScalarValue, StepError> {
    match place {
        CoreSystemPlace::Local { name, .. } => frame.local(name),
        CoreSystemPlace::ResourceField {
            param, field_name, ..
        } => {
            let resource = frame.resource_parameter(param)?;
            read_resource_field(world, resource.schema, field_name).map_err(Into::into)
        }
        CoreSystemPlace::ComponentField {
            binding,
            field_name,
            ..
        } => {
            let binding = frame.row_binding(binding)?;
            read_row_field(world, binding.spawn_ordinal, binding.schema, field_name)
                .map_err(Into::into)
        }
    }
}

fn write_place(
    world: &mut RuntimeWorldV2,
    frame: &mut SystemFrame<'_>,
    place: &CoreSystemPlace,
    value: ScalarValue,
) -> Result<(), StepError> {
    match place {
        CoreSystemPlace::Local { name, .. } => {
            let local = frame.local_binding_mut(name).ok_or_else(|| {
                invalid_step(format!("verified system assigns unknown local `{name}`"))
            })?;
            if !local.mutable {
                return Err(invalid_step(format!(
                    "verified system assigns immutable local `{name}`"
                )));
            }
            if scalar_type(local.value) != scalar_type(value) {
                return Err(invalid_step(format!(
                    "verified system changes the type of local `{name}`"
                )));
            }
            local.value = value;
            Ok(())
        }
        CoreSystemPlace::ResourceField {
            param, field_name, ..
        } => {
            let resource = frame.resource_parameter(param)?;
            if !resource.mutable {
                return Err(invalid_step(format!(
                    "verified system mutates read-only resource parameter `{param}`"
                )));
            }
            write_resource_field(world, resource.schema, field_name, value).map_err(Into::into)
        }
        CoreSystemPlace::ComponentField {
            binding,
            field_name,
            ..
        } => {
            let binding = frame.row_binding(binding)?;
            if binding.access != QueryAccess::Mut {
                return Err(invalid_step(format!(
                    "verified system mutates read-only query binding `{}`",
                    binding.name
                )));
            }
            write_row_field(
                world,
                binding.spawn_ordinal,
                binding.schema,
                field_name,
                value,
            )
            .map_err(Into::into)
        }
    }
}

#[derive(Clone, Copy)]
struct FieldLocation {
    primitive: PrimitiveType,
    offset: usize,
    end: usize,
}

fn field_location(
    world: &RuntimeWorldV2,
    schema: SchemaId,
    field_name: &str,
) -> Result<FieldLocation, ReferenceExecutionError> {
    let schema_index = world
        .schema_index(schema)
        .ok_or(RuntimeV2Error::UnknownSchema(schema))?;
    let descriptor = world.schema(schema_index)?;
    let field = descriptor
        .fields()
        .iter()
        .find(|field| field.name() == field_name)
        .ok_or_else(|| {
            invalid_core(format!(
                "linked schema {} has no field `{field_name}`",
                descriptor.id()
            ))
        })?;
    let offset = as_usize(field.byte_offset(), "field byte offset")?;
    let width = primitive_width(field.primitive());
    let end = offset
        .checked_add(width)
        .ok_or(ReferenceExecutionError::AddressSpaceOverflow(
            "field byte range",
        ))?;
    let schema_size = as_usize(descriptor.byte_size(), "schema byte size")?;
    if end > schema_size {
        return Err(invalid_core(format!(
            "linked field `{field_name}` lies outside schema {}",
            descriptor.id()
        )));
    }
    Ok(FieldLocation {
        primitive: field.primitive(),
        offset,
        end,
    })
}

fn read_resource_field(
    world: &RuntimeWorldV2,
    schema: SchemaId,
    field_name: &str,
) -> Result<ScalarValue, ReferenceExecutionError> {
    let location = field_location(world, schema, field_name)?;
    let resource = world.resource(schema)?;
    let ResourceState::Initialized(bytes) = resource.state() else {
        return Err(RuntimeV2Error::ResourceUninitialized(schema).into());
    };
    decode_scalar(location, bytes)
}

fn read_row_field(
    world: &RuntimeWorldV2,
    spawn_ordinal: u64,
    schema: SchemaId,
    field_name: &str,
) -> Result<ScalarValue, ReferenceExecutionError> {
    let location = field_location(world, schema, field_name)?;
    let row = world
        .row(spawn_ordinal)
        .ok_or(RuntimeV2Error::UnknownSpawnOrdinal(spawn_ordinal))?;
    let column = row.column(schema).ok_or(RuntimeV2Error::SchemaNotInRow {
        spawn_ordinal,
        schema,
    })?;
    decode_scalar(location, column.bytes())
}

fn write_resource_field(
    world: &mut RuntimeWorldV2,
    schema: SchemaId,
    field_name: &str,
    value: ScalarValue,
) -> Result<(), ReferenceExecutionError> {
    let location = field_location(world, schema, field_name)?;
    let bytes = {
        let resource = world.resource(schema)?;
        let ResourceState::Initialized(bytes) = resource.state() else {
            return Err(RuntimeV2Error::ResourceUninitialized(schema).into());
        };
        let mut bytes = try_copy_bytes(bytes, "resource field assignment")?;
        encode_scalar(location, &mut bytes, value)?;
        bytes
    };
    world.assign_resource(schema, &bytes)?;
    Ok(())
}

fn write_row_field(
    world: &mut RuntimeWorldV2,
    spawn_ordinal: u64,
    schema: SchemaId,
    field_name: &str,
    value: ScalarValue,
) -> Result<(), ReferenceExecutionError> {
    let location = field_location(world, schema, field_name)?;
    let bytes = {
        let row = world
            .row(spawn_ordinal)
            .ok_or(RuntimeV2Error::UnknownSpawnOrdinal(spawn_ordinal))?;
        let column = row.column(schema).ok_or(RuntimeV2Error::SchemaNotInRow {
            spawn_ordinal,
            schema,
        })?;
        let mut bytes = try_copy_bytes(column.bytes(), "component field assignment")?;
        encode_scalar(location, &mut bytes, value)?;
        bytes
    };
    world.assign_row(
        spawn_ordinal,
        &[SchemaPayload {
            schema_id: schema,
            bytes: &bytes,
        }],
    )?;
    Ok(())
}

fn decode_scalar(
    location: FieldLocation,
    bytes: &[u8],
) -> Result<ScalarValue, ReferenceExecutionError> {
    let field = bytes
        .get(location.offset..location.end)
        .ok_or_else(|| invalid_core("linked field byte range lies outside its runtime payload"))?;
    match location.primitive {
        PrimitiveType::I32 => Ok(ScalarValue::I32(i32::from_le_bytes(
            field
                .try_into()
                .expect("validated i32 fields are four bytes"),
        ))),
        PrimitiveType::F32 => Ok(ScalarValue::F32Bits(u32::from_le_bytes(
            field
                .try_into()
                .expect("validated f32 fields are four bytes"),
        ))),
        PrimitiveType::Bool => match field[0] {
            0 => Ok(ScalarValue::Bool(false)),
            1 => Ok(ScalarValue::Bool(true)),
            _ => Err(invalid_core(
                "linked runtime payload contains a noncanonical bool",
            )),
        },
    }
}

fn encode_scalar(
    location: FieldLocation,
    bytes: &mut [u8],
    value: ScalarValue,
) -> Result<(), ReferenceExecutionError> {
    let field = bytes
        .get_mut(location.offset..location.end)
        .ok_or_else(|| invalid_core("linked field byte range lies outside its runtime payload"))?;
    match (location.primitive, value) {
        (PrimitiveType::I32, ScalarValue::I32(value)) => {
            field.copy_from_slice(&value.to_le_bytes());
        }
        (PrimitiveType::F32, ScalarValue::F32Bits(bits)) => {
            field.copy_from_slice(&bits.to_le_bytes());
        }
        (PrimitiveType::Bool, ScalarValue::Bool(value)) => {
            field[0] = u8::from(value);
        }
        _ => {
            return Err(invalid_core(
                "verified field assignment changes scalar type",
            ))
        }
    }
    Ok(())
}

fn primitive_width(primitive: PrimitiveType) -> usize {
    match primitive {
        PrimitiveType::I32 | PrimitiveType::F32 => 4,
        PrimitiveType::Bool => 1,
    }
}

fn evaluate_unary(
    op: CoreSystemUnaryOp,
    operand: ScalarValue,
) -> Result<ScalarValue, ReferenceExecutionError> {
    match op {
        CoreSystemUnaryOp::I32Negate => Ok(ScalarValue::I32(scalar_v2::i32_negate(expect_i32(
            operand,
        )?))),
        CoreSystemUnaryOp::F32Negate => Ok(ScalarValue::F32Bits(scalar_v2::f32_negate(
            expect_f32(operand)?,
        ))),
        CoreSystemUnaryOp::I32BitNot => Ok(ScalarValue::I32(scalar_v2::i32_bit_not(expect_i32(
            operand,
        )?))),
        CoreSystemUnaryOp::BoolNot => Ok(ScalarValue::Bool(!expect_bool(operand)?)),
    }
}

enum BinaryEvaluationError {
    Ordinary(ReferenceExecutionError),
    Trap(TrapKind),
}

impl From<ReferenceExecutionError> for BinaryEvaluationError {
    fn from(error: ReferenceExecutionError) -> Self {
        Self::Ordinary(error)
    }
}

fn evaluate_binary(
    op: CoreSystemBinaryOp,
    left: ScalarValue,
    right: ScalarValue,
) -> Result<ScalarValue, BinaryEvaluationError> {
    match op {
        CoreSystemBinaryOp::I32Add
        | CoreSystemBinaryOp::I32Subtract
        | CoreSystemBinaryOp::I32Multiply
        | CoreSystemBinaryOp::I32Divide
        | CoreSystemBinaryOp::I32Remainder
        | CoreSystemBinaryOp::I32ShiftLeft
        | CoreSystemBinaryOp::I32ShiftRight
        | CoreSystemBinaryOp::I32BitAnd
        | CoreSystemBinaryOp::I32BitXor
        | CoreSystemBinaryOp::I32BitOr => {
            let op = match op {
                CoreSystemBinaryOp::I32Add => I32BinaryOp::Add,
                CoreSystemBinaryOp::I32Subtract => I32BinaryOp::Subtract,
                CoreSystemBinaryOp::I32Multiply => I32BinaryOp::Multiply,
                CoreSystemBinaryOp::I32Divide => I32BinaryOp::Divide,
                CoreSystemBinaryOp::I32Remainder => I32BinaryOp::Remainder,
                CoreSystemBinaryOp::I32ShiftLeft => I32BinaryOp::ShiftLeft,
                CoreSystemBinaryOp::I32ShiftRight => I32BinaryOp::ShiftRight,
                CoreSystemBinaryOp::I32BitAnd => I32BinaryOp::BitAnd,
                CoreSystemBinaryOp::I32BitXor => I32BinaryOp::BitXor,
                CoreSystemBinaryOp::I32BitOr => I32BinaryOp::BitOr,
                _ => unreachable!("outer match selected an i32 operation"),
            };
            let value = scalar_v2::i32_binary(op, expect_i32(left)?, expect_i32(right)?)
                .map_err(BinaryEvaluationError::Trap)?;
            Ok(ScalarValue::I32(value))
        }
        CoreSystemBinaryOp::F32Add
        | CoreSystemBinaryOp::F32Subtract
        | CoreSystemBinaryOp::F32Multiply
        | CoreSystemBinaryOp::F32Divide => {
            let op = match op {
                CoreSystemBinaryOp::F32Add => F32BinaryOp::Add,
                CoreSystemBinaryOp::F32Subtract => F32BinaryOp::Subtract,
                CoreSystemBinaryOp::F32Multiply => F32BinaryOp::Multiply,
                CoreSystemBinaryOp::F32Divide => F32BinaryOp::Divide,
                _ => unreachable!("outer match selected an f32 operation"),
            };
            Ok(ScalarValue::F32Bits(scalar_v2::f32_binary(
                op,
                expect_f32(left)?,
                expect_f32(right)?,
            )))
        }
        CoreSystemBinaryOp::I32Less
        | CoreSystemBinaryOp::I32LessEqual
        | CoreSystemBinaryOp::I32Greater
        | CoreSystemBinaryOp::I32GreaterEqual => Ok(ScalarValue::Bool(scalar_v2::i32_compare(
            system_comparison(op)?,
            expect_i32(left)?,
            expect_i32(right)?,
        ))),
        CoreSystemBinaryOp::F32Less
        | CoreSystemBinaryOp::F32LessEqual
        | CoreSystemBinaryOp::F32Greater
        | CoreSystemBinaryOp::F32GreaterEqual => Ok(ScalarValue::Bool(scalar_v2::f32_compare(
            system_comparison(op)?,
            expect_f32(left)?,
            expect_f32(right)?,
        ))),
        CoreSystemBinaryOp::Equal | CoreSystemBinaryOp::NotEqual => {
            let comparison = if op == CoreSystemBinaryOp::Equal {
                ComparisonOp::Equal
            } else {
                ComparisonOp::NotEqual
            };
            Ok(ScalarValue::Bool(compare_equal(comparison, left, right)?))
        }
        CoreSystemBinaryOp::LogicalAnd | CoreSystemBinaryOp::LogicalOr => {
            Err(invalid_core("logical operation bypassed short-circuit evaluation").into())
        }
    }
}

fn add_values(
    left: ScalarValue,
    right: ScalarValue,
) -> Result<ScalarValue, ReferenceExecutionError> {
    match (left, right) {
        (ScalarValue::I32(left), ScalarValue::I32(right)) => Ok(ScalarValue::I32(
            scalar_v2::i32_binary(I32BinaryOp::Add, left, right).expect("i32 addition cannot trap"),
        )),
        (ScalarValue::F32Bits(left), ScalarValue::F32Bits(right)) => Ok(ScalarValue::F32Bits(
            scalar_v2::f32_binary(F32BinaryOp::Add, left, right),
        )),
        _ => Err(invalid_core(
            "verified compound addition has incompatible scalar operands",
        )),
    }
}

fn compare_equal(
    comparison: ComparisonOp,
    left: ScalarValue,
    right: ScalarValue,
) -> Result<bool, ReferenceExecutionError> {
    match (left, right) {
        (ScalarValue::I32(left), ScalarValue::I32(right)) => {
            Ok(scalar_v2::i32_compare(comparison, left, right))
        }
        (ScalarValue::F32Bits(left), ScalarValue::F32Bits(right)) => {
            Ok(scalar_v2::f32_compare(comparison, left, right))
        }
        (ScalarValue::Bool(left), ScalarValue::Bool(right)) => {
            scalar_v2::bool_compare(comparison, left, right)
                .ok_or_else(|| invalid_core("verified bool expression uses an ordered comparison"))
        }
        _ => Err(invalid_core(
            "verified equality has incompatible scalar operands",
        )),
    }
}

fn equal_values(
    operand_type: CoreType,
    left: ScalarValue,
    right: ScalarValue,
) -> Result<bool, ReferenceExecutionError> {
    match operand_type {
        CoreType::I32 => Ok(scalar_v2::i32_compare(
            ComparisonOp::Equal,
            expect_i32(left)?,
            expect_i32(right)?,
        )),
        CoreType::F32 => Ok(scalar_v2::f32_compare(
            ComparisonOp::Equal,
            expect_f32(left)?,
            expect_f32(right)?,
        )),
        CoreType::Bool => {
            scalar_v2::bool_compare(ComparisonOp::Equal, expect_bool(left)?, expect_bool(right)?)
                .ok_or_else(|| invalid_core("bool equality was not accepted"))
        }
    }
}

fn i32_binary_op(op: CoreBinaryOp) -> I32BinaryOp {
    match op {
        CoreBinaryOp::Add => I32BinaryOp::Add,
        CoreBinaryOp::Subtract => I32BinaryOp::Subtract,
        CoreBinaryOp::Multiply => I32BinaryOp::Multiply,
        CoreBinaryOp::Divide => I32BinaryOp::Divide,
        CoreBinaryOp::Remainder => I32BinaryOp::Remainder,
        CoreBinaryOp::ShiftLeft => I32BinaryOp::ShiftLeft,
        CoreBinaryOp::ShiftRight => I32BinaryOp::ShiftRight,
        CoreBinaryOp::BitAnd => I32BinaryOp::BitAnd,
        CoreBinaryOp::BitXor => I32BinaryOp::BitXor,
        CoreBinaryOp::BitOr => I32BinaryOp::BitOr,
    }
}

fn f32_binary_op(op: CoreBinaryOp) -> Result<F32BinaryOp, ReferenceExecutionError> {
    match op {
        CoreBinaryOp::Add => Ok(F32BinaryOp::Add),
        CoreBinaryOp::Subtract => Ok(F32BinaryOp::Subtract),
        CoreBinaryOp::Multiply => Ok(F32BinaryOp::Multiply),
        CoreBinaryOp::Divide => Ok(F32BinaryOp::Divide),
        CoreBinaryOp::Remainder
        | CoreBinaryOp::ShiftLeft
        | CoreBinaryOp::ShiftRight
        | CoreBinaryOp::BitAnd
        | CoreBinaryOp::BitXor
        | CoreBinaryOp::BitOr => Err(invalid_core(
            "verified startup applies an integer-only binary operation to f32",
        )),
    }
}

fn comparison_op(op: CoreComparisonOp) -> ComparisonOp {
    match op {
        CoreComparisonOp::Less => ComparisonOp::Less,
        CoreComparisonOp::LessEqual => ComparisonOp::LessEqual,
        CoreComparisonOp::Greater => ComparisonOp::Greater,
        CoreComparisonOp::GreaterEqual => ComparisonOp::GreaterEqual,
    }
}

fn system_comparison(op: CoreSystemBinaryOp) -> Result<ComparisonOp, ReferenceExecutionError> {
    match op {
        CoreSystemBinaryOp::I32Less | CoreSystemBinaryOp::F32Less => Ok(ComparisonOp::Less),
        CoreSystemBinaryOp::I32LessEqual | CoreSystemBinaryOp::F32LessEqual => {
            Ok(ComparisonOp::LessEqual)
        }
        CoreSystemBinaryOp::I32Greater | CoreSystemBinaryOp::F32Greater => {
            Ok(ComparisonOp::Greater)
        }
        CoreSystemBinaryOp::I32GreaterEqual | CoreSystemBinaryOp::F32GreaterEqual => {
            Ok(ComparisonOp::GreaterEqual)
        }
        _ => Err(invalid_core(
            "non-comparison Core operation requested a comparison opcode",
        )),
    }
}

fn startup_function(
    core: &VerifiedExecutableCore,
) -> Result<&CoreFunction, ReferenceExecutionError> {
    core.program()
        .functions
        .iter()
        .find(|function| function.name == "startup")
        .ok_or_else(|| invalid_core("verified executable Core has no startup function"))
}

fn insert_value(
    values: &mut HashMap<ValueId, ScalarValue>,
    id: ValueId,
    value: ScalarValue,
) -> Result<(), ReferenceExecutionError> {
    if values.insert(id, value).is_some() {
        return Err(invalid_core(format!(
            "verified startup defines value {} more than once",
            id.0
        )));
    }
    Ok(())
}

fn value(
    values: &HashMap<ValueId, ScalarValue>,
    id: ValueId,
) -> Result<ScalarValue, ReferenceExecutionError> {
    values
        .get(&id)
        .copied()
        .ok_or_else(|| invalid_core(format!("verified startup reads undefined value {}", id.0)))
}

fn expect_i32(value: ScalarValue) -> Result<i32, ReferenceExecutionError> {
    match value {
        ScalarValue::I32(value) => Ok(value),
        ScalarValue::F32Bits(_) | ScalarValue::Bool(_) => {
            Err(invalid_core("verified scalar value is not i32"))
        }
    }
}

fn expect_f32(value: ScalarValue) -> Result<u32, ReferenceExecutionError> {
    match value {
        ScalarValue::F32Bits(bits) => Ok(bits),
        ScalarValue::I32(_) | ScalarValue::Bool(_) => {
            Err(invalid_core("verified scalar value is not f32"))
        }
    }
}

fn expect_bool(value: ScalarValue) -> Result<bool, ReferenceExecutionError> {
    match value {
        ScalarValue::Bool(value) => Ok(value),
        ScalarValue::I32(_) | ScalarValue::F32Bits(_) => {
            Err(invalid_core("verified scalar value is not bool"))
        }
    }
}

fn scalar_type(value: ScalarValue) -> CoreType {
    match value {
        ScalarValue::I32(_) => CoreType::I32,
        ScalarValue::F32Bits(_) => CoreType::F32,
        ScalarValue::Bool(_) => CoreType::Bool,
    }
}

fn reserve_vec<T>(
    values: &mut Vec<T>,
    additional: usize,
    context: &'static str,
) -> Result<(), ReferenceExecutionError> {
    values
        .try_reserve_exact(additional)
        .map_err(|_| ReferenceExecutionError::Allocation(context))
}

fn reserve_map<K: Eq + Hash, V>(
    values: &mut HashMap<K, V>,
    additional: usize,
    context: &'static str,
) -> Result<(), ReferenceExecutionError> {
    values
        .try_reserve(additional)
        .map_err(|_| ReferenceExecutionError::Allocation(context))
}

fn try_copy_bytes(bytes: &[u8], context: &'static str) -> Result<Vec<u8>, ReferenceExecutionError> {
    let mut copied = Vec::new();
    reserve_vec(&mut copied, bytes.len(), context)?;
    copied.extend_from_slice(bytes);
    Ok(copied)
}

fn try_clone_string(value: &str, context: &'static str) -> Result<String, ReferenceExecutionError> {
    let mut cloned = String::new();
    cloned
        .try_reserve_exact(value.len())
        .map_err(|_| ReferenceExecutionError::Allocation(context))?;
    cloned.push_str(value);
    Ok(cloned)
}

fn as_u64(value: usize, context: &'static str) -> Result<u64, ReferenceExecutionError> {
    u64::try_from(value).map_err(|_| ReferenceExecutionError::AddressSpaceOverflow(context))
}

fn as_usize(value: u64, context: &'static str) -> Result<usize, ReferenceExecutionError> {
    usize::try_from(value).map_err(|_| ReferenceExecutionError::AddressSpaceOverflow(context))
}

fn invalid_core(message: impl Into<String>) -> ReferenceExecutionError {
    ReferenceExecutionError::InvalidVerifiedCore(message.into())
}

fn invalid_step(message: impl Into<String>) -> StepError {
    StepError::Ordinary(invalid_core(message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use archec0::execution_package_v2::{write_package, StartupOperationKind};
    use std::io::{BufReader, Cursor};

    use crate::execution_package_build::{
        build_execution_package, NativeCodeLayout, NativeFunctionLayout, NativeFunctionTarget,
    };

    const FULL_SOURCE: &str = "world Demo
tag Enemy
component Empty {}
component Hidden {}
resource Ready { enabled: bool }
system Find(state: mut Ready, units: query[Enemy, !Hidden]) {
  for (_) in units { state.enabled = !state.enabled }
}
schedule Main { run Find }
startup {
  resource Ready { enabled: true }
  spawn {}
  spawn { Enemy {} Empty {} }
  run Main
  exit 47
}";

    const TRAP_SOURCE: &str = "world Demo
component Item { value: i32 }
resource Count { value: i32 }
system Crash(count: mut Count, items: query[mut Item]) {
  for (item) in items {
    count.value = count.value + 1
    item.value = 9
    item.value = 1 / 0
  }
}
schedule Main { run Crash }
startup {
  resource Count { value: 0 }
  spawn { Item { value: 1 } }
  spawn { Item { value: 2 } }
  run Main
  exit 0
}";

    #[derive(Default)]
    struct FlushFailingWriter {
        bytes: Vec<u8>,
    }

    impl Write for FlushFailingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("injected observation flush failure"))
        }
    }

    fn verified_source(source: &str) -> VerifiedExecutableCore {
        let reader = BufReader::new(Cursor::new(source.as_bytes()));
        let program =
            crate::parser::parse_lexer(crate::lexer::Lexer::new(reader)).expect("fixture parses");
        crate::checker::check_program(&program).expect("fixture checks");
        let core = crate::core_lower::lower_program_to_core(&program).expect("fixture lowers");
        crate::core_verify::verify_executable_core(core).expect("fixture Core verifies")
    }

    fn native_layout(core: &VerifiedExecutableCore) -> NativeCodeLayout {
        let ids = canonical_core_ids(core).expect("canonical IDs derive");
        let mut systems = core
            .program()
            .systems
            .iter()
            .map(|system| {
                (
                    ids.system(system.id).expect("system ID derives"),
                    system.name.as_str(),
                )
            })
            .collect::<Vec<_>>();
        systems.sort_unstable_by_key(|(id, _)| *id);

        let function_count = systems.len().checked_add(1).expect("fixture count fits");
        let byte_len = u64::try_from(function_count)
            .expect("fixture count fits u64")
            .checked_mul(0x20)
            .expect("fixture code range fits");
        let mut functions = Vec::with_capacity(function_count);
        functions.push(NativeFunctionLayout {
            target: NativeFunctionTarget::Startup,
            symbol_name: "arche_startup".to_string(),
            code_offset: 0x1000,
            code_byte_len: 0x20,
        });
        for (index, (id, name)) in systems.into_iter().enumerate() {
            functions.push(NativeFunctionLayout {
                target: NativeFunctionTarget::System(id),
                symbol_name: format!("arche_system_{name}"),
                code_offset: 0x1020 + u64::try_from(index).expect("fixture index fits u64") * 0x20,
                code_byte_len: 0x20,
            });
        }
        NativeCodeLayout {
            code_range: CodeImageRange {
                offset: 0x1000,
                byte_len,
            },
            functions,
        }
    }

    fn built_package(
        source: &str,
        file_name: &str,
    ) -> (VerifiedExecutableCore, NativeCodeLayout, ExecutionPackage) {
        let core = verified_source(source);
        let native = native_layout(&core);
        let package =
            build_execution_package(&core, file_name, &native).expect("execution package builds");
        (core, native, package)
    }

    fn execute_encoded(
        core: &VerifiedExecutableCore,
        native: &NativeCodeLayout,
        package: &ExecutionPackage,
    ) -> (ReferenceExecutionOutcome, String, String) {
        let mut encoded = Cursor::new(Vec::new());
        write_package(&mut encoded, package).expect("package streams");
        let encoded = encoded.into_inner();
        let mut reader = Cursor::new(encoded);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let outcome = execute_from_with_code_range(
            core,
            &mut reader,
            native.code_range,
            &mut stdout,
            &mut stderr,
        )
        .expect("reference execution succeeds");
        (
            outcome,
            String::from_utf8(stdout).expect("observation is ASCII"),
            String::from_utf8(stderr).expect("diagnostic is ASCII"),
        )
    }

    fn schema_id(core: &VerifiedExecutableCore, qualified_name: &str) -> SchemaId {
        let ids = canonical_core_ids(core).expect("canonical IDs derive");
        let legacy = core
            .program()
            .components
            .iter()
            .map(|component| (component.id, component.name.as_str()))
            .chain(
                core.program()
                    .resources
                    .iter()
                    .map(|resource| (resource.id, resource.name.as_str())),
            )
            .find(|(_, name)| *name == qualified_name)
            .map(|(id, _)| id)
            .expect("fixture schema exists");
        ids.schema(legacy).expect("schema ID derives")
    }

    #[test]
    fn metadata_payload_and_startup_order_change_behavior_without_core_shape_recognition() {
        let (core, native, package) = built_package(FULL_SOURCE, "full.arc");
        let ready = schema_id(&core, "Demo.Ready");
        let (original_outcome, original_stdout, original_stderr) =
            execute_encoded(&core, &native, &package);
        assert_eq!(
            original_outcome,
            ReferenceExecutionOutcome::Exited { status: 47 }
        );
        assert!(original_stderr.is_empty());
        assert!(original_stdout.contains(&format!("RESOURCE {ready} INITIALIZED 1 00\n")));
        assert!(original_stdout.contains("TABLE 0 1\nROW 0 0 0\n"));

        let mut payload_edited = package.clone();
        let ready_payload = payload_edited
            .startup_operations
            .iter()
            .find_map(|operation| match operation.kind {
                StartupOperationKind::ResourcePayload { resource, payload }
                    if payload_edited.schemas[resource.index() as usize].id == ready =>
                {
                    Some(payload)
                }
                StartupOperationKind::ResourcePayload { .. }
                | StartupOperationKind::Spawn { .. }
                | StartupOperationKind::RunSchedule { .. } => None,
            })
            .expect("Ready startup payload exists");
        payload_edited.payloads[ready_payload.index() as usize].bytes[0] = 0;
        let (payload_outcome, payload_stdout, payload_stderr) =
            execute_encoded(&core, &native, &payload_edited);
        assert_eq!(payload_outcome, original_outcome);
        assert!(payload_stderr.is_empty());
        assert!(payload_stdout.contains(&format!("RESOURCE {ready} INITIALIZED 1 01\n")));
        assert_ne!(payload_stdout, original_stdout);

        let mut reordered = package.clone();
        let tagged_spawn = reordered
            .startup_operations
            .iter()
            .position(|operation| {
                matches!(
                    operation.kind,
                    StartupOperationKind::Spawn {
                        payload_count: 2,
                        ..
                    }
                )
            })
            .expect("tagged spawn exists");
        let schedule = reordered
            .startup_operations
            .iter()
            .position(|operation| {
                matches!(operation.kind, StartupOperationKind::RunSchedule { .. })
            })
            .expect("schedule run exists");
        reordered.startup_operations.swap(tagged_spawn, schedule);

        let (reordered_outcome, reordered_stdout, reordered_stderr) =
            execute_encoded(&core, &native, &reordered);
        assert_eq!(reordered_outcome, original_outcome);
        assert!(reordered_stderr.is_empty());
        assert!(reordered_stdout.contains(&format!("RESOURCE {ready} INITIALIZED 1 01\n")));
        assert_ne!(reordered_stdout, original_stdout);
    }

    #[test]
    fn normal_exit_propagates_observation_flush_failure() {
        let (core, native, package) =
            built_package("world FlushFailure startup { exit 9 }", "flush_failure.arc");
        let mut stdout = FlushFailingWriter::default();
        let mut stderr = Vec::new();

        let error = execute_decoded(
            &core,
            package,
            Some(native.code_range),
            &mut stdout,
            &mut stderr,
        )
        .expect_err("a failed observation flush must reject the source exit");

        let ReferenceExecutionError::Observation(error) = error else {
            panic!("expected observation I/O failure, got {error}");
        };
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(error.to_string(), "injected observation flush failure");
        assert!(stdout.bytes.starts_with(b"ARCHEOBS2\n"));
        assert!(stdout.bytes.ends_with(b"END\n"));
        assert!(stderr.is_empty());
    }

    #[test]
    fn trap_preserves_committed_effects_and_reports_exact_linked_span() {
        let (core, native, package) = built_package(TRAP_SOURCE, "trap.arc");
        let count = schema_id(&core, "Demo.Count");
        let item = schema_id(&core, "Demo.Item");
        let (outcome, stdout, stderr) = execute_encoded(&core, &native, &package);

        assert_eq!(
            outcome,
            ReferenceExecutionOutcome::Trapped {
                kind: TrapKind::I32DivideByZero
            }
        );
        assert_eq!(outcome.process_status(), 70);
        assert!(stdout.starts_with("ARCHEOBS2\n"));
        assert!(stdout.ends_with("END\n"));
        assert!(stdout.contains(&format!("RESOURCE {count} INITIALIZED 4 01000000\n")));
        assert!(stdout.contains(&format!("COLUMN {item} 4 09000000\n")));
        assert!(stdout.contains(&format!("COLUMN {item} 4 02000000\n")));

        let expression = "1 / 0";
        let start = TRAP_SOURCE
            .find(expression)
            .expect("trap expression exists");
        let prefix = &TRAP_SOURCE[..start];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix
            .rfind('\n')
            .map_or(start + 1, |line_start| start - line_start);
        assert_eq!(
            stderr,
            format!(
                "arche: trap[I32_DIVIDE_BY_ZERO] trap.arc:{line}:{column} bytes {start}..{}\n",
                start + expression.len()
            )
        );
    }

    #[test]
    fn logical_short_circuit_skips_a_trapping_rhs() {
        let source = "world Demo
resource Ready { enabled: bool }
system Safe(state: mut Ready) {
  if false && (1 / 0 == 0) { state.enabled = false } else { state.enabled = true }
}
schedule Main { run Safe }
startup {
  resource Ready { enabled: false }
  run Main
  exit 5
}";
        let (core, native, package) = built_package(source, "short.arc");
        let ready = schema_id(&core, "Demo.Ready");
        let (outcome, stdout, stderr) = execute_encoded(&core, &native, &package);

        assert_eq!(outcome, ReferenceExecutionOutcome::Exited { status: 5 });
        assert!(stderr.is_empty());
        assert!(stdout.contains(&format!("RESOURCE {ready} INITIALIZED 1 01\n")));
    }

    #[test]
    fn exclusion_only_query_executes_with_zero_core_bindings() {
        let source = "world ExclusionOnly
component Hidden {}
resource Count { value: i32 }
system CountVisible(count: mut Count, visible: query[!Hidden]) {
  for () in visible { count.value += 1 }
}
schedule Main { run CountVisible }
startup {
  resource Count { value: 0 }
  spawn {}
  spawn {}
  spawn { Hidden {} }
  run Main
  exit 0
}";
        let (core, native, package) = built_package(source, "exclusion_only.arc");
        let count = schema_id(&core, "ExclusionOnly.Count");
        let (outcome, stdout, stderr) = execute_encoded(&core, &native, &package);

        assert_eq!(outcome, ReferenceExecutionOutcome::Exited { status: 0 });
        assert!(stderr.is_empty());
        assert!(stdout.contains(&format!("RESOURCE {count} INITIALIZED 4 02000000\n")));
    }

    #[test]
    fn startup_add_assign_initializes_typed_resource_payloads() {
        let source = "world StartupAdd
resource Result { integer: i32 scalar: f32 }
startup {
  let mut integer: i32 = 2147483647
  integer += 2
  let mut scalar: f32 = 0.5
  scalar += 0.25
  resource Result { integer: integer, scalar: scalar }
  exit integer
}";
        let (core, native, package) = built_package(source, "startup_add.arc");
        let result = schema_id(&core, "StartupAdd.Result");
        let (outcome, stdout, stderr) = execute_encoded(&core, &native, &package);

        assert_eq!(outcome, ReferenceExecutionOutcome::Exited { status: 1 });
        assert!(stderr.is_empty());
        assert!(stdout.contains(&format!(
            "RESOURCE {result} INITIALIZED 8 010000800000403F\n"
        )));
    }

    #[test]
    fn source_exit_uses_the_low_eight_bits_without_a_trap_diagnostic() {
        for (name, expression, expected_status) in [
            ("zero", "0", 0),
            ("source_seventy", "70", 70),
            ("positive_wrap", "256", 0),
            ("negative_one", "-1", 255),
            ("wrapped_maximum", "511", 255),
        ] {
            let source = format!("world Exit{name} startup {{ exit {expression} }}");
            let file_name = format!("exit_{name}.arc");
            let (core, native, package) = built_package(&source, &file_name);
            let (outcome, stdout, stderr) = execute_encoded(&core, &native, &package);

            assert_eq!(
                outcome,
                ReferenceExecutionOutcome::Exited {
                    status: expected_status
                },
                "wrong low-byte exit for {name}"
            );
            assert!(stdout.starts_with("ARCHEOBS2\n"));
            assert!(stdout.ends_with("END\n"));
            assert!(stderr.is_empty(), "source exit {name} looked like a trap");
        }
    }

    #[test]
    fn every_integer_trap_edge_preserves_prior_commits_and_skips_the_trapping_spawn() {
        for (name, expression, expected_kind) in [
            ("divide_zero", "1 / 0", TrapKind::I32DivideByZero),
            ("remainder_zero", "1 % 0", TrapKind::I32RemainderByZero),
            (
                "divide_overflow",
                "-2147483648 / -1",
                TrapKind::I32DivideOverflow,
            ),
            (
                "remainder_overflow",
                "-2147483648 % -1",
                TrapKind::I32RemainderOverflow,
            ),
        ] {
            let source = format!(
                "world Trap{name}\ncomponent Item {{ value: i32 }}\nstartup {{\n  spawn {{ Item {{ value: 7 }} }}\n  spawn {{ Item {{ value: {expression} }} }}\n  exit 0\n}}"
            );
            let file_name = format!("trap_{name}.arc");
            let (core, native, package) = built_package(&source, &file_name);
            let item = schema_id(&core, &format!("Trap{name}.Item"));
            let (outcome, stdout, stderr) = execute_encoded(&core, &native, &package);

            assert_eq!(
                outcome,
                ReferenceExecutionOutcome::Trapped {
                    kind: expected_kind
                },
                "wrong trap kind for {name}"
            );
            assert_eq!(outcome.process_status(), 70);
            assert!(stdout.starts_with("ARCHEOBS2\n"));
            assert!(stdout.ends_with("END\n"));
            assert_eq!(stdout.matches("ROW ").count(), 1);
            assert!(stdout.contains(&format!("COLUMN {item} 4 07000000\n")));
            let start = source.rfind(expression).expect("trap expression exists");
            assert!(stderr.contains(&format!(
                "trap[{}] {file_name}:",
                expected_kind.diagnostic_name()
            )));
            assert!(stderr.ends_with(&format!(" bytes {start}..{}\n", start + expression.len())));
        }
    }

    #[test]
    fn executes_the_primary_m26_closure_fixture_from_decoded_v2_metadata() {
        let source = include_str!("../../../examples/m26_closure.arc");
        let (core, native, package) = built_package(source, "m26_closure.arc");
        let empty_pending = schema_id(&core, "M26Closure.EmptyPending");
        let scratch = schema_id(&core, "M26Closure.Scratch");
        let (outcome, stdout, stderr) = execute_encoded(&core, &native, &package);

        assert_eq!(outcome, ReferenceExecutionOutcome::Exited { status: 47 });
        assert!(stderr.is_empty());
        assert!(stdout.starts_with("ARCHEOBS2\n"));
        assert!(stdout.ends_with("END\n"));
        assert!(stdout.contains(&format!("RESOURCE {empty_pending} UNINITIALIZED\n")));
        assert!(stdout.contains(&format!("RESOURCE {scratch} UNINITIALIZED\n")));
        assert!(stdout.contains("TABLE 0 1\nROW 0 0 0\n"));
    }

    #[test]
    fn executes_the_external_m26_trap_fixture_with_committed_state() {
        let source = include_str!("../../../examples/m26_trap.arc");
        let (core, native, package) = built_package(source, "m26_trap.arc");
        let counter = schema_id(&core, "M26Trap.Counter");
        let (outcome, stdout, stderr) = execute_encoded(&core, &native, &package);

        assert_eq!(
            outcome,
            ReferenceExecutionOutcome::Trapped {
                kind: TrapKind::I32DivideByZero
            }
        );
        assert!(stdout.starts_with("ARCHEOBS2\n"));
        assert!(stdout.ends_with("END\n"));
        assert!(stdout.contains(&format!("COLUMN {counter} 4 2A000000\n")));
        let expression = "counter.value / denominator.value";
        let start = source.find(expression).expect("trap expression exists");
        let prefix = &source[..start];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix
            .rfind('\n')
            .map_or(start + 1, |line_start| start - line_start);
        assert_eq!(
            stderr,
            format!(
                "arche: trap[I32_DIVIDE_BY_ZERO] m26_trap.arc:{line}:{column} bytes {start}..{}\n",
                start + expression.len()
            )
        );
    }
}
