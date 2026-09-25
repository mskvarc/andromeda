//! Query Temporal request validation (clause 5.7.4.4/.5): a temporal query is
//! required, the request must not be too wide, `orderBy` is refused where a
//! registered Context Source takes part in the answer and is restricted to `id`,
//! and an aggregation over an ineligible attribute is rejected. All `T-LOCAL`.
//! Forwarding and merge rows live in `crate::cons::query_temporal`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    oracle::{assert_problem_details, assert_status},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::{BAD_REQUEST_DATA, INVALID_REQUEST};
use pretty_assertions::assert_eq;

/// The whole-history temporal query window.
const BEFORE_2030: &str = "timerel=before&timeAt=2030-01-01T00:00:00Z";

/// A temporal query without any `timerel`/temporal query is rejected `400`
/// (clause 5.7.4.4). Confirms REQ-CONS-52.
#[tokio::test]
async fn distop_query_temporal_no_temporal_query_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_temporal_entities("type=Vehicle", None)
        .await
        .expect("query");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A temporal query with a temporal query but none of type/attrs/q/geoQ/local is
/// too wide and rejected `400` (clause 5.7.4.4). Confirms REQ-CONS-53.
#[tokio::test]
async fn distop_query_temporal_too_wide_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_temporal_entities(BEFORE_2030, None)
        .await
        .expect("query");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A temporal `orderBy` is rejected `400` where a registered Context Source
/// takes part in the answer, and accepted where none does -- clause 4.23.1's
/// "Sort ordering is never applied to distributed operations", read through
/// clause 5.5.13 as `docs/entity-ordering-local-scope.md` records. Confirms
/// REQ-CONS-54.
#[tokio::test]
async fn distop_query_temporal_orderby_with_a_participating_source_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    let ordered = format!("type={}&{BEFORE_2030}&orderBy=id", urlencoding::encode(&vtype));

    // With nothing registered the answer is this broker's alone, which clause
    // 5.5.13 makes a local execution however the request was written.
    let accepted = front.client().query_temporal_entities(&ordered, None).await.expect("query");
    assert_status(&accepted, 200);

    let registration_id = topology.id("registration:cons54");
    let registration = CsrBuilder::new(&registration_id, &mock.url(""))
        .mode("inclusive")
        .entity_type(&vtype)
        .operations(&["queryTemporal"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let rejected = front.client().query_temporal_entities(&ordered, None).await.expect("query");
    assert_status(&rejected, 400);
    assert_problem_details(&rejected, BAD_REQUEST_DATA);
    assert_eq!(mock.get_total_count(), 0, "the rejected query forwards nothing");

    let local = front
        .client()
        .query_temporal_entities(&format!("{ordered}&local=true"), None)
        .await
        .expect("query");
    assert_status(&local, 200);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A temporal `orderBy` on a non-`id` member is rejected `400`: temporal ordering
/// is allowed on `id` only (clause 5.7.4.5). Confirms REQ-CONS-55.
#[tokio::test]
async fn distop_query_temporal_orderby_non_id_member_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}&local=true&orderBy=speed"), None)
        .await
        .expect("query");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// An aggregated temporal representation requested over an attribute ineligible for
/// the aggregation method is rejected `400 InvalidRequest` (clause 5.7.4.4).
/// Confirms REQ-CONS-70.
#[tokio::test]
async fn distop_query_temporal_aggregated_ineligible_attribute_returns_invalidrequest() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_temporal_entities(
            &format!("type=Vehicle&{BEFORE_2030}&options=aggregatedValues&aggrMethods=sum&attrs=brandName"),
            None,
        )
        .await
        .expect("query");
    assert_status(&response, 400);
    assert_problem_details(&response, INVALID_REQUEST);
    topology.finish().await.expect("topology cleanup");
}
