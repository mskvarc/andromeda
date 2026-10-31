//! A null deletes only the default geo property instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeKind,
        AttributeName,
        Change,
        Entity,
        GeoProperty,
        InstanceDeletion,
        LanguageMap,
        LanguageProperty,
        NgsiLdMediaType,
        Property,
        Relationship,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: Partial Attribute Update of a `GeoProperty` with no `datasetId` and a Fragment whose content is NGSI-LD
/// Null answers 204 and deletes the default instance only; the instance with a `datasetId` and the other Attributes
/// stay
/// Clause: 5.6.4.4, 5.5.8, 4.5.0, 4.5.5
/// Recorded: clause 5.6.4.4 says NGSI-LD Nulls "should" be supported by Partial Attribute Update
/// (`OperationNotSupported` otherwise), and clause 5.6.4.1 says the operation does not delete the whole Attribute,
/// while clause 5.5.8 deletes the target member whose Fragment value is NGSI-LD Null; Andromeda reads a Fragment
/// whose value, object or `languageMap` is NGSI-LD Null as deleting the targeted Attribute instance
#[gtest]
#[tokio::test]
async fn o_att_pat_0022_a_null_deletes_only_the_default_geo_property_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let city_paris_id = namespace.urn("City", "Paris")?;
    let district_7th_id = namespace.urn("District", "7th")?;
    let dataset_02 = namespace.dataset_id("Dataset", "02")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("name", "Eiffel Tower")
        .property("name", Property::builder("La Tour Eiffel").dataset_id(namespace.dataset_id("Dataset", "02")?))
        .relationship("locatedAt", city_paris_id)
        .relationship(
            "locatedAt",
            Relationship::builder(district_7th_id).dataset_id(namespace.dataset_id("Dataset", "02")?),
        )
        .geo_property("location", Shape::Point(Position::try_new(2.2945, 48.8584)?))
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(2.2944, 48.8583)?)).dataset_id(namespace.dataset_id("Dataset", "02")?),
        )
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Champ de Mars"), ("fr", "Champ de Mars")])?),
        )
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Avenue Gustave Eiffel"), ("fr", "Avenue Gustave Eiffel")])?)
                .dataset_id(namespace.dataset_id("Dataset", "02")?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let deletion = Change::Delete(InstanceDeletion::new(AttributeKind::GeoProperty));
    broker
        .entities()
        .update_attribute(&building_id, &"location".parse::<AttributeName>()?, deletion)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("name", 2));
    expect_that!(entity, has_instances("locatedAt", 2));
    expect_that!(entity, has_instances("street", 2));
    expect_that!(
        entity,
        has_attribute(
            "location",
            any_attribute()
                .with_dataset_id(dataset_02)
                .value_member(["coordinates", "0"], Value::Number("2.2944".parse()?))
        )
    );
    Ok(())
}
