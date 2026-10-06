//! Which NGSI-LD Element the clause 4.3.6.8 fallbacks are being applied to.

use andromeda_ngsi_ld_lexicon::temporal_format::TemporalFormat;

/// Which NGSI-LD Element the clause 4.3.6.8 fallbacks are being applied to.
///
/// Tables 4.3.6.8-1 to -3 are written for the Entity data type, and two of
/// their rows read differently once the payload is the Temporal Evolution of an
/// Entity rather than an Entity, because clause 4.5.6 keeps the Entity's own
/// members but replaces every Attribute with its temporal representation:
///
/// - Notes 2 and 3 of Table 4.3.6.8-1 collapse an Attribute's JSON array to its
///   default instance below 1.3, because in an Entity that array is the clause
///   4.5.5 set of instances separated by `datasetId`. In a temporal
///   representation it is the clause 4.5.7 sequence of instances in time, which
///   those notes do not speak of and which a collapse would reduce to a single
///   point.
/// - Table 4.3.6.8-2 removes `observedAt` below 1.3. Clause 4.5.7 makes that
///   member the Temporal Property recording the point in time an instance
///   stands at, so removing it leaves a payload that is not a temporal
///   representation at all rather than an older one.
///
/// Every other fallback is the same on both: the Entity's own members, the
/// family reformats, and the remaining per-instance removals.
///
/// The simplified temporal representation of clause 4.5.9 moves the sequence of
/// instances in time inside a plural member, which puts both rows back where
/// Table 4.3.6.8-1 wrote them. Clause 4.5.19's aggregated representation shares
/// that shape, so both are written by the temporal writers
/// (`crate::temporal`) under the `SimplifiedTemporalEvolution` reading.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AdaptedPayload {
    /// An Entity, as clause 4.5.1 represents it.
    #[default]
    Entity,
    /// The Temporal Evolution of an Entity, as clause 4.5.6 represents it.
    TemporalEvolution,
    /// The Temporal Evolution of an Entity in the simplified representation of
    /// clause 4.5.9, or the aggregated one of clause 4.5.19.
    SimplifiedTemporalEvolution,
}

impl AdaptedPayload {
    /// Whether an Attribute's JSON array is the clause 4.5.5 set of instances
    /// separated by `datasetId`, which notes 2 and 3 of Table 4.3.6.8-1 collapse
    /// to the default instance below 1.3.
    #[must_use]
    pub const fn collapses_instances(self) -> bool {
        matches!(self, AdaptedPayload::Entity | AdaptedPayload::SimplifiedTemporalEvolution)
    }

    /// Whether `observedAt` is an ordinary Table 4.3.6.8-2 member that the
    /// fallback removes below 1.3, rather than the clause 4.5.7 Temporal
    /// Property that indexes a temporal instance.
    #[must_use]
    pub const fn removes_observed_at(self) -> bool {
        matches!(self, AdaptedPayload::Entity)
    }

    /// Which reading the Temporal Evolution of an Entity gets, given the clause
    /// 6.3.12 representation the temporal answer was rendered in.
    #[must_use]
    pub const fn of_temporal(format: TemporalFormat) -> AdaptedPayload {
        match format {
            TemporalFormat::Normalized => AdaptedPayload::TemporalEvolution,
            TemporalFormat::TemporalValues | TemporalFormat::AggregatedValues | TemporalFormat::GroupedValues => AdaptedPayload::SimplifiedTemporalEvolution,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::version::adapted_payload::AdaptedPayload;
    use andromeda_ngsi_ld_lexicon::temporal_format::TemporalFormat;
    use pretty_assertions::assert_eq;

    #[test]
    fn an_entity_payload_reads_both_rows_as_the_tables_write_them() {
        assert!(AdaptedPayload::Entity.collapses_instances());
        assert!(AdaptedPayload::Entity.removes_observed_at());
    }

    /// Clause 4.5.7: the array is the sequence of instances in time and
    /// `observedAt` is what places each one in it, so neither row reaches a
    /// temporal payload.
    #[test]
    fn a_temporal_payload_keeps_its_sequence_and_its_temporal_index() {
        assert!(!AdaptedPayload::TemporalEvolution.collapses_instances());
        assert!(!AdaptedPayload::TemporalEvolution.removes_observed_at());
    }

    #[test]
    fn the_default_payload_is_an_entity() {
        assert_eq!(AdaptedPayload::default(), AdaptedPayload::Entity);
    }

    /// Clause 4.5.9 puts the sequence in time inside a plural member, so the
    /// outer array is the `datasetId`-separated set notes 2 and 3 do collapse.
    #[test]
    fn a_simplified_temporal_payload_collapses_its_dataset_set() {
        assert!(AdaptedPayload::SimplifiedTemporalEvolution.collapses_instances());
    }

    #[test]
    fn the_clause_6_3_12_representation_decides_which_temporal_reading_applies() {
        assert_eq!(AdaptedPayload::of_temporal(TemporalFormat::Normalized), AdaptedPayload::TemporalEvolution);
        assert_eq!(
            AdaptedPayload::of_temporal(TemporalFormat::TemporalValues),
            AdaptedPayload::SimplifiedTemporalEvolution
        );
        assert_eq!(
            AdaptedPayload::of_temporal(TemporalFormat::AggregatedValues),
            AdaptedPayload::SimplifiedTemporalEvolution
        );
    }
}
