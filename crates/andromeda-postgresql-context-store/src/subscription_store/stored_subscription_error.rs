//! What a `subscriptions` row that no longer reads back as a subscription looks
//! like.
//!
//! The broker only ever writes values it validated, so each variant is a column
//! holding something the broker never wrote: a restore from another release, or
//! manual surgery. Each names the column and carries the parser's own rejection.

use andromeda_context_commit::subscription_key::SubscriptionKeyOutOfRange;
use andromeda_ngsi_ld_lexicon::{identifier::IdentifierParseError, reference::NgsiLdReferenceError, spec_version::SpecVersionParseError};
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_postgresql_codes::registry::UnknownCompactCode;
use iri_rs::InvalidIri;
use langtag::InvalidLangTag;
use thiserror::Error;

/// A stored subscription column that does not parse back into its domain type.
#[derive(Debug, Error)]
pub enum StoredSubscriptionError {
    /// The `id` column is not a URI.
    #[error("a stored subscription id is not a URI: {0}")]
    Id(#[source] IdentifierParseError),
    /// The `subscription_key` column is not a surrogate key.
    #[error("a stored subscription key is not a surrogate key: {0}")]
    Key(#[source] SubscriptionKeyOutOfRange),
    /// One `dataset_ids` element is neither a URI nor `@none`.
    #[error("a stored subscription datasetId is neither a URI nor @none: {0}")]
    DatasetId(#[source] Box<QueryError>),
    /// The `entities` column does not deserialize.
    #[error("the stored subscription entities do not deserialize: {0}")]
    Entities(#[source] serde_json::Error),
    /// One `watched_attributes` element is not an attribute name.
    #[error("a stored subscription watchedAttributes element is not an attribute name: {0}")]
    WatchedAttribute(#[source] NgsiLdReferenceError),
    /// One `expand_values` element is not an attribute name.
    #[error("a stored subscription expandValues element is not an attribute name: {0}")]
    ExpandValue(#[source] NgsiLdReferenceError),
    /// One `json_keys` element is not an attribute name.
    #[error("a stored subscription jsonKeys element is not an attribute name: {0}")]
    JsonKey(#[source] NgsiLdReferenceError),
    /// One `notification_triggers` element holds a code the trigger registry
    /// never assigned.
    #[error("a stored subscription notificationTrigger is not a known code: {0}")]
    NotificationTrigger(#[source] UnknownCompactCode),
    /// The `q` column is not a query.
    #[error("a stored subscription q is not a query: {0}")]
    Q(#[source] Box<QueryError>),
    /// The `q_written_members` column is not a query.
    #[error("a stored subscription q, as its members were written, is not a query: {0}")]
    QWrittenMembers(#[source] Box<QueryError>),
    /// The `scope_q` column is not a scope query.
    #[error("a stored subscription scopeQ is not a scope query: {0}")]
    ScopeQ(#[source] Box<QueryError>),
    /// The `csf` column is not a query.
    #[error("a stored subscription csf is not a query: {0}")]
    Csf(#[source] Box<QueryError>),
    /// The `notification` column does not deserialize.
    #[error("the stored subscription notification parameters do not deserialize: {0}")]
    Notification(#[source] serde_json::Error),
    /// The `notification` column holds the JSON `null` literal, so the
    /// subscription has no notification parameters.
    #[error("the stored subscription holds no notification parameters")]
    NotificationAbsent,
    /// The `geo_q` column does not deserialize.
    #[error("the stored subscription geoQ does not deserialize: {0}")]
    GeoQ(#[source] serde_json::Error),
    /// The `temporal_q` column does not deserialize.
    #[error("the stored subscription temporalQ does not deserialize: {0}")]
    TemporalQ(#[source] serde_json::Error),
    /// The `lang` column is not a language tag.
    #[error("a stored subscription lang is not a language tag: {0}")]
    Lang(#[source] InvalidLangTag<String>),
    /// The `jsonld_context` column is not an absolute IRI.
    #[error("a stored subscription jsonldContext is not an absolute IRI: {0}")]
    JsonldContext(#[source] InvalidIri<String>),
    /// The `status` column holds a code the status registry never assigned.
    #[error("a stored subscription status is not a known code: {0}")]
    Status(#[source] UnknownCompactCode),
    /// The `ngsild_conformance` column is not a specification version.
    #[error("a stored subscription ngsildConformance is not a specification version: {0}")]
    NgsildConformance(#[source] SpecVersionParseError),
    /// One registration a context source subscription recorded as notified is
    /// not a URI.
    #[error("a stored notified registration id is not a URI: {0}")]
    NotifiedRegistration(#[source] IdentifierParseError),
}
