//! Retrieve with a language keeps an array of strings.

use andromeda_test_suite::{attribute_matcher::property, broker, entity_matcher::has_attribute, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, Lossless, NgsiLdMediaType, RawJson},
    query::language_filter::LanguageFilter,
};

/// Pins: Retrieve Entity with lang=fr answers the array of strings the fr entry holds as the value of the Property
/// Clause: 4.15, 5.7.1.5 (the string or strings from the matching key-value pair), 4.5.18
#[gtest]
#[tokio::test]
async fn n_qry_lng_0017_retrieve_with_a_language_keeps_an_array_of_strings() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    // The model refuses it (a language map entry with several strings), so `street` is written as text.
    let street: RawJson = r#"{"type": "LanguageProperty", "languageMap": {"fr": ["Grand Place", "Place Royale"], "nl": "Grote Markt"}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building, Lossless::Normalized).set_member("street", &street);
    let request = broker.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .lang("fr".parse::<LanguageFilter>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "street",
            property()
                .length_at([], 2)
                .value_member(["0"], "Grand Place")
                .value_member(["1"], "Place Royale")
                .lang("fr")
        )
    );
    Ok(())
}
