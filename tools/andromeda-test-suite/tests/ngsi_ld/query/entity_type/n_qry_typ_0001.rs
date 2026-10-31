//! A comma separated type list is a disjunction.

use andromeda_test_suite::{
    attribute_matcher::{geo_property, property, relationship},
    broker,
    entity_matcher::{has_attribute, member_count},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        EntityId,
        GeoProperty,
        NgsiLdMediaType,
        OneOrMany,
        Property,
        Relationship,
        Value,
        geometry::{Position, Shape},
    },
    query::type_selection,
};

/// Pins: a comma-separated type list is a disjunction: each listed type selects its entities, every attribute of
/// each comes back, and an entity of an unlisted type is left out
/// Clause: 4.17, 5.7.2.4, 5.7.2.5
#[gtest]
#[tokio::test]
async fn n_qry_typ_0001_a_comma_separated_type_list_is_a_disjunction() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let monument_id = namespace.urn("Monument", 0)?;
    let vehicle_id = namespace.urn("Vehicle", "v")?;
    let parking_id = namespace.urn("OffStreetParking", "p")?;
    let monument_type = namespace.entity_type("Monument")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let parking_type = namespace.entity_type("OffStreetParking")?;
    let downtown_id = namespace.urn("OffStreetParking", "downtown1")?;
    let bob_id = namespace.urn("Person", "bob")?;
    let camera_id = namespace.urn("Camera", "c1")?;
    let entities = vec![
        Entity::builder().id(monument_id.clone()).entity_type(monument_type).build()?,
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("brandName", Property::builder("Mercedes"))
            .relationship(
                "isParked",
                Relationship::builder(downtown_id.clone())
                    .observed_at("2017-07-29T12:00:04Z".parse()?)
                    .relationship("providedBy", Relationship::builder(bob_id.clone())),
            )
            .build()?,
        Entity::builder()
            .id(parking_id.clone())
            .entity_type(parking_type.clone())
            .property("name", Property::builder("Downtown One"))
            .property(
                "availableSpotNumber",
                Property::builder(121)
                    .observed_at("2017-07-29T12:05:02Z".parse()?)
                    .property("reliability", Value::Number("0.7".parse()?))
                    .relationship("providedBy", Relationship::builder(camera_id.clone())),
            )
            .property("totalSpotNumber", Property::builder(200))
            .geo_property("location", GeoProperty::builder(Shape::Point(Position::try_new(-8.5, 41.2)?)))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(type_selection::entity_type(&vehicle_type).or(&parking_type))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), not(contains(has_id(&monument_id))));
    let vehicle = page.items().iter().find(|entity| entity.id == vehicle_id).or_fail()?;
    expect_that!(vehicle.entity_type, eq(&OneOrMany::one(vehicle_type.clone())));
    expect_that!(member_count(vehicle), eq(4));
    expect_that!(vehicle, has_attribute("brandName", property().value("Mercedes")));
    expect_that!(
        vehicle,
        has_attribute(
            "isParked",
            relationship()
                .object(downtown_id.clone())
                .observed_at("2017-07-29T12:00:04Z".parse()?)
                .sub_attribute("providedBy", relationship().object(bob_id.clone()))
        )
    );
    let parking = page.items().iter().find(|entity| entity.id == parking_id).or_fail()?;
    expect_that!(parking.entity_type, eq(&OneOrMany::one(parking_type.clone())));
    expect_that!(member_count(parking), eq(6));
    expect_that!(parking, has_attribute("name", property().value("Downtown One")));
    expect_that!(
        parking,
        has_attribute(
            "availableSpotNumber",
            property()
                .value(121)
                .observed_at("2017-07-29T12:05:02Z".parse()?)
                .sub_attribute("reliability", property().value(Value::Number("0.7".parse()?)))
                .sub_attribute("providedBy", relationship().object(camera_id.clone()))
        )
    );
    expect_that!(parking, has_attribute("totalSpotNumber", property().value(200)));
    expect_that!(
        parking,
        has_attribute(
            "location",
            geo_property()
                .value_member(["type"], "Point")
                .value_member(["coordinates", "0"], Value::Number("-8.5".parse()?))
                .value_member(["coordinates", "1"], Value::Number("41.2".parse()?))
        )
    );
    let building_1_id = namespace.urn("MultiTypes", 1)?;
    let building_2_id = namespace.urn("MultiTypes", 2)?;
    let building_3_id = namespace.urn("MultiTypes", 3)?;
    let building_type = namespace.entity_type("Building")?;
    let parking_kind_type = namespace.entity_type("Parking")?;
    let tourist_type = namespace.entity_type("TouristDestination")?;
    let multi_types = vec![
        Entity::builder()
            .id(building_1_id.clone())
            .entity_type(building_type.clone())
            .property("name", Property::builder("Eiffel Tower"))
            .build()?,
        Entity::builder().id(building_2_id.clone()).entity_type(parking_kind_type.clone()).build()?,
        Entity::builder()
            .id(building_3_id.clone())
            .entity_type(vec![building_type.clone(), tourist_type.clone()])
            .build()?,
    ];
    broker.entities().batch_create(&multi_types).send().await?.done()?;
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(type_selection::entity_type(&building_type).or(&parking_kind_type))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(3)));
    let first = page_2.items().iter().find(|entity| entity.id == building_1_id).or_fail()?;
    expect_that!(first.entity_type, eq(&OneOrMany::one(building_type.clone())));
    let second = page_2.items().iter().find(|entity| entity.id == building_2_id).or_fail()?;
    expect_that!(second.entity_type, eq(&OneOrMany::one(parking_kind_type.clone())));
    let third = page_2.items().iter().find(|entity| entity.id == building_3_id).or_fail()?;
    expect_that!(third.entity_type.as_slice().len(), eq(2));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(type_selection::entity_type(&parking_kind_type).or(&tourist_type))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    let ids = page_3.items().iter().map(|entity| entity.id.clone()).collect::<Vec<EntityId>>();
    expect_that!(ids, unordered_elements_are![eq(&building_2_id), eq(&building_3_id)]);
    let second_2 = page_3.items().iter().find(|entity| entity.id == building_2_id).or_fail()?;
    expect_that!(second_2.entity_type, eq(&OneOrMany::one(parking_kind_type.clone())));
    let third_2 = page_3.items().iter().find(|entity| entity.id == building_3_id).or_fail()?;
    expect_that!(third_2.entity_type.as_slice().len(), eq(2));
    expect_that!(page_3.items(), not(contains(has_id(&building_1_id))));
    Ok(())
}
