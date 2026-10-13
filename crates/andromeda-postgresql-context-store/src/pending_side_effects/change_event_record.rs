//! One change a respond-async job owes, as the `pending_change_event`
//! composite the job stores it in, and back.
//!
//! The record stores what a `notification_events` row stores, under the same
//! codes, and each state as an `entity_state_record`: a deletion's removed
//! state once, as `entity_snapshot`, which reads back as both the removed and
//! the previous state; a change that leaves the Entity in place its resulting
//! state as `entity_snapshot` and the state before it as `previous_snapshot`.
//! Every name is a dictionary key: the commit resolves the names of all its
//! events at once, and a claim looks up the names of all the keys one job
//! holds at once.

use crate::{
    entity::stored_entity_error::StoredEntityError,
    entity_state_record::{
        state_record::{EntityStateRecord, StateRecordError, record_of},
        stored_state::StoredState,
    },
    notification_delivery::{
        change_code::{stored_change_kind, stored_lifecycle},
        stored_change::{StoredChangeError, decode_change, decode_system_attributes},
    },
    pending_side_effects::origin_code::{origin_of, stored_origin},
};
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    identifier::{Identifier, IdentifierParseError},
};
use andromeda_ngsi_ld_model::{
    entity_change::EntityLifecycle,
    notification::{ChangeOrigin, SysAttrsSnapshot},
};
use andromeda_postgresql_attribute_instance::name_keys::{HeldKeys, KeyNames, MentionedNames, NameKeys};
use andromeda_postgresql_codes::{change_origin::ChangeOriginCode, registry::UnknownCompactCode};
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use thiserror::Error;

/// The `pending_change_event` composite, field for field in declaration order.
#[derive(Debug, sqlx::Type)]
#[sqlx(type_name = "pending_change_event")]
pub struct PendingChangeEvent {
    /// `lifecycle`.
    pub lifecycle: i16,
    /// `entity_id`.
    pub entity_id: String,
    /// `entity_type_name_keys`, in written order.
    pub entity_type_name_keys: Vec<i32>,
    /// `change_kinds`.
    pub change_kinds: Vec<i16>,
    /// `change_attribute_name_keys`, position for position with the kinds.
    pub change_attribute_name_keys: Vec<i32>,
    /// `change_dataset_ids`, a NULL element for a default instance.
    pub change_dataset_ids: Vec<Option<String>>,
    /// `occurred_at`.
    pub occurred_at: DateTime<Utc>,
    /// `origin`.
    pub origin: i16,
    /// `entity_snapshot`: the state after the change, or the state a deletion
    /// removed.
    pub entity_snapshot: Option<EntityStateRecord>,
    /// `previous_snapshot`: the state before a change that left the Entity in
    /// place.
    pub previous_snapshot: Option<EntityStateRecord>,
    /// `entity_created_at`.
    pub entity_created_at: Option<DateTime<Utc>>,
    /// `entity_modified_at`.
    pub entity_modified_at: Option<DateTime<Utc>>,
    /// `entity_expires_at`.
    pub entity_expires_at: Option<DateTime<Utc>>,
}

