//! What one kind of subscription does differently from the other.
//!
//! Entity subscriptions (clause 5.8) and Context Source Registration
//! Subscriptions (clause 5.11) share their whole lifecycle -- clause 5.11.2.4
//! says so outright, "as described in clause 5.8.1.4, restricted to the local
//! case" -- and differ in four places: which terms are expanded, how the
//! resource is rendered back, what an update additionally has to satisfy, and
//! which change event the store broadcast carries. Naming those four is what
//! lets `crate::lifecycle` be written once.

use crate::{change::UpsertKind, error::SubscriptionError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{compaction::NameCompactor, context_spec::ContextSpec};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::subscription::Subscription;
use futures::future::BoxFuture;
use serde_json::Value;
use url::Url;

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

// ---------------------------------------------------------------------------
// Variant trait
// ---------------------------------------------------------------------------

/// Trait for variant-specific subscription behaviour (entity vs. CSR subscriptions).
pub trait SubscriptionVariant: Send + Sync {
    /// The change-event type emitted over the broadcast channel.
    type Change: Clone + Send + 'static;

    /// Expand subscription terms in-place (context hosting, IRI expansion).
    fn expand_terms<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        payload: &'life1 mut Value,
        context: &'life2 ContextSpec,
        base_url: Option<&'life3 Url>,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;

    /// Serialize a subscription for an HTTP response (compaction, fixups),
    /// compacting its names with the one [`NameCompactor`] the response uses.
    fn serialize_for_response<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        subscription: &'life1 Subscription,
        names: &'life2 NameCompactor<'life3>,
    ) -> BoxFuture<'async_trait, Result<Value, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;

    /// Additional update-specific validation (e.g. `expiresAt` future check).
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::Validation`] when the fragment breaks a
    /// rule that holds for this variant alone, such as an `expiresAt` that is
    /// already in the past.
    fn validate_update_extra(&self, payload: &Value) -> Result<(), SubscriptionError>;

    /// Select how this subscription variant handles the `datasetId` field.
    fn dataset_id_policy(&self) -> DatasetIdPolicy;

    /// Build an "upserted" change event for a subscription that was created or
    /// updated, as `kind` says.
    fn make_upserted(&self, target: ContextTarget, id: Uri, kind: UpsertKind) -> Self::Change;

    /// Build a "deleted" change event.
    fn make_deleted(&self, target: ContextTarget, id: Uri) -> Self::Change;
}
