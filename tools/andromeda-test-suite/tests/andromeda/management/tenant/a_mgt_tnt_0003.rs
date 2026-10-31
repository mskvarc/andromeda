//! The collection pages by cursor.

use andromeda_test_suite::{
    broker,
    link_header::link_text,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    raw_call::target,
    tenant_resource::TenantResource,
    unchecked_page::follow,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::RelationType,
    model::{Entity, Tenant},
};

/// Pins: the tenant collection pages by cursor: a page of limit 1 holds one tenant, reports a total of at least the
/// tenants this file created and names a next page by cursor, and that page holds one other tenant
/// Clause: 5.5.9.1, 6.3.10
/// Tenant: private (the subject is the tenant collection, which only tenants the file creates can populate)
/// Recorded: Andromeda extension, not v1.9.1: tenant management (the /tenants resources), which clause 5.5.10
/// leaves implementation-specific ("How Tenants are created, is implementation-specific")
#[gtest]
#[tokio::test]
async fn a_mgt_tnt_0003_the_collection_pages_by_cursor() -> Result<()> {
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
    let request = request_to(&front, Method::GET, &target("tenants", &[("limit", "1")]))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = front.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let first_page = answer?;
    let first_listing = TenantResource::read_all(&first_page)?;
    expect_that!(first_listing, len(eq(1)));
    let total = first_page
        .headers()
        .get("NGSILD-Results-Count")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    expect_that!(total, some(ge(3)));
    expect_that!(link_text(first_page.headers()), contains_substring("rel=\"next\""));
    expect_that!(link_text(first_page.headers()), contains_substring("cursor="));
    let first_name = first_listing.first().or_fail()?.tenant_name.to_string();
    let next = follow(&front, &first_page, &RelationType::Next).await?.or_fail()?;
    expect_that!(&next, answered(StatusCode::OK));
    let second_page = next?;
    let second_listing = TenantResource::read_all(&second_page)?;
    expect_that!(second_listing, len(eq(1)));
    expect_that!(second_listing.first().map(|tenant| tenant.tenant_name.to_string()), some(not(eq(&first_name))));
    Ok(())
}
