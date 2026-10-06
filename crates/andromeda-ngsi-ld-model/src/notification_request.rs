//! The `notification` member of a Subscription request body (ETSI GS CIM 009
//! clause 5.2.14).
//!
//! A client writes `pick` and `omit` as arrays of clause 4.21 projection
//! language strings, while the subscription keeps the term tree of every string
//! with its names expanded, so the member a client sends is not the
//! [`NotificationParams`] the broker stores. The output-only members of Table
//! 5.2.14.2-1 (`timesSent`, `lastNotification`, ...) are accepted and ignored
//! as clause 5.8.2.4 requires: the broker supplies them on every read.

use crate::{
    non_null_member,
    subscription::{Endpoint, NotificationParams},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::projection::grammar::ProjectionList;
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat};
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};
use std::num::NonZeroU32;
use thiserror::Error;
use typed_builder::TypedBuilder;

/// Why a [`NotificationRequest`] is no complete [`NotificationParams`].
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum NotificationRequestError {
    /// The request named no `endpoint`, which Table 5.2.14.1-1 makes mandatory.
    #[error("notification.endpoint is required")]
    EndpointMissing,
}

/// A Subscription's notification parameters as a request writes them.
#[serde_as]
#[derive(Debug, Clone, Default, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
#[builder(field_defaults(default = Default::default()))]
pub struct NotificationRequest {
    /// Attributes to include in notifications (empty means all).
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "non_null_member::deserialize")]
    pub attributes: Option<Vec<AttributeName>>,
    /// Representation format for notification payloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<ResponseFormat>,
    /// Target endpoint for notification delivery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<Endpoint>,
    /// Whether to include system-generated attributes (createdAt, modifiedAt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sys_attrs: Option<bool>,
    /// Whether to include previous attribute values in notifications.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_changes: Option<bool>,
    /// The clause 4.21 projection keeping only the members it names, one list
    /// per projection-language string the client wrote.
    #[serde_as(as = "Option<Vec<DisplayFromStr>>")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pick: Option<Vec<ProjectionList>>,
    /// The clause 4.21 projection removing the members it names, written as
    /// `pick` is.
    #[serde_as(as = "Option<Vec<DisplayFromStr>>")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omit: Option<Vec<ProjectionList>>,
    /// Join mode for linked entity retrieval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<JoinMode>,
    /// Maximum depth for linked entity joins (clause 4.5.23 `joinLevel`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_level: Option<NonZeroU32>,
}

impl TryFrom<NotificationRequest> for NotificationParams {
    type Error = NotificationRequestError;

    /// The parameters the request describes, their output-only members left
    /// for the store to supply.
    ///
    /// The `pick` and `omit` lists are taken as they stand, so the caller
    /// expands their names first when the parameters are to be stored.
    fn try_from(request: NotificationRequest) -> Result<NotificationParams, NotificationRequestError> {
        let NotificationRequest {
            attributes,
            format,
            endpoint,
            sys_attrs,
            show_changes,
            pick,
            omit,
            join,
            join_level,
        } = request;
        Ok(NotificationParams {
            attributes,
            format,
            endpoint: endpoint.ok_or(NotificationRequestError::EndpointMissing)?,
            sys_attrs,
            show_changes,
            pick,
            omit,
            join,
            join_level,
            last_notification: None,
            last_success: None,
            last_failure: None,
            times_sent: None,
            times_failed: None,
            status: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        notification_request::{NotificationRequest, NotificationRequestError},
        subscription::NotificationParams,
    };
    use serde_json::json;

    fn request(wire: serde_json::Value) -> NotificationRequest {
        serde_json::from_value(wire).expect("a readable notification member")
    }

    /// Table 5.2.14.1-1: `pick` and `omit` are arrays of clause 4.21 strings,
    /// each read into its own list and written back as the string it was.
    #[test]
    fn projection_members_are_read_as_strings_and_written_back_as_strings() {
        let wire = json!({
            "endpoint": {"uri": "https://receiver.example.com/notify"},
            "pick": ["id", "owner{name},speed"],
            "omit": ["scope"]
        });
        let notification = request(wire.clone()); // clone: the wire form is compared with the written one below
        pretty_assertions::assert_eq!(notification.pick.as_ref().map(Vec::len), Some(2));
        pretty_assertions::assert_eq!(serde_json::to_value(&notification).expect("serialises"), wire);
    }

    /// Table 5.2.14.1-1: a string outside the clause 4.21 grammar, and an
    /// element that is no string, are refused.
    #[test]
    fn a_projection_member_that_is_not_an_array_of_projection_strings_is_refused() {
        for pick in [json!(["owner{"]), json!("id"), json!([{"name": "id", "linked": []}]), json!([["id"]])] {
            let wire = json!({"endpoint": {"uri": "https://receiver.example.com/notify"}, "pick": pick});
            assert!(serde_json::from_value::<NotificationRequest>(wire).is_err(), "{pick}");
        }
    }

    /// Clause 5.8.2.4: the output-only members are accepted and ignored.
    #[test]
    fn output_only_members_are_ignored_and_the_parameters_leave_them_to_the_store() {
        let params = NotificationParams::try_from(request(json!({
            "endpoint": {"uri": "https://receiver.example.com/notify"},
            "timesSent": 3,
            "lastNotification": "2024-01-01T00:00:00Z",
            "status": "ok"
        })))
        .expect("an endpoint is present");
        assert!(params.times_sent.is_none() && params.last_notification.is_none() && params.status.is_none());
    }

    #[test]
    fn a_notification_without_an_endpoint_is_no_set_of_parameters() {
        pretty_assertions::assert_eq!(
            NotificationParams::try_from(request(json!({"format": "normalized"}))).err(),
            Some(NotificationRequestError::EndpointMissing)
        );
    }
}
