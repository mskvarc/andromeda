//! A sub attribute language property is read through its language map.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType, Property},
    query::q,
};

/// Pins: q door.label`[en]`=="Front" and door.label`[en]`~="^F" match the entity whose en label is Front,
/// door.label`[de]`=="Front" matches none and door.label`[en]`!="Front" matches the other, the target element being
/// the last name of the path
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0013_a_sub_attribute_language_property_is_read_through_its_language_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_front_id = namespace.urn("Building", "front")?;
    let building_type = namespace.entity_type("Building")?;
    let building_back_id = namespace.urn("Building", "back")?;
    let building_front = Entity::builder()
        .id(building_front_id.clone())
        .entity_type(building_type.clone())
        .property(
            "door",
            Property::builder(1).language_property(
                "label",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Front"), ("de", "Vorne")])?),
            ),
        )
        .build()?;
    broker.entities().create(&building_front).send().await?.done()?;
    let building_back = Entity::builder()
        .id(building_back_id.clone())
        .entity_type(building_type.clone())
        .property(
            "door",
            Property::builder(1).language_property(
                "label",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Back"), ("de", "Hinten")])?),
            ),
        )
        .build()?;
    broker.entities().create(&building_back).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("door").sub_attr("label").member("en").eq("Front"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&building_front_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("door").sub_attr("label").member("de").eq("Front"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("door").sub_attr("label").member("en").ne("Front"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), contains(has_id(&building_back_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("door").sub_attr("label").member("en").matches("\"^F\""))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), contains(has_id(&building_front_id)));
    Ok(())
}
