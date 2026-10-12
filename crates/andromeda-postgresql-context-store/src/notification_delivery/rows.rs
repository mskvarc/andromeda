//! Validated notification delivery records and their mapper errors.

use crate::{
    json_text::JsonText,
    notification_delivery::change_code::{change_kind_of, lifecycle_of},
};
use andromeda_context_commit::{
    subscription_key::SubscriptionKey,
    write_id::WriteId,
    write_ordinal::{WriteOrdinal, WriteOrdinalOutOfRange},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::{
    entity_state::EntityState,
    reads::{attribute_change::AttributeChange, attribute_instance_key::AttributeInstanceKey},
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    identifier::{Identifier, IdentifierParseError},
    reference::NgsiLdReferenceError,
};
use andromeda_ngsi_ld_model::{entity_change::EntityChange, notification::SysAttrsSnapshot};
use andromeda_notification::delivery_entry::DeliveryKey;
use andromeda_postgresql_codes::{attribute_change_kind::AttributeChangeKindCode, entity_lifecycle::EntityLifecycleCode, registry::UnknownCompactCode};
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use chrono::{DateTime, Utc};
use thiserror::Error;
use uuid::Uuid;

/// One claimed delivery, together with the event it answers.
///
/// The queue columns come from `notification_deliveries` and the payload
/// columns from the `notification_events` row it references, because a worker
/// needs both to build a notification: what to send, and how many times sending
/// it has already been attempted.
#[derive(Debug)]
pub struct ClaimedDelivery {
    /// Context branch that owns the event and subscription.
    pub target: ContextTarget,
    /// Key of the claimed delivery row.
    pub delivery_key: DeliveryKey,
    /// Subscription this delivery answers.
    pub subscription_id: Identifier,
    /// Delivery attempts already made.
    pub attempt_count: i32,
    /// Earliest instant a further attempt may be made.
    pub next_attempt_at: DateTime<Utc>,
    /// Key of the event this delivery carries.
    pub event_key: i64,
    /// Write that produced the event.
    pub write_id: WriteId,
    /// Position of the event within that write.
    pub event_ordinal: WriteOrdinal,
    /// What the write did to the Entity (clause 5.8.6).
    pub change: EntityChange,
    /// Entity whose change the event records.
    pub entity_id: Identifier,
    /// Entity state after the change, or the state a deletion removed.
    pub entity_snapshot: Option<EntityState>,
    /// Entity state before the change, for `showChanges`.
    pub previous_snapshot: Option<EntityState>,
    /// The clause 4.8 system attributes of an entity a deletion removed.
    pub sys_attrs_snapshot: Option<SysAttrsSnapshot>,
    /// Instant the event was recorded.
    pub created_at: DateTime<Utc>,
}

/// A claimed delivery whose row could not be read back as its domain record.
///
/// The claim leased the row before reading it, so the branch and key its
/// acknowledgement addresses travel with the reason it could not be read.
#[derive(Debug, Error)]
#[error("notification delivery {delivery_key} {reason}")]
pub struct UnreadableClaimedDelivery {
    /// Context branch whose queue holds the row.
    pub target: ContextTarget,
    /// Key of the leased row.
    pub delivery_key: DeliveryKey,
    /// Why the row could not be read back.
    pub reason: ClaimedDeliveryError,
}

/// Why a claimed delivery could not be read back as its domain record.
#[derive(Debug, Error)]
pub enum ClaimedDeliveryError {
    /// The subscription identifier is not a URI permitted by clause 4.6.2.
    #[error("references an invalid subscription URI")]
    InvalidSubscriptionId {
        /// URI validation failure.
        #[source]
        source: IdentifierParseError,
    },
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

/// SQL decode shape of one claimed delivery joined to its event.
///
/// Crate-private so unvalidated database text cannot cross the persistence
/// boundary.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct RawClaimedDeliveryRow<'r> {
    /// Database-generated delivery key.
    delivery_key: i64,
    /// Raw subscription identifier decoded by the row mapper.
    subscription_id: String,
    /// Delivery attempts already made.
    attempt_count: i32,
    /// Earliest instant a further attempt may be made.
    next_attempt_at: DateTime<Utc>,
    /// Database-generated event key.
    event_key: i64,
    /// Raw write identity decoded by the row mapper.
    write_id: Uuid,
    /// Raw event ordinal decoded by the row mapper.
    event_ordinal: i32,
    /// Raw lifecycle number decoded by the row mapper.
    lifecycle: i16,
    /// Raw entity identifier decoded by the row mapper.
    entity_id: String,
    /// Raw transition number of each changed instance.
    change_kinds: Vec<i16>,
    /// Attribute name of each changed instance, resolved from the dictionary
    /// by the statement.
    ///
    /// The event stores dictionary keys; the claim resolves them back to the
    /// expanded IRIs a notification is built from, position for position.
    change_attributes: Vec<String>,
    /// Dataset discriminator of each changed instance, `None` for the default
    /// instance.
    change_dataset_ids: Vec<Option<String>>,
    /// Entity state after the change, as the stored document's JSON text.
    entity_snapshot: Option<JsonText<'r>>,
    /// Entity state before the change, as the stored document's JSON text.
    previous_snapshot: Option<JsonText<'r>>,
    /// Clause 4.8 `createdAt` of an entity a deletion removed.
    entity_created_at: Option<DateTime<Utc>>,
    /// Clause 4.8 `modifiedAt` of an entity a deletion removed.
    entity_modified_at: Option<DateTime<Utc>>,
    /// Clause 4.22 `expiresAt` of an entity a deletion removed.
    entity_expires_at: Option<DateTime<Utc>>,
    /// Instant the event was recorded.
    created_at: DateTime<Utc>,
}

impl ClaimedDelivery {
    /// Validate one claimed row into its domain record, attaching the branch
    /// `target` it was drained from.
    ///
    /// The branch is the caller's, not decoded from the row: a delivery lives in
    /// its branch's own queue, so the drain that reads `snap_<key>.notification_deliveries`
    /// already knows which Snapshot it is answering for. Validation still spans
    /// several independent domain types, so no derive expresses it.
    ///
    /// # Errors
    ///
    /// Returns [`UnreadableClaimedDelivery`] when a stored identifier,
    /// transition, ordinal, attribute name or dataset id is not a valid domain
    /// value, or the per-instance arrays disagree in length,
    /// carrying the branch and key the leased row's acknowledgement addresses.
    /// It is boxed because it is rare and far wider than the success path.
    pub(crate) fn from_row(row: RawClaimedDeliveryRow<'_>, target: &ContextTarget) -> Result<ClaimedDelivery, Box<UnreadableClaimedDelivery>> {
        let delivery_key = DeliveryKey::from(row.delivery_key);
        ClaimedDelivery::decode(row, target).map_err(|reason| {
            Box::new(UnreadableClaimedDelivery {
                target: target.clone(), // clone: the unreadable row answers for the branch every row of the claim shares.
                delivery_key,
                reason,
            })
        })
    }

    /// Validate every stored value of one claimed row.
    fn decode(row: RawClaimedDeliveryRow<'_>, target: &ContextTarget) -> Result<ClaimedDelivery, ClaimedDeliveryError> {
        let subscription_id = row
            .subscription_id
            .parse()
            .map_err(|source| ClaimedDeliveryError::InvalidSubscriptionId { source })?;
        let entity_id = row.entity_id.parse().map_err(|source| ClaimedDeliveryError::InvalidEntityId { source })?;
        let event_ordinal = WriteOrdinal::try_from(row.event_ordinal).map_err(|source| ClaimedDeliveryError::InvalidEventOrdinal { source })?;
        let change = decode_change(row.lifecycle, row.change_kinds, row.change_attributes, row.change_dataset_ids)?;
        let entity_snapshot = row.entity_snapshot.map(decode_snapshot).transpose()?;
        let previous_snapshot = row.previous_snapshot.map(decode_snapshot).transpose()?;
        let sys_attrs_snapshot = decode_system_attributes(row.entity_created_at, row.entity_modified_at, row.entity_expires_at)?;
        Ok(ClaimedDelivery {
            target: target.clone(), // clone: every row of the claim answers for the one branch it was drained from.
            delivery_key: DeliveryKey::from(row.delivery_key),
            subscription_id,
            attempt_count: row.attempt_count,
            next_attempt_at: row.next_attempt_at,
            event_key: row.event_key,
            write_id: WriteId::from_uuid(row.write_id),
            event_ordinal,
            change,
            entity_id,
            entity_snapshot,
            previous_snapshot,
            sys_attrs_snapshot,
            created_at: row.created_at,
        })
    }
}

/// Decode one stored snapshot into the entity state it was written from.
fn decode_snapshot(text: JsonText<'_>) -> Result<EntityState, ClaimedDeliveryError> {
    EntityState::from_slice(text.as_bytes()).map_err(|source| ClaimedDeliveryError::UnreadableSnapshot { source })
}

/// Read the removed entity's stored system attributes back: none, or a
/// `createdAt` and `modifiedAt` together with the optional `expiresAt`.
const fn decode_system_attributes(
    created_at: Option<DateTime<Utc>>,
    modified_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
) -> Result<Option<SysAttrsSnapshot>, ClaimedDeliveryError> {
    match (created_at, modified_at, expires_at) {
        (Some(created_at), Some(modified_at), expires_at) => Ok(Some(SysAttrsSnapshot {
            created_at,
            modified_at,
            expires_at,
        })),
        (None, None, None) => Ok(None),
        (Some(_), None, _) | (None, Some(_), _) | (None, None, Some(_)) => Err(ClaimedDeliveryError::IncompleteSystemAttributes),
    }
}

/// Validate the stored change: the lifecycle and, position for position, each
/// changed instance's transition, attribute and `datasetId`.
fn decode_change(lifecycle: i16, kinds: Vec<i16>, attributes: Vec<String>, datasets: Vec<Option<String>>) -> Result<EntityChange, ClaimedDeliveryError> {
    let lifecycle = EntityLifecycleCode::try_from(lifecycle)
        .map(lifecycle_of)
        .map_err(|source| ClaimedDeliveryError::UnknownLifecycle { source })?;
    if kinds.len() != attributes.len() || kinds.len() != datasets.len() {
        return Err(ClaimedDeliveryError::ChangeArity {
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
                .map_err(|source| ClaimedDeliveryError::UnknownChangeKind { source })?;
            let attr: AttributeName = attribute.parse().map_err(|source| ClaimedDeliveryError::InvalidAttributeName { source })?;
            let dataset_id = dataset
                .map(|value| value.parse().map_err(|source| ClaimedDeliveryError::InvalidDatasetId { source }))
                .transpose()?;
            Ok(AttributeChange {
                key: AttributeInstanceKey { attr, dataset_id },
                kind,
            })
        })
        .collect::<Result<Vec<AttributeChange>, ClaimedDeliveryError>>()?;
    Ok(EntityChange::new(lifecycle, instances))
}

/// One notification event to write, and the subscriptions it is due to.
///
/// The subscriptions are named by the surrogate key `notification_deliveries`
/// references, resolved by the matcher from the loaded subscription: the insert
/// then writes every delivery of the write from two parallel arrays with no
/// lookup of its own.
#[derive(Debug)]
pub struct NewNotificationEvent<'a> {
    /// Position of this event within its write, unique across the whole batch.
    pub ordinal: WriteOrdinal,
    /// What the write did to the Entity (clause 5.8.6).
    pub change: &'a EntityChange,
    /// Entity whose change the event records.
    pub entity_id: &'a Identifier,
    /// Dictionary key of each changed instance's attribute, position for
    /// position with `change.instances`.
    pub change_attribute_name_keys: Vec<AttributeNameKey>,
    /// Entity state after the change, or the state a deletion removed.
    pub entity_snapshot: Option<&'a EntityState>,
    /// Entity state before the change, for `showChanges`.
    pub previous_snapshot: Option<&'a EntityState>,
    /// The clause 4.8 system attributes of an entity a deletion removed.
    pub sys_attrs_snapshot: Option<&'a SysAttrsSnapshot>,
    /// Subscriptions this event is due to, in matcher order.
    pub subscription_keys: Vec<SubscriptionKey>,
}

#[cfg(test)]
mod tests {
    use crate::{
        json_text::JsonText,
        notification_delivery::rows::{ClaimedDelivery, ClaimedDeliveryError, RawClaimedDeliveryRow, UnreadableClaimedDelivery},
    };
    use andromeda_context_commit::write_ordinal::WriteOrdinal;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes, reads::attribute_change::AttributeChangeKind};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::entity_change::EntityLifecycle;
    use andromeda_notification::delivery_entry::DeliveryKey;
    use chrono::Utc;
    use uuid::Uuid;

    fn row() -> RawClaimedDeliveryRow<'static> {
        RawClaimedDeliveryRow {
            delivery_key: 7,
            subscription_id: "urn:ngsi-ld:Subscription:1".to_owned(),
            attempt_count: 2,
            next_attempt_at: Utc::now(),
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

    /// A stored snapshot reads back as the entity state it was written from,
    /// and the removed entity's system attributes travel beside it.
    #[test]
    fn a_stored_snapshot_and_its_system_attributes_read_back_typed() {
        let state = EntityState::new(
            "https://example.org/entities/1".parse().expect("a valid entity URI"),
            EntityTypes::one("https://example.org/Building".parse().expect("a valid type name")),
        );
        let encoded = state.to_vec().expect("the state encodes");
        let mut raw: RawClaimedDeliveryRow<'_> = row();
        raw.entity_snapshot = Some(JsonText::new(&encoded));
        let removed_at = Utc::now();
        raw.entity_created_at = Some(removed_at);
        raw.entity_modified_at = Some(removed_at);
        let record = ClaimedDelivery::from_row(raw, &ContextTarget::Live).expect("a valid claimed delivery");
        pretty_assertions::assert_eq!(record.entity_snapshot, Some(state));
        assert!(record.previous_snapshot.is_none());
        let sys_attrs = record.sys_attrs_snapshot.expect("the stored system attributes");
        pretty_assertions::assert_eq!(sys_attrs.created_at, removed_at);
        assert!(sys_attrs.expires_at.is_none());
    }

    #[test]
    fn a_snapshot_that_is_no_entity_state_is_rejected() {
        let mut raw = row();
        raw.previous_snapshot = Some(JsonText::new(br#"[{"@id":"urn:ngsi-ld:A"}]"#));
        assert!(matches!(
            ClaimedDelivery::from_row(raw, &ContextTarget::Live).map_err(|unreadable| *unreadable),
            Err(UnreadableClaimedDelivery {
                reason: ClaimedDeliveryError::UnreadableSnapshot { .. },
                ..
            })
        ));
    }

    #[test]
    fn a_creation_instant_without_a_modification_instant_is_rejected() {
        let mut raw = row();
        raw.entity_created_at = Some(Utc::now());
        assert!(matches!(
            ClaimedDelivery::from_row(raw, &ContextTarget::Live).map_err(|unreadable| *unreadable),
            Err(UnreadableClaimedDelivery {
                reason: ClaimedDeliveryError::IncompleteSystemAttributes,
                ..
            })
        ));
    }

    #[test]
    fn a_valid_row_decodes_every_domain_value() {
        let record = ClaimedDelivery::from_row(row(), &ContextTarget::Live).expect("a valid claimed delivery");
        pretty_assertions::assert_eq!(record.subscription_id.as_str(), "urn:ngsi-ld:Subscription:1");
        pretty_assertions::assert_eq!(record.entity_id.as_str(), "https://example.org/entities/1");
        pretty_assertions::assert_eq!(record.change.lifecycle, EntityLifecycle::Modified);
        pretty_assertions::assert_eq!(record.event_ordinal, WriteOrdinal::at(1));
        pretty_assertions::assert_eq!(record.change.instances[0].key.attr.as_str(), "temperature");
        pretty_assertions::assert_eq!(record.change.instances[0].kind, AttributeChangeKind::Updated);
        assert!(record.change.instances[0].key.dataset_id.is_none());
        pretty_assertions::assert_eq!(record.change.instances[1].kind, AttributeChangeKind::Deleted);
        pretty_assertions::assert_eq!(
            record.change.instances[1].key.dataset_id.as_ref().map(Identifier::as_str),
            Some("urn:ngsi-ld:Dataset:a")
        );
        pretty_assertions::assert_eq!(record.delivery_key, DeliveryKey::from(7));
    }

    #[test]
    fn an_invalid_subscription_identifier_is_rejected() {
        let mut raw = row();
        raw.subscription_id = "not a uri".to_owned();
        assert!(matches!(
            ClaimedDelivery::from_row(raw, &ContextTarget::Live).map_err(|unreadable| *unreadable),
            Err(UnreadableClaimedDelivery {
                reason: ClaimedDeliveryError::InvalidSubscriptionId { .. },
                ..
            })
        ));
    }

    #[test]
    fn an_invalid_entity_identifier_is_rejected() {
        let mut raw = row();
        raw.entity_id = "not a uri".to_owned();
        assert!(matches!(
            ClaimedDelivery::from_row(raw, &ContextTarget::Live).map_err(|unreadable| *unreadable),
            Err(UnreadableClaimedDelivery {
                reason: ClaimedDeliveryError::InvalidEntityId { .. },
                ..
            })
        ));
    }

    #[test]
    fn a_lifecycle_number_the_registry_never_assigned_is_rejected() {
        let mut raw = row();
        raw.lifecycle = 99;
        assert!(matches!(
            ClaimedDelivery::from_row(raw, &ContextTarget::Live).map_err(|unreadable| *unreadable),
            Err(UnreadableClaimedDelivery {
                reason: ClaimedDeliveryError::UnknownLifecycle { .. },
                ..
            })
        ));
    }

    #[test]
    fn an_instance_transition_number_the_registry_never_assigned_is_rejected() {
        let mut raw = row();
        raw.change_kinds = vec![2, 99];
        assert!(matches!(
            ClaimedDelivery::from_row(raw, &ContextTarget::Live).map_err(|unreadable| *unreadable),
            Err(UnreadableClaimedDelivery {
                reason: ClaimedDeliveryError::UnknownChangeKind { .. },
                ..
            })
        ));
    }

    /// An attribute key the dictionary no longer resolves shortens the
    /// resolved names, and the instances can then no longer be paired up.
    #[test]
    fn per_instance_arrays_of_different_lengths_are_rejected() {
        let mut raw = row();
        raw.change_attributes = vec!["temperature".to_owned()];
        assert!(matches!(
            ClaimedDelivery::from_row(raw, &ContextTarget::Live).map_err(|unreadable| *unreadable),
            Err(UnreadableClaimedDelivery {
                reason: ClaimedDeliveryError::ChangeArity {
                    kinds: 2,
                    attributes: 1,
                    datasets: 2
                },
                ..
            })
        ));
    }

    #[test]
    fn a_negative_event_ordinal_is_rejected() {
        let mut raw = row();
        raw.event_ordinal = -1;
        assert!(matches!(
            ClaimedDelivery::from_row(raw, &ContextTarget::Live).map_err(|unreadable| *unreadable),
            Err(UnreadableClaimedDelivery {
                reason: ClaimedDeliveryError::InvalidEventOrdinal { .. },
                ..
            })
        ));
    }

    #[test]
    fn an_invalid_changed_attribute_is_rejected() {
        let mut raw = row();
        raw.change_attributes = vec!["bad name".to_owned(), "https://example.org/pressure".to_owned()];
        assert!(matches!(
            ClaimedDelivery::from_row(raw, &ContextTarget::Live).map_err(|unreadable| *unreadable),
            Err(UnreadableClaimedDelivery {
                reason: ClaimedDeliveryError::InvalidAttributeName { .. },
                ..
            })
        ));
    }

    #[test]
    fn the_mapper_failure_names_the_delivery_it_could_not_read() {
        let mut raw = row();
        raw.lifecycle = 99;
        let error = ClaimedDelivery::from_row(raw, &ContextTarget::Live).expect_err("an unreadable lifecycle");
        assert!(
            error.to_string().contains("notification delivery 7 "),
            "the failure must name the unreadable delivery: {error}"
        );
    }

    /// The claim leased the row before reading it, so the key and branch its
    /// acknowledgement addresses survive the failure to read it.
    #[test]
    fn an_unreadable_row_keeps_the_key_and_branch_its_acknowledgement_addresses() {
        let mut raw = row();
        raw.entity_id = "not a uri".to_owned();
        let error = ClaimedDelivery::from_row(raw, &ContextTarget::Live).expect_err("an unreadable entity identifier");
        pretty_assertions::assert_eq!(error.delivery_key, DeliveryKey::from(7));
        pretty_assertions::assert_eq!(error.target, ContextTarget::Live);
    }
}
