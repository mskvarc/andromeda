//! Update attributes of an attribute the local entity lacks under an inclusive registration appends it.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
    wire::send_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::UpdateAttributes},
    model::{Entity, EntityFragment, NgsiLdMediaType},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Update Attributes naming an attribute the local entity does not hold is forwarded to the inclusive
/// registration that matches the entity and appended to the local entity: the answer is 204
/// Clause: 5.6.2.4, 6.6.3.2, 4.3.6.2
/// Recorded: 5.6.2.4 "such Attribute shall be appended to the target Entity"; an inclusive registration does not end
/// local processing
#[gtest]
#[tokio::test]
async fn n_fed_wrt_5105_update_attributes_of_an_attribute_the_local_entity_lacks_under_an_inclusive_registration_appends_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle_2).send().await?.done()?;
    source
        .answer_when(SourceRequest::any_matching("/.*").priority(1), source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id(vehicle_2_id.clone())])
            .build()?])
        .operations([OperationGroup::RedirectionOps])
        .mode(RegistrationMode::Inclusive)
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let fragment = EntityFragment::builder().property("speed", 99).build()?;
    let answer = send_to(
        &broker,
        &UpdateAttributes::builder().id(&vehicle_2_id).fragment(&fragment).build(),
        &format!("entities/{vehicle_2_id}/attrs/"),
    )
    .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let forwarded_request = SourceRequest::matching(Method::PATCH, "/ngsi-ld/v1/entit.*");
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(1)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_2_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    expect_that!(entity, has_attribute("speed", any_attribute().value(99)));
    Ok(())
}
