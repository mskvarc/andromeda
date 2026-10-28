//! A context source that accepts every subscription placed at it, registered
//! for `Vehicle` and one attribute named by a user term, which a placement of
//! a reduced copy is read off.

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

/// The attribute the source registers, as the user term `speed` of
/// `https://example.org/terms.jsonld` expands.
pub(crate) const SPEED: &str = "https://example.org/terms/speed";

/// A mock context source and the registration that points the broker at it.
pub(crate) struct SubscribingSource {
    /// The listening server.
    server: MockServer,
    /// The registration naming the server, as the registration store keeps it.
    registration: Value,
}

impl SubscribingSource {
    /// A source registered as `registration_id`, inclusive, advertising the
    /// creation of subscriptions for `Vehicle` and [`SPEED`].
    pub(crate) async fn start(registration_id: &Identifier) -> SubscribingSource {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ngsi-ld/v1/subscriptions"))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;
        let registration = json!({
            "id": registration_id.as_str(),
            "type": "ContextSourceRegistration",
            "endpoint": format!("{}/", server.uri()),
            "mode": "inclusive",
            "operations": ["createSubscription"],
            "information": [{"entities": [{"type": default_iri("Vehicle")}], "propertyNames": [SPEED]}],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        });
        SubscribingSource { server, registration }
    }

    /// The registration as the registration store keeps it, its entity type
    /// expanded.
    pub(crate) fn stored(&self) -> ContextSourceRegistration {
        serde_json::from_value(self.registration.clone()).expect("a valid registration") // clone: the decoding consumes its own copy
    }

    /// The registration as a companion notification writes it, compacted under
    /// the core context: the entity type is the term the default context
    /// expands, and the attribute no term of the core context names stays its
    /// full IRI.
    pub(crate) fn notified(&self) -> Value {
        let mut notified = self.registration.clone(); // clone: the notified form is a second document
        notified["information"][0]["entities"][0]["type"] = json!("Vehicle");
        notified
    }

    /// The bodies of the subscriptions placed at the source, in the order
    /// they arrived.
    pub(crate) async fn placed(&self) -> Vec<Value> {
        self.server
            .received_requests()
            .await
            .expect("the source records its requests")
            .iter()
            .map(|request| serde_json::from_slice(&request.body).expect("a placed subscription is JSON"))
            .collect()
    }
}
