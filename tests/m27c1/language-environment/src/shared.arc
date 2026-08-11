/// Shared by the library and environment targets without reopening the path.
use collections::Ordered;

pub tag Done;
pub tag Selected;

pub component Agent {
    pub id: u32,
}

pub resource Episode {
    pub index: u64,
    pub score: i64,
}

pub fn lifetime_sink<'a, T>(value: &'a T) -> &'a T {
    value
}

pub fn explicit_lifetime_call<'a, T>(value: &'a T) -> &'a T {
    lifetime_sink::<'a, T>(value)
}

pub resource SearchState<K, V, const N: usize>
where K: Clone + Ord, V: Clone {
    pub frontier: Vec<K>,
    pub scores: Map<K, V>,
    pub history: Ordered<K>,
    pub lanes: [Option<V>; N],
    pub label: String,
}

pub enum EnvironmentError {
    InvalidSeed,
    Bounds { axis: char, value: i32 },
    Message(String),
}

pub enum Cell<T> {
    Empty,
    Occupied(T),
}

pub const WIDTH: usize = 17usize;
pub const HEIGHT: usize = 17usize;
pub const AREA: usize = WIDTH * HEIGHT;
pub const LIMIT: i32 = 64i32;
pub const INCLUDED_TEXT: &'static str = include_str("data/message.txt");
pub const INCLUDED_BYTES: &'static [u8] = include_bytes("data/table.bin");

pub type Grid<T, const W: usize, const H: usize> = [T; W * H];
pub type NestedCollections<K, V> = Vec<Map<K, Option<Box<V>>>>;

pub trait ContextualMethods {
    fn read(&self) -> i32;
    fn resource<'a>(&'a self) -> &'a Episode;
    fn run(mut self, count: usize) -> usize;
    fn spawn(self) -> Agent;
}

pub trait ReceiverForms {
    fn owned(self);
    fn mutable(mut self);
    fn shared<'a>(&'a self) -> &'a Self;
    fn exclusive<'a>(&'a mut self) -> &'a mut Self;
}

pub fn const_shape<const N: usize>(values: [u8; N + 1usize]) -> [u8; N << 1usize] {
    [0u8; N << 1usize]
}

pub fn classify(
    value: Cell<i32>,
    tuple: (i32, char),
    slice: &[i32],
) throws { EnvironmentError } -> i32 {
    let Cell::Occupied(current) = value else {
        throw EnvironmentError::InvalidSeed;
    };

    let tuple_score = match tuple {
        (0i32, 'a'..='z') => 1i32,
        (left @ 1i32..10i32, letter @ 'A'..'Z') if letter != 'Q' => left,
        (left | left, _) => left,
    };

    let slice_score = match slice {
        [] => 0i32,
        [only] => *only,
        [first, .., last] => *first + *last,
    };

    let const_score = match current {
        package::shared::LIMIT => LIMIT,
        -128i32..0i32 => -1i32,
        0i32..=127i32 => current,
        _ => 128i32,
    };

    tuple_score + slice_score + const_score
}

pub fn recover(seed: i32) throws { EnvironmentError } -> i32 {
    catch classify(Cell::Occupied(seed), (seed, 'x'), &[seed]) {
        EnvironmentError::Bounds { axis: _, value: value } if value < 0i32 => -value,
        EnvironmentError::Message(message) if message == "retry" => 0i32,
        EnvironmentError::InvalidSeed => {
            throw;
        },
        _ => 1i32,
    }
}

pub fn collection_surface<K, V, const N: usize>(
    state: &mut SearchState<K, V, const N>,
    key: K,
    value: V,
) -> Option<V>
where K: Clone + Ord, V: Clone {
    state.frontier.push(key);
    state.scores.insert(key, value);
    state.scores.remove(key)
}

pub fn control_surface(mut value: i32, maybe: Cell<i32>) -> i32 {
    if let Cell::Occupied(inner) = maybe {
        value += inner;
    } else {
        value += 1i32;
    }

    while let Cell::Occupied(inner) = Cell::Occupied(value) {
        value = inner;
        break;
    }

    {
        let shadow = value;
        value = shadow;
    };

    value
}
