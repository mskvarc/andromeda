//! When a registration was written and when it stops applying (ETSI GS CIM 009 clauses 4.8 and 5.2.9).

use andromeda_ngsi_ld_lexicon::time::instant_serde;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// When a registration was written and when it stops applying
/// (ETSI GS CIM 009 clauses 4.8 and 5.2.9).
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationLifecycle {
    /// Registration expiration timestamp.
    #[serde(skip_serializing_if = "Option::is_none", with = "instant_serde::optional", default)]
    #[builder(default = Default::default())]
    pub expires_at: Option<DateTime<Utc>>,
    /// Timestamp when the registration was created.
    #[serde(with = "instant_serde::required")]
    pub created_at: DateTime<Utc>,
    /// Timestamp when the registration was last modified.
    #[serde(with = "instant_serde::required")]
    pub modified_at: DateTime<Utc>,
}

impl RegistrationLifecycle {
    /// Whether the registration is in force at `at`: it has no `expiresAt`, or
    /// that instant has not been reached (Table 5.2.9-1: once `expiresAt` has
    /// passed the registration "will become invalid").
    ///
    /// Liveness is this alone. `status` (Table 5.2.9-2) reports only the last
    /// distributed operation, and `createdAt` is stamped by the broker, so
    /// neither takes a registration out of force.
    #[must_use]
    pub fn in_force_at(&self, at: DateTime<Utc>) -> bool {
        RegistrationLifecycle::expiry_in_force_at(self.expires_at, at)
    }

    /// Whether a registration expiring at `expires_at` is in force at `at`,
    /// for a registration being written that has no lifecycle yet. The
    /// boundary is exclusive: at `expires_at` itself the registration is out
    /// of force.
    #[must_use]
    pub fn expiry_in_force_at(expires_at: Option<DateTime<Utc>>, at: DateTime<Utc>) -> bool {
        expires_at.is_none_or(|expires_at| expires_at > at)
    }
}

#[cfg(test)]
mod tests {
    use crate::registration::{ContextSourceRegistration, lifecycle::RegistrationLifecycle, status::CsrStatus};
    use chrono::{DateTime, Duration, Utc};
    use serde_json::json;

    fn lifecycle(expires_at: Option<DateTime<Utc>>) -> RegistrationLifecycle {
        RegistrationLifecycle::builder()
            .expires_at(expires_at)
            .created_at(DateTime::UNIX_EPOCH)
            .modified_at(DateTime::UNIX_EPOCH)
            .build()
    }

    #[test]
    fn in_force_at_is_false_at_and_after_expires_at_and_true_without_one() {
        let expires_at = DateTime::UNIX_EPOCH + Duration::days(10);
        let expiring = lifecycle(Some(expires_at));
        assert!(expiring.in_force_at(expires_at - Duration::milliseconds(1)));
        assert!(!expiring.in_force_at(expires_at), "expiresAt itself is out of force");
        assert!(!expiring.in_force_at(expires_at + Duration::days(1)));
        assert!(lifecycle(None).in_force_at(expires_at + Duration::days(36_500)));
    }

    /// Table 5.2.9-2: `status` reports the last distributed operation, not
    /// whether the registration applies.
    #[test]
    fn a_failed_status_does_not_take_a_registration_out_of_force() {
        let registration: ContextSourceRegistration = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:failing",
            "type": "ContextSourceRegistration",
            "information": [{"propertyNames": ["speed"]}],
            "endpoint": "https://source.example.com/",
            "status": "failed",
            "timesFailed": 4,
            "expiresAt": "2100-01-01T00:00:00Z",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00Z"
        }))
        .expect("a registration");
        pretty_assertions::assert_eq!(registration.health.status, Some(CsrStatus::Failed));
        assert!(registration.lifecycle.in_force_at(Utc::now()));
    }
}
