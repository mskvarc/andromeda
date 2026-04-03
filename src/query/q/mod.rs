//! NGSI-LD query language (`q` parameter) parsing, AST, evaluation, and serialization.
//!
//! The `q` parameter (NGSI-LD 4.9) allows filtering entities based on property values.
//! This module provides the full pipeline: parsing the query string into an AST,
//! evaluating it against expanded JSON-LD entities, converting it to SQL conditions,
//! and serializing it back to NGSI-LD query string form.

pub(crate) mod ast;
pub(crate) mod eval;
pub(crate) mod parser;
pub(crate) mod serialize;
