//! The physical-schema iden catalogue: the single source of truth for every
//! table, column and shared alias the PostgreSQL statements name.
//!
//! Each relation family owns a module: [`current`] for the current-state grain,
//! [`temporal`] for the temporal evolution, [`csr`] for context source
//! registrations. Isolation between them is module-only: any statement builder
//! may name any relation, and the module a relation lives in says which family
//! it belongs to.
//!
//! There is no NGSI-LD here and no dialect vocabulary, only `sea_query::Iden`
//! definitions and the column placements a relation's constraints dictate
//! ([`string_value_column`], [`uri_value_column`]), which the writer and the
//! reader of that relation must agree on. A backend that is not PostgreSQL
//! replaces the statement crates above; the identifiers a physical schema is
//! made of live here so a rename moves every reference with it.

pub mod branch_schema_version;
pub mod census;
pub mod context_source_change_clock;
pub mod csr;
pub mod current;
pub mod entity_map;
pub mod jsonld;
pub mod maintenance;
pub mod name_dictionary;
pub mod notification;
pub mod queue;
pub mod snapshot;
pub mod string_value_column;
pub mod subscription_forwarding;
pub mod subscriptions;
pub mod temporal;
pub mod temporal_intent;
pub mod tenancy;
pub mod type_membership;
pub mod uri_value_column;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["sea-query"],
}
