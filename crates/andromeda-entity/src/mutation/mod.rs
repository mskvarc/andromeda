//! Whole-entity writes: create, merge, replace and delete (clauses 5.6.1,
//! 5.6.17, 5.6.18 and 5.6.6).
//!
//! Each operation is one `impl EntityService` block in the module named after
//! it. Attribute-level mutations live in `attrs`; batch forms live in `batch`.

mod create;
mod delete;
mod merge;
mod replace;
