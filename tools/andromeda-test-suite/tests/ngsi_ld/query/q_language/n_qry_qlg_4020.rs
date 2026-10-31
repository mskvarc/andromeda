//! Q on a sub attribute matches a value held only by its second instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Number, Property, Value},
    query::q,
};

/// Pins: q temperature.accuracy<0.2 answers the entity whose temperature carries two accuracy instances, 0.5 by
/// default and 0.1 under a `datasetId`, and not the entity whose only accuracy is 0.5
/// Clause: 4.9, 4.5.5
/// Recorded: clause 4.9 makes the last name of the attribute path the target element, a (sub)Property here, and "If
/// a Property has multiple instances (identified by its respective `datasetId`), and no `datasetId` is explicitly
/// addressed, the target value shall be any Value of such instances"
#[gtest]
#[tokio::test]
async fn n_qry_qlg_4020_q_on_a_sub_attribute_matches_a_value_held_only_by_its_second_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_two_id = namespace.urn("Sensor", "two")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_one_id = namespace.urn("Sensor", "one")?;
    let entities = [
        Entity::builder()
            .id(sensor_two_id.clone())
            .entity_type(sensor_type.clone())
            .property(
                "temperature",
                Property::builder(20).property("accuracy", Value::Number("0.5".parse()?)).property(
                    "accuracy",
                    Property::builder(Value::Number("0.1".parse()?)).dataset_id(namespace.dataset_id("Dataset", "calibrated")?),
                ),
            )
            .build()?,
        Entity::builder()
            .id(sensor_one_id)
            .entity_type(sensor_type.clone())
            .property("temperature", Property::builder(20).property("accuracy", Value::Number("0.5".parse()?)))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_two_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", any_attribute().sub_attribute_instances("accuracy", 2)));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("temperature").sub_attr("accuracy").lt("0.2".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_two_id)));
    Ok(())
}
