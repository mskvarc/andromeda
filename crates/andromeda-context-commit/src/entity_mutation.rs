//! Entity-row mutations carried by atomic context writes.

use crate::entity_record::EntityRecord;
use derive_more::IsVariant;

/// Change made to the current entity row.
#[derive(Debug, IsVariant)]
pub enum EntityMutation {
    /// Insert a row that is not present.
    Insert(EntityRecord),
    /// Replace an existing row.
    Update(EntityRecord),
    /// Delete an existing row.
    Delete,
    /// Leave current state unchanged while committing other consequences.
    None,
}

impl EntityMutation {
    /// Return the entity state written by an insert or update.
    #[must_use]
    pub const fn record(&self) -> Option<&EntityRecord> {
        match self {
            EntityMutation::Insert(record) | EntityMutation::Update(record) => Some(record),
            EntityMutation::Delete | EntityMutation::None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity_mutation::EntityMutation, entity_record::EntityRecord};
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes};
    use std::sync::Arc;

    #[test]
    fn only_insert_and_update_mutations_carry_entity_records() {
        let state = EntityState::new(
            "urn:ngsi-ld:A".parse().expect("valid entity URI"),
            EntityTypes::one("https://example.org/T".parse().expect("valid type name")),
        );
        assert!(
            EntityMutation::Insert(EntityRecord::from_state(Arc::new(state)).expect("a projection"))
                .record()
                .is_some()
        );
        assert!(EntityMutation::Delete.record().is_none());
        assert!(EntityMutation::None.record().is_none());
    }
}
