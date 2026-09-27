//! Persisted (wire) form of a batch side-effect job stored in the
//! `pending_side_effects.payload` JSONB column.
//!
//! One write's change events all carry the entity that write left behind, so
//! a job stores each such entity once: the first event carrying it stores the
//! document, and every later event of the job carrying the same one refers
//! back to that event ([`PersistedEntityDocument`]).

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::{ChangeOrigin, NotificationTrigger};
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, hash_map::Entry},
    marker::PhantomData,
    sync::Arc,
};
use thiserror::Error;

/// Failure to decode a durable pending-side-effect JSONB payload.
#[derive(Debug, Error)]
pub enum PendingSideEffectPayloadError {
    /// Stored JSON does not match the fully validated persistence shape.
    #[error("pending-side-effect row contains an invalid domain value: {0}")]
    InvalidStoredPayload(#[from] serde_json::Error),
    /// A change event refers to the entity document of an event that does
    /// not precede it in the job, or that carries none.
    #[error("pending-side-effect change event {event} refers to the entity document of change event {referenced}, which holds none before it")]
    DanglingEntityDocument {
        /// Position of the referring event in `change_events`.
        event: usize,
        /// Position the event refers to.
        referenced: usize,
    },
}

/// JSON-persisted shape of a batch side-effect job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedBatchSideEffectJob {
    /// Temporal entity recordings queued for later.
    pub temporal_entities: Vec<PersistedTemporalEntity>,
    /// Changes whose notifications the queuing commit did not already record
    /// as durable deliveries, still owed by whichever broker drains the job.
    pub change_events: Vec<PersistedChangeEvent>,
}

impl PersistedBatchSideEffectJob {
    /// Decodes a JSONB value while preserving validation failures as a typed
    /// persistence error.
    ///
    /// # Errors
    ///
    /// Returns [`PendingSideEffectPayloadError::InvalidStoredPayload`] when the
    /// stored JSON does not match the fully validated persistence shape --
    /// an unknown member, a missing one, or a value that no longer parses into
    /// its domain type.
    pub fn decode(value: &Value) -> Result<PersistedBatchSideEffectJob, PendingSideEffectPayloadError> {
        deserialize_exact(value).map_err(PendingSideEffectPayloadError::from)
    }
}

/// Single temporal entity row queued for later recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedTemporalEntity {
    /// Entity identifier.
    pub id: Uri,
    /// The expanded JSON-LD payload.
    pub expanded: Value,
}

/// Wire-shape of a change event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedChangeEvent {
    /// Entity identifier.
    pub entity_id: Uri,
    /// Entity type IRIs.
    pub entity_types: Vec<EntityTypeName>,
    /// The NGSI-LD notification trigger reason.
    pub trigger_reason: NotificationTrigger,
    /// Attribute IRIs that changed.
    pub changed_attributes: Vec<AttributeName>,
    /// Dataset discriminator of the changed Attribute instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_dataset_id: Option<Uri>,
    /// When the change occurred.
    pub occurred_at: DateTime<Utc>,
    /// Entity snapshot after the change, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_snapshot: Option<Value>,
    /// Previous attribute values before the change, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_attributes: Option<Value>,
    /// System attributes at the time of the change, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sys_attrs_snapshot: Option<PersistedSysAttrs>,
    /// Whether the change originated locally or was forwarded.
    pub origin: ChangeOrigin,
    /// Entity as the write left it, if the change carried it: what the
    /// notification renders and what its `q`, `geoQ` and `scopeQ` are
    /// evaluated against (clause 5.8.6), whenever the job is drained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_payload: Option<PersistedEntityDocument>,
}

/// One change event's entity document, stored once per job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PersistedEntityDocument {
    /// The document itself, stored by the first event of the job carrying it.
    Stored(Value),
    /// The same document as the event at this position in `change_events`,
    /// which precedes the referring event.
    AsEvent(usize),
}

