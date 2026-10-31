//! A partial attribute update records the resulting instance with its unit code.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, NgsiLdMediaType, Property},
    query::{Locality, q},
};

/// Pins: Partial Attribute Update of temperature 25 CEL with value 100 and a new `observedAt` answers 204; the
/// Entity holds 100 CEL, the Temporal Evolution gains one instance holding 100 with `unitCode` CEL and an
/// `observedAt`, and q selects temperature==100 and no longer temperature==25
/// Clause: 5.6.4, 5.5.8 (EXAMPLE 1), 4.5.7, 4.3.1, 4.9
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9003_a_partial_attribute_update_records_the_resulting_instance_with_its_unit_code() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_partial_unit_code_id = namespace.urn("Sensor", "partial-unit-code")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_partial_unit_code = Entity::builder()
        .id(sensor_partial_unit_code_id.clone())
        .entity_type(sensor_type.clone())
        .property(
            "temperature",
            Property::builder(25).unit_code(UnitCode::Cel).observed_at("2022-03-14T01:59:26.535Z".parse()?),
        )
        .build()?;
    broker.entities().create(&sensor_partial_unit_code).send().await?.done()?;
    let patch = Attribute::from(Property::builder(100).observed_at("2022-03-14T13:00:00.5Z".parse()?).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&sensor_partial_unit_code_id, &"temperature".parse::<AttributeName>()?, patch)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_partial_unit_code_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", any_attribute().value(100).unit_code(UnitCode::Cel)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_partial_unit_code_id)
        .attrs(vec!["temperature".parse()?])
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        evolution,
        has_evolution(
            "temperature",
            all!(
                len(eq(2)),
                contains(any_attribute().value(100)),
                contains(any_attribute().value(100).unit_code(UnitCode::Cel)),
                contains(any_attribute().value(100)).times(eq(1)),
                contains(any_attribute().value(100).with_member(Member::ObservedAt))
            )
        )
    );
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .q(q::attr("temperature").eq(100))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&sensor_partial_unit_code_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .q(q::attr("temperature").eq(25))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
