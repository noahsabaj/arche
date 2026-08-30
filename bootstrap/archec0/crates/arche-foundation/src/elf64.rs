use std::io::{self, Seek, SeekFrom, Write};

const ELF_HEADER_SIZE: u16 = 64;
const PROGRAM_HEADER_SIZE: u16 = 56;

const STATIC_PIE_PROGRAM_HEADER_COUNT: u16 = 5;
const STATIC_PIE_PAGE_SIZE: u64 = 0x1000;
const STATIC_PIE_TEXT_OFFSET: u64 = STATIC_PIE_PAGE_SIZE;
#[cfg(test)]
const DEFAULT_STATIC_PIE_DATA_BYTES: u64 = STATIC_PIE_PAGE_SIZE;
const PT_LOAD: u32 = 1;
const PT_GNU_STACK: u32 = 0x6474_e551;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;
const METADATA_ANCHOR_PLACEHOLDER: [u8; 8] = *b"ARCHMETA";
#[cfg(test)]
const METADATA_ANCHOR_STUB_BYTE_LEN: usize = 20;
const METADATA_ANCHOR_BASE_OFFSET: u64 = 7;
const METADATA_ANCHOR_DELTA_OFFSET: u64 = 9;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticPieLayout {
    pub entry_point: u64,
    pub text_offset: u64,
    pub text_vaddr: u64,
    pub text_byte_len: u64,
    pub data_offset: u64,
    pub data_vaddr: u64,
    pub data_file_byte_len: u64,
    pub data_memory_byte_len: u64,
    pub metadata_offset: u64,
    pub metadata_vaddr: u64,
    pub metadata_byte_len: u64,
    pub file_byte_len: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetadataAnchorRelocation {
    /// Offset of the signed 64-bit immediate within the text segment.
    pub immediate_text_offset: u64,
    /// Text-relative address used as the base of the image-relative delta.
    pub anchor_text_offset: u64,
    /// Signed displacement already applied by the instruction that establishes
    /// the anchor (for example, the displacement of a RIP-relative `lea`).
    pub anchor_addend: i64,
}

#[derive(Clone, Copy, Debug)]
pub struct StaticPieRequest<'a> {
    pub entry_text_offset: u64,
    pub text_file_byte_len: u64,
    pub data_file_byte_len: u64,
    pub data_memory_byte_len: u64,
    pub metadata_file_byte_len: u64,
    pub minimum_metadata_offset: u64,
    pub metadata_anchor_relocations: &'a [MetadataAnchorRelocation],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticPiePlan {
    layout: StaticPieLayout,
    metadata_anchor_relocations: Vec<MetadataAnchorRelocation>,
}

impl StaticPiePlan {
    pub fn layout(&self) -> StaticPieLayout {
        self.layout
    }

    pub fn metadata_anchor_relocations(&self) -> &[MetadataAnchorRelocation] {
        &self.metadata_anchor_relocations
    }
}

pub trait WriteSeek: Write + Seek {}

impl<T: Write + Seek + ?Sized> WriteSeek for T {}

/// Validate all sizes, offsets, segment relationships, and explicit
/// image-relative metadata relocations before any output is mutated.
pub fn plan_static_pie(request: StaticPieRequest<'_>) -> io::Result<StaticPiePlan> {
    if request.text_file_byte_len == 0 {
        return Err(invalid_input("static PIE text payload must not be empty"));
    }
    if request.entry_text_offset >= request.text_file_byte_len {
        return Err(invalid_input(
            "static PIE entry point must be inside the text segment",
        ));
    }
    if request.data_file_byte_len > request.data_memory_byte_len {
        return Err(invalid_input(
            "static PIE data file bytes exceed data memory bytes",
        ));
    }

    let text_end = STATIC_PIE_TEXT_OFFSET
        .checked_add(request.text_file_byte_len)
        .ok_or_else(|| invalid_data("static PIE text range overflows u64"))?;
    let data_offset = align_up(text_end, STATIC_PIE_PAGE_SIZE)?;
    let data_file_end = data_offset
        .checked_add(request.data_file_byte_len)
        .ok_or_else(|| invalid_data("static PIE data file range overflows u64"))?;
    let data_memory_end = data_offset
        .checked_add(request.data_memory_byte_len)
        .ok_or_else(|| invalid_data("static PIE data memory range overflows u64"))?;
    let minimum_after_data = align_up(data_memory_end, STATIC_PIE_PAGE_SIZE)?;
    let requested_metadata_offset =
        align_up(request.minimum_metadata_offset, STATIC_PIE_PAGE_SIZE)?;
    let metadata_offset = minimum_after_data.max(requested_metadata_offset);
    let metadata_end = metadata_offset
        .checked_add(request.metadata_file_byte_len)
        .ok_or_else(|| invalid_data("static PIE metadata range overflows u64"))?;
    let entry_point = STATIC_PIE_TEXT_OFFSET
        .checked_add(request.entry_text_offset)
        .ok_or_else(|| invalid_data("static PIE entry point overflows u64"))?;
    let header_bytes = static_pie_header_bytes()?;
    let file_byte_len = header_bytes.max(text_end);
    let file_byte_len = if request.data_file_byte_len == 0 {
        file_byte_len
    } else {
        file_byte_len.max(data_file_end)
    };
    let file_byte_len = if request.metadata_file_byte_len == 0 {
        file_byte_len
    } else {
        file_byte_len.max(metadata_end)
    };

    let mut relocation_ranges = Vec::with_capacity(request.metadata_anchor_relocations.len());
    for relocation in request.metadata_anchor_relocations {
        let immediate_end = relocation
            .immediate_text_offset
            .checked_add(8)
            .ok_or_else(|| invalid_data("metadata anchor immediate range overflows u64"))?;
        if immediate_end > request.text_file_byte_len {
            return Err(invalid_input(
                "metadata anchor immediate is outside the text segment",
            ));
        }
        if relocation.anchor_text_offset > request.text_file_byte_len {
            return Err(invalid_input(
                "metadata anchor base is outside the text segment",
            ));
        }
        let anchor = checked_add_signed(
            STATIC_PIE_TEXT_OFFSET
                .checked_add(relocation.anchor_text_offset)
                .ok_or_else(|| invalid_data("metadata anchor base overflows u64"))?,
            relocation.anchor_addend,
            "metadata anchor addend places the base outside the image",
        )?;
        i64::try_from(i128::from(metadata_offset) - i128::from(anchor))
            .map_err(|_| invalid_data("metadata image-relative delta exceeds signed 64-bit"))?;
        relocation_ranges.push((relocation.immediate_text_offset, immediate_end));
    }
    relocation_ranges.sort_unstable();
    if relocation_ranges
        .windows(2)
        .any(|pair| pair[0].1 > pair[1].0)
    {
        return Err(invalid_input(
            "metadata anchor immediate ranges must not overlap",
        ));
    }

    Ok(StaticPiePlan {
        layout: StaticPieLayout {
            entry_point,
            text_offset: STATIC_PIE_TEXT_OFFSET,
            text_vaddr: STATIC_PIE_TEXT_OFFSET,
            text_byte_len: request.text_file_byte_len,
            data_offset,
            data_vaddr: data_offset,
            data_file_byte_len: request.data_file_byte_len,
            data_memory_byte_len: request.data_memory_byte_len,
            metadata_offset,
            metadata_vaddr: metadata_offset,
            metadata_byte_len: request.metadata_file_byte_len,
            file_byte_len,
        },
        metadata_anchor_relocations: request.metadata_anchor_relocations.to_vec(),
    })
}

