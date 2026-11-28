//! Whether a representation carries its system-generated attributes.

use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp};

/// Whether system-generated attributes (`createdAt`, `modifiedAt`) should be
/// included in the response or stripped before serialization.
///
/// A named policy rather than a bare `bool`: it is threaded through query
/// compilation, rendering and notification payload construction, and at each of
/// those a boolean would read as "some unexplained flag".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SysAttrsInclusion {
    /// Include `createdAt`, `modifiedAt`, and related system attributes.
    Include,
    /// Strip system attributes from the response.
    #[default]
    Strip,
}

impl SysAttrsInclusion {
    /// Whether a Temporal Evolution written in `format` over `timeproperty`
    /// carries the `createdAt` and `modifiedAt` of its instances under this
    /// choice.
    ///
    /// `sysAttrs` decides (clause 6.3.11), except that the clause 4.5.9
    /// `temporalValues` pairs need the instant on the Temporal Property the
    /// query ran on as their second element: over `createdAt` or `modifiedAt`
    /// that instant is a system timestamp, so the instances keep them. The
    /// projection that builds the pairs reads the value and that one instant
    /// alone, so the other timestamps kept alongside never reach the response.
    #[must_use]
    pub const fn in_temporal_evolution(self, format: TemporalFormat, timeproperty: TimeProp) -> SysAttrsInclusion {
        match (format, timeproperty) {
            (TemporalFormat::TemporalValues, TimeProp::Created | TimeProp::Modified) => SysAttrsInclusion::Include,
            (TemporalFormat::TemporalValues, TimeProp::Observed | TimeProp::Deleted)
            | (TemporalFormat::Normalized | TemporalFormat::AggregatedValues | TemporalFormat::GroupedValues, _) => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sys_attrs::SysAttrsInclusion;
    use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp};
    use strum::IntoEnumIterator;

    /// Clause 4.5.9: `temporalValues` over a system time pairs each value with
    /// that time, so the instances keep it whatever `sysAttrs` says.
    #[test]
    fn temporal_values_over_a_system_time_keep_the_system_times() {
        for timeproperty in [TimeProp::Created, TimeProp::Modified] {
            pretty_assertions::assert_eq!(
                SysAttrsInclusion::Strip.in_temporal_evolution(TemporalFormat::TemporalValues, timeproperty),
                SysAttrsInclusion::Include
            );
        }
    }

    /// Clause 6.3.11: every other representation follows `sysAttrs`.
    #[test]
    fn every_other_temporal_representation_follows_sys_attrs() {
        let instance_formats = [TemporalFormat::Normalized, TemporalFormat::AggregatedValues, TemporalFormat::GroupedValues];
        let followed = instance_formats
            .into_iter()
            .flat_map(|format| TimeProp::iter().map(move |timeproperty| (format, timeproperty)))
            .chain([
                (TemporalFormat::TemporalValues, TimeProp::Observed),
                (TemporalFormat::TemporalValues, TimeProp::Deleted),
            ]);
        for (format, timeproperty) in followed {
            for sys_attrs in [SysAttrsInclusion::Include, SysAttrsInclusion::Strip] {
                pretty_assertions::assert_eq!(sys_attrs.in_temporal_evolution(format, timeproperty), sys_attrs, "{format} over {timeproperty}");
            }
        }
    }
}
