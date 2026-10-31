//! An object value round trips through every reading.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    values_matcher::{has_values, pair},
};
use googletest::prelude::*;
use indexmap::IndexMap;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: A Property whose value is the JSON object {"unit": "km", "amount": 3} reads back as that object from every
/// temporal reading and the current representation
/// Clause: 5.2.5, 4.5.7, 4.5.9, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_trl_9005_an_object_value_round_trips_through_every_reading() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let array_probe_id = namespace.urn("ArrayProbe", 1)?;
    let array_probe_type = namespace.entity_type("ArrayProbe")?;
    let observed_at = "2024-01-01T00:00:00Z".parse::<DateTime>()?;
    let distance = Value::Object(IndexMap::from([("unit".to_owned(), Value::from("km")), ("amount".to_owned(), Value::from(3))]));
    let array_probe_evolution = TemporalEntity::builder()
        .id(array_probe_id.clone())
        .entity_type(array_probe_type.clone())
        .property("reading", Property::builder(distance.clone()).observed_at(observed_at))
        .build()?;
    broker.temporal().create_or_update(&array_probe_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&array_probe_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_evolution("reading", nth(0, property().value_member(["unit"], "km").value_member(["amount"], 3)))
    );
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&array_probe_type)
        .ids(vec![array_probe_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            has_evolution("reading", nth(0, any_attribute().value_member(["unit"], "km").value_member(["amount"], 3)))
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_values(&array_probe_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(retrieved_2.body(), has_values("reading", nth(0, pair().value(distance).time(observed_at))));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&array_probe_id)
        .send()
        .await?;
    expect_that!(
        retrieved_3.body(),
        has_attribute("reading", any_attribute().value_member(["unit"], "km").value_member(["amount"], 3))
    );
    Ok(())
}
