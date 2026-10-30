//! An Entity's state as storage holds it, every name still a dictionary key,
//! and its assembly once the names of those keys are looked up.
//!
//! An Entity read assembles the head and `current_attributes` rows it read
//! through it, and a queue read the `entity_state_record` it stored, so a
//! state reads back the same way from either.

use crate::{
    current_attribute::{assemble::assemble, row::CurrentAttributeRow},
    entity::stored_entity_error::StoredEntityError,
    entity_state_record::state_record::EntityStateRecord,
};
use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes, scopes::Scopes};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier, scope::path::ScopePath, time::date_time::NgsiLdDateTime};
use andromeda_postgresql_attribute_instance::name_keys::{HeldKeys, KeyNames};
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use chrono::{DateTime, Utc};

/// One Entity's state beside its identifier, its names held as keys.
#[derive(Debug)]
pub struct StoredState {
    /// The type keys, in written order.
    pub types: Vec<EntityTypeNameKey>,
    /// The Scopes as written (clause 4.18), `None` when the Entity has none.
    pub scope: Option<Scopes>,
    /// The Entity's clause 4.22 `expiresAt`.
    pub expires_at: Option<DateTime<Utc>>,
    /// The top-level attribute instances, in no order.
    pub rows: Vec<CurrentAttributeRow>,
}

impl TryFrom<EntityStateRecord> for StoredState {
    type Error = StoredEntityError;

    fn try_from(record: EntityStateRecord) -> Result<StoredState, StoredEntityError> {
        let paths = record
            .scope_paths
            .into_iter()
            .map(ScopePath::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoredEntityError::Scope)?;
        Ok(StoredState {
            types: record.entity_type_name_keys.into_iter().map(EntityTypeNameKey::from).collect(),
            scope: Scopes::try_from_paths(paths).ok(),
            expires_at: record.expires_at,
            rows: record
                .instances
                .into_iter()
                .map(CurrentAttributeRow::try_from)
                .collect::<Result<_, _>>()
                .map_err(|error| StoredEntityError::Row(Box::new(error)))?,
        })
    }
}

impl StoredState {
    /// Add every dictionary key the state holds to `held`.
    pub fn collect_keys(&self, held: &mut HeldKeys) {
        for row in &self.rows {
            held.collect(&row.members, &row.sub_attributes);
        }
        held.types.extend(self.types.iter().copied());
    }

    /// The state of the Entity `id`, every key read back through `names`.
    ///
    /// # Errors
    ///
    /// Returns [`StoredEntityError`] when a type key has no name, when the
    /// state carries no type, or when its rows do not assemble.
    pub fn assemble(self, id: &Identifier, names: &KeyNames) -> Result<EntityState, StoredEntityError> {
        let attributes = assemble(self.rows, names).map_err(|error| StoredEntityError::Attributes(Box::new(error)))?;
        let type_names = self
            .types
            .iter()
            .map(|key| names.type_name(*key).map(EntityTypeName::from).ok_or(StoredEntityError::TypeKey(*key)))
            .collect::<Result<Vec<_>, _>>()?;
        let entity_types = EntityTypes::try_from_names(type_names).map_err(|_| StoredEntityError::Untyped)?;
        let expires_at = self
            .expires_at
            .map(NgsiLdDateTime::normalise)
            .transpose()
            .map_err(StoredEntityError::ExpiresAt)?;
        // clone: the state owns its identifier, which a caller keeps to name
        // the Entity when its state does not assemble.
        Ok(EntityState::from_parts(id.clone(), entity_types, self.scope, expires_at, attributes))
    }
}
