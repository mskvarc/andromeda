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

use crate::{error::RegistrationError, resolved_update::ResolvedCsrUpdate};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, scope::grammar::ScopeBuf, tenant::TenantName, uri::Uri};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    registration::{ContextSourceRegistration, CsrOperation, ForwardingMode, RegistrationInfo},
};
use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrQuery, pagination::Pagination};
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
pub struct CreateCsrParams<'a> {
    /// Human-readable name of the registration.
    #[builder(default)]
    pub registration_name: Option<&'a str>,
    /// Free-text description of the registration.
    #[builder(default)]
    pub description: Option<&'a str>,
    /// Registration information entries describing provided entities/attributes.
    #[builder(default)]
    pub information: Option<Vec<RegistrationInfo>>,
    /// Tenant identifier for multi-tenancy.
    #[builder(default)]
    pub tenant: Option<TenantName>,
    /// Expected interval between observations.
    #[builder(default)]
    pub observation_interval: Option<&'a Value>,
    /// Expected interval between management interactions.
    #[builder(default)]
    pub management_interval: Option<&'a Value>,
    /// The `location` geometry.
    #[builder(default)]
    pub location: Option<Geometry>,
    /// The `observationSpace` geometry.
    #[builder(default)]
    pub observation_space: Option<Geometry>,
    /// The `operationSpace` geometry.
    #[builder(default)]
    pub operation_space: Option<Geometry>,
    /// Expiration timestamp for the registration.
    #[builder(default)]
    pub expires_at: Option<DateTime<Utc>>,
    /// Base endpoint URL of the context source.
    #[builder(default)]
    pub endpoint: Option<Url>,
    /// Preferred MIME type for responses from the context source.
    #[builder(default)]
    pub accept: Option<MediaTypeBuf>,
    /// Forwarding mode for distributed operations.
    pub mode: ForwardingMode,
    /// NGSI-LD operations supported by the context source.
    #[builder(default)]
    pub operations: Option<Vec<CsrOperation>>,
    /// Scope values for the registration.
    #[builder(default)]
    pub scope: Option<Vec<ScopeBuf>>,
    /// ISO 8601 duration between context source refreshes.
    #[builder(default)]
    pub refresh_rate: Option<Iso8601Duration>,
    /// Management information for the registration.
    #[builder(default)]
    pub management: Option<&'a Value>,
    /// Additional context source metadata entries.
    #[builder(default)]
    pub context_source_info: Option<Vec<KeyValuePair>>,
    /// JSON-LD context associated with this registration.
    #[builder(default)]
    pub jsonld_context: Option<Url>,
    /// Unique pseudonym for Via-based loop detection.
    #[builder(default)]
    pub context_source_alias: Option<ContextSourcePseudonym>,
    /// Non-standard properties preserved from the original request.
    #[builder(default)]
    pub additional_properties: Option<Value>,
}

impl CreateCsrParams<'_> {
    /// Keys recognized as standard NGSI-LD CSR properties.
    pub const STANDARD_KEYS: &'static [&'static str] = &[
        "id",
        "type",
        "@context",
        "registrationName",
        "description",
        "information",
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

/// Typed fragment for CSR updates. `None` = field not provided (keep existing).
/// `Some(None)` = explicitly null (clear field). `Some(Some(v))` = new value.
#[derive(Debug, Default, TypedBuilder)]
pub struct CsrUpdateFragment {
    /// Updated registration name.
    #[builder(default)]
    pub registration_name: Option<Option<String>>,
    /// Updated description.
    #[builder(default)]
    pub description: Option<Option<String>>,
    /// Updated registration information entries.
    #[builder(default)]
    pub information: Option<Vec<RegistrationInfo>>,
    /// Updated tenant identifier.
    #[builder(default)]
    pub tenant: Option<Option<TenantName>>,
    /// Updated observation interval.
    #[builder(default)]
    pub observation_interval: Option<Option<Value>>,
    /// Updated management interval.
    #[builder(default)]
    pub management_interval: Option<Option<Value>>,
    /// Updated `location` geometry (`Some(None)` clears, `Some(Some(_))` replaces).
    #[builder(default)]
    pub location: Option<Option<Geometry>>,
    /// Updated `observationSpace` geometry.
    #[builder(default)]
    pub observation_space: Option<Option<Geometry>>,
    /// Updated `operationSpace` geometry.
    #[builder(default)]
    pub operation_space: Option<Option<Geometry>>,
    /// Updated expiration timestamp.
    #[builder(default)]
    pub expires_at: Option<Option<DateTime<Utc>>>,
    /// Updated endpoint URL.
    #[builder(default)]
    pub endpoint: Option<Url>,
    /// Updated accepted MIME type.
    #[builder(default)]
    pub accept: Option<Option<MediaTypeBuf>>,
    /// Updated forwarding mode.
    #[builder(default)]
    pub mode: Option<ForwardingMode>,
    /// Updated supported operations.
    #[builder(default)]
    pub operations: Option<Option<Vec<CsrOperation>>>,
    /// Updated scope values.
    #[builder(default)]
    pub scope: Option<Option<Vec<ScopeBuf>>>,
    /// Updated refresh rate duration.
    #[builder(default)]
    pub refresh_rate: Option<Option<Iso8601Duration>>,
    /// Updated management information.
    #[builder(default)]
    pub management: Option<Option<Value>>,
    /// Updated context source metadata entries.
    #[builder(default)]
    pub context_source_info: Option<Option<Vec<KeyValuePair>>>,
    /// Updated JSON-LD context reference.
    #[builder(default)]
    pub jsonld_context: Option<Option<Url>>,
    /// Updated context source alias pseudonym.
    #[builder(default)]
    pub context_source_alias: Option<Option<ContextSourcePseudonym>>,
    /// Updated non-standard properties.
    #[builder(default)]
    pub additional_properties: Option<Value>,
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
    pub fn require_applied(self, identifier: &Uri) -> Result<(), RegistrationError> {
        match self {
            RegistrationMutation::Applied => Ok(()),
            RegistrationMutation::Missing => Err(RegistrationError::not_found(identifier)),
        }
    }
}

/// Storing context source registrations (clause 5.10).
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
    fn create<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        id: &'life1 Uri,
        params: &'life2 CreateCsrParams<'life3>,
    ) -> BoxFuture<'async_trait, Result<(), RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
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
    /// expired ones (clause 5.10.2).
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
