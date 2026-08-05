//! The NGSI-LD Entity Type Selection Language (clause 4.17).
//!
//! The `type` query parameter carries a boolean expression over entity type
//! names. This module owns its grammar, its parser, its in-memory evaluation
//! and the expansion of compact names to IRIs; rendering the expanded tree
//! against a store belongs to that backend.

pub mod expansion;
pub mod grammar;
pub mod parser;
