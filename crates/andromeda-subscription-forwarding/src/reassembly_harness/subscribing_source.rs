//! A context source that answers every subscription placed at it, accepting
//! it unless told otherwise, and accepts every deletion of one, registered for
//! `Vehicle` and one attribute named by a user term, which a placement of a
//! reduced copy is read off.

use crate::reassembly_harness::default_iri;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use http::Method;
use serde_json::{Value, json};
use std::time::Duration;
use wiremock::{
    Mock,
    MockServer,
    Request,
    Respond,
    ResponseTemplate,
    matchers::{method, path, path_regex},
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
    /// creation and deletion of subscriptions for `Vehicle` and [`SPEED`].
    pub(crate) async fn start(registration_id: &Identifier) -> SubscribingSource {
        SubscribingSource::start_answering(registration_id, ResponseTemplate::new(201)).await
    }

    /// The source [`SubscribingSource::start`] starts, answering a placed
    /// subscription with `placement`.
    pub(crate) async fn start_answering(registration_id: &Identifier, placement: impl Respond + 'static) -> SubscribingSource {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ngsi-ld/v1/subscriptions"))
            .respond_with(placement)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path_regex("^/ngsi-ld/v1/subscriptions/[^/]+$"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let registration = json!({
            "id": registration_id.as_str(),
            "type": "ContextSourceRegistration",
            "endpoint": format!("{}/", server.uri()),
            "mode": "inclusive",
            "operations": ["createSubscription", "deleteSubscription"],
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
        serde_json::from_value(self.registration.clone()).expect("a valid registration")
    }

    /// The registration as the registration store keeps it, with a
    /// `management.timeout` of `timeout` (clause 5.2.34).
    pub(crate) fn stored_timing_out_after(&self, timeout: Duration) -> ContextSourceRegistration {
        let mut registration = self.registration.clone();
        registration["management"] = json!({"timeout": timeout.as_millis()});
        serde_json::from_value(registration).expect("a valid registration")
    }

    /// The registration with its endpoint moved to the discard port of the
    /// loopback address, where nothing listens, so a forward to it is refused
    /// before any request is written.
    pub(crate) fn stored_at_a_closed_port(&self) -> ContextSourceRegistration {
        let mut registration = self.registration.clone();
        registration["endpoint"] = json!("http://127.0.0.1:9/");
        serde_json::from_value(registration).expect("a valid registration")
    }

    /// The registration as the registration store keeps it once it was made
    /// auxiliary, so its source takes no copy of a subscription.
    pub(crate) fn stored_as_auxiliary(&self) -> ContextSourceRegistration {
        let mut registration = self.registration.clone();
        registration["mode"] = json!("auxiliary");
        serde_json::from_value(registration).expect("a valid registration")
    }

    /// The registration as the registration store keeps it, advertising the
    /// deletion of subscriptions but not their creation.
    pub(crate) fn stored_without_subscription_creation(&self) -> ContextSourceRegistration {
        let mut registration = self.registration.clone();
        registration["operations"] = json!(["deleteSubscription"]);
        serde_json::from_value(registration).expect("a valid registration")
    }

    /// The registration as a companion notification writes it, compacted under
    /// the core context: the entity type is the term the default context
    /// expands, and the attribute no term of the core context names stays its
    /// full IRI.
    pub(crate) fn notified(&self) -> Value {
        let mut notified = self.registration.clone();
        notified["information"][0]["entities"][0]["type"] = json!("Vehicle");
        notified
    }

    /// The bodies of the subscriptions placed at the source, in the order
    /// they arrived.
    pub(crate) async fn placed(&self) -> Vec<Value> {
        self.received(Method::POST)
            .await
            .iter()
            .map(|request| serde_json::from_slice(&request.body).expect("a placed subscription is JSON"))
            .collect()
    }

    /// The paths of the subscriptions deleted at the source, in the order
    /// the deletions arrived.
    pub(crate) async fn deleted(&self) -> Vec<String> {
        self.received(Method::DELETE)
            .await
            .iter()
            .map(|request| request.url.path().to_owned())
            .collect()
    }

    /// The requests of `method` the source received.
    async fn received(&self, method: Method) -> Vec<Request> {
        self.server
            .received_requests()
            .await
            .expect("the source records its requests")
            .into_iter()
            .filter(|request| request.method == method)
            .collect()
    }
}
