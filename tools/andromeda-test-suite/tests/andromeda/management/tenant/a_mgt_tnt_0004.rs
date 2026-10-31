//! The collection pages by offset.

use andromeda_test_suite::{
    broker,
    link_header::link_text,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    raw_call::target,
    tenant_resource::{TenantResource, TenantType},
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::model::{Entity, Tenant};

/// Pins: the tenant collection pages by limit and offset: offset 1 with limit 1 holds one tenant and names the
/// previous and next pages by offset 0 and offset 2, with no cursor
/// Clause: 5.5.9.2, 6.3.10
/// Tenant: private (the subject is the tenant collection, which only tenants the file creates can populate)
/// Recorded: Andromeda extension, not v1.9.1: tenant management (the /tenants resources), which clause 5.5.10
/// leaves implementation-specific ("How Tenants are created, is implementation-specific")
#[gtest]
#[tokio::test]
async fn a_mgt_tnt_0004_the_collection_pages_by_offset() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    broker::private_tenant(&front, &namespace).await?;
    for suffix in ["b", "c"] {
        let tenant = front.with_tenant(format!("{namespace}-{suffix}").parse::<Tenant>()?);
        let device = Entity::builder()
            .id(namespace.urn("Device", 1)?)
            .entity_type(namespace.entity_type("Device")?)
            .property("reading", 1)
            .build()?;
        tenant.entities().create(&device).send().await?.done()?;
    }
    // The tenant collection is an Andromeda extension no NGSI-LD operation covers, so the request is sent as written.
    let request = request_to(&front, Method::GET, &target("tenants", &[("offset", "1"), ("limit", "1")]))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = front.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let page = answer?;
    let listing = TenantResource::read_all(&page)?;
    expect_that!(listing, len(eq(1)));
    expect_that!(listing.first().map(|tenant| tenant.resource_type), some(eq(TenantType::Tenant)));
    let links = link_text(page.headers());
    expect_that!(links, contains_substring("offset=0"));
    expect_that!(links, contains_substring("offset=2"));
    expect_that!(links, not(contains_substring("cursor=")));
    Ok(())
}
