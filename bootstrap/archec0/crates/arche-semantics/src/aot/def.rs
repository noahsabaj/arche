//! Low-Level Machine IR (LIR) and x86-64 definitions for Native AOT (M27-G).

/// Standard x86-64 64-bit general purpose registers.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Reg64 {
    Rax = 0,
    Rcx = 1,
    Rdx = 2,
    Rbx = 3,
    Rsp = 4,
    Rbp = 5,
    Rsi = 6,
    Rdi = 7,
    R8 = 8,
    R9 = 9,
    R10 = 10,
    R11 = 11,
    R12 = 12,
    R13 = 13,
    R14 = 14,
    R15 = 15,
}

impl Reg64 {
    /// Number encoding of the register (0..15).
    #[must_use]
    pub const fn index(self) -> u8 {
        self as u8
    }

    /// Whether this register requires the REX prefix bit (R8..R15).
    #[must_use]
    pub const fn is_extended(self) -> bool {
        self.index() >= 8
    }

    /// Whether this is a callee-saved register under SysV x86-64 ABI (rbx, rsp, rbp, r12..r15).
    #[must_use]
    pub const fn is_callee_saved(self) -> bool {
        matches!(
            self,
            Self::Rbx | Self::Rsp | Self::Rbp | Self::R12 | Self::R13 | Self::R14 | Self::R15
        )
    }
}

/// x86-64 128-bit SIMD / Floating-Point register.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RegXmm {
    Xmm0 = 0,
    Xmm1 = 1,
    Xmm2 = 2,
    Xmm3 = 3,
    Xmm4 = 4,
    Xmm5 = 5,
    Xmm6 = 6,
    Xmm7 = 7,
    Xmm8 = 8,
    Xmm9 = 9,
    Xmm10 = 10,
    Xmm11 = 11,
    Xmm12 = 12,
    Xmm13 = 13,
    Xmm14 = 14,
    Xmm15 = 15,
}

/// x86-64 memory addressing operand: `[base + scale * index + disp]`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mem {
    pub base: Option<Reg64>,
    pub index: Option<Reg64>,
    pub scale: u8, // 1, 2, 4, or 8
    pub disp: i32,
}

impl Mem {
    /// Base-displacement memory operand: `[base + disp]`.
    #[must_use]
    pub const fn base_disp(base: Reg64, disp: i32) -> Self {
        Self {
            base: Some(base),
            index: None,
            scale: 1,
            disp,
        }
    }
}

/// A machine operand.
#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    Reg(Reg64),
    Imm(i64),
    Mem(Mem),
}

/// Condition code for conditional branches (Jcc / Setcc).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConditionCode {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Above,
    AboveEqual,
    Below,
    BelowEqual,
    Zero,
    NotZero,
}

/// Low-level machine instruction for x86-64.
#[derive(Clone, Debug, PartialEq)]
pub enum LirInst {
    Mov { dst: Operand, src: Operand },
    Add { dst: Operand, src: Operand },
    Sub { dst: Operand, src: Operand },
    Imul { dst: Operand, src: Operand },
    Idiv { divisor: Operand },
    Lea { dst: Reg64, src: Mem },
    Cmp { lhs: Operand, rhs: Operand },
    Test { lhs: Operand, rhs: Operand },
    Jmp { target: String },
    Jcc { cc: ConditionCode, target: String },
    Call { target: String },
    Ret,
    Syscall,
    Push { src: Operand },
    Pop { dst: Reg64 },
    Label(String),
}

/// A lowered machine function ready for binary byte encoding.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LirFunction {
    pub name: String,
    pub instructions: Vec<LirInst>,
    pub frame_size: u32,
}
