//! The NGSI-LD Query Language (ETSI GS CIM 009 v1.9.1 clause 4.9).
//!
//! The `q` query parameter selects entities by the values of their attributes.
//! This module owns the language: the grammar it parses into, the lexer and
//! recursive-descent parser, term expansion through the active JSON-LD context,
//! in-memory evaluation, and serialisation back to clause 4.9 syntax.
//! Evaluation reads an entity through the traits of
//! [`queried_entity`](crate::queried_entity). How a filter is spelled in a
//! storage dialect is a property of that backend and lives in the crate that
//! owns it.

pub mod attribute_axis;
pub mod compaction;
pub mod eval;
pub mod expansion;
pub mod expectation;
pub mod expression;
pub mod family_target;
pub mod hop_steps;
pub mod json_key_attributes;
pub mod json_string;
pub mod lexer;
pub mod linked_entity;
pub mod operand;
pub mod parser;
pub mod path;
pub mod predicate;
pub mod quoting;
pub mod referenced_attributes;
pub mod scalar;
pub mod serialize;
pub mod target_value;
pub mod token;
pub mod token_cursor;
pub mod value_path;
pub mod written_members;
