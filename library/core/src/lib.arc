// Arche Core Standard Library (library/core)
// Substrate primitives, algebraic types, and core traits.

pub enum Option<T> {
    None,
    Some(T),
}

pub enum Result<T, E> {
    Ok(T),
    Err(E),
}

pub enum GeneratorState<Y, R> {
    Yielded(Y),
    Complete(R),
}

pub struct MaybeUninit<T> {
    value: T,
}

pub struct Pin<P> {
    pointer: P,
}

pub trait Clone {
    fn clone(&self) -> Self;
}

pub trait Copy: Clone {}

pub trait Default {
    fn default() -> Self;
}

pub trait PartialEq {
    fn eq(&self, other: &Self) -> bool;
}

pub trait Eq: PartialEq {}

pub trait PartialOrd: PartialEq {
    fn partial_cmp(&self, other: &Self) -> Option<i32>;
}

pub trait Ord: Eq + PartialOrd {
    fn cmp(&self, other: &Self) -> i32;
}

pub trait Add<Rhs = Self> {
    type Output;
    fn add(self, rhs: Rhs) -> Self::Output;
}

pub trait Sub<Rhs = Self> {
    type Output;
    fn sub(self, rhs: Rhs) -> Self::Output;
}

pub trait Mul<Rhs = Self> {
    type Output;
    fn mul(self, rhs: Rhs) -> Self::Output;
}

pub trait Div<Rhs = Self> {
    type Output;
    fn div(self, rhs: Rhs) -> Self::Output;
}

pub trait Rem<Rhs = Self> {
    type Output;
    fn rem(self, rhs: Rhs) -> Self::Output;
}

pub trait Send {}
pub trait Sync {}
pub trait Unpin {}
pub trait Drop {
    fn drop(&mut self);
}

pub fn panic(msg: &str) -> ! {
    loop {}
}