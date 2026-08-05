//! The assertions each workspace crate makes about its own layer.
//!
//! Two rules decide what a crate may hold, and neither is expressible in the
//! type system. [`manifest`] enforces the first: which crates a layer may name,
//! read from the crate's own `Cargo.toml`. [`dialect`] enforces the second:
//! that a specification crate does not spell a storage dialect, read from the
//! crate's own sources — because a crate can emit an entire SQL predicate as
//! string constants without declaring a single dependency, which is exactly how
//! six of them did.
//!
//! Each crate calls these from a `#[cfg(test)]` module of its own, passing its
//! own `CARGO_MANIFEST_DIR`. That placement is deliberate: a failure names the
//! crate that broke the rule rather than an unrelated one, and a crate added to
//! the workspace cannot be forgotten by a central table it was never added to.
//! The rules themselves live here so there is one reading of them rather than
//! one per crate.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod dialect;
pub mod manifest;
