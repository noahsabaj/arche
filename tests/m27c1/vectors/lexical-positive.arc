///exact doc bytes:  leading space remains
/* outer /* nested */ comment */
pub fn lexical_positive() -> () {
	let integers = (0, 1_000, 0b10_01u8, 0o7_7u16, 0xCA_FEu32, 1i64, 2isize, 3usize);
	let floats = (1., 1.0f32, 1e-2, 0x1.fp+2f64);
	let scalars = ("\\\"\'\n\r\t\0\x41\u{1f642}", '\n', '\x41', '\u{e9}');
	let ranges = match 1i32 {
		1i32..2i32 => true,
		1i32..=2i32 => false,
		_ => false,
	};
	let comparisons = 1i32 <= 2i32 && 3i32 >= 2i32 && 4i32 == 4i32 && 5i32 != 6i32;
}
