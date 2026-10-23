//! Registration persistence port and commands.
//!
//! `update` takes a fully resolved value set rather than a fragment: clause
//! 5.10.4 defines a registration update as a merge against the stored
//! registration, and that merge is policy. The store applies a decided row.
//!
//! # Information a backend can preselect against
//!
//! Registration `information` (clause 5.2.9) is a nested structure: each item
//! carries entity selectors, property names and relationship names. Held as a
//! single document it cannot be preselected against, so implementations store
//! it in a form the matching reads answer from directly, and every mutating
//! method of [`RegistrationStore`] writes, replaces or removes that form with
//! the rest of the registration. This is part of the port contract, not an
//! implementation detail.
//!
//! Two invariants define the contract.
//!
//! Atomicity: a registration's `information` is written, replaced or removed
//! in the same transaction as the rest of the registration. No intermediate
//! state in which the two disagree may ever be observable by a concurrent
//! reader, and a failed mutation leaves both untouched.
//!
//! Superset preselection: [`RegistrationStore::find_matching`] and
//! [`RegistrationStore::find_overlapping`] narrow candidates. The typed
//! residual evaluation over the returned registrations remains the oracle
//! deciding final membership, so a preselect may return registrations that do
//! not ultimately match (that costs residual CPU) but must never omit one that
//! does. A false negative silently drops a context source from distributed
//! operations.
//!
//! # Where the stored form is derived
//!
//! The port supplies no separate selector-summary type, and deliberately so.
//! JSON-LD expansion of `information` happens in the service layer before any
//! store call, so entity types, property names and relationship names reach
//! this port already expanded to IRIs inside the typed
//! [`RegistrationInfo`](andromeda_ngsi_ld_model::registration::info::RegistrationInfo)
//! values carried by [`CreateCsrParams`](crate::store::CreateCsrParams) and
//! [`ResolvedCsrUpdate`](crate::resolved_update::ResolvedCsrUpdate). Those
//! values are the complete and authoritative input; deriving a second
//! domain-side summary type would duplicate them and would force the domain to
//! know how a particular backend interns names or encodes operations.
//! Implementations therefore derive their stored form store-side from the
//! typed `information` they are handed, and own its encoding.

pub mod create_params;
pub mod expired_deletion;
pub mod forwarding_outcome;
pub mod mutation;
pub mod page;
pub mod update_fragment;
pub mod written_registration;

