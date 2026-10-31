//! Instance ids a client supplied are handed back and address their instances.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_where},
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, InstanceId, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Create Temporal Evolution of speed with four instances, one without instanceId and three with a URN, a URL and
/// a URN in the form of a generated one with a suffix, reads back each client instanceId unchanged and a generated one
/// for the first; deleting the generated one and the URN answers 204 each and leaves the URL and suffixed instances
/// Clause: 4.5.7, 4.5.2, 5.6.11, 5.6.15
/// Recorded: clause 4.5.2 gives an instance an instanceId uniquely identifying it and leaves open whether a broker keeps
/// one a client supplied; Andromeda keeps it as written
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9036_instance_ids_a_client_supplied_are_handed_back_and_address_their_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_instance_identity_id = namespace.urn("Vehicle", "instance-identity")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let speed = "speed".parse::<AttributeName>()?;
    let own = format!("urn:ngsi-ld:my-own:instance:{namespace}-42").parse::<InstanceId>()?;
    let url = format!("https://example.org/{namespace}/instances/7").parse::<InstanceId>()?;
    let suffixed = format!("urn:ngsi-ld:attribute:instance:01942f6c-7f00-7000-8000-000000000001:{namespace}").parse::<InstanceId>()?;
    let vehicle_instance_identity_evolution = TemporalEntity::builder()
        .id(vehicle_instance_identity_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(1).observed_at("2021-01-01T00:00:00Z".parse()?))
        .identified("speed", own.clone(), Property::builder(2).observed_at("2021-01-02T00:00:00Z".parse()?))
        .identified("speed", url.clone(), Property::builder(3).observed_at("2021-01-03T00:00:00Z".parse()?))
        .identified("speed", suffixed.clone(), Property::builder(4).observed_at("2021-01-04T00:00:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&vehicle_instance_identity_evolution)
        .send()
        .await?
        .created()?;
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = client.temporal().retrieve(&vehicle_instance_identity_id).send().await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", len(eq(4))));
    expect_that!(instance_id_where(evolution, &"speed".parse()?, any_attribute().value(2))?, eq(&own));
    expect_that!(instance_id_where(evolution, &"speed".parse()?, any_attribute().value(3))?, eq(&url));
    expect_that!(instance_id_where(evolution, &"speed".parse()?, any_attribute().value(4))?, eq(&suffixed));
    let generated = instance_id_where(evolution, &"speed".parse()?, any_attribute().value(1))?;
    client
        .temporal()
        .delete_instance(&vehicle_instance_identity_id, &speed, &generated)
        .send()
        .await?;
    client.temporal().delete_instance(&vehicle_instance_identity_id, &speed, &own).send().await?;
    let retrieved_2 = client.temporal().retrieve(&vehicle_instance_identity_id).send().await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, has_evolution("speed", len(eq(2))));
    expect_that!(instance_id_where(evolution_2, &"speed".parse()?, any_attribute().value(3))?, eq(&url));
    expect_that!(instance_id_where(evolution_2, &"speed".parse()?, any_attribute().value(4))?, eq(&suffixed));
    Ok(())
}
