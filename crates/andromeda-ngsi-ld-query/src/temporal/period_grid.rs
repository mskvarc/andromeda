//! How the periods of an `aggrPeriodDuration` lie on the timeline (clause
//! 4.5.19.1).
//!
//! A duration of days and time is an exact number of seconds, so every period
//! lasts as long as the next and the boundaries are the origin moved by whole
//! multiples of the duration. A duration with a year or month part lasts as long
//! as the calendar months it covers, so period k runs from the origin moved by k
//! durations to the origin moved by k + 1, each boundary counted from the origin
//! rather than from the boundary before it: adding a month to a month end clamps
//! the day, and stepping from boundary to boundary would carry the clamp forward.

/// The layout of the periods of one aggregation duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodGrid {
    /// A duration without a year or month part: every period lasts the same
    /// number of seconds.
    Exact,
    /// A duration with a year or month part: a period lasts as long as the
    /// calendar months it covers, 28 to 31 days for each.
    Calendar,
}
