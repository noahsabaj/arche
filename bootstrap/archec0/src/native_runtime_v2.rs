use std::collections::{HashMap, HashSet};
use std::io::{self, Seek, SeekFrom, Write};

use archec0::execution_package_v2::{
    validate_package_with_code_range, wire, write_package_with_code_range, CodeImageRange,
    ExecutionPackage, FunctionTarget, ParameterKind, QueryAccess, SECTION_KINDS,
};
use archec0::ids_v2::{AbiHash, BodyHash, DeclId, PrimitiveType, SchemaId, SchemaKind};

use crate::aot_v2::{
    AotV2Error, Assembler, Condition, DataAllocator, DataChunk, Label, NativeTrapPoint, Register,
    WorldStoragePlan,
};
use crate::core::CoreSourceSubject;
use crate::core_verify::VerifiedExecutableCore;
use crate::execution_package_build::{
    build_execution_package, canonical_core_ids, validate_execution_package_link, NativeCodeLayout,
    NativeFunctionLayout, NativeFunctionTarget,
};
use crate::lexer::SourceSpan;

const RUNTIME_HEADER_BYTE_LEN: u64 = 64;
const RUNTIME_SECTION_ROW_BYTE_LEN: u64 = 40;
const RUNTIME_FUNCTION_ROW_BYTE_LEN: u64 = 16;
const RUNTIME_TRAP_ROW_BYTE_LEN: u64 = 16;
const RUNTIME_STORAGE_ROW_BYTE_LEN: u64 = 72;
const RUNTIME_STORAGE_ID: u64 = 0;
const RUNTIME_STORAGE_KIND: u64 = 16;
const RUNTIME_STORAGE_FLAGS: u64 = 24;
const RUNTIME_STORAGE_BYTE_SIZE: u64 = 32;
const RUNTIME_STORAGE_ALIGNMENT: u64 = 40;
const RUNTIME_STORAGE_RESOURCE_INITIALIZED: u64 = 48;
const RUNTIME_STORAGE_RESOURCE_PAYLOAD: u64 = 56;
const RUNTIME_STORAGE_ROW_CELL: u64 = 64;
const METADATA_DIRECTORY_CAPTURE_BYTE_LEN: usize =
    wire::HEADER_SIZE as usize + SECTION_KINDS.len() * wire::DIRECTORY_ENTRY_SIZE as usize;
const RUNTIME_MAGIC: &[u8; 8] = b"ARCHERT2";
const OBSERVATION_HEADER: &[u8] = b"ARCHEOBS2\n";
const OBSERVATION_END: &[u8] = b"END\n";
const EMPTY_PAYLOAD: &[u8] = b"-";
const VERSION_ONE_DIAGNOSTIC: &[u8] =
    b"arche: unsupported ARCHEECS version 1; rebuild with archec0\n";
const ARCHECMP_DIAGNOSTIC: &[u8] = b"arche: unsupported ARCHECMP artifact; rebuild with archec0\n";
const TRAP_PREFIX: &[u8] = b"arche: trap[";
const TRAP_MIDDLE: &[u8] = b"] ";
const TRAP_LOCATION_SEPARATOR: &[u8] = b":";
const TRAP_BYTES_PREFIX: &[u8] = b" bytes ";
const TRAP_RANGE_SEPARATOR: &[u8] = b"..";
const NEWLINE: &[u8] = b"\n";
const RESOURCE_PREFIX: &[u8] = b"RESOURCE ";
const RESOURCE_UNINITIALIZED: &[u8] = b" UNINITIALIZED\n";
const RESOURCE_INITIALIZED: &[u8] = b" INITIALIZED ";
const TABLE_PREFIX: &[u8] = b"TABLE ";
const ROW_PREFIX: &[u8] = b"ROW ";
const COLUMN_PREFIX: &[u8] = b"COLUMN ";
const SPACE: &[u8] = b" ";
const TRAP_NAMES: [&[u8]; 4] = [
    b"I32_DIVIDE_BY_ZERO",
    b"I32_DIVIDE_OVERFLOW",
    b"I32_REMAINDER_BY_ZERO",
    b"I32_REMAINDER_OVERFLOW",
];

struct MetadataDirectoryCapture {
    bytes: [u8; METADATA_DIRECTORY_CAPTURE_BYTE_LEN],
    position: u64,
    byte_len: u64,
}

impl MetadataDirectoryCapture {
    fn new() -> Self {
        Self {
            bytes: [0; METADATA_DIRECTORY_CAPTURE_BYTE_LEN],
            position: 0,
            byte_len: 0,
        }
    }

    fn byte_len(&self) -> u64 {
        self.byte_len
    }
}

impl Write for MetadataDirectoryCapture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let byte_len = u64::try_from(bytes.len())
            .map_err(|_| invalid_seek("metadata write length does not fit u64"))?;
        let end = self
            .position
            .checked_add(byte_len)
            .ok_or_else(|| invalid_seek("metadata write position overflows u64"))?;
        let capture_byte_len = u64::try_from(METADATA_DIRECTORY_CAPTURE_BYTE_LEN)
            .map_err(|_| invalid_seek("metadata capture length does not fit u64"))?;
        if self.position < capture_byte_len {
            let captured_end = end.min(capture_byte_len);
            let source_byte_len = captured_end
                .checked_sub(self.position)
                .ok_or_else(|| invalid_seek("metadata capture range is invalid"))?;
            let destination_start = usize::try_from(self.position)
                .map_err(|_| invalid_seek("metadata capture offset does not fit usize"))?;
            let destination_end = usize::try_from(captured_end)
                .map_err(|_| invalid_seek("metadata capture end does not fit usize"))?;
            let source_end = usize::try_from(source_byte_len)
                .map_err(|_| invalid_seek("metadata capture length does not fit usize"))?;
            self.bytes[destination_start..destination_end].copy_from_slice(&bytes[..source_end]);
        }
        self.position = end;
        self.byte_len = self.byte_len.max(end);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for MetadataDirectoryCapture {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.position = match position {
            SeekFrom::Start(offset) => offset,
            SeekFrom::Current(delta) => checked_seek(self.position, delta)?,
            SeekFrom::End(delta) => checked_seek(self.byte_len, delta)?,
        };
        Ok(self.position)
    }
}

fn checked_seek(base: u64, delta: i64) -> io::Result<u64> {
    if delta >= 0 {
        base.checked_add(delta.unsigned_abs())
    } else {
        base.checked_sub(delta.unsigned_abs())
    }
    .ok_or_else(|| invalid_seek("metadata seek is outside the u64 stream range"))
}

fn invalid_seek(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[derive(Clone, Debug)]
struct TrapDescriptorPlan {
    point: NativeTrapPoint,
    span: SourceSpan,
    function_kind: u64,
    function_system: u64,
}

#[derive(Clone, Debug)]
struct RuntimeLiteral {
    offset: u64,
    bytes: &'static [u8],
}

#[derive(Clone, Debug)]
struct NativeWorldLink {
    name: String,
    startup_abi_hash: AbiHash,
    startup_body_hash: BodyHash,
}

#[derive(Clone, Debug)]
struct NativeSchemaLink {
    id: SchemaId,
    name: String,
    kind: SchemaKind,
    flags: u64,
    byte_size: u64,
    alignment: u64,
}

#[derive(Clone, Debug)]
struct NativeFieldLink {
    name: String,
    schema: u64,
    primitive: PrimitiveType,
    byte_offset: u64,
}

#[derive(Clone, Debug)]
struct NativeSystemLink {
    id: DeclId,
    name: String,
    abi_hash: AbiHash,
    body_hash: BodyHash,
}

#[derive(Clone, Debug)]
struct NativeParameterLink {
    name: String,
    system: u64,
    kind: u64,
    target: u64,
}

#[derive(Clone, Copy, Debug)]
struct NativeQueryLink {
    id: DeclId,
    system: u64,
    parameter: u64,
}

#[derive(Clone, Copy, Debug)]
struct NativeTermLink {
    query: u64,
    access: u64,
    schema: u64,
}

#[derive(Clone, Debug)]
struct NativeScheduleLink {
    id: DeclId,
    name: String,
}

#[derive(Clone, Debug)]
struct NativeLinkManifest {
    world: NativeWorldLink,
    schemas: Vec<NativeSchemaLink>,
    fields: Vec<NativeFieldLink>,
    systems: Vec<NativeSystemLink>,
    parameters: Vec<NativeParameterLink>,
    queries: Vec<NativeQueryLink>,
    terms: Vec<NativeTermLink>,
    schedules: Vec<NativeScheduleLink>,
    function_count: usize,
}

impl NativeLinkManifest {
    fn from_package(package: &ExecutionPackage) -> Result<Self, AotV2Error> {
        let function_count = package.function_links.len();
        let world = NativeWorldLink {
            name: clone_manifest_string(package, package.world.name.index(), "world name")?,
            startup_abi_hash: package.world.startup_abi_hash,
            startup_body_hash: package.world.startup_body_hash,
        };
        let mut schemas = Vec::new();
        reserve_vec(&mut schemas, package.schemas.len(), "native schema links")?;
        for record in &package.schemas {
            schemas.push(NativeSchemaLink {
                id: record.id,
                name: clone_manifest_string(package, record.name.index(), "schema name")?,
                kind: record.kind,
                flags: record.flags.bits(),
                byte_size: record.byte_size,
                alignment: record.alignment,
            });
        }
        let mut fields = Vec::new();
        reserve_vec(&mut fields, package.fields.len(), "native field links")?;
        for record in &package.fields {
            fields.push(NativeFieldLink {
                name: clone_manifest_string(package, record.name.index(), "field name")?,
                schema: record.schema.index(),
                primitive: record.primitive,
                byte_offset: record.byte_offset,
            });
        }
        let mut systems = Vec::new();
        reserve_vec(&mut systems, package.systems.len(), "native system links")?;
        for record in &package.systems {
            systems.push(NativeSystemLink {
                id: record.id,
                name: clone_manifest_string(package, record.name.index(), "system name")?,
                abi_hash: record.abi_hash,
                body_hash: record.body_hash,
            });
        }
        let mut parameters = Vec::new();
        reserve_vec(
            &mut parameters,
            package.parameters.len(),
            "native parameter links",
        )?;
        for record in &package.parameters {
            let (kind, target) = match record.kind {
                ParameterKind::ReadResource { resource } => {
                    (wire::parameter::READ_RESOURCE, resource.index())
                }
                ParameterKind::MutResource { resource } => {
                    (wire::parameter::MUT_RESOURCE, resource.index())
                }
                ParameterKind::Query { query } => (wire::parameter::QUERY, query.index()),
            };
            parameters.push(NativeParameterLink {
                name: clone_manifest_string(package, record.name.index(), "parameter name")?,
                system: record.system.index(),
                kind,
                target,
            });
        }
        let mut queries = Vec::new();
        reserve_vec(&mut queries, package.queries.len(), "native query links")?;
        for record in &package.queries {
            queries.push(NativeQueryLink {
                id: record.id,
                system: record.system.index(),
                parameter: record.parameter.index(),
            });
        }
        let mut terms = Vec::new();
        reserve_vec(&mut terms, package.terms.len(), "native term links")?;
        for record in &package.terms {
            let access = match record.access {
                QueryAccess::Read => wire::term::READ,
                QueryAccess::Mut => wire::term::MUT,
                QueryAccess::Exclude => wire::term::EXCLUDE,
            };
            terms.push(NativeTermLink {
                query: record.query.index(),
                access,
                schema: record.schema.index(),
            });
        }
        let mut schedules = Vec::new();
        reserve_vec(
            &mut schedules,
            package.schedules.len(),
            "native schedule links",
        )?;
        for record in &package.schedules {
            schedules.push(NativeScheduleLink {
                id: record.id,
                name: clone_manifest_string(package, record.name.index(), "schedule name")?,
            });
        }
        Ok(Self {
            world,
            schemas,
            fields,
            systems,
            parameters,
            queries,
            terms,
            schedules,
            function_count,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NativeRuntimePlan {
    world: WorldStoragePlan,
    link_manifest: NativeLinkManifest,
    trap_descriptors: Vec<TrapDescriptorPlan>,
    trap_indexes: HashMap<NativeTrapPoint, u64>,
    runtime_header_offset: u64,
    section_rows_offset: u64,
    function_rows_offset: u64,
    trap_rows_offset: u64,
    storage_rows_offset: u64,
    validated_offset: u64,
    startup_cursor_offset: u64,
    observation_schema_cursor_offset: u64,
    validation_resource_bits_offset: u64,
    validation_resource_bits_byte_len: u64,
    staging_row_offset: u64,
    decimal_scratch_offset: u64,
    hex_scratch_offset: u64,
    literals: Vec<RuntimeLiteral>,
    data_file_byte_len: u64,
    data_memory_byte_len: u64,
}

impl NativeRuntimePlan {
    pub(crate) fn build(
        core: &VerifiedExecutableCore,
        world: &WorldStoragePlan,
        allocator: &mut DataAllocator,
        trap_points: &[NativeTrapPoint],
    ) -> Result<Self, AotV2Error> {
        validate_world_storage(core, world)?;
        let link_manifest = build_link_manifest(core)?;
        let trap_descriptors = build_trap_descriptors(core, trap_points, &link_manifest)?;
        let mut trap_indexes = HashMap::new();
        reserve_map(
            &mut trap_indexes,
            trap_descriptors.len(),
            "runtime trap index",
        )?;
        for (index, descriptor) in trap_descriptors.iter().enumerate() {
            let index = as_u64(index, "runtime trap index")?;
            if trap_indexes.insert(descriptor.point, index).is_some() {
                return Err(invalid_native("duplicate native trap point"));
            }
        }

        let runtime_header_offset =
            allocator.allocate(RUNTIME_HEADER_BYTE_LEN, 8, "runtime initialized header")?;
        let section_rows_byte_len = as_u64(SECTION_KINDS.len(), "runtime section count")?
            .checked_mul(RUNTIME_SECTION_ROW_BYTE_LEN)
            .ok_or(AotV2Error::ArithmeticOverflow("runtime section rows"))?;
        let section_rows_offset =
            allocator.allocate(section_rows_byte_len, 8, "runtime section rows")?;
        let function_count = link_manifest.function_count;
        let function_rows_byte_len = as_u64(function_count, "runtime function count")?
            .checked_mul(RUNTIME_FUNCTION_ROW_BYTE_LEN)
            .ok_or(AotV2Error::ArithmeticOverflow("runtime function rows"))?;
        let function_rows_offset =
            allocator.allocate(function_rows_byte_len, 8, "runtime function rows")?;
        let trap_rows_byte_len = as_u64(trap_descriptors.len(), "runtime trap count")?
            .checked_mul(RUNTIME_TRAP_ROW_BYTE_LEN)
            .ok_or(AotV2Error::ArithmeticOverflow("runtime trap rows"))?;
        let trap_rows_offset = allocator.allocate(trap_rows_byte_len, 8, "runtime trap rows")?;
        let storage_rows_byte_len = as_u64(world.schemas.len(), "runtime storage row count")?
            .checked_mul(RUNTIME_STORAGE_ROW_BYTE_LEN)
            .ok_or(AotV2Error::ArithmeticOverflow("runtime storage rows"))?;
        let storage_rows_offset =
            allocator.allocate(storage_rows_byte_len, 8, "runtime storage rows")?;

        let mut literals = Vec::new();
        let literal_count = 18usize
            .checked_add(TRAP_NAMES.len())
            .ok_or(AotV2Error::AddressSpaceOverflow("runtime literal count"))?;
        reserve_vec(&mut literals, literal_count, "runtime literals")?;
        for bytes in [
            OBSERVATION_HEADER,
            OBSERVATION_END,
            EMPTY_PAYLOAD,
            VERSION_ONE_DIAGNOSTIC,
            ARCHECMP_DIAGNOSTIC,
            TRAP_PREFIX,
            TRAP_MIDDLE,
            TRAP_LOCATION_SEPARATOR,
            TRAP_BYTES_PREFIX,
            TRAP_RANGE_SEPARATOR,
            NEWLINE,
            RESOURCE_PREFIX,
            RESOURCE_UNINITIALIZED,
            RESOURCE_INITIALIZED,
            TABLE_PREFIX,
            ROW_PREFIX,
            COLUMN_PREFIX,
            SPACE,
        ]
        .into_iter()
        .chain(TRAP_NAMES)
        {
            let byte_len = as_u64(bytes.len(), "runtime literal byte length")?;
            let offset = allocator.allocate(byte_len, 1, "runtime literal")?;
            literals.push(RuntimeLiteral { offset, bytes });
        }

        let initialized_end = allocator.byte_len();
        let validated_offset = allocator.allocate(8, 8, "runtime validated flag")?;
        let startup_cursor_offset = allocator.allocate(8, 8, "runtime startup cursor")?;
        let observation_schema_cursor_offset =
            allocator.allocate(8, 8, "observation schema cursor")?;
        let validation_resource_bits_byte_len = as_u64(world.schemas.len(), "schema count")?
            .checked_add(7)
            .ok_or(AotV2Error::ArithmeticOverflow("validation resource bits"))?
            / 8;
        let validation_resource_bits_offset = allocator.allocate(
            validation_resource_bits_byte_len,
            1,
            "validation resource bits",
        )?;
        let staging_row_offset =
            allocator.allocate(world.row_stride, 8, "transactional spawn staging row")?;
        let decimal_scratch_offset = allocator.allocate(32, 8, "decimal formatter scratch")?;
        let hex_scratch_offset = allocator.allocate(2, 1, "hex formatter scratch")?;

        Ok(Self {
            world: world.clone(),
            link_manifest,
            trap_descriptors,
            trap_indexes,
            runtime_header_offset,
            section_rows_offset,
            function_rows_offset,
            trap_rows_offset,
            storage_rows_offset,
            validated_offset,
            startup_cursor_offset,
            observation_schema_cursor_offset,
            validation_resource_bits_offset,
            validation_resource_bits_byte_len,
            staging_row_offset,
            decimal_scratch_offset,
            hex_scratch_offset,
            literals,
            data_file_byte_len: initialized_end,
            data_memory_byte_len: allocator.byte_len(),
        })
    }

    pub(crate) const fn data_file_byte_len(&self) -> u64 {
        self.data_file_byte_len
    }

    pub(crate) fn trap_descriptor_index(&self, point: NativeTrapPoint) -> Result<u64, AotV2Error> {
        self.trap_indexes
            .get(&point)
            .copied()
            .ok_or_else(|| invalid_native("native trap point has no runtime descriptor"))
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NativeRuntimeLabels {
    pub validate_and_initialize: Label,
    pub execute_next_startup_operation: Label,
    pub next_query_row: Label,
    pub emit_observation_and_exit: Label,
    pub emit_trap_and_exit: Label,
}

impl NativeRuntimeLabels {
    pub(crate) fn declare(assembler: &mut Assembler) -> Result<Self, AotV2Error> {
        Ok(Self {
            validate_and_initialize: assembler.new_label()?,
            execute_next_startup_operation: assembler.new_label()?,
            next_query_row: assembler.new_label()?,
            emit_observation_and_exit: assembler.new_label()?,
            emit_trap_and_exit: assembler.new_label()?,
        })
    }
}

#[derive(Clone, Copy)]
struct RuntimeInternalLabels {
    failure: Label,
    version_one: Label,
    archecmp: Label,
    write_all: Label,
    write_decimal: Label,
    write_hex: Label,
    validate_utf8: Label,
    compare_bytes: Label,
    compare_keys: Label,
    observation_body: Label,
}

impl RuntimeInternalLabels {
    fn declare(assembler: &mut Assembler) -> Result<Self, AotV2Error> {
        Ok(Self {
            failure: assembler.new_label()?,
            version_one: assembler.new_label()?,
            archecmp: assembler.new_label()?,
            write_all: assembler.new_label()?,
            write_decimal: assembler.new_label()?,
            write_hex: assembler.new_label()?,
            validate_utf8: assembler.new_label()?,
            compare_bytes: assembler.new_label()?,
            compare_keys: assembler.new_label()?,
            observation_body: assembler.new_label()?,
        })
    }
}

pub(crate) fn emit_runtime(
    assembler: &mut Assembler,
    core: &VerifiedExecutableCore,
    world: &WorldStoragePlan,
    plan: &NativeRuntimePlan,
    labels: &NativeRuntimeLabels,
) -> Result<(), AotV2Error> {
    validate_world_storage(core, world)?;
    if world != &plan.world {
        return Err(invalid_native(
            "native runtime emission received a different world storage plan",
        ));
    }

    let internal = RuntimeInternalLabels::declare(assembler)?;
    emit_validate_and_initialize(assembler, plan, labels.validate_and_initialize, &internal)?;
    emit_execute_next_startup_operation(
        assembler,
        plan,
        labels.execute_next_startup_operation,
        &internal,
    )?;
    emit_next_query_row(assembler, plan, labels.next_query_row, &internal)?;
    emit_observation_exit(assembler, plan, labels.emit_observation_and_exit, &internal)?;
    emit_trap_exit(assembler, plan, labels.emit_trap_and_exit, &internal)?;
    emit_internal_helpers(assembler, plan, &internal)
}

fn emit_exit_group(assembler: &mut Assembler) -> Result<(), AotV2Error> {
    assembler.mov_imm64(Register::Rax, 231)?; // exit_group
    assembler.emit(&[0x0f, 0x05, 0x0f, 0x0b]) // syscall; ud2
}

fn emit_runtime_prologue(assembler: &mut Assembler) -> Result<(), AotV2Error> {
    assembler.emit(&[0x55, 0x53, 0x41, 0x54]) // push rbp; push rbx; push r12
}

fn emit_runtime_epilogue(assembler: &mut Assembler) -> Result<(), AotV2Error> {
    assembler.emit(&[0x41, 0x5c, 0x5b, 0x5d, 0xc3]) // pop r12; pop rbx; pop rbp; ret
}

fn register_code(register: Register) -> u8 {
    match register {
        Register::Rax | Register::R8 => 0,
        Register::Rcx | Register::R9 => 1,
        Register::Rdx | Register::R10 => 2,
        Register::Rbx | Register::R11 => 3,
        Register::Rsp | Register::R12 => 4,
        Register::Rbp | Register::R13 => 5,
        Register::Rsi | Register::R14 => 6,
        Register::Rdi | Register::R15 => 7,
    }
}

fn register_extended(register: Register) -> bool {
    matches!(
        register,
        Register::R8
            | Register::R9
            | Register::R10
            | Register::R11
            | Register::R12
            | Register::R13
            | Register::R14
            | Register::R15
    )
}

fn emit_rex(
    assembler: &mut Assembler,
    wide: bool,
    reg: Register,
    base: Register,
) -> Result<(), AotV2Error> {
    let rex = 0x40
        | u8::from(wide) << 3
        | u8::from(register_extended(reg)) << 2
        | u8::from(register_extended(base));
    if rex != 0x40 {
        assembler.emit(&[rex])?;
    }
    Ok(())
}

fn emit_memory_modrm(
    assembler: &mut Assembler,
    reg: Register,
    base: Register,
) -> Result<(), AotV2Error> {
    let base_code = register_code(base);
    if base_code == 5 {
        assembler.emit(&[0x40 | register_code(reg) << 3 | base_code, 0])?;
    } else {
        assembler.emit(&[register_code(reg) << 3 | base_code])?;
        if base_code == 4 {
            assembler.emit(&[0x24])?;
        }
    }
    Ok(())
}

fn emit_mov64_reg_mem(
    assembler: &mut Assembler,
    destination: Register,
    base: Register,
) -> Result<(), AotV2Error> {
    emit_rex(assembler, true, destination, base)?;
    assembler.emit(&[0x8b])?;
    emit_memory_modrm(assembler, destination, base)
}

fn emit_mov64_mem_reg(
    assembler: &mut Assembler,
    base: Register,
    source: Register,
) -> Result<(), AotV2Error> {
    emit_rex(assembler, true, source, base)?;
    assembler.emit(&[0x89])?;
    emit_memory_modrm(assembler, source, base)
}

fn emit_mov32_reg_mem(
    assembler: &mut Assembler,
    destination: Register,
    base: Register,
) -> Result<(), AotV2Error> {
    emit_rex(assembler, false, destination, base)?;
    assembler.emit(&[0x8b])?;
    emit_memory_modrm(assembler, destination, base)
}

fn emit_mov8_reg_mem(
    assembler: &mut Assembler,
    destination: Register,
    base: Register,
) -> Result<(), AotV2Error> {
    emit_rex(assembler, false, destination, base)?;
    assembler.emit(&[0x0f, 0xb6])?;
    emit_memory_modrm(assembler, destination, base)
}

fn emit_mov8_mem_reg(
    assembler: &mut Assembler,
    base: Register,
    source: Register,
) -> Result<(), AotV2Error> {
    let requires_low_byte_rex = !register_extended(source) && register_code(source) >= 4;
    if requires_low_byte_rex && !register_extended(base) {
        assembler.emit(&[0x40])?;
    } else {
        emit_rex(assembler, false, source, base)?;
    }
    assembler.emit(&[0x88])?;
    emit_memory_modrm(assembler, source, base)
}

fn emit_cmp64_reg_reg(
    assembler: &mut Assembler,
    left: Register,
    right: Register,
) -> Result<(), AotV2Error> {
    let rex = 0x48 | u8::from(register_extended(right)) << 2 | u8::from(register_extended(left));
    assembler.emit(&[
        rex,
        0x39,
        0xc0 | register_code(right) << 3 | register_code(left),
    ])
}

fn emit_test64_reg_reg(
    assembler: &mut Assembler,
    left: Register,
    right: Register,
) -> Result<(), AotV2Error> {
    let rex = 0x48 | u8::from(register_extended(right)) << 2 | u8::from(register_extended(left));
    assembler.emit(&[
        rex,
        0x85,
        0xc0 | register_code(right) << 3 | register_code(left),
    ])
}

fn emit_sub64_reg_reg(
    assembler: &mut Assembler,
    destination: Register,
    source: Register,
) -> Result<(), AotV2Error> {
    let rex =
        0x48 | u8::from(register_extended(source)) << 2 | u8::from(register_extended(destination));
    assembler.emit(&[
        rex,
        0x29,
        0xc0 | register_code(source) << 3 | register_code(destination),
    ])
}

fn emit_or64_reg_reg(
    assembler: &mut Assembler,
    destination: Register,
    source: Register,
) -> Result<(), AotV2Error> {
    let rex =
        0x48 | u8::from(register_extended(source)) << 2 | u8::from(register_extended(destination));
    assembler.emit(&[
        rex,
        0x09,
        0xc0 | register_code(source) << 3 | register_code(destination),
    ])
}

fn emit_and64_imm8(
    assembler: &mut Assembler,
    register: Register,
    immediate: u8,
) -> Result<(), AotV2Error> {
    let rex = 0x48 | u8::from(register_extended(register));
    assembler.emit(&[rex, 0x83, 0xe0 | register_code(register), immediate])
}

fn emit_shift_right64_imm8(
    assembler: &mut Assembler,
    register: Register,
    count: u8,
) -> Result<(), AotV2Error> {
    let rex = 0x48 | u8::from(register_extended(register));
    assembler.emit(&[rex, 0xc1, 0xe8 | register_code(register), count])
}

fn emit_shift_left64_cl(assembler: &mut Assembler, register: Register) -> Result<(), AotV2Error> {
    let rex = 0x48 | u8::from(register_extended(register));
    assembler.emit(&[rex, 0xd3, 0xe0 | register_code(register)])
}

fn emit_imul64_reg_reg(
    assembler: &mut Assembler,
    destination: Register,
    source: Register,
) -> Result<(), AotV2Error> {
    let rex =
        0x48 | u8::from(register_extended(destination)) << 2 | u8::from(register_extended(source));
    assembler.emit(&[
        rex,
        0x0f,
        0xaf,
        0xc0 | register_code(destination) << 3 | register_code(source),
    ])
}

fn emit_increment64(assembler: &mut Assembler, register: Register) -> Result<(), AotV2Error> {
    let rex = 0x48 | u8::from(register_extended(register));
    assembler.emit(&[rex, 0x83, 0xc0 | register_code(register), 1])
}

fn emit_decrement64(assembler: &mut Assembler, register: Register) -> Result<(), AotV2Error> {
    let rex = 0x48 | u8::from(register_extended(register));
    assembler.emit(&[rex, 0x83, 0xe8 | register_code(register), 1])
}

fn emit_add64_imm8(
    assembler: &mut Assembler,
    register: Register,
    value: u8,
) -> Result<(), AotV2Error> {
    let rex = 0x48 | u8::from(register_extended(register));
    assembler.emit(&[rex, 0x83, 0xc0 | register_code(register), value])
}

fn emit_sub64_imm8(
    assembler: &mut Assembler,
    register: Register,
    value: u8,
) -> Result<(), AotV2Error> {
    let rex = 0x48 | u8::from(register_extended(register));
    assembler.emit(&[rex, 0x83, 0xe8 | register_code(register), value])
}

fn emit_section_base(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    section_index: usize,
    destination: Register,
    scratch: Register,
) -> Result<(), AotV2Error> {
    let row_offset = as_u64(section_index, "runtime section index")?
        .checked_mul(RUNTIME_SECTION_ROW_BYTE_LEN)
        .and_then(|offset| plan.section_rows_offset.checked_add(offset))
        .ok_or(AotV2Error::ArithmeticOverflow("runtime section row"))?;
    assembler.data_address(scratch, row_offset)?;
    emit_mov64_reg_mem(assembler, destination, scratch)?;
    assembler.add_reg64(destination, Register::R13)
}

fn emit_section_record_count(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    section_index: usize,
    destination: Register,
) -> Result<(), AotV2Error> {
    let row_offset = as_u64(section_index, "runtime section index")?
        .checked_mul(RUNTIME_SECTION_ROW_BYTE_LEN)
        .and_then(|offset| plan.section_rows_offset.checked_add(offset))
        .and_then(|offset| offset.checked_add(16))
        .ok_or(AotV2Error::ArithmeticOverflow(
            "runtime section record count",
        ))?;
    let scratch = if destination == Register::R11 {
        Register::R10
    } else {
        Register::R11
    };
    assembler.data_address(scratch, row_offset)?;
    emit_mov64_reg_mem(assembler, destination, scratch)
}

fn emit_section_record_address(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    section_index: usize,
    index: Register,
    stride: u64,
    destination: Register,
) -> Result<(), AotV2Error> {
    emit_section_base(assembler, plan, section_index, destination, Register::R11)?;
    assembler.mov_reg64(Register::Rax, index)?;
    assembler.mov_imm64(Register::Rcx, stride)?;
    emit_imul64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.add_reg64(destination, Register::Rax)
}

fn emit_storage_row_address(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    schema: Register,
    destination: Register,
) -> Result<(), AotV2Error> {
    assembler.data_address(destination, plan.storage_rows_offset)?;
    assembler.mov_reg64(Register::Rax, schema)?;
    assembler.mov_imm64(Register::Rcx, RUNTIME_STORAGE_ROW_BYTE_LEN)?;
    emit_imul64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.add_reg64(destination, Register::Rax)
}

fn emit_data_offset_address(
    assembler: &mut Assembler,
    offset: Register,
    destination: Register,
) -> Result<(), AotV2Error> {
    if destination != offset {
        assembler.mov_reg64(destination, offset)?;
    }
    assembler.add_reg64(destination, Register::R14)
}

fn literal_offset(plan: &NativeRuntimePlan, bytes: &[u8]) -> Result<u64, AotV2Error> {
    plan.literals
        .iter()
        .find(|literal| literal.bytes == bytes)
        .map(|literal| literal.offset)
        .ok_or_else(|| invalid_native("native runtime literal is absent from the data plan"))
}

fn emit_write_literal(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    labels: &RuntimeInternalLabels,
    fd: u64,
    bytes: &[u8],
) -> Result<(), AotV2Error> {
    assembler.mov_imm64(Register::Rdi, fd)?;
    assembler.data_address(Register::Rsi, literal_offset(plan, bytes)?)?;
    assembler.mov_imm64(
        Register::Rdx,
        as_u64(bytes.len(), "runtime literal length")?,
    )?;
    assembler.far_call(labels.write_all)
}

fn emit_require_metadata_u64(
    assembler: &mut Assembler,
    offset: u64,
    expected: u64,
    failure: Label,
) -> Result<(), AotV2Error> {
    assembler.metadata_address(Register::R11, offset)?;
    emit_mov64_reg_mem(assembler, Register::Rax, Register::R11)?;
    assembler.mov_imm64(Register::Rcx, expected)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, failure)
}

fn emit_require_metadata_u32(
    assembler: &mut Assembler,
    offset: u64,
    expected: u32,
    failure: Label,
) -> Result<(), AotV2Error> {
    assembler.metadata_address(Register::R11, offset)?;
    emit_mov32_reg_mem(assembler, Register::Rax, Register::R11)?;
    assembler.mov_imm64(Register::Rcx, u64::from(expected))?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, failure)
}

fn emit_require_record_u64(
    assembler: &mut Assembler,
    record: Register,
    offset: u64,
    expected: u64,
    failure: Label,
) -> Result<(), AotV2Error> {
    let scratch = if record == Register::R11 {
        Register::R10
    } else {
        Register::R11
    };
    assembler.mov_imm64(scratch, offset)?;
    assembler.add_reg64(scratch, record)?;
    emit_mov64_reg_mem(assembler, Register::Rax, scratch)?;
    assembler.mov_imm64(Register::Rcx, expected)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, failure)
}

fn emit_require_record_id(
    assembler: &mut Assembler,
    record: Register,
    offset: u64,
    expected: &[u8; 16],
    failure: Label,
) -> Result<(), AotV2Error> {
    let first = u64::from_le_bytes(
        expected[..8]
            .try_into()
            .map_err(|_| invalid_native("identifier prefix is truncated"))?,
    );
    let second = u64::from_le_bytes(
        expected[8..]
            .try_into()
            .map_err(|_| invalid_native("identifier suffix is truncated"))?,
    );
    emit_require_record_u64(assembler, record, offset, first, failure)?;
    emit_require_record_u64(
        assembler,
        record,
        offset
            .checked_add(8)
            .ok_or(AotV2Error::ArithmeticOverflow("identifier field"))?,
        second,
        failure,
    )
}

fn clone_manifest_string(
    package: &ExecutionPackage,
    index: u64,
    context: &'static str,
) -> Result<String, AotV2Error> {
    let value = package
        .strings
        .get(as_usize(index, "linked string index")?)
        .map(String::as_str)
        .ok_or_else(|| invalid_native("linked string reference is out of range"))?;
    let mut output = String::new();
    output
        .try_reserve_exact(value.len())
        .map_err(|_| AotV2Error::Allocation(context))?;
    output.push_str(value);
    Ok(output)
}

struct RecordStringExpectation<'a> {
    section_kind: u64,
    index: usize,
    stride: u64,
    field: u64,
    expected: &'a str,
}

#[derive(Clone, Copy)]
struct RecordReferenceValidation {
    section_kind: u64,
    stride: u64,
    string_field: Option<u64>,
    span_field: Option<u64>,
    body_span_range: bool,
}

fn emit_require_record_string_bytes(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    expectation: RecordStringExpectation<'_>,
    failure: Label,
) -> Result<(), AotV2Error> {
    assembler.emit(&[0x41, 0x52])?; // preserve caller r10 record pointer
    emit_section_record_constant(
        assembler,
        plan,
        section_index(expectation.section_kind)?,
        expectation.index,
        expectation.stride,
        Register::R10,
    )?;
    emit_load_record_field(assembler, Register::R10, expectation.field, Register::R8)?;
    emit_string_pointer(assembler, plan, Register::R8, failure)?;
    assembler.mov_imm64(
        Register::Rax,
        as_u64(expectation.expected.len(), "linked string length")?,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rdx, Register::Rax)?;
    assembler.far_jcc(Condition::NotEqual, failure)?;
    for (offset, expected_byte) in expectation.expected.bytes().enumerate() {
        assembler.mov_reg64(Register::R11, Register::Rsi)?;
        assembler.mov_imm64(Register::Rax, as_u64(offset, "linked string byte offset")?)?;
        assembler.add_reg64(Register::R11, Register::Rax)?;
        emit_mov8_reg_mem(assembler, Register::Rax, Register::R11)?;
        assembler.mov_imm64(Register::Rcx, u64::from(expected_byte))?;
        emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
        assembler.far_jcc(Condition::NotEqual, failure)?;
    }
    assembler.emit(&[0x41, 0x5a]) // restore caller r10
}

fn emit_section_record_constant(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    section_index: usize,
    index: usize,
    stride: u64,
    destination: Register,
) -> Result<(), AotV2Error> {
    emit_section_base(assembler, plan, section_index, destination, Register::R11)?;
    let offset = as_u64(index, "metadata record index")?
        .checked_mul(stride)
        .ok_or(AotV2Error::ArithmeticOverflow("metadata record offset"))?;
    assembler.mov_imm64(Register::Rax, offset)?;
    assembler.add_reg64(destination, Register::Rax)
}

fn emit_validate_and_initialize(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    entry: Label,
    labels: &RuntimeInternalLabels,
) -> Result<(), AotV2Error> {
    assembler.bind(entry)?;
    emit_runtime_prologue(assembler)?;

    assembler.data_address(Register::R11, plan.runtime_header_offset)?;
    emit_mov64_reg_mem(assembler, Register::Rax, Register::R11)?;
    assembler.mov_imm64(Register::Rcx, u64::from_le_bytes(*RUNTIME_MAGIC))?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, labels.failure)?;

    assembler.metadata_address(Register::R11, wire::envelope::header::MAGIC)?;
    emit_mov64_reg_mem(assembler, Register::Rax, Register::R11)?;
    assembler.mov_imm64(Register::Rcx, u64::from_le_bytes(*b"ARCHECMP"))?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::Equal, labels.archecmp)?;
    assembler.mov_imm64(Register::Rcx, u64::from_le_bytes(*b"ARCHEECS"))?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, labels.failure)?;
    assembler.metadata_address(Register::R11, wire::envelope::header::VERSION)?;
    emit_mov32_reg_mem(assembler, Register::Rax, Register::R11)?;
    assembler.mov_imm64(Register::Rcx, 1)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::Equal, labels.version_one)?;
    assembler.mov_imm64(Register::Rcx, u64::from(wire::VERSION))?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, labels.failure)?;
    emit_require_metadata_u32(
        assembler,
        wire::envelope::header::HEADER_SIZE,
        wire::HEADER_SIZE,
        labels.failure,
    )?;
    emit_require_metadata_u64(assembler, wire::envelope::header::FLAGS, 0, labels.failure)?;

    assembler.data_address(Register::R11, plan.runtime_header_offset + 8)?;
    emit_mov64_reg_mem(assembler, Register::Rcx, Register::R11)?;
    assembler.metadata_address(Register::R11, wire::envelope::header::TOTAL_LENGTH)?;
    emit_mov64_reg_mem(assembler, Register::Rax, Register::R11)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, labels.failure)?;
    emit_require_metadata_u64(
        assembler,
        wire::envelope::header::DIRECTORY_OFFSET,
        u64::from(wire::HEADER_SIZE),
        labels.failure,
    )?;
    emit_require_metadata_u64(
        assembler,
        wire::envelope::header::DIRECTORY_COUNT,
        as_u64(SECTION_KINDS.len(), "metadata section count")?,
        labels.failure,
    )?;
    emit_require_metadata_u64(
        assembler,
        wire::envelope::header::DIRECTORY_ENTRY_SIZE,
        wire::DIRECTORY_ENTRY_SIZE,
        labels.failure,
    )?;
    emit_require_metadata_u64(
        assembler,
        wire::envelope::header::RESERVED,
        0,
        labels.failure,
    )?;

    for (index, kind) in SECTION_KINDS.into_iter().enumerate() {
        let directory_offset = u64::from(wire::HEADER_SIZE)
            .checked_add(
                as_u64(index, "directory index")?
                    .checked_mul(wire::DIRECTORY_ENTRY_SIZE)
                    .ok_or(AotV2Error::ArithmeticOverflow("directory row"))?,
            )
            .ok_or(AotV2Error::ArithmeticOverflow("directory row"))?;
        assembler.metadata_address(Register::R10, directory_offset)?;
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::envelope::directory::KIND,
            kind,
            labels.failure,
        )?;
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::envelope::directory::FLAGS,
            0,
            labels.failure,
        )?;
        let runtime_row = plan
            .section_rows_offset
            .checked_add(
                as_u64(index, "runtime section row")?
                    .checked_mul(RUNTIME_SECTION_ROW_BYTE_LEN)
                    .ok_or(AotV2Error::ArithmeticOverflow("runtime section row"))?,
            )
            .ok_or(AotV2Error::ArithmeticOverflow("runtime section row"))?;
        assembler.data_address(Register::R11, runtime_row)?;
        for (metadata_field, runtime_field) in [
            (wire::envelope::directory::OFFSET, 0),
            (wire::envelope::directory::BYTE_LENGTH, 8),
            (wire::envelope::directory::RECORD_COUNT, 16),
            (wire::envelope::directory::RECORD_STRIDE, 24),
            (wire::envelope::directory::ALIGNMENT, 32),
        ] {
            assembler.mov_imm64(Register::Rax, metadata_field)?;
            assembler.add_reg64(Register::Rax, Register::R10)?;
            emit_mov64_reg_mem(assembler, Register::Rax, Register::Rax)?;
            assembler.mov_imm64(Register::Rcx, runtime_field)?;
            assembler.add_reg64(Register::Rcx, Register::R11)?;
            emit_mov64_mem_reg(assembler, Register::Rcx, Register::Rax)?;
        }
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::envelope::directory::RESERVED,
            0,
            labels.failure,
        )?;
    }

    emit_validate_directory_structure(assembler, plan, labels.failure)?;
    emit_validate_strings(assembler, plan, labels)?;
    emit_validate_linked_records(assembler, plan, labels.failure)?;
    emit_validate_references_and_spans(assembler, plan, labels.failure)?;
    emit_validate_trap_links(assembler, plan, labels.failure)?;
    emit_validate_function_ranges(assembler, plan, labels.failure)?;
    emit_validate_startup_flow(assembler, plan, labels.failure)?;

    assembler.data_address(Register::R11, plan.validated_offset)?;
    assembler.mov_imm64(Register::Rax, 1)?;
    emit_mov64_mem_reg(assembler, Register::R11, Register::Rax)?;
    emit_runtime_epilogue(assembler)
}

