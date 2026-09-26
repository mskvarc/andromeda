//! Retrieve-by-id narrowing under an aliased `@context` (clause 5.7.1.4).
//!
//! A retrieval is forwarded as a retrieve-by-id to a source supporting
//! `retrieveEntity`, narrowed to the type the client selected (clause 5.7.1.3)
//! and the attribute the registration block covers (clause 4.3.6.3). Under an
//! aliased context both must be forwarded in the context's compact spelling.

use crate::{
    compaction::{CAPTURE_TIMEOUT, FORWARDED_ENTITIES, aliased_document, assert_no_last_segment, registration_context, serve},
    context_source_adapter::ContextSourceAdapter,
    hosted_context::{forget_cached, start_origin},
    oracle::assert_status,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

/// A retrieve-by-id naming the aliased type `Veh` is forwarded with that type
/// and the covered aliased attribute `spd`, never the IRIs' last segments.
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn distop_compaction_forwards_a_typed_retrieve_under_the_alias() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    let origin = start_origin().await;
    let context_url = serve(&origin, &aliased_document()).await;

    let covered = topology.id("Veh:compaction-retrieve");
    mock.set_stub_with_body(
        Method::GET,
        &format!("{FORWARDED_ENTITIES}/{covered}"),
        200,
        &json!({"id": covered, "type": "Veh"}),
    )
    .await;
    let registration = CsrBuilder::new(&topology.id("registration:compaction-retrieve"), &mock.url(""))
        .mode("inclusive")
        .entity_id(&covered)
        .entity_type("Veh")
        .property_names(&["spd"])
        .operations(&["retrieveEntity"])
        .context(registration_context(&context_url))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let response = front
        .client()
        .get_entity(&covered, "type=Veh", "application/ld+json", Some(&context_url))
        .await
        .expect("retrieve");
    assert!(
        response.status.as_u16() < 500,
        "the forwarding did not fail the retrieve: {} {:?}",
        response.status,
        response.body
    );

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(
        forwarded.uri.path(),
        format!("{FORWARDED_ENTITIES}/{covered}"),
        "the retrieve is forwarded as a retrieve-by-id"
    );
    let query = forwarded.uri.query().unwrap_or_default();
    assert!(query.contains("type=Veh"), "the selected type is forwarded under its alias: {query}");
    assert!(query.contains("attrs=spd"), "the covered attribute is supplied under its alias: {query}");
    assert_no_last_segment(query);

    forget_cached(front.pool(), &[context_url]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}
