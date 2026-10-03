//! Persisted (wire) form of a batch side-effect job stored in the
//! `pending_side_effects.payload` JSONB column.
//!
//! Every entity state a job carries is written by the entity state codec as
//! the stored expanded document an `entity_documents` row holds, and read back
//! by the same codec straight from the column's text.

use andromeda_ngsi_ld_entity::{entity_state::EntityState, reads::attribute_change::AttributeChangeKind};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{entity_change::EntityLifecycle, notification::ChangeOrigin};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

/// Failure to decode a durable pending-side-effect JSONB payload.
#[derive(Debug, Error)]
pub enum PendingSideEffectPayloadError {
    /// Stored JSON does not match the fully validated persistence shape.
    #[error("pending-side-effect row contains an invalid domain value: {0}")]
    InvalidStoredPayload(#[from] serde_json::Error),
}

/// JSON-persisted shape of a batch side-effect job.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct PersistedBatchSideEffectJob {
    /// Temporal entity recordings queued for later.
    pub temporal_entities: Vec<PersistedTemporalEntity>,
    /// Changes whose notifications the queuing commit did not already record
    /// as durable deliveries, still owed by whichever broker drains the job.
    pub change_events: Vec<PersistedChangeEvent>,
}

impl PersistedBatchSideEffectJob {
    /// Decodes the stored JSON text while preserving validation failures as a
    /// typed persistence error.
    ///
    /// # Errors
    ///
    /// Returns [`PendingSideEffectPayloadError::InvalidStoredPayload`] when the
    /// stored JSON does not match the fully validated persistence shape --
    /// an unknown member, a missing one, a value that no longer parses into
    /// its domain type, or an entity document that is no entity state.
    pub fn decode(text: &[u8]) -> Result<PersistedBatchSideEffectJob, PendingSideEffectPayloadError> {
        serde_json::from_slice(text).map_err(PendingSideEffectPayloadError::from)
    }
}

/// Single temporal entity row queued for later recording.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct PersistedTemporalEntity {
    /// Entity identifier.
    pub id: Identifier,
    /// The entity state whose observations are still owed, shared with the
    /// job it was persisted from and written as its stored expanded document.
    pub expanded: Arc<EntityState>,
}

/// Wire-shape of a change event.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct PersistedChangeEvent {
    /// Entity identifier.
    pub entity_id: Identifier,
    /// Entity type IRIs.
    pub entity_types: Vec<EntityTypeName>,
    /// What the write did to the Entity as a whole.
    pub lifecycle: EntityLifecycle,
    /// Every Attribute instance the write created, updated or deleted.
    pub instances: Vec<PersistedAttributeChange>,
    /// When the change occurred.
    pub occurred_at: DateTime<Utc>,
    /// Entity snapshot after the change, if available, shared with the change
    /// it was persisted from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_snapshot: Option<Arc<EntityState>>,
    /// Previous attribute values before the change, if available, shared with
    /// the change it was persisted from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_attributes: Option<Arc<EntityState>>,
    /// System attributes at the time of the change, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sys_attrs_snapshot: Option<PersistedSysAttrs>,
    /// Whether the change originated locally or was forwarded.
    pub origin: ChangeOrigin,
    /// Entity as the write left it, if the change carried it: what the
    /// notification renders and what its `q`, `geoQ` and `scopeQ` are
    /// evaluated against (clause 5.8.6), whenever the job is drained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_payload: Option<Arc<EntityState>>,
}

/// Wire-shape of one Attribute instance a change touched.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct PersistedAttributeChange {
    /// Expanded name of the Attribute.
    pub attribute: AttributeName,
    /// The instance's `datasetId`, absent for the default instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Identifier>,
    /// The instance's transition.
    pub kind: AttributeChangeKind,
}

