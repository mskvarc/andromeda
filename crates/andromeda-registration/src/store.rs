//! Registration persistence port and commands.
//!
//! `update` takes a fully resolved value set rather than a fragment: clause
//! 5.10.4 defines a registration update as a merge against the stored
//! registration, and that merge is policy. The store applies a decided row.
//!
//! # Routing projection
//!
//! Registration `information` (clause 5.2.9) is a nested structure: each entry
//! carries entity selectors, property names and relationship names. Stored as a
//! single document it cannot be preselected against, so implementations are
//! required to maintain a flattened routing projection — one row per selective
//! capability — alongside the registration payload. The obligation is stated on
//! every mutating method of [`RegistrationStore`] and is part of the port
//! contract, not an implementation detail: a backend that persists the payload
//! without refreshing the projection is incorrect even though every read of a
//! single registration would still return the right document.
//!
//! Two invariants define the contract.
//!
//! Atomicity: the projection is rewritten in the same transaction that writes,
//! replaces or removes the payload. No intermediate state in which the payload
//! and the projection disagree may ever be observable by a concurrent reader,
//! and a failed mutation leaves both untouched.
//!
//! Superset preselection: the projection exists only to narrow candidates for
//! [`RegistrationStore::find_matching`]. The typed residual evaluation over the
//! returned registrations remains the oracle deciding final membership, so a
//! projection-driven preselect may return registrations that do not ultimately
//! match — that costs residual CPU — but must never omit one that does. A
//! false negative silently drops a context source from distributed operations.
//!
//! # Where the projection is derived
//!
//! The port supplies no separate selector-summary type, and deliberately so.
//! JSON-LD expansion of `information` happens in the service layer before any
//! store call, so entity types, property names and relationship names reach
//! this port already expanded to IRIs inside the typed
//! [`RegistrationInfo`](andromeda_ngsi_ld_model::registration::RegistrationInfo)
//! values carried by [`CreateCsrParams`](crate::store::CreateCsrParams) and
//! [`ResolvedCsrUpdate`](crate::resolved_update::ResolvedCsrUpdate). Those
//! values are the complete and authoritative input to the projection; deriving
//! a second domain-side summary type would duplicate them and would force the
//! domain to know how a particular backend interns names or encodes operations.
//! Implementations therefore derive projection rows store-side from the typed
//! `information` they are handed, and own the encoding of that derivation.

use crate::{error::RegistrationError, nullable_member::NullableMember, resolved_update::ResolvedCsrUpdate};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    context_source_pseudonym::ContextSourcePseudonym,
    non_empty_text::NonEmptyText,
    scope::grammar::ScopeBuf,
    tenant::TenantName,
    uri::Uri,
};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    registration::{ContextSourceRegistration, CsrOperation, ForwardingMode, RegistrationInfo},
    registration_management::RegistrationManagementInfo,
    time_interval::TimeInterval,
};
use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrQuery, dataset::DatasetSelector, pagination::Pagination};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use iri_rs::IriBuf;
use iso8601_duration::Duration as Iso8601Duration;
use mediatype::MediaTypeBuf;
use serde_json::Value;
use typed_builder::TypedBuilder;
use url::Url;