/// Stream a checked static-PIE plan. Each producer receives a segment-relative
/// writer that implements both [`Write`] and [`Seek`], rejects access outside
/// its declared segment, and permits seek-created sparse ranges.
pub fn write_static_pie<Output, TextProducer, DataProducer, MetadataProducer>(
    output: &mut Output,
    plan: &StaticPiePlan,
    text_producer: TextProducer,
    data_producer: DataProducer,
    metadata_producer: MetadataProducer,
) -> io::Result<StaticPieLayout>
where
    Output: Write + Seek,
    TextProducer: FnOnce(&mut dyn WriteSeek) -> io::Result<()>,
    DataProducer: FnOnce(&mut dyn WriteSeek) -> io::Result<()>,
    MetadataProducer: FnOnce(&mut dyn WriteSeek) -> io::Result<()>,
{
    let layout = plan.layout();
    output.seek(SeekFrom::Start(0))?;
    write_static_pie_header(output, layout)?;
    write_static_pie_program_headers(output, layout)?;

    produce_segment(
        output,
        layout.text_offset,
        layout.text_byte_len,
        "text",
        text_producer,
    )?;
    patch_metadata_anchors(output, plan)?;
    produce_segment(
        output,
        layout.data_offset,
        layout.data_file_byte_len,
        "data",
        data_producer,
    )?;
    produce_segment(
        output,
        layout.metadata_offset,
        layout.metadata_byte_len,
        "metadata",
        metadata_producer,
    )?;
    output.seek(SeekFrom::Start(layout.file_byte_len))?;

    Ok(layout)
}

/// Stream a segmented x86-64 Linux static PIE.
///
/// The image contains distinct R-- headers, R-X text, RW data, and R--
/// metadata mappings plus a non-executable GNU stack declaration. Virtual
/// addresses are image-relative, so ASLR may choose the load base.
#[cfg(test)]
pub fn write_static_pie_with_metadata(
    output: &mut (impl Write + Seek),
    text_payload: &[u8],
    metadata_payload: &[u8],
) -> io::Result<StaticPieLayout> {
    write_static_pie_with_metadata_at(output, text_payload, metadata_payload, 0)
}

/// Stream a segmented static PIE while placing metadata no earlier than the
/// requested file offset. The offset is rounded up to a page boundary and any
/// intervening file range is created with seeking, so callers can exercise
/// large-file layouts without materializing the hole.
#[cfg(test)]
pub fn write_static_pie_with_metadata_at(
    output: &mut (impl Write + Seek),
    text_payload: &[u8],
    metadata_payload: &[u8],
    minimum_metadata_offset: u64,
) -> io::Result<StaticPieLayout> {
    let text_byte_len = u64::try_from(text_payload.len())
        .map_err(|_| invalid_data("text payload length exceeds u64"))?;
    let metadata_byte_len = u64::try_from(metadata_payload.len())
        .map_err(|_| invalid_data("metadata payload length exceeds u64"))?;
    let metadata_anchor_relocations = discover_far_metadata_anchors(text_payload)?;
    let plan = plan_static_pie(StaticPieRequest {
        entry_text_offset: 0,
        text_file_byte_len: text_byte_len,
        data_file_byte_len: DEFAULT_STATIC_PIE_DATA_BYTES,
        data_memory_byte_len: DEFAULT_STATIC_PIE_DATA_BYTES,
        metadata_file_byte_len: metadata_byte_len,
        minimum_metadata_offset,
        metadata_anchor_relocations: &metadata_anchor_relocations,
    })?;

    write_static_pie(
        output,
        &plan,
        |segment| segment.write_all(text_payload),
        |segment| write_zeroes(segment, DEFAULT_STATIC_PIE_DATA_BYTES),
        |segment| segment.write_all(metadata_payload),
    )
}

