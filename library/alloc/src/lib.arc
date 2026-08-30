// Arche Alloc Standard Library (library/alloc)
// Dynamic heap allocations and container primitives.

use core::{Clone, Drop, Option, Result};

pub struct AllocError {
    requested_bytes: u64,
}

pub struct Box<T> {
    ptr: *mut T,
}

pub struct Vec<T> {
    ptr: *mut T,
    len: u64,
    cap: u64,
}

pub struct Rc<T> {
    ptr: *const T,
}

pub struct RcWeak<T> {
    ptr: *const T,
}

pub struct Arc<T> {
    ptr: *const T,
}

pub struct ArcWeak<T> {
    ptr: *const T,
}