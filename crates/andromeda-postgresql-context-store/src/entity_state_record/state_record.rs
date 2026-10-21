//! One Entity's state as the `entity_state_record` composite, and the record
//! of a committed state.

use crate::{current_attribute::flatten::flatten, entity_state_record::instance_record::AttributeInstanceRecord};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, scope::path::ScopePath, time::written_date_time::DateTimeText};
use andromeda_postgresql_attribute_instance::{flatten::FlattenError, name_keys::NameKeys};
use andromeda_postgresql_encoding::ewkb::EwkbError;
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use chrono::{DateTime, Utc};
use thiserror::Error;

/// The `entity_state_record` composite, field for field in declaration order.
#[derive(Debug, sqlx::Type)]
#[sqlx(type_name = "entity_state_record")]
pub struct EntityStateRecord {
    /// `entity_type_name_keys`, in written order.
    pub entity_type_name_keys: Vec<i32>,
    /// `scope_paths`, as written, empty when the Entity has no Scope.
    pub scope_paths: Vec<String>,
    /// `expires_at`.
    pub expires_at: Option<DateTime<Utc>>,
    /// `instances`, in ordinal order.
    pub instances: Vec<AttributeInstanceRecord>,
}

/// A committed state that has no record under the keys a commit resolved.
#[derive(Debug, Error)]
pub enum StateRecordError {
    /// An attribute does not flatten: a name has no key, or the instance is
    /// not one a write commits.
    #[error(transparent)]
    Flatten(#[from] FlattenError),
    /// An entity type has no `entity_type_names` key.
    #[error("the entity type {0} has no dictionary key")]
    UnresolvedType(EntityTypeName),
    /// A geometry has no EWKB.
    #[error(transparent)]
    Geometry(#[from] EwkbError),
}

/// The record of the committed `state`, every name held as its key in `keys`.
///
/// # Errors
///
/// Returns [`StateRecordError`] when a name has no key in `keys`, or when the
/// state is not one a write commits.
pub fn record_of(state: &EntityState, keys: &NameKeys) -> Result<EntityStateRecord, StateRecordError> {
    let entity_type_name_keys = state
        .types()
        .as_slice()
        .iter()
        .map(|name| {
            keys.entity_type(name)
                .map(EntityTypeNameKey::get)
                .ok_or_else(|| StateRecordError::UnresolvedType(name.clone())) // clone: the refusal names the type it could not key.
        })
        .collect::<Result<_, _>>()?;
    let scope_paths = state.scope().map_or_else(Vec::new, |scopes| {
        scopes.as_slice().iter().map(|path| ScopePath::as_str(path).to_owned()).collect()
    });
    let instances = flatten(state.attributes(), keys)?
        .into_iter()
        .map(AttributeInstanceRecord::try_from)
        .collect::<Result<_, _>>()?;
    Ok(EntityStateRecord {
        entity_type_name_keys,
        scope_paths,
        expires_at: state.expires_at().map(DateTimeText::instant),
        instances,
    })
}
