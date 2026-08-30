//! Closure Fn/FnMut/FnOnce classification and pinned generator state machines for M27-C4.

use std::fmt;

use crate::mir::Place;
use arche_frontend::{Span, SymbolicType};

/// Kind of capture for a variable inside a closure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureKind {
    /// Captured by immutable reference `&T`.
    ByRefShared,
    /// Captured by mutable reference `&mut T`.
    ByRefMut,
    /// Captured by value (moved or copied).
    ByValue,
}

/// A captured place and its type within a closure frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClosureCapture {
    pub place: Place,
    pub kind: CaptureKind,
    pub ty: SymbolicType,
    pub is_copy: bool,
}

/// Inferred callable trait classification for a closure.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ClosureFnClass {
    /// Can be called multiple times via shared reference `&self`.
    Fn,
    /// Can be called multiple times via mutable reference `&mut self`.
    FnMut,
    /// Can be called only once by value `self`.
    FnOnce,
}

impl fmt::Display for ClosureFnClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fn => write!(formatter, "Fn"),
            Self::FnMut => write!(formatter, "FnMut"),
            Self::FnOnce => write!(formatter, "FnOnce"),
        }
    }
}

/// Classifies a closure into `Fn`, `FnMut`, or `FnOnce` based on its captures.
#[must_use]
pub fn classify_closure_fn_trait(captures: &[ClosureCapture]) -> ClosureFnClass {
    let mut has_mut_borrow = false;
    let mut has_move_non_copy = false;

    for capture in captures {
        match capture.kind {
            CaptureKind::ByRefShared => {}
            CaptureKind::ByRefMut => {
                has_mut_borrow = true;
            }
            CaptureKind::ByValue => {
                if !capture.is_copy {
                    has_move_non_copy = true;
                }
            }
        }
    }

    if has_move_non_copy {
        ClosureFnClass::FnOnce
    } else if has_mut_borrow {
        ClosureFnClass::FnMut
    } else {
        ClosureFnClass::Fn
    }
}

/// State discriminant of a pinned generator coroutine.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GeneratorState {
    /// Initial unresumed state.
    Unresumed,
    /// Suspended at yield point `k` (1-based).
    Yielded(u32),
    /// Completed or panicked.
    Terminal,
}

/// Descriptor of a pinned generator frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratorDescriptor {
    pub name: String,
    pub yield_count: u32,
    pub resume_type: SymbolicType,
    pub yield_type: SymbolicType,
    pub return_type: SymbolicType,
    pub captures: Vec<ClosureCapture>,
    pub is_pinned: bool,
    pub span: Option<Span>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mir::LocalId;

    #[test]
    fn closure_with_shared_borrows_is_fn() {
        let captures = vec![ClosureCapture {
            place: Place::from_local(LocalId(1)),
            kind: CaptureKind::ByRefShared,
            ty: SymbolicType::I32,
            is_copy: true,
        }];
        assert_eq!(classify_closure_fn_trait(&captures), ClosureFnClass::Fn);
    }

    #[test]
    fn closure_with_copy_by_value_is_fn() {
        let captures = vec![ClosureCapture {
            place: Place::from_local(LocalId(1)),
            kind: CaptureKind::ByValue,
            ty: SymbolicType::I32,
            is_copy: true,
        }];
        assert_eq!(classify_closure_fn_trait(&captures), ClosureFnClass::Fn);
    }

    #[test]
    fn closure_with_mutable_borrow_is_fn_mut() {
        let captures = vec![
            ClosureCapture {
                place: Place::from_local(LocalId(1)),
                kind: CaptureKind::ByRefShared,
                ty: SymbolicType::I32,
                is_copy: true,
            },
            ClosureCapture {
                place: Place::from_local(LocalId(2)),
                kind: CaptureKind::ByRefMut,
                ty: SymbolicType::I32,
                is_copy: true,
            },
        ];
        assert_eq!(classify_closure_fn_trait(&captures), ClosureFnClass::FnMut);
    }

    #[test]
    fn closure_with_non_copy_move_is_fn_once() {
        let captures = vec![ClosureCapture {
            place: Place::from_local(LocalId(1)),
            kind: CaptureKind::ByValue,
            ty: SymbolicType::Str,
            is_copy: false,
        }];
        assert_eq!(classify_closure_fn_trait(&captures), ClosureFnClass::FnOnce);
    }
}
