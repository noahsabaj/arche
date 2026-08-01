use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::io::{self, Seek, SeekFrom, Write};

use archec0::execution_package_v2::{
    write_package_with_code_range, CodeImageRange, ExecutionPackage,
};
use archec0::ids_v2::{DeclId, PrimitiveType, SchemaId, SchemaKind};

use crate::core::{
    BlockId, CoreBinaryOp, CoreComparisonOp, CoreComponentKind, CoreFunction, CoreInstruction,
    CoreQueryAccess, CoreSystem, CoreSystemBinaryOp, CoreSystemExpression, CoreSystemPlace,
    CoreSystemStatement, CoreSystemUnaryOp, CoreTerminator, CoreType, CoreUnaryOp, LocalId,
    ValueId,
};
use crate::core_verify::VerifiedExecutableCore;
use crate::elf64::{self, MetadataAnchorRelocation, StaticPieLayout, StaticPieRequest};
use crate::execution_package_build::{
    canonical_core_ids, validate_execution_package_link, ExecutionPackageBuildError,
    NativeCodeLayout, NativeFunctionLayout, NativeFunctionTarget,
};
use crate::identifier::Identifier;
use crate::native_runtime_v2::{
    emit_runtime, finalize_runtime_data, NativeRuntimeLabels, NativeRuntimePlan,
};

const TEXT_IMAGE_OFFSET: u64 = 0x1000;
const DATA_SEGMENT_ALIGNMENT: u64 = 0x1000;
const U64_NONE: u64 = u64::MAX;

#[derive(Debug)]
pub enum AotV2Error {
    InvalidCore(String),
    Link(ExecutionPackageBuildError),
    ArithmeticOverflow(&'static str),
    AddressSpaceOverflow(&'static str),
    Allocation(&'static str),
    InvalidNativePlan(String),
    Package(archec0::execution_package_v2::ExecutionPackageV2Error),
    Io(io::Error),
}

impl fmt::Display for AotV2Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCore(message) | Self::InvalidNativePlan(message) => {
                formatter.write_str(message)
            }
            Self::Link(error) => error.fmt(formatter),
            Self::ArithmeticOverflow(context) => {
                write!(
                    formatter,
                    "u64 arithmetic overflow while planning native {context}"
                )
            }
            Self::AddressSpaceOverflow(context) => {
                write!(
                    formatter,
                    "native {context} does not fit the host address space"
                )
            }
            Self::Allocation(context) => {
                write!(
                    formatter,
                    "allocation failed while planning native {context}"
                )
            }
            Self::Package(error) => error.fmt(formatter),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl Error for AotV2Error {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Link(error) => Some(error),
            Self::Package(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::InvalidCore(_)
            | Self::ArithmeticOverflow(_)
            | Self::AddressSpaceOverflow(_)
            | Self::Allocation(_)
            | Self::InvalidNativePlan(_) => None,
        }
    }
}

impl From<ExecutionPackageBuildError> for AotV2Error {
    fn from(error: ExecutionPackageBuildError) -> Self {
        Self::Link(error)
    }
}

impl From<archec0::execution_package_v2::ExecutionPackageV2Error> for AotV2Error {
    fn from(error: archec0::execution_package_v2::ExecutionPackageV2Error) -> Self {
        Self::Package(error)
    }
}

impl From<io::Error> for AotV2Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Label(u64);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Condition {
    Equal,
    NotEqual,
    Below,
    BelowEqual,
    Above,
    AboveEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Parity,
    NotParity,
    Zero,
    NotZero,
}

impl Condition {
    fn short_opcode(self) -> u8 {
        match self {
            Self::Equal | Self::Zero => 0x74,
            Self::NotEqual | Self::NotZero => 0x75,
            Self::Below => 0x72,
            Self::BelowEqual => 0x76,
            Self::Above => 0x77,
            Self::AboveEqual => 0x73,
            Self::Less => 0x7c,
            Self::LessEqual => 0x7e,
            Self::Greater => 0x7f,
            Self::GreaterEqual => 0x7d,
            Self::Parity => 0x7a,
            Self::NotParity => 0x7b,
        }
    }

    fn inverse(self) -> Self {
        match self {
            Self::Equal => Self::NotEqual,
            Self::NotEqual => Self::Equal,
            Self::Below => Self::AboveEqual,
            Self::BelowEqual => Self::Above,
            Self::Above => Self::BelowEqual,
            Self::AboveEqual => Self::Below,
            Self::Less => Self::GreaterEqual,
            Self::LessEqual => Self::Greater,
            Self::Greater => Self::LessEqual,
            Self::GreaterEqual => Self::Less,
            Self::Parity => Self::NotParity,
            Self::NotParity => Self::Parity,
            Self::Zero => Self::NotZero,
            Self::NotZero => Self::Zero,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Register {
    Rax,
    Rcx,
    Rdx,
    Rbx,
    Rsp,
    Rbp,
    Rsi,
    Rdi,
    R8,
    R9,
    R10,
    R11,
    R12,
    R13,
    R14,
    R15,
}

impl Register {
    fn code(self) -> u8 {
        match self {
            Self::Rax | Self::R8 => 0,
            Self::Rcx | Self::R9 => 1,
            Self::Rdx | Self::R10 => 2,
            Self::Rbx | Self::R11 => 3,
            Self::Rsp | Self::R12 => 4,
            Self::Rbp | Self::R13 => 5,
            Self::Rsi | Self::R14 => 6,
            Self::Rdi | Self::R15 => 7,
        }
    }

    fn extended(self) -> bool {
        matches!(
            self,
            Self::R8
                | Self::R9
                | Self::R10
                | Self::R11
                | Self::R12
                | Self::R13
                | Self::R14
                | Self::R15
        )
    }
}

#[derive(Clone, Copy, Debug)]
struct LabelFixup {
    immediate_offset: usize,
    anchor_offset: u64,
    target: Label,
}

#[derive(Clone, Copy, Debug)]
struct DataVaddrPatch {
    immediate_offset: usize,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Assembler {
    bytes: Vec<u8>,
    labels: Vec<Option<u64>>,
    label_fixups: Vec<LabelFixup>,
    metadata_relocations: Vec<MetadataAnchorRelocation>,
    data_vaddr_patches: Vec<DataVaddrPatch>,
}

impl Assembler {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn position(&self) -> Result<u64, AotV2Error> {
        u64::try_from(self.bytes.len())
            .map_err(|_| AotV2Error::AddressSpaceOverflow("text position"))
    }

    pub(crate) fn new_label(&mut self) -> Result<Label, AotV2Error> {
        let index = u64::try_from(self.labels.len())
            .map_err(|_| AotV2Error::AddressSpaceOverflow("label table"))?;
        self.labels
            .try_reserve(1)
            .map_err(|_| AotV2Error::Allocation("label table"))?;
        self.labels.push(None);
        Ok(Label(index))
    }

    pub(crate) fn bind(&mut self, label: Label) -> Result<(), AotV2Error> {
        let position = self.position()?;
        let slot = self
            .labels
            .get_mut(
                usize::try_from(label.0)
                    .map_err(|_| AotV2Error::AddressSpaceOverflow("label index"))?,
            )
            .ok_or_else(|| invalid_native("attempted to bind an unknown native label"))?;
        if slot.replace(position).is_some() {
            return Err(invalid_native("attempted to bind a native label twice"));
        }
        Ok(())
    }

    pub(crate) fn emit(&mut self, bytes: &[u8]) -> Result<(), AotV2Error> {
        self.bytes
            .try_reserve(bytes.len())
            .map_err(|_| AotV2Error::Allocation("text bytes"))?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    pub(crate) fn emit_u32(&mut self, value: u32) -> Result<(), AotV2Error> {
        self.emit(&value.to_le_bytes())
    }

    pub(crate) fn emit_u64(&mut self, value: u64) -> Result<(), AotV2Error> {
        self.emit(&value.to_le_bytes())
    }

    pub(crate) fn mov_imm64(&mut self, register: Register, value: u64) -> Result<(), AotV2Error> {
        self.emit(&[if register.extended() { 0x49 } else { 0x48 }])?;
        self.emit(&[0xb8 + register.code()])?;
        self.emit_u64(value)
    }

    pub(crate) fn mov_reg64(
        &mut self,
        destination: Register,
        source: Register,
    ) -> Result<(), AotV2Error> {
        let rex = 0x48 | u8::from(source.extended()) << 2 | u8::from(destination.extended());
        self.emit(&[rex, 0x89, 0xc0 | source.code() << 3 | destination.code()])
    }

    pub(crate) fn add_reg64(
        &mut self,
        destination: Register,
        source: Register,
    ) -> Result<(), AotV2Error> {
        let rex = 0x48 | u8::from(source.extended()) << 2 | u8::from(destination.extended());
        self.emit(&[rex, 0x01, 0xc0 | source.code() << 3 | destination.code()])
    }

    fn adjust_reg64_imm8(
        &mut self,
        register: Register,
        immediate: u8,
        subtract: bool,
    ) -> Result<(), AotV2Error> {
        let rex = 0x48 | u8::from(register.extended());
        let operation = if subtract { 0xe8 } else { 0xc0 };
        self.emit(&[rex, 0x83, operation | register.code(), immediate])
    }

    pub(crate) fn data_address(
        &mut self,
        destination: Register,
        offset: u64,
    ) -> Result<(), AotV2Error> {
        self.mov_imm64(destination, offset)?;
        self.add_reg64(destination, Register::R14)
    }

    pub(crate) fn metadata_address(
        &mut self,
        destination: Register,
        offset: u64,
    ) -> Result<(), AotV2Error> {
        self.mov_imm64(destination, offset)?;
        self.add_reg64(destination, Register::R13)
    }

    pub(crate) fn far_jump(&mut self, target: Label) -> Result<(), AotV2Error> {
        self.emit_far_transfer(target, false)
    }

    pub(crate) fn far_call(&mut self, target: Label) -> Result<(), AotV2Error> {
        self.emit_far_transfer(target, true)
    }

    fn emit_far_transfer(&mut self, target: Label, call: bool) -> Result<(), AotV2Error> {
        let start = self.position()?;
        // lea r11, [rip]
        self.emit(&[0x4c, 0x8d, 0x1d, 0, 0, 0, 0])?;
        // movabs r10, target - anchor
        self.emit(&[0x49, 0xba])?;
        let immediate_offset = self.bytes.len();
        self.emit_u64(0)?;
        // add r11, r10; call/jmp r11
        self.emit(&[0x4d, 0x01, 0xd3, 0x41, 0xff, if call { 0xd3 } else { 0xe3 }])?;
        self.label_fixups
            .try_reserve(1)
            .map_err(|_| AotV2Error::Allocation("far-transfer fixups"))?;
        self.label_fixups.push(LabelFixup {
            immediate_offset,
            anchor_offset: start
                .checked_add(7)
                .ok_or(AotV2Error::ArithmeticOverflow("far-transfer anchor"))?,
            target,
        });
        Ok(())
    }

    pub(crate) fn far_jcc(
        &mut self,
        condition: Condition,
        target: Label,
    ) -> Result<(), AotV2Error> {
        // The inverted local branch skips one fixed 23-byte far jump.
        self.emit(&[condition.inverse().short_opcode(), 23])?;
        self.far_jump(target)
    }

    pub(crate) fn emit_metadata_anchor(&mut self) -> Result<(), AotV2Error> {
        self.bytes
            .try_reserve(20)
            .map_err(|_| AotV2Error::Allocation("metadata anchor text"))?;
        let relocation = elf64::emit_metadata_anchor_stub(&mut self.bytes);
        self.metadata_relocations
            .try_reserve(1)
            .map_err(|_| AotV2Error::Allocation("metadata relocations"))?;
        self.metadata_relocations.push(relocation);
        // The ELF helper materializes metadata in rsi. Keep it in r13.
        self.mov_reg64(Register::R13, Register::Rsi)
    }

    pub(crate) fn emit_data_anchor(&mut self) -> Result<(), AotV2Error> {
        self.emit(&[0x49, 0xbe])?; // movabs r14, data image vaddr
        let immediate_offset = self.bytes.len();
        self.emit_u64(0)?;
        self.add_reg64(Register::R14, Register::R15)?;
        self.data_vaddr_patches
            .try_reserve(1)
            .map_err(|_| AotV2Error::Allocation("data relocations"))?;
        self.data_vaddr_patches
            .push(DataVaddrPatch { immediate_offset });
        Ok(())
    }

    fn finish(mut self) -> Result<AssembledText, AotV2Error> {
        for fixup in &self.label_fixups {
            let target = self
                .labels
                .get(
                    usize::try_from(fixup.target.0)
                        .map_err(|_| AotV2Error::AddressSpaceOverflow("label index"))?,
                )
                .and_then(|value| *value)
                .ok_or_else(|| invalid_native("native text contains an unbound label"))?;
            let delta = i128::from(target) - i128::from(fixup.anchor_offset);
            let delta = i64::try_from(delta)
                .map_err(|_| invalid_native("native far-transfer delta exceeds signed 64-bit"))?;
            let end = fixup
                .immediate_offset
                .checked_add(8)
                .ok_or(AotV2Error::ArithmeticOverflow("far-transfer patch range"))?;
            let destination = self
                .bytes
                .get_mut(fixup.immediate_offset..end)
                .ok_or_else(|| invalid_native("native far-transfer patch is outside text"))?;
            destination.copy_from_slice(&delta.to_le_bytes());
        }
        if self.labels.iter().any(Option::is_none) {
            return Err(invalid_native("native text contains an unbound label"));
        }
        Ok(AssembledText {
            bytes: self.bytes,
            metadata_relocations: self.metadata_relocations,
            data_vaddr_patches: self.data_vaddr_patches,
        })
    }
}

#[derive(Clone, Debug)]
struct AssembledText {
    bytes: Vec<u8>,
    metadata_relocations: Vec<MetadataAnchorRelocation>,
    data_vaddr_patches: Vec<DataVaddrPatch>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum NativeTrapPoint {
    Startup {
        block: BlockId,
        instruction_index: u64,
    },
    System {
        system_id: u64,
        expression_ordinal: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeFieldStorage {
    pub name: Identifier,
    pub primitive: PrimitiveType,
    pub byte_offset: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CanonicalSchemaStorage {
    pub legacy_id: u64,
    pub id: SchemaId,
    pub dense_index: u64,
    pub kind: SchemaKind,
    pub byte_size: u64,
    pub alignment: u64,
    pub fields: Vec<NativeFieldStorage>,
    pub resource_initialized_offset: Option<u64>,
    pub resource_payload_offset: Option<u64>,
    pub row_cell_offset: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorldStoragePlan {
    pub row_count_offset: u64,
    pub next_spawn_ordinal_offset: u64,
    pub rows_base: u64,
    pub row_stride: u64,
    pub row_active_offset: u64,
    pub row_spawn_ordinal_offset: u64,
    pub row_membership_offset: u64,
    pub row_membership_bytes: u64,
    pub max_rows: u64,
    pub schemas: Vec<CanonicalSchemaStorage>,
}

impl WorldStoragePlan {
    pub(crate) fn schema_by_legacy(&self, id: u64) -> Option<&CanonicalSchemaStorage> {
        self.schemas.iter().find(|schema| schema.legacy_id == id)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DataAllocator {
    cursor: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DataChunk {
    pub offset: u64,
    pub bytes: Vec<u8>,
}

impl DataAllocator {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn allocate(
        &mut self,
        byte_len: u64,
        alignment: u64,
        context: &'static str,
    ) -> Result<u64, AotV2Error> {
        self.cursor = align_u64(self.cursor, alignment, context)?;
        let offset = self.cursor;
        self.cursor = self
            .cursor
            .checked_add(byte_len)
            .ok_or(AotV2Error::ArithmeticOverflow(context))?;
        Ok(offset)
    }

    pub(crate) fn byte_len(&self) -> u64 {
        self.cursor
    }
}

#[derive(Clone, Debug)]
pub struct AotPlan {
    text: Vec<u8>,
    entry_text_offset: u64,
    metadata_relocations: Vec<MetadataAnchorRelocation>,
    data_vaddr_patches: Vec<DataVaddrPatch>,
    data_memory_byte_len: u64,
    native_layout: NativeCodeLayout,
    runtime: NativeRuntimePlan,
    world: WorldStoragePlan,
}

impl AotPlan {
    pub fn native_code_layout(&self) -> &NativeCodeLayout {
        &self.native_layout
    }
}

#[derive(Clone, Debug)]
pub struct AotImage {
    text: Vec<u8>,
    data_chunks: Vec<DataChunk>,
    data_file_byte_len: u64,
    package: ExecutionPackage,
    code_range: CodeImageRange,
    metadata_byte_len: u64,
    entry_text_offset: u64,
    data_memory_byte_len: u64,
    metadata_relocations: Vec<MetadataAnchorRelocation>,
}

impl AotImage {
    pub fn write_static_pie(
        &self,
        output: &mut (impl Write + Seek),
        minimum_metadata_offset: u64,
    ) -> Result<StaticPieLayout, AotV2Error> {
        let request = StaticPieRequest {
            entry_text_offset: self.entry_text_offset,
            text_file_byte_len: as_u64(self.text.len(), "text byte length")?,
            data_file_byte_len: self.data_file_byte_len,
            data_memory_byte_len: self.data_memory_byte_len,
            metadata_file_byte_len: self.metadata_byte_len,
            minimum_metadata_offset,
            metadata_anchor_relocations: &self.metadata_relocations,
        };
        let plan = elf64::plan_static_pie(request)?;
        let package = &self.package;
        let code_range = self.code_range;
        let expected_metadata_byte_len = self.metadata_byte_len;
        Ok(elf64::write_static_pie(
            output,
            &plan,
            |segment| segment.write_all(&self.text),
            |segment| write_data_chunks(segment, &self.data_chunks),
            |segment| {
                let mut output = DynWriteSeekAdapter(segment);
                let written = write_package_with_code_range(&mut output, package, code_range)
                    .map_err(|error| {
                        io::Error::new(io::ErrorKind::InvalidData, error.to_string())
                    })?;
                if written != expected_metadata_byte_len {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "streamed ARCHEECS metadata length changed after planning",
                    ));
                }
                Ok(())
            },
        )?)
    }
}

#[derive(Default)]
struct CountingWriteSeek {
    position: u64,
    byte_len: u64,
}

impl CountingWriteSeek {
    fn byte_len(&self) -> u64 {
        self.byte_len
    }
}

impl Write for CountingWriteSeek {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let byte_len = u64::try_from(bytes.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "metadata write length exceeds u64",
            )
        })?;
        self.position = self.position.checked_add(byte_len).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "metadata write range overflows u64",
            )
        })?;
        self.byte_len = self.byte_len.max(self.position);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for CountingWriteSeek {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let next = match position {
            SeekFrom::Start(position) => i128::from(position),
            SeekFrom::End(delta) => i128::from(self.byte_len) + i128::from(delta),
            SeekFrom::Current(delta) => i128::from(self.position) + i128::from(delta),
        };
        self.position = u64::try_from(next).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "metadata seek lies outside the u64 file range",
            )
        })?;
        Ok(self.position)
    }
}

struct DynWriteSeekAdapter<'a>(&'a mut dyn elf64::WriteSeek);

impl Write for DynWriteSeekAdapter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Seek for DynWriteSeekAdapter<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.0.seek(position)
    }
}

pub fn plan_native(core: &VerifiedExecutableCore) -> Result<AotPlan, AotV2Error> {
    let ids = canonical_core_ids(core)?;
    let mut allocator = DataAllocator::new();
    let world = plan_world_storage(core, &ids, &mut allocator)?;
    let trap_points = collect_trap_points(core)?;
    let runtime = NativeRuntimePlan::build(core, &world, &mut allocator, &trap_points)?;

    let mut assembler = Assembler::new();
    let runtime_labels = NativeRuntimeLabels::declare(&mut assembler)?;
    let entry_text_offset = assembler.position()?;
    emit_process_entry(&mut assembler, &runtime_labels)?;

    let startup_label = assembler.new_label()?;
    let mut system_targets = canonical_system_targets(core)?;
    let mut system_labels = HashMap::new();
    reserve_map(
        &mut system_labels,
        system_targets.len(),
        "native system label map",
    )?;
    for target in &system_targets {
        system_labels.insert(target.legacy_id, assembler.new_label()?);
    }
    assembler.far_jump(startup_label)?;

    let startup = core
        .program()
        .functions
        .iter()
        .find(|function| function.name == "startup")
        .ok_or_else(|| invalid_core("verified executable Core has no startup function"))?;
    let mut functions = Vec::new();
    reserve_vec(
        &mut functions,
        system_targets
            .len()
            .checked_add(1)
            .ok_or(AotV2Error::AddressSpaceOverflow("native function list"))?,
        "native function list",
    )?;

    assembler.bind(startup_label)?;
    let startup_start = assembler.position()?;
    emit_startup_function(
        &mut assembler,
        core,
        startup,
        &runtime,
        &runtime_labels,
        &mut allocator,
    )?;
    let startup_end = assembler.position()?;
    functions.push(NativeFunctionLayout {
        target: NativeFunctionTarget::Startup,
        symbol_name: "_arche_startup_v2".to_string(),
        code_offset: TEXT_IMAGE_OFFSET
            .checked_add(startup_start)
            .ok_or(AotV2Error::ArithmeticOverflow("startup code address"))?,
        code_byte_len: startup_end
            .checked_sub(startup_start)
            .ok_or(AotV2Error::ArithmeticOverflow("startup code length"))?,
    });

    for target in &mut system_targets {
        let label = *system_labels
            .get(&target.legacy_id)
            .ok_or_else(|| invalid_native("native system label map is incomplete"))?;
        assembler.bind(label)?;
        let start = assembler.position()?;
        emit_system_function(
            &mut assembler,
            SystemFunctionLowering {
                core,
                system: target.system,
                dense_index: target.dense_index,
                world: &world,
                runtime: &runtime,
                runtime_labels: &runtime_labels,
                allocator: &mut allocator,
            },
        )?;
        let end = assembler.position()?;
        target.code_start = Some(start);
        functions.push(NativeFunctionLayout {
            target: NativeFunctionTarget::System(target.id),
            symbol_name: format!("_arche_system_{}", target.id),
            code_offset: TEXT_IMAGE_OFFSET
                .checked_add(start)
                .ok_or(AotV2Error::ArithmeticOverflow("system code address"))?,
            code_byte_len: end
                .checked_sub(start)
                .ok_or(AotV2Error::ArithmeticOverflow("system code length"))?,
        });
    }

    emit_runtime(&mut assembler, core, &world, &runtime, &runtime_labels)?;
    let assembled = assembler.finish()?;
    let text_byte_len = as_u64(assembled.bytes.len(), "native text length")?;
    let native_layout = NativeCodeLayout {
        code_range: CodeImageRange {
            offset: TEXT_IMAGE_OFFSET,
            byte_len: text_byte_len,
        },
        functions,
    };
    let data_memory_byte_len = align_u64(
        allocator.byte_len().max(runtime.data_file_byte_len()),
        DATA_SEGMENT_ALIGNMENT,
        "data memory length",
    )?;
    Ok(AotPlan {
        text: assembled.bytes,
        entry_text_offset,
        metadata_relocations: assembled.metadata_relocations,
        data_vaddr_patches: assembled.data_vaddr_patches,
        data_memory_byte_len,
        native_layout,
        runtime,
        world,
    })
}

pub fn finalize_native(
    mut plan: AotPlan,
    core: &VerifiedExecutableCore,
    package: &ExecutionPackage,
) -> Result<AotImage, AotV2Error> {
    validate_execution_package_link(core, package, Some(plan.native_layout.code_range))?;
    let mut metadata_counter = CountingWriteSeek::default();
    let metadata_byte_len = write_package_with_code_range(
        &mut metadata_counter,
        package,
        plan.native_layout.code_range,
    )?;
    if metadata_byte_len != metadata_counter.byte_len() {
        return Err(invalid_native(
            "streamed ARCHEECS metadata length disagrees with its measured extent",
        ));
    }
    let data_chunks = finalize_runtime_data(
        &plan.runtime,
        &plan.world,
        core,
        package,
        &plan.native_layout,
        metadata_byte_len,
    )?;
    let data_file_byte_len = validate_data_chunks(&data_chunks)?;
    if data_file_byte_len > plan.data_memory_byte_len {
        return Err(invalid_native(
            "runtime initialized data exceeds its planned RW memory segment",
        ));
    }
    let provisional = elf64::plan_static_pie(StaticPieRequest {
        entry_text_offset: plan.entry_text_offset,
        text_file_byte_len: as_u64(plan.text.len(), "text byte length")?,
        data_file_byte_len,
        data_memory_byte_len: plan.data_memory_byte_len,
        metadata_file_byte_len: metadata_byte_len,
        minimum_metadata_offset: 0,
        metadata_anchor_relocations: &plan.metadata_relocations,
    })?;
    let data_vaddr = provisional.layout().data_vaddr;
    for patch in &plan.data_vaddr_patches {
        let end = patch
            .immediate_offset
            .checked_add(8)
            .ok_or(AotV2Error::ArithmeticOverflow("data relocation range"))?;
        let destination = plan
            .text
            .get_mut(patch.immediate_offset..end)
            .ok_or_else(|| invalid_native("data relocation lies outside native text"))?;
        destination.copy_from_slice(&data_vaddr.to_le_bytes());
    }
    Ok(AotImage {
        text: plan.text,
        data_chunks,
        data_file_byte_len,
        package: package.clone(),
        code_range: plan.native_layout.code_range,
        metadata_byte_len,
        entry_text_offset: plan.entry_text_offset,
        data_memory_byte_len: plan.data_memory_byte_len,
        metadata_relocations: plan.metadata_relocations,
    })
}

fn validate_data_chunks(chunks: &[DataChunk]) -> Result<u64, AotV2Error> {
    let mut previous_end = 0u64;
    for chunk in chunks {
        if chunk.bytes.is_empty() {
            return Err(invalid_native("initialized data chunks cannot be empty"));
        }
        if chunk.offset < previous_end {
            return Err(invalid_native(
                "initialized data chunks must be sorted and nonoverlapping",
            ));
        }
        previous_end = chunk
            .offset
            .checked_add(as_u64(chunk.bytes.len(), "initialized data chunk")?)
            .ok_or(AotV2Error::ArithmeticOverflow(
                "initialized data chunk range",
            ))?;
    }
    Ok(previous_end)
}

fn write_data_chunks(
    output: &mut dyn crate::elf64::WriteSeek,
    chunks: &[DataChunk],
) -> io::Result<()> {
    for chunk in chunks {
        output.seek(SeekFrom::Start(chunk.offset))?;
        output.write_all(&chunk.bytes)?;
    }
    Ok(())
}

fn emit_process_entry(
    assembler: &mut Assembler,
    runtime: &NativeRuntimeLabels,
) -> Result<(), AotV2Error> {
    let entry = assembler.position()?;
    // Establish r15 as the ELF load bias. The entry is in the R-X segment at
    // the fixed image-relative address TEXT_IMAGE_OFFSET + entry.
    assembler.emit(&[0x4c, 0x8d, 0x3d, 0, 0, 0, 0])?; // lea r15,[rip]
    let anchor = TEXT_IMAGE_OFFSET
        .checked_add(entry)
        .and_then(|value| value.checked_add(7))
        .ok_or(AotV2Error::ArithmeticOverflow("load-bias anchor"))?;
    assembler.mov_imm64(Register::Rax, anchor)?;
    assembler.emit(&[0x49, 0x29, 0xc7])?; // sub r15,rax
    assembler.emit_data_anchor()?;
    assembler.emit_metadata_anchor()?;

    emit_ignore_sigpipe(assembler)?;

    // Arche process-entry floating-point state: MXCSR 0x1F80 and x87 control
    // word 0x037F select RNE, mask exceptions, and disable FTZ/DAZ.
    assembler.adjust_reg64_imm8(Register::Rsp, 16, true)?; // sub rsp,16
    assembler.emit(&[0xc7, 0x04, 0x24])?;
    assembler.emit_u32(0x0000_1f80)?;
    assembler.emit(&[0x0f, 0xae, 0x14, 0x24])?; // ldmxcsr [rsp]
    assembler.emit(&[0x66, 0xc7, 0x44, 0x24, 0x04, 0x7f, 0x03])?;
    assembler.emit(&[0xd9, 0x6c, 0x24, 0x04])?; // fldcw [rsp+4]
    assembler.adjust_reg64_imm8(Register::Rsp, 16, false)?; // add rsp,16
    assembler.far_call(runtime.validate_and_initialize)
}

fn emit_ignore_sigpipe(assembler: &mut Assembler) -> Result<(), AotV2Error> {
    let installed = assembler.new_label()?;
    assembler.adjust_reg64_imm8(Register::Rsp, 32, true)?; // sub rsp,32
    assembler.emit(&[0x48, 0xc7, 0x04, 0x24, 1, 0, 0, 0])?; // handler = SIG_IGN
    assembler.emit(&[0x48, 0xc7, 0x44, 0x24, 0x08, 0, 0, 0, 0])?; // flags = 0
    assembler.emit(&[0x48, 0xc7, 0x44, 0x24, 0x10, 0, 0, 0, 0])?; // restorer = 0
    assembler.emit(&[0x48, 0xc7, 0x44, 0x24, 0x18, 0, 0, 0, 0])?; // mask = 0
    assembler.mov_imm64(Register::Rax, 13)?; // rt_sigaction
    assembler.mov_imm64(Register::Rdi, 13)?; // SIGPIPE
    assembler.mov_reg64(Register::Rsi, Register::Rsp)?;
    assembler.mov_imm64(Register::Rdx, 0)?;
    assembler.mov_imm64(Register::R10, 8)?; // kernel sigset_t byte length
    assembler.emit(&[0x0f, 0x05])?; // syscall
    assembler.emit(&[0x48, 0x85, 0xc0])?; // test rax,rax
    assembler.far_jcc(Condition::GreaterEqual, installed)?;
    assembler.mov_imm64(Register::Rdi, 1)?;
    assembler.mov_imm64(Register::Rax, 231)?; // exit_group
    assembler.emit(&[0x0f, 0x05, 0x0f, 0x0b])?; // syscall; ud2
    assembler.bind(installed)?;
    assembler.adjust_reg64_imm8(Register::Rsp, 32, false) // add rsp,32
}

struct CanonicalSystemTarget<'a> {
    legacy_id: u64,
    id: DeclId,
    dense_index: u64,
    system: &'a CoreSystem,
    code_start: Option<u64>,
}

fn canonical_system_targets(
    core: &VerifiedExecutableCore,
) -> Result<Vec<CanonicalSystemTarget<'_>>, AotV2Error> {
    let ids = canonical_core_ids(core)?;
    let mut systems = Vec::new();
    reserve_vec(
        &mut systems,
        core.program().systems.len(),
        "canonical native systems",
    )?;
    for system in &core.program().systems {
        systems.push(CanonicalSystemTarget {
            legacy_id: system.id,
            id: ids.system(system.id).ok_or_else(|| {
                invalid_core(format!(
                    "Core system `{}` has no canonical identifier",
                    system.name
                ))
            })?,
            dense_index: 0,
            system,
            code_start: None,
        });
    }
    systems.sort_unstable_by_key(|system| system.id);
    for (index, system) in systems.iter_mut().enumerate() {
        system.dense_index = as_u64(index, "dense system index")?;
    }
    Ok(systems)
}

