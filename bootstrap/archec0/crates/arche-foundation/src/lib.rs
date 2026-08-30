//! Shared, behavior-neutral contracts for the Arche bootstrap toolchain.
//!
//! This crate is the narrow boundary shared by the internal rchec0 seed and
//! the public rche driver. M27 implementations build on these definitions;
//! M26 execution remains in rchec0 until the later gates replace it.

pub mod elf64;
pub mod envelope;
pub mod identity;
pub mod status;
