//! Persisted (wire) form of a [`BatchSideEffectJob`] stored in the
//! `pending_side_effects.payload` JSONB column.
//!
//! Domain types like [`ChangeEvent`] are not `Serialize`/`Deserialize`
//! themselves, so this module defines a parallel shape using plain field
//! types and converts to/from the domain form at the repository boundary.

use crate::{core::error::AndromedaError, service::entity::side_effect::BatchSideEffectJob};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::{ChangeEvent, ChangeOrigin, SysAttrsSnapshot, TriggerReason};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{str::FromStr, sync::Arc};

/// JSON-persisted shape of a [`BatchSideEffectJob`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedBatchSideEffectJob {
    pub temporal_entities: Vec<PersistedTemporalEntity>,
    pub change_events: Vec<PersistedChangeEvent>,
}

/// Single temporal entity row queued for later recording.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedTemporalEntity {
    pub id: String,
    pub expanded: Value,
}

/// Wire-shape of [`ChangeEvent`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedChangeEvent {
    pub entity_id: String,
    pub entity_types: Vec<String>,
    pub trigger_reason: String,
    pub changed_attributes: Vec<String>,
    pub occurred_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_snapshot: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_attributes: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sys_attrs_snapshot: Option<PersistedSysAttrs>,
    pub origin: String,
}

/// Wire-shape of [`SysAttrsSnapshot`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSysAttrs {
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

impl From<&BatchSideEffectJob> for PersistedBatchSideEffectJob {
    fn from(job: &BatchSideEffectJob) -> PersistedBatchSideEffectJob {
        PersistedBatchSideEffectJob {
            temporal_entities: job
                .temporal_entities
                .iter()
                .map(|(id, expanded)| PersistedTemporalEntity {
                    id: id.as_ref().to_owned(),
                    expanded: (**expanded).clone(),
                })
                .collect(),
            change_events: job.change_events.iter().map(PersistedChangeEvent::from).collect(),
        }
    }
}

impl From<&ChangeEvent> for PersistedChangeEvent {
    fn from(event: &ChangeEvent) -> PersistedChangeEvent {
        PersistedChangeEvent {
            entity_id: event.entity_id.to_string(),
            entity_types: event.entity_types.iter().map(ToString::to_string).collect(),
            trigger_reason: event.trigger_reason.to_string(),
            changed_attributes: event.changed_attributes.iter().map(ToString::to_string).collect(),
            occurred_at: event.occurred_at,
            entity_snapshot: event.entity_snapshot.as_ref().map(|arc| (**arc).clone()),
            previous_attributes: event.previous_attributes.as_ref().map(|arc| (**arc).clone()),
            sys_attrs_snapshot: event.sys_attrs_snapshot.as_ref().map(PersistedSysAttrs::from),
            origin: match event.origin {
                ChangeOrigin::Local => "local".to_string(),
                ChangeOrigin::Forwarded => "forwarded".to_string(),
            },
        }
    }
}

impl From<&SysAttrsSnapshot> for PersistedSysAttrs {
    fn from(snapshot: &SysAttrsSnapshot) -> PersistedSysAttrs {
        PersistedSysAttrs {
            created_at: snapshot.created_at,
            modified_at: snapshot.modified_at,
            expires_at: snapshot.expires_at,
        }
    }
}

impl TryFrom<PersistedBatchSideEffectJob> for BatchSideEffectJob {
    type Error = AndromedaError;

    fn try_from(persisted: PersistedBatchSideEffectJob) -> Result<BatchSideEffectJob, AndromedaError> {
        let temporal_entities = persisted
            .temporal_entities
            .into_iter()
            .map(|t| (Arc::<str>::from(t.id.as_str()), Arc::new(t.expanded)))
            .collect();
        let change_events = persisted
            .change_events
            .into_iter()
            .map(ChangeEvent::try_from)
            .collect::<Result<Vec<_>, AndromedaError>>()?;
        Ok(BatchSideEffectJob {
            temporal_entities,
            change_events,
        })
    }
}

impl TryFrom<PersistedChangeEvent> for ChangeEvent {
    type Error = AndromedaError;

    fn try_from(persisted: PersistedChangeEvent) -> Result<ChangeEvent, AndromedaError> {
        let entity_id: Uri = persisted
            .entity_id
            .parse()
            .map_err(|err| AndromedaError::internal(format!("invalid entity_id in queued change event: {err}")))?;
        let trigger_reason = TriggerReason::from_str(&persisted.trigger_reason)
            .map_err(|err| AndromedaError::internal(format!("invalid trigger_reason in queued change event: {err}")))?;
        let origin = match persisted.origin.as_str() {
            "local" => ChangeOrigin::Local,
            "forwarded" => ChangeOrigin::Forwarded,
            other => return Err(AndromedaError::internal(format!("invalid origin in queued change event: {other}"))),
        };

        // Names that no longer satisfy the NGSI-LD grammar cannot name a type
        // or an attribute, so a queued event carrying one is rejected outright
        // rather than delivered with the offending member silently dropped.
        let entity_types = persisted
            .entity_types
            .iter()
            .map(|raw| raw.parse())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| AndromedaError::internal(format!("invalid entity type in queued change event: {err}")))?;
        let changed_attributes = persisted
            .changed_attributes
            .iter()
            .map(|raw| raw.parse())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| AndromedaError::internal(format!("invalid attribute name in queued change event: {err}")))?;

        // `current_payload` is intentionally not persisted: the durable queue path
        // is opt-in/async and consumers (spatial sync) fall back to fetching when
        // it is absent. Persisting it would double-store the expanded entity,
        // which `temporal_entities` already holds.
        Ok(ChangeEvent::builder()
            .entity_id(entity_id)
            .entity_types(entity_types)
            .trigger_reason(trigger_reason)
            .changed_attributes(changed_attributes)
            .occurred_at(persisted.occurred_at)
            .entity_snapshot(persisted.entity_snapshot.map(Arc::new))
            .previous_attributes(persisted.previous_attributes.map(Arc::new))
            .sys_attrs_snapshot(persisted.sys_attrs_snapshot.map(SysAttrsSnapshot::from))
            .origin(origin)
            .build())
    }
}

impl From<PersistedSysAttrs> for SysAttrsSnapshot {
    fn from(persisted: PersistedSysAttrs) -> SysAttrsSnapshot {
        SysAttrsSnapshot {
            created_at: persisted.created_at,
            modified_at: persisted.modified_at,
            expires_at: persisted.expires_at,
        }
    }
}
