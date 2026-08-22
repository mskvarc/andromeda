//! Distributed-operations topology declarations.
//!
//! Lifecycle, endpoint handling, readiness, database isolation, and teardown
//! are owned by the shared [`andromeda_test_harness::multi_broker`] module.

use andromeda_test_client::header::jsonld_link_header;
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use andromeda_test_harness::{
    multi_broker::{BrokerNodeSpec, MultiBrokerError, MultiBrokerTopology},
    readiness::ReadinessPolicy,
};
use std::time::Duration;

/// Roles in the distributed conformance compose topology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrokerRole {
    /// Front broker.
    A,
    /// Replica of the front broker's identity and store.
    AReplica,
    /// First peer.
    B,
    /// Second peer.
    C,
}

const READINESS: ReadinessPolicy = ReadinessPolicy::new(Duration::from_secs(45), Duration::from_millis(250), Duration::from_secs(2));

fn spec(role: BrokerRole) -> BrokerNodeSpec<BrokerRole> {
    match role {
        BrokerRole::A => BrokerNodeSpec::from_environment(
            role,
            BrokerRole::A,
            "DIST",
            "A",
            "http://127.0.0.1:1061/ngsi-ld/v1/",
            "http://broker-a:1056",
            "postgres://postgres:postgres@127.0.0.1:5442/broker_a",
            "broker-a",
            None,
        ),
        BrokerRole::AReplica => BrokerNodeSpec::from_environment(
            role,
            BrokerRole::A,
            "DIST",
            "A_REPLICA",
            "http://127.0.0.1:1064/ngsi-ld/v1/",
            "http://broker-a2:1056",
            "postgres://postgres:postgres@127.0.0.1:5442/broker_a",
            "broker-a",
            None,
        ),
        BrokerRole::B => BrokerNodeSpec::from_environment(
            role,
            BrokerRole::B,
            "DIST",
            "B",
            "http://127.0.0.1:1062/ngsi-ld/v1/",
            "http://broker-b:1056",
            "postgres://postgres:postgres@127.0.0.1:5443/broker_b",
            "broker-b",
            None,
        ),
        BrokerRole::C => BrokerNodeSpec::from_environment(
            role,
            BrokerRole::C,
            "DIST",
            "C",
            "http://127.0.0.1:1063/ngsi-ld/v1/",
            "http://broker-c:1056",
            "postgres://postgres:postgres@127.0.0.1:5444/broker_c",
            "broker-c",
            None,
        ),
    }
    .unwrap_or_else(|error| panic!("distributed broker specification is valid: {error}"))
}

async fn with_roles(roles: &[BrokerRole]) -> Result<andromeda_test_harness::multi_broker::MultiBrokerTopology<BrokerRole>, MultiBrokerError> {
    let context = Some(jsonld_link_header(COMPOUND_CONTEXT.as_str()).map_err(|error| MultiBrokerError::Client(error.to_string()))?);
    MultiBrokerTopology::from_specs("distop", roles.iter().copied().map(spec).collect(), context, READINESS).await
}

/// Build the single-broker topology.
pub async fn single() -> Result<andromeda_test_harness::multi_broker::MultiBrokerTopology<BrokerRole>, MultiBrokerError> {
    with_roles(&[BrokerRole::A]).await
}

/// Build the front-plus-peer topology.
pub async fn pair() -> Result<andromeda_test_harness::multi_broker::MultiBrokerTopology<BrokerRole>, MultiBrokerError> {
    with_roles(&[BrokerRole::A, BrokerRole::B]).await
}

/// Build the three-broker chain topology.
pub async fn chain() -> Result<andromeda_test_harness::multi_broker::MultiBrokerTopology<BrokerRole>, MultiBrokerError> {
    with_roles(&[BrokerRole::A, BrokerRole::B, BrokerRole::C]).await
}

/// Build the replica-plus-peer topology.
pub async fn replica_pair() -> Result<andromeda_test_harness::multi_broker::MultiBrokerTopology<BrokerRole>, MultiBrokerError> {
    with_roles(&[BrokerRole::A, BrokerRole::AReplica, BrokerRole::B]).await
}
