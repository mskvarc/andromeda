//! Cache-invalidation signals for subscription and CSR-subscription CRUD.
//!
//! One tightly-related family: both announce that a subscription registry entry
//! was upserted or deleted, so the matching cache drops its compiled copy.

use andromeda_ngsi_ld_lexicon::uri::Uri;

/// Signals subscription CRUD changes for cache invalidation.
#[derive(Clone, Debug)]
pub enum SubscriptionChange {
    /// The subscription with this id was created or replaced.
    Upserted(Uri),
    /// The subscription with this id was deleted.
    Deleted(Uri),
}

/// Signals CSR subscription CRUD changes for cache invalidation.
#[derive(Clone, Debug)]
pub enum CsourceSubscriptionChange {
    /// The CSR subscription with this id was created or replaced.
    Upserted(Uri),
    /// The CSR subscription with this id was deleted.
    Deleted(Uri),
}
