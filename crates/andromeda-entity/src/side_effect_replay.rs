//! The writes a drained side-effect job replays.

use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    entity_mutation::EntityMutation,
    history_record::HistoryRecord,
    subscription_match::SubscriptionMatch,
};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::{collections::HashMap, sync::Arc};

/// The writes one job replays: one per entity, carrying the observations still
/// owed to its Temporal Evolution and the durable matches its owed changes are
/// due.
///
/// A commit names each entity once, so an owed match joins its entity's
/// observation write, and an entity with no observation to record, a deleted
/// one, gets a write of its own. Neither changes the entity row. Observation
/// writes keep their stored order and the matches keep theirs, which is what
/// numbers the replay's rows the same way on every attempt.
pub(crate) fn replay_writes(temporal_entities: Vec<(Arc<Identifier>, Arc<EntityState>)>, owed: Vec<SubscriptionMatch>) -> Vec<AtomicWrite> {
    let mut writes: Vec<AtomicWrite> = temporal_entities
        .into_iter()
        .map(|(identifier, observed)| {
            AtomicWrite::builder()
                .entity_id(identifier)
                .entity(EntityMutation::None)
                .history(vec![HistoryRecord::Observations(observed)])
                .build()
        })
        .collect();
    let mut positions: HashMap<Arc<Identifier>, usize> = writes
        .iter()
        .enumerate()
        .map(|(position, write)| (Arc::clone(&write.entity_id), position)) // clone: Arc ref-count bump
        .collect();
    for matched in owed {
        if let Some(write) = positions.get(&matched.entity_id).and_then(|position| writes.get_mut(*position)) {
            write.subscription_matches.push(matched);
        } else {
            let identifier = Arc::new(matched.entity_id.clone()); // clone: the write owns the identifier the match also keeps.
            positions.insert(Arc::clone(&identifier), writes.len()); // clone: Arc ref-count bump
            writes.push(
                AtomicWrite::builder()
                    .entity_id(identifier)
                    .entity(EntityMutation::None)
                    .subscription_matches(vec![matched])
                    .build(),
            );
        }
    }
    writes
}

#[cfg(test)]
mod tests {
    use crate::{building_subscription_fixture::subscription_key, side_effect_replay::replay_writes};
    use andromeda_context_commit::{history_record::HistoryRecord, subscription_match::SubscriptionMatch};
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};
    use std::sync::Arc;

    fn uri(raw: &str) -> Identifier {
        raw.parse().expect("a valid URI")
    }

    fn matched(id: &str) -> SubscriptionMatch {
        SubscriptionMatch::builder()
            .subscription_key(subscription_key())
            .entity_id(uri(id))
            .change(EntityChange::new(EntityLifecycle::Deleted, Vec::new()))
            .build()
    }

    /// A deleted entity has no observation to replay, so its owed match gets a
    /// write of its own that leaves the entity row alone.
    #[test]
    fn an_owed_match_without_an_observation_write_gets_its_own_write() {
        let observed = "urn:ngsi-ld:Building:observed";
        let deleted = "urn:ngsi-ld:Building:deleted";
        let writes = replay_writes(
            vec![(
                Arc::new(uri(observed)),
                Arc::new(EntityState::new(
                    uri(observed),
                    EntityTypes::one("https://example.org/Building".parse().expect("a valid type")),
                )),
            )],
            vec![matched(deleted), matched(observed), matched(deleted)],
        );

        let shape: Vec<(String, usize, usize)> = writes
            .iter()
            .map(|write| (write.entity_id.as_str().to_owned(), write.history.len(), write.subscription_matches.len()))
            .collect();
        pretty_assertions::assert_eq!(shape, vec![(observed.to_owned(), 1, 1), (deleted.to_owned(), 0, 2)]);
        assert!(
            writes.iter().all(|write| write.entity.record().is_none() && !write.entity.is_insert()),
            "a replay never changes the entity row"
        );
        assert!(matches!(writes[0].history[0], HistoryRecord::Observations(_)));
    }
}