/// A change that has no record under the keys its commit resolved.
#[derive(Debug, Error)]
pub enum ChangeEventRecordError {
    /// A state has no `entity_state_record`.
    #[error(transparent)]
    State(#[from] StateRecordError),
    /// An Entity type of the change has no `entity_type_names` key.
    #[error("the entity type {0} has no dictionary key")]
    UnresolvedType(EntityTypeName),
    /// A changed attribute has no `attribute_names` key.
    #[error("the attribute {0} has no dictionary key")]
    UnresolvedAttribute(AttributeName),
}

/// Why a stored change does not read back as the change it was queued as.
#[derive(Debug, Error)]
pub enum PendingChangeEventError {
    /// The entity identifier is not a URI permitted by clause 4.6.2.
    #[error("references an invalid entity URI")]
    InvalidEntityId {
        /// URI validation failure.
        #[source]
        source: IdentifierParseError,
    },
    /// The stored number names no origin.
    #[error("carries an unreadable origin")]
    UnknownOrigin {
        /// Registry rejection naming the number.
        #[source]
        source: UnknownCompactCode,
    },
    /// The change itself does not read back.
    #[error(transparent)]
    Change(#[from] StoredChangeError),
    /// A stored state no longer reads back as an entity state.
    #[error("carries an unreadable entity snapshot")]
    UnreadableSnapshot {
        /// Why the stored record is not an entity state.
        #[source]
        source: StoredEntityError,
    },
    /// A type key no dictionary row names.
    #[error("holds the entity type key {}, which names no type", .0.get())]
    TypeKey(EntityTypeNameKey),
    /// A changed attribute's key no dictionary row names.
    #[error("holds the attribute key {}, which names no attribute", .0.get())]
    AttributeKey(AttributeNameKey),
}

/// The states `event` stores: its resulting state, or the state a deletion
/// removed, and the state before a change that left the Entity in place.
fn stored_states(event: &ChangeEvent) -> (Option<&EntityState>, Option<&EntityState>) {
    match event.change.lifecycle {
        EntityLifecycle::Deleted => (event.entity_snapshot.as_deref().or(event.previous_attributes.as_deref()), None),
        EntityLifecycle::Created | EntityLifecycle::Modified => (event.current_payload.as_deref(), event.previous_attributes.as_deref()),
    }
}

/// Add every name `event`'s record holds as a key to `mentioned`: its types,
/// its changed attributes and every name its stored states hold.
pub fn mention_event(event: &ChangeEvent, mentioned: &mut MentionedNames) {
    // clone: the dictionary request owns the names it resolves.
    mentioned.types.extend(event.entity_types.iter().cloned());
    mentioned
        .attributes
        .extend(event.change.instances.iter().map(|instance| instance.key.attr.clone())); // clone: the dictionary request owns the names it resolves.
    let (state, previous) = stored_states(event);
    for state in state.into_iter().chain(previous) {
        mentioned.collect(state.attributes());
        mentioned.types.extend(state.types().as_slice().iter().cloned()); // clone: the dictionary request owns the names it resolves.
    }
}

/// The record `event` is stored as, every name held as its key in `keys`.
///
/// # Errors
///
/// Returns [`ChangeEventRecordError`] when a name has no key in `keys`, or a
/// state is not one a write commits.
pub fn record_of_event(event: &ChangeEvent, keys: &NameKeys) -> Result<PendingChangeEvent, ChangeEventRecordError> {
    let (state, previous) = stored_states(event);
    let record = |state: Option<&EntityState>| state.map(|state| record_of(state, keys)).transpose();
    Ok(PendingChangeEvent {
        lifecycle: stored_lifecycle(event.change.lifecycle).code(),
        entity_id: event.entity_id.as_str().to_owned(),
        entity_type_name_keys: event
            .entity_types
            .iter()
            .map(|name| {
                keys.entity_type(name)
                    .map(EntityTypeNameKey::get)
                    .ok_or_else(|| ChangeEventRecordError::UnresolvedType(name.clone())) // clone: the refusal names the type it could not key.
            })
            .collect::<Result<_, _>>()?,
        change_kinds: event.change.instances.iter().map(|instance| stored_change_kind(instance.kind).code()).collect(),
        change_attribute_name_keys: event
            .change
            .instances
            .iter()
            .map(|instance| {
                keys.attribute(&instance.key.attr)
                    .map(AttributeNameKey::get)
                    .ok_or_else(|| ChangeEventRecordError::UnresolvedAttribute(instance.key.attr.clone())) // clone: the refusal names the attribute it could not key.
            })
            .collect::<Result<_, _>>()?,
        change_dataset_ids: event
            .change
            .instances
            .iter()
            .map(|instance| instance.key.dataset_id.as_ref().map(|dataset| dataset.as_str().to_owned()))
            .collect(),
        occurred_at: event.occurred_at,
        origin: stored_origin(event.origin).code(),
        entity_snapshot: record(state)?,
        previous_snapshot: record(previous)?,
        entity_created_at: event.sys_attrs_snapshot.as_ref().map(|sys_attrs| sys_attrs.created_at),
        entity_modified_at: event.sys_attrs_snapshot.as_ref().map(|sys_attrs| sys_attrs.modified_at),
        entity_expires_at: event.sys_attrs_snapshot.as_ref().and_then(|sys_attrs| sys_attrs.expires_at),
    })
}

/// One stored change, every stored value validated but the names behind its
/// keys.
#[derive(Debug)]
pub struct ReadChangeEvent {
    /// The Entity changed.
    entity_id: Identifier,
    /// The Entity's type keys, in written order.
    types: Vec<EntityTypeNameKey>,
    /// The stored lifecycle code.
    lifecycle: i16,
    /// The stored transition code of each changed instance.
    change_kinds: Vec<i16>,
    /// The attribute key of each changed instance.
    change_attributes: Vec<AttributeNameKey>,
    /// The `datasetId` of each changed instance.
    change_dataset_ids: Vec<Option<String>>,
    /// Instant the change occurred.
    occurred_at: DateTime<Utc>,
    /// Where the change came from.
    origin: ChangeOrigin,
    /// The Entity's resulting state, or the state a deletion removed.
    entity_state: Option<StoredState>,
    /// The Entity's state before a change that left it in place.
    previous_state: Option<StoredState>,
    /// The clause 4.8 system attributes of an Entity a deletion removed.
    sys_attrs_snapshot: Option<SysAttrsSnapshot>,
}

impl TryFrom<PendingChangeEvent> for ReadChangeEvent {
    type Error = PendingChangeEventError;

