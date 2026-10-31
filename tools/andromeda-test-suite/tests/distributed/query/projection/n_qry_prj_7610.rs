//! A get query applies pick, omit and attrs to the entities a source returns.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    entity_lookup::with_id,
    entity_matcher::{has_attribute, lacks_attribute, member_count, projected_member_count},
    namespace::Namespace,
    outcome::Done,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::projection::Projection,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities on broker A by type through an inclusive registration over a source that answers whole
/// entities, with pick id,name then omit location then attrs name, answers the local and the source entity each
/// reduced to exactly the projected members
/// Clause: 4.21, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_prj_7610_a_get_query_applies_pick_omit_and_attrs_to_the_entities_a_source_returns() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let local_id = namespace.urn("Vehicle", 1)?;
    let remote_id = namespace.urn("Vehicle", 2)?;
    let remote = Entity::output_builder()
        .id(remote_id.clone())
        .entity_type(vehicle_type.clone())
        .property("name", "Remote")
        .property("speed", 43)
        .geo_property("location", Shape::Point(Position::try_new(2.0, 41.0)?))
        .build()?;
    source.answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::entities(&[remote])).await?;
    let local = Entity::builder()
        .id(local_id.clone())
        .entity_type(vehicle_type.clone())
        .property("name", "Local")
        .property("speed", 42)
        .geo_property("location", Shape::Point(Position::try_new(3.0, 40.0)?))
        .build()?;
    broker.entities().create(&local).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let picked = json
        .entities()
        .query_projected()
        .entity_type(&vehicle_type)
        .pick("id,name".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(picked.items(), len(eq(2)));
    for id in [&local_id, &remote_id] {
        let entity = picked.items().iter().find(|entity| entity.id.as_ref() == Some(id)).or_fail()?;
        expect_that!(projected_member_count(entity), eq(2));
        expect_that!(entity.entity_type, none());
        expect_that!(entity, has_attribute("name", any_attribute()));
        expect_that!(entity, lacks_attribute("speed"));
        expect_that!(entity, lacks_attribute("location"));
    }
    let omitted = json
        .entities()
        .query_projected()
        .entity_type(&vehicle_type)
        .omit("location".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(omitted.items(), len(eq(2)));
    for id in [&local_id, &remote_id] {
        let entity = omitted.items().iter().find(|entity| entity.id.as_ref() == Some(id)).or_fail()?;
        expect_that!(projected_member_count(entity), eq(4));
        expect_that!(entity.entity_type, some(anything()));
        expect_that!(entity, has_attribute("name", any_attribute()));
        expect_that!(entity, has_attribute("speed", any_attribute()));
        expect_that!(entity, lacks_attribute("location"));
    }
    let attributes = json.entities().query().entity_type(&vehicle_type).attrs(vec!["name".parse()?]).send().await?;
    expect_that!(attributes.items(), len(eq(2)));
    for id in [&local_id, &remote_id] {
        let entity = with_id(attributes.items(), id).or_fail()?;
        expect_that!(member_count(entity), eq(3));
        expect_that!(entity, has_attribute("name", any_attribute()));
        expect_that!(entity, lacks_attribute("speed"));
        expect_that!(entity, lacks_attribute("location"));
    }
    Ok(())
}
