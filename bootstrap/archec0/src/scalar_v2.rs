use std::fmt;

pub const CANONICAL_NAN_BITS: u32 = 0x7FC0_0000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScalarValue {
    I32(i32),
    F32Bits(u32),
    Bool(bool),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum I32BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    BitAnd,
    BitXor,
    BitOr,
    ShiftLeft,
    ShiftRight,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum F32BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComparisonOp {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrapKind {
    I32DivideByZero,
    I32DivideOverflow,
    I32RemainderByZero,
    I32RemainderOverflow,
}

impl TrapKind {
    pub const fn diagnostic_name(self) -> &'static str {
        match self {
            Self::I32DivideByZero => "I32_DIVIDE_BY_ZERO",
            Self::I32DivideOverflow => "I32_DIVIDE_OVERFLOW",
            Self::I32RemainderByZero => "I32_REMAINDER_BY_ZERO",
            Self::I32RemainderOverflow => "I32_REMAINDER_OVERFLOW",
        }
    }
}

impl fmt::Display for TrapKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.diagnostic_name())
    }
}

impl std::error::Error for TrapKind {}

pub fn i32_binary(op: I32BinaryOp, left: i32, right: i32) -> Result<i32, TrapKind> {
    match op {
        I32BinaryOp::Add => Ok(left.wrapping_add(right)),
        I32BinaryOp::Subtract => Ok(left.wrapping_sub(right)),
        I32BinaryOp::Multiply => Ok(left.wrapping_mul(right)),
        I32BinaryOp::Divide => {
            if right == 0 {
                Err(TrapKind::I32DivideByZero)
            } else if left == i32::MIN && right == -1 {
                Err(TrapKind::I32DivideOverflow)
            } else {
                Ok(left / right)
            }
        }
        I32BinaryOp::Remainder => {
            if right == 0 {
                Err(TrapKind::I32RemainderByZero)
            } else if left == i32::MIN && right == -1 {
                Err(TrapKind::I32RemainderOverflow)
            } else {
                Ok(left % right)
            }
        }
        I32BinaryOp::BitAnd => Ok(left & right),
        I32BinaryOp::BitXor => Ok(left ^ right),
        I32BinaryOp::BitOr => Ok(left | right),
        I32BinaryOp::ShiftLeft => Ok(left.wrapping_shl(masked_shift_count(right))),
        I32BinaryOp::ShiftRight => Ok(left.wrapping_shr(masked_shift_count(right))),
    }
}

pub const fn i32_negate(value: i32) -> i32 {
    value.wrapping_neg()
}

pub const fn i32_bit_not(value: i32) -> i32 {
    !value
}

pub fn i32_compare(op: ComparisonOp, left: i32, right: i32) -> bool {
    match op {
        ComparisonOp::Less => left < right,
        ComparisonOp::LessEqual => left <= right,
        ComparisonOp::Greater => left > right,
        ComparisonOp::GreaterEqual => left >= right,
        ComparisonOp::Equal => left == right,
        ComparisonOp::NotEqual => left != right,
    }
}

pub fn f32_binary(op: F32BinaryOp, left_bits: u32, right_bits: u32) -> u32 {
    let left = f32::from_bits(left_bits);
    let right = f32::from_bits(right_bits);
    let result = match op {
        F32BinaryOp::Add => left + right,
        F32BinaryOp::Subtract => left - right,
        F32BinaryOp::Multiply => left * right,
        F32BinaryOp::Divide => left / right,
    };
    canonicalize_nan(result.to_bits())
}

pub fn f32_negate(bits: u32) -> u32 {
    canonicalize_nan((-f32::from_bits(bits)).to_bits())
}

pub fn f32_compare(op: ComparisonOp, left_bits: u32, right_bits: u32) -> bool {
    let left = f32::from_bits(left_bits);
    let right = f32::from_bits(right_bits);
    match op {
        ComparisonOp::Less => left < right,
        ComparisonOp::LessEqual => left <= right,
        ComparisonOp::Greater => left > right,
        ComparisonOp::GreaterEqual => left >= right,
        ComparisonOp::Equal => left == right,
        ComparisonOp::NotEqual => left != right,
    }
}

pub const fn bool_compare(op: ComparisonOp, left: bool, right: bool) -> Option<bool> {
    match op {
        ComparisonOp::Equal => Some(left == right),
        ComparisonOp::NotEqual => Some(left != right),
        ComparisonOp::Less
        | ComparisonOp::LessEqual
        | ComparisonOp::Greater
        | ComparisonOp::GreaterEqual => None,
    }
}

pub const fn canonicalize_nan(bits: u32) -> u32 {
    let exponent = bits & 0x7F80_0000;
    let significand = bits & 0x007F_FFFF;
    if exponent == 0x7F80_0000 && significand != 0 {
        CANONICAL_NAN_BITS
    } else {
        bits
    }
}

const fn masked_shift_count(right: i32) -> u32 {
    (right as u32) & 31
}

