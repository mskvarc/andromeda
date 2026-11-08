//! The configured bounds a temporal operation is refused with
//! `TooManyResults` over (clause 5.5.6).

use strum::Display;

/// What a `TooManyResults` refusal counted, and the setting that bounds it.
///
/// The rendering names the setting, so a client or an operator reading the
/// refusal knows which bound to narrow the request under or to raise.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum ResultBound {
    /// The history instances one delete-by-query would remove.
    #[strum(serialize = "history instances to delete (extensions.temporal_delete_by_query_max_matches)")]
    DeleteByQueryMatches,
    /// The distinct groups one grouped aggregation would emit.
    #[strum(serialize = "aggregation groups (extensions.temporal_aggregation_grouping_max_groups)")]
    AggregationGroups,
    /// The periods one page of an aggregated temporal read holds per attribute
    /// stream: a page of a grid the budget bounds, or a calendar grid whose
    /// page cannot end on a boundary that lays the grid within the budget.
    #[strum(serialize = "aggregation periods one page holds per attribute stream (temporal.query.max_last_n)")]
    AggregationPeriods,
    /// The instances of one attribute sharing the instant at the edge of a
    /// temporal page, which the page carries whole.
    #[strum(serialize = "instances of one attribute sharing the instant at a page edge (temporal.max_edge_group)")]
    EdgeGroup,
}

#[cfg(test)]
mod tests {
    use crate::result_bound::ResultBound;

    /// Each bound names the setting an operator raises it with.
    #[test]
    fn each_bound_names_its_setting() {
        assert!(
            ResultBound::DeleteByQueryMatches
                .to_string()
                .ends_with("(extensions.temporal_delete_by_query_max_matches)")
        );
        assert!(
            ResultBound::AggregationGroups
                .to_string()
                .ends_with("(extensions.temporal_aggregation_grouping_max_groups)")
        );
        assert!(ResultBound::EdgeGroup.to_string().ends_with("(temporal.max_edge_group)"));
        assert!(ResultBound::AggregationPeriods.to_string().ends_with("(temporal.query.max_last_n)"));
    }
}
