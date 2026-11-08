//! A context source that holds one entity, registered for `Vehicle`, which a
//! re-assembly retrieves the entity from.

use crate::reassembly_harness::default_iri;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use serde_json::{Value, json};
use wiremock::{
    Mock,
    MockServer,
    ResponseTemplate,
    matchers::{method, path},
};

/// A mock context source and the registration that points the broker at it.
pub(crate) struct AnsweringSource {
    /// The listening server.
    server: MockServer,
    /// The inclusive `retrieveEntity` registration of `Vehicle` naming it.
    registration: ContextSourceRegistration,
}

impl AnsweringSource {
    /// A source registered as `registration_id`, answering nothing yet.
    pub(crate) async fn start(registration_id: &Identifier) -> AnsweringSource {
        let server = MockServer::start().await;
        let registration = serde_json::from_value(json!({
            "id": registration_id.as_str(),
            "type": "ContextSourceRegistration",
            "endpoint": format!("{}/", server.uri()),
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
        self.registration.clone()
    }

    /// Answer every retrieval of `id` with `entity`.
    pub(crate) async fn answer(&self, id: &Identifier, entity: &Value) {
        let body = serde_json::to_vec(entity).expect("the entity writes");
        Mock::given(method("GET"))
            .and(path(format!("/ngsi-ld/v1/entities/{id}")))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
            .mount(&self.server)
            .await;
    }

    /// How many requests the source received.
    pub(crate) async fn requests(&self) -> usize {
        self.server.received_requests().await.expect("the source records its requests").len()
    }
}
