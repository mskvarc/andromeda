//! The events one delivery claim read back, each decoded once.
//!
//! A change watched by many Subscriptions is one event with many deliveries,
//! and a claim often holds several of them. The claim therefore reads each
//! event it needs once, and the deliveries of one event share the decoded
//! record, entity states included, instead of decoding the same state once
//! per delivery.
//!
//! An event stores its states as `entity_state_record`s, every name a
//! dictionary key. The claim decodes every event first, looks up the names of
//! every key the claim's states hold, one lookup per dictionary for the whole
//! claim, and only then assembles the states.

use crate::{
    entity::stored_entity_error::StoredEntityError,
    entity_state_record::{state_record::EntityStateRecord, stored_state::StoredState},
    notification_delivery::change_code::{change_kind_of, lifecycle_of},
};
use andromeda_context_commit::{
    write_id::WriteId,
    write_ordinal::{WriteOrdinal, WriteOrdinalOutOfRange},
};
use andromeda_ngsi_ld_entity::{
    entity_state::EntityState,
    reads::{attribute_change::AttributeChange, attribute_instance_key::AttributeInstanceKey},
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    identifier::{Identifier, IdentifierParseError},
    reference::{NgsiLdReference, NgsiLdReferenceError},
};
use andromeda_ngsi_ld_model::{
    entity_change::{EntityChange, EntityLifecycle},
    notification::SysAttrsSnapshot,
};
use andromeda_notification::notification_event::{NotificationEvent, NotificationEventKey};
use andromeda_postgresql_attribute_instance::name_keys::{HeldKeys, KeyNames};
use andromeda_postgresql_codes::{attribute_change_kind::AttributeChangeKindCode, entity_lifecycle::EntityLifecycleCode, registry::UnknownCompactCode};
use andromeda_postgresql_name_dictionary::name_lookup::{NameLookup, NameLookupError};
use chrono::{DateTime, Utc};
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;
use uuid::Uuid;

/// Why a claimed event could not be read back as the event it was written as.
#[derive(Debug, Error)]
pub enum ClaimedEventError {
    /// The entity identifier is not a URI permitted by clause 4.6.2.
    #[error("references an invalid entity URI")]
    InvalidEntityId {
        /// URI validation failure.
        #[source]
        source: IdentifierParseError,
    },
    /// The stored number names no Entity lifecycle transition, which is what a
    /// row written by a release that knows more transitions looks like.
    #[error("carries an unreadable lifecycle")]
    UnknownLifecycle {
        /// Registry rejection naming the number.
        #[source]
        source: UnknownCompactCode,
    },
    /// A stored number names no Attribute instance transition.
    #[error("carries an unreadable instance transition")]
    UnknownChangeKind {
        /// Registry rejection naming the number.
        #[source]
        source: UnknownCompactCode,
    },
    /// The per-instance arrays disagree in length, so no instance can be
    /// told apart from its neighbour.
    #[error("carries {kinds} instance transitions for {attributes} attributes and {datasets} datasetIds")]
    ChangeArity {
        /// Number of stored transitions.
        kinds: usize,
        /// Number of attribute names the dictionary resolved.
        attributes: usize,
        /// Number of stored dataset discriminators.
        datasets: usize,
    },
    /// The stored event ordinal is outside the range the write identity numbers.
    #[error("carries an unreadable event ordinal")]
    InvalidEventOrdinal {
        /// Ordinal range failure.
        #[source]
        source: WriteOrdinalOutOfRange,
    },
    /// One changed attribute is not a valid NGSI-LD attribute name.
    #[error("references an invalid attribute name")]
    InvalidAttributeName {
        /// Attribute-name validation failure.
        #[source]
        source: NgsiLdReferenceError,
    },
    /// The stored dataset discriminator is not a valid URI.
    #[error("references an invalid datasetId")]
    InvalidDatasetId {
        /// URI validation failure.
        #[source]
        source: IdentifierParseError,
    },
    /// A stored entity state no longer reads back as an entity state.
    #[error("carries an unreadable entity snapshot")]
    UnreadableSnapshot {
        /// Why the stored record is not an entity state.
        #[source]
        source: StoredEntityError,
    },
    /// The removed entity's `createdAt` and `modifiedAt` are not stored
    /// together, which the event's own check constraint rules out.
    #[error("carries an incomplete set of entity system attributes")]
    IncompleteSystemAttributes,
}

