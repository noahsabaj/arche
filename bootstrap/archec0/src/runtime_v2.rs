use crate::execution_package_v2::{
    self, CodeImageRange, ExecutionPackage, ExecutionPackageV2Error, FunctionTarget, ParameterKind,
    QueryAccess, ScheduleItemKind, StartupOperationKind,
};
use crate::ids_v2::{AbiHash, BodyHash, DeclId, PrimitiveType, SchemaId, SchemaKind};
use std::fmt;
use std::io::{Read, Seek};

macro_rules! dense_index {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(index: u64) -> Self {
                Self(index)
            }

            pub const fn index(self) -> u64 {
                self.0
            }
        }
    };
}

dense_index!(SchemaIndex);
dense_index!(SystemIndex);
dense_index!(QueryIndex);
dense_index!(ScheduleIndex);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldDescriptor {
    name: String,
    primitive: PrimitiveType,
    byte_offset: u64,
}

impl FieldDescriptor {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn primitive(&self) -> PrimitiveType {
        self.primitive
    }

    pub const fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaDescriptor {
    index: SchemaIndex,
    id: SchemaId,
    kind: SchemaKind,
    name: String,
    byte_size: u64,
    alignment: u64,
    fields: Vec<FieldDescriptor>,
}

impl SchemaDescriptor {
    pub const fn index(&self) -> SchemaIndex {
        self.index
    }

    pub const fn id(&self) -> SchemaId {
        self.id
    }

