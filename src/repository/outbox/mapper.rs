//! Row-to-domain mapping for the notification outbox.
//!
//! Converts a persistence-shaped [`OutboxEntryRow`] into the domain
//! [`OutboxEntry`] by fallibly parsing URIs and the trigger reason.

use crate::{
    repository::outbox::rows::OutboxEntryRow,
    service::outbox::entry::{OutboxEntry, OutboxEntryId, WorkerInstanceId},
};
use andromeda_ngsi_ld_lexicon::{reference::NgsiLdReferenceError, uri::Uri};
use andromeda_ngsi_ld_model::notification::TriggerReason;
use std::str::FromStr;
use thiserror::Error;

/// Errors produced while mapping an outbox row to the domain type.
#[derive(Debug, Error)]
pub enum OutboxMapperError {
    /// Stored trigger reason is not a known variant.
    #[error("invalid trigger reason '{value}': {source}")]
    Trigger {
        /// Raw database value.
        value: String,
        /// Underlying parse error.
        #[source]
        source: strum::ParseError,
    },
    /// Stored subscription identifier is not a valid URI.
    #[error("invalid subscription URI '{value}'")]
    SubscriptionUri {
        /// Raw database value.
        value: String,
    },
    /// Stored attribute name does not satisfy the NGSI-LD name grammar.
    #[error("invalid changed attribute name: {source}")]
    AttributeName {
        /// Underlying parse error.
        #[source]
        source: NgsiLdReferenceError,
    },
    /// Stored entity identifier is not a valid URI.
    #[error("invalid entity URI '{value}'")]
    EntityUri {
        /// Raw database value.
        value: String,
    },
}

/// Convert a persistence-shaped row into the domain [`OutboxEntry`].
pub fn row_to_domain(row: OutboxEntryRow) -> Result<OutboxEntry, OutboxMapperError> {
    let trigger = TriggerReason::from_str(&row.trigger).map_err(|source| OutboxMapperError::Trigger {
        value: row.trigger.clone(),
        source,
    })?;

    let subscription_id: Uri = row.subscription_id.parse().map_err(|_| OutboxMapperError::SubscriptionUri {
        value: row.subscription_id.clone(),
    })?;

    let entity_id: Uri = row
        .entity_id
        .parse()
        .map_err(|_| OutboxMapperError::EntityUri { value: row.entity_id.clone() })?;

    let changed_attrs = row
        .changed_attrs
        .iter()
        .map(|raw| raw.parse())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| OutboxMapperError::AttributeName { source })?;

    Ok(OutboxEntry::builder()
        .id(OutboxEntryId::from(row.id))
        .subscription_id(subscription_id)
        .entity_id(entity_id)
        .trigger(trigger)
        .entity_snapshot(row.entity_snapshot)
        .previous_snapshot(row.previous_snapshot)
        .changed_attrs(changed_attrs)
        .created_at(row.created_at)
        .claimed_at(row.claimed_at)
        .claimed_by(row.claimed_by.map(WorkerInstanceId::from))
        .delivered_at(row.delivered_at)
        .delivery_error(row.delivery_error)
        .attempt_count(row.attempt_count)
        .next_attempt_at(row.next_attempt_at)
        .build())
}
