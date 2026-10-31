//! An entity whose holder was not consulted stays in the map.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    received_status::ExpectedStatus,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, NgsiLdMediaType, Value},
    query::{IdPattern, Locality, q},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntitySource, RegistrationInfo},
};

/// Pins: Query Entities with local true and q under an `EntityMap` of two local Entities and one a registered
/// source answered answers without contacting the source, and the `EntityMap` then lists the source's Entity, which
/// was not checked, and no longer the local Entity deleted in between
/// Clause: 4.3.6.7, 5.5.13, 5.5.14, 5.2.39
/// Tenant: private (the file counts the requests its registered source receives; in the shared tenant a query of
/// another file naming no type matches the registration and reaches the source too, and clause 4.14 keeps each
/// tenant's registrations its own)
#[gtest]
#[tokio::test]
async fn n_qry_pag_9008_an_entity_whose_holder_was_not_consulted_stays_in_the_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let em_holder_id = namespace.urn("EmHolder", 1)?;
    let em_holder_type = namespace.entity_type("EmHolder")?;
    let em_holder_2_id = namespace.urn("EmHolder", 2)?;
    let em_holder_remote_id = namespace.urn("EmHolder", "remote")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let entities = [
        Entity::builder()
            .id(em_holder_id.clone())
            .entity_type(em_holder_type.clone())
            .property("temperature", Value::Number("30.0".parse()?))
            .build()?,
        Entity::builder()
            .id(em_holder_2_id.clone())
            .entity_type(em_holder_type.clone())
            .property("temperature", Value::Number("30.0".parse()?))
            .build()?,
    ];
    tenant.entities().batch_create(&entities).send().await?.done()?;
    let source_entities = [Entity::output_builder()
        .id(em_holder_remote_id.clone())
        .entity_type(em_holder_type.clone())
        .property("temperature", Value::Number("30.0".parse()?))
        .build()?];
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(em_holder_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    tenant.registrations().register(&registration).send().await?;
    let _registration_cleanup = Cleanup::of(&tenant).registration(&registration_id);
    let entity_map_request = EntityMapRequest::returned();
    let page = tenant
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&em_holder_type)
        .q(q::attr("temperature").gt(20))
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.items(), len(eq(3)));
    let map = page.entity_map().valid().or_fail()?;
    let map_id = map.id().or_fail()?;
    let map_2 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(&map_id)
        .send()
        .await?;
    expect_that!(
        map_2.entity_map.get(&em_holder_remote_id),
        some(contains(eq(&EntitySource::Registration(registration_id.clone()))))
    );
    expect_that!(&map_2.entity_map, len(eq(3)));
    expect_that!(map_2.entity_map.get(&em_holder_id), some(anything()));
    expect_that!(map_2.entity_map.get(&em_holder_2_id), some(anything()));
    tenant.entities().delete(&em_holder_2_id).send().await?.done()?;
    let entity_map_request_2 = EntityMapRequest::reuse(map.clone());
    let page_2 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&em_holder_type)
        .q(q::attr("temperature").gt(20))
        .locality(Locality::Local)
        .limit(100)
        .entity_map(&entity_map_request_2)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&em_holder_id)));
    // Only the query that created the EntityMap reached the source.
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let map_3 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(&map_id)
        .send()
        .await?;
    expect_that!(&map_3.entity_map, len(eq(2)));
    expect_that!(map_3.entity_map.get(&em_holder_id), some(anything()));
    expect_that!(map_3.entity_map.get(&em_holder_remote_id), some(anything()));
    expect_that!(map_3.entity_map.get(&em_holder_2_id), none());
    tenant.entity_maps().delete(&map_id).send().await?;
    tenant.registrations().delete(&registration_id).send().await?;
    Ok(())
}