fn plan_world_storage(
    core: &VerifiedExecutableCore,
    ids: &crate::execution_package_build::CanonicalCoreIds,
    allocator: &mut DataAllocator,
) -> Result<WorldStoragePlan, AotV2Error> {
    struct Draft<'a> {
        legacy_id: u64,
        id: SchemaId,
        kind: SchemaKind,
        fields: &'a [crate::core::CoreField],
    }

    let mut drafts = Vec::new();
    let schema_count = core
        .program()
        .components
        .len()
        .checked_add(core.program().resources.len())
        .ok_or(AotV2Error::AddressSpaceOverflow("schema count"))?;
    reserve_vec(&mut drafts, schema_count, "canonical schema drafts")?;
    for component in &core.program().components {
        drafts.push(Draft {
            legacy_id: component.id,
            id: ids.schema(component.id).ok_or_else(|| {
                invalid_core(format!(
                    "Core component `{}` has no canonical identifier",
                    component.name
                ))
            })?,
            kind: match component.kind {
                CoreComponentKind::Component => SchemaKind::Component,
                CoreComponentKind::Tag => SchemaKind::Tag,
            },
            fields: &component.fields,
        });
    }
    for resource in &core.program().resources {
        drafts.push(Draft {
            legacy_id: resource.id,
            id: ids.schema(resource.id).ok_or_else(|| {
                invalid_core(format!(
                    "Core resource `{}` has no canonical identifier",
                    resource.name
                ))
            })?,
            kind: SchemaKind::Resource,
            fields: &resource.fields,
        });
    }
    drafts.sort_unstable_by_key(|schema| schema.id);

    let row_count_offset = allocator.allocate(8, 8, "row count")?;
    let next_spawn_ordinal_offset = allocator.allocate(8, 8, "spawn ordinal")?;
    let mut schemas = Vec::new();
    reserve_vec(&mut schemas, drafts.len(), "canonical schema storage")?;
    for (dense_index, draft) in drafts.iter().enumerate() {
        let (fields, byte_size, alignment) = plan_fields(draft.fields)?;
        let (resource_initialized_offset, resource_payload_offset) =
            if draft.kind == SchemaKind::Resource {
                let initialized = allocator.allocate(1, 1, "resource initialization flag")?;
                let payload = allocator.allocate(byte_size, alignment, "resource payload")?;
                (Some(initialized), Some(payload))
            } else {
                (None, None)
            };
        schemas.push(CanonicalSchemaStorage {
            legacy_id: draft.legacy_id,
            id: draft.id,
            dense_index: as_u64(dense_index, "dense schema index")?,
            kind: draft.kind,
            byte_size,
            alignment,
            fields,
            resource_initialized_offset,
            resource_payload_offset,
            row_cell_offset: None,
        });
    }

    let schema_count_u64 = as_u64(schemas.len(), "schema count")?;
    let row_membership_bytes = schema_count_u64
        .checked_add(7)
        .ok_or(AotV2Error::ArithmeticOverflow("row membership length"))?
        / 8;
    let row_active_offset = 0u64;
    let row_spawn_ordinal_offset = 8u64;
    let row_membership_offset = 16u64;
    let mut row_cursor = row_membership_offset
        .checked_add(row_membership_bytes)
        .ok_or(AotV2Error::ArithmeticOverflow("row membership range"))?;
    let mut row_alignment = 8u64;
    for schema in &mut schemas {
        if schema.kind == SchemaKind::Resource {
            continue;
        }
        row_cursor = align_u64(row_cursor, schema.alignment, "row schema cell")?;
        schema.row_cell_offset = Some(row_cursor);
        row_cursor = row_cursor
            .checked_add(schema.byte_size)
            .ok_or(AotV2Error::ArithmeticOverflow("row schema cells"))?;
        row_alignment = row_alignment.max(schema.alignment);
    }
    let row_stride = align_u64(row_cursor, row_alignment, "row stride")?;
    // Metadata may coherently reorder effect records. The full verified effect
    // count is the exact finite upper bound on committed rows without retaining
    // a compiler-side startup shape.
    let max_rows = core.startup_operations().try_fold(0u64, |count, _| {
        count
            .checked_add(1)
            .ok_or(AotV2Error::ArithmeticOverflow("startup operation count"))
    })?;
    let row_bytes = row_stride
        .checked_mul(max_rows)
        .ok_or(AotV2Error::ArithmeticOverflow("row storage"))?;
    let rows_base = allocator.allocate(row_bytes, row_alignment, "row storage")?;
    Ok(WorldStoragePlan {
        row_count_offset,
        next_spawn_ordinal_offset,
        rows_base,
        row_stride,
        row_active_offset,
        row_spawn_ordinal_offset,
        row_membership_offset,
        row_membership_bytes,
        max_rows,
        schemas,
    })
}