    fn try_from(record: PendingChangeEvent) -> Result<ReadChangeEvent, PendingChangeEventError> {
        let state = |record: Option<EntityStateRecord>| {
            record
                .map(StoredState::try_from)
                .transpose()
                .map_err(|source| PendingChangeEventError::UnreadableSnapshot { source })
        };
        Ok(ReadChangeEvent {
            entity_id: record.entity_id.parse().map_err(|source| PendingChangeEventError::InvalidEntityId { source })?,
            types: record.entity_type_name_keys.into_iter().map(EntityTypeNameKey::from).collect(),
            lifecycle: record.lifecycle,
            change_kinds: record.change_kinds,
            change_attributes: record.change_attribute_name_keys.into_iter().map(AttributeNameKey::from).collect(),
            change_dataset_ids: record.change_dataset_ids,
            occurred_at: record.occurred_at,
            origin: ChangeOriginCode::try_from(record.origin)
                .map(origin_of)
                .map_err(|source| PendingChangeEventError::UnknownOrigin { source })?,
            entity_state: state(record.entity_snapshot)?,
            previous_state: state(record.previous_snapshot)?,
            sys_attrs_snapshot: decode_system_attributes(record.entity_created_at, record.entity_modified_at, record.entity_expires_at)?,
        })
    }
}

impl ReadChangeEvent {
    /// Add every dictionary key the change holds to `held`.
    pub fn collect_keys(&self, held: &mut HeldKeys) {
        held.types.extend(self.types.iter().copied());
        held.attributes.extend(self.change_attributes.iter().copied());
        for state in self.entity_state.iter().chain(&self.previous_state) {
            state.collect_keys(held);
        }
    }

