//! SysV x86-64 ABI classification and stack frame layout calculations (M27-G).

use crate::aot::def::{Reg64, RegXmm};
use crate::instance::align_to;
use arche_frontend::SymbolicType;

/// SysV x86-64 integer argument registers.
pub const INT_ARG_REGISTERS: [Reg64; 6] = [
    Reg64::Rdi,
    Reg64::Rsi,
    Reg64::Rdx,
    Reg64::Rcx,
    Reg64::R8,
    Reg64::R9,
];

/// SysV x86-64 float argument registers.
pub const FLOAT_ARG_REGISTERS: [RegXmm; 8] = [
    RegXmm::Xmm0,
    RegXmm::Xmm1,
    RegXmm::Xmm2,
    RegXmm::Xmm3,
    RegXmm::Xmm4,
    RegXmm::Xmm5,
    RegXmm::Xmm6,
    RegXmm::Xmm7,
];

/// Physical location of a function argument.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArgLocation {
    Reg(Reg64),
    Xmm(RegXmm),
    Stack(u32), // Byte offset from caller stack pointer
}

/// Physical location of a function return value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReturnLocation {
    Void,
    Scalar(Reg64),            // Rax
    ScalarPair(Reg64, Reg64), // Rax + Rdx
    Xmm(RegXmm),              // Xmm0
    HiddenPtr(Reg64),         // Passed in Rdi
}

/// Classifies function arguments into registers or stack slots under SysV x86-64 ABI.
#[must_use]
pub fn classify_arguments(arg_types: &[SymbolicType]) -> Vec<ArgLocation> {
    let mut int_idx = 0;
    let mut float_idx = 0;
    let mut stack_offset = 0u32;
    let mut locations = Vec::with_capacity(arg_types.len());

    for arg in arg_types {
        match arg {
            SymbolicType::F32 | SymbolicType::F64 => {
                if float_idx < FLOAT_ARG_REGISTERS.len() {
                    locations.push(ArgLocation::Xmm(FLOAT_ARG_REGISTERS[float_idx]));
                    float_idx += 1;
                } else {
                    locations.push(ArgLocation::Stack(stack_offset));
                    stack_offset += 8;
                }
            }
            SymbolicType::Unit => {
                // ZST arguments occupy zero registers and zero stack space
                locations.push(ArgLocation::Stack(stack_offset));
            }
            _ => {
                if int_idx < INT_ARG_REGISTERS.len() {
                    locations.push(ArgLocation::Reg(INT_ARG_REGISTERS[int_idx]));
                    int_idx += 1;
                } else {
                    locations.push(ArgLocation::Stack(stack_offset));
                    stack_offset += 8;
                }
            }
        }
    }

    locations
}

/// Classifies the return value location for a type.
#[must_use]
pub fn classify_return_type(ret_type: &SymbolicType) -> ReturnLocation {
    match ret_type {
        SymbolicType::Unit | SymbolicType::Never => ReturnLocation::Void,
        SymbolicType::F32 | SymbolicType::F64 => ReturnLocation::Xmm(RegXmm::Xmm0),
        _ => ReturnLocation::Scalar(Reg64::Rax),
    }
}

/// Computes the 16-byte aligned local stack frame allocation size.
///
/// In SysV x86-64, when a function begins:
/// - Return address is pushed (rsp % 16 == 8).
/// - `push rbp` occurs (rsp % 16 == 0).
/// - Each pushed callee-saved register subtracts 8 bytes.
/// - We must size the local allocation `frame_size` so that immediately before any `call`,
///   `rsp % 16 == 0`.
#[must_use]
pub fn compute_aligned_frame_size(local_bytes: u32, num_callee_saved_pushed: usize) -> u32 {
    let size = local_bytes as u64;
    // Base alignment adjustment:
    // Pushed return address (8) + pushed rbp (8) = 16 (aligned)
    // Plus callee saved registers = num_callee_saved_pushed * 8
    let extra_pushed = (num_callee_saved_pushed * 8) as u64;
    let total = size + extra_pushed;
    let aligned_total = align_to(total, 16);
    (aligned_total - extra_pushed) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sysv_arg_classification() {
        let args = vec![
            SymbolicType::I32,
            SymbolicType::F64,
            SymbolicType::U64,
            SymbolicType::F32,
        ];
        let locs = classify_arguments(&args);
        assert_eq!(locs[0], ArgLocation::Reg(Reg64::Rdi));
        assert_eq!(locs[1], ArgLocation::Xmm(RegXmm::Xmm0));
        assert_eq!(locs[2], ArgLocation::Reg(Reg64::Rsi));
        assert_eq!(locs[3], ArgLocation::Xmm(RegXmm::Xmm1));
    }

    #[test]
    fn stack_frame_16_byte_alignment() {
        // 0 local bytes, 0 callee saved -> 0 frame size
        assert_eq!(compute_aligned_frame_size(0, 0), 0);

        // 8 local bytes, 0 callee saved -> pads to 16
        assert_eq!(compute_aligned_frame_size(8, 0), 16);

        // 8 local bytes, 1 callee saved (8 bytes pushed) -> total = 16 -> frame_size = 8
        assert_eq!(compute_aligned_frame_size(8, 1), 8);
    }
}