fn plan_fields(
    fields: &[crate::core::CoreField],
) -> Result<(Vec<NativeFieldStorage>, u64, u64), AotV2Error> {
    let mut output = Vec::new();
    reserve_vec(&mut output, fields.len(), "native field layout")?;
    let mut cursor = 0u64;
    let mut schema_alignment = 1u64;
    for field in fields {
        let primitive = primitive_type(field.ty);
        let alignment = primitive_alignment(primitive);
        cursor = align_u64(cursor, alignment, "native field layout")?;
        output.push(NativeFieldStorage {
            name: field.name.clone(),
            primitive,
            byte_offset: cursor,
        });
        cursor = cursor
            .checked_add(primitive_byte_len(primitive))
            .ok_or(AotV2Error::ArithmeticOverflow("native schema size"))?;
        schema_alignment = schema_alignment.max(alignment);
    }
    let byte_size = align_u64(cursor, schema_alignment, "native schema size")?;
    Ok((output, byte_size, schema_alignment))
}

fn collect_trap_points(core: &VerifiedExecutableCore) -> Result<Vec<NativeTrapPoint>, AotV2Error> {
    let mut points = Vec::new();
    let startup = core
        .program()
        .functions
        .iter()
        .find(|function| function.name == "startup")
        .ok_or_else(|| invalid_core("verified executable Core has no startup function"))?;
    for block in &startup.blocks {
        for (index, instruction) in block.instructions.iter().enumerate() {
            if matches!(
                instruction,
                CoreInstruction::I32Binary {
                    op: CoreBinaryOp::Divide | CoreBinaryOp::Remainder,
                    ..
                }
            ) {
                points.push(NativeTrapPoint::Startup {
                    block: block.id,
                    instruction_index: as_u64(index, "startup instruction index")?,
                });
            }
        }
    }
    for system in &core.program().systems {
        let mut ordinal = 0u64;
        collect_statement_traps(
            system.id,
            &system.body.statements,
            &mut ordinal,
            &mut points,
        )?;
    }
    let mut unique = HashSet::new();
    if points.iter().any(|point| !unique.insert(*point)) {
        return Err(invalid_core(
            "duplicate semantic trap point in verified Core",
        ));
    }
    Ok(points)
}

fn collect_statement_traps(
    system_id: u64,
    statements: &[CoreSystemStatement],
    ordinal: &mut u64,
    output: &mut Vec<NativeTrapPoint>,
) -> Result<(), AotV2Error> {
    for statement in statements {
        match statement {
            CoreSystemStatement::Expression(expression)
            | CoreSystemStatement::Let {
                value: expression, ..
            }
            | CoreSystemStatement::Assign {
                value: expression, ..
            }
            | CoreSystemStatement::AddAssign {
                value: expression, ..
            } => collect_expression_traps(system_id, expression, ordinal, output)?,
            CoreSystemStatement::QueryLoop(query) => {
                collect_statement_traps(system_id, &query.body, ordinal, output)?;
            }
            CoreSystemStatement::Block(body) => {
                collect_statement_traps(system_id, body, ordinal, output)?;
            }
            CoreSystemStatement::If {
                condition,
                then_body,
                else_body,
            } => {
                collect_expression_traps(system_id, condition, ordinal, output)?;
                collect_statement_traps(system_id, then_body, ordinal, output)?;
                collect_statement_traps(system_id, else_body, ordinal, output)?;
            }
            CoreSystemStatement::While { condition, body } => {
                collect_expression_traps(system_id, condition, ordinal, output)?;
                collect_statement_traps(system_id, body, ordinal, output)?;
            }
        }
    }
    Ok(())
}

fn collect_expression_traps(
    system_id: u64,
    expression: &CoreSystemExpression,
    ordinal: &mut u64,
    output: &mut Vec<NativeTrapPoint>,
) -> Result<(), AotV2Error> {
    let current = *ordinal;
    *ordinal = ordinal
        .checked_add(1)
        .ok_or(AotV2Error::ArithmeticOverflow("system expression ordinal"))?;
    if matches!(
        expression,
        CoreSystemExpression::Binary {
            op: CoreSystemBinaryOp::I32Divide | CoreSystemBinaryOp::I32Remainder,
            ..
        }
    ) {
        output.push(NativeTrapPoint::System {
            system_id,
            expression_ordinal: current,
        });
    }
    match expression {
        CoreSystemExpression::BoolNot(operand) | CoreSystemExpression::Unary { operand, .. } => {
            collect_expression_traps(system_id, operand, ordinal, output)?;
        }
        CoreSystemExpression::Binary { left, right, .. } => {
            collect_expression_traps(system_id, left, ordinal, output)?;
            collect_expression_traps(system_id, right, ordinal, output)?;
        }
        CoreSystemExpression::I32Const(_)
        | CoreSystemExpression::F32Const(_)
        | CoreSystemExpression::BoolConst(_)
        | CoreSystemExpression::Local { .. }
        | CoreSystemExpression::ResourceField { .. }
        | CoreSystemExpression::ComponentField { .. } => {}
    }
    Ok(())
}

struct StartupEmitContext<'a> {
    function: &'a CoreFunction,
    value_slots: HashMap<ValueId, u64>,
    local_slots: HashMap<LocalId, u64>,
    block_labels: HashMap<BlockId, Label>,
}

fn emit_startup_function(
    assembler: &mut Assembler,
    _core: &VerifiedExecutableCore,
    function: &CoreFunction,
    runtime: &NativeRuntimePlan,
    runtime_labels: &NativeRuntimeLabels,
    allocator: &mut DataAllocator,
) -> Result<(), AotV2Error> {
    let instruction_count = function.blocks.iter().try_fold(0usize, |count, block| {
        count
            .checked_add(block.instructions.len())
            .ok_or(AotV2Error::AddressSpaceOverflow(
                "startup instruction slots",
            ))
    })?;
    let mut value_slots = HashMap::new();
    reserve_map(&mut value_slots, instruction_count, "startup value slots")?;
    for block in &function.blocks {
        for instruction in &block.instructions {
            if let Some(result) = instruction_result(instruction) {
                let offset = allocator.allocate(4, 4, "startup scalar value")?;
                if value_slots.insert(result, offset).is_some() {
                    return Err(invalid_core(format!(
                        "startup defines value {} more than once",
                        result.0
                    )));
                }
            }
        }
    }
    let mut local_slots = HashMap::new();
    reserve_map(
        &mut local_slots,
        function.locals.len(),
        "startup local slots",
    )?;
    for local in &function.locals {
        let offset = allocator.allocate(4, 4, "startup local")?;
        if local_slots.insert(local.id, offset).is_some() {
            return Err(invalid_core(format!(
                "startup local {} is duplicated",
                local.id.0
            )));
        }
    }
    let mut block_labels = HashMap::new();
    reserve_map(
        &mut block_labels,
        function.blocks.len(),
        "startup block labels",
    )?;
    for block in &function.blocks {
        if block_labels
            .insert(block.id, assembler.new_label()?)
            .is_some()
        {
            return Err(invalid_core(format!(
                "startup block {} is duplicated",
                block.id.0
            )));
        }
    }
    let entry = *block_labels
        .get(&function.entry)
        .ok_or_else(|| invalid_core("startup entry block has no native label"))?;
    assembler.far_jump(entry)?;
    let context = StartupEmitContext {
        function,
        value_slots,
        local_slots,
        block_labels,
    };
    for block in &function.blocks {
        assembler.bind(required_block_label(&context, block.id)?)?;
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            emit_startup_instruction(
                assembler,
                &context,
                runtime,
                runtime_labels,
                block.id,
                as_u64(instruction_index, "startup instruction index")?,
                instruction,
            )?;
        }
        emit_startup_terminator(assembler, &context, runtime_labels, &block.terminator)?;
    }
    Ok(())
}

fn emit_startup_instruction(
    assembler: &mut Assembler,
    context: &StartupEmitContext<'_>,
    runtime: &NativeRuntimePlan,
    runtime_labels: &NativeRuntimeLabels,
    block: BlockId,
    instruction_index: u64,
    instruction: &CoreInstruction,
) -> Result<(), AotV2Error> {
    match instruction {
        CoreInstruction::InitializeResource { .. }
        | CoreInstruction::Spawn { .. }
        | CoreInstruction::RunSchedule { .. } => {
            assembler.far_call(runtime_labels.execute_next_startup_operation)?;
        }
        CoreInstruction::I32Const { result, value } => {
            emit_mov_eax_imm32(assembler, u32::from_le_bytes(value.to_le_bytes()))?;
            emit_store_eax_data(assembler, value_slot(context, *result)?)?;
        }
        CoreInstruction::F32Const { result, bits } => {
            emit_mov_eax_imm32(assembler, *bits)?;
            emit_store_eax_data(assembler, value_slot(context, *result)?)?;
        }
        CoreInstruction::BoolConst { result, value } => {
            emit_mov_eax_imm32(assembler, u32::from(*value))?;
            emit_store_eax_data(assembler, value_slot(context, *result)?)?;
        }
        CoreInstruction::I32Binary {
            result,
            op,
            left,
            right,
        } => {
            emit_load_edx_data(assembler, value_slot(context, *left)?)?;
            emit_load_eax_data(assembler, value_slot(context, *right)?)?;
            let trap = if op.trap_kind().is_some() {
                Some(runtime.trap_descriptor_index(NativeTrapPoint::Startup {
                    block,
                    instruction_index,
                })?)
            } else {
                None
            };
            emit_i32_binary(assembler, *op, trap, runtime_labels)?;
            emit_store_eax_data(assembler, value_slot(context, *result)?)?;
        }
        CoreInstruction::I32Unary {
            result,
            op,
            operand,
        } => {
            emit_load_eax_data(assembler, value_slot(context, *operand)?)?;
            match op {
                CoreUnaryOp::Negate => assembler.emit(&[0xf7, 0xd8])?,
                CoreUnaryOp::BitNot => assembler.emit(&[0xf7, 0xd0])?,
            }
            emit_store_eax_data(assembler, value_slot(context, *result)?)?;
        }
        CoreInstruction::F32Unary {
            result,
            op,
            operand,
        } => {
            if *op != CoreUnaryOp::Negate {
                return Err(invalid_core("verified Core applies bit-not to f32"));
            }
            emit_load_eax_data(assembler, value_slot(context, *operand)?)?;
            emit_f32_negate(assembler)?;
            emit_store_eax_data(assembler, value_slot(context, *result)?)?;
        }
        CoreInstruction::F32Binary {
            result,
            op,
            left,
            right,
        } => {
            emit_load_edx_data(assembler, value_slot(context, *left)?)?;
            emit_load_eax_data(assembler, value_slot(context, *right)?)?;
            emit_f32_binary(assembler, *op)?;
            emit_store_eax_data(assembler, value_slot(context, *result)?)?;
        }
        CoreInstruction::Compare {
            result,
            op,
            left,
            right,
            operand_type,
        } => {
            emit_load_edx_data(assembler, value_slot(context, *left)?)?;
            emit_load_eax_data(assembler, value_slot(context, *right)?)?;
            emit_comparison(assembler, *op, *operand_type, false)?;
            emit_store_eax_data(assembler, value_slot(context, *result)?)?;
        }
        CoreInstruction::BoolNot { result, operand } => {
            emit_load_eax_data(assembler, value_slot(context, *operand)?)?;
            assembler.emit(&[0x83, 0xf0, 0x01])?; // xor eax,1
            emit_store_eax_data(assembler, value_slot(context, *result)?)?;
        }
        CoreInstruction::Equal {
            result,
            left,
            right,
            operand_type,
            negate,
        } => {
            emit_load_edx_data(assembler, value_slot(context, *left)?)?;
            emit_load_eax_data(assembler, value_slot(context, *right)?)?;
            emit_equality(assembler, *operand_type, *negate)?;
            emit_store_eax_data(assembler, value_slot(context, *result)?)?;
        }
        CoreInstruction::LocalStore { local, value } => {
            emit_load_eax_data(assembler, value_slot(context, *value)?)?;
            emit_store_eax_data(assembler, local_slot(context, *local)?)?;
        }
        CoreInstruction::LocalLoad { result, local } => {
            emit_load_eax_data(assembler, local_slot(context, *local)?)?;
            emit_store_eax_data(assembler, value_slot(context, *result)?)?;
        }
    }
    Ok(())
}

fn emit_startup_terminator(
    assembler: &mut Assembler,
    context: &StartupEmitContext<'_>,
    runtime: &NativeRuntimeLabels,
    terminator: &CoreTerminator,
) -> Result<(), AotV2Error> {
    match terminator {
        CoreTerminator::Exit { value } => {
            emit_load_eax_data(assembler, value_slot(context, *value)?)?;
            assembler.emit(&[0x89, 0xc7])?; // mov edi,eax
            assembler.far_call(runtime.emit_observation_and_exit)?;
            assembler.emit(&[0x0f, 0x0b])?; // fatal helper never returns
        }
        CoreTerminator::Jump { target } => {
            assembler.far_jump(required_block_label(context, *target)?)?;
        }
        CoreTerminator::Branch {
            condition,
            then_block,
            else_block,
        } => {
            emit_load_eax_data(assembler, value_slot(context, *condition)?)?;
            assembler.emit(&[0x85, 0xc0])?; // test eax,eax
            assembler.far_jcc(
                Condition::NotZero,
                required_block_label(context, *then_block)?,
            )?;
            assembler.far_jump(required_block_label(context, *else_block)?)?;
        }
    }
    Ok(())
}

fn instruction_result(instruction: &CoreInstruction) -> Option<ValueId> {
    match instruction {
        CoreInstruction::I32Const { result, .. }
        | CoreInstruction::I32Binary { result, .. }
        | CoreInstruction::I32Unary { result, .. }
        | CoreInstruction::F32Const { result, .. }
        | CoreInstruction::F32Unary { result, .. }
        | CoreInstruction::F32Binary { result, .. }
        | CoreInstruction::Compare { result, .. }
        | CoreInstruction::BoolConst { result, .. }
        | CoreInstruction::BoolNot { result, .. }
        | CoreInstruction::Equal { result, .. }
        | CoreInstruction::LocalLoad { result, .. } => Some(*result),
        CoreInstruction::InitializeResource { .. }
        | CoreInstruction::Spawn { .. }
        | CoreInstruction::RunSchedule { .. }
        | CoreInstruction::LocalStore { .. } => None,
    }
}

fn required_block_label(
    context: &StartupEmitContext<'_>,
    block: BlockId,
) -> Result<Label, AotV2Error> {
    context.block_labels.get(&block).copied().ok_or_else(|| {
        invalid_core(format!(
            "startup `{}` references unknown block {}",
            context.function.name, block.0
        ))
    })
}

fn value_slot(context: &StartupEmitContext<'_>, value: ValueId) -> Result<u64, AotV2Error> {
    context.value_slots.get(&value).copied().ok_or_else(|| {
        invalid_core(format!(
            "startup references unknown scalar value {}",
            value.0
        ))
    })
}

