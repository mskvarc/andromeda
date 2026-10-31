//! Before an instant inside the management interval with modified at matches.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    core_context::core_entry,
    hosted_context::HostedContext,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, DateTime, InlineContext, NgsiLdMediaType, OneOrMany},
    query::temporal_query::{self, TemporalProperty},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, TimeInterval},
};
use url::Url;

/// Pins: Query Context Source Registrations by type with timeproperty `modifiedAt`, timerel before and a `timeAt`
/// inside the registered `managementInterval` answers 200 with that registration and its `managementInterval`
/// Clause: 5.10.2, 5.10.2.4, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_qry_0010_before_an_instant_inside_the_management_interval_with_modified_at_matches() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let context = Context::Multiple(vec![ContextEntry::Inline(format!(r#"{{"Vehicle_{namespace}": "https://test.andromeda/{namespace}/Vehicle", "brandName": "https://test.andromeda/{namespace}/brandName", "speed": "https://test.andromeda/{namespace}/speed", "isParked": "https://test.andromeda/{namespace}/isParked", "OffStreetParking_{namespace}": "https://test.andromeda/{namespace}/OffStreetParking", "availableSpotNumber": "https://test.andromeda/{namespace}/availableSpotNumber", "totalSpotNumber": "https://test.andromeda/{namespace}/totalSpotNumber", "isNextToBuilding": "https://test.andromeda/{namespace}/isNextToBuilding", "Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/name", "subCategory": "https://test.andromeda/{namespace}/subCategory", "locatedAt": "https://test.andromeda/{namespace}/locatedAt"}}"#).parse::<InlineContext>()?), core_entry()?]);
    let ctx = HostedContext::add(&broker, &context).await?;
    let body_context = broker
        .with_context(Context::Multiple(vec![ctx.entry()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration = CSourceRegistration::builder("http://my.csource.org:1026/")
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .management_interval(TimeInterval::between("2020-08-01T22:07:00Z".parse()?, "2021-08-01T21:07:00Z".parse()?))
        .build()?;
    body_context.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&body_context).registration(&registration_id);
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .registrations()
        .query()
        .temporal_q(temporal_query::before("2021-08-01T20:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .entity_type(&building_type)
        .send()
        .await?;
    let listed = page.items().first().or_fail()?;
    expect_that!(
        listed
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(building_type.clone())))
    );
    expect_that!(
        listed.management_interval().map(TimeInterval::start_at).map(DateTime::timestamp),
        some(eq("2020-08-01T22:07:00Z".parse::<DateTime>()?.timestamp()))
    );
    expect_that!(
        listed.management_interval().and_then(TimeInterval::end_at).map(DateTime::timestamp),
        some(eq("2021-08-01T21:07:00Z".parse::<DateTime>()?.timestamp()))
    );
    expect_that!(listed.endpoint(), eq(&"http://my.csource.org:1026/".parse::<Url>()?));
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_id)));
    Ok(())
}
