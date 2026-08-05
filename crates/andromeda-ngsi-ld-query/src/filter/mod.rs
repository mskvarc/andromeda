//! The NGSI-LD Query Language (ETSI GS CIM 009 v1.9.1 clause 4.9).
//!
//! The `q` query parameter selects entities by the values of their attributes.
//! This module owns the whole pipeline for it: the grammar it parses into, the
//! lexer and recursive-descent parser, term expansion through the active
//! JSON-LD context, in-memory evaluation, serialisation back to clause 4.9
//! syntax, and the SQL renderings.

pub mod eval;
pub mod expansion;
pub mod expression;
pub mod lexer;
pub mod parser;
pub mod path;
pub mod predicate;
pub mod scalar;
pub mod serialize;