fn emit_validate_directory_structure(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    failure: Label,
) -> Result<(), AotV2Error> {
    let directory_end = u64::from(wire::HEADER_SIZE)
        .checked_add(
            as_u64(SECTION_KINDS.len(), "metadata section count")?
                .checked_mul(wire::DIRECTORY_ENTRY_SIZE)
                .ok_or(AotV2Error::ArithmeticOverflow("metadata directory"))?,
        )
        .ok_or(AotV2Error::ArithmeticOverflow("metadata directory"))?;
    assembler.mov_imm64(Register::Rbx, directory_end)?;
    for (index, kind) in SECTION_KINDS.into_iter().enumerate() {
        let directory_offset = u64::from(wire::HEADER_SIZE)
            .checked_add(
                as_u64(index, "directory index")?
                    .checked_mul(wire::DIRECTORY_ENTRY_SIZE)
                    .ok_or(AotV2Error::ArithmeticOverflow("directory row"))?,
            )
            .ok_or(AotV2Error::ArithmeticOverflow("directory row"))?;
        assembler.metadata_address(Register::R10, directory_offset)?;
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::envelope::directory::ALIGNMENT,
            wire::SECTION_ALIGNMENT,
            failure,
        )?;
        emit_load_record_field(
            assembler,
            Register::R10,
            wire::envelope::directory::OFFSET,
            Register::R8,
        )?;
        emit_cmp64_reg_reg(assembler, Register::R8, Register::Rbx)?;
        assembler.far_jcc(Condition::NotEqual, failure)?;
        if let Some((expected_count, stride)) = fixed_section_shape(plan, kind)? {
            emit_require_record_u64(
                assembler,
                Register::R10,
                wire::envelope::directory::RECORD_STRIDE,
                stride,
                failure,
            )?;
            emit_load_record_field(
                assembler,
                Register::R10,
                wire::envelope::directory::RECORD_COUNT,
                Register::R8,
            )?;
            if let Some(expected_count) = expected_count {
                assembler.mov_imm64(Register::Rax, expected_count)?;
                emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
                assembler.far_jcc(Condition::NotEqual, failure)?;
            }
            assembler.mov_reg64(Register::Rax, Register::R8)?;
            assembler.mov_imm64(Register::Rcx, stride)?;
            assembler.emit(&[0x48, 0xf7, 0xe1])?; // mul rcx
            emit_test64_reg_reg(assembler, Register::Rdx, Register::Rdx)?;
            assembler.far_jcc(Condition::NotZero, failure)?;
            emit_load_record_field(
                assembler,
                Register::R10,
                wire::envelope::directory::BYTE_LENGTH,
                Register::Rcx,
            )?;
            emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
            assembler.far_jcc(Condition::NotEqual, failure)?;
        } else {
            emit_require_record_u64(
                assembler,
                Register::R10,
                wire::envelope::directory::RECORD_COUNT,
                0,
                failure,
            )?;
            emit_require_record_u64(
                assembler,
                Register::R10,
                wire::envelope::directory::RECORD_STRIDE,
                0,
                failure,
            )?;
        }
        emit_load_record_field(
            assembler,
            Register::R10,
            wire::envelope::directory::BYTE_LENGTH,
            Register::R9,
        )?;
        assembler.add_reg64(Register::Rbx, Register::R9)?;
        assembler.far_jcc(Condition::Below, failure)?; // carry from checked addition
        if index + 1 != SECTION_KINDS.len() {
            assembler.mov_reg64(Register::R12, Register::Rbx)?;
            assembler.emit(&[0x48, 0x83, 0xc3, 0x07])?; // add rbx,7
            assembler.far_jcc(Condition::Below, failure)?;
            assembler.emit(&[0x48, 0x83, 0xe3, 0xf8])?; // and rbx,-8
            let padding = assembler.new_label()?;
            let padding_done = assembler.new_label()?;
            assembler.bind(padding)?;
            emit_cmp64_reg_reg(assembler, Register::R12, Register::Rbx)?;
            assembler.far_jcc(Condition::Equal, padding_done)?;
            assembler.mov_reg64(Register::R11, Register::R12)?;
            assembler.add_reg64(Register::R11, Register::R13)?;
            emit_mov8_reg_mem(assembler, Register::Rax, Register::R11)?;
            emit_test64_reg_reg(assembler, Register::Rax, Register::Rax)?;
            assembler.far_jcc(Condition::NotZero, failure)?;
            emit_increment64(assembler, Register::R12)?;
            assembler.far_jump(padding)?;
            assembler.bind(padding_done)?;
        }
    }
    assembler.metadata_address(Register::R11, wire::envelope::header::TOTAL_LENGTH)?;
    emit_mov64_reg_mem(assembler, Register::Rax, Register::R11)?;
    emit_cmp64_reg_reg(assembler, Register::Rbx, Register::Rax)?;
    assembler.far_jcc(Condition::NotEqual, failure)
}

fn fixed_section_shape(
    plan: &NativeRuntimePlan,
    kind: u64,
) -> Result<Option<(Option<u64>, u64)>, AotV2Error> {
    let count = match kind {
        wire::section_kind::STRINGS | wire::section_kind::PAYLOADS => return Ok(None),
        wire::section_kind::WORLD => Some(1),
        wire::section_kind::SCHEMAS => Some(plan.link_manifest.schemas.len()),
        wire::section_kind::FIELDS => Some(plan.link_manifest.fields.len()),
        wire::section_kind::SYSTEMS => Some(plan.link_manifest.systems.len()),
        wire::section_kind::PARAMETERS => Some(plan.link_manifest.parameters.len()),
        wire::section_kind::QUERIES => Some(plan.link_manifest.queries.len()),
        wire::section_kind::TERMS => Some(plan.link_manifest.terms.len()),
        wire::section_kind::SCHEDULES => Some(plan.link_manifest.schedules.len()),
        wire::section_kind::SCHEDULE_ITEMS | wire::section_kind::SOURCE_SPANS => None,
        wire::section_kind::STARTUP_OPERATIONS => None,
        wire::section_kind::FUNCTION_LINKS => Some(plan.link_manifest.function_count),
        _ => return Err(invalid_native("unknown canonical section kind")),
    };
    let stride = match kind {
        wire::section_kind::WORLD => wire::world::RECORD_SIZE,
        wire::section_kind::SCHEMAS => wire::schema::RECORD_SIZE,
        wire::section_kind::FIELDS => wire::field::RECORD_SIZE,
        wire::section_kind::SYSTEMS => wire::system::RECORD_SIZE,
        wire::section_kind::PARAMETERS => wire::parameter::RECORD_SIZE,
        wire::section_kind::QUERIES => wire::query::RECORD_SIZE,
        wire::section_kind::TERMS => wire::term::RECORD_SIZE,
        wire::section_kind::SCHEDULES => wire::schedule::RECORD_SIZE,
        wire::section_kind::SCHEDULE_ITEMS => wire::schedule_item::RECORD_SIZE,
        wire::section_kind::STARTUP_OPERATIONS => wire::startup_operation::RECORD_SIZE,
        wire::section_kind::FUNCTION_LINKS => wire::function_link::RECORD_SIZE,
        wire::section_kind::SOURCE_SPANS => wire::source_span::RECORD_SIZE,
        _ => return Err(invalid_native("raw section has no fixed stride")),
    };
    Ok(Some((
        count
            .map(|count| as_u64(count, "metadata record count"))
            .transpose()?,
        stride,
    )))
}

fn emit_validate_strings(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    labels: &RuntimeInternalLabels,
) -> Result<(), AotV2Error> {
    emit_section_base(
        assembler,
        plan,
        section_index(wire::section_kind::STRINGS)?,
        Register::R10,
        Register::R11,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::strings::COUNT,
        Register::Rbx,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::strings::BYTE_LENGTH,
        Register::Rbp,
    )?;
    assembler.mov_reg64(Register::Rax, Register::Rbx)?;
    assembler.mov_imm64(Register::Rcx, wire::strings::RECORD_SIZE)?;
    assembler.emit(&[0x48, 0xf7, 0xe1])?; // mul rcx
    emit_test64_reg_reg(assembler, Register::Rdx, Register::Rdx)?;
    assembler.far_jcc(Condition::NotZero, labels.failure)?;
    assembler.mov_imm64(Register::Rcx, wire::strings::HEADER_SIZE)?;
    assembler.add_reg64(Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::Below, labels.failure)?;
    assembler.add_reg64(Register::Rax, Register::Rbp)?;
    assembler.far_jcc(Condition::Below, labels.failure)?;
    let row_offset = plan
        .section_rows_offset
        .checked_add(
            as_u64(
                section_index(wire::section_kind::STRINGS)?,
                "string section index",
            )?
            .checked_mul(RUNTIME_SECTION_ROW_BYTE_LEN)
            .and_then(|offset| offset.checked_add(8))
            .ok_or(AotV2Error::ArithmeticOverflow("string section row"))?,
        )
        .ok_or(AotV2Error::ArithmeticOverflow("string section row"))?;
    assembler.data_address(Register::R11, row_offset)?;
    emit_mov64_reg_mem(assembler, Register::Rcx, Register::R11)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, labels.failure)?;

    assembler.mov_imm64(Register::R12, 0)?;
    assembler.mov_imm64(Register::R9, 0)?;
    let loop_label = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(loop_label)?;
    emit_cmp64_reg_reg(assembler, Register::R12, Register::Rbx)?;
    assembler.far_jcc(Condition::Equal, done)?;
    emit_section_base(
        assembler,
        plan,
        section_index(wire::section_kind::STRINGS)?,
        Register::R10,
        Register::R11,
    )?;
    assembler.mov_reg64(Register::Rax, Register::R12)?;
    assembler.mov_imm64(Register::Rcx, wire::strings::RECORD_SIZE)?;
    emit_imul64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.mov_imm64(Register::Rcx, wire::strings::HEADER_SIZE)?;
    assembler.add_reg64(Register::Rax, Register::Rcx)?;
    assembler.add_reg64(Register::R10, Register::Rax)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::strings::RECORD_OFFSET,
        Register::R8,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::R9)?;
    assembler.far_jcc(Condition::NotEqual, labels.failure)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::strings::RECORD_BYTE_LENGTH,
        Register::Rdx,
    )?;
    assembler.mov_reg64(Register::Rax, Register::R9)?;
    assembler.add_reg64(Register::Rax, Register::Rdx)?;
    assembler.far_jcc(Condition::Below, labels.failure)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rbp)?;
    assembler.far_jcc(Condition::Above, labels.failure)?;
    assembler.mov_reg64(Register::R9, Register::Rax)?;
    emit_section_base(
        assembler,
        plan,
        section_index(wire::section_kind::STRINGS)?,
        Register::Rdi,
        Register::R11,
    )?;
    assembler.mov_reg64(Register::Rax, Register::Rbx)?;
    assembler.mov_imm64(Register::Rcx, wire::strings::RECORD_SIZE)?;
    emit_imul64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.mov_imm64(Register::Rcx, wire::strings::HEADER_SIZE)?;
    assembler.add_reg64(Register::Rax, Register::Rcx)?;
    assembler.add_reg64(Register::Rdi, Register::Rax)?;
    assembler.add_reg64(Register::Rdi, Register::R8)?;
    assembler.far_call(labels.validate_utf8)?;

    let string_ordered = assembler.new_label()?;
    emit_test64_reg_reg(assembler, Register::R12, Register::R12)?;
    assembler.far_jcc(Condition::Zero, string_ordered)?;
    assembler.emit(&[0x41, 0x51])?; // push r9 (contiguous byte cursor)
    assembler.mov_reg64(Register::R8, Register::R12)?;
    emit_string_pointer(assembler, plan, Register::R8, labels.failure)?;
    assembler.emit(&[0x56, 0x52])?; // push current pointer; push current length
    assembler.mov_reg64(Register::R8, Register::R12)?;
    emit_decrement64(assembler, Register::R8)?;
    emit_string_pointer(assembler, plan, Register::R8, labels.failure)?;
    assembler.mov_reg64(Register::Rdi, Register::Rsi)?; // previous pointer
    assembler.emit(&[0x59, 0x5e])?; // pop current length; pop current pointer
    assembler.far_call(labels.compare_bytes)?;
    assembler.mov_imm64(Register::Rcx, u64::MAX)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, labels.failure)?;
    assembler.emit(&[0x41, 0x59])?; // pop r9
    assembler.bind(string_ordered)?;
    emit_increment64(assembler, Register::R12)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(done)?;
    emit_cmp64_reg_reg(assembler, Register::R9, Register::Rbp)?;
    assembler.far_jcc(Condition::NotEqual, labels.failure)
}

fn emit_validate_utf8_helper(
    assembler: &mut Assembler,
    labels: &RuntimeInternalLabels,
) -> Result<(), AotV2Error> {
    assembler.bind(labels.validate_utf8)?;
    emit_runtime_prologue(assembler)?;
    assembler.emit(&[0x41, 0x50, 0x41, 0x51])?; // push r8; push r9
    assembler.mov_reg64(Register::R8, Register::Rdi)?;
    assembler.mov_reg64(Register::R9, Register::Rdx)?;
    let loop_label = assembler.new_label()?;
    let two = assembler.new_label()?;
    let three = assembler.new_label()?;
    let four = assembler.new_label()?;
    let consume_one = assembler.new_label()?;
    let consume_two = assembler.new_label()?;
    let consume_three = assembler.new_label()?;
    let consume_four = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(loop_label)?;
    emit_test64_reg_reg(assembler, Register::R9, Register::R9)?;
    assembler.far_jcc(Condition::Zero, done)?;
    emit_mov8_reg_mem(assembler, Register::Rax, Register::R8)?;
    assembler.mov_imm64(Register::Rdx, 0x80)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    assembler.far_jcc(Condition::Below, consume_one)?;
    assembler.mov_imm64(Register::Rdx, 0xc2)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    assembler.far_jcc(Condition::Below, labels.failure)?;
    assembler.mov_imm64(Register::Rdx, 0xe0)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    assembler.far_jcc(Condition::Below, two)?;
    assembler.mov_imm64(Register::Rdx, 0xf0)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    assembler.far_jcc(Condition::Below, three)?;
    assembler.mov_imm64(Register::Rdx, 0xf5)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    assembler.far_jcc(Condition::Below, four)?;
    assembler.far_jump(labels.failure)?;

    assembler.bind(two)?;
    emit_require_remaining(assembler, Register::R9, 2, labels.failure)?;
    emit_require_utf8_byte(assembler, Register::R8, 1, 0x80, 0xbf, labels.failure)?;
    assembler.far_jump(consume_two)?;

    assembler.bind(three)?;
    emit_require_remaining(assembler, Register::R9, 3, labels.failure)?;
    let three_e0 = assembler.new_label()?;
    let three_ed = assembler.new_label()?;
    let three_tail = assembler.new_label()?;
    assembler.mov_imm64(Register::Rdx, 0xe0)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    assembler.far_jcc(Condition::Equal, three_e0)?;
    assembler.mov_imm64(Register::Rdx, 0xed)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    assembler.far_jcc(Condition::Equal, three_ed)?;
    emit_require_utf8_byte(assembler, Register::R8, 1, 0x80, 0xbf, labels.failure)?;
    assembler.far_jump(three_tail)?;
    assembler.bind(three_e0)?;
    emit_require_utf8_byte(assembler, Register::R8, 1, 0xa0, 0xbf, labels.failure)?;
    assembler.far_jump(three_tail)?;
    assembler.bind(three_ed)?;
    emit_require_utf8_byte(assembler, Register::R8, 1, 0x80, 0x9f, labels.failure)?;
    assembler.bind(three_tail)?;
    emit_require_utf8_byte(assembler, Register::R8, 2, 0x80, 0xbf, labels.failure)?;
    assembler.far_jump(consume_three)?;

    assembler.bind(four)?;
    emit_require_remaining(assembler, Register::R9, 4, labels.failure)?;
    let four_f0 = assembler.new_label()?;
    let four_f4 = assembler.new_label()?;
    let four_tail = assembler.new_label()?;
    assembler.mov_imm64(Register::Rdx, 0xf0)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    assembler.far_jcc(Condition::Equal, four_f0)?;
    assembler.mov_imm64(Register::Rdx, 0xf4)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    assembler.far_jcc(Condition::Equal, four_f4)?;
    emit_require_utf8_byte(assembler, Register::R8, 1, 0x80, 0xbf, labels.failure)?;
    assembler.far_jump(four_tail)?;
    assembler.bind(four_f0)?;
    emit_require_utf8_byte(assembler, Register::R8, 1, 0x90, 0xbf, labels.failure)?;
    assembler.far_jump(four_tail)?;
    assembler.bind(four_f4)?;
    emit_require_utf8_byte(assembler, Register::R8, 1, 0x80, 0x8f, labels.failure)?;
    assembler.bind(four_tail)?;
    emit_require_utf8_byte(assembler, Register::R8, 2, 0x80, 0xbf, labels.failure)?;
    emit_require_utf8_byte(assembler, Register::R8, 3, 0x80, 0xbf, labels.failure)?;
    assembler.far_jump(consume_four)?;

    assembler.bind(consume_one)?;
    emit_add64_imm8(assembler, Register::R8, 1)?;
    emit_sub64_imm8(assembler, Register::R9, 1)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(consume_two)?;
    emit_add64_imm8(assembler, Register::R8, 2)?;
    emit_sub64_imm8(assembler, Register::R9, 2)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(consume_three)?;
    emit_add64_imm8(assembler, Register::R8, 3)?;
    emit_sub64_imm8(assembler, Register::R9, 3)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(consume_four)?;
    emit_add64_imm8(assembler, Register::R8, 4)?;
    emit_sub64_imm8(assembler, Register::R9, 4)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(done)?;
    assembler.emit(&[0x41, 0x59, 0x41, 0x58])?; // pop r9; pop r8
    emit_runtime_epilogue(assembler)
}

