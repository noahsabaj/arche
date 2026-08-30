// Arche Standard Library (library/std)
// Comprehensive runtime, ECS abstractions, Capabilities, and Prelude.

pub use core::{
    Add, Clone, Copy, Default, Div, Drop, Eq, GeneratorState, MaybeUninit, Mul,
    Option, Ord, PartialEq, PartialOrd, Pin, Rem, Result, Send, Sub, Sync,
    Unpin, panic,
};

pub use alloc::{AllocError, Arc, ArcWeak, Box, Rc, RcWeak, Vec};

pub struct String {
    vec: Vec<u8>,
}

pub struct Map<K, V> {
    keys: Vec<K>,
    values: Vec<V>,
}

pub struct MapIter<K, V> {
    index: u64,
    len: u64,
}

pub struct Caps {
    fs_enabled: bool,
    net_enabled: bool,
    clock_enabled: bool,
}

pub struct Commands {
    epoch: u64,
}

pub struct Query<T> {
    marker: *const T,
}

pub struct App {
    world_id: u64,
}