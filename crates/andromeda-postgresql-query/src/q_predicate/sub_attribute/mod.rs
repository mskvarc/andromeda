//! Clause 4.9 Query Terms on a sub-attribute, compiled against the
//! `sub_attributes` tree of a `current_attributes` row.
//!
//! A term on a sub-attribute reads the rows of its top-level attribute that
//! hold a tree, and rechecks the path inline through the tree
//! ([`element_path`]). The planner prices such a recheck far below what a broad
//! predicate costs, so the probe is walked in page order rather than planned
//! as a join, and an equality on one operand is offered to the page through
//! the key index as well ([`keyed_equality`]).

pub mod element_path;
pub mod keyed_equality;
pub mod member_field;
