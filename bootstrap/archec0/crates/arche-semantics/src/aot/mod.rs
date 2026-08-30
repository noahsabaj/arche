//! Native x86-64 AOT Compilation, ABI lowering, and direct machine code emission (M27-G).

pub mod abi;
pub mod def;
pub mod encode;
pub mod syscall;

pub use abi::{
    classify_arguments, classify_return_type, compute_aligned_frame_size, ArgLocation,
    ReturnLocation, FLOAT_ARG_REGISTERS, INT_ARG_REGISTERS,
};
pub use def::{ConditionCode, LirFunction, LirInst, Mem, Operand, Reg64, RegXmm};
pub use encode::encode_lir_function;
pub use syscall::{
    build_syscall_invocation, SYS_CLOCK_GETTIME, SYS_CLOSE, SYS_EXIT, SYS_EXIT_GROUP,
    SYS_NANOSLEEP, SYS_OPEN, SYS_READ, SYS_SCHED_YIELD, SYS_WRITE,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_aot_function_pipeline() {
        let mut func = LirFunction {
            name: "main".into(),
            instructions: Vec::new(),
            frame_size: 16,
        };

        // Frame setup
        func.instructions.push(LirInst::Push {
            src: Operand::Reg(Reg64::Rbp),
        });
        func.instructions.push(LirInst::Mov {
            dst: Operand::Reg(Reg64::Rbp),
            src: Operand::Reg(Reg64::Rsp),
        });

        // Compute 10 + 32 = 42
        func.instructions.push(LirInst::Mov {
            dst: Operand::Reg(Reg64::Rax),
            src: Operand::Imm(10),
        });
        func.instructions.push(LirInst::Mov {
            dst: Operand::Reg(Reg64::Rdx),
            src: Operand::Imm(32),
        });
        func.instructions.push(LirInst::Add {
            dst: Operand::Reg(Reg64::Rax),
            src: Operand::Reg(Reg64::Rdx),
        });

        // Frame teardown & return
        func.instructions.push(LirInst::Pop { dst: Reg64::Rbp });
        func.instructions.push(LirInst::Ret);

        let code = encode_lir_function(&func);
        assert!(!code.is_empty());
        assert_eq!(code.last(), Some(&0xC3)); // ret
    }
}
