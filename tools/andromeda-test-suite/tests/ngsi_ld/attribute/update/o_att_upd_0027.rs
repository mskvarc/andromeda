//! A null without a dataset id deletes only the default language property instance.

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

/// Pins: Update Attributes with a `LanguageProperty` whose value is NGSI-LD Null and which carries no `datasetId`
/// answers 204 and deletes the default instance only; the instance with a `datasetId` and the other Attributes stay
/// Clause: 5.6.2.4, 5.5.8, 4.5.0, 4.5.5
/// Recorded: clause 5.6.2.4 says NGSI-LD Nulls "should" be supported by Update Attributes (`OperationNotSupported`
/// otherwise); Andromeda supports them, and the deletion then follows clause 5.6.2.4 and clause 5.5.8
#[gtest]
#[tokio::test]
async fn o_att_upd_0027_a_null_without_a_dataset_id_deletes_only_the_default_language_property_instance() -> Result<()> {
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
    let fragment = EntityFragment::builder().delete_kind("street", AttributeKind::LanguageProperty).build()?;
    broker.entities().update_attributes(&building_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("name", 2));
    expect_that!(entity, has_instances("locatedAt", 2));
    expect_that!(entity, has_instances("location", 2));
    expect_that!(
        entity,
        has_attribute("street", any_attribute().with_dataset_id(dataset_02).language("en", "Avenue Gustave Eiffel"))
    );
    Ok(())
}
