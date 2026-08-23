//! Bounded subscription expiry policy operations.

use crate::{
    error::SubscriptionError,
    lifecycle::{SubscriptionService, SubscriptionVariant},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Result of one bounded expiry execution.
#[derive(Debug, Eq, PartialEq)]
pub struct SubscriptionExpiryOutcome {
    /// Identifiers transitioned to expired status.
    pub expired: Vec<Uri>,
}

/// Independently scheduled expiry operation for one subscription variant.
pub struct SubscriptionExpiry<V: SubscriptionVariant> {
    target: ContextTarget,
    service: Arc<SubscriptionService<V>>,
}

impl<V> SubscriptionExpiry<V>
where
    V: SubscriptionVariant,
{
    /// Construct expiry around the policy service that owns lifecycle semantics.
    pub const fn new(target: ContextTarget, service: Arc<SubscriptionService<V>>) -> SubscriptionExpiry<V> {
        SubscriptionExpiry { target, service }
    }

    /// Transition every due subscription at the supplied cutoff and publish each change.
    pub async fn execute(&self, cutoff: DateTime<Utc>) -> Result<SubscriptionExpiryOutcome, SubscriptionError> {
        let expired = self.service.expire_due(&self.target, cutoff).await?;
        Ok(SubscriptionExpiryOutcome { expired })
    }
}
