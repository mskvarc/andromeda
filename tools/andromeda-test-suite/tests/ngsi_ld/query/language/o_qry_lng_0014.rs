//! Retrieve with a language keeps observed at and dataset id.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{attribute_members, has_attribute},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: Retrieve Entity with lang=fr keeps the `observedAt` and `datasetId` of the converted `LanguageProperty`
/// Clause: 4.15, 5.7.1.5, 4.5.18
/// Recorded: clauses 4.15 and 5.7.1.5 convert the `LanguageProperty` into a Property and add a lang sub-property,
/// and say nothing of its `observedAt` and `datasetId`; Andromeda keeps both
#[gtest]
#[tokio::test]
async fn o_qry_lng_0014_retrieve_with_a_language_keeps_observed_at_and_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let street_source_dataset = namespace.dataset_id("Dataset", "street-source")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place")])?)
                .dataset_id(namespace.dataset_id("Dataset", "street-source")?)
                .observed_at("2024-01-01T12:00:00Z".parse()?),
        )
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
    expect_that!(attribute_members(entity, "street"), some(eq(5)));
    expect_that!(
        entity,
        has_attribute(
            "street",
            property()
                .value("Grand Place")
                .lang("fr")
                .observed_at("2024-01-01T12:00:00Z".parse()?)
                .with_dataset_id(street_source_dataset)
        )
    );
    Ok(())
}
