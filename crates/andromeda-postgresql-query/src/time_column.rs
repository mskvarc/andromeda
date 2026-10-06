//! The time column a temporal statement filters and orders on.

use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;

/// Bare column name of one time axis, for the callers that qualify it themselves.
///
/// Every time property is stored on its own column and is read from that column
/// alone. Clause 5.2.21 admits `observedAt`, `createdAt`, `modifiedAt` and
/// `deletedAt` as a `timeproperty`, and clause 4.11 makes `observedAt` the
/// default; a row that carries no value on the requested axis is therefore
/// outside every bounded window on that axis, and the operational `recorded_at`
/// never stands in for a missing one.
///
/// Every relation that records the four semantic axes names them identically —
/// `attribute_history` and `scope_history` alike — so a statement over either
/// asks here for the column that answers the requested Temporal Property.
#[must_use]
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
    use crate::time_column::column_name;
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;

    #[test]
    fn no_time_axis_falls_back_to_another_column() {
        for prop in [TimeProp::Observed, TimeProp::Created, TimeProp::Modified, TimeProp::Deleted] {
            assert!(!column_name(prop).contains("COALESCE"), "{prop}");
            assert!(!column_name(prop).contains("recorded_at"), "{prop}");
        }
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
