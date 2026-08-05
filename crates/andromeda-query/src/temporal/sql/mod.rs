//! SQL generation for temporal queries.
//!
//! Temporal retrieval runs in two stages: stage 1 selects the entity ids that
//! satisfy the entity-level and temporal filters, and stage 2 reads history for
//! one entity at a time. Stage 2 is raw SQL rather than sea-query because the
//! `lastN` CTE and the `time_bucket` aggregation have no sea-query equivalent.

pub mod aggregation;
pub mod history;
pub mod stage1;