/// Assigns each change event of one job its [`PersistedEntityDocument`],
/// storing every distinct document once.
///
/// Documents are told apart by identity, not by content: the events one write
/// produces share the very allocation of the entity it left behind, and that
/// sharing is what a job preserves. Two equal documents from separate
/// allocations are stored twice, which costs space but never correctness.
///
/// The table borrows every document it has seen for `'a`, so no allocation it
/// remembers can be freed and reused by another document while it is in use.
#[derive(Debug, Default)]
pub struct EntityDocumentSharing<'a> {
    /// Position of the event that stored each document, by its allocation.
    stored_at: HashMap<*const Value, usize>,
    /// The documents the table has seen, borrowed for as long as it is used.
    documents: PhantomData<&'a Value>,
}

impl<'a> EntityDocumentSharing<'a> {
    /// The persisted form of `document`, carried by the change event at
    /// `event` in the job: a reference to the earlier event that stored the
    /// same document, or the document itself when no event stored it yet.
    pub fn persist(&mut self, event: usize, document: &'a Arc<Value>) -> PersistedEntityDocument {
        match self.stored_at.entry(Arc::as_ptr(document)) {
            Entry::Occupied(earlier) => PersistedEntityDocument::AsEvent(*earlier.get()),
            Entry::Vacant(first) => {
                first.insert(event);
                PersistedEntityDocument::Stored(Value::clone(document)) // clone: the persisted job owns the one stored copy.
            }
        }
    }
}

/// Resolve each change event's persisted entity document, in `change_events`
/// order, sharing one allocation among the events that refer to the same one.
///
/// # Errors
///
/// Returns [`PendingSideEffectPayloadError::DanglingEntityDocument`] when an
/// event refers to one that does not precede it or that carries no document.
pub fn resolve_entity_documents(
    documents: impl IntoIterator<Item = Option<PersistedEntityDocument>>,
) -> Result<Vec<Option<Arc<Value>>>, PendingSideEffectPayloadError> {
    let mut resolved: Vec<Option<Arc<Value>>> = Vec::new();
    for (event, document) in documents.into_iter().enumerate() {
        let shared = match document {
            None => None,
            Some(PersistedEntityDocument::Stored(value)) => Some(Arc::new(value)),
            Some(PersistedEntityDocument::AsEvent(referenced)) => Some(
                resolved
                    .get(referenced)
                    .and_then(Option::as_ref)
                    .map(Arc::clone) // clone: Arc ref-count bump
                    .ok_or(PendingSideEffectPayloadError::DanglingEntityDocument { event, referenced })?,
            ),
        };
        resolved.push(shared);
    }
    Ok(resolved)
}

