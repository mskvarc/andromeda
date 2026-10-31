//! A default sub attribute is added beside a dataset one.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DatasetId, Entity, EntityFragment, NgsiLdMediaType, Property, Value},
};

/// Pins: Merge Entity of speed with an accuracy without `datasetId` onto speed whose accuracy is of dataset a
/// leaves both accuracy instances
/// Clause: 4.5.5.1, 5.5.12, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9019_a_default_sub_attribute_is_added_beside_a_dataset_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_default_id = namespace.urn("Vehicle", "default")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let a_dataset = namespace.dataset_id("Dataset", "a")?;
    let vehicle_default = Entity::builder()
        .id(vehicle_default_id.clone())
        .entity_type(vehicle_type)
        .property(
            "speed",
            Property::builder(10).property(
                "accuracy",
                Property::builder(Value::Number("0.1".parse()?)).dataset_id(namespace.dataset_id("Dataset", "a")?),
            ),
        )
        .build()?;
    broker.entities().create(&vehicle_default).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("speed", Property::builder(20).property("accuracy", Value::Number("0.2".parse()?)))
        .build()?;
    broker.entities().merge(&vehicle_default_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_default_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "speed",
            any_attribute()
                .sub_attribute_instances("accuracy", 2)
                .sub_attribute("accuracy", any_attribute().dataset_id(a_dataset).value(Value::Number("0.1".parse()?)))
                .sub_attribute(
                    "accuracy",
                    any_attribute().dataset_id(DatasetId::DefaultInstance).value(Value::Number("0.2".parse()?))
                )
        )
    );
    Ok(())
}
