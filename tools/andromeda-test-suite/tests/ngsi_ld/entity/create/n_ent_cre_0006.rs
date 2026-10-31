//! A language property reads back with its language map.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::{Entity, Input, Lossless, NgsiLdMediaType, Output},
};

/// The entity as the request writes it.
const BODY: &str = r#"{
    "id": "urn:ngsi-ld:Building:{{ns}}-1",
    "type": "Building_{{ns}}",
    "name": {
        "type": "LanguageProperty",
        "languageMap": {
            "en": "Eiffel Tower",
            "fr": "Tour Eiffel",
            "de": "Eiffelturm"
        }
    }
}"#;

/// Pins: an entity created with a `LanguageProperty` answers 201 with a Location and reads back with the
/// `LanguageProperty` and every entry of its `languageMap`
/// Clause: 5.6.1, 4.5.18, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_0006_a_language_property_reads_back_with_its_language_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let body = BODY.replace("{{ns}}", namespace.token());
    let building = Entity::<Input>::parse(&body, Lossless::Normalized)?;
    let received = broker.with_responses().entities().create(&building).send().await?;
    received.outcome.done()?;
    expect_that!(
        received.head.typed::<Location>().valid().map(|location| location.as_iri_ref().as_str()),
        some(contains_substring(format!("urn:ngsi-ld:Building:{namespace}-1")))
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let expected = Entity::<Output>::parse(&body, Lossless::Normalized)?;
    expect_that!(retrieved.body(), eq(&expected));
    Ok(())
}
