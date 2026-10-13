//! The persisted kind of one entry of a temporal append intent.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// Which history row one queued temporal entry becomes.
    ///
    /// An attribute observation and the end of an attribute instance become
    /// `attribute_history` rows, a Scope entry a `scope_history` row; the
    /// entry carries its kind because one intent holds all three, in the
    /// order the write recorded them.
    TemporalEntryKind {
        frozen_prefix: 3,
        frozen_digest: 0xcd48_7b0f_6088_ecd4,
        codes: {
            /// One attribute observation (clause 4.5.7).
            Observation = 1, "observation";
            /// The end of one attribute instance (clauses 4.5.7 and 4.5.8).
            Tombstone = 2, "tombstone";
            /// One Scope observation or the end of the Scope (clause 4.18).
            Scope = 3, "scope";
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
        assert!(TemporalEntryKind::try_from(0).is_err());
    }
}
