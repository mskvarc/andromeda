//! Which enclosing statement a compiled `q` predicate is attached to.

/// Which enclosing query a compiled `q` predicate is being attached to.
///
/// Determines the column an `attribute_current` probe correlates against and
/// the payload column the jsonpath fallback reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QFilterTarget {
    /// Entity queries, which select directly from `entities`.
    Entities,
    /// Temporal stage 1, which drives from the TEA catalogue and left-joins
    /// `entities` under the alias `e`.
    TemporalStage1,
}

impl QFilterTarget {
    /// SQL reference to the entity id an `EXISTS` probe correlates against.
    pub const fn entity_id_column(self) -> &'static str {
        match self {
            Self::Entities => "\"entities\".\"id\"",
            Self::TemporalStage1 => "\"tea\".\"entity_id\"",
        }
    }

    /// SQL reference to the JSONB payload column used by the fallback.
    pub const fn payload_column(self) -> &'static str {
        match self {
            Self::Entities => "\"payload\"",
            Self::TemporalStage1 => "\"e\".\"payload\"",
        }
    }
}
