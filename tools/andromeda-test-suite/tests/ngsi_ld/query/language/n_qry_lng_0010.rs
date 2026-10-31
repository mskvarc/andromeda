//! Retrieve simplified with a language answers the bare string.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::{has_value, simplified_member_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: Retrieve Entity with lang=nl and `options=keyValues` answers the `LanguageProperty` as the bare Dutch
/// string
/// Clause: 4.15, 5.7.1.5, 4.5.4, Annex C (Normalized and Simplified Representation when the Language Filter is
/// used)
#[gtest]
#[tokio::test]
async fn n_qry_lng_0010_retrieve_simplified_with_a_language_answers_the_bare_string() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&building_id)
        .lang("nl".parse::<LanguageFilter>()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(simplified_member_count(entity), eq(3));
    expect_that!(entity, has_value("street", "Grote Markt"));
    Ok(())
}
