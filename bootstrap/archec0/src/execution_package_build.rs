use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use archec0::execution_package_v2::{
    validate_package, validate_package_with_code_range, CodeImageRange, ExecutionPackage,
    ExecutionPackageV2Error, FieldRecord, FunctionLinkRecord, FunctionTarget, ParameterKind,
    ParameterRecord, PayloadRecord, PayloadRef, QueryAccess, QueryRecord, QueryRef,
    ScheduleItemKind, ScheduleItemRecord, ScheduleRecord, ScheduleRef, SchemaFlags, SchemaRecord,
    SchemaRef, SourceSpanRecord, SourceSpanRef, StartupOperationKind, StartupOperationRecord,
    StringRef, SystemRecord, SystemRef, TermRecord, WorldRecord,
};
use archec0::ids_v2::{
    AbiHash, AbiHasher, BodyHash, BodyHasher, DeclId, PrimitiveType, SchemaField, SchemaId,
    SchemaKind,
};

use crate::core::{
    CoreBinaryOp, CoreComparisonOp, CoreComponentKind, CoreField, CoreFunction, CoreInstruction,
    CoreLiteralValue, CoreQueryAccess, CoreScheduleItem, CoreSourceSubject, CoreSystem,
    CoreSystemBinaryOp, CoreSystemExpression, CoreSystemParamKind, CoreSystemPlace,
    CoreSystemStatement, CoreSystemUnaryOp, CoreTerminator, CoreType, CoreUnaryOp,
};
use crate::core_verify::VerifiedExecutableCore;

