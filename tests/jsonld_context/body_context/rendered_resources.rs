//! One resource of every kind that renders a `@context`, and the `@context`
//! each renders under a request context.

use crate::{
    broker_endpoints::broker_url,
    context_api::extract_id,
    core_contexts::core,
    ngsi_requests::{create_device, device_query, get, ok_body},
    unique_ids::unique_id,
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};

/// The `@context` one resource rendered, with the resource it came from.
pub struct Rendered {
    /// Which resource answered.
    pub resource: &'static str,
    /// The `@context` its `application/ld+json` body named.
    pub context: Value,
}

/// A `Device`, a subscription, a context source subscription, a registration
/// and an entity map, all in one tenant.
pub struct RenderedResources {
    device: String,
    subscription: String,
    csource_subscription: String,
    registration: String,
    entity_map: String,
}

impl RenderedResources {
    /// Create one resource of every kind; the `Device` carries `speed` and is
    /// written under `terms`.
    pub async fn create(client: &Client, terms: &str) -> RenderedResources {
        let device = unique_id("urn:ngsi-ld:Device:rendered");
        create_device(client, &device, terms).await;
        let subscription = created(
            client,
            "subscriptions",
            &json!({
                "type": "Subscription",
                "entities": [{ "type": "Device" }],
                "notification": { "endpoint": { "uri": "http://127.0.0.1:1/notify", "accept": "application/json" } },
                "@context": core()
            }),
        )
        .await;
        let csource_subscription = created(
            client,
            "csourceSubscriptions",
            &json!({
                "type": "Subscription",
                "entities": [{ "type": "RegisteredThing" }],
                "notification": { "endpoint": { "uri": "http://127.0.0.1:1/notify", "accept": "application/json" } },
                "@context": core()
            }),
        )
        .await;
        let registration = created(
            client,
            "csourceRegistrations",
            &json!({
                "type": "ContextSourceRegistration",
                "information": [{ "entities": [{ "type": "RegisteredThing" }] }],
                "endpoint": "http://127.0.0.1:1",
                "@context": core()
            }),
        )
        .await;
        let mapped = client
            .post(broker_url("entityOperations/query"))
            .header("Content-Type", "application/ld+json")
            .json(&json!({ "type": "Query", "entities": [{ "type": "Device", "id": device }], "entityMap": true, "@context": terms }))
            .send()
            .await
            .expect("entity map query failed");
        assert_eq!(mapped.status(), StatusCode::CREATED, "a query that creates an entity map answers 201");
        let entity_map = mapped
            .headers()
            .get("NGSILD-EntityMap")
            .and_then(|value| value.to_str().ok())
            .expect("the created entity map")
            .to_owned();
        RenderedResources {
            device,
            subscription,
            csource_subscription,
            registration,
            entity_map,
        }
    }

    /// Retrieve every resource as `application/ld+json` naming `link` as the
    /// request context, and return the `@context` each rendered. A query
    /// renders it on every Entity, so the one it rendered is checked to be the
    /// same on all of them.
    pub async fn rendered(&self, client: &Client, link: Option<&str>) -> Vec<Rendered> {
        let mut rendered = Vec::new();
        for (resource, path) in [
            ("entity retrieval", format!("entities/{}", self.device)),
            ("temporal retrieval", format!("temporal/entities/{}", self.device)),
            ("subscription retrieval", format!("subscriptions/{}", self.subscription)),
            ("csourceSubscription retrieval", format!("csourceSubscriptions/{}", self.csource_subscription)),
            ("registration retrieval", format!("csourceRegistrations/{}", self.registration)),
            ("entity type list", "types".to_owned()),
            ("attribute list", "attributes".to_owned()),
            ("entity map retrieval", format!("entityMaps/{}", self.entity_map)),
        ] {
            let body = ok_body(get(client, &path, "application/ld+json", link).await).await;
            rendered.push(Rendered {
                resource,
                context: body["@context"].clone(), // clone: the body is dropped after this
            });
        }
        let listed = ok_body(get(client, &device_query(&self.device), "application/ld+json", link).await).await;
        let entities = listed.as_array().expect("a query answers an array");
        assert!(!entities.is_empty(), "the query finds the device: {listed}");
        let first = entities[0]["@context"].clone(); // clone: compared with every other entity below
        assert!(
            entities.iter().all(|entity| entity["@context"] == first),
            "every Entity names the same @context: {listed}"
        );
        rendered.push(Rendered {
            resource: "entity query (every Entity)",
            context: first,
        });
        rendered
    }
}

/// Create a resource at `collection` from an `application/ld+json` body and
/// return its identifier.
async fn created(client: &Client, collection: &str, body: &Value) -> String {
    let response = client
        .post(broker_url(collection))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create request failed");
    assert_eq!(response.status(), StatusCode::CREATED, "{collection}: {:?}", response.text().await);
    let location = response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .expect("a Location header");
    extract_id(location)
}

/// Assert every resource rendered `expected`.
pub fn assert_every_resource_named(rendered: &[Rendered], expected: &Value) {
    for Rendered { resource, context } in rendered {
        assert_eq!(context, expected, "{resource}");
    }
}
