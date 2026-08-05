//! The time column a temporal statement filters and orders on.

use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;

/// Time column expression for a statement that reads `attribute_history` under
/// the alias `ah`.
///
/// `observedAt` falls back to the row's write time when the client supplied no
/// observation timestamp; every other time property is the write time itself.
pub const fn history_column(timeproperty: TimeProp) -> &'static str {
    match timeproperty {
        TimeProp::Observed => "COALESCE(\"ah\".\"observed_at\", \"ah\".\"time\")",
        TimeProp::Created | TimeProp::Modified | TimeProp::Deleted => "\"ah\".\"time\"",
    }
}

/// Time column expression without a table prefix, for the outer query over a CTE
/// that already projected the columns.
pub const fn unprefixed_column(timeproperty: TimeProp) -> &'static str {
    match timeproperty {
        TimeProp::Observed => "COALESCE(\"observed_at\", \"time\")",
        TimeProp::Created | TimeProp::Modified | TimeProp::Deleted => "\"time\"",
    }
}

#[cfg(test)]
mod tests {
    use crate::time_column::{history_column, unprefixed_column};
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;

    #[test]
    fn observed_at_falls_back_to_the_write_time() {
        assert!(history_column(TimeProp::Observed).contains("COALESCE"));
        assert!(unprefixed_column(TimeProp::Observed).contains("COALESCE"));
    }

    #[test]
    fn every_other_time_property_reads_the_write_time_directly() {
        for prop in [TimeProp::Created, TimeProp::Modified, TimeProp::Deleted] {
            pretty_assertions::assert_eq!(history_column(prop), "\"ah\".\"time\"");
            pretty_assertions::assert_eq!(unprefixed_column(prop), "\"time\"");
        }
    }
}