fn local_slot(context: &StartupEmitContext<'_>, local: LocalId) -> Result<u64, AotV2Error> {
    context
        .local_slots
        .get(&local)
        .copied()
        .ok_or_else(|| invalid_core(format!("startup references unknown local {}", local.0)))
}

fn emit_i32_binary(
    assembler: &mut Assembler,
    op: CoreBinaryOp,
    trap_descriptor: Option<u64>,
    runtime: &NativeRuntimeLabels,
) -> Result<(), AotV2Error> {
    match op {
        CoreBinaryOp::Add => {
            assembler.emit(&[0x01, 0xc2, 0x89, 0xd0])?; // add edx,eax; mov eax,edx
        }
        CoreBinaryOp::Subtract => {
            assembler.emit(&[0x29, 0xc2, 0x89, 0xd0])?; // sub edx,eax; mov eax,edx
        }
        CoreBinaryOp::Multiply => {
            assembler.emit(&[0x0f, 0xaf, 0xd0, 0x89, 0xd0])?; // imul edx,eax
        }
        CoreBinaryOp::BitAnd => {
            assembler.emit(&[0x21, 0xc2, 0x89, 0xd0])?;
        }
        CoreBinaryOp::BitXor => {
            assembler.emit(&[0x31, 0xc2, 0x89, 0xd0])?;
        }
        CoreBinaryOp::BitOr => {
            assembler.emit(&[0x09, 0xc2, 0x89, 0xd0])?;
        }
        CoreBinaryOp::ShiftLeft | CoreBinaryOp::ShiftRight => {
            assembler.emit(&[0x89, 0xc1, 0x89, 0xd0])?; // mov ecx,eax; mov eax,edx
            assembler.emit(&[
                0xd3,
                if op == CoreBinaryOp::ShiftLeft {
                    0xe0
                } else {
                    0xf8
                },
            ])?;
        }
        CoreBinaryOp::Divide | CoreBinaryOp::Remainder => {
            let descriptor = trap_descriptor
                .ok_or_else(|| invalid_native("integer division has no native trap descriptor"))?;
            let nonzero = assembler.new_label()?;
            let division = assembler.new_label()?;
            let not_min = assembler.new_label()?;
            assembler.emit(&[0x89, 0xc1, 0x89, 0xd0])?; // divisor ecx, dividend eax
            assembler.emit(&[0x85, 0xc9])?; // test ecx,ecx
            assembler.far_jcc(Condition::NotZero, nonzero)?;
            emit_trap_transfer(
                assembler,
                if op == CoreBinaryOp::Divide { 0 } else { 2 },
                descriptor,
                runtime,
            )?;
            assembler.bind(nonzero)?;
            assembler.emit(&[0x3d, 0, 0, 0, 0x80])?; // cmp eax,i32::MIN
            assembler.far_jcc(Condition::NotEqual, not_min)?;
            assembler.emit(&[0x83, 0xf9, 0xff])?; // cmp ecx,-1
            assembler.far_jcc(Condition::NotEqual, division)?;
            emit_trap_transfer(
                assembler,
                if op == CoreBinaryOp::Divide { 1 } else { 3 },
                descriptor,
                runtime,
            )?;
            assembler.bind(not_min)?;
            assembler.far_jump(division)?;
            assembler.bind(division)?;
            assembler.emit(&[0x99, 0xf7, 0xf9])?; // cdq; idiv ecx
            if op == CoreBinaryOp::Remainder {
                assembler.emit(&[0x89, 0xd0])?; // mov eax,edx
            }
        }
    }
    Ok(())
}

fn emit_trap_transfer(
    assembler: &mut Assembler,
    kind: u32,
    descriptor: u64,
    runtime: &NativeRuntimeLabels,
) -> Result<(), AotV2Error> {
    assembler.emit(&[0xbf])?; // mov edi,imm32
    assembler.emit_u32(kind)?;
    assembler.mov_imm64(Register::Rsi, descriptor)?;
    assembler.far_call(runtime.emit_trap_and_exit)?;
    assembler.emit(&[0x0f, 0x0b])
}

fn emit_f32_negate(assembler: &mut Assembler) -> Result<(), AotV2Error> {
    assembler.emit(&[0x35, 0, 0, 0, 0x80])?; // xor eax,0x80000000
    emit_canonicalize_nan(assembler)
}

fn emit_f32_binary(assembler: &mut Assembler, op: CoreBinaryOp) -> Result<(), AotV2Error> {
    assembler.emit(&[0x66, 0x0f, 0x6e, 0xc2])?; // movd xmm0,edx
    assembler.emit(&[0x66, 0x0f, 0x6e, 0xc8])?; // movd xmm1,eax
    let opcode = match op {
        CoreBinaryOp::Add => 0x58,
        CoreBinaryOp::Subtract => 0x5c,
        CoreBinaryOp::Multiply => 0x59,
        CoreBinaryOp::Divide => 0x5e,
        CoreBinaryOp::Remainder
        | CoreBinaryOp::ShiftLeft
        | CoreBinaryOp::ShiftRight
        | CoreBinaryOp::BitAnd
        | CoreBinaryOp::BitXor
        | CoreBinaryOp::BitOr => {
            return Err(invalid_core(
                "verified Core uses a non-f32 binary operator on f32",
            ));
        }
    };
    assembler.emit(&[0xf3, 0x0f, opcode, 0xc1])?;
    assembler.emit(&[0x66, 0x0f, 0x7e, 0xc0])?; // movd eax,xmm0
    emit_canonicalize_nan(assembler)
}

fn emit_canonicalize_nan(assembler: &mut Assembler) -> Result<(), AotV2Error> {
    let done = assembler.new_label()?;
    assembler.emit(&[0x89, 0xc1])?; // mov ecx,eax
    assembler.emit(&[0x81, 0xe1, 0xff, 0xff, 0xff, 0x7f])?; // and ecx,0x7fffffff
    assembler.emit(&[0x81, 0xf9, 0, 0, 0x80, 0x7f])?; // cmp ecx,+inf
    assembler.far_jcc(Condition::BelowEqual, done)?;
    emit_mov_eax_imm32(assembler, 0x7fc0_0000)?;
    assembler.bind(done)
}

fn emit_comparison(
    assembler: &mut Assembler,
    op: CoreComparisonOp,
    operand_type: CoreType,
    _negate: bool,
) -> Result<(), AotV2Error> {
    match operand_type {
        CoreType::I32 => {
            assembler.emit(&[0x39, 0xc2])?; // cmp edx,eax
            emit_setcc_eax(
                assembler,
                match op {
                    CoreComparisonOp::Less => 0x9c,
                    CoreComparisonOp::LessEqual => 0x9e,
                    CoreComparisonOp::Greater => 0x9f,
                    CoreComparisonOp::GreaterEqual => 0x9d,
                },
            )
        }
        CoreType::F32 => emit_f32_ordered_comparison(assembler, op),
        CoreType::Bool => Err(invalid_core(
            "verified Core applies an ordered comparison to bool",
        )),
    }
}

fn emit_equality(
    assembler: &mut Assembler,
    operand_type: CoreType,
    negate: bool,
) -> Result<(), AotV2Error> {
    match operand_type {
        CoreType::I32 | CoreType::Bool => {
            assembler.emit(&[0x39, 0xc2])?;
            emit_setcc_eax(assembler, if negate { 0x95 } else { 0x94 })
        }
        CoreType::F32 => emit_f32_equality(assembler, negate),
    }
}

fn emit_f32_ordered_comparison(
    assembler: &mut Assembler,
    op: CoreComparisonOp,
) -> Result<(), AotV2Error> {
    assembler.emit(&[0x66, 0x0f, 0x6e, 0xc2])?;
    assembler.emit(&[0x66, 0x0f, 0x6e, 0xc8])?;
    assembler.emit(&[0x0f, 0x2e, 0xc1])?; // ucomiss xmm0,xmm1
    let condition = match op {
        CoreComparisonOp::Less => 0x92,
        CoreComparisonOp::LessEqual => 0x96,
        CoreComparisonOp::Greater => 0x97,
        CoreComparisonOp::GreaterEqual => 0x93,
    };
    assembler.emit(&[0x0f, condition, 0xc0])?; // setcc al
    assembler.emit(&[0x0f, 0x9b, 0xc2])?; // setnp dl
    assembler.emit(&[0x20, 0xd0, 0x0f, 0xb6, 0xc0])?; // and al,dl; movzx eax,al
    Ok(())
}

fn emit_f32_equality(assembler: &mut Assembler, negate: bool) -> Result<(), AotV2Error> {
    assembler.emit(&[0x66, 0x0f, 0x6e, 0xc2])?;
    assembler.emit(&[0x66, 0x0f, 0x6e, 0xc8])?;
    assembler.emit(&[0x0f, 0x2e, 0xc1])?;
    if negate {
        assembler.emit(&[0x0f, 0x95, 0xc0])?; // setne al
        assembler.emit(&[0x0f, 0x9a, 0xc2])?; // setp dl
        assembler.emit(&[0x08, 0xd0])?; // or al,dl
    } else {
        assembler.emit(&[0x0f, 0x94, 0xc0])?; // sete al
        assembler.emit(&[0x0f, 0x9b, 0xc2])?; // setnp dl
        assembler.emit(&[0x20, 0xd0])?; // and al,dl
    }
    assembler.emit(&[0x0f, 0xb6, 0xc0])
}

fn emit_setcc_eax(assembler: &mut Assembler, opcode: u8) -> Result<(), AotV2Error> {
    assembler.emit(&[0x0f, opcode, 0xc0, 0x0f, 0xb6, 0xc0])
}

fn emit_mov_eax_imm32(assembler: &mut Assembler, value: u32) -> Result<(), AotV2Error> {
    assembler.emit(&[0xb8])?;
    assembler.emit_u32(value)
}

fn emit_store_eax_data(assembler: &mut Assembler, offset: u64) -> Result<(), AotV2Error> {
    assembler.data_address(Register::Rdx, offset)?;
    assembler.emit(&[0x89, 0x02])
}

fn emit_load_eax_data(assembler: &mut Assembler, offset: u64) -> Result<(), AotV2Error> {
    assembler.data_address(Register::Rcx, offset)?;
    assembler.emit(&[0x8b, 0x01])
}

fn emit_load_edx_data(assembler: &mut Assembler, offset: u64) -> Result<(), AotV2Error> {
    assembler.data_address(Register::Rcx, offset)?;
    assembler.emit(&[0x8b, 0x11])
}

fn primitive_type(ty: CoreType) -> PrimitiveType {
    match ty {
        CoreType::I32 => PrimitiveType::I32,
        CoreType::F32 => PrimitiveType::F32,
        CoreType::Bool => PrimitiveType::Bool,
    }
}

fn primitive_byte_len(primitive: PrimitiveType) -> u64 {
    match primitive {
        PrimitiveType::I32 | PrimitiveType::F32 => 4,
        PrimitiveType::Bool => 1,
    }
}

fn primitive_alignment(primitive: PrimitiveType) -> u64 {
    primitive_byte_len(primitive)
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

fn as_u64(value: usize, context: &'static str) -> Result<u64, AotV2Error> {
    u64::try_from(value).map_err(|_| AotV2Error::AddressSpaceOverflow(context))
}

fn reserve_vec<T>(
    output: &mut Vec<T>,
    additional: usize,
    context: &'static str,
) -> Result<(), AotV2Error> {
    output
        .try_reserve(additional)
        .map_err(|_| AotV2Error::Allocation(context))
}

fn reserve_map<K: Eq + std::hash::Hash, V>(
    output: &mut HashMap<K, V>,
    additional: usize,
    context: &'static str,
) -> Result<(), AotV2Error> {
    output
        .try_reserve(additional)
        .map_err(|_| AotV2Error::Allocation(context))
}

fn invalid_core(message: impl Into<String>) -> AotV2Error {
    AotV2Error::InvalidCore(message.into())
}

fn invalid_native(message: impl Into<String>) -> AotV2Error {
    AotV2Error::InvalidNativePlan(message.into())
}

#[derive(Clone, Copy)]
struct SystemLocalBinding {
    offset: u64,
    ty: CoreType,
    mutable: bool,
}

#[derive(Clone, Copy)]
enum SystemParameterBinding {
    Resource {
        schema_legacy_id: u64,
        mutable: bool,
    },
    Query {
        dense_index: u64,
    },
}

#[derive(Clone, Copy)]
struct ActiveRowBinding {
    row_slot: u64,
    schema_legacy_id: u64,
    access: CoreQueryAccess,
}

struct SystemEmitContext<'a> {
    system: &'a CoreSystem,
    world: &'a WorldStoragePlan,
    runtime: &'a NativeRuntimePlan,
    runtime_labels: &'a NativeRuntimeLabels,
    allocator: &'a mut DataAllocator,
    parameters: HashMap<&'a str, SystemParameterBinding>,
    local_scopes: Vec<HashMap<&'a str, SystemLocalBinding>>,
    row_scopes: Vec<HashMap<&'a str, ActiveRowBinding>>,
    expression_ordinal: u64,
}

struct SystemFunctionLowering<'a> {
    core: &'a VerifiedExecutableCore,
    system: &'a CoreSystem,
    dense_index: u64,
    world: &'a WorldStoragePlan,
    runtime: &'a NativeRuntimePlan,
    runtime_labels: &'a NativeRuntimeLabels,
    allocator: &'a mut DataAllocator,
}

fn emit_system_function(
    assembler: &mut Assembler,
    lowering: SystemFunctionLowering<'_>,
) -> Result<(), AotV2Error> {
    let SystemFunctionLowering {
        core,
        system,
        dense_index,
        world,
        runtime,
        runtime_labels,
        allocator,
    } = lowering;
    let linked = assembler.new_label()?;
    assembler.mov_imm64(Register::Rcx, dense_index)?;
    assembler.emit(&[0x48, 0x39, 0xcf])?; // cmp rdi,rcx
    assembler.far_jcc(Condition::Equal, linked)?;
    assembler.mov_imm64(Register::Rdi, 1)?;
    assembler.mov_imm64(Register::Rax, 231)?;
    assembler.emit(&[0x0f, 0x05, 0x0f, 0x0b])?;
    assembler.bind(linked)?;
    assembler.emit(&[0x41, 0x54])?; // push r12 (callee-saved; also align helper calls)
    let parameters = system_parameter_bindings(core, system)?;
    let mut context = SystemEmitContext {
        system,
        world,
        runtime,
        runtime_labels,
        allocator,
        parameters,
        local_scopes: Vec::new(),
        row_scopes: Vec::new(),
        expression_ordinal: 0,
    };
    emit_system_scope(assembler, &mut context, &system.body.statements)?;
    assembler.emit(&[0x41, 0x5c])?; // pop r12
    assembler.emit(&[0xc3])?; // ret
    let mapped_count = core
        .program()
        .source_map
        .entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.subject,
                crate::core::CoreSourceSubject::SystemExpression { system_id, .. }
                    if system_id == system.id
            )
        })
        .count();
    if context.expression_ordinal != as_u64(mapped_count, "system expression count")? {
        return Err(invalid_core(format!(
            "system `{}` expression source map does not match native traversal",
            system.name
        )));
    }
    Ok(())
}

fn system_parameter_bindings<'a>(
    core: &VerifiedExecutableCore,
    system: &'a CoreSystem,
) -> Result<HashMap<&'a str, SystemParameterBinding>, AotV2Error> {
    let ids = canonical_core_ids(core)?;
    let canonical_system = ids.system(system.id).ok_or_else(|| {
        invalid_core(format!(
            "system `{}` has no canonical identifier",
            system.name
        ))
    })?;
    let mut query_ids = Vec::new();
    for candidate in &core.program().systems {
        let candidate_id = ids.system(candidate.id).ok_or_else(|| {
            invalid_core(format!(
                "system `{}` has no canonical identifier",
                candidate.name
            ))
        })?;
        for parameter in &candidate.params {
            if matches!(
                parameter.kind,
                crate::core::CoreSystemParamKind::Query { .. }
            ) {
                query_ids.push(DeclId::query(candidate_id, &parameter.name));
            }
        }
    }
    query_ids.sort_unstable();
    let mut output = HashMap::new();
    reserve_map(&mut output, system.params.len(), "native system parameters")?;
    for parameter in &system.params {
        let binding = match &parameter.kind {
            crate::core::CoreSystemParamKind::ReadResource { resource_id, .. } => {
                SystemParameterBinding::Resource {
                    schema_legacy_id: *resource_id,
                    mutable: false,
                }
            }
            crate::core::CoreSystemParamKind::MutResource { resource_id, .. } => {
                SystemParameterBinding::Resource {
                    schema_legacy_id: *resource_id,
                    mutable: true,
                }
            }
            crate::core::CoreSystemParamKind::Query { .. } => {
                let id = DeclId::query(canonical_system, &parameter.name);
                let dense_index = query_ids.binary_search(&id).map_err(|_| {
                    invalid_core(format!(
                        "query parameter `{}.{}` has no canonical index",
                        system.name, parameter.name
                    ))
                })?;
                SystemParameterBinding::Query {
                    dense_index: as_u64(dense_index, "dense query index")?,
                }
            }
        };
        if output.insert(parameter.name.as_str(), binding).is_some() {
            return Err(invalid_core(format!(
                "system `{}` has duplicate parameter `{}`",
                system.name, parameter.name
            )));
        }
    }
    Ok(output)
}

