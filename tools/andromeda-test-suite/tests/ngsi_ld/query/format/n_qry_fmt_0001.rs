//! Options key values answers the simplified representation.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::{has_value, simplified_member_count},
    type_matcher::has_type,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, SimplifiedEntity},
};

/// Pins: Query Entities with `options=keyValues` answers each selected Building in the simplified representation
/// and leaves the unselected one out
/// Clause: 6.3.7
#[gtest]
#[tokio::test]
async fn n_qry_fmt_0001_options_key_values_answers_the_simplified_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let ids = [namespace.urn("Building", 1)?, namespace.urn("Building", 2)?, namespace.urn("Building", 3)?];
    let mut entities = Vec::new();
    for id in &ids {
        entities.push(
            Entity::builder()
                .id(id.clone())
                .entity_type(building_type.clone())
                .property("name", "Eiffel Tower")
                .property("subCategory", "tourism")
                .property(
                    "airQualityLevel",
                    Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00.000Z".parse()?),
                )
                .property("almostFull", false)
                .build()?,
        );
    }
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_simplified()
        .ids(vec![ids[0].clone(), ids[1].clone()])
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    for id in &ids[..2] {
        expect_that!(
            page.items(),
            contains(all!(
                has_id(id),
                has_type(&building_type),
                predicate(|item: &SimplifiedEntity| simplified_member_count(item) == 6),
                has_value("name", "Eiffel Tower"),
                has_value("airQualityLevel", 4),
                has_value("almostFull", false),
                has_value("subCategory", "tourism")
            ))
        );
    }
    expect_that!(page.items(), not(contains(has_id(&ids[2]))));
    Ok(())
}
