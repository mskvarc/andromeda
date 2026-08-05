//! The time axis a temporal query filters and orders on.
//!
//! `observedAt` falls back to the row's write time when the client supplied no
//! observation timestamp; every other time property is the write time itself.

use andromeda_domain::temporal::{TimeProp, TimeRel};
use chrono::{DateTime, Utc};
use typed_builder::TypedBuilder;

/// The interval of the time axis a temporal query reads (clause 4.5.19).
///
/// `timerel` and `time_at` travel together — clause 5.7.3 requires both or
/// neither — and `end_time_at` is required exactly when `timerel` is
/// [`TimeRel::Between`]. Grouping them here is what lets the SQL builders read
/// the window once rather than reassembling it from four loose fields.
#[derive(Debug, Clone, TypedBuilder)]
pub struct TemporalWindow {
    /// Temporal relationship operator (`before`, `after`, `between`).
    #[builder(default)]
    pub timerel: Option<TimeRel>,
    /// Reference time point for the temporal filter.
    #[builder(default)]
    pub time_at: Option<DateTime<Utc>>,
    /// End time point, required for `between`.
    #[builder(default)]
    pub end_time_at: Option<DateTime<Utc>>,
    /// The time property the window applies to.
    pub timeproperty: TimeProp,
}

/// Time column expression for a statement that reads `attribute_history` under
/// the alias `ah`.
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
    use super::*;

    #[test]
    fn observed_at_falls_back_to_the_write_time() {
        assert!(history_column(TimeProp::Observed).contains("COALESCE"));
        assert!(unprefixed_column(TimeProp::Observed).contains("COALESCE"));
    }

    #[test]
    fn every_other_time_property_reads_the_write_time_directly() {
        for prop in [TimeProp::Created, TimeProp::Modified, TimeProp::Deleted] {
            assert_eq!(history_column(prop), "\"ah\".\"time\"");
            assert_eq!(unprefixed_column(prop), "\"time\"");
        }
    }
}