/// SQL decode shape of one claimed event.
///
/// Crate-private so unvalidated database values cannot cross the persistence
/// boundary.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct RawClaimedEventRow {
    /// Database-generated event key.
    pub(crate) event_key: i64,
    /// Raw write identity decoded by the row mapper.
    pub(crate) write_id: Uuid,
    /// Raw event ordinal decoded by the row mapper.
    pub(crate) event_ordinal: i32,
    /// Raw lifecycle number decoded by the row mapper.
    pub(crate) lifecycle: i16,
    /// Raw entity identifier decoded by the row mapper.
    pub(crate) entity_id: String,
    /// Raw transition number of each changed instance.
    pub(crate) change_kinds: Vec<i16>,
    /// Attribute name of each changed instance, resolved from the dictionary
    /// by the statement.
    ///
    /// The event stores dictionary keys; the claim resolves them back to the
    /// expanded IRIs a notification is built from, position for position.
    pub(crate) change_attributes: Vec<String>,
    /// Dataset discriminator of each changed instance, `None` for the default
    /// instance.
    pub(crate) change_dataset_ids: Vec<Option<String>>,
    /// Entity state after the change, or the state a deletion removed.
    pub(crate) entity_snapshot: Option<EntityStateRecord>,
    /// Entity state before a change that left the Entity in place.
    pub(crate) previous_snapshot: Option<EntityStateRecord>,
    /// Clause 4.8 `createdAt` of an entity a deletion removed.
    pub(crate) entity_created_at: Option<DateTime<Utc>>,
    /// Clause 4.8 `modifiedAt` of an entity a deletion removed.
    pub(crate) entity_modified_at: Option<DateTime<Utc>>,
    /// Clause 4.22 `expiresAt` of an entity a deletion removed.
    pub(crate) entity_expires_at: Option<DateTime<Utc>>,
    /// Instant the event was recorded.
    pub(crate) created_at: DateTime<Utc>,
}

/// One claimed event as its deliveries share it: the decoded event, or why it
/// could not be decoded.
pub(crate) type ClaimedEvent = Result<Arc<NotificationEvent>, Arc<ClaimedEventError>>;

/// The events one claim read back, by key, each decoded once.
#[derive(Debug, Default)]
pub(crate) struct ClaimedEvents(HashMap<NotificationEventKey, ClaimedEvent>);

impl ClaimedEvents {
    /// The event under `key`, or `None` when the claim did not read it back.
    pub(crate) fn get(&self, key: NotificationEventKey) -> Option<&ClaimedEvent> {
        self.0.get(&key)
    }
}

/// One claimed event, every stored value validated, its states still holding
/// dictionary keys.
#[derive(Debug)]
struct ReadEvent {
    /// The event's key.
    key: NotificationEventKey,
    /// The identity of the write that produced it.
    write_id: WriteId,
    /// Its position within that write.
    event_ordinal: WriteOrdinal,
    /// What the write did to the Entity.
    change: EntityChange,
    /// The Entity changed.
    entity_id: Identifier,
    /// The Entity's state after the change, or the state a deletion removed.
    entity_state: Option<StoredState>,
    /// The Entity's state before a change that left it in place.
    previous_state: Option<StoredState>,
    /// The clause 4.8 system attributes of an Entity a deletion removed.
    sys_attrs_snapshot: Option<SysAttrsSnapshot>,
    /// Instant the event was recorded.
    created_at: DateTime<Utc>,
}

/// The events one claim read back, by key, their states still holding
/// dictionary keys.
#[derive(Debug, Default)]
pub(crate) struct ReadEvents(Vec<(NotificationEventKey, Result<ReadEvent, ClaimedEventError>)>);