fn emit_system_scope<'a>(
    assembler: &mut Assembler,
    context: &mut SystemEmitContext<'a>,
    statements: &'a [CoreSystemStatement],
) -> Result<(), AotV2Error> {
    context.local_scopes.push(HashMap::new());
    context.row_scopes.push(HashMap::new());
    let result = emit_system_statements(assembler, context, statements);
    context.local_scopes.pop();
    context.row_scopes.pop();
    result
}

fn emit_system_statements<'a>(
    assembler: &mut Assembler,
    context: &mut SystemEmitContext<'a>,
    statements: &'a [CoreSystemStatement],
) -> Result<(), AotV2Error> {
    for statement in statements {
        match statement {
            CoreSystemStatement::Expression(expression) => {
                emit_system_expression(assembler, context, expression)?;
            }
            CoreSystemStatement::Let {
                name,
                ty,
                mutable,
                value,
            } => {
                emit_system_expression(assembler, context, value)?;
                let offset = context.allocator.allocate(4, 4, "system local")?;
                emit_store_eax_data(assembler, offset)?;
                let scope = context
                    .local_scopes
                    .last_mut()
                    .ok_or_else(|| invalid_core("system local has no lexical scope"))?;
                if scope
                    .insert(
                        name,
                        SystemLocalBinding {
                            offset,
                            ty: *ty,
                            mutable: *mutable,
                        },
                    )
                    .is_some()
                {
                    return Err(invalid_core(format!(
                        "system `{}` duplicates local `{name}`",
                        context.system.name
                    )));
                }
            }
            CoreSystemStatement::Assign { target, value } => {
                emit_system_expression(assembler, context, value)?;
                assembler.emit(&[0x41, 0x89, 0xc4])?; // mov r12d,eax
                let ty = emit_place_address(assembler, context, target, true)?;
                emit_store_r12_to_rdx(assembler, ty)?;
            }
            CoreSystemStatement::AddAssign { target, value } => {
                let ty = emit_place_address(assembler, context, target, false)?;
                emit_load_eax_from_rdx(assembler, ty)?;
                let left_slot = context
                    .allocator
                    .allocate(4, 4, "compound assignment scratch")?;
                emit_store_eax_data(assembler, left_slot)?;
                emit_system_expression(assembler, context, value)?;
                emit_load_edx_data(assembler, left_slot)?;
                match ty {
                    CoreType::I32 => {
                        emit_i32_binary(assembler, CoreBinaryOp::Add, None, context.runtime_labels)?
                    }
                    CoreType::F32 => emit_f32_binary(assembler, CoreBinaryOp::Add)?,
                    CoreType::Bool => {
                        return Err(invalid_core("verified Core applies += to bool"));
                    }
                }
                assembler.emit(&[0x41, 0x89, 0xc4])?;
                let target_ty = emit_place_address(assembler, context, target, true)?;
                if target_ty != ty {
                    return Err(invalid_core("compound assignment target type changed"));
                }
                emit_store_r12_to_rdx(assembler, ty)?;
            }
            CoreSystemStatement::Block(body) => {
                emit_system_scope(assembler, context, body)?;
            }
            CoreSystemStatement::If {
                condition,
                then_body,
                else_body,
            } => {
                let else_label = assembler.new_label()?;
                let done = assembler.new_label()?;
                emit_system_expression(assembler, context, condition)?;
                assembler.emit(&[0x85, 0xc0])?;
                assembler.far_jcc(Condition::Zero, else_label)?;
                emit_system_scope(assembler, context, then_body)?;
                assembler.far_jump(done)?;
                assembler.bind(else_label)?;
                emit_system_scope(assembler, context, else_body)?;
                assembler.bind(done)?;
            }
            CoreSystemStatement::While { condition, body } => {
                let head = assembler.new_label()?;
                let done = assembler.new_label()?;
                assembler.bind(head)?;
                emit_system_expression(assembler, context, condition)?;
                assembler.emit(&[0x85, 0xc0])?;
                assembler.far_jcc(Condition::Zero, done)?;
                emit_system_scope(assembler, context, body)?;
                assembler.far_jump(head)?;
                assembler.bind(done)?;
            }
            CoreSystemStatement::QueryLoop(query) => {
                emit_query_loop(assembler, context, query)?;
            }
        }
    }
    Ok(())
}

fn emit_query_loop<'a>(
    assembler: &mut Assembler,
    context: &mut SystemEmitContext<'a>,
    query: &'a crate::core::CoreQueryLoop,
) -> Result<(), AotV2Error> {
    let query_index = match context.parameters.get(query.query_param.as_str()) {
        Some(SystemParameterBinding::Query { dense_index }) => *dense_index,
        Some(SystemParameterBinding::Resource { .. }) => {
            return Err(invalid_core(format!(
                "resource parameter `{}` is used as a query",
                query.query_param
            )));
        }
        None => {
            return Err(invalid_core(format!(
                "unknown query parameter `{}`",
                query.query_param
            )));
        }
    };
    let cursor_slot = context.allocator.allocate(8, 8, "query cursor")?;
    let row_slot = context.allocator.allocate(8, 8, "query row binding")?;
    assembler.mov_imm64(Register::Rax, 0)?;
    emit_store_rax_data(assembler, cursor_slot)?;
    let head = assembler.new_label()?;
    let done = assembler.new_label()?;
    assembler.bind(head)?;
    assembler.mov_imm64(Register::Rdi, query_index)?;
    emit_load_rsi_data(assembler, cursor_slot)?;
    assembler.far_call(context.runtime_labels.next_query_row)?;
    assembler.mov_imm64(Register::Rcx, U64_NONE)?;
    assembler.emit(&[0x48, 0x39, 0xc8])?; // cmp rax,rcx
    assembler.far_jcc(Condition::Equal, done)?;
    emit_store_rax_data(assembler, row_slot)?;
    assembler.emit(&[0x48, 0x83, 0xc0, 0x01])?;
    emit_store_rax_data(assembler, cursor_slot)?;

    context.local_scopes.push(HashMap::new());
    let mut rows = HashMap::new();
    reserve_map(&mut rows, query.bindings.len(), "query row bindings")?;
    for binding in &query.bindings {
        if binding.name == "_" {
            continue;
        }
        if rows
            .insert(
                binding.name.as_str(),
                ActiveRowBinding {
                    row_slot,
                    schema_legacy_id: binding.component_id,
                    access: binding.access,
                },
            )
            .is_some()
        {
            return Err(invalid_core(format!(
                "query loop duplicates binding `{}`",
                binding.name
            )));
        }
    }
    context.row_scopes.push(rows);
    let body_result = emit_system_statements(assembler, context, &query.body);
    context.row_scopes.pop();
    context.local_scopes.pop();
    body_result?;
    assembler.far_jump(head)?;
    assembler.bind(done)
}

fn emit_system_expression(
    assembler: &mut Assembler,
    context: &mut SystemEmitContext<'_>,
    expression: &CoreSystemExpression,
) -> Result<CoreType, AotV2Error> {
    let ordinal = context.expression_ordinal;
    context.expression_ordinal = context
        .expression_ordinal
        .checked_add(1)
        .ok_or(AotV2Error::ArithmeticOverflow("system expression ordinal"))?;
    match expression {
        CoreSystemExpression::I32Const(value) => {
            emit_mov_eax_imm32(assembler, u32::from_le_bytes(value.to_le_bytes()))?;
            Ok(CoreType::I32)
        }
        CoreSystemExpression::F32Const(bits) => {
            emit_mov_eax_imm32(assembler, *bits)?;
            Ok(CoreType::F32)
        }
        CoreSystemExpression::BoolConst(value) => {
            emit_mov_eax_imm32(assembler, u32::from(*value))?;
            Ok(CoreType::Bool)
        }
        CoreSystemExpression::Local { name, ty } => {
            let binding = resolve_local(context, name)?;
            if binding.ty != *ty {
                return Err(invalid_core(format!(
                    "local `{name}` has inconsistent Core type"
                )));
            }
            emit_load_eax_data(assembler, binding.offset)?;
            Ok(binding.ty)
        }
        CoreSystemExpression::ResourceField {
            param,
            resource_id,
            field_name,
            ..
        } => {
            let ty = emit_resource_field_address(
                assembler,
                context,
                param,
                *resource_id,
                field_name,
                false,
            )?;
            emit_load_eax_from_rdx(assembler, ty)?;
            Ok(ty)
        }
        CoreSystemExpression::ComponentField {
            binding,
            component_id,
            field_name,
            ..
        } => {
            let ty = emit_component_field_address(
                assembler,
                context,
                binding,
                *component_id,
                field_name,
                false,
            )?;
            emit_load_eax_from_rdx(assembler, ty)?;
            Ok(ty)
        }
        CoreSystemExpression::BoolNot(operand) => {
            let ty = emit_system_expression(assembler, context, operand)?;
            if ty != CoreType::Bool {
                return Err(invalid_core("bool-not operand is not bool"));
            }
            assembler.emit(&[0x83, 0xf0, 0x01])?;
            Ok(CoreType::Bool)
        }
        CoreSystemExpression::Unary { op, operand } => {
            let ty = emit_system_expression(assembler, context, operand)?;
            match (op, ty) {
                (CoreSystemUnaryOp::I32Negate, CoreType::I32) => {
                    assembler.emit(&[0xf7, 0xd8])?;
                }
                (CoreSystemUnaryOp::I32BitNot, CoreType::I32) => {
                    assembler.emit(&[0xf7, 0xd0])?;
                }
                (CoreSystemUnaryOp::F32Negate, CoreType::F32) => {
                    emit_f32_negate(assembler)?;
                }
                (CoreSystemUnaryOp::BoolNot, CoreType::Bool) => {
                    assembler.emit(&[0x83, 0xf0, 0x01])?;
                }
                _ => return Err(invalid_core("system unary operator/type mismatch")),
            }
            Ok(ty)
        }
        CoreSystemExpression::Binary { op, left, right } => {
            emit_system_binary_expression(assembler, context, ordinal, *op, left, right)
        }
    }
}

fn emit_system_binary_expression(
    assembler: &mut Assembler,
    context: &mut SystemEmitContext<'_>,
    ordinal: u64,
    op: CoreSystemBinaryOp,
    left: &CoreSystemExpression,
    right: &CoreSystemExpression,
) -> Result<CoreType, AotV2Error> {
    if matches!(
        op,
        CoreSystemBinaryOp::LogicalAnd | CoreSystemBinaryOp::LogicalOr
    ) {
        let short = assembler.new_label()?;
        let done = assembler.new_label()?;
        let left_ty = emit_system_expression(assembler, context, left)?;
        if left_ty != CoreType::Bool {
            return Err(invalid_core("logical left operand is not bool"));
        }
        assembler.emit(&[0x85, 0xc0])?;
        assembler.far_jcc(
            if op == CoreSystemBinaryOp::LogicalAnd {
                Condition::Zero
            } else {
                Condition::NotZero
            },
            short,
        )?;
        let right_ty = emit_system_expression(assembler, context, right)?;
        if right_ty != CoreType::Bool {
            return Err(invalid_core("logical right operand is not bool"));
        }
        assembler.far_jump(done)?;
        assembler.bind(short)?;
        emit_mov_eax_imm32(assembler, u32::from(op == CoreSystemBinaryOp::LogicalOr))?;
        assembler.bind(done)?;
        return Ok(CoreType::Bool);
    }

    let left_ty = emit_system_expression(assembler, context, left)?;
    let scratch = context
        .allocator
        .allocate(4, 4, "binary expression scratch")?;
    emit_store_eax_data(assembler, scratch)?;
    let right_ty = emit_system_expression(assembler, context, right)?;
    if left_ty != right_ty {
        return Err(invalid_core("system binary operand types differ"));
    }
    emit_load_edx_data(assembler, scratch)?;
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
            if left_ty != CoreType::I32 {
                return Err(invalid_core("i32 operator has a non-i32 operand"));
            }
            let core_op = match op {
                CoreSystemBinaryOp::I32Add => CoreBinaryOp::Add,
                CoreSystemBinaryOp::I32Subtract => CoreBinaryOp::Subtract,
                CoreSystemBinaryOp::I32Multiply => CoreBinaryOp::Multiply,
                CoreSystemBinaryOp::I32Divide => CoreBinaryOp::Divide,
                CoreSystemBinaryOp::I32Remainder => CoreBinaryOp::Remainder,
                CoreSystemBinaryOp::I32ShiftLeft => CoreBinaryOp::ShiftLeft,
                CoreSystemBinaryOp::I32ShiftRight => CoreBinaryOp::ShiftRight,
                CoreSystemBinaryOp::I32BitAnd => CoreBinaryOp::BitAnd,
                CoreSystemBinaryOp::I32BitXor => CoreBinaryOp::BitXor,
                CoreSystemBinaryOp::I32BitOr => CoreBinaryOp::BitOr,
                _ => unreachable!(),
            };
            let trap_descriptor =
                if matches!(core_op, CoreBinaryOp::Divide | CoreBinaryOp::Remainder) {
                    Some(
                        context
                            .runtime
                            .trap_descriptor_index(NativeTrapPoint::System {
                                system_id: context.system.id,
                                expression_ordinal: ordinal,
                            })?,
                    )
                } else {
                    None
                };
            emit_i32_binary(assembler, core_op, trap_descriptor, context.runtime_labels)?;
            Ok(CoreType::I32)
        }
        CoreSystemBinaryOp::F32Add
        | CoreSystemBinaryOp::F32Subtract
        | CoreSystemBinaryOp::F32Multiply
        | CoreSystemBinaryOp::F32Divide => {
            if left_ty != CoreType::F32 {
                return Err(invalid_core("f32 operator has a non-f32 operand"));
            }
            emit_f32_binary(
                assembler,
                match op {
                    CoreSystemBinaryOp::F32Add => CoreBinaryOp::Add,
                    CoreSystemBinaryOp::F32Subtract => CoreBinaryOp::Subtract,
                    CoreSystemBinaryOp::F32Multiply => CoreBinaryOp::Multiply,
                    CoreSystemBinaryOp::F32Divide => CoreBinaryOp::Divide,
                    _ => unreachable!(),
                },
            )?;
            Ok(CoreType::F32)
        }
        CoreSystemBinaryOp::I32Less
        | CoreSystemBinaryOp::I32LessEqual
        | CoreSystemBinaryOp::I32Greater
        | CoreSystemBinaryOp::I32GreaterEqual
        | CoreSystemBinaryOp::F32Less
        | CoreSystemBinaryOp::F32LessEqual
        | CoreSystemBinaryOp::F32Greater
        | CoreSystemBinaryOp::F32GreaterEqual => {
            let comparison = match op {
                CoreSystemBinaryOp::I32Less | CoreSystemBinaryOp::F32Less => CoreComparisonOp::Less,
                CoreSystemBinaryOp::I32LessEqual | CoreSystemBinaryOp::F32LessEqual => {
                    CoreComparisonOp::LessEqual
                }
                CoreSystemBinaryOp::I32Greater | CoreSystemBinaryOp::F32Greater => {
                    CoreComparisonOp::Greater
                }
                CoreSystemBinaryOp::I32GreaterEqual | CoreSystemBinaryOp::F32GreaterEqual => {
                    CoreComparisonOp::GreaterEqual
                }
                _ => unreachable!(),
            };
            emit_comparison(assembler, comparison, left_ty, false)?;
            Ok(CoreType::Bool)
        }
        CoreSystemBinaryOp::Equal | CoreSystemBinaryOp::NotEqual => {
            emit_equality(assembler, left_ty, op == CoreSystemBinaryOp::NotEqual)?;
            Ok(CoreType::Bool)
        }
        CoreSystemBinaryOp::LogicalAnd | CoreSystemBinaryOp::LogicalOr => unreachable!(),
    }
}

fn emit_place_address(
    assembler: &mut Assembler,
    context: &SystemEmitContext<'_>,
    place: &CoreSystemPlace,
    require_mutable: bool,
) -> Result<CoreType, AotV2Error> {
    match place {
        CoreSystemPlace::Local { name, ty, mutable } => {
            let binding = resolve_local(context, name)?;
            if binding.ty != *ty || binding.mutable != *mutable {
                return Err(invalid_core(format!(
                    "local place `{name}` disagrees with its lexical binding"
                )));
            }
            if require_mutable && !binding.mutable {
                return Err(invalid_core(format!(
                    "assignment targets immutable local `{name}`"
                )));
            }
            assembler.data_address(Register::Rdx, binding.offset)?;
            Ok(binding.ty)
        }
        CoreSystemPlace::ResourceField {
            param,
            resource_id,
            field_name,
            ..
        } => emit_resource_field_address(
            assembler,
            context,
            param,
            *resource_id,
            field_name,
            require_mutable,
        ),
        CoreSystemPlace::ComponentField {
            binding,
            component_id,
            field_name,
            ..
        } => emit_component_field_address(
            assembler,
            context,
            binding,
            *component_id,
            field_name,
            require_mutable,
        ),
    }
}

