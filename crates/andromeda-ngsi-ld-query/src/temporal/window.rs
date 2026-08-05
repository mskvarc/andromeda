//! The interval of the time axis a temporal query reads.

use andromeda_ngsi_ld_lexicon::{time_prop::TimeProp, time_rel::TimeRel};
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
