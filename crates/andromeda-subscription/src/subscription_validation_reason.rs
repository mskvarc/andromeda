//! The concrete reasons a subscription request is rejected as `BadRequestData`.
//!
//! Every client-facing validation failure of subscription policy is one variant
//! here, carrying the offending value as a typed field and chaining the typed
//! source error where one exists, so
//! [`SubscriptionError::Validation`](crate::error::SubscriptionError::Validation)
//! never holds an ad-hoc string. Each `#[error]` renders lowercase client prose;
//! spec member and value tokens (`Subscription`, `notification.endpoint`,
//! `timeInterval`, `expiresAt`, `geoQ`, ...) keep their fixed casing, and clause
//! numbers stay in doc comments.
//!
//! A member whose JSON is not the data type Table 5.2.12-1 gives it never
//! reaches here: the request model refuses the body when it is read, and what
//! is judged here is what a data type cannot say.

use crate::{endpoint_refusal::EndpointRefusal, geo_filter::GeoQError};
use andromeda_linked_retrieval::admission::AdmissionError;
use andromeda_ngsi_ld_model::subscription_member::SubscriptionMember;
use andromeda_ngsi_ld_query::{error::QueryError, projection::exclusivity::ProjectionConflict};
use thiserror::Error;

/// Why a subscription request is invalid.
#[derive(Debug, Error)]
pub enum SubscriptionValidationReason {
    /// A create body left out `type`, which Table 5.2.12-1 gives cardinality 1.
    #[error("type is required and shall be \"Subscription\"")]
    TypeRequired,
    /// The `notification` member was absent.
    #[error("notification is required")]
    NotificationRequired,
    /// The `notification.endpoint` member was absent.
    #[error("notification.endpoint is required")]
    EndpointRequired,
    /// The endpoint could not be served by any delivery binding the broker was
    /// built with.
    ///
    /// The injected endpoint validator lives in a crate this one cannot depend
    /// on without inverting the layering, so its refusal arrives classified.
    #[error(transparent)]
    EndpointRejected(EndpointRefusal),
    /// A create body wrote a member as a null, a JSON `null` or the NGSI-LD
    /// Null `urn:ngsi-ld:null`. Clause 5.2.1 makes a null outside the update
    /// clauses 5.5.8 and 5.5.12 `BadRequestData`, and a create has no member to
    /// delete.
    #[error("{0} must not be null in a creation request, which has no member to delete")]
    NullOnCreate(SubscriptionMember),
    /// `expiresAt` was in the past (clauses 5.8.1.4, 5.8.2.4).
    #[error("expiresAt must be in the future")]
    ExpiresAtInPast,
    /// `timeInterval` and `watchedAttributes` were both present (clause 5.8.1.4).
    #[error("timeInterval and watchedAttributes are mutually exclusive")]
    TimeIntervalWithWatchedAttributes,
    /// `timeInterval` and `throttling` were both present (clause 5.8.1.4).
    #[error("timeInterval and throttling are mutually exclusive")]
    TimeIntervalWithThrottling,
    /// `throttling` was not greater than zero.
    #[error("throttling must be greater than 0")]
    ThrottlingNotPositive,
    /// `timeInterval` was not greater than zero.
    #[error("timeInterval must be greater than 0")]
    TimeIntervalNotPositive,
    /// `watchedAttributes` was present but empty.
    #[error("watchedAttributes must not be empty")]
    WatchedAttributesEmpty,
    /// `notificationTrigger` was present but empty (Table 5.2.12-1).
    #[error("notificationTrigger must not be empty")]
    NotificationTriggerEmpty,
    /// `entities` was present but empty.
    #[error("entities must not be empty")]
    EntitiesEmpty,
    /// Neither `entities` nor `watchedAttributes` was given on a non-local
    /// subscription (clause 5.8.1.4).
    #[error("at least one of 'entities' or 'watchedAttributes' is required, unless 'localOnly' is true")]
    SelectorRequired,
    /// `timeInterval` was given without `entities` on a non-local subscription.
    #[error("'entities' is required when 'timeInterval' is present, unless the request is limited to local scope")]
    EntitiesRequiredForTimeInterval,
    /// A `q` filter for a subscription named no comparison operator.
    ///
    /// A subscription's `q` is matched against one notified entity, so a filter
    /// with no comparison selects nothing meaningful.
    #[error("q expression must contain at least one comparison operator")]
    QExpressionNoComparison,
    /// `notification.showChanges` was true under a `keyValues` format, which
    /// cannot represent the previous value.
    #[error("notification.showChanges cannot be true when notification.format is keyValues")]
    ShowChangesWithKeyValues,
    /// The `geoQ` is not a `GeoQuery` the broker can evaluate (clause 5.2.13,
    /// Table 5.2.13-1, clause 4.10).
    #[error(transparent)]
    GeoQ(GeoQError),
    /// The `temporalQ` names no time window (clause 4.11, Table 5.2.21-1:
    /// `endTimeAt` comes with `between` alone and closes after `timeAt`).
    #[error("temporalQ is invalid: {0}")]
    TemporalQ(QueryError),
    /// The notification projection combined mutually exclusive selectors
    /// (clause 4.21).
    #[error(transparent)]
    Projection(#[from] ProjectionConflict),
    /// A Linked Entity reference the subscription operation does not admit.
    #[error(transparent)]
    LinkedRetrieval(#[from] AdmissionError),
}
