//! A null with a dataset id deletes only that property instance.

use andromeda_test_suite::{
    attribute_matcher::property,
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
        Entity,
        EntityFragment,
        GeoProperty,
        LanguageMap,
        LanguageProperty,
        NgsiLdMediaType,
        Property,
        Relationship,
        geometry::{Position, Shape},
    },
};

/// Pins: Merge Entity with a Property whose value is NGSI-LD Null and whose `datasetId` names one of two instances
/// answers 204 and deletes that instance only; the other instance and the other Attributes stay
/// Clause: 5.6.17.4, 5.5.12, 4.5.0, 4.5.5
/// Recorded: clause 5.6.17.4 says NGSI-LD Nulls "should" be supported by Merge Entity (`OperationNotSupported`
/// otherwise); Andromeda supports them, and the deletion then follows clause 5.6.17.4 and clause 5.5.12
#[gtest]
#[tokio::test]
async fn o_ent_mrg_0013_a_null_with_a_dataset_id_deletes_only_that_property_instance() -> Result<()> {
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
        .property("name", Property::builder("Eiffel Tower").dataset_id(namespace.dataset_id("Dataset", "01")?))
        .property("name", Property::builder("La Tour Eiffel").dataset_id(namespace.dataset_id("Dataset", "02")?))
        .relationship(
            "locatedAt",
            Relationship::builder(city_paris_id).dataset_id(namespace.dataset_id("Dataset", "01")?),
        )
        .relationship(
            "locatedAt",
            Relationship::builder(district_7th_id).dataset_id(namespace.dataset_id("Dataset", "02")?),
        )
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(2.2945, 48.8584)?)).dataset_id(namespace.dataset_id("Dataset", "01")?),
        )
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(2.2944, 48.8583)?)).dataset_id(namespace.dataset_id("Dataset", "02")?),
        )
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Champ de Mars"), ("fr", "Champ de Mars")])?)
                .dataset_id(namespace.dataset_id("Dataset", "01")?),
        )
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Avenue Gustave Eiffel"), ("fr", "Avenue Gustave Eiffel")])?)
                .dataset_id(namespace.dataset_id("Dataset", "02")?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .delete_instance("name", AttributeKind::Property, namespace.dataset_id("Dataset", "01")?)
        .build()?;
    broker.entities().merge(&building_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("locatedAt", 2));
    expect_that!(entity, has_instances("location", 2));
    expect_that!(entity, has_instances("street", 2));
    expect_that!(entity, has_attribute("name", property().with_dataset_id(dataset_02).value("La Tour Eiffel")));
    Ok(())
}
