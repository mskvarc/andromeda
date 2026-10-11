//! Retrieving and listing subscriptions (ETSI GS CIM 009 clauses 5.8.3, 5.8.4,
//! 5.11.4 and 5.11.5).

use crate::{
    error::SubscriptionError,
    lifecycle::SubscriptionService,
    response_compaction::compact_for_response,
    subscription_listing::SubscriptionListing,
    variant::SubscriptionVariant,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::subscription::Subscription;
use andromeda_ngsi_ld_query::page_window::PageWindow;

impl<V> SubscriptionService<V>
where
    V: SubscriptionVariant,
{
    /// Retrieve a subscription by its identifier.
    ///
    /// The store-assigned key is dropped here: every caller of this method
    /// renders or edits the resource, and the resource is addressed by its URI.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::NotFound`] when `target` holds no
    /// subscription under `id`, and [`SubscriptionError::Persistence`] when
    /// the store cannot be read.
    pub async fn get(&self, target: &ContextTarget, id: &Identifier) -> Result<Subscription, SubscriptionError> {
        self.repo
            .get(target, id)
            .await?
            .map(|stored| stored.subscription)
            .ok_or_else(|| SubscriptionError::not_found(id))
    }

    /// Retrieve a subscription spelled for the reader of `context`, ready to be
    /// written as a response.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::NotFound`] when `target` holds no
    /// subscription under `id`, [`SubscriptionError::Persistence`] when the
    /// store cannot be read, and [`SubscriptionError::JsonLd`] when `context`
    /// cannot be resolved or the resource cannot be compacted against it.
    pub async fn get_for_response(&self, target: &ContextTarget, id: &Identifier, context: &ContextSpec) -> Result<Subscription, SubscriptionError> {
        let subscription = self.get(target, id).await?;
        Ok(self.presented(subscription, &self.name_compaction(context).await?).await)
    }

    /// List the page `window` selects of the subscriptions, spelled for the
    /// reader of `context`, with the total and the boundary the neighbouring
    /// cursors are built from (clauses 5.5.9 and 6.3.13).
    ///
    /// The listing comes back in ascending identifier order, so the first and
    /// last identifiers are the page's boundary.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::Persistence`] when the store cannot be
    /// read, and [`SubscriptionError::JsonLd`] when `context` cannot be
    /// resolved or one of the listed resources cannot be compacted against it.
    pub async fn list_for_response(
        &self,
        target: &ContextTarget,
        window: &PageWindow,
        context: &ContextSpec,
    ) -> Result<SubscriptionListing, SubscriptionError> {
        let (read, total) = self.repo.list(target, &window.with_lookahead()).await?;
        let (subscriptions, reach) = window.settle(read);
        // clone: the boundary owns the two identifiers the cursor pointers are
        // built from, while the subscriptions themselves are consumed by the
        // compaction below.
        let boundary = subscriptions
            .first()
            .map(|subscription| subscription.id.clone())
            .zip(subscriptions.last().map(|subscription| subscription.id.clone()));
        let names = self.name_compaction(context).await?;
        let mut presented = Vec::with_capacity(subscriptions.len());
        for subscription in subscriptions {
            presented.push(self.presented(subscription, &names).await);
        }
        Ok(SubscriptionListing {
            subscriptions: presented,
            total,
            boundary,
            reach,
        })
    }

    /// A subscription in the reader's terms, with what this kind serves that
    /// the store does not hold.
    async fn presented(&self, subscription: Subscription, names: &NameCompaction) -> Subscription {
        let mut presented = compact_for_response(subscription, names).await;
        self.variant.present(&mut presented);
        presented
    }

    /// The names of one response, compacted against the reader's `context`.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::JsonLd`] when `context` cannot be resolved
    /// or processed: clause 5.5.7 answers a request whose `@context` is not
    /// available with `LdContextNotAvailable` rather than with names spelled
    /// in no context.
    async fn name_compaction(&self, context: &ContextSpec) -> Result<NameCompaction, SubscriptionError> {
        Ok(NameCompaction::new(self.engine.compactor(context).processed_context().await?))
    }
}
