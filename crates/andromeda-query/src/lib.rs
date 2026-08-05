//! NGSI-LD query compilation, SQL generation, and query language.
//!
//! ETSI GS CIM 009 v1.9.1 defines a family of query facilities — `q` (clause
//! 4.9), `georel` (4.10), `scopeQ` (4.18), type selection (4.17), `orderBy`
//! (4.23), projection (6.3.15) and the temporal window (4.5.19) — that a broker
//! must read identically wherever they appear. This crate owns all of them: each
//! concept's grammar, its compiled model and its SQL rendering live in one
//! directory, so a change to one concept touches one directory rather than three
//! pipeline stages.
//!
//! Modules are therefore named after NGSI-LD concepts, never after stages. The
//! crate sits above [`andromeda_domain`] and [`andromeda_geometry`] and below
//! the broker itself: it knows nothing of transport, persistence or tenancy.
//! `sqlx` in particular stays out — a rendered statement leaves here as
//! [`sql::built::BuiltQuery`], and the store binds its
//! [`sql::bind::BindValue`]s.
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
pub mod sql;
pub mod temporal;
pub mod term_expander;
pub mod type_selection;
