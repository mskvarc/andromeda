//! The user `@context`s of an Annex C example: documents the test serves for the broker to download, referenced in
//! a `Link` header as the examples reference theirs, and the reads that compare an answer with the printed one.

use andromeda_test_suite::{
    broker::BrokerClient,
    config::SuiteConfig,
    namespace::Namespace,
    problem::answered,
    remote_context::RemoteContexts,
    wire::{self, json_body},
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use iri_rs::IriBuf;
use ngsi_ld::{
    client::{ClientError, Received},
    model::{Context, EntityId, NgsiLdMediaType},
};
use serde_json::Value;
use std::{error::Error, fmt::Debug};
use url::Url;

/// The server of one test's `@context` documents, each at `annex-c/<ns>/<name>`.
#[derive(Debug)]
pub(crate) struct ServedContexts {
    /// The server holding the documents.
    remote: RemoteContexts,
    /// The namespace whose directory holds the documents.
    namespace: Namespace,
}

impl ServedContexts {
    /// A server serving nothing yet for `namespace`.
    pub(crate) async fn start(namespace: &Namespace) -> Result<ServedContexts> {
        Ok(ServedContexts {
            remote: RemoteContexts::start().await?,
            // The server names its paths after the namespace for its whole life.
            namespace: namespace.clone(),
        })
    }

    /// Serves `document` as `application/ld+json` at `annex-c/<ns>/<name>` and answers its URL.
    pub(crate) async fn serve(&self, name: &str, document: &Value) -> Result<Url> {
        Ok(self
            .remote
            .serve(
                &format!("annex-c/{}/{name}", self.namespace),
                &document.to_string(),
                Some("application/ld+json"),
            )
            .await?)
    }

    /// Stops serving the documents.
    pub(crate) fn stop(self) {
        self.remote.stop();
    }
}

/// `client` sending `context` in a `Link` header, its bodies as `application/json`.
pub(crate) fn linked(client: &BrokerClient, context: &Url) -> Result<BrokerClient> {
    Ok(client.with_context(Context::from(IriBuf::new(context.to_string())?))?)
}

/// The core `@context` the brokers answer with.
pub(crate) fn core_context() -> Result<&'static Url> {
    Ok(SuiteConfig::get().map_err(Clone::clone)?.core_context())
}

/// Retrieves `id` through `client` as `application/ld+json`, requires 200 and answers the body.
pub(crate) async fn retrieved_ld(client: &BrokerClient, id: &EntityId) -> Result<Value> {
    ok_body(
        client
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .entities()
            .retrieve(id)
            .send()
            .await,
    )
}

/// Requires `received` to have answered 200 and answers its body.
pub(crate) fn ok_body<V: Debug, E: Debug>(received: Result<Received<V>, ClientError<E>>) -> Result<Value>
where
    ClientError<E>: Error + Send + Sync + 'static,
{
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(json_body(&received?)?)
}

/// Retrieves `id` through `client` as `application/ld+json` without reading the answer as an Entity, requires 200
/// and answers the body: for an Entity the client's model refuses, such as a `location` that is not a `GeoProperty`.
pub(crate) async fn retrieved_document(client: &BrokerClient, id: &EntityId) -> Result<Value> {
    let request = wire::request_to(client, Method::GET, &format!("entities/{id}"))?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::OK))?;
    Ok(serde_json::from_slice(answer?.body())?)
}