/// Parameters for creating a new context source registration.
#[derive(Debug, TypedBuilder)]
pub struct CreateCsrParams {
    /// Human-readable name of the registration.
    #[builder(default = Default::default())]
    pub registration_name: Option<NonEmptyText>,
    /// Free-text description of the registration.
    #[builder(default = Default::default())]
    pub description: Option<NonEmptyText>,
    /// Registration information entries describing provided entities/attributes.
    pub information: Vec<RegistrationInfo>,
    /// The `datasetId`s of the attribute instances the source provides.
    #[builder(default = Default::default())]
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// Tenant identifier for multi-tenancy.
    #[builder(default = Default::default())]
    pub tenant: Option<TenantName>,
    /// Interval the source's observations span.
    #[builder(default = Default::default())]
    pub observation_interval: Option<TimeInterval>,
    /// Interval the source's management data spans.
    #[builder(default = Default::default())]
    pub management_interval: Option<TimeInterval>,
    /// The `location` geometry.
    #[builder(default = Default::default())]
    pub location: Option<Geometry>,
    /// The `observationSpace` geometry.
    #[builder(default = Default::default())]
    pub observation_space: Option<Geometry>,
    /// The `operationSpace` geometry.
    #[builder(default = Default::default())]
    pub operation_space: Option<Geometry>,
    /// Expiration timestamp for the registration.
    #[builder(default = Default::default())]
    pub expires_at: Option<DateTime<Utc>>,
    /// Base endpoint URL of the context source.
    pub endpoint: Url,
    /// Preferred MIME type for responses from the context source.
    #[builder(default = Default::default())]
    pub accept: Option<MediaTypeBuf>,
    /// Forwarding mode for distributed operations.
    #[builder(default = Default::default())]
    pub mode: ForwardingMode,
    /// NGSI-LD operations supported by the context source; the clause 4.20
    /// `federationOps` default is already materialised here.
    pub operations: Vec<CsrOperation>,
    /// Scope values for the registration.
    #[builder(default = Default::default())]
    pub scope: Option<Vec<ScopeBuf>>,
    /// ISO 8601 duration between context source refreshes.
    #[builder(default = Default::default())]
    pub refresh_rate: Option<Iso8601Duration>,
    /// Management information for the registration.
    #[builder(default = Default::default())]
    pub management: Option<RegistrationManagementInfo>,
    /// Additional context source metadata entries.
    #[builder(default = Default::default())]
    pub context_source_info: Option<Vec<KeyValuePair>>,
    /// JSON-LD context associated with this registration.
    #[builder(default = Default::default())]
    pub jsonld_context: Option<Url>,
    /// Unique pseudonym for Via-based loop detection.
    #[builder(default = Default::default())]
    pub context_source_alias: Option<ContextSourcePseudonym>,
    /// Non-standard properties preserved from the original request.
    #[builder(default = Default::default())]
    pub additional_properties: Option<Value>,
}

impl CreateCsrParams {
    /// Keys recognized as standard NGSI-LD CSR properties.
    pub const STANDARD_KEYS: &'static [&'static str] = &[
        "id",
        "type",
        "@context",
        "registrationName",
        "description",
        "information",
        "datasetId",
        "tenant",
        "observationInterval",
        "managementInterval",
        "location",
        "observationSpace",
        "operationSpace",
        "expiresAt",
        "endpoint",
        "accept",
        "mode",
        "operations",
        "scope",
        "refreshRate",
        "management",
        "contextSourceInfo",
        "jsonldContext",
        "contextSourceAlias",
        "status",
        "timesSent",
        "timesFailed",
        "lastSuccess",
        "lastFailure",
        "createdAt",
        "modifiedAt",
    ];
}

/// Typed fragment for CSR updates.
///
/// Every member clause 5.5.8 lets a fragment remove is a
/// [`NullableMember`]: omitted keeps the stored value, `null` clears it and a
/// value replaces it. The members the clause gives no removal semantics carry
/// a plain [`Option`], where `None` simply means the fragment left them alone.
#[derive(Debug, Default, TypedBuilder)]
pub struct CsrUpdateFragment {
    /// Updated registration name.
    #[builder(default = Default::default())]
    pub registration_name: NullableMember<NonEmptyText>,
    /// Updated description.
    #[builder(default = Default::default())]
    pub description: NullableMember<NonEmptyText>,
    /// Updated registration information entries.
    #[builder(default = Default::default())]
    pub information: Option<Vec<RegistrationInfo>>,
    /// Updated dataset identifiers.
    #[builder(default = Default::default())]
    pub dataset_id: NullableMember<Vec<DatasetSelector>>,
    /// Updated tenant identifier.
    #[builder(default = Default::default())]
    pub tenant: NullableMember<TenantName>,
    /// Updated observation interval.
    #[builder(default = Default::default())]
    pub observation_interval: NullableMember<TimeInterval>,
    /// Updated management interval.
    #[builder(default = Default::default())]
    pub management_interval: NullableMember<TimeInterval>,
    /// Updated `location` geometry.
    #[builder(default = Default::default())]
    pub location: NullableMember<Geometry>,
    /// Updated `observationSpace` geometry.
    #[builder(default = Default::default())]
    pub observation_space: NullableMember<Geometry>,
    /// Updated `operationSpace` geometry.
    #[builder(default = Default::default())]
    pub operation_space: NullableMember<Geometry>,
    /// Updated expiration timestamp.
    #[builder(default = Default::default())]
    pub expires_at: NullableMember<DateTime<Utc>>,
    /// Updated endpoint URL.
    #[builder(default = Default::default())]
    pub endpoint: Option<Url>,
    /// Updated accepted MIME type.
    #[builder(default = Default::default())]
    pub accept: NullableMember<MediaTypeBuf>,
    /// Updated forwarding mode.
    #[builder(default = Default::default())]
    pub mode: Option<ForwardingMode>,
    /// Updated supported operations.
    #[builder(default = Default::default())]
    pub operations: NullableMember<Vec<CsrOperation>>,
    /// Updated scope values.
    #[builder(default = Default::default())]
    pub scope: NullableMember<Vec<ScopeBuf>>,
    /// Updated refresh rate duration.
    #[builder(default = Default::default())]
    pub refresh_rate: NullableMember<Iso8601Duration>,
    /// Updated management information.
    #[builder(default = Default::default())]
    pub management: NullableMember<RegistrationManagementInfo>,
    /// Updated context source metadata entries.
    #[builder(default = Default::default())]
    pub context_source_info: NullableMember<Vec<KeyValuePair>>,
    /// Updated JSON-LD context reference.
    #[builder(default = Default::default())]
    pub jsonld_context: NullableMember<Url>,
    /// Updated context source alias pseudonym.
    #[builder(default = Default::default())]
    pub context_source_alias: NullableMember<ContextSourcePseudonym>,
    /// Updated non-standard properties.
    #[builder(default = Default::default())]
    pub additional_properties: Option<Value>,
}

