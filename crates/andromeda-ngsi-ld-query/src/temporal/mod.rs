//! Temporal queries: attribute history, instance paging, and aggregation (clause 4.5.19,
//! clause 5.7.x).

pub mod aggregation_period;
pub mod compiled;
pub mod continuation;
pub mod contributed_entities;
pub mod earlier_deletions;
pub mod grouping;
pub mod history_delete;
pub mod instance_page;
pub mod instance_selection;
pub mod ordering;
pub mod period_budget;
pub mod period_grid;
pub mod representation;
pub mod window;
