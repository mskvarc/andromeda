//! Current-entity query narrowing under an aliased `@context` (clauses 4.3.6.1,
//! 4.3.6.3, 5.7.2.4).
//!
//! Both narrowing directions are exercised: a client attribute kept in the
//! client's own spelling (direction A), and a block's declared attribute
//! supplied when the client named none (direction B). A coinciding-segment case
//! proves coverage is decided by the IRI, not the last segment, and a
//! core-context case proves the engine path is behaviourally identical there.

use crate::{
    compaction::{
        CAPTURE_TIMEOUT,
        FORWARDED_ENTITIES,
        aliased_document,
        assert_no_last_segment,
        coinciding_segment_document,
        entity_forward_count,
        registration_context,
        serve,
    },
    context_source_adapter::ContextSourceAdapter,
    hosted_context::{forget_cached, start_origin},
    oracle::assert_status,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

/// Direction A: a client attribute whose expanded IRI matches a declared IRI is
/// forwarded in the client's own aliased spelling `spd`, never the IRI's last
/// segment `speed` (clause 4.3.6.1).
#[tokio::test]
async fn distop_compaction_forwards_an_aliased_client_attribute_under_its_alias() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let origin = start_origin().await;
    let context_url = serve(&origin, &aliased_document()).await;

    let registration = CsrBuilder::new(&topology.id("registration:compaction-read-a"), &mock.url(""))
        .mode("inclusive")
        .entity_type("Veh")
        .property_names(&["spd"])
        .operations(&["queryEntity"])
        .context(registration_context(&context_url))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let response = front
        .client()
        .query_entities("type=Veh&attrs=spd", "application/ld+json", Some(&context_url))
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let query = forwarded.uri.query().unwrap_or_default();
    assert!(query.contains("attrs=spd"), "the aliased attribute is forwarded under its alias: {query}");
    assert!(query.contains("type=Veh"), "the aliased type is forwarded under its alias: {query}");
    assert_no_last_segment(query);

    forget_cached(front.pool(), &[context_url]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}

/// Direction B: an exclusive block names one entity and its attributes, so a
/// client asking only for the type has the block's id, type and attribute added
/// to the forwarded request, each in the context's aliased spelling.
#[tokio::test]
async fn distop_compaction_supplies_an_exclusive_blocks_aliased_names_for_a_type_only_query() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let origin = start_origin().await;
    let context_url = serve(&origin, &aliased_document()).await;

    let covered = topology.id("Veh:compaction-read-b");
    let registration = CsrBuilder::new(&topology.id("registration:compaction-read-b"), &mock.url(""))
        .mode("exclusive")
        .entity_id(&covered)
        .entity_type("Veh")
        .property_names(&["spd"])
        .operations(&["queryEntity"])
        .context(registration_context(&context_url))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let response = front
        .client()
        .query_entities("type=Veh", "application/ld+json", Some(&context_url))
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let query = forwarded.uri.query().unwrap_or_default();
    assert!(query.contains("type=Veh"), "the block's aliased type is supplied: {query}");
    assert!(query.contains("attrs=spd"), "the block's aliased attribute is supplied: {query}");
    assert!(query.contains("compaction-read-b"), "the exclusive block's id is supplied: {query}");
    assert_no_last_segment(query);

    forget_cached(front.pool(), &[context_url]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}

/// A requested attribute that expands to an IRI the block does not cover is
/// dropped even though its term shares a last segment with a covered attribute's
/// IRI: coverage is by IRI, not by string coincidence. The block declares
/// `spd` (`.../vocab/speed`); the client asks `speed` (`.../other/speed`). The
/// block then covers nothing requested and forwards no query.
#[tokio::test]
async fn distop_compaction_drops_an_uncovered_attribute_despite_a_coinciding_iri_segment() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let origin = start_origin().await;
    let context_url = serve(&origin, &coinciding_segment_document()).await;

    let registration = CsrBuilder::new(&topology.id("registration:compaction-negative"), &mock.url(""))
        .mode("inclusive")
        .entity_type("Veh")
        .property_names(&["spd"])
        .operations(&["queryEntity"])
        .context(registration_context(&context_url))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let response = front
        .client()
        .query_entities("type=Veh&attrs=speed", "application/ld+json", Some(&context_url))
        .await
        .expect("query");
    assert_status(&response, 200);
    assert_eq!(
        entity_forward_count(&mock),
        0,
        "an attribute expanding to an uncovered IRI is dropped, not forwarded by a shared last segment"
    );

    forget_cached(front.pool(), &[context_url]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}

/// The core context is the ETSI conformance case: every term equals its IRI's
/// last segment, so the engine forwards the same plain compact terms the old
/// heuristic did -- `Vehicle` and `speed`, unchanged.
#[tokio::test]
async fn distop_compaction_keeps_the_core_context_forwarding_plain_compact_terms() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let registration = CsrBuilder::new(&topology.id("registration:compaction-core"), &mock.url(""))
        .mode("inclusive")
        .entity_type("Vehicle")
        .property_names(&["speed"])
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let response = front
        .client()
        .query_entities("type=Vehicle&attrs=speed", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let query = forwarded.uri.query().unwrap_or_default();
    assert!(
        query.contains("type=Vehicle"),
        "the core-context type forwards as its plain compact term: {query}"
    );
    assert!(
        query.contains("attrs=speed"),
        "the core-context attribute forwards as its plain compact term: {query}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
