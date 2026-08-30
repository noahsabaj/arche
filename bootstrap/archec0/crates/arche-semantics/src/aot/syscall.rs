//! Direct Linux x86-64 system call definitions and LIR generation (M27-G).

use crate::aot::def::{LirInst, Operand, Reg64};

pub const SYS_READ: i64 = 0;
pub const SYS_WRITE: i64 = 1;
pub const SYS_OPEN: i64 = 2;
pub const SYS_CLOSE: i64 = 3;
pub const SYS_SCHED_YIELD: i64 = 24;
pub const SYS_NANOSLEEP: i64 = 35;
pub const SYS_EXIT: i64 = 60;
pub const SYS_CLOCK_GETTIME: i64 = 228;
pub const SYS_EXIT_GROUP: i64 = 231;

/// Emits LIR instructions to prepare and execute an inline Linux system call.
/// Syscall argument order: `rax` (nr), `rdi` (arg1), `rsi` (arg2), `rdx` (arg3), `r10` (arg4), `r8` (arg5), `r9` (arg6).
#[must_use]
pub fn build_syscall_invocation(syscall_nr: i64, args: &[Operand]) -> Vec<LirInst> {
    let mut insts = Vec::new();

    // 1. Move syscall number into RAX
    insts.push(LirInst::Mov {
        dst: Operand::Reg(Reg64::Rax),
        src: Operand::Imm(syscall_nr),
    });

    // 2. Map up to 6 arguments to syscall registers: [rdi, rsi, rdx, r10, r8, r9]
    let syscall_arg_regs = [
        Reg64::Rdi,
        Reg64::Rsi,
        Reg64::Rdx,
        Reg64::R10,
        Reg64::R8,
        Reg64::R9,
    ];

    for (i, arg) in args.iter().enumerate().take(6) {
        let target_reg = syscall_arg_regs[i];
        insts.push(LirInst::Mov {
            dst: Operand::Reg(target_reg),
            src: arg.clone(),
        });
    }

    // 3. Syscall instruction
    insts.push(LirInst::Syscall);

    insts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_syscall_construction() {
        let fd = Operand::Imm(1); // stdout
        let buf = Operand::Reg(Reg64::Rsi);
        let len = Operand::Imm(12);

        let insts = build_syscall_invocation(SYS_WRITE, &[fd, buf, len]);
        assert_eq!(insts.len(), 5);
        assert_eq!(
            insts[0],
            LirInst::Mov {
                dst: Operand::Reg(Reg64::Rax),
                src: Operand::Imm(SYS_WRITE)
            }
        );
        assert_eq!(insts[4], LirInst::Syscall);
    }
}