/// Emit a position-independent metadata-address materialization sequence:
///
/// `lea rsi, [rip] ; movabs rax, image_delta ; add rsi, rax`
///
/// The ELF writer replaces the placeholder with a checked signed 64-bit
/// image-relative delta. This remains valid when metadata is outside rel32
/// range and does not embed an ASLR-sensitive absolute address.
pub fn emit_metadata_anchor_stub(bytes: &mut Vec<u8>) -> MetadataAnchorRelocation {
    let stub_offset =
        u64::try_from(bytes.len()).expect("x86-64 Vec lengths are representable as u64");
    bytes.extend_from_slice(&[0x48, 0x8d, 0x35, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0x48, 0xb8]);
    bytes.extend_from_slice(&METADATA_ANCHOR_PLACEHOLDER);
    bytes.extend_from_slice(&[0x48, 0x01, 0xc6]);
    MetadataAnchorRelocation {
        immediate_text_offset: stub_offset + METADATA_ANCHOR_DELTA_OFFSET,
        anchor_text_offset: stub_offset + METADATA_ANCHOR_BASE_OFFSET,
        anchor_addend: 0,
    }
}

fn write_static_pie_header(output: &mut impl Write, layout: StaticPieLayout) -> io::Result<()> {
    output.write_all(&[
        0x7f, b'E', b'L', b'F', // ELF magic
        2,    // ELFCLASS64
        1,    // ELFDATA2LSB
        1,    // EV_CURRENT
        0,    // ELFOSABI_SYSV
        0,    // ABI version
        0, 0, 0, 0, 0, 0, 0, // padding
    ])?;
    write_u16(output, 3)?; // ET_DYN
    write_u16(output, 0x3e)?; // EM_X86_64
    write_u32(output, 1)?; // EV_CURRENT
    write_u64(output, layout.entry_point)?;
    write_u64(output, u64::from(ELF_HEADER_SIZE))?;
    write_u64(output, 0)?; // no section headers
    write_u32(output, 0)?;
    write_u16(output, ELF_HEADER_SIZE)?;
    write_u16(output, PROGRAM_HEADER_SIZE)?;
    write_u16(output, STATIC_PIE_PROGRAM_HEADER_COUNT)?;
    write_u16(output, 0)?;
    write_u16(output, 0)?;
    write_u16(output, 0)
}

fn write_static_pie_program_headers(
    output: &mut impl Write,
    layout: StaticPieLayout,
) -> io::Result<()> {
    let header_bytes = static_pie_header_bytes()?;

    write_program_header(
        output,
        ProgramHeader {
            kind: PT_LOAD,
            flags: PF_R,
            offset: 0,
            vaddr: 0,
            file_bytes: header_bytes,
            memory_bytes: header_bytes,
            align: STATIC_PIE_PAGE_SIZE,
        },
    )?;
    write_program_header(
        output,
        ProgramHeader {
            kind: PT_LOAD,
            flags: PF_R | PF_X,
            offset: layout.text_offset,
            vaddr: layout.text_vaddr,
            file_bytes: layout.text_byte_len,
            memory_bytes: layout.text_byte_len,
            align: STATIC_PIE_PAGE_SIZE,
        },
    )?;
    write_program_header(
        output,
        ProgramHeader {
            kind: PT_LOAD,
            flags: PF_R | PF_W,
            offset: layout.data_offset,
            vaddr: layout.data_vaddr,
            file_bytes: layout.data_file_byte_len,
            memory_bytes: layout.data_memory_byte_len,
            align: STATIC_PIE_PAGE_SIZE,
        },
    )?;
    write_program_header(
        output,
        ProgramHeader {
            kind: PT_LOAD,
            flags: PF_R,
            offset: layout.metadata_offset,
            vaddr: layout.metadata_vaddr,
            file_bytes: layout.metadata_byte_len,
            memory_bytes: layout.metadata_byte_len,
            align: STATIC_PIE_PAGE_SIZE,
        },
    )?;
    write_program_header(
        output,
        ProgramHeader {
            kind: PT_GNU_STACK,
            flags: PF_R | PF_W,
            offset: 0,
            vaddr: 0,
            file_bytes: 0,
            memory_bytes: 0,
            align: 16,
        },
    )
}

struct ProgramHeader {
    kind: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    file_bytes: u64,
    memory_bytes: u64,
    align: u64,
}

fn write_program_header(output: &mut impl Write, header: ProgramHeader) -> io::Result<()> {
    write_u32(output, header.kind)?;
    write_u32(output, header.flags)?;
    write_u64(output, header.offset)?;
    write_u64(output, header.vaddr)?;
    write_u64(output, header.vaddr)?;
    write_u64(output, header.file_bytes)?;
    write_u64(output, header.memory_bytes)?;
    write_u64(output, header.align)
}

fn static_pie_header_bytes() -> io::Result<u64> {
    u64::from(ELF_HEADER_SIZE)
        .checked_add(
            u64::from(PROGRAM_HEADER_SIZE)
                .checked_mul(u64::from(STATIC_PIE_PROGRAM_HEADER_COUNT))
                .ok_or_else(|| invalid_data("program-header table length overflows u64"))?,
        )
        .ok_or_else(|| invalid_data("ELF header range overflows u64"))
}

#[cfg(test)]
fn discover_far_metadata_anchors(text_payload: &[u8]) -> io::Result<Vec<MetadataAnchorRelocation>> {
    let mut relocations = Vec::new();
    let mut search_start = 0usize;
    while let Some(relative_offset) = text_payload[search_start..]
        .windows(METADATA_ANCHOR_STUB_BYTE_LEN)
        .position(is_metadata_anchor_stub)
    {
        let stub_offset = search_start
            .checked_add(relative_offset)
            .ok_or_else(|| invalid_data("metadata anchor text offset overflows usize"))?;
        let stub_offset_u64 = u64::try_from(stub_offset)
            .map_err(|_| invalid_data("metadata anchor text offset exceeds u64"))?;
        let existing_lea_displacement = i32::from_le_bytes(
            text_payload[stub_offset + 3..stub_offset + 7]
                .try_into()
                .map_err(|_| invalid_data("metadata anchor LEA displacement is truncated"))?,
        );
        relocations.push(MetadataAnchorRelocation {
            immediate_text_offset: stub_offset_u64
                .checked_add(METADATA_ANCHOR_DELTA_OFFSET)
                .ok_or_else(|| invalid_data("metadata anchor immediate overflows u64"))?,
            anchor_text_offset: stub_offset_u64
                .checked_add(METADATA_ANCHOR_BASE_OFFSET)
                .ok_or_else(|| invalid_data("metadata anchor base overflows u64"))?,
            anchor_addend: i64::from(existing_lea_displacement),
        });
        search_start = stub_offset
            .checked_add(METADATA_ANCHOR_STUB_BYTE_LEN)
            .ok_or_else(|| invalid_data("metadata anchor search offset overflows usize"))?;
    }

    if relocations.is_empty()
        && text_payload.starts_with(&[0x48, 0x8d, 0x35])
        && text_payload.len() >= 7
    {
        return Err(invalid_data(
            "implicit rel32 metadata anchors are unsupported; use an explicit far-safe anchor",
        ));
    }

    Ok(relocations)
}

