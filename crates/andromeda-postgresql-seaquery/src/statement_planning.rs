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

/// How PostgreSQL may plan a statement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatementPlanning {
    /// Any plan the plan cache chooses serves it, generic or custom.
    Cached,
    /// Planned with its bound values on every execution: its best access
    /// path depends on them, and a generic plan, which cannot estimate them,
    /// takes another.
    BoundValues,
}

/// Begin a transaction whose statements are planned with their bound values,
/// whatever `plan_cache_mode` the server, database or role sets.
#[must_use]
pub const fn begin_bound_value_planning() -> &'static str {
    "BEGIN; SET LOCAL plan_cache_mode = force_custom_plan"
}

#[cfg(test)]
mod tests {
    use crate::statement_planning::begin_bound_value_planning;
    use pretty_assertions::assert_eq;

    #[test]
    fn bound_value_planning_forces_custom_plans_for_its_transaction_alone() {
        assert_eq!(begin_bound_value_planning(), "BEGIN; SET LOCAL plan_cache_mode = force_custom_plan");
    }
}
