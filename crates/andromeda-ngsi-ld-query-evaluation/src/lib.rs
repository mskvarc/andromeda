//! Evaluating the NGSI-LD query languages against entities held in memory.
//!
//! A broker decides the `q` language (ETSI GS CIM 009 v1.9.1 clause 4.9), the
//! geo-query (clause 4.10) and the scope query (clause 4.19) in memory wherever
//! no store answers them: Subscription matching, Context Source selection,
//! Snapshot purging and the parts of a split query. This crate is that
//! execution. It reads an entity through the traits of [`queried_entity`],
//! which whatever holds the entity implements, so it depends on no entity
//! model. No storage backend lists it: a store answers a query in its own
//! statements, and reads an entity through none of these traits. What a store
//! compares while it renders a statement, such as the stored `unitCode`
//! against a `q` operand, uses the primitives of the language crate.

pub mod entity_filters;
pub mod geo_query;
pub mod q;
pub mod queried_entity;
pub mod scope_query;
pub mod split_filter;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-json-literal",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-value-order",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-q-corpus",
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