    /// The change, every key read back through `names`.
    ///
    /// A deletion's one stored state is both its removed and its previous
    /// state, one allocation; a change that leaves the Entity in place reads
    /// its resulting state back as the state the notification renders.
    ///
    /// # Errors
    ///
    /// Returns [`PendingChangeEventError`] when a key has no name, the change
    /// does not read back or a state does not assemble.
    pub fn assemble(self, names: &KeyNames) -> Result<ChangeEvent, PendingChangeEventError> {
        let entity_types = self
            .types
            .iter()
            .map(|key| names.type_name(*key).map(EntityTypeName::from).ok_or(PendingChangeEventError::TypeKey(*key)))
            .collect::<Result<Vec<_>, _>>()?;
        let attributes = self
            .change_attributes
            .iter()
            .map(|key| names.attribute(*key).cloned().ok_or(PendingChangeEventError::AttributeKey(*key))) // clone: the change owns the names it reports.
            .collect::<Result<Vec<_>, _>>()?;
        let change = decode_change(self.lifecycle, self.change_kinds, attributes, self.change_dataset_ids)?;
        let state = |stored: Option<StoredState>| {
            stored
                // clone: every state of the change owns the identifier it is assembled under.
                .map(|stored| stored.assemble(self.entity_id.clone(), names).map(Arc::new))
                .transpose()
                .map_err(|source| PendingChangeEventError::UnreadableSnapshot { source })
        };
        let resulting = state(self.entity_state)?;
        let (entity_snapshot, previous_attributes, current_payload) = match change.lifecycle {
            EntityLifecycle::Deleted => (resulting.as_ref().map(Arc::clone), resulting, None),
            EntityLifecycle::Created | EntityLifecycle::Modified => (None, state(self.previous_state)?, resulting),
        };
        Ok(ChangeEvent::builder()
            .entity_id(self.entity_id)
            .entity_types(entity_types)
            .change(change)
            .occurred_at(self.occurred_at)
            .entity_snapshot(entity_snapshot)
            .previous_attributes(previous_attributes)
            .sys_attrs_snapshot(self.sys_attrs_snapshot)
            .origin(self.origin)
            .current_payload(current_payload)
            .build())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        current_attribute::round_trip_tests::{dictionary_of, ex},
        notification_delivery::{claimed_event::tests::temperature, stored_change::StoredChangeError},
        pending_side_effects::change_event_record::{PendingChangeEvent, PendingChangeEventError, ReadChangeEvent, record_of_event},
    };
    use andromeda_context_commit::change_event::ChangeEvent;
    use andromeda_ngsi_ld_entity::{
        entity_state::EntityState,
        reads::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            attribute_instance_key::AttributeInstanceKey,
        },
    };
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::{ChangeOrigin, SysAttrsSnapshot},
    };
    use andromeda_postgresql_attribute_instance::name_keys::KeyNames;
    use chrono::{TimeZone, Utc};
    use std::sync::Arc;

    const LIFECYCLES: [EntityLifecycle; 3] = [EntityLifecycle::Created, EntityLifecycle::Modified, EntityLifecycle::Deleted];
    const ORIGINS: [ChangeOrigin; 2] = [ChangeOrigin::Local, ChangeOrigin::Forwarded];

    /// A change of the Building through `lifecycle` that created, updated and
    /// deleted three instances of `temperature`, one of them of a dataset.
    fn event(lifecycle: EntityLifecycle, origin: ChangeOrigin, current: &Arc<EntityState>, previous: &Arc<EntityState>) -> ChangeEvent {
        let instance = |dataset: Option<&str>, kind: AttributeChangeKind| AttributeChange {
            key: AttributeInstanceKey {
                attr: ex("temperature").parse().expect("a valid attribute name"),
                dataset_id: dataset.map(|raw| raw.parse().expect("a valid dataset URI")),
            },
            kind,
        };
        let change = EntityChange::new(
            lifecycle,
            vec![
                instance(None, AttributeChangeKind::Created),
                instance(Some("urn:ngsi-ld:Dataset:a"), AttributeChangeKind::Updated),
                instance(Some("urn:ngsi-ld:Dataset:b"), AttributeChangeKind::Deleted),
            ],
        );
        let at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let builder = ChangeEvent::builder()
            .entity_id("urn:ngsi-ld:Building:1".parse().expect("a valid entity URI"))
            .entity_types(vec![ex("Building").parse().expect("a valid type name")])
            .change(change)
            .occurred_at(at)
            .origin(origin);
        match lifecycle {
            EntityLifecycle::Deleted => builder
                .entity_snapshot(Some(Arc::clone(previous)))
                .previous_attributes(Some(Arc::clone(previous)))
                .sys_attrs_snapshot(Some(SysAttrsSnapshot {
                    created_at: at,
                    modified_at: at,
                    expires_at: Some(at),
                }))
                .build(),
            EntityLifecycle::Created | EntityLifecycle::Modified => builder
                .previous_attributes(Some(Arc::clone(previous)))
                .current_payload(Some(Arc::clone(current)))
                .build(),
        }
    }

