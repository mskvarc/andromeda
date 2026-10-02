//! Focused failures produced while evaluating or delivering notifications.

use crate::notification_invariant::NotificationInvariant;
use andromeda_entity::error::EntityError;
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_compaction::compaction_error::CompactionError;
use andromeda_ngsi_ld_render::error::RenderError;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_registration::error::RegistrationError;
use andromeda_subscription::error::SubscriptionError;

/// Failure from one notification policy operation.
#[derive(Debug, thiserror::Error)]
pub enum NotificationError {
    /// Entity access or query failed.
    #[error(transparent)]
    Entity(#[from] EntityError),
    /// Subscription access or lifecycle bookkeeping failed.
    #[error(transparent)]
    Subscription(#[from] SubscriptionError),
    /// Context Source Registration access failed.
    #[error(transparent)]
    Registration(#[from] RegistrationError),
    /// JSON-LD processing failed.
    #[error(transparent)]
    JsonLd(#[from] JsonLdError),
    /// A notified entity could not be compacted under the subscription's
    /// context, nor under the core context.
    #[error(transparent)]
    Compaction(#[from] CompactionError),
    /// A notification value could not be represented.
    #[error("notification representation failed: {0}")]
    Representation(#[source] serde_json::Error),
    /// A notified entity could not be written in the subscription's
    /// `notification.format`.
    #[error(transparent)]
    Rendering(#[from] RenderError),
    /// Durable notification persistence failed.
    #[error("notification persistence failed: {0}")]
    Persistence(#[from] PersistenceFailure),
    /// A policy invariant was violated.
    #[error("notification invariant violated: {0}")]
    Invariant(#[from] NotificationInvariant),
}