const CANONICAL_CORE_ENCODING_VERSION: u64 = 1;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NativeFunctionTarget {
    Startup,
    System(DeclId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFunctionLayout {
    pub target: NativeFunctionTarget,
    pub symbol_name: String,
    pub code_offset: u64,
    pub code_byte_len: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeCodeLayout {
    pub code_range: CodeImageRange,
    pub functions: Vec<NativeFunctionLayout>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalCoreIds {
    schemas: HashMap<u64, SchemaId>,
    systems: HashMap<u64, DeclId>,
    schedules: HashMap<u64, DeclId>,
}

impl CanonicalCoreIds {
    pub fn schema(&self, legacy_core_id: u64) -> Option<SchemaId> {
        self.schemas.get(&legacy_core_id).copied()
    }

    pub fn system(&self, legacy_core_id: u64) -> Option<DeclId> {
        self.systems.get(&legacy_core_id).copied()
    }

    pub fn schedule(&self, legacy_core_id: u64) -> Option<DeclId> {
        self.schedules.get(&legacy_core_id).copied()
    }

    pub fn query(&self, legacy_system_id: u64, parameter_name: &str) -> Option<DeclId> {
        self.system(legacy_system_id)
            .map(|system| DeclId::query(system, parameter_name))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionPackageBuildError {
    InvalidCore(String),
    InvalidNativeLayout(String),
    ArithmeticOverflow(&'static str),
    AddressSpaceOverflow(&'static str),
    Allocation(String),
    Package(ExecutionPackageV2Error),
}

impl fmt::Display for ExecutionPackageBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCore(message) | Self::InvalidNativeLayout(message) => {
                formatter.write_str(message)
            }
            Self::ArithmeticOverflow(context) => {
                write!(
                    formatter,
                    "u64 arithmetic overflow while building {context}"
                )
            }
            Self::AddressSpaceOverflow(context) => {
                write!(formatter, "{context} does not fit the host address space")
            }
            Self::Allocation(message) => formatter.write_str(message),
            Self::Package(error) => error.fmt(formatter),
        }
    }
}

impl Error for ExecutionPackageBuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Package(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ExecutionPackageV2Error> for ExecutionPackageBuildError {
    fn from(error: ExecutionPackageV2Error) -> Self {
        Self::Package(error)
    }
}

struct SchemaDraft<'a> {
    legacy_id: u64,
    id: SchemaId,
    kind: SchemaKind,
    local_name: &'a str,
    fields: &'a [CoreField],
}

struct SystemDraft<'a> {
    legacy_id: u64,
    id: DeclId,
    system: &'a CoreSystem,
    abi_hash: AbiHash,
    body_hash: BodyHash,
}

struct ScheduleDraft<'a> {
    legacy_id: u64,
    id: DeclId,
    name: &'a str,
    items: &'a [CoreScheduleItem],
}

struct QueryDraft<'a> {
    id: DeclId,
    system_legacy_id: u64,
    parameter_ordinal: usize,
    terms: &'a [crate::core::CoreQueryTerm],
}

struct SourceSpanPlan {
    spans: Vec<crate::lexer::SourceSpan>,
    subjects: HashMap<CoreSourceSubject, SourceSpanRef>,
    body_ranges: HashMap<NativeFunctionTarget, (SourceSpanRef, u64)>,
}

impl SourceSpanPlan {
    fn reference(
        &self,
        subject: &CoreSourceSubject,
    ) -> Result<SourceSpanRef, ExecutionPackageBuildError> {
        self.subjects
            .get(subject)
            .copied()
            .ok_or_else(|| invalid_core(format!("verified Core source map omits {subject:?}")))
    }

    fn body_range(
        &self,
        target: NativeFunctionTarget,
    ) -> Result<(SourceSpanRef, u64), ExecutionPackageBuildError> {
        self.body_ranges.get(&target).copied().ok_or_else(|| {
            invalid_core(format!(
                "verified Core source map omits body range for {target:?}"
            ))
        })
    }

    fn records(
        &self,
        file_name: StringRef,
    ) -> Result<Vec<SourceSpanRecord>, ExecutionPackageBuildError> {
        let mut records = Vec::new();
        reserve_vec(&mut records, self.spans.len(), "source-span records")?;
        for span in &self.spans {
            records.push(SourceSpanRecord {
                file_name,
                start_byte: span.start.byte,
                end_byte: span.end.byte,
                start_line: span.start.line,
                start_column: span.start.column,
                end_line: span.end.line,
                end_column: span.end.column,
            });
        }
        Ok(records)
    }
}

/// Build and fully validate the metadata-authoritative ARCHEECS v2 package.
///
/// `source_file_name` is retained as a separate input because the immutable
/// source identity used for publication deliberately does not expose or
/// re-resolve its original path. Source spans are attached from verified Core.
pub fn build_execution_package(
    core: &VerifiedExecutableCore,
    source_file_name: &str,
    native: &NativeCodeLayout,
) -> Result<ExecutionPackage, ExecutionPackageBuildError> {
    if source_file_name.is_empty() {
        return Err(ExecutionPackageBuildError::InvalidCore(
            "source file name cannot be empty".to_string(),
        ));
    }

    let program = core.program();
    let world_name = program.world.name.as_str();
    let mut schemas = schema_drafts(core)?;
    schemas.sort_unstable_by_key(|schema| schema.id);
    reject_duplicate_canonical_ids(schemas.iter().map(|schema| schema.id), "schema")?;
    let mut schema_refs = HashMap::new();
    reserve_map(&mut schema_refs, schemas.len(), "schema references")?;
    let mut schema_ids = HashMap::new();
    reserve_map(&mut schema_ids, schemas.len(), "schema identifiers")?;
    for (index, schema) in schemas.iter().enumerate() {
        schema_refs.insert(
            schema.legacy_id,
            SchemaRef::new(as_u64(index, "schema index")?),
        );
        schema_ids.insert(schema.legacy_id, schema.id);
    }

    let mut systems = system_drafts(core, &schema_ids)?;
    systems.sort_unstable_by_key(|system| system.id);
    reject_duplicate_canonical_ids(systems.iter().map(|system| system.id), "system")?;
    let mut system_refs = HashMap::new();
    reserve_map(&mut system_refs, systems.len(), "system references")?;
    for (index, system) in systems.iter().enumerate() {
        system_refs.insert(
            system.legacy_id,
            SystemRef::new(as_u64(index, "system index")?),
        );
    }

    let mut schedules = Vec::new();
    reserve_vec(&mut schedules, program.schedules.len(), "schedule drafts")?;
    for schedule in &program.schedules {
        schedules.push(ScheduleDraft {
            legacy_id: schedule.id,
            id: DeclId::schedule(world_name, &schedule.name),
            name: &schedule.name,
            items: &schedule.items,
        });
    }
    schedules.sort_unstable_by_key(|schedule| schedule.id);
    reject_duplicate_canonical_ids(schedules.iter().map(|schedule| schedule.id), "schedule")?;
    let mut schedule_refs = HashMap::new();
    reserve_map(&mut schedule_refs, schedules.len(), "schedule references")?;
    let mut schedule_ids = HashMap::new();
    reserve_map(&mut schedule_ids, schedules.len(), "schedule identifiers")?;
    for (index, schedule) in schedules.iter().enumerate() {
        schedule_refs.insert(
            schedule.legacy_id,
            ScheduleRef::new(as_u64(index, "schedule index")?),
        );
        schedule_ids.insert(schedule.legacy_id, schedule.id);
    }

    let mut query_drafts = Vec::new();
    let mut parameter_indexes = HashMap::new();
    let parameter_count = systems.iter().try_fold(0usize, |count, system| {
        count.checked_add(system.system.params.len()).ok_or(
            ExecutionPackageBuildError::AddressSpaceOverflow("system parameter count"),
        )
    })?;
    reserve_vec(&mut query_drafts, parameter_count, "query drafts")?;
    reserve_map(
        &mut parameter_indexes,
        parameter_count,
        "parameter references",
    )?;
    let mut parameter_cursor = 0u64;
    for system in &systems {
        for (ordinal, parameter) in system.system.params.iter().enumerate() {
            parameter_indexes.insert(
                (system.legacy_id, ordinal),
                archec0::execution_package_v2::ParameterRef::new(parameter_cursor),
            );
            parameter_cursor = parameter_cursor.checked_add(1).ok_or(
                ExecutionPackageBuildError::ArithmeticOverflow("parameter indexes"),
            )?;
            if let CoreSystemParamKind::Query { terms } = &parameter.kind {
                query_drafts.push(QueryDraft {
                    id: DeclId::query(system.id, &parameter.name),
                    system_legacy_id: system.legacy_id,
                    parameter_ordinal: ordinal,
                    terms,
                });
            }
        }
    }
    query_drafts.sort_unstable_by_key(|query| query.id);
    reject_duplicate_canonical_ids(query_drafts.iter().map(|query| query.id), "query")?;
    let mut query_refs = HashMap::new();
    reserve_map(&mut query_refs, query_drafts.len(), "query references")?;
    for (index, query) in query_drafts.iter().enumerate() {
        query_refs.insert(
            (query.system_legacy_id, query.parameter_ordinal),
            QueryRef::new(as_u64(index, "query index")?),
        );
    }

    let function_hashes = function_hashes(core, &systems, &schema_ids, &schedule_ids)?;
    let native_by_target = validate_native_layout(native, &systems)?;
    let source_plan = build_source_span_plan(core, &systems)?;

    let mut string_values = Vec::new();
    let string_value_count = schemas
        .iter()
        .try_fold(2usize, |count, schema| {
            count
                .checked_add(1)
                .and_then(|count| count.checked_add(schema.fields.len()))
                .ok_or(ExecutionPackageBuildError::AddressSpaceOverflow(
                    "string reference count",
                ))
        })?
        .checked_add(systems.len())
        .and_then(|count| count.checked_add(parameter_count))
        .and_then(|count| count.checked_add(schedules.len()))
        .and_then(|count| count.checked_add(native_by_target.len()))
        .ok_or(ExecutionPackageBuildError::AddressSpaceOverflow(
            "string reference count",
        ))?;
    reserve_vec(&mut string_values, string_value_count, "string references")?;
    string_values.push(world_name);
    string_values.push(source_file_name);
    for schema in &schemas {
        string_values.push(schema.local_name);
        string_values.extend(schema.fields.iter().map(|field| field.name.as_str()));
    }
    for system in &systems {
        string_values.push(system.system.name.as_str());
        string_values.extend(system.system.params.iter().map(|param| param.name.as_str()));
    }
    string_values.extend(schedules.iter().map(|schedule| schedule.name));
    string_values.extend(
        native_by_target
            .values()
            .map(|function| function.symbol_name.as_str()),
    );
    let (strings, string_refs) = canonical_strings(string_values)?;
    let source_file = string_ref(&string_refs, source_file_name)?;

    let world = WorldRecord {
        name: string_ref(&string_refs, world_name)?,
        source_span: Some(source_plan.reference(&CoreSourceSubject::World)?),
        startup_abi_hash: function_hashes.startup_abi,
        startup_body_hash: function_hashes.startup_body,
    };

    let mut schema_records = Vec::new();
    reserve_vec(&mut schema_records, schemas.len(), "schema records")?;
    let mut field_records = Vec::new();
    let field_count = schemas.iter().try_fold(0usize, |count, schema| {
        count.checked_add(schema.fields.len()).ok_or(
            ExecutionPackageBuildError::AddressSpaceOverflow("field record count"),
        )
    })?;
    reserve_vec(&mut field_records, field_count, "field records")?;
    for (index, schema) in schemas.iter().enumerate() {
        let schema_ref = SchemaRef::new(as_u64(index, "schema index")?);
        let layout = compute_layout(schema.fields)?;
        schema_records.push(SchemaRecord {
            id: schema.id,
            kind: schema.kind,
            flags: SchemaFlags::for_kind(schema.kind),
            name: string_ref(&string_refs, schema.local_name)?,
            byte_size: layout.byte_size,
            alignment: layout.alignment,
            source_span: Some(source_plan.reference(&schema_source_subject(schema))?),
        });
        for (field_index, (field, byte_offset)) in
            schema.fields.iter().zip(layout.field_offsets).enumerate()
        {
            field_records.push(FieldRecord {
                schema: schema_ref,
                name: string_ref(&string_refs, &field.name)?,
                primitive: primitive(field.ty),
                byte_offset,
                source_span: Some(source_plan.reference(&schema_field_source_subject(
                    schema,
                    as_u64(field_index, "schema field index")?,
                ))?),
            });
        }
    }

    let mut system_records = Vec::new();
    reserve_vec(&mut system_records, systems.len(), "system records")?;
    for system in &systems {
        system_records.push(SystemRecord {
            id: system.id,
            name: string_ref(&string_refs, &system.system.name)?,
            abi_hash: system.abi_hash,
            body_hash: system.body_hash,
            source_span: Some(source_plan.reference(&CoreSourceSubject::System {
                system_id: system.legacy_id,
            })?),
        });
    }

    let mut parameter_records = Vec::new();
    reserve_vec(&mut parameter_records, parameter_count, "parameter records")?;
    for system in &systems {
        let system_ref = required_system_ref(&system_refs, system.legacy_id)?;
        for (ordinal, parameter) in system.system.params.iter().enumerate() {
            let param_index = as_u64(ordinal, "system parameter index")?;
            let kind = match &parameter.kind {
                CoreSystemParamKind::ReadResource { resource_id, .. } => {
                    ParameterKind::ReadResource {
                        resource: required_schema_ref(&schema_refs, *resource_id)?,
                    }
                }
                CoreSystemParamKind::MutResource { resource_id, .. } => {
                    ParameterKind::MutResource {
                        resource: required_schema_ref(&schema_refs, *resource_id)?,
                    }
                }
                CoreSystemParamKind::Query { .. } => ParameterKind::Query {
                    query: *query_refs
                        .get(&(system.legacy_id, ordinal))
                        .ok_or_else(|| {
                            invalid_core(format!(
                                "query parameter `{}.{}` has no canonical query reference",
                                system.system.name, parameter.name
                            ))
                        })?,
                },
            };
            parameter_records.push(ParameterRecord {
                system: system_ref,
                name: string_ref(&string_refs, &parameter.name)?,
                kind,
                source_span: Some(source_plan.reference(&CoreSourceSubject::SystemParam {
                    system_id: system.legacy_id,
                    param_index,
                })?),
            });
        }
    }

    let mut query_records = Vec::new();
    reserve_vec(&mut query_records, query_drafts.len(), "query records")?;
    let mut term_records = Vec::new();
    let term_count = query_drafts.iter().try_fold(0usize, |count, query| {
        count.checked_add(query.terms.len()).ok_or(
            ExecutionPackageBuildError::AddressSpaceOverflow("query term count"),
        )
    })?;
    reserve_vec(&mut term_records, term_count, "query term records")?;
    for (index, query) in query_drafts.iter().enumerate() {
        let query_ref = QueryRef::new(as_u64(index, "query index")?);
        let system = required_system_ref(&system_refs, query.system_legacy_id)?;
        let parameter = *parameter_indexes
            .get(&(query.system_legacy_id, query.parameter_ordinal))
            .ok_or_else(|| invalid_core("query has no parameter reference"))?;
        query_records.push(QueryRecord {
            id: query.id,
            system,
            parameter,
            source_span: Some(source_plan.reference(&CoreSourceSubject::SystemParam {
                system_id: query.system_legacy_id,
                param_index: as_u64(query.parameter_ordinal, "system parameter index")?,
            })?),
        });
        for (term_index, term) in query.terms.iter().enumerate() {
            term_records.push(TermRecord {
                query: query_ref,
                access: query_access(term.access),
                schema: required_schema_ref(&schema_refs, term.component_id)?,
                source_span: Some(source_plan.reference(&CoreSourceSubject::QueryTerm {
                    system_id: query.system_legacy_id,
                    param_index: as_u64(query.parameter_ordinal, "system parameter index")?,
                    term_index: as_u64(term_index, "query term index")?,
                })?),
            });
        }
    }

    let mut schedule_records = Vec::new();
    reserve_vec(&mut schedule_records, schedules.len(), "schedule records")?;
    for schedule in &schedules {
        schedule_records.push(ScheduleRecord {
            id: schedule.id,
            name: string_ref(&string_refs, schedule.name)?,
            source_span: Some(source_plan.reference(&CoreSourceSubject::Schedule {
                schedule_id: schedule.legacy_id,
            })?),
        });
    }
    let mut schedule_items = Vec::new();
    let schedule_item_count = schedules.iter().try_fold(0usize, |count, schedule| {
        count.checked_add(schedule.items.len()).ok_or(
            ExecutionPackageBuildError::AddressSpaceOverflow("schedule item count"),
        )
    })?;
    reserve_vec(
        &mut schedule_items,
        schedule_item_count,
        "schedule item records",
    )?;
    for schedule in &schedules {
        let schedule_ref = required_schedule_ref(&schedule_refs, schedule.legacy_id)?;
        for (item_index, item) in schedule.items.iter().enumerate() {
            let CoreScheduleItem::Run { system_id, .. } = item;
            schedule_items.push(ScheduleItemRecord {
                schedule: schedule_ref,
                kind: ScheduleItemKind::RunSystem {
                    system: required_system_ref(&system_refs, *system_id)?,
                },
                source_span: Some(source_plan.reference(&CoreSourceSubject::ScheduleItem {
                    schedule_id: schedule.legacy_id,
                    item_index: as_u64(item_index, "schedule item index")?,
                })?),
            });
        }
    }

    let (payloads, startup_operations) =
        build_startup(core, &schemas, &schema_refs, &schedule_refs, &source_plan)?;

    let function_count =
        systems
            .len()
            .checked_add(1)
            .ok_or(ExecutionPackageBuildError::AddressSpaceOverflow(
                "function link count",
            ))?;
    let mut function_links = Vec::new();
    reserve_vec(&mut function_links, function_count, "function links")?;
    let startup_layout = native_by_target
        .get(&NativeFunctionTarget::Startup)
        .ok_or_else(|| invalid_native("native layout has no startup function"))?;
    let startup_source = source_plan.reference(&CoreSourceSubject::Startup)?;
    let (startup_body, startup_body_count) =
        source_plan.body_range(NativeFunctionTarget::Startup)?;
    function_links.push(FunctionLinkRecord {
        target: FunctionTarget::Startup,
        symbol_name: string_ref(&string_refs, &startup_layout.symbol_name)?,
        abi_hash: function_hashes.startup_abi,
        body_hash: function_hashes.startup_body,
        code_offset: startup_layout.code_offset,
        code_byte_len: startup_layout.code_byte_len,
        source_span: Some(startup_source),
        first_body_span: Some(startup_body),
        body_span_count: startup_body_count,
    });
    for (index, system) in systems.iter().enumerate() {
        let layout = native_by_target
            .get(&NativeFunctionTarget::System(system.id))
            .ok_or_else(|| {
                invalid_native(format!(
                    "native layout has no function for system `{}`",
                    system.system.name
                ))
            })?;
        let target = NativeFunctionTarget::System(system.id);
        let (first_body_span, body_span_count) = source_plan.body_range(target)?;
        function_links.push(FunctionLinkRecord {
            target: FunctionTarget::System {
                system: SystemRef::new(as_u64(index, "system index")?),
            },
            symbol_name: string_ref(&string_refs, &layout.symbol_name)?,
            abi_hash: system.abi_hash,
            body_hash: system.body_hash,
            code_offset: layout.code_offset,
            code_byte_len: layout.code_byte_len,
            source_span: Some(source_plan.reference(&CoreSourceSubject::System {
                system_id: system.legacy_id,
            })?),
            first_body_span: Some(first_body_span),
            body_span_count,
        });
    }

    let package = ExecutionPackage {
        strings,
        world,
        schemas: schema_records,
        fields: field_records,
        systems: system_records,
        parameters: parameter_records,
        queries: query_records,
        terms: term_records,
        schedules: schedule_records,
        schedule_items,
        startup_operations,
        payloads,
        function_links,
        source_spans: source_plan.records(source_file)?,
    };
    validate_package_with_code_range(&package, native.code_range)?;
    Ok(package)
}

/// Derive the canonical identifiers used to link verified Core to decoded v2
/// records. Legacy Core IDs are lookup keys only and never enter the package.
pub fn canonical_core_ids(
    core: &VerifiedExecutableCore,
) -> Result<CanonicalCoreIds, ExecutionPackageBuildError> {
    let program = core.program();
    let schemas = schema_drafts(core)?;
    let mut schema_ids = HashMap::new();
    reserve_map(&mut schema_ids, schemas.len(), "canonical schema ID map")?;
    for schema in schemas {
        schema_ids.insert(schema.legacy_id, schema.id);
    }
    let mut systems = HashMap::new();
    reserve_map(
        &mut systems,
        program.systems.len(),
        "canonical system ID map",
    )?;
    for system in &program.systems {
        systems.insert(system.id, DeclId::system(&program.world.name, &system.name));
    }
    let mut schedules = HashMap::new();
    reserve_map(
        &mut schedules,
        program.schedules.len(),
        "canonical schedule ID map",
    )?;
    for schedule in &program.schedules {
        schedules.insert(
            schedule.id,
            DeclId::schedule(&program.world.name, &schedule.name),
        );
    }
    Ok(CanonicalCoreIds {
        schemas: schema_ids,
        systems,
        schedules,
    })
}

/// Validate a decoded package against verified Core without comparing the
/// metadata-authoritative startup payloads, startup ordering, or schedule
/// item ordering. This is the pre-mutation link gate used by reference and
/// native execution.
pub fn validate_execution_package_link(
    core: &VerifiedExecutableCore,
    package: &ExecutionPackage,
    code_range: Option<CodeImageRange>,
) -> Result<(), ExecutionPackageBuildError> {
    if let Some(code_range) = code_range {
        validate_package_with_code_range(package, code_range)?;
    } else {
        validate_package(package)?;
    }

    let program = core.program();
    if package_string(package, package.world.name)? != program.world.name.as_str() {
        return Err(link_mismatch("world name does not match verified Core"));
    }
    if package.startup_operations.len() != core.startup_operations().count() {
        return Err(link_mismatch(
            "startup operation count does not match verified Core",
        ));
    }

    let mut schemas = schema_drafts(core)?;
    schemas.sort_unstable_by_key(|schema| schema.id);
    if package.schemas.len() != schemas.len() {
        return Err(link_mismatch("schema count does not match verified Core"));
    }
    let mut schema_ids = HashMap::new();
    reserve_map(&mut schema_ids, schemas.len(), "link schema identifiers")?;
    for (index, expected) in schemas.iter().enumerate() {
        let actual = package.schemas[index];
        let layout = compute_layout(expected.fields)?;
        if actual.id != expected.id
            || actual.kind != expected.kind
            || package_string(package, actual.name)? != expected.local_name
            || actual.byte_size != layout.byte_size
            || actual.alignment != layout.alignment
        {
            return Err(link_mismatch(format!(
                "schema `{}` does not match verified Core",
                expected.local_name
            )));
        }
        let schema_ref = SchemaRef::new(as_u64(index, "schema index")?);
        let mut actual_fields: Vec<&FieldRecord> = Vec::new();
        reserve_vec(
            &mut actual_fields,
            expected.fields.len(),
            "linked schema fields",
        )?;
        actual_fields.extend(
            package
                .fields
                .iter()
                .filter(|field| field.schema == schema_ref),
        );
        if actual_fields.len() != expected.fields.len() {
            return Err(link_mismatch(format!(
                "schema `{}` field count does not match verified Core",
                expected.local_name
            )));
        }
        for ((actual_field, expected_field), expected_offset) in actual_fields
            .iter()
            .zip(expected.fields)
            .zip(layout.field_offsets)
        {
            if package_string(package, actual_field.name)? != expected_field.name.as_str()
                || actual_field.primitive != primitive(expected_field.ty)
                || actual_field.byte_offset != expected_offset
            {
                return Err(link_mismatch(format!(
                    "schema `{}.{}` does not match verified Core",
                    expected.local_name, expected_field.name
                )));
            }
        }
        schema_ids.insert(expected.legacy_id, expected.id);
    }

    let mut systems = system_drafts(core, &schema_ids)?;
    systems.sort_unstable_by_key(|system| system.id);
    if package.systems.len() != systems.len() {
        return Err(link_mismatch("system count does not match verified Core"));
    }
    let startup = program
        .functions
        .iter()
        .find(|function| function.name == "startup")
        .ok_or_else(|| invalid_core("verified executable Core has no startup function"))?;
    let mut schedule_ids = HashMap::new();
    reserve_map(
        &mut schedule_ids,
        program.schedules.len(),
        "link schedule identifiers",
    )?;
    for schedule in &program.schedules {
        schedule_ids.insert(
            schedule.id,
            DeclId::schedule(&program.world.name, &schedule.name),
        );
    }
    let expected_startup_abi = hash_startup_abi();
    let expected_startup_body = hash_startup_body(startup, &schema_ids, &schedule_ids)?;
    if package.world.startup_abi_hash != expected_startup_abi
        || package.world.startup_body_hash != expected_startup_body
    {
        return Err(link_mismatch(
            "startup ABI or body hash does not match verified Core",
        ));
    }

    let mut parameter_cursor = 0usize;
    let mut expected_query_count = 0usize;
    for (system_index, expected) in systems.iter().enumerate() {
        let actual = package.systems[system_index];
        if actual.id != expected.id
            || package_string(package, actual.name)? != expected.system.name.as_str()
            || actual.abi_hash != expected.abi_hash
            || actual.body_hash != expected.body_hash
        {
            return Err(link_mismatch(format!(
                "system `{}` does not match verified Core",
                expected.system.name
            )));
        }
        for expected_parameter in &expected.system.params {
            let actual_parameter = package.parameters.get(parameter_cursor).ok_or_else(|| {
                link_mismatch(format!(
                    "system `{}` parameter count does not match verified Core",
                    expected.system.name
                ))
            })?;
            if actual_parameter.system.index() != as_u64(system_index, "system index")?
                || package_string(package, actual_parameter.name)?
                    != expected_parameter.name.as_str()
            {
                return Err(link_mismatch(format!(
                    "system `{}.{}` parameter does not match verified Core",
                    expected.system.name, expected_parameter.name
                )));
            }
            match (&expected_parameter.kind, actual_parameter.kind) {
                (
                    CoreSystemParamKind::ReadResource { resource_id, .. },
                    ParameterKind::ReadResource { resource },
                )
                | (
                    CoreSystemParamKind::MutResource { resource_id, .. },
                    ParameterKind::MutResource { resource },
                ) if package.schemas[resource.index() as usize].id
                    == *required_schema_id(&schema_ids, *resource_id)? => {}
                (CoreSystemParamKind::Query { terms }, ParameterKind::Query { query }) => {
                    expected_query_count = expected_query_count.checked_add(1).ok_or(
                        ExecutionPackageBuildError::AddressSpaceOverflow("query count"),
                    )?;
                    let actual_query = &package.queries[query.index() as usize];
                    let expected_query_id = DeclId::query(expected.id, &expected_parameter.name);
                    if actual_query.id != expected_query_id
                        || actual_query.system.index() != as_u64(system_index, "system index")?
                        || actual_query.parameter.index()
                            != as_u64(parameter_cursor, "parameter index")?
                    {
                        return Err(link_mismatch(format!(
                            "query `{}.{}` does not match verified Core",
                            expected.system.name, expected_parameter.name
                        )));
                    }
                    let mut actual_terms: Vec<&TermRecord> = Vec::new();
                    reserve_vec(&mut actual_terms, terms.len(), "linked query terms")?;
                    actual_terms.extend(package.terms.iter().filter(|term| term.query == query));
                    if actual_terms.len() != terms.len() {
                        return Err(link_mismatch(format!(
                            "query `{}.{}` term count does not match verified Core",
                            expected.system.name, expected_parameter.name
                        )));
                    }
                    for (actual_term, expected_term) in actual_terms.iter().zip(terms) {
                        if actual_term.access != query_access(expected_term.access)
                            || package.schemas[actual_term.schema.index() as usize].id
                                != *required_schema_id(&schema_ids, expected_term.component_id)?
                        {
                            return Err(link_mismatch(format!(
                                "query `{}.{}` terms do not match verified Core",
                                expected.system.name, expected_parameter.name
                            )));
                        }
                    }
                }
                _ => {
                    return Err(link_mismatch(format!(
                        "system `{}.{}` parameter kind does not match verified Core",
                        expected.system.name, expected_parameter.name
                    )))
                }
            }
            parameter_cursor = parameter_cursor.checked_add(1).ok_or(
                ExecutionPackageBuildError::AddressSpaceOverflow("parameter count"),
            )?;
        }
    }
    if parameter_cursor != package.parameters.len() || expected_query_count != package.queries.len()
    {
        return Err(link_mismatch(
            "parameter or query count does not match verified Core",
        ));
    }

    let mut schedules = Vec::new();
    reserve_vec(&mut schedules, program.schedules.len(), "linked schedules")?;
    for schedule in &program.schedules {
        schedules.push((
            DeclId::schedule(&program.world.name, &schedule.name),
            schedule.name.as_str(),
        ));
    }
    schedules.sort_unstable_by_key(|schedule| schedule.0);
    if schedules.len() != package.schedules.len() {
        return Err(link_mismatch("schedule count does not match verified Core"));
    }
    for (expected, actual) in schedules.iter().zip(&package.schedules) {
        if actual.id != expected.0 || package_string(package, actual.name)? != expected.1 {
            return Err(link_mismatch(format!(
                "schedule `{}` does not match verified Core",
                expected.1
            )));
        }
    }

    if package.function_links[0].abi_hash != expected_startup_abi
        || package.function_links[0].body_hash != expected_startup_body
    {
        return Err(link_mismatch(
            "startup function link does not match verified Core",
        ));
    }
    for (index, expected) in systems.iter().enumerate() {
        let actual = package.function_links[index + 1];
        if actual.target
            != (FunctionTarget::System {
                system: SystemRef::new(as_u64(index, "system index")?),
            })
            || actual.abi_hash != expected.abi_hash
            || actual.body_hash != expected.body_hash
        {
            return Err(link_mismatch(format!(
                "system function `{}` does not match verified Core",
                expected.system.name
            )));
        }
    }
    Ok(())
}

fn build_source_span_plan(
    core: &VerifiedExecutableCore,
    systems: &[SystemDraft<'_>],
) -> Result<SourceSpanPlan, ExecutionPackageBuildError> {
    let source_map = &core.program().source_map;
    let relevant_count = source_map
        .entries
        .iter()
        .filter(|entry| entry.subject != CoreSourceSubject::Program)
        .count();
    let mut entries = Vec::new();
    reserve_vec(&mut entries, relevant_count, "Core source-map ordering")?;
    entries.extend(
        source_map
            .entries
            .iter()
            .filter(|entry| entry.subject != CoreSourceSubject::Program),
    );
    entries.sort_unstable_by_key(|entry| (entry.span.start.byte, entry.span.end.byte));

    let mut spans: Vec<crate::lexer::SourceSpan> = Vec::new();
    reserve_vec(&mut spans, entries.len(), "canonical source spans")?;
    for entry in &entries {
        if let Some(previous) = spans.last() {
            if (previous.start.byte, previous.end.byte)
                == (entry.span.start.byte, entry.span.end.byte)
            {
                if *previous != entry.span {
                    return Err(invalid_core(
                        "Core source-map coordinates disagree for one byte span",
                    ));
                }
                continue;
            }
        }
        spans.push(entry.span);
    }

    let mut subjects = HashMap::new();
    reserve_map(
        &mut subjects,
        entries.len(),
        "Core source subject references",
    )?;
    for entry in entries {
        let index = spans
            .binary_search_by_key(&(entry.span.start.byte, entry.span.end.byte), |span| {
                (span.start.byte, span.end.byte)
            })
            .map_err(|_| invalid_core("Core source span was lost during canonical ordering"))?;
        let reference = SourceSpanRef::new(as_u64(index, "source-span index")?);
        if subjects.insert(entry.subject.clone(), reference).is_some() {
            return Err(invalid_core(format!(
                "verified Core source subject {:?} is duplicated",
                entry.subject
            )));
        }
    }

    let mut body_ranges = HashMap::new();
    let body_count =
        systems
            .len()
            .checked_add(1)
            .ok_or(ExecutionPackageBuildError::AddressSpaceOverflow(
                "function body-span count",
            ))?;
    reserve_map(&mut body_ranges, body_count, "function body-span ranges")?;
    insert_body_span_range(
        &mut body_ranges,
        &subjects,
        &spans,
        NativeFunctionTarget::Startup,
        &CoreSourceSubject::Startup,
    )?;
    for system in systems {
        insert_body_span_range(
            &mut body_ranges,
            &subjects,
            &spans,
            NativeFunctionTarget::System(system.id),
            &CoreSourceSubject::System {
                system_id: system.legacy_id,
            },
        )?;
    }

    Ok(SourceSpanPlan {
        spans,
        subjects,
        body_ranges,
    })
}

fn insert_body_span_range(
    ranges: &mut HashMap<NativeFunctionTarget, (SourceSpanRef, u64)>,
    subjects: &HashMap<CoreSourceSubject, SourceSpanRef>,
    spans: &[crate::lexer::SourceSpan],
    target: NativeFunctionTarget,
    owner_subject: &CoreSourceSubject,
) -> Result<(), ExecutionPackageBuildError> {
    let owner_ref = subjects
        .get(owner_subject)
        .copied()
        .ok_or_else(|| invalid_core(format!("verified Core source map omits {owner_subject:?}")))?;
    let owner = spans
        .get(usize::try_from(owner_ref.index()).map_err(|_| {
            ExecutionPackageBuildError::AddressSpaceOverflow("source-span reference")
        })?)
        .copied()
        .ok_or_else(|| invalid_core("function source-span reference is out of range"))?;
    let first = spans
        .iter()
        .position(|span| span_is_nested(*span, owner))
        .ok_or_else(|| invalid_core("function has no canonical body spans"))?;
    let last = spans
        .iter()
        .rposition(|span| span_is_nested(*span, owner))
        .ok_or_else(|| invalid_core("function has no canonical body spans"))?;
    if spans[first..=last]
        .iter()
        .any(|span| !span_is_nested(*span, owner))
    {
        return Err(invalid_core(
            "function source spans are not contiguous in canonical source order",
        ));
    }
    let count = last
        .checked_sub(first)
        .and_then(|distance| distance.checked_add(1))
        .ok_or(ExecutionPackageBuildError::AddressSpaceOverflow(
            "function body-span range",
        ))?;
    ranges.insert(
        target,
        (
            SourceSpanRef::new(as_u64(first, "source-span index")?),
            as_u64(count, "function body-span count")?,
        ),
    );
    Ok(())
}

fn span_is_nested(span: crate::lexer::SourceSpan, owner: crate::lexer::SourceSpan) -> bool {
    span.start.byte >= owner.start.byte && span.end.byte <= owner.end.byte
}

fn schema_drafts(
    core: &VerifiedExecutableCore,
) -> Result<Vec<SchemaDraft<'_>>, ExecutionPackageBuildError> {
    let program = core.program();
    let schema_count = program
        .components
        .len()
        .checked_add(program.resources.len())
        .ok_or(ExecutionPackageBuildError::AddressSpaceOverflow(
            "schema draft count",
        ))?;
    let mut result = Vec::new();
    reserve_vec(&mut result, schema_count, "schema drafts")?;
    for component in &program.components {
        let local_name = local_schema_name(&program.world.name, &component.name)?;
        let kind = match component.kind {
            CoreComponentKind::Component => SchemaKind::Component,
            CoreComponentKind::Tag => SchemaKind::Tag,
        };
        let fingerprint_fields = fingerprint_fields(&component.fields)?;
        result.push(SchemaDraft {
            legacy_id: component.id,
            id: SchemaId::derive(kind, &program.world.name, local_name, &fingerprint_fields),
            kind,
            local_name,
            fields: &component.fields,
        });
    }
    for resource in &program.resources {
        let local_name = local_schema_name(&program.world.name, &resource.name)?;
        let fingerprint_fields = fingerprint_fields(&resource.fields)?;
        result.push(SchemaDraft {
            legacy_id: resource.id,
            id: SchemaId::derive(
                SchemaKind::Resource,
                &program.world.name,
                local_name,
                &fingerprint_fields,
            ),
            kind: SchemaKind::Resource,
            local_name,
            fields: &resource.fields,
        });
    }
    Ok(result)
}

fn schema_source_subject(schema: &SchemaDraft<'_>) -> CoreSourceSubject {
    match schema.kind {
        SchemaKind::Component | SchemaKind::Tag => CoreSourceSubject::Component {
            component_id: schema.legacy_id,
        },
        SchemaKind::Resource => CoreSourceSubject::Resource {
            resource_id: schema.legacy_id,
        },
    }
}

fn schema_field_source_subject(schema: &SchemaDraft<'_>, field_index: u64) -> CoreSourceSubject {
    match schema.kind {
        SchemaKind::Component | SchemaKind::Tag => CoreSourceSubject::ComponentField {
            component_id: schema.legacy_id,
            field_index,
        },
        SchemaKind::Resource => CoreSourceSubject::ResourceField {
            resource_id: schema.legacy_id,
            field_index,
        },
    }
}

fn system_drafts<'a>(
    core: &'a VerifiedExecutableCore,
    schema_ids: &HashMap<u64, SchemaId>,
) -> Result<Vec<SystemDraft<'a>>, ExecutionPackageBuildError> {
    let program = core.program();
    let mut drafts = Vec::new();
    reserve_vec(&mut drafts, program.systems.len(), "system drafts")?;
    for system in &program.systems {
        let id = DeclId::system(&program.world.name, &system.name);
        drafts.push(SystemDraft {
            legacy_id: system.id,
            id,
            system,
            abi_hash: hash_system_abi(id, system, schema_ids)?,
            body_hash: hash_system_body(system, schema_ids)?,
        });
    }
    Ok(drafts)
}

