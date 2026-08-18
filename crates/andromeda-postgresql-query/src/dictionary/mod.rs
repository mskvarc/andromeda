//! What a rendered statement knows about the name dictionaries.
//!
//! Attribute names and entity type names are both interned in per-tenant
//! dictionaries, and every statement that filters on one binds its `INTEGER`
//! key rather than its IRI. The modules here are the statement side of that
//! contract: which name types are interned ([`interned_name`]), the resolved
//! keys a statement is rendered against ([`resolved_names`]), and the pair of
//! them one query needs ([`query_names`]). Reading and minting the keys is the
//! dictionary crate's job, above this one.

pub mod interned_name;
pub mod query_names;
pub mod resolved_names;
