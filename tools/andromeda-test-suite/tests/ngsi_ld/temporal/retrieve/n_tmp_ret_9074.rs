//! A declared number beyond the decimals reads back from every place a write stores it.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    value::xsd_typed,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
    query::temporal_query,
};

/// Pins: a Sensor created with `peak` "INF" declared xsd:double holding the sub-attribute `raw` "NaN" declared
/// xsd:double, and `low` "-INF" declared xsd:double then updated to "NaN", reads back `peak` "INF" with `raw` "NaN"
/// and `low` "NaN", each with its @type, and its Temporal Evolution holds `peak` "INF" and `low` "-INF" then "NaN"
/// Clause: 4.5.2, 4.6.4, 5.6.2, 5.7.3.4, C.6
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9074_a_declared_non_finite_number_reads_back_from_every_place_it_is_stored() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(namespace.entity_type("Sensor")?)
        .property(
            "peak",
            Property::builder(xsd_typed("double", "INF")?)
                .observed_at("2024-01-01T00:00:00Z".parse()?)
                .property("raw", xsd_typed("double", "NaN")?),
        )
        .property(
            "low",
            Property::builder(xsd_typed("double", "-INF")?).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property(
            "low",
            Property::builder(xsd_typed("double", "NaN")?).observed_at("2024-01-02T00:00:00Z".parse()?),
        )
        .build()?;
    broker.entities().update_attributes(&sensor_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "peak",
            any_attribute()
                .value_member_as_written([], xsd_typed("double", "INF")?)
                .sub_attribute("raw", any_attribute().value_member_as_written([], xsd_typed("double", "NaN")?))
        )
    );
    expect_that!(
        entity,
        has_attribute("low", any_attribute().value_member_as_written([], xsd_typed("double", "NaN")?))
    );
    let evolution_answer = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = evolution_answer.body();
    expect_that!(
        evolution,
        has_evolution(
            "peak",
            all!(len(eq(1)), nth(0, any_attribute().value_member_as_written([], xsd_typed("double", "INF")?)))
        )
    );
    expect_that!(
        evolution,
        has_evolution(
            "low",
            all!(
                len(eq(2)),
                contains(any_attribute().value_member_as_written([], xsd_typed("double", "-INF")?)),
                contains(any_attribute().value_member_as_written([], xsd_typed("double", "NaN")?))
            )
        )
    );
    Ok(())
}
