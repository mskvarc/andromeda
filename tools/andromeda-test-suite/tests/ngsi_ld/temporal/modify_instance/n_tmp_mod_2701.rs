//! An empty fragment is bad request data and changes nothing.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_at},
    instance_time::instance_time_at,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
    system_time::SystemMember,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{ModifyAttributeInstance, SystemAttributes},
    },
    model::{Attribute, AttributeName, ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Modify Attribute instance with the empty Fragment {} as application/json answers 400 `BadRequestData` and
/// the instance keeps its value and `modifiedAt`
/// Clause: 5.6.14.3, 5.6.14.4, Table 5.5.2-1
/// Recorded: 5.6.14.3 takes "an NGSI-LD Fragment of `EntityTemporal`, including only the new Attribute instance,
/// contained by an Array of exactly one item", which {} does not hold, and Table 5.5.2-1 answers input that does
/// not meet the operation's requirements with `BadRequestData`
#[gtest]
#[tokio::test]
async fn n_tmp_mod_2701_an_empty_fragment_is_bad_request_data_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json
        .temporal()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let instance_id = instance_id_at(retrieved.body(), &"speed".parse()?, 0)?;
    let modified = instance_time_at(retrieved.body(), &"speed".parse()?, 0, SystemMember::ModifiedAt)?;
    // The empty fragment is no attribute the model writes, so the typed request is sent with `{}` as its body.
    let placeholder = Attribute::from(Property::builder(129).build()?);
    let request = broker
        .unchecked(
            &ModifyAttributeInstance::builder()
                .id(&vehicle_id)
                .name(&"speed".parse::<AttributeName>()?)
                .instance_id(&instance_id)
                .instance(&placeholder)
                .build(),
        )?
        .body(Bytes::from_static(b"{}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_2 = json
        .temporal()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        evolution,
        has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(120).instance_id(&instance_id))))
    );
    expect_that!(instance_time_at(evolution, &"speed".parse()?, 0, SystemMember::ModifiedAt)?, eq(modified));
    Ok(())
}
