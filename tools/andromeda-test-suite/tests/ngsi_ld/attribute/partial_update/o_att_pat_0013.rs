//! A null deletes the relationship.

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
        AttributeKind,
        AttributeName,
        Change,
        Entity,
        InstanceDeletion,
        LanguageMap,
        LanguageProperty,
        NgsiLdMediaType,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: Partial Attribute Update of a Relationship with a Fragment whose content is NGSI-LD Null answers 204 and
/// deletes that Relationship, leaving the other Attributes in place
/// Clause: 5.6.4.4, 5.5.8, 4.5.0
/// Recorded: clause 5.6.4.4 says NGSI-LD Nulls "should" be supported by Partial Attribute Update
/// (`OperationNotSupported` otherwise), and clause 5.6.4.1 says the operation does not delete the whole Attribute,
/// while clause 5.5.8 deletes the target member whose Fragment value is NGSI-LD Null; Andromeda reads a Fragment
/// whose value, object or `languageMap` is NGSI-LD Null as deleting the targeted Attribute instance
#[gtest]
#[tokio::test]
async fn o_att_pat_0013_a_null_deletes_the_relationship() -> Result<()> {
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
    let deletion = Change::Delete(InstanceDeletion::new(AttributeKind::Relationship));
    broker
        .entities()
        .update_attribute(&building_id, &"locatedAt".parse::<AttributeName>()?, deletion)
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
