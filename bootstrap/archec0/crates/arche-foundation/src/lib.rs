//! Shared, behavior-neutral contracts for the Arche bootstrap toolchain.
//!
//! This crate is the narrow boundary shared by the internal `archec0` seed and
//! the public `arche` driver. M27 implementations build on these definitions;
//! M26 execution remains in `archec0` until the later gates replace it.

pub mod envelope;
pub mod identity;
pub mod status;
