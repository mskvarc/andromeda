//! The rules the `notification` member of a Subscription request body is
//! checked against (ETSI GS CIM 009 clause 5.2.14), beyond the data types the
//! request model already reads it as.

use crate::{error::SubscriptionError, subscription_validation_reason::SubscriptionValidationReason};
use andromeda_linked_retrieval::admission::{LinkedOperation, LinkedRequest, admit_request};
use andromeda_ngsi_ld_model::notification_request::NotificationRequest;
use andromeda_ngsi_ld_query::{
    filter::expression::Expr,
    projection::{
        exclusivity::{Presence, ProjectionSelectors, check_projection_exclusivity},
        grammar::{ProjectionList, RawProjectionTerm},
    },
};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;

/// Reject a notification that names no endpoint (Table 5.2.14.1-1 makes
/// `endpoint` mandatory).
///
/// A create requires the member as a whole, and an update fragment that names
/// a `notification` replaces the stored one, so it has to name the endpoint
/// too.
///
/// # Errors
///
/// Returns [`SubscriptionValidationReason::EndpointRequired`] when the
/// notification carries no `endpoint`.
pub(crate) const fn validate_endpoint_present(notification: &NotificationRequest) -> Result<(), SubscriptionError> {
    match notification.endpoint {
        Some(_) => Ok(()),
        None => Err(SubscriptionError::Validation(SubscriptionValidationReason::EndpointRequired)),
    }
}

/// Validate the parts of one notification that stand apart from the endpoint,
/// the `q` the Subscription filters by being the one it shares a Linked Entity
/// admission with.
///
/// # Errors
///
/// Returns the [`SubscriptionError::Validation`] of the first rule the
/// notification breaks.
pub(crate) fn validate_notification(notification: &NotificationRequest, q: Option<&Expr>) -> Result<(), SubscriptionError> {
    validate_show_changes_format(notification)?;
    validate_projection_exclusivity(notification)?;
    let pick = notification.pick.as_deref().map(projection_terms);
    let omit = notification.omit.as_deref().map(projection_terms);
    admit_linked_references(notification, pick.as_deref(), omit.as_deref(), q)
}

/// Reject a notification combining `pick` or `omit` with the deprecated
/// `attributes`.
///
/// Table 5.2.14.1-1 gives `NotificationParams` the same three members a query
/// carries on its URL -- `pick`, `omit` and the deprecated `attributes`, "a
/// synonym for pick" -- so it carries the same rule about combining them. The
/// rule itself is owned by
/// [`andromeda_ngsi_ld_query::projection::exclusivity`], which the query
/// bindings apply to their own parameters. A member named in both `pick` and
/// `omit` is refused once the two are expanded, since only the expansions
/// tell whether two names are one member.
fn validate_projection_exclusivity(notification: &NotificationRequest) -> Result<(), SubscriptionError> {
    check_projection_exclusivity(ProjectionSelectors {
        pick: Presence::of(notification.pick.as_ref()),
        omit: Presence::of(notification.omit.as_ref()),
        attrs: Presence::of(notification.attributes.as_ref()),
    })
    .map_err(|conflict| SubscriptionError::Validation(SubscriptionValidationReason::Projection(conflict)))
}

/// Admit the Linked Entity references of a subscription: the clause 4.21
/// nesting of `pick`/`omit`, the clause 4.9 hops of `q` and the retrieving
/// `notification.join`.
///
/// Clause 5.8.1.4 adds a Subscription that passes the clause 5.2.12 checks,
/// none of which bounds these references, so the engine admits every one.
fn admit_linked_references(
    notification: &NotificationRequest,
    pick: Option<&[RawProjectionTerm]>,
    omit: Option<&[RawProjectionTerm]>,
    q: Option<&Expr>,
) -> Result<(), SubscriptionError> {
    let request = LinkedRequest::builder()
        .pick(pick)
        .omit(omit)
        .q(q)
        .join(notification.join)
        .join_level(notification.join_level)
        .build();
    admit_request(LinkedOperation::Subscription, &request).map_err(|refusal| SubscriptionError::Validation(refusal.into()))
}

/// Every term the strings of one projection member hold.
fn projection_terms(lists: &[ProjectionList]) -> Vec<RawProjectionTerm> {
    // clone: the admission reads the terms of every string as one slice
    lists.iter().flat_map(|list| list.iter().cloned()).collect()
}

