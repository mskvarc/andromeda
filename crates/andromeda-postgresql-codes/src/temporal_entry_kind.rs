//! The persisted kind of one entry of a temporal append intent.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// Which history row one queued temporal entry becomes.
    ///
    /// An attribute observation and the end of an attribute instance become
    /// `attribute_history` rows, a Scope event a `scope_history` row, and a
    /// Scope assertion a `scope_history` row only where the Scope in force
    /// differs; the entry carries its kind because one intent holds them all,
    /// in the order the write recorded them.
    TemporalEntryKind {
        frozen_prefix: 3,
        frozen_digest: 0xcd48_7b0f_6088_ecd4,
        codes: {
            /// One attribute instance a write adds to the Temporal
            /// Evolution (clause 4.5.7): an observation, or a deletion a
            /// written Temporal Evolution states.
            Observation = 1, "observation";
            /// The end of one attribute instance a write removed from the
            /// current state (clauses 4.5.7 and 4.5.8).
            Tombstone = 2, "tombstone";
            /// One Scope event: a Scope set, or the end of the Scope
            /// (clause 4.18).
            Scope = 3, "scope";
            /// One path of a Scope a write stated the Entity is in, recorded
            /// only where the Scope in force at the write differs (clause
            /// 4.18).
            ScopeAssertion = 4, "scope-assertion";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal_entry_kind::TemporalEntryKind;
    use pretty_assertions::assert_eq;

    #[test]
    fn every_entry_kind_keeps_the_code_the_queue_stores() {
        assert_eq!(TemporalEntryKind::Observation.code(), 1);
        assert_eq!(TemporalEntryKind::Tombstone.code(), 2);
        assert_eq!(TemporalEntryKind::Scope.code(), 3);
        assert_eq!(TemporalEntryKind::ScopeAssertion.code(), 4);
        assert!(TemporalEntryKind::try_from(0).is_err());
    }
}
