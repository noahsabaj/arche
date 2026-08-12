/// Shared by the library and binary targets from one immutable snapshot.
/* Outer block comments may contain /* nested block comments. */ */
use math::Scalar;

pub component Position {
    /// Public member visibility is retained independently from its owner.
    pub x: f32,
    pub(package) y: f32,
}

pub component GenericComponent<T, const N: usize> {
    pub values: [T; N],
}

pub resource ScoreBoard<K, V>
where K: Eq + Ord {
    pub entries: Map<K, V>,
}

pub tag Disabled;

pub struct Unit;

pub struct EmptyTuple();

pub struct EmptyRecord {}

pub enum EmptyEnum {}

pub struct TupleRecord(
    pub i32,
    pub(package) u64,
    bool,
);

pub struct Wrapper<T> {
    pub value: T,
}

pub struct Episode {
    pub index: u64,
}

pub struct TrailingGenerics<T, const N: usize,>
where T: Clone, {
    pub value: T,
    pub bytes: [u8; N],
}

pub struct Borrowed<'a: 'static, T: Clone + 'a, const N: usize>
where T: Clone + 'a, 'a: 'static {
    pub values: &'a mut [T; N],
    pub view: &'a [T],
    pub text: &'static str,
}

pub struct MethodVisibility {
    private_field: i32,
}

impl MethodVisibility {
    pub fn make() -> Self {
        MethodVisibility { private_field: 0i32 }
    }

    pub fn exposed(&self) -> i32 {
        self.private_field
    }

    fn hidden(&mut self, value: i32) {
        self.private_field = value;
        let replacement = Self::make();
        self.private_field = replacement.private_field;
    }
}

impl Unit {}

struct SyntaxOnlyPrivate;

pub enum GameError {
    Disconnected,
    Code(i32, String),
    Detailed { code: i32, message: String },
}

pub enum Choice<T> {
    None,
    One(T),
    Pair { left: T, right: T },
}

pub type UnsafeCallback<'a, T> = unsafe fn(
    &'a T,
    *const u8,
) requires { Stdio } throws { GameError } -> (i32,);

pub type SafeCallback = fn(i32,) requires {} throws {} -> i32;
pub type DivergingCallback = fn() requires {} throws {} -> !;

pub type ScalarCatalog = (
    i8, i16, i32, i64,
    u8, u16, u32, u64,
    isize, usize, f32, f64,
    bool, char, entity, (),
);

pub type PointerCatalog<'a, T, const N: usize> = (
    &'a T,
    &'a mut T,
    *const T,
    *mut T,
    [T; N],
    &'a [T],
);

pub const BIT_MASK: u64 = (1u64 << 5u64) | 3u64 ^ 2u64 & 1u64;
pub const WRAPPED_INDEX: usize = ~0usize + 2usize * 4usize / 2usize % 3usize;
pub const CONST_OPERATORS: i32 = -8i32 - 2i32 + (32i32 >> 1i32);
pub static READY: bool = true;
pub static mut GLOBAL_COUNT: i32 = 0i32;

pub trait Algebra<T>
where T: Clone {
    /// Trait methods retain exact declared effects.
    fn combine<'a>(&'a self, left: T, right: T) requires {} throws {} -> T
    where T: 'a;
    unsafe fn unchecked(&mut self, pointer: *mut T) requires { Stdio } throws { GameError };
    fn clone_self(&self) -> Self;
}

impl<T> Algebra<T> for Wrapper<T>
where T: Clone {
    fn combine<'a>(&'a self, left: T, right: T) requires {} throws {} -> T
    where T: 'a {
        left
    }

    unsafe fn unchecked(
        &mut self,
        pointer: *mut T,
    ) requires { Stdio } throws { GameError } {
        *pointer = self.value.clone();
    }

    fn clone_self(&self) -> Self {
        Wrapper { value: self.value.clone() }
    }
}