fn emit_require_remaining(
    assembler: &mut Assembler,
    remaining: Register,
    required: u64,
    failure: Label,
) -> Result<(), AotV2Error> {
    assembler.mov_imm64(Register::Rdx, required)?;
    emit_cmp64_reg_reg(assembler, remaining, Register::Rdx)?;
    assembler.far_jcc(Condition::Below, failure)
}

fn emit_require_utf8_byte(
    assembler: &mut Assembler,
    pointer: Register,
    offset: u64,
    minimum: u64,
    maximum: u64,
    failure: Label,
) -> Result<(), AotV2Error> {
    assembler.mov_imm64(Register::R10, offset)?;
    assembler.add_reg64(Register::R10, pointer)?;
    emit_mov8_reg_mem(assembler, Register::Rcx, Register::R10)?;
    assembler.mov_imm64(Register::Rdx, minimum)?;
    emit_cmp64_reg_reg(assembler, Register::Rcx, Register::Rdx)?;
    assembler.far_jcc(Condition::Below, failure)?;
    assembler.mov_imm64(Register::Rdx, maximum)?;
    emit_cmp64_reg_reg(assembler, Register::Rcx, Register::Rdx)?;
    assembler.far_jcc(Condition::Above, failure)
}

fn emit_validate_linked_records(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    failure: Label,
) -> Result<(), AotV2Error> {
    let package = &plan.link_manifest;

    emit_section_record_constant(
        assembler,
        plan,
        section_index(wire::section_kind::WORLD)?,
        0,
        wire::world::RECORD_SIZE,
        Register::R10,
    )?;
    emit_require_record_id(
        assembler,
        Register::R10,
        wire::world::STARTUP_ABI_HASH,
        package.world.startup_abi_hash.as_bytes(),
        failure,
    )?;
    emit_require_record_id(
        assembler,
        Register::R10,
        wire::world::STARTUP_BODY_HASH,
        package.world.startup_body_hash.as_bytes(),
        failure,
    )?;
    emit_require_record_string_bytes(
        assembler,
        plan,
        RecordStringExpectation {
            section_kind: wire::section_kind::WORLD,
            index: 0,
            stride: wire::world::RECORD_SIZE,
            field: wire::world::NAME,
            expected: package.world.name.as_str(),
        },
        failure,
    )?;
    emit_zero_record_tail(
        assembler,
        Register::R10,
        wire::world::RESERVED,
        wire::world::RECORD_SIZE,
        failure,
    )?;

    for (index, record) in package.schemas.iter().enumerate() {
        emit_section_record_constant(
            assembler,
            plan,
            section_index(wire::section_kind::SCHEMAS)?,
            index,
            wire::schema::RECORD_SIZE,
            Register::R10,
        )?;
        emit_require_record_id(
            assembler,
            Register::R10,
            wire::schema::ID,
            record.id.as_bytes(),
            failure,
        )?;
        emit_require_record_string_bytes(
            assembler,
            plan,
            RecordStringExpectation {
                section_kind: wire::section_kind::SCHEMAS,
                index,
                stride: wire::schema::RECORD_SIZE,
                field: wire::schema::NAME,
                expected: record.name.as_str(),
            },
            failure,
        )?;
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::schema::KIND,
            u64::from(record.kind as u8),
            failure,
        )?;
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::schema::BYTE_SIZE,
            record.byte_size,
            failure,
        )?;
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::schema::ALIGNMENT,
            record.alignment,
            failure,
        )?;
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::schema::FLAGS,
            record.flags,
            failure,
        )?;
        emit_zero_record_tail(
            assembler,
            Register::R10,
            wire::schema::RESERVED,
            wire::schema::RECORD_SIZE,
            failure,
        )?;
        emit_validate_storage_link(assembler, plan, index, failure)?;
    }

    for (index, record) in package.fields.iter().enumerate() {
        emit_section_record_constant(
            assembler,
            plan,
            section_index(wire::section_kind::FIELDS)?,
            index,
            wire::field::RECORD_SIZE,
            Register::R10,
        )?;
        emit_require_record_string_bytes(
            assembler,
            plan,
            RecordStringExpectation {
                section_kind: wire::section_kind::FIELDS,
                index,
                stride: wire::field::RECORD_SIZE,
                field: wire::field::NAME,
                expected: record.name.as_str(),
            },
            failure,
        )?;
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::field::SCHEMA,
            record.schema,
            failure,
        )?;
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::field::PRIMITIVE,
            u64::from(record.primitive as u8),
            failure,
        )?;
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::field::BYTE_OFFSET,
            record.byte_offset,
            failure,
        )?;
        emit_zero_record_tail(
            assembler,
            Register::R10,
            wire::field::RESERVED,
            wire::field::RECORD_SIZE,
            failure,
        )?;
    }

    for (index, record) in package.systems.iter().enumerate() {
        emit_section_record_constant(
            assembler,
            plan,
            section_index(wire::section_kind::SYSTEMS)?,
            index,
            wire::system::RECORD_SIZE,
            Register::R10,
        )?;
        emit_require_record_id(
            assembler,
            Register::R10,
            wire::system::ID,
            record.id.as_bytes(),
            failure,
        )?;
        emit_require_record_string_bytes(
            assembler,
            plan,
            RecordStringExpectation {
                section_kind: wire::section_kind::SYSTEMS,
                index,
                stride: wire::system::RECORD_SIZE,
                field: wire::system::NAME,
                expected: record.name.as_str(),
            },
            failure,
        )?;
        emit_require_record_id(
            assembler,
            Register::R10,
            wire::system::ABI_HASH,
            record.abi_hash.as_bytes(),
            failure,
        )?;
        emit_require_record_id(
            assembler,
            Register::R10,
            wire::system::BODY_HASH,
            record.body_hash.as_bytes(),
            failure,
        )?;
        emit_zero_record_tail(
            assembler,
            Register::R10,
            wire::system::RESERVED,
            wire::system::RECORD_SIZE,
            failure,
        )?;
    }

    for (index, record) in package.parameters.iter().enumerate() {
        emit_section_record_constant(
            assembler,
            plan,
            section_index(wire::section_kind::PARAMETERS)?,
            index,
            wire::parameter::RECORD_SIZE,
            Register::R10,
        )?;
        emit_require_record_string_bytes(
            assembler,
            plan,
            RecordStringExpectation {
                section_kind: wire::section_kind::PARAMETERS,
                index,
                stride: wire::parameter::RECORD_SIZE,
                field: wire::parameter::NAME,
                expected: record.name.as_str(),
            },
            failure,
        )?;
        let (kind, target) = (record.kind, record.target);
        for (offset, expected) in [
            (wire::parameter::SYSTEM, record.system),
            (wire::parameter::KIND, kind),
            (wire::parameter::TARGET, target),
        ] {
            emit_require_record_u64(assembler, Register::R10, offset, expected, failure)?;
        }
        emit_zero_record_tail(
            assembler,
            Register::R10,
            wire::parameter::RESERVED,
            wire::parameter::RECORD_SIZE,
            failure,
        )?;
    }

    for (index, record) in package.queries.iter().enumerate() {
        emit_section_record_constant(
            assembler,
            plan,
            section_index(wire::section_kind::QUERIES)?,
            index,
            wire::query::RECORD_SIZE,
            Register::R10,
        )?;
        emit_require_record_id(
            assembler,
            Register::R10,
            wire::query::ID,
            record.id.as_bytes(),
            failure,
        )?;
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::query::SYSTEM,
            record.system,
            failure,
        )?;
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::query::PARAMETER,
            record.parameter,
            failure,
        )?;
        emit_zero_record_tail(
            assembler,
            Register::R10,
            wire::query::RESERVED,
            wire::query::RECORD_SIZE,
            failure,
        )?;
    }

    for (index, record) in package.terms.iter().enumerate() {
        emit_section_record_constant(
            assembler,
            plan,
            section_index(wire::section_kind::TERMS)?,
            index,
            wire::term::RECORD_SIZE,
            Register::R10,
        )?;
        let access = record.access;
        for (offset, expected) in [
            (wire::term::QUERY, record.query),
            (wire::term::ACCESS, access),
            (wire::term::SCHEMA, record.schema),
        ] {
            emit_require_record_u64(assembler, Register::R10, offset, expected, failure)?;
        }
        emit_zero_record_tail(
            assembler,
            Register::R10,
            wire::term::RESERVED,
            wire::term::RECORD_SIZE,
            failure,
        )?;
    }

    for (index, record) in package.schedules.iter().enumerate() {
        emit_section_record_constant(
            assembler,
            plan,
            section_index(wire::section_kind::SCHEDULES)?,
            index,
            wire::schedule::RECORD_SIZE,
            Register::R10,
        )?;
        emit_require_record_id(
            assembler,
            Register::R10,
            wire::schedule::ID,
            record.id.as_bytes(),
            failure,
        )?;
        emit_require_record_string_bytes(
            assembler,
            plan,
            RecordStringExpectation {
                section_kind: wire::section_kind::SCHEDULES,
                index,
                stride: wire::schedule::RECORD_SIZE,
                field: wire::schedule::NAME,
                expected: record.name.as_str(),
            },
            failure,
        )?;
        emit_zero_record_tail(
            assembler,
            Register::R10,
            wire::schedule::RESERVED,
            wire::schedule::RECORD_SIZE,
            failure,
        )?;
    }

    for index in 0..package.function_count {
        emit_section_record_constant(
            assembler,
            plan,
            section_index(wire::section_kind::FUNCTION_LINKS)?,
            index,
            wire::function_link::RECORD_SIZE,
            Register::R10,
        )?;
        let (kind, system, abi_hash, body_hash) = if index == 0 {
            (
                wire::function_link::STARTUP,
                u64::MAX,
                package.world.startup_abi_hash.as_bytes(),
                package.world.startup_body_hash.as_bytes(),
            )
        } else {
            let system_index = index - 1;
            let system_record = package
                .systems
                .get(system_index)
                .ok_or_else(|| invalid_native("function manifest has no linked system"))?;
            (
                wire::function_link::SYSTEM_TARGET,
                as_u64(system_index, "function system index")?,
                system_record.abi_hash.as_bytes(),
                system_record.body_hash.as_bytes(),
            )
        };
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::function_link::KIND,
            kind,
            failure,
        )?;
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::function_link::SYSTEM,
            system,
            failure,
        )?;
        emit_require_record_id(
            assembler,
            Register::R10,
            wire::function_link::ABI_HASH,
            abi_hash,
            failure,
        )?;
        emit_require_record_id(
            assembler,
            Register::R10,
            wire::function_link::BODY_HASH,
            body_hash,
            failure,
        )?;
        let runtime_row = plan
            .function_rows_offset
            .checked_add(
                as_u64(index, "runtime function index")?
                    .checked_mul(RUNTIME_FUNCTION_ROW_BYTE_LEN)
                    .ok_or(AotV2Error::ArithmeticOverflow("runtime function row"))?,
            )
            .ok_or(AotV2Error::ArithmeticOverflow("runtime function row"))?;
        assembler.data_address(Register::R11, runtime_row)?;
        for (metadata_offset, runtime_offset) in [
            (wire::function_link::CODE_OFFSET, 0),
            (wire::function_link::CODE_BYTE_LENGTH, 8),
        ] {
            assembler.mov_imm64(Register::Rax, metadata_offset)?;
            assembler.add_reg64(Register::Rax, Register::R10)?;
            emit_mov64_reg_mem(assembler, Register::Rax, Register::Rax)?;
            assembler.mov_imm64(Register::Rcx, runtime_offset)?;
            assembler.add_reg64(Register::Rcx, Register::R11)?;
            emit_mov64_reg_mem(assembler, Register::Rcx, Register::Rcx)?;
            emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
            assembler.far_jcc(Condition::NotEqual, failure)?;
        }
    }
    Ok(())
}

fn emit_validate_storage_link(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    index: usize,
    failure: Label,
) -> Result<(), AotV2Error> {
    emit_section_record_constant(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEMAS)?,
        index,
        wire::schema::RECORD_SIZE,
        Register::R10,
    )?;
    assembler.mov_imm64(Register::R8, as_u64(index, "runtime storage index")?)?;
    emit_storage_row_address(assembler, plan, Register::R8, Register::R12)?;
    for (metadata_field, storage_field) in [
        (wire::schema::ID, RUNTIME_STORAGE_ID),
        (wire::schema::ID + 8, RUNTIME_STORAGE_ID + 8),
        (wire::schema::KIND, RUNTIME_STORAGE_KIND),
        (wire::schema::FLAGS, RUNTIME_STORAGE_FLAGS),
        (wire::schema::BYTE_SIZE, RUNTIME_STORAGE_BYTE_SIZE),
        (wire::schema::ALIGNMENT, RUNTIME_STORAGE_ALIGNMENT),
    ] {
        emit_load_record_field(assembler, Register::R10, metadata_field, Register::Rax)?;
        emit_load_record_field(assembler, Register::R12, storage_field, Register::Rcx)?;
        emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
        assembler.far_jcc(Condition::NotEqual, failure)?;
    }
    emit_load_record_field(assembler, Register::R12, RUNTIME_STORAGE_KIND, Register::R8)?;
    emit_load_record_field(
        assembler,
        Register::R12,
        RUNTIME_STORAGE_RESOURCE_INITIALIZED,
        Register::R9,
    )?;
    emit_load_record_field(
        assembler,
        Register::R12,
        RUNTIME_STORAGE_RESOURCE_PAYLOAD,
        Register::Rdx,
    )?;
    emit_load_record_field(
        assembler,
        Register::R12,
        RUNTIME_STORAGE_ROW_CELL,
        Register::Rax,
    )?;
    let resource = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.mov_imm64(Register::Rcx, wire::schema::RESOURCE)?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rcx)?;
    assembler.far_jcc(Condition::Equal, resource)?;
    assembler.mov_imm64(Register::Rcx, u64::MAX)?;
    for value in [Register::R9, Register::Rdx] {
        emit_cmp64_reg_reg(assembler, value, Register::Rcx)?;
        assembler.far_jcc(Condition::NotEqual, failure)?;
    }
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::Equal, failure)?;
    assembler.far_jump(done)?;
    assembler.bind(resource)?;
    assembler.mov_imm64(Register::Rcx, u64::MAX)?;
    for value in [Register::R9, Register::Rdx] {
        emit_cmp64_reg_reg(assembler, value, Register::Rcx)?;
        assembler.far_jcc(Condition::Equal, failure)?;
    }
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, failure)?;
    assembler.bind(done)
}

fn emit_validate_references_and_spans(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    failure: Label,
) -> Result<(), AotV2Error> {
    for validation in [
        RecordReferenceValidation {
            section_kind: wire::section_kind::WORLD,
            stride: wire::world::RECORD_SIZE,
            string_field: Some(wire::world::NAME),
            span_field: Some(wire::world::SOURCE_SPAN),
            body_span_range: false,
        },
        RecordReferenceValidation {
            section_kind: wire::section_kind::SCHEMAS,
            stride: wire::schema::RECORD_SIZE,
            string_field: Some(wire::schema::NAME),
            span_field: Some(wire::schema::SOURCE_SPAN),
            body_span_range: false,
        },
        RecordReferenceValidation {
            section_kind: wire::section_kind::FIELDS,
            stride: wire::field::RECORD_SIZE,
            string_field: Some(wire::field::NAME),
            span_field: Some(wire::field::SOURCE_SPAN),
            body_span_range: false,
        },
        RecordReferenceValidation {
            section_kind: wire::section_kind::SYSTEMS,
            stride: wire::system::RECORD_SIZE,
            string_field: Some(wire::system::NAME),
            span_field: Some(wire::system::SOURCE_SPAN),
            body_span_range: false,
        },
        RecordReferenceValidation {
            section_kind: wire::section_kind::PARAMETERS,
            stride: wire::parameter::RECORD_SIZE,
            string_field: Some(wire::parameter::NAME),
            span_field: Some(wire::parameter::SOURCE_SPAN),
            body_span_range: false,
        },
        RecordReferenceValidation {
            section_kind: wire::section_kind::QUERIES,
            stride: wire::query::RECORD_SIZE,
            string_field: None,
            span_field: Some(wire::query::SOURCE_SPAN),
            body_span_range: false,
        },
        RecordReferenceValidation {
            section_kind: wire::section_kind::TERMS,
            stride: wire::term::RECORD_SIZE,
            string_field: None,
            span_field: Some(wire::term::SOURCE_SPAN),
            body_span_range: false,
        },
        RecordReferenceValidation {
            section_kind: wire::section_kind::SCHEDULES,
            stride: wire::schedule::RECORD_SIZE,
            string_field: Some(wire::schedule::NAME),
            span_field: Some(wire::schedule::SOURCE_SPAN),
            body_span_range: false,
        },
        RecordReferenceValidation {
            section_kind: wire::section_kind::SCHEDULE_ITEMS,
            stride: wire::schedule_item::RECORD_SIZE,
            string_field: None,
            span_field: Some(wire::schedule_item::SOURCE_SPAN),
            body_span_range: false,
        },
        RecordReferenceValidation {
            section_kind: wire::section_kind::STARTUP_OPERATIONS,
            stride: wire::startup_operation::RECORD_SIZE,
            string_field: None,
            span_field: Some(wire::startup_operation::SOURCE_SPAN),
            body_span_range: false,
        },
        RecordReferenceValidation {
            section_kind: wire::section_kind::FUNCTION_LINKS,
            stride: wire::function_link::RECORD_SIZE,
            string_field: Some(wire::function_link::SYMBOL_NAME),
            span_field: Some(wire::function_link::SOURCE_SPAN),
            body_span_range: true,
        },
    ] {
        emit_validate_record_references(assembler, plan, validation, failure)?;
    }

    assembler.mov_imm64(Register::Rbp, 0)?;
    let loop_label = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(loop_label)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SOURCE_SPANS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbp, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, done)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::SOURCE_SPANS)?,
        Register::Rbp,
        wire::source_span::RECORD_SIZE,
        Register::R12,
    )?;
    emit_load_record_field(
        assembler,
        Register::R12,
        wire::source_span::START_BYTE,
        Register::R8,
    )?;
    emit_load_record_field(
        assembler,
        Register::R12,
        wire::source_span::END_BYTE,
        Register::R9,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::R9)?;
    assembler.far_jcc(Condition::Above, failure)?;
    emit_load_record_field(
        assembler,
        Register::R12,
        wire::source_span::START_LINE,
        Register::R8,
    )?;
    emit_load_record_field(
        assembler,
        Register::R12,
        wire::source_span::END_LINE,
        Register::R9,
    )?;
    emit_load_record_field(
        assembler,
        Register::R12,
        wire::source_span::START_COLUMN,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R12,
        wire::source_span::END_COLUMN,
        Register::R11,
    )?;
    for coordinate in [Register::R8, Register::R9, Register::R10, Register::R11] {
        emit_test64_reg_reg(assembler, coordinate, coordinate)?;
        assembler.far_jcc(Condition::Zero, failure)?;
    }
    emit_cmp64_reg_reg(assembler, Register::R9, Register::R8)?;
    assembler.far_jcc(Condition::Below, failure)?;
    let coordinate_ok = assembler.new_label()?;
    assembler.far_jcc(Condition::Above, coordinate_ok)?;
    emit_cmp64_reg_reg(assembler, Register::R11, Register::R10)?;
    assembler.far_jcc(Condition::Below, failure)?;
    assembler.bind(coordinate_ok)?;
    let location_nonempty = assembler.new_label()?;
    let endpoints_agree = assembler.new_label()?;
    emit_cmp64_reg_reg(assembler, Register::R9, Register::R8)?;
    assembler.far_jcc(Condition::NotEqual, location_nonempty)?;
    emit_cmp64_reg_reg(assembler, Register::R11, Register::R10)?;
    assembler.far_jcc(Condition::NotEqual, location_nonempty)?;
    emit_load_record_field(
        assembler,
        Register::R12,
        wire::source_span::START_BYTE,
        Register::Rax,
    )?;
    emit_load_record_field(
        assembler,
        Register::R12,
        wire::source_span::END_BYTE,
        Register::Rcx,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, failure)?;
    assembler.far_jump(endpoints_agree)?;
    assembler.bind(location_nonempty)?;
    emit_load_record_field(
        assembler,
        Register::R12,
        wire::source_span::START_BYTE,
        Register::Rax,
    )?;
    emit_load_record_field(
        assembler,
        Register::R12,
        wire::source_span::END_BYTE,
        Register::Rcx,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::Equal, failure)?;
    assembler.bind(endpoints_agree)?;
    emit_zero_record_tail(
        assembler,
        Register::R12,
        wire::source_span::RESERVED,
        wire::source_span::RECORD_SIZE,
        failure,
    )?;
    emit_validate_string_ref_at_record(
        assembler,
        plan,
        Register::R12,
        wire::source_span::FILE_NAME,
        failure,
    )?;
    let order_done = assembler.new_label()?;
    emit_test64_reg_reg(assembler, Register::Rbp, Register::Rbp)?;
    assembler.far_jcc(Condition::Zero, order_done)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::SOURCE_SPANS)?,
        Register::Rbp,
        wire::source_span::RECORD_SIZE,
        Register::R12,
    )?;
    assembler.mov_reg64(Register::R8, Register::Rbp)?;
    emit_decrement64(assembler, Register::R8)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::SOURCE_SPANS)?,
        Register::R8,
        wire::source_span::RECORD_SIZE,
        Register::R10,
    )?;
    for field in [
        wire::source_span::FILE_NAME,
        wire::source_span::START_BYTE,
        wire::source_span::END_BYTE,
    ] {
        emit_load_record_field(assembler, Register::R10, field, Register::R8)?;
        emit_load_record_field(assembler, Register::R12, field, Register::R9)?;
        emit_cmp64_reg_reg(assembler, Register::R9, Register::R8)?;
        assembler.far_jcc(Condition::Below, failure)?;
        assembler.far_jcc(Condition::Above, order_done)?;
    }
    assembler.far_jump(failure)?; // duplicate canonical key
    assembler.bind(order_done)?;
    emit_increment64(assembler, Register::Rbp)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(done)?;
    emit_validate_function_body_spans(assembler, plan, failure)?;
    emit_validate_all_strings_used(assembler, plan, failure)?;
    emit_validate_all_spans_used(assembler, plan, failure)
}

fn emit_validate_record_references(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    validation: RecordReferenceValidation,
    failure: Label,
) -> Result<(), AotV2Error> {
    assembler.mov_imm64(Register::Rbp, 0)?;
    let loop_label = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(loop_label)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(validation.section_kind)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbp, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, done)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(validation.section_kind)?,
        Register::Rbp,
        validation.stride,
        Register::R10,
    )?;
    if let Some(field) = validation.string_field {
        emit_validate_string_ref_at_record(assembler, plan, Register::R10, field, failure)?;
    }
    if let Some(field) = validation.span_field {
        emit_validate_optional_span_at_record(assembler, plan, Register::R10, field, failure)?;
    }
    if validation.body_span_range {
        // An absent optional span reaches its continuation through a taken
        // far transfer, whose documented scratch registers are r10/r11.
        // Reconstruct the function-link record before validating its body.
        emit_section_record_address(
            assembler,
            plan,
            section_index(validation.section_kind)?,
            Register::Rbp,
            validation.stride,
            Register::R10,
        )?;
        emit_validate_function_body_span_range_at_record(assembler, plan, Register::R10, failure)?;
    }
    emit_increment64(assembler, Register::Rbp)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(done)
}

fn emit_validate_string_ref_at_record(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    record: Register,
    field: u64,
    failure: Label,
) -> Result<(), AotV2Error> {
    emit_load_record_field(assembler, record, field, Register::R8)?;
    emit_section_base(
        assembler,
        plan,
        section_index(wire::section_kind::STRINGS)?,
        Register::R11,
        Register::R9,
    )?;
    assembler.mov_imm64(Register::R9, wire::strings::COUNT)?;
    assembler.add_reg64(Register::R9, Register::R11)?;
    emit_mov64_reg_mem(assembler, Register::Rax, Register::R9)?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, failure)
}

fn emit_validate_optional_span_at_record(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    record: Register,
    field: u64,
    failure: Label,
) -> Result<(), AotV2Error> {
    emit_load_record_field(assembler, record, field, Register::R8)?;
    let done = assembler.new_label()?;
    assembler.mov_imm64(Register::Rax, wire::NONE_REFERENCE)?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::Equal, done)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SOURCE_SPANS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, failure)?;
    assembler.bind(done)
}

fn emit_validate_function_body_span_range_at_record(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    record: Register,
    failure: Label,
) -> Result<(), AotV2Error> {
    emit_load_record_field(
        assembler,
        record,
        wire::function_link::FIRST_BODY_SPAN,
        Register::R8,
    )?;
    emit_load_record_field(
        assembler,
        record,
        wire::function_link::BODY_SPAN_COUNT,
        Register::R9,
    )?;
    emit_load_record_field(
        assembler,
        record,
        wire::function_link::SOURCE_SPAN,
        Register::R12,
    )?;
    let present = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.mov_imm64(Register::Rax, wire::NONE_REFERENCE)?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::NotEqual, present)?;
    emit_test64_reg_reg(assembler, Register::R9, Register::R9)?;
    assembler.far_jcc(Condition::NotZero, failure)?;
    assembler.far_jump(done)?;
    assembler.bind(present)?;
    emit_test64_reg_reg(assembler, Register::R9, Register::R9)?;
    assembler.far_jcc(Condition::Zero, failure)?;
    assembler.mov_imm64(Register::Rax, wire::NONE_REFERENCE)?;
    emit_cmp64_reg_reg(assembler, Register::R12, Register::Rax)?;
    assembler.far_jcc(Condition::Equal, failure)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SOURCE_SPANS)?,
        Register::Rcx,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rcx)?;
    assembler.far_jcc(Condition::AboveEqual, failure)?;
    assembler.mov_reg64(Register::Rax, Register::R8)?;
    assembler.add_reg64(Register::Rax, Register::R9)?;
    assembler.far_jcc(Condition::Below, failure)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SOURCE_SPANS)?,
        Register::Rcx,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::Above, failure)?;
    assembler.bind(done)
}

fn emit_jump_if_record_field_uses_index(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    section_kind: u64,
    stride: u64,
    field: u64,
    used: Label,
) -> Result<(), AotV2Error> {
    assembler.mov_imm64(Register::Rbp, 0)?;
    let loop_label = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(loop_label)?;
    emit_section_record_count(assembler, plan, section_index(section_kind)?, Register::Rax)?;
    emit_cmp64_reg_reg(assembler, Register::Rbp, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, done)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(section_kind)?,
        Register::Rbp,
        stride,
        Register::R10,
    )?;
    emit_load_record_field(assembler, Register::R10, field, Register::Rax)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rbx)?;
    assembler.far_jcc(Condition::Equal, used)?;
    emit_increment64(assembler, Register::Rbp)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(done)
}

