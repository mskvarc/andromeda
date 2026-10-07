//! The Context Source Subscription test surface for the DISC area.
//!
//! The distributed [`TestClient`] models entities, registrations and ordinary subscriptions, but not the `/csourceSubscriptions` resource (clause 5.11). This module owns both halves of driving that resource from a test: a declarative [`CsrSubscriptionBuilder`] for the request body and the raw HTTP operations that create, retrieve, update, query and delete a Context Source Subscription.

use crate::{oracle::assert_status, topology::BrokerRole};
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use reqwest::Method;
use serde_json::{Map, Value, json};
use urlencoding::encode;

/// The origin-rooted Context Source Subscription collection resource.
const RESOURCE: &str = "/ngsi-ld/v1/csourceSubscriptions";

/// Build the JSON-LD `Link` header carrying the compound test context, used on the `application/json` update path so short-hand terms in a patch fragment expand exactly as they do on create (clause 5.5.7).
fn context_link() -> String {
    format!(
        "<{}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"",
        COMPOUND_CONTEXT.as_str()
    )
}

/// Create a Context Source Subscription from a full `application/ld+json` body.
pub async fn create(client: &TestClient, body: &Value) -> TestResponse {
    client
        .raw(Method::POST, RESOURCE)
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create context source subscription")
}

/// Create a Context Source Subscription on the front broker and assert the
/// `201 Created` acknowledgement.
pub async fn create_and_assert(topology: &MultiBrokerTopology<BrokerRole>, body: &Value) {
    let created = create(topology.broker(BrokerRole::A).expect("topology broker A").client(), body).await;
    assert_status(&created, 201);
}

/// Retrieve one Context Source Subscription, optionally passing `options` such as `sysAttrs` to expose the notification bookkeeping fields.
pub async fn retrieve(client: &TestClient, id: &str, options: Option<&str>) -> TestResponse {
    let query = match options {
        Some(options) => format!("?options={options}"),
        None => String::new(),
    };
    client
        .raw(Method::GET, &format!("{RESOURCE}/{}{query}", encode(id)))
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("retrieve context source subscription")
}

/// Query the Context Source Subscription collection with a raw query string
/// (for example `limit=1&offset=1`).
pub async fn query(client: &TestClient, query: &str) -> TestResponse {
    let suffix = if query.is_empty() { String::new() } else { format!("?{query}") };
    client
        .raw(Method::GET, &format!("{RESOURCE}{suffix}"))
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("query context source subscriptions")
}

/// Patch one Context Source Subscription with a merge fragment; the compound context travels in the `Link` header so the fragment carries only the fields it changes.
pub async fn update(client: &TestClient, id: &str, fragment: &Value) -> TestResponse {
    client
        .raw(Method::PATCH, &format!("{RESOURCE}/{}", encode(id)))
        .header("content-type", "application/json")
        .header("link", &context_link())
        .json(fragment)
        .send()
        .await
        .expect("update context source subscription")
}

/// Delete one Context Source Subscription.
pub async fn delete(client: &TestClient, id: &str) -> TestResponse {
    client
        .raw(Method::DELETE, &format!("{RESOURCE}/{}", encode(id)))
        .send()
        .await
        .expect("delete context source subscription")
}

/// Declarative builder for a Context Source Subscription body targeting a notification endpoint, covering the matching inputs the DISC area exercises: the watched entity type, watched and notification attributes, the temporal, geo, context-source and scope filters, a periodic time interval, the throttling, and the activation and expiry controls.
pub struct CsrSubscriptionBuilder {
    id: String,
    endpoint: String,
    entity_type: String,
    watched_attributes: Vec<String>,
    notification_attributes: Vec<String>,
    temporal_q: Option<Value>,
    geo_q: Option<Value>,
    csf: Option<String>,
    scope_q: Option<String>,
    time_interval: Option<u64>,
    throttling: Option<u64>,
    is_active: Option<bool>,
}

impl CsrSubscriptionBuilder {
    /// Start a subscription with the given id, notification endpoint and watched
    /// entity type.
    pub fn new(id: &str, endpoint: &str, entity_type: &str) -> CsrSubscriptionBuilder {
        CsrSubscriptionBuilder {
            id: id.to_owned(),
            endpoint: endpoint.to_owned(),
            entity_type: entity_type.to_owned(),
            watched_attributes: Vec::new(),
            notification_attributes: Vec::new(),
            temporal_q: None,
            geo_q: None,
            csf: None,
            scope_q: None,
            time_interval: None,
            throttling: None,
            is_active: None,
        }
    }

