//! PostgreSQL statements that read the temporal evolution of an entity.
//!
//! Clauses 5.6.11 onward define the temporal representation and the queries
//! over it. Answering one takes two stages (select the attribute instances a
//! request addresses, then read their history) plus a separate `time_bucket`
//! aggregation path for clause 4.5.19. This crate assembles all three from the
//! fragments the shared PostgreSQL query crate renders.
//!
//! Keeping it apart from the context statements is what makes a temporal
//! backend that is not PostgreSQL a wiring choice: a `ClickHouse` profile
//! replaces this crate and reuses the context statements unchanged.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

mod aggregated_page;
pub mod aggregated_value;
pub mod aggregation;
pub mod aggregation_grid;
pub mod aggregation_group;
pub mod aggregation_grouping;
pub mod alias;
pub mod history;
mod listed_period_page;
mod period_edge;
pub mod projected_members;
pub mod recorded_shape;
pub mod scope_history;
pub mod shape_mask;
pub mod shape_mask_walk;
pub mod split_filter_leaves;
pub mod split_part_rows;
pub mod stage1;
mod stream_page;
mod stream_parts;
mod stream_probes;
pub mod structured_json;
mod value_filters;
mod value_text;

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
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-representation",
    ],
    allow_production_dependencies: ["sea-query"],
}
