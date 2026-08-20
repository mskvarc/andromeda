//! Query-CSR temporal selection (clause 5.10.2 over the 5.12 temporal matching
//! rules): latest-versus-interval partitioning, observation and management
//! interval matching, absent-interval exclusion, and the before/after/between
//! relations. All `T-LOCAL`.

use crate::{
    disc::{
        PEER_ENDPOINT,
        csr_discovery::{contains, query, register, register_interval},
    },
    registration::CsrBuilder,
    topology::DistributedTopology,
};
use urlencoding::encode;

/// Without a temporal query the registry returns only latest-info (interval-free)
/// registrations, excluding interval-bearing ones (clauses 5.10.2, 5.12).
/// Confirms REQ-DISC-30.
#[tokio::test]
async fn query_csr_no_temporal_query_returns_latest_info_only() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let vtype = topology.id("LatestType");
    let latest = topology.id("registration:latest");
    let temporal = topology.id("registration:temporal");
    register(&topology, &CsrBuilder::new(&latest, PEER_ENDPOINT).entity_type(&vtype).build()).await;
    register_interval(
        &topology,
        &temporal,
        &vtype,
        "observationInterval",
        "2020-01-01T00:00:00Z",
        "2021-01-01T00:00:00Z",
    )
    .await;

    let body = query(&topology, &format!("type={}", encode(&vtype))).await;
    assert!(contains(&body, &latest), "the latest-info registration is returned");
    assert!(
        !contains(&body, &temporal),
        "the interval-bearing registration is excluded without a temporal query"
    );
    topology.finish().await.expect("topology cleanup");
}

/// A temporal query returns only interval-bearing registrations, excluding
/// latest-info ones (clauses 5.10.2, 5.12). Confirms REQ-DISC-31.
#[tokio::test]
async fn query_csr_temporal_query_returns_interval_bearing_only() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let vtype = topology.id("IntervalType");
    let latest = topology.id("registration:interval-latest");
    let temporal = topology.id("registration:interval-temporal");
    register(&topology, &CsrBuilder::new(&latest, PEER_ENDPOINT).entity_type(&vtype).build()).await;
    register_interval(
        &topology,
        &temporal,
        &vtype,
        "observationInterval",
        "2020-01-01T00:00:00Z",
        "2021-01-01T00:00:00Z",
    )
    .await;

    let body = query(
        &topology,
        &format!(
            "type={}&timerel=after&timeAt={}&timeproperty=observedAt",
            encode(&vtype),
            encode("2020-06-01T00:00:00Z")
        ),
    )
    .await;
    assert!(contains(&body, &temporal), "the interval-bearing registration is returned for a temporal query");
    assert!(!contains(&body, &latest), "the latest-info registration is excluded from a temporal query");
    topology.finish().await.expect("topology cleanup");
}

/// An `observedAt` temporal query is matched against a registration's
/// `observationInterval`: an overlapping instant matches, one outside excludes
/// (clause 5.12). Confirms REQ-DISC-32.
#[tokio::test]
async fn query_csr_observedat_matched_against_observation_interval() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let vtype = topology.id("ObsType");
    let id = topology.id("registration:observed");
    register_interval(&topology, &id, &vtype, "observationInterval", "2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z").await;

    let inside = query(
        &topology,
        &format!(
            "type={}&timerel=after&timeAt={}&timeproperty=observedAt",
            encode(&vtype),
            encode("2020-06-01T00:00:00Z")
        ),
    )
    .await;
    assert!(contains(&inside, &id), "an observedAt query inside the observation interval matches");

    let outside = query(
        &topology,
        &format!(
            "type={}&timerel=after&timeAt={}&timeproperty=observedAt",
            encode(&vtype),
            encode("2022-06-01T00:00:00Z")
        ),
    )
    .await;
    assert!(!contains(&outside, &id), "an observedAt query after the observation interval does not match");
    topology.finish().await.expect("topology cleanup");
}