/// Wire-shape of system attributes.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
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
        PendingSideEffectPayloadError,
        PersistedAttributeChange,
        PersistedBatchSideEffectJob,
        PersistedChangeEvent,
        PersistedSysAttrs,
        PersistedTemporalEntity,
    };
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes, reads::attribute_change::AttributeChangeKind};
    use andromeda_ngsi_ld_model::{entity_change::EntityLifecycle, notification::ChangeOrigin};
    use chrono::{TimeZone, Utc};
    use serde_json::{Value, json};
    use std::sync::Arc;

    /// An entity of type `Building` with no attribute.
    fn building(id: &str) -> Arc<EntityState> {
        Arc::new(EntityState::new(
            id.parse().expect("valid entity URI"),
            EntityTypes::one("https://example.org/Building".parse().expect("valid entity type")),
        ))
    }

    /// The job as the stored JSON text a queue row holds.
    fn stored(job: &PersistedBatchSideEffectJob) -> Vec<u8> {
        serde_json::to_vec(job).expect("payload serialises")
    }

    const LIFECYCLES: [EntityLifecycle; 3] = [EntityLifecycle::Created, EntityLifecycle::Modified, EntityLifecycle::Deleted];
    const ORIGINS: [ChangeOrigin; 2] = [ChangeOrigin::Local, ChangeOrigin::Forwarded];

    fn persisted_job() -> PersistedBatchSideEffectJob {
        let occurred_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid timestamp");
        let change_events = LIFECYCLES
            .into_iter()
            .flat_map(|lifecycle| {
                ORIGINS.into_iter().map(move |origin| PersistedChangeEvent {
                    entity_id: "https://example.org/entities/1".parse().expect("valid entity URI"),
                    entity_types: vec!["Building".parse().expect("valid entity type")],
                    lifecycle,
                    instances: vec![
                        PersistedAttributeChange {
                            attribute: "temperature".parse().expect("valid attribute name"),
                            dataset_id: None,
                            kind: AttributeChangeKind::Created,
                        },
                        PersistedAttributeChange {
                            attribute: "temperature".parse().expect("valid attribute name"),
                            dataset_id: Some("urn:ngsi-ld:Dataset:a".parse().expect("valid dataset URI")),
                            kind: AttributeChangeKind::Updated,
                        },
                        PersistedAttributeChange {
                            attribute: "humidity".parse().expect("valid attribute name"),
                            dataset_id: None,
                            kind: AttributeChangeKind::Deleted,
                        },
                    ],
                    occurred_at,
                    entity_snapshot: Some(building("https://example.org/entities/1")),
                    previous_attributes: Some(building("https://example.org/entities/1")),
                    sys_attrs_snapshot: Some(PersistedSysAttrs {
                        created_at: occurred_at,
                        modified_at: occurred_at,
                        expires_at: None,
                    }),
                    origin,
                    current_payload: Some(building("https://example.org/entities/1")),
                })
            })
            .collect();
        PersistedBatchSideEffectJob {
            temporal_entities: vec![
                PersistedTemporalEntity {
                    id: "urn:ngsi-ld:Building:1".parse().expect("valid entity URI"),
                    expanded: building("urn:ngsi-ld:Building:1"),
                },
                PersistedTemporalEntity {
                    id: "https://example.org/entities/1".parse().expect("valid entity URI"),
                    expanded: building("https://example.org/entities/1"),
                },
            ],
            change_events,
        }
    }

    #[test]
    fn pending_side_effect_jsonb_round_trips_every_lifecycle_transition_origin_and_uri_form() {
        let expected = persisted_job();
        let decoded = PersistedBatchSideEffectJob::decode(&stored(&expected)).expect("payload decodes");

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
        let decoded = PersistedBatchSideEffectJob::decode(&stored(&job)).expect("payload decodes");
        assert!(decoded.change_events.iter().all(|event| event.current_payload.is_none()));
    }

    #[test]
    fn pending_side_effect_jsonb_rejects_each_invalid_domain_member_without_defaulting() {
        let invalid_values = [
            (vec!["temporal_entities", "0", "id"], json!("not-a-uri")),
            (vec!["temporal_entities", "0", "expanded"], json!([{"@id": "urn:ngsi-ld:Building:1"}])),
            (
                vec!["change_events", "0", "entity_snapshot"],
                json!({"@type": ["https://example.org/Building"]}),
            ),
            (vec!["change_events", "0", "entity_id"], json!("not-a-uri")),
            (vec!["change_events", "0", "entity_types", "0"], json!("two words")),
            (vec!["change_events", "0", "instances", "0", "attribute"], json!("two words")),
            (vec!["change_events", "0", "instances", "1", "dataset_id"], json!("not-a-uri")),
            (vec!["change_events", "0", "instances", "0", "kind"], json!("unknown")),
            (vec!["change_events", "0", "lifecycle"], json!("unknown")),
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

            let text = serde_json::to_vec(&encoded).expect("the altered payload serialises");
            assert!(matches!(
                PersistedBatchSideEffectJob::decode(&text),
                Err(PendingSideEffectPayloadError::InvalidStoredPayload(_))
            ));
        }
    }

    /// The lifecycle and instance transitions are Andromeda's own vocabulary,
    /// so they persist in kebab-case.
    #[test]
    fn a_deferred_change_persists_its_transitions_in_kebab_case() {
        let encoded = serde_json::to_value(persisted_job()).expect("payload serialises");
        pretty_assertions::assert_eq!(encoded["change_events"][0]["lifecycle"], json!("created"));
        pretty_assertions::assert_eq!(encoded["change_events"][2]["lifecycle"], json!("modified"));
        pretty_assertions::assert_eq!(encoded["change_events"][0]["instances"][2]["kind"], json!("deleted"));
        assert!(encoded["change_events"][0]["instances"][0].get("dataset_id").is_none());
    }

    /// A snapshot is held in an `Arc` so a change and its persisted form share
    /// one allocation; the stored JSON is the entity's stored expanded
    /// document, with no trace of the sharing, and one snapshot shared by two
    /// members is stored in full under each and reads back as two equal states.
    #[test]
    fn an_arc_snapshot_is_persisted_as_the_stored_expanded_document() {
        let shared = building("https://example.org/entities/1");
        let document: Value = serde_json::from_slice(&shared.to_vec().expect("the state encodes")).expect("the encoding is JSON");
        let mut job = persisted_job();
        job.change_events.truncate(1);
        job.change_events[0].entity_snapshot = Some(Arc::clone(&shared));
        job.change_events[0].previous_attributes = Some(Arc::clone(&shared));
        job.change_events[0].current_payload = Some(shared);

        let encoded = serde_json::to_value(&job).expect("payload serialises");
        for member in ["entity_snapshot", "previous_attributes", "current_payload"] {
            pretty_assertions::assert_eq!(encoded["change_events"][0][member], document, "{member}");
        }
        let decoded = PersistedBatchSideEffectJob::decode(&stored(&job)).expect("payload decodes");
        pretty_assertions::assert_eq!(decoded, job);
    }

    /// A job holds entity states three containers deeper than their row, the
    /// deepest any queue holds one, so it is what the attribute nesting ceiling
    /// is computed from: a job carrying an attribute at the ceiling in every
    /// state it holds reads back, and one level deeper is past the reader.
    #[test]
    fn a_job_carrying_attributes_at_the_decodable_ceiling_reads_back_and_one_level_deeper_does_not() {
        use andromeda_ngsi_ld_representation::attribute_nesting::AttributeNestingLimit;
        use andromeda_test_fixtures::nesting_fixture::{NestingShape, nested_instance, stored_entity};

        let job_nesting = |levels: usize| {
            let stored = serde_json::to_vec(&stored_entity("urn:ngsi-ld:NestingProbe:1", nested_instance(NestingShape::ObjectChain, levels))).expect("JSON");
            let state = Arc::new(EntityState::from_slice(&stored).expect("the state decodes"));
            let mut job = persisted_job();
            job.change_events.truncate(1);
            job.temporal_entities.truncate(1);
            job.temporal_entities[0].expanded = Arc::clone(&state);
            job.change_events[0].entity_snapshot = Some(Arc::clone(&state));
            job.change_events[0].previous_attributes = Some(Arc::clone(&state));
            job.change_events[0].current_payload = Some(state);
            job
        };
        let ceiling = AttributeNestingLimit::DECODABLE_CEILING.get();
        let at_ceiling = job_nesting(ceiling);
        let decoded = PersistedBatchSideEffectJob::decode(&stored(&at_ceiling)).expect("a job at the ceiling decodes");
        pretty_assertions::assert_eq!(decoded, at_ceiling);
        assert!(matches!(
            PersistedBatchSideEffectJob::decode(&stored(&job_nesting(ceiling + 1))),
            Err(PendingSideEffectPayloadError::InvalidStoredPayload(_))
        ));
    }
}
