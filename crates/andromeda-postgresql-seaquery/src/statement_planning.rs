//! How PostgreSQL plans a statement the store executes, and the transaction
//! that makes it plan one with the values bound to it.
//!
//! A prepared statement may run on a generic plan, made once without its
//! parameter values. Under the default `plan_cache_mode = auto` PostgreSQL
//! switches to it after five executions when its estimated cost is not above
//! the custom plans' average, and `force_generic_plan` uses it always. The
//! setting applies to an unnamed statement as much as to a named one, so
//! sending a statement unnamed does not escape it; setting
//! `force_custom_plan` for the statement's transaction does.
//!
//! A statement of branches gated by one-time filters runs one branch, but a
//! `Gather` planned under a branch whose gate fails still launches its
//! workers: they start with the executor and only then read the gate. Such a
//! statement runs without parallel workers.

use crate::{
    setting::Setting,
    transaction::{Isolation, TransactionStart},
};

/// How PostgreSQL may plan a statement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatementPlanning {
    /// Any plan the plan cache chooses serves it, generic or custom.
    Cached,
    /// Planned with its bound values on every execution: its best access
    /// path depends on them, and a generic plan, which cannot estimate them,
    /// takes another.
    BoundValues,
    /// Planned with its bound values, and run without parallel workers: a
    /// `UNION ALL` of branches each gated by a one-time filter.
    GatedBranches,
    /// Planned once per connection without its values: its one access path
    /// serves every value it is given, while the custom plans' exact count of
    /// a bound array makes the generic plan, which counts ten elements, look
    /// dearer than it is and keeps the plan cache from adopting it.
    Generic,
}

impl StatementPlanning {
    /// The transaction a statement planned this way runs in, whatever the
    /// server, database or role sets; `None` when it runs on the pool as it
    /// comes.
    #[must_use]
    pub fn transaction(self) -> Option<TransactionStart> {
        let custom_plans = || TransactionStart::at(Isolation::ReadCommitted).with_local(Setting::PlanCacheMode, "force_custom_plan");
        match self {
            StatementPlanning::Cached => None,
            StatementPlanning::BoundValues => Some(custom_plans()),
            StatementPlanning::GatedBranches => Some(custom_plans().with_local(Setting::MaxParallelWorkersPerGather, "0")),
            StatementPlanning::Generic => Some(TransactionStart::at(Isolation::ReadCommitted).with_local(Setting::PlanCacheMode, "force_generic_plan")),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{command::rendering::RenderedCommand, statement_planning::StatementPlanning};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_cached_statement_runs_outside_a_transaction_of_its_own() {
        assert_eq!(StatementPlanning::Cached.transaction(), None);
    }

    #[test]
    fn bound_value_planning_forces_custom_plans_for_its_transaction_alone() {
        assert_eq!(
            StatementPlanning::BoundValues.transaction().map(|start| start.rendered()).as_deref(),
            Some("BEGIN; SET LOCAL plan_cache_mode = 'force_custom_plan'")
        );
    }

    #[test]
    fn generic_planning_forces_the_generic_plan_for_its_transaction_alone() {
        assert_eq!(
            StatementPlanning::Generic.transaction().map(|start| start.rendered()).as_deref(),
            Some("BEGIN; SET LOCAL plan_cache_mode = 'force_generic_plan'")
        );
    }

    #[test]
    fn gated_branches_are_planned_with_their_values_and_run_serially() {
        assert_eq!(
            StatementPlanning::GatedBranches.transaction().map(|start| start.rendered()).as_deref(),
            Some("BEGIN; SET LOCAL plan_cache_mode = 'force_custom_plan'; SET LOCAL max_parallel_workers_per_gather = '0'")
        );
    }
}
