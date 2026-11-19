//! A remote `@context` whose origin serves a document that is not valid JSON-LD, then a corrected one, for the
//! requests that name it (clauses 5.5.6 and 5.13.1).
//!
//! Whether the refused download left a Cached entry is read before the origin is corrected: the corrected download is
//! recorded as one, after the request that made it.

use crate::{answered_status::AnsweredStatus, broker::BrokerClient, context_record, namespace::Namespace, remote_context::RemoteContexts};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, NgsiLdMediaType},
};
use std::fmt::Debug;
use url::Url;

/// The path the origin serves the `@context` at.
const SERVED_AT: &str = "ctx/corrected.jsonld";

/// A remote `@context` whose origin serves a document that is not valid JSON-LD until it is corrected.
#[derive(Debug)]
pub struct CorrectedContext {
    /// The origin serving the `@context`.
    contexts: RemoteContexts,
    /// The URL the broker downloads the `@context` from.
    url: Url,
}

impl CorrectedContext {
    /// Serves `invalid` at a URL of an origin of its own.
    ///
    /// # Errors
    ///
    /// Fails when the origin cannot start or serve.
    pub async fn serving(invalid: &str) -> Result<CorrectedContext> {
        let contexts = RemoteContexts::start().await?;
        let url = contexts.serve(SERVED_AT, invalid, Some("application/ld+json")).await?;
        Ok(CorrectedContext { contexts, url })
    }

    /// The URL the broker downloads the `@context` from.
    #[must_use]
    pub const fn url(&self) -> &Url {
        &self.url
    }

    /// Query Entities of the entity type `Vehicle` of `namespace` under a `Link` naming the `@context`, as answered.
    ///
    /// # Errors
    ///
    /// Fails when the request cannot be written.
    pub async fn query(&self, broker: &BrokerClient, namespace: &Namespace) -> Result<impl AnsweredStatus + Debug> {
        Ok(broker
            .with_context(Context::Single(self.contexts.reference(SERVED_AT)?))?
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .query()
            .entity_type(&namespace.entity_type("Vehicle")?)
            .send()
            .await)
    }

    /// Requires that the refused download left no entry and is downloaded again: Serve @context with `details=true`
    /// for the URL answers 404 `ResourceNotFound`, and once the origin serves a valid `@context` there, the next
    /// Query Entities naming it succeeds after a second download, which the broker records as a Cached entry.
    ///
    /// # Errors
    ///
    /// Fails when an entry was recorded for the refused download, the corrected `@context` is not used or not
    /// downloaded again, or no Cached entry appears for it within the delivery deadline.
    pub async fn verify_downloaded_again(&self, broker: &BrokerClient, namespace: &Namespace) -> Result<()> {
        context_record::unrecorded(broker, &self.url).await?;
        self.contexts
            .serve(SERVED_AT, &corrected_document(namespace), Some("application/ld+json"))
            .await?;
        broker
            .with_context(Context::Single(self.contexts.reference(SERVED_AT)?))?
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .query()
            .entity_type(&namespace.entity_type("Vehicle")?)
            .send()
            .await?;
        verify_that!(self.contexts.downloads(SERVED_AT).await?, eq(2))?;
        context_record::cached_under(broker, &self.url).await
    }
}

/// The corrected document: the term `speed` under the namespace's vocabulary.
fn corrected_document(namespace: &Namespace) -> String {
    format!(r#"{{"@context": {{"speed": "https://test.andromeda/{namespace}/speed"}}}}"#)
}

#[cfg(test)]
mod tests {
    use crate::{corrected_context::corrected_document, namespace::Namespace};
    use googletest::prelude::*;
    use serde_json::{Value, json};

    #[gtest]
    fn the_corrected_document_defines_speed_under_the_namespace() -> Result<()> {
        let namespace = Namespace::fresh();
        let document: Value = serde_json::from_str(&corrected_document(&namespace))?;
        verify_that!(
            document,
            eq(&json!({ "@context": { "speed": format!("https://test.andromeda/{namespace}/speed") } }))
        )
    }
}
