//! Tenant isolation acceptance case.

use crate::fixtures::{FedRole, assert_status, body, broker, entity, registration, topology};
use andromeda_test_client::{header::jsonld_link_header, tenant_name::TenantName};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use pretty_assertions::assert_eq;
use serial_test::serial;

#[tokio::test]
#[serial(fed)]
async fn fed_tenant_isolation_preserves_domain_boundaries() {
    let topology = topology().await;
    let tenant_one = TenantName::parse("tenant-fed-one").expect("tenant one");
    let tenant_two = TenantName::parse("tenant-fed-two").expect("tenant two");
    let hub_one = broker(&topology, FedRole::Hub)
        .tenant_client(tenant_one.clone())
        .with_default_context(jsonld_link_header(COMPOUND_CONTEXT.as_str()).expect("context link"));
    let hub_two = broker(&topology, FedRole::Hub)
        .tenant_client(tenant_two.clone())
        .with_default_context(jsonld_link_header(COMPOUND_CONTEXT.as_str()).expect("context link"));
    let madrid_one = broker(&topology, FedRole::Madrid)
        .tenant_client(tenant_one)
        .with_default_context(jsonld_link_header(COMPOUND_CONTEXT.as_str()).expect("context link"));
    let madrid_two = broker(&topology, FedRole::Madrid)
        .tenant_client(tenant_two)
        .with_default_context(jsonld_link_header(COMPOUND_CONTEXT.as_str()).expect("context link"));
    let id = topology.id("Building:tenant-shared");
    assert_status(
        &madrid_one
            .post_entity(&entity(&id, "Building", 41))
            .with_ld_json()
            .send()
            .await
            .expect("tenant one entity"),
        201,
    );
    assert_status(
        &madrid_two
            .post_entity(&entity(&id, "Building", 42))
            .with_ld_json()
            .send()
            .await
            .expect("tenant two entity"),
        201,
    );
    assert_status(
        &hub_one
            .create_registration(&registration(
                &topology.id("registration:tenant-one"),
                "http://fed-domain-madrid:1072",
                "Building",
                None,
                "inclusive",
                Some(&["queryEntity"]),
            ))
            .await
            .expect("tenant one registration"),
        201,
    );
    assert_status(
        &hub_two
            .create_registration(&registration(
                &topology.id("registration:tenant-two"),
                "http://fed-domain-madrid:1072",
                "Building",
                None,
                "inclusive",
                Some(&["queryEntity"]),
            ))
            .await
            .expect("tenant two registration"),
        201,
    );
    let first = hub_one
        .query_entities("type=Building", "application/ld+json", None)
        .await
        .expect("tenant one query");
    let second = hub_two
        .query_entities("type=Building", "application/ld+json", None)
        .await
        .expect("tenant two query");
    assert_status(&first, 200);
    assert_status(&second, 200);
    assert_eq!(body(&first)[0]["speed"]["value"], 41);
    assert_eq!(body(&second)[0]["speed"]["value"], 42);
    topology.finish().await.expect("topology cleanup");
}
