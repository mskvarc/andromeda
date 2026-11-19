//! A Cached `@context` whose origin serves a revised document after the broker downloaded it, for Delete @context with
//! `reload=true` (clause 5.13.5.4).
//!
//! The first download defines `speed`, and an entity written under it holds a `speed`, so whether a later request
//! still reads `speed` by that download shows which document the broker holds. A downloaded `@context` is recorded
//! after the request that made the download, so the entry is read until it appears.

use crate::{
    attribute_matcher::any_attribute,
    broker::BrokerClient,
    context_record,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    remote_context::{RemoteContexts, ServedAnswer},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextId, ContextMetadata, Entity, EntityId, NgsiLdMediaType, StoredContextKind},
};
use url::Url;

/// The path the origin serves the `@context` at.
const SERVED_AT: &str = "ctx/revised.jsonld";

/// The `speed` the entity written under the first download holds.
const SPEED: i64 = 50;

/// A Cached `@context` whose origin serves a revised document to every download after the first.
#[derive(Debug)]
pub struct CachedRevision {
    /// The origin serving the `@context`.
    contexts: RemoteContexts,
    /// The URL the broker downloads the `@context` from.
    url: Url,
    /// The entry the first download made, read before any reload.
    record: ContextMetadata,
    /// The entity written with `speed` under the first download.
    entity: EntityId,
}

impl CachedRevision {
    /// Serves a `@context` defining `speed` to the first download and `revised` to every later one, writes an entity
    /// holding `speed` under it, and waits until the broker records the first download as a Cached entry.
    ///
    /// # Errors
    ///
    /// Fails when the origin cannot serve, the broker refuses the entity, or no Cached entry appears within the
    /// delivery deadline.
    pub async fn recorded(broker: &BrokerClient, namespace: &Namespace, revised: &str) -> Result<CachedRevision> {
        let contexts = RemoteContexts::start().await?;
        let served = ServedAnswer::ok().with_media_type("application/ld+json");
        contexts
            .serve_then(SERVED_AT, (&first_document(namespace), served.clone()), (revised, served))
            .await?;
        let entity = namespace.urn("Vehicle", 1)?;
        let vehicle = Entity::builder()
            .id(entity.clone())
            .entity_type(namespace.entity_type("Vehicle")?)
            .property("speed", SPEED)
            .build()?;
        broker
            .with_context(Context::Multiple(vec![contexts.reference(SERVED_AT)?]))?
            .with_context_delivery(ContextDelivery::LdJsonBody)
            .entities()
            .create(&vehicle)
            .send()
            .await?
            .done()?;
        let url = contexts.url(SERVED_AT)?;
        context_record::cached_under(broker, &url).await?;
        let record = context_record::stored(broker, &context_record::segment_of(&url)).await?;
        Ok(CachedRevision { contexts, url, record, entity })
    }

    /// The URL the broker downloads the `@context` from.
    #[must_use]
    pub const fn url(&self) -> &Url {
        &self.url
    }

    /// The identifier of the Cached entry.
    #[must_use]
    pub const fn local_id(&self) -> &ContextId {
        &self.record.local_id
    }

    /// Requires that the broker still holds the first download: the entry is Cached and created when it was, a
    /// request naming the URL reads `speed` by it, and the origin answered no download but the first and one more.
    ///
    /// # Errors
    ///
    /// Fails when the entry is gone or changed, the entity does not read back with `speed`, or the origin answered
    /// another number of downloads.
    pub async fn verify_kept(&self, broker: &BrokerClient) -> Result<()> {
        let kept = context_record::stored(broker, &self.record.local_id.to_string()).await?;
        verify_that!(kept.kind, eq(&StoredContextKind::Cached))?;
        verify_that!(&kept.created_at, eq(&self.record.created_at))?;
        let reread = broker
            .with_context(Context::Single(self.contexts.reference(SERVED_AT)?))?
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&self.entity)
            .send()
            .await?;
        verify_that!(reread.body(), has_attribute("speed", any_attribute().value(SPEED)))?;
        verify_that!(self.contexts.downloads(SERVED_AT).await?, eq(2))
    }
}

/// The first download: the entity type `Vehicle_<namespace>` and the term `speed`, under the namespace's vocabulary.
fn first_document(namespace: &Namespace) -> String {
    format!(r#"{{"@context": {{"Vehicle_{namespace}": "https://test.andromeda/{namespace}/Vehicle", "speed": "https://test.andromeda/{namespace}/speed"}}}}"#)
}

#[cfg(test)]
mod tests {
    use crate::{cached_revision::first_document, namespace::Namespace};
    use googletest::prelude::*;
    use serde_json::{Value, json};

    #[gtest]
    fn the_first_download_defines_the_entity_type_and_speed_under_the_namespace() -> Result<()> {
        let namespace = Namespace::fresh();
        let document: Value = serde_json::from_str(&first_document(&namespace))?;
        verify_that!(
            document,
            eq(&json!({ "@context": {
                format!("Vehicle_{namespace}"): format!("https://test.andromeda/{namespace}/Vehicle"),
                "speed": format!("https://test.andromeda/{namespace}/speed"),
            } }))
        )
    }
}
