//! The members an update fragment deletes (clauses 5.5.8 and 5.8.2.4).
//!
//! The merged subscription a forwarding hook is handed no longer says which
//! members the update removed: a deleted member is simply gone from it. A
//! source holding a reduced copy still has the member, so the deletion is
//! named here for the hook to forward (clause 5.8.2.4 has the copy updated
//! with the original's update, and clause 5.5.8 makes an NGSI-LD Null in it a
//! deletion). Only the members a reduced copy can carry are listed.

use andromeda_ngsi_ld_model::subscription_request::SubscriptionRequest;

/// A subscription member a reduced copy carries and an update may delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletableMember {
    /// `subscriptionName`.
    SubscriptionName,
    /// `description`.
    Description,
    /// `entities`.
    Entities,
    /// `watchedAttributes`.
    WatchedAttributes,
    /// `notificationTrigger`.
    NotificationTrigger,
    /// `timeInterval`.
    TimeInterval,
    /// `q`.
    Q,
    /// `geoQ`.
    GeoQ,
    /// `scopeQ`.
    ScopeQ,
    /// `datasetId`.
    DatasetId,
    /// `throttling`.
    Throttling,
    /// `expiresAt`.
    ExpiresAt,
}

/// The members one update fragment deleted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeletedMembers(Vec<DeletableMember>);

impl DeletedMembers {
    /// The members `fragment` deletes.
    #[must_use]
    pub fn of(fragment: &SubscriptionRequest) -> DeletedMembers {
        let members = [
            (DeletableMember::SubscriptionName, fragment.subscription_name.is_ngsi_ld_null()),
            (DeletableMember::Description, fragment.description.is_ngsi_ld_null()),
            (DeletableMember::Entities, fragment.entities.is_ngsi_ld_null()),
            (DeletableMember::WatchedAttributes, fragment.watched_attributes.is_ngsi_ld_null()),
            (DeletableMember::NotificationTrigger, fragment.notification_trigger.is_ngsi_ld_null()),
            (DeletableMember::TimeInterval, fragment.time_interval.is_ngsi_ld_null()),
            (DeletableMember::Q, fragment.q.is_ngsi_ld_null()),
            (DeletableMember::GeoQ, fragment.geo_q.is_ngsi_ld_null()),
            (DeletableMember::ScopeQ, fragment.scope_q.is_ngsi_ld_null()),
            (DeletableMember::DatasetId, fragment.dataset_id.is_ngsi_ld_null()),
            (DeletableMember::Throttling, fragment.throttling.is_ngsi_ld_null()),
            (DeletableMember::ExpiresAt, fragment.expires_at.is_ngsi_ld_null()),
        ];
        DeletedMembers(members.into_iter().filter_map(|(member, deleted)| deleted.then_some(member)).collect())
    }

    /// Whether the update deleted `member`.
    #[must_use]
    pub fn contains(&self, member: DeletableMember) -> bool {
        self.0.contains(&member)
    }
}

#[cfg(test)]
mod tests {
    use crate::deleted_members::{DeletableMember, DeletedMembers};
    use andromeda_ngsi_ld_model::subscription_request::SubscriptionRequest;
    use serde_json::json;

    fn fragment(members: &serde_json::Value) -> SubscriptionRequest {
        serde_json::from_value(members.clone()).expect("a readable fragment") // clone: the fixture is borrowed by the caller
    }

    /// Clause 5.5.8: a member written as the NGSI-LD Null is deleted, one that
    /// is set or left out is not.
    #[test]
    fn a_member_written_as_the_null_is_deleted_and_a_set_or_absent_member_is_not() {
        let deleted = DeletedMembers::of(&fragment(
            &json!({"expiresAt": "urn:ngsi-ld:null", "description": "kept", "q": "urn:ngsi-ld:null"}),
        ));
        assert!(deleted.contains(DeletableMember::ExpiresAt));
        assert!(deleted.contains(DeletableMember::Q));
        assert!(!deleted.contains(DeletableMember::Description));
        assert!(!deleted.contains(DeletableMember::SubscriptionName));
    }

    /// Clause 4.5.0: where an array is required, an array with one single
    /// NGSI-LD Null deletes the member.
    #[test]
    fn an_array_of_one_ngsi_ld_null_deletes_an_array_member() {
        let deleted = DeletedMembers::of(&fragment(&json!({"datasetId": ["urn:ngsi-ld:null"]})));
        assert!(deleted.contains(DeletableMember::DatasetId));
    }

    #[test]
    fn an_empty_fragment_deletes_nothing() {
        pretty_assertions::assert_eq!(DeletedMembers::of(&fragment(&json!({}))), DeletedMembers::default());
    }
}
