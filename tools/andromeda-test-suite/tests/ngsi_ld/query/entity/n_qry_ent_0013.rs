//! Post attrs alone selects and projects.

use andromeda_test_suite::{
    attribute_matcher::{property, relationship},
    broker,
    entity_matcher::{has_attribute, member_count},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    type_matcher::has_type,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{AttributeName, Entity, NgsiLdMediaType, Relationship},
};

/// Pins: a Query body with only attrs selects exactly the Entities holding a listed Attribute and answers each with
/// only those Attributes, by short name or by URI
/// Clause: 5.2.23, 6.23.3.1, 5.7.2
/// Tenant: private (its queries select by attributes, q or geometry alone, so the broker reads every entity and
/// forwards to every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_qry_ent_0013_post_attrs_alone_selects_and_projects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let parking_id = namespace.urn("OffStreetParking", "downtown1")?;
    let bob_id = namespace.urn("Person", "bob")?;
    let brand = format!("brandName_{namespace}");
    let parked = format!("isParked_{namespace}");
    let entities = [
        Entity::builder().id(building_id).entity_type(building_type).build()?,
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property(brand.as_str(), "Mercedes")
            .relationship(
                parked.as_str(),
                Relationship::builder(parking_id.clone())
                    .observed_at("2017-07-29T12:00:04Z".parse()?)
                    .relationship("providedBy", bob_id.clone()),
            )
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // attrs by short name; the attribute names carry the namespace, so the selection is exact in the shared tenant
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .attrs(vec![brand.parse::<AttributeName>()?, parked.parse::<AttributeName>()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let entity = page.items().first().or_fail()?;
    expect_that!(entity, all!(has_id(&vehicle_id), has_type(&vehicle_type)));
    expect_that!(member_count(entity), eq(4));
    expect_that!(entity, has_attribute(&brand, property().value("Mercedes")));
    expect_that!(entity, has_attribute(&parked, relationship().object(parking_id.clone())));
    // attrs by full IRI; the answer names every member by its expanded IRI
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .attrs(vec![
            format!("https://uri.etsi.org/ngsi-ld/default-context/{brand}").parse::<AttributeName>()?,
            format!("https://uri.etsi.org/ngsi-ld/default-context/{parked}").parse::<AttributeName>()?,
        ])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    let entity_2 = page_2.items().first().or_fail()?;
    expect_that!(entity_2, all!(has_id(&vehicle_id), has_type(&vehicle_type)));
    expect_that!(member_count(entity_2), eq(4));
    expect_that!(entity_2, has_attribute(&brand, property().value("Mercedes")));
    expect_that!(
        entity_2,
        has_attribute(
            &parked,
            relationship()
                .object(parking_id)
                .observed_at("2017-07-29T12:00:04Z".parse()?)
                .sub_attribute("providedBy", relationship().object(bob_id))
        )
    );
    Ok(())
}
