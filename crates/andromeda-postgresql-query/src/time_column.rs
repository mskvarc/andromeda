//! The time column a temporal statement filters and orders on.

use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;

/// Time column expression for a statement that reads `attribute_history` under
/// the alias `ah`.
///
/// Every time property is stored on its own column and is read from that column
/// alone. Clause 5.2.21 admits `observedAt`, `createdAt`, `modifiedAt` and
/// `deletedAt` as a `timeproperty`, and clause 4.11 makes `observedAt` the
/// default; a row that carries no value on the requested axis is therefore
/// outside every bounded window on that axis, and the operational `recorded_at`
/// never stands in for a missing one.
pub const fn history_column(timeproperty: TimeProp) -> &'static str {
    match timeproperty {
        TimeProp::Observed => "\"ah\".\"observed_at\"",
        TimeProp::Created => "\"ah\".\"created_at\"",
        TimeProp::Modified => "\"ah\".\"modified_at\"",
        TimeProp::Deleted => "\"ah\".\"deleted_at\"",
    }
}

/// Time column expression without a table prefix, for the outer query over a CTE
/// that already projected the columns.
pub const fn unprefixed_column(timeproperty: TimeProp) -> &'static str {
    match timeproperty {
        TimeProp::Observed => "\"observed_at\"",
        TimeProp::Created => "\"created_at\"",
        TimeProp::Modified => "\"modified_at\"",
        TimeProp::Deleted => "\"deleted_at\"",
    }
}

/// Time column expression for `attribute_history` under an arbitrary alias.
///
/// Used where a correlated subquery reads the same axis under a second alias
/// (e.g. `ah2`), so an aggregation bucket origin measures the same time
/// property the outer query filters and buckets on.
pub fn history_column_for_alias(timeproperty: TimeProp, alias: &str) -> String {
    let column = column_name(timeproperty);
    format!("\"{alias}\".\"{column}\"")
}

/// Bare column name of one time axis, for the callers that qualify it themselves.
///
/// Every relation that records the four semantic axes names them identically —
/// `attribute_history` and `scope_history` alike — so a statement over either
/// asks here for the column that answers the requested Temporal Property.
pub const fn column_name(timeproperty: TimeProp) -> &'static str {
    match timeproperty {
        TimeProp::Observed => "observed_at",
        TimeProp::Created => "created_at",
        TimeProp::Modified => "modified_at",
        TimeProp::Deleted => "deleted_at",
    }
}

#[cfg(test)]
mod tests {
    use crate::time_column::{column_name, history_column, history_column_for_alias, unprefixed_column};
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;

    /// Clause 4.11 and clause 5.2.21: the requested `timeproperty` is answered
    /// from the column that holds it, so an instance that carries no
    /// `observedAt` is not made to match one through the write clock.
    #[test]
    fn each_time_property_reads_exactly_its_own_column() {
        let expected = [
            (TimeProp::Observed, "\"ah\".\"observed_at\"", "\"observed_at\""),
            (TimeProp::Created, "\"ah\".\"created_at\"", "\"created_at\""),
            (TimeProp::Modified, "\"ah\".\"modified_at\"", "\"modified_at\""),
            (TimeProp::Deleted, "\"ah\".\"deleted_at\"", "\"deleted_at\""),
        ];
        for (prop, prefixed, unprefixed) in expected {
            pretty_assertions::assert_eq!(history_column(prop), prefixed);
            pretty_assertions::assert_eq!(unprefixed_column(prop), unprefixed);
        }
    }

    #[test]
    fn no_time_axis_falls_back_to_another_column() {
        for prop in [TimeProp::Observed, TimeProp::Created, TimeProp::Modified, TimeProp::Deleted] {
            assert!(!history_column(prop).contains("COALESCE"), "{prop}");
            assert!(!unprefixed_column(prop).contains("COALESCE"), "{prop}");
            assert!(!history_column(prop).contains("recorded_at"), "{prop}");
            assert!(!history_column_for_alias(prop, "ah2").contains("recorded_at"), "{prop}");
        }
    }

    #[test]
    fn the_aliased_column_mirrors_the_default_axis_under_a_second_alias() {
        pretty_assertions::assert_eq!(history_column_for_alias(TimeProp::Observed, "ah2"), "\"ah2\".\"observed_at\"");
        pretty_assertions::assert_eq!(history_column_for_alias(TimeProp::Created, "ah2"), "\"ah2\".\"created_at\"");
    }

    /// `scope_history` names the four axes exactly as `attribute_history` does,
    /// so both statements resolve a `timeproperty` through the same mapping.
    #[test]
    fn the_bare_axis_names_are_the_ones_every_history_relation_declares() {
        let expected = [
            (TimeProp::Observed, "observed_at"),
            (TimeProp::Created, "created_at"),
            (TimeProp::Modified, "modified_at"),
            (TimeProp::Deleted, "deleted_at"),
        ];
        for (prop, column) in expected {
            pretty_assertions::assert_eq!(column_name(prop), column);
        }
    }
}