impl<T> Clone for Wrapper<T>
where T: Clone {
    fn clone(&self) -> Wrapper<T> {
        Wrapper { value: self.value.clone() }
    }
}

pub fn identity<T>(value: T) -> T {
    value
}

pub fn lifetime_sink<'a, T>(value: &'a T) -> &'a T {
    value
}

pub fn explicit_lifetime_call<'a, T>(value: &'a T) -> &'a T {
    lifetime_sink::<'a, T>(value)
}

pub struct PostfixResource {
    pub index: i32,
}

pub struct PostfixSpawn {
    pub id: i32,
}

pub struct PostfixContext {
    pub read_value: i32,
    pub resource_value: PostfixResource,
}

pub trait PostfixRead {
    fn read(&self) -> i32;
    fn resource<'a>(&'a self) -> &'a PostfixResource;
}

impl PostfixRead for PostfixContext {
    fn read(&self) -> i32 {
        self.read_value
    }

    fn resource<'a>(&'a self) -> &'a PostfixResource {
        &self.resource_value
    }
}

impl PostfixContext {
    pub fn run<T>(&mut self, value: T) -> i32 {
        let _ = value;
        self.read_value
    }

    pub fn spawn(&mut self) -> PostfixSpawn {
        PostfixSpawn { id: self.read_value }
    }
}

pub struct CounterSource {
    pub start: i32,
    pub end: i32,
}

pub struct CounterIter {
    current: i32,
    end: i32,
}

impl IntoIterator<CounterSource, CounterIter> for CounterSource {
    fn into_iter(self) -> CounterIter {
        CounterIter { current: self.start, end: self.end }
    }
}

impl Iterator<CounterIter, i32> for CounterIter {
    fn next(&mut self) -> Option<i32> {
        if self.current < self.end {
            let value = self.current;
            self.current += 1i32;
            Option::Some(value)
        } else {
            Option::None
        }
    }
}

pub fn postfix_surface(
    context: &mut PostfixContext,
    cmd: &mut Commands,
    callable: fn(i32) -> i32,
) -> i32 {
    let read_value = context.read();
    let resource_index = context.resource().index;
    let run_value = context.run::<i32>(1i32,);
    let spawned_value = context.spawn();
    cmd.spawn {};
    callable(read_value) + resource_index + run_value + spawned_value.id
}

pub fn pattern_surface(
    unit: (),
    truth: bool,
    text: &'static str,
    mut reference: &mut i32,
) -> i32 {
    let () = unit;
    let ref mut borrowed = reference;
    let borrowed_value = **borrowed;
    let truth_value = match truth {
        true => 1i32,
        false => 0i32,
    };
    let text_value = match text {
        "ready" => 1i32,
        _ => 0i32,
    };
    let reference_value = match reference {
        &mut inner => inner,
    };
    truth_value + text_value + reference_value + borrowed_value
}

