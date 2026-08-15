//! Everything one entity write must land together.
//!
//! Clauses 5.6.1 through 5.6.6 make one entity the atomic unit, while clauses
//! 5.6.7 through 5.6.10 permit per-entity partial success. A backend may fuse
//! several independent writes without changing that contract.

use crate::{
    batch_side_effect_job::BatchSideEffectJob,
    entity_mutation::EntityMutation,
    history_record::HistoryRecord,
    precondition::Precondition,
    subscription_match::SubscriptionMatch,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// One entity mutation and every consequence that must share its commit.
#[derive(Debug, TypedBuilder)]
pub struct AtomicWrite {
    /// Entity this write concerns.
    pub entity_id: Arc<Uri>,
    /// Mutation of the current entity row.
    pub entity: EntityMutation,
    /// Temporal Evolution records produced by the mutation.
    #[builder(default = Default::default())]
    pub history: Vec<HistoryRecord>,
    /// Subscriptions the mutation matched, each owed a notification event.
    #[builder(default = Default::default())]
    pub subscription_matches: Vec<SubscriptionMatch>,
    /// Durable job used by the opt-in asynchronous batch path.
    #[builder(default = Default::default())]
    pub side_effects: Option<BatchSideEffectJob>,
    /// Assumption the mutation makes about current entity state.
    #[builder(default = Precondition::None)]
    pub precondition: Precondition,
}

#[cfg(test)]
mod tests {
    use crate::{atomic_write::AtomicWrite, entity_mutation::EntityMutation, precondition::Precondition};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use std::sync::Arc;

    #[test]
    fn the_builder_defaults_every_optional_consequence() {
        let write = AtomicWrite::builder()
            .entity_id(Arc::new("urn:ngsi-ld:A".parse::<Uri>().expect("valid entity URI")))
            .entity(EntityMutation::None)
            .build();
        assert!(write.history.is_empty());
        assert!(write.subscription_matches.is_empty());
        assert!(write.side_effects.is_none());
        pretty_assertions::assert_eq!(write.precondition, Precondition::None);
    }
}
