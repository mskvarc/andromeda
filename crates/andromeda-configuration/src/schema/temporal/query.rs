//! The `[temporal.query]` table: how much of a Temporal Evolution one read
//! returns.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

/// The largest `temporal.query.max_last_n` a deployment may configure.
///
/// Every instance of a temporal page is rendered and compacted in memory before
/// the response is written, so the ceiling bounds the work one request can ask
/// of the broker regardless of how an operator tunes the maximum below it.
pub const MAX_LAST_N_CEILING: u32 = 5000;

/// The largest `temporal.query.max_edge_group` a deployment may configure.
///
/// A page carries the instances of one attribute sharing the instant at its
/// edge whole, and renders and compacts them in memory before the response is
/// written, so the ceiling bounds the one part of a page no page size bounds.
pub const MAX_EDGE_GROUP_CEILING: u32 = 1_000_000;

/// The largest `temporal.query.max_aggregation_instances` a deployment may
/// configure.
///
/// The bound caps the raw instances one page of an aggregated temporal read
/// folds per attribute stream, so the ceiling bounds the rows one request can
/// make the database fold for each stream, as [`MAX_LAST_N_CEILING`] bounds
/// what it can make the broker render.
pub const MAX_AGGREGATION_INSTANCES_CEILING: u64 = 100_000_000;

/// The aggregation instance bound the broker reads under when no other is
/// configured.
pub const DEFAULT_MAX_AGGREGATION_INSTANCES: NonZeroU64 = NonZeroU64::MIN.saturating_add(999_999);

/// How much of a Temporal Evolution one read returns.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(description = "How much of a Temporal Evolution one temporal read returns.")]
pub struct TemporalQueryConfig {
    /// Instances per attribute a temporal read without `lastN` returns before
    /// it is paged forwards (clause 6.3.10: "If no limits are specified, a
    /// default limit is enforced, depending on implementation specific
    /// configurations").
    #[schemars(description = "Instances per attribute a temporal read without `lastN` returns before it is paged forwards.")]
    pub default_last_n: u32,
    /// The largest `lastN` answered in full; a larger one is narrowed to this
    /// many instances per attribute and paged backwards with a 206 whose
    /// `Content-Range` still reports the requested `lastN` (clause 6.3.10). At
    /// most 5000, and not below `default_last_n`.
    ///
    /// It is also the page of an aggregated read (clause 4.5.19): at most this
    /// many whole periods per attribute, paged forwards with a 206 whose
    /// `Content-Range` ends at the last period served.
    #[schemars(
        description = "The largest `lastN` answered in one response. A larger `lastN` is narrowed to this many instances per attribute and paged backwards with a 206 whose `Content-Range` still reports the requested `lastN`. It is also the most whole periods per attribute on one page of an aggregated read."
    )]
    pub max_last_n: u32,
    /// The most instances of one attribute sharing one instant on the
    /// requested axis a temporal page carries.
    ///
    /// A page never splits the instances sharing the instant at its edge,
    /// since a client resumes from a `range-end` `DateTime` that cannot point
    /// inside an instant (clause 6.3.10), so it carries them whole however far
    /// past its size that takes it; one temporal write stamps every instance it
    /// records with one `createdAt` and `modifiedAt`. A larger group is
    /// refused with `TooManyResults` (clause 5.5.6). At most 1,000,000, and
    /// not below `max_last_n`.
    #[schemars(
        description = "The most instances of one attribute sharing one instant that a temporal page carries. A page never splits the instances sharing the instant at its edge, since a client resumes from a timestamp that cannot point inside an instant, so it carries them whole however far past its size that takes it. A larger group is refused with `403` `TooManyResults`."
    )]
    pub max_edge_group: u32,
    /// The raw instances one page of an aggregated temporal read folds per
    /// attribute at most, beside its `max_last_n` periods.
    ///
    /// A page that would fold more ends at the start of the period the first
    /// instance beyond the bound falls in; a first period holding more than
    /// the bound alone is read whole, since a period is never split. At most
    /// 100,000,000.
    #[schemars(
        description = "The most raw instances per attribute one page of an aggregated temporal read folds, beside its `temporal.query.max_last_n` periods. A page that would fold more ends at the start of the period the first instance beyond the bound falls in; a first period holding more than the bound is read whole, since a period is never split. Zero is refused."
    )]
    pub max_aggregation_instances: NonZeroU64,
}

impl Default for TemporalQueryConfig {
    fn default() -> TemporalQueryConfig {
        TemporalQueryConfig {
            default_last_n: 100,
            max_last_n: 1000,
            max_edge_group: 100_000,
            max_aggregation_instances: DEFAULT_MAX_AGGREGATION_INSTANCES,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::temporal::query::TemporalQueryConfig;
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU64;

    fn bound(n: u64) -> NonZeroU64 {
        NonZeroU64::new(n).expect("a positive bound")
    }

    #[test]
    fn the_defaults_are_documented() {
        let config = TemporalQueryConfig::default();
        assert_eq!(config.max_edge_group, 100_000);
        assert_eq!(config.max_aggregation_instances, bound(1_000_000));
    }

    /// A page always holds a whole period, so a bound of no instances
    /// describes no page at all and is refused as it is read.
    #[test]
    fn a_zero_aggregation_bound_is_refused_as_it_is_read() {
        let rendered = toml::to_string(&TemporalQueryConfig::default()).expect("the defaults render");
        let zero = rendered.replace("max_aggregation_instances = 1000000", "max_aggregation_instances = 0");
        assert!(toml::from_str::<TemporalQueryConfig>(&zero).is_err());
    }
}