fn build_startup(
    core: &VerifiedExecutableCore,
    schemas: &[SchemaDraft<'_>],
    schema_refs: &HashMap<u64, SchemaRef>,
    schedule_refs: &HashMap<u64, ScheduleRef>,
    source_plan: &SourceSpanPlan,
) -> Result<(Vec<PayloadRecord>, Vec<StartupOperationRecord>), ExecutionPackageBuildError> {
    let mut schemas_by_legacy = HashMap::new();
    reserve_map(
        &mut schemas_by_legacy,
        schemas.len(),
        "startup schema references",
    )?;
    for schema in schemas {
        schemas_by_legacy.insert(schema.legacy_id, schema);
    }
    let mut payloads = Vec::new();
    let mut operations = Vec::new();
    let operation_count = core.startup_operations().count();
    reserve_vec(
        &mut operations,
        operation_count,
        "startup operation records",
    )?;
    // One resource payload or one payload per spawned schema is the exact
    // maximum implied by the verified startup effect stream.
    let payload_count =
        core.startup_operations()
            .try_fold(0usize, |count, instruction| {
                let additional = match instruction {
                    CoreInstruction::InitializeResource { .. } => 1,
                    CoreInstruction::Spawn { components } => components.len(),
                    CoreInstruction::RunSchedule { .. } => 0,
                    _ => 0,
                };
                count.checked_add(additional).ok_or(
                    ExecutionPackageBuildError::AddressSpaceOverflow("startup payload count"),
                )
            })?;
    reserve_vec(&mut payloads, payload_count, "startup payload records")?;
    for instruction in core.startup_operations() {
        let source_subject = startup_instruction_source_subject(core, instruction)?;
        let kind = match instruction {
            CoreInstruction::InitializeResource {
                resource_id,
                fields,
                ..
            } => {
                let schema = required_schema(&schemas_by_legacy, *resource_id)?;
                let payload = PayloadRef::new(as_u64(payloads.len(), "payload index")?);
                payloads.push(PayloadRecord {
                    schema: required_schema_ref(schema_refs, *resource_id)?,
                    bytes: encode_payload(
                        schema.fields,
                        fields
                            .iter()
                            .map(|field| (field.name.as_str(), &field.value)),
                    )?,
                });
                StartupOperationKind::ResourcePayload {
                    resource: required_schema_ref(schema_refs, *resource_id)?,
                    payload,
                }
            }
            CoreInstruction::Spawn { components } => {
                let first_payload = PayloadRef::new(as_u64(payloads.len(), "payload index")?);
                let mut ordered = Vec::new();
                reserve_vec(&mut ordered, components.len(), "spawn schema ordering")?;
                ordered.extend(components);
                ordered.sort_unstable_by_key(|component| {
                    schema_refs
                        .get(&component.component_id)
                        .copied()
                        .unwrap_or(SchemaRef::new(u64::MAX))
                });
                for component in ordered {
                    let schema = required_schema(&schemas_by_legacy, component.component_id)?;
                    payloads.push(PayloadRecord {
                        schema: required_schema_ref(schema_refs, component.component_id)?,
                        bytes: encode_payload(
                            schema.fields,
                            component
                                .fields
                                .iter()
                                .map(|field| (field.name.as_str(), &field.value)),
                        )?,
                    });
                }
                StartupOperationKind::Spawn {
                    first_payload,
                    payload_count: as_u64(components.len(), "spawn payload count")?,
                }
            }
            CoreInstruction::RunSchedule { schedule_id, .. } => StartupOperationKind::RunSchedule {
                schedule: required_schedule_ref(schedule_refs, *schedule_id)?,
            },
            _ => {
                return Err(invalid_core(
                    "verified startup operation list contains a non-effect instruction",
                ));
            }
        };
        operations.push(StartupOperationRecord {
            kind,
            source_span: Some(source_plan.reference(&source_subject)?),
        });
    }
    Ok((payloads, operations))
}

fn startup_instruction_source_subject(
    core: &VerifiedExecutableCore,
    target: &CoreInstruction,
) -> Result<CoreSourceSubject, ExecutionPackageBuildError> {
    let startup = core
        .program()
        .functions
        .iter()
        .find(|function| function.name == "startup")
        .ok_or_else(|| invalid_core("verified executable Core has no startup function"))?;
    for block in &startup.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            if std::ptr::eq(instruction, target) {
                return Ok(CoreSourceSubject::StartupInstruction {
                    block: block.id,
                    instruction_index: as_u64(instruction_index, "startup instruction index")?,
                });
            }
        }
    }
    Err(invalid_core(
        "verified startup operation does not belong to the startup function",
    ))
}

