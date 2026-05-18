use std::fs;
use std::io;
use std::path::Path;

const ELF_HEADER_SIZE: u16 = 64;
const PROGRAM_HEADER_SIZE: u16 = 56;
const TEXT_OFFSET: usize = ELF_HEADER_SIZE as usize + PROGRAM_HEADER_SIZE as usize;
const LOAD_BASE: u64 = 0x400000;
const ENTRY_POINT: u64 = LOAD_BASE + TEXT_OFFSET as u64;

pub fn write_executable_with_metadata(
    path: &Path,
    text_payload: &[u8],
    metadata_payload: &[u8],
) -> io::Result<()> {
    let file_size = TEXT_OFFSET + text_payload.len() + metadata_payload.len();
    let mut bytes = Vec::with_capacity(file_size);

    write_elf_header(&mut bytes);
    write_load_program_header(&mut bytes, file_size);
    write_text_payload(&mut bytes, text_payload);
    write_metadata_payload(&mut bytes, metadata_payload);

    debug_assert_eq!(bytes.len(), file_size);
    fs::write(path, bytes)
}

fn write_elf_header(bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&[
        0x7f, b'E', b'L', b'F', // ELF magic
        2,    // ELFCLASS64
        1,    // ELFDATA2LSB
        1,    // EV_CURRENT
        0,    // ELFOSABI_SYSV
        0,    // ABI version
        0, 0, 0, 0, 0, 0, 0, // padding
    ]);

    push_u16(bytes, 2); // ET_EXEC
    push_u16(bytes, 0x3e); // EM_X86_64
    push_u32(bytes, 1); // EV_CURRENT
    push_u64(bytes, ENTRY_POINT);
    push_u64(bytes, ELF_HEADER_SIZE as u64); // e_phoff
    push_u64(bytes, 0); // no section headers yet
    push_u32(bytes, 0); // e_flags
    push_u16(bytes, ELF_HEADER_SIZE);
    push_u16(bytes, PROGRAM_HEADER_SIZE);
    push_u16(bytes, 1); // one PT_LOAD program header
    push_u16(bytes, 0); // no section header entries yet
    push_u16(bytes, 0);
    push_u16(bytes, 0);
}

fn write_load_program_header(bytes: &mut Vec<u8>, file_size: usize) {
    push_u32(bytes, 1); // PT_LOAD
    push_u32(bytes, 5); // PF_R | PF_X
    push_u64(bytes, 0); // p_offset
    push_u64(bytes, LOAD_BASE); // p_vaddr
    push_u64(bytes, LOAD_BASE); // p_paddr
    push_u64(bytes, file_size as u64); // p_filesz
    push_u64(bytes, file_size as u64); // p_memsz
    push_u64(bytes, 0x1000); // p_align
}

fn write_text_payload(bytes: &mut Vec<u8>, text_payload: &[u8]) {
    debug_assert_eq!(bytes.len(), TEXT_OFFSET);
    bytes.extend_from_slice(text_payload);
}

fn write_metadata_payload(bytes: &mut Vec<u8>, metadata_payload: &[u8]) {
    bytes.extend_from_slice(metadata_payload);
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
