//! A language holding the null beside a string is bad request data.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::MergeEntity},
    model::{Entity, EntityFragment, ErrorType, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Merge Entity of a street whose en holds the NGSI-LD Null beside the string "Rue Cler" answers 400 `BadRequestData`, and the stored street keeps en "Champ de Mars"
/// Clause: 5.6.17.4, 5.5.12, 4.5.18.2, 5.5.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9028_a_language_holding_the_null_beside_a_string_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_mixed_id = namespace.urn("Building", "mixed")?;
    let building_type = namespace.entity_type("Building")?;
    let building_mixed = Entity::builder()
        .id(building_mixed_id.clone())
        .entity_type(building_type.clone())
        .language_property("street", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Champ de Mars")])?))
        .build()?;
    broker.entities().create(&building_mixed).send().await?.done()?;
    let fragment = EntityFragment::for_entity(building_mixed_id.clone(), building_type.clone()).build()?;
    // The model refuses it (a language holding the NGSI-LD Null beside a string: ["Rue Cler", "urn:ngsi-ld:null"]), so `street` is written as text.
    let street: RawJson = r#"{"type": "LanguageProperty", "languageMap": {"en": ["Rue Cler", "urn:ngsi-ld:null"]}}"#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("street", &street);
    let request = broker
        .unchecked(&MergeEntity::builder().id(&building_mixed_id).fragment(&fragment).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_mixed_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("street", any_attribute().language("en", "Champ de Mars")));
    Ok(())
}
