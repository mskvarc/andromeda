//! A partial attribute update of one dataset records only that instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_instance, has_instances},
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, NgsiLdMediaType, Property},
    query::{Locality, q, temporal_query::TemporalProperty},
};

/// Pins: Partial Attribute Update of speed with value 3 and `datasetId` odometer, over speed instances gps 1 and
/// odometer 2, answers 204; the Entity holds gps 1 and odometer 3, the Temporal Evolution holds one gps instance
/// and two odometer instances, and q selects speed==3
/// Clause: 5.6.4.4, 4.5.5, 4.5.7, 4.3.1, 4.9
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9005_a_partial_attribute_update_of_one_dataset_records_only_that_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_partial_dataset_id = namespace.urn("Vehicle", "partial-dataset")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let gps = namespace.dataset_id("Dataset", "gps")?;
    let odometer = namespace.dataset_id("Dataset", "odometer")?;
    let vehicle_partial_dataset = Entity::builder()
        .id(vehicle_partial_dataset_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(1).dataset_id(gps.clone()))
        .property("speed", Property::builder(2).dataset_id(odometer.clone()))
        .build()?;
    broker.entities().create(&vehicle_partial_dataset).send().await?.done()?;
    let patch = Attribute::from(Property::builder(3).dataset_id(odometer.clone()).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&vehicle_partial_dataset_id, &"speed".parse::<AttributeName>()?, patch)
        .send()
        .await?
        .done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json.entities().retrieve(&vehicle_partial_dataset_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(entity, has_instance("speed", any_attribute().dataset_id(gps.clone()).value(1)));
    expect_that!(entity, has_instance("speed", any_attribute().dataset_id(odometer.clone()).value(3)));
    let retrieved_2 = json
        .temporal()
        .retrieve(&vehicle_partial_dataset_id)
        .time_property(TemporalProperty::ModifiedAt)
        .attrs(vec!["speed".parse()?])
        .send()
        .await?;
    expect_that!(
        retrieved_2.body(),
        has_evolution(
            "speed",
            all!(
                contains(any_attribute().with_dataset_id(gps)).times(eq(1)),
                contains(any_attribute().with_dataset_id(odometer)).times(eq(2))
            )
        )
    );
    let page = json
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .locality(Locality::Local)
        .q(q::attr("speed").eq(3))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&vehicle_partial_dataset_id)));
    Ok(())
}
