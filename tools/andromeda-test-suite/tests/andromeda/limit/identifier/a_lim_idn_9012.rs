//! A tenant name at the bound is served and one octet past it is refused.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    exact_length::padded,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    tenant_resource::{TenantResource, TenantType},
    wire::{header_value, request_to},
};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, ErrorType, NgsiLdMediaType, Tenant},
};

/// Pins: Create Entity under an NGSILD-Tenant of 1,005 octets answers 201 and creates the Tenant, the Entity reads
/// back under it, and the Tenant resource reads back as urn:ngsi-ld:Tenant: followed by the name, 1,024 octets; a
/// name of 1,006 octets answers 400 `BadRequestData` stating its 1006 bytes and the 1005-byte bound on Create
/// Entity and Query Entities, and 400 `InvalidRequest` as the tenant path segment
/// Clause: 6.3.14, 5.5.10
/// Tenant: private (the subject is a tenant name at its bound, carried by an Entity of the private tenant so its
/// length does not depend on the namespace)
/// Recorded: Andromeda bound, not v1.9.1: clause 6.3.14 makes the tenant a String with no length; Andromeda bounds
/// a tenant name at the 1,024-octet identifier bound less the 19 octets of urn:ngsi-ld:Tenant:, so the identifier
/// of every Tenant resource fits that bound
#[gtest]
#[tokio::test]
async fn a_lim_idn_9012_a_tenant_name_at_the_bound_is_served_and_one_octet_past_it_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let private = broker::private_tenant(&front, &namespace).await?;
    let carrier_id = format!("urn:ngsi-ld:TenantNameCarrier:{namespace}").parse::<EntityId>()?;
    let carrier = Entity::builder()
        .id(carrier_id.clone())
        .entity_type(namespace.entity_type("TenantNameCarrier")?)
        .property("tenantName", padded(&namespace.to_string(), 1005)?)
        .build()?;
    private.entities().create(&carrier).send().await?.done()?;
    let carried = private
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&carrier_id)
        .send()
        .await?;
    let long_tenant = padded(&namespace.to_string(), 1005)?;
    expect_that!(carried.body(), has_attribute("tenantName", any_attribute().value(long_tenant.as_str())));
    expect_that!(long_tenant, matches_regex("^[a-z0-9]{1005}$"));
    let long = front.with_tenant(long_tenant.parse::<Tenant>()?);
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("speed", 55)
        .build()?;
    let created = long.with_responses().entities().create(&vehicle).send().await?;
    expect_that!(
        header_value(created.head.headers(), &HeaderName::from_static("ngsild-tenant")),
        some(eq(long_tenant.as_str()))
    );
    let read_back = long
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(
        header_value(read_back.head.headers(), &HeaderName::from_static("ngsild-tenant")),
        some(eq(long_tenant.as_str()))
    );
    expect_that!(read_back.outcome.body(), has_attribute("speed", any_attribute().value(55)));
    // The Tenant resource is an Andromeda extension no NGSI-LD operation covers, so the request is sent as written.
    let request = request_to(&front, Method::GET, &format!("tenants/{long_tenant}"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = front.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let resource = TenantResource::read(answer.as_ref().or_fail()?)?;
    expect_that!(resource.resource_type, eq(TenantType::Tenant));
    expect_that!(resource.tenant_name.to_string(), eq(long_tenant.as_str()));
    expect_that!(resource.id, matches_regex("^urn:ngsi-ld:Tenant:[a-z0-9]{1005}$"));
    let too_long = front.with_tenant(format!("{long_tenant}a").parse::<Tenant>()?);
    let detail = "the NGSILD-Tenant header is not a valid tenant name: a tenant name of 1006 bytes exceeds the 1005 bytes a tenant name may occupy";
    let vehicle_2 = Entity::builder()
        .id(namespace.urn("Vehicle", 2)?)
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("speed", 56)
        .build()?;
    let refused = too_long.with_responses().entities().create(&vehicle_2).send().await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail(detail));
    let refused_2 = too_long
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&namespace.entity_type("Vehicle")?)
        .send()
        .await;
    expect_that!(&refused_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail(detail));
    // The Tenant resource is an Andromeda extension no NGSI-LD operation covers, so the request is sent as written.
    let request_2 = request_to(&front, Method::GET, &format!("tenants/{long_tenant}a"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = front.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest)
            .with_detail_containing("a tenant name of 1006 bytes exceeds the 1005 bytes a tenant name may occupy")
    );
    Ok(())
}
