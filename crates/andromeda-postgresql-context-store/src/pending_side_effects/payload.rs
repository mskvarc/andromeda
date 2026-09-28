//! Persisted (wire) form of a batch side-effect job stored in the
//! `pending_side_effects.payload` JSONB column.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{entity_change::EntityLifecycle, expanded::attribute_change::AttributeChangeKind, notification::ChangeOrigin};
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Failure to decode a durable pending-side-effect JSONB payload.
#[derive(Debug, Error)]
pub enum PendingSideEffectPayloadError {
    /// Stored JSON does not match the fully validated persistence shape.
    #[error("pending-side-effect row contains an invalid domain value: {0}")]
    InvalidStoredPayload(#[from] serde_json::Error),
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
    /// What the write did to the Entity as a whole.
    pub lifecycle: EntityLifecycle,
    /// Every Attribute instance the write created, updated or deleted.
    pub instances: Vec<PersistedAttributeChange>,
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
    pub current_payload: Option<Value>,
}

/// Wire-shape of one Attribute instance a change touched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedAttributeChange {
    /// Expanded name of the Attribute.
    pub attribute: AttributeName,
    /// The instance's `datasetId`, absent for the default instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Uri>,
    /// The instance's transition.
    pub kind: AttributeChangeKind,
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
        PendingSideEffectPayloadError,
        PersistedAttributeChange,
        PersistedBatchSideEffectJob,
        PersistedChangeEvent,
        PersistedSysAttrs,
        PersistedTemporalEntity,
    };
    use andromeda_ngsi_ld_model::{entity_change::EntityLifecycle, expanded::attribute_change::AttributeChangeKind, notification::ChangeOrigin};
    use chrono::{TimeZone, Utc};
    use serde_json::{Value, json};

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
                    entity_snapshot: Some(json!({"id": "https://example.org/entities/1"})),
                    previous_attributes: Some(json!({"temperature": 20})),
                    sys_attrs_snapshot: Some(PersistedSysAttrs {
                        created_at: occurred_at,
                        modified_at: occurred_at,
                        expires_at: None,
                    }),
                    origin,
                    current_payload: Some(json!({"id": "https://example.org/entities/1", "temperature": 21})),
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
    fn pending_side_effect_jsonb_round_trips_every_lifecycle_transition_origin_and_uri_form() {
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

            assert!(matches!(
                PersistedBatchSideEffectJob::decode(&encoded),
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
}
