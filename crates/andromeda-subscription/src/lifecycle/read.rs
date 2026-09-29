//! Retrieving and listing subscriptions (ETSI GS CIM 009 clauses 5.8.3, 5.8.4,
//! 5.11.4 and 5.11.5).

use crate::{error::SubscriptionError, keyset_listing::KeysetListing, lifecycle::SubscriptionService, variant::SubscriptionVariant};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{compaction::NameCompactor, context_spec::ContextSpec};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::subscription::Subscription;
use andromeda_ngsi_ld_query::cursor::PageCursor;
use serde_json::Value;

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

    /// Retrieve a subscription and serialize it for an HTTP response.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::NotFound`] when `target` holds no
    /// subscription under `id`, [`SubscriptionError::Persistence`] when the
    /// store cannot be read, and [`SubscriptionError::JsonLd`] when `context`
    /// cannot be resolved or the resource cannot be compacted against it.
    pub async fn get_for_response(&self, target: &ContextTarget, id: &Identifier, context: &ContextSpec) -> Result<Value, SubscriptionError> {
        let subscription = self.get(target, id).await?;
        self.variant
            .serialize_for_response(&subscription, &NameCompactor::new(&self.engine, context))
            .await
    }

    /// List subscriptions with pagination and serialize them for an HTTP response.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::Persistence`] when the store cannot be
    /// read, and [`SubscriptionError::JsonLd`] when `context` cannot be
    /// resolved or one of the listed resources cannot be compacted against it.
    pub async fn list_for_response(
        &self,
        target: &ContextTarget,
        limit: Option<i64>,
        offset: Option<i64>,
        context: &ContextSpec,
    ) -> Result<(Vec<Value>, i64), SubscriptionError> {
        let (subscriptions, total) = self.repo.list(target, limit, offset).await?;
        let names = NameCompactor::new(&self.engine, context);
        let mut compacted = Vec::with_capacity(subscriptions.len());
        for subscription in subscriptions {
            compacted.push(self.variant.serialize_for_response(&subscription, &names).await?);
        }
        Ok((compacted, total))
    }

    /// List subscriptions by keyset for a stable-pagination response
    /// (clause 5.5.9.1), serializing them and reporting the boundary the
    /// neighbouring cursors are built from.
    ///
    /// `cursor` is the opaque position the request continued from, `None` on the
    /// first page. The listing comes back in ascending identifier order (the
    /// store reverses a backward or from-end page), so the first and last
    /// identifiers are the page's boundary; they are captured before the
    /// resources are serialized, which drops the typed identifier.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::Persistence`] when the store cannot be
    /// read, and [`SubscriptionError::JsonLd`] when `context` cannot be
    /// resolved or one of the listed resources cannot be compacted against it.
    pub async fn list_keyset_for_response(
        &self,
        target: &ContextTarget,
        cursor: Option<&PageCursor>,
        limit: i64,
        context: &ContextSpec,
    ) -> Result<KeysetListing, SubscriptionError> {
        let (subscriptions, total) = self.repo.list_keyset(target, cursor, limit).await?;
        // clone: the boundary owns the two identifiers the cursor pointers are
        // built from, while the subscriptions themselves are consumed by the
        // serialization below.
        let boundary = subscriptions
            .first()
            .map(|subscription| subscription.id.clone())
            .zip(subscriptions.last().map(|subscription| subscription.id.clone()));
        let names = NameCompactor::new(&self.engine, context);
        let mut compacted = Vec::with_capacity(subscriptions.len());
        for subscription in subscriptions {
            compacted.push(self.variant.serialize_for_response(&subscription, &names).await?);
        }
        Ok(KeysetListing {
            subscriptions: compacted,
            total,
            boundary,
        })
    }
}