    pub const fn kind(&self) -> SchemaKind {
        self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn byte_size(&self) -> u64 {
        self.byte_size
    }

    pub const fn alignment(&self) -> u64 {
        self.alignment
    }

    pub fn fields(&self) -> &[FieldDescriptor] {
        &self.fields
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemParameterKind {
    ReadResource { resource: SchemaIndex },
    MutResource { resource: SchemaIndex },
    Query { query: QueryIndex },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemParameterDescriptor {
    name: String,
    kind: SystemParameterKind,
}

impl SystemParameterDescriptor {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn kind(&self) -> SystemParameterKind {
        self.kind
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFunctionDescriptor {
    symbol_name: String,
    abi_hash: AbiHash,
    body_hash: BodyHash,
    code_offset: u64,
    code_byte_len: u64,
}

impl NativeFunctionDescriptor {
    pub fn symbol_name(&self) -> &str {
        &self.symbol_name
    }

    pub const fn abi_hash(&self) -> AbiHash {
        self.abi_hash
    }

    pub const fn body_hash(&self) -> BodyHash {
        self.body_hash
    }

    pub const fn code_offset(&self) -> u64 {
        self.code_offset
    }

    pub const fn code_byte_len(&self) -> u64 {
        self.code_byte_len
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemDescriptor {
    index: SystemIndex,
    id: DeclId,
    name: String,
    abi_hash: AbiHash,
    body_hash: BodyHash,
    parameters: Vec<SystemParameterDescriptor>,
    function: NativeFunctionDescriptor,
}

impl SystemDescriptor {
    pub const fn index(&self) -> SystemIndex {
        self.index
    }

    pub const fn id(&self) -> DeclId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn abi_hash(&self) -> AbiHash {
        self.abi_hash
    }

    pub const fn body_hash(&self) -> BodyHash {
        self.body_hash
    }

    pub fn parameters(&self) -> &[SystemParameterDescriptor] {
        &self.parameters
    }

    pub fn function(&self) -> &NativeFunctionDescriptor {
        &self.function
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueryTermDescriptor {
    access: QueryAccess,
    schema: SchemaIndex,
}

impl QueryTermDescriptor {
    pub const fn access(&self) -> QueryAccess {
        self.access
    }

    pub const fn schema(&self) -> SchemaIndex {
        self.schema
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryDescriptor {
    index: QueryIndex,
    id: DeclId,
    system: SystemIndex,
    terms: Vec<QueryTermDescriptor>,
}

impl QueryDescriptor {
    pub const fn index(&self) -> QueryIndex {
        self.index
    }

    pub const fn id(&self) -> DeclId {
        self.id
    }

    pub const fn system(&self) -> SystemIndex {
        self.system
    }

    pub fn terms(&self) -> &[QueryTermDescriptor] {
        &self.terms
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduleDescriptor {
    index: ScheduleIndex,
    id: DeclId,
    name: String,
    systems: Vec<SystemIndex>,
}

impl ScheduleDescriptor {
    pub const fn index(&self) -> ScheduleIndex {
        self.index
    }

    pub const fn id(&self) -> DeclId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn systems(&self) -> &[SystemIndex] {
        &self.systems
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceState<'a> {
    Uninitialized,
    Initialized(&'a [u8]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceView<'a> {
    schema: &'a SchemaDescriptor,
    state: ResourceState<'a>,
}

impl<'a> ResourceView<'a> {
    pub const fn schema(&self) -> &'a SchemaDescriptor {
        self.schema
    }

    pub const fn state(&self) -> ResourceState<'a> {
        self.state
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResourceSlot {
    schema: SchemaIndex,
    bytes: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredColumn {
    schema_id: SchemaId,
    bytes: Vec<u8>,
}

impl StoredColumn {
    pub const fn schema_id(&self) -> SchemaId {
        self.schema_id
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityRow {
    spawn_ordinal: u64,
    columns: Vec<StoredColumn>,
}

impl EntityRow {
    pub const fn spawn_ordinal(&self) -> u64 {
        self.spawn_ordinal
    }

    pub fn columns(&self) -> &[StoredColumn] {
        &self.columns
    }

    pub fn column(&self, schema_id: SchemaId) -> Option<&StoredColumn> {
        self.columns
            .binary_search_by_key(&schema_id, StoredColumn::schema_id)
            .ok()
            .map(|index| &self.columns[index])
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchetypeTable {
    key: Vec<SchemaId>,
    rows: Vec<EntityRow>,
}

impl ArchetypeTable {
    pub fn key(&self) -> &[SchemaId] {
        &self.key
    }

    pub fn rows(&self) -> &[EntityRow] {
        &self.rows
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchemaPayload<'a> {
    pub schema_id: SchemaId,
    pub bytes: &'a [u8],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupPayload {
    schema_id: SchemaId,
    bytes: Vec<u8>,
}

impl StartupPayload {
    pub const fn schema_id(&self) -> SchemaId {
        self.schema_id
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum OwnedStartupOperation {
    InitializeResource(StartupPayload),
    Spawn(Vec<StartupPayload>),
    RunSchedule(ScheduleIndex),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupOperationView<'a> {
    InitializeResource(SchemaPayload<'a>),
    Spawn(&'a [StartupPayload]),
    RunSchedule(ScheduleIndex),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupState {
    Ready,
    Running,
    Complete,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SystemInvocation {
    system: SystemIndex,
    id: DeclId,
}

impl SystemInvocation {
    pub const fn system(&self) -> SystemIndex {
        self.system
    }

    pub const fn id(&self) -> DeclId {
        self.id
    }
}

pub trait SystemDispatcher {
    type Error;

    fn dispatch(
        &mut self,
        world: &mut RuntimeWorldV2,
        invocation: SystemInvocation,
    ) -> Result<(), Self::Error>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeV2Error {
    Package(ExecutionPackageV2Error),
    HostIndexOverflow {
        table: &'static str,
        index: u64,
    },
    DenseIndexOverflow {
        table: &'static str,
    },
    UnknownSchema(SchemaId),
    UnknownSystem(DeclId),
    UnknownQuery(DeclId),
    UnknownSchedule(DeclId),
    WrongSchemaKind {
        schema: SchemaId,
        expected: &'static str,
    },
    PayloadLength {
        schema: SchemaId,
        expected: u64,
        actual: u64,
    },
    InvalidBoolPayload {
        schema: SchemaId,
        byte_offset: u64,
        actual: u8,
    },
    DuplicateSchema(SchemaId),
    ResourceAlreadyInitialized(SchemaId),
    ResourceUninitialized(SchemaId),
    UnknownSpawnOrdinal(u64),
    SchemaNotInRow {
        spawn_ordinal: u64,
        schema: SchemaId,
    },
    ZeroSizedMutation(SchemaId),
    SpawnOrdinalOverflow,
    AllocationFailed {
        context: &'static str,
    },
    StartupAlreadyExecuted,
}

impl fmt::Display for RuntimeV2Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Package(error) => write!(formatter, "{error}"),
            Self::HostIndexOverflow { table, index } => {
                write!(
                    formatter,
                    "{table} dense index {index} does not fit the host"
                )
            }
            Self::DenseIndexOverflow { table } => {
                write!(
                    formatter,
                    "{table} count does not fit the v2 u64 index space"
                )
            }
            Self::UnknownSchema(id) => write!(formatter, "unknown schema {id}"),
            Self::UnknownSystem(id) => write!(formatter, "unknown system {id}"),
            Self::UnknownQuery(id) => write!(formatter, "unknown query {id}"),
            Self::UnknownSchedule(id) => write!(formatter, "unknown schedule {id}"),
            Self::WrongSchemaKind { schema, expected } => {
                write!(formatter, "schema {schema} is not {expected}")
            }
            Self::PayloadLength {
                schema,
                expected,
                actual,
            } => write!(
                formatter,
                "schema {schema} requires {expected} payload bytes, but received {actual}"
            ),
            Self::InvalidBoolPayload {
                schema,
                byte_offset,
                actual,
            } => write!(
                formatter,
                "schema {schema} bool field at byte {byte_offset} is encoded as {actual}, not 0 or 1"
            ),
            Self::DuplicateSchema(id) => write!(formatter, "schema {id} occurs more than once"),
            Self::ResourceAlreadyInitialized(id) => {
                write!(formatter, "resource {id} is already initialized")
            }
            Self::ResourceUninitialized(id) => write!(formatter, "resource {id} is uninitialized"),
            Self::UnknownSpawnOrdinal(ordinal) => {
                write!(formatter, "spawn ordinal {ordinal} does not exist")
            }
            Self::SchemaNotInRow {
                spawn_ordinal,
                schema,
            } => write!(
                formatter,
                "spawn ordinal {spawn_ordinal} does not contain schema {schema}"
            ),
            Self::ZeroSizedMutation(id) => {
                write!(formatter, "zero-sized schema {id} cannot be mutated")
            }
            Self::SpawnOrdinalOverflow => formatter.write_str("spawn ordinal overflows u64"),
            Self::AllocationFailed { context } => {
                write!(formatter, "allocation failed for {context}")
            }
            Self::StartupAlreadyExecuted => {
                formatter.write_str("startup execution has already been attempted")
            }
        }
    }
}

impl std::error::Error for RuntimeV2Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Package(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ExecutionPackageV2Error> for RuntimeV2Error {
    fn from(error: ExecutionPackageV2Error) -> Self {
        Self::Package(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartupExecutionError<E> {
    Runtime(RuntimeV2Error),
    Dispatch(E),
}

impl<E: fmt::Display> fmt::Display for StartupExecutionError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Runtime(error) => write!(formatter, "{error}"),
            Self::Dispatch(error) => write!(formatter, "system dispatch failed: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for StartupExecutionError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Runtime(error) => Some(error),
            Self::Dispatch(error) => Some(error),
        }
    }
}

pub struct QueryTableIter<'a> {
    schemas: &'a [SchemaDescriptor],
    query: &'a QueryDescriptor,
    tables: std::slice::Iter<'a, ArchetypeTable>,
}

impl<'a> Iterator for QueryTableIter<'a> {
    type Item = &'a ArchetypeTable;

    fn next(&mut self) -> Option<Self::Item> {
        self.tables
            .find(|table| query_matches(self.schemas, self.query, table))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeWorldV2 {
    world_name: String,
    schemas: Vec<SchemaDescriptor>,
    resources: Vec<ResourceSlot>,
    systems: Vec<SystemDescriptor>,
    queries: Vec<QueryDescriptor>,
    schedules: Vec<ScheduleDescriptor>,
    startup_function: NativeFunctionDescriptor,
    startup_operations: Vec<OwnedStartupOperation>,
    startup_state: StartupState,
    tables: Vec<ArchetypeTable>,
    next_spawn_ordinal: u64,
}

impl RuntimeWorldV2 {
    pub fn from_package(package: ExecutionPackage) -> Result<Self, RuntimeV2Error> {
        execution_package_v2::validate_package(&package)?;
        Self::from_validated_package(package)
    }

    #[cfg(test)]
    pub fn decode(metadata: &[u8]) -> Result<Self, RuntimeV2Error> {
        let package = execution_package_v2::decode_package(metadata)?;
        Self::from_validated_package(package)
    }

    #[cfg(test)]
    pub fn decode_with_code_range(
        metadata: &[u8],
        code_range: CodeImageRange,
    ) -> Result<Self, RuntimeV2Error> {
        let package = execution_package_v2::decode_package_with_code_range(metadata, code_range)?;
        Self::from_validated_package(package)
    }

    pub fn decode_from<R: Read + Seek>(input: &mut R) -> Result<Self, RuntimeV2Error> {
        let package = execution_package_v2::decode_package_from(input)?;
        Self::from_validated_package(package)
    }

    pub fn decode_from_with_code_range<R: Read + Seek>(
        input: &mut R,
        code_range: CodeImageRange,
    ) -> Result<Self, RuntimeV2Error> {
        let package = execution_package_v2::decode_package_from_with_code_range(input, code_range)?;
        Self::from_validated_package(package)
    }

    fn from_validated_package(package: ExecutionPackage) -> Result<Self, RuntimeV2Error> {
        let schemas = build_schemas(&package)?;
        let resources = build_resource_slots(&schemas)?;
        let queries = build_queries(&package)?;
        let systems = build_systems(&package)?;
        let schedules = build_schedules(&package)?;
        let startup_function = function_descriptor(&package, 0)?;
        let startup_operations = build_startup_operations(&package, &schemas)?;
        let world_name = copy_string(
            package_string(&package, package.world.name.index())?,
            "world name",
        )?;

        Ok(Self {
            world_name,
            schemas,
            resources,
            systems,
            queries,
            schedules,
            startup_function,
            startup_operations,
            startup_state: StartupState::Ready,
            tables: Vec::new(),
            next_spawn_ordinal: 0,
        })
    }

    pub fn world_name(&self) -> &str {
        &self.world_name
    }

    pub fn schemas(&self) -> &[SchemaDescriptor] {
        &self.schemas
    }

    pub fn systems(&self) -> &[SystemDescriptor] {
        &self.systems
    }

    pub fn queries(&self) -> &[QueryDescriptor] {
        &self.queries
    }

    pub fn schedules(&self) -> &[ScheduleDescriptor] {
        &self.schedules
    }

    pub fn startup_function(&self) -> &NativeFunctionDescriptor {
        &self.startup_function
    }

    pub const fn startup_state(&self) -> StartupState {
        self.startup_state
    }

    pub fn tables(&self) -> &[ArchetypeTable] {
        &self.tables
    }

    pub const fn next_spawn_ordinal(&self) -> u64 {
        self.next_spawn_ordinal
    }

    pub fn schema_index(&self, id: SchemaId) -> Option<SchemaIndex> {
        self.schemas
            .binary_search_by_key(&id, SchemaDescriptor::id)
            .ok()
            .and_then(|index| u64::try_from(index).ok())
            .map(SchemaIndex::new)
    }

    pub fn system_index(&self, id: DeclId) -> Option<SystemIndex> {
        self.systems
            .binary_search_by_key(&id, SystemDescriptor::id)
            .ok()
            .and_then(|index| u64::try_from(index).ok())
            .map(SystemIndex::new)
    }

    pub fn query_index(&self, id: DeclId) -> Option<QueryIndex> {
        self.queries
            .binary_search_by_key(&id, QueryDescriptor::id)
            .ok()
            .and_then(|index| u64::try_from(index).ok())
            .map(QueryIndex::new)
    }

    pub fn schedule_index(&self, id: DeclId) -> Option<ScheduleIndex> {
        self.schedules
            .binary_search_by_key(&id, ScheduleDescriptor::id)
            .ok()
            .and_then(|index| u64::try_from(index).ok())
            .map(ScheduleIndex::new)
    }

    pub fn schema(&self, index: SchemaIndex) -> Result<&SchemaDescriptor, RuntimeV2Error> {
        indexed(&self.schemas, index.index(), "schemas")
    }

    pub fn system(&self, index: SystemIndex) -> Result<&SystemDescriptor, RuntimeV2Error> {
        indexed(&self.systems, index.index(), "systems")
    }

    pub fn query(&self, index: QueryIndex) -> Result<&QueryDescriptor, RuntimeV2Error> {
        indexed(&self.queries, index.index(), "queries")
    }

    pub fn schedule(&self, index: ScheduleIndex) -> Result<&ScheduleDescriptor, RuntimeV2Error> {
        indexed(&self.schedules, index.index(), "schedules")
    }

    pub fn resources(&self) -> impl ExactSizeIterator<Item = ResourceView<'_>> {
        self.resources.iter().map(|resource| {
            let schema = indexed(&self.schemas, resource.schema.index(), "schemas")
                .expect("validated resource slots have valid schema indexes");
            let state = resource
                .bytes
                .as_deref()
                .map_or(ResourceState::Uninitialized, ResourceState::Initialized);
            ResourceView { schema, state }
        })
    }

    pub fn resource(&self, id: SchemaId) -> Result<ResourceView<'_>, RuntimeV2Error> {
        let schema = self.schema_by_id(id)?;
        if schema.kind != SchemaKind::Resource {
            return Err(RuntimeV2Error::WrongSchemaKind {
                schema: id,
                expected: "a resource",
            });
        }
        let resource = self.resource_slot(id)?;
        let state = resource
            .bytes
            .as_deref()
            .map_or(ResourceState::Uninitialized, ResourceState::Initialized);
        Ok(ResourceView { schema, state })
    }

    pub fn initialize_resource(
        &mut self,
        id: SchemaId,
        bytes: &[u8],
    ) -> Result<(), RuntimeV2Error> {
        self.validate_resource_payload(id, bytes)?;
        let slot_index = self.resource_slot_index(id)?;
        if self.resources[slot_index].bytes.is_some() {
            return Err(RuntimeV2Error::ResourceAlreadyInitialized(id));
        }
        let bytes = copy_bytes(bytes, "resource payload")?;
        self.resources[slot_index].bytes = Some(bytes);
        Ok(())
    }

    pub fn assign_resource(&mut self, id: SchemaId, bytes: &[u8]) -> Result<(), RuntimeV2Error> {
        self.validate_resource_payload(id, bytes)?;
        let slot_index = self.resource_slot_index(id)?;
        if self.resources[slot_index].bytes.is_none() {
            return Err(RuntimeV2Error::ResourceUninitialized(id));
        }
        let bytes = copy_bytes(bytes, "resource assignment")?;
        self.resources[slot_index].bytes = Some(bytes);
        Ok(())
    }

    pub fn spawn(&mut self, payloads: &[SchemaPayload<'_>]) -> Result<u64, RuntimeV2Error> {
        let columns = self.prepare_columns(payloads)?;
        self.commit_spawn(columns)
    }

    pub fn assign_row(
        &mut self,
        spawn_ordinal: u64,
        assignments: &[SchemaPayload<'_>],
    ) -> Result<(), RuntimeV2Error> {
        let (table_index, row_index) = self
            .row_location(spawn_ordinal)
            .ok_or(RuntimeV2Error::UnknownSpawnOrdinal(spawn_ordinal))?;
        let row = &self.tables[table_index].rows[row_index];
        let mut prepared = Vec::new();
        prepared.try_reserve_exact(assignments.len()).map_err(|_| {
            RuntimeV2Error::AllocationFailed {
                context: "row assignments",
            }
        })?;
        for assignment in assignments {
            let schema = self.schema_by_id(assignment.schema_id)?;
            if schema.kind == SchemaKind::Resource {
                return Err(RuntimeV2Error::WrongSchemaKind {
                    schema: assignment.schema_id,
                    expected: "an entity schema",
                });
            }
            if schema.byte_size == 0 {
                return Err(RuntimeV2Error::ZeroSizedMutation(assignment.schema_id));
            }
            validate_payload(schema, assignment.bytes)?;
            let column_index = row
                .columns
                .binary_search_by_key(&assignment.schema_id, StoredColumn::schema_id)
                .map_err(|_| RuntimeV2Error::SchemaNotInRow {
                    spawn_ordinal,
                    schema: assignment.schema_id,
                })?;
            prepared.push((
                column_index,
                assignment.schema_id,
                copy_bytes(assignment.bytes, "row assignment")?,
            ));
        }
        prepared.sort_unstable_by_key(|(column, _, _)| *column);
        for pair in prepared.windows(2) {
            if pair[0].0 == pair[1].0 {
                return Err(RuntimeV2Error::DuplicateSchema(pair[1].1));
            }
        }
        let row = &mut self.tables[table_index].rows[row_index];
        for (column_index, _, bytes) in prepared {
            row.columns[column_index].bytes = bytes;
        }
        Ok(())
    }

    pub fn row(&self, spawn_ordinal: u64) -> Option<&EntityRow> {
        self.row_location(spawn_ordinal)
            .map(|(table, row)| &self.tables[table].rows[row])
    }

    pub fn matching_tables(&self, query: QueryIndex) -> Result<QueryTableIter<'_>, RuntimeV2Error> {
        let query = self.query(query)?;
        Ok(QueryTableIter {
            schemas: &self.schemas,
            query,
            tables: self.tables.iter(),
        })
    }

    pub fn startup_operations(&self) -> impl ExactSizeIterator<Item = StartupOperationView<'_>> {
        self.startup_operations
            .iter()
            .map(|operation| match operation {
                OwnedStartupOperation::InitializeResource(payload) => {
                    StartupOperationView::InitializeResource(SchemaPayload {
                        schema_id: payload.schema_id,
                        bytes: &payload.bytes,
                    })
                }
                OwnedStartupOperation::Spawn(payloads) => StartupOperationView::Spawn(payloads),
                OwnedStartupOperation::RunSchedule(schedule) => {
                    StartupOperationView::RunSchedule(*schedule)
                }
            })
    }

    pub fn execute_startup<D: SystemDispatcher>(
        &mut self,
        dispatcher: &mut D,
    ) -> Result<(), StartupExecutionError<D::Error>> {
        if self.startup_state != StartupState::Ready {
            return Err(StartupExecutionError::Runtime(
                RuntimeV2Error::StartupAlreadyExecuted,
            ));
        }
        self.startup_state = StartupState::Running;
        let result = self.execute_startup_operations(dispatcher);
        self.startup_state = if result.is_ok() {
            StartupState::Complete
        } else {
            StartupState::Failed
        };
        result
    }

    fn execute_startup_operations<D: SystemDispatcher>(
        &mut self,
        dispatcher: &mut D,
    ) -> Result<(), StartupExecutionError<D::Error>> {
        for operation_index in 0..self.startup_operations.len() {
            let operation = self.startup_operations[operation_index].clone();
            match operation {
                OwnedStartupOperation::InitializeResource(payload) => self
                    .initialize_resource(payload.schema_id, &payload.bytes)
                    .map_err(StartupExecutionError::Runtime)?,
                OwnedStartupOperation::Spawn(payloads) => {
                    let mut borrowed = Vec::new();
                    borrowed.try_reserve_exact(payloads.len()).map_err(|_| {
                        StartupExecutionError::Runtime(RuntimeV2Error::AllocationFailed {
                            context: "borrowed startup spawn payloads",
                        })
                    })?;
                    borrowed.extend(payloads.iter().map(|payload| SchemaPayload {
                        schema_id: payload.schema_id,
                        bytes: &payload.bytes,
                    }));
                    self.spawn(&borrowed)
                        .map_err(StartupExecutionError::Runtime)?;
                }
                OwnedStartupOperation::RunSchedule(schedule) => {
                    let systems = self
                        .schedule(schedule)
                        .map_err(StartupExecutionError::Runtime)?
                        .systems
                        .clone();
                    for system in systems {
                        let descriptor = self
                            .system(system)
                            .map_err(StartupExecutionError::Runtime)?;
                        let invocation = SystemInvocation {
                            system,
                            id: descriptor.id,
                        };
                        dispatcher
                            .dispatch(self, invocation)
                            .map_err(StartupExecutionError::Dispatch)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn schema_by_id(&self, id: SchemaId) -> Result<&SchemaDescriptor, RuntimeV2Error> {
        self.schema_index(id)
            .ok_or(RuntimeV2Error::UnknownSchema(id))
            .and_then(|index| self.schema(index))
    }

    fn resource_slot(&self, id: SchemaId) -> Result<&ResourceSlot, RuntimeV2Error> {
        let index = self.resource_slot_index(id)?;
        Ok(&self.resources[index])
    }

    fn resource_slot_index(&self, id: SchemaId) -> Result<usize, RuntimeV2Error> {
        self.resources
            .binary_search_by_key(&id, |resource| {
                self.schemas[host_index(resource.schema.index(), "schemas").expect(
                    "validated resource slots have schema indexes that fit the current host",
                )]
                .id
            })
            .map_err(|_| RuntimeV2Error::WrongSchemaKind {
                schema: id,
                expected: "a resource",
            })
    }

    fn validate_resource_payload(&self, id: SchemaId, bytes: &[u8]) -> Result<(), RuntimeV2Error> {
        let schema = self.schema_by_id(id)?;
        if schema.kind != SchemaKind::Resource {
            return Err(RuntimeV2Error::WrongSchemaKind {
                schema: id,
                expected: "a resource",
            });
        }
        validate_payload(schema, bytes)
    }

    fn prepare_columns(
        &self,
        payloads: &[SchemaPayload<'_>],
    ) -> Result<Vec<StoredColumn>, RuntimeV2Error> {
        let mut columns = Vec::new();
        columns.try_reserve_exact(payloads.len()).map_err(|_| {
            RuntimeV2Error::AllocationFailed {
                context: "spawn columns",
            }
        })?;
        for payload in payloads {
            let schema = self.schema_by_id(payload.schema_id)?;
            if schema.kind == SchemaKind::Resource {
                return Err(RuntimeV2Error::WrongSchemaKind {
                    schema: payload.schema_id,
                    expected: "an entity schema",
                });
            }
            validate_payload(schema, payload.bytes)?;
            columns.push(StoredColumn {
                schema_id: payload.schema_id,
                bytes: copy_bytes(payload.bytes, "spawn payload")?,
            });
        }
        columns.sort_unstable_by_key(StoredColumn::schema_id);
        for pair in columns.windows(2) {
            if pair[0].schema_id == pair[1].schema_id {
                return Err(RuntimeV2Error::DuplicateSchema(pair[1].schema_id));
            }
        }
        Ok(columns)
    }

    fn commit_spawn(&mut self, columns: Vec<StoredColumn>) -> Result<u64, RuntimeV2Error> {
        let ordinal = self.next_spawn_ordinal;
        let next_ordinal = ordinal
            .checked_add(1)
            .ok_or(RuntimeV2Error::SpawnOrdinalOverflow)?;
        let mut key = Vec::new();
        key.try_reserve_exact(columns.len())
            .map_err(|_| RuntimeV2Error::AllocationFailed {
                context: "archetype key",
            })?;
        key.extend(columns.iter().map(StoredColumn::schema_id));
        let row = EntityRow {
            spawn_ordinal: ordinal,
            columns,
        };
        match self.tables.binary_search_by(|table| table.key.cmp(&key)) {
            Ok(table_index) => {
                self.tables[table_index].rows.try_reserve(1).map_err(|_| {
                    RuntimeV2Error::AllocationFailed {
                        context: "archetype rows",
                    }
                })?;
                self.tables[table_index].rows.push(row);
            }
            Err(table_index) => {
                let mut rows = Vec::new();
                rows.try_reserve(1)
                    .map_err(|_| RuntimeV2Error::AllocationFailed {
                        context: "archetype rows",
                    })?;
                rows.push(row);
                self.tables
                    .try_reserve(1)
                    .map_err(|_| RuntimeV2Error::AllocationFailed {
                        context: "archetype tables",
                    })?;
                self.tables
                    .insert(table_index, ArchetypeTable { key, rows });
            }
        }
        self.next_spawn_ordinal = next_ordinal;
        Ok(ordinal)
    }

    fn row_location(&self, spawn_ordinal: u64) -> Option<(usize, usize)> {
        self.tables
            .iter()
            .enumerate()
            .find_map(|(table_index, table)| {
                table
                    .rows
                    .binary_search_by_key(&spawn_ordinal, EntityRow::spawn_ordinal)
                    .ok()
                    .map(|row_index| (table_index, row_index))
            })
    }
}

fn build_schemas(package: &ExecutionPackage) -> Result<Vec<SchemaDescriptor>, RuntimeV2Error> {
    let mut schemas = Vec::new();
    schemas
        .try_reserve_exact(package.schemas.len())
        .map_err(|_| RuntimeV2Error::AllocationFailed {
            context: "schema descriptors",
        })?;
    for (schema_position, schema) in package.schemas.iter().enumerate() {
        let index = SchemaIndex::new(dense_position(schema_position, "schemas")?);
        let mut fields = Vec::new();
        let field_count = package
            .fields
            .iter()
            .filter(|field| field.schema.index() == index.index())
            .count();
        fields
            .try_reserve_exact(field_count)
            .map_err(|_| RuntimeV2Error::AllocationFailed {
                context: "field descriptors",
            })?;
        for field in package
            .fields
            .iter()
            .filter(|field| field.schema.index() == index.index())
        {
            fields.push(FieldDescriptor {
                name: copy_string(package_string(package, field.name.index())?, "field name")?,
                primitive: field.primitive,
                byte_offset: field.byte_offset,
            });
        }
        schemas.push(SchemaDescriptor {
            index,
            id: schema.id,
            kind: schema.kind,
            name: copy_string(package_string(package, schema.name.index())?, "schema name")?,
            byte_size: schema.byte_size,
            alignment: schema.alignment,
            fields,
        });
    }
    Ok(schemas)
}

fn build_resource_slots(schemas: &[SchemaDescriptor]) -> Result<Vec<ResourceSlot>, RuntimeV2Error> {
    let count = schemas
        .iter()
        .filter(|schema| schema.kind == SchemaKind::Resource)
        .count();
    let mut resources = Vec::new();
    resources
        .try_reserve_exact(count)
        .map_err(|_| RuntimeV2Error::AllocationFailed {
            context: "resource descriptors",
        })?;
    resources.extend(
        schemas
            .iter()
            .filter(|schema| schema.kind == SchemaKind::Resource)
            .map(|schema| ResourceSlot {
                schema: schema.index,
                bytes: None,
            }),
    );
    Ok(resources)
}

fn build_queries(package: &ExecutionPackage) -> Result<Vec<QueryDescriptor>, RuntimeV2Error> {
    let mut queries = Vec::new();
    queries
        .try_reserve_exact(package.queries.len())
        .map_err(|_| RuntimeV2Error::AllocationFailed {
            context: "query descriptors",
        })?;
    for (position, query) in package.queries.iter().enumerate() {
        let index = QueryIndex::new(dense_position(position, "queries")?);
        let mut terms = Vec::new();
        let term_count = package
            .terms
            .iter()
            .filter(|term| term.query.index() == index.index())
            .count();
        terms
            .try_reserve_exact(term_count)
            .map_err(|_| RuntimeV2Error::AllocationFailed {
                context: "query terms",
            })?;
        terms.extend(
            package
                .terms
                .iter()
                .filter(|term| term.query.index() == index.index())
                .map(|term| QueryTermDescriptor {
                    access: term.access,
                    schema: SchemaIndex::new(term.schema.index()),
                }),
        );
        queries.push(QueryDescriptor {
            index,
            id: query.id,
            system: SystemIndex::new(query.system.index()),
            terms,
        });
    }
    Ok(queries)
}

fn build_systems(package: &ExecutionPackage) -> Result<Vec<SystemDescriptor>, RuntimeV2Error> {
    let mut systems = Vec::new();
    systems
        .try_reserve_exact(package.systems.len())
        .map_err(|_| RuntimeV2Error::AllocationFailed {
            context: "system descriptors",
        })?;
    for (position, system) in package.systems.iter().enumerate() {
        let index = SystemIndex::new(dense_position(position, "systems")?);
        let parameter_count = package
            .parameters
            .iter()
            .filter(|parameter| parameter.system.index() == index.index())
            .count();
        let mut parameters = Vec::new();
        parameters.try_reserve_exact(parameter_count).map_err(|_| {
            RuntimeV2Error::AllocationFailed {
                context: "system parameters",
            }
        })?;
        for parameter in package
            .parameters
            .iter()
            .filter(|parameter| parameter.system.index() == index.index())
        {
            let kind = match parameter.kind {
                ParameterKind::ReadResource { resource } => SystemParameterKind::ReadResource {
                    resource: SchemaIndex::new(resource.index()),
                },
                ParameterKind::MutResource { resource } => SystemParameterKind::MutResource {
                    resource: SchemaIndex::new(resource.index()),
                },
                ParameterKind::Query { query } => SystemParameterKind::Query {
                    query: QueryIndex::new(query.index()),
                },
            };
            parameters.push(SystemParameterDescriptor {
                name: copy_string(
                    package_string(package, parameter.name.index())?,
                    "parameter name",
                )?,
                kind,
            });
        }
        systems.push(SystemDescriptor {
            index,
            id: system.id,
            name: copy_string(package_string(package, system.name.index())?, "system name")?,
            abi_hash: system.abi_hash,
            body_hash: system.body_hash,
            parameters,
            function: function_descriptor(package, position + 1)?,
        });
    }
    Ok(systems)
}

fn build_schedules(package: &ExecutionPackage) -> Result<Vec<ScheduleDescriptor>, RuntimeV2Error> {
    let mut schedules = Vec::new();
    schedules
        .try_reserve_exact(package.schedules.len())
        .map_err(|_| RuntimeV2Error::AllocationFailed {
            context: "schedule descriptors",
        })?;
    for (position, schedule) in package.schedules.iter().enumerate() {
        let index = ScheduleIndex::new(dense_position(position, "schedules")?);
        let item_count = package
            .schedule_items
            .iter()
            .filter(|item| item.schedule.index() == index.index())
            .count();
        let mut systems = Vec::new();
        systems
            .try_reserve_exact(item_count)
            .map_err(|_| RuntimeV2Error::AllocationFailed {
                context: "schedule items",
            })?;
        for item in package
            .schedule_items
            .iter()
            .filter(|item| item.schedule.index() == index.index())
        {
            let ScheduleItemKind::RunSystem { system } = item.kind;
            systems.push(SystemIndex::new(system.index()));
        }
        schedules.push(ScheduleDescriptor {
            index,
            id: schedule.id,
            name: copy_string(
                package_string(package, schedule.name.index())?,
                "schedule name",
            )?,
            systems,
        });
    }
    Ok(schedules)
}

fn build_startup_operations(
    package: &ExecutionPackage,
    schemas: &[SchemaDescriptor],
) -> Result<Vec<OwnedStartupOperation>, RuntimeV2Error> {
    let mut operations = Vec::new();
    operations
        .try_reserve_exact(package.startup_operations.len())
        .map_err(|_| RuntimeV2Error::AllocationFailed {
            context: "startup operations",
        })?;
    for operation in &package.startup_operations {
        let owned = match operation.kind {
            StartupOperationKind::ResourcePayload { resource, payload } => {
                let payload = indexed(&package.payloads, payload.index(), "payloads")?;
                let schema = indexed(schemas, resource.index(), "schemas")?;
                OwnedStartupOperation::InitializeResource(StartupPayload {
                    schema_id: schema.id,
                    bytes: copy_bytes(&payload.bytes, "startup resource payload")?,
                })
            }
            StartupOperationKind::Spawn {
                first_payload,
                payload_count,
            } => {
                let first = host_index(first_payload.index(), "payloads")?;
                let count = host_index(payload_count, "spawn payload count")?;
                let end = first
                    .checked_add(count)
                    .ok_or(RuntimeV2Error::HostIndexOverflow {
                        table: "spawn payloads",
                        index: u64::MAX,
                    })?;
                let payloads =
                    package
                        .payloads
                        .get(first..end)
                        .ok_or(RuntimeV2Error::HostIndexOverflow {
                            table: "payloads",
                            index: first_payload.index().saturating_add(payload_count),
                        })?;
                let mut owned_payloads = Vec::new();
                owned_payloads
                    .try_reserve_exact(payloads.len())
                    .map_err(|_| RuntimeV2Error::AllocationFailed {
                        context: "startup spawn payloads",
                    })?;
                for payload in payloads {
                    let schema = indexed(schemas, payload.schema.index(), "schemas")?;
                    owned_payloads.push(StartupPayload {
                        schema_id: schema.id,
                        bytes: copy_bytes(&payload.bytes, "startup spawn payload")?,
                    });
                }
                OwnedStartupOperation::Spawn(owned_payloads)
            }
            StartupOperationKind::RunSchedule { schedule } => {
                OwnedStartupOperation::RunSchedule(ScheduleIndex::new(schedule.index()))
            }
        };
        operations.push(owned);
    }
    Ok(operations)
}

fn function_descriptor(
    package: &ExecutionPackage,
    position: usize,
) -> Result<NativeFunctionDescriptor, RuntimeV2Error> {
    let index = dense_position(position, "function links")?;
    let link = indexed(&package.function_links, index, "function links")?;
    if position == 0 {
        debug_assert_eq!(link.target, FunctionTarget::Startup);
    }
    Ok(NativeFunctionDescriptor {
        symbol_name: copy_string(
            package_string(package, link.symbol_name.index())?,
            "function symbol name",
        )?,
        abi_hash: link.abi_hash,
        body_hash: link.body_hash,
        code_offset: link.code_offset,
        code_byte_len: link.code_byte_len,
    })
}

fn package_string(package: &ExecutionPackage, index: u64) -> Result<&str, RuntimeV2Error> {
    indexed(&package.strings, index, "strings").map(String::as_str)
}

fn dense_position(position: usize, table: &'static str) -> Result<u64, RuntimeV2Error> {
    u64::try_from(position).map_err(|_| RuntimeV2Error::DenseIndexOverflow { table })
}

fn host_index(index: u64, table: &'static str) -> Result<usize, RuntimeV2Error> {
    usize::try_from(index).map_err(|_| RuntimeV2Error::HostIndexOverflow { table, index })
}

fn indexed<'a, T>(
    values: &'a [T],
    index: u64,
    table: &'static str,
) -> Result<&'a T, RuntimeV2Error> {
    let host = host_index(index, table)?;
    values
        .get(host)
        .ok_or(RuntimeV2Error::HostIndexOverflow { table, index })
}

fn copy_bytes(bytes: &[u8], context: &'static str) -> Result<Vec<u8>, RuntimeV2Error> {
    let mut owned = Vec::new();
    owned
        .try_reserve_exact(bytes.len())
        .map_err(|_| RuntimeV2Error::AllocationFailed { context })?;
    owned.extend_from_slice(bytes);
    Ok(owned)
}

fn copy_string(value: &str, context: &'static str) -> Result<String, RuntimeV2Error> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(value.len())
        .map_err(|_| RuntimeV2Error::AllocationFailed { context })?;
    owned.push_str(value);
    Ok(owned)
}

fn validate_payload(schema: &SchemaDescriptor, bytes: &[u8]) -> Result<(), RuntimeV2Error> {
    let actual = u64::try_from(bytes.len()).map_err(|_| RuntimeV2Error::DenseIndexOverflow {
        table: "payload bytes",
    })?;
    if actual != schema.byte_size {
        return Err(RuntimeV2Error::PayloadLength {
            schema: schema.id,
            expected: schema.byte_size,
            actual,
        });
    }
    for field in &schema.fields {
        if field.primitive != PrimitiveType::Bool {
            continue;
        }
        let offset = host_index(field.byte_offset, "bool field byte offset")?;
        let actual = bytes[offset];
        if actual > 1 {
            return Err(RuntimeV2Error::InvalidBoolPayload {
                schema: schema.id,
                byte_offset: field.byte_offset,
                actual,
            });
        }
    }
    Ok(())
}

fn query_matches(
    schemas: &[SchemaDescriptor],
    query: &QueryDescriptor,
    table: &ArchetypeTable,
) -> bool {
    query.terms.iter().all(|term| {
        let schema = indexed(schemas, term.schema.index(), "schemas")
            .expect("validated queries have valid schema indexes");
        let present = table.key.binary_search(&schema.id).is_ok();
        match term.access {
            QueryAccess::Read | QueryAccess::Mut => present,
            QueryAccess::Exclude => !present,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_package_v2::{
        encode_package, FieldRecord, FunctionLinkRecord, ParameterRecord, PayloadRecord,
        QueryRecord, QueryRef, ScheduleItemRecord, ScheduleRecord, ScheduleRef, SchemaFlags,
        SchemaRecord, SchemaRef, StartupOperationRecord, StringRef, SystemRecord, SystemRef,
        TermRecord, WorldRecord,
    };
    use crate::ids_v2::SchemaField;
    use std::convert::Infallible;
    use std::io::Cursor;

    #[derive(Clone, Copy)]
    struct FixtureIds {
        data: SchemaId,
        empty: SchemaId,
        empty_resource: SchemaId,
        excluded: SchemaId,
        flag: SchemaId,
        resource: SchemaId,
        tag: SchemaId,
        uninitialized: SchemaId,
        system: DeclId,
        query: DeclId,
        schedule: DeclId,
    }

    #[derive(Clone)]
    struct SchemaDefinition {
        name: &'static str,
        kind: SchemaKind,
        fields: Vec<(&'static str, PrimitiveType)>,
    }

    fn string_ref(strings: &[String], value: &str) -> StringRef {
        let index = strings
            .binary_search_by(|candidate| candidate.as_str().cmp(value))
            .unwrap();
        StringRef::new(u64::try_from(index).unwrap())
    }

    fn schema_ref(schemas: &[SchemaRecord], id: SchemaId) -> SchemaRef {
        let index = schemas
            .binary_search_by_key(&id, |schema| schema.id)
            .unwrap();
        SchemaRef::new(u64::try_from(index).unwrap())
    }

    fn fixture_package() -> (ExecutionPackage, FixtureIds) {
        let mut strings: Vec<String> = [
            "Data",
            "Demo",
            "Empty",
            "EmptyResource",
            "Excluded",
            "Main",
            "Resource",
            "System",
            "Tag",
            "Uninitialized",
            "_startup",
            "_system",
            "empty_resource",
            "q",
            "resource",
            "x",
            "zflag",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        strings.sort_unstable();

        let definitions = vec![
            SchemaDefinition {
                name: "Data",
                kind: SchemaKind::Component,
                fields: vec![("x", PrimitiveType::I32)],
            },
            SchemaDefinition {
                name: "Empty",
                kind: SchemaKind::Component,
                fields: Vec::new(),
            },
            SchemaDefinition {
                name: "EmptyResource",
                kind: SchemaKind::Resource,
                fields: Vec::new(),
            },
            SchemaDefinition {
                name: "Excluded",
                kind: SchemaKind::Component,
                fields: Vec::new(),
            },
            SchemaDefinition {
                name: "zflag",
                kind: SchemaKind::Component,
                fields: vec![("x", PrimitiveType::Bool)],
            },
            SchemaDefinition {
                name: "Resource",
                kind: SchemaKind::Resource,
                fields: vec![("x", PrimitiveType::I32)],
            },
            SchemaDefinition {
                name: "Tag",
                kind: SchemaKind::Tag,
                fields: Vec::new(),
            },
            SchemaDefinition {
                name: "Uninitialized",
                kind: SchemaKind::Resource,
                fields: Vec::new(),
            },
        ];
        let mut identified: Vec<_> = definitions
            .into_iter()
            .map(|definition| {
                let fields: Vec<_> = definition
                    .fields
                    .iter()
                    .map(|(name, primitive)| SchemaField {
                        name,
                        primitive: *primitive,
                    })
                    .collect();
                let id = SchemaId::derive(definition.kind, "Demo", definition.name, &fields);
                (id, definition)
            })
            .collect();
        identified.sort_unstable_by_key(|(id, _)| *id);

        let mut schemas = Vec::new();
        let mut fields = Vec::new();
        for (position, (id, definition)) in identified.iter().enumerate() {
            let schema = SchemaRef::new(u64::try_from(position).unwrap());
            let mut byte_size = 0_u64;
            let mut alignment = 1_u64;
            for (name, primitive) in &definition.fields {
                let (field_size, field_alignment) = match primitive {
                    PrimitiveType::I32 | PrimitiveType::F32 => (4, 4),
                    PrimitiveType::Bool => (1, 1),
                };
                byte_size = byte_size.div_ceil(field_alignment) * field_alignment;
                fields.push(FieldRecord {
                    schema,
                    name: string_ref(&strings, name),
                    primitive: *primitive,
                    byte_offset: byte_size,
                    source_span: None,
                });
                byte_size += field_size;
                alignment = alignment.max(field_alignment);
            }
            byte_size = byte_size.div_ceil(alignment) * alignment;
            schemas.push(SchemaRecord {
                id: *id,
                kind: definition.kind,
                flags: SchemaFlags::for_kind(definition.kind),
                name: string_ref(&strings, definition.name),
                byte_size,
                alignment,
                source_span: None,
            });
        }

        let data = identified
            .iter()
            .find(|(_, definition)| definition.name == "Data")
            .unwrap()
            .0;
        let empty = identified
            .iter()
            .find(|(_, definition)| definition.name == "Empty")
            .unwrap()
            .0;
        let empty_resource = identified
            .iter()
            .find(|(_, definition)| definition.name == "EmptyResource")
            .unwrap()
            .0;
        let excluded = identified
            .iter()
            .find(|(_, definition)| definition.name == "Excluded")
            .unwrap()
            .0;
        let flag = identified
            .iter()
            .find(|(_, definition)| definition.name == "zflag")
            .unwrap()
            .0;
        let resource = identified
            .iter()
            .find(|(_, definition)| definition.name == "Resource")
            .unwrap()
            .0;
        let tag = identified
            .iter()
            .find(|(_, definition)| definition.name == "Tag")
            .unwrap()
            .0;
        let uninitialized = identified
            .iter()
            .find(|(_, definition)| definition.name == "Uninitialized")
            .unwrap()
            .0;
        let system = DeclId::system("Demo", "System");
        let query = DeclId::query(system, "q");
        let schedule = DeclId::schedule("Demo", "Main");
        let abi_hash = AbiHash::from_bytes([0xA1; 16]);
        let body_hash = BodyHash::from_bytes([0xB1; 16]);
        let startup_abi_hash = AbiHash::from_bytes([0xA0; 16]);
        let startup_body_hash = BodyHash::from_bytes([0xB0; 16]);

        let data_ref = schema_ref(&schemas, data);
        let empty_resource_ref = schema_ref(&schemas, empty_resource);
        let excluded_ref = schema_ref(&schemas, excluded);
        let resource_ref = schema_ref(&schemas, resource);
        let tag_ref = schema_ref(&schemas, tag);

        let mut spawn_payloads = vec![
            PayloadRecord {
                schema: data_ref,
                bytes: 1i32.to_le_bytes().to_vec(),
            },
            PayloadRecord {
                schema: tag_ref,
                bytes: Vec::new(),
            },
        ];
        spawn_payloads.sort_unstable_by_key(|payload| payload.schema.index());
        let mut payloads = vec![
            PayloadRecord {
                schema: resource_ref,
                bytes: 7i32.to_le_bytes().to_vec(),
            },
            PayloadRecord {
                schema: empty_resource_ref,
                bytes: Vec::new(),
            },
        ];
        payloads.extend(spawn_payloads);

        let package = ExecutionPackage {
            strings,
            world: WorldRecord {
                name: StringRef::new(1),
                source_span: None,
                startup_abi_hash,
                startup_body_hash,
            },
            schemas,
            fields,
            systems: vec![SystemRecord {
                id: system,
                name: StringRef::new(7),
                abi_hash,
                body_hash,
                source_span: None,
            }],
            parameters: vec![
                ParameterRecord {
                    system: SystemRef::new(0),
                    name: StringRef::new(12),
                    kind: ParameterKind::ReadResource {
                        resource: empty_resource_ref,
                    },
                    source_span: None,
                },
                ParameterRecord {
                    system: SystemRef::new(0),
                    name: StringRef::new(13),
                    kind: ParameterKind::Query {
                        query: QueryRef::new(0),
                    },
                    source_span: None,
                },
                ParameterRecord {
                    system: SystemRef::new(0),
                    name: StringRef::new(14),
                    kind: ParameterKind::MutResource {
                        resource: resource_ref,
                    },
                    source_span: None,
                },
            ],
            queries: vec![QueryRecord {
                id: query,
                system: SystemRef::new(0),
                parameter: crate::execution_package_v2::ParameterRef::new(1),
                source_span: None,
            }],
            terms: vec![
                TermRecord {
                    query: QueryRef::new(0),
                    access: QueryAccess::Mut,
                    schema: data_ref,
                    source_span: None,
                },
                TermRecord {
                    query: QueryRef::new(0),
                    access: QueryAccess::Read,
                    schema: tag_ref,
                    source_span: None,
                },
                TermRecord {
                    query: QueryRef::new(0),
                    access: QueryAccess::Exclude,
                    schema: excluded_ref,
                    source_span: None,
                },
            ],
            schedules: vec![ScheduleRecord {
                id: schedule,
                name: StringRef::new(5),
                source_span: None,
            }],
            schedule_items: vec![
                ScheduleItemRecord {
                    schedule: ScheduleRef::new(0),
                    kind: ScheduleItemKind::RunSystem {
                        system: SystemRef::new(0),
                    },
                    source_span: None,
                },
                ScheduleItemRecord {
                    schedule: ScheduleRef::new(0),
                    kind: ScheduleItemKind::RunSystem {
                        system: SystemRef::new(0),
                    },
                    source_span: None,
                },
            ],
            startup_operations: vec![
                StartupOperationRecord {
                    kind: StartupOperationKind::ResourcePayload {
                        resource: resource_ref,
                        payload: crate::execution_package_v2::PayloadRef::new(0),
                    },
                    source_span: None,
                },
                StartupOperationRecord {
                    kind: StartupOperationKind::ResourcePayload {
                        resource: empty_resource_ref,
                        payload: crate::execution_package_v2::PayloadRef::new(1),
                    },
                    source_span: None,
                },
                StartupOperationRecord {
                    kind: StartupOperationKind::Spawn {
                        first_payload: crate::execution_package_v2::PayloadRef::new(2),
                        payload_count: 2,
                    },
                    source_span: None,
                },
                StartupOperationRecord {
                    kind: StartupOperationKind::Spawn {
                        first_payload: crate::execution_package_v2::PayloadRef::new(4),
                        payload_count: 0,
                    },
                    source_span: None,
                },
                StartupOperationRecord {
                    kind: StartupOperationKind::RunSchedule {
                        schedule: ScheduleRef::new(0),
                    },
                    source_span: None,
                },
            ],
            payloads,
            function_links: vec![
                FunctionLinkRecord {
                    target: FunctionTarget::Startup,
                    symbol_name: StringRef::new(10),
                    abi_hash: startup_abi_hash,
                    body_hash: startup_body_hash,
                    code_offset: 0,
                    code_byte_len: 1,
                    source_span: None,
                    first_body_span: None,
                    body_span_count: 0,
                },
                FunctionLinkRecord {
                    target: FunctionTarget::System {
                        system: SystemRef::new(0),
                    },
                    symbol_name: StringRef::new(11),
                    abi_hash,
                    body_hash,
                    code_offset: 1,
                    code_byte_len: 1,
                    source_span: None,
                    first_body_span: None,
                    body_span_count: 0,
                },
            ],
            source_spans: Vec::new(),
        };
        let ids = FixtureIds {
            data,
            empty,
            empty_resource,
            excluded,
            flag,
            resource,
            tag,
            uninitialized,
            system,
            query,
            schedule,
        };
        (package, ids)
    }

    struct UpdatingDispatcher {
        invocations: Vec<DeclId>,
    }

    impl SystemDispatcher for UpdatingDispatcher {
        type Error = Infallible;

        fn dispatch(
            &mut self,
            world: &mut RuntimeWorldV2,
            invocation: SystemInvocation,
        ) -> Result<(), Self::Error> {
            self.invocations.push(invocation.id());
            let resource = world.systems()[invocation.system().index() as usize]
                .parameters()
                .iter()
                .find_map(|parameter| match parameter.kind() {
                    SystemParameterKind::MutResource { resource } => {
                        Some(world.schemas()[resource.index() as usize].id())
                    }
                    SystemParameterKind::ReadResource { .. }
                    | SystemParameterKind::Query { .. } => None,
                })
                .unwrap();
            let ResourceState::Initialized(bytes) = world.resource(resource).unwrap().state()
            else {
                panic!("resource must be initialized before schedule dispatch");
            };
            let value = i32::from_le_bytes(bytes.try_into().unwrap()) + 1;
            world
                .assign_resource(resource, &value.to_le_bytes())
                .unwrap();
            Ok(())
        }
    }

    #[test]
    fn construction_validates_then_preregisters_without_world_mutation() {
        let (package, ids) = fixture_package();
        let metadata = encode_package(&package).unwrap();
        let mut reader = Cursor::new(metadata);
        let world = RuntimeWorldV2::decode_from(&mut reader).unwrap();

        assert_eq!(world.world_name(), "Demo");
        assert_eq!(world.schemas().len(), 8);
        assert_eq!(world.systems().len(), 1);
        assert_eq!(world.queries().len(), 1);
        assert_eq!(world.schedules().len(), 1);
        assert_eq!(world.system_index(ids.system), Some(SystemIndex::new(0)));
        assert_eq!(world.query_index(ids.query), Some(QueryIndex::new(0)));
        assert_eq!(
            world.schedule_index(ids.schedule),
            Some(ScheduleIndex::new(0))
        );
        assert_eq!(world.startup_function().symbol_name(), "_startup");
        assert_eq!(world.startup_state(), StartupState::Ready);
        assert_eq!(world.next_spawn_ordinal(), 0);
        assert!(world.tables().is_empty());
        assert!(world
            .resources()
            .all(|resource| resource.state() == ResourceState::Uninitialized));
    }

    #[test]
    fn startup_is_source_ordered_and_dispatches_repeated_schedule_entries() {
        let (package, ids) = fixture_package();
        let mut world = RuntimeWorldV2::from_package(package).unwrap();
        let mut dispatcher = UpdatingDispatcher {
            invocations: Vec::new(),
        };

        world.execute_startup(&mut dispatcher).unwrap();

        assert_eq!(dispatcher.invocations, vec![ids.system, ids.system]);
        assert_eq!(world.startup_state(), StartupState::Complete);
        assert_eq!(world.next_spawn_ordinal(), 2);
        assert_eq!(
            world.resource(ids.resource).unwrap().state(),
            ResourceState::Initialized(&9i32.to_le_bytes())
        );
        assert_eq!(
            world.resource(ids.empty_resource).unwrap().state(),
            ResourceState::Initialized(&[])
        );
        assert_eq!(
            world.resource(ids.uninitialized).unwrap().state(),
            ResourceState::Uninitialized
        );
        assert!(matches!(
            world.execute_startup(&mut dispatcher),
            Err(StartupExecutionError::Runtime(
                RuntimeV2Error::StartupAlreadyExecuted
            ))
        ));
    }

    #[test]
    fn tags_zsts_and_empty_archetypes_are_semantic_state() {
        let (package, ids) = fixture_package();
        let mut world = RuntimeWorldV2::from_package(package).unwrap();
        world
            .execute_startup(&mut UpdatingDispatcher {
                invocations: Vec::new(),
            })
            .unwrap();

        assert_eq!(world.tables().len(), 2);
        assert!(world
            .tables()
            .windows(2)
            .all(|pair| pair[0].key() < pair[1].key()));
        let empty_table = world
            .tables()
            .iter()
            .find(|table| table.key().is_empty())
            .unwrap();
        assert_eq!(empty_table.rows()[0].spawn_ordinal(), 1);
        assert!(empty_table.rows()[0].columns().is_empty());
        let tagged = world.row(0).unwrap();
        assert_eq!(tagged.column(ids.tag).unwrap().bytes(), &[]);
        assert_eq!(
            tagged.column(ids.data).unwrap().bytes(),
            &1i32.to_le_bytes()
        );
        assert!(world.schema_index(ids.empty).is_some());
    }

    #[test]
    fn query_matching_honors_required_mutable_and_excluded_terms() {
        let (package, ids) = fixture_package();
        let mut world = RuntimeWorldV2::from_package(package).unwrap();
        world
            .spawn(&[
                SchemaPayload {
                    schema_id: ids.data,
                    bytes: &1i32.to_le_bytes(),
                },
                SchemaPayload {
                    schema_id: ids.tag,
                    bytes: &[],
                },
            ])
            .unwrap();
        world
            .spawn(&[
                SchemaPayload {
                    schema_id: ids.data,
                    bytes: &2i32.to_le_bytes(),
                },
                SchemaPayload {
                    schema_id: ids.tag,
                    bytes: &[],
                },
                SchemaPayload {
                    schema_id: ids.excluded,
                    bytes: &[],
                },
            ])
            .unwrap();
        world
            .spawn(&[SchemaPayload {
                schema_id: ids.tag,
                bytes: &[],
            }])
            .unwrap();

        let query = world.query(QueryIndex::new(0)).unwrap();
        assert_eq!(
            query
                .terms()
                .iter()
                .map(QueryTermDescriptor::access)
                .collect::<Vec<_>>(),
            vec![QueryAccess::Mut, QueryAccess::Read, QueryAccess::Exclude]
        );
        let matches: Vec<_> = world.matching_tables(QueryIndex::new(0)).unwrap().collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].rows().len(), 1);
        assert_eq!(matches[0].rows()[0].spawn_ordinal(), 0);
    }

    #[test]
    fn invalid_mutations_are_transactional() {
        let (package, ids) = fixture_package();
        let mut world = RuntimeWorldV2::from_package(package).unwrap();
        assert!(matches!(
            world.spawn(&[SchemaPayload {
                schema_id: ids.flag,
                bytes: &[2],
            }]),
            Err(RuntimeV2Error::InvalidBoolPayload {
                schema,
                byte_offset: 0,
                actual: 2,
            }) if schema == ids.flag
        ));
        assert!(world.tables().is_empty());
        assert_eq!(world.next_spawn_ordinal(), 0);

        world
            .initialize_resource(ids.resource, &5i32.to_le_bytes())
            .unwrap();
        let ResourceState::Initialized(resource_before) =
            world.resource(ids.resource).unwrap().state()
        else {
            panic!("resource must be initialized");
        };
        let resource_before = resource_before.to_vec();
        assert!(matches!(
            world.assign_resource(ids.resource, &[1, 2]),
            Err(RuntimeV2Error::PayloadLength { .. })
        ));
        assert_eq!(
            world.resource(ids.resource).unwrap().state(),
            ResourceState::Initialized(&resource_before)
        );

        world
            .spawn(&[
                SchemaPayload {
                    schema_id: ids.data,
                    bytes: &10i32.to_le_bytes(),
                },
                SchemaPayload {
                    schema_id: ids.tag,
                    bytes: &[],
                },
            ])
            .unwrap();
        let table_snapshot = world.tables().to_vec();
        assert!(matches!(
            world.spawn(&[
                SchemaPayload {
                    schema_id: ids.data,
                    bytes: &11i32.to_le_bytes(),
                },
                SchemaPayload {
                    schema_id: ids.tag,
                    bytes: &[1],
                },
            ]),
            Err(RuntimeV2Error::PayloadLength { .. })
        ));
        assert_eq!(world.tables(), table_snapshot);
        assert_eq!(world.next_spawn_ordinal(), 1);

        let unknown = SchemaId::from_bytes([0xFF; 16]);
        assert!(matches!(
            world.assign_row(
                0,
                &[
                    SchemaPayload {
                        schema_id: ids.data,
                        bytes: &12i32.to_le_bytes(),
                    },
                    SchemaPayload {
                        schema_id: unknown,
                        bytes: &[],
                    },
                ]
            ),
            Err(RuntimeV2Error::UnknownSchema(id)) if id == unknown
        ));
        assert_eq!(
            world.row(0).unwrap().column(ids.data).unwrap().bytes(),
            &10i32.to_le_bytes()
        );
        assert!(matches!(
            world.assign_row(
                0,
                &[SchemaPayload {
                    schema_id: ids.tag,
                    bytes: &[],
                }]
            ),
            Err(RuntimeV2Error::ZeroSizedMutation(id)) if id == ids.tag
        ));
    }

    #[test]
    fn rows_keep_global_spawn_ordinals_in_per_table_commit_order() {
        let (package, ids) = fixture_package();
        let mut world = RuntimeWorldV2::from_package(package).unwrap();
        world
            .spawn(&[SchemaPayload {
                schema_id: ids.data,
                bytes: &1i32.to_le_bytes(),
            }])
            .unwrap();
        world.spawn(&[]).unwrap();
        world
            .spawn(&[SchemaPayload {
                schema_id: ids.data,
                bytes: &2i32.to_le_bytes(),
            }])
            .unwrap();

        let data_table = world
            .tables()
            .iter()
            .find(|table| table.key() == [ids.data])
            .unwrap();
        assert_eq!(
            data_table
                .rows()
                .iter()
                .map(EntityRow::spawn_ordinal)
                .collect::<Vec<_>>(),
            vec![0, 2]
        );
        assert_eq!(world.row(1).unwrap().columns(), &[]);
    }

    #[test]
    fn malformed_package_is_rejected_before_any_world_exists() {
        let (mut package, _) = fixture_package();
        package.payloads[0].bytes.pop();
        assert!(matches!(
            RuntimeWorldV2::from_package(package),
            Err(RuntimeV2Error::Package(
                ExecutionPackageV2Error::InvalidPayload { .. }
            ))
        ));

        let (mut package, _) = fixture_package();
        package.terms[1].access = QueryAccess::Mut;
        assert!(matches!(
            RuntimeWorldV2::from_package(package),
            Err(RuntimeV2Error::Package(
                ExecutionPackageV2Error::InvalidRecord { .. }
            ))
        ));

        let (mut package, ids) = fixture_package();
        package.payloads.push(PayloadRecord {
            schema: schema_ref(&package.schemas, ids.flag),
            bytes: vec![2],
        });
        assert!(matches!(
            RuntimeWorldV2::from_package(package),
            Err(RuntimeV2Error::Package(
                ExecutionPackageV2Error::InvalidPayload {
                    reason: "bool fields must be encoded as 0 or 1",
                    ..
                }
            ))
        ));
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct InjectedTrap;

    impl fmt::Display for InjectedTrap {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("injected trap")
        }
    }

    impl std::error::Error for InjectedTrap {}

    struct TrappingDispatcher {
        resource: SchemaId,
    }

    impl SystemDispatcher for TrappingDispatcher {
        type Error = InjectedTrap;

        fn dispatch(
            &mut self,
            world: &mut RuntimeWorldV2,
            _invocation: SystemInvocation,
        ) -> Result<(), Self::Error> {
            world
                .assign_resource(self.resource, &44i32.to_le_bytes())
                .unwrap();
            Err(InjectedTrap)
        }
    }

    #[test]
    fn dispatch_failure_preserves_earlier_committed_startup_effects() {
        let (package, ids) = fixture_package();
        let mut world = RuntimeWorldV2::from_package(package).unwrap();
        let error = world
            .execute_startup(&mut TrappingDispatcher {
                resource: ids.resource,
            })
            .unwrap_err();

        assert_eq!(error, StartupExecutionError::Dispatch(InjectedTrap));
        assert_eq!(world.startup_state(), StartupState::Failed);
        assert_eq!(world.next_spawn_ordinal(), 2);
        assert_eq!(
            world.resource(ids.resource).unwrap().state(),
            ResourceState::Initialized(&44i32.to_le_bytes())
        );
    }

    #[test]
    fn ordinal_overflow_does_not_publish_a_row() {
        let (package, _) = fixture_package();
        let mut world = RuntimeWorldV2::from_package(package).unwrap();
        world.next_spawn_ordinal = u64::MAX;
        assert_eq!(world.spawn(&[]), Err(RuntimeV2Error::SpawnOrdinalOverflow));
        assert!(world.tables().is_empty());
        assert_eq!(world.next_spawn_ordinal(), u64::MAX);
    }
}
