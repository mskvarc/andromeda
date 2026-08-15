//! The `attribute_history` column each Temporal Property is filtered, bucketed
//! and ordered on.
//!
//! Clause 5.2.21 admits `observedAt`, `createdAt`, `modifiedAt` and `deletedAt`
//! as a `timeproperty`, and clause 4.11 makes `observedAt` the default. Every
//! axis is stored on its own column and read from that column alone: a row that
//! carries no value on the requested axis is outside every bounded window on it,
//! and the operational `recorded_at` never stands in for a missing one.

use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_postgresql_schema::temporal::AttributeHistory;

/// The `attribute_history` column that holds `timeproperty`.
pub const fn axis_column(timeproperty: TimeProp) -> AttributeHistory {
    match timeproperty {
        TimeProp::Observed => AttributeHistory::ObservedAt,
        TimeProp::Created => AttributeHistory::CreatedAt,
        TimeProp::Modified => AttributeHistory::ModifiedAt,
        TimeProp::Deleted => AttributeHistory::DeletedAt,
    }
}

#[cfg(test)]
mod tests {
    use crate::axis::axis_column;
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use sea_query::Iden;

    #[test]
    fn each_time_property_maps_to_its_own_history_column() {
        let expected = [
            (TimeProp::Observed, "observed_at"),
            (TimeProp::Created, "created_at"),
            (TimeProp::Modified, "modified_at"),
            (TimeProp::Deleted, "deleted_at"),
        ];
        for (prop, column) in expected {
            pretty_assertions::assert_eq!(Iden::to_string(&axis_column(prop)), column);
        }
    }

    #[test]
    fn no_axis_resolves_to_the_operational_write_clock() {
        for prop in [TimeProp::Observed, TimeProp::Created, TimeProp::Modified, TimeProp::Deleted] {
            assert_ne!(Iden::to_string(&axis_column(prop)), "recorded_at", "{prop}");
        }
    }
}