/// Management time-properties (`createdAt`/`modifiedAt`) are matched against a
/// registration's `managementInterval` (clause 5.12). Confirms REQ-DISC-33.
#[tokio::test]
async fn query_csr_management_timeproperties_matched_against_management_interval() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let vtype = topology.id("MgmtType");
    let id = topology.id("registration:management");
    register_interval(&topology, &id, &vtype, "managementInterval", "2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z").await;

    let body = query(
        &topology,
        &format!(
            "type={}&timerel=after&timeAt={}&timeproperty=createdAt",
            encode(&vtype),
            encode("2020-06-01T00:00:00Z")
        ),
    )
    .await;
    assert!(contains(&body, &id), "a createdAt query inside the management interval matches");
    topology.finish().await.expect("topology cleanup");
}

/// When the interval relevant to the queried time-property is absent the
/// registration does not match: a `createdAt` query against a CSR carrying only
/// an `observationInterval` is excluded (clause 5.12). Confirms REQ-DISC-34.
#[tokio::test]
async fn query_csr_relevant_interval_absent_no_match() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let vtype = topology.id("AbsentType");
    let id = topology.id("registration:absent-interval");
    register_interval(&topology, &id, &vtype, "observationInterval", "2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z").await;

    let body = query(
        &topology,
        &format!(
            "type={}&timerel=after&timeAt={}&timeproperty=createdAt",
            encode(&vtype),
            encode("2020-06-01T00:00:00Z")
        ),
    )
    .await;
    assert!(
        !contains(&body, &id),
        "a createdAt query does not match a CSR that carries only an observation interval"
    );
    topology.finish().await.expect("topology cleanup");
}

/// The `before` and `after` relations match a registration whose interval falls
/// within or at the queried endpoint and exclude one entirely outside it
/// (clause 5.12). Confirms REQ-DISC-35.
#[tokio::test]
async fn query_csr_before_after_match_within_or_at_endpoint() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let vtype = topology.id("BeforeAfterType");
    let id = topology.id("registration:before-after");
    register_interval(&topology, &id, &vtype, "observationInterval", "2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z").await;

    let within = query(
        &topology,
        &format!(
            "type={}&timerel=before&timeAt={}&timeproperty=observedAt",
            encode(&vtype),
            encode("2020-06-01T00:00:00Z")
        ),
    )
    .await;
    assert!(contains(&within, &id), "a before query whose endpoint falls inside the interval matches");

    let outside = query(
        &topology,
        &format!(
            "type={}&timerel=before&timeAt={}&timeproperty=observedAt",
            encode(&vtype),
            encode("2019-01-01T00:00:00Z")
        ),
    )
    .await;
    assert!(!contains(&outside, &id), "a before query whose endpoint precedes the interval does not match");
    topology.finish().await.expect("topology cleanup");
}

/// The `between` relation matches on interval overlap and excludes a disjoint
/// window (clause 5.12). Confirms REQ-DISC-36.
#[tokio::test]
async fn query_csr_between_matches_on_interval_overlap() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let vtype = topology.id("BetweenType");
    let id = topology.id("registration:between");
    register_interval(&topology, &id, &vtype, "observationInterval", "2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z").await;

    let overlap = query(
        &topology,
        &format!(
            "type={}&timerel=between&timeAt={}&endTimeAt={}&timeproperty=observedAt",
            encode(&vtype),
            encode("2020-06-01T00:00:00Z"),
            encode("2020-09-01T00:00:00Z")
        ),
    )
    .await;
    assert!(contains(&overlap, &id), "a between window overlapping the interval matches");

    let disjoint = query(
        &topology,
        &format!(
            "type={}&timerel=between&timeAt={}&endTimeAt={}&timeproperty=observedAt",
            encode(&vtype),
            encode("2018-01-01T00:00:00Z"),
            encode("2019-01-01T00:00:00Z")
        ),
    )
    .await;
    assert!(!contains(&disjoint, &id), "a between window disjoint from the interval does not match");
    topology.finish().await.expect("topology cleanup");
}