impl ReadEvents {
    /// Validate every event row one claim read back.
    ///
    /// An event that does not decode is kept with its reason, so each of its
    /// deliveries can be reported unreadable under its own key.
    pub(crate) fn read(rows: impl IntoIterator<Item = RawClaimedEventRow>) -> ReadEvents {
        ReadEvents(
            rows.into_iter()
                .map(|row| (NotificationEventKey::from(row.event_key), read_event(row)))
                .collect(),
        )
    }

    /// Every dictionary key the states of the readable events hold, each once.
    pub(crate) fn held_keys(&self) -> HeldKeys {
        let mut held = HeldKeys::default();
        for state in self
            .0
            .iter()
            .filter_map(|(_, event)| event.as_ref().ok())
            .flat_map(|event| event.entity_state.iter().chain(&event.previous_state))
        {
            state.collect_keys(&mut held);
        }
        held.attributes.sort_unstable_by_key(|key| key.get());
        held.attributes.dedup();
        held.types.sort_unstable_by_key(|key| key.get());
        held.types.dedup();
        held
    }

    /// The events with their states assembled, every key read back through
    /// `names`.
    pub(crate) fn assemble(self, names: &KeyNames) -> ClaimedEvents {
        ClaimedEvents(
            self.0
                .into_iter()
                .map(|(key, event)| {
                    let event = event.and_then(|event| assemble_event(event, names));
                    (key, event.map(Arc::new).map_err(Arc::new))
                })
                .collect(),
        )
    }
}

/// The names behind `held`, one lookup per dictionary.
///
/// # Errors
///
/// Returns [`NameLookupError`] when a dictionary cannot be read.
pub(crate) async fn names_of(
    held: &HeldKeys,
    attribute_names: &dyn NameLookup<AttributeName>,
    entity_type_names: &dyn NameLookup<EntityTypeName>,
) -> Result<KeyNames, NameLookupError> {
    let attributes = attribute_names.names_of(&held.attributes).await?;
    let types = entity_type_names.names_of(&held.types).await?;
    Ok(KeyNames::from_pairs(
        attributes,
        types.iter().map(|(key, name)| (*key, NgsiLdReference::from(name))),
    ))
}

/// Validate every stored value of one claimed event but its states' names.
fn read_event(row: RawClaimedEventRow) -> Result<ReadEvent, ClaimedEventError> {
    let state = |record: Option<EntityStateRecord>| {
        record
            .map(StoredState::try_from)
            .transpose()
            .map_err(|source| ClaimedEventError::UnreadableSnapshot { source })
    };
    Ok(ReadEvent {
        key: NotificationEventKey::from(row.event_key),
        write_id: WriteId::from_uuid(row.write_id),
        event_ordinal: WriteOrdinal::try_from(row.event_ordinal).map_err(|source| ClaimedEventError::InvalidEventOrdinal { source })?,
        change: decode_change(row.lifecycle, row.change_kinds, row.change_attributes, row.change_dataset_ids)?,
        entity_id: row.entity_id.parse().map_err(|source| ClaimedEventError::InvalidEntityId { source })?,
        entity_state: state(row.entity_snapshot)?,
        previous_state: state(row.previous_snapshot)?,
        sys_attrs_snapshot: decode_system_attributes(row.entity_created_at, row.entity_modified_at, row.entity_expires_at)?,
        created_at: row.created_at,
    })
}

/// The event `read` holds, its states assembled through `names`.
///
/// A deletion stores its removed state once, and that state is its previous
/// state too: both are the one assembled state.
fn assemble_event(read: ReadEvent, names: &KeyNames) -> Result<NotificationEvent, ClaimedEventError> {
    let state = |stored: Option<StoredState>| {
        stored
            // clone: every state of the event owns the identifier it is assembled under.
            .map(|stored| stored.assemble(read.entity_id.clone(), names).map(Arc::new))
            .transpose()
            .map_err(|source| ClaimedEventError::UnreadableSnapshot { source })
    };
    let entity_snapshot: Option<Arc<EntityState>> = state(read.entity_state)?;
    let previous_snapshot = match read.change.lifecycle {
        EntityLifecycle::Deleted => entity_snapshot.as_ref().map(Arc::clone),
        EntityLifecycle::Created | EntityLifecycle::Modified => state(read.previous_state)?,
    };
    Ok(NotificationEvent::builder()
        .key(read.key)
        .write_id(read.write_id)
        .event_ordinal(read.event_ordinal)
        .change(read.change)
        .entity_id(read.entity_id)
        .entity_snapshot(entity_snapshot)
        .previous_snapshot(previous_snapshot)
        .sys_attrs_snapshot(read.sys_attrs_snapshot)
        .created_at(read.created_at)
        .build())
}