/// Wire-shape of system attributes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedSysAttrs {
    /// Entity creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Entity last-modification timestamp.
    pub modified_at: DateTime<Utc>,
    /// Entity expiration timestamp, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use crate::pending_side_effects::payload::{
        EntityDocumentSharing,
        PendingSideEffectPayloadError,
        PersistedBatchSideEffectJob,
        PersistedChangeEvent,
        PersistedEntityDocument,
        PersistedSysAttrs,
        PersistedTemporalEntity,
        resolve_entity_documents,
    };
    use andromeda_ngsi_ld_model::notification::{ChangeOrigin, NotificationTrigger};
    use chrono::{TimeZone, Utc};
    use serde_json::{Value, json};
    use std::sync::Arc;

    const TRIGGERS: [NotificationTrigger; 6] = [
        NotificationTrigger::EntityCreated,
        NotificationTrigger::EntityUpdated,
        NotificationTrigger::EntityDeleted,
        NotificationTrigger::AttributeCreated,
        NotificationTrigger::AttributeUpdated,
        NotificationTrigger::AttributeDeleted,
    ];
    const ORIGINS: [ChangeOrigin; 2] = [ChangeOrigin::Local, ChangeOrigin::Forwarded];

    fn persisted_job() -> PersistedBatchSideEffectJob {
        let occurred_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid timestamp");
        let change_events = TRIGGERS
            .into_iter()
            .flat_map(|trigger_reason| {
                ORIGINS.into_iter().map(move |origin| PersistedChangeEvent {
                    entity_id: "https://example.org/entities/1".parse().expect("valid entity URI"),
                    entity_types: vec!["Building".parse().expect("valid entity type")],
                    trigger_reason,
                    changed_attributes: vec!["temperature".parse().expect("valid attribute name")],
                    changed_dataset_id: None,
                    occurred_at,
                    entity_snapshot: Some(json!({"id": "https://example.org/entities/1"})),
                    previous_attributes: Some(json!({"temperature": 20})),
                    sys_attrs_snapshot: Some(PersistedSysAttrs {
                        created_at: occurred_at,
                        modified_at: occurred_at,
                        expires_at: None,
                    }),
                    origin,
                    current_payload: Some(PersistedEntityDocument::Stored(
                        json!({"id": "https://example.org/entities/1", "temperature": 21}),
                    )),
                })
            })
            .collect();
        PersistedBatchSideEffectJob {
            temporal_entities: vec![
                PersistedTemporalEntity {
                    id: "urn:ngsi-ld:Building:1".parse().expect("valid entity URI"),
                    expanded: json!({"@id": "urn:ngsi-ld:Building:1"}),
                },
                PersistedTemporalEntity {
                    id: "https://example.org/entities/1".parse().expect("valid entity URI"),
                    expanded: json!({"@id": "https://example.org/entities/1"}),
                },
            ],
            change_events,
        }
    }

    #[test]
    fn pending_side_effect_jsonb_round_trips_every_trigger_origin_and_uri_form() {
        let expected = persisted_job();
        let encoded = serde_json::to_value(&expected).expect("payload serialises");
        let decoded = PersistedBatchSideEffectJob::decode(&encoded).expect("payload decodes");

        pretty_assertions::assert_eq!(decoded, expected);
    }

    /// A deletion carries no resulting entity, so its event is stored without a
    /// `current_payload` member and reads back without one.
    #[test]
    fn a_change_without_a_resulting_entity_is_stored_without_a_current_payload() {
        let mut job = persisted_job();
        for event in &mut job.change_events {
            event.current_payload = None;
        }
        let encoded = serde_json::to_value(&job).expect("payload serialises");
        assert!(encoded["change_events"][0].get("current_payload").is_none(), "{encoded}");
        let decoded = PersistedBatchSideEffectJob::decode(&encoded).expect("payload decodes");
        assert!(decoded.change_events.iter().all(|event| event.current_payload.is_none()));
    }

    #[test]
    fn pending_side_effect_jsonb_rejects_each_invalid_domain_member_without_defaulting() {
        let invalid_values = [
            (vec!["temporal_entities", "0", "id"], json!("not-a-uri")),
            (vec!["change_events", "0", "entity_id"], json!("not-a-uri")),
            (vec!["change_events", "0", "entity_types", "0"], json!("two words")),
            (vec!["change_events", "0", "changed_attributes", "0"], json!("two words")),
            (vec!["change_events", "0", "trigger_reason"], json!("unknown")),
            (vec!["change_events", "0", "origin"], json!("unknown")),
        ];

        for (path, replacement) in invalid_values {
            let mut encoded = serde_json::to_value(persisted_job()).expect("payload serialises");
            let mut target = &mut encoded;
            for segment in &path[..path.len() - 1] {
                target = match target {
                    Value::Array(values) => &mut values[segment.parse::<usize>().expect("array index")],
                    Value::Object(values) => values.get_mut(*segment).expect("object member"),
                    Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => panic!("fixture path reaches a container"),
                };
            }
            let final_segment = path.last().expect("non-empty path");
            match target {
                Value::Array(values) => values[final_segment.parse::<usize>().expect("array index")] = replacement,
                Value::Object(values) => {
                    values.insert((*final_segment).to_string(), replacement);
                }
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => panic!("fixture path reaches a container"),
            }

            assert!(matches!(
                PersistedBatchSideEffectJob::decode(&encoded),
                Err(PendingSideEffectPayloadError::InvalidStoredPayload(_))
            ));
        }
    }

    /// The events one write produces share the entity it left behind, so the
    /// job stores that entity once and every later event refers back to the
    /// first; a separately allocated document is stored on its own.
    #[test]
    fn a_document_shared_by_several_events_is_stored_once() {
        let left_behind = Arc::new(json!([{"@id": "urn:ngsi-ld:Building:1"}]));
        let other = Arc::new(json!([{"@id": "urn:ngsi-ld:Building:2"}]));
        let mut sharing = EntityDocumentSharing::default();
        let persisted = vec![
            sharing.persist(0, &left_behind),
            sharing.persist(1, &left_behind),
            sharing.persist(2, &other),
            sharing.persist(3, &left_behind),
        ];
        pretty_assertions::assert_eq!(
            persisted,
            vec![
                PersistedEntityDocument::Stored(Value::clone(&left_behind)),
                PersistedEntityDocument::AsEvent(0),
                PersistedEntityDocument::Stored(Value::clone(&other)),
                PersistedEntityDocument::AsEvent(0),
            ]
        );
    }

    /// Resolution hands every event referring to one stored document the same
    /// allocation, and an event without a document stays without one.
    #[test]
    fn resolved_references_share_the_stored_document() -> Result<(), PendingSideEffectPayloadError> {
        let resolved = resolve_entity_documents([
            Some(PersistedEntityDocument::Stored(json!({"@id": "urn:ngsi-ld:Building:1"}))),
            None,
            Some(PersistedEntityDocument::AsEvent(0)),
        ])?;
        pretty_assertions::assert_eq!(resolved.len(), 3);
        assert!(resolved[1].is_none());
        match (&resolved[0], &resolved[2]) {
            (Some(stored), Some(referring)) => assert!(Arc::ptr_eq(stored, referring), "the referring event shares the stored allocation"),
            (first, third) => panic!("both events carry the document, got {first:?} and {third:?}"),
        }
        Ok(())
    }

    /// A reference to a later event, to itself, or to an event that carries
    /// no document names nothing the job holds, so the payload is refused.
    #[test]
    fn a_reference_to_no_earlier_document_is_refused() {
        for (documents, event, referenced) in [
            (
                vec![Some(PersistedEntityDocument::AsEvent(1)), Some(PersistedEntityDocument::Stored(json!({})))],
                0,
                1,
            ),
            (vec![Some(PersistedEntityDocument::AsEvent(0))], 0, 0),
            (vec![None, Some(PersistedEntityDocument::AsEvent(0))], 1, 0),
        ] {
            let refused = resolve_entity_documents(documents);
            assert!(
                matches!(refused, Err(PendingSideEffectPayloadError::DanglingEntityDocument { event: e, referenced: r }) if e == event && r == referenced),
                "{refused:?}"
            );
        }
    }

    /// The reference is spelled in this broker's own kebab-case vocabulary.
    #[test]
    fn a_shared_document_is_persisted_as_a_kebab_case_reference() {
        let encoded = serde_json::to_value(PersistedEntityDocument::AsEvent(2)).expect("reference serialises");
        pretty_assertions::assert_eq!(encoded, json!({"as-event": 2}));
        let encoded = serde_json::to_value(PersistedEntityDocument::Stored(json!({"@id": "urn:ngsi-ld:Building:1"}))).expect("document serialises");
        pretty_assertions::assert_eq!(encoded, json!({"stored": {"@id": "urn:ngsi-ld:Building:1"}}));
    }
}
