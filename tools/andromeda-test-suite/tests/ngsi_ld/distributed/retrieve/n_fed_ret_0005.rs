//! A flat join fetches an object type annotated entity from its source with the levels left.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    forwarded_query::query_values,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Relationship, VocabTerm},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: A flat join with `joinLevel` 2 from a local Building whose `linkedTo` carries `objectType` answers the
/// root and the Linked Entity only the registered source holds, fetched by one forwarded retrieval carrying join
/// flat, `joinLevel` 1 and `containedBy` naming the root
/// Clause: 4.5.23, 4.5.23.1, 6.3.21
/// Tenant: private (it counts every request its context source receives, and the discovery and unscoped requests of
/// other tests reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_fed_ret_0005_a_flat_join_fetches_an_object_type_annotated_entity_from_its_source_with_the_levels_left() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let building_a_id = namespace.urn("Building", "A")?;
    let building_type = namespace.entity_type("Building")?;
    let building_r_id = namespace.urn("Building", "R")?;
    let registration_id = namespace.registration_id(1)?;
    let building_a = Entity::builder()
        .id(building_a_id.clone())
        .entity_type(building_type.clone())
        .relationship(
            "linkedTo",
            Relationship::builder(building_r_id.clone()).object_type(VocabTerm::from(&building_type)),
        )
        .build()?;
    broker.entities().create(&building_a).locality(Locality::Local).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id(building_r_id.clone())])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_entity = Entity::output_builder()
        .id(building_r_id.clone())
        .entity_type(building_type.clone())
        .property("category", "commercial")
        .build()?;
    source
        .answer_when(
            SourceRequest::matching(Method::GET, "/a/ngsi-ld/v1/entities/.*-R"),
            source_answer::entity(&source_entity),
        )
        .await?;
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve_flattened(&building_a_id)
        .entity_type(&building_type)
        .join_level(2.try_into()?)
        .send()
        .await?;
    let entities = retrieved.body();
    expect_that!(entities.as_slice(), len(eq(2)));
    expect_that!(entities.as_slice(), contains(has_id(&building_a_id)));
    expect_that!(entities.as_slice(), contains(has_id(&building_r_id)));
    let forwarded_request = SourceRequest::matching(Method::GET, "/a/ngsi-ld/v1/entities/.*-R");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(query_values(forwarded_one, "join"), nth(0, eq("flat")));
    expect_that!(query_values(forwarded_one, "joinLevel"), nth(0, eq("1")));
    expect_that!(
        query_values(forwarded_one, "containedBy"),
        nth(0, contains_substring(format!("urn:ngsi-ld:Building:{namespace}-A").as_str()))
    );
    let forwarded_request_2 = SourceRequest::any_matching("/a/ngsi-ld/v1/.*");
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(1)));
    Ok(())
}