    /// Restrict the subscription to these `watchedAttributes`.
    pub fn watched_attributes(mut self, names: &[&str]) -> CsrSubscriptionBuilder {
        self.watched_attributes = names.iter().map(|name| (*name).to_owned()).collect();
        self
    }

    /// Restrict the `notification.attributes` the notification is scoped to.
    pub fn notification_attributes(mut self, names: &[&str]) -> CsrSubscriptionBuilder {
        self.notification_attributes = names.iter().map(|name| (*name).to_owned()).collect();
        self
    }

    /// Set the `temporalQ` matching filter verbatim.
    pub fn temporal_q(mut self, temporal_q: Value) -> CsrSubscriptionBuilder {
        self.temporal_q = Some(temporal_q);
        self
    }

    /// Set the `geoQ` matching filter verbatim.
    pub fn geo_q(mut self, geo_q: Value) -> CsrSubscriptionBuilder {
        self.geo_q = Some(geo_q);
        self
    }

    /// Set the Context Source Filter (`csf`) query string.
    pub fn csf(mut self, csf: &str) -> CsrSubscriptionBuilder {
        self.csf = Some(csf.to_owned());
        self
    }

    /// Set the `scopeQ` scope query string.
    pub fn scope_q(mut self, scope_q: &str) -> CsrSubscriptionBuilder {
        self.scope_q = Some(scope_q.to_owned());
        self
    }

    /// Make the subscription periodic with the given `timeInterval` (seconds).
    pub const fn time_interval(mut self, seconds: u64) -> CsrSubscriptionBuilder {
        self.time_interval = Some(seconds);
        self
    }

    /// Set the `throttling` (seconds): the shortest period between two
    /// consecutive notifications of the subscription.
    pub const fn throttling(mut self, seconds: u64) -> CsrSubscriptionBuilder {
        self.throttling = Some(seconds);
        self
    }

    /// Set the `isActive` flag (a `false` value creates the subscription paused).
    pub const fn active(mut self, is_active: bool) -> CsrSubscriptionBuilder {
        self.is_active = Some(is_active);
        self
    }

    /// Materialise the subscription as a JSON-LD body.
    pub fn build(self) -> Value {
        let mut notification = Map::new();
        notification.insert("format".to_owned(), json!("normalized"));
        notification.insert("endpoint".to_owned(), json!({ "uri": self.endpoint, "accept": "application/ld+json" }));
        if !self.notification_attributes.is_empty() {
            notification.insert("attributes".to_owned(), json!(self.notification_attributes));
        }

        let mut body = Map::new();
        body.insert("id".to_owned(), json!(self.id));
        body.insert("type".to_owned(), json!("Subscription"));
        body.insert("entities".to_owned(), json!([ { "type": self.entity_type } ]));
        body.insert("notification".to_owned(), Value::Object(notification));
        if !self.watched_attributes.is_empty() {
            body.insert("watchedAttributes".to_owned(), json!(self.watched_attributes));
        }
        if let Some(temporal_q) = self.temporal_q {
            body.insert("temporalQ".to_owned(), temporal_q);
        }
        if let Some(geo_q) = self.geo_q {
            body.insert("geoQ".to_owned(), geo_q);
        }
        if let Some(csf) = self.csf {
            body.insert("csf".to_owned(), json!(csf));
        }
        if let Some(scope_q) = self.scope_q {
            body.insert("scopeQ".to_owned(), json!(scope_q));
        }
        if let Some(time_interval) = self.time_interval {
            body.insert("timeInterval".to_owned(), json!(time_interval));
        }
        if let Some(throttling) = self.throttling {
            body.insert("throttling".to_owned(), json!(throttling));
        }
        if let Some(is_active) = self.is_active {
            body.insert("isActive".to_owned(), json!(is_active));
        }
        body.insert("@context".to_owned(), json!(COMPOUND_CONTEXT.as_str()));
        Value::Object(body)
    }
}
