//! Query Temporal request validation (clause 5.7.4.4/.5): a temporal query is
//! required, the request must not be too wide, `orderBy` requires `local=true` and
//! is restricted to `id`, and an aggregation over an ineligible attribute is
//! rejected. All `T-LOCAL`. Forwarding and merge rows live in
//! `crate::cons::query_temporal`.

use crate::{
    oracle::{assert_problem_details, assert_status},
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::{BAD_REQUEST_DATA, INVALID_REQUEST};

/// The whole-history temporal query window.
const BEFORE_2030: &str = "timerel=before&timeAt=2030-01-01T00:00:00Z";

/// A temporal query without any `timerel`/temporal query is rejected `400`
/// (clause 5.7.4.4). Confirms REQ-CONS-52.
#[tokio::test]
async fn distop_query_temporal_no_temporal_query_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
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
        .client()
        .query_temporal_entities(BEFORE_2030, None)
        .await
        .expect("query");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A temporal `orderBy` without `local=true` is rejected `400` (clause 5.7.4.5).
/// Confirms REQ-CONS-54.
#[tokio::test]
async fn distop_query_temporal_orderby_non_local_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}&orderBy=id"), None)
        .await
        .expect("query");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A temporal `orderBy` on a non-`id` member is rejected `400`: temporal ordering
/// is allowed on `id` only (clause 5.7.4.5). Confirms REQ-CONS-55.
#[tokio::test]
async fn distop_query_temporal_orderby_non_id_member_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
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
