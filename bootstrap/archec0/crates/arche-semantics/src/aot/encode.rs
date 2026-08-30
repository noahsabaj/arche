//! Direct x86-64 machine code binary byte encoder (M27-G).

use crate::aot::def::{LirFunction, LirInst, Operand};

/// Encodes an entire `LirFunction` into raw executable x86-64 machine code bytes.
#[must_use]
pub fn encode_lir_function(func: &LirFunction) -> Vec<u8> {
    let mut bytes = Vec::new();

    for inst in &func.instructions {
        match inst {
            LirInst::Syscall => {
                // 0F 05
                bytes.extend_from_slice(&[0x0F, 0x05]);
            }
            LirInst::Ret => {
                // C3
                bytes.push(0xC3);
            }
            LirInst::Push {
                src: Operand::Reg(reg),
            } => {
                if reg.is_extended() {
                    bytes.push(0x41); // REX.B
                }
                bytes.push(0x50 + (reg.index() & 0x07));
            }
            LirInst::Pop { dst } => {
                if dst.is_extended() {
                    bytes.push(0x41); // REX.B
                }
                bytes.push(0x58 + (dst.index() & 0x07));
            }
            LirInst::Mov { dst, src } => match (dst, src) {
                (Operand::Reg(dst_reg), Operand::Imm(imm)) => {
                    // REX.W + B8+rd io (MOV r64, imm64)
                    let rex = 0x48 | if dst_reg.is_extended() { 0x01 } else { 0x00 };
                    bytes.push(rex);
                    bytes.push(0xB8 + (dst_reg.index() & 0x07));
                    bytes.extend_from_slice(&imm.to_le_bytes());
                }
                (Operand::Reg(dst_reg), Operand::Reg(src_reg)) => {
                    // MOV r/m64, r64 -> REX.W 89 /r
                    let mut rex = 0x48;
                    if src_reg.is_extended() {
                        rex |= 0x04; // REX.R
                    }
                    if dst_reg.is_extended() {
                        rex |= 0x01; // REX.B
                    }
                    bytes.push(rex);
                    bytes.push(0x89);
                    let modrm = 0xC0 | ((src_reg.index() & 0x07) << 3) | (dst_reg.index() & 0x07);
                    bytes.push(modrm);
                }
                _ => {}
            },
            LirInst::Add { dst, src } => {
                if let (Operand::Reg(dst_reg), Operand::Reg(src_reg)) = (dst, src) {
                    let mut rex = 0x48;
                    if src_reg.is_extended() {
                        rex |= 0x04;
                    }
                    if dst_reg.is_extended() {
                        rex |= 0x01;
                    }
                    bytes.push(rex);
                    bytes.push(0x01); // ADD r/m64, r64
                    let modrm = 0xC0 | ((src_reg.index() & 0x07) << 3) | (dst_reg.index() & 0x07);
                    bytes.push(modrm);
                }
            }
            LirInst::Sub { dst, src } => {
                if let (Operand::Reg(dst_reg), Operand::Reg(src_reg)) = (dst, src) {
                    let mut rex = 0x48;
                    if src_reg.is_extended() {
                        rex |= 0x04;
                    }
                    if dst_reg.is_extended() {
                        rex |= 0x01;
                    }
                    bytes.push(rex);
                    bytes.push(0x29); // SUB r/m64, r64
                    let modrm = 0xC0 | ((src_reg.index() & 0x07) << 3) | (dst_reg.index() & 0x07);
                    bytes.push(modrm);
                }
            }
            _ => {}
        }
    }

    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aot::def::Reg64;

    #[test]
    fn encode_exit42_syscall_sequence() {
        let func = LirFunction {
            name: "exit42".into(),
            instructions: vec![
                LirInst::Mov {
                    dst: Operand::Reg(Reg64::Rax),
                    src: Operand::Imm(60), // sys_exit
                },
                LirInst::Mov {
                    dst: Operand::Reg(Reg64::Rdi),
                    src: Operand::Imm(42), // status
                },
                LirInst::Syscall,
            ],
            frame_size: 0,
        };

        let bytes = encode_lir_function(&func);
        // mov rax, 60 -> 48 B8 3C 00 00 00 00 00 00 00
        // mov rdi, 42 -> 48 BF 2A 00 00 00 00 00 00 00
        // syscall -> 0F 05
        assert_eq!(bytes.len(), 10 + 10 + 2);
        assert_eq!(&bytes[0..2], &[0x48, 0xB8]);
        assert_eq!(&bytes[10..12], &[0x48, 0xBF]);
        assert_eq!(&bytes[20..22], &[0x0F, 0x05]);
    }
}