struct ComputedLayout {
    field_offsets: Vec<u64>,
    byte_size: u64,
    alignment: u64,
}

fn compute_layout(fields: &[CoreField]) -> Result<ComputedLayout, ExecutionPackageBuildError> {
    let mut offsets = Vec::new();
    reserve_vec(&mut offsets, fields.len(), "field layout offsets")?;
    let mut cursor = 0u64;
    let mut alignment = 1u64;
    for field in fields {
        let primitive = primitive(field.ty);
        let field_alignment = primitive_alignment(primitive);
        cursor = align_u64(cursor, field_alignment, "field layout")?;
        offsets.push(cursor);
        cursor = cursor.checked_add(primitive_size(primitive)).ok_or(
            ExecutionPackageBuildError::ArithmeticOverflow("schema layout"),
        )?;
        alignment = alignment.max(field_alignment);
    }
    Ok(ComputedLayout {
        field_offsets: offsets,
        byte_size: align_u64(cursor, alignment, "schema layout")?,
        alignment,
    })
}

fn encode_payload<'a>(
    fields: &[CoreField],
    values: impl Iterator<Item = (&'a str, &'a CoreLiteralValue)>,
) -> Result<Vec<u8>, ExecutionPackageBuildError> {
    let layout = compute_layout(fields)?;
    let byte_len = usize::try_from(layout.byte_size)
        .map_err(|_| ExecutionPackageBuildError::AddressSpaceOverflow("schema payload"))?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(byte_len).map_err(|error| {
        ExecutionPackageBuildError::Allocation(format!(
            "could not allocate schema payload: {error}"
        ))
    })?;
    bytes.resize(byte_len, 0);
    let mut values_by_name = HashMap::new();
    reserve_map(&mut values_by_name, fields.len(), "payload field values")?;
    for (name, value) in values {
        values_by_name.insert(name, value);
    }
    for ((field, byte_offset), expected_type) in fields
        .iter()
        .zip(layout.field_offsets)
        .zip(fields.iter().map(|field| field.ty))
    {
        let value = values_by_name.get(field.name.as_str()).ok_or_else(|| {
            invalid_core(format!("verified payload omits field `{}`", field.name))
        })?;
        let (encoded, encoded_len): ([u8; 4], usize) = match (expected_type, *value) {
            (CoreType::I32, CoreLiteralValue::I32(value)) => (value.to_le_bytes(), 4),
            (CoreType::F32, CoreLiteralValue::F32Bits(bits)) => (bits.to_le_bytes(), 4),
            (CoreType::Bool, CoreLiteralValue::Bool(value)) => ([u8::from(*value), 0, 0, 0], 1),
            _ => {
                return Err(invalid_core(format!(
                    "verified payload field `{}` has a mismatched literal type",
                    field.name
                )))
            }
        };
        let start = usize::try_from(byte_offset).map_err(|_| {
            ExecutionPackageBuildError::AddressSpaceOverflow("payload field offset")
        })?;
        let end = start.checked_add(encoded_len).ok_or(
            ExecutionPackageBuildError::ArithmeticOverflow("payload field range"),
        )?;
        let destination = bytes.get_mut(start..end).ok_or_else(|| {
            invalid_core(format!(
                "verified payload field `{}` exceeds its schema",
                field.name
            ))
        })?;
        destination.copy_from_slice(&encoded[..encoded_len]);
    }
    Ok(bytes)
}

