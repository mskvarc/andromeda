//! The NGSI-LD Query Language (ETSI GS CIM 009 v1.9.1 clause 4.9).
//!
//! The `q` query parameter selects entities by the values of their attributes.
//! This module owns the language: the grammar it parses into, the lexer and
//! recursive-descent parser, term expansion through the active JSON-LD context,
//! and serialisation back to clause 4.9 syntax. How a filter is evaluated, in
//! memory or in a storage dialect, is a property of that execution and lives
//! with it.

pub mod compaction;
pub mod complexity;
pub mod expansion;
pub mod expectation;
pub mod expression;
pub mod instance_metadata;
pub mod json_key_attributes;
pub mod json_string;
pub mod language_matching;
pub mod language_range;
pub mod language_targeting;
pub mod lexer;
pub mod linked_entity;
pub mod operand;
pub mod operand_coercion;
pub mod parser;
pub mod path;
pub mod predicate;
pub mod quoting;
pub mod referenced_attributes;
#[cfg(test)]
mod round_trip_tests;
pub mod scalar;
pub mod serialize;
pub mod target_language;
pub mod token;
pub mod token_cursor;
pub mod trailing_members;
pub mod trailing_path;
pub mod value_path;
pub mod written_members;
