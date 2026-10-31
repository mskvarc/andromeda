//! A language map is replaced and sub attributes stay.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, attribute_of},
    broker,
    entity_matcher::has_attribute,
    language_map_matcher::has_language_map,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeKind, AttributeName, Entity, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType},
};

/// Pins: Partial Attribute Update of a `LanguageProperty` with a languageMap answers 204, the languageMap becomes the
/// one of the Fragment and the sub-Property stays
/// Clause: 5.6.4.4, 5.5.8, 4.5.18
#[gtest]
#[tokio::test]
async fn n_att_pat_0011_a_language_map_is_replaced_and_sub_attributes_stay() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Champ de Mars"), ("fr", "Champ de Mars")])?).property("verified", true),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragment =
        Attribute::from(LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Grand Place"), ("es", "Gran Lugar")])?).build()?).into_patch();
    let answer = broker
        .with_responses()
        .entities()
        .update_attribute(&building_id, &"street".parse::<AttributeName>()?, fragment)
        .form(Lossless::Concise)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "street",
            attribute_of(AttributeKind::LanguageProperty)
                .language("es", "Gran Lugar")
                .language("en", "Grand Place")
                .sub_attribute("verified", any_attribute().value(true))
        )
    );
    expect_that!(entity, has_language_map("street", &[("en", "Grand Place"), ("es", "Gran Lugar")]));
    Ok(())
}