/// Read the removed entity's stored system attributes back: none, or a
/// `createdAt` and `modifiedAt` together with the optional `expiresAt`.
const fn decode_system_attributes(
    created_at: Option<DateTime<Utc>>,
    modified_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
) -> Result<Option<SysAttrsSnapshot>, ClaimedEventError> {
    match (created_at, modified_at, expires_at) {
        (Some(created_at), Some(modified_at), expires_at) => Ok(Some(SysAttrsSnapshot {
            created_at,
            modified_at,
            expires_at,
        })),
        (None, None, None) => Ok(None),
        (Some(_), None, _) | (None, Some(_), _) | (None, None, Some(_)) => Err(ClaimedEventError::IncompleteSystemAttributes),
    }
}

/// Validate the stored change: the lifecycle and, position for position, each
/// changed instance's transition, attribute and `datasetId`.
fn decode_change(lifecycle: i16, kinds: Vec<i16>, attributes: Vec<String>, datasets: Vec<Option<String>>) -> Result<EntityChange, ClaimedEventError> {
    let lifecycle = EntityLifecycleCode::try_from(lifecycle)
        .map(lifecycle_of)
        .map_err(|source| ClaimedEventError::UnknownLifecycle { source })?;
    if kinds.len() != attributes.len() || kinds.len() != datasets.len() {
        return Err(ClaimedEventError::ChangeArity {
            kinds: kinds.len(),
            attributes: attributes.len(),
            datasets: datasets.len(),
        });
    }
    let instances = kinds
        .into_iter()
        .zip(attributes)
        .zip(datasets)
        .map(|((kind, attribute), dataset)| {
            let kind = AttributeChangeKindCode::try_from(kind)
                .map(change_kind_of)
                .map_err(|source| ClaimedEventError::UnknownChangeKind { source })?;
            let attr: AttributeName = attribute.parse().map_err(|source| ClaimedEventError::InvalidAttributeName { source })?;
            let dataset_id = dataset
                .map(|value| value.parse().map_err(|source| ClaimedEventError::InvalidDatasetId { source }))
                .transpose()?;
            Ok(AttributeChange {
                key: AttributeInstanceKey { attr, dataset_id },
                kind,
            })
        })
        .collect::<Result<Vec<AttributeChange>, ClaimedEventError>>()?;
    Ok(EntityChange::new(lifecycle, instances))
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::{
        current_attribute::round_trip_tests::{committed, dictionary_of, ex, ngsi, property, with},
        entity::stored_entity_error::StoredEntityError,
        entity_state_record::state_record::{EntityStateRecord, record_of},
        notification_delivery::claimed_event::{ClaimedEventError, ClaimedEvents, RawClaimedEventRow, ReadEvents, names_of},
    };
    use andromeda_context_commit::write_ordinal::WriteOrdinal;
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, reads::attribute_change::AttributeChangeKind};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::entity_change::EntityLifecycle;
    use andromeda_notification::notification_event::{NotificationEvent, NotificationEventKey};
    use andromeda_postgresql_attribute_instance::{name_keys::KeyNames, stored_members::StoredInstanceError};
    use andromeda_postgresql_name_dictionary::name_lookup::{NameLookup, NameLookupError};
    use andromeda_postgresql_query::dictionary::{interned_name::InternedName, resolved_names::ResolvedNames};
    use chrono::Utc;
    use serde_json::{Value, json};
    use sqlx::PgConnection;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };
    use uuid::Uuid;

    pub(crate) fn event_row() -> RawClaimedEventRow {
        RawClaimedEventRow {
            event_key: 3,
            write_id: Uuid::nil(),
            event_ordinal: 1,
            lifecycle: 2,
            entity_id: "urn:ngsi-ld:Building:1".to_owned(),
            change_kinds: vec![2, 3],
            change_attributes: vec!["temperature".to_owned(), "https://example.org/pressure".to_owned()],
            change_dataset_ids: vec![None, Some("urn:ngsi-ld:Dataset:a".to_owned())],
            entity_snapshot: None,
            previous_snapshot: None,
            entity_created_at: None,
            entity_modified_at: None,
            entity_expires_at: None,
            created_at: Utc::now(),
        }
    }

    /// The Building `urn:ngsi-ld:Building:1` with the attributes `members`.
    pub(crate) fn building(members: &Value) -> EntityState {
        committed(&json!([with(json!({"@id": "urn:ngsi-ld:Building:1", "@type": [ex("Building")]}), members)]))
    }

    /// A Building whose `temperature` holds `value`, with a sub-attribute
    /// whose name no change mentions.
    pub(crate) fn temperature(value: i64) -> EntityState {
        building(&json!({ex("temperature"): [with(property(&json!([{"@value": value}])), &json!({
            ex("accuracy"): [property(&json!([{"@value": 0.5}]))]
        }))]}))
    }

    /// The records of `states` and the names behind their keys, under one
    /// dictionary.
    pub(crate) fn records(states: &[&EntityState]) -> (Vec<EntityStateRecord>, KeyNames) {
        let (keys, names) = dictionary_of(states.iter().copied());
        let records = states
            .iter()
            .map(|state| record_of(state, &keys).expect("a committed state has a record"))
            .collect();
        (records, names)
    }

    fn decoded(row: RawClaimedEventRow, names: &KeyNames) -> Result<Arc<NotificationEvent>, Arc<ClaimedEventError>> {
        let key = NotificationEventKey::from(row.event_key);
        let events: ClaimedEvents = ReadEvents::read([row]).assemble(names);
        events.get(key).cloned().expect("the decoded event is held under its key")
    }

    fn refused(row: RawClaimedEventRow) -> Arc<ClaimedEventError> {
        decoded(row, &KeyNames::default()).expect_err("an unreadable event")
    }

    /// A stored state reads back as the entity state it was written from,
    /// and the removed entity's system attributes travel beside it.
    #[test]
    fn a_stored_snapshot_and_its_system_attributes_read_back_typed() {
        let state = temperature(20);
        let (mut records, names) = records(&[&state]);
        let mut raw = event_row();
        raw.entity_snapshot = records.pop();
        let removed_at = Utc::now();
        raw.entity_created_at = Some(removed_at);
        raw.entity_modified_at = Some(removed_at);
        let event = decoded(raw, &names).expect("a valid claimed event");
        pretty_assertions::assert_eq!(event.entity_snapshot.as_deref(), Some(&state));
        assert!(event.previous_snapshot.is_none());
        let sys_attrs = event.sys_attrs_snapshot.as_ref().expect("the stored system attributes");
        pretty_assertions::assert_eq!(sys_attrs.created_at, removed_at);
        assert!(sys_attrs.expires_at.is_none());
    }

    /// Clause 5.8.6 notifies a deletion with the state it removed, which is
    /// the state before it too; the event stores it once.
    #[test]
    fn a_deleted_events_previous_state_is_its_removed_state() {
        let removed = temperature(20);
        let (mut records, names) = records(&[&removed]);
        let mut raw = event_row();
        raw.lifecycle = 3;
        raw.entity_snapshot = records.pop();
        let event = decoded(raw, &names).expect("a valid claimed event");
        let (Some(notified), Some(previous)) = (&event.entity_snapshot, &event.previous_snapshot) else {
            panic!("a deletion carries its removed state as both states: {event:?}");
        };
        assert!(Arc::ptr_eq(notified, previous));
        pretty_assertions::assert_eq!(notified.as_ref(), &removed);
    }

    /// Clause 5.2.14.1 `showChanges` reads the previous value from the state
    /// before the change, which a modification stores apart.
    #[test]
    fn a_modified_events_states_decode_apart() {
        let (after, before) = (temperature(21), temperature(20));
        let (records, names) = records(&[&after, &before]);
        let [current, previous] = <[EntityStateRecord; 2]>::try_from(records).expect("two records");
        let mut raw = event_row();
        raw.entity_snapshot = Some(current);
        raw.previous_snapshot = Some(previous);
        let event = decoded(raw, &names).expect("a valid claimed event");
        pretty_assertions::assert_eq!(event.entity_snapshot.as_deref(), Some(&after));
        pretty_assertions::assert_eq!(event.previous_snapshot.as_deref(), Some(&before));
    }

    /// The names a claim's states hold are looked up for the whole claim at
    /// once: one lookup per dictionary, each key asked for once.
    #[tokio::test]
    async fn the_names_of_a_claims_events_are_looked_up_once_per_dictionary() {
        let states = [
            temperature(20),
            temperature(21),
            building(&json!({ex("owner"): [{
                "@type": [ngsi("Relationship")],
                ngsi("hasObject"): [{"@id": "urn:ngsi-ld:Person:1"}],
                ngsi("hasObjectType"): [{"@id": ex("Person")}]
            }]})),
        ];
        let (keys, _) = dictionary_of(&states);
        let rows = states.iter().zip(1..).map(|(state, event_key)| {
            let mut raw = event_row();
            raw.event_key = event_key;
            raw.entity_snapshot = Some(record_of(state, &keys).expect("a committed state has a record"));
            raw
        });
        let read = ReadEvents::read(rows);
        let attributes = RecordingNames::default();
        let types = RecordingNames::default();
        names_of(&read.held_keys(), &attributes, &types).await.expect("the dictionaries answer");
        let attribute_calls = attributes.calls();
        let type_calls = types.calls();
        pretty_assertions::assert_eq!(attribute_calls.len(), 1, "one attribute lookup: {attribute_calls:?}");
        pretty_assertions::assert_eq!(type_calls.len(), 1, "one type lookup: {type_calls:?}");
        // temperature, accuracy and owner; Building and Person.
        pretty_assertions::assert_eq!(attribute_calls[0].len(), 3, "{attribute_calls:?}");
        pretty_assertions::assert_eq!(type_calls[0].len(), 2, "{type_calls:?}");
    }

    /// A dictionary double that answers no name and records every key set it
    /// is asked for.
    #[derive(Debug, Default)]
    struct RecordingNames {
        /// The key sets `names_of` was asked for, in call order.
        asked: Mutex<Vec<Vec<i32>>>,
    }

    impl RecordingNames {
        fn calls(&self) -> Vec<Vec<i32>> {
            self.asked.lock().expect("the record is not poisoned").clone()
        }
    }

    #[async_trait::async_trait]
    impl<N: InternedName> NameLookup<N> for RecordingNames {
        async fn lookup(&self, _names: &[N]) -> Result<ResolvedNames<N>, NameLookupError> {
            Ok(ResolvedNames::default())
        }

        async fn resolve_or_create(&self, _connection: &mut PgConnection, _names: &[N]) -> Result<HashMap<N, N::Key>, NameLookupError> {
            Ok(HashMap::new())
        }

        async fn names_of(&self, keys: &[N::Key]) -> Result<HashMap<N::Key, N>, NameLookupError> {
            self.asked
                .lock()
                .expect("the record is not poisoned")
                .push(keys.iter().map(|key| N::key_value(*key)).collect());
            Ok(HashMap::new())
        }
    }

    #[test]
    fn a_valid_row_decodes_every_domain_value() {
        let event = decoded(event_row(), &KeyNames::default()).expect("a valid claimed event");
        pretty_assertions::assert_eq!(event.key, NotificationEventKey::from(3_i64));
        pretty_assertions::assert_eq!(event.entity_id.as_str(), "urn:ngsi-ld:Building:1");
        pretty_assertions::assert_eq!(event.change.lifecycle, EntityLifecycle::Modified);
        pretty_assertions::assert_eq!(event.event_ordinal, WriteOrdinal::at(1));
        pretty_assertions::assert_eq!(event.change.instances[0].key.attr.as_str(), "temperature");
        pretty_assertions::assert_eq!(event.change.instances[0].kind, AttributeChangeKind::Updated);
        assert!(event.change.instances[0].key.dataset_id.is_none());
        pretty_assertions::assert_eq!(event.change.instances[1].kind, AttributeChangeKind::Deleted);
        pretty_assertions::assert_eq!(
            event.change.instances[1].key.dataset_id.as_ref().map(Identifier::as_str),
            Some("urn:ngsi-ld:Dataset:a")
        );
    }

    #[test]
    fn a_snapshot_that_is_no_entity_state_is_rejected() {
        let state = temperature(20);
        let (mut records, _) = records(&[&state]);
        let mut record = records.pop().expect("one record");
        if let Some(instance) = record.instances.first_mut() {
            instance.value_string = Some("twenty".to_owned());
        }
        let mut raw = event_row();
        raw.previous_snapshot = Some(record);
        assert!(matches!(
            refused(raw).as_ref(),
            ClaimedEventError::UnreadableSnapshot {
                source: StoredEntityError::Row(row)
            } if matches!(row.as_ref(), StoredInstanceError::ValueFields { .. })
        ));
    }

    /// A key no dictionary row names is a store fault, never a state without
    /// that name.
    #[test]
    fn a_state_holding_a_key_without_a_name_is_rejected() {
        let state = temperature(20);
        let (mut records, _) = records(&[&state]);
        let mut raw = event_row();
        raw.entity_snapshot = records.pop();
        assert!(matches!(refused(raw).as_ref(), ClaimedEventError::UnreadableSnapshot { .. }));
    }

    #[test]
    fn a_creation_instant_without_a_modification_instant_is_rejected() {
        let mut raw = event_row();
        raw.entity_created_at = Some(Utc::now());
        assert!(matches!(refused(raw).as_ref(), ClaimedEventError::IncompleteSystemAttributes));
    }

    #[test]
    fn an_invalid_entity_identifier_is_rejected() {
        let mut raw = event_row();
        raw.entity_id = "not a uri".to_owned();
        assert!(matches!(refused(raw).as_ref(), ClaimedEventError::InvalidEntityId { .. }));
    }

    #[test]
    fn a_lifecycle_number_the_registry_never_assigned_is_rejected() {
        let mut raw = event_row();
        raw.lifecycle = 99;
        assert!(matches!(refused(raw).as_ref(), ClaimedEventError::UnknownLifecycle { .. }));
    }

    #[test]
    fn an_instance_transition_number_the_registry_never_assigned_is_rejected() {
        let mut raw = event_row();
        raw.change_kinds = vec![2, 99];
        assert!(matches!(refused(raw).as_ref(), ClaimedEventError::UnknownChangeKind { .. }));
    }

    /// An attribute key the dictionary no longer resolves shortens the
    /// resolved names, and the instances can then no longer be paired up.
    #[test]
    fn per_instance_arrays_of_different_lengths_are_rejected() {
        let mut raw = event_row();
        raw.change_attributes = vec!["temperature".to_owned()];
        assert!(matches!(
            refused(raw).as_ref(),
            ClaimedEventError::ChangeArity {
                kinds: 2,
                attributes: 1,
                datasets: 2
            }
        ));
    }

    #[test]
    fn a_negative_event_ordinal_is_rejected() {
        let mut raw = event_row();
        raw.event_ordinal = -1;
        assert!(matches!(refused(raw).as_ref(), ClaimedEventError::InvalidEventOrdinal { .. }));
    }

    #[test]
    fn an_invalid_changed_attribute_is_rejected() {
        let mut raw = event_row();
        raw.change_attributes = vec!["bad name".to_owned(), "https://example.org/pressure".to_owned()];
        assert!(matches!(refused(raw).as_ref(), ClaimedEventError::InvalidAttributeName { .. }));
    }

    #[test]
    fn an_event_the_claim_did_not_read_is_absent() {
        let events = ReadEvents::read([event_row()]).assemble(&KeyNames::default());
        assert!(events.get(NotificationEventKey::from(4_i64)).is_none());
    }
}
