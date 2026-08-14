//! The physical-schema iden catalogue: the single source of truth for every
//! table, column and shared alias the PostgreSQL statements name.
//!
//! Each relation family owns a module — [`current`] for the current-state grain,
//! [`temporal`] for the temporal evolution, [`csr`] for context source
//! registrations. Isolation between them is module-only: any statement builder
//! may name any relation, and the module a relation lives in says which family
//! it belongs to.
//!
//! There is no NGSI-LD here and no dialect vocabulary — only `sea_query::Iden`
//! definitions. A backend that is not PostgreSQL replaces the statement crates
//! above; the identifiers a physical schema is made of live here so a rename
//! moves every reference with it.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod csr;
pub mod current;
pub mod temporal;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["sea-query"],
}
