//! Dataset id none on a relationship a geo property and a language property is the default instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        DatasetId,
        Entity,
        GeoProperty,
        LanguageMap,
        LanguageProperty,
        NgsiLdMediaType,
        Relationship,
        geometry::{Position, Shape},
    },
};

/// Pins: Create Entity accepts "`datasetId`": "@none" on a Relationship, a `GeoProperty` and a `LanguageProperty`
/// as the default instance, and the entity reads back without a `datasetId` on any of them
/// Clause: 4.5.5, 5.6.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_2301_dataset_id_none_on_a_relationship_a_geo_property_and_a_language_property_is_the_default_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let person_id = namespace.urn("Person", 1)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .relationship("owner", Relationship::builder(person_id.clone()).dataset_id("@none".parse::<DatasetId>()?))
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(13.4, 52.5)?)).dataset_id("@none".parse::<DatasetId>()?),
        )
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place")])?).dataset_id("@none".parse::<DatasetId>()?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("owner", any_attribute().object(person_id.clone()).without(Member::DatasetId))
    );
    expect_that!(
        entity,
        has_attribute("location", any_attribute().value_member(["type"], "Point").without(Member::DatasetId))
    );
    expect_that!(
        entity,
        has_attribute("street", any_attribute().language("fr", "Grand Place").without(Member::DatasetId))
    );
    Ok(())
}