fn emit_validate_all_strings_used(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    failure: Label,
) -> Result<(), AotV2Error> {
    assembler.mov_imm64(Register::Rbx, 0)?;
    let loop_label = assembler.new_label()?;
    let used = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(loop_label)?;
    emit_section_base(
        assembler,
        plan,
        section_index(wire::section_kind::STRINGS)?,
        Register::R10,
        Register::R11,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::strings::COUNT,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbx, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, done)?;
    for (section_kind, stride, field) in [
        (
            wire::section_kind::WORLD,
            wire::world::RECORD_SIZE,
            wire::world::NAME,
        ),
        (
            wire::section_kind::SCHEMAS,
            wire::schema::RECORD_SIZE,
            wire::schema::NAME,
        ),
        (
            wire::section_kind::FIELDS,
            wire::field::RECORD_SIZE,
            wire::field::NAME,
        ),
        (
            wire::section_kind::SYSTEMS,
            wire::system::RECORD_SIZE,
            wire::system::NAME,
        ),
        (
            wire::section_kind::PARAMETERS,
            wire::parameter::RECORD_SIZE,
            wire::parameter::NAME,
        ),
        (
            wire::section_kind::SCHEDULES,
            wire::schedule::RECORD_SIZE,
            wire::schedule::NAME,
        ),
        (
            wire::section_kind::FUNCTION_LINKS,
            wire::function_link::RECORD_SIZE,
            wire::function_link::SYMBOL_NAME,
        ),
        (
            wire::section_kind::SOURCE_SPANS,
            wire::source_span::RECORD_SIZE,
            wire::source_span::FILE_NAME,
        ),
    ] {
        emit_jump_if_record_field_uses_index(assembler, plan, section_kind, stride, field, used)?;
    }
    assembler.far_jump(failure)?;
    assembler.bind(used)?;
    emit_increment64(assembler, Register::Rbx)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(done)
}

fn emit_jump_if_body_range_uses_span(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    used: Label,
) -> Result<(), AotV2Error> {
    assembler.mov_imm64(Register::Rbp, 0)?;
    let loop_label = assembler.new_label()?;
    let next = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(loop_label)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::FUNCTION_LINKS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbp, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, done)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::FUNCTION_LINKS)?,
        Register::Rbp,
        wire::function_link::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::function_link::FIRST_BODY_SPAN,
        Register::R8,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbx, Register::R8)?;
    assembler.far_jcc(Condition::Below, next)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::function_link::BODY_SPAN_COUNT,
        Register::R9,
    )?;
    assembler.mov_reg64(Register::Rax, Register::R8)?;
    assembler.add_reg64(Register::Rax, Register::R9)?;
    emit_cmp64_reg_reg(assembler, Register::Rbx, Register::Rax)?;
    assembler.far_jcc(Condition::Below, used)?;
    assembler.bind(next)?;
    emit_increment64(assembler, Register::Rbp)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(done)
}

fn emit_validate_all_spans_used(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    failure: Label,
) -> Result<(), AotV2Error> {
    assembler.mov_imm64(Register::Rbx, 0)?;
    let loop_label = assembler.new_label()?;
    let used = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(loop_label)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SOURCE_SPANS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbx, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, done)?;
    for (section_kind, stride, field) in [
        (
            wire::section_kind::WORLD,
            wire::world::RECORD_SIZE,
            wire::world::SOURCE_SPAN,
        ),
        (
            wire::section_kind::SCHEMAS,
            wire::schema::RECORD_SIZE,
            wire::schema::SOURCE_SPAN,
        ),
        (
            wire::section_kind::FIELDS,
            wire::field::RECORD_SIZE,
            wire::field::SOURCE_SPAN,
        ),
        (
            wire::section_kind::SYSTEMS,
            wire::system::RECORD_SIZE,
            wire::system::SOURCE_SPAN,
        ),
        (
            wire::section_kind::PARAMETERS,
            wire::parameter::RECORD_SIZE,
            wire::parameter::SOURCE_SPAN,
        ),
        (
            wire::section_kind::QUERIES,
            wire::query::RECORD_SIZE,
            wire::query::SOURCE_SPAN,
        ),
        (
            wire::section_kind::TERMS,
            wire::term::RECORD_SIZE,
            wire::term::SOURCE_SPAN,
        ),
        (
            wire::section_kind::SCHEDULES,
            wire::schedule::RECORD_SIZE,
            wire::schedule::SOURCE_SPAN,
        ),
        (
            wire::section_kind::SCHEDULE_ITEMS,
            wire::schedule_item::RECORD_SIZE,
            wire::schedule_item::SOURCE_SPAN,
        ),
        (
            wire::section_kind::STARTUP_OPERATIONS,
            wire::startup_operation::RECORD_SIZE,
            wire::startup_operation::SOURCE_SPAN,
        ),
        (
            wire::section_kind::FUNCTION_LINKS,
            wire::function_link::RECORD_SIZE,
            wire::function_link::SOURCE_SPAN,
        ),
    ] {
        emit_jump_if_record_field_uses_index(assembler, plan, section_kind, stride, field, used)?;
    }
    emit_jump_if_body_range_uses_span(assembler, plan, used)?;
    assembler.far_jump(failure)?;
    assembler.bind(used)?;
    emit_increment64(assembler, Register::Rbx)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(done)
}

fn emit_validate_function_body_spans(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    failure: Label,
) -> Result<(), AotV2Error> {
    assembler.mov_imm64(Register::Rbx, 0)?;
    let function_loop = assembler.new_label()?;
    let body_loop = assembler.new_label()?;
    let next_function = assembler.new_label()?;
    let nesting_done = assembler.new_label()?;
    assembler.bind(function_loop)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::FUNCTION_LINKS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbx, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, nesting_done)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::FUNCTION_LINKS)?,
        Register::Rbx,
        wire::function_link::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::function_link::FIRST_BODY_SPAN,
        Register::Rbp,
    )?;
    assembler.mov_imm64(Register::Rax, wire::NONE_REFERENCE)?;
    emit_cmp64_reg_reg(assembler, Register::Rbp, Register::Rax)?;
    assembler.far_jcc(Condition::Equal, next_function)?;
    assembler.bind(body_loop)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::FUNCTION_LINKS)?,
        Register::Rbx,
        wire::function_link::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::function_link::FIRST_BODY_SPAN,
        Register::R8,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::function_link::BODY_SPAN_COUNT,
        Register::R9,
    )?;
    assembler.add_reg64(Register::R9, Register::R8)?;
    emit_cmp64_reg_reg(assembler, Register::Rbp, Register::R9)?;
    assembler.far_jcc(Condition::AboveEqual, next_function)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::function_link::SOURCE_SPAN,
        Register::R12,
    )?;
    emit_source_span_record_address(assembler, plan, Register::R12, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::source_span::FILE_NAME,
        Register::R8,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::source_span::START_BYTE,
        Register::R9,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::source_span::END_BYTE,
        Register::R12,
    )?;
    emit_source_span_record_address(assembler, plan, Register::Rbp, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::source_span::FILE_NAME,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::R8)?;
    assembler.far_jcc(Condition::NotEqual, failure)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::source_span::START_BYTE,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::R9)?;
    assembler.far_jcc(Condition::Below, failure)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::source_span::END_BYTE,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::R12)?;
    assembler.far_jcc(Condition::Above, failure)?;
    emit_increment64(assembler, Register::Rbp)?;
    assembler.far_jump(body_loop)?;
    assembler.bind(next_function)?;
    emit_increment64(assembler, Register::Rbx)?;
    assembler.far_jump(function_loop)?;
    assembler.bind(nesting_done)?;

    assembler.mov_imm64(Register::Rbx, 0)?;
    let overlap_outer = assembler.new_label()?;
    let overlap_inner = assembler.new_label()?;
    let overlap_next_pair = assembler.new_label()?;
    let overlap_next_function = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(overlap_outer)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::FUNCTION_LINKS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbx, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, done)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::FUNCTION_LINKS)?,
        Register::Rbx,
        wire::function_link::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::function_link::FIRST_BODY_SPAN,
        Register::R8,
    )?;
    assembler.mov_imm64(Register::Rax, wire::NONE_REFERENCE)?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::Equal, overlap_next_function)?;
    assembler.mov_imm64(Register::Rbp, 0)?;
    assembler.bind(overlap_inner)?;
    emit_cmp64_reg_reg(assembler, Register::Rbp, Register::Rbx)?;
    assembler.far_jcc(Condition::AboveEqual, overlap_next_function)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::FUNCTION_LINKS)?,
        Register::Rbx,
        wire::function_link::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::function_link::FIRST_BODY_SPAN,
        Register::R8,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::function_link::BODY_SPAN_COUNT,
        Register::R9,
    )?;
    assembler.add_reg64(Register::R9, Register::R8)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::FUNCTION_LINKS)?,
        Register::Rbp,
        wire::function_link::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::function_link::FIRST_BODY_SPAN,
        Register::R12,
    )?;
    assembler.mov_imm64(Register::Rax, wire::NONE_REFERENCE)?;
    emit_cmp64_reg_reg(assembler, Register::R12, Register::Rax)?;
    assembler.far_jcc(Condition::Equal, overlap_next_pair)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::function_link::BODY_SPAN_COUNT,
        Register::Rax,
    )?;
    assembler.add_reg64(Register::Rax, Register::R12)?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, overlap_next_pair)?;
    emit_cmp64_reg_reg(assembler, Register::R12, Register::R9)?;
    assembler.far_jcc(Condition::AboveEqual, overlap_next_pair)?;
    assembler.far_jump(failure)?;
    assembler.bind(overlap_next_pair)?;
    emit_increment64(assembler, Register::Rbp)?;
    assembler.far_jump(overlap_inner)?;
    assembler.bind(overlap_next_function)?;
    emit_increment64(assembler, Register::Rbx)?;
    assembler.far_jump(overlap_outer)?;
    assembler.bind(done)
}

fn emit_validate_function_ranges(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    failure: Label,
) -> Result<(), AotV2Error> {
    assembler.data_address(Register::R11, plan.runtime_header_offset + 16)?;
    emit_mov64_reg_mem(assembler, Register::Rbx, Register::R11)?;
    assembler.data_address(Register::R11, plan.runtime_header_offset + 24)?;
    emit_mov64_reg_mem(assembler, Register::Rbp, Register::R11)?;
    assembler.add_reg64(Register::Rbp, Register::Rbx)?;
    assembler.far_jcc(Condition::Below, failure)?;
    assembler.mov_reg64(Register::R12, Register::Rbx)?;
    for index in 0..plan.link_manifest.function_count {
        emit_section_record_constant(
            assembler,
            plan,
            section_index(wire::section_kind::FUNCTION_LINKS)?,
            index,
            wire::function_link::RECORD_SIZE,
            Register::R10,
        )?;
        emit_load_record_field(
            assembler,
            Register::R10,
            wire::function_link::CODE_OFFSET,
            Register::R8,
        )?;
        emit_load_record_field(
            assembler,
            Register::R10,
            wire::function_link::CODE_BYTE_LENGTH,
            Register::R9,
        )?;
        emit_cmp64_reg_reg(assembler, Register::R8, Register::R12)?;
        assembler.far_jcc(Condition::Below, failure)?;
        emit_test64_reg_reg(assembler, Register::R9, Register::R9)?;
        assembler.far_jcc(Condition::Zero, failure)?;
        assembler.mov_reg64(Register::Rax, Register::R8)?;
        assembler.add_reg64(Register::Rax, Register::R9)?;
        assembler.far_jcc(Condition::Below, failure)?;
        emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rbp)?;
        assembler.far_jcc(Condition::Above, failure)?;
        assembler.mov_reg64(Register::R12, Register::Rax)?;
    }
    Ok(())
}

fn emit_validate_trap_links(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    failure: Label,
) -> Result<(), AotV2Error> {
    for (index, descriptor) in plan.trap_descriptors.iter().enumerate() {
        let row_offset = plan
            .trap_rows_offset
            .checked_add(
                as_u64(index, "runtime trap index")?
                    .checked_mul(RUNTIME_TRAP_ROW_BYTE_LEN)
                    .ok_or(AotV2Error::ArithmeticOverflow("runtime trap row"))?,
            )
            .ok_or(AotV2Error::ArithmeticOverflow("runtime trap row"))?;
        assembler.data_address(Register::R11, row_offset)?;
        emit_mov64_reg_mem(assembler, Register::R8, Register::R11)?;
        emit_section_record_count(
            assembler,
            plan,
            section_index(wire::section_kind::SOURCE_SPANS)?,
            Register::Rax,
        )?;
        emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
        assembler.far_jcc(Condition::AboveEqual, failure)?;
        emit_source_span_record_address(assembler, plan, Register::R8, Register::R10)?;
        for (field, expected) in [
            (wire::source_span::START_BYTE, descriptor.span.start.byte),
            (wire::source_span::END_BYTE, descriptor.span.end.byte),
            (wire::source_span::START_LINE, descriptor.span.start.line),
            (wire::source_span::END_LINE, descriptor.span.end.line),
            (
                wire::source_span::START_COLUMN,
                descriptor.span.start.column,
            ),
            (wire::source_span::END_COLUMN, descriptor.span.end.column),
        ] {
            emit_require_record_u64(assembler, Register::R10, field, expected, failure)?;
        }

        assembler.data_address(
            Register::R11,
            row_offset
                .checked_add(8)
                .ok_or(AotV2Error::ArithmeticOverflow("runtime trap function"))?,
        )?;
        emit_mov64_reg_mem(assembler, Register::R9, Register::R11)?;
        emit_section_record_count(
            assembler,
            plan,
            section_index(wire::section_kind::FUNCTION_LINKS)?,
            Register::Rax,
        )?;
        emit_cmp64_reg_reg(assembler, Register::R9, Register::Rax)?;
        assembler.far_jcc(Condition::AboveEqual, failure)?;
        emit_section_record_address(
            assembler,
            plan,
            section_index(wire::section_kind::FUNCTION_LINKS)?,
            Register::R9,
            wire::function_link::RECORD_SIZE,
            Register::R10,
        )?;
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::function_link::KIND,
            descriptor.function_kind,
            failure,
        )?;
        emit_require_record_u64(
            assembler,
            Register::R10,
            wire::function_link::SYSTEM,
            descriptor.function_system,
            failure,
        )?;
        emit_load_record_field(
            assembler,
            Register::R10,
            wire::function_link::FIRST_BODY_SPAN,
            Register::Rax,
        )?;
        assembler.mov_imm64(Register::Rcx, wire::NONE_REFERENCE)?;
        emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
        assembler.far_jcc(Condition::Equal, failure)?;
        emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
        assembler.far_jcc(Condition::Below, failure)?;
        emit_load_record_field(
            assembler,
            Register::R10,
            wire::function_link::BODY_SPAN_COUNT,
            Register::Rcx,
        )?;
        emit_test64_reg_reg(assembler, Register::Rcx, Register::Rcx)?;
        assembler.far_jcc(Condition::Zero, failure)?;
        assembler.add_reg64(Register::Rax, Register::Rcx)?;
        assembler.far_jcc(Condition::Below, failure)?;
        emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
        assembler.far_jcc(Condition::AboveEqual, failure)?;
    }
    Ok(())
}

fn emit_zero_record_tail(
    assembler: &mut Assembler,
    record: Register,
    start: u64,
    end: u64,
    failure: Label,
) -> Result<(), AotV2Error> {
    let mut offset = start;
    while offset < end {
        emit_require_record_u64(assembler, record, offset, 0, failure)?;
        offset = offset
            .checked_add(8)
            .ok_or(AotV2Error::ArithmeticOverflow("reserved record bytes"))?;
    }
    Ok(())
}

fn section_index(kind: u64) -> Result<usize, AotV2Error> {
    let index = kind
        .checked_sub(1)
        .ok_or_else(|| invalid_native("section kind zero has no canonical index"))?;
    let index = as_usize(index, "section index")?;
    if SECTION_KINDS.get(index).copied() != Some(kind) {
        return Err(invalid_native("section kind is not canonical"));
    }
    Ok(index)
}

fn emit_validate_startup_flow(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    failure: Label,
) -> Result<(), AotV2Error> {
    emit_validate_payloads(assembler, plan, failure)?;
    emit_validate_schedule_items(assembler, plan, failure)?;

    assembler.data_address(Register::Rdi, plan.validation_resource_bits_offset)?;
    assembler.mov_imm64(Register::Rcx, plan.validation_resource_bits_byte_len)?;
    assembler.emit(&[0x31, 0xc0, 0xf3, 0xaa])?; // xor eax,eax; rep stosb

    assembler.mov_imm64(Register::Rbx, 0)?;
    let loop_label = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(loop_label)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::STARTUP_OPERATIONS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbx, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, done)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::STARTUP_OPERATIONS)?,
        Register::Rbx,
        wire::startup_operation::RECORD_SIZE,
        Register::R10,
    )?;
    emit_zero_record_tail(
        assembler,
        Register::R10,
        wire::startup_operation::RESERVED,
        wire::startup_operation::RECORD_SIZE,
        failure,
    )?;
    emit_require_record_u64(
        assembler,
        Register::R10,
        wire::startup_operation::RESERVED_ARGUMENT,
        0,
        failure,
    )?;
    assembler.mov_imm64(Register::R11, wire::startup_operation::KIND)?;
    assembler.add_reg64(Register::R11, Register::R10)?;
    emit_mov64_reg_mem(assembler, Register::R8, Register::R11)?;
    let resource = assembler.new_label()?;
    let spawn = assembler.new_label()?;
    let schedule = assembler.new_label()?;
    let next = assembler.new_label()?;
    for (kind, target) in [
        (wire::startup_operation::RESOURCE_PAYLOAD, resource),
        (wire::startup_operation::SPAWN, spawn),
        (wire::startup_operation::RUN_SCHEDULE, schedule),
    ] {
        assembler.mov_imm64(Register::Rax, kind)?;
        emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
        assembler.far_jcc(Condition::Equal, target)?;
    }
    assembler.far_jump(failure)?;

    assembler.bind(resource)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::STARTUP_OPERATIONS)?,
        Register::Rbx,
        wire::startup_operation::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::startup_operation::FIRST,
        Register::R8,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::startup_operation::SECOND,
        Register::R9,
    )?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEMAS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, failure)?;
    emit_payload_count(assembler, plan, Register::Rax)?;
    emit_cmp64_reg_reg(assembler, Register::R9, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, failure)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEMAS)?,
        Register::R8,
        wire::schema::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(assembler, Register::R10, wire::schema::KIND, Register::Rax)?;
    assembler.mov_imm64(Register::Rcx, wire::schema::RESOURCE)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, failure)?;
    emit_payload_record_address(assembler, plan, Register::R9, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::payload::SCHEMA,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::R8)?;
    assembler.far_jcc(Condition::NotEqual, failure)?;
    emit_set_resource_bit(assembler, plan, Register::R8, true, failure)?;
    assembler.far_jump(next)?;

    assembler.bind(spawn)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::STARTUP_OPERATIONS)?,
        Register::Rbx,
        wire::startup_operation::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::startup_operation::FIRST,
        Register::R8,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::startup_operation::SECOND,
        Register::R9,
    )?;
    emit_validate_spawn_payload_range(assembler, plan, failure)?;
    assembler.far_jump(next)?;

    assembler.bind(schedule)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::STARTUP_OPERATIONS)?,
        Register::Rbx,
        wire::startup_operation::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::startup_operation::FIRST,
        Register::R8,
    )?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEDULES)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, failure)?;
    emit_require_schedule_resources(assembler, plan, Register::R8, failure)?;
    assembler.bind(next)?;
    emit_increment64(assembler, Register::Rbx)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(done)?;
    emit_validate_payload_usage(assembler, plan, failure)
}

fn emit_validate_payloads(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    failure: Label,
) -> Result<(), AotV2Error> {
    emit_section_base(
        assembler,
        plan,
        section_index(wire::section_kind::PAYLOADS)?,
        Register::R10,
        Register::R11,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::payload::COUNT,
        Register::Rbx,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::payload::BYTE_LENGTH,
        Register::Rbp,
    )?;
    assembler.mov_reg64(Register::Rax, Register::Rbx)?;
    assembler.mov_imm64(Register::Rcx, wire::payload::RECORD_SIZE)?;
    assembler.emit(&[0x48, 0xf7, 0xe1])?; // mul rcx
    emit_test64_reg_reg(assembler, Register::Rdx, Register::Rdx)?;
    assembler.far_jcc(Condition::NotZero, failure)?;
    assembler.mov_imm64(Register::Rcx, wire::payload::HEADER_SIZE)?;
    assembler.add_reg64(Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::Below, failure)?;
    assembler.add_reg64(Register::Rax, Register::Rbp)?;
    assembler.far_jcc(Condition::Below, failure)?;
    let row_offset = plan
        .section_rows_offset
        .checked_add(
            as_u64(
                section_index(wire::section_kind::PAYLOADS)?,
                "payload section index",
            )?
            .checked_mul(RUNTIME_SECTION_ROW_BYTE_LEN)
            .and_then(|offset| offset.checked_add(8))
            .ok_or(AotV2Error::ArithmeticOverflow("payload section row"))?,
        )
        .ok_or(AotV2Error::ArithmeticOverflow("payload section row"))?;
    assembler.data_address(Register::R11, row_offset)?;
    emit_mov64_reg_mem(assembler, Register::Rcx, Register::R11)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, failure)?;

    assembler.mov_imm64(Register::R12, 0)?;
    assembler.mov_imm64(Register::R9, 0)?;
    let loop_label = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(loop_label)?;
    emit_cmp64_reg_reg(assembler, Register::R12, Register::Rbx)?;
    assembler.far_jcc(Condition::Equal, done)?;
    emit_payload_record_address(assembler, plan, Register::R12, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::payload::SCHEMA,
        Register::R8,
    )?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEMAS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, failure)?;
    emit_payload_record_address(assembler, plan, Register::R12, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::payload::OFFSET,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::R9)?;
    assembler.far_jcc(Condition::NotEqual, failure)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::payload::LENGTH,
        Register::Rdx,
    )?;
    emit_require_record_u64(
        assembler,
        Register::R10,
        wire::payload::RESERVED,
        0,
        failure,
    )?;

    emit_storage_row_address(assembler, plan, Register::R8, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_BYTE_SIZE,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rdx, Register::Rax)?;
    assembler.far_jcc(Condition::NotEqual, failure)?;
    emit_payload_data_pointer(assembler, plan, Register::R9, Register::R10)?;
    assembler.mov_reg64(Register::Rdi, Register::R10)?;
    assembler.mov_imm64(Register::Rsi, 0)?;
    let field_loop = assembler.new_label()?;
    let next_field = assembler.new_label()?;
    let fields_done = assembler.new_label()?;
    assembler.bind(field_loop)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::FIELDS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rsi, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, fields_done)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::FIELDS)?,
        Register::Rsi,
        wire::field::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(assembler, Register::R10, wire::field::SCHEMA, Register::Rax)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::R8)?;
    assembler.far_jcc(Condition::NotEqual, next_field)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::field::PRIMITIVE,
        Register::Rax,
    )?;
    assembler.mov_imm64(Register::Rcx, u64::from(PrimitiveType::Bool as u8))?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, next_field)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::field::BYTE_OFFSET,
        Register::Rax,
    )?;
    assembler.mov_reg64(Register::R11, Register::Rdi)?;
    assembler.add_reg64(Register::R11, Register::Rax)?;
    emit_mov8_reg_mem(assembler, Register::Rax, Register::R11)?;
    assembler.mov_imm64(Register::Rcx, 1)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::Above, failure)?;
    assembler.bind(next_field)?;
    emit_increment64(assembler, Register::Rsi)?;
    assembler.far_jump(field_loop)?;
    assembler.bind(fields_done)?;
    assembler.mov_reg64(Register::Rax, Register::R9)?;
    assembler.add_reg64(Register::Rax, Register::Rdx)?;
    assembler.far_jcc(Condition::Below, failure)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rbp)?;
    assembler.far_jcc(Condition::Above, failure)?;
    assembler.mov_reg64(Register::R9, Register::Rax)?;
    emit_increment64(assembler, Register::R12)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(done)
}

fn emit_validate_schedule_items(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    failure: Label,
) -> Result<(), AotV2Error> {
    assembler.mov_imm64(Register::Rbp, 0)?;
    let loop_label = assembler.new_label()?;
    let next = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(loop_label)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEDULE_ITEMS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbp, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, done)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEDULE_ITEMS)?,
        Register::Rbp,
        wire::schedule_item::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::schedule_item::SCHEDULE,
        Register::R8,
    )?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEDULES)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, failure)?;
    emit_require_record_u64(
        assembler,
        Register::R10,
        wire::schedule_item::KIND,
        wire::schedule_item::RUN_SYSTEM,
        failure,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::schedule_item::TARGET,
        Register::R9,
    )?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SYSTEMS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R9, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, failure)?;
    emit_zero_record_tail(
        assembler,
        Register::R10,
        wire::schedule_item::RESERVED,
        wire::schedule_item::RECORD_SIZE,
        failure,
    )?;
    emit_test64_reg_reg(assembler, Register::Rbp, Register::Rbp)?;
    assembler.far_jcc(Condition::Zero, next)?;
    assembler.mov_reg64(Register::R12, Register::Rbp)?;
    emit_decrement64(assembler, Register::R12)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEDULE_ITEMS)?,
        Register::R12,
        wire::schedule_item::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::schedule_item::SCHEDULE,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::R8)?;
    assembler.far_jcc(Condition::Above, failure)?;
    assembler.bind(next)?;
    emit_increment64(assembler, Register::Rbp)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(done)
}

fn emit_validate_spawn_payload_range(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    failure: Label,
) -> Result<(), AotV2Error> {
    let done = assembler.new_label()?;
    let loop_label = assembler.new_label()?;
    let first_done = assembler.new_label()?;
    assembler.mov_reg64(Register::Rax, Register::R8)?;
    assembler.add_reg64(Register::Rax, Register::R9)?;
    assembler.far_jcc(Condition::Below, failure)?; // unsigned carry
    emit_payload_count(assembler, plan, Register::Rcx)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::Above, failure)?;
    emit_test64_reg_reg(assembler, Register::R9, Register::R9)?;
    assembler.far_jcc(Condition::Zero, done)?;
    emit_payload_record_address(assembler, plan, Register::R8, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::payload::SCHEMA,
        Register::R12,
    )?;
    emit_require_spawn_schema(assembler, plan, Register::R12, failure)?;
    emit_increment64(assembler, Register::R8)?;
    emit_decrement64(assembler, Register::R9)?;
    assembler.far_jcc(Condition::Zero, done)?;
    assembler.bind(loop_label)?;
    emit_payload_record_address(assembler, plan, Register::R8, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::payload::SCHEMA,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R12, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, failure)?;
    assembler.mov_reg64(Register::R12, Register::Rax)?;
    emit_require_spawn_schema(assembler, plan, Register::R12, failure)?;
    assembler.bind(first_done)?;
    emit_increment64(assembler, Register::R8)?;
    emit_decrement64(assembler, Register::R9)?;
    assembler.far_jcc(Condition::NotZero, loop_label)?;
    assembler.bind(done)
}

fn emit_require_spawn_schema(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    schema: Register,
    failure: Label,
) -> Result<(), AotV2Error> {
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEMAS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, schema, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, failure)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEMAS)?,
        schema,
        wire::schema::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(assembler, Register::R10, wire::schema::KIND, Register::Rax)?;
    assembler.mov_imm64(Register::Rcx, wire::schema::RESOURCE)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::Equal, failure)
}

fn emit_require_schedule_resources(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    schedule: Register,
    failure: Label,
) -> Result<(), AotV2Error> {
    if schedule != Register::R8 {
        assembler.mov_reg64(Register::R8, schedule)?;
    }
    assembler.mov_imm64(Register::Rdi, 0)?;
    let loop_label = assembler.new_label()?;
    let next = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(loop_label)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEDULE_ITEMS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rdi, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, done)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEDULE_ITEMS)?,
        Register::Rdi,
        wire::schedule_item::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::schedule_item::SCHEDULE,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::R8)?;
    assembler.far_jcc(Condition::NotEqual, next)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::schedule_item::TARGET,
        Register::R9,
    )?;
    emit_require_system_resources(assembler, plan, Register::R9, failure)?;
    assembler.bind(next)?;
    emit_increment64(assembler, Register::Rdi)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(done)
}

