//! An id pattern folds the part its group encloses.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, NgsiLdMediaType},
    query::IdPattern,
};

/// Pins: `idPattern` ^urn:ngsi-ld:Bee:<ns>:(?i:hive): selects the identifiers whose segment is Hive, HIVE or hive
/// and not Hivex, comparing the prefix exactly
/// Clause: 4.9, 5.7.2.4
/// Recorded: clause 4.9 hands `RegExp` to IEEE 1003.2, which has no inline options and no shorthand classes;
/// Andromeda admits the case-insensitive option and the shorthand classes \d, \w and \s as extensions, folding case
/// by Unicode simple case folding and classifying by Unicode
#[gtest]
#[tokio::test]
async fn a_qry_ent_0017_an_id_pattern_folds_the_part_its_group_encloses() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let hive_type = namespace.entity_type("Hive")?;
    let ngsi_ld_bee_id = format!("urn:ngsi-ld:Bee:{namespace}:Hive:1").parse::<EntityId>()?;
    let ngsi_ld_bee_id_2 = format!("urn:ngsi-ld:Bee:{namespace}:HIVE:2").parse::<EntityId>()?;
    let ngsi_ld_bee_id_3 = format!("urn:ngsi-ld:Bee:{namespace}:hive:3").parse::<EntityId>()?;
    let ngsi_ld_bee_id_4 = format!("urn:ngsi-ld:Bee:{namespace}:Hivex:4").parse::<EntityId>()?;
    let entities = [
        Entity::builder()
            .id(format!("urn:ngsi-ld:Bee:{namespace}:Hive:1").parse::<EntityId>()?)
            .entity_type(hive_type.clone())
            .build()?,
        Entity::builder()
            .id(format!("urn:ngsi-ld:Bee:{namespace}:HIVE:2").parse::<EntityId>()?)
            .entity_type(hive_type.clone())
            .build()?,
        Entity::builder()
            .id(format!("urn:ngsi-ld:Bee:{namespace}:hive:3").parse::<EntityId>()?)
            .entity_type(hive_type.clone())
            .build()?,
        Entity::builder()
            .id(format!("urn:ngsi-ld:Bee:{namespace}:Hivex:4").parse::<EntityId>()?)
            .entity_type(hive_type.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&hive_type)
        .id_pattern(format!("^urn:ngsi-ld:Bee:{namespace}:(?i:hive):").parse::<IdPattern>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), contains(has_id(&ngsi_ld_bee_id)));
    expect_that!(page.items(), contains(has_id(&ngsi_ld_bee_id_2)));
    expect_that!(page.items(), contains(has_id(&ngsi_ld_bee_id_3)));
    expect_that!(page.items(), not(contains(has_id(&ngsi_ld_bee_id_4))));
    Ok(())
}
