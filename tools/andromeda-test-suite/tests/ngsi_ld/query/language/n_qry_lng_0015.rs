//! Retrieve with a language leaves a property and a relationship as they are.

use andromeda_test_suite::{
    attribute_matcher::{property, relationship},
    attribute_member::Member,
    broker,
    entity_matcher::{attribute_members, has_attribute, member_count},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: Retrieve Entity with lang=fr converts only the `LanguageProperty` and leaves a Property and a Relationship
/// as they are
/// Clause: 4.15, 5.7.1.5
#[gtest]
#[tokio::test]
async fn n_qry_lng_0015_retrieve_with_a_language_leaves_a_property_and_a_relationship_as_they_are() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let paris_id = namespace.urn("City", "Paris")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?),
        )
        .property("name", "Eiffel Tower")
        .relationship("locatedIn", paris_id.clone())
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .lang("fr".parse::<LanguageFilter>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(member_count(entity), eq(5));
    expect_that!(attribute_members(entity, "name"), some(eq(2)));
    expect_that!(entity, has_attribute("name", property().value("Eiffel Tower")));
    expect_that!(attribute_members(entity, "street"), some(eq(3)));
    expect_that!(
        entity,
        has_attribute("street", property().value("Grand Place").lang("fr").without(Member::LanguageMap))
    );
    expect_that!(attribute_members(entity, "locatedIn"), some(eq(2)));
    expect_that!(entity, has_attribute("locatedIn", relationship().object(paris_id)));
    Ok(())
}