fn emit_resource_field_address(
    assembler: &mut Assembler,
    context: &SystemEmitContext<'_>,
    parameter_name: &str,
    resource_id: u64,
    field_name: &str,
    require_mutable: bool,
) -> Result<CoreType, AotV2Error> {
    let (parameter_resource, mutable) = match context.parameters.get(parameter_name) {
        Some(SystemParameterBinding::Resource {
            schema_legacy_id,
            mutable,
        }) => (*schema_legacy_id, *mutable),
        Some(SystemParameterBinding::Query { .. }) => {
            return Err(invalid_core(format!(
                "query parameter `{parameter_name}` is used as a resource"
            )));
        }
        None => {
            return Err(invalid_core(format!(
                "unknown resource parameter `{parameter_name}`"
            )));
        }
    };
    if parameter_resource != resource_id {
        return Err(invalid_core(format!(
            "resource parameter `{parameter_name}` has inconsistent schema"
        )));
    }
    if require_mutable && !mutable {
        return Err(invalid_core(format!(
            "assignment uses read-only resource parameter `{parameter_name}`"
        )));
    }
    let schema = context
        .world
        .schema_by_legacy(resource_id)
        .ok_or_else(|| invalid_core("resource field references unknown schema"))?;
    let payload = schema
        .resource_payload_offset
        .ok_or_else(|| invalid_core(format!("schema {} is not resource storage", schema.id)))?;
    let field = schema
        .fields
        .iter()
        .find(|field| field.name == field_name)
        .ok_or_else(|| invalid_core(format!("resource field `{field_name}` is unknown")))?;
    let offset = payload
        .checked_add(field.byte_offset)
        .ok_or(AotV2Error::ArithmeticOverflow("resource field address"))?;
    assembler.data_address(Register::Rdx, offset)?;
    Ok(core_type(field.primitive))
}

fn emit_component_field_address(
    assembler: &mut Assembler,
    context: &SystemEmitContext<'_>,
    binding_name: &str,
    component_id: u64,
    field_name: &str,
    require_mutable: bool,
) -> Result<CoreType, AotV2Error> {
    let binding = resolve_row_binding(context, binding_name)?;
    if binding.schema_legacy_id != component_id {
        return Err(invalid_core(format!(
            "query binding `{binding_name}` has inconsistent schema"
        )));
    }
    if require_mutable && binding.access != CoreQueryAccess::Mut {
        return Err(invalid_core(format!(
            "assignment uses read-only query binding `{binding_name}`"
        )));
    }
    let schema = context
        .world
        .schema_by_legacy(component_id)
        .ok_or_else(|| invalid_core("component field references unknown schema"))?;
    let cell = schema
        .row_cell_offset
        .ok_or_else(|| invalid_core(format!("schema {} has no row cell", schema.id)))?;
    let field = schema
        .fields
        .iter()
        .find(|field| field.name == field_name)
        .ok_or_else(|| invalid_core(format!("component field `{field_name}` is unknown")))?;
    emit_load_rax_data(assembler, binding.row_slot)?;
    assembler.mov_imm64(Register::Rcx, context.world.row_stride)?;
    assembler.emit(&[0x48, 0x0f, 0xaf, 0xc1])?; // imul rax,rcx
    let base = context
        .world
        .rows_base
        .checked_add(cell)
        .and_then(|offset| offset.checked_add(field.byte_offset))
        .ok_or(AotV2Error::ArithmeticOverflow("row field address"))?;
    assembler.data_address(Register::Rdx, base)?;
    assembler.add_reg64(Register::Rdx, Register::Rax)?;
    Ok(core_type(field.primitive))
}

fn resolve_local(
    context: &SystemEmitContext<'_>,
    name: &str,
) -> Result<SystemLocalBinding, AotV2Error> {
    context
        .local_scopes
        .iter()
        .rev()
        .find_map(|scope| scope.get(name).copied())
        .ok_or_else(|| invalid_core(format!("unknown active local `{name}`")))
}

fn resolve_row_binding(
    context: &SystemEmitContext<'_>,
    name: &str,
) -> Result<ActiveRowBinding, AotV2Error> {
    context
        .row_scopes
        .iter()
        .rev()
        .find_map(|scope| scope.get(name).copied())
        .ok_or_else(|| invalid_core(format!("unknown active query binding `{name}`")))
}

fn emit_load_eax_from_rdx(assembler: &mut Assembler, ty: CoreType) -> Result<(), AotV2Error> {
    match ty {
        CoreType::I32 | CoreType::F32 => assembler.emit(&[0x8b, 0x02]),
        CoreType::Bool => assembler.emit(&[0x0f, 0xb6, 0x02]),
    }
}

fn emit_store_r12_to_rdx(assembler: &mut Assembler, ty: CoreType) -> Result<(), AotV2Error> {
    match ty {
        CoreType::I32 | CoreType::F32 => assembler.emit(&[0x44, 0x89, 0x22]),
        CoreType::Bool => assembler.emit(&[0x44, 0x88, 0x22]),
    }
}

fn emit_store_rax_data(assembler: &mut Assembler, offset: u64) -> Result<(), AotV2Error> {
    assembler.data_address(Register::Rdx, offset)?;
    assembler.emit(&[0x48, 0x89, 0x02])
}

fn emit_load_rax_data(assembler: &mut Assembler, offset: u64) -> Result<(), AotV2Error> {
    assembler.data_address(Register::Rdx, offset)?;
    assembler.emit(&[0x48, 0x8b, 0x02])
}

fn emit_load_rsi_data(assembler: &mut Assembler, offset: u64) -> Result<(), AotV2Error> {
    assembler.data_address(Register::Rdx, offset)?;
    assembler.emit(&[0x48, 0x8b, 0x32])
}

fn core_type(primitive: PrimitiveType) -> CoreType {
    match primitive {
        PrimitiveType::I32 => CoreType::I32,
        PrimitiveType::F32 => CoreType::F32,
        PrimitiveType::Bool => CoreType::Bool,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verified(source: &str) -> VerifiedExecutableCore {
        let tokens = crate::lexer::lex(source).expect("fixture lexes");
        let program = crate::parser::parse_program(&tokens).expect("fixture parses");
        crate::checker::check_program(&program).expect("fixture checks");
        let core = crate::core_lower::lower_program_to_core(&program).expect("fixture lowers");
        crate::core_verify::verify_executable_core(core).expect("fixture Core verifies")
    }

    #[test]
    fn far_control_transfers_never_use_rel32_patches() {
        let mut assembler = Assembler::new();
        let target = assembler.new_label().expect("label allocates");
        assembler
            .far_jcc(Condition::Equal, target)
            .expect("far conditional emits");
        assembler.bind(target).expect("target binds");
        let text = assembler.finish().expect("labels patch");
        assert_eq!(&text.bytes[..5], &[0x75, 23, 0x4c, 0x8d, 0x1d]);
        assert_eq!(&text.bytes[19..25], &[0x4d, 0x01, 0xd3, 0x41, 0xff, 0xe3]);
    }

    #[test]
    fn data_addresses_use_full_u64_image_offsets() {
        let mut assembler = Assembler::new();
        assembler
            .data_address(Register::Rdx, 0x1_0000_1234)
            .expect("far data address emits");
        let text = assembler.finish().expect("text finishes");
        assert_eq!(&text.bytes[..2], &[0x48, 0xba]);
        assert_eq!(
            u64::from_le_bytes(text.bytes[2..10].try_into().expect("immediate slice")),
            0x1_0000_1234
        );
        assert_eq!(&text.bytes[10..], &[0x4c, 0x01, 0xf2]);
    }

    #[test]
    fn metadata_counting_writer_preserves_u64_positions_beyond_u32() {
        let base = u64::from(u32::MAX) + 65_537;
        let mut counter = CountingWriteSeek::default();

        assert_eq!(
            counter.seek(SeekFrom::Start(base)).unwrap(),
            base,
            "the metadata sizing pass must retain the full sparse base"
        );
        counter.write_all(b"ARCHEECS").unwrap();
        assert_eq!(counter.byte_len(), base + 8);
        assert_eq!(counter.seek(SeekFrom::End(-1)).unwrap(), base + 7);

        counter.seek(SeekFrom::Start(u64::MAX)).unwrap();
        let error = counter
            .write_all(&[0])
            .expect_err("a metadata extent beyond u64 must fail explicitly");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("overflows u64"));
    }

    #[test]
    fn rich_m26_fixture_plans_every_native_function() {
        let core = verified(include_str!("../../../examples/m26_closure.arc"));
        let plan = plan_native(&core).expect("rich native plan builds");
        assert_eq!(plan.native_layout.functions.len(), 3);
        assert!(plan.native_layout.code_range.byte_len > 0);
        assert!(plan.data_memory_byte_len > 0);
        assert!(plan.world.max_rows >= 4);
        assert!(plan
            .native_layout
            .functions
            .iter()
            .all(|function| function.code_byte_len != 0));
    }

    #[test]
    fn system_functions_preserve_r12_and_align_runtime_calls() {
        let core = verified(include_str!("../../../examples/m26_closure.arc"));
        let plan = plan_native(&core).expect("rich native plan builds");
        for function in &plan.native_layout.functions {
            if !matches!(function.target, NativeFunctionTarget::System(_)) {
                continue;
            }
            let start = usize::try_from(function.code_offset - TEXT_IMAGE_OFFSET)
                .expect("system start fits host address space");
            let end = start
                .checked_add(
                    usize::try_from(function.code_byte_len)
                        .expect("system length fits host address space"),
                )
                .expect("system range fits host address space");
            let code = &plan.text[start..end];
            assert!(
                code.windows(2).any(|window| window == [0x41, 0x54]),
                "system must push callee-saved r12 before using it"
            );
            assert_eq!(
                code.get(code.len().saturating_sub(3)..),
                Some([0x41, 0x5c, 0xc3].as_slice()),
                "system must restore r12 immediately before returning"
            );
        }
    }

    #[test]
    fn rich_m26_fixture_finalizes_segmented_v2_image() {
        let core = verified(include_str!("../../../examples/m26_closure.arc"));
        let plan = plan_native(&core).expect("rich native plan builds");
        let package = crate::execution_package_build::build_execution_package(
            &core,
            "m26_closure.arc",
            plan.native_code_layout(),
        )
        .expect("rich v2 package builds");
        let image = finalize_native(plan, &core, &package).expect("native image finalizes");
        let mut artifact = std::io::Cursor::new(Vec::new());
        let layout = image
            .write_static_pie(&mut artifact, 0)
            .expect("segmented static PIE writes");
        assert_eq!(&artifact.get_ref()[..4], b"\x7fELF");
        let metadata_start = usize::try_from(layout.metadata_offset)
            .expect("metadata offset fits the test address space");
        assert_eq!(
            &artifact.get_ref()[metadata_start..metadata_start + 8],
            b"ARCHEECS"
        );
        assert!(layout.metadata_vaddr > layout.data_vaddr);
        assert!(layout.data_memory_byte_len >= layout.data_file_byte_len);
    }

    #[test]
    fn integer_trap_sites_are_linked_for_startup_and_system_code() {
        let core = verified(
            r#"world Traps
resource Counter { value: i32 }
system Break(counter: mut Counter) {
    counter.value = counter.value / 0
    counter.value = counter.value % -1
}
schedule Main { run Break }
startup {
    resource Counter { value: 1 }
    let divisor: i32 = 0
    let trapped: i32 = 1 / divisor
    run Main
    exit trapped
}
"#,
        );
        let points = collect_trap_points(&core).expect("trap points collect");
        assert_eq!(points.len(), 3);
        let plan = plan_native(&core).expect("trap program plans");
        for point in points {
            plan.runtime
                .trap_descriptor_index(point)
                .expect("every trap point links to a runtime descriptor");
        }
    }

    struct NativeParityCase {
        image: AotImage,
        process_status: i32,
        reference_stdout: Vec<u8>,
        reference_stderr: Vec<u8>,
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct ExpectedNativeOutput {
        process_status: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    }

    struct PublishedMetadataAuthorityCase {
        name: &'static str,
        artifact: Vec<u8>,
        expected: ExpectedNativeOutput,
    }

    fn native_parity_case(source: &str, source_identity: &str) -> NativeParityCase {
        native_parity_case_with_package_mutation(source, source_identity, |_| {})
    }

    fn native_parity_case_with_package_mutation(
        source: &str,
        source_identity: &str,
        mutate: impl FnOnce(&mut ExecutionPackage),
    ) -> NativeParityCase {
        let core = verified(source);
        let plan = plan_native(&core).expect("native plan builds");
        let mut package = crate::execution_package_build::build_execution_package(
            &core,
            source_identity,
            plan.native_code_layout(),
        )
        .expect("v2 execution package builds");
        mutate(&mut package);
        let mut reference_stdout = Vec::new();
        let mut reference_stderr = Vec::new();
        let reference = crate::reference_executor_v2::execute_decoded(
            &core,
            package.clone(),
            Some(plan.native_code_layout().code_range),
            &mut reference_stdout,
            &mut reference_stderr,
        )
        .expect("direct Core reference executes");
        let process_status = reference.process_status();
        let image = finalize_native(plan, &core, &package).expect("native image finalizes");
        NativeParityCase {
            image,
            process_status,
            reference_stdout,
            reference_stderr,
        }
    }

    fn assert_exact_native_result(
        output: &std::process::Output,
        expected: &NativeParityCase,
    ) -> ExpectedNativeOutput {
        assert_eq!(
            output.status.code(),
            Some(expected.process_status),
            "native status differs; stderr={} ",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::str::from_utf8(&output.stdout).expect("native stdout is canonical ASCII"),
            std::str::from_utf8(&expected.reference_stdout)
                .expect("reference stdout is canonical ASCII")
        );
        assert_eq!(
            std::str::from_utf8(&output.stderr).expect("native stderr is canonical ASCII"),
            std::str::from_utf8(&expected.reference_stderr)
                .expect("reference stderr is canonical ASCII")
        );
        ExpectedNativeOutput {
            process_status: expected.process_status,
            stdout: expected.reference_stdout.clone(),
            stderr: expected.reference_stderr.clone(),
        }
    }

    #[derive(Clone, Copy)]
    enum ExpectedScalar {
        I32(i32),
        Bool(bool),
    }

    #[derive(Clone, Copy)]
    struct ScalarMatrixCase {
        field: &'static str,
        ty: &'static str,
        expression: &'static str,
        expected: ExpectedScalar,
    }

    const I32_MATRIX: &[ScalarMatrixCase] = &[
        ScalarMatrixCase {
            field: "add_wrap",
            ty: "i32",
            expression: "2147483647 + 1",
            expected: ExpectedScalar::I32(i32::MIN),
        },
        ScalarMatrixCase {
            field: "subtract_wrap",
            ty: "i32",
            expression: "-2147483648 - 1",
            expected: ExpectedScalar::I32(i32::MAX),
        },
        ScalarMatrixCase {
            field: "multiply_wrap",
            ty: "i32",
            expression: "1073741824 * 2",
            expected: ExpectedScalar::I32(i32::MIN),
        },
        ScalarMatrixCase {
            field: "divide",
            ty: "i32",
            expression: "-7 / 3",
            expected: ExpectedScalar::I32(-2),
        },
        ScalarMatrixCase {
            field: "remainder",
            ty: "i32",
            expression: "-7 % 3",
            expected: ExpectedScalar::I32(-1),
        },
        ScalarMatrixCase {
            field: "negate_wrap",
            ty: "i32",
            expression: "-(-2147483648)",
            expected: ExpectedScalar::I32(i32::MIN),
        },
        ScalarMatrixCase {
            field: "bit_and",
            ty: "i32",
            expression: "6 & 3",
            expected: ExpectedScalar::I32(2),
        },
        ScalarMatrixCase {
            field: "bit_or",
            ty: "i32",
            expression: "4 | 1",
            expected: ExpectedScalar::I32(5),
        },
        ScalarMatrixCase {
            field: "bit_xor",
            ty: "i32",
            expression: "7 ^ 3",
            expected: ExpectedScalar::I32(4),
        },
        ScalarMatrixCase {
            field: "bit_not",
            ty: "i32",
            expression: "~0",
            expected: ExpectedScalar::I32(-1),
        },
        ScalarMatrixCase {
            field: "shift_left_masked",
            ty: "i32",
            expression: "1 << 32",
            expected: ExpectedScalar::I32(1),
        },
        ScalarMatrixCase {
            field: "shift_left_negative",
            ty: "i32",
            expression: "1 << -1",
            expected: ExpectedScalar::I32(i32::MIN),
        },
        ScalarMatrixCase {
            field: "shift_right_arithmetic",
            ty: "i32",
            expression: "-2147483648 >> 33",
            expected: ExpectedScalar::I32(-1_073_741_824),
        },
        ScalarMatrixCase {
            field: "less",
            ty: "bool",
            expression: "-1 < 0",
            expected: ExpectedScalar::Bool(true),
        },
        ScalarMatrixCase {
            field: "less_equal",
            ty: "bool",
            expression: "3 <= 3",
            expected: ExpectedScalar::Bool(true),
        },
        ScalarMatrixCase {
            field: "greater",
            ty: "bool",
            expression: "4 > 3",
            expected: ExpectedScalar::Bool(true),
        },
        ScalarMatrixCase {
            field: "greater_equal",
            ty: "bool",
            expression: "4 >= 4",
            expected: ExpectedScalar::Bool(true),
        },
        ScalarMatrixCase {
            field: "equal",
            ty: "bool",
            expression: "5 == 5",
            expected: ExpectedScalar::Bool(true),
        },
        ScalarMatrixCase {
            field: "not_equal",
            ty: "bool",
            expression: "5 != 6",
            expected: ExpectedScalar::Bool(true),
        },
    ];

    fn scalar_matrix_source() -> String {
        use std::fmt::Write as _;

        let mut source = String::from("world NativeScalarMatrix\nresource Results {\n");
        for case in I32_MATRIX {
            writeln!(source, "    {}: {}", case.field, case.ty)
                .expect("writing a String cannot fail");
        }
        source.push_str("}\nsystem Compute(results: mut Results) {\n");
        for case in I32_MATRIX {
            writeln!(source, "    results.{} = {}", case.field, case.expression)
                .expect("writing a String cannot fail");
        }
        source.push_str("}\nschedule Main { run Compute }\nstartup {\n    resource Results {\n");
        for (index, case) in I32_MATRIX.iter().enumerate() {
            let initializer = match case.expected {
                ExpectedScalar::I32(_) => "0",
                ExpectedScalar::Bool(_) => "false",
            };
            writeln!(
                source,
                "        {}: {}{}",
                case.field,
                initializer,
                if index + 1 == I32_MATRIX.len() {
                    ""
                } else {
                    ","
                }
            )
            .expect("writing a String cannot fail");
        }
        source.push_str("    }\n    run Main\n    exit 0\n}\n");
        source
    }

    fn scalar_matrix_payload() -> Vec<u8> {
        let mut bytes = Vec::new();
        for case in I32_MATRIX {
            match case.expected {
                ExpectedScalar::I32(value) => bytes.extend_from_slice(&value.to_le_bytes()),
                ExpectedScalar::Bool(value) => bytes.push(u8::from(value)),
            }
        }
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        bytes
    }

    fn assert_initialized_resource_payload(output: &ExpectedNativeOutput, payload: &[u8]) {
        use std::fmt::Write as _;

        let mut hex = String::new();
        for byte in payload {
            write!(hex, "{byte:02X}").expect("writing a String cannot fail");
        }
        let marker = format!(" INITIALIZED {} {hex}\n", payload.len());
        let stdout = std::str::from_utf8(&output.stdout).expect("observation is canonical ASCII");
        assert!(
            stdout.contains(&marker),
            "observation does not contain expected resource payload {marker:?}:\n{stdout}"
        );
    }

    const EXCLUSION_ONLY_QUERY_SOURCE: &str = r#"world NativeExclusionOnly
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
}
"#;

    const STARTUP_ADD_ASSIGN_SOURCE: &str = r#"world StartupAdd
resource Result { integer: i32 scalar: f32 }
startup {
    let mut integer: i32 = 2147483647
    integer += 2
    let mut scalar: f32 = 0.5
    scalar += 0.25
    resource Result { integer: integer, scalar: scalar }
    exit integer
}
"#;

    const BOOL_SHORT_CIRCUIT_SOURCE: &str = r#"world NativeShortCircuit
resource Results { and_safe: bool or_safe: bool }
system Compute(results: mut Results) {
    if false && (1 / 0 == 0) {
        results.and_safe = false
    } else {
        results.and_safe = true
    }
    if true || (1 % 0 == 0) {
        results.or_safe = true
    } else {
        results.or_safe = false
    }
}
schedule Main { run Compute }
startup {
    resource Results { and_safe: false, or_safe: false }
    run Main
    exit 0
}
"#;

    const F32_EDGE_SOURCE: &str = r#"world NativeFloatEdges
resource Results {
    add: f32
    subtract: f32
    multiply: f32
    divide: f32
    negate: f32
    positive_infinity: f32
    negative_zero: f32
    subnormal: f32
    canonical_nan: f32
    nan_less: bool
    nan_less_equal: bool
    nan_greater: bool
    nan_greater_equal: bool
    nan_equal: bool
    nan_not_equal: bool
}
system Compute(results: mut Results) {
    let mut tiny: f32 = 1.0
    let mut count: i32 = 0
    while count < 149 {
        tiny = tiny / 2.0
        count = count + 1
    }
    let infinity: f32 = 1.0 / 0.0
    let nan: f32 = 0.0 / 0.0
    results.add = 1.5 + 2.25
    results.subtract = 5.5 - 2.0
    results.multiply = -2.0 * 3.0
    results.divide = 7.0 / 2.0
    results.negate = -1.25
    results.positive_infinity = infinity
    results.negative_zero = -0.0 * 1.0
    results.subnormal = tiny
    results.canonical_nan = nan + 1.0
    results.nan_less = nan < 1.0
    results.nan_less_equal = nan <= 1.0
    results.nan_greater = nan > 1.0
    results.nan_greater_equal = nan >= 1.0
    results.nan_equal = nan == nan
    results.nan_not_equal = nan != nan
}
schedule Main { run Compute }
startup {
    resource Results {
        add: 0.0,
        subtract: 0.0,
        multiply: 0.0,
        divide: 0.0,
        negate: 0.0,
        positive_infinity: 0.0,
        negative_zero: 0.0,
        subnormal: 0.0,
        canonical_nan: 0.0,
        nan_less: false,
        nan_less_equal: false,
        nan_greater: false,
        nan_greater_equal: false,
        nan_equal: false,
        nan_not_equal: false
    }
    run Main
    exit 0
}
"#;

    fn f32_edge_payload() -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in [3.75_f32, 3.5, -6.0, 3.5, -1.25, f32::INFINITY, -0.0] {
            bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&crate::scalar_v2::CANONICAL_NAN_BITS.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 0, 0, 1]);
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        bytes
    }

    fn exit_source(world: &str, expression: &str) -> String {
        format!("world {world}\nstartup {{\n    exit {expression}\n}}\n")
    }

    fn trap_source(world: &str, expression: &str) -> String {
        format!(
            "world {world}\nstartup {{\n    let trapped: i32 = {expression}\n    exit trapped\n}}\n"
        )
    }

    fn while_source(world: &str, limit: i32) -> String {
        format!(
            "world {world}\nresource Count {{ value: i32 }}\nsystem Run(count: mut Count) {{\n    let mut index: i32 = 0\n    while index < {limit} {{\n        count.value = count.value + 1\n        index = index + 1\n    }}\n}}\nschedule Main {{ run Run }}\nstartup {{\n    resource Count {{ value: 0 }}\n    run Main\n    exit 0\n}}\n"
        )
    }

    const METADATA_AUTHORITY_SOURCE: &str = r#"world NativeMetadataAuthority
