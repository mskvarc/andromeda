//! A sub attribute merges only into the instance of its dataset.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property, Value},
};

/// Pins: Merge Entity of speed with an accuracy of dataset a onto accuracy instances of datasets a and b changes
/// only the a instance, which keeps its `observedAt`
/// Clause: 4.5.5.1, 5.5.12, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9020_a_sub_attribute_merges_only_into_the_instance_of_its_dataset() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_matching_id = namespace.urn("Vehicle", "matching")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let a_dataset = namespace.dataset_id("Dataset", "a")?;
    let b_dataset = namespace.dataset_id("Dataset", "b")?;
    let vehicle_matching = Entity::builder()
        .id(vehicle_matching_id.clone())
        .entity_type(vehicle_type)
        .property(
            "speed",
            Property::builder(10)
                .property(
                    "accuracy",
                    Property::builder(Value::Number("0.1".parse()?))
                        .dataset_id(namespace.dataset_id("Dataset", "a")?)
                        .observed_at("2020-01-01T00:00:00Z".parse()?),
                )
                .property(
                    "accuracy",
                    Property::builder(Value::Number("0.5".parse()?)).dataset_id(namespace.dataset_id("Dataset", "b")?),
                ),
        )
        .build()?;
    broker.entities().create(&vehicle_matching).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property(
            "speed",
            Property::builder(20).property(
                "accuracy",
                Property::builder(Value::Number("0.2".parse()?)).dataset_id(namespace.dataset_id("Dataset", "a")?),
            ),
        )
        .build()?;
    broker.entities().merge(&vehicle_matching_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_matching_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "speed",
            any_attribute()
                .sub_attribute_instances("accuracy", 2)
                .sub_attribute(
                    "accuracy",
                    any_attribute()
                        .dataset_id(a_dataset)
                        .value(Value::Number("0.2".parse()?))
                        .observed_at("2020-01-01T00:00:00Z".parse()?)
                )
                .sub_attribute("accuracy", any_attribute().dataset_id(b_dataset).value(Value::Number("0.5".parse()?)))
        )
    );
    Ok(())
}
