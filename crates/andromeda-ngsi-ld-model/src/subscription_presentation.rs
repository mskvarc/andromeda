//! A Subscription as a client reads it: the stored resource beside its names spelled in the reader's terms.

use crate::{subscription::Subscription, written_subscription_terms::WrittenSubscriptionTerms};

/// A stored [`Subscription`] and the written spelling of every member of it that holds names.
///
/// The members of `subscription` that `terms` repeats are the expanded forms the broker matches by; a client is served `terms` for them and `subscription` for everything else.
#[derive(Debug, Clone)]
pub struct SubscriptionPresentation {
    /// The stored resource, whose lifecycle and delivery members are served as they stand.
    pub subscription: Subscription,
    /// The names of the resource in the reader's terms.
    pub terms: WrittenSubscriptionTerms,
}
