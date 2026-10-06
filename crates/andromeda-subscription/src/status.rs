//! Subscription lifecycle status transitions.
//!
//! `compute_updated_status` derives the next [`SubscriptionStatus`] from the
//! current state plus a [`StatusUpdateFields`] patch. Belongs to the service
//! layer because it is pure business logic (no I/O, no SQL).

use crate::store::StatusUpdateFields;
use andromeda_ngsi_ld_model::{clearable::Clearable, subscription::SubscriptionStatus, subscription_request::SubscriptionRequest};
use chrono::Utc;

/// The status members a create body or update fragment names: `isActive` and
/// `expiresAt` when it writes them, and nothing for one it leaves out.
///
/// A deleted `isActive` (clause 5.5.8) is the default Table 5.2.12-1 gives it,
/// `true`. A deleted `expiresAt` is a member that "is not present" in the
/// terms of the clause 5.8.2.4 status rules, which is also what a fragment that
/// does not mention it is.
#[must_use]
pub const fn requested_status_change(request: &SubscriptionRequest) -> StatusUpdateFields {
    StatusUpdateFields {
        is_active: match &request.is_active {
            Clearable::Set(is_active) => Some(*is_active),
            Clearable::Cleared | Clearable::NgsiLdNull => Some(true),
            Clearable::Absent => None,
        },
        expires_at: match &request.expires_at {
            Clearable::Set(expires_at) => Some(*expires_at),
            Clearable::Absent | Clearable::Cleared | Clearable::NgsiLdNull => None,
        },
    }
}

/// Derive the new subscription status based on the current status and the update fields provided.
#[must_use]
pub fn compute_updated_status(current_status: SubscriptionStatus, fields: &StatusUpdateFields) -> SubscriptionStatus {
    let now = Utc::now();

    if let Some(ea) = fields.expires_at
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
        (Some(true | false), false) | (None, true | false) => current_status,
    }
}

#[cfg(test)]
mod tests {
    use super::{StatusUpdateFields, compute_updated_status};
    use andromeda_ngsi_ld_model::{clearable::Clearable, subscription::SubscriptionStatus, subscription_request::SubscriptionRequest};
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
                expires_at: Some(future),
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
                expires_at: Some(past),
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
                expires_at: Some(future),
            },
        );
        pretty_assertions::assert_eq!(status, SubscriptionStatus::Active);
    }

    /// A request names the status members it writes, and no others.
    #[test]
    fn a_request_names_only_the_status_members_it_writes() {
        let none = super::requested_status_change(&SubscriptionRequest::default());
        assert!(none.is_active.is_none() && none.expires_at.is_none());

        let expires_at = Utc::now() + Duration::hours(1);
        let written = super::requested_status_change(&SubscriptionRequest {
            is_active: Clearable::Set(false),
            expires_at: Clearable::Set(expires_at),
            ..SubscriptionRequest::default()
        });
        pretty_assertions::assert_eq!(written.is_active, Some(false));
        pretty_assertions::assert_eq!(written.expires_at, Some(expires_at));
    }

    /// Clause 5.5.8 with Table 5.2.12-1: a deleted `isActive` is the default
    /// `true`, and a deleted `expiresAt` is a member that is not present.
    #[test]
    fn a_deleted_member_is_the_default_or_a_member_that_is_not_present() {
        let cleared = super::requested_status_change(&SubscriptionRequest {
            is_active: Clearable::Cleared,
            expires_at: Clearable::Cleared,
            ..SubscriptionRequest::default()
        });
        pretty_assertions::assert_eq!(cleared.is_active, Some(true));
        assert!(cleared.expires_at.is_none());
    }

    /// Clause 5.8.2.4: with `isActive` true and no `expiresAt`, a paused
    /// subscription becomes active, and an expired one stays expired.
    #[test]
    fn a_deleted_is_active_reactivates_a_paused_subscription_but_not_an_expired_one() {
        let cleared = super::requested_status_change(&SubscriptionRequest {
            is_active: Clearable::Cleared,
            ..SubscriptionRequest::default()
        });
        pretty_assertions::assert_eq!(compute_updated_status(SubscriptionStatus::Paused, &cleared), SubscriptionStatus::Active);
        pretty_assertions::assert_eq!(compute_updated_status(SubscriptionStatus::Expired, &cleared), SubscriptionStatus::Expired);
    }

    /// Clause 5.8.2.4: a fragment that deletes only `expiresAt` leaves the
    /// status the subscription has.
    #[test]
    fn a_deleted_expires_at_alone_leaves_the_status() {
        let cleared = super::requested_status_change(&SubscriptionRequest {
            expires_at: Clearable::Cleared,
            ..SubscriptionRequest::default()
        });
        pretty_assertions::assert_eq!(compute_updated_status(SubscriptionStatus::Active, &cleared), SubscriptionStatus::Active);
        pretty_assertions::assert_eq!(compute_updated_status(SubscriptionStatus::Paused, &cleared), SubscriptionStatus::Paused);
    }
}
