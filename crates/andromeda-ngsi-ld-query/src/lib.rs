//! The NGSI-LD query languages: their grammars, their ASTs and their compiled forms.
//!
//! ETSI GS CIM 009 v1.9.1 defines a family of query facilities — `q` (clause
//! 4.9), `georel` (4.10), `scopeQ` (4.18), type selection (4.17), `orderBy`
//! (4.23), projection (6.3.15) and the temporal window (4.5.19) — that a broker
//! must read identically wherever they appear. This crate owns all of them: each
//! concept's grammar, its AST and its compiled model live in one directory, so
//! a change to one concept touches one directory rather than several pipeline
//! stages.
//!
//! Modules are therefore named after NGSI-LD concepts, never after stages. How
//! a compiled query is rendered as SQL is deliberately absent: that is a
//! property of a backend, and it lives in `andromeda-postgresql-query` and the
//! two statement crates above it. Nothing here names a database.
//!
//! Term expansion is likewise inverted rather than depended upon: compilation
//! that needs a term expanded against an `@context` asks a
//! [`term_expander::TermExpander`], which the JSON-LD engine implements.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod csr;
pub mod entity;
pub mod error;
pub mod filter;
pub mod geo;
pub mod identifier;
pub mod ordering;
pub mod pagination;
pub mod pattern;
pub mod projection;
pub mod scope;
pub mod temporal;
pub mod term_expander;
pub mod type_selection;
