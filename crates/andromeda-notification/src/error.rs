//! Focused failures produced while evaluating or delivering notifications.

use andromeda_entity::error::EntityError;
use andromeda_jsonld::error::JsonLdError;
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
    /// A notification value could not be represented.
    #[error("notification representation failed: {detail}")]
    Representation {
        /// Sanitized diagnostic detail for internal logs.
        detail: String,
    },
    /// Durable notification persistence failed.
    #[error("notification persistence failed: {detail}")]
    Persistence {
        /// Sanitized diagnostic detail for internal logs.
        detail: String,
    },
    /// A policy invariant was violated.
    #[error("notification invariant violated: {detail}")]
    Invariant {
        /// Sanitized diagnostic detail for internal logs.
        detail: String,
    },
}

impl NotificationError {
    /// Construct a representation failure.
    pub fn representation(detail: impl Into<String>) -> Self {
        Self::Representation { detail: detail.into() }
    }

    /// Construct a persistence failure at an adapter boundary.
    pub fn persistence(detail: impl Into<String>) -> Self {
        Self::Persistence { detail: detail.into() }
    }

    /// Construct an invariant failure.
    pub fn invariant(detail: impl Into<String>) -> Self {
        Self::Invariant { detail: detail.into() }
    }
}
