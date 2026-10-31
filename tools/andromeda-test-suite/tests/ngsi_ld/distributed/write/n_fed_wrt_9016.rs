//! A local write forwards nothing to a matching registration.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UpsertOutcome, headers::Accept},
    model::{Attribute, AttributeName, Entity, EntityFragment, NgsiLdMediaType, Property},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};
use std::slice;

/// Pins: Under an inclusive registration matching the Entity type, Append Attributes, Update Attributes, Partial
/// Attribute Update, Replace Attribute and the batch update, upsert, merge, create and delete, each with local=true,
/// succeed locally and forward no request to the registration
/// Clause: 6.3.18, 5.5.13, 5.6.2.4, 5.6.3.4, 5.6.4.4, 5.6.19.4, 5.6.7.4, 5.6.8.4, 5.6.9.4, 5.6.10.4, 5.6.20.4
/// Tenant: private (its source must receive nothing, and another file's discovery or query that names no type
/// reaches every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9016_a_local_write_forwards_nothing_to_a_matching_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let speed_name: AttributeName = "speed".parse()?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 0)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::RedirectionOps])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    source
        .answer_when(SourceRequest::any_matching("/a/.*"), source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    let append = EntityFragment::builder().property("speed", 1).build()?;
    broker
        .entities()
        .append_attributes(&vehicle_id, &append)
        .locality(Locality::Local)
        .send()
        .await?
        .done()?;
    let update = EntityFragment::builder().property("speed", 2).build()?;
    broker
        .entities()
        .update_attributes(&vehicle_id, &update)
        .locality(Locality::Local)
        .send()
        .await?
        .done()?;
    let patch = Attribute::from(Property::builder(3).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&vehicle_id, &speed_name, patch)
        .locality(Locality::Local)
        .send()
        .await?
        .done()?;
    let replacement = Attribute::from(Property::builder(4).build()?);
    broker
        .entities()
        .replace_attribute(&vehicle_id, &speed_name, &replacement)
        .locality(Locality::Local)
        .send()
        .await?
        .done()?;
    let batch_update = [EntityFragment::for_entity(vehicle_id.clone(), vehicle_type.clone())
        .property("speed", 5)
        .build()?];
    broker.entities().batch_update(&batch_update).locality(Locality::Local).send().await?.done()?;
    let batch_upsert = [Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 6)
        .build()?];
    let upserted = broker.entities().batch_upsert(&batch_upsert).locality(Locality::Local).send().await?.done()?;
    expect_that!(upserted, eq(&UpsertOutcome::Updated));
    let batch_merge = [EntityFragment::for_entity(vehicle_id.clone(), vehicle_type.clone())
        .property("speed", 7)
        .build()?];
    broker.entities().batch_merge(&batch_merge).locality(Locality::Local).send().await?.done()?;
    let batch_create = [Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1)
        .build()?];
    broker.entities().batch_create(&batch_create).locality(Locality::Local).send().await?.done()?;
    broker
        .entities()
        .batch_delete(slice::from_ref(&vehicle_2_id))
        .locality(Locality::Local)
        .send()
        .await?
        .done()?;
    let forwarded_request = SourceRequest::any_matching("/a/.*");
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(0)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_attribute("speed", any_attribute().value(7)));
    let missing = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_2_id)
        .locality(Locality::Local)
        .send()
        .await;
    expect_that!(&missing, answered(StatusCode::NOT_FOUND));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