struct FunctionHashes {
    startup_abi: AbiHash,
    startup_body: BodyHash,
}

fn function_hashes(
    core: &VerifiedExecutableCore,
    _systems: &[SystemDraft<'_>],
    schema_ids: &HashMap<u64, SchemaId>,
    schedule_ids: &HashMap<u64, DeclId>,
) -> Result<FunctionHashes, ExecutionPackageBuildError> {
    let startup = core
        .program()
        .functions
        .iter()
        .find(|function| function.name == "startup")
        .ok_or_else(|| invalid_core("verified executable Core has no startup function"))?;
    Ok(FunctionHashes {
        startup_abi: hash_startup_abi(),
        startup_body: hash_startup_body(startup, schema_ids, schedule_ids)?,
    })
}

fn hash_startup_abi() -> AbiHash {
    let mut hash = AbiHasher::new();
    hash.append_u64(CANONICAL_CORE_ENCODING_VERSION)
        .append_u8(1)
        .append_u64(0)
        .append_u8(type_code(CoreType::I32));
    hash.finalize()
}

fn hash_system_abi(
    id: DeclId,
    system: &CoreSystem,
    schema_ids: &HashMap<u64, SchemaId>,
) -> Result<AbiHash, ExecutionPackageBuildError> {
    let mut hash = AbiHasher::new();
    hash.append_u64(CANONICAL_CORE_ENCODING_VERSION)
        .append_u8(2)
        .append_id(&id)
        .append_u64(as_u64(system.params.len(), "system parameter count")?);
    for parameter in &system.params {
        hash.append_string(&parameter.name);
        match &parameter.kind {
            CoreSystemParamKind::ReadResource { resource_id, .. } => {
                hash.append_u8(1)
                    .append_id(required_schema_id(schema_ids, *resource_id)?);
            }
            CoreSystemParamKind::MutResource { resource_id, .. } => {
                hash.append_u8(2)
                    .append_id(required_schema_id(schema_ids, *resource_id)?);
            }
            CoreSystemParamKind::Query { terms } => {
                hash.append_u8(3)
                    .append_u64(as_u64(terms.len(), "query term count")?);
                for term in terms {
                    hash.append_u8(query_access_code(term.access))
                        .append_id(required_schema_id(schema_ids, term.component_id)?);
                }
            }
        }
    }
    Ok(hash.finalize())
}

fn hash_startup_body(
    function: &CoreFunction,
    schema_ids: &HashMap<u64, SchemaId>,
    schedule_ids: &HashMap<u64, DeclId>,
) -> Result<BodyHash, ExecutionPackageBuildError> {
    let mut hash = BodyHasher::new();
    hash.append_u64(CANONICAL_CORE_ENCODING_VERSION)
        .append_u8(1)
        .append_string(&function.name)
        .append_u64(function.entry.0);
    let mut locals = Vec::new();
    reserve_vec(
        &mut locals,
        function.locals.len(),
        "Core local hash ordering",
    )?;
    locals.extend(&function.locals);
    locals.sort_unstable_by_key(|local| local.id.0);
    hash.append_u64(as_u64(locals.len(), "Core local count")?);
    for local in locals {
        hash.append_u64(local.id.0)
            .append_string(&local.name)
            .append_u8(type_code(local.ty));
    }
    let mut blocks = Vec::new();
    reserve_vec(
        &mut blocks,
        function.blocks.len(),
        "Core block hash ordering",
    )?;
    blocks.extend(&function.blocks);
    blocks.sort_unstable_by_key(|block| block.id.0);
    hash.append_u64(as_u64(blocks.len(), "Core block count")?);
    for block in blocks {
        hash.append_u64(block.id.0)
            .append_u64(as_u64(block.instructions.len(), "Core instruction count")?);
        for instruction in &block.instructions {
            hash_instruction(&mut hash, instruction, schema_ids, schedule_ids)?;
        }
        hash_terminator(&mut hash, &block.terminator);
    }
    Ok(hash.finalize())
}

fn hash_instruction(
    hash: &mut BodyHasher,
    instruction: &CoreInstruction,
    schema_ids: &HashMap<u64, SchemaId>,
    schedule_ids: &HashMap<u64, DeclId>,
) -> Result<(), ExecutionPackageBuildError> {
    match instruction {
        CoreInstruction::InitializeResource {
            resource_id,
            fields,
            ..
        } => {
            hash.append_u8(1)
                .append_id(required_schema_id(schema_ids, *resource_id)?)
                .append_u64(as_u64(fields.len(), "resource field count")?);
            for field in fields {
                hash.append_string(&field.name)
                    .append_u64(field.evaluation.0);
                hash_literal(hash, &field.value);
            }
        }
        CoreInstruction::Spawn { components } => {
            hash.append_u8(2)
                .append_u64(as_u64(components.len(), "spawn component count")?);
            for component in components {
                hash.append_id(required_schema_id(schema_ids, component.component_id)?)
                    .append_u64(as_u64(component.fields.len(), "spawn field count")?);
                for field in &component.fields {
                    hash.append_string(&field.name)
                        .append_u64(field.evaluation.0);
                    hash_literal(hash, &field.value);
                }
            }
        }
        CoreInstruction::RunSchedule { schedule_id, .. } => {
            let schedule = schedule_ids.get(schedule_id).ok_or_else(|| {
                invalid_core(format!("unknown Core schedule id 0x{schedule_id:016X}"))
            })?;
            hash.append_u8(3).append_id(schedule);
        }
        CoreInstruction::I32Const { result, value } => {
            hash.append_u8(4)
                .append_u64(result.0)
                .append_u64(u64::from(u32::from_le_bytes(value.to_le_bytes())));
        }
        CoreInstruction::I32Binary {
            result,
            op,
            left,
            right,
        } => {
            hash.append_u8(5)
                .append_u64(result.0)
                .append_u8(binary_code(*op))
                .append_u64(left.0)
                .append_u64(right.0);
        }
        CoreInstruction::I32Unary {
            result,
            op,
            operand,
        } => {
            hash.append_u8(6)
                .append_u64(result.0)
                .append_u8(unary_code(*op))
                .append_u64(operand.0);
        }
        CoreInstruction::F32Const { result, bits } => {
            hash.append_u8(7)
                .append_u64(result.0)
                .append_u64(u64::from(*bits));
        }
        CoreInstruction::F32Unary {
            result,
            op,
            operand,
        } => {
            hash.append_u8(8)
                .append_u64(result.0)
                .append_u8(unary_code(*op))
                .append_u64(operand.0);
        }
        CoreInstruction::F32Binary {
            result,
            op,
            left,
            right,
        } => {
            hash.append_u8(9)
                .append_u64(result.0)
                .append_u8(binary_code(*op))
                .append_u64(left.0)
                .append_u64(right.0);
        }
        CoreInstruction::Compare {
            result,
            op,
            left,
            right,
            operand_type,
        } => {
            hash.append_u8(10)
                .append_u64(result.0)
                .append_u8(comparison_code(*op))
                .append_u64(left.0)
                .append_u64(right.0)
                .append_u8(type_code(*operand_type));
        }
        CoreInstruction::BoolConst { result, value } => {
            hash.append_u8(11)
                .append_u64(result.0)
                .append_u8(u8::from(*value));
        }
        CoreInstruction::BoolNot { result, operand } => {
            hash.append_u8(12)
                .append_u64(result.0)
                .append_u64(operand.0);
        }
        CoreInstruction::Equal {
            result,
            left,
            right,
            operand_type,
            negate,
        } => {
            hash.append_u8(13)
                .append_u64(result.0)
                .append_u64(left.0)
                .append_u64(right.0)
                .append_u8(type_code(*operand_type))
                .append_u8(u8::from(*negate));
        }
        CoreInstruction::LocalStore { local, value } => {
            hash.append_u8(14).append_u64(local.0).append_u64(value.0);
        }
        CoreInstruction::LocalLoad { result, local } => {
            hash.append_u8(15).append_u64(result.0).append_u64(local.0);
        }
    }
    Ok(())
}

fn hash_terminator(hash: &mut BodyHasher, terminator: &CoreTerminator) {
    match terminator {
        CoreTerminator::Exit { value } => {
            hash.append_u8(1).append_u64(value.0);
        }
        CoreTerminator::Jump { target } => {
            hash.append_u8(2).append_u64(target.0);
        }
        CoreTerminator::Branch {
            condition,
            then_block,
            else_block,
        } => {
            hash.append_u8(3)
                .append_u64(condition.0)
                .append_u64(then_block.0)
                .append_u64(else_block.0);
        }
    }
}

fn hash_system_body(
    system: &CoreSystem,
    schema_ids: &HashMap<u64, SchemaId>,
) -> Result<BodyHash, ExecutionPackageBuildError> {
    let mut hash = BodyHasher::new();
    hash.append_u64(CANONICAL_CORE_ENCODING_VERSION)
        .append_u8(2)
        .append_u64(as_u64(
            system.body.statements.len(),
            "system statement count",
        )?);
    for statement in &system.body.statements {
        hash_system_statement(&mut hash, statement, schema_ids)?;
    }
    Ok(hash.finalize())
}

fn hash_system_statement(
    hash: &mut BodyHasher,
    statement: &CoreSystemStatement,
    schema_ids: &HashMap<u64, SchemaId>,
) -> Result<(), ExecutionPackageBuildError> {
    match statement {
        CoreSystemStatement::QueryLoop(loop_) => {
            hash.append_u8(1)
                .append_string(&loop_.query_param)
                .append_u64(as_u64(loop_.bindings.len(), "query binding count")?);
            for binding in &loop_.bindings {
                hash.append_string(&binding.name)
                    .append_id(required_schema_id(schema_ids, binding.component_id)?)
                    .append_u8(query_access_code(binding.access));
            }
            hash.append_u64(as_u64(loop_.body.len(), "query body statement count")?);
            for statement in &loop_.body {
                hash_system_statement(hash, statement, schema_ids)?;
            }
        }
        CoreSystemStatement::Expression(expression) => {
            hash.append_u8(2);
            hash_system_expression(hash, expression, schema_ids)?;
        }
        CoreSystemStatement::Let {
            name,
            ty,
            mutable,
            value,
        } => {
            hash.append_u8(3)
                .append_string(name)
                .append_u8(type_code(*ty))
                .append_u8(u8::from(*mutable));
            hash_system_expression(hash, value, schema_ids)?;
        }
        CoreSystemStatement::Assign { target, value } => {
            hash.append_u8(4);
            hash_system_place(hash, target, schema_ids)?;
            hash_system_expression(hash, value, schema_ids)?;
        }
        CoreSystemStatement::AddAssign { target, value } => {
            hash.append_u8(5);
            hash_system_place(hash, target, schema_ids)?;
            hash_system_expression(hash, value, schema_ids)?;
        }
        CoreSystemStatement::Block(statements) => {
            hash.append_u8(6)
                .append_u64(as_u64(statements.len(), "block statement count")?);
            for statement in statements {
                hash_system_statement(hash, statement, schema_ids)?;
            }
        }
        CoreSystemStatement::If {
            condition,
            then_body,
            else_body,
        } => {
            hash.append_u8(7);
            hash_system_expression(hash, condition, schema_ids)?;
            hash.append_u64(as_u64(then_body.len(), "then statement count")?);
            for statement in then_body {
                hash_system_statement(hash, statement, schema_ids)?;
            }
            hash.append_u64(as_u64(else_body.len(), "else statement count")?);
            for statement in else_body {
                hash_system_statement(hash, statement, schema_ids)?;
            }
        }
        CoreSystemStatement::While { condition, body } => {
            hash.append_u8(8);
            hash_system_expression(hash, condition, schema_ids)?;
            hash.append_u64(as_u64(body.len(), "while statement count")?);
            for statement in body {
                hash_system_statement(hash, statement, schema_ids)?;
            }
        }
    }
    Ok(())
}

fn hash_system_place(
    hash: &mut BodyHasher,
    place: &CoreSystemPlace,
    schema_ids: &HashMap<u64, SchemaId>,
) -> Result<(), ExecutionPackageBuildError> {
    match place {
        CoreSystemPlace::Local { name, ty, mutable } => {
            hash.append_u8(1)
                .append_string(name)
                .append_u8(type_code(*ty))
                .append_u8(u8::from(*mutable));
        }
        CoreSystemPlace::ComponentField {
            binding,
            component_id,
            field_name,
            ..
        } => {
            hash.append_u8(2)
                .append_string(binding)
                .append_id(required_schema_id(schema_ids, *component_id)?)
                .append_string(field_name);
        }
        CoreSystemPlace::ResourceField {
            param,
            resource_id,
            field_name,
            ..
        } => {
            hash.append_u8(3)
                .append_string(param)
                .append_id(required_schema_id(schema_ids, *resource_id)?)
                .append_string(field_name);
        }
    }
    Ok(())
}

fn hash_system_expression(
    hash: &mut BodyHasher,
    expression: &CoreSystemExpression,
    schema_ids: &HashMap<u64, SchemaId>,
) -> Result<(), ExecutionPackageBuildError> {
    match expression {
        CoreSystemExpression::I32Const(value) => {
            hash.append_u8(1)
                .append_u64(u64::from(u32::from_le_bytes(value.to_le_bytes())));
        }
        CoreSystemExpression::F32Const(bits) => {
            hash.append_u8(2).append_u64(u64::from(*bits));
        }
        CoreSystemExpression::BoolConst(value) => {
            hash.append_u8(3).append_u8(u8::from(*value));
        }
        CoreSystemExpression::Local { name, ty } => {
            hash.append_u8(4)
                .append_string(name)
                .append_u8(type_code(*ty));
        }
        CoreSystemExpression::ResourceField {
            param,
            resource_id,
            field_name,
            ..
        } => {
            hash.append_u8(5)
                .append_string(param)
                .append_id(required_schema_id(schema_ids, *resource_id)?)
                .append_string(field_name);
        }
        CoreSystemExpression::ComponentField {
            binding,
            component_id,
            field_name,
            ..
        } => {
            hash.append_u8(6)
                .append_string(binding)
                .append_id(required_schema_id(schema_ids, *component_id)?)
                .append_string(field_name);
        }
        CoreSystemExpression::BoolNot(operand) => {
            hash.append_u8(7);
            hash_system_expression(hash, operand, schema_ids)?;
        }
        CoreSystemExpression::Unary { op, operand } => {
            hash.append_u8(8).append_u8(system_unary_code(*op));
            hash_system_expression(hash, operand, schema_ids)?;
        }
        CoreSystemExpression::Binary { op, left, right } => {
            hash.append_u8(9).append_u8(system_binary_code(*op));
            hash_system_expression(hash, left, schema_ids)?;
            hash_system_expression(hash, right, schema_ids)?;
        }
    }
    Ok(())
}

fn hash_literal(hash: &mut BodyHasher, value: &CoreLiteralValue) {
    match value {
        CoreLiteralValue::I32(value) => {
            hash.append_u8(1)
                .append_u64(u64::from(u32::from_le_bytes(value.to_le_bytes())));
        }
        CoreLiteralValue::F32Bits(bits) => {
            hash.append_u8(2).append_u64(u64::from(*bits));
        }
        CoreLiteralValue::Bool(value) => {
            hash.append_u8(3).append_u8(u8::from(*value));
        }
    }
}

fn validate_native_layout<'a>(
    native: &'a NativeCodeLayout,
    systems: &[SystemDraft<'_>],
) -> Result<HashMap<NativeFunctionTarget, &'a NativeFunctionLayout>, ExecutionPackageBuildError> {
    let code_end = native
        .code_range
        .offset
        .checked_add(native.code_range.byte_len)
        .ok_or(ExecutionPackageBuildError::ArithmeticOverflow(
            "native code range",
        ))?;
    let mut by_target = HashMap::new();
    reserve_map(
        &mut by_target,
        native.functions.len(),
        "native function links",
    )?;
    for function in &native.functions {
        if function.symbol_name.is_empty() {
            return Err(invalid_native(
                "native function symbol name cannot be empty",
            ));
        }
        if function.code_byte_len == 0 {
            return Err(invalid_native(format!(
                "native function `{}` has zero byte length",
                function.symbol_name
            )));
        }
        let end = function
            .code_offset
            .checked_add(function.code_byte_len)
            .ok_or_else(|| {
                invalid_native(format!(
                    "native function `{}` range overflows u64",
                    function.symbol_name
                ))
            })?;
        if function.code_offset < native.code_range.offset || end > code_end {
            return Err(invalid_native(format!(
                "native function `{}` lies outside the code image",
                function.symbol_name
            )));
        }
        if by_target.insert(function.target, function).is_some() {
            return Err(invalid_native(format!(
                "native function target {:?} is duplicated",
                function.target
            )));
        }
    }
    if by_target.len() != systems.len() + 1 {
        return Err(invalid_native(
            "native layout must contain exactly startup and one function per system",
        ));
    }
    if !by_target.contains_key(&NativeFunctionTarget::Startup) {
        return Err(invalid_native("native layout has no startup function"));
    }
    for system in systems {
        if !by_target.contains_key(&NativeFunctionTarget::System(system.id)) {
            return Err(invalid_native(format!(
                "native layout has no function for system `{}`",
                system.system.name
            )));
        }
    }
    Ok(by_target)
}

