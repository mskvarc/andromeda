//! An object without object type missing locally is not fetched from a source.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: A flat join with `joinLevel` 2 over a `linkedTo` without `objectType` whose object the broker does not
/// hold answers the root alone and forwards nothing
/// Clause: 4.5.23.1
/// Tenant: private (it counts every request its context source receives, and the discovery and unscoped requests of
/// other tests reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_fed_ret_0007_an_object_without_object_type_missing_locally_is_not_fetched_from_a_source() -> Result<()> {
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
        .relationship("linkedTo", building_r_id.clone())
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
    expect_that!(entities.as_slice(), len(eq(1)));
    expect_that!(entities.as_slice(), contains(has_id(&building_a_id)));
    let forwarded_request = SourceRequest::any_matching("/a/ngsi-ld/v1/(entities|entityOperations|temporal).*");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(0)));
    Ok(())
}
