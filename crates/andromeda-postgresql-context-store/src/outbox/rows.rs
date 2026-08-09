//! Validated persistence records for the notification outbox table.

use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    reference::NgsiLdReferenceError,
    uri::{Uri, UriParseError},
};
use andromeda_ngsi_ld_model::notification::NotificationTrigger;
use chrono::{DateTime, Utc};
use serde_json::Value;
use strum::ParseError;
use thiserror::Error;

/// A validated pending notification read from the outbox.
#[derive(Debug)]
pub struct OutboxRecord {
    /// Auto-generated row identifier.
    pub id: i64,
    /// Subscription that matched the entity change.
    pub subscription_id: Uri,
    /// Entity that triggered the notification.
    pub entity_id: Uri,
    /// Mutation trigger reason.
    pub trigger: NotificationTrigger,
    /// Expanded entity snapshot at the time of the mutation.
    pub entity_snapshot: Option<Value>,
    /// Entity state before the mutation.
    pub previous_snapshot: Option<Value>,
    /// Attribute names changed by the mutation.
    pub changed_attrs: Vec<AttributeName>,
    /// Time at which the row was created.
    pub created_at: DateTime<Utc>,
    /// Number of delivery attempts made so far.
    pub attempt_count: i32,
    /// Earliest time at which another delivery may be attempted.
    pub next_attempt_at: DateTime<Utc>,
}

/// Why a stored outbox row could not be converted into its domain record.
#[derive(Debug, Error)]
pub enum OutboxRowError {
    /// The subscription identifier is not a URI permitted by clause 4.6.2.
    #[error("outbox row contains invalid subscription URI `{value}`")]
    InvalidSubscriptionId {
        /// Invalid stored value.
        value: String,
        /// URI validation failure.
        #[source]
        source: UriParseError,
    },
    /// The entity identifier is not a URI permitted by clause 4.6.2.
    #[error("outbox row contains invalid entity URI `{value}`")]
    InvalidEntityId {
        /// Invalid stored value.
        value: String,
        /// URI validation failure.
        #[source]
        source: UriParseError,
    },
    /// The trigger token is not one of the clause-5.3.3 values.
    #[error("outbox row contains invalid notification trigger `{value}`")]
    InvalidTrigger {
        /// Invalid stored value.
        value: String,
        /// Trigger validation failure.
        #[source]
        source: ParseError,
    },
    /// One changed attribute is not a valid NGSI-LD attribute name.
    #[error("outbox row contains invalid changed attribute `{value}`")]
    InvalidAttributeName {
        /// Invalid stored value.
        value: String,
        /// Attribute-name validation failure.
        #[source]
        source: NgsiLdReferenceError,
    },
}

/// SQL-decode shape. It is crate-private so unvalidated database text cannot
/// cross the persistence boundary.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct RawOutboxRow {
    /// Database-generated outbox identifier.
    id: i64,
    /// Raw subscription identifier decoded by the row mapper.
    subscription_id: String,
    /// Raw entity identifier decoded by the row mapper.
    entity_id: String,
    /// Raw notification trigger decoded by the row mapper.
    trigger: String,
    /// Current entity snapshot stored as JSONB.
    entity_snapshot: Option<Value>,
    /// Previous entity snapshot stored as JSONB.
    previous_snapshot: Option<Value>,
    /// Raw changed attribute names decoded by the row mapper.
    changed_attrs: Vec<String>,
    /// Time at which the outbox entry was created.
    created_at: DateTime<Utc>,
    /// Number of delivery attempts already made.
    attempt_count: i32,
    /// Earliest time at which delivery may next be attempted.
    next_attempt_at: DateTime<Utc>,
}

// Validation spans four independent domain types, so no derive can express
// this conversion from the SQL decode shape.
impl TryFrom<RawOutboxRow> for OutboxRecord {
    type Error = OutboxRowError;

    fn try_from(row: RawOutboxRow) -> Result<OutboxRecord, OutboxRowError> {
        let subscription_id = row.subscription_id.parse().map_err(|source| OutboxRowError::InvalidSubscriptionId {
            value: row.subscription_id,
            source,
        })?;
        let entity_id = row
            .entity_id
            .parse()
            .map_err(|source| OutboxRowError::InvalidEntityId { value: row.entity_id, source })?;
        let trigger = row
            .trigger
            .parse()
            .map_err(|source| OutboxRowError::InvalidTrigger { value: row.trigger, source })?;
        let changed_attrs = row
            .changed_attrs
            .into_iter()
            .map(|value| value.parse().map_err(|source| OutboxRowError::InvalidAttributeName { value, source }))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(OutboxRecord {
            id: row.id,
            subscription_id,
            entity_id,
            trigger,
            entity_snapshot: row.entity_snapshot,
            previous_snapshot: row.previous_snapshot,
            changed_attrs,
            created_at: row.created_at,
            attempt_count: row.attempt_count,
            next_attempt_at: row.next_attempt_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::outbox::rows::{OutboxRecord, OutboxRowError, RawOutboxRow};
    use chrono::Utc;

    fn row() -> RawOutboxRow {
        RawOutboxRow {
            id: 7,
            subscription_id: "urn:ngsi-ld:Subscription:1".to_string(),
            entity_id: "https://example.org/entities/1".to_string(),
            trigger: "entityUpdated".to_string(),
            entity_snapshot: None,
            previous_snapshot: None,
            changed_attrs: vec!["temperature".to_string(), "https://example.org/pressure".to_string()],
            created_at: Utc::now(),
            attempt_count: 2,
            next_attempt_at: Utc::now(),
        }
    }

    #[test]
    fn valid_row_decodes_every_domain_value() {
        let record = OutboxRecord::try_from(row()).expect("valid row");
        pretty_assertions::assert_eq!(record.subscription_id.as_str(), "urn:ngsi-ld:Subscription:1");
        pretty_assertions::assert_eq!(record.entity_id.as_str(), "https://example.org/entities/1");
        pretty_assertions::assert_eq!(record.trigger.to_string(), "entityUpdated");
        pretty_assertions::assert_eq!(record.changed_attrs[0].as_str(), "temperature");
    }

    #[test]
    fn invalid_subscription_id_is_rejected() {
        let mut raw = row();
        raw.subscription_id = "not-a-uri".to_string();
        assert!(matches!(OutboxRecord::try_from(raw), Err(OutboxRowError::InvalidSubscriptionId { .. })));
    }

    #[test]
    fn invalid_entity_id_is_rejected() {
        let mut raw = row();
        raw.entity_id = "not-a-uri".to_string();
        assert!(matches!(OutboxRecord::try_from(raw), Err(OutboxRowError::InvalidEntityId { .. })));
    }

    #[test]
    fn invalid_trigger_is_rejected() {
        let mut raw = row();
        raw.trigger = "unknown".to_string();
        assert!(matches!(OutboxRecord::try_from(raw), Err(OutboxRowError::InvalidTrigger { .. })));
    }

    #[test]
    fn invalid_changed_attribute_is_rejected() {
        let mut raw = row();
        raw.changed_attrs = vec!["bad name".to_string()];
        assert!(matches!(OutboxRecord::try_from(raw), Err(OutboxRowError::InvalidAttributeName { .. })));
    }
}