fn emit_require_system_resources(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    system: Register,
    failure: Label,
) -> Result<(), AotV2Error> {
    if system != Register::R9 {
        assembler.mov_reg64(Register::R9, system)?;
    }
    assembler.mov_imm64(Register::Rbp, 0)?;
    let loop_label = assembler.new_label()?;
    let next = assembler.new_label()?;
    let resource = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(loop_label)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::PARAMETERS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbp, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, done)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::PARAMETERS)?,
        Register::Rbp,
        wire::parameter::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::parameter::SYSTEM,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::R9)?;
    assembler.far_jcc(Condition::NotEqual, next)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::parameter::KIND,
        Register::Rax,
    )?;
    for kind in [
        wire::parameter::READ_RESOURCE,
        wire::parameter::MUT_RESOURCE,
    ] {
        assembler.mov_imm64(Register::Rcx, kind)?;
        emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
        assembler.far_jcc(Condition::Equal, resource)?;
    }
    assembler.far_jump(next)?;
    assembler.bind(resource)?;
    // The taken far conditional transfer uses r10/r11 as transfer scratch.
    // Reconstruct the parameter record before reading its resource target.
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::PARAMETERS)?,
        Register::Rbp,
        wire::parameter::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::parameter::TARGET,
        Register::R12,
    )?;
    emit_test_resource_bit(assembler, plan, Register::R12, failure)?;
    assembler.bind(next)?;
    emit_increment64(assembler, Register::Rbp)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(done)
}

fn emit_set_resource_bit(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    schema: Register,
    reject_existing: bool,
    failure: Label,
) -> Result<(), AotV2Error> {
    assembler.data_address(Register::R11, plan.validation_resource_bits_offset)?;
    assembler.mov_reg64(Register::Rax, schema)?;
    assembler.mov_reg64(Register::Rcx, Register::Rax)?;
    emit_shift_right64_imm8(assembler, Register::Rcx, 3)?;
    assembler.add_reg64(Register::R11, Register::Rcx)?;
    emit_and64_imm8(assembler, Register::Rax, 7)?;
    assembler.mov_reg64(Register::Rcx, Register::Rax)?;
    assembler.mov_imm64(Register::Rdx, 1)?;
    emit_shift_left64_cl(assembler, Register::Rdx)?;
    emit_mov8_reg_mem(assembler, Register::Rax, Register::R11)?;
    emit_test64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    if reject_existing {
        assembler.far_jcc(Condition::NotZero, failure)?;
    }
    emit_or64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    emit_mov8_mem_reg(assembler, Register::R11, Register::Rax)
}

fn emit_test_resource_bit(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    schema: Register,
    failure: Label,
) -> Result<(), AotV2Error> {
    assembler.data_address(Register::R11, plan.validation_resource_bits_offset)?;
    assembler.mov_reg64(Register::Rax, schema)?;
    assembler.mov_reg64(Register::Rcx, Register::Rax)?;
    emit_shift_right64_imm8(assembler, Register::Rcx, 3)?;
    assembler.add_reg64(Register::R11, Register::Rcx)?;
    emit_and64_imm8(assembler, Register::Rax, 7)?;
    assembler.mov_reg64(Register::Rcx, Register::Rax)?;
    assembler.mov_imm64(Register::Rdx, 1)?;
    emit_shift_left64_cl(assembler, Register::Rdx)?;
    emit_mov8_reg_mem(assembler, Register::Rax, Register::R11)?;
    emit_test64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    assembler.far_jcc(Condition::Zero, failure)
}

fn emit_validate_payload_usage(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    failure: Label,
) -> Result<(), AotV2Error> {
    assembler.mov_imm64(Register::Rbx, 0)?;
    let payload_loop = assembler.new_label()?;
    let operation_loop = assembler.new_label()?;
    let resource = assembler.new_label()?;
    let spawn = assembler.new_label()?;
    let reference = assembler.new_label()?;
    let next_operation = assembler.new_label()?;
    let operations_done = assembler.new_label()?;
    let next_payload = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(payload_loop)?;
    emit_payload_count(assembler, plan, Register::Rax)?;
    emit_cmp64_reg_reg(assembler, Register::Rbx, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, done)?;
    assembler.mov_imm64(Register::R12, 0)?;
    assembler.mov_imm64(Register::Rbp, 0)?;
    assembler.bind(operation_loop)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::STARTUP_OPERATIONS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbp, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, operations_done)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::STARTUP_OPERATIONS)?,
        Register::Rbp,
        wire::startup_operation::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::startup_operation::KIND,
        Register::R8,
    )?;
    assembler.mov_imm64(Register::Rax, wire::startup_operation::RESOURCE_PAYLOAD)?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::Equal, resource)?;
    assembler.mov_imm64(Register::Rax, wire::startup_operation::SPAWN)?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::Equal, spawn)?;
    assembler.far_jump(next_operation)?;

    assembler.bind(resource)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::STARTUP_OPERATIONS)?,
        Register::Rbp,
        wire::startup_operation::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::startup_operation::SECOND,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rbx)?;
    assembler.far_jcc(Condition::Equal, reference)?;
    assembler.far_jump(next_operation)?;

    assembler.bind(spawn)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::STARTUP_OPERATIONS)?,
        Register::Rbp,
        wire::startup_operation::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::startup_operation::FIRST,
        Register::R8,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbx, Register::R8)?;
    assembler.far_jcc(Condition::Below, next_operation)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::startup_operation::SECOND,
        Register::R9,
    )?;
    assembler.mov_reg64(Register::Rax, Register::R8)?;
    assembler.add_reg64(Register::Rax, Register::R9)?;
    assembler.far_jcc(Condition::Below, failure)?;
    emit_cmp64_reg_reg(assembler, Register::Rbx, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, next_operation)?;

    assembler.bind(reference)?;
    emit_increment64(assembler, Register::R12)?;
    assembler.mov_imm64(Register::Rax, 1)?;
    emit_cmp64_reg_reg(assembler, Register::R12, Register::Rax)?;
    assembler.far_jcc(Condition::Above, failure)?;
    assembler.bind(next_operation)?;
    emit_increment64(assembler, Register::Rbp)?;
    assembler.far_jump(operation_loop)?;
    assembler.bind(operations_done)?;
    assembler.mov_imm64(Register::Rax, 1)?;
    emit_cmp64_reg_reg(assembler, Register::R12, Register::Rax)?;
    assembler.far_jcc(Condition::Equal, next_payload)?;
    assembler.far_jump(failure)?;
    assembler.bind(next_payload)?;
    emit_increment64(assembler, Register::Rbx)?;
    assembler.far_jump(payload_loop)?;
    assembler.bind(done)
}

fn emit_load_record_field(
    assembler: &mut Assembler,
    record: Register,
    offset: u64,
    destination: Register,
) -> Result<(), AotV2Error> {
    let scratch = if record == Register::R11 {
        Register::R10
    } else {
        Register::R11
    };
    assembler.mov_imm64(scratch, offset)?;
    assembler.add_reg64(scratch, record)?;
    emit_mov64_reg_mem(assembler, destination, scratch)
}

fn emit_require_index_below(
    assembler: &mut Assembler,
    index: Register,
    count: u64,
    failure: Label,
) -> Result<(), AotV2Error> {
    assembler.mov_imm64(Register::Rcx, count)?;
    emit_cmp64_reg_reg(assembler, index, Register::Rcx)?;
    assembler.far_jcc(Condition::AboveEqual, failure)
}

fn emit_payload_record_address(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    index: Register,
    destination: Register,
) -> Result<(), AotV2Error> {
    emit_section_base(
        assembler,
        plan,
        section_index(wire::section_kind::PAYLOADS)?,
        destination,
        Register::R11,
    )?;
    assembler.mov_reg64(Register::Rax, index)?;
    assembler.mov_imm64(Register::Rcx, wire::payload::RECORD_SIZE)?;
    emit_imul64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.mov_imm64(Register::Rcx, wire::payload::HEADER_SIZE)?;
    assembler.add_reg64(Register::Rax, Register::Rcx)?;
    assembler.add_reg64(destination, Register::Rax)
}

fn emit_payload_count(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    destination: Register,
) -> Result<(), AotV2Error> {
    emit_section_base(
        assembler,
        plan,
        section_index(wire::section_kind::PAYLOADS)?,
        Register::R10,
        Register::R11,
    )?;
    emit_load_record_field(assembler, Register::R10, wire::payload::COUNT, destination)
}

fn emit_payload_data_pointer(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    payload_offset: Register,
    destination: Register,
) -> Result<(), AotV2Error> {
    if payload_offset == Register::Rcx || destination == payload_offset {
        return Err(invalid_native(
            "payload data pointer received an unsupported register assignment",
        ));
    }
    emit_section_base(
        assembler,
        plan,
        section_index(wire::section_kind::PAYLOADS)?,
        destination,
        Register::R11,
    )?;
    emit_load_record_field(assembler, destination, wire::payload::COUNT, Register::Rcx)?;
    assembler.add_reg64(destination, payload_offset)?;
    assembler.mov_reg64(Register::R11, Register::Rcx)?;
    assembler.mov_imm64(Register::Rax, wire::payload::RECORD_SIZE)?;
    emit_imul64_reg_reg(assembler, Register::R11, Register::Rax)?;
    assembler.mov_imm64(Register::Rax, wire::payload::HEADER_SIZE)?;
    assembler.add_reg64(Register::R11, Register::Rax)?;
    assembler.add_reg64(destination, Register::R11)
}

fn emit_execute_next_startup_operation(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    entry: Label,
    labels: &RuntimeInternalLabels,
) -> Result<(), AotV2Error> {
    assembler.bind(entry)?;
    emit_runtime_prologue(assembler)?;
    assembler.data_address(Register::R11, plan.validated_offset)?;
    emit_mov64_reg_mem(assembler, Register::Rax, Register::R11)?;
    assembler.mov_imm64(Register::Rcx, 1)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, labels.failure)?;
    assembler.data_address(Register::R11, plan.startup_cursor_offset)?;
    emit_mov64_reg_mem(assembler, Register::Rbx, Register::R11)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::STARTUP_OPERATIONS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbx, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, labels.failure)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::STARTUP_OPERATIONS)?,
        Register::Rbx,
        wire::startup_operation::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::startup_operation::KIND,
        Register::R8,
    )?;
    let resource = assembler.new_label()?;
    let spawn = assembler.new_label()?;
    let schedule = assembler.new_label()?;
    let success = assembler.new_label()?;
    for (kind, target) in [
        (wire::startup_operation::RESOURCE_PAYLOAD, resource),
        (wire::startup_operation::SPAWN, spawn),
        (wire::startup_operation::RUN_SCHEDULE, schedule),
    ] {
        assembler.mov_imm64(Register::Rax, kind)?;
        emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
        assembler.far_jcc(Condition::Equal, target)?;
    }
    assembler.far_jump(labels.failure)?;

    assembler.bind(resource)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::STARTUP_OPERATIONS)?,
        Register::Rbx,
        wire::startup_operation::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::startup_operation::FIRST,
        Register::R8,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::startup_operation::SECOND,
        Register::R9,
    )?;
    emit_resource_initialization(assembler, plan, labels, Register::R8, Register::R9)?;
    assembler.far_jump(success)?;

    assembler.bind(spawn)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::STARTUP_OPERATIONS)?,
        Register::Rbx,
        wire::startup_operation::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::startup_operation::FIRST,
        Register::R8,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::startup_operation::SECOND,
        Register::R9,
    )?;
    emit_spawn(assembler, plan, labels, Register::R8, Register::R9)?;
    assembler.far_jump(success)?;

    assembler.bind(schedule)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::STARTUP_OPERATIONS)?,
        Register::Rbx,
        wire::startup_operation::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::startup_operation::FIRST,
        Register::R8,
    )?;
    emit_schedule_dispatch(assembler, plan, labels, Register::R8)?;

    assembler.bind(success)?;
    assembler.data_address(Register::R11, plan.startup_cursor_offset)?;
    emit_mov64_reg_mem(assembler, Register::Rbx, Register::R11)?;
    emit_increment64(assembler, Register::Rbx)?;
    emit_mov64_mem_reg(assembler, Register::R11, Register::Rbx)?;
    emit_runtime_epilogue(assembler)
}

fn emit_resource_initialization(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    labels: &RuntimeInternalLabels,
    schema: Register,
    payload: Register,
) -> Result<(), AotV2Error> {
    emit_payload_record_address(assembler, plan, payload, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::payload::OFFSET,
        Register::Rax,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::payload::LENGTH,
        Register::Rdx,
    )?;
    emit_payload_data_pointer(assembler, plan, Register::Rax, Register::Rsi)?;
    emit_storage_row_address(assembler, plan, schema, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_KIND,
        Register::Rax,
    )?;
    assembler.mov_imm64(Register::Rcx, wire::schema::RESOURCE)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, labels.failure)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_RESOURCE_PAYLOAD,
        Register::Rdi,
    )?;
    assembler.mov_imm64(Register::Rax, u64::MAX)?;
    emit_cmp64_reg_reg(assembler, Register::Rdi, Register::Rax)?;
    assembler.far_jcc(Condition::Equal, labels.failure)?;
    emit_data_offset_address(assembler, Register::Rdi, Register::Rdi)?;
    assembler.mov_reg64(Register::Rcx, Register::Rdx)?;
    assembler.emit(&[0xfc, 0xf3, 0xa4])?; // cld; rep movsb
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_RESOURCE_INITIALIZED,
        Register::R11,
    )?;
    assembler.mov_imm64(Register::Rax, u64::MAX)?;
    emit_cmp64_reg_reg(assembler, Register::R11, Register::Rax)?;
    assembler.far_jcc(Condition::Equal, labels.failure)?;
    emit_data_offset_address(assembler, Register::R11, Register::R11)?;
    assembler.mov_imm64(Register::Rax, 1)?;
    emit_mov8_mem_reg(assembler, Register::R11, Register::Rax)
}

fn emit_spawn(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    labels: &RuntimeInternalLabels,
    first_payload: Register,
    payload_count: Register,
) -> Result<(), AotV2Error> {
    assembler.data_address(Register::Rdi, plan.staging_row_offset)?;
    assembler.mov_imm64(Register::Rcx, plan.world.row_stride)?;
    assembler.emit(&[0x31, 0xc0, 0xfc, 0xf3, 0xaa])?; // xor eax,eax; cld; rep stosb
    assembler.mov_reg64(Register::R8, first_payload)?;
    assembler.mov_reg64(Register::R9, payload_count)?;
    let payload_loop = assembler.new_label()?;
    let payload_done = assembler.new_label()?;
    emit_test64_reg_reg(assembler, Register::R9, Register::R9)?;
    assembler.far_jcc(Condition::Zero, payload_done)?;
    assembler.bind(payload_loop)?;
    emit_payload_record_address(assembler, plan, Register::R8, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::payload::SCHEMA,
        Register::R12,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::payload::OFFSET,
        Register::Rax,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::payload::LENGTH,
        Register::Rdx,
    )?;
    assembler.data_address(
        Register::R11,
        plan.staging_row_offset
            .checked_add(plan.world.row_membership_offset)
            .ok_or(AotV2Error::ArithmeticOverflow("staging membership"))?,
    )?;
    assembler.mov_reg64(Register::Rcx, Register::R12)?;
    emit_shift_right64_imm8(assembler, Register::Rcx, 3)?;
    assembler.add_reg64(Register::R11, Register::Rcx)?;
    emit_mov8_reg_mem(assembler, Register::Rcx, Register::R11)?;
    assembler.mov_reg64(Register::Rdi, Register::R12)?;
    emit_and64_imm8(assembler, Register::Rdi, 7)?;
    assembler.mov_reg64(Register::Rcx, Register::Rdi)?;
    assembler.mov_imm64(Register::Rdi, 1)?;
    emit_shift_left64_cl(assembler, Register::Rdi)?;
    emit_mov8_reg_mem(assembler, Register::Rcx, Register::R11)?;
    emit_or64_reg_reg(assembler, Register::Rcx, Register::Rdi)?;
    emit_mov8_mem_reg(assembler, Register::R11, Register::Rcx)?;

    emit_spawn_payload_copy(
        assembler,
        plan,
        labels,
        Register::R12,
        Register::Rax,
        Register::Rdx,
    )?;
    emit_increment64(assembler, Register::R8)?;
    emit_decrement64(assembler, Register::R9)?;
    assembler.far_jcc(Condition::NotZero, payload_loop)?;
    assembler.bind(payload_done)?;

    assembler.data_address(Register::R11, plan.world.next_spawn_ordinal_offset)?;
    emit_mov64_reg_mem(assembler, Register::R12, Register::R11)?;
    assembler.data_address(
        Register::R11,
        plan.staging_row_offset
            .checked_add(plan.world.row_spawn_ordinal_offset)
            .ok_or(AotV2Error::ArithmeticOverflow("staging spawn ordinal"))?,
    )?;
    emit_mov64_mem_reg(assembler, Register::R11, Register::R12)?;
    assembler.data_address(
        Register::R11,
        plan.staging_row_offset
            .checked_add(plan.world.row_active_offset)
            .ok_or(AotV2Error::ArithmeticOverflow("staging active flag"))?,
    )?;
    assembler.mov_imm64(Register::Rax, 1)?;
    emit_mov64_mem_reg(assembler, Register::R11, Register::Rax)?;

    assembler.data_address(Register::R11, plan.world.row_count_offset)?;
    emit_mov64_reg_mem(assembler, Register::Rbx, Register::R11)?;
    assembler.mov_imm64(Register::Rax, plan.world.max_rows)?;
    emit_cmp64_reg_reg(assembler, Register::Rbx, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, labels.failure)?;
    assembler.mov_imm64(Register::R8, 0)?; // insertion index
    let search = assembler.new_label()?;
    let insert = assembler.new_label()?;
    assembler.bind(search)?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rbx)?;
    assembler.far_jcc(Condition::Equal, insert)?;
    assembler.data_address(
        Register::Rdi,
        plan.staging_row_offset
            .checked_add(plan.world.row_membership_offset)
            .ok_or(AotV2Error::ArithmeticOverflow("staging membership"))?,
    )?;
    emit_row_address(assembler, plan, Register::R8, Register::Rsi)?;
    assembler.mov_imm64(Register::Rax, plan.world.row_membership_offset)?;
    assembler.add_reg64(Register::Rsi, Register::Rax)?;
    assembler.far_call(labels.compare_keys)?;
    emit_test64_reg_reg(assembler, Register::Rax, Register::Rax)?;
    assembler.far_jcc(Condition::Less, insert)?;
    emit_increment64(assembler, Register::R8)?;
    assembler.far_jump(search)?;
    assembler.bind(insert)?;

    assembler.mov_reg64(Register::R9, Register::Rbx)?;
    emit_sub64_reg_reg(assembler, Register::R9, Register::R8)?;
    let no_shift = assembler.new_label()?;
    emit_test64_reg_reg(assembler, Register::R9, Register::R9)?;
    assembler.far_jcc(Condition::Zero, no_shift)?;
    emit_row_address(assembler, plan, Register::Rbx, Register::Rdi)?;
    assembler.mov_imm64(Register::Rax, plan.world.row_stride)?;
    assembler.add_reg64(Register::Rdi, Register::Rax)?;
    emit_decrement64(assembler, Register::Rdi)?;
    emit_row_address(assembler, plan, Register::Rbx, Register::Rsi)?;
    emit_decrement64(assembler, Register::Rsi)?;
    assembler.mov_reg64(Register::Rax, Register::R9)?;
    assembler.mov_imm64(Register::Rcx, plan.world.row_stride)?;
    emit_imul64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.mov_reg64(Register::Rcx, Register::Rax)?; // byte count after address helpers
    assembler.emit(&[0xfd, 0xf3, 0xa4, 0xfc])?; // std; rep movsb; cld
    assembler.bind(no_shift)?;
    emit_row_address(assembler, plan, Register::R8, Register::Rdi)?;
    assembler.data_address(Register::Rsi, plan.staging_row_offset)?;
    assembler.mov_imm64(Register::Rcx, plan.world.row_stride)?;
    assembler.emit(&[0xfc, 0xf3, 0xa4])?;

    emit_increment64(assembler, Register::Rbx)?;
    assembler.data_address(Register::R11, plan.world.row_count_offset)?;
    emit_mov64_mem_reg(assembler, Register::R11, Register::Rbx)?;
    emit_increment64(assembler, Register::R12)?;
    assembler.data_address(Register::R11, plan.world.next_spawn_ordinal_offset)?;
    emit_mov64_mem_reg(assembler, Register::R11, Register::R12)
}

fn emit_spawn_payload_copy(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    labels: &RuntimeInternalLabels,
    schema: Register,
    payload_offset: Register,
    payload_len: Register,
) -> Result<(), AotV2Error> {
    assembler.mov_reg64(Register::Rbp, payload_offset)?;
    emit_storage_row_address(assembler, plan, schema, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_KIND,
        Register::Rax,
    )?;
    assembler.mov_imm64(Register::Rcx, wire::schema::RESOURCE)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::Equal, labels.failure)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_ROW_CELL,
        Register::Rax,
    )?;
    assembler.mov_imm64(Register::Rcx, u64::MAX)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::Equal, labels.failure)?;
    assembler.data_address(Register::Rdi, plan.staging_row_offset)?;
    assembler.add_reg64(Register::Rdi, Register::Rax)?;
    emit_payload_data_pointer(assembler, plan, Register::Rbp, Register::Rsi)?;
    assembler.mov_reg64(Register::Rcx, payload_len)?;
    assembler.emit(&[0xfc, 0xf3, 0xa4])
}

fn emit_schedule_dispatch(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    labels: &RuntimeInternalLabels,
    schedule: Register,
) -> Result<(), AotV2Error> {
    assembler.mov_reg64(Register::R12, schedule)?;
    assembler.mov_imm64(Register::Rbp, 0)?;
    let loop_label = assembler.new_label()?;
    let next = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(loop_label)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEDULE_ITEMS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbp, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, done)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEDULE_ITEMS)?,
        Register::Rbp,
        wire::schedule_item::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::schedule_item::SCHEDULE,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::R12)?;
    assembler.far_jcc(Condition::NotEqual, next)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::schedule_item::TARGET,
        Register::R9,
    )?;
    assembler.mov_imm64(Register::R8, 0)?;
    let function_loop = assembler.new_label()?;
    let function_next = assembler.new_label()?;
    assembler.bind(function_loop)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::FUNCTION_LINKS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, labels.failure)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::FUNCTION_LINKS)?,
        Register::R8,
        wire::function_link::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::function_link::KIND,
        Register::Rax,
    )?;
    assembler.mov_imm64(Register::Rcx, wire::function_link::SYSTEM_TARGET)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, function_next)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::function_link::SYSTEM,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::R9)?;
    assembler.far_jcc(Condition::NotEqual, function_next)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::function_link::CODE_OFFSET,
        Register::Rax,
    )?;
    assembler.add_reg64(Register::Rax, Register::R15)?;
    assembler.mov_reg64(Register::Rdi, Register::R9)?;
    assembler.emit(&[0xff, 0xd0])?; // call rax
    assembler.far_jump(next)?;
    assembler.bind(function_next)?;
    emit_increment64(assembler, Register::R8)?;
    assembler.far_jump(function_loop)?;
    assembler.bind(next)?;
    emit_increment64(assembler, Register::Rbp)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(done)?;
    Ok(())
}

fn emit_row_address(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    row: Register,
    destination: Register,
) -> Result<(), AotV2Error> {
    assembler.data_address(destination, plan.world.rows_base)?;
    assembler.mov_reg64(Register::Rax, row)?;
    assembler.mov_imm64(Register::Rcx, plan.world.row_stride)?;
    emit_imul64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.add_reg64(destination, Register::Rax)
}

fn emit_next_query_row(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    entry: Label,
    labels: &RuntimeInternalLabels,
) -> Result<(), AotV2Error> {
    assembler.bind(entry)?;
    emit_runtime_prologue(assembler)?;
    assembler.mov_reg64(Register::Rbx, Register::Rdi)?;
    assembler.mov_reg64(Register::R12, Register::Rsi)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::QUERIES)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbx, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, labels.failure)?;
    let none = assembler.new_label()?;
    let finish = assembler.new_label()?;
    let row_loop = assembler.new_label()?;
    let next_row = assembler.new_label()?;
    let term_loop = assembler.new_label()?;
    let next_term = assembler.new_label()?;
    let required_term = assembler.new_label()?;
    let excluded_term = assembler.new_label()?;
    let row_matches = assembler.new_label()?;
    assembler.bind(row_loop)?;
    assembler.data_address(Register::R11, plan.world.row_count_offset)?;
    emit_mov64_reg_mem(assembler, Register::Rax, Register::R11)?;
    emit_cmp64_reg_reg(assembler, Register::R12, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, none)?;
    emit_row_address(assembler, plan, Register::R12, Register::R10)?;
    assembler.mov_imm64(Register::R11, plan.world.row_active_offset)?;
    assembler.add_reg64(Register::R11, Register::R10)?;
    emit_mov64_reg_mem(assembler, Register::Rax, Register::R11)?;
    emit_test64_reg_reg(assembler, Register::Rax, Register::Rax)?;
    assembler.far_jcc(Condition::Zero, next_row)?;
    assembler.mov_imm64(Register::Rbp, 0)?;
    assembler.bind(term_loop)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::TERMS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbp, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, row_matches)?;
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::TERMS)?,
        Register::Rbp,
        wire::term::RECORD_SIZE,
        Register::R10,
    )?;
    emit_load_record_field(assembler, Register::R10, wire::term::QUERY, Register::Rax)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rbx)?;
    assembler.far_jcc(Condition::NotEqual, next_term)?;
    emit_load_record_field(assembler, Register::R10, wire::term::SCHEMA, Register::R8)?;
    emit_load_record_field(assembler, Register::R10, wire::term::ACCESS, Register::R9)?;
    emit_row_address(assembler, plan, Register::R12, Register::R10)?;
    assembler.mov_imm64(Register::R11, plan.world.row_membership_offset)?;
    assembler.add_reg64(Register::R11, Register::R10)?;
    assembler.mov_reg64(Register::Rax, Register::R8)?;
    emit_shift_right64_imm8(assembler, Register::Rax, 3)?;
    assembler.add_reg64(Register::R11, Register::Rax)?;
    emit_mov8_reg_mem(assembler, Register::Rax, Register::R11)?;
    assembler.mov_reg64(Register::Rcx, Register::R8)?;
    emit_and64_imm8(assembler, Register::Rcx, 7)?;
    assembler.mov_imm64(Register::Rdx, 1)?;
    emit_shift_left64_cl(assembler, Register::Rdx)?;
    emit_test64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    assembler.mov_imm64(Register::Rcx, wire::term::EXCLUDE)?;
    emit_cmp64_reg_reg(assembler, Register::R9, Register::Rcx)?;
    assembler.far_jcc(Condition::Equal, excluded_term)?;
    assembler.mov_imm64(Register::Rcx, wire::term::READ)?;
    emit_cmp64_reg_reg(assembler, Register::R9, Register::Rcx)?;
    assembler.far_jcc(Condition::Equal, required_term)?;
    assembler.mov_imm64(Register::Rcx, wire::term::MUT)?;
    emit_cmp64_reg_reg(assembler, Register::R9, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, labels.failure)?;
    assembler.bind(required_term)?;
    emit_test64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    assembler.far_jcc(Condition::Zero, next_row)?;
    assembler.far_jump(next_term)?;
    assembler.bind(excluded_term)?;
    emit_test64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    assembler.far_jcc(Condition::NotZero, next_row)?;
    assembler.bind(next_term)?;
    emit_increment64(assembler, Register::Rbp)?;
    assembler.far_jump(term_loop)?;
    assembler.bind(row_matches)?;
    assembler.mov_reg64(Register::Rax, Register::R12)?;
    assembler.far_jump(finish)?;
    assembler.bind(next_row)?;
    emit_increment64(assembler, Register::R12)?;
    assembler.far_jump(row_loop)?;
    assembler.bind(none)?;
    assembler.mov_imm64(Register::Rax, u64::MAX)?;
    assembler.bind(finish)?;
    emit_runtime_epilogue(assembler)
}

fn emit_observation_exit(
    assembler: &mut Assembler,
    _plan: &NativeRuntimePlan,
    entry: Label,
    labels: &RuntimeInternalLabels,
) -> Result<(), AotV2Error> {
    assembler.bind(entry)?;
    emit_runtime_prologue(assembler)?;
    assembler.mov_reg64(Register::Rbx, Register::Rdi)?;
    assembler.far_call(labels.observation_body)?;
    assembler.mov_reg64(Register::Rdi, Register::Rbx)?;
    assembler.emit(&[0x40, 0x0f, 0xb6, 0xff])?; // movzx edi,dil
    emit_exit_group(assembler)
}

