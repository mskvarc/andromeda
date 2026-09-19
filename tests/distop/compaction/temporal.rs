//! Temporal query narrowing under an aliased `@context` (clause 5.7.4.4).
//!
//! A temporal query is narrowed the same way a current-entity query is, so the
//! covered aliased type and attribute must reach a `queryTemporal` source in the
//! context's compact spelling.

use crate::{
    compaction::{CAPTURE_TIMEOUT, FORWARDED_TEMPORAL, aliased_document, assert_no_last_segment, registration_context, serve},
    context_source_adapter::ContextSourceAdapter,
    hosted_context::{forget_cached, start_origin},
    oracle::assert_status,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use serde_json::json;

/// A temporal query forwards the client's aliased type and attribute under their
/// aliases, never the IRIs' last segments.
#[tokio::test]
async fn distop_compaction_forwards_a_temporal_query_under_the_alias() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_TEMPORAL, 200, &json!([])).await;

    let origin = start_origin().await;
    let context_url = serve(&origin, &aliased_document()).await;

    let registration = CsrBuilder::new(&topology.id("registration:compaction-temporal"), &mock.url(""))
        .mode("inclusive")
        .entity_type("Veh")
        .property_names(&["spd"])
        .operations(&["queryTemporal"])
        .context(registration_context(&context_url))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let response = front
        .client()
        .query_temporal_entities("type=Veh&attrs=spd&timerel=before&timeAt=2030-01-01T00:00:00Z", Some(&context_url))
        .await
        .expect("temporal query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let query = forwarded.uri.query().unwrap_or_default();
    assert!(query.contains("type=Veh"), "the aliased type is forwarded under its alias: {query}");
    assert!(query.contains("attrs=spd"), "the aliased attribute is forwarded under its alias: {query}");
    assert_no_last_segment(query);

    forget_cached(front.pool(), &[context_url]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}