fn canonical_strings(
    values: Vec<&str>,
) -> Result<(Vec<String>, HashMap<String, StringRef>), ExecutionPackageBuildError> {
    let mut strings = Vec::new();
    reserve_vec(&mut strings, values.len(), "canonical strings")?;
    for value in values {
        strings.push(try_owned_string(value, "canonical string")?);
    }
    strings.sort_unstable();
    strings.dedup();
    let mut refs = HashMap::new();
    reserve_map(&mut refs, strings.len(), "canonical string references")?;
    for (index, value) in strings.iter().enumerate() {
        refs.insert(
            try_owned_string(value, "canonical string reference")?,
            StringRef::new(as_u64(index, "string index")?),
        );
    }
    Ok((strings, refs))
}

fn string_ref(
    refs: &HashMap<String, StringRef>,
    value: &str,
) -> Result<StringRef, ExecutionPackageBuildError> {
    refs.get(value)
        .copied()
        .ok_or_else(|| invalid_core(format!("string `{value}` was not interned")))
}

fn local_schema_name<'a>(
    world: &str,
    qualified: &'a str,
) -> Result<&'a str, ExecutionPackageBuildError> {
    let prefix = format!("{world}.");
    qualified
        .strip_prefix(&prefix)
        .filter(|name| !name.is_empty() && !name.contains('.'))
        .ok_or_else(|| {
            invalid_core(format!(
                "Core schema name `{qualified}` is not world-qualified"
            ))
        })
}

