//! The entities of a relayed copy, each beside the instances its source
//! reported it held before the change (clause 5.8.6, Table 5.2.14-1
//! `showChanges`).
//!
//! The previous values are the source's: it computed them from its own history
//! under the copy it holds, which carries the local subscription's
//! `showChanges`, and this broker cannot compute them for an entity it does
//! not hold. They are kept apart from the entity, as a notification of a local
//! change keeps the state before the change apart from the state after it, so
//! the local subscription's filters and projection read the entity alone.

use andromeda_ngsi_ld_model::entity::Entity;

/// The entities of one relayed copy, in notified order.
#[derive(Debug, Default)]
pub(crate) struct RelayedEntities {
    /// The entities, re-assembled for the local subscription.
    entities: Vec<Entity>,
    /// For each entity, at the same position, the instances its source
    /// reported it held before the change; `None` when it reported none or
    /// the local subscription does not ask for them.
    previous: Vec<Option<Entity>>,
}

impl RelayedEntities {
    /// Room for `capacity` entities.
    pub(crate) fn with_capacity(capacity: usize) -> RelayedEntities {
        RelayedEntities {
            entities: Vec::with_capacity(capacity),
            previous: Vec::with_capacity(capacity),
        }
    }

    /// Append `entity`, with the instances it held before the change.
    pub(crate) fn push(&mut self, entity: Entity, previous: Option<Entity>) {
        self.entities.push(entity);
        self.previous.push(previous);
    }

    /// The entities alone, as the Linked Entity traversal reads its roots.
    pub(crate) fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// Each entity with the instances it held before the change.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&Entity, Option<&Entity>)> {
        self.entities.iter().zip(self.previous.iter().map(Option::as_ref))
    }

    /// Whether the copy carries no entity.
    pub(crate) const fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}