resource State { value: i32 }
system AddOne(state: mut State) { state.value = state.value + 1 }
system Double(state: mut State) { state.value = state.value * 2 }
schedule Main { run AddOne run Double }
startup {
    resource State { value: 3 }
    run Main
    exit 0
}
"#;

    const PUBLISHED_METADATA_AUTHORITY_SOURCE: &str = r#"world PublishedMetadataAuthority
resource State { value: i32 }
system AddOne(state: mut State) { state.value = state.value + 1 }
system Double(state: mut State) { state.value = state.value * 2 }
schedule AddThenDouble { run AddOne run Double }
schedule DoubleThenAdd { run Double run AddOne }
startup {
    resource State { value: 3 }
    run AddThenDouble
    run DoubleThenAdd
    exit 0
}
"#;

    const NONTERMINATION_SOURCE: &str = r#"world NativeNontermination
resource State { value: i32 }
system Spin(state: mut State) {
    while true {
        state.value = state.value + 1
    }
}
schedule Main { run Spin }
startup {
    resource State { value: 0 }
    run Main
    exit 0
}
"#;

    fn nonterminating_native_image() -> AotImage {
        let core = verified(NONTERMINATION_SOURCE);
        let plan = plan_native(&core).expect("nonterminating native plan builds");
        let package = crate::execution_package_build::build_execution_package(
            &core,
            "native_nontermination.arc",
            plan.native_code_layout(),
        )
        .expect("nonterminating execution package builds");
        finalize_native(plan, &core, &package).expect("native image finalizes")
    }

    fn assert_numeric_native_matrix(
        mut run_native: impl FnMut(NativeParityCase) -> ExpectedNativeOutput,
    ) {
        let matrix_source = scalar_matrix_source();
        let matrix = run_native(native_parity_case(
            &matrix_source,
            "native_scalar_matrix.arc",
        ));
        assert_initialized_resource_payload(&matrix, &scalar_matrix_payload());

        let short_circuit = run_native(native_parity_case(
            BOOL_SHORT_CIRCUIT_SOURCE,
            "native_short_circuit.arc",
        ));
        assert_initialized_resource_payload(&short_circuit, &[1, 1]);

        let floats = run_native(native_parity_case(
            F32_EDGE_SOURCE,
            "native_float_edges.arc",
        ));
        assert_initialized_resource_payload(&floats, &f32_edge_payload());

        for (name, expression, status) in [
            ("zero", "0", 0),
            ("source_seventy", "70", 70),
            ("maximum", "255", 255),
            ("wrapped_zero", "256", 0),
            ("negative_one", "-1", 255),
            ("wrapped_maximum", "511", 255),
        ] {
            let source = exit_source(&format!("Exit{name}"), expression);
            let output = run_native(native_parity_case(&source, &format!("exit_{name}.arc")));
            assert_eq!(output.process_status, status, "low-byte exit case {name}");
            assert!(output.stderr.is_empty());
        }

        for (name, expression, diagnostic) in [
            ("divide_zero", "1 / 0", "I32_DIVIDE_BY_ZERO"),
            ("remainder_zero", "1 % 0", "I32_REMAINDER_BY_ZERO"),
            ("divide_overflow", "-2147483648 / -1", "I32_DIVIDE_OVERFLOW"),
            (
                "remainder_overflow",
                "-2147483648 % -1",
                "I32_REMAINDER_OVERFLOW",
            ),
        ] {
            let source = trap_source(&format!("Trap{name}"), expression);
            let output = run_native(native_parity_case(&source, &format!("trap_{name}.arc")));
            assert_eq!(output.process_status, 70, "trap case {name}");
            assert!(output.stdout.starts_with(b"ARCHEOBS2\n"));
            assert!(output.stdout.ends_with(b"END\n"));
            let stderr = std::str::from_utf8(&output.stderr).expect("trap diagnostic is ASCII");
            assert!(
                stderr.contains(&format!("trap[{diagnostic}]")),
                "wrong trap diagnostic for {name}: {stderr}"
            );
        }
    }

    fn assert_bounded_while_native_parity(
        mut run_native: impl FnMut(NativeParityCase) -> ExpectedNativeOutput,
    ) {
        for (name, limit) in [("Zero", 0), ("One", 1), ("Many", 17)] {
            let source = while_source(&format!("While{name}"), limit);
            let output = run_native(native_parity_case(
                &source,
                &format!("while_{}.arc", name.to_ascii_lowercase()),
            ));
            assert_initialized_resource_payload(&output, &limit.to_le_bytes());
        }
    }

    fn assert_coherent_metadata_edits_are_native_authority(
        mut run_native: impl FnMut(NativeParityCase) -> ExpectedNativeOutput,
    ) {
        let baseline = run_native(native_parity_case(
            METADATA_AUTHORITY_SOURCE,
            "metadata_authority.arc",
        ));
        assert_initialized_resource_payload(&baseline, &8_i32.to_le_bytes());

        let payload_edit = run_native(native_parity_case_with_package_mutation(
            METADATA_AUTHORITY_SOURCE,
            "metadata_authority.arc",
            |package| {
                package
                    .payloads
                    .first_mut()
                    .expect("resource payload exists")
                    .bytes
                    .copy_from_slice(&5_i32.to_le_bytes());
            },
        ));
        assert_initialized_resource_payload(&payload_edit, &12_i32.to_le_bytes());

        let schedule_edit = run_native(native_parity_case_with_package_mutation(
            METADATA_AUTHORITY_SOURCE,
            "metadata_authority.arc",
            |package| {
                assert_eq!(package.schedule_items.len(), 2);
                package.schedule_items.swap(0, 1);
            },
        ));
        assert_initialized_resource_payload(&schedule_edit, &7_i32.to_le_bytes());

        assert_ne!(baseline.stdout, payload_edit.stdout);
        assert_ne!(baseline.stdout, schedule_edit.stdout);
        assert_ne!(payload_edit.stdout, schedule_edit.stdout);
    }

    fn persisted_metadata_range(
        layout: StaticPieLayout,
        artifact_byte_len: usize,
    ) -> std::ops::Range<usize> {
        let start = usize::try_from(layout.metadata_offset)
            .expect("metadata offset fits the test address space");
        let byte_len = usize::try_from(layout.metadata_byte_len)
            .expect("metadata length fits the test address space");
        let end = start
            .checked_add(byte_len)
            .expect("metadata range fits the test address space");
        assert!(
            end <= artifact_byte_len,
            "persisted metadata range must lie inside the finalized PIE"
        );
        start..end
    }

    fn expected_from_persisted_metadata(
        core: &VerifiedExecutableCore,
        artifact: &[u8],
        layout: StaticPieLayout,
        code_range: CodeImageRange,
    ) -> ExpectedNativeOutput {
        let metadata_range = persisted_metadata_range(layout, artifact.len());
        let mut metadata = std::io::Cursor::new(&artifact[metadata_range]);
        let decoded = archec0::execution_package_v2::decode_package_from_with_code_range(
            &mut metadata,
            code_range,
        )
        .expect("persisted ARCHEECS v2 segment decodes independently");
        crate::execution_package_build::validate_execution_package_link(
            core,
            &decoded,
            Some(code_range),
        )
        .expect("persisted ARCHEECS v2 segment links independently to verified Core");

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let outcome = crate::reference_executor_v2::execute_decoded(
            core,
            decoded,
            Some(code_range),
            &mut stdout,
            &mut stderr,
        )
        .expect("direct Core reference executes independently decoded persisted metadata");
        ExpectedNativeOutput {
            process_status: outcome.process_status(),
            stdout,
            stderr,
        }
    }

    fn patch_persisted_metadata(
        name: &'static str,
        core: &VerifiedExecutableCore,
        baseline_artifact: &[u8],
        layout: StaticPieLayout,
        code_range: CodeImageRange,
        mutate: impl FnOnce(&mut ExecutionPackage),
    ) -> PublishedMetadataAuthorityCase {
        let metadata_range = persisted_metadata_range(layout, baseline_artifact.len());
        let mut baseline_metadata =
            std::io::Cursor::new(&baseline_artifact[metadata_range.clone()]);
        let mut package = archec0::execution_package_v2::decode_package_from_with_code_range(
            &mut baseline_metadata,
            code_range,
        )
        .expect("baseline persisted ARCHEECS v2 segment decodes");
        mutate(&mut package);
        let mut encoded = std::io::Cursor::new(Vec::new());
        archec0::execution_package_v2::write_package_with_code_range(
            &mut encoded,
            &package,
            code_range,
        )
        .expect("coherently mutated package re-encodes");
        let encoded = encoded.into_inner();
        assert_eq!(
            u64::try_from(encoded.len()).expect("encoded metadata length fits u64"),
            layout.metadata_byte_len,
            "coherent mutation must preserve the persisted segment length"
        );

        let mut artifact = std::io::Cursor::new(baseline_artifact.to_vec());
        artifact
            .seek(SeekFrom::Start(layout.metadata_offset))
            .expect("published artifact seeks to its metadata segment");
        artifact
            .write_all(&encoded)
            .expect("mutated metadata overwrites the published segment");
        let artifact = artifact.into_inner();
        assert_eq!(
            &artifact[..metadata_range.start],
            &baseline_artifact[..metadata_range.start],
            "metadata patch must not alter an earlier PIE byte"
        );
        assert_ne!(
            &artifact[metadata_range.clone()],
            &baseline_artifact[metadata_range.clone()],
            "metadata authority case must alter the persisted metadata bytes"
        );
        assert_eq!(
            &artifact[metadata_range.end..],
            &baseline_artifact[metadata_range.end..],
            "metadata patch must not alter a later PIE byte"
        );
        assert_eq!(
            artifact.len(),
            baseline_artifact.len(),
            "metadata patch must preserve the finalized PIE length"
        );

        let expected = expected_from_persisted_metadata(core, &artifact, layout, code_range);
        PublishedMetadataAuthorityCase {
            name,
            artifact,
            expected,
        }
    }

    fn schedule_reference_by_name(
        package: &ExecutionPackage,
        expected_name: &str,
    ) -> archec0::execution_package_v2::ScheduleRef {
        let index = package
            .schedules
            .iter()
            .position(|schedule| {
                let name = usize::try_from(schedule.name.index())
                    .ok()
                    .and_then(|index| package.strings.get(index));
                name.is_some_and(|name| name == expected_name)
            })
            .unwrap_or_else(|| panic!("schedule `{expected_name}` exists in persisted metadata"));
        archec0::execution_package_v2::ScheduleRef::new(
            u64::try_from(index).expect("schedule index fits u64"),
        )
    }

    fn published_metadata_authority_cases() -> [PublishedMetadataAuthorityCase; 4] {
        let core = verified(PUBLISHED_METADATA_AUTHORITY_SOURCE);
        let plan = plan_native(&core).expect("published-metadata native plan builds");
        let code_range = plan.native_code_layout().code_range;
        let package = crate::execution_package_build::build_execution_package(
            &core,
            "published_metadata_authority.arc",
            plan.native_code_layout(),
        )
        .expect("published-metadata v2 execution package builds");
        let image = finalize_native(plan, &core, &package)
            .expect("published-metadata native image finalizes once");
        let mut baseline_artifact = std::io::Cursor::new(Vec::new());
        let layout = image
            .write_static_pie(&mut baseline_artifact, 0)
            .expect("baseline published-metadata PIE writes once");
        let baseline_artifact = baseline_artifact.into_inner();
        let metadata_range = persisted_metadata_range(layout, baseline_artifact.len());
        assert_eq!(
            &baseline_artifact[metadata_range.start..metadata_range.start + 8],
            b"ARCHEECS",
            "StaticPieLayout must locate the persisted v2 segment"
        );

        let baseline = PublishedMetadataAuthorityCase {
            name: "baseline",
            expected: expected_from_persisted_metadata(
                &core,
                &baseline_artifact,
                layout,
                code_range,
            ),
            artifact: baseline_artifact.clone(),
        };
        let payload = patch_persisted_metadata(
            "payload bytes",
            &core,
            &baseline_artifact,
            layout,
            code_range,
            |package| {
                package
                    .payloads
                    .first_mut()
                    .expect("resource payload exists")
                    .bytes
                    .copy_from_slice(&5_i32.to_le_bytes());
            },
        );
        let schedule_items = patch_persisted_metadata(
            "schedule item order",
            &core,
            &baseline_artifact,
            layout,
            code_range,
            |package| {
                let schedule = schedule_reference_by_name(package, "AddThenDouble");
                let items: Vec<usize> = package
                    .schedule_items
                    .iter()
                    .enumerate()
                    .filter_map(|(index, item)| (item.schedule == schedule).then_some(index))
                    .collect();
                assert_eq!(items.len(), 2, "selected schedule has two persisted rows");
                package.schedule_items.swap(items[0], items[1]);
            },
        );
        let startup_schedules = patch_persisted_metadata(
            "startup schedule order",
            &core,
            &baseline_artifact,
            layout,
            code_range,
            |package| {
                let add_then_double = schedule_reference_by_name(package, "AddThenDouble");
                let double_then_add = schedule_reference_by_name(package, "DoubleThenAdd");
                let operation_index = |schedule| {
                    package
                        .startup_operations
                        .iter()
                        .position(|operation| {
                            matches!(
                                operation.kind,
                                archec0::execution_package_v2::StartupOperationKind::RunSchedule {
                                    schedule: actual
                                } if actual == schedule
                            )
                        })
                        .expect("startup schedule operation exists")
                };
                let first = operation_index(add_then_double);
                let second = operation_index(double_then_add);
                package.startup_operations.swap(first, second);
            },
        );

        [baseline, payload, schedule_items, startup_schedules]
    }

    fn assert_published_metadata_reference_expectations(
        cases: &[PublishedMetadataAuthorityCase; 4],
    ) {
        for (case, value) in cases.iter().zip([17_i32, 25, 15, 16]) {
            assert_eq!(case.expected.process_status, 0, "{} status", case.name);
            assert!(case.expected.stderr.is_empty(), "{} stderr", case.name);
            assert_initialized_resource_payload(&case.expected, &value.to_le_bytes());
        }
        for left in 0..cases.len() {
            for right in left + 1..cases.len() {
                assert_ne!(
                    cases[left].expected.stdout, cases[right].expected.stdout,
                    "{} and {} must produce distinct observations",
                    cases[left].name, cases[right].name
                );
            }
        }
    }

    fn assert_exact_published_native_result(
        output: &std::process::Output,
        expected: &PublishedMetadataAuthorityCase,
    ) {
        assert_eq!(
            output.status.code(),
            Some(expected.expected.process_status),
            "{} native status differs; stderr={}",
            expected.name,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.stdout, expected.expected.stdout,
            "{} native stdout differs from the direct Core reference",
            expected.name
        );
        assert_eq!(
            output.stderr, expected.expected.stderr,
            "{} native stderr differs from the direct Core reference",
            expected.name
        );
    }

    #[test]
    fn persisted_metadata_mutations_patch_only_the_finalized_pie_segment() {
        let cases = published_metadata_authority_cases();
        assert_published_metadata_reference_expectations(&cases);
    }

    #[cfg(target_os = "linux")]
    struct TemporaryNativeArtifact(std::path::PathBuf);

    #[cfg(target_os = "linux")]
    impl Drop for TemporaryNativeArtifact {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[cfg(target_os = "linux")]
    fn write_linux_native_image(image: &AotImage) -> TemporaryNativeArtifact {
        use std::fs::OpenOptions;
        use std::os::unix::fs::PermissionsExt;
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "archec0-m26-aot-{}-{unique}.elf",
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("temporary artifact opens");
        image
            .write_static_pie(&mut file, 0)
            .expect("native PIE writes");
        drop(file);
        let mut permissions = std::fs::metadata(&path)
            .expect("artifact metadata reads")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&path, permissions).expect("artifact becomes executable");
        TemporaryNativeArtifact(path)
    }

    #[cfg(target_os = "linux")]
    fn write_linux_published_artifact(bytes: &[u8]) -> TemporaryNativeArtifact {
        use std::fs::OpenOptions;
        use std::os::unix::fs::PermissionsExt;
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "archec0-m26-published-metadata-{}-{unique}.elf",
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("temporary published artifact opens");
        file.write_all(bytes)
            .expect("published artifact bytes write exactly");
        drop(file);
        let mut permissions = std::fs::metadata(&path)
            .expect("published artifact metadata reads")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&path, permissions)
            .expect("published artifact becomes executable");
        TemporaryNativeArtifact(path)
    }

    #[cfg(target_os = "linux")]
    fn assert_linux_native_case(expected: NativeParityCase) -> ExpectedNativeOutput {
        use std::process::Command;

        let _execution_guard = crate::lock_linux_test_artifact_execution();
        let artifact = write_linux_native_image(&expected.image);
        let output = Command::new(&artifact.0)
            .output()
            .expect("native PIE executes");
        let result = assert_exact_native_result(&output, &expected);
        drop(artifact);
        result
    }

    #[cfg(target_os = "linux")]
    fn assert_linux_native_parity(source: &str, source_identity: &str) -> ExpectedNativeOutput {
        assert_linux_native_case(native_parity_case(source, source_identity))
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rich_m26_native_matches_direct_core_reference() {
        assert_linux_native_parity(
            include_str!("../../../examples/m26_closure.arc"),
            "m26_closure.arc",
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn arena_native_matches_direct_core_reference() {
        assert_linux_native_parity(
            include_str!("../../../examples/arena_recovery.arc"),
            "arena_recovery.arc",
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn trap_native_matches_exact_direct_core_observation_and_diagnostic() {
        let expected = native_parity_case(
            include_str!("../../../examples/m26_trap.arc"),
            "m26_trap.arc",
        );
        assert_eq!(expected.process_status, 70);
        assert!(expected.reference_stdout.starts_with(b"ARCHEOBS2\n"));
        assert!(expected.reference_stdout.ends_with(b"END\n"));
        assert!(expected.reference_stderr.starts_with(b"arche: trap["));
        assert_linux_native_parity(
            include_str!("../../../examples/m26_trap.arc"),
            "m26_trap.arc",
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn numeric_edges_match_direct_core_reference_in_production_pies() {
        assert_numeric_native_matrix(assert_linux_native_case);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn zero_one_and_many_while_iterations_match_direct_core_reference() {
        assert_bounded_while_native_parity(assert_linux_native_case);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn exclusion_only_query_with_zero_bindings_matches_direct_core_reference() {
        let output = assert_linux_native_parity(EXCLUSION_ONLY_QUERY_SOURCE, "exclusion_only.arc");
        assert_initialized_resource_payload(&output, &2_i32.to_le_bytes());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn startup_add_assign_matches_direct_core_reference() {
        let output = assert_linux_native_parity(STARTUP_ADD_ASSIGN_SOURCE, "startup_add.arc");
        assert_eq!(output.process_status, 1);
        assert_initialized_resource_payload(
            &output,
            &[0x01, 0x00, 0x00, 0x80, 0x00, 0x00, 0x40, 0x3f],
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn coherent_payload_and_schedule_edits_change_reference_and_native_identically() {
        assert_coherent_metadata_edits_are_native_authority(assert_linux_native_case);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn persisted_metadata_edits_match_direct_core_in_the_same_patched_pies() {
        use std::process::Command;

        let _execution_guard = crate::lock_linux_test_artifact_execution();
        let cases = published_metadata_authority_cases();
        assert_published_metadata_reference_expectations(&cases);
        for case in &cases {
            let artifact = write_linux_published_artifact(&case.artifact);
            let output = Command::new(&artifact.0)
                .output()
                .expect("patched published PIE executes");
            assert_exact_published_native_result(&output, case);
            drop(artifact);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn intentional_nontermination_is_bounded_only_by_an_external_timeout() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};

        let _execution_guard = crate::lock_linux_test_artifact_execution();
        let image = nonterminating_native_image();
        let artifact = write_linux_native_image(&image);
        let mut child = Command::new(&artifact.0)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("nonterminating native PIE starts");
        let deadline = Instant::now() + Duration::from_millis(250);
        loop {
            assert!(
                child
                    .try_wait()
                    .expect("nonterminating native PIE can be polled")
                    .is_none(),
                "native while loop terminated without an external limit"
            );
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        child
            .kill()
            .expect("external timeout kills nonterminating native PIE");
        child
            .wait()
            .expect("externally killed native PIE can be reaped");
        drop(artifact);
    }

    #[cfg(target_os = "windows")]
    struct TemporaryWslNativeArtifact {
        windows_path: std::path::PathBuf,
        linux_path: String,
    }

    #[cfg(target_os = "windows")]
    impl Drop for TemporaryWslNativeArtifact {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.windows_path);
        }
    }

    #[cfg(target_os = "windows")]
    fn write_wsl_native_image(image: &AotImage) -> TemporaryWslNativeArtifact {
        use std::fs::OpenOptions;
        use std::process::Command;
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "archec0-m26-aot-{}-{unique}.elf",
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("temporary artifact opens");
        image
            .write_static_pie(&mut file, 0)
            .expect("native PIE writes");
        drop(file);

        let portable_windows_path = path.to_string_lossy().replace('\\', "/");
        let translated = Command::new("wsl.exe")
            .args(["wslpath", "-a", "-u"])
            .arg(&portable_windows_path)
            .output()
            .expect("WSL translates the artifact path");
        assert!(
            translated.status.success(),
            "wslpath failed: {}",
            String::from_utf8_lossy(&translated.stderr)
        );
        let linux_path = String::from_utf8(translated.stdout)
            .expect("wslpath emits UTF-8")
            .trim()
            .to_owned();
        let chmod = Command::new("wsl.exe")
            .args(["chmod", "700", &linux_path])
            .output()
            .expect("WSL chmod executes");
        assert!(
            chmod.status.success(),
            "WSL chmod failed: {}",
            String::from_utf8_lossy(&chmod.stderr)
        );
        TemporaryWslNativeArtifact {
            windows_path: path,
            linux_path,
        }
    }

    #[cfg(target_os = "windows")]
    fn write_wsl_published_artifact(bytes: &[u8]) -> TemporaryWslNativeArtifact {
        use std::fs::OpenOptions;
        use std::process::Command;
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "archec0-m26-published-metadata-{}-{unique}.elf",
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("temporary published artifact opens");
        file.write_all(bytes)
            .expect("published artifact bytes write exactly");
        drop(file);

        let portable_windows_path = path.to_string_lossy().replace('\\', "/");
        let translated = Command::new("wsl.exe")
            .args(["wslpath", "-a", "-u"])
            .arg(&portable_windows_path)
            .output()
            .expect("WSL translates the published artifact path");
        assert!(
            translated.status.success(),
            "wslpath failed: {}",
            String::from_utf8_lossy(&translated.stderr)
        );
        let linux_path = String::from_utf8(translated.stdout)
            .expect("wslpath emits UTF-8")
            .trim()
            .to_owned();
        let chmod = Command::new("wsl.exe")
            .args(["chmod", "700", &linux_path])
            .output()
            .expect("WSL chmod executes");
        assert!(
            chmod.status.success(),
            "WSL chmod failed: {}",
            String::from_utf8_lossy(&chmod.stderr)
        );
        TemporaryWslNativeArtifact {
            windows_path: path,
            linux_path,
        }
    }

    #[cfg(target_os = "windows")]
    fn assert_wsl_native_case(expected: NativeParityCase) -> ExpectedNativeOutput {
        use std::process::Command;

        let artifact = write_wsl_native_image(&expected.image);
        let output = Command::new("wsl.exe")
            .arg(&artifact.linux_path)
            .output()
            .expect("native PIE executes in WSL");
        let result = assert_exact_native_result(&output, &expected);
        drop(artifact);
        result
    }

    #[cfg(target_os = "windows")]
    fn assert_wsl_native_parity(source: &str, source_identity: &str) -> ExpectedNativeOutput {
        assert_wsl_native_case(native_parity_case(source, source_identity))
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an installed WSL 2 distribution"]
    fn rich_m26_native_matches_direct_core_reference_via_wsl() {
        assert_wsl_native_parity(
            include_str!("../../../examples/m26_closure.arc"),
            "m26_closure.arc",
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an installed WSL 2 distribution"]
    fn arena_native_matches_direct_core_reference_via_wsl() {
        assert_wsl_native_parity(
            include_str!("../../../examples/arena_recovery.arc"),
            "arena_recovery.arc",
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an installed WSL 2 distribution"]
    fn trap_native_matches_direct_core_reference_via_wsl() {
        assert_wsl_native_parity(
            include_str!("../../../examples/m26_trap.arc"),
            "m26_trap.arc",
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an installed WSL 2 distribution"]
    fn numeric_edges_match_direct_core_reference_via_wsl() {
        assert_numeric_native_matrix(assert_wsl_native_case);
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an installed WSL 2 distribution"]
    fn while_iteration_matrix_matches_direct_core_reference_via_wsl() {
        assert_bounded_while_native_parity(assert_wsl_native_case);
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an installed WSL 2 distribution"]
    fn exclusion_only_query_with_zero_bindings_matches_direct_core_reference_via_wsl() {
        let output = assert_wsl_native_parity(EXCLUSION_ONLY_QUERY_SOURCE, "exclusion_only.arc");
        assert_initialized_resource_payload(&output, &2_i32.to_le_bytes());
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an installed WSL 2 distribution"]
    fn startup_add_assign_matches_direct_core_reference_via_wsl() {
        let output = assert_wsl_native_parity(STARTUP_ADD_ASSIGN_SOURCE, "startup_add.arc");
        assert_eq!(output.process_status, 1);
        assert_initialized_resource_payload(
            &output,
            &[0x01, 0x00, 0x00, 0x80, 0x00, 0x00, 0x40, 0x3f],
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an installed WSL 2 distribution"]
    fn coherent_metadata_edits_match_direct_core_reference_via_wsl() {
        assert_coherent_metadata_edits_are_native_authority(assert_wsl_native_case);
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an installed WSL 2 distribution"]
    fn persisted_metadata_edits_match_direct_core_in_the_same_patched_pies_via_wsl() {
        use std::process::Command;

        let cases = published_metadata_authority_cases();
        assert_published_metadata_reference_expectations(&cases);
        for case in &cases {
            let artifact = write_wsl_published_artifact(&case.artifact);
            let output = Command::new("wsl.exe")
                .arg(&artifact.linux_path)
                .output()
                .expect("patched published PIE executes in WSL");
            assert_exact_published_native_result(&output, case);
            drop(artifact);
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an installed WSL 2 distribution"]
    fn intentional_nontermination_requires_an_external_wsl_timeout() {
        use std::process::Command;

        let image = nonterminating_native_image();
        let artifact = write_wsl_native_image(&image);
        let output = Command::new("wsl.exe")
            .args(["timeout", "--signal=KILL", "0.25s"])
            .arg(&artifact.linux_path)
            .output()
            .expect("WSL external timeout executes");
        assert!(
            matches!(output.status.code(), Some(9 | 124 | 137)),
            "nonterminating native PIE did not reach the external timeout: status={:?}, stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        drop(artifact);
    }
}