pub unsafe fn generic_surface<'a: 'static, T: Clone + 'a, const N: usize>(
    mut value: T,
    ref pair: TupleRecord,
    bytes: &'a mut [u8; N],
    callback: unsafe fn(i32) requires {} throws { GameError } -> i32,
) requires {} throws { GameError } -> T
where T: Clone + 'a, 'a: 'static {
    let _: () = ();
    let singleton = (1i32,);
    let tuple = (1i32, 2u64, false, 'x');
    let empty_array: [i32; 0] = [];
    let array = [1i32, 2i32, 3i32,];
    let repeated = [0u8; N];
    let record = Wrapper::<i32> { value: 7i32 };
    let selected = identity::<i32>(record.value);
    let first = array[0usize];
    let tuple_field = tuple.0;
    let dereferenced = *bytes;
    let borrowed = &mut value;
    let arithmetic = -selected + first * 3i32 / 2i32 % 5i32;
    let shifted = arithmetic << 1i32 >> 1i32;
    let compared = shifted < 10i32 && shifted != tuple_field || READY;
    let bits = (~0i32 & 0xffi32) ^ 0x55i32 | 0x100i32;

    let _integers = (
        1i8, 2i16, 3i32, 4i64,
        5u8, 6u16, 7u32, 8u64,
        9isize, 10usize,
        0b1010u8, 0o17u16, 0xFF_FFu32,
    );
    let _floats = (
        1.0f32,
        1.f64,
        6.022e+23f64,
        0x1.fp+3f32,
        -0.0f64,
    );
    let _text = (
        "line\nquote\"slash\\nul\0tab\treturn\rcafe\u{e9}\x41",
        '\n', '\'', '\u{1f642}', '\x41',
    );

    let Choice::One(bound) = Choice::One(selected) else {
        return value;
    };
    let mut destination = arithmetic;
    destination = shifted;
    destination += 1i32;

    if compared {
        destination += 1i32;
    } else if let Choice::One(inner) = Choice::One(destination) {
        destination = inner;
    } else {
        destination = 0i32;
    }

    while destination < 4i32 {
        destination += 1i32;
        if destination == 2i32 {
            continue;
        }
        if destination == 3i32 {
            break;
        }
    }

    while let Choice::One(inner) = Choice::One(destination) {
        destination = inner;
        break;
    }

    let counter_source = CounterSource { start: 0i32, end: 3i32 };
    for element in counter_source {
        destination += element;
    };

    let loop_value = loop {
        break destination;
    };

    let matched = match (Choice::Pair { left: loop_value, right: bits }) {
        Choice::None => 0i32,
        Choice::One(0i32..=3i32) => 1i32,
        Choice::One(name @ 4i32..8i32) if name != 6i32 => name,
        Choice::Pair {
            left: left @ -10i32..0i32 | left @ 8i32..16i32,
            right: _,
        } => left,
        _ => -1i32,
    };

    let _patterns = match (singleton, array, &record) {
        ((only,), [head, middle, ..], &Wrapper { value: ref held }) if only >= 0i32 => only + head + middle + *held,
        _ => 0i32,
    };

    let recovered = catch callback(matched) {
        GameError::Code(code, _) if code > 0i32 => code,
        GameError::Detailed { code: code, message: _ } => code,
        _ => 0i32,
    };

    let closure = move |mut input: i32, ref other: TupleRecord|
        requires {} throws {} -> i32 {
            input += other.0;
            input
        };
    let generator_factory = gen move |seed: i32|
        resume i32 yields i32 requires {} throws { GameError } -> i32 {
            let resumed = yield seed;
            resumed
        };
    let generator = generator_factory(recovered);
    let resumed = generator.resume(1i32);

    unsafe {
        let array_pointer = bytes as *mut [u8; N];
        let pointer = array_pointer as *mut u8;
        *pointer = 1u8;
    };

    value
}

pub gen fn Counter(start: i32)
    resume i32 yields i32 requires {} throws { GameError } -> i32 {
    let mut current = start;
    loop {
        let next = yield current;
        current += next;
        if current > 100i32 {
            return current;
        }
    }
}

pub unsafe gen fn UnsafeCounter(start: i32)
    resume () yields i32 requires {} throws {} -> i32 {
    yield start;
    start
}

pub system GenericSystem<T, const N: usize>(
    board: read ScoreBoard<i32, i32>,
    positions: query [Position, mut GenericComponent<T, const N>, !Disabled],
    cmd: commands,
    stdio: &Stdio,
) requires { Stdio } throws { GameError } {
    for (position, _) in positions {
        cmd.spawn { Position { x: position.x, y: position.y } };
    }
}

pub schedule GenericFrame {
    run package::shared::GenericSystem::<i32, const 4>;
}

pub system EmptyQuerySystem(empty: query []) requires {} throws {} {
}

pub schedule EmptySchedule {}

pub trait EmptyTrait {}
