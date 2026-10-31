//! Replacing a client instance on a long stream leaves every other instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{InstanceId, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: two Meters each hold a reading instance under the client `instanceId` -a-7, one among twelve instances;
/// adding a reading of 700 under that `instanceId` to the first replaces its instance alone, so the first still holds
/// twelve instances with 700 under -a-7 and its neighbours unchanged, and the second still holds 500 under -a-7
/// Clause: 4.5.7, 5.6.12.4, 5.6.14
/// Recorded: an instance written under a client `instanceId` the attribute of that Entity already holds replaces that
/// instance; the identifier names one instance per Entity and attribute, so the same identifier on another Entity is
/// another instance
#[gtest]
#[tokio::test]
async fn o_tmp_add_9376_replacing_a_client_instance_on_a_long_stream_leaves_every_other_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let meter_a_id = namespace.urn("Meter", "a")?;
    let meter_type = namespace.entity_type("Meter")?;
    let meter_b_id = namespace.urn("Meter", "b")?;
    let instance_id = |n: u32| format!("urn:ngsi-ld:Instance:{namespace}-a-{n}").parse::<InstanceId>();
    let mut builder = TemporalEntity::builder().id(meter_a_id.clone()).entity_type(meter_type.clone());
    for n in 1..=12_u32 {
        builder = builder.identified(
            "reading",
            instance_id(n)?,
            Property::builder(n).observed_at(format!("2024-01-01T{n:02}:00:00Z").parse()?),
        );
    }
    let meter_a = builder.build()?;
    broker.temporal().create_or_update(&meter_a).send().await?.created()?;
    let meter_b = TemporalEntity::builder()
        .id(meter_b_id.clone())
        .entity_type(meter_type.clone())
        .identified("reading", instance_id(7)?, Property::builder(500).observed_at("2024-01-01T07:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&meter_b).send().await?.created()?;
    let replacement = TemporalEntity::builder()
        .id(meter_a_id.clone())
        .entity_type(meter_type.clone())
        .identified("reading", instance_id(7)?, Property::builder(700).observed_at("2024-01-01T07:00:00Z".parse()?))
        .build()?
        .attributes;
    let added = broker.with_responses().temporal().add_attributes(&meter_a_id, &replacement).send().await;
    expect_that!(&added, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&meter_a_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "reading",
            all!(
                len(eq(12)),
                contains(any_attribute().instance_id(&instance_id(7)?).value(700)).times(eq(1)),
                contains(any_attribute().instance_id(&instance_id(6)?).value(6)),
                contains(any_attribute().instance_id(&instance_id(8)?).value(8)),
                not(contains(any_attribute().value(7)))
            )
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&meter_b_id)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution("reading", all!(len(eq(1)), contains(any_attribute().instance_id(&instance_id(7)?).value(500))))
    );
    Ok(())
}
