//! The events one delivery claim read back, each decoded once.
//!
//! A change watched by many Subscriptions is one event with many deliveries,
//! and a claim often holds several of them. The claim therefore reads each
//! event it needs once, and the deliveries of one event share the decoded
//! record, entity states included, instead of decoding the same document once
//! per delivery.

use crate::{
    json_text::JsonText,
    notification_delivery::change_code::{change_kind_of, lifecycle_of},
};
use andromeda_context_commit::{
    write_id::WriteId,
    write_ordinal::{WriteOrdinal, WriteOrdinalOutOfRange},
};
use andromeda_ngsi_ld_entity::{
    entity_state::EntityState,
    reads::{attribute_change::AttributeChange, attribute_instance_key::AttributeInstanceKey},
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::IdentifierParseError, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_model::{entity_change::EntityChange, notification::SysAttrsSnapshot};
use andromeda_notification::notification_event::{NotificationEvent, NotificationEventKey};
use andromeda_postgresql_codes::{attribute_change_kind::AttributeChangeKindCode, entity_lifecycle::EntityLifecycleCode, registry::UnknownCompactCode};
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
    /// A stored entity snapshot no longer decodes as an entity state.
    #[error("carries an unreadable entity snapshot")]
    UnreadableSnapshot {
        /// Why the stored document is not an entity state.
        #[source]
        source: EntityRefusal,
    },
    /// The removed entity's `createdAt` and `modifiedAt` are not stored
    /// together, which the event's own check constraint rules out.
    #[error("carries an incomplete set of entity system attributes")]
    IncompleteSystemAttributes,
}

/// SQL decode shape of one claimed event.
///
/// Crate-private so unvalidated database text cannot cross the persistence
/// boundary.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct RawClaimedEventRow<'r> {
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
    /// Entity state after the change, as the stored document's JSON text.
    pub(crate) entity_snapshot: Option<JsonText<'r>>,
    /// Entity state before the change, as the stored document's JSON text.
    pub(crate) previous_snapshot: Option<JsonText<'r>>,
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
    /// Decode every event row one claim read back.
    ///
    /// An event that does not decode is kept with its reason, so each of its
    /// deliveries can be reported unreadable under its own key.
    pub(crate) fn decode<'r>(rows: impl IntoIterator<Item = RawClaimedEventRow<'r>>) -> ClaimedEvents {
        ClaimedEvents(
            rows.into_iter()
                .map(|row| (NotificationEventKey::from(row.event_key), decode_event(row).map(Arc::new).map_err(Arc::new)))
                .collect(),
        )
    }

    /// The event under `key`, or `None` when the claim did not read it back.
    pub(crate) fn get(&self, key: NotificationEventKey) -> Option<&ClaimedEvent> {
        self.0.get(&key)
    }
}

/// Validate every stored value of one claimed event.
fn decode_event(row: RawClaimedEventRow<'_>) -> Result<NotificationEvent, ClaimedEventError> {
    let entity_id = row.entity_id.parse().map_err(|source| ClaimedEventError::InvalidEntityId { source })?;
    let event_ordinal = WriteOrdinal::try_from(row.event_ordinal).map_err(|source| ClaimedEventError::InvalidEventOrdinal { source })?;
    let change = decode_change(row.lifecycle, row.change_kinds, row.change_attributes, row.change_dataset_ids)?;
    let entity_snapshot = row.entity_snapshot.map(decode_snapshot).transpose()?;
    let previous_snapshot = row.previous_snapshot.map(decode_snapshot).transpose()?;
    let sys_attrs_snapshot = decode_system_attributes(row.entity_created_at, row.entity_modified_at, row.entity_expires_at)?;
    Ok(NotificationEvent::builder()
        .key(NotificationEventKey::from(row.event_key))
        .write_id(WriteId::from_uuid(row.write_id))
        .event_ordinal(event_ordinal)
        .change(change)
        .entity_id(entity_id)
        .entity_snapshot(entity_snapshot)
        .previous_snapshot(previous_snapshot)
        .sys_attrs_snapshot(sys_attrs_snapshot)
        .created_at(row.created_at)
        .build())
}

/// Decode one stored snapshot into the entity state it was written from.
fn decode_snapshot(text: JsonText<'_>) -> Result<Arc<EntityState>, ClaimedEventError> {
    EntityState::from_slice(text.as_bytes())
        .map(Arc::new)
        .map_err(|source| ClaimedEventError::UnreadableSnapshot { source })
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
        json_text::JsonText,
        notification_delivery::claimed_event::{ClaimedEventError, ClaimedEvents, RawClaimedEventRow},
    };
    use andromeda_context_commit::write_ordinal::WriteOrdinal;
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes, reads::attribute_change::AttributeChangeKind};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::entity_change::EntityLifecycle;
    use andromeda_notification::notification_event::{NotificationEvent, NotificationEventKey};
    use chrono::Utc;
    use std::sync::Arc;
    use uuid::Uuid;

    pub(crate) fn event_row() -> RawClaimedEventRow<'static> {
        RawClaimedEventRow {
            event_key: 3,
            write_id: Uuid::nil(),
            event_ordinal: 1,
            lifecycle: 2,
            entity_id: "https://example.org/entities/1".to_owned(),
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

    fn decoded(row: RawClaimedEventRow<'_>) -> Result<Arc<NotificationEvent>, Arc<ClaimedEventError>> {
        let key = NotificationEventKey::from(row.event_key);
        let events = ClaimedEvents::decode([row]);
        events.get(key).cloned().expect("the decoded event is held under its key")
    }

    fn refused(row: RawClaimedEventRow<'_>) -> Arc<ClaimedEventError> {
        decoded(row).expect_err("an unreadable event")
    }

    /// A stored snapshot reads back as the entity state it was written from,
    /// and the removed entity's system attributes travel beside it.
    #[test]
    fn a_stored_snapshot_and_its_system_attributes_read_back_typed() {
        let state = EntityState::new(
            "https://example.org/entities/1".parse().expect("a valid entity URI"),
            EntityTypes::one("https://example.org/Building".parse().expect("a valid type name")),
        );
        let encoded = state.to_vec().expect("the state encodes");
        let mut raw = event_row();
        raw.entity_snapshot = Some(JsonText::new(&encoded));
        let removed_at = Utc::now();
        raw.entity_created_at = Some(removed_at);
        raw.entity_modified_at = Some(removed_at);
        let event = decoded(raw).expect("a valid claimed event");
        pretty_assertions::assert_eq!(event.entity_snapshot.as_deref(), Some(&state));
        assert!(event.previous_snapshot.is_none());
        let sys_attrs = event.sys_attrs_snapshot.as_ref().expect("the stored system attributes");
        pretty_assertions::assert_eq!(sys_attrs.created_at, removed_at);
        assert!(sys_attrs.expires_at.is_none());
    }

    #[test]
    fn a_valid_row_decodes_every_domain_value() {
        let event = decoded(event_row()).expect("a valid claimed event");
        pretty_assertions::assert_eq!(event.key, NotificationEventKey::from(3_i64));
        pretty_assertions::assert_eq!(event.entity_id.as_str(), "https://example.org/entities/1");
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
        let mut raw = event_row();
        raw.previous_snapshot = Some(JsonText::new(br#"[{"@id":"urn:ngsi-ld:A"}]"#));
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
        let events = ClaimedEvents::decode([event_row()]);
        assert!(events.get(NotificationEventKey::from(4_i64)).is_none());
    }
}