fn emit_trap_exit(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    entry: Label,
    labels: &RuntimeInternalLabels,
) -> Result<(), AotV2Error> {
    assembler.bind(entry)?;
    emit_runtime_prologue(assembler)?;
    assembler.mov_reg64(Register::Rbx, Register::Rdi)?;
    assembler.mov_reg64(Register::R12, Register::Rsi)?;
    assembler.far_call(labels.observation_body)?;
    emit_require_index_below(
        assembler,
        Register::R12,
        as_u64(plan.trap_descriptors.len(), "runtime trap descriptor count")?,
        labels.failure,
    )?;
    assembler.data_address(Register::R11, plan.trap_rows_offset)?;
    assembler.mov_reg64(Register::Rax, Register::R12)?;
    assembler.mov_imm64(Register::Rcx, RUNTIME_TRAP_ROW_BYTE_LEN)?;
    emit_imul64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.add_reg64(Register::R11, Register::Rax)?;
    emit_mov64_reg_mem(assembler, Register::Rbp, Register::R11)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SOURCE_SPANS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::Rbp, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, labels.failure)?;
    emit_write_literal(assembler, plan, labels, 2, TRAP_PREFIX)?;
    let kind_done = assembler.new_label()?;
    for (index, name) in TRAP_NAMES.into_iter().enumerate() {
        let matched = assembler.new_label()?;
        assembler.mov_imm64(Register::Rax, as_u64(index, "runtime trap kind")?)?;
        emit_cmp64_reg_reg(assembler, Register::Rbx, Register::Rax)?;
        assembler.far_jcc(Condition::Equal, matched)?;
        let after = assembler.new_label()?;
        assembler.far_jump(after)?;
        assembler.bind(matched)?;
        emit_write_literal(assembler, plan, labels, 2, name)?;
        assembler.far_jump(kind_done)?;
        assembler.bind(after)?;
    }
    assembler.far_jump(labels.failure)?;
    assembler.bind(kind_done)?;
    emit_write_literal(assembler, plan, labels, 2, TRAP_MIDDLE)?;
    emit_source_span_record_address(assembler, plan, Register::Rbp, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        wire::source_span::FILE_NAME,
        Register::R8,
    )?;
    emit_string_pointer(assembler, plan, Register::R8, labels.failure)?;
    assembler.mov_imm64(Register::Rdi, 2)?;
    assembler.far_call(labels.write_all)?;
    emit_write_literal(assembler, plan, labels, 2, TRAP_LOCATION_SEPARATOR)?;
    emit_write_source_span_number(assembler, plan, labels, wire::source_span::START_LINE)?;
    emit_write_literal(assembler, plan, labels, 2, TRAP_LOCATION_SEPARATOR)?;
    emit_write_source_span_number(assembler, plan, labels, wire::source_span::START_COLUMN)?;
    emit_write_literal(assembler, plan, labels, 2, TRAP_BYTES_PREFIX)?;
    emit_write_source_span_number(assembler, plan, labels, wire::source_span::START_BYTE)?;
    emit_write_literal(assembler, plan, labels, 2, TRAP_RANGE_SEPARATOR)?;
    emit_write_source_span_number(assembler, plan, labels, wire::source_span::END_BYTE)?;
    emit_write_literal(assembler, plan, labels, 2, NEWLINE)?;
    assembler.mov_imm64(Register::Rdi, 70)?;
    emit_exit_group(assembler)
}

fn emit_source_span_record_address(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    index: Register,
    destination: Register,
) -> Result<(), AotV2Error> {
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::SOURCE_SPANS)?,
        index,
        wire::source_span::RECORD_SIZE,
        destination,
    )
}

fn emit_write_source_span_number(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    labels: &RuntimeInternalLabels,
    field: u64,
) -> Result<(), AotV2Error> {
    emit_source_span_record_address(assembler, plan, Register::Rbp, Register::R10)?;
    emit_load_record_field(assembler, Register::R10, field, Register::Rsi)?;
    assembler.mov_imm64(Register::Rdi, 2)?;
    assembler.far_call(labels.write_decimal)
}

fn emit_string_pointer(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    index: Register,
    failure: Label,
) -> Result<(), AotV2Error> {
    emit_section_base(
        assembler,
        plan,
        section_index(wire::section_kind::STRINGS)?,
        Register::R10,
        Register::R11,
    )?;
    emit_load_record_field(assembler, Register::R10, wire::strings::COUNT, Register::R9)?;
    emit_cmp64_reg_reg(assembler, index, Register::R9)?;
    assembler.far_jcc(Condition::AboveEqual, failure)?;
    assembler.mov_reg64(Register::R11, index)?;
    assembler.mov_imm64(Register::Rax, wire::strings::RECORD_SIZE)?;
    emit_imul64_reg_reg(assembler, Register::R11, Register::Rax)?;
    assembler.mov_imm64(Register::Rax, wire::strings::HEADER_SIZE)?;
    assembler.add_reg64(Register::R11, Register::Rax)?;
    assembler.add_reg64(Register::R11, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R11,
        wire::strings::RECORD_OFFSET,
        Register::Rax,
    )?;
    emit_load_record_field(
        assembler,
        Register::R11,
        wire::strings::RECORD_BYTE_LENGTH,
        Register::Rdx,
    )?;
    emit_section_base(
        assembler,
        plan,
        section_index(wire::section_kind::STRINGS)?,
        Register::Rsi,
        Register::R11,
    )?;
    assembler.mov_reg64(Register::Rcx, Register::R9)?;
    assembler.mov_imm64(Register::R11, wire::strings::RECORD_SIZE)?;
    emit_imul64_reg_reg(assembler, Register::Rcx, Register::R11)?;
    assembler.mov_imm64(Register::R11, wire::strings::HEADER_SIZE)?;
    assembler.add_reg64(Register::Rsi, Register::R11)?;
    assembler.add_reg64(Register::Rsi, Register::Rcx)?;
    assembler.add_reg64(Register::Rsi, Register::Rax)
}

fn emit_internal_helpers(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    labels: &RuntimeInternalLabels,
) -> Result<(), AotV2Error> {
    assembler.bind(labels.failure)?;
    assembler.mov_imm64(Register::Rdi, 1)?;
    emit_exit_group(assembler)?;

    assembler.bind(labels.version_one)?;
    emit_write_literal(assembler, plan, labels, 2, VERSION_ONE_DIAGNOSTIC)?;
    assembler.mov_imm64(Register::Rdi, 1)?;
    emit_exit_group(assembler)?;

    assembler.bind(labels.archecmp)?;
    emit_write_literal(assembler, plan, labels, 2, ARCHECMP_DIAGNOSTIC)?;
    assembler.mov_imm64(Register::Rdi, 1)?;
    emit_exit_group(assembler)?;

    emit_write_all_helper(assembler, labels)?;
    emit_write_decimal_helper(assembler, plan, labels)?;
    emit_write_hex_helper(assembler, plan, labels)?;
    emit_validate_utf8_helper(assembler, labels)?;
    emit_compare_bytes_helper(assembler, labels)?;
    emit_compare_keys_helper(assembler, plan, labels)?;

    emit_observation_body(assembler, plan, labels)
}

fn emit_observation_body(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    labels: &RuntimeInternalLabels,
) -> Result<(), AotV2Error> {
    assembler.bind(labels.observation_body)?;
    emit_runtime_prologue(assembler)?;
    emit_write_literal(assembler, plan, labels, 1, OBSERVATION_HEADER)?;

    assembler.mov_imm64(Register::R12, 0)?;
    let resource_loop = assembler.new_label()?;
    let next_resource = assembler.new_label()?;
    let resources_done = assembler.new_label()?;
    assembler.bind(resource_loop)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEMAS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R12, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, resources_done)?;
    emit_storage_row_address(assembler, plan, Register::R12, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_KIND,
        Register::Rax,
    )?;
    assembler.mov_imm64(Register::Rcx, wire::schema::RESOURCE)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::NotEqual, next_resource)?;
    emit_write_literal(assembler, plan, labels, 1, RESOURCE_PREFIX)?;
    emit_schema_id_pointer(assembler, plan, Register::R12, Register::Rsi)?;
    emit_write_hex_call(assembler, labels, Register::Rsi, 16)?;
    emit_storage_row_address(assembler, plan, Register::R12, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_RESOURCE_INITIALIZED,
        Register::R11,
    )?;
    emit_data_offset_address(assembler, Register::R11, Register::R11)?;
    emit_mov8_reg_mem(assembler, Register::Rax, Register::R11)?;
    emit_test64_reg_reg(assembler, Register::Rax, Register::Rax)?;
    let initialized = assembler.new_label()?;
    let resource_done = assembler.new_label()?;
    assembler.far_jcc(Condition::NotZero, initialized)?;
    emit_write_literal(assembler, plan, labels, 1, RESOURCE_UNINITIALIZED)?;
    assembler.far_jump(resource_done)?;
    assembler.bind(initialized)?;
    emit_write_literal(assembler, plan, labels, 1, RESOURCE_INITIALIZED)?;
    emit_storage_row_address(assembler, plan, Register::R12, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_BYTE_SIZE,
        Register::Rsi,
    )?;
    emit_write_decimal_call(assembler, labels, Register::Rsi)?;
    emit_write_literal(assembler, plan, labels, 1, SPACE)?;
    emit_storage_row_address(assembler, plan, Register::R12, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_RESOURCE_PAYLOAD,
        Register::Rsi,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_BYTE_SIZE,
        Register::Rdx,
    )?;
    emit_data_offset_address(assembler, Register::Rsi, Register::Rsi)?;
    emit_write_hex_register(assembler, labels, Register::Rsi, Register::Rdx)?;
    emit_write_literal(assembler, plan, labels, 1, NEWLINE)?;
    assembler.bind(resource_done)?;
    assembler.bind(next_resource)?;
    emit_increment64(assembler, Register::R12)?;
    assembler.far_jump(resource_loop)?;
    assembler.bind(resources_done)?;

    assembler.mov_imm64(Register::Rbx, 0)?; // canonical table start row
    let table_loop = assembler.new_label()?;
    let tables_done = assembler.new_label()?;
    assembler.bind(table_loop)?;
    assembler.data_address(Register::R11, plan.world.row_count_offset)?;
    emit_mov64_reg_mem(assembler, Register::Rax, Register::R11)?;
    emit_cmp64_reg_reg(assembler, Register::Rbx, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, tables_done)?;
    assembler.mov_reg64(Register::R8, Register::Rbx)?;
    emit_increment64(assembler, Register::R8)?;
    let group_scan = assembler.new_label()?;
    let group_ready = assembler.new_label()?;
    assembler.bind(group_scan)?;
    assembler.data_address(Register::R11, plan.world.row_count_offset)?;
    emit_mov64_reg_mem(assembler, Register::Rax, Register::R11)?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, group_ready)?;
    emit_row_address(assembler, plan, Register::Rbx, Register::Rdi)?;
    assembler.mov_imm64(Register::Rax, plan.world.row_membership_offset)?;
    assembler.add_reg64(Register::Rdi, Register::Rax)?;
    emit_row_address(assembler, plan, Register::R8, Register::Rsi)?;
    assembler.mov_imm64(Register::Rax, plan.world.row_membership_offset)?;
    assembler.add_reg64(Register::Rsi, Register::Rax)?;
    assembler.far_call(labels.compare_keys)?;
    emit_test64_reg_reg(assembler, Register::Rax, Register::Rax)?;
    assembler.far_jcc(Condition::NotZero, group_ready)?;
    emit_increment64(assembler, Register::R8)?;
    assembler.far_jump(group_scan)?;
    assembler.bind(group_ready)?;
    assembler.mov_reg64(Register::Rbp, Register::R8)?; // group end, preserved by writers

    emit_write_literal(assembler, plan, labels, 1, TABLE_PREFIX)?;
    emit_count_row_columns(assembler, plan, Register::Rbx, Register::Rsi)?;
    emit_write_decimal_call(assembler, labels, Register::Rsi)?;
    assembler.mov_imm64(Register::R12, 0)?;
    let table_schema_loop = assembler.new_label()?;
    let next_table_schema = assembler.new_label()?;
    let table_schemas_done = assembler.new_label()?;
    assembler.bind(table_schema_loop)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEMAS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R12, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, table_schemas_done)?;
    emit_storage_row_address(assembler, plan, Register::R12, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_KIND,
        Register::Rax,
    )?;
    assembler.mov_imm64(Register::Rcx, wire::schema::RESOURCE)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::Equal, next_table_schema)?;
    emit_test_row_membership(
        assembler,
        plan,
        Register::Rbx,
        Register::R12,
        Condition::Zero,
        next_table_schema,
    )?;
    emit_write_literal(assembler, plan, labels, 1, SPACE)?;
    emit_schema_id_pointer(assembler, plan, Register::R12, Register::Rsi)?;
    emit_write_hex_call(assembler, labels, Register::Rsi, 16)?;
    assembler.bind(next_table_schema)?;
    emit_increment64(assembler, Register::R12)?;
    assembler.far_jump(table_schema_loop)?;
    assembler.bind(table_schemas_done)?;
    emit_write_literal(assembler, plan, labels, 1, SPACE)?;
    assembler.mov_reg64(Register::Rsi, Register::Rbp)?;
    emit_sub64_reg_reg(assembler, Register::Rsi, Register::Rbx)?;
    emit_write_decimal_call(assembler, labels, Register::Rsi)?;
    emit_write_literal(assembler, plan, labels, 1, NEWLINE)?;

    assembler.mov_reg64(Register::R12, Register::Rbx)?;
    let row_loop = assembler.new_label()?;
    let rows_done = assembler.new_label()?;
    assembler.bind(row_loop)?;
    emit_cmp64_reg_reg(assembler, Register::R12, Register::Rbp)?;
    assembler.far_jcc(Condition::AboveEqual, rows_done)?;
    emit_write_literal(assembler, plan, labels, 1, ROW_PREFIX)?;
    assembler.mov_reg64(Register::Rsi, Register::R12)?;
    emit_sub64_reg_reg(assembler, Register::Rsi, Register::Rbx)?;
    emit_write_decimal_call(assembler, labels, Register::Rsi)?;
    emit_write_literal(assembler, plan, labels, 1, SPACE)?;
    emit_row_address(assembler, plan, Register::R12, Register::R10)?;
    assembler.mov_imm64(Register::R11, plan.world.row_spawn_ordinal_offset)?;
    assembler.add_reg64(Register::R11, Register::R10)?;
    emit_mov64_reg_mem(assembler, Register::Rsi, Register::R11)?;
    emit_write_decimal_call(assembler, labels, Register::Rsi)?;
    emit_write_literal(assembler, plan, labels, 1, SPACE)?;
    emit_count_row_columns(assembler, plan, Register::R12, Register::Rsi)?;
    emit_write_decimal_call(assembler, labels, Register::Rsi)?;
    emit_write_literal(assembler, plan, labels, 1, NEWLINE)?;

    assembler.data_address(Register::R11, plan.observation_schema_cursor_offset)?;
    assembler.mov_imm64(Register::Rax, 0)?;
    emit_mov64_mem_reg(assembler, Register::R11, Register::Rax)?;
    let column_loop = assembler.new_label()?;
    let next_column = assembler.new_label()?;
    let columns_done = assembler.new_label()?;
    assembler.bind(column_loop)?;
    assembler.data_address(Register::R11, plan.observation_schema_cursor_offset)?;
    emit_mov64_reg_mem(assembler, Register::R9, Register::R11)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEMAS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R9, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, columns_done)?;
    emit_storage_row_address(assembler, plan, Register::R9, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_KIND,
        Register::Rax,
    )?;
    assembler.mov_imm64(Register::Rcx, wire::schema::RESOURCE)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::Equal, next_column)?;
    emit_test_row_membership(
        assembler,
        plan,
        Register::R12,
        Register::R9,
        Condition::Zero,
        next_column,
    )?;
    emit_write_literal(assembler, plan, labels, 1, COLUMN_PREFIX)?;
    assembler.data_address(Register::R11, plan.observation_schema_cursor_offset)?;
    emit_mov64_reg_mem(assembler, Register::R9, Register::R11)?;
    emit_schema_id_pointer(assembler, plan, Register::R9, Register::Rsi)?;
    emit_write_hex_call(assembler, labels, Register::Rsi, 16)?;
    emit_write_literal(assembler, plan, labels, 1, SPACE)?;
    assembler.data_address(Register::R11, plan.observation_schema_cursor_offset)?;
    emit_mov64_reg_mem(assembler, Register::R9, Register::R11)?;
    emit_storage_row_address(assembler, plan, Register::R9, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_BYTE_SIZE,
        Register::Rsi,
    )?;
    emit_write_decimal_call(assembler, labels, Register::Rsi)?;
    emit_write_literal(assembler, plan, labels, 1, SPACE)?;
    assembler.data_address(Register::R11, plan.observation_schema_cursor_offset)?;
    emit_mov64_reg_mem(assembler, Register::R9, Register::R11)?;
    emit_storage_row_address(assembler, plan, Register::R9, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_ROW_CELL,
        Register::R8,
    )?;
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_BYTE_SIZE,
        Register::Rdx,
    )?;
    emit_row_address(assembler, plan, Register::R12, Register::Rsi)?;
    assembler.add_reg64(Register::Rsi, Register::R8)?;
    emit_write_hex_register(assembler, labels, Register::Rsi, Register::Rdx)?;
    emit_write_literal(assembler, plan, labels, 1, NEWLINE)?;
    assembler.bind(next_column)?;
    assembler.data_address(Register::R11, plan.observation_schema_cursor_offset)?;
    emit_mov64_reg_mem(assembler, Register::R9, Register::R11)?;
    emit_increment64(assembler, Register::R9)?;
    emit_mov64_mem_reg(assembler, Register::R11, Register::R9)?;
    assembler.far_jump(column_loop)?;
    assembler.bind(columns_done)?;
    emit_increment64(assembler, Register::R12)?;
    assembler.far_jump(row_loop)?;
    assembler.bind(rows_done)?;
    assembler.mov_reg64(Register::Rbx, Register::Rbp)?;
    assembler.far_jump(table_loop)?;
    assembler.bind(tables_done)?;
    emit_write_literal(assembler, plan, labels, 1, OBSERVATION_END)?;
    emit_runtime_epilogue(assembler)
}

fn emit_write_decimal_call(
    assembler: &mut Assembler,
    labels: &RuntimeInternalLabels,
    value: Register,
) -> Result<(), AotV2Error> {
    if value != Register::Rsi {
        assembler.mov_reg64(Register::Rsi, value)?;
    }
    assembler.mov_imm64(Register::Rdi, 1)?;
    assembler.far_call(labels.write_decimal)
}

fn emit_write_hex_call(
    assembler: &mut Assembler,
    labels: &RuntimeInternalLabels,
    bytes: Register,
    byte_len: u64,
) -> Result<(), AotV2Error> {
    if bytes != Register::Rsi {
        assembler.mov_reg64(Register::Rsi, bytes)?;
    }
    assembler.mov_imm64(Register::Rdi, 1)?;
    assembler.mov_imm64(Register::Rdx, byte_len)?;
    assembler.far_call(labels.write_hex)
}

fn emit_write_hex_register(
    assembler: &mut Assembler,
    labels: &RuntimeInternalLabels,
    bytes: Register,
    byte_len: Register,
) -> Result<(), AotV2Error> {
    if bytes != Register::Rsi {
        assembler.mov_reg64(Register::Rsi, bytes)?;
    }
    if byte_len != Register::Rdx {
        assembler.mov_reg64(Register::Rdx, byte_len)?;
    }
    assembler.mov_imm64(Register::Rdi, 1)?;
    assembler.far_call(labels.write_hex)
}

fn emit_schema_id_pointer(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    schema: Register,
    destination: Register,
) -> Result<(), AotV2Error> {
    emit_section_record_address(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEMAS)?,
        schema,
        wire::schema::RECORD_SIZE,
        destination,
    )
}

fn emit_test_row_membership(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    row: Register,
    schema: Register,
    condition: Condition,
    target: Label,
) -> Result<(), AotV2Error> {
    emit_row_address(assembler, plan, row, Register::R10)?;
    assembler.mov_imm64(Register::R11, plan.world.row_membership_offset)?;
    assembler.add_reg64(Register::R11, Register::R10)?;
    assembler.mov_reg64(Register::Rax, schema)?;
    emit_shift_right64_imm8(assembler, Register::Rax, 3)?;
    assembler.add_reg64(Register::R11, Register::Rax)?;
    emit_mov8_reg_mem(assembler, Register::Rax, Register::R11)?;
    assembler.mov_reg64(Register::Rcx, schema)?;
    emit_and64_imm8(assembler, Register::Rcx, 7)?;
    assembler.mov_imm64(Register::Rdx, 1)?;
    emit_shift_left64_cl(assembler, Register::Rdx)?;
    emit_test64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    assembler.far_jcc(condition, target)
}

fn emit_count_row_columns(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    row: Register,
    destination: Register,
) -> Result<(), AotV2Error> {
    assembler.mov_imm64(Register::R8, 0)?;
    assembler.mov_imm64(Register::R9, 0)?;
    let loop_label = assembler.new_label()?;
    let next = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(loop_label)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEMAS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::AboveEqual, done)?;
    emit_storage_row_address(assembler, plan, Register::R8, Register::R10)?;
    emit_load_record_field(
        assembler,
        Register::R10,
        RUNTIME_STORAGE_KIND,
        Register::Rax,
    )?;
    assembler.mov_imm64(Register::Rcx, wire::schema::RESOURCE)?;
    emit_cmp64_reg_reg(assembler, Register::Rax, Register::Rcx)?;
    assembler.far_jcc(Condition::Equal, next)?;
    emit_test_row_membership(assembler, plan, row, Register::R8, Condition::Zero, next)?;
    emit_increment64(assembler, Register::R9)?;
    assembler.bind(next)?;
    emit_increment64(assembler, Register::R8)?;
    assembler.far_jump(loop_label)?;
    assembler.bind(done)?;
    if destination != Register::R9 {
        assembler.mov_reg64(destination, Register::R9)?;
    }
    Ok(())
}

fn emit_write_all_helper(
    assembler: &mut Assembler,
    labels: &RuntimeInternalLabels,
) -> Result<(), AotV2Error> {
    assembler.bind(labels.write_all)?;
    emit_runtime_prologue(assembler)?;
    let loop_label = assembler.new_label()?;
    let done = assembler.new_label()?;
    emit_test64_reg_reg(assembler, Register::Rdx, Register::Rdx)?;
    assembler.far_jcc(Condition::Zero, done)?;
    assembler.bind(loop_label)?;
    assembler.mov_imm64(Register::Rax, 1)?; // write
    assembler.emit(&[0x0f, 0x05])?;
    emit_test64_reg_reg(assembler, Register::Rax, Register::Rax)?;
    assembler.far_jcc(Condition::LessEqual, labels.failure)?;
    assembler.add_reg64(Register::Rsi, Register::Rax)?;
    emit_sub64_reg_reg(assembler, Register::Rdx, Register::Rax)?;
    assembler.far_jcc(Condition::NotZero, loop_label)?;
    assembler.bind(done)?;
    emit_runtime_epilogue(assembler)
}

fn emit_write_decimal_helper(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    labels: &RuntimeInternalLabels,
) -> Result<(), AotV2Error> {
    assembler.bind(labels.write_decimal)?;
    emit_runtime_prologue(assembler)?;
    assembler.mov_reg64(Register::Rbx, Register::Rdi)?;
    assembler.mov_reg64(Register::R12, Register::Rsi)?;
    assembler.data_address(
        Register::R9,
        plan.decimal_scratch_offset
            .checked_add(32)
            .ok_or(AotV2Error::ArithmeticOverflow("decimal scratch end"))?,
    )?;
    assembler.mov_reg64(Register::R8, Register::R9)?;
    let nonzero = assembler.new_label()?;
    let digits = assembler.new_label()?;
    let ready = assembler.new_label()?;
    emit_test64_reg_reg(assembler, Register::R12, Register::R12)?;
    assembler.far_jcc(Condition::NotZero, nonzero)?;
    emit_decrement64(assembler, Register::R8)?;
    assembler.mov_imm64(Register::Rax, u64::from(b'0'))?;
    emit_mov8_mem_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jump(ready)?;
    assembler.bind(nonzero)?;
    assembler.bind(digits)?;
    assembler.mov_reg64(Register::Rax, Register::R12)?;
    assembler.emit(&[0x48, 0x31, 0xd2])?; // xor rdx,rdx
    assembler.mov_imm64(Register::Rcx, 10)?;
    assembler.emit(&[0x48, 0xf7, 0xf1])?; // div rcx
    assembler.mov_reg64(Register::R12, Register::Rax)?;
    emit_decrement64(assembler, Register::R8)?;
    assembler.emit(&[0x80, 0xc2, b'0'])?; // add dl,'0'
    emit_mov8_mem_reg(assembler, Register::R8, Register::Rdx)?;
    emit_test64_reg_reg(assembler, Register::R12, Register::R12)?;
    assembler.far_jcc(Condition::NotZero, digits)?;
    assembler.bind(ready)?;
    assembler.mov_reg64(Register::Rdi, Register::Rbx)?;
    assembler.mov_reg64(Register::Rsi, Register::R8)?;
    assembler.mov_reg64(Register::Rdx, Register::R9)?;
    emit_sub64_reg_reg(assembler, Register::Rdx, Register::R8)?;
    assembler.far_call(labels.write_all)?;
    emit_runtime_epilogue(assembler)
}

fn emit_write_hex_helper(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    labels: &RuntimeInternalLabels,
) -> Result<(), AotV2Error> {
    assembler.bind(labels.write_hex)?;
    emit_runtime_prologue(assembler)?;
    assembler.mov_reg64(Register::Rbx, Register::Rdi)?;
    assembler.mov_reg64(Register::R12, Register::Rsi)?;
    assembler.mov_reg64(Register::Rbp, Register::Rdx)?;
    let loop_label = assembler.new_label()?;
    let done = assembler.new_label()?;
    let nonempty = assembler.new_label()?;
    emit_test64_reg_reg(assembler, Register::Rbp, Register::Rbp)?;
    assembler.far_jcc(Condition::NotZero, nonempty)?;
    emit_write_literal(assembler, plan, labels, 1, EMPTY_PAYLOAD)?;
    assembler.far_jump(done)?;
    assembler.bind(nonempty)?;
    assembler.bind(loop_label)?;
    emit_mov8_reg_mem(assembler, Register::Rax, Register::R12)?;
    assembler.mov_reg64(Register::Rcx, Register::Rax)?;
    assembler.emit(&[0xc0, 0xe9, 0x04])?; // shr cl,4
    assembler.emit(&[0x83, 0xe0, 0x0f])?; // and eax,15
    emit_hex_digit(assembler, Register::Rcx)?;
    emit_hex_digit(assembler, Register::Rax)?;
    assembler.data_address(Register::R8, plan.hex_scratch_offset)?;
    emit_mov8_mem_reg(assembler, Register::R8, Register::Rcx)?;
    emit_increment64(assembler, Register::R8)?;
    emit_mov8_mem_reg(assembler, Register::R8, Register::Rax)?;
    assembler.mov_imm64(Register::Rdi, 1)?;
    assembler.data_address(Register::Rsi, plan.hex_scratch_offset)?;
    assembler.mov_imm64(Register::Rdx, 2)?;
    assembler.far_call(labels.write_all)?;
    emit_increment64(assembler, Register::R12)?;
    emit_decrement64(assembler, Register::Rbp)?;
    assembler.far_jcc(Condition::NotZero, loop_label)?;
    assembler.bind(done)?;
    emit_runtime_epilogue(assembler)
}

fn emit_hex_digit(assembler: &mut Assembler, register: Register) -> Result<(), AotV2Error> {
    let decimal = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.mov_imm64(Register::Rdx, 10)?;
    emit_cmp64_reg_reg(assembler, register, Register::Rdx)?;
    assembler.far_jcc(Condition::Below, decimal)?;
    assembler.mov_imm64(Register::Rdx, u64::from(b'A' - 10))?;
    assembler.add_reg64(register, Register::Rdx)?;
    assembler.far_jump(done)?;
    assembler.bind(decimal)?;
    assembler.mov_imm64(Register::Rdx, u64::from(b'0'))?;
    assembler.add_reg64(register, Register::Rdx)?;
    assembler.bind(done)
}

