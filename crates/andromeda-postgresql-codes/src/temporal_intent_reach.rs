//! The persisted needs of the statements of an intent application that one
//! queued temporal intent meets.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// What a statement of an intent application needs among the intents it
    /// applies for it to change anything.
    ///
    /// A queued intent stores the needs its entries meet, so an application
    /// learns which statements its claimed intents reach from that column
    /// alone and skips the rest without reading any entry.
    TemporalIntentReach {
        frozen_prefix: 5,
        frozen_digest: 0x2d53_da8e_35be_853f,
        codes: {
            /// The intent states types, which bring a Temporal Evolution into
            /// being (clause 4.5.6).
            TypedIntent = 1, "typed-intent";
            /// The intent states no type and holds no entry or one that does
            /// not end an instance, so it needs an evolution that exists
            /// (clause 4.5.1).
            UntypedObservation = 2, "untyped-observation";
            /// An attribute observation or the end of an attribute instance.
            AttributeEntry = 3, "attribute-entry";
            /// An attribute entry carrying a client-supplied `instanceId`
            /// (clause 4.5.7).
            VerbatimInstance = 4, "verbatim-instance";
            /// A Scope entry (clause 4.18).
            ScopeEntry = 5, "scope-entry";
            /// The intent states no type and holds an attribute entry, whose
            /// stream only an evolution that exists can carry.
            UntypedAttributeEntry = 6, "untyped-attribute-entry";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal_intent_reach::TemporalIntentReach;
    use pretty_assertions::assert_eq;

    #[test]
    fn every_reach_keeps_the_code_the_queue_stores() {
        let codes = TemporalIntentReach::ALL.iter().map(|reach| reach.code()).collect::<Vec<_>>();
        assert_eq!(codes, [1, 2, 3, 4, 5, 6]);
        assert!(TemporalIntentReach::try_from(0).is_err());
    }
}
