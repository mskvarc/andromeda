//! What one kind of subscription does differently from the other.
//!
//! Entity subscriptions (clause 5.8) and Context Source Registration
//! Subscriptions (clause 5.11) share their whole lifecycle (clause 5.11.2.4
//! says so outright, "as described in clause 5.8.1.4, restricted to the local
//! case") and differ in four places: which terms are expanded, how the
//! resource is presented, what an update additionally has to satisfy, and
//! which change event the store broadcast carries. Naming those four is what
//! lets `crate::lifecycle` be written once.

use crate::{change::UpsertKind, error::SubscriptionError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{subscription::Subscription, subscription_request::SubscriptionRequest};
use futures::future::BoxFuture;

/// Controls whether a subscription variant persists clause 5.8 `datasetId`
/// filtering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatasetIdPolicy {
    /// Retain validated dataset identifiers for entity subscriptions.
    Retain,
    /// Discard dataset identifiers for context-source subscriptions, whose
    /// contract does not define this filter.
    Discard,
}

/// Trait for variant-specific subscription behaviour (entity vs. CSR subscriptions).
pub trait SubscriptionVariant: Send + Sync {
    /// The change-event type emitted over the broadcast channel.
    type Change: Clone + Send + 'static;

    /// Expand the request's terms in place under `context` (clause 5.5.7).
    ///
    /// Initialising `jsonldContext` is not part of it: that is a clause 5.8.1.4
    /// creation step, and an update leaves the stored member as it is unless
    /// the fragment names one (clauses 5.8.2.4 and 5.5.8).
    fn expand_terms<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        request: &'life1 mut SubscriptionRequest,
        context: &'life2 ContextSpec,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Complete a subscription, already spelled in the reader's terms, with
    /// what this kind of subscription serves that the store does not hold.
    fn present(&self, subscription: &mut Subscription);

    /// Refuse the members of a create body or update fragment that Table
    /// 5.2.12-1 reserves for the other kind of subscription.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::Validation`] naming the member this kind
    /// of subscription does not carry.
    fn validate_members(&self, request: &SubscriptionRequest) -> Result<(), SubscriptionError>;

    /// Additional update-specific validation (e.g. `expiresAt` future check).
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::Validation`] when the fragment breaks a
    /// rule that holds for this variant alone, such as an `expiresAt` that is
    /// already in the past.
    fn validate_update_extra(&self, request: &SubscriptionRequest) -> Result<(), SubscriptionError>;

    /// Select how this subscription variant handles the `datasetId` field.
    fn dataset_id_policy(&self) -> DatasetIdPolicy;

    /// Build an "upserted" change event for a subscription that was created or
    /// updated, as `kind` says.
    fn make_upserted(&self, target: ContextTarget, id: Identifier, kind: UpsertKind) -> Self::Change;

    /// Build a "deleted" change event.
    fn make_deleted(&self, target: ContextTarget, id: Identifier) -> Self::Change;
}