fn emit_compare_keys_helper(
    assembler: &mut Assembler,
    plan: &NativeRuntimePlan,
    labels: &RuntimeInternalLabels,
) -> Result<(), AotV2Error> {
    assembler.bind(labels.compare_keys)?;
    emit_runtime_prologue(assembler)?;
    assembler.emit(&[0x41, 0x50, 0x41, 0x51])?; // push r8; push r9
    assembler.mov_imm64(Register::R8, 0)?;
    assembler.mov_imm64(Register::R9, 0)?;
    let outer = assembler.new_label()?;
    let find_a = assembler.new_label()?;
    let found_a = assembler.new_label()?;
    let find_b = assembler.new_label()?;
    let found_b = assembler.new_label()?;
    let indexes_differ = assembler.new_label()?;
    let less = assembler.new_label()?;
    let greater = assembler.new_label()?;
    let equal = assembler.new_label()?;
    let finish = assembler.new_label()?;
    assembler.bind(outer)?;
    assembler.bind(find_a)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEMAS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::Equal, found_a)?;
    assembler.mov_reg64(Register::R10, Register::R8)?;
    emit_shift_right64_imm8(assembler, Register::R10, 3)?;
    assembler.add_reg64(Register::R10, Register::Rdi)?;
    emit_mov8_reg_mem(assembler, Register::Rax, Register::R10)?;
    assembler.mov_reg64(Register::Rcx, Register::R8)?;
    emit_and64_imm8(assembler, Register::Rcx, 7)?;
    assembler.mov_imm64(Register::Rdx, 1)?;
    emit_shift_left64_cl(assembler, Register::Rdx)?;
    emit_test64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    assembler.far_jcc(Condition::NotZero, found_a)?;
    emit_increment64(assembler, Register::R8)?;
    assembler.far_jump(find_a)?;
    assembler.bind(found_a)?;

    assembler.bind(find_b)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEMAS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R9, Register::Rax)?;
    assembler.far_jcc(Condition::Equal, found_b)?;
    assembler.mov_reg64(Register::R10, Register::R9)?;
    emit_shift_right64_imm8(assembler, Register::R10, 3)?;
    assembler.add_reg64(Register::R10, Register::Rsi)?;
    emit_mov8_reg_mem(assembler, Register::Rax, Register::R10)?;
    assembler.mov_reg64(Register::Rcx, Register::R9)?;
    emit_and64_imm8(assembler, Register::Rcx, 7)?;
    assembler.mov_imm64(Register::Rdx, 1)?;
    emit_shift_left64_cl(assembler, Register::Rdx)?;
    emit_test64_reg_reg(assembler, Register::Rax, Register::Rdx)?;
    assembler.far_jcc(Condition::NotZero, found_b)?;
    emit_increment64(assembler, Register::R9)?;
    assembler.far_jump(find_b)?;
    assembler.bind(found_b)?;

    emit_cmp64_reg_reg(assembler, Register::R8, Register::R9)?;
    assembler.far_jcc(Condition::NotEqual, indexes_differ)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEMAS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::Equal, equal)?;
    emit_increment64(assembler, Register::R8)?;
    emit_increment64(assembler, Register::R9)?;
    assembler.far_jump(outer)?;

    assembler.bind(indexes_differ)?;
    emit_section_record_count(
        assembler,
        plan,
        section_index(wire::section_kind::SCHEMAS)?,
        Register::Rax,
    )?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::Rax)?;
    assembler.far_jcc(Condition::Equal, less)?;
    emit_cmp64_reg_reg(assembler, Register::R9, Register::Rax)?;
    assembler.far_jcc(Condition::Equal, greater)?;
    emit_cmp64_reg_reg(assembler, Register::R8, Register::R9)?;
    assembler.far_jcc(Condition::Below, less)?;
    assembler.far_jump(greater)?;

    assembler.bind(less)?;
    assembler.mov_imm64(Register::Rax, u64::MAX)?;
    assembler.far_jump(finish)?;
    assembler.bind(greater)?;
    assembler.mov_imm64(Register::Rax, 1)?;
    assembler.far_jump(finish)?;
    assembler.bind(equal)?;
    assembler.mov_imm64(Register::Rax, 0)?;
    assembler.bind(finish)?;
    assembler.emit(&[0x41, 0x59, 0x41, 0x58])?; // pop r9; pop r8
    emit_runtime_epilogue(assembler)
}

fn emit_compare_bytes_helper(
    assembler: &mut Assembler,
    labels: &RuntimeInternalLabels,
) -> Result<(), AotV2Error> {
    assembler.bind(labels.compare_bytes)?;
    emit_runtime_prologue(assembler)?;
    assembler.emit(&[0x41, 0x50, 0x41, 0x51])?; // push r8; push r9
    assembler.mov_reg64(Register::Rbp, Register::Rdx)?; // left length
    assembler.mov_reg64(Register::Rbx, Register::Rcx)?; // right length
    assembler.mov_reg64(Register::Rcx, Register::Rbp)?;
    emit_cmp64_reg_reg(assembler, Register::Rcx, Register::Rbx)?;
    let length_ready = assembler.new_label()?;
    assembler.far_jcc(Condition::BelowEqual, length_ready)?;
    assembler.mov_reg64(Register::Rcx, Register::Rbx)?;
    assembler.bind(length_ready)?;

    let prefix_equal = assembler.new_label()?;
    let less = assembler.new_label()?;
    let greater = assembler.new_label()?;
    let finish = assembler.new_label()?;
    emit_test64_reg_reg(assembler, Register::Rcx, Register::Rcx)?;
    assembler.far_jcc(Condition::Zero, prefix_equal)?;
    assembler.emit(&[0xfc, 0xf3, 0xa6])?; // cld; repe cmpsb
                                          // cmpsb sets flags from [rsi] - [rdi], the reverse of this helper's
                                          // documented left-[rdi], right-[rsi] ordering.
    assembler.far_jcc(Condition::Above, less)?;
    assembler.far_jcc(Condition::Below, greater)?;
    assembler.bind(prefix_equal)?;
    emit_cmp64_reg_reg(assembler, Register::Rbp, Register::Rbx)?;
    assembler.far_jcc(Condition::Below, less)?;
    assembler.far_jcc(Condition::Above, greater)?;
    assembler.mov_imm64(Register::Rax, 0)?;
    assembler.far_jump(finish)?;
    assembler.bind(less)?;
    assembler.mov_imm64(Register::Rax, u64::MAX)?;
    assembler.far_jump(finish)?;
    assembler.bind(greater)?;
    assembler.mov_imm64(Register::Rax, 1)?;
    assembler.bind(finish)?;
    assembler.emit(&[0x41, 0x59, 0x41, 0x58])?; // pop r9; pop r8
    emit_runtime_epilogue(assembler)
}

pub(crate) fn finalize_runtime_data(
    plan: &NativeRuntimePlan,
    world: &WorldStoragePlan,
    core: &VerifiedExecutableCore,
    package: &ExecutionPackage,
    native_layout: &NativeCodeLayout,
    metadata_byte_len: u64,
) -> Result<Vec<DataChunk>, AotV2Error> {
    if world != &plan.world {
        return Err(invalid_native(
            "native runtime finalization received a different world storage plan",
        ));
    }
    validate_world_storage(core, world)?;
    validate_execution_package_link(core, package, Some(native_layout.code_range))?;
    validate_package_with_code_range(package, native_layout.code_range)?;
    validate_native_function_layout(package, native_layout)?;
    let mut metadata_directory = MetadataDirectoryCapture::new();
    let encoded_metadata_byte_len =
        write_package_with_code_range(&mut metadata_directory, package, native_layout.code_range)?;
    if metadata_byte_len != encoded_metadata_byte_len
        || metadata_byte_len != metadata_directory.byte_len()
    {
        return Err(invalid_native(
            "native runtime metadata length disagrees with the encoded v2 package",
        ));
    }

    let mut header = Vec::new();
    reserve_vec(
        &mut header,
        as_usize(RUNTIME_HEADER_BYTE_LEN, "runtime header length")?,
        "runtime header",
    )?;
    header.extend_from_slice(RUNTIME_MAGIC);
    append_u64(&mut header, metadata_byte_len);
    append_u64(&mut header, native_layout.code_range.offset);
    append_u64(&mut header, native_layout.code_range.byte_len);
    for _ in 0..4 {
        append_u64(&mut header, 0);
    }
    if header.len() != as_usize(RUNTIME_HEADER_BYTE_LEN, "runtime header length")? {
        return Err(invalid_native(
            "runtime header has the wrong encoded length",
        ));
    }

    let mut function_rows = Vec::new();
    reserve_vec(
        &mut function_rows,
        package
            .function_links
            .len()
            .checked_mul(as_usize(
                RUNTIME_FUNCTION_ROW_BYTE_LEN,
                "runtime function row length",
            )?)
            .ok_or(AotV2Error::AddressSpaceOverflow("runtime function rows"))?,
        "runtime function rows",
    )?;
    for function in &native_layout.functions {
        append_u64(&mut function_rows, function.code_offset);
        append_u64(&mut function_rows, function.code_byte_len);
    }

    let mut storage_rows = Vec::new();
    reserve_vec(
        &mut storage_rows,
        world
            .schemas
            .len()
            .checked_mul(as_usize(
                RUNTIME_STORAGE_ROW_BYTE_LEN,
                "runtime storage row length",
            )?)
            .ok_or(AotV2Error::AddressSpaceOverflow("runtime storage rows"))?,
        "runtime storage rows",
    )?;
    for storage in &world.schemas {
        storage_rows.extend_from_slice(storage.id.as_bytes());
        let kind = match storage.kind {
            SchemaKind::Component => wire::schema::COMPONENT,
            SchemaKind::Resource => wire::schema::RESOURCE,
            SchemaKind::Tag => wire::schema::TAG,
        };
        append_u64(&mut storage_rows, kind);
        append_u64(
            &mut storage_rows,
            if storage.kind == SchemaKind::Tag {
                wire::schema::flags::TAG
            } else {
                0
            },
        );
        append_u64(&mut storage_rows, storage.byte_size);
        append_u64(&mut storage_rows, storage.alignment);
        append_u64(
            &mut storage_rows,
            storage.resource_initialized_offset.unwrap_or(u64::MAX),
        );
        append_u64(
            &mut storage_rows,
            storage.resource_payload_offset.unwrap_or(u64::MAX),
        );
        append_u64(
            &mut storage_rows,
            storage.row_cell_offset.unwrap_or(u64::MAX),
        );
    }

    let mut trap_rows = Vec::new();
    reserve_vec(
        &mut trap_rows,
        plan.trap_descriptors
            .len()
            .checked_mul(as_usize(
                RUNTIME_TRAP_ROW_BYTE_LEN,
                "runtime trap row length",
            )?)
            .ok_or(AotV2Error::AddressSpaceOverflow("runtime trap rows"))?,
        "runtime trap rows",
    )?;
    for descriptor in &plan.trap_descriptors {
        let span_index = package
            .source_spans
            .iter()
            .position(|record| source_span_matches(record, descriptor.span))
            .ok_or_else(|| invalid_native("native trap span is absent from v2 metadata"))?;
        append_u64(
            &mut trap_rows,
            as_u64(span_index, "runtime trap source span index")?,
        );
        let function_index = package
            .function_links
            .iter()
            .position(|link| match link.target {
                FunctionTarget::Startup => descriptor.function_kind == wire::function_link::STARTUP,
                FunctionTarget::System { system } => {
                    descriptor.function_kind == wire::function_link::SYSTEM_TARGET
                        && system.index() == descriptor.function_system
                }
            })
            .ok_or_else(|| invalid_native("native trap function is absent from v2 metadata"))?;
        append_u64(
            &mut trap_rows,
            as_u64(function_index, "runtime trap function index")?,
        );
    }

    let mut chunks = Vec::new();
    let chunk_count = 4usize
        .checked_add(plan.literals.len())
        .ok_or(AotV2Error::AddressSpaceOverflow("runtime data chunks"))?;
    reserve_vec(&mut chunks, chunk_count, "runtime data chunks")?;
    chunks.push(DataChunk {
        offset: plan.runtime_header_offset,
        bytes: header,
    });
    if !function_rows.is_empty() {
        chunks.push(DataChunk {
            offset: plan.function_rows_offset,
            bytes: function_rows,
        });
    }
    if !trap_rows.is_empty() {
        chunks.push(DataChunk {
            offset: plan.trap_rows_offset,
            bytes: trap_rows,
        });
    }
    if !storage_rows.is_empty() {
        chunks.push(DataChunk {
            offset: plan.storage_rows_offset,
            bytes: storage_rows,
        });
    }
    for literal in &plan.literals {
        chunks.push(DataChunk {
            offset: literal.offset,
            bytes: literal.bytes.to_vec(),
        });
    }
    chunks.sort_unstable_by_key(|chunk| chunk.offset);
    validate_chunks(plan, &chunks)?;
    Ok(chunks)
}

fn validate_native_function_layout(
    package: &ExecutionPackage,
    native_layout: &NativeCodeLayout,
) -> Result<(), AotV2Error> {
    if package.function_links.len() != native_layout.functions.len() {
        return Err(invalid_native(
            "metadata function-link count does not match emitted native functions",
        ));
    }
    for (link, function) in package.function_links.iter().zip(&native_layout.functions) {
        let target_matches = match (link.target, function.target) {
            (FunctionTarget::Startup, NativeFunctionTarget::Startup) => true,
            (FunctionTarget::System { system }, NativeFunctionTarget::System(expected_id)) => {
                package
                    .systems
                    .get(as_usize(system.index(), "function-link system index")?)
                    .is_some_and(|record| record.id == expected_id)
            }
            _ => false,
        };
        if !target_matches
            || link.code_offset != function.code_offset
            || link.code_byte_len != function.code_byte_len
        {
            return Err(invalid_native(
                "metadata function link does not match the emitted native function",
            ));
        }
    }
    Ok(())
}

fn build_link_manifest(core: &VerifiedExecutableCore) -> Result<NativeLinkManifest, AotV2Error> {
    let ids = canonical_core_ids(core)?;
    let mut systems = Vec::new();
    reserve_vec(
        &mut systems,
        core.program().systems.len(),
        "dummy runtime system layout",
    )?;
    for system in &core.program().systems {
        systems.push((
            ids.system(system.id)
                .ok_or_else(|| invalid_native("Core system has no canonical ID"))?,
            system.name.as_str(),
        ));
    }
    systems.sort_unstable_by_key(|(id, _)| *id);
    let function_count = systems
        .len()
        .checked_add(1)
        .ok_or(AotV2Error::AddressSpaceOverflow("dummy runtime functions"))?;
    let code_byte_len = as_u64(function_count, "dummy runtime code range")?;
    let mut functions = Vec::new();
    reserve_vec(&mut functions, function_count, "dummy runtime functions")?;
    functions.push(NativeFunctionLayout {
        target: NativeFunctionTarget::Startup,
        symbol_name: "arche_runtime_expected_startup".to_string(),
        code_offset: 0,
        code_byte_len: 1,
    });
    for (index, (id, name)) in systems.into_iter().enumerate() {
        functions.push(NativeFunctionLayout {
            target: NativeFunctionTarget::System(id),
            symbol_name: format!("arche_runtime_expected_{name}"),
            code_offset: as_u64(index, "dummy runtime function index")?
                .checked_add(1)
                .ok_or(AotV2Error::ArithmeticOverflow("dummy runtime function"))?,
            code_byte_len: 1,
        });
    }
    let package = build_execution_package(
        core,
        "native-runtime.arc",
        &NativeCodeLayout {
            code_range: CodeImageRange {
                offset: 0,
                byte_len: code_byte_len,
            },
            functions,
        },
    )?;
    NativeLinkManifest::from_package(&package)
}

fn build_trap_descriptors(
    core: &VerifiedExecutableCore,
    trap_points: &[NativeTrapPoint],
    manifest: &NativeLinkManifest,
) -> Result<Vec<TrapDescriptorPlan>, AotV2Error> {
    let ids = canonical_core_ids(core)?;
    let mut seen = HashSet::new();
    reserve_set(&mut seen, trap_points.len(), "runtime trap uniqueness")?;
    let mut descriptors = Vec::new();
    reserve_vec(
        &mut descriptors,
        trap_points.len(),
        "runtime trap descriptors",
    )?;
    for point in trap_points {
        if !seen.insert(*point) {
            return Err(invalid_native("duplicate native trap point"));
        }
        let (subject, function_kind, function_system) = match *point {
            NativeTrapPoint::Startup {
                block,
                instruction_index,
            } => (
                CoreSourceSubject::StartupInstruction {
                    block,
                    instruction_index,
                },
                wire::function_link::STARTUP,
                u64::MAX,
            ),
            NativeTrapPoint::System {
                system_id,
                expression_ordinal,
            } => {
                let canonical_id = ids.system(system_id).ok_or_else(|| {
                    invalid_native("native trap system has no canonical identifier")
                })?;
                let system_index = manifest
                    .systems
                    .binary_search_by_key(&canonical_id, |record| record.id)
                    .map_err(|_| {
                        invalid_native("native trap system is absent from the manifest")
                    })?;
                (
                    CoreSourceSubject::SystemExpression {
                        system_id,
                        expression_ordinal,
                    },
                    wire::function_link::SYSTEM_TARGET,
                    as_u64(system_index, "native trap system index")?,
                )
            }
        };
        let span = core.program().source_map.span(&subject).ok_or_else(|| {
            invalid_native(format!("native trap point has no source span: {subject:?}"))
        })?;
        descriptors.push(TrapDescriptorPlan {
            point: *point,
            span,
            function_kind,
            function_system,
        });
    }
    Ok(descriptors)
}

fn validate_world_storage(
    core: &VerifiedExecutableCore,
    world: &WorldStoragePlan,
) -> Result<(), AotV2Error> {
    if world.schemas.len()
        != core
            .program()
            .components
            .len()
            .checked_add(core.program().resources.len())
            .ok_or(AotV2Error::AddressSpaceOverflow("world schema count"))?
    {
        return Err(invalid_native(
            "native world schema count does not match verified Core",
        ));
    }
    let expected_membership_bytes = as_u64(world.schemas.len(), "world schema count")?
        .checked_add(7)
        .ok_or(AotV2Error::ArithmeticOverflow("world membership bytes"))?
        / 8;
    if world.row_membership_bytes != expected_membership_bytes {
        return Err(invalid_native(
            "native row membership bitmap has the wrong size",
        ));
    }
    for (index, schema) in world.schemas.iter().enumerate() {
        if schema.dense_index != as_u64(index, "world dense schema index")? {
            return Err(invalid_native("native schemas are not densely indexed"));
        }
        if index > 0 && world.schemas[index - 1].id >= schema.id {
            return Err(invalid_native(
                "native schemas are not in canonical ID order",
            ));
        }
        match schema.kind {
            SchemaKind::Resource
                if schema.resource_initialized_offset.is_some()
                    && schema.resource_payload_offset.is_some()
                    && schema.row_cell_offset.is_none() => {}
            SchemaKind::Component | SchemaKind::Tag
                if schema.resource_initialized_offset.is_none()
                    && schema.resource_payload_offset.is_none()
                    && (schema.byte_size == 0 || schema.row_cell_offset.is_some()) => {}
            _ => {
                return Err(invalid_native(format!(
                    "native storage for schema {} has inconsistent world cells",
                    schema.id
                )));
            }
        }
        let mut expected_offset = 0u64;
        let mut expected_alignment = 1u64;
        for field in &schema.fields {
            let (byte_len, alignment) = match field.primitive {
                archec0::ids_v2::PrimitiveType::I32 | archec0::ids_v2::PrimitiveType::F32 => (4, 4),
                archec0::ids_v2::PrimitiveType::Bool => (1, 1),
            };
            expected_offset = align_u64(expected_offset, alignment, "runtime field layout")?;
            if field.byte_offset != expected_offset {
                return Err(invalid_native(format!(
                    "native field `{}.{}` has a noncanonical byte offset",
                    schema.id, field.name
                )));
            }
            expected_offset = expected_offset
                .checked_add(byte_len)
                .ok_or(AotV2Error::ArithmeticOverflow("native field layout"))?;
            expected_alignment = expected_alignment.max(alignment);
        }
        expected_offset = align_u64(expected_offset, expected_alignment, "runtime schema size")?;
        if expected_offset != schema.byte_size {
            return Err(invalid_native(format!(
                "native schema {} byte size does not match its fields",
                schema.id
            )));
        }
        if expected_alignment != schema.alignment {
            return Err(invalid_native(format!(
                "native schema {} alignment does not match its fields",
                schema.id
            )));
        }
    }
    Ok(())
}

fn validate_chunks(plan: &NativeRuntimePlan, chunks: &[DataChunk]) -> Result<(), AotV2Error> {
    let mut prior_end = 0;
    for chunk in chunks {
        let end = chunk
            .offset
            .checked_add(as_u64(chunk.bytes.len(), "runtime data chunk")?)
            .ok_or(AotV2Error::ArithmeticOverflow("runtime data chunk"))?;
        if chunk.offset < prior_end {
            return Err(invalid_native("runtime data chunks overlap"));
        }
        if end > plan.data_file_byte_len || end > plan.data_memory_byte_len {
            return Err(invalid_native(
                "runtime data chunk exceeds the planned data image",
            ));
        }
        prior_end = end;
    }
    Ok(())
}

fn source_span_matches(
    record: &archec0::execution_package_v2::SourceSpanRecord,
    span: SourceSpan,
) -> bool {
    record.start_byte == span.start.byte
        && record.end_byte == span.end.byte
        && record.start_line == span.start.line
        && record.start_column == span.start.column
        && record.end_line == span.end.line
        && record.end_column == span.end.column
}

fn append_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn reserve_vec<T>(
    values: &mut Vec<T>,
    additional: usize,
    context: &'static str,
) -> Result<(), AotV2Error> {
    values
        .try_reserve_exact(additional)
        .map_err(|_| AotV2Error::Allocation(context))
}

fn reserve_map<K: Eq + std::hash::Hash, V>(
    values: &mut HashMap<K, V>,
    additional: usize,
    context: &'static str,
) -> Result<(), AotV2Error> {
    values
        .try_reserve(additional)
        .map_err(|_| AotV2Error::Allocation(context))
}

fn reserve_set<K: Eq + std::hash::Hash>(
    values: &mut HashSet<K>,
    additional: usize,
    context: &'static str,
) -> Result<(), AotV2Error> {
    values
        .try_reserve(additional)
        .map_err(|_| AotV2Error::Allocation(context))
}

fn as_u64(value: usize, context: &'static str) -> Result<u64, AotV2Error> {
    u64::try_from(value).map_err(|_| AotV2Error::AddressSpaceOverflow(context))
}

fn as_usize(value: u64, context: &'static str) -> Result<usize, AotV2Error> {
    usize::try_from(value).map_err(|_| AotV2Error::AddressSpaceOverflow(context))
}

fn align_u64(value: u64, alignment: u64, context: &'static str) -> Result<u64, AotV2Error> {
    if alignment == 0 || !alignment.is_power_of_two() {
        return Err(invalid_native(format!(
            "native {context} alignment {alignment} is not a nonzero power of two"
        )));
    }
    value
        .checked_add(alignment - 1)
        .map(|adjusted| adjusted & !(alignment - 1))
        .ok_or(AotV2Error::ArithmeticOverflow(context))
}

