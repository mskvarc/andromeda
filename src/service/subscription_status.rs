//! Subscription lifecycle status transitions.
//!
//! `compute_updated_status` derives the next [`SubscriptionStatus`] from the
//! current state plus a [`StatusUpdateFields`] patch. Belongs to the service
//! layer because it is pure business logic (no I/O, no SQL).

use crate::repository::subscription_store::params::StatusUpdateFields;
use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
use chrono::Utc;

/// Derive the new subscription status based on the current status and the update fields provided.
pub fn compute_updated_status(current_status: SubscriptionStatus, fields: &StatusUpdateFields) -> SubscriptionStatus {
    let now = Utc::now();

    if let Some(Some(ea)) = fields.expires_at
        && ea <= now
    {
        return current_status;
    }

    match (fields.is_active, fields.expires_at.is_some()) {
        (Some(true), false) if current_status != SubscriptionStatus::Expired => SubscriptionStatus::Active,
        (Some(false), false) if current_status != SubscriptionStatus::Expired => SubscriptionStatus::Paused,
        (Some(true), true) => SubscriptionStatus::Active,
        (Some(false), true) => SubscriptionStatus::Paused,
        (None, true) if current_status == SubscriptionStatus::Expired => SubscriptionStatus::Active,
        (Some(true), false) | (Some(false), false) => current_status,
        (None, true) | (None, false) => current_status,
    }
}

#[cfg(test)]
mod tests {
    use super::{StatusUpdateFields, compute_updated_status};
    use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
    use chrono::{Duration, Utc};

    #[test]
    fn active_with_is_active_true_stays_active() {
        let status = compute_updated_status(
            SubscriptionStatus::Active,
            &StatusUpdateFields {
                is_active: Some(true),
                expires_at: None,
            },
        );
        pretty_assertions::assert_eq!(status, SubscriptionStatus::Active);
    }

    #[test]
    fn active_with_is_active_false_becomes_paused() {
        let status = compute_updated_status(
            SubscriptionStatus::Active,
            &StatusUpdateFields {
                is_active: Some(false),
                expires_at: None,
            },
        );
        pretty_assertions::assert_eq!(status, SubscriptionStatus::Paused);
    }

    #[test]
    fn expired_with_is_active_true_no_expiry_change_stays_expired() {
        let status = compute_updated_status(
            SubscriptionStatus::Expired,
            &StatusUpdateFields {
                is_active: Some(true),
                expires_at: None,
            },
        );
        pretty_assertions::assert_eq!(status, SubscriptionStatus::Expired);
    }

    #[test]
    fn expired_with_new_future_expires_at_reactivates() {
        let future = Utc::now() + Duration::hours(1);
        let status = compute_updated_status(
            SubscriptionStatus::Expired,
            &StatusUpdateFields {
                is_active: None,
                expires_at: Some(Some(future)),
            },
        );
        pretty_assertions::assert_eq!(status, SubscriptionStatus::Active);
    }

    #[test]
    fn expired_with_past_expires_at_stays_expired() {
        let past = Utc::now() - Duration::hours(1);
        let status = compute_updated_status(
            SubscriptionStatus::Expired,
            &StatusUpdateFields {
                is_active: None,
                expires_at: Some(Some(past)),
            },
        );
        pretty_assertions::assert_eq!(status, SubscriptionStatus::Expired);
    }

    #[test]
    fn active_with_no_changes_stays_active() {
        let status = compute_updated_status(
            SubscriptionStatus::Active,
            &StatusUpdateFields {
                is_active: None,
                expires_at: None,
            },
        );
        pretty_assertions::assert_eq!(status, SubscriptionStatus::Active);
    }

    #[test]
    fn paused_with_is_active_true_and_new_expiry_becomes_active() {
        let future = Utc::now() + Duration::hours(1);
        let status = compute_updated_status(
            SubscriptionStatus::Paused,
            &StatusUpdateFields {
                is_active: Some(true),
                expires_at: Some(Some(future)),
            },
        );
        pretty_assertions::assert_eq!(status, SubscriptionStatus::Active);
    }
}