fn fingerprint_fields(
    fields: &[CoreField],
) -> Result<Vec<SchemaField<'_>>, ExecutionPackageBuildError> {
    let mut result = Vec::new();
    reserve_vec(&mut result, fields.len(), "schema fingerprint fields")?;
    for field in fields {
        result.push(SchemaField {
            name: &field.name,
            primitive: primitive(field.ty),
        });
    }
    Ok(result)
}

fn required_schema<'a>(
    schemas: &HashMap<u64, &'a SchemaDraft<'a>>,
    id: u64,
) -> Result<&'a SchemaDraft<'a>, ExecutionPackageBuildError> {
    schemas
        .get(&id)
        .copied()
        .ok_or_else(|| invalid_core(format!("unknown Core schema id 0x{id:016X}")))
}

fn required_schema_ref(
    refs: &HashMap<u64, SchemaRef>,
    id: u64,
) -> Result<SchemaRef, ExecutionPackageBuildError> {
    refs.get(&id)
        .copied()
        .ok_or_else(|| invalid_core(format!("unknown Core schema id 0x{id:016X}")))
}

fn required_schema_id(
    ids: &HashMap<u64, SchemaId>,
    id: u64,
) -> Result<&SchemaId, ExecutionPackageBuildError> {
    ids.get(&id)
        .ok_or_else(|| invalid_core(format!("unknown Core schema id 0x{id:016X}")))
}

fn required_system_ref(
    refs: &HashMap<u64, SystemRef>,
    id: u64,
) -> Result<SystemRef, ExecutionPackageBuildError> {
    refs.get(&id)
        .copied()
        .ok_or_else(|| invalid_core(format!("unknown Core system id 0x{id:016X}")))
}

fn required_schedule_ref(
    refs: &HashMap<u64, ScheduleRef>,
    id: u64,
) -> Result<ScheduleRef, ExecutionPackageBuildError> {
    refs.get(&id)
        .copied()
        .ok_or_else(|| invalid_core(format!("unknown Core schedule id 0x{id:016X}")))
}

fn reject_duplicate_canonical_ids<T: Copy + Eq + fmt::Debug>(
    values: impl Iterator<Item = T>,
    kind: &str,
) -> Result<(), ExecutionPackageBuildError> {
    let mut previous = None;
    for value in values {
        if previous == Some(value) {
            return Err(invalid_core(format!(
                "duplicate canonical {kind} id {value:?}"
            )));
        }
        previous = Some(value);
    }
    Ok(())
}

fn align_u64(
    value: u64,
    alignment: u64,
    context: &'static str,
) -> Result<u64, ExecutionPackageBuildError> {
    debug_assert!(alignment.is_power_of_two());
    value
        .checked_add(alignment - 1)
        .map(|sum| sum & !(alignment - 1))
        .ok_or(ExecutionPackageBuildError::ArithmeticOverflow(context))
}

fn as_u64(value: usize, context: &'static str) -> Result<u64, ExecutionPackageBuildError> {
    u64::try_from(value).map_err(|_| ExecutionPackageBuildError::AddressSpaceOverflow(context))
}

fn reserve_vec<T>(
    values: &mut Vec<T>,
    additional: usize,
    context: &'static str,
) -> Result<(), ExecutionPackageBuildError> {
    values.try_reserve_exact(additional).map_err(|error| {
        ExecutionPackageBuildError::Allocation(format!("could not allocate {context}: {error}"))
    })
}

fn reserve_map<K: Eq + std::hash::Hash, V>(
    values: &mut HashMap<K, V>,
    additional: usize,
    context: &'static str,
) -> Result<(), ExecutionPackageBuildError> {
    values.try_reserve(additional).map_err(|error| {
        ExecutionPackageBuildError::Allocation(format!("could not allocate {context}: {error}"))
    })
}

fn package_string(
    package: &ExecutionPackage,
    reference: StringRef,
) -> Result<&str, ExecutionPackageBuildError> {
    let index = usize::try_from(reference.index()).map_err(|_| {
        ExecutionPackageBuildError::AddressSpaceOverflow("package string reference")
    })?;
    package
        .strings
        .get(index)
        .map(String::as_str)
        .ok_or_else(|| link_mismatch("package string reference is out of range"))
}

