//! The concrete reasons a subscription request is rejected as `BadRequestData`.
//!
//! Every client-facing validation failure of subscription policy is one variant
//! here, carrying the offending value as a typed field and chaining the typed
//! source error where one exists, so
//! [`SubscriptionError::Validation`](crate::error::SubscriptionError::Validation)
//! never holds an ad-hoc string. Each `#[error]` renders lowercase client prose;
//! spec member and value tokens (`Subscription`, `notification.endpoint.uri`,
//! `timeInterval`, `expiresAt`, `geoQ`, ...) keep their fixed casing, and clause
//! numbers stay in doc comments.

use andromeda_ngsi_ld_geometry::geojson::GeometryError;
use andromeda_ngsi_ld_lexicon::{spec_version::SpecVersionParseError, uri::UriParseError};
use andromeda_ngsi_ld_query::projection::exclusivity::ProjectionConflict;
use thiserror::Error;

/// Why a subscription request is invalid.
#[derive(Debug, Error)]
pub enum SubscriptionValidationReason {
    /// The `notification` member was absent.
    #[error("notification is required")]
    NotificationRequired,
    /// The `notification` member was explicitly null in an update.
    #[error("notification cannot be set to null")]
    NotificationNull,
    /// The `notification` member was not a JSON object.
    #[error("notification must be an object")]
    NotificationNotObject,
    /// The `notification.endpoint` member was absent.
    #[error("notification.endpoint is required")]
    EndpointRequired,
    /// The `notification.endpoint` member was explicitly null in an update.
    #[error("notification.endpoint cannot be set to null")]
    EndpointNull,
    /// The `notification.endpoint.uri` member was absent.
    #[error("notification.endpoint.uri is required")]
    EndpointUriRequired,
    /// The `notification.endpoint.uri` member was explicitly null in an update.
    #[error("notification.endpoint.uri cannot be set to null")]
    EndpointUriNull,
    /// The `notification.endpoint.uri` member was not a non-empty string.
    #[error("notification.endpoint.uri must be a non-empty string")]
    EndpointUriEmpty,
    /// The `notification.endpoint.uri` was not a valid URL.
    #[error("notification.endpoint.uri is not a valid URL: {source}")]
    EndpointUri {
        /// The URL grammar violation.
        #[source]
        source: url::ParseError,
    },
    /// The endpoint could not be served by any delivery binding the broker was
    /// built with.
    ///
    /// The text is relayed verbatim from the injected endpoint validator, which
    /// lives in a crate this one cannot depend on without inverting the layering,
    /// so its reason arrives as opaque prose rather than a typed cause.
    #[error("{detail}")]
    EndpointRejected {
        /// The endpoint validator's own refusal detail.
        detail: String,
    },
    /// `notifierInfo` was not an array of key-value pairs.
    #[error("notifierInfo must be an array of key-value pairs: {source}")]
    NotifierInfo {
        /// The JSON shape violation.
        #[source]
        source: serde_json::Error,
    },
    /// `expiresAt` was in the past (clauses 5.8.1.4, 5.8.2.4).
    #[error("expiresAt must be in the future")]
    ExpiresAtInPast,
    /// `expiresAt` was not a clause 4.6.3 `DateTime`.
    #[error("expiresAt must be a valid ISO 8601 date-time")]
    ExpiresAtNotDateTime,
    /// `expiresAt` was a string that did not parse as an RFC 3339 timestamp.
    #[error("invalid 'expiresAt' value '{raw}': {source}")]
    ExpiresAt {
        /// The rejected raw value.
        raw: String,
        /// The timestamp grammar violation.
        #[source]
        source: chrono::ParseError,
    },
    /// The subscription `type` member was not `Subscription` (clause 5.2.12).
    #[error("type must be 'Subscription'")]
    TypeNotSubscription,
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
    /// One typed member of the payload was not the JSON shape its data type
    /// requires (clause 5.2.12).
    #[error("invalid '{member}' value: {source}")]
    MemberValue {
        /// The member whose value was rejected.
        member: String,
        /// The JSON shape violation.
        #[source]
        source: serde_json::Error,
    },
    /// `ngsildConformance` was present but not a string.
    #[error("'ngsildConformance' must be a string of the form major.minor")]
    NgsildConformanceNotString,
    /// `ngsildConformance` was a string outside the `major.minor` grammar.
    #[error("invalid 'ngsildConformance' value: {source}")]
    NgsildConformance {
        /// The version grammar violation.
        #[source]
        source: SpecVersionParseError,
    },
    /// A `q` filter for a subscription named no comparison operator.
    ///
    /// A subscription's `q` is matched against one notified entity, so a filter
    /// with no comparison selects nothing meaningful.
    #[error("q expression must contain at least one comparison operator")]
    QExpressionNoComparison,
    /// A subscription `q` filter traversed into a linked entity.
    #[error(
        "a filter that traverses into a linked entity is not supported for subscriptions, because a subscription is matched against the notified entity's own data"
    )]
    QExpressionLinkedTraversal,
    /// `notification.showChanges` was true under a `keyValues` format, which
    /// cannot represent the previous value.
    #[error("notification.showChanges cannot be true when notification.format is keyValues")]
    ShowChangesWithKeyValues,
    /// The `geoQ.geometry` member was not a string.
    #[error("geoQ geometry must be a GeoJSON geometry type string")]
    GeoQGeometryNotString,
    /// The `geoQ.geometry` string named no `GeoJSON` geometry type.
    #[error("geoQ geometry: {source}")]
    GeoQGeometryType {
        /// The unknown geometry-type token.
        #[source]
        source: strum::ParseError,
    },
    /// The `geoQ.coordinates` member was a string that was not JSON.
    #[error("geoQ coordinates: {source}")]
    GeoQCoordinates {
        /// The JSON parse failure.
        #[source]
        source: serde_json::Error,
    },
    /// The `geoQ` geometry and coordinates did not form a `GeoJSON` geometry.
    #[error("geoQ reference geometry: {source}")]
    GeoQReferenceGeometry {
        /// The geometry decoding failure.
        #[source]
        source: GeometryError,
    },
    /// The subscription `id` was not a `URI`.
    #[error("subscription id must be a valid URI: {source}")]
    SubscriptionId {
        /// The `URI` grammar violation.
        #[source]
        source: UriParseError,
    },
    /// The notification projection combined mutually exclusive selectors
    /// (clause 4.21).
    #[error(transparent)]
    Projection(#[from] ProjectionConflict),
}