    /// `event` stored as its record and read back.
    fn round_trip(event: &ChangeEvent) -> ChangeEvent {
        let states = [event.current_payload.as_deref(), event.previous_attributes.as_deref()];
        let (keys, names) = dictionary_of(states.into_iter().flatten());
        let record = record_of_event(event, &keys).expect("a committed change has a record");
        ReadChangeEvent::try_from(record)
            .expect("the record reads back")
            .assemble(&names)
            .expect("the names assemble")
    }

    #[test]
    fn a_change_event_round_trips_through_its_record() {
        let (current, previous) = (Arc::new(temperature(21)), Arc::new(temperature(20)));
        for lifecycle in LIFECYCLES {
            for origin in ORIGINS {
                let written = event(lifecycle, origin, &current, &previous);
                let read = round_trip(&written);
                pretty_assertions::assert_eq!(read.entity_id, written.entity_id);
                pretty_assertions::assert_eq!(read.entity_types, written.entity_types);
                pretty_assertions::assert_eq!(read.change, written.change);
                pretty_assertions::assert_eq!(read.occurred_at, written.occurred_at);
                pretty_assertions::assert_eq!(read.origin, written.origin);
                pretty_assertions::assert_eq!(read.entity_snapshot, written.entity_snapshot);
                pretty_assertions::assert_eq!(read.previous_attributes, written.previous_attributes);
                pretty_assertions::assert_eq!(read.current_payload, written.current_payload);
                pretty_assertions::assert_eq!(read.sys_attrs_snapshot, written.sys_attrs_snapshot);
            }
        }
    }

    /// Clause 5.8.6 notifies a deletion with the Entity it removed, which is
    /// also its previous state: the record stores it once and both roles read
    /// back as one state.
    #[test]
    fn a_deletion_stores_its_removed_state_once() {
        let removed = Arc::new(temperature(20));
        let deletion = event(EntityLifecycle::Deleted, ChangeOrigin::Local, &removed, &removed);
        let (keys, names) = dictionary_of([removed.as_ref()]);
        let record = record_of_event(&deletion, &keys).expect("a committed change has a record");
        assert!(record.entity_snapshot.is_some());
        assert!(record.previous_snapshot.is_none());
        let read = ReadChangeEvent::try_from(record)
            .expect("the record reads back")
            .assemble(&names)
            .expect("the names assemble");
        let (Some(snapshot), Some(previous)) = (&read.entity_snapshot, &read.previous_attributes) else {
            panic!("a deletion reads back its removed state in both roles: {read:?}");
        };
        assert!(Arc::ptr_eq(snapshot, previous));
        pretty_assertions::assert_eq!(snapshot.as_ref(), removed.as_ref());
        assert!(read.current_payload.is_none());
    }

    fn stored() -> PendingChangeEvent {
        let (current, previous) = (Arc::new(temperature(21)), Arc::new(temperature(20)));
        let (keys, _) = dictionary_of([current.as_ref(), previous.as_ref()]);
        record_of_event(&event(EntityLifecycle::Modified, ChangeOrigin::Local, &current, &previous), &keys).expect("a committed change has a record")
    }

    #[test]
    fn an_unknown_origin_code_is_refused() {
        let mut record = stored();
        record.origin = 99;
        assert!(matches!(ReadChangeEvent::try_from(record), Err(PendingChangeEventError::UnknownOrigin { .. })));
    }

    #[test]
    fn an_unknown_lifecycle_code_is_refused() {
        let (current, previous) = (temperature(21), temperature(20));
        let (_, names) = dictionary_of([&current, &previous]);
        let mut record = stored();
        record.lifecycle = 99;
        let read = ReadChangeEvent::try_from(record).expect("the lifecycle is read with the change");
        assert!(matches!(
            read.assemble(&names),
            Err(PendingChangeEventError::Change(StoredChangeError::UnknownLifecycle { .. }))
        ));
    }

    /// A key no dictionary row names is a store fault, never a change
    /// without that name.
    #[test]
    fn a_changed_attribute_key_without_a_name_is_refused() {
        let read = ReadChangeEvent::try_from(stored()).expect("the record reads back");
        assert!(read.assemble(&KeyNames::default()).is_err());
    }
}