fn try_owned_string(
    value: &str,
    context: &'static str,
) -> Result<String, ExecutionPackageBuildError> {
    let mut owned = String::new();
    owned.try_reserve_exact(value.len()).map_err(|error| {
        ExecutionPackageBuildError::Allocation(format!("could not allocate {context}: {error}"))
    })?;
    owned.push_str(value);
    Ok(owned)
}

fn primitive(ty: CoreType) -> PrimitiveType {
    match ty {
        CoreType::I32 => PrimitiveType::I32,
        CoreType::F32 => PrimitiveType::F32,
        CoreType::Bool => PrimitiveType::Bool,
    }
}

fn primitive_size(primitive: PrimitiveType) -> u64 {
    match primitive {
        PrimitiveType::I32 | PrimitiveType::F32 => 4,
        PrimitiveType::Bool => 1,
    }
}

fn primitive_alignment(primitive: PrimitiveType) -> u64 {
    primitive_size(primitive)
}

fn query_access(access: CoreQueryAccess) -> QueryAccess {
    match access {
        CoreQueryAccess::Read => QueryAccess::Read,
        CoreQueryAccess::Mut => QueryAccess::Mut,
        CoreQueryAccess::Exclude => QueryAccess::Exclude,
    }
}

fn query_access_code(access: CoreQueryAccess) -> u8 {
    match access {
        CoreQueryAccess::Read => 1,
        CoreQueryAccess::Mut => 2,
        CoreQueryAccess::Exclude => 3,
    }
}

fn type_code(ty: CoreType) -> u8 {
    primitive(ty) as u8
}

fn binary_code(op: CoreBinaryOp) -> u8 {
    match op {
        CoreBinaryOp::Add => 1,
        CoreBinaryOp::Subtract => 2,
        CoreBinaryOp::Multiply => 3,
        CoreBinaryOp::Divide => 4,
        CoreBinaryOp::Remainder => 5,
        CoreBinaryOp::ShiftLeft => 6,
        CoreBinaryOp::ShiftRight => 7,
        CoreBinaryOp::BitAnd => 8,
        CoreBinaryOp::BitXor => 9,
        CoreBinaryOp::BitOr => 10,
    }
}

fn unary_code(op: CoreUnaryOp) -> u8 {
    match op {
        CoreUnaryOp::Negate => 1,
        CoreUnaryOp::BitNot => 2,
    }
}

fn comparison_code(op: CoreComparisonOp) -> u8 {
    match op {
        CoreComparisonOp::Less => 1,
        CoreComparisonOp::LessEqual => 2,
        CoreComparisonOp::Greater => 3,
        CoreComparisonOp::GreaterEqual => 4,
    }
}

fn system_unary_code(op: CoreSystemUnaryOp) -> u8 {
    match op {
        CoreSystemUnaryOp::I32Negate => 1,
        CoreSystemUnaryOp::F32Negate => 2,
        CoreSystemUnaryOp::I32BitNot => 3,
        CoreSystemUnaryOp::BoolNot => 4,
    }
}

fn system_binary_code(op: CoreSystemBinaryOp) -> u8 {
    match op {
        CoreSystemBinaryOp::I32Add => 1,
        CoreSystemBinaryOp::I32Subtract => 2,
        CoreSystemBinaryOp::I32Multiply => 3,
        CoreSystemBinaryOp::I32Divide => 4,
        CoreSystemBinaryOp::I32Remainder => 5,
        CoreSystemBinaryOp::I32ShiftLeft => 6,
        CoreSystemBinaryOp::I32ShiftRight => 7,
        CoreSystemBinaryOp::I32BitAnd => 8,
        CoreSystemBinaryOp::I32BitXor => 9,
        CoreSystemBinaryOp::I32BitOr => 10,
        CoreSystemBinaryOp::F32Add => 11,
        CoreSystemBinaryOp::F32Subtract => 12,
        CoreSystemBinaryOp::F32Multiply => 13,
        CoreSystemBinaryOp::F32Divide => 14,
        CoreSystemBinaryOp::I32Less => 15,
        CoreSystemBinaryOp::I32LessEqual => 16,
        CoreSystemBinaryOp::I32Greater => 17,
        CoreSystemBinaryOp::I32GreaterEqual => 18,
        CoreSystemBinaryOp::F32Less => 19,
        CoreSystemBinaryOp::F32LessEqual => 20,
        CoreSystemBinaryOp::F32Greater => 21,
        CoreSystemBinaryOp::F32GreaterEqual => 22,
        CoreSystemBinaryOp::Equal => 23,
        CoreSystemBinaryOp::NotEqual => 24,
        CoreSystemBinaryOp::LogicalAnd => 25,
        CoreSystemBinaryOp::LogicalOr => 26,
    }
}

fn invalid_core(message: impl Into<String>) -> ExecutionPackageBuildError {
    ExecutionPackageBuildError::InvalidCore(message.into())
}

fn invalid_native(message: impl Into<String>) -> ExecutionPackageBuildError {
    ExecutionPackageBuildError::InvalidNativeLayout(message.into())
}

fn link_mismatch(message: impl Into<String>) -> ExecutionPackageBuildError {
    ExecutionPackageBuildError::InvalidCore(format!(
        "execution package link mismatch: {}",
        message.into()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use archec0::execution_package_v2::{decode_package_from, write_package};
    use archec0::ids_v2::BodyHash;
    use std::io::{BufReader, Cursor};

    fn encode_package_for_test(package: &ExecutionPackage) -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        write_package(&mut output, package).expect("package streams");
        output.into_inner()
    }

    const FULL_V2_SOURCE: &str = "world Demo
tag Enemy
component Empty {}
component Hidden {}
component Ready { marker: i32 }
resource Ready { enabled: bool }
system Find(state: mut Ready, units: query[Enemy, mut Empty, !Hidden]) {
  for (_, _) in units { state.enabled = !state.enabled }
}
schedule Main { run Find }
startup {
  resource Ready { enabled: true }
  spawn {}
  spawn { Enemy {} Empty {} }
  run Main
  exit 47
}";

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
        let system = &core.program().systems[0];
        NativeCodeLayout {
            code_range: CodeImageRange {
                offset: 0x1000,
                byte_len: 0x100,
            },
            functions: vec![
                NativeFunctionLayout {
                    target: NativeFunctionTarget::Startup,
                    symbol_name: "arche_startup".to_string(),
                    code_offset: 0x1000,
                    code_byte_len: 0x20,
                },
                NativeFunctionLayout {
                    target: NativeFunctionTarget::System(
                        ids.system(system.id).expect("system ID derives"),
                    ),
                    symbol_name: "arche_system_find".to_string(),
                    code_offset: 0x1020,
                    code_byte_len: 0x20,
                },
            ],
        }
    }

    #[test]
    fn builds_round_trips_and_links_a_full_canonical_v2_package() {
        let core = verified_source(FULL_V2_SOURCE);
        let native = native_layout(&core);
        let package =
            build_execution_package(&core, "full.arc", &native).expect("execution package builds");

        validate_execution_package_link(&core, &package, Some(native.code_range))
            .expect("package links to verified Core");
        let encoded = encode_package_for_test(&package);
        let decoded = decode_package_from(&mut Cursor::new(encoded)).expect("package decodes");
        assert_eq!(decoded, package);
        assert!(package
            .schemas
            .windows(2)
            .all(|pair| pair[0].id < pair[1].id));
        assert!(package
            .schemas
            .iter()
            .all(|schema| schema.flags == SchemaFlags::for_kind(schema.kind)));
        assert!(package.schemas.iter().any(|schema| {
            schema.kind == SchemaKind::Tag
                && schema.flags == SchemaFlags::TAG
                && schema.byte_size == 0
        }));
        assert!(package.terms.iter().any(|term| {
            term.access == QueryAccess::Mut
                && package.schemas[term.schema.index() as usize].kind == SchemaKind::Component
                && package.schemas[term.schema.index() as usize].byte_size == 0
        }));
        assert_eq!(
            package
                .schemas
                .iter()
                .filter(|schema| package.strings[schema.name.index() as usize] == "Ready")
                .count(),
            2,
            "component and resource names occupy separate namespaces"
        );
        assert!(package.startup_operations.iter().any(|operation| matches!(
            operation.kind,
            StartupOperationKind::Spawn {
                payload_count: 0,
                ..
            }
        )));
        assert!(package.function_links.iter().all(|link| {
            link.source_span.is_some() && link.first_body_span.is_some() && link.body_span_count > 0
        }));
        assert!(!package.source_spans.is_empty());
        assert!(package
            .source_spans
            .iter()
            .all(|span| package.strings[span.file_name.index() as usize] == "full.arc"));
    }

    #[test]
    fn link_gate_allows_metadata_payload_edits_but_rejects_coherent_hash_tampering() {
        let core = verified_source(FULL_V2_SOURCE);
        let native = native_layout(&core);
        let mut package =
            build_execution_package(&core, "full.arc", &native).expect("execution package builds");

        package.payloads[0].bytes[0] ^= 1;
        validate_execution_package_link(&core, &package, Some(native.code_range))
            .expect("coherent metadata payload edit remains authoritative");

        let forged = BodyHash::from_bytes([0xA5; 16]);
        package.systems[0].body_hash = forged;
        package.function_links[1].body_hash = forged;
        assert!(validate_package_with_code_range(&package, native.code_range).is_ok());
        assert!(matches!(
            validate_execution_package_link(&core, &package, Some(native.code_range)),
            Err(ExecutionPackageBuildError::InvalidCore(_))
        ));
    }

    #[test]
    fn builder_rejects_duplicate_native_targets_before_publication() {
        let core = verified_source(FULL_V2_SOURCE);
        let mut native = native_layout(&core);
        native.functions[1].target = NativeFunctionTarget::Startup;
        assert!(matches!(
            build_execution_package(&core, "full.arc", &native),
            Err(ExecutionPackageBuildError::InvalidNativeLayout(_))
        ));
    }
}