fn patch_metadata_anchors(
    output: &mut (impl Write + Seek),
    plan: &StaticPiePlan,
) -> io::Result<()> {
    for relocation in plan.metadata_anchor_relocations() {
        let anchor_without_addend = plan
            .layout
            .text_vaddr
            .checked_add(relocation.anchor_text_offset)
            .ok_or_else(|| invalid_data("metadata anchor base overflows u64"))?;
        let anchor = checked_add_signed(
            anchor_without_addend,
            relocation.anchor_addend,
            "metadata anchor addend places the base outside the image",
        )?;
        let delta = i128::from(plan.layout.metadata_vaddr) - i128::from(anchor);
        let delta = i64::try_from(delta)
            .map_err(|_| invalid_data("metadata image-relative delta exceeds signed 64-bit"))?;
        let patch_offset = plan
            .layout
            .text_offset
            .checked_add(relocation.immediate_text_offset)
            .ok_or_else(|| invalid_data("metadata anchor file offset overflows u64"))?;
        output.seek(SeekFrom::Start(patch_offset))?;
        output.write_all(&delta.to_le_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
fn is_metadata_anchor_stub(candidate: &[u8]) -> bool {
    candidate.len() == METADATA_ANCHOR_STUB_BYTE_LEN
        && candidate[..3] == [0x48, 0x8d, 0x35]
        && candidate[7..9] == [0x48, 0xb8]
        && candidate[9..17] == METADATA_ANCHOR_PLACEHOLDER
        && candidate[17..] == [0x48, 0x01, 0xc6]
}

#[cfg(test)]
fn write_zeroes(output: &mut dyn WriteSeek, byte_len: u64) -> io::Result<()> {
    const ZEROES: [u8; 8192] = [0; 8192];
    let mut remaining = byte_len;
    while remaining != 0 {
        let chunk_byte_len = remaining.min(ZEROES.len() as u64);
        let chunk_byte_len = usize::try_from(chunk_byte_len)
            .map_err(|_| invalid_data("zero-fill chunk length exceeds usize"))?;
        output.write_all(&ZEROES[..chunk_byte_len])?;
        remaining -= chunk_byte_len as u64;
    }
    Ok(())
}

fn produce_segment<Output, Producer>(
    output: &mut Output,
    file_offset: u64,
    file_byte_len: u64,
    name: &'static str,
    producer: Producer,
) -> io::Result<()>
where
    Output: Write + Seek,
    Producer: FnOnce(&mut dyn WriteSeek) -> io::Result<()>,
{
    let mut segment = SegmentWriter::new(output, file_offset, file_byte_len)?;
    producer(&mut segment)?;
    segment.finish(name)
}

struct SegmentWriter<'a, Output> {
    output: &'a mut Output,
    file_offset: u64,
    file_byte_len: u64,
    position: u64,
    maximum_written_end: u64,
}

impl<'a, Output: Write + Seek> SegmentWriter<'a, Output> {
    fn new(output: &'a mut Output, file_offset: u64, file_byte_len: u64) -> io::Result<Self> {
        output.seek(SeekFrom::Start(file_offset))?;
        Ok(Self {
            output,
            file_offset,
            file_byte_len,
            position: 0,
            maximum_written_end: 0,
        })
    }

    fn finish(self, name: &'static str) -> io::Result<()> {
        if self.maximum_written_end != self.file_byte_len {
            return Err(invalid_input(match name {
                "text" => "text producer did not extend to its declared byte length",
                "data" => "data producer did not extend to its declared byte length",
                "metadata" => "metadata producer did not extend to its declared byte length",
                _ => "segment producer did not extend to its declared byte length",
            }));
        }
        Ok(())
    }

    fn absolute_position(&self) -> io::Result<u64> {
        self.file_offset
            .checked_add(self.position)
            .ok_or_else(|| invalid_data("segment file position overflows u64"))
    }

    fn seek_to(&mut self, position: u64) -> io::Result<u64> {
        if position > self.file_byte_len {
            return Err(invalid_input(
                "segment seek exceeds its declared byte length",
            ));
        }
        self.position = position;
        self.output
            .seek(SeekFrom::Start(self.absolute_position()?))?;
        Ok(position)
    }
}

impl<Output: Write + Seek> Write for SegmentWriter<'_, Output> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let byte_len = u64::try_from(bytes.len())
            .map_err(|_| invalid_data("segment write length exceeds u64"))?;
        let write_end = self
            .position
            .checked_add(byte_len)
            .ok_or_else(|| invalid_data("segment write range overflows u64"))?;
        if write_end > self.file_byte_len {
            return Err(invalid_input(
                "segment write exceeds its declared byte length",
            ));
        }
        self.output
            .seek(SeekFrom::Start(self.absolute_position()?))?;
        let written = self.output.write(bytes)?;
        let written =
            u64::try_from(written).map_err(|_| invalid_data("segment write result exceeds u64"))?;
        self.position = self
            .position
            .checked_add(written)
            .ok_or_else(|| invalid_data("segment position overflows u64"))?;
        self.maximum_written_end = self.maximum_written_end.max(self.position);
        usize::try_from(written).map_err(|_| invalid_data("segment write result exceeds usize"))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

impl<Output: Write + Seek> Seek for SegmentWriter<'_, Output> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let position = match position {
            SeekFrom::Start(position) => position,
            SeekFrom::Current(delta) => checked_add_signed(
                self.position,
                delta,
                "segment-relative seek is outside its declared range",
            )?,
            SeekFrom::End(delta) => checked_add_signed(
                self.file_byte_len,
                delta,
                "segment-relative seek is outside its declared range",
            )?,
        };
        self.seek_to(position)
    }
}

