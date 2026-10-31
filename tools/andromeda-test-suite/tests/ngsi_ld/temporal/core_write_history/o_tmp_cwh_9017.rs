//! A later kind change leaves the recorded instance as it was.

use andromeda_test_suite::{
    attribute_matcher::property,
    body_json::json_count,
    broker,
    evolution_answer::{retrieve_unread, whole_history},
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property, Relationship},
    query::temporal_query,
};

/// Pins: After signal, created as Property 5 observed at 2021-01-01, is updated to a Relationship observed at
/// 2021-02-01 and to Property "amber" observed at 2021-03-01, the Temporal Evolution holds three instances, and the
/// window around 2021-01-01 holds the first alone, still a Property holding 5 with no object
/// Clause: 4.5.7, 4.5.8, 5.6.2, 5.7.3, 4.3.1
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9017_a_later_kind_change_leaves_the_recorded_instance_as_it_was() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_kind_change_id = namespace.urn("Sensor", "kind-change")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_kind_change = Entity::builder()
        .id(sensor_kind_change_id.clone())
        .entity_type(sensor_type)
        .property("signal", Property::builder(5).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&sensor_kind_change).send().await?.done()?;
    let to_relationship = EntityFragment::builder()
        .relationship(
            "signal",
            Relationship::builder(namespace.urn("Thing", 1)?).observed_at("2021-02-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker
        .entities()
        .update_attributes(&sensor_kind_change_id, &to_relationship)
        .send()
        .await?
        .done()?;
    let to_amber = EntityFragment::builder()
        .property("signal", Property::builder("amber").observed_at("2021-03-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&sensor_kind_change_id, &to_amber).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    // The typed reader holds the instances of one attribute as one kind, so the history that changes kind is read as received.
    let history = retrieve_unread(&json, &sensor_kind_change_id, whole_history()).await?;
    expect_that!(history.status(), eq(StatusCode::OK));
    expect_that!(history.body(), json_count("$.signal", 3));
    let window = json
        .temporal()
        .retrieve(&sensor_kind_change_id)
        .temporal_q(temporal_query::between("2020-12-31T00:00:00Z".parse()?, "2021-01-02T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(window.body(), has_evolution("signal", all!(len(eq(1)), nth(0, property().value(5)))));
    Ok(())
}