use crate::{
    error::RegistrationError,
    resolved_update::ResolvedCsrUpdate,
    store::{
        create_params::CreateCsrParams,
        expired_deletion::ExpiredDeletion,
        forwarding_outcome::ForwardingOutcome,
        mutation::RegistrationMutation,
        page::RegistrationPage,
        written_registration::WrittenRegistration,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{csource_change_revision::CsourceChangeRevision, registration::ContextSourceRegistration};
use andromeda_ngsi_ld_query::{
    csr::{compiled::CompiledCsrQuery, expanded_entity_specification::ExpandedEntitySpecification},
    page_window::PageWindow,
};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;

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
    /// Removes the stored `information` of every deleted registration in the
    /// same transaction as the deletion. The sweep draws one change revision
    /// for every deletion it commits.
    fn delete_expired<'life0, 'async_trait>(&'life0 self, cutoff: DateTime<Utc>) -> BoxFuture<'async_trait, Result<ExpiredDeletion, RegistrationError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// The registrations due at or before an explicit cutoff, read without
    /// deleting them: what an expiry sweep is about to remove.
    fn find_expired<'life0, 'async_trait>(
        &'life0 self,
        cutoff: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Whether a registration with this identifier exists.
    fn exists<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Identifier) -> BoxFuture<'async_trait, Result<bool, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Persist a new registration, returning it as written, read in the same
    /// transaction, with the change revision the write drew.
    ///
    /// Writes `params.information` in the same transaction as the rest of the
    /// registration, so the registration never becomes visible without what
    /// makes it discoverable.
    fn create<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
        params: &'life2 CreateCsrParams,
    ) -> BoxFuture<'async_trait, Result<WrittenRegistration, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Retrieve a registration by identifier.
    fn get<'life0, 'life1, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Run a compiled registration query over the page `window` selects of the
    /// matches in identifier order, returning the page ascending by identifier
    /// and the total count of matches (clause 5.10.2).
    fn query_compiled<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        compiled: &'life1 CompiledCsrQuery,
        window: &'life2 PageWindow,
    ) -> BoxFuture<'async_trait, Result<RegistrationPage, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Replace a registration with a fully resolved value set, returning it
    /// as written, read in the same transaction, with the change revision the
    /// write drew.
    ///
    /// Replaces the stored `information` with `values.information` in the same
    /// transaction as the rest of the registration. Because `values` is a
    /// resolved value set rather than a fragment, the `information` is replaced
    /// wholesale rather than patched: capabilities the update dropped must not
    /// survive it.
    fn update<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
        values: &'life2 ResolvedCsrUpdate,
    ) -> BoxFuture<'async_trait, Result<RegistrationMutation<WrittenRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Registrations whose `information` may match `entities` by clause 5.12,
    /// excluding expired ones (clause 5.9.2.4).
    ///
    /// This is the forwarding preselect. Every member of `entities` narrows
    /// it: the type selector, each of the identifiers and the identifier
    /// pattern. The result is a superset: callers evaluate the full typed
    /// registration to decide actual membership, so returning a registration
    /// that turns out not to serve the request is permitted, while omitting one
    /// that does is a correctness defect.
    fn find_matching<'life0, 'life1, 'async_trait>(
        &'life0 self,
        entities: ExpandedEntitySpecification<'life1>,
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Non-expired proxied registrations — mode `exclusive` or `redirect` —
    /// that may cover `entity_id` and declare one of `attributes`, or declare
    /// no attribute at all (clauses 4.3.6.3, 5.9.2.4, 5.9.3.4).
    ///
    /// Like [`RegistrationStore::find_matching`] this is a preselect over the
    /// stored `information` and returns a superset: the typed overlap evaluation
    /// in registration policy decides whether a returned registration actually
    /// collides. A `None` identifier places no entity constraint.
    fn find_overlapping<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        entity_id: Option<&'life1 Identifier>,
        attributes: &'life2 [AttributeName],
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
        after: Option<&'life1 Identifier>,
        limit: u32,
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Delete a registration by identifier, returning the change revision the
    /// deletion drew.
    ///
    /// Removes that registration's stored `information` in the same
    /// transaction as the rest of the registration.
    fn delete<'life0, 'life1, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<RegistrationMutation<CsourceChangeRevision>, RegistrationError>>
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
        id: &'life1 Identifier,
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
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::csource_change_revision::CsourceChangeRevision;
    use pretty_assertions::assert_eq;

    /// Parse the fixed valid registration identifier used by mutation tests.
    fn registration_identifier() -> Option<Identifier> {
        let parsed = "urn:ngsi-ld:ContextSourceRegistration:test".parse();
        assert!(parsed.is_ok());
        parsed.ok()
    }

    /// An applied store mutation is accepted by registration policy, which
    /// keeps the revision the write drew.
    #[test]
    fn applied_mutation_satisfies_the_registration_precondition() {
        let Some(identifier) = registration_identifier() else {
            return;
        };
        let revision = CsourceChangeRevision::from(3);
        assert_eq!(RegistrationMutation::Applied(revision).require_applied(&identifier).ok(), Some(revision));
    }

    /// A missing store mutation becomes the policy-owned not-found error.
    #[test]
    fn missing_mutation_becomes_registration_not_found() {
        let Some(identifier) = registration_identifier() else {
            return;
        };
        let result = RegistrationMutation::<CsourceChangeRevision>::Missing.require_applied(&identifier);
        assert!(matches!(result, Err(RegistrationError::NotFound { identifier: missing }) if missing == identifier));
    }
}