fn checked_add_signed(value: u64, delta: i64, message: &'static str) -> io::Result<u64> {
    let adjusted = i128::from(value) + i128::from(delta);
    u64::try_from(adjusted).map_err(|_| invalid_input(message))
}

fn align_up(value: u64, align: u64) -> io::Result<u64> {
    if align == 0 || !align.is_power_of_two() {
        return Err(invalid_data("ELF alignment must be a nonzero power of two"));
    }
    value
        .checked_add(align - 1)
        .map(|adjusted| adjusted & !(align - 1))
        .ok_or_else(|| invalid_data("ELF alignment overflows u64"))
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn write_u16(output: &mut impl Write, value: u16) -> io::Result<()> {
    output.write_all(&value.to_le_bytes())
}

fn write_u32(output: &mut impl Write, value: u32) -> io::Result<()> {
    output.write_all(&value.to_le_bytes())
}

fn write_u64(output: &mut impl Write, value: u64) -> io::Result<()> {
    output.write_all(&value.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::fs::OpenOptions;
    use std::io::{Cursor, Read};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_segmented_static_pie_with_non_executable_stack() {
        let text = [0xb8, 0x3c, 0, 0, 0, 0x31, 0xff, 0x0f, 0x05];
        let metadata = b"ARCHEECS";
        let mut output = Cursor::new(Vec::new());

        let layout =
            write_static_pie_with_metadata(&mut output, &text, metadata).expect("PIE encodes");
        let bytes = output.into_inner();

        assert_eq!(&bytes[..4], b"\x7fELF");
        assert_eq!(u16::from_le_bytes(bytes[16..18].try_into().unwrap()), 3);
        assert_eq!(
            u16::from_le_bytes(bytes[56..58].try_into().unwrap()),
            STATIC_PIE_PROGRAM_HEADER_COUNT
        );
        assert_eq!(
            &bytes[layout.metadata_offset as usize..],
            metadata.as_slice()
        );

        let headers = (0..STATIC_PIE_PROGRAM_HEADER_COUNT)
            .map(|index| {
                let offset = usize::from(ELF_HEADER_SIZE)
                    + usize::from(index) * usize::from(PROGRAM_HEADER_SIZE);
                (
                    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()),
                    u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            headers,
            [
                (PT_LOAD, PF_R),
                (PT_LOAD, PF_R | PF_X),
                (PT_LOAD, PF_R | PF_W),
                (PT_LOAD, PF_R),
                (PT_GNU_STACK, PF_R | PF_W),
            ]
        );
        assert!(!headers
            .iter()
            .any(|(_, flags)| flags & (PF_W | PF_X) == (PF_W | PF_X)));
    }

    #[test]
    fn rejects_implicit_rel32_metadata_anchor_patching() {
        let text = [0x48, 0x8d, 0x35, 0, 0, 0, 0, 0xc3];
        let mut output = Cursor::new(Vec::new());

        let error = write_static_pie_with_metadata(&mut output, &text, b"x")
            .expect_err("implicit rel32 metadata anchor is rejected");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error
            .to_string()
            .contains("implicit rel32 metadata anchors are unsupported"));
    }

    #[test]
    fn plans_and_writes_file_backed_data_with_larger_bss_memory() {
        let plan = plan_static_pie(StaticPieRequest {
            entry_text_offset: 0,
            text_file_byte_len: 1,
            data_file_byte_len: 2,
            data_memory_byte_len: STATIC_PIE_PAGE_SIZE * 2,
            metadata_file_byte_len: 1,
            minimum_metadata_offset: 0,
            metadata_anchor_relocations: &[],
        })
        .expect("BSS-aware static PIE plan validates");
        let mut output = Cursor::new(Vec::new());

        let layout = write_static_pie(
            &mut output,
            &plan,
            |segment| segment.write_all(&[0xc3]),
            |segment| segment.write_all(b"rw"),
            |segment| segment.write_all(b"m"),
        )
        .expect("BSS-aware static PIE streams");
        let bytes = output.into_inner();
        let data_header_offset =
            usize::from(ELF_HEADER_SIZE) + 2 * usize::from(PROGRAM_HEADER_SIZE);
        let file_bytes = u64::from_le_bytes(
            bytes[data_header_offset + 32..data_header_offset + 40]
                .try_into()
                .unwrap(),
        );
        let memory_bytes = u64::from_le_bytes(
            bytes[data_header_offset + 40..data_header_offset + 48]
                .try_into()
                .unwrap(),
        );

        assert_eq!(file_bytes, 2);
        assert_eq!(memory_bytes, STATIC_PIE_PAGE_SIZE * 2);
        assert!(memory_bytes > file_bytes);
        assert_eq!(layout.data_file_byte_len, file_bytes);
        assert_eq!(layout.data_memory_byte_len, memory_bytes);
        assert!(layout.metadata_offset >= layout.data_offset + memory_bytes);
    }

    #[test]
    fn zero_length_trailing_metadata_does_not_overstate_file_length() {
        let plan = plan_static_pie(StaticPieRequest {
            entry_text_offset: 0,
            text_file_byte_len: 1,
            data_file_byte_len: 0,
            data_memory_byte_len: STATIC_PIE_PAGE_SIZE,
            metadata_file_byte_len: 0,
            minimum_metadata_offset: u64::from(u32::MAX) + 1,
            metadata_anchor_relocations: &[],
        })
        .expect("zero-length trailing metadata plan validates");
        let mut output = Cursor::new(Vec::new());

        let layout = write_static_pie(
            &mut output,
            &plan,
            |segment| segment.write_all(&[0xc3]),
            |_| Ok(()),
            |_| Ok(()),
        )
        .expect("zero-length trailing segments stream");
        let bytes = output.into_inner();

        assert!(layout.metadata_offset > u64::from(u32::MAX));
        assert_eq!(u64::try_from(bytes.len()).unwrap(), layout.file_byte_len);
        assert_eq!(
            layout.file_byte_len,
            layout.text_offset + layout.text_byte_len
        );
    }

    #[test]
    fn streams_exact_segment_bytes_and_supports_segment_relative_backpatching() {
        let plan = plan_static_pie(StaticPieRequest {
            entry_text_offset: 1,
            text_file_byte_len: 3,
            data_file_byte_len: 2,
            data_memory_byte_len: 2,
            metadata_file_byte_len: 4,
            minimum_metadata_offset: 0,
            metadata_anchor_relocations: &[],
        })
        .expect("streamed static PIE plan validates");
        let mut output = Cursor::new(Vec::new());

        let layout = write_static_pie(
            &mut output,
            &plan,
            |segment| {
                segment.write_all(&[0x90, 0, 0xc3])?;
                segment.seek(SeekFrom::Start(1))?;
                segment.write_all(&[0x91])
            },
            |segment| segment.write_all(b"RW"),
            |segment| segment.write_all(b"META"),
        )
        .expect("streamed static PIE encodes");
        let bytes = output.into_inner();

        assert_eq!(u64::try_from(bytes.len()).unwrap(), layout.file_byte_len);
        assert_eq!(
            &bytes[usize::try_from(layout.text_offset).unwrap()
                ..usize::try_from(layout.text_offset + layout.text_byte_len).unwrap()],
            &[0x90, 0x91, 0xc3]
        );
        assert_eq!(
            &bytes[usize::try_from(layout.data_offset).unwrap()
                ..usize::try_from(layout.data_offset + layout.data_file_byte_len).unwrap()],
            b"RW"
        );
        assert_eq!(
            &bytes[usize::try_from(layout.metadata_offset).unwrap()..],
            b"META"
        );
        assert_eq!(layout.entry_point, layout.text_vaddr + 1);
    }

    #[test]
    fn writer_creates_large_inter_segment_holes_only_with_seeks() {
        #[derive(Default)]
        struct SparseWriter {
            position: u64,
            file_byte_len: u64,
            written_ranges: Vec<(u64, u64)>,
        }

        impl Write for SparseWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                let byte_len = u64::try_from(bytes.len()).unwrap();
                let end = self
                    .position
                    .checked_add(byte_len)
                    .ok_or_else(|| invalid_data("sparse writer position overflows"))?;
                if byte_len != 0 {
                    self.written_ranges.push((self.position, end));
                }
                self.position = end;
                self.file_byte_len = self.file_byte_len.max(end);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        impl Seek for SparseWriter {
            fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
                self.position = match position {
                    SeekFrom::Start(position) => position,
                    SeekFrom::Current(delta) => {
                        checked_add_signed(self.position, delta, "sparse writer seek underflows")?
                    }
                    SeekFrom::End(delta) => checked_add_signed(
                        self.file_byte_len,
                        delta,
                        "sparse writer seek underflows",
                    )?,
                };
                Ok(self.position)
            }
        }

        let requested_metadata_offset = u64::from(u32::MAX) + STATIC_PIE_PAGE_SIZE + 1;
        let plan = plan_static_pie(StaticPieRequest {
            entry_text_offset: 0,
            text_file_byte_len: 1,
            data_file_byte_len: 0,
            data_memory_byte_len: 0,
            metadata_file_byte_len: 1,
            minimum_metadata_offset: requested_metadata_offset,
            metadata_anchor_relocations: &[],
        })
        .expect("sparse plan validates");
        let mut output = SparseWriter::default();

        let layout = write_static_pie(
            &mut output,
            &plan,
            |segment| segment.write_all(&[0xc3]),
            |_| Ok(()),
            |segment| segment.write_all(b"M"),
        )
        .expect("sparse plan streams");
        let written_byte_len = output
            .written_ranges
            .iter()
            .map(|(start, end)| end - start)
            .sum::<u64>();

        assert!(layout.metadata_offset > u64::from(u32::MAX));
        assert_eq!(output.file_byte_len, layout.file_byte_len);
        assert!(written_byte_len < 2 * STATIC_PIE_PAGE_SIZE);
        assert!(!output.written_ranges.iter().any(|(start, end)| {
            *start < layout.metadata_offset
                && *end > layout.text_offset + layout.text_byte_len
                && *start >= layout.text_offset + layout.text_byte_len
        }));
    }

    #[test]
    fn rejects_u64_layout_overflow_before_writing() {
        let error = plan_static_pie(StaticPieRequest {
            entry_text_offset: 0,
            text_file_byte_len: u64::MAX,
            data_file_byte_len: 0,
            data_memory_byte_len: 0,
            metadata_file_byte_len: 0,
            minimum_metadata_offset: 0,
            metadata_anchor_relocations: &[],
        })
        .expect_err("overflowing text range is rejected");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("text range overflows"));
    }

    #[test]
    fn propagates_producer_failure_without_running_later_producers() {
        let plan = plan_static_pie(StaticPieRequest {
            entry_text_offset: 0,
            text_file_byte_len: 1,
            data_file_byte_len: 0,
            data_memory_byte_len: 0,
            metadata_file_byte_len: 0,
            minimum_metadata_offset: 0,
            metadata_anchor_relocations: &[],
        })
        .expect("producer-failure plan validates");
        let data_ran = Cell::new(false);
        let mut output = Cursor::new(Vec::new());

        let error = write_static_pie(
            &mut output,
            &plan,
            |_| Err(io::Error::other("text producer failed")),
            |_| {
                data_ran.set(true);
                Ok(())
            },
            |_| Ok(()),
        )
        .expect_err("producer error propagates");

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(error.to_string(), "text producer failed");
        assert!(!data_ran.get());
    }

    #[test]
    fn patches_far_safe_image_relative_metadata_anchor_beyond_four_gib() {
        let mut text = Vec::new();
        let relocation = emit_metadata_anchor_stub(&mut text);
        text.push(0xc3);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock follows the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "archec0-far-metadata-{}-{unique}.elf",
            std::process::id()
        ));
        let mut output = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .expect("sparse test artifact opens");
        let requested_metadata_offset = u64::from(u32::MAX) + STATIC_PIE_PAGE_SIZE + 1;

        let plan = plan_static_pie(StaticPieRequest {
            entry_text_offset: 0,
            text_file_byte_len: u64::try_from(text.len()).unwrap(),
            data_file_byte_len: 0,
            data_memory_byte_len: 0,
            metadata_file_byte_len: 8,
            minimum_metadata_offset: requested_metadata_offset,
            metadata_anchor_relocations: &[relocation],
        })
        .expect("explicit far-anchor plan validates");
        let layout = write_static_pie(
            &mut output,
            &plan,
            |segment| segment.write_all(&text),
            |_| Ok(()),
            |segment| segment.write_all(b"ARCHEECS"),
        )
        .expect("far metadata PIE encodes");
        let immediate_offset = layout.text_offset + METADATA_ANCHOR_DELTA_OFFSET;
        output
            .seek(SeekFrom::Start(immediate_offset))
            .expect("metadata delta seek succeeds");
        let mut encoded_delta = [0; 8];
        output
            .read_exact(&mut encoded_delta)
            .expect("metadata delta is present");
        let delta = i64::from_le_bytes(encoded_delta);

        assert!(layout.metadata_offset > u64::from(u32::MAX));
        assert_eq!(
            output.metadata().expect("artifact metadata reads").len(),
            layout.file_byte_len
        );
        assert_eq!(
            i128::from(layout.text_vaddr + METADATA_ANCHOR_BASE_OFFSET) + i128::from(delta),
            i128::from(layout.metadata_vaddr)
        );
        drop(output);
        std::fs::remove_file(path).expect("sparse test artifact removes");
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "required sparse >4-GiB PR proof"]
    fn executes_sparse_pie_with_real_v2_metadata_beyond_four_gib() {
        let _execution_guard = crate::lock_linux_test_artifact_execution();

        use std::os::linux::fs::MetadataExt;
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        struct RemoveOnDrop {
            path: std::path::PathBuf,
            preserve: bool,
        }

        impl Drop for RemoveOnDrop {
            fn drop(&mut self) {
                if self.preserve {
                    eprintln!("preserved sparse proof artifact at {}", self.path.display());
                } else {
                    let _ = std::fs::remove_file(&self.path);
                }
            }
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        struct ParsedProgramHeader {
            kind: u32,
            flags: u32,
            offset: u64,
            vaddr: u64,
            file_bytes: u64,
            memory_bytes: u64,
            align: u64,
        }

        fn command_output(command: &mut Command, label: &str) -> String {
            let output = command.output().unwrap_or_else(|error| {
                panic!("{label} could not launch: {error}");
            });
            assert!(
                output.status.success(),
                "{label} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout)
                .unwrap_or_else(|error| panic!("{label} output is not UTF-8: {error}"))
        }

        let source = "world SparseProof\nstartup { exit 47 }\n";
        let source_reader = std::io::BufReader::new(Cursor::new(source.as_bytes()));
        let program = crate::parser::parse_lexer(crate::lexer::Lexer::new(source_reader))
            .expect("ordinary sparse-proof source parses through the streaming frontend");
        crate::checker::check_program(&program)
            .expect("ordinary sparse-proof source passes executable checking");
        let core = crate::core_lower::lower_program_to_core(&program)
            .expect("ordinary sparse-proof source lowers to Core");
        let core = crate::core_verify::verify_executable_core(core)
            .expect("ordinary sparse-proof Core verifies");
        let plan = crate::aot_v2::plan_native(&core)
            .expect("ordinary sparse-proof Core plans through production AOT");
        let code_range = plan.native_code_layout().code_range;
        let package = crate::execution_package_build::build_execution_package(
            &core,
            "sparse-proof.arc",
            plan.native_code_layout(),
        )
        .expect("ordinary sparse-proof ARCHEECS v2 package builds");
        let image = crate::aot_v2::finalize_native(plan, &core, &package)
            .expect("ordinary sparse-proof production image finalizes");

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock follows the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "archec0-sparse-v2-{}-{unique}.elf",
            std::process::id()
        ));
        let _cleanup = RemoveOnDrop {
            path: path.clone(),
            preserve: std::env::var_os("ARCHEC0_KEEP_SPARSE_PROOF").is_some(),
        };
        let mut output = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .expect("sparse proof artifact opens");
        let layout = image
            .write_static_pie(&mut output, u64::from(u32::MAX) + STATIC_PIE_PAGE_SIZE + 1)
            .expect("production sparse v2 PIE encodes");
        output.sync_all().expect("sparse v2 PIE synchronizes");
        let file_metadata = output.metadata().expect("sparse v2 PIE metadata reads");
        assert!(layout.metadata_offset > u64::from(u32::MAX));
        assert!(layout.file_byte_len > u64::from(u32::MAX));
        assert_eq!(file_metadata.len(), layout.file_byte_len);
        let allocated_bytes = file_metadata
            .st_blocks()
            .checked_mul(512)
            .expect("sparse artifact allocated-byte count fits u64");
        assert!(
            allocated_bytes != 0,
            "the sparse proof must observe physically allocated ELF bytes"
        );
        assert!(
            allocated_bytes < 16 * 1024 * 1024,
            "the >4-GiB hole must remain sparse"
        );

        output
            .seek(SeekFrom::Start(0))
            .expect("ELF header seek succeeds");
        let header_byte_len = usize::from(ELF_HEADER_SIZE)
            + usize::from(PROGRAM_HEADER_SIZE) * usize::from(STATIC_PIE_PROGRAM_HEADER_COUNT);
        let mut header_bytes = vec![0; header_byte_len];
        output
            .read_exact(&mut header_bytes)
            .expect("ELF and program headers are present");
        assert_eq!(&header_bytes[..4], b"\x7fELF");
        assert_eq!(
            u16::from_le_bytes(header_bytes[16..18].try_into().unwrap()),
            3,
            "the sparse artifact is ET_DYN"
        );
        assert_eq!(
            u64::from_le_bytes(header_bytes[24..32].try_into().unwrap()),
            layout.entry_point
        );
        assert_eq!(
            u16::from_le_bytes(header_bytes[56..58].try_into().unwrap()),
            STATIC_PIE_PROGRAM_HEADER_COUNT
        );

        let program_headers = (0..STATIC_PIE_PROGRAM_HEADER_COUNT)
            .map(|index| {
                let offset = usize::from(ELF_HEADER_SIZE)
                    + usize::from(index) * usize::from(PROGRAM_HEADER_SIZE);
                ParsedProgramHeader {
                    kind: u32::from_le_bytes(header_bytes[offset..offset + 4].try_into().unwrap()),
                    flags: u32::from_le_bytes(
                        header_bytes[offset + 4..offset + 8].try_into().unwrap(),
                    ),
                    offset: u64::from_le_bytes(
                        header_bytes[offset + 8..offset + 16].try_into().unwrap(),
                    ),
                    vaddr: u64::from_le_bytes(
                        header_bytes[offset + 16..offset + 24].try_into().unwrap(),
                    ),
                    file_bytes: u64::from_le_bytes(
                        header_bytes[offset + 32..offset + 40].try_into().unwrap(),
                    ),
                    memory_bytes: u64::from_le_bytes(
                        header_bytes[offset + 40..offset + 48].try_into().unwrap(),
                    ),
                    align: u64::from_le_bytes(
                        header_bytes[offset + 48..offset + 56].try_into().unwrap(),
                    ),
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            program_headers
                .iter()
                .map(|header| (header.kind, header.flags))
                .collect::<Vec<_>>(),
            [
                (PT_LOAD, PF_R),
                (PT_LOAD, PF_R | PF_X),
                (PT_LOAD, PF_R | PF_W),
                (PT_LOAD, PF_R),
                (PT_GNU_STACK, PF_R | PF_W),
            ]
        );
        for header in program_headers
            .iter()
            .filter(|header| header.kind == PT_LOAD)
        {
            assert_eq!(header.offset % header.align, header.vaddr % header.align);
            assert!(header.file_bytes <= header.memory_bytes);
            assert_ne!(
                header.flags & (PF_W | PF_X),
                PF_W | PF_X,
                "no segment may be writable and executable"
            );
        }
        let text_header = program_headers[1];
        assert!(layout.entry_point >= text_header.vaddr);
        assert!(layout.entry_point < text_header.vaddr + text_header.memory_bytes);
        let metadata_header = program_headers[3];
        assert_eq!(metadata_header.offset, layout.metadata_offset);
        assert_eq!(metadata_header.vaddr, layout.metadata_vaddr);
        assert_eq!(metadata_header.file_bytes, layout.metadata_byte_len);
        assert_eq!(metadata_header.flags, PF_R);

        output
            .seek(SeekFrom::Start(layout.metadata_offset))
            .expect("v2 metadata seek succeeds");
        let persisted_package = archec0::execution_package_v2::decode_package_from_with_code_range(
            &mut output,
            code_range,
        )
        .expect("persisted complete v2 package validates");
        assert_eq!(persisted_package, package);
        assert_eq!(
            output
                .stream_position()
                .expect("v2 metadata end position reads"),
            layout.metadata_offset + layout.metadata_byte_len
        );

        let mut reference_stdout = Vec::new();
        let mut reference_stderr = Vec::new();
        let reference = crate::reference_executor_v2::execute_decoded(
            &core,
            persisted_package,
            Some(code_range),
            &mut reference_stdout,
            &mut reference_stderr,
        )
        .expect("persisted sparse v2 package executes through the direct Core reference");

        let mut permissions = file_metadata.permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&path, permissions).expect("sparse v2 PIE becomes executable");
        drop(output);

        let elf_header =
            command_output(Command::new("readelf").arg("-hW").arg(&path), "readelf -hW");
        assert!(elf_header.contains("Type:                              DYN"));
        let readelf_program_headers =
            command_output(Command::new("readelf").arg("-lW").arg(&path), "readelf -lW");
        assert_eq!(readelf_program_headers.matches("  LOAD ").count(), 4);
        assert!(readelf_program_headers.contains("GNU_STACK"));
        assert!(!readelf_program_headers.contains(" INTERP "));
        assert!(!readelf_program_headers.contains(" DYNAMIC "));
        assert!(!readelf_program_headers.contains("RWE"));
        let dynamic = command_output(Command::new("readelf").arg("-dW").arg(&path), "readelf -dW");
        assert!(dynamic.contains("There is no dynamic section in this file."));
        let relocations =
            command_output(Command::new("readelf").arg("-rW").arg(&path), "readelf -rW");
        assert!(relocations.contains("There are no relocations in this file."));

        let native = Command::new(&path)
            .output()
            .expect("sparse v2 PIE executes");
        assert_eq!(
            native.status.code(),
            Some(reference.process_status()),
            "sparse native status differs; stdout={} stderr={}",
            String::from_utf8_lossy(&native.stdout),
            String::from_utf8_lossy(&native.stderr)
        );
        assert_eq!(native.stdout, reference_stdout);
        assert_eq!(native.stderr, reference_stderr);
    }
}
