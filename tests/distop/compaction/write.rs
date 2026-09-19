//! Write narrowing under an aliased `@context` (clauses 4.3.6.1, 5.6).
//!
//! The write-forwarding filter keeps a body member when its expanded IRI is one
//! the registration declares. Under an aliased context the covered attribute
//! `spd` must survive the filter and reach the source; the removed last-segment
//! heuristic would have compared the declared IRI's segment `speed` against the
//! body key `spd`, dropped it, and forwarded an empty body.

use crate::{
    compaction::{CAPTURE_TIMEOUT, FORWARDED_ENTITIES, aliased_document, registration_context, serve},
    context_source_adapter::ContextSourceAdapter,
    hosted_context::{forget_cached, start_origin},
    oracle::assert_status,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use serde_json::{Value, json};

/// A forwarded create carries the covered aliased attribute `spd` intact, in the
/// registration context's compact spelling, never the IRI's last segment.
#[tokio::test]
async fn distop_compaction_forwards_a_created_aliased_attribute_intact() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::POST, FORWARDED_ENTITIES, 201, &json!({})).await;

    let origin = start_origin().await;
    let context_url = serve(&origin, &aliased_document()).await;

    let entity_id = topology.id("Veh:compaction-write");
    let registration = CsrBuilder::new(&topology.id("registration:compaction-write"), &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .entity_type("Veh")
        .property_names(&["spd"])
        .operations(&["createEntity"])
        .context(registration_context(&context_url))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let entity = json!({ "id": entity_id, "type": "Veh", "spd": { "type": "Property", "value": 42 } });
    let created = front.client().post_entity(&entity).with_json(&context_url).send().await.expect("create entity");
    assert!(created.status.is_success(), "the create succeeds: {} {:?}", created.status, created.body);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(forwarded.method, Method::POST, "the create is forwarded as a POST");
    let body: Value = forwarded.json().expect("the forwarded create body is JSON");
    assert!(
        body.get("spd").is_some(),
        "the covered attribute survives the write filter under its alias: {body}"
    );
    assert!(
        body.get("speed").is_none(),
        "the attribute IRI's last segment must not leak into the body: {body}"
    );

    forget_cached(front.pool(), &[context_url]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}