/// Establishes Arche's process-entry floating-point environment on x86 hosts.
///
/// This clears pending exception flags, masks all six IEEE/SSE exceptions,
/// selects round-to-nearest-even, and disables DAZ and FTZ. The x87 control
/// word is kept consistent even though generated scalar `f32` code uses SSE.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn initialize_floating_point_environment() {
    let mut mxcsr: u32 = 0;
    let mut x87_control: u16 = 0;
    // SAFETY: both instructions only read the current thread's floating-point
    // control state into valid, correctly sized stack locations.
    unsafe {
        std::arch::asm!(
            "stmxcsr [{mxcsr}]",
            "fnstcw [{x87_control}]",
            mxcsr = in(reg) &mut mxcsr,
            x87_control = in(reg) &mut x87_control,
            options(nostack, preserves_flags),
        );
    }
    mxcsr = (mxcsr & !0x0000_FFFF) | 0x0000_1F80;
    x87_control = (x87_control | 0x003F) & !0x0C00;
    // SAFETY: the values use the architectural control-word layouts; reserved
    // MXCSR bits are preserved and the pointers remain valid for both loads.
    unsafe {
        std::arch::asm!(
            "ldmxcsr [{mxcsr}]",
            "fldcw [{x87_control}]",
            mxcsr = in(reg) &mxcsr,
            x87_control = in(reg) &x87_control,
            options(nostack, preserves_flags),
        );
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub fn initialize_floating_point_environment() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i32_wrapping_and_masked_shift_semantics_are_exact() {
        assert_eq!(i32_binary(I32BinaryOp::Add, i32::MAX, 1), Ok(i32::MIN));
        assert_eq!(i32_binary(I32BinaryOp::Subtract, i32::MIN, 1), Ok(i32::MAX));
        assert_eq!(i32_negate(i32::MIN), i32::MIN);
        assert_eq!(i32_binary(I32BinaryOp::ShiftLeft, 1, 32), Ok(1));
        assert_eq!(i32_binary(I32BinaryOp::ShiftLeft, 1, -1), Ok(i32::MIN));
        assert_eq!(
            i32_binary(I32BinaryOp::ShiftRight, i32::MIN, 33),
            Ok(0xC000_0000_u32 as i32)
        );
    }

    #[test]
    fn integer_divide_and_remainder_traps_are_distinct() {
        assert_eq!(
            i32_binary(I32BinaryOp::Divide, 1, 0),
            Err(TrapKind::I32DivideByZero)
        );
        assert_eq!(
            i32_binary(I32BinaryOp::Remainder, 1, 0),
            Err(TrapKind::I32RemainderByZero)
        );
        assert_eq!(
            i32_binary(I32BinaryOp::Divide, i32::MIN, -1),
            Err(TrapKind::I32DivideOverflow)
        );
        assert_eq!(
            i32_binary(I32BinaryOp::Remainder, i32::MIN, -1),
            Err(TrapKind::I32RemainderOverflow)
        );
        assert_eq!(i32_binary(I32BinaryOp::Divide, -7, 3), Ok(-2));
        assert_eq!(i32_binary(I32BinaryOp::Remainder, -7, 3), Ok(-1));
    }

    #[test]
    fn f32_arithmetic_canonicalizes_nan_and_preserves_zero_and_subnormals() {
        assert_eq!(
            f32_binary(F32BinaryOp::Add, 0x7FA1_2345, 1.0_f32.to_bits()),
            CANONICAL_NAN_BITS
        );
        assert_eq!(f32_negate(0x7FC0_1234), CANONICAL_NAN_BITS);
        assert_eq!(
            f32_binary(
                F32BinaryOp::Multiply,
                (-0.0_f32).to_bits(),
                1.0_f32.to_bits()
            ),
            (-0.0_f32).to_bits()
        );
        assert_eq!(f32_binary(F32BinaryOp::Multiply, 1, 1.0_f32.to_bits()), 1);
    }

    #[test]
    fn f32_nan_comparisons_are_ordered_except_not_equal() {
        let nan = 0x7FC0_0001;
        let one = 1.0_f32.to_bits();
        assert!(!f32_compare(ComparisonOp::Less, nan, one));
        assert!(!f32_compare(ComparisonOp::LessEqual, nan, one));
        assert!(!f32_compare(ComparisonOp::Greater, nan, one));
        assert!(!f32_compare(ComparisonOp::GreaterEqual, nan, one));
        assert!(!f32_compare(ComparisonOp::Equal, nan, nan));
        assert!(f32_compare(ComparisonOp::NotEqual, nan, nan));
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[test]
    fn floating_point_entry_state_disables_ftz_and_daz_and_masks_exceptions() {
        initialize_floating_point_environment();
        let mut mxcsr: u32 = 0;
        // SAFETY: `mxcsr` is a valid four-byte output location.
        unsafe {
            std::arch::asm!(
                "stmxcsr [{mxcsr}]",
                mxcsr = in(reg) &mut mxcsr,
                options(nostack, preserves_flags),
            );
        }
        assert_eq!(mxcsr & 0x0000_FFFF, 0x0000_1F80);
    }
}
