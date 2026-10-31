//! A concise null deletes a relationship and keeps the others.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        EntityFragment,
        LanguageMap,
        LanguageProperty,
        NgsiLdMediaType,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: Merge Entity with a fragment whose `locatedAt` member is the concise NGSI-LD Null "urn:ngsi-ld:null"
/// answers 204 and deletes that Relationship, leaving the other Attributes in place
/// Clause: 5.6.17.4, 5.5.12, 4.5.0
/// Recorded: clause 5.6.17.4 says NGSI-LD Nulls "should" be supported by Merge Entity (`OperationNotSupported`
/// otherwise); Andromeda supports them, and the deletion then follows clause 5.6.17.4 and clause 5.5.12
#[gtest]
#[tokio::test]
async fn o_ent_mrg_0010_a_concise_null_deletes_a_relationship_and_keeps_the_others() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let city_paris_id = namespace.urn("City", "Paris")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("name", "Eiffel Tower")
        .relationship("locatedAt", city_paris_id)
        .geo_property("location", Shape::Point(Position::try_new(2.2945, 48.8584)?))
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Champ de Mars"), ("fr", "Champ de Mars")])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragment = EntityFragment::builder().delete("locatedAt").build()?;
    broker.entities().merge(&building_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity, lacks_attribute("locatedAt"));
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(
        entity,
        has_attribute("location", any_attribute().value_member(["coordinates", "0"], Value::Number("2.2945".parse()?)))
    );
    expect_that!(entity, has_attribute("street", any_attribute().language("en", "Champ de Mars")));
    Ok(())
}