fn invalid_native(message: impl Into<String>) -> AotV2Error {
    AotV2Error::InvalidNativePlan(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_directory_capture_retains_only_its_bounded_prefix() {
        let mut capture = MetadataDirectoryCapture::new();
        let beyond_capture = u64::try_from(METADATA_DIRECTORY_CAPTURE_BYTE_LEN)
            .expect("capture length fits u64")
            + 32;
        capture
            .seek(SeekFrom::Start(beyond_capture))
            .expect("forward sparse seek succeeds");
        capture
            .write_all(b"discarded")
            .expect("suffix write succeeds");
        capture
            .seek(SeekFrom::Start(0))
            .expect("backpatch seek succeeds");
        capture
            .write_all(b"ARCHEECS")
            .expect("prefix backpatch succeeds");

        assert_eq!(&capture.bytes[..8], b"ARCHEECS");
        assert_eq!(capture.byte_len(), beyond_capture + 9);
        assert_eq!(capture.bytes.len(), 64 + 14 * 64);
    }

    #[test]
    fn native_finalization_rejects_function_ranges_shifted_from_emitted_code() {
        let source = "world FunctionRange\nstartup { exit 0 }\n";
        let tokens = crate::lexer::lex(source).expect("fixture lexes");
        let program = crate::parser::parse_program(&tokens).expect("fixture parses");
        crate::checker::check_program(&program).expect("fixture checks");
        let core = crate::core_lower::lower_program_to_core(&program).expect("fixture lowers");
        let core = crate::core_verify::verify_executable_core(core).expect("fixture verifies");
        let plan = crate::aot_v2::plan_native(&core).expect("native plan builds");
        let mut package =
            build_execution_package(&core, "function-range.arc", plan.native_code_layout())
                .expect("package builds");
        let startup = package
            .function_links
            .first_mut()
            .expect("startup function link exists");
        assert!(startup.code_byte_len > 1);
        startup.code_offset += 1;
        startup.code_byte_len -= 1;

        let error = crate::aot_v2::finalize_native(plan, &core, &package)
            .expect_err("shifted metadata function range is rejected");
        assert!(error
            .to_string()
            .contains("does not match the emitted native function"));
    }

    #[cfg(target_os = "linux")]
    mod linux {
        use super::*;
        use std::fs::OpenOptions;
        use std::os::fd::OwnedFd;
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::net::UnixStream;
        use std::process::{Command, Output, Stdio};
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};

        static ARTIFACT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

        #[derive(Clone, Copy)]
        struct EmbeddedMetadata {
            start: usize,
            directory_offset: usize,
            directory_count: usize,
            directory_entry_size: usize,
        }

        impl EmbeddedMetadata {
            fn parse(artifact: &[u8], start: u64, byte_len: u64) -> Self {
                let start = usize::try_from(start).expect("metadata offset fits usize");
                let byte_len = usize::try_from(byte_len).expect("metadata length fits usize");
                let metadata = artifact
                    .get(start..start + byte_len)
                    .expect("metadata segment is in the artifact");
                assert_eq!(&metadata[..8], b"ARCHEECS");
                Self {
                    start,
                    directory_offset: usize::try_from(read_test_u64(metadata, 32))
                        .expect("directory offset fits usize"),
                    directory_count: usize::try_from(read_test_u64(metadata, 40))
                        .expect("directory count fits usize"),
                    directory_entry_size: usize::try_from(read_test_u64(metadata, 48))
                        .expect("directory stride fits usize"),
                }
            }

            fn directory_row(&self, index: usize) -> usize {
                assert!(index < self.directory_count);
                self.start + self.directory_offset + index * self.directory_entry_size
            }

            fn section_row(&self, artifact: &[u8], kind: u64) -> usize {
                (0..self.directory_count)
                    .map(|index| self.directory_row(index))
                    .find(|row| read_test_u64(artifact, *row) == kind)
                    .unwrap_or_else(|| panic!("section kind {kind} is present"))
            }

            fn section_start(&self, artifact: &[u8], kind: u64) -> usize {
                let row = self.section_row(artifact, kind);
                self.start
                    + usize::try_from(read_test_u64(artifact, row + 16))
                        .expect("section offset fits usize")
            }

            fn section_record_count(&self, artifact: &[u8], kind: u64) -> usize {
                let row = self.section_row(artifact, kind);
                usize::try_from(read_test_u64(artifact, row + 32)).expect("record count fits usize")
            }
        }

        #[derive(Clone, Copy)]
        enum SectionMutation {
            RelativeByte(u64),
            FirstStringByte,
        }

        #[derive(Clone, Copy)]
        struct SectionCorruption {
            name: &'static str,
            kind: u64,
            mutation: SectionMutation,
        }

        const SECTION_CORRUPTIONS: &[SectionCorruption] = &[
            SectionCorruption {
                name: "strings_invalid_utf8",
                kind: wire::section_kind::STRINGS,
                mutation: SectionMutation::FirstStringByte,
            },
            SectionCorruption {
                name: "world_reserved",
                kind: wire::section_kind::WORLD,
                mutation: SectionMutation::RelativeByte(wire::world::RESERVED),
            },
            SectionCorruption {
                name: "schemas_reserved",
                kind: wire::section_kind::SCHEMAS,
                mutation: SectionMutation::RelativeByte(wire::schema::RESERVED),
            },
            SectionCorruption {
                name: "fields_reserved",
                kind: wire::section_kind::FIELDS,
                mutation: SectionMutation::RelativeByte(wire::field::RESERVED),
            },
            SectionCorruption {
                name: "systems_reserved",
                kind: wire::section_kind::SYSTEMS,
                mutation: SectionMutation::RelativeByte(wire::system::RESERVED),
            },
            SectionCorruption {
                name: "parameters_reserved",
                kind: wire::section_kind::PARAMETERS,
                mutation: SectionMutation::RelativeByte(wire::parameter::RESERVED),
            },
            SectionCorruption {
                name: "queries_reserved",
                kind: wire::section_kind::QUERIES,
                mutation: SectionMutation::RelativeByte(wire::query::RESERVED),
            },
            SectionCorruption {
                name: "terms_reserved",
                kind: wire::section_kind::TERMS,
                mutation: SectionMutation::RelativeByte(wire::term::RESERVED),
            },
            SectionCorruption {
                name: "schedules_reserved",
                kind: wire::section_kind::SCHEDULES,
                mutation: SectionMutation::RelativeByte(wire::schedule::RESERVED),
            },
            SectionCorruption {
                name: "schedule_items_reserved",
                kind: wire::section_kind::SCHEDULE_ITEMS,
                mutation: SectionMutation::RelativeByte(wire::schedule_item::RESERVED),
            },
            SectionCorruption {
                name: "startup_operations_reserved",
                kind: wire::section_kind::STARTUP_OPERATIONS,
                mutation: SectionMutation::RelativeByte(wire::startup_operation::RESERVED),
            },
            SectionCorruption {
                name: "payloads_reserved",
                kind: wire::section_kind::PAYLOADS,
                mutation: SectionMutation::RelativeByte(
                    wire::payload::HEADER_SIZE + wire::payload::RESERVED,
                ),
            },
            SectionCorruption {
                name: "function_links_abi_hash",
                kind: wire::section_kind::FUNCTION_LINKS,
                mutation: SectionMutation::RelativeByte(wire::function_link::ABI_HASH),
            },
            SectionCorruption {
                name: "source_spans_reserved",
                kind: wire::section_kind::SOURCE_SPANS,
                mutation: SectionMutation::RelativeByte(wire::source_span::RESERVED),
            },
        ];

        struct RemoveOnDrop(std::path::PathBuf);

        impl Drop for RemoveOnDrop {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }

        fn verified(source: &str) -> VerifiedExecutableCore {
            let tokens = crate::lexer::lex(source).expect("fixture lexes");
            let program = crate::parser::parse_program(&tokens).expect("fixture parses");
            crate::checker::check_program(&program).expect("fixture checks");
            let core = crate::core_lower::lower_program_to_core(&program).expect("fixture lowers");
            crate::core_verify::verify_executable_core(core).expect("fixture Core verifies")
        }

        fn native_artifact() -> (Vec<u8>, EmbeddedMetadata) {
            native_artifact_for_source(
                include_str!("../../../examples/m26_closure.arc"),
                "m26_closure.arc",
            )
        }

        fn native_artifact_for_source(
            source: &str,
            source_name: &str,
        ) -> (Vec<u8>, EmbeddedMetadata) {
            let core = verified(source);
            let plan = crate::aot_v2::plan_native(&core).expect("native plan builds");
            let package = build_execution_package(&core, source_name, plan.native_code_layout())
                .expect("v2 package builds");
            let image = crate::aot_v2::finalize_native(plan, &core, &package)
                .expect("native image finalizes");
            let mut artifact = std::io::Cursor::new(Vec::new());
            let layout = image
                .write_static_pie(&mut artifact, 0)
                .expect("native PIE writes");
            let artifact = artifact.into_inner();
            let metadata = EmbeddedMetadata::parse(
                &artifact,
                layout.metadata_offset,
                layout.metadata_byte_len,
            );
            (artifact, metadata)
        }

        fn read_test_u64(bytes: &[u8], offset: usize) -> u64 {
            u64::from_le_bytes(
                bytes[offset..offset + 8]
                    .try_into()
                    .expect("u64 field is in range"),
            )
        }

        fn write_test_u64(bytes: &mut [u8], offset: usize, value: u64) {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }

        fn string_data_range(
            artifact: &[u8],
            metadata: EmbeddedMetadata,
            index: u64,
        ) -> std::ops::Range<usize> {
            let strings = metadata.section_start(artifact, wire::section_kind::STRINGS);
            let count = usize::try_from(read_test_u64(
                artifact,
                strings + wire::strings::COUNT as usize,
            ))
            .expect("string count fits usize");
            let index = usize::try_from(index).expect("string index fits usize");
            assert!(index < count);
            let record = strings
                + wire::strings::HEADER_SIZE as usize
                + index * wire::strings::RECORD_SIZE as usize;
            let offset = usize::try_from(read_test_u64(
                artifact,
                record + wire::strings::RECORD_OFFSET as usize,
            ))
            .expect("string offset fits usize");
            let byte_len = usize::try_from(read_test_u64(
                artifact,
                record + wire::strings::RECORD_BYTE_LENGTH as usize,
            ))
            .expect("string byte length fits usize");
            let data = strings
                + wire::strings::HEADER_SIZE as usize
                + count * wire::strings::RECORD_SIZE as usize;
            data + offset..data + offset + byte_len
        }

        fn write_executable_artifact(
            bytes: &[u8],
            name: &str,
        ) -> (RemoveOnDrop, std::path::PathBuf) {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock is after epoch")
                .as_nanos();
            let sequence = ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "archec0-native-corruption-{}-{unique}-{sequence}-{name}.elf",
                std::process::id()
            ));
            let cleanup = RemoveOnDrop(path.clone());
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .expect("temporary artifact opens");
            file.write_all(bytes).expect("artifact writes");
            drop(file);
            let mut permissions = std::fs::metadata(&path)
                .expect("artifact metadata reads")
                .permissions();
            permissions.set_mode(0o700);
            std::fs::set_permissions(&path, permissions).expect("artifact becomes executable");
            (cleanup, path)
        }

        fn execute_artifact(bytes: &[u8], name: &str) -> Output {
            let _execution_guard = crate::lock_linux_test_artifact_execution();
            let (cleanup, path) = write_executable_artifact(bytes, name);
            let output = Command::new(&path).output().expect("native PIE executes");
            drop(cleanup);
            output
        }

        fn assert_rejected_before_mutation(
            baseline: &[u8],
            metadata: EmbeddedMetadata,
            name: &str,
            mutate: impl FnOnce(&mut [u8], EmbeddedMetadata),
        ) {
            let mut artifact = baseline.to_vec();
            mutate(&mut artifact, metadata);
            let output = execute_artifact(&artifact, name);
            assert_eq!(
                output.status.code(),
                Some(1),
                "{name} returned the wrong status; stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                output.stdout.is_empty(),
                "{name} mutated world state or emitted an observation: {}",
                String::from_utf8_lossy(&output.stdout)
            );
            assert!(
                output.stderr.is_empty(),
                "{name} emitted an unexpected diagnostic: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        fn assert_legacy_rejection(
            baseline: &[u8],
            metadata: EmbeddedMetadata,
            name: &str,
            expected_stderr: &[u8],
            mutate: impl FnOnce(&mut [u8], EmbeddedMetadata),
        ) {
            let mut artifact = baseline.to_vec();
            mutate(&mut artifact, metadata);
            let output = execute_artifact(&artifact, name);
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stdout.is_empty());
            assert_eq!(output.stderr, expected_stderr);
        }

        fn mutate_section(
            artifact: &mut [u8],
            metadata: EmbeddedMetadata,
            case: SectionCorruption,
        ) {
            let section = metadata.section_start(artifact, case.kind);
            let offset = match case.mutation {
                SectionMutation::RelativeByte(relative) => {
                    section + usize::try_from(relative).expect("wire offset fits usize")
                }
                SectionMutation::FirstStringByte => {
                    let count = metadata.section_record_count(artifact, case.kind);
                    section
                        + usize::try_from(wire::strings::HEADER_SIZE)
                            .expect("string header fits usize")
                        + count
                            * usize::try_from(wire::strings::RECORD_SIZE)
                                .expect("string record fits usize")
                }
            };
            artifact[offset] = if matches!(case.mutation, SectionMutation::FirstStringByte) {
                0xff
            } else {
                artifact[offset] ^ 0x80
            };
        }

        #[test]
        fn native_rejects_corruption_in_every_v2_section_before_mutation() {
            let (baseline, metadata) = native_artifact();
            assert_eq!(SECTION_CORRUPTIONS.len(), SECTION_KINDS.len());
            for &case in SECTION_CORRUPTIONS {
                assert_rejected_before_mutation(
                    &baseline,
                    metadata,
                    case.name,
                    |artifact, metadata| mutate_section(artifact, metadata, case),
                );
            }
        }

        #[test]
        fn native_rejects_unknown_and_kind_mismatched_schema_flags_before_mutation() {
            let (baseline, metadata) = native_artifact();

            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "unknown_schema_flags",
                |artifact, metadata| {
                    let schemas = metadata.section_start(artifact, wire::section_kind::SCHEMAS);
                    write_test_u64(
                        artifact,
                        schemas + wire::schema::FLAGS as usize,
                        wire::schema::flags::KNOWN_MASK + 1,
                    );
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "tag_kind_flag_mismatch",
                |artifact, metadata| {
                    let schemas = metadata.section_start(artifact, wire::section_kind::SCHEMAS);
                    let count =
                        metadata.section_record_count(artifact, wire::section_kind::SCHEMAS);
                    let stride = wire::schema::RECORD_SIZE as usize;
                    let tag = (0..count)
                        .find(|index| {
                            read_test_u64(
                                artifact,
                                schemas + index * stride + wire::schema::KIND as usize,
                            ) == wire::schema::TAG
                        })
                        .expect("M26 fixture contains a tag schema");
                    write_test_u64(
                        artifact,
                        schemas + tag * stride + wire::schema::FLAGS as usize,
                        0,
                    );
                },
            );
        }

        #[test]
        fn native_rejects_name_order_span_order_and_term_tampering_before_mutation() {
            let (baseline, metadata) = native_artifact();

            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "stale_linked_schema_name",
                |artifact, metadata| {
                    let schemas = metadata.section_start(artifact, wire::section_kind::SCHEMAS);
                    let name = read_test_u64(artifact, schemas + wire::schema::NAME as usize);
                    let range = string_data_range(artifact, metadata, name);
                    assert!(!range.is_empty());
                    let last = range.end - 1;
                    artifact[last] = if artifact[last] == b'X' { b'Y' } else { b'X' };
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "noncanonical_string_order",
                |artifact, metadata| {
                    let range = string_data_range(artifact, metadata, 0);
                    assert!(!range.is_empty());
                    artifact[range.start] = b'~';
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "noncanonical_source_span_order",
                |artifact, metadata| {
                    let spans = metadata.section_start(artifact, wire::section_kind::SOURCE_SPANS);
                    let count =
                        metadata.section_record_count(artifact, wire::section_kind::SOURCE_SPANS);
                    assert!(count >= 2);
                    let stride = wire::source_span::RECORD_SIZE as usize;
                    let first = artifact[spans..spans + stride].to_vec();
                    artifact[spans + stride..spans + 2 * stride].copy_from_slice(&first);
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "linked_term_access_tamper",
                |artifact, metadata| {
                    let terms = metadata.section_start(artifact, wire::section_kind::TERMS);
                    let access = read_test_u64(artifact, terms + wire::term::ACCESS as usize);
                    let replacement = if access == wire::term::READ {
                        wire::term::EXCLUDE
                    } else {
                        wire::term::READ
                    };
                    write_test_u64(artifact, terms + wire::term::ACCESS as usize, replacement);
                },
            );
        }

        #[test]
        fn native_rejects_unused_strings_and_spans_before_mutation() {
            let (baseline, metadata) = native_artifact();

            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "unused_source_file_string",
                |artifact, metadata| {
                    let world = metadata.section_start(artifact, wire::section_kind::WORLD);
                    let replacement = read_test_u64(artifact, world + wire::world::NAME as usize);
                    let spans = metadata.section_start(artifact, wire::section_kind::SOURCE_SPANS);
                    let count =
                        metadata.section_record_count(artifact, wire::section_kind::SOURCE_SPANS);
                    let stride = wire::source_span::RECORD_SIZE as usize;
                    let original =
                        read_test_u64(artifact, spans + wire::source_span::FILE_NAME as usize);
                    assert_ne!(original, replacement);
                    for index in 0..count {
                        write_test_u64(
                            artifact,
                            spans + index * stride + wire::source_span::FILE_NAME as usize,
                            replacement,
                        );
                    }
                },
            );

            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "unused_source_span",
                |artifact, metadata| {
                    let span_count =
                        metadata.section_record_count(artifact, wire::section_kind::SOURCE_SPANS);
                    let mut usage = vec![0usize; span_count];
                    let mut direct_fields = vec![Vec::<usize>::new(); span_count];
                    for (section_kind, stride, field) in [
                        (
                            wire::section_kind::WORLD,
                            wire::world::RECORD_SIZE,
                            wire::world::SOURCE_SPAN,
                        ),
                        (
                            wire::section_kind::SCHEMAS,
                            wire::schema::RECORD_SIZE,
                            wire::schema::SOURCE_SPAN,
                        ),
                        (
                            wire::section_kind::FIELDS,
                            wire::field::RECORD_SIZE,
                            wire::field::SOURCE_SPAN,
                        ),
                        (
                            wire::section_kind::SYSTEMS,
                            wire::system::RECORD_SIZE,
                            wire::system::SOURCE_SPAN,
                        ),
                        (
                            wire::section_kind::PARAMETERS,
                            wire::parameter::RECORD_SIZE,
                            wire::parameter::SOURCE_SPAN,
                        ),
                        (
                            wire::section_kind::QUERIES,
                            wire::query::RECORD_SIZE,
                            wire::query::SOURCE_SPAN,
                        ),
                        (
                            wire::section_kind::TERMS,
                            wire::term::RECORD_SIZE,
                            wire::term::SOURCE_SPAN,
                        ),
                        (
                            wire::section_kind::SCHEDULES,
                            wire::schedule::RECORD_SIZE,
                            wire::schedule::SOURCE_SPAN,
                        ),
                        (
                            wire::section_kind::SCHEDULE_ITEMS,
                            wire::schedule_item::RECORD_SIZE,
                            wire::schedule_item::SOURCE_SPAN,
                        ),
                        (
                            wire::section_kind::STARTUP_OPERATIONS,
                            wire::startup_operation::RECORD_SIZE,
                            wire::startup_operation::SOURCE_SPAN,
                        ),
                        (
                            wire::section_kind::FUNCTION_LINKS,
                            wire::function_link::RECORD_SIZE,
                            wire::function_link::SOURCE_SPAN,
                        ),
                    ] {
                        let section = metadata.section_start(artifact, section_kind);
                        let count = metadata.section_record_count(artifact, section_kind);
                        let stride = stride as usize;
                        for index in 0..count {
                            let offset = section + index * stride + field as usize;
                            let span = read_test_u64(artifact, offset);
                            if span != wire::NONE_REFERENCE {
                                let span = usize::try_from(span).expect("span index fits usize");
                                usage[span] += 1;
                                if section_kind != wire::section_kind::FUNCTION_LINKS {
                                    direct_fields[span].push(offset);
                                }
                            }
                        }
                    }
                    let links =
                        metadata.section_start(artifact, wire::section_kind::FUNCTION_LINKS);
                    let link_count =
                        metadata.section_record_count(artifact, wire::section_kind::FUNCTION_LINKS);
                    let link_stride = wire::function_link::RECORD_SIZE as usize;
                    for index in 0..link_count {
                        let link = links + index * link_stride;
                        let first = read_test_u64(
                            artifact,
                            link + wire::function_link::FIRST_BODY_SPAN as usize,
                        );
                        let count = read_test_u64(
                            artifact,
                            link + wire::function_link::BODY_SPAN_COUNT as usize,
                        );
                        if first != wire::NONE_REFERENCE {
                            for span in first..first + count {
                                usage[usize::try_from(span).expect("span index fits usize")] += 1;
                            }
                        }
                    }
                    let offset = (0..span_count)
                        .find_map(|span| {
                            (usage[span] == 1 && direct_fields[span].len() == 1)
                                .then_some(direct_fields[span][0])
                        })
                        .expect("fixture has a uniquely direct-referenced source span");
                    write_test_u64(artifact, offset, wire::NONE_REFERENCE);
                },
            );
        }

        #[test]
        fn native_rejects_invalid_function_body_span_slices_before_mutation() {
            let (baseline, metadata) = native_artifact();
            let links = metadata.section_start(&baseline, wire::section_kind::FUNCTION_LINKS);
            let count =
                metadata.section_record_count(&baseline, wire::section_kind::FUNCTION_LINKS);
            let stride = wire::function_link::RECORD_SIZE as usize;
            let first_link = (0..count)
                .find(|index| {
                    read_test_u64(
                        &baseline,
                        links + index * stride + wire::function_link::BODY_SPAN_COUNT as usize,
                    ) > 0
                })
                .expect("fixture has a function body-span slice");
            let first_record = links + first_link * stride;
            let first_owner = read_test_u64(
                &baseline,
                first_record + wire::function_link::SOURCE_SPAN as usize,
            );
            let first_body = read_test_u64(
                &baseline,
                first_record + wire::function_link::FIRST_BODY_SPAN as usize,
            );
            let first_count = read_test_u64(
                &baseline,
                first_record + wire::function_link::BODY_SPAN_COUNT as usize,
            );

            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "present_zero_count_body_slice",
                move |artifact, metadata| {
                    let links =
                        metadata.section_start(artifact, wire::section_kind::FUNCTION_LINKS);
                    write_test_u64(
                        artifact,
                        links + first_link * stride + wire::function_link::BODY_SPAN_COUNT as usize,
                        0,
                    );
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "body_slice_without_owner",
                move |artifact, metadata| {
                    let links =
                        metadata.section_start(artifact, wire::section_kind::FUNCTION_LINKS);
                    write_test_u64(
                        artifact,
                        links + first_link * stride + wire::function_link::SOURCE_SPAN as usize,
                        wire::NONE_REFERENCE,
                    );
                },
            );

            let other_link = (0..count)
                .rev()
                .find(|index| {
                    *index != first_link
                        && read_test_u64(
                            &baseline,
                            links + index * stride + wire::function_link::BODY_SPAN_COUNT as usize,
                        ) > 0
                })
                .expect("fixture has a second function body-span slice");
            let other_record = links + other_link * stride;
            let other_body = read_test_u64(
                &baseline,
                other_record + wire::function_link::FIRST_BODY_SPAN as usize,
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "body_span_outside_owner",
                move |artifact, metadata| {
                    let links =
                        metadata.section_start(artifact, wire::section_kind::FUNCTION_LINKS);
                    write_test_u64(
                        artifact,
                        links + first_link * stride + wire::function_link::FIRST_BODY_SPAN as usize,
                        other_body,
                    );
                    write_test_u64(
                        artifact,
                        links + first_link * stride + wire::function_link::BODY_SPAN_COUNT as usize,
                        1,
                    );
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "overlapping_function_body_slices",
                move |artifact, metadata| {
                    let links =
                        metadata.section_start(artifact, wire::section_kind::FUNCTION_LINKS);
                    let other = links + other_link * stride;
                    write_test_u64(
                        artifact,
                        other + wire::function_link::SOURCE_SPAN as usize,
                        first_owner,
                    );
                    write_test_u64(
                        artifact,
                        other + wire::function_link::FIRST_BODY_SPAN as usize,
                        first_body,
                    );
                    write_test_u64(
                        artifact,
                        other + wire::function_link::BODY_SPAN_COUNT as usize,
                        first_count,
                    );
                },
            );
        }

        #[test]
        fn native_rejects_envelope_link_layout_and_flow_corruption_before_mutation() {
            let (baseline, metadata) = native_artifact();

            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "total_length_mismatch",
                |artifact, metadata| {
                    let total = read_test_u64(artifact, metadata.start + 24);
                    write_test_u64(artifact, metadata.start + 24, total - 1);
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "section_offset_overflow",
                |artifact, metadata| {
                    let row = metadata.section_row(artifact, wire::section_kind::FIELDS);
                    write_test_u64(artifact, row + 16, u64::MAX);
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "section_overlap",
                |artifact, metadata| {
                    let strings = metadata.section_row(artifact, wire::section_kind::STRINGS);
                    let world = metadata.section_row(artifact, wire::section_kind::WORLD);
                    let first_offset = read_test_u64(artifact, strings + 16);
                    write_test_u64(artifact, world + 16, first_offset);
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "invalid_section_alignment",
                |artifact, metadata| {
                    let row = metadata.section_row(artifact, wire::section_kind::SCHEMAS);
                    write_test_u64(artifact, row + 48, 3);
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "truncated_record_section",
                |artifact, metadata| {
                    let row = metadata.section_row(artifact, wire::section_kind::FIELDS);
                    let byte_len = read_test_u64(artifact, row + 24);
                    write_test_u64(artifact, row + 24, byte_len - 1);
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "duplicate_section_kind",
                |artifact, metadata| {
                    let first = metadata.directory_row(0);
                    let second = metadata.directory_row(1);
                    let first_kind = read_test_u64(artifact, first);
                    write_test_u64(artifact, second, first_kind);
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "bad_dense_schema_index",
                |artifact, metadata| {
                    let field = metadata.section_start(artifact, wire::section_kind::FIELDS);
                    write_test_u64(artifact, field + wire::field::SCHEMA as usize, u64::MAX);
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "duplicate_schema_id",
                |artifact, metadata| {
                    let schemas = metadata.section_start(artifact, wire::section_kind::SCHEMAS);
                    let stride = wire::schema::RECORD_SIZE as usize;
                    let first_id: [u8; 16] = artifact[schemas..schemas + 16]
                        .try_into()
                        .expect("schema identifier is in range");
                    artifact[schemas + stride..schemas + stride + 16].copy_from_slice(&first_id);
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "bad_payload_length",
                |artifact, metadata| {
                    let payloads = metadata.section_start(artifact, wire::section_kind::PAYLOADS);
                    let record = payloads + wire::payload::HEADER_SIZE as usize;
                    let length = read_test_u64(artifact, record + wire::payload::LENGTH as usize);
                    write_test_u64(
                        artifact,
                        record + wire::payload::LENGTH as usize,
                        length + 1,
                    );
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "invalid_startup_resource_flow",
                |artifact, metadata| {
                    let operations =
                        metadata.section_start(artifact, wire::section_kind::STARTUP_OPERATIONS);
                    let count = metadata
                        .section_record_count(artifact, wire::section_kind::STARTUP_OPERATIONS);
                    let stride = wire::startup_operation::RECORD_SIZE as usize;
                    let schedule = (0..count)
                        .find(|index| {
                            read_test_u64(artifact, operations + index * stride)
                                == wire::startup_operation::RUN_SCHEDULE
                        })
                        .expect("fixture runs a schedule");
                    for byte in 0..stride {
                        artifact.swap(operations + byte, operations + schedule * stride + byte);
                    }
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "bad_function_offset",
                |artifact, metadata| {
                    let links =
                        metadata.section_start(artifact, wire::section_kind::FUNCTION_LINKS);
                    write_test_u64(
                        artifact,
                        links + wire::function_link::CODE_OFFSET as usize,
                        u64::MAX,
                    );
                },
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "function_body_hash_mismatch",
                |artifact, metadata| {
                    let links =
                        metadata.section_start(artifact, wire::section_kind::FUNCTION_LINKS);
                    artifact[links + wire::function_link::BODY_HASH as usize] ^= 0x80;
                },
            );
        }

        #[test]
        fn native_rejects_duplicate_and_unused_payloads_before_mutation() {
            let source = r#"world PayloadUse
component Value { value: i32 }
startup {
    spawn { Value { value: 11 } }
    spawn { Value { value: 22 } }
    exit 0
}
"#;
            let (baseline, metadata) = native_artifact_for_source(source, "payload-use.arc");
            let operations =
                metadata.section_start(&baseline, wire::section_kind::STARTUP_OPERATIONS);
            let operation_count =
                metadata.section_record_count(&baseline, wire::section_kind::STARTUP_OPERATIONS);
            let stride = wire::startup_operation::RECORD_SIZE as usize;
            let spawn_operations = (0..operation_count)
                .filter(|index| {
                    read_test_u64(&baseline, operations + index * stride)
                        == wire::startup_operation::SPAWN
                })
                .collect::<Vec<_>>();
            assert_eq!(spawn_operations.len(), 2);
            let first_payload = read_test_u64(
                &baseline,
                operations + spawn_operations[0] * stride + wire::startup_operation::FIRST as usize,
            );
            let second_payload = read_test_u64(
                &baseline,
                operations + spawn_operations[1] * stride + wire::startup_operation::FIRST as usize,
            );
            assert_ne!(first_payload, second_payload);

            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "duplicate_unused_payload",
                |artifact, metadata| {
                    let operations =
                        metadata.section_start(artifact, wire::section_kind::STARTUP_OPERATIONS);
                    write_test_u64(
                        artifact,
                        operations
                            + spawn_operations[1] * stride
                            + wire::startup_operation::FIRST as usize,
                        first_payload,
                    );
                },
            );
        }

        #[test]
        fn native_rejects_trap_coordinate_tamper_before_mutation() {
            let (baseline, metadata) = native_artifact_for_source(
                include_str!("../../../examples/m26_trap.arc"),
                "m26_trap.arc",
            );
            assert_rejected_before_mutation(
                &baseline,
                metadata,
                "trap_coordinate_tamper",
                |artifact, metadata| {
                    let spans = metadata.section_start(artifact, wire::section_kind::SOURCE_SPANS);
                    let count =
                        metadata.section_record_count(artifact, wire::section_kind::SOURCE_SPANS);
                    let stride = wire::source_span::RECORD_SIZE as usize;
                    for index in 0..count {
                        let record = spans + index * stride;
                        for field in [wire::source_span::START_LINE, wire::source_span::END_LINE] {
                            let offset = record + field as usize;
                            let value = read_test_u64(artifact, offset);
                            write_test_u64(artifact, offset, value + 1000);
                        }
                    }
                },
            );
        }

        #[test]
        fn native_accepts_coherently_reordered_payload_records_and_references() {
            let source = r#"world PayloadOrder
resource First { value: i32 }
resource Second { value: i32 }
startup {
    resource First { value: 11 }
    resource Second { value: 22 }
    exit 47
}
"#;
            let core = verified(source);
            let plan = crate::aot_v2::plan_native(&core).expect("native plan builds");
            let code_range = plan.native_code_layout().code_range;
            let mut package =
                build_execution_package(&core, "payload-order.arc", plan.native_code_layout())
                    .expect("v2 package builds");
            assert_eq!(package.payloads.len(), 2);
            package.payloads.swap(0, 1);
            for operation in &mut package.startup_operations {
                if let archec0::execution_package_v2::StartupOperationKind::ResourcePayload {
                    payload,
                    ..
                } = &mut operation.kind
                {
                    *payload = archec0::execution_package_v2::PayloadRef::new(1 - payload.index());
                }
            }

            let mut reference_stdout = Vec::new();
            let mut reference_stderr = Vec::new();
            let reference = crate::reference_executor_v2::execute_decoded(
                &core,
                package.clone(),
                Some(code_range),
                &mut reference_stdout,
                &mut reference_stderr,
            )
            .expect("reordered package executes through direct Core reference");
            let image = crate::aot_v2::finalize_native(plan, &core, &package)
                .expect("reordered package finalizes through native AOT");
            let mut artifact = std::io::Cursor::new(Vec::new());
            image
                .write_static_pie(&mut artifact, 0)
                .expect("reordered native PIE writes");
            let output = execute_artifact(artifact.get_ref(), "payload_reorder");
            assert_eq!(output.status.code(), Some(reference.process_status()));
            assert_eq!(output.stdout, reference_stdout);
            assert_eq!(output.stderr, reference_stderr);
        }

        #[test]
        fn native_observation_write_to_closed_pipe_exits_one_without_sigpipe() {
            let (artifact, _) = native_artifact();
            let _execution_guard = crate::lock_linux_test_artifact_execution();
            let (cleanup, path) = write_executable_artifact(&artifact, "closed_pipe");
            let (reader, writer) = UnixStream::pair().expect("Unix stream pair opens");
            drop(reader);
            let stdout: OwnedFd = writer.into();
            let output = Command::new(&path)
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::piped())
                .output()
                .expect("native PIE executes with a closed output pipe");
            drop(cleanup);

            assert_eq!(
                output.status.code(),
                Some(1),
                "closed-pipe observation must be an ordinary I/O failure, not SIGPIPE"
            );
            assert!(output.stderr.is_empty());
        }

        #[test]
        fn native_observation_write_to_dev_full_exits_one() {
            let (artifact, _) = native_artifact();
            let _execution_guard = crate::lock_linux_test_artifact_execution();
            let (cleanup, path) = write_executable_artifact(&artifact, "dev_full");
            let full = OpenOptions::new()
                .write(true)
                .open("/dev/full")
                .expect("/dev/full opens");
            let output = Command::new(&path)
                .stdout(Stdio::from(full))
                .stderr(Stdio::piped())
                .output()
                .expect("native PIE executes with a full output device");
            drop(cleanup);

            assert_eq!(output.status.code(), Some(1));
            assert!(output.stderr.is_empty());
        }

        #[test]
        fn native_hard_rejects_v1_and_archecmp_with_rebuild_diagnostics() {
            let (baseline, metadata) = native_artifact();
            assert_legacy_rejection(
                &baseline,
                metadata,
                "archeecs_v1",
                VERSION_ONE_DIAGNOSTIC,
                |artifact, metadata| {
                    artifact[metadata.start + 8..metadata.start + 12]
                        .copy_from_slice(&1_u32.to_le_bytes());
                },
            );
            assert_legacy_rejection(
                &baseline,
                metadata,
                "archecmp",
                ARCHECMP_DIAGNOSTIC,
                |artifact, metadata| {
                    artifact[metadata.start..metadata.start + 8].copy_from_slice(b"ARCHECMP");
                },
            );
        }
    }
}
