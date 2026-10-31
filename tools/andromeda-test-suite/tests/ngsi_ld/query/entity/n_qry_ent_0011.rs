//! Post an entity selector with a type answers every entity of it.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property, relationship},
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
    model::{Entity, NgsiLdMediaType, Relationship},
};

/// Pins: Query Entities by POST with an `EntitySelector` holding only a type answers every Entity of that type,
/// each with all its Attributes, and no Entity of another type
/// Clause: 5.2.23, 5.2.33, 6.23.3.1, 4.17
#[gtest]
#[tokio::test]
async fn n_qry_ent_0011_post_an_entity_selector_with_a_type_answers_every_entity_of_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "downtown1")?;
    let person_bob_id = namespace.urn("Person", "bob")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let entities = [
        Entity::builder().id(building_id.clone()).entity_type(building_type).build()?,
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("brandName", "Mercedes")
            .relationship(
                "isParked",
                Relationship::builder(off_street_parking_downtown1_id.clone())
                    .observed_at("2017-07-29T12:00:04Z".parse()?)
                    .relationship("providedBy", person_bob_id.clone()),
            )
            .build()?,
        Entity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .property("brandName", "Mercedes")
            .relationship(
                "isParked",
                Relationship::builder(off_street_parking_downtown1_id.clone())
                    .observed_at("2017-07-29T12:00:04Z".parse()?)
                    .relationship("providedBy", person_bob_id.clone()),
            )
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items().first().map(member_count), some(eq(4)));
    expect_that!(page.items().get(1).map(member_count), some(eq(4)));
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&vehicle_id),
            has_attribute("brandName", any_attribute().value("Mercedes")),
            has_attribute(
                "isParked",
                any_attribute()
                    .object(off_street_parking_downtown1_id.clone())
                    .sub_attribute("providedBy", any_attribute().object(person_bob_id.clone()))
            ),
            has_type(&vehicle_type)
        ))
    );
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&vehicle_2_id),
            has_attribute("brandName", any_attribute().value("Mercedes")),
            has_attribute(
                "isParked",
                any_attribute()
                    .object(off_street_parking_downtown1_id.clone())
                    .sub_attribute("providedBy", any_attribute().object(person_bob_id.clone()))
            ),
            has_type(&vehicle_type)
        ))
    );
    expect_that!(page.items(), not(contains(has_id(&building_id))));
    // the type as a URI with no user context, answered as application/ld+json
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(format!("https://uri.etsi.org/ngsi-ld/default-context/Vehicle_{namespace}").as_str())
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(
        page_2.items(),
        contains(all!(
            has_id(&vehicle_id),
            has_attribute("brandName", property().value("Mercedes")),
            has_attribute(
                "isParked",
                relationship()
                    .object(off_street_parking_downtown1_id.clone())
                    .observed_at("2017-07-29T12:00:04Z".parse()?)
                    .sub_attribute("providedBy", any_attribute().object(person_bob_id.clone()))
            ),
            has_type(&vehicle_type)
        ))
    );
    expect_that!(
        page_2.items(),
        contains(all!(
            has_id(&vehicle_2_id),
            has_attribute("brandName", property().value("Mercedes")),
            has_attribute(
                "isParked",
                relationship()
                    .object(off_street_parking_downtown1_id.clone())
                    .observed_at("2017-07-29T12:00:04Z".parse()?)
                    .sub_attribute("providedBy", any_attribute().object(person_bob_id.clone()))
            ),
            has_type(&vehicle_type)
        ))
    );
    expect_that!(page_2.items(), not(contains(has_id(&building_id))));
    Ok(())
}
