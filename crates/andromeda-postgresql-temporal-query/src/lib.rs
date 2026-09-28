//! PostgreSQL statements that read the temporal evolution of an entity.
//!
//! Clauses 5.6.11 onward define the temporal representation and the queries
//! over it. Answering one takes two stages — select the attribute instances a
//! request addresses, then read their history — plus a separate `time_bucket`
//! aggregation path for clause 4.5.19. This crate assembles all three from the
//! fragments `andromeda-postgresql-query` renders.
//!
//! Keeping it apart from the context statements is what makes a temporal
//! backend that is not PostgreSQL a wiring choice: a `ClickHouse` profile
//! replaces this crate and reuses the context statements unchanged.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod aggregated_value;
pub mod aggregation;
pub mod aggregation_group;
pub mod aggregation_grouping;
pub mod alias;
pub mod axis;
pub mod history;
pub mod projected_members;
pub mod recorded_shape;
pub mod scope_history;
pub mod stage1;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-postgresql-codes",
        "andromeda-postgresql-query",
        "andromeda-postgresql-schema",
        "andromeda-postgresql-seaquery",
        "andromeda-unit-catalog",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-representation",
    ],
    allow_production_dependencies: ["sea-query"],
}
