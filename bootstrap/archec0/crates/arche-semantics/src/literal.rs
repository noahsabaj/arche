//! Exact integer-literal selection and fixed-width encoding.

use arche_frontend::{
    lexer::{FloatLiteral, FloatSuffix, IntegerLiteral, IntegerSuffix, NumericBase},
    IntegerType,
};
use num_bigint::BigUint;
use num_traits::{One, ToPrimitive, Zero};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedIntegerLiteral {
    integer_type: IntegerType,
    little_endian_bits: Box<[u8]>,
}

impl TypedIntegerLiteral {
    pub const fn integer_type(&self) -> IntegerType {
        self.integer_type
    }

    pub fn little_endian_bits(&self) -> &[u8] {
        &self.little_endian_bits
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegerLiteralError {
    SuffixContextMismatch {
        suffix: IntegerType,
        context: IntegerType,
    },
    MagnitudeTooLarge,
    PositiveOutOfRange(IntegerType),
    NegativeUnsigned(IntegerType),
    NegativeOutOfRange(IntegerType),
}

/// Selects and encodes one literal after unary-negative interpretation.
///
/// Unsuffixed literals use the contextual integer type or default to `i32`.
/// The returned bytes are the exact fixed-width two's-complement/modulo bits;
/// no host integer representation enters later identity or Core inputs.
pub fn check_integer_literal(
    literal: &IntegerLiteral,
    contextual_type: Option<IntegerType>,
    unary_negative: bool,
) -> Result<TypedIntegerLiteral, IntegerLiteralError> {
    let suffix_type = literal.suffix.map(integer_suffix_type);
    if let (Some(suffix), Some(context)) = (suffix_type, contextual_type) {
        if suffix != context {
            return Err(IntegerLiteralError::SuffixContextMismatch { suffix, context });
        }
    }
    let integer_type = suffix_type.or(contextual_type).unwrap_or(IntegerType::I32);
    let magnitude = parse_magnitude(literal)?;
    let bit_width =
        u32::try_from(integer_type.byte_width() * 8).expect("integer widths are fixed and fit u32");
    let signed = matches!(
        integer_type,
        IntegerType::I8
            | IntegerType::I16
            | IntegerType::I32
            | IntegerType::I64
            | IntegerType::Isize
    );

    let bits = if unary_negative {
        if !signed {
            return Err(IntegerLiteralError::NegativeUnsigned(integer_type));
        }
        let maximum_magnitude = 1_u128 << (bit_width - 1);
        if magnitude > maximum_magnitude {
            return Err(IntegerLiteralError::NegativeOutOfRange(integer_type));
        }
        if magnitude == 0 {
            0
        } else {
            (1_u128 << bit_width) - magnitude
        }
    } else {
        let maximum = if signed {
            (1_u128 << (bit_width - 1)) - 1
        } else {
            (1_u128 << bit_width) - 1
        };
        if magnitude > maximum {
            return Err(IntegerLiteralError::PositiveOutOfRange(integer_type));
        }
        magnitude
    };

    let bytes = bits.to_le_bytes();
    Ok(TypedIntegerLiteral {
        integer_type,
        little_endian_bits: bytes[..integer_type.byte_width()].into(),
    })
}

fn parse_magnitude(literal: &IntegerLiteral) -> Result<u128, IntegerLiteralError> {
    let radix = match literal.base {
        NumericBase::Binary => 2,
        NumericBase::Octal => 8,
        NumericBase::Decimal => 10,
        NumericBase::Hexadecimal => 16,
    };
    let mut value = 0_u128;
    for byte in literal.digits.bytes() {
        let digit = match byte {
            b'0'..=b'9' => u128::from(byte - b'0'),
            b'a'..=b'f' => u128::from(byte - b'a' + 10),
            b'A'..=b'F' => u128::from(byte - b'A' + 10),
            _ => return Err(IntegerLiteralError::MagnitudeTooLarge),
        };
        if digit >= radix {
            return Err(IntegerLiteralError::MagnitudeTooLarge);
        }
        value = value
            .checked_mul(radix)
            .and_then(|value| value.checked_add(digit))
            .ok_or(IntegerLiteralError::MagnitudeTooLarge)?;
    }
    Ok(value)
}

const fn integer_suffix_type(suffix: IntegerSuffix) -> IntegerType {
    match suffix {
        IntegerSuffix::I8 => IntegerType::I8,
        IntegerSuffix::I16 => IntegerType::I16,
        IntegerSuffix::I32 => IntegerType::I32,
        IntegerSuffix::I64 => IntegerType::I64,
        IntegerSuffix::Isize => IntegerType::Isize,
        IntegerSuffix::U8 => IntegerType::U8,
        IntegerSuffix::U16 => IntegerType::U16,
        IntegerSuffix::U32 => IntegerType::U32,
        IntegerSuffix::U64 => IntegerType::U64,
        IntegerSuffix::Usize => IntegerType::Usize,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FloatType {
    F32,
    F64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedFloatLiteral {
    float_type: FloatType,
    raw_bits: u64,
    little_endian_bits: Box<[u8]>,
}

impl TypedFloatLiteral {
    pub const fn float_type(&self) -> FloatType {
        self.float_type
    }

    pub const fn raw_bits(&self) -> u64 {
        self.raw_bits
    }

    pub fn little_endian_bits(&self) -> &[u8] {
        &self.little_endian_bits
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FloatLiteralError {
    SuffixContextMismatch {
        suffix: FloatType,
        context: FloatType,
    },
    InvalidSpelling,
    FiniteOverflow(FloatType),
}

#[derive(Clone, Copy)]
struct FloatFormat {
    ty: FloatType,
    precision: u32,
    fraction_bits: u32,
    total_bits: u32,
    minimum_normal_exponent: i128,
    maximum_normal_exponent: i128,
    exponent_bias: i128,
}

impl FloatFormat {
    const fn for_type(ty: FloatType) -> Self {
        match ty {
            FloatType::F32 => Self {
                ty,
                precision: 24,
                fraction_bits: 23,
                total_bits: 32,
                minimum_normal_exponent: -126,
                maximum_normal_exponent: 127,
                exponent_bias: 127,
            },
            FloatType::F64 => Self {
                ty,
                precision: 53,
                fraction_bits: 52,
                total_bits: 64,
                minimum_normal_exponent: -1022,
                maximum_normal_exponent: 1023,
                exponent_bias: 1023,
            },
        }
    }
}

struct PositiveRational {
    numerator: BigUint,
    denominator: BigUint,
    binary_exponent: i128,
}

/// Converts one validated finite spelling exactly once using
/// round-to-nearest, ties-to-even. Unary minus is applied only after positive
/// conversion, preserving the required negative-zero behavior.
pub fn check_float_literal(
    literal: &FloatLiteral,
    contextual_type: Option<FloatType>,
    unary_negative: bool,
) -> Result<TypedFloatLiteral, FloatLiteralError> {
    let suffix_type = literal.suffix.map(float_suffix_type);
    if let (Some(suffix), Some(context)) = (suffix_type, contextual_type) {
        if suffix != context {
            return Err(FloatLiteralError::SuffixContextMismatch { suffix, context });
        }
    }
    let float_type = suffix_type.or(contextual_type).unwrap_or(FloatType::F64);
    let format = FloatFormat::for_type(float_type);
    let rational = parse_float_rational(literal, format)?;
    let mut raw_bits = round_rational_to_ieee(&rational, format)?;
    if unary_negative {
        raw_bits |= 1_u64 << (format.total_bits - 1);
    }
    let bytes = raw_bits.to_le_bytes();
    let width = usize::try_from(format.total_bits / 8).expect("IEEE width fits usize");
    Ok(TypedFloatLiteral {
        float_type,
        raw_bits,
        little_endian_bits: bytes[..width].into(),
    })
}

fn parse_float_rational(
    literal: &FloatLiteral,
    format: FloatFormat,
) -> Result<PositiveRational, FloatLiteralError> {
    let body = strip_expected_float_suffix(literal)?;
    let body = body.replace('_', "");
    match literal.base {
        NumericBase::Decimal => parse_decimal_rational(&body, format),
        NumericBase::Hexadecimal => parse_hexadecimal_rational(&body, format),
        NumericBase::Binary | NumericBase::Octal => Err(FloatLiteralError::InvalidSpelling),
    }
}

fn strip_expected_float_suffix(literal: &FloatLiteral) -> Result<&str, FloatLiteralError> {
    match literal.suffix {
        Some(FloatSuffix::F32) => literal
            .raw
            .strip_suffix("f32")
            .ok_or(FloatLiteralError::InvalidSpelling),
        Some(FloatSuffix::F64) => literal
            .raw
            .strip_suffix("f64")
            .ok_or(FloatLiteralError::InvalidSpelling),
        None => Ok(&literal.raw),
    }
}

fn parse_decimal_rational(
    body: &str,
    format: FloatFormat,
) -> Result<PositiveRational, FloatLiteralError> {
    if !body.contains('.') && !body.contains(['e', 'E']) {
        return Err(FloatLiteralError::InvalidSpelling);
    }
    let (significand, exponent) = split_exponent(body, ['e', 'E'])?;
    let exponent = parse_signed_exponent(exponent)?;
    let (whole, fraction) = significand.split_once('.').unwrap_or((significand, ""));
    if whole.is_empty() {
        return Err(FloatLiteralError::InvalidSpelling);
    }
    let mut digits = format!("{whole}{fraction}");
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(FloatLiteralError::InvalidSpelling);
    }
    let leading = digits
        .bytes()
        .position(|byte| byte != b'0')
        .unwrap_or(digits.len());
    digits.drain(..leading);
    if digits.is_empty() {
        return Ok(zero_rational());
    }
    let mut decimal_power = exponent.saturating_sub(
        i128::try_from(fraction.len()).map_err(|_| FloatLiteralError::InvalidSpelling)?,
    );
    while digits.ends_with('0') {
        digits.pop();
        decimal_power = decimal_power.saturating_add(1);
    }

    let scientific_exponent = i128::try_from(digits.len())
        .map_err(|_| FloatLiteralError::InvalidSpelling)?
        .saturating_sub(1)
        .saturating_add(decimal_power);
    if scientific_exponent > 400 {
        return Err(FloatLiteralError::FiniteOverflow(format.ty));
    }
    if scientific_exponent < -500 {
        return Ok(zero_rational());
    }

    let mut numerator =
        BigUint::parse_bytes(digits.as_bytes(), 10).ok_or(FloatLiteralError::InvalidSpelling)?;
    let mut denominator = BigUint::one();
    if decimal_power >= 0 {
        numerator *= pow_biguint(10, decimal_power)?;
    } else {
        denominator = pow_biguint(
            10,
            decimal_power
                .checked_neg()
                .ok_or(FloatLiteralError::InvalidSpelling)?,
        )?;
    }
    Ok(PositiveRational {
        numerator,
        denominator,
        binary_exponent: 0,
    })
}

fn parse_hexadecimal_rational(
    body: &str,
    format: FloatFormat,
) -> Result<PositiveRational, FloatLiteralError> {
    if !body.contains(['p', 'P']) {
        return Err(FloatLiteralError::InvalidSpelling);
    }
    let body = body
        .strip_prefix("0x")
        .ok_or(FloatLiteralError::InvalidSpelling)?;
    let (significand, exponent) = split_exponent(body, ['p', 'P'])?;
    let exponent = parse_signed_exponent(exponent)?;
    let (whole, fraction) = significand.split_once('.').unwrap_or((significand, ""));
    if whole.is_empty() {
        return Err(FloatLiteralError::InvalidSpelling);
    }
    let digits = format!("{whole}{fraction}");
    if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(FloatLiteralError::InvalidSpelling);
    }
    let numerator =
        BigUint::parse_bytes(digits.as_bytes(), 16).ok_or(FloatLiteralError::InvalidSpelling)?;
    if numerator.is_zero() {
        return Ok(zero_rational());
    }
    let fractional_bits = i128::try_from(fraction.len())
        .map_err(|_| FloatLiteralError::InvalidSpelling)?
        .saturating_mul(4);
    let binary_exponent = exponent.saturating_sub(fractional_bits);
    let effective_exponent = i128::from(numerator.bits())
        .saturating_sub(1)
        .saturating_add(binary_exponent);
    if effective_exponent > format.maximum_normal_exponent + 1 {
        return Err(FloatLiteralError::FiniteOverflow(format.ty));
    }
    if effective_exponent < format.minimum_normal_exponent - i128::from(format.precision) - 1 {
        return Ok(zero_rational());
    }
    Ok(PositiveRational {
        numerator,
        denominator: BigUint::one(),
        binary_exponent,
    })
}

fn split_exponent<const N: usize>(
    body: &str,
    markers: [char; N],
) -> Result<(&str, &str), FloatLiteralError> {
    let marker = body
        .char_indices()
        .find(|(_, character)| markers.contains(character));
    match marker {
        Some((index, _)) => {
            let exponent = body
                .get(index + 1..)
                .ok_or(FloatLiteralError::InvalidSpelling)?;
            if exponent.is_empty() {
                return Err(FloatLiteralError::InvalidSpelling);
            }
            Ok((&body[..index], exponent))
        }
        None => Ok((body, "0")),
    }
}

fn parse_signed_exponent(value: &str) -> Result<i128, FloatLiteralError> {
    let (negative, digits) = if let Some(value) = value.strip_prefix('-') {
        (true, value)
    } else if let Some(value) = value.strip_prefix('+') {
        (false, value)
    } else {
        (false, value)
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(FloatLiteralError::InvalidSpelling);
    }
    let mut exponent = 0_i128;
    for byte in digits.bytes() {
        let digit = i128::from(byte - b'0');
        exponent = exponent.saturating_mul(10).saturating_add(digit);
    }
    Ok(if negative {
        exponent.saturating_neg()
    } else {
        exponent
    })
}

fn pow_biguint(base: u8, exponent: i128) -> Result<BigUint, FloatLiteralError> {
    let mut exponent = u128::try_from(exponent).map_err(|_| FloatLiteralError::InvalidSpelling)?;
    let mut factor = BigUint::from(base);
    let mut result = BigUint::one();
    while exponent != 0 {
        if exponent & 1 == 1 {
            result *= &factor;
        }
        exponent >>= 1;
        if exponent != 0 {
            factor = &factor * &factor;
        }
    }
    Ok(result)
}

fn zero_rational() -> PositiveRational {
    PositiveRational {
        numerator: BigUint::zero(),
        denominator: BigUint::one(),
        binary_exponent: 0,
    }
}

fn round_rational_to_ieee(
    rational: &PositiveRational,
    format: FloatFormat,
) -> Result<u64, FloatLiteralError> {
    if rational.numerator.is_zero() {
        return Ok(0);
    }
    let base_exponent = floor_log2_ratio(&rational.numerator, &rational.denominator)?;
    let mut exponent = base_exponent
        .checked_add(rational.binary_exponent)
        .ok_or(FloatLiteralError::FiniteOverflow(format.ty))?;
    if exponent > format.maximum_normal_exponent {
        return Err(FloatLiteralError::FiniteOverflow(format.ty));
    }

    let hidden_bit = 1_u64 << (format.precision - 1);
    if exponent >= format.minimum_normal_exponent {
        let shift = rational
            .binary_exponent
            .checked_add(i128::from(format.precision - 1))
            .and_then(|value| value.checked_sub(exponent))
            .ok_or(FloatLiteralError::InvalidSpelling)?;
        let mut significand =
            round_scaled_ratio(&rational.numerator, &rational.denominator, shift)?;
        if significand == (hidden_bit << 1) {
            significand >>= 1;
            exponent += 1;
            if exponent > format.maximum_normal_exponent {
                return Err(FloatLiteralError::FiniteOverflow(format.ty));
            }
        }
        if significand < hidden_bit || significand >= (hidden_bit << 1) {
            return Err(FloatLiteralError::InvalidSpelling);
        }
        let exponent_field = u64::try_from(exponent + format.exponent_bias)
            .map_err(|_| FloatLiteralError::InvalidSpelling)?;
        let fraction = significand - hidden_bit;
        return Ok((exponent_field << format.fraction_bits) | fraction);
    }

    // Values below half the minimum subnormal round to positive zero without
    // constructing an enormous shifted denominator.
    if exponent < format.minimum_normal_exponent - i128::from(format.precision) {
        return Ok(0);
    }
    let shift = rational
        .binary_exponent
        .checked_add(i128::from(format.precision - 1))
        .and_then(|value| value.checked_sub(format.minimum_normal_exponent))
        .ok_or(FloatLiteralError::InvalidSpelling)?;
    let fraction = round_scaled_ratio(&rational.numerator, &rational.denominator, shift)?;
    if fraction > hidden_bit {
        return Err(FloatLiteralError::InvalidSpelling);
    }
    if fraction == hidden_bit {
        // Rounded up to the smallest normal value.
        return Ok(1_u64 << format.fraction_bits);
    }
    Ok(fraction)
}

fn floor_log2_ratio(numerator: &BigUint, denominator: &BigUint) -> Result<i128, FloatLiteralError> {
    let delta = i128::from(numerator.bits()) - i128::from(denominator.bits());
    if delta >= 0 {
        let shift = usize::try_from(delta).map_err(|_| FloatLiteralError::InvalidSpelling)?;
        Ok(if numerator < &(denominator << shift) {
            delta - 1
        } else {
            delta
        })
    } else {
        let shift = usize::try_from(-delta).map_err(|_| FloatLiteralError::InvalidSpelling)?;
        Ok(if &(numerator << shift) < denominator {
            delta - 1
        } else {
            delta
        })
    }
}

fn round_scaled_ratio(
    numerator: &BigUint,
    denominator: &BigUint,
    binary_shift: i128,
) -> Result<u64, FloatLiteralError> {
    let (scaled_numerator, scaled_denominator) = if binary_shift >= 0 {
        let shift =
            usize::try_from(binary_shift).map_err(|_| FloatLiteralError::InvalidSpelling)?;
        (numerator << shift, denominator.clone())
    } else {
        let shift =
            usize::try_from(-binary_shift).map_err(|_| FloatLiteralError::InvalidSpelling)?;
        (numerator.clone(), denominator << shift)
    };
    let quotient = &scaled_numerator / &scaled_denominator;
    let remainder = &scaled_numerator % &scaled_denominator;
    let mut quotient = quotient
        .to_u64()
        .ok_or(FloatLiteralError::InvalidSpelling)?;
    let twice_remainder = remainder << 1_usize;
    if twice_remainder > scaled_denominator
        || (twice_remainder == scaled_denominator && quotient & 1 == 1)
    {
        quotient = quotient
            .checked_add(1)
            .ok_or(FloatLiteralError::InvalidSpelling)?;
    }
    Ok(quotient)
}

const fn float_suffix_type(suffix: FloatSuffix) -> FloatType {
    match suffix {
        FloatSuffix::F32 => FloatType::F32,
        FloatSuffix::F64 => FloatType::F64,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn literal(base: NumericBase, digits: &str, suffix: Option<IntegerSuffix>) -> IntegerLiteral {
        IntegerLiteral {
            base,
            digits: Arc::from(digits),
            suffix,
            raw: Arc::from(digits),
        }
    }

    #[test]
    fn unsuffixed_literals_use_context_or_default_i32() {
        let value = check_integer_literal(&literal(NumericBase::Decimal, "42", None), None, false)
            .expect("default i32");
        assert_eq!(value.integer_type(), IntegerType::I32);
        assert_eq!(value.little_endian_bits(), &[42, 0, 0, 0]);

        let value = check_integer_literal(
            &literal(NumericBase::Hexadecimal, "ff", None),
            Some(IntegerType::U16),
            false,
        )
        .expect("contextual u16");
        assert_eq!(value.integer_type(), IntegerType::U16);
        assert_eq!(value.little_endian_bits(), &[0xff, 0]);
    }

    #[test]
    fn every_base_converts_to_the_same_exact_bits() {
        for (base, digits) in [
            (NumericBase::Binary, "101010"),
            (NumericBase::Octal, "52"),
            (NumericBase::Decimal, "42"),
            (NumericBase::Hexadecimal, "2A"),
        ] {
            assert_eq!(
                check_integer_literal(
                    &literal(base, digits, Some(IntegerSuffix::U8)),
                    None,
                    false,
                )
                .expect("valid base")
                .little_endian_bits(),
                &[42]
            );
        }
    }

    #[test]
    fn signed_minimum_is_valid_only_after_unary_negative_interpretation() {
        let magnitude = literal(NumericBase::Decimal, "2147483648", None);
        assert_eq!(
            check_integer_literal(&magnitude, Some(IntegerType::I32), false),
            Err(IntegerLiteralError::PositiveOutOfRange(IntegerType::I32))
        );
        assert_eq!(
            check_integer_literal(&magnitude, Some(IntegerType::I32), true)
                .expect("i32 minimum")
                .little_endian_bits(),
            &[0, 0, 0, 0x80]
        );
        let too_large = literal(NumericBase::Decimal, "2147483649", None);
        assert_eq!(
            check_integer_literal(&too_large, Some(IntegerType::I32), true),
            Err(IntegerLiteralError::NegativeOutOfRange(IntegerType::I32))
        );
    }

    #[test]
    fn unsigned_range_and_negative_forms_are_rejected_exactly() {
        assert_eq!(
            check_integer_literal(
                &literal(NumericBase::Decimal, "256", Some(IntegerSuffix::U8)),
                None,
                false,
            ),
            Err(IntegerLiteralError::PositiveOutOfRange(IntegerType::U8))
        );
        assert_eq!(
            check_integer_literal(
                &literal(NumericBase::Decimal, "1", Some(IntegerSuffix::U8)),
                None,
                true,
            ),
            Err(IntegerLiteralError::NegativeUnsigned(IntegerType::U8))
        );
    }

    #[test]
    fn suffix_and_context_must_be_identical() {
        assert_eq!(
            check_integer_literal(
                &literal(NumericBase::Decimal, "1", Some(IntegerSuffix::I64)),
                Some(IntegerType::I32),
                false,
            ),
            Err(IntegerLiteralError::SuffixContextMismatch {
                suffix: IntegerType::I64,
                context: IntegerType::I32,
            })
        );
    }

    #[test]
    fn arbitrarily_large_source_magnitudes_fail_closed_without_host_wrapping() {
        let huge = literal(
            NumericBase::Decimal,
            "999999999999999999999999999999999999999999999999999999999999999999999999",
            None,
        );
        assert_eq!(
            check_integer_literal(&huge, Some(IntegerType::U64), false),
            Err(IntegerLiteralError::MagnitudeTooLarge)
        );
    }

    fn float(raw: &str, suffix: Option<FloatSuffix>) -> FloatLiteral {
        FloatLiteral {
            base: if raw.starts_with("0x") {
                NumericBase::Hexadecimal
            } else {
                NumericBase::Decimal
            },
            raw: Arc::from(raw),
            suffix,
        }
    }

    #[test]
    fn float_defaults_contexts_and_suffixes_are_exact() {
        let value = check_float_literal(&float("1.0", None), None, false).expect("default f64");
        assert_eq!(value.float_type(), FloatType::F64);
        assert_eq!(value.raw_bits(), 1.0_f64.to_bits());
        let value = check_float_literal(
            &float("1.0f32", Some(FloatSuffix::F32)),
            Some(FloatType::F32),
            false,
        )
        .expect("suffixed f32");
        assert_eq!(value.raw_bits(), u64::from(1.0_f32.to_bits()));
        assert_eq!(value.little_endian_bits(), &1.0_f32.to_bits().to_le_bytes());
    }

    #[test]
    fn decimal_halfway_rounds_to_even_and_one_unit_above_rounds_up() {
        let halfway = float("1.000000059604644775390625f32", Some(FloatSuffix::F32));
        assert_eq!(
            check_float_literal(&halfway, None, false)
                .expect("halfway")
                .raw_bits(),
            u64::from(1.0_f32.to_bits())
        );
        let above = float("1.000000059604644775390626f32", Some(FloatSuffix::F32));
        assert_eq!(
            check_float_literal(&above, None, false)
                .expect("above halfway")
                .raw_bits(),
            u64::from(1.0_f32.to_bits() + 1)
        );

        let halfway = float(
            "1.00000000000000011102230246251565404236316680908203125f64",
            Some(FloatSuffix::F64),
        );
        assert_eq!(
            check_float_literal(&halfway, None, false)
                .expect("f64 halfway")
                .raw_bits(),
            1.0_f64.to_bits()
        );
        let above = float(
            "1.00000000000000011102230246251565404236316680908203126f64",
            Some(FloatSuffix::F64),
        );
        assert_eq!(
            check_float_literal(&above, None, false)
                .expect("f64 above halfway")
                .raw_bits(),
            1.0_f64.to_bits() + 1
        );
    }

    #[test]
    fn hexadecimal_subnormal_maximum_and_overflow_boundaries_are_exact() {
        assert_eq!(
            check_float_literal(&float("0x1p-149f32", Some(FloatSuffix::F32)), None, false,)
                .expect("minimum subnormal")
                .raw_bits(),
            1
        );
        assert_eq!(
            check_float_literal(&float("0x1p-150f32", Some(FloatSuffix::F32)), None, false,)
                .expect("half minimum subnormal")
                .raw_bits(),
            0
        );
        assert_eq!(
            check_float_literal(
                &float("0x1.fffffep127f32", Some(FloatSuffix::F32)),
                None,
                false,
            )
            .expect("maximum finite")
            .raw_bits(),
            u64::from(f32::MAX.to_bits())
        );
        assert_eq!(
            check_float_literal(
                &float("0x1.ffffffp127f32", Some(FloatSuffix::F32)),
                None,
                false,
            ),
            Err(FloatLiteralError::FiniteOverflow(FloatType::F32))
        );

        assert_eq!(
            check_float_literal(&float("0x1p-1074f64", Some(FloatSuffix::F64)), None, false,)
                .expect("f64 minimum subnormal")
                .raw_bits(),
            1
        );
        assert_eq!(
            check_float_literal(&float("0x1p-1075f64", Some(FloatSuffix::F64)), None, false,)
                .expect("f64 half minimum subnormal")
                .raw_bits(),
            0
        );
        assert_eq!(
            check_float_literal(
                &float("0x1.fffffffffffffp1023f64", Some(FloatSuffix::F64)),
                None,
                false,
            )
            .expect("f64 maximum finite")
            .raw_bits(),
            f64::MAX.to_bits()
        );
        assert_eq!(
            check_float_literal(
                &float("0x1.fffffffffffff8p1023f64", Some(FloatSuffix::F64)),
                None,
                false,
            ),
            Err(FloatLiteralError::FiniteOverflow(FloatType::F64))
        );
    }

    #[test]
    fn unary_minus_is_applied_after_rounding_and_preserves_negative_zero() {
        assert_eq!(
            check_float_literal(&float("0.0f32", Some(FloatSuffix::F32)), None, true)
                .expect("negative zero")
                .raw_bits(),
            u64::from((-0.0_f32).to_bits())
        );
    }

    #[test]
    fn float_suffix_and_context_must_be_identical() {
        assert_eq!(
            check_float_literal(
                &float("1.0f32", Some(FloatSuffix::F32)),
                Some(FloatType::F64),
                false,
            ),
            Err(FloatLiteralError::SuffixContextMismatch {
                suffix: FloatType::F32,
                context: FloatType::F64,
            })
        );
    }
}
