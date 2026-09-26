//! Topology smoke tests: prove the multi-broker fixture and real forwarding
//! before the area suites build on them (plan §10.1).

use crate::{
    entity::vehicle,
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use pretty_assertions::{assert_eq, assert_ne};

/// Each broker in the compose stack answers `GET /info/sourceIdentity` with its
/// own distinct `contextSourceAlias` -- a direct check that the federation
/// identities are wired as configured (clause 5.2.40). Under the default tenant
/// the alias is the configured one; under the leased tenant it is a different
/// alias, since Table 5.2.40-1 makes the alias identify "a specific Tenant
/// within a registered Context Source".
#[tokio::test]
async fn topology_pair_reports_distinct_source_identities() {
    let topology = topology::pair().await.expect("two-broker topology is available");

    let mut tenant_aliases = Vec::new();
    for role in [BrokerRole::A, BrokerRole::B] {
        let broker = topology.broker(role).expect("topology broker for role");
        let response = broker
            .default_tenant_client()
            .source_identity(Some("application/ld+json"))
            .await
            .expect("source identity request");
        assert_status(&response, 200);
        let body = json_body(&response);
        assert_eq!(
            body["contextSourceAlias"].as_str(),
            Some(broker.configured_alias()),
            "broker {role:?} must report its configured alias under the default tenant"
        );
        assert_ne!(
            broker.alias(),
            broker.configured_alias(),
            "broker {role:?} names the leased tenant by an alias of its own"
        );
        tenant_aliases.push(broker.alias().to_owned());
    }
    assert_ne!(tenant_aliases[0], tenant_aliases[1], "the two brokers' tenant aliases are distinct");

    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration on broker A pointing at broker B serves the covered
/// attribute from B on a retrieve issued to A, with nothing held locally on A —
/// the minimal end-to-end proof that real broker-to-broker forwarding works
/// (clause 5.7.2, exclusive mode).
#[tokio::test]
async fn topology_pair_exclusive_retrieve_forwards_to_peer() {
    let topology = topology::pair().await.expect("two-broker topology is available");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let peer = topology.broker(BrokerRole::B).expect("topology broker B");

    let entity_id = topology.id("Vehicle:smoke-exclusive");
    let registration_id = topology.id("registration:smoke-exclusive");

    let created = peer
        .client()
        .post_entity(&vehicle(&entity_id, 42))
        .with_ld_json()
        .send()
        .await
        .expect("peer entity create");
    assert_status(&created, 201);

    let registration = CsrBuilder::new(&registration_id, peer.peer_url())
        .tenant(topology.tenant())
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity", "queryEntity"])
        .build();
    let registered = front.client().create_registration(&registration).await.expect("registration create");
    assert_status(&registered, 201);

    let retrieved = front
        .client()
        .get_entity(&entity_id, "", "application/ld+json", Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("front retrieve");
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    assert_eq!(
        body["speed"]["value"].as_i64(),
        Some(42),
        "the exclusive attribute must be served from the peer"
    );

    topology.finish().await.expect("topology cleanup");
}
