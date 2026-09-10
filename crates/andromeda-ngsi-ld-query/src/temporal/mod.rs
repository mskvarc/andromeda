//! Temporal queries: attribute history, `lastN`, and aggregation (clause 4.5.19,
//! clause 5.7.x).

pub mod aggregation_period;
pub mod compiled;
pub mod instance_selection;
pub mod last_n;
pub mod ordering;
pub mod representation;
pub mod window;
