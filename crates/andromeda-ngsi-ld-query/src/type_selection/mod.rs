//! The NGSI-LD Entity Type Selection Language (clause 4.17).
//!
//! The `type` query parameter carries a boolean expression over entity type
//! names. This module owns its grammar, its parser, expansion of compact names
//! to IRIs, and the SQL rendering of the expanded tree.

pub mod expansion;
pub mod grammar;
pub mod parser;