/// How one distributed operation through a registration ended, for the
/// read-only health members of Table 5.2.9-2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardingOutcome {
    /// The source answered the operation.
    Succeeded,
    /// The source failed to answer, or answered with an error.
    Failed,
}

/// Outcome of a mutation against one registration identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationMutation {
    /// One registration was changed.
    Applied,
    /// No registration matched the identifier.
    Missing,
}

/// One backend-neutral page of registrations and its full match count.
#[derive(Debug)]
pub struct RegistrationPage {
    /// Registrations in the requested deterministic page.
    pub registrations: Vec<ContextSourceRegistration>,
    /// Total registrations matching the request before pagination.
    pub total: u64,
}

impl RegistrationMutation {
    /// Require a mutation to have changed the requested registration.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::NotFound`] when the mutation matched no
    /// registration carrying `identifier`.
    pub fn require_applied(self, identifier: &Uri) -> Result<(), RegistrationError> {
        match self {
            RegistrationMutation::Applied => Ok(()),
            RegistrationMutation::Missing => Err(RegistrationError::not_found(identifier)),
        }
    }
}

/// Storing context source registrations (clause 5.9).
///
/// Implementations own two representations of the same facts: the registration
/// payload, which is the authority for every response body, and the routing
/// projection derived from its `information`, which exists solely to preselect
/// candidates for [`RegistrationStore::find_matching`]. Every method that
/// creates, replaces or removes a registration carries the obligation to
/// rewrite the projection for that registration in the same transaction, and
/// the module documentation states the atomicity and superset invariants that
/// obligation has to preserve.
pub trait RegistrationStore: Send + Sync + 'static {
    /// Delete registrations due at or before an explicit cutoff and return their pre-deletion values.
    ///
    /// Removes the routing projection rows of every deleted registration in the
    /// same transaction as the deletion: an expired registration that lingers in
    /// the projection keeps being preselected for distributed operations.
    fn delete_expired<'life0, 'async_trait>(
        &'life0 self,
        cutoff: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Whether a registration with this identifier exists.
    fn exists<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<bool, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Persist a new registration.
    ///
    /// Writes the routing projection derived from `params.information` in the
    /// same transaction as the payload, so the registration never becomes
    /// visible without the rows that make it discoverable.
    fn create<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        id: &'life1 Uri,
        params: &'life2 CreateCsrParams,
    ) -> BoxFuture<'async_trait, Result<(), RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Retrieve a registration by identifier.
    fn get<'life0, 'life1, 'async_trait>(
        &'life0 self,
        id: &'life1 Uri,
    ) -> BoxFuture<'async_trait, Result<Option<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// List registrations using typed, backend-neutral pagination.
    fn list<'life0, 'async_trait>(&'life0 self, pagination: Pagination) -> BoxFuture<'async_trait, Result<RegistrationPage, RegistrationError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Run a compiled registration query, returning matches and the total count.
    fn query_compiled<'life0, 'life1, 'async_trait>(
        &'life0 self,
        compiled: &'life1 CompiledCsrQuery,
    ) -> BoxFuture<'async_trait, Result<RegistrationPage, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Replace a registration with a fully resolved value set.
    ///
    /// Rebuilds the routing projection from `values.information` in the same
    /// transaction as the payload replacement. Because `values` is a resolved
    /// value set rather than a fragment, the projection is replaced wholesale
    /// rather than patched: rows for capabilities the update dropped must not
    /// survive it.
    fn update<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        id: &'life1 Uri,
        values: &'life2 ResolvedCsrUpdate,
    ) -> BoxFuture<'async_trait, Result<RegistrationMutation, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Registrations that may serve an entity type and/or identifier, excluding
    /// expired ones (clause 5.9.2.4).
    ///
    /// This is the discovery preselect and the sole reader of the routing
    /// projection. The result is a superset: callers evaluate the full typed
    /// registration to decide actual membership, so returning a registration
    /// that turns out not to serve the request is permitted, while omitting one
    /// that does is a correctness defect.
    fn find_matching<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        entity_type: Option<&'life1 IriBuf>,
        entity_id: Option<&'life2 Uri>,
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Non-expired proxied registrations — mode `exclusive` or `redirect` —
    /// that may cover `entity_id` and declare one of `attributes`, or declare
    /// no attribute at all (clauses 4.3.6.3, 5.9.2.4, 5.9.3.4).
    ///
    /// Like [`RegistrationStore::find_matching`] this is a preselect over the
    /// routing projection and returns a superset: the typed overlap evaluation
    /// in registration policy decides whether a returned registration actually
    /// collides. A `None` identifier places no entity constraint.
    fn find_overlapping<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        entity_id: Option<&'life1 Uri>,
        attributes: &'life2 [IriBuf],
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// One page of non-expired registrations, keyset-paginated on the identifier.
    ///
    /// `after` is the last identifier of the previous page, exclusive.
    fn find_all_active<'life0, 'life1, 'async_trait>(
        &'life0 self,
        after: Option<&'life1 Uri>,
        limit: u32,
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Delete a registration by identifier.
    ///
    /// Removes that registration's routing projection rows in the same
    /// transaction as the payload.
    fn delete<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<RegistrationMutation, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Record one distributed operation on the registration's health members
    /// (Table 5.2.9-2): `timesSent` always grows, `timesFailed`, `lastFailure`
    /// and `status: failed` on a failure, `lastSuccess` and `status: ok` on a
    /// success. A missing registration is not an error: it may have been
    /// deleted while the operation was in flight.
    fn record_forwarding_outcome<'life0, 'life1, 'async_trait>(
        &'life0 self,
        id: &'life1 Uri,
        outcome: ForwardingOutcome,
        at: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<(), RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
}

#[cfg(test)]
mod tests {
    use crate::{error::RegistrationError, store::RegistrationMutation};
    use andromeda_ngsi_ld_lexicon::uri::Uri;

    /// Parse the fixed valid registration identifier used by mutation tests.
    fn registration_identifier() -> Option<Uri> {
        let parsed = "urn:ngsi-ld:ContextSourceRegistration:test".parse();
        assert!(parsed.is_ok());
        parsed.ok()
    }

    /// An applied store mutation is accepted by registration policy.
    #[test]
    fn applied_mutation_satisfies_the_registration_precondition() {
        let Some(identifier) = registration_identifier() else {
            return;
        };
        assert!(RegistrationMutation::Applied.require_applied(&identifier).is_ok());
    }

    /// A missing store mutation becomes the policy-owned not-found error.
    #[test]
    fn missing_mutation_becomes_registration_not_found() {
        let Some(identifier) = registration_identifier() else {
            return;
        };
        let result = RegistrationMutation::Missing.require_applied(&identifier);
        assert!(matches!(result, Err(RegistrationError::NotFound { identifier: missing }) if missing == identifier));
    }
}