/// Reject a notification asking for `showChanges` under a simplified format
/// (clause 5.2.14.1).
///
/// Table 5.2.14.1-1 states that "showChanges cannot be true in case format is
/// `keyValues`". The simplified rendering of clause 4.5.4 reduces an attribute
/// to its value alone, leaving nowhere for the `previousValue`,
/// `previousObject` or `previousLanguageMap` that `showChanges` exists to
/// deliver, so the combination asks for something no notification could carry.
/// Refusing it at creation and update is what stops a client holding a live
/// subscription whose defining option silently does nothing.
///
/// `simplified` is the same rendering under the name Table 5.2.14.1-1 gives as
/// the primary one, with `keyValues` listed as its synonym, so both are
/// refused.
fn validate_show_changes_format(notification: &NotificationRequest) -> Result<(), SubscriptionError> {
    let shows_changes = notification.show_changes == Some(true);
    if shows_changes && notification.format.is_some_and(ResponseFormat::is_simplified) {
        return Err(SubscriptionError::Validation(SubscriptionValidationReason::ShowChangesWithKeyValues));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        error::SubscriptionError,
        notification_validation::{validate_endpoint_present, validate_notification},
        subscription_validation_reason::SubscriptionValidationReason,
    };
    use andromeda_ngsi_ld_model::notification_request::NotificationRequest;
    use serde_json::{Value, json};

    fn notification(wire: Value) -> NotificationRequest {
        serde_json::from_value(wire).expect("a readable notification")
    }

    /// Clause 5.2.14.1, Table 5.2.14.1-1: "showChanges cannot be true in case
    /// format is `keyValues`". The simplified rendering of clause 4.5.4 has
    /// nowhere to carry a `previousValue`, so the combination is refused rather
    /// than silently doing nothing.
    #[test]
    fn show_changes_under_a_simplified_notification_format_is_rejected() {
        for format in ["keyValues", "simplified"] {
            let notification = notification(json!({"format": format, "showChanges": true, "endpoint": {"uri": "http://127.0.0.1:8085/notify"}}));
            let error = validate_notification(&notification, None).expect_err("clause 5.2.14.1 forbids the combination");
            assert!(error.to_string().contains("showChanges"), "{format}: {error}");
        }
    }

    /// The combination is only refused for the simplified renderings; the
    /// normalized and concise ones carry the previous member fine.
    #[test]
    fn show_changes_under_a_normalized_or_concise_format_is_accepted() {
        for format in ["normalized", "concise"] {
            let notification = notification(json!({"format": format, "showChanges": true, "endpoint": {"uri": "http://127.0.0.1:8085/notify"}}));
            let result = validate_notification(&notification, None);
            assert!(result.is_ok(), "{format}: {result:?}");
        }
    }

    /// The deprecated `attributes`, "a synonym for pick", cannot stand beside
    /// `pick` or `omit`.
    #[test]
    fn the_deprecated_attributes_beside_pick_or_omit_are_refused() {
        for wire in [
            json!({"pick": ["id"], "attributes": ["speed"]}),
            json!({"omit": ["type"], "attributes": ["speed"]}),
        ] {
            let result = validate_notification(&notification(wire.clone()), None);
            assert!(
                matches!(&result, Err(SubscriptionError::Validation(SubscriptionValidationReason::Projection(_)))),
                "{wire}: {result:?}"
            );
        }
    }

    #[test]
    fn pick_and_omit_of_different_members_are_admitted() {
        let result = validate_notification(&notification(json!({"pick": ["id", "speed"], "omit": ["type"]})), None);
        assert!(result.is_ok(), "{result:?}");
    }

    /// Clause 5.8.1.4: no clause 5.2.12 check bounds a Linked Entity
    /// reference, so a nesting is admitted without a retrieving join and
    /// deeper than `joinLevel` alike.
    #[test]
    fn a_nested_pick_without_join_is_admitted() {
        let q = "owner{name}==\"x\"".parse().expect("a filter with a hop");
        for wire in [
            json!({"pick": ["id", "owner{name}"]}),
            json!({"pick": ["owner{employer{name}}"], "join": "flat", "joinLevel": 1}),
            json!({"omit": ["owner{email}"], "join": "@none"}),
        ] {
            let result = validate_notification(&notification(wire.clone()), Some(&q));
            assert!(result.is_ok(), "{wire}: {result:?}");
        }
    }

    #[test]
    fn a_notification_without_an_endpoint_is_refused() {
        let result = validate_endpoint_present(&notification(json!({"format": "normalized"})));
        assert!(
            matches!(result, Err(SubscriptionError::Validation(SubscriptionValidationReason::EndpointRequired))),
            "{result:?}"
        );
        assert!(validate_endpoint_present(&notification(json!({"endpoint": {"uri": "http://127.0.0.1:8085/notify"}}))).is_ok());
    }
}
