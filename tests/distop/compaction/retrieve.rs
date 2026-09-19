//! Retrieve-by-id narrowing under an aliased `@context` (clause 5.7.1.4).
//!
//! A retrieve names no type, so a source advertising only `queryEntity` is
//! reached through a query downgrade that supplies the registered type. Under an
//! aliased context both the supplied type and the block's covered attribute must
//! be forwarded in the context's compact spelling.

use crate::{
    compaction::{CAPTURE_TIMEOUT, FORWARDED_ENTITIES, aliased_document, assert_no_last_segment, registration_context, serve},
    context_source_adapter::ContextSourceAdapter,
    hosted_context::{forget_cached, start_origin},
    oracle::assert_status,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use serde_json::json;

/// A retrieve forwarded as a query downgrade supplies the registered aliased
/// type `Veh` and the covered aliased attribute `spd`, never the IRIs' last
/// segments.
#[tokio::test]
async fn distop_compaction_forwards_a_retrieve_downgrade_under_the_alias() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let origin = start_origin().await;
    let context_url = serve(&origin, &aliased_document()).await;

    let covered = topology.id("Veh:compaction-retrieve");
    let registration = CsrBuilder::new(&topology.id("registration:compaction-retrieve"), &mock.url(""))
        .mode("inclusive")
        .entity_id(&covered)
        .entity_type("Veh")
        .property_names(&["spd"])
        // Only queryEntity is advertised, so the retrieve is downgraded to a GET
        // query naming the entity by id and its registered type.
        .operations(&["queryEntity"])
        .context(registration_context(&context_url))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let response = front
        .client()
        .get_entity(&covered, "", "application/ld+json", Some(&context_url))
        .await
        .expect("retrieve");
    assert!(
        response.status.as_u16() < 500,
        "the forwarding did not fail the retrieve: {} {:?}",
        response.status,
        response.body
    );

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let query = forwarded.uri.query().unwrap_or_default();
    assert!(query.contains("type=Veh"), "the registered type is supplied under its alias: {query}");
    assert!(query.contains("attrs=spd"), "the covered attribute is supplied under its alias: {query}");
    assert!(query.contains("compaction-retrieve"), "the retrieve names the entity by id: {query}");
    assert_no_last_segment(query);

    forget_cached(front.pool(), &[context_url]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}
