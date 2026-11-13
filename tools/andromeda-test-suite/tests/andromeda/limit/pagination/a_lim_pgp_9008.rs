//! With a configured api root prefix every pointer of the tenant collection starts with its path.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    link_header::link_target,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    raw_call::target,
    wire::andromeda_request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::RelationType,
    model::{Entity, Tenant},
};

/// Pins: On a broker whose configured apiRoot carries the path prefix /broker, listing tenants with limit 1 over at
/// least the two tenants this file created answers a next pointer starting with /broker/ngsi-ld/andromeda/v1/tenants?
/// Clause: 5.5.9, 6.2, 6.3.10
/// Tenant: private (the subject is the tenant collection, which only tenants the file creates can populate)
/// Recorded: Andromeda extension, not v1.9.1: tenant management (the /tenants resources), and `server.public_url` as
/// the apiRoot whose path a pointer starts with (clause 6.2 allows a prefix string), so it resolves through the proxy
/// that strips the prefix
#[gtest]
#[tokio::test]
async fn a_lim_pgp_9008_with_a_configured_api_root_prefix_every_pointer_of_the_tenant_collection_starts_with_its_path() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_prefixed = broker::client(BrokerRole::Prefixed)?;
    broker::private_tenant(&broker_prefixed, &namespace).await?;
    let tenant = broker_prefixed.with_tenant(format!("{namespace}-b").parse::<Tenant>()?);
    let device = Entity::builder()
        .id(namespace.urn("Device", 1)?)
        .entity_type(namespace.entity_type("Device")?)
        .property("reading", 1)
        .build()?;
    tenant.entities().create(&device).send().await?.done()?;
    // The tenant collection is an Andromeda extension no NGSI-LD operation covers, so the request is sent as written.
    let request = andromeda_request_to(&broker_prefixed, Method::GET, &target("tenants", &[("limit", "1")]))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker_prefixed.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let tenants = answer?;
    expect_that!(
        link_target(tenants.headers(), &RelationType::Next)?,
        some(starts_with("/broker/ngsi-ld/andromeda/v1/tenants?"))
    );
    Ok(())
}
