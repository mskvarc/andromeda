//! A context source that holds one entity, registered for `Vehicle`, which a
//! re-assembly retrieves the entity from.

use crate::reassembly_harness::default_iri;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_test_mocks::{
    callback_path::CallbackPath,
    callback_server::MockResponse,
    context_source_server::ContextSourceServer,
    server_config::MockServerConfig,
};
use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use serde_json::{Value, json};
use std::net::{Ipv4Addr, SocketAddr};
use url::Host;

/// A mock context source and the registration that points the broker at it.
pub(crate) struct AnsweringSource {
    /// The listening server.
    server: ContextSourceServer,
    /// The inclusive `retrieveEntity` registration of `Vehicle` naming it.
    registration: ContextSourceRegistration,
}

impl AnsweringSource {
    /// A source registered as `registration_id`, answering nothing yet.
    pub(crate) async fn start(registration_id: &Identifier) -> AnsweringSource {
        let config = MockServerConfig::new(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), Host::Ipv4(Ipv4Addr::LOCALHOST));
        let server = ContextSourceServer::start(config).await.expect("the mock source starts");
        let root = CallbackPath::parse("/").expect("an absolute path");
        let registration = serde_json::from_value(json!({
            "id": registration_id.as_str(),
            "type": "ContextSourceRegistration",
            "endpoint": server.endpoint(&root).expect("the endpoint resolves").as_str(),
            "mode": "inclusive",
            "operations": ["retrieveEntity"],
            "information": [{"entities": [{"type": default_iri("Vehicle")}]}],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("a valid registration");
        AnsweringSource { server, registration }
    }

    /// The registration naming this source.
    pub(crate) fn registration(&self) -> ContextSourceRegistration {
        self.registration.clone() // clone: the scene takes its own copy of the registration
    }

    /// Answer the next retrieval of `id` with `entity`.
    pub(crate) async fn answer(&self, id: &Identifier, entity: &Value) {
        let path = CallbackPath::parse(format!("/ngsi-ld/v1/entities/{id}")).expect("an absolute path");
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        let body = serde_json::to_vec(entity).expect("the entity writes");
        let response = MockResponse::builder().status(StatusCode::OK).headers(headers).body(Bytes::from(body)).build();
        self.server.enqueue(Method::GET, path, response).await;
    }

    /// How many requests the source received.
    pub(crate) fn requests(&self) -> usize {
        self.server.total_request_count()
    }

    /// Stop listening.
    pub(crate) async fn stop(self) {
        self.server.shutdown().await.expect("the mock source stops");
    }
}
